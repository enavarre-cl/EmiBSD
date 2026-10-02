/*	$OpenBSD: kern_time.c,v 1.171 2026/05/05 12:28:59 kettenis Exp $	*/
/*	$NetBSD: kern_time.c,v 1.20 1996/02/18 11:57:06 fvdl Exp $	*/
/* <LICENSES> */
/*
 * Copyright (c) 1982, 1986, 1989, 1993
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
 *	@(#)kern_time.c	8.4 (Berkeley) 5/26/95
 */
/* </LICENSES> */

//! Time-related system calls and the interval timers: `kern/kern_time.c`.
//!
//! Upstream: sys/kern/kern_time.c @ 3ce1f3f79392
//!
//! Status: `wip`. Milestone M5 (part a) ports `itimer_update` as far as it goes without a
//! current thread (its first test, `p == NULL`, is where every call ends until M5-b) and
//! `ratecheck`/`ppsratecheck`. `settime`, `clock_gettime`, the `sys_*` time calls, the
//! interval timer setters and `itimerdecr` arrive with `struct process` (part b) and the
//! syscalls (M6).

use core::ffi::c_void;

use crate::kern::kern_lock::{mtx_enter, mtx_leave};
use crate::kern::kern_tc::getmicrouptime;
use crate::machine::Machine;
use crate::machine::cpu::{Cpu, curcpu};
use crate::machine::intr::IPL_HIGH;
use crate::sys::clockintr::Clockrequest;
use crate::sys::mutex::Mutex;
use crate::sys::time::{Timeval, timersub};
use crate::unported;

/// `itimer_update`: the per-CPU clock interrupt that decrements the running process's
/// virtual and profiling interval timers.
pub fn itimer_update(_cr: &Clockrequest, _cf: *mut c_void, _arg: *mut c_void) {
    let p = Machine::ci_curproc(curcpu());

    if p.is_null() {
        return;
    }
    // ISSET(p->p_flag, P_SYSTEM | P_WEXIT), pr->ps_flags & PS_ITIMER, the decrement under
    // itimer_mtx: struct process (M5-b).
    let _ = unported!("itimer_update: process interval timers (M5-b)");
}

/// `ratecheck_mtx`.
static RATECHECK_MTX: Mutex = Mutex::new(IPL_HIGH);

/// `ratecheck()`: simple time-based rate-limit checking. see ratecheck(9) for usage and
/// rationale.
pub fn ratecheck(lasttime: &mut Timeval, mininterval: &Timeval) -> bool {
    let mut rv = false;

    let tv = getmicrouptime();

    mtx_enter(&RATECHECK_MTX);
    let delta = timersub(&tv, lasttime);

    // check for 0,0 is so that the message will be seen at least once, even if interval is
    // huge.
    if delta >= *mininterval || (lasttime.tv_sec == 0 && lasttime.tv_usec == 0) {
        *lasttime = tv;
        rv = true;
    }
    mtx_leave(&RATECHECK_MTX);

    rv
}

/// `ppsratecheck_mtx`.
static PPSRATECHECK_MTX: Mutex = Mutex::new(IPL_HIGH);

/// `ppsratecheck()`: packets (or events) per second limitation.
pub fn ppsratecheck(lasttime: &mut Timeval, curpps: &mut i32, maxpps: i32) -> bool {
    let tv = getmicrouptime();

    mtx_enter(&PPSRATECHECK_MTX);
    let delta = timersub(&tv, lasttime);

    // check for 0,0 is so that the message will be seen at least once. if more than one
    // second have passed since the last update of lasttime, reset the counter.
    //
    // we do increment *curpps even in *curpps < maxpps case, as some may try to use *curpps
    // for stat purposes as well.
    let rv = if (lasttime.tv_sec == 0 && lasttime.tv_usec == 0) || delta.tv_sec >= 1 {
        *lasttime = tv;
        *curpps = 0;
        true
    } else if maxpps < 0 {
        true
    } else {
        *curpps < maxpps
    };

    // the following should be done on 32-bit arithmetic, since the C code did it so
    // (rate-limit "maxpps" is an int).
    *curpps = curpps.saturating_add(1);
    mtx_leave(&PPSRATECHECK_MTX);

    rv
}
