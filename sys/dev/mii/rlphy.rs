/*	$OpenBSD: rlphy.c,v 1.35 2025/05/09 13:53:10 jcs Exp $	*/
/* <LICENSES> */
/*
 * Copyright (c) 1998, 1999 Jason L. Wright (jason@thought.net)
 * All rights reserved.
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
 * IMPLIED WARRANTIES, INCLUDING, BUT NOT LIMITED TO, THE IMPLIED
 * WARRANTIES OF MERCHANTABILITY AND FITNESS FOR A PARTICULAR PURPOSE ARE
 * DISCLAIMED.  IN NO EVENT SHALL THE AUTHOR BE LIABLE FOR ANY DIRECT,
 * INDIRECT, INCIDENTAL, SPECIAL, EXEMPLARY, OR CONSEQUENTIAL DAMAGES
 * (INCLUDING, BUT NOT LIMITED TO, PROCUREMENT OF SUBSTITUTE GOODS OR
 * SERVICES; LOSS OF USE, DATA, OR PROFITS; OR BUSINESS INTERRUPTION)
 * HOWEVER CAUSED AND ON ANY THEORY OF LIABILITY, WHETHER IN CONTRACT,
 * STRICT LIABILITY, OR TORT (INCLUDING NEGLIGENCE OR OTHERWISE) ARISING IN
 * ANY WAY OUT OF THE USE OF THIS SOFTWARE, EVEN IF ADVISED OF THE
 * POSSIBILITY OF SUCH DAMAGE.
 */
/* </LICENSES> */

//! `dev/mii/rlphy.c`: driver for the internal PHY found on RTL8139 based nics (`rlphy* at
//! mii?`), based on drivers for the 'exphy' (Internal 3Com phys) and 'nsphy' (National
//! Semiconductor DP83840).
//!
//! Upstream: sys/dev/mii/rlphy.c @ 3ce1f3f79392
//!
//! QEMU's `rtl8139` is an 8139C+: re(4) presents its PHY at address 0 with zero ID registers,
//! which only this driver (at priority 5, below a PHY it knows) and ukphy(4) take.
//!
//! ## Deviations
//! - `rlphys[]` is a slice without the `{ 0, 0, NULL }` terminator; the parent driver's name
//!   is compared as bytes (`cd_name`).
//! - `rlphy_service` returns `Result<(), Errno>`.

use core::ffi::c_void;

use crate::dev::ic::rtl81x9reg::{RL_MEDIASTAT, RL_MEDIASTAT_SPEED10};
use crate::dev::mii::mii::{
    ANLPAR_10, ANLPAR_10_FD, ANLPAR_T4, ANLPAR_TX, ANLPAR_TX_FD, BMCR_AUTOEN, BMCR_ISO, BMCR_LOOP,
    BMSR_ACOMP, BMSR_LINK, BMSR_MEDIAMASK, MII_ANAR, MII_ANLPAR, MII_BMCR, MII_BMSR,
};
use crate::dev::mii::mii_physubr::{
    mii_anar, mii_phy_add_media, mii_phy_auto, mii_phy_detach, mii_phy_down, mii_phy_match,
    mii_phy_reset, mii_phy_status, mii_phy_update,
};
use crate::dev::mii::miidevs::*;
use crate::dev::mii::miivar::{
    MII_DOWN, MII_MEDIACHG, MII_POLLSTAT, MII_TICK, MIIF_NOISOLATE, MiiAttachArgs, MiiData,
    MiiPhyFuncs, MiiPhydesc, MiiSoftc, mii_model, mii_oui, mii_rev,
};
use crate::kern::subr_prf::{panic, printf};
use crate::net::if_::IFF_UP;
use crate::net::if_media::{
    IFM_10_T, IFM_100_T4, IFM_100_TX, IFM_ACTIVE, IFM_AUTO, IFM_AVALID, IFM_ETHER, IFM_FDX,
    IFM_HDX, IFM_LOOP, IFM_NONE, ifm_inst, ifm_subtype,
};
use crate::sys::device::{CfMatch, Cfattach, Cfdriver, DV_DULL, DVF_ACTIVE, Device};
use crate::sys::errno::Errno;

/// `rlphy_ca`.
pub static RLPHY_CA: Cfattach = Cfattach {
    ca_devsize: size_of::<MiiSoftc>(),
    ca_match: Some(rlphymatch),
    ca_attach: rlphyattach,
    ca_detach: Some(mii_phy_detach),
    ca_activate: None,
};

/// `rlphy_cd`.
pub static RLPHY_CD: Cfdriver = Cfdriver::new(b"rlphy", DV_DULL, 0);

/// `rlphy_funcs`.
pub static RLPHY_FUNCS: MiiPhyFuncs = MiiPhyFuncs {
    pf_service: rlphy_service,
    pf_status: rlphy_status,
    pf_reset: mii_phy_reset,
};

/// `rlphys[]`.
static RLPHYS: [MiiPhydesc; 4] = [
    MiiPhydesc {
        mpd_oui: MII_OUI_REALTEK,
        mpd_model: MII_MODEL_REALTEK_RTL8201L,
        mpd_name: MII_STR_REALTEK_RTL8201L,
    },
    MiiPhydesc {
        mpd_oui: MII_OUI_xxREALTEK,
        mpd_model: MII_MODEL_xxREALTEK_RTL8201E,
        mpd_name: MII_STR_xxREALTEK_RTL8201E,
    },
    MiiPhydesc {
        mpd_oui: MII_OUI_xxREALTEK,
        mpd_model: MII_MODEL_xxREALTEK_RTL8201F,
        mpd_name: MII_STR_xxREALTEK_RTL8201F,
    },
    MiiPhydesc {
        mpd_oui: MII_OUI_ICPLUS,
        mpd_model: MII_MODEL_ICPLUS_IP101,
        mpd_name: MII_STR_ICPLUS_IP101,
    },
];

/// `rlphymatch`.
pub fn rlphymatch(parent: Option<&Device>, _match: &CfMatch, aux: *mut c_void) -> i32 {
    // SAFETY: `mii_attach` hands its children a `struct mii_attach_args`.
    let ma = unsafe { &*aux.cast::<MiiAttachArgs>() };
    let devname = parent.map_or(&b""[..], |p| p.cfdata().cf_driver.cd_name);

    if mii_phy_match(ma, &RLPHYS).is_some() {
        return 10;
    }

    if mii_oui(ma.mii_id1, ma.mii_id2) != 0 || mii_model(ma.mii_id2) != 0 {
        return 0;
    }

    if devname != b"re" && devname != b"rl" {
        return 0;
    }

    // A "real" phy should get preference, but on the 8139 there is no phyid register.
    5
}

/// `rlphyattach`.
pub fn rlphyattach(_parent: Option<&Device>, self_: &Device, aux: *mut c_void) {
    // SAFETY: `rlphy_ca`'s softc is a `struct mii_softc`; softcs are never freed while their
    // devices exist.
    let sc: &'static MiiSoftc = unsafe { &*core::ptr::from_ref(self_.softc::<MiiSoftc>()) };
    // SAFETY: `mii_attach` hands its children a `struct mii_attach_args`.
    let ma = unsafe { &*aux.cast::<MiiAttachArgs>() };
    let mii: &'static MiiData = ma.mii_data;

    match mii_phy_match(ma, &RLPHYS) {
        Some(mpd) => printf(format_args!(
            ": {}, rev. {}\n",
            mpd.mpd_name,
            mii_rev(ma.mii_id2)
        )),
        None => printf(format_args!(": RTL internal PHY\n")),
    };

    sc.mii_inst.set(mii.mii_instance.get());
    sc.mii_phy.set(ma.mii_phyno);
    sc.mii_funcs.set(Some(&RLPHY_FUNCS));
    sc.mii_pdata.set(Some(mii));
    sc.mii_flags.set(ma.mii_flags);

    sc.mii_flags.set(sc.mii_flags.get() | MIIF_NOISOLATE);

    sc.phy_reset();

    sc.mii_capabilities
        .set(sc.phy_read(MII_BMSR) & ma.mii_capmask);
    if sc.mii_capabilities.get() & BMSR_MEDIAMASK != 0 {
        mii_phy_add_media(sc);
    }
}

/// `rlphy_service`.
pub fn rlphy_service(sc: &'static MiiSoftc, mii: &'static MiiData, cmd: i32) -> Result<(), Errno> {
    if sc.mii_dev.dv_flags.get() & DVF_ACTIVE == 0 {
        return Err(Errno::ENXIO);
    }

    let ife_media = mii.cur_media();

    // Can't isolate the RTL8139 phy, so it has to be the only one.
    if ifm_inst(ife_media) != sc.mii_inst.get() {
        panic(format_args!("rlphy_service: attempt to isolate phy"));
    }

    match cmd {
        MII_POLLSTAT => {}

        MII_MEDIACHG => {
            // If the interface is not up, don't do anything.
            if mii.ifp().if_flags.get() & IFF_UP != 0 {
                match ifm_subtype(ife_media) {
                    IFM_AUTO => {
                        // If we're already in auto mode, just return.
                        if sc.phy_read(MII_BMCR) & BMCR_AUTOEN != 0 {
                            return Ok(());
                        }
                        let _ = mii_phy_auto(sc, false);
                    }
                    IFM_100_T4 => {
                        // XXX Not supported as a manual setting right now.
                        return Err(Errno::EINVAL);
                    }
                    _ => {
                        // BMCR data is stored in the ifmedia entry.
                        sc.phy_write(MII_ANAR, mii_anar(ife_media));
                        sc.phy_write(MII_BMCR, mii.cur_data() as i32);
                    }
                }
            }
        }

        MII_TICK => {
            // Is the interface even up?
            if mii.ifp().if_flags.get() & IFF_UP == 0 {
                return Ok(());
            }

            // The Realtek PHY's autonegotiation doesn't need to be kicked; it continues in
            // the background.
        }

        MII_DOWN => {
            mii_phy_down(sc);
            return Ok(());
        }

        _ => {}
    }

    // Update the media status.
    mii_phy_status(sc);

    // Callback if something changed.
    mii_phy_update(sc, cmd);
    Ok(())
}

/// `rlphy_status`.
pub fn rlphy_status(sc: &'static MiiSoftc) {
    let mii = sc.pdata();
    let devname = sc.parent().cfdata().cf_driver.cd_name;

    mii.mii_media_status.set(IFM_AVALID);
    mii.mii_media_active.set(IFM_ETHER);
    let or_active = |w: u64| mii.mii_media_active.set(mii.mii_media_active.get() | w);

    let bmsr = sc.phy_read(MII_BMSR) | sc.phy_read(MII_BMSR);
    if bmsr & BMSR_LINK != 0 {
        mii.mii_media_status
            .set(mii.mii_media_status.get() | IFM_ACTIVE);
    }

    let bmcr = sc.phy_read(MII_BMCR);
    if bmcr & BMCR_ISO != 0 {
        or_active(IFM_NONE);
        mii.mii_media_status.set(0);
        return;
    }

    if bmcr & BMCR_LOOP != 0 {
        or_active(IFM_LOOP);
    }

    if bmcr & BMCR_AUTOEN != 0 {
        // NWay autonegotiation takes the highest-order common bit of the ANAR and ANLPAR
        // (i.e. best media advertised both by us and our link partner).
        if bmsr & BMSR_ACOMP == 0 {
            // Erg, still trying, I guess...
            or_active(IFM_NONE);
            return;
        }

        let anlpar = sc.phy_read(MII_ANAR) & sc.phy_read(MII_ANLPAR);
        if anlpar != 0 {
            if anlpar & ANLPAR_TX_FD != 0 {
                or_active(IFM_100_TX | IFM_FDX);
            } else if anlpar & ANLPAR_T4 != 0 {
                or_active(IFM_100_T4 | IFM_HDX);
            } else if anlpar & ANLPAR_TX != 0 {
                or_active(IFM_100_TX | IFM_HDX);
            } else if anlpar & ANLPAR_10_FD != 0 {
                or_active(IFM_10_T | IFM_FDX);
            } else if anlpar & ANLPAR_10 != 0 {
                or_active(IFM_10_T | IFM_HDX);
            } else {
                or_active(IFM_NONE);
            }
            return;
        }

        // If the other side doesn't support NWAY, then the best we can do is determine if
        // we have a 10Mbps or 100Mbps link. There's no way to know if the link is full or
        // half duplex, so we default to half duplex and hope that the user is clever enough
        // to manually change the media settings if we're wrong.

        // The Realtek PHY supports non-NWAY link speed detection, however it does not report
        // the link detection results via the ANLPAR or BMSR registers. (What? Realtek
        // doesn't do things the way everyone else does? I'm just shocked, shocked I tell
        // you.) To determine the link speed, we have to do one of two things:
        //
        // - If this is a standalone Realtek RTL8201(L) PHY, we can determine the link speed
        //   by testing bit 0 in the magic, vendor-specific register at offset 0x19.
        //
        // - If this is a Realtek MAC with integrated PHY, we can test the 'SPEED10' bit of
        //   the MAC's media status register.
        if devname == b"rl" || devname == b"re" {
            if sc.phy_read(RL_MEDIASTAT as i32) & RL_MEDIASTAT_SPEED10 as i32 != 0 {
                or_active(IFM_10_T);
            } else {
                or_active(IFM_100_TX);
            }
        } else if sc.phy_read(0x0019) & 0x01 != 0 {
            or_active(IFM_100_TX);
        } else {
            or_active(IFM_10_T);
        }
        or_active(IFM_HDX);
    } else {
        mii.mii_media_active.set(mii.cur_media());
    }
}
