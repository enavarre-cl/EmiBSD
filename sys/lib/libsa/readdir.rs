/*	$OpenBSD: readdir.c,v 1.10 2022/01/11 06:35:03 visa Exp $	*/
/* <LICENSES> */
/*
 * Copyright (c) 1996 Michael Shalayeff
 * All rights reserved.
 *
 * Redistribution and use in source and binary forms, with or without
 * modification, are permitted provided that the following conditions
 * are met:
 * 1. Redistributions of source code must retain the above copyright
 *    notice, this list of conditions and the following disclaimer.
 * 2. Redistributions in binary form must reproduce the above copyright
 *    notice, this list of conditions and the following disclaimer in the
 *    documentation and/or other materials provided with the distribution.
 *
 * THIS SOFTWARE IS PROVIDED BY THE AUTHOR ``AS IS'' AND ANY EXPRESS OR
 * IMPLIED WARRANTIES, INCLUDING, BUT NOT LIMITED TO, THE IMPLIED
 * WARRANTIES OF MERCHANTABILITY AND FITNESS FOR A PARTICULAR PURPOSE
 * ARE DISCLAIMED.  IN NO EVENT SHALL THE REGENTS OR CONTRIBUTORS BE LIABLE
 * FOR ANY DIRECT, INDIRECT, INCIDENTAL, SPECIAL, EXEMPLARY, OR CONSEQUENTIAL
 * DAMAGES (INCLUDING, BUT NOT LIMITED TO, PROCUREMENT OF SUBSTITUTE GOODS
 * OR SERVICES; LOSS OF USE, DATA, OR PROFITS; OR BUSINESS INTERRUPTION)
 * HOWEVER CAUSED AND ON ANY THEORY OF LIABILITY, WHETHER IN CONTRACT, STRICT
 * LIABILITY, OR TORT (INCLUDING NEGLIGENCE OR OTHERWISE) ARISING IN ANY WAY
 * OUT OF THE USE OF THIS SOFTWARE, EVEN IF ADVISED OF THE POSSIBILITY OF
 * SUCH DAMAGE.
 *
 */
/* </LICENSES> */

//! `opendir()`, `readdir()`, `closedir()`: the names in a directory, for boot(8)'s `ls`.
//!
//! Upstream: sys/lib/libsa/readdir.c @ 3ce1f3f79392
//!
//! Built with `__INTERNAL_LIBSA_CREAD`: the directory is opened, rewound and closed with
//! [`oopen`], [`olseek`] and [`oclose`].
//!
//! ## Deviations
//! - Failure is an `Err` (the C's -1), with `errno` set as in C; the end of the directory is
//!   the file system's own error (`ufs_readdir` returns -1).

use crate::close::oclose;
use crate::dev::set_errno;
use crate::hdr::stat::{Stat, s_isdir};
use crate::lseek::olseek;
use crate::open::{files, oopen};
use crate::saerrno::Errno;
use crate::stand::{F_RAW, F_READ, SEEK_SET, SOPEN_MAX};
use crate::stat::stat;

/// `opendir(name)`: a descriptor on the directory `name`.
pub fn opendir(name: &[u8]) -> Result<usize, Errno> {
    let mut sb = Stat::default();
    stat(name, &mut sb)?;

    if !s_isdir(sb.st_mode) {
        set_errno(Errno::ENOTDIR);
        return Err(Errno::ENOTDIR);
    }

    // XXX rewind needed for some dirs
    let fd = oopen(name, 0)?;
    let _ = olseek(fd, 0, SEEK_SET);
    Ok(fd)
}

/// `readdir(fd, dest)`: the next name, NUL-terminated, into `dest`.
pub fn readdir(fd: usize, dest: &mut [u8]) -> Result<(), Errno> {
    if fd >= SOPEN_MAX {
        set_errno(Errno::EBADF);
        return Err(Errno::EBADF);
    }
    // SAFETY: entry point; the reference ends before this function returns and the readdir
    // routines it calls do not reach FILES.
    let f = unsafe { &mut files()[fd] };
    if (f.f_flags & F_READ) == 0 {
        set_errno(Errno::EBADF);
        return Err(Errno::EBADF);
    }
    if (f.f_flags & F_RAW) != 0 {
        set_errno(Errno::EOPNOTSUPP);
        return Err(Errno::EOPNOTSUPP);
    }
    let Some(ops) = f.f_ops else {
        set_errno(Errno::EBADF);
        return Err(Errno::EBADF);
    };
    let res = (ops.readdir)(f, Some(dest));
    set_errno(res.err().unwrap_or(Errno(0)));
    res
}

/// `closedir(fd)`.
pub fn closedir(fd: usize) {
    let _ = oclose(fd);
}
