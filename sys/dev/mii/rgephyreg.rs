/*	$OpenBSD: rgephyreg.h,v 1.10 2023/04/05 10:45:07 kettenis Exp $	*/
/* <LICENSES> */
/*
 * Copyright (c) 2003
 *	Bill Paul <wpaul@windriver.com>.  All rights reserved.
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
 *	This product includes software developed by Bill Paul.
 * 4. Neither the name of the author nor the names of any co-contributors
 *    may be used to endorse or promote products derived from this software
 *    without specific prior written permission.
 *
 * THIS SOFTWARE IS PROVIDED BY Bill Paul AND CONTRIBUTORS ``AS IS'' AND
 * ANY EXPRESS OR IMPLIED WARRANTIES, INCLUDING, BUT NOT LIMITED TO, THE
 * IMPLIED WARRANTIES OF MERCHANTABILITY AND FITNESS FOR A PARTICULAR PURPOSE
 * ARE DISCLAIMED.  IN NO EVENT SHALL Bill Paul OR THE VOICES IN HIS HEAD
 * BE LIABLE FOR ANY DIRECT, INDIRECT, INCIDENTAL, SPECIAL, EXEMPLARY, OR
 * CONSEQUENTIAL DAMAGES (INCLUDING, BUT NOT LIMITED TO, PROCUREMENT OF
 * SUBSTITUTE GOODS OR SERVICES; LOSS OF USE, DATA, OR PROFITS; OR BUSINESS
 * INTERRUPTION) HOWEVER CAUSED AND ON ANY THEORY OF LIABILITY, WHETHER IN
 * CONTRACT, STRICT LIABILITY, OR TORT (INCLUDING NEGLIGENCE OR OTHERWISE)
 * ARISING IN ANY WAY OUT OF THE USE OF THIS SOFTWARE, EVEN IF ADVISED OF
 * THE POSSIBILITY OF SUCH DAMAGE.
 *
 * $FreeBSD: rgephyreg.h,v 1.1 2003/09/11 03:53:46 wpaul Exp $
 */
/* </LICENSES> */

//! Realtek RTL8169S/8110S and RTL8211 gigabit PHY registers (`<dev/mii/rgephyreg.h>`).
//!
//! Upstream: sys/dev/mii/rgephyreg.h @ 3ce1f3f79392
//!
//! The whole header. em(4)'s shared code (`if_em_hw.c`) programs the RTL8211 PHY of the
//! EP80579 boards through `RGEPHY_CR` and `RGEPHY_LC`; rgephy(4) itself is not ported.
//! Register numbers are `u32` (the PHY access functions take a `uint32_t` address), register
//! bits `u16` (a PHY register is 16 bits wide).
//!
//! ## Deviations
//! - `RGEPHY_SR_SPEED(X)` and `RGEPHY_F_SR_SPEED(X)` are the const fns `rgephy_sr_speed` and
//!   `rgephy_f_sr_speed`.

/// `RGEPHY_8211B`.
pub const RGEPHY_8211B: u32 = 2;
/// `RGEPHY_8211C`.
pub const RGEPHY_8211C: u32 = 3;
/// `RGEPHY_8211F`.
pub const RGEPHY_8211F: u32 = 6;
/// `RGEPHY_CR`: PHY Specific Control.
pub const RGEPHY_CR: u32 = 0x10;
/// `RGEPHY_CR_ASSERT_CRS`.
pub const RGEPHY_CR_ASSERT_CRS: u16 = 0x0800;
/// `RGEPHY_CR_FORCE_LINK`.
pub const RGEPHY_CR_FORCE_LINK: u16 = 0x0400;
/// `RGEPHY_CR_MDI_MASK`.
pub const RGEPHY_CR_MDI_MASK: u16 = 0x0060;
/// `RGEPHY_CR_MDIX_AUTO`.
pub const RGEPHY_CR_MDIX_AUTO: u16 = 0x0040;
/// `RGEPHY_CR_MDIX_MANUAL`.
pub const RGEPHY_CR_MDIX_MANUAL: u16 = 0x0020;
/// `RGEPHY_CR_MDI_MANUAL`.
pub const RGEPHY_CR_MDI_MANUAL: u16 = 0x0000;
/// `RGEPHY_CR_CLK125_DIS`.
pub const RGEPHY_CR_CLK125_DIS: u16 = 0x0010;
/// `RGEPHY_CR_ALDPS`: RTL8251 only.
pub const RGEPHY_CR_ALDPS: u16 = 0x0004;
/// `RGEPHY_CR_JABBER_DIS`.
pub const RGEPHY_CR_JABBER_DIS: u16 = 0x0001;
/// `RGEPHY_SR`: PHY Specific Status.
pub const RGEPHY_SR: u32 = 0x11;
/// `RGEPHY_SR_SPEED_1000MBPS`.
pub const RGEPHY_SR_SPEED_1000MBPS: u16 = 0x8000;
/// `RGEPHY_SR_SPEED_100MBPS`.
pub const RGEPHY_SR_SPEED_100MBPS: u16 = 0x4000;
/// `RGEPHY_SR_SPEED_10MBPS`.
pub const RGEPHY_SR_SPEED_10MBPS: u16 = 0x0000;
/// `RGEPHY_SR_SPEED_MASK`.
pub const RGEPHY_SR_SPEED_MASK: u16 = 0xc000;
/// `RGEPHY_SR_FDX`: full duplex.
pub const RGEPHY_SR_FDX: u16 = 0x2000;
/// `RGEPHY_SR_PAGE_RECEIVED`: new page received.
pub const RGEPHY_SR_PAGE_RECEIVED: u16 = 0x1000;
/// `RGEPHY_SR_SPD_DPLX_RESOLVED`: speed/duplex resolved.
pub const RGEPHY_SR_SPD_DPLX_RESOLVED: u16 = 0x0800;
/// `RGEPHY_SR_LINK`: link up.
pub const RGEPHY_SR_LINK: u16 = 0x0400;
/// `RGEPHY_SR_MDI_XOVER`: MDI crossover.
pub const RGEPHY_SR_MDI_XOVER: u16 = 0x0040;
/// `RGEPHY_SR_ALDPS`: RTL8211C(L) only.
pub const RGEPHY_SR_ALDPS: u16 = 0x0008;
/// `RGEPHY_SR_JABBER`: Jabber.
pub const RGEPHY_SR_JABBER: u16 = 0x0001;
/// `RGEPHY_SR_SPEED(X)`: the speed bits of the PHY specific status.
pub const fn rgephy_sr_speed(x: u16) -> u16 {
    x & RGEPHY_SR_SPEED_MASK
}
/// `RGEPHY_F_SR`: PHY Specific Status.
pub const RGEPHY_F_SR: u32 = 0x1A;
/// `RGEPHY_F_SR_SPEED_1000MBPS`.
pub const RGEPHY_F_SR_SPEED_1000MBPS: u16 = 0x0020;
/// `RGEPHY_F_SR_SPEED_100MBPS`.
pub const RGEPHY_F_SR_SPEED_100MBPS: u16 = 0x0010;
/// `RGEPHY_F_SR_SPEED_10MBPS`.
pub const RGEPHY_F_SR_SPEED_10MBPS: u16 = 0x0000;
/// `RGEPHY_F_SR_SPEED_MASK`.
pub const RGEPHY_F_SR_SPEED_MASK: u16 = 0x0030;
/// `RGEPHY_F_SR_FDX`.
pub const RGEPHY_F_SR_FDX: u16 = 0x0008;
/// `RGEPHY_F_SR_LINK`.
pub const RGEPHY_F_SR_LINK: u16 = 0x0004;
/// `RGEPHY_F_SR_SPEED(X)`: the speed bits of the RTL8211F status.
pub const fn rgephy_f_sr_speed(x: u16) -> u16 {
    x & RGEPHY_F_SR_SPEED_MASK
}
/// `RGEPHY_LC`: PHY LED Control Register.
pub const RGEPHY_LC: u32 = 0x18;
/// `RGEPHY_LC_P2`: PHY LED Control Register, Page 2.
pub const RGEPHY_LC_P2: u32 = 0x1A;
/// `RGEPHY_LC_DISABLE`: disable leds.
pub const RGEPHY_LC_DISABLE: u16 = 0x8000;
/// `RGEPHY_LC_PULSE_1_3S`.
pub const RGEPHY_LC_PULSE_1_3S: u16 = 0x7000;
/// `RGEPHY_LC_PULSE_670MS`.
pub const RGEPHY_LC_PULSE_670MS: u16 = 0x6000;
/// `RGEPHY_LC_PULSE_340MS`.
pub const RGEPHY_LC_PULSE_340MS: u16 = 0x5000;
/// `RGEPHY_LC_PULSE_170MS`.
pub const RGEPHY_LC_PULSE_170MS: u16 = 0x4000;
/// `RGEPHY_LC_PULSE_84MS`.
pub const RGEPHY_LC_PULSE_84MS: u16 = 0x3000;
/// `RGEPHY_LC_PULSE_42MS`.
pub const RGEPHY_LC_PULSE_42MS: u16 = 0x2000;
/// `RGEPHY_LC_PULSE_21MS`.
pub const RGEPHY_LC_PULSE_21MS: u16 = 0x1000;
/// `RGEPHY_LC_PULSE_0MS`.
pub const RGEPHY_LC_PULSE_0MS: u16 = 0x0000;
/// `RGEPHY_LC_LINK`: Link and speed indicated by combination of leds.
pub const RGEPHY_LC_LINK: u16 = 0x0008;
/// `RGEPHY_LC_DUPLEX`.
pub const RGEPHY_LC_DUPLEX: u16 = 0x0004;
/// `RGEPHY_LC_RX`.
pub const RGEPHY_LC_RX: u16 = 0x0002;
/// `RGEPHY_LC_TX`.
pub const RGEPHY_LC_TX: u16 = 0x0001;
/// `RGEPHY_PS`: Page Select Register.
pub const RGEPHY_PS: u32 = 0x1F;
/// `RGEPHY_PS_PAGE_0`.
pub const RGEPHY_PS_PAGE_0: u16 = 0x0000;
/// `RGEPHY_PS_PAGE_1`.
pub const RGEPHY_PS_PAGE_1: u16 = 0x0001;
/// `RGEPHY_PS_PAGE_2`.
pub const RGEPHY_PS_PAGE_2: u16 = 0x0002;
/// `RGEPHY_PS_PAGE_3`.
pub const RGEPHY_PS_PAGE_3: u16 = 0x0003;
/// `RGEPHY_PS_PAGE_4`.
pub const RGEPHY_PS_PAGE_4: u16 = 0x0004;
/// `RGEPHY_PS_PAGE_MII`.
pub const RGEPHY_PS_PAGE_MII: u16 = 0x0d08;
/// `RGEPHY_MIICR1`.
pub const RGEPHY_MIICR1: u32 = 0x11;
/// `RGEPHY_MIICR1_TXDLY_EN`.
pub const RGEPHY_MIICR1_TXDLY_EN: u16 = 0x0100;
/// `RGEPHY_MIICR2`.
pub const RGEPHY_MIICR2: u32 = 0x15;
/// `RGEPHY_MIICR2_RXDLY_EN`.
pub const RGEPHY_MIICR2_RXDLY_EN: u16 = 0x0008;

#[cfg(test)]
mod tests {
    use super::*;

    /// Every define of the header, against the C (`just test-ref`).
    #[test]
    #[ignore = "needs OPENBSD_SRC (just test-ref)"]
    fn defines_match_reference() {
        let defs = crate::reftest::defines("sys/dev/mii/rgephyreg.h");
        let ours = crate::reftest::assert_defines!(defs;
            RGEPHY_8211B, RGEPHY_8211C, RGEPHY_8211F, RGEPHY_CR, RGEPHY_CR_ASSERT_CRS, RGEPHY_CR_FORCE_LINK, RGEPHY_CR_MDI_MASK, RGEPHY_CR_MDIX_AUTO, RGEPHY_CR_MDIX_MANUAL, RGEPHY_CR_MDI_MANUAL, RGEPHY_CR_CLK125_DIS, RGEPHY_CR_ALDPS, RGEPHY_CR_JABBER_DIS, RGEPHY_SR, RGEPHY_SR_SPEED_1000MBPS, RGEPHY_SR_SPEED_100MBPS, RGEPHY_SR_SPEED_10MBPS, RGEPHY_SR_SPEED_MASK, RGEPHY_SR_FDX, RGEPHY_SR_PAGE_RECEIVED, RGEPHY_SR_SPD_DPLX_RESOLVED, RGEPHY_SR_LINK, RGEPHY_SR_MDI_XOVER, RGEPHY_SR_ALDPS, RGEPHY_SR_JABBER, RGEPHY_F_SR, RGEPHY_F_SR_SPEED_1000MBPS, RGEPHY_F_SR_SPEED_100MBPS, RGEPHY_F_SR_SPEED_10MBPS, RGEPHY_F_SR_SPEED_MASK, RGEPHY_F_SR_FDX, RGEPHY_F_SR_LINK, RGEPHY_LC, RGEPHY_LC_P2, RGEPHY_LC_DISABLE, RGEPHY_LC_PULSE_1_3S, RGEPHY_LC_PULSE_670MS, RGEPHY_LC_PULSE_340MS, RGEPHY_LC_PULSE_170MS, RGEPHY_LC_PULSE_84MS, RGEPHY_LC_PULSE_42MS, RGEPHY_LC_PULSE_21MS, RGEPHY_LC_PULSE_0MS, RGEPHY_LC_LINK, RGEPHY_LC_DUPLEX, RGEPHY_LC_RX, RGEPHY_LC_TX, RGEPHY_PS, RGEPHY_PS_PAGE_0, RGEPHY_PS_PAGE_1, RGEPHY_PS_PAGE_2, RGEPHY_PS_PAGE_3, RGEPHY_PS_PAGE_4, RGEPHY_PS_PAGE_MII, RGEPHY_MIICR1, RGEPHY_MIICR1_TXDLY_EN, RGEPHY_MIICR2, RGEPHY_MIICR2_RXDLY_EN);
        crate::reftest::assert_complete(&defs, "RGEPHY_", &ours);
    }

    #[test]
    fn speed_macros() {
        assert_eq!(rgephy_sr_speed(0xffff), RGEPHY_SR_SPEED_MASK);
        assert_eq!(
            rgephy_f_sr_speed(RGEPHY_F_SR_SPEED_100MBPS | 1),
            RGEPHY_F_SR_SPEED_100MBPS
        );
    }
}
