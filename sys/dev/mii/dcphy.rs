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
 * Copyright (c) 1997, 1998, 1999
 *	Bill Paul <wpaul@ee.columbia.edu>.  All rights reserved.
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
 * $FreeBSD: src/sys/dev/mii/dcphy.c,v 1.6 2000/10/05 17:36:14 wpaul Exp $
 */
/* </LICENSES> */

/* <CODE> */
//! `dev/mii/dcphy.c`: pseudo-driver for internal NWAY support on DEC 21143 and workalike
//! controllers (`dcphy* at mii?`).
//!
//! Upstream: sys/dev/mii/dcphy.c @ 3ce1f3f79392
//!
//! Technically this abuses the miibus code to handle media selection and NWAY support, since
//! there is no MII interface: the logical operations are roughly the same, and the
//! alternative is to create a fake MII interface in the driver, which is harder to do. dc(4)
//! reports the 21143's vendor and device id at MII address 31 when its media is not MII
//! (`dc_miibus_readreg`), and this driver then drives the chip's SIA and NWAY registers
//! through the dc softc. On QEMU's `tulip` the MII has a real PHY (lxtphy(4)), so dcphy does
//! not attach there.
//!
//! ## Deviations
//! - `dcphy_service` and `dcphy_mii_phy_auto` return `Result<(), Errno>` (the C's
//!   `EJUSTRETURN` and `EIO` are `Err`s).
//! - The C's local `DC_SETBIT`/`DC_CLRBIT` macros (without parentheses around the bits, every
//!   use passes one constant) are `DcSoftc::dc_setbit`/`dc_clrbit`.
//! - The link-partner decision of `dcphy_status` is the pure [`dcphy_anlpar_media`].

use core::ffi::c_void;
use core::ptr;

use crate::dev::ic::dcreg::{
    DC_10BTCTRL, DC_10BTSTAT, DC_ASTAT_AUTONEGCMP, DC_ASTAT_DISABLE, DC_ASTAT_TXDISABLE, DC_NETCFG,
    DC_NETCFG_FULLDUPLEX, DC_NETCFG_PCS, DC_NETCFG_PORTSEL, DC_NETCFG_SCRAMBLER,
    DC_NETCFG_SPEEDSEL, DC_PMODE_SIA, DC_SIA_RESET, DC_SIARESET, DC_TCTL_AUTONEGENBL,
    DC_TSTAT_ANEGSTAT, DC_TSTAT_LP_CAN_NWAY, DC_TSTAT_LS10, DC_TSTAT_LS100, DC_TYPE_21145, DcSoftc,
};
use crate::dev::mii::mii::{
    ANLPAR_10, ANLPAR_10_FD, ANLPAR_T4, ANLPAR_TX, ANLPAR_TX_FD, BMCR_LOOP, BMCR_S100, BMSR_10TFDX,
    BMSR_10THDX, BMSR_100T4, BMSR_100TXFDX, BMSR_100TXHDX, BMSR_ANEG, BMSR_MEDIAMASK,
};
use crate::dev::mii::mii_physubr::{
    mii_phy_add_media, mii_phy_detach, mii_phy_status, mii_phy_update,
};
use crate::dev::mii::miidevs::{MII_MODEL_xxDEC_xxDC, MII_OUI_xxDEC};
use crate::dev::mii::miivar::{
    MII_MEDIACHG, MII_POLLSTAT, MII_TICK, MIIF_DOINGAUTO, MIIF_NOISOLATE, MiiAttachArgs, MiiData,
    MiiPhyFuncs, MiiSoftc, mii_model, mii_oui,
};
use crate::kern::subr_prf::{panic, printf};
use crate::machine::cpu::delay;
use crate::net::if_::IFF_UP;
use crate::net::if_media::{
    IFM_10_T, IFM_100_T4, IFM_100_TX, IFM_ACTIVE, IFM_AUTO, IFM_AVALID, IFM_ETHER, IFM_FDX,
    IFM_GMASK, IFM_HDX, IFM_LOOP, IFM_NONE, ifm_inst, ifm_makeword, ifm_subtype, ifmedia_add,
};
use crate::sys::device::{CfMatch, Cfattach, Cfdriver, DV_DULL, DVF_ACTIVE, Device};
use crate::sys::errno::Errno;

/// `MIIF_AUTOTIMEOUT`.
pub const MIIF_AUTOTIMEOUT: i32 = 0x0004;

/// `COMPAQ_PRESARIO_ID`: the subsystem ID for the built-in 21143 ethernet in several Compaq
/// Presario systems. Apparently these are 10Mbps only, so we need to treat them specially.
pub const COMPAQ_PRESARIO_ID: u32 = 0xb0bb0e11;

/// `dcphy_ca`.
pub static DCPHY_CA: Cfattach = Cfattach {
    ca_devsize: size_of::<MiiSoftc>(),
    ca_match: Some(dcphy_match),
    ca_attach: dcphy_attach,
    ca_detach: Some(mii_phy_detach),
    ca_activate: None,
};

/// `dcphy_cd`.
pub static DCPHY_CD: Cfdriver = Cfdriver::new(b"dcphy", DV_DULL, 0);

/// `dcphy_funcs`.
pub static DCPHY_FUNCS: MiiPhyFuncs = MiiPhyFuncs {
    pf_service: dcphy_service,
    pf_status: dcphy_status,
    pf_reset: dcphy_reset,
};

/// `mii->mii_ifp->if_softc`: the dc(4) softc this PHY belongs to.
fn dcphy_dc_sc(mii: &MiiData) -> &'static DcSoftc {
    let p = mii.ifp().if_softc.get().cast::<DcSoftc>().cast_const();
    if p.is_null() {
        panic(format_args!("dcphy: interface without its softc"));
    }
    // SAFETY: dcphy matches only the id dc_miibus_readreg makes up, so the interface is a dc
    // interface, whose `if_softc` dc_attach set to its softc; softcs outlive their
    // interfaces.
    unsafe { &*p }
}

/// `dcphy_match`: the dc driver will report the 21143 vendor and device ID to let us know
/// that it wants us to attach.
pub fn dcphy_match(_parent: Option<&Device>, _match: &CfMatch, aux: *mut c_void) -> i32 {
    // SAFETY: `mii_attach` hands its children a `struct mii_attach_args`.
    let ma = unsafe { &*aux.cast::<MiiAttachArgs>() };

    if mii_oui(ma.mii_id1, ma.mii_id2) as u32 == MII_OUI_xxDEC
        && mii_model(ma.mii_id2) as u32 == MII_MODEL_xxDEC_xxDC
    {
        return 10;
    }

    0
}

/// `dcphy_attach`.
pub fn dcphy_attach(_parent: Option<&Device>, self_: &Device, aux: *mut c_void) {
    // SAFETY: `dcphy_ca`'s softc is a `struct mii_softc`; softcs are never freed while their
    // devices exist.
    let sc: &'static MiiSoftc = unsafe { &*ptr::from_ref(self_.softc::<MiiSoftc>()) };
    // SAFETY: `mii_attach` hands its children a `struct mii_attach_args`.
    let ma = unsafe { &*aux.cast::<MiiAttachArgs>() };
    let mii: &'static MiiData = ma.mii_data;

    printf(format_args!(": internal PHY\n"));
    sc.mii_inst.set(mii.mii_instance.get());
    sc.mii_phy.set(ma.mii_phyno);
    sc.mii_funcs.set(Some(&DCPHY_FUNCS));
    sc.mii_pdata.set(Some(mii));
    sc.mii_flags.set(ma.mii_flags);
    sc.mii_anegticks.set(50);

    sc.mii_flags.set(sc.mii_flags.get() | MIIF_NOISOLATE);

    let dc_sc = dcphy_dc_sc(mii);
    dc_sc.csr_write_4(DC_10BTSTAT, 0);
    dc_sc.csr_write_4(DC_10BTCTRL, 0);

    let caps = match dc_sc.dc_csid.get() {
        // Example of how to only allow 10Mbps modes.
        COMPAQ_PRESARIO_ID => BMSR_ANEG | BMSR_10TFDX | BMSR_10THDX,
        _ if dc_sc.dc_pmode.get() == DC_PMODE_SIA => BMSR_ANEG | BMSR_10TFDX | BMSR_10THDX,
        _ => {
            ifmedia_add(
                &mii.mii_media,
                ifm_makeword(IFM_ETHER, IFM_100_TX, IFM_LOOP, sc.mii_inst.get()),
                BMCR_LOOP | BMCR_S100,
                ptr::null_mut(),
            );

            BMSR_ANEG | BMSR_100TXFDX | BMSR_100TXHDX | BMSR_10TFDX | BMSR_10THDX
        }
    };
    sc.mii_capabilities.set(caps);

    if dc_sc.dc_type.get() == DC_TYPE_21145 {
        sc.mii_capabilities.set(BMSR_10THDX);
    }

    sc.mii_capabilities
        .set(sc.mii_capabilities.get() & ma.mii_capmask);
    if sc.mii_capabilities.get() & BMSR_MEDIAMASK != 0 {
        mii_phy_add_media(sc);
    }
}

/// `dcphy_service`.
pub fn dcphy_service(sc: &'static MiiSoftc, mii: &'static MiiData, cmd: i32) -> Result<(), Errno> {
    if sc.mii_dev.dv_flags.get() & DVF_ACTIVE == 0 {
        return Err(Errno::ENXIO);
    }

    let dc_sc = dcphy_dc_sc(mii);
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
                return Ok(());
            }

            // If the interface is not up, don't do anything.
            if mii.ifp().if_flags.get() & IFF_UP != 0 {
                sc.mii_flags.set(0);
                mii.mii_media_active.set(IFM_NONE);
                let mut mode = dc_sc.csr_read_4(DC_NETCFG);
                mode &= !(DC_NETCFG_FULLDUPLEX
                    | DC_NETCFG_PORTSEL
                    | DC_NETCFG_PCS
                    | DC_NETCFG_SCRAMBLER
                    | DC_NETCFG_SPEEDSEL);
                let fdx = ife_media & IFM_GMASK == IFM_FDX;

                match ifm_subtype(ife_media) {
                    IFM_AUTO => {
                        // PHY_RESET(sc);
                        sc.mii_flags.set(sc.mii_flags.get() & !MIIF_DOINGAUTO);
                        let _ = dcphy_mii_phy_auto(sc, false);
                    }
                    IFM_100_TX => {
                        sc.phy_reset();
                        dc_sc.dc_clrbit(DC_10BTCTRL, DC_TCTL_AUTONEGENBL);
                        mode |= DC_NETCFG_PORTSEL | DC_NETCFG_PCS | DC_NETCFG_SCRAMBLER;
                        if fdx {
                            mode |= DC_NETCFG_FULLDUPLEX;
                        } else {
                            mode &= !DC_NETCFG_FULLDUPLEX;
                        }
                        dc_sc.csr_write_4(DC_NETCFG, mode);
                    }
                    IFM_10_T => {
                        dc_sc.dc_clrbit(DC_SIARESET, DC_SIA_RESET);
                        dc_sc.dc_clrbit(DC_10BTCTRL, 0xFFFF);
                        if fdx {
                            dc_sc.dc_setbit(DC_10BTCTRL, 0x7F3D);
                        } else {
                            dc_sc.dc_setbit(DC_10BTCTRL, 0x7F3F);
                        }
                        dc_sc.dc_setbit(DC_SIARESET, DC_SIA_RESET);
                        dc_sc.dc_clrbit(DC_10BTCTRL, DC_TCTL_AUTONEGENBL);
                        mode &= !DC_NETCFG_PORTSEL;
                        mode |= DC_NETCFG_SPEEDSEL;
                        if fdx {
                            mode |= DC_NETCFG_FULLDUPLEX;
                        } else {
                            mode &= !DC_NETCFG_FULLDUPLEX;
                        }
                        dc_sc.csr_write_4(DC_NETCFG, mode);
                    }
                    _ => return Err(Errno::EINVAL),
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
                let reg = dc_sc.csr_read_4(DC_10BTSTAT);
                if reg & DC_TSTAT_LS10 == 0 || reg & DC_TSTAT_LS100 == 0 {
                    sc.mii_ticks.set(0);
                } else {
                    // Only retry autonegotiation every mii_anegticks seconds.
                    //
                    // Otherwise, fall through to calling dcphy_status() since real Intel
                    // 21143 chips don't show valid link status until autonegotiation is
                    // switched off, and that only happens in dcphy_status(). Without this,
                    // successful autonegotiation is never recognised on these chips.
                    sc.mii_ticks.set(sc.mii_ticks.get() + 1);
                    if sc.mii_ticks.get() > sc.mii_anegticks.get() {
                        sc.mii_ticks.set(0);
                        sc.mii_flags.set(sc.mii_flags.get() & !MIIF_DOINGAUTO);
                        let _ = dcphy_mii_phy_auto(sc, false);
                    }
                }
            }
        }

        _ => {}
    }

    // Update the media status.
    mii_phy_status(sc);

    // Callback if something changed.
    mii_phy_update(sc, cmd);
    Ok(())
}

/// The media a link partner that negotiated advertises (`anlpar`, the upper half of
/// `DC_10BTSTAT`) and our capabilities `caps` resolve to, best first, as `dcphy_status`
/// picks them.
pub fn dcphy_anlpar_media(anlpar: i32, caps: i32) -> u64 {
    if anlpar & ANLPAR_TX_FD != 0 && caps & BMSR_100TXFDX != 0 {
        IFM_100_TX | IFM_FDX
    } else if anlpar & ANLPAR_T4 != 0 && caps & BMSR_100T4 != 0 {
        IFM_100_T4 | IFM_HDX
    } else if anlpar & ANLPAR_TX != 0 && caps & BMSR_100TXHDX != 0 {
        IFM_100_TX | IFM_HDX
    } else if anlpar & ANLPAR_10_FD != 0 {
        IFM_10_T | IFM_FDX
    } else if anlpar & ANLPAR_10 != 0 {
        IFM_10_T | IFM_HDX
    } else {
        IFM_NONE
    }
}

/// `dcphy_status`.
pub fn dcphy_status(sc: &'static MiiSoftc) {
    let mii = sc.pdata();
    let dc_sc = dcphy_dc_sc(mii);
    let or_active = |w: u64| mii.mii_media_active.set(mii.mii_media_active.get() | w);

    mii.mii_media_status.set(IFM_AVALID);
    mii.mii_media_active.set(IFM_ETHER);

    let reg = dc_sc.csr_read_4(DC_10BTSTAT);
    if reg & DC_TSTAT_LS10 == 0 || reg & DC_TSTAT_LS100 == 0 {
        mii.mii_media_status
            .set(mii.mii_media_status.get() | IFM_ACTIVE);
    }

    let mut skip = false;
    if dc_sc.csr_read_4(DC_10BTCTRL) & DC_TCTL_AUTONEGENBL != 0 {
        // Erg, still trying, I guess...
        let tstat = dc_sc.csr_read_4(DC_10BTSTAT);
        if tstat & DC_TSTAT_ANEGSTAT != DC_ASTAT_AUTONEGCMP {
            if (dc_sc.dc_is_macronix() || dc_sc.dc_is_pnicii())
                && tstat & DC_TSTAT_ANEGSTAT == DC_ASTAT_DISABLE
            {
                skip = true;
            } else {
                or_active(IFM_NONE);
                return;
            }
        }

        if !skip {
            if tstat & DC_TSTAT_LP_CAN_NWAY != 0 {
                let anlpar = (tstat >> 16) as i32;
                or_active(dcphy_anlpar_media(anlpar, sc.mii_capabilities.get()));
                if dc_sc.dc_is_intel() {
                    dc_sc.dc_clrbit(DC_10BTCTRL, DC_TCTL_AUTONEGENBL);
                }
                return;
            }

            // If the other side doesn't support NWAY, then the best we can do is determine
            // if we have a 10Mbps or 100Mbps link. There's no way to know if the link is
            // full or half duplex, so we default to half duplex and hope that the user is
            // clever enough to manually change the media settings if we're wrong.
            if reg & DC_TSTAT_LS100 == 0 {
                or_active(IFM_100_TX | IFM_HDX);
            } else if reg & DC_TSTAT_LS10 == 0 {
                or_active(IFM_10_T | IFM_HDX);
            } else {
                or_active(IFM_NONE);
            }
            if dc_sc.dc_is_intel() {
                dc_sc.dc_clrbit(DC_10BTCTRL, DC_TCTL_AUTONEGENBL);
            }
            return;
        }
    }

    // skip:
    if dc_sc.csr_read_4(DC_NETCFG) & DC_NETCFG_SPEEDSEL != 0 {
        or_active(IFM_10_T);
    } else {
        or_active(IFM_100_TX);
    }

    if dc_sc.csr_read_4(DC_NETCFG) & DC_NETCFG_FULLDUPLEX != 0 {
        or_active(IFM_FDX);
    } else {
        or_active(IFM_HDX);
    }
}

/// `dcphy_mii_phy_auto`: start the 21143's NWAY; with `waitfor`, wait 500ms for it to
/// complete (`Err(EIO)` when it does not), else let it finish asynchronously
/// (`Err(EJUSTRETURN)`).
pub fn dcphy_mii_phy_auto(mii: &'static MiiSoftc, waitfor: bool) -> Result<(), Errno> {
    let sc = dcphy_dc_sc(mii.pdata());

    if mii.mii_flags.get() & MIIF_DOINGAUTO == 0 {
        sc.dc_clrbit(DC_NETCFG, DC_NETCFG_PORTSEL);
        sc.dc_setbit(DC_NETCFG, DC_NETCFG_FULLDUPLEX);
        sc.dc_clrbit(DC_SIARESET, DC_SIA_RESET);
        if mii.mii_capabilities.get() & BMSR_100TXHDX != 0 {
            sc.csr_write_4(DC_10BTCTRL, 0x3FFFF);
        } else {
            sc.csr_write_4(DC_10BTCTRL, 0xFFFF);
        }
        sc.dc_setbit(DC_SIARESET, DC_SIA_RESET);
        sc.dc_setbit(DC_10BTCTRL, DC_TCTL_AUTONEGENBL);
        sc.dc_setbit(DC_10BTSTAT, DC_ASTAT_TXDISABLE);
    }

    if waitfor {
        // Wait 500ms for it to complete.
        for _ in 0..500 {
            if sc.csr_read_4(DC_10BTSTAT) & DC_TSTAT_ANEGSTAT == DC_ASTAT_AUTONEGCMP {
                return Ok(());
            }
            delay(1000);
        }
        // Don't need to worry about clearing MIIF_DOINGAUTO. If that's set, a timeout is
        // pending, and it will clear the flag.
        return Err(Errno::EIO);
    }

    // Just let it finish asynchronously. This is for the benefit of the tick handler driving
    // autonegotiation. Don't want 500ms delays all the time while the system is running!
    if mii.mii_flags.get() & MIIF_DOINGAUTO == 0 {
        mii.mii_flags.set(mii.mii_flags.get() | MIIF_DOINGAUTO);
    }

    Err(Errno::EJUSTRETURN)
}

/// `dcphy_reset`: pulse the SIA reset.
pub fn dcphy_reset(mii: &'static MiiSoftc) {
    let sc = dcphy_dc_sc(mii.pdata());

    sc.dc_clrbit(DC_SIARESET, DC_SIA_RESET);
    delay(1000);
    sc.dc_setbit(DC_SIARESET, DC_SIA_RESET);
}
/* </CODE> */

/* <TESTS> */
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn link_partner_media_best_first() {
        let all = BMSR_100TXFDX | BMSR_100TXHDX | BMSR_100T4 | BMSR_10TFDX | BMSR_10THDX;
        assert_eq!(
            dcphy_anlpar_media(ANLPAR_TX_FD | ANLPAR_10, all),
            IFM_100_TX | IFM_FDX
        );
        // 100baseTX full duplex not ours: the next one both sides have.
        assert_eq!(
            dcphy_anlpar_media(ANLPAR_TX_FD | ANLPAR_TX, BMSR_100TXHDX),
            IFM_100_TX | IFM_HDX
        );
        assert_eq!(dcphy_anlpar_media(ANLPAR_T4, all), IFM_100_T4 | IFM_HDX);
        // The 10 Mb/s media are taken whatever the capabilities say, as in C.
        assert_eq!(dcphy_anlpar_media(ANLPAR_10_FD, 0), IFM_10_T | IFM_FDX);
        assert_eq!(dcphy_anlpar_media(ANLPAR_10, 0), IFM_10_T | IFM_HDX);
        assert_eq!(dcphy_anlpar_media(0, all), IFM_NONE);
    }

    #[test]
    fn matches_the_id_dc_makes_up() {
        // dc_miibus_readreg answers PCI_VENDOR_DEC and PCI_PRODUCT_DEC_21142 at MII address
        // 31: MII_OUI(0x1011, 0x0019) is xxDEC and MII_MODEL(0x0019) xxDC.
        assert_eq!(mii_oui(0x1011, 0x0019) as u32, MII_OUI_xxDEC);
        assert_eq!(mii_model(0x0019) as u32, MII_MODEL_xxDEC_xxDC);
    }
}
/* </TESTS> */
