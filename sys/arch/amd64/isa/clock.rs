/*	$OpenBSD: clock.c,v 1.44 2026/04/11 16:24:13 deraadt Exp $	*/
/*	$NetBSD: clock.c,v 1.1 2003/04/26 18:39:50 fvdl Exp $	*/

/*-
 * Copyright (c) 1993, 1994 Charles M. Hannum.
 * Copyright (c) 1990 The Regents of the University of California.
 * All rights reserved.
 *
 * This code is derived from software contributed to Berkeley by
 * William Jolitz and Don Ahn.
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
 *	@(#)clock.c	7.2 (Berkeley) 5/12/91
 */
/*
 * Mach Operating System
 * Copyright (c) 1991,1990,1989 Carnegie Mellon University
 * All Rights Reserved.
 *
 * Permission to use, copy, modify and distribute this software and its
 * documentation is hereby granted, provided that both the copyright
 * notice and this permission notice appear in all copies of the
 * software, derivative works or modified versions, and any portions
 * thereof, and that both notices appear in supporting documentation.
 *
 * CARNEGIE MELLON ALLOWS FREE USE OF THIS SOFTWARE IN ITS "AS IS"
 * CONDITION.  CARNEGIE MELLON DISCLAIMS ANY LIABILITY OF ANY KIND FOR
 * ANY DAMAGES WHATSOEVER RESULTING FROM THE USE OF THIS SOFTWARE.
 *
 * Carnegie Mellon requests users of this software to return to
 *
 *  Software Distribution Coordinator  or  Software.Distribution@CS.CMU.EDU
 *  School of Computer Science
 *  Carnegie Mellon University
 *  Pittsburgh PA 15213-3890
 *
 * any improvements or extensions that they make and grant Carnegie Mellon
 * the rights to redistribute these changes.
 */
/*
  Copyright 1988, 1989 by Intel Corporation, Santa Clara, California.

        All Rights Reserved

Permission to use, copy, modify, and distribute this software and
its documentation for any purpose and without fee is hereby
granted, provided that the above copyright notice appears in all
copies and that both the copyright notice and this permission notice
appear in supporting documentation, and that the name of Intel
not be used in advertising or publicity pertaining to distribution
of the software without specific, written prior permission.

INTEL DISCLAIMS ALL WARRANTIES WITH REGARD TO THIS SOFTWARE
INCLUDING ALL IMPLIED WARRANTIES OF MERCHANTABILITY AND FITNESS,
IN NO EVENT SHALL INTEL BE LIABLE FOR ANY SPECIAL, INDIRECT, OR
CONSEQUENTIAL DAMAGES OR ANY DAMAGES WHATSOEVER RESULTING FROM
LOSS OF USE, DATA OR PROFITS, WHETHER IN ACTION OF CONTRACT,
NEGLIGENCE, OR OTHER TORTIOUS ACTION, ARISING OUT OF OR IN CONNECTION
WITH THE USE OR PERFORMANCE OF THIS SOFTWARE.
*/

//! Primitive clock interrupt routines: `arch/amd64/isa/clock.c`.
//!
//! Upstream: sys/arch/amd64/isa/clock.c @ 3ce1f3f79392
//!
//! Status: `wip`. Milestone M2 ports `gettick` and `i8254_delay`, the `delay(9)` the console
//! polls with before the TSC is calibrated. The clock interrupt, the timecounter, the RTC
//! (`mc146818_*`, `rtcget`/`rtcput`, `inittodr`/`resettodr`) and `startclocks` arrive with M4.
//!
//! ## Deviations
//! - `timer_mutex` arrives with M5; `gettick` masks interrupts, as the C also does.
//! - A negative `n` is treated as 0 (the C indexes `delaytab` with it).

use crate::arch::amd64::include::cpufunc::{intr_disable, intr_restore};
use crate::arch::amd64::include::pio::{inb, outb};
use crate::conf::param::HZ;
use crate::dev::ic::i8253reg::{TIMER_CNTR0, TIMER_FREQ, TIMER_LATCH, TIMER_MODE, TIMER_SEL0};
use crate::dev::isa::isareg::IO_TIMER1;

/// `gettick`: the current value of timer 0, latched.
pub fn gettick() -> i32 {
    // Don't want someone screwing with the counter while we're here.
    // mtx_enter(&timer_mutex): M5.
    let s = intr_disable();
    // SAFETY: the i8254 is at IO_TIMER1 on every PC; latching and reading counter 0 is its
    // documented read-on-the-fly sequence and has no other effect.
    let (lo, hi) = unsafe {
        // Select counter 0 and latch it.
        outb(IO_TIMER1 + TIMER_MODE, TIMER_SEL0 | TIMER_LATCH);
        (inb(IO_TIMER1 + TIMER_CNTR0), inb(IO_TIMER1 + TIMER_CNTR0))
    };
    // SAFETY: `s` came from `intr_disable` just above, on this CPU.
    unsafe { intr_restore(s) };
    (i32::from(hi) << 8) | i32::from(lo)
}

/// `i8254_delay`: wait approximately `n` microseconds. Relies on timer 1 counting down from
/// `TIMER_FREQ / hz` at `TIMER_FREQ` Hz. Note: timer had better be running before this is
/// first used!
pub fn i8254_delay(n: i32) {
    const DELAYTAB: [i32; 26] = [
        0, 2, 3, 4, 5, 6, 7, 9, 10, 11, 12, 13, 15, 16, 17, 18, 19, 21, 22, 23, 24, 25, 27, 28, 29,
        30,
    ];

    // Read the counter first, so that the rest of the setup overhead is counted.
    let mut otick = gettick();

    let mut n = if n <= 25 {
        DELAYTAB[n.max(0) as usize]
    } else {
        // Force 64-bit math to avoid 32-bit overflow if possible.
        (i64::from(n) * i64::from(TIMER_FREQ) / 1_000_000) as i32
    };

    let limit = TIMER_FREQ / HZ.load(core::sync::atomic::Ordering::Relaxed);

    while n > 0 {
        let tick = gettick();
        if tick > otick {
            n -= limit - (tick - otick);
        } else {
            n -= otick - tick;
        }
        otick = tick;
    }
}
