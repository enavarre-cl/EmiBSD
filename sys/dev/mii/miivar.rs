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
//! `<dev/mii/miivar.h>`: Media Independent Interface autoconfiguration definitions, the
//! interface between a network interface driver and the MII layer (`struct mii_data`), a PHY
//! driver's softc head (`struct mii_softc`), the attach arguments and the PHY tables. It
//! attempts to be compatible with the BSD/OS 3.0 interface.
//!
//! Upstream: sys/dev/mii/miivar.h @ 3ce1f3f79392
//!
//! ## Deviations
//! - `struct mii_data` and `struct mii_softc` are made of `Cell`s (all-zero valid: both live
//!   in zeroed softcs); they are changed under the kernel lock, where the C changes them (the
//!   attach, the ioctls, the drivers' timeouts). The callbacks are `Option<fn>`s with methods
//!   of the same names that call them (`mii.mii_readreg(dev, phy, reg)`), panicking with the
//!   hook's name where the C would call through NULL. `mii_ifp`, `mii_funcs` and `mii_pdata`
//!   are `Cell<Option<&'static ..>>`: the softcs that hold them are never freed while their
//!   devices exist.
//! - The callbacks' `struct device *` is `&Device`; `pf_service` returns `Result<(), Errno>`
//!   (`EJUSTRETURN` is an `Errno`).
//! - `PHY_READ`, `PHY_WRITE`, `PHY_SERVICE`, `PHY_STATUS` and `PHY_RESET` are the methods
//!   `phy_read`, `phy_write`, `phy_service`, `phy_status` and `phy_reset` of `MiiSoftc`;
//!   `MII_OUI`, `MII_MODEL` and `MII_REV` are `const fn`s with lower-case names, and
//!   `mii_phy_probe` a function.
//! - A PHY table (`struct mii_phydesc[]`) is a slice without the C's `{ 0, 0, NULL }`
//!   terminator; `mpd_name` is a `&str`.
//! - `struct mii_attach_args` holds `mii_data` as `&'static MiiData`.
//! - The prototypes of `mii.c` and `mii_physubr.c` (and `ukphy_status`) are those files' own
//!   functions (`mii.rs`, `mii_physubr.rs`, `ukphy_subr.rs`).

use core::cell::Cell;

use crate::dev::mii::mii::{IDR2_MODEL, IDR2_REV, mii_attach};
use crate::kern::subr_prf::panic;
use crate::net::if_media::Ifmedia;
use crate::net::if_var::Ifnet;
use crate::queue_adapter;
use crate::sys::device::{Device, Softc};
use crate::sys::errno::Errno;
use crate::sys::queue::{ListEntry, ListHead};
use crate::sys::timeout::Timeout;

// Requests that can be made to the downcall.

/// `MII_TICK`: once-per-second tick.
pub const MII_TICK: i32 = 1;
/// `MII_MEDIACHG`: user changed media; perform the switch.
pub const MII_MEDIACHG: i32 = 2;
/// `MII_POLLSTAT`: user requested media status; fill it in.
pub const MII_POLLSTAT: i32 = 3;
/// `MII_DOWN`: interface is down.
pub const MII_DOWN: i32 = 4;

// Default mii_anegticks values.

/// `MII_ANEGTICKS`.
pub const MII_ANEGTICKS: i32 = 5;
/// `MII_ANEGTICKS_GIGE`.
pub const MII_ANEGTICKS_GIGE: i32 = 10;

// mii_flags

/// `MIIF_INITDONE`: has been initialized (mii_data).
pub const MIIF_INITDONE: i32 = 0x0001;
/// `MIIF_NOISOLATE`: do not isolate the PHY.
pub const MIIF_NOISOLATE: i32 = 0x0002;
/// `MIIF_NOLOOP`: no loopback capability.
pub const MIIF_NOLOOP: i32 = 0x0004;
/// `MIIF_DOINGAUTO`: doing autonegotiation (mii_softc).
pub const MIIF_DOINGAUTO: i32 = 0x0008;
/// `MIIF_AUTOTSLEEP`: use tsleep(), not timeout().
pub const MIIF_AUTOTSLEEP: i32 = 0x0010;
/// `MIIF_HAVEFIBER`: from parent: has fiber interface.
pub const MIIF_HAVEFIBER: i32 = 0x0020;
/// `MIIF_HAVE_GTCR`: has 100base-T2/1000base-T CR.
pub const MIIF_HAVE_GTCR: i32 = 0x0040;
/// `MIIF_IS_1000X`: is a 1000BASE-X device.
pub const MIIF_IS_1000X: i32 = 0x0080;
/// `MIIF_DOPAUSE`: advertise PAUSE capability.
pub const MIIF_DOPAUSE: i32 = 0x0100;
/// `MIIF_IS_HPNA`: is a HomePNA device.
pub const MIIF_IS_HPNA: i32 = 0x0200;
/// `MIIF_FORCEANEG`: force autonegotiation.
pub const MIIF_FORCEANEG: i32 = 0x0400;
/// `MIIF_SETDELAY`: set internal delay.
pub const MIIF_SETDELAY: i32 = 0x0800;
/// `MIIF_RXID`: add Rx delay.
pub const MIIF_RXID: i32 = 0x1000;
/// `MIIF_TXID`: add Tx delay.
pub const MIIF_TXID: i32 = 0x2000;
/// `MIIF_SGMII`: MAC to PHY interface is SGMII.
pub const MIIF_SGMII: i32 = 0x4000;

/// `MIIF_INHERIT_MASK`.
pub const MIIF_INHERIT_MASK: i32 = MIIF_NOISOLATE | MIIF_NOLOOP | MIIF_AUTOTSLEEP;

// Special `locators' passed to mii_attach(). If one of these is not an `any' value, we look
// for *that* PHY and configure it. If both are not `any', that is an error, and mii_attach()
// will panic.

/// `MII_OFFSET_ANY`.
pub const MII_OFFSET_ANY: i32 = -1;
/// `MII_PHY_ANY`.
pub const MII_PHY_ANY: i32 = -1;

/// `MII_MEDIA_NONE`.
pub const MII_MEDIA_NONE: u32 = 0;
/// `MII_MEDIA_10_T`.
pub const MII_MEDIA_10_T: u32 = 1;
/// `MII_MEDIA_10_T_FDX`.
pub const MII_MEDIA_10_T_FDX: u32 = 2;
/// `MII_MEDIA_100_T4`.
pub const MII_MEDIA_100_T4: u32 = 3;
/// `MII_MEDIA_100_TX`.
pub const MII_MEDIA_100_TX: u32 = 4;
/// `MII_MEDIA_100_TX_FDX`.
pub const MII_MEDIA_100_TX_FDX: u32 = 5;
/// `MII_MEDIA_1000_X`.
pub const MII_MEDIA_1000_X: u32 = 6;
/// `MII_MEDIA_1000_X_FDX`.
pub const MII_MEDIA_1000_X_FDX: u32 = 7;
/// `MII_MEDIA_1000_T`.
pub const MII_MEDIA_1000_T: u32 = 8;
/// `MII_MEDIA_1000_T_FDX`.
pub const MII_MEDIA_1000_T_FDX: u32 = 9;
/// `MII_NMEDIA`.
pub const MII_NMEDIA: u32 = 10;

/// `mii_readreg_t`: reads register `reg` of PHY `phy` through the network interface's
/// device.
pub type MiiReadregT = fn(&Device, i32, i32) -> i32;
/// `mii_writereg_t`: writes `val` to register `reg` of PHY `phy`.
pub type MiiWriteregT = fn(&Device, i32, i32, i32);
/// `mii_statchg_t`: the media status changed.
pub type MiiStatchgT = fn(&Device);

/// `struct mii_data`: a network interface driver has one of these structures in its softc.
/// It is the interface from the network interface driver to the MII layer.
pub struct MiiData {
    /// `mii_media`: media information.
    pub mii_media: Ifmedia,
    /// `mii_ifp`: pointer back to network interface.
    pub mii_ifp: Cell<Option<&'static Ifnet>>,
    /// `mii_flags`: misc. flags; see below.
    pub mii_flags: Cell<i32>,
    /// `mii_node`: FDT node.
    pub mii_node: Cell<i32>,
    /// `mii_phys`: for network interfaces with multiple PHYs, a list of all PHYs is required
    /// so they can all be notified when a media request is made.
    pub mii_phys: ListHead<MiiList>,
    /// `mii_instance`.
    pub mii_instance: Cell<u64>,
    /// `mii_media_status`: PHY driver fills this in with active media status.
    pub mii_media_status: Cell<u64>,
    /// `mii_media_active`.
    pub mii_media_active: Cell<u64>,
    /// `mii_readreg`: calls from MII layer into network interface driver.
    pub mii_readreg: Cell<Option<MiiReadregT>>,
    /// `mii_writereg`.
    pub mii_writereg: Cell<Option<MiiWriteregT>>,
    /// `mii_statchg`.
    pub mii_statchg: Cell<Option<MiiStatchgT>>,
}

impl MiiData {
    /// An empty `struct mii_data`, what a zeroed softc holds.
    pub const fn new() -> Self {
        Self {
            mii_media: Ifmedia::new(),
            mii_ifp: Cell::new(None),
            mii_flags: Cell::new(0),
            mii_node: Cell::new(0),
            mii_phys: ListHead::new(),
            mii_instance: Cell::new(0),
            mii_media_status: Cell::new(0),
            mii_media_active: Cell::new(0),
            mii_readreg: Cell::new(None),
            mii_writereg: Cell::new(None),
            mii_statchg: Cell::new(None),
        }
    }

    /// `(*mii->mii_readreg)(dev, phy, reg)`.
    pub fn mii_readreg(&self, dev: &Device, phy: i32, reg: i32) -> i32 {
        match self.mii_readreg.get() {
            Some(f) => f(dev, phy, reg),
            None => panic(format_args!("mii: no mii_readreg")),
        }
    }

    /// `(*mii->mii_writereg)(dev, phy, reg, val)`.
    pub fn mii_writereg(&self, dev: &Device, phy: i32, reg: i32, val: i32) {
        match self.mii_writereg.get() {
            Some(f) => f(dev, phy, reg, val),
            None => panic(format_args!("mii: no mii_writereg")),
        }
    }

    /// `(*mii->mii_statchg)(dev)`.
    pub fn mii_statchg(&self, dev: &Device) {
        match self.mii_statchg.get() {
            Some(f) => f(dev),
            None => panic(format_args!("mii: no mii_statchg")),
        }
    }

    /// `mii->mii_ifp`, which the driver sets before it attaches the PHYs.
    pub fn ifp(&self) -> &'static Ifnet {
        match self.mii_ifp.get() {
            Some(ifp) => ifp,
            None => panic(format_args!("mii: no mii_ifp")),
        }
    }

    /// `mii->mii_media.ifm_cur->ifm_media`: the selected media word, which the driver's
    /// `ifmedia_set` chose before the PHYs are serviced (the C dereferences the pointer).
    pub fn cur_media(&self) -> u64 {
        match self.mii_media.ifm_cur() {
            Some(ife) => ife.ifm_media,
            None => panic(format_args!("mii: no current media")),
        }
    }

    /// `mii->mii_media.ifm_cur->ifm_data`.
    pub fn cur_data(&self) -> u32 {
        match self.mii_media.ifm_cur() {
            Some(ife) => ife.ifm_data,
            None => panic(format_args!("mii: no current media")),
        }
    }
}

impl Default for MiiData {
    fn default() -> Self {
        Self::new()
    }
}

// SAFETY: the members change under the kernel lock, as in C (the attach, the ioctls and the
// drivers' timeouts, which run with it); `mii_media` has its own rules (`net/if_media.rs`).
unsafe impl Sync for MiiData {}

/// `struct mii_phy_funcs`: a PHY driver's functions.
pub struct MiiPhyFuncs {
    /// `pf_service`.
    pub pf_service: fn(&'static MiiSoftc, &'static MiiData, i32) -> Result<(), Errno>,
    /// `pf_status`.
    pub pf_status: fn(&'static MiiSoftc),
    /// `pf_reset`.
    pub pf_reset: fn(&'static MiiSoftc),
}

/// `struct mii_softc`: each PHY driver's softc has one of these as the first member.
/// XXX This would be better named "phy_softc", but this is the name BSDI used, and we would
/// like to have the same interface.
#[repr(C)]
pub struct MiiSoftc {
    /// `mii_dev`: generic device glue.
    pub mii_dev: Device,
    /// `mii_list`: entry on parent's PHY list.
    pub mii_list: ListEntry<MiiSoftc>,
    /// `mii_phy`: our MII address.
    pub mii_phy: Cell<i32>,
    /// `mii_oui`: `MII_OUI(ma->mii_id1, ma->mii_id2)`.
    pub mii_oui: Cell<i32>,
    /// `mii_model`: `MII_MODEL(ma->mii_id2)`.
    pub mii_model: Cell<i32>,
    /// `mii_rev`: `MII_REV(ma->mii_id2)`.
    pub mii_rev: Cell<i32>,
    /// `mii_offset`: first PHY, second PHY, etc.
    pub mii_offset: Cell<i32>,
    /// `mii_inst`: instance for ifmedia.
    pub mii_inst: Cell<u64>,
    /// `mii_funcs`: our PHY functions.
    pub mii_funcs: Cell<Option<&'static MiiPhyFuncs>>,
    /// `mii_pdata`: pointer to parent's mii_data.
    pub mii_pdata: Cell<Option<&'static MiiData>>,
    /// `mii_flags`: misc. flags; see below.
    pub mii_flags: Cell<i32>,
    /// `mii_capabilities`: capabilities from BMSR.
    pub mii_capabilities: Cell<i32>,
    /// `mii_extcapabilities`: extended capabilities.
    pub mii_extcapabilities: Cell<i32>,
    /// `mii_ticks`: MII_TICK counter.
    pub mii_ticks: Cell<i32>,
    /// `mii_anegticks`: ticks before retrying aneg.
    pub mii_anegticks: Cell<i32>,
    /// `mii_phy_timo`: timeout handle.
    pub mii_phy_timo: Timeout,
    /// `mii_media_active`: last active media.
    pub mii_media_active: Cell<u64>,
    /// `mii_media_status`: last active status.
    pub mii_media_status: Cell<u64>,
}

impl MiiSoftc {
    /// `sc->mii_pdata`, which the attach sets.
    pub fn pdata(&self) -> &'static MiiData {
        match self.mii_pdata.get() {
            Some(mii) => mii,
            None => panic(format_args!("{}: no mii_pdata", self.mii_dev.xname())),
        }
    }

    /// `sc->mii_dev.dv_parent`: the network interface's device.
    pub fn parent(&self) -> &Device {
        match self.mii_dev.parent() {
            Some(p) => p,
            None => panic(format_args!(
                "{}: PHY without a parent",
                self.mii_dev.xname()
            )),
        }
    }

    /// `sc->mii_funcs`, which the attach sets.
    fn funcs(&self) -> &'static MiiPhyFuncs {
        match self.mii_funcs.get() {
            Some(f) => f,
            None => panic(format_args!("{}: no mii_funcs", self.mii_dev.xname())),
        }
    }

    /// `PHY_READ(p, r)`.
    pub fn phy_read(&self, r: i32) -> i32 {
        self.pdata()
            .mii_readreg(self.parent(), self.mii_phy.get(), r)
    }

    /// `PHY_WRITE(p, r, v)`.
    pub fn phy_write(&self, r: i32, v: i32) {
        self.pdata()
            .mii_writereg(self.parent(), self.mii_phy.get(), r, v);
    }

    /// `PHY_SERVICE(p, d, o)`.
    pub fn phy_service(&'static self, d: &'static MiiData, o: i32) -> Result<(), Errno> {
        (self.funcs().pf_service)(self, d, o)
    }

    /// `PHY_STATUS(p)`.
    pub fn phy_status(&'static self) {
        (self.funcs().pf_status)(self);
    }

    /// `PHY_RESET(p)`.
    pub fn phy_reset(&'static self) {
        (self.funcs().pf_reset)(self);
    }
}

// SAFETY: `#[repr(C)]` with the device first; the list entry and the timeout are all-zero
// valid, and every other member is a `Cell` of an integer or an `Option` of a reference.
unsafe impl Softc for MiiSoftc {}

// SAFETY: the members change under the kernel lock, as in C (the attach, `mii_attach` linking
// the PHY in, the PHY services the drivers call from their ioctls and timeouts).
unsafe impl Sync for MiiSoftc {}

queue_adapter!(
    /// `LIST_HEAD(mii_listhead, mii_softc)`: the PHYs of a `struct mii_data`.
    pub MiiList: MiiSoftc, mii_list => ListEntry<MiiSoftc>
);

/// `struct mii_attach_args`: used to attach a PHY to a parent.
pub struct MiiAttachArgs {
    /// `mii_data`: pointer to parent data.
    pub mii_data: &'static MiiData,
    /// `mii_phyno`: MII address.
    pub mii_phyno: i32,
    /// `mii_id1`: PHY ID register 1.
    pub mii_id1: i32,
    /// `mii_id2`: PHY ID register 2.
    pub mii_id2: i32,
    /// `mii_capmask`: capability mask from BMSR.
    pub mii_capmask: i32,
    /// `mii_flags`: flags from parent.
    pub mii_flags: i32,
}

/// `struct mii_phydesc`: used to match a PHY.
pub struct MiiPhydesc {
    /// `mpd_oui`: the PHY's OUI.
    pub mpd_oui: u32,
    /// `mpd_model`: the PHY's model.
    pub mpd_model: u32,
    /// `mpd_name`: the PHY's name.
    pub mpd_name: &'static str,
}

/// `struct mii_media`: an array of these structures map MII media types to BMCR/ANAR
/// settings.
pub struct MiiMedia {
    /// `mm_bmcr`: BMCR settings for this media.
    pub mm_bmcr: i32,
    /// `mm_anar`: ANAR settings for this media.
    pub mm_anar: i32,
    /// `mm_gtcr`: 100base-T2 or 1000base-T CR.
    pub mm_gtcr: i32,
}

/// `mii_phy_probe(x, y, z)`: `mii_attach` at any PHY address and offset, without flags.
pub fn mii_phy_probe(x: &Device, y: &'static MiiData, z: i32) {
    mii_attach(x, y, z, MII_PHY_ANY, MII_OFFSET_ANY, 0);
}

/// `MII_OUI(id1, id2)`.
pub const fn mii_oui(id1: i32, id2: i32) -> i32 {
    (id1 << 6) | (id2 >> 10)
}

/// `MII_MODEL(id2)`.
pub const fn mii_model(id2: i32) -> i32 {
    (id2 & IDR2_MODEL) >> 4
}

/// `MII_REV(id2)`.
pub const fn mii_rev(id2: i32) -> i32 {
    id2 & IDR2_REV
}
/* </CODE> */

/* <TESTS> */
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    #[ignore = "needs OPENBSD_SRC (just test-ref)"]
    fn values_match_the_c_header() {
        let defs = crate::reftest::defines("sys/dev/mii/miivar.h");
        crate::reftest::assert_defines!(defs; MII_TICK, MII_MEDIACHG, MII_POLLSTAT, MII_DOWN,
            MII_ANEGTICKS, MII_ANEGTICKS_GIGE, MIIF_INITDONE, MIIF_NOISOLATE, MIIF_NOLOOP,
            MIIF_DOINGAUTO, MIIF_AUTOTSLEEP, MIIF_HAVEFIBER, MIIF_HAVE_GTCR, MIIF_IS_1000X,
            MIIF_DOPAUSE, MIIF_IS_HPNA, MIIF_FORCEANEG, MIIF_SETDELAY, MIIF_RXID, MIIF_TXID,
            MIIF_SGMII, MIIF_INHERIT_MASK, MII_OFFSET_ANY, MII_PHY_ANY, MII_MEDIA_NONE,
            MII_MEDIA_10_T, MII_MEDIA_10_T_FDX, MII_MEDIA_100_T4, MII_MEDIA_100_TX,
            MII_MEDIA_100_TX_FDX, MII_MEDIA_1000_X, MII_MEDIA_1000_X_FDX, MII_MEDIA_1000_T,
            MII_MEDIA_1000_T_FDX, MII_NMEDIA);
    }

    #[test]
    fn id_registers_split() {
        // QEMU's 8139C+ reads 0 from both ID registers; a Realtek RTL8201L reads 0x0000/0x8201.
        assert_eq!((mii_oui(0, 0), mii_model(0), mii_rev(0)), (0, 0, 0));
        assert_eq!(mii_oui(0x0000, 0x8201), 0x20);
        assert_eq!(mii_model(0x8201), 0x20);
        assert_eq!(mii_rev(0x8201), 1);
    }
}
/* </TESTS> */
