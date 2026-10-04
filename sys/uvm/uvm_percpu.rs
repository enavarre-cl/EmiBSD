/*	$OpenBSD: uvm_percpu.h,v 1.3 2024/05/01 12:54:27 mpi Exp $	*/

/* <LICENSES> */
/*
 * Copyright (c) 2024 Martin Pieuchot <mpi@openbsd.org>
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
/* </LICENSES> */

//! The per-CPU cache of physical pages: `<uvm/uvm_percpu.h>`.
//!
//! Upstream: sys/uvm/uvm_percpu.h @ 3ce1f3f79392
//!
//! Two magazines of free pages per CPU, which `uvm_pmemrange.c`'s `uvm_pmr_cache_get` and
//! `uvm_pmr_cache_put` fill and drain (`MULTIPROCESSOR && __HAVE_UVM_PERCPU`, which amd64 and
//! arm64 define), so concurrent page faults do not all contend for `uvm.fpageqlock`.
//!
//! ## Deviations
//! - The fields are `Cell`s: only the owning CPU touches its cache, at `splbio` (see
//!   [`UvmPmrCache`]'s `Sync`).

use core::cell::Cell;
use core::ptr;

use crate::uvm::uvm_page::VmPage;

/// `UVM_PMR_CACHEMAGSZ`: the number of pages per magazine should be large enough to get rid
/// of the contention in the pmemrange allocator during concurrent page faults and small
/// enough to limit fragmentation.
pub const UVM_PMR_CACHEMAGSZ: usize = 8;

/// `struct uvm_pmr_cache_item`: a magazine.
pub struct UvmPmrCacheItem {
    /// `upci_pages`: the pages; the first `upci_npages` are valid.
    pub upci_pages: [Cell<*const VmPage>; UVM_PMR_CACHEMAGSZ],
    /// `upci_npages`: # of pages in magazine.
    pub upci_npages: Cell<i32>,
}

impl UvmPmrCacheItem {
    /// An empty magazine.
    pub const fn new() -> Self {
        Self {
            upci_pages: [const { Cell::new(ptr::null()) }; UVM_PMR_CACHEMAGSZ],
            upci_npages: Cell::new(0),
        }
    }
}

impl Default for UvmPmrCacheItem {
    fn default() -> Self {
        Self::new()
    }
}

/// `struct uvm_pmr_cache`: per-CPU cache of physical pages.
pub struct UvmPmrCache {
    /// `upc_magz`: magazines.
    pub upc_magz: [UvmPmrCacheItem; 2],
    /// `upc_actv`: index of active magazine.
    pub upc_actv: Cell<i32>,
}

impl UvmPmrCache {
    /// Two empty magazines, the first one active.
    pub const fn new() -> Self {
        Self {
            upc_magz: [UvmPmrCacheItem::new(), UvmPmrCacheItem::new()],
            upc_actv: Cell::new(0),
        }
    }
}

impl Default for UvmPmrCache {
    fn default() -> Self {
        Self::new()
    }
}

// SAFETY: a cache is only touched by the CPU it belongs to, at `splbio` (the C's `[o]`, with
// the `splbio` that keeps the buffer flipper's interrupt-time allocations out); the kernel is
// not preemptive, so a thread cannot move to another CPU between finding its cache and
// leaving `splbio`.
unsafe impl Sync for UvmPmrCache {}
