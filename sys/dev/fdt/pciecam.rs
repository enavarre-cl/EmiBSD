/* $OpenBSD: pciecam.c,v 1.5 2024/02/03 10:37:26 kettenis Exp $ */
/* <LICENSES> */
/*
 * Copyright (c) 2013,2017 Patrick Wildt <patrick@blueri.se>
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
//! The generic PCIe host bridge with an ECAM configuration space (`pci-host-ecam-generic`,
//! QEMU `virt`'s): `dev/fdt/pciecam.c`. It maps the ECAM region, translates the bus's
//! memory and I/O addresses through the node's `ranges` with a bus space of its own, routes
//! INTx through the node's `interrupt-map` and MSI/MSI-X through its `msi-map` or
//! `msi-parent`, and attaches `pci*` below it.
//!
//! Upstream: sys/dev/fdt/pciecam.c @ 3ce1f3f79392
//!
//! The C is machine-independent but written against the machine's `struct
//! machine_pci_chipset`, `struct bus_space` and `struct machine_intr_handle`; `files.arm64`
//! lists it, so it is compiled where cfg `machine_pci_chipset` is set (arm64), and reaches
//! those items through `machine::pci_chipset`.
//!
//! ## Deviations
//! - `struct extent` is not ported (`sys/extent.h`), so `sc_ioex`/`sc_memex` and their names
//!   are left out and `pba_ioex`, `pba_memex`, `pba_pmemex` stay NULL (C_TO_RUST,
//!   "a member pointing into a subsystem that is not ported"): a BAR the firmware left
//!   unassigned cannot be placed, as on amd64.
//! - `iommu_device_map_pci` and `iommu_reserve_region_pci` (`ofw_misc.c`) are reported; the
//!   function keeps the bus's DMA tag, which is what the C gets when no IOMMU claims the
//!   requester ID (QEMU `virt` has no `iommu-map`).
//! - The softc's `sc_bus` and `sc_pc` are `MaybeUninit` behind an `UnsafeCell` (the softc is
//!   zeroed memory, which is no valid table of functions), written once by the attach before
//!   the bus attaches; the softc lives as long as the kernel (no detach), so the tags handed
//!   to the bus are `'static`.
//! - `pciecam_make_tag` builds the tag in 64 bits, where the C's `int` shift sign-extends a
//!   bus number of 128 or more; only `pciecam_decompose_tag` reads tags, and it masks the
//!   same bits either way.
//! - `pciecam_intr_map` fills `ih_dmat` with the function's DMA tag, which the C leaves
//!   unset for INTx (only MSI reads it).
//! - The `ic_barrier` of `pciecam_ic` is a function that calls `intr_barrier` on the
//!   cookie, since `intr_barrier` here takes a `NonNull`.
//! - `pciecam_bs_mmap` returns `None` for the C's `-1`.

use core::cell::{Cell, UnsafeCell};
use core::ffi::c_void;
use core::mem::MaybeUninit;
use core::ptr::{self, NonNull};
use core::slice;
use core::sync::atomic::Ordering;

use crate::dev::ofw::openfirm::{
    OF_getpropint, OF_getpropintarray, OF_getproplen, OF_is_compatible,
};
use crate::dev::pci::pci::{PCI_NDOMAINS, pci_requester_id};
use crate::dev::pci::pcireg::PCIE_CONFIG_SPACE_SIZE;
use crate::dev::pci::pcivar::{PCI_FLAGS_MSI_ENABLED, PciAttachArgs, PcibusAttachArgs, Pcireg};
use crate::kassert;
use crate::kern::kern_malloc::{free, malloc, mallocarray};
use crate::kern::subr_autoconf::config_found;
use crate::kern::subr_prf::{panic, printf};
use crate::machine::bus::{
    BUS_DMA_WAITOK, BusAddr, BusDmaSegment, BusDmaTag, BusDmamap, BusSize, BusSpaceHandle,
    BusSpaceTag, bus_dmamap_create, bus_dmamap_destroy, bus_dmamap_load_raw, bus_dmamap_unload,
    bus_space_map, bus_space_read_4, bus_space_write_4,
};
use crate::machine::cpu::CpuInfo;
use crate::machine::fdt::{
    FdtAttachArgs, fdt_intr_disestablish, fdt_intr_establish_imap_cpu, fdt_intr_establish_msi_cpu,
};
use crate::machine::intr::intr_barrier;
use crate::machine::pci_chipset::{
    _pci_intr_map_msi, _pci_intr_map_msivec, _pci_intr_map_msix, BusSpace, InterruptController,
    MachineIntrHandle, MachinePciChipset, PCI_INTX, PCI_MSI, PCI_MSIX, PCI_NONE, PciIntrHandle,
    bus_space_mmap, pci_msi_enable, pci_msix_enable,
};
use crate::machine::pci_machdep::{PciIntrFn, PciIntrStr, Pcitag};
use crate::sys::device::{CfMatch, Cfattach, Cfdriver, DV_DULL, Device, Softc};
use crate::sys::errno::Errno;
use crate::sys::malloc::{M_DEVBUF, M_TEMP, M_WAITOK};
use crate::sys::queue::ListEntry;
use crate::sys::types::{Off, Paddr};
use crate::unported;

// Assembling ECAM Configuration Address

/// `PCIE_BUS_SHIFT`.
const PCIE_BUS_SHIFT: u32 = 20;
/// `PCIE_SLOT_SHIFT`.
const PCIE_SLOT_SHIFT: u32 = 15;
/// `PCIE_FUNC_SHIFT`.
const PCIE_FUNC_SHIFT: u32 = 12;
/// `PCIE_BUS_MASK`.
const PCIE_BUS_MASK: u32 = 0xff;
/// `PCIE_SLOT_MASK`.
const PCIE_SLOT_MASK: u32 = 0x1f;
/// `PCIE_FUNC_MASK`.
const PCIE_FUNC_MASK: u32 = 0x7;
/// `PCIE_REG_MASK`.
const PCIE_REG_MASK: u32 = 0xfff;

/// `PCIE_ADDR_OFFSET(bus, slot, func, reg)`: the register's offset in the ECAM region.
const fn pcie_addr_offset(bus: i32, slot: i32, func: i32, reg: i32) -> usize {
    ((((bus as u32) & PCIE_BUS_MASK) << PCIE_BUS_SHIFT)
        | (((slot as u32) & PCIE_SLOT_MASK) << PCIE_SLOT_SHIFT)
        | (((func as u32) & PCIE_FUNC_MASK) << PCIE_FUNC_SHIFT)
        | ((reg as u32) & PCIE_REG_MASK)) as usize
}

/// `BUS_SHIFT`: the bus number's place in a tag.
const BUS_SHIFT: u32 = 24;
/// `DEVICE_SHIFT`.
const DEVICE_SHIFT: u32 = 19;
/// `FNC_SHIFT`.
const FNC_SHIFT: u32 = 16;

/// `struct pciecam_range`: one entry of the node's `ranges`.
#[derive(Clone, Copy, Default)]
pub struct PciecamRange {
    /// `flags`: the PCI address's `phys.hi` cell (space code in bits 24-25).
    pub flags: u32,
    /// `pci_base`: the address on the PCI bus.
    pub pci_base: u64,
    /// `phys_base`: the address on the parent bus.
    pub phys_base: u64,
    /// `size`.
    pub size: u64,
}

/// `struct pciecam_softc`.
#[repr(C)]
pub struct PciecamSoftc {
    /// `sc_dev`.
    pub sc_dev: Device,
    /// `sc_node`.
    pub sc_node: Cell<i32>,
    /// `sc_iot`: the parent's bus space.
    pub sc_iot: Cell<Option<BusSpaceTag>>,
    /// `sc_ioh`: the ECAM region.
    pub sc_ioh: Cell<Option<BusSpaceHandle>>,
    /// `sc_dmat`.
    pub sc_dmat: Cell<Option<BusDmaTag>>,
    /// `sc_dw_quirk`: a Synopsys DesignWare root port, with one device on bus 0.
    pub sc_dw_quirk: Cell<i32>,
    /// `sc_acells`: the node's `#address-cells` (3 for PCI).
    pub sc_acells: Cell<i32>,
    /// `sc_scells`: its `#size-cells`.
    pub sc_scells: Cell<i32>,
    /// `sc_pacells`: the parent's `#address-cells`.
    pub sc_pacells: Cell<i32>,
    /// `sc_pscells`: the parent's `#size-cells`.
    pub sc_pscells: Cell<i32>,
    /// `sc_bus`: the bus space the PCI devices get, a copy of `sc_iot` translating through
    /// `ranges`.
    pub sc_bus: UnsafeCell<MaybeUninit<BusSpace>>,
    /// `sc_pciranges`: `sc_pcirangeslen` entries (`mallocarray`, `M_TEMP`).
    pub sc_pciranges: Cell<*mut PciecamRange>,
    /// `sc_pcirangeslen`.
    pub sc_pcirangeslen: Cell<i32>,
    // sc_ioex, sc_memex, sc_ioex_name, sc_memex_name: sys/extent.h (see the deviations).
    /// `sc_pc`: the chipset the PCI bus dispatches through.
    pub sc_pc: UnsafeCell<MaybeUninit<MachinePciChipset>>,
}

impl PciecamSoftc {
    /// The ECAM region, mapped by the attach.
    fn regs(&self) -> (BusSpaceTag, BusSpaceHandle) {
        match (self.sc_iot.get(), self.sc_ioh.get()) {
            (Some(t), Some(h)) => (t, h),
            _ => panic(format_args!("pciecam: not attached")),
        }
    }

    /// `HREAD4(sc, reg)`.
    fn hread4(&self, reg: usize) -> u32 {
        let (t, h) = self.regs();
        bus_space_read_4(t, h, reg)
    }

    /// `HWRITE4(sc, reg, val)`.
    fn hwrite4(&self, reg: usize, val: u32) {
        let (t, h) = self.regs();
        bus_space_write_4(t, h, reg, val)
    }

    /// `sc->sc_pciranges[0 .. sc->sc_pcirangeslen]`.
    fn pciranges(&self) -> &[PciecamRange] {
        let p = self.sc_pciranges.get();
        if p.is_null() {
            return &[];
        }
        // SAFETY: the attach allocated and filled `sc_pcirangeslen` entries at
        // `sc_pciranges`, never freed.
        unsafe { slice::from_raw_parts(p, self.sc_pcirangeslen.get() as usize) }
    }

    /// `&sc->sc_bus`, the bus space the attach filled.
    fn bus(&'static self) -> &'static BusSpace {
        // SAFETY: written by `pciecam_attach` before any caller can reach the softc through
        // the bus; never written again.
        unsafe { (*self.sc_bus.get()).assume_init_ref() }
    }

    /// `&sc->sc_pc`, the chipset the attach filled.
    fn pc(&'static self) -> &'static MachinePciChipset {
        // SAFETY: as for `bus`.
        unsafe { (*self.sc_pc.get()).assume_init_ref() }
    }
}

// SAFETY: `#[repr(C)]` with the device first; every other member is a `Cell` of an integer,
// a raw pointer or an `Option` of a reference, or `MaybeUninit`: all valid as zero bits.
unsafe impl Softc for PciecamSoftc {}

/// `struct pciecam_intr_handle`: what `pciecam_intr_establish` returns. The machine's
/// `struct machine_intr_handle` comes first, so `intr_barrier` reaches `pciecam_ic`.
#[repr(C)]
pub struct PciecamIntrHandle {
    /// `pih_ih`.
    pub pih_ih: MachineIntrHandle,
    /// `pih_dmat`: the tag the MSI doorbell is mapped with (NULL for INTx).
    pub pih_dmat: Option<BusDmaTag>,
    /// `pih_map`: the doorbell's map.
    pub pih_map: Option<&'static BusDmamap>,
}

/// `pciecam_ic`, behind a wrapper that can be a `static`.
pub struct PciecamIc(InterruptController);

// SAFETY: `pciecam_ic` is never written: its cells keep their initial values and only its
// `ic_barrier` is ever read.
unsafe impl Sync for PciecamIc {}

/// `pciecam_ic`: only `ic_barrier`, for `intr_barrier` on a `pciecam_intr_handle`.
pub static PCIECAM_IC: PciecamIc = PciecamIc(InterruptController {
    ic_node: Cell::new(0),
    ic_cookie: Cell::new(ptr::null()),
    ic_establish: None,
    ic_establish_msi: None,
    ic_disestablish: None,
    ic_enable: None,
    ic_disable: None,
    ic_route: None,
    ic_cpu_enable: None,
    ic_barrier: Some(pciecam_intr_barrier),
    ic_set_wakeup: None,
    ic_list: ListEntry::new(),
    ic_phandle: Cell::new(0),
    ic_cells: Cell::new(0),
    ic_gic_its_id: Cell::new(0),
});

/// `pciecam_ca`.
pub static PCIECAM_CA: Cfattach = Cfattach {
    ca_devsize: size_of::<PciecamSoftc>(),
    ca_match: Some(pciecam_match),
    ca_attach: pciecam_attach,
    ca_detach: None,
    ca_activate: None,
};

/// `pciecam_cd`.
pub static PCIECAM_CD: Cfdriver = Cfdriver::new(b"pciecam", DV_DULL, 0);

/// `(struct pciecam_softc *)v`: the chipset's and the bus space's cookie.
fn softc_of(v: *mut c_void) -> &'static PciecamSoftc {
    // SAFETY: `pciecam_attach` makes the softc the cookie of its chipset and the private of
    // its bus space, and only their functions call this; the softc is never freed (no
    // detach).
    unsafe { &*v.cast::<PciecamSoftc>() }
}

/// `pciecam_intr_barrier`: `pciecam_ic`'s `ic_barrier`, the C's `intr_barrier` on the
/// cookie the parent controller gave.
fn pciecam_intr_barrier(cookie: *mut c_void) {
    if let Some(cookie) = NonNull::new(cookie) {
        intr_barrier(cookie);
    }
}

/// `pciecam_match`.
pub fn pciecam_match(_parent: Option<&Device>, _match: &CfMatch, aux: *mut c_void) -> i32 {
    // SAFETY: `pciecam` attaches at `fdt`, whose buses hand over a `FdtAttachArgs`.
    let faa = unsafe { &*aux.cast::<FdtAttachArgs<'_>>() };

    i32::from(
        OF_is_compatible(faa.fa_node, b"pci-host-ecam-generic")
            || OF_is_compatible(faa.fa_node, b"snps,dw-pcie-ecam"),
    )
}

/// `pciecam_attach`: reads the `ranges`, maps the ECAM region, fills the bus space and the
/// chipset, and attaches the PCI bus.
pub fn pciecam_attach(_parent: Option<&Device>, self_: &Device, aux: *mut c_void) {
    // SAFETY: `pciecam_ca` made the device, as a `PciecamSoftc`; it is never detached, so
    // the softc lives as long as the kernel.
    let sc: &'static PciecamSoftc = unsafe { &*ptr::from_ref(self_.softc::<PciecamSoftc>()) };
    // SAFETY: as in `pciecam_match`.
    let faa = unsafe { &*aux.cast::<FdtAttachArgs<'_>>() };

    sc.sc_node.set(faa.fa_node);
    sc.sc_iot.set(Some(faa.fa_iot));
    sc.sc_dmat.set(Some(faa.fa_dmat));

    if OF_is_compatible(faa.fa_node, b"snps,dw-pcie-ecam") {
        sc.sc_dw_quirk.set(1);
    }

    sc.sc_acells
        .set(OF_getpropint(sc.sc_node.get(), b"#address-cells", faa.fa_acells as u32) as i32);
    sc.sc_scells
        .set(OF_getpropint(sc.sc_node.get(), b"#size-cells", faa.fa_scells as u32) as i32);
    sc.sc_pacells.set(faa.fa_acells);
    sc.sc_pscells.set(faa.fa_scells);

    let acells = sc.sc_acells.get();
    let pacells = sc.sc_pacells.get();
    let scells = sc.sc_scells.get();
    let line = acells + pacells + scells;

    let rangeslen = OF_getproplen(sc.sc_node.get(), b"ranges");
    if rangeslen <= 0 || rangeslen % 4 != 0 || line <= 0 || (rangeslen / 4) % line != 0 {
        panic(format_args!("pciecam_attach: invalid ranges property"));
    }
    let rangeslen = rangeslen as usize;

    let Some(ranges) = malloc(rangeslen, M_TEMP, M_WAITOK) else {
        panic(format_args!("pciecam_attach: out of memory"));
    };
    let ranges = ranges.cast::<u32>();
    // SAFETY: a fresh allocation of `rangeslen` bytes, a whole number of cells; freed below.
    let cells = unsafe { slice::from_raw_parts_mut(ranges.as_ptr(), rangeslen / 4) };
    OF_getpropintarray(sc.sc_node.get(), b"ranges", cells);

    let nranges = (rangeslen / 4) / line as usize;
    let Some(pciranges) = mallocarray(nranges, size_of::<PciecamRange>(), M_TEMP, M_WAITOK) else {
        panic(format_args!("pciecam_attach: out of memory"));
    };
    let pciranges = pciranges.cast::<PciecamRange>();
    sc.sc_pcirangeslen.set(nranges as i32);

    let mut j = 0;
    for i in 0..nranges {
        let mut r = PciecamRange {
            flags: cells[j],
            ..PciecamRange::default()
        };
        j += 1;
        r.pci_base = u64::from(cells[j]);
        j += 1;
        if acells - 1 == 2 {
            r.pci_base <<= 32;
            r.pci_base |= u64::from(cells[j]);
            j += 1;
        }
        r.phys_base = u64::from(cells[j]);
        j += 1;
        if pacells == 2 {
            r.phys_base <<= 32;
            r.phys_base |= u64::from(cells[j]);
            j += 1;
        }
        r.size = u64::from(cells[j]);
        j += 1;
        if scells == 2 {
            r.size <<= 32;
            r.size |= u64::from(cells[j]);
            j += 1;
        }
        // SAFETY: `pciranges` has room for `nranges` entries.
        unsafe { pciranges.add(i).write(r) };
    }
    sc.sc_pciranges.set(pciranges.as_ptr());

    free(ranges.cast(), M_TEMP, rangeslen);

    let Some(reg) = faa.fa_reg.first() else {
        panic(format_args!("pciecam_attach: bus_space_map failed!"));
    };
    // SAFETY: the node's ECAM region, which only this bridge drives.
    let Ok(ioh) =
        (unsafe { bus_space_map(faa.fa_iot, reg.addr as BusAddr, reg.size as BusSize, 0) })
    else {
        panic(format_args!("pciecam_attach: bus_space_map failed!"));
    };
    sc.sc_ioh.set(Some(ioh));

    printf(format_args!("\n"));

    // Map PCIe address space: extent_create of "<dv_xname> pciio" and "<dv_xname> pcimem",
    // then extent_free of each I/O (space code 1) and memory range of `ranges` into them.
    // sys/extent.h is not ported (see the deviations): the bus gets no extents.

    // SAFETY: attach time, before the bus exists: nothing reads `sc_bus` yet; written once.
    unsafe {
        (*sc.sc_bus.get()).write(BusSpace {
            bus_private: ptr::from_ref(sc).cast_mut().cast::<c_void>(),
            _space_map: pciecam_bs_map,
            _space_mmap: pciecam_bs_mmap,
            ..*faa.fa_iot
        });
    }

    let cookie = ptr::from_ref(sc).cast_mut().cast::<c_void>();
    // SAFETY: as for `sc_bus`.
    unsafe {
        (*sc.sc_pc.get()).write(MachinePciChipset {
            pc_conf_v: cookie,
            pc_attach_hook: pciecam_attach_hook,
            pc_bus_maxdevs: pciecam_bus_maxdevs,
            pc_make_tag: pciecam_make_tag,
            pc_decompose_tag: pciecam_decompose_tag,
            pc_conf_size: pciecam_conf_size,
            pc_conf_read: pciecam_conf_read,
            pc_conf_write: pciecam_conf_write,
            pc_probe_device_hook: pciecam_probe_device_hook,

            pc_intr_v: cookie,
            pc_intr_map: pciecam_intr_map,
            pc_intr_map_msi: _pci_intr_map_msi,
            pc_intr_map_msivec: _pci_intr_map_msivec,
            pc_intr_map_msix: _pci_intr_map_msix,
            pc_intr_string: pciecam_intr_string,
            pc_intr_establish: pciecam_intr_establish,
            pc_intr_disestablish: pciecam_intr_disestablish,
        });
    }

    let mut pba = PcibusAttachArgs {
        pba_busname: b"pci",
        pba_iot: sc.bus(),
        pba_memt: sc.bus(),
        pba_dmat: faa.fa_dmat,
        pba_pc: sc.pc(),
        pba_flags: 0,
        // pba_ioex = sc_ioex, pba_memex = pba_pmemex = sc_memex: NULL (sys/extent.h).
        pba_domain: PCI_NDOMAINS.fetch_add(1, Ordering::Relaxed),
        pba_bus: 0,
        pba_bridgetag: None,
        pba_bridgeih: None,
        pba_intrswiz: 0,
        pba_intrtag: 0,
    };

    if OF_getproplen(sc.sc_node.get(), b"msi-map") > 0
        || OF_getproplen(sc.sc_node.get(), b"msi-parent") > 0
    {
        pba.pba_flags |= PCI_FLAGS_MSI_ENABLED;
    }

    let _ = config_found(self_, ptr::from_mut(&mut pba).cast(), None);
}

/// `pciecam_attach_hook`: nothing to do.
pub fn pciecam_attach_hook(_parent: &Device, _self: &Device, _pba: &PcibusAttachArgs) {}

/// `pciecam_bus_maxdevs`: a DesignWare root port has one device on bus 0.
pub fn pciecam_bus_maxdevs(v: *mut c_void, bus: i32) -> i32 {
    let sc = softc_of(v);

    if bus == 0 && sc.sc_dw_quirk.get() != 0 {
        return 1;
    }
    32
}

/// `pciecam_make_tag`.
pub fn pciecam_make_tag(_sc: *mut c_void, bus: i32, dev: i32, fnc: i32) -> Pcitag {
    ((bus as u64) << BUS_SHIFT) | ((dev as u64) << DEVICE_SHIFT) | ((fnc as u64) << FNC_SHIFT)
}

/// `pciecam_decompose_tag`: `(bus, device, function)`.
pub fn pciecam_decompose_tag(_sc: *mut c_void, tag: Pcitag) -> (i32, i32, i32) {
    (
        ((tag >> BUS_SHIFT) & 0xff) as i32,
        ((tag >> DEVICE_SHIFT) & 0x1f) as i32,
        ((tag >> FNC_SHIFT) & 0x7) as i32,
    )
}

/// `pciecam_conf_size`: the whole extended configuration space.
pub fn pciecam_conf_size(_sc: *mut c_void, _tag: Pcitag) -> i32 {
    PCIE_CONFIG_SPACE_SIZE
}

/// `pciecam_conf_read`.
pub fn pciecam_conf_read(v: *mut c_void, tag: Pcitag, reg: i32) -> Pcireg {
    let sc = softc_of(v);
    let (bus, dev, fn_) = pciecam_decompose_tag(v, tag);

    sc.hread4(pcie_addr_offset(bus, dev, fn_, reg & !0x3))
}

/// `pciecam_conf_write`.
pub fn pciecam_conf_write(v: *mut c_void, tag: Pcitag, reg: i32, data: Pcireg) {
    let sc = softc_of(v);
    let (bus, dev, fn_) = pciecam_decompose_tag(v, tag);

    sc.hwrite4(pcie_addr_offset(bus, dev, fn_, reg & !0x3), data);
}

/// `pciecam_probe_device_hook`: the function's DMA tag through the IOMMU its requester ID
/// maps to, with the bridge's windows reserved in it.
pub fn pciecam_probe_device_hook(v: *mut c_void, pa: &mut PciAttachArgs) -> i32 {
    let sc = softc_of(v);

    let _rid = pci_requester_id(pa.pa_pc, pa.pa_tag);
    // pa->pa_dmat = iommu_device_map_pci(sc->sc_node, rid, pa->pa_dmat): ofw_misc.c, which
    // hands back the tag unchanged when no IOMMU claims the requester ID.
    let _ = unported!("iommu_device_map_pci (ofw_misc.c)");

    for r in sc.pciranges() {
        if r.flags >> 24 == 0 {
            continue;
        }
        // iommu_reserve_region_pci(sc->sc_node, rid, r.pci_base, r.size): ofw_misc.c, a
        // no-op without an IOMMU.
        let _ = unported!("iommu_reserve_region_pci (ofw_misc.c)");
    }

    0
}

/// `pciecam_intr_map`: the function's INTx pin.
pub fn pciecam_intr_map(pa: &PciAttachArgs) -> Option<PciIntrHandle> {
    Some(PciIntrHandle {
        ih_pc: pa.pa_pc,
        ih_tag: pa.pa_intrtag,
        ih_intrpin: i32::from(pa.pa_intrpin),
        ih_type: PCI_INTX,
        ih_dmat: pa.pa_dmat,
    })
}

/// `pciecam_intr_string`.
pub fn pciecam_intr_string(_sc: *mut c_void, ih: PciIntrHandle) -> PciIntrStr {
    match ih.ih_type {
        PCI_MSI => PciIntrStr::new(format_args!("msi")),
        PCI_MSIX => PciIntrStr::new(format_args!("msix")),
        _ => PciIntrStr::new(format_args!("irq")),
    }
}

/// `pciecam_intr_establish`: an MSI or MSI-X through the node's MSI controller, the
/// doorbell mapped for the device and programmed into it; or the INTx line through the
/// node's `interrupt-map`.
#[allow(clippy::too_many_arguments)] // the C's signature
pub fn pciecam_intr_establish(
    self_: *mut c_void,
    ih: PciIntrHandle,
    level: i32,
    ci: Option<&'static CpuInfo>,
    func: PciIntrFn,
    arg: *mut c_void,
    name: &'static str,
) -> Option<NonNull<c_void>> {
    let sc = softc_of(self_);

    kassert!(ih.ih_type != PCI_NONE);

    let pih = if ih.ih_type != PCI_INTX {
        let mut addr: u64 = 0;

        // Assume hardware passes Requester ID as sideband data.
        let mut data = u64::from(pci_requester_id(ih.ih_pc, ih.ih_tag));
        let cookie = fdt_intr_establish_msi_cpu(
            sc.sc_node.get(),
            &mut addr,
            &mut data,
            level,
            ci,
            func,
            arg,
            name,
        )?;

        let Some(pih) = malloc(size_of::<PciecamIntrHandle>(), M_DEVBUF, M_WAITOK) else {
            // SAFETY: the cookie just established, not used afterwards.
            unsafe { fdt_intr_disestablish(cookie) };
            return None;
        };
        let pih = pih.cast::<PciecamIntrHandle>();
        let dmat = ih.ih_dmat;

        let map = match bus_dmamap_create(dmat, 4, 1, 4, 0, BUS_DMA_WAITOK) {
            Ok(map) => map,
            Err(_) => {
                free(pih.cast(), M_DEVBUF, size_of::<PciecamIntrHandle>());
                // SAFETY: as above.
                unsafe { fdt_intr_disestablish(cookie) };
                return None;
            }
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
            free(pih.cast(), M_DEVBUF, size_of::<PciecamIntrHandle>());
            // SAFETY: as above.
            unsafe { fdt_intr_disestablish(cookie) };
            return None;
        }

        // SAFETY: a fresh allocation of the handle's size, written once before use.
        unsafe {
            pih.write(PciecamIntrHandle {
                pih_ih: MachineIntrHandle {
                    ih_ic: &PCIECAM_IC.0,
                    ih_ih: cookie.as_ptr(),
                },
                pih_dmat: Some(dmat),
                pih_map: Some(map),
            });
        }

        let addr = map.dm_segs()[0].get().ds_addr;
        if ih.ih_type == PCI_MSIX {
            pci_msix_enable(
                ih.ih_pc,
                ih.ih_tag,
                sc.bus(),
                ih.ih_intrpin,
                addr,
                data as u32,
            );
        } else {
            pci_msi_enable(ih.ih_pc, ih.ih_tag, addr, data as u32);
        }
        pih
    } else {
        let (bus, dev, fn_) = pciecam_decompose_tag(self_, ih.ih_tag);

        let reg: [u32; 4] = [
            ((bus << 16) | (dev << 11) | (fn_ << 8)) as u32,
            0,
            0,
            ih.ih_intrpin as u32,
        ];

        let cookie =
            fdt_intr_establish_imap_cpu(sc.sc_node.get(), &reg, level, ci, func, arg, name)?;

        let Some(pih) = malloc(size_of::<PciecamIntrHandle>(), M_DEVBUF, M_WAITOK) else {
            // SAFETY: the cookie just established, not used afterwards.
            unsafe { fdt_intr_disestablish(cookie) };
            return None;
        };
        let pih = pih.cast::<PciecamIntrHandle>();
        // SAFETY: a fresh allocation of the handle's size, written once before use.
        unsafe {
            pih.write(PciecamIntrHandle {
                pih_ih: MachineIntrHandle {
                    ih_ic: &PCIECAM_IC.0,
                    ih_ih: cookie.as_ptr(),
                },
                pih_dmat: None,
                pih_map: None,
            });
        }
        pih
    };

    Some(pih.cast())
}

/// `pciecam_intr_disestablish`.
///
/// # Safety
///
/// `cookie` came from `pciecam_intr_establish` and is not used afterwards.
pub unsafe fn pciecam_intr_disestablish(_sc: *mut c_void, cookie: NonNull<c_void>) {
    let pih = cookie.cast::<PciecamIntrHandle>();
    // SAFETY: the caller's guarantee: a handle `pciecam_intr_establish` wrote.
    let h = unsafe { pih.as_ref() };

    if let Some(ih) = NonNull::new(h.pih_ih.ih_ih) {
        // SAFETY: the parent controller's cookie, not used after this.
        unsafe { fdt_intr_disestablish(ih) };
    }
    if let (Some(dmat), Some(map)) = (h.pih_dmat, h.pih_map) {
        bus_dmamap_unload(dmat, map);
        // SAFETY: the doorbell's map, unloaded above and not used afterwards.
        unsafe { bus_dmamap_destroy(dmat, NonNull::from(map)) };
    }
    free(pih.cast(), M_DEVBUF, size_of::<PciecamIntrHandle>());
}

/// `pciecam_bs_map`: translate memory address if needed.
///
/// # Safety
///
/// As for `machine::bus::BusSpace::bus_space_map`.
pub unsafe fn pciecam_bs_map(
    t: &'static BusSpace,
    bpa: BusAddr,
    size: BusSize,
    flag: u32,
) -> Result<BusSpaceHandle, Errno> {
    let sc = softc_of(t.bus_private);
    let Some(iot) = sc.sc_iot.get() else {
        return Err(Errno::ENXIO);
    };
    let bpa64 = bpa as u64;
    let size64 = size as u64;

    for r in sc.pciranges() {
        let physbase = r.phys_base;
        let pcibase = r.pci_base;
        let psize = r.size;

        if bpa64 >= pcibase && bpa64 + size64 <= pcibase + psize {
            // SAFETY: the caller's region, as the parent bus sees it.
            return unsafe {
                bus_space_map(iot, (bpa64 - pcibase + physbase) as BusAddr, size, flag)
            };
        }
    }

    Err(Errno::ENXIO)
}

/// `pciecam_bs_mmap`: as `pciecam_bs_map`, for `mmap(2)`.
pub fn pciecam_bs_mmap(
    t: &'static BusSpace,
    bpa: BusAddr,
    off: Off,
    prot: i32,
    flags: i32,
) -> Option<Paddr> {
    let sc = softc_of(t.bus_private);
    let iot = sc.sc_iot.get()?;
    let bpa64 = bpa as u64;

    for r in sc.pciranges() {
        let physbase = r.phys_base;
        let pcibase = r.pci_base;
        let psize = r.size;

        if bpa64 >= pcibase && bpa64 < pcibase + psize {
            return bus_space_mmap(
                iot,
                (bpa64 - pcibase + physbase) as BusAddr,
                off,
                prot,
                flags,
            );
        }
    }

    None
}
/* </CODE> */
