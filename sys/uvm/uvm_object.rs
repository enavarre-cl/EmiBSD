/*	$OpenBSD: uvm_object.h,v 1.30 2022/09/04 06:49:11 jsg Exp $	*/
/*	$OpenBSD: uvm_object.c,v 1.29 2026/07/22 20:58:23 kirill Exp $	*/
/*	$NetBSD: uvm_object.h,v 1.11 2001/03/09 01:02:12 chs Exp $	*/
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
 *
 * from: Id: uvm_object.h,v 1.1.2.2 1998/01/04 22:44:51 chuck Exp
 */
/* </LICENSES> */

//! The UVM memory object interface: `<uvm/uvm_object.h>`, and `uvm_object.c`: operate with
//! memory objects.
//!
//! Upstream: sys/uvm/uvm_object.h @ 3ce1f3f79392
//! Upstream: sys/uvm/uvm_object.c @ 3ce1f3f79392
//!
//! A UVM memory object represents a list of pages, which are managed by the object's pager
//! operations (`uvm_object::pgops`). All pages belonging to an object are owned by it and thus
//! protected by the object lock.
//!
//! The lock (`uvm_object::vmobjlock`) may be shared amongst the UVM objects. By default, the
//! lock is allocated dynamically using `rw_obj_init()` cache. Lock sharing is normally used
//! when there is an underlying object. For example, vnode representing a file may have an
//! underlying node, which is the case for tmpfs and layered file systems. In such case,
//! vnode's UVM object and the underlying UVM object shares the lock.
//!
//! The reference count is managed atomically for the anonymous UVM objects. For other
//! objects, it is arbitrary (may use the lock or atomics).
//!
//! Status: `wip`. Milestone M3 has the page tree and the counters; M7a adds `vmobjlock`,
//! `pgops`, the dummy pagers, `uvm_obj_init`/`destroy`/`setlock`/`wire`/`unwire`/`free` and
//! the `UVM_OBJ_IS_*` tests that have a pager to compare with.
//!
//! ## Deviations
//! - `UVM_OBJ_IS_DEVICE` answers false: `uvm_deviceops` (`uvm_device.c`) does not exist yet.
//! - `UVM_OBJ_IS_VTEXT` tests `VTEXT` on the object's vnode (`uvn->u_vnode`): the C casts the
//!   object itself to `struct vnode *`, which reads a member of the `uvm_vnode` instead.
//! - `UvmObject::new(refs)` is the `const` form of `uvm_obj_init(uobj, &pmap_pager, refs)`
//!   for the objects that live in statics (the pmaps' PTP objects): a dummy, lockless object.
//! - `uo_refs` is [`UoRefs`], an atomic `int`: the C changes it with `atomic_inc_int`/
//!   `atomic_dec_int_nv` (aobjs, the amd64 pmap's `pm_obj[0]`) or under the object's lock
//!   (vnodes); one type serves both, with relaxed plain loads and stores for the latter.

use core::cell::Cell;
use core::cmp::Ordering;
use core::ptr;
use core::sync::atomic::{self, AtomicI32};

use crate::kassert;
use crate::kern::kern_rwlock::{rw_enter, rw_exit, rw_obj_alloc, rw_obj_free};
use crate::sys::mman::{MADV_SEQUENTIAL, PROT_READ, PROT_WRITE};
use crate::sys::param::{PAGE_SHIFT, PAGE_SIZE};
use crate::sys::rwlock::{RW_DUPOK, RW_WRITE, Rwlock};
use crate::sys::systm::kernel_assert_locked;
use crate::sys::tree::{RbtEntry, RbtHead};
use crate::sys::vnode::VTEXT;
use crate::tree_adapter;
use crate::uvm::uvm_aobj::{AOBJ_PAGER, uao_dropswap};
use crate::uvm::uvm_extern::Voff;
use crate::uvm::uvm_page::{
    PG_CLEAN, PG_RELEASED, PG_TABLED, PQ_AOBJ, Pglist, VmPage, uvm_page_unbusy, uvm_pageclean,
    uvm_pagelookup, uvm_pageunwire, uvm_pagewire, uvm_pglistfree,
};
use crate::uvm::uvm_pager::{PGO_ALLPAGES, PGO_SYNCIO, UvmPagerops};
use crate::uvm::uvm_vnode::{UVM_VNODEOPS, uvn_of};

/// `UVM_OBJ_KERN` is a 'special' `uo_refs` value which indicates that the object is a kernel
/// memory object rather than a normal one (kernel memory objects don't have reference counts:
/// they never die).
///
/// This value is used to detected kernel object mappings at `uvm_unmap()` time. Normally when
/// an object is unmapped its pages eventually become deactivated and then paged out and/or
/// freed. This is not useful for kernel objects... when a kernel object is unmapped we always
/// want to free the resources associated with the mapping. `UVM_OBJ_KERN` allows us to decide
/// which type of unmapping we want to do.
///
/// In addition, we have kernel objects which may be used in an interrupt context. These
/// objects get their mappings entered with `pmap_kenter*()` and removed with `pmap_kremove()`,
/// which are safe to call in interrupt context, and must be used ONLY for wired kernel
/// mappings in these objects and their associated maps.
pub const UVM_OBJ_KERN: i32 = -2;

/// `FETCH_PAGECOUNT`: page count to fetch per single step.
const FETCH_PAGECOUNT: usize = 16;

/// `uvm_pagecmp`: orders an object's pages by offset.
pub fn uvm_pagecmp(a: &VmPage, b: &VmPage) -> Ordering {
    a.offset.get().cmp(&b.offset.get())
}

tree_adapter!(
    /// `uvm_objtree`: the pages of an object, by offset.
    pub UvmObjtree: VmPage, objt => RbtEntry, uvm_pagecmp
);

/// `uo_refs`, an `int` the C changes atomically (`atomic_inc_int`, `atomic_dec_int_nv`) or
/// under the object's lock (see the module's deviations).
pub struct UoRefs(AtomicI32);

impl UoRefs {
    /// A count of `refs`.
    pub const fn new(refs: i32) -> Self {
        Self(AtomicI32::new(refs))
    }

    /// Reads the count.
    pub fn get(&self) -> i32 {
        self.0.load(atomic::Ordering::Relaxed)
    }

    /// Sets the count (under the object's lock, or before the object is shared).
    pub fn set(&self, refs: i32) {
        self.0.store(refs, atomic::Ordering::Relaxed);
    }

    /// `atomic_inc_int(&uo_refs)`.
    pub fn atomic_inc(&self) {
        self.0.fetch_add(1, atomic::Ordering::Relaxed);
    }

    /// `atomic_dec_int_nv(&uo_refs)`: the count after the decrement. The release/acquire
    /// pair orders every use of the object before its teardown by the last holder.
    pub fn atomic_dec_nv(&self) -> i32 {
        self.0.fetch_sub(1, atomic::Ordering::AcqRel) - 1
    }
}

/// `struct uvm_object`.
pub struct UvmObject {
    /// `vmobjlock`: lock on object.
    pub vmobjlock: Cell<*const Rwlock>,
    /// `pgops`: pager ops.
    pub pgops: Cell<Option<&'static UvmPagerops>>,
    /// `memt`: pages in object.
    pub memt: RbtHead<UvmObjtree>,
    /// `uo_npages`: # of pages in memt.
    pub uo_npages: Cell<i32>,
    /// `uo_refs`: reference count.
    pub uo_refs: UoRefs,
}

// SAFETY: the object lock (`vmobjlock`) guards the page tree and the counters; dummy objects
// have none: a pmap's PTP objects are guarded by the pmap's lock (`pm_mtx`), the buffer
// cache's by the kernel lock, as in C. `uo_refs` is atomic.
unsafe impl Sync for UvmObject {}

impl UvmObject {
    /// An empty dummy object with `refs` references (see the module's deviations).
    pub const fn new(refs: i32) -> Self {
        Self {
            vmobjlock: Cell::new(ptr::null()),
            pgops: Cell::new(Some(&PMAP_PAGER)),
            memt: RbtHead::new(),
            uo_npages: Cell::new(0),
            uo_refs: UoRefs::new(refs),
        }
    }

    /// `uobj->vmobjlock`, which `uvm_obj_init` allocated or `uvm_obj_setlock` assigned.
    pub fn vmobjlock(&self) -> &'static Rwlock {
        let lock = self.vmobjlock.get();
        kassert!(!lock.is_null());
        // SAFETY: a lock object from `rw_obj_alloc` (alive until `uvm_obj_destroy` frees it)
        // or a static one.
        unsafe { &*lock }
    }

    /// `uobj->pgops`, which every object but the dummies has.
    pub fn pgops(&self) -> &'static UvmPagerops {
        match self.pgops.get() {
            Some(ops) => ops,
            None => crate::kern::subr_prf::panic(format_args!("uvm_object without pgops")),
        }
    }
}

/// `pmap_pager`: dummy object used by some pmaps for sanity checks.
pub static PMAP_PAGER: UvmPagerops = UvmPagerops::empty();

/// `bufcache_pager`: dummy object used by the buffer cache for sanity checks.
pub static BUFCACHE_PAGER: UvmPagerops = UvmPagerops::empty();

/// Whether `uobj`'s pager is `ops`.
fn pgops_is(uobj: &UvmObject, ops: &'static UvmPagerops) -> bool {
    uobj.pgops.get().is_some_and(|p| ptr::eq(p, ops))
}

/// `UVM_OBJ_IS_KERN_OBJECT(uobj)`.
pub fn uvm_obj_is_kern_object(uobj: &UvmObject) -> bool {
    uobj.uo_refs.get() == UVM_OBJ_KERN
}

/// `UVM_OBJ_IS_VNODE(uobj)`.
pub fn uvm_obj_is_vnode(uobj: &UvmObject) -> bool {
    pgops_is(uobj, &UVM_VNODEOPS)
}

/// `UVM_OBJ_IS_DEVICE(uobj)`: false until the device pager exists.
pub fn uvm_obj_is_device(_uobj: &UvmObject) -> bool {
    false
}

/// `UVM_OBJ_IS_VTEXT(uobj)`: a vnode object whose vnode is a running text (`VTEXT`; see the
/// module's deviations).
pub fn uvm_obj_is_vtext(uobj: &UvmObject) -> bool {
    uvn_of(uobj).is_some_and(|uvn| uvn.vnode().v_flag.get() & VTEXT != 0)
}

/// `UVM_OBJ_IS_AOBJ(uobj)`.
pub fn uvm_obj_is_aobj(uobj: &UvmObject) -> bool {
    pgops_is(uobj, &AOBJ_PAGER)
}

/// `UVM_OBJ_IS_PMAP(uobj)`.
pub fn uvm_obj_is_pmap(uobj: &UvmObject) -> bool {
    pgops_is(uobj, &PMAP_PAGER)
}

/// `UVM_OBJ_IS_BUFCACHE(uobj)`.
pub fn uvm_obj_is_bufcache(uobj: &UvmObject) -> bool {
    pgops_is(uobj, &BUFCACHE_PAGER)
}

/// `UVM_OBJ_IS_DUMMY(uobj)`: a pmap or bufcache object, which has no lock of its own.
pub fn uvm_obj_is_dummy(uobj: &UvmObject) -> bool {
    uvm_obj_is_pmap(uobj) || uvm_obj_is_bufcache(uobj)
}

/// `uvm_obj_init`: initialize UVM memory object.
pub fn uvm_obj_init(uobj: &UvmObject, pgops: Option<&'static UvmPagerops>, refs: i32) {
    let alock = pgops.is_some_and(|p| !ptr::eq(p, &PMAP_PAGER) && !ptr::eq(p, &BUFCACHE_PAGER))
        && refs != UVM_OBJ_KERN;

    if alock {
        // Allocate and assign a lock.
        uobj.vmobjlock.set(rw_obj_alloc("uobjlk"));
    } else {
        // The lock will need to be set via uvm_obj_setlock().
        uobj.vmobjlock.set(ptr::null());
    }
    uobj.pgops.set(pgops);
    uobj.memt.init();
    uobj.uo_npages.set(0);
    uobj.uo_refs.set(refs);
}

/// `uvm_obj_destroy`: destroy UVM memory object.
pub fn uvm_obj_destroy(uo: &UvmObject) {
    kassert!(uo.memt.is_empty());

    rw_obj_free(uo.vmobjlock());
    uo.vmobjlock.set(ptr::null());
}

/// `uvm_obj_setlock`: assign a vmobjlock to the UVM object.
///
/// Caller is responsible to ensure that UVM objects is not use. Only dynamic lock may be
/// previously set. We drop the reference then.
pub fn uvm_obj_setlock(uo: &UvmObject, lockptr: Option<&'static Rwlock>) {
    let olockptr = uo.vmobjlock.get();

    if !olockptr.is_null() {
        // Drop the reference on the old lock.
        // SAFETY: a non-null `vmobjlock` is a lock object from `rw_obj_alloc`.
        rw_obj_free(unsafe { &*olockptr });
    }
    let lockptr = match lockptr {
        Some(l) => l,
        // If new lock is not passed - allocate default one.
        None => rw_obj_alloc("uobjlk"),
    };
    uo.vmobjlock.set(lockptr);
}

/// `uvm_obj_wire`: wire the pages of entire UVM object.
///
/// NOTE: this function should only be used for types of objects where `PG_RELEASED` flag is
/// never set (aobj objects). Caller must pass page-aligned start and end values. If the
/// caller passes in a pageq pointer, we'll return a list of wired pages.
pub fn uvm_obj_wire(uobj: &UvmObject, start: Voff, end: Voff, pageq: Option<&Pglist>) -> i32 {
    let mut pgs: [*const VmPage; FETCH_PAGECOUNT] = [ptr::null(); FETCH_PAGECOUNT];
    let mut offset = start;

    let mut left = ((end - start) >> PAGE_SHIFT) as usize;

    let _ = rw_enter(uobj.vmobjlock(), RW_WRITE | RW_DUPOK);
    while left != 0 {
        let mut npages = FETCH_PAGECOUNT.min(left) as i32;

        // Get the pages
        pgs.fill(ptr::null());
        let Some(get) = uobj.pgops().pgo_get else {
            crate::kern::subr_prf::panic(format_args!("uvm_obj_wire: object without pgo_get"));
        };
        let error = get(
            uobj,
            offset,
            &mut pgs,
            &mut npages,
            0,
            PROT_READ | PROT_WRITE,
            MADV_SEQUENTIAL,
            PGO_ALLPAGES | PGO_SYNCIO,
        );

        if error != 0 {
            // error: Unwire the pages which have been wired
            uvm_obj_unwire(uobj, start, offset);
            return error;
        }

        let _ = rw_enter(uobj.vmobjlock(), RW_WRITE | RW_DUPOK);
        let npages = npages as usize;
        for &pg in &pgs[..npages] {
            kassert!(!pg.is_null());
            // SAFETY: `pgo_get` filled the slot with a page of the object, busy and ours.
            let pg = unsafe { &*pg };
            kassert!(pg.flags() & PG_RELEASED == 0);

            if pg.flags() & PQ_AOBJ != 0 {
                pg.clear_bits(PG_CLEAN);
                uao_dropswap(uobj, (pg.offset.get() >> PAGE_SHIFT) as i32);
            }
        }

        // Wire the pages
        for &pg in &pgs[..npages] {
            // SAFETY: as above.
            let pg = unsafe { &*pg };
            uvm_pagewire(pg);
            if let Some(pageq) = pageq {
                // SAFETY: a page of this object, on no page queue once wired; the caller's
                // list.
                unsafe { pageq.insert_tail(pg) };
            }
        }

        // Unbusy the pages
        let mut unbusy: [Option<&VmPage>; FETCH_PAGECOUNT] = [None; FETCH_PAGECOUNT];
        for (slot, &pg) in unbusy.iter_mut().zip(&pgs[..npages]) {
            // SAFETY: as above; the slots are the object's pages.
            *slot = unsafe { pg.as_ref() };
        }
        uvm_page_unbusy(&unbusy[..npages]);

        left -= npages;
        offset += (npages as Voff) << PAGE_SHIFT;
    }
    rw_exit(uobj.vmobjlock());

    0
}

/// `uvm_obj_unwire`: unwire the pages of entire UVM object.
///
/// Caller must pass page-aligned start and end values.
pub fn uvm_obj_unwire(uobj: &UvmObject, start: Voff, end: Voff) {
    let _ = rw_enter(uobj.vmobjlock(), RW_WRITE | RW_DUPOK);
    let mut offset = start;
    while offset < end {
        let pg = uvm_pagelookup(uobj, offset);

        let Some(pg) = pg else {
            crate::kern::subr_prf::panic(format_args!("uvm_obj_unwire: page {offset:#x} is gone"));
        };
        kassert!(pg.flags() & PG_RELEASED == 0);

        uvm_pageunwire(pg);
        offset += PAGE_SIZE as Voff;
    }
    rw_exit(uobj.vmobjlock());
}

/// `uvm_obj_free`: free all pages in a uvm object, used by the buffer cache to free all pages
/// attached to a buffer.
pub fn uvm_obj_free(uobj: &UvmObject) {
    let pgl = Pglist::new();

    kassert!(uvm_obj_is_bufcache(uobj));
    kernel_assert_locked();

    pgl.init();
    // Extract from rb tree in offset order. The phys addresses usually increase in that
    // order, which is better for uvm_pglistfree().
    for pg in uobj.memt.iter() {
        // clear PG_TABLED and `uobject' so we don't do work to remove this pg from the uobj
        // we are throwing away.
        pg.clear_bits(PG_TABLED);
        pg.uobject.set(ptr::null());
        uvm_pageclean(pg);
        // SAFETY: the page just left every queue; the list is this function's.
        unsafe { pgl.insert_tail(pg) };
    }
    uvm_pglistfree(&pgl);
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn uo_refs_count_like_the_c_atomics() {
        let refs = UoRefs::new(1);
        refs.atomic_inc();
        assert_eq!(refs.get(), 2);
        assert_eq!(refs.atomic_dec_nv(), 1);
        assert_eq!(refs.atomic_dec_nv(), 0);
        refs.set(UVM_OBJ_KERN);
        assert_eq!(refs.get(), UVM_OBJ_KERN);
    }
}
