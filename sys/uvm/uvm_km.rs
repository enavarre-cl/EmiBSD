/*	$OpenBSD: uvm_km.c,v 1.160 2026/06/23 14:40:40 bluhm Exp $	*/
/*	$NetBSD: uvm_km.c,v 1.42 2001/01/14 02:10:01 thorpej Exp $	*/
/* <LICENSES> */
/*
 * Copyright (c) 1997 Charles D. Cranor and Washington University.
 * Copyright (c) 1991, 1993, The Regents of the University of California.
 *
 * All rights reserved.
 *
 * This code is derived from software contributed to Berkeley by
 * The Mach Operating System project at Carnegie-Mellon University.
 *
 * Redistribution and use in source and binary forms, with or without
 * modification, are permitted provided that the following conditions
 * are met:
 * 1. Redistributions of source code must retain the above copyright
 *    notice, this list of conditions and the following disclaimer.
 * 2. Redistributions in binary form must reproduce the above copyright
 *    notice, this list of conditions and the following disclaimer in the
 *    documentation and/or other materials provided with the distribution.
 * 3. Neither the name of the University nor the names of its contributors
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
 *
 *	@(#)vm_kern.c   8.3 (Berkeley) 1/12/94
 * from: Id: uvm_km.c,v 1.1.2.14 1998/02/06 05:19:27 chs Exp
 *
 *
 * Copyright (c) 1987, 1990 Carnegie-Mellon University.
 * All rights reserved.
 *
 * Permission to use, copy, modify and distribute this software and
 * its documentation is hereby granted, provided that both the copyright
 * notice and this permission notice appear in all copies of the
 * software, derivative works or modified versions, and any portions
 * thereof, and that both notices appear in supporting documentation.
 *
 * CARNEGIE MELLON ALLOWS FREE USE OF THIS SOFTWARE IN ITS "AS IS"
 * CONDITION.  CARNEGIE MELLON DISCLAIMS ANY LIABILITY OF ANY KIND
 * FOR ANY DAMAGES WHATSOEVER RESULTING FROM THE USE OF THIS SOFTWARE.
 *
 * Carnegie Mellon requests users of this software to return to
 *
 *  Software Distribution Coordinator  or  Software.Distribution@CS.CMU.EDU
 *  School of Computer Science
 *  Carnegie Mellon University
 *  Pittsburgh PA 15213-3890
 *
 * any improvements or extensions that they make and grant Carnegie the
 * rights to redistribute these changes.
 */
/* </LICENSES> */

//! Kernel memory allocation and management: `uvm/uvm_km.c`.
//!
//! Upstream: sys/uvm/uvm_km.c @ 3ce1f3f79392
//!
//! Status: `wip`. Milestone M3 ports `km_alloc`/`km_free`, the allocation modes
//! (`kv_*`, `kp_*`, `kd_*`), `no_constraint` and the bounds `uvm_km_init` records;
//! `uvm_km_suballoc`, `uvm_km_pgremove`, the single-page thread (`uvm_km_page_*`) and the
//! maps themselves come with `uvm_map`.
//!
//! ## Deviations
//! - `uvm_km_init` only records the kernel map's range: `kernel_map` (`uvm_map_setup`, the
//!   reservation of `[base, start)`) waits for `uvm_map.c`. `kernel_map_min`/`kernel_map_max`
//!   stand in for `vm_map_min(kernel_map)`/`vm_map_max(kernel_map)` until then.
//! - Without `kernel_map`/`kmem_map`, `km_alloc` serves every request through the direct map
//!   (`__HAVE_PMAP_DIRECT`), which the C does only for single pages and single segments: a
//!   `kv_any`/`kv_intrsafe` request is therefore made physically contiguous (`kp_maxseg` 1)
//!   and reported once as unported, and `km_free` takes every non-pageable block back the
//!   direct-map way. `kp_nomem` and `kp_pageable` requests need the maps and fail, reported.
//! - `kd_slowdown` is a value, not a pointer: only the single-page thread writes it, and that
//!   thread is not used with `__HAVE_PMAP_DIRECT`.

use core::ptr::NonNull;
use core::sync::atomic::{AtomicUsize, Ordering};

use crate::machine::pmap::{pmap_map_direct, pmap_unmap_direct};
use crate::machine::{Machine, Pmap};
use crate::sys::param::PAGE_SIZE;
use crate::sys::types::{Paddr, Vaddr};
use crate::uvm::uvm_extern::{
    KmemDynMode, KmemPaMode, KmemVaMode, KvMap, UVM_PLA_NOWAIT, UVM_PLA_TRYCONTIG, UVM_PLA_WAITOK,
    UVM_PLA_ZERO, UVM_UNKNOWN_OFFSET, UvmConstraintRange,
};
use crate::uvm::uvm_page::{Pglist, uvm_pglistalloc, uvm_pglistfree};
use crate::uvm::uvm_param::round_page;
use crate::{kassert, unported};

/// `vm_map_min(kernel_map)` until the map exists (see the module's deviations).
static KERNEL_MAP_MIN: AtomicUsize = AtomicUsize::new(0);
/// `vm_map_max(kernel_map)` until the map exists.
static KERNEL_MAP_MAX: AtomicUsize = AtomicUsize::new(0);

/// `no_constraint`: unconstrained range.
pub static NO_CONSTRAINT: UvmConstraintRange = UvmConstraintRange {
    ucr_low: Paddr::new(0),
    ucr_high: Paddr::new(usize::MAX),
};

/// `kv_any`: any kernel virtual address, from `kernel_map`.
pub static KV_ANY: KmemVaMode = KmemVaMode {
    kv_map: KvMap::Kernel,
    kv_align: 0,
    kv_wait: false,
    kv_singlepage: false,
};

/// `kv_intrsafe`: from `kmem_map`, usable from interrupt handlers.
pub static KV_INTRSAFE: KmemVaMode = KmemVaMode {
    kv_map: KvMap::Kmem,
    kv_align: 0,
    kv_wait: false,
    kv_singlepage: false,
};

/// `kv_page`: the single page allocator.
pub static KV_PAGE: KmemVaMode = KmemVaMode {
    kv_map: KvMap::None,
    kv_align: 0,
    kv_wait: false,
    kv_singlepage: true,
};

const fn pa_mode(constraint: &'static UvmConstraintRange) -> KmemPaMode {
    KmemPaMode {
        kp_constraint: constraint,
        kp_object: false,
        kp_align: Paddr::new(0),
        kp_boundary: Paddr::new(0),
        kp_maxseg: 0,
        kp_nomem: false,
        kp_zero: false,
        kp_pageable: false,
    }
}

/// `kp_dirty`: any physical pages, not zeroed.
pub static KP_DIRTY: KmemPaMode = pa_mode(&NO_CONSTRAINT);

/// `kp_zero`: any physical pages, zeroed.
pub static KP_ZERO: KmemPaMode = KmemPaMode {
    kp_zero: true,
    ..pa_mode(&NO_CONSTRAINT)
};

/// `kp_dma`: pages every DMA-capable device can reach.
pub static KP_DMA: KmemPaMode = pa_mode(<Machine as Pmap>::DMA_CONSTRAINT);

/// `kp_dma_contig`: one physically contiguous DMA-reachable segment.
pub static KP_DMA_CONTIG: KmemPaMode = KmemPaMode {
    kp_maxseg: 1,
    ..pa_mode(<Machine as Pmap>::DMA_CONSTRAINT)
};

/// `kp_dma_zero`: zeroed DMA-reachable pages.
pub static KP_DMA_ZERO: KmemPaMode = KmemPaMode {
    kp_zero: true,
    ..pa_mode(<Machine as Pmap>::DMA_CONSTRAINT)
};

/// `kp_mbuf_contig`: one physically contiguous segment.
pub static KP_MBUF_CONTIG: KmemPaMode = KmemPaMode {
    kp_maxseg: 1,
    ..pa_mode(&NO_CONSTRAINT)
};

/// `kp_pageable`: pageable memory from the kernel object. XXX - kp_nomem, maybe, but we'll need
/// to fix km_free.
pub static KP_PAGEABLE: KmemPaMode = KmemPaMode {
    kp_object: true,
    kp_pageable: true,
    ..pa_mode(&NO_CONSTRAINT)
};

/// `kp_none`: virtual space only, no backing pages.
pub static KP_NONE: KmemPaMode = KmemPaMode {
    kp_nomem: true,
    ..pa_mode(&NO_CONSTRAINT)
};

/// `kd_waitok`: may sleep.
pub static KD_WAITOK: KmemDynMode = KmemDynMode {
    kd_prefer: UVM_UNKNOWN_OFFSET,
    kd_slowdown: false,
    kd_waitok: true,
    kd_trylock: false,
};

/// `kd_nowait`: must not sleep.
pub static KD_NOWAIT: KmemDynMode = KmemDynMode {
    kd_prefer: UVM_UNKNOWN_OFFSET,
    kd_slowdown: false,
    kd_waitok: false,
    kd_trylock: false,
};

/// `kd_trylock`: must not sleep on map locks.
pub static KD_TRYLOCK: KmemDynMode = KmemDynMode {
    kd_prefer: UVM_UNKNOWN_OFFSET,
    kd_slowdown: false,
    kd_waitok: false,
    kd_trylock: true,
};

/// `uvm_km_init`: init kernel virtual memory. `base` is the base of kernel virtual space,
/// `start` the first free address inside it and `end` its end (see the module's deviations).
pub fn uvm_km_init(_base: Vaddr, start: Vaddr, end: Vaddr) {
    // next, init kernel memory objects, uvm_map_setup(&kernel_map_store, pmap_kernel(), base,
    // end, VM_MAP_PAGEABLE), the reservation of [base, start) with uvm_map(): uvm_map.c.
    KERNEL_MAP_MIN.store(start.as_usize(), Ordering::Relaxed);
    KERNEL_MAP_MAX.store(end.as_usize(), Ordering::Relaxed);
}

/// `vm_map_min(kernel_map)`: the first kernel virtual address available for allocation.
pub fn kernel_map_min() -> Vaddr {
    Vaddr::new(KERNEL_MAP_MIN.load(Ordering::Relaxed))
}

/// `vm_map_max(kernel_map)`: the end of kernel virtual space.
pub fn kernel_map_max() -> Vaddr {
    Vaddr::new(KERNEL_MAP_MAX.load(Ordering::Relaxed))
}

/// `km_alloc`: `sz` bytes of kernel memory, laid out as `kv`, backed as `kp`, waiting as `kd`
/// (see the module's deviations).
pub fn km_alloc(
    sz: usize,
    kv: &KmemVaMode,
    kp: &KmemPaMode,
    kd: &KmemDynMode,
) -> Option<NonNull<u8>> {
    kassert!(sz == round_page(sz));

    let pgl = Pglist::new();
    pgl.init();

    if kp.kp_nomem || kp.kp_pageable {
        // alloc_va: uvm_map(map, &va, sz, uobj, kd->kd_prefer, kv->kv_align, ...) and, for
        // kp_pageable, pmap_enter of each page: uvm_map.c.
        let _ = unported!("km_alloc: virtual-only and pageable allocations (uvm_map)");
        return None;
    }

    let mut pla_flags = if kd.kd_waitok {
        UVM_PLA_WAITOK
    } else {
        UVM_PLA_NOWAIT
    };
    pla_flags |= UVM_PLA_TRYCONTIG;
    if kp.kp_zero {
        pla_flags |= UVM_PLA_ZERO;
    }

    let mut pla_align = kp.kp_align.as_usize();
    if <Machine as Pmap>::HAVE_PMAP_DIRECT && pla_align < kv.kv_align {
        pla_align = kv.kv_align;
    }
    let direct = kv.kv_singlepage || kp.kp_maxseg == 1;
    let pla_maxseg = if direct {
        kp.kp_maxseg.max(1)
    } else {
        // The C takes as many segments as pages here and maps them from the map; until
        // uvm_map exists the direct map serves, which needs one segment.
        let _ = unported!(
            "km_alloc: kernel_map/kmem_map space (uvm_map); served contiguously through the direct map"
        );
        1
    };

    uvm_pglistalloc(
        sz,
        kp.kp_constraint.ucr_low,
        kp.kp_constraint.ucr_high,
        Paddr::new(pla_align),
        kp.kp_boundary,
        &pgl,
        pla_maxseg,
        pla_flags,
    )
    .ok()?;

    // __HAVE_PMAP_DIRECT: only use direct mappings for single page or single segment
    // allocations (and, here, for every other one).
    let mut sva: Option<Vaddr> = None;
    while let Some(pg) = pgl.first() {
        // SAFETY: `pg` is the head of `pgl`.
        unsafe { pgl.remove(pg) };
        let va = pmap_map_direct(pg);
        if sva.is_none() {
            sva = Some(va);
        }
    }
    NonNull::new(sva?.as_usize() as *mut u8)
}

/// `km_free`: returns what `km_alloc` gave, with the same modes.
pub fn km_free(v: NonNull<u8>, sz: usize, kv: &KmemVaMode, kp: &KmemPaMode) {
    let sva = v.as_ptr() as usize;
    let eva = sva + sz;

    if kp.kp_nomem {
        // free_va: uvm_unmap(*kv->kv_map, sva, eva): uvm_map.c.
        let _ = unported!("km_free: uvm_unmap of virtual-only space (uvm_map)");
        return;
    }

    // __HAVE_PMAP_DIRECT: single pages and single segments (and, here, every non-pageable
    // block) come back through the direct map.
    if kv.kv_singlepage || kp.kp_maxseg == 1 || !kp.kp_pageable {
        let pgl = Pglist::new();
        pgl.init();
        let mut va = sva;
        while va < eva {
            let Some(pg) = pmap_unmap_direct(Vaddr::new(va)) else {
                #[allow(clippy::panic)] // the C panics on an unmanaged page too
                {
                    panic!("km_free: unmanaged page at {:#x}", va);
                }
            };
            // SAFETY: a page given out by km_alloc is on no list.
            unsafe { pgl.insert_tail(pg) };
            va += PAGE_SIZE;
        }
        uvm_pglistfree(&pgl);
        return;
    }

    // kp_pageable: pmap_remove(pmap_kernel(), sva, eva); pmap_update; then uvm_unmap.
    let _ = unported!("km_free: pageable blocks (pmap_remove, uvm_unmap)");
    let _ = kv.kv_wait; // wakeup(*kv->kv_map) when the map was waited for
}
