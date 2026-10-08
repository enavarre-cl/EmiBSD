/* $OpenBSD: smmu_acpi.c,v 1.14 2026/01/24 16:07:09 kettenis Exp $ */
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
//! The System MMU through the IORT: `arch/arm64/dev/smmu_acpi.c` (`smmu* at acpiiort?`).
//!
//! Upstream: sys/arch/arm64/dev/smmu_acpi.c @ 3ce1f3f79392
//!
//! acpiiort0 offers each IORT node; an SMMU node's driver maps the registers the node
//! names, attaches the version's driver with the node's interrupts and registers itself
//! with acpiiort (`acpiiort_smmu_register`), through which `acpipci` and ACPI devices get
//! their DMA tags. At the pin, `smmu_acpi_match` takes only `ACPI_IORT_SMMU` (v2) nodes:
//! an SMMUv3 node (QEMU `virt,acpi=on,iommu=smmuv3`) is not matched, so its devices keep
//! their tags and the SMMU stays as the firmware left it; the v3 attach is ported for when
//! the match takes it.
//!
//! ## Deviations
//! - The union of the v2 and v3 softcs is a struct with both.
//! - The node's data, its global and context interrupt records are read from acpi0's copy
//!   of the IORT in bounds (`acpiiort.rs`'s `iort_read`), where the C follows the
//!   offsets unchecked; a record past the table fails the attach with `ENXIO`.
//! - The interrupt handles are raw pointers (`Cell<*mut c_void>`, null for NULL).
//! - The QCOM `_HID` walk starts at a clone of acpi0's `sc_root`, so no borrow of it is held
//!   while `acpi_parsehid` evaluates.

use alloc::boxed::Box;
use core::cell::Cell;
use core::ffi::c_void;
use core::ptr;

use crate::arch::arm64::arm64::acpi_machdep::acpi_intr_establish;
use crate::arch::arm64::dev::acpiiort::{
    AcpiiortAttachArgs, AcpiiortSmmu, IortPlain, acpiiort_smmu_register, acpiiort_table, iort_read,
};
use crate::arch::arm64::dev::smmu::{
    smmu_device_map, smmu_reserve_region, smmu_v2_attach, smmu_v2_context_irq, smmu_v2_global_irq,
    smmu_v3_attach, smmu_v3_event_irq, smmu_v3_gerr_irq, smmu_v3_priq_irq,
};
use crate::arch::arm64::dev::smmuvar::{SmmuCbIrq, SmmuSoftc};
use crate::arch::arm64::include::bus::{BUS_DMA_COHERENT, BusDmaTag};
use crate::dev::acpi::acpi::acpi_parsehid;
use crate::dev::acpi::acpireg::{
    ACPI_IORT_SMMU, ACPI_IORT_SMMU_CAVIUM_THUNDERX, ACPI_IORT_SMMU_COHERENT,
    ACPI_IORT_SMMU_CORELINK_MMU400, ACPI_IORT_SMMU_CORELINK_MMU401, ACPI_IORT_SMMU_CORELINK_MMU500,
    ACPI_IORT_SMMU_INTR_EDGE, ACPI_IORT_SMMU_V1, ACPI_IORT_SMMU_V2, ACPI_IORT_SMMU_V3,
    ACPI_IORT_SMMU_V3_GENERIC, AcpiIortNode, AcpiIortSmmuContextInterrupt,
    AcpiIortSmmuGlobalInterrupt, AcpiIortSmmuNode, AcpiIortSmmuV3Node,
    acpi_iort_smmu_v3_cohacc_override,
};
use crate::dev::acpi::acpivar::{AcpiSoftc, acpi_softc};
use crate::dev::acpi::amltypes::AmlNodeRef;
use crate::dev::acpi::dsdt::{LR_EXTIRQ_MODE, aml_find_node};
use crate::kern::subr_prf::printf;
use crate::machine::bus::bus_space_map;
use crate::machine::intr::IPL_TTY;
use crate::sys::device::{CfMatch, Cfattach, Device, Softc};
use crate::sys::errno::Errno;
use crate::sys::queue::SimpleqEntry;

/// `struct smmu_v2_acpi_softc`.
pub struct SmmuV2AcpiSoftc {
    /// `sc_gih`: the global interrupt.
    pub sc_gih: Cell<*mut c_void>,
}

/// `struct smmu_v3_acpi_softc`.
pub struct SmmuV3AcpiSoftc {
    /// `sc_eih`: the event queue interrupt.
    pub sc_eih: Cell<*mut c_void>,
    /// `sc_gih`: the global error interrupt.
    pub sc_gih: Cell<*mut c_void>,
    /// `sc_pih`: the PRI queue interrupt.
    pub sc_pih: Cell<*mut c_void>,
}

/// `struct smmu_acpi_softc`.
#[repr(C)]
pub struct SmmuAcpiSoftc {
    /// `sc_smmu`: the driver's softc, first.
    pub sc_smmu: SmmuSoftc,
    /// `v2`.
    pub v2: SmmuV2AcpiSoftc,
    /// `v3`.
    pub v3: SmmuV3AcpiSoftc,
}

// SAFETY: `#[repr(C)]` with the SMMU softc (device-first, all-zero valid) first; the
// interrupt cells are valid as null.
unsafe impl Softc for SmmuAcpiSoftc {}

// SAFETY: `#[repr(C, packed)]` structures of integers only.
unsafe impl IortPlain for AcpiIortSmmuNode {}
// SAFETY: as above.
unsafe impl IortPlain for AcpiIortSmmuV3Node {}
// SAFETY: as above.
unsafe impl IortPlain for AcpiIortSmmuGlobalInterrupt {}
// SAFETY: as above.
unsafe impl IortPlain for AcpiIortSmmuContextInterrupt {}

/// `smmu_acpi_ca`.
pub static SMMU_ACPI_CA: Cfattach = Cfattach {
    ca_devsize: size_of::<SmmuAcpiSoftc>(),
    ca_match: Some(smmu_acpi_match),
    ca_attach: smmu_acpi_attach,
    ca_detach: None,
    ca_activate: None,
};

/// The IORT node acpiiort0 offers, read from its table.
fn aia_node(aia: &AcpiiortAttachArgs) -> AcpiIortNode {
    // SAFETY: acpiiort0 points `aia_node` at a node it read in bounds from acpi0's copy of
    // the IORT, which is never freed; the read is unaligned (a packed table).
    unsafe { ptr::read_unaligned(aia.aia_node) }
}

/// `smmu_acpi_match(parent, match, aux)`: an `ACPI_IORT_SMMU` node.
pub fn smmu_acpi_match(_parent: Option<&Device>, _match: &CfMatch, aux: *mut c_void) -> i32 {
    // SAFETY: acpiiort0 hands its children `struct acpiiort_attach_args`.
    let aia = unsafe { &*aux.cast_const().cast::<AcpiiortAttachArgs>() };
    let node = aia_node(aia);

    if node.r#type != ACPI_IORT_SMMU {
        return 0;
    }

    1
}

/// `smmu_acpi_attach(parent, self, aux)`.
pub fn smmu_acpi_attach(_parent: Option<&Device>, self_: &Device, aux: *mut c_void) {
    // SAFETY: as in `smmu_acpi_match`.
    let aia = unsafe { &*aux.cast_const().cast::<AcpiiortAttachArgs>() };
    // SAFETY: `smmu_acpi_ca` makes `SmmuAcpiSoftc`s; the allocation lives as long as the
    // device, which is never detached.
    let asc: &'static SmmuAcpiSoftc = unsafe { &*ptr::from_ref(self_.softc::<SmmuAcpiSoftc>()) };
    let sc = &asc.sc_smmu;
    let node = aia_node(aia);
    let mut ret = Err(Errno::ENXIO);

    sc.sc_dmat.set(aia.aia_dmat);
    sc.sc_iot.set(aia.aia_memt);

    if node.r#type == ACPI_IORT_SMMU {
        ret = smmu_v2_acpi_attach(asc, aia.aia_node);
    }
    if node.r#type == ACPI_IORT_SMMU_V3 {
        ret = smmu_v3_acpi_attach(asc, aia.aia_node);
    }

    if ret.is_err() {
        return;
    }

    let as_: &'static AcpiiortSmmu = Box::leak(Box::new(AcpiiortSmmu {
        as_list: SimpleqEntry::new(),
        as_node: aia.aia_node,
        as_cookie: ptr::from_ref(sc).cast_mut().cast::<c_void>(),
        as_map: smmu_device_map,
        as_reserve: smmu_reserve_region,
    }));
    // SAFETY: a fresh registration, in no list, never freed.
    unsafe { acpiiort_smmu_register(as_) };
}

/// The IORT and the offset of `node` in it.
fn iort_of(node: *const AcpiIortNode) -> Option<(&'static [u8], usize)> {
    let iort = acpi_softc().and_then(acpiiort_table)?;
    let off = (node as usize).checked_sub(iort.as_ptr() as usize)?;
    (off < iort.len()).then_some((iort, off))
}

/// The edge flag of an IORT SMMU interrupt record as `acpi_intr_establish` flags.
fn smmu_acpi_intr_flags(flags: u32) -> i32 {
    if flags & ACPI_IORT_SMMU_INTR_EDGE != 0 {
        i32::from(LR_EXTIRQ_MODE)
    } else {
        0
    }
}

/// The softc as an interrupt cookie.
fn smmu_cookie(sc: &'static SmmuSoftc) -> *mut c_void {
    ptr::from_ref(sc).cast_mut().cast::<c_void>()
}

/// `smmu_v2_acpi_attach(asc, node)`: an SMMUv2 node: registers, model, coherency, then the
/// global and context interrupts.
fn smmu_v2_acpi_attach(
    asc: &'static SmmuAcpiSoftc,
    node: *const AcpiIortNode,
) -> Result<(), Errno> {
    let sc = &asc.sc_smmu;
    let (iort, off) = iort_of(node).ok_or(Errno::ENXIO)?;
    let smmu: AcpiIortSmmuNode =
        iort_read(iort, off + size_of::<AcpiIortNode>()).ok_or(Errno::ENXIO)?;
    let (base, span) = (smmu.base_address, smmu.span);

    printf(format_args!(" addr 0x{:x}/0x{:x}", base, span));

    let iot = sc.sc_iot.get().ok_or(Errno::EIO)?;
    // SAFETY: the SMMU's register window the IORT names, which only this driver drives.
    match unsafe { bus_space_map(iot, base as usize, span as usize, 0) } {
        Ok(ioh) => sc.sc_ioh.set(Some(ioh)),
        Err(_) => {
            printf(format_args!(": can't map registers\n"));
            return Err(Errno::EIO);
        }
    }

    let model = smmu.model;
    match model {
        ACPI_IORT_SMMU_V1 | ACPI_IORT_SMMU_CORELINK_MMU400 | ACPI_IORT_SMMU_CORELINK_MMU401 => {
            printf(format_args!(": SMMUv1 is unsupported\n"));
            return Err(Errno::ENXIO);
        }
        ACPI_IORT_SMMU_CORELINK_MMU500 => sc.sc_is_mmu500.set(1),
        ACPI_IORT_SMMU_V2 | ACPI_IORT_SMMU_CAVIUM_THUNDERX => {}
        _ => {
            printf(format_args!(": unknown model {}\n", model));
            return Err(Errno::ENXIO);
        }
    }

    if smmu.flags & ACPI_IORT_SMMU_COHERENT != 0 {
        sc.sc_coherent.set(1);
    }

    // Check for QCOM devices to enable quirk.
    smmu_acpi_findqcom(sc);

    // FIXME: Don't configure on QCOM until its runtime use is fixed.
    if sc.sc_is_qcom.get() != 0 {
        printf(format_args!(": disabled\n"));
        return Err(Errno::ENXIO);
    }

    smmu_v2_attach(sc).map_err(|_| Errno::ENXIO)?;

    let name: &'static str = sc.sc_dev.xname();
    let girq: AcpiIortSmmuGlobalInterrupt =
        iort_read(iort, off + smmu.global_interrupt_offset as usize).ok_or(Errno::ENXIO)?;
    let gih = acpi_intr_establish(
        girq.nsgirpt_gsiv as i32,
        smmu_acpi_intr_flags(girq.nsgirpt_flags),
        IPL_TTY,
        smmu_v2_global_irq,
        smmu_cookie(sc),
        name,
    );
    asc.v2
        .sc_gih
        .set(gih.map_or(ptr::null_mut(), |ih| ih.as_ptr()));
    if gih.is_none() {
        return Err(Errno::ENXIO);
    }

    let cirq_off = off + smmu.context_interrupt_offset as usize;
    for i in 0..smmu.number_of_context_interrupts as usize {
        let cirq: AcpiIortSmmuContextInterrupt = iort_read(
            iort,
            cirq_off + i * size_of::<AcpiIortSmmuContextInterrupt>(),
        )
        .ok_or(Errno::ENXIO)?;
        let cbi: &'static SmmuCbIrq = Box::leak(Box::new(SmmuCbIrq {
            cbi_sc: sc,
            cbi_idx: i as i32,
        }));
        let _ = acpi_intr_establish(
            cirq.gsiv as i32,
            smmu_acpi_intr_flags(cirq.flags),
            IPL_TTY,
            smmu_v2_context_irq,
            ptr::from_ref(cbi).cast_mut().cast::<c_void>(),
            name,
        );
    }

    Ok(())
}

/// `aml_find_node(acpi_softc->sc_root, "_HID", smmu_acpi_foundqcom, sc)`.
fn smmu_acpi_findqcom(sc: &SmmuSoftc) {
    let Some(acpi) = acpi_softc() else {
        return;
    };
    let root = acpi.sc_root.borrow().clone();
    if let Some(root) = root {
        aml_find_node(&root, b"_HID", &mut |n| smmu_acpi_foundqcom(n, acpi, sc));
    }
}

/// `smmu_acpi_foundqcom(node, arg)`: marks the SMMU of a Qualcomm SoC, named by the `_HID`s
/// of its devices.
fn smmu_acpi_foundqcom(node: &AmlNodeRef, acpi: &AcpiSoftc, sc: &SmmuSoftc) -> i32 {
    let mut cdev = [0u8; 32];
    let mut dev = [0u8; 32];

    if acpi_parsehid(node, acpi, &mut cdev, &mut dev) != 0 {
        return 0;
    }

    let dev = &dev[..dev.iter().position(|&c| c == 0).unwrap_or(dev.len())];
    if dev == b"QCOM0409" // SC8180X/XP
        || dev == b"QCOM0609" // SC8280XP
        || dev == b"QCOM0809" // SC7180
        || dev == b"QCOM0C09" // X1E80100
        || dev == b"QCOM0E09"
    // QSC6490
    {
        sc.sc_is_qcom.set(1);
    }

    0
}

/// `smmu_v3_acpi_attach(asc, node)`: an SMMUv3 node: registers (a fixed 128 KB span), model,
/// the coherency override, then the event, global error and PRI interrupts.
fn smmu_v3_acpi_attach(
    asc: &'static SmmuAcpiSoftc,
    node: *const AcpiIortNode,
) -> Result<(), Errno> {
    let sc = &asc.sc_smmu;
    let (iort, off) = iort_of(node).ok_or(Errno::ENXIO)?;
    let smmu: AcpiIortSmmuV3Node =
        iort_read(iort, off + size_of::<AcpiIortNode>()).ok_or(Errno::ENXIO)?;
    let span: u64 = 0x20000;
    let base = smmu.base_address;

    printf(format_args!(" addr 0x{:x}/0x{:x}", base, span));

    let iot = sc.sc_iot.get().ok_or(Errno::EIO)?;
    // SAFETY: the SMMU's register window the IORT names, which only this driver drives.
    match unsafe { bus_space_map(iot, base as usize, span as usize, 0) } {
        Ok(ioh) => sc.sc_ioh.set(Some(ioh)),
        Err(_) => {
            printf(format_args!(": can't map registers\n"));
            return Err(Errno::EIO);
        }
    }

    let model = smmu.model;
    if model != ACPI_IORT_SMMU_V3_GENERIC {
        printf(format_args!(": unknown model {}\n", model));
        return Err(Errno::ENXIO);
    }

    if acpi_iort_smmu_v3_cohacc_override(smmu.flags) != 0
        && let Some(dmat) = sc.sc_dmat.get()
    {
        let dmat: &'static BusDmaTag = Box::leak(Box::new(BusDmaTag {
            _flags: dmat._flags | BUS_DMA_COHERENT,
            ..*dmat
        }));
        sc.sc_dmat.set(Some(dmat));
        sc.sc_coherent.set(1);
    }

    // Check for QCOM devices to enable quirk.
    smmu_acpi_findqcom(sc);

    // FIXME: Don't configure on QCOM until its runtime use is fixed.
    if sc.sc_is_qcom.get() != 0 {
        printf(format_args!(": disabled\n"));
        return Err(Errno::ENXIO);
    }

    smmu_v3_attach(sc).map_err(|_| Errno::ENXIO)?;

    let name: &'static str = sc.sc_dev.xname();
    let establish = |gsiv: u32, func: fn(*mut c_void) -> i32, slot: &Cell<*mut c_void>| {
        let ih = acpi_intr_establish(
            gsiv as i32,
            i32::from(LR_EXTIRQ_MODE),
            IPL_TTY,
            func,
            smmu_cookie(sc),
            name,
        );
        slot.set(ih.map_or(ptr::null_mut(), |ih| ih.as_ptr()));
    };
    let (event, gerr, pri) = (smmu.event, smmu.gerr, smmu.pri);
    if event != 0 {
        establish(event, smmu_v3_event_irq, &asc.v3.sc_eih);
    }
    if gerr != 0 {
        establish(gerr, smmu_v3_gerr_irq, &asc.v3.sc_gih);
    }
    if sc.v3.sc_has_pri.get() != 0 && pri != 0 {
        establish(pri, smmu_v3_priq_irq, &asc.v3.sc_pih);
    }

    let (eih, gih, pih) = (
        asc.v3.sc_eih.get().is_null(),
        asc.v3.sc_gih.get().is_null(),
        asc.v3.sc_pih.get().is_null(),
    );
    if eih || gih || (sc.v3.sc_has_pri.get() != 0 && pih) {
        printf(format_args!(
            "{}: couldn't establish all interrupts:{}{}{}\n",
            name,
            if eih { " event" } else { "" },
            if gih { " gerr" } else { "" },
            if pih { " pri" } else { "" }
        ));
    }

    Ok(())
}
/* </CODE> */
