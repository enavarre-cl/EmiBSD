/*	$OpenBSD: kern_sig.c,v 1.366 2026/08/23 17:06:56 daniel Exp $	*/
/*	$NetBSD: kern_sig.c,v 1.54 1996/04/22 01:38:32 christos Exp $	*/
/* <LICENSES> */
/*
 * Copyright (c) 1997 Theo de Raadt. All rights reserved.
 * Copyright (c) 1982, 1986, 1989, 1991, 1993
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
 *	@(#)kern_sig.c	8.7 (Berkeley) 4/18/94
 */
/* </LICENSES> */

//! Signals: `kern/kern_sig.c`.
//!
//! Upstream: sys/kern/kern_sig.c @ 3ce1f3f79392
//!
//! Status: `wip`. Milestone M6 (part a) ports what the system call path needs before
//! signals exist: `sys_nosys` and `userret`. The signal machinery (`signal_init`,
//! `sigactsinit`, `sys_sigaction`, `psignal`/`ptsignal`, `cursig`, `postsig`, `sigexit`,
//! `trapsignal`, `proc_suspend_check`, the stop/single-thread logic, core dumps) comes with
//! M6-b/M7.
//!
//! ## Deviations
//! - `sys_nosys` cannot `ptsignal(SIGSYS)` yet: it reports the gap and returns `ENOSYS`.
//! - `userret` reports instead of delivering when a signal or suspension is pending; none can
//!   be before the signal code lands.

use core::sync::atomic::Ordering;

use crate::machine::Machine;
use crate::machine::cpu::Cpu;
use crate::sys::errno::Errno;
use crate::sys::proc::{P_ALRMPEND, P_PROFPEND, P_SIGSUSPEND, P_SUSPSIG, P_SUSPSINGLE, Proc};
use crate::sys::systm::SysArgs;
use crate::sys::types::Register;
use crate::unported;

/// `sys_nosys`: nonexistent system call-- signal process (may want to handle it). Flag error
/// in case process won't see signal immediately (blocked or ignored).
pub fn sys_nosys(_p: &Proc, _v: &SysArgs, _retval: &mut [Register; 2]) -> Result<(), Errno> {
    // ptsignal(p, SIGSYS, STHREAD): the signals (M6-b).
    let _ = unported!("sys_nosys: ptsignal(SIGSYS) (M6-b)");
    Err(Errno::ENOSYS)
}

/// `userret`: the last thing before a thread returns to user mode: pending suspensions and
/// signals, then the CPU's current priority.
pub fn userret(p: &Proc) {
    if p.p_flag.load(Ordering::Relaxed) & (P_SUSPSINGLE | P_SUSPSIG) != 0 {
        let _ = unported!("userret: proc_suspend_check (M6-b)");
    }

    // send SIGPROF or SIGVTALRM if their timers interrupted this thread
    if p.p_flag.load(Ordering::Relaxed) & P_PROFPEND != 0 {
        p.p_flag.fetch_and(!P_PROFPEND, Ordering::Relaxed);
        let _ = unported!("userret: psignal(SIGPROF) (M6-b)");
    }
    if p.p_flag.load(Ordering::Relaxed) & P_ALRMPEND != 0 {
        p.p_flag.fetch_and(!P_ALRMPEND, Ordering::Relaxed);
        let _ = unported!("userret: psignal(SIGVTALRM) (M6-b)");
    }

    // SIGPENDING(p) != 0: while ((signum = cursig(p, &ctx, 0)) != 0) postsig(p, signum, &ctx)
    if p.p_siglist.load(Ordering::Relaxed) != 0
        || p.process().ps_siglist.load(Ordering::Relaxed) != 0
    {
        let _ = unported!("userret: cursig/postsig (M6-b)");
    }

    // If P_SIGSUSPEND is still set here, then we still need to restore the original sigmask
    // before returning to userspace. Also, this might unmask some pending signals, so we
    // need to check a second time for signals to post.
    if p.p_flag.load(Ordering::Relaxed) & P_SIGSUSPEND != 0 {
        p.p_sigmask.set(p.p_oldmask.get());
        p.p_flag.fetch_and(!P_SIGSUSPEND, Ordering::Relaxed);
        let _ = unported!("userret: cursig/postsig after sigsuspend (M6-b)");
    }

    // WITNESS_WARN(WARN_PANIC, NULL, "userret: returning"): not configured.

    if let Some(ci) = p.cpu() {
        Machine::ci_schedstate(ci)
            .spc_curpriority
            .set(p.p_usrpri.get());
    }
}
