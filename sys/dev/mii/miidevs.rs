/*	$OpenBSD: miidevs.h,v 1.139 2025/05/09 13:51:21 jcs Exp $	*/

/*
 * THIS FILE AUTOMATICALLY GENERATED.  DO NOT EDIT.
 *
 * generated from:
 *	OpenBSD: miidevs,v 1.135 2025/05/09 13:51:03 jcs Exp
 */
/* $NetBSD: miidevs,v 1.3 1998/11/05 03:43:43 thorpej Exp $ */
/* <LICENSES> */
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
//! `<dev/mii/miidevs.h>`: the MII OUIs and PHY models, the subset the ported PHY drivers
//! name (rlphy(4), rgephy(4)).
//!
//! Upstream: sys/dev/mii/miidevs.h @ 3ce1f3f79392
//!
//! ## Deviations
//! - Partial: only the ids the ported PHY drivers match; the file is generated from
//!   `miidevs` and grows with the drivers. OUIs and models are `u32` (`struct mii_phydesc`'s
//!   `u_int32_t`), the names `&str`.

#![allow(non_upper_case_globals)] // the C's `xx` prefixes (bit-reversed OUIs), verbatim

/// `MII_OUI_REALTEK`: Realtek.
pub const MII_OUI_REALTEK: u32 = 0x000020;
/// `MII_OUI_ICPLUS`: IC Plus.
pub const MII_OUI_ICPLUS: u32 = 0x0090c3;
/// `MII_OUI_REALTEK2`: Realtek.
pub const MII_OUI_REALTEK2: u32 = 0x00e04c;
/// `MII_OUI_xxREALTEK`: Realtek.
pub const MII_OUI_xxREALTEK: u32 = 0x000732;

/// `MII_MODEL_ICPLUS_IP101`.
pub const MII_MODEL_ICPLUS_IP101: u32 = 0x0005;
/// `MII_STR_ICPLUS_IP101`.
pub const MII_STR_ICPLUS_IP101: &str = "IP101";
/// `MII_MODEL_xxREALTEK_RTL8251`.
pub const MII_MODEL_xxREALTEK_RTL8251: u32 = 0x0000;
/// `MII_STR_xxREALTEK_RTL8251`.
pub const MII_STR_xxREALTEK_RTL8251: &str = "RTL8251";
/// `MII_MODEL_xxREALTEK_RTL8201F`.
pub const MII_MODEL_xxREALTEK_RTL8201F: u32 = 0x0001;
/// `MII_STR_xxREALTEK_RTL8201F`.
pub const MII_STR_xxREALTEK_RTL8201F: &str = "RTL8201F";
/// `MII_MODEL_xxREALTEK_RTL8211FVD`.
pub const MII_MODEL_xxREALTEK_RTL8211FVD: u32 = 0x0007;
/// `MII_STR_xxREALTEK_RTL8211FVD`.
pub const MII_STR_xxREALTEK_RTL8211FVD: &str = "RTL8211F-VD";
/// `MII_MODEL_xxREALTEK_RTL8201E`.
pub const MII_MODEL_xxREALTEK_RTL8201E: u32 = 0x0008;
/// `MII_STR_xxREALTEK_RTL8201E`.
pub const MII_STR_xxREALTEK_RTL8201E: &str = "RTL8201E";
/// `MII_MODEL_xxREALTEK_RTL8169S`.
pub const MII_MODEL_xxREALTEK_RTL8169S: u32 = 0x0011;
/// `MII_STR_xxREALTEK_RTL8169S`.
pub const MII_STR_xxREALTEK_RTL8169S: &str = "RTL8169S/8110S/8211";
/// `MII_MODEL_REALTEK_RTL8201L`.
pub const MII_MODEL_REALTEK_RTL8201L: u32 = 0x0020;
/// `MII_STR_REALTEK_RTL8201L`.
pub const MII_STR_REALTEK_RTL8201L: &str = "RTL8201L";
/* </CODE> */

/* <TESTS> */
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    #[ignore = "needs OPENBSD_SRC (just test-ref)"]
    fn values_match_the_c_header() {
        let defs = crate::reftest::defines("sys/dev/mii/miidevs.h");
        crate::reftest::assert_defines!(defs; MII_OUI_REALTEK, MII_OUI_ICPLUS, MII_OUI_REALTEK2,
            MII_OUI_xxREALTEK, MII_MODEL_ICPLUS_IP101, MII_MODEL_xxREALTEK_RTL8251,
            MII_MODEL_xxREALTEK_RTL8201F, MII_MODEL_xxREALTEK_RTL8211FVD,
            MII_MODEL_xxREALTEK_RTL8201E, MII_MODEL_xxREALTEK_RTL8169S,
            MII_MODEL_REALTEK_RTL8201L);
        for (name, s) in [
            ("MII_STR_ICPLUS_IP101", MII_STR_ICPLUS_IP101),
            ("MII_STR_xxREALTEK_RTL8251", MII_STR_xxREALTEK_RTL8251),
            ("MII_STR_xxREALTEK_RTL8201F", MII_STR_xxREALTEK_RTL8201F),
            ("MII_STR_xxREALTEK_RTL8211FVD", MII_STR_xxREALTEK_RTL8211FVD),
            ("MII_STR_xxREALTEK_RTL8201E", MII_STR_xxREALTEK_RTL8201E),
            ("MII_STR_xxREALTEK_RTL8169S", MII_STR_xxREALTEK_RTL8169S),
            ("MII_STR_REALTEK_RTL8201L", MII_STR_REALTEK_RTL8201L),
        ] {
            assert_eq!(defs[name], std::format!("\"{s}\""), "{name}");
        }
    }
}
/* </TESTS> */
