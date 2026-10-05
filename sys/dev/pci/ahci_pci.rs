/*	$OpenBSD: ahci_pci.c,v 1.18 2024/06/16 18:00:08 kn Exp $ */
/* <LICENSES> */
/*
 * Copyright (c) 2006 David Gwynne <dlg@openbsd.org>
 * Copyright (c) 2010 Conformal Systems LLC <info@conformal.com>
 * Copyright (c) 2010 Jonathan Matthew <jonathan@d14n.org>
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

//! `ahci* at pci?`: the PCI front-end of ahci(4): the controller quirks by vendor and
//! product, the interrupt (MSI, else INTx), the register BAR, and `ahci_attach`.
//!
//! Upstream: sys/dev/pci/ahci_pci.c @ 3ce1f3f79392
//!
//! ## Deviations
//! - The softc begins with the `struct ahci_softc`, as in C; `psc_pc` is an `Option` (zero
//!   is `None`), set by attach.
//! - `ahci_devices[]`'s `ad_match` and `ad_attach` are `Option`s of function pointers;
//!   `ad_attach` returns `Result<(), Errno>`. `ahci_map_regs` and `ahci_map_intr` return
//!   `Result<(), Errno>` for the C's 0/1.
//! - On the machines here `pci_intr_map_msi` refuses MSI until ACPI's `mp_busses` exist
//!   (amd64 later in M13; arm64 has no PCI bus before M12), so the controller runs on the
//!   INTx line the firmware routed (level-triggered and shareable on the i8259 since M10f),
//!   as the C does on such a machine. The MSI path is the C's and takes over once the
//!   machine grants it.
//! - `ahci_jmb_ca` (the attachment `jmb(4)` uses) is ported but unused: `jmb(4)` is not.
//! - `ahci_unmap_intr` skips a missing interrupt handle (the C passes NULL to
//!   `pci_intr_disestablish`).

use core::cell::Cell;
use core::ffi::c_void;
use core::ptr::{self, NonNull};

use crate::dev::ic::ahci::{ahci_activate, ahci_attach, ahci_detach, ahci_intr};
use crate::dev::ic::ahcivar::{
    AHCI_F_IPMS_PROBE, AHCI_F_NO_MSI, AHCI_F_NO_NCQ, AHCI_F_NO_PMP, AhciSoftc,
};
use crate::dev::pci::pci_map::{pci_mapreg_map, pci_mapreg_type};
use crate::dev::pci::pcidevs::{
    PCI_PRODUCT_AMD_HUDSON2_SATA_1, PCI_PRODUCT_AMD_HUDSON2_SATA_2, PCI_PRODUCT_AMD_HUDSON2_SATA_3,
    PCI_PRODUCT_AMD_HUDSON2_SATA_4, PCI_PRODUCT_AMD_HUDSON2_SATA_5, PCI_PRODUCT_AMD_HUDSON2_SATA_6,
    PCI_PRODUCT_ASMEDIA_ASM1061_SATA, PCI_PRODUCT_ATI_SB600_SATA, PCI_PRODUCT_ATI_SBX00_SATA_1,
    PCI_PRODUCT_ATI_SBX00_SATA_2, PCI_PRODUCT_ATI_SBX00_SATA_3, PCI_PRODUCT_ATI_SBX00_SATA_4,
    PCI_PRODUCT_ATI_SBX00_SATA_5, PCI_PRODUCT_ATI_SBX00_SATA_6, PCI_PRODUCT_INTEL_6SERIES_AHCI_1,
    PCI_PRODUCT_INTEL_6SERIES_AHCI_2, PCI_PRODUCT_INTEL_3400_AHCI_1, PCI_PRODUCT_INTEL_3400_AHCI_2,
    PCI_PRODUCT_INTEL_3400_AHCI_3, PCI_PRODUCT_INTEL_3400_AHCI_4, PCI_PRODUCT_INTEL_6321ESB_AHCI,
    PCI_PRODUCT_INTEL_82801GBM_AHCI, PCI_PRODUCT_INTEL_82801GR_AHCI,
    PCI_PRODUCT_INTEL_82801H_AHCI_4P, PCI_PRODUCT_INTEL_82801H_AHCI_6P,
    PCI_PRODUCT_INTEL_82801HBM_AHCI, PCI_PRODUCT_INTEL_82801I_AHCI_1,
    PCI_PRODUCT_INTEL_82801I_AHCI_2, PCI_PRODUCT_INTEL_82801I_AHCI_3,
    PCI_PRODUCT_INTEL_82801JD_AHCI, PCI_PRODUCT_INTEL_82801JI_AHCI, PCI_PRODUCT_INTEL_EP80579_AHCI,
    PCI_PRODUCT_SAMSUNG2_S4LN053X01, PCI_PRODUCT_SAMSUNG2_SM951_AHCI, PCI_PRODUCT_SAMSUNG2_XP941,
    PCI_PRODUCT_VIATECH_VT8251_SATA, PCI_PRODUCT_ZHAOXIN_STORX_AHCI, PCI_VENDOR_AMD,
    PCI_VENDOR_ASMEDIA, PCI_VENDOR_ATI, PCI_VENDOR_INTEL, PCI_VENDOR_SAMSUNG2, PCI_VENDOR_VIATECH,
    PCI_VENDOR_ZHAOXIN,
};
use crate::dev::pci::pcireg::{
    PCI_CLASS_MASS_STORAGE, PCI_CLASS_REG, PCI_CLASS_SHIFT, PCI_INTERFACE_SATA_AHCI10,
    PCI_INTERFACE_SHIFT, PCI_REVISION_SHIFT, PCI_SUBCLASS_MASS_STORAGE_IDE,
    PCI_SUBCLASS_MASS_STORAGE_SATA, PCI_SUBCLASS_SHIFT, pci_class, pci_interface, pci_product,
    pci_revision, pci_subclass, pci_vendor,
};
use crate::dev::pci::pcivar::{PCI_FLAGS_MSI_ENABLED, PciAttachArgs};
use crate::kern::subr_prf::printf;
use crate::machine::bus::bus_space_unmap;
use crate::machine::intr::IPL_BIO;
use crate::machine::pci_machdep::{
    PciChipsetTag, PciIntrHandle, Pcitag, pci_conf_read, pci_conf_write, pci_intr_disestablish,
    pci_intr_establish, pci_intr_map, pci_intr_map_msi, pci_intr_string,
};
use crate::sys::device::{CfMatch, Cfattach, Device, Softc};
use crate::sys::errno::Errno;

/// `AHCI_PCI_BAR`: the register BAR (ABAR).
pub const AHCI_PCI_BAR: i32 = 0x24;
/// `AHCI_PCI_ATI_SB600_MAGIC`.
pub const AHCI_PCI_ATI_SB600_MAGIC: i32 = 0x40;
/// `AHCI_PCI_ATI_SB600_LOCKED`.
pub const AHCI_PCI_ATI_SB600_LOCKED: u32 = 0x01;

/// `struct ahci_pci_softc`.
#[repr(C)]
pub struct AhciPciSoftc {
    /// `psc_ahci`.
    pub psc_ahci: AhciSoftc,

    /// `psc_pc`.
    pub psc_pc: Cell<Option<PciChipsetTag>>,
    /// `psc_tag`.
    pub psc_tag: Cell<Option<Pcitag>>,

    /// `psc_flags`.
    pub psc_flags: Cell<i32>,
}

// SAFETY: `#[repr(C)]` with the ahci softc (itself headed by the device, all-zero valid)
// first, then `Cell`s of `Option`s of the chipset tag and the PCI tag and an integer, valid
// as zero bits as in `NvmePciSoftc`.
unsafe impl Softc for AhciPciSoftc {}

/// `ad_match` of [`AhciDevice`].
pub type AhciMatchFn = fn(pa: &PciAttachArgs) -> i32;

/// `ad_attach` of [`AhciDevice`].
pub type AhciAttachFn = fn(sc: &AhciSoftc, pa: &PciAttachArgs) -> Result<(), Errno>;

/// `struct ahci_device`: a controller that needs special handling.
pub struct AhciDevice {
    /// `ad_vendor`.
    pub ad_vendor: u32,
    /// `ad_product`.
    pub ad_product: u32,
    /// `ad_match`: decides the match instead of the table entry.
    pub ad_match: Option<AhciMatchFn>,
    /// `ad_attach`: applies the controller's quirks before the attach.
    pub ad_attach: Option<AhciAttachFn>,
}

/// An `ahci_devices[]` entry.
const fn ad(
    ad_vendor: u32,
    ad_product: u32,
    ad_match: Option<AhciMatchFn>,
    ad_attach: Option<AhciAttachFn>,
) -> AhciDevice {
    AhciDevice {
        ad_vendor,
        ad_product,
        ad_match,
        ad_attach,
    }
}

/// `ahci_devices[]`.
pub static AHCI_DEVICES: [AhciDevice; 37] = [
    ad(
        PCI_VENDOR_AMD,
        PCI_PRODUCT_AMD_HUDSON2_SATA_1,
        None,
        Some(ahci_amd_hudson2_attach),
    ),
    ad(
        PCI_VENDOR_AMD,
        PCI_PRODUCT_AMD_HUDSON2_SATA_2,
        None,
        Some(ahci_amd_hudson2_attach),
    ),
    ad(
        PCI_VENDOR_AMD,
        PCI_PRODUCT_AMD_HUDSON2_SATA_3,
        None,
        Some(ahci_amd_hudson2_attach),
    ),
    ad(
        PCI_VENDOR_AMD,
        PCI_PRODUCT_AMD_HUDSON2_SATA_4,
        None,
        Some(ahci_amd_hudson2_attach),
    ),
    ad(
        PCI_VENDOR_AMD,
        PCI_PRODUCT_AMD_HUDSON2_SATA_5,
        None,
        Some(ahci_amd_hudson2_attach),
    ),
    ad(
        PCI_VENDOR_AMD,
        PCI_PRODUCT_AMD_HUDSON2_SATA_6,
        None,
        Some(ahci_amd_hudson2_attach),
    ),
    ad(
        PCI_VENDOR_ATI,
        PCI_PRODUCT_ATI_SB600_SATA,
        None,
        Some(ahci_ati_sb600_attach),
    ),
    ad(
        PCI_VENDOR_ATI,
        PCI_PRODUCT_ATI_SBX00_SATA_1,
        None,
        Some(ahci_ati_sb700_attach),
    ),
    ad(
        PCI_VENDOR_ATI,
        PCI_PRODUCT_ATI_SBX00_SATA_2,
        None,
        Some(ahci_ati_sb700_attach),
    ),
    ad(
        PCI_VENDOR_ATI,
        PCI_PRODUCT_ATI_SBX00_SATA_3,
        None,
        Some(ahci_ati_sb700_attach),
    ),
    ad(
        PCI_VENDOR_ATI,
        PCI_PRODUCT_ATI_SBX00_SATA_4,
        None,
        Some(ahci_ati_sb700_attach),
    ),
    ad(
        PCI_VENDOR_ATI,
        PCI_PRODUCT_ATI_SBX00_SATA_5,
        None,
        Some(ahci_ati_sb700_attach),
    ),
    ad(
        PCI_VENDOR_ATI,
        PCI_PRODUCT_ATI_SBX00_SATA_6,
        None,
        Some(ahci_ati_sb700_attach),
    ),
    ad(
        PCI_VENDOR_ASMEDIA,
        PCI_PRODUCT_ASMEDIA_ASM1061_SATA,
        None,
        None,
    ),
    ad(
        PCI_VENDOR_INTEL,
        PCI_PRODUCT_INTEL_6SERIES_AHCI_1,
        None,
        Some(ahci_intel_attach),
    ),
    ad(
        PCI_VENDOR_INTEL,
        PCI_PRODUCT_INTEL_6SERIES_AHCI_2,
        None,
        Some(ahci_intel_attach),
    ),
    ad(
        PCI_VENDOR_INTEL,
        PCI_PRODUCT_INTEL_6321ESB_AHCI,
        None,
        Some(ahci_intel_attach),
    ),
    ad(
        PCI_VENDOR_INTEL,
        PCI_PRODUCT_INTEL_82801GR_AHCI,
        None,
        Some(ahci_intel_attach),
    ),
    ad(
        PCI_VENDOR_INTEL,
        PCI_PRODUCT_INTEL_82801GBM_AHCI,
        None,
        Some(ahci_intel_attach),
    ),
    ad(
        PCI_VENDOR_INTEL,
        PCI_PRODUCT_INTEL_82801H_AHCI_6P,
        None,
        Some(ahci_intel_attach),
    ),
    ad(
        PCI_VENDOR_INTEL,
        PCI_PRODUCT_INTEL_82801H_AHCI_4P,
        None,
        Some(ahci_intel_attach),
    ),
    ad(
        PCI_VENDOR_INTEL,
        PCI_PRODUCT_INTEL_82801HBM_AHCI,
        None,
        Some(ahci_intel_attach),
    ),
    ad(
        PCI_VENDOR_INTEL,
        PCI_PRODUCT_INTEL_82801I_AHCI_1,
        None,
        Some(ahci_intel_attach),
    ),
    ad(
        PCI_VENDOR_INTEL,
        PCI_PRODUCT_INTEL_82801I_AHCI_2,
        None,
        Some(ahci_intel_attach),
    ),
    ad(
        PCI_VENDOR_INTEL,
        PCI_PRODUCT_INTEL_82801I_AHCI_3,
        None,
        Some(ahci_intel_attach),
    ),
    ad(
        PCI_VENDOR_INTEL,
        PCI_PRODUCT_INTEL_82801JD_AHCI,
        None,
        Some(ahci_intel_attach),
    ),
    ad(
        PCI_VENDOR_INTEL,
        PCI_PRODUCT_INTEL_82801JI_AHCI,
        None,
        Some(ahci_intel_attach),
    ),
    ad(
        PCI_VENDOR_INTEL,
        PCI_PRODUCT_INTEL_3400_AHCI_1,
        None,
        Some(ahci_intel_attach),
    ),
    ad(
        PCI_VENDOR_INTEL,
        PCI_PRODUCT_INTEL_3400_AHCI_2,
        None,
        Some(ahci_intel_attach),
    ),
    ad(
        PCI_VENDOR_INTEL,
        PCI_PRODUCT_INTEL_3400_AHCI_3,
        None,
        Some(ahci_intel_attach),
    ),
    ad(
        PCI_VENDOR_INTEL,
        PCI_PRODUCT_INTEL_3400_AHCI_4,
        None,
        Some(ahci_intel_attach),
    ),
    ad(
        PCI_VENDOR_INTEL,
        PCI_PRODUCT_INTEL_EP80579_AHCI,
        None,
        Some(ahci_intel_attach),
    ),
    ad(
        PCI_VENDOR_SAMSUNG2,
        PCI_PRODUCT_SAMSUNG2_S4LN053X01,
        None,
        Some(ahci_samsung_attach),
    ),
    ad(
        PCI_VENDOR_SAMSUNG2,
        PCI_PRODUCT_SAMSUNG2_XP941,
        None,
        Some(ahci_samsung_attach),
    ),
    ad(
        PCI_VENDOR_SAMSUNG2,
        PCI_PRODUCT_SAMSUNG2_SM951_AHCI,
        None,
        Some(ahci_samsung_attach),
    ),
    ad(
        PCI_VENDOR_VIATECH,
        PCI_PRODUCT_VIATECH_VT8251_SATA,
        Some(ahci_no_match),
        Some(ahci_vt8251_attach),
    ),
    ad(
        PCI_VENDOR_ZHAOXIN,
        PCI_PRODUCT_ZHAOXIN_STORX_AHCI,
        None,
        Some(ahci_storx_attach),
    ),
];

/// `ahci_pci_ca`.
pub static AHCI_PCI_CA: Cfattach = Cfattach {
    ca_devsize: size_of::<AhciPciSoftc>(),
    ca_match: Some(ahci_pci_match),
    ca_attach: ahci_pci_attach,
    ca_detach: Some(ahci_pci_detach),
    ca_activate: Some(ahci_pci_activate),
};

/// `ahci_jmb_ca`: the attachment `jmb(4)` uses (not ported).
pub static AHCI_JMB_CA: Cfattach = Cfattach {
    ca_devsize: size_of::<AhciPciSoftc>(),
    ca_match: Some(ahci_pci_match),
    ca_attach: ahci_pci_attach,
    ca_detach: Some(ahci_pci_detach),
    ca_activate: None,
};

/// `ahci_lookup_device`: the `ahci_devices[]` entry of a vendor and product ID register.
pub fn ahci_lookup_device(id: u32) -> Option<&'static AhciDevice> {
    AHCI_DEVICES
        .iter()
        .find(|ad| ad.ad_vendor == pci_vendor(id) && ad.ad_product == pci_product(id))
}

/// `ahci_no_match`.
pub fn ahci_no_match(_pa: &PciAttachArgs) -> i32 {
    0
}

/// `ahci_vt8251_attach`.
pub fn ahci_vt8251_attach(sc: &AhciSoftc, _pa: &PciAttachArgs) -> Result<(), Errno> {
    sc.sc_flags.set(sc.sc_flags.get() | AHCI_F_NO_NCQ);

    Ok(())
}

/// `ahci_ati_sb_idetoahci`: switches an ATI/AMD southbridge in IDE mode to AHCI.
pub fn ahci_ati_sb_idetoahci(_sc: &AhciSoftc, pa: &PciAttachArgs) {
    if pci_subclass(pa.pa_class) == PCI_SUBCLASS_MASS_STORAGE_IDE {
        let magic = pci_conf_read(pa.pa_pc, pa.pa_tag, AHCI_PCI_ATI_SB600_MAGIC);
        pci_conf_write(
            pa.pa_pc,
            pa.pa_tag,
            AHCI_PCI_ATI_SB600_MAGIC,
            magic | AHCI_PCI_ATI_SB600_LOCKED,
        );

        pci_conf_write(
            pa.pa_pc,
            pa.pa_tag,
            PCI_CLASS_REG,
            (PCI_CLASS_MASS_STORAGE << PCI_CLASS_SHIFT)
                | (PCI_SUBCLASS_MASS_STORAGE_SATA << PCI_SUBCLASS_SHIFT)
                | (PCI_INTERFACE_SATA_AHCI10 << PCI_INTERFACE_SHIFT)
                | (pci_revision(pa.pa_class) << PCI_REVISION_SHIFT),
        );

        pci_conf_write(pa.pa_pc, pa.pa_tag, AHCI_PCI_ATI_SB600_MAGIC, magic);
    }
}

/// `ahci_ati_sb600_attach`.
pub fn ahci_ati_sb600_attach(sc: &AhciSoftc, pa: &PciAttachArgs) -> Result<(), Errno> {
    ahci_ati_sb_idetoahci(sc, pa);

    sc.sc_flags.set(sc.sc_flags.get() | AHCI_F_IPMS_PROBE);

    Ok(())
}

/// `ahci_ati_sb700_attach`.
pub fn ahci_ati_sb700_attach(sc: &AhciSoftc, pa: &PciAttachArgs) -> Result<(), Errno> {
    ahci_ati_sb_idetoahci(sc, pa);

    sc.sc_flags.set(sc.sc_flags.get() | AHCI_F_IPMS_PROBE);

    Ok(())
}

/// `ahci_amd_hudson2_attach`.
pub fn ahci_amd_hudson2_attach(sc: &AhciSoftc, pa: &PciAttachArgs) -> Result<(), Errno> {
    ahci_ati_sb_idetoahci(sc, pa);

    sc.sc_flags.set(sc.sc_flags.get() | AHCI_F_IPMS_PROBE);

    Ok(())
}

/// `ahci_intel_attach`.
pub fn ahci_intel_attach(sc: &AhciSoftc, _pa: &PciAttachArgs) -> Result<(), Errno> {
    sc.sc_flags.set(sc.sc_flags.get() | AHCI_F_NO_PMP);

    Ok(())
}

/// `ahci_samsung_attach`.
pub fn ahci_samsung_attach(sc: &AhciSoftc, _pa: &PciAttachArgs) -> Result<(), Errno> {
    // Disable MSI with the Samsung S4LN053X01 SSD controller as found in some Apple MacBook
    // Air models such as the 6,1 and 6,2, as well as the XP941 SSD controller.
    // https://bugzilla.kernel.org/show_bug.cgi?id=60731
    // https://bugzilla.kernel.org/show_bug.cgi?id=89171
    sc.sc_flags.set(sc.sc_flags.get() | AHCI_F_NO_MSI);

    Ok(())
}

/// `ahci_storx_attach`.
pub fn ahci_storx_attach(sc: &AhciSoftc, _pa: &PciAttachArgs) -> Result<(), Errno> {
    // Disable MSI with the ZX-100/ZX-200/ZX-E StorX AHCI Controller in the Unchartevice
    // 6640MA notebook, otherwise ahci(4) hangs with SATA speed set to "Gen3" in BIOS.
    sc.sc_flags.set(sc.sc_flags.get() | AHCI_F_NO_MSI);

    Ok(())
}

/// The match of `ahci_pci_match` for a function's ID and class registers, without the
/// table entries' own `ad_match`.
pub fn ahci_pci_match_class(id: u32, class: u32) -> i32 {
    if ahci_lookup_device(id).is_some() {
        return 2; // match higher than pciide
    }

    if pci_class(class) == PCI_CLASS_MASS_STORAGE
        && pci_subclass(class) == PCI_SUBCLASS_MASS_STORAGE_SATA
        && pci_interface(class) == PCI_INTERFACE_SATA_AHCI10
    {
        return 2;
    }

    0
}

/// `ahci_pci_match`.
pub fn ahci_pci_match(_parent: Option<&Device>, _match: &CfMatch, aux: *mut c_void) -> i32 {
    // SAFETY: pci attaches its children with a `pci_attach_args`.
    let pa = unsafe { &*aux.cast::<PciAttachArgs>() };

    if let Some(ad) = ahci_lookup_device(pa.pa_id) {
        // the device may need special checks to see if it matches
        if let Some(m) = ad.ad_match {
            return m(pa);
        }

        return 2; // match higher than pciide
    }

    ahci_pci_match_class(pa.pa_id, pa.pa_class)
}

/// `ahci_pci_attach`.
pub fn ahci_pci_attach(_parent: Option<&Device>, self_: &Device, aux: *mut c_void) {
    // SAFETY: `self_` was made for `ahci_pci_ca` (or `ahci_jmb_ca`), whose softc is an
    // `AhciPciSoftc`; softcs are never freed while the device exists, so the softc may be
    // borrowed for 'static.
    let psc: &'static AhciPciSoftc = unsafe { &*ptr::from_ref(self_.softc::<AhciPciSoftc>()) };
    let sc = &psc.psc_ahci;
    // SAFETY: pci attaches its children with a `pci_attach_args` it owns for the duration of
    // the attach; nothing else touches it meanwhile (the C clears a flag in it).
    let pa = unsafe { &mut *aux.cast::<PciAttachArgs>() };

    psc.psc_pc.set(Some(pa.pa_pc));
    psc.psc_tag.set(Some(pa.pa_tag));
    sc.sc_dmat.set(Some(pa.pa_dmat));

    if let Some(ad) = ahci_lookup_device(pa.pa_id)
        && let Some(attach) = ad.ad_attach
        && attach(sc, pa).is_err()
    {
        // error should be printed by ad_attach
        return;
    }

    if sc.sc_flags.get() & AHCI_F_NO_MSI != 0 {
        pa.pa_flags &= !PCI_FLAGS_MSI_ENABLED;
    }

    let Some(ih) = pci_intr_map_msi(pa).or_else(|| pci_intr_map(pa)) else {
        printf(format_args!(": unable to map interrupt\n"));
        return;
    };
    printf(format_args!(": {},", pci_intr_string(pa.pa_pc, ih)));

    if ahci_map_regs(psc, pa).is_err() {
        // error already printed by ahci_map_regs
        return;
    }

    if ahci_map_intr(psc, pa, ih).is_err() || ahci_attach(sc).is_err() {
        // error already printed by ahci_map_intr / ahci_attach
        // unmap:
        ahci_unmap_regs(psc);
    }
}

/// `ahci_pci_detach`.
pub fn ahci_pci_detach(self_: &Device, flags: i32) -> Result<(), Errno> {
    // SAFETY: `self_` was made for `ahci_pci_ca`; softcs are never freed while the device
    // exists.
    let psc: &'static AhciPciSoftc = unsafe { &*ptr::from_ref(self_.softc::<AhciPciSoftc>()) };
    let sc = &psc.psc_ahci;

    let _ = ahci_detach(sc, flags);

    ahci_unmap_intr(psc);
    ahci_unmap_regs(psc);

    Ok(())
}

/// `ahci_map_regs`: maps the ABAR.
pub fn ahci_map_regs(psc: &AhciPciSoftc, pa: &PciAttachArgs) -> Result<(), Errno> {
    let sc = &psc.psc_ahci;

    let maptype = pci_mapreg_type(pa.pa_pc, pa.pa_tag, AHCI_PCI_BAR);
    let Ok((iot, ioh, _base, ios)) = pci_mapreg_map(pa, AHCI_PCI_BAR, maptype, 0, 0) else {
        printf(format_args!(" unable to map registers\n"));
        return Err(Errno::EIO);
    };
    sc.sc_iot.set(Some(iot));
    sc.sc_ioh.set(Some(ioh));
    sc.sc_ios.set(ios);

    Ok(())
}

/// `ahci_unmap_regs`.
pub fn ahci_unmap_regs(psc: &AhciPciSoftc) {
    let sc = &psc.psc_ahci;

    if let (Some(iot), Some(ioh)) = (sc.sc_iot.get(), sc.sc_ioh.get()) {
        bus_space_unmap(iot, ioh, sc.sc_ios.get());
    }
    sc.sc_ios.set(0);
}

/// `ahci_map_intr`: establishes `ahci_intr` at `IPL_BIO`.
pub fn ahci_map_intr(
    psc: &'static AhciPciSoftc,
    pa: &PciAttachArgs,
    ih: PciIntrHandle,
) -> Result<(), Errno> {
    let sc = &psc.psc_ahci;
    let Some(cookie) = pci_intr_establish(
        pa.pa_pc,
        ih,
        IPL_BIO,
        ahci_intr,
        ptr::from_ref(sc).cast_mut().cast(),
        sc.sc_dev.xname(),
    ) else {
        printf(format_args!(
            "{}: unable to map interrupt\n",
            sc.sc_dev.xname()
        ));
        return Err(Errno::EIO);
    };
    sc.sc_ih.set(cookie.as_ptr());

    Ok(())
}

/// `ahci_unmap_intr`.
pub fn ahci_unmap_intr(psc: &AhciPciSoftc) {
    let sc = &psc.psc_ahci;
    if let (Some(pc), Some(ih)) = (
        psc.psc_pc.get(),
        NonNull::new(sc.sc_ih.replace(ptr::null_mut())),
    ) {
        // SAFETY: the handle ahci_map_intr established, dropped here.
        unsafe { pci_intr_disestablish(pc, ih) };
    }
}

/// `ahci_pci_activate`.
pub fn ahci_pci_activate(self_: &Device, act: i32) -> Result<(), Errno> {
    // SAFETY: `self_` was made for `ahci_pci_ca`; softcs are never freed while the device
    // exists.
    let psc: &'static AhciPciSoftc = unsafe { &*ptr::from_ref(self_.softc::<AhciPciSoftc>()) };
    ahci_activate(&psc.psc_ahci.sc_dev, act)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn matches_ahci_controllers() {
        // QEMU's ich9-ahci: 8086:2922 (82801I AHCI), class 01 06 01.
        assert_eq!(ahci_pci_match_class(0x2922_8086, 0x0106_0102), 2);
        let ad = ahci_lookup_device(0x2922_8086).map(|ad| ad.ad_attach.is_some());
        assert_eq!(ad, Some(true));
        // Any AHCI 1.0 SATA function matches by class.
        assert_eq!(ahci_pci_match_class(0x1234_1af4, 0x0106_0100), 2);
        // NVMe (01 08 02) and IDE (01 01 8a) functions do not.
        assert_eq!(ahci_pci_match_class(0x0010_1b36, 0x0108_0200), 0);
        assert_eq!(ahci_pci_match_class(0x7010_8086, 0x0101_8a00), 0);
        // The VT8251 is in the table, but its ad_match refuses it.
        assert!(ahci_lookup_device(0x3349_1106).is_some_and(|ad| ad.ad_match.is_some()));
    }
}
