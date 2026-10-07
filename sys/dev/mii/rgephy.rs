/*	$OpenBSD: rgephy.c,v 1.43 2023/04/05 10:45:07 kettenis Exp $	*/
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
 * $FreeBSD: rgephy.c,v 1.5 2004/05/30 17:57:40 phk Exp $
 */
/* </LICENSES> */

//! `dev/mii/rgephy.c`: driver for the Realtek 8169S/8110S internal 10/100/1000 PHY and its
//! successors (RTL8211F, RTL8251) (`rgephy* at mii?`).
//!
//! Upstream: sys/dev/mii/rgephy.c @ 3ce1f3f79392
//!
//! ## Deviations
//! - `rgephys[]` is a slice without the terminator; the parent driver's name is compared as
//!   bytes (`cd_name`).
//! - `rgephy_service` returns `Result<(), Errno>`; `rgephy_mii_phy_auto` returns
//!   `Err(EJUSTRETURN)`, as the C's `EJUSTRETURN`. The `setit:` label of the media change is
//!   a match arm that computes the speed and ANAR for the three manual media.
//! - `rgephyreg.rs` keeps its registers `u32` and their bits `u16` (em(4)'s shared code uses
//!   them); here they are widened to the `int` the PHY accessors take.
//! - `PHY_SETBIT`/`PHY_CLRBIT` (local macros) are the closures `setbit`/`clrbit` of
//!   `rgephy_load_dspcode`.

use core::ffi::c_void;

use crate::dev::ic::rtl81x9reg::{
    RL_GMEDIASTAT, RL_GMEDIASTAT_10MBPS, RL_GMEDIASTAT_100MBPS, RL_GMEDIASTAT_1000MBPS,
    RL_GMEDIASTAT_FDX, RL_GMEDIASTAT_LINK,
};
use crate::dev::mii::mii::{
    ANAR_10, ANAR_10_FD, ANAR_CSMA, ANAR_FC, ANAR_TX, ANAR_TX_FD, ANAR_X_PAUSE_ASYM, BMCR_AUTOEN,
    BMCR_FDX, BMCR_ISO, BMCR_LOOP, BMCR_PDOWN, BMCR_S10, BMCR_S100, BMCR_S1000, BMCR_STARTNEG,
    BMSR_ACOMP, BMSR_EXTSTAT, BMSR_LINK, BMSR_MEDIAMASK, EXTSR_MEDIAMASK, GTCR_ADV_1000TFDX,
    GTCR_ADV_1000THDX, GTCR_ADV_MS, GTCR_MAN_MS, GTSR_MS_RES, MII_100T2CR, MII_100T2SR, MII_ANAR,
    MII_BMCR, MII_BMSR, MII_EXTSR, bmsr_media_to_anar,
};
use crate::dev::mii::mii_physubr::{
    mii_phy_add_media, mii_phy_detach, mii_phy_flowstatus, mii_phy_match, mii_phy_reset,
    mii_phy_status, mii_phy_update,
};
use crate::dev::mii::miidevs::*;
use crate::dev::mii::miivar::{
    MII_ANEGTICKS_GIGE, MII_MEDIACHG, MII_POLLSTAT, MII_TICK, MIIF_DOPAUSE, MIIF_NOISOLATE,
    MIIF_RXID, MIIF_SETDELAY, MIIF_TXID, MiiAttachArgs, MiiData, MiiPhyFuncs, MiiPhydesc, MiiSoftc,
    mii_model, mii_rev,
};
use crate::dev::mii::rgephyreg::{
    RGEPHY_8211F, RGEPHY_F_SR, RGEPHY_F_SR_FDX, RGEPHY_F_SR_LINK, RGEPHY_F_SR_SPEED_10MBPS,
    RGEPHY_F_SR_SPEED_100MBPS, RGEPHY_F_SR_SPEED_1000MBPS, RGEPHY_MIICR1, RGEPHY_MIICR1_TXDLY_EN,
    RGEPHY_MIICR2, RGEPHY_MIICR2_RXDLY_EN, RGEPHY_PS, RGEPHY_PS_PAGE_MII, RGEPHY_SR, RGEPHY_SR_FDX,
    RGEPHY_SR_LINK, RGEPHY_SR_SPEED_10MBPS, RGEPHY_SR_SPEED_100MBPS, RGEPHY_SR_SPEED_1000MBPS,
    rgephy_f_sr_speed, rgephy_sr_speed,
};
use crate::kern::subr_prf::printf;
use crate::machine::cpu::delay;
use crate::net::if_::IFF_UP;
use crate::net::if_media::{
    IFM_10_T, IFM_100_TX, IFM_1000_T, IFM_ACTIVE, IFM_AUTO, IFM_AVALID, IFM_ETH_MASTER, IFM_ETHER,
    IFM_FDX, IFM_GMASK, IFM_HDX, IFM_LOOP, IFM_NONE, ifm_inst, ifm_subtype,
};
use crate::sys::device::{CfMatch, Cfattach, Cfdriver, DV_DULL, Device};
use crate::sys::errno::Errno;

/// `rgephy_ca`.
pub static RGEPHY_CA: Cfattach = Cfattach {
    ca_devsize: size_of::<MiiSoftc>(),
    ca_match: Some(rgephymatch),
    ca_attach: rgephyattach,
    ca_detach: Some(mii_phy_detach),
    ca_activate: None,
};

/// `rgephy_cd`.
pub static RGEPHY_CD: Cfdriver = Cfdriver::new(b"rgephy", DV_DULL, 0);

/// `rgephy_funcs`.
pub static RGEPHY_FUNCS: MiiPhyFuncs = MiiPhyFuncs {
    pf_service: rgephy_service,
    pf_status: rgephy_status,
    pf_reset: rgephy_reset,
};

/// `rgephys[]`.
static RGEPHYS: [MiiPhydesc; 4] = [
    MiiPhydesc {
        mpd_oui: MII_OUI_REALTEK2,
        mpd_model: MII_MODEL_xxREALTEK_RTL8169S,
        mpd_name: MII_STR_xxREALTEK_RTL8169S,
    },
    MiiPhydesc {
        mpd_oui: MII_OUI_xxREALTEK,
        mpd_model: MII_MODEL_xxREALTEK_RTL8169S,
        mpd_name: MII_STR_xxREALTEK_RTL8169S,
    },
    MiiPhydesc {
        mpd_oui: MII_OUI_xxREALTEK,
        mpd_model: MII_MODEL_xxREALTEK_RTL8251,
        mpd_name: MII_STR_xxREALTEK_RTL8251,
    },
    MiiPhydesc {
        mpd_oui: MII_OUI_xxREALTEK,
        mpd_model: MII_MODEL_xxREALTEK_RTL8211FVD,
        mpd_name: MII_STR_xxREALTEK_RTL8211FVD,
    },
];

/// `RGEPHY_8211F` as the `int` `mii_rev` is compared with.
const REV_8211F: i32 = RGEPHY_8211F as i32;

/// Whether the parent is re(4) or ure(4), whose MAC reports the link in `RL_GMEDIASTAT`.
fn parent_is_re(sc: &MiiSoftc) -> bool {
    let devname = sc.parent().cfdata().cf_driver.cd_name;
    devname == b"re" || devname == b"ure"
}

/// Whether the PHY is an RTL8211F, which has its own status register.
fn is_8211f(sc: &MiiSoftc) -> bool {
    sc.mii_model.get() == MII_MODEL_xxREALTEK_RTL8211FVD as i32
        || (sc.mii_model.get() == MII_MODEL_xxREALTEK_RTL8169S as i32
            && sc.mii_rev.get() == REV_8211F)
}

/// `rgephymatch`.
pub fn rgephymatch(_parent: Option<&Device>, _match: &CfMatch, aux: *mut c_void) -> i32 {
    // SAFETY: `mii_attach` hands its children a `struct mii_attach_args`.
    let ma = unsafe { &*aux.cast::<MiiAttachArgs>() };

    if mii_phy_match(ma, &RGEPHYS).is_some() {
        return 10;
    }

    0
}

/// `rgephyattach`.
pub fn rgephyattach(_parent: Option<&Device>, self_: &Device, aux: *mut c_void) {
    // SAFETY: `rgephy_ca`'s softc is a `struct mii_softc`; softcs are never freed while their
    // devices exist.
    let sc: &'static MiiSoftc = unsafe { &*core::ptr::from_ref(self_.softc::<MiiSoftc>()) };
    // SAFETY: `mii_attach` hands its children a `struct mii_attach_args`.
    let ma = unsafe { &*aux.cast::<MiiAttachArgs>() };
    let mii: &'static MiiData = ma.mii_data;

    // The match succeeded, so the PHY is in the table.
    let name = mii_phy_match(ma, &RGEPHYS).map_or("?", |mpd| mpd.mpd_name);
    printf(format_args!(": {}, rev. {}\n", name, mii_rev(ma.mii_id2)));

    sc.mii_inst.set(mii.mii_instance.get());
    sc.mii_phy.set(ma.mii_phyno);
    sc.mii_funcs.set(Some(&RGEPHY_FUNCS));
    sc.mii_model.set(mii_model(ma.mii_id2));
    sc.mii_rev.set(mii_rev(ma.mii_id2));
    sc.mii_pdata.set(Some(mii));
    sc.mii_flags.set(ma.mii_flags);
    sc.mii_anegticks.set(MII_ANEGTICKS_GIGE);

    sc.mii_flags.set(sc.mii_flags.get() | MIIF_NOISOLATE);

    sc.mii_capabilities
        .set(sc.phy_read(MII_BMSR) & ma.mii_capmask);

    if sc.mii_capabilities.get() & BMSR_EXTSTAT != 0 {
        sc.mii_extcapabilities.set(sc.phy_read(MII_EXTSR));
    }
    if sc.mii_capabilities.get() & BMSR_MEDIAMASK != 0
        || sc.mii_extcapabilities.get() & EXTSR_MEDIAMASK != 0
    {
        mii_phy_add_media(sc);
    }

    if is_8211f(sc) {
        rgephy_init_rtl8211f(sc);
    }

    sc.phy_reset();
}

/// `rgephy_service`.
pub fn rgephy_service(sc: &'static MiiSoftc, mii: &'static MiiData, cmd: i32) -> Result<(), Errno> {
    let ife_media = mii.cur_media();

    match cmd {
        MII_POLLSTAT => {
            // If we're not polling our PHY instance, just return.
            if ifm_inst(ife_media) != sc.mii_inst.get() {
                return Ok(());
            }
        }

        MII_MEDIACHG => {
            // If the media indicates a different PHY instance, isolate ourselves.
            if ifm_inst(ife_media) != sc.mii_inst.get() {
                let reg = sc.phy_read(MII_BMCR);
                sc.phy_write(MII_BMCR, reg | BMCR_ISO);
                return Ok(());
            }

            // If the interface is not up, don't do anything.
            if mii.ifp().if_flags.get() & IFF_UP != 0 {
                sc.phy_reset(); // XXX hardware bug work-around

                let mut anar = sc.phy_read(MII_ANAR);
                anar &= !(ANAR_TX_FD | ANAR_TX | ANAR_10_FD | ANAR_10);

                let subtype = ifm_subtype(ife_media);
                let setit = match subtype {
                    IFM_AUTO => {
                        let _ = rgephy_mii_phy_auto(sc);
                        None
                    }
                    IFM_1000_T => Some(BMCR_S1000),
                    IFM_100_TX => {
                        anar |= ANAR_TX_FD | ANAR_TX;
                        Some(BMCR_S100)
                    }
                    IFM_10_T => {
                        anar |= ANAR_10_FD | ANAR_10;
                        Some(BMCR_S10)
                    }
                    // #if 0: IFM_NONE: PHY_WRITE(sc, MII_BMCR, BMCR_ISO|BMCR_PDOWN);
                    _ => return Err(Errno::EINVAL),
                };

                if let Some(mut speed) = setit {
                    let mut gig = 0;
                    rgephy_loop(sc);
                    if (ife_media & IFM_GMASK) == IFM_FDX {
                        speed |= BMCR_FDX;
                        if subtype == IFM_1000_T {
                            gig = GTCR_ADV_1000TFDX;
                        }
                        anar &= !(ANAR_TX | ANAR_10);
                    } else {
                        if subtype == IFM_1000_T {
                            gig = GTCR_ADV_1000THDX;
                        }
                        anar &= !(ANAR_TX_FD | ANAR_10_FD);
                    }

                    if subtype == IFM_1000_T && mii.mii_media.ifm_media.get() & IFM_ETH_MASTER != 0
                    {
                        gig |= GTCR_MAN_MS | GTCR_ADV_MS;
                    }

                    sc.phy_write(MII_100T2CR, gig);
                    sc.phy_write(MII_BMCR, speed | BMCR_AUTOEN | BMCR_STARTNEG);
                    sc.phy_write(MII_ANAR, anar);
                }
            }
        }

        MII_TICK => {
            // If we're not currently selected, just return.
            if ifm_inst(ife_media) != sc.mii_inst.get() {
                return Ok(());
            }

            // Is the interface even up?
            if mii.ifp().if_flags.get() & IFF_UP == 0 {
                return Ok(());
            }

            // Only used for autonegotiation.
            if ifm_subtype(ife_media) == IFM_AUTO {
                rgephy_tick_autoneg(sc);
            }
        }

        _ => {}
    }

    // Update the media status.
    mii_phy_status(sc);

    // Callback if something changed. Note that we need to poke the DSP on the Realtek PHYs
    // if the media changes.
    if sc.mii_media_active.get() != mii.mii_media_active.get()
        || sc.mii_media_status.get() != mii.mii_media_status.get()
        || cmd == MII_MEDIACHG
    {
        rgephy_load_dspcode(sc);
    }

    // Callback if something changed.
    mii_phy_update(sc, cmd);

    Ok(())
}

/// `rgephy_service`'s `MII_TICK` work while autonegotiating: check to see if we have link.
/// If we do, we don't need to restart the autonegotiation process. Read the BMSR twice in
/// case it's latched. Only retry autonegotiation every mii_anegticks seconds.
fn rgephy_tick_autoneg(sc: &'static MiiSoftc) {
    if parent_is_re(sc) {
        let reg = sc.phy_read(RL_GMEDIASTAT as i32);
        if reg & RL_GMEDIASTAT_LINK as i32 != 0 {
            sc.mii_ticks.set(0);
            return;
        }
    } else if is_8211f(sc) {
        let reg = sc.phy_read(RGEPHY_F_SR as i32);
        if reg & i32::from(RGEPHY_F_SR_LINK) != 0 {
            sc.mii_ticks.set(0);
        }
    } else {
        let reg = sc.phy_read(RGEPHY_SR as i32);
        if reg & i32::from(RGEPHY_SR_LINK) != 0 {
            sc.mii_ticks.set(0);
            return;
        }
    }

    sc.mii_ticks.set(sc.mii_ticks.get() + 1);
    if sc.mii_ticks.get() <= sc.mii_anegticks.get() {
        return;
    }

    sc.mii_ticks.set(0);
    let _ = rgephy_mii_phy_auto(sc);
}

/// `rgephy_status`.
pub fn rgephy_status(sc: &'static MiiSoftc) {
    let mii = sc.pdata();
    let or_status = |w: u64| mii.mii_media_status.set(mii.mii_media_status.get() | w);
    let or_active = |w: u64| mii.mii_media_active.set(mii.mii_media_active.get() | w);

    mii.mii_media_status.set(IFM_AVALID);
    mii.mii_media_active.set(IFM_ETHER);

    if parent_is_re(sc) {
        let bmsr = sc.phy_read(RL_GMEDIASTAT as i32);
        if bmsr & RL_GMEDIASTAT_LINK as i32 != 0 {
            or_status(IFM_ACTIVE);
        }
    } else if is_8211f(sc) {
        let bmsr = sc.phy_read(RGEPHY_F_SR as i32);
        if bmsr & i32::from(RGEPHY_F_SR_LINK) != 0 {
            or_status(IFM_ACTIVE);
        }
    } else {
        let bmsr = sc.phy_read(RGEPHY_SR as i32);
        if bmsr & i32::from(RGEPHY_SR_LINK) != 0 {
            or_status(IFM_ACTIVE);
        }
    }

    let bmsr = sc.phy_read(MII_BMSR);

    let bmcr = sc.phy_read(MII_BMCR);

    if bmcr & BMCR_LOOP != 0 {
        or_active(IFM_LOOP);
    }

    if bmcr & BMCR_AUTOEN != 0 && bmsr & BMSR_ACOMP == 0 {
        // Erg, still trying, I guess...
        or_active(IFM_NONE);
        return;
    }

    if parent_is_re(sc) {
        let bmsr = sc.phy_read(RL_GMEDIASTAT as i32) as u32;
        if bmsr & RL_GMEDIASTAT_1000MBPS != 0 {
            or_active(IFM_1000_T);
        } else if bmsr & RL_GMEDIASTAT_100MBPS != 0 {
            or_active(IFM_100_TX);
        } else if bmsr & RL_GMEDIASTAT_10MBPS != 0 {
            or_active(IFM_10_T);
        }

        if bmsr & RL_GMEDIASTAT_FDX != 0 {
            or_active(mii_phy_flowstatus(sc) | IFM_FDX);
        } else {
            or_active(IFM_HDX);
        }
    } else if is_8211f(sc) {
        let bmsr = sc.phy_read(RGEPHY_F_SR as i32) as u16;
        let speed = rgephy_f_sr_speed(bmsr);
        if speed == RGEPHY_F_SR_SPEED_1000MBPS {
            or_active(IFM_1000_T);
        } else if speed == RGEPHY_F_SR_SPEED_100MBPS {
            or_active(IFM_100_TX);
        } else if speed == RGEPHY_F_SR_SPEED_10MBPS {
            or_active(IFM_10_T);
        }

        if bmsr & RGEPHY_F_SR_FDX != 0 {
            or_active(mii_phy_flowstatus(sc) | IFM_FDX);
        } else {
            or_active(IFM_HDX);
        }
    } else {
        let bmsr = sc.phy_read(RGEPHY_SR as i32) as u16;
        let speed = rgephy_sr_speed(bmsr);
        if speed == RGEPHY_SR_SPEED_1000MBPS {
            or_active(IFM_1000_T);
        } else if speed == RGEPHY_SR_SPEED_100MBPS {
            or_active(IFM_100_TX);
        } else if speed == RGEPHY_SR_SPEED_10MBPS {
            or_active(IFM_10_T);
        }

        if bmsr & RGEPHY_SR_FDX != 0 {
            or_active(mii_phy_flowstatus(sc) | IFM_FDX);
        } else {
            or_active(IFM_HDX);
        }
    }

    let gtsr = sc.phy_read(MII_100T2SR);
    if ifm_subtype(mii.mii_media_active.get()) == IFM_1000_T && gtsr & GTSR_MS_RES != 0 {
        or_active(IFM_ETH_MASTER);
    }
}

/// `rgephy_mii_phy_auto`: restarts autonegotiation; always `EJUSTRETURN`.
pub fn rgephy_mii_phy_auto(sc: &'static MiiSoftc) -> Result<(), Errno> {
    rgephy_loop(sc);
    sc.phy_reset();

    let mut anar = bmsr_media_to_anar(sc.mii_capabilities.get()) | ANAR_CSMA;
    if sc.mii_flags.get() & MIIF_DOPAUSE != 0 {
        anar |= ANAR_FC | ANAR_X_PAUSE_ASYM;
    }

    sc.phy_write(MII_ANAR, anar);
    delay(1000);
    sc.phy_write(MII_100T2CR, GTCR_ADV_1000THDX | GTCR_ADV_1000TFDX);
    delay(1000);
    sc.phy_write(MII_BMCR, BMCR_AUTOEN | BMCR_STARTNEG);
    delay(100);

    Err(Errno::EJUSTRETURN)
}

/// `rgephy_loop`: powers an early RTL8169S PHY down, then waits for the link to drop.
pub fn rgephy_loop(sc: &MiiSoftc) {
    if sc.mii_model.get() == MII_MODEL_xxREALTEK_RTL8169S as i32 && sc.mii_rev.get() < 2 {
        sc.phy_write(MII_BMCR, BMCR_PDOWN);
        delay(1000);
    }

    for _ in 0..15000 {
        let bmsr = sc.phy_read(MII_BMSR) as u32;
        if bmsr & BMSR_LINK as u32 == 0 {
            break;
        }
        delay(10);
    }
}

/// `rgephy_init_rtl8211f`: the RTL8211F's RGMII delays, when the parent asks for them.
pub fn rgephy_init_rtl8211f(sc: &MiiSoftc) {
    if sc.mii_flags.get() & MIIF_SETDELAY != 0 {
        // save page
        let page = sc.phy_read(RGEPHY_PS as i32);
        sc.phy_write(RGEPHY_PS as i32, i32::from(RGEPHY_PS_PAGE_MII));

        let mut val = sc.phy_read(RGEPHY_MIICR1 as i32);
        if sc.mii_flags.get() & MIIF_TXID != 0 {
            val |= i32::from(RGEPHY_MIICR1_TXDLY_EN);
        } else {
            val &= !i32::from(RGEPHY_MIICR1_TXDLY_EN);
        }
        sc.phy_write(RGEPHY_MIICR1 as i32, val);

        let mut val = sc.phy_read(RGEPHY_MIICR2 as i32);
        if sc.mii_flags.get() & MIIF_RXID != 0 {
            val |= i32::from(RGEPHY_MIICR2_RXDLY_EN);
        } else {
            val &= !i32::from(RGEPHY_MIICR2_RXDLY_EN);
        }
        sc.phy_write(RGEPHY_MIICR2 as i32, val);

        // restore page
        sc.phy_write(RGEPHY_PS as i32, page);
    }
}

/// `rgephy_load_dspcode`: initialize Realtek PHY per the datasheet. The DSP in the PHYs of
/// existing revisions of the 8169S/8110S chips need to be tuned in order to reliably
/// negotiate a 1000Mbps link. This is only needed for rev 0 and rev 1 of the PHY. Later
/// versions work without any fixups.
pub fn rgephy_load_dspcode(sc: &MiiSoftc) {
    if sc.mii_model.get() != MII_MODEL_xxREALTEK_RTL8169S as i32 || sc.mii_rev.get() > 1 {
        return;
    }

    let setbit = |y: i32, z: i32| sc.phy_write(y, sc.phy_read(y) | z);
    let clrbit = |y: i32, z: i32| sc.phy_write(y, sc.phy_read(y) & !z);

    sc.phy_write(31, 0x0001);
    sc.phy_write(21, 0x1000);
    sc.phy_write(24, 0x65C7);
    clrbit(4, 0x0800);
    let val = sc.phy_read(4) & 0xFFF;
    sc.phy_write(4, val);
    sc.phy_write(3, 0x00A1);
    sc.phy_write(2, 0x0008);
    sc.phy_write(1, 0x1020);
    sc.phy_write(0, 0x1000);
    setbit(4, 0x0800);
    clrbit(4, 0x0800);
    let val = (sc.phy_read(4) & 0xFFF) | 0x7000;
    sc.phy_write(4, val);
    sc.phy_write(3, 0xFF41);
    sc.phy_write(2, 0xDE60);
    sc.phy_write(1, 0x0140);
    sc.phy_write(0, 0x0077);
    let val = (sc.phy_read(4) & 0xFFF) | 0xA000;
    sc.phy_write(4, val);
    sc.phy_write(3, 0xDF01);
    sc.phy_write(2, 0xDF20);
    sc.phy_write(1, 0xFF95);
    sc.phy_write(0, 0xFA00);
    let val = (sc.phy_read(4) & 0xFFF) | 0xB000;
    sc.phy_write(4, val);
    sc.phy_write(3, 0xFF41);
    sc.phy_write(2, 0xDE20);
    sc.phy_write(1, 0x0140);
    sc.phy_write(0, 0x00BB);
    let val = (sc.phy_read(4) & 0xFFF) | 0xF000;
    sc.phy_write(4, val);
    sc.phy_write(3, 0xDF01);
    sc.phy_write(2, 0xDF20);
    sc.phy_write(1, 0xFF95);
    sc.phy_write(0, 0xBF00);
    setbit(4, 0x0800);
    clrbit(4, 0x0800);
    sc.phy_write(31, 0x0000);

    delay(40);
}

/// `rgephy_reset`.
pub fn rgephy_reset(sc: &'static MiiSoftc) {
    mii_phy_reset(sc);
    delay(1000);
    rgephy_load_dspcode(sc);
}
