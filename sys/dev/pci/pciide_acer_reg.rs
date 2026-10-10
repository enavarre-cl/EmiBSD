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
 * Copyright (c) 1999 Manuel Bouyer.
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
//! Acer Labs M5229 IDE controller registers.
//!
//! Upstream: sys/dev/pci/pciide_acer_reg.h @ 3ce1f3f79392
//!
//! ## Deviations
//! - The function-like macros are `const fn`s named in lower case. The configuration-space
//!   offsets `pciide.c` reads and writes are `i32` (`pci_conf_read`'s `reg`), bus space
//!   offsets `BusSize`, shift counts `i32`, register values and unused offsets `u32`.
//! - The `static` timing tables are `pub const` arrays named in capitals, of `u8` where the
//!   C has `int8_t` or `u_int8_t` (every entry is written to a byte-wide register or shifted
//!   into a field, never sign-extended).
//! - `acer_dma` (under `#ifdef unused`, never defined) is not compiled, as in the C.
#![allow(non_upper_case_globals)] // the C's mixed-case names (`PDC2xx_STATE`, `ACER_0x4B`)

//  class code attribute register 1 (1 byte)
/// `ACER_CCAR1`.
pub const ACER_CCAR1: i32 = 0x43;
/// `ACER_CHANSTATUS_RO`.
pub const ACER_CHANSTATUS_RO: u32 = 0x40;
/// `PCIIDE_CHAN_RO`.
pub const fn pciide_chan_ro(chan: i32) -> u32 {
    0x20 >> chan
}

// from Linux, 80 pins cable detect
/// `ACER_0x4A`.
pub const ACER_0x4A: i32 = 0x4a;
// bit 0 is 0 -> primary has 80 pin cable
// bit 1 is 0 -> secondary has 80 pin cable
/// `ACER_0x4A_80PIN`.
pub const fn acer_0x4a_80pin(chan: i32) -> u32 {
    0x1 << chan
}

// From FreeBSD, for UDMA mode > 2
/// `ACER_0x4B`.
pub const ACER_0x4B: i32 = 0x4b;
/// `ACER_0x4B_UDMA66`.
pub const ACER_0x4B_UDMA66: u32 = 0x01;
// From Linux
/// `ACER_0x4B_CDETECT`.
pub const ACER_0x4B_CDETECT: u32 = 0x08;

// class code attribute register 2 (1 byte)
/// `ACER_CCAR2`.
pub const ACER_CCAR2: i32 = 0x4d;
/// `ACER_CHANSTATUSREGS_RO`.
pub const ACER_CHANSTATUSREGS_RO: u32 = 0x80;

// class code attribute register 3 (1 byte)
/// `ACER_CCAR3`.
pub const ACER_CCAR3: i32 = 0x50;
/// `ACER_CCAR3_PI`.
pub const ACER_CCAR3_PI: u32 = 0x02;

// flexible channel setting register
/// `ACER_FCS`.
pub const ACER_FCS: u32 = 0x52;
/// `ACER_FCS_TIMREG`.
pub const fn acer_fcs_timreg(chan: i32, drv: i32) -> u32 {
    0x8 >> (drv + chan * 2)
}

// CD-ROM control register
/// `ACER_CDRC`.
pub const ACER_CDRC: i32 = 0x53;
/// `ACER_CDRC_FIFO_DISABLE`.
pub const ACER_CDRC_FIFO_DISABLE: u32 = 0x02;
/// `ACER_CDRC_DMA_EN`.
pub const ACER_CDRC_DMA_EN: u32 = 0x01;

// Fifo threshold and Ultra-DMA settings (4 bytes).
/// `ACER_FTH_UDMA`.
pub const ACER_FTH_UDMA: i32 = 0x54;
/// `ACER_FTH_VAL`.
pub const fn acer_fth_val(chan: i32, drv: i32, val: u32) -> u32 {
    (val & 0x3) << (drv * 4 + chan * 8)
}
/// `ACER_FTH_OPL`.
pub const fn acer_fth_opl(chan: i32, drv: i32, val: u32) -> u32 {
    (val & 0x3) << (2 + drv * 4 + chan * 8)
}
/// `ACER_UDMA_EN`.
pub const fn acer_udma_en(chan: i32, drv: i32) -> u32 {
    0x8 << (16 + drv * 4 + chan * 8)
}
/// `ACER_UDMA_TIM`.
pub const fn acer_udma_tim(chan: i32, drv: i32, val: u32) -> u32 {
    (val & 0x7) << (16 + drv * 4 + chan * 8)
}

// drives timings setup (1 byte)
/// `ACER_IDETIM`.
pub const fn acer_idetim(chan: i32, drv: i32) -> i32 {
    0x5a + drv + chan * 4
}

// IRQ and drive select status
/// `ACER_CHIDS`.
pub const ACER_CHIDS: i32 = 0x75;
/// `ACER_CHIDS_DRV`.
pub const fn acer_chids_drv(channel: i32) -> u32 {
    0x4 << channel
}
/// `ACER_CHIDS_INT`.
pub const fn acer_chids_int(channel: i32) -> u32 {
    0x1 << channel
}

// Linux: south-bridge's enable bit m1533
/// `ACER_0x79`.
pub const ACER_0x79: u32 = 0x79;
/// `ACER_0x79_REVC2_EN`.
pub const ACER_0x79_REVC2_EN: u32 = 0x4;
/// `ACER_0x79_EN`.
pub const ACER_0x79_EN: u32 = 0x2;

// IDE bus frequency (1 byte)
// This should be setup by the BIOS - can we rely on this ?
/// `ACER_IDE_CLK`.
pub const ACER_IDE_CLK: u32 = 0x78;

// acer UDMA3/4/5 from FreeBSD
/// `acer_udma`.
pub const ACER_UDMA: [u8; 6] = [0x4, 0x3, 0x2, 0x1, 0x0, 0x7];
/// `acer_pio`.
pub const ACER_PIO: [u8; 5] = [0x0c, 0x58, 0x44, 0x33, 0x31];
// #ifdef unused: static int8_t acer_dma[] = {0x08, 0x33, 0x31};
/* </CODE> */

/* <TESTS> */
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    #[ignore = "needs OPENBSD_SRC (just test-ref)"]
    fn defines_match_the_header() {
        let defs = crate::reftest::defines("sys/dev/pci/pciide_acer_reg.h");

        crate::reftest::assert_defines!(defs; ACER_CCAR1, ACER_CHANSTATUS_RO, ACER_0x4A, ACER_0x4B, ACER_0x4B_UDMA66, ACER_0x4B_CDETECT, ACER_CCAR2, ACER_CHANSTATUSREGS_RO, ACER_CCAR3, ACER_CCAR3_PI, ACER_FCS, ACER_CDRC, ACER_CDRC_FIFO_DISABLE, ACER_CDRC_DMA_EN, ACER_FTH_UDMA, ACER_CHIDS, ACER_0x79, ACER_0x79_REVC2_EN, ACER_0x79_EN, ACER_IDE_CLK);
    }
}
/* </TESTS> */
