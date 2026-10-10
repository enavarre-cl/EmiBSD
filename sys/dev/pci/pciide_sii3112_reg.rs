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
 * Copyright (c) 2003 Wasabi Systems, Inc.
 * All rights reserved.
 *
 * Written by Jason R. Thorpe for Wasabi Systems, Inc.
 *
 * Redistribution and use in source and binary forms, with or without
 * modification, are permitted provided that the following conditions
 * are met:
 * 1. Redistributions of source code must retain the above copyright
 *    notice, this list of conditions and the following disclaimer.
 * 2. Redistributions in binary form must reproduce the above copyright
 *    notice, this list of conditions and the following disclaimer in the
 *    documentation and/or other materials provided with the distribution.
 * 3. All advertising materials mentioning features or use of this software
 *    must display the following acknowledgement:
 *	This product includes software developed for the NetBSD Project by
 *	Wasabi Systems, Inc.
 * 4. The name of Wasabi Systems, Inc. may not be used to endorse
 *    or promote products derived from this software without specific prior
 *    written permission.
 *
 * THIS SOFTWARE IS PROVIDED BY WASABI SYSTEMS, INC. ``AS IS'' AND
 * ANY EXPRESS OR IMPLIED WARRANTIES, INCLUDING, BUT NOT LIMITED
 * TO, THE IMPLIED WARRANTIES OF MERCHANTABILITY AND FITNESS FOR A PARTICULAR
 * PURPOSE ARE DISCLAIMED.  IN NO EVENT SHALL WASABI SYSTEMS, INC
 * BE LIABLE FOR ANY DIRECT, INDIRECT, INCIDENTAL, SPECIAL, EXEMPLARY, OR
 * CONSEQUENTIAL DAMAGES (INCLUDING, BUT NOT LIMITED TO, PROCUREMENT OF
 * SUBSTITUTE GOODS OR SERVICES; LOSS OF USE, DATA, OR PROFITS; OR BUSINESS
 * INTERRUPTION) HOWEVER CAUSED AND ON ANY THEORY OF LIABILITY, WHETHER IN
 * CONTRACT, STRICT LIABILITY, OR TORT (INCLUDING NEGLIGENCE OR OTHERWISE)
 * ARISING IN ANY WAY OUT OF THE USE OF THIS SOFTWARE, EVEN IF ADVISED OF THE
 * POSSIBILITY OF SUCH DAMAGE.
 */
/* </LICENSES> */

/* <CODE> */
//! Silicon Image SiI3112/3512/3114 SATA controller registers.
//!
//! Upstream: sys/dev/pci/pciide_sii3112_reg.h @ 3ce1f3f79392
//!
//! ## Deviations
//! - The function-like macros are `const fn`s named in lower case. The configuration-space
//!   offsets `pciide.c` reads and writes are `i32` (`pci_conf_read`'s `reg`), bus space
//!   offsets `BusSize`, shift counts `i32`, register values and unused offsets `u32`.
//! - The `static` timing tables are `pub const` arrays named in capitals, of `u8` where the
//!   C has `int8_t` or `u_int8_t` (every entry is written to a byte-wide register or shifted
//!   into a field, never sign-extended).
//! - The `static const struct { .. } satalink_ba5_regmap[]` is [`SATALINK_BA5_REGMAP`], an array of [`SatalinkBa5Regmap`] (its member names kept).
//! - `struct pciide_satalink` is [`PciideSatalink`] (tags and handles `Option`s, allocated zeroed).
//! - The header's `static` functions `ba5_read_4_ind`, `ba5_read_4`, `ba5_write_4_ind` and `ba5_write_4` are public functions here, reaching the cookie through [`satalink`], which checks the chip; the `BA5_READ_4(sc, chan, reg)`/`BA5_WRITE_4` macros, which name a member, are written out where `pciide.rs` uses them (`ba5_write_4(sc, SATALINK_BA5_REGMAP[chan].ba5_IDE_DTM, v)`).
//! - `wdc_sii3114_vtbl` is a `static` [`ChannelSoftcVtbl`] over `pciide.rs`'s `sii3114_read_reg`/`sii3114_write_reg`.
#![allow(non_upper_case_globals)] // the C's mixed-case names (`PDC2xx_STATE`, `ACER_0x4B`)
#![allow(non_snake_case)] // the C's member names (`ba5_IDEDMA_CMD`, `ba5_SControl`)

use crate::dev::ic::wdc::{
    wdc_default_lba48_write_reg, wdc_default_read_raw_multi_2, wdc_default_read_raw_multi_4,
    wdc_default_write_raw_multi_2, wdc_default_write_raw_multi_4,
};
use crate::dev::ic::wdcvar::{ChannelSoftcVtbl, WDC_NREG, WDC_NSHADOWREG};
use crate::dev::pci::pciide::{
    PciideChipMap, sii3112_chip_map, sii3114_chip_map, sii3114_read_reg, sii3114_write_reg,
};
use crate::dev::pci::pciidereg::IDEDMA_NREGS;
use crate::dev::pci::pciidevar::PciideSoftc;
use crate::dev::pci::pcivar::Pcireg;
use crate::kern::subr_prf::panic;
use crate::machine::bus::BusSize;
use crate::machine::bus::{
    BusAddr, BusSpaceHandle, BusSpaceTag, bus_space_read_4, bus_space_write_4,
};
use crate::machine::intr::{splbio, splx};
use crate::machine::pci_machdep::{pci_conf_read, pci_conf_write};
use core::cell::Cell;
use core::ptr;

// PCI configuration space registers.

/// `SII3112_PCI_CFGCTL`.
pub const SII3112_PCI_CFGCTL: i32 = 0x40;
/// `CFGCTL_CFGWREN`: enable cfg writes.
pub const CFGCTL_CFGWREN: u32 = 1 << 0;
/// `CFGCTL_BA5INDEN`: BA5 indirect access enable.
pub const CFGCTL_BA5INDEN: u32 = 1 << 1;

/// `SII3112_PCI_SWDATA`.
pub const SII3112_PCI_SWDATA: u32 = 0x44;

/// `SII3112_PCI_BM_IDE0`.
pub const SII3112_PCI_BM_IDE0: u32 = 0x70;
// == BAR4+0x00

/// `SII3112_PCI_PRD_IDE0`.
pub const SII3112_PCI_PRD_IDE0: u32 = 0x74;
// == BAR4+0x04

/// `SII3112_PCI_BM_IDE1`.
pub const SII3112_PCI_BM_IDE1: u32 = 0x78;
// == BAR4+0x08

/// `SII3112_PCI_PRD_IDE1`.
pub const SII3112_PCI_PRD_IDE1: u32 = 0x7c;
// == BAR4+0x0c

/// `SII3112_DTM_IDE0`: Data Transfer Mode - IDE0.
pub const SII3112_DTM_IDE0: u32 = 0x80;
/// `SII3112_DTM_IDE1`: Data Transfer Mode - IDE1.
pub const SII3112_DTM_IDE1: u32 = 0x84;
/// `DTM_IDEx_PIO`: PCI DMA, IDE PIO (or 1).
pub const DTM_IDEx_PIO: u32 = 0x00000000;
/// `DTM_IDEx_DMA`: PCI DMA, IDE DMA (or 3).
pub const DTM_IDEx_DMA: u32 = 0x00000002;

/// `SII3112_SCS_CMD`: System Config Status.
pub const SII3112_SCS_CMD: i32 = 0x88;
/// `SCS_CMD_PBM_RESET`: PBM module reset.
pub const SCS_CMD_PBM_RESET: u32 = 1 << 0;
/// `SCS_CMD_ARB_RESET`: ARB module reset.
pub const SCS_CMD_ARB_RESET: u32 = 1 << 1;
/// `SCS_CMD_FF1_RESET`: IDE1 FIFO reset.
pub const SCS_CMD_FF1_RESET: u32 = 1 << 4;
/// `SCS_CMD_FF0_RESET`: IDE0 FIFO reset.
pub const SCS_CMD_FF0_RESET: u32 = 1 << 5;
/// `SCS_CMD_IDE1_RESET`: IDE1 module reset.
pub const SCS_CMD_IDE1_RESET: u32 = 1 << 6;
/// `SCS_CMD_IDE0_RESET`: IDE0 module reset.
pub const SCS_CMD_IDE0_RESET: u32 = 1 << 7;
/// `SCS_CMD_FF3_RESET`: IDE3 FIFO reset 3114.
pub const SCS_CMD_FF3_RESET: u32 = 1 << 8;
/// `SCS_CMD_FF2_RESET`: IDE2 FIFO reset 3114.
pub const SCS_CMD_FF2_RESET: u32 = 1 << 9;
/// `SCS_CMD_IDE3_RESET`: IDE3 module reset 3114.
pub const SCS_CMD_IDE3_RESET: u32 = 1 << 10;
/// `SCS_CMD_IDE2_RESET`: IDE2 module reset 3114.
pub const SCS_CMD_IDE2_RESET: u32 = 1 << 11;
/// `SCS_CMD_BA5_EN`: BA5 is enabled 3112.
pub const SCS_CMD_BA5_EN: u32 = 1 << 16;
/// `SCS_CMD_M66EN`: 1=66MHz, 0=33MHz 3114.
pub const SCS_CMD_M66EN: u32 = 1 << 16;
/// `SCS_CMD_IDE0_INT_BLOCK`: IDE0 interrupt block.
pub const SCS_CMD_IDE0_INT_BLOCK: u32 = 1 << 22;
/// `SCS_CMD_IDE1_INT_BLOCK`: IDE1 interrupt block.
pub const SCS_CMD_IDE1_INT_BLOCK: u32 = 1 << 23;
/// `SCS_CMD_IDE2_INT_BLOCK`: IDE2 interrupt block.
pub const SCS_CMD_IDE2_INT_BLOCK: u32 = 1 << 24;
/// `SCS_CMD_IDE3_INT_BLOCK`: IDE3 interrupt block.
pub const SCS_CMD_IDE3_INT_BLOCK: u32 = 1 << 25;

/// `SII3112_SSDR`: System SW Data Register.
pub const SII3112_SSDR: u32 = 0x8c;

/// `SII3112_FMA_CSR`: Flash Memory Addr - CSR.
pub const SII3112_FMA_CSR: u32 = 0x90;

/// `SII3112_FM_DATA`: Flash Memory Data.
pub const SII3112_FM_DATA: u32 = 0x94;

/// `SII3112_EEA_CSR`: EEPROM Memory Addr - CSR.
pub const SII3112_EEA_CSR: u32 = 0x98;

/// `SII3112_EE_DATA`: EEPROM Data.
pub const SII3112_EE_DATA: u32 = 0x9c;

/// `SII3112_TCS_IDE0`: IDEx config, status.
pub const SII3112_TCS_IDE0: u32 = 0xa0;
/// `SII3112_TCS_IDE1`.
pub const SII3112_TCS_IDE1: u32 = 0xb0;
/// `TCS_IDEx_BCA`: buffered command active.
pub const TCS_IDEx_BCA: u32 = 1 << 1;
/// `TCS_IDEx_CH_RESET`: channel reset.
pub const TCS_IDEx_CH_RESET: u32 = 1 << 2;
/// `TCS_IDEx_VDMA_INT`: virtual DMA interrupt.
pub const TCS_IDEx_VDMA_INT: u32 = 1 << 10;
/// `TCS_IDEx_INT`: interrupt status.
pub const TCS_IDEx_INT: u32 = 1 << 11;
/// `TCS_IDEx_WTT`: watchdog timer timeout.
pub const TCS_IDEx_WTT: u32 = 1 << 12;
/// `TCS_IDEx_WTEN`: watchdog timer enable.
pub const TCS_IDEx_WTEN: u32 = 1 << 13;
/// `TCS_IDEx_WTINTEN`: watchdog timer int. enable.
pub const TCS_IDEx_WTINTEN: u32 = 1 << 14;

/// `SII3112_BA5_IND_ADDR`: BA5 indirect address.
pub const SII3112_BA5_IND_ADDR: i32 = 0xc0;

/// `SII3112_BA5_IND_DATA`: BA5 indirect data.
pub const SII3112_BA5_IND_DATA: i32 = 0xc4;

// Register map for BA5 register space, indexed by channel.

/// `ba5_SIS`: summary interrupt status.
pub const ba5_SIS: u32 = 0x214;

// Interrupt steering bit in BA5[0x200].
/// `IDEDMA_CMD_INT_STEER`.
pub const IDEDMA_CMD_INT_STEER: u32 = 1 << 1;

// Private data

/// One channel's row of `satalink_ba5_regmap[]`: the BA5 offsets of its registers.
#[derive(Clone, Copy)]
pub struct SatalinkBa5Regmap {
    /// `ba5_IDEDMA_CMD`.
    pub ba5_IDEDMA_CMD: BusAddr,
    /// `ba5_IDEDMA_CTL`.
    pub ba5_IDEDMA_CTL: BusAddr,
    /// `ba5_IDEDMA_TBL`.
    pub ba5_IDEDMA_TBL: BusAddr,
    /// `ba5_IDEDMA_CMD2`.
    pub ba5_IDEDMA_CMD2: BusAddr,
    /// `ba5_IDEDMA_CTL2`.
    pub ba5_IDEDMA_CTL2: BusAddr,
    /// `ba5_IDE_TF0`: wd_data.
    pub ba5_IDE_TF0: BusAddr,
    /// `ba5_IDE_TF1`: wd_error.
    pub ba5_IDE_TF1: BusAddr,
    /// `ba5_IDE_TF2`: wd_seccnt.
    pub ba5_IDE_TF2: BusAddr,
    /// `ba5_IDE_TF3`: wd_sector.
    pub ba5_IDE_TF3: BusAddr,
    /// `ba5_IDE_TF4`: wd_cyl_lo.
    pub ba5_IDE_TF4: BusAddr,
    /// `ba5_IDE_TF5`: wd_cyl_hi.
    pub ba5_IDE_TF5: BusAddr,
    /// `ba5_IDE_TF6`: wd_sdh.
    pub ba5_IDE_TF6: BusAddr,
    /// `ba5_IDE_TF7`: wd_command.
    pub ba5_IDE_TF7: BusAddr,
    /// `ba5_IDE_TF8`: wd_altsts.
    pub ba5_IDE_TF8: BusAddr,
    /// `ba5_IDE_RAD`.
    pub ba5_IDE_RAD: BusAddr,
    /// `ba5_IDE_TF9`: Features 2.
    pub ba5_IDE_TF9: BusAddr,
    /// `ba5_IDE_TF10`: Sector Count 2.
    pub ba5_IDE_TF10: BusAddr,
    /// `ba5_IDE_TF11`: Start Sector 2.
    pub ba5_IDE_TF11: BusAddr,
    /// `ba5_IDE_TF12`: Cylinder Low 2.
    pub ba5_IDE_TF12: BusAddr,
    /// `ba5_IDE_TF13`: Cylinder High 2.
    pub ba5_IDE_TF13: BusAddr,
    /// `ba5_IDE_TF14`: Device/Head 2.
    pub ba5_IDE_TF14: BusAddr,
    /// `ba5_IDE_TF15`: Cmd Sts 2.
    pub ba5_IDE_TF15: BusAddr,
    /// `ba5_IDE_TF16`: Sector Count 2 ext.
    pub ba5_IDE_TF16: BusAddr,
    /// `ba5_IDE_TF17`: Start Sector 2 ext.
    pub ba5_IDE_TF17: BusAddr,
    /// `ba5_IDE_TF18`: Cyl Low 2 ext.
    pub ba5_IDE_TF18: BusAddr,
    /// `ba5_IDE_TF19`: Cyl High 2 ext.
    pub ba5_IDE_TF19: BusAddr,
    /// `ba5_IDE_RABC`.
    pub ba5_IDE_RABC: BusAddr,
    /// `ba5_IDE_CMD_STS`.
    pub ba5_IDE_CMD_STS: BusAddr,
    /// `ba5_IDE_CFG_STS`.
    pub ba5_IDE_CFG_STS: BusAddr,
    /// `ba5_IDE_DTM`.
    pub ba5_IDE_DTM: BusAddr,
    /// `ba5_SControl`.
    pub ba5_SControl: BusAddr,
    /// `ba5_SStatus`.
    pub ba5_SStatus: BusAddr,
    /// `ba5_SError`.
    pub ba5_SError: BusAddr,
    /// `ba5_SActive`: 3114.
    pub ba5_SActive: BusAddr,
    /// `ba5_SMisc`.
    pub ba5_SMisc: BusAddr,
    /// `ba5_PHY_CONFIG`.
    pub ba5_PHY_CONFIG: BusAddr,
    /// `ba5_SIEN`.
    pub ba5_SIEN: BusAddr,
    /// `ba5_SFISCfg`.
    pub ba5_SFISCfg: BusAddr,
}

/// One channel's register handles in `struct pciide_satalink`.
pub struct SatalinkChanRegs {
    /// `cmd_iot`.
    pub cmd_iot: Cell<Option<BusSpaceTag>>,
    /// `cmd_baseioh`.
    pub cmd_baseioh: Cell<Option<BusSpaceHandle>>,
    /// `cmd_iohs[WDC_NREG+WDC_NSHADOWREG]`.
    pub cmd_iohs: [Cell<Option<BusSpaceHandle>>; (WDC_NREG + WDC_NSHADOWREG) as usize],
    /// `ctl_iot`.
    pub ctl_iot: Cell<Option<BusSpaceTag>>,
    /// `ctl_ioh`.
    pub ctl_ioh: Cell<Option<BusSpaceHandle>>,
    /// `dma_iohs[IDEDMA_NREGS]`.
    pub dma_iohs: [Cell<Option<BusSpaceHandle>>; IDEDMA_NREGS],
}

impl SatalinkChanRegs {
    /// Nothing mapped.
    pub const fn new() -> Self {
        Self {
            cmd_iot: Cell::new(None),
            cmd_baseioh: Cell::new(None),
            cmd_iohs: [const { Cell::new(None) }; (WDC_NREG + WDC_NSHADOWREG) as usize],
            ctl_iot: Cell::new(None),
            ctl_ioh: Cell::new(None),
            dma_iohs: [const { Cell::new(None) }; IDEDMA_NREGS],
        }
    }
}

impl Default for SatalinkChanRegs {
    fn default() -> Self {
        Self::new()
    }
}

/// `struct pciide_satalink`: private data.
pub struct PciideSatalink {
    /// `ba5_st`.
    pub ba5_st: Cell<Option<BusSpaceTag>>,
    /// `ba5_sh`.
    pub ba5_sh: Cell<Option<BusSpaceHandle>>,
    /// `ba5_en`.
    pub ba5_en: Cell<i32>,
    /// `regs[4]`.
    pub regs: [SatalinkChanRegs; 4],
}

impl PciideSatalink {
    /// Zeroed, as `malloc(.., M_ZERO)` leaves it.
    pub const fn new() -> Self {
        Self {
            ba5_st: Cell::new(None),
            ba5_sh: Cell::new(None),
            ba5_en: Cell::new(0),
            regs: [const { SatalinkChanRegs::new() }; 4],
        }
    }

    /// `sl->ba5_st`, set when BA5 is mapped.
    pub fn ba5_st(&self) -> BusSpaceTag {
        match self.ba5_st.get() {
            Some(t) => t,
            None => panic(format_args!("satalink: BA5 not mapped")),
        }
    }

    /// `sl->ba5_sh`.
    pub fn ba5_sh(&self) -> BusSpaceHandle {
        match self.ba5_sh.get() {
            Some(h) => h,
            None => panic(format_args!("satalink: BA5 not mapped")),
        }
    }
}

impl Default for PciideSatalink {
    fn default() -> Self {
        Self::new()
    }
}

/// `satalink_ba5_regmap[]`: register map for BA5 register space, indexed by channel.
pub static SATALINK_BA5_REGMAP: [SatalinkBa5Regmap; 4] = [
    // Channel 0
    SatalinkBa5Regmap {
        ba5_IDEDMA_CMD: 0x000,
        ba5_IDEDMA_CTL: 0x002,
        ba5_IDEDMA_TBL: 0x004,
        ba5_IDEDMA_CMD2: 0x010,
        ba5_IDEDMA_CTL2: 0x012,
        ba5_IDE_TF0: 0x080,
        ba5_IDE_TF1: 0x081,
        ba5_IDE_TF2: 0x082,
        ba5_IDE_TF3: 0x083,
        ba5_IDE_TF4: 0x084,
        ba5_IDE_TF5: 0x085,
        ba5_IDE_TF6: 0x086,
        ba5_IDE_TF7: 0x087,
        ba5_IDE_TF8: 0x08a,
        ba5_IDE_RAD: 0x08c,
        ba5_IDE_TF9: 0x091,
        ba5_IDE_TF10: 0x092,
        ba5_IDE_TF11: 0x093,
        ba5_IDE_TF12: 0x094,
        ba5_IDE_TF13: 0x095,
        ba5_IDE_TF14: 0x096,
        ba5_IDE_TF15: 0x097,
        ba5_IDE_TF16: 0x098,
        ba5_IDE_TF17: 0x099,
        ba5_IDE_TF18: 0x09a,
        ba5_IDE_TF19: 0x09b,
        ba5_IDE_RABC: 0x09c,
        ba5_IDE_CMD_STS: 0x0a0,
        ba5_IDE_CFG_STS: 0x0a1,
        ba5_IDE_DTM: 0x0b4,
        ba5_SControl: 0x100,
        ba5_SStatus: 0x104,
        ba5_SError: 0x108,
        ba5_SActive: 0x10c,
        ba5_SMisc: 0x140,
        ba5_PHY_CONFIG: 0x144,
        ba5_SIEN: 0x148,
        ba5_SFISCfg: 0x14c,
    },
    // Channel 1
    SatalinkBa5Regmap {
        ba5_IDEDMA_CMD: 0x008,
        ba5_IDEDMA_CTL: 0x00a,
        ba5_IDEDMA_TBL: 0x00c,
        ba5_IDEDMA_CMD2: 0x018,
        ba5_IDEDMA_CTL2: 0x01a,
        ba5_IDE_TF0: 0x0c0,
        ba5_IDE_TF1: 0x0c1,
        ba5_IDE_TF2: 0x0c2,
        ba5_IDE_TF3: 0x0c3,
        ba5_IDE_TF4: 0x0c4,
        ba5_IDE_TF5: 0x0c5,
        ba5_IDE_TF6: 0x0c6,
        ba5_IDE_TF7: 0x0c7,
        ba5_IDE_TF8: 0x0ca,
        ba5_IDE_RAD: 0x0cc,
        ba5_IDE_TF9: 0x0d1,
        ba5_IDE_TF10: 0x0d2,
        ba5_IDE_TF11: 0x0d3,
        ba5_IDE_TF12: 0x0d4,
        ba5_IDE_TF13: 0x0d5,
        ba5_IDE_TF14: 0x0d6,
        ba5_IDE_TF15: 0x0d7,
        ba5_IDE_TF16: 0x0d8,
        ba5_IDE_TF17: 0x0d9,
        ba5_IDE_TF18: 0x0da,
        ba5_IDE_TF19: 0x0db,
        ba5_IDE_RABC: 0x0dc,
        ba5_IDE_CMD_STS: 0x0e0,
        ba5_IDE_CFG_STS: 0x0e1,
        ba5_IDE_DTM: 0x0f4,
        ba5_SControl: 0x180,
        ba5_SStatus: 0x184,
        ba5_SError: 0x188,
        ba5_SActive: 0x18c,
        ba5_SMisc: 0x1c0,
        ba5_PHY_CONFIG: 0x1c4,
        ba5_SIEN: 0x1c8,
        ba5_SFISCfg: 0x1cc,
    },
    // Channel 2 3114
    SatalinkBa5Regmap {
        ba5_IDEDMA_CMD: 0x200,
        ba5_IDEDMA_CTL: 0x202,
        ba5_IDEDMA_TBL: 0x204,
        ba5_IDEDMA_CMD2: 0x210,
        ba5_IDEDMA_CTL2: 0x212,
        ba5_IDE_TF0: 0x280,
        ba5_IDE_TF1: 0x281,
        ba5_IDE_TF2: 0x282,
        ba5_IDE_TF3: 0x283,
        ba5_IDE_TF4: 0x284,
        ba5_IDE_TF5: 0x285,
        ba5_IDE_TF6: 0x286,
        ba5_IDE_TF7: 0x287,
        ba5_IDE_TF8: 0x28a,
        ba5_IDE_RAD: 0x28c,
        ba5_IDE_TF9: 0x291,
        ba5_IDE_TF10: 0x292,
        ba5_IDE_TF11: 0x293,
        ba5_IDE_TF12: 0x294,
        ba5_IDE_TF13: 0x295,
        ba5_IDE_TF14: 0x296,
        ba5_IDE_TF15: 0x297,
        ba5_IDE_TF16: 0x298,
        ba5_IDE_TF17: 0x299,
        ba5_IDE_TF18: 0x29a,
        ba5_IDE_TF19: 0x29b,
        ba5_IDE_RABC: 0x29c,
        ba5_IDE_CMD_STS: 0x2a0,
        ba5_IDE_CFG_STS: 0x2a1,
        ba5_IDE_DTM: 0x2b4,
        ba5_SControl: 0x300,
        ba5_SStatus: 0x304,
        ba5_SError: 0x308,
        ba5_SActive: 0x30c,
        ba5_SMisc: 0x340,
        ba5_PHY_CONFIG: 0x344,
        ba5_SIEN: 0x348,
        ba5_SFISCfg: 0x34c,
    },
    // Channel 3 3114
    SatalinkBa5Regmap {
        ba5_IDEDMA_CMD: 0x208,
        ba5_IDEDMA_CTL: 0x20a,
        ba5_IDEDMA_TBL: 0x20c,
        ba5_IDEDMA_CMD2: 0x218,
        ba5_IDEDMA_CTL2: 0x21a,
        ba5_IDE_TF0: 0x2c0,
        ba5_IDE_TF1: 0x2c1,
        ba5_IDE_TF2: 0x2c2,
        ba5_IDE_TF3: 0x2c3,
        ba5_IDE_TF4: 0x2c4,
        ba5_IDE_TF5: 0x2c5,
        ba5_IDE_TF6: 0x2c6,
        ba5_IDE_TF7: 0x2c7,
        ba5_IDE_TF8: 0x2ca,
        ba5_IDE_RAD: 0x2cc,
        ba5_IDE_TF9: 0x2d1,
        ba5_IDE_TF10: 0x2d2,
        ba5_IDE_TF11: 0x2d3,
        ba5_IDE_TF12: 0x2d4,
        ba5_IDE_TF13: 0x2d5,
        ba5_IDE_TF14: 0x2d6,
        ba5_IDE_TF15: 0x2d7,
        ba5_IDE_TF16: 0x2d8,
        ba5_IDE_TF17: 0x2d9,
        ba5_IDE_TF18: 0x2da,
        ba5_IDE_TF19: 0x2db,
        ba5_IDE_RABC: 0x2dc,
        ba5_IDE_CMD_STS: 0x2e0,
        ba5_IDE_CFG_STS: 0x2e1,
        ba5_IDE_DTM: 0x2f4,
        ba5_SControl: 0x380,
        ba5_SStatus: 0x384,
        ba5_SError: 0x388,
        ba5_SActive: 0x38c,
        ba5_SMisc: 0x3c0,
        ba5_PHY_CONFIG: 0x3c4,
        ba5_SIEN: 0x3c8,
        ba5_SFISCfg: 0x3cc,
    },
];

/// `wdc_sii3114_vtbl`.
pub static WDC_SII3114_VTBL: ChannelSoftcVtbl = ChannelSoftcVtbl {
    read_reg: sii3114_read_reg,
    write_reg: sii3114_write_reg,
    lba48_write_reg: wdc_default_lba48_write_reg,
    read_raw_multi_2: wdc_default_read_raw_multi_2,
    write_raw_multi_2: wdc_default_write_raw_multi_2,
    read_raw_multi_4: wdc_default_read_raw_multi_4,
    write_raw_multi_4: wdc_default_write_raw_multi_4,
};

/// `(struct pciide_satalink *)sc->sc_cookie` of a SiI3112/3114 controller. Panics if `sc`'s
/// chip is not one of them.
pub fn satalink(sc: &PciideSoftc) -> &PciideSatalink {
    let map = sc.pp().chip_map;
    if !ptr::fn_addr_eq(map, sii3112_chip_map as PciideChipMap)
        && !ptr::fn_addr_eq(map, sii3114_chip_map as PciideChipMap)
    {
        panic(format_args!("{}: not a SiI controller", sc.xname()));
    }
    // SAFETY: `sii3112_chip_map` and `sii3114_chip_map` set `sc_cookie` to a
    // `PciideSatalink` before anything reaches the BA5 helpers, and free it only on detach.
    unsafe { sc.cookie::<PciideSatalink>() }
}

/// `ba5_read_4_ind`.
pub fn ba5_read_4_ind(sc: &PciideSoftc, reg: Pcireg) -> u32 {
    let s = splbio();
    pci_conf_write(sc.pc(), sc.tag(), SII3112_BA5_IND_ADDR, reg);
    let rv = pci_conf_read(sc.pc(), sc.tag(), SII3112_BA5_IND_DATA);
    splx(s);

    rv
}

/// `ba5_read_4`.
pub fn ba5_read_4(sc: &PciideSoftc, reg: BusSize) -> u32 {
    let sl = satalink(sc);

    if sl.ba5_en.get() != 0 {
        return bus_space_read_4(sl.ba5_st(), sl.ba5_sh(), reg);
    }

    ba5_read_4_ind(sc, reg as Pcireg)
}

/// `ba5_write_4_ind`.
pub fn ba5_write_4_ind(sc: &PciideSoftc, reg: Pcireg, val: u32) {
    let s = splbio();
    pci_conf_write(sc.pc(), sc.tag(), SII3112_BA5_IND_ADDR, reg);
    pci_conf_write(sc.pc(), sc.tag(), SII3112_BA5_IND_DATA, val);
    splx(s);
}

/// `ba5_write_4`.
pub fn ba5_write_4(sc: &PciideSoftc, reg: BusSize, val: u32) {
    let sl = satalink(sc);

    if sl.ba5_en.get() != 0 {
        bus_space_write_4(sl.ba5_st(), sl.ba5_sh(), reg, val);
    } else {
        ba5_write_4_ind(sc, reg as Pcireg, val);
    }
}
/* </CODE> */

/* <TESTS> */
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    #[ignore = "needs OPENBSD_SRC (just test-ref)"]
    fn defines_match_the_header() {
        let defs = crate::reftest::defines("sys/dev/pci/pciide_sii3112_reg.h");

        crate::reftest::assert_defines!(defs; SII3112_PCI_CFGCTL, CFGCTL_CFGWREN, CFGCTL_BA5INDEN, SII3112_PCI_SWDATA, SII3112_PCI_BM_IDE0, SII3112_PCI_PRD_IDE0, SII3112_PCI_BM_IDE1, SII3112_PCI_PRD_IDE1, SII3112_DTM_IDE0, SII3112_DTM_IDE1, DTM_IDEx_PIO, DTM_IDEx_DMA, SII3112_SCS_CMD, SCS_CMD_PBM_RESET, SCS_CMD_ARB_RESET, SCS_CMD_FF1_RESET, SCS_CMD_FF0_RESET, SCS_CMD_IDE1_RESET, SCS_CMD_IDE0_RESET, SCS_CMD_FF3_RESET, SCS_CMD_FF2_RESET, SCS_CMD_IDE3_RESET, SCS_CMD_IDE2_RESET, SCS_CMD_BA5_EN, SCS_CMD_M66EN, SCS_CMD_IDE0_INT_BLOCK, SCS_CMD_IDE1_INT_BLOCK, SCS_CMD_IDE2_INT_BLOCK, SCS_CMD_IDE3_INT_BLOCK, SII3112_SSDR, SII3112_FMA_CSR, SII3112_FM_DATA, SII3112_EEA_CSR, SII3112_EE_DATA, SII3112_TCS_IDE0, SII3112_TCS_IDE1, TCS_IDEx_BCA, TCS_IDEx_CH_RESET, TCS_IDEx_VDMA_INT, TCS_IDEx_INT, TCS_IDEx_WTT, TCS_IDEx_WTEN, TCS_IDEx_WTINTEN, SII3112_BA5_IND_ADDR, SII3112_BA5_IND_DATA, IDEDMA_CMD_INT_STEER);
    }
}
/* </TESTS> */
