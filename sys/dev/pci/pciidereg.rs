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
 * Copyright (c) 1998 Christopher G. Demetriou.  All rights reserved.
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
 *      This product includes software developed by Christopher G. Demetriou
 *	for the NetBSD Project.
 * 4. The name of the author may not be used to endorse or promote products
 *    derived from this software without specific prior written permission
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
 */
/* </LICENSES> */

/* <CODE> */
//! PCI IDE controller register definitions: the channel layout of the PCI configuration
//! space, compatibility-mode addresses, and the bus master DMA registers and descriptor.
//!
//! Upstream: sys/dev/pci/pciidereg.h @ 3ce1f3f79392
//!
//! See "PCI IDE Controller Specification, Revision 1.0 3/4/94" from the PCI SIG.
//!
//! ## Deviations
//! - The function-like macros are `const fn`s with the macros' names in lower case; their
//!   types follow their use: configuration-space offsets `i32` (`pci_conf_read`'s `reg`),
//!   compatibility addresses `BusAddr`, bus space offsets and sizes `BusSize`, the
//!   interface and DMA register bits `u32` or `u8` (the width of the register they fill).
//! - `PCIIDE_CHANNEL_NAME` returns a `&'static str`.

use crate::machine::bus::{BusAddr, BusSize};
use crate::sys::param::{MAXPHYS, PAGE_SIZE};

// Definitions for PCI IDE controllers.

// Channel number definitions (as above).
/// `PCIIDE_NUM_CHANNELS`.
pub const PCIIDE_NUM_CHANNELS: i32 = 2;

// Register offsets in PCI configuration space.
/// `PCIIDE_REG_CMD_BASE(chan)`.
pub const fn pciide_reg_cmd_base(chan: i32) -> i32 {
    0x10 + 8 * chan
}
/// `PCIIDE_REG_CTL_BASE(chan)`.
pub const fn pciide_reg_ctl_base(chan: i32) -> i32 {
    0x14 + 8 * chan
}
/// `PCIIDE_REG_BUS_MASTER_DMA`.
pub const PCIIDE_REG_BUS_MASTER_DMA: i32 = 0x20;

// Bits in the PCI Programming Interface register (some are per-channel). Bits 6-4 are
// defined as read-only in PCI 2.1 specification. Microsoft proposed to use these bits for
// independent channels enable/disable. This feature is enabled based on the value of bit 6.
/// `PCIIDE_CHANSTATUS_EN`.
pub const PCIIDE_CHANSTATUS_EN: u32 = 0x40;
/// `PCIIDE_CHAN_EN(chan)`.
pub const fn pciide_chan_en(chan: i32) -> u32 {
    0x20 >> chan
}
/// `PCIIDE_INTERFACE_PCI(chan)`.
pub const fn pciide_interface_pci(chan: i32) -> u32 {
    0x01 << (2 * chan)
}
/// `PCIIDE_INTERFACE_SETTABLE(chan)`.
pub const fn pciide_interface_settable(chan: i32) -> u32 {
    0x02 << (2 * chan)
}
/// `PCIIDE_INTERFACE_BUS_MASTER_DMA`.
pub const PCIIDE_INTERFACE_BUS_MASTER_DMA: u32 = 0x80;

// Compatibility address/IRQ definitions. Should be elsewhere; they're not specific to PCI
// IDE controllers, but rather are part of the (ISA) PC architecture.
/// `PCIIDE_COMPAT_CMD_BASE(chan)`.
pub const fn pciide_compat_cmd_base(chan: i32) -> BusAddr {
    if chan == 0 { 0x1f0 } else { 0x170 }
}
/// `PCIIDE_COMPAT_CMD_SIZE`.
pub const PCIIDE_COMPAT_CMD_SIZE: BusSize = 8;
/// `PCIIDE_COMPAT_CTL_BASE(chan)`.
pub const fn pciide_compat_ctl_base(chan: i32) -> BusAddr {
    if chan == 0 { 0x3f6 } else { 0x376 }
}
/// `PCIIDE_COMPAT_CTL_SIZE`.
pub const PCIIDE_COMPAT_CTL_SIZE: BusSize = 1;
/// `PCIIDE_COMPAT_IRQ(chan)`.
pub const fn pciide_compat_irq(chan: i32) -> i32 {
    if chan == 0 { 14 } else { 15 }
}

/// `PCIIDE_CHANNEL_NAME(chan)`.
pub const fn pciide_channel_name(chan: i32) -> &'static str {
    if chan == 0 { "channel 0" } else { "channel 1" }
}

// definitions for IDE DMA
// XXX maybe this should go elsewhere

/// `IDEDMA_SCH_OFFSET`: secondary channel registers offset.
pub const IDEDMA_SCH_OFFSET: BusSize = 0x08;
/// `IDEDMA_NREGS`.
pub const IDEDMA_NREGS: usize = 8;

// Bus master command register (per channel)
/// `IDEDMA_CMD(chan)`.
pub const fn idedma_cmd(chan: i32) -> BusSize {
    IDEDMA_SCH_OFFSET * chan as BusSize
}
/// `IDEDMA_CMD_WRITE`.
pub const IDEDMA_CMD_WRITE: u8 = 0x08;
/// `IDEDMA_CMD_START`.
pub const IDEDMA_CMD_START: u8 = 0x01;

// Bus master status register (per channel)
/// `IDEDMA_CTL(chan)`.
pub const fn idedma_ctl(chan: i32) -> BusSize {
    0x02 + IDEDMA_SCH_OFFSET * chan as BusSize
}
/// `IDEDMA_CTL_DRV_DMA(d)`.
pub const fn idedma_ctl_drv_dma(d: i32) -> u8 {
    0x20 << d
}
/// `IDEDMA_CTL_INTR`.
pub const IDEDMA_CTL_INTR: u8 = 0x04;
/// `IDEDMA_CTL_ERR`.
pub const IDEDMA_CTL_ERR: u8 = 0x02;
/// `IDEDMA_CTL_ACT`.
pub const IDEDMA_CTL_ACT: u8 = 0x01;

// Bus master table pointer register (per channel)
/// `IDEDMA_TBL(chan)`.
pub const fn idedma_tbl(chan: i32) -> BusSize {
    0x04 + IDEDMA_SCH_OFFSET * chan as BusSize
}
/// `IDEDMA_TBL_MASK`.
pub const IDEDMA_TBL_MASK: u32 = 0xfffffffc;
/// `IDEDMA_TBL_ALIGN`.
pub const IDEDMA_TBL_ALIGN: BusSize = 0x00010000;

/// `IDEDMA_BYTE_COUNT_MASK`.
pub const IDEDMA_BYTE_COUNT_MASK: u32 = 0x0000FFFF;
/// `IDEDMA_BYTE_COUNT_EOT`.
pub const IDEDMA_BYTE_COUNT_EOT: u32 = 0x80000000;

/// `IDEDMA_BYTE_COUNT_MAX`: Max I/O per table.
pub const IDEDMA_BYTE_COUNT_MAX: BusSize = 0x00010000;
/// `IDEDMA_BYTE_COUNT_ALIGN`.
pub const IDEDMA_BYTE_COUNT_ALIGN: BusSize = 0x00010000;

/// `NIDEDMA_TABLES`: Number of idedma table needed.
pub const NIDEDMA_TABLES: usize = MAXPHYS / PAGE_SIZE + 1;

// Intel SCH
/// `SCH_D0TIM`.
pub const SCH_D0TIM: i32 = 0x80;
/// `SCH_D1TIM`.
pub const SCH_D1TIM: i32 = 0x84;
/// `SCH_TIM_UDMA`.
pub const SCH_TIM_UDMA: u32 = 0x70000;
/// `SCH_TIM_MDMA`.
pub const SCH_TIM_MDMA: u32 = 0x00300;
/// `SCH_TIM_PIO`.
pub const SCH_TIM_PIO: u32 = 0x00007;
/// `SCH_TIM_SYNCDMA`.
pub const SCH_TIM_SYNCDMA: u32 = 1 << 31;

/// `SCH_TIM_MASK`.
pub const SCH_TIM_MASK: u32 = SCH_TIM_UDMA | SCH_TIM_MDMA | SCH_TIM_PIO;

/// `struct idedma_table`: bus master table descriptor.
#[repr(C)]
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct IdedmaTable {
    /// `base_addr`: physical base addr of memory region (little-endian).
    pub base_addr: u32,
    /// `byte_count`: memory region length (little-endian).
    pub byte_count: u32,
}

const _: () = assert!(size_of::<IdedmaTable>() == 8);
/* </CODE> */

/* <TESTS> */
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn channel_macros() {
        assert_eq!(pciide_reg_cmd_base(1), 0x18);
        assert_eq!(pciide_reg_ctl_base(0), 0x14);
        assert_eq!(pciide_interface_pci(1), 0x04);
        assert_eq!(pciide_interface_settable(1), 0x08);
        assert_eq!(pciide_chan_en(1), 0x10);
        assert_eq!(pciide_compat_cmd_base(1), 0x170);
        assert_eq!(pciide_compat_ctl_base(0), 0x3f6);
        assert_eq!(pciide_compat_irq(1), 15);
        assert_eq!(idedma_cmd(1), 0x08);
        assert_eq!(idedma_ctl(1), 0x0a);
        assert_eq!(idedma_tbl(1), 0x0c);
        assert_eq!(idedma_ctl_drv_dma(1), 0x40);
        assert_eq!(pciide_channel_name(1), "channel 1");
    }

    #[test]
    #[ignore = "needs OPENBSD_SRC (just test-ref)"]
    fn defines_match_the_header() {
        let defs = crate::reftest::defines("sys/dev/pci/pciidereg.h");
        for (name, value) in [
            ("PCIIDE_NUM_CHANNELS", PCIIDE_NUM_CHANNELS as i64),
            (
                "PCIIDE_REG_BUS_MASTER_DMA",
                PCIIDE_REG_BUS_MASTER_DMA as i64,
            ),
            ("PCIIDE_CHANSTATUS_EN", PCIIDE_CHANSTATUS_EN as i64),
            (
                "PCIIDE_INTERFACE_BUS_MASTER_DMA",
                PCIIDE_INTERFACE_BUS_MASTER_DMA as i64,
            ),
            ("PCIIDE_COMPAT_CMD_SIZE", PCIIDE_COMPAT_CMD_SIZE as i64),
            ("PCIIDE_COMPAT_CTL_SIZE", PCIIDE_COMPAT_CTL_SIZE as i64),
            ("IDEDMA_SCH_OFFSET", IDEDMA_SCH_OFFSET as i64),
            ("IDEDMA_NREGS", IDEDMA_NREGS as i64),
            ("IDEDMA_CMD_WRITE", IDEDMA_CMD_WRITE as i64),
            ("IDEDMA_CMD_START", IDEDMA_CMD_START as i64),
            ("IDEDMA_CTL_INTR", IDEDMA_CTL_INTR as i64),
            ("IDEDMA_CTL_ERR", IDEDMA_CTL_ERR as i64),
            ("IDEDMA_CTL_ACT", IDEDMA_CTL_ACT as i64),
            ("IDEDMA_TBL_MASK", IDEDMA_TBL_MASK as i64),
            ("IDEDMA_TBL_ALIGN", IDEDMA_TBL_ALIGN as i64),
            ("IDEDMA_BYTE_COUNT_MASK", IDEDMA_BYTE_COUNT_MASK as i64),
            ("IDEDMA_BYTE_COUNT_EOT", IDEDMA_BYTE_COUNT_EOT as i64),
            ("IDEDMA_BYTE_COUNT_MAX", IDEDMA_BYTE_COUNT_MAX as i64),
            ("IDEDMA_BYTE_COUNT_ALIGN", IDEDMA_BYTE_COUNT_ALIGN as i64),
            ("SCH_D0TIM", SCH_D0TIM as i64),
            ("SCH_D1TIM", SCH_D1TIM as i64),
            ("SCH_TIM_UDMA", SCH_TIM_UDMA as i64),
            ("SCH_TIM_MDMA", SCH_TIM_MDMA as i64),
            ("SCH_TIM_PIO", SCH_TIM_PIO as i64),
        ] {
            assert_eq!(crate::reftest::int(&defs, name), Some(value), "{name}");
        }
    }
}
/* </TESTS> */
