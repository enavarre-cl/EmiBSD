/* <LICENSES> */
/*
 * Copyright (c) 2026 Emilio Navarrete Lineros <enavarre@outlook.com>
 *
 * Permission to use, copy, modify, and distribute this software for any
 * purpose with or without fee is hereby granted, provided that the above
 * copyright notice and this permission notice appear in all copies.
 *
 * THE SOFTWARE IS PROVIDED "AS IS" AND THE AUTHOR DISCLAIMS ALL WARRANTIES
 * WITH REGARD TO THIS SOFTWARE INCLUDING ALL IMPLIED WARRANTIES OF
 * MERCHANTABILITY AND FITNESS. IN NO EVENT SHALL THE AUTHOR BE LIABLE FOR
 * ANY SPECIAL, DIRECT, INDIRECT, OR CONSEQUENTIAL DAMAGES OR ANY DAMAGES
 * WHATSOEVER RESULTING FROM LOSS OF USE, DATA OR PROFITS, WHETHER IN AN
 * ACTION OF CONTRACT, NEGLIGENCE OR OTHER TORTIOUS ACTION, ARISING OUT OF
 * OR IN CONNECTION WITH THE USE OR PERFORMANCE OF THIS SOFTWARE.
 */
/* </LICENSES> */

/* <CODE> */
//! A VM driven over its serial console: QEMU with the serial port on stdio, a reader thread
//! collecting the transcript, and send/expect on top (the smoke machinery's `send_paced`
//! and `slurp_into`, `boot.rs`).

use std::fs;
use std::path::PathBuf;
use std::process::{Child, ChildStdin, Command, Stdio};
use std::sync::{Arc, Mutex};
use std::thread::{self, JoinHandle};
use std::time::{Duration, Instant};

use crate::Result;
use crate::boot;

/// A running VM.
pub(crate) struct Vm {
    /// For messages: `openbsd-amd64`, `emibsd-arm64`.
    pub(crate) name: String,
    child: Child,
    stdin: Option<ChildStdin>,
    transcript: Arc<Mutex<Vec<u8>>>,
    reader: Option<JoinHandle<()>>,
    /// Where in the transcript the next `wait_for` starts looking.
    cursor: usize,
    /// The transcript is written here every few seconds and when the VM is dropped.
    log: PathBuf,
    last_flush: Instant,
    heartbeat: boot::Heartbeat,
}

/// What [`Vm::respond`] stops on.
pub(crate) enum Stop {
    /// QEMU exited.
    Exited,
}

impl Vm {
    /// Starts `cmd` (a QEMU command line with `-serial stdio`).
    pub(crate) fn spawn(name: &str, mut cmd: Command, log: PathBuf) -> Result<Vm> {
        cmd.stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped());
        println!("xtask: {name}: {}", boot::command_line(&cmd));
        let mut child = cmd
            .spawn()
            .map_err(|e| format!("{name}: {}: {e}", cmd.get_program().to_string_lossy()))?;
        let stdout = child.stdout.take().ok_or("qemu stdout is not a pipe")?;
        let stderr = child.stderr.take().ok_or("qemu stderr is not a pipe")?;
        let stdin = child.stdin.take();
        let transcript: Arc<Mutex<Vec<u8>>> = Arc::new(Mutex::new(Vec::new()));
        let reader = {
            let t = Arc::clone(&transcript);
            thread::spawn(move || boot::slurp_into(stdout, &t))
        };
        // QEMU's own complaints go into the transcript too, marked.
        {
            let t = Arc::clone(&transcript);
            thread::spawn(move || {
                let err = boot::slurp(stderr);
                if !err.is_empty()
                    && let Ok(mut t) = t.lock()
                {
                    t.extend_from_slice(b"\n[qemu stderr] ");
                    t.extend_from_slice(&err);
                }
            });
        }
        Ok(Vm {
            name: name.to_string(),
            child,
            stdin,
            transcript,
            reader: Some(reader),
            cursor: 0,
            log,
            last_flush: Instant::now(),
            heartbeat: boot::Heartbeat::new(),
        })
    }

    /// The whole transcript so far.
    pub(crate) fn text(&self) -> String {
        self.transcript
            .lock()
            .map(|t| String::from_utf8_lossy(&t).into_owned())
            .unwrap_or_default()
    }

    fn len(&self) -> usize {
        self.transcript.lock().map(|t| t.len()).unwrap_or(0)
    }

    /// The transcript from byte `from` on.
    fn text_from(&self, from: usize) -> String {
        self.transcript
            .lock()
            .map(|t| String::from_utf8_lossy(&t[from.min(t.len())..]).into_owned())
            .unwrap_or_default()
    }

    /// Types `text` on the console.
    pub(crate) fn send(&mut self, text: &str) -> Result<()> {
        let stdin = self.stdin.as_mut().ok_or("stdin closed")?;
        boot::send_paced(stdin, text).map_err(|e| format!("{}: send: {e}", self.name))?;
        Ok(())
    }

    /// Whether QEMU has exited. (Also refreshes the log file now and then, so a long run
    /// can be followed with `tail -f`, and prints `boot::Heartbeat`'s line once a minute.)
    pub(crate) fn exited(&mut self) -> bool {
        let bytes = self.len();
        self.heartbeat.tick(&self.name, bytes);
        if self.last_flush.elapsed() > Duration::from_secs(5) {
            self.last_flush = Instant::now();
            let _ = fs::write(&self.log, self.text());
        }
        matches!(self.child.try_wait(), Ok(Some(_)))
    }

    /// Waits until `pat` appears after the cursor, then moves the cursor past it. Fails when
    /// QEMU exits first or `limit` passes.
    pub(crate) fn wait_for(&mut self, pat: &str, limit: Duration) -> Result<()> {
        let started = Instant::now();
        loop {
            let from = self.cursor;
            let tail = self.text_from(from);
            if let Some(at) = tail.find(pat) {
                // `tail` is lossy UTF-8; count bytes of the matched prefix in the original.
                self.cursor = from + tail[..at + pat.len()].len();
                return Ok(());
            }
            if self.exited() {
                return Err(format!("{}: QEMU exited before {pat:?} appeared", self.name).into());
            }
            if started.elapsed() > limit {
                return Err(format!(
                    "{}: {pat:?} did not appear in {}s",
                    self.name,
                    limit.as_secs()
                )
                .into());
            }
            thread::sleep(Duration::from_millis(100));
        }
    }

    /// Answers prompts until a stop condition: whenever one of `rules`' texts appears after
    /// the cursor, its answer is typed (each rule as often as its text appears); `fails`'
    /// texts end with an error; the run ends at the first of `stops`.
    pub(crate) fn respond(
        &mut self,
        rules: &[(&str, &str)],
        fails: &[&str],
        stops: &[Stop],
        limit: Duration,
    ) -> Result<()> {
        let started = Instant::now();
        loop {
            let from = self.cursor;
            let tail = self.text_from(from);
            // The earliest of all matches, so prompts are answered in order.
            let mut best: Option<(usize, usize, Option<&str>)> = None;
            for (pat, answer) in rules {
                if let Some(at) = tail.find(pat)
                    && best.is_none_or(|(b, _, _)| at < b)
                {
                    best = Some((at, pat.len(), Some(answer)));
                }
            }
            for f in fails {
                if let Some(at) = tail.find(f) {
                    return Err(format!("{}: saw {f:?} at offset {}", self.name, from + at).into());
                }
            }
            if let Some((at, n, Some(answer))) = best {
                self.cursor = from + tail[..at + n].len();
                println!(
                    "xtask: {}: answering {:?} after {:.0}s",
                    self.name,
                    answer.trim_end(),
                    started.elapsed().as_secs_f32()
                );
                self.send(answer)?;
                continue;
            }
            if self.exited() {
                if stops.iter().any(|s| matches!(s, Stop::Exited)) {
                    return Ok(());
                }
                return Err(format!("{}: QEMU exited", self.name).into());
            }
            if started.elapsed() > limit {
                return Err(format!("{}: no end in {}s", self.name, limit.as_secs()).into());
            }
            thread::sleep(Duration::from_millis(200));
        }
    }

    /// Answers an interactive program over the console, as a person would: like
    /// [`Vm::respond`], but a rule's answers are typed one per appearance of its text, in
    /// turn (the last one again after the list is used up), so a question asked twice can be
    /// answered differently each time. A question no rule knows is an error as soon as the
    /// console has been still for `still` with a question at its end (its last line ends in
    /// `?` or `]`, the installer's `ask` shows its default between brackets), rather than a
    /// wait until `limit`.
    pub(crate) fn respond_in_turn(
        &mut self,
        rules: &[(&str, &[&str])],
        fails: &[&str],
        stops: &[Stop],
        limit: Duration,
        still: Duration,
    ) -> Result<()> {
        let started = Instant::now();
        let mut used = vec![0usize; rules.len()];
        let mut last_len = self.len();
        let mut last_change = Instant::now();
        loop {
            let from = self.cursor;
            let tail = self.text_from(from);
            let mut best: Option<(usize, usize, usize)> = None;
            for (i, (pat, _)) in rules.iter().enumerate() {
                if let Some(at) = tail.find(pat)
                    && best.is_none_or(|(b, _, _)| at < b)
                {
                    best = Some((at, pat.len(), i));
                }
            }
            for f in fails {
                if let Some(at) = tail.find(f) {
                    return Err(format!("{}: saw {f:?} at offset {}", self.name, from + at).into());
                }
            }
            if let Some((at, n, i)) = best {
                self.cursor = from + tail[..at + n].len();
                let answers = rules[i].1;
                let answer = answers
                    .get(used[i])
                    .or(answers.last())
                    .copied()
                    .unwrap_or("\n");
                used[i] += 1;
                println!(
                    "xtask: {}: {:?} -> {:?} after {:.0}s",
                    self.name,
                    rules[i].0,
                    answer.trim_end(),
                    started.elapsed().as_secs_f32()
                );
                self.send(answer)?;
                continue;
            }
            if self.exited() {
                if stops.iter().any(|s| matches!(s, Stop::Exited)) {
                    return Ok(());
                }
                return Err(format!("{}: QEMU exited", self.name).into());
            }
            let len = self.len();
            if len != last_len {
                last_len = len;
                last_change = Instant::now();
            } else if last_change.elapsed() > still {
                let text = self.text_from(from);
                let line = text.lines().last().unwrap_or("").trim_end();
                if line.ends_with('?') || line.ends_with(']') {
                    return Err(
                        format!("{}: a question no answer covers: {line:?}", self.name).into(),
                    );
                }
            }
            if started.elapsed() > limit {
                return Err(format!("{}: no end in {}s", self.name, limit.as_secs()).into());
            }
            thread::sleep(Duration::from_millis(200));
        }
    }

    /// Waits up to `limit` for QEMU to exit on its own.
    pub(crate) fn wait_exit(&mut self, limit: Duration) -> Result<()> {
        let started = Instant::now();
        while !self.exited() {
            if started.elapsed() > limit {
                return Err(
                    format!("{}: still running after {}s", self.name, limit.as_secs()).into(),
                );
            }
            thread::sleep(Duration::from_millis(200));
        }
        Ok(())
    }

    /// The transcript bytes written since the last `mark`.
    pub(crate) fn mark(&self) -> usize {
        self.len()
    }

    /// The transcript from a [`Vm::mark`] on.
    pub(crate) fn since(&self, mark: usize) -> String {
        self.text_from(mark)
    }
}

impl Drop for Vm {
    fn drop(&mut self) {
        drop(self.stdin.take());
        if !self.exited() {
            let _ = self.child.kill();
        }
        let _ = self.child.wait();
        if let Some(r) = self.reader.take() {
            let _ = r.join();
        }
        let _ = fs::write(&self.log, self.text());
    }
}
/* </CODE> */
