/* <LICENSES> */
/*
 * Copyright (c) 1988 University of Utah.
 * Copyright (c) 1982, 1990, 1993
 *	The Regents of the University of California.  All rights reserved.
 *
 * This code is derived from software contributed to Berkeley by
 * the Systems Programming Group of the University of Utah Computer
 * Science Department.
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
 * from: Utah $Hdr: clock.c 1.18 91/01/21$
 *
 *	@(#)clock.c	8.2 (Berkeley) 1/12/94
 */
/* </LICENSES> */

/* <CODE> */
//! Generic routines to convert between a POSIX date (seconds since 1/1/1970) and
//! yr/mo/day/hr/min/sec: `kern/clock_subr.c`.
//!
//! Upstream: sys/kern/clock_subr.c @ 3ce1f3f79392
//!
//! Status: `ported`. `clock_ymdhms_to_secs` and `clock_secs_to_ymdhms`, with `leapyear` and the
//! month table. `struct clock_ymdhms`, `FROMBCD`/`TOBCD`, `SECDAY` and `POSIX_BASE_YEAR` are
//! `<sys/time.h>`'s (`sys/time.rs`).
//!
//! ## Deviations
//! - `clock_secs_to_ymdhms` returns the broken-down time instead of filling an out parameter.
//! - The day count and the seconds sum are `i64`, where the C uses `int` for the days (and for
//!   `((days * 24 + hour) * 60 + min)`, which overflows past the year 6000).
//! - A month above 12 in `clock_ymdhms_to_secs` adds no days for the months past December; the
//!   C reads past the end of `month_days[]`.
//! - The C copies `month_days[]` to a local so as to make February 29 days in a leap year;
//!   here the leap day is added to the month's length when it is looked up.

use crate::sys::time::{ClockYmdhms, POSIX_BASE_YEAR, SECDAY};
use crate::sys::types::Time;

/// `FEBRUARY`.
const FEBRUARY: i32 = 2;

/// `month_days`: days in each month of a non-leap year.
const MONTH_DAYS: [i32; 12] = [31, 28, 31, 30, 31, 30, 31, 31, 30, 31, 30, 31];

/// `leapyear`: this avoids some unnecessary modulo operations as compared with the usual
/// `(year % 4 == 0 && year % 100 != 0) || year % 400 == 0`; it is otherwise equivalent.
fn leapyear(year: i32) -> bool {
    let mut rv = false;

    if (year & 3) == 0 {
        rv = true;
        if (year % 100) == 0 {
            rv = false;
            if (year % 400) == 0 {
                rv = true;
            }
        }
    }
    rv
}

/// `days_in_year(a)`.
fn days_in_year(year: i32) -> i32 {
    if leapyear(year) { 366 } else { 365 }
}

/// `days_in_month(a)` on the plain table (`month` is 1-based).
fn days_in_month(month: i32) -> i32 {
    MONTH_DAYS.get((month - 1) as usize).copied().unwrap_or(0)
}

/// `clock_ymdhms_to_secs`: the POSIX time (seconds since 1/1/1970) of a broken-down date.
pub fn clock_ymdhms_to_secs(dt: &ClockYmdhms) -> Time {
    let year = i32::from(dt.dt_year);
    let mon = i32::from(dt.dt_mon);

    // Compute days since start of time. First from years, then from months.
    let mut days: i64 = 0;
    for i in POSIX_BASE_YEAR..year {
        days += i64::from(days_in_year(i));
    }
    if leapyear(year) && mon > FEBRUARY {
        days += 1;
    }

    // Months
    for i in 1..mon {
        days += i64::from(days_in_month(i));
    }
    days += i64::from(dt.dt_day) - 1;

    // Add hours, minutes, seconds.
    ((days * 24 + i64::from(dt.dt_hour)) * 60 + i64::from(dt.dt_min)) * 60 + i64::from(dt.dt_sec)
}

/// `clock_secs_to_ymdhms`: the broken-down date of a POSIX time. The day of the week counts
/// from Sunday = 0.
pub fn clock_secs_to_ymdhms(secs: Time) -> ClockYmdhms {
    let mut days = (secs / SECDAY) as i32;
    let mut rsec = (secs % SECDAY) as i32; // remainder seconds

    // Day of week (Note: 1/1/1970 was a Thursday)
    let dt_wday = ((days + 4) % 7) as u8;

    // Subtract out whole years, counting them in year.
    let mut year = POSIX_BASE_YEAR;
    while days >= days_in_year(year) {
        days -= days_in_year(year);
        year += 1;
    }

    // Subtract out whole months, counting them in mon. February has 29 days in a leap year.
    let leap = leapyear(year);
    let mdays = |m: i32| days_in_month(m) + i32::from(leap && m == FEBRUARY);
    let mut mon = 1;
    while days >= mdays(mon) {
        days -= mdays(mon);
        mon += 1;
    }

    // Hours, minutes, seconds are easy
    let dt_hour = (rsec / 3600) as u8;
    rsec %= 3600;
    let dt_min = (rsec / 60) as u8;
    rsec %= 60;

    ClockYmdhms {
        dt_year: year as u16,
        dt_mon: mon as u8,
        // Days are what is left over (+1) from all that.
        dt_day: (days + 1) as u8,
        dt_wday,
        dt_hour,
        dt_min,
        dt_sec: rsec as u8,
    }
}
/* </CODE> */

/* <TESTS> */
#[cfg(test)]
mod tests {
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
}
/* </TESTS> */
