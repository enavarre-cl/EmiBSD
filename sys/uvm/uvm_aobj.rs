/*	$OpenBSD: uvm_aobj.h,v 1.20 2023/05/13 09:24:59 mpi Exp $	*/
/*	$NetBSD: uvm_aobj.h,v 1.10 2000/01/11 06:57:49 chs Exp $	*/
/*	$OpenBSD: uvm_aobj.c,v 1.123 2026/09/06 21:01:30 kirill Exp $	*/
/*	$NetBSD: uvm_aobj.c,v 1.39 2001/02/18 21:19:08 chs Exp $	*/
/* <LICENSES> */
/*
 * Copyright (c) 1998 Chuck Silvers, Charles D. Cranor and
 *                    Washington University.
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
 * from: Id: uvm_aobj.h,v 1.1.2.4 1998/02/06 05:19:28 chs Exp
 * from: Id: uvm_aobj.c,v 1.1.2.5 1998/02/06 05:14:38 chs Exp
 */
/* </LICENSES> */

/* <CODE> */
//! `uvm_aobj.h` / `uvm_aobj.c`: anonymous memory uvm_object pager.
//!
//! Upstream: sys/uvm/uvm_aobj.h @ 3ce1f3f79392
//! Upstream: sys/uvm/uvm_aobj.c @ 3ce1f3f79392
//!
//! author: Chuck Silvers, started: Jan-1998, design mostly from Chuck Cranor.
//!
//! An anonymous UVM object (aobj) manages anonymous-memory. In addition to keeping the list
//! of resident pages, it may also keep a list of allocated swap blocks. Depending on the
//! size of the object, this list is either stored in an array (small objects) or in a hash
//! table (large objects).
//!
//! Status: `wip`. Milestone M7a (part 1) ports the object, `uao_create` (the kernel object
//! included), the pager operations and the swap-slot bookkeeping.
//!
//! ## Deviations
//! - Swap does not exist yet (M7): `uao_get` reports `uvm_swap_get` and treats a slot as an
//!   I/O error (the `SWSLOT_BAD` marking and `uvm_swap_markbad` are reported with it);
//!   `uao_dropswap` and `uao_dropswap_range` report `uvm_swap_free`. Nothing assigns a slot,
//!   so none of these paths run.
//! - `uao_shrink`/`uao_grow` and their helpers are `#ifdef TMPFS`: feature `tmpfs`. When
//!   they move the swap-hash elements to a table of another size, each element goes to the
//!   bucket its tag hashes to in the new table; the C moves bucket `i` of the old table to
//!   bucket `i` of the new one, which is the same bucket whenever the tags are below both
//!   tables' sizes (its comment's assumption) and indexes past a smaller table otherwise.
//!   Rust keeps `u_swslots` and `u_swhash` side by side, so a conversion clears the one it
//!   leaves (the C's union is overwritten).
//! - `u_swhash` keeps the slice `hashinit` returns; `u_swhashmask` is its length minus one.
//! - `UVM_PAGE_OWN` is not configured.

use core::cell::Cell;
use core::ptr::{self, NonNull};
use core::sync::atomic::{AtomicI32, Ordering};

use crate::kassert;
use crate::kern::kern_lock::{mtx_enter, mtx_leave};
use crate::kern::kern_malloc::{free, mallocarray};
use crate::kern::kern_rwlock::{rw_enter, rw_exit, rw_init};
use crate::kern::kern_subr::{hashfree, hashinit};
use crate::kern::kern_synch::wakeup;
use crate::kern::subr_pool::{pool_get, pool_init, pool_put};
use crate::kern::subr_prf::{panic, printf};
use crate::machine::intr::{IPL_MPFLOOR, IPL_NONE};
use crate::machine::pmap::{pmap_clear_modify, pmap_page_protect};
#[cfg(feature = "tmpfs")]
use crate::sys::errno::Errno;
#[cfg(feature = "tmpfs")]
use crate::sys::malloc::M_CANFAIL;
use crate::sys::malloc::{M_NOWAIT, M_UVMAOBJ, M_WAITOK, M_ZERO};
use crate::sys::mman::{PROT_NONE, PROT_READ, PROT_WRITE};
use crate::sys::mutex::Mutex;
use crate::sys::param::{PAGE_SHIFT, PAGE_SIZE};
use crate::sys::pool::{PR_NOWAIT, PR_WAITOK, PR_ZERO, Pool};
use crate::sys::queue::{ListEntry, ListHead};
use crate::sys::rwlock::{RW_WRITE, Rwlock, rw_lock_held, rw_write_held};
use crate::sys::types::Vsize;
use crate::uvm::uvm_extern::{VmProt, Voff};
use crate::uvm::uvm_init::UVMEXP;
use crate::uvm::uvm_object::{
    UVM_OBJ_KERN, UvmObject, uvm_obj_destroy, uvm_obj_init, uvm_obj_is_aobj,
    uvm_obj_is_kern_object, uvm_obj_setlock,
};
use crate::uvm::uvm_page::{
    PG_BUSY, PG_CLEAN, PG_FAKE, PG_WANTED, PQ_AOBJ, VmPage, uvm_pagealloc, uvm_pagedeactivate,
    uvm_pagefree, uvm_pagelookup, uvm_pagewait, uvm_pagezero,
};
use crate::uvm::uvm_pager::{
    PGO_ALLPAGES, PGO_CLEANIT, PGO_DEACTIVATE, PGO_FREE, PGO_LOCKED, PGO_SYNCIO, UvmPagerops,
    VM_PAGER_BAD, VM_PAGER_ERROR, VM_PAGER_OK, VM_PAGER_REFAULT, VM_PAGER_UNLOCK, pgo_dontcare,
};
use crate::uvm::uvm_param::{round_page, trunc_page};
use crate::uvm::uvm_pdaemon::uvm_wait;
use crate::{queue_adapter, unported};

// flags

/// `UAO_FLAG_KERNOBJ`: create kernel object (`uao_create`; can only be used one time, at
/// bootup).
pub const UAO_FLAG_KERNOBJ: i32 = 0x1;
/// `UAO_FLAG_KERNSWAP`: enable kernel swap (`uao_create`, once).
pub const UAO_FLAG_KERNSWAP: i32 = 0x2;
/// `UAO_FLAG_CANFAIL`: creation can fail.
pub const UAO_FLAG_CANFAIL: i32 = 0x4;
/// `UAO_FLAG_NOSWAP`: internal: aobj can't swap (kernel obj only!).
pub const UAO_FLAG_NOSWAP: i32 = 0x8;

/// `UAO_SWHASH_CLUSTER_SHIFT`: for hash tables, we break the address space of the aobj into
/// blocks of `UAO_SWHASH_CLUSTER_SIZE` pages, which shall be a power of two.
const UAO_SWHASH_CLUSTER_SHIFT: u32 = 4;
/// `UAO_SWHASH_CLUSTER_SIZE`.
const UAO_SWHASH_CLUSTER_SIZE: usize = 1 << UAO_SWHASH_CLUSTER_SHIFT;

/// `UAO_SWHASH_ELT_TAG(idx)`: the "tag" for this page index.
const fn uao_swhash_elt_tag(idx: Voff) -> Voff {
    idx >> UAO_SWHASH_CLUSTER_SHIFT
}

/// `UAO_SWHASH_ELT_PAGESLOT_IDX(idx)`.
const fn uao_swhash_elt_pageslot_idx(idx: Voff) -> usize {
    (idx as usize) & (UAO_SWHASH_CLUSTER_SIZE - 1)
}

/// `UAO_SWHASH_THRESHOLD`: the threshold which determines whether we will use an array or a
/// hash table to store the list of allocated swap blocks.
const UAO_SWHASH_THRESHOLD: i32 = (UAO_SWHASH_CLUSTER_SIZE * 4) as i32;

/// `UAO_SWHASH_MAXBUCKETS`: the number of buckets in a hash, with an upper bound.
const UAO_SWHASH_MAXBUCKETS: i32 = 256;

/// `UAO_SWHASH_BUCKETS(pages)`.
const fn uao_swhash_buckets(pages: i32) -> i32 {
    let n = pages >> UAO_SWHASH_CLUSTER_SHIFT;
    if n < UAO_SWHASH_MAXBUCKETS {
        n
    } else {
        UAO_SWHASH_MAXBUCKETS
    }
}

/// `struct uao_swhash_elt`: when a hash table is being used, this structure defines the
/// format of an entry in the bucket list.
pub struct UaoSwhashElt {
    /// `list`: the hash list.
    pub list: ListEntry<UaoSwhashElt>,
    /// `tag`: our 'tag'.
    pub tag: Cell<Voff>,
    /// `count`: our number of active slots.
    pub count: Cell<i32>,
    /// `slots`: the slots.
    pub slots: [Cell<i32>; UAO_SWHASH_CLUSTER_SIZE],
}

impl UaoSwhashElt {
    /// An element with no slots (what `pool_get` with `PR_ZERO` hands out).
    pub const fn new() -> Self {
        Self {
            list: ListEntry::new(),
            tag: Cell::new(0),
            count: Cell::new(0),
            slots: [const { Cell::new(0) }; UAO_SWHASH_CLUSTER_SIZE],
        }
    }

    /// `UAO_SWHASH_ELT_PAGESLOT(elt, idx)`: given an ELT and a page index, find the swap
    /// slot.
    fn pageslot(&self, idx: Voff) -> &Cell<i32> {
        &self.slots[uao_swhash_elt_pageslot_idx(idx)]
    }

    /// `UAO_SWHASH_ELT_PAGEIDX_BASE(elt)`: given an ELT, return its pageidx base.
    fn pageidx_base(&self) -> Voff {
        self.tag.get() << UAO_SWHASH_CLUSTER_SHIFT
    }
}

impl Default for UaoSwhashElt {
    fn default() -> Self {
        Self::new()
    }
}

queue_adapter!(
    /// `struct uao_swhash`: the swap hash table structure (one bucket's list).
    pub UaoSwhash: UaoSwhashElt, list => ListEntry<UaoSwhashElt>
);

/// `struct uvm_aobj`: the actual anon-backed uvm_object.
///
/// The uvm_object is at the top of the structure, this allows `(struct uvm_aobj *) ==
/// (struct uvm_object *)`. Only one of `u_swslots` and `u_swhash` is used in any given aobj.
#[repr(C)]
pub struct UvmAobj {
    /// `u_obj`: has: pgops, memt, #pages, #refs.
    pub u_obj: UvmObject,
    /// `u_pages`: number of pages in entire object.
    pub u_pages: Cell<i32>,
    /// `u_flags`: the flags (see above).
    pub u_flags: Cell<i32>,
    /// `u_swslots` (`u_swap.slot_array`): offset -> swapslot mappings of a small aobj.
    pub u_swslots: Cell<*mut i32>,
    /// `u_swhash` (`u_swap.slot_hash`): the hash table (array of bucket heads) of a large
    /// aobj.
    pub u_swhash: Cell<Option<&'static [ListHead<UaoSwhash>]>>,
    /// `u_list`: global list of aobjs.
    pub u_list: ListEntry<UvmAobj>,
}

// SAFETY: `u_obj.vmobjlock` guards the swap bookkeeping, `uao_list_lock` the list link.
unsafe impl Sync for UvmAobj {}

impl UvmAobj {
    /// An empty aobj (the kernel object before `uao_create`).
    pub const fn new() -> Self {
        Self {
            u_obj: UvmObject::new(0),
            u_pages: Cell::new(0),
            u_flags: Cell::new(0),
            u_swslots: Cell::new(ptr::null_mut()),
            u_swhash: Cell::new(None),
            u_list: ListEntry::new(),
        }
    }

    /// `UAO_USES_SWHASH(aobj)`.
    fn uses_swhash(&self) -> bool {
        self.u_pages.get() > UAO_SWHASH_THRESHOLD
    }

    /// `u_swhashmask`: mask for hashtable.
    fn swhashmask(&self) -> usize {
        self.swhash().len() - 1
    }

    /// The hash table of a large aobj.
    fn swhash(&self) -> &'static [ListHead<UaoSwhash>] {
        match self.u_swhash.get() {
            Some(h) => h,
            None => panic(format_args!("uvm_aobj: no swap hash")),
        }
    }

    /// `UAO_SWHASH_HASH(aobj, idx)`: the hash function.
    fn swhash_bucket(&self, idx: Voff) -> &'static ListHead<UaoSwhash> {
        &self.swhash()[((idx >> UAO_SWHASH_CLUSTER_SHIFT) as usize) & self.swhashmask()]
    }

    /// `u_swslots[i]` of a small aobj.
    fn swslot(&self, i: usize) -> &Cell<i32> {
        kassert!((i as i32) < self.u_pages.get());
        // SAFETY: `i` is below `u_pages`, the length of the array `uao_create` made; the
        // array lives until `uao_free`; reading it through a `Cell` keeps the API `&self`.
        unsafe { &*self.u_swslots.get().add(i).cast::<Cell<i32>>() }
    }
}

impl Default for UvmAobj {
    fn default() -> Self {
        Self::new()
    }
}

queue_adapter!(
    /// `aobjlist`: the adapter of `uao_list` (`u_list`).
    pub Aobjlist: UvmAobj, u_list => ListEntry<UvmAobj>
);

/// `(struct uvm_aobj *)uobj`: the aobj an aobj-pager object is the head of.
fn aobj(uobj: &UvmObject) -> &UvmAobj {
    kassert!(uvm_obj_is_aobj(uobj));
    // SAFETY: an object whose pager is `aobj_pager` was made by `uao_create` as the first
    // field of a `#[repr(C)]` `UvmAobj`, so the pointers coincide.
    unsafe { &*ptr::from_ref(uobj).cast::<UvmAobj>() }
}

/// `uao_swhash_elt_pool`: pool of uao_swhash_elt structures.
static UAO_SWHASH_ELT_POOL: Pool = Pool::new();
/// `uvm_aobj_pool`.
static UVM_AOBJ_POOL: Pool = Pool::new();

/// `aobj_pager`: note that some functions (e.g. put) are handled elsewhere.
pub static AOBJ_PAGER: UvmPagerops = UvmPagerops {
    pgo_reference: Some(uao_reference),
    pgo_detach: Some(uao_detach),
    pgo_flush: Some(uao_flush),
    pgo_get: Some(uao_get),
    ..UvmPagerops::empty()
};

/// `uao_list`: global list of active aobjs, locked by `uao_list_lock`.
///
/// Lock ordering: generally the locking order is object lock, then list lock. In the case of
/// swap off we have to iterate over the list, and thus the ordering is reversed. In that
/// case we must use trylocking to prevent deadlock.
struct UaoList(ListHead<Aobjlist>);
// SAFETY: `UAO_LIST_LOCK` guards the list.
unsafe impl Sync for UaoList {}
static UAO_LIST: UaoList = UaoList(ListHead::new());
/// `uao_list_lock`.
static UAO_LIST_LOCK: Mutex = Mutex::new(IPL_MPFLOOR);

/// `kernel_object_store`: the kernel object (`uao_create`).
static KERNEL_OBJECT_STORE: UvmAobj = UvmAobj::new();
/// `bootstrap_kernel_object_lock`.
static BOOTSTRAP_KERNEL_OBJECT_LOCK: Rwlock = Rwlock::new("kobjlk");
/// `kobj_alloced`: how far the kernel object's creation got.
static KOBJ_ALLOCED: AtomicI32 = AtomicI32::new(0);

// hash table/array related functions

/// `uao_find_swhash_elt`: find (or create) a hash table entry for a page offset.
fn uao_find_swhash_elt(
    aobj: &UvmAobj,
    pageidx: Voff,
    create: bool,
    wait: bool,
) -> Option<&'static UaoSwhashElt> {
    let waitf = if wait { PR_WAITOK } else { PR_NOWAIT };

    let swhash = aobj.swhash_bucket(pageidx); // first hash to get bucket
    let page_tag = uao_swhash_elt_tag(pageidx); // tag to search for

    // now search the bucket for the requested tag
    if let Some(elt) = swhash.iter().find(|elt| elt.tag.get() == page_tag) {
        return Some(elt);
    }

    if !create {
        return None;
    }

    // allocate a new entry for the bucket and init/insert it in
    let mem = pool_get(&UAO_SWHASH_ELT_POOL, waitf | PR_ZERO)?;
    let elt = mem.cast::<UaoSwhashElt>();
    // SAFETY: a fresh pool item of `size_of::<UaoSwhashElt>()` bytes, written once; it lives
    // until `uao_set_swslot` or `uao_dropswap_range` returns it.
    let elt: &'static UaoSwhashElt = unsafe {
        elt.as_ptr().write(UaoSwhashElt::new());
        elt.as_ref()
    };
    // SAFETY: an element on no list, under the object lock.
    unsafe { swhash.insert_head(elt) };
    elt.tag.set(page_tag);

    Some(elt)
}

/// `uao_find_swslot`: find the swap slot number for an aobj/pageidx.
pub fn uao_find_swslot(uobj: &UvmObject, pageidx: i32) -> i32 {
    let aobj = aobj(uobj);

    // if noswap flag is set, then we never return a slot
    if aobj.u_flags.get() & UAO_FLAG_NOSWAP != 0 {
        return 0;
    }

    // if hashing, look in hash table.
    if aobj.uses_swhash() {
        return match uao_find_swhash_elt(aobj, pageidx as Voff, false, false) {
            Some(elt) => elt.pageslot(pageidx as Voff).get(),
            None => 0,
        };
    }

    // otherwise, look in the array
    aobj.swslot(pageidx as usize).get()
}

/// `uao_set_swslot`: set the swap slot for a page in an aobj.
///
/// Setting a slot to zero frees the slot. Object must be locked by caller. We return the old
/// slot number, or -1 if we failed to allocate memory to record the new slot number.
pub fn uao_set_swslot(uobj: &UvmObject, pageidx: i32, slot: i32) -> i32 {
    let aobj = aobj(uobj);

    kassert!(rw_write_held(uobj.vmobjlock()) || uobj.uo_refs.get() == 0);

    // if noswap flag is set, then we can't set a slot
    if aobj.u_flags.get() & UAO_FLAG_NOSWAP != 0 {
        if slot == 0 {
            return 0; // a clear is ok
        }

        // but a set is not
        printf(format_args!(
            "uao_set_swslot: uobj = {:p}\n",
            ptr::from_ref(uobj)
        ));
        panic(format_args!(
            "uao_set_swslot: attempt to set a slot on a NOSWAP object"
        ));
    }

    // are we using a hash table?  if so, add it in the hash.
    if aobj.uses_swhash() {
        // Avoid allocating an entry just to free it again if the page had not swap slot in
        // the first place, and we are freeing.
        let Some(elt) = uao_find_swhash_elt(aobj, pageidx as Voff, slot != 0, false) else {
            return if slot != 0 { -1 } else { 0 };
        };

        let oldslot = elt.pageslot(pageidx as Voff).replace(slot);

        // now adjust the elt's reference counter and free it if we've dropped it to zero.
        if slot != 0 {
            if oldslot == 0 {
                elt.count.set(elt.count.get() + 1);
            }
        } else {
            if oldslot != 0 {
                elt.count.set(elt.count.get() - 1);
            }

            if elt.count.get() == 0 {
                // SAFETY: an element on its bucket's list, under the object lock.
                unsafe { ListHead::<UaoSwhash>::remove(elt) };
                pool_put(&UAO_SWHASH_ELT_POOL, NonNull::from(elt).cast::<u8>());
            }
        }
        oldslot
    } else {
        // we are using an array
        aobj.swslot(pageidx as usize).replace(slot)
    }
}

// end of hash/array functions

/// `uao_free`: free all resources held by an aobj, and then free the aobj.
///
/// The aobj should be dead.
fn uao_free(aobj: &UvmAobj) {
    let uobj = &aobj.u_obj;

    kassert!(uvm_obj_is_aobj(uobj));
    kassert!(rw_write_held(uobj.vmobjlock()));
    uao_dropswap_range(uobj, 0, 0);
    rw_exit(uobj.vmobjlock());

    if aobj.uses_swhash() {
        // free the hash table itself.
        // SAFETY: the table `uao_create` made for `u_pages`, emptied by `uao_dropswap_range`
        // above; nothing uses the dead object.
        unsafe {
            hashfree(
                aobj.swhash(),
                uao_swhash_buckets(aobj.u_pages.get()),
                M_UVMAOBJ,
            )
        };
        aobj.u_swhash.set(None);
    } else if let Some(slots) = NonNull::new(aobj.u_swslots.get().cast::<u8>()) {
        free(
            slots,
            M_UVMAOBJ,
            aobj.u_pages.get() as usize * size_of::<i32>(),
        );
        aobj.u_swslots.set(ptr::null_mut());
    }

    // finally free the aobj itself
    uvm_obj_destroy(uobj);
    pool_put(&UVM_AOBJ_POOL, NonNull::from(aobj).cast::<u8>());
}

// pager functions

/// `uao_shrink_flush`: free the pages and drop the swap slots of the pages
/// `[startpg, endpg)` of an aobj that is about to shrink.
///
/// Shrinking an aobj to a given number of pages is always the same procedure: assess the
/// necessity of data structure conversion (hash to array), secure resources, flush pages and
/// drop swap slots.
#[cfg(feature = "tmpfs")]
pub fn uao_shrink_flush(uobj: &UvmObject, startpg: i32, endpg: i32) {
    kassert!(startpg < endpg);
    kassert!(uobj.uo_refs.get() == 1);
    let _ = uao_flush(
        uobj,
        Voff::from(startpg) << PAGE_SHIFT,
        Voff::from(endpg) << PAGE_SHIFT,
        PGO_FREE,
    );
    uao_dropswap_range(uobj, Voff::from(startpg), Voff::from(endpg));
}

/// Moves every swap-hash element of `old` into `new`, each to the bucket its tag hashes to
/// in `new` (see the module's deviations).
#[cfg(feature = "tmpfs")]
fn uao_swhash_move(old: &'static [ListHead<UaoSwhash>], new: &'static [ListHead<UaoSwhash>]) {
    let newmask = new.len() - 1;
    for bucket in old {
        while let Some(elt) = bucket.first() {
            // SAFETY: an element on its bucket's list, under the object lock; it goes on
            // exactly one list of the new table.
            unsafe {
                ListHead::<UaoSwhash>::remove(elt);
                new[(elt.tag.get() as usize) & newmask].insert_head(elt);
            }
        }
    }
}

/// `uao_shrink_hash`: shrink an aobj that keeps a hash table and still needs one.
#[cfg(feature = "tmpfs")]
pub fn uao_shrink_hash(uobj: &UvmObject, pages: i32) -> Result<(), Errno> {
    let aobj = aobj(uobj);

    kassert!(aobj.uses_swhash());

    // If the size of the hash table doesn't change, all we need to do is to adjust the
    // page count.
    if uao_swhash_buckets(aobj.u_pages.get()) == uao_swhash_buckets(pages) {
        uao_shrink_flush(uobj, pages, aobj.u_pages.get());
        aobj.u_pages.set(pages);
        return Ok(());
    }

    let new_swhash =
        hashinit::<UaoSwhash>(uao_swhash_buckets(pages), M_UVMAOBJ, M_WAITOK | M_CANFAIL)
            .ok_or(Errno::ENOMEM)?;

    uao_shrink_flush(uobj, pages, aobj.u_pages.get());

    // Even though the hash table size is changing, the hash of the buckets we are
    // interested in copying should not change.
    let old = aobj.swhash();
    uao_swhash_move(old, new_swhash);

    // SAFETY: the table `uao_create` (or an earlier resize) made for `u_pages`, emptied
    // just above; the object now uses `new_swhash`.
    unsafe { hashfree(old, uao_swhash_buckets(aobj.u_pages.get()), M_UVMAOBJ) };

    aobj.u_swhash.set(Some(new_swhash));
    aobj.u_pages.set(pages);

    Ok(())
}

/// `uao_shrink_convert`: shrink an aobj that keeps a hash table below the threshold, so
/// that its swap slots move to an array.
#[cfg(feature = "tmpfs")]
pub fn uao_shrink_convert(uobj: &UvmObject, pages: i32) -> Result<(), Errno> {
    let aobj = aobj(uobj);

    let new_swslots = mallocarray(
        pages as usize,
        size_of::<i32>(),
        M_UVMAOBJ,
        M_WAITOK | M_CANFAIL | M_ZERO,
    )
    .ok_or(Errno::ENOMEM)?
    .cast::<i32>();

    uao_shrink_flush(uobj, pages, aobj.u_pages.get());

    // Convert swap slots from hash to array.
    for i in 0..pages {
        let Some(elt) = uao_find_swhash_elt(aobj, Voff::from(i), false, false) else {
            continue;
        };
        let slot = elt.pageslot(Voff::from(i)).get();
        // SAFETY: `i` is below `pages`, the length of the zeroed array just allocated.
        unsafe { new_swslots.as_ptr().add(i as usize).write(slot) };
        if slot != 0 {
            elt.count.set(elt.count.get() - 1);
        }
        if elt.count.get() == 0 {
            // SAFETY: an element on its bucket's list, under the object lock.
            unsafe { ListHead::<UaoSwhash>::remove(elt) };
            pool_put(&UAO_SWHASH_ELT_POOL, NonNull::from(elt).cast::<u8>());
        }
    }

    // SAFETY: the object's table for `u_pages`; the flush and the loop above emptied it
    // (every element of `[0, pages)` lost its last slot), and the object now uses the array.
    unsafe {
        hashfree(
            aobj.swhash(),
            uao_swhash_buckets(aobj.u_pages.get()),
            M_UVMAOBJ,
        )
    };
    aobj.u_swhash.set(None);

    aobj.u_swslots.set(new_swslots.as_ptr());
    aobj.u_pages.set(pages);

    Ok(())
}

/// `uao_shrink_array`: shrink an aobj that keeps its swap slots in an array.
#[cfg(feature = "tmpfs")]
pub fn uao_shrink_array(uobj: &UvmObject, pages: i32) -> Result<(), Errno> {
    let aobj = aobj(uobj);

    let new_swslots = mallocarray(
        pages as usize,
        size_of::<i32>(),
        M_UVMAOBJ,
        M_WAITOK | M_CANFAIL | M_ZERO,
    )
    .ok_or(Errno::ENOMEM)?
    .cast::<i32>();

    uao_shrink_flush(uobj, pages, aobj.u_pages.get());

    for i in 0..pages as usize {
        // SAFETY: `i` is below `pages`, the length of the new array.
        unsafe { new_swslots.as_ptr().add(i).write(aobj.swslot(i).get()) };
    }

    if let Some(old) = NonNull::new(aobj.u_swslots.get().cast::<u8>()) {
        free(
            old,
            M_UVMAOBJ,
            aobj.u_pages.get() as usize * size_of::<i32>(),
        );
    }

    aobj.u_swslots.set(new_swslots.as_ptr());
    aobj.u_pages.set(pages);

    Ok(())
}

/// `uao_shrink`: shrink an aobj to `pages` pages, freeing the pages and swap slots past
/// the new end. The object is locked by the caller.
#[cfg(feature = "tmpfs")]
pub fn uao_shrink(uobj: &UvmObject, pages: i32) -> Result<(), Errno> {
    let aobj = aobj(uobj);

    kassert!(pages < aobj.u_pages.get());

    // Distinguish between three possible cases:
    // 1. aobj uses hash and must be converted to array.
    // 2. aobj uses array and array size needs to be adjusted.
    // 3. aobj uses hash and hash size needs to be adjusted.
    if pages > UAO_SWHASH_THRESHOLD {
        uao_shrink_hash(uobj, pages) // case 3
    } else if aobj.u_pages.get() > UAO_SWHASH_THRESHOLD {
        uao_shrink_convert(uobj, pages) // case 1
    } else {
        uao_shrink_array(uobj, pages) // case 2
    }
}

/// `uao_grow_array`: grow an aobj whose swap slots stay in an array.
///
/// Growing an aobj only adjusts the swap slots; the pages themselves come later through
/// `uvm_fault()`. It is thus mandatory that the caller of these functions does not allow
/// faults to happen in case of growth error.
#[cfg(feature = "tmpfs")]
pub fn uao_grow_array(uobj: &UvmObject, pages: i32) -> Result<(), Errno> {
    let aobj = aobj(uobj);

    kassert!(aobj.u_pages.get() <= UAO_SWHASH_THRESHOLD);

    let new_swslots = mallocarray(
        pages as usize,
        size_of::<i32>(),
        M_UVMAOBJ,
        M_WAITOK | M_CANFAIL | M_ZERO,
    )
    .ok_or(Errno::ENOMEM)?
    .cast::<i32>();

    for i in 0..aobj.u_pages.get() as usize {
        // SAFETY: `i` is below the old `u_pages`, itself below `pages`, the length of the
        // new array.
        unsafe { new_swslots.as_ptr().add(i).write(aobj.swslot(i).get()) };
    }

    if let Some(old) = NonNull::new(aobj.u_swslots.get().cast::<u8>()) {
        free(
            old,
            M_UVMAOBJ,
            aobj.u_pages.get() as usize * size_of::<i32>(),
        );
    }

    aobj.u_swslots.set(new_swslots.as_ptr());
    aobj.u_pages.set(pages);

    Ok(())
}

/// `uao_grow_hash`: grow an aobj that keeps a hash table.
#[cfg(feature = "tmpfs")]
pub fn uao_grow_hash(uobj: &UvmObject, pages: i32) -> Result<(), Errno> {
    let aobj = aobj(uobj);

    kassert!(pages > UAO_SWHASH_THRESHOLD);

    // If the size of the hash table doesn't change, all we need to do is to adjust the
    // page count.
    if uao_swhash_buckets(aobj.u_pages.get()) == uao_swhash_buckets(pages) {
        aobj.u_pages.set(pages);
        return Ok(());
    }

    kassert!(uao_swhash_buckets(aobj.u_pages.get()) < uao_swhash_buckets(pages));

    let new_swhash =
        hashinit::<UaoSwhash>(uao_swhash_buckets(pages), M_UVMAOBJ, M_WAITOK | M_CANFAIL)
            .ok_or(Errno::ENOMEM)?;

    let old = aobj.swhash();
    uao_swhash_move(old, new_swhash);

    // SAFETY: the object's table for `u_pages`, emptied just above; the object now uses
    // `new_swhash`.
    unsafe { hashfree(old, uao_swhash_buckets(aobj.u_pages.get()), M_UVMAOBJ) };

    aobj.u_swhash.set(Some(new_swhash));
    aobj.u_pages.set(pages);

    Ok(())
}

/// `uao_grow_convert`: grow an aobj past the threshold, so that its swap slots move from
/// an array to a hash table.
#[cfg(feature = "tmpfs")]
pub fn uao_grow_convert(uobj: &UvmObject, pages: i32) -> Result<(), Errno> {
    let aobj = aobj(uobj);

    let new_swhash =
        hashinit::<UaoSwhash>(uao_swhash_buckets(pages), M_UVMAOBJ, M_WAITOK | M_CANFAIL)
            .ok_or(Errno::ENOMEM)?;

    // Set these now, so we can use uao_find_swhash_elt().
    let old_swslots = aobj.u_swslots.get();
    aobj.u_swhash.set(Some(new_swhash));

    for i in 0..aobj.u_pages.get() {
        let slot = aobj.swslot(i as usize).get();
        if slot != 0 {
            let Some(elt) = uao_find_swhash_elt(aobj, Voff::from(i), true, true) else {
                panic(format_args!("uao_grow_convert: no swap hash element"));
            };
            elt.count.set(elt.count.get() + 1);
            elt.pageslot(Voff::from(i)).set(slot);
        }
    }

    if let Some(old) = NonNull::new(old_swslots.cast::<u8>()) {
        free(
            old,
            M_UVMAOBJ,
            aobj.u_pages.get() as usize * size_of::<i32>(),
        );
    }
    aobj.u_swslots.set(ptr::null_mut());
    aobj.u_pages.set(pages);

    Ok(())
}

/// `uao_grow`: grow an aobj to `pages` pages. The object is locked by the caller.
#[cfg(feature = "tmpfs")]
pub fn uao_grow(uobj: &UvmObject, pages: i32) -> Result<(), Errno> {
    let aobj = aobj(uobj);

    kassert!(pages > aobj.u_pages.get());

    // Distinguish between three possible cases:
    // 1. aobj uses hash and hash size needs to be adjusted.
    // 2. aobj uses array and array size needs to be adjusted.
    // 3. aobj uses array and must be converted to hash.
    if pages <= UAO_SWHASH_THRESHOLD {
        uao_grow_array(uobj, pages) // case 2
    } else if aobj.u_pages.get() > UAO_SWHASH_THRESHOLD {
        uao_grow_hash(uobj, pages) // case 1
    } else {
        uao_grow_convert(uobj, pages)
    }
}

/// `uao_create`: create an aobj of the given size and return its uvm_object.
///
/// For normal use, flags are zero or `UAO_FLAG_CANFAIL`. For the kernel object, the flags
/// are: `UAO_FLAG_KERNOBJ`, allocate the kernel object (can only happen once);
/// `UAO_FLAG_KERNSWAP`, enable swapping of kernel object ("           ").
pub fn uao_create(size: Vsize, flags: i32) -> Option<&'static UvmObject> {
    let pages = (round_page(size.as_usize()) >> PAGE_SHIFT) as i32;
    let aobj: &'static UvmAobj;
    let refs;

    // Allocate a new aobj, unless kernel object is requested.
    if flags & UAO_FLAG_KERNOBJ != 0 {
        kassert!(KOBJ_ALLOCED.load(Ordering::Relaxed) == 0);
        aobj = &KERNEL_OBJECT_STORE;
        aobj.u_pages.set(pages);
        aobj.u_flags.set(UAO_FLAG_NOSWAP);
        refs = UVM_OBJ_KERN;
        KOBJ_ALLOCED.store(UAO_FLAG_KERNOBJ, Ordering::Relaxed);
    } else if flags & UAO_FLAG_KERNSWAP != 0 {
        kassert!(KOBJ_ALLOCED.load(Ordering::Relaxed) == UAO_FLAG_KERNOBJ);
        aobj = &KERNEL_OBJECT_STORE;
        refs = UVM_OBJ_KERN; // unused: this path returns before uvm_obj_init
        KOBJ_ALLOCED.store(UAO_FLAG_KERNSWAP, Ordering::Relaxed);
    } else {
        let Some(mem) = pool_get(&UVM_AOBJ_POOL, PR_WAITOK) else {
            panic(format_args!("uao_create: pool_get failed"));
        };
        let p = mem.cast::<UvmAobj>();
        // SAFETY: a fresh pool item of `size_of::<UvmAobj>()` bytes, written once; it lives
        // until `uao_free` returns it.
        aobj = unsafe {
            p.as_ptr().write(UvmAobj::new());
            p.as_ref()
        };
        aobj.u_pages.set(pages);
        aobj.u_flags.set(0);
        refs = 1;
    }

    // allocate hash/array if necessary
    if flags == 0 || flags & (UAO_FLAG_KERNSWAP | UAO_FLAG_CANFAIL) != 0 {
        let mflags = if flags != 0 { M_NOWAIT } else { M_WAITOK };

        // allocate hash table or array depending on object size
        if aobj.uses_swhash() {
            let swhash = hashinit::<UaoSwhash>(uao_swhash_buckets(pages), M_UVMAOBJ, mflags);
            let Some(swhash) = swhash else {
                if flags & UAO_FLAG_CANFAIL != 0 {
                    pool_put(&UVM_AOBJ_POOL, NonNull::from(aobj).cast::<u8>());
                    return None;
                }
                panic(format_args!("uao_create: hashinit swhash failed"));
            };
            aobj.u_swhash.set(Some(swhash));
        } else {
            let slots = mallocarray(pages as usize, size_of::<i32>(), M_UVMAOBJ, mflags | M_ZERO);
            let Some(slots) = slots else {
                if flags & UAO_FLAG_CANFAIL != 0 {
                    pool_put(&UVM_AOBJ_POOL, NonNull::from(aobj).cast::<u8>());
                    return None;
                }
                panic(format_args!("uao_create: malloc swslots failed"));
            };
            aobj.u_swslots.set(slots.cast::<i32>().as_ptr());
        }

        if flags & UAO_FLAG_KERNSWAP != 0 {
            aobj.u_flags.set(aobj.u_flags.get() & !UAO_FLAG_NOSWAP); // clear noswap
            return Some(&aobj.u_obj);
            // done!
        }
    }

    // Initialise UVM object.
    uvm_obj_init(&aobj.u_obj, Some(&AOBJ_PAGER), refs);
    if flags & UAO_FLAG_KERNOBJ != 0 {
        // Use a temporary static lock for kernel_object.
        rw_init(&BOOTSTRAP_KERNEL_OBJECT_LOCK, "kobjlk");
        uvm_obj_setlock(&aobj.u_obj, Some(&BOOTSTRAP_KERNEL_OBJECT_LOCK));
    }

    // now that aobj is ready, add it to the global list
    mtx_enter(&UAO_LIST_LOCK);
    // SAFETY: an aobj on no list yet, under the list lock.
    unsafe { UAO_LIST.0.insert_head(aobj) };
    mtx_leave(&UAO_LIST_LOCK);

    Some(&aobj.u_obj)
}

/// `uao_init`: set up aobj pager subsystem.
///
/// Called at boot time from `uvm_pager_init()`.
pub fn uao_init() {
    // NOTE: Pages for this pool must not come from a pageable kernel map!
    pool_init(
        &UAO_SWHASH_ELT_POOL,
        size_of::<UaoSwhashElt>(),
        0,
        IPL_NONE,
        PR_WAITOK,
        "uaoeltpl",
        None,
    );
    pool_init(
        &UVM_AOBJ_POOL,
        size_of::<UvmAobj>(),
        0,
        IPL_NONE,
        PR_WAITOK,
        "aobjpl",
        None,
    );
}

/// `uao_reference`: hold a reference to an anonymous UVM object.
pub fn uao_reference(uobj: &UvmObject) {
    // Kernel object is persistent.
    if uvm_obj_is_kern_object(uobj) {
        return;
    }

    uobj.uo_refs.atomic_inc();
}

/// `uao_detach`: drop a reference to an anonymous UVM object.
pub fn uao_detach(uobj: &UvmObject) {
    let aobj = aobj(uobj);

    // Detaching from kernel_object is a NOP.
    if uvm_obj_is_kern_object(uobj) {
        return;
    }

    // Drop the reference.  If it was the last one, destroy the object.
    if uobj.uo_refs.atomic_dec_nv() > 0 {
        return;
    }

    // Remove the aobj from the global list.
    mtx_enter(&UAO_LIST_LOCK);
    // SAFETY: an aobj on the list, under the list lock.
    unsafe { ListHead::<Aobjlist>::remove(aobj) };
    mtx_leave(&UAO_LIST_LOCK);

    // Free all the pages left in the aobj.  For each page, when the page is no longer busy
    // (and thus after any disk I/O that it is involved in is complete), release any swap
    // resources and free the page itself.
    let _ = rw_enter(uobj.vmobjlock(), RW_WRITE);
    while let Some(pg) = uobj.memt.root() {
        pmap_page_protect(pg, PROT_NONE);
        if pg.flags() & PG_BUSY != 0 {
            uvm_pagewait(pg, uobj.vmobjlock(), "uao_det");
            let _ = rw_enter(uobj.vmobjlock(), RW_WRITE);
            continue;
        }
        uao_dropswap(&aobj.u_obj, (pg.offset.get() >> PAGE_SHIFT) as i32);
        uvm_pagefree(pg);
    }

    // Finally, free the anonymous UVM object itself.
    uao_free(aobj);
}

/// `uao_flush`: flush pages out of a uvm object.
///
/// If `PGO_CLEANIT` is not set, then we will not block. If `PGO_ALLPAGE` is set, then all
/// pages in the object are valid targets for flushing. NOTE: we are allowed to lock the page
/// queues, so the caller must not be holding the lock on them (e.g. pagedaemon had better
/// not call us with the queues locked). We return `true` unless we encountered some sort of
/// I/O error (XXXJRT currently never happens, as we never directly initiate I/O).
pub fn uao_flush(uobj: &UvmObject, start: Voff, stop: Voff, flags: i32) -> bool {
    let aobj = aobj(uobj);

    kassert!(rw_write_held(uobj.vmobjlock()));

    let (start, mut stop) = if flags & PGO_ALLPAGES != 0 {
        (0, (aobj.u_pages.get() as Voff) << PAGE_SHIFT)
    } else {
        (
            trunc_page(start as usize) as Voff,
            round_page(stop as usize) as Voff,
        )
    };
    if flags & PGO_ALLPAGES == 0 && stop > ((aobj.u_pages.get() as Voff) << PAGE_SHIFT) {
        printf(format_args!(
            "uao_flush: strange, got an out of range flush (fixed)\n"
        ));
        stop = (aobj.u_pages.get() as Voff) << PAGE_SHIFT;
    }

    // Don't need to do any work here if we're not freeing or deactivating pages.
    if flags & (PGO_DEACTIVATE | PGO_FREE) == 0 {
        return true;
    }

    let mut curoff = start;
    while curoff < stop {
        let pg = uvm_pagelookup(uobj, curoff);
        curoff += PAGE_SIZE as Voff;
        let Some(pg) = pg else {
            continue;
        };

        // Make sure page is unbusy, else wait for it.
        if pg.flags() & PG_BUSY != 0 {
            uvm_pagewait(pg, uobj.vmobjlock(), "uaoflsh");
            let _ = rw_enter(uobj.vmobjlock(), RW_WRITE);
            curoff -= PAGE_SIZE as Voff;
            continue;
        }

        let what = flags & (PGO_CLEANIT | PGO_FREE | PGO_DEACTIVATE);
        // XXX In the first 3 cases, we always just deactivate the page. We may want to
        // handle the different cases more specifically in the future.
        let deactivate = what == PGO_CLEANIT | PGO_FREE
            || what == PGO_CLEANIT | PGO_DEACTIVATE
            || what == PGO_DEACTIVATE
            // If there are multiple references to the object, just deactivate the page.
            || (what == PGO_FREE && uobj.uo_refs.get() > 1);
        if deactivate {
            // deactivate_it:
            uvm_pagedeactivate(pg);
            continue;
        }
        if what != PGO_FREE {
            panic(format_args!("uao_flush: weird flags"));
        }

        // XXX skip the page if it's wired
        if pg.wire_count.get() != 0 {
            continue;
        }

        // free the swap slot and the page.
        pmap_page_protect(pg, PROT_NONE);

        // freeing swapslot here is not strictly necessary. however, leaving it here doesn't
        // save much because we need to update swap accounting anyway.
        uao_dropswap(uobj, (pg.offset.get() >> PAGE_SHIFT) as i32);
        uvm_pagefree(pg);
    }

    true
}

/// `uao_get_validate`: `PGO_ALLPAGES` makes every requested page mandatory; otherwise only
/// centeridx must remain within the object, because adjacent pages are fault clustering
/// candidates rather than required fetches. Validate this distinction before allocation
/// because pageidx directly indexes swap metadata; an out of range value can therefore
/// access storage beyond the object's extent.
#[inline]
fn uao_get_validate(
    aobj: &UvmAobj,
    firstpage: Voff,
    maxpages: i32,
    centeridx: i32,
    flags: i32,
) -> bool {
    let u_pages = aobj.u_pages.get() as Voff;
    if maxpages <= 0 || firstpage < 0 || firstpage >= u_pages {
        return false;
    }

    if flags & PGO_ALLPAGES != 0 {
        return (maxpages as Voff) <= u_pages - firstpage;
    }

    centeridx >= 0 && centeridx < maxpages && (centeridx as Voff) < u_pages - firstpage
}

/// `uao_get`: fetch me a page.
///
/// We have three cases: 1: page is resident -> just return the page. 2: page is zero-fill ->
/// allocate a new page and zero it. 3: page is swapped out -> fetch the page from swap.
///
/// Cases 1 can be handled with `PGO_LOCKED`, cases 2 and 3 cannot. So, if the "center" page
/// hits case 3 (or any page, with `PGO_ALLPAGES`), then we will need to return
/// `VM_PAGER_UNLOCK`.
///
/// Flags: `PGO_ALLPAGES`: get all of the pages; `PGO_LOCKED`: fault data structures are
/// locked. NOTE: offset is the offset of pps\[0\], _NOT_ pps\[centeridx\]. NOTE: caller must
/// check for released pages!!
#[allow(clippy::too_many_arguments)] // the C's signature
pub fn uao_get(
    uobj: &UvmObject,
    offset: Voff,
    pps: &mut [*const VmPage],
    npagesp: &mut i32,
    centeridx: i32,
    access_type: VmProt,
    _advice: i32,
    flags: i32,
) -> i32 {
    let aobj = aobj(uobj);

    kassert!(rw_lock_held(uobj.vmobjlock()));
    kassert!(
        rw_write_held(uobj.vmobjlock())
            || (flags & PGO_LOCKED != 0 && access_type & PROT_WRITE == 0)
    );

    // get number of pages
    let maxpages = *npagesp;
    let firstpage = offset >> PAGE_SHIFT;
    if !uao_get_validate(aobj, firstpage, maxpages, centeridx, flags) {
        *npagesp = 0;
        if flags & PGO_LOCKED == 0 {
            rw_exit(uobj.vmobjlock());
        }
        return VM_PAGER_BAD;
    }
    kassert!(pps.len() >= maxpages as usize);

    if flags & PGO_LOCKED != 0 {
        // step 1a: get pages that are already resident.   only do this if the data
        // structures are locked (i.e. the first time through).
        let mut done = true; // be optimistic
        let mut gotpages = 0; // # of pages we got so far

        let mut current_offset = offset;
        for (lcv, slot) in pps.iter_mut().enumerate().take(maxpages as usize) {
            // do we care about this page?  if not, skip it
            if pgo_dontcare(*slot) {
                current_offset += PAGE_SIZE as Voff;
                continue;
            }

            // lookup page
            let ptmp = uvm_pagelookup(uobj, current_offset);
            current_offset += PAGE_SIZE as Voff;

            // to be useful must get a non-busy page
            let Some(ptmp) = ptmp.filter(|p| p.flags() & PG_BUSY == 0) else {
                if lcv as i32 == centeridx {
                    // need to do a wait or I/O!
                    done = false;
                }
                if flags & PGO_ALLPAGES != 0 {
                    done = false;
                    break;
                }
                continue;
            };

            // useful page: plug it in our result array
            *slot = ptmp;
            gotpages += 1;
        }

        // step 1b: now we've either done everything needed or we to unlock and do some
        // waiting or I/O.
        *npagesp = gotpages;
        return if done { VM_PAGER_OK } else { VM_PAGER_UNLOCK };
    }

    // step 2: get non-resident or busy pages. data structures are unlocked.
    let mut current_offset = offset;
    for (lcv, slot) in pps.iter_mut().enumerate().take(maxpages as usize) {
        let page_offset = current_offset;
        current_offset += PAGE_SIZE as Voff;

        // - skip over pages we've already gotten or don't want
        // - skip over pages we don't _have_ to get
        if !slot.is_null() || (lcv as i32 != centeridx && flags & PGO_ALLPAGES == 0) {
            continue;
        }

        let pageidx = (firstpage + lcv as Voff) as i32;

        // we have yet to locate the current page (pps[lcv]).   we first look for a page that
        // is already at the current offset. if we find a page, we check to see if it is busy
        // or released.  if that is the case, then we sleep on the page until it is no longer
        // busy or released and repeat the lookup. if the page we found is neither busy nor
        // released, then we busy it (so we own it) and plug it into pps[lcv].   this
        // 'break's the following while loop and indicates we are ready to move on to the
        // next page in the "lcv" loop above.
        //
        // if we exit the while loop with pps[lcv] still set to NULL, then it means that we
        // allocated a new busy/fake/clean page ptmp in the object and we need to do I/O to
        // fill in the data.

        // top of "pps" while loop
        let ptmp: &VmPage = loop {
            // look for a resident page
            let ptmp = uvm_pagelookup(uobj, page_offset);

            // not resident?   allocate one now (if we can)
            let Some(ptmp) = ptmp else {
                let Some(ptmp) = uvm_pagealloc(Some(uobj), page_offset, None, 0) else {
                    // out of RAM?
                    rw_exit(uobj.vmobjlock());
                    uvm_wait("uao_getpage");
                    let _ = rw_enter(uobj.vmobjlock(), RW_WRITE);
                    // goto top of pps while loop
                    continue;
                };

                // safe with PQ's unlocked: because we just alloc'd the page
                ptmp.set_bits(PQ_AOBJ);

                // got new page ready for I/O.  break pps while loop.  pps[lcv] is still NULL.
                break ptmp;
            };

            // page is there, see if we need to wait on it
            if ptmp.flags() & PG_BUSY != 0 {
                uvm_pagewait(ptmp, uobj.vmobjlock(), "uao_get");
                let _ = rw_enter(uobj.vmobjlock(), RW_WRITE);
                continue; // goto top of pps while loop
            }

            // if we get here then the page is resident and unbusy.  we busy it now (so we
            // own it).
            // we own it, caller must un-busy
            ptmp.set_bits(PG_BUSY);
            // UVM_PAGE_OWN(ptmp, "uao_get2"): not configured.
            *slot = ptmp;
            break ptmp;
        };

        // if we own the valid page at the correct offset, pps[lcv] will point to it.
        // nothing more to do except go to the next page.
        if !slot.is_null() {
            continue; // next lcv
        }

        // we have a "fake/busy/clean" page that we just allocated. do the needed "i/o",
        // either reading from swap or zeroing.
        let swslot = uao_find_swslot(uobj, pageidx);

        // just zero the page if there's nothing in swap.
        if swslot == 0 {
            // page hasn't existed before, just zero it.
            uvm_pagezero(ptmp);
        } else {
            // page in the swapped-out page. unlock object for i/o, relock when done.
            rw_exit(uobj.vmobjlock());
            // rv = uvm_swap_get(ptmp, swslot, PGO_SYNCIO): swap (M7); an error here.
            let _ = unported!("uao_get: uvm_swap_get (swap, M7)");
            let rv = VM_PAGER_ERROR;
            let _ = PGO_SYNCIO;
            let _ = rw_enter(uobj.vmobjlock(), RW_WRITE);

            // I/O done.  check for errors.
            if rv != VM_PAGER_OK {
                // remove the swap slot from the aobj and mark the aobj as having no real
                // slot.  don't free the swap slot, thus preventing it from being used again:
                // uao_set_swslot(&aobj->u_obj, pageidx, SWSLOT_BAD); uvm_swap_markbad(swslot,
                // 1) (swap, M7).
                let _ = unported!("uao_get: SWSLOT_BAD / uvm_swap_markbad (swap, M7)");

                if ptmp.flags() & PG_WANTED != 0 {
                    wakeup(ptr::from_ref(ptmp));
                }
                ptmp.clear_bits(PG_WANTED | PG_BUSY);
                // UVM_PAGE_OWN(ptmp, NULL): not configured.
                uvm_pagefree(ptmp);
                rw_exit(uobj.vmobjlock());

                return rv;
            }
        }

        // we got the page!   clear the fake flag (indicates valid data now in page) and plug
        // into our result array.   note that page is still busy.
        //
        // it is the callers job to: check if the page is released, unbusy the page, activate
        // the page.
        ptmp.clear_bits(PG_FAKE);
        pmap_clear_modify(ptmp); // ... and clean
        *slot = ptmp;
    } // lcv loop

    rw_exit(uobj.vmobjlock());
    VM_PAGER_OK
}

/// `uao_dropswap`: release any swap resources from this aobj page.
///
/// Aobj must be locked or have a reference count of 0.
pub fn uao_dropswap(uobj: &UvmObject, pageidx: i32) -> i32 {
    kassert!(uvm_obj_is_aobj(uobj));

    let slot = uao_set_swslot(uobj, pageidx, 0);
    if slot != 0 {
        // uvm_swap_free(slot, 1): swap (M7).
        let _ = unported!("uao_dropswap: uvm_swap_free (swap, M7)");
    }
    slot
}

/// `uao_swap_off`: page in every page in every aobj that is paged-out to a range of swslots.
///
/// Aobj must be locked and is returned locked. Returns `true` if pagein was aborted due to
/// lack of memory.
pub fn uao_swap_off(startslot: i32, endslot: i32) -> bool {
    // Walk the list of all anonymous UVM objects.  Grab the first.
    mtx_enter(&UAO_LIST_LOCK);
    let Some(mut aobj) = UAO_LIST.0.first() else {
        mtx_leave(&UAO_LIST_LOCK);
        return false;
    };
    uao_reference(&aobj.u_obj);

    loop {
        // Prefetch the next object and immediately hold a reference on it, so neither the
        // current nor the next entry could disappear while we are iterating.
        let nextaobj = ListHead::<Aobjlist>::next(aobj);
        if let Some(next) = nextaobj {
            uao_reference(&next.u_obj);
        }
        mtx_leave(&UAO_LIST_LOCK);

        // Page in all pages in the swap slot range.
        let _ = rw_enter(aobj.u_obj.vmobjlock(), RW_WRITE);
        let rv = uao_pagein(aobj, startslot, endslot);
        rw_exit(aobj.u_obj.vmobjlock());

        // Drop the reference of the current object.
        uao_detach(&aobj.u_obj);
        if rv {
            if let Some(next) = nextaobj {
                uao_detach(&next.u_obj);
            }
            return rv;
        }

        let Some(next) = nextaobj else {
            break;
        };
        aobj = next;
        mtx_enter(&UAO_LIST_LOCK);
    }

    // done with traversal, unlock the list
    mtx_leave(&UAO_LIST_LOCK);
    false
}

/// `uao_pagein`: page in any pages from aobj in the given range.
///
/// Returns `true` if pagein was aborted due to lack of memory.
fn uao_pagein(aobj: &UvmAobj, startslot: i32, endslot: i32) -> bool {
    if aobj.uses_swhash() {
        'restart: loop {
            for bucket in (0..=aobj.swhashmask()).rev() {
                for elt in aobj.swhash()[bucket].iter() {
                    for i in 0..UAO_SWHASH_CLUSTER_SIZE {
                        let slot = elt.slots[i].get();

                        // if the slot isn't in range, skip it.
                        if slot < startslot || slot >= endslot {
                            continue;
                        }

                        // process the page, the start over on this object since the swhash
                        // elt may have been freed.
                        let rv = uao_pagein_page(aobj, (elt.pageidx_base() as i32) + i as i32);
                        if rv {
                            return rv;
                        }
                        continue 'restart;
                    }
                }
            }
            break;
        }
    } else {
        for i in 0..aobj.u_pages.get() as usize {
            let slot = aobj.swslot(i).get();

            // if the slot isn't in range, skip it
            if slot < startslot || slot >= endslot {
                continue;
            }

            // process the page.
            let rv = uao_pagein_page(aobj, i as i32);
            if rv {
                return rv;
            }
        }
    }

    false
}

/// `uao_pagein_page`: page in a single page from an anonymous UVM object.
///
/// Returns `true` if pagein was aborted due to lack of memory.
fn uao_pagein_page(aobj: &UvmAobj, pageidx: i32) -> bool {
    let uobj = &aobj.u_obj;
    let mut pg: [*const VmPage; 1] = [ptr::null()];
    let mut npages = 1;

    kassert!(rw_write_held(uobj.vmobjlock()));
    let rv = uao_get(
        &aobj.u_obj,
        (pageidx as Voff) << PAGE_SHIFT,
        &mut pg,
        &mut npages,
        0,
        PROT_READ | PROT_WRITE,
        0,
        0,
    );

    // relock and finish up.
    let _ = rw_enter(uobj.vmobjlock(), RW_WRITE);
    match rv {
        VM_PAGER_OK => {}
        // nothing more to do on errors. VM_PAGER_REFAULT can only mean that the anon was
        // freed, so again there's nothing to do.
        VM_PAGER_ERROR | VM_PAGER_REFAULT => return false,
        _ => {}
    }
    // SAFETY: `uao_get` returned OK, so the slot holds a page of the object, busy and ours.
    let Some(pg) = (unsafe { pg[0].as_ref() }) else {
        panic(format_args!("uao_pagein_page: no page"));
    };

    // ok, we've got the page now. mark it as dirty, clear its swslot and un-busy it.
    uao_dropswap(&aobj.u_obj, pageidx);
    pg.clear_bits(PG_BUSY | PG_CLEAN | PG_FAKE);
    // UVM_PAGE_OWN(pg, NULL): not configured.

    // deactivate the page (to put it on a page queue).
    uvm_pagedeactivate(pg);

    false
}

/// `uao_dropswap_range`: drop swapslots in the range.
///
/// Aobj must be locked and is returned locked. `start` is inclusive, `end` is exclusive.
pub fn uao_dropswap_range(uobj: &UvmObject, start: Voff, end: Voff) {
    let aobj = aobj(uobj);
    let mut swpgonlydelta = 0;

    kassert!(rw_write_held(uobj.vmobjlock()));

    let end = if end == 0 { i64::MAX } else { end };

    if aobj.uses_swhash() {
        let hashbuckets = aobj.swhashmask() + 1;

        let taglo = uao_swhash_elt_tag(start);
        let taghi = uao_swhash_elt_tag(end);

        for i in 0..hashbuckets {
            let mut elt = aobj.swhash()[i].first();
            while let Some(e) = elt {
                let next = ListHead::<UaoSwhash>::next(e);
                elt = next;

                if e.tag.get() < taglo || taghi < e.tag.get() {
                    continue;
                }

                let startidx = if e.tag.get() == taglo {
                    uao_swhash_elt_pageslot_idx(start)
                } else {
                    0
                };

                let endidx = if e.tag.get() == taghi {
                    uao_swhash_elt_pageslot_idx(end)
                } else {
                    UAO_SWHASH_CLUSTER_SIZE
                };

                for j in startidx..endidx {
                    let slot = e.slots[j].get();

                    kassert!(
                        uvm_pagelookup(&aobj.u_obj, (e.pageidx_base() + j as Voff) << PAGE_SHIFT)
                            .is_none()
                    );

                    if slot > 0 {
                        // uvm_swap_free(slot, 1): swap (M7).
                        let _ = unported!("uao_dropswap_range: uvm_swap_free (swap, M7)");
                        swpgonlydelta += 1;
                        kassert!(e.count.get() > 0);
                        e.slots[j].set(0);
                        e.count.set(e.count.get() - 1);
                    }
                }

                if e.count.get() == 0 {
                    // SAFETY: an element on its bucket's list, under the object lock.
                    unsafe { ListHead::<UaoSwhash>::remove(e) };
                    pool_put(&UAO_SWHASH_ELT_POOL, NonNull::from(e).cast::<u8>());
                }
            }
        }
    } else {
        let end = end.min(aobj.u_pages.get() as Voff);
        for i in start..end {
            let slot = aobj.swslot(i as usize).get();

            if slot > 0 {
                // uvm_swap_free(slot, 1): swap (M7).
                let _ = unported!("uao_dropswap_range: uvm_swap_free (swap, M7)");
                swpgonlydelta += 1;
            }
        }
    }

    // adjust the counter of pages only in swap for all the swap slots we've freed.
    if swpgonlydelta > 0 {
        kassert!(UVMEXP.swpgonly.load(Ordering::Relaxed) >= swpgonlydelta);
        UVMEXP.swpgonly.fetch_sub(swpgonlydelta, Ordering::Relaxed);
    }
}
/* </CODE> */

/* <TESTS> */
#[cfg(test)]
mod tests {
    // Host tests for the anonymous object pager over real memory (see `subr_pool.rs` for
    // the setup).

    use std::sync::MutexGuard;
    use std::{assert, assert_eq};

    use super::*;
    use crate::kern::kern_rwlock::rw_obj_init;
    use crate::kern::subr_pool::tests::setup_real_memory;
    use crate::uvm::uvm_page::uvm_page_unbusy;
    use crate::uvm::uvm_pager::PGO_DONTCARE;

    /// Real memory, the lock objects and the aobj pools.
    fn setup() -> MutexGuard<'static, ()> {
        let guard = setup_real_memory();
        rw_obj_init();
        uao_init();
        guard
    }

    #[test]
    fn small_aobj_keeps_swap_slots_in_an_array() {
        let _g = setup();
        let uobj = uao_create(Vsize::new(4 * PAGE_SIZE), 0).expect("an aobj");
        let aobj = aobj(uobj);
        assert!(uvm_obj_is_aobj(uobj));
        assert!(!aobj.uses_swhash());
        assert_eq!(aobj.u_pages.get(), 4);
        assert_eq!(uobj.uo_refs.get(), 1);

        let _ = rw_enter(uobj.vmobjlock(), RW_WRITE);
        assert_eq!(uao_find_swslot(uobj, 1), 0);
        assert_eq!(uao_set_swslot(uobj, 1, 7), 0);
        assert_eq!(uao_find_swslot(uobj, 1), 7);
        assert_eq!(uao_set_swslot(uobj, 1, 9), 7);
        assert_eq!(uao_dropswap(uobj, 1), 9);
        assert_eq!(uao_find_swslot(uobj, 1), 0);
        rw_exit(uobj.vmobjlock());

        uao_reference(uobj);
        assert_eq!(uobj.uo_refs.get(), 2);
        uao_detach(uobj);
        assert_eq!(uobj.uo_refs.get(), 1);
        uao_detach(uobj);
    }

    #[test]
    fn large_aobj_hashes_swap_slots() {
        let _g = setup();
        let pages = (UAO_SWHASH_THRESHOLD + 1) as usize;
        let uobj = uao_create(Vsize::new(pages * PAGE_SIZE), 0).expect("an aobj");
        let aobj = aobj(uobj);
        assert!(aobj.uses_swhash());
        assert_eq!(aobj.swhashmask(), 3, "65 pages hash into 4 buckets");

        let _ = rw_enter(uobj.vmobjlock(), RW_WRITE);
        assert_eq!(uao_set_swslot(uobj, 3, 11), 0);
        assert_eq!(uao_set_swslot(uobj, 5, 12), 0); // the same cluster
        assert_eq!(uao_set_swslot(uobj, 40, 13), 0); // another one
        let elt = uao_find_swhash_elt(aobj, 3, false, false).expect("an element");
        assert_eq!(elt.count.get(), 2);
        assert_eq!(uao_find_swslot(uobj, 5), 12);
        assert_eq!(uao_find_swslot(uobj, 40), 13);
        assert_eq!(uao_find_swslot(uobj, 41), 0);

        assert_eq!(uao_set_swslot(uobj, 3, 0), 11);
        assert_eq!(elt.count.get(), 1);
        assert_eq!(uao_set_swslot(uobj, 5, 0), 12);
        assert!(
            uao_find_swhash_elt(aobj, 3, false, false).is_none(),
            "an element with no slots left goes back to the pool"
        );

        // the slot at 40 stands for a page only in swap; dropping the range accounts for it
        UVMEXP.swpgonly.fetch_add(1, Ordering::Relaxed);
        uao_dropswap_range(uobj, 0, 0);
        assert_eq!(uao_find_swslot(uobj, 40), 0);
        assert_eq!(UVMEXP.swpgonly.load(Ordering::Relaxed), 0);
        rw_exit(uobj.vmobjlock());

        uao_detach(uobj);
    }

    #[test]
    fn uao_get_zero_fills_and_finds_resident_pages() {
        let _g = setup();
        let uobj = uao_create(Vsize::new(4 * PAGE_SIZE), 0).expect("an aobj");
        let off = PAGE_SIZE as Voff;
        let mut pps: [*const VmPage; 1] = [ptr::null()];
        let mut npages = 1;

        // PGO_LOCKED on a page that is not resident: the caller must unlock and come back
        let _ = rw_enter(uobj.vmobjlock(), RW_WRITE);
        let rv = uao_get(
            uobj,
            off,
            &mut pps,
            &mut npages,
            0,
            PROT_READ,
            0,
            PGO_LOCKED,
        );
        assert_eq!(rv, VM_PAGER_UNLOCK);
        assert_eq!(npages, 0);
        assert!(pps[0].is_null());

        // the full path allocates a zero-filled page, busy for the caller, and drops the lock
        npages = 1;
        let rv = uao_get(
            uobj,
            off,
            &mut pps,
            &mut npages,
            0,
            PROT_READ | PROT_WRITE,
            0,
            0,
        );
        assert_eq!(rv, VM_PAGER_OK);
        // SAFETY: `uao_get` returned OK, so the slot holds a page of the object.
        let pg = unsafe { pps[0].as_ref() }.expect("a page");
        assert!(pg.flags() & PG_BUSY != 0);
        assert_eq!(pg.flags() & PG_FAKE, 0);
        assert!(pg.flags() & PQ_AOBJ != 0);
        assert_eq!(pg.offset.get(), off);
        assert!(pg.uobject().is_some_and(|o| ptr::eq(o, uobj)));
        assert_eq!(uobj.uo_npages.get(), 1);

        // once unbusied, PGO_LOCKED finds it and leaves the PGO_DONTCARE slot alone
        let _ = rw_enter(uobj.vmobjlock(), RW_WRITE);
        uvm_page_unbusy(&[Some(pg)]);
        let mut pps2: [*const VmPage; 2] = [PGO_DONTCARE, ptr::null()];
        let mut n2 = 2;
        let rv = uao_get(uobj, 0, &mut pps2, &mut n2, 1, PROT_READ, 0, PGO_LOCKED);
        assert_eq!(rv, VM_PAGER_OK);
        assert_eq!(n2, 1);
        assert!(ptr::eq(pps2[1], pg));
        assert!(pgo_dontcare(pps2[0]));

        // past the object: VM_PAGER_BAD, and the lock is dropped
        let mut pps3: [*const VmPage; 1] = [ptr::null()];
        let mut n3 = 1;
        let rv = uao_get(uobj, 4 * off, &mut pps3, &mut n3, 0, PROT_READ, 0, 0);
        assert_eq!(rv, VM_PAGER_BAD);
        assert_eq!(n3, 0);
        assert!(!rw_write_held(uobj.vmobjlock()));

        // the last reference frees the page with the object
        uao_detach(uobj);
    }

    #[cfg(feature = "tmpfs")]
    #[test]
    fn grow_and_shrink_keep_the_swap_slots_below_the_end() {
        let _g = setup();
        let uobj = uao_create(Vsize::new(4 * PAGE_SIZE), 0).expect("an aobj");
        let aobj = aobj(uobj);

        let _ = rw_enter(uobj.vmobjlock(), RW_WRITE);
        assert_eq!(uao_set_swslot(uobj, 1, 7), 0);
        assert_eq!(uao_set_swslot(uobj, 3, 9), 0);

        // case 2: the array grows
        uao_grow(uobj, 8).expect("grow the array");
        assert!(!aobj.uses_swhash());
        assert_eq!(aobj.u_pages.get(), 8);
        assert_eq!(uao_find_swslot(uobj, 1), 7);
        assert_eq!(uao_find_swslot(uobj, 7), 0);

        // case 3: past the threshold the slots move to a hash table
        uao_grow(uobj, 100).expect("convert to a hash");
        assert!(aobj.uses_swhash());
        assert!(aobj.u_swslots.get().is_null());
        assert_eq!(uao_find_swslot(uobj, 1), 7);
        assert_eq!(uao_find_swslot(uobj, 3), 9);

        // case 1: a bigger table, the elements rehashed by their tags
        uao_grow(uobj, 300).expect("grow the hash");
        assert_eq!(aobj.swhashmask(), 31, "18 buckets round up to 32");
        assert_eq!(uao_set_swslot(uobj, 290, 11), 0);
        assert_eq!(uao_set_swslot(uobj, 70, 12), 0);
        assert_eq!(uao_find_swslot(uobj, 3), 9);

        // shrinking the hash drops the slots past the end (a page only in swap each)
        UVMEXP.swpgonly.fetch_add(1, Ordering::Relaxed);
        uao_shrink(uobj, 200).expect("shrink the hash");
        assert_eq!(aobj.u_pages.get(), 200);
        assert_eq!(UVMEXP.swpgonly.load(Ordering::Relaxed), 0);
        assert_eq!(uao_find_swslot(uobj, 70), 12);
        assert_eq!(uao_find_swslot(uobj, 1), 7);

        // case 1 of shrink: back to an array below the threshold
        UVMEXP.swpgonly.fetch_add(1, Ordering::Relaxed);
        uao_shrink(uobj, 10).expect("convert to an array");
        assert!(!aobj.uses_swhash());
        assert!(aobj.u_swhash.get().is_none());
        assert_eq!(UVMEXP.swpgonly.load(Ordering::Relaxed), 0);
        assert_eq!(uao_find_swslot(uobj, 1), 7);
        assert_eq!(uao_find_swslot(uobj, 3), 9);

        // case 2 of shrink: a smaller array (the slot at 3 goes)
        UVMEXP.swpgonly.fetch_add(1, Ordering::Relaxed);
        uao_shrink(uobj, 2).expect("shrink the array");
        assert_eq!(UVMEXP.swpgonly.load(Ordering::Relaxed), 0);
        assert_eq!(aobj.u_pages.get(), 2);
        assert_eq!(uao_find_swslot(uobj, 1), 7);
        assert_eq!(uao_set_swslot(uobj, 1, 0), 7);
        rw_exit(uobj.vmobjlock());

        uao_detach(uobj);
    }

    #[cfg(feature = "tmpfs")]
    #[test]
    fn shrink_frees_the_resident_pages_past_the_end() {
        let _g = setup();
        let uobj = uao_create(Vsize::new(4 * PAGE_SIZE), 0).expect("an aobj");
        for idx in [0, 3] {
            let mut pps: [*const VmPage; 1] = [ptr::null()];
            let mut npages = 1;
            let _ = rw_enter(uobj.vmobjlock(), RW_WRITE);
            let rv = uao_get(
                uobj,
                (idx * PAGE_SIZE) as Voff,
                &mut pps,
                &mut npages,
                0,
                PROT_READ | PROT_WRITE,
                0,
                0,
            );
            assert_eq!(rv, VM_PAGER_OK);
            // SAFETY: `uao_get` returned OK, so the slot holds a page of the object.
            let pg = unsafe { pps[0].as_ref() }.expect("a page");
            let _ = rw_enter(uobj.vmobjlock(), RW_WRITE);
            uvm_page_unbusy(&[Some(pg)]);
            rw_exit(uobj.vmobjlock());
        }
        assert_eq!(uobj.uo_npages.get(), 2);

        let _ = rw_enter(uobj.vmobjlock(), RW_WRITE);
        uao_shrink(uobj, 1).expect("shrink");
        assert_eq!(uobj.uo_npages.get(), 1, "the page at index 3 is gone");
        assert!(uvm_pagelookup(uobj, 0).is_some());
        uao_grow(uobj, 4).expect("grow");
        assert_eq!(uobj.uo_npages.get(), 1);
        rw_exit(uobj.vmobjlock());

        uao_detach(uobj);
    }
}
/* </TESTS> */
