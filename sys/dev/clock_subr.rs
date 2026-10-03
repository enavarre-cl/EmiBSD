/*	$OpenBSD: clock_subr.h,v 1.8 2022/10/12 13:39:50 kettenis Exp $	*/
/*	$NetBSD: clock_subr.h,v 1.2 1997/03/15 18:11:17 is Exp $	*/
/* <LICENSES> */
/*-
 * Copyright (c) 1996 The NetBSD Foundation, Inc.
 * All rights reserved.
 *
 * This code is derived from software contributed to The NetBSD Foundation
 * by Gordon W. Ross
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
 * THIS SOFTWARE IS PROVIDED BY THE NETBSD FOUNDATION, INC. AND CONTRIBUTORS
 * ``AS IS'' AND ANY EXPRESS OR IMPLIED WARRANTIES, INCLUDING, BUT NOT LIMITED
 * TO, THE IMPLIED WARRANTIES OF MERCHANTABILITY AND FITNESS FOR A PARTICULAR
 * PURPOSE ARE DISCLAIMED.  IN NO EVENT SHALL THE FOUNDATION OR CONTRIBUTORS
 * BE LIABLE FOR ANY DIRECT, INDIRECT, INCIDENTAL, SPECIAL, EXEMPLARY, OR
 * CONSEQUENTIAL DAMAGES (INCLUDING, BUT NOT LIMITED TO, PROCUREMENT OF
 * SUBSTITUTE GOODS OR SERVICES; LOSS OF USE, DATA, OR PROFITS; OR BUSINESS
 * INTERRUPTION) HOWEVER CAUSED AND ON ANY THEORY OF LIABILITY, WHETHER IN
 * CONTRACT, STRICT LIABILITY, OR TORT (INCLUDING NEGLIGENCE OR OTHERWISE)
 * ARISING IN ANY WAY OUT OF THE USE OF THIS SOFTWARE, EVEN IF ADVISED OF THE
 * POSSIBILITY OF SUCH DAMAGE.
 */
/* </LICENSES> */

//! `<dev/clock_subr.h>`: the time-of-day clock chip interface.
//!
//! Upstream: sys/dev/clock_subr.h @ 3ce1f3f79392
//!
//! Status: `ported`. `struct todr_chip_handle` and the `todr_gettime`/`todr_settime`/
//! `todr_wenable` macros; `todr_attach` is `kern_time.c`'s. No driver attaches a chip yet
//! (the mc146818 on amd64, the PL031 on arm64 are not ported).

use core::ffi::c_void;

use crate::sys::errno::Errno;
use crate::sys::time::Timeval;

/// `todr_gettime`/`todr_settime`: read or set the chip's time.
pub type TodrTimeFn = fn(&TodrChipHandle, &mut Timeval) -> Result<(), Errno>;

/// `todr_setwen`: enable or disable writes to the chip.
pub type TodrSetwenFn = fn(&TodrChipHandle, i32) -> Result<(), Errno>;

/// `struct todr_chip_handle`: a time-of-day clock chip.
pub struct TodrChipHandle {
    /// `cookie`: device specific data.
    pub cookie: *mut c_void,
    /// `bus_cookie`: bus specific data.
    pub bus_cookie: *mut c_void,
    /// `todr_quality`: the better chip wins `todr_attach`.
    pub todr_quality: i32,
    /// `todr_gettime`: convert time-of-day clock into a `struct timeval`.
    pub todr_gettime: TodrTimeFn,
    /// `todr_settime`: set time-of-day clock from a `struct timeval`.
    pub todr_settime: TodrTimeFn,
    /// `todr_setwen`: write enable (NULL when the chip needs none).
    pub todr_setwen: Option<TodrSetwenFn>,
}

// SAFETY: a chip handle is set up once by its driver and then only read; the cookies are
// the driver's, used by its own functions under its own locking.
unsafe impl Sync for TodrChipHandle {}

/// `todr_gettime(ct, t)`.
pub fn todr_gettime(ct: &TodrChipHandle, t: &mut Timeval) -> Result<(), Errno> {
    (ct.todr_gettime)(ct, t)
}

/// `todr_settime(ct, t)`.
pub fn todr_settime(ct: &TodrChipHandle, t: &mut Timeval) -> Result<(), Errno> {
    (ct.todr_settime)(ct, t)
}

/// `todr_wenable(ct, v)`.
pub fn todr_wenable(ct: &TodrChipHandle, v: i32) {
    if let Some(setwen) = ct.todr_setwen {
        let _ = setwen(ct, v);
    }
}
