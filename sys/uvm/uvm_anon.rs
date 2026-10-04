/*	$OpenBSD: uvm_anon.h,v 1.24 2025/12/15 13:02:18 mpi Exp $	*/
/*	$NetBSD: uvm_anon.h,v 1.13 2000/12/27 09:17:04 chs Exp $	*/
/*	$OpenBSD: uvm_anon.c,v 1.67 2026/02/11 22:34:40 deraadt Exp $	*/
/*	$NetBSD: uvm_anon.c,v 1.10 2000/11/25 06:27:59 chs Exp $	*/
/* <LICENSES> */
/*
 * Copyright (c) 1997 Charles D. Cranor and Washington University.
 * All rights reserved.
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

//! Anonymous memory management: `<uvm/uvm_anon.h>` and `uvm_anon.c` (uvm anon ops).
//!
//! Upstream: sys/uvm/uvm_anon.h @ 3ce1f3f79392
//! Upstream: sys/uvm/uvm_anon.c @ 3ce1f3f79392
//!
//! Anonymous virtual memory is short term virtual memory that goes away when the processes
//! referencing it go away. An anonymous page of virtual memory is described by `struct
//! vm_anon`. For active anons the data can be in one of the following state: \[1\] in a
//! vm_page with no backing store allocated yet, \[2\] in a vm_page with backing store
//! allocated, or \[3\] paged out to backing store (no vm_page).
//!
//! Status: `wip`. Milestone M3 had the structure; M7a (part 1) adds `an_lock`, `struct
//! vm_aref` and the functions: the anon pool, `uvm_analloc`, `uvm_anfree`, `uvm_anwait`,
//! `uvm_anon_dropswap`, `uvm_anon_release`, `uvm_anon_pagein`.
//!
//! ## Deviations
//! - Swap is not here (M7): `uvm_anon_dropswap` reports `uvm_swap_free` when an anon has a
//!   slot, which nothing assigns yet.
//! - `uvm_anon_pagein` needs `uvmfault_anonget` (M7a part 3) and reports it.
//! - M11a: `MULTIPROCESSOR`'s `unused` padding and `pool_cache_init` are in (the per-CPU
//!   pool caches need items of at least a `pool_cache_item`).

use core::cell::Cell;
use core::ptr::{self, NonNull};
use core::sync::atomic::Ordering;

use crate::kassert;
use crate::kern::kern_rwlock::{rw_exit, rw_obj_free, rw_obj_hold};
use crate::kern::subr_pool::{pool_get, pool_init, pool_put, pool_sethiwat};
use crate::machine::intr::IPL_MPFLOOR;
use crate::machine::pmap::pmap_page_protect;
use crate::sys::mman::PROT_NONE;
use crate::sys::pool::{PR_NOWAIT, PR_WAITOK, Pool};
use crate::sys::rwlock::{Rwlock, rw_lock_held, rw_write_held};
use crate::unported;
use crate::uvm::uvm_amap::VmAmap;
use crate::uvm::uvm_init::UVMEXP;
use crate::uvm::uvm_page::{PG_BUSY, PG_RELEASED, VmPage, uvm_pagefree};

/// `struct vm_anon`.
pub struct VmAnon {
    /// `an_lock`: the lock, shared with the amap the anon is in.
    pub an_lock: Cell<*const Rwlock>,
    /// `an_page`: if in RAM.
    pub an_page: Cell<*const VmPage>,
    /// `an_ref`: reference count.
    pub an_ref: Cell<i32>,
    /// `an_swslot`: drum swap slot # (if != 0) \[if we hold an_page, PG_BUSY\].
    pub an_swslot: Cell<i32>,
    /// `unused`: the per-CPU pool caching code requires pool item to be at least the size of
    /// struct pool_cache_item (`MULTIPROCESSOR && __LP64__`).
    #[cfg(feature = "multiprocessor")]
    pub unused: Cell<i64>,
}

// SAFETY: `an_lock` guards every field once the anon is in an amap; a fresh anon is its
// maker's.
unsafe impl Sync for VmAnon {}

impl VmAnon {
    /// An anon with no page, no lock and one reference (what `uvm_analloc` hands out).
    pub const fn new() -> Self {
        Self {
            an_lock: Cell::new(ptr::null()),
            an_page: Cell::new(ptr::null()),
            an_ref: Cell::new(1),
            an_swslot: Cell::new(0),
            #[cfg(feature = "multiprocessor")]
            unused: Cell::new(0),
        }
    }

    /// `an_lock`, if the anon has one.
    pub fn an_lock(&self) -> Option<&'static Rwlock> {
        // SAFETY: a non-null lock is a lock object held by this anon's amap.
        unsafe { self.an_lock.get().as_ref() }
    }

    /// `an_page`, if resident.
    pub fn page(&self) -> Option<&'static VmPage> {
        // SAFETY: a non-null `an_page` is a page of the page array, owned by this anon.
        unsafe { self.an_page.get().as_ref() }
    }
}

impl Default for VmAnon {
    fn default() -> Self {
        Self::new()
    }
}

/// `struct vm_aref`: processes reference anonymous virtual memory maps with an anonymous
/// reference structure. Note that the offset field indicates which part of the amap we are
/// referencing. Locked by vm_map lock.
pub struct VmAref {
    /// `ar_pageoff`: page offset into amap we start.
    pub ar_pageoff: Cell<i32>,
    /// `ar_amap`: pointer to amap.
    pub ar_amap: Cell<*const VmAmap>,
}

impl VmAref {
    /// No amap.
    pub const fn new() -> Self {
        Self {
            ar_pageoff: Cell::new(0),
            ar_amap: Cell::new(ptr::null()),
        }
    }

    /// `ar_amap`, if the entry has one.
    pub fn amap(&self) -> Option<&'static VmAmap> {
        // SAFETY: a non-null `ar_amap` is an amap this reference holds alive.
        unsafe { self.ar_amap.get().as_ref() }
    }
}

impl Default for VmAref {
    fn default() -> Self {
        Self::new()
    }
}

/// `uvm_anon_pool`.
static UVM_ANON_POOL: Pool = Pool::new();

/// `uvm_anon_init`: the anon pool.
pub fn uvm_anon_init() {
    pool_init(
        &UVM_ANON_POOL,
        size_of::<VmAnon>(),
        0,
        IPL_MPFLOOR,
        PR_WAITOK,
        "anonpl",
        None,
    );
    pool_sethiwat(
        &UVM_ANON_POOL,
        (UVMEXP.free.load(Ordering::Relaxed) / 16).max(0) as u32,
    );
}

/// `uvm_anon_init_percpu`: the anon pool's per-CPU caches (`MULTIPROCESSOR`).
pub fn uvm_anon_init_percpu() {
    #[cfg(feature = "multiprocessor")]
    crate::kern::subr_pool::pool_cache_init(&UVM_ANON_POOL);
}

/// `uvm_analloc`: allocate a new anon.
///
/// Anon will have no lock associated.
pub fn uvm_analloc() -> Option<&'static VmAnon> {
    let mem = pool_get(&UVM_ANON_POOL, PR_NOWAIT)?;
    let anon = mem.cast::<VmAnon>();
    // SAFETY: a fresh, suitably aligned pool item of `size_of::<VmAnon>()` bytes, written
    // once before anything else sees it; it lives until `uvm_anfree` returns it.
    Some(unsafe {
        anon.as_ptr().write(VmAnon::new());
        anon.as_ref()
    })
}

/// `uvm_anfree`: free a single anon structure.
///
/// Anon must be removed from the amap (if anon was in an amap). Amap must be locked, if anon
/// was owned by amap.
pub fn uvm_anfree(anon: &VmAnon) {
    let pg = anon.page();

    kassert!(anon.an_lock().is_none_or(rw_write_held));
    kassert!(anon.an_ref.get() == 0);

    // Dispose of the page, if it is resident.
    if let Some(pg) = pg {
        let Some(lock) = anon.an_lock() else {
            crate::kern::subr_prf::panic(format_args!("uvm_anfree: resident anon without a lock"));
        };

        // If the page is busy, mark it as PG_RELEASED, so that uvm_anon_release(9) would
        // release it later.
        if pg.flags() & PG_BUSY != 0 {
            pg.set_bits(PG_RELEASED);
            rw_obj_hold(lock);
            return;
        }
        pmap_page_protect(pg, PROT_NONE);
        uvm_pagefree(pg);
    } else if anon.an_swslot.get() != 0 {
        // This page is no longer only in swap.
        kassert!(UVMEXP.swpgonly.load(Ordering::Relaxed) > 0);
        UVMEXP.swpgonly.fetch_sub(1, Ordering::Relaxed);
    }
    anon.an_lock.set(ptr::null());

    // Free any swap resources, leave a page replacement hint.
    uvm_anon_dropswap(anon);

    kassert!(anon.an_page.get().is_null());
    kassert!(anon.an_swslot.get() == 0);

    pool_put(&UVM_ANON_POOL, NonNull::from(anon).cast::<u8>());
}

/// `uvm_anwait`: wait for memory to become available to allocate an anon.
pub fn uvm_anwait() {
    // XXX: Want something like pool_wait()?
    if let Some(anon) = pool_get(&UVM_ANON_POOL, PR_WAITOK) {
        pool_put(&UVM_ANON_POOL, anon);
    }
}

/// `uvm_anon_pagein`: fetch an anon's page.
///
/// Anon must be locked, and is unlocked upon return. Returns true if pagein was aborted due
/// to lack of memory.
pub fn uvm_anon_pagein(amap: &VmAmap, anon: &VmAnon) -> bool {
    kassert!(anon.an_lock().is_some_and(rw_write_held));
    kassert!(ptr::eq(anon.an_lock.get(), amap.am_lock.get()));

    // Get the page of the anon: uvmfault_anonget (uvm_fault.c, M7a part 3); until then
    // nothing is paged out, so there is nothing to fetch.
    let _ = unported!("uvm_anon_pagein: uvmfault_anonget (M7a-3)");
    if let Some(lock) = anon.an_lock() {
        rw_exit(lock);
    }
    false
}

/// `uvm_anon_dropswap`: release any swap resources from this anon.
///
/// Anon must be locked or have a reference count of 0.
pub fn uvm_anon_dropswap(anon: &VmAnon) {
    kassert!(anon.an_ref.get() == 0 || anon.an_lock().is_some_and(rw_lock_held));

    if anon.an_swslot.get() == 0 {
        return;
    }

    let _ = unported!("uvm_anon_dropswap: uvm_swap_free (swap, M7)");
    anon.an_swslot.set(0);
}

/// `uvm_anon_release`: release an anon and its page.
///
/// Anon should not have any references. Anon must be locked.
pub fn uvm_anon_release(anon: &VmAnon) {
    let Some(lock) = anon.an_lock() else {
        crate::kern::subr_prf::panic(format_args!("uvm_anon_release: anon without a lock"));
    };
    let Some(pg) = anon.page() else {
        crate::kern::subr_prf::panic(format_args!("uvm_anon_release: anon without a page"));
    };

    kassert!(rw_write_held(lock));
    kassert!(pg.flags() & PG_RELEASED != 0);
    kassert!(pg.flags() & PG_BUSY != 0);
    kassert!(pg.uobject().is_none());
    kassert!(ptr::eq(pg.uanon.get(), anon));
    kassert!(anon.an_ref.get() == 0);

    pmap_page_protect(pg, PROT_NONE);
    uvm_pagefree(pg);
    kassert!(anon.an_page.get().is_null());
    uvm_anon_dropswap(anon);
    pool_put(&UVM_ANON_POOL, NonNull::from(anon).cast::<u8>());
    rw_exit(lock);
    // Note: extra reference is held for PG_RELEASED case.
    rw_obj_free(lock);
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::kern::kern_rwlock::{rw_enter_write, rw_obj_alloc, rw_obj_init};
    use crate::kern::subr_pool::tests::setup_real_memory;
    use crate::uvm::uvm_page::uvm_pagealloc;

    #[test]
    fn anons_come_from_the_pool_and_free_their_page() {
        let _g = setup_real_memory();
        rw_obj_init();
        uvm_anon_init();
        let anon = uvm_analloc().expect("an anon");
        assert_eq!(anon.an_ref.get(), 1);
        assert!(anon.page().is_none() && anon.an_lock().is_none());

        let lock = rw_obj_alloc("testlk");
        anon.an_lock.set(lock);
        rw_enter_write(lock);
        let pg = uvm_pagealloc(None, 0, Some(anon), 0).expect("a page");
        assert!(anon.page().is_some_and(|p| ptr::eq(p, pg)));
        assert!(pg.uanon().is_some_and(|a| ptr::eq(a, anon)));
        pg.clear_bits(PG_BUSY);

        anon.an_ref.set(0);
        uvm_anfree(anon);
        assert!(pg.uanon().is_none(), "the page went back to the free pool");
        rw_exit(lock);
        rw_obj_free(lock);
    }
}
