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

/*
 * Copyright (c) 2024 Martin Pieuchot <mpi@openbsd.org>
 * Copyright (c) 2009, 2010 Ariane van der Steldt <ariane@stack.nl>
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

/* <CODE> */
//! Describe and manage free physical memory: `<uvm/uvm_pmemrange.h>` and
//! `uvm/uvm_pmemrange.c`.
//!
//! Upstream: sys/uvm/uvm_pmemrange.h @ 3ce1f3f79392
//! Upstream: sys/uvm/uvm_pmemrange.c @ 3ce1f3f79392
//!
//! 2 trees: addr tree and size tree.
//!
//! The allocator keeps chunks of free pages (called a range). Two pages are part of the same
//! range if: all pages in between are part of that range, they are of the same memory type
//! (zeroed or non-zeroed), they are part of the same pmemrange. A pmemrange is a range of memory
//! which is part of the same vm_physseg and has a use-count.
//!
//! addr tree is `vm_page[0].objt`; size tree is `vm_page[1].objt`.
//!
//! The size tree is not used for memory ranges of 1 page, instead, single queue is
//! `vm_page[0].pageq`.
//!
//! `vm_page[0].fpgsz` describes the length of a free range. Two adjacent ranges are joined,
//! unless: they have pages in between them which are not free; they belong to different
//! memtypes (zeroed vs dirty memory); they are in different pmemrange areas (ISA vs non-ISA
//! memory for instance); they are not a continuation of the same array. The latter issue is
//! caused by vm_physseg ordering and splitting from the MD initialization machinery. The MD code
//! is dependant on freelists and happens to split ISA memory from non-ISA memory. (Note:
//! freelists die die die!)
//!
//! `uvm_page_init` guarantees that every vm_physseg contains an array of struct vm_page. Also,
//! `uvm_page_physload` allocates an array of struct vm_page. This code depends on that array.
//! The array may break across vm_physsegs boundaries.
//!
//! Status: `wip`.
//!
//! ## Deviations
//! - Page numbers (`paddr_t` holding `atop()` results in C) are `usize`; byte addresses stay
//!   `Paddr`.
//! - Pages and ranges are `&'static`: the page arrays and the pmemranges live for the whole
//!   kernel; [`VmPage::forever`] restores that lifetime where a list API shortened it.
//! - `uvm_lock_fpageq` is the C's mutex (`uvm.fpageqlock`, M11a): every path that touches the
//!   ranges' trees and free lists holds it as the C does. `uvm_wait` is reported as unported
//!   (there is no page daemon), so a `UVM_PLA_WAITOK` request that cannot be met returns
//!   `ENOMEM` instead of sleeping for the page daemon.
//! - `in_pagedaemon` answers false: there is no page daemon nor `curproc` yet (M5).
//! - The `goto`-driven search of `uvm_pmr_getpages` is written with labelled loops; the order
//!   of the tries, memtypes and ranges is the C's.
//! - `uvm_pmr_assertvalid` (`DEBUG`) is compiled for feature `debug` and for the host tests.
//! - `uvm_pagezero_thread` needs `msleep`/`yield` (M5) and is not here.
//! - The per-CPU page cache (`MULTIPROCESSOR && __HAVE_UVM_PERCPU`, both machines define the
//!   latter; M11e) keeps each CPU's magazines in [`UVM_PMR_CACHES`], indexed by
//!   `cpu_number()`, instead of `curcpu()->ci_uvm`: the uvm type stays out of the machine's
//!   `struct cpu_info`. Without `MULTIPROCESSOR` the single-CPU versions are used, as in C.
//! - `uvm_pmr_allocpmr` after `uvm_init` needs `malloc(9)` (later in M3).

use core::cell::Cell;
use core::cmp::Ordering as Cmp;
use core::ptr;
use core::sync::atomic::Ordering;

use crate::sys::errno::Errno;
use crate::sys::mount::{bufpages_deficit, bufpages_inact};
use crate::sys::param::powerof2;
use crate::sys::queue::{TailqEntry, TailqHead};
use crate::sys::tree::{RbtEntry, RbtHead};
use crate::sys::types::Paddr;
use crate::uvm::uvm_extern::{
    UVM_PLA_NOWAIT, UVM_PLA_NOWAKE, UVM_PLA_TRYCONTIG, UVM_PLA_USERESERVE, UVM_PLA_WAITOK,
    UVM_PLA_ZERO,
};
use crate::uvm::uvm_init::{UVM, UVMEXP};
use crate::uvm::uvm_page::{
    PG_PMAPMASK, PG_ZERO, PQ_FREE, Pglist, VmPage, uvm_lock_fpageq, uvm_pageboot_alloc,
    uvm_pagezero, uvm_pagezero_target, uvm_unlock_fpageq, vm_page_to_phys,
};
use crate::uvm::uvm_param::{atop, round_page, trunc_page};
use crate::{kassert, kdassert, kprintf, queue_adapter, tree_adapter, unported};
#[cfg(feature = "multiprocessor")]
use crate::{
    machine::cpu::{MAXCPUS, cpu_number},
    machine::intr::{IPL_BIO, splassert, splbio, splx},
    uvm::uvm_percpu::{UVM_PMR_CACHEMAGSZ, UvmPmrCache, UvmPmrCacheItem},
};

/// Dirty memory.
pub const UVM_PMR_MEMTYPE_DIRTY: usize = 0;
/// Zeroed memory.
pub const UVM_PMR_MEMTYPE_ZERO: usize = 1;
/// Number of memory types.
pub const UVM_PMR_MEMTYPE_MAX: usize = 2;

tree_adapter!(
    /// `uvm_pmr_addr`: free ranges by address (`vm_page[0].objt`).
    pub UvmPmrAddr: VmPage, objt => RbtEntry, uvm_pmr_addr_cmp
);
tree_adapter!(
    /// `uvm_pmr_size`: free ranges by size (`vm_page[1].objt`).
    pub UvmPmrSize: VmPage, objt => RbtEntry, uvm_pmr_size_cmp
);
tree_adapter!(
    /// `uvm_pmemrange_addr`: the ranges by address.
    pub UvmPmemrangeAddr: UvmPmemrange, pmr_addr => RbtEntry, uvm_pmemrange_addr_cmp
);
queue_adapter!(
    /// `uvm_pmemrange_use`: the ranges by use count.
    pub PmrUse: UvmPmemrange, pmr_use => TailqEntry<UvmPmemrange>
);

/// `struct uvm_pmemrange`: a range of physical memory with a use count and its free chunks.
pub struct UvmPmemrange {
    /// Free page chunks, sorted by addr.
    pub addr: RbtHead<UvmPmrAddr>,
    /// Free page chunks, sorted by size.
    pub size: [RbtHead<UvmPmrSize>; UVM_PMR_MEMTYPE_MAX],
    /// Single page regions (uses pageq).
    pub single: [Pglist; UVM_PMR_MEMTYPE_MAX],
    /// Start of address range (pgno).
    pub low: Cell<usize>,
    /// End +1 (pgno).
    pub high: Cell<usize>,
    /// Use counter.
    pub r#use: Cell<i32>,
    /// Current range count.
    pub nsegs: Cell<usize>,
    /// pmr, sorted by use.
    pub pmr_use: TailqEntry<UvmPmemrange>,
    /// pmr, sorted by address.
    pub pmr_addr: RbtEntry,
}

// SAFETY: every field is guarded by `uvm.fpageqlock`.
unsafe impl Sync for UvmPmemrange {}

impl UvmPmemrange {
    /// An empty range (what `uvm_pmr_allocpmr` builds).
    pub const fn new() -> Self {
        Self {
            addr: RbtHead::new(),
            size: [RbtHead::new(), RbtHead::new()],
            single: [Pglist::new(), Pglist::new()],
            low: Cell::new(0),
            high: Cell::new(0),
            r#use: Cell::new(0),
            nsegs: Cell::new(0),
            pmr_use: TailqEntry::new(),
            pmr_addr: RbtEntry::new(),
        }
    }
}

impl Default for UvmPmemrange {
    fn default() -> Self {
        Self::new()
    }
}

/// `struct uvm_pmr_control`: all the ranges, by address and by use.
pub struct UvmPmrControl {
    /// The ranges, sorted by address.
    pub addr: RbtHead<UvmPmemrangeAddr>,
    /// The ranges, sorted by use.
    pub r#use: TailqHead<PmrUse>,
}

// SAFETY: guarded by `uvm.fpageqlock`.
unsafe impl Sync for UvmPmrControl {}

impl UvmPmrControl {
    /// No ranges.
    pub const fn new() -> Self {
        Self {
            addr: RbtHead::new(),
            r#use: TailqHead::new(),
        }
    }
}

impl Default for UvmPmrControl {
    fn default() -> Self {
        Self::new()
    }
}

/// Every CPU's page cache, by `cpu_number()`: `ci_uvm` (see the module's deviations).
#[cfg(feature = "multiprocessor")]
static UVM_PMR_CACHES: [UvmPmrCache; MAXCPUS as usize] =
    [const { UvmPmrCache::new() }; MAXCPUS as usize];

/// Validate the flags of the page (used in asserts). Any free page must have the PQ_FREE flag
/// set. Free pages may be zeroed. Pmap flags are left untouched. The PQ_FREE flag is not checked
/// here: by not checking, we can easily use this check in pages which are freed.
fn valid_flags(pg_flags: u32) -> bool {
    pg_flags & !(PQ_FREE | PG_ZERO | PG_PMAPMASK) == 0
}

/// `in_pagedaemon`: whether the caller is the page daemon (or the syncer, when allowed).
fn in_pagedaemon(_allowsyncer: bool) -> bool {
    // curcpu()->ci_idepth, uvm.pagedaemon_proc and syncerproc arrive with M5.
    false
}

/// `uvm_pmr_pg_to_memtype`: memory types. The page flags are used to derive what the current
/// memory type of a page is.
pub fn uvm_pmr_pg_to_memtype(pg: &VmPage) -> usize {
    if pg.flags() & PG_ZERO != 0 {
        return UVM_PMR_MEMTYPE_ZERO;
    }
    // Default: dirty memory.
    UVM_PMR_MEMTYPE_DIRTY
}

/// Page number of a page.
fn pgno(pg: &VmPage) -> usize {
    vm_page_to_phys(pg).atop()
}

/// `pow2divide`: computes num/denom and rounds it up to the next power-of-2.
///
/// This is a division function which calculates an approximation of num/denom, with result =~
/// num/denom. It is meant to be fast and doesn't have to be accurate.
///
/// Providing too large a value makes the allocator slightly faster, at the risk of hitting the
/// failure case more often. Providing too small a value makes the allocator a bit slower, but
/// less likely to hit a failure case.
pub fn pow2divide(num: usize, denom: usize) -> usize {
    let mut rshift = 0;
    let mut denom = denom;
    while num > denom {
        rshift += 1;
        denom <<= 1;
    }
    1 << rshift
}

/// `PMR_IS_SUBRANGE_OF`: lhs is a subrange of rhs. If rhs_low == 0: don't care about lower
/// bound. If rhs_high == 0: don't care about upper bound.
fn pmr_is_subrange_of(lhs_low: usize, lhs_high: usize, rhs_low: usize, rhs_high: usize) -> bool {
    (rhs_low == 0 || lhs_low >= rhs_low) && (rhs_high == 0 || lhs_high <= rhs_high)
}

/// `PMR_INTERSECTS_WITH`: lhs intersects with rhs. If rhs_low == 0: don't care about lower
/// bound. If rhs_high == 0: don't care about upper bound. Ranges don't intersect if they don't
/// have any page in common, array semantics mean that < instead of <= should be used here.
fn pmr_intersects_with(lhs_low: usize, lhs_high: usize, rhs_low: usize, rhs_high: usize) -> bool {
    (rhs_low == 0 || rhs_low < lhs_high) && (rhs_high == 0 || lhs_low < rhs_high)
}

/// `PMR_ALIGN`: align to power-of-2 alignment.
fn pmr_align(pgno: usize, align: usize) -> usize {
    (pgno + (align - 1)) & !(align - 1)
}

/// `PMR_ALIGN_DOWN`.
fn pmr_align_down(pgno: usize, align: usize) -> usize {
    pgno & !(align - 1)
}

/// `uvm_pmemrange_addr_cmp`: sort by address ascending.
pub fn uvm_pmemrange_addr_cmp(lhs: &UvmPmemrange, rhs: &UvmPmemrange) -> Cmp {
    lhs.low.get().cmp(&rhs.low.get())
}

/// `uvm_pmemrange_use_cmp`: sort by use ascending. The higher the use value of a range, the
/// more devices need memory in this range. Therefore allocate from the range with the lowest use
/// first.
pub fn uvm_pmemrange_use_cmp(lhs: &UvmPmemrange, rhs: &UvmPmemrange) -> Cmp {
    lhs.r#use
        .get()
        .cmp(&rhs.r#use.get())
        .then_with(|| uvm_pmemrange_addr_cmp(lhs, rhs))
}

/// `uvm_pmr_addr_cmp`: pages by physical address.
pub fn uvm_pmr_addr_cmp(lhs: &VmPage, rhs: &VmPage) -> Cmp {
    vm_page_to_phys(lhs)
        .as_usize()
        .cmp(&vm_page_to_phys(rhs).as_usize())
}

/// `uvm_pmr_size_cmp`: ranges by size, then address. Using second tree, so we receive `pg[1]`
/// instead of `pg[0]`.
pub fn uvm_pmr_size_cmp(lhs: &VmPage, rhs: &VmPage) -> Cmp {
    // SAFETY: size-tree nodes are the second page of a range of two or more
    // (`uvm_pmr_insert_size`), so the page before each exists in the same array.
    let (lhs0, rhs0) = unsafe { (lhs.sub(1), rhs.sub(1)) };
    lhs0.fpgsz
        .get()
        .cmp(&rhs0.fpgsz.get())
        .then_with(|| uvm_pmr_addr_cmp(lhs0, rhs0))
}

/// `uvm_pmr_nfindsz`: find the first range of free pages that is at least sz pages long.
pub fn uvm_pmr_nfindsz(
    pmr: &'static UvmPmemrange,
    sz: usize,
    mti: usize,
) -> Option<&'static VmPage> {
    kassert!(sz >= 1);

    if sz == 1 && !pmr.single[mti].is_empty() {
        return pmr.single[mti].first().map(VmPage::forever);
    }

    let mut node = pmr.size[mti].root();
    let mut best = None;
    while let Some(n) = node {
        // SAFETY: a size-tree node is the second page of its range.
        let range = unsafe { n.sub(1) };
        if range.fpgsz.get() >= sz {
            best = Some(range.forever());
            node = RbtHead::<UvmPmrSize>::left(n);
        } else {
            node = RbtHead::<UvmPmrSize>::right(n);
        }
    }
    best
}

/// `uvm_pmr_nextsz`: finds the next range. The next range has a size >= pg->fpgsz. Returns
/// `None` if no more ranges are available.
pub fn uvm_pmr_nextsz(
    pmr: &'static UvmPmemrange,
    pg: &'static VmPage,
    mt: usize,
) -> Option<&'static VmPage> {
    let npg = if pg.fpgsz.get() == 1 {
        match Pglist::next(pg) {
            Some(next) => return Some(next.forever()),
            None => pmr.size[mt].min(),
        }
    } else {
        // SAFETY: a range of two or more pages has its second page in the same array.
        RbtHead::<UvmPmrSize>::next(unsafe { pg.add(1) })
    };

    // SAFETY: a size-tree node is the second page of its range.
    npg.map(|n| unsafe { n.sub(1) }.forever())
}

/// `uvm_pmr_pnaddr`: finds the previous and next ranges relative to the (uninserted) pg range.
/// `prev` is `None` if no previous range is available that can join with pg; `next` likewise.
pub fn uvm_pmr_pnaddr(
    pmr: &'static UvmPmemrange,
    pg: &'static VmPage,
) -> (Option<&'static VmPage>, Option<&'static VmPage>) {
    let mut pg_next = pmr.addr.nfind(pg).map(VmPage::forever);
    let mut pg_prev = match pg_next {
        None => pmr.addr.max().map(VmPage::forever),
        Some(next) => RbtHead::<UvmPmrAddr>::prev(next).map(VmPage::forever),
    };

    kdassert!(
        pg_next.is_none_or(|n| vm_page_to_phys(n).as_usize() > vm_page_to_phys(pg).as_usize())
    );
    kdassert!(
        pg_prev.is_none_or(|p| vm_page_to_phys(p).as_usize() < vm_page_to_phys(pg).as_usize())
    );

    // Reset if not contig.
    if let Some(prev) = pg_prev
        && (pgno(prev) + prev.fpgsz.get() != pgno(pg)
            || prev.ptr_add(prev.fpgsz.get()) != ptr::from_ref(pg) // Array broke.
            || uvm_pmr_pg_to_memtype(prev) != uvm_pmr_pg_to_memtype(pg))
    {
        pg_prev = None;
    }
    if let Some(next) = pg_next
        && (pgno(pg) + pg.fpgsz.get() != pgno(next)
            || pg.ptr_add(pg.fpgsz.get()) != ptr::from_ref(next) // Array broke.
            || uvm_pmr_pg_to_memtype(next) != uvm_pmr_pg_to_memtype(pg))
    {
        pg_next = None;
    }
    (pg_prev, pg_next)
}

/// `uvm_pmr_remove_addr`: remove a range from the address tree. Address tree maintains pmr
/// counters.
pub fn uvm_pmr_remove_addr(pmr: &UvmPmemrange, pg: &VmPage) {
    kdassert!(pmr.addr.find(pg).is_some_and(|f| VmPage::same(f, pg)));
    kdassert!(pg.flags() & PQ_FREE != 0);
    // SAFETY: the range is in this address tree (the assertion above is the C's).
    unsafe { pmr.addr.remove(pg) };

    pmr.nsegs.set(pmr.nsegs.get() - 1);
}

/// `uvm_pmr_remove_size`: remove a range from the size tree.
pub fn uvm_pmr_remove_size(pmr: &UvmPmemrange, pg: &VmPage) {
    kdassert!(pg.fpgsz.get() >= 1);
    kdassert!(pg.flags() & PQ_FREE != 0);
    let memtype = uvm_pmr_pg_to_memtype(pg);

    if pg.fpgsz.get() == 1 {
        #[cfg(feature = "debug")]
        kdassert!(pmr.single[memtype].iter().any(|i| VmPage::same(i, pg)));
        // SAFETY: a one-page range is on its memtype's single queue (checked under DEBUG).
        unsafe { pmr.single[memtype].remove(pg) };
    } else {
        // SAFETY: a longer range has its second page in the same array, and that page is the
        // node in the size tree.
        let node = unsafe { pg.add(1) };
        kdassert!(
            pmr.size[memtype]
                .find(node)
                .is_some_and(|f| VmPage::same(f, node))
        );
        // SAFETY: the node is in this size tree (the assertion above is the C's).
        unsafe { pmr.size[memtype].remove(node) };
    }
}

/// `uvm_pmr_remove`: remove from both trees.
pub fn uvm_pmr_remove(pmr: &UvmPmemrange, pg: &VmPage) {
    uvm_pmr_assertvalid(pmr);
    uvm_pmr_remove_size(pmr, pg);
    uvm_pmr_remove_addr(pmr, pg);
    uvm_pmr_assertvalid(pmr);
}

/// `uvm_pmr_insert_addr`: insert the range described in pg. Returns the range thus created
/// (which may be joined with the previous and next ranges). If `no_join`, the caller guarantees
/// that the range cannot possibly join with adjacent ranges.
pub fn uvm_pmr_insert_addr(
    pmr: &'static UvmPmemrange,
    pg: &'static VmPage,
    no_join: bool,
) -> &'static VmPage {
    kdassert!(pg.flags() & PQ_FREE != 0);
    kdassert!(pg.fpgsz.get() >= 1);

    #[cfg(feature = "debug")]
    for mt in 0..UVM_PMR_MEMTYPE_MAX {
        kdassert!(!pmr.single[mt].iter().any(|i| VmPage::same(i, pg)));
        if pg.fpgsz.get() > 1 {
            // SAFETY: a longer range has its second page in the same array.
            kdassert!(pmr.size[mt].find(unsafe { pg.add(1) }).is_none());
        }
        kdassert!(pmr.addr.find(pg).is_none());
    }

    if !no_join {
        let (prev, next) = uvm_pmr_pnaddr(pmr, pg);
        if let Some(next) = next {
            uvm_pmr_remove_size(pmr, next);
            uvm_pmr_remove_addr(pmr, next);
            pg.fpgsz.set(pg.fpgsz.get() + next.fpgsz.get());
            next.fpgsz.set(0);
        }
        if let Some(prev) = prev {
            uvm_pmr_remove_size(pmr, prev);
            prev.fpgsz.set(prev.fpgsz.get() + pg.fpgsz.get());
            pg.fpgsz.set(0);
            return prev;
        }
    }

    // SAFETY: the range is in no tree (the DEBUG assertions above are the C's).
    unsafe { pmr.addr.insert(pg) };

    pmr.nsegs.set(pmr.nsegs.get() + 1);

    pg
}

/// `uvm_pmr_insert_size`: insert the range described in pg into the size tree. Page must
/// already be in the address tree.
pub fn uvm_pmr_insert_size(pmr: &UvmPmemrange, pg: &VmPage) {
    kdassert!(pg.fpgsz.get() >= 1);
    kdassert!(pg.flags() & PQ_FREE != 0);

    let memtype = uvm_pmr_pg_to_memtype(pg);
    #[cfg(feature = "debug")]
    {
        for mti in 0..UVM_PMR_MEMTYPE_MAX {
            kdassert!(!pmr.single[mti].iter().any(|i| VmPage::same(i, pg)));
            if pg.fpgsz.get() > 1 {
                // SAFETY: a longer range has its second page in the same array.
                kdassert!(pmr.size[mti].find(unsafe { pg.add(1) }).is_none());
            }
            kdassert!(pmr.addr.find(pg).is_some_and(|f| VmPage::same(f, pg)));
        }
        for i in 0..pg.fpgsz.get() {
            // SAFETY: inside the range.
            kassert!(uvm_pmr_pg_to_memtype(unsafe { pg.add(i) }) == memtype);
        }
    }

    if pg.fpgsz.get() == 1 {
        // SAFETY: a range is on no queue when it is inserted (the assertions above).
        unsafe { pmr.single[memtype].insert_tail(pg) };
    } else {
        // SAFETY: the second page of the range is in the same array and in no tree.
        unsafe { pmr.size[memtype].insert(pg.add(1)) };
    }
}

/// `uvm_pmr_insert`: insert in both trees.
pub fn uvm_pmr_insert(
    pmr: &'static UvmPmemrange,
    pg: &'static VmPage,
    no_join: bool,
) -> &'static VmPage {
    uvm_pmr_assertvalid(pmr);
    let pg = uvm_pmr_insert_addr(pmr, pg, no_join);
    uvm_pmr_insert_size(pmr, pg);
    uvm_pmr_assertvalid(pmr);
    pg
}

/// `uvm_pmr_findnextsegment`: find the last page that is part of this segment.
///
/// - `pg`: the range at which to start the search.
/// - `boundary`: the page number boundary specification (0 = no boundary).
/// - `pmr`: the pmemrange of the page.
///
/// This function returns 1 before the next range, so if you want to have the next range, you
/// need to run `TAILQ_NEXT(result, pageq)` after calling. The reason is that this way, the
/// length of the segment is easily calculated using: `atop(result) - atop(pg) + 1`. Hence this
/// function also never returns NULL.
pub fn uvm_pmr_findnextsegment(
    pmr: &UvmPmemrange,
    pg: &'static VmPage,
    boundary: usize,
) -> &'static VmPage {
    kdassert!(pmr.low.get() <= pgno(pg) && pmr.high.get() > pgno(pg));
    let first_boundary = if boundary != 0 {
        pmr_align(pgno(pg) + 1, boundary)
    } else {
        0
    };

    // Increase next until it hits the first page of the next segment.
    //
    // While loop checks the following:
    // - next != NULL: we have not reached the end of pgl
    // - boundary == 0 || next < first_boundary: we do not cross a boundary
    // - atop(prev) + 1 == atop(next): still in the same segment
    // - low <= last, high > last: still in the same memory range
    // - memtype is equal: allocator is unable to view different memtypes as part of the same
    //   segment
    // - prev + 1 == next: no array breakage occurs
    let mut prev = pg;
    let mut next = Pglist::next(prev);
    while let Some(n) = next
        && (boundary == 0 || pgno(n) < first_boundary)
        && pgno(prev) + 1 == pgno(n)
        && pmr.low.get() <= pgno(n)
        && pmr.high.get() > pgno(n)
        && uvm_pmr_pg_to_memtype(prev) == uvm_pmr_pg_to_memtype(n)
        && prev.ptr_add(1) == ptr::from_ref(n)
    {
        prev = n.forever();
        next = Pglist::next(prev);
    }

    // End of this segment.
    prev
}

/// `uvm_pmr_findprevsegment`: find the first page that is part of this segment.
///
/// This function returns 1 after the previous range, so if you want to have the previous
/// range, you need to run `TAILQ_NEXT(result, pageq)` after calling. The reason is that this
/// way, the length of the segment is easily calculated using: `atop(pg) - atop(result) + 1`.
/// Hence this function also never returns NULL.
pub fn uvm_pmr_findprevsegment(
    pmr: &UvmPmemrange,
    pg: &'static VmPage,
    boundary: usize,
) -> &'static VmPage {
    kdassert!(pmr.low.get() <= pgno(pg) && pmr.high.get() > pgno(pg));
    let first_boundary = if boundary != 0 {
        pmr_align_down(pgno(pg), boundary)
    } else {
        0
    };

    // Increase next until it hits the first page of the previous segment (the checks mirror
    // uvm_pmr_findnextsegment's, walking down).
    let mut prev = pg;
    let mut next = Pglist::next(prev);
    while let Some(n) = next
        && (boundary == 0 || pgno(n) >= first_boundary)
        && pgno(prev).wrapping_sub(1) == pgno(n)
        && pmr.low.get() <= pgno(n)
        && pmr.high.get() > pgno(n)
        && uvm_pmr_pg_to_memtype(prev) == uvm_pmr_pg_to_memtype(n)
        && prev.ptr_sub(1) == ptr::from_ref(n)
    {
        prev = n.forever();
        next = Pglist::next(prev);
    }

    // Start of this segment.
    prev
}

/// `uvm_pmr_remove_1strange`: remove the first segment of contiguous pages from pgl. A segment
/// ends if it crosses boundary (unless boundary = 0) or if it would enter a different
/// uvm_pmemrange.
///
/// `work`: the page range that the caller is currently working with. May be `None`.
///
/// If `is_desperate`, the smallest segment is erased. Otherwise, the first segment is erased
/// (which, if called by `uvm_pmr_getpages`, probably is the smallest or very close to it).
/// Returns the number of pages removed.
pub fn uvm_pmr_remove_1strange(
    pgl: &Pglist,
    boundary: usize,
    work: Option<&mut Option<&'static VmPage>>,
    is_desperate: bool,
) -> usize {
    kassert!(!pgl.is_empty());

    // Initialize to first page. Unless desperate scan finds a better candidate, this is what'll
    // be erased.
    let Some(first) = pgl.first() else {
        return 0;
    };
    let mut start = first.forever();
    let Some(mut pmr) = uvm_pmemrange_find(pgno(start)) else {
        return 0;
    };
    let mut end = uvm_pmr_findnextsegment(pmr, start, boundary);

    // If we are desperate, we _really_ want to get rid of the smallest element (rather than a
    // close match to the smallest element).
    if is_desperate {
        // Linear search for smallest segment.
        let mut pmr_iter = pmr;
        let mut iter = Pglist::next(end).map(VmPage::forever);
        while let Some(it) = iter
            && !VmPage::same(start, end)
        {
            // Only update pmr if it doesn't match current iteration.
            if pmr.low.get() > pgno(it) || pmr.high.get() <= pgno(it) {
                let Some(found) = uvm_pmemrange_find(pgno(it)) else {
                    break;
                };
                pmr_iter = found;
            }

            let iter_end = uvm_pmr_findnextsegment(pmr_iter, it, boundary);

            // Current iteration is smaller than best match so far; update.
            if vm_page_to_phys(iter_end).as_usize() - vm_page_to_phys(it).as_usize()
                < vm_page_to_phys(end).as_usize() - vm_page_to_phys(start).as_usize()
            {
                start = it;
                end = iter_end;
                pmr = pmr_iter;
            }
            iter = Pglist::next(iter_end).map(VmPage::forever);
        }
    }

    // Calculate count and end of the list.
    let count = atop(vm_page_to_phys(end).as_usize() - vm_page_to_phys(start).as_usize()) + 1;
    let lowest = start;
    let end = Pglist::next(end).map(ptr::from_ref);

    // Actually remove the range of pages.
    //
    // Sadly, this cannot be done using pointer iteration: vm_physseg is not guaranteed to be
    // sorted on address, hence uvm_page_init() may not have initialized its array sorted by
    // page number.
    let mut iter: Option<&'static VmPage> = Some(start);
    while let Some(it) = iter
        && end != Some(ptr::from_ref(it))
    {
        let iter_end = Pglist::next(it).map(VmPage::forever);
        // SAFETY: `it` is on `pgl`: it was reached by walking the list from its head.
        unsafe { pgl.remove(it) };
        iter = iter_end;
    }

    lowest.fpgsz.set(count);
    let inserted = uvm_pmr_insert(pmr, lowest, false);

    // If the caller was working on a range and this function modified that range, update the
    // pointer.
    if let Some(work) = work
        && let Some(w) = *work
        && pgno(inserted) <= pgno(w)
        && pgno(inserted) + inserted.fpgsz.get() > pgno(w)
    {
        *work = Some(inserted);
    }
    count
}

/// `uvm_pmr_remove_1strange_reverse`: remove the first segment of contiguous pages from a pgl
/// with the list elements in reverse order of physaddr. A segment ends if it would enter a
/// different uvm_pmemrange. Returns the count and the starting physical address of the segment.
pub fn uvm_pmr_remove_1strange_reverse(pgl: &Pglist) -> (usize, Paddr) {
    kassert!(!pgl.is_empty());

    let Some(first) = pgl.first() else {
        return (0, Paddr::new(0));
    };
    let start = first.forever();
    let Some(pmr) = uvm_pmemrange_find(pgno(start)) else {
        return (0, Paddr::new(0));
    };
    let end = uvm_pmr_findprevsegment(pmr, start, 0);

    kassert!(ptr::from_ref(end) <= ptr::from_ref(start));

    // Calculate count and end of the list.
    let count = atop(vm_page_to_phys(start).as_usize() - vm_page_to_phys(end).as_usize()) + 1;
    let lowest = end;
    let end = Pglist::next(end).map(ptr::from_ref);

    // Actually remove the range of pages (see uvm_pmr_remove_1strange for why it walks the
    // list).
    let mut iter: Option<&'static VmPage> = Some(start);
    while let Some(it) = iter
        && end != Some(ptr::from_ref(it))
    {
        let iter_end = Pglist::next(it).map(VmPage::forever);
        // SAFETY: `it` is on `pgl`: it was reached by walking the list from its head.
        unsafe { pgl.remove(it) };
        iter = iter_end;
    }

    lowest.fpgsz.set(count);
    let _ = uvm_pmr_insert(pmr, lowest, false);

    (count, vm_page_to_phys(lowest))
}

/// `uvm_pmr_extract_range`: extract a number of pages from a segment of free pages. Called by
/// `uvm_pmr_getpages`. Returns the segment that was created from pages left over at the tail of
/// the remove set of pages, or `None` if no pages were left at the tail.
pub fn uvm_pmr_extract_range(
    pmr: &'static UvmPmemrange,
    pg: &'static VmPage,
    start: usize,
    end: usize,
    result: &Pglist,
) -> Option<&'static VmPage> {
    kdassert!(end > start);
    kdassert!(pmr.low.get() <= pgno(pg));
    kdassert!(pmr.high.get() >= pgno(pg) + pg.fpgsz.get());
    kdassert!(pgno(pg) <= start);
    kdassert!(pgno(pg) + pg.fpgsz.get() >= end);

    let before_sz = start - pgno(pg);
    let after_sz = pgno(pg) + pg.fpgsz.get() - end;
    kdassert!(before_sz + after_sz + (end - start) == pg.fpgsz.get());
    uvm_pmr_assertvalid(pmr);

    uvm_pmr_remove_size(pmr, pg);
    if before_sz == 0 {
        uvm_pmr_remove_addr(pmr, pg);
    }

    // Add selected pages to result.
    for i in before_sz..before_sz + (end - start) {
        // SAFETY: inside the range (`before_sz + (end - start) <= fpgsz`).
        let pg_i = unsafe { pg.add(i) };
        kassert!(pg_i.flags() & PQ_FREE != 0);
        pg_i.fpgsz.set(0);
        // SAFETY: a page inside a free range is on no list.
        unsafe { result.insert_tail(pg_i) };
    }

    // Before handling.
    if before_sz > 0 {
        pg.fpgsz.set(before_sz);
        uvm_pmr_insert_size(pmr, pg);
    }

    // After handling.
    if after_sz > 0 {
        // SAFETY: `before_sz + (end - start) < fpgsz` when `after_sz > 0`.
        let after = unsafe { pg.add(before_sz + (end - start)) }.forever();
        #[cfg(feature = "debug")]
        for i in 0..after_sz {
            // SAFETY: inside the range.
            kassert!(!uvm_pmr_isfree(unsafe { after.add(i) }));
        }
        kdassert!(pgno(after) == end);
        after.fpgsz.set(after_sz);
        let after = uvm_pmr_insert_addr(pmr, after, true);
        uvm_pmr_insert_size(pmr, after);
        uvm_pmr_assertvalid(pmr);
        return Some(after);
    }

    uvm_pmr_assertvalid(pmr);
    None
}

/// `uvm_pmr_getpages`: acquire a number of pages.
///
/// - `count`: the number of pages returned
/// - `start`: lowest page number
/// - `end`: highest page number +1 (start = end = 0: no limitation)
/// - `align`: power-of-2 alignment constraint (align = 1: no alignment)
/// - `boundary`: power-of-2 boundary (boundary = 0: no boundary)
/// - `maxseg`: maximum number of segments to return
/// - `flags`: `UVM_PLA_*` flags
/// - `result`: returned pages storage (uses pageq)
#[allow(clippy::too_many_arguments)] // the C signature
pub fn uvm_pmr_getpages(
    count: usize,
    start: usize,
    end: usize,
    align: usize,
    boundary: usize,
    maxseg: i32,
    flags: i32,
    result: &Pglist,
) -> Result<(), Errno> {
    // Validate arguments.
    kassert!(count > 0);
    kassert!(start == 0 || end == 0 || start < end);
    kassert!(align >= 1);
    kassert!(powerof2(align));
    kassert!(maxseg > 0);
    kassert!(boundary == 0 || powerof2(boundary));
    kassert!(boundary == 0 || (maxseg as usize) * boundary >= count);
    kassert!(result.is_empty());
    kassert!((flags & UVM_PLA_WAITOK == 0) ^ (flags & UVM_PLA_NOWAIT == 0));

    let mut flags = flags;
    let maxseg = maxseg as usize;

    // TRYCONTIG is a noop if you only want a single segment. Remove it if that's the case:
    // otherwise it'll deny the fast allocation.
    if maxseg == 1 || count == 1 {
        flags &= !UVM_PLA_TRYCONTIG;
    }

    // Configure search.
    //
    // search[0] is one segment, only used in UVM_PLA_TRYCONTIG case.
    // search[1] is multiple segments, chosen to fulfill the search in approximately even-sized
    //   segments. This is a good trade-off between slightly reduced allocation speed and less
    //   fragmentation.
    // search[2] is the worst case, in which all segments are evaluated. This provides the least
    //   fragmentation, but makes the search possibly longer (although in the case it is
    //   selected, that no longer matters most).
    //
    // The exception is when maxseg == 1: since we can only fulfill that with one segment of
    // size pages, only a single search type has to be attempted.
    let mut search = [0usize; 3];
    let mut start_try;
    if maxseg == 1 || count == 1 {
        start_try = 2;
        search[2] = count;
    } else if maxseg >= count && flags & UVM_PLA_TRYCONTIG == 0 {
        start_try = 2;
        search[2] = 1;
    } else {
        start_try = 0;
        search[0] = count;
        search[1] = pow2divide(count, maxseg);
        search[2] = 1;
        if flags & UVM_PLA_TRYCONTIG == 0 {
            start_try = 1;
        }
        if search[1] >= search[0] {
            search[1] = search[0];
            start_try = 1;
        }
        if search[2] >= search[start_try] {
            start_try = 2;
        }
    }

    // Memory type: if zeroed memory is requested, traverse the zero set. Otherwise, traverse
    // the dirty set. The memtype iterator is reinitialized to memtype_init on entrance of a
    // pmemrange.
    let memtype_init = if flags & UVM_PLA_ZERO != 0 {
        UVM_PMR_MEMTYPE_ZERO
    } else {
        UVM_PMR_MEMTYPE_DIRTY
    };

    // Initially, we're not desperate. Note that if we return from a sleep, we are still
    // desperate. Chances are that memory pressure is still high, so resetting seems
    // over-optimistic to me.
    let mut desperate = false;

    uvm_lock_fpageq();
    // retry: the return point after sleeping is not reachable yet (uvm_wait is unported).

    // check to see if we need to generate some free pages waking the pagedaemon.
    let free = i64::from(UVMEXP.free.load(Ordering::Relaxed));
    let deficit = bufpages_deficit();
    if (free - deficit) < i64::from(UVMEXP.freemin.load(Ordering::Relaxed))
        || ((free - deficit) < i64::from(UVMEXP.freetarg.load(Ordering::Relaxed))
            && (i64::from(UVMEXP.inactive.load(Ordering::Relaxed)) + bufpages_inact())
                < i64::from(UVMEXP.inactarg.load(Ordering::Relaxed)))
    {
        crate::kern::kern_synch::wakeup(ptr::from_ref(&UVM));
    }

    // fail if any of these conditions is true:
    // [1] there really are no free pages, or
    // [2] only kernel "reserved" pages remain and the UVM_PLA_USERESERVE flag wasn't used.
    // [3] only pagedaemon "reserved" pages remain and the requestor isn't the pagedaemon nor
    //     the syncer.
    let free = UVMEXP.free.load(Ordering::Relaxed) as i64;
    if free <= i64::from(UVMEXP.reserve_kernel.load(Ordering::Relaxed)) + count as i64
        && flags & UVM_PLA_USERESERVE == 0
    {
        uvm_unlock_fpageq();
        return Err(Errno::ENOMEM);
    }

    if free <= i64::from(UVMEXP.reserve_pagedaemon.load(Ordering::Relaxed)) + count as i64
        && !in_pagedaemon(true)
    {
        uvm_unlock_fpageq();
        if flags & UVM_PLA_WAITOK != 0 {
            crate::uvm::uvm_pdaemon::uvm_wait("uvm_pmr_getpages");
            // goto retry: without a page daemon the wait cannot be satisfied.
        }
        return Err(Errno::ENOMEM);
    }

    let mut fcount = 0usize;
    let mut fnsegs = 0usize;

    // retry_desperate / fail / out: the labelled block ends with the C's exit taken.
    let found_all = 'retry_desperate: loop {
        // If we just want any page(s), go for the really fast option.
        if count <= maxseg && align == 1 && boundary == 0 && flags & UVM_PLA_TRYCONTIG == 0 {
            fcount += uvm_pmr_get1page(count - fcount, memtype_init, result, start, end, false);

            // If we found sufficient pages, go to the success exit code. Otherwise, go
            // immediately to fail, since we collected all we could anyway.
            break 'retry_desperate fcount == count;
        }

        // The heart of the contig case.
        //
        // foreach (struct pmemrange) {
        //	foreach (memtype) {
        //		foreach(try) {
        //			foreach (free range of memtype in pmemrange, starting at
        //			    search[try]) {
        //				while (range has space left)
        //					take from range
        //			}
        //		}
        //	}
        //	if next pmemrange has higher usecount than current:
        //		enter desperate case (which will drain the pmemranges until empty
        //		prior to moving to the next one)
        // }
        //
        // When desperate is activated, try always starts at the highest value.
        let mut pmr_iter = UVM.pmr_control.r#use.first();
        'pmr: while let Some(pmr) = pmr_iter {
            let pmr: &'static UvmPmemrange = pmr_static(pmr);
            pmr_iter = TailqHead::<PmrUse>::next(pmr);

            // Empty range.
            if pmr.nsegs.get() == 0 {
                continue;
            }

            // Outside requested range.
            if !pmr_intersects_with(pmr.low.get(), pmr.high.get(), start, end) {
                continue;
            }

            let mut memtype = memtype_init;

            'rescan_memtype: loop {
                // Return point at memtype++.
                let mut r#try = start_try;

                'rescan: loop {
                    // Return point at try++.
                    let mut found = uvm_pmr_nfindsz(pmr, search[r#try], memtype);
                    while let Some(mut f) = found {
                        let f_next = uvm_pmr_nextsz(pmr, f, memtype);

                        let mut fstart = pgno(f);
                        if start != 0 {
                            fstart = start.max(fstart);
                        }
                        'drain_found: loop {
                            // Throw away the first segment if fnsegs == maxseg.
                            //
                            // Note that f_next is still valid after this call, since we only
                            // allocated from entries before f_next. We don't revisit the entries
                            // we already extracted from unless we entered the desperate case.
                            if fnsegs == maxseg {
                                fnsegs -= 1;
                                let mut work = Some(f);
                                fcount -= uvm_pmr_remove_1strange(
                                    result,
                                    boundary,
                                    Some(&mut work),
                                    desperate,
                                );
                                if let Some(w) = work {
                                    f = w;
                                }
                            }

                            fstart = pmr_align(fstart, align);
                            let mut fend = pgno(f) + f.fpgsz.get();
                            if end != 0 {
                                fend = end.min(fend);
                            }
                            if boundary != 0 {
                                fend = fend.min(pmr_align(fstart + 1, boundary));
                            }
                            if fstart >= fend {
                                break 'drain_found;
                            }
                            if fend - fstart > count - fcount {
                                fend = fstart + (count - fcount);
                            }

                            fcount += fend - fstart;
                            fnsegs += 1;
                            let after = uvm_pmr_extract_range(pmr, f, fstart, fend, result);

                            if fcount == count {
                                break 'retry_desperate true;
                            }

                            // If there's still space left in found, try to fully drain it prior
                            // to continuing.
                            match after {
                                Some(a) => {
                                    f = a;
                                    fstart = fend;
                                }
                                None => break 'drain_found,
                            }
                        }
                        found = f_next;
                    }

                    // Try a smaller search now.
                    r#try += 1;
                    if r#try < search.len() {
                        continue 'rescan;
                    }
                    break 'rescan;
                }

                // Exhaust all memory types prior to going to the next memory segment. This
                // means that zero-vs-dirty are eaten prior to moving to a pmemrange with a
                // higher use-count.
                memtype += 1;
                if memtype == UVM_PMR_MEMTYPE_MAX {
                    memtype = 0;
                }
                if memtype != memtype_init {
                    continue 'rescan_memtype;
                }
                break 'rescan_memtype;
            }

            // If not desperate, enter desperate case prior to eating all the good stuff in the
            // next range.
            if !desperate
                && let Some(next) = pmr_iter
                && next.r#use.get() != pmr.r#use.get()
            {
                break 'pmr;
            }
        }

        // Not enough memory of the requested type available. Fall back to less good memory
        // that we'll clean up better later.
        //
        // This algorithm is not very smart though, it just starts scanning a different typed
        // range, but the nicer ranges of the previous iteration may fall out. Hence there is a
        // small chance of a false negative.
        //
        // When desperate: scan all sizes starting at the smallest (start_try = 1) and do not
        // consider UVM_PLA_TRYCONTIG (which may allow us to hit the fast path now).
        //
        // Also, because we will revisit entries we scanned before, we need to reset the page
        // queue, or we may end up releasing entries in such a way as to invalidate f_next.
        if !desperate {
            desperate = true;
            start_try = search.len() - 1;
            flags &= !UVM_PLA_TRYCONTIG;

            while !result.is_empty() {
                uvm_pmr_remove_1strange(result, 0, None, false);
            }
            fnsegs = 0;
            fcount = 0;
            continue 'retry_desperate;
        }
        break 'retry_desperate false;
    };

    if !found_all {
        // fail: allocation failed.
        // XXX: claim from memory reserve here

        while !result.is_empty() {
            uvm_pmr_remove_1strange(result, 0, None, false);
        }

        if flags & UVM_PLA_WAITOK != 0 {
            uvm_unlock_fpageq();
            crate::uvm::uvm_pdaemon::uvm_wait("pmrwait");
            // goto retry: without a page daemon the wait cannot be satisfied.
            return Err(Errno::ENOMEM);
        }
        if flags & UVM_PLA_NOWAKE == 0 {
            crate::kern::kern_synch::wakeup(ptr::from_ref(&UVM));
        }
        uvm_unlock_fpageq();

        return Err(Errno::ENOMEM);
    }

    // out: allocation successful.
    UVMEXP.free.fetch_sub(fcount as i32, Ordering::Relaxed);

    uvm_unlock_fpageq();

    // Update statistics and zero pages if UVM_PLA_ZERO.
    #[cfg(feature = "diagnostic")]
    let (mut diag_fnsegs, mut diag_fcount, mut diag_prev): (usize, usize, Option<&VmPage>) =
        (0, 0, None);
    for found in result.iter() {
        found.clear_bits(PG_PMAPMASK);

        if found.flags() & PG_ZERO != 0 {
            uvm_lock_fpageq();
            UVMEXP.zeropages.fetch_sub(1, Ordering::Relaxed);
            if UVMEXP.zeropages.load(Ordering::Relaxed) < uvm_pagezero_target() {
                crate::kern::kern_synch::wakeup(ptr::from_ref(&UVMEXP.zeropages));
            }
            uvm_unlock_fpageq();
        }
        if flags & UVM_PLA_ZERO != 0 {
            if found.flags() & PG_ZERO != 0 {
                UVMEXP.pga_zerohit.fetch_add(1, Ordering::Relaxed);
            } else {
                UVMEXP.pga_zeromiss.fetch_add(1, Ordering::Relaxed);
                uvm_pagezero(found);
            }
        }
        found.clear_bits(PG_ZERO | PQ_FREE);

        kassert!(found.uobject.get().is_null());
        kassert!(found.uanon.get().is_null());
        found.pg_version.set(found.pg_version.get().wrapping_add(1));

        // Validate that the page matches range criterium.
        kdassert!(start == 0 || pgno(found) >= start);
        kdassert!(end == 0 || pgno(found) < end);

        #[cfg(feature = "diagnostic")]
        {
            // Update fcount (# found pages) and fnsegs (# found segments) counters.
            if diag_prev.is_none_or(|prev| {
                // new segment if it contains a hole
                pgno(prev) + 1 != pgno(found)
                    // new segment if it crosses boundary
                    || (pgno(prev) & !boundary.wrapping_sub(1))
                        != (pgno(found) & !boundary.wrapping_sub(1))
            }) {
                diag_fnsegs += 1;
            }
            diag_fcount += 1;
            diag_prev = Some(found);
        }
    }

    #[cfg(feature = "diagnostic")]
    {
        // Panic on algorithm failure.
        if diag_fcount != count || diag_fnsegs > maxseg {
            #[allow(clippy::panic)] // the C panics here too
            {
                panic!(
                    "pmemrange allocation error: allocated {} pages in {} segments, but request was {} pages in {} segments",
                    diag_fcount, diag_fnsegs, count, maxseg
                );
            }
        }
    }

    Ok(())
}

/// A pmemrange reference with the lifetime of the ranges, which are never freed.
fn pmr_static(pmr: &UvmPmemrange) -> &'static UvmPmemrange {
    // SAFETY: pmemranges are allocated by `uvm_pmr_allocpmr` for the kernel's whole life and
    // never freed (a lost one is leaked, as the C says in `uvm_pmr_split`).
    unsafe { &*ptr::from_ref(pmr) }
}

/// `uvm_pmr_getone`: acquire a single page.
pub fn uvm_pmr_getone(flags: i32) -> Option<&'static VmPage> {
    let pgl = Pglist::new();
    pgl.init();
    uvm_pmr_getpages(1, 0, 0, 1, 0, 1, flags, &pgl).ok()?;

    let pg = pgl.first().map(VmPage::forever);
    kassert!(pg.is_some_and(|pg| Pglist::next(pg).is_none()));
    if let Some(pg) = pg {
        // SAFETY: `pg` is the only element of `pgl`; the caller takes it off the list.
        unsafe { pgl.remove(pg) };
    }

    pg
}

/// `uvm_pmr_freepages`: free a number of contig pages (invoked by `uvm_page_init`).
pub fn uvm_pmr_freepages(pg: &VmPage, count: usize) {
    let mut pg = pg.forever();
    for i in 0..count {
        // SAFETY: the caller frees `count` contiguous pages of one array.
        let pgi = unsafe { pg.add(i) };
        kassert!(pgi.uobject.get().is_null());
        kassert!(pgi.uanon.get().is_null());

        kassert!(pgno(pgi) == pgno(pg) + i);

        if !(pgi.flags() & PQ_FREE == 0 && valid_flags(pgi.flags())) {
            kprintf!("Flags: {:#x}, will panic now.\n", pgi.flags());
        }
        kassert!(pgi.flags() & PQ_FREE == 0 && valid_flags(pgi.flags()));
        pgi.set_bits(PQ_FREE);
        pgi.clear_bits(PG_ZERO);
    }

    uvm_lock_fpageq();

    let mut i = count;
    while i > 0 {
        let Some(pmr) = uvm_pmemrange_find(pgno(pg)) else {
            kassert!(false);
            break;
        };

        let pmr_count = i.min(pmr.high.get() - pgno(pg));
        pg.fpgsz.set(pmr_count);
        uvm_pmr_insert(pmr, pg, false);

        UVMEXP.free.fetch_add(pmr_count as i32, Ordering::Relaxed);
        i -= pmr_count;
        if i > 0 {
            // SAFETY: still inside the `count` pages the caller handed over.
            pg = unsafe { pg.add(pmr_count) };
        }
    }
    crate::kern::kern_synch::wakeup(ptr::from_ref(&UVMEXP.free));
    if UVMEXP.zeropages.load(Ordering::Relaxed) < uvm_pagezero_target() {
        crate::kern::kern_synch::wakeup(ptr::from_ref(&UVMEXP.zeropages));
    }

    uvm_unlock_fpageq();
}

/// `uvm_pmr_freepageq`: free all pages in the queue.
pub fn uvm_pmr_freepageq(pgl: &Pglist) {
    for pg in pgl.iter() {
        kassert!(pg.uobject.get().is_null());
        kassert!(pg.uanon.get().is_null());

        if !(pg.flags() & PQ_FREE == 0 && valid_flags(pg.flags())) {
            kprintf!("Flags: {:#x}, will panic now.\n", pg.flags());
        }
        kassert!(pg.flags() & PQ_FREE == 0 && valid_flags(pg.flags()));
        pg.set_bits(PQ_FREE);
        pg.clear_bits(PG_ZERO);
    }

    uvm_lock_fpageq();
    while let Some(pg) = pgl.first() {
        let plen = if Pglist::next(pg).is_some_and(|next| ptr::from_ref(pg) == next.ptr_add(1)) {
            // If pg is one behind the position of the next page in the list in the page array,
            // try going backwards instead of forward.
            uvm_pmr_remove_1strange_reverse(pgl).0
        } else {
            uvm_pmr_remove_1strange(pgl, 0, None, false)
        };
        UVMEXP.free.fetch_add(plen as i32, Ordering::Relaxed);
    }
    crate::kern::kern_synch::wakeup(ptr::from_ref(&UVMEXP.free));
    if UVMEXP.zeropages.load(Ordering::Relaxed) < uvm_pagezero_target() {
        crate::kern::kern_synch::wakeup(ptr::from_ref(&UVMEXP.zeropages));
    }
    uvm_unlock_fpageq();
}

/// `uvm_pmemrange_use_insert`: store a pmemrange in the list. The list is sorted by use.
/// Returns the range already there when one compares equal.
pub fn uvm_pmemrange_use_insert(
    useq: &TailqHead<PmrUse>,
    pmr: &'static UvmPmemrange,
) -> Option<&'static UvmPmemrange> {
    let mut at: Option<&UvmPmemrange> = None;
    for iter in useq.iter() {
        match uvm_pmemrange_use_cmp(pmr, iter) {
            Cmp::Equal => return Some(pmr_static(iter)),
            Cmp::Less => {
                at = Some(iter);
                break;
            }
            Cmp::Greater => {}
        }
    }

    match at {
        // SAFETY: `pmr` is on no use list (the caller took it off or just made it).
        None => unsafe { useq.insert_tail(pmr) },
        // SAFETY: as above; `iter` is on this list.
        Some(iter) => unsafe { TailqHead::<PmrUse>::insert_before(iter, pmr) },
    }
    None
}

/// `uvm_pmr_assertvalid`: validation of the whole pmemrange. Called with fpageq locked.
#[cfg(any(feature = "debug", test))]
pub fn uvm_pmr_assertvalid(pmr: &UvmPmemrange) {
    // Empty range
    if pmr.nsegs.get() == 0 {
        return;
    }

    // Validate address tree.
    for i in pmr.addr.iter() {
        // Validate the range.
        assert!(i.fpgsz.get() > 0);
        assert!(pgno(i) >= pmr.low.get());
        assert!(pgno(i) + i.fpgsz.get() <= pmr.high.get());

        // Validate each page in this range.
        for lcv in 0..i.fpgsz.get() {
            // SAFETY: inside the range.
            let p = unsafe { i.add(lcv) };
            // Only the first page has a size specification. Rest is size 0.
            assert!(lcv == 0 || p.fpgsz.get() == 0);
            // Flag check.
            assert!(valid_flags(p.flags()) && p.flags() & PQ_FREE == PQ_FREE);
            // Free pages are: not wired, have no vm_anon, have no uvm_object.
            assert!(p.wire_count.get() == 0);
            assert!(p.uanon.get().is_null());
            assert!(p.uobject.get().is_null());
            // Pages in a single range always have the same memtype.
            assert!(uvm_pmr_pg_to_memtype(i) == uvm_pmr_pg_to_memtype(p));
        }

        // Check that it shouldn't be joined with its predecessor.
        if let Some(prev) = RbtHead::<UvmPmrAddr>::prev(i) {
            assert!(
                uvm_pmr_pg_to_memtype(i) != uvm_pmr_pg_to_memtype(prev)
                    || pgno(i) > pgno(prev) + prev.fpgsz.get()
                    || prev.ptr_add(prev.fpgsz.get()) != ptr::from_ref(i)
            );
        }

        // Assert i is in the size tree as well.
        if i.fpgsz.get() == 1 {
            assert!(
                pmr.single[uvm_pmr_pg_to_memtype(i)]
                    .iter()
                    .any(|xref| VmPage::same(xref, i))
            );
        } else {
            // SAFETY: a longer range has its second page in the same array.
            let node = unsafe { i.add(1) };
            assert!(
                pmr.size[uvm_pmr_pg_to_memtype(i)]
                    .find(node)
                    .is_some_and(|f| VmPage::same(f, node))
            );
        }
    }

    // Validate size tree.
    let pmr = pmr_static(pmr);
    for mti in 0..UVM_PMR_MEMTYPE_MAX {
        let mut i = uvm_pmr_nfindsz(pmr, 1, mti);
        while let Some(cur) = i {
            let next = uvm_pmr_nextsz(pmr, cur, mti);
            if let Some(n) = next {
                assert!(cur.fpgsz.get() <= n.fpgsz.get());
            }

            // Assert i is in the addr tree as well.
            assert!(pmr.addr.find(cur).is_some_and(|f| VmPage::same(f, cur)));

            // Assert i is of the correct memory type.
            assert!(uvm_pmr_pg_to_memtype(cur) == mti);
            i = next;
        }
    }

    // Validate nsegs statistic.
    assert_eq!(pmr.nsegs.get(), pmr.addr.iter().count());
}

/// `uvm_pmr_assertvalid`: nothing without `DEBUG`.
#[cfg(not(any(feature = "debug", test)))]
pub fn uvm_pmr_assertvalid(_pmr: &UvmPmemrange) {}

/// `uvm_pmr_split`: split pmr at split point pageno. Called with fpageq unlocked. Split is
/// only applied if a pmemrange spans pageno.
pub fn uvm_pmr_split(pageno: usize) {
    uvm_lock_fpageq();
    let needs_split = uvm_pmemrange_find(pageno).is_some_and(|pmr| pmr.low.get() < pageno);
    if !needs_split {
        // No split required.
        uvm_unlock_fpageq();
        return;
    }

    // uvm_pmr_allocpmr() calls into malloc() which in turn calls into uvm_kmemalloc which calls
    // into pmemrange, making the locking a bit hard, so we just race!
    uvm_unlock_fpageq();
    let drain = uvm_pmr_allocpmr();
    uvm_lock_fpageq();
    let Some(pmr) = uvm_pmemrange_find(pageno).filter(|pmr| pmr.low.get() < pageno) else {
        // We lost the race since someone else ran this or a related function, however this
        // should be triggered very rarely so we just leak the pmr.
        kprintf!("uvm_pmr_split: lost one pmr\n");
        uvm_unlock_fpageq();
        return;
    };

    kassert!(pmr.low.get() < pageno);
    kassert!(pmr.high.get() > pageno);

    drain.low.set(pageno);
    drain.high.set(pmr.high.get());
    drain.r#use.set(pmr.r#use.get());

    uvm_pmr_assertvalid(pmr);
    uvm_pmr_assertvalid(drain);
    kassert!(drain.nsegs.get() == 0);

    let mut rebuild = pmr
        .addr
        .iter()
        .find(|r| pgno(r) >= pageno)
        .map(VmPage::forever);
    let prev = match rebuild {
        None => pmr.addr.max().map(VmPage::forever),
        Some(r) => RbtHead::<UvmPmrAddr>::prev(r).map(VmPage::forever),
    };
    kassert!(prev.is_none_or(|p| pgno(p) < pageno));

    // Handle free chunk that spans the split point.
    if let Some(prev) = prev
        && pgno(prev) + prev.fpgsz.get() > pageno
    {
        kassert!(pgno(prev) < pageno);

        uvm_pmr_remove(pmr, prev);
        let prev_sz = prev.fpgsz.get();
        let before = pageno - pgno(prev);
        let after = pgno(prev) + prev_sz - pageno;

        kassert!(before > 0);
        kassert!(after > 0);

        prev.fpgsz.set(before);
        uvm_pmr_insert(pmr, prev, true);
        // SAFETY: `before < prev_sz`, inside the range that was just split.
        let tail = unsafe { prev.add(before) }.forever();
        tail.fpgsz.set(after);
        uvm_pmr_insert(drain, tail, true);
    }

    // Move free chunks that no longer fall in the range.
    while let Some(r) = rebuild {
        let next = RbtHead::<UvmPmrAddr>::next(r).map(VmPage::forever);

        uvm_pmr_remove(pmr, r);
        uvm_pmr_insert(drain, r, true);
        rebuild = next;
    }

    pmr.high.set(pageno);
    uvm_pmr_assertvalid(pmr);
    uvm_pmr_assertvalid(drain);

    // SAFETY: `drain` is a fresh range, in no tree and on no list.
    unsafe { UVM.pmr_control.addr.insert(drain) };
    uvm_pmemrange_use_insert(&UVM.pmr_control.r#use, drain);
    uvm_unlock_fpageq();
}

/// `uvm_pmr_use_inc`: increase the usage counter for the given range of memory. The more usage
/// counters a given range of memory has, the more will be attempted not to allocate from it.
/// Addresses here are in `paddr_t`, not page-numbers. The lowest and highest allowed address are
/// specified.
pub fn uvm_pmr_use_inc(low: Paddr, high: Paddr) {
    // pmr uses page numbers, translate low and high.
    let high = atop(trunc_page(high.as_usize().wrapping_add(1)));
    let low = atop(round_page(low.as_usize()));
    uvm_pmr_split(low);
    uvm_pmr_split(high);

    let mut sz = 0usize;
    uvm_lock_fpageq();
    // Increase use count on segments in range.
    let mut it = UVM.pmr_control.addr.min().map(pmr_static);
    while let Some(pmr) = it {
        it = RbtHead::<UvmPmemrangeAddr>::next(pmr).map(pmr_static);
        if pmr_is_subrange_of(pmr.low.get(), pmr.high.get(), low, high) {
            // SAFETY: every range in the address tree is on the use list.
            unsafe { UVM.pmr_control.r#use.remove(pmr) };
            pmr.r#use.set(pmr.r#use.get() + 1);
            sz += pmr.high.get() - pmr.low.get();
            uvm_pmemrange_use_insert(&UVM.pmr_control.r#use, pmr);
        }
        uvm_pmr_assertvalid(pmr);
    }
    uvm_unlock_fpageq();

    kassert!(sz >= high.wrapping_sub(low));
}

/// `uvm_pmr_allocpmr`: allocate a pmemrange. If called from `uvm_page_init`, the
/// `uvm_pageboot_alloc` is used. If called after `uvm_init`, malloc is used. (And if called in
/// between, you're dead.)
pub fn uvm_pmr_allocpmr() -> &'static UvmPmemrange {
    // We're only ever hitting the !uvm.page_init_done case for now.
    if UVM.page_init_done.load(Ordering::Relaxed) {
        let _ = unported!("malloc (uvm_pmr_allocpmr after uvm_init)");
        #[allow(clippy::panic)] // KASSERT(nw != NULL) in the C
        {
            panic!("uvm_pmr_allocpmr: no allocator after uvm_init yet");
        }
    }
    let nw = uvm_pageboot_alloc(crate::sys::types::Vsize::new(size_of::<UvmPmemrange>())).as_usize()
        as *mut UvmPmemrange;
    // SAFETY: `uvm_pageboot_alloc` returned `size_of::<UvmPmemrange>()` zeroed, page-aligned
    // bytes that live forever; `new()` is what the C's memset + RBT_INIT/TAILQ_INIT produce.
    unsafe {
        ptr::write(nw, UvmPmemrange::new());
        &*nw
    }
}

/// `uvm_pmr_init`: initialization of pmr. Called by `uvm_page_init`. Sets up pmemranges.
pub fn uvm_pmr_init() {
    UVM.pmr_control.r#use.init();
    UVM.pmr_control.addr.init();

    // By default, one range for the entire address space.
    let new_pmr = uvm_pmr_allocpmr();
    new_pmr.low.set(0);
    new_pmr.high.set(atop(usize::MAX) + 1);

    // SAFETY: `new_pmr` is fresh, in no tree and on no list.
    unsafe { UVM.pmr_control.addr.insert(new_pmr) };
    uvm_pmemrange_use_insert(&UVM.pmr_control.r#use, new_pmr);

    for c in <crate::machine::Machine as crate::machine::Pmap>::UVM_MD_CONSTRAINTS {
        uvm_pmr_use_inc(c.ucr_low, c.ucr_high);
    }
}

/// `uvm_pmemrange_find`: find the pmemrange that contains the given page number. (Manually
/// traverses the binary tree, because that is cheaper on stack usage.)
pub fn uvm_pmemrange_find(pageno: usize) -> Option<&'static UvmPmemrange> {
    let mut pmr = UVM.pmr_control.addr.root();
    while let Some(p) = pmr {
        if p.low.get() > pageno {
            pmr = RbtHead::<UvmPmemrangeAddr>::left(p);
        } else if p.high.get() <= pageno {
            pmr = RbtHead::<UvmPmemrangeAddr>::right(p);
        } else {
            return Some(pmr_static(p));
        }
    }
    None
}

/// `uvm_pmr_isfree`: return true if the given page is in any of the free lists. Used by
/// `uvm_page_printit`. This function is safe, even if the page is not on the freeq. Note: does
/// not apply locking, only called from ddb.
pub fn uvm_pmr_isfree(pg: &VmPage) -> bool {
    let Some(pmr) = uvm_pmemrange_find(pgno(pg)) else {
        return false;
    };
    let r = match pmr.addr.nfind(pg) {
        None => pmr.addr.max(),
        Some(r) if !VmPage::same(r, pg) => RbtHead::<UvmPmrAddr>::prev(r),
        Some(r) => Some(r),
    };
    let Some(r) = r else {
        return false; // Empty tree.
    };

    kdassert!(pgno(r) <= pgno(pg));
    pgno(r) + r.fpgsz.get() > pgno(pg)
}

/// `uvm_pmr_rootupdate`: given a root of a tree, find a range which intersects start, end and
/// is of the same memtype. Page must be in the address tree.
pub fn uvm_pmr_rootupdate(
    pmr: &UvmPmemrange,
    init_root: &'static VmPage,
    start: usize,
    end: usize,
    memtype: usize,
) -> Option<&'static VmPage> {
    let _ = pmr;
    let mut root = Some(init_root);

    // Which direction to use for searching.
    let direction = if start != 0 && pgno(init_root) + init_root.fpgsz.get() <= start {
        1
    } else if end != 0 && pgno(init_root) >= end {
        -1
    } else {
        // nothing to do
        return Some(init_root);
    };

    // First, update root to fall within the chosen range.
    while let Some(r) = root
        && !pmr_intersects_with(pgno(r), pgno(r) + r.fpgsz.get(), start, end)
    {
        root = if direction == 1 {
            RbtHead::<UvmPmrAddr>::right(r).map(VmPage::forever)
        } else {
            RbtHead::<UvmPmrAddr>::left(r).map(VmPage::forever)
        };
    }
    let root = root?;
    if uvm_pmr_pg_to_memtype(root) == memtype {
        return Some(root);
    }

    // Root is valid, but of the wrong memtype.
    //
    // Try to find a range that has the given memtype in the subtree (memtype mismatches are
    // costly, either because the conversion is expensive, or a later allocation will need to do
    // the opposite conversion, which will be expensive).
    //
    // First, simply increase address until we hit something we can use. Cache the upper page, so
    // we can page-walk later.
    let mut high = root;
    let mut high_next = RbtHead::<UvmPmrAddr>::right(high).map(VmPage::forever);
    while let Some(hn) = high_next
        && pmr_intersects_with(pgno(hn), pgno(hn) + hn.fpgsz.get(), start, end)
    {
        high = hn;
        if uvm_pmr_pg_to_memtype(high) == memtype {
            return Some(high);
        }
        high_next = RbtHead::<UvmPmrAddr>::right(high).map(VmPage::forever);
    }

    // Second, decrease the address until we hit something we can use. Cache the lower page, so
    // we can page-walk later.
    let mut low = root;
    let mut low_next = RbtHead::<UvmPmrAddr>::left(low).map(VmPage::forever);
    while let Some(ln) = low_next
        && pmr_intersects_with(pgno(ln), pgno(ln) + ln.fpgsz.get(), start, end)
    {
        low = ln;
        if uvm_pmr_pg_to_memtype(low) == memtype {
            return Some(low);
        }
        low_next = RbtHead::<UvmPmrAddr>::left(low).map(VmPage::forever);
    }

    if VmPage::same(low, high) {
        return None;
    }

    // No hits. Walk the address tree until we find something usable.
    let mut cur = RbtHead::<UvmPmrAddr>::next(low).map(VmPage::forever);
    while let Some(l) = cur
        && !VmPage::same(l, high)
    {
        kdassert!(pmr_is_subrange_of(
            pgno(l),
            pgno(l) + l.fpgsz.get(),
            start,
            end
        ));
        if uvm_pmr_pg_to_memtype(l) == memtype {
            return Some(l);
        }
        cur = RbtHead::<UvmPmrAddr>::next(l).map(VmPage::forever);
    }

    // Nothing found.
    None
}

/// `uvm_pmr_get1page`: allocate any page, the fastest way. Page number constraints only.
pub fn uvm_pmr_get1page(
    count: usize,
    memtype_init: usize,
    result: &Pglist,
    start: usize,
    end: usize,
    memtype_only: bool,
) -> usize {
    let mut fcount = 0usize;
    let mut pmr_iter = UVM.pmr_control.r#use.first();
    while let Some(pmr) = pmr_iter {
        let pmr = pmr_static(pmr);
        pmr_iter = TailqHead::<PmrUse>::next(pmr);

        // We're done.
        if fcount == count {
            break;
        }

        // Outside requested range.
        if !(start == 0 && end == 0)
            && !pmr_intersects_with(pmr.low.get(), pmr.high.get(), start, end)
        {
            continue;
        }

        // Range is empty.
        if pmr.nsegs.get() == 0 {
            continue;
        }

        // Loop over all memtypes, starting at memtype_init.
        let mut memtype = memtype_init;
        while fcount != count {
            let mut found = pmr.single[memtype].first().map(VmPage::forever);
            // If found is outside the range, walk the list until we find something that
            // intersects with boundaries.
            while let Some(f) = found
                && !pmr_intersects_with(pgno(f), pgno(f) + 1, start, end)
            {
                found = Pglist::next(f).map(VmPage::forever);
            }

            if found.is_none() {
                // Check if the size tree contains a range that intersects with the boundaries.
                // As the allocation is for any page, try the smallest range so that large ranges
                // are preserved for more constrained cases. Only one entry is checked here, to
                // avoid a brute-force search.
                //
                // Note that a size tree gives pg[1] instead of pg[0].
                // SAFETY: a size-tree node is the second page of its range.
                found = pmr.size[memtype]
                    .min()
                    .map(|n| unsafe { n.sub(1) }.forever());
                if let Some(f) = found
                    && !pmr_intersects_with(pgno(f), pgno(f) + f.fpgsz.get(), start, end)
                {
                    found = None;
                }
            }
            if found.is_none() {
                // Try address-guided search to meet the page number constraints.
                if let Some(root) = pmr.addr.root() {
                    found = uvm_pmr_rootupdate(pmr, root.forever(), start, end, memtype);
                }
            }
            if let Some(f) = found {
                uvm_pmr_assertvalid(pmr);
                uvm_pmr_remove_size(pmr, f);

                // If the page intersects the end, then it'll need splitting.
                //
                // Note that we don't need to split if the page intersects start: the drain
                // function will simply stop on hitting start.
                if end != 0 && pgno(f) + f.fpgsz.get() > end {
                    let splitsz = pgno(f) + f.fpgsz.get() - end;

                    uvm_pmr_remove_addr(pmr, f);
                    uvm_pmr_assertvalid(pmr);
                    f.fpgsz.set(f.fpgsz.get() - splitsz);
                    // SAFETY: `fpgsz + splitsz` was the range's length, so the split point is
                    // inside the range.
                    let splitpg = unsafe { f.add(f.fpgsz.get()) }.forever();
                    splitpg.fpgsz.set(splitsz);
                    uvm_pmr_insert(pmr, splitpg, true);

                    // At this point, splitpg and found actually should be joined. But we
                    // explicitly disable that, because we will start subtracting from found.
                    kassert!(start == 0 || pgno(f) + f.fpgsz.get() > start);
                    uvm_pmr_insert_addr(pmr, f, true);
                }

                // Fetch pages from the end. If the range is larger than the requested number of
                // pages, this saves us an addr-tree update.
                //
                // Since we take from the end and insert at the head, any ranges keep preserved.
                while f.fpgsz.get() > 0
                    && fcount < count
                    && (start == 0 || pgno(f) + f.fpgsz.get() > start)
                {
                    f.fpgsz.set(f.fpgsz.get() - 1);
                    fcount += 1;
                    // SAFETY: `fpgsz` (after the decrement) is inside the range's old length.
                    let taken = unsafe { f.add(f.fpgsz.get()) };
                    // SAFETY: a page inside a free range is on no list.
                    unsafe { result.insert_head(taken) };
                }
                if f.fpgsz.get() > 0 {
                    uvm_pmr_insert_size(pmr, f);
                    kdassert!(fcount == count);
                    uvm_pmr_assertvalid(pmr);
                    return fcount;
                }

                // Delayed addr-tree removal.
                uvm_pmr_remove_addr(pmr, f);
                uvm_pmr_assertvalid(pmr);
            } else {
                if memtype_only {
                    break;
                }
                // Skip to the next memtype.
                memtype += 1;
                if memtype == UVM_PMR_MEMTYPE_MAX {
                    memtype = 0;
                }
                if memtype == memtype_init {
                    break;
                }
            }
        }
    }

    // Search finished.
    //
    // Ran out of ranges before enough pages were gathered, or we hit the case where
    // found->fpgsz == count - fcount, in which case the above exit condition didn't trigger.
    //
    // On failure, caller will free the pages.
    fcount
}

/// `uvm_pmr_print`: print information about pmemrange. Does not do locking (so either call it
/// from DDB or acquire fpageq lock before invoking).
pub fn uvm_pmr_print() {
    kprintf!("Ranges, use queue:\n");
    let mut useq_len = 0;
    for pmr in UVM.pmr_control.r#use.iter() {
        useq_len += 1;
        let mut free = 0usize;
        let mut size = [0usize; UVM_PMR_MEMTYPE_MAX];
        for (mt, sz) in size.iter_mut().enumerate() {
            let pg = match pmr.size[mt].max() {
                // SAFETY: a size-tree node is the second page of its range.
                Some(n) => Some(unsafe { n.sub(1) }),
                None => pmr.single[mt].first(),
            };
            *sz = pg.map_or(0, |pg| pg.fpgsz.get());

            for pg in pmr.addr.iter() {
                free += pg.fpgsz.get();
            }
        }

        kprintf!(
            "* [{:#x}-{:#x}] use={} nsegs={}",
            pmr.low.get(),
            pmr.high.get(),
            pmr.r#use.get(),
            pmr.nsegs.get()
        );
        for (mt, sz) in size.iter().enumerate() {
            kprintf!(" maxsegsz[{}]={:#x}", mt, sz);
        }
        kprintf!(" free={:#x}\n", free);
    }
    kprintf!("#ranges = {}\n", useq_len);
}

/// This CPU's page cache (`&curcpu()->ci_uvm`, see the module's deviations).
#[cfg(feature = "multiprocessor")]
fn uvm_pmr_curcache() -> &'static UvmPmrCache {
    &UVM_PMR_CACHES[cpu_number() as usize]
}

/// `uvm_pmr_cache_alloc`: fills the empty magazine `upci` from the allocator; false when the
/// allocator has no `UVM_PMR_CACHEMAGSZ` pages to spare.
#[cfg(feature = "multiprocessor")]
fn uvm_pmr_cache_alloc(upci: &UvmPmrCacheItem) -> bool {
    let flags = UVM_PLA_NOWAIT | UVM_PLA_NOWAKE;
    let npages = UVM_PMR_CACHEMAGSZ;

    splassert(IPL_BIO, "uvm_pmr_cache_alloc");
    kassert!(upci.upci_npages.get() == 0);

    let pgl = Pglist::new();
    pgl.init();
    if uvm_pmr_getpages(npages, 0, 0, 1, 0, npages as i32, flags, &pgl).is_err() {
        return false;
    }

    while let Some(pg) = pgl.first().map(VmPage::forever) {
        // SAFETY: `pg` is on `pgl`, which only this function sees.
        unsafe { pgl.remove(pg) };
        let n = upci.upci_npages.get();
        upci.upci_pages[n as usize].set(ptr::from_ref(pg));
        upci.upci_npages.set(n + 1);
    }
    UVMEXP
        .percpucaches
        .fetch_add(npages as i32, Ordering::Relaxed);

    true
}

/// `uvm_pmr_cache_get`: a page from this CPU's cache, refilled from the allocator when both
/// magazines are empty; straight from the allocator when it cannot be refilled.
#[cfg(feature = "multiprocessor")]
pub fn uvm_pmr_cache_get(flags: i32) -> Option<&'static VmPage> {
    let upc = uvm_pmr_curcache();

    // XXX The buffer flipper (incorrectly?) allocates & frees pages (from
    // uvm_pagerealloc_multi()) from interrupt context!
    let s = splbio();
    let mut upci = &upc.upc_magz[upc.upc_actv.get() as usize];
    if upci.upci_npages.get() == 0 {
        let prev = if upc.upc_actv.get() == 0 { 1 } else { 0 };
        upci = &upc.upc_magz[prev as usize];
        if upci.upci_npages.get() == 0 {
            UVMEXP.pcpmiss.fetch_add(1, Ordering::Relaxed);
            if !uvm_pmr_cache_alloc(upci) {
                splx(s);
                return uvm_pmr_getone(flags);
            }
        }
        // Swap magazines
        upc.upc_actv.set(prev);
    } else {
        UVMEXP.pcphit.fetch_add(1, Ordering::Relaxed);
    }

    UVMEXP.percpucaches.fetch_sub(1, Ordering::Relaxed);
    let n = upci.upci_npages.get() - 1;
    upci.upci_npages.set(n);
    let pg = upci.upci_pages[n as usize].get();
    splx(s);

    // SAFETY: a page of the vm_page array the allocator handed to this magazine.
    let pg = unsafe { &*pg };
    if flags & UVM_PLA_ZERO != 0 {
        uvm_pagezero(pg);
    }

    Some(pg)
}

/// `uvm_pmr_cache_free`: gives every page of the magazine back to the allocator; the number
/// of pages.
#[cfg(feature = "multiprocessor")]
fn uvm_pmr_cache_free(upci: &UvmPmrCacheItem) -> u32 {
    splassert(IPL_BIO, "uvm_pmr_cache_free");

    let pgl = Pglist::new();
    pgl.init();
    let n = upci.upci_npages.get() as usize;
    for slot in &upci.upci_pages[..n] {
        // SAFETY: the magazine's first `upci_npages` slots hold free pages of the vm_page
        // array, on no list.
        unsafe { pgl.insert_tail(&*slot.get()) };
    }

    uvm_pmr_freepageq(&pgl);

    UVMEXP
        .percpucaches
        .fetch_sub(upci.upci_npages.get(), Ordering::Relaxed);
    upci.upci_npages.set(0);
    for slot in &upci.upci_pages {
        slot.set(ptr::null());
    }

    n as u32
}

/// `uvm_pmr_cache_put`: returns a page to this CPU's cache; low pages always go back to the
/// allocator, so as not to accelerate their exhaustion.
#[cfg(feature = "multiprocessor")]
pub fn uvm_pmr_cache_put(pg: &VmPage) {
    let upc = uvm_pmr_curcache();

    // Always give back low pages to the allocator to not accelerate their exhaustion.
    let pmr = uvm_pmemrange_find(atop(vm_page_to_phys(pg).as_usize()));
    if pmr.is_none_or(|pmr| pmr.r#use.get() > 0) {
        uvm_pmr_freepages(pg, 1);
        return;
    }

    kassert!(pg.wire_count.get() == 0);
    kassert!(pg.uanon.get().is_null());
    kassert!(pg.uobject.get().is_null());

    // XXX The buffer flipper (incorrectly?) allocates & frees pages (from
    // uvm_pagerealloc_multi()) from interrupt context!
    let s = splbio();
    let mut upci = &upc.upc_magz[upc.upc_actv.get() as usize];
    if upci.upci_npages.get() as usize >= UVM_PMR_CACHEMAGSZ {
        let prev = if upc.upc_actv.get() == 0 { 1 } else { 0 };
        upci = &upc.upc_magz[prev as usize];
        if upci.upci_npages.get() > 0 {
            let _ = uvm_pmr_cache_free(upci);
        }

        // Swap magazines
        upc.upc_actv.set(prev);
        kassert!(upci.upci_npages.get() == 0);
    }

    let n = upci.upci_npages.get();
    upci.upci_pages[n as usize].set(ptr::from_ref(pg));
    upci.upci_npages.set(n + 1);
    UVMEXP.percpucaches.fetch_add(1, Ordering::Relaxed);
    splx(s);
}

/// `uvm_pmr_cache_drain`: gives this CPU's cached pages back to the allocator; how many.
#[cfg(feature = "multiprocessor")]
pub fn uvm_pmr_cache_drain() -> u32 {
    let upc = uvm_pmr_curcache();
    let mut freed = 0;

    // XXX The buffer flipper (incorrectly?) allocates & frees pages (from
    // uvm_pagerealloc_multi()) from interrupt context!
    let s = splbio();
    freed += uvm_pmr_cache_free(&upc.upc_magz[0]);
    freed += uvm_pmr_cache_free(&upc.upc_magz[1]);
    splx(s);

    freed
}

// !(MULTIPROCESSOR && __HAVE_UVM_PERCPU)

/// `uvm_pmr_cache_get`: without a per-CPU cache, straight from the allocator.
#[cfg(not(feature = "multiprocessor"))]
pub fn uvm_pmr_cache_get(flags: i32) -> Option<&'static VmPage> {
    uvm_pmr_getone(flags)
}

/// `uvm_pmr_cache_put`: without a per-CPU cache, straight to the allocator.
#[cfg(not(feature = "multiprocessor"))]
pub fn uvm_pmr_cache_put(pg: &VmPage) {
    uvm_pmr_freepages(pg, 1);
}

/// `uvm_pmr_cache_drain`: nothing to drain without a per-CPU cache.
#[cfg(not(feature = "multiprocessor"))]
pub fn uvm_pmr_cache_drain() -> u32 {
    0
}
/* </CODE> */

/* <TESTS> */
#[cfg(test)]
pub(crate) mod tests {
    // Host tests for the physical page allocator over synthetic physical segments.
    //
    // The page system is global, so every test takes [`LOCK`], forgets the previous memory and
    // loads two segments: one inside the host's "ISA" range (use count 2 after `uvm_pmr_init`)
    // and one above 4 GiB (use count 0), so allocations prefer the second. The page array and
    // the ranges are stolen from the segments, so the free total is a little under the loaded
    // size; [`total`] reads it from the segments.

    use std::sync::{Mutex, MutexGuard};
    use std::vec::Vec;
    use std::{assert, assert_eq, assert_ne, vec};

    use super::*;
    use crate::sys::param::PAGE_SIZE;
    use crate::sys::types::Vaddr;
    use crate::uvm::uvm_page::{
        PG_BUSY, PHYS_TO_VM_PAGE, uvm_page_init, uvm_page_physload, uvm_page_test_reset,
        uvm_pagealloc, uvm_pagefree, uvm_pglistalloc, uvm_pglistfree, uvm_setpagesize, vm_physmem,
    };

    /// Serialises the tests: the page system is one set of statics.
    pub(crate) static LOCK: Mutex<()> = Mutex::new(());

    /// A segment in the ISA range: pages 0x100..0x200 (1 MiB..2 MiB).
    const SEG_ISA: (usize, usize) = (0x100, 0x200);
    /// A segment above 4 GiB: pages 0x10_0000..0x10_1000 (16 MiB).
    const SEG_HIGH: (usize, usize) = (0x10_0000, 0x10_1000);

    fn pages(seg: (usize, usize)) -> usize {
        seg.1 - seg.0
    }

    /// The pages the segments hold after the boot-time stealing.
    fn total() -> usize {
        vm_physmem()
            .iter()
            .map(|seg| seg.avail_end - seg.avail_start)
            .sum()
    }

    fn free() -> usize {
        UVMEXP.free.load(Ordering::Relaxed) as usize
    }

    fn in_seg(pg: &VmPage, seg: (usize, usize)) -> bool {
        (seg.0..seg.1).contains(&pgno(pg))
    }

    fn assert_all_valid() {
        for pmr in UVM.pmr_control.addr.iter() {
            uvm_pmr_assertvalid(pmr);
        }
    }

    fn setup() -> MutexGuard<'static, ()> {
        let guard = LOCK.lock().unwrap_or_else(|e| e.into_inner());
        uvm_page_test_reset();
        UVMEXP.pagesize.store(PAGE_SIZE as i32, Ordering::Relaxed);
        uvm_setpagesize();
        uvm_page_physload(SEG_HIGH.0, SEG_HIGH.1, SEG_HIGH.0, SEG_HIGH.1, 0);
        uvm_page_physload(SEG_ISA.0, SEG_ISA.1, SEG_ISA.0, SEG_ISA.1, 0);
        let (mut start, mut end) = (Vaddr::new(0), Vaddr::new(0));
        uvm_page_init(&mut start, &mut end);
        assert!(start < end, "the host reports a kernel virtual range");
        assert!(
            total() > pages(SEG_ISA) + pages(SEG_HIGH) - 256,
            "little was stolen"
        );
        guard
    }

    #[test]
    fn init_frees_every_page_and_splits_the_ranges() {
        let _g = setup();
        assert_eq!(free(), total());
        assert_all_valid();

        // The host's constraints (ISA at 16 MiB, DMA at 4 GiB) split the single range in three,
        // sorted by use on the use queue: the unconstrained range first.
        let uses: Vec<i32> = UVM
            .pmr_control
            .r#use
            .iter()
            .map(|p| p.r#use.get())
            .collect();
        assert_eq!(uses, vec![0, 1, 2]);
        assert_eq!(UVM.pmr_control.addr.iter().count(), 3);

        let isa = uvm_pmemrange_find(SEG_ISA.0).expect("the ISA segment has a range");
        assert_eq!((isa.low.get(), isa.high.get()), (0, 0x1000));
        assert_eq!(isa.nsegs.get(), 1);
        let high = uvm_pmemrange_find(SEG_HIGH.0).expect("the high segment has a range");
        assert_eq!(high.low.get(), 0x10_0000);
        assert_eq!(high.nsegs.get(), 1);

        // Boot memory was stolen from a boundary of one segment (`uvm_page_physsteal`): what is
        // left of each segment is one free range.
        for seg in vm_physmem() {
            let pg = PHYS_TO_VM_PAGE(Paddr::new(seg.avail_start << crate::sys::param::PAGE_SHIFT))
                .expect("the first available page of a segment has a structure");
            assert!(uvm_pmr_isfree(pg));
            assert_eq!(pg.fpgsz.get(), seg.avail_end - seg.avail_start);
        }
        assert!(
            total() < pages(SEG_ISA) + pages(SEG_HIGH),
            "the page array came out of a segment"
        );
    }

    #[test]
    fn getone_prefers_the_least_used_range() {
        let _g = setup();
        let pg = uvm_pmr_getone(UVM_PLA_NOWAIT).expect("a page");
        assert!(
            in_seg(pg, SEG_HIGH),
            "page {:#x} is not in the high segment",
            pgno(pg)
        );
        assert_eq!(pg.flags() & PQ_FREE, 0);
        assert_eq!(free(), total() - 1);
        assert!(!uvm_pmr_isfree(pg));
        assert_all_valid();

        uvm_pmr_freepages(pg, 1);
        assert_eq!(free(), total());
        assert!(uvm_pmr_isfree(pg));
        assert_all_valid();
        // The page joined its range again: one segment per range.
        let high = uvm_pmemrange_find(SEG_HIGH.0).expect("range");
        assert_eq!(high.nsegs.get(), 1);
    }

    #[test]
    fn contiguous_aligned_and_bounded() {
        let _g = setup();
        let list = Pglist::new();
        list.init();
        let npages = 16;
        let r = uvm_pglistalloc(
            npages * PAGE_SIZE,
            Paddr::new(SEG_HIGH.0 << crate::sys::param::PAGE_SHIFT),
            Paddr::new((SEG_HIGH.1 << crate::sys::param::PAGE_SHIFT) - 1),
            Paddr::new(16 * PAGE_SIZE),
            Paddr::new(64 * PAGE_SIZE),
            &list,
            1,
            UVM_PLA_NOWAIT,
        );
        assert_eq!(r, Ok(()));
        let got: Vec<usize> = list.iter().map(pgno).collect();
        assert_eq!(got.len(), npages);
        assert_eq!(got[0] % 16, 0, "aligned");
        assert_eq!(got[0] / 64, got[npages - 1] / 64, "no boundary crossing");
        for w in got.windows(2) {
            assert_eq!(w[1], w[0] + 1, "contiguous");
        }
        assert!(got.iter().all(|&p| (SEG_HIGH.0..SEG_HIGH.1).contains(&p)));
        assert_eq!(free(), total() - npages);
        assert_all_valid();

        uvm_pglistfree(&list);
        assert!(list.is_empty());
        assert_eq!(free(), total());
        assert_all_valid();
    }

    #[test]
    fn constrained_to_the_isa_range() {
        let _g = setup();
        let list = Pglist::new();
        list.init();
        let r = uvm_pglistalloc(
            4 * PAGE_SIZE,
            Paddr::new(0),
            Paddr::new(0x00ff_ffff),
            Paddr::new(0),
            Paddr::new(0),
            &list,
            4,
            UVM_PLA_NOWAIT,
        );
        assert_eq!(r, Ok(()));
        assert_eq!(list.iter().count(), 4);
        assert!(list.iter().all(|pg| in_seg(pg, SEG_ISA)));
        uvm_pglistfree(&list);
        assert_eq!(free(), total());
        assert_all_valid();
    }

    #[test]
    fn exhaustion_fails_cleanly() {
        let _g = setup();
        let list = Pglist::new();
        list.init();

        // More than exists.
        let r = uvm_pmr_getpages(total() + 1, 0, 0, 1, 0, 1, UVM_PLA_NOWAIT, &list);
        assert_eq!(r, Err(Errno::ENOMEM));
        assert!(list.is_empty());
        assert_eq!(free(), total());

        // Everything but the reserves, in as many segments as it takes.
        let reserve = UVMEXP.reserve_kernel.load(Ordering::Relaxed) as usize;
        let count = total() - reserve - 1;
        let r = uvm_pmr_getpages(count, 0, 0, 1, 0, count as i32, UVM_PLA_NOWAIT, &list);
        assert_eq!(r, Ok(()));
        assert_eq!(list.iter().count(), count);
        assert_eq!(free(), total() - count);
        assert!(list.iter().all(|pg| pg.flags() & PQ_FREE == 0));
        assert_all_valid();

        // Into the reserve: refused without UVM_PLA_USERESERVE.
        let more = Pglist::new();
        more.init();
        let r = uvm_pmr_getpages(2, 0, 0, 1, 0, 2, UVM_PLA_NOWAIT, &more);
        assert_eq!(r, Err(Errno::ENOMEM));
        assert!(more.is_empty());

        uvm_pmr_freepageq(&list);
        assert!(list.is_empty());
        assert_eq!(free(), total());
        assert_all_valid();
        for pmr in UVM.pmr_control.addr.iter() {
            assert!(pmr.nsegs.get() <= 1, "every range is one free chunk again");
        }
    }

    #[test]
    fn freeing_a_descending_list_joins_the_range() {
        let _g = setup();
        let list = Pglist::new();
        list.init();
        let r = uvm_pmr_getpages(32, 0, 0, 1, 0, 1, UVM_PLA_NOWAIT, &list);
        assert_eq!(r, Ok(()));

        // Rebuild the list with the pages in descending order of physical address.
        let reversed = Pglist::new();
        reversed.init();
        while let Some(pg) = list.first() {
            // SAFETY: `pg` is the head of `list`; it goes to `reversed` right after.
            unsafe {
                list.remove(pg);
                reversed.insert_head(pg);
            }
        }
        let order: Vec<usize> = reversed.iter().map(pgno).collect();
        assert!(order.windows(2).all(|w| w[0] == w[1] + 1));

        uvm_pmr_freepageq(&reversed);
        assert!(reversed.is_empty());
        assert_eq!(free(), total());
        assert_all_valid();
        let high = uvm_pmemrange_find(SEG_HIGH.0).expect("range");
        assert_eq!(high.nsegs.get(), 1);
    }

    #[test]
    fn zeroed_pages_are_counted() {
        let _g = setup();
        let before = UVMEXP.pga_zeromiss.load(Ordering::Relaxed);
        let pg = uvm_pmr_getone(UVM_PLA_NOWAIT | UVM_PLA_ZERO).expect("a page");
        assert_eq!(UVMEXP.pga_zeromiss.load(Ordering::Relaxed), before + 1);
        assert_eq!(pg.flags() & (PG_ZERO | PQ_FREE), 0);
        uvm_pmr_freepages(pg, 1);
        assert_eq!(free(), total());
    }

    #[test]
    fn pagealloc_and_pagefree() {
        let _g = setup();
        let pg = uvm_pagealloc(None, 0, None, 0).expect("a page");
        assert_ne!(pg.flags() & PG_BUSY, 0);
        assert_eq!(pg.flags() & PQ_FREE, 0);
        assert!(pg.uobject().is_none());
        assert_eq!(free(), total() - 1);

        uvm_pagefree(pg);
        assert_eq!(free(), total());
        assert!(uvm_pmr_isfree(pg));
        assert_all_valid();
    }

    #[test]
    fn pow2divide_rounds_up_to_a_power_of_two() {
        assert_eq!(pow2divide(1, 8), 1);
        assert_eq!(pow2divide(8, 8), 1);
        assert_eq!(pow2divide(9, 8), 2);
        assert_eq!(pow2divide(100, 7), 16);
        assert_eq!(pow2divide(4096, 1), 4096);
    }

    #[test]
    fn range_predicates() {
        assert!(pmr_is_subrange_of(10, 20, 0, 0));
        assert!(pmr_is_subrange_of(10, 20, 10, 20));
        assert!(!pmr_is_subrange_of(10, 21, 10, 20));
        assert!(pmr_intersects_with(10, 20, 19, 0));
        assert!(!pmr_intersects_with(10, 20, 20, 0));
        assert!(pmr_intersects_with(10, 20, 0, 11));
        assert!(!pmr_intersects_with(10, 20, 0, 10));
        assert_eq!(pmr_align(17, 16), 32);
        assert_eq!(pmr_align_down(17, 16), 16);
    }
}
/* </TESTS> */
