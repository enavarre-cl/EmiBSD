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
//! `xtask smoke-all`: the justfile's smoke recipes, several at a time.
//!
//! ```text
//! cargo xtask smoke-all [-j N] [--just PATH] RECIPE...
//! ```
//!
//! `just smoke` builds every kernel the recipes boot first, once and serially (`smoke-build`),
//! then hands the recipe names to this command, which runs `just --no-deps RECIPE` for each,
//! `N` (default 4) at a time. `--no-deps` keeps the recipes from rebuilding while others boot
//! the kernels they would overwrite. Each recipe runs exactly as `just RECIPE` would, with its
//! own expectations; what changes is where its files are and how long its boots may take:
//!
//! - `EMIBSD_RUN_DIR=target/smoke/RECIPE` (`boot::run_dir`): the recipe's boot images, EDK2
//!   variable stores and persistent disks live there, so two recipes never write the same
//!   file. The persistent disks stay between runs, as `target/disk-*.img` do.
//! - `EMIBSD_TIMEOUT_SCALE` (`boot::time_limit`): with more than two recipes at a time every
//!   time limit is multiplied (by `N / 2`, rounded up), since the VMs share the host's cores.
//! - The recipe's output goes to `target/smoke/RECIPE/log`; one line per recipe is printed
//!   when it ends, and the whole log of every failed recipe once all have ended. The command
//!   fails if any recipe failed.
//! - A watchdog outside the recipe (the user's decision of 2026-10-09): every limit above
//!   lives inside the recipe's own xtask process, so a process that hangs before it gets
//!   there (an `xtask smoke` was once frozen in macOS's dynamic loader, `_dyld_start`, for
//!   2 h 30 min, before `main` and so before QEMU) is stopped by nothing. `smoke-all` polls
//!   each recipe once a second and stops it when it outlives its limit, the sum of its boots'
//!   limits (`just --dry-run`: 180 s per `xtask smoke`, a `smoke2`'s `--timeout`) times the
//!   scale, plus [`LIMIT_MARGIN`]; or when its log has not grown for [`QUIET_LIMIT`] (a
//!   waiting boot prints `boot::Heartbeat`'s line once a minute, so a still log means a hung
//!   process). Stopping kills the recipe's whole process tree (`just`, its shell, `cargo`,
//!   xtask, QEMU, swtpm) with SIGKILL, appends the cause to the log, and reports the recipe as
//!   `TIMEOUT`, a failure, while the others go on.
//!
//! The rest of what runs at once was made per run before: `smoke2`'s link ports are free
//! ports asked of the system (and asked again if QEMU finds one taken), its VMs have their
//! own `-a`/`-b` files. The HTTPS test servers' ports are fixed, but only `smoke-https` uses
//! them. Recipes start longest first, by the time each took last (`target/smoke/RECIPE/seconds`),
//! so a long one does not start last.

use std::collections::VecDeque;
use std::fs::{self, File, OpenOptions};
use std::io::Write;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::sync::mpsc;
use std::sync::{Arc, Mutex};
use std::thread;
use std::time::{Duration, Instant};

use crate::Result;
use crate::boot;

/// Recipes run at once unless `-j` says otherwise.
pub const DEFAULT_JOBS: usize = 4;

/// The most recipes `-j` may run at once.
pub const MAX_JOBS: usize = 32;

/// Added to a recipe's summed boot limits before the watchdog stops it: what runs between
/// the boots (`cargo` starting xtask, disk images, `nvme-root`, `e2fsck`).
const LIMIT_MARGIN: Duration = Duration::from_secs(300);

/// A recipe whose log has not grown for this long is stopped (the user's figure).
const QUIET_LIMIT: Duration = Duration::from_secs(600);

/// A recipe's boot limits when `just --dry-run` shows none (it failed, or the recipe boots
/// through something else): one hour, before the scale.
const UNKNOWN_BOOTS: Duration = Duration::from_secs(3600);

/// `smoke-all`'s arguments: `-j N`, `--just PATH` and the recipe names, in any order.
#[derive(Debug, PartialEq, Eq)]
pub struct Args<'a> {
    pub jobs: usize,
    pub just: &'a str,
    pub recipes: Vec<&'a str>,
}

/// Parses `smoke-all`'s arguments (after the subcommand's name).
pub fn parse_args<'a>(args: &[&'a str]) -> Result<Args<'a>> {
    let mut parsed = Args {
        jobs: DEFAULT_JOBS,
        just: "just",
        recipes: Vec::new(),
    };
    let mut it = args.iter();
    while let Some(&a) = it.next() {
        match a {
            "-j" | "--jobs" => {
                let v = it.next().ok_or("smoke-all: -j needs a number")?;
                parsed.jobs = match v.parse::<usize>() {
                    Ok(n) if (1..=MAX_JOBS).contains(&n) => n,
                    _ => {
                        return Err(format!(
                            "smoke-all: -j {v}: expected a number from 1 to {MAX_JOBS}"
                        )
                        .into());
                    }
                };
            }
            "--just" => parsed.just = it.next().ok_or("smoke-all: --just needs a path")?,
            flag if flag.starts_with('-') => {
                return Err(format!("smoke-all: unknown flag {flag}").into());
            }
            recipe => parsed.recipes.push(recipe),
        }
    }
    Ok(parsed)
}

/// How one recipe ended.
struct Finished {
    recipe: String,
    ok: bool,
    /// Stopped by the watchdog (`why` says which limit).
    timeout: bool,
    seconds: f32,
    log: PathBuf,
    why: String,
}

/// Runs `recipes` with `just` (`just_bin`), `jobs` at a time; see the module documentation.
pub fn smoke_all(root: &Path, jobs: usize, just_bin: &str, recipes: &[&str]) -> Result<()> {
    if recipes.is_empty() {
        return Err("smoke-all: no recipes given".into());
    }
    let base = root.join("target").join("smoke");
    let scale = timeout_scale(jobs);
    let queue: VecDeque<String> = order(&base, recipes).into_iter().collect();
    let total = queue.len();
    println!(
        "smoke-all: {total} recipes, {jobs} at a time, time limits x{scale}; logs in {}/<recipe>/log",
        base.display()
    );
    let queue = Arc::new(Mutex::new(queue));
    let (tx, rx) = mpsc::channel::<Finished>();
    let started = Instant::now();
    let mut workers = Vec::new();
    for _ in 0..jobs.min(total) {
        let queue = Arc::clone(&queue);
        let tx = tx.clone();
        let root = root.to_path_buf();
        let base = base.clone();
        let just_bin = just_bin.to_string();
        workers.push(thread::spawn(move || {
            loop {
                let next = queue.lock().ok().and_then(|mut q| q.pop_front());
                let Some(recipe) = next else {
                    break;
                };
                let limit = recipe_limit(&root, &just_bin, &recipe, scale);
                let done = run_recipe(&root, &base, &just_bin, &recipe, scale, limit);
                if tx.send(done).is_err() {
                    break;
                }
            }
        }));
    }
    drop(tx);

    let mut failed: Vec<Finished> = Vec::new();
    let mut count = 0;
    for done in rx {
        count += 1;
        println!(
            "smoke-all: [{count:>2}/{total}] {} {:<16} {:>6.1}s{}",
            if done.ok {
                "ok  "
            } else if done.timeout {
                "TIMEOUT"
            } else {
                "FAIL"
            },
            done.recipe,
            done.seconds,
            if done.ok {
                String::new()
            } else {
                format!("  ({})", done.why)
            }
        );
        if !done.ok {
            failed.push(done);
        }
    }
    for w in workers {
        let _ = w.join();
    }
    let elapsed = started.elapsed().as_secs();
    for f in &failed {
        println!();
        println!("===== {} FAILED: {} =====", f.recipe, f.log.display());
        match fs::read(&f.log) {
            Ok(bytes) => print!("{}", String::from_utf8_lossy(&bytes)),
            Err(e) => println!("(cannot read the log: {e})"),
        }
        println!("===== end of {} =====", f.recipe);
    }
    let names: Vec<&str> = failed.iter().map(|f| f.recipe.as_str()).collect();
    println!(
        "smoke-all: {} of {total} passed in {}m{:02}s{}",
        total - failed.len(),
        elapsed / 60,
        elapsed % 60,
        if names.is_empty() {
            String::new()
        } else {
            format!("; FAILED: {}", names.join(" "))
        }
    );
    if failed.is_empty() {
        Ok(())
    } else {
        Err(format!(
            "smoke-all: {} recipe(s) failed: {}",
            failed.len(),
            names.join(" ")
        )
        .into())
    }
}

/// The time-limit factor for `jobs` recipes at a time: 1 up to two, then `jobs / 2` rounded
/// up (2 for the default 4), at most 10.
fn timeout_scale(jobs: usize) -> usize {
    jobs.div_ceil(2).clamp(1, 10)
}

/// `recipes` longest first, by the seconds each took last time; recipes never timed come
/// first, in the given order (they may be long). Duplicates are dropped.
fn order(base: &Path, recipes: &[&str]) -> Vec<String> {
    let mut seen: Vec<&str> = Vec::new();
    for r in recipes {
        if !seen.contains(r) {
            seen.push(r);
        }
    }
    let last = |r: &str| -> Option<f32> {
        fs::read_to_string(base.join(r).join("seconds"))
            .ok()
            .and_then(|s| s.trim().parse::<f32>().ok())
    };
    let mut timed: Vec<(String, Option<f32>)> =
        seen.iter().map(|r| (r.to_string(), last(r))).collect();
    sort_longest_first(&mut timed);
    timed.into_iter().map(|(r, _)| r).collect()
}

/// Untimed entries first (stable), then by time, longest first.
fn sort_longest_first(v: &mut [(String, Option<f32>)]) {
    v.sort_by(|a, b| match (a.1, b.1) {
        (None, None) => std::cmp::Ordering::Equal,
        (None, Some(_)) => std::cmp::Ordering::Less,
        (Some(_), None) => std::cmp::Ordering::Greater,
        (Some(x), Some(y)) => y.total_cmp(&x),
    });
}

/// The watchdog's limit for `recipe`: its boots' limits from `just --dry-run`
/// ([`boot_limits`]) times `scale`, plus [`LIMIT_MARGIN`].
fn recipe_limit(root: &Path, just_bin: &str, recipe: &str, scale: usize) -> Duration {
    let shown = Command::new(just_bin)
        .args(["--no-deps", "--dry-run", recipe])
        .current_dir(root)
        .stdin(Stdio::null())
        .output();
    let boots = match shown {
        Ok(o) if o.status.success() => {
            // `just` prints the commands of a dry run on stderr.
            let text = String::from_utf8_lossy(&o.stderr).into_owned()
                + &String::from_utf8_lossy(&o.stdout);
            boot_limits(&text)
        }
        _ => None,
    };
    boots.unwrap_or(UNKNOWN_BOOTS) * u32::try_from(scale).unwrap_or(10) + LIMIT_MARGIN
}

/// The sum of the time limits of the boots in a recipe's commands: `boot::SMOKE_TIMEOUT` for
/// each `xtask smoke`, its `--timeout` (or the same default) for each `xtask smoke2`. `None`
/// when there is no boot.
fn boot_limits(commands: &str) -> Option<Duration> {
    let mut sum = Duration::ZERO;
    let mut boots = 0;
    for line in commands.lines() {
        let mut words = line.split_whitespace();
        let Some(cmd) = words
            .by_ref()
            .skip_while(|w| *w != "xtask")
            .nth(1)
            .filter(|c| *c == "smoke" || *c == "smoke2")
        else {
            continue;
        };
        let mut limit = boot::SMOKE_TIMEOUT;
        if cmd == "smoke2" {
            let mut rest = words;
            while let Some(w) = rest.next() {
                if w == "--timeout"
                    && let Some(s) = rest.next().and_then(|v| v.parse::<u64>().ok())
                {
                    limit = Duration::from_secs(s);
                }
            }
        }
        sum += limit;
        boots += 1;
    }
    (boots > 0).then_some(sum)
}

/// Why the watchdog stops a recipe that has run `elapsed` with a log still for `quiet`, if
/// it does.
fn watchdog(elapsed: Duration, quiet: Duration, limit: Duration) -> Option<String> {
    if elapsed > limit {
        Some(format!(
            "over its limit of {}s (its boots' limits x the scale, + {}s)",
            limit.as_secs(),
            LIMIT_MARGIN.as_secs()
        ))
    } else if quiet >= QUIET_LIMIT {
        Some(format!("its log has not grown for {}s", quiet.as_secs()))
    } else {
        None
    }
}

/// `root` and every process below it in `table` (pid, parent pid pairs), parents first.
fn descendants(table: &[(u32, u32)], root: u32) -> Vec<u32> {
    let mut found = vec![root];
    let mut i = 0;
    while i < found.len() {
        let parent = found[i];
        for &(pid, ppid) in table {
            if ppid == parent && !found.contains(&pid) {
                found.push(pid);
            }
        }
        i += 1;
    }
    found
}

/// SIGKILLs `root` and its whole process tree, read from `ps` once, parents first, so none
/// of them starts another process meanwhile. A tree, not a process group: a recipe in a
/// group of its own would no longer get the terminal's Ctrl-C.
fn kill_tree(root: u32) {
    let table: Vec<(u32, u32)> = Command::new("ps")
        .args(["-axo", "pid=,ppid="])
        .output()
        .map(|o| {
            String::from_utf8_lossy(&o.stdout)
                .lines()
                .filter_map(|l| {
                    let mut w = l.split_whitespace();
                    Some((w.next()?.parse().ok()?, w.next()?.parse().ok()?))
                })
                .collect()
        })
        .unwrap_or_default();
    let pids: Vec<String> = descendants(&table, root)
        .iter()
        .map(u32::to_string)
        .collect();
    let _ = Command::new("kill")
        .arg("-KILL")
        .args(&pids)
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .status();
}

/// Runs `just --no-deps recipe` with its own run directory and log, under the watchdog
/// (`limit`, [`QUIET_LIMIT`]); never fails itself, the result says how the recipe ended.
fn run_recipe(
    root: &Path,
    base: &Path,
    just_bin: &str,
    recipe: &str,
    scale: usize,
    limit: Duration,
) -> Finished {
    let dir = base.join(recipe);
    let log = dir.join("log");
    let started = Instant::now();
    let finish = |ok: bool, why: String| Finished {
        recipe: recipe.to_string(),
        ok,
        timeout: false,
        seconds: started.elapsed().as_secs_f32(),
        log: log.clone(),
        why,
    };
    if let Err(e) = fs::create_dir_all(&dir) {
        return finish(false, format!("{}: {e}", dir.display()));
    }
    let mut out = match File::create(&log) {
        Ok(f) => f,
        Err(e) => return finish(false, format!("{}: {e}", log.display())),
    };
    let _ = writeln!(
        out,
        "smoke-all: watchdog: {recipe} stops after {}s, or {}s without output",
        limit.as_secs(),
        QUIET_LIMIT.as_secs()
    );
    let err = match out.try_clone() {
        Ok(f) => f,
        Err(e) => return finish(false, format!("{}: {e}", log.display())),
    };
    let spawned = Command::new(just_bin)
        .args(["--no-deps", recipe])
        .current_dir(root)
        .env(boot::RUN_DIR_ENV, &dir)
        .env(boot::TIMEOUT_SCALE_ENV, scale.to_string())
        .stdin(Stdio::null())
        .stdout(Stdio::from(out))
        .stderr(Stdio::from(err))
        .spawn();
    let mut child = match spawned {
        Ok(c) => c,
        Err(e) => return finish(false, format!("{just_bin}: {e}")),
    };
    let mut size = 0;
    let mut grew = Instant::now();
    let status = loop {
        match child.try_wait() {
            Ok(Some(s)) => break Ok(s),
            Ok(None) => {}
            Err(e) => break Err(e),
        }
        let now = fs::metadata(&log).map(|m| m.len()).unwrap_or(0);
        if now != size {
            size = now;
            grew = Instant::now();
        }
        if let Some(why) = watchdog(started.elapsed(), grew.elapsed(), limit) {
            kill_tree(child.id());
            let _ = child.wait();
            if let Ok(mut f) = OpenOptions::new().append(true).open(&log) {
                let _ = writeln!(
                    f,
                    "\nsmoke-all: watchdog: {recipe} {why}; killed its process tree"
                );
            }
            let mut done = finish(false, why);
            done.timeout = true;
            return done;
        }
        thread::sleep(Duration::from_secs(1));
    };
    let done = match status {
        Ok(s) if s.success() => finish(true, String::new()),
        Ok(s) => finish(false, format!("just exited with {s}")),
        Err(e) => finish(false, format!("{just_bin}: {e}")),
    };
    if done.ok {
        let _ = fs::write(dir.join("seconds"), format!("{:.1}\n", done.seconds));
        remove_boot_files(&dir);
    }
    done
}

/// Removes what a passed recipe's boots rebuild anyway (boot images, EDK2 variable stores),
/// keeping the persistent disks and the log. A failed recipe keeps everything.
fn remove_boot_files(dir: &Path) {
    let Ok(entries) = fs::read_dir(dir) else {
        return;
    };
    for e in entries.flatten() {
        let name = e.file_name();
        let name = name.to_string_lossy();
        let boot_image = name.starts_with("emibsd-") && name.ends_with(".img");
        let vars = name.starts_with("edk2-") && name.ends_with("-vars.fd");
        if boot_image || vars {
            let _ = fs::remove_file(e.path());
        }
    }
}
/* </CODE> */

/* <TESTS> */
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn args_take_jobs_just_and_recipes_in_any_order() {
        let a = parse_args(&["smoke-a", "-j", "6", "smoke-b", "--just", "/x/just"]).unwrap();
        assert_eq!(
            a,
            Args {
                jobs: 6,
                just: "/x/just",
                recipes: vec!["smoke-a", "smoke-b"],
            }
        );
        let d = parse_args(&["smoke-a"]).unwrap();
        assert_eq!((d.jobs, d.just), (DEFAULT_JOBS, "just"));
        assert!(parse_args(&["-j", "0", "smoke-a"]).is_err());
        assert!(parse_args(&["-j", "x"]).is_err());
        assert!(parse_args(&["-j"]).is_err());
        assert!(parse_args(&["--bogus", "smoke-a"]).is_err());
    }

    #[test]
    fn boot_limits_add_up_the_recipes_boots() {
        let cmds = "test -f target/userland/amd64/ramdisk.ffs || exit 1\n\
            cargo xtask smoke --arch amd64 --send 'x --timeout 9\\n'\n\
            cargo xtask nvme-root --arch amd64 target/x.img\n\
            cargo xtask smoke2 --arch arm64 --timeout 400 --both-expect ok\n\
            cargo xtask smoke2 --arch amd64\n";
        // 180 (a smoke's --timeout-like text is not smoke2's flag) + 400 + 180.
        assert_eq!(boot_limits(cmds), Some(Duration::from_secs(760)));
        assert_eq!(boot_limits("cargo xtask smoke-all -j 4 smoke-a\n"), None);
        assert_eq!(boot_limits("echo nothing\n"), None);
    }

    #[test]
    fn watchdog_stops_on_the_limit_or_a_still_log() {
        let s = Duration::from_secs;
        assert_eq!(watchdog(s(100), s(30), s(500)), None);
        assert_eq!(watchdog(s(500), s(599), s(500)), None);
        let over = watchdog(s(501), s(0), s(500)).unwrap();
        assert!(over.starts_with("over its limit of 500s"), "{over}");
        let quiet = watchdog(s(700), s(600), s(9000)).unwrap();
        assert_eq!(quiet, "its log has not grown for 600s");
    }

    #[test]
    fn the_tree_is_the_root_and_everything_below_it_parents_first() {
        // just 10 -> sh 11 -> cargo 12 -> xtask 13 -> qemu 14; 20 is a stranger.
        let table = [
            (14, 13),
            (11, 10),
            (20, 1),
            (13, 12),
            (12, 11),
            (15, 13),
            (10, 1),
        ];
        assert_eq!(descendants(&table, 10), [10, 11, 12, 13, 14, 15]);
        assert_eq!(descendants(&table, 13), [13, 14, 15]);
        assert_eq!(descendants(&table, 99), [99]);
    }

    #[test]
    fn a_hung_recipe_is_killed_whole_and_reported_as_a_timeout() {
        // A stand-in `just`: a shell whose child sleeps without a word, as the frozen xtask did.
        let dir = std::env::temp_dir().join(format!("xtask-smokeall-wd-{}", std::process::id()));
        fs::create_dir_all(&dir).unwrap();
        let just = dir.join("fake-just");
        fs::write(
            &just,
            "#!/bin/sh\necho started\nsleep 300 &\necho $! > \"$EMIBSD_RUN_DIR/child\"\nwait\n",
        )
        .unwrap();
        Command::new("chmod").arg("+x").arg(&just).status().unwrap();
        let done = run_recipe(
            &dir,
            &dir,
            just.to_str().unwrap(),
            "smoke-hang",
            1,
            Duration::from_secs(2),
        );
        assert!(!done.ok && done.timeout, "{}", done.why);
        assert!(done.why.starts_with("over its limit of 2s"), "{}", done.why);
        let log = fs::read_to_string(dir.join("smoke-hang/log")).unwrap();
        assert!(
            log.contains("smoke-all: watchdog: smoke-hang over its limit"),
            "{log}"
        );
        // The sleeping grandchild went with it.
        let child = fs::read_to_string(dir.join("smoke-hang/child")).unwrap();
        let alive = Command::new("kill")
            .args(["-0", child.trim()])
            .stderr(Stdio::null())
            .status()
            .unwrap();
        assert!(!alive.success(), "pid {} still alive", child.trim());
        fs::remove_dir_all(&dir).unwrap();
    }

    #[test]
    fn scale_grows_with_jobs() {
        let got: Vec<usize> = [1, 2, 3, 4, 8, 32]
            .iter()
            .map(|&j| timeout_scale(j))
            .collect();
        assert_eq!(got, [1, 1, 2, 2, 4, 10]);
    }

    #[test]
    fn untimed_first_then_longest_first() {
        let mut v = vec![
            ("a".to_string(), Some(10.0)),
            ("b".to_string(), None),
            ("c".to_string(), Some(90.0)),
            ("d".to_string(), None),
            ("e".to_string(), Some(30.0)),
        ];
        sort_longest_first(&mut v);
        let names: Vec<&str> = v.iter().map(|(n, _)| n.as_str()).collect();
        assert_eq!(names, ["b", "d", "c", "e", "a"]);
    }

    #[test]
    fn order_reads_last_times_and_drops_duplicates() {
        let base = std::env::temp_dir().join(format!("xtask-smokeall-{}", std::process::id()));
        fs::create_dir_all(base.join("slow")).unwrap();
        fs::create_dir_all(base.join("fast")).unwrap();
        fs::write(base.join("slow/seconds"), "120.5\n").unwrap();
        fs::write(base.join("fast/seconds"), "8.0\n").unwrap();
        let got = order(&base, &["fast", "new", "slow", "fast"]);
        assert_eq!(got, ["new", "slow", "fast"]);
        fs::remove_dir_all(&base).unwrap();
    }

    #[test]
    fn only_boot_files_are_removed() {
        let dir = std::env::temp_dir().join(format!("xtask-smokeall-rm-{}", std::process::id()));
        fs::create_dir_all(&dir).unwrap();
        for f in [
            "emibsd-amd64.img",
            "emibsd-arm64-a.img",
            "edk2-arm64-b-vars.fd",
            "disk-amd64.img",
            "log",
        ] {
            fs::write(dir.join(f), "x").unwrap();
        }
        remove_boot_files(&dir);
        let mut left: Vec<String> = fs::read_dir(&dir)
            .unwrap()
            .map(|e| e.unwrap().file_name().to_string_lossy().into_owned())
            .collect();
        left.sort();
        assert_eq!(left, ["disk-amd64.img", "log"]);
        fs::remove_dir_all(&dir).unwrap();
    }
}
/* </TESTS> */
