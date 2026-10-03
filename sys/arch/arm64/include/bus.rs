/* $OpenBSD: bus.h,v 1.13 2026/06/22 07:54:19 deraadt Exp $ */
/* <LICENSES> */
/*
 * Copyright (c) 2003-2004 Opsycon AB Sweden.  All rights reserved.
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

//! arm64 `<machine/bus.h>`: the bus access methods as a table of functions.
//!
//! Upstream: sys/arch/arm64/include/bus.h @ 3ce1f3f79392
//!
//! Status: `wip`. Milestone M2 ports `struct bus_space` with the single-register accessors,
//! map, unmap, subregion and vaddr, the `BUS_SPACE_MAP_*` flags and `bus_space_barrier`. The
//! raw-multi accessors, `_space_mmap` and `bus_private` arrive with the drivers that use them.
//! M7b adds `bus_dma`: the `BUS_DMA_*` and `BUS_DMASYNC_*` flags, `bus_dma_segment_t`,
//! `struct bus_dma_tag` and `struct bus_dmamap`; their functions are `arm64/bus_dma.rs`.
//!
//! ## Deviations
//! - A tag is a `&'static BusSpace` (C: `bus_space_tag_t`, a pointer to the table).
//! - `_space_map` is an `unsafe fn`, as in the machine contract.
//! - `bus_dma_tag_t` is `&'static BusDmaTag`, a table of Rust `fn` pointers that return
//!   `Result` and take slices for the C's pointer-and-count pairs; the loads are `unsafe fn`,
//!   as in the machine contract. `_dmamap_load_buffer` keeps the C's in/out state as `&mut`.
//! - A `bus_dmamap_t` is `&'static BusDmamap`, whose header is followed in the same
//!   allocation by its segments and bounce-page pointers (the C's trailing `dm_segs[1]`),
//!   reached through pointers taken from the allocation; members that change after creation
//!   are `Cell`s. `vaddr_t` members holding `-1` are `usize` with `usize::MAX`.

use core::arch::asm;
use core::cell::Cell;
use core::ffi::c_void;
use core::ptr::NonNull;
use core::slice;

use crate::kassert;
use crate::machine::bus::{BusAddr, BusSize};
use crate::sys::errno::Errno;
use crate::sys::mbuf::Mbuf;
use crate::sys::proc::Proc;
use crate::sys::types::{Off, Paddr};
use crate::sys::uio::Uio;
use crate::uvm::uvm_page::VmPage;

/// `BUS_SPACE_MAP_CACHEABLE`.
pub const BUS_SPACE_MAP_CACHEABLE: u32 = 0x01;
/// `BUS_SPACE_MAP_POSTED`: device memory with posted writes (nGnRE).
pub const BUS_SPACE_MAP_POSTED: u32 = 0x02;
/// `BUS_SPACE_MAP_LINEAR`.
pub const BUS_SPACE_MAP_LINEAR: u32 = 0x04;
/// `BUS_SPACE_MAP_PREFETCHABLE`.
pub const BUS_SPACE_MAP_PREFETCHABLE: u32 = 0x08;

/// `bus_space_handle_t`: the kernel virtual address of a mapped region, only ever produced by
/// `_space_map` or `_space_subregion`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct BusSpaceHandle(pub(in crate::arch::arm64) usize);

/// `struct bus_space` (`bus_space_t`): one bus's access methods.
pub struct BusSpace {
    /// `bus_base`: the bus's base address.
    pub bus_base: BusAddr,
    /// `_space_read_1`.
    pub _space_read_1: fn(&'static BusSpace, BusSpaceHandle, BusSize) -> u8,
    /// `_space_write_1`.
    pub _space_write_1: fn(&'static BusSpace, BusSpaceHandle, BusSize, u8),
    /// `_space_read_2`.
    pub _space_read_2: fn(&'static BusSpace, BusSpaceHandle, BusSize) -> u16,
    /// `_space_write_2`.
    pub _space_write_2: fn(&'static BusSpace, BusSpaceHandle, BusSize, u16),
    /// `_space_read_4`.
    pub _space_read_4: fn(&'static BusSpace, BusSpaceHandle, BusSize) -> u32,
    /// `_space_write_4`.
    pub _space_write_4: fn(&'static BusSpace, BusSpaceHandle, BusSize, u32),
    /// `_space_read_8`.
    pub _space_read_8: fn(&'static BusSpace, BusSpaceHandle, BusSize) -> u64,
    /// `_space_write_8`.
    pub _space_write_8: fn(&'static BusSpace, BusSpaceHandle, BusSize, u64),
    /// `_space_map`.
    pub _space_map:
        unsafe fn(&'static BusSpace, BusAddr, BusSize, u32) -> Result<BusSpaceHandle, Errno>,
    /// `_space_unmap`.
    pub _space_unmap: fn(&'static BusSpace, BusSpaceHandle, BusSize),
    /// `_space_subregion`.
    pub _space_subregion:
        fn(&'static BusSpace, BusSpaceHandle, BusSize, BusSize) -> Result<BusSpaceHandle, Errno>,
    /// `_space_vaddr`.
    pub _space_vaddr: fn(&'static BusSpace, BusSpaceHandle) -> *mut u8,
}

/// Two tags are equal when they are the same bus space: the C compares `bus_space_tag_t`
/// pointers (`com_attach_subr`'s `sc->sc_iot == comconsiot`).
impl PartialEq for BusSpace {
    fn eq(&self, other: &Self) -> bool {
        core::ptr::eq(self, other)
    }
}

/// `bus_space_barrier`: a full system barrier (`dsb sy`), whatever the flags.
pub fn bus_space_barrier(
    _t: &'static BusSpace,
    _h: BusSpaceHandle,
    _offset: BusSize,
    _length: BusSize,
    _flags: u32,
) {
    // SAFETY: a barrier orders memory accesses and touches nothing else.
    unsafe { asm!("dsb sy", options(nostack, preserves_flags)) };
}

/// `BUS_DMA_WAITOK`.
pub const BUS_DMA_WAITOK: i32 = 0x0000;
/// `BUS_DMA_NOWAIT`.
pub const BUS_DMA_NOWAIT: i32 = 0x0001;
/// `BUS_DMA_ALLOCNOW`.
pub const BUS_DMA_ALLOCNOW: i32 = 0x0002;
/// `BUS_DMA_COHERENT`.
pub const BUS_DMA_COHERENT: i32 = 0x0008;
/// `BUS_DMA_BUS1`: placeholders for bus functions...
pub const BUS_DMA_BUS1: i32 = 0x0010;
/// `BUS_DMA_BUS2`.
pub const BUS_DMA_BUS2: i32 = 0x0020;
/// `BUS_DMA_BUS3`.
pub const BUS_DMA_BUS3: i32 = 0x0040;
/// `BUS_DMA_BUS4`.
pub const BUS_DMA_BUS4: i32 = 0x0080;
/// `BUS_DMA_READ`: mapping is device -> memory only.
pub const BUS_DMA_READ: i32 = 0x0100;
/// `BUS_DMA_WRITE`: mapping is memory -> device only.
pub const BUS_DMA_WRITE: i32 = 0x0200;
/// `BUS_DMA_STREAMING`: hint: sequential, unidirectional.
pub const BUS_DMA_STREAMING: i32 = 0x0400;
/// `BUS_DMA_ZERO`: zero memory in dmamem_alloc.
pub const BUS_DMA_ZERO: i32 = 0x0800;
/// `BUS_DMA_NOCACHE`.
pub const BUS_DMA_NOCACHE: i32 = 0x1000;
/// `BUS_DMA_64BIT`: device handles 64bit dva.
pub const BUS_DMA_64BIT: i32 = 0x2000;
/// `BUS_DMA_FIXED`: place mapping at specified dva.
pub const BUS_DMA_FIXED: i32 = 0x4000;

/// `BUS_DMASYNC_POSTREAD`.
pub const BUS_DMASYNC_POSTREAD: i32 = 0x0001;
/// `BUS_DMASYNC_POSTWRITE`.
pub const BUS_DMASYNC_POSTWRITE: i32 = 0x0002;
/// `BUS_DMASYNC_PREREAD`.
pub const BUS_DMASYNC_PREREAD: i32 = 0x0004;
/// `BUS_DMASYNC_PREWRITE`.
pub const BUS_DMASYNC_PREWRITE: i32 = 0x0008;

/// `bus_dma_tag_t`.
pub type BusDmaTagT = &'static BusDmaTag;
/// `bus_dmamap_t`.
pub type BusDmamapT = &'static BusDmamap;

/// `bus_dma_segment_t`: describes a single contiguous DMA transaction. Values are suitable
/// for programming into DMA registers.
#[repr(C)]
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct BusDmaSegment {
    /// `ds_addr`: DMA address.
    pub ds_addr: BusAddr,
    /// `ds_len`: length of transfer.
    pub ds_len: BusSize,
    /// `_ds_paddr`: CPU address.
    pub _ds_paddr: usize,
    /// `_ds_vaddr`: CPU address.
    pub _ds_vaddr: usize,
    /// `_ds_bounce_va`: mapped bounced data (`usize::MAX`: none).
    pub _ds_bounce_va: usize,
}

/// `_dmamap_create`.
pub type DmamapCreateFn =
    fn(BusDmaTagT, BusSize, i32, BusSize, BusSize, i32) -> Result<BusDmamapT, Errno>;
/// `_dmamap_destroy`.
pub type DmamapDestroyFn = unsafe fn(BusDmaTagT, NonNull<BusDmamap>);
/// `_dmamap_load`.
pub type DmamapLoadFn =
    unsafe fn(BusDmaTagT, &BusDmamap, *mut u8, BusSize, Option<&Proc>, i32) -> Result<(), Errno>;
/// `_dmamap_load_mbuf`.
pub type DmamapLoadMbufFn = unsafe fn(BusDmaTagT, &BusDmamap, &Mbuf, i32) -> Result<(), Errno>;
/// `_dmamap_load_uio`.
pub type DmamapLoadUioFn = unsafe fn(BusDmaTagT, &BusDmamap, &Uio<'_>, i32) -> Result<(), Errno>;
/// `_dmamap_load_raw`.
pub type DmamapLoadRawFn =
    unsafe fn(BusDmaTagT, &BusDmamap, &[BusDmaSegment], BusSize, i32) -> Result<(), Errno>;
/// `_dmamap_load_buffer`: `(t, map, buf, buflen, p, flags, &lastaddr, &seg, &used,
/// &lastbounce, first)`.
pub type DmamapLoadBufferFn = unsafe fn(
    BusDmaTagT,
    &BusDmamap,
    usize,
    BusSize,
    Option<&Proc>,
    i32,
    &mut usize,
    &mut i32,
    &mut i32,
    &mut bool,
    bool,
) -> Result<(), Errno>;
/// `_dmamap_unload`.
pub type DmamapUnloadFn = fn(BusDmaTagT, &BusDmamap);
/// `_dmamap_sync`.
pub type DmamapSyncFn = fn(BusDmaTagT, &BusDmamap, BusAddr, BusSize, i32);
/// `_dmamem_alloc`.
pub type DmamemAllocFn =
    fn(BusDmaTagT, BusSize, BusSize, BusSize, &mut [BusDmaSegment], i32) -> Result<usize, Errno>;
/// `_dmamem_alloc_range`.
pub type DmamemAllocRangeFn = fn(
    BusDmaTagT,
    BusSize,
    BusSize,
    BusSize,
    &mut [BusDmaSegment],
    i32,
    BusAddr,
    BusAddr,
) -> Result<usize, Errno>;
/// `_dmamem_free`.
pub type DmamemFreeFn = unsafe fn(BusDmaTagT, &[BusDmaSegment]);
/// `_dmamem_map`.
pub type DmamemMapFn =
    fn(BusDmaTagT, &mut [BusDmaSegment], usize, i32) -> Result<NonNull<u8>, Errno>;
/// `_dmamem_unmap`.
pub type DmamemUnmapFn = unsafe fn(BusDmaTagT, NonNull<u8>, usize);
/// `_dmamem_mmap`.
pub type DmamemMmapFn = fn(BusDmaTagT, &[BusDmaSegment], Off, i32, i32) -> Option<Paddr>;

/// `struct bus_dma_tag`: a machine-dependent opaque type describing the implementation of
/// DMA for a given bus.
#[derive(Clone, Copy)]
pub struct BusDmaTag {
    /// `_cookie`: cookie used in the guts.
    pub _cookie: *mut c_void,
    /// `_flags`: misc. flags (`BUS_DMA_COHERENT` for a cache-coherent bus).
    pub _flags: i32,
    /// `_dmamap_create`.
    pub _dmamap_create: DmamapCreateFn,
    /// `_dmamap_destroy`.
    pub _dmamap_destroy: DmamapDestroyFn,
    /// `_dmamap_load`.
    pub _dmamap_load: DmamapLoadFn,
    /// `_dmamap_load_mbuf`.
    pub _dmamap_load_mbuf: DmamapLoadMbufFn,
    /// `_dmamap_load_uio`.
    pub _dmamap_load_uio: DmamapLoadUioFn,
    /// `_dmamap_load_raw`.
    pub _dmamap_load_raw: DmamapLoadRawFn,
    /// `_dmamap_load_buffer`.
    pub _dmamap_load_buffer: DmamapLoadBufferFn,
    /// `_dmamap_unload`.
    pub _dmamap_unload: DmamapUnloadFn,
    /// `_dmamap_sync`.
    pub _dmamap_sync: DmamapSyncFn,
    /// `_dmamem_alloc`.
    pub _dmamem_alloc: DmamemAllocFn,
    /// `_dmamem_alloc_range`.
    pub _dmamem_alloc_range: DmamemAllocRangeFn,
    /// `_dmamem_free`.
    pub _dmamem_free: DmamemFreeFn,
    /// `_dmamem_map`.
    pub _dmamem_map: DmamemMapFn,
    /// `_dmamem_unmap`.
    pub _dmamem_unmap: DmamemUnmapFn,
    /// `_dmamem_mmap`.
    pub _dmamem_mmap: DmamemMmapFn,
    /// `_dma_mask`: internal memory address translation information.
    pub _dma_mask: BusAddr,
}

// SAFETY: a tag is written once (statically, or by mainbus before a child sees its copy)
// and only read afterwards; `_cookie` is only read, by the tag's own functions.
unsafe impl Sync for BusDmaTag {}

/// `struct bus_dmamap`: describes a DMA mapping.
///
/// The private members are for the machine-dependent code; `dm_mapsize`, `dm_nsegs` and
/// [`dm_segs`](Self::dm_segs) are what machine-independent code uses. The scalar members
/// change only in the map's own functions, called by the one driver that owns the map.
#[repr(C)]
pub struct BusDmamap {
    /// `_dm_size`: largest DMA transfer mappable.
    pub _dm_size: BusSize,
    /// `_dm_segcnt`: number of segs this map can map.
    pub _dm_segcnt: i32,
    /// `_dm_maxsegsz`: largest possible segment.
    pub _dm_maxsegsz: BusSize,
    /// `_dm_boundary`: don't cross this.
    pub _dm_boundary: BusSize,
    /// `_dm_flags`: misc. flags.
    pub _dm_flags: i32,
    /// `_dm_cookie`: cookie for bus-specific functions.
    pub _dm_cookie: *mut c_void,
    /// `_dm_pages`: replacement pages (`_dm_npages` of them after the segments; null when
    /// the map does not bounce).
    pub _dm_pages: *mut *const VmPage,
    /// `_dm_pgva`: those above -- mapped (0: none).
    pub _dm_pgva: usize,
    /// `_dm_npages`: number of pages allocated.
    pub _dm_npages: i32,
    /// `_dm_nused`: number of pages replaced.
    pub _dm_nused: Cell<i32>,
    /// `dm_mapsize`: size of the mapping.
    pub dm_mapsize: Cell<BusSize>,
    /// `dm_nsegs`: # valid segments in mapping.
    pub dm_nsegs: Cell<i32>,
    /// `dm_segs`: the first of `_dm_segcnt` segments, right after the header.
    pub _dm_segs: NonNull<Cell<BusDmaSegment>>,
}

impl BusDmamap {
    /// `dm_segs`: the map's `_dm_segcnt` segments; the first `dm_nsegs` are valid.
    pub fn dm_segs(&self) -> &[Cell<BusDmaSegment>] {
        // SAFETY: `_dmamap_create` allocated `_dm_segcnt` initialised segments at
        // `_dm_segs`, in the map's own allocation, which lives as long as the map.
        unsafe { slice::from_raw_parts(self._dm_segs.as_ptr(), self._dm_segcnt as usize) }
    }

    /// `map->dm_segs[i]`.
    pub fn seg(&self, i: usize) -> &Cell<BusDmaSegment> {
        &self.dm_segs()[i]
    }

    /// `map->_dm_pages[i]`: the `i`th bounce page.
    pub fn dm_page(&self, i: usize) -> &'static VmPage {
        kassert!(i < self._dm_npages as usize);
        // SAFETY: a bouncing map has `_dm_npages` page pointers at `_dm_pages`, each set by
        // `_dmamap_create` to a page it allocated and owns until destroy.
        unsafe { &**self._dm_pages.add(i) }
    }
}
