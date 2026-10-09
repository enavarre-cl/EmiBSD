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
//! `cargo xtask userland --arch A`: cross-compiles OpenBSD's own userland C, unmodified, from
//! the reference clone (milestone M8, decided by the user on 2026-10-03).
//!
//! What it builds, all under `target/userland/<arch>/`:
//!
//! 1. `sysroot/usr/include`, assembled the way `include/Makefile` installs `/usr/include`
//!    (`includes` + `copies`): `FILES`, `DIRS`, the `LFILES`/`MFILES` symlinks, the kernel
//!    headers of `LDIRS`, `<machine/*>` from `sys/arch/<MACHINE>/include`, and of the `RDIRS`
//!    those of `lib/libutil` and `lib/librpcsvc` (made by OpenBSD's `rpcgen`, built for this
//!    machine first), the only ones what is built here needs.
//! 2. `lib/csu` (`crt0.o`, `rcrt0.o`, `crtbegin.o`, `crtend.o`, ...) by running the rules of
//!    its Makefile, into `sysroot/usr/lib`.
//! 3. `libc.a` and `libutil.a`: `SRCS`, `OBJS` and `.PATH` come from evaluating the libraries'
//!    Makefiles with `bsdmake.rs`; the system-call stubs are made by the rules of
//!    `lib/libc/sys/Makefile.inc` (`GENERATE.*` piped into `FINISH.*`), run through `/bin/sh`
//!    exactly as make would; generated C (the `lib/libc/hash` helpers) likewise. Then (M14)
//!    the shared libraries `libc.so.M.m`, `libutil`, `libm`, `libpthread` and the run-time
//!    link-editor `/usr/libexec/ld.so`, as `bsd.lib.mk` and ld.so's Makefile make them
//!    (`userland/shlib.rs`), into the sysroot and `root/`.
//! 4. The programs of `PROGRAMS` (`sbin/init`, `bin/ksh`, `sbin/mount`, `libexec/getty`,
//!    `usr.bin/login`, `libexec/login_passwd`, ...), linked as static PIE executables (what
//!    OpenBSD's `cc -static` makes for `/bin` and `/sbin`), installed stripped into `root/`
//!    where their Makefile's `BINDIR` says, with the `BINOWN`/`BINGRP`/`BINMODE` it sets.
//! 5. `ramdisk.ffs`: `root/` plus `/etc`, `/dev` and the directories of a multi-user system,
//!    made into an ffs image by OpenBSD's makefs(8) built for this machine (`ramdisk.rs`),
//!    with `pwd.db` and `spwd.db` made by OpenBSD's pwd_mkdb(8), also built for this machine
//!    (`passwd.rs`).
//!
//! `share/mk` is not in the reference clone. `sys.mk` and `bsd.own.mk` are stood in for by
//! the predefined variables and `BSD_OWN_MK` below; `bsd.prog.mk`/`bsd.lib.mk` by
//! `BSD_PROG_MK` and the implicit `.c.o`/`.S.o` rules (`RULE_C_O`, `RULE_S_O`). Every compiler
//! flag workaround is in `UNSUPPORTED_FLAGS`, `HOST_CFLAGS` and `VARIANTS`, and in
//! `docs/ARCHITECTURE.md`. `-lcompiler_rt` is linked only when the clone has
//! `COMPILER_RT_DIR` (added to the sparse set on 2026-10-03); without it arm64 programs do
//! not link (`__multf3`).
//!
//! Tools (approved by the user on 2026-10-03): Apple clang (`$EMIBSD_CC`, default
//! `/usr/bin/clang`) and LLVM's `ld.lld`, `llvm-ar`, `llvm-ranlib`, `llvm-objcopy`,
//! `llvm-objdump` from `$EMIBSD_LLVM_BIN` (default `~/.swiftly/bin`). The OpenBSD sources come
//! from `$OPENBSD_SRC`, else `reference/openbsd-src` of this checkout, else of the main
//! checkout when run from a git worktree. Nothing is ever written there.

use std::collections::{BTreeMap, BTreeSet, HashMap};
use std::fs;
use std::io::Write as _;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::sync::Mutex;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::time::SystemTime;

use crate::Result;
use crate::boot::Arch;
use crate::bsdmake::{Locals, Make};

/// `<bsd.own.mk>`: the OpenBSD defaults the Makefiles read.
const BSD_OWN_MK: &str = "\
YP?=\t\tyes
COMPILER_VERSION?=\tclang
BUILD_CLANG?=\tyes
STATIC?=\t-static
NOPIE_FLAGS?=\t-fno-pie
PICFLAG?=\t-fpic
";

/// `<bsd.prog.mk>` and `<bsd.lib.mk>`: the parts that change variables. `CDIAGFLAGS`
/// (warnings only) is left empty.
const BSD_PROG_MK: &str = "\
.if exists(${.CURDIR}/../Makefile.inc)
.include \"${.CURDIR}/../Makefile.inc\"
.endif
.include <bsd.own.mk>
.if ${WARNINGS:L} == \"yes\"
CFLAGS+=\t${CDIAGFLAGS}
.endif
CFLAGS+=\t${COPTS}
";

/// `<bsd.lib.mk>`: as `BSD_PROG_MK`, but `<bsd.own.mk>` comes first, as in the real one
/// (`share/mk/bsd.lib.mk`), so `../Makefile.inc` sees its defaults: the compiler's
/// `gnu/usr.bin/clang/Makefile.inc` sets `CXX=clang++` unless `COMPILER_VERSION` is clang
/// (M14).
const BSD_LIB_MK: &str = "\
.include <bsd.own.mk>
.if exists(${.CURDIR}/../Makefile.inc)
.include \"${.CURDIR}/../Makefile.inc\"
.endif
.if ${WARNINGS:L} == \"yes\"
CFLAGS+=\t${CDIAGFLAGS}
.endif
CFLAGS+=\t${COPTS}
";

/// `bsd.lib.mk`'s `.c.o` rule, without its final `ld -X -r` (which only drops local
/// temporary symbols).
const RULE_C_O: &str = "${COMPILE.c} ${DFLAGS} -MF ${.TARGET:R}.d ${.IMPSRC} -o ${.TARGET}";
/// `bsd.lib.mk`'s `.cpp.o` (and `.cc.o`) rule, likewise: the compiler's sources (M14,
/// `comp.rs`).
const RULE_CC_O: &str = "${COMPILE.cc} ${DFLAGS} -MF ${.TARGET:R}.d ${.IMPSRC} -o ${.TARGET}";
/// `bsd.lib.mk`'s `.S.o` rule, likewise.
const RULE_S_O: &str = "${COMPILE.S} ${CFLAGS:M-[IDM]*} ${AINC} ${DFLAGS} -MF ${.TARGET:R}.d \
                        -o ${.TARGET} ${.IMPSRC}";
/// `bsd.lib.mk`'s `.c.so` rule (M14, shared libraries), likewise; its `.d` file is named
/// after the whole target (`foo.so.d`), so it does not replace the `.o`'s `foo.d`.
const RULE_C_SO: &str =
    "${COMPILE.c} ${DFLAGS} ${PICFLAG} -DPIC -MF ${.TARGET}.d ${.IMPSRC} -o ${.TARGET}";
/// `bsd.lib.mk`'s `.cpp.so` rule, likewise.
const RULE_CC_SO: &str =
    "${COMPILE.cc} ${DFLAGS} ${PICFLAG} -DPIC -MF ${.TARGET}.d ${.IMPSRC} -o ${.TARGET}";
/// `bsd.lib.mk`'s `.S.so` rule, likewise.
const RULE_S_SO: &str = "${COMPILE.S} ${DFLAGS} -MF ${.TARGET}.d ${PICFLAG} -DSOLIB \
                         ${CFLAGS:M-[IDM]*} ${AINC} ${.IMPSRC} -o ${.TARGET}";

/// Flags of OpenBSD's patched base clang that Apple clang does not know, removed from
/// `CFLAGS`/`AFLAGS`/`COPTS` after evaluation, with the reason.
const UNSUPPORTED_FLAGS: &[(&str, &str)] = &[
    (
        "-fret-clean",
        "OpenBSD-local clang option (lib/libc/arch/amd64/Makefile.inc): clears the return \
         address slot after use; Apple clang rejects it",
    ),
    (
        "-fno-ret-clean",
        "OpenBSD-local clang option (distrib/special/Makefile.inc, M14c): the negation of \
         -fret-clean, which Apple clang does not have either",
    ),
    (
        "-fno-ret-protector",
        "OpenBSD-local clang option (gnu/usr.bin/clang/Makefile.inc, M14): turns off the \
         return-address protector OpenBSD's clang adds by default, which Apple clang \
         neither adds nor knows",
    ),
];

/// The `UNSUPPORTED_FLAGS` already reported in this run.
static REPORTED_FLAGS: Mutex<BTreeSet<&str>> = Mutex::new(BTreeSet::new());

/// Warnings OpenBSD's base clang leaves off by default and Apple clang turns on, switched
/// off where a Makefile makes warnings errors (`-Werror`: LibreSSL's), with the reason.
/// Elsewhere they stay mere warnings, and the command lines (and objects) are unchanged.
const WERROR_DEFAULTS: &[(&str, &str)] = &[(
    "-Wno-pointer-sign",
    "OpenBSD's clang does not warn about char/unsigned char pointer mixes by default \
     (libcrypto builds with -Werror there and mixes them); Apple clang does",
)];

/// A program built differently from its Makefile, and why.
struct Variant {
    dir: &'static str,
    add_cflags: &'static str,
    drop_ldadd: &'static [&'static str],
    /// Link `-static` although the Makefiles do not say so (`/usr/bin` programs are
    /// dynamic on OpenBSD; the ramdisk's programs stay static, see `Variant::statically`).
    static_link: bool,
    why: &'static str,
}

impl Variant {
    /// A program of a dynamic directory (`/usr/bin`, `/usr/sbin`, `/usr/libexec`) linked
    /// `-static`.
    const fn statically(dir: &'static str) -> Self {
        Variant {
            dir,
            add_cflags: "",
            drop_ldadd: &[],
            static_link: true,
            why: "linked -static, as the install media's crunched programs are: /usr/bin, \
                  /usr/sbin and /usr/libexec are dynamic on OpenBSD; the ramdisk keeps every \
                  program static (ld.so and the shared libraries, M14, serve what `cc` links)",
        }
    }
}

// `bin/ksh` is built by its own Makefile (M14c: with `-lcurses`, without `-DSMALL`), as
// OpenBSD's base set has it; the install media's `-DSMALL` ksh is `distrib/special/ksh`,
// which `userland/miniroot.rs` builds for the miniroot only (`MINIROOT_ONLY`).
const VARIANTS: &[Variant] = &[
    Variant::statically("usr.bin/id"),
    Variant::statically("usr.bin/uname"),
    Variant::statically("libexec/getty"),
    Variant::statically("usr.bin/login"),
    Variant::statically("libexec/login_passwd"),
    Variant::statically("usr.bin/ftp"),
    Variant::statically("usr.bin/nc"),
    Variant::statically("usr.sbin/quotaon"),
    Variant::statically("usr.sbin/edquota"),
    Variant::statically("usr.sbin/repquota"),
    Variant::statically("usr.bin/quota"),
    Variant::statically("usr.bin/su"),
    Variant::statically("usr.bin/fstat"),
    Variant::statically("usr.bin/vmstat"),
    // M10e: the NFS userland. portmap and showmount are in dynamic directories; mountd and
    // nfsd are in /sbin but their Makefiles end with `LDSTATIC=` (not static by default).
    Variant::statically("usr.sbin/portmap"),
    Variant::statically("usr.bin/showmount"),
    Variant {
        dir: "sbin/mountd",
        add_cflags: "",
        drop_ldadd: &[],
        static_link: true,
        why: "linked -static: its Makefile ends with `LDSTATIC=` (dynamic, as OpenBSD ships \
              it); the ramdisk keeps every program static",
    },
    Variant {
        dir: "sbin/nfsd",
        add_cflags: "",
        drop_ldadd: &[],
        static_link: true,
        why: "linked -static: its Makefile ends with `LDSTATIC=` (dynamic, as OpenBSD ships \
              it); the ramdisk keeps every program static",
    },
    // M11e: the two-VM network stress.
    Variant::statically("usr.bin/tcpbench"),
    // M12: the audio programs and cmp(1).
    Variant::statically("usr.bin/cmp"),
    Variant::statically("usr.bin/audioctl"),
    Variant::statically("usr.bin/mixerctl"),
    Variant::statically("usr.bin/aucat"),
    // M12+: the file utilities of `diff-openbsd`'s scenarios that live in /usr/bin.
    Variant::statically("usr.bin/readlink"),
    Variant::statically("usr.bin/stat"),
    Variant::statically("usr.bin/touch"),
    Variant::statically("usr.bin/wc"),
    // M14: ld.so's ldd(1) runs a program with `LD_TRACE_LOADED_OBJECTS` set and needs no
    // dynamic linking of its own (only its `dlopen` of a shared object would); chroot(8).
    Variant::statically("libexec/ld.so/ldd"),
    Variant::statically("usr.sbin/chroot"),
    // M14c: the install media's programs of dynamic directories, and the daemons whose
    // Makefiles end with `LDSTATIC=`.
    Variant::statically("usr.bin/arch"),
    Variant::statically("usr.bin/compress"),
    Variant::statically("usr.bin/doas"),
    Variant::statically("usr.bin/encrypt"),
    Variant::statically("usr.bin/grep"),
    Variant::statically("usr.bin/sed"),
    Variant::statically("usr.bin/signify"),
    Variant::statically("usr.bin/tee"),
    Variant::statically("usr.sbin/installboot"),
    Variant::statically("usr.sbin/pwd_mkdb"),
    // M16b: usbdevs(8).
    Variant::statically("usr.sbin/usbdevs"),
    Variant::statically("sbin/dhcpleased"),
    Variant::statically("sbin/resolvd"),
    Variant::statically("sbin/slaacd"),
    Variant {
        dir: "usr.sbin/tcpdump",
        add_cflags: "",
        drop_ldadd: &[],
        static_link: true,
        why: "linked -static, as the install media's crunched programs are: /usr/sbin is \
              dynamic on OpenBSD; the ramdisk keeps every program static",
    },
];

/// OpenBSD's compiler runtime (the `-lcompiler_rt` its clang driver adds to every link): a
/// Makefile over `gnu/llvm/compiler-rt` (Apache-2.0 WITH LLVM-exception), both in the sparse
/// clone since 2026-10-03. When the clone has it, it is built like the other libraries and
/// linked; `BUILD_CLANG` in `BSD_OWN_MK` selects its clang branch.
const COMPILER_RT_DIR: &str = "gnu/lib/libcompiler_rt";

/// OpenBSD's yacc(1), built for this machine the first time a `.y` source is met. It is
/// never macOS's `/usr/bin/yacc` (bison).
const YACC_DIR: &str = "usr.bin/yacc";

/// The header force-included into yacc's sources: macOS's libc has no `reallocarray(3)`.
const YACC_COMPAT_H: &str = "\
/* EmiBSD: host shim for building OpenBSD's yacc(1) on macOS (tools/xtask, userland.rs). */
#include <errno.h>
#include <stdint.h>
#include <stdlib.h>
static inline void *
emibsd_reallocarray(void *p, size_t n, size_t size)
{
\tif (size != 0 && n > SIZE_MAX / size) {
\t\terrno = ENOMEM;
\t\treturn NULL;
\t}
\treturn realloc(p, n * size);
}
#define reallocarray emibsd_reallocarray
";

/// `bsd.sys.mk`'s `.y.c` rule (`YACC.y` is `${YACC} -d ${YFLAGS}`): yacc writes the `.c` file
/// and, named after it, its `.h` (`-o`, yacc(1)) into the object directory. libpcap's
/// scanner includes the `grammar.h` that `grammar.y` makes so.
const RULE_Y_C: [&str; 1] = ["${YACC.y} -o ${.TARGET} ${.IMPSRC}"];

/// OpenBSD's lex(1) (flex 2.5.39), built for this machine the first time a `.l` source is
/// met, as yacc is. Its own scanner (`scan.l`; the tree has no `initscan.c` to bootstrap
/// from) is made by the Mac's `BOOTSTRAP_LEX`, as OpenBSD makes it with the lex already
/// installed; every `.l` of the target's userland goes through OpenBSD's lex.
const LEX_DIR: &str = "usr.bin/lex";

/// The lex that makes OpenBSD lex's own `scan.c` (part of the Xcode command line tools, like
/// `/usr/bin/clang`, `/usr/bin/perl` and `/usr/bin/awk`).
const BOOTSTRAP_LEX: &str = "/usr/bin/lex";

/// `bsd.sys.mk`'s `.l.c` rule.
const RULE_L_C: [&str; 1] = ["${LEX.l} -o ${.TARGET} ${.IMPSRC}"];

/// The programs, in build order.
const PROGRAMS: &[&str] = &[
    "sbin/init",
    "sbin/sysctl",
    "bin/ksh",
    "bin/cat",
    "bin/date",
    "bin/echo",
    "bin/hostname",
    "bin/ls",
    "bin/pwd",
    "bin/sleep",
    "usr.bin/id",
    "usr.bin/uname",
    "sbin/mount",
    "sbin/mount_ffs",
    "libexec/getty",
    "usr.bin/login",
    "libexec/login_passwd",
    "sbin/ifconfig",
    "sbin/ping",
    "sbin/route",
    "sbin/pfctl",
    "sbin/ipsecctl",
    // M9+: HTTPS clients over LIBRARIES.
    "usr.bin/ftp",
    "usr.bin/nc",
    // Diagnostic tools stage 2, over libkvm.
    "bin/ps",
    "bin/df",
    "usr.bin/fstat",
    "usr.bin/vmstat",
    // M9+: over libpcap (LIBRARIES); `iapp.h` from usr.sbin/hostapd (-I../hostapd).
    "usr.sbin/tcpdump",
    // M10b: disk quotas (quota(1) over librpcsvc, LIBRARIES) and su(1) to write as a user
    // under quota; mount_mfs(8) is newfs (its LINKS). mkdir(1) and chmod(1) (with its chgrp
    // and /sbin/chown links) to give that user a directory.
    "bin/mkdir",
    "bin/chmod",
    "sbin/quotacheck",
    "usr.sbin/quotaon",
    "usr.sbin/edquota",
    "usr.sbin/repquota",
    "usr.bin/quota",
    "usr.bin/su",
    // M10a: the disk tools, over libutil.
    "sbin/umount",
    "sbin/newfs",
    "sbin/fsck",
    "sbin/fsck_ffs",
    "sbin/disklabel",
    "sbin/fdisk",
    // M10c: the memory and removable file systems, vnd(4)'s tools, and dd(1) to make an
    // empty image for newfs_msdos.
    "bin/dd",
    "sbin/mount_tmpfs",
    "sbin/mount_msdos",
    "sbin/mount_cd9660",
    "sbin/mount_udf",
    "sbin/newfs_msdos",
    "sbin/fsck_msdos",
    "sbin/vnconfig",
    "sbin/mount_vnd",
    // M10f: softraid's control program, over libutil (bcrypt_pbkdf, pkcs5_pbkdf2, opendev).
    "sbin/bioctl",
    // M10e: the NFS userland, over librpcsvc (LIBRARIES): portmap(8), mountd(8), nfsd(8),
    // mount_nfs(8) and showmount(8).
    "usr.sbin/portmap",
    "sbin/mountd",
    "sbin/nfsd",
    "sbin/mount_nfs",
    "usr.bin/showmount",
    // M10d: the other disk file systems. ext2fs's tools (newfs_ext2fs and fsck_ext2fs take
    // `ext2fs_bswap.c` from sys/ufs/ext2fs, fsck_ext2fs `fsutil.c` from sbin/fsck), and
    // mount_ntfs(8), whose Makefile builds it only for alpha, amd64 and i386 (`NOPROG=`
    // elsewhere: skipped on arm64, `build_prog`).
    "sbin/newfs_ext2fs",
    "sbin/fsck_ext2fs",
    "sbin/mount_ext2fs",
    "sbin/mount_ntfs",
    // M11e: tcpbench(1), the network stress between two VMs, over libevent (LIBRARIES),
    // LibreSSL and libm.
    "usr.bin/tcpbench",
    // M12: what the USB stick smoke copies and compares files with (cp(1), rm(1), cmp(1),
    // and md5(1) for its `cksum` link), and audio(4)'s userland: audioctl(8) and
    // mixerctl(8) on `/dev/audioctl0`, and aucat(1) over libsndio (LIBRARIES), which plays
    // through `/dev/audio0` (`rsnd/0`) when no sndiod(8) runs, as `sio_open(3)` falls back.
    "bin/cp",
    "bin/rm",
    "bin/md5",
    "usr.bin/cmp",
    "usr.bin/audioctl",
    "usr.bin/mixerctl",
    "usr.bin/aucat",
    // M12+: the other file utilities `cargo xtask diff-openbsd`'s file-system scenarios run on
    // both systems (`tools/xtask/diff-openbsd/fs.scn`; cp and rm are above).
    "bin/ln",
    "bin/mv",
    "bin/rmdir",
    "usr.bin/readlink",
    "usr.bin/stat",
    "usr.bin/touch",
    "usr.bin/wc",
    // M13: reboot(8) and its halt link, over libutil (logwtmp): `halt -p` powers the machine
    // off through ACPI S5 and `reboot` resets it through the FADT's reset register
    // (`just smoke-power`).
    "sbin/reboot",
    // M14: ld.so's own tools, ldconfig(8) (static by its Makefile) and ldd(1), and chroot(8),
    // which `just smoke-cc` runs the compiler and the dynamic program it makes under.
    "libexec/ld.so/ldconfig",
    "libexec/ld.so/ldd",
    "usr.sbin/chroot",
    // M14c: what the install media's list (`distrib/amd64/ramdisk_cd/list`) and the installed
    // system's `/etc/rc` run, built from the normal Makefiles (not `distrib/special`'s
    // -DSMALL ones, docs/ARCHITECTURE.md "The install media"). libz (`LIBRARIES`) is gzip's
    // and grep's.
    "bin/ed",
    "bin/mt",
    "bin/pax",
    "bin/stty",
    "bin/sync",
    "sbin/dmesg",
    "sbin/growfs",
    "sbin/kbd",
    "sbin/mknod",
    "sbin/restore",
    "sbin/dhcpleased",
    "sbin/resolvd",
    "sbin/slaacd",
    "usr.bin/arch",
    "usr.bin/compress",
    "usr.bin/doas",
    "usr.bin/encrypt",
    "usr.bin/grep",
    "usr.bin/sed",
    "usr.bin/signify",
    "usr.bin/tee",
    "usr.sbin/installboot",
    "usr.sbin/pwd_mkdb",
    // M16b: usbdevs(8), the USB device tree over /dev/usb0's USB_DEVICEINFO.
    "usr.sbin/usbdevs",
];

/// Scripts that a Makefile's `afterinstall` rule installs next to the program: (directory, files).
const AFTERINSTALL_SCRIPTS: &[(&str, &[&str])] = &[(
    "usr.bin/compress",
    &["zmore", "zdiff", "zforce", "gzexe", "znew"],
)];

/// EmiBSD's own test programs, built after `PROGRAMS` the same way (an OpenBSD-style Makefile,
/// `build_prog`) from directories of this repository instead of the reference tree: paths
/// relative to the workspace root. `tools/sr6create` makes a RAID 6 softraid(4) volume, which
/// OpenBSD's own bioctl(8) refuses to; `tools/fusehello` (M10d) is a read-only FUSE file
/// system over OpenBSD's libfuse (`LIBRARIES`); `tools/difftest` (M12+) holds the system call
/// probes `cargo xtask diff-openbsd` runs on EmiBSD and on a real OpenBSD. Their sources are
/// not OpenBSD's, so the licence report (which lists only the reference tree's files) does not
/// name them.
const OWN_PROGRAMS: &[&str] = &["tools/sr6create", "tools/fusehello", "tools/difftest"];

/// Programs whose Makefile embeds their manual page in a generated `manual.c` (`disklabel`'s
/// and `fdisk`'s `-h`/`help` pager): the Makefile renders `*.8` with mandoc(1), which this
/// machine may not have, so they are built the way its `.ifdef NOMAN` branch says, with the
/// text `no manual` in place of the page. Only that string differs; every code path is built.
const NOMAN_PROGRAMS: &[&str] = &["sbin/disklabel", "sbin/fdisk"];

/// Libraries built after libc, libutil, libm and libcompiler_rt (M9+), in link order of
/// dependence: LibreSSL (`libcrypto`, `libssl`, `libtls`) for ftp(1) and nc(1), and ncurses
/// (`libcurses`) under `libedit`, ftp(1)'s command-line editing. Their headers are installed
/// by running each one's own `includes` rule (`library_includes`), and their generated
/// sources (`BUILDFIRST`: libcrypto's perlasm `.S` files and `obj_mac.h`, libcurses's
/// tables and the host-built `make_keys`/`make_hash`, libedit's `makelist` headers) by
/// running its rules (`make_target`). `libpcap` (tcpdump(8)) has its scanner made by
/// OpenBSD's lex and its grammar by OpenBSD's yacc, both built for this machine. `librpcsvc`
/// (quota(1), M10b) has its sources made from its `.x` files by OpenBSD's `rpcgen`.
/// `libfuse` (M10d) is the FUSE library our own `tools/fusehello` links; its `includes` rule
/// installs `<fuse/*.h>`, and its sources include the kernel's `<sys/fusebuf.h>`.
/// `libevent` (M11e) is tcpbench(1)'s event loop; its `includes` rule installs `<event.h>`
/// and `<evutil.h>`. `libsndio` (M12) is aucat(1)'s audio library; `<sndio.h>` is one of
/// `include/`'s own headers.
const LIBRARIES: &[&str] = &[
    "lib/libcrypto",
    "lib/libssl",
    "lib/libtls",
    "lib/libcurses",
    "lib/libedit",
    "lib/libpcap",
    "lib/librpcsvc",
    "lib/libfuse",
    "lib/libevent",
    "lib/libsndio",
    // M14c: gzip(1) and grep(1) of the install media and the base set.
    "lib/libz",
];

/// Flags added to host tools (built for macOS with the same clang) and why.
const HOST_CFLAGS: &[(&str, &str)] = &[(
    "'-Dpledge(p,e)=0'",
    "rpcgen, makefs and pwd_mkdb call pledge(2), which macOS does not have; the call only \
     restricts the tool itself",
)];

/// One architecture's names, as OpenBSD's make sees them.
struct Machine {
    machine: &'static str,
    machine_arch: &'static str,
    machine_cpu: &'static str,
    triple: &'static str,
    e_machine: u16,
}

impl Machine {
    fn of(arch: Arch) -> Self {
        match arch {
            Arch::Amd64 => Machine {
                machine: "amd64",
                machine_arch: "amd64",
                machine_cpu: "amd64",
                triple: "x86_64-unknown-openbsd",
                e_machine: 62,
            },
            Arch::Arm64 => Machine {
                machine: "arm64",
                machine_arch: "aarch64",
                machine_cpu: "aarch64",
                triple: "aarch64-unknown-openbsd",
                e_machine: 183,
            },
        }
    }
}

/// The external tools, verified to exist.
struct Tools {
    cc: PathBuf,
    ld: PathBuf,
    ar: PathBuf,
    ranlib: PathBuf,
    objcopy: PathBuf,
    objdump: PathBuf,
}

impl Tools {
    fn locate() -> Result<Self> {
        let cc = std::env::var_os("EMIBSD_CC")
            .map(PathBuf::from)
            .unwrap_or_else(|| PathBuf::from("/usr/bin/clang"));
        let bindir = match std::env::var_os("EMIBSD_LLVM_BIN") {
            Some(d) => PathBuf::from(d),
            None => {
                let home = std::env::var_os("HOME").ok_or("$HOME is not set")?;
                PathBuf::from(home).join(".swiftly").join("bin")
            }
        };
        let need = |p: PathBuf, env: &str| -> Result<PathBuf> {
            if p.is_file() {
                Ok(p)
            } else {
                Err(format!(
                    "{} not found; see docs/SETUP.md (\"Userland toolchain\") or set ${env}",
                    p.display()
                )
                .into())
            }
        };
        Ok(Tools {
            cc: need(cc, "EMIBSD_CC")?,
            ld: need(bindir.join("ld.lld"), "EMIBSD_LLVM_BIN")?,
            ar: need(bindir.join("llvm-ar"), "EMIBSD_LLVM_BIN")?,
            ranlib: need(bindir.join("llvm-ranlib"), "EMIBSD_LLVM_BIN")?,
            objcopy: need(bindir.join("llvm-objcopy"), "EMIBSD_LLVM_BIN")?,
            objdump: need(bindir.join("llvm-objdump"), "EMIBSD_LLVM_BIN")?,
        })
    }
}

/// Everything a build step needs.
pub(crate) struct Ctx<'a> {
    /// The workspace root (the home of `OWN_PROGRAMS`).
    root: PathBuf,
    src: PathBuf,
    out: PathBuf,
    sysroot: PathBuf,
    m: Machine,
    tools: &'a Tools,
    /// Installed header → its file in the reference tree (for the licence report).
    installed: Mutex<HashMap<PathBuf, PathBuf>>,
    /// Lower-cased installed path → installed path: two names that differ only in case
    /// would be one file on macOS's default file system.
    lower: Mutex<HashMap<String, PathBuf>>,
    /// Every input file a compile read (sources and headers), for the licence report.
    inputs: Mutex<BTreeSet<PathBuf>>,
    /// Owner, group and mode of installed files whose Makefile sets `BINOWN`, `BINGRP` or
    /// `BINMODE` (the ramdisk image applies them, `ramdisk.rs`).
    owners: Mutex<Vec<ramdisk::Attr>>,
    /// Where yacc(1) was built, once a `.y` source asked for it.
    yacc: Mutex<Option<PathBuf>>,
    /// Where lex(1) was built, once a `.l` source asked for it.
    lex: Mutex<Option<PathBuf>>,
}

/// The build context of `userland` for `arch`: the reference sources, `target/userland/<arch>`
/// as the output, `tools` as the compiler and linker.
fn new_ctx<'a>(root: &Path, arch: Arch, tools: &'a Tools) -> Result<Ctx<'a>> {
    let src = fs::canonicalize(openbsd_src(root)?)?;
    let out = root.join("target").join("userland").join(arch.name());
    for p in [&src, &out] {
        if p.to_string_lossy().contains(char::is_whitespace) {
            return Err(format!("{}: paths with whitespace are not supported", p.display()).into());
        }
    }
    let sysroot = out.join("sysroot");
    Ok(Ctx {
        root: root.to_path_buf(),
        src,
        sysroot,
        m: Machine::of(arch),
        tools,
        installed: Mutex::new(HashMap::new()),
        lower: Mutex::new(HashMap::new()),
        inputs: Mutex::new(BTreeSet::new()),
        owners: Mutex::new(Vec::new()),
        yacc: Mutex::new(None),
        lex: Mutex::new(None),
        out,
    })
}

/// Runs `f` with the context of an already built `userland` (the install media and the sets
/// of `cargo xtask miniroot`/`sets`, which only add the host tools and read the staged files).
pub(crate) fn with_ctx<R>(
    root: &Path,
    arch: Arch,
    f: impl FnOnce(&Ctx<'_>) -> Result<R>,
) -> Result<R> {
    let tools = Tools::locate()?;
    let ctx = new_ctx(root, arch, &tools)?;
    if !ctx.out.join("root").is_dir() {
        return Err(format!(
            "{}: no userland build; run `just userland` first",
            ctx.out.join("root").display()
        )
        .into());
    }
    f(&ctx)
}

/// `cargo xtask userland --arch A`.
pub fn userland(root: &Path, arch: Arch) -> Result<()> {
    let tools = Tools::locate()?;
    let ctx = new_ctx(root, arch, &tools)?;
    println!(
        "userland {}: OpenBSD sources {}, output {}",
        arch.name(),
        ctx.src.display(),
        ctx.out.display()
    );
    println!("  cc {}: {}", tools.cc.display(), tool_version(&tools.cc)?);
    println!("  ld {}: {}", tools.ld.display(), tool_version(&tools.ld)?);

    install_includes(&ctx)?;
    build_csu(&ctx)?;
    build_lib(&ctx, "lib/libc")?;
    build_lib(&ctx, "lib/libutil")?;
    build_lib(&ctx, "lib/libm")?;
    build_lib(&ctx, "lib/libkvm")?;
    if has_compiler_rt(&ctx) {
        build_lib(&ctx, COMPILER_RT_DIR)?;
    } else {
        println!("  {COMPILER_RT_DIR}: not in the reference clone; links go without it");
    }
    for dir in LIBRARIES {
        build_lib(&ctx, dir)?;
    }
    // M14: libpthread (static, as `comp` also builds it), the shared libraries and ld.so.
    build_lib(&ctx, "lib/librthread")?;
    shlib::build_shared(&ctx)?;
    let mut built = Vec::new();
    let mut blocked = Vec::new();
    for dir in PROGRAMS.iter().chain(OWN_PROGRAMS) {
        match build_prog(&ctx, dir)? {
            Linked::Yes(prog, exe, installed) => built.push((prog, exe, installed)),
            Linked::NeedsCompilerRt(symbols) => {
                println!(
                    "  {dir}: NOT LINKED: needs {} from libcompiler_rt",
                    symbols.join(" ")
                );
                blocked.push(*dir);
            }
            Linked::NoProg => println!(
                "  {dir}: NOPROG: its Makefile builds nothing for MACHINE={}",
                ctx.m.machine
            ),
        }
    }
    if !built.is_empty() {
        println!("  executables (static PIE, installed stripped under root/):");
    }
    for (name, path, installed) in &built {
        verify(&ctx, name, path, installed)?;
    }
    if blocked.is_empty() {
        ramdisk::build_ramdisk(&ctx)?;
        licence_report(&ctx)?;
        return Ok(());
    }
    licence_report(&ctx)?;
    Err(format!(
        "{}: {} not linked: they need compiler builtins that OpenBSD takes from \
         libcompiler_rt ({COMPILER_RT_DIR} over gnu/llvm/compiler-rt, Apache-2.0 WITH \
         LLVM-exception), which is not in the sparse reference clone; widening the clone is \
         the user's decision (docs/SETUP.md, \"Userland toolchain\")",
        arch.name(),
        blocked.join(", ")
    )
    .into())
}

fn has_compiler_rt(ctx: &Ctx<'_>) -> bool {
    ctx.src.join(COMPILER_RT_DIR).join("Makefile").is_file()
}

/// The outcome of linking a program.
enum Linked {
    /// Linked: the program's name, the linked file and the installed (stripped) copy.
    Yes(String, PathBuf, PathBuf),
    /// Every undefined symbol is a compiler builtin (`__multf3`, ...): libcompiler_rt is
    /// missing.
    NeedsCompilerRt(Vec<String>),
    /// The Makefile builds nothing for this `MACHINE` (`NOPROG` and no `PROG`, as
    /// `bsd.prog.mk` reads it): mount_ntfs(8) on arm64.
    NoProg,
}

/// Whether `sym` is a compiler-rt builtin (soft quad float, 128-bit integer, emulated TLS).
fn is_compiler_builtin(sym: &str) -> bool {
    sym.starts_with("__")
        && (["tf3", "tf2", "ti3", "ti2", "di3"]
            .iter()
            .any(|s| sym.ends_with(s))
            || ["__extend", "__trunc", "__fix", "__float", "__emutls"]
                .iter()
                .any(|p| sym.starts_with(p)))
}

/// `$OPENBSD_SRC`, `<root>/reference/openbsd-src`, or the main checkout's when `root` is a
/// git worktree (the clone is gitignored, so worktrees do not have it).
fn openbsd_src(root: &Path) -> Result<PathBuf> {
    let usable = |p: &Path| p.join("lib/libc/Makefile").is_file() && p.join("sys").is_dir();
    if let Some(env) = std::env::var_os("OPENBSD_SRC") {
        let p = PathBuf::from(env);
        let p = if p.is_absolute() { p } else { root.join(p) };
        return if usable(&p) {
            Ok(p)
        } else {
            Err(format!("$OPENBSD_SRC={}: no lib/libc/Makefile or sys/", p.display()).into())
        };
    }
    let local = root.join(crate::REFERENCE_DIR);
    if usable(&local) {
        return Ok(local);
    }
    let common = Command::new("git")
        .arg("-C")
        .arg(root)
        .args(["rev-parse", "--path-format=absolute", "--git-common-dir"])
        .output()
        .ok()
        .filter(|o| o.status.success())
        .map(|o| PathBuf::from(String::from_utf8_lossy(&o.stdout).trim()));
    if let Some(main) = common.as_deref().and_then(Path::parent) {
        let p = main.join(crate::REFERENCE_DIR);
        if usable(&p) {
            return Ok(p);
        }
    }
    Err(format!(
        "{} has no userland sources (lib/, include/); clone it as in reference/README.md or set \
         $OPENBSD_SRC",
        local.display()
    )
    .into())
}

fn tool_version(tool: &Path) -> Result<String> {
    let out = Command::new(tool)
        .arg("--version")
        .output()
        .map_err(|e| format!("{}: {e}", tool.display()))?;
    if !out.status.success() {
        return Err(format!("{} --version failed", tool.display()).into());
    }
    Ok(String::from_utf8_lossy(&out.stdout)
        .lines()
        .next()
        .unwrap_or("")
        .to_string())
}

// --- make glue ------------------------------------------------------------------------------

/// A `Make` for the Makefile in `dir` (relative to the sources), with what `sys.mk` and the
/// environment of a cross build would define.
fn new_make(ctx: &Ctx<'_>, dir: &str, objdir: &Path) -> Result<Make> {
    make_for(ctx, dir, objdir, false)
}

/// A `Make` for a build tool that runs on this machine (`HOSTCC`): same clang, no target,
/// no sysroot.
fn new_host_make(ctx: &Ctx<'_>, dir: &str, objdir: &Path) -> Result<Make> {
    let mut mk = make_for(ctx, dir, objdir, true)?;
    let cflags = mk.var("CFLAGS")?;
    let extra: Vec<&str> = HOST_CFLAGS.iter().map(|(f, _)| *f).collect();
    mk.set("CFLAGS", &format!("{cflags} {}", extra.join(" ")));
    for (f, why) in HOST_CFLAGS {
        println!("  {dir} (host tool): added {f}: {why}");
    }
    Ok(mk)
}

fn make_for(ctx: &Ctx<'_>, dir: &str, objdir: &Path, host: bool) -> Result<Make> {
    make_for_with(ctx, dir, objdir, host, &[])
}

/// `make_for`, with `extra` variables defined before the Makefile is read (what `comp.rs`
/// adds of `sys.mk` and `bsd.own.mk` for the compiler's Makefiles).
fn make_for_with(
    ctx: &Ctx<'_>,
    dir: &str,
    objdir: &Path,
    host: bool,
    extra: &[(&str, String)],
) -> Result<Make> {
    let curdir = if OWN_PROGRAMS.contains(&dir) {
        ctx.root.join(dir)
    } else {
        ctx.src.join(dir)
    };
    let t = ctx.tools;
    let cc = if host {
        t.cc.display().to_string()
    } else {
        format!(
            "{} --target={} --sysroot={}",
            t.cc.display(),
            ctx.m.triple,
            ctx.sysroot.display()
        )
    };
    // The C++ driver beside the C one (`/usr/bin/clang++`), for the `.cpp` sources of the
    // compiler and its libraries (M14, `comp.rs`); nothing of `userland` is C++.
    let cxx_driver = cxx_of(&t.cc);
    let cxx = if host {
        cxx_driver.display().to_string()
    } else {
        format!(
            "{} --target={} --sysroot={}",
            cxx_driver.display(),
            ctx.m.triple,
            ctx.sysroot.display()
        )
    };
    let predefined = [
        ("MACHINE", ctx.m.machine.to_string()),
        ("MACHINE_ARCH", ctx.m.machine_arch.to_string()),
        ("MACHINE_CPU", ctx.m.machine_cpu.to_string()),
        (".OBJDIR", objdir.display().to_string()),
        ("DESTDIR", ctx.sysroot.display().to_string()),
        ("CC", cc),
        // sys.mk: the compiler for build tools run during the build (libcurses's
        // `make_keys` and `make_hash`).
        ("HOSTCC", t.cc.display().to_string()),
        ("LD", t.ld.display().to_string()),
        ("AR", t.ar.display().to_string()),
        ("RANLIB", t.ranlib.display().to_string()),
        // sys.mk
        ("CFLAGS", "-O2 ${PIPE} ${DEBUG}".to_string()),
        ("PIPE", "-pipe".to_string()),
        ("COMPILE.c", "${CC} ${CFLAGS} ${CPPFLAGS} -c".to_string()),
        ("COMPILE.S", "${CC} ${AFLAGS} ${CPPFLAGS} -c".to_string()),
        // sys.mk's C++ defaults.
        ("CXX", cxx),
        ("CXXFLAGS", "-O2 ${PIPE} ${DEBUG}".to_string()),
        (
            "COMPILE.cc",
            "${CXX} ${CXXFLAGS} ${CPPFLAGS} -c".to_string(),
        ),
        ("DFLAGS", "-MD -MP".to_string()),
        ("YACC", ctx.out.join("host/bin/yacc").display().to_string()),
        ("YACC.y", "${YACC} -d ${YFLAGS}".to_string()),
        ("YFLAGS", String::new()),
        ("LEX", ctx.out.join("host/bin/lex").display().to_string()),
        ("LEX.l", "${LEX} ${LFLAGS}".to_string()),
        ("LFLAGS", String::new()),
    ];
    let sys_mk = [
        ("bsd.own.mk", BSD_OWN_MK),
        ("bsd.prog.mk", BSD_PROG_MK),
        ("bsd.lib.mk", BSD_LIB_MK),
        // Only the recursion into `SUBDIR` (the libraries' `man` directories): nothing
        // that changes a variable.
        ("bsd.subdir.mk", ""),
        // Only the `obj` target (making `.OBJDIR`, which xtask does): the compiler's
        // `include/*` Makefiles (M14, `comp.rs`).
        ("bsd.obj.mk", ""),
    ];
    let mut predefined = predefined.to_vec();
    predefined.extend(extra.iter().cloned());
    if !host && NOMAN_PROGRAMS.contains(&dir) {
        predefined.push(("NOMAN", "yes".to_string()));
        println!("  {dir}: NOMAN: the embedded manual page is `no manual` (no mandoc here)");
    }
    let mut mk = Make::new(&curdir, &predefined, &sys_mk);
    mk.read(&curdir.join("Makefile"))?;
    for (flag, why) in UNSUPPORTED_FLAGS {
        let mut removed = false;
        for var in ["CFLAGS", "AFLAGS", "COPTS", "CXXFLAGS"] {
            removed |= mk.remove_word(var, flag);
        }
        // Said once per flag and run: the compiler's ~150 Makefiles (M14) all drop
        // `-fno-ret-protector`.
        let first = removed
            && REPORTED_FLAGS
                .lock()
                .map(|mut r| r.insert(*flag))
                .unwrap_or(true);
        if first {
            println!("  {dir}: dropped {flag} (and wherever else it is set): {why}");
        }
    }
    if !host && mk.words("CFLAGS")?.iter().any(|w| w == "-Werror") {
        let cflags = mk.var("CFLAGS")?;
        let extra: Vec<&str> = WERROR_DEFAULTS.iter().map(|(f, _)| *f).collect();
        mk.set("CFLAGS", &format!("{cflags} {}", extra.join(" ")));
        for (f, why) in WERROR_DEFAULTS {
            println!("  {dir}: -Werror: added {f}: {why}");
        }
    }
    Ok(mk)
}

/// The C++ driver of the C compiler `cc`: `cc++` beside it when it exists (Apple's
/// `/usr/bin/clang++`), else `cc` itself, which compiles `.cpp` files as C++ too.
fn cxx_of(cc: &Path) -> PathBuf {
    let mut name = cc.as_os_str().to_os_string();
    name.push("++");
    let cxx = PathBuf::from(name);
    if cxx.is_file() { cxx } else { cc.to_path_buf() }
}

/// Target-local variables for making `target` from `sources`.
fn locals(target: &str, sources: &[PathBuf]) -> Locals {
    let stem = target
        .rsplit_once('.')
        .map_or(target, |(s, _)| s)
        .to_string();
    let all: Vec<String> = sources.iter().map(|p| p.display().to_string()).collect();
    let first = all.first().cloned().unwrap_or_default();
    let mut l = Locals::new();
    for (k, v) in [
        ("@", target.to_string()),
        (".TARGET", target.to_string()),
        ("*", stem.clone()),
        (".PREFIX", stem),
        ("<", first.clone()),
        (".IMPSRC", first),
        (">", all.join(" ")),
        (".ALLSRC", all.join(" ")),
    ] {
        l.insert(k.to_string(), v);
    }
    l
}

/// One target and the shell commands that make it.
struct Job {
    target: PathBuf,
    cwd: PathBuf,
    /// Expanded commands, with make's `-` (ignore errors) prefix kept as a flag.
    commands: Vec<(String, bool)>,
    /// Inputs known before running (rule sources); the `.d` file adds the rest.
    deps: Vec<PathBuf>,
    /// A directory put first in `$PATH` (where the host-built tools are).
    path: Option<PathBuf>,
}

impl Job {
    /// The job for an explicit rule's commands.
    fn from_rule(
        mk: &Make,
        commands: &[String],
        target: &str,
        sources: Vec<PathBuf>,
        objdir: &Path,
    ) -> Result<Job> {
        let l = locals(target, &sources);
        let mut cmds = Vec::new();
        for c in commands {
            let mut c = c.as_str();
            let mut ignore = false;
            while let Some(rest) = c.strip_prefix(['@', '-', '+']) {
                ignore |= c.starts_with('-');
                c = rest;
            }
            cmds.push((mk.expand_local(c, &l)?, ignore));
        }
        Ok(Job {
            target: objdir.join(target),
            cwd: objdir.to_path_buf(),
            commands: cmds,
            deps: sources,
            path: None,
        })
    }

    fn stamp(&self) -> String {
        let mut s = String::new();
        for (c, _) in &self.commands {
            s.push_str(c);
            s.push('\n');
        }
        s
    }

    fn stamp_path(&self) -> PathBuf {
        let mut p = self.target.clone().into_os_string();
        p.push(".cmd");
        PathBuf::from(p)
    }

    fn depfile(&self) -> Option<PathBuf> {
        match self.target.extension().and_then(|e| e.to_str()) {
            Some("o") => Some(self.target.with_extension("d")),
            // `RULE_C_SO` and friends (M14): `foo.so.d`.
            Some("so") => {
                let mut d = self.target.clone().into_os_string();
                d.push(".d");
                Some(PathBuf::from(d))
            }
            _ => None,
        }
    }

    /// Every input: the rule's sources and what the compiler's `.d` file lists.
    fn all_deps(&self) -> Vec<PathBuf> {
        let mut deps = self.deps.clone();
        if let Some(d) = self.depfile().and_then(|d| fs::read_to_string(d).ok()) {
            deps.extend(
                parse_depfile(&d)
                    .into_iter()
                    .map(|p| if p.is_absolute() { p } else { self.cwd.join(p) }),
            );
        }
        deps
    }

    fn up_to_date(&self) -> bool {
        let Some(t) = mtime(&self.target) else {
            return false;
        };
        if fs::read_to_string(self.stamp_path()).ok().as_deref() != Some(self.stamp().as_str()) {
            return false;
        }
        self.all_deps()
            .iter()
            .all(|d| d.as_os_str() == "-" || mtime(d).is_some_and(|m| m <= t))
    }

    fn run(&self) -> std::result::Result<(), String> {
        for (cmd, ignore) in &self.commands {
            let mut sh = Command::new("/bin/sh");
            sh.arg("-c")
                .arg(format!("{ECHO_SHIM}{cmd}"))
                .current_dir(&self.cwd)
                .stdin(Stdio::null());
            if let Some(dir) = &self.path {
                let old = std::env::var_os("PATH").unwrap_or_default();
                let mut paths = vec![dir.clone()];
                paths.extend(std::env::split_paths(&old));
                sh.env(
                    "PATH",
                    std::env::join_paths(paths).map_err(|e| format!("PATH: {e}"))?,
                );
            }
            let out = sh.output().map_err(|e| format!("/bin/sh: {e}"))?;
            if !out.status.success() && !ignore {
                let _ = fs::remove_file(&self.target);
                return Err(format!(
                    "{}\n{}{}",
                    cmd,
                    String::from_utf8_lossy(&out.stdout),
                    String::from_utf8_lossy(&out.stderr)
                ));
            }
        }
        fs::write(self.stamp_path(), self.stamp()).map_err(|e| e.to_string())
    }
}

/// The prerequisites listed by a compiler `.d` file (first rule only).
fn parse_depfile(text: &str) -> Vec<PathBuf> {
    let joined = text.replace("\\\n", " ");
    let Some(first) = joined.lines().next() else {
        return Vec::new();
    };
    let Some((_, deps)) = first.split_once(": ") else {
        return Vec::new();
    };
    deps.split_whitespace().map(PathBuf::from).collect()
}

fn mtime(p: &Path) -> Option<SystemTime> {
    fs::metadata(p).and_then(|m| m.modified()).ok()
}

/// How many jobs `run_jobs` runs at once; 0 means one per CPU (the default of `userland`).
/// `comp` (M14, `comp.rs`) sets it from `--jobs`: two other milestone agents may share the
/// machine.
/// macOS's `/bin/sh` has no `echo -n` (it prints the `-n`); OpenBSD's ksh does, and
/// Makefiles use it (`distrib/special/more`'s `morehelp.h`). The rest of `echo` is left alone.
const ECHO_SHIM: &str = "echo() { if [ \"$1\" = -n ]; then shift; printf '%s' \"$*\"; \
                         else command echo \"$@\"; fi; }\n";

static JOBS: AtomicUsize = AtomicUsize::new(0);

/// Runs the jobs that are out of date, in parallel; returns how many ran. Every failure is
/// reported, not just the first.
fn run_jobs(ctx: &Ctx<'_>, what: &str, jobs: &[Job]) -> Result<usize> {
    let workers = match JOBS.load(Ordering::Relaxed) {
        0 => std::thread::available_parallelism().map_or(4, |n| n.get()),
        n => n,
    };
    // Which jobs are out of date, checked in parallel: the compiler's ~3,000 objects
    // (M14, `comp.rs`) each have a `.d` file of hundreds of headers to stat.
    let mut stale = vec![false; jobs.len()];
    let chunk = jobs.len().div_ceil(workers).max(1);
    std::thread::scope(|s| {
        for (js, out) in jobs.chunks(chunk).zip(stale.chunks_mut(chunk)) {
            s.spawn(move || {
                for (j, o) in js.iter().zip(out.iter_mut()) {
                    *o = !j.up_to_date();
                }
            });
        }
    });
    let todo: Vec<&Job> = jobs
        .iter()
        .zip(&stale)
        .filter_map(|(j, s)| s.then_some(j))
        .collect();
    let next = AtomicUsize::new(0);
    let failures: Mutex<Vec<(PathBuf, String)>> = Mutex::new(Vec::new());
    std::thread::scope(|s| {
        for _ in 0..workers {
            s.spawn(|| {
                loop {
                    let i = next.fetch_add(1, Ordering::Relaxed);
                    let Some(job) = todo.get(i) else {
                        break;
                    };
                    if let Err(e) = job.run()
                        && let Ok(mut f) = failures.lock()
                    {
                        f.push((job.target.clone(), e));
                    }
                }
            });
        }
    });
    let deps: Vec<Vec<PathBuf>> = std::thread::scope(|s| {
        let handles: Vec<_> = jobs
            .chunks(chunk)
            .map(|js| s.spawn(move || js.iter().flat_map(Job::all_deps).collect::<Vec<_>>()))
            .collect();
        handles.into_iter().filter_map(|h| h.join().ok()).collect()
    });
    if let Ok(mut inputs) = ctx.inputs.lock() {
        for d in deps {
            inputs.extend(d);
        }
    }
    let failures = failures
        .into_inner()
        .map_err(|_| "a build thread panicked")?;
    if failures.is_empty() {
        return Ok(todo.len());
    }
    for (target, err) in &failures {
        eprintln!("--- failed: {}", target.display());
        for line in err.lines().take(40) {
            eprintln!("    {line}");
        }
    }
    Err(format!("{what}: {} of {} job(s) failed", failures.len(), jobs.len()).into())
}

// --- 1. /usr/include ------------------------------------------------------------------------

/// Copies `from` to `to` unless `to` already has the same contents (`cmp -s || install`), so
/// unchanged headers keep their mtime and nothing recompiles. Returns whether it wrote.
fn install_file(ctx: &Ctx<'_>, from: &Path, to: &Path) -> Result<bool> {
    let data = fs::read(from).map_err(|e| format!("{}: {e}", from.display()))?;
    if let Ok(mut l) = ctx.lower.lock() {
        let key = to.to_string_lossy().to_lowercase();
        if let Some(other) = l.get(&key).filter(|o| o.as_path() != to) {
            return Err(format!(
                "{} and {} differ only in case; this file system cannot hold both",
                other.display(),
                to.display()
            )
            .into());
        }
        l.insert(key, to.to_path_buf());
    }
    if let Ok(mut m) = ctx.installed.lock() {
        m.insert(to.to_path_buf(), from.to_path_buf());
    }
    if fs::read(to).ok().as_deref() == Some(data.as_slice()) {
        return Ok(false);
    }
    if let Some(parent) = to.parent() {
        fs::create_dir_all(parent).map_err(|e| format!("{}: {e}", parent.display()))?;
    }
    let _ = fs::remove_file(to);
    fs::write(to, &data).map_err(|e| format!("{}: {e}", to.display()))?;
    Ok(true)
}

/// `ln -sf target link`, unless it already points there.
fn symlink(target: &str, link: &Path) -> Result<()> {
    if fs::read_link(link).ok().as_deref() == Some(Path::new(target)) {
        return Ok(());
    }
    if fs::symlink_metadata(link).is_ok_and(|m| m.is_dir()) {
        fs::remove_dir_all(link).map_err(|e| format!("{}: {e}", link.display()))?;
    } else {
        let _ = fs::remove_file(link);
    }
    std::os::unix::fs::symlink(target, link)
        .map_err(|e| format!("ln -s {target} {}: {e}", link.display()).into())
}

/// Files of `dir` (not recursive) whose name matches `pattern`, sorted.
fn glob_dir(dir: &Path, pattern: &str) -> Result<Vec<PathBuf>> {
    let mut v = Vec::new();
    for e in fs::read_dir(dir).map_err(|e| format!("{}: {e}", dir.display()))? {
        let p = e?.path();
        let name = p.file_name().and_then(|n| n.to_str()).unwrap_or("");
        if p.is_file() && crate::bsdmake::glob(pattern.as_bytes(), name.as_bytes()) {
            v.push(p);
        }
    }
    v.sort();
    Ok(v)
}

/// `find dir -follow -type f -name '*.h'`, as paths relative to `base`.
fn find_headers(base: &Path, dir: &Path, out: &mut Vec<PathBuf>) -> Result<()> {
    for e in fs::read_dir(dir).map_err(|e| format!("{}: {e}", dir.display()))? {
        let p = e?.path();
        // `metadata` follows symbolic links, like `find -follow`.
        let Ok(meta) = fs::metadata(&p) else {
            continue;
        };
        if meta.is_dir() {
            find_headers(base, &p, out)?;
        } else if meta.is_file() && p.extension().and_then(|x| x.to_str()) == Some("h") {
            out.push(p.strip_prefix(base).unwrap_or(&p).to_path_buf());
        }
    }
    Ok(())
}

fn install_includes(ctx: &Ctx<'_>) -> Result<()> {
    let objdir = ctx.out.join("obj/include");
    let mk = new_make(ctx, "include", &objdir)?;
    let inc = ctx.sysroot.join("usr/include");
    let srcinc = ctx.src.join("include");
    let mut total = 0usize;
    let mut written = 0usize;
    let mut count = |w: bool| {
        total += 1;
        written += usize::from(w);
    };

    // includes: FILES, DIRS, LFILES, MFILES.
    for f in mk.words("FILES")? {
        count(install_file(ctx, &srcinc.join(&f), &inc.join(&f))?);
    }
    for d in mk.words("DIRS")? {
        for p in glob_dir(&srcinc.join(&d), "*.[ih]")? {
            let name = p.file_name().ok_or("bad file name")?;
            count(install_file(ctx, &p, &inc.join(&d).join(name))?);
        }
    }
    for f in mk.words("LFILES")? {
        symlink(&format!("sys/{f}"), &inc.join(&f))?;
    }
    for f in mk.words("MFILES")? {
        symlink(&format!("machine/{f}"), &inc.join(&f))?;
    }

    // copies (SYS_INCLUDE?= copies): LDIRS from sys/, then <machine/*>.
    if mk.var("SYS_INCLUDE")? != "copies" {
        return Err("include/Makefile: SYS_INCLUDE is not `copies`".into());
    }
    let sys = ctx.src.join("sys");
    let mut headers = Vec::new();
    for d in mk.words("LDIRS")? {
        find_headers(&sys, &sys.join(&d), &mut headers)?;
    }
    headers.retain(|h| !(h.starts_with("dev/microcode") || h.starts_with("dev/pci/drm")));
    headers.sort();
    for h in &headers {
        count(install_file(ctx, &sys.join(h), &inc.join(h))?);
    }
    let (machine, cpu) = (ctx.m.machine, ctx.m.machine_cpu);
    for p in glob_dir(&sys.join("arch").join(machine).join("include"), "*.h")? {
        let name = p.file_name().ok_or("bad file name")?;
        count(install_file(ctx, &p, &inc.join(machine).join(name))?);
    }
    let cpu_inc = sys.join("arch").join(cpu).join("include");
    if machine != cpu && cpu_inc.is_dir() {
        for p in glob_dir(&cpu_inc, "*.h")? {
            let name = p.file_name().ok_or("bad file name")?;
            count(install_file(ctx, &p, &inc.join(cpu).join(name))?);
        }
    }
    symlink(machine, &inc.join("machine"))?;

    // RDIRS (PRDIRS included): their own `includes` targets. Only libutil's, librpcsvc's and
    // those of LIBRARIES are needed by what is built here.
    let mut skipped = Vec::new();
    for r in mk.words("RDIRS")? {
        if r == "../lib/librpcsvc" {
            for (from, to) in rpcsvc_headers(ctx)? {
                count(install_file(ctx, &from, &inc.join("rpcsvc").join(to))?);
            }
            continue;
        }
        if let Some(lib) = LIBRARIES
            .iter()
            .find(|l| r.strip_prefix("../") == Some(**l))
        {
            for w in libraries::library_includes(ctx, lib)? {
                count(w);
            }
            continue;
        }
        if r != "../lib/libutil" {
            skipped.push(r);
            continue;
        }
        let dir = Path::new("include").join(&r);
        let dir = dir.to_string_lossy();
        let sub = new_make(ctx, &dir, &objdir)?;
        let rule = sub
            .rule_for("includes")
            .ok_or_else(|| format!("{r}/Makefile: no `includes` target"))?;
        if !rule.commands.iter().any(|c| c.contains("$(HDRS)")) {
            return Err(format!("{r}/Makefile: `includes` does not install $(HDRS)").into());
        }
        for h in sub.words("HDRS")? {
            count(install_file(
                ctx,
                &ctx.src.join(&*dir).join(&h),
                &inc.join(&h),
            )?);
        }
    }
    println!(
        "  /usr/include: {total} headers ({written} updated); RDIRS not installed (nothing \
         built here needs them): {}",
        skipped.join(" ")
    );
    Ok(())
}

/// `lib/librpcsvc`'s `includes`: its `.x` files and the headers its `.x.h` rule makes with
/// `rpcgen`, which is OpenBSD's own, built for this machine first. Returns (file, name).
fn rpcsvc_headers(ctx: &Ctx<'_>) -> Result<Vec<(PathBuf, String)>> {
    let bindir = build_host_prog(ctx, "usr.bin/rpcgen")?;
    let dir = "lib/librpcsvc";
    let objdir = ctx.out.join("obj").join(dir);
    fs::create_dir_all(&objdir).map_err(|e| format!("{}: {e}", objdir.display()))?;
    let mk = new_make(ctx, dir, &objdir)?;
    let rule = mk
        .rule_for(".x.h")
        .ok_or("lib/librpcsvc/Makefile: no .x.h rule")?;
    let mut jobs = Vec::new();
    let mut out = Vec::new();
    for x in mk.words("RPCSRCS")? {
        let src = mk
            .search(&x)
            .ok_or_else(|| format!("lib/librpcsvc: no {x}"))?;
        let h = format!("{}.h", x.trim_end_matches(".x"));
        let mut job = Job::from_rule(&mk, &rule.commands, &h, vec![src.clone()], &objdir)?;
        job.path = Some(bindir.clone());
        out.push((src, x));
        out.push((job.target.clone(), h));
        jobs.push(job);
    }
    let hdrs = mk.words("HDRS")?;
    if hdrs.len() != jobs.len() {
        return Err("lib/librpcsvc: HDRS and RPCSRCS disagree".into());
    }
    run_jobs(ctx, dir, &jobs)?;
    Ok(out)
}

/// Builds a program for this machine (a build tool); returns the directory holding it.
fn build_host_prog(ctx: &Ctx<'_>, dir: &str) -> Result<PathBuf> {
    build_host_prog_with(ctx, dir, |_, _| Ok(()))
}

/// Builds OpenBSD's yacc(1) for this machine (`$out/host/bin/yacc`, what `YACC` names), with
/// the `reallocarray(3)` shim of `YACC_COMPAT_H` force-included.
fn build_yacc(ctx: &Ctx<'_>) -> Result<PathBuf> {
    let mut built = ctx.yacc.lock().map_err(|_| "lock poisoned")?;
    if let Some(bindir) = built.as_ref() {
        return Ok(bindir.clone());
    }
    let bindir = build_host_prog_with(ctx, YACC_DIR, |mk, objdir| {
        let inc = objdir.join("emibsd-compat.h");
        ramdisk::write_if_changed(&inc, YACC_COMPAT_H)?;
        let cppflags = mk.var("CPPFLAGS")?;
        mk.set(
            "CPPFLAGS",
            &format!("{cppflags} -include {}", inc.display()),
        );
        println!(
            "  {YACC_DIR} (host tool): OpenBSD's yacc, with a reallocarray(3) shim (macOS has none)"
        );
        Ok(())
    })?;
    *built = Some(bindir.clone());
    Ok(bindir)
}

/// Builds OpenBSD's lex(1) for this machine (`$out/host/bin/lex`, what `LEX` names); its own
/// `scan.l` is made by `BOOTSTRAP_LEX` and its `parse.y` by OpenBSD's yacc.
fn build_lex(ctx: &Ctx<'_>) -> Result<PathBuf> {
    let mut built = ctx.lex.lock().map_err(|_| "lock poisoned")?;
    if let Some(bindir) = built.as_ref() {
        return Ok(bindir.clone());
    }
    let bindir = build_host_prog_with(ctx, LEX_DIR, |mk, _| {
        mk.set("LEX", BOOTSTRAP_LEX);
        println!(
            "  {LEX_DIR} (host tool): OpenBSD's lex; its own scan.l made by {BOOTSTRAP_LEX} \
             (no initscan.c in the tree)"
        );
        Ok(())
    })?;
    *built = Some(bindir.clone());
    Ok(bindir)
}

/// `build_host_prog`, with `adapt` changing the evaluated Makefile (given the object
/// directory) before anything is compiled: the host portability shims of `ramdisk.rs`.
fn build_host_prog_with(
    ctx: &Ctx<'_>,
    dir: &str,
    adapt: impl FnOnce(&mut Make, &Path) -> Result<()>,
) -> Result<PathBuf> {
    let objdir = ctx.out.join("host/obj").join(dir);
    let bindir = ctx.out.join("host/bin");
    for d in [&objdir, &bindir] {
        fs::create_dir_all(d).map_err(|e| format!("{}: {e}", d.display()))?;
    }
    let mut mk = new_host_make(ctx, dir, &objdir)?;
    adapt(&mut mk, &objdir)?;
    let prog = mk.var("PROG")?;
    let jobs = object_jobs(ctx, &mk, &objdir, &[])?;
    run_jobs(ctx, dir, &jobs)?;
    let objs: Vec<PathBuf> = jobs.iter().map(|j| j.target.clone()).collect();
    let target = bindir.join(&prog);
    let link = Job::from_rule(
        &mk,
        &["${CC} ${LDFLAGS} -o ${.TARGET} ${.ALLSRC}".to_string()],
        &target.display().to_string(),
        objs,
        &objdir,
    )?;
    run_jobs(ctx, dir, &[link])?;
    Ok(bindir)
}

// --- 2. csu ---------------------------------------------------------------------------------

fn build_csu(ctx: &Ctx<'_>) -> Result<()> {
    let objdir = ctx.out.join("obj/lib/csu");
    fs::create_dir_all(&objdir).map_err(|e| format!("{}: {e}", objdir.display()))?;
    let mk = new_make(ctx, "lib/csu", &objdir)?;
    let mut jobs = Vec::new();
    for o in mk.words("OBJS")? {
        let rule = mk
            .rule_for(&o)
            .ok_or_else(|| format!("lib/csu/Makefile: no rule for {o}"))?;
        let sources = resolve_sources(&mk, &objdir, &rule.sources)?;
        jobs.push(Job::from_rule(&mk, &rule.commands, &o, sources, &objdir)?);
    }
    let ran = run_jobs(ctx, "lib/csu", &jobs)?;
    let lib = ctx.sysroot.join("usr/lib");
    for j in &jobs {
        let name = j.target.file_name().ok_or("bad object name")?;
        install_file(ctx, &j.target, &lib.join(name))?;
    }
    println!("  lib/csu: {} objects ({ran} rebuilt)", jobs.len());
    Ok(())
}

/// Rule sources: absolute paths stay, others are looked up like make does; a source no
/// directory has may be one an earlier rule made in `objdir` (libcurses's `make_hash`).
fn resolve_sources(mk: &Make, objdir: &Path, sources: &[String]) -> Result<Vec<PathBuf>> {
    sources
        .iter()
        .map(|s| {
            mk.search(s)
                .or_else(|| Some(objdir.join(s)).filter(|p| p.exists()))
                .ok_or_else(|| format!("cannot find source {s}").into())
        })
        .collect()
}

// --- 3. libraries ---------------------------------------------------------------------------

/// The object files `mk` builds, as jobs: explicit rules (the system-call stubs) or the
/// implicit `.c.o`/`.S.o` rules on the source found through `.PATH` or generated by a rule.
fn object_jobs(ctx: &Ctx<'_>, mk: &Make, objdir: &Path, extra_objs: &[String]) -> Result<Vec<Job>> {
    object_jobs_as(ctx, mk, objdir, extra_objs, ObjKind::Static)
}

/// Which of `bsd.lib.mk`'s object kinds `object_jobs_as` makes.
#[derive(Clone, Copy, PartialEq, Eq)]
enum ObjKind {
    /// `${OBJS}`: `.o` files, by the `.c.o`/`.S.o` rules.
    Static,
    /// `${SOBJS}` (M14): `.so` files, by the `.c.so`/`.S.so` rules (`${PICFLAG} -DPIC`,
    /// `-DSOLIB`), for a shared library (`userland/shlib.rs`).
    Pic,
}

/// `object_jobs` for objects of `kind`: `extra_objs` are `.o` names (`${OBJS}`), turned
/// into `.so` names for `ObjKind::Pic` as `${OBJS:.o=.so}` does.
fn object_jobs_as(
    ctx: &Ctx<'_>,
    mk: &Make,
    objdir: &Path,
    extra_objs: &[String],
    kind: ObjKind,
) -> Result<Vec<Job>> {
    let suffix = match kind {
        ObjKind::Static => "o",
        ObjKind::Pic => "so",
    };
    let srcs = mk.words("SRCS")?;
    let mut objs: Vec<String> = extra_objs
        .iter()
        .map(|o| match o.strip_suffix(".o") {
            Some(stem) => format!("{stem}.{suffix}"),
            None => o.clone(),
        })
        .collect();
    for s in srcs.iter().filter(|s| !s.ends_with(".h")) {
        let stem = s.rsplit_once('.').map_or(s.as_str(), |(a, _)| a);
        objs.push(format!("{stem}.{suffix}"));
    }
    let mut seen = BTreeSet::new();
    objs.retain(|o| seen.insert(o.clone()));

    let mut generated = Vec::new();
    let mut jobs = Vec::new();
    let mut lower: HashMap<String, usize> = HashMap::new();
    for o in &objs {
        // macOS file systems ignore case by default: libc's `_exit.o` (a system-call stub)
        // and `_Exit.o` (stdlib/_Exit.c) would be one file. The second of such a pair is
        // built in a subdirectory of its own; the archive keeps both member names.
        let n = lower.entry(o.to_lowercase()).or_insert(0);
        let odir = if *n == 0 {
            objdir.to_path_buf()
        } else {
            objdir.join(format!("case{n}"))
        };
        *n += 1;
        fs::create_dir_all(&odir).map_err(|e| format!("{}: {e}", odir.display()))?;
        if let Some(rule) = mk.rule_for(o) {
            let sources = resolve_sources(mk, objdir, &rule.sources)?;
            jobs.push(Job::from_rule(mk, &rule.commands, o, sources, &odir)?);
            continue;
        }
        let stem = &o[..o.len() - suffix.len() - 1];
        let named: Vec<String> = srcs
            .iter()
            .filter(|s| s.rsplit_once('.').is_some_and(|(a, _)| a == stem))
            .cloned()
            .collect();
        let candidates = if named.is_empty() {
            ["c", "S", "s", "y", "l"]
                .iter()
                .map(|x| format!("{stem}.{x}"))
                .collect()
        } else {
            named
        };
        let mut found = None;
        for c in &candidates {
            if c.ends_with(".y")
                && let Some(p) = mk.search(c)
            {
                // `.y.c`: made by OpenBSD's own yacc, built for this machine.
                build_yacc(ctx)?;
                let c_name = format!("{stem}.c");
                let rule: Vec<String> = RULE_Y_C.iter().map(|c| c.to_string()).collect();
                generated.push(Job::from_rule(mk, &rule, &c_name, vec![p], objdir)?);
                found = Some((c_name.clone(), objdir.join(c_name)));
                break;
            }
            if c.ends_with(".l")
                && let Some(p) = mk.search(c)
            {
                // `.l.c`: made by OpenBSD's own lex, built for this machine (except for
                // lex's own scanner, `LEX_DIR`).
                if mk.var("LEX")? != BOOTSTRAP_LEX {
                    build_lex(ctx)?;
                }
                let c_name = format!("{stem}.c");
                let rule: Vec<String> = RULE_L_C.iter().map(|c| c.to_string()).collect();
                generated.push(Job::from_rule(mk, &rule, &c_name, vec![p], objdir)?);
                found = Some((c_name.clone(), objdir.join(c_name)));
                break;
            }
            if let Some(rule) = mk.rule_for(c) {
                let sources = resolve_sources(mk, objdir, &rule.sources)?;
                generated.push(Job::from_rule(mk, &rule.commands, c, sources, objdir)?);
                found = Some((c.clone(), objdir.join(c)));
                break;
            }
            if c.ends_with(".c")
                && let Some(rule) = mk.rule_for(".x.c")
                && let Some(p) = mk.search(&format!("{stem}.x"))
            {
                // `.x.c` (librpcsvc, M10b): made by OpenBSD's own rpcgen, built for this
                // machine; the `.x.h` headers it includes are already in `objdir`
                // (`rpcsvc_headers`).
                let bindir = build_host_prog(ctx, "usr.bin/rpcgen")?;
                let mut job = Job::from_rule(mk, &rule.commands, c, vec![p], objdir)?;
                job.path = Some(bindir);
                generated.push(job);
                found = Some((c.clone(), objdir.join(c)));
                break;
            }
            if let Some(p) = mk.search(c) {
                found = Some((c.clone(), p));
                break;
            }
        }
        let (name, path) =
            found.ok_or_else(|| format!("no source for {o} (tried {candidates:?})"))?;
        let cc = [".cpp", ".cc", ".cxx"].iter().any(|x| name.ends_with(x));
        let template = match (kind, name.ends_with(".c"), cc) {
            (ObjKind::Static, true, _) => RULE_C_O,
            (ObjKind::Static, false, true) => RULE_CC_O,
            (ObjKind::Static, false, false) => RULE_S_O,
            (ObjKind::Pic, true, _) => RULE_C_SO,
            (ObjKind::Pic, false, true) => RULE_CC_SO,
            (ObjKind::Pic, false, false) => RULE_S_SO,
        };
        jobs.push(Job::from_rule(
            mk,
            &[template.to_string()],
            o,
            vec![path],
            &odir,
        )?);
    }
    if !generated.is_empty() {
        run_jobs(ctx, "generated sources", &generated)?;
    }
    Ok(jobs)
}

fn build_lib(ctx: &Ctx<'_>, dir: &str) -> Result<()> {
    let objdir = ctx.out.join("obj").join(dir);
    fs::create_dir_all(&objdir).map_err(|e| format!("{}: {e}", objdir.display()))?;
    let mk = new_make(ctx, dir, &objdir)?;
    let lib = mk.var("LIB")?;
    // `bsd.lib.mk` makes `BUILDFIRST` (generated headers and sources) before any object.
    let mut made = BTreeSet::new();
    for t in mk.words("BUILDFIRST")? {
        libraries::make_target(ctx, &mk, &objdir, &t, None, &mut made)?;
    }
    let jobs = object_jobs(ctx, &mk, &objdir, &mk.words("OBJS")?)?;
    let ran = run_jobs(ctx, dir, &jobs)?;

    let archive = objdir.join(format!("lib{lib}.a"));
    let _ = fs::remove_file(&archive);
    let mut ar = Command::new(&ctx.tools.ar);
    ar.arg("cq").arg(&archive).current_dir(&objdir);
    for j in &jobs {
        ar.arg(j.target.strip_prefix(&objdir).unwrap_or(&j.target));
    }
    run(&mut ar)?;
    run(Command::new(&ctx.tools.ranlib).arg(&archive))?;
    let installed = ctx.sysroot.join("usr/lib").join(format!("lib{lib}.a"));
    install_file(ctx, &archive, &installed)?;
    let size = fs::metadata(&archive).map(|m| m.len()).unwrap_or(0);
    println!(
        "  {dir}: lib{lib}.a, {} objects ({ran} rebuilt), {size} bytes",
        jobs.len()
    );
    Ok(())
}

fn run(cmd: &mut Command) -> Result<()> {
    let out = cmd.output().map_err(|e| format!("{cmd:?}: {e}"))?;
    if out.status.success() {
        Ok(())
    } else {
        Err(format!(
            "{cmd:?} failed:\n{}{}",
            String::from_utf8_lossy(&out.stdout),
            String::from_utf8_lossy(&out.stderr)
        )
        .into())
    }
}

// --- 4. programs ----------------------------------------------------------------------------

/// Builds the program in `dir`; returns its name, the linked file and the installed copy.
fn build_prog(ctx: &Ctx<'_>, dir: &str) -> Result<Linked> {
    let objdir = ctx.out.join("obj").join(dir);
    fs::create_dir_all(&objdir).map_err(|e| format!("{}: {e}", objdir.display()))?;
    let mut mk = new_make(ctx, dir, &objdir)?;
    let prog = mk.var("PROG")?;
    // `bsd.prog.mk` builds no program when the Makefile defines `NOPROG` (even empty) and no
    // `PROG`: it has nothing for this `MACHINE`.
    if prog.is_empty() && mk.defined("NOPROG") {
        return Ok(Linked::NoProg);
    }
    if prog.is_empty() {
        return Err(format!("{dir}/Makefile: no PROG").into());
    }
    if !mk.defined("SRCS") {
        mk.set("SRCS", &format!("{prog}.c"));
    }
    let mut ldadd = mk.words("LDADD")?;
    if let Some(v) = VARIANTS.iter().find(|v| v.dir == dir) {
        let cflags = mk.var("CFLAGS")?;
        mk.set("CFLAGS", &format!("{cflags} {}", v.add_cflags));
        ldadd.retain(|w| !v.drop_ldadd.contains(&w.as_str()));
        if v.static_link {
            mk.set("LDSTATIC", "${STATIC}");
        }
        println!("  {dir}: {}", v.why);
    }
    if !mk.words("LDSTATIC")?.iter().any(|w| w == "-static") {
        return Err(format!(
            "{dir}: not linked -static (LDSTATIC); the ramdisk's programs are all static (add a \
             VARIANTS entry)"
        )
        .into());
    }
    if let Some(w) = ldadd.iter().find(|w| !w.starts_with("-l")) {
        return Err(format!("{dir}: unsupported LDADD word `{w}`").into());
    }

    // Headers a Makefile makes by a rule and names as a dependency of an object (`more.o:
    // morehelp.h` in `distrib/special/more`): made before the objects, as make would.
    let mut made = BTreeSet::new();
    for src in mk.words("SRCS")? {
        let Some(stem) = src.strip_suffix(".c") else {
            continue;
        };
        for dep in mk.sources_of(&format!("{stem}.o")) {
            if dep.ends_with(".h") && mk.rule_for(&dep).is_some() {
                libraries::make_target(ctx, &mk, &objdir, &dep, None, &mut made)?;
            }
        }
    }
    let jobs = object_jobs(ctx, &mk, &objdir, &[])?;
    let ran = run_jobs(ctx, dir, &jobs)?;

    // What OpenBSD's clang driver passes for `cc -static` (its lld defaults to PIE; LLD 17
    // needs `-pie` said); `-lcompiler_rt` only when the clone has it (`COMPILER_RT_DIR`).
    let lib = ctx.sysroot.join("usr/lib");
    let exe = objdir.join(&prog);
    let mut ld = Command::new(&ctx.tools.ld);
    ld.arg(format!("--sysroot={}", ctx.sysroot.display()))
        .args(["-e", "__start", "--eh-frame-hdr", "-Bstatic", "-pie", "-o"])
        .arg(&exe)
        .arg(lib.join("rcrt0.o"))
        .arg(lib.join("crtbegin.o"))
        .arg(format!("-L{}", lib.display()));
    for j in &jobs {
        ld.arg(&j.target);
    }
    ld.args(&ldadd);
    if has_compiler_rt(ctx) {
        ld.args(["-lcompiler_rt", "-lc", "-lcompiler_rt"]);
    } else {
        ld.arg("-lc");
    }
    ld.arg(lib.join("crtend.o"));
    let out = ld.output().map_err(|e| format!("ld.lld: {e}"))?;
    if !out.status.success() {
        let stderr = String::from_utf8_lossy(&out.stderr);
        let undefined: BTreeSet<String> = stderr
            .lines()
            .filter_map(|l| l.strip_prefix("ld.lld: error: undefined symbol: "))
            .map(|s| s.trim().to_string())
            .collect();
        if !undefined.is_empty() && undefined.iter().all(|s| is_compiler_builtin(s)) {
            return Ok(Linked::NeedsCompilerRt(undefined.into_iter().collect()));
        }
        return Err(format!("{dir}: link failed:\n{ld:?}\n{stderr}").into());
    }

    let bindir = mk.var("BINDIR")?;
    let rootdir = ctx.out.join("root");
    let installed = rootdir.join(bindir.trim_start_matches('/')).join(&prog);
    if let Some(parent) = installed.parent() {
        fs::create_dir_all(parent).map_err(|e| format!("{}: {e}", parent.display()))?;
    }
    // `install -s`.
    run(Command::new(&ctx.tools.objcopy)
        .arg("--strip-all")
        .arg(&exe)
        .arg(&installed))?;
    // `afterinstall`: shell scripts a Makefile installs beside the program (the scripts of
    // `usr.bin/compress`, which its LINKS name). Only `AFTERINSTALL_SCRIPTS`' files; the
    // rule itself is not evaluated.
    for (sdir, files) in AFTERINSTALL_SCRIPTS {
        if *sdir != dir {
            continue;
        }
        for f in *files {
            let from = ctx.src.join(dir).join(f);
            let to = rootdir.join(bindir.trim_start_matches('/')).join(f);
            let _ = fs::remove_file(&to);
            fs::copy(&from, &to).map_err(|e| format!("{dir}: install {f}: {e}"))?;
        }
    }
    // LINKS: pairs of (existing, new) absolute paths, hard links like install(1) makes.
    let links = mk.words("LINKS")?;
    for pair in links.chunks(2) {
        if let [from, to] = pair {
            let to = rootdir.join(to.trim_start_matches('/'));
            let _ = fs::remove_file(&to);
            fs::hard_link(rootdir.join(from.trim_start_matches('/')), &to)
                .map_err(|e| format!("ln {from} {}: {e}", to.display()))?;
        }
    }
    // `install -o ${BINOWN} -g ${BINGRP} -m ${BINMODE}`, when the Makefile asks for it
    // (`login_passwd`: root:auth 4555). The defaults are those of `bsd.own.mk`.
    let attrs = ["BINOWN", "BINGRP", "BINMODE"].map(|v| mk.var(v));
    if let [Ok(owner), Ok(group), Ok(mode)] = &attrs
        && [owner, group, mode].iter().any(|v| !v.is_empty())
    {
        let attr = ramdisk::Attr::installed(
            &format!("/{}/{prog}", bindir.trim_matches('/')),
            if owner.is_empty() { "root" } else { owner },
            if group.is_empty() { "bin" } else { group },
            if mode.is_empty() { "555" } else { mode },
        )?;
        ctx.owners
            .lock()
            .map_err(|_| "lock poisoned")?
            .push(attr.clone());
        for pair in links.chunks(2) {
            if let [_, to] = pair {
                ctx.owners
                    .lock()
                    .map_err(|_| "lock poisoned")?
                    .push(ramdisk::Attr {
                        path: to.clone(),
                        ..attr.clone()
                    });
            }
        }
    }
    println!("  {dir}: {} objects ({ran} rebuilt), linked", jobs.len());
    Ok(Linked::Yes(prog, exe, installed))
}

// --- verification ---------------------------------------------------------------------------

fn phdr_name(t: u32) -> String {
    match t {
        1 => "LOAD".into(),
        2 => "DYNAMIC".into(),
        3 => "INTERP".into(),
        4 => "NOTE".into(),
        6 => "PHDR".into(),
        7 => "TLS".into(),
        0x6474_e550 => "GNU_EH_FRAME".into(),
        0x6474_e551 => "GNU_STACK".into(),
        0x6474_e552 => "GNU_RELRO".into(),
        0x6474_e553 => "GNU_PROPERTY".into(),
        0x65a3_dbe5 => "OPENBSD_MUTABLE".into(),
        0x65a3_dbe6 => "OPENBSD_RANDOMIZE".into(),
        0x65a3_dbe7 => "OPENBSD_WXNEEDED".into(),
        0x65a3_dbe8 => "OPENBSD_NOBTCFI".into(),
        0x65a3_dbe9 => "OPENBSD_SYSCALLS".into(),
        0x65a4_1be6 => "OPENBSD_BOOTDATA".into(),
        other => format!("{other:#x}"),
    }
}

/// Checks the ELF header and program headers directly, and with `llvm-objdump -p -h`.
fn verify(ctx: &Ctx<'_>, name: &str, exe: &Path, installed: &Path) -> Result<()> {
    let data = fs::read(exe).map_err(|e| format!("{}: {e}", exe.display()))?;
    let u16_at = |o: usize| data.get(o..o + 2).map(|b| u16::from_le_bytes([b[0], b[1]]));
    let u32_at = |o: usize| {
        data.get(o..o + 4)
            .map(|b| u32::from_le_bytes([b[0], b[1], b[2], b[3]]))
    };
    let u64_at = |o: usize| {
        data.get(o..o + 8).map(|b| {
            let mut a = [0u8; 8];
            a.copy_from_slice(b);
            u64::from_le_bytes(a)
        })
    };
    if data.get(..4) != Some(b"\x7fELF".as_slice()) || data.get(4) != Some(&2) {
        return Err(format!("{name}: not an ELF64 file").into());
    }
    let (Some(e_type), Some(e_machine), Some(phoff), Some(phentsize), Some(phnum)) =
        (u16_at(16), u16_at(18), u64_at(32), u16_at(54), u16_at(56))
    else {
        return Err(format!("{name}: truncated ELF header").into());
    };
    if e_machine != ctx.m.e_machine {
        return Err(format!(
            "{name}: e_machine {e_machine}, expected {}",
            ctx.m.e_machine
        )
        .into());
    }
    let mut types = Vec::new();
    for i in 0..usize::from(phnum) {
        let off = usize::try_from(phoff)? + i * usize::from(phentsize);
        types.push(phdr_name(
            u32_at(off).ok_or_else(|| format!("{name}: truncated program headers"))?,
        ));
    }
    if types.iter().any(|t| t == "INTERP") {
        return Err(format!("{name}: has PT_INTERP, not statically linked").into());
    }
    let objdump = Command::new(&ctx.tools.objdump)
        .args(["-p", "-h"])
        .arg(exe)
        .output()
        .map_err(|e| format!("llvm-objdump: {e}"))?;
    let text = String::from_utf8_lossy(&objdump.stdout);
    if !objdump.status.success() || text.lines().any(|l| l.trim_start().starts_with("INTERP ")) {
        return Err(format!("{name}: llvm-objdump -p failed or shows INTERP").into());
    }
    if !text.contains(".note.openbsd.ident") {
        return Err(format!("{name}: no .note.openbsd.ident section").into());
    }
    let report = exe.with_extension("objdump.txt");
    fs::write(&report, text.as_bytes()).map_err(|e| format!("{}: {e}", report.display()))?;
    let size = data.len();
    let stripped = fs::metadata(installed).map(|m| m.len()).unwrap_or(0);
    let kind = if e_type == 3 {
        "ET_DYN (PIE)"
    } else {
        "ET_EXEC"
    };
    println!(
        "    {name}: ELF64 {} {kind}, no PT_INTERP, {size} bytes ({stripped} stripped); \
         phdrs: {}",
        ctx.m.machine_arch,
        types.join(" ")
    );
    Ok(())
}

// --- licences -------------------------------------------------------------------------------

/// The licence families named in a file's text (first 300 lines).
fn licence_families(text: &str) -> Vec<&'static str> {
    let head: String = text.lines().take(300).collect::<Vec<_>>().join(" ");
    let t = head
        .to_lowercase()
        .replace(['*', '#', '\t'], " ")
        .split_whitespace()
        .collect::<Vec<_>>()
        .join(" ");
    let mut f = Vec::new();
    if t.contains("gnu general public license") || t.contains("gnu lesser general public") {
        f.push("GPL/LGPL");
    }
    // LibreSSL's two licences (BSD-style, with advertising clauses), named rather than
    // counted as BSD-4-Clause: the OpenSSL licence (some files only refer to it by name)
    // and Eric Young's SSLeay licence.
    let openssl = t.contains("openssl project")
        && (t.contains("openssl license")
            || t.contains("developed by the openssl project for use in the openssl toolkit"));
    let ssleay = t.contains("eric young") && t.contains("eay@cryptsoft.com");
    if openssl {
        f.push("OpenSSL");
    }
    if ssleay {
        f.push("SSLeay");
    }
    if openssl || ssleay {
        // The BSD-style text of the file is theirs.
    } else if t.contains("redistribution and use in source and binary forms") {
        if t.contains("all advertising materials mentioning") {
            f.push("BSD-4-Clause");
        } else if t.contains("neither the name") || t.contains("to endorse or promote") {
            f.push("BSD-3-Clause");
        } else {
            f.push("BSD-2-Clause");
        }
    }
    if t.contains("permission to use, copy, modify, and/or distribute this software for any")
        || t.contains("permission to use, copy, modify, and distribute this software for any purpose with or without fee")
    {
        f.push("ISC");
    }
    if t.contains("permission is hereby granted, free of charge") {
        f.push(if t.contains("unicode, inc") {
            "Unicode (data files and software)"
        } else {
            "MIT"
        });
    }
    if t.contains("beer-ware") {
        f.push("beerware");
    }
    if t.contains("carnegie mellon") && t.contains("permission to use, copy, modify and distribute")
    {
        f.push("Mach (CMU)");
    }
    if t.contains("spdx-license-identifier: apache-2.0 with llvm-exception") {
        f.push("Apache-2.0 WITH LLVM-exception");
    }
    if t.contains("lucent technologies")
        && t.contains("permission to use, copy, modify, and distribute")
    {
        f.push("Lucent (gdtoa)");
    }
    if t.contains("martin birgmeier") && t.contains("you may redistribute unmodified or modified") {
        f.push("Birgmeier (rand48)");
    }
    if t.contains("angelos d. keromytis")
        && t.contains("permission to use, copy, and modify this software with or without fee")
    {
        f.push("IPsec (Ioannidis/Keromytis)");
    }
    if t.contains("carnegie mellon")
        && t.contains("permission to use, copy, modify, and distribute this software and its documentation is hereby granted")
    {
        f.push("CMU (ALTQ)");
    }
    if t.contains("massachusetts institute of technology")
        && t.contains("permission to use, copy, modify, and distribute this software and its documentation for any purpose and without fee")
    {
        f.push("M.I.T.");
    }
    if (t.contains("developed at sunpro")
        || t.contains("developed at sunsoft")
        || t.contains("sun microsystems, inc."))
        && t.contains("is freely granted")
    {
        f.push("SunPro (fdlibm)");
    }
    if t.contains("carnegie mellon")
        && t.contains("copying and distribution is by permission of carnegie mellon and stanford")
    {
        f.push("CMU/Stanford (BOOTP)");
    }
    if t.contains("aleksey cheusov") && t.contains("permission to use or copy this software") {
        f.push("Cheusov");
    }
    if t.contains("daniel boulet") && t.contains("provided that this entire comment appears intact")
    {
        f.push("Boulet/RTMX");
    }
    if f.is_empty() {
        if t.contains("permission to use, copy, modify, and distribute this software")
            || t.contains("permission to use, copy, modify and distribute this software")
        {
            f.push("other permissive notice");
        } else if t.contains("public domain") {
            f.push("public domain");
        } else if t.contains("copyright") {
            f.push("unclassified (copyright without a recognised licence)");
        } else {
            f.push("no licence text");
        }
    }
    f
}

/// Classifies every source and header the build read and writes `licences.txt`.
fn licence_report(ctx: &Ctx<'_>) -> Result<()> {
    let installed = ctx.installed.lock().map_err(|_| "lock poisoned")?.clone();
    let inputs = ctx.inputs.lock().map_err(|_| "lock poisoned")?.clone();
    let mut by_file: BTreeMap<PathBuf, String> = BTreeMap::new();
    for p in inputs {
        // Generated files (stubs come from stdin, helpers from hash/helper.c) are not
        // classified themselves; their inputs are.
        if p.starts_with(&ctx.out) && !p.starts_with(&ctx.sysroot) {
            continue;
        }
        let real = installed.get(&p).cloned().unwrap_or(p);
        // `-I${.CURDIR}/../../lib/libc/gen` gives paths with `..`.
        let real = fs::canonicalize(&real).unwrap_or(real);
        if !real.starts_with(&ctx.src) {
            continue; // clang's own headers (stddef.h, ...), outside the OpenBSD tree
        }
        let Ok(text) = fs::read(&real) else {
            continue;
        };
        let mut fam = licence_families(&String::from_utf8_lossy(&text)).join(" + ");
        if fam == "no licence text" {
            // pdksh states its licence once, in bin/ksh/LEGAL.
            let legal = real.with_file_name("LEGAL");
            if let Ok(t) = fs::read_to_string(&legal) {
                let rel = legal.strip_prefix(&ctx.src).unwrap_or(&legal);
                fam = format!(
                    "{} (per {})",
                    licence_families(&t).join(" + "),
                    rel.display()
                );
            }
        }
        let rel = real.strip_prefix(&ctx.src).unwrap_or(&real).to_path_buf();
        by_file.insert(rel, fam);
    }
    let mut counts: BTreeMap<&str, usize> = BTreeMap::new();
    for f in by_file.values() {
        *counts.entry(f.as_str()).or_insert(0) += 1;
    }
    let path = ctx.out.join("licences.txt");
    let mut file = fs::File::create(&path).map_err(|e| format!("{}: {e}", path.display()))?;
    for (p, f) in &by_file {
        writeln!(file, "{f}\t{}", p.display())?;
    }
    println!(
        "  licences of the {} OpenBSD files compiled or included ({}):",
        by_file.len(),
        path.display()
    );
    for (f, n) in &counts {
        println!("    {n:5}  {f}");
    }
    // The families the user has accepted (`.claude/rules/scope-and-stubs.md`); the userland
    // ones (Apache-2.0 WITH LLVM-exception, public domain, no licence text, Lucent,
    // Birgmeier, Unicode, SunPro, Cheusov, Boulet/RTMX) only for code compiled unmodified,
    // decided 2026-10-03; the IPsec notice for the kernel too.
    let usual = [
        "ISC",
        "BSD-2-Clause",
        "BSD-3-Clause",
        "BSD-4-Clause",
        "MIT",
        "Mach (CMU)",
        "beerware",
        "public domain",
        "no licence text",
        "Apache-2.0 WITH LLVM-exception",
        "Lucent (gdtoa)",
        "Birgmeier (rand48)",
        "Unicode (data files and software)",
        "SunPro (fdlibm)",
        "Cheusov",
        "Boulet/RTMX",
        "IPsec (Ioannidis/Keromytis)",
        "CMU (ALTQ)",
        "M.I.T.",
        // LibreSSL (M9+), accepted by the user on 2026-10-03.
        "OpenSSL",
        "SSLeay",
        // libpcap's ppp.h and tcpdump's bootp.h (M9+), accepted by the user on 2026-10-04.
        "CMU/Stanford (BOOTP)",
    ];
    let unusual: Vec<_> = by_file
        .iter()
        .filter(|(_, f)| {
            let f = f.split(" (per ").next().unwrap_or(f);
            !f.split(" + ").all(|x| usual.contains(&x))
        })
        .collect();
    if !unusual.is_empty() {
        println!(
            "  files with less common notices (accepted: they are in the pinned tree; name new families in LICENSE):"
        );
        for (p, f) in unusual {
            println!("    {f}: {}", p.display());
        }
    }
    Ok(())
}

pub(crate) mod comp;
mod firmware;
mod images;
mod libraries;
pub(crate) mod miniroot;
mod passwd;
mod ramdisk;
pub(crate) use ramdisk::check_devices as check_ramdisk_devices;
pub(crate) mod sets;
mod shlib;
mod signify;
pub(crate) mod testca;
mod zoneinfo;
/* </CODE> */

/* <TESTS> */
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn licence_families_are_recognised() {
        let isc = "Permission to use, copy, modify, and distribute this software for any\n\
               * purpose with or without fee is hereby granted";
        assert_eq!(licence_families(isc), vec!["ISC"]);
        let bsd3 = "Redistribution and use in source and binary forms ...\n\
                * 3. Neither the name of the University nor";
        assert_eq!(licence_families(bsd3), vec!["BSD-3-Clause"]);
        let bsd4 = "Redistribution and use in source and binary forms\n\
                * 3. All advertising materials mentioning features";
        assert_eq!(licence_families(bsd4), vec!["BSD-4-Clause"]);
        assert_eq!(
            licence_families("This file is in the public domain."),
            vec!["public domain"]
        );
        assert_eq!(licence_families("int x;"), vec!["no licence text"]);
        let llvm = "// SPDX-License-Identifier: Apache-2.0 WITH LLVM-exception";
        assert_eq!(
            licence_families(llvm),
            vec!["Apache-2.0 WITH LLVM-exception"]
        );
        let lucent = "Copyright (C) 1998 by Lucent Technologies\n\
                  Permission to use, copy, modify, and distribute this software and\n\
                  its documentation for any purpose and without fee is hereby";
        assert_eq!(licence_families(lucent), vec!["Lucent (gdtoa)"]);
        let birgmeier = "Copyright (c) 1993 Martin Birgmeier\n\
                     * You may redistribute unmodified or modified versions";
        assert_eq!(licence_families(birgmeier), vec!["Birgmeier (rand48)"]);
        let unicode = "Copyright (c) 1991-2015 Unicode, Inc. All rights reserved.\n\
                   * Permission is hereby granted, free of charge, to any person";
        assert_eq!(
            licence_families(unicode),
            vec!["Unicode (data files and software)"]
        );
        let sunpro = "Developed at SunPro, a Sun Microsystems, Inc. business.\n\
                  * Permission to use, copy, modify, and distribute this\n\
                  * software is freely granted, provided that this notice";
        assert_eq!(licence_families(sunpro), vec!["SunPro (fdlibm)"]);
        let bootp = "Copyright 1988 by Carnegie Mellon.\n\
                 * Permission to use, copy, modify, and distribute this program for any\n\
                 * permission, and notice be given in supporting documentation that copying\n\
                 * and distribution is by permission of Carnegie Mellon and Stanford\n\
                 * University.  Carnegie Mellon makes no representations about the";
        assert_eq!(licence_families(bootp), vec!["CMU/Stanford (BOOTP)"]);
    }

    #[test]
    fn libressl_licences_are_named() {
        let dual = "Copyright (c) 1998-2005 The OpenSSL Project. Redistribution and use in source \
                and binary forms ... All advertising materials mentioning ... This product \
                includes software developed by the OpenSSL Project for use in the OpenSSL \
                Toolkit. ... Copyright (C) 1995-1998 Eric Young (eay@cryptsoft.com)";
        assert_eq!(licence_families(dual), vec!["OpenSSL", "SSLeay"]);
        let by_name = "Copyright (c) 2008 The OpenSSL Project. All rights reserved.\n * Rights for \
                   redistribution and usage ... according to the OpenSSL license.";
        assert_eq!(licence_families(by_name), vec!["OpenSSL"]);
        let eay = "Copyright (C) 1995-1998 Eric Young (eay@cryptsoft.com) Redistribution and use \
               in source and binary forms ... All advertising materials mentioning";
        assert_eq!(licence_families(eay), vec!["SSLeay"]);
    }

    #[test]
    fn compiler_builtins() {
        for s in [
            "__multf3",
            "__extenddftf2",
            "__fixtfsi",
            "__udivti3",
            "__emutls_get_address",
        ] {
            assert!(is_compiler_builtin(s), "{s}");
        }
        for s in ["_exit", "main", "__start", "__libc_init"] {
            assert!(!is_compiler_builtin(s), "{s}");
        }
    }

    #[test]
    fn depfiles_and_locals() {
        let d = "x.o: /a/x.c /a/b.h \\\n  /a/c.h\n\n/a/b.h:\n";
        assert_eq!(
            parse_depfile(d),
            vec![
                PathBuf::from("/a/x.c"),
                PathBuf::from("/a/b.h"),
                PathBuf::from("/a/c.h")
            ]
        );
        let l = locals("___realpath.o", &[PathBuf::from("/s/helper.c")]);
        assert_eq!(l["@"], "___realpath.o");
        assert_eq!(l[".PREFIX"], "___realpath");
        assert_eq!(l[">"], "/s/helper.c");
    }

    #[test]
    fn y_c_rule_runs_yacc_d_with_the_target_as_output() -> Result<()> {
        let predefined = [
            ("YACC", "/out/host/bin/yacc".to_string()),
            ("YACC.y", "${YACC} -d ${YFLAGS}".to_string()),
            ("YFLAGS", String::new()),
        ];
        let mk = Make::new(Path::new("/nonexistent"), &predefined, &[]);
        let rule: Vec<String> = RULE_Y_C.iter().map(|c| c.to_string()).collect();
        let job = Job::from_rule(
            &mk,
            &rule,
            "parse.c",
            vec![PathBuf::from("/src/parse.y")],
            Path::new("/obj"),
        )?;
        let cmds: Vec<&str> = job.commands.iter().map(|(c, _)| c.as_str()).collect();
        assert_eq!(
            cmds.iter()
                .map(|c| c.split_whitespace().collect::<Vec<_>>().join(" "))
                .collect::<Vec<_>>(),
            ["/out/host/bin/yacc -d -o parse.c /src/parse.y"]
        );
        assert_eq!(job.target, PathBuf::from("/obj/parse.c"));
        Ok(())
    }

    #[test]
    fn l_c_rule_runs_lex_with_lflags_and_the_target_as_output() -> Result<()> {
        let predefined = [
            ("LEX", "/out/host/bin/lex".to_string()),
            ("LEX.l", "${LEX} ${LFLAGS}".to_string()),
            ("LFLAGS", "-Ppcap_yy".to_string()),
        ];
        let mk = Make::new(Path::new("/nonexistent"), &predefined, &[]);
        let rule: Vec<String> = RULE_L_C.iter().map(|c| c.to_string()).collect();
        let job = Job::from_rule(
            &mk,
            &rule,
            "scanner.c",
            vec![PathBuf::from("/src/scanner.l")],
            Path::new("/obj"),
        )?;
        let cmds: Vec<String> = job
            .commands
            .iter()
            .map(|(c, _)| c.split_whitespace().collect::<Vec<_>>().join(" "))
            .collect();
        assert_eq!(
            cmds,
            ["/out/host/bin/lex -Ppcap_yy -o scanner.c /src/scanner.l"]
        );
        assert_eq!(job.target, PathBuf::from("/obj/scanner.c"));
        Ok(())
    }

    /// Evaluates the real `lib/libc` Makefiles for amd64: `cargo test -p xtask -- --ignored`
    /// with `$OPENBSD_SRC` naming the reference clone.
    #[test]
    #[ignore]
    fn libc_makefile_evaluates() {
        let src = PathBuf::from(std::env::var("OPENBSD_SRC").expect("OPENBSD_SRC"));
        let curdir = src.join("lib/libc");
        let predefined = [
            ("MACHINE", "amd64".to_string()),
            ("MACHINE_ARCH", "amd64".to_string()),
            ("MACHINE_CPU", "amd64".to_string()),
            ("CFLAGS", "-O2".to_string()),
        ];
        let sys_mk = [
            ("bsd.own.mk", BSD_OWN_MK),
            ("bsd.prog.mk", BSD_PROG_MK),
            ("bsd.lib.mk", BSD_PROG_MK),
        ];
        let mut mk = Make::new(&curdir, &predefined, &sys_mk);
        mk.read(&curdir.join("Makefile")).unwrap();
        let words = |v: &str| mk.words(v).unwrap();
        assert!(words("ASM").contains(&"access.o".to_string()));
        assert!(words("HIDDEN").contains(&"read.o".to_string()));
        assert!(words("PSEUDO_NOERR").contains(&"_exit.o".to_string()));
        assert!(words("SRCS").contains(&"w_read.c".to_string()));
        assert!(words("SRCS").contains(&"md5hl.c".to_string()));
        assert!(words("CFLAGS").contains(&"-fret-clean".to_string()));
        assert!(words("CFLAGS").contains(&"-DYP".to_string()));
        assert!(mk.search("_atomic_lock.c").is_some());
        assert!(mk.rule_for("md5hl.c").is_some());
        assert!(mk.rule_for("access.o").is_some());
    }

    #[test]
    fn own_programs_have_a_makefile_and_their_source() {
        let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("../..");
        for dir in OWN_PROGRAMS {
            assert!(!PROGRAMS.contains(dir), "{dir} is in both lists");
            let makefile = fs::read_to_string(root.join(dir).join("Makefile")).unwrap();
            let prog = makefile
                .lines()
                .find_map(|l| l.strip_prefix("PROG="))
                .unwrap()
                .trim();
            assert!(root.join(dir).join(format!("{prog}.c")).is_file(), "{dir}");
            assert!(makefile.contains("LDSTATIC=\t-static"), "{dir}");
        }
    }
}
/* </TESTS> */
