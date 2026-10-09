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
//!
//! The rest of what runs at once was made per run before: `smoke2`'s link ports are free
//! ports asked of the system (and asked again if QEMU finds one taken), its VMs have their
//! own `-a`/`-b` files. The HTTPS test servers' ports are fixed, but only `smoke-https` uses
//! them. Recipes start longest first, by the time each took last (`target/smoke/RECIPE/seconds`),
//! so a long one does not start last.

use std::collections::VecDeque;
use std::fs::{self, File};
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::sync::mpsc;
use std::sync::{Arc, Mutex};
use std::thread;
use std::time::Instant;

use crate::Result;
use crate::boot;

/// Recipes run at once unless `-j` says otherwise.
pub const DEFAULT_JOBS: usize = 4;

/// The most recipes `-j` may run at once.
pub const MAX_JOBS: usize = 32;

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
                let done = run_recipe(&root, &base, &just_bin, &recipe, scale);
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
            if done.ok { "ok  " } else { "FAIL" },
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

/// Runs `just --no-deps recipe` with its own run directory and log; never fails itself, the
/// result says how the recipe ended.
fn run_recipe(root: &Path, base: &Path, just_bin: &str, recipe: &str, scale: usize) -> Finished {
    let dir = base.join(recipe);
    let log = dir.join("log");
    let started = Instant::now();
    let finish = |ok: bool, why: String| Finished {
        recipe: recipe.to_string(),
        ok,
        seconds: started.elapsed().as_secs_f32(),
        log: log.clone(),
        why,
    };
    if let Err(e) = fs::create_dir_all(&dir) {
        return finish(false, format!("{}: {e}", dir.display()));
    }
    let out = match File::create(&log) {
        Ok(f) => f,
        Err(e) => return finish(false, format!("{}: {e}", log.display())),
    };
    let err = match out.try_clone() {
        Ok(f) => f,
        Err(e) => return finish(false, format!("{}: {e}", log.display())),
    };
    let status = Command::new(just_bin)
        .args(["--no-deps", recipe])
        .current_dir(root)
        .env(boot::RUN_DIR_ENV, &dir)
        .env(boot::TIMEOUT_SCALE_ENV, scale.to_string())
        .stdin(Stdio::null())
        .stdout(Stdio::from(out))
        .stderr(Stdio::from(err))
        .status();
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
