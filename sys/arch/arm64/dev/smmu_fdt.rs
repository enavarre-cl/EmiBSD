/* <LICENSES> */
/*
 * Copyright (c) 2021 Patrick Wildt <patrick@blueri.se>
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
//! The System MMU on the device tree: `arch/arm64/dev/smmu_fdt.c` (`smmu* at fdt?`).
//!
//! Upstream: sys/arch/arm64/dev/smmu_fdt.c @ 3ce1f3f79392
//!
//! Matches `arm,mmu-500`, `arm,smmu-v2` and `arm,smmu-v3`, maps the registers, attaches
//! the version's driver, establishes its interrupts and registers the SMMU as an IOMMU
//! (`ofw_misc.c`), so the nodes whose `iommus` (or the host bridges whose `iommu-map`)
//! name it get their DMA tags through it.
//!
//! ## Deviations
//! - `struct smmu_v2_fdt_softc` is empty; the union of the v2 and v3 softcs is the v3
//!   one.
//! - The `struct iommu_device` embedded in the softc is an `UnsafeCell<MaybeUninit<_>>`
//!   written once by the attach (`docs/C_TO_RUST.md`, softc members of function pointers):
//!   autoconf zeroes the softc, which a `fn` pointer cannot be.
//! - The interrupt handles are kept as raw pointers (`Cell<*mut c_void>`, null for NULL).
//! - The interrupts' name is the device's `dv_xname`, borrowed for `'static`: the softc is
//!   never detached.

use core::cell::{Cell, UnsafeCell};
use core::ffi::c_void;
use core::mem::MaybeUninit;
use core::ptr;

use crate::arch::arm64::arm64::intr::arm_intr_establish_fdt_idx;
use crate::arch::arm64::dev::smmu::{
    smmu_device_map, smmu_reserve_region, smmu_v2_attach, smmu_v2_context_irq, smmu_v2_global_irq,
    smmu_v3_attach, smmu_v3_event_irq, smmu_v3_gerr_irq, smmu_v3_priq_irq,
};
use crate::arch::arm64::dev::smmuvar::{SmmuCbIrq, SmmuSoftc};
use crate::dev::ofw::fdt::{OF_getindex, OF_getpropint, OF_getproplen, OF_is_compatible};
use crate::dev::ofw::ofw_misc::{IommuDevice, iommu_device_register};
use crate::kern::subr_prf::printf;
use crate::machine::bus::{BusAddr, BusDmaTag, BusSize, bus_space_map};
use crate::machine::fdt::FdtAttachArgs;
use crate::machine::intr::IPL_TTY;
use crate::sys::device::{CfMatch, Cfattach, Device, Softc};
use crate::sys::errno::Errno;
use crate::sys::queue::ListEntry;

/// `struct smmu_v3_fdt_softc`.
pub struct SmmuV3FdtSoftc {
    /// `sc_eih`: the event queue interrupt.
    pub sc_eih: Cell<*mut c_void>,
    /// `sc_gih`: the global error interrupt.
    pub sc_gih: Cell<*mut c_void>,
    /// `sc_pih`: the PRI queue interrupt.
    pub sc_pih: Cell<*mut c_void>,
}

/// `struct smmu_fdt_softc`.
#[repr(C)]
pub struct SmmuFdtSoftc {
    /// `sc_smmu`: the driver's softc, first (its device first in turn).
    pub sc_smmu: SmmuSoftc,
    /// `sc_id`: the IOMMU registered for the node, written once by the attach.
    pub sc_id: UnsafeCell<MaybeUninit<IommuDevice>>,
    /// `v3`.
    pub v3: SmmuV3FdtSoftc,
}

// SAFETY: `#[repr(C)]` with the SMMU softc (itself device-first and all-zero valid) first;
// `MaybeUninit` and null-pointer cells are valid as zeroes.
unsafe impl Softc for SmmuFdtSoftc {}

/// `smmu_fdt_ca`.
pub static SMMU_FDT_CA: Cfattach = Cfattach {
    ca_devsize: size_of::<SmmuFdtSoftc>(),
    ca_match: Some(smmu_fdt_match),
    ca_attach: smmu_fdt_attach,
    ca_detach: None,
    ca_activate: None,
};

/// `smmu_fdt_match(parent, match, aux)`: an MMU-500, an SMMUv2 or an SMMUv3.
pub fn smmu_fdt_match(_parent: Option<&Device>, _match: &CfMatch, aux: *mut c_void) -> i32 {
    // SAFETY: the device-tree buses hand their children `struct fdt_attach_args`.
    let faa = unsafe { &*aux.cast::<FdtAttachArgs<'_>>() };

    i32::from(
        OF_is_compatible(faa.fa_node, b"arm,mmu-500")
            || OF_is_compatible(faa.fa_node, b"arm,smmu-v2")
            || OF_is_compatible(faa.fa_node, b"arm,smmu-v3"),
    )
}

/// `smmu_fdt_attach(parent, self, aux)`.
pub fn smmu_fdt_attach(_parent: Option<&Device>, self_: &Device, aux: *mut c_void) {
    // SAFETY: as in `smmu_fdt_match`.
    let faa = unsafe { &*aux.cast::<FdtAttachArgs<'_>>() };
    // SAFETY: `smmu_fdt_ca` makes `SmmuFdtSoftc`s; the allocation lives as long as the
    // device, which is never detached.
    let fsc: &'static SmmuFdtSoftc = unsafe { &*ptr::from_ref(self_.softc::<SmmuFdtSoftc>()) };
    let sc = &fsc.sc_smmu;
    let mut ret = Err(Errno::ENXIO);

    let Some(reg) = faa.fa_reg.first() else {
        printf(format_args!(": no registers\n"));
        return;
    };

    sc.sc_dmat.set(Some(faa.fa_dmat));
    sc.sc_iot.set(Some(faa.fa_iot));
    // SAFETY: the node's register window, which only this driver drives.
    match unsafe { bus_space_map(faa.fa_iot, reg.addr as usize, reg.size as usize, 0) } {
        Ok(ioh) => sc.sc_ioh.set(Some(ioh)),
        Err(_) => {
            printf(format_args!(": can't map registers\n"));
            return;
        }
    }

    if OF_is_compatible(faa.fa_node, b"arm,mmu-500")
        || OF_is_compatible(faa.fa_node, b"arm,smmu-v2")
    {
        ret = smmu_v2_fdt_attach(fsc, faa.fa_node);
    }

    if OF_is_compatible(faa.fa_node, b"arm,smmu-v3") {
        ret = smmu_v3_fdt_attach(fsc, faa.fa_node);
    }

    if ret.is_err() {
        return;
    }

    // SAFETY: written once, here, before the IOMMU is registered; nothing reads it before.
    let id: &'static IommuDevice = unsafe {
        (*fsc.sc_id.get()).write(IommuDevice {
            id_node: faa.fa_node,
            id_cookie: ptr::from_ref(fsc).cast_mut().cast::<c_void>(),
            id_map: smmu_fdt_map,
            id_mirror: None,
            id_reserve: smmu_fdt_reserve,
            id_list: ListEntry::new(),
            id_phandle: Cell::new(0),
        })
    };
    // SAFETY: the softc's own IOMMU, registered once and never freed.
    unsafe { iommu_device_register(id) };
}

/// The device's name for its interrupts.
fn smmu_fdt_name(sc: &'static SmmuSoftc) -> &'static str {
    sc.sc_dev.xname()
}

/// The softc as an interrupt cookie.
fn smmu_cookie(sc: &'static SmmuSoftc) -> *mut c_void {
    ptr::from_ref(sc).cast_mut().cast::<c_void>()
}

/// `smmu_v2_fdt_attach(fsc, node)`: an MMU-500 or SMMUv2 with its global interrupts and one
/// interrupt per context bank.
fn smmu_v2_fdt_attach(fsc: &'static SmmuFdtSoftc, node: i32) -> Result<(), Errno> {
    let sc = &fsc.sc_smmu;

    if OF_is_compatible(node, b"arm,mmu-500") {
        sc.sc_is_mmu500.set(1);
    }
    if OF_is_compatible(node, b"marvell,ap806-smmu-500") {
        sc.sc_is_ap806.set(1);
    }
    if OF_is_compatible(node, b"qcom,sc7280-smmu-500")
        || OF_is_compatible(node, b"qcom,sc8280xp-smmu-500")
        || OF_is_compatible(node, b"qcom,x1e80100-smmu-500")
    {
        sc.sc_is_qcom.set(1);
    }
    if OF_getproplen(node, b"dma-coherent") == 0 {
        sc.sc_coherent.set(1);
    }

    if sc.sc_is_qcom.get() != 0 {
        printf(format_args!(": disabled\n"));
        return Err(Errno::ENXIO);
    }

    smmu_v2_attach(sc).map_err(|_| Errno::ENXIO)?;

    let name = smmu_fdt_name(sc);
    let ngirq = OF_getpropint(node, b"#global-interrupts", 1) as usize;
    for i in 0..ngirq {
        let _ =
            arm_intr_establish_fdt_idx(node, i, IPL_TTY, smmu_v2_global_irq, smmu_cookie(sc), name);
    }
    let mut i = ngirq;
    loop {
        let cbi: &'static mut SmmuCbIrq =
            alloc::boxed::Box::leak(alloc::boxed::Box::new(SmmuCbIrq {
                cbi_sc: sc,
                cbi_idx: (i - ngirq) as i32,
            }));
        let cookie = ptr::from_mut(cbi).cast::<c_void>();
        if arm_intr_establish_fdt_idx(node, i, IPL_TTY, smmu_v2_context_irq, cookie, name).is_none()
        {
            // SAFETY: the box leaked above, which nothing kept.
            drop(unsafe { alloc::boxed::Box::from_raw(cookie.cast::<SmmuCbIrq>()) });
            break;
        }
        i += 1;
    }

    Ok(())
}

/// `smmu_v3_fdt_attach(fsc, node)`: an SMMUv3 with its event queue, global error and (with
/// PRI) PRI queue interrupts, found by name.
fn smmu_v3_fdt_attach(fsc: &'static SmmuFdtSoftc, node: i32) -> Result<(), Errno> {
    let sc = &fsc.sc_smmu;
    let name = smmu_fdt_name(sc);

    if OF_getproplen(node, b"dma-coherent") == 0 {
        sc.sc_coherent.set(1);
    }

    smmu_v3_attach(sc).map_err(|_| Errno::ENXIO)?;

    let idx = OF_getindex(node, Some(b"eventq"), b"interrupt-names");
    if idx < 0 {
        printf(format_args!("{}: no eventq interrupt\n", name));
        return Err(Errno::ENXIO);
    }
    let ih = arm_intr_establish_fdt_idx(
        node,
        idx as usize,
        IPL_TTY,
        smmu_v3_event_irq,
        smmu_cookie(sc),
        name,
    );
    fsc.v3
        .sc_eih
        .set(ih.map_or(ptr::null_mut(), |ih| ih.as_ptr().cast()));
    if ih.is_none() {
        printf(format_args!("{}: can't establish eventq interrupt\n", name));
        return Err(Errno::ENXIO);
    }

    let idx = OF_getindex(node, Some(b"gerror"), b"interrupt-names");
    if idx < 0 {
        printf(format_args!("{}: no gerror interrupt\n", name));
        return Err(Errno::ENXIO);
    }
    let ih = arm_intr_establish_fdt_idx(
        node,
        idx as usize,
        IPL_TTY,
        smmu_v3_gerr_irq,
        smmu_cookie(sc),
        name,
    );
    fsc.v3
        .sc_gih
        .set(ih.map_or(ptr::null_mut(), |ih| ih.as_ptr().cast()));
    if ih.is_none() {
        printf(format_args!("{}: can't establish gerror interrupt\n", name));
        return Err(Errno::ENXIO);
    }

    if sc.v3.sc_has_pri.get() != 0 {
        let idx = OF_getindex(node, Some(b"priq"), b"interrupt-names");
        if idx < 0 {
            // No priq interrupt, PRI will be broken
            return Ok(());
        }
        let ih = arm_intr_establish_fdt_idx(
            node,
            idx as usize,
            IPL_TTY,
            smmu_v3_priq_irq,
            smmu_cookie(sc),
            name,
        );
        fsc.v3
            .sc_pih
            .set(ih.map_or(ptr::null_mut(), |ih| ih.as_ptr().cast()));
        if ih.is_none() {
            printf(format_args!("{}: can't establish priq interrupt\n", name));
            return Err(Errno::ENXIO);
        }
    }

    Ok(())
}

/// `smmu_fdt_map(cookie, cells, dmat)`: the tag of the stream ID in `cells[0]`.
pub fn smmu_fdt_map(cookie: *mut c_void, cells: &[u32], dmat: BusDmaTag) -> BusDmaTag {
    // `&fsc->sc_smmu` is the softc's own address (`#[repr(C)]`, first member).
    smmu_device_map(cookie, cells[0], Some(dmat)).unwrap_or(dmat)
}

/// `smmu_fdt_reserve(cookie, cells, addr, size)`.
pub fn smmu_fdt_reserve(cookie: *mut c_void, cells: &[u32], addr: BusAddr, size: BusSize) {
    smmu_reserve_region(cookie, cells[0], addr, size);
}
/* </CODE> */
