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
 * Copyright (c) 2011 Manuel Bouyer.
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
 *
 */
/* </LICENSES> */

/* <CODE> */
//! RDC IDE controller registers (the PMX-1000 SoC's).
//!
//! Upstream: sys/dev/pci/pciide_rdc_reg.h @ 3ce1f3f79392
//!
//! ## Deviations
//! - The function-like macros are `const fn`s named in lower case. The configuration-space
//!   offsets `pciide.c` reads and writes are `i32` (`pci_conf_read`'s `reg`), bus space
//!   offsets `BusSize`, shift counts `i32`, register values and unused offsets `u32`.
//! - The `static` timing tables are `pub const` arrays named in capitals, of `u8` where the
//!   C has `int8_t` or `u_int8_t` (every entry is written to a byte-wide register or shifted
//!   into a field, never sign-extended).
#![allow(non_upper_case_globals)] // the C's mixed-case names (`PDC2xx_STATE`, `ACER_0x4B`)

// register definitions for the RDC ide controller as found in the
// PMX-1000 SoC
// ATA Timing Register
/// `RDCIDE_PATR`.
pub const RDCIDE_PATR: i32 = 0x40;
/// `RDCIDE_PATR_EN`.
pub const fn rdcide_patr_en(chan: i32) -> u32 {
    0x8000 << (chan * 16)
}
/// `RDCIDE_PATR_DEV1_TEN`.
pub const fn rdcide_patr_dev1_ten(chan: i32) -> u32 {
    0x4000 << (chan * 16)
}
/// `RDCIDE_PATR_SETUP`.
pub const fn rdcide_patr_setup(val: u32, chan: i32) -> u32 {
    (val << 12) << (chan * 16)
}
/// `RDCIDE_PATR_SETUP_MASK`.
pub const fn rdcide_patr_setup_mask(chan: i32) -> u32 {
    0x3000 << (chan * 16)
}
/// `RDCIDE_PATR_HOLD`.
pub const fn rdcide_patr_hold(val: u32, chan: i32) -> u32 {
    (val << 8) << (chan * 16)
}
/// `RDCIDE_PATR_HOLD_MASK`.
pub const fn rdcide_patr_hold_mask(chan: i32) -> u32 {
    0x0300 << (chan * 16)
}
/// `RDCIDE_PATR_DMAEN`.
pub const fn rdcide_patr_dmaen(chan: i32, drv: i32) -> u32 {
    (0x0008 << (drv * 4)) << (chan * 16)
}
/// `RDCIDE_PATR_ATA`.
pub const fn rdcide_patr_ata(chan: i32, drv: i32) -> u32 {
    (0x0004 << (drv * 4)) << (chan * 16)
}
/// `RDCIDE_PATR_IORDY`.
pub const fn rdcide_patr_iordy(chan: i32, drv: i32) -> u32 {
    (0x0002 << (drv * 4)) << (chan * 16)
}
/// `RDCIDE_PATR_FTIM`.
pub const fn rdcide_patr_ftim(chan: i32, drv: i32) -> u32 {
    (0x0001 << (drv * 4)) << (chan * 16)
}

// Primary and Secondary Device 1 ATA Timing
/// `RDCIDE_PSD1ATR`.
pub const RDCIDE_PSD1ATR: i32 = 0x44;
/// `RDCIDE_PSD1ATR_SETUP`.
pub const fn rdcide_psd1atr_setup(val: u32, chan: i32) -> u32 {
    (val << 2) << (chan * 4)
}
/// `RDCIDE_PSD1ATR_SETUP_MASK`.
pub const fn rdcide_psd1atr_setup_mask(chan: i32) -> u32 {
    0x0c << (chan * 4)
}
/// `RDCIDE_PSD1ATR_HOLD`.
pub const fn rdcide_psd1atr_hold(val: u32, chan: i32) -> u32 {
    val << (chan * 4)
}
/// `RDCIDE_PSD1ATR_HOLD_MASK`.
pub const fn rdcide_psd1atr_hold_mask(chan: i32) -> u32 {
    0x03 << (chan * 4)
}

/// `rdcide_setup`.
pub const RDCIDE_SETUP: [u8; 5] = [0, 0, 1, 2, 2];
/// `rdcide_hold`.
pub const RDCIDE_HOLD: [u8; 5] = [0, 0, 0, 1, 3];

// Ultra DMA Control and timing Register
/// `RDCIDE_UDCCR`.
pub const RDCIDE_UDCCR: i32 = 0x48;
/// `RDCIDE_UDCCR_EN`.
pub const fn rdcide_udccr_en(chan: i32, drv: i32) -> u32 {
    (1 << drv) << (chan * 2)
}
/// `RDCIDE_UDCCR_TIM`.
pub const fn rdcide_udccr_tim(val: u32, chan: i32, drv: i32) -> u32 {
    (val << (drv * 4)) << (chan * 8)
}
/// `RDCIDE_UDCCR_TIM_MASK`.
pub const fn rdcide_udccr_tim_mask(chan: i32, drv: i32) -> u32 {
    (0x3 << (drv * 4)) << (chan * 8)
}

/// `rdcide_udmatim`.
pub const RDCIDE_UDMATIM: [u8; 6] = [0, 1, 2, 1, 2, 1];

// IDE I/O Configuration Registers
/// `RDCIDE_IIOCR`.
pub const RDCIDE_IIOCR: i32 = 0x54;
/// `RDCIDE_IIOCR_CABLE`.
pub const fn rdcide_iiocr_cable(chan: i32, drv: i32) -> u32 {
    (0x10 << drv) << (chan * 2)
}
/// `RDCIDE_IIOCR_CLK`.
pub const fn rdcide_iiocr_clk(val: u32, chan: i32, drv: i32) -> u32 {
    (val << drv) << (chan * 2)
}
/// `RDCIDE_IIOCR_CLK_MASK`.
pub const fn rdcide_iiocr_clk_mask(chan: i32, drv: i32) -> u32 {
    (0x1001 << drv) << (chan * 2)
}

/// `rdcide_udmaclk`.
pub const RDCIDE_UDMACLK: [u32; 6] = [0x0000, 0x0000, 0x0000, 0x0001, 0x0001, 0x1000];

// Miscellaneous Control Register
/// `RDCIDE_MCR`.
pub const RDCIDE_MCR: u32 = 0x90;
/// `RDCIDE_MCR_RESET`.
pub const fn rdcide_mcr_reset(chan: i32) -> u32 {
    0x01000000 << chan
}
/* </CODE> */

/* <TESTS> */
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    #[ignore = "needs OPENBSD_SRC (just test-ref)"]
    fn defines_match_the_header() {
        let defs = crate::reftest::defines("sys/dev/pci/pciide_rdc_reg.h");

        crate::reftest::assert_defines!(defs; RDCIDE_PATR, RDCIDE_PSD1ATR, RDCIDE_UDCCR, RDCIDE_IIOCR, RDCIDE_MCR);
    }
}
/* </TESTS> */
