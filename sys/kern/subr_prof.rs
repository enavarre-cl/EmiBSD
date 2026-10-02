/*	$OpenBSD: subr_prof.c,v 1.44 2026/09/25 04:55:43 gnezdo Exp $	*/
/*	$NetBSD: subr_prof.c,v 1.12 1996/04/22 01:38:50 christos Exp $	*/
/* <LICENSES> */
/*-
 * Copyright (c) 1982, 1986, 1993
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
 *	@(#)subr_prof.c	8.3 (Berkeley) 9/23/93
 */
/* </LICENSES> */

//! Profiling support: `kern/subr_prof.c`.
//!
//! Upstream: sys/kern/subr_prof.c @ 3ce1f3f79392
//!
//! Status: `wip`. Milestone M5 (part a) ports `profclock_period` and `profclock`, the
//! clock interrupt handle `sched_init_cpu` binds; `GPROF`/`DDBPROF` are not configured.
//! `sys_profil`, `addupc_intr`/`addupc_task` and the rest arrive with `struct process`
//! (part b) and the syscalls (M6).

use core::ffi::c_void;
use core::sync::atomic::{AtomicU64, Ordering};

use crate::kern::kern_clockintr::clockrequest_advance;
use crate::machine::Machine;
use crate::machine::cpu::{ClockFrame, Cpu, curcpu};
use crate::sys::clockintr::Clockrequest;
use crate::unported;

/// `profclock_period`.
pub static PROFCLOCK_PERIOD: AtomicU64 = AtomicU64::new(0);

/// `profclock_period`.
pub fn profclock_period() -> u64 {
    PROFCLOCK_PERIOD.load(Ordering::Relaxed)
}

/// `profclock`: the profiling clock interrupt.
pub fn profclock(cr: &Clockrequest, cf: *mut c_void, _arg: *mut c_void) {
    let p = Machine::ci_curproc(curcpu());

    // if (count > ULONG_MAX) count = ULONG_MAX: a no-op on LP64.
    let _count = clockrequest_advance(cr, profclock_period());

    // SAFETY: the dispatcher passes the clock frame the interrupt entry built.
    let frame = unsafe { cf.cast::<ClockFrame>().as_ref() };
    if frame.is_some_and(Machine::clkf_usermode) || !p.is_null() {
        // ISSET(p->p_p->ps_flags, PS_PROFIL): addupc_intr(p, CLKF_PC(frame) / PROC_PC(p),
        // count): struct process (M5-b).
        let _ = unported!("profclock: addupc_intr (struct process, M5-b)");
    }
}
