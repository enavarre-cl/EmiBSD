/* <LICENSES> */
/*
 * Copyright (c) 2026 Emilio Navarrete Lineros <enavarre@outlook.com>
 *
 * Permission to use, copy, modify, and distribute this software for any
 * purpose with or without fee is hereby granted, provided that the above
 * copyright notice and this permission notice appear in all copies.
 *
 * THE SOFTWARE IS PROVIDED "AS IS" AND THE AUTHOR DISCLAIMS ALL WARRANTIES
 * WITH REGARD TO THIS SOFTWARE INCLUDING ALL IMPLIED WARRANTIES OF
 * MERCHANTABILITY AND FITNESS. IN NO EVENT SHALL THE AUTHOR BE LIABLE FOR
 * ANY SPECIAL, DIRECT, INDIRECT, OR CONSEQUENTIAL DAMAGES OR ANY DAMAGES
 * WHATSOEVER RESULTING FROM LOSS OF USE, DATA OR PROFITS, WHETHER IN AN
 * ACTION OF CONTRACT, NEGLIGENCE OR OTHER TORTIOUS ACTION, ARISING OUT OF
 * OR IN CONNECTION WITH THE USE OR PERFORMANCE OF THIS SOFTWARE.
 */

/*
 * Copyright (c) 1995 Carnegie-Mellon University.
 * All rights reserved.
 *
 * Permission to use, copy, modify and distribute this software and
 * its documentation is hereby granted, provided that both the copyright
 * notice and this permission notice appear in all copies of the
 * software, derivative works or modified versions, and any portions
 * thereof, and that both notices appear in supporting documentation.
 *
 * CARNEGIE MELLON ALLOWS FREE USE OF THIS SOFTWARE IN ITS "AS IS"
 * CONDITION.  CARNEGIE MELLON DISCLAIMS ANY LIABILITY OF ANY KIND
 * FOR ANY DAMAGES WHATSOEVER RESULTING FROM THE USE OF THIS SOFTWARE.
 *
 * Carnegie Mellon requests users of this software to return to
 *
 *  Software Distribution Coordinator  or  Software.Distribution@CS.CMU.EDU
 *  School of Computer Science
 *  Carnegie Mellon University
 *  Pittsburgh PA 15213-3890
 *
 * any improvements or extensions that they make and grant Carnegie the
 * rights to redistribute these changes.
 */
/* </LICENSES> */

/* <CODE> */
//! Definitions for the Motorola MC146818A Real Time Clock: `<dev/ic/mc146818reg.h>`.
//!
//! Upstream: sys/dev/ic/mc146818reg.h @ 3ce1f3f79392
//!
//! They also apply for the (compatible) Dallas Semiconductor DS1287A RTC. The MC146818A has 16
//! registers. The first 10 contain time-of-year and alarm data. The rest contain various
//! control and status bits.
//!
//! To read or write the registers, one writes the register number to the RTC's control port,
//! then either reads from or writes the new data to the RTC's data port. Since the locations
//! of these ports and the method used to access them can be machine-dependent, the low-level
//! details of reading and writing the RTC's registers are handled by machine-specific
//! functions (`mc146818_read` and `mc146818_write`; amd64's are in `isa/clock.rs`).
//!
//! The time-of-year and alarm data can be expressed in either binary or BCD, and they are
//! selected by a bit in register B. The "hour" fields can either be expressed in AM/PM format
//! or in 24-hour format, again selected by a bit in register B. It is assumed that if systems
//! are going to use BCD (rather than binary) mode, or AM/PM hour format, they will do the
//! appropriate conversions in machine-dependent code. Also, if the clock is switched between
//! BCD and binary mode, or between AM/PM mode and 24-hour mode, the time-of-day and alarm
//! registers are NOT automatically reset; they must be reprogrammed with correct values.
//!
//! Status: `ported`.
//!
//! ## Deviations
//! - `mc146818_read`/`mc146818_write` are not declared here: Rust has no prototypes, and the
//!   machine-dependent definitions are what [`mc146818_gettod`] and [`mc146818_puttod`] are
//!   handed, as closures (the `sc` argument of the C macros is whatever the closures capture).
//! - `MC146818_GETTOD(sc, regs)` and `MC146818_PUTTOD(sc, regs)` are those two functions.
//! - The constants with a lower-case unit in their C name are upper case (`MC_RATE_1024_Hz` is
//!   `MC_RATE_1024_HZ`, `MC_BASE_32_KHz` is `MC_BASE_32_KHZ`, `MC_BASE_4_MHz` is
//!   `MC_BASE_4_MHZ`), as Rust's naming lint wants.
//! - `mc_todregs` is [`McTodregs`], an array of `u32` (`u_int`).

/// Time of year: seconds (0-59).
pub const MC_SEC: u32 = 0x0;
/// Alarm: seconds.
pub const MC_ASEC: u32 = 0x1;
/// Time of year: minutes (0-59).
pub const MC_MIN: u32 = 0x2;
/// Alarm: minutes.
pub const MC_AMIN: u32 = 0x3;
/// Time of year: hour.
pub const MC_HOUR: u32 = 0x4;
/// Alarm: hour.
pub const MC_AHOUR: u32 = 0x5;
/// Time of year: day of week (1-7).
pub const MC_DOW: u32 = 0x6;
/// Time of year: day of month (1-31).
pub const MC_DOM: u32 = 0x7;
/// Time of year: month (1-12).
pub const MC_MONTH: u32 = 0x8;
/// Time of year: year in century (0-99).
pub const MC_YEAR: u32 = 0x9;

/// Control register A.
pub const MC_REGA: u32 = 0xa;

/// Interrupt rate select mask (see below).
pub const MC_REGA_RSMASK: u32 = 0x0f;
/// Divisor select mask (see below).
pub const MC_REGA_DVMASK: u32 = 0x70;
/// Update in progress; read only.
pub const MC_REGA_UIP: u32 = 0x80;

/// Control register B.
pub const MC_REGB: u32 = 0xb;

/// Daylight Saving Enable.
pub const MC_REGB_DSE: u32 = 0x01;
/// 24-hour mode (AM/PM mode when clear).
pub const MC_REGB_24HR: u32 = 0x02;
/// Binary mode (BCD mode when clear).
pub const MC_REGB_BINARY: u32 = 0x04;
/// Square wave enable, ONLY in BQ3285E.
pub const MC_REGB_SQWE: u32 = 0x08;
/// Update End interrupt enable.
pub const MC_REGB_UIE: u32 = 0x10;
/// Alarm interrupt enable.
pub const MC_REGB_AIE: u32 = 0x20;
/// Periodic interrupt enable.
pub const MC_REGB_PIE: u32 = 0x40;
/// Allow time to be set; stops updates.
pub const MC_REGB_SET: u32 = 0x80;

/// Control register C.
pub const MC_REGC: u32 = 0xc;

/// Update End interrupt flag.
pub const MC_REGC_UF: u32 = 0x10;
/// Alarm interrupt flag.
pub const MC_REGC_AF: u32 = 0x20;
/// Periodic interrupt flag.
pub const MC_REGC_PF: u32 = 0x40;
/// Interrupt request pending flag.
pub const MC_REGC_IRQF: u32 = 0x80;

/// Control register D.
pub const MC_REGD: u32 = 0xd;

/// Valid RAM and Time bit.
pub const MC_REGD_VRT: u32 = 0x80;

/// 14 registers; CMOS follows.
pub const MC_NREGS: u32 = 0xe;
/// 10 of those regs are for TOD and alarm.
pub const MC_NTODREGS: usize = 0xa;

/// Start of NVRAM: offset 14.
pub const MC_NVRAM_START: u32 = 0xe;
/// 50 bytes of NVRAM.
pub const MC_NVRAM_SIZE: u32 = 50;

// Periodic Interrupt Rate Select constants (Control register A).

/// No periodic interrupt.
pub const MC_RATE_NONE: u32 = 0x0;
/// 256 Hz if `MC_BASE_32_KHZ`, else 32768 Hz.
pub const MC_RATE_1: u32 = 0x1;
/// 128 Hz if `MC_BASE_32_KHZ`, else 16384 Hz.
pub const MC_RATE_2: u32 = 0x2;
/// 122.070 us period.
pub const MC_RATE_8192_HZ: u32 = 0x3;
/// 244.141 us period.
pub const MC_RATE_4096_HZ: u32 = 0x4;
/// 488.281 us period.
pub const MC_RATE_2048_HZ: u32 = 0x5;
/// 976.562 us period.
pub const MC_RATE_1024_HZ: u32 = 0x6;
/// 1.953125 ms period.
pub const MC_RATE_512_HZ: u32 = 0x7;
/// 3.90625 ms period.
pub const MC_RATE_256_HZ: u32 = 0x8;
/// 7.8125 ms period.
pub const MC_RATE_128_HZ: u32 = 0x9;
/// 15.625 ms period.
pub const MC_RATE_64_HZ: u32 = 0xa;
/// 31.25 ms period.
pub const MC_RATE_32_HZ: u32 = 0xb;
/// 62.5 ms period.
pub const MC_RATE_16_HZ: u32 = 0xc;
/// 125 ms period.
pub const MC_RATE_8_HZ: u32 = 0xd;
/// 250 ms period.
pub const MC_RATE_4_HZ: u32 = 0xe;
/// 500 ms period.
pub const MC_RATE_2_HZ: u32 = 0xf;

// Time base (divisor select) constants (Control register A).

/// 4MHz crystal.
pub const MC_BASE_4_MHZ: u32 = 0x00;
/// 1MHz crystal.
pub const MC_BASE_1_MHZ: u32 = 0x10;
/// 32KHz crystal.
pub const MC_BASE_32_KHZ: u32 = 0x20;
/// Actually, both of these reset.
pub const MC_BASE_NONE: u32 = 0x60;
/// Reset the divider.
pub const MC_BASE_RESET: u32 = 0x70;

/// `mc_todregs`: a collection of TOD/Alarm registers.
pub type McTodregs = [u32; MC_NTODREGS];

/// `MC146818_GETTOD`: get all of the TOD/Alarm registers. Must be called at `splhigh()`, and
/// with the RTC properly set up. `read` is `mc146818_read(sc, _)`.
pub fn mc146818_gettod(regs: &mut McTodregs, read: impl Fn(u32) -> u32) {
    // update in progress; spin loop
    while read(MC_REGA) & MC_REGA_UIP != 0 {}

    loop {
        // read all of the tod/alarm regs
        for (i, reg) in regs.iter_mut().enumerate() {
            *reg = read(i as u32);
        }
        if regs[MC_SEC as usize] == read(MC_SEC) {
            break;
        }
    }
}

/// `MC146818_PUTTOD`: set all of the TOD/Alarm registers. Must be called at `splhigh()`, and
/// with the RTC properly set up. `read` and `write` are `mc146818_read(sc, _)` and
/// `mc146818_write(sc, _, _)`.
pub fn mc146818_puttod(regs: &McTodregs, read: impl Fn(u32) -> u32, write: impl Fn(u32, u32)) {
    // stop updates while setting
    write(MC_REGB, read(MC_REGB) | MC_REGB_SET);

    // write all of the tod/alarm regs
    for (i, reg) in regs.iter().enumerate() {
        write(i as u32, *reg);
    }

    // reenable updates
    write(MC_REGB, read(MC_REGB) & !MC_REGB_SET);
}
/* </CODE> */

/* <TESTS> */
#[cfg(test)]
mod tests {
    use super::*;

    use core::cell::{Cell, RefCell};

    /// A fake chip: 14 registers, register A's UIP bit set for the first `uip_reads` reads of it,
    /// and the seconds register ticking over once, between the first sweep's two reads of it.
    struct FakeRtc {
        regs: RefCell<[u32; 14]>,
        uip_reads: Cell<u32>,
        sec_ticks: Cell<u32>,
        log: RefCell<std::vec::Vec<(u32, u32)>>,
    }

    impl FakeRtc {
        fn new(regs: [u32; 14]) -> Self {
            Self {
                regs: RefCell::new(regs),
                uip_reads: Cell::new(0),
                sec_ticks: Cell::new(0),
                log: RefCell::new(std::vec::Vec::new()),
            }
        }

        fn read(&self, reg: u32) -> u32 {
            let mut v = self.regs.borrow()[reg as usize];
            if reg == MC_REGA && self.uip_reads.get() > 0 {
                self.uip_reads.set(self.uip_reads.get() - 1);
                v |= MC_REGA_UIP;
            }
            if reg == MC_SEC && self.sec_ticks.get() > 0 {
                // The seconds changed since the sweep read them: the sweep must go round again.
                self.sec_ticks.set(self.sec_ticks.get() - 1);
                self.regs.borrow_mut()[MC_SEC as usize] += 1;
            }
            v
        }

        fn write(&self, reg: u32, datum: u32) {
            self.log.borrow_mut().push((reg, datum));
            self.regs.borrow_mut()[reg as usize] = datum;
        }
    }

    #[test]
    fn gettod_waits_for_uip_and_retries_a_torn_read() {
        let rtc = FakeRtc::new([
            0x56, 0, 0x34, 0, 0x12, 0, 0x07, 0x03, 0x10, 0x26, 0, 0, 0, 0,
        ]);
        rtc.uip_reads.set(3);
        rtc.sec_ticks.set(1);

        let mut regs: McTodregs = [0; MC_NTODREGS];
        mc146818_gettod(&mut regs, |r| rtc.read(r));

        // The second sweep saw seconds 0x57 twice (the first sweep saw 0x56 then 0x57).
        assert_eq!(rtc.uip_reads.get(), 0);
        assert_eq!(rtc.sec_ticks.get(), 0);
        assert_eq!(regs[MC_SEC as usize], 0x57);
        assert_eq!(regs[MC_MIN as usize], 0x34);
        assert_eq!(regs[MC_HOUR as usize], 0x12);
        assert_eq!(regs[MC_DOM as usize], 0x03);
        assert_eq!(regs[MC_MONTH as usize], 0x10);
        assert_eq!(regs[MC_YEAR as usize], 0x26);
    }

    #[test]
    fn puttod_brackets_the_write_with_set() {
        let rtc = FakeRtc::new([0; 14]);
        rtc.regs.borrow_mut()[MC_REGB as usize] = MC_REGB_24HR;

        let regs: McTodregs = [1, 2, 3, 4, 5, 6, 7, 8, 9, 10];
        mc146818_puttod(&regs, |r| rtc.read(r), |r, d| rtc.write(r, d));

        let log = rtc.log.borrow();
        assert_eq!(log.first(), Some(&(MC_REGB, MC_REGB_24HR | MC_REGB_SET)));
        assert_eq!(log.last(), Some(&(MC_REGB, MC_REGB_24HR)));
        assert_eq!(log.len(), 2 + MC_NTODREGS);
        for (i, want) in regs.iter().enumerate() {
            assert_eq!(log[1 + i], (i as u32, *want));
        }
        assert_eq!(rtc.regs.borrow()[MC_REGB as usize], MC_REGB_24HR);
    }

    #[test]
    #[ignore = "needs OPENBSD_SRC (just test-ref)"]
    fn values_match_the_c_header() {
        let defs = crate::reftest::defines("sys/dev/ic/mc146818reg.h");
        let ours: &[(&str, i64)] = &[
            ("MC_SEC", MC_SEC as i64),
            ("MC_ASEC", MC_ASEC as i64),
            ("MC_MIN", MC_MIN as i64),
            ("MC_AMIN", MC_AMIN as i64),
            ("MC_HOUR", MC_HOUR as i64),
            ("MC_AHOUR", MC_AHOUR as i64),
            ("MC_DOW", MC_DOW as i64),
            ("MC_DOM", MC_DOM as i64),
            ("MC_MONTH", MC_MONTH as i64),
            ("MC_YEAR", MC_YEAR as i64),
            ("MC_REGA", MC_REGA as i64),
            ("MC_REGA_RSMASK", MC_REGA_RSMASK as i64),
            ("MC_REGA_DVMASK", MC_REGA_DVMASK as i64),
            ("MC_REGA_UIP", MC_REGA_UIP as i64),
            ("MC_REGB", MC_REGB as i64),
            ("MC_REGB_DSE", MC_REGB_DSE as i64),
            ("MC_REGB_24HR", MC_REGB_24HR as i64),
            ("MC_REGB_BINARY", MC_REGB_BINARY as i64),
            ("MC_REGB_SQWE", MC_REGB_SQWE as i64),
            ("MC_REGB_UIE", MC_REGB_UIE as i64),
            ("MC_REGB_AIE", MC_REGB_AIE as i64),
            ("MC_REGB_PIE", MC_REGB_PIE as i64),
            ("MC_REGB_SET", MC_REGB_SET as i64),
            ("MC_REGC", MC_REGC as i64),
            ("MC_REGC_UF", MC_REGC_UF as i64),
            ("MC_REGC_AF", MC_REGC_AF as i64),
            ("MC_REGC_PF", MC_REGC_PF as i64),
            ("MC_REGC_IRQF", MC_REGC_IRQF as i64),
            ("MC_REGD", MC_REGD as i64),
            ("MC_REGD_VRT", MC_REGD_VRT as i64),
            ("MC_NREGS", MC_NREGS as i64),
            ("MC_NTODREGS", MC_NTODREGS as i64),
            ("MC_NVRAM_START", MC_NVRAM_START as i64),
            ("MC_NVRAM_SIZE", MC_NVRAM_SIZE as i64),
            ("MC_RATE_NONE", MC_RATE_NONE as i64),
            ("MC_RATE_1", MC_RATE_1 as i64),
            ("MC_RATE_2", MC_RATE_2 as i64),
            ("MC_RATE_8192_Hz", MC_RATE_8192_HZ as i64),
            ("MC_RATE_4096_Hz", MC_RATE_4096_HZ as i64),
            ("MC_RATE_2048_Hz", MC_RATE_2048_HZ as i64),
            ("MC_RATE_1024_Hz", MC_RATE_1024_HZ as i64),
            ("MC_RATE_512_Hz", MC_RATE_512_HZ as i64),
            ("MC_RATE_256_Hz", MC_RATE_256_HZ as i64),
            ("MC_RATE_128_Hz", MC_RATE_128_HZ as i64),
            ("MC_RATE_64_Hz", MC_RATE_64_HZ as i64),
            ("MC_RATE_32_Hz", MC_RATE_32_HZ as i64),
            ("MC_RATE_16_Hz", MC_RATE_16_HZ as i64),
            ("MC_RATE_8_Hz", MC_RATE_8_HZ as i64),
            ("MC_RATE_4_Hz", MC_RATE_4_HZ as i64),
            ("MC_RATE_2_Hz", MC_RATE_2_HZ as i64),
            ("MC_BASE_4_MHz", MC_BASE_4_MHZ as i64),
            ("MC_BASE_1_MHz", MC_BASE_1_MHZ as i64),
            ("MC_BASE_32_KHz", MC_BASE_32_KHZ as i64),
            ("MC_BASE_NONE", MC_BASE_NONE as i64),
            ("MC_BASE_RESET", MC_BASE_RESET as i64),
        ];
        for (name, value) in ours {
            assert_eq!(crate::reftest::int(&defs, name), Some(*value), "{name}");
        }
    }
}
/* </TESTS> */
