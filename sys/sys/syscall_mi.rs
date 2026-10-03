/*	$OpenBSD: syscall_mi.h,v 1.37 2024/12/27 11:57:16 mpi Exp $	*/
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
 *	@(#)kern_xxx.c	8.2 (Berkeley) 11/14/93
 */
/* </LICENSES> */

//! `<sys/syscall_mi.h>`: the machine-independent part of a system call, around the
//! machine-dependent entry (`syscall()` on amd64, `svc_handler()` on arm64).
//!
//! Upstream: sys/sys/syscall_mi.h @ 3ce1f3f79392
//!
//! Status: `wip` (M6-a): `mi_syscall`, `mi_syscall_return`, `mi_child_return`, `mi_ast`
//! and `pin_check`.
//!
//! ## Deviations
//! - `pin_check` accepts every system call site: no process has a pin table until `exec`
//!   reads `PT_OPENBSD_SYSCALLS` (M7), and the C refuses a process without one. Reported
//!   once.
//! - `mi_syscall` reports the `MAP_STACK` check (`uvm_map_inentry`, with the user map,
//!   M6-b) and skips pledge (`PS_PLEDGE` is never set before `pledge(2)`, M7); `KTRACE`,
//!   `SYSCALL_DEBUG`, dt(4) and the kernel lock (`MULTIPROCESSOR`) are not configured.

use core::ffi::c_void;
use core::sync::atomic::Ordering;

use crate::kern::kern_sig::userret;
use crate::kern::sched_bsd::preempt;
use crate::sys::errno::Errno;
use crate::sys::proc::{P_OWEUPC, PS_PLEDGE, Proc, refreshcreds};
use crate::sys::systm::{SY_NOLOCK, Sysent};
use crate::sys::types::Register;
use crate::unported;

/// `pin_check`: check if a system call is entered from precisely correct location (see the
/// module's deviations).
#[inline]
fn pin_check(_p: &Proc, _code: Register) -> Result<(), Errno> {
    let _ =
        unported!("pin_check: pinsyscalls (PT_OPENBSD_SYSCALLS, M7); every call site is accepted");
    Ok(())
}

/// `mi_syscall`: the MD setup for a system call has been done; here's the MI part.
#[inline]
pub fn mi_syscall(
    p: &Proc,
    code: Register,
    callp: &Sysent,
    argp: *const c_void,
    retval: &mut [Register; 2],
) -> Result<(), Errno> {
    let _lock = callp.sy_flags & SY_NOLOCK == 0; // KERNEL_LOCK(): nothing without MULTIPROCESSOR

    // refresh the thread's cache of the process's creds
    refreshcreds(p);

    // SYSCALL_DEBUG, dt(4), KTRACE: not configured.

    // SP must be within MAP_STACK space: uvm_map_inentry(p, &p->p_spinentry, PROC_STACK(p),
    // ...) with the user map (M6-b).
    let _ = unported!("mi_syscall: uvm_map_inentry (the MAP_STACK check, M6-b)");

    pin_check(p, code)?;

    if p.process().ps_flags.load(Ordering::Relaxed) & PS_PLEDGE != 0 {
        // pledge_syscall / pledge_fail: kern_pledge.c (M7).
        let _ = unported!("mi_syscall: pledge_syscall (kern_pledge.c, M7)");
    }

    (callp.sy_call)(p, argp, retval)
}

/// `mi_syscall_return`: finish MI stuff on return, after the registers have been set.
#[inline]
pub fn mi_syscall_return(
    p: &Proc,
    _code: Register,
    _error: Result<(), Errno>,
    _retval: &[Register; 2],
) {
    // SYSCALL_DEBUG, dt(4), KTRACE: not configured.
    userret(p);
}

/// `mi_child_return`: finish MI stuff for a new process/thread to return.
#[inline]
pub fn mi_child_return(p: &Proc) {
    // TRACEPOINT(sched, on__cpu, NULL), SYSCALL_DEBUG, dt(4), KTRACE: not configured.
    userret(p);
}

/// `mi_ast`: do the specific processing necessary for an AST.
#[inline]
pub fn mi_ast(p: &Proc, resched: bool) {
    if p.p_flag.load(Ordering::Relaxed) & P_OWEUPC != 0 {
        // ADDUPROF(p): the profiling clock (subr_prof.c, M7).
        let _ = unported!("mi_ast: ADDUPROF (M7)");
    }
    if resched {
        preempt();
    }

    // XXX could move call to userret() here, but hppa calls ast() in syscall return and sh
    // calls it after userret()
}
