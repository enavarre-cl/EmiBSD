/*	$OpenBSD: acpi_machdep.c,v 1.22 2024/05/22 05:51:49 jsg Exp $	*/
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
//! The arm64 half of acpi(4): `arch/arm64/arm64/acpi_machdep.c`. acpi0 attaches to the
//! device tree node efiboot makes from the UEFI configuration table (`openbsd,acpi-5.0`,
//! its `reg` the RSDP); the firmware tables are mapped with `km_alloc` and
//! `pmap_kenter_pa`, the AML interpreter's registers through the plain `bus_space_map`,
//! interrupts go to the GIC (the interrupt controller with phandle 1, which efiboot's tree
//! gives the GIC), and the IOMMU mapping of a device to `acpiiort`. There is no global lock
//! on arm64. Generic code reaches it through `machine::acpi_machdep`.
//!
//! Upstream: sys/arch/arm64/arm64/acpi_machdep.c @ 3ce1f3f79392
//!
//! ## Deviations
//! - `acpi_map` returns the mapping (the C fills a handle and returns 0 or `ENOMEM`).
//! - `acpi_attach_machdep`'s `apm_setinfohook(acpi_apminfo)` (`NAPM > 0`: arm64 GENERIC has
//!   `apm0 at mainbus?`) is reported: `apm(4)` and `acpiapm.c` are not ported.
//! - `pwr_action` is `machine::acpi_machdep::pwr_action()`'s constant 1 (nothing writes it).

use core::ffi::c_void;
use core::ptr::{self, NonNull};

use crate::arch::arm64::arm64::intr::interrupt_controllers;
use crate::arch::arm64::arm64::pmap::{pmap_kenter_pa, pmap_kernel, pmap_kremove};
use crate::arch::arm64::dev::acpiiort::acpiiort_device_map;
use crate::arch::arm64::include::bus::{self, BUS_DMA_COHERENT};
use crate::arch::arm64::include::fdt::FdtAttachArgs;
use crate::arch::arm64::include::intr::MachineIntrHandle;
use crate::arch::arm64::include::param::{NBPG, PGOFSET};
use crate::dev::acpi::acpi::acpi_attach_common;
use crate::dev::acpi::acpivar::{AcpiMemMap, AcpiSoftc};
use crate::dev::acpi::amltypes::AmlNodeRef;
use crate::dev::acpi::dsdt::{LR_EXTIRQ_MODE, LR_EXTIRQ_POLARITY};
use crate::dev::ofw::openfirm::OF_is_compatible;
use crate::kern::kern_malloc::{free, malloc};
use crate::kern::subr_prf::panic;
use crate::machine::bus::{
    BusAddr, BusDmaTag, BusSize, BusSpaceHandle, BusSpaceTag, bus_space_map, bus_space_unmap,
};
use crate::machine::pmap::pmap_update;
use crate::sys::device::{CfMatch, Cfattach, Device};
use crate::sys::errno::Errno;
use crate::sys::malloc::{M_DEVBUF, M_WAITOK, M_ZERO};
use crate::sys::mman::{PROT_READ, PROT_WRITE};
use crate::sys::types::{Paddr, Vaddr, Vsize};
use crate::unported;
use crate::uvm::uvm_km::{KD_NOWAIT, KP_NONE, KV_ANY, km_alloc, km_free};
use crate::uvm::uvm_param::{round_page, trunc_page};

/// `pwr_action`: what the power button does (1: power down).
pub const PWR_ACTION: i32 = 1;

/// `acpi_fdt_ca`.
pub static ACPI_FDT_CA: Cfattach = Cfattach {
    ca_devsize: size_of::<AcpiSoftc>(),
    ca_match: Some(acpi_fdt_match),
    ca_attach: acpi_fdt_attach,
    ca_detach: None,
    ca_activate: None,
};

/// `acpi_fdt_match(parent, match, aux)`: the node efiboot made for the ACPI tables.
pub fn acpi_fdt_match(_parent: Option<&Device>, _match: &CfMatch, aux: *mut c_void) -> i32 {
    // SAFETY: the device-tree buses hand their children `struct fdt_attach_args`.
    let faa = unsafe { &*aux.cast::<FdtAttachArgs<'_>>() };

    i32::from(OF_is_compatible(faa.fa_node, b"openbsd,acpi-5.0"))
}

/// `acpi_fdt_attach(parent, self, aux)`: acpi0, from the RSDP the node's `reg` names, with
/// a coherent copy of the node's DMA tag.
pub fn acpi_fdt_attach(_parent: Option<&Device>, self_: &Device, aux: *mut c_void) {
    // SAFETY: `acpi_fdt_ca`'s softc is an `AcpiSoftc` (`ca_devsize`), never freed (acpi0
    // does not detach).
    let sc: &'static AcpiSoftc = unsafe { &*ptr::from_ref(self_.softc::<AcpiSoftc>()) };
    // SAFETY: as in `acpi_fdt_match`.
    let faa = unsafe { &*aux.cast::<FdtAttachArgs<'_>>() };

    sc.sc_memt.set(Some(faa.fa_iot));
    sc.sc_ci_dmat.set(Some(faa.fa_dmat));

    // Create coherent DMA tag.
    let Some(p) = malloc(size_of::<bus::BusDmaTag>(), M_DEVBUF, M_WAITOK | M_ZERO) else {
        panic(format_args!("acpi_fdt_attach: out of memory for a dma tag"));
    };
    let dmat = p.cast::<bus::BusDmaTag>();
    // SAFETY: a fresh allocation of a tag's size, written once before it is shared and never
    // freed, as in C.
    let dmat: BusDmaTag = unsafe {
        dmat.write(bus::BusDmaTag {
            _flags: faa.fa_dmat._flags | BUS_DMA_COHERENT,
            ..*faa.fa_dmat
        });
        &*dmat.as_ptr()
    };
    sc.sc_cc_dmat.set(Some(dmat));

    let Some(reg) = faa.fa_reg.first() else {
        return;
    };
    acpi_attach_common(sc, Paddr::new(reg.addr as usize));
}

/// `acpi_map(pa, len, handle)`: maps `len` bytes of physical memory at `pa` read-write into
/// fresh kernel virtual space.
pub fn acpi_map(pa: Paddr, len: usize) -> Result<AcpiMemMap, Errno> {
    let pa = pa.as_usize();
    let mut pgpa = trunc_page(pa);
    let endpa = round_page(pa + len);
    let Some(va) = km_alloc(endpa - pgpa, &KV_ANY, &KP_NONE, &KD_NOWAIT) else {
        return Err(Errno::ENOMEM);
    };
    let mut va = va.as_ptr() as usize;

    let handle = AcpiMemMap {
        baseva: va,
        va: va + (pa & PGOFSET),
        vsize: endpa - pgpa,
        pa,
    };

    loop {
        // SAFETY: `va` is fresh kernel virtual space from km_alloc(kv_any, kp_none) that
        // nothing else maps; `pgpa` is firmware memory the caller names.
        unsafe { pmap_kenter_pa(Vaddr::new(va), Paddr::new(pgpa), PROT_READ | PROT_WRITE) };
        va += NBPG;
        pgpa += NBPG;
        if pgpa >= endpa {
            break;
        }
    }
    pmap_update(pmap_kernel());

    Ok(handle)
}

/// `acpi_unmap(handle)`.
pub fn acpi_unmap(handle: &AcpiMemMap) {
    // SAFETY: `acpi_map` entered these pages; the caller is done with them.
    unsafe { pmap_kremove(Vaddr::new(handle.baseva), Vsize::new(handle.vsize)) };
    pmap_update(pmap_kernel());
    if let Some(va) = NonNull::new(handle.baseva as *mut u8) {
        km_free(va, handle.vsize, &KV_ANY, &KP_NONE);
    }
}

/// `acpi_bus_space_map(t, addr, size, flags, &bsh)`: `bus_space_map`.
///
/// # Safety
///
/// As for `bus_space_map`.
pub unsafe fn acpi_bus_space_map(
    t: BusSpaceTag,
    addr: BusAddr,
    size: BusSize,
    flags: i32,
) -> Result<BusSpaceHandle, Errno> {
    // SAFETY: the caller's guarantee.
    unsafe { bus_space_map(t, addr, size, flags as u32) }
}

/// `acpi_bus_space_unmap(t, bsh, size)`.
pub fn acpi_bus_space_unmap(t: BusSpaceTag, bsh: BusSpaceHandle, size: BusSize) {
    bus_space_unmap(t, bsh, size);
}

/// `acpi_acquire_glk(lock)`: no global lock; always ours.
pub fn acpi_acquire_glk(_lock: *mut u32) -> i32 {
    // No global lock.
    1
}

/// `acpi_release_glk(lock)`: no global lock; nobody waits.
pub fn acpi_release_glk(_lock: *mut u32) -> i32 {
    // No global lock.
    0
}

/// `acpi_attach_machdep(sc)`: the APM information hook (`NAPM > 0`).
pub fn acpi_attach_machdep(_sc: &'static AcpiSoftc) {
    // apm_setinfohook(acpi_apminfo): see the deviations.
    let _ = unported!("acpi_attach_machdep: apm_setinfohook (apm.c, acpiapm.c)");
}

/// `acpi_intr_establish(irq, flags, level, func, arg, name)`: the GSI `irq` on the GIC
/// (phandle 1), edge or level and its polarity from the `_CRS` flags.
pub fn acpi_intr_establish(
    irq: i32,
    flags: i32,
    level: i32,
    func: fn(*mut c_void) -> i32,
    arg: *mut c_void,
    name: &'static str,
) -> Option<NonNull<c_void>> {
    let ic = interrupt_controllers()
        .iter()
        .find(|ic| ic.ic_phandle.get() == 1)?;

    let mode = if flags & i32::from(LR_EXTIRQ_MODE) != 0 {
        // edge
        if flags & i32::from(LR_EXTIRQ_POLARITY) != 0 {
            0x2 // falling
        } else {
            0x1 // rising
        }
    } else {
        // level
        if flags & i32::from(LR_EXTIRQ_POLARITY) != 0 {
            0x8 // low
        } else {
            0x4 // high
        }
    };
    let interrupt: [u32; 3] = [0, (irq - 32) as u32, mode];

    let establish = ic.ic_establish?;
    let cookie = establish(ic.ic_cookie.get(), &interrupt, level, None, func, arg, name);
    if cookie.is_null() {
        return None;
    }

    let aih =
        malloc(size_of::<MachineIntrHandle>(), M_DEVBUF, M_WAITOK)?.cast::<MachineIntrHandle>();
    // SAFETY: a fresh allocation of a handle's size, written once before use.
    unsafe {
        aih.write(MachineIntrHandle {
            ih_ic: ic,
            ih_ih: cookie,
        });
    }

    Some(aih.cast())
}

/// `acpi_intr_disestablish(cookie)`.
///
/// # Safety
///
/// `cookie` came from `acpi_intr_establish` and is not used afterwards.
pub unsafe fn acpi_intr_disestablish(cookie: NonNull<c_void>) {
    let aih = cookie.cast::<MachineIntrHandle>();
    // SAFETY: the caller's guarantee: a handle `acpi_intr_establish` wrote.
    let h = unsafe { aih.as_ref() };
    // SAFETY: an established handle names a registered controller, never freed.
    let ic = unsafe { &*h.ih_ic };

    if let Some(dis) = ic.ic_disestablish {
        dis(h.ih_ih);
    }
    free(aih.cast(), M_DEVBUF, size_of::<MachineIntrHandle>());
}

/// `acpi_iommu_device_map(node, dmat)`: through the IORT (`acpiiort_device_map`).
pub fn acpi_iommu_device_map(node: &AmlNodeRef, dmat: Option<BusDmaTag>) -> Option<BusDmaTag> {
    acpiiort_device_map(node, dmat)
}
/* </CODE> */
