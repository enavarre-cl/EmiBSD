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
 * Copyright (c) 1988, 1993
 *	The Regents of the University of California.  All rights reserved.
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
 *	@(#)saerrno.h	8.1 (Berkeley) 6/11/93
 */
/* </LICENSES> */

/* <CODE> */
//! `saerrno.h`: the error numbers of the standalone library, `<sys/errno.h>`'s and the
//! special standalone ones (`EADAPT` .. `EHER`).
//!
//! Upstream: sys/lib/libsa/saerrno.h @ 3ce1f3f79392
//!
//! ## Deviations
//! - The C's `int` error numbers are an `Errno` newtype over `i32` rather than the kernel's
//!   `enum`: libsa's errors are open-ended, as in C. A device's open routine may return a
//!   status that is no error number (efiboot's `espopen` and `tftpopen` return 1, "not
//!   mine"), `open()` passes it on, and boot(8) then compares `errno` with `EPERM`, which is 1.
//!   `ufs_readdir` returns -1 at the end of a directory, which `readdir()` passes on too.
//! - `extern int errno` is [`ERRNO`](crate::dev::ERRNO), defined in `dev.rs` as `dev.c`
//!   defines it.
//! - Only the `<sys/errno.h>` numbers libsa and the boot programs use are declared here; the
//!   kernel's `sys/sys/errno.rs` has them all.

/// An error number: one of `<sys/errno.h>`'s, a standalone one, or a status a device routine
/// returned.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct Errno(pub i32);

impl Errno {
    /// `EPERM`: operation not permitted.
    pub const EPERM: Self = Self(1);
    /// `ENOENT`: no such file or directory.
    pub const ENOENT: Self = Self(2);
    /// `EIO`: input/output error.
    pub const EIO: Self = Self(5);
    /// `ENXIO`: device not configured.
    pub const ENXIO: Self = Self(6);
    /// `E2BIG`: argument list too long.
    pub const E2BIG: Self = Self(7);
    /// `ENOEXEC`: exec format error.
    pub const ENOEXEC: Self = Self(8);
    /// `EBADF`: bad file descriptor.
    pub const EBADF: Self = Self(9);
    /// `ENOMEM`: cannot allocate memory.
    pub const ENOMEM: Self = Self(12);
    /// `ENODEV`: operation not supported by device.
    pub const ENODEV: Self = Self(19);
    /// `EEXIST`: file exists.
    pub const EEXIST: Self = Self(17);
    /// `ENOTDIR`: not a directory.
    pub const ENOTDIR: Self = Self(20);
    /// `EINVAL`: invalid argument.
    pub const EINVAL: Self = Self(22);
    /// `EMFILE`: too many open files.
    pub const EMFILE: Self = Self(24);
    /// `EFBIG`: file too large.
    pub const EFBIG: Self = Self(27);
    /// `ENOSPC`: no space left on device.
    pub const ENOSPC: Self = Self(28);
    /// `EROFS`: read-only file system.
    pub const EROFS: Self = Self(30);
    /// `EOPNOTSUPP`: operation not supported.
    pub const EOPNOTSUPP: Self = Self(45);
    /// `ETIMEDOUT`: operation timed out.
    pub const ETIMEDOUT: Self = Self(60);
    /// `ESTALE`: stale NFS file handle.
    pub const ESTALE: Self = Self(70);
    /// `EFTYPE`: inappropriate file type or format.
    pub const EFTYPE: Self = Self(79);
    /// `ELAST`: the largest `<sys/errno.h>` number.
    pub const ELAST: Self = Self(95);
    /// `EADAPT`: bad adaptor.
    pub const EADAPT: Self = Self(Self::ELAST.0 + 1);
    /// `ECTLR`: bad controller.
    pub const ECTLR: Self = Self(Self::ELAST.0 + 2);
    /// `EUNIT`: bad drive.
    pub const EUNIT: Self = Self(Self::ELAST.0 + 3);
    /// `EPART`: bad partition.
    pub const EPART: Self = Self(Self::ELAST.0 + 4);
    /// `ERDLAB`: can't read disk label.
    pub const ERDLAB: Self = Self(Self::ELAST.0 + 5);
    /// `EOFFSET`: relative seek not supported.
    pub const EOFFSET: Self = Self(Self::ELAST.0 + 6);
    /// `EBSE`: bad sector error.
    pub const EBSE: Self = Self(Self::ELAST.0 + 7);
    /// `EECC`: uncorrectable ecc error.
    pub const EECC: Self = Self(Self::ELAST.0 + 8);
    /// `EHER`: hard error.
    pub const EHER: Self = Self(Self::ELAST.0 + 9);
    /// `ESALAST`: the last standalone error.
    pub const ESALAST: Self = Self(Self::ELAST.0 + 9);
}
/* </CODE> */
