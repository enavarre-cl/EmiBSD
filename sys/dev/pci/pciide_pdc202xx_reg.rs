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
//! Promise PDC202xx, PDC203xx and PDC205xx SATA IDE controller registers.
//!
//! Upstream: sys/dev/pci/pciide_pdc202xx_reg.h @ 3ce1f3f79392
//!
//! ## Deviations
//! - The function-like macros are `const fn`s named in lower case. The configuration-space
//!   offsets `pciide.c` reads and writes are `i32` (`pci_conf_read`'s `reg`), bus space
//!   offsets `BusSize`, shift counts `i32`, register values and unused offsets `u32`.
//! - The `static` timing tables are `pub const` arrays named in capitals, of `u8` where the
//!   C has `int8_t` or `u_int8_t` (every entry is written to a byte-wide register or shifted
//!   into a field, never sign-extended).
//! - `SCONTROL_WRITE(ps, channel, scontrol)` and `SSTATUS_READ(sc, channel)` (which reads the caller's `ps`) are functions over a [`PciidePdcsata`].
//! - `struct pciide_pdcsata` is [`PciidePdcsata`]: its tags and handles are `Option`s (it is allocated zeroed, as `M_ZERO`), set by `pdcsata_chip_map`.
//! - `wdc_pdc203xx_vtbl` is a `static` [`ChannelSoftcVtbl`] over `pciide.rs`'s `pdc203xx_read_reg`/`pdc203xx_write_reg`.
#![allow(non_upper_case_globals)] // the C's mixed-case names (`PDC2xx_STATE`, `ACER_0x4B`)

use crate::dev::ic::wdc::{
    wdc_default_lba48_write_reg, wdc_default_read_raw_multi_2, wdc_default_read_raw_multi_4,
    wdc_default_write_raw_multi_2, wdc_default_write_raw_multi_4,
};
use crate::dev::ic::wdcvar::{ChannelSoftcVtbl, WDC_NREG, WDC_NSHADOWREG};
use crate::dev::pci::pciide::{pdc203xx_read_reg, pdc203xx_write_reg};
use crate::dev::pci::pciidereg::IDEDMA_NREGS;
use crate::dev::pci::pciidereg::IDEDMA_SCH_OFFSET;
use crate::kern::subr_prf::panic;
use crate::machine::bus::BusSize;
use crate::machine::bus::{BusSpaceHandle, BusSpaceTag, bus_space_read_4, bus_space_write_4};
use core::cell::Cell;

// Registers definitions for PROMISE PDC20246/PDC20262 PCI IDE controller.
// Unfortunately the HW docs are not publicly available. I've been able
// to get a partial one for the PDC20246, and a better one for the PDC20262
// from Promise.

/// `PDC2xx_STATE`.
pub const PDC2xx_STATE: i32 = 0x50;
/// `PDC2xx_STATE_IDERAID`.
pub const PDC2xx_STATE_IDERAID: u32 = 0x0001;
/// `PDC2xx_STATE_NATIVE`.
pub const PDC2xx_STATE_NATIVE: u32 = 0x0080;
// controller initial state values(PDC20246 only)
/// `PDC246_STATE_SHIPID`.
pub const PDC246_STATE_SHIPID: u32 = 0x8000;
/// `PDC246_STATE_IOCHRDY`.
pub const PDC246_STATE_IOCHRDY: u32 = 0x0400;
/// `PDC246_STATE_LBA`.
pub const fn pdc246_state_lba(channel: i32) -> u32 {
    0x0100 << channel
}
/// `PDC246_STATE_ISAIRQ`.
pub const PDC246_STATE_ISAIRQ: u32 = 0x0008;
/// `PDC246_STATE_EN`.
pub const fn pdc246_state_en(channel: i32) -> u32 {
    0x0002 << channel
}
// controller initial state values(PDC20262 only)
/// `PDC262_STATE_EN`.
pub const fn pdc262_state_en(chan: i32) -> u32 {
    0x1000 << chan
}
/// `PDC262_STATE_80P`.
pub const fn pdc262_state_80p(chan: i32) -> u32 {
    0x0400 << chan
}

// per-drive timings
/// `PDC2xx_TIM`.
pub const fn pdc2xx_tim(channel: i32, drive: i32) -> i32 {
    0x60 + 4 * drive + 8 * channel
}
/// `PDC2xx_TIM_SET_PA`.
pub const fn pdc2xx_tim_set_pa(r: u32, x: u32) -> u32 {
    (r & 0xfffffff0) | (x & 0xf)
}
/// `PDC2xx_TIM_SET_PB`.
pub const fn pdc2xx_tim_set_pb(r: u32, x: u32) -> u32 {
    (r & 0xffffe0ff) | ((x & 0x1f) << 8)
}
/// `PDC2xx_TIM_SET_MB`.
pub const fn pdc2xx_tim_set_mb(r: u32, x: u32) -> u32 {
    (r & 0xffff1fff) | ((x & 0x7) << 13)
}
/// `PDC2xx_TIM_SET_MC`.
pub const fn pdc2xx_tim_set_mc(r: u32, x: u32) -> u32 {
    (r & 0xfff0ffff) | ((x & 0xf) << 16)
}
/// `PDC2xx_TIM_PRE`.
pub const PDC2xx_TIM_PRE: u32 = 0x00000010;
/// `PDC2xx_TIM_IORDY`.
pub const PDC2xx_TIM_IORDY: u32 = 0x00000020;
/// `PDC2xx_TIM_ERRDY`.
pub const PDC2xx_TIM_ERRDY: u32 = 0x00000040;
/// `PDC2xx_TIM_SYNC`.
pub const PDC2xx_TIM_SYNC: u32 = 0x00000080;
/// `PDC2xx_TIM_DMAW`.
pub const PDC2xx_TIM_DMAW: u32 = 0x00100000;
/// `PDC2xx_TIM_DMAR`.
pub const PDC2xx_TIM_DMAR: u32 = 0x00200000;
/// `PDC2xx_TIM_IORDYp`.
pub const PDC2xx_TIM_IORDYp: u32 = 0x00400000;
/// `PDC2xx_TIM_DMARQp`.
pub const PDC2xx_TIM_DMARQp: u32 = 0x00800000;

// The following are extensions of the DMA registers

// Ultra-DMA mode 3/4 control (PDC20262 only, 1 byte)
/// `PDC262_U66`.
pub const PDC262_U66: BusSize = 0x11;
/// `PDC262_U66_EN`.
pub const fn pdc262_u66_en(chan: i32) -> u32 {
    0x2 << (chan * 2)
}
// primary mode (1 byte)
/// `PDC2xx_PM`.
pub const PDC2xx_PM: BusSize = 0x1a;
// secondary mode (1 byte)
/// `PDC2xx_SM`.
pub const PDC2xx_SM: BusSize = 0x1b;
// System control register (4 bytes)
/// `PDC2xx_SCR`.
pub const PDC2xx_SCR: BusSize = 0x1c;
/// `PDC2xx_SCR_SET_GEN`.
pub const fn pdc2xx_scr_set_gen(r: u32, x: u32) -> u32 {
    (r & 0xffffff00) | (x & 0xff)
}
/// `PDC2xx_SCR_EMPTY`.
pub const fn pdc2xx_scr_empty(channel: i32) -> u32 {
    0x00000100 << (4 * channel)
}
/// `PDC2xx_SCR_FULL`.
pub const fn pdc2xx_scr_full(channel: i32) -> u32 {
    0x00000200 << (4 * channel)
}
/// `PDC2xx_SCR_INT`.
pub const fn pdc2xx_scr_int(channel: i32) -> u32 {
    0x00000400 << (4 * channel)
}
/// `PDC2xx_SCR_ERR`.
pub const fn pdc2xx_scr_err(channel: i32) -> u32 {
    0x00000800 << (4 * channel)
}
/// `PDC2xx_SCR_SET_I2C`.
pub const fn pdc2xx_scr_set_i2c(r: u32, x: u32) -> u32 {
    (r & 0xfff0ffff) | ((x & 0xf) << 16)
}
/// `PDC2xx_SCR_SET_POLL`.
pub const fn pdc2xx_scr_set_poll(r: u32, x: u32) -> u32 {
    (r & 0xff0fffff) | ((x & 0xf) << 20)
}
/// `PDC2xx_SCR_DMA`.
pub const PDC2xx_SCR_DMA: u32 = 0x01000000;
/// `PDC2xx_SCR_IORDY`.
pub const PDC2xx_SCR_IORDY: u32 = 0x02000000;
/// `PDC2xx_SCR_G2FD`.
pub const PDC2xx_SCR_G2FD: u32 = 0x04000000;
/// `PDC2xx_SCR_FLOAT`.
pub const PDC2xx_SCR_FLOAT: u32 = 0x08000000;
/// `PDC2xx_SCR_RSET`.
pub const PDC2xx_SCR_RSET: u32 = 0x10000000;
/// `PDC2xx_SCR_TST`.
pub const PDC2xx_SCR_TST: u32 = 0x20000000;
// Values for "General Purpose Register" (PDC20262 only)
/// `PDC262_SCR_GEN_LAT`.
pub const PDC262_SCR_GEN_LAT: u32 = 0x20;

// ATAPI port ((PDC20262 only) (4 bytes)
/// `PDC262_ATAPI`.
pub const fn pdc262_atapi(chan: i32) -> BusSize {
    0x20 + 4 * chan as BusSize
}
/// `PDC262_ATAPI_WC_MASK`.
pub const PDC262_ATAPI_WC_MASK: u32 = 0x00000fff;
/// `PDC262_ATAPI_DMA_READ`.
pub const PDC262_ATAPI_DMA_READ: u32 = 0x00001000;
/// `PDC262_ATAPI_DMA_WRITE`.
pub const PDC262_ATAPI_DMA_WRITE: u32 = 0x00002000;
/// `PDC262_ATAPI_UDMA`.
pub const PDC262_ATAPI_UDMA: u32 = 0x00004000;
/// `PDC262_ATAPI_LBA48_READ`.
pub const PDC262_ATAPI_LBA48_READ: u32 = 0x05000000;
/// `PDC262_ATAPI_LBA48_WRITE`.
pub const PDC262_ATAPI_LBA48_WRITE: u32 = 0x06000000;

// The timings provided here comes from the PDC20262 docs. I hope they are
// right for the PDC20246 too ...

/// `pdc2xx_pa`.
pub const PDC2XX_PA: [u8; 5] = [0x9, 0x5, 0x3, 0x2, 0x1];
/// `pdc2xx_pb`.
pub const PDC2XX_PB: [u8; 5] = [0x13, 0xc, 0x8, 0x6, 0x4];
/// `pdc2xx_dma_mb`.
pub const PDC2XX_DMA_MB: [u8; 3] = [0x3, 0x3, 0x3];
/// `pdc2xx_dma_mc`.
pub const PDC2XX_DMA_MC: [u8; 3] = [0x5, 0x4, 0x3];
/// `pdc2xx_udma_mb`.
pub const PDC2XX_UDMA_MB: [u8; 6] = [0x3, 0x2, 0x1, 0x2, 0x1, 0x1];
/// `pdc2xx_udma_mc`.
pub const PDC2XX_UDMA_MC: [u8; 6] = [0x3, 0x2, 0x1, 0x2, 0x1, 0x1];

// Registers definitions for Promise PDC20268 and above chips
/// `PDC268_INDEX`.
pub const fn pdc268_index(chan: i32) -> BusSize {
    0x01 + IDEDMA_SCH_OFFSET * chan as BusSize
}
/// `PDC268_DATA`.
pub const fn pdc268_data(chan: i32) -> BusSize {
    0x03 + IDEDMA_SCH_OFFSET * chan as BusSize
}
/// `PDC268_CABLE`.
pub const PDC268_CABLE: u32 = 0x04;
/// `PDC268_INTR`.
pub const PDC268_INTR: u32 = 0x20;

// PDC203xx register definitions.
/// `PDC203xx_NCHANNELS`.
pub const PDC203xx_NCHANNELS: i32 = 4;
/// `PDC203xx_BAR_IDEREGS`.
pub const PDC203xx_BAR_IDEREGS: u32 = 0x1c;

// PDC205xx register definitions.
/// `PDC40718_NCHANNELS`.
pub const PDC40718_NCHANNELS: i32 = 4;
/// `PDC20575_NCHANNELS`.
pub const PDC20575_NCHANNELS: i32 = 3;

/// `PDC205_REGADDR`.
pub const fn pdc205_regaddr(base: BusSize, ch: i32) -> BusSize {
    base + ((ch as BusSize) << 8)
}
/// `PDC205_SSTATUS`.
pub const fn pdc205_sstatus(ch: i32) -> BusSize {
    pdc205_regaddr(0x400, ch)
}
/// `PDC205_SERROR`.
pub const fn pdc205_serror(ch: i32) -> BusSize {
    pdc205_regaddr(0x404, ch)
}
/// `PDC205_SCONTROL`.
pub const fn pdc205_scontrol(ch: i32) -> BusSize {
    pdc205_regaddr(0x408, ch)
}
/// `PDC205_MULTIPLIER`.
pub const fn pdc205_multiplier(ch: i32) -> BusSize {
    pdc205_regaddr(0x4e8, ch)
}

/// `SCONTROL_WRITE`.
pub fn scontrol_write(ps: &PciidePdcsata, channel: i32, scontrol: u32) {
    bus_space_write_4(ps.ba5_st(), ps.ba5_sh(), pdc205_scontrol(channel), scontrol)
}

/// `SSTATUS_READ`.
pub fn sstatus_read(ps: &PciidePdcsata, channel: i32) -> u32 {
    bus_space_read_4(ps.ba5_st(), ps.ba5_sh(), pdc205_sstatus(channel))
}

// Private data

/// One channel's register handles in `struct pciide_pdcsata`.
pub struct PdcsataChanRegs {
    /// `cmd_iot`.
    pub cmd_iot: Cell<Option<BusSpaceTag>>,
    /// `cmd_iohs[WDC_NREG+WDC_NSHADOWREG]`.
    pub cmd_iohs: [Cell<Option<BusSpaceHandle>>; (WDC_NREG + WDC_NSHADOWREG) as usize],
    /// `ctl_iot`.
    pub ctl_iot: Cell<Option<BusSpaceTag>>,
    /// `ctl_ioh`.
    pub ctl_ioh: Cell<Option<BusSpaceHandle>>,
    /// `dma_iohs[IDEDMA_NREGS]`.
    pub dma_iohs: [Cell<Option<BusSpaceHandle>>; IDEDMA_NREGS],
}

impl PdcsataChanRegs {
    /// Nothing mapped.
    pub const fn new() -> Self {
        Self {
            cmd_iot: Cell::new(None),
            cmd_iohs: [const { Cell::new(None) }; (WDC_NREG + WDC_NSHADOWREG) as usize],
            ctl_iot: Cell::new(None),
            ctl_ioh: Cell::new(None),
            dma_iohs: [const { Cell::new(None) }; IDEDMA_NREGS],
        }
    }
}

impl Default for PdcsataChanRegs {
    fn default() -> Self {
        Self::new()
    }
}

/// `struct pciide_pdcsata`: private data.
pub struct PciidePdcsata {
    /// `ba5_st`.
    pub ba5_st: Cell<Option<BusSpaceTag>>,
    /// `ba5_sh`.
    pub ba5_sh: Cell<Option<BusSpaceHandle>>,
    /// `regs[PDC203xx_NCHANNELS]`.
    pub regs: [PdcsataChanRegs; PDC203xx_NCHANNELS as usize],
}

impl PciidePdcsata {
    /// Zeroed, as `malloc(.., M_ZERO)` leaves it.
    pub const fn new() -> Self {
        Self {
            ba5_st: Cell::new(None),
            ba5_sh: Cell::new(None),
            regs: [const { PdcsataChanRegs::new() }; PDC203xx_NCHANNELS as usize],
        }
    }

    /// `ps->ba5_st`, set when the IDE register BAR is mapped.
    pub fn ba5_st(&self) -> BusSpaceTag {
        match self.ba5_st.get() {
            Some(t) => t,
            None => panic(format_args!("pdcsata: IDE registers not mapped")),
        }
    }

    /// `ps->ba5_sh`.
    pub fn ba5_sh(&self) -> BusSpaceHandle {
        match self.ba5_sh.get() {
            Some(h) => h,
            None => panic(format_args!("pdcsata: IDE registers not mapped")),
        }
    }
}

impl Default for PciidePdcsata {
    fn default() -> Self {
        Self::new()
    }
}

/// `wdc_pdc203xx_vtbl`.
pub static WDC_PDC203XX_VTBL: ChannelSoftcVtbl = ChannelSoftcVtbl {
    read_reg: pdc203xx_read_reg,
    write_reg: pdc203xx_write_reg,
    lba48_write_reg: wdc_default_lba48_write_reg,
    read_raw_multi_2: wdc_default_read_raw_multi_2,
    write_raw_multi_2: wdc_default_write_raw_multi_2,
    read_raw_multi_4: wdc_default_read_raw_multi_4,
    write_raw_multi_4: wdc_default_write_raw_multi_4,
};
/* </CODE> */

/* <TESTS> */
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    #[ignore = "needs OPENBSD_SRC (just test-ref)"]
    fn defines_match_the_header() {
        let defs = crate::reftest::defines("sys/dev/pci/pciide_pdc202xx_reg.h");

        crate::reftest::assert_defines!(defs; PDC2xx_STATE, PDC2xx_STATE_IDERAID, PDC2xx_STATE_NATIVE, PDC246_STATE_SHIPID, PDC246_STATE_IOCHRDY, PDC246_STATE_ISAIRQ, PDC2xx_TIM_PRE, PDC2xx_TIM_IORDY, PDC2xx_TIM_ERRDY, PDC2xx_TIM_SYNC, PDC2xx_TIM_DMAW, PDC2xx_TIM_DMAR, PDC2xx_TIM_IORDYp, PDC2xx_TIM_DMARQp, PDC262_U66, PDC2xx_PM, PDC2xx_SM, PDC2xx_SCR, PDC2xx_SCR_DMA, PDC2xx_SCR_IORDY, PDC2xx_SCR_G2FD, PDC2xx_SCR_FLOAT, PDC2xx_SCR_RSET, PDC2xx_SCR_TST, PDC262_SCR_GEN_LAT, PDC262_ATAPI_WC_MASK, PDC262_ATAPI_DMA_READ, PDC262_ATAPI_DMA_WRITE, PDC262_ATAPI_UDMA, PDC262_ATAPI_LBA48_READ, PDC262_ATAPI_LBA48_WRITE, PDC268_CABLE, PDC268_INTR, PDC203xx_NCHANNELS, PDC203xx_BAR_IDEREGS, PDC40718_NCHANNELS, PDC20575_NCHANNELS);
    }
}
/* </TESTS> */
