/*	$OpenBSD: sched_bsd.c,v 1.105 2025/09/25 08:46:50 mvs Exp $	*/
/*	$NetBSD: kern_synch.c,v 1.37 1996/04/22 01:38:37 christos Exp $	*/
/* <LICENSES> */
/*-
 * Copyright (c) 1982, 1986, 1990, 1991, 1993
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
 *	@(#)kern_synch.c	8.6 (Berkeley) 1/21/94
 */
/* </LICENSES> */

//! The 4.4BSD scheduler: `kern/sched_bsd.c`.
//!
//! Upstream: sys/kern/sched_bsd.c @ 3ce1f3f79392
//!
//! Status: `wip`. Milestone M5 (part a) ports `roundrobin_period`, `sched_lock` with
//! `SCHED_LOCK_INIT` and `roundrobin`, the clock interrupt that `clockintr_cpu_init`
//! schedules. The load average (`update_loadavg`, `averunnable`, `cexp`), `schedcpu`,
//! `decay_aftersleep`, `yield`, `mi_switch`, `setrunnable`, `schedclock` and the rest arrive
//! with `struct proc` (part b).

use core::sync::atomic::{AtomicU64, Ordering};

use crate::kern::kern_clockintr::clockrequest_advance;
use crate::kern::kern_lock::mtx_init;
use crate::machine::Machine;
use crate::machine::cpu::{Cpu, curcpu, need_resched};
use crate::machine::intr::{IPL_NONE, IPL_SCHED};
use crate::sys::clockintr::Clockrequest;
use crate::sys::mutex::Mutex;
use crate::sys::sched::{SPCF_SEENRR, SPCF_SHOULDYIELD};

/// \[I\] `roundrobin_period`: roundrobin period (ns).
pub static ROUNDROBIN_PERIOD: AtomicU64 = AtomicU64::new(0);

/// `sched_lock`: initialised by `SCHED_LOCK_INIT()` in `main`.
pub static SCHED_LOCK: Mutex = Mutex::new(IPL_NONE);

/// `roundrobin_period`.
pub fn roundrobin_period() -> u64 {
    ROUNDROBIN_PERIOD.load(Ordering::Relaxed)
}

/// `SCHED_LOCK_INIT()`.
pub fn sched_lock_init() {
    mtx_init(&SCHED_LOCK, IPL_SCHED);
}

/// `roundrobin`: force switch among equal priority processes every 100ms.
pub fn roundrobin(cr: &Clockrequest, _cf: *mut core::ffi::c_void, _arg: *mut core::ffi::c_void) {
    let ci = curcpu();
    let spc = Machine::ci_schedstate(ci);

    let count = clockrequest_advance(cr, roundrobin_period());

    if !Machine::ci_curproc(ci).is_null() {
        if spc.spc_schedflags.load(Ordering::Relaxed) & SPCF_SEENRR != 0 || count >= 2 {
            // The process has already been through a roundrobin without switching and may
            // be hogging the CPU. Indicate that the process should yield.
            spc.spc_schedflags
                .fetch_or(SPCF_SEENRR | SPCF_SHOULDYIELD, Ordering::Relaxed);
        } else {
            spc.spc_schedflags.fetch_or(SPCF_SEENRR, Ordering::Relaxed);
        }
    }

    if spc.spc_nrun.get() != 0 || spc.spc_schedflags.load(Ordering::Relaxed) & SPCF_SHOULDYIELD != 0
    {
        need_resched(ci);
    }
}
