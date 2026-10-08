/*	$OpenBSD: i8253reg.h,v 1.3 2003/06/02 23:28:02 millert Exp $	*/
/*	$NetBSD: i8253reg.h,v 1.5 1998/01/19 11:38:00 drochner Exp $	*/
/* <LICENSES> */
/*-
 * Copyright (c) 1993 The Regents of the University of California.
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

/* <CODE> */
//! Register definitions for the Intel 8253 Programmable Interval Timer: `<dev/ic/i8253reg.h>`.
//!
//! Upstream: sys/dev/ic/i8253reg.h @ 3ce1f3f79392
//!
//! This chip has three independent 16-bit down counters that can be read on the fly. There are
//! three mode registers and three countdown registers. The countdown registers are addressed
//! directly, via the first three I/O ports. The three mode registers are accessed via the fourth
//! I/O port, with two bits in the mode byte indicating the register.
//!
//! To read the current value ("on the fly") from the countdown register, write a "latch"
//! command into the mode register, then read the stable value from the corresponding I/O port.
//! Reading in this manner has no side effects.
//!
//! The outputs of the three timers are connected as follows: timer 0 to irq 0 (`hardclock`),
//! timer 1 to dma channel 0 (dram refresh), timer 2 to the speaker (console beeps).
//!
//! ## Deviations
//! - `TIMER_DIV(x)` is the `const fn` [`timer_div`].

/// Frequency of all three count-down timers; `TIMER_FREQ / freq` is the appropriate count to
/// generate a frequency of `freq` Hz.
pub const TIMER_FREQ: i32 = 1_193_182;

/// `TIMER_DIV(x)`: the count for `x` Hz, rounded.
pub const fn timer_div(x: i32) -> i32 {
    (TIMER_FREQ + x / 2) / x
}

/// Timer 0 counter port.
pub const TIMER_CNTR0: u16 = 0;
/// Timer 1 counter port.
pub const TIMER_CNTR1: u16 = 1;
/// Timer 2 counter port.
pub const TIMER_CNTR2: u16 = 2;
/// Timer mode port.
pub const TIMER_MODE: u16 = 3;
/// Select counter 0.
pub const TIMER_SEL0: u8 = 0x00;
/// Select counter 1.
pub const TIMER_SEL1: u8 = 0x40;
/// Select counter 2.
pub const TIMER_SEL2: u8 = 0x80;
/// Mode 0, intr on terminal cnt.
pub const TIMER_INTTC: u8 = 0x00;
/// Mode 1, one shot.
pub const TIMER_ONESHOT: u8 = 0x02;
/// Mode 2, rate generator.
pub const TIMER_RATEGEN: u8 = 0x04;
/// Mode 3, square wave.
pub const TIMER_SQWAVE: u8 = 0x06;
/// Mode 4, s/w triggered strobe.
pub const TIMER_SWSTROBE: u8 = 0x08;
/// Mode 5, h/w triggered strobe.
pub const TIMER_HWSTROBE: u8 = 0x0a;
/// Latch counter for reading.
pub const TIMER_LATCH: u8 = 0x00;
/// R/W counter LSB.
pub const TIMER_LSB: u8 = 0x10;
/// R/W counter MSB.
pub const TIMER_MSB: u8 = 0x20;
/// R/W counter 16 bits, LSB first.
pub const TIMER_16BIT: u8 = 0x30;
/// Count in BCD.
pub const TIMER_BCD: u8 = 0x01;
/* </CODE> */

/* <TESTS> */
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn divisor_for_hz() {
        assert_eq!(timer_div(100), 11932);
    }

    #[test]
    #[ignore = "needs OPENBSD_SRC (just test-ref)"]
    fn values_match_the_c_header() {
        let defs = crate::reftest::defines("sys/dev/ic/i8253reg.h");
        let ours: &[(&str, i64)] = &[
            ("TIMER_FREQ", TIMER_FREQ as i64),
            ("TIMER_CNTR0", TIMER_CNTR0 as i64),
            ("TIMER_CNTR2", TIMER_CNTR2 as i64),
            ("TIMER_MODE", TIMER_MODE as i64),
            ("TIMER_SEL0", TIMER_SEL0 as i64),
            ("TIMER_SEL2", TIMER_SEL2 as i64),
            ("TIMER_RATEGEN", TIMER_RATEGEN as i64),
            ("TIMER_SQWAVE", TIMER_SQWAVE as i64),
            ("TIMER_LATCH", TIMER_LATCH as i64),
            ("TIMER_16BIT", TIMER_16BIT as i64),
            ("TIMER_BCD", TIMER_BCD as i64),
        ];
        for (name, value) in ours {
            assert_eq!(crate::reftest::int(&defs, name), Some(*value), "{name}");
        }
    }
}
/* </TESTS> */
