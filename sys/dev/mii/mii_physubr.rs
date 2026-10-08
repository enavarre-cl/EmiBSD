/*	$OpenBSD: mii_physubr.c,v 1.46 2020/01/15 00:14:47 cheloha Exp $	*/
/*	$NetBSD: mii_physubr.c,v 1.20 2001/04/13 23:30:09 thorpej Exp $	*/
/* <LICENSES> */
/*-
 * Copyright (c) 1998, 1999, 2000 The NetBSD Foundation, Inc.
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
//! `dev/mii/mii_physubr.c`: subroutines common to all PHYs (media setting,
//! autonegotiation, the tick, reset, status reporting and the generic media list).
//!
//! Upstream: sys/dev/mii/mii_physubr.c @ 3ce1f3f79392
//!
//! ## Deviations
//! - Functions returning 0 or an errno return `Result<(), Errno>` (`mii_phy_auto`,
//!   `mii_phy_tick`, `mii_phy_detach`); `mii_phy_auto`'s `waitfor` and
//!   `mii_phy_statusmsg`'s result are `bool`s.
//! - `mii_phy_setmedia`'s `DIAGNOSTIC` check of the table index is under feature
//!   `diagnostic`; its "MASTER on wrong media" panic is the C's.
//! - `mii_phy_match` takes a slice (no `{ 0, 0, NULL }` terminator) and returns
//!   `Option<&MiiPhydesc>`.
//! - `mii_phy_auto_timeout`'s argument is the PHY's softc, as in C; `mii_phy_auto` sets the
//!   timeout up with it.

use core::ffi::c_void;
use core::ptr;

use crate::dev::mii::mii::*;
use crate::dev::mii::miivar::{
    MII_ANEGTICKS, MII_ANEGTICKS_GIGE, MII_MEDIA_10_T, MII_MEDIA_10_T_FDX, MII_MEDIA_100_T4,
    MII_MEDIA_100_TX, MII_MEDIA_100_TX_FDX, MII_MEDIA_1000_T, MII_MEDIA_1000_T_FDX,
    MII_MEDIA_1000_X, MII_MEDIA_1000_X_FDX, MII_MEDIA_NONE, MII_MEDIACHG, MII_NMEDIA, MII_POLLSTAT,
    MIIF_AUTOTSLEEP, MIIF_DOINGAUTO, MIIF_DOPAUSE, MIIF_FORCEANEG, MIIF_HAVE_GTCR, MIIF_IS_1000X,
    MIIF_NOISOLATE, MiiAttachArgs, MiiMedia, MiiPhydesc, MiiSoftc, mii_model, mii_oui,
};
use crate::kern::kern_synch::tsleep_nsec;
use crate::kern::kern_timeout::{timeout_add_msec, timeout_del, timeout_set};
use crate::kern::subr_prf::panic;
use crate::machine::cpu::delay;
use crate::machine::intr::{splnet, splx};
use crate::net::if_::{
    IFF_UP, LINK_STATE_DOWN, LINK_STATE_FULL_DUPLEX, LINK_STATE_HALF_DUPLEX, LINK_STATE_UNKNOWN,
    if_link_state_change,
};
use crate::net::if_media::{
    IFM_10_T, IFM_100_T4, IFM_100_TX, IFM_1000_SX, IFM_1000_T, IFM_ACTIVE, IFM_AUTO, IFM_AVALID,
    IFM_ETH_MASTER, IFM_ETH_RXPAUSE, IFM_ETH_TXPAUSE, IFM_ETHER, IFM_FDX, IFM_FLOW, IFM_LOOP,
    IFM_NMASK, IFM_NONE, IFM_TMASK, ifm_makeword, ifm_subtype, ifmedia_add, ifmedia_baudrate,
    ifmedia_delete_instance,
};
use crate::sys::device::{DVF_ACTIVE, Device};
use crate::sys::errno::Errno;
use crate::sys::param::PZERO;
use crate::sys::time::msec_to_nsec;

/// `mii_media_table[]`: media to register setting conversion table. Order matters.
/// XXX 802.3 doesn't specify ANAR or ANLPAR bits for 1000base.
pub static MII_MEDIA_TABLE: [MiiMedia; MII_NMEDIA as usize] = [
    // None
    MiiMedia {
        mm_bmcr: BMCR_ISO,
        mm_anar: ANAR_CSMA,
        mm_gtcr: 0,
    },
    // 10baseT
    MiiMedia {
        mm_bmcr: BMCR_S10,
        mm_anar: ANAR_CSMA | ANAR_10,
        mm_gtcr: 0,
    },
    // 10baseT-FDX
    MiiMedia {
        mm_bmcr: BMCR_S10 | BMCR_FDX,
        mm_anar: ANAR_CSMA | ANAR_10_FD,
        mm_gtcr: 0,
    },
    // 100baseT4
    MiiMedia {
        mm_bmcr: BMCR_S100,
        mm_anar: ANAR_CSMA | ANAR_T4,
        mm_gtcr: 0,
    },
    // 100baseTX
    MiiMedia {
        mm_bmcr: BMCR_S100,
        mm_anar: ANAR_CSMA | ANAR_TX,
        mm_gtcr: 0,
    },
    // 100baseTX-FDX
    MiiMedia {
        mm_bmcr: BMCR_S100 | BMCR_FDX,
        mm_anar: ANAR_CSMA | ANAR_TX_FD,
        mm_gtcr: 0,
    },
    // 1000baseX
    MiiMedia {
        mm_bmcr: BMCR_S1000,
        mm_anar: ANAR_CSMA,
        mm_gtcr: 0,
    },
    // 1000baseX-FDX
    MiiMedia {
        mm_bmcr: BMCR_S1000 | BMCR_FDX,
        mm_anar: ANAR_CSMA,
        mm_gtcr: 0,
    },
    // 1000baseT
    MiiMedia {
        mm_bmcr: BMCR_S1000,
        mm_anar: ANAR_CSMA,
        mm_gtcr: GTCR_ADV_1000THDX,
    },
    // 1000baseT-FDX
    MiiMedia {
        mm_bmcr: BMCR_S1000 | BMCR_FDX,
        mm_anar: ANAR_CSMA,
        mm_gtcr: GTCR_ADV_1000TFDX,
    },
];

/// `mii_phy_setmedia`: programs the PHY for the selected media (autonegotiation, or the
/// table entry whose index the media entry holds).
pub fn mii_phy_setmedia(sc: &'static MiiSoftc) {
    let mii = sc.pdata();
    let ife_media = mii.cur_media();
    let ife_data = mii.cur_data();

    if ifm_subtype(ife_media) == IFM_AUTO {
        if (sc.phy_read(MII_BMCR) & BMCR_AUTOEN) == 0 || (sc.mii_flags.get() & MIIF_FORCEANEG) != 0
        {
            let _ = mii_phy_auto(sc, true);
        }
        return;
    }

    // Table index is stored in the media entry.
    #[cfg(feature = "diagnostic")]
    if ife_data >= MII_NMEDIA {
        panic(format_args!("mii_phy_setmedia"));
    }
    let Some(mm) = MII_MEDIA_TABLE.get(ife_data as usize) else {
        // Without DIAGNOSTIC the C reads past the table.
        panic(format_args!("mii_phy_setmedia: bad media index {ife_data}"));
    };

    let anar = mm.mm_anar;
    let mut bmcr = mm.mm_bmcr;
    let mut gtcr = mm.mm_gtcr;

    if mii.mii_media.ifm_media.get() & IFM_ETH_MASTER != 0 {
        match ifm_subtype(ife_media) {
            IFM_1000_T => gtcr |= GTCR_MAN_MS | GTCR_ADV_MS,
            _ => panic(format_args!("mii_phy_setmedia: MASTER on wrong media")),
        }
    }

    if ife_media & IFM_LOOP != 0 {
        bmcr |= BMCR_LOOP;
    }

    sc.phy_write(MII_ANAR, anar);
    sc.phy_write(MII_BMCR, bmcr);
    if sc.mii_flags.get() & MIIF_HAVE_GTCR != 0 {
        sc.phy_write(MII_100T2CR, gtcr);
    }
}

/// `mii_phy_auto`: starts autonegotiation; with `waitfor`, waits up to 500ms for it (`EIO`
/// when it did not complete), otherwise lets it finish asynchronously (`EJUSTRETURN`).
pub fn mii_phy_auto(sc: &'static MiiSoftc, waitfor: bool) -> Result<(), Errno> {
    let flags = sc.mii_flags.get();
    let ext = sc.mii_extcapabilities.get();

    if flags & MIIF_DOINGAUTO == 0 {
        // Check for 1000BASE-X. Autonegotiation is a bit different on such devices.
        if flags & MIIF_IS_1000X != 0 {
            let mut anar = 0;

            if ext & EXTSR_1000XFDX != 0 {
                anar |= ANAR_X_FD;
            }
            if ext & EXTSR_1000XHDX != 0 {
                anar |= ANAR_X_HD;
            }

            if flags & MIIF_DOPAUSE != 0 && ext & EXTSR_1000XFDX != 0 {
                anar |= ANAR_X_PAUSE_TOWARDS;
            }

            sc.phy_write(MII_ANAR, anar);
        } else {
            let mut anar = bmsr_media_to_anar(sc.mii_capabilities.get()) | ANAR_CSMA;
            // Most 100baseTX PHY's only support symmetric PAUSE, so we don't advertise
            // asymmetric PAUSE unless we also have 1000baseT capability.
            if flags & MIIF_DOPAUSE != 0 {
                if sc.mii_capabilities.get() & BMSR_100TXFDX != 0 {
                    anar |= ANAR_FC;
                }
                if ext & EXTSR_1000TFDX != 0 {
                    anar |= ANAR_PAUSE_TOWARDS;
                }
            }
            // The C's `uint16_t anar`.
            sc.phy_write(MII_ANAR, anar & 0xffff);
            if flags & MIIF_HAVE_GTCR != 0 {
                let mut gtcr = 0;

                if ext & EXTSR_1000TFDX != 0 {
                    gtcr |= GTCR_ADV_1000TFDX;
                }
                if ext & EXTSR_1000THDX != 0 {
                    gtcr |= GTCR_ADV_1000THDX;
                }

                sc.phy_write(MII_100T2CR, gtcr);
            }
        }
        sc.phy_write(MII_BMCR, BMCR_AUTOEN | BMCR_STARTNEG);
    }

    if waitfor {
        // Wait 500ms for it to complete.
        for _ in 0..500 {
            if sc.phy_read(MII_BMSR) & BMSR_ACOMP != 0 {
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
    if sc.mii_flags.get() & MIIF_AUTOTSLEEP != 0 {
        sc.mii_flags.set(sc.mii_flags.get() | MIIF_DOINGAUTO);
        let _ = tsleep_nsec(sc.mii_flags.as_ptr(), PZERO, "miiaut", msec_to_nsec(500));
        mii_phy_auto_timeout(ptr::from_ref(sc).cast_mut().cast());
    } else if sc.mii_flags.get() & MIIF_DOINGAUTO == 0 {
        sc.mii_flags.set(sc.mii_flags.get() | MIIF_DOINGAUTO);
        timeout_set(
            &sc.mii_phy_timo,
            mii_phy_auto_timeout,
            ptr::from_ref(sc).cast_mut().cast(),
        );
        timeout_add_msec(&sc.mii_phy_timo, 500);
    }
    Err(Errno::EJUSTRETURN)
}

/// `mii_phy_auto_timeout`: autonegotiation had its time; update the media status.
pub fn mii_phy_auto_timeout(arg: *mut c_void) {
    // SAFETY: `mii_phy_auto` passes its PHY's softc, never freed while the device exists.
    let sc: &'static MiiSoftc = unsafe { &*arg.cast::<MiiSoftc>() };

    if sc.mii_dev.dv_flags.get() & DVF_ACTIVE == 0 {
        return;
    }

    let s = splnet();
    sc.mii_flags.set(sc.mii_flags.get() & !MIIF_DOINGAUTO);
    let _bmsr = sc.phy_read(MII_BMSR);

    // Update the media status.
    let _ = sc.phy_service(sc.pdata(), MII_POLLSTAT);
    splx(s);
}

/// `mii_phy_tick`: the once-per-second work of an autonegotiating PHY; `EJUSTRETURN` when
/// the caller should not update the status.
pub fn mii_phy_tick(sc: &'static MiiSoftc) -> Result<(), Errno> {
    let mii = sc.pdata();
    let ife_media = mii.cur_media();

    // Just bail now if the interface is down.
    if mii.ifp().if_flags.get() & IFF_UP == 0 {
        return Err(Errno::EJUSTRETURN);
    }

    // If we're not doing autonegotiation, we don't need to do any extra work here. However,
    // we need to check the link status so we can generate an announcement if the status
    // changes.
    if ifm_subtype(ife_media) != IFM_AUTO {
        return Ok(());
    }

    // Read the status register twice; BMSR_LINK is latch-low.
    let reg = sc.phy_read(MII_BMSR) | sc.phy_read(MII_BMSR);
    if reg & BMSR_LINK != 0 {
        // See above.
        return Ok(());
    }

    // Only retry autonegotiation every mii_anegticks seconds.
    if sc.mii_anegticks.get() == 0 {
        sc.mii_anegticks.set(MII_ANEGTICKS);
    }

    sc.mii_ticks.set(sc.mii_ticks.get() + 1);
    if sc.mii_ticks.get() <= sc.mii_anegticks.get() {
        return Err(Errno::EJUSTRETURN);
    }

    sc.mii_ticks.set(0);
    sc.phy_reset();

    if mii_phy_auto(sc, false) == Err(Errno::EJUSTRETURN) {
        return Err(Errno::EJUSTRETURN);
    }

    // Might need to generate a status message if autonegotiation failed.
    Ok(())
}

/// `mii_phy_reset`: resets the PHY and waits for it, isolating it unless it is instance 0
/// or may not be isolated.
pub fn mii_phy_reset(sc: &'static MiiSoftc) {
    let mut reg = if sc.mii_flags.get() & MIIF_NOISOLATE != 0 {
        BMCR_RESET
    } else {
        BMCR_RESET | BMCR_ISO
    };
    sc.phy_write(MII_BMCR, reg);

    // It is best to allow a little time for the reset to settle in before we start polling
    // the BMCR again. Notably, the DP83840A manual states that there should be a 500us delay
    // between asserting software reset and attempting MII serial operations. Also, a DP83815
    // can get into a bad state on cable removal and reinsertion if we do not delay here.
    delay(500);

    // Wait another 100ms for it to complete.
    for _ in 0..100 {
        reg = sc.phy_read(MII_BMCR);
        if reg & BMCR_RESET == 0 {
            break;
        }
        delay(1000);
    }

    if sc.mii_inst.get() != 0 && (sc.mii_flags.get() & MIIF_NOISOLATE) == 0 {
        sc.phy_write(MII_BMCR, reg | BMCR_ISO);
    }
}

/// `mii_phy_down`: the interface went down; stop a pending autonegotiation timeout.
pub fn mii_phy_down(sc: &MiiSoftc) {
    if sc.mii_flags.get() & MIIF_DOINGAUTO != 0 {
        sc.mii_flags.set(sc.mii_flags.get() & !MIIF_DOINGAUTO);
        timeout_del(&sc.mii_phy_timo);
    }
}

/// `mii_phy_status`: `PHY_STATUS(sc)`.
pub fn mii_phy_status(sc: &'static MiiSoftc) {
    sc.phy_status();
}

/// `mii_phy_update`: tells the driver and the stack when the media or its status changed.
pub fn mii_phy_update(sc: &'static MiiSoftc, cmd: i32) {
    let mii = sc.pdata();
    let ifp = mii.ifp();

    if sc.mii_media_active.get() != mii.mii_media_active.get()
        || sc.mii_media_status.get() != mii.mii_media_status.get()
        || cmd == MII_MEDIACHG
    {
        let announce = mii_phy_statusmsg(sc);
        mii.mii_statchg(sc.parent());
        sc.mii_media_active.set(mii.mii_media_active.get());
        sc.mii_media_status.set(mii.mii_media_status.get());

        if announce {
            let s = splnet();
            if_link_state_change(ifp);
            splx(s);
        }
    }
}

/// `mii_phy_statusmsg`: updates the interface's link state and baudrate from the media;
/// whether either changed.
pub fn mii_phy_statusmsg(sc: &MiiSoftc) -> bool {
    let mii = sc.pdata();
    let ifp = mii.ifp();
    let mut announce = false;
    let status = mii.mii_media_status.get();

    let link_state = if status & IFM_AVALID != 0 {
        if status & IFM_ACTIVE != 0 {
            if mii.mii_media_active.get() & IFM_FDX != 0 {
                LINK_STATE_FULL_DUPLEX
            } else {
                LINK_STATE_HALF_DUPLEX
            }
        } else {
            LINK_STATE_DOWN
        }
    } else {
        LINK_STATE_UNKNOWN
    };

    let baudrate = ifmedia_baudrate(mii.mii_media_active.get());

    if link_state != ifp.if_link_state.get() {
        ifp.if_link_state.set(link_state);
        // XXX Right here we'd like to notify protocols
        // XXX that the link status has changed, so that
        // XXX e.g. Duplicate Address Detection can restart.
        announce = true;
    }

    if baudrate != ifp.if_baudrate.get() {
        ifp.if_baudrate.set(baudrate);
        announce = true;
    }

    announce
}

/// `mii_phy_add_media`: initialize generic PHY media based on BMSR, called when a PHY is
/// attached.
pub fn mii_phy_add_media(sc: &MiiSoftc) {
    let mii = sc.pdata();
    let inst = sc.mii_inst.get();
    let caps = sc.mii_capabilities.get();
    let ext = sc.mii_extcapabilities.get();
    let add = |m: u64, c: u32| ifmedia_add(&mii.mii_media, m, c as i32, ptr::null_mut());

    if sc.mii_flags.get() & MIIF_NOISOLATE == 0 {
        add(ifm_makeword(IFM_ETHER, IFM_NONE, 0, inst), MII_MEDIA_NONE);
    }

    if caps & BMSR_10THDX != 0 {
        add(ifm_makeword(IFM_ETHER, IFM_10_T, 0, inst), MII_MEDIA_10_T);
    }
    if caps & BMSR_10TFDX != 0 {
        add(
            ifm_makeword(IFM_ETHER, IFM_10_T, IFM_FDX, inst),
            MII_MEDIA_10_T_FDX,
        );
    }
    if caps & BMSR_100TXHDX != 0 {
        add(
            ifm_makeword(IFM_ETHER, IFM_100_TX, 0, inst),
            MII_MEDIA_100_TX,
        );
    }
    if caps & BMSR_100TXFDX != 0 {
        add(
            ifm_makeword(IFM_ETHER, IFM_100_TX, IFM_FDX, inst),
            MII_MEDIA_100_TX_FDX,
        );
    }
    if caps & BMSR_100T4 != 0 {
        add(
            ifm_makeword(IFM_ETHER, IFM_100_T4, 0, inst),
            MII_MEDIA_100_T4,
        );
    }
    if ext & EXTSR_MEDIAMASK != 0 {
        // XXX Right now only handle 1000SX and 1000TX. Need
        // XXX to handle 1000LX and 1000CX some how.
        if ext & EXTSR_1000XHDX != 0 {
            sc.mii_anegticks.set(MII_ANEGTICKS_GIGE);
            sc.mii_flags.set(sc.mii_flags.get() | MIIF_IS_1000X);
            add(
                ifm_makeword(IFM_ETHER, IFM_1000_SX, 0, inst),
                MII_MEDIA_1000_X,
            );
        }
        if ext & EXTSR_1000XFDX != 0 {
            sc.mii_anegticks.set(MII_ANEGTICKS_GIGE);
            sc.mii_flags.set(sc.mii_flags.get() | MIIF_IS_1000X);
            add(
                ifm_makeword(IFM_ETHER, IFM_1000_SX, IFM_FDX, inst),
                MII_MEDIA_1000_X_FDX,
            );
        }

        // 1000baseT media needs to be able to manipulate master/slave mode. We set
        // IFM_ETH_MASTER in the "don't care mask" and filter it out when the media is set.
        //
        // All 1000baseT PHYs have a 1000baseT control register.
        if ext & EXTSR_1000THDX != 0 {
            sc.mii_anegticks.set(MII_ANEGTICKS_GIGE);
            sc.mii_flags.set(sc.mii_flags.get() | MIIF_HAVE_GTCR);
            let m = &mii.mii_media.ifm_mask;
            m.set(m.get() | IFM_ETH_MASTER);
            add(
                ifm_makeword(IFM_ETHER, IFM_1000_T, 0, inst),
                MII_MEDIA_1000_T,
            );
        }
        if ext & EXTSR_1000TFDX != 0 {
            sc.mii_anegticks.set(MII_ANEGTICKS_GIGE);
            sc.mii_flags.set(sc.mii_flags.get() | MIIF_HAVE_GTCR);
            let m = &mii.mii_media.ifm_mask;
            m.set(m.get() | IFM_ETH_MASTER);
            add(
                ifm_makeword(IFM_ETHER, IFM_1000_T, IFM_FDX, inst),
                MII_MEDIA_1000_T_FDX,
            );
        }
    }

    if caps & BMSR_ANEG != 0 {
        // intentionally invalid index
        add(ifm_makeword(IFM_ETHER, IFM_AUTO, 0, inst), MII_NMEDIA);
    }
}

/// `mii_phy_delete_media`: removes this PHY instance's media from its parent's list.
pub fn mii_phy_delete_media(sc: &MiiSoftc) {
    let mii = sc.pdata();

    ifmedia_delete_instance(&mii.mii_media, sc.mii_inst.get());
}

/// `mii_phy_detach`: the `ca_detach` of the PHY drivers.
pub fn mii_phy_detach(self_: &Device, _flags: i32) -> Result<(), Errno> {
    // SAFETY: the PHY drivers' softcs are `struct mii_softc`s (their `ca_devsize`).
    let sc: &MiiSoftc = unsafe { self_.softc::<MiiSoftc>() };

    if sc.mii_flags.get() & MIIF_DOINGAUTO != 0 {
        timeout_del(&sc.mii_phy_timo);
    }

    mii_phy_delete_media(sc);

    Ok(())
}

/// `mii_phy_match`: the entry of `mpd` that names this PHY's OUI and model.
pub fn mii_phy_match<'a>(ma: &MiiAttachArgs, mpd: &'a [MiiPhydesc]) -> Option<&'a MiiPhydesc> {
    mpd.iter().find(|d| {
        mii_oui(ma.mii_id1, ma.mii_id2) as u32 == d.mpd_oui
            && mii_model(ma.mii_id2) as u32 == d.mpd_model
    })
}

/// `mii_phy_flowstatus`: return the flow control status flag from MII_ANAR & MII_ANLPAR.
pub fn mii_phy_flowstatus(sc: &MiiSoftc) -> u64 {
    if sc.mii_flags.get() & MIIF_DOPAUSE == 0 {
        return 0;
    }

    let mut anar = sc.phy_read(MII_ANAR);
    let mut anlpar = sc.phy_read(MII_ANLPAR);

    // For 1000baseX, the bits are in a different location.
    if sc.mii_flags.get() & MIIF_IS_1000X != 0 {
        anar <<= 3;
        anlpar <<= 3;
    }

    if (anar & ANAR_PAUSE_SYM) & (anlpar & ANLPAR_PAUSE_SYM) != 0 {
        return IFM_FLOW | IFM_ETH_TXPAUSE | IFM_ETH_RXPAUSE;
    }

    if anar & ANAR_PAUSE_SYM == 0 {
        if anar & ANAR_PAUSE_ASYM != 0 && (anlpar & ANLPAR_PAUSE_TOWARDS) == ANLPAR_PAUSE_TOWARDS {
            return IFM_FLOW | IFM_ETH_TXPAUSE;
        } else {
            return 0;
        }
    }

    if anar & ANAR_PAUSE_ASYM == 0 {
        if anlpar & ANLPAR_PAUSE_SYM != 0 {
            return IFM_FLOW | IFM_ETH_TXPAUSE | IFM_ETH_RXPAUSE;
        } else {
            return 0;
        }
    }

    match anlpar & ANLPAR_PAUSE_TOWARDS {
        ANLPAR_PAUSE_NONE => 0,
        ANLPAR_PAUSE_ASYM => IFM_FLOW | IFM_ETH_RXPAUSE,
        _ => IFM_FLOW | IFM_ETH_RXPAUSE | IFM_ETH_TXPAUSE,
    }
}

/// `mii_anar`: given an ifmedia word, return the corresponding ANAR value.
pub fn mii_anar(media: u64) -> i32 {
    const E: u64 = IFM_ETHER;
    match media & (IFM_TMASK | IFM_NMASK | IFM_FDX) {
        m if m == E | IFM_10_T => ANAR_10 | ANAR_CSMA,
        m if m == E | IFM_10_T | IFM_FDX => ANAR_10_FD | ANAR_CSMA,
        m if m == E | IFM_100_TX => ANAR_TX | ANAR_CSMA,
        m if m == E | IFM_100_TX | IFM_FDX => ANAR_TX_FD | ANAR_CSMA,
        m if m == E | IFM_100_T4 => ANAR_T4 | ANAR_CSMA,
        _ => 0,
    }
}
/* </CODE> */

/* <TESTS> */
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn anar_of_media_words() {
        assert_eq!(
            mii_anar(IFM_ETHER | IFM_100_TX | IFM_FDX),
            ANAR_TX_FD | ANAR_CSMA
        );
        assert_eq!(mii_anar(IFM_ETHER | IFM_10_T), ANAR_10 | ANAR_CSMA);
        assert_eq!(mii_anar(IFM_ETHER | IFM_100_T4), ANAR_T4 | ANAR_CSMA);
        assert_eq!(mii_anar(IFM_ETHER | IFM_AUTO), 0);
        assert_eq!(mii_anar(IFM_ETHER | IFM_1000_T | IFM_FDX), 0);
    }

    #[test]
    fn table_rows_follow_the_media_indices() {
        let t = &MII_MEDIA_TABLE;
        assert_eq!(t[MII_MEDIA_NONE as usize].mm_bmcr, BMCR_ISO);
        assert_eq!(
            t[MII_MEDIA_100_TX_FDX as usize].mm_bmcr,
            BMCR_S100 | BMCR_FDX
        );
        assert_eq!(t[MII_MEDIA_1000_T_FDX as usize].mm_gtcr, GTCR_ADV_1000TFDX);
    }

    #[test]
    fn phy_match_compares_oui_and_model() {
        static T: [MiiPhydesc; 1] = [MiiPhydesc {
            mpd_oui: 0x20,
            mpd_model: 0x20,
            mpd_name: "RTL8201L",
        }];
        let mii: &'static crate::dev::mii::miivar::MiiData =
            std::boxed::Box::leak(std::boxed::Box::new(crate::dev::mii::miivar::MiiData::new()));
        let mut ma = MiiAttachArgs {
            mii_data: mii,
            mii_phyno: 0,
            mii_id1: 0,
            mii_id2: 0x8201,
            mii_capmask: -1,
            mii_flags: 0,
        };
        assert_eq!(mii_phy_match(&ma, &T).map(|d| d.mpd_name), Some("RTL8201L"));
        ma.mii_id2 = 0;
        assert!(mii_phy_match(&ma, &T).is_none());
    }
}
/* </TESTS> */
