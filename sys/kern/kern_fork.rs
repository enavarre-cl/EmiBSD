/*	$OpenBSD: kern_fork.c,v 1.272 2026/04/04 08:46:30 jsg Exp $	*/
/*	$NetBSD: kern_fork.c,v 1.29 1996/02/09 18:59:34 christos Exp $	*/
/* <LICENSES> */
/*
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
 *	@(#)kern_fork.c	8.6 (Berkeley) 4/8/94
 */
/* </LICENSES> */

//! Creating processes and threads: `kern/kern_fork.c`.
//!
//! Upstream: sys/kern/kern_fork.c @ 3ce1f3f79392
//!
//! Status: `wip`. Milestone M5 (part b1) ports `nprocesses`/`nthreads`,
//! `process_initialize`, `fork_check_maxthread`, `alloctid`, `allocpid`, `ispidtaken` and
//! `freepid`; `thread_new`, `process_new`, `fork1` (kernel threads), `fork_thread_start` and
//! `proc_trampoline_mi` come with part b2, `sys_fork`/`sys_vfork`/`sys___tfork`,
//! `thread_fork` and `fork_return` with the syscalls (M6).
//!
//! ## Deviations
//! - `process_initialize` reports what its process does not have yet: `crhold` (M6),
//!   `prof_fork` (M6), `rw_init(ps_lock)` (M5-b2), `klist_init_mutex` (M6) and the two
//!   timeouts' handlers (`realitexpire`, `rucheck`: M6).

use core::sync::atomic::{AtomicI32, AtomicU32, Ordering};

use libkern::StaticCell;

use crate::conf::param::MAXTHREAD;
use crate::dev::rnd::{arc4random, arc4random_uniform};
use crate::kern::kern_lock::mtx_init;
use crate::kern::kern_proc::{pgfind, prfind, tfind, zombiefind};
use crate::kern::kern_synch::refcnt_init;
use crate::kern::kern_time::ratecheck;
use crate::kern::subr_prf::printf;
use crate::machine::intr::IPL_HIGH;
use crate::sys::errno::Errno;
use crate::sys::proc::{PID_MAX, Proc, Process, TID_MASK};
use crate::sys::time::Timeval;
use crate::sys::types::{Pid, Uid};
use crate::unported;

/// `nprocesses`: process 0.
pub static NPROCESSES: AtomicI32 = AtomicI32::new(1);
/// \[a\] `nthreads`: proc 0.
pub static NTHREADS: AtomicI32 = AtomicI32::new(1);

// forkstat: struct forkstat (sys/vmmeter.h), with fork1 (M5-b2).

/// `process_initialize`: initialize common bits of a process structure, given the initial
/// thread.
pub fn process_initialize(pr: &'static Process, p: &'static Proc) {
    refcnt_init(&pr.ps_refcnt);

    // initialize the thread links
    pr.ps_mainproc.set(p);
    pr.ps_threads.init();
    // SAFETY: `p` is static and in no thread list yet.
    unsafe { pr.ps_threads.insert_tail(p) };
    pr.ps_threadcnt.set(1);
    p.p_p.set(pr);

    // give the process the same creds as the initial thread
    pr.ps_ucred.set(p.p_ucred.get());
    // crhold(pr->ps_ucred): kern_prot.c (M6); KASSERT(cr_refcnt >= 2) with it.
    let _ = unported!("process_initialize: crhold (kern_prot.c, M6)");

    // prof_fork(pr): subr_prof.c (M6).
    let _ = unported!("process_initialize: prof_fork (M6)");

    pr.ps_children.init();
    pr.ps_orphans.init();
    // LIST_INIT(&pr->ps_sigiolst): sigio (M6).

    // rw_init(&pr->ps_lock, "pslock"): kern_rwlock.c (M5-b2).
    mtx_init(&pr.ps_mtx, IPL_HIGH);
    // klist_init_mutex(&pr->ps_klist, &pr->ps_mtx): kqueue (M6).

    // timeout_set_flags(&pr->ps_realit_to, realitexpire, pr, KCLOCK_UPTIME, 0) and
    // timeout_set(&pr->ps_rucheck_to, rucheck, pr): kern_time.c and kern_resource.c (M6).
    let _ = unported!("process_initialize: realitexpire/rucheck timeouts (M6)");
}

/// `fork_tfmrate`: print the 'table full' message once per 10 seconds.
const FORK_TFMRATE: Timeval = Timeval::new(10, 0);

/// `fork_check_maxthread`: although process entries are dynamically created, we still keep
/// a global limit on the maximum number we will create. We reserve the last 5 processes to
/// root. The variable `nprocesses` is the current number of processes, `maxprocess` is the
/// limit. Similar rules for threads (struct proc): we reserve the last 5 to root; the
/// variable `nthreads` is the current number of procs, `maxthread` is the limit.
pub fn fork_check_maxthread(uid: Uid) -> Result<(), Errno> {
    static LASTTFM: StaticCell<Timeval> = StaticCell::new(Timeval::new(0, 0));

    let maxthread_local = MAXTHREAD.load(Ordering::Relaxed);
    let val = NTHREADS.fetch_add(1, Ordering::Relaxed) + 1;
    if (val > maxthread_local - 5 && uid != 0) || val > maxthread_local {
        // SAFETY: `lasttfm` is a rate limiter touched under the kernel lock.
        let lasttfm = unsafe { LASTTFM.get_mut() };
        if ratecheck(lasttfm, &FORK_TFMRATE) {
            printf(format_args!("thread: table is full\n")); // tablefull("thread")
        }
        NTHREADS.fetch_sub(1, Ordering::Relaxed);
        return Err(Errno::EAGAIN);
    }

    Ok(())
}

/// `alloctid`: find an unused tid.
pub fn alloctid() -> Pid {
    loop {
        // (0 .. TID_MASK+1]
        let tid = 1 + (arc4random() as Pid & TID_MASK);
        if tfind(tid).is_none() {
            return tid;
        }
    }
}

/// `oldpids`: the recently freed pids, not reused for a while.
static OLDPIDS: StaticCell<[Pid; 128]> = StaticCell::new([0; 128]);

/// `ispidtaken`: checks for current use of a pid, either as a pid or pgid.
pub fn ispidtaken(pid: Pid) -> bool {
    // SAFETY: written only by `freepid` under the kernel lock.
    if unsafe { OLDPIDS.get() }.contains(&pid) {
        return true;
    }

    if prfind(pid).is_some() {
        return true;
    }
    if pgfind(pid).is_some() {
        return true;
    }
    if zombiefind(pid).is_some() {
        return true;
    }
    false
}

/// `allocpid`: find an unused pid.
pub fn allocpid() -> Pid {
    static FIRST: AtomicI32 = AtomicI32::new(1);

    // The first PID allocated is always 1.
    if FIRST.swap(0, Ordering::Relaxed) == 1 {
        return 1;
    }

    // All subsequent PIDs are chosen randomly. We need to find an unused PID in the range
    // [2, PID_MAX].
    loop {
        let pid = 2 + arc4random_uniform((PID_MAX - 1) as u32) as Pid;
        if !ispidtaken(pid) {
            return pid;
        }
    }
}

/// `freepid`: remembers a freed pid so it is not reused right away.
pub fn freepid(pid: Pid) {
    static IDX: AtomicU32 = AtomicU32::new(0);

    let idx = IDX.fetch_add(1, Ordering::Relaxed) as usize % 128;
    // SAFETY: as for `ispidtaken`; one writer under the kernel lock.
    unsafe { OLDPIDS.get_mut()[idx] = pid };
}
