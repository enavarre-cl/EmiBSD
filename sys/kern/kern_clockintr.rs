/* $OpenBSD: kern_clockintr.c,v 1.71 2024/11/07 16:02:29 miod Exp $ */
/* <LICENSES> */
/*
 * Copyright (c) 2003 Dale Rahn <drahn@openbsd.org>
 * Copyright (c) 2020 Mark Kettenis <kettenis@openbsd.org>
 * Copyright (c) 2020-2024 Scott Cheloha <cheloha@openbsd.org>
 *
 * Permission to use, copy, modify, and distribute this software for any
 * purpose with or without fee is hereby granted, provided that the above
 * copyright notice and this permission notice appear in all copies.
 *
 * THE SOFTWARE IS PROVIDED "AS IS" AND THE AUTHOR DISCLAIMS ALL WARRANTIES
 * WITH REGARD TO THIS SOFTWARE INCLUDING ALL IMPLIED WARRANTIES OF
 * MERCHANTABILITY AND FITNESS. IN NO EVENT SHALL THE AUTHOR BE LIABLE FOR
 * ANY SPECIAL, DIRECT, INDIRECT, OR CONSEQUENTIAL DAMAGES OR ANY DAMAGES
 * WHATSOEVER RESULTING FROM LOSS OF USE, DATA OR PROFITS, WHETHER IN AN
 * ACTION OF CONTRACT, NEGLIGENCE OR OTHER TORTIOUS ACTION, ARISING OUT OF
 * OR IN CONNECTION WITH THE USE OR PERFORMANCE OF THIS SOFTWARE.
 */
/* </LICENSES> */

//! Schedulable clock interrupts: `kern/kern_clockintr.c`. Every CPU keeps a queue of
//! `clockintr`s ordered by expiration; the platform's interrupt clock (`intrclock`) is
//! re-armed for the next one, and `clockintr_dispatch` runs the expired ones.
//!
//! Upstream: sys/kern/kern_clockintr.c @ 3ce1f3f79392
//!
//! Status: `wip`. Milestone M5 ports `clockintr_cpu_init`, `clockintr_trigger`,
//! `clockintr_dispatch`, `clockintr_advance`, `clockrequest_advance`,
//! `clockrequest_advance_random`, `clockintr_cancel[_locked]`, `clockintr_bind`,
//! `clockintr_unbind`, `clockintr_schedule[_locked]`, `clockintr_stagger`,
//! `clockintr_hardclock`, `clockqueue_init`, `clockqueue_intrclock_install`,
//! `clockqueue_next`, `clockqueue_pend_delete/insert`, `clockqueue_intrclock_reprogram`,
//! `intrclock_rearm/trigger` and `nsec_advance`. `sysctl_clockintr` is not here yet
//! (`kern_sysctl.rs` reports `kern.clockintr`); the `ddb` `show all clockintr` printers come
//! with the real ddb (M7).
//!
//! ## Deviations
//! - `clockintr_unbind` with `CL_BARRIER` sleeps (`msleep_nsec`) until the running callback
//!   returns: reported as unported until the sleep queues exist (M5-b); nothing unbinds a
//!   running callback before then.
//! - The callback runs with the clock frame as an opaque `*mut c_void`, as in C.
//! - Not a deviation, a reminder: the `cq_stat` time sums keep the C's modular `uint64_t`
//!   arithmetic (`wrapping_add`/`wrapping_sub`). `clockintr_dispatch` can read the clock
//!   behind its own `start`: amd64's only timecounter is the i8254 behind the LAPIC timer,
//!   whose 15-bit count wraps every 27.46 ms, and a clock interrupt held off longer than that
//!   (QEMU's vCPU thread descheduled by the host) loses a period in `tc_delta`. OpenBSD on
//!   the same counter does the same; checked arithmetic would panic where the C goes on.

use core::ffi::c_void;
use core::ptr;
use core::sync::atomic::{Ordering, fence};

use crate::kassert;
use crate::kern::kern_clock::{
    STATCLOCK_AVG, STATCLOCK_IS_RANDOMIZED, hardclock, hardclock_period,
};
use crate::kern::kern_lock::{mtx_enter, mtx_leave};
use crate::kern::kern_synch::wakeup;
use crate::kern::kern_tc::nsecuptime;
use crate::kern::sched_bsd::roundrobin_period;
use crate::kern::subr_prf::panic;
use crate::kern::subr_prof::profclock_period;
use crate::machine::Machine;
use crate::machine::cpu::{Cpu, CpuInfo, MAXCPUS, curcpu};
use crate::machine::intr::{IPL_CLOCK, IPL_NONE, splassert};
use crate::sys::clockintr::{
    CL_FLAG_MASK, CLST_PENDING, CQ_IGNORE_REQUEST, CQ_INIT, CQ_INTRCLOCK, CQ_NEED_WAKEUP,
    CR_RESCHEDULE, Clockintr, ClockintrFn, Clockqueue, Clockrequest, Intrclock,
};
use crate::sys::mutex::mutex_assert_locked;
use crate::sys::queue::TailqHead;
use crate::unported;

/// `cl->cl_queue`: the queue a bound clockintr belongs to.
fn cl_queue(cl: &Clockintr) -> &'static Clockqueue {
    let cq = cl.cl_queue.get();
    if cq.is_null() {
        panic(format_args!("clockintr {:p} is not bound", cl));
    }
    // SAFETY: a bound clockintr's queue is a CPU's `ci_queue`, inside a static `cpu_info`.
    unsafe { &*cq }
}

/// `clockintr_cpu_init`: ready the calling CPU for `clockintr_dispatch()`. If this is our
/// first time here, install the intrclock, if any, and set necessary flags. Advance the
/// schedule as needed.
pub fn clockintr_cpu_init(ic: Option<&Intrclock>) {
    let mut multiplier: u64 = 0;
    let ci = curcpu();
    let cq = Machine::ci_queue(ci);
    let spc = Machine::ci_schedstate(ci);
    let mut reset_cq_intrclock = false;

    if let Some(ic) = ic {
        clockqueue_intrclock_install(cq, ic);
    }

    // TODO: Remove this from struct clockqueue.
    if Machine::cpu_is_primary(ci) && cq.cq_hardclock.cl_expiration.get() == 0 {
        clockintr_bind(&cq.cq_hardclock, ci, clockintr_hardclock, ptr::null_mut());
    }

    // Mask CQ_INTRCLOCK while we're advancing the internal clock interrupts. We don't want
    // the intrclock to fire until this thread reaches clockintr_trigger().
    if cq.cq_flags.get() & CQ_INTRCLOCK != 0 {
        cq.cq_flags.set(cq.cq_flags.get() & !CQ_INTRCLOCK);
        reset_cq_intrclock = true;
    }

    // Until we understand scheduler lock contention better, stagger the hardclock and
    // statclock so they don't all happen at once. If we have no intrclock it doesn't matter,
    // we have no control anyway. The primary CPU's starting offset is always zero, so leave
    // the multiplier zero.
    if !Machine::cpu_is_primary(ci) && reset_cq_intrclock {
        multiplier = u64::from(Machine::cpu_info_unit(ci));
    }

    // The first time we do this, the primary CPU cannot skip any hardclocks. We can skip
    // hardclocks on subsequent calls because the global tick value is advanced during
    // inittodr(9) on our behalf.
    if Machine::cpu_is_primary(ci) {
        if cq.cq_hardclock.cl_expiration.get() == 0 {
            clockintr_schedule(&cq.cq_hardclock, 0);
        } else {
            clockintr_advance(&cq.cq_hardclock, hardclock_period());
        }
    }

    // We can always advance the statclock. There is no reason to stagger a randomized
    // statclock.
    if !STATCLOCK_IS_RANDOMIZED.load(Ordering::Relaxed)
        && spc.spc_statclock.cl_expiration.get() == 0
    {
        clockintr_stagger(
            &spc.spc_statclock,
            STATCLOCK_AVG.load(Ordering::Relaxed),
            multiplier as u32,
            MAXCPUS,
        );
    }
    clockintr_advance(&spc.spc_statclock, STATCLOCK_AVG.load(Ordering::Relaxed));

    // XXX Need to find a better place to do this. We can't do it in sched_init_cpu() because
    // initclocks() runs after it.
    if spc.spc_itimer.cl_expiration.get() == 0 {
        clockintr_stagger(
            &spc.spc_itimer,
            hardclock_period(),
            multiplier as u32,
            MAXCPUS,
        );
    }
    if spc.spc_profclock.cl_expiration.get() == 0 {
        clockintr_stagger(
            &spc.spc_profclock,
            profclock_period(),
            multiplier as u32,
            MAXCPUS,
        );
    }
    if spc.spc_roundrobin.cl_expiration.get() == 0 {
        clockintr_stagger(
            &spc.spc_roundrobin,
            hardclock_period(),
            multiplier as u32,
            MAXCPUS,
        );
    }
    clockintr_advance(&spc.spc_roundrobin, roundrobin_period());

    if reset_cq_intrclock {
        cq.cq_flags.set(cq.cq_flags.get() | CQ_INTRCLOCK);
    }
}

/// `clockintr_trigger`: if we have an intrclock, trigger it to start the dispatch cycle.
pub fn clockintr_trigger() {
    let cq = Machine::ci_queue(curcpu());

    kassert!(cq.cq_flags.get() & CQ_INIT != 0);

    if cq.cq_flags.get() & CQ_INTRCLOCK != 0
        && let Some(ic) = cq.cq_intrclock.get()
    {
        intrclock_trigger(&ic);
    }
}

/// `clockintr_dispatch`: run all expired events scheduled on the calling CPU. Returns 1 when
/// something ran (the interrupt handler's "handled").
pub fn clockintr_dispatch(frame: *mut c_void) -> i32 {
    let mut lateness: u64 = 0;
    let mut run: u64 = 0;
    let ci = curcpu();
    let cq = Machine::ci_queue(ci);
    let request = &cq.cq_request;

    if cq.cq_dispatch.load(Ordering::Relaxed) != 0 {
        panic(format_args!("clockintr_dispatch: recursive dispatch"));
    }
    cq.cq_dispatch.store(1, Ordering::Relaxed);

    splassert(IPL_CLOCK, "clockintr_dispatch");
    kassert!(cq.cq_flags.get() & CQ_INIT != 0);

    mtx_enter(&cq.cq_mtx);

    // If nothing is scheduled or we arrived too early, we have nothing to do.
    let start = nsecuptime();
    cq.cq_uptime.set(start);
    let mut rearm = true;
    if cq.cq_pend.is_empty() {
        rearm = false; // goto stats
    } else if cq.cq_uptime.get() < clockqueue_next(cq) {
        // goto rearm
    } else {
        lateness = start - clockqueue_next(cq);

        // Dispatch expired events.
        while let Some(cl) = cq.cq_pend.first() {
            if cq.cq_uptime.get() < cl.cl_expiration.get() {
                // Double-check the time before giving up.
                cq.cq_uptime.set(nsecuptime());
                if cq.cq_uptime.get() < cl.cl_expiration.get() {
                    break;
                }
            }

            // This clockintr has expired. Execute it.
            clockqueue_pend_delete(cq, cl);
            request.cr_expiration.set(cl.cl_expiration.get());
            let arg = cl.cl_arg.get();
            let func = cl.cl_func.get();
            cq.cq_running.set(cl);
            mtx_leave(&cq.cq_mtx);

            if let Some(func) = func {
                func(request, frame, arg);
            }

            mtx_enter(&cq.cq_mtx);
            cq.cq_running.set(ptr::null());
            if cq.cq_flags.get() & CQ_IGNORE_REQUEST != 0 {
                cq.cq_flags.set(cq.cq_flags.get() & !CQ_IGNORE_REQUEST);
                request
                    .cr_flags
                    .set(request.cr_flags.get() & !CR_RESCHEDULE);
            }
            if request.cr_flags.get() & CR_RESCHEDULE != 0 {
                request
                    .cr_flags
                    .set(request.cr_flags.get() & !CR_RESCHEDULE);
                clockqueue_pend_insert(cq, cl, request.cr_expiration.get());
            }
            if cq.cq_flags.get() & CQ_NEED_WAKEUP != 0 {
                cq.cq_flags.set(cq.cq_flags.get() & !CQ_NEED_WAKEUP);
                mtx_leave(&cq.cq_mtx);
                wakeup(ptr::addr_of!(cq.cq_running));
                mtx_enter(&cq.cq_mtx);
            }
            run += 1;
        }
    }

    // Dispatch complete.
    if rearm {
        // Rearm the interrupt clock if we have one.
        if cq.cq_flags.get() & CQ_INTRCLOCK != 0
            && !cq.cq_pend.is_empty()
            && let Some(ic) = cq.cq_intrclock.get()
        {
            intrclock_rearm(&ic, clockqueue_next(cq) - cq.cq_uptime.get());
        }
    }

    // Update our stats.
    let ogen = cq.cq_gen.load(Ordering::Relaxed);
    cq.cq_gen.store(0, Ordering::Relaxed);
    fence(Ordering::Release); // membar_producer()
    let mut stat = cq.cq_stat.get();
    // The C's uint64_t sums are modular, and this one relies on it: nsecuptime() is only
    // monotonic while hardclock winds the timehands up once per period of the timecounter, so
    // a dispatch held off longer can read the clock behind `start` (see the deviations).
    stat.cs_dispatched = stat
        .cs_dispatched
        .wrapping_add(cq.cq_uptime.get().wrapping_sub(start));
    if run > 0 {
        stat.cs_lateness = stat.cs_lateness.wrapping_add(lateness);
        stat.cs_prompt += 1;
        stat.cs_run += run;
    } else if !cq.cq_pend.is_empty() {
        stat.cs_early += 1;
        stat.cs_earliness = stat
            .cs_earliness
            .wrapping_add(clockqueue_next(cq) - cq.cq_uptime.get());
    } else {
        stat.cs_spurious += 1;
    }
    cq.cq_stat.set(stat);
    fence(Ordering::Release); // membar_producer()
    cq.cq_gen
        .store(ogen.wrapping_add(1).max(1), Ordering::Relaxed);

    mtx_leave(&cq.cq_mtx);

    if cq.cq_dispatch.load(Ordering::Relaxed) != 1 {
        panic(format_args!(
            "clockintr_dispatch: unexpected value: {}",
            cq.cq_dispatch.load(Ordering::Relaxed)
        ));
    }
    cq.cq_dispatch.store(0, Ordering::Relaxed);

    i32::from(run > 0)
}

/// `clockintr_advance`: schedules `cl` for the next multiple of `period` after now; returns
/// how many periods it was advanced.
pub fn clockintr_advance(cl: &Clockintr, period: u64) -> u64 {
    let cq = cl_queue(cl);

    mtx_enter(&cq.cq_mtx);
    let mut expiration = cl.cl_expiration.get();
    let count = nsec_advance(&mut expiration, period, nsecuptime());
    clockintr_schedule_locked(cl, expiration);
    mtx_leave(&cq.cq_mtx);

    count
}

/// `clockrequest_advance`: from a callback, asks to run again `period` after the dispatch
/// time; returns how many periods elapsed.
pub fn clockrequest_advance(cr: &Clockrequest, period: u64) -> u64 {
    // SAFETY: a request is the `cq_request` of a static CPU queue.
    let cq = unsafe { &*cr.cr_queue.get() };

    kassert!(ptr::eq(cr, &cq.cq_request));

    cr.cr_flags.set(cr.cr_flags.get() | CR_RESCHEDULE);
    let mut expiration = cr.cr_expiration.get();
    let count = nsec_advance(&mut expiration, period, cq.cq_uptime.get());
    cr.cr_expiration.set(expiration);
    count
}

/// `clockrequest_advance_random`: as `clockrequest_advance`, with a period of `min` plus a
/// random offset within `mask`.
pub fn clockrequest_advance_random(cr: &Clockrequest, min: u64, mask: u32) -> u64 {
    let mut count: u64 = 0;
    // SAFETY: as for `clockrequest_advance`.
    let cq = unsafe { &*cr.cr_queue.get() };

    kassert!(ptr::eq(cr, &cq.cq_request));

    while cr.cr_expiration.get() <= cq.cq_uptime.get() {
        let mut off;
        loop {
            off = libkern::random(Machine::ci_randseed(curcpu())) & mask;
            if off != 0 {
                break;
            }
        }
        cr.cr_expiration
            .set(cr.cr_expiration.get() + min + u64::from(off));
        count += 1;
    }
    cr.cr_flags.set(cr.cr_flags.get() | CR_RESCHEDULE);
    count
}

/// `clockintr_cancel`.
pub fn clockintr_cancel(cl: &Clockintr) {
    let cq = cl_queue(cl);

    mtx_enter(&cq.cq_mtx);
    clockintr_cancel_locked(cl);
    mtx_leave(&cq.cq_mtx);
}

/// `clockintr_cancel_locked`.
pub fn clockintr_cancel_locked(cl: &Clockintr) {
    let cq = cl_queue(cl);

    mutex_assert_locked(&cq.cq_mtx, "clockintr_cancel_locked");

    if cl.cl_flags.get() & CLST_PENDING != 0 {
        let was_next = cq.cq_pend.first().is_some_and(|f| ptr::eq(f, cl));
        clockqueue_pend_delete(cq, cl);
        if cq.cq_flags.get() & CQ_INTRCLOCK != 0
            && was_next
            && !cq.cq_pend.is_empty()
            && ptr::eq(cq, Machine::ci_queue(curcpu()))
        {
            clockqueue_intrclock_reprogram(cq);
        }
    }
    if ptr::eq(cl, cq.cq_running.get()) {
        cq.cq_flags.set(cq.cq_flags.get() | CQ_IGNORE_REQUEST);
    }
}

/// `clockintr_bind`: attaches `cl` to `ci`'s queue with its callback.
pub fn clockintr_bind(cl: &'static Clockintr, ci: &CpuInfo, func: ClockintrFn, arg: *mut c_void) {
    let cq = Machine::ci_queue(ci);

    splassert(IPL_NONE, "clockintr_bind");
    kassert!(cl.cl_queue.get().is_null());

    mtx_enter(&cq.cq_mtx);
    cl.cl_arg.set(arg);
    cl.cl_func.set(Some(func));
    cl.cl_queue.set(cq);
    // SAFETY: `cl` is static and in no `cq_all`; it stays bound until `clockintr_unbind`.
    unsafe { cq.cq_all.insert_tail(cl) };
    mtx_leave(&cq.cq_mtx);
}

/// `clockintr_unbind`: detaches `cl` from its queue; `CL_BARRIER` waits for a running
/// callback (see the module's deviations).
pub fn clockintr_unbind(cl: &Clockintr, flags: u32) {
    let cq = cl_queue(cl);

    kassert!(flags & !CL_FLAG_MASK == 0);

    mtx_enter(&cq.cq_mtx);

    clockintr_cancel_locked(cl);

    cl.cl_arg.set(ptr::null_mut());
    cl.cl_func.set(None);
    cl.cl_queue.set(ptr::null());
    // SAFETY: `cl` is in `cq_all` since `clockintr_bind`, under the queue's mutex.
    unsafe { cq.cq_all.remove(cl) };

    if flags & crate::sys::clockintr::CL_BARRIER != 0 && ptr::eq(cl, cq.cq_running.get()) {
        cq.cq_flags.set(cq.cq_flags.get() | CQ_NEED_WAKEUP);
        // msleep_nsec(&cq->cq_running, &cq->cq_mtx, PWAIT | PNORELOCK, "clkbar", INFSLP)
        let _ = unported!("clockintr_unbind: msleep_nsec (CL_BARRIER, M5-b)");
        mtx_leave(&cq.cq_mtx);
    } else {
        mtx_leave(&cq.cq_mtx);
    }
}

/// `clockintr_schedule`: schedules `cl` at the absolute uptime `expiration`.
pub fn clockintr_schedule(cl: &Clockintr, expiration: u64) {
    let cq = cl_queue(cl);

    mtx_enter(&cq.cq_mtx);
    clockintr_schedule_locked(cl, expiration);
    mtx_leave(&cq.cq_mtx);
}

/// `clockintr_schedule_locked`.
pub fn clockintr_schedule_locked(cl: &Clockintr, expiration: u64) {
    let cq = cl_queue(cl);

    mutex_assert_locked(&cq.cq_mtx, "clockintr_schedule_locked");

    if cl.cl_flags.get() & CLST_PENDING != 0 {
        clockqueue_pend_delete(cq, cl);
    }
    clockqueue_pend_insert(cq, cl, expiration);
    if cq.cq_flags.get() & CQ_INTRCLOCK != 0
        && cq.cq_pend.first().is_some_and(|f| ptr::eq(f, cl))
        && ptr::eq(cq, Machine::ci_queue(curcpu()))
    {
        clockqueue_intrclock_reprogram(cq);
    }
    if ptr::eq(cl, cq.cq_running.get()) {
        cq.cq_flags.set(cq.cq_flags.get() | CQ_IGNORE_REQUEST);
    }
}

/// `clockintr_stagger`: sets an unscheduled `cl`'s expiration to `period * numer / denom`.
pub fn clockintr_stagger(cl: &Clockintr, period: u64, numer: u32, denom: u32) {
    let cq = cl_queue(cl);

    kassert!(numer < denom);

    mtx_enter(&cq.cq_mtx);
    if cl.cl_flags.get() & CLST_PENDING != 0 {
        panic(format_args!("clockintr_stagger: clock interrupt pending"));
    }
    cl.cl_expiration
        .set(period / u64::from(denom) * u64::from(numer));
    mtx_leave(&cq.cq_mtx);
}

/// `clockintr_hardclock`: the hardclock's clockintr callback.
pub fn clockintr_hardclock(cr: &Clockrequest, frame: *mut c_void, _arg: *mut c_void) {
    let count = clockrequest_advance(cr, hardclock_period());
    for _ in 0..count {
        hardclock(frame.cast());
    }
}

/// `clockqueue_init`: readies a CPU's queue, once.
pub fn clockqueue_init(cq: &Clockqueue) {
    if cq.cq_flags.get() & CQ_INIT != 0 {
        return;
    }

    cq.cq_request.cr_queue.set(cq);
    crate::kern::kern_lock::mtx_init(&cq.cq_mtx, IPL_CLOCK);
    cq.cq_all.init();
    cq.cq_pend.init();
    cq.cq_gen.store(1, Ordering::Relaxed);
    cq.cq_flags.set(cq.cq_flags.get() | CQ_INIT);
}

/// `clockqueue_intrclock_install`: installs the platform's interrupt clock, once.
pub fn clockqueue_intrclock_install(cq: &Clockqueue, ic: &Intrclock) {
    mtx_enter(&cq.cq_mtx);
    if cq.cq_flags.get() & CQ_INTRCLOCK == 0 {
        cq.cq_intrclock.set(Some(*ic));
        cq.cq_flags.set(cq.cq_flags.get() | CQ_INTRCLOCK);
    }
    mtx_leave(&cq.cq_mtx);
}

/// `clockqueue_next`: the expiration of the first pending clockintr.
pub fn clockqueue_next(cq: &Clockqueue) -> u64 {
    mutex_assert_locked(&cq.cq_mtx, "clockqueue_next");
    let Some(first) = cq.cq_pend.first() else {
        panic(format_args!("clockqueue_next: nothing pending"));
    };
    first.cl_expiration.get()
}

/// `clockqueue_pend_delete`.
pub fn clockqueue_pend_delete(cq: &Clockqueue, cl: &Clockintr) {
    mutex_assert_locked(&cq.cq_mtx, "clockqueue_pend_delete");
    kassert!(cl.cl_flags.get() & CLST_PENDING != 0);

    // SAFETY: `CLST_PENDING` says `cl` is in `cq_pend`, under the queue's mutex.
    unsafe { cq.cq_pend.remove(cl) };
    cl.cl_flags.set(cl.cl_flags.get() & !CLST_PENDING);
}

/// `clockqueue_pend_insert`: inserts `cl` into `cq_pend` in expiration order.
pub fn clockqueue_pend_insert(cq: &Clockqueue, cl: &Clockintr, expiration: u64) {
    mutex_assert_locked(&cq.cq_mtx, "clockqueue_pend_insert");
    kassert!(cl.cl_flags.get() & CLST_PENDING == 0);

    cl.cl_expiration.set(expiration);
    let elm = cq
        .cq_pend
        .iter()
        .find(|elm| cl.cl_expiration.get() < elm.cl_expiration.get());
    // SAFETY: `cl` is bound (in `cq_all`, so it lives until unbound) and not in `cq_pend`;
    // `elm` is a linked element of `cq_pend`; all under the queue's mutex.
    unsafe {
        match elm {
            None => cq.cq_pend.insert_tail(cl),
            Some(elm) => TailqHead::<crate::sys::clockintr::ClockintrPend>::insert_before(elm, cl),
        }
    }
    cl.cl_flags.set(cl.cl_flags.get() | CLST_PENDING);
}

/// `clockqueue_intrclock_reprogram`: arms the interrupt clock for the first pending
/// clockintr, or triggers it if that is already due.
pub fn clockqueue_intrclock_reprogram(cq: &Clockqueue) {
    mutex_assert_locked(&cq.cq_mtx, "clockqueue_intrclock_reprogram");
    kassert!(cq.cq_flags.get() & CQ_INTRCLOCK != 0);

    let exp = clockqueue_next(cq);
    let now = nsecuptime();
    let Some(ic) = cq.cq_intrclock.get() else {
        return;
    };
    if now < exp {
        intrclock_rearm(&ic, exp - now);
    } else {
        intrclock_trigger(&ic);
    }
}

/// `intrclock_rearm`.
pub fn intrclock_rearm(ic: &Intrclock, nsecs: u64) {
    (ic.ic_rearm)(ic.ic_cookie, nsecs);
}

/// `intrclock_trigger`.
pub fn intrclock_trigger(ic: &Intrclock) {
    (ic.ic_trigger)(ic.ic_cookie);
}

/// `nsec_advance`: advance `*next` in increments of `period` until it exceeds `now`. Returns
/// the number of increments `*next` was advanced.
///
/// We check the common cases first to avoid division if possible. This does no overflow
/// checking.
pub fn nsec_advance(next: &mut u64, period: u64, now: u64) -> u64 {
    if now < *next {
        return 0;
    }

    if now < next.wrapping_add(period) {
        *next = next.wrapping_add(period);
        return 1;
    }

    let elapsed = (now - *next) / period + 1;
    *next = next.wrapping_add(period.wrapping_mul(elapsed));
    elapsed
}

#[cfg(test)]
mod tests;
