//! Host tests for `<sys/time.h>`'s arithmetic.

use super::*;

#[test]
fn timeval_add_and_sub_carry() {
    let a = Timeval::new(1, 900_000);
    let b = Timeval::new(2, 200_000);
    assert_eq!(timeradd(&a, &b), Timeval::new(4, 100_000));
    assert_eq!(timersub(&b, &a), Timeval::new(0, 300_000));
    assert_eq!(timersub(&a, &b), Timeval::new(-1, 700_000));
    assert!(a < b);
    assert!(Timeval::new(1, 5) < Timeval::new(1, 6));
    assert!(a.is_set() && a.is_valid());
    assert!(!Timeval::new(0, 1_000_000).is_valid());
    let mut c = a;
    c.clear();
    assert!(!c.is_set());
}

#[test]
fn timespec_add_and_sub_carry() {
    let a = Timespec::new(1, 900_000_000);
    let b = Timespec::new(2, 200_000_000);
    assert_eq!(timespecadd(&a, &b), Timespec::new(4, 100_000_000));
    assert_eq!(timespecsub(&b, &a), Timespec::new(0, 300_000_000));
    assert_eq!(timespecsub(&a, &b), Timespec::new(-1, 700_000_000));
    assert!(a < b);
    assert_eq!(
        timeval_to_timespec(&Timeval::new(3, 7)),
        Timespec::new(3, 7000)
    );
    assert_eq!(
        timespec_to_timeval(&Timespec::new(3, 7999)),
        Timeval::new(3, 7)
    );
}

#[test]
fn bintime_carries_through_frac() {
    let half = Bintime::new(0, 1 << 63);
    assert_eq!(bintimeadd(&half, &half), Bintime::new(1, 0));
    assert_eq!(bintimeaddfrac(&half, 1 << 63), Bintime::new(1, 0));
    assert_eq!(bintimesub(&Bintime::new(1, 0), &half), half);
    assert_eq!(frac_to_nsec(1 << 63), 500_000_000);
    assert_eq!(bintime_to_timespec(&half), Timespec::new(0, 500_000_000));
    assert_eq!(bintime_to_timeval(&half), Timeval::new(0, 500_000));
    assert!(half < Bintime::new(1, 0));
}

#[test]
fn bintime_round_trips() {
    let ts = Timespec::new(12, 345_678_901);
    let back = bintime_to_timespec(&timespec_to_bintime(&ts));
    assert_eq!(back.tv_sec, 12);
    assert!((back.tv_nsec - ts.tv_nsec).abs() <= 1, "{back:?}");
    let tv = Timeval::new(12, 345_678);
    let back = bintime_to_timeval(&timeval_to_bintime(&tv));
    assert_eq!(back.tv_sec, 12);
    assert!((back.tv_usec - tv.tv_usec).abs() <= 1, "{back:?}");
}

#[test]
fn timecount_to_bintime_at_one_mhz() {
    // The dummy timecounter's scale: 2^64 / 10^6 fractions per count.
    let scale = u64::MAX / 1_000_000;
    let bt = timecount_to_bintime(1_000_000, scale);
    assert!(bt.sec == 0 && bt.frac > u64::MAX - 2_000_000, "{bt:?}");
    let bt = timecount_to_bintime(1_500_000, scale);
    assert_eq!(bt.sec, 1);
    assert_eq!(frac_to_nsec(bt.frac) / 1_000_000, 499);
}

#[test]
fn nsec_conversions_saturate() {
    assert_eq!(
        nsec_to_timespec(1_500_000_001),
        Timespec::new(1, 500_000_001)
    );
    assert_eq!(nsec_to_timeval(1_500_000_999), Timeval::new(1, 500_000));
    assert_eq!(usec_to_timeval(2_000_003), Timeval::new(2, 3));
    assert_eq!(sec_to_nsec(2), 2_000_000_000);
    assert_eq!(sec_to_nsec(u64::MAX), u64::MAX);
    assert_eq!(msec_to_nsec(u64::MAX / 2), u64::MAX);
    assert_eq!(usec_to_nsec(3), 3000);
    assert_eq!(timespec_to_nsec(&Timespec::new(1, 1)), 1_000_000_001);
    assert_eq!(timespec_to_nsec(&Timespec::new(i64::MAX, 0)), u64::MAX);
    assert_eq!(timeval_to_nsec(&Timeval::new(1, 1)), 1_000_001_000);
    assert_eq!(timeval_to_nsec(&Timeval::new(i64::MAX, 0)), u64::MAX);
    assert_eq!(bintime_to_nsec(&Bintime::new(2, 1 << 63)), 2_500_000_000);
}

#[test]
fn bcd() {
    assert_eq!(frombcd(0x59), 59);
    assert_eq!(tobcd(59), 0x59);
}
