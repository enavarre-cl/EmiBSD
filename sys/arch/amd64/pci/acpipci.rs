/*	$OpenBSD: acpipci.c,v 1.13 2026/08/10 15:14:57 hshoexer Exp $	*/
/* <LICENSES> */
/*
 * Copyright (c) 2018 Mark Kettenis
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
//! acpipci(4) on amd64: the PCI host bridges ACPI describes, `arch/amd64/pci/acpipci.c`.
//!
//! Upstream: sys/arch/amd64/pci/acpipci.c @ 3ce1f3f79392
//!
//! A `PNP0A08`/`PNP0A03` device in the namespace is a PCI host bridge: `acpipci` asks the
//! firmware for control of PCIe and MSI (`_OSC`), reads its bus (`_BBN`, `_CRS`'s bus range)
//! and segment (`_SEG`) and the address windows it forwards (`_CRS`). Once one attached,
//! `acpi_haspci` is set and mainbus attaches a `pci` bus per host bridge through
//! `acpipci_attach_busses`, with MSI enabled on an ACPI 2.0 (or QEMU) machine whose host
//! bridge is not a VIA or SiS one and whose bus is not HyperTransport.
//!
//! ## Deviations
//! - The bus, I/O and memory extents (`sys/extent.h`, `subr_extent.c`) are not ported:
//!   `extent_create`, `extent_free` and `extent_destroy` are reported once, the softc keeps
//!   no extents and `pba_busex`/`pba_ioex`/`pba_memex`/`pba_pmemex` do not exist
//!   (`pcivar.rs`). `acpipci_parse_resources` still lets the `_CRS` bus range override
//!   `_BBN`, as the C does.
//! - `_OSC`'s UUID and capabilities buffers are built as byte buffers (`AmlValue::buffer`).
//! - `ACPIPCI_DEBUG`'s `extent_print`s are not configured.

use core::cell::{Cell, RefCell};
use core::ffi::c_void;
use core::ptr;
use core::sync::atomic::Ordering;

use crate::arch::amd64::amd64::bus_space::{X86_BUS_SPACE_IO, X86_BUS_SPACE_MEM};
use crate::arch::amd64::amd64::cpu::CPU_ECXFEATURE;
use crate::arch::amd64::include::pci_machdep::PciChipsetTag;
use crate::arch::amd64::include::specialreg::CPUIDECX_HV;
use crate::arch::amd64::pci::pci_machdep::{
    PCI_BUS_DMA_TAG, pci_conf_read, pci_init_extents, pci_make_tag,
};
use crate::dev::acpi::acpi::{ACPI_HASPCI, acpi_matchhids};
use crate::dev::acpi::acpireg::FADT_NO_MSI;
use crate::dev::acpi::acpivar::{AcpiAttachArgs, AcpiSoftc};
use crate::dev::acpi::amltypes::{AML_OBJTYPE_BUFFER, AmlNodeRef, AmlValue};
use crate::dev::acpi::dsdt::{
    AcpiResource, LR_DWORD, LR_IO_TTP, LR_MEM32FIXED, LR_MEMORY_TTP, LR_QWORD, LR_TYPE_BUS,
    LR_TYPE_IO, LR_TYPE_MEMORY, LR_WORD, aml_crstype, aml_evalinteger, aml_evalname,
    aml_parse_resource, cstr,
};
use crate::dev::pci::pci::{PCI_NDOMAINS, pci_get_capability};
use crate::dev::pci::pcidevs::{
    PCI_VENDOR_AMD, PCI_VENDOR_INTEL, PCI_VENDOR_NVIDIA, PCI_VENDOR_QUMRANET,
};
use crate::dev::pci::pcireg::{
    PCI_CAP_HT, PCI_CLASS_BRIDGE, PCI_CLASS_REG, PCI_ID_REG, PCI_SUBCLASS_BRIDGE_HOST,
    PCI_SUBSYS_ID_REG, pci_class, pci_subclass, pci_vendor,
};
use crate::dev::pci::pcivar::{PCI_FLAGS_MSI_ENABLED, PcibusAttachArgs};
use crate::kern::subr_autoconf::config_found;
use crate::kern::subr_prf::Str;
use crate::sys::device::{CD_COCOVM, CfMatch, Cfattach, Cfdriver, DV_DULL, Device, Softc, UNCONF};
use crate::{kprintf, unported};

/// `ACPI_PCI_UUID`: 33DB4D5B-1FF7-401C-9657-7441C03DD766.
const ACPI_PCI_UUID: [u8; 16] = [
    0x5b, 0x4d, 0xdb, 0x33, 0xf7, 0x1f, 0x1c, 0x40, 0x96, 0x57, 0x74, 0x41, 0xc0, 0x3d, 0xd7, 0x66,
];

// Support field.

/// `ACPI_PCI_PCIE_CONFIG`.
pub const ACPI_PCI_PCIE_CONFIG: u32 = 0x0000_0001;
/// `ACPI_PCI_ASPM`.
pub const ACPI_PCI_ASPM: u32 = 0x0000_0002;
/// `ACPI_PCI_CPMC`.
pub const ACPI_PCI_CPMC: u32 = 0x0000_0004;
/// `ACPI_PCI_SEGMENTS`.
pub const ACPI_PCI_SEGMENTS: u32 = 0x0000_0008;
/// `ACPI_PCI_MSI`.
pub const ACPI_PCI_MSI: u32 = 0x0000_0010;

// Control field.

/// `ACPI_PCI_PCIE_HOTPLUG`.
pub const ACPI_PCI_PCIE_HOTPLUG: u32 = 0x0000_0001;

/// `struct acpipci_softc` (without the extents, see the deviations).
#[repr(C)]
pub struct AcpipciSoftc {
    /// `sc_dev`.
    pub sc_dev: Device,
    /// `sc_acpi`: acpi0, set by `acpipci_attach`.
    pub sc_acpi: Cell<*const AcpiSoftc>,
    /// `sc_node`: the host bridge's node, set by `acpipci_attach` (kernel lock).
    pub sc_node: RefCell<Option<AmlNodeRef>>,
    /// `sc_bus`.
    pub sc_bus: Cell<i32>,
    /// `sc_seg`.
    pub sc_seg: Cell<u32>,
    /// `sc_domain`: assigned when the PCI bus attaches.
    pub sc_domain: Cell<i32>,
}

// SAFETY: `#[repr(C)]` with the device first; zeroes are a null pointer, an unborrowed
// `RefCell` holding `None` and 0.
unsafe impl Softc for AcpipciSoftc {}

/// `acpipci_ca`.
pub static ACPIPCI_CA: Cfattach = Cfattach {
    ca_devsize: size_of::<AcpipciSoftc>(),
    ca_match: Some(acpipci_match),
    ca_attach: acpipci_attach,
    ca_detach: None,
    ca_activate: None,
};

/// `acpipci_cd`.
pub static ACPIPCI_CD: Cfdriver = Cfdriver::new(b"acpipci", DV_DULL, CD_COCOVM);

/// `acpipci_hids[]`.
pub const ACPIPCI_HIDS: [&str; 2] = ["PNP0A08", "PNP0A03"];

/// The attached host bridges, from `acpipci_cd.cd_devs`.
fn acpipci_softcs() -> impl Iterator<Item = &'static AcpipciSoftc> {
    (0..ACPIPCI_CD.cd_ndevs.get()).filter_map(|i| {
        // SAFETY: a device in `acpipci_cd` was made for `acpipci_ca` and is never freed.
        ACPIPCI_CD
            .cd_dev(i)
            .map(|d| unsafe { &*ptr::from_ref(d.as_ref().softc::<AcpipciSoftc>()) })
    })
}

/// `acpipci_domain_to_seg(domain)`: translate the OpenBSD PCI domain enumeration index back
/// to the ACPI segment number (`_SEG`); -1 for none.
pub fn acpipci_domain_to_seg(domain: i32) -> i32 {
    acpipci_softcs()
        .find(|sc| sc.sc_domain.get() == domain)
        .map_or(-1, |sc| sc.sc_seg.get() as i32)
}

/// `acpipci_match(parent, match, aux)`: a PCI host bridge's `_HID`.
pub fn acpipci_match(_parent: Option<&Device>, _match: &CfMatch, aux: *mut c_void) -> i32 {
    // SAFETY: acpi0 hands its children `struct acpi_attach_args`.
    let aaa = unsafe { &*aux.cast_const().cast::<AcpiAttachArgs>() };

    acpi_matchhids(aaa, &ACPIPCI_HIDS, "acpipci")
}

/// `acpipci_attach(parent, self, aux)`.
pub fn acpipci_attach(parent: Option<&Device>, self_: &Device, aux: *mut c_void) {
    // SAFETY: acpi0 hands its children `struct acpi_attach_args`.
    let aaa = unsafe { &*aux.cast_const().cast::<AcpiAttachArgs>() };
    // SAFETY: `self_` was made for `acpipci_ca`.
    let sc = unsafe { self_.softc::<AcpipciSoftc>() };
    let mut bbn: i64 = 0;
    let mut seg: i64 = 0;

    ACPI_HASPCI.store(1, Ordering::Relaxed);

    // sc_iot, sc_memt and sc_dmat: amd64's (see acpipci_attach_bus).

    let Some(parent) = parent else {
        return;
    };
    // SAFETY: acpipci attaches at acpi0, whose softc is an `AcpiSoftc`.
    let acpi: &AcpiSoftc = unsafe { parent.softc::<AcpiSoftc>() };
    sc.sc_acpi.set(ptr::from_ref(acpi));
    *sc.sc_node.borrow_mut() = aaa.aaa_node.clone();
    let Some(node) = aaa.aaa_node.clone() else {
        return;
    };
    kprintf!(" {}", Str(cstr(&node.name)));

    acpipci_osc(sc, acpi, &node);

    aml_evalinteger(Some(acpi), Some(&node), b"_BBN", &[], &mut bbn);
    sc.sc_bus.set(bbn as i32);

    aml_evalinteger(Some(acpi), Some(&node), b"_SEG", &[], &mut seg);
    sc.sc_seg.set(seg as u32);

    // Assigned when the PCI bus attaches.
    sc.sc_domain.set(-1);

    let res = AmlValue::new();
    if aml_evalname(Some(acpi), Some(&node), b"_CRS", &[], Some(&res)) != 0 {
        kprintf!(": can't find resources\n");

        pci_init_extents();
        // sc_busex = pcibus_ex, sc_ioex = pciio_ex, sc_memex = pcimem_ex: no extents.
        return;
    }

    // Create extents for our address spaces: extent_create of sc_busex, sc_ioex and
    // sc_memex (reported below, after the line: see the deviations).

    aml_parse_resource(&res, &mut |i, crs| acpipci_parse_resources(i, crs, sc));

    if acpi.sc_major.get() < 5 && CPU_ECXFEATURE.load(Ordering::Relaxed) & CPUIDECX_HV == 0 {
        // extent_destroy(sc_ioex), extent_destroy(sc_memex): no extents.
        pci_init_extents();
        // sc_ioex = pciio_ex, sc_memex = pcimem_ex.
    }

    kprintf!("\n");
    let _ = unported!(
        "acpipci: the bus, I/O and memory extents (extent_create, extent_free, subr_extent.c)"
    );

    // ACPIPCI_DEBUG: extent_print of the three extents.
}

/// `acpipci_attach_bus(parent, sc)`: attaches the `pci` bus behind host bridge `sc`.
pub fn acpipci_attach_bus(parent: &Device, sc: &AcpipciSoftc) {
    let mut flags = 0;
    let domain = PCI_NDOMAINS.fetch_add(1, Ordering::Relaxed);
    sc.sc_domain.set(domain);

    // SAFETY: `acpipci_attach` set it to acpi0's softc, which is never freed.
    let Some(acpi) = (unsafe { sc.sc_acpi.get().as_ref() }) else {
        return;
    };
    // SAFETY: acpi0 sets `sc_fadt` to its copy of the FADT before attaching its children.
    let Some(fadt) = (unsafe { acpi.sc_fadt.get().as_ref() }) else {
        return;
    };

    // Enable MSI in ACPI 2.0 and above, unless we're told not to.
    let revision = fadt.hdr.revision;
    let iapc_boot_arch = fadt.iapc_boot_arch;
    if revision >= 2 && iapc_boot_arch & FADT_NO_MSI == 0 {
        flags |= PCI_FLAGS_MSI_ENABLED;
    }

    // Enable MSI for QEMU claiming ACPI 1.0
    let pc: PciChipsetTag = None;
    let tag = pci_make_tag(pc, sc.sc_bus.get(), 0, 0);
    let id = pci_conf_read(pc, tag, PCI_SUBSYS_ID_REG);
    if revision == 1 && pci_vendor(id) == PCI_VENDOR_QUMRANET {
        flags |= PCI_FLAGS_MSI_ENABLED;
    }

    // Don't enable MSI on chipsets from low-end manufacturers like VIA and SiS. We do this
    // by looking at the host bridge, which should be device 0 function 0.
    let id = pci_conf_read(pc, tag, PCI_ID_REG);
    let class = pci_conf_read(pc, tag, PCI_CLASS_REG);
    if pci_class(class) == PCI_CLASS_BRIDGE
        && pci_subclass(class) != PCI_SUBCLASS_BRIDGE_HOST
        && pci_vendor(id) != PCI_VENDOR_AMD
        && pci_vendor(id) != PCI_VENDOR_NVIDIA
        && pci_vendor(id) != PCI_VENDOR_INTEL
    {
        flags &= !PCI_FLAGS_MSI_ENABLED;
    }

    // Don't enable MSI on a HyperTransport bus. In order to determine that a bus is a
    // HyperTransport bus, we look at device 24 function 0, which is the HyperTransport
    // host/primary interface integrated on most 64-bit AMD CPUs. If that device has a
    // HyperTransport capability, this must be a HyperTransport bus and we disable MSI.
    let tag = pci_make_tag(pc, sc.sc_bus.get(), 24, 0);
    if pci_get_capability(pc, tag, PCI_CAP_HT).is_some() {
        flags &= !PCI_FLAGS_MSI_ENABLED;
    }

    let mut pba = PcibusAttachArgs {
        pba_busname: b"pci",
        pba_iot: X86_BUS_SPACE_IO,
        pba_memt: X86_BUS_SPACE_MEM,
        pba_dmat: &PCI_BUS_DMA_TAG,
        pba_pc: None,
        pba_flags: flags,
        // pba_busex, pba_ioex, pba_memex, pba_pmemex: no extents (see the deviations).
        pba_domain: domain,
        pba_bus: sc.sc_bus.get(),
        pba_bridgetag: None,
        pba_bridgeih: None,
        pba_intrswiz: 0,
        pba_intrtag: 0,
    };

    let _ = config_found(parent, ptr::from_mut(&mut pba).cast(), Some(acpipci_print));
}

/// `acpipci_attach_busses(parent)`: a `pci` bus per attached host bridge (mainbus).
pub fn acpipci_attach_busses(parent: &Device) {
    for sc in acpipci_softcs() {
        acpipci_attach_bus(parent, sc);
    }
}

/// `acpipci_print(aux, pnp)`.
pub fn acpipci_print(aux: *mut c_void, pnp: Option<&[u8]>) -> i32 {
    // SAFETY: `acpipci_attach_bus` hands mainbus's `pci` child a `struct
    // pcibus_attach_args`.
    let pba = unsafe { &*aux.cast_const().cast::<PcibusAttachArgs>() };

    if let Some(pnp) = pnp {
        kprintf!("{} at {}", Str(pba.pba_busname), Str(pnp));
    }
    kprintf!(" bus {}", pba.pba_bus);
    UNCONF
}

/// `acpipci_parse_resources(crsidx, crs, arg)`: one address window of the host bridge's
/// `_CRS` (see the deviations: only the bus range has an effect).
pub fn acpipci_parse_resources(_crsidx: i32, crs: &AcpiResource<'_>, sc: &AcpipciSoftc) -> i32 {
    let typ = aml_crstype(crs);
    let mut restype: u8 = 0xff;
    let mut tflags: u8 = 0;
    let (mut min, mut len): (u64, u64) = (0, 0);

    match typ {
        LR_WORD => {
            restype = crs.lr_word_type();
            tflags = crs.lr_word_tflags();
            min = u64::from(crs.lr_word__min());
            len = u64::from(crs.lr_word__len());
        }
        LR_DWORD => {
            restype = crs.lr_dword_type();
            tflags = crs.lr_dword_tflags();
            min = u64::from(crs.lr_dword__min());
            len = u64::from(crs.lr_dword__len());
        }
        LR_QWORD => {
            restype = crs.lr_qword_type();
            tflags = crs.lr_qword_tflags();
            min = crs.lr_qword__min();
            len = crs.lr_qword__len();
        }
        LR_MEM32FIXED => {
            // Coreboot on the PC Engines apu2 incorrectly uses a Memory32Fixed resource
            // descriptor to describe mmio address space forwarded to the PCI bus.
            restype = LR_TYPE_MEMORY;
            min = u64::from(crs.lr_m32fixed__bas());
            len = u64::from(crs.lr_m32fixed__len());
        }
        _ => {}
    }
    // _tra: the translation offset, which only the extents use.

    if len == 0 {
        return 0;
    }

    match restype {
        LR_TYPE_MEMORY => {
            if tflags & LR_MEMORY_TTP != 0 {
                return 0;
            }
            // extent_free(sc->sc_memex, min, len, EX_WAITOK | EX_CONFLICTOK)
        }
        LR_TYPE_IO => {
            if tflags & LR_IO_TTP != 0 {
                return 0;
            }
            // extent_free(sc->sc_ioex, min, len, EX_WAITOK | EX_CONFLICTOK)
        }
        LR_TYPE_BUS => {
            // extent_free(sc->sc_busex, min, len, EX_WAITOK)
            // Let _CRS minimum bus number override _BBN.
            sc.sc_bus.set(min as i32);
        }
        _ => {}
    }

    0
}

/// `acpipci_osc(sc)`: asks for native PCIe configuration and MSI (`_OSC`), printing the
/// answer's words.
pub fn acpipci_osc(_sc: &AcpipciSoftc, acpi: &AcpiSoftc, node: &AmlNodeRef) {
    let buf: [u32; 3] = [
        0x0,
        ACPI_PCI_PCIE_CONFIG | ACPI_PCI_MSI,
        ACPI_PCI_PCIE_HOTPLUG,
    ];
    let mut bytes = [0u8; 12];
    for (i, w) in buf.iter().enumerate() {
        bytes[4 * i..4 * i + 4].copy_from_slice(&w.to_le_bytes());
    }
    let args = [
        AmlValue::buffer(&ACPI_PCI_UUID),
        AmlValue::integer(1),
        AmlValue::integer(3),
        AmlValue::buffer(&bytes),
    ];

    let res = AmlValue::new();
    if aml_evalname(Some(acpi), Some(node), b"_OSC", &args, Some(&res)) != 0 {
        return;
    }

    if res.r#type() == AML_OBJTYPE_BUFFER {
        kprintf!(":");
        for w in res.v_buffer().as_chunks::<4>().0 {
            kprintf!(" 0x{:08x}", u32::from_le_bytes(*w));
        }
    }
}
/* </CODE> */
