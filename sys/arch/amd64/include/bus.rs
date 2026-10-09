/* <LICENSES> */
/*-
 * Copyright (c) 1996, 1997 The NetBSD Foundation, Inc.
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
 * Copyright (c) 1996 Charles M. Hannum.  All rights reserved.
 * Copyright (c) 1996 Jason R. Thorpe.  All rights reserved.
 * Copyright (c) 1996 Christopher G. Demetriou.  All rights reserved.
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
 *	This product includes software developed by Christopher G. Demetriou
 *	for the NetBSD Project.
 * 4. The name of the author may not be used to endorse or promote products
 *    derived from this software without specific prior written permission
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
//! amd64 `<machine/bus.h>`: the `bus_dma(9)` types and flags.
//!
//! Upstream: sys/arch/amd64/include/bus.h @ 3ce1f3f79392
//!
//! Status: `wip`. Milestone M7b ports the `bus_dma` half: the `BUS_DMA_*` and
//! `BUS_DMASYNC_*` flags, `bus_dma_segment_t`, `struct bus_dma_tag` (the method table the
//! `bus_dma*` macros call through) and `struct bus_dmamap`; the functions it declares
//! (`bus_dma_init`, `_bus_dmamap_*`, `_bus_dmamem_*`) are `amd64/bus_dma.rs`. The
//! `bus_space` half (the address types, `struct x86_bus_space_ops`, the `BUS_SPACE_MAP_*`
//! flags and `bus_space_barrier`) still lives in `amd64/bus_space.rs` and the machine
//! contract, where milestone M2 put it.
//!
//! ## Deviations
//! - `bus_dma_tag_t` is `&'static BusDmaTag`; the table holds Rust `fn` pointers whose
//!   signatures return `Result` and take slices where the C passes a pointer and a count
//!   (`segs`/`nsegs`) or an out pointer (`dmamp`, `rsegs`, `kvap`). The loads are `unsafe
//!   fn`, as in the machine contract.
//! - A `bus_dmamap_t` is `&'static BusDmamap`. The C ends the map with the variable-length
//!   `dm_segs[1]` and, for bounce buffers, the page pointers after it; here the map header is
//!   followed in the same allocation by `_dm_segcnt` segments and `_dm_npages` page pointers,
//!   which [`BusDmamap::dm_segs`] and `_dm_pages` reach through pointers taken from the
//!   allocation (a Rust struct cannot end in an unsized array of `Cell`s). The members the
//!   load, unload and sync functions change after creation are `Cell`s, and so is
//!   `_dm_cookie`, which a tag's own create function sets on the map the common one made.
//! - `vaddr_t` members that hold `-1` for "none" (`_ds_va`, `_ds_bounce_va`) are `usize` with
//!   `usize::MAX`.

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

/// `BUS_DMA_WAITOK`: safe to sleep (pseudo-flag).
pub const BUS_DMA_WAITOK: i32 = 0x0000;
/// `BUS_DMA_NOWAIT`: not safe to sleep.
pub const BUS_DMA_NOWAIT: i32 = 0x0001;
/// `BUS_DMA_ALLOCNOW`: perform resource allocation now.
pub const BUS_DMA_ALLOCNOW: i32 = 0x0002;
/// `BUS_DMA_COHERENT`: hint: map memory DMA coherent.
pub const BUS_DMA_COHERENT: i32 = 0x0004;
/// `BUS_DMA_BUS1`: placeholders for bus functions...
pub const BUS_DMA_BUS1: i32 = 0x0010;
/// `BUS_DMA_BUS2`.
pub const BUS_DMA_BUS2: i32 = 0x0020;
/// `BUS_DMA_32BIT`.
pub const BUS_DMA_32BIT: i32 = 0x0040;
/// `BUS_DMA_24BIT`: isadma map.
pub const BUS_DMA_24BIT: i32 = 0x0080;
/// `BUS_DMA_STREAMING`: hint: sequential, unidirectional.
pub const BUS_DMA_STREAMING: i32 = 0x0100;
/// `BUS_DMA_READ`: mapping is device -> memory only.
pub const BUS_DMA_READ: i32 = 0x0200;
/// `BUS_DMA_WRITE`: mapping is memory -> device only.
pub const BUS_DMA_WRITE: i32 = 0x0400;
/// `BUS_DMA_NOCACHE`: map memory uncached.
pub const BUS_DMA_NOCACHE: i32 = 0x0800;
/// `BUS_DMA_ZERO`: zero memory in dmamem_alloc.
pub const BUS_DMA_ZERO: i32 = 0x1000;
/// `BUS_DMA_64BIT`: device handles 64bit dva.
pub const BUS_DMA_64BIT: i32 = 0x2000;

/// `BUS_DMASYNC_PREREAD`: operation performed by `bus_dmamap_sync()`.
pub const BUS_DMASYNC_PREREAD: i32 = 0x01;
/// `BUS_DMASYNC_POSTREAD`.
pub const BUS_DMASYNC_POSTREAD: i32 = 0x02;
/// `BUS_DMASYNC_PREWRITE`.
pub const BUS_DMASYNC_PREWRITE: i32 = 0x04;
/// `BUS_DMASYNC_POSTWRITE`.
pub const BUS_DMASYNC_POSTWRITE: i32 = 0x08;

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
    /// `_ds_va`: mapped loaded data (`usize::MAX`: none).
    pub _ds_va: usize,
    /// `_ds_bounce_va`: mapped bounced data (`usize::MAX`: none).
    pub _ds_bounce_va: usize,
    /// `_ds_boundary`: don't cross. Ugh: needed so the alignment can be passed down from
    /// `bus_dmamem_alloc` to scatter gather maps; only the first segment's is used.
    pub _ds_boundary: BusSize,
    /// `_ds_align`: align to me.
    pub _ds_align: BusSize,
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
pub struct BusDmaTag {
    /// `_cookie`: cookie used in the guts.
    pub _cookie: *mut c_void,
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
}

// SAFETY: a tag is a table of functions written once at compile time; `_cookie` is only
// read, by the tag's own functions.
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
    /// `_dm_flags`: misc. flags.
    pub _dm_flags: i32,
    /// `_dm_segcnt`: number of segs this map can map.
    pub _dm_segcnt: i32,
    /// `_dm_maxsegsz`: largest possible segment.
    pub _dm_maxsegsz: BusSize,
    /// `_dm_boundary`: don't cross this.
    pub _dm_boundary: BusSize,
    /// `_dm_cookie`: cookie for bus-specific functions (a `Cell`: a tag's create function
    /// sets it after `_bus_dmamap_create` handed the map out, acpidmar's `dmar_dmamap_create`).
    pub _dm_cookie: Cell<*mut c_void>,
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
        // SAFETY: `_bus_dmamap_create` allocated `_dm_segcnt` initialised segments at
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
        // `_bus_dmamap_create` to a page it allocated and owns until destroy.
        unsafe { &**self._dm_pages.add(i) }
    }
}
/* </CODE> */
