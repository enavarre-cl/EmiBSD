/*	$OpenBSD: _time.h,v 1.10 2022/10/25 16:30:30 millert Exp $	*/
/* <LICENSES> */
/*
 * Copyright (c) 1982, 1986, 1993
 *	The Regents of the University of California.  All rights reserved.
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
 */
/* </LICENSES> */

//! `<sys/_time.h>`: the clock ids, `struct itimerspec` and the `__CLOCK_*` encoding of a
//! thread's CPU clock.
//!
//! Upstream: sys/sys/_time.h @ 3ce1f3f79392
//!
//! Status: `ported`. `time_t` and `struct timespec`, which this header also declares, are
//! `sys/time.rs`'s (`Time`, `Timespec`). The `__CLOCK_*` macros are lower-case `const fn`s.

use crate::sys::time::Timespec;
use crate::sys::types::Clockid;

/// `CLOCKS_PER_SEC`.
pub const CLOCKS_PER_SEC: i32 = 100;

/// `__CLOCK_ENCODE(type, id)`: a clock id for thread `id`'s clock of `type`.
pub const fn clock_encode(r#type: Clockid, id: i32) -> Clockid {
    r#type | (id << 12)
}

/// `__CLOCK_TYPE(c)`.
pub const fn clock_type(c: Clockid) -> Clockid {
    c & 0xfff
}

/// `__CLOCK_PTID(c)`.
pub const fn clock_ptid(c: Clockid) -> i32 {
    (c >> 12) & 0xfffff
}

/// `CLOCK_REALTIME`.
pub const CLOCK_REALTIME: Clockid = 0;
/// `CLOCK_PROCESS_CPUTIME_ID`.
pub const CLOCK_PROCESS_CPUTIME_ID: Clockid = 2;
/// `CLOCK_MONOTONIC`.
pub const CLOCK_MONOTONIC: Clockid = 3;
/// `CLOCK_THREAD_CPUTIME_ID`.
pub const CLOCK_THREAD_CPUTIME_ID: Clockid = 4;
/// `CLOCK_UPTIME`.
pub const CLOCK_UPTIME: Clockid = 5;
/// `CLOCK_BOOTTIME`.
pub const CLOCK_BOOTTIME: Clockid = 6;

/// `struct itimerspec`: structure defined by POSIX 1003.1b to be like a itimerval, but with
/// timespecs. Used in the timer_*() system calls.
#[repr(C)]
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Itimerspec {
    /// `it_interval`: timer interval.
    pub it_interval: Timespec,
    /// `it_value`: timer expiration.
    pub it_value: Timespec,
}

impl Itimerspec {
    /// All zero.
    pub const fn new() -> Self {
        Self {
            it_interval: Timespec::new(0, 0),
            it_value: Timespec::new(0, 0),
        }
    }
}

/// `TIMER_RELTIME`: relative timer.
pub const TIMER_RELTIME: i32 = 0x0;
/// `TIMER_ABSTIME`: absolute timer.
pub const TIMER_ABSTIME: i32 = 0x1;

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn thread_clock_encoding() {
        let c = clock_encode(CLOCK_THREAD_CPUTIME_ID, 100_123);
        assert_eq!(clock_type(c), CLOCK_THREAD_CPUTIME_ID);
        assert_eq!(clock_ptid(c), 100_123);
    }
}
