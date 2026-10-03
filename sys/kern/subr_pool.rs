/*	$OpenBSD: subr_pool.c,v 1.243 2026/01/29 01:04:35 dlg Exp $	*/
/*	$NetBSD: subr_pool.c,v 1.61 2001/09/26 07:14:56 chs Exp $	*/
/* <LICENSES> */
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
//! The per-CPU caches (`MULTIPROCESSOR`), the garbage collector (`pool_gc_*`, a timeout and
//! a task), the ddb printers, `pool_chk`, `pool_walk` and `sysctl_dopool` come with M5 to M7.
//!
//! ## Deviations
//! - The lock operations keep a flag per lock so the `pl_assert_*` checks mean something;
//!   the mutex and rwlock they stand for arrive with M5. `pl_sleep` cannot sleep: a
//!   `PR_WAITOK` `pool_get` that finds no memory fails instead of queueing a request and
//!   sleeping for `pool_runqueue` (a request left queued would outlive the caller's frame).
//! - `pool_lock` (the rwlock over the pool list) and `pr_refcnt` wait for M5; the boot CPU
//!   is alone. `splassert(pr_ipl)` waits for M4; `KERNEL_LOCK` in the `_ni` allocators for M5.
//! - `poison_mem`/`poison_check` (`subr_poison.c`) are reported where `POOL_DEBUG` would
//!   call them; the double-put check under `DIAGNOSTIC` is ported.
//! - `arc4random` (page magics, freelist order, the `XSIMPLEQ` cookies) is `dev/rnd.rs`'s
//!   placeholder stream until M5.

use core::cell::Cell;
use core::ptr::{self, NonNull};
use core::sync::atomic::{AtomicI32, AtomicU32, Ordering};

use crate::dev::rnd::{arc4random, arc4random_buf};
use crate::kern::kern_rwlock::{rw_enter_read, rw_enter_write, rw_exit_read, rw_exit_write};
use crate::kern::kern_synch::wakeup_one;
use crate::kern::kern_tc::getnsecuptime;
use crate::machine::intr::IPL_HIGH;
use crate::sys::errno::Errno;
use crate::sys::param::{PAGE_SIZE, align, roundup};
use crate::sys::pool::{
    POOL_ALLOC_ALIGNED, POOL_ALLOC_DEFAULT, PR_LIMITFAIL, PR_NOWAIT, PR_RWLOCK, PR_WAITOK, PR_ZERO,
    Pool, PoolAllocator, PoolLock, PoolRequest, PoolRequestHandler, PrEntry, pool_alloc_size,
    pool_alloc_sizes,
};
use crate::sys::queue::{SimpleqHead, TailqEntry, TailqHead, XsimpleqEntry, XsimpleqHead};
use crate::sys::rwlock::Rwlock;
use crate::sys::tree::RbtEntry;
use crate::uvm::uvm_extern::{KMEM_DYN_INITIALIZER, KmemPaMode, KmemVaMode};
use crate::uvm::uvm_km::{KP_DIRTY, KV_ANY, KV_INTRSAFE, KV_PAGE, km_alloc, km_free};
use crate::{kassert, queue_adapter, tree_adapter, unported};

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
    /// `pl_sleep`: sleeps on `ident` with the lock released; returns 0 or an errno.
    pub pl_sleep: fn(*const (), &PoolLock, i32, &str) -> i32,
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

// SAFETY: guarded by `pool_lock` (M5); the boot CPU is alone until then.
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

#[allow(dead_code)] // the C's, for pool_cache (MULTIPROCESSOR)
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

#[allow(dead_code)] // the C's, for pool_get's sleeping path (M5)
fn pl_sleep(pp: &Pool, ident: *const (), lock: &PoolLock, priority: i32, wmesg: &str) -> i32 {
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
    // refcnt_init(&pp->pr_refcnt): M5.
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

    // Wait for concurrent sysctl_dopool(): refcnt_finalize(&pp->pr_refcnt, "pooldtor"), M5.

    // MULTIPROCESSOR: pool_cache_destroy, not configured.

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

/// `pool_get`: grab an item from the pool.
pub fn pool_get(pp: &Pool, flags: i32) -> Option<NonNull<u8>> {
    let mut v: Option<NonNull<u8>> = None;
    let mut slowdown = 0;

    // assertwaitok() with PR_WAITOK: M5.

    kassert!(flags & (PR_WAITOK | PR_NOWAIT) != 0);
    if pp.pr_flags.get() & PR_RWLOCK != 0 {
        kassert!(flags & PR_WAITOK != 0);
    }

    // MULTIPROCESSOR: pool_cache_get, not configured.

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
        // yield(): M5.
    }

    let Some(v) = v else {
        // The C queues a pool_request here and sleeps in pl_sleep until pool_runqueue serves
        // it (see the module's deviations).
        let _ = unported!("pool_get: sleeping for memory (pool_request + pl_sleep, M5)");
        pl_enter(pp, &pp.pr_lock);
        return pool_get_fail(pp);
    };

    if flags & PR_ZERO != 0 {
        // SAFETY: `v` is a free item of `pr_size` bytes that is now the caller's.
        unsafe { ptr::write_bytes(v.as_ptr(), 0, pp.pr_size.get() as usize) };
    }

    // TRACEPOINT(uvm, pool_get): not configured.

    Some(v)
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

    // MULTIPROCESSOR: pool_cache_put, not configured.

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

    // splvm(): M4.
    km_alloc(pp.pr_pgsize.get() as usize, &kv, pp.pr_crange.get()?, &kd)
}

/// `pool_multi_free`.
pub fn pool_multi_free(pp: &Pool, v: NonNull<u8>) {
    let mut kv: KmemVaMode = KV_INTRSAFE;

    if pool_inpghdr(pp) {
        kv.kv_align = pp.pr_pgsize.get() as usize;
    }

    // splvm(): M4.
    if let Some(crange) = pp.pr_crange.get() {
        km_free(v, pp.pr_pgsize.get() as usize, &kv, crange);
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

    // KERNEL_LOCK(): M5.
    km_alloc(pp.pr_pgsize.get() as usize, &kv, pp.pr_crange.get()?, &kd)
}

/// `pool_multi_free_ni`.
pub fn pool_multi_free_ni(pp: &Pool, v: NonNull<u8>) {
    let mut kv: KmemVaMode = KV_ANY;

    if pool_inpghdr(pp) {
        kv.kv_align = pp.pr_pgsize.get() as usize;
    }

    // KERNEL_LOCK(): M5.
    if let Some(crange) = pp.pr_crange.get() {
        km_free(v, pp.pr_pgsize.get() as usize, &kv, crange);
    }
}

// The lock operations (see the module's deviations).

fn pool_lock_mtx_init(_pp: &Pool, lock: &PoolLock) {
    // _mtx_init_flags(&lock->prl_mtx, pp->pr_ipl, pp->pr_wchan, 0, type): M5.
    lock.set_locked(false);
}

fn pool_lock_mtx_enter(lock: &PoolLock) {
    kassert!(!lock.is_locked());
    lock.set_locked(true);
}

fn pool_lock_mtx_enter_try(lock: &PoolLock) -> bool {
    if lock.is_locked() {
        return false;
    }
    lock.set_locked(true);
    true
}

fn pool_lock_mtx_leave(lock: &PoolLock) {
    kassert!(lock.is_locked());
    lock.set_locked(false);
}

fn pool_lock_mtx_assert_locked(lock: &PoolLock) {
    kassert!(lock.is_locked());
}

fn pool_lock_mtx_assert_unlocked(lock: &PoolLock) {
    kassert!(!lock.is_locked());
}

fn pool_lock_mtx_sleep(_ident: *const (), _lock: &PoolLock, _priority: i32, _wmesg: &str) -> i32 {
    unported!("msleep_nsec (pool lock sleep, M5)") as i32
}

fn pool_lock_rw_init(_pp: &Pool, lock: &PoolLock) {
    // _rw_init_flags(&lock->prl_rwlock, pp->pr_wchan, 0, type, 0): M5.
    lock.set_locked(false);
}

fn pool_lock_rw_enter(lock: &PoolLock) {
    kassert!(!lock.is_locked());
    lock.set_locked(true);
}

fn pool_lock_rw_enter_try(lock: &PoolLock) -> bool {
    if lock.is_locked() {
        return false;
    }
    lock.set_locked(true);
    true
}

fn pool_lock_rw_leave(lock: &PoolLock) {
    kassert!(lock.is_locked());
    lock.set_locked(false);
}

fn pool_lock_rw_assert_locked(lock: &PoolLock) {
    kassert!(lock.is_locked());
}

fn pool_lock_rw_assert_unlocked(lock: &PoolLock) {
    kassert!(!lock.is_locked());
}

fn pool_lock_rw_sleep(_ident: *const (), _lock: &PoolLock, _priority: i32, _wmesg: &str) -> i32 {
    unported!("rwsleep_nsec (pool lock sleep, M5)") as i32
}

#[cfg(test)]
pub(crate) mod tests;
