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
//! schedules; part b2 adds the `SCHED_LOCK`/`SCHED_UNLOCK`/`SCHED_ASSERT_*` macros of
//! `<sys/sched.h>`, the load average (`cexp`, `averunnable`, `update_loadavg`), `ccpu`,
//! `schedcpu`, `decay_aftersleep`, `yield`, `preempt`, `mi_switch`, `setrunnable`,
//! `setpriority`, `schedclock` and `scheduler_start`. The CPU throttling (`cpu_setperf`,
//! `perflevel`, the `perfpolicy_*` knobs, `setperf_auto`, `sysctl_hwsetperf`,
//! `sysctl_hwperfpolicy`) needs sysctl and the power sensors (M7). M11a: `mi_switch` calls
//! `smr_idle` and, with `MULTIPROCESSOR`, drops every hold of the kernel lock around the
//! switch (`__mp_release_all`/`__mp_acquire_count`).
//!
//! ## Deviations
//! - `averunnable.ldavg` are atomics (`Averunnable`, written by the softclock thread, read
//!   racily by `schedcpu` and sysctl as in C); `averunnable()` returns a `Loadavg` snapshot.
//! - `cexp` and `ccpu` are compile-time constants, like the C's constant-folded doubles; no
//!   floating point reaches the kernel.
//! - `yield` is a Rust keyword: the function is the raw identifier `r#yield`.

use core::ffi::c_void;
use core::ptr;
use core::sync::atomic::{AtomicU32, AtomicU64, Ordering};

use crate::kassert;
use crate::kern::kern_clock::{STATHZ, hardclock_period};
use crate::kern::kern_clockintr::{clockintr_advance, clockintr_cancel, clockrequest_advance};
#[cfg(feature = "multiprocessor")]
use crate::kern::kern_lock::{
    __mp_acquire_count, __mp_release_all, _kernel_lock_held, KERNEL_LOCK,
};
use crate::kern::kern_lock::{mtx_enter, mtx_init, mtx_leave};
use crate::kern::kern_proc::ALLPROC;
use crate::kern::kern_resource::tuagg_add_runtime;
use crate::kern::kern_sched::{
    SCHED_ALL_CPUS, SCHED_IDLE_CPUS, cpuset_cardinality, cpuset_complement, remrunqueue,
    sched_chooseproc, setrunqueue,
};
use crate::kern::kern_smr::smr_idle;
use crate::kern::kern_tc::nanouptime;
use crate::kern::kern_timeout::timeout_add_sec;
use crate::kern::subr_prf::panic;
use crate::kern::subr_prof::profclock_period;
use crate::kern::subr_xxx::assertwaitok;
use crate::machine::Machine;
use crate::machine::cpu::{Cpu, cpu_info_foreach, curcpu, curproc, need_resched};
use crate::machine::intr::{IPL_NONE, IPL_SCHED};
use crate::sys::clockintr::Clockrequest;
use crate::sys::mutex::{Mutex, mutex_assert_locked, mutex_assert_unlocked};
use crate::sys::param::{FSCALE, FSHIFT, MAXPRI, NZERO, PUSER};
use crate::sys::proc::{P_INSCHED, PS_ITIMER, PS_PROFIL, Proc, SONPROC, SRUN, SSLEEP, SSTOP};
use crate::sys::resource::Loadavg;
use crate::sys::sched::{
    NICE_WEIGHT, SCHED_PPQ, SPCF_ITIMER, SPCF_PROFCLOCK, SPCF_SEENRR, SPCF_SHOULDYIELD,
    SPCF_SWITCHCLEAR, estcpulim,
};
use crate::sys::timeout::Timeout;
use crate::sys::types::Fixpt;
use crate::uvm::uvm_init::UVMEXP;

/// \[I\] `roundrobin_period`: roundrobin period (ns).
pub static ROUNDROBIN_PERIOD: AtomicU64 = AtomicU64::new(0);

/// `sched_lock`: initialised by `SCHED_LOCK_INIT()` in `main`.
pub static SCHED_LOCK: Mutex = Mutex::new(IPL_NONE);

/// `cexp[3]`: constants for averages over 1, 5, and 15 minutes when sampling at 5 second
/// intervals.
static CEXP: [Fixpt; 3] = [
    (0.920_044_414_629_323_2 * FSCALE as f64) as Fixpt, /* exp(-1/12) */
    (0.983_471_453_821_617_4 * FSCALE as f64) as Fixpt, /* exp(-1/60) */
    (0.994_459_848_004_896_7 * FSCALE as f64) as Fixpt, /* exp(-1/180) */
];

/// `struct loadavg averunnable` with atomic averages (see the module's deviations).
pub struct Averunnable {
    /// `ldavg`: the 1, 5 and 15 minute averages, fixed point.
    pub ldavg: [AtomicU32; 3],
    /// `fscale`: the scale of `ldavg`.
    pub fscale: i64,
}

/// `averunnable`: the load average.
pub static AVERUNNABLE: Averunnable = Averunnable {
    ldavg: [const { AtomicU32::new(0) }; 3],
    fscale: FSCALE as i64,
};

/// `averunnable` as the C struct, for sysctl and the like.
pub fn averunnable() -> Loadavg {
    Loadavg {
        ldavg: [
            AVERUNNABLE.ldavg[0].load(Ordering::Relaxed),
            AVERUNNABLE.ldavg[1].load(Ordering::Relaxed),
            AVERUNNABLE.ldavg[2].load(Ordering::Relaxed),
        ],
        fscale: AVERUNNABLE.fscale,
    }
}

/// `ccpu`: decay 95% of `p_pctcpu` in 60 seconds; see `CCPU_SHIFT` before changing.
/// exp(-1/20): the C writes `0.95122942450071400909`, more digits than a double keeps.
pub const CCPU: Fixpt = (0.951_229_424_500_714 * FSCALE as f64) as Fixpt;

/// `CCPU_SHIFT`: if `ccpu` is not equal to `exp(-1/20)` and you still want to use the
/// faster/more-accurate formula, you'll have to estimate `CCPU_SHIFT` below and possibly
/// adjust `FSHIFT` in "param.h" so that (`FSHIFT >= CCPU_SHIFT`).
///
/// To estimate `CCPU_SHIFT` for exp(-1/20), the following formula was used:
/// 1 - exp(-1/20) ~= 0.0487 ~= 0.0488 == 1 (fixed pt, *11* bits).
///
/// If you don't want to bother with the faster/more-accurate formula, you can set
/// `CCPU_SHIFT` to (`FSHIFT + 1`) which will use a slower/less-accurate (more general)
/// method of calculating the %age of CPU used by a process.
const CCPU_SHIFT: u32 = 11;

/// `roundrobin_period`.
pub fn roundrobin_period() -> u64 {
    ROUNDROBIN_PERIOD.load(Ordering::Relaxed)
}

/// `SCHED_LOCK_INIT()`.
pub fn sched_lock_init() {
    mtx_init(&SCHED_LOCK, IPL_SCHED);
}

/// `SCHED_LOCK()`.
pub fn sched_lock() {
    mtx_enter(&SCHED_LOCK);
}

/// `SCHED_UNLOCK()`.
pub fn sched_unlock() {
    mtx_leave(&SCHED_LOCK);
}

/// `SCHED_ASSERT_LOCKED()`.
pub fn sched_assert_locked() {
    mutex_assert_locked(&SCHED_LOCK, "SCHED_ASSERT_LOCKED");
}

/// `SCHED_ASSERT_UNLOCKED()`.
pub fn sched_assert_unlocked() {
    mutex_assert_unlocked(&SCHED_LOCK, "SCHED_ASSERT_UNLOCKED");
}

/// `roundrobin`: force switch among equal priority processes every 100ms.
pub fn roundrobin(cr: &Clockrequest, _cf: *mut c_void, _arg: *mut c_void) {
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

    if spc.spc_nrun.load(Ordering::Relaxed) != 0
        || spc.spc_schedflags.load(Ordering::Relaxed) & SPCF_SHOULDYIELD != 0
    {
        need_resched(ci);
    }
}

/// `update_loadavg`: compute a tenex style load average of a quantity on 1, 5, and 15
/// minute intervals.
pub fn update_loadavg(_unused: *mut c_void) {
    static TO: Timeout = Timeout::new(update_loadavg, ptr::null_mut());

    let set = cpuset_complement(&SCHED_IDLE_CPUS, &SCHED_ALL_CPUS);
    let mut nrun = u64::from(cpuset_cardinality(&set));
    cpu_info_foreach(&mut |ci| {
        nrun += u64::from(Machine::ci_schedstate(ci).spc_nrun.load(Ordering::Relaxed));
    });

    for (ldavg, &cexp) in AVERUNNABLE.ldavg.iter().zip(CEXP.iter()) {
        let old = u64::from(ldavg.load(Ordering::Relaxed));
        let new =
            (u64::from(cexp) * old + nrun * u64::from(FSCALE) * u64::from(FSCALE - cexp)) >> FSHIFT;
        ldavg.store(new as Fixpt, Ordering::Relaxed);
    }

    timeout_add_sec(&TO, 5);
}

/*
 * Constants for digital decay and forget:
 *	90% of (p_estcpu) usage in 5 * loadav time
 *	95% of (p_pctcpu) usage in 60 seconds (load insensitive)
 *          Note that, as ps(1) mentions, this can let percentages
 *          total over 100% (I've seen 137.9% for 3 processes).
 *
 * Note that p_estcpu and p_cpticks are updated independently.
 *
 * We wish to decay away 90% of p_estcpu in (5 * loadavg) seconds.
 * That is, the system wants to compute a value of decay such
 * that the following for loop:
 * 	for (i = 0; i < (5 * loadavg); i++)
 * 		p_estcpu *= decay;
 * will compute
 * 	p_estcpu *= 0.1;
 * for all values of loadavg:
 *
 * Mathematically this loop can be expressed by saying:
 * 	decay ** (5 * loadavg) ~= .1
 *
 * The system computes decay as:
 * 	decay = (2 * loadavg) / (2 * loadavg + 1)
 *
 * We wish to prove that the system's computation of decay
 * will always fulfill the equation:
 * 	decay ** (5 * loadavg) ~= .1
 *
 * If we compute b as:
 * 	b = 2 * loadavg
 * then
 * 	decay = b / (b + 1)
 *
 * We now need to prove two things:
 *	1) Given factor ** (5 * loadavg) ~= .1, prove factor == b/(b+1)
 *	2) Given b/(b+1) ** power ~= .1, prove power == (5 * loadavg)
 *
 * Facts:
 *         For x close to zero, exp(x) =~ 1 + x, since
 *              exp(x) = 0! + x**1/1! + x**2/2! + ... .
 *              therefore exp(-1/b) =~ 1 - (1/b) = (b-1)/b.
 *         For x close to zero, ln(1+x) =~ x, since
 *              ln(1+x) = x - x**2/2 + x**3/3 - ...     -1 < x < 1
 *              therefore ln(b/(b+1)) = ln(1 - 1/(b+1)) =~ -1/(b+1).
 *         ln(.1) =~ -2.30
 *
 * Proof of (1):
 *    Solve (factor)**(power) =~ .1 given power (5*loadav):
 *	solving for factor,
 *      ln(factor) =~ (-2.30/5*loadav), or
 *      factor =~ exp(-1/((5/2.30)*loadav)) =~ exp(-1/(2*loadav)) =
 *          exp(-1/b) =~ (b-1)/b =~ b/(b+1).                    QED
 *
 * Proof of (2):
 *    Solve (factor)**(power) =~ .1 given factor == (b/(b+1)):
 *	solving for power,
 *      power*ln(b/(b+1)) =~ -2.30, or
 *      power =~ 2.3 * (b + 1) = 4.6*loadav + 2.3 =~ 5*loadav.  QED
 *
 * Actual power values for the implemented algorithm are as follows:
 *      loadav: 1       2       3       4
 *      power:  5.68    10.32   14.94   19.55
 */

/// `loadfactor(loadav)`: calculations for digital decay to forget 90% of usage in 5*loadav
/// sec.
const fn loadfactor(loadav: Fixpt) -> Fixpt {
    2 * loadav
}

/// `decay_cpu(loadfac, cpu)`.
const fn decay_cpu(loadfac: Fixpt, cpu: u32) -> u32 {
    ((loadfac as u64 * cpu as u64) / (loadfac as u64 + FSCALE as u64)) as u32
}

/// `schedcpu`: recompute process priorities, every second.
pub fn schedcpu(_unused: *mut c_void) {
    static TO: Timeout = Timeout::new(schedcpu, ptr::null_mut());

    let loadfac = loadfactor(AVERUNNABLE.ldavg[0].load(Ordering::Relaxed));
    let stathz = STATHZ.load(Ordering::Relaxed).max(1) as u32;

    for p in ALLPROC.0.iter() {
        // Idle threads are never placed on the runqueue, therefore computing their priority
        // is pointless.
        if let Some(ci) = p.cpu()
            && ptr::eq(Machine::ci_schedstate(ci).spc_idleproc.get(), p)
        {
            continue;
        }
        // Increment sleep time (if sleeping). We ignore overflow.
        if p.p_stat.get() == SSLEEP || p.p_stat.get() == SSTOP {
            p.p_slptime.set(p.p_slptime.get().wrapping_add(1));
        }
        let mut pctcpu =
            ((u64::from(p.p_pctcpu.load(Ordering::Relaxed)) * u64::from(CCPU)) >> FSHIFT) as Fixpt;
        // If the process has slept the entire second, stop recalculating its priority until
        // it wakes up.
        if p.p_slptime.get() > 1 {
            p.p_pctcpu.store(pctcpu, Ordering::Relaxed);
            continue;
        }
        sched_lock();
        // p_pctcpu is only for diagnostic tools such as ps.
        let cpt = p.p_cpticks.get(); // READ_ONCE
        let dticks = cpt.wrapping_sub(p.p_cpticks2.get());
        // FSHIFT >= CCPU_SHIFT
        pctcpu = pctcpu.wrapping_add(if stathz == 100 {
            dticks << (FSHIFT - CCPU_SHIFT)
        } else {
            100u32.wrapping_mul(dticks << (FSHIFT - CCPU_SHIFT)) / stathz
        });
        p.p_pctcpu.store(pctcpu, Ordering::Relaxed);
        p.p_cpticks2.set(cpt);
        let newcpu = decay_cpu(loadfac, p.p_estcpu.get());
        setpriority(p, newcpu, p.process().ps_nice.get());
        if p.p_stat.get() == SRUN
            && u32::from(p.p_runpri.get()) / SCHED_PPQ != u32::from(p.p_usrpri.get()) / SCHED_PPQ
        {
            remrunqueue(p);
            setrunqueue(p.cpu(), p, p.p_usrpri.get());
        }
        sched_unlock();
    }
    timeout_add_sec(&TO, 1);
}

/// `decay_aftersleep`: recalculate the priority of a process after it has slept for a
/// while. For all load averages >= 1 and max `p_estcpu` of 255, sleeping for at least six
/// times the loadfactor will decay `p_estcpu` to zero.
pub fn decay_aftersleep(estcpu: u32, slptime: u32) -> u32 {
    let loadfac = loadfactor(AVERUNNABLE.ldavg[0].load(Ordering::Relaxed));

    if slptime > 5 * loadfac {
        return 0;
    }
    let mut newcpu = estcpu;
    let mut slptime = slptime.wrapping_sub(1); // the first time was done in schedcpu
    while newcpu != 0 {
        slptime = slptime.wrapping_sub(1);
        if slptime == 0 {
            break;
        }
        newcpu = decay_cpu(loadfac, newcpu);
    }
    newcpu
}

/// `yield`: general yield call. Puts the current process back on its run queue and performs
/// a voluntary context switch.
pub fn r#yield() {
    let p = current();

    sched_lock();
    setrunqueue(p.cpu(), p, p.p_usrpri.get());
    p.p_ru.ru_nvcsw.set(p.p_ru.ru_nvcsw.get() + 1);
    mi_switch();
}

/// `preempt`: general preemption call. Puts the current process back on its run queue and
/// performs an involuntary context switch. If a process is supplied, we switch to that
/// process. Otherwise, we use the normal process selection criteria.
pub fn preempt() {
    let p = current();

    sched_lock();
    setrunqueue(p.cpu(), p, p.p_usrpri.get());
    p.p_ru.ru_nivcsw.set(p.p_ru.ru_nivcsw.get() + 1);
    mi_switch();
}

/// `curproc`, which the scheduler's callers always have.
fn current() -> &'static Proc {
    let Some(p) = curproc() else {
        panic(format_args!("scheduler: no curproc"));
    };
    p
}

/// `mi_switch`: the machine-independent half of a context switch. Called with the scheduler
/// lock held and `p_stat` already changed from `SONPROC`; returns on the same thread once it
/// is scheduled again, with the scheduler lock released.
pub fn mi_switch() {
    let ci = curcpu();
    let spc = Machine::ci_schedstate(ci);
    let p = current();

    kassert!(p.p_stat.get() != SONPROC);
    sched_assert_locked();

    // Release the kernel_lock, as we are about to yield the CPU.
    #[cfg(feature = "multiprocessor")]
    let hold_count = if _kernel_lock_held() {
        __mp_release_all(&KERNEL_LOCK)
    } else {
        0
    };

    // Update thread runtime
    tuagg_add_runtime();

    // Stop any optional clock interrupts.
    if spc.spc_schedflags.load(Ordering::Relaxed) & SPCF_ITIMER != 0 {
        spc.spc_schedflags
            .fetch_and(!SPCF_ITIMER, Ordering::Relaxed);
        clockintr_cancel(&spc.spc_itimer);
    }
    if spc.spc_schedflags.load(Ordering::Relaxed) & SPCF_PROFCLOCK != 0 {
        spc.spc_schedflags
            .fetch_and(!SPCF_PROFCLOCK, Ordering::Relaxed);
        clockintr_cancel(&spc.spc_profclock);
    }

    // Process is about to yield the CPU; clear the appropriate scheduling flags.
    spc.spc_schedflags
        .fetch_and(!SPCF_SWITCHCLEAR, Ordering::Relaxed);

    let nextproc = sched_chooseproc();

    // preserve old IPL level so we can switch back to that
    let oldipl = SCHED_LOCK.mtx_oldipl.get();

    if !ptr::eq(p, nextproc) {
        UVMEXP.swtch.fetch_add(1, Ordering::Relaxed);
        // TRACEPOINT(sched, off__cpu, ...): dt(4), not configured.
        // SAFETY: `p` is the running thread, `nextproc` a runnable one the scheduler just
        // took off the run queue (or idle); both have a kernel stack and a pcb, and the
        // scheduler lock serialises the switch.
        unsafe { Machine::cpu_switchto(Some(p), nextproc) };
        // TRACEPOINT(sched, on__cpu, NULL).
    } else {
        // TRACEPOINT(sched, remain__cpu, NULL).
        p.p_stat.set(SONPROC);
    }

    Machine::clear_resched(curcpu());

    sched_assert_locked();

    // Restore proc's IPL.
    SCHED_LOCK.mtx_oldipl.set(oldipl);
    sched_unlock();

    sched_assert_unlocked();

    assertwaitok();
    smr_idle();

    // We're running again; record our new start time. We might be running on a new CPU
    // now, so refetch the schedstate_percpu pointer.
    kassert!(ptr::eq(p.p_cpu.get(), curcpu()));
    let spc = Machine::ci_schedstate(curcpu());

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

    // Reacquire the kernel_lock now. We do this after we've released the scheduler lock to
    // avoid deadlock, and before we reacquire the interlock and the scheduler lock.
    #[cfg(feature = "multiprocessor")]
    if hold_count != 0 {
        __mp_acquire_count(&KERNEL_LOCK, hold_count);
    }
}

/// `setrunnable`: change process state to be runnable, placing it on the run queue.
pub fn setrunnable(p: &Proc) {
    let pr = p.process();

    sched_assert_locked();

    let prio = match p.p_stat.get() {
        SSTOP => {
            let prio = p.p_usrpri.get();
            // TRACEPOINT(sched, unstop, ...): dt(4), not configured.
            // If not yet stopped or asleep, unstop but don't add to runq
            if p.p_flag.load(Ordering::Relaxed) & P_INSCHED != 0 {
                if !p.p_wchan.get().is_null() {
                    p.p_stat.set(SSLEEP);
                } else {
                    p.p_stat.set(SONPROC);
                }
                return;
            }
            setrunqueue(None, p, prio);
            prio
        }
        SSLEEP => {
            let prio = p.p_slppri.get();
            // TRACEPOINT(sched, wakeup, ...): dt(4), not configured.
            // if not yet asleep, don't add to runqueue
            if p.p_flag.load(Ordering::Relaxed) & P_INSCHED != 0 {
                return;
            }
            setrunqueue(None, p, prio);
            prio
        }
        // 0, SRUN, SONPROC, SDEAD, SIDL and anything else
        _ => panic(format_args!("setrunnable")),
    };
    let _ = prio;

    if p.p_slptime.get() > 1 {
        let newcpu = decay_aftersleep(p.p_estcpu.get(), p.p_slptime.get());
        setpriority(p, newcpu, pr.ps_nice.get());
    }
    p.p_slptime.set(0);
}

/// `setpriority`: compute the priority of a process.
pub fn setpriority(p: &Proc, newcpu: u32, nice: u8) {
    let newprio = (i64::from(PUSER)
        + i64::from(newcpu)
        + i64::from(NICE_WEIGHT) * (i64::from(nice) - i64::from(NZERO)))
    .min(i64::from(MAXPRI));

    sched_assert_locked();
    p.p_estcpu.set(newcpu);
    p.p_usrpri.set(newprio.clamp(0, 255) as u8);
}

/// `schedclock`: we adjust the priority of the current process. The priority of a process
/// gets worse as it accumulates CPU time. The cpu usage estimator (`p_estcpu`) is increased
/// here. The formula for computing priorities (in `kern_synch.c`) will compute a different
/// value each time `p_estcpu` increases. This can cause a switch, but unless the priority
/// crosses a PPQ boundary the actual queue will not change. The cpu usage estimator ramps
/// up quite quickly when the process is running (linearly), and decays away exponentially,
/// at a rate which is proportionally slower when the system is busy. The basic principle is
/// that the system will 90% forget that the process used a lot of CPU time in 5 * loadav
/// seconds. This causes the system to favor processes which haven't run much recently, and
/// to round-robin among other processes.
pub fn schedclock(p: &Proc) {
    let ci = curcpu();
    let spc = Machine::ci_schedstate(ci);

    if ptr::eq(p, spc.spc_idleproc.get()) || spc.spc_spinning.load(Ordering::Relaxed) != 0 {
        return;
    }

    sched_lock();
    let newcpu = estcpulim(p.p_estcpu.get() + 1);
    setpriority(p, newcpu, p.process().ps_nice.get());
    sched_unlock();
}

// cpu_setperf, perflevel, perfpolicy_on_ac, perfpolicy_on_battery, setperf_auto,
// sysctl_hwsetperf, sysctl_hwperfpolicy: CPU throttling (sysctl and hw_power, M7).

/// `scheduler_start`: start the scheduler's periodic timeouts.
pub fn scheduler_start() {
    schedcpu(ptr::null_mut());
    update_loadavg(ptr::null_mut());

    // perfpolicy_dynamic() -> timeout_add_msec(&setperf_to, 200): CPU throttling (M7).
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn decay_constants_match_the_c_doubles() {
        // 0.9200444146293232 * 2048, 0.9834714538216174 * 2048, 0.9944598480048967 * 2048
        assert_eq!(CEXP, [1884, 2014, 2036]);
        // 0.95122942450071400909 * 2048
        assert_eq!(CCPU, 1948);
    }

    #[test]
    fn decay_cpu_forgets_ninety_percent_in_five_loadav() {
        // loadav 1.0: loadfac 2*FSCALE; after 5 decays 100 -> about 10.
        let loadfac = loadfactor(FSCALE);
        let mut cpu = 100;
        for _ in 0..5 {
            cpu = decay_cpu(loadfac, cpu);
        }
        assert!((8..=16).contains(&cpu), "{cpu}");
    }

    #[test]
    fn decay_aftersleep_zeroes_long_sleeps() {
        AVERUNNABLE.ldavg[0].store(FSCALE, Ordering::Relaxed);
        assert_eq!(decay_aftersleep(200, 5 * loadfactor(FSCALE) + 1), 0);
        assert!(decay_aftersleep(200, 3) < 200);
        AVERUNNABLE.ldavg[0].store(0, Ordering::Relaxed);
    }
}
