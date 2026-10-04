/*	$OpenBSD: pool.h,v 1.81 2025/05/21 09:33:49 mvs Exp $	*/
/*	$NetBSD: pool.h,v 1.27 2001/06/06 22:00:17 rafal Exp $	*/
/* <LICENSES> */
/*-
 * Copyright (c) 1997, 1998, 1999, 2000 The NetBSD Foundation, Inc.
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

//! `<sys/pool.h>`: the pool(9) resource allocator's types.
//!
//! Upstream: sys/sys/pool.h @ 3ce1f3f79392
//!
//! Status: `wip`. Milestone M3 ports `struct pool`, `struct pool_allocator`,
//! `struct pool_request`, the `PR_*` flags and the `POOL_ALLOC_*` encoding; `struct
//! kinfo_pool` (diagnostic tools, stage 2). The per-CPU cache fields and their
//! `kinfo_pool_cache*` records (`MULTIPROCESSOR`) come later.
//!
//! ## Deviations
//! - `union pool_lock` (a mutex or an rwlock, M5) is [`PoolLock`], a flag that only backs
//!   the `pl_assert_*` checks until the real locks land.
//! - `pr_refcnt` (`refcnt(9)`, M5) is not here.
//! - `struct kinfo_pool` is copied out through `KinfoPool::to_bytes` (the C layout with its
//!   padding zeroed), not as the Rust structure's memory.
//! - `pr_wchan` is a `&'static str`; `pr_alloc` and `pr_crange` are references, `None` before
//!   `pool_init`.

use core::cell::Cell;
use core::ptr::{self, NonNull};

use crate::kern::subr_pool::{PhEntry, Phtree, PoolLockOps, PoolPageHeader};
use crate::queue_adapter;
use crate::sys::param::PAGE_SIZE;
use crate::sys::queue::{SimpleqEntry, TailqEntry, TailqHead};
use crate::sys::tree::RbtHead;
use crate::uvm::uvm_extern::KmemPaMode;

// sysctls: kern.pool.npools, kern.pool.name.<number>, kern.pool.pool.<number>

/// `KERN_POOL_NPOOLS`.
pub const KERN_POOL_NPOOLS: i32 = 1;
/// `KERN_POOL_NAME`.
pub const KERN_POOL_NAME: i32 = 2;
/// `KERN_POOL_POOL`.
pub const KERN_POOL_POOL: i32 = 3;
/// `KERN_POOL_CACHE`: global pool cache info.
pub const KERN_POOL_CACHE: i32 = 4;
/// `KERN_POOL_CACHE_CPUS`: all cpus cache info.
pub const KERN_POOL_CACHE_CPUS: i32 = 5;

/// `struct kinfo_pool`: what `kern.pool.pool.<serial>` reports about a pool.
#[derive(Clone, Copy, Default, Debug, PartialEq, Eq)]
pub struct KinfoPool {
    /// `pr_size`: size of a pool item.
    pub pr_size: u32,
    /// `pr_pgsize`: size of a "page".
    pub pr_pgsize: u32,
    /// `pr_itemsperpage`: number of items per "page".
    pub pr_itemsperpage: u32,
    /// `pr_minpages`: same in page units.
    pub pr_minpages: u32,
    /// `pr_maxpages`: maximum # of idle pages to keep.
    pub pr_maxpages: u32,
    /// `pr_hardlimit`: hard limit to number of allocated items.
    pub pr_hardlimit: u32,
    /// `pr_npages`: # of pages allocated.
    pub pr_npages: u32,
    /// `pr_nout`: # items currently allocated.
    pub pr_nout: u32,
    /// `pr_nitems`: # items in the pool.
    pub pr_nitems: u32,
    /// `pr_nget`: # of successful requests.
    pub pr_nget: u64,
    /// `pr_nput`: # of releases.
    pub pr_nput: u64,
    /// `pr_nfail`: # of unsuccessful requests.
    pub pr_nfail: u64,
    /// `pr_npagealloc`: # of pages allocated.
    pub pr_npagealloc: u64,
    /// `pr_npagefree`: # of pages released.
    pub pr_npagefree: u64,
    /// `pr_hiwat`: max # of pages in pool.
    pub pr_hiwat: u32,
    /// `pr_nidle`: # of idle pages.
    pub pr_nidle: u64,
}

impl KinfoPool {
    /// The structure's bytes in the C layout (LP64: `unsigned long` is 8 bytes), 96 bytes,
    /// the padding after `pr_nitems` and `pr_hiwat` zeroed ("don't leak padding").
    pub fn to_bytes(&self) -> [u8; 96] {
        let mut out = [0u8; 96];
        let ints = [
            self.pr_size,
            self.pr_pgsize,
            self.pr_itemsperpage,
            self.pr_minpages,
            self.pr_maxpages,
            self.pr_hardlimit,
            self.pr_npages,
            self.pr_nout,
            self.pr_nitems,
        ];
        for (i, v) in ints.iter().enumerate() {
            out[4 * i..4 * i + 4].copy_from_slice(&v.to_ne_bytes());
        }
        let longs = [
            self.pr_nget,
            self.pr_nput,
            self.pr_nfail,
            self.pr_npagealloc,
            self.pr_npagefree,
        ];
        for (i, v) in longs.iter().enumerate() {
            out[40 + 8 * i..48 + 8 * i].copy_from_slice(&v.to_ne_bytes());
        }
        out[80..84].copy_from_slice(&self.pr_hiwat.to_ne_bytes());
        out[88..96].copy_from_slice(&self.pr_nidle.to_ne_bytes());
        out
    }
}

/// `pa_alloc`: gives `pr_pgsize` bytes for the pool; `slowdown` is set when the caller should
/// yield.
pub type PoolAllocFn = fn(&Pool, i32, &mut i32) -> Option<NonNull<u8>>;
/// `pa_free`: takes a page back.
pub type PoolFreeFn = fn(&Pool, NonNull<u8>);

/// `struct pool_allocator`: a backend allocator.
pub struct PoolAllocator {
    /// Allocates a page.
    pub pa_alloc: PoolAllocFn,
    /// Frees a page.
    pub pa_free: PoolFreeFn,
    /// The sizes of pages the allocator provides, and whether they are aligned (see below).
    pub pa_pagesz: usize,
}

// The pa_pagesz member encodes the sizes of pages that can be provided by the allocator, and
// whether the allocations can be aligned to their size.
//
// Page sizes can only be powers of two. Each available page size is represented by its value
// set as a bit. e.g., to indicate that an allocator can provide 16k and 32k pages you
// initialise pa_pagesz to (32768 | 16384).
//
// If the allocator can provide aligned pages the low bit in pa_pagesz is set. The
// POOL_ALLOC_ALIGNED macro is provided as a convenience.
//
// If pa_pagesz is unset (i.e. 0), POOL_ALLOC_DEFAULT will be used instead.

/// `POOL_ALLOC_ALIGNED`.
pub const POOL_ALLOC_ALIGNED: usize = 1;

/// `POOL_ALLOC_SIZE(_sz, _a)`.
pub const fn pool_alloc_size(sz: usize, a: usize) -> usize {
    sz | a
}

/// `POOL_ALLOC_SIZES(_min, _max, _a)`.
pub const fn pool_alloc_sizes(min: usize, max: usize, a: usize) -> usize {
    max | ((max - 1) & !(min - 1)) | a
}

/// `POOL_ALLOC_DEFAULT`.
pub const POOL_ALLOC_DEFAULT: usize = pool_alloc_size(PAGE_SIZE, POOL_ALLOC_ALIGNED);

/// `union pool_lock`: the pool's mutex or rwlock (see the module's deviations).
pub struct PoolLock {
    locked: Cell<bool>,
}

impl PoolLock {
    /// An unlocked lock.
    pub const fn new() -> Self {
        Self {
            locked: Cell::new(false),
        }
    }

    /// Whether the lock is held.
    pub fn is_locked(&self) -> bool {
        self.locked.get()
    }

    /// Records the lock as held or not.
    pub fn set_locked(&self, locked: bool) {
        self.locked.set(locked);
    }
}

impl Default for PoolLock {
    fn default() -> Self {
        Self::new()
    }
}

/// `PR_WAITOK`: `M_WAITOK`.
pub const PR_WAITOK: i32 = 0x0001;
/// `PR_NOWAIT`: `M_NOWAIT`.
pub const PR_NOWAIT: i32 = 0x0002;
/// `PR_LIMITFAIL`: `M_CANFAIL`.
pub const PR_LIMITFAIL: i32 = 0x0004;
/// `PR_ZERO`: `M_ZERO`.
pub const PR_ZERO: i32 = 0x0008;
/// `PR_RWLOCK`: the pool is guarded by an rwlock.
pub const PR_RWLOCK: i32 = 0x0010;
/// `PR_WANTED`.
pub const PR_WANTED: i32 = 0x0100;

queue_adapter!(
    /// `SIMPLEQ_ENTRY(pool) pr_poollist`: the list of all pools.
    pub PoolList: Pool, pr_poollist => SimpleqEntry<Pool>
);

queue_adapter!(
    /// `TAILQ_HEAD(pool_requests, pool_request)`.
    pub PrEntry: PoolRequest, pr_entry => TailqEntry<PoolRequest>
);

/// `struct pool`.
pub struct Pool {
    // pr_refcnt: M5.
    /// The pool's lock.
    pub pr_lock: PoolLock,
    /// The lock's operations (mutex or rwlock).
    pub pr_lock_ops: Cell<Option<&'static PoolLockOps>>,
    /// The list of all pools.
    pub pr_poollist: SimpleqEntry<Pool>,
    /// Empty pages.
    pub pr_emptypages: TailqHead<PhEntry>,
    /// Full pages.
    pub pr_fullpages: TailqHead<PhEntry>,
    /// Partially-allocated pages.
    pub pr_partpages: TailqHead<PhEntry>,
    /// The page items are taken from.
    pub pr_curpage: Cell<*const PoolPageHeader>,
    /// Size of item.
    pub pr_size: Cell<u32>,
    /// Minimum # of items to keep.
    pub pr_minitems: Cell<u32>,
    /// Same in page units.
    pub pr_minpages: Cell<u32>,
    /// Maximum # of idle pages to keep.
    pub pr_maxpages: Cell<u32>,
    /// # of pages allocated.
    pub pr_npages: Cell<u32>,
    /// # items that fit in a page.
    pub pr_itemsperpage: Cell<u32>,
    /// Unused space in a page.
    pub pr_slack: Cell<u32>,
    /// Number of available items in pool.
    pub pr_nitems: Cell<u32>,
    /// # items currently allocated.
    pub pr_nout: Cell<u32>,
    /// Hard limit to number of allocated items.
    pub pr_hardlimit: Cell<u32>,
    /// Unique serial number of the pool.
    pub pr_serial: Cell<u32>,
    /// Size of a "page".
    pub pr_pgsize: Cell<u32>,
    /// Mask with an item to get a page.
    pub pr_pgmask: Cell<usize>,
    /// Backend allocator.
    pub pr_alloc: Cell<Option<&'static PoolAllocator>>,
    /// tsleep(9) identifier.
    pub pr_wchan: Cell<&'static str>,
    /// `PR_*`.
    pub pr_flags: Cell<i32>,
    /// The IPL the pool is used at.
    pub pr_ipl: Cell<i32>,
    /// Off-page page headers, by page address.
    pub pr_phtree: RbtHead<Phtree>,
    // pr_cache* (MULTIPROCESSOR): not configured.
    /// Item alignment.
    pub pr_align: Cell<u32>,
    /// Cache coloring.
    pub pr_maxcolors: Cell<u32>,
    /// Offset in page of page header.
    pub pr_phoffset: Cell<i32>,
    // pool item requests queue
    /// The requests queue's lock.
    pub pr_requests_lock: PoolLock,
    /// The requests queue.
    pub pr_requests: TailqHead<PrEntry>,
    /// A request run is in progress.
    pub pr_requesting: Cell<u32>,
    // Instrumentation
    /// # of successful requests.
    pub pr_nget: Cell<u64>,
    /// # of unsuccessful requests.
    pub pr_nfail: Cell<u64>,
    /// # of releases.
    pub pr_nput: Cell<u64>,
    /// # of pages allocated.
    pub pr_npagealloc: Cell<u64>,
    /// # of pages released.
    pub pr_npagefree: Cell<u64>,
    /// Max # of pages in pool.
    pub pr_hiwat: Cell<u32>,
    /// # of idle pages.
    pub pr_nidle: Cell<u64>,
    /// Physical memory configuration.
    pub pr_crange: Cell<Option<&'static KmemPaMode>>,
}

// SAFETY: every field is guarded by `pr_lock` or `pr_requests_lock` (M5); the boot CPU is
// alone until then.
unsafe impl Sync for Pool {}

impl Pool {
    /// A pool before `pool_init` (the C's zeroed `struct pool`).
    pub const fn new() -> Self {
        Self {
            pr_lock: PoolLock::new(),
            pr_lock_ops: Cell::new(None),
            pr_poollist: SimpleqEntry::new(),
            pr_emptypages: TailqHead::new(),
            pr_fullpages: TailqHead::new(),
            pr_partpages: TailqHead::new(),
            pr_curpage: Cell::new(ptr::null()),
            pr_size: Cell::new(0),
            pr_minitems: Cell::new(0),
            pr_minpages: Cell::new(0),
            pr_maxpages: Cell::new(0),
            pr_npages: Cell::new(0),
            pr_itemsperpage: Cell::new(0),
            pr_slack: Cell::new(0),
            pr_nitems: Cell::new(0),
            pr_nout: Cell::new(0),
            pr_hardlimit: Cell::new(0),
            pr_serial: Cell::new(0),
            pr_pgsize: Cell::new(0),
            pr_pgmask: Cell::new(0),
            pr_alloc: Cell::new(None),
            pr_wchan: Cell::new(""),
            pr_flags: Cell::new(0),
            pr_ipl: Cell::new(0),
            pr_phtree: RbtHead::new(),
            pr_align: Cell::new(0),
            pr_maxcolors: Cell::new(0),
            pr_phoffset: Cell::new(0),
            pr_requests_lock: PoolLock::new(),
            pr_requests: TailqHead::new(),
            pr_requesting: Cell::new(0),
            pr_nget: Cell::new(0),
            pr_nfail: Cell::new(0),
            pr_nput: Cell::new(0),
            pr_npagealloc: Cell::new(0),
            pr_npagefree: Cell::new(0),
            pr_hiwat: Cell::new(0),
            pr_nidle: Cell::new(0),
            pr_crange: Cell::new(None),
        }
    }
}

impl Default for Pool {
    fn default() -> Self {
        Self::new()
    }
}

/// `pr_handler`: called with the pool, the request's cookie and the item.
pub type PoolRequestHandler = fn(&Pool, *mut (), NonNull<u8>);

/// `struct pool_request`: a deferred item request, served when memory frees up.
pub struct PoolRequest {
    /// The requests queue entry.
    pub pr_entry: TailqEntry<PoolRequest>,
    /// Called with the item.
    pub pr_handler: PoolRequestHandler,
    /// Passed to the handler.
    pub pr_cookie: *mut (),
    /// The item, once served.
    pub pr_item: Cell<Option<NonNull<u8>>>,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn allocator_size_encoding() {
        assert_eq!(pool_alloc_size(4096, POOL_ALLOC_ALIGNED), 4097);
        let sizes = pool_alloc_sizes(4096, 1 << 31, POOL_ALLOC_ALIGNED);
        for shift in 12..=31 {
            assert!(sizes & (1 << shift) != 0, "{shift}");
        }
        assert!(sizes & (1 << 11) == 0);
        assert!(sizes & POOL_ALLOC_ALIGNED != 0);
        assert_eq!(POOL_ALLOC_DEFAULT, PAGE_SIZE | 1);
    }

    #[test]
    fn kinfo_pool_layout() {
        let pi = KinfoPool {
            pr_size: 1,
            pr_nitems: 9,
            pr_nget: 10,
            pr_npagefree: 14,
            pr_hiwat: 15,
            pr_nidle: 16,
            ..KinfoPool::default()
        };
        let b = pi.to_bytes();
        let word = |o: usize| u64::from_ne_bytes(b[o..o + 8].try_into().expect("8 bytes"));
        assert_eq!(u32::from_ne_bytes(b[0..4].try_into().expect("4")), 1);
        assert_eq!(u32::from_ne_bytes(b[32..36].try_into().expect("4")), 9);
        assert_eq!(&b[36..40], &[0; 4], "padding");
        assert_eq!((word(40), word(72)), (10, 14));
        assert_eq!(u32::from_ne_bytes(b[80..84].try_into().expect("4")), 15);
        assert_eq!(word(88), 16);
    }
}
