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
 * Copyright (c) 2001 Jason L. Wright (jason@thought.net)
 * Copyright (c) 2004 Alexander Yurchenko <grange@openbsd.org>
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
 * IMPLIED WARRANTIES, INCLUDING, BUT NOT LIMITED TO, THE IMPLIED
 * WARRANTIES OF MERCHANTABILITY AND FITNESS FOR A PARTICULAR PURPOSE ARE
 * DISCLAIMED.  IN NO EVENT SHALL THE AUTHOR BE LIABLE FOR ANY DIRECT,
 * INDIRECT, INCIDENTAL, SPECIAL, EXEMPLARY, OR CONSEQUENTIAL DAMAGES
 * (INCLUDING, BUT NOT LIMITED TO, PROCUREMENT OF SUBSTITUTE GOODS OR
 * SERVICES; LOSS OF USE, DATA, OR PROFITS; OR BUSINESS INTERRUPTION)
 * HOWEVER CAUSED AND ON ANY THEORY OF LIABILITY, WHETHER IN CONTRACT,
 * STRICT LIABILITY, OR TORT (INCLUDING NEGLIGENCE OR OTHERWISE) ARISING IN
 * ANY WAY OUT OF THE USE OF THIS SOFTWARE, EVEN IF ADVISED OF THE
 * POSSIBILITY OF SUCH DAMAGE.
 */
/* </LICENSES> */

/* <CODE> */
//! National Semiconductor PC87415 and SCx200 IDE controller registers.
//!
//! Upstream: sys/dev/pci/pciide_natsemi_reg.h @ 3ce1f3f79392
//!
//! ## Deviations
//! - The function-like macros are `const fn`s named in lower case. The configuration-space
//!   offsets `pciide.c` reads and writes are `i32` (`pci_conf_read`'s `reg`), bus space
//!   offsets `BusSize`, shift counts `i32`, register values and unused offsets `u32`.
//! - The `static` timing tables are `pub const` arrays named in capitals, of `u8` where the
//!   C has `int8_t` or `u_int8_t` (every entry is written to a byte-wide register or shifted
//!   into a field, never sign-extended).
#![allow(non_upper_case_globals)] // the C's mixed-case names (`PDC2xx_STATE`, `ACER_0x4B`)

// Register definitions for National Semiconductor PC87415.  Definitions
// based on "PC87415: PCI-IDE DMA Master Mode Interface Controller"
// (March 1996) datasheet from their website.

/// `NATSEMI_CTRL1`: Control register1.
pub const NATSEMI_CTRL1: i32 = 0x40;
/// `NATSEMI_CTRL1_SWRST`: sw rst to ch1/ch2 on.
pub const NATSEMI_CTRL1_SWRST: u32 = 0x04;
/// `NATSEMI_CTRL1_IDEPWR`.
pub const NATSEMI_CTRL1_IDEPWR: u32 = 0x08;
/// `NATSEMI_CTRL1_CH1INTMAP`.
pub const NATSEMI_CTRL1_CH1INTMAP: u32 = 0x10;
/// `NATSEMI_CTRL1_CH2INTMAP`.
pub const NATSEMI_CTRL1_CH2INTMAP: u32 = 0x20;
/// `NATSEMI_CTRL1_INTAMASK`.
pub const NATSEMI_CTRL1_INTAMASK: u32 = 0x40;
/// `NATSEMI_CTRL1_IDWR`: write to did/vid enable.
pub const NATSEMI_CTRL1_IDWR: u32 = 0x80;

/// `NATSEMI_CTRL2`: Control register2.
pub const NATSEMI_CTRL2: i32 = 0x41;
/// `NATSEMI_CTRL2_CH1MASK`: channel 1 intr masked.
pub const NATSEMI_CTRL2_CH1MASK: u32 = 0x01;
/// `NATSEMI_CTRL2_CH2MASK`: channel 2 intr masked.
pub const NATSEMI_CTRL2_CH2MASK: u32 = 0x02;
/// `NATSEMI_CTRL2_BARDIS`: PCI BAR 2/3 disable.
pub const NATSEMI_CTRL2_BARDIS: u32 = 0x04;
/// `NATSEMI_CTRL2_WATCHDOG`: enable watchdog timer.
pub const NATSEMI_CTRL2_WATCHDOG: u32 = 0x08;
/// `NATSEMI_CTRL2_BUF1BYP`: bypass buffer 1.
pub const NATSEMI_CTRL2_BUF1BYP: u32 = 0x10;
/// `NATSEMI_CTRL2_BUF2BYP`: bypass buffer 2.
pub const NATSEMI_CTRL2_BUF2BYP: u32 = 0x20;
/// `NATSEMI_CTRL2_IDE1MAP`: IDE at bar 1.
pub const NATSEMI_CTRL2_IDE1MAP: u32 = 0x40;
/// `NATSEMI_CTRL2_IDE2MAP`: IDE at bar 2.
pub const NATSEMI_CTRL2_IDE2MAP: u32 = 0x80;

/// `NATSEMI_CHMASK`.
pub const fn natsemi_chmask(chn: i32) -> u32 {
    NATSEMI_CTRL2_CH1MASK << chn
}

/// `NATSEMI_CTRL3`: Control register3.
pub const NATSEMI_CTRL3: u32 = 0x42;
/// `NATSEMI_CTRL3_CH1PREDIS`: channel 1 prefetch disable.
pub const NATSEMI_CTRL3_CH1PREDIS: u32 = 0x01;
/// `NATSEMI_CTRL3_CH2PREDIS`: channel 2 prefetch disable.
pub const NATSEMI_CTRL3_CH2PREDIS: u32 = 0x02;
/// `NATSEMI_CTRL3_RSTIDLE`: reset idle state.
pub const NATSEMI_CTRL3_RSTIDLE: u32 = 0x04;
/// `NATSEMI_CTRL3_C1D1DMARQ`: c1d1 dmarq handshaking.
pub const NATSEMI_CTRL3_C1D1DMARQ: u32 = 0x10;
/// `NATSEMI_CTRL3_C1D2DMARQ`: c1d2 dmarq handshaking.
pub const NATSEMI_CTRL3_C1D2DMARQ: u32 = 0x20;
/// `NATSEMI_CTRL3_C2D1DMARQ`: c2d1 dmarq handshaking.
pub const NATSEMI_CTRL3_C2D1DMARQ: u32 = 0x40;
/// `NATSEMI_CTRL3_C2D2DMARQ`: c2d2 dmarq handshaking.
pub const NATSEMI_CTRL3_C2D2DMARQ: u32 = 0x80;

/// `NATSEMI_WBS`: Write buffer status.
pub const NATSEMI_WBS: u32 = 0x43;
/// `NATSEMI_WBS_WB1NMPTY`: chan 1 write buf not empty.
pub const NATSEMI_WBS_WB1NMPTY: u32 = 0x01;
/// `NATSEMI_WBS_WB2NMPTY`: chan 2 write buf not empty.
pub const NATSEMI_WBS_WB2NMPTY: u32 = 0x02;

/// `NATSEMI_C1D1DRT`: Channel 1/device 1 data read timing.
pub const NATSEMI_C1D1DRT: u32 = 0x44;
/// `NATSEMI_C1D1DWT`: Channel 1/device 1 data write timing.
pub const NATSEMI_C1D1DWT: u32 = 0x45;
/// `NATSEMI_C1D2DRT`: Channel 1/device 2 data read timing.
pub const NATSEMI_C1D2DRT: u32 = 0x48;
/// `NATSEMI_C1D2DWT`: Channel 1/device 2 data write timing.
pub const NATSEMI_C1D2DWT: u32 = 0x49;
/// `NATSEMI_C2D1DRT`: Channel 2/device 1 data read timing.
pub const NATSEMI_C2D1DRT: u32 = 0x4c;
/// `NATSEMI_C2D1DWT`: Channel 2/device 1 data write timing.
pub const NATSEMI_C2D1DWT: u32 = 0x4d;
/// `NATSEMI_C2D2DRT`: Channel 2/device 2 data read timing.
pub const NATSEMI_C2D2DRT: u32 = 0x50;
/// `NATSEMI_C2D2DWT`: Channel 2/device 2 data write timing.
pub const NATSEMI_C2D2DWT: u32 = 0x51;

/// `NATSEMI_CCBT`: Command and control block timing.
pub const NATSEMI_CCBT: i32 = 0x54;

/// `NATSEMI_SECT`: Sector size.
pub const NATSEMI_SECT: u32 = 0x55;
/// `NATSEMI_SECT_C1UNUSED`: not used.
pub const NATSEMI_SECT_C1UNUSED: u32 = 0x0f;
/// `NATSEMI_SECT_C1_512`: 512 bytes.
pub const NATSEMI_SECT_C1_512: u32 = 0x0e;
/// `NATSEMI_SECT_C1_1024`: 1024 bytes.
pub const NATSEMI_SECT_C1_1024: u32 = 0x0c;
/// `NATSEMI_SECT_C1_2048`: 2048 bytes.
pub const NATSEMI_SECT_C1_2048: u32 = 0x08;
/// `NATSEMI_SECT_C1_4096`: 4096 bytes.
pub const NATSEMI_SECT_C1_4096: u32 = 0x00;
/// `NATSEMI_SECT_C2UNUSED`: not used.
pub const NATSEMI_SECT_C2UNUSED: u32 = 0xf0;
/// `NATSEMI_SECT_C2_512`: 512 bytes.
pub const NATSEMI_SECT_C2_512: u32 = 0xe0;
/// `NATSEMI_SECT_C2_1024`: 1024 bytes.
pub const NATSEMI_SECT_C2_1024: u32 = 0xc0;
/// `NATSEMI_SECT_C2_2048`: 2048 bytes.
pub const NATSEMI_SECT_C2_2048: u32 = 0x80;
/// `NATSEMI_SECT_C2_4096`: 4096 bytes.
pub const NATSEMI_SECT_C2_4096: u32 = 0x00;

/// `NATSEMI_RTREG`.
pub const fn natsemi_rtreg(c: i32, d: i32) -> i32 {
    0x44 + (c * 8) + (d * 4)
}
/// `NATSEMI_WTREG`.
pub const fn natsemi_wtreg(c: i32, d: i32) -> i32 {
    0x44 + (c * 8) + (d * 4) + 1
}

// 17 - N = number of clocks
/// `natsemi_pio_pulse`.
pub const NATSEMI_PIO_PULSE: [u8; 5] = [7, 12, 13, 14, 14];
/// `natsemi_dma_pulse`.
pub const NATSEMI_DMA_PULSE: [u8; 3] = [7, 10, 10];
// 16 - N = number of clocks
/// `natsemi_pio_recover`.
pub const NATSEMI_PIO_RECOVER: [u8; 5] = [6, 8, 11, 13, 15];
/// `natsemi_dma_recover`.
pub const NATSEMI_DMA_RECOVER: [u8; 3] = [6, 8, 9];

// Register definitions for National Semiconductor SCx200 IDE found
// on Geode SC1100 IAOC.
/// `SCx200_TIM_PIO`.
pub const fn scx200_tim_pio(chan: i32, drive: i32) -> i32 {
    0x40 + 16 * chan + 8 * drive
}
/// `SCx200_TIM_DMA`.
pub const fn scx200_tim_dma(chan: i32, drive: i32) -> i32 {
    0x44 + 16 * chan + 8 * drive
}

/// `SCx200_PIOFORMAT_SHIFT`.
pub const SCx200_PIOFORMAT_SHIFT: i32 = 31;

// PIO mode timings
/// `scx200_pio33`.
pub const SCX200_PIO33: [[u32; 5]; 2] = [
    [0x00009172, 0x00012171, 0x00020080, 0x00032010, 0x00040010],
    [0x9172d132, 0x21717121, 0x00803020, 0x20102010, 0x00100010],
];
/// `scx200_pio66`.
pub const SCX200_PIO66: [[u32; 5]; 2] = [
    [0x0000f8e4, 0x000153f3, 0x000213f1, 0x00034231, 0x00041131],
    [0xf8e4f8e4, 0x53f3f353, 0x13f18141, 0x42314231, 0x11311131],
];

// DMA mode timings
/// `scx200_dma33`.
pub const SCX200_DMA33: [u32; 3] = [0x00077771, 0x00012121, 0x00002020];
/// `scx200_dma66`.
pub const SCX200_DMA66: [u32; 3] = [0x000ffff3, 0x00035352, 0x00015151];

// UDMA mode timings
/// `scx200_udma33`.
pub const SCX200_UDMA33: [u32; 3] = [0x00921250, 0x00911140, 0x00911030];
/// `scx200_udma66`.
pub const SCX200_UDMA66: [u32; 3] = [0x009436a1, 0x00933481, 0x00923261];
/* </CODE> */

/* <TESTS> */
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    #[ignore = "needs OPENBSD_SRC (just test-ref)"]
    fn defines_match_the_header() {
        let defs = crate::reftest::defines("sys/dev/pci/pciide_natsemi_reg.h");

        crate::reftest::assert_defines!(defs; NATSEMI_CTRL1, NATSEMI_CTRL1_SWRST, NATSEMI_CTRL1_IDEPWR, NATSEMI_CTRL1_CH1INTMAP, NATSEMI_CTRL1_CH2INTMAP, NATSEMI_CTRL1_INTAMASK, NATSEMI_CTRL1_IDWR, NATSEMI_CTRL2, NATSEMI_CTRL2_CH1MASK, NATSEMI_CTRL2_CH2MASK, NATSEMI_CTRL2_BARDIS, NATSEMI_CTRL2_WATCHDOG, NATSEMI_CTRL2_BUF1BYP, NATSEMI_CTRL2_BUF2BYP, NATSEMI_CTRL2_IDE1MAP, NATSEMI_CTRL2_IDE2MAP, NATSEMI_CTRL3, NATSEMI_CTRL3_CH1PREDIS, NATSEMI_CTRL3_CH2PREDIS, NATSEMI_CTRL3_RSTIDLE, NATSEMI_CTRL3_C1D1DMARQ, NATSEMI_CTRL3_C1D2DMARQ, NATSEMI_CTRL3_C2D1DMARQ, NATSEMI_CTRL3_C2D2DMARQ, NATSEMI_WBS, NATSEMI_WBS_WB1NMPTY, NATSEMI_WBS_WB2NMPTY, NATSEMI_C1D1DRT, NATSEMI_C1D1DWT, NATSEMI_C1D2DRT, NATSEMI_C1D2DWT, NATSEMI_C2D1DRT, NATSEMI_C2D1DWT, NATSEMI_C2D2DRT, NATSEMI_C2D2DWT, NATSEMI_CCBT, NATSEMI_SECT, NATSEMI_SECT_C1UNUSED, NATSEMI_SECT_C1_512, NATSEMI_SECT_C1_1024, NATSEMI_SECT_C1_2048, NATSEMI_SECT_C1_4096, NATSEMI_SECT_C2UNUSED, NATSEMI_SECT_C2_512, NATSEMI_SECT_C2_1024, NATSEMI_SECT_C2_2048, NATSEMI_SECT_C2_4096, SCx200_PIOFORMAT_SHIFT);
    }
}
/* </TESTS> */
