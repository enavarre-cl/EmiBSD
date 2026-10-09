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
//! The libraries of `LIBRARIES` (M9+): LibreSSL's `libcrypto`, `libssl` and `libtls`, and
//! `libcurses` and `libedit`. They are built by `build_lib` like libc; what is new about
//! them is here, and is driven by their own Makefiles, not by lists of files:
//!
//! - **Generated sources** (`BUILDFIRST`, which `bsd.lib.mk` makes before any object):
//!   `make_target` runs a target's rule after making, recursively, every source of it that
//!   has a rule of its own, as make(1) does. That covers libcrypto's `obj_mac.h` and
//!   `obj_dat.h` (`objects.pl`, `obj_dat.pl`) and, on amd64, its perlasm `.S` files (each
//!   `${f}.S` rule of `arch/amd64/Makefile.inc` runs `/usr/bin/perl ./asm/${f}.pl openbsd`,
//!   the Mac's perl); libcurses's tables (`MKkeys_list.sh`, `MKcaptab.sh` over
//!   `MKcaptab.awk`, `MKnames.awk`, `MKcodes.awk`, ... with the Mac's `/usr/bin/awk`, the
//!   same one-true-awk as OpenBSD's) and the build tools `make_keys` and `make_hash`, which
//!   its rules compile with `${HOSTCC}` (the Mac's clang) and run; libedit's headers from its
//!   `makelist` script.
//! - **Headers** (`include/Makefile`'s `RDIRS`): `library_includes` runs the library's own
//!   `includes` rule with an `install(1)` stand-in in `$PATH` that only records what it is
//!   asked to install, and a `cmp(1)` that always says "different", so that every header
//!   is recorded on every run. xtask then installs the recorded files itself
//!   (`install_file`: unchanged headers keep their mtime, and the licence report knows
//!   where each came from). So libcrypto's headers land in `<openssl/...>` with the
//!   generated `obj_mac.h`, libcurses's `curses.h` as `<ncurses.h>`, as their Makefiles say.

use super::*;

/// The `install(1)` stand-in for `includes` rules: options dropped, every source recorded
/// (made absolute) with the destination, one `source destination` line each, into
/// `@MANIFEST@`. `install -d dir...` (libfuse's rule makes `/usr/include/fuse` so) records a
/// `-d dir` line per directory, which xtask creates, so that a later `install file dir`
/// lands inside it.
pub(super) const INSTALL_SH: &str = "\
#!/bin/sh
# EmiBSD: install(1) for a library's `includes` rule (tools/xtask, userland/libraries.rs).
# Records what would be installed; xtask installs it.
d=
while getopts CcDdo:g:m: opt; do [ \"$opt\" = d ] && d=1; done
shift $((OPTIND - 1))
if [ -n \"$d\" ]; then
\tfor dir; do echo \"-d $dir\" >> '@MANIFEST@'; done
\texit 0
fi
eval \"dest=\\${$#}\"
while [ $# -gt 1 ]; do
\tcase $1 in /*) src=$1 ;; *) src=$(pwd)/$1 ;; esac
\techo \"$src $dest\" >> '@MANIFEST@'
\tshift
done
";

/// The `cmp(1)` stand-in for `includes` rules: never equal, so `cmp -s a b || install ...`
/// always reaches `install`.
pub(super) const CMP_SH: &str = "#!/bin/sh\nexit 1\n";

/// Makes `target` of `mk` in `objdir` as make(1) would: every source that a rule makes
/// first (recursively), then the target's own commands if a rule has some. `path` is put
/// first in `$PATH` for the target's own commands (not for its sources'). `made` holds the
/// targets already made in this evaluation. Returns the target's path.
pub(super) fn make_target(
    ctx: &Ctx<'_>,
    mk: &Make,
    objdir: &Path,
    target: &str,
    path: Option<&Path>,
    made: &mut BTreeSet<String>,
) -> Result<PathBuf> {
    let out = objdir.join(target);
    if !made.insert(target.to_string()) {
        return Ok(out);
    }
    let mut sources = Vec::new();
    for s in mk.sources_of(target) {
        let generated =
            mk.rule_for(&s).is_some() || (mk.search(&s).is_none() && !mk.sources_of(&s).is_empty());
        if generated {
            sources.push(make_target(ctx, mk, objdir, &s, None, made)?);
        } else if let Some(p) = mk.search(&s) {
            sources.push(p);
        } else if objdir.join(&s).exists() {
            sources.push(objdir.join(&s));
        } else {
            return Err(format!("no rule to make {s}, needed by {target}").into());
        }
    }
    if let Some(rule) = mk.rule_for(target) {
        let mut job = Job::from_rule(mk, &rule.commands, target, sources, objdir)?;
        job.path = path.map(Path::to_path_buf);
        run_jobs(ctx, target, &[job])?;
    }
    Ok(out)
}

/// Installs the headers of library `dir` (one of `LIBRARIES`, relative to the sources) into
/// the sysroot by running its `includes` rule (module docs). Returns, per header, whether
/// it was written (`install_file`).
pub(super) fn library_includes(ctx: &Ctx<'_>, dir: &str) -> Result<Vec<bool>> {
    let objdir = ctx.out.join("obj").join(dir);
    let shims = objdir.join("emibsd-includes");
    fs::create_dir_all(&shims).map_err(|e| format!("{}: {e}", shims.display()))?;
    let manifest = shims.join("manifest");
    let _ = fs::remove_file(&manifest);
    for (name, text) in [
        (
            "install",
            INSTALL_SH.replace("@MANIFEST@", &manifest.display().to_string()),
        ),
        ("cmp", CMP_SH.to_string()),
    ] {
        let p = shims.join(name);
        ramdisk::write_if_changed(&p, &text)?;
        use std::os::unix::fs::PermissionsExt as _;
        fs::set_permissions(&p, fs::Permissions::from_mode(0o755))
            .map_err(|e| format!("{}: {e}", p.display()))?;
    }
    let mut mk = new_make(ctx, dir, &objdir)?;
    // bsd.own.mk's owner and group (ignored by the stand-in) and sys.mk's `INSTALL`.
    for (k, v) in [
        ("INSTALL", "install"),
        ("INSTALL_COPY", "-C"),
        ("BINOWN", "root"),
        ("BINGRP", "bin"),
    ] {
        mk.set(k, v);
    }
    if mk.rule_for("includes").is_none() {
        return Err(format!("{dir}/Makefile: no `includes` rule").into());
    }
    make_target(
        ctx,
        &mk,
        &objdir,
        "includes",
        Some(&shims),
        &mut BTreeSet::new(),
    )?;
    let text = fs::read_to_string(&manifest).map_err(|e| {
        format!(
            "{dir}: its `includes` rule installed nothing ({}: {e})",
            manifest.display()
        )
    })?;
    let mut written = Vec::new();
    for line in text.lines() {
        let (src, dest) = line
            .split_once(' ')
            .ok_or_else(|| format!("{}: bad line `{line}`", manifest.display()))?;
        let (src, mut dest) = (PathBuf::from(src), PathBuf::from(dest));
        if !dest.starts_with(&ctx.sysroot) {
            return Err(format!("{dir}: `includes` installs outside the sysroot: {line}").into());
        }
        // `install -d dest`: a directory (an earlier build may have left a file there).
        if src == Path::new("-d") {
            if dest.is_file() {
                fs::remove_file(&dest).map_err(|e| format!("{}: {e}", dest.display()))?;
            }
            fs::create_dir_all(&dest).map_err(|e| format!("{}: {e}", dest.display()))?;
            continue;
        }
        if dest.is_dir() {
            dest = dest.join(src.file_name().ok_or("bad header name")?);
        }
        written.push(install_file(ctx, &src, &dest)?);
    }
    Ok(written)
}
/* </CODE> */

/* <TESTS> */
#[cfg(test)]
mod tests {
    use super::*;

    /// The `install(1)` stand-in records absolute sources and the destination, whatever
    /// the options.
    #[test]
    fn install_stand_in_records_sources_and_destination() {
        let dir = std::env::temp_dir().join(format!("emibsd-install-sh-{}", std::process::id()));
        let _ = fs::remove_dir_all(&dir);
        fs::create_dir_all(&dir).unwrap();
        let manifest = dir.join("manifest");
        let sh = dir.join("install");
        fs::write(
            &sh,
            INSTALL_SH.replace("@MANIFEST@", &manifest.display().to_string()),
        )
        .unwrap();
        let ok = Command::new("/bin/sh")
            .arg(&sh)
            .args(["-C", "-o", "root", "-g", "bin", "-m", "444"])
            .args(["a.h", "/abs/b.h", "/dest/include"])
            .current_dir(&dir)
            .status()
            .unwrap()
            .success();
        assert!(ok);
        // `install -d` (libfuse's `includes`) records the directory to make.
        let ok = Command::new("/bin/sh")
            .arg(&sh)
            .args([
                "-d",
                "-o",
                "root",
                "-g",
                "bin",
                "-m",
                "755",
                "/dest/include/fuse",
            ])
            .current_dir(&dir)
            .status()
            .unwrap()
            .success();
        assert!(ok);
        let text = fs::read_to_string(&manifest).unwrap();
        let cwd = fs::canonicalize(&dir).unwrap();
        let lines: Vec<&str> = text.lines().collect();
        assert_eq!(lines.len(), 3);
        assert_eq!(lines[2], "-d /dest/include/fuse");
        assert!(
            lines[0] == format!("{}/a.h /dest/include", cwd.display())
                || lines[0] == format!("{}/a.h /dest/include", dir.display())
        );
        assert_eq!(lines[1], "/abs/b.h /dest/include");
        let _ = fs::remove_dir_all(&dir);
    }
}
/* </TESTS> */
