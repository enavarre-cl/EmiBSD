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

/* <CODE> */
//! Kernel memory allocation and management: `uvm/uvm_km.c`.
//!
//! Upstream: sys/uvm/uvm_km.c @ 3ce1f3f79392
//!
//! Status: `wip`. Milestone M3 ports `km_alloc`/`km_free`, the allocation modes
//! (`kv_*`, `kp_*`, `kd_*`) and `no_constraint`; M7a-2 `uvm_km_init` with `kernel_map`,
//! `uvm_km_suballoc`, `uvm_km_pgremove` and `uvm_km_pgremove_intrsafe`. The single-page
//! thread (`uvm_km_page_*`) is `__HAVE_PMAP_DIRECT`: nothing.
//!
//! ## Deviations
//! - `kernel_map` is [`kernel_map()`] (a panic before `uvm_km_init`) and the C's
//!   `map == kernel_map` is [`is_kernel_map`]; `kmem_map` is `kern_malloc.rs`'s
//!   [`kmem_map()`](crate::kern::kern_malloc::kmem_map).
//! - [`kernel_map_min`] is the first address the kernel map can hand out after the bootstrap
//!   reservation (the selftests probe it), not `vm_map_min(kernel_map)`.
//! - On a machine without an MMU (`PMAP_NOMMU`, only the host test double) `km_alloc` and
//!   `km_free` serve every request through the direct map, physically contiguous, and refuse
//!   `kp_nomem`/`kp_pageable` (reported).
//! - `km_alloc`'s single-page allocator (`uvm_km_pages`, `!__HAVE_PMAP_DIRECT`) is not
//!   compiled: both machines have a direct map, so a `kv_singlepage` request that reaches the
//!   map path panics, as the C does.
//! - `kd_slowdown` is a value, not a pointer: only the single-page thread writes it, and that
//!   thread is not used with `__HAVE_PMAP_DIRECT`.

use core::ptr::{self, NonNull};
use core::sync::atomic::{AtomicPtr, AtomicUsize, Ordering};

use crate::kern::kern_malloc::kmem_map;
use crate::kern::kern_rwlock::rw_enter_write;
use crate::kern::kern_synch::{tsleep_nsec, wakeup};
use crate::kern::subr_prf::panic;
use crate::machine::pmap::{pmap_enter, pmap_kenter_pa, pmap_update};
use crate::machine::pmap::{
    pmap_extract, pmap_kernel, pmap_kremove, pmap_map_direct, pmap_reference, pmap_remove,
    pmap_unmap_direct,
};
use crate::machine::{Machine, Pmap};
use crate::sys::mman::{MADV_RANDOM, MAP_INHERIT_NONE, PROT_READ, PROT_WRITE};
use crate::sys::param::PVM;
use crate::sys::param::{PAGE_SHIFT, PAGE_SIZE};
use crate::sys::rwlock::rw_write_held;
use crate::sys::systm::INFSLP;
use crate::sys::types::{Paddr, Vaddr, Vsize};
use crate::uvm::uvm_aobj::{UAO_FLAG_KERNOBJ, uao_create, uao_dropswap, uao_init};
use crate::uvm::uvm_extern::UVM_FLAG_TRYLOCK;
use crate::uvm::uvm_extern::{
    KmemDynMode, KmemPaMode, KmemVaMode, KvMap, UVM_FLAG_FIXED, UVM_FLAG_NOMERGE, UVM_PLA_NOWAIT,
    UVM_PLA_TRYCONTIG, UVM_PLA_WAITOK, UVM_PLA_ZERO, UVM_UNKNOWN_OFFSET, UvmConstraintRange, Voff,
    uvm_mapflag,
};
use crate::uvm::uvm_init::{UVM, UVMEXP};
use crate::uvm::uvm_map::{
    VM_MAP_PAGEABLE, VmMap, uvm_map, uvm_map_create, uvm_map_setup, uvm_map_submap, uvm_unmap,
};
use crate::uvm::uvm_object::{UvmObject, uvm_obj_is_aobj};
use crate::uvm::uvm_page::vm_page_to_phys;
use crate::uvm::uvm_page::{
    PG_BUSY, PHYS_TO_VM_PAGE, Pglist, uvm_pagefree, uvm_pagelookup, uvm_pagewait, uvm_pglistalloc,
    uvm_pglistfree,
};
use crate::uvm::uvm_param::{VM_KERNEL_SPACE_SIZE, round_page};
use crate::uvm::uvm_pmap::PMAP_WIRED;
use crate::{kassert, unported};

/// `kernel_map_store`: the kernel map.
static KERNEL_MAP_STORE: VmMap = VmMap::new();
/// `kernel_map`: null until `uvm_km_init` set the store up.
static KERNEL_MAP: AtomicPtr<VmMap> = AtomicPtr::new(ptr::null_mut());
/// The first free kernel virtual address at `uvm_km_init` (see the module's deviations).
static KERNEL_MAP_MIN: AtomicUsize = AtomicUsize::new(0);
/// `vm_map_max(kernel_map)`, kept for the selftests.
static KERNEL_MAP_MAX: AtomicUsize = AtomicUsize::new(0);

/// `kernel_map`: the kernel's map, once `uvm_km_init` set it up.
pub fn kernel_map() -> &'static VmMap {
    let map = KERNEL_MAP.load(Ordering::Relaxed);
    if map.is_null() {
        panic(format_args!("kernel_map used before uvm_km_init"));
    }
    // SAFETY: the pointer is `&KERNEL_MAP_STORE`, a static, stored once by `uvm_km_init`.
    unsafe { &*map }
}

/// `map == kernel_map`: whether `map` is the kernel map (false before `uvm_km_init`, as
/// the C's null `kernel_map` compares).
pub fn is_kernel_map(map: &VmMap) -> bool {
    ptr::eq(KERNEL_MAP.load(Ordering::Relaxed), map)
}

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

/// `uvm_km_init`: init kernel maps and objects to reflect reality (i.e. KVM already
/// allocated for text, data, bss, and static data structures).
///
/// - KVM is defined by `[base.. base + VM_KERNEL_SPACE_SIZE]`. we assume that
///   `[base -> start]` has already been allocated and that "end" is the end of the kernel
///   image span.
pub fn uvm_km_init(base: Vaddr, start: Vaddr, end: Vaddr) {
    // kernel_object: for pageable anonymous kernel memory
    uao_init();
    let Some(kernel_object) = uao_create(Vsize::new(VM_KERNEL_SPACE_SIZE), UAO_FLAG_KERNOBJ) else {
        panic(format_args!("uvm_km_init: no kernel object"));
    };
    UVM.kernel_object.set(kernel_object);

    // init the map and reserve already allocated kernel space before installing.
    // KVA_GUARDPAGES is not configured.
    uvm_map_setup(
        &KERNEL_MAP_STORE,
        pmap_kernel(),
        base.as_usize(),
        end.as_usize(),
        VM_MAP_PAGEABLE,
    );
    let mut reserve = base.as_usize();
    if base != start
        && uvm_map(
            &KERNEL_MAP_STORE,
            &mut reserve,
            start.as_usize() - base.as_usize(),
            None,
            UVM_UNKNOWN_OFFSET,
            0,
            uvm_mapflag(
                PROT_READ | PROT_WRITE,
                PROT_READ | PROT_WRITE,
                MAP_INHERIT_NONE,
                MADV_RANDOM,
                UVM_FLAG_FIXED,
            ),
        )
        .is_err()
    {
        panic(format_args!(
            "uvm_km_init: could not reserve space for kernel"
        ));
    }

    KERNEL_MAP.store(
        ptr::from_ref(&KERNEL_MAP_STORE).cast_mut(),
        Ordering::Relaxed,
    );
    KERNEL_MAP_MIN.store(start.as_usize(), Ordering::Relaxed);
    KERNEL_MAP_MAX.store(end.as_usize(), Ordering::Relaxed);

    // __HAVE_PMAP_DIRECT: no uvm_km_pages.mtx.
}

/// The first kernel virtual address past the bootstrap reservation of `uvm_km_init` (see
/// the module's deviations).
pub fn kernel_map_min() -> Vaddr {
    Vaddr::new(KERNEL_MAP_MIN.load(Ordering::Relaxed))
}

/// `vm_map_max(kernel_map)`: the end of kernel virtual space.
pub fn kernel_map_max() -> Vaddr {
    Vaddr::new(KERNEL_MAP_MAX.load(Ordering::Relaxed))
}

/// `uvm_km_suballoc`: allocate a submap in the kernel map. once a submap is allocated all
/// references to that area of VM must go through it. this allows the locking of VAs in
/// kernel_map to be broken up into regions.
///
/// - if `fixed` is true, `*min` specifies where the region described by the submap must
///   start
/// - if submap is given we use that as the submap, otherwise we alloc a new map
pub fn uvm_km_suballoc(
    map: &VmMap,
    min: &mut usize,
    max: &mut usize,
    size: usize,
    flags: i32,
    fixed: bool,
    submap: Option<&'static VmMap>,
) -> &'static VmMap {
    let mapflags = UVM_FLAG_NOMERGE | if fixed { UVM_FLAG_FIXED } else { 0 };

    let size = round_page(size); // round up to pagesize

    // first allocate a blank spot in the parent map
    if uvm_map(
        map,
        min,
        size,
        None,
        UVM_UNKNOWN_OFFSET,
        0,
        uvm_mapflag(
            PROT_READ | PROT_WRITE,
            PROT_READ | PROT_WRITE,
            MAP_INHERIT_NONE,
            MADV_RANDOM,
            mapflags,
        ),
    )
    .is_err()
    {
        panic(format_args!(
            "uvm_km_suballoc: unable to allocate space in parent map"
        ));
    }

    // set VM bounds (min is filled in by uvm_map)
    *max = *min + size;

    // add references to pmap and create or init the submap
    pmap_reference(map.pmap());
    let submap = match submap {
        None => match uvm_map_create(map.pmap(), *min, *max, flags) {
            Some(submap) => submap,
            None => panic(format_args!("uvm_km_suballoc: unable to create submap")),
        },
        Some(submap) => {
            uvm_map_setup(submap, map.pmap(), *min, *max, flags);
            submap
        }
    };

    // now let uvm_map_submap plug in it...
    if uvm_map_submap(map, *min, *max, submap).is_err() {
        panic(format_args!("uvm_km_suballoc: submap allocation failed"));
    }

    submap
}

/// `uvm_km_pgremove`: remove pages from a kernel uvm_object.
///
/// - when you unmap a part of anonymous kernel memory you want to toss the pages right
///   away. (this gets called from uvm_unmap_...).
pub fn uvm_km_pgremove(uobj: &UvmObject, startva: Vaddr, endva: Vaddr) {
    let start = (startva.as_usize() - kernel_map().min_offset.get()) as Voff;
    let end = (endva.as_usize() - kernel_map().min_offset.get()) as Voff;
    let mut swpgonlydelta = 0;

    kassert!(uvm_obj_is_aobj(uobj));
    kassert!(rw_write_held(uobj.vmobjlock()));

    pmap_remove(pmap_kernel(), startva, endva);
    let mut curoff = start;
    while curoff < end {
        let pp = uvm_pagelookup(uobj, curoff);
        if let Some(pg) = pp.filter(|pg| pg.flags() & PG_BUSY != 0) {
            uvm_pagewait(pg, uobj.vmobjlock(), "km_pgrm");
            rw_enter_write(uobj.vmobjlock());
            continue; // loop back to us
        }

        // free the swap slot, then the page
        let slot = uao_dropswap(uobj, (curoff >> PAGE_SHIFT) as i32);

        match pp {
            Some(pg) => uvm_pagefree(pg),
            None if slot != 0 => swpgonlydelta += 1,
            None => {}
        }
        curoff += PAGE_SIZE as Voff;
    }

    if swpgonlydelta > 0 {
        kassert!(UVMEXP.swpgonly.load(Ordering::Relaxed) >= swpgonlydelta);
        UVMEXP.swpgonly.fetch_sub(swpgonlydelta, Ordering::Relaxed);
    }
}

/// `uvm_km_pgremove_intrsafe`: like `uvm_km_pgremove()`, but for "intrsafe" objects.
///
/// - when you unmap a part of anonymous kernel memory you want to toss the pages right
///   away. (this gets called from uvm_unmap_...).
/// - none of the pages will ever be busy, and none of them will ever be on the active or
///   inactive queues (because these objects are never allowed to "page").
pub fn uvm_km_pgremove_intrsafe(start: Vaddr, end: Vaddr) {
    let mut va = start.as_usize();
    while va < end.as_usize() {
        if let Some(pa) = pmap_extract(pmap_kernel(), Vaddr::new(va)) {
            let Some(pg) = PHYS_TO_VM_PAGE(pa) else {
                panic(format_args!("uvm_km_pgremove_intrsafe: no page"));
            };
            uvm_pagefree(pg);
        }
        va += PAGE_SIZE;
    }
    // SAFETY: `[start, end)` is a range of the intrsafe kernel map whose pages were just
    // freed; the entry that described it is being killed under the map lock.
    unsafe { pmap_kremove(start, Vsize::new(end.as_usize() - start.as_usize())) };
}

/// The map a `kmem_va_mode` allocates from (`*kv->kv_map`).
fn kv_map_of(kv: &KmemVaMode) -> &'static VmMap {
    match kv.kv_map {
        KvMap::Kernel => kernel_map(),
        KvMap::Kmem => kmem_map(),
        KvMap::Exec => crate::uvm::uvm_extern::exec_map(),
        KvMap::Phys => crate::uvm::uvm_extern::phys_map(),
        KvMap::None => panic(format_args!("km_alloc: single page mode has no map")),
    }
}

/// `km_alloc`: allocate `sz` bytes of kernel memory with the given virtual, physical and
/// dynamic modes.
pub fn km_alloc(
    sz: usize,
    kv: &KmemVaMode,
    kp: &KmemPaMode,
    kd: &KmemDynMode,
) -> Option<NonNull<u8>> {
    kassert!(sz == round_page(sz));

    let pgl = Pglist::new();
    pgl.init();

    if !(kp.kp_nomem || kp.kp_pageable) {
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
        let mut pla_maxseg = kp.kp_maxseg;
        if pla_maxseg == 0 {
            pla_maxseg = (sz / PAGE_SIZE) as i32;
        }
        if <Machine as Pmap>::PMAP_NOMMU {
            // No MMU (the host double): the direct map must serve, so one segment.
            pla_maxseg = 1;
        }

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
        // allocations.
        if <Machine as Pmap>::HAVE_PMAP_DIRECT
            && (kv.kv_singlepage || kp.kp_maxseg == 1 || <Machine as Pmap>::PMAP_NOMMU)
        {
            let mut sva: Option<Vaddr> = None;
            while let Some(pg) = pgl.first() {
                // SAFETY: `pg` is the head of `pgl`.
                unsafe { pgl.remove(pg) };
                let va = pmap_map_direct(pg);
                if sva.is_none() {
                    sva = Some(va);
                }
            }
            return NonNull::new(sva?.as_usize() as *mut u8);
        }
    }

    // alloc_va:
    if <Machine as Pmap>::PMAP_NOMMU {
        // Kernel virtual space without memory, or pageable memory, means nothing without an
        // MMU (the host double).
        let _ = unported!("km_alloc: kp_nomem/kp_pageable on a machine without an MMU");
        return None;
    }
    let prot = PROT_READ | PROT_WRITE;

    if kp.kp_pageable {
        kassert!(kp.kp_object);
        kassert!(!kv.kv_singlepage);
    } else {
        kassert!(!kp.kp_object);
    }

    if kv.kv_singlepage {
        kassert!(sz == PAGE_SIZE);
        // __HAVE_PMAP_DIRECT on both machines: the single-page thread is not used.
        panic(format_args!("km_alloc: DIRECT single page"));
    }

    let mut mapflags = 0;
    if kd.kd_trylock {
        mapflags |= UVM_FLAG_TRYLOCK;
    }
    let uobj = if kp.kp_object {
        // SAFETY: uvm_km_init created the kernel object, which lives forever.
        unsafe { UVM.kernel_object.get().as_ref() }
    } else {
        None
    };
    let mut va;
    loop {
        // try_map:
        let map = kv_map_of(kv);
        va = map.min_offset.get();
        if uvm_map(
            map,
            &mut va,
            sz,
            uobj,
            kd.kd_prefer,
            kv.kv_align,
            uvm_mapflag(prot, prot, MAP_INHERIT_NONE, MADV_RANDOM, mapflags),
        )
        .is_err()
        {
            if kv.kv_wait && kd.kd_waitok {
                let _ = tsleep_nsec(ptr::from_ref(map), PVM, "km_allocva", INFSLP);
                continue;
            }
            uvm_pglistfree(&pgl);
            return None;
        }
        break;
    }
    let sva = va;
    while let Some(pg) = pgl.first() {
        // SAFETY: `pg` is the head of `pgl`.
        unsafe { pgl.remove(pg) };
        if kp.kp_pageable {
            let _ = pmap_enter(
                pmap_kernel(),
                Vaddr::new(va),
                vm_page_to_phys(pg),
                prot,
                prot | PMAP_WIRED,
            );
        } else {
            // SAFETY: `va` is fresh space uvm_map just gave us in a kernel map, and `pg` a
            // page uvm_pglistalloc just gave us.
            unsafe { pmap_kenter_pa(Vaddr::new(va), vm_page_to_phys(pg), prot) };
        }
        va += PAGE_SIZE;
    }
    pmap_update(pmap_kernel());
    NonNull::new(sva as *mut u8)
}

/// `km_free`: returns what `km_alloc` gave, with the same modes.
pub fn km_free(v: NonNull<u8>, sz: usize, kv: &KmemVaMode, kp: &KmemPaMode) {
    let sva = v.as_ptr() as usize;
    let eva = sva + sz;

    if !kp.kp_nomem {
        // __HAVE_PMAP_DIRECT
        if <Machine as Pmap>::HAVE_PMAP_DIRECT
            && (kv.kv_singlepage || kp.kp_maxseg == 1 || <Machine as Pmap>::PMAP_NOMMU)
        {
            let pgl = Pglist::new();
            pgl.init();
            let mut va = sva;
            while va < eva {
                let Some(pg) = pmap_unmap_direct(Vaddr::new(va)) else {
                    panic(format_args!("km_free: unmanaged page at {va:#x}"));
                };
                // SAFETY: a page given out by km_alloc is on no list.
                unsafe { pgl.insert_tail(pg) };
                va += PAGE_SIZE;
            }
            uvm_pglistfree(&pgl);
            return;
        }

        if kp.kp_pageable {
            pmap_remove(pmap_kernel(), Vaddr::new(sva), Vaddr::new(eva));
            pmap_update(pmap_kernel());
        } else {
            let pgl = Pglist::new();
            pgl.init();
            let mut va = sva;
            while va < eva {
                if let Some(pa) = pmap_extract(pmap_kernel(), Vaddr::new(va)) {
                    let Some(pg) = PHYS_TO_VM_PAGE(pa) else {
                        panic(format_args!("km_free: unmanaged page {:#x}", pa.as_usize()));
                    };
                    // SAFETY: a page km_alloc mapped is on no list.
                    unsafe { pgl.insert_tail(pg) };
                }
                va += PAGE_SIZE;
            }
            // SAFETY: the range is ours, mapped by km_alloc with pmap_kenter_pa; the caller
            // gives it back.
            unsafe { pmap_kremove(Vaddr::new(sva), Vsize::new(sz)) };
            pmap_update(pmap_kernel());
            uvm_pglistfree(&pgl);
        }
    }
    // free_va:
    let map = kv_map_of(kv);
    uvm_unmap(map, sva, eva);
    if kv.kv_wait {
        wakeup(ptr::from_ref(map));
    }
}
/* </CODE> */
