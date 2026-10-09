/* <LICENSES> */
/*-
 * Copyright (c) 1983, 1990, 1993
 *	The Regents of the University of California.  All rights reserved.
 * (c) UNIX System Laboratories, Inc.
 * All or some portions of this file are derived from material licensed
 * to the University of California by American Telephone and Telegraph
 * Co. or Unix System Laboratories, Inc. and are reproduced herein with
 * the permission of UNIX System Laboratories, Inc.
 *
 * Redistribution and use in source and binary forms, with or without
 * modification, are permitted provided that the following conditions
 * are met:
 * 1. Redistributions of source code must retain the above copyright
 *    notice, this list of conditions and the following disclaimer.
 * 2. Redistributions in binary form must reproduce the above copyright
 *    notice, this list of conditions and the following disclaimer in the
 *    documentation and/or other materials provided with the distribution.
 * 3. Neither the name of the University nor the names of its contributors
 *    may be used to endorse or promote products derived from this software
 *    without specific prior written permission.
 *
 * THIS SOFTWARE IS PROVIDED BY THE REGENTS AND CONTRIBUTORS ``AS IS'' AND
 * ANY EXPRESS OR IMPLIED WARRANTIES, INCLUDING, BUT NOT LIMITED TO, THE
 * IMPLIED WARRANTIES OF MERCHANTABILITY AND FITNESS FOR A PARTICULAR PURPOSE
 * ARE DISCLAIMED.  IN NO EVENT SHALL THE REGENTS OR CONTRIBUTORS BE LIABLE
 * FOR ANY DIRECT, INDIRECT, INCIDENTAL, SPECIAL, EXEMPLARY, OR CONSEQUENTIAL
 * DAMAGES (INCLUDING, BUT NOT LIMITED TO, PROCUREMENT OF SUBSTITUTE GOODS
 * OR SERVICES; LOSS OF USE, DATA, OR PROFITS; OR BUSINESS INTERRUPTION)
 * HOWEVER CAUSED AND ON ANY THEORY OF LIABILITY, WHETHER IN CONTRACT, STRICT
 * LIABILITY, OR TORT (INCLUDING NEGLIGENCE OR OTHERWISE) ARISING IN ANY WAY
 * OUT OF THE USE OF THIS SOFTWARE, EVEN IF ADVISED OF THE POSSIBILITY OF
 * SUCH DAMAGE.
 *
 *	@(#)fcntl.h	8.3 (Berkeley) 1/21/94
 */
/* </LICENSES> */

/* <CODE> */
//! `<sys/fcntl.h>`: the definitions for `open(2)` and `fcntl(2)` described by POSIX, and the
//! related kernel definitions: the `O_*` open flags and their kernel `F*` twins, the
//! `fcntl` commands, `FD_CLOEXEC`/`FD_CLOFORK`, the record-locking types and `struct flock`,
//! the `flock(2)` operations and the `*at` constants.
//!
//! Upstream: sys/sys/fcntl.h @ 3ce1f3f79392
//!
//! ## Deviations
//! - `FFLAGS`/`OFLAGS` are the `const fn`s [`fflags`]/[`oflags`] over `i32`, the type of the
//!   `int` flags `open(2)` and `fcntl(2)` take; `f_flag` (a `u_int`) converts at its sites.
//! - [`Flock::from_bytes`]/[`Flock::to_bytes`] are what `copyin`/`copyout` of a `struct
//!   flock` become: the structure has no padding, so its bytes are its fields in order.
//! - The userland prototypes (`open`, `creat`, `fcntl`, `flock`, `openat`) are not kernel
//!   material.

use crate::sys::types::{Off, Pid};

/*
 * File status flags: these are used by open(2), fcntl(2). They are also used (indirectly) in
 * the kernel file structure f_flags, which is a superset of the open/fcntl flags. Open flags
 * and f_flags are inter-convertible using OFLAGS(fflags) and FFLAGS(oflags). Open/fcntl
 * flags begin with O_; kernel-internal flags begin with F.
 */

/// `O_RDONLY`: open for reading only.
pub const O_RDONLY: i32 = 0x0000;
/// `O_WRONLY`: open for writing only.
pub const O_WRONLY: i32 = 0x0001;
/// `O_RDWR`: open for reading and writing.
pub const O_RDWR: i32 = 0x0002;
/// `O_ACCMODE`: mask for the above modes.
pub const O_ACCMODE: i32 = 0x0003;

/// `FREAD`: the kernel encoding of open mode, readable (1 greater than `O_RDONLY`).
pub const FREAD: i32 = 0x0001;
/// `FWRITE`: the kernel encoding of open mode, writable.
pub const FWRITE: i32 = 0x0002;
/// `O_NONBLOCK`: no delay.
pub const O_NONBLOCK: i32 = 0x0004;
/// `O_APPEND`: set append mode.
pub const O_APPEND: i32 = 0x0008;
/// `O_SHLOCK`: open with shared file lock.
pub const O_SHLOCK: i32 = 0x0010;
/// `O_EXLOCK`: open with exclusive file lock.
pub const O_EXLOCK: i32 = 0x0020;
/// `O_ASYNC`: signal pgrp when data ready.
pub const O_ASYNC: i32 = 0x0040;
/// `O_FSYNC`: backwards compatibility.
pub const O_FSYNC: i32 = 0x0080;
/// `O_SYNC`: synchronous writes.
pub const O_SYNC: i32 = 0x0080;
/// `O_DSYNC`: synchronous data writes (all or nothing in OpenBSD: `O_SYNC`).
pub const O_DSYNC: i32 = O_SYNC;
/// `O_RSYNC`: synchronous reads (`O_SYNC`).
pub const O_RSYNC: i32 = O_SYNC;

/// `O_NOFOLLOW`: if path is a symlink, don't follow.
pub const O_NOFOLLOW: i32 = 0x0100;

/// `O_CREAT`: create if nonexistent.
pub const O_CREAT: i32 = 0x0200;
/// `O_TRUNC`: truncate to zero length.
pub const O_TRUNC: i32 = 0x0400;
/// `O_EXCL`: error if already exists.
pub const O_EXCL: i32 = 0x0800;

/// `O_NOCTTY`: don't assign controlling terminal.
pub const O_NOCTTY: i32 = 0x8000;

/// `O_CLOEXEC`: atomically set `FD_CLOEXEC`.
pub const O_CLOEXEC: i32 = 0x10000;
/// `O_DIRECTORY`: fail if not a directory.
pub const O_DIRECTORY: i32 = 0x20000;

/// `O_CLOFORK`: atomically set `FD_CLOFORK`.
pub const O_CLOFORK: i32 = 0x40000;

/// `FFLAGS(oflags)`: convert from `open()` flags to fflags: `O_RD`/`WR` become
/// `FREAD`/`FWRITE`. For out-of-range values for the flags, be slightly careful (but lossy).
pub const fn fflags(oflags: i32) -> i32 {
    (oflags & !O_ACCMODE) | (oflags.wrapping_add(1) & O_ACCMODE)
}

/// `OFLAGS(fflags)`: convert from fflags back to `open()` flags.
pub const fn oflags(fflags: i32) -> i32 {
    (fflags & !O_ACCMODE) | (fflags.wrapping_sub(1) & O_ACCMODE)
}

/// `FAPPEND`: kernel/compat.
pub const FAPPEND: i32 = O_APPEND;
/// `FASYNC`: kernel/compat.
pub const FASYNC: i32 = O_ASYNC;
/// `FFSYNC`: kernel.
pub const FFSYNC: i32 = O_SYNC;
/// `FNONBLOCK`: kernel.
pub const FNONBLOCK: i32 = O_NONBLOCK;
/// `FNDELAY`: compat.
pub const FNDELAY: i32 = O_NONBLOCK;
/// `O_NDELAY`: compat.
pub const O_NDELAY: i32 = O_NONBLOCK;

/// `FMASK`: bits to save after open.
pub const FMASK: i32 = FREAD | FWRITE | FAPPEND | FASYNC | FFSYNC | FNONBLOCK;
/// `FCNTLFLAGS`: bits settable by `fcntl(F_SETFL, ...)`.
pub const FCNTLFLAGS: i32 = FAPPEND | FASYNC | FFSYNC | FNONBLOCK;

/*
 * Constants used for fcntl(2)
 */

/// `F_DUPFD`: duplicate file descriptor.
pub const F_DUPFD: i32 = 0;
/// `F_GETFD`: get file descriptor flags.
pub const F_GETFD: i32 = 1;
/// `F_SETFD`: set file descriptor flags.
pub const F_SETFD: i32 = 2;
/// `F_GETFL`: get file status flags.
pub const F_GETFL: i32 = 3;
/// `F_SETFL`: set file status flags.
pub const F_SETFL: i32 = 4;
/// `F_GETOWN`: get SIGIO/SIGURG proc/pgrp.
pub const F_GETOWN: i32 = 5;
/// `F_SETOWN`: set SIGIO/SIGURG proc/pgrp.
pub const F_SETOWN: i32 = 6;
/// `F_GETLK`: get record locking information.
pub const F_GETLK: i32 = 7;
/// `F_SETLK`: set record locking information.
pub const F_SETLK: i32 = 8;
/// `F_SETLKW`: `F_SETLK`; wait if blocked.
pub const F_SETLKW: i32 = 9;
/// `F_DUPFD_CLOEXEC`: duplicate with `FD_CLOEXEC` set.
pub const F_DUPFD_CLOEXEC: i32 = 10;
/// `F_ISATTY`: used by `isatty(3)`.
pub const F_ISATTY: i32 = 11;
/// `F_DUPFD_CLOFORK`: duplicate with `FD_CLOFORK` set.
pub const F_DUPFD_CLOFORK: i32 = 12;

/// `FD_CLOEXEC`: close-on-exec flag (`F_GETFD`, `F_SETFD`).
pub const FD_CLOEXEC: i32 = 1;
/// `FD_CLOFORK`: close-on-fork flag.
pub const FD_CLOFORK: i32 = 4;

/// `F_RDLCK`: shared or read lock.
pub const F_RDLCK: i16 = 1;
/// `F_UNLCK`: unlock.
pub const F_UNLCK: i16 = 2;
/// `F_WRLCK`: exclusive or write lock.
pub const F_WRLCK: i16 = 3;
/// `F_WAIT`: wait until lock is granted.
pub const F_WAIT: i32 = 0x010;
/// `F_FLOCK`: use `flock(2)` semantics for lock.
pub const F_FLOCK: i32 = 0x020;
/// `F_POSIX`: use POSIX semantics for lock.
pub const F_POSIX: i32 = 0x040;
/// `F_INTR`: lock operation interrupted.
pub const F_INTR: i32 = 0x080;

/// `LOCK_SH`: shared file lock (`flock(2)`).
pub const LOCK_SH: i32 = 0x01;
/// `LOCK_EX`: exclusive file lock.
pub const LOCK_EX: i32 = 0x02;
/// `LOCK_NB`: don't block when locking.
pub const LOCK_NB: i32 = 0x04;
/// `LOCK_UN`: unlock file.
pub const LOCK_UN: i32 = 0x08;

/// `AT_FDCWD`: the `*at` calls' "current directory" descriptor.
pub const AT_FDCWD: i32 = -100;

/// `AT_EACCESS`.
pub const AT_EACCESS: i32 = 0x01;
/// `AT_SYMLINK_NOFOLLOW`.
pub const AT_SYMLINK_NOFOLLOW: i32 = 0x02;
/// `AT_SYMLINK_FOLLOW`.
pub const AT_SYMLINK_FOLLOW: i32 = 0x04;
/// `AT_REMOVEDIR`.
pub const AT_REMOVEDIR: i32 = 0x08;

/// `struct flock`: advisory file segment locking data type, information passed to system by
/// user.
#[repr(C)]
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Flock {
    /// `l_start`: starting offset.
    pub l_start: Off,
    /// `l_len`: len = 0 means until end of file.
    pub l_len: Off,
    /// `l_pid`: lock owner.
    pub l_pid: Pid,
    /// `l_type`: lock type: read/write, etc.
    pub l_type: i16,
    /// `l_whence`: type of `l_start`.
    pub l_whence: i16,
}

impl Flock {
    /// The structure's size in user space.
    pub const SIZE: usize = size_of::<Flock>();

    /// The structure read from its user-space bytes (`copyin`).
    pub fn from_bytes(b: &[u8; Self::SIZE]) -> Self {
        let i64_at = |o: usize| {
            let mut w = [0u8; 8];
            w.copy_from_slice(&b[o..o + 8]);
            i64::from_ne_bytes(w)
        };
        let i16_at = |o: usize| i16::from_ne_bytes([b[o], b[o + 1]]);
        Self {
            l_start: i64_at(0),
            l_len: i64_at(8),
            l_pid: i32::from_ne_bytes([b[16], b[17], b[18], b[19]]),
            l_type: i16_at(20),
            l_whence: i16_at(22),
        }
    }

    /// The structure's user-space bytes (`copyout`).
    pub fn to_bytes(&self) -> [u8; Self::SIZE] {
        let mut b = [0u8; Self::SIZE];
        b[0..8].copy_from_slice(&self.l_start.to_ne_bytes());
        b[8..16].copy_from_slice(&self.l_len.to_ne_bytes());
        b[16..20].copy_from_slice(&self.l_pid.to_ne_bytes());
        b[20..22].copy_from_slice(&self.l_type.to_ne_bytes());
        b[22..24].copy_from_slice(&self.l_whence.to_ne_bytes());
        b
    }
}

const _: () = {
    assert!(size_of::<Flock>() == 24);
    assert!(core::mem::offset_of!(Flock, l_pid) == 16);
    assert!(core::mem::offset_of!(Flock, l_whence) == 22);
};
/* </CODE> */

/* <TESTS> */
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn fflags_and_oflags_convert_the_access_mode() {
        assert_eq!(fflags(O_RDONLY), FREAD);
        assert_eq!(fflags(O_WRONLY), FWRITE);
        assert_eq!(fflags(O_RDWR), FREAD | FWRITE);
        assert_eq!(fflags(O_RDWR | O_NONBLOCK), FREAD | FWRITE | FNONBLOCK);
        for o in [O_RDONLY, O_WRONLY, O_RDWR] {
            assert_eq!(oflags(fflags(o | O_APPEND)), o | O_APPEND);
        }
    }

    #[test]
    fn flock_bytes_round_trip() {
        let fl = Flock {
            l_start: -5,
            l_len: 1 << 40,
            l_pid: 7,
            l_type: F_WRLCK,
            l_whence: 1,
        };
        assert_eq!(Flock::from_bytes(&fl.to_bytes()), fl);
    }

    #[test]
    #[ignore = "needs OPENBSD_SRC (just test-ref)"]
    fn constants_match_the_c_header() {
        let defs = crate::reftest::defines("sys/sys/fcntl.h");
        for (name, value) in [
            ("O_NONBLOCK", O_NONBLOCK),
            ("O_APPEND", O_APPEND),
            ("O_CREAT", O_CREAT),
            ("O_CLOEXEC", O_CLOEXEC),
            ("O_CLOFORK", O_CLOFORK),
            ("F_DUPFD_CLOFORK", F_DUPFD_CLOFORK),
            ("F_ISATTY", F_ISATTY),
            ("FD_CLOFORK", FD_CLOFORK),
            ("F_POSIX", F_POSIX),
            ("LOCK_UN", LOCK_UN),
        ] {
            assert_eq!(
                crate::reftest::int(&defs, name),
                Some(i64::from(value)),
                "{name}"
            );
        }
    }
}
/* </TESTS> */
