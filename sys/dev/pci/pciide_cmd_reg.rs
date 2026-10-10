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
//! CMD Technology PCI 064x and Silicon Image 0680 IDE controller registers.
//!
//! Upstream: sys/dev/pci/pciide_cmd_reg.h @ 3ce1f3f79392
//!
//! ## Deviations
//! - The function-like macros are `const fn`s named in lower case. The configuration-space
//!   offsets `pciide.c` reads and writes are `i32` (`pci_conf_read`'s `reg`), bus space
//!   offsets `BusSize`, shift counts `i32`, register values and unused offsets `u32`.
//! - The `static` timing tables are `pub const` arrays named in capitals, of `u8` where the
//!   C has `int8_t` or `u_int8_t` (every entry is written to a byte-wide register or shifted
//!   into a field, never sign-extended).
//! - `CMD_DATA_TIM(chan, drive)`'s nested conditional is an `if`.
#![allow(non_upper_case_globals)] // the C's mixed-case names (`PDC2xx_STATE`, `ACER_0x4B`)

// Registers definitions for CMD Technologies's PCI 064x IDE controllers.

// Interesting revision of the 0646
/// `CMD0646U2_REV`.
pub const CMD0646U2_REV: u32 = 0x05;
/// `CMD0646U_REV`.
pub const CMD0646U_REV: u32 = 0x03;

// Configuration RO
/// `CMD_CONF`.
pub const CMD_CONF: i32 = 0x50;
/// `CMD_CONF_REV_MASK`: 0640/3/6 only.
pub const CMD_CONF_REV_MASK: u32 = 0x03;
/// `CMD_CONF_DRV0_INTR`.
pub const CMD_CONF_DRV0_INTR: u32 = 0x04;
/// `CMD_CONF_DEVID`: 0640/3/6 only.
pub const CMD_CONF_DEVID: u32 = 0x18;
/// `CMD_CONF_VESAPRT`: 0640/3/6 only.
pub const CMD_CONF_VESAPRT: u32 = 0x20;
/// `CMD_CONF_DSA1`.
pub const CMD_CONF_DSA1: u32 = 0x40;
/// `CMD_CONF_DSA0`: 0640/3/6 only.
pub const CMD_CONF_DSA0: u32 = 0x80;

// Control register RW
/// `CMD_CTRL`.
pub const CMD_CTRL: i32 = 0x51;
/// `CMD_CTRL_HR_FIFO`: 0640/3/6 only.
pub const CMD_CTRL_HR_FIFO: u32 = 0x01;
/// `CMD_CTRL_HW_FIFO`: 0640/3/6 only.
pub const CMD_CTRL_HW_FIFO: u32 = 0x02;
/// `CMD_CTRL_DEVSEL`.
pub const CMD_CTRL_DEVSEL: u32 = 0x04;
/// `CMD_CTRL_2PORT`.
pub const CMD_CTRL_2PORT: u32 = 0x08;
/// `CMD_CTRL_PAR`: 0640/3/6 only.
pub const CMD_CTRL_PAR: u32 = 0x10;
/// `CMD_CTRL_HW_HLD`: 0640/3/6 only.
pub const CMD_CTRL_HW_HLD: u32 = 0x20;
/// `CMD_CTRL_DRV0_RAHEAD`.
pub const CMD_CTRL_DRV0_RAHEAD: u32 = 0x40;
/// `CMD_CTRL_DRV1_RAHEAD`.
pub const CMD_CTRL_DRV1_RAHEAD: u32 = 0x80;

// data read/write timing registers . 0640 uses the same for drive 0 and 1
// on the secondary channel
/// `CMD_DATA_TIM`.
pub const fn cmd_data_tim(chan: i32, drive: i32) -> i32 {
    if chan == 0 {
        if drive == 0 { 0x54 } else { 0x56 }
    } else if drive == 0 {
        0x58
    } else {
        0x5b
    }
}

// secondary channel status and addr timings
/// `CMD_ARTTIM23`.
pub const CMD_ARTTIM23: i32 = 0x57;
/// `CMD_ARTTIM23_IRQ`.
pub const CMD_ARTTIM23_IRQ: u32 = 0x10;
/// `CMD_ARTTIM23_RHAEAD`.
pub const fn cmd_arttim23_rhaead(d: i32) -> u32 {
    0x4 << d
}

// DMA master read mode select
/// `CMD_DMA_MODE`.
pub const CMD_DMA_MODE: i32 = 0x71;
/// `CMD_DMA_MASK`.
pub const CMD_DMA_MASK: u32 = 0x03;
/// `CMD_DMA`.
pub const CMD_DMA: u32 = 0x00;
/// `CMD_DMA_MULTIPLE`.
pub const CMD_DMA_MULTIPLE: u32 = 0x01;
/// `CMD_DMA_LINE`.
pub const CMD_DMA_LINE: u32 = 0x03;
// the following bits are only for 0646U/646U2/648/649
/// `CMD_DMA_IRQ`.
pub const fn cmd_dma_irq(chan: i32) -> u32 {
    0x4 << chan
}
/// `CMD_DMA_IRQ_DIS`.
pub const fn cmd_dma_irq_dis(chan: i32) -> u32 {
    0x10 << chan
}
/// `CMD_DMA_RST`.
pub const CMD_DMA_RST: u32 = 0x40;

// the following is only for 0646U/646U2/648/649
// busmaster control/status register
/// `CMD_BICSR`.
pub const CMD_BICSR: i32 = 0x79;
/// `CMD_BICSR_80`.
pub const fn cmd_bicsr_80(chan: i32) -> u32 {
    0x01 << chan
}
// Ultra/DMA timings reg
/// `CMD_UDMATIM`.
pub const fn cmd_udmatim(channel: i32) -> i32 {
    0x73 + (8 * channel)
}
/// `CMD_UDMATIM_UDMA`.
pub const fn cmd_udmatim_udma(drive: i32) -> u32 {
    0x01 << drive
}
/// `CMD_UDMATIM_UDMA33`.
pub const fn cmd_udmatim_udma33(drive: i32) -> u32 {
    0x04 << drive
}
/// `CMD_UDMATIM_TIM_MASK`.
pub const CMD_UDMATIM_TIM_MASK: u32 = 0x3;
/// `CMD_UDMATIM_TIM_OFF`.
pub const fn cmd_udmatim_tim_off(drive: i32) -> i32 {
    4 + (drive * 2)
}
/// `cmd0646_9_tim_udma`.
pub const CMD0646_9_TIM_UDMA: [u8; 6] = [0x03, 0x02, 0x01, 0x02, 0x01, 0x00];

// timings values for the 0643/6/8/9
// for all dma_mode we have to have
// DMA_timings(dma_mode) >= PIO_timings(dma_mode + 2)
/// `cmd0643_9_data_tim_pio`.
pub const CMD0643_9_DATA_TIM_PIO: [u8; 5] = [0xA9, 0x57, 0x44, 0x32, 0x3F];
/// `cmd0643_9_data_tim_dma`.
pub const CMD0643_9_DATA_TIM_DMA: [u8; 3] = [0x87, 0x32, 0x3F];
/* </CODE> */

/* <TESTS> */
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    #[ignore = "needs OPENBSD_SRC (just test-ref)"]
    fn defines_match_the_header() {
        let defs = crate::reftest::defines("sys/dev/pci/pciide_cmd_reg.h");

        crate::reftest::assert_defines!(defs; CMD0646U2_REV, CMD0646U_REV, CMD_CONF, CMD_CONF_REV_MASK, CMD_CONF_DRV0_INTR, CMD_CONF_DEVID, CMD_CONF_VESAPRT, CMD_CONF_DSA1, CMD_CONF_DSA0, CMD_CTRL, CMD_CTRL_HR_FIFO, CMD_CTRL_HW_FIFO, CMD_CTRL_DEVSEL, CMD_CTRL_2PORT, CMD_CTRL_PAR, CMD_CTRL_HW_HLD, CMD_CTRL_DRV0_RAHEAD, CMD_CTRL_DRV1_RAHEAD, CMD_ARTTIM23, CMD_ARTTIM23_IRQ, CMD_DMA_MODE, CMD_DMA_MASK, CMD_DMA, CMD_DMA_MULTIPLE, CMD_DMA_LINE, CMD_DMA_RST, CMD_BICSR, CMD_UDMATIM_TIM_MASK);
    }
}
/* </TESTS> */
