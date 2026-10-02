//! Host tests for the timer wheel: the clock is driven by hand (`TICKS` and
//! `timeout_hardclock_update`), the soft interrupt by calling `softclock` directly.

use core::sync::atomic::{AtomicU32, AtomicUsize};
use std::sync::Mutex as StdMutex;

use super::*;
use crate::kern::kern_clock::TICKS;
use crate::sys::timeout::{timeout_initialized, timeout_pending, timeout_triggered};

/// The wheel is global: the tests serialise on it (`kern_tc`'s `tc_setclock` test too).
pub(crate) static LOCK: StdMutex<()> = StdMutex::new(());

static FIRED: AtomicU32 = AtomicU32::new(0);
static LAST_ARG: AtomicUsize = AtomicUsize::new(0);

fn fire(arg: *mut c_void) {
    FIRED.fetch_add(1, Ordering::Relaxed);
    LAST_ARG.store(arg as usize, Ordering::Relaxed);
}

/// One hardclock's worth of wheel work: `ticks++`, then the update, then the softclock.
fn tick_once() {
    TICKS.fetch_add(1, Ordering::Relaxed);
    timeout_hardclock_update();
    softclock(ptr::null_mut());
}

fn setup() -> std::sync::MutexGuard<'static, ()> {
    let g = LOCK.lock().unwrap_or_else(|e| e.into_inner());
    timeout_startup();
    FIRED.store(0, Ordering::Relaxed);
    g
}

#[test]
fn a_tick_timeout_fires_after_its_ticks() {
    let _g = setup();
    static TO: Timeout = Timeout::zeroed();
    timeout_set(&TO, fire, 7 as *mut c_void);
    assert!(timeout_initialized(&TO));
    assert!(timeout_add(&TO, 3));
    assert!(timeout_pending(&TO));
    assert!(!timeout_add(&TO, 3), "already scheduled");
    // timeout_add(to, n) fires in the n-th hardclock (timeout_add_ticks adds the extra one).
    for _ in 0..2 {
        tick_once();
        assert_eq!(FIRED.load(Ordering::Relaxed), 0);
    }
    tick_once();
    assert_eq!(FIRED.load(Ordering::Relaxed), 1);
    assert_eq!(LAST_ARG.load(Ordering::Relaxed), 7);
    assert!(timeout_triggered(&TO));
    assert!(!timeout_pending(&TO));
    assert!(!timeout_del(&TO), "already ran");
    assert!(!timeout_triggered(&TO));
}

#[test]
fn del_cancels_and_a_shorter_readd_moves_earlier() {
    let _g = setup();
    static TO: Timeout = Timeout::new(fire, ptr::null_mut());
    assert!(timeout_add(&TO, 100));
    assert!(timeout_del(&TO));
    tick_once();
    assert_eq!(FIRED.load(Ordering::Relaxed), 0);
    assert!(timeout_add(&TO, 100));
    tick_once(); // bucketed now
    assert!(!timeout_add(&TO, 1), "moved earlier while on the wheel");
    tick_once();
    tick_once();
    assert_eq!(FIRED.load(Ordering::Relaxed), 1);
    assert_eq!(TOSTAT.tos_pending.get(), 0);
}

#[test]
fn long_timeouts_cascade_through_the_wheel_levels() {
    let _g = setup();
    static TO: Timeout = Timeout::new(fire, ptr::null_mut());
    // Past the first level (256 buckets) and into the second.
    let n = 300;
    assert!(timeout_add(&TO, n));
    for _ in 0..n - 1 {
        tick_once();
    }
    assert_eq!(FIRED.load(Ordering::Relaxed), 0);
    tick_once();
    assert_eq!(FIRED.load(Ordering::Relaxed), 1);
}

#[test]
fn add_helpers_round_up() {
    let _g = setup();
    static TO: Timeout = Timeout::new(fire, ptr::null_mut());
    let now = ticks();
    let hz = HZ.load(Ordering::Relaxed);
    assert!(timeout_add_sec(&TO, 2));
    assert_eq!(TO.to_time.get().wrapping_sub(now), 2 * hz + 1);
    timeout_del(&TO);
    assert!(timeout_add_msec(&TO, 15));
    assert_eq!(
        TO.to_time.get().wrapping_sub(now),
        2 + 1,
        "15 ms is two 10 ms ticks, plus one"
    );
    timeout_del(&TO);
    assert!(timeout_add_usec(&TO, 1));
    assert_eq!(TO.to_time.get().wrapping_sub(now), 1 + 1);
    timeout_del(&TO);
    assert!(timeout_add_nsec(&TO, u64::MAX));
    assert_eq!(TO.to_time.get().wrapping_sub(now), i32::MAX);
    timeout_del(&TO);
    assert!(timeout_add_msec(&TO, u64::MAX));
    assert_eq!(TO.to_time.get().wrapping_sub(now), i32::MAX);
    timeout_del(&TO);
}

#[test]
fn absolute_uptime_timeouts_fire_when_the_clock_passes_them() {
    let _g = setup();
    static TO: Timeout = Timeout::zeroed();
    timeout_set_flags(&TO, fire, ptr::null_mut(), KCLOCK_UPTIME, 0);
    // The dummy timecounter advances a microsecond per read: a deadline a few hundred reads
    // ahead is reached after a handful of ticks.
    let now = nanouptime();
    let deadline = timespecadd(&now, &Timespec::new(0, 2_000));
    assert!(timeout_abs_ts(&TO, &deadline));
    let mut n = 0;
    while FIRED.load(Ordering::Relaxed) == 0 && n < 100 {
        tick_once();
        n += 1;
    }
    assert_eq!(FIRED.load(Ordering::Relaxed), 1, "fired after {n} ticks");
    assert_eq!(timeout_maskwheel(0, &Timespec::new(1, 0)), 128);
    assert_eq!(timeout_maskwheel(1, &Timespec::new(2, 0)), 1);
}

#[test]
fn adjust_ticks_runs_what_the_step_skipped() {
    let _g = setup();
    static TO: Timeout = Timeout::new(fire, ptr::null_mut());
    assert!(timeout_add(&TO, 50));
    tick_once(); // bucketed
    timeout_adjust_ticks(60);
    softclock(ptr::null_mut());
    assert_eq!(FIRED.load(Ordering::Relaxed), 1);
}

#[test]
fn proc_timeouts_wait_for_the_thread() {
    let _g = setup();
    static TO: Timeout = Timeout::zeroed();
    timeout_set_proc(&TO, fire, ptr::null_mut());
    assert!(timeout_add(&TO, 1));
    tick_once();
    tick_once();
    assert_eq!(
        FIRED.load(Ordering::Relaxed),
        0,
        "queued for the softclock thread"
    );
    assert!(timeout_pending(&TO));
    assert!(timeout_del_barrier(&TO));
}
