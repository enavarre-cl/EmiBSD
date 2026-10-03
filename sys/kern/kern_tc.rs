/*	$OpenBSD: kern_tc.c,v 1.84 2025/06/12 20:37:58 deraadt Exp $ */
/* <LICENSES> */
/*
 * Copyright (c) 2000 Poul-Henning Kamp <phk@FreeBSD.org>
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

/*
 * If we meet some day, and you think this stuff is worth it, you
 * can buy me a beer in return. Poul-Henning Kamp
 */
/* </LICENSES> */

//! Timecounters: `kern/kern_tc.c`, the kernel's notion of time. A timehands ring holds the
//! offset the active hardware counter is read against; `tc_windup` advances it every tick,
//! and the `[get]{bin,nano,micro}[boot|up]time()` family reads it lock-free with a generation
//! check (see `<sys/time.h>` for a description of these functions).
//!
//! Upstream: sys/kern/kern_tc.c @ 3ce1f3f79392
//!
//! Status: `wip`. Milestone M5 ports the dummy timecounter, the timehands ring, every time
//! reader, `tc_init`, `tc_reset_quality`, `tc_getfrequency`/`tc_getprecision`,
//! `tc_setrealtimeclock`, `tc_setclock`, `tc_update_timekeep`, `tc_windup`, `tc_ticktock`,
//! `inittimecounter`, `ntp_update_second`, `tc_adjfreq` and `tc_adjtime`. The sysctl side
//! (`sysctl_tc`, `sysctl_tc_hardware`, `sysctl_tc_choice`, `tc_vars`) is not here yet:
//! `kern_sysctl.rs` reports `kern.timecounter`.
//!
//! ## Deviations
//! - `tc_lock` is an rwlock (`kern_rwlock.c`, M5-b): the paths that take it
//!   (`tc_setrealtimeclock`, `tc_adjfreq`, `tc_adjtime`) report it as unported and go on
//!   under `windup_mtx` alone, which on one CPU with no other thread is the same exclusion.
//! - `timekeep` (the page shared with userland) is `kern_exec.c`'s global
//!   (`kern_exec::TIMEKEEP`): null until the first exec maps the page, so
//!   `tc_update_timekeep` returns at its null check before then, as the C does.
//! - `getuptime`/`gettime` take the `__LP64__` branch (both architectures are LP64).
//! - `membar_consumer`/`membar_producer` are `fence(Acquire)`/`fence(Release)`.

use core::cell::Cell;
use core::ptr;
use core::sync::atomic::{AtomicBool, AtomicI32, AtomicI64, AtomicPtr, AtomicU32, Ordering, fence};

use crate::conf::param::{HZ, TICK_NSEC};
use crate::dev::rnd::enqueue_randomness;
use crate::kern::kern_lock::{mtx_enter, mtx_enter_try, mtx_leave};
use crate::kern::kern_rwlock::{
    rw_assert_anylock, rw_assert_wrlock, rw_enter_write, rw_exit_write,
};
use crate::kern::kern_timeout::timeout_adjust_ticks;
use crate::kern::subr_prf::{log, panic, printf};
use crate::machine::intr::IPL_CLOCK;
use crate::sys::mutex::{Mutex, mutex_assert_locked};
use crate::sys::queue::SlistHead;
use crate::sys::rwlock::Rwlock;
use crate::sys::syslog::LOG_INFO;
use crate::sys::time::{
    Bintime, Timespec, Timeval, bintime_to_nsec, bintime_to_timespec, bintime_to_timeval,
    bintimeadd, bintimesub, timecount_to_bintime, timespec_to_bintime,
};
use crate::sys::timetc::{TcList, Timecounter, Timekeep};
use crate::sys::types::Time;

/// `dummy_get_timecount`: a counter that advances once per read.
static DUMMY_NOW: AtomicU32 = AtomicU32::new(0);

/// `dummy_get_timecount`.
pub fn dummy_get_timecount(_tc: &Timecounter) -> u32 {
    DUMMY_NOW.fetch_add(1, Ordering::Relaxed).wrapping_add(1)
}

/// `dummy_timecounter`: implement a dummy timecounter which we can use until we get a real
/// one in the air. This allows the console and other early stuff to use time services.
static DUMMY_TIMECOUNTER: Timecounter = Timecounter::new(
    dummy_get_timecount,
    !0u32,
    1_000_000,
    "dummy",
    -1_000_000,
    0,
);

/*
 * Locks used to protect struct members, global variables in this file:
 *	I	immutable after initialization
 *	T	tc_lock
 *	W	windup_mtx
 */

/// `struct timehands`.
struct Timehands {
    // These fields must be initialized by the driver.
    /// \[W\] `th_counter`.
    th_counter: Cell<*const Timecounter>,
    /// \[T,W\] `th_adjtimedelta`.
    th_adjtimedelta: Cell<i64>,
    /// \[T,W\] `th_next_ntp_update`.
    th_next_ntp_update: Cell<Bintime>,
    /// \[W\] `th_adjustment`.
    th_adjustment: Cell<i64>,
    /// \[W\] `th_scale`.
    th_scale: Cell<u64>,
    /// \[W\] `th_offset_count`.
    th_offset_count: Cell<u32>,
    /// \[T,W\] `th_boottime`.
    th_boottime: Cell<Bintime>,
    /// \[W\] `th_offset`.
    th_offset: Cell<Bintime>,
    /// \[W\] `th_naptime`.
    th_naptime: Cell<Bintime>,
    /// \[W\] `th_microtime`.
    th_microtime: Cell<Timeval>,
    /// \[W\] `th_nanotime`.
    th_nanotime: Cell<Timespec>,
    // Fields not to be copied in tc_windup start with th_generation.
    /// \[W\] `th_generation` (volatile).
    th_generation: AtomicU32,
    /// \[I\] `th_next`.
    th_next: &'static Timehands,
}

// SAFETY: written under `windup_mtx` with the generation protocol; read lock-free against
// the generation, as the C does.
unsafe impl Sync for Timehands {}

impl Timehands {
    /// A timehands with the fields the C leaves zero.
    const fn new(
        counter: *const Timecounter,
        scale: u64,
        generation: u32,
        next: &'static Timehands,
    ) -> Self {
        Self {
            th_counter: Cell::new(counter),
            th_adjtimedelta: Cell::new(0),
            th_next_ntp_update: Cell::new(Bintime::new(0, 0)),
            th_adjustment: Cell::new(0),
            th_scale: Cell::new(scale),
            th_offset_count: Cell::new(0),
            th_boottime: Cell::new(Bintime::new(0, 0)),
            th_offset: Cell::new(Bintime::new(0, 0)),
            th_naptime: Cell::new(Bintime::new(0, 0)),
            th_microtime: Cell::new(Timeval::new(0, 0)),
            th_nanotime: Cell::new(Timespec::new(0, 0)),
            th_generation: AtomicU32::new(generation),
            th_next: next,
        }
    }

    /// `memcpy(th, tho, offsetof(struct timehands, th_generation))`: every field before the
    /// generation.
    fn copy_from(&self, tho: &Timehands) {
        self.th_counter.set(tho.th_counter.get());
        self.th_adjtimedelta.set(tho.th_adjtimedelta.get());
        self.th_next_ntp_update.set(tho.th_next_ntp_update.get());
        self.th_adjustment.set(tho.th_adjustment.get());
        self.th_scale.set(tho.th_scale.get());
        self.th_offset_count.set(tho.th_offset_count.get());
        self.th_boottime.set(tho.th_boottime.get());
        self.th_offset.set(tho.th_offset.get());
        self.th_naptime.set(tho.th_naptime.get());
        self.th_microtime.set(tho.th_microtime.get());
        self.th_nanotime.set(tho.th_nanotime.get());
    }

    /// `th->th_counter`.
    fn counter(&self) -> &'static Timecounter {
        // SAFETY: the counter is a `&'static Timecounter` registered by `tc_init`, or the
        // dummy; both live forever.
        unsafe { &*self.th_counter.get() }
    }
}

/// `th1`.
static TH1: Timehands = Timehands::new(ptr::null(), 0, 0, &TH0);
/// `th0`.
static TH0: Timehands = Timehands::new(&DUMMY_TIMECOUNTER, u64::MAX / 1_000_000, 1, &TH1);

/// `tc_lock`: the lock over the timecounter choice and the clock settings.
pub static TC_LOCK: Rwlock = Rwlock::new("tc_lock");

/// `windup_mtx`: `tc_windup()` must be called before leaving this mutex.
static WINDUP_MTX: Mutex = Mutex::new(IPL_CLOCK);

/// \[W\] `timehands`: the active timehands.
static TIMEHANDS: AtomicPtr<Timehands> = AtomicPtr::new(ptr::addr_of!(TH0).cast_mut());
/// \[T\] `timecounter`: the active timecounter.
static TIMECOUNTER: AtomicPtr<Timecounter> =
    AtomicPtr::new(ptr::addr_of!(DUMMY_TIMECOUNTER).cast_mut());
/// `tc_list`'s head, made `Sync`: the list is written by `tc_init` on the boot CPU.
struct TcListHead(SlistHead<TcList>);

// SAFETY: see `TC_LIST`.
unsafe impl Sync for TcListHead {}

/// `tc_list`: the registered timecounters.
static TC_LIST: TcListHead = TcListHead(SlistHead::new());

/*
 * These are updated from tc_windup().  They are useful when
 * examining kernel core dumps.
 */

/// `naptime` (volatile).
pub static NAPTIME: AtomicI64 = AtomicI64::new(0);
/// `time_second` (volatile).
pub static TIME_SECOND: AtomicI64 = AtomicI64::new(0);
/// `time_uptime` (volatile).
pub static TIME_UPTIME: AtomicI64 = AtomicI64::new(0);

/// `timestepwarnings`.
static TIMESTEPWARNINGS: AtomicI32 = AtomicI32::new(0);

// `timekeep`: kern_exec.c's global (`kern_exec::TIMEKEEP`).

/// The active timehands.
fn timehands() -> &'static Timehands {
    // SAFETY: `TIMEHANDS` only ever points at `TH0` or `TH1`, which live forever.
    unsafe { &*TIMEHANDS.load(Ordering::Relaxed) }
}

/// `timecounter`: the active timecounter.
pub fn timecounter() -> &'static Timecounter {
    // SAFETY: `TIMECOUNTER` only ever points at a `&'static Timecounter`.
    unsafe { &*TIMECOUNTER.load(Ordering::Relaxed) }
}

/// `tc_delta`: return the difference between the timehands' counter value now and what was
/// when we copied it to the timehands' offset_count.
#[inline]
fn tc_delta(th: &Timehands) -> u32 {
    let tc = th.counter();
    tc.get_timecount().wrapping_sub(th.th_offset_count.get()) & tc.tc_counter_mask.get()
}

/// Reads a consistent value off the timehands: we have to loop until we are sure that the
/// timehands that we operated on was not updated under our feet.
#[inline]
fn read_timehands<T>(f: impl Fn(&Timehands) -> T) -> T {
    loop {
        let th = timehands();
        let generation = th.th_generation.load(Ordering::Relaxed);
        fence(Ordering::Acquire); // membar_consumer()
        let v = f(th);
        fence(Ordering::Acquire); // membar_consumer()
        if generation != 0 && generation == th.th_generation.load(Ordering::Relaxed) {
            return v;
        }
    }
}

/// `binboottime`.
pub fn binboottime() -> Bintime {
    read_timehands(|th| th.th_boottime.get())
}

/// `microboottime`.
pub fn microboottime() -> Timeval {
    bintime_to_timeval(&binboottime())
}

/// `nanoboottime`.
pub fn nanoboottime() -> Timespec {
    bintime_to_timespec(&binboottime())
}

/// `binuptime`.
pub fn binuptime() -> Bintime {
    read_timehands(|th| {
        let bt = timecount_to_bintime(tc_delta(th), th.th_scale.get());
        bintimeadd(&bt, &th.th_offset.get())
    })
}

/// `getbinuptime`.
pub fn getbinuptime() -> Bintime {
    read_timehands(|th| th.th_offset.get())
}

/// `nanouptime`.
pub fn nanouptime() -> Timespec {
    bintime_to_timespec(&binuptime())
}

/// `microuptime`.
pub fn microuptime() -> Timeval {
    bintime_to_timeval(&binuptime())
}

/// `getuptime`.
pub fn getuptime() -> Time {
    TIME_UPTIME.load(Ordering::Relaxed) // atomic
}

/// `nsecuptime`.
pub fn nsecuptime() -> u64 {
    bintime_to_nsec(&binuptime())
}

/// `getnsecuptime`.
pub fn getnsecuptime() -> u64 {
    bintime_to_nsec(&getbinuptime())
}

/// `binruntime`.
pub fn binruntime() -> Bintime {
    read_timehands(|th| {
        let bt = timecount_to_bintime(tc_delta(th), th.th_scale.get());
        let bt = bintimeadd(&bt, &th.th_offset.get());
        bintimesub(&bt, &th.th_naptime.get())
    })
}

/// `nanoruntime`.
pub fn nanoruntime() -> Timespec {
    bintime_to_timespec(&binruntime())
}

/// `getbinruntime`.
pub fn getbinruntime() -> Bintime {
    read_timehands(|th| bintimesub(&th.th_offset.get(), &th.th_naptime.get()))
}

/// `getnsecruntime`.
pub fn getnsecruntime() -> u64 {
    bintime_to_nsec(&getbinruntime())
}

/// `bintime`.
pub fn bintime() -> Bintime {
    read_timehands(|th| {
        let bt = timecount_to_bintime(tc_delta(th), th.th_scale.get());
        let bt = bintimeadd(&bt, &th.th_offset.get());
        bintimeadd(&bt, &th.th_boottime.get())
    })
}

/// `nanotime`.
pub fn nanotime() -> Timespec {
    bintime_to_timespec(&bintime())
}

/// `microtime`.
pub fn microtime() -> Timeval {
    bintime_to_timeval(&bintime())
}

/// `gettime`.
pub fn gettime() -> Time {
    TIME_SECOND.load(Ordering::Relaxed) // atomic
}

/// `getnanouptime`.
pub fn getnanouptime() -> Timespec {
    read_timehands(|th| bintime_to_timespec(&th.th_offset.get()))
}

/// `getmicrouptime`.
pub fn getmicrouptime() -> Timeval {
    read_timehands(|th| bintime_to_timeval(&th.th_offset.get()))
}

/// `getnanotime`.
pub fn getnanotime() -> Timespec {
    read_timehands(|th| th.th_nanotime.get())
}

/// `getmicrotime`.
pub fn getmicrotime() -> Timeval {
    read_timehands(|th| th.th_microtime.get())
}

/// `tc_init`: initialize a new timecounter and possibly use it.
pub fn tc_init(tc: &'static Timecounter) {
    let mut u = (tc.tc_frequency.get() / u64::from(tc.tc_counter_mask.get())) as u32;
    // XXX: We need some margin here, 10% is a guess
    u = u.wrapping_mul(11);
    u /= 10;
    if tc.tc_quality.get() >= 0 && u > HZ.load(Ordering::Relaxed) as u32 {
        tc.tc_quality.set(-2000);
        printf(format_args!(
            "Timecounter \"{}\" frequency {} Hz",
            tc.tc_name,
            tc.tc_frequency.get()
        ));
        printf(format_args!(" -- Insufficient hz, needs at least {u}\n"));
    }

    // Determine the counter's precision.
    let mut tmp: u64 = 1;
    while tmp & u64::from(tc.tc_counter_mask.get()) == 0 {
        tmp <<= 1;
    }
    tc.tc_precision.set(tmp);

    // SAFETY: a static timecounter, registered once, never unlinked.
    unsafe { TC_LIST.0.insert_head(tc) };

    // Never automatically use a timecounter with negative quality. Even though we run on the
    // dummy counter, switching here may be worse since this timecounter may not be monotonic.
    if tc.tc_quality.get() < 0 {
        return;
    }
    let active = timecounter();
    if tc.tc_quality.get() < active.tc_quality.get() {
        return;
    }
    if tc.tc_quality.get() == active.tc_quality.get()
        && tc.tc_frequency.get() < active.tc_frequency.get()
    {
        return;
    }
    let _ = tc.get_timecount();
    enqueue_randomness(tc.get_timecount());

    TIMECOUNTER.store(ptr::from_ref(tc).cast_mut(), Ordering::Relaxed);
}

/// `tc_reset_quality`: change the given timecounter's quality. If it is the active counter
/// and it is no longer the best counter, activate the best counter.
pub fn tc_reset_quality(tc: &'static Timecounter, quality: i32) {
    if ptr::eq(tc, &DUMMY_TIMECOUNTER) {
        panic(format_args!(
            "tc_reset_quality: cannot change dummy counter quality"
        ));
    }

    tc.tc_quality.set(quality);
    if ptr::eq(timecounter(), tc) {
        let mut best: &'static Timecounter = &DUMMY_TIMECOUNTER;
        for tmp in TC_LIST.0.iter() {
            if tmp.tc_quality.get() < 0 {
                continue;
            }
            if tmp.tc_quality.get() < best.tc_quality.get() {
                continue;
            }
            if tmp.tc_quality.get() == best.tc_quality.get()
                && tmp.tc_frequency.get() < best.tc_frequency.get()
            {
                continue;
            }
            best = tmp;
        }
        if !ptr::eq(best, tc) {
            enqueue_randomness(best.get_timecount());
            TIMECOUNTER.store(ptr::from_ref(best).cast_mut(), Ordering::Relaxed);
            printf(format_args!(
                "timecounter: active counter changed: {} -> {}\n",
                tc.tc_name, best.tc_name
            ));
        }
    }
}

/// `tc_getfrequency`: report the frequency of the current timecounter.
pub fn tc_getfrequency() -> u64 {
    timehands().counter().tc_frequency.get()
}

/// `tc_getprecision`: report the precision of the current timecounter.
pub fn tc_getprecision() -> u64 {
    timehands().counter().tc_precision.get()
}

/// `tc_setrealtimeclock`: step our concept of UTC, aka the realtime clock. This is done by
/// modifying our estimate of when we booted.
///
/// Any ongoing adjustment is meaningless after a clock jump, so we zero adjtimedelta here as
/// well.
pub fn tc_setrealtimeclock(ts: &Timespec) {
    let utc = timespec_to_bintime(ts);

    rw_enter_write(&TC_LOCK);
    mtx_enter(&WINDUP_MTX);

    let uptime = binuptime();
    let boottime = bintimesub(&utc, &uptime);
    let old_utc = bintimeadd(&timehands().th_boottime.get(), &uptime);
    // XXX fiddle all the little crinkly bits around the fiords...
    tc_windup(Some(&boottime), None, Some(0));

    mtx_leave(&WINDUP_MTX);
    rw_exit_write(&TC_LOCK);

    enqueue_randomness(ts.tv_sec as u32);

    if TIMESTEPWARNINGS.load(Ordering::Relaxed) != 0 {
        let tmp = bintime_to_timespec(&old_utc);
        log(
            LOG_INFO,
            format_args!(
                "Time stepped from {}.{:09} to {}.{:09}\n",
                tmp.tv_sec, tmp.tv_nsec, ts.tv_sec, ts.tv_nsec
            ),
        );
    }
}

/// `tc_setclock`: step the monotonic and realtime clocks, triggering any timeouts that
/// should have occurred across the interval.
pub fn tc_setclock(ts: &Timespec) {
    static FIRST: AtomicBool = AtomicBool::new(true);

    // When we're called for the first time, during boot when the root partition is mounted,
    // we need to set boottime.
    if FIRST.swap(false, Ordering::Relaxed) {
        tc_setrealtimeclock(ts);
        return;
    }

    enqueue_randomness(ts.tv_sec as u32);

    let utc = timespec_to_bintime(ts);

    mtx_enter(&WINDUP_MTX);

    let uptime = bintimesub(&utc, &timehands().th_boottime.get());
    let old_naptime = timehands().th_naptime.get();
    // XXX fiddle all the little crinkly bits around the fiords...
    tc_windup(None, Some(&uptime), None);
    let new_naptime = timehands().th_naptime.get();

    mtx_leave(&WINDUP_MTX);

    // SMALL_KERNEL: not configured. Convert the bintime to ticks.
    let elapsed = bintimesub(&new_naptime, &old_naptime);
    let adj_ticks = bintime_to_nsec(&elapsed) / TICK_NSEC.load(Ordering::Relaxed) as u64;
    if adj_ticks > 0 {
        timeout_adjust_ticks(adj_ticks.min(i32::MAX as u64) as i32);
    }
}

/// `tc_update_timekeep`: publishes the timehands to the page userland reads.
pub fn tc_update_timekeep() {
    static LAST_TC: AtomicPtr<Timecounter> = AtomicPtr::new(ptr::null_mut());

    mutex_assert_locked(&WINDUP_MTX, "tc_update_timekeep");

    let tk: *mut Timekeep = crate::kern::kern_exec::TIMEKEEP.load(Ordering::Acquire);
    if tk.is_null() {
        return;
    }
    // SAFETY: a non-null `timekeep` is the wired page `exec_timekeep_map` mapped; it is written
    // only here, under `windup_mtx`, with the generation protocol userland follows.
    let timekeep = unsafe { &mut *tk };

    let th = timehands();
    timekeep.tk_generation = 0;
    fence(Ordering::Release); // membar_producer()
    timekeep.tk_scale = th.th_scale.get();
    timekeep.tk_offset_count = th.th_offset_count.get();
    timekeep.tk_offset = th.th_offset.get();
    timekeep.tk_naptime = th.th_naptime.get();
    timekeep.tk_boottime = th.th_boottime.get();
    let counter = th.th_counter.get();
    if LAST_TC.load(Ordering::Relaxed).cast_const() != counter {
        timekeep.tk_counter_mask = th.counter().tc_counter_mask.get();
        timekeep.tk_user = th.counter().tc_user.get();
        LAST_TC.store(counter.cast_mut(), Ordering::Relaxed);
    }
    fence(Ordering::Release); // membar_producer()
    timekeep.tk_generation = th.th_generation.load(Ordering::Relaxed);
}

/// `tc_windup`: initialize the next struct timehands in the ring and make it the active
/// timehands. Along the way we might switch to a different timecounter and/or do seconds
/// processing in NTP. Slightly magic.
pub fn tc_windup(
    new_boottime: Option<&Bintime>,
    new_offset: Option<&Bintime>,
    new_adjtimedelta: Option<i64>,
) {
    // rw_assert_wrlock(&tc_lock) when new_boottime or new_adjtimedelta: see the deviations.
    mutex_assert_locked(&WINDUP_MTX, "tc_windup");

    let active_tc = timecounter();

    // Make the next timehands a copy of the current one, but do not overwrite the generation
    // or next pointer. While we update the contents, the generation must be zero.
    let tho = timehands();
    let mut ogen = tho.th_generation.load(Ordering::Relaxed);
    let th = tho.th_next;
    th.th_generation.store(0, Ordering::Relaxed);
    fence(Ordering::Release); // membar_producer()
    th.copy_from(tho);

    // Capture a timecounter delta on the current timecounter and if changing timecounters,
    // a counter value from the new timecounter. Update the offset fields accordingly.
    let delta = tc_delta(th);
    let ncount = if !ptr::eq(th.counter(), active_tc) {
        active_tc.get_timecount()
    } else {
        0
    };
    th.th_offset_count
        .set(th.th_offset_count.get().wrapping_add(delta) & th.counter().tc_counter_mask.get());
    let bt = timecount_to_bintime(delta, th.th_scale.get());
    th.th_offset.set(bintimeadd(&th.th_offset.get(), &bt));

    // Ignore new offsets that predate the current offset. If changing the offset, first
    // increase the naptime accordingly.
    if let Some(new_offset) = new_offset
        && th.th_offset.get() < *new_offset
    {
        let bt = bintimesub(new_offset, &th.th_offset.get());
        th.th_naptime.set(bintimeadd(&th.th_naptime.get(), &bt));
        NAPTIME.store(th.th_naptime.get().sec, Ordering::Relaxed);
        th.th_offset.set(*new_offset);
    }

    // If changing the boot time or clock adjustment, do so before NTP processing.
    if let Some(new_boottime) = new_boottime {
        th.th_boottime.set(*new_boottime);
    }
    if let Some(new_adjtimedelta) = new_adjtimedelta {
        th.th_adjtimedelta.set(new_adjtimedelta);
        // Reset the NTP update period.
        th.th_next_ntp_update
            .set(bintimesub(&th.th_offset.get(), &th.th_naptime.get()));
    }

    // Deal with NTP second processing. The while-loop normally iterates at most once, but in
    // extreme situations it might keep NTP sane if tc_windup() is not run for several
    // seconds.
    let bt = bintimesub(&th.th_offset.get(), &th.th_naptime.get());
    while th.th_next_ntp_update.get() <= bt {
        ntp_update_second(th);
        let mut next = th.th_next_ntp_update.get();
        next.sec += 1;
        th.th_next_ntp_update.set(next);
    }

    // Update the UTC timestamps used by the get*() functions.
    let bt = bintimeadd(&th.th_boottime.get(), &th.th_offset.get());
    th.th_microtime.set(bintime_to_timeval(&bt));
    th.th_nanotime.set(bintime_to_timespec(&bt));

    // Now is a good time to change timecounters.
    if !ptr::eq(th.counter(), active_tc) {
        th.th_counter.set(active_tc);
        th.th_offset_count.set(ncount);
    }

    /*-
     * Recalculate the scaling factor.  We want the number of 1/2^64
     * fractions of a second per period of the hardware counter, taking
     * into account the th_adjustment factor which the NTP PLL/adjtime(2)
     * processing provides us with.
     *
     * The th_adjustment is nanoseconds per second with 32 bit binary
     * fraction and we want 64 bit binary fraction of second:
     *
     *	 x = a * 2^32 / 10^9 = a * 4.294967296
     *
     * The range of th_adjustment is +/- 5000PPM so inside a 64bit int
     * we can only multiply by about 850 without overflowing, but that
     * leaves suitably precise fractions for multiply before divide.
     *
     * Divide before multiply with a fraction of 2199/512 results in a
     * systematic undercompensation of 10PPM of th_adjustment.  On a
     * 5000PPM adjustment this is a 0.05PPM error.  This is acceptable.
     *
     * We happily sacrifice the lowest of the 64 bits of our result
     * to the goddess of code clarity.
     *
     */
    let mut scale: u64 = 1 << 63;
    scale = scale.wrapping_add(
        (((th.th_adjustment.get() + th.counter().tc_freq_adj.get()) / 1024) * 2199) as u64,
    );
    scale /= th.counter().tc_frequency.get();
    th.th_scale.set(scale.wrapping_mul(2));

    // Now that the struct timehands is again consistent, set the new generation number,
    // making sure to not make it zero.
    ogen = ogen.wrapping_add(1);
    if ogen == 0 {
        ogen = 1;
    }
    fence(Ordering::Release); // membar_producer()
    th.th_generation.store(ogen, Ordering::Relaxed);

    // Go live with the new struct timehands.
    TIME_SECOND.store(th.th_microtime.get().tv_sec, Ordering::Relaxed);
    TIME_UPTIME.store(th.th_offset.get().sec, Ordering::Relaxed);
    fence(Ordering::Release); // membar_producer()
    TIMEHANDS.store(ptr::from_ref(th).cast_mut(), Ordering::Relaxed);

    tc_update_timekeep();
}

/// `tc_tick`: timecounters need to be updated every so often to prevent the hardware counter
/// from overflowing. Updating also recalculates the cached values used by the get*() family
/// of functions, so their precision depends on the update frequency.
static TC_TICK: AtomicI32 = AtomicI32::new(0);

/// `tc_ticktock`: the hardclock's share: winds the timehands up every `tc_tick` ticks.
pub fn tc_ticktock() {
    static COUNT: AtomicI32 = AtomicI32::new(0);

    if COUNT.fetch_add(1, Ordering::Relaxed) + 1 < TC_TICK.load(Ordering::Relaxed) {
        return;
    }
    if !mtx_enter_try(&WINDUP_MTX) {
        return;
    }
    COUNT.store(0, Ordering::Relaxed);
    tc_windup(None, None, None);
    mtx_leave(&WINDUP_MTX);
}

/// `inittimecounter`.
pub fn inittimecounter() {
    // Set the initial timeout to max(1, <approx. number of hardclock ticks in a millisecond>).
    // People should probably not use the sysctl to set the timeout to smaller than its
    // initial value, since that value is the smallest reasonable one. If they want better
    // timestamps they should use the non-"get"* functions.
    let hz = HZ.load(Ordering::Relaxed);
    TC_TICK.store(
        if hz > 1000 { (hz + 500) / 1000 } else { 1 },
        Ordering::Relaxed,
    );
    #[cfg(feature = "debug")]
    {
        let p = (TC_TICK.load(Ordering::Relaxed) * 1_000_000) / hz;
        printf(format_args!(
            "Timecounters tick every {}.{:03} msec\n",
            p / 1000,
            p % 1000
        ));
    }

    // warm up new timecounter (again) and get rolling.
    let _ = timecounter().get_timecount();
    let _ = timecounter().get_timecount();
}

/// `ntp_update_second`: skew the timehands according to any adjtime(2) adjustment.
fn ntp_update_second(th: &Timehands) {
    mutex_assert_locked(&WINDUP_MTX, "ntp_update_second");

    let adj = if th.th_adjtimedelta.get() > 0 {
        th.th_adjtimedelta.get().min(5000)
    } else {
        th.th_adjtimedelta.get().max(-5000)
    };
    th.th_adjtimedelta.set(th.th_adjtimedelta.get() - adj);
    th.th_adjustment.set((adj * 1000) << 32);
}

/// `tc_adjfreq`: reads and/or sets the active counter's frequency adjustment.
pub fn tc_adjfreq(old: Option<&mut i64>, new: Option<i64>) {
    if let Some(old) = old {
        rw_assert_anylock(&TC_LOCK);
        *old = timecounter().tc_freq_adj.get();
    }
    if let Some(new) = new {
        rw_assert_wrlock(&TC_LOCK);
        mtx_enter(&WINDUP_MTX);
        timecounter().tc_freq_adj.set(new);
        tc_windup(None, None, None);
        mtx_leave(&WINDUP_MTX);
    }
}

/// `tc_adjtime`: reads and/or sets the pending adjtime(2) delta.
pub fn tc_adjtime(old: Option<&mut i64>, new: Option<i64>) {
    if let Some(old) = old {
        *old = read_timehands(|th| th.th_adjtimedelta.get());
    }
    if let Some(new) = new {
        rw_assert_wrlock(&TC_LOCK);
        mtx_enter(&WINDUP_MTX);
        tc_windup(None, None, Some(new));
        mtx_leave(&WINDUP_MTX);
    }
}

#[cfg(test)]
pub(crate) mod tests;
