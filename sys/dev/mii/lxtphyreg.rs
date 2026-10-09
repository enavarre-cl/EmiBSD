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

/*-
 * Copyright (c) 1998 The NetBSD Foundation, Inc.
 * All rights reserved.
 *
 * This code is derived from software contributed to The NetBSD Foundation
 * by Jason R. Thorpe of the Numerical Aerospace Simulation Facility,
 * NASA Ames Research Center.
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
 * THIS SOFTWARE IS PROVIDED BY THE NETBSD FOUNDATION, INC. AND CONTRIBUTORS
 * ``AS IS'' AND ANY EXPRESS OR IMPLIED WARRANTIES, INCLUDING, BUT NOT LIMITED
 * TO, THE IMPLIED WARRANTIES OF MERCHANTABILITY AND FITNESS FOR A PARTICULAR
 * PURPOSE ARE DISCLAIMED.  IN NO EVENT SHALL THE FOUNDATION OR CONTRIBUTORS
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
//! Level One LXT970 PHY registers (`<dev/mii/lxtphyreg.h>`).
//!
//! Upstream: sys/dev/mii/lxtphyreg.h @ 3ce1f3f79392
//!
//! The whole header, for lxtphy(4). Register numbers and bits are `i32`, what `PHY_READ` and
//! `PHY_WRITE` take and return.
//!
//! ## Deviations
//! - None.

/// `MII_LXTPHY_MIRROR`: Mirror register (all bits user-defined).
pub const MII_LXTPHY_MIRROR: i32 = 0x10;

/// `MII_LXTPHY_IER`: Interrupt Enable Register.
pub const MII_LXTPHY_IER: i32 = 0x11;
/// `IER_MIIDRVLVL`: reduced MII driver levels.
pub const IER_MIIDRVLVL: i32 = 0x0008;
/// `IER_LNK_CRITERIA`: enhanced link loss criteria.
pub const IER_LNK_CRITERIA: i32 = 0x0004;
/// `IER_INTEN`: interrupt enable.
pub const IER_INTEN: i32 = 0x0002;
/// `IER_TINT`: force interrupt.
pub const IER_TINT: i32 = 0x0001;

/// `MII_LXTPHY_ISR`: Interrupt Status Register.
pub const MII_LXTPHY_ISR: i32 = 0x12;
/// `ISR_MINT`: MII interrupt pending.
pub const ISR_MINT: i32 = 0x8000;
/// `ISR_XTALOK`: clocks OK.
pub const ISR_XTALOK: i32 = 0x4000;

/// `MII_LXTPHY_CONFIG`: Configuration Register.
pub const MII_LXTPHY_CONFIG: i32 = 0x13;
/// `CONFIG_TXMIT_TEST`: 100base-T transmit test.
pub const CONFIG_TXMIT_TEST: i32 = 0x4000;
/// `CONFIG_REPEATER`: repeater mode.
pub const CONFIG_REPEATER: i32 = 0x2000;
/// `CONFIG_MDIO_INT`: enable interrupt signalling on MDIO.
pub const CONFIG_MDIO_INT: i32 = 0x1000;
/// `CONFIG_TPLOOP`: disable 10base-T loopback.
pub const CONFIG_TPLOOP: i32 = 0x0800;
/// `CONFIG_SQE`: enable SQE.
pub const CONFIG_SQE: i32 = 0x0400;
/// `CONFIG_DISJABBER`: disable jabber.
pub const CONFIG_DISJABBER: i32 = 0x0200;
/// `CONFIG_DISLINKTEST`: disable link test.
pub const CONFIG_DISLINKTEST: i32 = 0x0100;
/// `CONFIG_LEDC1`: LEDC configuration, high bit (with `CONFIG_LEDC0`: 0 0 LEDC indicates
/// collision, 0 1 LEDC is off, 1 0 LEDC indicates activity, 1 1 LEDC is on).
pub const CONFIG_LEDC1: i32 = 0x0080;
/// `CONFIG_LEDC0`: LEDC configuration, low bit.
pub const CONFIG_LEDC0: i32 = 0x0040;
/// `CONFIG_ADVTXCLK`: advance TX clock.
pub const CONFIG_ADVTXCLK: i32 = 0x0020;
/// `CONFIG_5BSYMBOL`: 5-bit symbol mode.
pub const CONFIG_5BSYMBOL: i32 = 0x0010;
/// `CONFIG_SCRAMBLER`: bypass scrambler.
pub const CONFIG_SCRAMBLER: i32 = 0x0008;
/// `CONFIG_100BASEFX`: 100base-FX.
pub const CONFIG_100BASEFX: i32 = 0x0004;
/// `CONFIG_TXDISCON`: disconnect TP transmitter.
pub const CONFIG_TXDISCON: i32 = 0x0001;

/// `MII_LXTPHY_CSR`: Chip Status Register.
pub const MII_LXTPHY_CSR: i32 = 0x14;
/// `CSR_LINK`: link is up.
pub const CSR_LINK: i32 = 0x2000;
/// `CSR_DUPLEX`: full-duplex.
pub const CSR_DUPLEX: i32 = 0x1000;
/// `CSR_SPEED`: 100Mbps.
pub const CSR_SPEED: i32 = 0x0800;
/// `CSR_ACOMP`: autonegotiation complete.
pub const CSR_ACOMP: i32 = 0x0400;
/// `CSR_PAGERCVD`: link page received.
pub const CSR_PAGERCVD: i32 = 0x0200;
/// `CSR_LOWVCC`: low voltage fault.
pub const CSR_LOWVCC: i32 = 0x0004;
/* </CODE> */

/* <TESTS> */
#[cfg(test)]
mod tests {
    use super::*;

    /// Every define of the header, against the C (`just test-ref`).
    #[test]
    #[ignore = "needs OPENBSD_SRC (just test-ref)"]
    fn defines_match_reference() {
        let defs = crate::reftest::defines("sys/dev/mii/lxtphyreg.h");
        crate::reftest::assert_defines!(defs;
            MII_LXTPHY_MIRROR, MII_LXTPHY_IER, IER_MIIDRVLVL, IER_LNK_CRITERIA, IER_INTEN,
            IER_TINT, MII_LXTPHY_ISR, ISR_MINT, ISR_XTALOK, MII_LXTPHY_CONFIG,
            CONFIG_TXMIT_TEST, CONFIG_REPEATER, CONFIG_MDIO_INT, CONFIG_TPLOOP, CONFIG_SQE,
            CONFIG_DISJABBER, CONFIG_DISLINKTEST, CONFIG_LEDC1, CONFIG_LEDC0, CONFIG_ADVTXCLK,
            CONFIG_5BSYMBOL, CONFIG_SCRAMBLER, CONFIG_100BASEFX, CONFIG_TXDISCON,
            MII_LXTPHY_CSR, CSR_LINK, CSR_DUPLEX, CSR_SPEED, CSR_ACOMP, CSR_PAGERCVD,
            CSR_LOWVCC);
    }
}
/* </TESTS> */
