/*	$OpenBSD: kern_timeout.c,v 1.112 2025/07/28 05:25:44 dlg Exp $	*/
/* <LICENSES> */
/*
 * Copyright (c) 2001 Thomas Nordin <nordin@openbsd.org>
 * Copyright (c) 2000-2001 Artur Grabowski <art@openbsd.org>
 * All rights reserved.
 *
 * Redistribution and use in source and binary forms, with or without
 * modification, are permitted provided that the following conditions
 * are met:
 *
 * 1. Redistributions of source code must retain the above copyright
 *    notice, this list of conditions and the following disclaimer.
 * 2. The name of the author may not be used to endorse or promote products
 *    derived from this software without specific prior written permission.
 *
 * THIS SOFTWARE IS PROVIDED ``AS IS'' AND ANY EXPRESS OR IMPLIED WARRANTIES,
 * INCLUDING, BUT NOT LIMITED TO, THE IMPLIED WARRANTIES OF MERCHANTABILITY
 * AND FITNESS FOR A PARTICULAR PURPOSE ARE DISCLAIMED. IN NO EVENT SHALL
 * THE AUTHOR BE LIABLE FOR ANY DIRECT, INDIRECT, INCIDENTAL, SPECIAL,
 * EXEMPLARY, OR CONSEQUENTIAL  DAMAGES (INCLUDING, BUT NOT LIMITED TO,
 * PROCUREMENT OF SUBSTITUTE GOODS OR SERVICES; LOSS OF USE, DATA, OR PROFITS;
 * OR BUSINESS INTERRUPTION) HOWEVER CAUSED AND ON ANY THEORY OF LIABILITY,
 * WHETHER IN CONTRACT, STRICT LIABILITY, OR TORT (INCLUDING NEGLIGENCE OR
 * OTHERWISE) ARISING IN ANY WAY OUT OF THE USE OF THIS SOFTWARE, EVEN IF
 * ADVISED OF THE POSSIBILITY OF SUCH DAMAGE.
 */
/* </LICENSES> */

//! `timeout(9)`: `kern/kern_timeout.c`. Timeouts are kept in a hierarchical timing wheel.
//! The `to_time` is the value of the global variable `ticks` when the timeout should be
//! called. There are four levels with 256 buckets each.
//!
//! Upstream: sys/kern/kern_timeout.c @ 3ce1f3f79392
//!
//! Status: `wip`. Milestone M5 ports the circular queue, `timeout_startup`,
//! `timeout_proc_init` (as far as the soft interrupt), `timeout_set[_flags|_proc]`,
//! `timeout_add[_sec|_msec|_usec|_nsec]`, `timeout_abs_ts`, `timeout_del`,
//! `timeout_del_barrier`, `timeout_barrier` (reported), `timeout_bucket`,
//! `timeout_maskwheel`, `timeout_hardclock_update`, `timeout_run`,
//! `softclock_process_{kclock,tick}_timeout`, `softclock` and `timeout_adjust_ticks`; part
//! b2 adds the process-context side (`softclock_create_thread`, `softclock_thread[_run]`)
//! and the real `timeout_barrier`; `timeout_sysctl` came with `kern_sysctl.c`; the `ddb`
//! `show callout` printers come with the real ddb (M7).
//!
//! `MULTIPROCESSOR` (M11e): as in the C, a `TIMEOUT_PROC | TIMEOUT_MPSAFE` timeout goes to
//! `timeout_proc_mp` and runs in the `softclockmp` thread (`softclock_thread_mp`), which drops
//! the kernel lock; the other process-context timeouts run in the softclock thread under the
//! kernel lock, and the soft-interrupt ones in `softclock`, a soft interrupt established
//! without `SIF_MPSAFE`, so also under the kernel lock (the C allows `TIMEOUT_MPSAFE` only
//! with `TIMEOUT_PROC`).
//!
//! ## Deviations
//! - `WITNESS` and `kcov` are not configured: `timeout_sync_*` are no-ops and `to_process`
//!   is only recorded.
//! - `timeout_level_width` is a `const` (the C fills it once at startup).

use core::ffi::c_void;
use core::ptr::{self, NonNull};
use core::sync::atomic::{AtomicPtr, Ordering};

use libkern::StaticCell;

use crate::conf::param::{HZ, TICK, TICK_NSEC};
use crate::kassert;
use crate::kern::kern_clock::ticks;
use crate::kern::kern_kthread::{kthread_create, kthread_create_deferred};
use crate::kern::kern_lock::{mtx_enter, mtx_leave};
use crate::kern::kern_sched::sched_peg_curproc;
use crate::kern::kern_softintr::{SoftintrHand, softintr_establish, softintr_schedule};
use crate::kern::kern_synch::{
    cond_init, cond_signal_handler, cond_wait, sleep_finish, sleep_setup, wakeup,
};
use crate::kern::kern_sysctl::sysctl_rdstruct;
use crate::kern::kern_tc::nanouptime;
use crate::kern::subr_prf::panic;
use crate::machine::Machine;
use crate::machine::cpu::{Cpu, CpuInfo, cpu_info_foreach, curproc};
use crate::machine::intr::{IPL_HIGH, IPL_SOFTCLOCK, splsoftclock, splx};
use crate::sys::errno::Errno;
use crate::sys::mutex::{Mutex, mutex_assert_locked};
use crate::sys::param::PSWP;
use crate::sys::proc::Cond;
use crate::sys::sysctl::SysctlPlain;
#[cfg(feature = "multiprocessor")]
use crate::sys::systm::kernel_unlock;
use crate::sys::systm::{INFSLP, kernel_assert_locked};
use crate::sys::time::{Timespec, nsec_to_timespec, timespecadd, timespecsub};
use crate::sys::timeout::{
    Circq, KCLOCK_MAX, KCLOCK_NONE, KCLOCK_UPTIME, TIMEOUT_INITIALIZED, TIMEOUT_MPSAFE,
    TIMEOUT_ONQUEUE, TIMEOUT_PROC, TIMEOUT_TRIGGERED, Timeout, TimeoutFn, Timeoutstat,
};

/// `WHEELCOUNT`.
const WHEELCOUNT: usize = 4;
/// `WHEELSIZE`.
const WHEELSIZE: usize = 256;
/// `WHEELMASK`.
const WHEELMASK: u32 = 255;
/// `WHEELBITS`.
const WHEELBITS: u32 = 8;
/// `BUCKETS`.
const BUCKETS: usize = WHEELCOUNT * WHEELSIZE;

/// `struct timeout_ctx`: a context that runs timeouts (the soft interrupt or a thread).
struct TimeoutCtx {
    /// \[I\] `tctx_todo`.
    tctx_todo: &'static Circq,
    /// \[T\] `tctx_running`.
    tctx_running: core::cell::Cell<*const Timeout>,
}

// SAFETY: `tctx_running` is written under `timeout_mutex`.
unsafe impl Sync for TimeoutCtx {}

/// `struct kclock`: a kernel clock's cached scan times.
struct Kclock {
    /// \[T\] `kc_lastscan`: clock time at last wheel scan.
    kc_lastscan: core::cell::Cell<Timespec>,
    /// \[T\] `kc_late`: late if due prior.
    kc_late: core::cell::Cell<Timespec>,
    /// \[T\] `kc_offset`: offset from primary kclock.
    kc_offset: core::cell::Cell<Timespec>,
}

// SAFETY: written under `timeout_mutex`.
unsafe impl Sync for Kclock {}

impl Kclock {
    const fn new() -> Self {
        Self {
            kc_lastscan: core::cell::Cell::new(Timespec::new(0, 0)),
            kc_late: core::cell::Cell::new(Timespec::new(0, 0)),
            kc_offset: core::cell::Cell::new(Timespec::new(0, 0)),
        }
    }
}

/*
 * Locks used to protect global variables in this file:
 *
 *	I	immutable after initialization
 *	T	timeout_mutex
 */

/// `timeout_mutex`.
static TIMEOUT_MUTEX: Mutex = Mutex::new(IPL_HIGH);

/// \[I\] `softclock_si`: `softclock()` interrupt handle.
static SOFTCLOCK_SI: AtomicPtr<SoftintrHand> = AtomicPtr::new(ptr::null_mut());
/// \[T\] `tostat`: statistics and totals.
pub static TOSTAT: Timeoutstat = Timeoutstat::new();

/// \[T\] `timeout_wheel`: tick-based timeouts.
static TIMEOUT_WHEEL: [Circq; BUCKETS] = [const { Circq::new() }; BUCKETS];
/// \[T\] `timeout_wheel_kc`: clock-based timeouts.
static TIMEOUT_WHEEL_KC: [Circq; BUCKETS] = [const { Circq::new() }; BUCKETS];
/// \[T\] `timeout_new`: new, unscheduled timeouts.
static TIMEOUT_NEW: Circq = Circq::new();
/// \[T\] `timeout_todo`: due or needs rescheduling.
static TIMEOUT_TODO: Circq = Circq::new();
/// `timeout_ctx_si`.
static TIMEOUT_CTX_SI: TimeoutCtx = TimeoutCtx {
    tctx_todo: &TIMEOUT_TODO,
    tctx_running: core::cell::Cell::new(ptr::null()),
};
/// \[T\] `timeout_proc`: due + needs process context.
static TIMEOUT_PROC_Q: Circq = Circq::new();
/// `timeout_ctx_proc`.
static TIMEOUT_CTX_PROC: TimeoutCtx = TimeoutCtx {
    tctx_todo: &TIMEOUT_PROC_Q,
    tctx_running: core::cell::Cell::new(ptr::null()),
};
/// \[T\] `timeout_proc_mp`: process context and no kernel lock (`MULTIPROCESSOR`).
#[cfg(feature = "multiprocessor")]
static TIMEOUT_PROC_MP: Circq = Circq::new();
/// `timeout_ctx_proc_mp` (`MULTIPROCESSOR`).
#[cfg(feature = "multiprocessor")]
static TIMEOUT_CTX_PROC_MP: TimeoutCtx = TimeoutCtx {
    tctx_todo: &TIMEOUT_PROC_MP,
    tctx_running: core::cell::Cell::new(ptr::null()),
};

/// \[I\] `timeout_level_width`: wheel level width (seconds).
const TIMEOUT_LEVEL_WIDTH: [i64; WHEELCOUNT] = {
    let mut w = [0i64; WHEELCOUNT];
    let mut level = 0;
    while level < WHEELCOUNT {
        w[level] = 2 << (level as u32 * WHEELBITS);
        level += 1;
    }
    w
};
/// \[I\] `tick_ts`: length of a tick (1/hz secs).
static TICK_TS: StaticCell<Timespec> = StaticCell::new(Timespec::new(0, 0));

/// `timeout_kclock[KCLOCK_MAX]`.
static TIMEOUT_KCLOCK: [Kclock; KCLOCK_MAX as usize] =
    [const { Kclock::new() }; KCLOCK_MAX as usize];

/// `tick_ts`.
fn tick_ts() -> Timespec {
    // SAFETY: written once by `timeout_startup` on the boot CPU before any timeout is added.
    unsafe { TICK_TS.read() }
}

/// `MASKWHEEL(wheel, time)`.
const fn maskwheel(wheel: u32, time: i32) -> u32 {
    ((time as u32) >> (wheel * WHEELBITS)) & WHEELMASK
}

/// `BUCKET(rel, abs)`: the tick wheel's bucket for a timeout `rel` ticks away at `abs`.
fn bucket(rel: i32, abs: i32) -> &'static Circq {
    let idx = if rel <= (1 << (2 * WHEELBITS)) {
        if rel <= (1 << WHEELBITS) {
            maskwheel(0, abs) as usize
        } else {
            maskwheel(1, abs) as usize + WHEELSIZE
        }
    } else if rel <= (1 << (3 * WHEELBITS)) {
        maskwheel(2, abs) as usize + 2 * WHEELSIZE
    } else {
        maskwheel(3, abs) as usize + 3 * WHEELSIZE
    };
    &TIMEOUT_WHEEL[idx]
}

/// `MOVEBUCKET(wheel, time)`: moves a wheel's bucket for `time` onto `timeout_todo`.
fn movebucket(wheel: u32, time: i32) {
    // SAFETY: under `timeout_mutex`; both are initialised lists.
    unsafe {
        circq_concat(
            &TIMEOUT_TODO,
            &TIMEOUT_WHEEL[maskwheel(wheel, time) as usize + wheel as usize * WHEELSIZE],
        )
    };
}

/*
 * Circular queue definitions.
 */

/// `CIRCQ_INIT(elem)`: makes `elem` an empty list.
fn circq_init(elem: &Circq) {
    elem.next.set(elem);
    elem.prev.set(elem);
}

/// `CIRCQ_INSERT_HEAD(list, elem)`.
///
/// # Safety
///
/// `list` is an initialised list and `elem` a node in no list; both stay in place while
/// linked; the caller holds `timeout_mutex`.
unsafe fn circq_insert_head(list: &Circq, elem: &Circq) {
    let first = list.next.get();
    elem.next.set(first);
    // SAFETY: a linked node is valid until unlinked.
    unsafe { (*first).prev.set(elem) };
    list.next.set(elem);
    elem.prev.set(list);
    TOSTAT.tos_pending.set(TOSTAT.tos_pending.get() + 1);
}

/// `CIRCQ_INSERT_TAIL(list, elem)`.
///
/// # Safety
///
/// As for [`circq_insert_head`].
unsafe fn circq_insert_tail(list: &Circq, elem: &Circq) {
    let last = list.prev.get();
    elem.prev.set(last);
    elem.next.set(list);
    // SAFETY: a linked node is valid until unlinked.
    unsafe { (*last).next.set(elem) };
    list.prev.set(elem);
    TOSTAT.tos_pending.set(TOSTAT.tos_pending.get() + 1);
}

/// `CIRCQ_CONCAT(fst, snd)`: appends `snd`'s elements to `fst` and empties `snd`.
///
/// # Safety
///
/// Both are initialised lists; the caller holds `timeout_mutex`.
unsafe fn circq_concat(fst: &Circq, snd: &Circq) {
    if !circq_empty(snd) {
        // SAFETY: the lists' first and last nodes are linked, hence valid.
        unsafe {
            (*fst.prev.get()).next.set(snd.next.get());
            (*snd.next.get()).prev.set(fst.prev.get());
            (*snd.prev.get()).next.set(fst);
        }
        fst.prev.set(snd.prev.get());
        circq_init(snd);
    }
}

/// `CIRCQ_REMOVE(elem)`.
///
/// # Safety
///
/// `elem` is linked in a list; the caller holds `timeout_mutex`.
unsafe fn circq_remove(elem: &Circq) {
    // SAFETY: the neighbours of a linked node are linked, hence valid.
    unsafe {
        (*elem.next.get()).prev.set(elem.prev.get());
        (*elem.prev.get()).next.set(elem.next.get());
    }
    #[cfg(feature = "diagnostic")]
    {
        // _Q_INVALIDATE
        elem.prev.set(usize::MAX as *const Circq);
        elem.next.set(usize::MAX as *const Circq);
    }
    TOSTAT.tos_pending.set(TOSTAT.tos_pending.get() - 1);
}

/// `CIRCQ_FIRST(elem)`.
fn circq_first(elem: &Circq) -> *const Circq {
    elem.next.get()
}

/// `CIRCQ_EMPTY(elem)`.
fn circq_empty(elem: &Circq) -> bool {
    ptr::eq(circq_first(elem), elem)
}

// WITNESS: timeout_sleeplock_obj, timeout_spinlock_obj, timeout_sync_order/enter/leave.

/// `timeout_from_circq`: the first thing in a struct timeout is its struct circq, so we can
/// get back from a pointer to the latter to a pointer to the whole timeout with just a cast.
///
/// # Safety
///
/// `p` is the `to_list` of a `Timeout` that is linked in a list, hence alive.
#[inline]
unsafe fn timeout_from_circq<'a>(p: *const Circq) -> &'a Timeout {
    // SAFETY: `to_list` is at offset 0 of `#[repr(C)] Timeout` (asserted in `sys/timeout.rs`),
    // and the caller vouches for the pointer.
    unsafe { &*p.cast::<Timeout>() }
}

/*
 * Some of the "math" in here is a bit tricky.
 *
 * We have to beware of wrapping ints.
 * We use the fact that any element added to the queue must be added with a
 * positive time. That means that any element `to' on the queue cannot be
 * scheduled to timeout further in time than INT_MAX, but to->to_time can
 * be positive or negative so comparing it with anything is dangerous.
 * The only way we can use the to->to_time value in any predictable way
 * is when we calculate how far in the future `to' will timeout -
 * "to->to_time - ticks". The result will always be positive for future
 * timeouts and 0 or negative for due timeouts.
 */

/// `timeout_startup`: initialises the queues and the wheels.
pub fn timeout_startup() {
    circq_init(&TIMEOUT_NEW);
    circq_init(&TIMEOUT_TODO);
    circq_init(&TIMEOUT_PROC_Q);
    #[cfg(feature = "multiprocessor")]
    circq_init(&TIMEOUT_PROC_MP);
    for b in TIMEOUT_WHEEL.iter() {
        circq_init(b);
    }
    for b in TIMEOUT_WHEEL_KC.iter() {
        circq_init(b);
    }

    // timeout_level_width: a const here.
    // SAFETY: once, on the boot CPU, before any timeout is added.
    unsafe { TICK_TS.write(nsec_to_timespec(TICK_NSEC.load(Ordering::Relaxed) as u64)) };
}

/// `timeout_proc_init`: establishes the soft interrupt and creates the softclock thread.
pub fn timeout_proc_init() {
    let Some(si) = softintr_establish(IPL_SOFTCLOCK, softclock, ptr::null_mut()) else {
        panic(format_args!(
            "timeout_proc_init: unable to register softclock interrupt"
        ));
    };
    SOFTCLOCK_SI.store(si.as_ptr(), Ordering::Relaxed);

    // WITNESS_INIT: not configured.

    kthread_create_deferred(softclock_create_thread, ptr::null_mut());
}

/// `timeout_set`.
pub fn timeout_set(new: &Timeout, func: TimeoutFn, arg: *mut c_void) {
    timeout_set_flags(new, func, arg, KCLOCK_NONE, 0);
}

/// `timeout_set_flags`.
pub fn timeout_set_flags(to: &Timeout, func: TimeoutFn, arg: *mut c_void, kclock: i32, flags: i32) {
    kassert!(flags & !(TIMEOUT_PROC | TIMEOUT_MPSAFE) == 0);
    kassert!((KCLOCK_NONE..KCLOCK_MAX).contains(&kclock));

    to.to_func.set(Some(func));
    to.to_arg.set(arg);
    to.to_kclock.set(kclock);
    to.to_flags.set(flags | TIMEOUT_INITIALIZED);

    // For now, only process context timeouts may be marked MP-safe.
    if to.to_flags.get() & TIMEOUT_MPSAFE != 0 {
        kassert!(to.to_flags.get() & TIMEOUT_PROC != 0);
    }
}

/// `timeout_set_proc`.
pub fn timeout_set_proc(new: &Timeout, func: TimeoutFn, arg: *mut c_void) {
    timeout_set_flags(new, func, arg, KCLOCK_NONE, TIMEOUT_PROC);
}

/// `timeout_add`: schedules `new` in `to_ticks` ticks; returns `true` if it was not already
/// scheduled.
pub fn timeout_add(new: &Timeout, to_ticks: i32) -> bool {
    let mut ret = true;

    kassert!(new.to_flags.get() & TIMEOUT_INITIALIZED != 0);
    kassert!(new.to_kclock.get() == KCLOCK_NONE);
    kassert!(to_ticks >= 0);

    mtx_enter(&TIMEOUT_MUTEX);

    // Initialize the time here, it won't change.
    let old_time = new.to_time.get();
    let now = ticks();
    new.to_time.set(to_ticks.wrapping_add(now));
    new.to_flags.set(new.to_flags.get() & !TIMEOUT_TRIGGERED);

    // If this timeout already is scheduled and now is moved earlier, reschedule it now.
    // Otherwise leave it in place and let it be rescheduled later.
    if new.to_flags.get() & TIMEOUT_ONQUEUE != 0 {
        if new.to_time.get().wrapping_sub(now) < old_time.wrapping_sub(now) {
            // SAFETY: ONQUEUE: linked; under `timeout_mutex`.
            unsafe {
                circq_remove(&new.to_list);
                circq_insert_tail(&TIMEOUT_NEW, &new.to_list);
            }
        }
        TOSTAT.tos_readded.set(TOSTAT.tos_readded.get() + 1);
        ret = false;
    } else {
        new.to_flags.set(new.to_flags.get() | TIMEOUT_ONQUEUE);
        // SAFETY: not ONQUEUE: in no list; under `timeout_mutex`.
        unsafe { circq_insert_tail(&TIMEOUT_NEW, &new.to_list) };
    }
    // NKCOV > 0: to_process = curproc->p_p.
    TOSTAT.tos_added.set(TOSTAT.tos_added.get() + 1);
    mtx_leave(&TIMEOUT_MUTEX);

    ret
}

/// `timeout_add_ticks`: adds one tick so the requested time has elapsed (see the C's
/// comment).
#[inline]
fn timeout_add_ticks(to: &Timeout, to_ticks: u64) -> bool {
    // XXX to_ticks is added to the current ticks value, but timeouts are run in the next
    // clock interrupt after ticks is incremented. however, the deadline comparison in
    // softclock_process_tick_timeout will fire a timeout if it's deadline is at the current
    // ticks value. eg, a to_ticks value of 1 plus the current ticks value will fire in the
    // next interrupt, which will be too early. add 1 here to ensure the requested time has
    // elapsed.
    let to_ticks = if to_ticks >= i32::MAX as u64 {
        i32::MAX as u64
    } else {
        to_ticks + 1
    };

    timeout_add(to, to_ticks as i32)
}

/// `timeout_add_sec`.
pub fn timeout_add_sec(to: &Timeout, secs: i32) -> bool {
    kassert!(secs >= 0);
    // secs is a 31bit int, so this can't overflow 64bits
    let to_ticks = HZ.load(Ordering::Relaxed) as u64 * secs as u64;

    timeout_add_ticks(to, to_ticks)
}

/*
 * interpret the specified times below as a AT LEAST how long the
 * system should wait before firing the timeouts. this requires
 * rounding up, which has the potential to overflow. if we detect
 * overflow, interpret it as "wait for as long as possible". this will
 * be shorter than specified time, which violates the "wait at least
 * this much time", but it's on the other end of the timescale.
 */

/// `timeout_add_msec`.
pub fn timeout_add_msec(to: &Timeout, msecs: u64) -> bool {
    if msecs >= u64::MAX / 1000 {
        return timeout_add(to, i32::MAX);
    }

    timeout_add_usec(to, msecs * 1000)
}

/// `timeout_add_usec`.
pub fn timeout_add_usec(to: &Timeout, usecs: u64) -> bool {
    let tick = TICK.load(Ordering::Relaxed) as u64;
    if usecs >= u64::MAX - tick {
        return timeout_add(to, i32::MAX);
    }

    let to_ticks = usecs.div_ceil(tick);

    timeout_add_ticks(to, to_ticks)
}

/// `timeout_add_nsec`.
pub fn timeout_add_nsec(to: &Timeout, nsecs: u64) -> bool {
    let tick_nsec = TICK_NSEC.load(Ordering::Relaxed) as u64;
    if nsecs >= u64::MAX - tick_nsec {
        return timeout_add(to, i32::MAX);
    }

    let to_ticks = nsecs.div_ceil(tick_nsec);

    timeout_add_ticks(to, to_ticks)
}

/// `timeout_abs_ts`: schedules `to` at the absolute uptime `abstime`.
pub fn timeout_abs_ts(to: &Timeout, abstime: &Timespec) -> bool {
    let mut ret = true;

    mtx_enter(&TIMEOUT_MUTEX);

    kassert!(to.to_flags.get() & TIMEOUT_INITIALIZED != 0);
    kassert!(to.to_kclock.get() == KCLOCK_UPTIME);

    let old_abstime = to.to_abstime.get();
    to.to_abstime.set(*abstime);
    to.to_flags.set(to.to_flags.get() & !TIMEOUT_TRIGGERED);

    if to.to_flags.get() & TIMEOUT_ONQUEUE != 0 {
        if *abstime < old_abstime {
            // SAFETY: ONQUEUE: linked; under `timeout_mutex`.
            unsafe {
                circq_remove(&to.to_list);
                circq_insert_tail(&TIMEOUT_NEW, &to.to_list);
            }
        }
        TOSTAT.tos_readded.set(TOSTAT.tos_readded.get() + 1);
        ret = false;
    } else {
        to.to_flags.set(to.to_flags.get() | TIMEOUT_ONQUEUE);
        // SAFETY: not ONQUEUE: in no list; under `timeout_mutex`.
        unsafe { circq_insert_tail(&TIMEOUT_NEW, &to.to_list) };
    }
    // NKCOV > 0: to_process = curproc->p_p.
    TOSTAT.tos_added.set(TOSTAT.tos_added.get() + 1);

    mtx_leave(&TIMEOUT_MUTEX);

    ret
}

/// `timeout_del`: cancels `to`; returns `true` if it was scheduled.
pub fn timeout_del(to: &Timeout) -> bool {
    let mut ret = false;

    mtx_enter(&TIMEOUT_MUTEX);
    if to.to_flags.get() & TIMEOUT_ONQUEUE != 0 {
        // SAFETY: ONQUEUE: linked; under `timeout_mutex`.
        unsafe { circq_remove(&to.to_list) };
        to.to_flags.set(to.to_flags.get() & !TIMEOUT_ONQUEUE);
        TOSTAT.tos_cancelled.set(TOSTAT.tos_cancelled.get() + 1);
        ret = true;
    }
    to.to_flags.set(to.to_flags.get() & !TIMEOUT_TRIGGERED);
    TOSTAT.tos_deleted.set(TOSTAT.tos_deleted.get() + 1);
    mtx_leave(&TIMEOUT_MUTEX);

    ret
}

/// `timeout_del_barrier`.
pub fn timeout_del_barrier(to: &Timeout) -> bool {
    // timeout_sync_order: WITNESS.

    let removed = timeout_del(to);
    timeout_barrier(to);

    removed
}

/// `timeout_barrier`: waits for a running `to` to finish (see the module's deviations).
pub fn timeout_barrier(to: &Timeout) {
    let c = Cond::new();
    let flags = to.to_flags.get() & (TIMEOUT_PROC | TIMEOUT_MPSAFE);
    // timeout_sync_order: WITNESS.

    let barrier = Timeout::new_flags(
        cond_signal_handler,
        ptr::from_ref(&c).cast_mut().cast(),
        KCLOCK_NONE,
        flags,
    );
    barrier
        .to_process
        .set(curproc().map_or(ptr::null(), |p| p.p_p.get().cast()));
    cond_init(&c);

    mtx_enter(&TIMEOUT_MUTEX);
    let tctx = if flags & TIMEOUT_PROC != 0 {
        proc_ctx(flags)
    } else {
        &TIMEOUT_CTX_SI
    };

    if !ptr::eq(tctx.tctx_running.get(), to) {
        mtx_leave(&TIMEOUT_MUTEX);
        return;
    }

    barrier.to_time.set(ticks());
    barrier
        .to_flags
        .set(barrier.to_flags.get() | TIMEOUT_ONQUEUE);
    // SAFETY: `barrier` is in no list; the context unlinks it (`timeout_run`) before
    // signalling `c`, so it is off the list before `cond_wait` returns and it goes out of
    // scope; under `timeout_mutex`.
    unsafe { circq_insert_head(tctx.tctx_todo, &barrier.to_list) };
    mtx_leave(&TIMEOUT_MUTEX);

    // We know the relevant timeout context was running something and now also has the
    // barrier to run, so we just have to wait for it to pick up the barrier task now.
    cond_wait(&c, "tmobar");
}

/// `timeout_bucket`: the clock wheel's bucket for `to`.
pub fn timeout_bucket(to: &Timeout) -> u32 {
    kassert!(to.to_kclock.get() == KCLOCK_UPTIME);
    let kc = &TIMEOUT_KCLOCK[to.to_kclock.get() as usize];

    kassert!(kc.kc_lastscan.get() < to.to_abstime.get());
    let diff = timespecsub(&to.to_abstime.get(), &kc.kc_lastscan.get());
    let mut level: u32 = 0;
    while (level as usize) < TIMEOUT_LEVEL_WIDTH.len() - 1 {
        if diff.tv_sec < TIMEOUT_LEVEL_WIDTH[level as usize] {
            break;
        }
        level += 1;
    }
    let shifted_abstime = timespecadd(&to.to_abstime.get(), &kc.kc_offset.get());
    level * WHEELSIZE as u32 + timeout_maskwheel(level, &shifted_abstime)
}

/// `timeout_maskwheel`: hash the absolute time into a bucket on a given level of the wheel.
///
/// The complete hash is 32 bits. The upper 25 bits are seconds, the lower 7 bits are
/// nanoseconds. `tv_nsec` is a positive value less than one billion so we need to divide it
/// to isolate the desired bits. We can't just shift it.
///
/// The level is used to isolate an 8-bit portion of the hash. The resulting number indicates
/// which bucket the absolute time belongs in on the given level of the wheel.
pub fn timeout_maskwheel(level: u32, abstime: &Timespec) -> u32 {
    let hi = (abstime.tv_sec as u32) << 7;
    let lo = (abstime.tv_nsec / 7_812_500) as u32;

    ((hi | lo) >> (level * WHEELBITS)) & WHEELMASK
}

/// `timeout_hardclock_update`: this is called from `hardclock()` on the primary CPU at the
/// start of every tick.
pub fn timeout_hardclock_update() {
    let lastscan = &TIMEOUT_KCLOCK[KCLOCK_UPTIME as usize].kc_lastscan;
    let mut need_softclock = true;

    mtx_enter(&TIMEOUT_MUTEX);

    let now_ticks = ticks();
    movebucket(0, now_ticks);
    if maskwheel(0, now_ticks) == 0 {
        movebucket(1, now_ticks);
        if maskwheel(1, now_ticks) == 0 {
            movebucket(2, now_ticks);
            if maskwheel(2, now_ticks) == 0 {
                movebucket(3, now_ticks);
            }
        }
    }

    // Dump the buckets that expired while we were away.
    //
    // If the elapsed time has exceeded a level's limit then we need to dump every bucket in
    // the level. We have necessarily completed a lap of that level, too, so we need to
    // process buckets in the next level.
    //
    // Otherwise we need to compare indices: if the index of the first expired bucket is
    // greater than that of the last then we have completed a lap of the level and need to
    // process buckets in the next level.
    let now = nanouptime();
    let elapsed = timespecsub(&now, &lastscan.get());
    for level in 0..TIMEOUT_LEVEL_WIDTH.len() as u32 {
        let first = timeout_maskwheel(level, &lastscan.get()) as usize;
        let (last, done) = if elapsed.tv_sec >= TIMEOUT_LEVEL_WIDTH[level as usize] {
            (if first == 0 { WHEELSIZE - 1 } else { first - 1 }, false)
        } else {
            let last = timeout_maskwheel(level, &now) as usize;
            (last, first <= last)
        };
        let off = level as usize * WHEELSIZE;
        let mut b = first;
        loop {
            // SAFETY: under `timeout_mutex`; both are initialised lists.
            unsafe { circq_concat(&TIMEOUT_TODO, &TIMEOUT_WHEEL_KC[off + b]) };
            if b == last {
                break;
            }
            b = (b + 1) % WHEELSIZE;
        }
        if done {
            break;
        }
    }

    // Update the cached state for each kclock.
    for kc in TIMEOUT_KCLOCK.iter() {
        kc.kc_lastscan.set(timespecadd(&now, &kc.kc_offset.get()));
        kc.kc_late
            .set(timespecsub(&kc.kc_lastscan.get(), &tick_ts()));
    }

    if circq_empty(&TIMEOUT_NEW) && circq_empty(&TIMEOUT_TODO) {
        need_softclock = false;
    }

    mtx_leave(&TIMEOUT_MUTEX);

    if need_softclock && let Some(si) = NonNull::new(SOFTCLOCK_SI.load(Ordering::Relaxed)) {
        softintr_schedule(si);
    }
}

/// The process context a `TIMEOUT_PROC` timeout with `flags` runs in: `timeout_ctx_proc_mp`
/// for `TIMEOUT_MPSAFE` with `MULTIPROCESSOR`, `timeout_ctx_proc` otherwise (the C's
/// `#ifdef MULTIPROCESSOR` choice in `timeout_barrier` and `softclock_process_*_timeout`).
fn proc_ctx(flags: i32) -> &'static TimeoutCtx {
    #[cfg(feature = "multiprocessor")]
    if flags & TIMEOUT_MPSAFE != 0 {
        return &TIMEOUT_CTX_PROC_MP;
    }
    let _ = flags;
    &TIMEOUT_CTX_PROC
}

/// `timeout_run`: runs `to` in `tctx`, dropping the mutex around the call.
fn timeout_run(tctx: &TimeoutCtx, to: &Timeout) {
    mutex_assert_locked(&TIMEOUT_MUTEX, "timeout_run");

    to.to_flags.set(to.to_flags.get() & !TIMEOUT_ONQUEUE);
    to.to_flags.set(to.to_flags.get() | TIMEOUT_TRIGGERED);

    let func = to.to_func.get();
    let arg = to.to_arg.get();
    // needsproc = ISSET(to->to_flags, TIMEOUT_PROC): WITNESS only.
    // NKCOV > 0: kcov_process.

    tctx.tctx_running.set(to);
    mtx_leave(&TIMEOUT_MUTEX);
    // timeout_sync_enter(needsproc): WITNESS.
    if let Some(func) = func {
        func(arg);
    }
    // timeout_sync_leave(needsproc): WITNESS.
    mtx_enter(&TIMEOUT_MUTEX);
    tctx.tctx_running.set(ptr::null());
}

/// `softclock_process_kclock_timeout`.
fn softclock_process_kclock_timeout(to: &Timeout, new: bool) {
    let kc = &TIMEOUT_KCLOCK[to.to_kclock.get() as usize];

    if to.to_abstime.get() > kc.kc_lastscan.get() {
        TOSTAT.tos_scheduled.set(TOSTAT.tos_scheduled.get() + 1);
        if !new {
            TOSTAT.tos_rescheduled.set(TOSTAT.tos_rescheduled.get() + 1);
        }
        // SAFETY: `to` was just removed from `timeout_todo`; under `timeout_mutex`.
        unsafe { circq_insert_tail(&TIMEOUT_WHEEL_KC[timeout_bucket(to) as usize], &to.to_list) };
        return;
    }
    if !new && to.to_abstime.get() <= kc.kc_late.get() {
        TOSTAT.tos_late.set(TOSTAT.tos_late.get() + 1);
    }
    if to.to_flags.get() & TIMEOUT_PROC != 0 {
        // SAFETY: as above.
        unsafe { circq_insert_tail(proc_ctx(to.to_flags.get()).tctx_todo, &to.to_list) };
        return;
    }
    timeout_run(&TIMEOUT_CTX_SI, to);
    TOSTAT
        .tos_run_softclock
        .set(TOSTAT.tos_run_softclock.get() + 1);
}

/// `softclock_process_tick_timeout`.
fn softclock_process_tick_timeout(to: &Timeout, new: bool) {
    let delta = to.to_time.get().wrapping_sub(ticks());

    if delta > 0 {
        TOSTAT.tos_scheduled.set(TOSTAT.tos_scheduled.get() + 1);
        if !new {
            TOSTAT.tos_rescheduled.set(TOSTAT.tos_rescheduled.get() + 1);
        }
        // SAFETY: `to` was just removed from `timeout_todo`; under `timeout_mutex`.
        unsafe { circq_insert_tail(bucket(delta, to.to_time.get()), &to.to_list) };
        return;
    }
    if !new && delta < 0 {
        TOSTAT.tos_late.set(TOSTAT.tos_late.get() + 1);
    }
    if to.to_flags.get() & TIMEOUT_PROC != 0 {
        // SAFETY: as above.
        unsafe { circq_insert_tail(proc_ctx(to.to_flags.get()).tctx_todo, &to.to_list) };
        return;
    }
    timeout_run(&TIMEOUT_CTX_SI, to);
    TOSTAT
        .tos_run_softclock
        .set(TOSTAT.tos_run_softclock.get() + 1);
}

/// `softclock`: timeouts are processed here instead of `timeout_hardclock_update()` to
/// avoid doing any more work at `IPL_CLOCK` than absolutely necessary. Down here at
/// `IPL_SOFTCLOCK` other interrupts can be serviced promptly so the system remains responsive
/// even if there is a surge of timeouts.
pub fn softclock(_arg: *mut c_void) {
    let mut first_new: *const Timeout = ptr::null();
    let mut new = false;

    mtx_enter(&TIMEOUT_MUTEX);
    if !circq_empty(&TIMEOUT_NEW) {
        // SAFETY: the first node of a non-empty list is a linked timeout's `to_list`.
        first_new = unsafe { timeout_from_circq(circq_first(&TIMEOUT_NEW)) };
    }
    // SAFETY: under `timeout_mutex`; both are initialised lists.
    unsafe { circq_concat(&TIMEOUT_TODO, &TIMEOUT_NEW) };
    while !circq_empty(&TIMEOUT_TODO) {
        // SAFETY: as above; the node is unlinked right away, under the mutex.
        let to = unsafe { timeout_from_circq(circq_first(&TIMEOUT_TODO)) };
        // SAFETY: linked, under `timeout_mutex`.
        unsafe { circq_remove(&to.to_list) };
        if ptr::eq(to, first_new) {
            new = true;
        }
        match to.to_kclock.get() {
            KCLOCK_NONE => softclock_process_tick_timeout(to, new),
            KCLOCK_UPTIME => softclock_process_kclock_timeout(to, new),
            other => panic(format_args!("softclock: invalid to_clock: {other}")),
        }
    }
    TOSTAT.tos_softclocks.set(TOSTAT.tos_softclocks.get() + 1);
    let needsproc = !circq_empty(&TIMEOUT_PROC_Q);
    #[cfg(feature = "multiprocessor")]
    let need_proc_mp = !circq_empty(&TIMEOUT_PROC_MP);
    mtx_leave(&TIMEOUT_MUTEX);

    if needsproc {
        wakeup(ptr::addr_of!(TIMEOUT_PROC_Q));
    }
    #[cfg(feature = "multiprocessor")]
    if need_proc_mp {
        wakeup(ptr::addr_of!(TIMEOUT_PROC_MP));
    }
}

/// `softclock_create_thread`: creates the softclock thread (deferred from
/// `timeout_proc_init`).
pub fn softclock_create_thread(_arg: *mut c_void) {
    if kthread_create(softclock_thread, ptr::null_mut(), b"softclock").is_err() {
        panic(format_args!("fork softclock"));
    }
    #[cfg(feature = "multiprocessor")]
    if kthread_create(softclock_thread_mp, ptr::null_mut(), b"softclockmp").is_err() {
        panic(format_args!("kthread_create softclock_thread_mp"));
    }
}

/// `softclock_thread_run`: the softclock thread's loop: sleeps on the context's list and
/// runs what `softclock` queued there.
fn softclock_thread_run(tctx: &TimeoutCtx) {
    let todo = tctx.tctx_todo;

    loop {
        // Avoid holding both timeout_mutex and SCHED_LOCK at the same time.
        sleep_setup(ptr::from_ref(todo).cast(), PSWP, "tmoslp");
        let _ = sleep_finish(INFSLP, circq_empty(todo));

        mtx_enter(&TIMEOUT_MUTEX);
        TOSTAT
            .tos_thread_wakeups
            .set(TOSTAT.tos_thread_wakeups.get() + 1);
        while !circq_empty(todo) {
            // SAFETY: the first node of a non-empty list is a linked timeout's `to_list`,
            // under `timeout_mutex`.
            let to = unsafe { timeout_from_circq(circq_first(todo)) };
            // SAFETY: linked, under `timeout_mutex`.
            unsafe { circq_remove(&to.to_list) };
            timeout_run(tctx, to);
            TOSTAT.tos_run_thread.set(TOSTAT.tos_run_thread.get() + 1);
        }
        mtx_leave(&TIMEOUT_MUTEX);
    }
}

/// `softclock_thread`: the thread that runs the `TIMEOUT_PROC` timeouts, pegged to the
/// primary CPU.
pub fn softclock_thread(_arg: *mut c_void) {
    kernel_assert_locked(); // KERNEL_ASSERT_LOCKED()

    // Be conservative for the moment
    let mut primary: Option<&'static CpuInfo> = None;
    cpu_info_foreach(&mut |ci| {
        if primary.is_none() && Machine::cpu_is_primary(ci) {
            primary = Some(ci);
        }
    });
    let Some(ci) = primary else {
        panic(format_args!("softclock_thread: no primary CPU"));
    };
    sched_peg_curproc(ci);

    let s = splsoftclock();
    softclock_thread_run(&TIMEOUT_CTX_PROC);
    splx(s);
}

/// `softclock_thread_mp` (`MULTIPROCESSOR`): the thread that runs the `TIMEOUT_PROC |
/// TIMEOUT_MPSAFE` timeouts without the kernel lock, on any CPU.
#[cfg(feature = "multiprocessor")]
pub fn softclock_thread_mp(_arg: *mut c_void) {
    kernel_assert_locked(); // KERNEL_ASSERT_LOCKED()
    kernel_unlock(); // KERNEL_UNLOCK()

    softclock_thread_run(&TIMEOUT_CTX_PROC_MP);
}

/// `timeout_adjust_ticks`: moves the tick wheel forward by `adj` ticks after the clock was
/// stepped (`tc_setclock`).
pub fn timeout_adjust_ticks(adj: i32) {
    // adjusting the monotonic clock backwards would be a Bad Thing
    if adj <= 0 {
        return;
    }

    mtx_enter(&TIMEOUT_MUTEX);
    let now = ticks();
    let new_ticks = now.wrapping_add(adj);
    for b in TIMEOUT_WHEEL.iter() {
        let mut p = circq_first(b);
        while !ptr::eq(p, b) {
            // SAFETY: a node of the bucket's list is a linked timeout's `to_list`.
            let to = unsafe { timeout_from_circq(p) };
            // SAFETY: `p` is linked.
            p = unsafe { circq_first(&*p) };

            // when moving a timeout forward need to reinsert it
            if to.to_time.get().wrapping_sub(now) < adj {
                to.to_time.set(new_ticks);
            }
            // SAFETY: linked, under `timeout_mutex`.
            unsafe {
                circq_remove(&to.to_list);
                circq_insert_tail(&TIMEOUT_TODO, &to.to_list);
            }
        }
    }
    crate::kern::kern_clock::TICKS.store(new_ticks, Ordering::Relaxed);
    mtx_leave(&TIMEOUT_MUTEX);
}

/// `timeout_sysctl` (`!SMALL_KERNEL`): `kern.timeout_stats`, a copy of `tostat` taken under
/// `timeout_mutex`.
pub fn timeout_sysctl(
    oldp: usize,
    oldlenp: &mut usize,
    newp: usize,
    newlen: usize,
) -> Result<(), Errno> {
    let _ = newlen;

    // struct timeoutstat: twelve uint64_t, in the C's order.
    mtx_enter(&TIMEOUT_MUTEX);
    let status: [u64; 12] = [
        TOSTAT.tos_added.get(),
        TOSTAT.tos_cancelled.get(),
        TOSTAT.tos_deleted.get(),
        TOSTAT.tos_late.get(),
        TOSTAT.tos_pending.get(),
        TOSTAT.tos_readded.get(),
        TOSTAT.tos_rescheduled.get(),
        TOSTAT.tos_run_softclock.get(),
        TOSTAT.tos_run_thread.get(),
        TOSTAT.tos_scheduled.get(),
        TOSTAT.tos_softclocks.get(),
        TOSTAT.tos_thread_wakeups.get(),
    ];
    mtx_leave(&TIMEOUT_MUTEX);

    sysctl_rdstruct(oldp, oldlenp, newp, status.as_bytes())
}

// db_kclock, db_timespec, db_show_callout_bucket, db_show_timeout, db_show_callout: the real
// ddb (M7).

#[cfg(test)]
pub(crate) mod tests;
