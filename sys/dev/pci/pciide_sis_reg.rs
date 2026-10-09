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
 * Copyright (c) 1998 Manuel Bouyer.
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
//! SiS IDE controller registers (5513, 5518, 96x and their host bridges).
//!
//! Upstream: sys/dev/pci/pciide_sis_reg.h @ 3ce1f3f79392
//!
//! ## Deviations
//! - The function-like macros are `const fn`s named in lower case. The configuration-space
//!   offsets `pciide.c` reads and writes are `i32` (`pci_conf_read`'s `reg`), bus space
//!   offsets `BusSize`, shift counts `i32`, register values and unused offsets `u32`.
//! - The `static` timing tables are `pub const` arrays named in capitals, of `u8` where the
//!   C has `int8_t` or `u_int8_t` (every entry is written to a byte-wide register or shifted
//!   into a field, never sign-extended).
//! - `SIS_TIM133(reg57, channel, drive)`'s conditional is an `if`.
//! - `struct pciide_sis` is [`PciideSis`].
#![allow(non_upper_case_globals)] // the C's mixed-case names (`PDC2xx_STATE`, `ACER_0x4B`)

use core::cell::Cell;

// Registers definitions for SiS SiS5597/98 PCI IDE controller.
// Available from http://www.sis.com.tw/html/databook.html

// IDE timing control registers (32 bits), for all but 96x
/// `SIS_TIM`.
pub const fn sis_tim(channel: i32) -> i32 {
    0x40 + (channel * 4)
}
// for 730, 630 and older (66, 100OLD)
/// `SIS_TIM66_REC_OFF`.
pub const fn sis_tim66_rec_off(drive: i32) -> i32 {
    16 * drive
}
/// `SIS_TIM66_ACT_OFF`.
pub const fn sis_tim66_act_off(drive: i32) -> i32 {
    8 + 16 * drive
}
/// `SIS_TIM66_UDMA_TIME_OFF`.
pub const fn sis_tim66_udma_time_off(drive: i32) -> i32 {
    12 + 16 * drive
}
// for older than 96x (100NEW, 133OLD)
/// `SIS_TIM100_REC_OFF`.
pub const fn sis_tim100_rec_off(drive: i32) -> i32 {
    16 * drive
}
/// `SIS_TIM100_ACT_OFF`.
pub const fn sis_tim100_act_off(drive: i32) -> i32 {
    4 + 16 * drive
}
/// `SIS_TIM100_UDMA_TIME_OFF`.
pub const fn sis_tim100_udma_time_off(drive: i32) -> i32 {
    8 + 16 * drive
}

// From FreeBSD: on 96x, the timing registers may start from 0x40 or 0x70
// depending on the value from register 0x57. 32bits of timing info for
// each drive.
/// `SIS_TIM133`.
pub const fn sis_tim133(reg57: u32, channel: i32, drive: i32) -> i32 {
    (if reg57 & 0x40 != 0 { 0x70 } else { 0x40 }) + (channel << 3) + (drive << 2)
}

// IDE general control register 0 (8 bits)
/// `SIS_CTRL0`.
pub const SIS_CTRL0: i32 = 0x4a;
/// `SIS_CTRL0_PCIBURST`.
pub const SIS_CTRL0_PCIBURST: u32 = 0x80;
/// `SIS_CTRL0_FAST_PW`.
pub const SIS_CTRL0_FAST_PW: u32 = 0x20;
/// `SIS_CTRL0_BO`.
pub const SIS_CTRL0_BO: u32 = 0x08;
/// `SIS_CTRL0_CHAN0_EN`: manual (v2.0) is wrong!!!.
pub const SIS_CTRL0_CHAN0_EN: u32 = 0x02;
/// `SIS_CTRL0_CHAN1_EN`: manual (v2.0) is wrong!!!.
pub const SIS_CTRL0_CHAN1_EN: u32 = 0x04;

// IDE general control register 1 (8 bits)
/// `SIS_CTRL1`.
pub const SIS_CTRL1: u32 = 0x4b;
/// `SIS_CTRL1_POSTW_EN`.
pub const fn sis_ctrl1_postw_en(chan: i32, drv: i32) -> u32 {
    0x10 << (drv + 2 * chan)
}
/// `SIS_CTRL1_PREFETCH_EN`.
pub const fn sis_ctrl1_prefetch_en(chan: i32, drv: i32) -> u32 {
    0x01 << (drv + 2 * chan)
}

// IDE misc control register (8 bit)
/// `SIS_MISC`.
pub const SIS_MISC: i32 = 0x52;
/// `SIS_MISC_TIM_SEL`.
pub const SIS_MISC_TIM_SEL: u32 = 0x08;
/// `SIS_MISC_GTC`.
pub const SIS_MISC_GTC: u32 = 0x04;
/// `SIS_MISC_FIFO_SIZE`.
pub const SIS_MISC_FIFO_SIZE: u32 = 0x01;

// following are from FreeBSD (sorry, no description)
/// `SIS_REG_49`.
pub const SIS_REG_49: i32 = 0x49;
/// `SIS_REG_50`.
pub const SIS_REG_50: i32 = 0x50;
/// `SIS_REG_51`.
pub const SIS_REG_51: u32 = 0x51;
/// `SIS_REG_52`.
pub const SIS_REG_52: i32 = 0x52;
/// `SIS_REG_53`.
pub const SIS_REG_53: u32 = 0x53;
/// `SIS_REG_57`.
pub const SIS_REG_57: i32 = 0x57;

/// `SIS_REG_CBL`.
pub const SIS_REG_CBL: i32 = 0x48;
/// `SIS_REG_CBL_33`.
pub const fn sis_reg_cbl_33(channel: i32) -> u32 {
    0x10 << channel
}
/// `SIS96x_REG_CBL`.
pub const fn sis96x_reg_cbl(channel: i32) -> i32 {
    0x51 + channel * 2
}
/// `SIS96x_REG_CBL_33`.
pub const SIS96x_REG_CBL_33: u32 = 0x80;

/// `SIS_PRODUCT_5518`.
pub const SIS_PRODUCT_5518: u32 = 0x5518;

// Private data
/// `struct pciide_sis`.
pub struct PciideSis {
    /// `sis_type`.
    pub sis_type: Cell<u8>,
}

// timings values, mostly from FreeBSD
// PIO timings, for all up to 133NEW
/// `sis_pio_act`.
pub const SIS_PIO_ACT: [u8; 5] = [12, 6, 4, 3, 3];
/// `sis_pio_rec`.
pub const SIS_PIO_REC: [u8; 5] = [11, 7, 4, 3, 1];
// DMA timings for 66 and 100OLD
/// `sis_udma66_tim`.
pub const SIS_UDMA66_TIM: [u8; 6] = [15, 13, 11, 10, 9, 8];
// DMA timings for 100NEW
/// `sis_udma100new_tim`.
pub const SIS_UDMA100NEW_TIM: [u8; 6] = [0x8b, 0x87, 0x85, 0x84, 0x82, 0x81];
// DMA timings for 133OLD
/// `sis_udma133old_tim`.
pub const SIS_UDMA133OLD_TIM: [u8; 7] = [0x8f, 0x8a, 0x87, 0x85, 0x83, 0x82, 0x81];
// PIO, DMA and UDMA timings for 133NEW
/// `sis_pio133new_tim`.
pub const SIS_PIO133NEW_TIM: [u32; 5] = [0x28269008, 0x0c266008, 0x4263008, 0x0c0a3008, 0x05093008];
/// `sis_dma133new_tim`.
pub const SIS_DMA133NEW_TIM: [u32; 3] = [0x22196008, 0x0c0a3008, 0x05093008];
/// `sis_udma133new_tim`.
pub const SIS_UDMA133NEW_TIM: [u32; 7] = [0x9f4, 0x64a, 0x474, 0x254, 0x234, 0x224, 0x214];
/* </CODE> */

/* <TESTS> */
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    #[ignore = "needs OPENBSD_SRC (just test-ref)"]
    fn defines_match_the_header() {
        let defs = crate::reftest::defines("sys/dev/pci/pciide_sis_reg.h");

        crate::reftest::assert_defines!(defs; SIS_CTRL0, SIS_CTRL0_PCIBURST, SIS_CTRL0_FAST_PW, SIS_CTRL0_BO, SIS_CTRL0_CHAN0_EN, SIS_CTRL0_CHAN1_EN, SIS_CTRL1, SIS_MISC, SIS_MISC_TIM_SEL, SIS_MISC_GTC, SIS_MISC_FIFO_SIZE, SIS_REG_49, SIS_REG_50, SIS_REG_51, SIS_REG_52, SIS_REG_53, SIS_REG_57, SIS_REG_CBL, SIS96x_REG_CBL_33, SIS_PRODUCT_5518);
    }
}
/* </TESTS> */
