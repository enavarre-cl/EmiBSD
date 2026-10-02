/*	$OpenBSD: sched.h,v 1.78 2026/03/31 16:46:21 deraadt Exp $	*/
/* $NetBSD: sched.h,v 1.2 1999/02/28 18:14:58 ross Exp $ */
/* <LICENSES> */
/*-
 * Copyright (c) 1999 The NetBSD Foundation, Inc.
 * All rights reserved.
 *
 * This code is derived from software contributed to The NetBSD Foundation
 * by Ross Harvey.
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
 *	@(#)kern_clock.c	8.5 (Berkeley) 1/21/94
 */
/* </LICENSES> */

//! `<sys/sched.h>`: the per-CPU scheduler state and the CPU-state statistics.
//!
//! Upstream: sys/sys/sched.h @ 3ce1f3f79392
//!
//! Status: `wip`. Milestone M5 (part a, the clocks) ports the `CP_*` states, `struct
//! cpustats`, the `SPCF_*` flags, `SCHED_NQS`/`SCHED_PPQ`/`NICE_WEIGHT`/`ESTCPULIM` and the
//! members of `struct schedstate_percpu` the clock interrupts use: the four clockintr
//! handles, `spc_cp_time` with its lock, `spc_schedticks`, `spc_schedflags`, `spc_nrun`,
//! `spc_whichqs`, `spc_spinning`, `spc_curpriority`, `spc_runtime`. The run queues
//! (`spc_qs`, `spc_idleproc`, `spc_deadproc`) need `struct proc` and the SMR members
//! (`spc_deferred` and company) `kern_smr.c`: part b. The functions are in
//! `kern/kern_sched.rs` and `kern/sched_bsd.rs`; `sched_lock` and the `SCHED_LOCK*` macros
//! in `sched_bsd.rs`.
//!
//! ## Deviations
//! - `spc_schedflags` (`volatile int`, set with `atomic_setbits_int`) is an `AtomicI32`;
//!   `spc_whichqs` and `spc_spinning` (`volatile`) `AtomicU32`s.

use core::cell::Cell;
use core::sync::atomic::{AtomicI32, AtomicU32};

use crate::sys::clockintr::Clockintr;
use crate::sys::pclock::PcLock;
use crate::sys::time::Timespec;

/*
 * CPU states.
 * XXX Not really scheduler state, but no other good place to put
 * it right now, and it really is per-CPU.
 */

/// `CP_USER`.
pub const CP_USER: usize = 0;
/// `CP_NICE`.
pub const CP_NICE: usize = 1;
/// `CP_SYS`.
pub const CP_SYS: usize = 2;
/// `CP_SPIN`.
pub const CP_SPIN: usize = 3;
/// `CP_INTR`.
pub const CP_INTR: usize = 4;
/// `CP_IDLE`.
pub const CP_IDLE: usize = 5;
/// `CPUSTATES`.
pub const CPUSTATES: usize = 6;

/// `struct cpustats`.
#[derive(Clone, Copy, Debug, Default)]
pub struct Cpustats {
    /// `cs_time`: CPU state statistics.
    pub cs_time: [u64; CPUSTATES],
    /// `cs_flags`: see below.
    pub cs_flags: u64,
}

/// `CPUSTATS_ONLINE`: CPU is schedulable.
pub const CPUSTATS_ONLINE: u64 = 0x0001;

/// `SCHED_NQS`: 32 run queues.
pub const SCHED_NQS: usize = 32;

/// `struct schedstate_percpu`: per-CPU scheduler state (the M5-a subset, see the module doc).
///
/// - o: owned (modified only) by this CPU.
pub struct SchedstatePercpu {
    // spc_idleproc, spc_qs[SCHED_NQS], spc_deadproc: struct proc (M5-b).
    /// `spc_runtime`: time curproc started running.
    pub spc_runtime: Cell<Timespec>,
    /// `spc_schedflags` (volatile): flags; see below.
    pub spc_schedflags: AtomicI32,
    /// `spc_schedticks`: ticks for `schedclock()`.
    pub spc_schedticks: Cell<u32>,
    /// `spc_cp_time_lock`.
    pub spc_cp_time_lock: PcLock,
    /// `spc_cp_time`: CPU state statistics.
    pub spc_cp_time: [Cell<u64>; CPUSTATES],

    /// \[o\] `spc_itimer`: `itimer_update` handle.
    pub spc_itimer: Clockintr,
    /// \[o\] `spc_profclock`: `profclock` handle.
    pub spc_profclock: Clockintr,
    /// \[o\] `spc_roundrobin`: `roundrobin` handle.
    pub spc_roundrobin: Clockintr,
    /// \[o\] `spc_statclock`: `statclock` handle.
    pub spc_statclock: Clockintr,

    /// `spc_nrun`: procs on the run queues.
    pub spc_nrun: Cell<u32>,

    /// `spc_whichqs` (volatile).
    pub spc_whichqs: AtomicU32,
    /// `spc_spinning` (volatile): this cpu is currently spinning.
    pub spc_spinning: AtomicU32,

    // spc_deferred, spc_ndeferred, spc_smrdepth, spc_smrexpedite, spc_smrgp: kern_smr.c.
    /// \[o\] `spc_curpriority` (volatile): usrpri of curproc.
    pub spc_curpriority: Cell<u8>,
}

// SAFETY: one CPU's scheduler state, touched by that CPU (the clock handles under their
// queue's mutex); the flag words other CPUs read are atomics.
unsafe impl Sync for SchedstatePercpu {}

impl SchedstatePercpu {
    /// A CPU's scheduler state before `sched_init_cpu`: all zero, as the C's static.
    pub const fn new() -> Self {
        Self {
            spc_runtime: Cell::new(Timespec::new(0, 0)),
            spc_schedflags: AtomicI32::new(0),
            spc_schedticks: Cell::new(0),
            spc_cp_time_lock: PcLock::new(),
            spc_cp_time: [const { Cell::new(0) }; CPUSTATES],
            spc_itimer: Clockintr::new(),
            spc_profclock: Clockintr::new(),
            spc_roundrobin: Clockintr::new(),
            spc_statclock: Clockintr::new(),
            spc_nrun: Cell::new(0),
            spc_whichqs: AtomicU32::new(0),
            spc_spinning: AtomicU32::new(0),
            spc_curpriority: Cell::new(0),
        }
    }
}

impl Default for SchedstatePercpu {
    fn default() -> Self {
        Self::new()
    }
}

/* spc_flags */

/// `SPCF_SEENRR`: process has seen `roundrobin()`.
pub const SPCF_SEENRR: i32 = 0x0001;
/// `SPCF_SHOULDYIELD`: process should yield the CPU.
pub const SPCF_SHOULDYIELD: i32 = 0x0002;
/// `SPCF_SWITCHCLEAR`.
pub const SPCF_SWITCHCLEAR: i32 = SPCF_SEENRR | SPCF_SHOULDYIELD;
/// `SPCF_SHOULDHALT`: CPU should be vacated.
pub const SPCF_SHOULDHALT: i32 = 0x0004;
/// `SPCF_HALTED`: CPU has been halted.
pub const SPCF_HALTED: i32 = 0x0008;
/// `SPCF_PROFCLOCK`: `profclock()` was started.
pub const SPCF_PROFCLOCK: i32 = 0x0010;
/// `SPCF_ITIMER`: `itimer_update()` was started.
pub const SPCF_ITIMER: i32 = 0x0020;

/// `SCHED_PPQ`: priorities per queue.
pub const SCHED_PPQ: u32 = 128 / SCHED_NQS as u32;
/// `NICE_WEIGHT`: priorities per nice level.
pub const NICE_WEIGHT: u32 = 2;
/// `PRIO_MAX` (`<sys/resource.h>`): the largest nice value.
const PRIO_MAX: u32 = 20;

/// `ESTCPULIM(e)`.
pub const fn estcpulim(e: u32) -> u32 {
    let lim = NICE_WEIGHT * PRIO_MAX - SCHED_PPQ;
    if e < lim { e } else { lim }
}

/// `CPUTYP_SMT`: SMT cpu.
pub const CPUTYP_SMT: i32 = 0x01;
/// `CPUTYP_P`: Performance core.
pub const CPUTYP_P: i32 = 0x02;
/// `CPUTYP_E`: Efficiency core.
pub const CPUTYP_E: i32 = 0x04;
/// `CPUTYP_L`: Lethargic, Low Power Efficiency core.
pub const CPUTYP_L: i32 = 0x08;

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn estcpulim_clamps() {
        assert_eq!(estcpulim(0), 0);
        assert_eq!(estcpulim(35), 35);
        assert_eq!(estcpulim(36), 36);
        assert_eq!(estcpulim(1000), 36);
    }
}
