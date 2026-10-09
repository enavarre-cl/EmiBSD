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
//! `xtask smoke2`: two EmiBSD VMs of the same arch, booted concurrently on a private link.
//!
//! M9b (WireGuard) and M9c (IPsec ESP) need a tunnel between two machines. Each VM keeps its
//! `vio0` on QEMU's user-mode network and gets a second virtio-net NIC, `vio1`, on a link that
//! only the two of them share (`boot::VmLink`: a pair of localhost UDP sockets, QEMU's `dgram`
//! netdev). Every VM has its own image and its own EDK2 variable store (`-a` / `-b` file
//! suffixes), so the two QEMUs never write to the same file.
//!
//! Each VM runs its own script, the same `send-after`/`send`/`expect` machinery as `smoke
//! --until-seen`:
//!
//! ```text
//! cargo xtask smoke2 --arch A [--kernel K] [--cmdline C] [--timeout SECS] [--show-transcripts]
//!     [--both-send-after L --both-send T]... [--both-expect L]...
//!     [--a-send-after L --a-send T]... [--a-expect L]...
//!     [--b-send-after L --b-send T]... [--b-expect L]... [--reject L]...
//! ```
//!
//! The `--both-*` lines are put in front of each VM's own (a shared login, then one command per
//! VM). The run passes once both VMs have sent everything and seen everything they expect; a
//! VM that is done keeps running until the other one is, since the other may still be talking
//! to it. A VM that is not done after the timeout, or whose QEMU exits early, fails the run;
//! both transcripts are printed then. A `--reject` line in either transcript fails the run too.

use std::net::UdpSocket;
use std::path::Path;
use std::process::{Child, ChildStdin, Stdio};
use std::sync::{Arc, Mutex};
use std::thread::{self, JoinHandle};
use std::time::{Duration, Instant};

use crate::boot::{self, Arch, VmLink};
use crate::{Result, flags, optional_flag};

/// What one VM does: the sends, in order, and the lines it must show.
#[derive(Clone, Debug, PartialEq, Eq, Default)]
pub struct Script {
    /// `(after this line, send this text)`; each trigger is looked for only after the
    /// previous text was sent.
    sends: Vec<(String, String)>,
    /// Serial lines that must appear, anywhere in the transcript.
    expects: Vec<String>,
    /// How many of `sends` went out.
    next_send: usize,
    /// Where in the transcript the next trigger is looked for.
    search_from: usize,
}

impl Script {
    /// A script that has sent nothing yet.
    pub fn new(sends: Vec<(String, String)>, expects: Vec<String>) -> Self {
        Self {
            sends,
            expects,
            next_send: 0,
            search_from: 0,
        }
    }

    /// Looks at the transcript so far; returns the text to send now, if the next trigger line
    /// has appeared. At most one text per call, as in `smoke`.
    pub fn step(&mut self, transcript: &[u8]) -> Option<String> {
        let (after, text) = self.sends.get(self.next_send)?;
        let from = self.search_from.min(transcript.len());
        let seen = String::from_utf8_lossy(&transcript[from..]).contains(after.as_str());
        if !seen {
            return None;
        }
        let text = text.clone();
        self.search_from = transcript.len();
        self.next_send += 1;
        Some(text)
    }

    /// The expected lines not in the transcript.
    pub fn missing(&self, transcript: &[u8]) -> Vec<&str> {
        let s = String::from_utf8_lossy(transcript);
        self.expects
            .iter()
            .map(String::as_str)
            .filter(|e| !s.contains(e))
            .collect()
    }

    /// Whether every text was sent and every expected line seen.
    pub fn done(&self, transcript: &[u8]) -> bool {
        self.next_send == self.sends.len() && self.missing(transcript).is_empty()
    }

    /// The trigger the script waits for, if it has sends left (for the failure report).
    pub fn waiting_for(&self) -> Option<&str> {
        self.sends.get(self.next_send).map(|(a, _)| a.as_str())
    }
}

/// The scripts of both VMs and the run's options, as the command line gave them.
#[derive(Debug, PartialEq, Eq)]
pub struct Plan {
    /// VM A's script.
    pub a: Script,
    /// VM B's script.
    pub b: Script,
    /// Per-VM time limit.
    pub timeout: Duration,
    /// Print both transcripts on success too.
    pub show_transcripts: bool,
    /// `--reject`: lines that fail the run if either VM prints them.
    pub rejects: Vec<String>,
    /// `--disk-fresh`: delete and recreate both persistent disks before booting.
    pub disk_fresh: bool,
    /// `--disks N`: persistent disks per VM (1 to `boot::MAX_DISKS`).
    pub disks: usize,
}

/// The `--<who>-send-after`/`--<who>-send` pairs and `--<who>-expect` lines, with the
/// `--both-*` ones first.
fn script_for(args: &[&str], who: &str) -> Result<Script> {
    let mut sends: Vec<(String, String)> = Vec::new();
    let mut expects: Vec<String> = Vec::new();
    for prefix in ["both", who] {
        let afters = flags(args, &format!("--{prefix}-send-after"));
        let texts = flags(args, &format!("--{prefix}-send"));
        if afters.len() != texts.len() {
            return Err(format!("--{prefix}-send-after and --{prefix}-send go together").into());
        }
        sends.extend(
            afters
                .iter()
                .zip(&texts)
                .map(|(a, t)| ((*a).to_string(), t.replace("\\n", "\n"))),
        );
        expects.extend(
            flags(args, &format!("--{prefix}-expect"))
                .into_iter()
                .map(str::to_string),
        );
    }
    Ok(Script::new(sends, expects))
}

/// Parses the scripts and options of `smoke2` out of `args`.
pub fn parse_plan(args: &[&str]) -> Result<Plan> {
    let timeout = match optional_flag(args, "--timeout") {
        Some(s) => Duration::from_secs(
            s.parse::<u64>()
                .map_err(|e| format!("--timeout {s}: {e}"))?,
        ),
        None => boot::SMOKE_TIMEOUT,
    };
    Ok(Plan {
        a: script_for(args, "a")?,
        b: script_for(args, "b")?,
        timeout,
        show_transcripts: args.contains(&"--show-transcripts"),
        rejects: flags(args, "--reject")
            .into_iter()
            .map(str::to_string)
            .collect(),
        disk_fresh: args.contains(&"--disk-fresh"),
        disks: crate::disks_flag(args)?,
    })
}

/// Two distinct free UDP ports on localhost (bound and released: QEMU binds them next).
fn free_udp_ports() -> Result<(u16, u16)> {
    let one = UdpSocket::bind("127.0.0.1:0").map_err(|e| format!("udp bind: {e}"))?;
    let two = UdpSocket::bind("127.0.0.1:0").map_err(|e| format!("udp bind: {e}"))?;
    let port = |s: &UdpSocket| s.local_addr().map(|a| a.port());
    Ok((
        port(&one).map_err(|e| format!("udp port: {e}"))?,
        port(&two).map_err(|e| format!("udp port: {e}"))?,
    ))
}

/// The link of VM `index` (0 = A, 1 = B) given the two ports: its own MACs, its port first.
fn vm_link(index: usize, ports: (u16, u16)) -> VmLink {
    let (tag, local_port, remote_port) = if index == 0 {
        ("a", ports.0, ports.1)
    } else {
        ("b", ports.1, ports.0)
    };
    VmLink {
        tag,
        user_mac: format!("52:54:00:aa:00:0{}", index + 1),
        link_mac: format!("52:54:00:bb:00:0{}", index + 1),
        local_port,
        remote_port,
    }
}

/// One running QEMU and what it printed.
struct Vm {
    tag: &'static str,
    child: Child,
    stdin: Option<ChildStdin>,
    transcript: Arc<Mutex<Vec<u8>>>,
    out_reader: Option<JoinHandle<()>>,
    err_reader: Option<JoinHandle<Vec<u8>>>,
    script: Script,
    /// Seconds into the run when the script was satisfied.
    done_at: Option<f32>,
    /// The QEMU exit status, if it exited before the script was satisfied.
    exited: Option<Option<i32>>,
}

impl Vm {
    fn snapshot(&self) -> Vec<u8> {
        self.transcript
            .lock()
            .map(|t| t.clone())
            .unwrap_or_default()
    }

    fn report(&self, serial: &str, diagnostics: &str) {
        println!("----- serial transcript (vm {}) -----", self.tag);
        print!("{serial}");
        if !serial.ends_with('\n') {
            println!();
        }
        if !diagnostics.trim().is_empty() {
            println!("----- qemu stderr (vm {}) -----", self.tag);
            print!("{diagnostics}");
        }
    }
}

/// Boots two VMs of `arch` from `kernel` (or the images already built when `None`) and
/// runs `plan` on them.
pub fn smoke2(
    root: &Path,
    arch: Arch,
    kernel: Option<&Path>,
    cmdline: Option<&str>,
    init: Option<&Path>,
    ramdisk: Option<&Path>,
    plan: Plan,
) -> Result<()> {
    let boot = Boot {
        root,
        arch,
        kernel,
        cmdline,
        init,
        ramdisk,
    };
    let mut attempt = 1;
    loop {
        let ports = free_udp_ports()?;
        match run(&boot, &plan, ports, attempt == LINK_ATTEMPTS)? {
            Outcome::Done => return Ok(()),
            Outcome::LinkPortTaken => {
                println!(
                    "xtask: smoke2 {}: a link port ({} or {}) was taken before QEMU bound it; \
                     booting again on two new ports",
                    arch.name(),
                    ports.0,
                    ports.1
                );
                attempt += 1;
            }
        }
    }
}

/// How many times [`smoke2`] boots its two VMs when QEMU finds a link port taken.
/// [`free_udp_ports`] releases the ports it found before QEMU binds them, so another process
/// (another `smoke2` of `smoke-all`, or any program asking for an ephemeral port) may take one
/// in between; QEMU then exits at once with `Address already in use`.
const LINK_ATTEMPTS: u32 = 3;

/// What QEMU prints when a `dgram` netdev cannot bind its local port.
const PORT_TAKEN: &str = "Address already in use";

/// The boot arguments of a [`smoke2`] run, the same for every attempt.
struct Boot<'a> {
    root: &'a Path,
    arch: Arch,
    kernel: Option<&'a Path>,
    cmdline: Option<&'a str>,
    init: Option<&'a Path>,
    ramdisk: Option<&'a Path>,
}

/// How one attempt of [`smoke2`] ended, when it did not fail.
enum Outcome {
    /// Both scripts were done: the run passed.
    Done,
    /// A VM's QEMU could not bind its link port; the run is to be tried again.
    LinkPortTaken,
}

/// One attempt of [`smoke2`] on the link `ports`. Unless it is the `last` attempt, a failure
/// caused by a taken link port is [`Outcome::LinkPortTaken`] instead of an error.
fn run(boot: &Boot<'_>, plan: &Plan, ports: (u16, u16), last: bool) -> Result<Outcome> {
    let Boot {
        root,
        arch,
        kernel,
        cmdline,
        init,
        ramdisk,
    } = *boot;
    let started = Instant::now();
    let limit = boot::time_limit(plan.timeout);
    let mut vms: Vec<Vm> = Vec::new();
    for (index, script) in [plan.a.clone(), plan.b.clone()].into_iter().enumerate() {
        let link = vm_link(index, ports);
        let image = match kernel {
            Some(k) => boot::image_tagged(root, arch, Some(link.tag), k, cmdline, init, ramdisk)?,
            None => {
                let p = boot::image_path(root, arch, Some(link.tag));
                if !p.is_file() {
                    kill_all(&mut vms);
                    return Err(format!("{} does not exist; pass --kernel", p.display()).into());
                }
                p
            }
        };
        let mut cmd = boot::qemu_command(
            root,
            arch,
            &image,
            "stdio",
            Some(&link),
            &boot::Disks {
                fresh: plan.disk_fresh,
                count: plan.disks,
                set: None,
            },
        )?;
        cmd.stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped());
        println!("xtask: vm {}: {}", link.tag, boot::command_line(&cmd));
        let mut child = match cmd.spawn() {
            Ok(c) => c,
            Err(e) => {
                kill_all(&mut vms);
                return Err(boot::spawn_error(arch, &e).into());
            }
        };
        let stdout = child.stdout.take().ok_or("qemu stdout is not a pipe")?;
        let stderr = child.stderr.take().ok_or("qemu stderr is not a pipe")?;
        let transcript: Arc<Mutex<Vec<u8>>> = Arc::new(Mutex::new(Vec::new()));
        let out_reader = {
            let transcript = Arc::clone(&transcript);
            thread::spawn(move || boot::slurp_into(stdout, &transcript))
        };
        let err_reader = thread::spawn(move || boot::slurp(stderr));
        vms.push(Vm {
            tag: link.tag,
            stdin: child.stdin.take(),
            child,
            transcript,
            out_reader: Some(out_reader),
            err_reader: Some(err_reader),
            script,
            done_at: None,
            exited: None,
        });
    }

    // Poll both VMs until both are done, one failed or the time is up.
    let mut failure: Option<String> = None;
    loop {
        for vm in &mut vms {
            if vm.done_at.is_some() {
                continue;
            }
            let t = vm.snapshot();
            if let (Some(text), Some(stdin)) = (vm.script.step(&t), vm.stdin.as_mut()) {
                let sent = boot::send_paced(stdin, &text);
                if let Err(e) = sent {
                    failure = Some(format!("vm {}: writing to the serial console: {e}", vm.tag));
                }
                println!(
                    "xtask: vm {}: sent {text:?} after {:.1}s",
                    vm.tag,
                    started.elapsed().as_secs_f32()
                );
            }
            if vm.script.done(&t) {
                vm.done_at = Some(started.elapsed().as_secs_f32());
            } else if let Ok(Some(status)) = vm.child.try_wait() {
                vm.exited = Some(status.code());
                failure = Some(format!(
                    "vm {}: qemu exited with status {:?} before its script was done",
                    vm.tag,
                    status.code()
                ));
            }
        }
        if failure.is_some() || vms.iter().all(|v| v.done_at.is_some()) {
            break;
        }
        if started.elapsed() > limit {
            let late: Vec<String> = vms
                .iter()
                .filter(|v| v.done_at.is_none())
                .map(|v| v.tag.to_string())
                .collect();
            failure = Some(format!(
                "vm {}: timed out after {:.0}s",
                late.join(" and vm "),
                started.elapsed().as_secs_f32()
            ));
            break;
        }
        thread::sleep(Duration::from_millis(100));
    }

    kill_all(&mut vms);
    let elapsed = started.elapsed().as_secs_f32();
    let rejects: Vec<&str> = plan.rejects.iter().map(String::as_str).collect();
    let mut outputs: Vec<(String, String)> = Vec::new();
    for vm in &mut vms {
        drop(vm.stdin.take());
        if let Some(r) = vm.out_reader.take() {
            r.join().ok();
        }
        let diagnostics = vm
            .err_reader
            .take()
            .and_then(|r| r.join().ok())
            .map(|b| String::from_utf8_lossy(&b).into_owned())
            .unwrap_or_default();
        let serial = String::from_utf8_lossy(&vm.snapshot()).into_owned();
        let seen = boot::rejected(&serial, &rejects);
        if !seen.is_empty() && failure.is_none() {
            failure = Some(format!(
                "vm {}: REJECTED line(s) seen: {}",
                vm.tag,
                seen.join(" | ")
            ));
        }
        outputs.push((serial, diagnostics));
    }
    if failure.is_some() && !last && outputs.iter().any(|(_, d)| d.contains(PORT_TAKEN)) {
        for (vm, (serial, diagnostics)) in vms.iter().zip(&outputs) {
            vm.report(serial, diagnostics);
        }
        return Ok(Outcome::LinkPortTaken);
    }
    let mut summary: Vec<String> = Vec::new();
    for (vm, (serial, diagnostics)) in vms.iter().zip(&outputs) {
        let t = serial.as_bytes();
        let missing = vm.script.missing(t);
        if failure.is_some() || plan.show_transcripts {
            vm.report(serial, diagnostics);
        }
        summary.push(match vm.done_at {
            Some(at) => format!("vm {}: done at {at:.1}s", vm.tag),
            None => format!(
                "vm {}: NOT done; waiting for {:?}, NOT seen: {}",
                vm.tag,
                vm.script.waiting_for().unwrap_or("(nothing)"),
                if missing.is_empty() {
                    "(nothing)".to_string()
                } else {
                    missing.join(" | ")
                }
            ),
        });
    }
    println!("-----");
    match failure {
        None => {
            println!(
                "smoke2 {}: ok in {elapsed:.1}s ({})",
                arch.name(),
                summary.join("; ")
            );
            Ok(Outcome::Done)
        }
        Some(why) => Err(format!("smoke2 {}: {why}; {}", arch.name(), summary.join("; ")).into()),
    }
}

fn kill_all(vms: &mut [Vm]) {
    for vm in vms {
        let _ = vm.child.kill();
        let _ = vm.child.wait();
    }
}
/* </CODE> */

/* <TESTS> */
#[cfg(test)]
mod tests {
    use super::*;

    fn script(sends: &[(&str, &str)], expects: &[&str]) -> Script {
        Script::new(
            sends
                .iter()
                .map(|(a, t)| ((*a).to_string(), (*t).to_string()))
                .collect(),
            expects.iter().map(|e| (*e).to_string()).collect(),
        )
    }

    #[test]
    fn script_sends_each_text_after_its_trigger_in_order() {
        let mut s = script(&[("login:", "root\n"), ("Password:", "pw\n")], &["# "]);
        assert_eq!(s.step(b"booting\n"), None);
        assert_eq!(s.step(b"booting\nlogin:"), Some("root\n".to_string()));
        // The first trigger is behind us: it does not satisfy the second pair.
        assert_eq!(s.step(b"booting\nlogin:"), None);
        assert_eq!(s.waiting_for(), Some("Password:"));
        assert_eq!(
            s.step(b"booting\nlogin: root\nPassword:"),
            Some("pw\n".to_string())
        );
        assert_eq!(s.waiting_for(), None);
        assert!(!s.done(b"login: root\nPassword:"));
        assert!(s.done(b"login: root\nPassword:\n# "));
    }

    #[test]
    fn script_trigger_must_come_after_the_previous_send() {
        let mut s = script(&[("# ", "a\n"), ("# ", "b\n")], &[]);
        assert_eq!(s.step(b"# "), Some("a\n".to_string()));
        // The same prompt again would be the old one.
        assert_eq!(s.step(b"# "), None);
        assert_eq!(s.step(b"# a\n# "), Some("b\n".to_string()));
    }

    #[test]
    fn script_without_sends_is_done_when_the_lines_are_seen() {
        let s = script(&[], &["one", "two"]);
        assert_eq!(s.missing(b"one"), vec!["two"]);
        assert!(!s.done(b"one"));
        assert!(s.done(b"two\none"));
        assert!(script(&[], &[]).done(b""));
    }

    #[test]
    fn script_search_offset_survives_a_shorter_transcript() {
        let mut s = script(&[("a", "1"), ("b", "2")], &[]);
        assert_eq!(s.step(b"xxxa"), Some("1".to_string()));
        assert_eq!(s.step(b""), None);
    }

    #[test]
    fn plan_puts_the_shared_lines_first_and_unescapes_newlines() {
        let args = [
            "--arch",
            "amd64",
            "--a-send-after",
            "# ",
            "--a-send",
            "ifconfig vio1 inet 192.168.77.1/24\\n",
            "--both-send-after",
            "login:",
            "--both-send",
            "root\\n",
            "--both-expect",
            "rc: multi-user",
            "--b-expect",
            "vio1:",
        ];
        let plan = parse_plan(&args).unwrap();
        assert_eq!(
            plan.a,
            script(
                &[
                    ("login:", "root\n"),
                    ("# ", "ifconfig vio1 inet 192.168.77.1/24\n")
                ],
                &["rc: multi-user"]
            )
        );
        assert_eq!(
            plan.b,
            script(&[("login:", "root\n")], &["rc: multi-user", "vio1:"])
        );
        assert_eq!(plan.timeout, boot::SMOKE_TIMEOUT);
        assert!(!plan.show_transcripts);
    }

    #[test]
    fn plan_reads_timeout_and_transcript_flags() {
        let plan = parse_plan(&["--timeout", "42", "--show-transcripts"]).unwrap();
        assert_eq!(plan.timeout, Duration::from_secs(42));
        assert!(plan.show_transcripts);
        assert!(parse_plan(&["--timeout", "soon"]).is_err());
    }

    #[test]
    fn plan_reads_reject_lines() {
        let plan = parse_plan(&[
            "--reject",
            "uptime went backwards",
            "--a-expect",
            "x",
            "--reject",
            "panic:",
        ])
        .unwrap();
        assert_eq!(plan.rejects, vec!["uptime went backwards", "panic:"]);
        assert!(parse_plan(&[]).unwrap().rejects.is_empty());
    }

    #[test]
    fn plan_takes_the_disk_count() {
        assert_eq!(parse_plan(&[]).unwrap().disks, 1);
        assert_eq!(parse_plan(&["--disks", "3"]).unwrap().disks, 3);
        assert!(parse_plan(&["--disks", "9"]).is_err());
    }

    #[test]
    fn plan_rejects_unpaired_sends() {
        assert!(parse_plan(&["--a-send-after", "x"]).is_err());
        assert!(parse_plan(&["--b-send", "x"]).is_err());
        assert!(parse_plan(&["--both-send-after", "x"]).is_err());
    }

    #[test]
    fn links_are_mirrored_and_macs_distinct() {
        let a = vm_link(0, (1111, 2222));
        let b = vm_link(1, (1111, 2222));
        assert_eq!((a.tag, b.tag), ("a", "b"));
        assert_eq!((a.local_port, a.remote_port), (1111, 2222));
        assert_eq!((b.local_port, b.remote_port), (2222, 1111));
        let macs = [&a.user_mac, &a.link_mac, &b.user_mac, &b.link_mac];
        for (i, x) in macs.iter().enumerate() {
            for y in &macs[i + 1..] {
                assert_ne!(x, y);
            }
        }
        // A unicast, locally assigned QEMU-prefix MAC.
        assert!(a.link_mac.starts_with("52:54:00:"));
    }

    #[test]
    fn netdev_is_a_dgram_pair_on_localhost() {
        let a = vm_link(0, (1111, 2222));
        assert_eq!(
            a.netdev(),
            "dgram,id=n1,local.type=inet,local.host=127.0.0.1,local.port=1111,\
             remote.type=inet,remote.host=127.0.0.1,remote.port=2222"
        );
    }

    #[test]
    fn free_ports_are_distinct() {
        let (x, y) = free_udp_ports().unwrap();
        assert_ne!(x, y);
        assert_ne!(x, 0);
    }
}
/* </TESTS> */
