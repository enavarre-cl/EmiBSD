/*	$OpenBSD: nvme_pci.c,v 1.15 2026/06/30 16:24:33 jcs Exp $ */
/* <LICENSES> */
/*
 * Copyright (c) 2014 David Gwynne <dlg@openbsd.org>
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
/* </LICENSES> */

/* <CODE> */
//! `nvme* at pci?`: the PCI front-end of nvme(4): maps the register BAR, establishes the
//! interrupt (MSI-X, else MSI, else INTx) and attaches the controller.
//!
//! Upstream: sys/dev/pci/nvme_pci.c @ 3ce1f3f79392
//!
//! ## Deviations
//! - The softc begins with the `struct nvme_softc`, as in C; `psc_pc` is an `Option` (zero
//!   is `None`), set by attach.
//! - On the machines here MSI and MSI-X are refused by `pci_intr_map_msi*` until ACPI's
//!   `mp_busses` exist (amd64, M13 later; arm64 has no PCI bus before M12), so the
//!   controller runs on the INTx line the firmware routed, with `nvme_intr_intx`, as the C
//!   does on such a machine. The MSI-X and MSI paths are the C's and take over once the
//!   machine grants them.
//! - The interrupt is established with the device's name (`DEVNAME(sc)`), which lives as
//!   long as the softc.

use core::cell::Cell;
use core::ffi::c_void;
use core::ptr::{self, NonNull};

use crate::dev::ic::nvme::{nvme_activate, nvme_attach, nvme_intr, nvme_intr_intx};
use crate::dev::ic::nvmevar::NvmeSoftc;
use crate::dev::pci::pci_map::{pci_mapreg_map, pci_mapreg_type};
use crate::dev::pci::pcidevs::{
    PCI_PRODUCT_APPLE_NVME1, PCI_PRODUCT_APPLE_NVME2, PCI_PRODUCT_APPLE_NVME3, PCI_VENDOR_APPLE,
};
use crate::dev::pci::pcireg::{
    PCI_CLASS_MASS_STORAGE, PCI_SUBCLASS_MASS_STORAGE_NVM, pci_class, pci_interface, pci_product,
    pci_subclass, pci_vendor,
};
use crate::dev::pci::pcivar::PciAttachArgs;
use crate::kern::subr_autoconf::config_detach_children;
use crate::kern::subr_prf::printf;
use crate::machine::bus::bus_space_unmap;
use crate::machine::intr::IPL_BIO;
use crate::machine::pci_machdep::{
    PciChipsetTag, PciIntrFn, pci_intr_disestablish, pci_intr_establish, pci_intr_map,
    pci_intr_map_msi, pci_intr_map_msix, pci_intr_string,
};
use crate::sys::device::{CfMatch, Cfattach, Device, Softc};
use crate::sys::errno::Errno;

/// `NVME_PCI_BAR`: the register BAR.
pub const NVME_PCI_BAR: i32 = 0x10;
/// `NVME_PCI_INTERFACE`: the NVM Express programming interface.
pub const NVME_PCI_INTERFACE: u32 = 0x02;

/// `struct nvme_pci_softc`.
#[repr(C)]
pub struct NvmePciSoftc {
    /// `psc_nvme`.
    pub psc_nvme: NvmeSoftc,
    /// `psc_pc`.
    pub psc_pc: Cell<Option<PciChipsetTag>>,
}

// SAFETY: `#[repr(C)]` with the nvme softc (itself headed by the device, all-zero valid)
// first, then a `Cell` of an `Option` of the chipset tag, valid as zero bits.
unsafe impl Softc for NvmePciSoftc {}

/// `nvme_pci_ca`.
pub static NVME_PCI_CA: Cfattach = Cfattach {
    ca_devsize: size_of::<NvmePciSoftc>(),
    ca_match: Some(nvme_pci_match),
    ca_attach: nvme_pci_attach,
    ca_detach: Some(nvme_pci_detach),
    ca_activate: Some(nvme_pci_activate),
};

/// Whether a function with class register `class` and ID register `id` is an NVMe
/// controller: the NVM Express class, or one of Apple's controllers.
pub fn nvme_pci_is_nvme(class: u32, id: u32) -> bool {
    if pci_class(class) == PCI_CLASS_MASS_STORAGE
        && pci_subclass(class) == PCI_SUBCLASS_MASS_STORAGE_NVM
        && pci_interface(class) == NVME_PCI_INTERFACE
    {
        return true;
    }

    pci_vendor(id) == PCI_VENDOR_APPLE
        && matches!(
            pci_product(id),
            PCI_PRODUCT_APPLE_NVME1 | PCI_PRODUCT_APPLE_NVME2 | PCI_PRODUCT_APPLE_NVME3
        )
}

/// `nvme_pci_match`.
pub fn nvme_pci_match(_parent: Option<&Device>, _match: &CfMatch, aux: *mut c_void) -> i32 {
    // SAFETY: pci attaches its children with a `pci_attach_args`.
    let pa = unsafe { &*aux.cast::<PciAttachArgs>() };

    i32::from(nvme_pci_is_nvme(pa.pa_class, pa.pa_id))
}

/// `nvme_pci_attach`.
pub fn nvme_pci_attach(_parent: Option<&Device>, self_: &Device, aux: *mut c_void) {
    // SAFETY: `self_` was made for `nvme_pci_ca`, whose softc is an `NvmePciSoftc`; softcs
    // are never freed while the device exists, so the softc may be borrowed for 'static.
    let psc: &'static NvmePciSoftc = unsafe { &*ptr::from_ref(self_.softc::<NvmePciSoftc>()) };
    let sc = &psc.psc_nvme;
    // SAFETY: pci attaches its children with a `pci_attach_args` it owns for the duration of
    // the attach.
    let pa = unsafe { &*aux.cast::<PciAttachArgs>() };

    psc.psc_pc.set(Some(pa.pa_pc));
    sc.sc_dmat.set(Some(pa.pa_dmat));

    printf(format_args!(": "));

    let maptype = pci_mapreg_type(pa.pa_pc, pa.pa_tag, NVME_PCI_BAR);
    let Ok((iot, ioh, _base, ios)) = pci_mapreg_map(pa, NVME_PCI_BAR, maptype, 0, 0) else {
        printf(format_args!("unable to map registers\n"));
        return;
    };
    sc.sc_iot.set(Some(iot));
    sc.sc_ioh.set(Some(ioh));
    sc.sc_ios.set(ios);

    let unmap = 'unmap: {
        let (ih, msi) = match pci_intr_map_msix(pa, 0).or_else(|| pci_intr_map_msi(pa)) {
            Some(ih) => (ih, true),
            None => match pci_intr_map(pa) {
                Some(ih) => (ih, false),
                None => {
                    printf(format_args!("unable to map interrupt\n"));
                    break 'unmap true;
                }
            },
        };

        let func: PciIntrFn = if msi { nvme_intr } else { nvme_intr_intx };
        let Some(cookie) = pci_intr_establish(
            pa.pa_pc,
            ih,
            IPL_BIO,
            func,
            ptr::from_ref(sc).cast_mut().cast(),
            sc.sc_dev.xname(),
        ) else {
            printf(format_args!("unable to establish interrupt\n"));
            break 'unmap true;
        };
        sc.sc_ih.set(cookie.as_ptr());

        printf(format_args!("{}, ", pci_intr_string(pa.pa_pc, ih)));

        if pci_vendor(pa.pa_id) == PCI_VENDOR_APPLE
            && pci_product(pa.pa_id) == PCI_PRODUCT_APPLE_NVME3
        {
            // Apple T2 requires 128-byte submission queue entries
            sc.sc_sqe_size.set(128);
        }

        if nvme_attach(sc).is_err() {
            // error printed by nvme_attach()
            // disestablish:
            if let Some(ih) = NonNull::new(sc.sc_ih.replace(ptr::null_mut())) {
                // SAFETY: the handle established above, dropped here.
                unsafe { pci_intr_disestablish(pa.pa_pc, ih) };
            }
            break 'unmap true;
        }

        false
    };

    if unmap {
        bus_space_unmap(iot, ioh, sc.sc_ios.get());
        sc.sc_ios.set(0);
    }
}

/// `nvme_pci_detach`.
pub fn nvme_pci_detach(self_: &Device, flags: i32) -> Result<(), Errno> {
    config_detach_children(self_, flags)
}

/// `nvme_pci_activate`.
pub fn nvme_pci_activate(self_: &Device, act: i32) -> Result<(), Errno> {
    // SAFETY: `self_` was made for `nvme_pci_ca`; softcs are never freed while the device
    // exists.
    let psc: &'static NvmePciSoftc = unsafe { &*ptr::from_ref(self_.softc::<NvmePciSoftc>()) };

    nvme_activate(&psc.psc_nvme, act)
}
/* </CODE> */

/* <TESTS> */
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn matches_the_nvme_class_and_apple_controllers() {
        // QEMU's nvme: 1b36:0010, class 01 08 02.
        assert!(nvme_pci_is_nvme(0x0108_0200, 0x0010_1b36));
        // AHCI (01 06 01) is not NVMe, nor an NVM device with another interface.
        assert!(!nvme_pci_is_nvme(0x0106_0100, 0x2922_8086));
        assert!(!nvme_pci_is_nvme(0x0108_0100, 0x0010_1b36));
        // Apple's controllers match by ID whatever their class.
        assert!(nvme_pci_is_nvme(0x0180_0000, 0x2005_106b));
        assert!(!nvme_pci_is_nvme(0x0180_0000, 0x2004_106b));
    }
}
/* </TESTS> */
