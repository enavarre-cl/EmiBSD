/*	$OpenBSD: open.c,v 1.11 2016/03/14 23:08:06 krw Exp $	*/
/*	$NetBSD: open.c,v 1.12 1996/09/30 16:01:21 ws Exp $	*/
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
 *	@(#)open.c	8.1 (Berkeley) 6/11/93
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

//! The open file table and `open()`, which finds the device and then the file system.
//!
//! Upstream: sys/lib/libsa/open.c @ 3ce1f3f79392
//!
//! libsa is built here as efiboot builds it, with `__INTERNAL_LIBSA_CREAD`: this `open` is
//! [`oopen`], and `cread.rs`'s `open` wraps it with gzip decompression.
//!
//! ## Deviations
//! - `files[]` is the [`StaticCell`] [`FILES`]; [`files`] hands it out to libsa's entry
//!   points, one at a time.
//! - A descriptor is a `usize` index and failure an `Err` (the C's -1 with `errno` set; the
//!   error is also stored in `errno`, which boot(8) prints). The file name is a byte slice
//!   the file systems copy what they need from, where the C passes a pointer they write NULs
//!   into and restore.

use libkern::staticcell::StaticCell;

use crate::dev::set_errno;
use crate::saerrno::Errno;
use crate::stand::{F_NODEV, F_RAW, OpenFile, SOPEN_MAX, sa_conf};

/// `files[SOPEN_MAX]`: the open files.
pub static FILES: StaticCell<[OpenFile; SOPEN_MAX]> =
    StaticCell::new([const { OpenFile::new() }; SOPEN_MAX]);

/// The open file table.
///
/// # Safety
///
/// No other reference into [`FILES`] may be live. libsa's entry points take it once and
/// none of them keeps it across a call into another entry point; the standalone programs
/// are single-threaded.
pub unsafe fn files() -> &'static mut [OpenFile; SOPEN_MAX] {
    // SAFETY: the caller excludes every other reference (the function's contract).
    unsafe { FILES.get_mut() }
}

/// `oopen(fname, mode)`: open `fname` (`dev:path`, as the program's `devopen` parses it)
/// with mode 0, 1 or 2 (read, write, both); the descriptor.
pub fn oopen(fname: &[u8], mode: i32) -> Result<usize, Errno> {
    // SAFETY: entry point; the reference ends before this function returns and nothing it
    // calls (devopen, the file systems' open, dv_close) reaches FILES.
    let files = unsafe { files() };

    // find a free file descriptor
    let Some(fd) = files.iter().position(|f| f.f_flags == 0) else {
        set_errno(Errno::EMFILE);
        return Err(Errno::EMFILE);
    };
    let f = &mut files[fd];

    // Try to open the device; convert open mode (0,1,2) to F_READ, F_WRITE.
    f.f_flags = mode + 1;
    f.f_dev = None;
    f.f_ops = None;
    let conf = sa_conf();
    let file = match (conf.devopen)(f, fname) {
        Ok(file) if (f.f_flags & F_NODEV) != 0 || f.f_dev.is_some() => file,
        Ok(_) => return Err(open_failed(f, Errno(0))),
        Err(e) => return Err(open_failed(f, e)),
    };

    // see if we opened a raw device; otherwise, 'file' is the file name.
    if file.is_empty() || file[0] == 0 {
        f.f_flags |= F_RAW;
        return Ok(fd);
    }

    // pass file name to the different filesystem open routines
    let mut error = Errno(0);
    for fs in conf.file_system {
        match (fs.open)(file, f) {
            Ok(()) => {
                f.f_ops = Some(fs);
                return Ok(fd);
            }
            Err(e) => {
                error = e;
                if e == Errno::ENOENT || e == Errno::ENOTDIR {
                    break;
                }
            }
        }
    }
    if error == Errno(0) {
        error = Errno::ENOENT;
    }

    if let Some(dv) = f.f_dev {
        let _ = (dv.dv_close)(f);
    }
    Err(open_failed(f, error))
}

/// The C's `err:` label: free the slot and set `errno`.
fn open_failed(f: &mut OpenFile, error: Errno) -> Errno {
    f.f_flags = 0;
    set_errno(error);
    error
}
