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

/* <CODE> */
//! Primitive clock interrupt routines: `arch/amd64/isa/clock.c`.
//!
//! Upstream: sys/arch/amd64/isa/clock.c @ 3ce1f3f79392
//!
//! Status: `wip`. Milestone M2 ports `gettick` and `i8254_delay`, the `delay(9)` the console
//! polls with before the TSC is calibrated; M5 adds the i8254 timecounter (`i8254_timecounter`,
//! `i8254_get_timecount`, `i8254_simple_get_timecount`, `i8254_inittimecounter[_simple]`),
//! `timer_mutex`, `startclocks`, `i8254_startclock`, `clockintr`, `i8254_initclocks`,
//! `i8254_start_both_clocks` and `setstatclockrate`. M8 adds the mc146818 time-of-day clock:
//! `mc146818_read/write`, `rtcget/put`, `bcdtobin/bintobcd`, `cmoscheck`, `rtc_update_century`,
//! `clock_expandyear`, `rtcgettime/settime`, `rtc_todr`, `rtcinit` (called from `cpu_startup`)
//! and `rtcalarm_suspend/resume/fired`. What is left of the RTC is its periodic interrupt
//! (`rtcintr`, `rtcdrain`, `rtcstart`, `rtcstop`), which is the statclock on the i8254 path.
//!
//! ## Deviations
//! - The RTC's periodic interrupt is not here: `i8254_start_both_clocks` establishes IRQ0 and
//!   reports the RTC's IRQ8 statclock and `rtcstart`; `setstatclockrate` on the i8254 path
//!   reports the rate change (an RTC register write).
//! - A negative `n` in `i8254_delay` is treated as 0 (the C indexes `delaytab` with it).
//! - `mc146818_read/write` drop the `sc` argument (the C always passes NULL: "XXX softc").
//! - `rtcget` returns `Err(EINVAL)` where the C returns -1, and fills the registers in place.
//!   The century byte is the plain `NVRAM_CENTURY` where the C keeps it in the `centb` variable.
//! - `rtc_update_century` is an atomic (the C's patchable `int`); `rtc_todr` is a `static`
//!   [`TodrChipHandle`] handed to `todr_attach`.
//! - `CLOCK_DEBUG` is not configured.
//! - The `#[cfg(test)]` tests below are not compiled by `just test`: amd64's modules build
//!   only for the bare target, as for the rest of `sys/arch/amd64/`.

use core::ffi::c_void;
use core::ptr;
use core::sync::atomic::{AtomicBool, AtomicI32, AtomicU32, AtomicU64, Ordering};

use crate::arch::amd64::amd64::machdep::{delay, initclock_is_i8254};
use crate::arch::amd64::include::cpufunc::{intr_disable, intr_restore};
use crate::arch::amd64::include::intr::IntrFn;
use crate::arch::amd64::include::intrdefs::{IPL_CLOCK, IPL_HIGH, IPL_MPSAFE, IST_PULSE};
use crate::arch::amd64::include::pio::{inb, outb};
use crate::arch::amd64::isa::isa_machdep::isa_intr_establish;
use crate::arch::amd64::isa::nvram::NVRAM_CENTURY;
use crate::conf::param::{HZ, UTC_OFFSET};
use crate::dev::clock_subr::TodrChipHandle;
use crate::dev::ic::i8253reg::{
    TIMER_16BIT, TIMER_CNTR0, TIMER_FREQ, TIMER_LATCH, TIMER_MODE, TIMER_RATEGEN, TIMER_SEL0,
    timer_div,
};
use crate::dev::ic::mc146818reg::{
    MC_AHOUR, MC_AMIN, MC_ASEC, MC_DOM, MC_DOW, MC_HOUR, MC_MIN, MC_MONTH, MC_NTODREGS, MC_REGB,
    MC_REGB_24HR, MC_REGB_AIE, MC_REGC, MC_REGC_AF, MC_REGD, MC_REGD_VRT, MC_SEC, MC_YEAR,
    McTodregs, mc146818_gettod, mc146818_puttod,
};
use crate::dev::isa::isareg::{IO_RTC, IO_TIMER1};
use crate::kern::clock_subr::{clock_secs_to_ymdhms, clock_ymdhms_to_secs};
use crate::kern::kern_clock::{PROFHZ, STATHZ};
use crate::kern::kern_clockintr::{clockintr_cpu_init, clockintr_dispatch};
use crate::kern::kern_lock::{mtx_enter, mtx_leave};
use crate::kern::kern_tc::{tc_init, timecounter};
use crate::kern::kern_time::todr_attach;
use crate::kprintf;
use crate::machine::intr::{splclock, splx};
use crate::sys::errno::Errno;
use crate::sys::mutex::Mutex;
use crate::sys::time::{ClockYmdhms, Timeval, timeradd};
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

/// `mc146818_read`: reads RTC register or NVRAM byte `reg`.
pub fn mc146818_read(reg: u32) -> u32 {
    // SAFETY: the mc146818 answers at IO_RTC (index) and IO_RTC + 1 (data) on every PC;
    // selecting a register and reading it is the chip's documented access sequence.
    unsafe {
        outb(IO_RTC, reg as u8);
        delay(1);
        u32::from(inb(IO_RTC + 1))
    }
}

/// `mc146818_write`: writes `datum` to RTC register or NVRAM byte `reg`.
pub fn mc146818_write(reg: u32, datum: u32) {
    // SAFETY: as for `mc146818_read`; the caller chose the register and the value.
    unsafe {
        outb(IO_RTC, reg as u8);
        delay(1);
        outb(IO_RTC + 1, datum as u8);
        delay(1);
    }
}

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

// rtcintr: the RTC's periodic interrupt (not ported, see the module's deviations).

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

// rtcdrain: the RTC's periodic interrupt (not ported, see the module's deviations).

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

// rtcstart, rtcstop: the RTC's periodic interrupt (not ported, see the module's deviations).

/// `rtcget`: reads the TOD/alarm registers; fails when the chip's battery is dead (VRT clear).
pub fn rtcget(regs: &mut McTodregs) -> Result<(), Errno> {
    if (mc146818_read(MC_REGD) & MC_REGD_VRT) == 0 {
        // XXX softc
        return Err(Errno::EINVAL);
    }
    mc146818_gettod(regs, mc146818_read);
    Ok(())
}

/// `rtcput`: writes the TOD/alarm registers.
pub fn rtcput(regs: &McTodregs) {
    mc146818_puttod(regs, mc146818_read, mc146818_write);
}

/// `bcdtobin`.
pub fn bcdtobin(n: i32) -> i32 {
    ((n >> 4) & 0x0f) * 10 + (n & 0x0f)
}

/// `bintobcd`.
pub fn bintobcd(n: i32) -> i32 {
    ((((n / 10) << 4) & 0xf0) as u8 as i32) | ((n % 10) & 0x0f)
}

/// `cmoscheck`: check whether the CMOS layout is "standard"-like (ie, not PS/2-like), to be
/// called at `splclock()`.
fn cmoscheck() -> bool {
    let mut cksum: u16 = 0;

    for i in 0x10..=0x2d {
        cksum = cksum.wrapping_add(mc146818_read(i) as u16);
    }

    u32::from(cksum) == (mc146818_read(0x2e) << 8) + mc146818_read(0x2f)
}

/// `rtc_update_century`: patchable to control century byte handling:
/// 1: always update
/// -1: never touch
/// 0: try to figure out itself
pub static RTC_UPDATE_CENTURY: AtomicI32 = AtomicI32::new(0);

/// `clock_expandyear`: expand a two-digit year as read from the clock chip into full width.
/// Being here, deal with the CMOS century byte.
fn clock_expandyear(clockyear: i32) -> i32 {
    let clockcentury = if clockyear < 70 { 20 } else { 19 };
    let clockyear = clockyear + 100 * clockcentury;

    if RTC_UPDATE_CENTURY.load(Ordering::Relaxed) < 0 {
        return clockyear;
    }

    let s = splclock();
    let cmoscentury = if cmoscheck() {
        mc146818_read(NVRAM_CENTURY) as i32
    } else {
        0
    };
    splx(s);
    if cmoscentury == 0 {
        return clockyear;
    }

    let cmoscentury = bcdtobin(cmoscentury);

    if cmoscentury != clockcentury {
        // XXX note: saying "century is 20" might confuse the naive.
        kprintf!(
            "WARNING: NVRAM century is {} but RTC year is {}\n",
            cmoscentury,
            clockyear
        );

        // Kludge to roll over century.
        if RTC_UPDATE_CENTURY.load(Ordering::Relaxed) > 0
            || (cmoscentury == 19 && clockcentury == 20 && clockyear == 2000)
        {
            kprintf!("WARNING: Setting NVRAM century to {}\n", clockcentury);
            let s = splclock();
            mc146818_write(NVRAM_CENTURY, bintobcd(clockcentury) as u32);
            splx(s);
        }
    } else if cmoscentury == 19 && RTC_UPDATE_CENTURY.load(Ordering::Relaxed) == 0 {
        RTC_UPDATE_CENTURY.store(1, Ordering::Relaxed); // will update later in resettodr()
    }

    clockyear
}

/// The date and time the TOD registers hold (`rtcgettime` and `rtcalarm_suspend` both decode
/// them this way); the day of the week is not read.
fn rtc_decode(rtclk: &McTodregs) -> ClockYmdhms {
    ClockYmdhms {
        dt_sec: bcdtobin(rtclk[MC_SEC as usize] as i32) as u8,
        dt_min: bcdtobin(rtclk[MC_MIN as usize] as i32) as u8,
        dt_hour: bcdtobin(rtclk[MC_HOUR as usize] as i32) as u8,
        dt_day: bcdtobin(rtclk[MC_DOM as usize] as i32) as u8,
        dt_mon: bcdtobin(rtclk[MC_MONTH as usize] as i32) as u8,
        dt_year: clock_expandyear(bcdtobin(rtclk[MC_YEAR as usize] as i32)) as u16,
        dt_wday: 0,
    }
}

/// `rtcgettime`: the chip's time, as the `todr_gettime` of `rtc_todr`.
pub fn rtcgettime(_handle: &TodrChipHandle, tv: &mut Timeval) -> Result<(), Errno> {
    let mut rtclk: McTodregs = [0; MC_NTODREGS];

    let s = splclock();
    if let Err(e) = rtcget(&mut rtclk) {
        splx(s);
        return Err(e);
    }
    splx(s);

    let dt = rtc_decode(&rtclk);

    tv.tv_sec = clock_ymdhms_to_secs(&dt) - i64::from(UTC_OFFSET.load(Ordering::Relaxed));
    tv.tv_usec = 0;
    Ok(())
}

/// `rtcsettime`: sets the chip's time, as the `todr_settime` of `rtc_todr`.
pub fn rtcsettime(_handle: &TodrChipHandle, tv: &mut Timeval) -> Result<(), Errno> {
    let mut rtclk: McTodregs = [0; MC_NTODREGS];

    let s = splclock();
    if rtcget(&mut rtclk).is_err() {
        rtclk = [0; MC_NTODREGS];
    }
    splx(s);

    let dt = clock_secs_to_ymdhms(tv.tv_sec + i64::from(UTC_OFFSET.load(Ordering::Relaxed)));

    rtclk[MC_SEC as usize] = bintobcd(i32::from(dt.dt_sec)) as u32;
    rtclk[MC_MIN as usize] = bintobcd(i32::from(dt.dt_min)) as u32;
    rtclk[MC_HOUR as usize] = bintobcd(i32::from(dt.dt_hour)) as u32;
    rtclk[MC_DOW as usize] = u32::from(dt.dt_wday) + 1;
    rtclk[MC_YEAR as usize] = bintobcd(i32::from(dt.dt_year) % 100) as u32;
    rtclk[MC_MONTH as usize] = bintobcd(i32::from(dt.dt_mon)) as u32;
    rtclk[MC_DOM as usize] = bintobcd(i32::from(dt.dt_day)) as u32;

    let s = splclock();
    rtcput(&rtclk);
    if RTC_UPDATE_CENTURY.load(Ordering::Relaxed) > 0 {
        let century = bintobcd(i32::from(dt.dt_year) / 100);
        mc146818_write(NVRAM_CENTURY, century as u32); // XXX softc
    }
    splx(s);
    Ok(())
}

/// `rtc_todr`: the mc146818 as a time-of-day clock chip.
static RTC_TODR: TodrChipHandle = TodrChipHandle {
    cookie: ptr::null_mut(),
    bus_cookie: ptr::null_mut(),
    todr_quality: 0,
    todr_gettime: rtcgettime,
    todr_settime: rtcsettime,
    todr_setwen: None,
};

/// `rtcinit`: attaches the mc146818 as a time-of-day clock.
pub fn rtcinit() {
    todr_attach(&RTC_TODR);
}

/// `rtcalarm_suspend`: arms the RTC's alarm `delta` from now, to wake the machine.
pub fn rtcalarm_suspend(delta: &Timeval) -> Result<(), Errno> {
    let mut rtclk: McTodregs = [0; MC_NTODREGS];

    let s = splclock();
    if let Err(e) = rtcget(&mut rtclk) {
        splx(s);
        return Err(e);
    }
    splx(s);

    let dt = rtc_decode(&rtclk);

    let tv = Timeval {
        tv_sec: clock_ymdhms_to_secs(&dt),
        tv_usec: 0,
    };

    let tv = timeradd(&tv, delta);

    let dt = clock_secs_to_ymdhms(tv.tv_sec);

    let s = splclock();
    if let Err(e) = rtcget(&mut rtclk) {
        splx(s);
        return Err(e);
    }
    rtclk[MC_ASEC as usize] = bintobcd(i32::from(dt.dt_sec)) as u32;
    rtclk[MC_AMIN as usize] = bintobcd(i32::from(dt.dt_min)) as u32;
    rtclk[MC_AHOUR as usize] = bintobcd(i32::from(dt.dt_hour)) as u32;
    rtcput(&rtclk);
    splx(s);

    while mc146818_read(MC_REGC) & MC_REGC_AF != 0 {}
    mc146818_write(MC_REGB, MC_REGB_24HR | MC_REGB_AIE);

    Ok(())
}

/// `rtcalarm_resume`.
pub fn rtcalarm_resume() {
    mc146818_write(MC_REGB, MC_REGB_24HR);
}

/// `rtcalarm_fired`: whether the RTC's alarm went off.
pub fn rtcalarm_fired() -> bool {
    if (mc146818_read(MC_REGB) & MC_REGB_AIE) == 0 {
        return false;
    }

    (mc146818_read(MC_REGC) & MC_REGC_AF) != 0
}

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
/* </CODE> */

/* <TESTS> */
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn bcd_conversions() {
        assert_eq!(bcdtobin(0x00), 0);
        assert_eq!(bcdtobin(0x09), 9);
        assert_eq!(bcdtobin(0x10), 10);
        assert_eq!(bcdtobin(0x26), 26);
        assert_eq!(bcdtobin(0x59), 59);
        assert_eq!(bcdtobin(0x99), 99);
        assert_eq!(bintobcd(0), 0x00);
        assert_eq!(bintobcd(9), 0x09);
        assert_eq!(bintobcd(10), 0x10);
        assert_eq!(bintobcd(26), 0x26);
        assert_eq!(bintobcd(59), 0x59);
        assert_eq!(bintobcd(99), 0x99);
        for n in 0..100 {
            assert_eq!(bcdtobin(bintobcd(n)), n);
        }
    }
}
/* </TESTS> */
