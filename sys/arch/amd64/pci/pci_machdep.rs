/*	$OpenBSD: pci_machdep.c,v 1.81 2025/01/23 11:24:34 kettenis Exp $	*/
/*	$NetBSD: pci_machdep.c,v 1.3 2003/05/07 21:33:58 fvdl Exp $	*/
/* <LICENSES> */
/*-
 * Copyright (c) 1997, 1998 The NetBSD Foundation, Inc.
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
 * Copyright (c) 1996 Christopher G. Demetriou.  All rights reserved.
 * Copyright (c) 1994 Charles M. Hannum.  All rights reserved.
 *
 * Redistribution and use in source and binary forms, with or without
 * modification, are permitted provided that the following conditions
 * are met:
 * 1. Redistributions of source code must retain the above copyright
 *    notice, this list of conditions and the following disclaimer.
 * 2. Redistributions in binary form must reproduce the above copyright
 *    notice, this list of conditions and the following disclaimer in the
 *    documentation and/or other materials provided with the distribution.
 * 3. All advertising materials mentioning features or use of this software
 *    must display the following acknowledgement:
 *	This product includes software developed by Charles M. Hannum.
 * 4. The name of the author may not be used to endorse or promote products
 *    derived from this software without specific prior written permission.
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
//! Machine-specific functions for PCI autoconfiguration: `arch/amd64/pci/pci_machdep.c`.
//!
//! Upstream: sys/arch/amd64/pci/pci_machdep.c @ 3ce1f3f79392
//!
//! Configuration space is reached with mechanism #1 (an address written to port `0xcf8`, the
//! data through `0xcfc`) under `pci_conf_lock`, or through the memory-mapped (ECAM) window
//! `pci_mcfg_init` records when ACPI's MCFG table names one, for the extended registers.
//! Interrupts are mapped from the line register the firmware wrote (the 8259), or, with MP
//! or ACPI tables, through an I/O APIC; MSI and MSI-X are programmed to the local APIC
//! directly by the `msi_pic` and `msix_pic` routing functions.
//!
//! ## Deviations
//! - Since M13 `mp_busses` is `acpimadt`'s (`mainbus.rs`'s globals) and `acpiprt` fills its
//!   buses, so the `NIOAPIC > 0` paths run as in C: `pci_intr_map`'s bus, bridge and ISA
//!   lookups, `pci_intr_establish_cpu`'s `acpiprt_route_interrupt` and `ioapic_find`; MSI and
//!   MSI-X (`msi_pic`, `msix_pic`, on the I/O APIC's edge stubs) are mapped when the bus has
//!   `PCI_FLAGS_MSI_ENABLED` (`acpipci`, or virtio's own). Without ACPI the NULL branches
//!   are taken, as the C takes them on a machine without tables.
//! - ECAM: `pci_mcfg_init` records the window; `pci_mcfg_map_bus` maps it through
//!   `bus_space_map` and panics as the C does when the map fails; no caller sets a window
//!   until ACPI's MCFG table is read (`acpimcfg`).
//! - `pci_init_extents` (extents and `bios_memmap`) is reported. `pci_probe_device_hook`
//!   calls acpidmar(4)'s `acpidmar_pci_hook` (M16e), which does nothing unless acpidmar0
//!   attached (GENERIC has it `disable`d). Since M13
//!   `pci_dev_postattach`, `pci_min_powerstate` and `pci_set_powerstate_md` take their
//!   `NACPI > 0` bodies (acpi(4)'s `acpi_pci_match`, `acpi_pci_min_powerstate`,
//!   `acpi_pci_set_powerstate`).
//! - `pci_intr_string` returns its text by value (the C's static buffer).
//! - `pci_conf_read`/`pci_conf_write` check the alignment of `reg` with `kassert!`, as the
//!   C's `KASSERT`.

use core::ffi::c_void;
use core::ptr::{self, NonNull};
use core::sync::atomic::{AtomicI32, AtomicUsize, Ordering};

use libkern::StaticCell;

use crate::arch::amd64::amd64::bus_dma::{
    _bus_dmamap_create, _bus_dmamap_destroy, _bus_dmamap_load, _bus_dmamap_load_mbuf,
    _bus_dmamap_load_raw, _bus_dmamap_load_uio, _bus_dmamap_sync, _bus_dmamap_unload,
    _bus_dmamem_alloc, _bus_dmamem_alloc_range, _bus_dmamem_free, _bus_dmamem_map,
    _bus_dmamem_mmap, _bus_dmamem_unmap,
};
use crate::arch::amd64::amd64::bus_space::{
    BusSpaceHandle, X86_BUS_SPACE_MEM, X86BusSpace, bus_space_barrier, bus_space_map,
    bus_space_read_4, bus_space_unmap, bus_space_write_4,
};
use crate::arch::amd64::amd64::i8259::I8259_PIC;
use crate::arch::amd64::amd64::intr::{intr_disestablish, intr_establish};
use crate::arch::amd64::amd64::ioapic::{ioapic_edge_stubs_table, ioapic_find};
use crate::arch::amd64::amd64::machdep::idt_vec_alloc_range;
use crate::arch::amd64::amd64::mainbus::{mp_busses, mp_eisa_bus, mp_isa_bus};
use crate::arch::amd64::include::bus::BusDmaTag;
use crate::arch::amd64::include::cpu::CpuInfo;
use crate::arch::amd64::include::i82093var::{
    APIC_INT_VIA_APIC, APIC_INT_VIA_MSG, APIC_INT_VIA_MSGX, apic_irq_apic, apic_irq_legacy_irq,
    apic_irq_pin,
};
use crate::arch::amd64::include::intr::{IntrFn, Intrhand};
use crate::arch::amd64::include::intrdefs::{IST_LEVEL, IST_PULSE, NUM_LEGACY_IRQS};
use crate::arch::amd64::include::pci_machdep::{
    PciChipsetTag, PciIntrHandle, Pcitag, X86_PCI_INTERRUPT_LINE_NO_CONNECTION, pci_intr_line,
};
use crate::arch::amd64::include::pic::{PIC_MSI, Pic};
use crate::arch::amd64::include::pio::{inl, outl};
use crate::dev::acpi::acpiprt::acpiprt_route_interrupt;
use crate::dev::pci::pci::pci_get_capability;
use crate::dev::pci::pci_map::{pci_mapreg_info, pci_mapreg_type};
use crate::dev::pci::pcireg::*;
use crate::dev::pci::pcivar::{PCI_FLAGS_MSI_ENABLED, PciAttachArgs, PcibusAttachArgs, Pcireg};
use crate::dev::pci::ppbreg::ppb_interrupt_swizzle;
use crate::kassert;
use crate::kern::kern_lock::{mtx_enter, mtx_leave};
use crate::kern::subr_prf::{panic, printf};
use crate::machine::bus::{BUS_SPACE_BARRIER_WRITE, BusAddr};
use crate::machine::intr::IPL_HIGH;
use crate::machine::pci_machdep::PciIntrStr;
use crate::sys::device::Device;
use crate::sys::errno::Errno;
use crate::sys::mutex::Mutex;
use crate::unported;

/// `PCI_MODE1_ENABLE`: configuration mechanism #1's enable bit.
pub const PCI_MODE1_ENABLE: u32 = 0x8000_0000;
/// `PCI_MODE1_ADDRESS_REG`.
pub const PCI_MODE1_ADDRESS_REG: u16 = 0x0cf8;
/// `PCI_MODE1_DATA_REG`.
pub const PCI_MODE1_DATA_REG: u16 = 0x0cfc;

/// `PCI_MSI_VEC_MASK`: we pack the MSI vector number into the lower 8 bits of the PCI tag
/// and use that as the MSI/MSI-X "PIC" pin number. This allows us to address 256 MSI
/// vectors which ought to be enough for anybody.
pub const PCI_MSI_VEC_MASK: u32 = 0xff;

/// `PCI_MSI_VEC(pin)`.
pub const fn pci_msi_vec(pin: i32) -> i32 {
    (pin as u32 & PCI_MSI_VEC_MASK) as i32
}

/// `PCI_MSI_TAG(pin)`.
pub const fn pci_msi_tag(pin: i32) -> Pcitag {
    pin as u32 & !PCI_MSI_VEC_MASK
}

/// `PCI_MSI_PIN(tag, vec)`.
pub const fn pci_msi_pin(tag: Pcitag, vec: i32) -> Pcitag {
    tag | vec as u32
}

/// `pci_mcfg_addr`: the memory mapped configuration space (ECAM) of segment 0, 0: none.
/// Since mapping the whole configuration space will cost us up to 256MB of kernel virtual
/// memory, we use separate mappings per bus, created on demand, so that we only use kernel
/// virtual memory for busses that are actually present.
pub static PCI_MCFG_ADDR: AtomicUsize = AtomicUsize::new(0);
/// `pci_mcfg_min_bus`.
pub static PCI_MCFG_MIN_BUS: AtomicI32 = AtomicI32::new(0);
/// `pci_mcfg_max_bus`.
pub static PCI_MCFG_MAX_BUS: AtomicI32 = AtomicI32::new(0);
/// `pci_mcfgt`: the space the window is in. Written by `pci_mcfg_init` while cold.
pub static PCI_MCFGT: StaticCell<X86BusSpace> = StaticCell::new(X86_BUS_SPACE_MEM);
/// `pci_mcfgh[256]`: each bus's mapping of the window. Written under `pci_mcfg_map_bus`'s
/// first use per bus, by autoconfiguration.
pub static PCI_MCFGH: StaticCell<[Option<BusSpaceHandle>; 256]> = StaticCell::new([None; 256]);

/// `pci_conf_lock`: serialises mechanism #1's address/data pair.
pub static PCI_CONF_LOCK: Mutex = Mutex::new(IPL_HIGH);

/// `pci_bus_dma_tag`: PCI doesn't have any special needs; just use the generic versions of
/// these functions.
pub static PCI_BUS_DMA_TAG: BusDmaTag = BusDmaTag {
    _cookie: ptr::null_mut(), // _may_bounce
    _dmamap_create: _bus_dmamap_create,
    _dmamap_destroy: _bus_dmamap_destroy,
    _dmamap_load: _bus_dmamap_load,
    _dmamap_load_mbuf: _bus_dmamap_load_mbuf,
    _dmamap_load_uio: _bus_dmamap_load_uio,
    _dmamap_load_raw: _bus_dmamap_load_raw,
    _dmamap_unload: _bus_dmamap_unload,
    _dmamap_sync: _bus_dmamap_sync,
    _dmamem_alloc: _bus_dmamem_alloc,
    _dmamem_alloc_range: _bus_dmamem_alloc_range,
    _dmamem_free: _bus_dmamem_free,
    _dmamem_map: _bus_dmamem_map,
    _dmamem_unmap: _bus_dmamem_unmap,
    _dmamem_mmap: _bus_dmamem_mmap,
};

/// `msi_pic`: MSI vectors, routed by programming the device's MSI capability.
pub static MSI_PIC: Pic = Pic {
    pic_name: "msi",
    pic_type: PIC_MSI,
    pic_hwmask: Some(msi_hwmask),
    pic_hwunmask: Some(msi_hwunmask),
    pic_addroute: Some(msi_addroute),
    pic_delroute: Some(msi_delroute),
    pic_allocidtvec: Some(msi_allocidtvec),
    pic_level_stubs: None,
    pic_edge_stubs: Some(ioapic_edge_stubs_table),
};

/// `msix_pic`: MSI-X vectors, routed by programming the device's MSI-X table.
pub static MSIX_PIC: Pic = Pic {
    pic_name: "msix",
    pic_type: PIC_MSI,
    pic_hwmask: Some(msix_hwmask),
    pic_hwunmask: Some(msix_hwunmask),
    pic_addroute: Some(msix_addroute),
    pic_delroute: Some(msix_delroute),
    pic_allocidtvec: None,
    pic_level_stubs: None,
    pic_edge_stubs: Some(ioapic_edge_stubs_table),
};

/// `mp_busses != NULL`: an MP bus table (`acpimadt`, or `mpbios`) is installed.
fn mp_busses_present() -> bool {
    mp_busses().is_some()
}

/// `pci_mcfg_init`: records the ECAM window of `segment` (ACPI's MCFG table).
pub fn pci_mcfg_init(iot: X86BusSpace, addr: BusAddr, segment: i32, min_bus: i32, max_bus: i32) {
    if segment == 0 {
        // SAFETY: called while cold, before any configuration space access uses the window.
        unsafe { PCI_MCFGT.write(iot) };
        PCI_MCFG_ADDR.store(addr, Ordering::Relaxed);
        PCI_MCFG_MIN_BUS.store(min_bus, Ordering::Relaxed);
        PCI_MCFG_MAX_BUS.store(max_bus, Ordering::Relaxed);
    }
}

/// `pci_lookup_segment`: the chipset tag of a PCI segment (only segment 0 exists).
pub fn pci_lookup_segment(segment: i32, _bus: i32) -> PciChipsetTag {
    kassert!(segment == 0);
    None
}

/// `pci_attach_hook`: nothing to do on amd64.
pub fn pci_attach_hook(_parent: &Device, _self: &Device, _pba: &PcibusAttachArgs) {}

/// `pci_bus_maxdevs`: 32 devices per bus.
pub fn pci_bus_maxdevs(_pc: PciChipsetTag, _busno: i32) -> i32 {
    32
}

/// `pci_make_tag`: mechanism #1's address of a function.
pub fn pci_make_tag(_pc: PciChipsetTag, bus: i32, device: i32, function: i32) -> Pcitag {
    if !(0..256).contains(&bus) || !(0..32).contains(&device) || !(0..8).contains(&function) {
        panic(format_args!("pci_make_tag: bad request"));
    }

    PCI_MODE1_ENABLE | ((bus as u32) << 16) | ((device as u32) << 11) | ((function as u32) << 8)
}

/// `pci_decompose_tag`: `(bus, device, function)`.
pub fn pci_decompose_tag(_pc: PciChipsetTag, tag: Pcitag) -> (i32, i32, i32) {
    (
        ((tag >> 16) & 0xff) as i32,
        ((tag >> 11) & 0x1f) as i32,
        ((tag >> 8) & 0x7) as i32,
    )
}

/// `pci_conf_size`: 4 KiB where ECAM reaches the bus, 256 bytes otherwise.
pub fn pci_conf_size(pc: PciChipsetTag, tag: Pcitag) -> i32 {
    if PCI_MCFG_ADDR.load(Ordering::Relaxed) != 0 {
        let (bus, _, _) = pci_decompose_tag(pc, tag);
        if mcfg_covers(bus) {
            return PCIE_CONFIG_SPACE_SIZE;
        }
    }

    PCI_CONFIG_SPACE_SIZE
}

/// Whether the ECAM window covers `bus`.
fn mcfg_covers(bus: i32) -> bool {
    bus >= PCI_MCFG_MIN_BUS.load(Ordering::Relaxed)
        && bus <= PCI_MCFG_MAX_BUS.load(Ordering::Relaxed)
}

/// `pci_mcfg_map_bus`: maps `bus`'s 1 MiB of the ECAM window, once.
pub fn pci_mcfg_map_bus(bus: i32) -> BusSpaceHandle {
    // SAFETY: the handles are written here only, by configuration space accesses, which
    // autoconfiguration and the drivers make one at a time.
    let handles = unsafe { PCI_MCFGH.get_mut() };
    if let Some(h) = handles[bus as usize] {
        return h;
    }

    let addr = PCI_MCFG_ADDR.load(Ordering::Relaxed) + ((bus as usize) << 20);
    // SAFETY: `pci_mcfg_init` recorded the window from the firmware's MCFG table, which
    // describes it as this segment's configuration space.
    let h = match unsafe { bus_space_map(PCI_MCFGT.read(), addr, 1 << 20, 0) } {
        Ok(h) => h,
        Err(_) => panic(format_args!("pci_conf_read: cannot map mcfg space")),
    };
    handles[bus as usize] = Some(h);
    h
}

/// `pci_conf_read`: reads the 32-bit configuration register `reg` of `tag`.
pub fn pci_conf_read(pc: PciChipsetTag, tag: Pcitag, reg: i32) -> Pcireg {
    kassert!(reg & 0x3 == 0);

    if PCI_MCFG_ADDR.load(Ordering::Relaxed) != 0 && reg >= PCI_CONFIG_SPACE_SIZE {
        let (bus, _, _) = pci_decompose_tag(pc, tag);
        if mcfg_covers(bus) {
            let h = pci_mcfg_map_bus(bus);
            // SAFETY: as in pci_mcfg_map_bus.
            let t = unsafe { PCI_MCFGT.read() };
            return bus_space_read_4(t, h, (((tag & 0x0000_ff00) << 4) | reg as u32) as usize);
        }
    }

    mtx_enter(&PCI_CONF_LOCK);
    // SAFETY: ports 0xcf8/0xcfc are the PC's configuration mechanism #1, used only here
    // and under pci_conf_lock.
    let data = unsafe {
        outl(PCI_MODE1_ADDRESS_REG, tag | reg as u32);
        let data = inl(PCI_MODE1_DATA_REG);
        outl(PCI_MODE1_ADDRESS_REG, 0);
        data
    };
    mtx_leave(&PCI_CONF_LOCK);

    data
}

/// `pci_conf_write`: writes the 32-bit configuration register `reg` of `tag`.
pub fn pci_conf_write(pc: PciChipsetTag, tag: Pcitag, reg: i32, data: Pcireg) {
    kassert!(reg & 0x3 == 0);

    if PCI_MCFG_ADDR.load(Ordering::Relaxed) != 0 && reg >= PCI_CONFIG_SPACE_SIZE {
        let (bus, _, _) = pci_decompose_tag(pc, tag);
        if mcfg_covers(bus) {
            let h = pci_mcfg_map_bus(bus);
            // SAFETY: as in pci_mcfg_map_bus.
            let t = unsafe { PCI_MCFGT.read() };
            bus_space_write_4(
                t,
                h,
                (((tag & 0x0000_ff00) << 4) | reg as u32) as usize,
                data,
            );
            return;
        }
    }

    mtx_enter(&PCI_CONF_LOCK);
    // SAFETY: as in pci_conf_read.
    unsafe {
        outl(PCI_MODE1_ADDRESS_REG, tag | reg as u32);
        outl(PCI_MODE1_DATA_REG, data);
        outl(PCI_MODE1_ADDRESS_REG, 0);
    }
    mtx_leave(&PCI_CONF_LOCK);
}

/// `pci_msix_table_map`: maps the MSI-X table of `tag` (through the BAR its capability
/// names).
pub fn pci_msix_table_map(
    pc: PciChipsetTag,
    tag: Pcitag,
    memt: X86BusSpace,
) -> Result<BusSpaceHandle, Errno> {
    let Some((off, reg)) = pci_get_capability(pc, tag, PCI_CAP_MSIX) else {
        panic(format_args!("pci_msix_table_map: no msix capability"));
    };

    let table = pci_conf_read(pc, tag, off + PCI_MSIX_TABLE);
    let bir = table & PCI_MSIX_TABLE_BIR;
    let offset = (table & PCI_MSIX_TABLE_OFF) as usize;
    let tblsz = pci_msix_mc_tblsz(reg) as usize + 1;

    let bir = PCI_MAPREG_START + bir as i32 * 4;
    let type_ = pci_mapreg_type(pc, tag, bir);
    let (base, _, _) = pci_mapreg_info(pc, tag, bir, type_)?;
    // _bus_space_map: without extent checking, which bus_space_map does not do yet either.
    // SAFETY: the table lives in the device's own BAR, as its MSI-X capability says.
    unsafe { bus_space_map(memt, base + offset, tblsz * 16, 0) }
}

/// `pci_msix_table_unmap`.
pub fn pci_msix_table_unmap(
    pc: PciChipsetTag,
    tag: Pcitag,
    memt: X86BusSpace,
    memh: BusSpaceHandle,
) {
    let Some((_, reg)) = pci_get_capability(pc, tag, PCI_CAP_MSIX) else {
        panic(format_args!("pci_msix_table_unmap: no msix capability"));
    };

    let tblsz = pci_msix_mc_tblsz(reg) as usize + 1;
    bus_space_unmap(memt, memh, tblsz * 16);
}

/// `msi_hwmask`: masks MSI vector `pin` with the per-vector mask bits, if implemented.
pub fn msi_hwmask(_pic: &Pic, pin: i32) {
    let pc: PciChipsetTag = None; // XXX
    let tag = pci_msi_tag(pin);
    let vec = pci_msi_vec(pin);

    // The C looks up the MSI-X capability here, then reads the MSI layout through it.
    let Some((off, reg)) = pci_get_capability(pc, tag, PCI_CAP_MSIX) else {
        return;
    };

    // We can't mask if per-vector masking isn't implemented.
    if reg & PCI_MSI_MC_PVMASK == 0 {
        return;
    }

    let maskreg = if reg & PCI_MSI_MC_C64 != 0 {
        PCI_MSI_MASK64
    } else {
        PCI_MSI_MASK32
    };
    let mask = pci_conf_read(pc, tag, off + maskreg);
    pci_conf_write(pc, tag, off + maskreg, mask | (1u32 << vec));
}

/// `msi_hwunmask`.
pub fn msi_hwunmask(_pic: &Pic, pin: i32) {
    let pc: PciChipsetTag = None; // XXX
    let tag = pci_msi_tag(pin);
    let vec = pci_msi_vec(pin);

    // The C looks up the MSI-X capability here too (see msi_hwmask).
    let Some((off, reg)) = pci_get_capability(pc, tag, PCI_CAP_MSIX) else {
        return;
    };

    // We can't mask if per-vector masking isn't implemented.
    if reg & PCI_MSI_MC_PVMASK == 0 {
        return;
    }

    let maskreg = if reg & PCI_MSI_MC_C64 != 0 {
        PCI_MSI_MASK64
    } else {
        PCI_MSI_MASK32
    };
    let mask = pci_conf_read(pc, tag, off + maskreg);
    pci_conf_write(pc, tag, off + maskreg, mask & !(1u32 << vec));
}

/// `msi_addroute`: points the device's MSI at `idtvec` on `ci`'s local APIC and enables it.
pub fn msi_addroute(_pic: &Pic, ci: &CpuInfo, pin: i32, idtvec: i32, _type: i32) {
    let pc: PciChipsetTag = None; // XXX
    let tag = pci_msi_tag(pin);
    let vec = pci_msi_vec(pin);

    let Some((off, reg)) = pci_get_capability(pc, tag, PCI_CAP_MSI) else {
        panic(format_args!("msi_addroute: no msi capability"));
    };

    if vec != 0 {
        return;
    }

    let addr: Pcireg = 0xfee0_0000 | (ci.ci_apicid.get() << 12);

    if reg & PCI_MSI_MC_C64 != 0 {
        pci_conf_write(pc, tag, off + PCI_MSI_MA, addr);
        pci_conf_write(pc, tag, off + PCI_MSI_MAU32, 0);
        pci_conf_write(pc, tag, off + PCI_MSI_MD64, idtvec as u32);
    } else {
        pci_conf_write(pc, tag, off + PCI_MSI_MA, addr);
        pci_conf_write(pc, tag, off + PCI_MSI_MD32, idtvec as u32);
    }
    pci_conf_write(pc, tag, off, reg | PCI_MSI_MC_MSIE);
}

/// `msi_delroute`: disables the device's MSI.
pub fn msi_delroute(_pic: &Pic, _ci: &CpuInfo, pin: i32, _idtvec: i32, _type: i32) {
    let pc: PciChipsetTag = None; // XXX
    let tag = pci_msi_tag(pin);
    let vec = pci_msi_vec(pin);

    if vec != 0 {
        return;
    }

    if let Some((off, reg)) = pci_get_capability(pc, tag, PCI_CAP_MSI) {
        pci_conf_write(pc, tag, off, reg & !PCI_MSI_MC_MSIE);
    }
}

/// `msi_allocidtvec`: allocates the block of IDT vectors a multi-message MSI needs (vector
/// 0 allocates, the others follow it).
pub fn msi_allocidtvec(_pic: &Pic, pin: i32, low: i32, high: i32) -> i32 {
    let pc: PciChipsetTag = None; // XXX
    let tag = pci_msi_tag(pin);
    let vec = pci_msi_vec(pin);

    let Some((off, _)) = pci_get_capability(pc, tag, PCI_CAP_MSI) else {
        panic(format_args!("msi_allocidtvec: no msi capability"));
    };

    let reg = pci_conf_read(pc, tag, off);
    let mme = ((reg & PCI_MSI_MC_MME_MASK) >> PCI_MSI_MC_MME_SHIFT) as i32;
    if vec >= (1 << mme) {
        return 0;
    }

    let mdreg = if reg & PCI_MSI_MC_C64 != 0 {
        PCI_MSI_MD64
    } else {
        PCI_MSI_MD32
    };
    if vec == 0 {
        let idtvec = idt_vec_alloc_range(low, high, 1 << mme);
        pci_conf_write(pc, tag, off + mdreg, idtvec as u32);
        idtvec
    } else {
        let reg = pci_conf_read(pc, tag, off + mdreg);
        kassert!(reg > 0);
        reg as i32 + vec
    }
}

/// `pci_intr_enable_msivec`: enables enough MSI vectors for `num_vec`; `true` when MSI
/// cannot be used or the device has too few.
pub fn pci_intr_enable_msivec(pa: &PciAttachArgs, num_vec: i32) -> bool {
    let pc = pa.pa_pc;
    let tag = pa.pa_tag;

    if pa.pa_flags & PCI_FLAGS_MSI_ENABLED == 0 || !mp_busses_present() {
        return true;
    }
    let Some((off, mut reg)) = pci_get_capability(pc, tag, PCI_CAP_MSI) else {
        return true;
    };

    let mmc = ((reg & PCI_MSI_MC_MMC_MASK) >> PCI_MSI_MC_MMC_SHIFT) as i32;
    if num_vec > (1 << mmc) {
        return true;
    }

    let mut mme = ((reg & PCI_MSI_MC_MME_MASK) >> PCI_MSI_MC_MME_SHIFT) as i32;
    while (1 << mme) < num_vec {
        mme += 1;
    }
    reg &= !PCI_MSI_MC_MME_MASK;
    reg |= (mme as u32) << PCI_MSI_MC_MME_SHIFT;
    pci_conf_write(pc, tag, off, reg);

    false
}

/// `pci_intr_map_msi`: the device's single MSI vector.
pub fn pci_intr_map_msi(pa: &PciAttachArgs) -> Option<PciIntrHandle> {
    let pc = pa.pa_pc;
    let tag = pa.pa_tag;

    if pa.pa_flags & PCI_FLAGS_MSI_ENABLED == 0 || !mp_busses_present() {
        return None;
    }
    let (off, mut reg) = pci_get_capability(pc, tag, PCI_CAP_MSI)?;

    // Make sure we only enable one MSI vector.
    reg &= !PCI_MSI_MC_MME_MASK;
    pci_conf_write(pc, tag, off, reg);

    Some(PciIntrHandle {
        tag,
        line: APIC_INT_VIA_MSG,
        pin: 0,
    })
}

/// `pci_intr_map_msivec`: MSI vector `vec` (enabled by `pci_intr_enable_msivec`).
pub fn pci_intr_map_msivec(pa: &PciAttachArgs, vec: i32) -> Option<PciIntrHandle> {
    let pc = pa.pa_pc;
    let tag = pa.pa_tag;

    if pa.pa_flags & PCI_FLAGS_MSI_ENABLED == 0 || !mp_busses_present() {
        return None;
    }
    let (_, reg) = pci_get_capability(pc, tag, PCI_CAP_MSI)?;

    let mme = ((reg & PCI_MSI_MC_MME_MASK) >> PCI_MSI_MC_MME_SHIFT) as i32;
    if vec >= (1 << mme) {
        return None;
    }

    Some(PciIntrHandle {
        tag: pci_msi_pin(tag, vec),
        line: APIC_INT_VIA_MSG,
        pin: 0,
    })
}

/// `msix_hwmask`: sets the mask bit of MSI-X table entry `pin`.
pub fn msix_hwmask(_pic: &Pic, pin: i32) {
    let pc: PciChipsetTag = None; // XXX
    let memt = X86_BUS_SPACE_MEM; // XXX
    let tag = pci_msi_tag(pin);
    let entry = pci_msi_vec(pin) as usize;

    let Some((_, reg)) = pci_get_capability(pc, tag, PCI_CAP_MSIX) else {
        return;
    };

    kassert!(entry <= pci_msix_mc_tblsz(reg) as usize);

    let Ok(memh) = pci_msix_table_map(pc, tag, memt) else {
        panic(format_args!("msix_hwmask: cannot map registers"));
    };

    let ctrl = bus_space_read_4(memt, memh, pci_msix_vc(entry));
    bus_space_write_4(memt, memh, pci_msix_vc(entry), ctrl | PCI_MSIX_VC_MASK);

    pci_msix_table_unmap(pc, tag, memt, memh);
}

/// `msix_hwunmask`.
pub fn msix_hwunmask(_pic: &Pic, pin: i32) {
    let pc: PciChipsetTag = None; // XXX
    let memt = X86_BUS_SPACE_MEM; // XXX
    let tag = pci_msi_tag(pin);
    let entry = pci_msi_vec(pin) as usize;

    if pci_get_capability(pc, tag, PCI_CAP_MSIX).is_none() {
        return;
    }

    let Ok(memh) = pci_msix_table_map(pc, tag, memt) else {
        panic(format_args!("msix_hwunmask: cannot map registers"));
    };

    let ctrl = bus_space_read_4(memt, memh, pci_msix_vc(entry));
    bus_space_write_4(memt, memh, pci_msix_vc(entry), ctrl & !PCI_MSIX_VC_MASK);

    pci_msix_table_unmap(pc, tag, memt, memh);
}

/// `msix_addroute`: points MSI-X table entry `pin` at `idtvec` on `ci`'s local APIC,
/// unmasks it and enables MSI-X.
pub fn msix_addroute(_pic: &Pic, ci: &CpuInfo, pin: i32, idtvec: i32, _type: i32) {
    let pc: PciChipsetTag = None; // XXX
    let memt = X86_BUS_SPACE_MEM; // XXX
    let tag = pci_msi_tag(pin);
    let entry = pci_msi_vec(pin) as usize;

    let Some((off, reg)) = pci_get_capability(pc, tag, PCI_CAP_MSIX) else {
        panic(format_args!("msix_addroute: no msix capability"));
    };

    kassert!(entry <= pci_msix_mc_tblsz(reg) as usize);

    let Ok(memh) = pci_msix_table_map(pc, tag, memt) else {
        panic(format_args!("msix_addroute: cannot map registers"));
    };

    let addr: Pcireg = 0xfee0_0000 | (ci.ci_apicid.get() << 12);

    bus_space_write_4(memt, memh, pci_msix_ma(entry), addr);
    bus_space_write_4(memt, memh, pci_msix_mau32(entry), 0);
    bus_space_write_4(memt, memh, pci_msix_md(entry), idtvec as u32);
    bus_space_barrier(memt, memh, pci_msix_ma(entry), 16, BUS_SPACE_BARRIER_WRITE);
    let ctrl = bus_space_read_4(memt, memh, pci_msix_vc(entry));
    bus_space_write_4(memt, memh, pci_msix_vc(entry), ctrl & !PCI_MSIX_VC_MASK);

    pci_msix_table_unmap(pc, tag, memt, memh);

    pci_conf_write(pc, tag, off, reg | PCI_MSIX_MC_MSIXE);
}

/// `msix_delroute`: masks MSI-X table entry `pin`.
pub fn msix_delroute(_pic: &Pic, _ci: &CpuInfo, pin: i32, _idtvec: i32, _type: i32) {
    let pc: PciChipsetTag = None; // XXX
    let memt = X86_BUS_SPACE_MEM; // XXX
    let tag = pci_msi_tag(pin);
    let entry = pci_msi_vec(pin) as usize;

    let Some((_, reg)) = pci_get_capability(pc, tag, PCI_CAP_MSIX) else {
        return;
    };

    kassert!(entry <= pci_msix_mc_tblsz(reg) as usize);

    let Ok(memh) = pci_msix_table_map(pc, tag, memt) else {
        return;
    };

    let ctrl = bus_space_read_4(memt, memh, pci_msix_vc(entry));
    bus_space_write_4(memt, memh, pci_msix_vc(entry), ctrl | PCI_MSIX_VC_MASK);

    pci_msix_table_unmap(pc, tag, memt, memh);
}

/// `pci_intr_map_msix`: MSI-X table entry `vec`.
pub fn pci_intr_map_msix(pa: &PciAttachArgs, vec: i32) -> Option<PciIntrHandle> {
    let pc = pa.pa_pc;
    let tag = pa.pa_tag;

    kassert!(pci_msi_vec(vec) == vec);

    if pa.pa_flags & PCI_FLAGS_MSI_ENABLED == 0 || !mp_busses_present() {
        return None;
    }
    let (_, reg) = pci_get_capability(pc, tag, PCI_CAP_MSIX)?;

    if vec as u32 > pci_msix_mc_tblsz(reg) {
        return None;
    }

    Some(PciIntrHandle {
        tag: pci_msi_pin(tag, vec),
        line: APIC_INT_VIA_MSGX,
        pin: 0,
    })
}

/// `pci_intr_map`: the device's INTx interrupt: the line the firmware wrote, routed
/// through the MP tables when there are any.
pub fn pci_intr_map(pa: &PciAttachArgs) -> Option<PciIntrHandle> {
    let pin = i32::from(pa.pa_rawintrpin);
    let line = i32::from(pa.pa_intrline);

    if pin == 0 {
        // No IRQ used.
        return None;
    }

    if pin > PCI_INTERRUPT_PIN_MAX as i32 {
        printf(format_args!("pci_intr_map: bad interrupt pin {pin}\n"));
        return None;
    }

    let mut ih = PciIntrHandle {
        tag: pa.pa_tag,
        line,
        pin,
    };

    // NIOAPIC > 0
    let (bus, dev, func) = pci_decompose_tag(pa.pa_pc, pa.pa_tag);

    if let Some(busses) = mp_busses() {
        let mpspec_pin = (dev << 2) | (pin - 1);

        if let Some(mb) = busses.get(bus as usize) {
            let is_isa = mp_isa_bus().is_some_and(|b| ptr::eq(b, mb));
            let is_eisa = mp_eisa_bus().is_some_and(|b| ptr::eq(b, mb));
            if !is_isa
                && !is_eisa
                && let Some(mip) = mb.intrs().find(|mip| mip.bus_pin == mpspec_pin)
            {
                ih.line = mip.ioapic_ih | line;
                return Some(ih);
            }
        }

        if pa.pa_bridgetag.is_some() {
            let swizpin = ppb_interrupt_swizzle(pin, dev);
            if let Some(bih) = pa
                .pa_bridgeih
                .and_then(|b| b.get((swizpin - 1) as usize))
                .and_then(Option::as_ref)
                .filter(|bih| bih.line != -1)
            {
                ih.line = bih.line | line;
                return Some(ih);
            }
        }
        // No explicit PCI mapping found. This is not fatal, we'll try the ISA (or possibly
        // EISA) mappings next.
    }

    // Section 6.2.4, `Miscellaneous Functions', says that 255 means `unknown' or `no
    // connection' on a PC. We assume that a device with `no connection' either doesn't have
    // an interrupt (in which case the pin number should be 0, and would have been noticed
    // above), or wasn't configured by the BIOS (in which case we punt, since there's no
    // real way we can know how the interrupt lines are mapped in the hardware).
    //
    // XXX Since IRQ 0 is only used by the clock, and we can't actually be sure that the
    // BIOS did its job, we also recognize that as meaning that the BIOS has not configured
    // the device.
    if line == 0 || line == X86_PCI_INTERRUPT_LINE_NO_CONNECTION {
        return None;
    }

    if line >= NUM_LEGACY_IRQS as i32 {
        printf(format_args!("pci_intr_map: bad interrupt line {line}\n"));
        return None;
    }
    let mut line = line;
    if line == 2 {
        printf(format_args!("pci_intr_map: changed line 2 to line 9\n"));
        // Only the I/O APIC lookups below read the local `line`; the handle keeps the line
        // it was filled with, as in C.
        line = 9;
    }

    // NIOAPIC > 0
    if mp_busses_present() {
        let isa = mp_isa_bus().and_then(|b| b.intrs().find(|mip| mip.bus_pin == line));
        // NEISA > 0: not configured (mp_eisa_bus stays NULL with ACPI).
        if let Some(mip) = isa {
            ih.line = mip.ioapic_ih | line;
            return Some(ih);
        }
        printf(format_args!(
            "pci_intr_map: bus {bus} dev {dev} func {func} pin {pin}; line {line}\n"
        ));
        printf(format_args!("pci_intr_map: no MP mapping found\n"));
    }

    Some(ih)
}

/// `pci_intr_string`: "msi", "msix", "apic A int P" or "irq N".
pub fn pci_intr_string(_pc: PciChipsetTag, ih: PciIntrHandle) -> PciIntrStr {
    if ih.line == 0 {
        panic(format_args!(
            "pci_intr_string: bogus handle 0x{:x}",
            ih.line
        ));
    }

    if ih.line & APIC_INT_VIA_MSG != 0 {
        return PciIntrStr::new(format_args!("msi"));
    }
    if ih.line & APIC_INT_VIA_MSGX != 0 {
        return PciIntrStr::new(format_args!("msix"));
    }

    // NIOAPIC > 0
    if ih.line & APIC_INT_VIA_APIC != 0 {
        PciIntrStr::new(format_args!(
            "apic {} int {}",
            apic_irq_apic(ih.line),
            apic_irq_pin(ih.line)
        ))
    } else {
        PciIntrStr::new(format_args!("irq {}", pci_intr_line(ih)))
    }
}

/// `pci_intr_establish`: as `pci_intr_establish_cpu` on any CPU.
pub fn pci_intr_establish(
    pc: PciChipsetTag,
    ih: PciIntrHandle,
    level: i32,
    func: IntrFn,
    arg: *mut c_void,
    what: &'static str,
) -> Option<NonNull<Intrhand>> {
    pci_intr_establish_cpu(pc, ih, level, None, func, arg, what)
}

/// `pci_intr_establish_cpu`: registers `func(arg)` for the interrupt `ih` names.
#[allow(clippy::too_many_arguments)] // the C's signature
pub fn pci_intr_establish_cpu(
    pc: PciChipsetTag,
    ih: PciIntrHandle,
    level: i32,
    ci: Option<&'static CpuInfo>,
    func: IntrFn,
    arg: *mut c_void,
    what: &'static str,
) -> Option<NonNull<Intrhand>> {
    let tag = ih.tag;

    if ih.line & (APIC_INT_VIA_MSG | APIC_INT_VIA_MSGX) != 0 {
        let pic = if ih.line & APIC_INT_VIA_MSG != 0 {
            &MSI_PIC
        } else {
            &MSIX_PIC
        };
        return intr_establish(-1, pic, tag as i32, IST_PULSE, level, ci, func, arg, what);
    }

    let (bus, dev, _) = pci_decompose_tag(pc, ih.tag);
    // NACPIPRT > 0
    acpiprt_route_interrupt(bus, dev, ih.pin);

    let mut pic: &'static Pic = &I8259_PIC;
    let mut pin = ih.line;
    let mut irq = ih.line;

    // NIOAPIC > 0
    if ih.line & APIC_INT_VIA_APIC != 0 {
        let Some(apic) = ioapic_find(apic_irq_apic(ih.line)) else {
            printf(format_args!(
                "pci_intr_establish: bad ioapic {}\n",
                apic_irq_apic(ih.line)
            ));
            return None;
        };
        // SAFETY: an attached I/O APIC's pic is initialised before it is in `ioapics`.
        pic = unsafe { apic.pic() };
        pin = apic_irq_pin(ih.line);
        irq = apic_irq_legacy_irq(ih.line);
        if !(0..NUM_LEGACY_IRQS as i32).contains(&irq) {
            irq = -1;
        }
    }

    intr_establish(irq, pic, pin, IST_LEVEL, level, ci, func, arg, what)
}

/// `pci_intr_disestablish`.
///
/// # Safety
///
/// `cookie` came from `pci_intr_establish*` and is not used afterwards.
pub unsafe fn pci_intr_disestablish(_pc: PciChipsetTag, cookie: NonNull<Intrhand>) {
    // SAFETY: the caller's guarantee is intr_disestablish's.
    unsafe { intr_disestablish(cookie) }
}

/// `pci_init_extents`: the extents of the PCI I/O space, memory space (without the BIOS
/// memory map's RAM and the ISA hole) and bus numbers (see the module's deviations).
pub fn pci_init_extents() {
    let _ = unported!("pci_init_extents (subr_extent.c, bios_memmap)");
}

/// `pci_probe_device_hook`: lets the IOMMU (acpidmar(4)) see the device and give it its
/// domain's DMA tag (`NACPIDMAR > 0`).
pub fn pci_probe_device_hook(pc: PciChipsetTag, pa: &mut PciAttachArgs) -> i32 {
    // NACPIDMAR > 0
    crate::dev::acpi::acpidmar::acpidmar_pci_hook(pc, pa);
    0
}

/// `pci_dev_postattach`: `NACPI > 0` would match the device with its ACPI node.
pub fn pci_dev_postattach(dev: &Device, pa: &PciAttachArgs) {
    // NACPI > 0
    // SAFETY: a PCI device that attached is never freed on amd64 (no PCI hot-unplug path
    // detaches one), so acpi(4) may keep it, as the C keeps the pointer.
    let dev: &'static Device = unsafe { &*core::ptr::from_ref(dev) };
    let _ = crate::dev::acpi::acpi::acpi_pci_match(dev, pa);
}

/// `pci_min_powerstate`: the lowest power state ACPI allows the device in the sleep state
/// the machine goes to.
pub fn pci_min_powerstate(pc: PciChipsetTag, tag: Pcitag) -> Pcireg {
    // NACPI > 0
    crate::dev::acpi::acpi::acpi_pci_min_powerstate(pc, tag)
}

/// `pci_set_powerstate_md`: tells ACPI (`_PSx`) about a power-state change.
pub fn pci_set_powerstate_md(pc: PciChipsetTag, tag: Pcitag, state: i32, pre: i32) {
    // NACPI > 0
    crate::dev::acpi::acpi::acpi_pci_set_powerstate(pc, tag, state, pre);
}
/* </CODE> */
