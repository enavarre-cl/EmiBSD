/*	$OpenBSD: fstat.c,v 1.4 2003/08/11 06:23:09 deraadt Exp $	*/
/*	$NetBSD: stat.c,v 1.3 1994/10/26 05:45:07 cgd Exp $	*/
/* <LICENSES> */
/*-
 * Copyright (c) 1993
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
 *	@(#)stat.c	8.1 (Berkeley) 6/11/93
 */
/* </LICENSES> */

/* <CODE> */
//! `fstat()`: the status of an open file.
//!
//! Upstream: sys/lib/libsa/fstat.c @ 3ce1f3f79392
//!
//! ## Deviations
//! - Failure is an `Err` (the C's -1). As in C, the file system's own `stat` error only goes
//!   to `errno`: `fstat` still succeeds.

use crate::dev::set_errno;
use crate::hdr::stat::Stat;
use crate::open::files;
use crate::saerrno::Errno;
use crate::stand::{F_RAW, SOPEN_MAX};

/// `fstat(fd, sb)`.
pub fn fstat(fd: usize, sb: &mut Stat) -> Result<(), Errno> {
    // SAFETY: entry point; the reference ends before this function returns and the stat
    // routines it calls do not reach FILES.
    let files = unsafe { files() };
    if fd >= SOPEN_MAX || files[fd].f_flags == 0 {
        set_errno(Errno::EBADF);
        return Err(Errno::EBADF);
    }
    let f = &mut files[fd];

    // operation not defined on raw devices
    if (f.f_flags & F_RAW) != 0 {
        set_errno(Errno::EOPNOTSUPP);
        return Err(Errno::EOPNOTSUPP);
    }

    let Some(ops) = f.f_ops else {
        set_errno(Errno::EBADF);
        return Err(Errno::EBADF);
    };
    set_errno((ops.stat)(f, sb).err().unwrap_or(Errno(0)));
    Ok(())
}
/* </CODE> */
