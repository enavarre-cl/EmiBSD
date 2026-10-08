/*	$OpenBSD: ofw_misc.h,v 1.32 2026/09/05 20:36:56 kettenis Exp $	*/
/*	$OpenBSD: ofw_misc.c,v 1.45 2026/09/05 20:36:56 kettenis Exp $	*/
/* <LICENSES> */
/*
 * Copyright (c) 2017-2021 Mark Kettenis
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
 * Copyright (c) 2017-2021 Mark Kettenis
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
//! Miscellaneous device-tree frameworks: `dev/ofw/ofw_misc.c` and its header `ofw_misc.h`.
//! Drivers register a provider (a register map, an IOMMU, ...) for their node, and the
//! consumers find it by node, phandle or `compatible` string.
//!
//! Upstream: sys/dev/ofw/ofw_misc.c @ 3ce1f3f79392
//! Upstream: sys/dev/ofw/ofw_misc.h @ 3ce1f3f79392
//!
//! The header's types live here with the functions: the `.c` and the `.h` share a name and a
//! directory, so they share the module (a `ports.toml` entry each).
//!
//! Ported so far (M16f, what the SMMU needs, and the register maps):
//! - register maps: `struct regmap`, `regmap_register`, `regmap_bycompatible`,
//!   `regmap_bynode`, `regmap_byphandle`, `regmap_read_4`, `regmap_write_4`;
//! - IOMMU support: `struct iommu_device`, `iommu_device_register`, `iommu_device_do_map`,
//!   `iommu_device_lookup_idx`, `iommu_device_lookup`, `iommu_device_lookup_pci`,
//!   `iommu_device_map_idx`, `iommu_device_map`, `iommu_device_mirror_idx`,
//!   `iommu_device_map_pci`, `iommu_device_do_reserve`, `iommu_reserve_region_pci`.
//!
//! An IOMMU driver (`smmu(4)`) registers an [`IommuDevice`] for its node; a device whose
//! `iommus` property, or a PCI function whose requester ID in its host bridge's `iommu-map`,
//! names that node gets the DMA tag the driver hands back, and a host bridge reserves its
//! windows in the function's I/O virtual space.
//!
//! ## Deviations
//! - Left for the boards that use them (no driver of QEMU `virt` does): the interface
//!   (`if_register`, `if_bynode`, `if_byphandle`), PHY (`phy_*`), I2C (`i2c_*`), SFP
//!   (`sfp_*`), PWM (`pwm_*`), NVMEM (`nvmem_*`), port/endpoint and DAI (`device_port*`,
//!   `endpoint_*`, `dai_*`), MII (`mii_*`), mailbox (`mbox_*`) and hardware lock (`hwlock_*`)
//!   frameworks and their header types. Nothing calls them yet.
//! - The lookups return `Option<(phandle, cells)>` where the C returns 0 or 1 and fills
//!   `*phandle` and `cells[]`; the property walks are [`iommus_lookup`] and
//!   [`iommu_map_lookup`], over the property's cells, so the host tests drive them.
//! - At most two IOMMU cells are copied (the C's `KASSERT(icells <= 2)`): without
//!   `DIAGNOSTIC` the C would write past its two-cell array.
//! - `id_mirror` is an `Option`: `smmu_fdt.c` registers no mirror function, so the C would
//!   call through a NULL pointer; here such an IOMMU hands the tag back unchanged.
//! - The provider lists are never emptied (the C has no unregister either); the registered
//!   structures are `&'static`.

use alloc::boxed::Box;
use alloc::vec;
use core::ffi::c_void;

use crate::dev::ofw::fdt::{
    OF_getnodebyphandle, OF_getpropint, OF_getpropintarray, OF_getproplen, OF_is_compatible,
};
use crate::kassert;
use crate::machine::bus::{
    BusAddr, BusDmaTag, BusSize, BusSpaceHandle, BusSpaceTag, bus_space_read_4, bus_space_write_4,
};
use crate::sys::queue::{ListEntry, ListHead};

/// `struct regmap`: a register window a driver shares with others (a syscon).
pub struct Regmap {
    /// `rm_node`.
    pub rm_node: i32,
    /// `rm_phandle`: the node's phandle, 0 when it has none.
    pub rm_phandle: u32,
    /// `rm_tag`.
    pub rm_tag: BusSpaceTag,
    /// `rm_handle`.
    pub rm_handle: BusSpaceHandle,
    /// `rm_size`.
    pub rm_size: BusSize,
    /// `rm_list`: link in `regmaps`.
    pub rm_list: ListEntry<Regmap>,
}

crate::queue_adapter!(
    /// `LIST_HEAD(, regmap)`: through `rm_list`.
    pub RegmapList: Regmap, rm_list => ListEntry<Regmap>
);

/// `bus_dma_tag_t (*)(void *, uint32_t *, bus_dma_tag_t)`: an IOMMU's `id_map` or
/// `id_mirror`.
pub type IommuMapFn = fn(*mut c_void, &[u32], BusDmaTag) -> BusDmaTag;

/// `struct iommu_device`: an IOMMU a driver registers for its node.
pub struct IommuDevice {
    /// `id_node`.
    pub id_node: i32,
    /// `id_cookie`: the driver's.
    pub id_cookie: *mut c_void,
    /// `id_map(cookie, cells, dmat)`: the DMA tag of the device the IOMMU cells name.
    pub id_map: IommuMapFn,
    /// `id_mirror(cookie, cells, dmat)`: a tag mirroring another device's mappings; `None`
    /// for the C's NULL.
    pub id_mirror: Option<IommuMapFn>,
    /// `id_reserve(cookie, cells, addr, size)`: keeps `[addr, addr + size)` out of the
    /// device's I/O virtual space.
    pub id_reserve: fn(*mut c_void, &[u32], BusAddr, BusSize),
    /// `id_list`: link in `iommu_devices`.
    pub id_list: ListEntry<IommuDevice>,
    /// `id_phandle`: the node's phandle, set by `iommu_device_register`.
    pub id_phandle: core::cell::Cell<u32>,
}

crate::queue_adapter!(
    /// `LIST_HEAD(, iommu_device)`: through `id_list`.
    pub IommuDeviceList: IommuDevice, id_list => ListEntry<IommuDevice>
);

/// A provider list that can be a `static`.
pub struct ProviderList<A: crate::sys::queue::ListAdapter>(ListHead<A>);

// SAFETY: the providers register while autoconfiguring (kernel lock) and the lists are read
// afterwards, as the C's unlocked lists; an element is never removed.
unsafe impl<A: crate::sys::queue::ListAdapter> Sync for ProviderList<A> {}

/// `regmaps`.
pub static REGMAPS: ProviderList<RegmapList> = ProviderList(ListHead::new());

/// `iommu_devices`.
pub static IOMMU_DEVICES: ProviderList<IommuDeviceList> = ProviderList(ListHead::new());

/// `regmap_register(node, tag, handle, size)`: offers the mapped window `handle` of `node`.
pub fn regmap_register(node: i32, tag: BusSpaceTag, handle: BusSpaceHandle, size: BusSize) {
    let rm: &'static Regmap = Box::leak(Box::new(Regmap {
        rm_node: node,
        rm_phandle: OF_getpropint(node, b"phandle", 0),
        rm_tag: tag,
        rm_handle: handle,
        rm_size: size,
        rm_list: ListEntry::new(),
    }));
    // SAFETY: a fresh element, in no list, never freed.
    unsafe { REGMAPS.0.insert_head(rm) };
}

/// `regmap_bycompatible(compatible)`.
pub fn regmap_bycompatible(compatible: &[u8]) -> Option<&'static Regmap> {
    REGMAPS
        .0
        .iter()
        .find(|rm| OF_is_compatible(rm.rm_node, compatible))
}

/// `regmap_bynode(node)`.
pub fn regmap_bynode(node: i32) -> Option<&'static Regmap> {
    REGMAPS.0.iter().find(|rm| rm.rm_node == node)
}

/// `regmap_byphandle(phandle)`.
pub fn regmap_byphandle(phandle: u32) -> Option<&'static Regmap> {
    if phandle == 0 {
        return None;
    }

    REGMAPS.0.iter().find(|rm| rm.rm_phandle == phandle)
}

/// `regmap_write_4(rm, offset, value)`.
pub fn regmap_write_4(rm: &Regmap, offset: BusSize, value: u32) {
    kassert!(offset <= rm.rm_size.wrapping_sub(size_of::<u32>()));
    bus_space_write_4(rm.rm_tag, rm.rm_handle, offset, value);
}

/// `regmap_read_4(rm, offset)`.
pub fn regmap_read_4(rm: &Regmap, offset: BusSize) -> u32 {
    kassert!(offset <= rm.rm_size.wrapping_sub(size_of::<u32>()));
    bus_space_read_4(rm.rm_tag, rm.rm_handle, offset)
}

/// `iommu_device_register(id)`: registers the IOMMU of `id.id_node`; one without a phandle
/// can be named by nobody and is left out.
///
/// # Safety
///
/// `id` is in no list (it was never registered).
pub unsafe fn iommu_device_register(id: &'static IommuDevice) {
    id.id_phandle.set(OF_getpropint(id.id_node, b"phandle", 0));
    if id.id_phandle.get() == 0 {
        return;
    }

    // SAFETY: the caller's guarantee; the element is `'static` and never removed.
    unsafe { IOMMU_DEVICES.0.insert_head(id) };
}

/// The IOMMU registered for `phandle`.
fn iommu_device_byphandle(phandle: u32) -> Option<&'static IommuDevice> {
    IOMMU_DEVICES
        .0
        .iter()
        .find(|id| id.id_phandle.get() == phandle)
}

/// `iommu_device_do_map(phandle, cells, dmat)`: the tag the IOMMU `phandle` gives the
/// device `cells` names, or `dmat` when no driver registered it.
pub fn iommu_device_do_map(phandle: u32, cells: &[u32], dmat: BusDmaTag) -> BusDmaTag {
    match iommu_device_byphandle(phandle) {
        Some(id) => (id.id_map)(id.id_cookie, cells, dmat),
        None => dmat,
    }
}

/// `#iommu-cells` of the IOMMU `phandle`, `None` when no node has that phandle.
fn iommu_cells_of(phandle: u32) -> Option<u32> {
    let node = OF_getnodebyphandle(phandle);
    if node == 0 {
        return None;
    }
    Some(OF_getpropint(node, b"#iommu-cells", 1))
}

/// A property of 32-bit cells, read whole; `None` when it is missing or empty.
fn getpropcells(node: i32, prop: &[u8]) -> Option<alloc::vec::Vec<u32>> {
    let len = OF_getproplen(node, prop);
    if len <= 0 {
        return None;
    }

    let mut map = vec![0u32; len as usize / size_of::<u32>()];
    OF_getpropintarray(node, prop, &mut map);
    Some(map)
}

/// The walk of `iommu_device_lookup_idx` over an `iommus` property `map`: the `idx`th
/// `<phandle cells...>` entry, its cell count from `icells_of(phandle)` (`None`: no such
/// node, the walk stops).
pub fn iommus_lookup(
    map: &[u32],
    mut idx: usize,
    icells_of: impl Fn(u32) -> Option<u32>,
) -> Option<(u32, [u32; 2])> {
    let mut cell = map;
    while cell.len() > 1 {
        let icells = icells_of(cell[0])? as usize;
        if cell.len() < icells + 1 {
            return None;
        }

        kassert!(icells <= 2);

        if idx == 0 {
            let mut cells = [0u32; 2];
            let n = icells.min(cells.len());
            cells[..n].copy_from_slice(&cell[1..1 + n]);
            return Some((cell[0], cells));
        }

        cell = &cell[1 + icells..];
        idx -= 1;
    }

    None
}

/// `iommu_device_lookup_idx(node, &phandle, cells, idx)`: the `idx`th IOMMU of `node`'s
/// `iommus` property and its cells.
pub fn iommu_device_lookup_idx(node: i32, idx: usize) -> Option<(u32, [u32; 2])> {
    let map = getpropcells(node, b"iommus")?;
    iommus_lookup(&map, idx, iommu_cells_of)
}

/// `iommu_device_lookup(node, &phandle, cells)`: the first IOMMU of `node`.
pub fn iommu_device_lookup(node: i32) -> Option<(u32, [u32; 2])> {
    iommu_device_lookup_idx(node, 0)
}

/// The walk of `iommu_device_lookup_pci` over an `iommu-map` property `map` of
/// `<rid-base iommu-phandle iommu-base length>` entries: the IOMMU and stream ID of
/// requester ID `rid` once `iommu-map-mask` `mask` is applied; `icells_of` as in
/// [`iommus_lookup`].
pub fn iommu_map_lookup(
    map: &[u32],
    rid: u32,
    mask: u32,
    icells_of: impl Fn(u32) -> Option<u32>,
) -> Option<(u32, [u32; 2])> {
    let rid = rid & mask;

    for entry in map.as_chunks::<4>().0 {
        let icells = icells_of(entry[1])?;
        kassert!(icells <= 2);
        let _ = icells;

        let rid_base = entry[0];
        let sid_base = entry[2];
        let length = entry[3];
        if rid >= rid_base && rid < rid_base.wrapping_add(length) {
            return Some((entry[1], [sid_base.wrapping_add(rid - rid_base), 0]));
        }
    }

    None
}

/// `iommu_device_lookup_pci(node, rid, &phandle, cells)`: the IOMMU and stream ID the host
/// bridge `node`'s `iommu-map` gives requester ID `rid`.
pub fn iommu_device_lookup_pci(node: i32, rid: u32) -> Option<(u32, [u32; 2])> {
    let map = getpropcells(node, b"iommu-map")?;
    let mask = OF_getpropint(node, b"iommu-map-mask", 0xffff);
    iommu_map_lookup(&map, rid, mask, iommu_cells_of)
}

/// `iommu_device_map_idx(node, dmat, idx)`: `node`'s DMA tag through its `idx`th IOMMU.
pub fn iommu_device_map_idx(node: i32, dmat: BusDmaTag, idx: usize) -> BusDmaTag {
    match iommu_device_lookup_idx(node, idx) {
        Some((phandle, cells)) => iommu_device_do_map(phandle, &cells, dmat),
        None => dmat,
    }
}

/// `iommu_device_map(node, dmat)`: `node`'s DMA tag through its first IOMMU, or `dmat`.
pub fn iommu_device_map(node: i32, dmat: BusDmaTag) -> BusDmaTag {
    iommu_device_map_idx(node, dmat, 0)
}

/// `iommu_device_mirror_idx(node, dmat, idx)`: a tag mirroring the mappings of `node`'s
/// `idx`th IOMMU stream.
pub fn iommu_device_mirror_idx(node: i32, dmat: BusDmaTag, idx: usize) -> BusDmaTag {
    let Some((phandle, cells)) = iommu_device_lookup_idx(node, idx) else {
        return dmat;
    };

    match iommu_device_byphandle(phandle) {
        Some(id) => match id.id_mirror {
            Some(mirror) => mirror(id.id_cookie, &cells, dmat),
            None => dmat,
        },
        None => dmat,
    }
}

/// `iommu_device_map_pci(node, rid, dmat)`: the DMA tag of the PCI function `rid` below the
/// host bridge `node`, or `dmat`.
pub fn iommu_device_map_pci(node: i32, rid: u32, dmat: BusDmaTag) -> BusDmaTag {
    match iommu_device_lookup_pci(node, rid) {
        Some((phandle, cells)) => iommu_device_do_map(phandle, &cells, dmat),
        None => dmat,
    }
}

/// `iommu_device_do_reserve(phandle, cells, addr, size)`.
pub fn iommu_device_do_reserve(phandle: u32, cells: &[u32], addr: BusAddr, size: BusSize) {
    if phandle == 0 {
        return;
    }

    if let Some(id) = iommu_device_byphandle(phandle) {
        (id.id_reserve)(id.id_cookie, cells, addr, size);
    }
}

/// `iommu_reserve_region_pci(node, rid, addr, size)`: keeps `[addr, addr + size)` out of the
/// I/O virtual space of the PCI function `rid` below the host bridge `node`.
pub fn iommu_reserve_region_pci(node: i32, rid: u32, addr: BusAddr, size: BusSize) {
    if let Some((phandle, cells)) = iommu_device_lookup_pci(node, rid) {
        iommu_device_do_reserve(phandle, &cells, addr, size);
    }
}
/* </CODE> */

/* <TESTS> */
#[cfg(test)]
mod tests {
    use std::assert_eq;

    use super::*;

    /// Two IOMMUs: phandle 1 with one cell, phandle 2 with two; phandle 9 does not exist.
    fn icells(ph: u32) -> Option<u32> {
        match ph {
            1 => Some(1),
            2 => Some(2),
            _ => None,
        }
    }

    #[test]
    fn iommus_walks_entries_of_each_width() {
        let map = [1, 0x10, 2, 0x20, 0x21, 1, 0x30];
        assert_eq!(iommus_lookup(&map, 0, icells), Some((1, [0x10, 0])));
        assert_eq!(iommus_lookup(&map, 1, icells), Some((2, [0x20, 0x21])));
        assert_eq!(iommus_lookup(&map, 2, icells), Some((1, [0x30, 0])));
        assert_eq!(iommus_lookup(&map, 3, icells), None);
    }

    #[test]
    fn iommus_stops_at_unknown_or_truncated_entries() {
        assert_eq!(iommus_lookup(&[9, 0x10, 1, 0x20], 1, icells), None);
        assert_eq!(iommus_lookup(&[2, 0x20], 0, icells), None);
        assert_eq!(iommus_lookup(&[1], 0, icells), None);
        assert_eq!(iommus_lookup(&[], 0, icells), None);
    }

    #[test]
    fn iommu_map_translates_rids_in_range() {
        // QEMU virt with iommu=smmuv3: <0x0 &smmu 0x0 0x10000>.
        let map = [0, 1, 0, 0x10000];
        assert_eq!(
            iommu_map_lookup(&map, 0x08, 0xffff, icells),
            Some((1, [0x08, 0]))
        );
        // Two ranges, the second with an offset stream base.
        let map = [0, 1, 0x100, 0x10, 0x20, 2, 0x400, 0x8];
        assert_eq!(
            iommu_map_lookup(&map, 0x0f, 0xffff, icells),
            Some((1, [0x10f, 0]))
        );
        assert_eq!(iommu_map_lookup(&map, 0x10, 0xffff, icells), None);
        assert_eq!(
            iommu_map_lookup(&map, 0x23, 0xffff, icells),
            Some((2, [0x403, 0]))
        );
    }

    #[test]
    fn iommu_map_applies_the_mask() {
        let map = [0, 1, 0, 0x100];
        // Bus 1 masked away: only the devfn is kept.
        assert_eq!(
            iommu_map_lookup(&map, 0x108, 0xff, icells),
            Some((1, [0x08, 0]))
        );
        assert_eq!(iommu_map_lookup(&map, 0x108, 0xffff, icells), None);
    }

    #[test]
    fn iommu_map_stops_at_an_unknown_iommu() {
        let map = [0, 9, 0, 0x10, 0, 1, 0, 0x10];
        assert_eq!(iommu_map_lookup(&map, 0x1, 0xffff, icells), None);
    }
}
/* </TESTS> */
