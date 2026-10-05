/*	$OpenBSD: lseek.c,v 1.7 2003/08/11 06:23:09 deraadt Exp $	*/
/*	$NetBSD: lseek.c,v 1.3 1996/06/21 20:09:03 pk Exp $	*/
/* <LICENSES> */
/*-
 * Copyright (c) 1993
 *	The Regents of the University of California.  All rights reserved.
 *
 * This code is derived from software contributed to Berkeley by
 * The Mach Operating System project at Carnegie-Mellon University.
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
 *	@(#)lseek.c	8.1 (Berkeley) 6/11/93
 *
 *
 * Copyright (c) 1989, 1990, 1991 Carnegie Mellon University
 * All Rights Reserved.
 *
 * Author: Alessandro Forin
 *
 * Permission to use, copy, modify and distribute this software and its
 * documentation is hereby granted, provided that both the copyright
 * notice and this permission notice appear in all copies of the
 * software, derivative works or modified versions, and any portions
 * thereof, and that both notices appear in supporting documentation.
 *
 * CARNEGIE MELLON ALLOWS FREE USE OF THIS SOFTWARE IN ITS "AS IS"
 * CONDITION.  CARNEGIE MELLON DISCLAIMS ANY LIABILITY OF ANY KIND FOR
 * ANY DAMAGES WHATSOEVER RESULTING FROM THE USE OF THIS SOFTWARE.
 *
 * Carnegie Mellon requests users of this software to return to
 *
 *  Software Distribution Coordinator  or  Software.Distribution@CS.CMU.EDU
 *  School of Computer Science
 *  Carnegie Mellon University
 *  Pittsburgh PA 15213-3890
 *
 * any improvements or extensions that they make and grant Carnegie the
 * rights to redistribute these changes.
 */
/* </LICENSES> */

//! `lseek()`: move the offset of a file, or of a raw device.
//!
//! Upstream: sys/lib/libsa/lseek.c @ 3ce1f3f79392
//!
//! Built with `__INTERNAL_LIBSA_CREAD`, as [`olseek`] (see `open.rs`).
//!
//! ## Deviations
//! - The new offset is an `Ok`, failure an `Err` (the C's -1), with `errno` set as in C.

use crate::dev::set_errno;
use crate::hdr::types::Off;
use crate::open::files;
use crate::saerrno::Errno;
use crate::stand::{F_RAW, SEEK_CUR, SEEK_SET, SOPEN_MAX};

/// `olseek(fd, offset, where)`.
pub fn olseek(fd: usize, offset: Off, whence: i32) -> Result<Off, Errno> {
    // SAFETY: entry point; the reference ends before this function returns and the seek
    // routines it calls do not reach FILES.
    let files = unsafe { files() };
    if fd >= SOPEN_MAX || files[fd].f_flags == 0 {
        set_errno(Errno::EBADF);
        return Err(Errno::EBADF);
    }
    let f = &mut files[fd];

    if (f.f_flags & F_RAW) != 0 {
        // On RAW devices, update internal offset.
        match whence {
            SEEK_SET => f.f_offset = offset,
            SEEK_CUR => f.f_offset += offset,
            _ => {
                set_errno(Errno::EOFFSET);
                return Err(Errno::EOFFSET);
            }
        }
        return Ok(f.f_offset);
    }

    let Some(ops) = f.f_ops else {
        set_errno(Errno::EBADF);
        return Err(Errno::EBADF);
    };
    (ops.seek)(f, offset, whence).inspect_err(|&e| set_errno(e))
}
