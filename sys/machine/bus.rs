/* <LICENSES> */
/*
 * Copyright (c) 2026 Emilio Navarrete Lineros <enavarre@outlook.com>
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
//! `<machine/bus.h>` as a trait: the `bus_space(9)` methods a machine-independent driver uses to
//! reach its registers.
//!
//! Each architecture defines what a tag and a handle are (amd64 distinguishes port and memory
//! space, arm64 maps everything), the map flags it understands, and the accessors. The free
//! functions below carry the C names, so a driver reads like its C original; they dispatch to the
//! selected [`Machine`].
//!
//! The `bus_space_map` method is `unsafe`: the caller asserts that the range is a device this
//! driver owns, which is what the firmware tables (ACPI, FDT) or a configured address guarantee
//! in OpenBSD. Reads and writes through a handle returned by it are safe.
//!
//! `bus_dma(9)` (M7b) is the [`BusDma`] trait: the tag, map and segment types are the
//! architecture's (`struct bus_dma_tag`, `struct bus_dmamap`, `bus_dma_segment_t` of its
//! `<machine/bus.h>`), and the free functions with the C names dispatch through the tag's
//! function table, as the C macros do. A machine-independent driver reads the public members
//! of a map and a segment by name, exactly as in C: `map.dm_mapsize`, `map.dm_nsegs`,
//! `map.dm_segs()`, `seg.ds_addr`, `seg.ds_len`. Every architecture defines them with these
//! names and types; [`bus_dma_public_members`] makes the compiler check it. The flags whose
//! values differ between architectures (`BUS_DMA_COHERENT`, `BUS_DMASYNC_*`, ...) come from
//! the trait's constants.
//!
//! Loading a map is `unsafe`: the caller vouches that the memory stays allocated and mapped
//! until `bus_dmamap_unload`, because `bus_dmamap_sync` may copy to and from it (bounce
//! buffers) or clean the caches over it; that is the C's contract too.

use core::cell::Cell;
use core::ptr::NonNull;

use crate::machine::Machine;
use crate::sys::errno::Errno;
use crate::sys::mbuf::Mbuf;
use crate::sys::proc::Proc;
use crate::sys::types::{Off, Paddr};
use crate::sys::uio::Uio;

/// `bus_addr_t`: an address on the bus.
pub type BusAddr = usize;
/// `bus_size_t`: a size or offset on the bus.
pub type BusSize = usize;

/// `BUS_SPACE_BARRIER_READ`: force a read barrier.
pub const BUS_SPACE_BARRIER_READ: u32 = 0x01;
/// `BUS_SPACE_BARRIER_WRITE`: force a write barrier.
pub const BUS_SPACE_BARRIER_WRITE: u32 = 0x02;

/// `bus_space_tag_t`: the selected architecture's tag type.
pub type BusSpaceTag = <Machine as BusSpace>::Tag;
/// `bus_space_handle_t`: the selected architecture's handle type.
pub type BusSpaceHandle = <Machine as BusSpace>::Handle;

/// The `bus_space(9)` access methods.
pub trait BusSpace {
    /// `bus_space_tag_t`: which bus space (port I/O, memory, a particular bus).
    type Tag: Copy;
    /// `bus_space_handle_t`: a mapped region, as returned by `bus_space_map`.
    type Handle: Copy;

    /// `BUS_SPACE_MAP_CACHEABLE`: the region may be cached.
    const BUS_SPACE_MAP_CACHEABLE: u32;
    /// `BUS_SPACE_MAP_LINEAR`: the region must be linear (`bus_space_vaddr` works on it).
    const BUS_SPACE_MAP_LINEAR: u32;
    /// `BUS_SPACE_MAP_PREFETCHABLE`: the region may be prefetched.
    const BUS_SPACE_MAP_PREFETCHABLE: u32;

    /// `bus_space_map`: maps `size` bytes at `addr` in the space `t` names.
    ///
    /// # Safety
    ///
    /// `[addr, addr + size)` must be a device region this driver owns, as the firmware or the
    /// configured console address guarantees; mapping arbitrary memory as a device is undefined.
    unsafe fn bus_space_map(
        t: Self::Tag,
        addr: BusAddr,
        size: BusSize,
        flags: u32,
    ) -> Result<Self::Handle, Errno>;

    /// `bus_space_unmap`: releases a mapping made by `bus_space_map`.
    fn bus_space_unmap(t: Self::Tag, h: Self::Handle, size: BusSize);

    /// `bus_space_subregion`: a handle for `[offset, offset + size)` of the mapping `h`.
    fn bus_space_subregion(
        t: Self::Tag,
        h: Self::Handle,
        offset: BusSize,
        size: BusSize,
    ) -> Result<Self::Handle, Errno>;

    /// `bus_space_vaddr`: the kernel virtual address of a mapping made with
    /// `BUS_SPACE_MAP_LINEAR` (null for a space that has none, such as amd64's I/O ports).
    fn bus_space_vaddr(t: Self::Tag, h: Self::Handle) -> *mut u8;

    /// `bus_space_read_1`.
    fn bus_space_read_1(t: Self::Tag, h: Self::Handle, offset: BusSize) -> u8;
    /// `bus_space_read_2`.
    fn bus_space_read_2(t: Self::Tag, h: Self::Handle, offset: BusSize) -> u16;
    /// `bus_space_read_4`.
    fn bus_space_read_4(t: Self::Tag, h: Self::Handle, offset: BusSize) -> u32;
    /// `bus_space_write_1`.
    fn bus_space_write_1(t: Self::Tag, h: Self::Handle, offset: BusSize, value: u8);
    /// `bus_space_write_2`.
    fn bus_space_write_2(t: Self::Tag, h: Self::Handle, offset: BusSize, value: u16);
    /// `bus_space_write_4`.
    fn bus_space_write_4(t: Self::Tag, h: Self::Handle, offset: BusSize, value: u32);

    /// `bus_space_copy_2`: copies `count` 2-byte locations from `o1` of `h1` to `o2` of `h2`
    /// (the architecture decides how overlapping ranges are handled: amd64 copies as a move,
    /// arm64 always forward, as their C does).
    fn bus_space_copy_2(
        t: Self::Tag,
        h1: Self::Handle,
        o1: BusSize,
        h2: Self::Handle,
        o2: BusSize,
        count: usize,
    );

    /// `bus_space_barrier`: orders accesses to `[offset, offset + length)` of `h` according to
    /// `flags` ([`BUS_SPACE_BARRIER_READ`], [`BUS_SPACE_BARRIER_WRITE`]).
    fn bus_space_barrier(
        t: Self::Tag,
        h: Self::Handle,
        offset: BusSize,
        length: BusSize,
        flags: u32,
    );
}

/// `bus_space_map(9)` on the selected machine.
///
/// # Safety
///
/// As for [`BusSpace::bus_space_map`].
pub unsafe fn bus_space_map(
    t: BusSpaceTag,
    addr: BusAddr,
    size: BusSize,
    flags: u32,
) -> Result<BusSpaceHandle, Errno> {
    // SAFETY: forwarded.
    unsafe { Machine::bus_space_map(t, addr, size, flags) }
}

/// `bus_space_unmap(9)` on the selected machine.
pub fn bus_space_unmap(t: BusSpaceTag, h: BusSpaceHandle, size: BusSize) {
    Machine::bus_space_unmap(t, h, size)
}

/// `bus_space_subregion(9)` on the selected machine.
pub fn bus_space_subregion(
    t: BusSpaceTag,
    h: BusSpaceHandle,
    offset: BusSize,
    size: BusSize,
) -> Result<BusSpaceHandle, Errno> {
    Machine::bus_space_subregion(t, h, offset, size)
}

/// `bus_space_vaddr(9)` on the selected machine.
pub fn bus_space_vaddr(t: BusSpaceTag, h: BusSpaceHandle) -> *mut u8 {
    Machine::bus_space_vaddr(t, h)
}

/// `bus_space_read_1(9)`.
pub fn bus_space_read_1(t: BusSpaceTag, h: BusSpaceHandle, offset: BusSize) -> u8 {
    Machine::bus_space_read_1(t, h, offset)
}

/// `bus_space_read_2(9)`.
pub fn bus_space_read_2(t: BusSpaceTag, h: BusSpaceHandle, offset: BusSize) -> u16 {
    Machine::bus_space_read_2(t, h, offset)
}

/// `bus_space_read_4(9)`.
pub fn bus_space_read_4(t: BusSpaceTag, h: BusSpaceHandle, offset: BusSize) -> u32 {
    Machine::bus_space_read_4(t, h, offset)
}

/// `bus_space_write_1(9)`.
pub fn bus_space_write_1(t: BusSpaceTag, h: BusSpaceHandle, offset: BusSize, value: u8) {
    Machine::bus_space_write_1(t, h, offset, value)
}

/// `bus_space_write_2(9)`.
pub fn bus_space_write_2(t: BusSpaceTag, h: BusSpaceHandle, offset: BusSize, value: u16) {
    Machine::bus_space_write_2(t, h, offset, value)
}

/// `bus_space_write_4(9)`.
pub fn bus_space_write_4(t: BusSpaceTag, h: BusSpaceHandle, offset: BusSize, value: u32) {
    Machine::bus_space_write_4(t, h, offset, value)
}

/// `bus_space_write_region_4(9)`: writes the words of `values` to consecutive 4-byte
/// locations from `offset`. Every architecture's region write is its 4-byte write at each
/// address in turn (amd64's `x86_bus_space_{io,mem}_write_region_4`, arm64's inline
/// `bus_space_write_region_4` in its `<machine/bus.h>`), so it is written here once, over
/// [`bus_space_write_4`].
pub fn bus_space_write_region_4(
    t: BusSpaceTag,
    h: BusSpaceHandle,
    offset: BusSize,
    values: &[u32],
) {
    for (i, &v) in values.iter().enumerate() {
        bus_space_write_4(t, h, offset + i * 4, v);
    }
}

/// `bus_space_set_region_4(9)`: writes `value` to `count` consecutive 4-byte locations from
/// `offset`, as [`bus_space_write_region_4`] does.
pub fn bus_space_set_region_4(
    t: BusSpaceTag,
    h: BusSpaceHandle,
    offset: BusSize,
    value: u32,
    count: usize,
) {
    for i in 0..count {
        bus_space_write_4(t, h, offset + i * 4, value);
    }
}

/// `bus_space_read_region_2(9)`: reads consecutive 2-byte locations from `offset` into
/// `values`, one [`bus_space_read_2`] per address, as every architecture's region read does
/// (amd64's `x86_bus_space_{io,mem}_read_region_2`, arm64's inline one). The C's `u_int16_t *`
/// buffer is a slice of cells, which a backing store shared with other code already is; a
/// caller with a `&mut [u16]` passes `Cell::from_mut(buf).as_slice_of_cells()`.
pub fn bus_space_read_region_2(
    t: BusSpaceTag,
    h: BusSpaceHandle,
    offset: BusSize,
    values: &[Cell<u16>],
) {
    for (i, v) in values.iter().enumerate() {
        v.set(bus_space_read_2(t, h, offset + i * 2));
    }
}

/// `bus_space_write_region_2(9)`: writes `values` to consecutive 2-byte locations from
/// `offset`, as [`bus_space_read_region_2`] reads them.
pub fn bus_space_write_region_2(
    t: BusSpaceTag,
    h: BusSpaceHandle,
    offset: BusSize,
    values: &[Cell<u16>],
) {
    for (i, v) in values.iter().enumerate() {
        bus_space_write_2(t, h, offset + i * 2, v.get());
    }
}

/// `bus_space_set_region_2(9)`: writes `value` to `count` consecutive 2-byte locations from
/// `offset`, as [`bus_space_set_region_4`] does.
pub fn bus_space_set_region_2(
    t: BusSpaceTag,
    h: BusSpaceHandle,
    offset: BusSize,
    value: u16,
    count: usize,
) {
    for i in 0..count {
        bus_space_write_2(t, h, offset + i * 2, value);
    }
}

/// `bus_space_read_multi_1(9)`: reads `values.len()` bytes from the one location `offset`
/// (a FIFO port), one [`bus_space_read_1`] each, as `rep insb` does on amd64 and the inline
/// loop of arm64's `<machine/bus.h>`.
pub fn bus_space_read_multi_1(
    t: BusSpaceTag,
    h: BusSpaceHandle,
    offset: BusSize,
    values: &mut [u8],
) {
    for v in values.iter_mut() {
        *v = bus_space_read_1(t, h, offset);
    }
}

/// `bus_space_write_multi_1(9)`: writes `values` to the one location `offset`, one
/// [`bus_space_write_1`] each (`rep outsb` on amd64).
pub fn bus_space_write_multi_1(t: BusSpaceTag, h: BusSpaceHandle, offset: BusSize, values: &[u8]) {
    for &v in values {
        bus_space_write_1(t, h, offset, v);
    }
}

/// `bus_space_read_raw_multi_2(9)`: reads `values.len() / 2` 2-byte words from the one
/// location `offset` into `values` in the bus's byte order, without swapping (the "raw"
/// variants move bytes, so on a little-endian host a word lands low byte first).
pub fn bus_space_read_raw_multi_2(
    t: BusSpaceTag,
    h: BusSpaceHandle,
    offset: BusSize,
    values: &mut [u8],
) {
    for w in values.as_chunks_mut::<2>().0.iter_mut() {
        w.copy_from_slice(&bus_space_read_2(t, h, offset).to_ne_bytes());
    }
}

/// `bus_space_write_raw_multi_2(9)`: writes the 2-byte words of `values` to the one location
/// `offset`, as [`bus_space_read_raw_multi_2`] reads them.
pub fn bus_space_write_raw_multi_2(
    t: BusSpaceTag,
    h: BusSpaceHandle,
    offset: BusSize,
    values: &[u8],
) {
    for w in values.as_chunks::<2>().0.iter() {
        bus_space_write_2(t, h, offset, u16::from_ne_bytes(*w));
    }
}

/// `bus_space_copy_2(9)` on the selected machine.
pub fn bus_space_copy_2(
    t: BusSpaceTag,
    h1: BusSpaceHandle,
    o1: BusSize,
    h2: BusSpaceHandle,
    o2: BusSize,
    count: usize,
) {
    Machine::bus_space_copy_2(t, h1, o1, h2, o2, count)
}

/// `bus_space_barrier(9)`.
pub fn bus_space_barrier(
    t: BusSpaceTag,
    h: BusSpaceHandle,
    offset: BusSize,
    length: BusSize,
    flags: u32,
) {
    Machine::bus_space_barrier(t, h, offset, length, flags)
}

/// `bus_dma_tag_t`: the selected architecture's DMA tag (a pointer to its method table).
pub type BusDmaTag = <Machine as BusDma>::DmaTag;
/// `struct bus_dmamap`: what a `bus_dmamap_t` points at.
pub type BusDmamap = <Machine as BusDma>::Dmamap;
/// `bus_dma_segment_t`: one contiguous DMA transaction.
pub type BusDmaSegment = <Machine as BusDma>::DmaSegment;

/// `BUS_DMA_WAITOK`: safe to sleep (pseudo-flag).
pub const BUS_DMA_WAITOK: i32 = <Machine as BusDma>::BUS_DMA_WAITOK;
/// `BUS_DMA_NOWAIT`: not safe to sleep.
pub const BUS_DMA_NOWAIT: i32 = <Machine as BusDma>::BUS_DMA_NOWAIT;
/// `BUS_DMA_ALLOCNOW`: perform resource allocation now.
pub const BUS_DMA_ALLOCNOW: i32 = <Machine as BusDma>::BUS_DMA_ALLOCNOW;
/// `BUS_DMA_COHERENT`: hint: map memory DMA coherent.
pub const BUS_DMA_COHERENT: i32 = <Machine as BusDma>::BUS_DMA_COHERENT;
/// `BUS_DMA_BUS1`: placeholder for bus functions.
pub const BUS_DMA_BUS1: i32 = <Machine as BusDma>::BUS_DMA_BUS1;
/// `BUS_DMA_BUS2`: placeholder for bus functions.
pub const BUS_DMA_BUS2: i32 = <Machine as BusDma>::BUS_DMA_BUS2;
/// `BUS_DMA_STREAMING`: hint: sequential, unidirectional.
pub const BUS_DMA_STREAMING: i32 = <Machine as BusDma>::BUS_DMA_STREAMING;
/// `BUS_DMA_READ`: mapping is device -> memory only.
pub const BUS_DMA_READ: i32 = <Machine as BusDma>::BUS_DMA_READ;
/// `BUS_DMA_WRITE`: mapping is memory -> device only.
pub const BUS_DMA_WRITE: i32 = <Machine as BusDma>::BUS_DMA_WRITE;
/// `BUS_DMA_NOCACHE`: map memory uncached.
pub const BUS_DMA_NOCACHE: i32 = <Machine as BusDma>::BUS_DMA_NOCACHE;
/// `BUS_DMA_ZERO`: zero memory in dmamem_alloc.
pub const BUS_DMA_ZERO: i32 = <Machine as BusDma>::BUS_DMA_ZERO;
/// `BUS_DMA_64BIT`: device handles 64bit dva.
pub const BUS_DMA_64BIT: i32 = <Machine as BusDma>::BUS_DMA_64BIT;

/// `BUS_DMASYNC_PREREAD`: before the device writes to memory.
pub const BUS_DMASYNC_PREREAD: i32 = <Machine as BusDma>::BUS_DMASYNC_PREREAD;
/// `BUS_DMASYNC_POSTREAD`: after the device wrote to memory.
pub const BUS_DMASYNC_POSTREAD: i32 = <Machine as BusDma>::BUS_DMASYNC_POSTREAD;
/// `BUS_DMASYNC_PREWRITE`: before the device reads from memory.
pub const BUS_DMASYNC_PREWRITE: i32 = <Machine as BusDma>::BUS_DMASYNC_PREWRITE;
/// `BUS_DMASYNC_POSTWRITE`: after the device read from memory.
pub const BUS_DMASYNC_POSTWRITE: i32 = <Machine as BusDma>::BUS_DMASYNC_POSTWRITE;

/// The `bus_dma(9)` operations: the macros of `<machine/bus.h>` that call through
/// `struct bus_dma_tag`.
pub trait BusDma {
    /// `bus_dma_tag_t`.
    type DmaTag: Copy + 'static;
    /// `struct bus_dmamap`; a `bus_dmamap_t` is a `&'static` to one.
    type Dmamap: 'static;
    /// `bus_dma_segment_t`.
    type DmaSegment: Copy + Default + 'static;

    /// `BUS_DMA_WAITOK`.
    const BUS_DMA_WAITOK: i32;
    /// `BUS_DMA_NOWAIT`.
    const BUS_DMA_NOWAIT: i32;
    /// `BUS_DMA_ALLOCNOW`.
    const BUS_DMA_ALLOCNOW: i32;
    /// `BUS_DMA_COHERENT`.
    const BUS_DMA_COHERENT: i32;
    /// `BUS_DMA_BUS1`.
    const BUS_DMA_BUS1: i32;
    /// `BUS_DMA_BUS2`.
    const BUS_DMA_BUS2: i32;
    /// `BUS_DMA_STREAMING`.
    const BUS_DMA_STREAMING: i32;
    /// `BUS_DMA_READ`.
    const BUS_DMA_READ: i32;
    /// `BUS_DMA_WRITE`.
    const BUS_DMA_WRITE: i32;
    /// `BUS_DMA_NOCACHE`.
    const BUS_DMA_NOCACHE: i32;
    /// `BUS_DMA_ZERO`.
    const BUS_DMA_ZERO: i32;
    /// `BUS_DMA_64BIT`.
    const BUS_DMA_64BIT: i32;
    /// `BUS_DMASYNC_PREREAD`.
    const BUS_DMASYNC_PREREAD: i32;
    /// `BUS_DMASYNC_POSTREAD`.
    const BUS_DMASYNC_POSTREAD: i32;
    /// `BUS_DMASYNC_PREWRITE`.
    const BUS_DMASYNC_PREWRITE: i32;
    /// `BUS_DMASYNC_POSTWRITE`.
    const BUS_DMASYNC_POSTWRITE: i32;

    /// `bus_dmamap_create(t, size, nsegments, maxsegsz, boundary, flags, &map)`: a map for
    /// transfers of up to `size` bytes in up to `nsegments` segments of up to `maxsegsz`
    /// bytes, none crossing a `boundary` (0: none).
    fn bus_dmamap_create(
        t: Self::DmaTag,
        size: BusSize,
        nsegments: i32,
        maxsegsz: BusSize,
        boundary: BusSize,
        flags: i32,
    ) -> Result<&'static Self::Dmamap, Errno>;

    /// `bus_dmamap_destroy(t, map)`: frees a map.
    ///
    /// # Safety
    ///
    /// `map` came from `bus_dmamap_create` on `t` and nothing uses it afterwards.
    unsafe fn bus_dmamap_destroy(t: Self::DmaTag, map: NonNull<Self::Dmamap>);

    /// `bus_dmamap_load(t, map, buf, buflen, p, flags)`: loads a linear buffer, in `p`'s
    /// address space when `p` is given, in the kernel's otherwise.
    ///
    /// # Safety
    ///
    /// `[buf, buf + buflen)` is mapped in that address space and stays allocated until
    /// `bus_dmamap_unload`.
    unsafe fn bus_dmamap_load(
        t: Self::DmaTag,
        map: &Self::Dmamap,
        buf: *mut u8,
        buflen: BusSize,
        p: Option<&Proc>,
        flags: i32,
    ) -> Result<(), Errno>;

    /// `bus_dmamap_load_mbuf(t, map, m, flags)`: loads a packet's mbuf chain.
    ///
    /// # Safety
    ///
    /// The chain's data stays allocated until `bus_dmamap_unload`.
    unsafe fn bus_dmamap_load_mbuf(
        t: Self::DmaTag,
        map: &Self::Dmamap,
        m: &Mbuf,
        flags: i32,
    ) -> Result<(), Errno>;

    /// `bus_dmamap_load_uio(t, map, uio, flags)`: loads the buffers a `struct uio` describes.
    ///
    /// # Safety
    ///
    /// As for [`BusDma::bus_dmamap_load`], for every iovec.
    unsafe fn bus_dmamap_load_uio(
        t: Self::DmaTag,
        map: &Self::Dmamap,
        uio: &Uio<'_>,
        flags: i32,
    ) -> Result<(), Errno>;

    /// `bus_dmamap_load_raw(t, map, segs, nsegs, size, flags)`: loads memory allocated with
    /// `bus_dmamem_alloc` (`segs.len()` is the C's `nsegs`).
    ///
    /// # Safety
    ///
    /// The segments stay allocated (not `bus_dmamem_free`d) until `bus_dmamap_unload`.
    unsafe fn bus_dmamap_load_raw(
        t: Self::DmaTag,
        map: &Self::Dmamap,
        segs: &[Self::DmaSegment],
        size: BusSize,
        flags: i32,
    ) -> Result<(), Errno>;

    /// `bus_dmamap_unload(t, map)`: the map no longer describes a transfer.
    fn bus_dmamap_unload(t: Self::DmaTag, map: &Self::Dmamap);

    /// `bus_dmamap_sync(t, map, offset, len, ops)`: makes `[offset, offset + len)` of the
    /// loaded memory consistent for the `BUS_DMASYNC_*` operations in `ops`.
    fn bus_dmamap_sync(
        t: Self::DmaTag,
        map: &Self::Dmamap,
        offset: BusAddr,
        len: BusSize,
        ops: i32,
    );

    /// `bus_dmamem_alloc(t, size, alignment, boundary, segs, nsegs, &rsegs, flags)`: DMA-safe
    /// memory in up to `segs.len()` segments; returns how many were used (`rsegs`).
    fn bus_dmamem_alloc(
        t: Self::DmaTag,
        size: BusSize,
        alignment: BusSize,
        boundary: BusSize,
        segs: &mut [Self::DmaSegment],
        flags: i32,
    ) -> Result<usize, Errno>;

    /// `bus_dmamem_alloc_range(t, ..., low, high)`: as `bus_dmamem_alloc`, inside the
    /// physical range `[low, high]`.
    #[allow(clippy::too_many_arguments)] // the C's signature
    fn bus_dmamem_alloc_range(
        t: Self::DmaTag,
        size: BusSize,
        alignment: BusSize,
        boundary: BusSize,
        segs: &mut [Self::DmaSegment],
        flags: i32,
        low: BusAddr,
        high: BusAddr,
    ) -> Result<usize, Errno>;

    /// `bus_dmamem_free(t, segs, nsegs)`: gives memory from `bus_dmamem_alloc` back.
    ///
    /// # Safety
    ///
    /// The segments came from `bus_dmamem_alloc` on `t`, are unmapped and unloaded, and are
    /// not used afterwards.
    unsafe fn bus_dmamem_free(t: Self::DmaTag, segs: &[Self::DmaSegment]);

    /// `bus_dmamem_map(t, segs, nsegs, size, &kva, flags)`: maps the segments into kernel
    /// virtual space; returns the address.
    fn bus_dmamem_map(
        t: Self::DmaTag,
        segs: &mut [Self::DmaSegment],
        size: usize,
        flags: i32,
    ) -> Result<NonNull<u8>, Errno>;

    /// `bus_dmamem_unmap(t, kva, size)`: undoes `bus_dmamem_map`.
    ///
    /// # Safety
    ///
    /// `kva` and `size` came from `bus_dmamem_map` on `t` and nothing uses the mapping
    /// afterwards.
    unsafe fn bus_dmamem_unmap(t: Self::DmaTag, kva: NonNull<u8>, size: usize);

    /// `bus_dmamem_mmap(t, segs, nsegs, off, prot, flags)`: the physical address (with the
    /// pmap flags) of the page at `off` for `mmap(2)`; `None` for the C's `-1`.
    fn bus_dmamem_mmap(
        t: Self::DmaTag,
        segs: &[Self::DmaSegment],
        off: Off,
        prot: i32,
        flags: i32,
    ) -> Option<Paddr>;
}

/// `bus_dmamap_create(9)` on the selected machine.
pub fn bus_dmamap_create(
    t: BusDmaTag,
    size: BusSize,
    nsegments: i32,
    maxsegsz: BusSize,
    boundary: BusSize,
    flags: i32,
) -> Result<&'static BusDmamap, Errno> {
    Machine::bus_dmamap_create(t, size, nsegments, maxsegsz, boundary, flags)
}

/// `bus_dmamap_destroy(9)` on the selected machine.
///
/// # Safety
///
/// As for [`BusDma::bus_dmamap_destroy`].
pub unsafe fn bus_dmamap_destroy(t: BusDmaTag, map: NonNull<BusDmamap>) {
    // SAFETY: forwarded.
    unsafe { Machine::bus_dmamap_destroy(t, map) }
}

/// `bus_dmamap_load(9)` on the selected machine.
///
/// # Safety
///
/// As for [`BusDma::bus_dmamap_load`].
pub unsafe fn bus_dmamap_load(
    t: BusDmaTag,
    map: &BusDmamap,
    buf: *mut u8,
    buflen: BusSize,
    p: Option<&Proc>,
    flags: i32,
) -> Result<(), Errno> {
    // SAFETY: forwarded.
    unsafe { Machine::bus_dmamap_load(t, map, buf, buflen, p, flags) }
}

/// `bus_dmamap_load_mbuf(9)` on the selected machine.
///
/// # Safety
///
/// As for [`BusDma::bus_dmamap_load_mbuf`].
pub unsafe fn bus_dmamap_load_mbuf(
    t: BusDmaTag,
    map: &BusDmamap,
    m: &Mbuf,
    flags: i32,
) -> Result<(), Errno> {
    // SAFETY: forwarded.
    unsafe { Machine::bus_dmamap_load_mbuf(t, map, m, flags) }
}

/// `bus_dmamap_load_uio(9)` on the selected machine.
///
/// # Safety
///
/// As for [`BusDma::bus_dmamap_load_uio`].
pub unsafe fn bus_dmamap_load_uio(
    t: BusDmaTag,
    map: &BusDmamap,
    uio: &Uio<'_>,
    flags: i32,
) -> Result<(), Errno> {
    // SAFETY: forwarded.
    unsafe { Machine::bus_dmamap_load_uio(t, map, uio, flags) }
}

/// `bus_dmamap_load_raw(9)` on the selected machine.
///
/// # Safety
///
/// As for [`BusDma::bus_dmamap_load_raw`].
pub unsafe fn bus_dmamap_load_raw(
    t: BusDmaTag,
    map: &BusDmamap,
    segs: &[BusDmaSegment],
    size: BusSize,
    flags: i32,
) -> Result<(), Errno> {
    // SAFETY: forwarded.
    unsafe { Machine::bus_dmamap_load_raw(t, map, segs, size, flags) }
}

/// `bus_dmamap_unload(9)` on the selected machine.
pub fn bus_dmamap_unload(t: BusDmaTag, map: &BusDmamap) {
    Machine::bus_dmamap_unload(t, map)
}

/// `bus_dmamap_sync(9)` on the selected machine.
pub fn bus_dmamap_sync(t: BusDmaTag, map: &BusDmamap, offset: BusAddr, len: BusSize, ops: i32) {
    Machine::bus_dmamap_sync(t, map, offset, len, ops)
}

/// `bus_dmamem_alloc(9)` on the selected machine.
pub fn bus_dmamem_alloc(
    t: BusDmaTag,
    size: BusSize,
    alignment: BusSize,
    boundary: BusSize,
    segs: &mut [BusDmaSegment],
    flags: i32,
) -> Result<usize, Errno> {
    Machine::bus_dmamem_alloc(t, size, alignment, boundary, segs, flags)
}

/// `bus_dmamem_alloc_range(9)` on the selected machine.
#[allow(clippy::too_many_arguments)] // the C's signature
pub fn bus_dmamem_alloc_range(
    t: BusDmaTag,
    size: BusSize,
    alignment: BusSize,
    boundary: BusSize,
    segs: &mut [BusDmaSegment],
    flags: i32,
    low: BusAddr,
    high: BusAddr,
) -> Result<usize, Errno> {
    Machine::bus_dmamem_alloc_range(t, size, alignment, boundary, segs, flags, low, high)
}

/// `bus_dmamem_free(9)` on the selected machine.
///
/// # Safety
///
/// As for [`BusDma::bus_dmamem_free`].
pub unsafe fn bus_dmamem_free(t: BusDmaTag, segs: &[BusDmaSegment]) {
    // SAFETY: forwarded.
    unsafe { Machine::bus_dmamem_free(t, segs) }
}

/// `bus_dmamem_map(9)` on the selected machine.
pub fn bus_dmamem_map(
    t: BusDmaTag,
    segs: &mut [BusDmaSegment],
    size: usize,
    flags: i32,
) -> Result<NonNull<u8>, Errno> {
    Machine::bus_dmamem_map(t, segs, size, flags)
}

/// `bus_dmamem_unmap(9)` on the selected machine.
///
/// # Safety
///
/// As for [`BusDma::bus_dmamem_unmap`].
pub unsafe fn bus_dmamem_unmap(t: BusDmaTag, kva: NonNull<u8>, size: usize) {
    // SAFETY: forwarded.
    unsafe { Machine::bus_dmamem_unmap(t, kva, size) }
}

/// `bus_dmamem_mmap(9)` on the selected machine.
pub fn bus_dmamem_mmap(
    t: BusDmaTag,
    segs: &[BusDmaSegment],
    off: Off,
    prot: i32,
    flags: i32,
) -> Option<Paddr> {
    Machine::bus_dmamem_mmap(t, segs, off, prot, flags)
}

/// The public members machine-independent code reads, by the names `bus_dma(9)` gives them:
/// `(dm_mapsize, dm_nsegs, dm_segs[0].ds_addr, dm_segs[0].ds_len)`. Compiling it for every
/// architecture checks that each defines them.
pub fn bus_dma_public_members(map: &BusDmamap) -> (BusSize, i32, BusAddr, BusSize) {
    let first: BusDmaSegment = map.dm_segs().first().map(|s| s.get()).unwrap_or_default();
    (
        map.dm_mapsize.get(),
        map.dm_nsegs.get(),
        first.ds_addr,
        first.ds_len,
    )
}
/* </CODE> */
