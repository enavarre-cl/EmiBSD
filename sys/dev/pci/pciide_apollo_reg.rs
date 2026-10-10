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
//! VIA Apollo IDE controller registers (VT82C586A/B, VT82C596, VT82C686, VT8231, ...).
//!
//! Upstream: sys/dev/pci/pciide_apollo_reg.h @ 3ce1f3f79392
//!
//! ## Deviations
//! - The function-like macros are `const fn`s named in lower case. The configuration-space
//!   offsets `pciide.c` reads and writes are `i32` (`pci_conf_read`'s `reg`), bus space
//!   offsets `BusSize`, shift counts `i32`, register values and unused offsets `u32`.
//! - The `static` timing tables are `pub const` arrays named in capitals, of `u8` where the
//!   C has `int8_t` or `u_int8_t` (every entry is written to a byte-wide register or shifted
//!   into a field, never sign-extended).
//! - `APO_IDECONF_FIFO_TRSH(channel, x)` keeps the C's `<< 1 + 24` precedence (a shift by 25 << (1 - channel)); nothing uses it.
#![allow(non_upper_case_globals)] // the C's mixed-case names (`PDC2xx_STATE`, `ACER_0x4B`)

// Registers definitions for VIA technologies's Apollo controllers (VT82V580VO,
// VT82C586A and VT82C586B).
// UDMA1/2/3/4 capable
// http://www.via.com.tw/pdf/productinfo/686a.pdf
// http://www.via.com.tw/pdf/productinfo/596b.pdf
// UDMA1/2 capable
// http://www.via.com.tw/pdf/productinfo/586b.pdf
// http://www.via.com.tw/pdf/productinfo/586a.pdf

// misc. configuration registers
/// `APO_IDECONF`.
pub const APO_IDECONF: i32 = 0x40;
/// `APO_IDECONF_EN`.
pub const fn apo_ideconf_en(channel: i32) -> u32 {
    0x00000001 << (1 - channel)
}
/// `APO_IDECONF_SERR_EN`: 580 only.
pub const APO_IDECONF_SERR_EN: u32 = 0x00000100;
/// `APO_IDECONF_DS_SOURCE`: 580 only.
pub const APO_IDECONF_DS_SOURCE: u32 = 0x00000200;
/// `APO_IDECONF_ALT_INTR_EN`: 580 only.
pub const APO_IDECONF_ALT_INTR_EN: u32 = 0x00000400;
/// `APO_IDECONF_PERR_EN`: 580 only.
pub const APO_IDECONF_PERR_EN: u32 = 0x00000800;
/// `APO_IDECONF_WR_BUFF_EN`.
pub const fn apo_ideconf_wr_buff_en(channel: i32) -> u32 {
    0x00001000 << ((1 - channel) << 1)
}
/// `APO_IDECONF_RD_PREF_EN`.
pub const fn apo_ideconf_rd_pref_en(channel: i32) -> u32 {
    0x00002000 << ((1 - channel) << 1)
}
/// `APO_IDECONF_DEVSEL_TME`: 580 only.
pub const APO_IDECONF_DEVSEL_TME: u32 = 0x00010000;
/// `APO_IDECONF_MAS_CMD_MON`: 580 only.
pub const APO_IDECONF_MAS_CMD_MON: u32 = 0x00020000;
/// `APO_IDECONF_IO_NAT`: 580 only.
pub const fn apo_ideconf_io_nat(channel: i32) -> u32 {
    0x00400000 << (1 - channel)
}
/// `APO_IDECONF_FIFO_TRSH`.
pub const fn apo_ideconf_fifo_trsh(channel: i32, x: u32) -> u32 {
    (x & 0x3) << ((1 - channel) << (1 + 24))
}
/// `APO_IDECONF_FIFO_CONF_MASK`.
pub const APO_IDECONF_FIFO_CONF_MASK: u32 = 0x60000000;

// Misc. controls register
/// `APO_CTLMISC`.
pub const APO_CTLMISC: i32 = 0x44;
/// `APO_CTLMISC_BM_STS_RTY`.
pub const APO_CTLMISC_BM_STS_RTY: u32 = 0x00000008;
/// `APO_CTLMISC_FIFO_HWS`.
pub const APO_CTLMISC_FIFO_HWS: u32 = 0x00000010;
/// `APO_CTLMISC_WR_IRDY_WS`.
pub const APO_CTLMISC_WR_IRDY_WS: u32 = 0x00000020;
/// `APO_CTLMISC_RD_IRDY_WS`.
pub const APO_CTLMISC_RD_IRDY_WS: u32 = 0x00000040;
/// `APO_CTLMISC_INTR_SWP`.
pub const APO_CTLMISC_INTR_SWP: u32 = 0x00004000;
/// `APO_CTLMISC_DRDY_TIME_MASK`.
pub const APO_CTLMISC_DRDY_TIME_MASK: u32 = 0x00030000;
/// `APO_CTLMISC_FIFO_FLSH_RD`.
pub const fn apo_ctlmisc_fifo_flsh_rd(channel: i32) -> u32 {
    0x00100000 << (1 - channel)
}
/// `APO_CTLMISC_FIFO_FLSH_DMA`.
pub const fn apo_ctlmisc_fifo_flsh_dma(channel: i32) -> u32 {
    0x00400000 << (1 - channel)
}

// data port timings controls
/// `APO_DATATIM`.
pub const APO_DATATIM: i32 = 0x48;
/// `APO_DATATIM_MASK`.
pub const fn apo_datatim_mask(channel: i32) -> u32 {
    0xffff << ((1 - channel) << 4)
}
/// `APO_DATATIM_RECOV`.
pub const fn apo_datatim_recov(channel: i32, drive: i32, x: u32) -> u32 {
    (x & 0xf) << (((1 - channel) << 4) + ((1 - drive) << 3))
}
/// `APO_DATATIM_PULSE`.
pub const fn apo_datatim_pulse(channel: i32, drive: i32, x: u32) -> u32 {
    (x & 0xf) << (((1 - channel) << 4) + ((1 - drive) << 3) + 4)
}

// misc timings control
/// `APO_MISCTIM`.
pub const APO_MISCTIM: u32 = 0x4c;

// UltraDMA control (586A/B and higher only)
/// `APO_UDMA`.
pub const APO_UDMA: i32 = 0x50;
/// `APO_UDMA_MASK`.
pub const fn apo_udma_mask(channel: i32) -> u32 {
    0xffff << ((1 - channel) << 4)
}
/// `APO_UDMA_TIME`.
pub const fn apo_udma_time(channel: i32, drive: i32, x: u32) -> u32 {
    (x & 0xf) << (((1 - channel) << 4) + ((1 - drive) << 3))
}
/// `APO_UDMA_PIO_MODE`.
pub const fn apo_udma_pio_mode(channel: i32, drive: i32) -> u32 {
    0x20 << (((1 - channel) << 4) + ((1 - drive) << 3))
}
/// `APO_UDMA_EN`.
pub const fn apo_udma_en(channel: i32, drive: i32) -> u32 {
    0x40 << (((1 - channel) << 4) + ((1 - drive) << 3))
}
/// `APO_UDMA_EN_MTH`.
pub const fn apo_udma_en_mth(channel: i32, drive: i32) -> u32 {
    0x80 << (((1 - channel) << 4) + ((1 - drive) << 3))
}
/// `APO_UDMA_CLK66`.
pub const fn apo_udma_clk66(channel: i32) -> u32 {
    0x08 << ((1 - channel) << 4)
}

/// `apollo_udma133_tim`.
pub const APOLLO_UDMA133_TIM: [u8; 7] = [0x07, 0x07, 0x06, 0x04, 0x02, 0x01, 0x00];
/// `apollo_udma100_tim`.
pub const APOLLO_UDMA100_TIM: [u8; 6] = [0x07, 0x07, 0x04, 0x02, 0x01, 0x00];
/// `apollo_udma66_tim`.
pub const APOLLO_UDMA66_TIM: [u8; 5] = [0x03, 0x03, 0x02, 0x01, 0x00];
/// `apollo_udma33_tim`.
pub const APOLLO_UDMA33_TIM: [u8; 3] = [0x03, 0x02, 0x00];
/// `apollo_pio_set`.
pub const APOLLO_PIO_SET: [u8; 5] = [0x0a, 0x0a, 0x0a, 0x02, 0x02];
/// `apollo_pio_rec`.
pub const APOLLO_PIO_REC: [u8; 5] = [0x08, 0x08, 0x08, 0x02, 0x00];
/* </CODE> */

/* <TESTS> */
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    #[ignore = "needs OPENBSD_SRC (just test-ref)"]
    fn defines_match_the_header() {
        let defs = crate::reftest::defines("sys/dev/pci/pciide_apollo_reg.h");

        crate::reftest::assert_defines!(defs; APO_IDECONF, APO_IDECONF_SERR_EN, APO_IDECONF_DS_SOURCE, APO_IDECONF_ALT_INTR_EN, APO_IDECONF_PERR_EN, APO_IDECONF_DEVSEL_TME, APO_IDECONF_MAS_CMD_MON, APO_IDECONF_FIFO_CONF_MASK, APO_CTLMISC, APO_CTLMISC_BM_STS_RTY, APO_CTLMISC_FIFO_HWS, APO_CTLMISC_WR_IRDY_WS, APO_CTLMISC_RD_IRDY_WS, APO_CTLMISC_INTR_SWP, APO_CTLMISC_DRDY_TIME_MASK, APO_DATATIM, APO_MISCTIM, APO_UDMA);
    }
}
/* </TESTS> */
