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
//! `/usr/share/zoneinfo` for the base set (M14c): OpenBSD's own `usr.sbin/zic` built for this
//! machine and run as `share/zoneinfo/Makefile`'s `posix_only` target runs it
//! (`zic -d DIR -L /dev/null africa antarctica asia australasia europe northamerica
//! southamerica etcetera factory backward`, in `share/zoneinfo/datfiles`), plus the text
//! files the Makefile installs: `iso3166.tab`, `zone.tab`, `zone1970.tab`, `zonenow.tab`,
//! `leapseconds` (the host's awk over `leapseconds.awk`) and `tzdata.zi` (`ziguard.awk` and
//! `zishrink.awk` the same way). The installer needs the names (`install.sub`'s
//! `set_timezone` lists `/mnt/usr/share/zoneinfo`) and `/etc/localtime` points into it.
//!
//! The host shim, here and nowhere in the sources: a force-included header with no-op
//! `pledge`/`unveil` (macOS has neither) and `__dead`.

use super::ramdisk::write_if_changed;
use super::*;

/// The header force-included into zic.
const COMPAT_H: &str = "\
/* EmiBSD: host shims for building OpenBSD's zic(8) on macOS (tools/xtask, zoneinfo.rs). */
#include <sys/types.h>
#include <stdint.h>
#include <stdlib.h>
#include <errno.h>
/* macOS's libc has no reallocarray(3). */
static inline void *
emibsd_reallocarray(void *p, size_t n, size_t size)
{
	if (size != 0 && n > SIZE_MAX / size) {
		errno = ENOMEM;
		return NULL;
	}
	return realloc(p, n * size);
}
#define reallocarray emibsd_reallocarray
#define pledge(p, e) 0
#define unveil(p, e) 0
#ifndef __dead
#define __dead __attribute__((__noreturn__))
#endif
";

/// The zone source files `share/zoneinfo/Makefile` names in `TDATA`.
const TDATA: &[&str] = &[
    "africa",
    "antarctica",
    "asia",
    "australasia",
    "europe",
    "northamerica",
    "southamerica",
    "etcetera",
    "factory",
    "backward",
];

/// The text files copied as they are (`TABDATA` less the generated ones).
const TABDATA: &[&str] = &["iso3166.tab", "zone.tab", "zone1970.tab", "zonenow.tab"];

/// Runs `awk` with `args` in `dir`, writing its output to `to`.
fn awk(dir: &Path, args: &[&str], stdin_files: &[&str], to: &Path) -> Result<()> {
    let out = Command::new("awk")
        .args(args)
        .args(stdin_files)
        .current_dir(dir)
        .output()
        .map_err(|e| format!("awk: {e}"))?;
    if !out.status.success() {
        return Err(format!(
            "awk {}: {}",
            args.join(" "),
            String::from_utf8_lossy(&out.stderr)
        )
        .into());
    }
    fs::write(to, &out.stdout).map_err(|e| format!("{}: {e}", to.display()).into())
}

/// Makes the tree under `target/userland/<arch>/host/zoneinfo`; returns its path.
pub(super) fn build(ctx: &Ctx<'_>) -> Result<PathBuf> {
    let src = &ctx.src;
    let base = src.join("share/zoneinfo");
    let dat = base.join("datfiles");
    if !dat.is_dir() {
        return Err(format!("{}: not in the reference clone", dat.display()).into());
    }
    let zic_dir = build_host_prog_with(ctx, "usr.sbin/zic", |mk, objdir| {
        let inc = objdir.join("emibsd-include");
        fs::create_dir_all(&inc).map_err(|e| format!("{}: {e}", inc.display()))?;
        write_if_changed(&inc.join("emibsd-compat.h"), COMPAT_H)?;
        let cppflags = mk.var("CPPFLAGS")?;
        mk.set(
            "CPPFLAGS",
            &format!(
                "{cppflags} -w -include {}",
                inc.join("emibsd-compat.h").display()
            ),
        );
        Ok(())
    })?;
    let zic = zic_dir.join("zic");
    let out = ctx.out.join("host/zoneinfo");
    let _ = fs::remove_dir_all(&out);
    fs::create_dir_all(&out).map_err(|e| format!("{}: {e}", out.display()))?;
    let mut cmd = Command::new(&zic);
    cmd.arg("-d")
        .arg(&out)
        .args(["-L", "/dev/null"])
        .args(TDATA)
        .current_dir(&dat);
    run(&mut cmd)?;
    for f in TABDATA {
        fs::copy(dat.join(f), out.join(f)).map_err(|e| format!("{f}: {e}"))?;
    }
    // leapseconds: `awk -v EXPIRES_LINE=0 -f leapseconds.awk leap-seconds.list`.
    awk(
        &base,
        &["-v", "EXPIRES_LINE=0", "-f", "leapseconds.awk"],
        &["datfiles/leap-seconds.list"],
        &out.join("leapseconds"),
    )?;
    // tzdata.zi: ziguard.awk over the zone files, then zishrink.awk over its output.
    let zi = out.join("main.zi");
    let mut ziguard = vec!["-v", "DATAFORM=main", "-f", "../ziguard.awk"];
    ziguard.extend(TDATA);
    awk(&dat, &ziguard, &[], &zi)?;
    let version = fs::read_to_string(base.join("version"))
        .map_err(|e| format!("share/zoneinfo/version: {e}"))?;
    let version = version.lines().next().unwrap_or("").to_string();
    let deps = format!("ziguard.awk {} zishrink.awk", TDATA.join(" "));
    awk(
        &dat,
        &[
            "-v",
            "dataform=main",
            "-v",
            &format!("deps={deps}"),
            "-v",
            "redo=posix_only",
            "-v",
            &format!("version={version}"),
            "-f",
            "../zishrink.awk",
        ],
        &[&zi.to_string_lossy()],
        &out.join("tzdata.zi"),
    )?;
    let _ = fs::remove_file(&zi);
    println!(
        "  share/zoneinfo: {} (OpenBSD's zic built for this machine, `zic -d ... -L /dev/null`)",
        out.display()
    );
    Ok(out)
}

/// `distrib/miniroot/maketz.sh`'s `var/tzlist`: `ls -1dF $(tar cvf /dev/null [A-Za-y]*)` run
/// in the zoneinfo directory `dir`: every name below a top-level name that starts with a
/// letter other than `z`, directories with a trailing `/`, sorted.
pub(super) fn tzlist(dir: &Path) -> Result<String> {
    fn collect(dir: &Path, rel: &str, out: &mut Vec<String>) -> Result<()> {
        let mut entries: Vec<_> = fs::read_dir(dir)
            .map_err(|e| format!("{}: {e}", dir.display()))?
            .collect::<std::result::Result<_, _>>()?;
        entries.sort_by_key(|e| e.file_name());
        for e in entries {
            let name = e.file_name().to_string_lossy().into_owned();
            let path = if rel.is_empty() {
                name.clone()
            } else {
                format!("{rel}/{name}")
            };
            if rel.is_empty()
                && !name
                    .chars()
                    .next()
                    .is_some_and(|c| c.is_ascii_alphabetic() && c != 'z')
            {
                continue;
            }
            if fs::metadata(e.path())?.is_dir() {
                out.push(format!("{path}/"));
                collect(&e.path(), &path, out)?;
            } else {
                out.push(path);
            }
        }
        Ok(())
    }
    let mut lines = Vec::new();
    collect(dir, "", &mut lines)?;
    lines.sort();
    Ok(lines.iter().map(|l| format!("{l}\n")).collect())
}
/* </CODE> */
