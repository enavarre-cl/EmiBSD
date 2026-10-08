/*	$OpenBSD: acpipci.c,v 1.44 2026/01/06 11:51:33 patrick Exp $	*/
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

//! acpipci(4) on arm64: the PCI host bridges ACPI describes (`PNP0A08`),
//! `arch/arm64/dev/acpipci.c`. The configuration space is the ECAM window the MCFG table
//! names (`pci_mcfg_init`, called by `acpimcfg`; `pci_lookup_segment` finds it); the bridge's
//! `_CRS` gives its bus range and the memory and I/O windows, with the offset (`_TRA`) at
//! which the CPU sees them, so the PCI bus gets bus spaces of its own that translate; INTx
//! comes from `_PRT` (or the bridge swizzle), MSI and MSI-X from the GIC's MSI frame (v2m)
//! or ITS, the requester ID mapped through the IORT; the IORT also leads a function's DMA
//! to an SMMU (`acpiiort`).
//!
//! Upstream: sys/arch/arm64/dev/acpipci.c @ 3ce1f3f79392
//!
//! ## Deviations
//! - The chipset of an MCFG window gets its interrupt members and its probe hook from
//!   `pci_mcfg_init`, as functions that reach the host bridge through the window's
//!   `am_sc` cell, which `acpipci_attach` sets (the C writes `pc_intr_v` and the members
//!   into the chipset at attach time). Before that, and on `acpipci_dummy_chipset` (whose
//!   interrupt members the C leaves NULL), they find no bridge: no mapping, no handle.
//! - The extents are created and filled from `_CRS` as in C, but `pcibus_attach_args` has no
//!   extent members yet (`pcivar.rs`), so `pba_busex`, `pba_ioex`, `pba_memex` and
//!   `pba_pmemex` are not handed to the bus: a BAR the firmware left unassigned cannot be
//!   placed (UEFI assigns them all on QEMU `virt`).
//! - The softc's bus spaces are `MaybeUninit` behind an `UnsafeCell` (the softc is zeroed
//!   memory, which is no valid table of functions), written once by the attach before the
//!   bus attaches, as `pciecam.rs` does.
//! - The translations (`struct acpipci_trans`) and the MCFG windows are leaked boxes, never
//!   freed, as the C's `malloc`s.
//! - `acpipci_intr_string` returns its text by value (the C's static buffer).
//! - The IORT is read in bounds through `acpiiort.rs`'s helpers; a node or mapping past the
//!   table ends the walk as "not found".

use core::cell::{Cell, RefCell, UnsafeCell};
use core::ffi::c_void;
use core::mem::MaybeUninit;
use core::ptr::{self, NonNull};
use core::sync::atomic::Ordering;

use alloc::boxed::Box;

use crate::arch::arm64::arm64::intr::interrupt_controllers;
use crate::arch::arm64::dev::acpiiort::{
    acpiiort_smmu_map, acpiiort_smmu_reserve_region, acpiiort_table, iort_mapping, iort_read,
};
use crate::arch::arm64::include::bus::{BusDmamap, BusSpace};
use crate::arch::arm64::include::cpu::CpuInfo;
use crate::arch::arm64::include::intr::{InterruptController, MachineIntrHandle};
use crate::dev::acpi::acpi::{acpi_find_pci, acpi_getsta, acpi_matchhids};
use crate::dev::acpi::acpidev::STA_PRESENT;
use crate::dev::acpi::acpireg::{
    ACPI_IORT_ITS, ACPI_IORT_MAPPING_SINGLE, ACPI_IORT_ROOT_COMPLEX, ACPI_IORT_SMMU,
    ACPI_IORT_SMMU_V3, AcpiIort, AcpiIortItsNode, AcpiIortNode, AcpiIortRcNode, acpi_adr_pcidev,
    acpi_adr_pcifun,
};
use crate::dev::acpi::acpivar::{AcpiAttachArgs, AcpiSoftc, acpi_softc};
use crate::dev::acpi::amltypes::{
    AML_OBJTYPE_DEVICE, AML_OBJTYPE_INTEGER, AML_OBJTYPE_NAMEREF, AML_OBJTYPE_OBJREF,
    AML_OBJTYPE_PACKAGE, AmlNodeRef, AmlValue, AmlValueRef,
};
use crate::dev::acpi::dsdt::{
    AcpiResource, LR_DWORD, LR_EXTIRQ, LR_MEM32FIXED, LR_MEMORY_TTP, LR_QWORD, LR_TYPE_BUS,
    LR_TYPE_IO, LR_TYPE_MEMORY, LR_WORD, SR_IRQ, aml_crstype, aml_evalinteger, aml_evalname,
    aml_getname, aml_parse_resource, aml_searchrel, cstr,
};
use crate::dev::pci::pci::{PCI_NDOMAINS, pci_requester_id};
use crate::dev::pci::pcidevs::{PCI_PRODUCT_QUALCOMM_SC8280XP_PCIE, PCI_VENDOR_QUALCOMM};
use crate::dev::pci::pcireg::{PCI_ID_REG, PCIE_CONFIG_SPACE_SIZE, pci_product, pci_vendor};
use crate::dev::pci::pcivar::{
    PCI_FLAGS_MSI_ENABLED, PCI_FLAGS_MSIVEC_ENABLED, PciAttachArgs, PcibusAttachArgs, Pcireg,
};
use crate::dev::pci::ppbreg::ppb_interrupt_swizzle;
use crate::kassert;
use crate::kern::kern_malloc::{free, malloc};
use crate::kern::subr_autoconf::config_found;
use crate::kern::subr_extent::{extent_create, extent_free};
use crate::kern::subr_prf::{Str, panic, printf, snprintf};
use crate::machine::acpi_machdep::{acpi_intr_disestablish, acpi_intr_establish};
use crate::machine::bus::{
    BUS_DMA_WAITOK, BusAddr, BusDmaSegment, BusDmaTag, BusSize, BusSpaceHandle, BusSpaceTag,
    bus_dmamap_create, bus_dmamap_destroy, bus_dmamap_load_raw, bus_dmamap_unload, bus_space_map,
    bus_space_read_4, bus_space_write_4,
};
use crate::machine::cpu::Cpu;
use crate::machine::pci_chipset::{
    _pci_intr_map_msi, _pci_intr_map_msivec, _pci_intr_map_msix, MachinePciChipset, PCI_INTX,
    PCI_MSI, PCI_MSIX, PCI_NONE, PciIntrHandle, bus_space_mmap, pci_msi_enable, pci_msix_enable,
};
use crate::machine::pci_machdep::{PciChipsetTag, PciIntrFn, PciIntrStr, Pcitag, pci_conf_read};
use crate::sys::device::{CfMatch, Cfattach, Cfdriver, DV_DULL, Device, Softc};
use crate::sys::errno::Errno;
use crate::sys::extent::{EX_FILLED, EX_WAITOK, Extent};
use crate::sys::malloc::{M_DEVBUF, M_WAITOK};
use crate::sys::queue::{SlistEntry, SlistHead};
use crate::sys::types::{Off, Paddr};

/// `struct acpipci_mcfg`: an MCFG window, the configuration space of a segment's buses.
pub struct AcpipciMcfg {
    /// `am_list`: link in `acpipci_mcfgs`.
    pub am_list: SlistEntry<AcpipciMcfg>,
    /// `am_segment`.
    pub am_segment: u16,
    /// `am_min_bus`.
    pub am_min_bus: u8,
    /// `am_max_bus`.
    pub am_max_bus: u8,
    /// `am_iot`.
    pub am_iot: BusSpaceTag,
    /// `am_ioh`: the window, mapped.
    pub am_ioh: BusSpaceHandle,
    /// `am_pc`: the segment's chipset (see the deviations for its interrupt members).
    pub am_pc: MachinePciChipset,
    /// The host bridge that attached on this window (the C's `pc_intr_v`), set once by
    /// `acpipci_attach`.
    pub am_sc: Cell<*const AcpipciSoftc>,
}

crate::queue_adapter!(
    /// `SLIST_HEAD(, acpipci_mcfg)`: through `am_list`.
    pub AcpipciMcfgList: AcpipciMcfg, am_list => SlistEntry<AcpipciMcfg>
);

/// `struct acpipci_trans`: a window of the bridge and the offset the CPU sees it at.
pub struct AcpipciTrans {
    /// `at_next`.
    pub at_next: *const AcpipciTrans,
    /// `at_iot`: the parent's bus space.
    pub at_iot: BusSpaceTag,
    /// `at_base`: the window on the PCI bus.
    pub at_base: BusAddr,
    /// `at_size`.
    pub at_size: BusSize,
    /// `at_offset`: `_TRA`, added for the CPU's address.
    pub at_offset: BusSize,
}

/// `struct acpipci_softc`.
#[repr(C)]
pub struct AcpipciSoftc {
    /// `sc_dev`.
    pub sc_dev: Device,
    /// `sc_acpi`: acpi0.
    pub sc_acpi: Cell<*const AcpiSoftc>,
    /// `sc_node`: the host bridge's node.
    pub sc_node: RefCell<Option<AmlNodeRef>>,
    /// `sc_iot`: acpi0's memory space.
    pub sc_iot: Cell<Option<BusSpaceTag>>,
    /// `sc_pc`: the segment's chipset.
    pub sc_pc: Cell<Option<PciChipsetTag>>,
    /// `sc_bus_iot`: the PCI bus's I/O space (translating through `sc_io_trans`).
    pub sc_bus_iot: UnsafeCell<MaybeUninit<BusSpace>>,
    /// `sc_bus_memt`: the PCI bus's memory space (translating through `sc_mem_trans`).
    pub sc_bus_memt: UnsafeCell<MaybeUninit<BusSpace>>,
    /// `sc_io_trans`.
    pub sc_io_trans: Cell<*const AcpipciTrans>,
    /// `sc_mem_trans`.
    pub sc_mem_trans: Cell<*const AcpipciTrans>,
    /// `sc_busex`.
    pub sc_busex: Cell<Option<&'static Extent>>,
    /// `sc_memex`.
    pub sc_memex: Cell<Option<&'static Extent>>,
    /// `sc_ioex`.
    pub sc_ioex: Cell<Option<&'static Extent>>,
    /// `sc_busex_name[32]`, written once by the attach.
    pub sc_busex_name: UnsafeCell<[u8; 32]>,
    /// `sc_ioex_name[32]`.
    pub sc_ioex_name: UnsafeCell<[u8; 32]>,
    /// `sc_memex_name[32]`.
    pub sc_memex_name: UnsafeCell<[u8; 32]>,
    /// `sc_bus`.
    pub sc_bus: Cell<i32>,
    /// `sc_seg`.
    pub sc_seg: Cell<u32>,
    /// `sc_msi_ic`: the first controller that establishes MSIs.
    pub sc_msi_ic: Cell<*const InterruptController>,
}

impl AcpipciSoftc {
    /// `&sc->sc_bus_iot`, the bus space the attach filled.
    fn bus_iot(&'static self) -> &'static BusSpace {
        // SAFETY: written by `acpipci_attach` before the bus can reach the softc; never
        // written again.
        unsafe { (*self.sc_bus_iot.get()).assume_init_ref() }
    }

    /// `&sc->sc_bus_memt`.
    fn bus_memt(&'static self) -> &'static BusSpace {
        // SAFETY: as for `bus_iot`.
        unsafe { (*self.sc_bus_memt.get()).assume_init_ref() }
    }

    /// acpi0, set by the attach.
    fn acpi(&self) -> Option<&'static AcpiSoftc> {
        // SAFETY: set by `acpipci_attach` to acpi0's softc, which is never freed.
        unsafe { self.sc_acpi.get().as_ref() }
    }

    /// One of the extent names, formatted once into its buffer.
    fn exname(
        cell: &'static UnsafeCell<[u8; 32]>,
        args: core::fmt::Arguments<'_>,
    ) -> &'static [u8] {
        // SAFETY: the attach formats each name once, before the extent that keeps it exists;
        // nothing writes it afterwards.
        let buf = unsafe { &mut *cell.get() };
        let n = snprintf(buf, args).min(buf.len() - 1);
        &buf[..n]
    }
}

// SAFETY: `#[repr(C)]` with the device first; the other members are `Cell`s of null
// pointers, `None` and 0, an unborrowed `RefCell` of `None`, zeroed byte arrays and
// `MaybeUninit`: all valid as zero bits.
unsafe impl Softc for AcpipciSoftc {}

/// `struct acpipci_intr_handle`: an MSI's handle; the controller's handle first.
#[repr(C)]
pub struct AcpipciIntrHandle {
    /// `aih_ih`.
    pub aih_ih: MachineIntrHandle,
    /// `aih_dmat`.
    pub aih_dmat: BusDmaTag,
    /// `aih_map`: the doorbell's map.
    pub aih_map: &'static BusDmamap,
}

/// `acpipci_ca`.
pub static ACPIPCI_CA: Cfattach = Cfattach {
    ca_devsize: size_of::<AcpipciSoftc>(),
    ca_match: Some(acpipci_match),
    ca_attach: acpipci_attach,
    ca_detach: None,
    ca_activate: None,
};

/// `acpipci_cd`.
pub static ACPIPCI_CD: Cfdriver = Cfdriver::new(b"acpipci", DV_DULL, 0);

/// `acpipci_hids[]`.
pub const ACPIPCI_HIDS: [&str; 1] = ["PNP0A08"];

/// `acpipci_mcfgs`, behind a wrapper that can be a `static`.
pub struct AcpipciMcfgs(SlistHead<AcpipciMcfgList>);

// SAFETY: `pci_mcfg_init` inserts while acpi0 attaches its table devices (kernel lock,
// autoconfiguration); the list is only read afterwards, as the C's unlocked list.
unsafe impl Sync for AcpipciMcfgs {}

/// `acpipci_mcfgs`.
pub static ACPIPCI_MCFGS: AcpipciMcfgs = AcpipciMcfgs(SlistHead::new());

/// `acpipci_dummy_chipset`: the chipset of a segment no MCFG window covers; every
/// configuration read gives all ones.
pub static ACPIPCI_DUMMY_CHIPSET: MachinePciChipset = MachinePciChipset {
    pc_conf_v: ptr::null_mut(),
    pc_attach_hook: acpipci_attach_hook,
    pc_bus_maxdevs: acpipci_bus_maxdevs,
    pc_make_tag: acpipci_make_tag,
    pc_decompose_tag: acpipci_decompose_tag,
    pc_conf_size: acpipci_conf_size,
    pc_conf_read: acpipci_dummy_conf_read,
    pc_conf_write: acpipci_dummy_conf_write,
    pc_probe_device_hook: acpipci_probe_device_hook,

    pc_intr_v: ptr::null_mut(),
    pc_intr_map: acpipci_intr_map,
    pc_intr_map_msi: _pci_intr_map_msi,
    pc_intr_map_msivec: _pci_intr_map_msivec,
    pc_intr_map_msix: _pci_intr_map_msix,
    pc_intr_string: acpipci_intr_string,
    pc_intr_establish: acpipci_intr_establish,
    pc_intr_disestablish: acpipci_intr_disestablish,
};

/// The MCFG window behind a chipset cookie (`pc_conf_v`, `pc_intr_v`); `None` for the dummy
/// chipset's NULL.
fn mcfg_of(v: *mut c_void) -> Option<&'static AcpipciMcfg> {
    // SAFETY: `pci_mcfg_init` makes a leaked `AcpipciMcfg` the cookie of its chipset, and
    // only the chipset's functions call this.
    unsafe { v.cast::<AcpipciMcfg>().as_ref() }
}

/// The host bridge behind a chipset's interrupt cookie, once attached.
fn softc_of(v: *mut c_void) -> Option<&'static AcpipciSoftc> {
    // SAFETY: `am_sc` is null or the softc of the bridge that attached on the window,
    // never freed (no detach).
    mcfg_of(v).and_then(|am| unsafe { am.am_sc.get().as_ref() })
}

/// The IORT node at `offset`, as the pointer `acpiiort_smmu_map` compares.
fn iort_node_ptr(iort: &[u8], offset: usize) -> *const AcpiIortNode {
    iort[offset..].as_ptr().cast()
}

/// `acpipci_match(parent, match, aux)`.
pub fn acpipci_match(_parent: Option<&Device>, _match: &CfMatch, aux: *mut c_void) -> i32 {
    // SAFETY: acpi0 hands its children `struct acpi_attach_args`.
    let aaa = unsafe { &*aux.cast_const().cast::<AcpiAttachArgs>() };

    acpi_matchhids(aaa, &ACPIPCI_HIDS, "acpipci")
}

/// `acpipci_attach(parent, self, aux)`.
pub fn acpipci_attach(parent: Option<&Device>, self_: &Device, aux: *mut c_void) {
    // SAFETY: as in `acpipci_match`.
    let aaa = unsafe { &*aux.cast_const().cast::<AcpiAttachArgs>() };
    // SAFETY: `acpipci_ca` made the device, as an `AcpipciSoftc`; never detached, so the
    // softc lives as long as the kernel.
    let sc: &'static AcpipciSoftc = unsafe { &*ptr::from_ref(self_.softc::<AcpipciSoftc>()) };
    let mut bbn: i64 = 0;
    let mut seg: i64 = 0;

    let Some(parent) = parent else {
        return;
    };
    // SAFETY: acpipci attaches at acpi0, whose softc is an `AcpiSoftc`.
    let acpi: &'static AcpiSoftc = unsafe { &*ptr::from_ref(parent.softc::<AcpiSoftc>()) };
    sc.sc_acpi.set(ptr::from_ref(acpi));
    *sc.sc_node.borrow_mut() = aaa.aaa_node.clone();
    let Some(node) = aaa.aaa_node.clone() else {
        return;
    };
    printf(format_args!(" {}", Str(cstr(&node.name))));

    let res = AmlValue::new();
    if aml_evalname(Some(acpi), Some(&node), b"_CRS", &[], Some(&res)) != 0 {
        printf(format_args!(": can't find resources\n"));
        return;
    }

    aml_evalinteger(Some(acpi), Some(&node), b"_BBN", &[], &mut bbn);
    sc.sc_bus.set(bbn as i32);

    aml_evalinteger(Some(acpi), Some(&node), b"_SEG", &[], &mut seg);
    sc.sc_seg.set(seg as u32);

    let Some(iot) = aaa.aaa_memt else {
        printf(format_args!(": no memory space\n"));
        return;
    };
    sc.sc_iot.set(Some(iot));

    printf(format_args!("\n"));

    // Create extents for our address spaces.
    let xname = sc.sc_dev.xname();
    let busex_name = AcpipciSoftc::exname(&sc.sc_busex_name, format_args!("{} pcibus", xname));
    let ioex_name = AcpipciSoftc::exname(&sc.sc_ioex_name, format_args!("{} pciio", xname));
    let memex_name = AcpipciSoftc::exname(&sc.sc_memex_name, format_args!("{} pcimem", xname));
    sc.sc_busex.set(extent_create(
        busex_name,
        0,
        255,
        M_DEVBUF,
        None,
        EX_WAITOK | EX_FILLED,
    ));
    sc.sc_ioex.set(extent_create(
        ioex_name,
        0,
        0xffff_ffff,
        M_DEVBUF,
        None,
        EX_WAITOK | EX_FILLED,
    ));
    sc.sc_memex.set(extent_create(
        memex_name,
        0,
        u64::MAX,
        M_DEVBUF,
        None,
        EX_WAITOK | EX_FILLED,
    ));

    aml_parse_resource(&res, &mut |i, crs| acpipci_parse_resources(i, crs, sc));

    // SAFETY: attach time, before the bus exists: nothing reads the bus spaces yet; each is
    // written once.
    unsafe {
        (*sc.sc_bus_iot.get()).write(BusSpace {
            bus_private: sc.sc_io_trans.get().cast_mut().cast(),
            _space_map: acpipci_bs_map,
            _space_mmap: acpipci_bs_mmap,
            ..*iot
        });
        (*sc.sc_bus_memt.get()).write(BusSpace {
            bus_private: sc.sc_mem_trans.get().cast_mut().cast(),
            _space_map: acpipci_bs_map,
            _space_mmap: acpipci_bs_mmap,
            ..*iot
        });
    }

    let ic = interrupt_controllers()
        .iter()
        .find(|ic| ic.ic_establish_msi.is_some());
    sc.sc_msi_ic.set(ic.map_or(ptr::null(), ptr::from_ref));

    let pc = pci_lookup_segment(sc.sc_seg.get() as i32, sc.sc_bus.get());
    sc.sc_pc.set(Some(pc));
    if let Some(am) = mcfg_of(pc.pc_conf_v) {
        kassert!(am.am_sc.get().is_null());
        // pc_probe_device_hook, pc_intr_v and the interrupt members: see the deviations.
        am.am_sc.set(ptr::from_ref(sc));
    }

    let mut pba = PcibusAttachArgs {
        pba_busname: b"pci",
        pba_iot: sc.bus_iot(),
        pba_memt: sc.bus_memt(),
        pba_dmat: aaa.aaa_dmat.unwrap_or_else(|| {
            panic(format_args!("acpipci_attach: no DMA tag"));
        }),
        pba_pc: pc,
        pba_flags: 0,
        // pba_busex = sc_busex, pba_ioex = sc_ioex, pba_memex = pba_pmemex = sc_memex:
        // see the deviations.
        pba_domain: PCI_NDOMAINS.fetch_add(1, Ordering::Relaxed),
        pba_bus: sc.sc_bus.get(),
        pba_bridgetag: None,
        pba_bridgeih: None,
        pba_intrswiz: 0,
        pba_intrtag: 0,
    };
    if !sc.sc_msi_ic.get().is_null() {
        pba.pba_flags |= PCI_FLAGS_MSI_ENABLED;
        pba.pba_flags |= PCI_FLAGS_MSIVEC_ENABLED;
    }

    let _ = config_found(self_, ptr::from_mut(&mut pba).cast(), None);
}

/// Pushes a window on a translation list (`at->at_next = *head; *head = at`).
fn acpipci_trans_push(head: &Cell<*const AcpipciTrans>, at: AcpipciTrans) {
    let at = Box::leak(Box::new(AcpipciTrans {
        at_next: head.get(),
        ..at
    }));
    head.set(at);
}

/// `acpipci_parse_resources(crsidx, crs, arg)`: one window of the bridge's `_CRS`, freed in
/// its extent and recorded with its translation.
pub fn acpipci_parse_resources(_crsidx: i32, crs: &AcpiResource<'_>, sc: &AcpipciSoftc) -> i32 {
    let typ = aml_crstype(crs);
    let (restype, tflags, min, len, tra): (u8, u8, u64, u64, u64) = match typ {
        LR_WORD => (
            crs.lr_word_type(),
            crs.lr_word_tflags(),
            u64::from(crs.lr_word__min()),
            u64::from(crs.lr_word__len()),
            u64::from(crs.lr_word__tra()),
        ),
        LR_DWORD => (
            crs.lr_dword_type(),
            crs.lr_dword_tflags(),
            u64::from(crs.lr_dword__min()),
            u64::from(crs.lr_dword__len()),
            u64::from(crs.lr_dword__tra()),
        ),
        LR_QWORD => (
            crs.lr_qword_type(),
            crs.lr_qword_tflags(),
            crs.lr_qword__min(),
            crs.lr_qword__len(),
            crs.lr_qword__tra(),
        ),
        LR_MEM32FIXED => (
            LR_TYPE_MEMORY,
            0,
            u64::from(crs.lr_m32fixed__bas()),
            u64::from(crs.lr_m32fixed__len()),
            0,
        ),
        // The C leaves `len` at 0 for the other descriptors.
        _ => (0xff, 0, 0, 0, 0),
    };

    if len == 0 {
        return 0;
    }

    let Some(iot) = sc.sc_iot.get() else {
        return 0;
    };
    let window = AcpipciTrans {
        at_next: ptr::null(),
        at_iot: iot,
        at_base: min as BusAddr,
        at_size: len as BusSize,
        at_offset: tra as BusSize,
    };

    match restype {
        LR_TYPE_MEMORY => {
            if tflags & LR_MEMORY_TTP != 0 {
                return 0;
            }
            if let Some(ex) = sc.sc_memex.get() {
                let _ = extent_free(ex, min, len, EX_WAITOK);
            }
            acpipci_trans_push(&sc.sc_mem_trans, window);
        }
        LR_TYPE_IO => {
            // Don't check _TTP as various firmwares don't set it, even though they should!!
            if let Some(ex) = sc.sc_ioex.get() {
                let _ = extent_free(ex, min, len, EX_WAITOK);
            }
            acpipci_trans_push(&sc.sc_io_trans, window);
        }
        LR_TYPE_BUS => {
            if let Some(ex) = sc.sc_busex.get() {
                let _ = extent_free(ex, min, len, EX_WAITOK);
            }
            // Let _CRS minimum bus number override _BBN.
            sc.sc_bus.set(min as i32);
        }
        _ => {}
    }

    0
}

/// `acpipci_attach_hook`: nothing to do.
pub fn acpipci_attach_hook(_parent: &Device, _self: &Device, _pba: &PcibusAttachArgs) {}

/// `acpipci_bus_maxdevs`: 32 devices per bus.
pub fn acpipci_bus_maxdevs(_v: *mut c_void, _bus: i32) -> i32 {
    32
}

/// `acpipci_make_tag`: the function's offset in an ECAM window.
pub fn acpipci_make_tag(_v: *mut c_void, bus: i32, device: i32, function: i32) -> Pcitag {
    ((bus as u64) << 20) | ((device as u64) << 15) | ((function as u64) << 12)
}

/// `acpipci_decompose_tag`: `(bus, device, function)`.
pub fn acpipci_decompose_tag(_v: *mut c_void, tag: Pcitag) -> (i32, i32, i32) {
    (
        ((tag >> 20) & 0xff) as i32,
        ((tag >> 15) & 0x1f) as i32,
        ((tag >> 12) & 0x7) as i32,
    )
}

/// `acpipci_conf_size`: the whole extended configuration space.
pub fn acpipci_conf_size(_v: *mut c_void, _tag: Pcitag) -> i32 {
    PCIE_CONFIG_SPACE_SIZE
}

/// Whether `tag` addresses a bus the window covers.
fn acpipci_tag_in_window(am: &AcpipciMcfg, tag: Pcitag) -> bool {
    tag >= u64::from(am.am_min_bus) << 20 && tag < (u64::from(am.am_max_bus) + 1) << 20
}

/// `acpipci_conf_read`: all ones outside the window's buses.
pub fn acpipci_conf_read(v: *mut c_void, tag: Pcitag, reg: i32) -> Pcireg {
    let Some(am) = mcfg_of(v) else {
        return 0xffff_ffff;
    };

    if !acpipci_tag_in_window(am, tag) {
        return 0xffff_ffff;
    }

    bus_space_read_4(am.am_iot, am.am_ioh, (tag | reg as u64) as usize)
}

/// `acpipci_conf_write`: nothing outside the window's buses.
pub fn acpipci_conf_write(v: *mut c_void, tag: Pcitag, reg: i32, data: Pcireg) {
    let Some(am) = mcfg_of(v) else {
        return;
    };

    if !acpipci_tag_in_window(am, tag) {
        return;
    }

    bus_space_write_4(am.am_iot, am.am_ioh, (tag | reg as u64) as usize, data);
}

/// `acpipci_probe_device_hook(v, pa)`: the function's DMA through the SMMU its requester ID
/// maps to in the IORT, with the bridge's windows reserved there.
pub fn acpipci_probe_device_hook(v: *mut c_void, pa: &mut PciAttachArgs) -> i32 {
    let Some(am) = mcfg_of(v) else {
        return 0;
    };
    let mut rid = u32::from(pci_requester_id(pa.pa_pc, pa.pa_tag));

    // Look for IORT table.
    let Some(iort) = acpi_softc().and_then(acpiiort_table) else {
        return 0;
    };
    let Some(hdr) = iort_read::<AcpiIort>(iort, 0) else {
        return 0;
    };

    // Find our root complex.
    let mut offset = hdr.offset as usize;
    let mut found = None;
    for _ in 0..hdr.number_of_nodes {
        let Some(node) = iort_read::<AcpiIortNode>(iort, offset) else {
            return 0;
        };
        if node.r#type == ACPI_IORT_ROOT_COMPLEX
            && let Some(rc) = iort_read::<AcpiIortRcNode>(iort, offset + size_of::<AcpiIortNode>())
            && rc.segment == u32::from(am.am_segment)
        {
            found = Some((offset, node));
            break;
        }
        offset += usize::from(node.length);
    }

    // No RC found? Weird.
    let Some((noffset, node)) = found else {
        return 0;
    };

    // Find our output base towards SMMU.
    let mut out = None;
    for i in 0..node.number_of_mappings as usize {
        let Some(map) = iort_mapping(iort, noffset, &node, i) else {
            return 0;
        };

        if map.flags & ACPI_IORT_MAPPING_SINGLE != 0 {
            rid = map.output_base;
            out = Some(map.output_reference);
            break;
        }

        // Mapping encodes number of IDs in the range minus one.
        if map.input_base <= rid && rid <= map.input_base.wrapping_add(map.number_of_ids) {
            rid = map.output_base.wrapping_add(rid - map.input_base);
            out = Some(map.output_reference);
            break;
        }
    }

    // No mapping found? Even weirder.
    let Some(offset) = out else {
        return 0;
    };

    let offset = offset as usize;
    let Some(node) = iort_read::<AcpiIortNode>(iort, offset) else {
        return 0;
    };
    if node.r#type == ACPI_IORT_SMMU || node.r#type == ACPI_IORT_SMMU_V3 {
        let np = iort_node_ptr(iort, offset);
        pa.pa_dmat = acpiiort_smmu_map(np, rid, Some(pa.pa_dmat)).unwrap_or(pa.pa_dmat);
        for list in [pa.pa_iot.bus_private, pa.pa_memt.bus_private] {
            let mut at = list.cast_const().cast::<AcpipciTrans>();
            // SAFETY: the bus spaces of an acpipci bus carry its translation lists, leaked
            // boxes never freed.
            while let Some(t) = unsafe { at.as_ref() } {
                acpiiort_smmu_reserve_region(np, rid, t.at_base, t.at_size);
                at = t.at_next;
            }
        }
    }

    0
}

/// `acpipci_intr_swizzle(pa, ihp)`: the bridge's handle for the swizzled pin.
pub fn acpipci_intr_swizzle(pa: &PciAttachArgs) -> Option<PciIntrHandle> {
    let bridgeih = pa.pa_bridgeih?;

    let (_, dev, _) = crate::machine::pci_machdep::pci_decompose_tag(pa.pa_pc, pa.pa_tag);
    let mut swizpin = ppb_interrupt_swizzle(i32::from(pa.pa_rawintrpin), dev);

    // Qualcomm SC8280XP Root Complex violates PCI bridge interrupt swizzling rules.
    if let Some(bridgetag) = pa.pa_bridgetag {
        let id = pci_conf_read(pa.pa_pc, *bridgetag, PCI_ID_REG);
        if pci_vendor(id) == PCI_VENDOR_QUALCOMM
            && pci_product(id) == PCI_PRODUCT_QUALCOMM_SC8280XP_PCIE
        {
            swizpin = (((swizpin - 1) + 3) % 4) + 1;
        }
    }

    let ih = bridgeih.get((swizpin - 1) as usize)?;
    if ih.ih_type == PCI_NONE {
        return None;
    }

    Some(*ih)
}

/// `acpipci_getirq(crsidx, crs, arg)`: the interrupt of a link device's `_CRS`.
pub fn acpipci_getirq(_crsidx: i32, crs: &AcpiResource<'_>, irq: &mut i32) -> i32 {
    match aml_crstype(crs) {
        SR_IRQ => {
            // ffs(letoh16(irq_mask)) - 1
            let mask = crs.sr_irq_irq_mask();
            *irq = if mask == 0 {
                -1
            } else {
                mask.trailing_zeros() as i32
            };
        }
        LR_EXTIRQ => {
            *irq = crs.lr_extirq_irq(0) as i32;
        }
        _ => {}
    }

    0
}

/// `acpipci_intr_link(sc, node, val)`: the interrupt of the link device a `_PRT` entry
/// names; -1 for none.
pub fn acpipci_intr_link(sc: &AcpipciSoftc, node: &AmlNodeRef, val: &AmlValueRef) -> i32 {
    let Some(acpi) = sc.acpi() else {
        return -1;
    };
    let mut val = val.clone();

    if val.r#type() == AML_OBJTYPE_NAMEREF {
        let name = val
            .v_nameref()
            .map(|p| aml_getname(p.tail()))
            .unwrap_or_default();
        if let Some(n) = aml_searchrel(Some(node), &name)
            && let Some(v) = n.value()
        {
            val = v;
        }
    }
    if val.r#type() == AML_OBJTYPE_OBJREF
        && let Some(r) = val.v_objref().and_then(|o| o.r#ref)
    {
        val = r;
    }
    if val.r#type() != AML_OBJTYPE_DEVICE {
        return -1;
    }
    let Some(dev) = val.node() else {
        return -1;
    };

    let sta = acpi_getsta(acpi, Some(&dev));
    if sta & i64::from(STA_PRESENT) == 0 {
        return -1;
    }

    let res = AmlValue::new();
    if aml_evalname(Some(acpi), Some(&dev), b"_CRS", &[], Some(&res)) != 0 {
        return -1;
    }
    let mut irq = -1;
    aml_parse_resource(&res, &mut |i, crs| acpipci_getirq(i, crs, &mut irq));

    irq
}

/// `acpipci_intr_map(pa, ihp)`: the INTx line from the `_PRT` of the bridge above the
/// function (or of the host bridge), or the swizzle when there is none.
pub fn acpipci_intr_map(pa: &PciAttachArgs) -> Option<PciIntrHandle> {
    let sc = softc_of(pa.pa_pc.pc_intr_v)?;
    let acpi = sc.acpi()?;
    let mut node = sc.sc_node.borrow().clone()?;

    // If we're behind a bridge, we need to look for a _PRT for it. If we don't find a
    // _PRT, we need to swizzle. If we're not behind a bridge we need to look for a _PRT on
    // the host bridge node itself.
    if let Some(bridgetag) = pa.pa_bridgetag {
        match acpi_find_pci(pa.pa_pc, *bridgetag) {
            Some(n) => node = n,
            None => return acpipci_intr_swizzle(pa),
        }
    }

    let res = AmlValue::new();
    if aml_evalname(Some(acpi), Some(&node), b"_PRT", &[], Some(&res)) != 0 {
        return acpipci_intr_swizzle(pa);
    }

    if res.r#type() != AML_OBJTYPE_PACKAGE {
        return None;
    }

    for i in 0..res.length().max(0) as usize {
        let Some(val) = res.v_package(i) else {
            continue;
        };

        if val.r#type() != AML_OBJTYPE_PACKAGE {
            continue;
        }
        if val.length() != 4 {
            continue;
        }
        let elem = |j: usize| val.v_package(j);
        let (Some(e0), Some(e1), Some(e2), Some(e3)) = (elem(0), elem(1), elem(2), elem(3)) else {
            continue;
        };
        if e0.r#type() != AML_OBJTYPE_INTEGER
            || e1.r#type() != AML_OBJTYPE_INTEGER
            || e3.r#type() != AML_OBJTYPE_INTEGER
        {
            continue;
        }

        let addr = e0.v_integer() as u64;
        let pin = e1.v_integer() as u64;
        if u32::from(acpi_adr_pcidev(addr)) != pa.pa_device
            || acpi_adr_pcifun(addr) != 0xffff
            || pin != u64::from(pa.pa_intrpin).wrapping_sub(1)
        {
            continue;
        }

        let (source, index): (u64, u64) = if e2.r#type() == AML_OBJTYPE_INTEGER {
            (e2.v_integer() as u64, e3.v_integer() as u64)
        } else {
            (0, acpipci_intr_link(sc, &node, &e2) as i64 as u64)
        };
        if source != 0 || index == u64::MAX {
            continue;
        }

        return Some(PciIntrHandle {
            ih_pc: pa.pa_pc,
            ih_tag: pa.pa_tag,
            ih_intrpin: index as i32,
            ih_type: PCI_INTX,
            ih_dmat: pa.pa_dmat,
        });
    }

    None
}

/// `acpipci_intr_string(v, ih)`.
pub fn acpipci_intr_string(_v: *mut c_void, ih: PciIntrHandle) -> PciIntrStr {
    match ih.ih_type {
        PCI_MSI => PciIntrStr::new(format_args!("msi")),
        PCI_MSIX => PciIntrStr::new(format_args!("msix")),
        _ => PciIntrStr::new(format_args!("irq {}", ih.ih_intrpin)),
    }
}

/// `acpipci_intr_establish(v, ih, level, ci, func, arg, name)`: an MSI or MSI-X through
/// the MSI controller (the requester ID mapped through the IORT as sideband data), the
/// doorbell mapped for the device and programmed into it; or the INTx line through
/// `acpi_intr_establish`, on the primary CPU only.
#[allow(clippy::too_many_arguments)] // the C's signature
pub fn acpipci_intr_establish(
    v: *mut c_void,
    ih: PciIntrHandle,
    level: i32,
    ci: Option<&'static CpuInfo>,
    func: PciIntrFn,
    arg: *mut c_void,
    name: &'static str,
) -> Option<NonNull<c_void>> {
    let sc = softc_of(v)?;

    kassert!(ih.ih_type != PCI_NONE);

    if ih.ih_type != PCI_INTX {
        // SAFETY: a registered interrupt controller, never freed.
        let mut ic: &'static InterruptController = unsafe { sc.sc_msi_ic.get().as_ref() }?;
        let mut addr: u64 = 0;

        // Map Requester ID through IORT to get sideband data.
        let mut data = u64::from(acpipci_iort_map_msi(ih.ih_pc, ih.ih_tag, &mut ic));
        let establish_msi = ic.ic_establish_msi?;
        let cookie = establish_msi(
            ic.ic_cookie.get(),
            &mut addr,
            &mut data,
            level,
            ci,
            func,
            arg,
            name,
        );
        if cookie.is_null() {
            return None;
        }
        let disestablish = |cookie| {
            if let Some(dis) = ic.ic_disestablish {
                dis(cookie);
            }
        };

        let Some(aih) = malloc(size_of::<AcpipciIntrHandle>(), M_DEVBUF, M_WAITOK) else {
            disestablish(cookie);
            return None;
        };
        let aih = aih.cast::<AcpipciIntrHandle>();
        let dmat = ih.ih_dmat;

        let Ok(map) = bus_dmamap_create(dmat, 4, 1, 4, 0, BUS_DMA_WAITOK) else {
            free(aih.cast(), M_DEVBUF, size_of::<AcpipciIntrHandle>());
            disestablish(cookie);
            return None;
        };

        let seg = BusDmaSegment {
            ds_addr: addr as BusAddr,
            ds_len: size_of::<u32>(),
            ..BusDmaSegment::default()
        };

        // SAFETY: the "memory" is the MSI controller's doorbell, which outlives the map; the
        // load only translates its address for the device.
        if unsafe { bus_dmamap_load_raw(dmat, map, &[seg], size_of::<u32>(), BUS_DMA_WAITOK) }
            .is_err()
        {
            // SAFETY: the map just created, unloaded, not used afterwards.
            unsafe { bus_dmamap_destroy(dmat, NonNull::from(map)) };
            free(aih.cast(), M_DEVBUF, size_of::<AcpipciIntrHandle>());
            disestablish(cookie);
            return None;
        }

        // SAFETY: a fresh allocation of the handle's size, written once before use.
        unsafe {
            aih.write(AcpipciIntrHandle {
                aih_ih: MachineIntrHandle {
                    ih_ic: ic,
                    ih_ih: cookie,
                },
                aih_dmat: dmat,
                aih_map: map,
            });
        }

        let addr = map.dm_segs()[0].get().ds_addr;
        if ih.ih_type == PCI_MSIX {
            pci_msix_enable(
                ih.ih_pc,
                ih.ih_tag,
                sc.bus_memt(),
                ih.ih_intrpin,
                addr,
                data as u32,
            );
        } else {
            pci_msi_enable(ih.ih_pc, ih.ih_tag, addr, data as u32);
        }

        Some(aih.cast())
    } else {
        if let Some(ci) = ci
            && !<crate::machine::Machine as Cpu>::cpu_is_primary(ci)
        {
            return None;
        }
        acpi_intr_establish(ih.ih_intrpin, 0, level, func, arg, name)
    }
}

/// `acpipci_intr_disestablish(v, cookie)`.
///
/// # Safety
///
/// `cookie` came from `acpipci_intr_establish` and is not used afterwards.
pub unsafe fn acpipci_intr_disestablish(_v: *mut c_void, cookie: NonNull<c_void>) {
    // Both kinds of handle start with the controller's `struct machine_intr_handle`.
    // SAFETY: the caller's guarantee.
    let mih = unsafe { cookie.cast::<MachineIntrHandle>().as_ref() };
    // SAFETY: an established handle names a registered controller, never freed.
    let ic = unsafe { &*mih.ih_ic };

    if ic.ic_establish_msi.is_some() {
        let aih = cookie.cast::<AcpipciIntrHandle>();
        // SAFETY: an MSI handle `acpipci_intr_establish` wrote.
        let h = unsafe { aih.as_ref() };
        if let Some(dis) = ic.ic_disestablish {
            dis(h.aih_ih.ih_ih);
        }
        bus_dmamap_unload(h.aih_dmat, h.aih_map);
        // SAFETY: the doorbell's map, unloaded above and not used afterwards.
        unsafe { bus_dmamap_destroy(h.aih_dmat, NonNull::from(h.aih_map)) };
        free(aih.cast(), M_DEVBUF, size_of::<AcpipciIntrHandle>());
    } else {
        // SAFETY: an INTx handle from `acpi_intr_establish`, not used afterwards.
        unsafe { acpi_intr_disestablish(cookie) };
    }
}

/// The translation of `[addr, ...)` through a bus space's window list.
fn acpipci_trans_find(t: &BusSpace, addr: BusAddr) -> Option<&'static AcpipciTrans> {
    let mut at = t.bus_private.cast_const().cast::<AcpipciTrans>();
    // SAFETY: an acpipci bus space's `bus_private` is its translation list, leaked boxes
    // never freed.
    while let Some(t) = unsafe { at.as_ref() } {
        if addr >= t.at_base && addr < t.at_base + t.at_size {
            return Some(t);
        }
        at = t.at_next;
    }
    None
}

/// `acpipci_bs_map`: translate memory address if needed.
///
/// # Safety
///
/// As for `machine::bus::BusSpace::bus_space_map`.
pub unsafe fn acpipci_bs_map(
    t: &'static BusSpace,
    addr: BusAddr,
    size: BusSize,
    flags: u32,
) -> Result<BusSpaceHandle, Errno> {
    let Some(at) = acpipci_trans_find(t, addr) else {
        return Err(Errno::ENXIO);
    };
    // SAFETY: the caller's region, as the parent bus sees it.
    unsafe { bus_space_map(at.at_iot, addr + at.at_offset, size, flags) }
}

/// `acpipci_bs_mmap`: as `acpipci_bs_map`, for `mmap(2)`; `None` for the C's -1.
pub fn acpipci_bs_mmap(
    t: &'static BusSpace,
    addr: BusAddr,
    off: Off,
    prot: i32,
    flags: i32,
) -> Option<Paddr> {
    let at = acpipci_trans_find(t, addr)?;
    bus_space_mmap(at.at_iot, addr + at.at_offset, off, prot, flags)
}

/// `pci_mcfg_init(iot, addr, segment, min_bus, max_bus)`: maps the ECAM window of a segment
/// the MCFG table describes and makes its chipset (`acpimcfg`).
pub fn pci_mcfg_init(iot: BusSpaceTag, addr: BusAddr, segment: i32, min_bus: i32, max_bus: i32) {
    // SAFETY: the firmware's MCFG window, which only this chipset drives.
    let Ok(ioh) = (unsafe { bus_space_map(iot, addr, ((max_bus + 1) as usize) << 20, 0) }) else {
        panic(format_args!("pci_mcfg_init: can't map config space"));
    };

    let am: &'static mut MaybeUninit<AcpipciMcfg> = Box::leak(Box::new(MaybeUninit::uninit()));
    let cookie = am.as_mut_ptr().cast::<c_void>();
    let am: &'static AcpipciMcfg = am.write(AcpipciMcfg {
        am_list: SlistEntry::new(),
        am_segment: segment as u16,
        am_min_bus: min_bus as u8,
        am_max_bus: max_bus as u8,
        am_iot: iot,
        am_ioh: ioh,
        am_pc: MachinePciChipset {
            pc_conf_v: cookie,
            pc_attach_hook: acpipci_attach_hook,
            pc_bus_maxdevs: acpipci_bus_maxdevs,
            pc_make_tag: acpipci_make_tag,
            pc_decompose_tag: acpipci_decompose_tag,
            pc_conf_size: acpipci_conf_size,
            pc_conf_read: acpipci_conf_read,
            pc_conf_write: acpipci_conf_write,
            pc_probe_device_hook: acpipci_probe_device_hook,

            pc_intr_v: cookie,
            pc_intr_map: acpipci_intr_map,
            pc_intr_map_msi: _pci_intr_map_msi,
            pc_intr_map_msivec: _pci_intr_map_msivec,
            pc_intr_map_msix: _pci_intr_map_msix,
            pc_intr_string: acpipci_intr_string,
            pc_intr_establish: acpipci_intr_establish,
            pc_intr_disestablish: acpipci_intr_disestablish,
        },
        am_sc: Cell::new(ptr::null()),
    });
    // SAFETY: a new window, in no list, leaked for good.
    unsafe { ACPIPCI_MCFGS.0.insert_head(am) };
}

/// `acpipci_dummy_conf_read`: all ones.
pub fn acpipci_dummy_conf_read(_v: *mut c_void, _tag: Pcitag, _reg: i32) -> Pcireg {
    0xffff_ffff
}

/// `acpipci_dummy_conf_write`: nothing.
pub fn acpipci_dummy_conf_write(_v: *mut c_void, _tag: Pcitag, _reg: i32, _data: Pcireg) {}

/// `pci_lookup_segment(segment, bus)`: the chipset of the MCFG window covering `bus` of
/// `segment`, or `acpipci_dummy_chipset`.
pub fn pci_lookup_segment(segment: i32, bus: i32) -> PciChipsetTag {
    for am in ACPIPCI_MCFGS.0.iter() {
        if segment == i32::from(am.am_segment)
            && bus >= i32::from(am.am_min_bus)
            && bus <= i32::from(am.am_max_bus)
        {
            return &am.am_pc;
        }
    }

    &ACPIPCI_DUMMY_CHIPSET
}

// IORT support.

/// `acpipci_iort_map_node(iort, node, id, ic)`: follows the node's mapping of `id`.
pub fn acpipci_iort_map_node(
    iort: &[u8],
    offset: usize,
    id: u32,
    ic: &mut &'static InterruptController,
) -> u32 {
    let Some(node) = iort_read::<AcpiIortNode>(iort, offset) else {
        return id;
    };

    for i in 0..node.number_of_mappings as usize {
        let Some(map) = iort_mapping(iort, offset, &node, i) else {
            return id;
        };
        let out = map.output_reference as usize;

        if map.flags & ACPI_IORT_MAPPING_SINGLE != 0 {
            return acpipci_iort_map(iort, out, map.output_base, ic);
        }

        // Mapping encodes number of IDs in the range minus one.
        if map.input_base <= id && id <= map.input_base.wrapping_add(map.number_of_ids) {
            let id = map.output_base.wrapping_add(id - map.input_base);
            return acpipci_iort_map(iort, out, id, ic);
        }
    }

    id
}

/// `acpipci_iort_map(iort, offset, id, ic)`: `id` through the node at `offset`; an ITS
/// group picks the controller with its ID.
pub fn acpipci_iort_map(
    iort: &[u8],
    offset: usize,
    id: u32,
    ic: &mut &'static InterruptController,
) -> u32 {
    let Some(node) = iort_read::<AcpiIortNode>(iort, offset) else {
        return id;
    };

    match node.r#type {
        ACPI_IORT_ITS => {
            let itsn_at = offset + size_of::<AcpiIortNode>();
            let Some(itsn) = iort_read::<AcpiIortItsNode>(iort, itsn_at) else {
                return id;
            };
            for icl in interrupt_controllers().iter() {
                for i in 0..itsn.number_of_itss as usize {
                    let Some(its_id) =
                        iort_read::<u32>(iort, itsn_at + size_of::<AcpiIortItsNode>() + 4 * i)
                    else {
                        break;
                    };
                    if icl.ic_establish_msi.is_some() && icl.ic_gic_its_id.get() == its_id {
                        // SAFETY: a registered controller, never freed.
                        *ic = unsafe { &*ptr::from_ref(icl) };
                        break;
                    }
                }
            }

            id
        }
        ACPI_IORT_SMMU | ACPI_IORT_SMMU_V3 => acpipci_iort_map_node(iort, offset, id, ic),
        _ => id,
    }
}

/// `acpipci_iort_map_msi(pc, tag, ic)`: the MSI sideband data of a function: its requester
/// ID through the IORT mapping of the bridge's root complex.
pub fn acpipci_iort_map_msi(
    pc: PciChipsetTag,
    tag: Pcitag,
    ic: &mut &'static InterruptController,
) -> u32 {
    let rid = u32::from(pci_requester_id(pc, tag));
    let Some(sc) = softc_of(pc.pc_intr_v) else {
        return rid;
    };

    // Look for IORT table.
    let Some(iort) = sc.acpi().and_then(acpiiort_table) else {
        return rid;
    };
    let Some(hdr) = iort_read::<AcpiIort>(iort, 0) else {
        return rid;
    };

    // Find our root complex and map.
    let mut offset = hdr.offset as usize;
    for _ in 0..hdr.number_of_nodes {
        let Some(node) = iort_read::<AcpiIortNode>(iort, offset) else {
            return rid;
        };
        if node.r#type == ACPI_IORT_ROOT_COMPLEX
            && let Some(rc) = iort_read::<AcpiIortRcNode>(iort, offset + size_of::<AcpiIortNode>())
            && rc.segment == sc.sc_seg.get()
        {
            return acpipci_iort_map_node(iort, offset, rid, ic);
        }
        offset += usize::from(node.length);
    }

    rid
}
