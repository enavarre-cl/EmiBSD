/*	$OpenBSD: ukphy_subr.c,v 1.10 2008/10/24 16:50:01 brad Exp $	*/
/*	$NetBSD: ukphy_subr.c,v 1.2 1998/11/05 04:08:02 thorpej Exp $	*/
/* <LICENSES> */
/*-
 * Copyright (c) 1998 The NetBSD Foundation, Inc.
 * All rights reserved.
 *
 * This code is derived from software contributed to The NetBSD Foundation
 * by Jason R. Thorpe of the Numerical Aerospace Simulation Facility,
 * NASA Ames Research Center, and by Frank van der Linden.
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
//! `dev/mii/ukphy_subr.c`: subroutines shared by the ukphy driver and other PHY drivers.
//!
//! Upstream: sys/dev/mii/ukphy_subr.c @ 3ce1f3f79392
//!
//! ## Deviations
//! - None beyond reading the selected media through `MiiData::cur_media`.

use crate::dev::mii::mii::{
    ANLPAR_10, ANLPAR_10_FD, ANLPAR_T4, ANLPAR_TX, ANLPAR_TX_FD, BMCR_AUTOEN, BMCR_ISO, BMCR_LOOP,
    BMSR_ACOMP, BMSR_LINK, EXTSR_1000TFDX, EXTSR_1000THDX, GTCR_ADV_1000TFDX, GTCR_ADV_1000THDX,
    GTSR_LP_1000TFDX, GTSR_LP_1000THDX, GTSR_MS_RES, MII_100T2CR, MII_100T2SR, MII_ANAR,
    MII_ANLPAR, MII_BMCR, MII_BMSR,
};
use crate::dev::mii::mii_physubr::mii_phy_flowstatus;
use crate::dev::mii::miivar::{MIIF_HAVE_GTCR, MiiSoftc};
use crate::net::if_media::{
    IFM_10_T, IFM_100_T4, IFM_100_TX, IFM_1000_T, IFM_ACTIVE, IFM_AVALID, IFM_ETH_MASTER,
    IFM_ETHER, IFM_FDX, IFM_HDX, IFM_LOOP, IFM_NONE, ifm_subtype,
};

/// `ukphy_status`: media status subroutine. If a PHY driver does media detection simply by
/// decoding the NWay autonegotiation, use this routine.
pub fn ukphy_status(phy: &'static MiiSoftc) {
    let mii = phy.pdata();
    let mut status = IFM_AVALID;
    let mut active = IFM_ETHER;

    let bmsr = phy.phy_read(MII_BMSR) | phy.phy_read(MII_BMSR);
    if bmsr & BMSR_LINK != 0 {
        status |= IFM_ACTIVE;
    }

    let bmcr = phy.phy_read(MII_BMCR);
    if bmcr & BMCR_ISO != 0 {
        mii.mii_media_active.set(active | IFM_NONE);
        mii.mii_media_status.set(0);
        return;
    }

    if bmcr & BMCR_LOOP != 0 {
        active |= IFM_LOOP;
    }

    if bmcr & BMCR_AUTOEN != 0 {
        // NWay autonegotiation takes the highest-order common bit of the ANAR and ANLPAR
        // (i.e. best media advertised both by us and our link partner).
        if bmsr & BMSR_ACOMP == 0 {
            // Erg, still trying, I guess...
            mii.mii_media_status.set(status);
            mii.mii_media_active.set(active | IFM_NONE);
            return;
        }

        let anlpar = phy.phy_read(MII_ANAR) & phy.phy_read(MII_ANLPAR);
        let (gtcr, gtsr) = if phy.mii_flags.get() & MIIF_HAVE_GTCR != 0
            && phy.mii_extcapabilities.get() & (EXTSR_1000THDX | EXTSR_1000TFDX) != 0
        {
            (phy.phy_read(MII_100T2CR), phy.phy_read(MII_100T2SR))
        } else {
            (0, 0)
        };

        if gtcr & GTCR_ADV_1000TFDX != 0 && gtsr & GTSR_LP_1000TFDX != 0 {
            active |= IFM_1000_T | IFM_FDX;
        } else if gtcr & GTCR_ADV_1000THDX != 0 && gtsr & GTSR_LP_1000THDX != 0 {
            active |= IFM_1000_T | IFM_HDX;
        } else if anlpar & ANLPAR_TX_FD != 0 {
            active |= IFM_100_TX | IFM_FDX;
        } else if anlpar & ANLPAR_T4 != 0 {
            active |= IFM_100_T4 | IFM_HDX;
        } else if anlpar & ANLPAR_TX != 0 {
            active |= IFM_100_TX | IFM_HDX;
        } else if anlpar & ANLPAR_10_FD != 0 {
            active |= IFM_10_T | IFM_FDX;
        } else if anlpar & ANLPAR_10 != 0 {
            active |= IFM_10_T | IFM_HDX;
        } else {
            active |= IFM_NONE;
        }

        // mii_phy_flowstatus reads the PHY, not the media words: publish them first, as
        // the C's in-place updates have.
        mii.mii_media_status.set(status);
        mii.mii_media_active.set(active);
        if active & IFM_FDX != 0 {
            active |= mii_phy_flowstatus(phy);
        }

        if ifm_subtype(active) == IFM_1000_T && gtsr & GTSR_MS_RES != 0 {
            active |= IFM_ETH_MASTER;
        }
        mii.mii_media_active.set(active);
    } else {
        mii.mii_media_status.set(status);
        mii.mii_media_active.set(mii.cur_media());
    }
}
/* </CODE> */
