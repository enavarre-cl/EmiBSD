/*	$OpenBSD: clock.c,v 1.44 2026/04/11 16:24:13 deraadt Exp $	*/
/*	$NetBSD: clock.c,v 1.1 2003/04/26 18:39:50 fvdl Exp $	*/
/* <LICENSES> */
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
/* </LICENSES> */

//! Primitive clock interrupt routines: `arch/amd64/isa/clock.c`.
//!
//! Upstream: sys/arch/amd64/isa/clock.c @ 3ce1f3f79392
//!
//! Status: `wip`. Milestone M2 ports `gettick` and `i8254_delay`, the `delay(9)` the console
//! polls with before the TSC is calibrated; M5 adds the i8254 timecounter (`i8254_timecounter`,
//! `i8254_get_timecount`, `i8254_simple_get_timecount`, `i8254_inittimecounter[_simple]`),
//! `timer_mutex`, `startclocks`, `i8254_startclock`, `clockintr`, `i8254_initclocks`,
//! `i8254_start_both_clocks` and `setstatclockrate`. The mc146818 real-time clock
//! (`mc146818_read/write`, `rtcintr`, `rtcstart/stop`, `rtcget/put`, `rtcgettime/settime`,
//! `rtcinit`, `rtcalarm_*`, `cmoscheck`, `clock_expandyear`, `bcdtobin/bintobcd`) comes with
//! the time-of-day clocks (`todr_attach`, M7).
//!
//! ## Deviations
//! - The RTC is not here (M7): `i8254_start_both_clocks` establishes IRQ0 and reports the
//!   RTC's IRQ8 statclock and `rtcstart`; `setstatclockrate` on the i8254 path reports the
//!   rate change (an RTC register write).
//! - A negative `n` in `i8254_delay` is treated as 0 (the C indexes `delaytab` with it).

use core::ffi::c_void;
use core::ptr;
use core::sync::atomic::{AtomicBool, AtomicU32, AtomicU64, Ordering};

use crate::arch::amd64::amd64::machdep::initclock_is_i8254;
use crate::arch::amd64::include::cpufunc::{intr_disable, intr_restore};
use crate::arch::amd64::include::intr::IntrFn;
use crate::arch::amd64::include::intrdefs::{IPL_CLOCK, IPL_HIGH, IPL_MPSAFE, IST_PULSE};
use crate::arch::amd64::include::pio::{inb, outb};
use crate::arch::amd64::isa::isa_machdep::isa_intr_establish;
use crate::conf::param::HZ;
use crate::dev::ic::i8253reg::{
    TIMER_16BIT, TIMER_CNTR0, TIMER_FREQ, TIMER_LATCH, TIMER_MODE, TIMER_RATEGEN, TIMER_SEL0,
    timer_div,
};
use crate::dev::isa::isareg::IO_TIMER1;
use crate::kern::kern_clock::{PROFHZ, STATHZ};
use crate::kern::kern_clockintr::{clockintr_cpu_init, clockintr_dispatch};
use crate::kern::kern_lock::{mtx_enter, mtx_leave};
use crate::kern::kern_tc::{tc_init, timecounter};
use crate::sys::mutex::Mutex;
use crate::sys::timetc::{Timecounter, TimecounterGet};
use crate::unported;

/* Timecounter on the i8254 */

/// `i8254_lastcount`.
static I8254_LASTCOUNT: AtomicU32 = AtomicU32::new(0);
/// `i8254_offset`.
static I8254_OFFSET: AtomicU32 = AtomicU32::new(0);
/// `i8254_ticked`.
static I8254_TICKED: AtomicBool = AtomicBool::new(false);

/// `i8254_timecounter`.
static I8254_TIMECOUNTER: Timecounter =
    Timecounter::new(i8254_get_timecount, !0u32, TIMER_FREQ as u64, "i8254", 0, 0);

/// `timer_mutex`.
static TIMER_MUTEX: Mutex = Mutex::new(IPL_HIGH);

/// `rtclock_tval`: the reload value timer 0 counts down from.
pub static RTCLOCK_TVAL: AtomicU64 = AtomicU64::new(0);

// mc146818_read, mc146818_write: the RTC (M7).

/// `startclocks`: starts timer 0 at `hz`.
pub fn startclocks() {
    mtx_enter(&TIMER_MUTEX);
    RTCLOCK_TVAL.store(
        timer_div(HZ.load(Ordering::Relaxed)) as u64,
        Ordering::Relaxed,
    );
    i8254_startclock();
    mtx_leave(&TIMER_MUTEX);
}

/// `clockintr`: the IRQ0 handler when the i8254 drives the clock interrupts.
pub fn clockintr(frame: *mut c_void) -> i32 {
    if ptr::fn_addr_eq(
        timecounter().tc_get_timecount.get(),
        i8254_get_timecount as TimecounterGet,
    ) {
        if I8254_TICKED.swap(false, Ordering::Relaxed) {
            // the timecounter saw the wrap already
        } else {
            I8254_OFFSET.fetch_add(
                RTCLOCK_TVAL.load(Ordering::Relaxed) as u32,
                Ordering::Relaxed,
            );
            I8254_LASTCOUNT.store(0, Ordering::Relaxed);
        }
    }

    clockintr_dispatch(frame);

    1
}

// rtcintr: the RTC's periodic interrupt (M7).

/// `gettick`: the current value of timer 0, latched.
pub fn gettick() -> i32 {
    // Don't want someone screwing with the counter while we're here.
    mtx_enter(&TIMER_MUTEX);
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
    mtx_leave(&TIMER_MUTEX);
    (i32::from(hi) << 8) | i32::from(lo)
}

/// `i8254_delay`: wait approximately `n` microseconds. Relies on timer 1 counting down from
/// `TIMER_FREQ / hz` at `TIMER_FREQ` Hz. Note: timer had better have been programmed before
/// this is first used! (Note that we use `rate generator' mode, which counts at 1:1; `square
/// wave' mode counts at 2:1).
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

    let limit = TIMER_FREQ / HZ.load(Ordering::Relaxed);

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

// rtcdrain: the RTC (M7).

/// `i8254_initclocks`: the i8254 drives hardclock and the RTC the statclock.
pub fn i8254_initclocks() {
    i8254_inittimecounter(); // hook the interrupt-based i8254 tc

    STATHZ.store(128, Ordering::Relaxed);
    PROFHZ.store(1024, Ordering::Relaxed); // XXX does not divide into 1 billion
}

/// `i8254_start_both_clocks`: establishes the clock interrupts on the i8254 (IRQ0) and the
/// RTC (IRQ8, see the module's deviations).
pub fn i8254_start_both_clocks() {
    clockintr_cpu_init(None);

    // While the clock interrupt handler isn't really MPSAFE, the i8254 can't really be used
    // as a clock on a true MP system.
    isa_intr_establish(
        ptr::null(),
        0,
        IST_PULSE,
        IPL_CLOCK | IPL_MPSAFE,
        clockintr as IntrFn,
        ptr::null_mut(),
        "clock",
    );
    // isa_intr_establish(NULL, 8, IST_PULSE, IPL_STATCLOCK | IPL_MPSAFE, rtcintr, 0, "rtc")
    let _ = unported!("rtcintr on IRQ8 (the mc146818 statclock, M7)");

    // rtcstart(): start the mc146818 clock
    let _ = unported!("rtcstart (the mc146818 clock, M7)");
}

// rtcstart, rtcstop, rtcget, rtcput, bcdtobin, bintobcd, cmoscheck, rtc_update_century,
// clock_expandyear, rtcgettime, rtcsettime, rtc_todr, rtcinit, rtcalarm_suspend,
// rtcalarm_resume, rtcalarm_fired: the RTC (M7).

/// `setstatclockrate`: on the i8254 path the RTC's rate register (see the module's
/// deviations); nothing on the LAPIC path.
pub fn setstatclockrate(arg: i32) {
    if initclock_is_i8254() {
        // mc146818_write(NULL, MC_REGA, MC_BASE_32_KHz | MC_RATE_128_Hz / MC_RATE_1024_Hz)
        let _ = arg;
        let _ = unported!("setstatclockrate: the mc146818 rate register (M7)");
    }
}

/// `i8254_inittimecounter`.
pub fn i8254_inittimecounter() {
    tc_init(&I8254_TIMECOUNTER);
}

/// `i8254_inittimecounter_simple`: if we're using lapic to drive hardclock, we can use a
/// simpler algorithm for the i8254 timecounters.
pub fn i8254_inittimecounter_simple() {
    I8254_TIMECOUNTER
        .tc_get_timecount
        .set(i8254_simple_get_timecount);
    I8254_TIMECOUNTER.tc_counter_mask.set(0x7fff);
    I8254_TIMECOUNTER.tc_frequency.set(TIMER_FREQ as u64);

    mtx_enter(&TIMER_MUTEX);
    RTCLOCK_TVAL.store(0x8000, Ordering::Relaxed);
    i8254_startclock();
    mtx_leave(&TIMER_MUTEX);

    tc_init(&I8254_TIMECOUNTER);
}

/// `i8254_startclock`: programs timer 0 as a rate generator reloading `rtclock_tval`.
pub fn i8254_startclock() {
    let tval = RTCLOCK_TVAL.load(Ordering::Relaxed);

    // SAFETY: the i8254's documented programming sequence for counter 0.
    unsafe {
        outb(
            IO_TIMER1 + TIMER_MODE,
            TIMER_SEL0 | TIMER_RATEGEN | TIMER_16BIT,
        );
        outb(IO_TIMER1 + TIMER_CNTR0, (tval & 0xff) as u8);
        outb(IO_TIMER1 + TIMER_CNTR0, (tval >> 8) as u8);
    }
}

/// `i8254_simple_get_timecount`.
pub fn i8254_simple_get_timecount(_tc: &Timecounter) -> u32 {
    (RTCLOCK_TVAL.load(Ordering::Relaxed) as u32).wrapping_sub(gettick() as u32)
}

/// `i8254_get_timecount`: the counter plus the wraps seen since the last clock interrupt.
pub fn i8254_get_timecount(_tc: &Timecounter) -> u32 {
    let s = intr_disable();

    // SAFETY: as for `gettick`.
    let (lo, hi) = unsafe {
        outb(IO_TIMER1 + TIMER_MODE, TIMER_SEL0 | TIMER_LATCH);
        (inb(IO_TIMER1 + TIMER_CNTR0), inb(IO_TIMER1 + TIMER_CNTR0))
    };

    let mut count = (RTCLOCK_TVAL.load(Ordering::Relaxed) as u32)
        .wrapping_sub((u32::from(hi) << 8) | u32::from(lo));

    if count < I8254_LASTCOUNT.load(Ordering::Relaxed) {
        I8254_TICKED.store(true, Ordering::Relaxed);
        I8254_OFFSET.fetch_add(
            RTCLOCK_TVAL.load(Ordering::Relaxed) as u32,
            Ordering::Relaxed,
        );
    }
    I8254_LASTCOUNT.store(count, Ordering::Relaxed);
    count = count.wrapping_add(I8254_OFFSET.load(Ordering::Relaxed));

    // SAFETY: `s` came from `intr_disable` above, on this CPU.
    unsafe { intr_restore(s) };

    count
}
