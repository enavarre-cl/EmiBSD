/* <LICENSES> */
/*
 * Copyright (c) 2026 Emilio Navarrete Lineros <enavarre@outlook.com>
 *
 * Permission to use, copy, modify, and distribute this software for any
 * purpose with or without fee is hereby granted, provided that the above
 * copyright notice and this permission notice appear in all copies.
 *
 * THE SOFTWARE IS PROVIDED "AS IS" AND THE AUTHOR DISCLAIMS ALL WARRANTIES
 * WITH REGARD TO THIS SOFTWARE INCLUDING ALL IMPLIED WARRANTIES OF
 * MERCHANTABILITY AND FITNESS. IN NO EVENT SHALL THE AUTHOR BE LIABLE FOR
 * ANY SPECIAL, DIRECT, INDIRECT, OR CONSEQUENTIAL DAMAGES OR ANY DAMAGES
 * WHATSOEVER RESULTING FROM LOSS OF USE, DATA OR PROFITS, WHETHER IN AN
 * ACTION OF CONTRACT, NEGLIGENCE OR OTHER TORTIOUS ACTION, ARISING OUT OF
 * OR IN CONNECTION WITH THE USE OR PERFORMANCE OF THIS SOFTWARE.
 */

/*-
 * Copyright (c) 1997, 1999, 2000 The NetBSD Foundation, Inc.
 * All rights reserved.
 *
 * This code is derived from software contributed to The NetBSD Foundation
 * by Paul Kranenburg; by Jason R. Thorpe of the Numerical Aerospace
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
/* </LICENSES> */

/* <CODE> */
//! Pool resource management utility: `kern/subr_pool.c`.
//!
//! Upstream: sys/kern/subr_pool.c @ 3ce1f3f79392
//!
//! Memory is allocated in pages which are split into pieces according to the pool item size.
//! Each page is kept on one of three lists in the pool structure: `pr_emptypages`,
//! `pr_fullpages` and `pr_partpages`, for empty, full and partially-full pages respectively.
//! The individual pool items are on a linked list headed by `ph_items` in each page header.
//! The memory for building the page list is either taken from the allocated pages themselves
//! (for small pool items) or taken from an internal pool of page headers (`phpool`).
//!
//! Status: `wip`. Milestone M3 ports `pool_init`, `pool_destroy`, `pool_get`/`pool_put`
//! with `pool_do_get`/`pool_do_put`, the page management (`pool_p_alloc`/`free`/`insert`/
//! `remove`, `pool_update_curpage`, `pr_find_pagehead`), the request queue (`pool_request`,
//! `pool_runqueue`, `pool_get_done`, `pool_wakeup`), `pool_prime`, the watermarks and
//! limits, `pool_reclaim`/`pool_reclaim_all`, the page allocators and the lock operations.
//! `pool_walk` came with the NFS client's ddb printers (M10e); `sysctl_dopool` and the
//! non-`MULTIPROCESSOR` `pool_cache_*info` with the diagnostic tools (stage 2). M11a makes
//! the lock operations the real mutex and rwlock (so `pool_get` sleeps for memory as the C
//! does), takes `pr_refcnt` in `sysctl_dopool`, and ports the `MULTIPROCESSOR` per-CPU caches
//! (`pool_cache_init`, `pool_cache_get`/`put`, the list functions, `pool_cache_destroy`,
//! `pool_cache_gc`, the `pool_cache_*info` sysctls) and the garbage collector
//! (`pool_gc_sched`, `pool_gc_pages`). The ddb printers (`pool_printit`,
//! `db_show_all_pools`) and `pool_chk` are not here.
//!
//! ## Deviations
//! - `pl_init` has no `lock_type` argument (`WITNESS` is not configured).
//! - A `PR_WAITOK` `pool_get` that finds no memory while the kernel is `cold` or on proc0
//!   (or a thread-less host test) fails, where the C panics under `DIAGNOSTIC` ("cannot sleep
//!   for memory during boot") and spins in `msleep`'s cold path otherwise; after boot it
//!   queues a request and sleeps for `pool_runqueue` as in C.
//! - `poison_mem`/`poison_check` (`subr_poison.c`) are reported where `POOL_DEBUG` would
//!   call them, in the page lists and in the per-CPU caches; the double-put check under
//!   `DIAGNOSTIC` is ported.
//! - `splassert(pp->pr_ipl)` in `pool_do_get`/`pool_do_put` is not checked.
//! - `arc4random` (page magics, freelist order, the `XSIMPLEQ` cookies, the cache magics) is
//!   `dev/rnd.rs`'s stream.
//! - Fields the C reads without the lock as a guess (`pr_nidle` in `pool_gc_pages`, the
//!   request queue's emptiness in `pool_put`) are read as the C reads them; the per-CPU cache
//!   counters another CPU reads (`pc_gen`, `pc_nget`, ...) are relaxed atomics written only
//!   by their CPU, the generation number with release/acquire ordering.
//! - A cached item's `ci_nextl` link doubles as two words of magic while the item sits in a
//!   CPU's list (`pool_cache_item_magic`): the link is read and written as two `usize`s.

use core::cell::Cell;
use core::ptr::{self, NonNull};
use core::sync::atomic::{AtomicI32, AtomicU32, Ordering};
#[cfg(feature = "multiprocessor")]
use core::sync::atomic::{AtomicU64, fence};

use crate::dev::rnd::{arc4random, arc4random_buf};
use crate::kern::kern_lock::{mtx_enter, mtx_enter_try, mtx_init_flags, mtx_leave};
use crate::kern::kern_rwlock::{
    rw_assert_wrlock, rw_enter, rw_enter_read, rw_enter_write, rw_exit_read, rw_exit_write,
    rw_init_flags, rw_status,
};
use crate::kern::kern_synch::{
    msleep_nsec, refcnt_finalize, refcnt_init, refcnt_rele_wake, refcnt_take, rwsleep_nsec,
    wakeup_one,
};
use crate::kern::kern_sysctl::{sysctl_rdint, sysctl_rdstring, sysctl_rdstruct};
use crate::kern::kern_task::{SYSTQMP, task_add};
use crate::kern::kern_tc::getnsecuptime;
use crate::kern::kern_timeout::timeout_add_sec;
use crate::kern::subr_xxx::assertwaitok;
use crate::machine::intr::{IPL_HIGH, splvm, splx};
use crate::sys::errno::Errno;
use crate::sys::mutex::{mutex_assert_locked, mutex_assert_unlocked};
use crate::sys::param::{PAGE_SIZE, PSWP, align, roundup};
use crate::sys::pool::{
    KERN_POOL_CACHE, KERN_POOL_CACHE_CPUS, KERN_POOL_NAME, KERN_POOL_NPOOLS, KERN_POOL_POOL,
    KinfoPool, POOL_ALLOC_ALIGNED, POOL_ALLOC_DEFAULT, PR_LIMITFAIL, PR_NOWAIT, PR_RWLOCK,
    PR_WAITOK, PR_ZERO, Pool, PoolAllocator, PoolLock, PoolRequest, PoolRequestHandler, PrEntry,
    pool_alloc_size, pool_alloc_sizes,
};
use crate::sys::queue::{SimpleqHead, TailqEntry, TailqHead, XsimpleqEntry, XsimpleqHead};
use crate::sys::rwlock::{RW_NOSLEEP, RW_WRITE, Rwlock};
use crate::sys::systm::{INFSLP, kernel_lock, kernel_unlock};
use crate::sys::task::Task;
use crate::sys::timeout::Timeout;
use crate::sys::tree::RbtEntry;
#[cfg(feature = "diagnostic")]
use crate::unported;
use crate::uvm::uvm_extern::{KMEM_DYN_INITIALIZER, KmemPaMode, KmemVaMode};
use crate::uvm::uvm_km::{KP_DIRTY, KV_ANY, KV_INTRSAFE, KV_PAGE, km_alloc, km_free};
use crate::{kassert, queue_adapter, tree_adapter};
#[cfg(feature = "multiprocessor")]
use crate::{
    kern::kern_malloc::{free, mallocarray},
    kern::subr_percpu::{cpumem_get, ncpusfound},
    machine::intr::{IPL_NONE, splraise},
    sys::malloc::{M_CANFAIL, M_TEMP, M_WAITOK, M_ZERO},
    sys::percpu::{CACHELINESIZE, CpumemPtr, cpumem_enter, cpumem_foreach, cpumem_leave},
    sys::pool::{KinfoPoolCache, KinfoPoolCacheCpu},
};

/// `struct pool_item`: the free-list link at the start of every free item.
#[repr(C)]
pub struct PoolItem {
    /// `pi_magic`: the item's address XOR the page's magic; catches freelist corruption.
    pub pi_magic: Cell<usize>,
    /// `pi_list`: the page's free items.
    pub pi_list: XsimpleqEntry<PoolItem>,
}

queue_adapter!(
    /// `XSIMPLEQ_HEAD(, pool_item) ph_items`.
    pub PiList: PoolItem, pi_list => XsimpleqEntry<PoolItem>
);

/// `POOL_IMAGIC(ph, pi)`.
fn pool_imagic(ph: &PoolPageHeader, pi: *const PoolItem) -> usize {
    (pi as usize) ^ ph.ph_magic.get()
}

/// `struct pool_page_header`: one page of items.
pub struct PoolPageHeader {
    /// Pool page list.
    pub ph_entry: TailqEntry<PoolPageHeader>,
    /// Free items on the page.
    pub ph_items: XsimpleqHead<PiList>,
    /// Off-page page headers.
    pub ph_node: RbtEntry,
    /// # of chunks in use.
    pub ph_nmissing: Cell<u32>,
    /// This page's address.
    pub ph_page: Cell<*mut u8>,
    /// Page's colored address.
    pub ph_colored: Cell<*mut u8>,
    /// The page's magic (see `POOL_IMAGIC`).
    pub ph_magic: Cell<usize>,
    /// When the page became idle.
    pub ph_timestamp: Cell<u64>,
}

impl PoolPageHeader {
    /// A header that describes no page yet.
    pub const fn new() -> Self {
        Self {
            ph_entry: TailqEntry::new(),
            ph_items: XsimpleqHead::new(),
            ph_node: RbtEntry::new(),
            ph_nmissing: Cell::new(0),
            ph_page: Cell::new(ptr::null_mut()),
            ph_colored: Cell::new(ptr::null_mut()),
            ph_magic: Cell::new(0),
            ph_timestamp: Cell::new(0),
        }
    }
}

impl Default for PoolPageHeader {
    fn default() -> Self {
        Self::new()
    }
}

/// `POOL_MAGICBIT`: keep away from perturbed low bits.
#[cfg(feature = "diagnostic")]
const POOL_MAGICBIT: usize = 1 << 3;

/// `POOL_PHPOISON(ph)`: whether the page's free items are poisoned.
#[cfg(feature = "diagnostic")]
fn pool_phpoison(ph: &PoolPageHeader) -> bool {
    ph.ph_magic.get() & POOL_MAGICBIT != 0
}

queue_adapter!(
    /// `TAILQ_HEAD(pool_pagelist, pool_page_header)`.
    pub PhEntry: PoolPageHeader, ph_entry => TailqEntry<PoolPageHeader>
);

tree_adapter!(
    /// `RBT_HEAD(phtree, pool_page_header)`: off-page headers by page address.
    pub Phtree: PoolPageHeader, ph_node => RbtEntry, phtree_compare
);

/// `POOL_CACHE_LIST_MIN`: minimum list length.
#[cfg(feature = "multiprocessor")]
const POOL_CACHE_LIST_MIN: u32 = 8;
/// `POOL_CACHE_LIST_INC`.
#[cfg(feature = "multiprocessor")]
const POOL_CACHE_LIST_INC: u32 = 8;
/// `POOL_CACHE_LIST_DEC`.
#[cfg(feature = "multiprocessor")]
const POOL_CACHE_LIST_DEC: u32 = 1;

/// `struct pool_cache_item`: a free item in a CPU's cache, the head of a list of them.
#[repr(C)]
pub struct PoolCacheItem {
    /// `ci_next`: next item in list.
    pub ci_next: Cell<*mut PoolCacheItem>,
    /// `ci_nitems`: number of items in list (the high bit: the items are poisoned).
    pub ci_nitems: Cell<usize>,
    /// `ci_nextl`: entry in list of lists; two words of magic while in a CPU's list.
    pub ci_nextl: TailqEntry<PoolCacheItem>,
}

queue_adapter!(
    /// `TAILQ_HEAD(pool_cache_lists, pool_cache_item)`.
    pub CiList: PoolCacheItem, ci_nextl => TailqEntry<PoolCacheItem>
);

/// `POOL_CACHE_ITEM_NITEMS_MASK`: we store whether the cached item is poisoned in the high
/// bit of nitems.
#[cfg(feature = "multiprocessor")]
const POOL_CACHE_ITEM_NITEMS_MASK: usize = 0x7ff_ffff;
/// `POOL_CACHE_ITEM_NITEMS_POISON`.
#[cfg(all(feature = "multiprocessor", feature = "diagnostic"))]
const POOL_CACHE_ITEM_NITEMS_POISON: usize = 0x800_0000;

/// `POOL_CACHE_ITEM_NITEMS(_ci)`.
#[cfg(feature = "multiprocessor")]
fn pool_cache_item_nitems(ci: &PoolCacheItem) -> usize {
    ci.ci_nitems.get() & POOL_CACHE_ITEM_NITEMS_MASK
}

/// `POOL_CACHE_ITEM_POISONED(_ci)`.
#[cfg(all(feature = "multiprocessor", feature = "diagnostic"))]
fn pool_cache_item_poisoned(ci: &PoolCacheItem) -> bool {
    ci.ci_nitems.get() & POOL_CACHE_ITEM_NITEMS_POISON != 0
}

/// `struct pool_cache`: one CPU's cache. Every field is written only by its CPU, at
/// `pr_ipl`; the counters other CPUs read (`pool_cache_pool_info`, `pool_cache_cpus_info`)
/// are atomics under the generation number `pc_gen`, which is odd while the CPU works.
#[cfg(feature = "multiprocessor")]
pub struct PoolCache {
    /// `pc_actv`: active list of items.
    pub pc_actv: Cell<*mut PoolCacheItem>,
    /// `pc_nactv`: actv head nitems cache.
    pub pc_nactv: Cell<usize>,
    /// `pc_prev`: previous list of items.
    pub pc_prev: Cell<*mut PoolCacheItem>,
    /// `pc_gen`: generation number.
    pub pc_gen: AtomicU64,
    /// `pc_nget`: # of successful requests.
    pub pc_nget: AtomicU64,
    /// `pc_nfail`: # of unsuccessful reqs.
    pub pc_nfail: AtomicU64,
    /// `pc_nput`: # of releases.
    pub pc_nput: AtomicU64,
    /// `pc_nlget`: # of list requests.
    pub pc_nlget: AtomicU64,
    /// `pc_nlfail`: # of fails getting a list.
    pub pc_nlfail: AtomicU64,
    /// `pc_nlput`: # of list releases.
    pub pc_nlput: AtomicU64,
    /// `pc_nout`: items out through this cache, folded into `pr_cache_nout` under the list
    /// lock.
    pub pc_nout: AtomicI32,
}

/// `struct pool_lock_ops`: how a pool's locks are taken (mutex or rwlock).
pub struct PoolLockOps {
    /// `pl_init`.
    pub pl_init: fn(&Pool, &PoolLock),
    /// `pl_enter`.
    pub pl_enter: fn(&PoolLock),
    /// `pl_enter_try`.
    pub pl_enter_try: fn(&PoolLock) -> bool,
    /// `pl_leave`.
    pub pl_leave: fn(&PoolLock),
    /// `pl_assert_locked`.
    pub pl_assert_locked: fn(&PoolLock),
    /// `pl_assert_unlocked`.
    pub pl_assert_unlocked: fn(&PoolLock),
    /// `pl_sleep`: sleeps on `ident` with the lock released, and takes it again.
    pub pl_sleep: fn(*const (), &PoolLock, i32, &'static str) -> Result<(), Errno>,
}

/// `POOL_WAIT_FREE`: an idle page is freed by `pool_put` after this long.
const POOL_WAIT_FREE: u64 = 1_000_000_000;
/// `POOL_WAIT_GC`: the garbage collector's period.
pub const POOL_WAIT_GC: u64 = 8_000_000_000;

queue_adapter!(
    /// The list of all pools.
    PoolListHead: Pool, pr_poollist => crate::sys::queue::SimpleqEntry<Pool>
);

/// The list of all pools, as a static.
struct PoolHead(SimpleqHead<PoolListHead>);

// SAFETY: guarded by `pool_lock`, the rwlock below.
unsafe impl Sync for PoolHead {}

/// `pool_head`: list of all pools.
static POOL_HEAD: PoolHead = PoolHead(SimpleqHead::new());
/// `pool_serial`: every pool gets a unique serial number assigned to it. If this counter
/// wraps, we're screwed, but we shouldn't create so many pools anyway.
static POOL_SERIAL: AtomicU32 = AtomicU32::new(0);
/// `pool_count`.
static POOL_COUNT: AtomicU32 = AtomicU32::new(0);
/// `pool_lock`: the rwlock over the previous variables.
static POOL_LOCK: Rwlock = Rwlock::new("pools");
/// `phpool`: private pool for page header structures.
pub static PHPOOL: Pool = Pool::new();
/// `pool_gc_tick`: runs `pool_gc_sched` every second once `pool_gc_pages` started it.
static POOL_GC_TICK: Timeout = Timeout::new(pool_gc_sched, ptr::null_mut());
/// `pool_gc_task`: `pool_gc_pages` on `systqmp`.
static POOL_GC_TASK: Task = Task::new(pool_gc_pages, ptr::null_mut());
/// `pool_caches`: per cpu cache entries.
#[cfg(feature = "multiprocessor")]
static POOL_CACHES: Pool = Pool::new();
/// `pool_debug`: 1 with `POOL_DEBUG`, 2 forces a yield on every waiting get.
#[cfg(feature = "pool_debug")]
pub static POOL_DEBUG: AtomicI32 = AtomicI32::new(1);
/// `pool_debug`: 1 with `POOL_DEBUG`, 2 forces a yield on every waiting get.
#[cfg(not(feature = "pool_debug"))]
pub static POOL_DEBUG: AtomicI32 = AtomicI32::new(0);

/// `pool_allocator_single`: safe for interrupts; this is the default allocator.
pub static POOL_ALLOCATOR_SINGLE: PoolAllocator = PoolAllocator {
    pa_alloc: pool_page_alloc,
    pa_free: pool_page_free,
    pa_pagesz: pool_alloc_size(PAGE_SIZE, POOL_ALLOC_ALIGNED),
};

/// `pool_allocator_multi`: pages of several sizes, from `kmem_map`.
pub static POOL_ALLOCATOR_MULTI: PoolAllocator = PoolAllocator {
    pa_alloc: pool_multi_alloc,
    pa_free: pool_multi_free,
    pa_pagesz: pool_alloc_sizes(PAGE_SIZE, 1 << 31, POOL_ALLOC_ALIGNED),
};

/// `pool_allocator_multi_ni`: as `pool_allocator_multi`, not interrupt safe (may sleep).
pub static POOL_ALLOCATOR_MULTI_NI: PoolAllocator = PoolAllocator {
    pa_alloc: pool_multi_alloc_ni,
    pa_free: pool_multi_free_ni,
    pa_pagesz: pool_alloc_sizes(PAGE_SIZE, 1 << 31, POOL_ALLOC_ALIGNED),
};

/// `pool_lock_ops_mtx`.
static POOL_LOCK_OPS_MTX: PoolLockOps = PoolLockOps {
    pl_init: pool_lock_mtx_init,
    pl_enter: pool_lock_mtx_enter,
    pl_enter_try: pool_lock_mtx_enter_try,
    pl_leave: pool_lock_mtx_leave,
    pl_assert_locked: pool_lock_mtx_assert_locked,
    pl_assert_unlocked: pool_lock_mtx_assert_unlocked,
    pl_sleep: pool_lock_mtx_sleep,
};

/// `pool_lock_ops_rw`.
static POOL_LOCK_OPS_RW: PoolLockOps = PoolLockOps {
    pl_init: pool_lock_rw_init,
    pl_enter: pool_lock_rw_enter,
    pl_enter_try: pool_lock_rw_enter_try,
    pl_leave: pool_lock_rw_leave,
    pl_assert_locked: pool_lock_rw_assert_locked,
    pl_assert_unlocked: pool_lock_rw_assert_unlocked,
    pl_sleep: pool_lock_rw_sleep,
};

fn lock_ops(pp: &Pool) -> &'static PoolLockOps {
    pp.pr_lock_ops.get().unwrap_or(&POOL_LOCK_OPS_MTX)
}

fn pl_init(pp: &Pool, pl: &PoolLock) {
    (lock_ops(pp).pl_init)(pp, pl);
}

fn pl_enter(pp: &Pool, pl: &PoolLock) {
    (lock_ops(pp).pl_enter)(pl);
}

fn pl_enter_try(pp: &Pool, pl: &PoolLock) -> bool {
    (lock_ops(pp).pl_enter_try)(pl)
}

fn pl_leave(pp: &Pool, pl: &PoolLock) {
    (lock_ops(pp).pl_leave)(pl);
}

fn pl_assert_locked(pp: &Pool, pl: &PoolLock) {
    (lock_ops(pp).pl_assert_locked)(pl);
}

fn pl_assert_unlocked(pp: &Pool, pl: &PoolLock) {
    (lock_ops(pp).pl_assert_unlocked)(pl);
}

fn pl_sleep(
    pp: &Pool,
    ident: *const (),
    lock: &PoolLock,
    priority: i32,
    wmesg: &'static str,
) -> Result<(), Errno> {
    (lock_ops(pp).pl_sleep)(ident, lock, priority, wmesg)
}

/// `POOL_INPGHDR(pp)`: the page header lives inside the page.
fn pool_inpghdr(pp: &Pool) -> bool {
    pp.pr_phoffset.get() != 0
}

/// `phtree_compare`: by page address, reversed so that `RBT_NFIND` on an item address gives
/// the page at or below it. The compares in this order are important for the NFIND to work.
fn phtree_compare(a: &PoolPageHeader, b: &PoolPageHeader) -> core::cmp::Ordering {
    let va = a.ph_page.get() as usize;
    let vb = b.ph_page.get() as usize;

    if vb < va {
        core::cmp::Ordering::Less
    } else if vb > va {
        core::cmp::Ordering::Greater
    } else {
        core::cmp::Ordering::Equal
    }
}

/// The page header at `ph`, which lives as long as its page.
///
/// # Safety
///
/// `ph` must point at a page header `pool_p_alloc` made and `pool_p_free` has not freed.
unsafe fn ph_ref<'a>(ph: *const PoolPageHeader) -> &'a PoolPageHeader {
    // SAFETY: the caller's guarantee.
    unsafe { &*ph }
}

/// `pr_find_pagehead`: return the pool page header based on page address.
fn pr_find_pagehead(pp: &Pool, v: *mut u8) -> &'static PoolPageHeader {
    if pool_inpghdr(pp) {
        let page = (v as usize) & pp.pr_pgmask.get();
        // SAFETY: an item of this pool lies in a page whose header `pool_p_alloc` wrote at
        // `pr_phoffset`.
        return unsafe { ph_ref((page + pp.pr_phoffset.get() as usize) as *const PoolPageHeader) };
    }

    let key = PoolPageHeader::new();
    key.ph_page.set(v);
    let Some(ph) = pp.pr_phtree.nfind(&key) else {
        #[allow(clippy::panic)] // the C panics here too
        {
            panic!(
                "pr_find_pagehead: {}: page header missing",
                pp.pr_wchan.get()
            );
        }
    };

    kassert!(ph.ph_page.get() as usize <= v as usize);
    if ph.ph_page.get() as usize + pp.pr_pgsize.get() as usize <= v as usize {
        #[allow(clippy::panic)] // the C panics here too
        {
            panic!("pr_find_pagehead: {}: incorrect page", pp.pr_wchan.get());
        }
    }

    // SAFETY: off-page headers are in the tree only while their page is allocated.
    unsafe { ph_ref(ph) }
}

/// `pool_init`: initialize the given pool resource structure. We export this routine to allow
/// other kernel parts to declare static pools that must be initialized before `malloc()` is
/// available.
pub fn pool_init(
    pp: &'static Pool,
    size: usize,
    align_: u32,
    ipl: i32,
    flags: i32,
    wchan: &'static str,
    palloc: Option<&'static PoolAllocator>,
) {
    let mut off = 0usize;
    let mut pgsize = PAGE_SIZE;

    let align_ = if align_ == 0 { align(1) as u32 } else { align_ };

    let size = size.max(size_of::<PoolItem>());

    let size = roundup(size, align_ as usize);

    while size * 8 > pgsize {
        pgsize <<= 1;
    }

    let (palloc, pa_pagesz) = match palloc {
        None => {
            let palloc = if pgsize > PAGE_SIZE {
                if flags & PR_WAITOK != 0 {
                    &POOL_ALLOCATOR_MULTI_NI
                } else {
                    &POOL_ALLOCATOR_MULTI
                }
            } else {
                &POOL_ALLOCATOR_SINGLE
            };
            (palloc, palloc.pa_pagesz)
        }
        Some(palloc) => {
            let mut pa_pagesz = palloc.pa_pagesz;
            if pa_pagesz == 0 {
                pa_pagesz = POOL_ALLOC_DEFAULT;
            }

            let pgsizes = pa_pagesz & !POOL_ALLOC_ALIGNED;

            // make sure the allocator can fit at least one item
            if size > pgsizes {
                #[allow(clippy::panic)] // the C panics here too
                {
                    panic!(
                        "pool_init: pool {} item size {:#x} > allocator {:p} sizes {:#x}",
                        wchan, size, palloc, pgsizes
                    );
                }
            }

            // shrink pgsize until it fits into the range
            while pgsizes & pgsize == 0 {
                pgsize >>= 1;
            }
            (palloc, pa_pagesz)
        }
    };
    kassert!(pa_pagesz & pgsize != 0);

    let mut items = pgsize / size;

    // Decide whether to put the page header off page to avoid wasting too large a part of
    // the page. Off-page page headers go into an RB tree, so we can match a returned item
    // with its header based on the page address.
    if pa_pagesz & POOL_ALLOC_ALIGNED != 0 {
        if pgsize - (size * items) > size_of::<PoolPageHeader>() {
            off = pgsize - size_of::<PoolPageHeader>();
        } else if size_of::<PoolPageHeader>() * 2 >= size {
            off = pgsize - size_of::<PoolPageHeader>();
            items = off / size;
        }
    }

    kassert!(items > 0);

    // Initialize the pool structure.
    refcnt_init(&pp.pr_refcnt);
    if flags & PR_RWLOCK != 0 {
        kassert!(flags & PR_WAITOK != 0);
        pp.pr_lock_ops.set(Some(&POOL_LOCK_OPS_RW));
    } else {
        pp.pr_lock_ops.set(Some(&POOL_LOCK_OPS_MTX));
    }
    pp.pr_emptypages.init();
    pp.pr_fullpages.init();
    pp.pr_partpages.init();
    pp.pr_curpage.set(ptr::null());
    pp.pr_npages.set(0);
    pp.pr_minitems.set(0);
    pp.pr_minpages.set(0);
    pp.pr_maxpages.set(8);
    pp.pr_size.set(size as u32);
    pp.pr_pgsize.set(pgsize as u32);
    pp.pr_pgmask.set(!0usize ^ (pgsize - 1));
    pp.pr_phoffset.set(off as i32);
    pp.pr_itemsperpage.set(items as u32);
    pp.pr_wchan.set(wchan);
    pp.pr_alloc.set(Some(palloc));
    pp.pr_nitems.set(0);
    pp.pr_nout.set(0);
    pp.pr_hardlimit.set(u32::MAX);
    pp.pr_phtree.init();

    // Use the space between the chunks and the page header for cache coloring.
    let space = if pool_inpghdr(pp) {
        pp.pr_phoffset.get() as usize
    } else {
        pp.pr_pgsize.get() as usize
    };
    let space = space - pp.pr_itemsperpage.get() as usize * pp.pr_size.get() as usize;
    pp.pr_align.set(align_);
    pp.pr_maxcolors.set((space / align_ as usize) as u32 + 1);

    pp.pr_nget.set(0);
    pp.pr_nfail.set(0);
    pp.pr_nput.set(0);
    pp.pr_npagealloc.set(0);
    pp.pr_npagefree.set(0);
    pp.pr_hiwat.set(0);
    pp.pr_nidle.set(0);

    pp.pr_ipl.set(ipl);
    pp.pr_flags.set(flags);

    pl_init(pp, &pp.pr_lock);
    pl_init(pp, &pp.pr_requests_lock);
    pp.pr_requests.init();

    if PHPOOL.pr_size.get() == 0 {
        pool_init(
            &PHPOOL,
            size_of::<PoolPageHeader>(),
            0,
            IPL_HIGH,
            0,
            "phpool",
            None,
        );

        // make sure phpool won't "recurse"
        kassert!(pool_inpghdr(&PHPOOL));
    }

    // pglistalloc/constraint parameters
    pp.pr_crange.set(Some(&KP_DIRTY));

    // Insert this into the list of all pools.
    rw_enter_write(&POOL_LOCK);
    #[cfg(feature = "diagnostic")]
    for iter in POOL_HEAD.0.iter() {
        if ptr::eq(iter, pp) {
            #[allow(clippy::panic)] // the C panics here too
            {
                panic!("pool_init: pool {} already on list", wchan);
            }
        }
    }

    let serial = POOL_SERIAL.fetch_add(1, Ordering::Relaxed) + 1;
    pp.pr_serial.set(serial);
    if serial == 0 {
        #[allow(clippy::panic)] // the C panics here too
        {
            panic!("pool_init: too much uptime");
        }
    }

    // SAFETY: a pool is initialised once (the DIAGNOSTIC check above is the C's), so it is on
    // no list.
    unsafe { POOL_HEAD.0.insert_head(pp) };
    POOL_COUNT.fetch_add(1, Ordering::Relaxed);
    rw_exit_write(&POOL_LOCK);
}

/// `pool_destroy`: decommission a pool resource.
pub fn pool_destroy(pp: &'static Pool) {
    #[cfg(feature = "diagnostic")]
    if pp.pr_nout.get() != 0 {
        #[allow(clippy::panic)] // the C panics here too
        {
            panic!("pool_destroy: pool busy: still out: {}", pp.pr_nout.get());
        }
    }

    // Remove from global pool list
    rw_enter_write(&POOL_LOCK);
    POOL_COUNT.fetch_sub(1, Ordering::Relaxed);
    if POOL_HEAD.0.first().is_some_and(|first| ptr::eq(first, pp)) {
        // SAFETY: `pp` is the head of the list.
        unsafe { POOL_HEAD.0.remove_head() };
    } else {
        let mut prev = POOL_HEAD.0.first();
        for iter in POOL_HEAD.0.iter() {
            if ptr::eq(iter, pp) {
                if let Some(prev) = prev {
                    // SAFETY: `prev` is on the list and `pp` follows it.
                    unsafe { POOL_HEAD.0.remove_after(prev) };
                }
                break;
            }
            prev = Some(iter);
        }
    }
    rw_exit_write(&POOL_LOCK);

    // Wait for concurrent sysctl_dopool()
    refcnt_finalize(&pp.pr_refcnt, "pooldtor");

    #[cfg(feature = "multiprocessor")]
    if !pp.pr_cache.load(Ordering::Acquire).is_null() {
        pool_cache_destroy(pp);
    }

    // Remove all pages
    while let Some(ph) = pp.pr_emptypages.first() {
        let ph: *const PoolPageHeader = ph;
        pl_enter(pp, &pp.pr_lock);
        // SAFETY: the header is on the pool's empty list, so its page is allocated.
        pool_p_remove(pp, unsafe { ph_ref(ph) });
        pl_leave(pp, &pp.pr_lock);
        // SAFETY: as above; `pool_p_free` is its last use.
        pool_p_free(pp, unsafe { ph_ref(ph) });
    }
    kassert!(pp.pr_fullpages.is_empty());
    kassert!(pp.pr_partpages.is_empty());
}

/// `pool_request_init`.
pub fn pool_request_init(
    pr: &PoolRequest,
    handler: PoolRequestHandler,
    cookie: *mut (),
) -> PoolRequest {
    let _ = pr;
    PoolRequest {
        pr_entry: TailqEntry::new(),
        pr_handler: handler,
        pr_cookie: cookie,
        pr_item: Cell::new(None),
    }
}

/// `pool_request`: queues a request and runs the queue.
///
/// # Safety
///
/// `pr` must outlive its stay on the queue: until its handler has been called.
pub unsafe fn pool_request(pp: &Pool, pr: &PoolRequest) {
    pl_enter(pp, &pp.pr_requests_lock);
    // SAFETY: the caller's guarantee; a request is queued once.
    unsafe { pp.pr_requests.insert_tail(pr) };
    pool_runqueue(pp, PR_NOWAIT);
    pl_leave(pp, &pp.pr_requests_lock);
}

/// `struct pool_get_memory`: what a sleeping `pool_get` waits on.
pub struct PoolGetMemory {
    /// Guards `v`.
    pub lock: PoolLock,
    /// The item, once a request run delivers it.
    pub v: Cell<Option<NonNull<u8>>>,
}

/// `pool_get`: grab an item from the pool. With `PR_WAITOK` and no memory it queues a
/// request and sleeps until `pool_runqueue` serves it.
pub fn pool_get(pp: &Pool, flags: i32) -> Option<NonNull<u8>> {
    let mut v: Option<NonNull<u8>> = None;
    let mut slowdown = 0;

    if flags & PR_WAITOK != 0 {
        assertwaitok();
    }

    kassert!(flags & (PR_WAITOK | PR_NOWAIT) != 0);
    if pp.pr_flags.get() & PR_RWLOCK != 0 {
        kassert!(flags & PR_WAITOK != 0);
    }

    #[cfg(feature = "multiprocessor")]
    if !pp.pr_cache.load(Ordering::Acquire).is_null()
        && let Some(v) = pool_cache_get(pp)
    {
        return Some(pool_get_good(pp, v, flags));
    }

    pl_enter(pp, &pp.pr_lock);
    if pp.pr_nout.get() >= pp.pr_hardlimit.get() {
        if flags & (PR_NOWAIT | PR_LIMITFAIL) != 0 {
            return pool_get_fail(pp);
        }
    } else {
        v = pool_do_get(pp, flags, &mut slowdown);
        if v.is_none() && flags & PR_NOWAIT != 0 {
            return pool_get_fail(pp);
        }
    }
    pl_leave(pp, &pp.pr_lock);

    if (slowdown != 0 || POOL_DEBUG.load(Ordering::Relaxed) == 2) && flags & PR_WAITOK != 0 {
        crate::kern::sched_bsd::r#yield();
    }

    let v = match v {
        Some(v) => v,
        None => {
            let mem = PoolGetMemory {
                lock: PoolLock::new(),
                v: Cell::new(None),
            };

            // During boot nothing can be waited for: the C panics under DIAGNOSTIC when proc0
            // would sleep, and otherwise spins in msleep's cold path; here the get fails (see
            // the module's deviations).
            if flags & PR_WAITOK != 0
                && (crate::sys::systm::COLD.load(Ordering::Relaxed)
                    || crate::machine::cpu::curproc()
                        .is_none_or(|p| ptr::eq(p, &crate::kern::init_main::PROC0)))
            {
                #[cfg(feature = "diagnostic")]
                crate::kern::subr_prf::panic(format_args!(
                    "pool_get: cannot sleep for memory during boot"
                ));
                #[cfg(not(feature = "diagnostic"))]
                {
                    pl_enter(pp, &pp.pr_lock);
                    return pool_get_fail(pp);
                }
            }
            pl_init(pp, &mem.lock);
            // pool_request_init(&pr, pool_get_done, &mem)
            let pr = PoolRequest {
                pr_entry: TailqEntry::new(),
                pr_handler: pool_get_done,
                pr_cookie: ptr::from_ref(&mem).cast_mut().cast(),
                pr_item: Cell::new(None),
            };
            // SAFETY: `pr` stays on this stack until `pool_get_done` has run: this thread
            // sleeps below until the handler stores the item, and `pool_runqueue` takes a
            // request off the queue before calling its handler.
            unsafe { pool_request(pp, &pr) };

            pl_enter(pp, &mem.lock);
            while mem.v.get().is_none() {
                let _ = pl_sleep(
                    pp,
                    ptr::from_ref(&mem).cast(),
                    &mem.lock,
                    PSWP,
                    pp.pr_wchan.get(),
                );
            }
            pl_leave(pp, &mem.lock);

            mem.v.get()?
        }
    };

    Some(pool_get_good(pp, v, flags))
}

/// `pool_get`'s `good:` label: `PR_ZERO`.
fn pool_get_good(pp: &Pool, v: NonNull<u8>, flags: i32) -> NonNull<u8> {
    if flags & PR_ZERO != 0 {
        // SAFETY: `v` is a free item of `pr_size` bytes that is now the caller's.
        unsafe { ptr::write_bytes(v.as_ptr(), 0, pp.pr_size.get() as usize) };
    }

    // TRACEPOINT(uvm, pool_get): not configured.

    v
}

/// `pool_get`'s `fail:` label: counts the failure and drops the lock.
fn pool_get_fail(pp: &Pool) -> Option<NonNull<u8>> {
    pp.pr_nfail.set(pp.pr_nfail.get() + 1);
    pl_leave(pp, &pp.pr_lock);
    None
}

/// `pool_get_done`: the request handler of a sleeping `pool_get`.
pub fn pool_get_done(pp: &Pool, xmem: *mut (), v: NonNull<u8>) {
    // SAFETY: the cookie is the `PoolGetMemory` of the sleeping pool_get, which waits for it.
    let mem = unsafe { &*(xmem as *const PoolGetMemory) };

    pl_enter(pp, &mem.lock);
    mem.v.set(Some(v));
    pl_leave(pp, &mem.lock);

    wakeup_one(xmem);
}

/// `pool_runqueue`: serves the queued requests that memory allows.
pub fn pool_runqueue(pp: &Pool, flags: i32) {
    let prl: TailqHead<PrEntry> = TailqHead::new();
    prl.init();

    pl_assert_unlocked(pp, &pp.pr_lock);
    pl_assert_locked(pp, &pp.pr_requests_lock);

    let requesting = pp.pr_requesting.get();
    pp.pr_requesting.set(requesting + 1);
    if requesting != 0 {
        return;
    }

    loop {
        pp.pr_requesting.set(1);

        // SAFETY: both queues are this pool's; the requests lock is held.
        unsafe { prl.concat(&pp.pr_requests) };
        if !prl.is_empty() {
            pl_leave(pp, &pp.pr_requests_lock);

            pl_enter(pp, &pp.pr_lock);
            let mut pr = prl.first();
            while let Some(r) = pr {
                let mut slowdown = 0;

                if pp.pr_nout.get() >= pp.pr_hardlimit.get() {
                    break;
                }

                r.pr_item.set(pool_do_get(pp, flags, &mut slowdown));
                if r.pr_item.get().is_none() {
                    // || slowdown ?
                    break;
                }

                pr = TailqHead::<PrEntry>::next(r);
            }
            pl_leave(pp, &pp.pr_lock);

            while let Some(r) = prl.first() {
                let Some(item) = r.pr_item.get() else {
                    break;
                };
                // SAFETY: `r` is the head of `prl`.
                unsafe { prl.remove(r) };
                (r.pr_handler)(pp, r.pr_cookie, item);
            }

            pl_enter(pp, &pp.pr_requests_lock);
        }
        let left = pp.pr_requesting.get() - 1;
        pp.pr_requesting.set(left);
        if left == 0 {
            break;
        }
    }

    // SAFETY: as above.
    unsafe { pp.pr_requests.concat(&prl) };
}

/// `pool_do_get`: takes one item, allocating a page when none is free.
pub fn pool_do_get(pp: &Pool, flags: i32, slowdown: &mut i32) -> Option<NonNull<u8>> {
    pl_assert_locked(pp, &pp.pr_lock);

    // splassert(pp->pr_ipl): M4.

    // Account for this item now to avoid races if we need to give up pr_lock to allocate a
    // page.
    pp.pr_nout.set(pp.pr_nout.get() + 1);

    if pp.pr_curpage.get().is_null() {
        pl_leave(pp, &pp.pr_lock);
        let ph = pool_p_alloc(pp, flags, slowdown);
        pl_enter(pp, &pp.pr_lock);

        let Some(ph) = ph else {
            pp.pr_nout.set(pp.pr_nout.get() - 1);
            return None;
        };

        pool_p_insert(pp, ph);
    }

    // SAFETY: `pr_curpage` names a page the pool holds.
    let ph = unsafe { ph_ref(pp.pr_curpage.get()) };
    let Some(pi) = ph.ph_items.first() else {
        #[allow(clippy::panic)] // the C panics here too
        {
            panic!("pool_do_get: {}: page empty", pp.pr_wchan.get());
        }
    };
    let pi: *const PoolItem = pi;
    // SAFETY: a linked item is valid until unlinked.
    let pi_ref = unsafe { &*pi };

    if pi_ref.pi_magic.get() != pool_imagic(ph, pi) {
        #[allow(clippy::panic)] // the C panics here too
        {
            panic!(
                "pool_do_get: {} free list modified: page {:p}; item addr {:p}; offset 0x0={:#x} != {:#x}",
                pp.pr_wchan.get(),
                ph.ph_page.get(),
                pi,
                pi_ref.pi_magic.get(),
                pool_imagic(ph, pi)
            );
        }
    }

    // SAFETY: `pi` is the head of `ph_items`.
    unsafe { ph.ph_items.remove_head() };

    #[cfg(feature = "diagnostic")]
    if POOL_DEBUG.load(Ordering::Relaxed) != 0 && pool_phpoison(ph) {
        let _ = unported!("poison_check (subr_poison.c) in pool_do_get");
    }

    let nmissing = ph.ph_nmissing.get();
    ph.ph_nmissing.set(nmissing + 1);
    if nmissing == 0 {
        // This page was previously empty. Move it to the list of partially-full pages. This
        // page is already curpage.
        // SAFETY: the header is on the empty list (it had no missing items).
        unsafe {
            pp.pr_emptypages.remove(ph);
            pp.pr_partpages.insert_tail(ph);
        }

        pp.pr_nidle.set(pp.pr_nidle.get() - 1);
    }

    if ph.ph_nmissing.get() == pp.pr_itemsperpage.get() {
        // This page is now full. Move it to the full list and select a new current page.
        // SAFETY: the header is on the partial list.
        unsafe {
            pp.pr_partpages.remove(ph);
            pp.pr_fullpages.insert_tail(ph);
        }
        pool_update_curpage(pp);
    }

    pp.pr_nget.set(pp.pr_nget.get() + 1);

    NonNull::new(pi.cast_mut().cast::<u8>())
}

/// `pool_put`: return resource to the pool.
pub fn pool_put(pp: &Pool, v: NonNull<u8>) {
    let mut freeph: Option<&PoolPageHeader> = None;

    // TRACEPOINT(uvm, pool_put): not configured.

    #[cfg(feature = "multiprocessor")]
    if !pp.pr_cache.load(Ordering::Acquire).is_null() && pp.pr_requests.is_empty() {
        pool_cache_put(pp, v);
        return;
    }

    pl_enter(pp, &pp.pr_lock);

    pool_do_put(pp, v);

    pp.pr_nout.set(pp.pr_nout.get() - 1);
    pp.pr_nput.set(pp.pr_nput.get() + 1);

    // is it time to free a page?
    if pp.pr_nidle.get() > u64::from(pp.pr_maxpages.get())
        && let Some(ph) = pp.pr_emptypages.first()
        && getnsecuptime().wrapping_sub(ph.ph_timestamp.get()) > POOL_WAIT_FREE
    {
        // SAFETY: the header is on the empty list, so its page is allocated.
        let ph = unsafe { ph_ref(ph) };
        freeph = Some(ph);
        pool_p_remove(pp, ph);
    }

    pl_leave(pp, &pp.pr_lock);

    if let Some(ph) = freeph {
        pool_p_free(pp, ph);
    }

    pool_wakeup(pp);
}

/// `pool_wakeup`: runs the request queue when something is waiting.
pub fn pool_wakeup(pp: &Pool) {
    if !pp.pr_requests.is_empty() {
        pl_enter(pp, &pp.pr_requests_lock);
        pool_runqueue(pp, PR_NOWAIT);
        pl_leave(pp, &pp.pr_requests_lock);
    }
}

/// `pool_do_put`: puts one item back on its page.
pub fn pool_do_put(pp: &Pool, v: NonNull<u8>) {
    let pi = v.as_ptr().cast::<PoolItem>();

    // splassert(pp->pr_ipl): M4.

    let ph = pr_find_pagehead(pp, v.as_ptr());

    #[cfg(feature = "diagnostic")]
    if POOL_DEBUG.load(Ordering::Relaxed) != 0 {
        for qi in ph.ph_items.iter() {
            if ptr::eq(qi, pi) {
                #[allow(clippy::panic)] // the C panics here too
                {
                    panic!(
                        "pool_do_put: {}: double pool_put: {:p}",
                        pp.pr_wchan.get(),
                        pi
                    );
                }
            }
        }
    }

    // SAFETY: the item is `pr_size` (>= a pool_item) bytes of memory the caller returns; any
    // bit pattern is a valid pool_item (two words).
    let pi_ref = unsafe { &*pi };
    pi_ref.pi_magic.set(pool_imagic(ph, pi));
    // SAFETY: the item is on no list (the DIAGNOSTIC check above is the C's).
    unsafe { ph.ph_items.insert_head(pi_ref) };
    #[cfg(feature = "diagnostic")]
    if pool_phpoison(ph) {
        let _ = unported!("poison_mem (subr_poison.c) in pool_do_put");
    }

    let nmissing = ph.ph_nmissing.get();
    ph.ph_nmissing.set(nmissing - 1);
    if nmissing == pp.pr_itemsperpage.get() {
        // The page was previously completely full, move it to the partially-full list.
        // SAFETY: the header is on the full list.
        unsafe {
            pp.pr_fullpages.remove(ph);
            pp.pr_partpages.insert_tail(ph);
        }
    }

    if ph.ph_nmissing.get() == 0 {
        // The page is now empty, so move it to the empty page list.
        pp.pr_nidle.set(pp.pr_nidle.get() + 1);

        ph.ph_timestamp.set(getnsecuptime());
        // SAFETY: the header is on the partial list.
        unsafe {
            pp.pr_partpages.remove(ph);
            pp.pr_emptypages.insert_tail(ph);
        }
        pool_update_curpage(pp);
    }
}

/// `pool_prime`: add N items to the pool.
pub fn pool_prime(pp: &Pool, n: u32) -> Result<(), Errno> {
    let pl: TailqHead<PhEntry> = TailqHead::new();
    pl.init();

    let itemsperpage = pp.pr_itemsperpage.get();
    let mut newpages = roundup(n as usize, itemsperpage as usize) / itemsperpage as usize;

    while newpages > 0 {
        newpages -= 1;
        let mut slowdown = 0;

        let Some(ph) = pool_p_alloc(pp, PR_NOWAIT, &mut slowdown) else {
            break; // or slowdown?
        };

        // SAFETY: a fresh header, on no list.
        unsafe { pl.insert_tail(ph) };
    }

    pl_enter(pp, &pp.pr_lock);
    while let Some(ph) = pl.first() {
        let ph: *const PoolPageHeader = ph;
        // SAFETY: `ph` is the head of `pl`, a header made above.
        let ph = unsafe { ph_ref(ph) };
        // SAFETY: as above.
        unsafe { pl.remove(ph) };
        pool_p_insert(pp, ph);
    }
    pl_leave(pp, &pp.pr_lock);

    Ok(())
}

/// `pool_p_alloc`: allocates a page and carves it into free items.
pub fn pool_p_alloc(pp: &Pool, flags: i32, slowdown: &mut i32) -> Option<&'static PoolPageHeader> {
    pl_assert_unlocked(pp, &pp.pr_lock);
    kassert!(pp.pr_size.get() as usize >= size_of::<PoolItem>());

    let addr = pool_allocator_alloc(pp, flags, slowdown)?;

    let ph: *mut PoolPageHeader = if pool_inpghdr(pp) {
        addr.as_ptr()
            .wrapping_add(pp.pr_phoffset.get() as usize)
            .cast::<PoolPageHeader>()
    } else {
        let Some(ph) = pool_get(&PHPOOL, flags) else {
            pool_allocator_free(pp, addr);
            return None;
        };
        ph.as_ptr().cast::<PoolPageHeader>()
    };
    // SAFETY: `ph` is header-sized memory at the end of the page or an item of phpool,
    // aligned (pr_phoffset keeps the page's alignment; phpool's items are 8-aligned).
    unsafe { ptr::write(ph, PoolPageHeader::new()) };
    // SAFETY: just written; it lives until pool_p_free.
    let ph = unsafe { ph_ref(ph) };

    let mut cookie = [0u8; size_of::<usize>()];
    arc4random_buf(&mut cookie);
    ph.ph_items.init(usize::from_ne_bytes(cookie));
    ph.ph_page.set(addr.as_ptr());
    let mut addr = addr.as_ptr().wrapping_add(
        pp.pr_align.get() as usize
            * (pp.pr_npagealloc.get() % u64::from(pp.pr_maxcolors.get())) as usize,
    );
    ph.ph_colored.set(addr);
    ph.ph_nmissing.set(0);
    let mut magic = [0u8; size_of::<usize>()];
    arc4random_buf(&mut magic);
    ph.ph_magic.set(usize::from_ne_bytes(magic));
    #[cfg(feature = "diagnostic")]
    {
        // use a bit in ph_magic to record if we poison page items
        if POOL_DEBUG.load(Ordering::Relaxed) != 0 {
            ph.ph_magic.set(ph.ph_magic.get() | POOL_MAGICBIT);
        } else {
            ph.ph_magic.set(ph.ph_magic.get() & !POOL_MAGICBIT);
        }
    }

    let mut n = pp.pr_itemsperpage.get();
    let mut o = 32;
    let mut order = 0u32;
    while n > 0 {
        n -= 1;
        let pi = addr.cast::<PoolItem>();
        // SAFETY: `pi` is the first words of an item inside the page; any bit pattern is a
        // valid pool_item.
        let pi_ref = unsafe { &*pi };
        pi_ref.pi_magic.set(pool_imagic(ph, pi));

        if o == 32 {
            order = arc4random();
            o = 0;
        }
        let bit = order & (1u32 << o) != 0;
        o += 1;
        // SAFETY: the item is on no list yet.
        unsafe {
            if bit {
                ph.ph_items.insert_tail(pi_ref);
            } else {
                ph.ph_items.insert_head(pi_ref);
            }
        }

        #[cfg(feature = "diagnostic")]
        if pool_phpoison(ph) {
            let _ = unported!("poison_mem (subr_poison.c) in pool_p_alloc");
        }

        addr = addr.wrapping_add(pp.pr_size.get() as usize);
    }

    Some(ph)
}

/// `pool_p_free`: returns an empty page to the allocator.
pub fn pool_p_free(pp: &Pool, ph: &PoolPageHeader) {
    pl_assert_unlocked(pp, &pp.pr_lock);
    kassert!(ph.ph_nmissing.get() == 0);

    for pi in ph.ph_items.iter() {
        if pi.pi_magic.get() != pool_imagic(ph, pi) {
            #[allow(clippy::panic)] // the C panics here too
            {
                panic!(
                    "pool_p_free: {} free list modified: page {:p}; item addr {:p}; offset 0x0={:#x}",
                    pp.pr_wchan.get(),
                    ph.ph_page.get(),
                    pi,
                    pi.pi_magic.get()
                );
            }
        }

        #[cfg(feature = "diagnostic")]
        if pool_phpoison(ph) {
            let _ = unported!("poison_check (subr_poison.c) in pool_p_free");
        }
    }

    let page = ph.ph_page.get();
    let inpghdr = pool_inpghdr(pp);
    let ph_ptr: *const PoolPageHeader = ph;

    if let Some(page) = NonNull::new(page) {
        pool_allocator_free(pp, page);
    }

    if !inpghdr && let Some(ph) = NonNull::new(ph_ptr.cast_mut().cast::<u8>()) {
        pool_put(&PHPOOL, ph);
    }
}

/// `pool_p_insert`: adds a fresh page to the pool.
pub fn pool_p_insert(pp: &Pool, ph: &PoolPageHeader) {
    pl_assert_locked(pp, &pp.pr_lock);

    // If the pool was depleted, point at the new page
    if pp.pr_curpage.get().is_null() {
        pp.pr_curpage.set(ph);
    }

    // SAFETY: a fresh header is on no list and in no tree.
    unsafe {
        pp.pr_emptypages.insert_tail(ph);
        if !pool_inpghdr(pp) {
            pp.pr_phtree.insert(ph);
        }
    }

    pp.pr_nitems
        .set(pp.pr_nitems.get() + pp.pr_itemsperpage.get());
    pp.pr_nidle.set(pp.pr_nidle.get() + 1);

    pp.pr_npagealloc.set(pp.pr_npagealloc.get() + 1);
    let npages = pp.pr_npages.get() + 1;
    pp.pr_npages.set(npages);
    if npages > pp.pr_hiwat.get() {
        pp.pr_hiwat.set(npages);
    }
}

/// `pool_p_remove`: takes an empty page out of the pool.
pub fn pool_p_remove(pp: &Pool, ph: &PoolPageHeader) {
    pl_assert_locked(pp, &pp.pr_lock);

    pp.pr_npagefree.set(pp.pr_npagefree.get() + 1);
    pp.pr_npages.set(pp.pr_npages.get() - 1);
    pp.pr_nidle.set(pp.pr_nidle.get() - 1);
    pp.pr_nitems
        .set(pp.pr_nitems.get() - pp.pr_itemsperpage.get());

    // SAFETY: the header is in the tree (off-page) and on the empty list.
    unsafe {
        if !pool_inpghdr(pp) {
            pp.pr_phtree.remove(ph);
        }
        pp.pr_emptypages.remove(ph);
    }

    pool_update_curpage(pp);
}

/// `pool_update_curpage`: the last partial page, else the last empty one.
pub fn pool_update_curpage(pp: &Pool) {
    let last = pp.pr_partpages.last().or_else(|| pp.pr_emptypages.last());
    pp.pr_curpage.set(last.map_or(ptr::null(), ptr::from_ref));
}

/// `pool_setlowat`: keeps at least `n` items allocated.
pub fn pool_setlowat(pp: &Pool, n: u32) {
    let mut prime = 0;

    pl_enter(pp, &pp.pr_lock);
    pp.pr_minitems.set(n);
    let itemsperpage = pp.pr_itemsperpage.get();
    pp.pr_minpages.set(if n == 0 {
        0
    } else {
        (roundup(n as usize, itemsperpage as usize) / itemsperpage as usize) as u32
    });

    if pp.pr_nitems.get() < n {
        prime = n - pp.pr_nitems.get();
    }
    pl_leave(pp, &pp.pr_lock);

    if prime > 0 {
        let _ = pool_prime(pp, prime);
    }
}

/// `pool_sethiwat`: keeps at most `n` idle items.
pub fn pool_sethiwat(pp: &Pool, n: u32) {
    let itemsperpage = pp.pr_itemsperpage.get();
    pp.pr_maxpages.set(if n == 0 {
        0
    } else {
        (roundup(n as usize, itemsperpage as usize) / itemsperpage as usize) as u32
    });
}

/// `pool_sethardlimit`: caps the items out at `n`.
pub fn pool_sethardlimit(pp: &Pool, n: u32) -> Result<(), Errno> {
    pl_enter(pp, &pp.pr_lock);

    let r = if n < pp.pr_nout.get() {
        Err(Errno::EINVAL)
    } else {
        pp.pr_hardlimit.set(n);
        Ok(())
    };
    pl_leave(pp, &pp.pr_lock);

    r
}

/// `pool_set_constraints`: where the pool's pages may come from.
pub fn pool_set_constraints(pp: &Pool, mode: &'static KmemPaMode) {
    pp.pr_crange.set(Some(mode));
}

/// `pool_reclaim`: release all complete pages that have not been used recently. Returns
/// true if any pages have been reclaimed.
pub fn pool_reclaim(pp: &Pool) -> bool {
    let pl: TailqHead<PhEntry> = TailqHead::new();
    pl.init();

    pl_enter(pp, &pp.pr_lock);
    let mut ph = pp.pr_emptypages.first();
    while let Some(cur) = ph {
        let phnext = TailqHead::<PhEntry>::next(cur);

        // Check our minimum page claim
        if pp.pr_npages.get() <= pp.pr_minpages.get() {
            break;
        }

        // If freeing this page would put us below the low water mark, stop now.
        if pp.pr_nitems.get() - pp.pr_itemsperpage.get() < pp.pr_minitems.get() {
            break;
        }

        let cur: *const PoolPageHeader = cur;
        // SAFETY: the header is on the empty list, so its page is allocated.
        let cur = unsafe { ph_ref(cur) };
        pool_p_remove(pp, cur);
        // SAFETY: just taken off the pool's lists.
        unsafe { pl.insert_tail(cur) };
        ph = phnext;
    }
    pl_leave(pp, &pp.pr_lock);

    if pl.is_empty() {
        return false;
    }

    while let Some(ph) = pl.first() {
        let ph: *const PoolPageHeader = ph;
        // SAFETY: the head of `pl`, moved there above.
        let ph = unsafe { ph_ref(ph) };
        // SAFETY: as above.
        unsafe { pl.remove(ph) };
        pool_p_free(pp, ph);
    }

    true
}

/// `pool_reclaim_all`: release all complete pages that have not been used recently from all
/// pools.
pub fn pool_reclaim_all() {
    rw_enter_read(&POOL_LOCK);
    for pp in POOL_HEAD.0.iter() {
        pool_reclaim(pp);
    }
    rw_exit_read(&POOL_LOCK);
}

/// `pool_count`: how many pools exist.
pub fn pool_count() -> u32 {
    POOL_COUNT.load(Ordering::Relaxed)
}

/// The printer `pool_walk` hands its `func` (`db_printf`'s type).
pub type PoolWalkPr = fn(core::fmt::Arguments<'_>) -> usize;

/// What `pool_walk` calls for every item in use: the item, `full`, the printer.
pub type PoolWalkFn = fn(*const u8, bool, PoolWalkPr);

/// `pool_walk(pp, full, pr, func)` (`DDB`): calls `func(item, full, pr)` for every item of the
/// pool that is in use (`db_show_all_nfsreqs`, `db_show_all_nfsnodes`): the items of the full
/// pages, then the items of the partial pages that are not on the page's free list. `pr` is
/// the printer (`db_printf`) `func` writes with.
pub fn pool_walk(pp: &Pool, full: bool, pr: PoolWalkPr, func: PoolWalkFn) {
    let size = pp.pr_size.get() as usize;
    for ph in pp.pr_fullpages.iter() {
        let mut cp = ph.ph_colored.get().cast_const();
        for _ in 0..ph.ph_nmissing.get() {
            func(cp, full, pr);
            cp = cp.wrapping_add(size);
        }
    }

    for ph in pp.pr_partpages.iter() {
        let mut cp = ph.ph_colored.get().cast_const();
        let mut n = ph.ph_nmissing.get();

        while n > 0 {
            let free = ph
                .ph_items
                .iter()
                .any(|pi| ptr::eq(cp, ptr::from_ref(pi).cast()));
            if !free {
                func(cp, full, pr);
                n -= 1;
            }

            cp = cp.wrapping_add(size);
        }
    }
}

/// `sysctl_dopool`: the `kern.pool` sysctls. `kern.pool.npools` is the number of pools;
/// `kern.pool.pool.<serial>` the `kinfo_pool` of the pool with that serial number and
/// `kern.pool.name.<serial>` its name. `kern.pool.cache` and `kern.pool.cache_cpus` are
/// `EOPNOTSUPP` without `MULTIPROCESSOR`, as in C.
pub fn sysctl_dopool(name: &[i32], oldp: usize, oldlenp: &mut usize) -> Result<(), Errno> {
    let Some(&what) = name.first() else {
        return Err(Errno::EOPNOTSUPP);
    };
    match what {
        KERN_POOL_NPOOLS => {
            if name.len() != 1 {
                return Err(Errno::ENOTDIR);
            }
            return sysctl_rdint(oldp, oldlenp, 0, pool_count() as i32);
        }
        KERN_POOL_NAME | KERN_POOL_POOL | KERN_POOL_CACHE | KERN_POOL_CACHE_CPUS => {}
        _ => return Err(Errno::EOPNOTSUPP),
    }

    if name.len() != 2 {
        return Err(Errno::ENOTDIR);
    }

    rw_enter_read(&POOL_LOCK);
    let found = POOL_HEAD
        .0
        .iter()
        .find(|pp| name[1] >= 0 && pp.pr_serial.get() == name[1] as u32);
    if let Some(pp) = found {
        refcnt_take(&pp.pr_refcnt);
    }
    rw_exit_read(&POOL_LOCK);

    let Some(pp) = found else {
        return Err(Errno::ENOENT);
    };

    let rv = match what {
        KERN_POOL_NAME => sysctl_rdstring(oldp, oldlenp, 0, pp.pr_wchan.get().as_bytes()),
        KERN_POOL_POOL => {
            pl_enter(pp, &pp.pr_lock);
            let mut pi = KinfoPool {
                pr_size: pp.pr_size.get(),
                pr_pgsize: pp.pr_pgsize.get(),
                pr_itemsperpage: pp.pr_itemsperpage.get(),
                pr_npages: pp.pr_npages.get(),
                pr_minpages: pp.pr_minpages.get(),
                pr_maxpages: pp.pr_maxpages.get(),
                pr_hardlimit: pp.pr_hardlimit.get(),
                pr_nout: pp.pr_nout.get(),
                pr_nitems: pp.pr_nitems.get(),
                pr_nget: pp.pr_nget.get(),
                pr_nput: pp.pr_nput.get(),
                pr_nfail: pp.pr_nfail.get(),
                pr_npagealloc: pp.pr_npagealloc.get(),
                pr_npagefree: pp.pr_npagefree.get(),
                pr_hiwat: pp.pr_hiwat.get(),
                pr_nidle: pp.pr_nidle.get(),
            };
            pl_leave(pp, &pp.pr_lock);

            pool_cache_pool_info(pp, &mut pi);

            sysctl_rdstruct(oldp, oldlenp, 0, &pi.to_bytes())
        }
        KERN_POOL_CACHE => pool_cache_info(pp, oldp, oldlenp),
        _ => pool_cache_cpus_info(pp, oldp, oldlenp),
    };

    refcnt_rele_wake(&pp.pr_refcnt);

    rv
}

/// `pool_gc_sched`: the gc timeout hands the work to `systqmp`.
pub fn pool_gc_sched(_null: *mut core::ffi::c_void) {
    task_add(SYSTQMP, &POOL_GC_TASK);
}

/// `pool_gc_pages`: frees, in every pool, one idle page that has been idle for
/// `POOL_WAIT_GC`, gives the per-CPU caches' idle lists back (`MULTIPROCESSOR`), and runs again
/// in a second. `init_main` starts it with `MULTIPROCESSOR`.
pub fn pool_gc_pages(_null: *mut core::ffi::c_void) {
    rw_enter_read(&POOL_LOCK);
    let s = splvm(); // XXX go to splvm until all pools _setipl properly
    for pp in POOL_HEAD.0.iter() {
        #[cfg(feature = "multiprocessor")]
        if !pp.pr_cache.load(Ordering::Acquire).is_null() {
            pool_cache_gc(pp);
        }

        if pp.pr_nidle.get() <= u64::from(pp.pr_minpages.get()) // guess
            || !pl_enter_try(pp, &pp.pr_lock)
        // try
        {
            continue;
        }

        // is it time to free a page?
        let mut freeph: Option<&PoolPageHeader> = None;
        if pp.pr_nidle.get() > u64::from(pp.pr_minpages.get())
            && let Some(ph) = pp.pr_emptypages.first()
            && getnsecuptime().wrapping_sub(ph.ph_timestamp.get()) > POOL_WAIT_GC
        {
            // SAFETY: the header is on the empty list, so its page is allocated.
            let ph = unsafe { ph_ref(ph) };
            freeph = Some(ph);
            pool_p_remove(pp, ph);
        }

        pl_leave(pp, &pp.pr_lock);

        if let Some(ph) = freeph {
            pool_p_free(pp, ph);
        }
    }
    splx(s);
    rw_exit_read(&POOL_LOCK);

    timeout_add_sec(&POOL_GC_TICK, 1);
}

// Pool backend allocators.

/// `pool_allocator_alloc`: a page from the pool's allocator.
pub fn pool_allocator_alloc(pp: &Pool, flags: i32, slowdown: &mut i32) -> Option<NonNull<u8>> {
    let pa = pp.pr_alloc.get()?;
    let v = (pa.pa_alloc)(pp, flags, slowdown);

    #[cfg(feature = "diagnostic")]
    if let Some(v) = v
        && pool_inpghdr(pp)
    {
        let addr = v.as_ptr() as usize;
        if addr & pp.pr_pgmask.get() != addr {
            #[allow(clippy::panic)] // the C panics here too
            {
                panic!(
                    "pool_allocator_alloc: {} page address {:p} isn't aligned to {}",
                    pp.pr_wchan.get(),
                    v,
                    pp.pr_pgsize.get()
                );
            }
        }
    }

    v
}

/// `pool_allocator_free`: a page back to the pool's allocator.
pub fn pool_allocator_free(pp: &Pool, v: NonNull<u8>) {
    if let Some(pa) = pp.pr_alloc.get() {
        (pa.pa_free)(pp, v);
    }
}

/// `pool_page_alloc`: the default allocator, one page through `kv_page`.
pub fn pool_page_alloc(pp: &Pool, flags: i32, _slowdown: &mut i32) -> Option<NonNull<u8>> {
    // kd_slowdown: the single-page thread is not used with __HAVE_PMAP_DIRECT.
    let kd = crate::uvm::uvm_extern::KmemDynMode {
        kd_waitok: flags & PR_WAITOK != 0,
        ..KMEM_DYN_INITIALIZER
    };

    km_alloc(
        pp.pr_pgsize.get() as usize,
        &KV_PAGE,
        pp.pr_crange.get()?,
        &kd,
    )
}

/// `pool_page_free`.
pub fn pool_page_free(pp: &Pool, v: NonNull<u8>) {
    if let Some(crange) = pp.pr_crange.get() {
        km_free(v, pp.pr_pgsize.get() as usize, &KV_PAGE, crange);
    }
}

/// `pool_multi_alloc`: pages of the pool's size from `kmem_map`, interrupt safe.
pub fn pool_multi_alloc(pp: &Pool, flags: i32, _slowdown: &mut i32) -> Option<NonNull<u8>> {
    let mut kv: KmemVaMode = KV_INTRSAFE;
    let kd = crate::uvm::uvm_extern::KmemDynMode {
        kd_waitok: flags & PR_WAITOK != 0,
        ..KMEM_DYN_INITIALIZER
    };

    if pool_inpghdr(pp) {
        kv.kv_align = pp.pr_pgsize.get() as usize;
    }

    let crange = pp.pr_crange.get()?;
    let s = splvm();
    let v = km_alloc(pp.pr_pgsize.get() as usize, &kv, crange, &kd);
    splx(s);

    v
}

/// `pool_multi_free`.
pub fn pool_multi_free(pp: &Pool, v: NonNull<u8>) {
    let mut kv: KmemVaMode = KV_INTRSAFE;

    if pool_inpghdr(pp) {
        kv.kv_align = pp.pr_pgsize.get() as usize;
    }

    if let Some(crange) = pp.pr_crange.get() {
        let s = splvm();
        km_free(v, pp.pr_pgsize.get() as usize, &kv, crange);
        splx(s);
    }
}

/// `pool_multi_alloc_ni`: pages of the pool's size from `kernel_map`; may sleep.
pub fn pool_multi_alloc_ni(pp: &Pool, flags: i32, _slowdown: &mut i32) -> Option<NonNull<u8>> {
    let mut kv: KmemVaMode = KV_ANY;
    let kd = crate::uvm::uvm_extern::KmemDynMode {
        kd_waitok: flags & PR_WAITOK != 0,
        ..KMEM_DYN_INITIALIZER
    };

    if pool_inpghdr(pp) {
        kv.kv_align = pp.pr_pgsize.get() as usize;
    }

    let crange = pp.pr_crange.get()?;
    kernel_lock();
    let v = km_alloc(pp.pr_pgsize.get() as usize, &kv, crange, &kd);
    kernel_unlock();

    v
}

/// `pool_multi_free_ni`.
pub fn pool_multi_free_ni(pp: &Pool, v: NonNull<u8>) {
    let mut kv: KmemVaMode = KV_ANY;

    if pool_inpghdr(pp) {
        kv.kv_align = pp.pr_pgsize.get() as usize;
    }

    if let Some(crange) = pp.pr_crange.get() {
        kernel_lock();
        km_free(v, pp.pr_pgsize.get() as usize, &kv, crange);
        kernel_unlock();
    }
}

// The per-CPU caches (MULTIPROCESSOR).

/// The pool's per-CPU caches, `None` when it has none.
#[cfg(feature = "multiprocessor")]
fn pr_cache(pp: &Pool) -> Option<CpumemPtr> {
    let cm = NonNull::new(pp.pr_cache.load(Ordering::Acquire))?;
    // SAFETY: `pool_cache_init` published a `cpumem_get(&pool_caches)` array, whose items are
    // `pool_caches`' size; it lives until `pool_cache_destroy` takes it away.
    Some(unsafe { CpumemPtr::from_raw(cm, POOL_CACHES.pr_size.get() as usize) })
}

/// The `struct pool_cache` at one CPU's memory of `pr_cache`.
#[cfg(feature = "multiprocessor")]
fn pool_cache_at(mem: NonNull<u8>) -> &'static PoolCache {
    // SAFETY: an item of `pool_caches` (`size_of::<PoolCache>()` bytes, cache-line aligned),
    // zeroed by `cpumem_get`; every bit pattern of zero is a valid `PoolCache` and the fields
    // are cells and atomics.
    unsafe { &*mem.as_ptr().cast::<PoolCache>() }
}

/// The cache item at `ci`, a free item of the pool.
///
/// # Safety
///
/// `ci` must be a free item of a pool whose items are at least a `pool_cache_item`, aligned.
#[cfg(feature = "multiprocessor")]
unsafe fn ci_ref<'a>(ci: *mut PoolCacheItem) -> &'a PoolCacheItem {
    // SAFETY: the caller's guarantee; every bit pattern is a valid item (cells of words).
    unsafe { &*ci }
}

/// `pool_cache_init`: gives the pool a cache per CPU (`pool_caches` items through
/// `cpumem_get`); `pool_get`/`pool_put` use them from then on.
#[cfg(feature = "multiprocessor")]
pub fn pool_cache_init(pp: &Pool) {
    if POOL_CACHES.pr_size.get() == 0 {
        pool_init(
            &POOL_CACHES,
            size_of::<PoolCache>(),
            CACHELINESIZE as u32,
            IPL_NONE,
            PR_WAITOK | PR_RWLOCK,
            "plcache",
            None,
        );
    }

    // must be able to use the pool items as cache list items
    kassert!(pp.pr_size.get() as usize >= size_of::<PoolCacheItem>());

    let cm = cpumem_get(&POOL_CACHES);

    pl_init(pp, &pp.pr_cache_lock);
    let mut magic = [0u8; 2 * size_of::<usize>()];
    arc4random_buf(&mut magic);
    let (words, _) = magic.as_chunks::<{ size_of::<usize>() }>();
    for (m, w) in pp.pr_cache_magic.iter().zip(words) {
        m.set(usize::from_ne_bytes(*w));
    }
    pp.pr_cache_lists.init();
    pp.pr_cache_nitems.set(0);
    pp.pr_cache_timestamp
        .store(getnsecuptime(), Ordering::Relaxed);
    pp.pr_cache_items
        .store(POOL_CACHE_LIST_MIN, Ordering::Relaxed);
    pp.pr_cache_contention.store(0, Ordering::Relaxed);
    pp.pr_cache_ngc.set(0);

    for mem in cpumem_foreach(cm) {
        let pc = pool_cache_at(mem);
        pc.pc_actv.set(ptr::null_mut());
        pc.pc_nactv.set(0);
        pc.pc_prev.set(ptr::null_mut());

        pc.pc_nget.store(0, Ordering::Relaxed);
        pc.pc_nfail.store(0, Ordering::Relaxed);
        pc.pc_nput.store(0, Ordering::Relaxed);
        pc.pc_nlget.store(0, Ordering::Relaxed);
        pc.pc_nlfail.store(0, Ordering::Relaxed);
        pc.pc_nlput.store(0, Ordering::Relaxed);
        pc.pc_nout.store(0, Ordering::Relaxed);
    }

    fence(Ordering::Release); // membar_producer()

    pp.pr_cache.store(cm.as_ptr().as_ptr(), Ordering::Release);
}

/// The two words of `ci_nextl` that hold a cached item's magic.
#[cfg(feature = "multiprocessor")]
fn ci_magic_words(ci: &PoolCacheItem) -> &[Cell<usize>; 2] {
    // SAFETY: `ci_nextl` is a `#[repr(C)]` TAILQ_ENTRY of two pointer-sized cells; any value is
    // a valid `usize`, and an item in a CPU's list is on no tail queue.
    unsafe { &*ptr::from_ref(&ci.ci_nextl).cast::<[Cell<usize>; 2]>() }
}

/// `pool_cache_item_magic`: marks a cached item's link words.
#[cfg(feature = "multiprocessor")]
fn pool_cache_item_magic(pp: &Pool, ci: &PoolCacheItem) {
    let entry = ci_magic_words(ci);

    entry[0].set(pp.pr_cache_magic[0].get() ^ ptr::from_ref(ci) as usize);
    entry[1].set(pp.pr_cache_magic[1].get() ^ ci.ci_next.get() as usize);
}

/// `pool_cache_item_magic_check`: panics when a cached item's link words were modified.
#[cfg(feature = "multiprocessor")]
fn pool_cache_item_magic_check(pp: &Pool, ci: &PoolCacheItem) {
    let entry = ci_magic_words(ci);
    let vals = [
        pp.pr_cache_magic[0].get() ^ ptr::from_ref(ci) as usize,
        pp.pr_cache_magic[1].get() ^ ci.ci_next.get() as usize,
    ];

    for (i, val) in vals.into_iter().enumerate() {
        if entry[i].get() != val {
            let off = ptr::from_ref(&entry[i]) as usize - ptr::from_ref(ci) as usize;
            crate::kern::subr_prf::panic(format_args!(
                "pool_cache_item_magic_check: {} cpu free list modified: item addr {:p}+{} {:#x}!={:#x}",
                pp.pr_wchan.get(),
                ci,
                off,
                entry[i].get(),
                val
            ));
        }
    }
}

/// `pool_list_enter`: takes the idle list lock, counting contention.
#[cfg(feature = "multiprocessor")]
fn pool_list_enter(pp: &Pool) {
    if !pl_enter_try(pp, &pp.pr_cache_lock) {
        pl_enter(pp, &pp.pr_cache_lock);
        pp.pr_cache_contention.fetch_add(1, Ordering::Relaxed);
    }
}

/// `pool_list_leave`.
#[cfg(feature = "multiprocessor")]
fn pool_list_leave(pp: &Pool) {
    pl_leave(pp, &pp.pr_cache_lock);
}

/// One more in a counter only its CPU writes.
#[cfg(feature = "multiprocessor")]
fn pc_inc(c: &AtomicU64) {
    c.store(c.load(Ordering::Relaxed) + 1, Ordering::Relaxed);
}

/// `pool_cache_list_alloc`: an idle list from the pool for `pc`, or null.
#[cfg(feature = "multiprocessor")]
fn pool_cache_list_alloc(pp: &Pool, pc: &PoolCache) -> *mut PoolCacheItem {
    pool_list_enter(pp);
    let pl = pp
        .pr_cache_lists
        .first()
        .map_or(ptr::null_mut(), |pl| ptr::from_ref(pl).cast_mut());
    // SAFETY: a list on `pr_cache_lists` is a free item of the pool.
    if let Some(l) = (!pl.is_null()).then(|| unsafe { ci_ref(pl) }) {
        // SAFETY: `l` is on the pool's list of lists, which the list lock guards.
        unsafe { pp.pr_cache_lists.remove(l) };
        pp.pr_cache_nitems
            .set(pp.pr_cache_nitems.get() - pool_cache_item_nitems(l) as u32);

        pool_cache_item_magic(pp, l);

        pc_inc(&pc.pc_nlget);
    } else {
        pc_inc(&pc.pc_nlfail);
    }

    // fold this cpus nout into the global while we have the lock
    pp.pr_cache_nout
        .set(pp.pr_cache_nout.get() + pc.pc_nout.load(Ordering::Relaxed));
    pc.pc_nout.store(0, Ordering::Relaxed);
    pool_list_leave(pp);

    pl
}

/// `pool_cache_list_free`: gives a full list of `pc` to the pool's idle lists.
#[cfg(feature = "multiprocessor")]
fn pool_cache_list_free(pp: &Pool, pc: &PoolCache, ci: &PoolCacheItem) {
    pool_list_enter(pp);
    if pp.pr_cache_lists.is_empty() {
        pp.pr_cache_timestamp
            .store(getnsecuptime(), Ordering::Relaxed);
    }

    pp.pr_cache_nitems
        .set(pp.pr_cache_nitems.get() + pool_cache_item_nitems(ci) as u32);
    // SAFETY: a CPU's list is on no tail queue; the list lock guards the queue.
    unsafe { pp.pr_cache_lists.insert_tail(ci) };

    pc_inc(&pc.pc_nlput);

    // fold this cpus nout into the global while we have the lock
    pp.pr_cache_nout
        .set(pp.pr_cache_nout.get() + pc.pc_nout.load(Ordering::Relaxed));
    pc.pc_nout.store(0, Ordering::Relaxed);
    pool_list_leave(pp);
}

/// `pool_cache_enter`: this CPU's cache at `pr_ipl`, its generation number made odd.
#[cfg(feature = "multiprocessor")]
fn pool_cache_enter(pp: &Pool, cm: CpumemPtr) -> (&'static PoolCache, i32) {
    let pc = pool_cache_at(cpumem_enter(cm));
    let s = splraise(pp.pr_ipl.get());
    pc.pc_gen
        .store(pc.pc_gen.load(Ordering::Relaxed) + 1, Ordering::Relaxed);
    fence(Ordering::Release);

    (pc, s)
}

/// `pool_cache_leave`: the generation number even again, the level back.
#[cfg(feature = "multiprocessor")]
fn pool_cache_leave(cm: CpumemPtr, pc: &PoolCache, s: i32) {
    pc.pc_gen
        .store(pc.pc_gen.load(Ordering::Relaxed) + 1, Ordering::Release);
    splx(s);
    cpumem_leave(cm, NonNull::from(pc).cast());
}

/// `pool_cache_get`: an item from this CPU's cache, else a list from the pool's idle lists;
/// `None` sends `pool_get` to the pages.
#[cfg(feature = "multiprocessor")]
pub fn pool_cache_get(pp: &Pool) -> Option<NonNull<u8>> {
    let cm = pr_cache(pp)?;
    let (pc, s) = pool_cache_enter(pp, cm);

    let ci = if !pc.pc_actv.get().is_null() {
        pc.pc_actv.get()
    } else if !pc.pc_prev.get().is_null() {
        let ci = pc.pc_prev.get();
        pc.pc_prev.set(ptr::null_mut());
        ci
    } else {
        let ci = pool_cache_list_alloc(pp, pc);
        if ci.is_null() {
            pc_inc(&pc.pc_nfail);
            pool_cache_leave(cm, pc, s);
            return None;
        }
        ci
    };
    // SAFETY: the lists of a cache hold free items of this pool.
    let ci = unsafe { ci_ref(ci) };

    pool_cache_item_magic_check(pp, ci);
    #[cfg(feature = "diagnostic")]
    if POOL_DEBUG.load(Ordering::Relaxed) != 0 && pool_cache_item_poisoned(ci) {
        let _ = unported!("poison_check (subr_poison.c) in pool_cache_get");
    }

    pc.pc_actv.set(ci.ci_next.get());
    pc.pc_nactv.set(pool_cache_item_nitems(ci) - 1);
    pc_inc(&pc.pc_nget);
    pc.pc_nout
        .store(pc.pc_nout.load(Ordering::Relaxed) + 1, Ordering::Relaxed);

    pool_cache_leave(cm, pc, s);

    Some(NonNull::from(ci).cast())
}

/// `pool_cache_put`: an item onto this CPU's active list; a full active list becomes the
/// previous one, and a previous one goes to the pool's idle lists.
#[cfg(feature = "multiprocessor")]
pub fn pool_cache_put(pp: &Pool, v: NonNull<u8>) {
    let Some(cm) = pr_cache(pp) else {
        return;
    };
    // SAFETY: `v` is an item of this pool the caller gives back, at least a pool_cache_item
    // (`pool_cache_init`'s KASSERT) and aligned.
    let ci = unsafe { ci_ref(v.as_ptr().cast::<PoolCacheItem>()) };
    #[cfg(feature = "diagnostic")]
    let poison = POOL_DEBUG.load(Ordering::Relaxed) != 0
        && pp.pr_size.get() as usize > size_of::<PoolCacheItem>();
    #[cfg(feature = "diagnostic")]
    if poison {
        let _ = unported!("poison_mem (subr_poison.c) in pool_cache_put");
    }

    let (pc, s) = pool_cache_enter(pp, cm);

    let mut nitems = pc.pc_nactv.get();
    if nitems >= pp.pr_cache_items.load(Ordering::Relaxed) as usize {
        if !pc.pc_prev.get().is_null() {
            // SAFETY: the previous list is free items of this pool.
            pool_cache_list_free(pp, pc, unsafe { ci_ref(pc.pc_prev.get()) });
        }

        pc.pc_prev.set(pc.pc_actv.get());

        pc.pc_actv.set(ptr::null_mut());
        pc.pc_nactv.set(0);
        nitems = 0;
    }

    ci.ci_next.set(pc.pc_actv.get());
    nitems += 1;
    ci.ci_nitems.set(nitems);
    #[cfg(feature = "diagnostic")]
    if poison {
        ci.ci_nitems
            .set(ci.ci_nitems.get() | POOL_CACHE_ITEM_NITEMS_POISON);
    }
    pool_cache_item_magic(pp, ci);

    pc.pc_actv.set(ptr::from_ref(ci).cast_mut());
    pc.pc_nactv.set(nitems);

    pc_inc(&pc.pc_nput);
    pc.pc_nout
        .store(pc.pc_nout.load(Ordering::Relaxed) - 1, Ordering::Relaxed);

    pool_cache_leave(cm, pc, s);
}

/// `pool_cache_list_put`: every item of the list `pl` back onto its page. Returns what was
/// `pl`'s `TAILQ_NEXT` (meaningful only for a list taken off `pr_cache_lists`).
#[cfg(feature = "multiprocessor")]
fn pool_cache_list_put(pp: &Pool, pl: *mut PoolCacheItem) -> *mut PoolCacheItem {
    if pl.is_null() {
        return ptr::null_mut();
    }

    // SAFETY: a list is free items of this pool.
    let rpl = ci_magic_words(unsafe { ci_ref(pl) })[0].get() as *mut PoolCacheItem;

    let mut pl = pl;
    pl_enter(pp, &pp.pr_lock);
    loop {
        // SAFETY: as above.
        let next = unsafe { ci_ref(pl) }.ci_next.get();
        if let Some(v) = NonNull::new(pl.cast::<u8>()) {
            pool_do_put(pp, v);
        }
        pl = next;
        if pl.is_null() {
            break;
        }
    }
    pl_leave(pp, &pp.pr_lock);

    rpl
}

/// `pool_cache_destroy`: every cached item back onto its page and the caches freed; the pool
/// works without them from then on. Nobody may be using the caches.
#[cfg(feature = "multiprocessor")]
pub fn pool_cache_destroy(pp: &Pool) {
    rw_enter_write(&POOL_LOCK); // serialise with the gc
    let cm = pr_cache(pp);
    pp.pr_cache.store(ptr::null_mut(), Ordering::Release); // make pool_put avoid the cache
    rw_exit_write(&POOL_LOCK);

    let Some(cm) = cm else {
        return;
    };

    for mem in cpumem_foreach(cm) {
        let pc = pool_cache_at(mem);
        pool_cache_list_put(pp, pc.pc_actv.get());
        pool_cache_list_put(pp, pc.pc_prev.get());
    }

    // SAFETY: `pr_cache` no longer names `cm`, and no CPU is using its cache (the caller's
    // contract).
    unsafe { crate::kern::subr_percpu::cpumem_put(&POOL_CACHES, cm) };

    let mut pl = pp
        .pr_cache_lists
        .first()
        .map_or(ptr::null_mut(), |pl| ptr::from_ref(pl).cast_mut());
    while !pl.is_null() {
        pl = pool_cache_list_put(pp, pl);
    }
}

/// `pool_cache_gc`: gives one idle list that has been idle for `POOL_WAIT_GC` back to the
/// pages, and adapts the list length to the contention on the list lock.
#[cfg(feature = "multiprocessor")]
pub fn pool_cache_gc(pp: &Pool) {
    if getnsecuptime().wrapping_sub(pp.pr_cache_timestamp.load(Ordering::Relaxed)) > POOL_WAIT_GC
        && !pp.pr_cache_lists.is_empty()
        && pl_enter_try(pp, &pp.pr_cache_lock)
    {
        let mut pl: *mut PoolCacheItem = ptr::null_mut();

        if let Some(l) = pp.pr_cache_lists.first() {
            // SAFETY: `l` is on the list of lists, which the lock guards.
            unsafe { pp.pr_cache_lists.remove(l) };
            pp.pr_cache_nitems
                .set(pp.pr_cache_nitems.get() - pool_cache_item_nitems(l) as u32);
            pp.pr_cache_timestamp
                .store(getnsecuptime(), Ordering::Relaxed);

            pp.pr_cache_ngc.set(pp.pr_cache_ngc.get() + 1);
            pl = ptr::from_ref(l).cast_mut();
        }

        pl_leave(pp, &pp.pr_cache_lock);

        pool_cache_list_put(pp, pl);
    }

    // if there's a lot of contention on the pr_cache_mtx then consider growing the length of
    // the list to reduce the need to access the global pool.

    let contention = pp.pr_cache_contention.load(Ordering::Relaxed);
    let delta = contention.wrapping_sub(pp.pr_cache_contention_prev.get());
    if delta > 8 {
        // magic
        if (ncpusfound() as u32 * POOL_CACHE_LIST_MIN * 2) <= pp.pr_cache_nitems.get() {
            pp.pr_cache_items
                .fetch_add(POOL_CACHE_LIST_INC, Ordering::Relaxed);
        }
    } else if delta == 0 && pp.pr_cache_items.load(Ordering::Relaxed) > POOL_CACHE_LIST_MIN {
        pp.pr_cache_items
            .fetch_sub(POOL_CACHE_LIST_DEC, Ordering::Relaxed);
    }
    pp.pr_cache_contention_prev.set(contention);
}

/// `pool_cache_pool_info`: adds the per-CPU caches' gets, puts and items out to `pi`.
#[cfg(feature = "multiprocessor")]
pub fn pool_cache_pool_info(pp: &Pool, pi: &mut KinfoPool) {
    let Some(cm) = pr_cache(pp) else {
        return;
    };

    // loop through the caches twice to collect stats

    // once without the lock so we can yield while reading nget/nput
    for mem in cpumem_foreach(cm) {
        let pc = pool_cache_at(mem);
        let (mut nget, mut nput);

        loop {
            let mut r#gen = pc.pc_gen.load(Ordering::Acquire);
            while r#gen & 1 != 0 {
                crate::kern::sched_bsd::r#yield();
                r#gen = pc.pc_gen.load(Ordering::Acquire);
            }

            nget = pc.pc_nget.load(Ordering::Relaxed);
            nput = pc.pc_nput.load(Ordering::Relaxed);
            fence(Ordering::Acquire);
            if r#gen == pc.pc_gen.load(Ordering::Relaxed) {
                break;
            }
        }

        pi.pr_nget += nget;
        pi.pr_nput += nput;
    }

    // and once with the mtx so we can get consistent nout values
    pl_enter(pp, &pp.pr_cache_lock);
    for mem in cpumem_foreach(cm) {
        let pc = pool_cache_at(mem);
        pi.pr_nout = pi
            .pr_nout
            .wrapping_add_signed(pc.pc_nout.load(Ordering::Relaxed));
    }

    pi.pr_nout = pi.pr_nout.wrapping_add_signed(pp.pr_cache_nout.get());
    pl_leave(pp, &pp.pr_cache_lock);
}

/// `pool_cache_info`: `kern.pool.cache.<serial>`, the pool's `struct kinfo_pool_cache`.
#[cfg(feature = "multiprocessor")]
pub fn pool_cache_info(pp: &Pool, oldp: usize, oldlenp: &mut usize) -> Result<(), Errno> {
    if pr_cache(pp).is_none() {
        return Err(Errno::EOPNOTSUPP);
    }

    pl_enter(pp, &pp.pr_cache_lock);
    let kpc = KinfoPoolCache {
        pr_ngc: pp.pr_cache_ngc.get(),
        pr_len: pp.pr_cache_items.load(Ordering::Relaxed),
        pr_nitems: pp.pr_cache_nitems.get(),
        pr_contention: pp.pr_cache_contention.load(Ordering::Relaxed),
    };
    pl_leave(pp, &pp.pr_cache_lock);

    sysctl_rdstruct(oldp, oldlenp, 0, &kpc.to_bytes())
}

/// `pool_cache_cpus_info`: `kern.pool.cache_cpus.<serial>`, a `struct kinfo_pool_cache_cpu`
/// per CPU.
#[cfg(feature = "multiprocessor")]
pub fn pool_cache_cpus_info(pp: &Pool, oldp: usize, oldlenp: &mut usize) -> Result<(), Errno> {
    let Some(cm) = pr_cache(pp) else {
        return Err(Errno::EOPNOTSUPP);
    };
    if !(*oldlenp).is_multiple_of(KinfoPoolCacheCpu::SIZE) {
        return Err(Errno::EINVAL);
    }

    let ncpus = ncpusfound();
    let Some(kpcc) = mallocarray(
        ncpus,
        KinfoPoolCacheCpu::SIZE,
        M_TEMP,
        M_WAITOK | M_CANFAIL | M_ZERO,
    ) else {
        return Err(Errno::EIO);
    };

    let len = ncpus * KinfoPoolCacheCpu::SIZE;
    // SAFETY: `len` bytes just allocated, ours until the free below.
    let buf = unsafe { core::slice::from_raw_parts_mut(kpcc.as_ptr(), len) };

    let mut error = Ok(());
    for (cpu, mem) in cpumem_foreach(cm).enumerate() {
        if cpu >= ncpus {
            error = Err(Errno::EIO);
            break;
        }

        let pc = pool_cache_at(mem);
        let mut info;
        loop {
            let mut r#gen = pc.pc_gen.load(Ordering::Acquire);
            while r#gen & 1 != 0 {
                crate::kern::sched_bsd::r#yield();
                r#gen = pc.pc_gen.load(Ordering::Acquire);
            }

            info = KinfoPoolCacheCpu {
                pr_cpu: cpu as u32,
                pr_nget: pc.pc_nget.load(Ordering::Relaxed),
                pr_nfail: pc.pc_nfail.load(Ordering::Relaxed),
                pr_nput: pc.pc_nput.load(Ordering::Relaxed),
                pr_nlget: pc.pc_nlget.load(Ordering::Relaxed),
                pr_nlfail: pc.pc_nlfail.load(Ordering::Relaxed),
                pr_nlput: pc.pc_nlput.load(Ordering::Relaxed),
            };
            fence(Ordering::Acquire);
            if r#gen == pc.pc_gen.load(Ordering::Relaxed) {
                break;
            }
        }

        buf[cpu * KinfoPoolCacheCpu::SIZE..(cpu + 1) * KinfoPoolCacheCpu::SIZE]
            .copy_from_slice(&info.to_bytes());
    }

    if error.is_ok() {
        error = sysctl_rdstruct(oldp, oldlenp, 0, buf);
    }
    free(kpcc, M_TEMP, len);

    error
}

// The per-CPU caches without MULTIPROCESSOR.

/// `pool_cache_init`: nothing without `MULTIPROCESSOR`.
#[cfg(not(feature = "multiprocessor"))]
pub fn pool_cache_init(_pp: &Pool) {
    // nop
}

/// `pool_cache_pool_info`: adds the per-CPU caches' counts to `pi`; nothing to add without
/// `MULTIPROCESSOR`.
#[cfg(not(feature = "multiprocessor"))]
pub fn pool_cache_pool_info(_pp: &Pool, _pi: &mut KinfoPool) {
    // nop
}

/// `pool_cache_info`: `kern.pool.cache.<serial>`; there is no cache without
/// `MULTIPROCESSOR`.
#[cfg(not(feature = "multiprocessor"))]
pub fn pool_cache_info(_pp: &Pool, _oldp: usize, _oldlenp: &mut usize) -> Result<(), Errno> {
    Err(Errno::EOPNOTSUPP)
}

/// `pool_cache_cpus_info`: `kern.pool.cache_cpus.<serial>`; there is no cache without
/// `MULTIPROCESSOR`.
#[cfg(not(feature = "multiprocessor"))]
pub fn pool_cache_cpus_info(_pp: &Pool, _oldp: usize, _oldlenp: &mut usize) -> Result<(), Errno> {
    Err(Errno::EOPNOTSUPP)
}

// The lock operations.

fn pool_lock_mtx_init(pp: &Pool, lock: &PoolLock) {
    mtx_init_flags(&lock.prl_mtx, pp.pr_ipl.get(), Some(pp.pr_wchan.get()), 0);
}

fn pool_lock_mtx_enter(lock: &PoolLock) {
    mtx_enter(&lock.prl_mtx);
}

fn pool_lock_mtx_enter_try(lock: &PoolLock) -> bool {
    mtx_enter_try(&lock.prl_mtx)
}

fn pool_lock_mtx_leave(lock: &PoolLock) {
    mtx_leave(&lock.prl_mtx);
}

fn pool_lock_mtx_assert_locked(lock: &PoolLock) {
    mutex_assert_locked(&lock.prl_mtx, "pool_lock_mtx_assert_locked");
}

fn pool_lock_mtx_assert_unlocked(lock: &PoolLock) {
    mutex_assert_unlocked(&lock.prl_mtx, "pool_lock_mtx_assert_unlocked");
}

fn pool_lock_mtx_sleep(
    ident: *const (),
    lock: &PoolLock,
    priority: i32,
    wmesg: &'static str,
) -> Result<(), Errno> {
    msleep_nsec(ident, &lock.prl_mtx, priority, wmesg, INFSLP)
}

fn pool_lock_rw_init(pp: &Pool, lock: &PoolLock) {
    rw_init_flags(&lock.prl_rwlock, pp.pr_wchan.get(), 0);
}

fn pool_lock_rw_enter(lock: &PoolLock) {
    rw_enter_write(&lock.prl_rwlock);
}

fn pool_lock_rw_enter_try(lock: &PoolLock) -> bool {
    rw_enter(&lock.prl_rwlock, RW_WRITE | RW_NOSLEEP).is_ok()
}

fn pool_lock_rw_leave(lock: &PoolLock) {
    rw_exit_write(&lock.prl_rwlock);
}

fn pool_lock_rw_assert_locked(lock: &PoolLock) {
    rw_assert_wrlock(&lock.prl_rwlock);
}

fn pool_lock_rw_assert_unlocked(lock: &PoolLock) {
    kassert!(rw_status(&lock.prl_rwlock) != RW_WRITE);
}

fn pool_lock_rw_sleep(
    ident: *const (),
    lock: &PoolLock,
    priority: i32,
    wmesg: &'static str,
) -> Result<(), Errno> {
    rwsleep_nsec(ident, &lock.prl_rwlock, priority, wmesg, INFSLP)
}
/* </CODE> */

/* <TESTS> */
#[cfg(test)]
pub(crate) mod tests {
    // Host tests for pool(9) over real memory.
    //
    // The host double's "physical" pages are numbers and its direct map is the identity, so a
    // test loads a leaked block of its own memory as the physical segment: the page frames are the
    // block's addresses, and everything the allocators hand out is memory the test can touch.

    use std::sync::MutexGuard;
    use std::vec::Vec;
    use std::{assert, assert_eq, vec};

    use super::*;
    use crate::kern::kern_malloc::kmeminit;
    use crate::sys::param::PAGE_MASK;
    use crate::uvm::uvm_init::UVMEXP;
    use crate::uvm::uvm_page::{
        uvm_page_init, uvm_page_physload, uvm_page_test_reset, uvm_setpagesize,
    };
    use crate::uvm::uvm_param::atop;

    /// The block each test loads.
    const BLOCK_BYTES: usize = 32 << 20;

    /// Loads a fresh block of host memory as the physical memory, brings the page system up and
    /// `kmeminit`; holds the page system's lock for the test.
    pub(crate) fn setup_real_memory() -> MutexGuard<'static, ()> {
        let guard = crate::uvm::uvm_pmemrange::tests::LOCK
            .lock()
            .unwrap_or_else(|e| e.into_inner());
        uvm_page_test_reset();
        UVMEXP.pagesize.store(PAGE_SIZE as i32, Ordering::Relaxed);
        uvm_setpagesize();

        let block: &'static mut [u8] = vec![0u8; BLOCK_BYTES + PAGE_SIZE].leak();
        let base = (block.as_ptr() as usize + PAGE_MASK) & !PAGE_MASK;
        let (start, end) = (atop(base), atop(base + BLOCK_BYTES));
        uvm_page_physload(start, end, start, end, 0);
        // physmem, as the machine's bootstrap records it: kmeminit_nkmempages sizes kmem_map
        // and, under KMEMSTATS, every type's ks_limit from it.
        crate::sys::systm::PHYSMEM.store(end - start, Ordering::Relaxed);

        let (mut s, mut e) = (
            crate::sys::types::Vaddr::new(0),
            crate::sys::types::Vaddr::new(0),
        );
        uvm_page_init(&mut s, &mut e);
        kmeminit();
        #[cfg(feature = "kmemstats")]
        crate::kern::kern_malloc::kmemstats_test_reset();
        // The radix trees' globals (`rn_zeros`, the mask tree) point into the memory just replaced:
        // the next `rn_init` (`vfsinit`'s, `pfr_initialize`'s) starts over.
        crate::net::radix::rn_test_reset();
        // The same for the wsmux table, grown with malloc(9) by the tests that attach muxes.
        crate::dev::wscons::wsmux::wsmux_test_reset();
        // An attach a failed test left counted would make every later `config_detach` sleep.
        crate::kern::subr_autoconf::autoconf_test_reset();
        // And pf's OS fingerprints, whose entries came from pools other tests initialise again.
        crate::net::pf_osfp::pf_osfp_test_reset();
        guard
    }

    fn inside_a_page_of(pp: &Pool, p: NonNull<u8>) -> bool {
        let page = (p.as_ptr() as usize) & pp.pr_pgmask.get();
        let ph = pr_find_pagehead(pp, p.as_ptr());
        ph.ph_page.get() as usize == page || !pool_inpghdr(pp)
    }

    #[test]
    fn small_items_cycle_through_pages() {
        let _g = setup_real_memory();
        static P: Pool = Pool::new();
        pool_init(&P, 48, 0, IPL_HIGH, 0, "test48", None);
        assert!(
            pool_inpghdr(&P),
            "a 48-byte item pool keeps its header in the page"
        );
        assert_eq!(P.pr_size.get(), 48);
        let itemsperpage = P.pr_itemsperpage.get() as usize;
        assert!(itemsperpage > 60);

        let mut items: Vec<NonNull<u8>> = Vec::new();
        for i in 0..200u8 {
            let p = pool_get(&P, PR_NOWAIT).expect("an item");
            assert_eq!(p.as_ptr() as usize % 8, 0, "aligned");
            assert!(inside_a_page_of(&P, p));
            // SAFETY: a 48-byte item of the pool, this test's.
            unsafe { ptr::write_bytes(p.as_ptr(), i, 48) };
            items.push(p);
        }
        let mut sorted = items.clone();
        sorted.sort_unstable();
        sorted.dedup();
        assert_eq!(sorted.len(), 200, "every item is distinct");
        assert_eq!(P.pr_nout.get(), 200);
        assert_eq!(P.pr_npages.get() as usize, 200usize.div_ceil(itemsperpage));

        for (i, p) in items.iter().enumerate() {
            // SAFETY: as above.
            assert!((unsafe { ptr::read_volatile(p.as_ptr()) }) == i as u8);
            pool_put(&P, *p);
        }
        assert_eq!(P.pr_nout.get(), 0);
        assert_eq!(P.pr_nidle.get() as u32, P.pr_npages.get());

        let free_before = UVMEXP.free.load(Ordering::Relaxed);
        assert!(pool_reclaim(&P));
        assert_eq!(P.pr_npages.get(), 0);
        assert!(UVMEXP.free.load(Ordering::Relaxed) > free_before);
        pool_destroy(&P);
    }

    #[test]
    fn large_items_keep_their_headers_in_phpool() {
        let _g = setup_real_memory();
        static P: Pool = Pool::new();
        pool_init(&P, 1024, 0, IPL_HIGH, 0, "test1k", None);
        // 1 KiB items make the pool use 8 KiB pages (size * 8 > PAGE_SIZE), which the items fill
        // exactly, so the header goes off-page into phpool.
        assert!(!pool_inpghdr(&P), "the header goes off-page");
        assert_eq!(P.pr_pgsize.get() as usize, 2 * PAGE_SIZE);
        assert_eq!(P.pr_itemsperpage.get(), 8);

        let items: Vec<NonNull<u8>> = (0..10)
            .map(|_| pool_get(&P, PR_NOWAIT).expect("an item"))
            .collect();
        assert_eq!(P.pr_npages.get(), 2);
        assert_eq!(P.pr_phtree.iter().count(), 2);
        for p in &items {
            let ph = pr_find_pagehead(&P, p.as_ptr());
            let page = ph.ph_page.get() as usize;
            let pgsize = P.pr_pgsize.get() as usize;
            assert!(page <= p.as_ptr() as usize && (p.as_ptr() as usize) < page + pgsize);
        }
        for p in items {
            pool_put(&P, p);
        }
        assert!(pool_reclaim(&P));
        assert_eq!(P.pr_phtree.iter().count(), 0);
        pool_destroy(&P);
    }

    #[test]
    fn hard_limit_and_limitfail() {
        let _g = setup_real_memory();
        static P: Pool = Pool::new();
        pool_init(&P, 64, 0, IPL_HIGH, 0, "testlim", None);
        assert_eq!(pool_sethardlimit(&P, 2), Ok(()));
        let a = pool_get(&P, PR_NOWAIT).expect("first");
        let b = pool_get(&P, PR_NOWAIT).expect("second");
        assert!(pool_get(&P, PR_NOWAIT | PR_LIMITFAIL).is_none());
        assert_eq!(P.pr_nfail.get(), 1);
        assert_eq!(pool_sethardlimit(&P, 1), Err(Errno::EINVAL));
        pool_put(&P, a);
        pool_put(&P, b);
        pool_destroy(&P);
    }

    #[test]
    fn pr_zero_clears_a_reused_item() {
        let _g = setup_real_memory();
        static P: Pool = Pool::new();
        pool_init(&P, 96, 0, IPL_HIGH, 0, "testzero", None);
        let p = pool_get(&P, PR_NOWAIT).expect("an item");
        // SAFETY: a 96-byte item of the pool, this test's.
        unsafe { ptr::write_bytes(p.as_ptr(), 0xa5, 96) };
        pool_put(&P, p);
        let q = pool_get(&P, PR_NOWAIT | PR_ZERO).expect("an item");
        // SAFETY: as above.
        assert!((0..96).all(|i| (unsafe { ptr::read_volatile(q.as_ptr().add(i)) }) == 0));
        pool_put(&P, q);
        pool_destroy(&P);
    }

    #[test]
    fn setlowat_primes_pages_and_reclaim_keeps_them() {
        let _g = setup_real_memory();
        static P: Pool = Pool::new();
        pool_init(&P, 128, 0, IPL_HIGH, 0, "testlow", None);
        pool_setlowat(&P, 100);
        assert!(P.pr_nitems.get() >= 100);
        assert!(P.pr_npages.get() >= 2);
        assert!(
            !pool_reclaim(&P),
            "nothing above the low water mark to release"
        );
        assert!(P.pr_npages.get() >= 2);
        pool_setlowat(&P, 0);
        assert!(pool_reclaim(&P));
        assert_eq!(P.pr_npages.get(), 0);
        pool_destroy(&P);
    }

    #[test]
    fn freelist_order_is_randomised_and_items_do_not_overlap() {
        let _g = setup_real_memory();
        static P: Pool = Pool::new();
        pool_init(&P, 200, 0, IPL_HIGH, 0, "testrnd", None);
        let items: Vec<usize> = (0..40)
            .map(|_| pool_get(&P, PR_NOWAIT).expect("an item").as_ptr() as usize)
            .collect();
        let ascending = items.windows(2).all(|w| w[1] > w[0]);
        let descending = items.windows(2).all(|w| w[1] < w[0]);
        assert!(!ascending && !descending, "the free list is shuffled");
        let mut sorted = items.clone();
        sorted.sort_unstable();
        assert!(sorted.windows(2).all(|w| w[1] - w[0] >= 200), "no overlap");
        for p in items {
            pool_put(&P, NonNull::new(p as *mut u8).expect("non-null"));
        }
        pool_destroy(&P);
    }

    std::thread_local! {
        /// The items `pool_walk` reported to `walk_collect`.
        static WALKED: core::cell::RefCell<Vec<usize>> = const { core::cell::RefCell::new(Vec::new()) };
    }

    fn walk_collect(item: *const u8, full: bool, _pr: PoolWalkPr) {
        assert!(!full);
        WALKED.with(|w| w.borrow_mut().push(item as usize));
    }

    fn walk_pr(_args: core::fmt::Arguments<'_>) -> usize {
        0
    }

    #[test]
    fn pool_walk_visits_exactly_the_items_in_use() {
        let _g = setup_real_memory();
        static P: Pool = Pool::new();
        pool_init(&P, 1000, 0, IPL_HIGH, 0, "testwalk", None);
        let per_page = P.pr_itemsperpage.get() as usize;
        assert!(per_page >= 2);

        // Enough for a full page and part of the next.
        let n = per_page + 2;
        let items: Vec<NonNull<u8>> = (0..n)
            .map(|_| pool_get(&P, PR_NOWAIT).expect("an item"))
            .collect();
        let mut in_use: Vec<usize> = items.iter().map(|p| p.as_ptr() as usize).collect();

        let walk = || {
            WALKED.with(|w| w.borrow_mut().clear());
            pool_walk(&P, false, walk_pr, walk_collect);
            let mut seen = WALKED.with(|w| w.borrow().clone());
            seen.sort_unstable();
            seen
        };
        in_use.sort_unstable();
        assert_eq!(walk(), in_use);

        // Give three back (from both pages): they are no longer reported.
        for p in [items[0], items[per_page], items[n - 1]] {
            pool_put(&P, p);
            in_use.retain(|&a| a != p.as_ptr() as usize);
        }
        assert_eq!(walk(), in_use);

        for p in &items {
            if in_use.contains(&(p.as_ptr() as usize)) {
                pool_put(&P, *p);
            }
        }
        assert_eq!(walk(), Vec::<usize>::new());
        pool_destroy(&P);
    }
}
/* </TESTS> */
