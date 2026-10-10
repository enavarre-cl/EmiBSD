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
 * Copyright (c) 2000 Manuel Bouyer.
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
//! HighPoint HPT366/370/372/374 IDE controller registers.
//!
//! Upstream: sys/dev/pci/pciide_hpt_reg.h @ 3ce1f3f79392
//!
//! ## Deviations
//! - The function-like macros are `const fn`s named in lower case. The configuration-space
//!   offsets `pciide.c` reads and writes are `i32` (`pci_conf_read`'s `reg`), bus space
//!   offsets `BusSize`, shift counts `i32`, register values and unused offsets `u32`.
//! - The `static` timing tables are `pub const` arrays named in capitals, of `u8` where the
//!   C has `int8_t` or `u_int8_t` (every entry is written to a byte-wide register or shifted
//!   into a field, never sign-extended).
#![allow(non_upper_case_globals)] // the C's mixed-case names (`PDC2xx_STATE`, `ACER_0x4B`)

// Register definitions for Highpoint PCI IDE controllers.
// The HPT366 has 2 PCI IDE functions, each of them has only one channel.
// The HPT370 and HPT372 has the 2 channels on the same PCI IDE function.

// The HPT366, HPT370 and HPT372 have the same vendor/device ID but not the
// same revision.
/// `HPT366_REV`.
pub const HPT366_REV: u32 = 0x01;
/// `HPT370_REV`.
pub const HPT370_REV: u32 = 0x03;
/// `HPT370A_REV`.
pub const HPT370A_REV: u32 = 0x04;
/// `HPT372_REV`.
pub const HPT372_REV: u32 = 0x05;
/// `HPT374_REV`.
pub const HPT374_REV: u32 = 0x07;

/// `HPT_IDETIM`.
pub const fn hpt_idetim(chan: i32, drive: i32) -> i32 {
    0x40 + (drive * 4) + (chan * 8)
}
/// `HPT_IDETIM_BUFEN`.
pub const HPT_IDETIM_BUFEN: u32 = 0x80000000;
/// `HPT_IDETIM_MSTEN`.
pub const HPT_IDETIM_MSTEN: u32 = 0x40000000;
/// `HPT_IDETIM_DMAEN`.
pub const HPT_IDETIM_DMAEN: u32 = 0x20000000;
/// `HPT_IDETIM_UDMAEN`.
pub const HPT_IDETIM_UDMAEN: u32 = 0x10000000;

/// `HPT366_CTRL1`.
pub const HPT366_CTRL1: u32 = 0x50;
/// `HPT366_CTRL1_BLKDIS`.
pub const fn hpt366_ctrl1_blkdis(chan: i32) -> u32 {
    0x40 << chan
}
/// `HPT366_CTRL1_CHANEN`.
pub const fn hpt366_ctrl1_chanen(chan: i32) -> u32 {
    0x10 << chan
}
/// `HPT366_CTRL1_CLRBUF`.
pub const fn hpt366_ctrl1_clrbuf(chan: i32) -> u32 {
    0x04 << chan
}
/// `HPT366_CTRL1_LEG`.
pub const fn hpt366_ctrl1_leg(chan: i32) -> u32 {
    0x01 << chan
}

/// `HPT366_CTRL2`.
pub const HPT366_CTRL2: u32 = 0x51;
/// `HPT366_CTRL2_FASTIRQ`.
pub const HPT366_CTRL2_FASTIRQ: u32 = 0x80;
/// `HPT366_CTRL2_HOLDIRQ`.
pub const fn hpt366_ctrl2_holdirq(chan: i32) -> u32 {
    0x20 << chan
}
/// `HPT366_CTRL2_SGEN`.
pub const HPT366_CTRL2_SGEN: u32 = 0x10;
/// `HPT366_CTRL2_CLEARFIFO`.
pub const fn hpt366_ctrl2_clearfifo(chan: i32) -> u32 {
    0x04 << chan
}
/// `HPT366_CTRL2_CLEARBMSM`.
pub const HPT366_CTRL2_CLEARBMSM: u32 = 0x02;
/// `HPT366_CTRL2_CLEARSG`.
pub const HPT366_CTRL2_CLEARSG: u32 = 0x01;

/// `HPT366_CTRL3`.
pub const fn hpt366_ctrl3(chan: i32) -> i32 {
    0x52 + chan * 4
}
/// `HPT366_CTRL3_PDMA`.
pub const HPT366_CTRL3_PDMA: u32 = 0x8000;
/// `HPT366_CTRL3_BP`.
pub const HPT366_CTRL3_BP: u32 = 0x4000;
/// `HPT366_CTRL3_FASTIRQ_OFFSET`.
pub const HPT366_CTRL3_FASTIRQ_OFFSET: i32 = 9;
/// `HPT366_CTRL3_FASTIRQ_MASK`.
pub const HPT366_CTRL3_FASTIRQ_MASK: u32 = 0x3;

/// `HPT370_CTRL1`.
pub const fn hpt370_ctrl1(chan: i32) -> i32 {
    0x50 + (chan * 4)
}
/// `HPT370_CTRL1_CLRSG`.
pub const HPT370_CTRL1_CLRSG: u32 = 0x80;
/// `HPT370_CTRL1_READF`.
pub const HPT370_CTRL1_READF: u32 = 0x40;
/// `HPT370_CTRL1_CLRST`.
pub const HPT370_CTRL1_CLRST: u32 = 0x20;
/// `HPT370_CTRL1_CLRSGC`.
pub const HPT370_CTRL1_CLRSGC: u32 = 0x10;
/// `HPT370_CTRL1_BLKDIS`.
pub const HPT370_CTRL1_BLKDIS: u32 = 0x08;
/// `HPT370_CTRL1_EN`.
pub const HPT370_CTRL1_EN: u32 = 0x04;
/// `HPT370_CTRL1_CLRDBUF`.
pub const HPT370_CTRL1_CLRDBUF: u32 = 0x02;
/// `HPT370_CTRL1_LEGEN`.
pub const HPT370_CTRL1_LEGEN: u32 = 0x01;

/// `HPT370_CTRL2`.
pub const fn hpt370_ctrl2(chan: i32) -> i32 {
    0x51 + (chan * 4)
}
/// `HPT370_CTRL2_FASTIRQ`.
pub const HPT370_CTRL2_FASTIRQ: u32 = 0x02;
/// `HPT370_CTRL2_HIRQ`.
pub const HPT370_CTRL2_HIRQ: u32 = 0x01;

/// `HPT370_CTRL3`.
pub const fn hpt370_ctrl3(chan: i32) -> i32 {
    0x52 + chan * 4
}
/// `HPT370_CTRL3_HIZ`.
pub const HPT370_CTRL3_HIZ: u32 = 0x8000;
/// `HPT370_CTRL3_BP`.
pub const HPT370_CTRL3_BP: u32 = 0x4000;
/// `HPT370_CTRL3_FASTIRQ_OFFSET`.
pub const HPT370_CTRL3_FASTIRQ_OFFSET: i32 = 9;
/// `HPT370_CTRL3_FASTIRQ_MASK`.
pub const HPT370_CTRL3_FASTIRQ_MASK: u32 = 0x3;

/// `HPT_STAT1`.
pub const HPT_STAT1: u32 = 0x58;
/// `HPT_STAT1_IRQPOLL`: 366 only.
pub const fn hpt_stat1_irqpoll(chan: i32) -> u32 {
    0x40 << chan
}
/// `HPT_STAT1_DMARQ`.
pub const fn hpt_stat1_dmarq(chan: i32) -> u32 {
    0x04 << (chan * 3)
}
/// `HPT_STAT1_DMACK`.
pub const fn hpt_stat1_dmack(chan: i32) -> u32 {
    0x02 << (chan * 3)
}
/// `HPT_STAT1_IORDY`.
pub const fn hpt_stat1_iordy(chan: i32) -> u32 {
    0x01 << (chan * 3)
}

/// `HPT_STAT2`.
pub const HPT_STAT2: u32 = 0x59;
/// `HPT_STAT2_FLT_RST`: 366 only.
pub const HPT_STAT2_FLT_RST: u32 = 0x40;
/// `HPT_STAT2_RST`: 370 only.
pub const fn hpt_stat2_rst(chan: i32) -> u32 {
    0x40 << chan
}
/// `HPT_STAT2_POLLEN`.
pub const fn hpt_stat2_pollen(chan: i32) -> u32 {
    0x04 << (chan * 3)
}
/// `HPT_STAT2_IRQD1`.
pub const fn hpt_stat2_irqd1(chan: i32) -> u32 {
    0x02 << (chan * 3)
}
/// `HPT_STAT2_IRQD0_CH1`.
pub const HPT_STAT2_IRQD0_CH1: u32 = 0x08;
/// `HPT_STAT2_POLLST`.
pub const HPT_STAT2_POLLST: u32 = 0x01;

/// `HPT_CSEL`.
pub const HPT_CSEL: i32 = 0x5a;
/// `HPT_CSEL_IRQDIS`: 370 only.
pub const HPT_CSEL_IRQDIS: u32 = 0x10;
/// `HPT_CSEL_PCIDIS`: 370 only.
pub const HPT_CSEL_PCIDIS: u32 = 0x08;
/// `HPT_CSEL_PCIWR`: 370 only.
pub const HPT_CSEL_PCIWR: u32 = 0x04;
/// `HPT_CSEL_CBLID`.
pub const fn hpt_csel_cblid(chan: i32) -> u32 {
    0x01 << (1 - chan)
}

/// `HPT_SC2`.
pub const HPT_SC2: i32 = 0x5b;
/// `HPT_SC2_OSC_OK`.
pub const HPT_SC2_OSC_OK: u32 = 0x80;
/// `HPT_SC2_OSC_EN`.
pub const HPT_SC2_OSC_EN: u32 = 0x20;
/// `HPT_SC2_ECLK`.
pub const HPT_SC2_ECLK: u32 = 0x10;
/// `HPT_SC2_BPIO`.
pub const HPT_SC2_BPIO: u32 = 0x08;
/// `HPT_SC2_DMARQW`.
pub const HPT_SC2_DMARQW: u32 = 0x04;
/// `HPT_SC2_SCLK`.
pub const HPT_SC2_SCLK: u32 = 0x02;
/// `HPT_SC2_MAEN`.
pub const HPT_SC2_MAEN: u32 = 0x01;

/// `hpt366_pio`.
pub const HPT366_PIO: [u32; 4] = [0x00d0a7aa, 0x00c8a753, 0x00c8a742, 0x00c8a731];
/// `hpt366_dma`.
pub const HPT366_DMA: [u32; 3] = [0x20c8a797, 0x20c8a742, 0x20c8a731];
/// `hpt366_udma`.
pub const HPT366_UDMA: [u32; 5] = [0x10c8a731, 0x10cba731, 0x10caa731, 0x10cfa731, 0x10c9a731];

/// `hpt370_pio`.
pub const HPT370_PIO: [u32; 5] = [0x06914e8a, 0x06914e65, 0x06514e33, 0x06514e22, 0x06514e21];
/// `hpt370_dma`.
pub const HPT370_DMA: [u32; 3] = [0x26514e97, 0x26514e33, 0x26514e21];
/// `hpt370_udma`.
pub const HPT370_UDMA: [u32; 6] = [
    0x16514e31, 0x164d4e31, 0x16494e31, 0x166d4e31, 0x16454e31, 0x16454e31,
];

/// `hpt372_pio`.
pub const HPT372_PIO: [u32; 5] = [0x0d029d5e, 0x0d029d26, 0x0c829ca6, 0x0c829c84, 0x0c829c62];
/// `hpt372_dma`.
pub const HPT372_DMA: [u32; 3] = [0x2c82922e, 0x2c829266, 0x2c829262];
/// `hpt372_udma`.
pub const HPT372_UDMA: [u32; 7] = [
    0x1c82dc62, 0x1c9adc62, 0x1c91dc62, 0x1c8edc62, 0x1c8ddc62, 0x1c6ddc62, 0x1c81dc62,
];

/// `hpt374_pio`.
pub const HPT374_PIO: [u32; 5] = [0x0ac1f48a, 0x0ac1f465, 0x0a81f454, 0x0a81f443, 0x0a81f442];
/// `hpt374_dma`.
pub const HPT374_DMA: [u32; 3] = [0x228082ea, 0x22808254, 0x22808242];
/// `hpt374_udma`.
pub const HPT374_UDMA: [u32; 7] = [
    0x121882ea, 0x12148254, 0x120c8242, 0x128c8242, 0x12ac8242, 0x12848242, 0x12808242,
];
/* </CODE> */

/* <TESTS> */
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    #[ignore = "needs OPENBSD_SRC (just test-ref)"]
    fn defines_match_the_header() {
        let defs = crate::reftest::defines("sys/dev/pci/pciide_hpt_reg.h");

        crate::reftest::assert_defines!(defs; HPT366_REV, HPT370_REV, HPT370A_REV, HPT372_REV, HPT374_REV, HPT_IDETIM_BUFEN, HPT_IDETIM_MSTEN, HPT_IDETIM_DMAEN, HPT_IDETIM_UDMAEN, HPT366_CTRL1, HPT366_CTRL2, HPT366_CTRL2_FASTIRQ, HPT366_CTRL2_SGEN, HPT366_CTRL2_CLEARBMSM, HPT366_CTRL2_CLEARSG, HPT366_CTRL3_PDMA, HPT366_CTRL3_BP, HPT366_CTRL3_FASTIRQ_OFFSET, HPT366_CTRL3_FASTIRQ_MASK, HPT370_CTRL1_CLRSG, HPT370_CTRL1_READF, HPT370_CTRL1_CLRST, HPT370_CTRL1_CLRSGC, HPT370_CTRL1_BLKDIS, HPT370_CTRL1_EN, HPT370_CTRL1_CLRDBUF, HPT370_CTRL1_LEGEN, HPT370_CTRL2_FASTIRQ, HPT370_CTRL2_HIRQ, HPT370_CTRL3_HIZ, HPT370_CTRL3_BP, HPT370_CTRL3_FASTIRQ_OFFSET, HPT370_CTRL3_FASTIRQ_MASK, HPT_STAT1, HPT_STAT2, HPT_STAT2_FLT_RST, HPT_STAT2_IRQD0_CH1, HPT_STAT2_POLLST, HPT_CSEL, HPT_CSEL_IRQDIS, HPT_CSEL_PCIDIS, HPT_CSEL_PCIWR, HPT_SC2, HPT_SC2_OSC_OK, HPT_SC2_OSC_EN, HPT_SC2_ECLK, HPT_SC2_BPIO, HPT_SC2_DMARQW, HPT_SC2_SCLK, HPT_SC2_MAEN);
    }
}
/* </TESTS> */
