/*	$OpenBSD: gcu_reg.h,v 1.1 2009/11/25 13:28:13 dms Exp $	*/
/* <LICENSES> */

/*
 *   Copyright(c) 2007,2008,2009 Intel Corporation. All rights reserved.
 *   All rights reserved.
 *
 *   Redistribution and use in source and binary forms, with or without
 *   modification, are permitted provided that the following conditions
 *   are met:
 *
 *     * Redistributions of source code must retain the above copyright
 *       notice, this list of conditions and the following disclaimer.
 *     * Redistributions in binary form must reproduce the above copyright
 *       notice, this list of conditions and the following disclaimer in
 *       the documentation and/or other materials provided with the
 *       distribution.
 *     * Neither the name of Intel Corporation nor the names of its
 *       contributors may be used to endorse or promote products derived
 *       from this software without specific prior written permission.
 *
 *   THIS SOFTWARE IS PROVIDED BY THE COPYRIGHT HOLDERS AND CONTRIBUTORS
 *   "AS IS" AND ANY EXPRESS OR IMPLIED WARRANTIES, INCLUDING, BUT NOT
 *   LIMITED TO, THE IMPLIED WARRANTIES OF MERCHANTABILITY AND FITNESS FOR
 *   A PARTICULAR PURPOSE ARE DISCLAIMED. IN NO EVENT SHALL THE COPYRIGHT
 *   OWNER OR CONTRIBUTORS BE LIABLE FOR ANY DIRECT, INDIRECT, INCIDENTAL,
 *   SPECIAL, EXEMPLARY, OR CONSEQUENTIAL DAMAGES (INCLUDING, BUT NOT
 *   LIMITED TO, PROCUREMENT OF SUBSTITUTE GOODS OR SERVICES; LOSS OF USE,
 *   DATA, OR PROFITS; OR BUSINESS INTERRUPTION) HOWEVER CAUSED AND ON ANY
 *   THEORY OF LIABILITY, WHETHER IN CONTRACT, STRICT LIABILITY, OR TORT
 *   (INCLUDING NEGLIGENCE OR OTHERWISE) ARISING IN ANY WAY OUT OF THE USE
 *   OF THIS SOFTWARE, EVEN IF ADVISED OF THE POSSIBILITY OF SUCH DAMAGE.
 *
 *  version: Embedded.B.1.0.3-146
 */
/* </LICENSES> */

/* <CODE> */
//! The registers of the EP80579 (Tolapai) Global Configuration Unit, the device that owns
//! the MDIO bus of the SoC's integrated em(4) MACs (`<dev/pci/gcu_reg.h>`).
//!
//! Upstream: sys/dev/pci/gcu_reg.h @ 3ce1f3f79392
//!
//! The whole header. Register offsets are `BusSize` (they go to `bus_space_read_4`), fields
//! `u32`. gcu(4) is configured only by i386 GENERIC; `if_em_soc.c` reaches these registers
//! when a GCU attached.
//!
//! ## Deviations
//! - None.

use crate::machine::bus::BusSize;

/// `MDIO_STATUS_REG`.
pub const MDIO_STATUS_REG: BusSize = 0x00000010;
/// `MDIO_COMMAND_REG`.
pub const MDIO_COMMAND_REG: BusSize = 0x00000014;
/// `MDIO_STATUS_STATUS_MASK`: bit 31 = 1 on error.
pub const MDIO_STATUS_STATUS_MASK: u32 = 0x80000000;
/// `MDIO_STATUS_READ_DATA_MASK`.
pub const MDIO_STATUS_READ_DATA_MASK: u32 = 0x0000FFFF;
/// `MDIO_COMMAND_GO_MASK`: bit 31 = 1 during read or write, 0 on completion.
pub const MDIO_COMMAND_GO_MASK: u32 = 0x80000000;
/// `MDIO_COMMAND_OPER_MASK`: bit = 1 is a write.
pub const MDIO_COMMAND_OPER_MASK: u32 = 0x04000000;
/// `MDIO_COMMAND_PHY_ADDR_MASK`.
pub const MDIO_COMMAND_PHY_ADDR_MASK: u32 = 0x03E00000;
/// `MDIO_COMMAND_PHY_REG_MASK`.
pub const MDIO_COMMAND_PHY_REG_MASK: u32 = 0x001F0000;
/// `MDIO_COMMAND_WRITE_DATA_MASK`.
pub const MDIO_COMMAND_WRITE_DATA_MASK: u32 = 0x0000FFFF;
/// `MDIO_COMMAND_GO_OFFSET`.
pub const MDIO_COMMAND_GO_OFFSET: u32 = 31;
/// `MDIO_COMMAND_OPER_OFFSET`.
pub const MDIO_COMMAND_OPER_OFFSET: u32 = 26;
/// `MDIO_COMMAND_PHY_ADDR_OFFSET`.
pub const MDIO_COMMAND_PHY_ADDR_OFFSET: u32 = 21;
/// `MDIO_COMMAND_PHY_REG_OFFSET`.
pub const MDIO_COMMAND_PHY_REG_OFFSET: u32 = 16;
/// `MDIO_COMMAND_WRITE_DATA_OFFSET`.
pub const MDIO_COMMAND_WRITE_DATA_OFFSET: u32 = 0;
/// `MDIO_COMMAND_PHY_ADDR_MAX`: total phys supported by GCU.
pub const MDIO_COMMAND_PHY_ADDR_MAX: u32 = 2;
/// `MDIO_COMMAND_PHY_REG_MAX`: total registers available on the M88 Phy used on truxton.
pub const MDIO_COMMAND_PHY_REG_MAX: u32 = 31;
/* </CODE> */

/* <TESTS> */
#[cfg(test)]
mod tests {
    use super::*;

    /// Every define of the header, against the C (`just test-ref`).
    #[test]
    #[ignore = "needs OPENBSD_SRC (just test-ref)"]
    fn defines_match_reference() {
        let defs = crate::reftest::defines("sys/dev/pci/gcu_reg.h");
        let ours = crate::reftest::assert_defines!(defs;
            MDIO_STATUS_REG, MDIO_COMMAND_REG, MDIO_STATUS_STATUS_MASK, MDIO_STATUS_READ_DATA_MASK, MDIO_COMMAND_GO_MASK, MDIO_COMMAND_OPER_MASK, MDIO_COMMAND_PHY_ADDR_MASK, MDIO_COMMAND_PHY_REG_MASK, MDIO_COMMAND_WRITE_DATA_MASK, MDIO_COMMAND_GO_OFFSET, MDIO_COMMAND_OPER_OFFSET, MDIO_COMMAND_PHY_ADDR_OFFSET, MDIO_COMMAND_PHY_REG_OFFSET, MDIO_COMMAND_WRITE_DATA_OFFSET, MDIO_COMMAND_PHY_ADDR_MAX, MDIO_COMMAND_PHY_REG_MAX);
        crate::reftest::assert_complete(&defs, "MDIO_", &ours);
    }
}
/* </TESTS> */
