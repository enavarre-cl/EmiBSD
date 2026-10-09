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
 * Copyright (c) 1995, David Greenman
 * All rights reserved.
 *
 * Modifications to support NetBSD:
 * Copyright (c) 1997 Jason R. Thorpe.  All rights reserved.
 *
 * Redistribution and use in source and binary forms, with or without
 * modification, are permitted provided that the following conditions
 * are met:
 * 1. Redistributions of source code must retain the above copyright
 *    notice unmodified, this list of conditions, and the following
 *    disclaimer.
 * 2. Redistributions in binary form must reproduce the above copyright
 *    notice, this list of conditions and the following disclaimer in the
 *    documentation and/or other materials provided with the distribution.
 *
 * THIS SOFTWARE IS PROVIDED BY THE AUTHOR AND CONTRIBUTORS ``AS IS'' AND
 * ANY EXPRESS OR IMPLIED WARRANTIES, INCLUDING, BUT NOT LIMITED TO, THE
 * IMPLIED WARRANTIES OF MERCHANTABILITY AND FITNESS FOR A PARTICULAR PURPOSE
 * ARE DISCLAIMED.  IN NO EVENT SHALL THE AUTHOR OR CONTRIBUTORS BE LIABLE
 * FOR ANY DIRECT, INDIRECT, INCIDENTAL, SPECIAL, EXEMPLARY, OR CONSEQUENTIAL
 * DAMAGES (INCLUDING, BUT NOT LIMITED TO, PROCUREMENT OF SUBSTITUTE GOODS
 * OR SERVICES; LOSS OF USE, DATA, OR PROFITS; OR BUSINESS INTERRUPTION)
 * HOWEVER CAUSED AND ON ANY THEORY OF LIABILITY, WHETHER IN CONTRACT, STRICT
 * LIABILITY, OR TORT (INCLUDING NEGLIGENCE OR OTHERWISE) ARISING IN ANY WAY
 * OUT OF THE USE OF THIS SOFTWARE, EVEN IF ADVISED OF THE POSSIBILITY OF
 * SUCH DAMAGE.
 *
 *	Id: if_fxp.c,v 1.55 1998/08/04 08:53:12 dg Exp
 */
/* </LICENSES> */

/* <CODE> */
//! `dev/pci/if_fxp_pci.c`: the PCI front-end of fxp(4) (`fxp* at pci?`) for the Intel
//! EtherExpress PRO/100 family.
//!
//! Upstream: sys/dev/pci/if_fxp_pci.c @ 3ce1f3f79392
//!
//! It matches `fxp_pci_devices[]`, maps the I/O BAR (`FXP_PCI_IOBA`), takes the chip
//! revision from the PCI class register, names the chip (`i82557` ... `i82551`, `i82552`,
//! `i82562`), flags the cards whose EEPROM needs the dynamic standby workaround
//! (`FXPF_DISABLE_STANDBY`) and turns on Memory Write and Invalidate on the 82558 and later.
//! QEMU's `i82559er` is PCI id 8086:1209 revision 0x09: `i82559S`.
//!
//! ## Deviations
//! - The softc is `#[repr(C)]` with the `struct fxp_softc` first (its device first), as in C;
//!   `psc_pc` is an `Option` (all-zero valid).
//! - `fxp_pci_detach` and `fxp_pci_activate` return `Result<(), Errno>`.
//! - The interrupt is established under the device's name, which lives as long as the softc.
//! - The chip name and the standby test are `fxp_chipname` and `fxp_disable_standby` so that
//!   they can be tested.
//! - `fxp_pci_attach` prints the name and the interrupt of a failed attach as the C does and
//!   leaves the device unattached.

use core::cell::Cell;
use core::ffi::c_void;
use core::ptr::{self, NonNull};

use crate::dev::ic::fxp::{fxp_activate, fxp_attach, fxp_detach, fxp_intr};
use crate::dev::ic::fxpreg::{
    FXP_PCI_IOBA, FXP_REV_82550, FXP_REV_82551_E, FXP_REV_82558_A4, FXP_REV_82559_A0,
    FXP_REV_82559S_A,
};
use crate::dev::ic::fxpvar::{FXPF_DISABLE_STANDBY, FXPF_MWI_ENABLE, FxpSoftc};
use crate::dev::pci::pci::pci_matchbyid;
use crate::dev::pci::pci_map::pci_mapreg_map;
use crate::dev::pci::pcidevs::*;
use crate::dev::pci::pcireg::{
    PCI_BHLC_REG, PCI_COMMAND_INVALIDATE_ENABLE, PCI_COMMAND_STATUS_REG, PCI_MAPREG_TYPE_IO,
    PciProductId, PciVendorId, pci_cacheline, pci_product, pci_revision, pci_vendor,
};
use crate::dev::pci::pcivar::{PciAttachArgs, PciMatchid};
use crate::kern::subr_prf::{Str, printf};
use crate::machine::bus::{BusSize, bus_space_unmap};
use crate::machine::intr::IPL_NET;
use crate::machine::pci_machdep::{
    PciChipsetTag, pci_conf_read, pci_conf_write, pci_intr_disestablish, pci_intr_establish,
    pci_intr_map, pci_intr_string,
};
use crate::sys::device::{CfMatch, Cfattach, Device, Softc};
use crate::sys::errno::Errno;

/// `struct fxp_pci_softc`.
#[repr(C)]
pub struct FxpPciSoftc {
    /// `psc_softc`.
    pub psc_softc: FxpSoftc,
    /// `psc_pc`.
    pub psc_pc: Cell<Option<PciChipsetTag>>,
    /// `psc_mapsize`.
    pub psc_mapsize: Cell<BusSize>,
}

// SAFETY: `#[repr(C)]` with the `struct fxp_softc` (a `Softc`, its device first) first; the
// other members are `Cell`s of an `Option` of a tag and of a size, all-zero valid.
unsafe impl Softc for FxpPciSoftc {}

/// `{ vendor, product }` of `fxp_pci_devices[]`.
const fn id(vendor: u32, product: u32) -> PciMatchid {
    PciMatchid {
        pm_vid: vendor as PciVendorId,
        pm_pid: product as PciProductId,
    }
}

/// `fxp_pci_devices[]`.
pub static FXP_PCI_DEVICES: [PciMatchid; 47] = [
    id(PCI_VENDOR_INTEL, PCI_PRODUCT_INTEL_8255X),
    id(PCI_VENDOR_INTEL, PCI_PRODUCT_INTEL_82552),
    id(PCI_VENDOR_INTEL, PCI_PRODUCT_INTEL_82559),
    id(PCI_VENDOR_INTEL, PCI_PRODUCT_INTEL_82559ER),
    id(PCI_VENDOR_INTEL, PCI_PRODUCT_INTEL_82562),
    id(PCI_VENDOR_INTEL, PCI_PRODUCT_INTEL_82562EH_HPNA_0),
    id(PCI_VENDOR_INTEL, PCI_PRODUCT_INTEL_82562EH_HPNA_1),
    id(PCI_VENDOR_INTEL, PCI_PRODUCT_INTEL_82562EH_HPNA_2),
    id(PCI_VENDOR_INTEL, PCI_PRODUCT_INTEL_PRO_100_VE_0),
    id(PCI_VENDOR_INTEL, PCI_PRODUCT_INTEL_PRO_100_VE_1),
    id(PCI_VENDOR_INTEL, PCI_PRODUCT_INTEL_PRO_100_VE_2),
    id(PCI_VENDOR_INTEL, PCI_PRODUCT_INTEL_PRO_100_VE_3),
    id(PCI_VENDOR_INTEL, PCI_PRODUCT_INTEL_PRO_100_VE_4),
    id(PCI_VENDOR_INTEL, PCI_PRODUCT_INTEL_PRO_100_VE_5),
    id(PCI_VENDOR_INTEL, PCI_PRODUCT_INTEL_PRO_100_VE_6),
    id(PCI_VENDOR_INTEL, PCI_PRODUCT_INTEL_PRO_100_VE_7),
    id(PCI_VENDOR_INTEL, PCI_PRODUCT_INTEL_PRO_100_VE_8),
    id(PCI_VENDOR_INTEL, PCI_PRODUCT_INTEL_PRO_100_VM_0),
    id(PCI_VENDOR_INTEL, PCI_PRODUCT_INTEL_PRO_100_VM_1),
    id(PCI_VENDOR_INTEL, PCI_PRODUCT_INTEL_PRO_100_VM_2),
    id(PCI_VENDOR_INTEL, PCI_PRODUCT_INTEL_PRO_100_VM_3),
    id(PCI_VENDOR_INTEL, PCI_PRODUCT_INTEL_PRO_100_VM_4),
    id(PCI_VENDOR_INTEL, PCI_PRODUCT_INTEL_PRO_100_VM_5),
    id(PCI_VENDOR_INTEL, PCI_PRODUCT_INTEL_PRO_100_VM_6),
    id(PCI_VENDOR_INTEL, PCI_PRODUCT_INTEL_PRO_100_VM_7),
    id(PCI_VENDOR_INTEL, PCI_PRODUCT_INTEL_PRO_100_VM_8),
    id(PCI_VENDOR_INTEL, PCI_PRODUCT_INTEL_PRO_100_VM_9),
    id(PCI_VENDOR_INTEL, PCI_PRODUCT_INTEL_PRO_100_VM_10),
    id(PCI_VENDOR_INTEL, PCI_PRODUCT_INTEL_PRO_100_VM_11),
    id(PCI_VENDOR_INTEL, PCI_PRODUCT_INTEL_PRO_100_VM_12),
    id(PCI_VENDOR_INTEL, PCI_PRODUCT_INTEL_PRO_100_VM_13),
    id(PCI_VENDOR_INTEL, PCI_PRODUCT_INTEL_PRO_100_VM_14),
    id(PCI_VENDOR_INTEL, PCI_PRODUCT_INTEL_PRO_100_VM_15),
    id(PCI_VENDOR_INTEL, PCI_PRODUCT_INTEL_PRO_100_VM_16),
    id(PCI_VENDOR_INTEL, PCI_PRODUCT_INTEL_PRO_100_VM_17),
    id(PCI_VENDOR_INTEL, PCI_PRODUCT_INTEL_PRO_100_VM_18),
    id(PCI_VENDOR_INTEL, PCI_PRODUCT_INTEL_PRO_100_VM_19),
    id(PCI_VENDOR_INTEL, PCI_PRODUCT_INTEL_PRO_100_M),
    id(PCI_VENDOR_INTEL, PCI_PRODUCT_INTEL_PRO_100),
    id(PCI_VENDOR_INTEL, PCI_PRODUCT_INTEL_82801DB_LAN),
    id(PCI_VENDOR_INTEL, PCI_PRODUCT_INTEL_82801E_LAN_1),
    id(PCI_VENDOR_INTEL, PCI_PRODUCT_INTEL_82801E_LAN_2),
    id(PCI_VENDOR_INTEL, PCI_PRODUCT_INTEL_82801FB_LAN),
    id(PCI_VENDOR_INTEL, PCI_PRODUCT_INTEL_82801FB_LAN_2),
    id(PCI_VENDOR_INTEL, PCI_PRODUCT_INTEL_82801FBM_LAN),
    id(PCI_VENDOR_INTEL, PCI_PRODUCT_INTEL_82801GB_LAN),
    id(PCI_VENDOR_INTEL, PCI_PRODUCT_INTEL_82801GB_LAN_2),
];

/// `fxp_pci_ca`.
pub static FXP_PCI_CA: Cfattach = Cfattach {
    ca_devsize: size_of::<FxpPciSoftc>(),
    ca_match: Some(fxp_pci_match),
    ca_attach: fxp_pci_attach,
    ca_detach: Some(fxp_pci_detach),
    ca_activate: Some(fxp_activate),
};

/// The softc of a device made for `fxp_pci_ca`.
fn fxp_pci_sc(self_: &Device) -> &'static FxpPciSoftc {
    // SAFETY: `self_` was made for `fxp_pci_ca`, whose softc is a `FxpPciSoftc`; softcs are
    // never freed while the device exists, so the softc may be borrowed for 'static.
    unsafe { &*ptr::from_ref(self_.softc::<FxpPciSoftc>()) }
}

/// `fxp_pci_match`.
pub fn fxp_pci_match(_parent: Option<&Device>, _match: &CfMatch, aux: *mut c_void) -> i32 {
    // SAFETY: pci attaches its children with a `pci_attach_args`.
    let pa = unsafe { &*aux.cast::<PciAttachArgs>() };
    pci_matchbyid(pa, &FXP_PCI_DEVICES)
}

/// The chip name `fxp_pci_attach` prints after the interrupt: the 8255x ids by revision, the
/// 82552, and `i82562` for every other id.
pub fn fxp_chipname(product: u32, revision: u32) -> &'static str {
    match product {
        PCI_PRODUCT_INTEL_8255X | PCI_PRODUCT_INTEL_82559 | PCI_PRODUCT_INTEL_82559ER => {
            let mut chipname = "i82557";
            if revision >= FXP_REV_82558_A4 {
                chipname = "i82558";
            }
            if revision >= FXP_REV_82559_A0 {
                chipname = "i82559";
            }
            if revision >= FXP_REV_82559S_A {
                chipname = "i82559S";
            }
            if revision >= FXP_REV_82550 {
                chipname = "i82550";
            }
            if revision >= FXP_REV_82551_E {
                chipname = "i82551";
            }
            chipname
        }
        PCI_PRODUCT_INTEL_82552 => "i82552",
        _ => "i82562",
    }
}

/// Cards for which we should WRITE TO THE EEPROM to turn off dynamic standby mode to avoid a
/// problem where the card will fail to resume when entering the IDLE state. The C uses this
/// nasty `if` and corresponding pci dev numbers directly so that people know not to add new
/// cards to this unless you are really certain what you are doing and are not going to end up
/// killing people's eeproms.
pub fn fxp_disable_standby(vendor: u32, product: u32, revision: u32) -> bool {
    vendor == PCI_VENDOR_INTEL
        && (product == 0x2449
            || (product > 0x1030 && product < 0x1039)
            || (product == 0x1229 && (8..=16).contains(&revision)))
}

/// `fxp_pci_attach`.
pub fn fxp_pci_attach(_parent: Option<&Device>, self_: &Device, aux: *mut c_void) {
    let psc = fxp_pci_sc(self_);
    let sc: &'static FxpSoftc = &psc.psc_softc;
    // SAFETY: pci attaches its children with a `pci_attach_args` it owns for the duration of
    // the attach.
    let pa = unsafe { &*aux.cast::<PciAttachArgs>() };
    let pc = pa.pa_pc;

    let Ok((t, h, _base, mapsize)) = pci_mapreg_map(pa, FXP_PCI_IOBA, PCI_MAPREG_TYPE_IO, 0, 0)
    else {
        printf(format_args!(": can't map i/o space\n"));
        return;
    };
    sc.sc_st.set(Some(t));
    sc.sc_sh.set(Some(h));
    psc.psc_mapsize.set(mapsize);
    psc.psc_pc.set(Some(pc));
    sc.sc_dmat.set(Some(pa.pa_dmat));

    sc.sc_revision.set(pci_revision(pa.pa_class));

    // Allocate our interrupt.
    let Some(ih) = pci_intr_map(pa) else {
        printf(format_args!(": couldn't map interrupt\n"));
        bus_space_unmap(t, h, mapsize);
        return;
    };

    let intrstr = pci_intr_string(pc, ih);
    let Some(cookie) = pci_intr_establish(
        pc,
        ih,
        IPL_NET,
        fxp_intr,
        ptr::from_ref(sc).cast_mut().cast(),
        sc.devname(),
    ) else {
        printf(format_args!(
            ": couldn't establish interrupt at {}\n",
            Str(intrstr.as_bytes())
        ));
        bus_space_unmap(t, h, mapsize);
        return;
    };
    sc.sc_ih.set(cookie.as_ptr());

    let chipname = fxp_chipname(pci_product(pa.pa_id), sc.sc_revision.get());
    printf(format_args!(", {chipname}"));

    if fxp_disable_standby(
        pci_vendor(pa.pa_id),
        pci_product(pa.pa_id),
        sc.sc_revision.get(),
    ) {
        sc.sc_flags.set(sc.sc_flags.get() | FXPF_DISABLE_STANDBY);
    }

    // enable PCI Memory Write and Invalidate command
    if sc.sc_revision.get() >= FXP_REV_82558_A4
        && pci_cacheline(pci_conf_read(pc, pa.pa_tag, PCI_BHLC_REG)) != 0
    {
        pci_conf_write(
            pc,
            pa.pa_tag,
            PCI_COMMAND_STATUS_REG,
            PCI_COMMAND_INVALIDATE_ENABLE | pci_conf_read(pc, pa.pa_tag, PCI_COMMAND_STATUS_REG),
        );
        sc.sc_flags.set(sc.sc_flags.get() | FXPF_MWI_ENABLE);
    }

    // Do generic parts of attach.
    if fxp_attach(sc, intrstr.as_bytes()).is_err() {
        // Failed!
        sc.sc_ih.set(ptr::null_mut());
        // SAFETY: the handler established above; its pointer is gone from the softc.
        unsafe { pci_intr_disestablish(pc, cookie) };
        bus_space_unmap(t, h, mapsize);
    }
}

/// `fxp_pci_detach`.
pub fn fxp_pci_detach(self_: &Device, _flags: i32) -> Result<(), Errno> {
    let psc = fxp_pci_sc(self_);
    let sc: &'static FxpSoftc = &psc.psc_softc;

    if let (Some(cookie), Some(pc)) = (NonNull::new(sc.sc_ih.get()), psc.psc_pc.get()) {
        sc.sc_ih.set(ptr::null_mut());
        // SAFETY: the handler fxp_pci_attach established; its pointer is gone from the
        // softc, and fxp_detach stops the interface just below.
        unsafe { pci_intr_disestablish(pc, cookie) };
    }
    fxp_detach(sc);
    let (t, h) = sc.regs();
    bus_space_unmap(t, h, psc.psc_mapsize.get());

    Ok(())
}
/* </CODE> */

/* <TESTS> */
#[cfg(test)]
mod tests {
    use super::*;
    use crate::dev::ic::fxpreg::FXP_REV_82551_10;

    #[test]
    fn the_device_table_has_the_47_c_entries() {
        assert_eq!(FXP_PCI_DEVICES.len(), 47);
        assert!(
            FXP_PCI_DEVICES
                .iter()
                .any(|m| u32::from(m.pm_pid) == PCI_PRODUCT_INTEL_82559ER)
        );
    }

    #[test]
    fn chip_names_follow_the_revision() {
        // QEMU's i82559er is 8086:1209 revision 9.
        assert_eq!(fxp_chipname(PCI_PRODUCT_INTEL_82559ER, 9), "i82559S");
        assert_eq!(fxp_chipname(PCI_PRODUCT_INTEL_8255X, 1), "i82557");
        assert_eq!(fxp_chipname(PCI_PRODUCT_INTEL_8255X, 4), "i82558");
        assert_eq!(fxp_chipname(PCI_PRODUCT_INTEL_8255X, 8), "i82559");
        assert_eq!(fxp_chipname(PCI_PRODUCT_INTEL_8255X, 12), "i82550");
        assert_eq!(
            fxp_chipname(PCI_PRODUCT_INTEL_8255X, FXP_REV_82551_10),
            "i82551"
        );
        assert_eq!(fxp_chipname(PCI_PRODUCT_INTEL_82552, 0), "i82552");
        assert_eq!(fxp_chipname(PCI_PRODUCT_INTEL_82562, 0), "i82562");
    }

    #[test]
    fn the_standby_workaround_cards() {
        assert!(fxp_disable_standby(PCI_VENDOR_INTEL, 0x2449, 0));
        assert!(fxp_disable_standby(PCI_VENDOR_INTEL, 0x1031, 0));
        assert!(!fxp_disable_standby(PCI_VENDOR_INTEL, 0x1030, 0));
        assert!(!fxp_disable_standby(PCI_VENDOR_INTEL, 0x1039, 0));
        assert!(fxp_disable_standby(PCI_VENDOR_INTEL, 0x1229, 8));
        assert!(fxp_disable_standby(PCI_VENDOR_INTEL, 0x1229, 16));
        assert!(!fxp_disable_standby(PCI_VENDOR_INTEL, 0x1229, 7));
        assert!(!fxp_disable_standby(PCI_VENDOR_INTEL, 0x1229, 17));
        // QEMU's i82559er (0x1209) is not in the list.
        assert!(!fxp_disable_standby(PCI_VENDOR_INTEL, 0x1209, 9));
    }
}
/* </TESTS> */
