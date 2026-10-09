/* <LICENSES> */
/*-
 * Copyright (c) 1990, 1993, 1994
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
 *	@(#)acct.h	8.3 (Berkeley) 7/10/94
 */
/* </LICENSES> */

/* <CODE> */
//! `<sys/acct.h>`: accounting structures. These use a `comp_t` type which is a 3 bits base 8
//! exponent, 13 bit fraction ``floating point'' number. Units are 1/`AHZ` seconds.
//!
//! Upstream: sys/sys/acct.h @ 3ce1f3f79392
//!
//! Status: `ported` (M5). `acct_process` and `acct_shutdown` are `kern/kern_acct.c` (M7).

use crate::sys::syslimits::_MAXCOMLEN;
use crate::sys::types::{Dev, Gid, Pid, Time, Uid};

/// `comp_t`.
pub type Comp = u16;

/// `struct acct`: one accounting record.
#[repr(C)]
#[derive(Clone, Copy, Debug)]
pub struct Acct {
    /// `ac_comm`: command name, incl NUL.
    pub ac_comm: [u8; _MAXCOMLEN],
    /// `ac_utime`: user time.
    pub ac_utime: Comp,
    /// `ac_stime`: system time.
    pub ac_stime: Comp,
    /// `ac_etime`: elapsed time.
    pub ac_etime: Comp,
    /// `ac_io`: count of IO blocks.
    pub ac_io: Comp,
    /// `ac_btime`: starting time.
    pub ac_btime: Time,
    /// `ac_uid`: user id.
    pub ac_uid: Uid,
    /// `ac_gid`: group id.
    pub ac_gid: Gid,
    /// `ac_mem`: average memory usage.
    pub ac_mem: u32,
    /// `ac_tty`: controlling tty, or -1.
    pub ac_tty: Dev,
    /// `ac_pid`: process id.
    pub ac_pid: Pid,
    /// `ac_flag`: accounting flags.
    pub ac_flag: u32,
}

/// `AFORK`: fork'd but not exec'd.
pub const AFORK: u32 = 0x0000_0001;
/// `AMAP`: killed by syscall or stack mapping violation.
pub const AMAP: u32 = 0x0000_0004;
/// `ACORE`: dumped core.
pub const ACORE: u32 = 0x0000_0008;
/// `AXSIG`: killed by a signal.
pub const AXSIG: u32 = 0x0000_0010;
/// `APLEDGE`: killed due to pledge violation.
pub const APLEDGE: u32 = 0x0000_0020;
/// `ATRAP`: memory access violation.
pub const ATRAP: u32 = 0x0000_0040;
/// `AUNVEIL`: unveil access violation.
pub const AUNVEIL: u32 = 0x0000_0080;
/// `APINSYS`: killed by syscall pin violation.
pub const APINSYS: u32 = 0x0000_0200;
/// `ABTCFI`: BT CFI violation.
pub const ABTCFI: u32 = 0x0000_0400;

/// `AHZ`: 1/AHZ is the granularity of the data encoded in the `comp_t` fields. This is not
/// necessarily equal to hz.
pub const AHZ: u32 = 64;
/* </CODE> */
