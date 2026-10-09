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
 * Copyright (c) 1998, 1999 The NetBSD Foundation, Inc.
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

/*
 * Copyright (c) 1997 Manuel Bouyer.  All rights reserved.
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
//! `dev/mii/lxtphy.c`: driver for Level One's LXT-970/971 ethernet 10/100 PHY
//! (`lxtphy* at mii?`); datasheet from www.level1.com.
//!
//! Upstream: sys/dev/mii/lxtphy.c @ 3ce1f3f79392
//!
//! QEMU's `tulip` (a DEC 21143, dc(4)) presents an LXT970 at MII address 1: OpenBSD 8.0
//! prints `lxtphy0 at dc0 phy 1: LXT970, rev. 0` on it. The LXT970 reports its link, speed
//! and duplex in its chip status register (`lxtphy_status`); the LXT971 uses ukphy(4)'s
//! generic status.
//!
//! ## Deviations
//! - `lxtphys[]` is a slice without the `{ 0, 0, NULL }` terminator.
//! - `lxtphy_service` returns `Result<(), Errno>`; it reads the selected media through
//!   `MiiData::cur_media` (the C dereferences `ifm_cur`).
//! - `lxtphyattach` sets no `mii_funcs` for a PHY that is neither an LXT970 nor an LXT971,
//!   as in C; `lxtphymatch` never lets one attach.

use core::ffi::c_void;
use core::ptr;

use crate::dev::mii::lxtphyreg::MII_LXTPHY_IER;
use crate::dev::mii::lxtphyreg::{CSR_DUPLEX, CSR_LINK, CSR_SPEED, IER_INTEN, MII_LXTPHY_CSR};
use crate::dev::mii::mii::{
    BMCR_AUTOEN, BMCR_ISO, BMCR_LOOP, BMSR_ACOMP, BMSR_MEDIAMASK, MII_BMCR, MII_BMSR,
};
use crate::dev::mii::mii_physubr::{
    mii_phy_add_media, mii_phy_detach, mii_phy_down, mii_phy_match, mii_phy_reset,
    mii_phy_setmedia, mii_phy_status, mii_phy_tick, mii_phy_update,
};
use crate::dev::mii::miidevs::*;
use crate::dev::mii::miivar::{
    MII_DOWN, MII_MEDIACHG, MII_POLLSTAT, MII_TICK, MiiAttachArgs, MiiData, MiiPhyFuncs,
    MiiPhydesc, MiiSoftc, mii_model, mii_oui, mii_rev,
};
use crate::dev::mii::ukphy_subr::ukphy_status;
use crate::kern::subr_prf::printf;
use crate::net::if_::IFF_UP;
use crate::net::if_media::{
    IFM_10_T, IFM_100_TX, IFM_ACTIVE, IFM_AVALID, IFM_ETHER, IFM_FDX, IFM_HDX, IFM_LOOP, IFM_NONE,
    ifm_inst,
};
use crate::sys::device::{CfMatch, Cfattach, Cfdriver, DV_DULL, DVF_ACTIVE, Device};
use crate::sys::errno::Errno;

/// `lxtphy_ca`.
pub static LXTPHY_CA: Cfattach = Cfattach {
    ca_devsize: size_of::<MiiSoftc>(),
    ca_match: Some(lxtphymatch),
    ca_attach: lxtphyattach,
    ca_detach: Some(mii_phy_detach),
    ca_activate: None,
};

/// `lxtphy_cd`.
pub static LXTPHY_CD: Cfdriver = Cfdriver::new(b"lxtphy", DV_DULL, 0);

/// `lxtphy_funcs`: the LXT970's.
pub static LXTPHY_FUNCS: MiiPhyFuncs = MiiPhyFuncs {
    pf_service: lxtphy_service,
    pf_status: lxtphy_status,
    pf_reset: lxtphy_reset,
};

/// `lxtphy971_funcs`: the LXT971's, with the generic status.
pub static LXTPHY971_FUNCS: MiiPhyFuncs = MiiPhyFuncs {
    pf_service: lxtphy_service,
    pf_status: ukphy_status,
    pf_reset: lxtphy_reset,
};

/// `lxtphys[]`.
static LXTPHYS: [MiiPhydesc; 2] = [
    MiiPhydesc {
        mpd_oui: MII_OUI_xxLEVEL1,
        mpd_model: MII_MODEL_xxLEVEL1_LXT970,
        mpd_name: MII_STR_xxLEVEL1_LXT970,
    },
    MiiPhydesc {
        mpd_oui: MII_OUI_xxLEVEL1a,
        mpd_model: MII_MODEL_xxLEVEL1a_LXT971,
        mpd_name: MII_STR_xxLEVEL1a_LXT971,
    },
];

/// `lxtphymatch`.
pub fn lxtphymatch(_parent: Option<&Device>, _match: &CfMatch, aux: *mut c_void) -> i32 {
    // SAFETY: `mii_attach` hands its children a `struct mii_attach_args`.
    let ma = unsafe { &*aux.cast::<MiiAttachArgs>() };

    if mii_phy_match(ma, &LXTPHYS).is_some() {
        return 10;
    }

    0
}

/// `lxtphyattach`.
pub fn lxtphyattach(_parent: Option<&Device>, self_: &Device, aux: *mut c_void) {
    // SAFETY: `lxtphy_ca`'s softc is a `struct mii_softc`; softcs are never freed while their
    // devices exist.
    let sc: &'static MiiSoftc = unsafe { &*ptr::from_ref(self_.softc::<MiiSoftc>()) };
    // SAFETY: `mii_attach` hands its children a `struct mii_attach_args`.
    let ma = unsafe { &*aux.cast::<MiiAttachArgs>() };
    let mii: &'static MiiData = ma.mii_data;

    let oui = mii_oui(ma.mii_id1, ma.mii_id2) as u32;
    let model = mii_model(ma.mii_id2) as u32;
    if oui == MII_OUI_xxLEVEL1 && model == MII_MODEL_xxLEVEL1_LXT970 {
        sc.mii_funcs.set(Some(&LXTPHY_FUNCS));
    }
    if oui == MII_OUI_xxLEVEL1a && model == MII_MODEL_xxLEVEL1a_LXT971 {
        sc.mii_funcs.set(Some(&LXTPHY971_FUNCS));
    }

    // lxtphymatch let only the two models through.
    let name = mii_phy_match(ma, &LXTPHYS).map_or("", |mpd| mpd.mpd_name);
    printf(format_args!(": {}, rev. {}\n", name, mii_rev(ma.mii_id2)));

    sc.mii_inst.set(mii.mii_instance.get());
    sc.mii_phy.set(ma.mii_phyno);
    sc.mii_pdata.set(Some(mii));
    sc.mii_flags.set(ma.mii_flags);

    sc.phy_reset();

    sc.mii_capabilities
        .set(sc.phy_read(MII_BMSR) & ma.mii_capmask);
    if sc.mii_capabilities.get() & BMSR_MEDIAMASK != 0 {
        mii_phy_add_media(sc);
    }
}

/// `lxtphy_service`.
pub fn lxtphy_service(sc: &'static MiiSoftc, mii: &'static MiiData, cmd: i32) -> Result<(), Errno> {
    if sc.mii_dev.dv_flags.get() & DVF_ACTIVE == 0 {
        return Err(Errno::ENXIO);
    }

    match cmd {
        MII_POLLSTAT => {
            // If we're not polling our PHY instance, just return.
            if ifm_inst(mii.cur_media()) != sc.mii_inst.get() {
                return Ok(());
            }
        }

        MII_MEDIACHG => {
            // If the media indicates a different PHY instance, isolate ourselves.
            if ifm_inst(mii.cur_media()) != sc.mii_inst.get() {
                let reg = sc.phy_read(MII_BMCR);
                sc.phy_write(MII_BMCR, reg | BMCR_ISO);
                return Ok(());
            }

            // If the interface is not up, don't do anything.
            if mii.ifp().if_flags.get() & IFF_UP != 0 {
                mii_phy_setmedia(sc);
            }
        }

        MII_TICK => {
            // If we're not currently selected, just return.
            if ifm_inst(mii.cur_media()) != sc.mii_inst.get() {
                return Ok(());
            }

            if mii_phy_tick(sc) == Err(Errno::EJUSTRETURN) {
                return Ok(());
            }
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

/// The media an LXT970 has resolved, from its chip status register `csr` and its BMCR and
/// BMSR (`bmsr` is read only when autonegotiation is on): `(status, active)`, what
/// `lxtphy_status` stores (`active` is `None` for "the selected media").
pub fn lxtphy_media(csr: i32, bmcr: i32, bmsr: impl FnOnce() -> i32) -> (u64, Option<u64>) {
    let mut status = IFM_AVALID;
    let mut active = IFM_ETHER;

    // Get link status from the CSR; we need to read the CSR for media type anyhow, and the
    // link status in the CSR doesn't latch, so fewer register reads are required.
    if csr & CSR_LINK != 0 {
        status |= IFM_ACTIVE;
    }

    if bmcr & BMCR_ISO != 0 {
        return (0, Some(active | IFM_NONE));
    }

    if bmcr & BMCR_LOOP != 0 {
        active |= IFM_LOOP;
    }

    if bmcr & BMCR_AUTOEN == 0 {
        return (status, None);
    }

    if bmsr() & BMSR_ACOMP == 0 {
        // Erg, still trying, I guess...
        return (status, Some(active | IFM_NONE));
    }
    active |= if csr & CSR_SPEED != 0 {
        IFM_100_TX
    } else {
        IFM_10_T
    };
    active |= if csr & CSR_DUPLEX != 0 {
        IFM_FDX
    } else {
        IFM_HDX
    };
    (status, Some(active))
}

/// `lxtphy_status`.
pub fn lxtphy_status(sc: &'static MiiSoftc) {
    let mii = sc.pdata();

    let csr = sc.phy_read(MII_LXTPHY_CSR);
    let bmcr = sc.phy_read(MII_BMCR);
    let (status, active) =
        lxtphy_media(csr, bmcr, || sc.phy_read(MII_BMSR) | sc.phy_read(MII_BMSR));

    mii.mii_media_status.set(status);
    mii.mii_media_active
        .set(active.unwrap_or_else(|| mii.cur_media()));
}

/// `lxtphy_reset`: the generic reset, then the PHY's interrupt off.
pub fn lxtphy_reset(sc: &'static MiiSoftc) {
    mii_phy_reset(sc);
    sc.phy_write(MII_LXTPHY_IER, sc.phy_read(MII_LXTPHY_IER) & !IER_INTEN);
}
/* </CODE> */

/* <TESTS> */
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn media_from_the_chip_status() {
        use crate::dev::mii::lxtphyreg::CSR_ACOMP;

        // Autonegotiated 100baseTX full duplex with link, as QEMU's tulip reports it.
        let (st, act) = lxtphy_media(
            CSR_LINK | CSR_SPEED | CSR_DUPLEX | CSR_ACOMP,
            BMCR_AUTOEN,
            || BMSR_ACOMP,
        );
        assert_eq!(st, IFM_AVALID | IFM_ACTIVE);
        assert_eq!(act, Some(IFM_ETHER | IFM_100_TX | IFM_FDX));

        // Still negotiating.
        let (st, act) = lxtphy_media(0, BMCR_AUTOEN, || 0);
        assert_eq!(st, IFM_AVALID);
        assert_eq!(act, Some(IFM_ETHER | IFM_NONE));

        // Isolated: no status at all.
        let (st, act) = lxtphy_media(CSR_LINK, BMCR_ISO, || panic!("BMSR read"));
        assert_eq!((st, act), (0, Some(IFM_ETHER | IFM_NONE)));

        // Manual media: the selected one, BMSR not read.
        let (st, act) = lxtphy_media(CSR_LINK, BMCR_LOOP, || panic!("BMSR read"));
        assert_eq!((st, act), (IFM_AVALID | IFM_ACTIVE, None));

        // 10baseT half duplex.
        let (_, act) = lxtphy_media(CSR_LINK, BMCR_AUTOEN | BMCR_LOOP, || BMSR_ACOMP);
        assert_eq!(act, Some(IFM_ETHER | IFM_LOOP | IFM_10_T | IFM_HDX));
    }

    #[test]
    fn matches_only_the_two_level_one_models() {
        assert_eq!(LXTPHYS[0].mpd_name, "LXT970");
        assert_eq!(LXTPHYS[1].mpd_name, "LXT971");
    }
}
/* </TESTS> */
