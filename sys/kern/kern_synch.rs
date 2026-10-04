/*	$OpenBSD: kern_synch.c,v 1.217 2026/08/19 13:05:12 claudio Exp $	*/
/*	$NetBSD: kern_synch.c,v 1.37 1996/04/22 01:38:37 christos Exp $	*/
/* <LICENSES> */
/*
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

//! Sleep and wakeup: `kern/kern_synch.c`.
//!
//! Upstream: sys/kern/kern_synch.c @ 3ce1f3f79392
//!
//! Status: `wip`. Milestone M5 ports the sleep queues (`sleep_queue_init`), `tsleep`,
//! `tsleep_nsec`, `msleep`, `msleep_nsec`, `sleep_setup`, `sleep_finish`, `wakeup_proc`,
//! `endtsleep`, `unsleep`, `wakeup_n`, `wakeup`, `wakeup_one`, the reference counts
//! (`refcnt_*`) and the condition variables (`cond_*`). `rwsleep[_nsec]` wait for
//! `kern_rwlock.c`; `sleep_signal_check` with `kern_sig.c`; `__thrsleep`/`__thrwakeup`
//! and `tslp_init` for the syscalls (M6). M8: `sys_sched_yield`. M11a: the `MULTIPROCESSOR`
//! kernel lock steps of `tsleep_nsec` and `msleep_nsec`.
//!
//! ## Deviations
//! - The sleep functions return `Result<(), Errno>` (`EWOULDBLOCK` on timeout, `EINTR`/
//!   `ERESTART` from a signal) instead of an `int`.
//! - The `cold == 2` ddb stack dump is not here (`cold` is a flag, not a counter).
//! - `safepri` and `cold` are `sys/systm.rs` statics (see there).
//! - The `cold || panicstr` path's release and reacquisition of the kernel lock, written
//!   twice in C, is one helper, `kernel_lock_bounce`.

use core::ffi::c_void;
use core::ptr;
use core::sync::atomic::{AtomicI32, Ordering, fence};

use crate::conf::param::TICK_NSEC;
use crate::kassert;
#[cfg(feature = "multiprocessor")]
use crate::kern::kern_lock::{
    __mp_acquire_count, __mp_release_all, _kernel_lock_held, KERNEL_LOCK,
};
use crate::kern::kern_lock::{mtx_enter, mtx_leave};
use crate::kern::kern_rwlock::{rw_assert_anylock, rw_enter, rw_exit, rw_status};
use crate::kern::kern_sched::setrunqueue;
use crate::kern::kern_sig::{cursig, proc_suspend_check, process_stop, process_suspend_signal};
use crate::kern::kern_timeout::{timeout_add_nsec, timeout_del};
use crate::kern::sched_bsd::{
    mi_switch, sched_assert_locked, sched_lock, sched_unlock, setrunnable,
};
#[cfg(feature = "diagnostic")]
use crate::kern::subr_prf::{Str, log};
use crate::kern::subr_prf::{panic, panicstr};
use crate::machine::Machine;
use crate::machine::cpu::{Cpu, curproc};
use crate::machine::intr::{splhigh, splx};
use crate::sys::errno::Errno;
use crate::sys::mutex::Mutex;
use crate::sys::param::{PCATCH, PNORELOCK, PRIMASK, PWAIT};
#[cfg(feature = "diagnostic")]
use crate::sys::proc::P_CANTSLEEP;
use crate::sys::proc::{
    Cond, P_INSCHED, P_SINTR, P_SUSPSIG, P_TIMEOUT, P_TIMEOUTRAN, P_WEXIT, PS_STOPPING, Proc,
    ProcRunq, SINGLE_SUSPEND, SONPROC, SSLEEP, SSTOP,
};
use crate::sys::queue::TailqHead;
use crate::sys::refcnt::Refcnt;
use crate::sys::rwlock::Rwlock;
use crate::sys::signal::sigmask;
use crate::sys::signalvar::Sigctx;
#[cfg(feature = "diagnostic")]
use crate::sys::syslog::LOG_WARNING;
use crate::sys::systm::{COLD, INFSLP, SAFEPRI, SysArgs};
use crate::sys::types::Register;

/// `TABLESIZE`: we're only looking at 7 bits of the address; everything is aligned to 4,
/// lots of things are aligned to greater powers of 2. Shift right by 8, i.e. drop the bottom
/// 256 worth.
const TABLESIZE: usize = 128;

/// `LOOKUP(x)`.
fn lookup(x: *const c_void) -> usize {
    ((x as usize) >> 8) & (TABLESIZE - 1)
}

/// `slpque[TABLESIZE]`'s table, made `Sync`: touched under the scheduler lock.
struct Slpque([TailqHead<ProcRunq>; TABLESIZE]);
// SAFETY: see the type's doc.
unsafe impl Sync for Slpque {}

/// `slpque`: the sleep queues.
static SLPQUE: Slpque = Slpque([const { TailqHead::new() }; TABLESIZE]);

/// `sleep_queue_init`.
pub fn sleep_queue_init() {
    for q in SLPQUE.0.iter() {
        q.init();
    }
}

/// `nowake`: global sleep channel for threads that do not want to receive wakeup(9)
/// broadcasts.
pub static NOWAKE: AtomicI32 = AtomicI32::new(0);

/// `&nowake` as an ident.
pub fn nowake() -> *const c_void {
    ptr::from_ref(&NOWAKE).cast()
}

/// `curproc`, which every sleeper has.
fn sleeper() -> &'static Proc {
    let Some(p) = curproc() else {
        panic(format_args!("sleep: no curproc"));
    };
    p
}

/// `tsleep_nsec`: general sleep call. Suspends the current process until a wakeup is
/// performed on the specified identifier. The process will then be made runnable with the
/// specified priority. Sleeps at most `nsecs` (`INFSLP` means no timeout). If `priority`
/// includes the `PCATCH` flag, signals are checked before and after sleeping, else signals
/// are not checked. Returns `Ok` if awakened, `EWOULDBLOCK` if the timeout expires. If
/// `PCATCH` is set and a signal needs to be delivered, `ERESTART` is returned if the current
/// system call should be restarted if possible, and `EINTR` is returned if the system call
/// should be interrupted by the signal.
pub fn tsleep_nsec<T: ?Sized>(
    ident: *const T,
    priority: i32,
    wmesg: &'static str,
    nsecs: u64,
) -> Result<(), Errno> {
    let ident = ident.cast::<c_void>();
    kassert!(priority & !(PRIMASK | PCATCH) == 0);
    kassert!(!ptr::eq(ident, nowake()) || priority & PCATCH != 0 || nsecs != INFSLP);

    #[cfg(feature = "multiprocessor")]
    kassert!(ptr::eq(ident, nowake()) || nsecs != INFSLP || _kernel_lock_held());

    if COLD.load(Ordering::Relaxed) || panicstr() {
        // After a panic, or during autoconfiguration, just give interrupts a chance, then just
        // return; don't run any other procs or panic below, in case this is the idle process
        // and already asleep.
        let s = splhigh();
        splx(SAFEPRI.load(Ordering::Relaxed));
        #[cfg(feature = "multiprocessor")]
        kernel_lock_bounce();
        splx(s);
        return Ok(());
    }

    sleep_setup(ident, priority, wmesg);
    sleep_finish(nsecs, true)
}

/// The `cold || panicstr` path's `MULTIPROCESSOR` step in `tsleep_nsec` and `msleep_nsec`:
/// if this CPU holds the kernel lock, drop every hold and take them back, so another CPU
/// waiting for the lock gets a turn.
#[cfg(feature = "multiprocessor")]
fn kernel_lock_bounce() {
    if _kernel_lock_held() {
        let hold_count = __mp_release_all(&KERNEL_LOCK);
        __mp_acquire_count(&KERNEL_LOCK, hold_count);
    }
}

/// `tsleep`: `tsleep_nsec` with the timeout in ticks.
pub fn tsleep<T: ?Sized>(
    ident: *const T,
    priority: i32,
    wmesg: &'static str,
    timo: i32,
) -> Result<(), Errno> {
    let mut nsecs = INFSLP;

    if timo < 0 {
        panic(format_args!("tsleep: negative timo {timo}"));
    }
    if timo > 0 {
        nsecs = timo as u64 * TICK_NSEC.load(Ordering::Relaxed) as u64;
    }

    tsleep_nsec(ident, priority, wmesg, nsecs)
}

/// `msleep_nsec`: same as `tsleep`, but if we have a mutex provided, then once we've entered
/// the sleep queue we drop the mutex. After sleeping we re-lock.
pub fn msleep_nsec<T: ?Sized>(
    ident: *const T,
    mtx: &Mutex,
    priority: i32,
    wmesg: &'static str,
    nsecs: u64,
) -> Result<(), Errno> {
    let ident = ident.cast::<c_void>();
    kassert!(priority & !(PRIMASK | PCATCH | PNORELOCK) == 0);
    kassert!(!ptr::eq(ident, nowake()) || priority & PCATCH != 0 || nsecs != INFSLP);

    if COLD.load(Ordering::Relaxed) || panicstr() {
        // After a panic, or during autoconfiguration, just give interrupts a chance, then just
        // return; don't run any other procs or panic below, in case this is the idle process
        // and already asleep.
        let spl = mtx.mtx_oldipl.get();
        mtx.mtx_oldipl.set(SAFEPRI.load(Ordering::Relaxed));
        mtx_leave(mtx);
        #[cfg(feature = "multiprocessor")]
        kernel_lock_bounce();
        if priority & PNORELOCK == 0 {
            mtx_enter(mtx);
            mtx.mtx_oldipl.set(spl);
        } else {
            splx(spl);
        }
        return Ok(());
    }

    sleep_setup(ident, priority, wmesg);

    mtx_leave(mtx);
    // signal may stop the process, release mutex before that
    let error = sleep_finish(nsecs, true);

    if priority & PNORELOCK == 0 {
        mtx_enter(mtx);
    }

    error
}

/// `msleep`: `msleep_nsec` with the timeout in ticks.
pub fn msleep<T: ?Sized>(
    ident: *const T,
    mtx: &Mutex,
    priority: i32,
    wmesg: &'static str,
    timo: i32,
) -> Result<(), Errno> {
    let mut nsecs = INFSLP;

    if timo < 0 {
        panic(format_args!("msleep: negative timo {timo}"));
    }
    if timo > 0 {
        nsecs = timo as u64 * TICK_NSEC.load(Ordering::Relaxed) as u64;
    }

    msleep_nsec(ident, mtx, priority, wmesg, nsecs)
}

/// `rwsleep_nsec`: same as `tsleep`, but if we have a rwlock provided, then once we've
/// entered the sleep queue we drop the it. After sleeping we re-lock.
pub fn rwsleep_nsec<T: ?Sized>(
    ident: *const T,
    rwl: &Rwlock,
    priority: i32,
    wmesg: &'static str,
    nsecs: u64,
) -> Result<(), Errno> {
    let ident = ident.cast::<c_void>();
    kassert!(priority & !(PRIMASK | PCATCH | PNORELOCK) == 0);
    kassert!(!ptr::eq(ident, nowake()) || priority & PCATCH != 0 || nsecs != INFSLP);
    kassert!(!ptr::eq(ident, ptr::from_ref(rwl).cast()));
    rw_assert_anylock(rwl);
    let status = rw_status(rwl);

    sleep_setup(ident, priority, wmesg);

    rw_exit(rwl);
    // signal may stop the process, release rwlock before that
    let error = sleep_finish(nsecs, true);

    if priority & PNORELOCK == 0 {
        let _ = rw_enter(rwl, status);
    }

    error
}

/// `rwsleep`: `rwsleep_nsec` with the timeout in ticks.
pub fn rwsleep<T: ?Sized>(
    ident: *const T,
    rwl: &Rwlock,
    priority: i32,
    wmesg: &'static str,
    timo: i32,
) -> Result<(), Errno> {
    let mut nsecs = INFSLP;

    if timo < 0 {
        panic(format_args!("rwsleep: negative timo {timo}"));
    }
    if timo > 0 {
        nsecs = timo as u64 * TICK_NSEC.load(Ordering::Relaxed) as u64;
    }

    rwsleep_nsec(ident, rwl, priority, wmesg, nsecs)
}

/// `sleep_setup`: puts the current thread on the sleep queue of `ident`.
pub fn sleep_setup(ident: *const c_void, prio: i32, wmesg: &'static str) {
    let p = sleeper();
    let mut prio = prio;

    #[cfg(feature = "diagnostic")]
    {
        if p.p_flag.load(Ordering::Relaxed) & P_CANTSLEEP != 0 {
            panic(format_args!(
                "sleep: {} failed insomnia",
                Str(p.process().comm())
            ));
        }
        if p.p_flag.load(Ordering::Relaxed) & P_SINTR != 0 {
            panic(format_args!("sleep: stale P_SINTR"));
        }
        if ident.is_null() {
            panic(format_args!("sleep: no ident"));
        }
        if p.p_stat.get() != SONPROC {
            panic(format_args!("sleep: not SONPROC but {}", p.p_stat.get()));
        }
    }
    // exiting processes are not allowed to catch signals
    if p.p_flag.load(Ordering::Relaxed) & P_WEXIT != 0 {
        prio &= !PCATCH;
    }

    sched_lock();

    // TRACEPOINT(sched, sleep, NULL): dt(4), not configured.

    p.p_wchan.set(ident);
    p.p_wmesg.set(Some(wmesg));
    p.p_slptime.set(0);
    p.p_slppri.set((prio & PRIMASK) as u8);
    p.p_flag.fetch_or(P_INSCHED, Ordering::Relaxed);
    // SAFETY: `p` is on no run or sleep queue (it is SONPROC), and stays valid: a thread
    // outlives its sleeps.
    unsafe { SLPQUE.0[lookup(ident)].insert_tail(p) };
    if prio & PCATCH != 0 {
        p.p_flag.fetch_or(P_SINTR, Ordering::Relaxed);
    }
    p.p_stat.set(SSLEEP);

    sched_unlock();
}

/// `sleep_finish`: sleeps (or not) after `sleep_setup`; returns the sleep's error.
pub fn sleep_finish(nsecs: u64, do_sleep: bool) -> Result<(), Errno> {
    let p = sleeper();
    let mut do_sleep = do_sleep;
    let mut error: Result<(), Errno> = Ok(());
    let mut error1: Result<(), Errno> = Ok(());

    #[cfg(feature = "diagnostic")]
    if nsecs == 0 {
        log(
            LOG_WARNING,
            format_args!(
                "sleep_finish: {}[{}]: {}: trying to sleep zero nanoseconds\n",
                Str(p.process().comm()),
                p.process().ps_pid.get(),
                p.p_wmesg.get().unwrap_or("")
            ),
        );
    }

    if nsecs != INFSLP {
        kassert!(p.p_flag.load(Ordering::Relaxed) & (P_TIMEOUT | P_TIMEOUTRAN) == 0);
        timeout_add_nsec(&p.p_sleep_to, nsecs);
    }

    let mut catch = p.p_flag.load(Ordering::Relaxed) & P_SINTR != 0;
    if catch {
        error = sleep_signal_check(p, false);
        if error.is_err() {
            catch = false;
            do_sleep = false;
        }
    }

    sched_lock();
    // A few checks need to happen before going to sleep:
    // - If the wakeup happens while going to sleep, p->p_wchan will be NULL. In that case
    //   unwind immediately but still check for possible signals and timeouts.
    // - If the sleep is aborted call unsleep and take us of the sleep queue.
    // - If requested to stop force a switch even if the sleep condition got cleared.
    if p.p_wchan.get().is_null() {
        do_sleep = false;
    }
    if !do_sleep {
        unsleep(p);
    }
    if p.p_stat.get() == SSTOP {
        do_sleep = true;
    }
    p.p_flag.fetch_and(!P_INSCHED, Ordering::Relaxed);

    if do_sleep {
        kassert!(p.p_stat.get() == SSLEEP || p.p_stat.get() == SSTOP);
        p.p_ru.ru_nvcsw.set(p.p_ru.ru_nvcsw.get() + 1);
        mi_switch();
    } else {
        kassert!(p.p_stat.get() == SONPROC || p.p_stat.get() == SSLEEP);
        p.p_stat.set(SONPROC);
        sched_unlock();
    }

    #[cfg(feature = "diagnostic")]
    if p.p_stat.get() != SONPROC {
        panic(format_args!("sleep_finish !SONPROC"));
    }

    if let Some(ci) = p.cpu() {
        Machine::ci_schedstate(ci)
            .spc_curpriority
            .store(p.p_usrpri.get(), Ordering::Relaxed);
    }

    // Even though this belongs to the signal handling part of sleep, we need to clear it
    // before the ktrace.
    p.p_flag.fetch_and(!P_SINTR, Ordering::Relaxed);

    // There are three situations to handle when cancelling the p_sleep_to timeout:
    //
    // 1. The timeout has not fired yet
    // 2. The timeout is running
    // 3. The timeout has run
    //
    // If timeout_del succeeds then the timeout won't run and situation 1 is dealt with.
    //
    // If timeout_del does not remove the timeout, then we're handling 2 or 3, but it won't
    // tell us which one. Instead, the P_TIMEOUTRAN flag is used to figure out when we move
    // from 2 to 3. endtsleep() (the p_sleep_to handler) sets the flag when it's finished
    // running, so we spin waiting for it.
    //
    // We spin instead of sleeping because endtsleep() takes the sched lock to do all it's
    // work. If we wanted to go to sleep to wait for endtsleep to run, we'd also have to take
    // the sched lock, so we'd be spinning against it anyway.
    if nsecs != INFSLP && !timeout_del(&p.p_sleep_to) {
        // Wait for endtsleep timeout to finish running
        let mut flag;
        loop {
            flag = p.p_flag.load(Ordering::Relaxed);
            if flag & P_TIMEOUTRAN != 0 {
                break;
            }
            core::hint::spin_loop(); // CPU_BUSY_CYCLE()
        }
        p.p_flag
            .fetch_and(!(P_TIMEOUT | P_TIMEOUTRAN), Ordering::Relaxed);

        if flag & P_TIMEOUT != 0 {
            error1 = Err(Errno::EWOULDBLOCK);
        }
    }

    // Check if thread was woken up because of a unwind or signal but ignore any pending stop
    // condition.
    if catch {
        error = sleep_signal_check(p, true);
    }

    // Signal errors are higher priority than timeouts.
    if error.is_ok() && error1.is_err() {
        error = error1;
    }

    error
}

/// `sleep_signal_check`: check and handle signals and suspensions around a sleep cycle. The
/// 2nd call in `sleep_finish()` sets `after_sleep`. In this case any pending suspend event
/// came in after the wakeup / unsleep and can therefor be ignored. Once the process hits
/// userret the event will be picked up again.
pub fn sleep_signal_check(p: &Proc, after_sleep: bool) -> Result<(), Errno> {
    let pr = p.process();
    let mut ctx = Sigctx::default();

    if let Err(err) = proc_suspend_check(p, true) {
        if err != Errno::EWOULDBLOCK {
            return Err(err);
        }

        // requested to stop
        if !after_sleep {
            mtx_enter(&pr.ps_mtx);
            process_suspend_signal(pr);

            sched_lock();
            p.p_stat.set(SSTOP);
            sched_unlock();
            mtx_leave(&pr.ps_mtx);
        }
    }

    let sig = cursig(p, &mut ctx, true);
    if sig != 0 {
        if ctx.sig_stop {
            if !after_sleep {
                mtx_enter(&pr.ps_mtx);
                pr.ps_xsig.set(sig);
                // This is for stop signals delivered before sleep_setup() was called. We need
                // to do the full dance here before going to sleep.
                p.p_siglist.fetch_and(!sigmask(sig), Ordering::Relaxed);
                pr.ps_flags.fetch_or(PS_STOPPING, Ordering::Relaxed);
                sched_lock();
                process_stop(pr, P_SUSPSIG, SINGLE_SUSPEND);
                sched_unlock();
                p.p_flag.fetch_or(P_SUSPSIG, Ordering::Relaxed);
                process_suspend_signal(pr);
                sched_lock();
                p.p_stat.set(SSTOP);
                sched_unlock();
                mtx_leave(&pr.ps_mtx);
            }
        } else if ctx.sig_intr && !ctx.sig_ignore {
            return Err(Errno::EINTR);
        } else {
            return Err(Errno::ERESTART);
        }
    }

    Ok(())
}

/// `wakeup_proc`: if process hasn't been awakened (wchan non-zero), undo the sleep. If proc is
/// stopped, just unsleep so it will remain stopped.
pub fn wakeup_proc(p: &Proc) -> bool {
    let mut awakened = false;

    sched_assert_locked();

    if !p.p_wchan.get().is_null() {
        awakened = true;
        #[cfg(feature = "diagnostic")]
        if p.p_stat.get() != SSLEEP && p.p_stat.get() != SSTOP {
            panic(format_args!(
                "thread {} p_stat is {}",
                p.p_tid.get(),
                p.p_stat.get()
            ));
        }
        unsleep(p);
        if p.p_stat.get() == SSLEEP {
            setrunnable(p);
        }
    }

    awakened
}

/// `endtsleep`: this is the timeout handler that wakes up procs that only want to sleep for a
/// period of time rather than forever (until they get a wakeup from somewhere else). It is
/// only scheduled and used by `sleep_finish()`, which coordinates with this handler via the
/// `P_TIMEOUT` and `P_TIMEOUTRAN` flags.
pub fn endtsleep(arg: *mut c_void) {
    // SAFETY: `arg` is the thread that armed its `p_sleep_to`, alive while it sleeps.
    let p = unsafe { &*arg.cast::<Proc>() };

    sched_lock();
    let awakened = wakeup_proc(p);
    sched_unlock();

    let mut flags = P_TIMEOUTRAN;
    if awakened {
        flags |= P_TIMEOUT;
    }

    // Let sleep_finish() proceed.
    p.p_flag.fetch_or(flags, Ordering::Relaxed);
    // Do not alter the proc after this point.
}

/// `unsleep`: remove a process from its wait queue.
pub fn unsleep(p: &Proc) {
    sched_assert_locked();

    if !p.p_wchan.get().is_null() {
        // SAFETY: a thread with a wchan is on that channel's sleep queue, under the
        // scheduler lock.
        unsafe { SLPQUE.0[lookup(p.p_wchan.get())].remove(p) };
        p.p_wchan.set(ptr::null());
        p.p_wmesg.set(None);
        // TRACEPOINT(sched, unsleep, ...): dt(4), not configured.
    }
}

/// `wakeup_n`: make a number of processes sleeping on the specified identifier runnable.
pub fn wakeup_n<T: ?Sized>(ident: *const T, n: i32) {
    let ident = ident.cast::<c_void>();
    let wakeq: TailqHead<ProcRunq> = TailqHead::new();
    let mut n = n;

    sched_lock();
    let qp = &SLPQUE.0[lookup(ident)];
    let mut next = qp.first();
    while let Some(p) = next
        && n != 0
    {
        next = TailqHead::<ProcRunq>::next(p);
        #[cfg(feature = "diagnostic")]
        if p.p_stat.get() != SSLEEP && p.p_stat.get() != SSTOP {
            panic(format_args!(
                "thread {} p_stat is {}",
                p.p_tid.get(),
                p.p_stat.get()
            ));
        }
        kassert!(!p.p_wchan.get().is_null());
        if ptr::eq(p.p_wchan.get(), ident) {
            // SAFETY: `p` is on `qp`, under the scheduler lock; it moves to `wakeq`, a local
            // queue that is emptied below before it goes out of scope.
            unsafe {
                qp.remove(p);
                p.p_wchan.set(ptr::null());
                p.p_wmesg.set(None);
                wakeq.insert_tail(p);
            }
            n -= 1;
        }
    }
    while let Some(p) = wakeq.first() {
        // SAFETY: `p` is on `wakeq`.
        unsafe { wakeq.remove(p) };
        // TRACEPOINT(sched, unsleep, ...): dt(4), not configured.
        if p.p_stat.get() == SSLEEP {
            setrunnable(p);
        }
    }
    sched_unlock();
}

/// `wakeup`: make all processes sleeping on the specified identifier runnable.
pub fn wakeup<T: ?Sized>(chan: *const T) {
    wakeup_n(chan, -1);
}

/// `sched_yield(2)`: give the CPU up; a thread of a multi-threaded process drops to the
/// lowest priority among its siblings so that they can make some progress.
pub fn sys_sched_yield(p: &Proc, _v: &SysArgs, _retval: &mut [Register; 2]) -> Result<(), Errno> {
    // If one of the threads of a multi-threaded process called sched_yield(2), drop its
    // priority to ensure its siblings can make some progress.
    let pr = p.process();
    mtx_enter(&pr.ps_mtx);
    let mut newprio = p.p_usrpri.get();
    for q in pr.ps_threads.iter() {
        newprio = newprio.max(q.p_runpri.get());
    }
    mtx_leave(&pr.ps_mtx);

    sched_lock();
    setrunqueue(p.cpu(), p, newprio);
    p.p_ru.ru_nvcsw.set(p.p_ru.ru_nvcsw.get() + 1);
    mi_switch();

    Ok(())
}

/// `wakeup_one(c)`: `wakeup_n((c), 1)` (`<sys/systm.h>`).
pub fn wakeup_one<T: ?Sized>(chan: *const T) {
    wakeup_n(chan, 1);
}

// sys_sched_yield, thrsleep_unlock, tslp_init, thrsleep_bucket, thrsleep, sys___thrsleep,
// tslp_wakeups, sys___thrwakeup: the syscalls (M6).

/// `refcnt_init`.
pub fn refcnt_init(r: &Refcnt) {
    refcnt_init_trace(r, 0);
}

/// `refcnt_init_trace`.
pub fn refcnt_init_trace(r: &Refcnt, trace: i32) {
    r.r_traceidx.set(trace);
    r.r_refs.store(1, Ordering::Relaxed);
    // TRACEINDEX(refcnt, ...): dt(4), not configured.
}

/// `refcnt_take`.
pub fn refcnt_take(r: &Refcnt) {
    let refs = r.r_refs.fetch_add(1, Ordering::Relaxed).wrapping_add(1);
    kassert!(refs > 1);
}

/// `refcnt_rele`: drops a reference; `true` when it was the last.
pub fn refcnt_rele(r: &Refcnt) -> bool {
    fence(Ordering::Release); // membar_exit_before_atomic()
    let refs = r.r_refs.fetch_sub(1, Ordering::Relaxed).wrapping_sub(1);
    kassert!(refs != u32::MAX);
    if refs == 0 {
        fence(Ordering::Acquire); // membar_enter_after_atomic()
        return true;
    }
    false
}

/// `refcnt_rele_wake`.
pub fn refcnt_rele_wake(r: &Refcnt) {
    if refcnt_rele(r) {
        wakeup_one(ptr::from_ref(r));
    }
}

/// `refcnt_finalize`: drops the caller's reference and sleeps until every other one is gone.
pub fn refcnt_finalize(r: &Refcnt, wmesg: &'static str) {
    fence(Ordering::Release); // membar_exit_before_atomic()
    let mut refs = r.r_refs.fetch_sub(1, Ordering::Relaxed).wrapping_sub(1);
    kassert!(refs != u32::MAX);
    while refs != 0 {
        sleep_setup(ptr::from_ref(r).cast(), PWAIT, wmesg);
        refs = r.r_refs.load(Ordering::Relaxed);
        let _ = sleep_finish(INFSLP, refs != 0);
    }
    // Order subsequent loads and stores after refs == 0 load.
    fence(Ordering::SeqCst); // membar_sync()
}

/// `refcnt_read`.
pub fn refcnt_read(r: &Refcnt) -> u32 {
    r.r_refs.load(Ordering::Relaxed)
}

/// `refcnt_shared(_r)`.
pub fn refcnt_shared(r: &Refcnt) -> bool {
    refcnt_read(r) > 1
}

/// `cond_init`.
pub fn cond_init(c: &Cond) {
    c.c_wait.store(1, Ordering::Relaxed);
}

/// `cond_signal_handler`: the timeout/task handler that signals a `cond`.
pub fn cond_signal_handler(arg: *mut c_void) {
    // SAFETY: `arg` is the `cond` its waiter armed the handler with, alive while it waits.
    let c = unsafe { &*arg.cast::<Cond>() };

    c.c_wait.store(0, Ordering::Relaxed);

    wakeup_one(ptr::from_ref(c));
}

/// `cond_wait`: sleeps until the cond is signalled.
pub fn cond_wait(c: &Cond, wmesg: &'static str) {
    let mut wait = c.c_wait.load(Ordering::Relaxed);
    while wait != 0 {
        sleep_setup(ptr::from_ref(c).cast(), PWAIT, wmesg);
        wait = c.c_wait.load(Ordering::Relaxed);
        let _ = sleep_finish(INFSLP, wait != 0);
    }
}
