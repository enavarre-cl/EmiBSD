//! Step 5 of `cargo xtask userland`: the ffs ramdisk image, `target/userland/<arch>/ramdisk.ffs`.
//!
//! The image is made by OpenBSD's own makefs(8) (`usr.sbin/makefs`, in the reference clone
//! since 2026-10-03, the user's decision), built for this machine like `rpcgen`, not by a
//! file system writer of our own. It is invoked as OpenBSD's `distrib/` makes its ramdisks
//! (`-o disklabel=rdroot,minfree=0,density=4096`), with `rdroot=1` in place of the `rdroot`
//! entry of `/etc/disktab` (makefs builds the same one-partition label itself): rd(4) needs a
//! disklabel, and `rd0a` is the file system.
//!
//! makefs is written for OpenBSD only. Building it on macOS takes these host shims, all here
//! and none in the sources (`docs/ARCHITECTURE.md`, "Userland build"):
//!
//! - a force-included header (`COMPAT_H`): `daddr_t` is 64 bits on OpenBSD and 32 on macOS;
//!   `st_atim`/`st_mtim`/`st_ctim` are `st_*timespec` on macOS; `MAXBSIZE` is OpenBSD's
//!   64 KiB, not macOS's 1 MiB; `pledge`, `unveil` and `srandom_deterministic` (used only
//!   with `-T`) have no macOS counterpart;
//! - OpenBSD's own headers macOS lacks, from the clone: `ufs/`, `msdosfs/`,
//!   `sys/disklabel.h`, `machine/disklabel.h` (identical on amd64 and arm64), and
//!   `sys/uuid.h` with its `uuid_t` renamed (macOS has a different `uuid_t`); `sys/endian.h`
//!   is written here over `<libkern/OSByteOrder.h>`;
//! - `scan_scaled` from OpenBSD's `lib/libutil/fmt_scaled.c` (macOS's libutil lacks it);
//! - `lstat` wrapped (`COMPAT_C`) so that device nodes can be made without root: macOS
//!   lets only root `mknod`, and OpenBSD's makefs has no mtree spec. A regular file in the
//!   staging tree that holds exactly one `DEVICE_MAGIC` line is reported as that device,
//!   with OpenBSD's `makedev()` encoding of `st_rdev`.

use super::*;

/// makefs, relative to the OpenBSD sources.
const MAKEFS_DIR: &str = "usr.sbin/makefs";

/// The first word of a device-node placeholder file (see `COMPAT_C`).
const DEVICE_MAGIC: &str = "emibsd-makefs-device";

/// The device nodes the root needs before anything can run MAKEDEV: what `init(8)` opens
/// (`/dev/console`), what a shell expects (`/dev/tty`, `/dev/null`). Majors and minors are
/// those of OpenBSD's `MAKEDEV` and of `cdevsw[]` in `sys/arch/{amd64,arm64}/{amd64,arm64}/conf.c`,
/// the same on both architectures: `cn` 0, `ctty` 1, `mm` 2 (`null` is its minor 2).
/// (name, kind, major, minor, mode)
const DEVICES: &[(&str, char, u32, u32, u32)] = &[
    ("console", 'c', 0, 0, 0o600),
    ("tty", 'c', 1, 0, 0o666),
    ("null", 'c', 2, 2, 0o666),
];

/// makefs's ffs options: those of OpenBSD's `distrib/` ramdisks, with `rdroot=1` and the
/// block and fragment sizes of the `rdroot` entry of OpenBSD's `/etc/disktab` (`ba#4096`,
/// `fa#512`) in place of `disklabel=rdroot`, which reads that file.
const FS_OPTIONS: &str = "rdroot=1,bsize=4096,fsize=512,minfree=0,density=4096";

/// A fixed timestamp (`makefs -T`) so that the image is reproducible: 2026-10-02, the date of
/// the reference pin.
const TIMESTAMP: u64 = 1_790_899_200;

/// The header force-included (`-include`) into every makefs source.
const COMPAT_H: &str = "\
/* EmiBSD: host shims for building OpenBSD's makefs(8) on macOS (tools/xtask, ramdisk.rs). */
#include <sys/types.h>
#include <sys/param.h>
#include <sys/stat.h>
#include <sys/endian.h>
#include <stdint.h>
#include <stdlib.h>
#include <time.h>
#define daddr_t int64_t
#define st_atim st_atimespec
#define st_mtim st_mtimespec
#define st_ctim st_ctimespec
#undef MAXBSIZE
#define MAXBSIZE (64 * 1024)
#define pledge(p, e) 0
#define unveil(p, f) 0
#define srandom_deterministic(s) srandom(s)
int emibsd_lstat(const char *, struct stat *);
#define lstat(p, sb) emibsd_lstat(p, sb)
int scan_scaled(char *, long long *);
";

/// `<sys/endian.h>` (OpenBSD names over macOS's byte-order primitives).
const ENDIAN_H: &str = "\
/* EmiBSD: OpenBSD's <sys/endian.h> names for the makefs host build (tools/xtask). */
#ifndef EMIBSD_HOST_SYS_ENDIAN_H
#define EMIBSD_HOST_SYS_ENDIAN_H
#include <libkern/OSByteOrder.h>
#define htole16(x) OSSwapHostToLittleInt16(x)
#define htole32(x) OSSwapHostToLittleInt32(x)
#define htole64(x) OSSwapHostToLittleInt64(x)
#define letoh16(x) OSSwapLittleToHostInt16(x)
#define letoh32(x) OSSwapLittleToHostInt32(x)
#define letoh64(x) OSSwapLittleToHostInt64(x)
#define htobe16(x) OSSwapHostToBigInt16(x)
#define htobe32(x) OSSwapHostToBigInt32(x)
#define htobe64(x) OSSwapHostToBigInt64(x)
#define betoh16(x) OSSwapBigToHostInt16(x)
#define betoh32(x) OSSwapBigToHostInt32(x)
#define betoh64(x) OSSwapBigToHostInt64(x)
#define swap16(x) OSSwapInt16(x)
#define swap32(x) OSSwapInt32(x)
#define swap64(x) OSSwapInt64(x)
#endif
";

/// `lstat` that reports device-node placeholders as devices.
const COMPAT_C: &str = r#"/* EmiBSD: lstat for the makefs host build (tools/xtask, ramdisk.rs). */
#include <sys/types.h>
#include <sys/stat.h>
#include <stdio.h>
#include <string.h>

#undef lstat	/* the force-included header points lstat here */

#define MAGIC "@MAGIC@"

/* OpenBSD's makedev() (sys/types.h), not macOS's. */
#define OPENBSD_MAKEDEV(x, y) \
	((dev_t)((((x) & 0xff) << 8) | ((y) & 0xff) | (((y) & 0xffff00) << 8)))

int
emibsd_lstat(const char *path, struct stat *sb)
{
	char kind;
	unsigned int maj, min, mode;
	char line[128];
	FILE *f;
	int n;

	if (lstat(path, sb) == -1)
		return -1;
	if (!S_ISREG(sb->st_mode) || sb->st_size == 0 ||
	    sb->st_size >= (off_t)sizeof(line))
		return 0;
	if ((f = fopen(path, "r")) == NULL)
		return 0;
	n = 0;
	if (fgets(line, sizeof(line), f) != NULL)
		n = sscanf(line, MAGIC " %c %u %u %o", &kind, &maj, &min, &mode);
	fclose(f);
	if (n != 4 || (kind != 'c' && kind != 'b'))
		return 0;
	sb->st_mode = (kind == 'c' ? S_IFCHR : S_IFBLK) | (mode & 07777);
	sb->st_rdev = OPENBSD_MAKEDEV(maj, min);
	sb->st_size = 0;
	sb->st_blocks = 0;
	return 0;
}
"#;

/// Builds makefs for this machine, stages `root/` plus `/dev`, and writes `ramdisk.ffs`.
pub(super) fn build_ramdisk(ctx: &Ctx<'_>) -> Result<()> {
    if !ctx.src.join(MAKEFS_DIR).join("Makefile").is_file() {
        println!("  {MAKEFS_DIR}: not in the reference clone; no ramdisk image");
        return Ok(());
    }
    let makefs =
        build_host_prog_with(ctx, MAKEFS_DIR, |mk, objdir| shim(ctx, mk, objdir))?.join("makefs");

    let staging = ctx.out.join("ramdisk-root");
    if staging.exists() {
        fs::remove_dir_all(&staging).map_err(|e| format!("{}: {e}", staging.display()))?;
    }
    copy_tree(&ctx.out.join("root"), &staging, &mut HashMap::new())?;
    let dev = staging.join("dev");
    fs::create_dir_all(&dev).map_err(|e| format!("{}: {e}", dev.display()))?;
    for (name, kind, major, minor, mode) in DEVICES {
        let p = dev.join(name);
        fs::write(
            &p,
            format!("{DEVICE_MAGIC} {kind} {major} {minor} {mode:o}\n"),
        )
        .map_err(|e| format!("{}: {e}", p.display()))?;
    }

    // `rdroot` needs a fixed size (`-s`): twice the contents, in whole MiB, at least 2 MiB
    // (inodes, directories and indirect blocks fit with room to spare).
    const MIB: u64 = 1 << 20;
    let size = (tree_bytes(&staging)? * 2).div_ceil(MIB).max(2) * MIB;
    let image = ctx.out.join("ramdisk.ffs");
    let _ = fs::remove_file(&image);
    run(Command::new(&makefs)
        .args(["-t", "ffs", "-T", &TIMESTAMP.to_string()])
        .args(["-s", &size.to_string()])
        .args(["-o", FS_OPTIONS])
        .arg(&image)
        .arg(&staging))?;
    let size = fs::metadata(&image).map(|m| m.len()).unwrap_or(0);
    println!(
        "  ramdisk: {} ({size} bytes; makefs -t ffs -o {FS_OPTIONS}; /dev: {})",
        image.display(),
        DEVICES.iter().map(|d| d.0).collect::<Vec<_>>().join(" ")
    );
    Ok(())
}

/// Adds the host shims (module docs) to makefs's evaluated Makefile.
fn shim(ctx: &Ctx<'_>, mk: &mut Make, objdir: &Path) -> Result<()> {
    let inc = objdir.join("emibsd-include");
    for d in ["sys", "machine"] {
        fs::create_dir_all(inc.join(d)).map_err(|e| format!("{}: {e}", inc.display()))?;
    }
    let sys = ctx.src.join("sys");
    let links = [
        ("ufs", sys.join("ufs")),
        ("msdosfs", sys.join("msdosfs")),
        ("sys/disklabel.h", sys.join("sys/disklabel.h")),
        (
            "machine/disklabel.h",
            sys.join("arch/amd64/include/disklabel.h"),
        ),
    ];
    for (name, target) in &links {
        let l = inc.join(name);
        if fs::read_link(&l).ok().as_deref() != Some(target.as_path()) {
            let _ = fs::remove_file(&l);
            std::os::unix::fs::symlink(target, &l).map_err(|e| format!("{}: {e}", l.display()))?;
        }
    }
    let uuid_h = format!(
        "/* EmiBSD: OpenBSD's <sys/uuid.h>, its uuid_t renamed (macOS has its own). */\n\
         #define uuid_t openbsd_uuid_t\n#include \"{}\"\n#undef uuid_t\n",
        sys.join("sys/uuid.h").display()
    );
    let compat_c = COMPAT_C.replace("@MAGIC@", DEVICE_MAGIC);
    for (name, text) in [
        ("emibsd-compat.h", COMPAT_H),
        ("sys/endian.h", ENDIAN_H),
        ("sys/uuid.h", uuid_h.as_str()),
        ("emibsd_lstat.c", compat_c.as_str()),
    ] {
        write_if_changed(&inc.join(name), text)?;
    }

    let cppflags = mk.var("CPPFLAGS")?;
    mk.set(
        "CPPFLAGS",
        &format!(
            "{cppflags} -include {} -I{}",
            inc.join("emibsd-compat.h").display(),
            inc.display()
        ),
    );
    mk.add_path(&inc);
    mk.add_path(&ctx.src.join("lib/libutil"));
    let srcs = mk.var("SRCS")?;
    mk.set("SRCS", &format!("{srcs} fmt_scaled.c emibsd_lstat.c"));
    println!(
        "  {MAKEFS_DIR} (host tool): built with the host shims of tools/xtask/src/userland/ramdisk.rs \
         (OpenBSD headers macOS lacks, daddr_t, st_*tim, MAXBSIZE, lstat for device nodes, \
         scan_scaled from lib/libutil)"
    );
    Ok(())
}

/// The bytes of the regular files under `dir`, each hard-linked file once.
fn tree_bytes(dir: &Path) -> Result<u64> {
    use std::os::unix::fs::MetadataExt as _;
    let mut total = 0;
    let mut seen = BTreeSet::new();
    let mut dirs = vec![dir.to_path_buf()];
    while let Some(d) = dirs.pop() {
        for e in fs::read_dir(&d).map_err(|e| format!("{}: {e}", d.display()))? {
            let p = e?.path();
            let md = fs::symlink_metadata(&p)?;
            if md.is_dir() {
                dirs.push(p);
            } else if md.is_file() && seen.insert((md.dev(), md.ino())) {
                total += md.len();
            }
        }
    }
    Ok(total)
}

/// Writes `text` to `path` unless it already holds exactly that (keeps rebuilds incremental).
fn write_if_changed(path: &Path, text: &str) -> Result<()> {
    if fs::read_to_string(path).ok().as_deref() == Some(text) {
        return Ok(());
    }
    fs::write(path, text).map_err(|e| format!("{}: {e}", path.display()).into())
}

/// Copies the tree `from` to `to`, keeping symbolic links and hard links (`ksh`, `sh` and
/// `rksh` are one file).
fn copy_tree(from: &Path, to: &Path, seen: &mut HashMap<(u64, u64), PathBuf>) -> Result<()> {
    use std::os::unix::fs::MetadataExt as _;
    fs::create_dir_all(to).map_err(|e| format!("{}: {e}", to.display()))?;
    let mut entries: Vec<_> = fs::read_dir(from)
        .map_err(|e| format!("{}: {e}", from.display()))?
        .collect::<std::result::Result<_, _>>()?;
    entries.sort_by_key(|e| e.file_name());
    for e in entries {
        let src = e.path();
        let dst = to.join(e.file_name());
        let md = fs::symlink_metadata(&src)?;
        if md.is_dir() {
            copy_tree(&src, &dst, seen)?;
        } else if md.file_type().is_symlink() {
            std::os::unix::fs::symlink(fs::read_link(&src)?, &dst)?;
        } else if let Some(first) = seen.get(&(md.dev(), md.ino())) {
            fs::hard_link(first, &dst)?;
        } else {
            fs::copy(&src, &dst).map_err(|e| format!("{}: {e}", src.display()))?;
            seen.insert((md.dev(), md.ino()), dst);
        }
    }
    Ok(())
}
