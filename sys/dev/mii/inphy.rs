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
//! `dev/mii/inphy.c`: driver for Intel's i82555 ethernet 10/100 PHY (`inphy* at mii?`), the
//! one of the EtherExpress PRO/100 (fxp(4)) and so of QEMU's `i82559er`; the 82562EM, 82562ET
//! and 82562G are the same PHY with a few extra registers.
//!
//! Upstream: sys/dev/mii/inphy.c @ 3ce1f3f79392
//!
//! ## Deviations
//! - `inphys[]` is a slice without the `{ 0, 0, NULL }` terminator.
//! - `inphy_service` returns `Result<(), Errno>`; it reads the selected media through
//!   `MiiData::cur_media` (the C dereferences `ifm_cur`).
//! - `inphyattach` panics if `mii_phy_match` finds nothing, where the C would dereference
//!   NULL; `inphymatch` returned 10 only for a match.

use core::ffi::c_void;

use crate::dev::mii::inphyreg::{MII_INPHY_SCR, SCR_FDX, SCR_S100, SCR_T4};
use crate::dev::mii::mii::{
    BMCR_AUTOEN, BMCR_ISO, BMCR_LOOP, BMSR_100T4, BMSR_ACOMP, BMSR_LINK, BMSR_MEDIAMASK, MII_BMCR,
    MII_BMSR,
};
use crate::dev::mii::mii_physubr::{
    mii_phy_add_media, mii_phy_detach, mii_phy_down, mii_phy_match, mii_phy_reset,
    mii_phy_setmedia, mii_phy_status, mii_phy_tick, mii_phy_update,
};
use crate::dev::mii::miidevs::*;
use crate::dev::mii::miivar::{
    MII_DOWN, MII_MEDIACHG, MII_POLLSTAT, MII_TICK, MiiAttachArgs, MiiData, MiiPhyFuncs,
    MiiPhydesc, MiiSoftc, mii_rev,
};
use crate::kern::subr_prf::{panic, printf};
use crate::net::if_::IFF_UP;
use crate::net::if_media::{
    IFM_10_T, IFM_100_T4, IFM_100_TX, IFM_ACTIVE, IFM_AVALID, IFM_ETHER, IFM_FDX, IFM_HDX,
    IFM_LOOP, IFM_NONE, ifm_inst,
};
use crate::sys::device::{CfMatch, Cfattach, Cfdriver, DV_DULL, DVF_ACTIVE, Device};
use crate::sys::errno::Errno;

/// `inphy_ca`.
pub static INPHY_CA: Cfattach = Cfattach {
    ca_devsize: size_of::<MiiSoftc>(),
    ca_match: Some(inphymatch),
    ca_attach: inphyattach,
    ca_detach: Some(mii_phy_detach),
    ca_activate: None,
};

/// `inphy_cd`.
pub static INPHY_CD: Cfdriver = Cfdriver::new(b"inphy", DV_DULL, 0);

/// `inphy_funcs`.
pub static INPHY_FUNCS: MiiPhyFuncs = MiiPhyFuncs {
    pf_service: inphy_service,
    pf_status: inphy_status,
    pf_reset: mii_phy_reset,
};

/// `inphys[]`.
static INPHYS: [MiiPhydesc; 4] = [
    MiiPhydesc {
        mpd_oui: MII_OUI_INTEL,
        mpd_model: MII_MODEL_INTEL_I82555,
        mpd_name: MII_STR_INTEL_I82555,
    },
    MiiPhydesc {
        mpd_oui: MII_OUI_INTEL,
        mpd_model: MII_MODEL_INTEL_I82562EM,
        mpd_name: MII_STR_INTEL_I82562EM,
    },
    MiiPhydesc {
        mpd_oui: MII_OUI_INTEL,
        mpd_model: MII_MODEL_INTEL_I82562ET,
        mpd_name: MII_STR_INTEL_I82562ET,
    },
    MiiPhydesc {
        mpd_oui: MII_OUI_INTEL,
        mpd_model: MII_MODEL_INTEL_I82562G,
        mpd_name: MII_STR_INTEL_I82562G,
    },
];

/// `inphymatch`.
pub fn inphymatch(_parent: Option<&Device>, _match: &CfMatch, aux: *mut c_void) -> i32 {
    // SAFETY: `mii_attach` hands its children a `struct mii_attach_args`.
    let ma = unsafe { &*aux.cast::<MiiAttachArgs>() };

    if mii_phy_match(ma, &INPHYS).is_some() {
        return 10;
    }

    0
}

/// `inphyattach`.
pub fn inphyattach(_parent: Option<&Device>, self_: &Device, aux: *mut c_void) {
    // SAFETY: `inphy_ca`'s softc is a `struct mii_softc`; softcs are never freed while their
    // devices exist.
    let sc: &'static MiiSoftc = unsafe { &*core::ptr::from_ref(self_.softc::<MiiSoftc>()) };
    // SAFETY: `mii_attach` hands its children a `struct mii_attach_args`.
    let ma = unsafe { &*aux.cast::<MiiAttachArgs>() };
    let mii: &'static MiiData = ma.mii_data;

    let Some(mpd) = mii_phy_match(ma, &INPHYS) else {
        panic(format_args!("inphyattach: no match"));
    };
    printf(format_args!(
        ": {}, rev. {}\n",
        mpd.mpd_name,
        mii_rev(ma.mii_id2)
    ));

    sc.mii_inst.set(mii.mii_instance.get());
    sc.mii_phy.set(ma.mii_phyno);
    sc.mii_funcs.set(Some(&INPHY_FUNCS));
    sc.mii_pdata.set(Some(mii));
    sc.mii_flags.set(ma.mii_flags);

    sc.phy_reset();

    sc.mii_capabilities
        .set(sc.phy_read(MII_BMSR) & ma.mii_capmask);
    if sc.mii_capabilities.get() & BMSR_MEDIAMASK != 0 {
        mii_phy_add_media(sc);
    }
}

/// `inphy_service`.
pub fn inphy_service(sc: &'static MiiSoftc, mii: &'static MiiData, cmd: i32) -> Result<(), Errno> {
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

/// `inphy_status`.
pub fn inphy_status(sc: &'static MiiSoftc) {
    let mii = sc.pdata();
    let or_active = |w: u64| mii.mii_media_active.set(mii.mii_media_active.get() | w);

    mii.mii_media_status.set(IFM_AVALID);
    mii.mii_media_active.set(IFM_ETHER);

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
        if bmsr & BMSR_ACOMP == 0 {
            // Erg, still trying, I guess...
            or_active(IFM_NONE);
            return;
        }

        let scr = sc.phy_read(MII_INPHY_SCR as i32);
        if scr & i32::from(SCR_S100) != 0 {
            or_active(IFM_100_TX);
        } else if bmsr & BMSR_100T4 != 0 && scr & i32::from(SCR_T4) != 0 {
            or_active(IFM_100_T4);
        } else {
            or_active(IFM_10_T);
        }

        if scr & i32::from(SCR_FDX) != 0 {
            or_active(IFM_FDX);
        } else {
            or_active(IFM_HDX);
        }
    } else {
        mii.mii_media_active.set(mii.cur_media());
    }
}
/* </CODE> */
