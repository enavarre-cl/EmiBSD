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
//! The install media's miniroot (M14c): the ffs image `bsd.rd` boots from, built from
//! OpenBSD's own recipe, `distrib/<arch>/ramdisk_cd/list` (amd64) or
//! `distrib/arm64/ramdisk/list` (arm64), as `distrib/miniroot/list2sh.awk` interprets it.
//!
//! The list is read here, keyword by keyword, with the same meaning as `list2sh.awk`:
//!
//! - `COPY`, `SCRIPT` (a copy with the full-line comments stripped, as its `sed` does),
//!   `LINK`, `SYMLINK`, `MKDIR`, `STRIP`, `REMOVE`, `SPECIAL` (each command of the lists we
//!   build is spelled out below: `rm`, `awk -f trimcerts.awk`, `cd dev; sh MAKEDEV ramdisk`,
//!   `pwd_mkdb`, `chmod`), `TZ`, `TERMCAP`; `SRCDIRS`, `ARGVLINK`, `LIBS` and `CRUNCHSPECIAL`
//!   are crunchgen's and are ignored there too. An entry this build cannot satisfy is
//!   reported in the output (and kept in `miniroot.txt`), never dropped silently.
//! - The directories come from `distrib/miniroot/mtree.conf`, parsed here.
//!
//! Deviations (docs/ARCHITECTURE.md, "The install media"):
//!
//! - No crunched binary. OpenBSD links the programs of the list into one `instbin`
//!   (crunchgen) and `LINK`s every name to it; here each name is the program of that name
//!   that `userland` built (static PIE, from the normal Makefiles, not `distrib/special`'s
//!   `-DSMALL -Oz` ones), copied to the listed place. Bigger, same behaviour. `more` and
//!   `less` are `distrib/special/more` (OpenBSD's tiny pager of the install media), `doas`
//!   is `distrib/special/doas` (the media's root-only one, no `doas.conf`) and `ksh`/`sh`
//!   are `distrib/special/ksh` (the media's `-DSMALL` shell; base's is the full one), built
//!   here, not part of `root/`.
//! - `/dev` is made from the table of the smoke ramdisk (`ramdisk.rs`, `DEVICES`) instead of
//!   running `MAKEDEV ramdisk` on a build host; `/dev/MAKEDEV` is OpenBSD's
//!   (`etc/etc.<arch>/MAKEDEV`) so `install.sub` can make more (`make_dev`).
//! - `usr/mdec/mbr` is a 512-byte stub (no boot code, the 0x55 0xAA signature): the
//!   machines boot through UEFI, where `fdisk(8)` only needs a template for the MBR.
//! - `/etc/signify/openbsd-<rev>-base.pub` is the build's test key (`sets.rs`), never
//!   OpenBSD's. `/auto_install.conf`, when asked for, is the answers of the install run
//!   (`install.rs`): `.profile` then starts `autoinstall` by itself after its 5 s timeout.
//! - Firmware (`etc/firmware/*`), `usr/share/misc/termcap` (`TERMCAP`) and the Raspberry
//!   Pi's `u-boot.bin` are not provided; `TZ` makes `var/tzlist` only when zoneinfo was
//!   built (`sets.rs`).

use std::collections::BTreeMap;

use super::ramdisk::{self, Attr, tree_bytes, write_if_changed};
use super::*;

/// The size of `rd0`'s image in sectors: `bsd.rd` is built with this `EMIBSD_MINIROOTSIZE`
/// (the justfile's `miniroot_sectors`), the image is padded to it, so one kernel serves every
/// run (`rdsetroot` needs the image to fit `rd_root_size`).
pub(crate) const MINIROOT_SECTORS: u64 = 65536;

/// What the build of one miniroot is asked for.
pub(crate) struct Options<'a> {
    /// The signify public key to install as `/etc/signify/openbsd-<rev>-base.pub`.
    pub(crate) pubkey: Option<&'a Path>,
    /// The text of `/auto_install.conf`.
    pub(crate) auto_install_conf: Option<&'a str>,
    /// The image to write (`MINIROOT_SECTORS` sectors).
    pub(crate) image: &'a Path,
}

/// The list of `arch`'s install media, relative to the OpenBSD sources, and its `install.md`.
pub(crate) fn media_files(arch: crate::boot::Arch) -> (&'static str, &'static str) {
    match arch {
        crate::boot::Arch::Amd64 => (
            "distrib/amd64/ramdisk_cd/list",
            "distrib/amd64/common/install.md",
        ),
        crate::boot::Arch::Arm64 => (
            "distrib/arm64/ramdisk/list",
            "distrib/arm64/ramdisk/install.md",
        ),
    }
}

/// `OSrev` of the sources (`80` for 8.0): `OSMAJOR` and `OSMINOR` of `share/mk/sys.mk`.
pub(crate) fn os_rev(src: &Path) -> Result<String> {
    let sys_mk = src.join("share/mk/sys.mk");
    let text = fs::read_to_string(&sys_mk).map_err(|e| format!("{}: {e}", sys_mk.display()))?;
    let get = |name: &str| {
        text.lines()
            .filter_map(|l| l.strip_prefix(name))
            .find_map(|rest| rest.trim_start().strip_prefix('='))
            .map(|v| v.trim().to_string())
            .ok_or_else(|| format!("{}: no {name}", sys_mk.display()))
    };
    Ok(format!("{}{}", get("OSMAJOR")?, get("OSMINOR")?))
}

/// One directory of an mtree(8) specification (`distrib/miniroot/mtree.conf`,
/// `etc/mtree/4.4BSD.dist`).
#[derive(Debug, PartialEq, Eq, Clone)]
pub(crate) struct MtreeDir {
    /// `/etc/ssl`: absolute, no trailing slash.
    pub(crate) path: String,
    pub(crate) mode: u32,
    pub(crate) uname: String,
    pub(crate) gname: String,
}

/// The directories of an mtree specification, in file order, with the `/set` defaults and
/// the `uname=`, `gname=` and `mode=` keywords of each line (`nochange` and the other
/// keywords are not needed).
pub(crate) fn parse_mtree(text: &str) -> Vec<MtreeDir> {
    let mut stack: Vec<String> = Vec::new();
    let mut out = Vec::new();
    let (mut mode, mut uname, mut gname) = (0o755, "root".to_string(), "wheel".to_string());
    for line in text.lines() {
        let line = line.trim();
        if line.is_empty() || line.starts_with('#') {
            continue;
        }
        let keywords = |words: &mut dyn Iterator<Item = &str>,
                        mode: &mut u32,
                        uname: &mut String,
                        gname: &mut String| {
            for w in words {
                if let Some(m) = w.strip_prefix("mode=") {
                    *mode = u32::from_str_radix(m, 8).unwrap_or(*mode);
                } else if let Some(u) = w.strip_prefix("uname=") {
                    *uname = u.to_string();
                } else if let Some(g) = w.strip_prefix("gname=") {
                    *gname = g.to_string();
                }
            }
        };
        if let Some(rest) = line.strip_prefix("/set") {
            keywords(
                &mut rest.split_whitespace(),
                &mut mode,
                &mut uname,
                &mut gname,
            );
            continue;
        }
        if line == ".." {
            stack.pop();
            continue;
        }
        let mut words = line.split_whitespace();
        let name = words.next().unwrap_or(".");
        if name == "." {
            stack.clear();
            continue;
        }
        let (mut m, mut u, mut g) = (mode, uname.clone(), gname.clone());
        keywords(&mut words, &mut m, &mut u, &mut g);
        stack.push(name.to_string());
        out.push(MtreeDir {
            path: format!("/{}", stack.join("/")),
            mode: m,
            uname: u,
            gname: g,
        });
    }
    out
}

/// The directories of `mtree.conf` with their modes: (path, mode).
pub(crate) fn mtree_dirs(text: &str) -> Vec<(String, u32)> {
    parse_mtree(text)
        .into_iter()
        .map(|d| (d.path, d.mode))
        .collect()
}

/// `list2sh.awk`'s `SCRIPT`: `sed -e '/^[ \t]*#[ \t].*$/d' -e '/^[ \t]*#$/d'`.
pub(crate) fn strip_script_comments(text: &str) -> String {
    let mut out = String::new();
    for line in text.lines() {
        let t = line.trim_start_matches([' ', '\t']);
        let comment = t == "#" || (t.starts_with('#') && t[1..].starts_with([' ', '\t']));
        if !comment {
            out.push_str(line);
            out.push('\n');
        }
    }
    out
}

/// One line of a `list` file: the keyword and its words, with `${CURDIR}`, `${DESTDIR}` and
/// the other variables left as they are.
#[derive(Debug, PartialEq, Eq)]
pub(crate) struct Entry {
    pub(crate) keyword: String,
    pub(crate) args: Vec<String>,
    /// The line as written, for messages.
    pub(crate) line: String,
}

/// The entries of a `list` file (comments and blank lines dropped).
pub(crate) fn parse_list(text: &str) -> Vec<Entry> {
    text.lines()
        .filter(|l| !l.trim().is_empty() && !l.trim_start().starts_with('#'))
        .map(|l| {
            let mut w = l.split_whitespace().map(str::to_string);
            let keyword = w.next().unwrap_or_default();
            Entry {
                keyword,
                args: w.collect(),
                line: l.to_string(),
            }
        })
        .collect()
}

/// The staged tree being built, and what could not be.
struct Stage {
    root: PathBuf,
    /// Attributes that differ from root:wheel and the file's own mode.
    attrs: Vec<Attr>,
    /// Entries of the list this build could not satisfy: (line, why).
    missing: Vec<(String, String)>,
    /// `userland`'s staging root, `target/userland/<arch>/root`.
    userland_root: PathBuf,
    /// Where each program of `userland` is: basename → path in `root/`.
    programs: BTreeMap<String, PathBuf>,
    /// `userland`'s ownership table: (path in the image) → (mode, uid, gid).
    owners: BTreeMap<String, (u32, u32, u32)>,
    /// Programs built for the miniroot only (`more`): basename → file.
    only: BTreeMap<String, PathBuf>,
}

impl Stage {
    fn path(&self, rel: &str) -> PathBuf {
        self.root.join(rel.trim_start_matches('/'))
    }

    fn put_file(&mut self, rel: &str, from: &Path, mode: Option<u32>) -> Result<()> {
        let to = self.path(rel);
        if let Some(dir) = to.parent() {
            fs::create_dir_all(dir).map_err(|e| format!("{}: {e}", dir.display()))?;
        }
        let _ = fs::remove_file(&to);
        fs::copy(from, &to).map_err(|e| format!("cp {} {}: {e}", from.display(), to.display()))?;
        if let Some(mode) = mode {
            self.attrs.push(Attr::root(
                &format!("/{}", rel.trim_start_matches('/')),
                0,
                mode,
            ));
        }
        Ok(())
    }

    fn put_text(&mut self, rel: &str, text: &str, mode: u32) -> Result<()> {
        let to = self.path(rel);
        if let Some(dir) = to.parent() {
            fs::create_dir_all(dir).map_err(|e| format!("{}: {e}", dir.display()))?;
        }
        let _ = fs::remove_file(&to);
        fs::write(&to, text).map_err(|e| format!("{}: {e}", to.display()))?;
        self.attrs.push(Attr::root(
            &format!("/{}", rel.trim_start_matches('/')),
            0,
            mode,
        ));
        Ok(())
    }

    fn miss(&mut self, line: &str, why: &str) {
        self.missing.push((line.to_string(), why.to_string()));
    }

    /// The file of `userland` for the program `rel` names (`usr/bin/gzip`): the same path
    /// in `root/` first, else the program of that name anywhere in it.
    fn program(&self, rel: &str) -> Option<PathBuf> {
        let base = Path::new(rel).file_name()?.to_str()?;
        if let Some(p) = self.only.get(base) {
            return Some(p.clone());
        }
        let p = self.path_in_root(rel);
        if p.is_file() {
            return Some(p);
        }
        self.programs.get(base).cloned()
    }

    fn path_in_root(&self, rel: &str) -> PathBuf {
        self.userland_root.join(rel.trim_start_matches('/'))
    }
}

/// `userland`'s ownership table: `mode uid gid path` lines.
fn read_owners(path: &Path) -> BTreeMap<String, (u32, u32, u32)> {
    let mut map = BTreeMap::new();
    let Ok(text) = fs::read_to_string(path) else {
        return map;
    };
    for line in text.lines() {
        let mut w = line.split_whitespace();
        if let (Some(m), Some(u), Some(g), Some(p)) = (w.next(), w.next(), w.next(), w.next())
            && let (Ok(m), Ok(u), Ok(g)) = (
                u32::from_str_radix(m, 8),
                u.parse::<u32>(),
                g.parse::<u32>(),
            )
        {
            map.insert(p.to_string(), (m, u, g));
        }
    }
    map
}

/// The 512-byte MBR template of `usr/mdec/mbr` (module docs).
pub(crate) fn mbr_stub() -> Vec<u8> {
    let mut mbr = vec![0u8; 512];
    mbr[510] = 0x55;
    mbr[511] = 0xaa;
    mbr
}

/// Every file under `dir`, as (path relative to `dir`, absolute path).
fn walk_files(dir: &Path, rel: &str, out: &mut Vec<(String, PathBuf)>) -> Result<()> {
    let mut entries: Vec<_> = fs::read_dir(dir)
        .map_err(|e| format!("{}: {e}", dir.display()))?
        .collect::<std::result::Result<_, _>>()?;
    entries.sort_by_key(|e| e.file_name());
    for e in entries {
        let name = e.file_name().to_string_lossy().into_owned();
        let sub = if rel.is_empty() {
            name.clone()
        } else {
            format!("{rel}/{name}")
        };
        let md = fs::symlink_metadata(e.path())?;
        if md.is_dir() {
            walk_files(&e.path(), &sub, out)?;
        } else {
            out.push((sub, e.path()));
        }
    }
    Ok(())
}

/// Programs the miniroot builds from `distrib/special` and base does not use: (directory, name).
/// `init` is built `-DDEFAULT_STATE=single_user` and without `DEBUGSHELL`/`SECURE` (the install
/// media runs the installer as its single-user shell, no questions asked); `more` is the
/// media's small pager; `doas` is the media's own (a root-only `doas -u user command`
/// without `/etc/doas.conf`, which `install.sub`'s `unpriv` runs `ftp` and `signify`
/// through; base's `usr.bin/doas` refuses without a config); `ksh` is the media's shell,
/// `-DSMALL` (no `KSH_VERSION`, no mail check, no persistent history, no curses), while
/// base's `/bin/ksh` is `bin/ksh`'s full build. Their Makefiles install into `/usr/bin` or
/// nowhere, which is undone: they go to `target/userland/<arch>/miniroot-only/`. A name
/// here, and each name its Makefile's `LINKS` make (the third field), wins over the base
/// program of that name (`Stage::program`), so the list's `ksh` and `sh` are this `ksh`.
const MINIROOT_ONLY: &[(&str, &str, &[&str])] = &[
    ("distrib/special/more", "more", &[]),
    ("distrib/special/init", "init", &[]),
    ("distrib/special/doas", "doas", &[]),
    ("distrib/special/ksh", "ksh", &["rksh", "sh"]),
];

/// Nodes `MAKEDEV ramdisk` makes that the smoke ramdisk's table (`ramdisk::devices`) lacks:
/// (name, kind, major, minor, mode, group). `diskmap` (major 90 on amd64 and arm64,
/// `M diskmap c 90 0 640 operator`) is how opendev(3) opens a disk by its DUID, which
/// `install.sub` names every partition by.
const MINIROOT_DEVICES: &[(&str, char, u32, u32, u32, &str)] =
    &[("diskmap", 'c', 90, 0, 0o640, "operator")];

/// Builds `dir` of `MINIROOT_ONLY` for the miniroot only: its executable. `links` are the
/// names its `LINKS` made beside it, removed from `root/` as the program is.
fn build_only(ctx: &Ctx<'_>, dir: &str, name: &str, links: &[&str]) -> Result<Option<PathBuf>> {
    let installed = match build_prog(ctx, dir)? {
        Linked::Yes(_, _, installed) => installed,
        _ => return Ok(None),
    };
    let keep = ctx.out.join("miniroot-only");
    fs::create_dir_all(&keep).map_err(|e| format!("{}: {e}", keep.display()))?;
    let to = keep.join(name);
    let _ = fs::remove_file(&to);
    fs::copy(&installed, &to).map_err(|e| format!("{}: {e}", to.display()))?;
    // Their Makefiles name no BINDIR, so build_prog put them (and their links) at the top
    // of `root/`: not base's.
    let _ = fs::remove_file(&installed);
    if let Some(dir) = installed.parent() {
        for link in links {
            let _ = fs::remove_file(dir.join(link));
        }
    }
    Ok(Some(to))
}

/// Builds the miniroot image: writes `opts.image`, and `miniroot.txt` (what the list asked
/// for and was not built) beside it. Returns the number of entries not satisfied.
pub(crate) fn build(ctx: &Ctx<'_>, arch: crate::boot::Arch, opts: &Options<'_>) -> Result<usize> {
    let (list_rel, install_md_rel) = media_files(arch);
    let src = &ctx.src;
    let rev = os_rev(src)?;
    let list_text =
        fs::read_to_string(src.join(list_rel)).map_err(|e| format!("{list_rel}: {e}"))?;
    let makefs = ramdisk::build_makefs(ctx)?;
    let pwd_mkdb = passwd::build_pwd_mkdb(ctx)?;

    let root = ctx.out.join("miniroot-root");
    if root.exists() {
        fs::remove_dir_all(&root).map_err(|e| format!("{}: {e}", root.display()))?;
    }
    fs::create_dir_all(&root).map_err(|e| format!("{}: {e}", root.display()))?;
    let userland_root = ctx.out.join("root");
    let mut programs = BTreeMap::new();
    let mut files = Vec::new();
    walk_files(&userland_root, "", &mut files)?;
    for (rel, path) in files {
        if let Some(base) = Path::new(&rel).file_name().and_then(|b| b.to_str()) {
            // `bin/` and `sbin/` win over `usr/...` copies of a name.
            let first = !rel.starts_with("usr/lib");
            if first {
                programs.entry(base.to_string()).or_insert(path);
            }
        }
    }
    let mut stage = Stage {
        root: root.clone(),
        userland_root: userland_root.clone(),
        attrs: Vec::new(),
        missing: Vec::new(),
        programs,
        owners: read_owners(&ctx.out.join("host/owners.txt")),
        only: BTreeMap::new(),
    };
    for (dir, name, links) in MINIROOT_ONLY {
        if let Some(p) = build_only(ctx, dir, name, links)? {
            for link in *links {
                stage.only.insert((*link).to_string(), p.clone());
            }
            stage.only.insert((*name).to_string(), p);
        }
    }

    // The directories, `mtree.conf`.
    let mtree = fs::read_to_string(src.join("distrib/miniroot/mtree.conf"))
        .map_err(|e| format!("distrib/miniroot/mtree.conf: {e}"))?;
    for (dir, mode) in mtree_dirs(&mtree) {
        let p = stage.path(&dir);
        fs::create_dir_all(&p).map_err(|e| format!("{}: {e}", p.display()))?;
        if mode != 0o755 {
            stage.attrs.push(Attr::root(&dir, 0, mode));
        }
    }

    let curdir = list_rel.rsplit_once('/').map_or("", |(d, _)| d).to_string();
    let miniroot_dir = "distrib/miniroot";
    // `${CURDIR}/../../miniroot/x`, `${CURDIR}/../common/install.md` and `${DESTDIR}/...`.
    let resolve = |word: &str| -> Option<PathBuf> {
        let w = word
            .replace("${OSrev}", &rev)
            .replace("${UTILS}", miniroot_dir);
        if let Some(rest) = w.strip_prefix("${CURDIR}/") {
            let joined = Path::new(&curdir).join(rest);
            return normalize(&joined).map(|p| src.join(p));
        }
        None
    };

    let mut unsatisfied = 0;
    for e in parse_list(&list_text) {
        let a = &e.args;
        match e.keyword.as_str() {
            "SRCDIRS" | "ARGVLINK" | "LIBS" | "CRUNCHSPECIAL" => {}
            "COPY" if a.len() == 2 => {
                // list2sh.awk's lines are shell commands: `${OSrev}` expands on both sides
                // (`etc/signify/openbsd-${OSrev}-base.pub`).
                let (from, to) = (&a[0], &a[1].replace("${OSrev}", &rev));
                let word = from.replace("${OSrev}", &rev);
                if word.starts_with("${OBJDIR}/instbin") {
                    // The crunched binary: not built (module docs).
                } else if word == "${DESTDIR}/usr/mdec/mbr" {
                    let tmp = stage.path("usr/mdec/mbr");
                    fs::write(&tmp, mbr_stub()).map_err(|e| format!("{}: {e}", tmp.display()))?;
                } else if word.starts_with("${DESTDIR}/etc/signify/") {
                    match opts.pubkey {
                        Some(p) => stage.put_file(to, p, Some(0o644))?,
                        None => stage.miss(&e.line, "no signify test key given"),
                    }
                } else if let Some(p) = resolve(&word) {
                    stage.put_file(to, &p, None)?;
                } else if word.starts_with("${DESTDIR}/etc/firmware/") {
                    stage.miss(
                        &e.line,
                        "firmware is not built (no device that needs it in QEMU)",
                    );
                    unsatisfied += 1;
                } else {
                    stage.miss(&e.line, "source is outside the build (not provided)");
                    unsatisfied += 1;
                }
            }
            "SCRIPT" if a.len() == 2 => {
                let (from, to) = (&a[0], &a[1].replace("${OSrev}", &rev));
                let word = from.replace("${OSrev}", &rev);
                let path = if word == "${DESTDIR}/dev/MAKEDEV" {
                    Some(src.join(format!("etc/etc.{}/MAKEDEV", arch.name())))
                } else if word.contains("install.md") {
                    Some(src.join(install_md_rel))
                } else {
                    resolve(&word)
                };
                match path {
                    Some(p) => {
                        let text =
                            fs::read_to_string(&p).map_err(|e| format!("{}: {e}", p.display()))?;
                        // `SPECIAL chmod 755 install.sub` follows in the list; MAKEDEV is
                        // installed 555 by `etc/Makefile`.
                        let mode = if to.as_str() == "dev/MAKEDEV" {
                            0o555
                        } else {
                            0o644
                        };
                        stage.put_text(to, &strip_script_comments(&text), mode)?;
                    }
                    None => {
                        stage.miss(&e.line, "source not found");
                        unsatisfied += 1;
                    }
                }
            }
            "LINK" if a.len() >= 2 => {
                // `LINK instbin a b c`: every name is the program of that name; one with no
                // program of its own (`less`, `ftp-ssl`) is the line's first program (the
                // crunched binary picks by argv[0], and these are aliases).
                let first = a[1..].iter().find_map(|n| stage.program(n));
                for name in &a[1..] {
                    match stage.program(name).or_else(|| first.clone()) {
                        Some(p) => {
                            let mode = stage
                                .owners
                                .get(&format!("/{name}"))
                                .or_else(|| {
                                    stage.owners.get(
                                        &p.strip_prefix(&userland_root)
                                            .map(|r| format!("/{}", r.display()))
                                            .unwrap_or_default(),
                                    )
                                })
                                .map(|(m, _, _)| *m);
                            stage.put_file(name, &p, mode)?;
                            let to = stage.path(name);
                            set_exec(&to)?;
                        }
                        None => {
                            stage.miss(
                                &format!("LINK {name}"),
                                "no such program in this build (see ARCHITECTURE.md, \"The install media\")",
                            );
                            unsatisfied += 1;
                        }
                    }
                }
            }
            "SYMLINK" if a.len() >= 2 => {
                for name in &a[1..] {
                    let to = stage.path(name);
                    if let Some(dir) = to.parent() {
                        fs::create_dir_all(dir)?;
                    }
                    let _ = fs::remove_file(&to);
                    std::os::unix::fs::symlink(&a[0], &to)
                        .map_err(|e| format!("ln -s {} {name}: {e}", a[0]))?;
                }
            }
            "MKDIR" if a.len() == 1 => {
                fs::create_dir_all(stage.path(&a[0]))?;
            }
            "REMOVE" if a.len() == 1 => {
                let _ = fs::remove_file(stage.path(&a[0]));
            }
            "SPECIAL" => {
                let cmd = e.line.trim_start().trim_start_matches("SPECIAL").trim();
                special(ctx, &mut stage, &pwd_mkdb, cmd, &e.line, arch)?;
            }
            "TZ" => {
                // `maketz.sh`: the list of zoneinfo names, from the zic output (`zoneinfo.rs`).
                match zoneinfo::build(ctx).and_then(|z| zoneinfo::tzlist(&z)) {
                    Ok(text) => stage.put_text("var/tzlist", &text, 0o644)?,
                    Err(err) => stage.miss(&e.line, &format!("no zoneinfo: {err}")),
                }
            }
            "TERMCAP" => {
                stage.miss(&e.line, "no termcap built (share/termtypes needs tic(1)); /usr/share/misc/termcap not made");
            }
            "COPY" | "SCRIPT" | "LINK" | "SYMLINK" | "MKDIR" | "REMOVE" | "STRIP" | "COPYDIR" => {
                stage.miss(&e.line, "unsupported form of the keyword");
                unsatisfied += 1;
            }
            other => {
                return Err(format!("{list_rel}: unknown keyword `{other}`: {}", e.line).into());
            }
        }
    }

    // install.sub and the profile are executable scripts.
    for exe in ["install.sub", "dev/MAKEDEV"] {
        let p = stage.path(exe);
        if p.is_file() {
            set_exec(&p)?;
        }
    }

    // `/dev`.
    let dev = stage.path("dev");
    fs::create_dir_all(dev.join("fd")).map_err(|e| format!("{}: {e}", dev.display()))?;
    let device = |name: &str, kind: char, major: u32, minor: u32, mode: u32| -> Result<()> {
        let p = dev.join(name);
        fs::write(
            &p,
            format!(
                "{} {kind} {major} {minor} {mode:o}\n",
                ramdisk::DEVICE_MAGIC
            ),
        )
        .map_err(|e| format!("{}: {e}", p.display()).into())
    };
    for (name, kind, major, minor, mode, group) in ramdisk::devices().into_iter().chain(
        MINIROOT_DEVICES
            .iter()
            .map(|&(n, k, ma, mi, mo, g)| (n.to_string(), k, ma, mi, mo, g)),
    ) {
        device(&name, kind, major, minor, mode)?;
        stage.attrs.push(Attr::root(
            &format!("/dev/{name}"),
            ramdisk::group_id(group).unwrap_or(0),
            mode,
        ));
    }
    for n in 0..ramdisk::FD_NODES {
        device(&format!("fd/{n}"), 'c', 22, n, 0o666)?;
        stage
            .attrs
            .push(Attr::root(&format!("/dev/fd/{n}"), 0, 0o666));
    }
    for (name, target) in ramdisk::DEV_LINKS {
        symlink(target, &dev.join(name))?;
    }

    // `/auto_install.conf` and the answers' home.
    if let Some(conf) = opts.auto_install_conf {
        stage.put_text("auto_install.conf", conf, 0o644)?;
    }
    stage.attrs.push(Attr::root("/tmp", 0, 0o1777));
    stage.attrs.push(Attr::root("/var/tmp", 0, 0o1777));

    // The image: partition `a` is the whole `MINIROOT_SECTORS`.
    let size = tree_bytes(&root)?;
    let capacity = MINIROOT_SECTORS * 512;
    if size * 5 / 4 > capacity {
        return Err(format!(
            "miniroot: the tree is {size} bytes, too big for {MINIROOT_SECTORS} sectors \
             (raise MINIROOT_SECTORS and the justfile's miniroot_sectors)"
        )
        .into());
    }
    let owners = ctx.out.join("host/miniroot-owners.txt");
    write_if_changed(&owners, &ramdisk::owners_table(&last_wins(&stage.attrs)))?;
    let disktab = ctx.out.join("host/miniroot-disktab");
    write_if_changed(&disktab, &ramdisk::disktab_entry(MINIROOT_SECTORS))?;
    if let Some(dir) = opts.image.parent() {
        fs::create_dir_all(dir).map_err(|e| format!("{}: {e}", dir.display()))?;
    }
    let _ = fs::remove_file(opts.image);
    run(Command::new(&makefs)
        .args(["-t", "ffs", "-T", &ramdisk::TIMESTAMP.to_string()])
        .env("EMIBSD_DISKTAB", &disktab)
        .env("EMIBSD_OWNERS", &owners)
        .env("EMIBSD_STAGING", &root)
        .args(["-o", ramdisk::FS_OPTIONS])
        .arg(opts.image)
        .arg(&root))?;
    let len = fs::metadata(opts.image).map(|m| m.len()).unwrap_or(0);
    if len != capacity {
        return Err(format!(
            "miniroot: makefs wrote {len} bytes, expected {capacity} ({} sectors)",
            MINIROOT_SECTORS
        )
        .into());
    }

    let report: String = stage
        .missing
        .iter()
        .map(|(l, why)| format!("{l}\t# {why}\n"))
        .collect();
    let txt = opts.image.with_file_name("miniroot.txt");
    write_if_changed(&txt, &report)?;
    println!(
        "  miniroot ({list_rel}): {} ({len} bytes, tree {size}); {} list entr{} not satisfied, \
         see {}",
        opts.image.display(),
        stage.missing.len(),
        if stage.missing.len() == 1 { "y" } else { "ies" },
        txt.display()
    );
    Ok(unsatisfied)
}

/// The attributes with one entry per path, the last one given winning (a `COPY`'s mode is
/// overridden by a later `SPECIAL chmod`, as in the shell script `list2sh.awk` makes).
fn last_wins(attrs: &[Attr]) -> Vec<Attr> {
    let mut seen = std::collections::BTreeSet::new();
    let mut out: Vec<Attr> = attrs
        .iter()
        .rev()
        .filter(|a| seen.insert(a.path.clone()))
        .cloned()
        .collect();
    out.reverse();
    out
}

/// `bsd.rd`: the kernel (built with feature `miniroot`) with its debug information stripped
/// (OpenBSD strips `bsd.rd` too: `objcopy -g -x ...`) and `image` put in by `rdsetroot`.
pub(crate) fn make_bsd_rd(ctx: &Ctx<'_>, kernel: &Path, image: &Path, out: &Path) -> Result<()> {
    if let Some(dir) = out.parent() {
        fs::create_dir_all(dir).map_err(|e| format!("{}: {e}", dir.display()))?;
    }
    let _ = fs::remove_file(out);
    run(Command::new(&ctx.tools.objcopy)
        .arg("--strip-debug")
        .arg(kernel)
        .arg(out))?;
    let o = out.to_string_lossy().into_owned();
    let i = image.to_string_lossy().into_owned();
    crate::rdsetroot::rdsetroot(&[o.as_str(), i.as_str()])
}

/// Makes `p` executable (the staging copies of programs keep the build's mode; scripts get
/// 0755).
fn set_exec(p: &Path) -> Result<()> {
    use std::os::unix::fs::PermissionsExt as _;
    fs::set_permissions(p, fs::Permissions::from_mode(0o755))
        .map_err(|e| format!("chmod {}: {e}", p.display()).into())
}

/// `a/b/../c` → `a/c`; `None` if it climbs out.
fn normalize(p: &Path) -> Option<PathBuf> {
    let mut out = PathBuf::new();
    for c in p.components() {
        match c {
            std::path::Component::ParentDir => {
                if !out.pop() {
                    return None;
                }
            }
            std::path::Component::Normal(n) => out.push(n),
            _ => {}
        }
    }
    Some(out)
}

/// One `SPECIAL` command of the lists (module docs): the ones the lists use, spelled out.
fn special(
    ctx: &Ctx<'_>,
    stage: &mut Stage,
    pwd_mkdb: &Path,
    cmd: &str,
    line: &str,
    arch: crate::boot::Arch,
) -> Result<()> {
    let _ = arch;
    let words: Vec<&str> = cmd.split_whitespace().collect();
    match words.as_slice() {
        ["rm", f] => {
            let _ = fs::remove_file(stage.path(f));
        }
        ["chmod", mode, f] => {
            let m = u32::from_str_radix(mode, 8)
                .map_err(|e| format!("{line}: bad mode `{mode}`: {e}"))?;
            stage
                .attrs
                .push(Attr::root(&format!("/{}", f.trim_start_matches('/')), 0, m));
        }
        ["awk", "-f", _, from, to] if from.ends_with("etc/ssl/cert.pem") => {
            // `trimcerts.awk`: only the certificates the install needs.
            let awk = ctx.src.join("distrib/miniroot/trimcerts.awk");
            let cert = ctx.src.join("lib/libcrypto/cert.pem");
            let p = stage.path(to);
            if let Some(d) = p.parent() {
                fs::create_dir_all(d)?;
            }
            // The script writes its output file itself (`print > ARGV[2]`) and then runs
            // `chown root:bin`, which fails without root: its exit status is not checked.
            let status = Command::new("awk")
                .arg("-f")
                .arg(&awk)
                .arg(&cert)
                .arg(&p)
                .stderr(Stdio::null())
                .status()
                .map_err(|e| format!("awk: {e}"))?;
            if !p.is_file() || fs::metadata(&p).map_or(true, |m| m.len() == 0) {
                stage.miss(line, &format!("trimcerts.awk made nothing ({status})"));
            }
            let _ = fs::set_permissions(&p, std::os::unix::fs::PermissionsExt::from_mode(0o644));
        }
        ["cd", "dev;", "sh", "MAKEDEV", "ramdisk"] => {
            // `/dev` is written from `ramdisk::devices()` (module docs).
        }
        ["pwd_mkdb", ..] => {
            let etc = stage.path("etc");
            passwd::make_databases(pwd_mkdb, &etc)?;
            let _ = fs::remove_file(etc.join("master.passwd"));
            let shadow = ramdisk::group_id("_shadow").unwrap_or(0);
            stage.attrs.push(Attr::root("/etc/spwd.db", shadow, 0o640));
            stage.attrs.push(Attr::root("/etc/passwd", 0, 0o644));
            stage.attrs.push(Attr::root("/etc/pwd.db", 0, 0o644));
        }
        _ => {
            stage.miss(line, "SPECIAL command not understood by this build");
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
    fn mtree_conf_gives_the_directories() {
        let dirs = mtree_dirs(
            "/set type=dir uname=root gname=wheel mode=0755\n\n.\n\nbin\n..\netc\n    ssl\n    ..\n..\ntmp\t\tmode=01777\n..\nusr\n    share\n        misc\n        ..\n    ..\n..\n..\n",
        );
        let names: Vec<&str> = dirs.iter().map(|d| d.0.as_str()).collect();
        assert_eq!(
            names,
            [
                "/bin",
                "/etc",
                "/etc/ssl",
                "/tmp",
                "/usr",
                "/usr/share",
                "/usr/share/misc"
            ]
        );
        assert_eq!(dirs[3].1, 0o1777);
        assert_eq!(dirs[0].1, 0o755);
    }

    #[test]
    fn script_comments_are_stripped_like_sed() {
        let text =
            "#!/bin/sh\n# a comment\n#\n\t# indented\necho hi # not a full-line comment\n#x\n";
        assert_eq!(
            strip_script_comments(text),
            "#!/bin/sh\necho hi # not a full-line comment\n#x\n"
        );
    }

    #[test]
    fn list_entries_have_keyword_and_words() {
        let e = parse_list("# c\nSRCDIRS distrib/special\n\nLINK\tinstbin\t\tbin/ksh bin/sh\n");
        assert_eq!(e.len(), 2);
        assert_eq!(e[1].keyword, "LINK");
        assert_eq!(e[1].args, ["instbin", "bin/ksh", "bin/sh"]);
    }

    #[test]
    fn real_lists_use_only_keywords_this_build_knows() {
        for arch in [crate::boot::Arch::Amd64, crate::boot::Arch::Arm64] {
            let (list, _) = media_files(arch);
            let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../reference/openbsd-src");
            let Ok(text) = fs::read_to_string(root.join(list)) else {
                continue;
            };
            for e in parse_list(&text) {
                assert!(
                    matches!(
                        e.keyword.as_str(),
                        "SRCDIRS"
                            | "COPY"
                            | "LINK"
                            | "ARGVLINK"
                            | "SPECIAL"
                            | "SYMLINK"
                            | "SCRIPT"
                            | "TZ"
                            | "TERMCAP"
                            | "MKDIR"
                            | "REMOVE"
                            | "LIBS"
                            | "CRUNCHSPECIAL"
                            | "STRIP"
                            | "COPYDIR"
                    ),
                    "{list}: {}",
                    e.line
                );
            }
        }
    }

    #[test]
    fn paths_are_normalised() {
        assert_eq!(
            normalize(Path::new("distrib/amd64/ramdisk_cd/../../miniroot/group")),
            Some(PathBuf::from("distrib/miniroot/group"))
        );
        assert_eq!(normalize(Path::new("../x")), None);
    }
}
/* </TESTS> */
