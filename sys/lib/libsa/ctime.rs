/*	$OpenBSD: ctime.c,v 1.6 2018/05/23 16:23:48 cheloha Exp $	*/
/* <LICENSES> */
/*
 * Copyright (c) 1998 Michael Shalayeff
 * All rights reserved.
 *
 * Redistribution and use in source and binary forms, with or without
 * modification, are permitted provided that the following conditions
 * are met:
 * 1. Redistributions of source code must retain the above copyright
 *    notice, this list of conditions and the following disclaimer.
 * 2. Redistributions in binary form must reproduce the above copyright
 *    notice, this list of conditions and the following disclaimer in the
 *    documentation and/or other materials provided with the distribution.
 *
 * THIS SOFTWARE IS PROVIDED BY THE AUTHOR ``AS IS'' AND ANY EXPRESS OR
 * IMPLIED WARRANTIES, INCLUDING, BUT NOT LIMITED TO, THE IMPLIED WARRANTIES
 * OF MERCHANTABILITY AND FITNESS FOR A PARTICULAR PURPOSE ARE DISCLAIMED.
 * IN NO EVENT SHALL THE AUTHOR BE LIABLE FOR ANY DIRECT, INDIRECT,
 * INCIDENTAL, SPECIAL, EXEMPLARY, OR CONSEQUENTIAL DAMAGES (INCLUDING, BUT
 * NOT LIMITED TO, PROCUREMENT OF SUBSTITUTE GOODS OR SERVICES; LOSS OF USE,
 * DATA, OR PROFITS; OR BUSINESS INTERRUPTION) HOWEVER CAUSED AND ON ANY
 * THEORY OF LIABILITY, WHETHER IN CONTRACT, STRICT LIABILITY, OR TORT
 * (INCLUDING NEGLIGENCE OR OTHERWISE) ARISING IN ANY WAY OUT OF THE USE OF
 * THIS SOFTWARE, EVEN IF ADVISED OF THE POSSIBILITY OF SUCH DAMAGE.
 */
/* </LICENSES> */

//! `ctime()`: a time as `Thu Jan  1 00:00:00 1970\n`, for boot(8)'s `time` command.
//!
//! Upstream: sys/lib/libsa/ctime.c @ 3ce1f3f79392
//!
//! ## Deviations
//! - The C returns a `static char buf[64]`; here the text is returned in a [`Ctime`] value.

use core::fmt;

use crate::hdr::types::Time;
use crate::snprintf;

/// `isleap(y)`.
const fn isleap(y: i64) -> bool {
    (y % 4 == 0 && y % 100 != 0) || y % 400 == 0
}

/// The text `ctime()` made.
pub struct Ctime {
    buf: [u8; 64],
    len: usize,
}

impl Ctime {
    /// The bytes, newline included.
    pub fn as_bytes(&self) -> &[u8] {
        &self.buf[..self.len.min(self.buf.len() - 1)]
    }
}

impl fmt::Display for Ctime {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        for &b in self.as_bytes() {
            fmt::Write::write_char(f, char::from(b))?;
        }
        Ok(())
    }
}

/// `ctime(clock)`.
pub fn ctime(clock: Time) -> Ctime {
    const WDAYS: [&str; 7] = ["Sun", "Mon", "Tue", "Wed", "Thu", "Fri", "Sat"];
    const MONTHS: [&str; 12] = [
        "Jan", "Feb", "Mar", "Apr", "May", "Jun", "Jul", "Aug", "Sep", "Oct", "Nov", "Dec",
    ];
    const MONTHCNT: [i64; 12] = [31, 28, 31, 30, 31, 30, 31, 31, 30, 31, 30, 31];

    let mut tt = clock;
    let ss = tt % 60;
    tt /= 60; // minutes
    let mm = tt % 60;
    tt /= 60; // hours
    let hh = tt % 24;
    tt /= 24; // days
    let wday = (4 + tt) % 7; // weekday, 'twas thursday when time started

    let mut year = 1970;
    while tt >= 365 {
        tt -= if isleap(year) { 366 } else { 365 };
        year += 1;
    }

    tt += 1; // days are 1-based

    let mut month = 0;
    while month < 12 && tt > MONTHCNT[month] {
        tt -= MONTHCNT[month];
        month += 1;
    }

    if month > 2 && isleap(year) {
        tt -= 1;
    }

    let mut out = Ctime {
        buf: [0; 64],
        len: 0,
    };
    out.len = snprintf!(
        &mut out.buf,
        "{} {}{:3} {:02}:{:02}:{:02} {}\n",
        usize::try_from(wday)
            .ok()
            .and_then(|w| WDAYS.get(w))
            .unwrap_or(&"???"),
        MONTHS.get(month).unwrap_or(&"???"),
        tt,
        hh,
        mm,
        ss,
        year
    );
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn epoch_and_leap_years() {
        assert_eq!(ctime(0).as_bytes(), b"Thu Jan  1 00:00:00 1970\n");
        // 2024-07-04 12:34:56 UTC, after the leap day the month walk takes off again
        assert_eq!(
            ctime(1_720_096_496).as_bytes(),
            b"Thu Jul  4 12:34:56 2024\n"
        );
        // The C's month walk counts February as 28 days and only corrects from April on:
        // 2024-02-29 reads as "Mar  1", as it does in OpenBSD's boot(8).
        assert_eq!(
            ctime(1_709_210_096).as_bytes(),
            b"Thu Mar  1 12:34:56 2024\n"
        );
    }
}
