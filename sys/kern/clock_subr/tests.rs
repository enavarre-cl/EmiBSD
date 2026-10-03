use super::*;

const fn ymdhms(year: u16, mon: u8, day: u8, hour: u8, min: u8, sec: u8) -> ClockYmdhms {
    ClockYmdhms {
        dt_year: year,
        dt_mon: mon,
        dt_day: day,
        dt_wday: 0,
        dt_hour: hour,
        dt_min: min,
        dt_sec: sec,
    }
}

#[test]
fn leap_years() {
    assert!(leapyear(2000));
    assert!(leapyear(2024));
    assert!(!leapyear(1900));
    assert!(!leapyear(2100));
    assert!(!leapyear(2026));
    assert_eq!(days_in_year(2000), 366);
    assert_eq!(days_in_year(2001), 365);
}

/// (broken-down date, day of the week with Sunday = 0, POSIX time).
const KNOWN: &[(ClockYmdhms, u8, Time)] = &[
    (ymdhms(1970, 1, 1, 0, 0, 0), 4, 0),
    (ymdhms(1999, 12, 31, 23, 59, 59), 5, 946_684_799),
    (ymdhms(2000, 1, 1, 0, 0, 0), 6, 946_684_800),
    (ymdhms(2000, 2, 29, 0, 0, 0), 2, 951_782_400),
    (ymdhms(2000, 3, 1, 0, 0, 0), 3, 951_868_800),
    (ymdhms(2026, 10, 3, 12, 34, 56), 6, 1_791_030_896),
    (ymdhms(2038, 1, 19, 3, 14, 7), 2, 2_147_483_647),
    (ymdhms(2100, 3, 1, 0, 0, 0), 1, 4_107_542_400),
];

#[test]
fn ymdhms_to_secs_known_dates() {
    for (dt, _, secs) in KNOWN {
        assert_eq!(clock_ymdhms_to_secs(dt), *secs, "{dt:?}");
    }
}

#[test]
fn secs_to_ymdhms_known_dates() {
    for (dt, wday, secs) in KNOWN {
        let want = ClockYmdhms {
            dt_wday: *wday,
            ..*dt
        };
        assert_eq!(clock_secs_to_ymdhms(*secs), want, "{secs}");
    }
}

#[test]
fn round_trip_every_day_and_odd_seconds() {
    // Every day from 1970 to 2105, at a time of day that moves with the day.
    for day in 0..49_400i64 {
        let secs = day * SECDAY + (day * 37) % SECDAY;
        let dt = clock_secs_to_ymdhms(secs);
        assert_eq!(clock_ymdhms_to_secs(&dt), secs);
        assert_eq!(i64::from(dt.dt_wday), (day + 4) % 7);
    }
}
