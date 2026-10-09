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
 *	@(#)dev.c	8.1 (Berkeley) 6/11/93
 */
/* </LICENSES> */

/* <CODE> */
//! `errno` and the do-nothing device routines.
//!
//! Upstream: sys/lib/libsa/dev.c @ 3ce1f3f79392
//!
//! ## Deviations
//! - `int errno` is the atomic [`ERRNO`], read and written through [`errno`] and
//!   [`set_errno`]; libsa's functions also return the error they set.

use core::ffi::c_void;
use core::sync::atomic::{AtomicI32, Ordering};

use crate::saerrno::Errno;
use crate::stand::OpenFile;

/// `errno`: the last error a libsa routine reported.
pub static ERRNO: AtomicI32 = AtomicI32::new(0);

/// The value of `errno`.
pub fn errno() -> Errno {
    Errno(ERRNO.load(Ordering::Relaxed))
}

/// `errno = e`.
pub fn set_errno(e: Errno) {
    ERRNO.store(e.0, Ordering::Relaxed);
}

/// `nodev()`: the routine of a device that is not there.
pub fn nodev() -> Result<(), Errno> {
    Err(Errno::ENXIO)
}

/// `nullsys()`: does nothing.
pub fn nullsys() {}

/// `noioctl()`: a device without ioctls.
pub fn noioctl(_f: &mut OpenFile, _cmd: u64, _data: *mut c_void) -> Result<(), Errno> {
    Err(Errno::EINVAL)
}
/* </CODE> */
