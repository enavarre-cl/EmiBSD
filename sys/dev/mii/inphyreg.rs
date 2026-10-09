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
//! Registers of Intel's i82555, i82562EM and i82562ET PHYs (`<dev/mii/inphyreg.h>`), the
//! ones inphy(4) and the 82562 variants program.
//!
//! Upstream: sys/dev/mii/inphyreg.h @ 3ce1f3f79392
//!
//! The whole header. Register numbers are `u32` (the PHY access functions take a register
//! number), register bits `u16` (a PHY register is 16 bits wide).

/// `MII_INPHY_SCR`: Status and Control.
pub const MII_INPHY_SCR: u32 = 0x10;
/// `SCR_FLOWCTL`: PHY Base flow control enabled.
pub const SCR_FLOWCTL: u16 = 0x8000;
/// `SCR_CSDC`: Carrier sense disconnect control.
pub const SCR_CSDC: u16 = 0x2000;
/// `SCR_TFCD`: Transmit flow control disable.
pub const SCR_TFCD: u16 = 0x1000;
/// `SCR_RDSI`: Receive deserializer in-sync.
pub const SCR_RDSI: u16 = 0x0800;
/// `SCR_100TXPD`: 100baseTX is powered down.
pub const SCR_100TXPD: u16 = 0x0400;
/// `SCR_10TPD`: 10baseT is powered down.
pub const SCR_10TPD: u16 = 0x0200;
/// `SCR_POLARITY`: reverse 10baseT polarity.
pub const SCR_POLARITY: u16 = 0x0100;
/// `SCR_T4`: autoneg resulted in 100baseT4.
pub const SCR_T4: u16 = 0x0004;
/// `SCR_S100`: autoneg resulted in 100baseTX.
pub const SCR_S100: u16 = 0x0002;
/// `SCR_FDX`: autoneg resulted in full-duplex.
pub const SCR_FDX: u16 = 0x0001;
/// `SCR_PHYADDR_M`: phy address mask.
pub const SCR_PHYADDR_M: u16 = 0x007c;
/// `SCR_PHYADDR_S`: shift to normalize.
pub const SCR_PHYADDR_S: u32 = 2;
/// `MII_INPHY_SCTRL`: Special Control Bit.
pub const MII_INPHY_SCTRL: u32 = 0x11;
/// `SCTRL_SCRBYPASS`: scrambler bypass.
pub const SCTRL_SCRBYPASS: u16 = 0x8000;
/// `SCTRL_4B5BNYPASS`: 4bit to 5bit bypass.
pub const SCTRL_4B5BNYPASS: u16 = 0x4000;
/// `SCTRL_FTHP`: force transmit H-pattern.
pub const SCTRL_FTHP: u16 = 0x2000;
/// `SCTRL_F34TP`: force 34 transmit pattern.
pub const SCTRL_F34TP: u16 = 0x1000;
/// `SCTRL_GOODLINK`: 100baseTX link good.
pub const SCTRL_GOODLINK: u16 = 0x0800;
/// `SCTRL_TCSD`: transmit carrier sense disable.
pub const SCTRL_TCSD: u16 = 0x0200;
/// `SCTRL_DDPD`: disable dynamic power-down.
pub const SCTRL_DDPD: u16 = 0x0100;
/// `SCTRL_ANEGLOOP`: autonegotiation loopback.
pub const SCTRL_ANEGLOOP: u16 = 0x0080;
/// `SCTRL_MDITRISTATE`: MDI Tri-state.
pub const SCTRL_MDITRISTATE: u16 = 0x0040;
/// `SCTRL_FILTERBYPASS`: Filter bypass.
pub const SCTRL_FILTERBYPASS: u16 = 0x0020;
/// `SCTRL_AUTOPOLDIS`: auto-polarity disable.
pub const SCTRL_AUTOPOLDIS: u16 = 0x0010;
/// `SCTRL_SQUELCHDIS`: squlch test disable.
pub const SCTRL_SQUELCHDIS: u16 = 0x0008;
/// `SCTRL_EXTSQUELCH`: extended sequelch enable.
pub const SCTRL_EXTSQUELCH: u16 = 0x0004;
/// `SCTRL_LINKINTDIS`: link integrity disable.
pub const SCTRL_LINKINTDIS: u16 = 0x0002;
/// `SCTRL_JABBERDIS`: jabber disabled.
pub const SCTRL_JABBERDIS: u16 = 0x0001;
/// `SCTRL_SRE`: symbol error enable.
pub const SCTRL_SRE: u16 = 0x0400;
/// `SCTRL_FORCEPOL`: force polarity, 0 = normal.
pub const SCTRL_FORCEPOL: u16 = 0x0020;
/// `MII_INPHY_PHYADDR`: phy address register, 82562 only.
pub const MII_INPHY_PHYADDR: u32 = 0x12;
/// `MII_INPHY_100TXFCC`: false carrier counter.
pub const MII_INPHY_100TXFCC: u32 = 0x13;
/// `MII_INPHY_100TXRDC`: 100baseTX Receive Disconnect Cntr.
pub const MII_INPHY_100TXRDC: u32 = 0x14;
/// `MII_INPHY_100TXREFC`: 100baseTX Receive Error Frame Ctr.
pub const MII_INPHY_100TXREFC: u32 = 0x15;
/// `MII_INPHY_RSEC`: Receive Symbol Error Counter.
pub const MII_INPHY_RSEC: u32 = 0x16;
/// `MII_INPHY_100TXRPEOFC`: 100baseTX Rcv Premature EOF Ctr.
pub const MII_INPHY_100TXRPEOFC: u32 = 0x17;
/// `MII_INPHY_10TREOFC`: 10baseT Rcv EOF Ctr.
pub const MII_INPHY_10TREOFC: u32 = 0x18;
/// `MII_INPHY_10TTJDC`: 10baseT Tx Jabber Detect Ctr.
pub const MII_INPHY_10TTJDC: u32 = 0x19;
/// `MII_INPHY_SCTRL2`: 82555 Special Control.
pub const MII_INPHY_SCTRL2: u32 = 0x1b;
/// `SCTRL2_LEDMASK`: mask of LEDs control: see below.
pub const SCTRL2_LEDMASK: u16 = 0x0007;
/// `LEDMASK_ACTLINK`: A = Activity, L = Link.
pub const LEDMASK_ACTLINK: u16 = 0x0000;
/// `LEDMASK_SPDCOLL`: A = Speed, L = Collision.
pub const LEDMASK_SPDCOLL: u16 = 0x0001;
/// `LEDMASK_SPDLINK`: A = Speed, L = Link.
pub const LEDMASK_SPDLINK: u16 = 0x0002;
/// `LEDMASK_ACTCOLL`: A = Activity, L = Collision.
pub const LEDMASK_ACTCOLL: u16 = 0x0003;
/// `LEDMASK_OFFOFF`: A = off, L = off.
pub const LEDMASK_OFFOFF: u16 = 0x0004;
/// `LEDMASK_OFFON`: A = off, L = on.
pub const LEDMASK_OFFON: u16 = 0x0005;
/// `LEDMASK_ONOFF`: A = on, L = off.
pub const LEDMASK_ONOFF: u16 = 0x0006;
/// `LESMASK_ONON`: A = on, L = on.
pub const LESMASK_ONON: u16 = 0x0007;
/* </CODE> */

/* <TESTS> */
#[cfg(test)]
mod tests {
    use super::*;

    /// Every define of the header, against the C (`just test-ref`).
    #[test]
    #[ignore = "needs OPENBSD_SRC (just test-ref)"]
    fn defines_match_reference() {
        let defs = crate::reftest::defines("sys/dev/mii/inphyreg.h");
        let ours = crate::reftest::assert_defines!(defs; MII_INPHY_SCR, SCR_FLOWCTL, SCR_CSDC, SCR_TFCD, SCR_RDSI, SCR_100TXPD, SCR_10TPD, SCR_POLARITY, SCR_T4, SCR_S100, SCR_FDX, SCR_PHYADDR_M, SCR_PHYADDR_S, MII_INPHY_SCTRL, SCTRL_SCRBYPASS, SCTRL_4B5BNYPASS, SCTRL_FTHP, SCTRL_F34TP, SCTRL_GOODLINK, SCTRL_TCSD, SCTRL_DDPD, SCTRL_ANEGLOOP, SCTRL_MDITRISTATE, SCTRL_FILTERBYPASS, SCTRL_AUTOPOLDIS, SCTRL_SQUELCHDIS, SCTRL_EXTSQUELCH, SCTRL_LINKINTDIS, SCTRL_JABBERDIS, SCTRL_SRE, SCTRL_FORCEPOL, MII_INPHY_PHYADDR, MII_INPHY_100TXFCC, MII_INPHY_100TXRDC, MII_INPHY_100TXREFC, MII_INPHY_RSEC, MII_INPHY_100TXRPEOFC, MII_INPHY_10TREOFC, MII_INPHY_10TTJDC, MII_INPHY_SCTRL2, SCTRL2_LEDMASK, LEDMASK_ACTLINK, LEDMASK_SPDCOLL, LEDMASK_SPDLINK, LEDMASK_ACTCOLL, LEDMASK_OFFOFF, LEDMASK_OFFON, LEDMASK_ONOFF, LESMASK_ONON);
        crate::reftest::assert_complete(&defs, "MII_INPHY_", &ours);
        crate::reftest::assert_complete(&defs, "SCR_", &ours);
        crate::reftest::assert_complete(&defs, "SCTRL_", &ours);
        crate::reftest::assert_complete(&defs, "SCTRL2_", &ours);
        crate::reftest::assert_complete(&defs, "LEDMASK_", &ours);
        crate::reftest::assert_complete(&defs, "LESMASK_", &ours);
    }
}
/* </TESTS> */
