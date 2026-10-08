/*	$OpenBSD: uvm_amap.h,v 1.36 2025/05/25 01:52:00 gnezdo Exp $	*/
/*	$NetBSD: uvm_amap.h,v 1.14 2001/02/18 21:19:08 chs Exp $	*/
/*	$OpenBSD: uvm_amap.c,v 1.101 2026/08/12 14:49:25 gnezdo Exp $	*/
/*	$NetBSD: uvm_amap.c,v 1.27 2000/11/25 06:27:59 chs Exp $	*/
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

/* <CODE> */
//! `uvm_amap.h`: general amap interface and amap implementation-specific info, and
//! `uvm_amap.c`: amap operations.
//!
//! Upstream: sys/uvm/uvm_amap.h @ 3ce1f3f79392
//! Upstream: sys/uvm/uvm_amap.c @ 3ce1f3f79392
//!
//! An amap structure contains pointers to a set of anons that are mapped together in virtual
//! memory (an anon is a single page of anonymous virtual memory, see `uvm_anon.rs`). The
//! entries in an amap are called slots; the slots are clustered into chunks of
//! `UVM_AMAP_CHUNK` slots each (`struct vm_amap_chunk`): an array of pointers to `vm_anon`
//! and a bitmap of the slots in use. Small amaps of up to `UVM_AMAP_CHUNK` slots have the
//! chunk directly embedded in the amap; normal amaps organize chunks in a hash table of
//! buckets, with every chunk on one list too. `ppref` is an optional per-page reference
//! count array used when only part of an amap is referenced (see `amap_pp_adjref`).
//!
//! Milestone M7a (part 1) ported the whole file; `amap_copy`'s chunking of a large amap
//! (`UVM_MAP_CLIP_START`/`END` over `uvm_map_clip_*_at`) followed in M12+, when
//! `cargo xtask diff-openbsd` found its stub still printing after the clip functions had
//! been ported.
//!
//! ## Deviations
//! - A chunk always has `UVM_AMAP_CHUNK` anon slots: the C's flexible `ac_anon[]` and the
//!   sixteen `uvm_small_amap_pool[]`s sized by slot count collapse into one amap pool and one
//!   chunk pool. The `am_impl` union is two fields (`am_small` next to the normal amap's
//!   buckets and chunk list).
//! - Swap off (`amap_swap_off`) runs over the list but `uvm_anon_pagein` reports its fault
//!   path (M7a part 3); `UVM_PAGE_OWN` is not configured.

use core::cell::Cell;
use core::ptr::{self, NonNull};

use crate::kassert;
use crate::kern::kern_malloc::{free, mallocarray};
use crate::kern::kern_rwlock::{
    rw_enter, rw_enter_write, rw_exit, rw_exit_write, rw_obj_alloc, rw_obj_free, rw_obj_hold,
};
use crate::kern::subr_pool::{pool_get, pool_init, pool_put, pool_sethiwat};
use crate::kern::subr_prf::panic;
use crate::machine::intr::IPL_MPFLOOR;
use crate::machine::pmap::pmap_page_protect;
use crate::queue_adapter;
use crate::sys::limits::INT_MAX;
use crate::sys::malloc::{M_NOWAIT, M_UVMAMAP, M_WAITOK, M_ZERO};
use crate::sys::mman::PROT_NONE;
use crate::sys::param::{PAGE_SHIFT, PAGE_SIZE};
use crate::sys::pool::{PR_NOWAIT, PR_WAITOK, PR_ZERO, Pool};
use crate::sys::queue::{ListEntry, ListHead, TailqEntry, TailqHead};
use crate::sys::rwlock::{RW_WRITE, Rwlock, rw_write_held};
use crate::uvm::uvm::UVM_ET_NEEDSCOPY;
use crate::uvm::uvm_anon::{VmAnon, VmAref, uvm_analloc, uvm_anfree, uvm_anon_pagein};
use crate::uvm::uvm_map::{VmMap, VmMapEntry, uvm_map_clip_end_at, uvm_map_clip_start_at};
use crate::uvm::uvm_page::{
    PG_BUSY, PG_FAKE, uvm_pageactivate, uvm_pagealloc, uvm_pagecopy, uvm_pagewait,
};
use crate::uvm::uvm_param::atop;
use crate::uvm::uvm_pdaemon::uvm_wait;

/// `AMAP_SHARED`: amap is shared.
pub const AMAP_SHARED: i32 = 0x1;
/// `AMAP_REFALL`: amap_ref: reference entire amap.
pub const AMAP_REFALL: i32 = 0x2;
/// `AMAP_SWAPOFF`: amap_swap_off() is in progress.
pub const AMAP_SWAPOFF: i32 = 0x4;

/// `UVM_AMAP_LARGE`: # of slots in "large" amap.
pub const UVM_AMAP_LARGE: usize = 256;
/// `UVM_AMAP_CHUNK`: # of slots to chunk large amaps in.
pub const UVM_AMAP_CHUNK: usize = 16;

/// `PPREF_NONE`: not using ppref.
const PPREF_NONE: *mut i32 = usize::MAX as *mut i32;

/// `struct vm_amap_chunk`.
pub struct VmAmapChunk {
    /// `ac_list`.
    pub ac_list: TailqEntry<VmAmapChunk>,
    /// `ac_baseslot`.
    pub ac_baseslot: Cell<i32>,
    /// `ac_usedmap`: a bit per slot in use.
    pub ac_usedmap: Cell<u16>,
    /// `ac_nslot`.
    pub ac_nslot: Cell<u16>,
    /// `ac_anon[]`.
    pub ac_anon: [Cell<*const VmAnon>; UVM_AMAP_CHUNK],
}

impl VmAmapChunk {
    /// An empty chunk.
    pub const fn new() -> Self {
        Self {
            ac_list: TailqEntry::new(),
            ac_baseslot: Cell::new(0),
            ac_usedmap: Cell::new(0),
            ac_nslot: Cell::new(0),
            ac_anon: [const { Cell::new(ptr::null()) }; UVM_AMAP_CHUNK],
        }
    }

    /// `ac_anon[i]`.
    pub fn anon(&self, i: usize) -> Option<&'static VmAnon> {
        // SAFETY: a non-null slot is an anon this chunk's amap holds a reference to.
        unsafe { self.ac_anon[i].get().as_ref() }
    }
}

impl Default for VmAmapChunk {
    fn default() -> Self {
        Self::new()
    }
}

queue_adapter!(
    /// The chunk list of a normal amap (`am_chunks`).
    pub AcList: VmAmapChunk, ac_list => TailqEntry<VmAmapChunk>
);

/// `struct vm_amap`.
pub struct VmAmap {
    /// `am_lock`: lock for all vm_amap flags.
    pub am_lock: Cell<*const Rwlock>,
    /// `am_ref`: reference count.
    pub am_ref: Cell<i32>,
    /// `am_flags`: flags.
    pub am_flags: Cell<i32>,
    /// `am_nslot`: # of slots currently in map.
    pub am_nslot: Cell<i32>,
    /// `am_nused`: # of slots currently in use.
    pub am_nused: Cell<i32>,
    /// `am_ppref`: per page reference count (if !NULL).
    pub am_ppref: Cell<*mut i32>,
    /// `am_list`.
    pub am_list: ListEntry<VmAmap>,
    /// `am_buckets` (`ami_normal.amn_buckets`).
    pub am_buckets: Cell<*mut *const VmAmapChunk>,
    /// `am_chunks` (`ami_normal.amn_chunks`).
    pub am_chunks: TailqHead<AcList>,
    /// `am_nbuckets`: # of buckets.
    pub am_nbuckets: Cell<i32>,
    /// `am_ncused`: # of chunkers currently in use.
    pub am_ncused: Cell<i32>,
    /// `am_hashshift`: shift count to hash slot to bucket.
    pub am_hashshift: Cell<i32>,
    /// `am_small` (`ami_small`): the one chunk of a small amap.
    pub am_small: VmAmapChunk,
}

// SAFETY: `am_lock` guards every field once the amap is shared; the list link is the list
// lock's.
unsafe impl Sync for VmAmap {}

impl VmAmap {
    /// An empty, unlocked amap with one reference and no slots (the shape `amap_alloc1`
    /// fills in, and the marker `amap_swap_off` puts on the list).
    pub const fn new() -> Self {
        Self {
            am_lock: Cell::new(ptr::null()),
            am_ref: Cell::new(1),
            am_flags: Cell::new(0),
            am_nslot: Cell::new(0),
            am_nused: Cell::new(0),
            am_ppref: Cell::new(ptr::null_mut()),
            am_list: ListEntry::new(),
            am_buckets: Cell::new(ptr::null_mut()),
            am_chunks: TailqHead::new(),
            am_nbuckets: Cell::new(0),
            am_ncused: Cell::new(0),
            am_hashshift: Cell::new(0),
            am_small: VmAmapChunk::new(),
        }
    }

    /// `UVM_AMAP_SMALL(amap)`.
    pub fn is_small(&self) -> bool {
        self.am_nslot.get() as usize <= UVM_AMAP_CHUNK
    }

    /// `am_lock`, which `amap_alloc` and `amap_copy` set.
    pub fn lock(&self) -> &'static Rwlock {
        let lock = self.am_lock.get();
        kassert!(!lock.is_null());
        // SAFETY: a lock object this amap holds a reference to.
        unsafe { &*lock }
    }

    /// `am_buckets[bucket]`.
    fn bucket(&self, bucket: usize) -> Option<&'static VmAmapChunk> {
        kassert!((bucket as i32) < self.am_nbuckets.get());
        // SAFETY: `bucket` is below `am_nbuckets`, the length of the array `amap_alloc1`
        // made; a non-null entry is a chunk of this amap.
        unsafe { (*self.am_buckets.get().add(bucket)).as_ref() }
    }

    /// `am_buckets[bucket] = chunk`.
    fn set_bucket(&self, bucket: usize, chunk: Option<&VmAmapChunk>) {
        kassert!((bucket as i32) < self.am_nbuckets.get());
        // SAFETY: as for `bucket`.
        unsafe {
            *self.am_buckets.get().add(bucket) = chunk.map_or(ptr::null(), ptr::from_ref);
        }
    }

    /// `AMAP_CHUNK_FOREACH`'s first chunk.
    fn first_chunk(&self) -> Option<&VmAmapChunk> {
        if self.is_small() {
            Some(&self.am_small)
        } else {
            self.am_chunks.first()
        }
    }

    /// `AMAP_CHUNK_FOREACH`'s next chunk.
    fn next_chunk<'a>(&self, chunk: &'a VmAmapChunk) -> Option<&'a VmAmapChunk> {
        if self.is_small() {
            None
        } else {
            TailqHead::<AcList>::next(chunk)
        }
    }

    /// `am_ppref`, when the array exists and is in use.
    fn ppref(&self) -> Option<*mut i32> {
        let p = self.am_ppref.get();
        if p.is_null() || ptr::eq(p, PPREF_NONE) {
            None
        } else {
            Some(p)
        }
    }
}

impl Default for VmAmap {
    fn default() -> Self {
        Self::new()
    }
}

queue_adapter!(
    /// `amap_list`'s adapter (`am_list`).
    pub AmList: VmAmap, am_list => ListEntry<VmAmap>
);

/// `UVM_AMAP_SLOTIDX(slot)`.
pub const fn uvm_amap_slotidx(slot: usize) -> usize {
    slot % UVM_AMAP_CHUNK
}

/// `UVM_AMAP_BUCKET(amap, slot)`.
fn uvm_amap_bucket(amap: &VmAmap, slot: usize) -> usize {
    (slot / UVM_AMAP_CHUNK) >> amap.am_hashshift.get()
}

/// `AMAP_B2SLOT(S, B)`: convert byte offset to slot.
fn amap_b2slot(b: usize) -> usize {
    kassert!(b & (PAGE_SIZE - 1) == 0);
    b >> PAGE_SHIFT
}

/// `AMAP_BASE_SLOT(slot)`.
pub const fn amap_base_slot(slot: usize) -> usize {
    (slot / UVM_AMAP_CHUNK) * UVM_AMAP_CHUNK
}

/// `amap_flags(AMAP)`.
pub fn amap_flags(amap: &VmAmap) -> i32 {
    amap.am_flags.get()
}

/// `amap_refs(AMAP)`.
pub fn amap_refs(amap: &VmAmap) -> i32 {
    amap.am_ref.get()
}

/// `amap_lock(AMAP, RWLT)`.
pub fn amap_lock(amap: &VmAmap, rwlt: i32) {
    let _ = rw_enter(amap.lock(), rwlt);
}

/// `amap_unlock(AMAP)`.
pub fn amap_unlock(amap: &VmAmap) {
    rw_exit(amap.lock());
}

/// Pools for allocation of vm_amap structures. Note that in order to avoid an endless loop,
/// the amap pool's allocator cannot allocate memory from an amap (it currently goes through
/// the kernel uobj, so we are ok).
static UVM_AMAP_POOL: Pool = Pool::new();
/// `uvm_amap_chunk_pool`.
static UVM_AMAP_CHUNK_POOL: Pool = Pool::new();

/// `amap_list`: every amap.
struct AmapList(ListHead<AmList>);
// SAFETY: `amap_list_lock` guards the list.
unsafe impl Sync for AmapList {}
static AMAP_LIST: AmapList = AmapList(ListHead::new());
/// `amap_list_lock`.
static AMAP_LIST_LOCK: Rwlock = Rwlock::new("amaplstlk");

/// `amap_lock_list()`.
fn amap_lock_list() {
    rw_enter_write(&AMAP_LIST_LOCK);
}

/// `amap_unlock_list()`.
fn amap_unlock_list() {
    rw_exit_write(&AMAP_LIST_LOCK);
}

/// `amap_list_insert`.
fn amap_list_insert(amap: &VmAmap) {
    amap_lock_list();
    // SAFETY: an amap on no list yet, under the list lock.
    unsafe { AMAP_LIST.0.insert_head(amap) };
    amap_unlock_list();
}

/// `amap_list_remove`.
fn amap_list_remove(amap: &VmAmap) {
    amap_lock_list();
    // SAFETY: an amap on the list, under the list lock.
    unsafe { ListHead::<AmList>::remove(amap) };
    amap_unlock_list();
}

/// `amap_chunk_get`: lookup a chunk for slot. If `create` is non-zero, the chunk is created
/// if it does not yet exist.
///
/// Returns the chunk on success or `None` on error.
pub fn amap_chunk_get<'a>(
    amap: &'a VmAmap,
    slot: usize,
    create: bool,
    waitf: i32,
) -> Option<&'a VmAmapChunk> {
    if amap.is_small() {
        return Some(&amap.am_small);
    }

    let bucket = uvm_amap_bucket(amap, slot);
    let baseslot = amap_base_slot(slot) as i32;

    let mut pchunk: Option<&'a VmAmapChunk> = None;
    let mut chunk: Option<&'a VmAmapChunk> = amap.bucket(bucket);
    while let Some(c) = chunk {
        if uvm_amap_bucket(amap, c.ac_baseslot.get() as usize) != bucket {
            break;
        }
        if c.ac_baseslot.get() == baseslot {
            return Some(c);
        }
        pchunk = Some(c);
        chunk = TailqHead::<AcList>::next(c);
    }
    if !create {
        return None;
    }

    let n = if amap.am_nslot.get() - baseslot >= UVM_AMAP_CHUNK as i32 {
        UVM_AMAP_CHUNK as i32
    } else {
        amap.am_nslot.get() - baseslot
    };

    let mem = pool_get(&UVM_AMAP_CHUNK_POOL, waitf | PR_ZERO)?;
    let newchunk = mem.cast::<VmAmapChunk>();
    // SAFETY: a fresh pool item of `size_of::<VmAmapChunk>()` bytes, written once; it lives
    // until `amap_chunk_free` returns it.
    let newchunk: &'static VmAmapChunk = unsafe {
        newchunk.as_ptr().write(VmAmapChunk::new());
        newchunk.as_ref()
    };

    match pchunk {
        None => {
            // SAFETY: a chunk on no list, under the amap lock.
            unsafe { amap.am_chunks.insert_tail(newchunk) };
            kassert!(amap.bucket(bucket).is_none());
            amap.set_bucket(bucket, Some(newchunk));
        }
        Some(pchunk) => {
            // SAFETY: as above; `pchunk` is on the list.
            unsafe { amap.am_chunks.insert_after(pchunk, newchunk) };
        }
    }

    amap.am_ncused.set(amap.am_ncused.get() + 1);
    newchunk.ac_baseslot.set(baseslot);
    newchunk.ac_nslot.set(n as u16);
    Some(newchunk)
}

/// `amap_chunk_free`.
pub fn amap_chunk_free(amap: &VmAmap, chunk: &VmAmapChunk) {
    if amap.is_small() {
        return;
    }

    let bucket = uvm_amap_bucket(amap, chunk.ac_baseslot.get() as usize);
    let nchunk = TailqHead::<AcList>::next(chunk);
    // SAFETY: a chunk on the amap's list, under the amap lock.
    unsafe { amap.am_chunks.remove(chunk) };
    if amap.bucket(bucket).is_some_and(|c| ptr::eq(c, chunk)) {
        match nchunk {
            Some(n) if uvm_amap_bucket(amap, n.ac_baseslot.get() as usize) == bucket => {
                amap.set_bucket(bucket, Some(n));
            }
            _ => amap.set_bucket(bucket, None),
        }
    }
    pool_put(&UVM_AMAP_CHUNK_POOL, NonNull::from(chunk).cast::<u8>());
    amap.am_ncused.set(amap.am_ncused.get() - 1);
}

/// `pp_getreflen`: get the reference and length for a specific offset.
///
/// ppref's amap must be locked.
fn pp_getreflen(ppref: *mut i32, offset: usize) -> (i32, i32) {
    // SAFETY: `offset` is below the amap's slot count, the length of the array
    // `amap_pp_establish` made.
    unsafe {
        let v = *ppref.add(offset);
        if v > 0 {
            // chunk size must be 1
            (v - 1, 1) // don't forget to adjust
        } else {
            (-v - 1, *ppref.add(offset + 1))
        }
    }
}

/// `pp_setreflen`: set the reference and length for a specific offset.
///
/// ppref's amap must be locked.
fn pp_setreflen(ppref: *mut i32, offset: usize, r#ref: i32, len: i32) {
    // SAFETY: as for `pp_getreflen`.
    unsafe {
        if len == 1 {
            *ppref.add(offset) = r#ref + 1;
        } else {
            *ppref.add(offset) = -(r#ref + 1);
            *ppref.add(offset + 1) = len;
        }
    }
}

/// `amap_init`: called at boot time to init global amap data structures.
pub fn amap_init() {
    // Initialize the vm_amap pool.
    pool_init(
        &UVM_AMAP_POOL,
        size_of::<VmAmap>(),
        0,
        IPL_MPFLOOR,
        PR_WAITOK,
        "amappl",
        None,
    );
    pool_sethiwat(&UVM_AMAP_POOL, 4096);

    // initialize small amap pools: one pool serves every size (see the module's deviations).

    pool_init(
        &UVM_AMAP_CHUNK_POOL,
        size_of::<VmAmapChunk>(),
        0,
        IPL_MPFLOOR,
        PR_WAITOK,
        "amapchunkpl",
        None,
    );
    pool_sethiwat(&UVM_AMAP_CHUNK_POOL, 4096);
}

/// `amap_alloc1`: allocate an amap, but do not initialise the overlay.
///
/// Note: lock is not set.
fn amap_alloc1(slots: usize, waitf: i32, lazyalloc: bool) -> Option<&'static VmAmap> {
    let mut hashshift = 0u32;
    let mut chunkperbucket = 1usize;
    let pwaitf = if waitf & M_WAITOK != 0 {
        PR_WAITOK
    } else {
        PR_NOWAIT
    };

    kassert!(slots > 0);

    // Cast to unsigned so that rounding up cannot cause integer overflow if slots is large.
    let chunks = slots.div_ceil(UVM_AMAP_CHUNK);

    if lazyalloc {
        // Basically, the amap is a hash map where the number of buckets is fixed. We select
        // the number of buckets using the following strategy:
        //
        // 1. The maximal number of entries to search in a bucket upon a collision should be
        // less than or equal to log2(slots / UVM_AMAP_CHUNK). This is the worst-case number
        // of lookups we would have if we could chunk the amap. The log2(n) comes from the
        // fact that amaps are chunked by splitting up their vm_map_entries and organizing
        // those in a binary search tree.
        //
        // 2. The maximal number of entries in a bucket must be a power of two.
        //
        // The maximal number of entries per bucket is used to hash a slot to a bucket.
        //
        // In the future, this strategy could be refined to make it even harder/impossible
        // that the total amount of KVA needed for the hash buckets of all amaps to exceed
        // the maximal amount of KVA memory reserved for amaps.
        let mut log_chunks = 1;
        while (chunks >> log_chunks) > 0 {
            log_chunks += 1;
        }

        chunkperbucket = 1usize << hashshift;
        while chunkperbucket + 1 < log_chunks {
            hashshift += 1;
            chunkperbucket = 1usize << hashshift;
        }
    }

    let mem = pool_get(&UVM_AMAP_POOL, pwaitf | PR_ZERO)?;
    let ap = mem.cast::<VmAmap>();
    // SAFETY: a fresh pool item of `size_of::<VmAmap>()` bytes, written once; it lives until
    // `amap_free` returns it.
    let amap: &'static VmAmap = unsafe {
        ap.as_ptr().write(VmAmap::new());
        ap.as_ref()
    };

    amap.am_lock.set(ptr::null());
    amap.am_ref.set(1);
    amap.am_flags.set(0);
    amap.am_ppref.set(ptr::null_mut());
    amap.am_nslot.set(slots as i32);
    amap.am_nused.set(0);

    if amap.is_small() {
        amap.am_small.ac_nslot.set(slots as u16);
        return Some(amap);
    }

    amap.am_ncused.set(0);
    amap.am_chunks.init();
    amap.am_hashshift.set(hashshift as i32);
    amap.am_buckets.set(ptr::null_mut());

    let buckets = chunks.div_ceil(chunkperbucket);
    let Some(array) = mallocarray(
        buckets,
        size_of::<*const VmAmapChunk>(),
        M_UVMAMAP,
        waitf | if lazyalloc { M_ZERO } else { 0 },
    ) else {
        // fail1:
        pool_put(&UVM_AMAP_POOL, NonNull::from(amap).cast::<u8>());
        return None;
    };
    amap.am_buckets
        .set(array.cast::<*const VmAmapChunk>().as_ptr());
    amap.am_nbuckets.set(buckets as i32);

    if !lazyalloc {
        for i in 0..buckets {
            let n = if i == buckets - 1 {
                match slots % UVM_AMAP_CHUNK {
                    0 => UVM_AMAP_CHUNK,
                    n => n,
                }
            } else {
                UVM_AMAP_CHUNK
            };

            let Some(mem) = pool_get(&UVM_AMAP_CHUNK_POOL, PR_ZERO | pwaitf) else {
                // fail1:
                free(array, M_UVMAMAP, buckets * size_of::<*const VmAmapChunk>());
                while let Some(chunk) = amap.am_chunks.first() {
                    // SAFETY: a chunk on the list, which only this function sees.
                    unsafe { amap.am_chunks.remove(chunk) };
                    pool_put(&UVM_AMAP_CHUNK_POOL, NonNull::from(chunk).cast::<u8>());
                }
                pool_put(&UVM_AMAP_POOL, NonNull::from(amap).cast::<u8>());
                return None;
            };
            let chunk = mem.cast::<VmAmapChunk>();
            // SAFETY: as for the amap.
            let chunk: &'static VmAmapChunk = unsafe {
                chunk.as_ptr().write(VmAmapChunk::new());
                chunk.as_ref()
            };

            amap.set_bucket(i, Some(chunk));
            amap.am_ncused.set(amap.am_ncused.get() + 1);
            chunk.ac_baseslot.set((i * UVM_AMAP_CHUNK) as i32);
            chunk.ac_nslot.set(n as u16);
            // SAFETY: a chunk on no list, which only this function sees.
            unsafe { amap.am_chunks.insert_tail(chunk) };
        }
    }

    Some(amap)
}

/// `amap_lock_alloc`.
fn amap_lock_alloc(amap: &VmAmap) {
    amap.am_lock.set(rw_obj_alloc("amaplk"));
}

/// `amap_alloc`: allocate an amap to manage "sz" bytes of anonymous VM.
///
/// Caller should ensure sz is a multiple of PAGE_SIZE. Reference count to new amap is set to
/// one. New amap is returned unlocked.
pub fn amap_alloc(sz: usize, waitf: i32, lazyalloc: bool) -> Option<&'static VmAmap> {
    let slots = amap_b2slot(sz); // load slots
    if slots > INT_MAX as usize {
        return None;
    }

    let amap = amap_alloc1(slots, waitf, lazyalloc)?;
    amap_lock_alloc(amap);
    amap_list_insert(amap);

    Some(amap)
}

/// `amap_free`: free an amap.
///
/// The amap must be unlocked. The amap should have a zero reference count and be empty.
pub fn amap_free(amap: &VmAmap) {
    kassert!(amap.am_ref.get() == 0 && amap.am_nused.get() == 0);
    kassert!(amap.am_flags.get() & AMAP_SWAPOFF == 0);

    if !amap.am_lock.get().is_null() {
        kassert!(!rw_write_held(amap.lock()));
        rw_obj_free(amap.lock());
    }

    if let Some(ppref) = amap.ppref() {
        // SAFETY: the array `amap_pp_establish` allocated, of `am_nslot` ints.
        free(
            unsafe { NonNull::new_unchecked(ppref.cast::<u8>()) },
            M_UVMAMAP,
            amap.am_nslot.get() as usize * size_of::<i32>(),
        );
    }

    if !amap.is_small() {
        while let Some(chunk) = amap.am_chunks.first() {
            // SAFETY: a chunk on the amap's list; the amap is dead.
            unsafe { amap.am_chunks.remove(chunk) };
            pool_put(&UVM_AMAP_CHUNK_POOL, NonNull::from(chunk).cast::<u8>());
        }
        // SAFETY: the bucket array `amap_alloc1` made.
        free(
            unsafe { NonNull::new_unchecked(amap.am_buckets.get().cast::<u8>()) },
            M_UVMAMAP,
            amap.am_nbuckets.get() as usize * size_of::<*const VmAmapChunk>(),
        );
    }
    pool_put(&UVM_AMAP_POOL, NonNull::from(amap).cast::<u8>());
}

/// `amap_wipeout`: wipeout all anon's in an amap; then free the amap!
///
/// Called from `amap_unref()`, when reference count drops to zero. Amap must be locked.
pub fn amap_wipeout(amap: &VmAmap) {
    kassert!(rw_write_held(amap.lock()));
    kassert!(amap.am_ref.get() == 0);

    if amap.am_flags.get() & AMAP_SWAPOFF != 0 {
        // Note: amap_swap_off() will call us again.
        amap_unlock(amap);
        return;
    }

    let mut chunk = amap.first_chunk();
    while let Some(c) = chunk {
        let mut map = c.ac_usedmap.get();

        while map != 0 {
            let slot = map.trailing_zeros() as usize;
            map ^= 1 << slot;
            let anon = c.anon(slot);

            let Some(anon) = anon.filter(|a| a.an_ref.get() != 0) else {
                panic(format_args!("amap_wipeout: corrupt amap"));
            };
            kassert!(ptr::eq(anon.an_lock.get(), amap.am_lock.get()));

            // Drop the reference.
            anon.an_ref.set(anon.an_ref.get() - 1);
            if anon.an_ref.get() == 0 {
                uvm_anfree(anon);
            }
        }
        chunk = amap.next_chunk(c);
    }

    // Finally, destroy the amap.
    amap.am_ref.set(0); // ... was one
    amap.am_nused.set(0);
    amap_unlock(amap);
    amap_free(amap);
}

/// `amap_copy`: ensure that a map entry's "needs_copy" flag is false by copying the amap if
/// necessary.
///
/// An entry with a null amap pointer will get a new (blank) one. The map that the map entry
/// belongs to must be locked by caller. The amap currently attached to "entry" (if any) must
/// be unlocked. If canchunk is true, then we may clip the entry into a chunk. "startva" and
/// "endva" are used only if canchunk is true. They are used to limit chunking (e.g. if you
/// have a large space that you know you are going to need to allocate amaps for, there is
/// no point in allowing that to be chunked).
pub fn amap_copy(
    map: &VmMap,
    entry: &VmMapEntry,
    waitf: i32,
    canchunk: bool,
    startva: usize,
    endva: usize,
) {
    let mut lazyalloc = false;

    // KASSERT(map != kernel_map): we use sleeping locks (the kernel map, M7a part 2).

    // Is there an amap to copy? If not, create one.
    let Some(srcamap) = entry.aref.amap() else {
        // Check to see if we have a large amap that we can chunk. We align startva/endva
        // to chunk-sized boundaries and then clip to them.
        //
        // If we cannot chunk the amap, allocate it in a way that makes it grow or shrink
        // dynamically with the number of slots.
        if atop(entry.end.get() - entry.start.get()) >= UVM_AMAP_LARGE {
            if canchunk {
                // convert slots to bytes
                let chunksize = UVM_AMAP_CHUNK << PAGE_SHIFT;
                let startva = (startva / chunksize) * chunksize;
                // roundup(endva, chunksize), wrapping as the C's unsigned arithmetic does.
                let endva = (endva.wrapping_add(chunksize - 1) / chunksize) * chunksize;
                uvm_map_clip_start_at(map, entry, startva);
                // watch out for endva wrap-around!
                if endva >= startva {
                    uvm_map_clip_end_at(map, entry, endva);
                }
            } else {
                lazyalloc = true;
            }
        }

        entry.aref.ar_pageoff.set(0);
        let amap = amap_alloc(entry.end.get() - entry.start.get(), waitf, lazyalloc);
        entry
            .aref
            .ar_amap
            .set(amap.map_or(ptr::null(), ptr::from_ref));
        if amap.is_some() {
            entry.etype.set(entry.etype.get() & !UVM_ET_NEEDSCOPY);
        }
        return;
    };

    // First check and see if we are the only map entry referencing he amap we currently
    // have. If so, then just take it over instead of copying it. Note that we are reading
    // am_ref without lock held as the value can only be one if we have the only reference
    // to the amap (via our locked map). If the value is greater than one, then allocate
    // amap and re-check the value.
    if srcamap.am_ref.get() == 1 {
        entry.etype.set(entry.etype.get() & !UVM_ET_NEEDSCOPY);
        return;
    }

    // Allocate a new amap (note: not initialised, etc).
    let slots = amap_b2slot(entry.end.get() - entry.start.get());
    if !srcamap.is_small() && srcamap.am_hashshift.get() != 0 {
        lazyalloc = true;
    }
    let Some(amap) = amap_alloc1(slots, waitf, lazyalloc) else {
        return;
    };

    // Make the new amap share the source amap's lock, and then lock both.
    amap.am_lock.set(srcamap.am_lock.get());
    rw_obj_hold(amap.lock());

    amap_lock(srcamap, RW_WRITE);

    // Re-check the reference count with the lock held. If it has dropped to one - we can
    // take over the existing map.
    if srcamap.am_ref.get() == 1 {
        // Just take over the existing amap.
        entry.etype.set(entry.etype.get() & !UVM_ET_NEEDSCOPY);
        amap_unlock(srcamap);
        // Destroy the new (unused) amap.
        amap.am_ref.set(amap.am_ref.get() - 1);
        amap_free(amap);
        return;
    }

    // Copy the slots.
    let mut lcv = 0;
    while lcv < slots {
        let srcslot = entry.aref.ar_pageoff.get() as usize + lcv;
        let i = uvm_amap_slotidx(lcv);
        let j = uvm_amap_slotidx(srcslot);
        let mut n = UVM_AMAP_CHUNK;
        if i > j {
            n -= i;
        } else {
            n -= j;
        }
        if lcv + n > slots {
            n = slots - lcv;
        }

        let Some(srcchunk) = amap_chunk_get(srcamap, srcslot, false, PR_NOWAIT) else {
            lcv += n;
            continue;
        };

        let Some(chunk) = amap_chunk_get(amap, lcv, true, PR_NOWAIT) else {
            // amap_wipeout() releases the shared lock.
            amap.am_ref.set(amap.am_ref.get() - 1);
            amap_wipeout(amap);
            return;
        };

        for k in 0..n {
            let (i, j) = (i + k, j + k);
            let anon = srcchunk.anon(j);
            chunk.ac_anon[i].set(anon.map_or(ptr::null(), ptr::from_ref));
            if let Some(anon) = anon {
                kassert!(ptr::eq(anon.an_lock.get(), srcamap.am_lock.get()));
                kassert!(anon.an_ref.get() > 0);
                chunk.ac_usedmap.set(chunk.ac_usedmap.get() | (1 << i));
                anon.an_ref.set(anon.an_ref.get() + 1);
                amap.am_nused.set(amap.am_nused.get() + 1);
            }
        }
        lcv += n;
    }

    // Drop our reference to the old amap (srcamap) and unlock. Since the reference count on
    // srcamap is greater than one, (we checked above), it cannot drop to zero while it is
    // locked.
    srcamap.am_ref.set(srcamap.am_ref.get() - 1);
    kassert!(srcamap.am_ref.get() > 0);

    if srcamap.am_ref.get() == 1 && srcamap.am_flags.get() & AMAP_SHARED != 0 {
        srcamap.am_flags.set(srcamap.am_flags.get() & !AMAP_SHARED); // clear shared flag
    }
    if srcamap.ppref().is_some() {
        amap_pp_adjref(
            srcamap,
            entry.aref.ar_pageoff.get() as usize,
            (entry.end.get() - entry.start.get()) >> PAGE_SHIFT,
            -1,
        );
    }

    // If we referenced any anons, then share the source amap's lock. Otherwise, we have
    // nothing in common, so allocate a new one.
    kassert!(ptr::eq(amap.am_lock.get(), srcamap.am_lock.get()));
    if amap.am_nused.get() == 0 {
        rw_obj_free(amap.lock());
        amap.am_lock.set(ptr::null());
    }
    amap_unlock(srcamap);

    if amap.am_lock.get().is_null() {
        amap_lock_alloc(amap);
    }

    // Install new amap.
    entry.aref.ar_pageoff.set(0);
    entry.aref.ar_amap.set(amap);
    entry.etype.set(entry.etype.get() & !UVM_ET_NEEDSCOPY);

    amap_list_insert(amap);
}

/// `amap_cow_now`: resolve all copy-on-write faults in an amap now for fork(2).
///
/// Called during fork(2) when the parent process has a wired map entry. In that case we want
/// to avoid write-protecting pages in the parent's map (e.g. like what you'd do for a COW
/// page) so we resolve the COW here.
///
/// Assume parent's entry was wired, thus all pages are resident. The parent and child vm_map
/// must both be locked. Caller passes child's map/entry in to us. XXXCDC: out of memory
/// should cause fork to fail, but there is currently no easy way to do this (needs fix).
pub fn amap_cow_now(_map: &VmMap, entry: &VmMapEntry) {
    let Some(amap) = entry.aref.amap() else {
        return;
    };

    // note that if we unlock the amap then we must ReStart the "lcv" for loop because some
    // other process could reorder the anon's in the am_anon[] array on us while the lock is
    // dropped.
    'restart: loop {
        amap_lock(amap, RW_WRITE);
        let mut chunk = amap.first_chunk();
        while let Some(c) = chunk {
            let mut map = c.ac_usedmap.get();

            while map != 0 {
                let slot = map.trailing_zeros() as usize;
                map ^= 1 << slot;
                let Some(anon) = c.anon(slot) else {
                    panic(format_args!("amap_cow_now: corrupt amap"));
                };
                kassert!(ptr::eq(anon.an_lock.get(), amap.am_lock.get()));

                // The old page must be resident since the parent is wired.
                let Some(pg) = anon.page() else {
                    panic(format_args!("amap_cow_now: anon without a page"));
                };

                // if the anon ref count is one, we are safe (the child has exclusive access
                // to the page).
                if anon.an_ref.get() <= 1 {
                    continue;
                }

                // If the page is busy, then we have to unlock, wait for it and then restart.
                if pg.flags() & PG_BUSY != 0 {
                    uvm_pagewait(pg, amap.lock(), "cownow");
                    continue 'restart;
                }

                // Perform a copy-on-write. First - get a new anon and a page.
                let nanon = uvm_analloc();
                let npg = match nanon {
                    Some(nanon) => {
                        // the new anon will share the amap's lock
                        nanon.an_lock.set(amap.am_lock.get());
                        uvm_pagealloc(None, 0, Some(nanon), 0)
                    }
                    None => None,
                };

                let (Some(nanon), Some(npg)) = (nanon, npg) else {
                    // out of memory
                    amap_unlock(amap);
                    if let Some(nanon) = nanon {
                        nanon.an_lock.set(ptr::null());
                        nanon.an_ref.set(nanon.an_ref.get() - 1);
                        kassert!(nanon.an_ref.get() == 0);
                        uvm_anfree(nanon);
                    }
                    uvm_wait("cownowpage");
                    continue 'restart;
                };

                // Copy the data and replace anon with the new one. Also, setup its lock
                // (share the with amap's lock).
                uvm_pagecopy(pg, npg);
                anon.an_ref.set(anon.an_ref.get() - 1);
                kassert!(anon.an_ref.get() > 0);
                c.ac_anon[slot].set(nanon);

                // Drop PG_BUSY on new page. Since its owner was write locked all this time -
                // it cannot be PG_RELEASED or PG_WANTED.
                npg.clear_bits(PG_BUSY | PG_FAKE);
                // UVM_PAGE_OWN(npg, NULL): not configured.
                uvm_pageactivate(npg);
            }
            chunk = amap.next_chunk(c);
        }
        amap_unlock(amap);
        return;
    }
}

/// `amap_splitref`: split a single reference into two separate references.
///
/// Called from uvm_map's clip routines. origref's map should be locked. origref->ar_amap
/// should be unlocked (we will lock).
pub fn amap_splitref(origref: &VmAref, splitref: &VmAref, offset: usize) {
    let Some(amap) = origref.amap() else {
        panic(format_args!("amap_splitref: no amap"));
    };

    kassert!(ptr::eq(splitref.ar_amap.get(), amap));
    let leftslots = amap_b2slot(offset) as i32;
    if leftslots == 0 {
        panic(format_args!("amap_splitref: split at zero offset"));
    }

    amap_lock(amap, RW_WRITE);

    if amap.am_nslot.get() - origref.ar_pageoff.get() - leftslots <= 0 {
        panic(format_args!("amap_splitref: map size check failed"));
    }

    // Establish ppref before we add a duplicate reference to the amap.
    if amap.am_ppref.get().is_null() {
        amap_pp_establish(amap);
    }

    // Note: not a share reference.
    amap.am_ref.set(amap.am_ref.get() + 1);
    splitref.ar_amap.set(amap);
    splitref
        .ar_pageoff
        .set(origref.ar_pageoff.get() + leftslots);
    amap_unlock(amap);
}

/// `amap_pp_establish`: add a ppref array to an amap, if possible.
///
/// Amap should be locked by caller.
pub fn amap_pp_establish(amap: &VmAmap) {
    kassert!(rw_write_held(amap.lock()));
    let ppref = mallocarray(
        amap.am_nslot.get() as usize,
        size_of::<i32>(),
        M_UVMAMAP,
        M_NOWAIT | M_ZERO,
    );

    let Some(ppref) = ppref else {
        // Failure - just do not use ppref.
        amap.am_ppref.set(PPREF_NONE);
        return;
    };
    let ppref = ppref.cast::<i32>().as_ptr();
    amap.am_ppref.set(ppref);

    pp_setreflen(ppref, 0, amap.am_ref.get(), amap.am_nslot.get());
}

/// `amap_pp_adjref`: adjust reference count to a part of an amap using the per-page
/// reference count array.
///
/// Caller must check that ppref != PPREF_NONE before calling. Map and amap must be locked.
pub fn amap_pp_adjref(amap: &VmAmap, curslot: usize, slotlen: usize, adjval: i32) {
    kassert!(rw_write_held(amap.lock()));

    let stopslot = curslot + slotlen;
    let Some(ppref) = amap.ppref() else {
        panic(format_args!("amap_pp_adjref: no ppref"));
    };
    let mut prevlcv = 0;

    // Advance to the correct place in the array, fragment if needed.
    let mut lcv = 0;
    let mut len;
    while lcv < curslot {
        let (r#ref, l) = pp_getreflen(ppref, lcv);
        len = l as usize;
        if lcv + len > curslot {
            // goes past start?
            pp_setreflen(ppref, lcv, r#ref, (curslot - lcv) as i32);
            pp_setreflen(ppref, curslot, r#ref, (len - (curslot - lcv)) as i32);
            len = curslot - lcv; // new length of entry @ lcv
        }
        prevlcv = lcv;
        lcv += len;
    }
    let (prevref, prevlen) = if lcv != 0 {
        pp_getreflen(ppref, prevlcv)
    } else {
        // Ensure that the "prevref == ref" test below always fails, since we are starting
        // from the beginning of the ppref array; that is, there is no previous chunk.
        (-1, 0)
    };

    // Now adjust reference counts in range. Merge the first changed entry with the last
    // unchanged entry if possible.
    if lcv != curslot {
        panic(format_args!("amap_pp_adjref: overshot target"));
    }

    while lcv < stopslot {
        let (mut r#ref, l) = pp_getreflen(ppref, lcv);
        len = l as usize;
        if lcv + len > stopslot {
            // goes past end?
            pp_setreflen(ppref, lcv, r#ref, (stopslot - lcv) as i32);
            pp_setreflen(ppref, stopslot, r#ref, (len - (stopslot - lcv)) as i32);
            len = stopslot - lcv;
        }
        r#ref += adjval;
        if r#ref < 0 {
            panic(format_args!("amap_pp_adjref: negative reference count"));
        }
        if lcv == prevlcv + prevlen as usize && r#ref == prevref {
            pp_setreflen(ppref, prevlcv, r#ref, prevlen + len as i32);
        } else {
            pp_setreflen(ppref, lcv, r#ref, len as i32);
        }
        if r#ref == 0 {
            amap_wiperange(amap, lcv, len);
        }
        lcv += len;
    }
}

/// `amap_wiperange_chunk`: drops the anons of `[slotoff, slotoff + slots)` that `chunk`
/// holds.
pub fn amap_wiperange_chunk(amap: &VmAmap, chunk: &VmAmapChunk, slotoff: usize, slots: usize) {
    let startbase = amap_base_slot(slotoff) as i32;
    let endbase = amap_base_slot(slotoff + slots - 1) as i32;

    let mut map = chunk.ac_usedmap.get();
    if startbase == chunk.ac_baseslot.get() {
        map &= !(((1u32 << (slotoff - startbase as usize)) - 1) as u16);
    }
    if endbase == chunk.ac_baseslot.get() {
        map &= ((1u32 << (slotoff + slots - endbase as usize)) - 1) as u16;
    }

    while map != 0 {
        let curslot = map.trailing_zeros() as usize;
        map ^= 1 << curslot;
        chunk
            .ac_usedmap
            .set(chunk.ac_usedmap.get() ^ (1 << curslot));
        let Some(anon) = chunk.anon(curslot) else {
            panic(format_args!("amap_wiperange_chunk: empty slot in use"));
        };
        kassert!(ptr::eq(anon.an_lock.get(), amap.am_lock.get()));

        // remove it from the amap
        chunk.ac_anon[curslot].set(ptr::null());

        amap.am_nused.set(amap.am_nused.get() - 1);

        // drop anon reference count
        anon.an_ref.set(anon.an_ref.get() - 1);
        if anon.an_ref.get() == 0 {
            uvm_anfree(anon);
        }

        // done with this anon, next ...!
    }
}

/// `amap_wiperange`: wipe out a range of an amap. Note: different from amap_wipeout because
/// the amap is kept intact.
///
/// Both map and amap must be locked by caller.
pub fn amap_wiperange(amap: &VmAmap, slotoff: usize, slots: usize) {
    kassert!(rw_write_held(amap.lock()));

    let startbucket = uvm_amap_bucket(amap, slotoff);
    let endbucket = uvm_amap_bucket(amap, slotoff + slots - 1);

    // We can either traverse the amap by am_chunks or by am_buckets. Determine which way is
    // less expensive.
    if amap.is_small() {
        amap_wiperange_chunk(amap, &amap.am_small, slotoff, slots);
    } else if endbucket + 1 - startbucket >= amap.am_ncused.get() as usize {
        let mut chunk = amap.am_chunks.first();
        while let Some(c) = chunk {
            let nchunk = TailqHead::<AcList>::next(c);
            let base = c.ac_baseslot.get() as usize;
            if base + c.ac_nslot.get() as usize > slotoff && base < slotoff + slots {
                amap_wiperange_chunk(amap, c, slotoff, slots);
                if c.ac_usedmap.get() == 0 {
                    amap_chunk_free(amap, c);
                }
            }
            chunk = nchunk;
        }
    } else {
        for bucket in startbucket..=endbucket {
            let mut chunk = amap.bucket(bucket);
            while let Some(c) = chunk {
                let nchunk = TailqHead::<AcList>::next(c);

                if uvm_amap_bucket(amap, c.ac_baseslot.get() as usize) != bucket {
                    break;
                }
                let base = c.ac_baseslot.get() as usize;
                if base + c.ac_nslot.get() as usize > slotoff && base < slotoff + slots {
                    amap_wiperange_chunk(amap, c, slotoff, slots);
                    if c.ac_usedmap.get() == 0 {
                        amap_chunk_free(amap, c);
                    }
                }
                chunk = nchunk;
            }
        }
    }
}

/// `amap_swap_off`: pagein anonymous pages in amaps and drop swap slots.
///
/// Note that we don't always traverse all anons, eg. amaps being wiped out, released anons.
/// Returns true if failed.
pub fn amap_swap_off(startslot: i32, endslot: i32) -> bool {
    let marker = VmAmap::new();
    let mut rv = false;

    amap_lock_list();
    let mut am = AMAP_LIST.0.first();
    while let Some(a) = am {
        if rv {
            break;
        }
        amap_lock(a, RW_WRITE);
        if a.am_ref.get() == 0 || a.am_nused.get() == 0 {
            amap_unlock(a);
            am = ListHead::<AmList>::next(a);
            continue;
        }

        // SAFETY: the marker lives on this stack frame for the whole walk, under the list
        // lock while linked; it is removed below before the frame goes.
        unsafe { ListHead::<AmList>::insert_after(a, &marker) };
        amap_unlock_list();

        let mut cur: Option<&VmAmap> = Some(a);
        'again: while let Some(a) = cur {
            let mut chunk = a.first_chunk();
            while let Some(c) = chunk {
                let mut map = c.ac_usedmap.get();

                while map != 0 {
                    let slot = map.trailing_zeros() as usize;
                    map ^= 1 << slot;
                    let Some(anon) = c.anon(slot) else {
                        panic(format_args!("amap_swap_off: empty slot in use"));
                    };

                    let swslot = anon.an_swslot.get();
                    if swslot < startslot || endslot <= swslot {
                        continue;
                    }

                    a.am_flags.set(a.am_flags.get() | AMAP_SWAPOFF);

                    rv = uvm_anon_pagein(a, anon);
                    amap_lock(a, RW_WRITE);

                    a.am_flags.set(a.am_flags.get() & !AMAP_SWAPOFF);
                    if amap_refs(a) == 0 {
                        amap_unlock(a);
                        amap_list_remove(a);
                        amap_lock(a, RW_WRITE);
                        amap_wipeout(a);
                        cur = None;
                        break 'again;
                    }
                    if rv {
                        break 'again;
                    }
                    continue 'again;
                }
                chunk = a.next_chunk(c);
            }
            break;
        }
        // nextamap:
        if let Some(a) = cur {
            amap_unlock(a);
        }
        amap_lock_list();
        let am_next = ListHead::<AmList>::next(&marker);
        // SAFETY: the marker is on the list, under the list lock.
        unsafe { ListHead::<AmList>::remove(&marker) };
        am = am_next;
    }
    amap_unlock_list();

    rv
}

/// `amap_lookup`: look up a page in an amap.
///
/// Amap should be locked by caller.
pub fn amap_lookup(aref: &VmAref, offset: usize) -> Option<&'static VmAnon> {
    let Some(amap) = aref.amap() else {
        panic(format_args!("amap_lookup: no amap"));
    };

    let slot = amap_b2slot(offset) + aref.ar_pageoff.get() as usize;
    kassert!(slot < amap.am_nslot.get() as usize);

    let chunk = amap_chunk_get(amap, slot, false, PR_NOWAIT)?;

    chunk.anon(uvm_amap_slotidx(slot))
}

/// `amap_lookups`: look up a range of pages in an amap.
///
/// Amap should be locked by caller. XXXCDC: this interface is biased toward array-based
/// amaps. fix.
pub fn amap_lookups(aref: &VmAref, offset: usize, anons: &mut [*const VmAnon]) {
    let Some(amap) = aref.amap() else {
        panic(format_args!("amap_lookups: no amap"));
    };
    let npages = anons.len();

    let slot = amap_b2slot(offset) + aref.ar_pageoff.get() as usize;

    kassert!(slot + (npages - 1) < amap.am_nslot.get() as usize);

    let mut i = 0;
    let mut lcv = slot;
    while lcv < slot + npages {
        let mut n = UVM_AMAP_CHUNK - uvm_amap_slotidx(lcv);
        if lcv + n > slot + npages {
            n = slot + npages - lcv;
        }

        match amap_chunk_get(amap, lcv, false, PR_NOWAIT) {
            None => anons[i..i + n].fill(ptr::null()),
            Some(chunk) => {
                let base = uvm_amap_slotidx(lcv);
                for k in 0..n {
                    anons[i + k] = chunk.ac_anon[base + k].get();
                }
            }
        }
        i += n;
        lcv += n;
    }
}

/// `amap_populate`: ensure that the amap can store an anon for the page at offset. This
/// function can sleep until memory to store the anon is available.
pub fn amap_populate(aref: &VmAref, offset: usize) {
    let Some(amap) = aref.amap() else {
        panic(format_args!("amap_populate: no amap"));
    };

    let slot = amap_b2slot(offset) + aref.ar_pageoff.get() as usize;
    kassert!(slot < amap.am_nslot.get() as usize);

    let chunk = amap_chunk_get(amap, slot, true, PR_WAITOK);
    kassert!(chunk.is_some());
}

/// `amap_add`: add (or replace) a page to an amap.
///
/// Amap should be locked by caller. Anon must have the lock associated with this amap.
/// Returns `false` when no chunk could be had (the C's 1).
pub fn amap_add(aref: &VmAref, offset: usize, anon: &VmAnon, replace: bool) -> bool {
    let Some(amap) = aref.amap() else {
        panic(format_args!("amap_add: no amap"));
    };

    let slot = amap_b2slot(offset) + aref.ar_pageoff.get() as usize;
    kassert!(slot < amap.am_nslot.get() as usize);

    let Some(chunk) = amap_chunk_get(amap, slot, true, PR_NOWAIT) else {
        return false;
    };

    let slot = uvm_amap_slotidx(slot);
    if replace {
        let Some(oanon) = chunk.anon(slot) else {
            panic(format_args!("amap_add: replacing an empty slot"));
        };
        if let Some(opg) = oanon.page()
            && amap.am_flags.get() & AMAP_SHARED != 0
        {
            pmap_page_protect(opg, PROT_NONE);
            // XXX: suppose page is supposed to be wired somewhere?
        }
    } else {
        // !replace
        if chunk.anon(slot).is_some() {
            panic(format_args!("amap_add: slot in use"));
        }

        chunk.ac_usedmap.set(chunk.ac_usedmap.get() | (1 << slot));
        amap.am_nused.set(amap.am_nused.get() + 1);
    }
    chunk.ac_anon[slot].set(anon);

    true
}

/// `amap_unadd`: remove a page from an amap.
///
/// Amap should be locked by caller.
pub fn amap_unadd(aref: &VmAref, offset: usize) {
    let Some(amap) = aref.amap() else {
        panic(format_args!("amap_unadd: no amap"));
    };

    kassert!(rw_write_held(amap.lock()));

    let slot = amap_b2slot(offset) + aref.ar_pageoff.get() as usize;
    kassert!(slot < amap.am_nslot.get() as usize);
    let Some(chunk) = amap_chunk_get(amap, slot, false, PR_NOWAIT) else {
        panic(format_args!("amap_unadd: no chunk"));
    };

    let slot = uvm_amap_slotidx(slot);
    kassert!(chunk.anon(slot).is_some());

    chunk.ac_anon[slot].set(ptr::null());
    chunk.ac_usedmap.set(chunk.ac_usedmap.get() & !(1 << slot));
    amap.am_nused.set(amap.am_nused.get() - 1);

    if chunk.ac_usedmap.get() == 0 {
        amap_chunk_free(amap, chunk);
    }
}

/// `amap_adjref_anons`: adjust the reference count(s) on amap and its anons.
fn amap_adjref_anons(amap: &VmAmap, offset: usize, len: usize, refv: i32, all: bool) {
    kassert!(rw_write_held(amap.lock()));

    // We must establish the ppref array before changing am_ref so that the ppref values
    // match the current amap refcount.
    if amap.am_ppref.get().is_null() && !all && len != amap.am_nslot.get() as usize {
        amap_pp_establish(amap);
    }
    amap.am_ref.set(amap.am_ref.get() + refv);

    if amap.ppref().is_some() {
        if all {
            amap_pp_adjref(amap, 0, amap.am_nslot.get() as usize, refv);
        } else {
            amap_pp_adjref(amap, offset, len, refv);
        }
    }
    amap_unlock(amap);
}

/// `amap_ref`: gain a reference to an amap.
///
/// Amap must not be locked (we will lock). "offset" and "len" are in units of pages. Called
/// at fork time to gain the child's reference.
pub fn amap_ref(amap: &VmAmap, offset: usize, len: usize, flags: i32) {
    amap_lock(amap, RW_WRITE);
    if flags & AMAP_SHARED != 0 {
        amap.am_flags.set(amap.am_flags.get() | AMAP_SHARED);
    }
    amap_adjref_anons(amap, offset, len, 1, flags & AMAP_REFALL != 0);
}

/// `amap_unref`: remove a reference to an amap.
///
/// All pmap-level references to this amap must be already removed. Called from
/// `uvm_unmap_detach()`; entry is already removed from the map. We will lock amap, so it
/// must be unlocked.
pub fn amap_unref(amap: &VmAmap, offset: usize, len: usize, all: bool) {
    amap_lock(amap, RW_WRITE);

    kassert!(amap.am_ref.get() > 0);

    if amap.am_ref.get() == 1 {
        // If the last reference - wipeout and destroy the amap.
        amap.am_ref.set(amap.am_ref.get() - 1);
        if amap.am_flags.get() & AMAP_SWAPOFF != 0 {
            amap_wipeout(amap);
            return;
        }
        amap_unlock(amap);
        amap_list_remove(amap);
        amap_lock(amap, RW_WRITE);
        amap_wipeout(amap);
        return;
    }

    // Otherwise, drop the reference count(s) on anons.
    if amap.am_ref.get() == 2 && amap.am_flags.get() & AMAP_SHARED != 0 {
        amap.am_flags.set(amap.am_flags.get() & !AMAP_SHARED);
    }
    amap_adjref_anons(amap, offset, len, -1, all);
}
/* </CODE> */

/* <TESTS> */
#[cfg(test)]
mod tests {
    // Host tests for the amap layer over real memory (see `subr_pool.rs` for the setup).

    use std::sync::MutexGuard;
    use std::{assert, assert_eq};

    use super::*;
    use crate::kern::kern_rwlock::rw_obj_init;
    use crate::kern::subr_pool::tests::setup_real_memory;
    use crate::uvm::uvm_anon::uvm_anon_init;

    /// Real memory, the lock objects, the anon pool and the amap pools.
    fn setup() -> MutexGuard<'static, ()> {
        let guard = setup_real_memory();
        rw_obj_init();
        uvm_anon_init();
        amap_init();
        guard
    }

    /// An anon that shares `amap`'s lock, as `uvm_fault` makes them.
    fn anon_for(amap: &VmAmap) -> &'static VmAnon {
        let anon = uvm_analloc().expect("an anon");
        anon.an_lock.set(amap.am_lock.get());
        anon
    }

    #[test]
    fn small_amap_add_lookup_unadd() {
        let _g = setup();
        let amap = amap_alloc(4 * PAGE_SIZE, M_WAITOK, false).expect("an amap");
        assert!(amap.is_small());
        assert_eq!(amap.am_nslot.get(), 4);
        assert_eq!(amap_refs(amap), 1);
        let aref = VmAref::new();
        aref.ar_amap.set(amap);

        let anon = anon_for(amap);
        amap_lock(amap, RW_WRITE);
        assert!(amap_add(&aref, 2 * PAGE_SIZE, anon, false));
        assert_eq!(amap.am_nused.get(), 1);
        assert!(amap_lookup(&aref, 2 * PAGE_SIZE).is_some_and(|a| ptr::eq(a, anon)));
        assert!(amap_lookup(&aref, 0).is_none());

        let mut anons: [*const VmAnon; 4] = [ptr::null(); 4];
        amap_lookups(&aref, 0, &mut anons);
        assert!(anons[0].is_null() && anons[1].is_null() && anons[3].is_null());
        assert!(ptr::eq(anons[2], anon));

        amap_unadd(&aref, 2 * PAGE_SIZE);
        assert_eq!(amap.am_nused.get(), 0);
        assert!(amap_lookup(&aref, 2 * PAGE_SIZE).is_none());
        // the anon left the amap with its reference: drop it under the shared lock
        anon.an_ref.set(0);
        uvm_anfree(anon);
        amap_unlock(amap);

        amap_unref(amap, 0, 4, true);
    }

    #[test]
    fn large_amap_chunks_and_buckets() {
        let _g = setup();
        let slots = UVM_AMAP_LARGE + 5;

        // eager: every chunk exists, the last one short
        let eager = amap_alloc(slots * PAGE_SIZE, M_WAITOK, false).expect("an amap");
        assert!(!eager.is_small());
        assert_eq!(eager.am_hashshift.get(), 0);
        assert_eq!(eager.am_nbuckets.get(), 17);
        assert_eq!(eager.am_ncused.get(), 17);
        let last = eager.am_chunks.iter().last().expect("a chunk");
        assert_eq!(last.ac_baseslot.get(), 256);
        assert_eq!(last.ac_nslot.get(), 5);
        amap_unref(eager, 0, slots, true);

        // lazy: 17 chunks hash into 5 buckets of 4 (log2(17) rounds to 5, so 4 + 1 < 5 fails)
        let lazy = amap_alloc(slots * PAGE_SIZE, M_WAITOK, true).expect("an amap");
        assert_eq!(lazy.am_hashshift.get(), 2);
        assert_eq!(lazy.am_nbuckets.get(), 5);
        assert_eq!(lazy.am_ncused.get(), 0);
        let aref = VmAref::new();
        aref.ar_amap.set(lazy);
        let (a0, a1) = (anon_for(lazy), anon_for(lazy));
        amap_lock(lazy, RW_WRITE);
        assert!(amap_add(&aref, 0, a0, false));
        assert!(amap_add(&aref, 260 * PAGE_SIZE, a1, false));
        assert_eq!(lazy.am_ncused.get(), 2);
        assert_eq!(lazy.am_nused.get(), 2);
        assert!(amap_lookup(&aref, 260 * PAGE_SIZE).is_some_and(|a| ptr::eq(a, a1)));
        assert!(amap_lookup(&aref, 100 * PAGE_SIZE).is_none());
        amap_unadd(&aref, 0);
        assert_eq!(
            lazy.am_ncused.get(),
            1,
            "an emptied chunk goes back to the pool"
        );
        a0.an_ref.set(0);
        uvm_anfree(a0);
        amap_unlock(lazy);
        amap_unref(lazy, 0, slots, true);
    }

    #[test]
    fn ppref_counts_partial_references() {
        let _g = setup();
        let amap = amap_alloc(8 * PAGE_SIZE, M_WAITOK, false).expect("an amap");
        amap_ref(amap, 0, 8, AMAP_REFALL);
        assert_eq!(amap_refs(amap), 2);
        assert!(
            amap.am_ppref.get().is_null(),
            "a whole reference needs no ppref"
        );

        amap_ref(amap, 2, 4, 0);
        assert_eq!(amap_refs(amap), 3);
        let ppref = amap.ppref().expect("a ppref array");
        assert_eq!(pp_getreflen(ppref, 0), (2, 2));
        assert_eq!(pp_getreflen(ppref, 2), (3, 4));
        assert_eq!(pp_getreflen(ppref, 6), (2, 2));

        amap_unref(amap, 2, 4, false);
        assert_eq!(amap_refs(amap), 2);
        assert_eq!(
            pp_getreflen(ppref, 0),
            (2, 6),
            "the first changed entry merges with the unchanged one before it"
        );
        assert_eq!(pp_getreflen(ppref, 6), (2, 2));

        amap_unref(amap, 0, 8, true);
        assert_eq!(amap_refs(amap), 1);
        amap_unref(amap, 0, 8, true);
    }

    #[test]
    fn amap_copy_takes_over_or_copies() {
        let _g = setup();
        let map = VmMap::new();
        let entry = VmMapEntry::new();
        entry.start.set(0x1000);
        entry.end.set(0x1000 + 4 * PAGE_SIZE);
        entry.etype.set(UVM_ET_NEEDSCOPY);

        // no amap yet: one is made for the entry
        amap_copy(&map, &entry, M_WAITOK, false, 0, 0);
        let amap = entry.aref.amap().expect("an amap");
        assert_eq!(amap.am_nslot.get(), 4);
        assert_eq!(entry.etype.get() & UVM_ET_NEEDSCOPY, 0);

        // a sole reference is taken over
        entry.etype.set(UVM_ET_NEEDSCOPY);
        amap_copy(&map, &entry, M_WAITOK, false, 0, 0);
        assert!(ptr::eq(entry.aref.amap().expect("the amap"), amap));
        assert_eq!(entry.etype.get() & UVM_ET_NEEDSCOPY, 0);

        // a shared amap is copied: the anons gain a reference, the source loses one
        let anon = anon_for(amap);
        amap_lock(amap, RW_WRITE);
        assert!(amap_add(&entry.aref, PAGE_SIZE, anon, false));
        amap_unlock(amap);
        amap_ref(amap, 0, 4, AMAP_REFALL); // the other map entry, as fork makes it
        entry.etype.set(UVM_ET_NEEDSCOPY);
        amap_copy(&map, &entry, M_WAITOK, false, 0, 0);
        let copy = entry.aref.amap().expect("the copy");
        assert!(!ptr::eq(copy, amap));
        assert_eq!(amap_refs(amap), 1);
        assert_eq!(copy.am_nused.get(), 1);
        assert_eq!(anon.an_ref.get(), 2);
        assert!(
            ptr::eq(copy.am_lock.get(), amap.am_lock.get()),
            "a copy with shared anons shares the lock"
        );
        amap_lock(copy, RW_WRITE);
        assert!(amap_lookup(&entry.aref, PAGE_SIZE).is_some_and(|a| ptr::eq(a, anon)));
        amap_unlock(copy);

        amap_unref(copy, 0, 4, true);
        assert_eq!(anon.an_ref.get(), 1);
        amap_unref(amap, 0, 4, true);
    }
}
/* </TESTS> */
