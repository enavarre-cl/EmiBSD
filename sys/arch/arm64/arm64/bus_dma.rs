/*	$OpenBSD: bus_dma.c,v 1.18 2026/08/07 19:07:30 kettenis Exp $ */
/* <LICENSES> */
/*
 * Copyright (c) 2003-2004 Opsycon AB  (www.opsycon.se / www.opsycon.com)
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
 * THIS SOFTWARE IS PROVIDED BY THE AUTHOR ``AS IS'' AND ANY EXPRESS
 * OR IMPLIED WARRANTIES, INCLUDING, BUT NOT LIMITED TO, THE IMPLIED
 * WARRANTIES OF MERCHANTABILITY AND FITNESS FOR A PARTICULAR PURPOSE
 * ARE DISCLAIMED.  IN NO EVENT SHALL THE AUTHOR BE LIABLE FOR ANY
 * DIRECT, INDIRECT, INCIDENTAL, SPECIAL, EXEMPLARY, OR CONSEQUENTIAL
 * DAMAGES (INCLUDING, BUT NOT LIMITED TO, PROCUREMENT OF SUBSTITUTE GOODS
 * OR SERVICES; LOSS OF USE, DATA, OR PROFITS; OR BUSINESS INTERRUPTION)
 * HOWEVER CAUSED AND ON ANY THEORY OF LIABILITY, WHETHER IN CONTRACT, STRICT
 * LIABILITY, OR TORT (INCLUDING NEGLIGENCE OR OTHERWISE) ARISING IN ANY WAY
 * OUT OF THE USE OF THIS SOFTWARE, EVEN IF ADVISED OF THE POSSIBILITY OF
 * SUCH DAMAGE.
 *
 */
/*-
 * Copyright (c) 1996, 1997, 1998 The NetBSD Foundation, Inc.
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
/* </LICENSES> */

//! arm64 `bus_dma(9)`: the common DMA map and DMA memory functions every arm64 bus tag uses,
//! `arch/arm64/arm64/bus_dma.c`.
//!
//! Upstream: sys/arch/arm64/arm64/bus_dma.c @ 3ce1f3f79392
//!
//! A map translates a buffer (linear, an mbuf chain, a `uio`, or raw segments from
//! `bus_dmamem_alloc`) into physical segments, coalescing contiguous chunks and splitting at
//! `_dm_maxsegsz` and `_dm_boundary`. Unlike amd64, arm64 caches are not coherent with DMA
//! unless the tag says so (`BUS_DMA_COHERENT`, a `dma-coherent` device-tree node), so
//! `_dmamap_sync` cleans and invalidates the data cache over the segments; bounce pages below
//! `dma_constraint` are used when there is memory above it.
//!
//! ## Deviations
//! - The `NKSTAT` statistics (`bus_dma_kstat_copy` and the `mainbus0` "dma" kstat) need
//!   `kstat(4)` (`kern/subr_kstat.c`, not ported): the counters exist, the kstat is reported.
//! - The map's size counts the segments after the header (the C's `sizeof` includes
//!   `dm_segs[0]` and adds `nsegments - 1`); a map always has room for one segment, as the
//!   C's `dm_segs[1]` does, so `_dm_segcnt` is at least 1.
//! - Out pointers are return values (`_dmamap_create`, `_dmamem_alloc*`, `_dmamem_map`);
//!   `_dmamap_load_buffer` keeps the C's in/out state as `&mut`.
//! - `_dmamem_alloc_range` takes the pages off its local list before returning, so no page's
//!   queue entry points into a dead stack frame.
//! - `_dmamap_sync`'s "ran off map" panics with or without `DIAGNOSTIC`, as in C.
//! - `km_alloc(kv_any, kp_none)` (bounce-page and `bus_dmamem_map` virtual space) is reported
//!   by `uvm_km.rs` until it maps virtual-only space; those paths fail with `ENOMEM`, as the C
//!   does when `km_alloc` returns NULL. With the direct map (an arm64 deviation of this
//!   project, `docs/ARCHITECTURE.md`) drivers can still load buffers that live there.

use core::cell::Cell;
use core::ptr::{self, NonNull};
use core::sync::atomic::{AtomicUsize, Ordering, fence};

use crate::arch::arm64::arm64::cpufunc::{
    cpu_dcache_inv_range, cpu_dcache_wb_range, cpu_dcache_wbinv_range,
};
use crate::arch::arm64::arm64::machdep::DMA_CONSTRAINT;
use crate::arch::arm64::arm64::pmap::pmap_kenter_cache;
use crate::arch::arm64::include::bus::{
    BUS_DMA_64BIT, BUS_DMA_COHERENT, BUS_DMA_NOCACHE, BUS_DMA_NOWAIT, BUS_DMA_WAITOK, BUS_DMA_ZERO,
    BUS_DMASYNC_POSTREAD, BUS_DMASYNC_POSTWRITE, BUS_DMASYNC_PREREAD, BUS_DMASYNC_PREWRITE,
    BusDmaSegment, BusDmaTagT, BusDmamap, BusDmamapT,
};
use crate::arch::arm64::include::pmap::{PMAP_CACHE_CI, PMAP_CACHE_WB, PMAP_NOCACHE};
use crate::kern::kern_malloc::{free, malloc};
use crate::kern::subr_prf::panic;
use crate::machine::bus::{BusAddr, BusSize};
use crate::machine::pmap::{pmap_extract, pmap_kenter_pa, pmap_kernel, pmap_update};
use crate::sys::errno::Errno;
use crate::sys::malloc::{M_DEVBUF, M_NOWAIT, M_WAITOK, M_ZERO};
use crate::sys::mbuf::Mbuf;
use crate::sys::mman::{PROT_READ, PROT_WRITE};
use crate::sys::param::{NBPG, PAGE_MASK, PAGE_SIZE, PGOFSET, PGSHIFT};
use crate::sys::proc::Proc;
use crate::sys::types::{Off, Paddr, Vaddr};
use crate::sys::uio::{Uio, UioSeg::UIO_USERSPACE};
use crate::unported;
use crate::uvm::uvm_extern::{UVM_PLA_NOWAIT, UVM_PLA_WAITOK, UVM_PLA_ZERO, UvmConstraintRange};
use crate::uvm::uvm_km::{
    KD_TRYLOCK, KD_WAITOK, KP_NONE, KV_ANY, NO_CONSTRAINT, km_alloc, km_free,
};
use crate::uvm::uvm_page::{
    PHYS_TO_VM_PAGE, Pglist, VmPage, uvm_pagecount, uvm_pglistalloc, uvm_pglistfree,
    vm_page_to_phys,
};
use crate::uvm::uvm_param::round_page;
use crate::uvm::uvm_pmap::{PMAP_CANFAIL, PMAP_WIRED};

/// The `-1` of a `vaddr_t` that names no mapping.
const NO_VA: usize = usize::MAX;

/// `bus_dma_high_pages`: pages of memory above `dma_constraint`.
pub static BUS_DMA_HIGH_PAGES: AtomicUsize = AtomicUsize::new(0);
/// `bus_dma_bounce_pages`: bounce pages reserved by maps.
pub static BUS_DMA_BOUNCE_PAGES: AtomicUsize = AtomicUsize::new(0);
/// `bus_dma_bounces`: copies made to or from bounce pages.
pub static BUS_DMA_BOUNCES: AtomicUsize = AtomicUsize::new(0);

/// `bus_dma_init`: counts the memory outside `dma_constraint`, which decides whether maps
/// need bounce pages.
pub fn bus_dma_init() {
    let mut high_constraint = UvmConstraintRange {
        ucr_low: DMA_CONSTRAINT.ucr_high,
        ucr_high: NO_CONSTRAINT.ucr_high,
    };
    if high_constraint.ucr_low != high_constraint.ucr_high {
        high_constraint.ucr_low = Paddr::new(high_constraint.ucr_low.as_usize() + 1);
    }

    BUS_DMA_HIGH_PAGES.store(uvm_pagecount(&high_constraint), Ordering::Relaxed);

    // NKSTAT > 0: kstat_create("mainbus0", 0, "dma", ...) with bus_dma_kstat_copy.
    let _ = unported!("bus_dma_init: the mainbus0 dma kstat (subr_kstat.c)");
}

/// The bytes a map with `nsegments` segments and `npages` bounce pages takes.
fn dmamap_size(nsegments: usize, npages: usize) -> usize {
    size_of::<BusDmamap>()
        + size_of::<Cell<BusDmaSegment>>() * nsegments
        + size_of::<*const VmPage>() * npages
}

/// `_dmamap_create`: common function for DMA map creation. May be called by bus-specific
/// DMA map creation functions.
pub fn _dmamap_create(
    _t: BusDmaTagT,
    size: BusSize,
    nsegments: i32,
    maxsegsz: BusSize,
    boundary: BusSize,
    flags: i32,
) -> Result<BusDmamapT, Errno> {
    let mut constraint: &UvmConstraintRange = &NO_CONSTRAINT;
    let mut use_bounce_buffer = false;

    if BUS_DMA_HIGH_PAGES.load(Ordering::Relaxed) > 0 {
        use_bounce_buffer = true;
        constraint = &DMA_CONSTRAINT;
    }

    // Allocate and initialize the DMA map. The end of the map is a variable-sized array of
    // segments, so we allocate enough room for them in one shot.
    //
    // Note we don't preserve the WAITOK or NOWAIT flags. Preservation of ALLOCNOW notifies
    // others that we've reserved these resources, and they are not to be freed.
    //
    // The bus_dmamap_t includes one bus_dma_segment_t, hence at least one.
    let nsegs = nsegments.max(1) as usize;
    let mut npages = 0usize;
    if use_bounce_buffer {
        // this many pages plus one in case we get split
        npages = round_page(size) / PAGE_SIZE + 1;
        if npages < nsegs {
            npages = nsegs;
        }
        BUS_DMA_BOUNCE_PAGES.fetch_add(npages, Ordering::Relaxed);
    }
    let mapsize = dmamap_size(nsegs, npages);

    let mflags = if flags & BUS_DMA_NOWAIT != 0 {
        M_NOWAIT | M_ZERO
    } else {
        M_WAITOK | M_ZERO
    };
    let mapstore = malloc(mapsize, M_DEVBUF, mflags).ok_or(Errno::ENOMEM)?;

    let base = mapstore.as_ptr();
    // SAFETY: the segments start right after the header, inside the allocation.
    let segs = unsafe { base.add(size_of::<BusDmamap>()) }.cast::<Cell<BusDmaSegment>>();
    let pages = if use_bounce_buffer {
        // SAFETY: the page pointers follow the segments, inside the allocation.
        unsafe { segs.add(nsegs) }.cast::<*const VmPage>()
    } else {
        ptr::null_mut()
    };
    let map_ptr = mapstore.cast::<BusDmamap>();
    // SAFETY: a fresh allocation of `mapsize` bytes, aligned for the header, the segments
    // and the pointers (all word-aligned); each part is written before the map is shared.
    unsafe {
        for i in 0..nsegs {
            segs.add(i).write(Cell::new(BusDmaSegment::default()));
        }
        map_ptr.write(BusDmamap {
            _dm_size: size,
            _dm_segcnt: nsegs as i32,
            _dm_maxsegsz: maxsegsz,
            _dm_boundary: boundary,
            _dm_flags: flags & !(BUS_DMA_WAITOK | BUS_DMA_NOWAIT) & !BUS_DMA_64BIT, // XXX
            _dm_cookie: ptr::null_mut(),
            _dm_pages: pages,
            _dm_pgva: 0,
            _dm_npages: npages as i32,
            _dm_nused: Cell::new(0),
            dm_mapsize: Cell::new(0),
            dm_nsegs: Cell::new(0),
            _dm_segs: NonNull::new_unchecked(segs),
        });
    }
    // SAFETY: written above; the map lives until `_dmamap_destroy`.
    let map: &'static mut BusDmamap = unsafe { &mut *map_ptr.as_ptr() };

    if !use_bounce_buffer {
        return Ok(map);
    }

    let sz = npages << PGSHIFT;
    let kd = if flags & BUS_DMA_NOWAIT != 0 {
        &KD_TRYLOCK
    } else {
        &KD_WAITOK
    };
    let Some(kva) = km_alloc(sz, &KV_ANY, &KP_NONE, kd) else {
        map._dm_npages = 0;
        free(mapstore, M_DEVBUF, mapsize);
        return Err(Errno::ENOMEM);
    };

    let mlist = Pglist::new();
    mlist.init();
    let pla = if flags & BUS_DMA_NOWAIT != 0 {
        UVM_PLA_NOWAIT
    } else {
        UVM_PLA_WAITOK
    };
    if uvm_pglistalloc(
        sz,
        constraint.ucr_low,
        constraint.ucr_high,
        Paddr::new(PAGE_SIZE),
        Paddr::new(0),
        &mlist,
        nsegments,
        pla,
    )
    .is_err()
    {
        map._dm_npages = 0;
        km_free(kva, sz, &KV_ANY, &KP_NONE);
        free(mapstore, M_DEVBUF, mapsize);
        return Err(Errno::ENOMEM);
    }

    let sva = kva.as_ptr() as usize;
    let mut va = sva;
    for i in 0..npages {
        let Some(pg) = mlist.first() else {
            panic(format_args!("_dmamap_create: short page list"));
        };
        // SAFETY: `pg` is the head of `mlist`; the map keeps it from now on.
        unsafe { mlist.remove(pg) };
        // SAFETY: slot `i` of the `npages` page pointers.
        unsafe { pages.add(i).write(ptr::from_ref(pg)) };
        // SAFETY: `va` is in the virtual range km_alloc gave this map; the page is the map's.
        unsafe { pmap_kenter_pa(Vaddr::new(va), vm_page_to_phys(pg), PROT_READ | PROT_WRITE) };
        // SAFETY: just mapped to a page this map owns.
        unsafe { ptr::write_bytes(va as *mut u8, 0, PAGE_SIZE) };
        va += PAGE_SIZE;
    }
    pmap_update(pmap_kernel());
    map._dm_pgva = sva;

    Ok(map)
}

/// `_dmamap_destroy`: common function for DMA map destruction. May be called by
/// bus-specific DMA map destruction functions.
///
/// # Safety
///
/// `map` came from [`_dmamap_create`] and is not used afterwards.
pub unsafe fn _dmamap_destroy(_t: BusDmaTagT, map: NonNull<BusDmamap>) {
    // SAFETY: the caller's guarantee: a live map.
    let m = unsafe { map.as_ref() };

    // if (map->_dm_pgva): a null address is no mapping.
    if let Some(va) = NonNull::new(m._dm_pgva as *mut u8) {
        km_free(va, (m._dm_npages as usize) << PGSHIFT, &KV_ANY, &KP_NONE);
    }

    let mapsize = dmamap_size(m._dm_segcnt as usize, m._dm_npages as usize);

    if !m._dm_pages.is_null() {
        let mlist = Pglist::new();
        mlist.init();
        for i in 0..m._dm_npages as usize {
            // SAFETY: the map owns its bounce pages, which are on no list.
            unsafe { mlist.insert_tail(m.dm_page(i)) };
        }
        uvm_pglistfree(&mlist);
    }

    free(map.cast::<u8>(), M_DEVBUF, mapsize);
}

/// `_dmamap_load`: common function for loading a DMA map with a linear buffer. May be called
/// by bus-specific DMA map load functions.
///
/// # Safety
///
/// As for `machine::bus::BusDma::bus_dmamap_load`.
pub unsafe fn _dmamap_load(
    t: BusDmaTagT,
    map: &BusDmamap,
    buf: *mut u8,
    buflen: BusSize,
    p: Option<&Proc>,
    flags: i32,
) -> Result<(), Errno> {
    let mut lastaddr: BusAddr = 0;

    // Make sure that on error condition we return "no valid mappings".
    map.dm_nsegs.set(0);
    map.dm_mapsize.set(0);

    if buflen > map._dm_size {
        return Err(Errno::EINVAL);
    }

    let mut seg = 0;
    let mut used = 0;
    let mut lastbounce = false;
    // SAFETY: the caller's guarantee covers the buffer.
    unsafe {
        (t._dmamap_load_buffer)(
            t,
            map,
            buf as usize,
            buflen,
            p,
            flags,
            &mut lastaddr,
            &mut seg,
            &mut used,
            &mut lastbounce,
            true,
        )
    }?;
    map.dm_nsegs.set(seg + 1);
    map.dm_mapsize.set(buflen);
    map._dm_nused.set(used);
    Ok(())
}

/// `_dmamap_load_mbuf`: like `_bus_dmamap_load()`, but for mbufs.
///
/// # Safety
///
/// As for `machine::bus::BusDma::bus_dmamap_load_mbuf`.
pub unsafe fn _dmamap_load_mbuf(
    t: BusDmaTagT,
    map: &BusDmamap,
    m0: &Mbuf,
    flags: i32,
) -> Result<(), Errno> {
    let mut lastaddr: BusAddr = 0;

    // Make sure that on error condition we return "no valid mappings".
    map.dm_nsegs.set(0);
    map.dm_mapsize.set(0);

    #[cfg(feature = "diagnostic")]
    if m0.m_flags().get() & crate::sys::mbuf::M_PKTHDR == 0 {
        panic(format_args!("_dmamap_load_mbuf: no packet header"));
    }

    let pktlen = m0.m_pkthdr().len.get().max(0) as BusSize;
    if pktlen > map._dm_size {
        return Err(Errno::EINVAL);
    }

    let mut first = true;
    let mut seg = 0;
    let mut used = 0;
    let mut lastbounce = false;
    let mut m = Some(m0);
    while let Some(mb) = m {
        m = mb.m_next().get();
        if mb.m_len().get() == 0 {
            continue;
        }
        // SAFETY: the caller's guarantee covers every mbuf of the chain.
        unsafe {
            (t._dmamap_load_buffer)(
                t,
                map,
                mb.m_data().get() as usize,
                mb.m_len().get() as BusSize,
                None,
                flags,
                &mut lastaddr,
                &mut seg,
                &mut used,
                &mut lastbounce,
                first,
            )
        }?;
        first = false;
    }
    map.dm_mapsize.set(pktlen);
    map.dm_nsegs.set(seg + 1);
    map._dm_nused.set(used);
    Ok(())
}

/// `_dmamap_load_uio`: like `_dmamap_load()`, but for uios.
///
/// # Safety
///
/// As for `machine::bus::BusDma::bus_dmamap_load_uio`.
pub unsafe fn _dmamap_load_uio(
    t: BusDmaTagT,
    map: &BusDmamap,
    uio: &Uio<'_>,
    flags: i32,
) -> Result<(), Errno> {
    let mut lastaddr: BusAddr = 0;

    // Make sure that on error condition we return "no valid mappings".
    map.dm_nsegs.set(0);
    map.dm_mapsize.set(0);

    let mut resid = uio.uio_resid;
    let mut p = None;
    if uio.uio_segflg == UIO_USERSPACE {
        p = uio.uio_procp;
        #[cfg(feature = "diagnostic")]
        if p.is_none() {
            panic(format_args!("_dmamap_load_uio: USERSPACE but no proc"));
        }
    }

    let mut first = true;
    let mut seg = 0;
    let mut used = 0;
    let mut lastbounce = false;
    for iov in uio.uio_iov.iter() {
        if resid == 0 {
            break;
        }
        // Now at the first iovec to load. Load each iovec until we have exhausted the
        // residual count.
        let minlen = resid.min(iov.iov_len);
        // SAFETY: the caller's guarantee covers every iovec.
        unsafe {
            (t._dmamap_load_buffer)(
                t,
                map,
                iov.iov_base as usize,
                minlen,
                p,
                flags,
                &mut lastaddr,
                &mut seg,
                &mut used,
                &mut lastbounce,
                first,
            )
        }?;
        first = false;
        resid -= minlen;
    }
    map.dm_nsegs.set(seg + 1);
    map.dm_mapsize.set(uio.uio_resid);
    map._dm_nused.set(used);
    Ok(())
}

/// One chunk entering a map: the C's "insert chunk into a segment, coalescing with previous
/// segment if possible", shared by [`_dmamap_load_raw`] and [`_dmamap_load_buffer`].
/// Returns `false` when a new segment was needed and the map has none left.
#[allow(clippy::too_many_arguments)] // the C's locals
fn dmamap_add_chunk(
    t: BusDmaTagT,
    map: &BusDmamap,
    bmask: BusAddr,
    seg: &mut i32,
    first: &mut bool,
    addr: BusAddr,
    sgsize: BusSize,
    vaddr: usize,
    pgva: usize,
    bounce: bool,
    lastaddr: BusAddr,
    lastbounce: bool,
) -> bool {
    let fresh = BusDmaSegment {
        ds_addr: addr,
        ds_len: sgsize,
        _ds_paddr: addr,
        _ds_vaddr: vaddr,
        _ds_bounce_va: pgva,
    };
    if *first {
        map.seg(*seg as usize).set(fresh);
        *first = false;
        return true;
    }
    let cur = map.seg(*seg as usize).get();
    if addr == lastaddr
        && bounce == lastbounce
        && cur.ds_len + sgsize <= map._dm_maxsegsz
        && (map._dm_boundary == 0 || (cur.ds_addr & bmask) == (addr & bmask))
        && ((t._flags & BUS_DMA_COHERENT != 0 && !bounce)
            || cur._ds_vaddr.wrapping_add(cur.ds_len) == vaddr)
    {
        map.seg(*seg as usize).set(BusDmaSegment {
            ds_len: cur.ds_len + sgsize,
            ..cur
        });
        return true;
    }
    *seg += 1;
    if *seg >= map._dm_segcnt {
        return false;
    }
    map.seg(*seg as usize).set(fresh);
    true
}

/// `_dmamap_load_raw`: like `_dmamap_load()`, but for raw memory allocated with
/// `bus_dmamem_alloc()`.
///
/// # Safety
///
/// As for `machine::bus::BusDma::bus_dmamap_load_raw`.
pub unsafe fn _dmamap_load_raw(
    t: BusDmaTagT,
    map: &BusDmamap,
    segs: &[BusDmaSegment],
    size: BusSize,
    _flags: i32,
) -> Result<(), Errno> {
    let mut lastaddr: BusAddr = 0;
    let mut lastbounce = false;
    let mut first = true;
    let mut seg = 0;
    let mut page = 0;
    let mut size = size;

    // Make sure that on error condition we return "no valid mappings".
    map.dm_mapsize.set(0);
    map.dm_nsegs.set(0);

    if segs.len() > map._dm_segcnt as usize || size > map._dm_size {
        return Err(Errno::EINVAL);
    }

    let mapsize = size;
    let bmask = !map._dm_boundary.wrapping_sub(1);

    for s in segs {
        if size == 0 {
            break;
        }
        let mut paddr = s.ds_addr;
        let mut vaddr = s._ds_vaddr;
        let mut plen = s.ds_len.min(size);

        let mut bounce = false;
        if paddr + plen - 1 > DMA_CONSTRAINT.ucr_high.as_usize() {
            bounce = true;
        }

        while plen > 0 {
            let pgva = if bounce {
                if page >= map._dm_npages {
                    return Err(Errno::EFBIG);
                }
                let off = paddr & PAGE_MASK;
                let va = map._dm_pgva + ((page as usize) << PGSHIFT) + off;
                page += 1;
                va
            } else {
                NO_VA
            };

            // Compute the segment size, and adjust counts.
            let mut sgsize = PAGE_SIZE - (paddr & PGOFSET);
            if plen < sgsize {
                sgsize = plen;
            }

            // Make sure we don't cross any boundaries.
            if map._dm_boundary > 0 {
                let baddr = (paddr + map._dm_boundary) & bmask;
                if sgsize > baddr - paddr {
                    sgsize = baddr - paddr;
                }
            }

            if !dmamap_add_chunk(
                t, map, bmask, &mut seg, &mut first, paddr, sgsize, vaddr, pgva, bounce, lastaddr,
                lastbounce,
            ) {
                return Err(Errno::EINVAL);
            }

            paddr += sgsize;
            vaddr = vaddr.wrapping_add(sgsize);
            plen -= sgsize;
            size -= sgsize;

            lastaddr = paddr;
            lastbounce = bounce;
        }
    }

    map.dm_mapsize.set(mapsize);
    map.dm_nsegs.set(seg + 1);
    map._dm_nused.set(page);
    Ok(())
}

/// `_dmamap_unload`: common function for unloading a DMA map. May be called by bus-specific
/// DMA map unload functions.
pub fn _dmamap_unload(_t: BusDmaTagT, map: &BusDmamap) {
    // No resources to free; just mark the mappings as invalid.
    map.dm_nsegs.set(0);
    map.dm_mapsize.set(0);
    map._dm_nused.set(0);
}

/// `_dmamap_sync_segment`: the cache maintenance one segment needs for `ops`.
fn _dmamap_sync_segment(va: usize, len: usize, ops: i32) {
    match ops {
        o if o == BUS_DMASYNC_PREREAD | BUS_DMASYNC_PREWRITE || o == BUS_DMASYNC_PREREAD => {
            cpu_dcache_wbinv_range(va, len);
        }
        o if o == BUS_DMASYNC_PREWRITE => {
            cpu_dcache_wb_range(va, len);
        }
        // Cortex CPUs can do speculative loads so we need to clean the cache after a DMA
        // read to deal with any speculatively loaded cache lines. Since these can't be
        // dirty, we can just invalidate them and don't have to worry about having to write
        // back their contents.
        o if o == BUS_DMASYNC_POSTREAD || o == BUS_DMASYNC_POSTREAD | BUS_DMASYNC_POSTWRITE => {
            fence(Ordering::SeqCst); // membar_sync()
            // SAFETY: the device has just written this loaded memory; the PREREAD sync
            // before the transfer cleaned it, so no CPU write is lost.
            unsafe { cpu_dcache_inv_range(va, len) };
        }
        _ => {}
    }
}

/// `_dmamap_sync`: common function for DMA map synchronization. May be called by
/// bus-specific DMA map synchronization functions.
pub fn _dmamap_sync(t: BusDmaTagT, map: &BusDmamap, addr: BusAddr, size: BusSize, op: i32) {
    let coherent = t._flags & BUS_DMA_COHERENT != 0;
    let bounce = map._dm_nused.get() > 0;
    let mut addr = addr;
    let mut size = size;

    // If we're fully coherent, just make sure the write buffer is synced and return.
    if coherent && !bounce {
        fence(Ordering::SeqCst); // membar_sync()
        return;
    }

    let mut nsegs = map.dm_nsegs.get();
    let mut curseg = 0usize;

    while size != 0 && nsegs != 0 {
        let s = map.seg(curseg).get();
        let mut ssize = s.ds_len;
        let mut vaddr = s._ds_vaddr;
        let mut bounce_va = s._ds_bounce_va;

        if addr != 0 {
            if addr >= ssize {
                addr -= ssize;
                ssize = 0;
            } else {
                vaddr += addr;
                if bounce_va != NO_VA {
                    bounce_va += addr;
                }
                ssize -= addr;
                addr = 0;
            }
        }
        if ssize > size {
            ssize = size;
        }

        if ssize != 0 {
            let flush_va = if bounce_va != NO_VA { bounce_va } else { vaddr };

            if bounce_va != NO_VA && op & BUS_DMASYNC_PREWRITE != 0 {
                // SAFETY: the bounce page belongs to the map and the loaded buffer stays
                // mapped until unload (the load's contract); they do not overlap.
                unsafe {
                    ptr::copy_nonoverlapping(vaddr as *const u8, bounce_va as *mut u8, ssize)
                };
                BUS_DMA_BOUNCES.fetch_add(1, Ordering::Relaxed);
            }

            if !coherent {
                _dmamap_sync_segment(flush_va, ssize, op);
            }

            if bounce_va != NO_VA && op & BUS_DMASYNC_POSTREAD != 0 {
                // SAFETY: as above, the other way.
                unsafe {
                    ptr::copy_nonoverlapping(bounce_va as *const u8, vaddr as *mut u8, ssize)
                };
                BUS_DMA_BOUNCES.fetch_add(1, Ordering::Relaxed);
            }

            size -= ssize;
        }
        curseg += 1;
        nsegs -= 1;
    }

    if size != 0 {
        panic(format_args!("_dmamap_sync: ran off map!"));
    }
}

/// `_dmamem_alloc`: common function for DMA-safe memory allocation. May be called by
/// bus-specific DMA memory allocation functions.
pub fn _dmamem_alloc(
    t: BusDmaTagT,
    size: BusSize,
    alignment: BusSize,
    boundary: BusSize,
    segs: &mut [BusDmaSegment],
    flags: i32,
) -> Result<usize, Errno> {
    _dmamem_alloc_range(
        t,
        size,
        alignment,
        boundary,
        segs,
        flags,
        DMA_CONSTRAINT.ucr_low.as_usize(),
        DMA_CONSTRAINT.ucr_high.as_usize(),
    )
}

/// `_dmamem_free`: common function for freeing DMA-safe memory. May be called by
/// bus-specific DMA memory free functions.
///
/// # Safety
///
/// As for `machine::bus::BusDma::bus_dmamem_free`.
pub unsafe fn _dmamem_free(_t: BusDmaTagT, segs: &[BusDmaSegment]) {
    // Build a list of pages to free back to the VM system.
    let mlist = Pglist::new();
    mlist.init();
    for s in segs {
        let mut addr = s.ds_addr;
        while addr < s.ds_addr + s.ds_len {
            let Some(m) = PHYS_TO_VM_PAGE(Paddr::new(addr)) else {
                panic(format_args!("_dmamem_free: unmanaged page {:#x}", addr));
            };
            // SAFETY: the caller's guarantee: the page was allocated by `_dmamem_alloc` and
            // is on no list.
            unsafe { mlist.insert_tail(m) };
            addr += PAGE_SIZE;
        }
    }

    uvm_pglistfree(&mlist);
}

/// `_dmamem_map`: common function for mapping DMA-safe memory. May be called by
/// bus-specific DMA memory map functions.
pub fn _dmamem_map(
    t: BusDmaTagT,
    segs: &mut [BusDmaSegment],
    size: usize,
    flags: i32,
) -> Result<NonNull<u8>, Errno> {
    let mut size = round_page(size);
    let kd = if flags & BUS_DMA_NOWAIT != 0 {
        &KD_TRYLOCK
    } else {
        &KD_WAITOK
    };
    let kva = km_alloc(size, &KV_ANY, &KP_NONE, kd).ok_or(Errno::ENOMEM)?;

    let mut va = kva.as_ptr() as usize;
    let pmap_flags = PMAP_WIRED | PMAP_CANFAIL;
    let mut cache = PMAP_CACHE_WB;
    if (t._flags & BUS_DMA_COHERENT == 0 && flags & BUS_DMA_COHERENT != 0)
        || flags & BUS_DMA_NOCACHE != 0
    {
        cache = PMAP_CACHE_CI;
    }
    for s in segs.iter_mut() {
        s._ds_vaddr = va;
        let mut addr = s.ds_addr;
        while addr < s.ds_addr + s.ds_len {
            if size == 0 {
                panic(format_args!("_dmamem_map: size botch"));
            }
            // SAFETY: `va` is in the virtual range km_alloc just gave; `addr` is a page of
            // the DMA memory the caller allocated.
            unsafe {
                pmap_kenter_cache(
                    Vaddr::new(va),
                    Paddr::new(addr),
                    PROT_READ | PROT_WRITE | pmap_flags,
                    cache,
                )
            };
            addr += NBPG;
            va += NBPG;
            size -= NBPG;
        }
        pmap_update(pmap_kernel());
    }

    Ok(kva)
}

/// `_dmamem_unmap`: common function for unmapping DMA-safe memory. May be called by
/// bus-specific DMA memory unmapping functions.
///
/// # Safety
///
/// As for `machine::bus::BusDma::bus_dmamem_unmap`.
pub unsafe fn _dmamem_unmap(_t: BusDmaTagT, kva: NonNull<u8>, size: usize) {
    km_free(kva, round_page(size), &KV_ANY, &KP_NONE);
}

/// `_dmamem_mmap`: common function for mmap(2)'ing DMA-safe memory. May be called by
/// bus-specific DMA mmap(2)'ing functions.
pub fn _dmamem_mmap(
    _t: BusDmaTagT,
    segs: &[BusDmaSegment],
    off: Off,
    _prot: i32,
    flags: i32,
) -> Option<Paddr> {
    let mut pmapflags = 0;
    if flags & BUS_DMA_NOCACHE != 0 {
        pmapflags |= PMAP_NOCACHE as usize;
    }

    let mut off = off as usize;
    for s in segs {
        #[cfg(feature = "diagnostic")]
        {
            if off & PGOFSET != 0 {
                panic(format_args!("_dmamem_mmap: offset unaligned"));
            }
            if s.ds_addr & PGOFSET != 0 {
                panic(format_args!("_dmamem_mmap: segment unaligned"));
            }
            if s.ds_len & PGOFSET != 0 {
                panic(format_args!(
                    "_dmamem_mmap: segment size not multiple of page size"
                ));
            }
        }
        if off >= s.ds_len {
            off -= s.ds_len;
            continue;
        }

        return Some(Paddr::new((s.ds_addr + off) | pmapflags));
    }

    // Page not found.
    None
}

/*
 * DMA utility functions
 */

/// `_dmamap_load_buffer`: utility function to load a linear buffer. `lastaddrp` holds
/// state between invocations (for multiple-buffer loads). `segp` contains the starting
/// segment on entrance, and the ending segment on exit. `first` indicates if this is the
/// first invocation of this function.
///
/// # Safety
///
/// `[vaddr, vaddr + buflen)` is mapped in `p`'s address space (the kernel's when `p` is
/// `None`) and stays allocated until the map is unloaded.
#[allow(clippy::too_many_arguments)] // the C's signature
pub unsafe fn _dmamap_load_buffer(
    t: BusDmaTagT,
    map: &BusDmamap,
    vaddr: usize,
    buflen: BusSize,
    p: Option<&Proc>,
    _flags: i32,
    lastaddrp: &mut BusAddr,
    segp: &mut i32,
    usedp: &mut i32,
    lastbouncep: &mut bool,
    first: bool,
) -> Result<(), Errno> {
    let pmap = match p {
        Some(p) => p.vmspace().vm_map.pmap(),
        None => pmap_kernel(),
    };

    let mut page = *usedp;
    let mut lastaddr = *lastaddrp;
    let mut lastbounce = *lastbouncep;
    let mut bmask = !map._dm_boundary.wrapping_sub(1);
    if t._dma_mask != 0 {
        bmask &= t._dma_mask;
    }
    let mut first = first;
    let mut vaddr = vaddr;
    let mut buflen = buflen;

    let mut seg = *segp;
    while buflen > 0 {
        // Get the physical address for this segment.
        let Some(pa) = pmap_extract(pmap, Vaddr::new(vaddr)) else {
            panic(format_args!(
                "_dmapmap_load_buffer: pmap_extract({:p}, {:#x}) failed!",
                pmap, vaddr
            ));
        };
        let mut curaddr = pa.as_usize();

        let mut bounce = false;
        if curaddr > DMA_CONSTRAINT.ucr_high.as_usize() && map._dm_flags & BUS_DMA_64BIT == 0 {
            bounce = true;
        }

        let pgva = if bounce {
            if page >= map._dm_npages {
                return Err(Errno::EFBIG);
            }

            let off = vaddr & PAGE_MASK;
            let pg = map.dm_page(page as usize);
            curaddr = vm_page_to_phys(pg).as_usize() + off;
            let va = map._dm_pgva + ((page as usize) << PGSHIFT) + off;
            page += 1;
            va
        } else {
            NO_VA
        };

        // Compute the segment size, and adjust counts.
        let mut sgsize = NBPG - (vaddr & PGOFSET);
        if buflen < sgsize {
            sgsize = buflen;
        }

        // Make sure we don't cross any boundaries.
        if map._dm_boundary > 0 {
            let baddr = (curaddr + map._dm_boundary) & bmask;
            if sgsize > baddr.wrapping_sub(curaddr) {
                sgsize = baddr.wrapping_sub(curaddr);
            }
        }

        if !dmamap_add_chunk(
            t, map, bmask, &mut seg, &mut first, curaddr, sgsize, vaddr, pgva, bounce, lastaddr,
            lastbounce,
        ) {
            break;
        }

        lastaddr = curaddr + sgsize;
        lastbounce = bounce;
        vaddr += sgsize;
        buflen -= sgsize;
    }

    *segp = seg;
    *usedp = page;
    *lastaddrp = lastaddr;
    *lastbouncep = lastbounce;

    // Did we fit?
    if buflen != 0 {
        return Err(Errno::EFBIG); // XXX better return value here?
    }

    Ok(())
}

/// `_dmamem_alloc_range`: allocate physical memory from the given physical address range.
/// Called by DMA-safe memory allocation methods.
#[allow(clippy::too_many_arguments)] // the C's signature
pub fn _dmamem_alloc_range(
    _t: BusDmaTagT,
    size: BusSize,
    alignment: BusSize,
    boundary: BusSize,
    segs: &mut [BusDmaSegment],
    flags: i32,
    low: BusAddr,
    high: BusAddr,
) -> Result<usize, Errno> {
    // Always round the size.
    let size = round_page(size);

    // Allocate pages from the VM system.
    let mut plaflag = if flags & BUS_DMA_NOWAIT != 0 {
        UVM_PLA_NOWAIT
    } else {
        UVM_PLA_WAITOK
    };
    if flags & BUS_DMA_ZERO != 0 {
        plaflag |= UVM_PLA_ZERO;
    }

    let mlist = Pglist::new();
    mlist.init();
    uvm_pglistalloc(
        size,
        Paddr::new(low),
        Paddr::new(high),
        Paddr::new(alignment),
        Paddr::new(boundary),
        &mlist,
        segs.len() as i32,
        plaflag,
    )?;

    // Compute the location, size, and number of segments actually returned by the VM code.
    let Some(m) = mlist.first() else {
        panic(format_args!("_dmamem_alloc_range: empty page list"));
    };
    let mut curseg = 0;
    let mut lastaddr = vm_page_to_phys(m).as_usize();
    segs[curseg].ds_addr = lastaddr;
    segs[curseg].ds_len = PAGE_SIZE;
    // SAFETY: `m` is on `mlist`.
    unsafe { mlist.remove(m) };

    while let Some(m) = mlist.first() {
        // SAFETY: `m` is on `mlist`.
        unsafe { mlist.remove(m) };
        let curaddr = vm_page_to_phys(m).as_usize();
        #[cfg(feature = "diagnostic")]
        if curaddr < low || curaddr >= high {
            crate::kern::subr_prf::printf(format_args!(
                "vm_page_alloc_memory returned non-sensical address {:#x}\n",
                curaddr
            ));
            panic(format_args!("_dmamem_alloc_range"));
        }
        if curaddr == lastaddr + PAGE_SIZE {
            segs[curseg].ds_len += PAGE_SIZE;
        } else {
            curseg += 1;
            segs[curseg].ds_addr = curaddr;
            segs[curseg].ds_len = PAGE_SIZE;
        }
        lastaddr = curaddr;
    }

    Ok(curseg + 1)
}
