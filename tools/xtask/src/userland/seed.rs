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
//! A git worktree's first `userland` or `comp` starts from the main checkout's build (the
//! user's decision of 2026-10-09: the OpenBSD sources are pinned, so compiling them again in
//! every worktree an agent gets is wasted time).
//!
//! When `<root>/target/userland/<arch>` (or `<root>/target/comp`) does not exist, `root` is a
//! worktree, and the main checkout has that directory, it is copied with `cp -Rcp`: APFS
//! clones every file (instant, no space until a file changes) and keeps the modification
//! times, so the up-to-date checks see what the main checkout's build saw. The build records
//! absolute paths, so the copy's rule stamps (`*.cmd`), dependency files (`*.d`), archive
//! member lists (`*.members`), include manifests (`manifest`, `install`) and absolute
//! symbolic links that name `<main>/...` are rebased to `<root>/...`. Paths into
//! `<main>/reference/` stay: a worktree's `reference/openbsd-src` is the main checkout's
//! (`openbsd_src`), so its builds name the same files. Then the build runs as usual and
//! redoes only what this worktree changed: what depends on its own sources (which git
//! checked out after the main checkout's objects were made) and whatever its commits touch.
//! Anything that fails leaves no copy behind, and the build starts from nothing as before.

use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;

use crate::Result;

/// The main checkout of the worktree `root`, or `None` when `root` is the main checkout (or
/// not a git checkout).
fn main_checkout(root: &Path) -> Option<PathBuf> {
    let common = Command::new("git")
        .arg("-C")
        .arg(root)
        .args(["rev-parse", "--path-format=absolute", "--git-common-dir"])
        .output()
        .ok()
        .filter(|o| o.status.success())
        .map(|o| PathBuf::from(String::from_utf8_lossy(&o.stdout).trim()))?;
    let main = fs::canonicalize(common.parent()?).ok()?;
    let here = fs::canonicalize(root).ok()?;
    (main != here).then_some(main)
}

/// Seeds `<root>/<rel>` from the main checkout's (see the module documentation). Returns
/// whether it did.
pub(crate) fn seed_from_main(root: &Path, rel: &str) -> Result<bool> {
    let dest = root.join(rel);
    if dest.exists() {
        return Ok(false);
    }
    let Some(main) = main_checkout(root) else {
        return Ok(false);
    };
    let from = main.join(rel);
    if !from.is_dir() {
        return Ok(false);
    }
    if let Some(parent) = dest.parent() {
        fs::create_dir_all(parent)?;
    }
    let here = fs::canonicalize(root)?;
    let copied = Command::new("cp")
        .arg("-Rcp")
        .arg(&from)
        .arg(&dest)
        .status()
        .map(|s| s.success())
        .unwrap_or(false);
    let rebased = copied && rebase_tree(&dest, &main, &here).is_ok();
    if !rebased {
        let _ = fs::remove_dir_all(&dest);
        println!(
            "  {rel}: could not seed it from {}; building from nothing",
            from.display()
        );
        return Ok(false);
    }
    println!(
        "  {rel}: seeded from {} (APFS clone, paths rebased)",
        from.display()
    );
    Ok(true)
}

/// Whether the file `name` is one of the records that hold absolute paths.
fn is_record(name: &str) -> bool {
    name.ends_with(".cmd")
        || name.ends_with(".d")
        || name.ends_with(".members")
        || name == "manifest"
        || name == "install"
}

/// `text` with `<main>/` rebased to `<here>/`, but `<main>/reference/` kept; `None` when it
/// names no path under `main`.
fn rebase(text: &str, main: &Path, here: &Path) -> Option<String> {
    let from = format!("{}/", main.display());
    if !text.contains(&from) {
        return None;
    }
    let to = format!("{}/", here.display());
    let keep = format!("{from}reference/");
    let moved = format!("{to}reference/");
    Some(text.replace(&from, &to).replace(&moved, &keep))
}

/// Rebases every record and every absolute symbolic link under `dir` (not following links).
fn rebase_tree(dir: &Path, main: &Path, here: &Path) -> std::io::Result<()> {
    for entry in fs::read_dir(dir)? {
        let entry = entry?;
        let path = entry.path();
        let kind = entry.file_type()?;
        if kind.is_symlink() {
            let target = fs::read_link(&path)?;
            if target.is_absolute()
                && let Some(t) = target.to_str()
                && let Some(new) = rebase(t, main, here)
            {
                fs::remove_file(&path)?;
                std::os::unix::fs::symlink(new, &path)?;
            }
        } else if kind.is_dir() {
            rebase_tree(&path, main, here)?;
        } else if is_record(&entry.file_name().to_string_lossy())
            && let Ok(text) = fs::read_to_string(&path)
            && let Some(new) = rebase(&text, main, here)
        {
            fs::write(&path, new)?;
        }
    }
    Ok(())
}
/* </CODE> */

/* <TESTS> */
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn rebase_moves_the_build_but_keeps_the_reference() {
        let main = Path::new("/w/EmiBSD.LZ");
        let here = Path::new("/w/EmiBSD.LZ/.claude/worktrees/agent-x");
        let cmd = "clang --sysroot=/w/EmiBSD.LZ/target/userland/amd64/sysroot \
                   -c /w/EmiBSD.LZ/reference/openbsd-src/bin/ls/ls.c /w/EmiBSD.LZ/tools/t.c";
        assert_eq!(
            rebase(cmd, main, here).as_deref(),
            Some(
                "clang --sysroot=/w/EmiBSD.LZ/.claude/worktrees/agent-x/target/userland/amd64/sysroot \
                 -c /w/EmiBSD.LZ/reference/openbsd-src/bin/ls/ls.c \
                 /w/EmiBSD.LZ/.claude/worktrees/agent-x/tools/t.c"
            )
        );
        assert_eq!(rebase("cc -c x.c -o x.o", main, here), None);
        // A sibling directory that only starts like the main checkout is not it.
        assert_eq!(rebase("/w/EmiBSD.LZ2/x", main, here), None);
    }

    #[test]
    fn records_are_the_files_that_hold_paths() {
        for name in ["ls.o.cmd", "ls.d", "libc.a.members", "manifest", "install"] {
            assert!(is_record(name), "{name}");
        }
        for name in ["ls.o", "ls.c", "ramdisk.ffs", "keys.list"] {
            assert!(!is_record(name), "{name}");
        }
    }

    #[test]
    fn a_tree_is_rebased_in_place() {
        let base = std::env::temp_dir().join(format!("xtask-seed-{}", std::process::id()));
        let main = base.join("main");
        let here = base.join("main/wt");
        let obj = here.join("target/userland/amd64/obj");
        fs::create_dir_all(&obj).unwrap();
        let m = main.display();
        fs::write(
            obj.join("a.o.cmd"),
            format!("cc -I{m}/target/inc {m}/reference/a.c\n"),
        )
        .unwrap();
        fs::write(obj.join("a.o"), format!("binary {m}/target\n")).unwrap();
        std::os::unix::fs::symlink(format!("{m}/target/host/tool"), obj.join("tool")).unwrap();
        std::os::unix::fs::symlink("../relative", obj.join("rel")).unwrap();
        rebase_tree(&here.join("target"), &main, &here).unwrap();
        let h = here.display();
        assert_eq!(
            fs::read_to_string(obj.join("a.o.cmd")).unwrap(),
            format!("cc -I{h}/target/inc {m}/reference/a.c\n")
        );
        assert_eq!(
            fs::read_to_string(obj.join("a.o")).unwrap(),
            format!("binary {m}/target\n"),
            "objects are not records"
        );
        assert_eq!(
            fs::read_link(obj.join("tool")).unwrap(),
            PathBuf::from(format!("{h}/target/host/tool"))
        );
        assert_eq!(
            fs::read_link(obj.join("rel")).unwrap(),
            PathBuf::from("../relative")
        );
        fs::remove_dir_all(&base).unwrap();
    }

    #[test]
    fn the_main_checkout_seeds_nothing() {
        let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("../..");
        // In the main checkout there is nothing to seed from; in a worktree the directory
        // asked for does not exist in the main checkout either.
        assert!(!seed_from_main(&root, "target/xtask-seed-test-nothing-here").unwrap());
    }
}
/* </TESTS> */
