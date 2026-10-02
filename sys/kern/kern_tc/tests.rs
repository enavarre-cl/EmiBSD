//! Host tests for the timecounters, over the dummy counter (one count per read at 1 MHz).
//!
//! The timehands are global, so the tests that wind them up serialise on `LOCK`.

use std::sync::Mutex as StdMutex;

use super::*;

/// Serialises the tests that call `tc_windup`.
static LOCK: StdMutex<()> = StdMutex::new(());

#[test]
fn readers_are_monotonic_on_the_dummy_counter() {
    let _g = LOCK.lock().unwrap_or_else(|e| e.into_inner());
    let a = nsecuptime();
    let b = nsecuptime();
    assert!(b > a, "the dummy counter advances on every read: {a} {b}");
    let bt = binuptime();
    let ts = nanouptime();
    assert_eq!(ts.tv_sec, bt.sec);
    let tv = microuptime();
    assert!(tv.tv_sec >= ts.tv_sec);
}

#[test]
fn windup_advances_the_offset_and_the_cached_times() {
    let _g = LOCK.lock().unwrap_or_else(|e| e.into_inner());
    inittimecounter();
    let before = getnsecuptime();
    // Burn counts: at 1 MHz, 2_000_000 reads are two seconds of dummy time.
    for _ in 0..2_000_000 {
        let _ = dummy_get_timecount(&DUMMY_TIMECOUNTER);
    }
    mtx_enter(&WINDUP_MTX);
    tc_windup(None, None, None);
    mtx_leave(&WINDUP_MTX);
    let after = getnsecuptime();
    assert!(after - before >= 1_900_000_000, "{before} -> {after}");
    assert!(getuptime() >= 1);
    assert_eq!(getnanouptime().tv_sec, getuptime());
    assert_eq!(getmicrouptime().tv_sec, getuptime());
    assert!(getnanotime().tv_sec >= getuptime());
    assert_eq!(gettime(), getmicrotime().tv_sec);
}

#[test]
fn ticktock_winds_every_tc_tick() {
    let _g = LOCK.lock().unwrap_or_else(|e| e.into_inner());
    inittimecounter();
    let before = getbinuptime();
    for _ in 0..10_000 {
        let _ = dummy_get_timecount(&DUMMY_TIMECOUNTER);
    }
    tc_ticktock();
    let after = getbinuptime();
    assert!(after > before, "{before:?} -> {after:?}");
    assert!(tc_getfrequency() >= 1_000_000);
    // The dummy counter never goes through tc_init, so its precision stays 0.
    assert!(tc_getprecision() <= 1);
}

#[test]
fn setrealtimeclock_sets_the_boot_time() {
    let _g = LOCK.lock().unwrap_or_else(|e| e.into_inner());
    // tc_setclock steps the timer wheel (timeout_adjust_ticks): take its tests' lock and
    // bring the wheel up.
    let _w = crate::kern::kern_timeout::tests::LOCK
        .lock()
        .unwrap_or_else(|e| e.into_inner());
    crate::kern::kern_timeout::timeout_startup();
    inittimecounter();
    let utc = Timespec::new(1_700_000_000, 0);
    tc_setrealtimeclock(&utc);
    let boot = binboottime();
    let up = getbinuptime();
    // boottime = utc - uptime, so the seconds carry through the fractions.
    assert_eq!(bintimeadd(&boot, &up).sec, 1_700_000_000);
    assert_eq!(bintime().sec, 1_700_000_000);
    assert!(nanotime().tv_sec >= 1_700_000_000);
    assert!(microtime().tv_sec >= 1_700_000_000);
    assert_eq!(nanoboottime().tv_sec, boot.sec);
    assert_eq!(microboottime().tv_sec, boot.sec);
    // tc_setclock's first call sets the boot time too; its second steps uptime forward.
    tc_setclock(&utc);
    let run_before = getnsecruntime();
    tc_setclock(&Timespec::new(1_700_000_100, 0));
    assert!(getuptime() >= up.sec + 99, "{}", getuptime());
    assert!(NAPTIME.load(Ordering::Relaxed) >= 99);
    assert!(getnsecruntime() < getnsecuptime());
    assert!(nanoruntime().tv_sec <= nanouptime().tv_sec);
    assert!(getnsecruntime() >= run_before);
    assert!(binruntime() < binuptime());
}

#[test]
fn a_better_timecounter_takes_over_and_quality_resets() {
    let _g = LOCK.lock().unwrap_or_else(|e| e.into_inner());
    static COUNT: AtomicU32 = AtomicU32::new(0);
    fn get(_tc: &Timecounter) -> u32 {
        COUNT.fetch_add(100, Ordering::Relaxed)
    }
    static GOOD: Timecounter = Timecounter::new(get, 0xffff_ffff, 100_000_000, "testtc", 1000, 0);
    static BAD: Timecounter = Timecounter::new(get, 0xffff_ffff, 100_000_000, "badtc", -1, 0);
    tc_init(&BAD);
    assert_ne!(
        timecounter().tc_name,
        "badtc",
        "negative quality is never chosen"
    );
    tc_init(&GOOD);
    assert_eq!(timecounter().tc_name, "testtc");
    assert_eq!(GOOD.tc_precision.get(), 1);
    mtx_enter(&WINDUP_MTX);
    tc_windup(None, None, None);
    mtx_leave(&WINDUP_MTX);
    assert_eq!(tc_getfrequency(), 100_000_000);
    tc_reset_quality(&GOOD, -5);
    assert_ne!(
        timecounter().tc_name,
        "testtc",
        "demoted: the best remaining counter wins"
    );
    mtx_enter(&WINDUP_MTX);
    tc_windup(None, None, None);
    mtx_leave(&WINDUP_MTX);
}

#[test]
fn adjtime_and_adjfreq_round_trip() {
    let _g = LOCK.lock().unwrap_or_else(|e| e.into_inner());
    let mut old = 0i64;
    tc_adjtime(Some(&mut old), Some(4_000_000));
    tc_adjtime(Some(&mut old), None);
    // The windup that installs the delta also runs one NTP second, which skews 5000 ns.
    assert_eq!(old, 4_000_000 - 5000);
    tc_adjtime(None, Some(0));
    let mut f = 0i64;
    tc_adjfreq(Some(&mut f), Some(12));
    tc_adjfreq(Some(&mut f), None);
    assert_eq!(f, 12);
    tc_adjfreq(None, Some(0));
}
