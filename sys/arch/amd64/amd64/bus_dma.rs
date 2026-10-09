/* <LICENSES> */
/*-
 * Copyright (c) 1996, 1997, 1998 The NetBSD Foundation, Inc.
 * All rights reserved.
 *
 * This code is derived from software contributed to The NetBSD Foundation
 * by Charles M. Hannum and by Jason R. Thorpe of the Numerical Aerospace
 * Simulation Facility, NASA Ames Research Center.
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
 * The following is included because _bus_dma_uiomove is derived from
 * uiomove() in kern_subr.c.
 */

/*
 * Copyright (c) 1982, 1986, 1991, 1993
 *	The Regents of the University of California.  All rights reserved.
 * (c) UNIX System Laboratories, Inc.
 * All or some portions of this file are derived from material licensed
 * to the University of California by American Telephone and Telegraph
 * Co. or Unix System Laboratories, Inc. and are reproduced herein with
 * the permission of UNIX System Laboratories, Inc.
 *
 * Copyright (c) 1992, 1993
 *	The Regents of the University of California.  All rights reserved.
 *
 * This software was developed by the Computer Systems Engineering group
 * at Lawrence Berkeley Laboratory under DARPA contract BG 91-66 and
 * contributed to Berkeley.
 *
 * All advertising materials mentioning features or use of this software
 * must display the following acknowledgement:
 *	This product includes software developed by the University of
 *	California, Lawrence Berkeley Laboratory.
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
 *	This product includes software developed by the University of
 *	California, Berkeley and its contributors.
 * 4. Neither the name of the University nor the names of its contributors
 *    may be used to endorse or promote products derived from this software
 *    without specific prior written permission.
 *
 * THIS SOFTWARE IS PROVIDED BY THE REGENTS AND CONTRIBUTORS ``AS IS'' AND
 * ANY EXPRESS OR IMPLIED WARRANTIES, INCLUDING, BUT NOT LIMITED TO, THE
 * IMPLIED WARRANTIES OF MERCHANTABILITY AND FITNESS FOR A PARTICULAR PURPOSE
 * ARE DISCLAIMED.  IN NO EVENT SHALL THE REGENTS OR CONTRIBUTORS BE LIABLE
 * FOR ANY DIRECT, INDIRECT, INCIDENTAL, SPECIAL, EXEMPLARY, OR CONSEQUENTIAL
 * DAMAGES (INCLUDING, BUT NOT LIMITED TO, PROCUREMENT OF SUBSTITUTE GOODS
 * OR SERVICES; LOSS OF USE, DATA, OR PROFITS; OR BUSINESS INTERRUPTION)
 * HOWEVER CAUSED AND ON ANY THEORY OF LIABILITY, WHETHER IN CONTRACT, STRICT
 * LIABILITY, OR TORT (INCLUDING NEGLIGENCE OR OTHERWISE) ARISING IN ANY WAY
 * OUT OF THE USE OF THIS SOFTWARE, EVEN IF ADVISED OF THE POSSIBILITY OF
 * SUCH DAMAGE.
 */
/* </LICENSES> */

/* <CODE> */
//! amd64 `bus_dma(9)`: the common DMA map and DMA memory functions every amd64 bus tag uses,
//! `arch/amd64/amd64/bus_dma.c`.
//!
//! Upstream: sys/arch/amd64/amd64/bus_dma.c @ 3ce1f3f79392
//!
//! A map translates a buffer (linear, an mbuf chain, a `uio`, or raw segments from
//! `bus_dmamem_alloc`) into physical segments a device can be programmed with, coalescing
//! physically contiguous pages and splitting at `_dm_maxsegsz` and `_dm_boundary`. A device
//! that cannot address all of memory (no `BUS_DMA_64BIT`) gets a map with bounce pages below
//! `dma_constraint` when there is memory above it; `_bus_dmamap_sync` copies between the
//! buffer and the bounce pages. amd64 caches are coherent with DMA, so a sync does nothing
//! else.
//!
//! ## Deviations
//! - The C's `NKSTAT` statistics (`bus_dma_kstat_copy` and the `mainbus0` "dma" kstat that
//!   `bus_dma_init` creates) need `kstat(4)` (`kern/subr_kstat.c`, not ported): the counters
//!   exist, the kstat is reported.
//! - `cpu_sev_guestmode` is false: the SEV probe (`identcpu.c`) is not ported, so a map only
//!   bounces for the DMA constraint (`FORCE_BOUNCE_BUFFER` is 0, as in C).
//! - A `pmap_extract` that fails in `_bus_dmamap_load_buffer` panics, as arm64's does; the C
//!   goes on with an uninitialised address.
//! - The map's size counts the segments after the header (`size_of::<BusDmamap>()` has none),
//!   where the C's `sizeof(struct bus_dmamap)` includes `dm_segs[0]` and adds
//!   `nsegments - 1`; the allocation is freed with the size it was made with.
//!   A map always has room for one segment, as the C's `dm_segs[1]` does, so `_dm_segcnt` is
//!   at least 1.
//! - Out pointers are return values (`_bus_dmamap_create`, `_bus_dmamem_alloc*`,
//!   `_bus_dmamem_map`); `_bus_dmamap_load_buffer` keeps the C's in/out state (`lastaddrp`,
//!   `segp`, `usedp`, `lastbouncep`) as `&mut` because it carries across calls.
//! - `_bus_dmamem_alloc_range` takes the pages off its local list before returning, so no
//!   page's queue entry points into a dead stack frame (the C leaves them linked).
//! - `km_alloc(kv_any, kp_none)` (bounce-page and multi-segment `bus_dmamem_map` virtual
//!   space) is reported by `uvm_km.rs` until it maps virtual-only space; those paths fail with
//!   `ENOMEM`, as the C does when `km_alloc` returns NULL. Single-segment cacheable
//!   `bus_dmamem_map` uses the direct map and works.

use core::cell::Cell;
use core::ptr::{self, NonNull};
use core::sync::atomic::{AtomicUsize, Ordering};

use crate::arch::amd64::amd64::machdep::DMA_CONSTRAINT;
use crate::arch::amd64::amd64::pmap::{pmap_direct_map, pmap_direct_mapped};
use crate::arch::amd64::include::bus::{
    BUS_DMA_64BIT, BUS_DMA_NOCACHE, BUS_DMA_NOWAIT, BUS_DMA_WAITOK, BUS_DMA_ZERO,
    BUS_DMASYNC_POSTREAD, BUS_DMASYNC_PREWRITE, BusDmaSegment, BusDmaTagT, BusDmamap, BusDmamapT,
};
use crate::arch::amd64::include::pmap::{PMAP_NOCACHE, PMAP_NOCRYPT};
use crate::kern::kern_malloc::{free, malloc};
use crate::kern::subr_prf::panic;
use crate::machine::bus::{BusAddr, BusSize};
use crate::machine::pmap::{pmap_enter, pmap_extract, pmap_kernel, pmap_update};
use crate::sys::errno::Errno;
use crate::sys::malloc::{M_DEVBUF, M_NOWAIT, M_WAITOK, M_ZERO};
use crate::sys::mbuf::Mbuf;
use crate::sys::mman::{PROT_READ, PROT_WRITE};
use crate::sys::param::{PAGE_MASK, PAGE_SIZE, PGOFSET, PGSHIFT};
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

/// `FORCE_BOUNCE_BUFFER`: bounce every map (a debugging knob, off).
const FORCE_BOUNCE_BUFFER: bool = false;

/// `cpu_sev_guestmode`: running as an AMD SEV guest (see the module's deviations).
const CPU_SEV_GUESTMODE: bool = false;

/// The `-1` of a `vaddr_t` that names no mapping.
const NO_VA: usize = usize::MAX;

/// `bus_dma_high_pages`: pages of memory above `dma_constraint`.
pub static BUS_DMA_HIGH_PAGES: AtomicUsize = AtomicUsize::new(0);
/// `bus_dma_bounce_pages`: bounce pages reserved by maps.
pub static BUS_DMA_BOUNCE_PAGES: AtomicUsize = AtomicUsize::new(0);
/// `bus_dma_bounces`: copies made to or from bounce pages.
pub static BUS_DMA_BOUNCES: AtomicUsize = AtomicUsize::new(0);

/// `bus_dma_init`: counts the memory a 32-bit device cannot reach, which decides whether
/// maps need bounce pages.
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

/// `_bus_dmamap_create`: common function for DMA map creation. May be called by
/// bus-specific DMA map creation functions.
pub fn _bus_dmamap_create(
    _t: BusDmaTagT,
    size: BusSize,
    nsegments: i32,
    maxsegsz: BusSize,
    boundary: BusSize,
    flags: i32,
) -> Result<BusDmamapT, Errno> {
    // allocate and use bounce buffers when running as SEV guest
    let mut use_bounce_buffer = CPU_SEV_GUESTMODE || FORCE_BOUNCE_BUFFER;
    let mut constraint: &UvmConstraintRange = &NO_CONSTRAINT;

    if flags & BUS_DMA_64BIT == 0 && BUS_DMA_HIGH_PAGES.load(Ordering::Relaxed) > 0 {
        use_bounce_buffer = true;
        constraint = &DMA_CONSTRAINT;
    }

    // Allocate and initialize the DMA map. The end of the map is a variable-sized array of
    // segments (and, when bouncing, of page pointers), so we allocate enough room for them
    // in one shot.
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
            _dm_flags: flags & !(BUS_DMA_WAITOK | BUS_DMA_NOWAIT),
            _dm_segcnt: nsegs as i32,
            _dm_maxsegsz: maxsegsz,
            _dm_boundary: boundary,
            _dm_cookie: Cell::new(ptr::null_mut()),
            _dm_pages: pages,
            _dm_pgva: 0,
            _dm_npages: npages as i32,
            _dm_nused: Cell::new(0),
            dm_mapsize: Cell::new(0),
            dm_nsegs: Cell::new(0),
            _dm_segs: NonNull::new_unchecked(segs),
        });
    }
    // SAFETY: written above; the map lives until `_bus_dmamap_destroy`.
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
            panic(format_args!("_bus_dmamap_create: short page list"));
        };
        // SAFETY: `pg` is the head of `mlist`; the map keeps it from now on.
        unsafe { mlist.remove(pg) };
        // SAFETY: slot `i` of the `npages` page pointers.
        unsafe { pages.add(i).write(ptr::from_ref(pg)) };
        if pmap_enter(
            pmap_kernel(),
            Vaddr::new(va),
            vm_page_to_phys(pg),
            PROT_READ | PROT_WRITE,
            PROT_READ | PROT_WRITE | PMAP_WIRED | PMAP_CANFAIL | PMAP_NOCRYPT,
        )
        .is_err()
        {
            pmap_update(pmap_kernel());
            map._dm_npages = 0;
            km_free(kva, sz, &KV_ANY, &KP_NONE);
            // Give back the pages taken so far with the ones still on the list.
            for j in 0..=i {
                // SAFETY: slots 0..=i were written above.
                let p = unsafe { &**pages.add(j) };
                // SAFETY: the page left `mlist` above and is on no other list.
                unsafe { mlist.insert_tail(p) };
            }
            free(mapstore, M_DEVBUF, mapsize);
            uvm_pglistfree(&mlist);
            return Err(Errno::ENOMEM);
        }
        // SAFETY: `va` was just mapped to a page this map owns.
        unsafe { ptr::write_bytes(va as *mut u8, 0, PAGE_SIZE) };
        va += PAGE_SIZE;
    }
    pmap_update(pmap_kernel());
    map._dm_pgva = sva;

    Ok(map)
}

/// `_bus_dmamap_destroy`: common function for DMA map destruction. May be called by
/// bus-specific DMA map destruction functions.
///
/// # Safety
///
/// `map` came from [`_bus_dmamap_create`] and is not used afterwards.
pub unsafe fn _bus_dmamap_destroy(_t: BusDmaTagT, map: NonNull<BusDmamap>) {
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

/// `_bus_dmamap_load`: common function for loading a DMA map with a linear buffer. May be
/// called by bus-specific DMA map load functions.
///
/// # Safety
///
/// As for `machine::bus::BusDma::bus_dmamap_load`.
pub unsafe fn _bus_dmamap_load(
    t: BusDmaTagT,
    map: &BusDmamap,
    buf: *mut u8,
    buflen: BusSize,
    p: Option<&Proc>,
    flags: i32,
) -> Result<(), Errno> {
    let mut lastaddr: BusAddr = 0;

    // Make sure that on error condition we return "no valid mappings".
    map.dm_mapsize.set(0);
    map.dm_nsegs.set(0);

    if buflen > map._dm_size {
        return Err(Errno::EINVAL);
    }

    let mut seg = 0;
    let mut used = 0;
    let mut lastbounce = false;
    // SAFETY: the caller's guarantee covers the buffer.
    unsafe {
        _bus_dmamap_load_buffer(
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
    map.dm_mapsize.set(buflen);
    map.dm_nsegs.set(seg + 1);
    map._dm_nused.set(used);
    Ok(())
}

/// `_bus_dmamap_load_mbuf`: like `_bus_dmamap_load()`, but for mbufs.
///
/// # Safety
///
/// As for `machine::bus::BusDma::bus_dmamap_load_mbuf`.
pub unsafe fn _bus_dmamap_load_mbuf(
    t: BusDmaTagT,
    map: &BusDmamap,
    m0: &Mbuf,
    flags: i32,
) -> Result<(), Errno> {
    let mut lastaddr: BusAddr = 0;

    // Make sure that on error condition we return "no valid mappings".
    map.dm_mapsize.set(0);
    map.dm_nsegs.set(0);

    #[cfg(feature = "diagnostic")]
    if m0.m_flags().get() & crate::sys::mbuf::M_PKTHDR == 0 {
        panic(format_args!("_bus_dmamap_load_mbuf: no packet header"));
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
            _bus_dmamap_load_buffer(
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

/// `_bus_dmamap_load_uio`: like `_bus_dmamap_load()`, but for uios.
///
/// # Safety
///
/// As for `machine::bus::BusDma::bus_dmamap_load_uio`.
pub unsafe fn _bus_dmamap_load_uio(
    t: BusDmaTagT,
    map: &BusDmamap,
    uio: &Uio<'_>,
    flags: i32,
) -> Result<(), Errno> {
    let mut lastaddr: BusAddr = 0;

    // Make sure that on error condition we return "no valid mappings".
    map.dm_mapsize.set(0);
    map.dm_nsegs.set(0);

    let mut resid = uio.uio_resid;
    let mut p = None;
    if uio.uio_segflg == UIO_USERSPACE {
        p = uio.uio_procp;
        #[cfg(feature = "diagnostic")]
        if p.is_none() {
            panic(format_args!("_bus_dmamap_load_uio: USERSPACE but no proc"));
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
            _bus_dmamap_load_buffer(
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
    map.dm_mapsize.set(uio.uio_resid);
    map.dm_nsegs.set(seg + 1);
    map._dm_nused.set(used);
    Ok(())
}

/// One chunk entering a map: the C's "insert chunk into a segment, coalescing with previous
/// segment if possible", shared by [`_bus_dmamap_load_raw`] and
/// [`_bus_dmamap_load_buffer`]. Returns `false` when a new segment was needed and the map has
/// none left.
#[allow(clippy::too_many_arguments)] // the C's locals
fn dmamap_add_chunk(
    map: &BusDmamap,
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
    let bmask = !map._dm_boundary.wrapping_sub(1);
    let fresh = BusDmaSegment {
        ds_addr: addr,
        ds_len: sgsize,
        _ds_va: vaddr,
        _ds_bounce_va: pgva,
        ..BusDmaSegment::default()
    };
    if *first {
        let cell = map.seg(*seg as usize);
        cell.set(BusDmaSegment {
            _ds_boundary: cell.get()._ds_boundary,
            _ds_align: cell.get()._ds_align,
            ..fresh
        });
        *first = false;
        return true;
    }
    let cur = map.seg(*seg as usize).get();
    if addr == lastaddr
        && bounce == lastbounce
        && cur.ds_len + sgsize <= map._dm_maxsegsz
        && (map._dm_boundary == 0 || (cur.ds_addr & bmask) == (addr & bmask))
        && (!bounce || cur._ds_va.wrapping_add(cur.ds_len) == vaddr)
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
    let cell = map.seg(*seg as usize);
    cell.set(BusDmaSegment {
        _ds_boundary: cell.get()._ds_boundary,
        _ds_align: cell.get()._ds_align,
        ..fresh
    });
    true
}

/// `_bus_dmamap_load_raw`: like `_bus_dmamap_load()`, but for raw memory allocated with
/// `bus_dmamem_alloc()`.
///
/// # Safety
///
/// As for `machine::bus::BusDma::bus_dmamap_load_raw`.
pub unsafe fn _bus_dmamap_load_raw(
    _t: BusDmaTagT,
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
        let mut plen = s.ds_len.min(size);

        let mut bounce = CPU_SEV_GUESTMODE || FORCE_BOUNCE_BUFFER;
        if paddr + plen - 1 > DMA_CONSTRAINT.ucr_high.as_usize()
            && map._dm_flags & BUS_DMA_64BIT == 0
        {
            bounce = true;
        }

        while plen > 0 {
            let (vaddr, pgva);
            if bounce {
                if page >= map._dm_npages {
                    return Err(Errno::EFBIG);
                }
                let off = paddr & PAGE_MASK;
                vaddr = pmap_direct_map(Paddr::new(paddr)).as_usize();
                pgva = map._dm_pgva + ((page as usize) << PGSHIFT) + off;
                page += 1;
            } else {
                vaddr = NO_VA;
                pgva = NO_VA;
            }

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
                map, &mut seg, &mut first, paddr, sgsize, vaddr, pgva, bounce, lastaddr, lastbounce,
            ) {
                return Err(Errno::EINVAL);
            }

            paddr += sgsize;
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

/// `_bus_dmamap_unload`: common function for unloading a DMA map. May be called by
/// bus-specific DMA map unload functions.
pub fn _bus_dmamap_unload(_t: BusDmaTagT, map: &BusDmamap) {
    // No resources to free; just mark the mappings as invalid.
    map.dm_mapsize.set(0);
    map.dm_nsegs.set(0);
    map._dm_nused.set(0);
}

/// `_bus_dmamap_sync`: common function for DMA map synchronization. May be called by
/// bus-specific DMA map synchronization functions.
pub fn _bus_dmamap_sync(_t: BusDmaTagT, map: &BusDmamap, addr: BusAddr, size: BusSize, op: i32) {
    let mut off = addr;
    let mut size = size;

    if map._dm_nused.get() == 0 {
        return;
    }

    for cell in map.dm_segs() {
        if size == 0 {
            break;
        }
        let sg = cell.get();
        if off >= sg.ds_len {
            off -= sg.ds_len;
            continue;
        }

        let l = (sg.ds_len - off).min(size);
        size -= l;

        if sg._ds_bounce_va == NO_VA {
            off = 0;
            continue;
        }

        // PREREAD and POSTWRITE are no-ops.

        // READ: device -> memory
        if op & BUS_DMASYNC_POSTREAD != 0 {
            // SAFETY: the bounce page belongs to the map and the buffer is loaded, which the
            // load's caller keeps mapped until unload; the two never overlap.
            unsafe {
                ptr::copy_nonoverlapping(
                    (sg._ds_bounce_va + off) as *const u8,
                    (sg._ds_va + off) as *mut u8,
                    l,
                )
            };
            BUS_DMA_BOUNCES.fetch_add(1, Ordering::Relaxed);
        }

        // WRITE: memory -> device
        if op & BUS_DMASYNC_PREWRITE != 0 {
            // SAFETY: as above, the other way.
            unsafe {
                ptr::copy_nonoverlapping(
                    (sg._ds_va + off) as *const u8,
                    (sg._ds_bounce_va + off) as *mut u8,
                    l,
                )
            };
            BUS_DMA_BOUNCES.fetch_add(1, Ordering::Relaxed);
        }

        off = 0;
    }
}

/// `_bus_dmamem_alloc`: common function for DMA-safe memory allocation. May be called by
/// bus-specific DMA memory allocation functions.
pub fn _bus_dmamem_alloc(
    t: BusDmaTagT,
    size: BusSize,
    alignment: BusSize,
    boundary: BusSize,
    segs: &mut [BusDmaSegment],
    flags: i32,
) -> Result<usize, Errno> {
    let (low, high) = if flags & BUS_DMA_64BIT != 0 {
        (NO_CONSTRAINT.ucr_low, NO_CONSTRAINT.ucr_high)
    } else {
        (DMA_CONSTRAINT.ucr_low, DMA_CONSTRAINT.ucr_high)
    };

    _bus_dmamem_alloc_range(
        t,
        size,
        alignment,
        boundary,
        segs,
        flags,
        low.as_usize(),
        high.as_usize(),
    )
}

/// `_bus_dmamem_free`: common function for freeing DMA-safe memory. May be called by
/// bus-specific DMA memory free functions.
///
/// # Safety
///
/// As for `machine::bus::BusDma::bus_dmamem_free`.
pub unsafe fn _bus_dmamem_free(_t: BusDmaTagT, segs: &[BusDmaSegment]) {
    // Build a list of pages to free back to the VM system.
    let mlist = Pglist::new();
    mlist.init();
    for s in segs {
        let mut addr = s.ds_addr;
        while addr < s.ds_addr + s.ds_len {
            let Some(m) = PHYS_TO_VM_PAGE(Paddr::new(addr)) else {
                panic(format_args!("_bus_dmamem_free: unmanaged page {:#x}", addr));
            };
            // SAFETY: the caller's guarantee: the page was allocated by `_bus_dmamem_alloc`
            // and is on no list.
            unsafe { mlist.insert_tail(m) };
            addr += PAGE_SIZE;
        }
    }

    uvm_pglistfree(&mlist);
}

/// `_bus_dmamem_map`: common function for mapping DMA-safe memory. May be called by
/// bus-specific DMA memory map functions.
pub fn _bus_dmamem_map(
    _t: BusDmaTagT,
    segs: &mut [BusDmaSegment],
    size: usize,
    flags: i32,
) -> Result<NonNull<u8>, Errno> {
    if segs.len() == 1 && flags & BUS_DMA_NOCACHE == 0 {
        let va = pmap_direct_map(Paddr::new(segs[0].ds_addr)).as_usize();
        return NonNull::new(va as *mut u8).ok_or(Errno::ENOMEM);
    }

    let mut pmapflags = 0;
    if flags & BUS_DMA_NOCACHE != 0 {
        pmapflags |= PMAP_NOCACHE as usize;
    }

    let mut size = round_page(size);
    let kd = if flags & BUS_DMA_NOWAIT != 0 {
        &KD_TRYLOCK
    } else {
        &KD_WAITOK
    };
    let kva = km_alloc(size, &KV_ANY, &KP_NONE, kd).ok_or(Errno::ENOMEM)?;

    let sva = kva.as_ptr() as usize;
    let ssize = size;
    let mut va = sva;
    for s in segs.iter() {
        let mut addr = s.ds_addr;
        while addr < s.ds_addr + s.ds_len {
            if size == 0 {
                panic(format_args!("_bus_dmamem_map: size botch"));
            }
            if let Err(error) = pmap_enter(
                pmap_kernel(),
                Vaddr::new(va),
                Paddr::new(addr | pmapflags),
                PROT_READ | PROT_WRITE,
                PROT_READ | PROT_WRITE | PMAP_WIRED | PMAP_CANFAIL,
            ) {
                pmap_update(pmap_kernel());
                km_free(kva, ssize, &KV_ANY, &KP_NONE);
                return Err(error);
            }
            addr += PAGE_SIZE;
            va += PAGE_SIZE;
            size -= PAGE_SIZE;
        }
    }
    pmap_update(pmap_kernel());

    Ok(kva)
}

/// `_bus_dmamem_unmap`: common function for unmapping DMA-safe memory. May be called by
/// bus-specific DMA memory unmapping functions.
///
/// # Safety
///
/// As for `machine::bus::BusDma::bus_dmamem_unmap`.
pub unsafe fn _bus_dmamem_unmap(_t: BusDmaTagT, kva: NonNull<u8>, size: usize) {
    #[cfg(feature = "diagnostic")]
    if kva.as_ptr() as usize & PGOFSET != 0 {
        panic(format_args!("_bus_dmamem_unmap"));
    }
    if pmap_direct_mapped(Vaddr::new(kva.as_ptr() as usize)) {
        return;
    }

    km_free(kva, round_page(size), &KV_ANY, &KP_NONE);
}

/// `_bus_dmamem_mmap`: common function for mmap(2)'ing DMA-safe memory. May be called by
/// bus-specific DMA mmap(2)'ing functions.
pub fn _bus_dmamem_mmap(
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
                panic(format_args!("_bus_dmamem_mmap: offset unaligned"));
            }
            if s.ds_addr & PGOFSET != 0 {
                panic(format_args!("_bus_dmamem_mmap: segment unaligned"));
            }
            if s.ds_len & PGOFSET != 0 {
                panic(format_args!(
                    "_bus_dmamem_mmap: segment size not multiple of page size"
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

/// `_bus_dmamap_load_buffer`: utility function to load a linear buffer. `lastaddrp` holds
/// state between invocations (for multiple-buffer loads). `segp` contains the starting
/// segment on entrance, and the ending segment on exit. `first` indicates if this is the
/// first invocation of this function.
///
/// # Safety
///
/// `[vaddr, vaddr + buflen)` is mapped in `p`'s address space (the kernel's when `p` is
/// `None`) and stays allocated until the map is unloaded.
#[allow(clippy::too_many_arguments)] // the C's signature
pub unsafe fn _bus_dmamap_load_buffer(
    _t: BusDmaTagT,
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
    let bmask = !map._dm_boundary.wrapping_sub(1);
    let mut first = first;
    let mut vaddr = vaddr;
    let mut buflen = buflen;

    let mut seg = *segp;
    while buflen > 0 {
        // Get the physical address for this segment.
        let Some(pa) = pmap_extract(pmap, Vaddr::new(vaddr)) else {
            panic(format_args!(
                "_bus_dmamap_load_buffer: pmap_extract({:p}, {:#x}) failed!",
                pmap, vaddr
            ));
        };
        let mut curaddr = pa.as_usize();

        let mut bounce = CPU_SEV_GUESTMODE || FORCE_BOUNCE_BUFFER;
        if curaddr > DMA_CONSTRAINT.ucr_high.as_usize() && map._dm_flags & BUS_DMA_64BIT == 0 {
            bounce = true;
        }

        let pgva;
        if bounce {
            if page >= map._dm_npages {
                return Err(Errno::EFBIG);
            }

            let off = vaddr & PAGE_MASK;
            let pg = map.dm_page(page as usize);
            curaddr = vm_page_to_phys(pg).as_usize() + off;
            pgva = map._dm_pgva + ((page as usize) << PGSHIFT) + off;
            page += 1;
        } else {
            pgva = NO_VA;
        }

        // Compute the segment size, and adjust counts.
        let mut sgsize = PAGE_SIZE - (vaddr & PGOFSET);
        if buflen < sgsize {
            sgsize = buflen;
        }

        // Make sure we don't cross any boundaries.
        if map._dm_boundary > 0 {
            let baddr = (curaddr + map._dm_boundary) & bmask;
            if sgsize > baddr - curaddr {
                sgsize = baddr - curaddr;
            }
        }

        if !dmamap_add_chunk(
            map, &mut seg, &mut first, curaddr, sgsize, vaddr, pgva, bounce, lastaddr, lastbounce,
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

/// `_bus_dmamem_alloc_range`: allocate physical memory from the given physical address
/// range. Called by DMA-safe memory allocation methods.
#[allow(clippy::too_many_arguments)] // the C's signature
pub fn _bus_dmamem_alloc_range(
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

    segs[0]._ds_boundary = boundary;
    segs[0]._ds_align = alignment;

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
        panic(format_args!("_bus_dmamem_alloc_range: empty page list"));
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
        {
            if curseg == segs.len() {
                crate::kern::subr_prf::printf(format_args!("uvm_pglistalloc returned too many\n"));
                panic(format_args!("_bus_dmamem_alloc_range"));
            }
            if curaddr < low || curaddr >= high {
                crate::kern::subr_prf::printf(format_args!(
                    "uvm_pglistalloc returned non-sensical address {:#x}\n",
                    curaddr
                ));
                panic(format_args!("_bus_dmamem_alloc_range"));
            }
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
/* </CODE> */
