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
//! The IORT (I/O Remapping Table) on arm64: `arch/arm64/dev/acpiiort.c` and its header
//! `acpiiort.h`. acpiiort0 attaches to the IORT table and offers each of its nodes to the
//! IOMMU drivers (`smmu* at acpiiort?`); those register themselves here, and the PCI host
//! bridges (`acpipci`) and ACPI devices (`acpi_iommu_device_map`) map their DMA through the
//! SMMU a requester ID or named component leads to.
//!
//! Upstream: sys/arch/arm64/dev/acpiiort.c @ 3ce1f3f79392
//! Upstream: sys/arch/arm64/dev/acpiiort.h @ 3ce1f3f79392
//!
//! The header's types (`struct acpiiort_attach_args`, `struct acpiiort_smmu`) live here
//! with the functions: the `.c` and the `.h` share a name and a directory, so they share the
//! module (a `ports.toml` entry each).
//!
//! The table is read where acpi0 copied it (`q_table`), in bounds: [`iort_read`] reads a
//! node, a mapping or a node's data by offset, and gives `None` past the table where the C
//! would read past it. `smmu* at acpiiort?` (`smmu_acpi.rs`, M16f) registers the SMMUv2
//! nodes it matches; at the pin it matches no SMMUv3 node, so on QEMU `virt` every map hands
//! the tag back.
//!
//! ## Deviations
//! - The `SIMPLEQ_FOREACH` over `acpi_softc->sc_tables` looking for `IORT_SIG` is
//!   [`acpiiort_table`], shared with `acpipci.c`'s two copies of it.
//! - `rid` in `acpiiort_device_map` starts at 0 (the C leaves it unset when a node has
//!   several mappings and none is single, a path that returns before reading it).

use core::ffi::c_void;
use core::ptr;

use crate::dev::acpi::acpi::q_table_bytes;
use crate::dev::acpi::acpireg::{
    ACPI_IORT_MAPPING_SINGLE, ACPI_IORT_NAMED_COMPONENT, ACPI_IORT_SMMU, ACPI_IORT_SMMU_V3,
    AcpiIort, AcpiIortItsNode, AcpiIortMapping, AcpiIortNcNode, AcpiIortNode, AcpiIortRcNode,
    IORT_SIG,
};
use crate::dev::acpi::acpivar::{AcpiAttachArgs, AcpiSoftc, acpi_softc};
use crate::dev::acpi::amltypes::AmlNodeRef;
use crate::dev::acpi::dsdt::aml_searchname;
use crate::kern::subr_autoconf::config_found;
use crate::kern::subr_prf::printf;
use crate::machine::bus::{BusAddr, BusDmaTag, BusSize, BusSpaceTag};
use crate::sys::device::{CfMatch, Cfattach, Cfdriver, DV_DULL, Device};
use crate::sys::queue::{SimpleqEntry, SimpleqHead};

/// The packed IORT structures [`iort_read`] may read: plain integers and arrays of them,
/// valid for any bytes.
///
/// # Safety
///
/// Every bit pattern of the implementing type is a valid value.
pub unsafe trait IortPlain: Copy {}

// SAFETY: `#[repr(C, packed)]` structures of integers and integer arrays only.
unsafe impl IortPlain for AcpiIort {}
// SAFETY: as above.
unsafe impl IortPlain for AcpiIortNode {}
// SAFETY: as above.
unsafe impl IortPlain for AcpiIortMapping {}
// SAFETY: as above.
unsafe impl IortPlain for AcpiIortRcNode {}
// SAFETY: as above.
unsafe impl IortPlain for AcpiIortNcNode {}
// SAFETY: as above.
unsafe impl IortPlain for AcpiIortItsNode {}
// SAFETY: an integer.
unsafe impl IortPlain for u32 {}

/// `struct acpiiort_attach_args`: what an IOMMU driver attaches with, one IORT node.
pub struct AcpiiortAttachArgs {
    /// `aia_node`: the node, in acpi0's copy of the IORT.
    pub aia_node: *const AcpiIortNode,
    /// `aia_iot`.
    pub aia_iot: Option<BusSpaceTag>,
    /// `aia_memt`.
    pub aia_memt: Option<BusSpaceTag>,
    /// `aia_dmat`.
    pub aia_dmat: Option<BusDmaTag>,
}

/// `struct acpiiort_smmu`: an IOMMU an `smmu` driver registers for its IORT node.
pub struct AcpiiortSmmu {
    /// `as_list`: link in `acpiiort_smmu_list`.
    pub as_list: SimpleqEntry<AcpiiortSmmu>,
    /// `as_node`: the SMMU's IORT node.
    pub as_node: *const AcpiIortNode,
    /// `as_cookie`: the driver's.
    pub as_cookie: *mut c_void,
    /// `as_map(cookie, rid, dmat)`: the DMA tag of the device with stream ID `rid`.
    pub as_map: fn(*mut c_void, u32, Option<BusDmaTag>) -> Option<BusDmaTag>,
    /// `as_reserve(cookie, rid, addr, size)`: keeps `[addr, addr + size)` out of the
    /// device's I/O virtual space.
    pub as_reserve: fn(*mut c_void, u32, BusAddr, BusSize),
}

crate::queue_adapter!(
    /// `SIMPLEQ_HEAD(, acpiiort_smmu)`: through `as_list`.
    pub AcpiiortSmmuList: AcpiiortSmmu, as_list => SimpleqEntry<AcpiiortSmmu>
);

/// `acpiiort_smmu_list`, behind a wrapper that can be a `static`.
pub struct AcpiiortSmmuHead(SimpleqHead<AcpiiortSmmuList>);

// SAFETY: the SMMU drivers register while autoconfiguring (kernel lock) and the list is only
// read afterwards, as the C's unlocked list.
unsafe impl Sync for AcpiiortSmmuHead {}

/// `acpiiort_smmu_list`.
pub static ACPIIORT_SMMU_LIST: AcpiiortSmmuHead = AcpiiortSmmuHead(SimpleqHead::new());

/// `acpiiort_ca`.
pub static ACPIIORT_CA: Cfattach = Cfattach {
    ca_devsize: size_of::<Device>(),
    ca_match: Some(acpiiort_match),
    ca_attach: acpiiort_attach,
    ca_detach: None,
    ca_activate: None,
};

/// `acpiiort_cd`.
pub static ACPIIORT_CD: Cfdriver = Cfdriver::new(b"acpiiort", DV_DULL, 0);

/// Reads a `T` at `offset` in the table `iort`; `None` when it does not fit.
pub fn iort_read<T: IortPlain>(iort: &[u8], offset: usize) -> Option<T> {
    let bytes = iort.get(offset..offset.checked_add(size_of::<T>())?)?;
    // SAFETY: `bytes` holds `size_of::<T>()` bytes, `read_unaligned` needs no alignment and
    // any bytes are a valid `T` (`IortPlain`).
    Some(unsafe { ptr::read_unaligned(bytes.as_ptr().cast::<T>()) })
}

/// The `i`th mapping of the IORT node at `offset`.
pub fn iort_mapping(
    iort: &[u8],
    offset: usize,
    node: &AcpiIortNode,
    i: usize,
) -> Option<AcpiIortMapping> {
    let base = offset.checked_add(node.mapping_offset as usize)?;
    iort_read(
        iort,
        base.checked_add(i.checked_mul(size_of::<AcpiIortMapping>())?)?,
    )
}

/// The IORT among acpi0's tables (`SIMPLEQ_FOREACH(entry, &sc->sc_tables, q_next)` with
/// `strncmp(hdr->signature, IORT_SIG, 4) == 0`).
pub fn acpiiort_table(sc: &AcpiSoftc) -> Option<&'static [u8]> {
    sc.sc_tables
        .iter()
        .map(q_table_bytes)
        .find(|t| t.starts_with(IORT_SIG))
}

/// `acpiiort_match(parent, match, aux)`: the IORT table.
pub fn acpiiort_match(_parent: Option<&Device>, _match: &CfMatch, aux: *mut c_void) -> i32 {
    // SAFETY: acpi0 hands its children `struct acpi_attach_args`.
    let aaa = unsafe { &*aux.cast_const().cast::<AcpiAttachArgs>() };

    // If we do not have a table, it is not us
    if aaa.aaa_table.is_null() {
        return 0;
    }

    // If it is an IORT table, we can attach
    // SAFETY: a table argument points at a table acpi0 loaded, at least a header long.
    let sig = unsafe { ptr::read_unaligned(aaa.aaa_table.cast_const().cast::<[u8; 4]>()) };
    i32::from(&sig == IORT_SIG)
}

/// `acpiiort_attach(parent, self, aux)`: offers each IORT node to the IOMMU drivers.
pub fn acpiiort_attach(_parent: Option<&Device>, self_: &Device, aux: *mut c_void) {
    // SAFETY: as in `acpiiort_match`.
    let aaa = unsafe { &*aux.cast_const().cast::<AcpiAttachArgs>() };
    let base = aaa.aaa_table.cast_const().cast::<u8>();
    // SAFETY: the table acpi0 loaded, as long as its header says, never freed.
    let len = unsafe { ptr::read_unaligned(base.cast::<AcpiIort>()) }
        .hdr
        .length as usize;
    // SAFETY: as above.
    let iort = unsafe { core::slice::from_raw_parts(base, len) };

    printf(format_args!("\n"));

    let mut aia = AcpiiortAttachArgs {
        aia_node: ptr::null(),
        aia_iot: aaa.aaa_iot,
        aia_memt: aaa.aaa_memt,
        aia_dmat: aaa.aaa_dmat,
    };

    let Some(hdr) = iort_read::<AcpiIort>(iort, 0) else {
        return;
    };
    let mut offset = hdr.offset as usize;
    for _ in 0..hdr.number_of_nodes {
        let Some(node) = iort_read::<AcpiIortNode>(iort, offset) else {
            return;
        };
        // In bounds: `iort_read` read the node there.
        aia.aia_node = iort[offset..].as_ptr().cast();
        let _ = config_found(self_, ptr::from_mut(&mut aia).cast(), None);
        offset += usize::from(node.length);
    }
}

/// `acpiiort_smmu_register(as)`: an IOMMU driver's registration.
///
/// # Safety
///
/// `as_` is in no list and stays valid (it is never unregistered).
pub unsafe fn acpiiort_smmu_register(as_: &'static AcpiiortSmmu) {
    // SAFETY: the caller's guarantee.
    unsafe { ACPIIORT_SMMU_LIST.0.insert_tail(as_) };
}

/// `acpiiort_smmu_map(node, rid, dmat)`: the tag of stream `rid` behind the SMMU of `node`,
/// or `dmat` when no driver registered it.
pub fn acpiiort_smmu_map(
    node: *const AcpiIortNode,
    rid: u32,
    dmat: Option<BusDmaTag>,
) -> Option<BusDmaTag> {
    for as_ in ACPIIORT_SMMU_LIST.0.iter() {
        if ptr::eq(as_.as_node, node) {
            return (as_.as_map)(as_.as_cookie, rid, dmat);
        }
    }

    dmat
}

/// `acpiiort_smmu_reserve_region(node, rid, addr, size)`.
pub fn acpiiort_smmu_reserve_region(
    node: *const AcpiIortNode,
    rid: u32,
    addr: BusAddr,
    size: BusSize,
) {
    for as_ in ACPIIORT_SMMU_LIST.0.iter() {
        if ptr::eq(as_.as_node, node) {
            (as_.as_reserve)(as_.as_cookie, rid, addr, size);
            return;
        }
    }
}

/// `acpiiort_device_map(root, dmat)`: the DMA tag of the ACPI device `root` through the SMMU
/// its IORT named component maps to; `dmat` otherwise.
pub fn acpiiort_device_map(root: &AmlNodeRef, dmat: Option<BusDmaTag>) -> Option<BusDmaTag> {
    let Some(sc) = acpi_softc() else {
        return dmat;
    };

    // Look for IORT table.
    let Some(iort) = acpiiort_table(sc) else {
        return dmat;
    };
    let Some(hdr) = iort_read::<AcpiIort>(iort, 0) else {
        return dmat;
    };

    // Find our named component.
    let mut offset = hdr.offset as usize;
    let mut found = None;
    for _ in 0..hdr.number_of_nodes {
        let Some(node) = iort_read::<AcpiIortNode>(iort, offset) else {
            return dmat;
        };
        if node.r#type == ACPI_IORT_NAMED_COMPONENT {
            // The name follows the named component's fixed part, NUL-terminated.
            let name_at = offset + size_of::<AcpiIortNode>() + size_of::<AcpiIortNcNode>();
            let name = iort.get(name_at..).unwrap_or(&[]);
            let name = &name[..name.iter().position(|&c| c == 0).unwrap_or(name.len())];
            let anc = aml_searchname(sc.sc_root.borrow().as_ref(), name);
            if anc.is_some_and(|anc| AmlNodeRef::ptr_eq(&anc, root)) {
                found = Some((offset, node));
                break;
            }
        }
        offset += usize::from(node.length);
    }

    // No NC found? Weird.
    let Some((noffset, node)) = found else {
        return dmat;
    };

    // Find our output base towards SMMU.
    let mut rid: u32 = 0;
    let mut out = None;
    for i in 0..node.number_of_mappings as usize {
        let Some(map) = iort_mapping(iort, noffset, &node, i) else {
            return dmat;
        };
        if map.flags & ACPI_IORT_MAPPING_SINGLE != 0 {
            rid = map.output_base;
            out = Some(map.output_reference);
            break;
        }
    }

    // The IORT spec allows NCs to use implementation-defined IDs, whose interpretation is
    // up to the device driver. For now simply take the mapping if there's a single one.
    // This might change in the future.
    if out.is_none()
        && node.number_of_mappings == 1
        && let Some(map) = iort_mapping(iort, noffset, &node, 0)
    {
        rid = map.output_base;
        out = Some(map.output_reference);
    }

    // No mapping found? Even weirder.
    let Some(offset) = out else {
        return dmat;
    };

    let offset = offset as usize;
    let Some(node) = iort_read::<AcpiIortNode>(iort, offset) else {
        return dmat;
    };
    if node.r#type == ACPI_IORT_SMMU || node.r#type == ACPI_IORT_SMMU_V3 {
        return acpiiort_smmu_map(iort[offset..].as_ptr().cast(), rid, dmat);
    }

    dmat
}
/* </CODE> */
