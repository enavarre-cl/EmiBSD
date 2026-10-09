/* <LICENSES> */
/*-
 * Copyright (c) 1982, 1986, 1991, 1993
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
 *	@(#)tty_conf.c	8.4 (Berkeley) 1/21/94
 */
/* </LICENSES> */

/* <CODE> */
//! The line discipline switch: `kern/tty_conf.c`.
//!
//! Upstream: sys/kern/tty_conf.c @ 3ce1f3f79392
//!
//! ## Deviations
//! - `pseudo-device ppp`, `nmea`, `msts` and `endrun` are not configured (their line
//!   disciplines are not ported), so slots 5, 7, 8 and 9 hold the defunct entries the C
//!   uses when `NPPP`, `NNMEA`, `NMSTS` or `NENDRUN` is 0.
//! - `ttynodisc`, `ttyerrclose`, `ttyerrio`, `ttyerrinput` and `ttyerrstart` are the C's
//!   casts of `enodev`: small functions of each type here. `l_rint` and `l_start` return the
//!   errno as an `int`, as the cast does.
//! - `nullioctl` returns `Ok(false)` for the C's `-1` (`docs/C_TO_RUST.md`).

use crate::kern::subr_xxx::enodev;
use crate::kern::tty::{
    nullmodem, ttread, ttstart, ttwrite, ttyinput, ttylclose, ttymodem, ttyopen,
};
use crate::sys::conf::Linesw;
use crate::sys::errno::Errno;
use crate::sys::proc::Proc;
use crate::sys::tty::Tty;
use crate::sys::types::Dev;
use crate::sys::uio::Uio;

/// `ttynodisc`: `enodev` as an `l_open`.
fn ttynodisc(_dev: Dev, _tp: &Tty, _p: &Proc) -> Result<(), Errno> {
    enodev()
}

/// `ttyerrclose`: `enodev` as an `l_close`.
fn ttyerrclose(_tp: &Tty, _flags: i32, _p: Option<&Proc>) -> Result<(), Errno> {
    enodev()
}

/// `ttyerrio`: `enodev` as an `l_read`/`l_write`.
fn ttyerrio(_tp: &Tty, _uio: &mut Uio<'_>, _flag: i32) -> Result<(), Errno> {
    enodev()
}

/// `ttyerrinput`: `enodev` as an `l_rint`.
fn ttyerrinput(_c: i32, _tp: &Tty) -> i32 {
    Errno::ENODEV as i32
}

/// `ttyerrstart`: `enodev` as an `l_start`.
fn ttyerrstart(_tp: &Tty) -> i32 {
    Errno::ENODEV as i32
}

/// A defunct (or not configured) line discipline.
const DEFUNCT: Linesw = Linesw {
    l_open: ttynodisc,
    l_close: ttyerrclose,
    l_read: ttyerrio,
    l_write: ttyerrio,
    l_ioctl: nullioctl,
    l_rint: ttyerrinput,
    l_start: ttyerrstart,
    l_modem: nullmodem,
};

/// `linesw[]`: the line disciplines, indexed by `t_line` (`TTYDISC`, ...).
pub static LINESW: [Linesw; 10] = [
    // 0- termios
    Linesw {
        l_open: ttyopen,
        l_close: ttylclose,
        l_read: ttread,
        l_write: ttwrite,
        l_ioctl: nullioctl,
        l_rint: ttyinput,
        l_start: ttstart,
        l_modem: ttymodem,
    },
    // 1- defunct
    DEFUNCT,
    // 2- old NTTYDISC (defunct)
    DEFUNCT,
    // 3- TABLDISC (defunct)
    DEFUNCT,
    // 4- SLIPDISC (defunct)
    DEFUNCT,
    // 5- PPPDISC: NPPP == 0
    DEFUNCT,
    // 6- STRIPDISC (defunct)
    DEFUNCT,
    // 7- NMEADISC: NNMEA == 0
    DEFUNCT,
    // 8- MSTSDISC: NMSTS == 0
    DEFUNCT,
    // 9- ENDRUNDISC: NENDRUN == 0
    DEFUNCT,
];

/// `nlinesw`.
pub const NLINESW: usize = LINESW.len();

/// `linesw[tp->t_line]`.
pub fn linesw(tp: &Tty) -> &'static Linesw {
    LINESW.get(usize::from(tp.t_line.get())).unwrap_or(&DEFUNCT)
}

/// `nullioctl`: do nothing specific version of line discipline specific ioctl command.
pub fn nullioctl(
    _tp: &Tty,
    _cmd: u64,
    _data: &mut [u8],
    _flags: i32,
    _p: &Proc,
) -> Result<bool, Errno> {
    Ok(false)
}
/* </CODE> */
