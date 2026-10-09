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

/*
 * Copyright (c) 1982, 1986, 1989, 1993
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
 *	@(#)errno.h	8.5 (Berkeley) 1/21/94
 */
/* </LICENSES> */

/* <CODE> */
//! `errno(2)` values as the kernel sees them: `<sys/errno.h>`.
//!
//! Upstream: sys/sys/errno.h @ 3ce1f3f79392
//!
//! Where C returns an `int` that is `0` or an errno, Rust returns `Result<T, Errno>`
//! (`docs/C_TO_RUST.md`). The two kernel-internal pseudo-errors `ERESTART` and `EJUSTRETURN`
//! are variants too: they steer the syscall return path and never reach userland.
//!
//! ## Deviations
//! - `EWOULDBLOCK` is an associated constant aliasing `EAGAIN`: a Rust enum cannot give two
//!   variants one discriminant.
//! - `ELAST` is an associated constant (`i32`), not a variant.
//! - `__BSD_VISIBLE` is always on: the kernel sees every value.

/// Declares [`Errno`] from one list, so the enum, `from_raw` and the test table cannot drift.
macro_rules! errno_values {
    ($( $(#[$doc:meta])* $name:ident = $value:literal, )+) => {
        /// An error number: what C passes around as `int error`.
        #[repr(i32)]
        #[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
        #[allow(clippy::upper_case_acronyms)] // OpenBSD names, verbatim, for grep-ability
        pub enum Errno {
            $( $(#[$doc])* $name = $value, )+
        }

        impl Errno {
            /// Every value with its C name, in header order (for reference-backed tests).
            #[cfg(test)]
            pub(crate) const TABLE: &'static [(&'static str, i32)] =
                &[ $( (stringify!($name), $value), )+ ];

            /// The `Errno` for a raw C value, `None` if the value names no error.
            pub const fn from_raw(raw: i32) -> Option<Self> {
                match raw {
                    $( $value => Some(Self::$name), )+
                    _ => None,
                }
            }
        }
    };
}

errno_values! {
    /// Operation not permitted.
    EPERM = 1,
    /// No such file or directory.
    ENOENT = 2,
    /// No such process.
    ESRCH = 3,
    /// Interrupted system call.
    EINTR = 4,
    /// Input/output error.
    EIO = 5,
    /// Device not configured.
    ENXIO = 6,
    /// Argument list too long.
    E2BIG = 7,
    /// Exec format error.
    ENOEXEC = 8,
    /// Bad file descriptor.
    EBADF = 9,
    /// No child processes.
    ECHILD = 10,
    /// Resource deadlock avoided (11 was `EAGAIN`).
    EDEADLK = 11,
    /// Cannot allocate memory.
    ENOMEM = 12,
    /// Permission denied.
    EACCES = 13,
    /// Bad address.
    EFAULT = 14,
    /// Block device required.
    ENOTBLK = 15,
    /// Device busy.
    EBUSY = 16,
    /// File exists.
    EEXIST = 17,
    /// Cross-device link.
    EXDEV = 18,
    /// Operation not supported by device.
    ENODEV = 19,
    /// Not a directory.
    ENOTDIR = 20,
    /// Is a directory.
    EISDIR = 21,
    /// Invalid argument.
    EINVAL = 22,
    /// Too many open files in system.
    ENFILE = 23,
    /// Too many open files.
    EMFILE = 24,
    /// Inappropriate ioctl for device.
    ENOTTY = 25,
    /// Text file busy.
    ETXTBSY = 26,
    /// File too large.
    EFBIG = 27,
    /// No space left on device.
    ENOSPC = 28,
    /// Illegal seek.
    ESPIPE = 29,
    /// Read-only file system.
    EROFS = 30,
    /// Too many links.
    EMLINK = 31,
    /// Broken pipe.
    EPIPE = 32,

    // math software
    /// Numerical argument out of domain.
    EDOM = 33,
    /// Result too large.
    ERANGE = 34,

    // non-blocking and interrupt i/o
    /// Resource temporarily unavailable.
    EAGAIN = 35,
    /// Operation now in progress.
    EINPROGRESS = 36,
    /// Operation already in progress.
    EALREADY = 37,

    // ipc/network software -- argument errors
    /// Socket operation on non-socket.
    ENOTSOCK = 38,
    /// Destination address required.
    EDESTADDRREQ = 39,
    /// Message too long.
    EMSGSIZE = 40,
    /// Protocol wrong type for socket.
    EPROTOTYPE = 41,
    /// Protocol not available.
    ENOPROTOOPT = 42,
    /// Protocol not supported.
    EPROTONOSUPPORT = 43,
    /// Socket type not supported.
    ESOCKTNOSUPPORT = 44,
    /// Operation not supported.
    EOPNOTSUPP = 45,
    /// Protocol family not supported.
    EPFNOSUPPORT = 46,
    /// Address family not supported by protocol family.
    EAFNOSUPPORT = 47,
    /// Address already in use.
    EADDRINUSE = 48,
    /// Can't assign requested address.
    EADDRNOTAVAIL = 49,

    // ipc/network software -- operational errors
    /// Network is down.
    ENETDOWN = 50,
    /// Network is unreachable.
    ENETUNREACH = 51,
    /// Network dropped connection on reset.
    ENETRESET = 52,
    /// Software caused connection abort.
    ECONNABORTED = 53,
    /// Connection reset by peer.
    ECONNRESET = 54,
    /// No buffer space available.
    ENOBUFS = 55,
    /// Socket is already connected.
    EISCONN = 56,
    /// Socket is not connected.
    ENOTCONN = 57,
    /// Can't send after socket shutdown.
    ESHUTDOWN = 58,
    /// Too many references: can't splice.
    ETOOMANYREFS = 59,
    /// Operation timed out.
    ETIMEDOUT = 60,
    /// Connection refused.
    ECONNREFUSED = 61,

    /// Too many levels of symbolic links.
    ELOOP = 62,
    /// File name too long.
    ENAMETOOLONG = 63,

    // should be rearranged
    /// Host is down.
    EHOSTDOWN = 64,
    /// No route to host.
    EHOSTUNREACH = 65,
    /// Directory not empty.
    ENOTEMPTY = 66,

    // quotas & mush
    /// Too many processes.
    EPROCLIM = 67,
    /// Too many users.
    EUSERS = 68,
    /// Disk quota exceeded.
    EDQUOT = 69,

    // Network File System
    /// Stale NFS file handle.
    ESTALE = 70,
    /// Too many levels of remote in path.
    EREMOTE = 71,
    /// RPC struct is bad.
    EBADRPC = 72,
    /// RPC version wrong.
    ERPCMISMATCH = 73,
    /// RPC program not available.
    EPROGUNAVAIL = 74,
    /// Program version wrong.
    EPROGMISMATCH = 75,
    /// Bad procedure for program.
    EPROCUNAVAIL = 76,

    /// No locks available.
    ENOLCK = 77,
    /// Function not implemented.
    ENOSYS = 78,

    /// Inappropriate file type or format.
    EFTYPE = 79,
    /// Authentication error.
    EAUTH = 80,
    /// Need authenticator.
    ENEEDAUTH = 81,
    /// IPsec processing failure.
    EIPSEC = 82,
    /// Attribute not found.
    ENOATTR = 83,
    /// Illegal byte sequence.
    EILSEQ = 84,
    /// No medium found.
    ENOMEDIUM = 85,
    /// Wrong medium type.
    EMEDIUMTYPE = 86,
    /// Value too large to be stored in data type.
    EOVERFLOW = 87,
    /// Operation canceled.
    ECANCELED = 88,
    /// Identifier removed.
    EIDRM = 89,
    /// No message of desired type.
    ENOMSG = 90,
    /// Not supported.
    ENOTSUP = 91,
    /// Bad message.
    EBADMSG = 92,
    /// State not recoverable.
    ENOTRECOVERABLE = 93,
    /// Previous owner died.
    EOWNERDEAD = 94,
    /// Protocol error.
    EPROTO = 95,

    // pseudo-errors returned inside kernel to modify return to process
    /// Restart syscall.
    ERESTART = -1,
    /// Don't modify regs, just return.
    EJUSTRETURN = -2,
}

impl Errno {
    /// Operation would block: the same value as `EAGAIN`.
    pub const EWOULDBLOCK: Errno = Errno::EAGAIN;
    /// Must be equal to the largest errno.
    pub const ELAST: i32 = 95;

    /// The raw C value.
    pub const fn as_i32(self) -> i32 {
        self as i32
    }
}
/* </CODE> */

/* <TESTS> */
#[cfg(test)]
mod tests {
    // Tests of `errno.rs`; see there.

    use super::*;

    #[test]
    fn values_and_aliases() {
        assert_eq!(Errno::EPERM.as_i32(), 1);
        assert_eq!(Errno::EPROTO.as_i32(), Errno::ELAST);
        assert_eq!(Errno::EWOULDBLOCK, Errno::EAGAIN);
        assert_eq!(Errno::ERESTART.as_i32(), -1);
        assert_eq!(Errno::EJUSTRETURN.as_i32(), -2);
    }

    #[test]
    fn from_raw_round_trips_every_variant() {
        for (name, value) in Errno::TABLE {
            let e = Errno::from_raw(*value).unwrap_or_else(|| panic!("{name} not mapped"));
            assert_eq!(e.as_i32(), *value, "{name}");
        }
        assert_eq!(Errno::from_raw(0), None);
        assert_eq!(Errno::from_raw(96), None);
        assert_eq!(Errno::from_raw(-3), None);
    }

    #[test]
    fn errno_fits_in_an_i32_and_is_not_zero() {
        assert_eq!(core::mem::size_of::<Errno>(), 4);
        assert!(Errno::TABLE.iter().all(|(_, v)| *v != 0));
    }

    #[test]
    #[ignore = "needs OPENBSD_SRC (just test-ref)"]
    fn values_match_the_c_header() {
        let defs = crate::reftest::defines("sys/sys/errno.h");
        for (name, value) in Errno::TABLE {
            assert_eq!(
                crate::reftest::int(&defs, name),
                Some(i64::from(*value)),
                "{name}"
            );
        }
        for name in defs.keys().filter(|n| n.starts_with('E')) {
            let header = crate::reftest::int(&defs, name);
            match name.as_str() {
                "EWOULDBLOCK" => {
                    assert_eq!(header, Some(i64::from(Errno::EWOULDBLOCK.as_i32())));
                }
                "ELAST" => assert_eq!(header, Some(i64::from(Errno::ELAST))),
                _ => assert!(
                    Errno::TABLE.iter().any(|(n, _)| *n == name.as_str()),
                    "{name} is in errno.h but not ported"
                ),
            }
        }
    }
}
/* </TESTS> */
