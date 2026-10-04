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
//! `freepid`; part b2 adds `forkstat`, `thread_new`, `process_new`, `fork_thread_start`,
//! `fork1` and `proc_trampoline_mi`, enough for kernel threads. M8 adds `fork_return`,
//! `sys_fork`, `sys_vfork`, `sys___tfork` and `thread_fork` (the file is complete); the
//! child returns to user mode through the machine's `child_return` (`machine::cpu`).
//!
//! ## Deviations
//! - `fork1` returns the new thread (`Result<&Proc, Errno>`) instead of an `int` plus the
//!   `retval`/`rnewprocp` out-pointers; `thread_new` and `process_new` write a whole
//!   `Proc::new()`/`Process::new()` into the pool item and then copy the `p_startcopy`/
//!   `ps_startcopy` fields from the parent, instead of `memset`/`memcpy` by field offset.
//! - `process_initialize` reports what its process does not have yet: `prof_fork` (M6);
//!   the `realitexpire` timeout is real since `kern_time.c` (M8) and `klist_init_mutex`
//!   since `kern_event.c`. `process_new` likewise reports `startprofclock` (M6);
//!   `sigactsinit` is real since `kern_sig.c`, `fdcopy`/`fdshare` since `kern_descrip.c`
//!   and `lim_fork` since the `plimit` port. `fork1` skips the `RLIMIT_NPROC` check for
//!   root as the C does, reports it for other users (the limits); `knote_processfork` is
//!   real since `kern_event.c`. The credentials (`crhold` in `thread_new` and
//!   `process_initialize`, the forking thread's real uid in `fork1`) are real since
//!   `kern_prot.c`.
//! - `sys_fork` never asks for `FORK_PTRACE`/`fork_return`: the `ptrace(2)` event mask
//!   (`ps_ptmask`, `sys_process.c`) is not ported, so no process wants fork reports.

use core::ffi::c_void;
use core::ptr::{self, NonNull};
use core::sync::atomic::{AtomicI32, AtomicU32, Ordering, fence};

use libkern::StaticCell;

use crate::conf::param::{MAXPROCESS, MAXTHREAD};
use crate::dev::rnd::{arc4random, arc4random_uniform};
use crate::kassert;
use crate::kern::kern_clock::hardclock_period;
use crate::kern::kern_clockintr::clockintr_advance;
use crate::kern::kern_descrip::{fdcopy, fdshare};
use crate::kern::kern_event::{klist_init_mutex, knote_processfork};
use crate::kern::kern_lock::{mtx_enter, mtx_init, mtx_leave};
use crate::kern::kern_proc::{
    ALLPROC, ALLPROCESS, PROC_POOL, PROCESS_POOL, chgproccnt, pgfind, pidhash, prfind, tfind,
    tidhash, zombiefind,
};
use crate::kern::kern_prot::crhold;
use crate::kern::kern_resource::{lim_fork, rucheck};
use crate::kern::kern_rwlock::rw_init;
use crate::kern::kern_sched::{sched_choosecpu_fork, setrunqueue};
use crate::kern::kern_sig::{psignal, sigactsinit, sigstkinit};
use crate::kern::kern_smr::smr_idle;
use crate::kern::kern_synch::{endtsleep, refcnt_init, tsleep_nsec};
use crate::kern::kern_tc::nanouptime;
use crate::kern::kern_time::ratecheck;
use crate::kern::kern_timeout::timeout_set;
use crate::kern::sched_bsd::{
    SCHED_LOCK, sched_assert_locked, sched_assert_unlocked, sched_lock, sched_unlock,
};
use crate::kern::subr_pool::pool_get;
use crate::kern::subr_prf::{panic, tablefull};
use crate::kern::subr_prof::profclock_period;
use crate::kern::subr_xxx::assertwaitok;
use crate::machine::Machine;
use crate::machine::copy::{copyin, copyout};
use crate::machine::cpu::{Cpu, CpuInfo, child_return, curcpu, curproc};
use crate::machine::intr::{IPL_HIGH, spl0};
use crate::machine::tcb::tcb_invalid;
use crate::sys::acct::AFORK;
use crate::sys::errno::Errno;
use crate::sys::param::PWAIT;
use crate::sys::pool::PR_WAITOK;
use crate::sys::proc::{
    FORK_FORK, FORK_IDLE, FORK_NOZOMBIE, FORK_PPWAIT, FORK_PTRACE, FORK_SHAREFILES, FORK_SHAREVM,
    FORK_SYSTEM, FORK_VFORK, P_CPUPEG, P_SUSPSIG, P_SUSPSINGLE, P_SYSTEM, P_THREAD, PID_MAX,
    PS_EMBRYO, PS_FLAGS_INHERITED_ON_FORK, PS_ISPWAIT, PS_ITIMER, PS_NOZOMBIE, PS_PPWAIT,
    PS_PROFIL, PS_STOPPING, PS_SYSTEM, PS_TRACED, Proc, Process, ProcessPglist, SIDL,
    THREAD_PID_OFFSET, TID_MASK,
};
use crate::sys::queue::ListHead;
use crate::sys::sched::{SPCF_ITIMER, SPCF_PROFCLOCK};
use crate::sys::signal::{SIGSEGV, SIGTRAP};
use crate::sys::syscallargs::SysTforkArgs;
use crate::sys::systm::{INFSLP, SysArgs, kernel_assert_unlocked, kernel_lock, sysargs};
use crate::sys::time::Timeval;
use crate::sys::types::{Pid, Register, Uid};
use crate::sys::unistd::Tfork;
use crate::sys::user::User;
use crate::sys::vmmeter::Forkstat;
use crate::unported;
use crate::uvm::uvm_glue::uvm_uarea_alloc;
use crate::uvm::uvm_init::UVMEXP;
use crate::uvm::uvm_map::{uvmspace_fork, uvmspace_share};

/// `nprocesses`: process 0.
pub static NPROCESSES: AtomicI32 = AtomicI32::new(1);
/// \[a\] `nthreads`: proc 0.
pub static NTHREADS: AtomicI32 = AtomicI32::new(1);

/// `forkstat`: the fork statistics (`<sys/vmmeter.h>`).
pub static FORKSTAT: Forkstat = Forkstat::new();

/// The calling thread as the `&'static Proc` `fork1` and `thread_fork` keep (the child's
/// parent links and `cpu_fork`'s frame copy).
fn curthread(p: &Proc) -> &'static Proc {
    // SAFETY: a thread is a `proc_pool` item freed only by the reaper after it exited; the
    // thread making this system call is running, so it outlives every use made of it here.
    unsafe { &*ptr::from_ref(p) }
}

/// `fork_return`: the first thing a forked child runs when the parent asked `ptrace(2)` to
/// report forks: stop for the tracer, then return to user mode as `child_return` does.
pub fn fork_return(arg: *mut c_void) {
    // SAFETY: `fork1` passes the new thread itself as the argument.
    let p = unsafe { &*arg.cast::<Proc>() };

    if p.process().ps_flags.load(Ordering::Relaxed) & PS_TRACED != 0 {
        psignal(p, SIGTRAP);
    }

    child_return(arg);
}

/// `fork(2)`.
pub fn sys_fork(p: &Proc, _v: &SysArgs, retval: &mut [Register; 2]) -> Result<(), Errno> {
    let func: fn(*mut c_void) = child_return;
    let flags = FORK_FORK;

    // ps_ptmask & PTRACE_FORK (flags |= FORK_PTRACE, func = fork_return): the ptrace event
    // mask is sys_process.c's, which is not ported, so no process asks for fork reports.

    let child = fork1(curthread(p), flags, func, ptr::null_mut())?;
    retval[0] = child.process().ps_pid.get() as Register;
    Ok(())
}

/// `vfork(2)`: the parent sleeps until the child execs or exits (`FORK_PPWAIT`).
pub fn sys_vfork(p: &Proc, _v: &SysArgs, retval: &mut [Register; 2]) -> Result<(), Errno> {
    let child = fork1(
        curthread(p),
        FORK_VFORK | FORK_PPWAIT,
        child_return,
        ptr::null_mut(),
    )?;
    retval[0] = child.process().ps_pid.get() as Register;
    Ok(())
}

/// `__tfork(2)`: a new thread in the calling process, with its own stack and TCB.
#[allow(non_snake_case)] // the C name: sys___tfork
pub fn sys___tfork(p: &Proc, v: &SysArgs, retval: &mut [Register; 2]) -> Result<(), Errno> {
    let uap: &SysTforkArgs = sysargs(v);
    let psize = uap.psize.get();
    let mut raw = [0u8; size_of::<Tfork>()];

    if psize == 0 || psize > raw.len() {
        return Err(Errno::EINVAL);
    }
    copyin(uap.param.get() as usize, &mut raw[..psize])?;
    let word = |i: usize| {
        const W: usize = size_of::<usize>();
        let mut w = [0u8; W];
        w.copy_from_slice(&raw[i * W..(i + 1) * W]);
        usize::from_ne_bytes(w)
    };
    let param = Tfork {
        tf_tcb: word(0),
        tf_tid: word(1),
        tf_stack: word(2),
    };
    // KTRACE: not configured.
    if tcb_invalid(param.tf_tcb) {
        return Err(Errno::EINVAL);
    }

    thread_fork(
        curthread(p),
        param.tf_stack,
        param.tf_tcb,
        param.tf_tid,
        retval,
    )
}

/// `thread_new`: allocates a thread, copying `parent`'s inheritable fields, with its u-area
/// at `uaddr`.
fn thread_new(parent: &Proc, uaddr: NonNull<u8>) -> &'static Proc {
    let Some(mem) = pool_get(&PROC_POOL, PR_WAITOK) else {
        panic(format_args!("thread_new: proc_pool is empty"));
    };
    let pp = mem.cast::<Proc>();
    // SAFETY: a fresh, suitably aligned pool item of `size_of::<Proc>()` bytes, written once
    // before anything else sees it.
    unsafe { pp.as_ptr().write(Proc::new()) };
    // SAFETY: as above; the thread lives until `exit2` returns it to the pool.
    let p: &'static Proc = unsafe { pp.as_ref() };

    p.p_stat.set(SIDL); // protect against others
    p.p_runpri.set(0);
    p.p_flag.store(0, Ordering::Relaxed);

    // Make a proc table entry for the new process. Start by zeroing the section of proc
    // that is zero-initialized (`Proc::new()` did), then copy the section that is copied
    // directly from the parent (p_startcopy .. p_endcopy).
    p.p_sigmask.set(parent.p_sigmask.get());
    // SAFETY: `p_name` is this thread's and the parent's (which is running, hence not
    // renaming itself); plain copies.
    unsafe { ptr::write(p.p_name.get(), ptr::read(parent.p_name.get())) };
    p.p_slppri.set(parent.p_slppri.get());
    p.p_usrpri.set(parent.p_usrpri.get());
    p.p_estcpu.set(parent.p_estcpu.get());
    p.p_pledge_syscall.set(parent.p_pledge_syscall.get());
    p.p_pledge.set(parent.p_pledge.get());
    p.p_ucred.set(parent.p_ucred.get());
    p.p_sigstk.set(parent.p_sigstk.get());
    p.p_prof_addr.set(parent.p_prof_addr.get());
    p.p_prof_ticks.set(parent.p_prof_ticks.get());

    crhold(p.ucred());
    p.p_addr.set(uaddr.as_ptr().cast::<User>());

    // Initialize the timeouts.
    timeout_set(&p.p_sleep_to, endtsleep, ptr::from_ref(p).cast_mut().cast());

    p
}

/// `process_initialize`: initialize common bits of a process structure, given the initial
/// thread.
pub fn process_initialize(pr: &'static Process, p: &'static Proc) {
    refcnt_init(&pr.ps_refcnt);

    // initialize the thread links
    pr.ps_mainproc.set(p);
    pr.ps_threads.init();
    // SAFETY: `p` outlives its process and is in no thread list yet.
    unsafe { pr.ps_threads.insert_tail(p) };
    pr.ps_threadcnt.set(1);
    p.p_p.set(pr);

    // give the process the same creds as the initial thread
    pr.ps_ucred.set(p.p_ucred.get());
    crhold(pr.ucred());
    // new thread and new process
    kassert!(p.ucred().cr_refcnt.r_refs.load(Ordering::Relaxed) >= 2);

    // prof_fork(pr): subr_prof.c (M6).
    let _ = unported!("process_initialize: prof_fork (M6)");

    pr.ps_children.init();
    pr.ps_orphans.init();
    pr.ps_sigiolst.init();

    rw_init(&pr.ps_lock, "pslock");
    mtx_init(&pr.ps_mtx, IPL_HIGH);
    // SAFETY: `ps_mtx` is a member of the same process, alive as long as its `ps_klist`.
    unsafe { klist_init_mutex(&pr.ps_klist, &pr.ps_mtx) };

    crate::kern::kern_timeout::timeout_set_flags(
        &pr.ps_realit_to,
        crate::kern::kern_time::realitexpire,
        ptr::from_ref(pr).cast_mut().cast::<c_void>(),
        crate::sys::timeout::KCLOCK_UPTIME,
        0,
    );
    timeout_set(
        &pr.ps_rucheck_to,
        rucheck,
        ptr::from_ref(pr).cast_mut().cast::<c_void>(),
    );
}

/// `process_new`: allocate and initialize a new process.
fn process_new(p: &'static Proc, parent: &'static Process, flags: i32) -> &'static Process {
    let Some(mem) = pool_get(&PROCESS_POOL, PR_WAITOK) else {
        panic(format_args!("process_new: process_pool is empty"));
    };
    let pp = mem.cast::<Process>();
    // SAFETY: a fresh, suitably aligned pool item of `size_of::<Process>()` bytes, written
    // once before anything else sees it.
    unsafe { pp.as_ptr().write(Process::new()) };
    // SAFETY: as above; the process lives until `process_zap`/`exit2` free it.
    let pr: &'static Process = unsafe { pp.as_ref() };

    // Make a process structure for the new process. Start by zeroing the section of proc
    // that is zero-initialized (`Process::new()` did), then copy the section that is copied
    // directly from the parent (ps_startcopy .. ps_endcopy).
    pr.ps_limit.set(parent.ps_limit.get());
    pr.ps_pgrp.set(parent.ps_pgrp.get());
    pr.set_comm(parent.comm());
    pr.ps_strings.set(parent.ps_strings.get());
    pr.ps_auxinfo.set(parent.ps_auxinfo.get());
    pr.ps_timekeep.set(parent.ps_timekeep.get());
    pr.ps_sigcode.set(parent.ps_sigcode.get());
    pr.ps_sigcoderet.set(parent.ps_sigcoderet.get());
    pr.ps_sigcookie.set(parent.ps_sigcookie.get());
    pr.ps_rtableid.store(
        parent.ps_rtableid.load(Ordering::Relaxed),
        Ordering::Relaxed,
    );
    pr.ps_iflags.set(parent.ps_iflags.get());
    pr.ps_nice.set(parent.ps_nice.get());
    pr.ps_acflag.set(parent.ps_acflag.get());
    pr.ps_pledge
        .store(parent.ps_pledge.load(Ordering::Relaxed), Ordering::Relaxed);
    pr.ps_execpledge.set(parent.ps_execpledge.get());
    pr.ps_kbind_cookie.set(parent.ps_kbind_cookie.get());
    pr.ps_kbind_addr.set(parent.ps_kbind_addr.get());

    process_initialize(pr, p);
    pr.ps_pid.set(allocpid());
    lim_fork(parent, pr);

    // post-copy fixups
    pr.ps_pptr.set(parent);
    pr.ps_pgrp.set(ptr::null());
    pr.ps_ppid.set(parent.ps_pid.get());
    // WITNESS_SETCHILD: not configured.

    // bump references to the text vnode (for sysctl)
    pr.ps_textvp.set(parent.ps_textvp.get());
    if let Some(vp) = pr.ps_textvp.get() {
        crate::kern::vfs_subr::vref(vp);
    }

    // copy unveil if unveil is active
    crate::kern::kern_unveil::unveil_copy(parent, pr);

    pr.ps_flags.store(
        parent.ps_flags.load(Ordering::Relaxed) & PS_FLAGS_INHERITED_ON_FORK,
        Ordering::Relaxed,
    );
    // SAFETY: a process always has a session, a static or pool item alive while the
    // process is.
    let session = unsafe { &*parent.session() };
    if !session.s_ttyvp.get().is_null() {
        pr.ps_flags.fetch_or(
            parent.ps_flags.load(Ordering::Relaxed) & crate::sys::proc::PS_CONTROLT,
            Ordering::Relaxed,
        );
    }

    pin_copy(&parent.ps_pin, &pr.ps_pin);
    pin_copy(&parent.ps_libcpin, &pr.ps_libcpin);

    // Duplicate sub-structures as needed. Increase reference counts on shared objects.
    if flags & FORK_SHAREFILES != 0 {
        pr.ps_fd.set(fdshare(parent));
    } else {
        pr.ps_fd.set(fdcopy(parent));
    }
    pr.ps_sigacts.set(sigactsinit(parent));
    if flags & FORK_SHAREVM != 0 {
        pr.ps_vmspace.set(uvmspace_share(parent));
    } else {
        pr.ps_vmspace.set(uvmspace_fork(parent));
    }

    if parent.ps_flags.load(Ordering::Relaxed) & PS_PROFIL != 0 {
        let _ = unported!("process_new: startprofclock (subr_prof.c, M6)");
    }
    if flags & FORK_PTRACE != 0 {
        pr.ps_flags.fetch_or(
            parent.ps_flags.load(Ordering::Relaxed) & PS_TRACED,
            Ordering::Relaxed,
        );
    }
    if flags & FORK_NOZOMBIE != 0 {
        pr.ps_flags.fetch_or(PS_NOZOMBIE, Ordering::Relaxed);
    }
    if flags & FORK_SYSTEM != 0 {
        pr.ps_flags.fetch_or(PS_SYSTEM, Ordering::Relaxed);
    }

    // mark as embryo to protect against others
    pr.ps_flags.fetch_or(PS_EMBRYO, Ordering::Relaxed);

    // Force visibility of all of the above changes
    fence(Ordering::Release); // membar_producer()

    // it's sufficiently inited to be globally visible
    // SAFETY: `pr` is static-lived (see above) and in no list yet.
    unsafe { ALLPROCESS.0.insert_head(pr) };

    pr
}

/// `process_new`'s copy of a pin table (`ps_pin`, `ps_libcpin`): the bounds come with the
/// copied area, the table itself is duplicated (`mallocarray(M_PINSYSCALL)` + `memcpy`).
fn pin_copy(from: &crate::sys::proc::Pinsyscall, to: &crate::sys::proc::Pinsyscall) {
    to.pn_start.set(from.pn_start.get());
    to.pn_end.set(from.pn_end.get());
    to.pn_npins.set(from.pn_npins.get());
    to.pn_pins.set(ptr::null_mut());
    let Some(src) = NonNull::new(from.pn_pins.get()) else {
        return;
    };
    let n = from.pn_npins.get().max(0) as usize;
    let Some(mem) = crate::kern::kern_malloc::mallocarray(
        n,
        size_of::<u32>(),
        crate::sys::malloc::M_PINSYSCALL,
        crate::sys::malloc::M_WAITOK,
    ) else {
        // M_WAITOK cannot fail in C; here the child simply has no table (pin_check kills
        // it at its first system call, as it would a process without one).
        to.pn_npins.set(0);
        return;
    };
    let dst = mem.cast::<u32>();
    // SAFETY: `src` is the parent's table of `n` entries (alive while the parent forks),
    // `dst` a fresh allocation of `n` entries; they do not overlap.
    unsafe { ptr::copy_nonoverlapping(src.as_ptr(), dst.as_ptr(), n) };
    to.pn_pins.set(dst.as_ptr());
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
            tablefull("thread");
        }
        NTHREADS.fetch_sub(1, Ordering::Relaxed);
        return Err(Errno::EAGAIN);
    }

    Ok(())
}

/// `fork_thread_start`: puts the new thread on a run queue.
fn fork_thread_start(p: &Proc, parent: &Proc, flags: i32) {
    sched_lock();
    let ci = sched_choosecpu_fork(parent, flags);
    // TRACEPOINT(sched, fork, ...): dt(4), not configured.
    setrunqueue(Some(ci), p, p.p_usrpri.get());
    sched_unlock();
}

/// `fork1`: creates a new process (and its first thread) that starts in `func(arg)`
/// (`arg` null: the new thread itself). Returns the new thread (the C's `*rnewprocp`;
/// `*retval`, the child's pid, is `p.process().ps_pid`).
pub fn fork1(
    curp: &'static Proc,
    flags: i32,
    func: fn(*mut c_void),
    arg: *mut c_void,
) -> Result<&'static Proc, Errno> {
    static LASTTFM: StaticCell<Timeval> = StaticCell::new(Timeval::new(0, 0));

    let curpr: &'static Process = curp.process();
    let uid: Uid = curp.ucred().cr_ruid.get();

    kassert!(
        flags
            & !(FORK_FORK
                | FORK_VFORK
                | FORK_PPWAIT
                | FORK_PTRACE
                | FORK_IDLE
                | FORK_SHAREVM
                | FORK_SHAREFILES
                | FORK_NOZOMBIE
                | FORK_SYSTEM)
            == 0
    );

    fork_check_maxthread(uid)?;

    let maxprocess_local = MAXPROCESS.load(Ordering::Relaxed);
    let nprocesses = NPROCESSES.load(Ordering::Relaxed);
    if (nprocesses >= maxprocess_local - 5 && uid != 0) || nprocesses >= maxprocess_local {
        // SAFETY: `lasttfm` is a rate limiter touched under the kernel lock.
        let lasttfm = unsafe { LASTTFM.get_mut() };
        if ratecheck(lasttfm, &FORK_TFMRATE) {
            tablefull("process");
        }
        NTHREADS.fetch_sub(1, Ordering::Relaxed);
        return Err(Errno::EAGAIN);
    }
    NPROCESSES.fetch_add(1, Ordering::Relaxed);

    // Increment the count of processes running with this uid. Don't allow a nonprivileged
    // user to exceed their current limit.
    let _count = chgproccnt(uid, 1);
    if uid != 0 {
        // count > lim_cur(RLIMIT_NPROC): the limits (kern_resource.c, M6).
        let _ = unported!("fork1: lim_cur(RLIMIT_NPROC) (M6)");
    }

    let Some(uaddr) = uvm_uarea_alloc() else {
        chgproccnt(uid, -1);
        NPROCESSES.fetch_sub(1, Ordering::Relaxed);
        NTHREADS.fetch_sub(1, Ordering::Relaxed);
        return Err(Errno::ENOMEM);
    };

    // From now on, we're committed to the fork and cannot fail.
    let p = thread_new(curp, uaddr);
    let pr = process_new(p, curpr, flags);

    p.p_fd.set(pr.ps_fd.get());
    p.p_vmspace.set(pr.ps_vmspace.get());
    if pr.ps_flags.load(Ordering::Relaxed) & PS_SYSTEM != 0 {
        p.p_flag.fetch_or(P_SYSTEM, Ordering::Relaxed);
    }

    if flags & FORK_PPWAIT != 0 {
        pr.ps_flags.fetch_or(PS_PPWAIT, Ordering::Relaxed);
        curpr.ps_flags.fetch_or(PS_ISPWAIT, Ordering::Relaxed);
    }

    // KTRACE: not configured.

    // Finish creating the child thread. cpu_fork() will copy and update the pcb and make
    // the child ready to run. If this is a normal user fork, the child will exit directly to
    // user mode via child_return() on its first time slice and will not return here. If
    // this is a kernel thread, the specified entry point will be executed.
    let arg = if arg.is_null() {
        ptr::from_ref(p).cast_mut().cast::<c_void>()
    } else {
        arg
    };
    Machine::cpu_fork(curp, p, ptr::null_mut(), ptr::null_mut(), func, arg);

    let vm = pr.vmspace();
    let vmsize = (vm.vm_dsize.get() + vm.vm_ssize.get()) as u64;
    if flags & FORK_FORK != 0 {
        FORKSTAT.cntfork.fetch_add(1, Ordering::Relaxed);
        FORKSTAT.sizfork.fetch_add(vmsize, Ordering::Relaxed);
    } else if flags & FORK_VFORK != 0 {
        FORKSTAT.cntvfork.fetch_add(1, Ordering::Relaxed);
        FORKSTAT.sizvfork.fetch_add(vmsize, Ordering::Relaxed);
    } else {
        FORKSTAT.cntkthread.fetch_add(1, Ordering::Relaxed);
        FORKSTAT.sizkthread.fetch_add(vmsize, Ordering::Relaxed);
    }

    p.p_tid.set(alloctid());
    // SAFETY: `p` and `pr` are static-lived pool items in no list or hash chain yet; the
    // lists are the kernel lock's.
    unsafe {
        ALLPROC.0.insert_head(p);
        tidhash(p.p_tid.get()).insert_head(p);
        pidhash(pr.ps_pid.get()).insert_head(pr);
    }

    pr.ps_pgrp.set(curpr.ps_pgrp.get());
    // SAFETY: `curpr` is on its pgrp's member list; `pr` joins it behind, and the children
    // list of `curpr`, which it is not on yet.
    unsafe {
        ListHead::<ProcessPglist>::insert_after(curpr, pr);
        curpr.ps_children.insert_head(pr);
    }

    mtx_enter(&pr.ps_mtx);
    if pr.ps_flags.load(Ordering::Relaxed) & PS_TRACED != 0 {
        // ps_opptr, process_reparent, the ptrace status: ptrace (M6).
        let _ = unported!("fork1: ptrace (process_reparent, ps_ptstat, M6)");
    }
    mtx_leave(&pr.ps_mtx);

    // For new processes, set accounting bits and mark as complete.
    pr.ps_start.set(nanouptime());
    pr.ps_acflag.set(AFORK);
    pr.ps_flags.fetch_and(!PS_EMBRYO, Ordering::Relaxed);

    // Idle threads are just assigned to the CPU but not added to any runqueue.
    if flags & FORK_IDLE != 0 {
        p.p_cpu.set(arg.cast::<CpuInfo>());
        // for consistency mark idle procs as pegged
        p.p_flag.fetch_or(P_CPUPEG, Ordering::Relaxed);
    } else {
        fork_thread_start(p, curp, flags);
    }

    // Notify any interested parties about the new process.
    knote_processfork(curpr, pr.ps_pid.get());

    // Update stats now that we know the fork was successful.
    UVMEXP.forks.fetch_add(1, Ordering::Relaxed);
    if flags & FORK_PPWAIT != 0 {
        UVMEXP.forks_ppwait.fetch_add(1, Ordering::Relaxed);
    }
    if flags & FORK_SHAREVM != 0 {
        UVMEXP.forks_sharevm.fetch_add(1, Ordering::Relaxed);
    }

    // Preserve synchronization semantics of vfork. If waiting for child to exec or exit,
    // set PS_PPWAIT on child and PS_ISPWAIT on ourselves, and sleep on our process for the
    // latter flag to go away. XXX Need to stop other rthreads in the parent
    if flags & FORK_PPWAIT != 0 {
        while curpr.ps_flags.load(Ordering::Relaxed) & PS_ISPWAIT != 0 {
            let _ = tsleep_nsec(ptr::from_ref(curpr), PWAIT, "ppwait", INFSLP);
        }
    }

    // If we're tracing the child, alert the parent too.
    if flags & FORK_PTRACE != 0 && curpr.ps_flags.load(Ordering::Relaxed) & PS_TRACED != 0 {
        psignal(curp, SIGTRAP);
    }

    // Return child pid to parent process: the caller reads pr->ps_pid.
    Ok(p)
}

/// `thread_fork`: a new thread of `curp`'s process that returns to user mode on `stack`
/// with `tcb` as its TCB; its id (offset by `THREAD_PID_OFFSET`) is the return value and is
/// copied out to `tidptr` when that is not NULL.
pub fn thread_fork(
    curp: &'static Proc,
    stack: usize,
    tcb: usize,
    tidptr: usize,
    retval: &mut [Register; 2],
) -> Result<(), Errno> {
    let pr = curp.process();

    if stack == 0 {
        return Err(Errno::EINVAL);
    }

    fork_check_maxthread(curp.ucred().cr_ruid.get())?;

    let Some(uaddr) = uvm_uarea_alloc() else {
        NTHREADS.fetch_sub(1, Ordering::Relaxed);
        return Err(Errno::ENOMEM);
    };

    // From now on, we're committed to the fork and cannot fail.
    let p = thread_new(curp, uaddr);
    p.p_flag.fetch_or(P_THREAD, Ordering::Relaxed);
    sigstkinit(&p.p_sigstk);
    p.set_name(b"");

    // other links
    p.p_p.set(pr);

    // local copies
    p.p_fd.set(pr.ps_fd.get());
    p.p_vmspace.set(pr.ps_vmspace.get());

    // Finish creating the child thread. cpu_fork() will copy and update the pcb and make
    // the child ready to run. The child will exit directly to user mode via child_return()
    // on its first time slice and will not return here.
    Machine::cpu_fork(
        curp,
        p,
        stack as *mut u8,
        tcb as *mut u8,
        child_return,
        ptr::from_ref(p).cast_mut().cast::<c_void>(),
    );

    p.p_tid.set(alloctid());

    // SAFETY: `p` is a static-lived pool item in no list or hash chain yet; the lists are
    // the kernel lock's.
    unsafe {
        ALLPROC.0.insert_head(p);
        tidhash(p.p_tid.get()).insert_head(p);
    }

    mtx_enter(&pr.ps_mtx);
    // SAFETY: `p` is on no thread list yet; `ps_threads` is `ps_mtx`'s, held.
    unsafe { pr.ps_threads.insert_tail(p) };
    pr.ps_threadcnt.set(pr.ps_threadcnt.get() + 1);

    // if somebody else wants to take us to single threaded mode or suspend the process,
    // count ourselves in.
    if !pr.ps_single.get().is_null() || pr.ps_flags.load(Ordering::Relaxed) & PS_STOPPING != 0 {
        pr.ps_suspendcnt.set(pr.ps_suspendcnt.get() + 1);
        p.p_flag.fetch_or(
            curp.p_flag.load(Ordering::Relaxed) & (P_SUSPSINGLE | P_SUSPSIG),
            Ordering::Relaxed,
        );
    }
    mtx_leave(&pr.ps_mtx);

    // Return tid to parent thread and copy it out to userspace
    let tid: Pid = p.p_tid.get() + THREAD_PID_OFFSET;
    retval[0] = tid as Register;
    if tidptr != 0 && copyout(&tid.to_ne_bytes(), tidptr).is_err() {
        psignal(curp, SIGSEGV);
    }

    fork_thread_start(p, curp, 0);

    // Update stats now that we know the fork was successful.
    FORKSTAT.cnttfork.fetch_add(1, Ordering::Relaxed);
    UVMEXP.forks.fetch_add(1, Ordering::Relaxed);
    UVMEXP.forks_sharevm.fetch_add(1, Ordering::Relaxed);

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

/// `proc_trampoline_mi`: do machine independent parts of switching to a new process. The
/// first thing a new thread runs, from `proc_trampoline`, with the scheduler lock still
/// held by the switch that started it.
pub fn proc_trampoline_mi() {
    let spc = Machine::ci_schedstate(curcpu());
    let Some(p) = curproc() else {
        panic(format_args!("proc_trampoline_mi: no curproc"));
    };

    sched_assert_locked();
    Machine::clear_resched(curcpu());
    mtx_leave(&SCHED_LOCK);
    spl0();

    sched_assert_unlocked();
    kernel_assert_unlocked(); // KERNEL_ASSERT_UNLOCKED()
    assertwaitok();
    smr_idle();

    // Start any optional clock interrupts needed by the thread.
    if p.process().ps_flags.load(Ordering::Relaxed) & PS_ITIMER != 0 {
        spc.spc_schedflags.fetch_or(SPCF_ITIMER, Ordering::Relaxed);
        clockintr_advance(&spc.spc_itimer, hardclock_period());
    }
    if p.process().ps_flags.load(Ordering::Relaxed) & PS_PROFIL != 0 {
        spc.spc_schedflags
            .fetch_or(SPCF_PROFCLOCK, Ordering::Relaxed);
        clockintr_advance(&spc.spc_profclock, profclock_period());
    }

    spc.spc_runtime.set(nanouptime());
    kernel_lock(); // KERNEL_LOCK()
}
