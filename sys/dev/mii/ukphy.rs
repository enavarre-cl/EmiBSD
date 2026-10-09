/* <LICENSES> */
/*-
 * Copyright (c) 1998, 1999 The NetBSD Foundation, Inc.
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
//! `dev/mii/ukphy.c`: driver for generic unknown PHYs (`ukphy* at mii?`), which matches any
//! PHY at the lowest priority.
//!
//! Upstream: sys/dev/mii/ukphy.c @ 3ce1f3f79392
//!
//! ## Deviations
//! - `ukphy_service` returns `Result<(), Errno>`; it reads the selected media through
//!   `MiiData::cur_media` (the C dereferences `ifm_cur`).

use core::ffi::c_void;

use crate::dev::mii::mii::{BMCR_ISO, BMSR_EXTSTAT, BMSR_MEDIAMASK, EXTSR_MEDIAMASK, MII_BMCR};
use crate::dev::mii::mii::{MII_BMSR, MII_EXTSR};
use crate::dev::mii::mii_physubr::{
    mii_phy_add_media, mii_phy_detach, mii_phy_down, mii_phy_reset, mii_phy_setmedia,
    mii_phy_status, mii_phy_tick, mii_phy_update,
};
use crate::dev::mii::miivar::{
    MII_DOWN, MII_MEDIACHG, MII_POLLSTAT, MII_TICK, MIIF_NOLOOP, MiiAttachArgs, MiiData,
    MiiPhyFuncs, MiiSoftc, mii_model, mii_oui, mii_rev,
};
use crate::dev::mii::ukphy_subr::ukphy_status;
use crate::kern::subr_prf::printf;
use crate::net::if_::IFF_UP;
use crate::net::if_media::ifm_inst;
use crate::sys::device::{CfMatch, Cfattach, Cfdriver, DV_DULL, DVF_ACTIVE, Device};
use crate::sys::errno::Errno;

/// `ukphy_ca`.
pub static UKPHY_CA: Cfattach = Cfattach {
    ca_devsize: size_of::<MiiSoftc>(),
    ca_match: Some(ukphymatch),
    ca_attach: ukphyattach,
    ca_detach: Some(mii_phy_detach),
    ca_activate: None,
};

/// `ukphy_cd`.
pub static UKPHY_CD: Cfdriver = Cfdriver::new(b"ukphy", DV_DULL, 0);

/// `ukphy_funcs`.
pub static UKPHY_FUNCS: MiiPhyFuncs = MiiPhyFuncs {
    pf_service: ukphy_service,
    pf_status: ukphy_status,
    pf_reset: mii_phy_reset,
};

/// `ukphymatch`: we know something is here, so always match at a low priority.
pub fn ukphymatch(_parent: Option<&Device>, _match: &CfMatch, _aux: *mut c_void) -> i32 {
    1
}

/// `ukphyattach`.
pub fn ukphyattach(_parent: Option<&Device>, self_: &Device, aux: *mut c_void) {
    // SAFETY: `ukphy_ca`'s softc is a `struct mii_softc`; softcs are never freed while their
    // devices exist.
    let sc: &'static MiiSoftc = unsafe { &*core::ptr::from_ref(self_.softc::<MiiSoftc>()) };
    // SAFETY: `mii_attach` hands its children a `struct mii_attach_args`.
    let ma = unsafe { &*aux.cast::<MiiAttachArgs>() };
    let mii: &'static MiiData = ma.mii_data;

    printf(format_args!(
        ": Generic IEEE 802.3u media interface, rev. {}:",
        mii_rev(ma.mii_id2)
    ));
    printf(format_args!(
        " OUI 0x{:06x}, model 0x{:04x}\n",
        mii_oui(ma.mii_id1, ma.mii_id2),
        mii_model(ma.mii_id2)
    ));

    sc.mii_inst.set(mii.mii_instance.get());
    sc.mii_phy.set(ma.mii_phyno);
    sc.mii_funcs.set(Some(&UKPHY_FUNCS));
    sc.mii_oui.set(mii_oui(ma.mii_id1, ma.mii_id2));
    sc.mii_model.set(mii_model(ma.mii_id2));
    sc.mii_pdata.set(Some(mii));
    sc.mii_flags.set(ma.mii_flags);

    // Don't do loopback on unknown PHYs. It might confuse some of them.
    sc.mii_flags.set(sc.mii_flags.get() | MIIF_NOLOOP);

    sc.phy_reset();

    sc.mii_capabilities
        .set(sc.phy_read(MII_BMSR) & ma.mii_capmask);
    if sc.mii_capabilities.get() & BMSR_EXTSTAT != 0 {
        sc.mii_extcapabilities.set(sc.phy_read(MII_EXTSR));
    }
    if (sc.mii_capabilities.get() & BMSR_MEDIAMASK) == 0
        && (sc.mii_extcapabilities.get() & EXTSR_MEDIAMASK) == 0
    {
        printf(format_args!("{}: no media present\n", sc.mii_dev.xname()));
    } else {
        mii_phy_add_media(sc);
    }
}

/// `ukphy_service`.
pub fn ukphy_service(sc: &'static MiiSoftc, mii: &'static MiiData, cmd: i32) -> Result<(), Errno> {
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
/* </CODE> */
