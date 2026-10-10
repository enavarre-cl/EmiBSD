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
//! Step 5 of `cargo xtask userland`: the ffs ramdisk image, `target/userland/<arch>/ramdisk.ffs`.
//!
//! The image is made by OpenBSD's own makefs(8) (`usr.sbin/makefs`, in the reference clone
//! since 2026-10-03, the user's decision), built for this machine like `rpcgen`, not by a
//! file system writer of our own. It is invoked as OpenBSD's `distrib/` makes its ramdisks
//! (`-o disklabel=rdroot,minfree=0,density=4096`): rd(4) needs a disklabel, and `rd0a` is the
//! file system. The `rdroot` entry comes from a disktab written here (`disktab_entry`), read
//! by OpenBSD's own `getdiskbyname` (`lib/libc/gen/disklabel.c`). makefs's `rdroot=1` option
//! is not used: the label it builds leaves `d_nsectors` 0, which `checkdisklabel`
//! (`sys/kern/subr_disk.c`) rejects, so OpenBSD's rd would not open it either.
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
//! - `getdiskbyname` from OpenBSD's `lib/libc/gen/disklabel.c` (macOS has none), with
//!   `cgetent` pointed at `$EMIBSD_DISKTAB` in place of `/etc/disktab` (`COMPAT_C`);
//! - `lstat` wrapped (`COMPAT_C`) so that device nodes can be made without root: macOS
//!   lets only root `mknod`, and OpenBSD's makefs has no mtree spec. A regular file in the
//!   staging tree that holds exactly one `DEVICE_MAGIC` line is reported as that device,
//!   with OpenBSD's `makedev()` encoding of `st_rdev`;
//! - the same wrapper gives every file an owner, a group and a mode from a table
//!   (`$EMIBSD_OWNERS`, one `mode uid gid path` line per entry; paths relative to
//!   `$EMIBSD_STAGING`), root:wheel where the table is silent: makefs takes them from the
//!   host's files, which are the building user's, and `login_passwd` must be root's setuid
//!   program, `spwd.db` root:_shadow, `/tmp` sticky.
//!
//! The ramdisk's `/etc` (`etc_files`) is our own minimal set, OpenBSD's `etc/` not being in
//! the clone: enough for `init(8)` to run `/etc/rc` and a `getty(8)` on the console, and for
//! `login(1)` to check a password. `/etc/pwd.db` and `/etc/spwd.db` are made by OpenBSD's
//! own pwd_mkdb(8), with the root password's bcrypt hash (`passwd.rs`).

use super::*;

/// makefs, relative to the OpenBSD sources.
const MAKEFS_DIR: &str = "usr.sbin/makefs";

/// The first word of a device-node placeholder file (see `COMPAT_C`).
pub(super) const DEVICE_MAGIC: &str = "emibsd-makefs-device";

/// The device nodes the root needs before anything can run MAKEDEV: what `init(8)` opens
/// (`/dev/console`), what a shell expects (`/dev/tty`, `/dev/null`), what `getty(8)`,
/// `login(1)` and `mount(8)` open (the console's tty, `rd0a`, `klog` for `syslog(3)`).
/// Majors and minors are those of OpenBSD's `MAKEDEV` and of `cdevsw[]`/`bdevsw[]` in
/// `sys/arch/{amd64,arm64}/{amd64,arm64}/conf.c`, and they are the same on both
/// architectures, so the table is not per architecture:
///
/// - character (`cdevsw[]`, line numbers amd64 / arm64): `cn` 0 (177 / 127), `ctty` 1
///   (178 / 128), `mm` 2 (179 / 129; minors: `mem` 0, `kmem` 1, `null` 2, `zero` 12),
///   `log` 7 (184 / 134: `/dev/klog`), `com` 8 (185 / 135: the serial console's tty, minor
///   = unit), `bpf` 23 (201 / 151: `/dev/bpf`, cloning; `MAKEDEV` makes it 0600), `rd` 47
///   (225 / 175: the raw disk) and `pf` 73 (258 / 208: `/dev/pf`, which
///   pfctl(8) opens; `MAKEDEV` makes it 0600). The amd64 console is `com0`, `tty00`.
///   On arm64 `pluartcnattach` finds the major of `comopen` and puts `pluartdev` in its slot
///   (`sys/dev/ic/pluart.c:856-863`, "KLUDGE"), so `pluart0` is major 8, minor 0 too:
///   `tty00` on both;
/// - block (`bdevsw[]`): `rd` 17 (72 / 70), minor `unit * 64 + partition` (`DISKMINOR`,
///   `MAXPARTITIONSUNIT` 64, `MAKEDEV`'s `UNITMULT`): `rd0a` 0, `rd0b` 1, `rd0c` 2;
/// - SCSI disks (`sd`, M10a), made by `devices()` from `DISK_UNITS` and not listed in the
///   table: block `sd` 4 (`bdev_disk_init(NSD,sd)`, 59 / 57), character `sd` 13 (191 /
///   141), minor `unit * 64 + partition` for the partitions `a`..`p` (`MAKEDEV`'s `dodisk`:
///   `sd0a`..`sd0p` and `rsd0a`..`rsd0p`, mode 0640, group `operator`). `MAKEDEV all` makes
///   `sd0`..`sd9`; the image has `sd0`..`sd15` (M10f): `sd0` is the first persistent disk,
///   the arm64 boot disk is one more block device the kernel finds, and softraid volumes
///   take the next units;
/// - CD-ROM drives (`cd`, M13), made by `devices()` from `CD_UNITS` the same way: block `cd`
///   6 (`bdev_disk_init(NCD,cd)`), character 15, `cd0a`..`cd0p` and `rcd0a`..`rcd0p` (and
///   `cd1`; `MAKEDEV all` makes `cd0` and `cd1`);
/// - vnode disks (`vnd`, M10c), made by `devices()` from `VND_UNITS` the same way: block 14
///   (`bdev_disk_init(NVND,vnd)`, 69 / 67), character 41 (219 / 169), `vnd0`..`vnd3`;
/// - IDE disks (`wd`, M16a), made by `devices()` from `WD_UNITS` the same way: block `wd` 0
///   (`bdev_disk_init(NWD,wd)`), character 3, `wd0a`..`wd0p` and `rwd0a`..`rwd0p` for
///   `wd0`..`wd3` (`MAKEDEV all`'s; `dodisk wd $U 0 3`);
/// - the floppy (`fd`, M16a, amd64): block 2 (`bdev_disk_init(NFD,fd)`), character 9,
///   `fd0a`, `fd0b`, `fd0c`, `fd0i` and `rfd0a`..`rfd0i`, minors 0, 1, 2 and 8 (`MAKEDEV`'s
///   `fd*` for `fd0`, the ramdisk target's), mode 0640, group `operator`;
/// - `bio` is major 79 (`bio` 79 / 79: `/dev/bio`, `MAKEDEV` makes it 0600), minor 0;
/// - `fuse` is major 92 (`cdev_fuse_init`, 277 / 227: `/dev/fuse0`, cloning, the one node
///   libfuse opens; `MAKEDEV`'s `_mcdev(fuse, ...)` makes it 0600), minor 0;
/// - `fd/N` is `filedesc` 22 (200 / 150), minor N, for N in `0..64` like MAKEDEV;
///   `stdin`, `stdout` and `stderr` link to `fd/0..2` (`DEV_LINKS`).
///
/// - `wsdisplay` is major 12 (`cdev_wsdisplay_init`, M13; 189 / 139): `ttyC0`..`ttyCb`,
///   minor = screen, and `ttyCcfg`, minor 255, the control device, mode 0600 (`MAKEDEV`'s
///   `wscons`); `wskbd` is major 67 (`cdev_mouse_init`, M13): `wskbd0`..`wskbd3`, minor =
///   unit; `wsmouse` is major 68 (`cdev_mouse_init`, M13): `wsmouse0`..`wsmouse3`, minor =
///   unit; `wsmux` is major 69: `wsmouse` (mux 0) and `wskbd` (mux 1), the nodes of
///   `MAKEDEV`'s `wsmux` target (it makes no `/dev/wsmux`), all mode 0600.
/// - `audio` is major 42 (`cdev_audio_init`, M12; 220 / 170): `audio0` minor 0 and
///   `audioctl0` minor 192 (`AUDIO_DEV_AUDIOCTL`), mode 0660, group `_sndiop` (`MAKEDEV`'s
///   `audio*` entry); `usb` is major 61 (`cdev_usb_init`, 239 / 191): `usb0`, mode 0640
///   (`MAKEDEV`'s `usb*`).
/// - `uhid` is major 62 (`cdev_usbdev_init(NUHID,uhid)`, M16b): `uhid0`..`uhid7`, minor =
///   unit, mode 0600 (`MAKEDEV`'s `uhid*`); `ugen` is major 63 (`cdev_usbdev_init(NUGEN,ugen)`):
///   `ugen0.00`..`ugen1.15`, minor `unit * 16 + endpoint`, mode 0600 (`MAKEDEV`'s `ugen*`).
/// - `ucom` is major 66 (`cdev_tty_init(NUCOM,ucom)`, M16b): `ttyU0`..`ttyU3`, minor = unit, and
///   the call-out nodes `cuaU0`..`cuaU3`, minor `unit + 128` (`UCOMCUA_MASK`), mode 0660,
///   group `dialer` (`MAKEDEV`'s `ttyU*`).
///
/// - `random` is major 45 (`cdev_random_init(1,random)`, M16d; 223 / 173): `urandom`, minor 0,
///   mode 0644, and `random` a symbolic link to it (`MAKEDEV`'s `rnd`: `M urandom c 45 0 644`,
///   `ln -s urandom random`; `DEV_LINKS`).
///
/// (name, kind, major, minor, mode, group)
const DEVICES: &[(&str, char, u32, u32, u32, &str)] = &[
    ("console", 'c', 0, 0, 0o600, "wheel"),
    ("tty", 'c', 1, 0, 0o666, "wheel"),
    ("mem", 'c', 2, 0, 0o640, "kmem"),
    ("kmem", 'c', 2, 1, 0o640, "kmem"),
    ("null", 'c', 2, 2, 0o666, "wheel"),
    ("zero", 'c', 2, 12, 0o666, "wheel"),
    ("klog", 'c', 7, 0, 0o600, "wheel"),
    ("urandom", 'c', 45, 0, 0o644, "wheel"),
    ("tty00", 'c', 8, 0, 0o600, "wheel"),
    ("bpf", 'c', 23, 0, 0o600, "wheel"),
    ("rd0a", 'b', 17, 0, 0o640, "operator"),
    ("rd0b", 'b', 17, 1, 0o640, "operator"),
    ("rd0c", 'b', 17, 2, 0o640, "operator"),
    ("rrd0a", 'c', 47, 0, 0o640, "operator"),
    ("rrd0b", 'c', 47, 1, 0o640, "operator"),
    ("rrd0c", 'c', 47, 2, 0o640, "operator"),
    ("pf", 'c', 73, 0, 0o600, "wheel"),
    // M10f: `bio` 79 (`MAKEDEV`'s `_mkdev(bio, bio, {-M bio c major_bio_c 0 600-})`):
    // bioctl(8) and the softraid tools.
    ("bio", 'c', 79, 0, 0o600, "wheel"),
    // M10d: `fuse` 92 (`MAKEDEV`'s `_mcdev(fuse, fuse, fuse, {-major_fuse_c-}, 600)`):
    // libfuse opens `/dev/fuse0` (`lib/libfuse/fuse.c`, `fuse_mount`).
    ("fuse0", 'c', 92, 0, 0o600, "wheel"),
    // M12: audio(4) (`MAKEDEV`'s `audio*`) and the first USB bus (`usb*`).
    ("audio0", 'c', 42, 0, 0o660, "_sndiop"),
    ("audioctl0", 'c', 42, 192, 0o660, "_sndiop"),
    // M16d: lpt(4)'s first port (`MAKEDEV`'s `lpt*`: `M lpt$U c 16 $U 600`; amd64's cdevsw 16,
    // which arm64's leaves unconfigured).
    ("lpt0", 'c', 16, 0, 0o600, "wheel"),
    // M16d: spkr(4) (`MAKEDEV`'s `speaker`: `M speaker c 27 0 600`; amd64's cdevsw 27).
    ("speaker", 'c', 27, 0, 0o600, "wheel"),
    // M16a: fd(4)'s first drive (`MAKEDEV`'s `fd*` for `fd0`: `n=$((U*128 + typnum*64))`,
    // `M fd0a b 2 $n`, `fd0b` n+1, `fd0c` n+2, `fd0i` n+8, and `rfd0*` on character 9;
    // amd64's bdevsw 2 and cdevsw 9, which arm64's leave unconfigured).
    ("fd0a", 'b', 2, 0, 0o640, "operator"),
    ("fd0b", 'b', 2, 1, 0o640, "operator"),
    ("fd0c", 'b', 2, 2, 0o640, "operator"),
    ("fd0i", 'b', 2, 8, 0o640, "operator"),
    ("rfd0a", 'c', 9, 0, 0o640, "operator"),
    ("rfd0b", 'c', 9, 1, 0o640, "operator"),
    ("rfd0c", 'c', 9, 2, 0o640, "operator"),
    ("rfd0i", 'c', 9, 8, 0o640, "operator"),
    ("usb0", 'c', 61, 0, 0o640, "wheel"),
    // M16b: `uhid` 62 (`MAKEDEV`'s `_mcdev(uhid, uhid*, uhid, {-major_uhid_c-}, 600)`, whose
    // target lists units 0 to 7). `ugen` 63 is made by `devices()`.
    ("uhid0", 'c', 62, 0, 0o600, "wheel"),
    ("uhid1", 'c', 62, 1, 0o600, "wheel"),
    ("uhid2", 'c', 62, 2, 0o600, "wheel"),
    ("uhid3", 'c', 62, 3, 0o600, "wheel"),
    ("uhid4", 'c', 62, 4, 0o600, "wheel"),
    ("uhid5", 'c', 62, 5, 0o600, "wheel"),
    ("uhid6", 'c', 62, 6, 0o600, "wheel"),
    ("uhid7", 'c', 62, 7, 0o600, "wheel"),
    // M16b: `ucom` 66 (`cdev_tty_init(NUCOM,ucom)`), `MAKEDEV`'s `ttyU*` target for units 0 to
    // 3: `M ttyU$U c 66 $U 660 dialer root` and `M cuaU$U c 66 $((U+128)) 660 dialer root`.
    ("ttyU0", 'c', 66, 0, 0o660, "dialer"),
    ("ttyU1", 'c', 66, 1, 0o660, "dialer"),
    ("ttyU2", 'c', 66, 2, 0o660, "dialer"),
    ("ttyU3", 'c', 66, 3, 0o660, "dialer"),
    ("cuaU0", 'c', 66, 128, 0o660, "dialer"),
    ("cuaU1", 'c', 66, 129, 0o660, "dialer"),
    ("cuaU2", 'c', 66, 130, 0o660, "dialer"),
    ("cuaU3", 'c', 66, 131, 0o660, "dialer"),
    // M13: `com4`, the first `com* at puc?` after amd64's four ISA lines (`smoke-puc`): the
    // call-out node (`com`'s `COMDIALOUT`, minor bit 0x80), which opens without a carrier.
    ("cua04", 'c', 8, 132, 0o600, "wheel"),
    // M13: wsdisplay(4)'s screens and control device (`MAKEDEV`'s `wscons` and
    // `tty[C-J]*`: `M ttyC$U c 12 $((16#$U)) 600`, `M ttyCcfg c 12 255 600`).
    ("ttyC0", 'c', 12, 0, 0o600, "wheel"),
    ("ttyC1", 'c', 12, 1, 0o600, "wheel"),
    ("ttyC2", 'c', 12, 2, 0o600, "wheel"),
    ("ttyC3", 'c', 12, 3, 0o600, "wheel"),
    ("ttyC4", 'c', 12, 4, 0o600, "wheel"),
    ("ttyC5", 'c', 12, 5, 0o600, "wheel"),
    ("ttyC6", 'c', 12, 6, 0o600, "wheel"),
    ("ttyC7", 'c', 12, 7, 0o600, "wheel"),
    ("ttyC8", 'c', 12, 8, 0o600, "wheel"),
    ("ttyC9", 'c', 12, 9, 0o600, "wheel"),
    ("ttyCa", 'c', 12, 10, 0o600, "wheel"),
    ("ttyCb", 'c', 12, 11, 0o600, "wheel"),
    ("ttyCcfg", 'c', 12, 255, 0o600, "wheel"),
    // M13: wskbd(4)'s keyboards and wsmux(4)'s muxes (`MAKEDEV`'s `wscons`: `wskbd[0-9]*` is
    // `M wskbd$U c 67 $U 600`; `wsmux|wsmouse|wskbd` is `M wsmouse c 69 0 600` and
    // `M wskbd c 69 1 600`, the mouse and keyboard muxes).
    ("wskbd0", 'c', 67, 0, 0o600, "wheel"),
    ("wskbd1", 'c', 67, 1, 0o600, "wheel"),
    ("wskbd2", 'c', 67, 2, 0o600, "wheel"),
    ("wskbd3", 'c', 67, 3, 0o600, "wheel"),
    // M13: wsmouse(4)'s mice (`MAKEDEV`'s `wscons`: `wsmouse[0-9]*` is
    // `M wsmouse$U c 68 $U 600`).
    ("wsmouse0", 'c', 68, 0, 0o600, "wheel"),
    ("wsmouse1", 'c', 68, 1, 0o600, "wheel"),
    ("wsmouse2", 'c', 68, 2, 0o600, "wheel"),
    ("wsmouse3", 'c', 68, 3, 0o600, "wheel"),
    ("wsmouse", 'c', 69, 0, 0o600, "wheel"),
    ("wskbd", 'c', 69, 1, 0o600, "wheel"),
];

/// The `sd` units the image has nodes for (module docs of `DEVICES`): M10f's four vioblk
/// disks, the arm64 boot disk and the softraid volumes (`sd4` and up) fit in sixteen.
const DISK_UNITS: &[u32] = &[0, 1, 2, 3, 4, 5, 6, 7, 8, 9, 10, 11, 12, 13, 14, 15];

/// `MAKEDEV`'s `UNITMULT`: minors per disk unit (`MAXPARTITIONSUNIT` of `sys/disklabel.h`).
const UNITMULT: u32 = 64;

/// `bdevsw[]` and `cdevsw[]` majors of `sd`.
const SD_BLOCK_MAJOR: u32 = 4;
const SD_CHAR_MAJOR: u32 = 13;

/// The `cd` units the image has nodes for: `MAKEDEV all`'s `cd0` and `cd1`.
const CD_UNITS: &[u32] = &[0, 1];

/// `bdevsw[]` and `cdevsw[]` majors of `cd` (6 and 15 on amd64 and arm64).
const CD_BLOCK_MAJOR: u32 = 6;
const CD_CHAR_MAJOR: u32 = 15;

/// The `vnd` units the image has nodes for: `MAKEDEV all`'s `vnd0`..`vnd3` (GENERIC's
/// `pseudo-device vnd 4`).
const VND_UNITS: &[u32] = &[0, 1, 2, 3];

/// The `ugen` units the image has nodes for (M16b): `MAKEDEV`'s `ugen*` makes sixteen nodes
/// per unit, `ugen$U.00`..`ugen$U.15`, major 63, minor `unit * 16 + endpoint`, mode 0600.
/// The first two of its eight units are enough for the machine's USB devices.
const UGEN_UNITS: &[u32] = &[0, 1];

/// `cdevsw[]` major of `ugen` (`cdev_usbdev_init(NUGEN,ugen)`, 63 on amd64 and arm64).
const UGEN_CHAR_MAJOR: u32 = 63;

/// The `wd` units the image has nodes for: `MAKEDEV all`'s `wd0`..`wd3`.
const WD_UNITS: &[u32] = &[0, 1, 2, 3];

/// `bdevsw[]` and `cdevsw[]` majors of `wd` (0 and 3 on amd64 and arm64).
const WD_BLOCK_MAJOR: u32 = 0;
const WD_CHAR_MAJOR: u32 = 3;

/// `bdevsw[]` and `cdevsw[]` majors of `vnd` (14 and 41 on amd64 and arm64).
const VND_BLOCK_MAJOR: u32 = 14;
const VND_CHAR_MAJOR: u32 = 41;

/// Every device node of the image, in the shape of `DEVICES`: its table plus the `sd` disk
/// partitions (`MAKEDEV`'s `dodisk`: `a`..`p` of a kernel with `kern.maxpartitions` 16, at
/// `UNITMULT` (`MAXPARTITIONSUNIT`) 64 minors per unit: `DISKUNIT` divides by 64).
pub(super) fn devices() -> Vec<(String, char, u32, u32, u32, &'static str)> {
    let mut all: Vec<_> = DEVICES
        .iter()
        .map(|&(n, k, major, minor, mode, group)| (n.to_string(), k, major, minor, mode, group))
        .collect();
    let disks = DISK_UNITS
        .iter()
        .map(|u| ("sd", *u, SD_BLOCK_MAJOR, SD_CHAR_MAJOR))
        .chain(
            CD_UNITS
                .iter()
                .map(|u| ("cd", *u, CD_BLOCK_MAJOR, CD_CHAR_MAJOR)),
        )
        .chain(
            VND_UNITS
                .iter()
                .map(|u| ("vnd", *u, VND_BLOCK_MAJOR, VND_CHAR_MAJOR)),
        )
        .chain(
            WD_UNITS
                .iter()
                .map(|u| ("wd", *u, WD_BLOCK_MAJOR, WD_CHAR_MAJOR)),
        );
    for unit in UGEN_UNITS {
        for endpoint in 0..16 {
            all.push((
                format!("ugen{unit}.{endpoint:02}"),
                'c',
                UGEN_CHAR_MAJOR,
                unit * 16 + endpoint,
                0o600,
                "wheel",
            ));
        }
    }
    for (name, unit, bmajor, cmajor) in disks {
        for (part, letter) in ('a'..='p').enumerate() {
            let minor = unit * UNITMULT + part as u32;
            all.push((
                format!("{name}{unit}{letter}"),
                'b',
                bmajor,
                minor,
                0o640,
                "operator",
            ));
            all.push((
                format!("r{name}{unit}{letter}"),
                'c',
                cmajor,
                minor,
                0o640,
                "operator",
            ));
        }
    }
    all
}

/// The placeholder file of a device node in the staging tree (`COMPAT_C` reads it back).
fn device_line(kind: char, major: u32, minor: u32, mode: u32) -> String {
    format!("{DEVICE_MAGIC} {kind} {major} {minor} {mode:o}\n")
}

/// Fails when `image` predates the current [`devices`] table: the staging tree makefs read
/// (`ramdisk-root/` beside the image) lacks a node of today's table, or has it with another
/// major, minor or mode. Such an image still boots, but whatever opens the missing node gets
/// `ENOENT` far from the cause: `smoke-kbd`'s dd(1) of `/dev/wskbd0` read nothing (M15, a
/// ramdisk made between M13's wsdisplay and wskbd ports). An image without a staging tree
/// beside it was not made by `cargo xtask userland` and is not checked.
pub(crate) fn check_devices(image: &Path) -> Result<()> {
    let Some(dev) = image.parent().map(|d| d.join("ramdisk-root").join("dev")) else {
        return Ok(());
    };
    if !dev.is_dir() {
        return Ok(());
    }
    let stale: Vec<String> = devices()
        .into_iter()
        .filter(|(name, kind, major, minor, mode, _)| {
            fs::read_to_string(dev.join(name)).ok()
                != Some(device_line(*kind, *major, *minor, *mode))
        })
        .map(|(name, ..)| format!("/dev/{name}"))
        .collect();
    if stale.is_empty() {
        return Ok(());
    }
    Err(format!(
        "{}: made before the device table changed ({} missing or different); run `just userland`",
        image.display(),
        stale.join(" ")
    )
    .into())
}

/// `/dev/fd/N` exists for N below this (`MAKEDEV fd`).
pub(super) const FD_NODES: u32 = 64;

/// `/dev/stdin` and friends, as `MAKEDEV` links them: (name, target).
pub(super) const DEV_LINKS: &[(&str, &str)] = &[
    ("stdin", "fd/0"),
    ("stdout", "fd/1"),
    ("stderr", "fd/2"),
    ("random", "urandom"),
];

/// A user of `/etc/master.passwd`: (name, uid, gid, class, gecos, home, shell). OpenBSD's
/// `root`, `daemon` and `nobody` (the lines of its stock `master.passwd`), and tcpdump(8)'s
/// privsep users `_tcpdump` and `_portmap`, and `_sndiop` (M12, the group of the audio
/// devices) (portmap(8) chroots to `/var/empty` as it;
/// `etc/master.passwd` of the reference clone, line for line; the empty class is
/// login.conf's `default`), no more.
const USERS: &[(&str, u32, u32, &str, &str, &str, &str)] = &[
    ("root", 0, 0, "daemon", "Charlie &", "/root", "/bin/ksh"),
    (
        "daemon",
        1,
        1,
        "daemon",
        "The devil himself",
        "/root",
        "/sbin/nologin",
    ),
    (
        "_portmap",
        28,
        28,
        "",
        "portmap",
        "/var/empty",
        "/sbin/nologin",
    ),
    (
        "_tcpdump",
        76,
        76,
        "",
        "tcpdump privsep",
        "/var/empty",
        "/sbin/nologin",
    ),
    // M12: the group of `/dev/audio0` and `/dev/audioctl0` (`MAKEDEV`), and its user.
    (
        "_sndiop",
        110,
        110,
        "",
        "sndio privileged user",
        "/var/empty",
        "/sbin/nologin",
    ),
    (
        "nobody",
        32767,
        32767,
        "daemon",
        "Unprivileged user",
        "/nonexistent",
        "/sbin/nologin",
    ),
];

/// A group of `/etc/group`: (name, gid, members). The gids OpenBSD's stock `group` gives
/// (`auth` 11 for `login_passwd`, `utmp` 45, `tty` 4, ...); `_shadow` (the group of
/// `spwd.db`, which `pwd_mkdb` insists on) is ours.
const GROUPS: &[(&str, u32, &str)] = &[
    ("wheel", 0, "root"),
    ("daemon", 1, "root"),
    ("kmem", 2, "root"),
    ("sys", 3, "root"),
    ("tty", 4, ""),
    ("operator", 5, "root"),
    ("bin", 7, ""),
    ("auth", 11, ""),
    ("_shadow", 14, ""),
    ("utmp", 45, ""),
    // etc/group of the reference clone.
    ("_portmap", 28, ""),
    ("_tcpdump", 76, ""),
    ("_sndiop", 110, ""),
    // M16b: the group of `/dev/ttyU*` and `/dev/cuaU*` (`MAKEDEV`); etc/group of the clone.
    ("dialer", 117, ""),
    ("nogroup", 32766, ""),
    ("nobody", 32767, ""),
];

/// The directories the multi-user system needs, with their modes: (path, mode).
const DIRS: &[(&str, u32)] = &[
    ("/home", 0o755),
    ("/mnt", 0o755),
    ("/root", 0o700),
    ("/tmp", 0o1777),
    ("/var", 0o755),
    // etc/mtree/4.4BSD.dist: root:wheel 0755; tcpdump's privsep chroots here.
    ("/var/empty", 0o755),
    // mountd(8)'s `mountdtab` (`_PATH_RMOUNTLIST`); etc/mtree/4.4BSD.dist: root:wheel 0755.
    ("/var/db", 0o755),
    ("/var/log", 0o755),
    ("/var/mail", 0o755),
    ("/var/run", 0o755),
    ("/var/tmp", 0o1777),
];

/// The id of user `name`.
fn user_id(name: &str) -> Option<u32> {
    USERS.iter().find(|u| u.0 == name).map(|u| u.1)
}

/// The id of group `name`.
pub(super) fn group_id(name: &str) -> Option<u32> {
    GROUPS.iter().find(|g| g.0 == name).map(|g| g.1)
}

/// Owner, group and mode of one path of the image (the makefs shim applies them).
#[derive(Clone)]
pub(super) struct Attr {
    /// Absolute path inside the image.
    pub(super) path: String,
    pub(super) uid: u32,
    pub(super) gid: u32,
    pub(super) mode: u32,
}

impl Attr {
    /// `install -o owner -g group -m mode`: the names are users and groups of this image,
    /// the mode octal.
    pub(super) fn installed(path: &str, owner: &str, group: &str, mode: &str) -> Result<Attr> {
        Ok(Attr {
            path: path.to_string(),
            uid: user_id(owner).ok_or_else(|| format!("{path}: unknown owner `{owner}`"))?,
            gid: group_id(group).ok_or_else(|| format!("{path}: unknown group `{group}`"))?,
            mode: u32::from_str_radix(mode, 8)
                .map_err(|e| format!("{path}: bad mode `{mode}`: {e}"))?,
        })
    }

    pub(super) fn root(path: &str, gid: u32, mode: u32) -> Attr {
        Attr {
            path: path.to_string(),
            uid: 0,
            gid,
            mode,
        }
    }
}

/// makefs's ffs options: those of OpenBSD's `distrib/` ramdisks.
pub(super) const FS_OPTIONS: &str = "disklabel=rdroot,minfree=0,density=4096";

/// The disktab entry makefs reads for `disklabel=rdroot`, for an image of `sectors` 512-byte
/// sectors: one track of one cylinder (so `d_secpercyl` = `d_nsectors` = the whole disk, as
/// rd(4)'s own spoofed label has it), partition `a` the FFS file system at offset 0 with
/// 4096-byte blocks and 512-byte fragments, `c` the whole disk.
pub(super) fn disktab_entry(sectors: u64) -> String {
    format!(
        "rdroot|EmiBSD ramdisk root (generated by cargo xtask userland):\\\n\
         \t:dt=rdroot:se#512:nc#1:nt#1:ns#{sectors}:\\\n\
         \t:ta=4.2BSD:oa#0:pa#{sectors}:fa#512:ba#4096:\\\n\
         \t:oc#0:pc#{sectors}:\n"
    )
}

/// The files the ramdisk's `/etc` holds that are plain text: (name, mode, text). Written
/// here (OpenBSD's `etc/` is not in the reference clone); `motd` is the text `cat /etc/motd`
/// shows in the smoke test. `group` and `master.passwd` come from `GROUPS` and `USERS`
/// (`etc_files`), `passwd`, `pwd.db` and `spwd.db` from pwd_mkdb(8).
const ETC_FILES: &[(&str, u32, &str)] = &[
    (
        "motd",
        0o644,
        "Welcome to EmiBSD 8.0: OpenBSD's init(8) and ksh(1) on an ffs ramdisk root.\n",
    ),
    ("shells", 0o644, "/bin/sh\n/bin/ksh\n"),
    ("fstab", 0o644, FSTAB),
    ("ttys", 0o644, TTYS),
    ("gettytab", 0o644, GETTYTAB),
    ("login.conf", 0o644, LOGIN_CONF),
    ("rc", 0o644, RC),
    ("pf.conf", 0o600, PF_CONF),
    ("pf.os", 0o444, PF_OS),
    ("protocols", 0o644, PROTOCOLS),
    ("resolv.conf", 0o644, RESOLV_CONF),
    ("hosts", 0o644, HOSTS),
    ("rpc", 0o644, RPC),
];

/// `resolv.conf(5)`: QEMU's user network answers DNS on 10.0.2.3 (forwarded to this
/// machine's resolvers). libc's resolver (`asr`) reads it for ftp(1) and nc(1).
const RESOLV_CONF: &str = "nameserver 10.0.2.3\n";

/// `hosts(5)`: `localhost`, and `emibsd-host` for 10.0.2.2, the address QEMU's user network
/// gives this machine (a guest's connection to it reaches the host's 127.0.0.1), where
/// `cargo xtask smoke --https-server` runs the test servers. The test CA's server
/// certificate is for that name (`testca.rs`).
const HOSTS: &str = "\
127.0.0.1\tlocalhost
::1\t\tlocalhost
10.0.2.2\temibsd-host
";

/// `rpc(5)`: getrpcbyname(3)'s database, for the NFS programs (portmap(8), mountd(8), nfsd(8),
/// showmount(8)). The lines of `etc/rpc` of the reference clone for the programs the
/// ramdisk has (`reference_etc_entries_match`), no more.
const RPC: &str = "\
#
# rpc(5) of the EmiBSD ramdisk, a subset of OpenBSD's etc/rpc (the programs it has):
# from: rpc 88/08/01 4.0 RPCSRC; from 1.12   88/02/07 SMI
#
portmapper\t100000\tportmap sunrpc
nfs\t\t100003\tnfsprog
mountd\t\t100005\tmount showmount
rquotad\t\t100011\trquotaprog quota rquota
";

/// `protocols(5)`: pfctl(8)'s parser names protocols through getprotobyname(3) (`proto icmp`
/// in `pf.conf`). The IANA numbers of the protocols this kernel handles, with `ip` for
/// pseudo-protocol 0; OpenBSD's `etc/protocols` is not in the reference clone.
const PROTOCOLS: &str = "\
# protocols(5) of the EmiBSD ramdisk: name, number, aliases.
ip\t0\tIP\t\t# internet protocol, pseudo protocol number
icmp\t1\tICMP\t\t# internet control message protocol
igmp\t2\tIGMP\t\t# internet group management protocol
ipencap\t4\tIP-ENCAP\t# IP encapsulated in IP
tcp\t6\tTCP\t\t# transmission control protocol
udp\t17\tUDP\t\t# user datagram protocol
gre\t47\tGRE\t\t# generic routing encapsulation
esp\t50\tESP\t\t# encapsulated security payload
ah\t51\tAH\t\t# authentication header
ipv6-icmp\t58\tIPv6-ICMP icmp6\t# ICMP for IPv6
carp\t112\tCARP\t\t# common address redundancy protocol
pfsync\t240\tPFSYNC\t\t# pf state synchronisation
";

/// `pf.conf(5)`: what `just smoke-pf` loads with `pfctl -f /etc/pf.conf`: pass everything
/// but ICMP to QEMU's gateway, so the ping that worked before is blocked.
const PF_CONF: &str = "\
# pf.conf(5) of the EmiBSD ramdisk: block the ping to QEMU's user-network gateway.
set skip on lo
pass
block drop quick inet proto icmp from any to 10.0.2.2
";

/// `pf.os(5)`: pfctl(8) loads the passive OS fingerprints from it with every ruleset; the
/// ramdisk has none (OpenBSD's `etc/pf.os` is not in the reference clone).
const PF_OS: &str = "# pf.os(5) of the EmiBSD ramdisk: no fingerprints.\n";

/// `fstab(5)`: `mount -uw /` finds the root's entry here (`mount.c` looks the root up by its
/// mount point because the kernel names it `root_device`). The `sd0a` line (M10b) is how
/// quotacheck(8), quotaon(8), edquota(8), repquota(8) and quota(1) find a file system with
/// quotas (`userquota`); `noauto` keeps `mount -a` off it.
const FSTAB: &str = "/dev/rd0a / ffs rw 1 1\n/dev/sd0a /mnt ffs rw,userquota,noauto 1 2\n";

/// `ttys(5)`: `init(8)` runs `getty` on the line the kernel's console is, `tty00` on both
/// architectures (`DEVICES`), and not on `/dev/console` itself, which is the same device.
const TTYS: &str = "\
console\t\"/usr/libexec/getty std.9600\"\tvt220\toff secure
tty00\t\"/usr/libexec/getty std.9600\"\tvt220\ton secure
";

/// `gettytab(5)`: a `default` entry (8-bit, no parity, the banner) and `std.9600`, the entry
/// `ttys` names. The banner's `%s/%m (%h) (%t)` is OpenBSD's: system, machine, host, tty.
const GETTYTAB: &str = "\
# gettytab(5) of the EmiBSD ramdisk (a minimal version of OpenBSD's).
default:\\
\t:np:im=\\r\\n%s/%m (%h) (%t)\\r\\n\\r\\n:sp#1200:

std.9600|9600-baud:\\
\t:sp#9600:
";

/// `login.conf(5)`: the default class, which authenticates with `login_passwd`
/// (`/usr/libexec/auth/login_passwd`) and gives a login the usual `PATH` and `umask`; and the
/// `daemon` class `init(8)` runs `/etc/rc` in and root and the system users belong to.
const LOGIN_CONF: &str = "\
# login.conf(5) of the EmiBSD ramdisk (a minimal version of OpenBSD's).
default:\\
\t:path=/usr/bin /bin /usr/sbin /sbin:\\
\t:umask=022:\\
\t:auth=passwd:\\
\t:localcipher=blowfish,8:\\
\t:welcome=/etc/motd:

daemon:\\
\t:ignorenologin:\\
\t:tc=default:
";

/// `/etc/rc`, run by `init(8)` as `sh /etc/rc autoboot`: ours, minimal (OpenBSD's full `rc`
/// belongs to later milestones). The root file system comes up read-only from the ramdisk;
/// the files `login(1)` writes must exist (`utmp`, `wtmp`, `lastlog`, `failedlogin`).
const RC: &str = "\
# /etc/rc of the EmiBSD ramdisk: a minimal version, OpenBSD's full rc comes later.
PATH=/sbin:/bin:/usr/sbin:/usr/bin
export PATH
umask 022

mount -uw / || echo 'rc: mount -uw / failed; the root stays read-only'

for f in /var/run/utmp /var/log/wtmp /var/log/lastlog /var/log/failedlogin; do
\t[ -f $f ] || : > $f
done

echo 'rc: multi-user'
exit 0
";

/// The file `name`'s text of `/etc`, for every file of the ramdisk's `/etc` that is not made
/// by pwd_mkdb(8): (name, mode, text). `root_hash` is the root password's hash.
fn etc_files(root_hash: &str) -> Vec<(String, u32, String)> {
    let mut files: Vec<(String, u32, String)> = ETC_FILES
        .iter()
        .map(|(n, m, t)| (n.to_string(), *m, t.to_string()))
        .collect();
    files.push(("group".into(), 0o644, group_file()));
    files.push(("master.passwd".into(), 0o600, master_passwd(root_hash)));
    files
}

/// `/etc/group` from `GROUPS`.
fn group_file() -> String {
    GROUPS
        .iter()
        .map(|(name, gid, members)| format!("{name}:*:{gid}:{members}\n"))
        .collect()
}

/// `/etc/master.passwd` from `USERS`: `name:passwd:uid:gid:class:change:expire:gecos:home:
/// shell`; only root has a password, the others cannot log in (`*`).
fn master_passwd(root_hash: &str) -> String {
    USERS
        .iter()
        .map(|(name, uid, gid, class, gecos, home, shell)| {
            let pw = if *name == "root" { root_hash } else { "*" };
            format!("{name}:{pw}:{uid}:{gid}:{class}:0:0:{gecos}:{home}:{shell}\n")
        })
        .collect()
}

/// The owner, group and mode of every path of the image that is not a program installed by
/// a Makefile (those are `installed`): the `/etc` files (those pwd_mkdb(8) makes too), the
/// directories and the device nodes.
fn image_attrs(installed: &[Attr]) -> Vec<Attr> {
    let mut attrs = installed.to_vec();
    let wheel = 0;
    for (name, mode, _) in etc_files("") {
        attrs.push(Attr::root(&format!("/etc/{name}"), wheel, mode));
    }
    let shadow = group_id("_shadow").unwrap_or(wheel);
    attrs.push(Attr::root("/etc/passwd", wheel, 0o644));
    attrs.push(Attr::root("/etc/pwd.db", wheel, 0o644));
    attrs.push(Attr::root("/etc/spwd.db", shadow, 0o640));
    for (path, mode) in DIRS {
        attrs.push(Attr::root(path, wheel, *mode));
    }
    for (name, _, _, _, mode, group) in devices() {
        attrs.push(Attr::root(
            &format!("/dev/{name}"),
            group_id(group).unwrap_or(wheel),
            mode,
        ));
    }
    attrs
}

/// The ownership table the makefs shim reads (`$EMIBSD_OWNERS`): `mode uid gid path` lines.
pub(super) fn owners_table(attrs: &[Attr]) -> String {
    attrs
        .iter()
        .map(|a| format!("{:o} {} {} {}\n", a.mode, a.uid, a.gid, a.path))
        .collect()
}

/// A fixed timestamp (`makefs -T`: inode times and generation numbers), 2026-10-02, the date
/// of the reference pin. The image is not bit-for-bit reproducible: makefs gives the label a
/// random `d_uid` (`arc4random_buf`).
pub(crate) const TIMESTAMP: u64 = 1_790_899_200;

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
int emibsd_cgetent(char **, char **, const char *);
#define cgetent(buf, db, name) emibsd_cgetent(buf, db, name)
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
const COMPAT_C: &str = r#"/* EmiBSD: lstat and cgetent for the makefs host build (tools/xtask, ramdisk.rs). */
#include <sys/types.h>
#include <sys/stat.h>
#include <stdio.h>
#include <string.h>

#include <stdlib.h>

#undef lstat	/* the force-included header points lstat and cgetent here */
#undef cgetent

/* getdiskbyname(3) reads the disktab named by $EMIBSD_DISKTAB, not /etc/disktab. */
int
emibsd_cgetent(char **buf, char **db_array, const char *name)
{
	char *db[2];

	(void)db_array;
	db[0] = getenv("EMIBSD_DISKTAB");
	db[1] = NULL;
	if (db[0] == NULL)
		return -1;
	return cgetent(buf, db, name);
}

#define MAGIC "@MAGIC@"

/* OpenBSD's makedev() (sys/types.h), not macOS's. */
#define OPENBSD_MAKEDEV(x, y) \
	((dev_t)((((x) & 0xff) << 8) | ((y) & 0xff) | (((y) & 0xffff00) << 8)))

static int
lstat_device(const char *path, struct stat *sb)
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

/*
 * Ownership: every file is root:wheel, and the table in $EMIBSD_OWNERS (lines of
 * "mode uid gid /path") overrides the mode, owner and group of the paths it lists. The
 * paths makefs gives lstat are $EMIBSD_STAGING/./dir/name; both sides are reduced to their
 * components, without the "." ones.
 */
#define MAXATTR 8192
#define MAXPATHLEN_ATTR 256

static struct attr {
	char path[MAXPATHLEN_ATTR];
	unsigned int mode, uid, gid;
} attrs[MAXATTR];
static int nattrs = -1;
static char staging[MAXPATHLEN_ATTR];

/* `a/./b//c` -> `a/b/c`. */
static void
normalise(const char *p, char *out, size_t n)
{
	size_t o = 0, l;
	const char *s;

	while (*p != '\0') {
		while (*p == '/')
			p++;
		s = p;
		while (*p != '\0' && *p != '/')
			p++;
		l = p - s;
		if (l == 0 || (l == 1 && s[0] == '.'))
			continue;
		if (o + l + 2 > n)
			break;
		if (o != 0)
			out[o++] = '/';
		memcpy(out + o, s, l);
		o += l;
	}
	out[o] = '\0';
}

static void
load_attrs(void)
{
	const char *file = getenv("EMIBSD_OWNERS"), *root = getenv("EMIBSD_STAGING");
	char line[2 * MAXPATHLEN_ATTR], path[MAXPATHLEN_ATTR];
	unsigned int mode, uid, gid;
	FILE *f;

	nattrs = 0;
	if (file == NULL || root == NULL)
		return;
	normalise(root, staging, sizeof(staging));
	if ((f = fopen(file, "r")) == NULL)
		return;
	while (fgets(line, sizeof(line), f) != NULL) {
		if (sscanf(line, "%o %u %u %255s", &mode, &uid, &gid, path) != 4)
			continue;
		if (nattrs == MAXATTR) {
			/* A table cut short would silently leave paths root:wheel 0755/0644. */
			fprintf(stderr, "makefs: %s: more than %d entries\n", file, MAXATTR);
			exit(1);
		}
		normalise(path, attrs[nattrs].path, MAXPATHLEN_ATTR);
		attrs[nattrs].mode = mode;
		attrs[nattrs].uid = uid;
		attrs[nattrs].gid = gid;
		nattrs++;
	}
	fclose(f);
}

int
emibsd_lstat(const char *path, struct stat *sb)
{
	char norm[MAXPATHLEN_ATTR];
	size_t sl;
	int i;

	if (lstat_device(path, sb) == -1)
		return -1;
	sb->st_uid = 0;
	sb->st_gid = 0;
	if (nattrs < 0)
		load_attrs();
	normalise(path, norm, sizeof(norm));
	sl = strlen(staging);
	if (strncmp(norm, staging, sl) != 0 || (norm[sl] != '/' && norm[sl] != '\0'))
		return 0;
	for (i = 0; i < nattrs; i++) {
		if (strcmp(attrs[i].path, norm + sl + (norm[sl] == '/')) != 0)
			continue;
		sb->st_mode = (sb->st_mode & S_IFMT) | (attrs[i].mode & 07777);
		sb->st_uid = attrs[i].uid;
		sb->st_gid = attrs[i].gid;
		break;
	}
	return 0;
}
"#;

/// Builds makefs and pwd_mkdb for this machine, stages `root/` plus `/etc`, `/dev` and the
/// directories, and writes `ramdisk.ffs`.
/// OpenBSD's makefs(8) built for this machine (with this module's host shims); the
/// executable. The install media (`miniroot.rs`) makes its images with it too.
pub(super) fn build_makefs(ctx: &Ctx<'_>) -> Result<PathBuf> {
    if !ctx.src.join(MAKEFS_DIR).join("Makefile").is_file() {
        return Err(format!("{MAKEFS_DIR}: not in the reference clone").into());
    }
    Ok(build_host_prog_with(ctx, MAKEFS_DIR, |mk, objdir| shim(ctx, mk, objdir))?.join("makefs"))
}

pub(super) fn build_ramdisk(ctx: &Ctx<'_>) -> Result<()> {
    if !ctx.src.join(MAKEFS_DIR).join("Makefile").is_file() {
        println!("  {MAKEFS_DIR}: not in the reference clone; no ramdisk image");
        return Ok(());
    }
    let makefs =
        build_host_prog_with(ctx, MAKEFS_DIR, |mk, objdir| shim(ctx, mk, objdir))?.join("makefs");
    let pwd_mkdb = passwd::build_pwd_mkdb(ctx)?;
    let root_hash = passwd::bcrypt_hash(ctx, passwd::ROOT_PASSWORD)?;

    let staging = ctx.out.join("ramdisk-root");
    if staging.exists() {
        fs::remove_dir_all(&staging).map_err(|e| format!("{}: {e}", staging.display()))?;
    }
    copy_tree(&ctx.out.join("root"), &staging, &mut HashMap::new())?;
    let etc = staging.join("etc");
    fs::create_dir_all(&etc).map_err(|e| format!("{}: {e}", etc.display()))?;
    let files = etc_files(&root_hash);
    for (name, _, text) in &files {
        let p = etc.join(name);
        fs::write(&p, text).map_err(|e| format!("{}: {e}", p.display()))?;
    }
    passwd::make_databases(&pwd_mkdb, &etc)?;
    // /etc/ssl (M9+): LibreSSL's CA bundle and the test CA (`testca.rs`).
    let ssl = testca::ssl_files(ctx)?;
    fs::create_dir_all(etc.join("ssl")).map_err(|e| format!("{}: {e}", etc.display()))?;
    for (name, from) in &ssl {
        let to = etc.join("ssl").join(name);
        fs::copy(from, &to).map_err(|e| format!("{}: {e}", from.display()))?;
    }
    for (dir, _) in DIRS {
        let p = staging.join(dir.trim_start_matches('/'));
        fs::create_dir_all(&p).map_err(|e| format!("{}: {e}", p.display()))?;
    }
    // M10c: the FAT, ISO 9660 and UDF images vnconfig(8) attaches (`images.rs`).
    images::make_images(ctx, &makefs, &staging.join("root/images"))?;
    // M12: the tone `just smoke-audio` plays with aucat(1).
    let tone = staging.join("root/tone.wav");
    fs::write(&tone, tone_wav()).map_err(|e| format!("{}: {e}", tone.display()))?;
    let dev = staging.join("dev");
    fs::create_dir_all(dev.join("fd")).map_err(|e| format!("{}: {e}", dev.display()))?;
    let device = |name: &str, kind: char, major: u32, minor: u32, mode: u32| -> Result<()> {
        let p = dev.join(name);
        fs::write(&p, device_line(kind, major, minor, mode))
            .map_err(|e| format!("{}: {e}", p.display()).into())
    };
    for (name, kind, major, minor, mode, _) in devices() {
        device(&name, kind, major, minor, mode)?;
    }
    for n in 0..FD_NODES {
        device(&format!("fd/{n}"), 'c', 22, n, 0o666)?;
    }
    for (name, target) in DEV_LINKS {
        symlink(target, &dev.join(name))?;
    }

    // Owners, groups and modes (module docs); the programs' own come from their Makefiles.
    let installed = ctx.owners.lock().map_err(|_| "lock poisoned")?.clone();
    let mut attrs = image_attrs(&installed);
    attrs.push(Attr::root("/etc/ssl", 0, 0o755));
    for (name, _) in &ssl {
        attrs.push(Attr::root(&format!("/etc/ssl/{name}"), 0, 0o444));
    }
    for n in 0..FD_NODES {
        attrs.push(Attr::root(&format!("/dev/fd/{n}"), 0, 0o666));
    }
    let owners = ctx.out.join("host/owners.txt");
    write_if_changed(&owners, &owners_table(&attrs))?;

    // The size (partition `a` of the disktab entry): twice the contents, in whole MiB, at
    // least 2 MiB (inodes, directories and indirect blocks fit with room to spare).
    const MIB: u64 = 1 << 20;
    let size = (tree_bytes(&staging)? * 2).div_ceil(MIB).max(2) * MIB;
    let disktab = ctx.out.join("host/disktab");
    write_if_changed(&disktab, &disktab_entry(size / 512))?;
    let image = ctx.out.join("ramdisk.ffs");
    let _ = fs::remove_file(&image);
    run(Command::new(&makefs)
        .args(["-t", "ffs", "-T", &TIMESTAMP.to_string()])
        .env("EMIBSD_DISKTAB", &disktab)
        .env("EMIBSD_OWNERS", &owners)
        .env("EMIBSD_STAGING", &staging)
        .args(["-o", FS_OPTIONS])
        .arg(&image)
        .arg(&staging))?;
    let size = fs::metadata(&image).map(|m| m.len()).unwrap_or(0);
    println!(
        "  ramdisk: {} ({size} bytes; makefs -t ffs -o {FS_OPTIONS}; /etc: {} pwd.db spwd.db passwd; \
         /dev: {} fd/0..{}; root password `{}`, hash {root_hash})",
        image.display(),
        files
            .iter()
            .map(|f| f.0.as_str())
            .collect::<Vec<_>>()
            .join(" "),
        devices()
            .iter()
            .map(|d| d.0.as_str())
            .collect::<Vec<_>>()
            .join(" "),
        FD_NODES - 1,
        passwd::ROOT_PASSWORD,
    );
    Ok(())
}

/// `/root/tone.wav` (M12): one second of a 440 Hz sine at half scale, 48 kHz, 16-bit
/// signed little-endian, two channels, in a RIFF WAVE file aucat(1) plays. `smoke-audio`
/// checks that QEMU's `wav` audio backend captured it (`devices.rs`, `--expect-tone`).
fn tone_wav() -> Vec<u8> {
    const RATE: u32 = 48_000;
    const CHANNELS: u16 = 2;
    let frames = RATE;
    let data_len = frames * u32::from(CHANNELS) * 2;
    let mut v = Vec::with_capacity(44 + data_len as usize);
    v.extend_from_slice(b"RIFF");
    v.extend_from_slice(&(36 + data_len).to_le_bytes());
    v.extend_from_slice(b"WAVEfmt ");
    v.extend_from_slice(&16u32.to_le_bytes());
    v.extend_from_slice(&1u16.to_le_bytes()); // PCM
    v.extend_from_slice(&CHANNELS.to_le_bytes());
    v.extend_from_slice(&RATE.to_le_bytes());
    v.extend_from_slice(&(RATE * u32::from(CHANNELS) * 2).to_le_bytes());
    v.extend_from_slice(&(CHANNELS * 2).to_le_bytes());
    v.extend_from_slice(&16u16.to_le_bytes());
    v.extend_from_slice(b"data");
    v.extend_from_slice(&data_len.to_le_bytes());
    for i in 0..frames {
        let t = f64::from(i) / f64::from(RATE);
        let s = (16_383.0 * (2.0 * std::f64::consts::PI * 440.0 * t).sin()).round() as i16;
        for _ in 0..CHANNELS {
            v.extend_from_slice(&s.to_le_bytes());
        }
    }
    v
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
        ("emibsd_compat.c", compat_c.as_str()),
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
    mk.add_path(&ctx.src.join("lib/libc/gen"));
    mk.set(
        "SRCS",
        &format!("{srcs} fmt_scaled.c disklabel.c emibsd_compat.c"),
    );
    println!(
        "  {MAKEFS_DIR} (host tool): built with the host shims of tools/xtask/src/userland/ramdisk.rs \
         (OpenBSD headers macOS lacks, daddr_t, st_*tim, MAXBSIZE, lstat for device nodes, \
         scan_scaled from lib/libutil, getdiskbyname from lib/libc/gen over $EMIBSD_DISKTAB)"
    );
    Ok(())
}

/// The bytes of the regular files under `dir`, each hard-linked file once.
pub(super) fn tree_bytes(dir: &Path) -> Result<u64> {
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
pub(super) fn write_if_changed(path: &Path, text: &str) -> Result<()> {
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
/* </CODE> */

/* <TESTS> */
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn master_passwd_has_ten_fields_and_a_root_hash() {
        let text = master_passwd("$2b$08$hash");
        let lines: Vec<&str> = text.lines().collect();
        assert_eq!(lines.len(), USERS.len());
        for l in &lines {
            assert_eq!(l.split(':').count(), 10, "{l}");
        }
        assert!(lines[0].starts_with("root:$2b$08$hash:0:0:daemon:0:0:"));
        assert!(lines[1].starts_with("daemon:*:1:1:"));
    }

    #[test]
    fn every_user_has_a_group_and_every_class_exists() {
        for (name, _, gid, class, ..) in USERS {
            assert!(GROUPS.iter().any(|g| g.1 == *gid), "{name}: gid {gid}");
            // An empty class is login.conf(5)'s `default`.
            let class = if class.is_empty() { "default" } else { class };
            assert!(
                LOGIN_CONF.contains(&format!("\n{class}:\\")),
                "{name}: login class {class}"
            );
        }
    }

    #[test]
    fn group_file_is_name_star_gid_members() {
        let text = group_file();
        assert!(text.starts_with("wheel:*:0:root\n"));
        assert!(text.contains("\nauth:*:11:\n"));
        assert!(text.contains("\nutmp:*:45:\n"));
        assert_eq!(text.lines().count(), GROUPS.len());
    }

    /// The users and groups whose lines come from the reference clone's `etc/`.
    const FROM_REFERENCE_ETC: &[&str] = &["_tcpdump", "_portmap", "_sndiop"];

    /// The users and groups taken from the reference clone's `etc/master.passwd` and
    /// `etc/group` reproduce their lines: `cargo test -p xtask -- --ignored` with
    /// `$OPENBSD_SRC` naming the clone.
    #[test]
    #[ignore]
    fn reference_etc_entries_match() {
        let src = std::path::PathBuf::from(std::env::var("OPENBSD_SRC").expect("OPENBSD_SRC"));
        let passwd = fs::read_to_string(src.join("etc/master.passwd")).expect("master.passwd");
        let group = fs::read_to_string(src.join("etc/group")).expect("group");
        let ours = master_passwd("");
        for name in FROM_REFERENCE_ETC {
            let line = |text: &str| {
                text.lines()
                    .find(|l| l.split(':').next() == Some(*name))
                    .map(str::to_string)
            };
            assert_eq!(line(&ours), line(&passwd), "{name} in master.passwd");
            assert_eq!(line(&group_file()), line(&group), "{name} in group");
        }
        // Every non-comment line of our rpc(5) is a line of the reference's.
        let rpc = fs::read_to_string(src.join("etc/rpc")).expect("rpc");
        for l in RPC.lines().filter(|l| !l.starts_with('#')) {
            assert!(rpc.lines().any(|r| r == l), "rpc: `{l}` is not in etc/rpc");
        }
    }

    #[test]
    fn tone_is_one_second_of_48k_stereo() {
        let w = tone_wav();
        assert_eq!(&w[0..4], b"RIFF");
        assert_eq!(&w[36..40], b"data");
        assert_eq!(w.len(), 44 + 48_000 * 4);
        let peak = w[44..]
            .chunks(2)
            .map(|c| i16::from_le_bytes([c[0], c[1]]).unsigned_abs())
            .max();
        assert_eq!(peak, Some(16_383));
    }

    #[test]
    fn ids_are_unique() {
        for (i, g) in GROUPS.iter().enumerate() {
            assert!(GROUPS[i + 1..].iter().all(|o| o.0 != g.0 && o.1 != g.1));
        }
        for (i, u) in USERS.iter().enumerate() {
            assert!(USERS[i + 1..].iter().all(|o| o.0 != u.0 && o.1 != u.1));
        }
    }

    #[test]
    fn installed_attrs_resolve_names_and_octal_modes() -> Result<()> {
        let a = Attr::installed("/usr/libexec/auth/login_passwd", "root", "auth", "4555")?;
        assert_eq!((a.uid, a.gid, a.mode), (0, 11, 0o4555));
        assert!(Attr::installed("/x", "nobody-such", "auth", "555").is_err());
        assert!(Attr::installed("/x", "root", "no-such", "555").is_err());
        assert!(Attr::installed("/x", "root", "auth", "9").is_err());
        Ok(())
    }

    #[test]
    fn image_attrs_cover_the_private_and_sticky_paths() {
        let attrs = image_attrs(&[]);
        let find = |p: &str| attrs.iter().find(|a| a.path == p);
        assert_eq!(find("/etc/master.passwd").map(|a| a.mode), Some(0o600));
        let spwd = find("/etc/spwd.db");
        assert_eq!(spwd.map(|a| (a.gid, a.mode)), Some((14, 0o640)));
        assert_eq!(find("/tmp").map(|a| a.mode), Some(0o1777));
        assert_eq!(find("/dev/kmem").map(|a| a.gid), Some(2));
        let mut paths: Vec<&str> = attrs.iter().map(|a| a.path.as_str()).collect();
        let n = paths.len();
        paths.sort_unstable();
        paths.dedup();
        assert_eq!(paths.len(), n, "a path is listed twice");
        assert!(owners_table(&attrs).contains("600 0 0 /etc/master.passwd\n"));
    }

    #[test]
    fn ttys_run_getty_on_the_console_tty() {
        assert!(TTYS.contains("tty00\t\"/usr/libexec/getty std.9600\"\tvt220\ton secure"));
        assert!(
            DEVICES
                .iter()
                .any(|d| d.0 == "tty00" && (d.2, d.3) == (8, 0))
        );
        assert!(
            DEVICES
                .iter()
                .any(|d| d.0 == "rd0a" && d.1 == 'b' && (d.2, d.3) == (17, 0))
        );
    }

    #[test]
    fn fd_nodes_follow_makedev() {
        let find = |n: &str| DEVICES.iter().find(|d| d.0 == n).map(|d| (d.1, d.2, d.3));
        assert_eq!(find("fd0c"), Some(('b', 2, 2)));
        assert_eq!(find("rfd0c"), Some(('c', 9, 2)));
        assert_eq!(find("fd0i"), Some(('b', 2, 8)));
    }

    #[test]
    fn sd_nodes_follow_makedev() {
        let all = devices();
        let find = |n: &str| all.iter().find(|d| d.0 == n);
        // Block 4 / char 13, minor unit * 64 + partition, 0640 operator.
        assert_eq!(
            find("sd0a").map(|d| (d.1, d.2, d.3, d.4, d.5)),
            Some(('b', 4, 0, 0o640, "operator"))
        );
        assert_eq!(find("sd0c").map(|d| (d.1, d.2, d.3)), Some(('b', 4, 2)));
        assert_eq!(find("sd0p").map(|d| (d.1, d.2, d.3)), Some(('b', 4, 15)));
        assert_eq!(find("rsd0a").map(|d| (d.1, d.2, d.3)), Some(('c', 13, 0)));
        assert_eq!(find("sd1a").map(|d| (d.1, d.2, d.3)), Some(('b', 4, 64)));
        assert_eq!(find("rsd1p").map(|d| (d.1, d.2, d.3)), Some(('c', 13, 79)));
        // M13: cd: block 6 / char 15, the same layout.
        assert_eq!(find("cd0c").map(|d| (d.1, d.2, d.3)), Some(('b', 6, 2)));
        assert_eq!(find("rcd1a").map(|d| (d.1, d.2, d.3)), Some(('c', 15, 64)));
        assert!(find("cd2a").is_none());
        // vnd: block 14 / char 41, the same layout.
        assert_eq!(find("vnd1c").map(|d| (d.1, d.2, d.3)), Some(('b', 14, 66)));
        assert_eq!(
            find("rvnd3p").map(|d| (d.1, d.2, d.3)),
            Some(('c', 41, 207))
        );
        assert_eq!(find("sd3a").map(|d| (d.1, d.2, d.3)), Some(('b', 4, 192)));
        assert_eq!(
            find("rsd15p").map(|d| (d.1, d.2, d.3)),
            Some(('c', 13, 975))
        );
        assert!(find("sd0q").is_none() && find("sd16a").is_none());
        // M10f: /dev/bio is cdevsw 79, 0600.
        assert_eq!(
            find("bio").map(|d| (d.1, d.2, d.3, d.4)),
            Some(('c', 79, 0, 0o600))
        );
        // M10d: /dev/fuse0 is cdevsw 92, minor 0, 0600 root:wheel.
        assert_eq!(
            find("fuse0").map(|d| (d.1, d.2, d.3, d.4, d.5)),
            Some(('c', 92, 0, 0o600, "wheel"))
        );
        // Every name once, and the attributes table gets them and /mnt.
        let mut names: Vec<&str> = all.iter().map(|d| d.0.as_str()).collect();
        let n = names.len();
        names.sort_unstable();
        names.dedup();
        assert_eq!(names.len(), n);
        let attrs = image_attrs(&[]);
        let attr = |p: &str| attrs.iter().find(|a| a.path == p);
        assert_eq!(
            attr("/dev/rsd1b").map(|a| (a.gid, a.mode)),
            Some((5, 0o640))
        );
        assert_eq!(attr("/mnt").map(|a| a.mode), Some(0o755));
    }

    #[test]
    fn rc_ends_multi_user() {
        assert!(RC.contains("mount -uw /"));
        assert!(RC.contains("echo 'rc: multi-user'"));
        assert!(RC.trim_end().ends_with("exit 0"));
    }

    #[test]
    fn check_devices_wants_every_node_of_the_table() {
        let dir = std::env::temp_dir().join(format!("emibsd-ramdisk-devs-{}", std::process::id()));
        let _ = fs::remove_dir_all(&dir);
        let image = dir.join("ramdisk.ffs");
        fs::create_dir_all(&dir).expect("mkdir");
        // No staging tree beside the image: not ours, not checked.
        assert!(check_devices(&image).is_ok());

        let dev = dir.join("ramdisk-root/dev");
        fs::create_dir_all(&dev).expect("mkdir dev");
        for (name, kind, major, minor, mode, _) in devices() {
            fs::write(dev.join(&name), device_line(kind, major, minor, mode)).expect("node");
        }
        assert!(check_devices(&image).is_ok());

        // An image made before a node joined the table, and one with an old minor.
        fs::remove_file(dev.join("wskbd0")).expect("rm");
        fs::write(dev.join("wsmouse1"), device_line('c', 68, 0, 0o600)).expect("node");
        let err = check_devices(&image).expect_err("stale").to_string();
        assert!(err.contains("/dev/wskbd0 /dev/wsmouse1 missing"), "{err}");
        assert!(err.contains("just userland"), "{err}");
        let _ = fs::remove_dir_all(&dir);
    }
}
/* </TESTS> */
