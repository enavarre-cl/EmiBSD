/*	$OpenBSD: uvm_page.h,v 1.74 2026/07/11 13:13:16 kettenis Exp $	*/
/*	$NetBSD: uvm_page.h,v 1.19 2000/12/28 08:24:55 chs Exp $	*/
/*	$OpenBSD: uvm_page.c,v 1.190 2026/07/11 13:13:16 kettenis Exp $	*/
/*	$NetBSD: uvm_page.c,v 1.44 2000/11/27 08:40:04 chs Exp $	*/
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
 *	@(#)vm_page.h   7.3 (Berkeley) 4/21/91
 *	@(#)vm_page.c   8.3 (Berkeley) 3/21/94
 * from: Id: uvm_page.h,v 1.1.2.6 1998/02/04 02:31:42 chuck Exp
 * from: Id: uvm_page.c,v 1.1.2.18 1998/02/06 05:24:42 chs Exp
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
//! Resident memory system definitions and page ops: `<uvm/uvm_page.h>` and `uvm/uvm_page.c`.
//!
//! Upstream: sys/uvm/uvm_page.h @ 3ce1f3f79392
//! Upstream: sys/uvm/uvm_page.c @ 3ce1f3f79392
//!
//! Management of resident (logical) pages. A small structure is kept for each resident page,
//! indexed by page number. Each structure contains a list used for manipulating pages, and a
//! tree structure for in object/offset lookups. In addition, the structure contains the object
//! and offset to which this page belongs (for pageout), and sundry status bits.
//!
//! Locks, as in the C: `I` immutable after creation, `a` atomic operations, `Q`
//! `uvm.pageqlock`, `F` `uvm.fpageqlock`, `o` owner lock (`uobject->vmobjlock` or
//! `uanon->an_lock`).
//!
//! Status: `wip`. Milestone M3 ports the page structure, the physical segments
//! (`uvm_page_physload`, `uvm_page_physget`, `vm_physseg_find`, `PHYS_TO_VM_PAGE`),
//! `uvm_page_init`, `uvm_pageboot_alloc`, the allocation and free paths (`uvm_pagealloc`,
//! `uvm_pglistalloc`, `uvm_pagefree`, `uvm_pglistfree`) and the queue and wiring helpers.
//!
//! ## Deviations
//! - `struct vm_page`'s mutable fields are `Cell`s and `pg_flags` an atomic; `phys_addr` is
//!   plain, set when the page is written into its array. The `UVM_PAGE_TRKOWN` fields are not
//!   compiled in.
//! - `pg + n` in the vm_page array is [`VmPage::add`] (unsafe: the array is the invariant), and
//!   `pg + n` as an address for comparisons is [`VmPage::ptr_add`].
//! - `vm_physmem[]` and `vm_nphysseg` are behind [`vm_physmem`] / [`vm_physmem_mut`] /
//!   [`VM_NPHYSSEG`]: written on the boot CPU before `uvm.page_init_done`, read afterwards.
//! - `uvm_page_physload` after `uvm_init` needs `km_alloc` (`uvm_km.c`, later in M3): the
//!   non-preload path reports it and ignores the segment, as the C does when the allocation
//!   fails.
//! - `uvm_page_unbusy` takes `Option`s where the C accepts NULL or `PGO_DONTCARE`.
//! - `uvm_maxkaddr` belongs to `uvm_map.c`; it lives here until that file is ported.
//! - On a machine without an MMU (`PMAP_NOMMU`, the host double) `uvm_pagealloc_multi` asks
//!   for one physical segment: the buffer cache reaches a buffer's pages through the direct
//!   map there (`vfs_biomem.rs`).

use core::cell::Cell;
use core::ptr;
use core::sync::atomic::{AtomicU32, AtomicUsize, Ordering};

use libkern::StaticCell;

use crate::kern::kern_lock::{mtx_enter, mtx_init, mtx_leave};
use crate::kern::kern_synch::rwsleep_nsec;
use crate::kern::subr_prf::panic;
use crate::machine::intr::IPL_VM;
use crate::machine::{Machine, Pmap, VmPageMd, VmParam};
use crate::sys::errno::Errno;
use crate::sys::mman::PROT_NONE;
use crate::sys::mutex::mutex_assert_locked;
use crate::sys::param::{PAGE_MASK, PAGE_SHIFT, PAGE_SIZE, PNORELOCK, PVM};
use crate::sys::queue::{TailqEntry, TailqHead};
use crate::sys::rwlock::{Rwlock, rw_lock_held, rw_write_held};
use crate::sys::smr::smr_flush;
use crate::sys::systm::{INFSLP, kernel_assert_locked};
use crate::sys::tree::RbtEntry;
use crate::sys::types::{Paddr, Vaddr, Vsize};
use crate::uvm::uvm_anon::VmAnon;
use crate::uvm::uvm_extern::{
    PHYSLOAD_DEVICE, UVM_PGA_USERESERVE, UVM_PGA_ZERO, UVM_PLA_NOWAIT, UVM_PLA_USERESERVE,
    UVM_PLA_WAITOK, UVM_PLA_ZERO, UvmConstraintRange, Voff,
};
use crate::uvm::uvm_init::{UVM, UVMEXP};
use crate::uvm::uvm_map::UVM_MAXKADDR;
use crate::uvm::uvm_object::{UvmObject, uvm_obj_is_dummy, uvm_obj_is_kern_object};
use crate::uvm::uvm_param::{DEFAULT_PAGE_SIZE, atop, ptoa, round_page, trunc_page};
use crate::uvm::uvm_pmemrange::{
    uvm_pmr_cache_get, uvm_pmr_cache_put, uvm_pmr_freepageq, uvm_pmr_freepages, uvm_pmr_getpages,
    uvm_pmr_init,
};
use crate::{kassert, kprintf, queue_adapter, unported};

// pg_flags: object flags

/// Page is locked.
pub const PG_BUSY: u32 = 0x0000_0001;
/// Someone is waiting for page.
pub const PG_WANTED: u32 = 0x0000_0002;
/// Page is in VP table.
pub const PG_TABLED: u32 = 0x0000_0004;
/// Page has not been modified.
pub const PG_CLEAN: u32 = 0x0000_0008;
/// Clean bit has been checked.
pub const PG_CLEANCHK: u32 = 0x0000_0010;
/// Page released while paging.
pub const PG_RELEASED: u32 = 0x0000_0020;
/// Page is not yet initialized.
pub const PG_FAKE: u32 = 0x0000_0040;
/// Page must be mapped read-only.
pub const PG_RDONLY: u32 = 0x0000_0080;
/// Page is pre-zero'd.
pub const PG_ZERO: u32 = 0x0000_0100;
/// Page is in device space, lay off.
pub const PG_DEV: u32 = 0x0000_0200;
/// Mask of the object flags.
pub const PG_MASK: u32 = 0x0000_ffff;

/// Page is on free list.
pub const PQ_FREE: u32 = 0x0001_0000;
/// Page is in inactive list.
pub const PQ_INACTIVE: u32 = 0x0002_0000;
/// Page is in active list.
pub const PQ_ACTIVE: u32 = 0x0004_0000;
/// Page is an iterator marker.
pub const PQ_ITER: u32 = 0x0008_0000;
/// Page is part of an anon, rather than an uvm_object.
pub const PQ_ANON: u32 = 0x0010_0000;
/// Page is part of an anonymous uvm_object.
pub const PQ_AOBJ: u32 = 0x0020_0000;
/// Swap-backed pages.
pub const PQ_SWAPBACKED: u32 = PQ_ANON | PQ_AOBJ;
/// Page needs {en,de}cryption.
pub const PQ_ENCRYPT: u32 = 0x0040_0000;
/// Mask of the queue flags.
pub const PQ_MASK: u32 = 0x00ff_0000;

/// Used by some pmaps.
pub const PG_PMAP0: u32 = 0x0100_0000;
/// Used by some pmaps.
pub const PG_PMAP1: u32 = 0x0200_0000;
/// Used by some pmaps.
pub const PG_PMAP2: u32 = 0x0400_0000;
/// Used by some pmaps.
pub const PG_PMAP3: u32 = 0x0800_0000;
/// Used by some pmaps.
pub const PG_PMAP4: u32 = 0x1000_0000;
/// Used by some pmaps.
pub const PG_PMAP5: u32 = 0x2000_0000;
/// Mask of the pmap flags.
pub const PG_PMAPMASK: u32 = 0x3f00_0000;

// Physical memory segment strategies: how vm_physmem[] is kept.

/// Random: put it at the end (if this is "1" then we revert to a "contig" case).
pub const VM_PSTRAT_RANDOM: i32 = 1;
/// Binary search (sorted by address).
pub const VM_PSTRAT_BSEARCH: i32 = 2;
/// Linear search (sorted by largest segment first).
pub const VM_PSTRAT_BIGFIRST: i32 = 3;

/// `VM_PHYSSEG_MAX` of the selected machine.
pub const VM_PHYSSEG_MAX: usize = <Machine as VmParam>::VM_PHYSSEG_MAX;
/// `VM_PHYSSEG_STRAT` of the selected machine.
pub const VM_PHYSSEG_STRAT: i32 = <Machine as VmParam>::VM_PHYSSEG_STRAT;

queue_adapter!(
    /// The `pageq` list adapter: the LRU and free page queues.
    pub Pageq: VmPage, pageq => TailqEntry<VmPage>
);

/// `struct pglist`: a `TAILQ` of pages through `pageq`.
pub type Pglist = TailqHead<Pageq>;

/// `struct vm_page`.
pub struct VmPage {
    /// \[Q\] LRU or free page queue.
    pub pageq: TailqEntry<VmPage>,
    /// \[o\] object tree.
    pub objt: RbtEntry,
    /// \[o\] anon.
    pub uanon: Cell<*const VmAnon>,
    /// \[o\] object.
    pub uobject: Cell<*const UvmObject>,
    /// \[o\] offset into object.
    pub offset: Cell<Voff>,
    /// \[a\] object flags.
    pg_flags: AtomicU32,
    /// Version count.
    pub pg_version: Cell<u32>,
    /// \[o\] wired down map refs.
    pub wire_count: Cell<u32>,
    /// \[I\] physical address.
    pub phys_addr: Paddr,
    /// \[F\] free page range size, in pages.
    pub fpgsz: Cell<usize>,
    /// Pmap-specific data.
    pub mdpage: VmPageMd,
}

// SAFETY: every field is guarded by one of the locks in the module doc, as in C: the owner
// lock (`o`), `uvm.pageqlock` (`Q`, the queue linkage), `uvm.fpageqlock` (`F`, a free page's
// range and size); `pg_flags` is atomic and `phys_addr` immutable.
unsafe impl Sync for VmPage {}

impl VmPage {
    /// A fresh page structure for the frame at `phys_addr`: what `uvm_page_init` leaves after
    /// zeroing the array and setting `phys_addr`, `VM_MDPAGE_INIT`, `uobject` and `uanon`.
    pub const fn new(phys_addr: Paddr) -> Self {
        Self {
            pageq: TailqEntry::new(),
            objt: RbtEntry::new(),
            uanon: Cell::new(ptr::null()),
            uobject: Cell::new(ptr::null()),
            offset: Cell::new(0),
            pg_flags: AtomicU32::new(0),
            pg_version: Cell::new(0),
            wire_count: Cell::new(0),
            phys_addr,
            fpgsz: Cell::new(0),
            mdpage: <Machine as Pmap>::VM_MDPAGE_INIT,
        }
    }

    /// `pg_flags`.
    pub fn flags(&self) -> u32 {
        self.pg_flags.load(Ordering::Relaxed)
    }

    /// `atomic_setbits_int(&pg->pg_flags, bits)`.
    pub fn set_bits(&self, bits: u32) {
        self.pg_flags.fetch_or(bits, Ordering::Relaxed);
    }

    /// `atomic_clearbits_int(&pg->pg_flags, bits)`.
    pub fn clear_bits(&self, bits: u32) {
        self.pg_flags.fetch_and(!bits, Ordering::Relaxed);
    }

    /// `pg->uobject` as a reference.
    pub fn uobject(&self) -> Option<&UvmObject> {
        // SAFETY: the pointer is null or set by `uvm_pagealloc_pg`/`uvm_pagerealloc` to an
        // object that outlives its pages (an object is freed only once it has none).
        unsafe { self.uobject.get().as_ref() }
    }

    /// `pg->uanon` as a reference.
    pub fn uanon(&self) -> Option<&VmAnon> {
        // SAFETY: as for `uobject`.
        unsafe { self.uanon.get().as_ref() }
    }

    /// `pg + n`: the page `n` slots further into the same `vm_physseg` array.
    ///
    /// # Safety
    ///
    /// `self` must lie in a `vm_physseg` array with at least `n` more entries after it: the
    /// pages of one free range do (`fpgsz`), and `uvm_page_init` lays each segment out
    /// contiguously.
    pub unsafe fn add(&self, n: usize) -> &VmPage {
        // SAFETY: the caller's guarantee is exactly what `ptr::add` and the dereference need.
        unsafe { &*ptr::from_ref(self).add(n) }
    }

    /// `pg - n`: the page `n` slots earlier in the same array.
    ///
    /// # Safety
    ///
    /// `self` must lie in a `vm_physseg` array with at least `n` entries before it.
    pub unsafe fn sub(&self, n: usize) -> &VmPage {
        // SAFETY: as for `add`.
        unsafe { &*ptr::from_ref(self).sub(n) }
    }

    /// `pg + n` as an address, for comparisons only (it may point one past an array).
    pub fn ptr_add(&self, n: usize) -> *const VmPage {
        ptr::from_ref(self).wrapping_add(n)
    }

    /// `pg - n` as an address, for comparisons only.
    pub fn ptr_sub(&self, n: usize) -> *const VmPage {
        ptr::from_ref(self).wrapping_sub(n)
    }

    /// Whether two references name the same page structure.
    pub fn same(a: &VmPage, b: &VmPage) -> bool {
        ptr::eq(a, b)
    }

    /// The page with the lifetime of the page arrays, which are never freed.
    pub fn forever(&self) -> &'static VmPage {
        // SAFETY: every `vm_page` lives in a `vm_physseg` array allocated for the kernel's
        // whole life (`uvm_page_init`, `uvm_page_physload`); only the borrow is shortened by
        // the list and tree APIs that handed it out.
        unsafe { &*ptr::from_ref(self) }
    }
}

/// `VM_PAGE_TO_PHYS(pg)`.
pub fn vm_page_to_phys(pg: &VmPage) -> Paddr {
    pg.phys_addr
}

/// `VM_PAGE_IS_FREE(pg)`.
pub fn vm_page_is_free(pg: &VmPage) -> bool {
    pg.flags() & PQ_FREE != 0
}

/// `UVM_PAGEZERO_TARGET`: how many zeroed pages the zeroing thread keeps ready.
pub fn uvm_pagezero_target() -> i32 {
    UVMEXP.free.load(Ordering::Relaxed) / 8
}

/// `PADDR_IS_DMA_REACHABLE(paddr)`.
pub fn paddr_is_dma_reachable(paddr: Paddr) -> bool {
    let c = <Machine as Pmap>::DMA_CONSTRAINT;
    c.ucr_low <= paddr && c.ucr_high > paddr
}

/// `struct vm_physseg`: one segment of physical memory, in page frame numbers.
#[derive(Clone, Copy, Debug)]
pub struct VmPhysseg {
    /// PF# of first page in segment.
    pub start: usize,
    /// (PF# of last page in segment) + 1.
    pub end: usize,
    /// PF# of first free page in segment.
    pub avail_start: usize,
    /// (PF# of last free page in segment) + 1.
    pub avail_end: usize,
    /// `vm_page` structures (from start); null before `uvm_page_init`.
    pub pgs: *const VmPage,
    /// `vm_page` structure for end.
    pub lastpg: *const VmPage,
}

impl VmPhysseg {
    const EMPTY: VmPhysseg = VmPhysseg {
        start: 0,
        end: 0,
        avail_start: 0,
        avail_end: 0,
        pgs: ptr::null(),
        lastpg: ptr::null(),
    };

    /// The page structure `off` pages into the segment.
    ///
    /// # Safety
    ///
    /// `uvm_page_init` must have run (`pgs` is set) and `off < end - start`.
    pub unsafe fn page(&self, off: usize) -> &'static VmPage {
        // SAFETY: the caller's guarantee; the array lives forever.
        unsafe { &*self.pgs.add(off) }
    }
}

// SAFETY: `pgs`/`lastpg` point into arrays that live forever; the segment table is written
// only on the boot CPU before `uvm.page_init_done` (see `vm_physmem_mut`).
unsafe impl Sync for VmPhysseg {}
// SAFETY: as above; the pointers name no thread-bound resource.
unsafe impl Send for VmPhysseg {}

/// `vm_physmem[]`: physical memory config (XXXCDC: uvm.physmem).
static VM_PHYSMEM: StaticCell<[VmPhysseg; VM_PHYSSEG_MAX]> =
    StaticCell::new([VmPhysseg::EMPTY; VM_PHYSSEG_MAX]);
/// `vm_nphysseg`: how many of `vm_physmem[]` are in use (XXXCDC: uvm.nphysseg).
pub static VM_NPHYSSEG: AtomicUsize = AtomicUsize::new(0);

/// These variables record the values returned by vm_page_bootstrap, for debugging purposes.
/// The implementation of `uvm_pageboot_alloc` and `pmap_startup` here also uses them
/// internally.
static VIRTUAL_SPACE_START: AtomicUsize = AtomicUsize::new(0);
static VIRTUAL_SPACE_END: AtomicUsize = AtomicUsize::new(0);

/// `vm_physmem[0..vm_nphysseg]`.
pub fn vm_physmem() -> &'static [VmPhysseg] {
    // SAFETY: the table is written only by the boot-time functions below, on the boot CPU,
    // with no reader slice live (they take `vm_physmem_mut` instead); afterwards it is
    // read-only.
    unsafe { &VM_PHYSMEM.get()[..VM_NPHYSSEG.load(Ordering::Relaxed)] }
}

/// The whole `vm_physmem[]` table, for the boot-time code that rewrites it.
///
/// # Safety
///
/// Only before `uvm.page_init_done`, on the boot CPU, with no slice from [`vm_physmem`] live.
pub unsafe fn vm_physmem_mut() -> &'static mut [VmPhysseg; VM_PHYSSEG_MAX] {
    // SAFETY: the caller's guarantee excludes every other reference.
    unsafe { VM_PHYSMEM.get_mut() }
}

/// `uvm_lock_pageq()`: the page queue lock, `mtx_enter(&uvm.pageqlock)`.
pub fn uvm_lock_pageq() {
    mtx_enter(&UVM.pageqlock);
}
/// `uvm_unlock_pageq()`: `mtx_leave(&uvm.pageqlock)`.
pub fn uvm_unlock_pageq() {
    mtx_leave(&UVM.pageqlock);
}
/// `uvm_lock_fpageq()`: the free page queue lock, `mtx_enter(&uvm.fpageqlock)`.
pub fn uvm_lock_fpageq() {
    mtx_enter(&UVM.fpageqlock);
}
/// `uvm_unlock_fpageq()`: `mtx_leave(&uvm.fpageqlock)`.
pub fn uvm_unlock_fpageq() {
    mtx_leave(&UVM.fpageqlock);
}

/// `uvm_pageinsert`: insert a page in the object. Caller must lock object; call should have
/// already set pg's object and offset pointers and bumped the version counter.
fn uvm_pageinsert(pg: &VmPage) {
    let Some(obj) = pg.uobject() else {
        return;
    };
    kassert!(uvm_obj_is_dummy(obj) || rw_write_held(obj.vmobjlock()));
    kassert!(pg.flags() & PG_TABLED == 0);

    // SAFETY: the page is not in any object tree (PG_TABLED is clear), and the object's tree
    // holds pages of this object only.
    let dupe = unsafe { obj.memt.insert(pg) };
    // not allowed to insert over another page
    kassert!(dupe.is_none());
    pg.set_bits(PG_TABLED);
    obj.uo_npages.set(obj.uo_npages.get() + 1);
}

/// `uvm_pageremove`: remove page from object. Caller must lock object.
fn uvm_pageremove(pg: &VmPage) {
    let Some(obj) = pg.uobject() else {
        return;
    };
    kassert!(uvm_obj_is_dummy(obj) || rw_write_held(obj.vmobjlock()));
    kassert!(pg.flags() & PG_TABLED != 0);

    // SAFETY: the page is in this object's tree (PG_TABLED is set).
    unsafe { obj.memt.remove(pg) };

    pg.clear_bits(PG_TABLED);
    obj.uo_npages.set(obj.uo_npages.get() - 1);
    pg.uobject.set(ptr::null());
    pg.pg_version.set(pg.pg_version.get().wrapping_add(1));
}

/// `uvm_page_init`: init the page system. Called from `uvm_init()`. Returns the range of
/// kernel virtual memory in `kvm_startp`/`kvm_endp`.
pub fn uvm_page_init(kvm_startp: &mut Vaddr, kvm_endp: &mut Vaddr) {
    // init the page queues and page queue locks
    UVM.page_active.init();
    UVM.page_inactive.init();
    mtx_init(&UVM.pageqlock, IPL_VM);
    mtx_init(&UVM.fpageqlock, IPL_VM);
    uvm_pmr_init();

    // allocate vm_page structures.
    //
    // sanity check: before calling this function the MD code is expected to register some
    // free RAM with the uvm_page_physload() function. our job now is to allocate vm_page
    // structures for this memory.
    if VM_NPHYSSEG.load(Ordering::Relaxed) == 0 {
        #[allow(clippy::panic)] // the C panics here too
        {
            panic!("uvm_page_bootstrap: no memory pre-allocated");
        }
    }

    // first calculate the number of free pages... note that we use start/end rather than
    // avail_start/avail_end. this allows us to allocate extra vm_page structures in case we
    // want to return some memory to the pool after booting.
    let freepages: usize = vm_physmem().iter().map(|seg| seg.end - seg.start).sum();

    // we now know we have (PAGE_SIZE * freepages) bytes of memory we can use. for each page of
    // memory we use we need a vm_page structure. thus, the total number of pages we can use is
    // the total size of the memory divided by the PAGE_SIZE plus the size of the vm_page
    // structure. we add one to freepages as a fudge factor to avoid truncation errors (since
    // we can only allocate in terms of whole pages).
    let mut pagecount = ((freepages + 1) << PAGE_SHIFT) / (PAGE_SIZE + size_of::<VmPage>());
    let pagearray = uvm_pageboot_alloc(Vsize::new(pagecount * size_of::<VmPage>()));
    let mut cur = pagearray.as_usize() as *mut VmPage;
    // The array is zeroed by uvm_pageboot_alloc; every entry is written below anyway.

    // init the vm_page structures and put them in the correct place.
    // SAFETY: boot CPU, before page_init_done; no `vm_physmem()` slice is live.
    let segs = unsafe { vm_physmem_mut() };
    let nseg = VM_NPHYSSEG.load(Ordering::Relaxed);
    for seg in segs.iter_mut().take(nseg) {
        let n = seg.end - seg.start;
        if n > pagecount {
            #[allow(clippy::panic)] // the C panics here too
            {
                panic!("uvm_page_init: lost {} page(s) in init", n - pagecount);
                // XXXCDC: shouldn't happen?
            }
        }

        // set up page array pointers
        seg.pgs = cur;
        pagecount -= n;
        seg.lastpg = cur.wrapping_add(n - 1);

        // init and free vm_pages (we've already zeroed them)
        for (i, pgno) in (seg.start..seg.end).enumerate() {
            let paddr = ptoa(pgno);
            // SAFETY: `cur + i` is inside the array just allocated for `pagecount` entries.
            unsafe { ptr::write(cur.add(i), VmPage::new(Paddr::new(paddr))) };
            if pgno >= seg.avail_start && pgno < seg.avail_end {
                UVMEXP.npages.fetch_add(1, Ordering::Relaxed);
            }
        }

        // Add pages to free pool.
        if seg.avail_end > seg.avail_start {
            // SAFETY: `avail_start - start < n`, inside the segment's array.
            let first = unsafe { &*cur.add(seg.avail_start - seg.start) };
            uvm_pmr_freepages(first, seg.avail_end - seg.avail_start);
        }
        cur = cur.wrapping_add(n);
    }

    // pass up the values of virtual_space_start and virtual_space_end (obtained by
    // uvm_pageboot_alloc) to the upper layers of the VM.
    *kvm_startp = Vaddr::new(round_page(VIRTUAL_SPACE_START.load(Ordering::Relaxed)));
    *kvm_endp = Vaddr::new(trunc_page(VIRTUAL_SPACE_END.load(Ordering::Relaxed)));

    // init locks for kernel threads: mtx_init(&uvm.aiodoned_lock, IPL_BIO): M5.

    // init reserve thresholds.
    //
    // XXX As long as some disk drivers cannot write any physical page, we need DMA reachable
    // reserves for the pagedaemon. We cannot enforce such requirement but it should be ok in
    // most of the cases because the pmemrange tries hard to allocate them last.
    UVMEXP.reserve_pagedaemon.store(32, Ordering::Relaxed);
    UVMEXP.reserve_kernel.store(32 + 32, Ordering::Relaxed);

    UVM.page_init_done.store(true, Ordering::Release);
}

/// `uvm_setpagesize`: set the page size. Sets `page_shift` and `page_mask` from
/// `uvmexp.pagesize`.
pub fn uvm_setpagesize() {
    if UVMEXP.pagesize.load(Ordering::Relaxed) == 0 {
        UVMEXP
            .pagesize
            .store(DEFAULT_PAGE_SIZE as i32, Ordering::Relaxed);
    }
    let pagesize = UVMEXP.pagesize.load(Ordering::Relaxed);
    UVMEXP.pagemask.store(pagesize - 1, Ordering::Relaxed);
    if (pagesize - 1) & pagesize != 0 {
        #[allow(clippy::panic)] // the C panics here too
        {
            panic!("uvm_setpagesize: page size not a power of two");
        }
    }
    let mut shift = 0;
    while (1 << shift) != pagesize {
        shift += 1;
    }
    UVMEXP.pageshift.store(shift, Ordering::Relaxed);
}

/// `uvm_pageboot_alloc`: steal memory from physmem for bootstrapping. Returns the (zeroed)
/// kernel virtual address of `size` bytes.
pub fn uvm_pageboot_alloc(size: Vsize) -> Vaddr {
    if <Machine as Pmap>::PMAP_STEAL_MEMORY {
        // defer bootstrap allocation to MD code (it may want to allocate from a direct-mapped
        // segment). pmap_steal_memory should round off virtual_space_start/virtual_space_end.
        let mut start = Vaddr::new(VIRTUAL_SPACE_START.load(Ordering::Relaxed));
        let mut end = Vaddr::new(VIRTUAL_SPACE_END.load(Ordering::Relaxed));
        // SAFETY: `uvm_pageboot_alloc` is only called before `uvm.page_init_done` (the C
        // panics in `uvm_page_physget` otherwise), on the boot CPU.
        let addr = unsafe { Machine::pmap_steal_memory(size, Some(&mut start), Some(&mut end)) };
        VIRTUAL_SPACE_START.store(start.as_usize(), Ordering::Relaxed);
        VIRTUAL_SPACE_END.store(end.as_usize(), Ordering::Relaxed);
        return addr;
    }

    static INITIALIZED: core::sync::atomic::AtomicBool = core::sync::atomic::AtomicBool::new(false);

    // round to page size
    let size = size.round_page();

    // on first call to this function, initialize ourselves.
    if !INITIALIZED.swap(true, Ordering::Relaxed) {
        let mut start = Vaddr::new(0);
        let mut end = Vaddr::new(0);
        Machine::pmap_virtual_space(&mut start, &mut end);

        // round it the way we like it
        VIRTUAL_SPACE_START.store(start.round_page().as_usize(), Ordering::Relaxed);
        VIRTUAL_SPACE_END.store(end.trunc_page().as_usize(), Ordering::Relaxed);
    }

    // allocate virtual memory for this request
    let vs_start = VIRTUAL_SPACE_START.load(Ordering::Relaxed);
    let vs_end = VIRTUAL_SPACE_END.load(Ordering::Relaxed);
    if vs_start == vs_end || vs_end - vs_start < size.as_usize() {
        #[allow(clippy::panic)] // the C panics here too
        {
            panic!("uvm_pageboot_alloc: out of virtual space");
        }
    }

    let addr = vs_start;

    // PMAP_GROWKERNEL: if the kernel pmap can't map the requested space, then allocate more
    // resources for it.
    if UVM_MAXKADDR.load(Ordering::Relaxed) < addr + size.as_usize() {
        let reached = Machine::pmap_growkernel(Vaddr::new(addr + size.as_usize()));
        UVM_MAXKADDR.store(reached.as_usize(), Ordering::Relaxed);
        if reached.as_usize() < addr + size.as_usize() {
            #[allow(clippy::panic)] // the C panics here too
            {
                panic!("uvm_pageboot_alloc: pmap_growkernel() failed");
            }
        }
    }

    VIRTUAL_SPACE_START.store(vs_start + size.as_usize(), Ordering::Relaxed);

    // allocate and mapin physical pages to back new virtual pages
    let mut vaddr = round_page(addr);
    while vaddr < addr + size.as_usize() {
        let Some(paddr) = uvm_page_physget() else {
            #[allow(clippy::panic)] // the C panics here too
            {
                panic!("uvm_pageboot_alloc: out of memory");
            }
        };

        // Note this memory is no longer managed, so using pmap_kenter is safe.
        // SAFETY: the virtual range was just carved out of the free kernel space above, and the
        // page was just taken from the unmanaged part of a segment.
        unsafe {
            Machine::pmap_kenter_pa(
                Vaddr::new(vaddr),
                paddr,
                crate::sys::mman::PROT_READ | crate::sys::mman::PROT_WRITE,
            )
        };
        vaddr += PAGE_SIZE;
    }
    Machine::pmap_update(Machine::pmap_kernel());
    Vaddr::new(addr)
}

/// Removes segment `lcv` from `vm_physmem[]`, shifting the later ones down (the C's
/// `seg[0] = seg[1]` loop).
fn vm_physmem_remove(segs: &mut [VmPhysseg; VM_PHYSSEG_MAX], lcv: usize) {
    let n = VM_NPHYSSEG.load(Ordering::Relaxed);
    if n == 1 {
        #[allow(clippy::panic)] // the C panics here too
        {
            panic!("uvm_page_physget: out of memory!");
        }
    }
    VM_NPHYSSEG.store(n - 1, Ordering::Relaxed);
    for i in lcv..n - 1 {
        segs[i] = segs[i + 1]; // structure copy
    }
}

/// `uvm_page_physget`: "steal" one page from the vm_physmem structure. Attempts to allocate it
/// off the end of a segment in which the "avail" values match the start/end values. If we can't
/// do that, then we will advance both values (making them equal, and removing some vm_page
/// structures from the non-avail area). `None` if out of memory.
pub fn uvm_page_physget() -> Option<Paddr> {
    if UVM.page_init_done.load(Ordering::Relaxed) {
        #[allow(clippy::panic)] // the C panics here too
        {
            panic!("uvm_page_physget: called _after_ bootstrap");
        }
    }
    // SAFETY: before page_init_done, on the boot CPU; no `vm_physmem()` slice is live.
    let segs = unsafe { vm_physmem_mut() };
    let order = || {
        let n = VM_NPHYSSEG.load(Ordering::Relaxed);
        let forward = !matches!(VM_PHYSSEG_STRAT, VM_PSTRAT_BIGFIRST | VM_PSTRAT_BSEARCH);
        (0..n).map(move |i| if forward { i } else { n - 1 - i })
    };

    // pass 1: try allocating from a matching end
    for lcv in order() {
        let seg = &mut segs[lcv];
        // try from front
        if seg.avail_start == seg.start && seg.avail_start < seg.avail_end {
            let pa = Paddr::ptoa(seg.avail_start);
            seg.avail_start += 1;
            seg.start += 1;
            // nothing left? nuke it
            if seg.avail_start == seg.end {
                vm_physmem_remove(segs, lcv);
            }
            return Some(pa);
        }

        // try from rear
        if seg.avail_end == seg.end && seg.avail_start < seg.avail_end {
            let pa = Paddr::ptoa(seg.avail_end - 1);
            seg.avail_end -= 1;
            seg.end -= 1;
            // nothing left? nuke it
            if seg.avail_end == seg.start {
                vm_physmem_remove(segs, lcv);
            }
            return Some(pa);
        }
    }

    // pass2: forget about matching ends, just allocate something
    for lcv in order() {
        let seg = &mut segs[lcv];
        // any room in this bank?
        if seg.avail_start >= seg.avail_end {
            continue; // nope
        }

        let pa = Paddr::ptoa(seg.avail_start);
        seg.avail_start += 1;
        // truncate!
        seg.start = seg.avail_start;

        // nothing left? nuke it
        if seg.avail_start == seg.end {
            vm_physmem_remove(segs, lcv);
        }
        return Some(pa);
    }

    None // whoops!
}

/// Takes `npg` contiguous page frames out of `vm_physmem[]`, at an unused boundary (the start
/// or the end) of the first segment with room; a segment used up on the way is removed.
///
/// Not a function of the C: it is the `vm_physmem[]` half of amd64's `pmap_steal_memory`,
/// which every `pmap_steal_memory` working through a direct map shares (amd64, arm64 and the
/// host double), so that it is written once (`docs/ARCHITECTURE.md`, deviations). `None` when
/// no segment has `npg` free frames at a boundary.
pub fn uvm_page_physsteal(npg: usize) -> Option<Paddr> {
    if UVM.page_init_done.load(Ordering::Relaxed) {
        #[allow(clippy::panic)] // as uvm_page_physget
        {
            panic!("uvm_page_physsteal: called _after_ bootstrap");
        }
    }

    // SAFETY: before page_init_done, on the boot CPU; no `vm_physmem()` slice is live.
    let segs = unsafe { vm_physmem_mut() };
    let nseg = VM_NPHYSSEG.load(Ordering::Relaxed);
    let segno = segs[..nseg].iter().position(|seg| {
        seg.avail_end - seg.avail_start >= npg
            // We can only steal at an ``unused'' segment boundary, i.e. either at the start or
            // at the end.
            && (seg.avail_start == seg.start || seg.avail_end == seg.end)
    })?;
    let seg = &mut segs[segno];
    let pa = if seg.avail_start == seg.start {
        let pa = ptoa(seg.avail_start);
        seg.avail_start += npg;
        seg.start += npg;
        pa
    } else {
        let pa = ptoa(seg.avail_end - npg);
        seg.avail_end -= npg;
        seg.end -= npg;
        pa
    };
    // If all the segment has been consumed now, remove it. Note that the crash dump code
    // still knows about it and will dump it correctly.
    if seg.start == seg.end {
        vm_physmem_remove(segs, segno);
    }
    Some(Paddr::new(pa))
}

/// `uvm_page_physload`: load physical memory into VM system. All args are PFs; all pages in
/// start/end get vm_page structures; areas marked by avail_start/avail_end get added to the
/// free page pool; we are limited to `VM_PHYSSEG_MAX` physical memory segments.
pub fn uvm_page_physload(
    start: usize,
    end: usize,
    avail_start: usize,
    avail_end: usize,
    flags: i32,
) {
    #[cfg(feature = "diagnostic")]
    {
        #[allow(clippy::panic)] // the C panics here too
        {
            if UVMEXP.pagesize.load(Ordering::Relaxed) == 0 {
                panic!("uvm_page_physload: page size not set!");
            }
            if start >= end {
                panic!("uvm_page_physload: start >= end");
            }
        }
    }

    let nseg = VM_NPHYSSEG.load(Ordering::Relaxed);

    // do we have room?
    if nseg == VM_PHYSSEG_MAX {
        kprintf!("uvm_page_physload: unable to load physical memory segment\n");
        kprintf!(
            "\t{} segments allocated, ignoring {:#x} -> {:#x}\n",
            VM_PHYSSEG_MAX,
            start,
            end
        );
        kprintf!("\tincrease VM_PHYSSEG_MAX\n");
        return;
    }

    // check to see if this is a "preload" (i.e. uvm_mem_init hasn't been called yet, so malloc
    // is not available).
    let preload = vm_physmem().iter().all(|seg| seg.pgs.is_null());

    // if VM is already running, attempt to malloc() vm_page structures
    let (pgs, npages): (*const VmPage, usize) = if !preload {
        // XXXCDC: need some sort of lockout for this case right now it is only used by devices
        // so it should be alright.
        let npages = end - start; // # of pages
        // pgs = km_alloc(round_page(npages * sizeof(*pgs)), &kv_any, &kp_zero, &kd_waitok)
        let _ = unported!("km_alloc (uvm_page_physload after uvm_init)");
        let _ = npages;
        let _ = (avail_start, avail_end, flags & PHYSLOAD_DEVICE);
        kprintf!("uvm_page_physload: can not malloc vm_page structs for segment\n");
        kprintf!("\tignoring {:#x} -> {:#x}\n", start, end);
        return;
    } else {
        // gcc complains if these don't get init'd
        (ptr::null(), 0)
    };

    // now insert us in the proper place in vm_physmem[]
    // SAFETY: before page_init_done, on the boot CPU; the `vm_physmem()` slice above is dead.
    let segs = unsafe { vm_physmem_mut() };
    let lcv = match VM_PHYSSEG_STRAT {
        VM_PSTRAT_RANDOM => {
            // random: put it at the end (easy!)
            nseg
        }
        VM_PSTRAT_BSEARCH => {
            // sort by address for binary search
            segs[..nseg]
                .iter()
                .position(|seg| start < seg.start)
                .unwrap_or(nseg)
        }
        VM_PSTRAT_BIGFIRST => {
            // sort by largest segment first
            segs[..nseg]
                .iter()
                .position(|seg| (end - start) > (seg.end - seg.start))
                .unwrap_or(nseg)
        }
        _ => {
            #[allow(clippy::panic)] // the C panics here too
            {
                panic!("uvm_page_physload: unknown physseg strategy selected!");
            }
        }
    };
    // move back other entries, if necessary ...
    let mut x = nseg;
    while x > lcv {
        segs[x] = segs[x - 1]; // structure copy
        x -= 1;
    }

    let ps = &mut segs[lcv];
    ps.start = start;
    ps.end = end;
    ps.avail_start = avail_start;
    ps.avail_end = avail_end;
    if preload {
        ps.pgs = ptr::null();
        ps.lastpg = ptr::null();
    } else {
        ps.pgs = pgs;
        ps.lastpg = pgs.wrapping_add(npages - 1);
    }
    VM_NPHYSSEG.store(nseg + 1, Ordering::Relaxed);
}

/// `uvm_page_physdump`: prints the physical memory config (XXXCDC: TMP TMP TMP DEBUG DEBUG
/// DEBUG). Call from DDB.
pub fn uvm_page_physdump() {
    kprintf!(
        "uvm_page_physdump: physical memory config [segs={} of {}]:\n",
        VM_NPHYSSEG.load(Ordering::Relaxed),
        VM_PHYSSEG_MAX
    );
    for seg in vm_physmem() {
        kprintf!(
            "{:#x}->{:#x} [{:#x}->{:#x}]\n",
            seg.start,
            seg.end,
            seg.avail_start,
            seg.avail_end
        );
    }
    kprintf!("STRATEGY = ");
    match VM_PHYSSEG_STRAT {
        VM_PSTRAT_RANDOM => kprintf!("RANDOM\n"),
        VM_PSTRAT_BSEARCH => kprintf!("BSEARCH\n"),
        VM_PSTRAT_BIGFIRST => kprintf!("BIGFIRST\n"),
        _ => kprintf!("<<UNKNOWN>>!!!!\n"),
    };
}

/// `uvm_shutdown`: what the VM does on the way down.
pub fn uvm_shutdown() {
    // UVM_SWAP_ENCRYPT: uvm_swap_finicrypt_all(): not configured.
    smr_flush();
}

/// `uvm_pagealloc_pg`: perform insert of a given page in the specified anon of obj. This is
/// basically `uvm_pagealloc`, but with the page already given.
pub fn uvm_pagealloc_pg(pg: &VmPage, obj: Option<&UvmObject>, off: Voff, anon: Option<&VmAnon>) {
    kassert!(obj.is_none() || anon.is_none());
    kassert!(anon.is_none() || off == 0);
    kassert!(off == trunc_page(off as usize) as Voff);
    kassert!(obj.is_none_or(|obj| uvm_obj_is_dummy(obj) || rw_write_held(obj.vmobjlock())));
    kassert!(anon.is_none_or(|anon| anon.an_lock().is_none_or(rw_write_held)));

    let mut flags = PG_BUSY | PG_FAKE;
    pg.offset.set(off);
    pg.uobject.set(obj.map_or(ptr::null(), ptr::from_ref));
    pg.uanon.set(anon.map_or(ptr::null(), ptr::from_ref));
    kassert!(uvm_page_owner_locked_p(pg, true));
    if let Some(anon) = anon {
        anon.an_page.set(ptr::from_ref(pg));
        flags |= PQ_ANON;
    } else if obj.is_some() {
        uvm_pageinsert(pg);
    }
    pg.set_bits(flags);
    // UVM_PAGE_OWN(pg, "new alloc"): UVM_PAGE_TRKOWN is not configured.
}

/// `uvm_pglistalloc`: allocate a list of pages. Allocated pages are placed at the tail of
/// `rlist`, which is assumed to be properly initialized by the caller.
///
/// - `size`: the size of the allocation, rounded to page size.
/// - `low`, `high`: the allowed allocation range.
/// - `alignment`: memory must be aligned to this power-of-two boundary.
/// - `boundary`: no segment in the allocation may cross this power-of-two boundary (relative
///   to zero).
/// - flags: `UVM_PLA_NOWAIT` fail if allocation fails, `UVM_PLA_WAITOK` wait for memory to
///   become avail, `UVM_PLA_ZERO` return zeroed memory.
#[allow(clippy::too_many_arguments)] // the C signature
pub fn uvm_pglistalloc(
    size: usize,
    low: Paddr,
    high: Paddr,
    alignment: Paddr,
    boundary: Paddr,
    rlist: &Pglist,
    nsegs: i32,
    flags: i32,
) -> Result<(), Errno> {
    let mut alignment = alignment.as_usize();
    let mut boundary = boundary.as_usize();
    kassert!(alignment & alignment.wrapping_sub(1) == 0);
    kassert!(boundary & boundary.wrapping_sub(1) == 0);
    kassert!((flags & UVM_PLA_WAITOK == 0) ^ (flags & UVM_PLA_NOWAIT == 0));

    if size == 0 {
        return Err(Errno::EINVAL);
    }
    let size = atop(round_page(size));

    // XXX uvm_pglistalloc is currently only used for kernel objects. Unlike the checks in
    // uvm_pagealloc, below, here we are always allowed to use the kernel reserve.
    let flags = flags | UVM_PLA_USERESERVE;

    if high.as_usize() & PAGE_MASK != PAGE_MASK {
        kprintf!(
            "uvm_pglistalloc: Upper boundary {:#x} not on pagemask.\n",
            high.as_usize()
        );
    }

    // Our allocations are always page granularity, so our alignment must be, too.
    if alignment < PAGE_SIZE {
        alignment = PAGE_SIZE;
    }

    let low = atop(low.as_usize().div_ceil(alignment) * alignment);
    // high + 1 may result in overflow, in which case high becomes 0x0, which is the 'don't
    // care' value. The only requirement in that case is that low is also 0x0, or the low<high
    // assert will fail.
    let high = atop(high.as_usize().wrapping_add(1));
    let alignment = atop(alignment);
    if boundary < PAGE_SIZE && boundary != 0 {
        boundary = PAGE_SIZE;
    }
    let boundary = atop(boundary);

    uvm_pmr_getpages(size, low, high, alignment, boundary, nsegs, flags, rlist)
}

/// `uvm_pglistfree`: free a list of pages. Pages should already be unmapped.
pub fn uvm_pglistfree(list: &Pglist) {
    uvm_pmr_freepageq(list);
}

/// `uvm_pagealloc_multi`: interface used by the buffer cache to allocate a buffer at a time.
pub fn uvm_pagealloc_multi(
    obj: &UvmObject,
    off: Voff,
    size: usize,
    flags: i32,
) -> Result<(), Errno> {
    kassert!(crate::uvm::uvm_object::uvm_obj_is_bufcache(obj));
    kernel_assert_locked();
    let plist = Pglist::new();
    plist.init();
    let r = uvm_pglistalloc(
        size,
        crate::uvm::uvm_km::NO_CONSTRAINT.ucr_low,
        crate::uvm::uvm_km::NO_CONSTRAINT.ucr_high,
        Paddr::new(0),
        Paddr::new(0),
        &plist,
        // No MMU (the host double): the buffer cache reaches the pages through the direct
        // map, so they must be one segment (see the module's deviations).
        if <crate::machine::Machine as crate::machine::pmap::Pmap>::PMAP_NOMMU {
            1
        } else {
            atop(round_page(size)) as i32
        },
        flags,
    );
    if r.is_ok() {
        let mut i = 0;
        while let Some(pg) = plist.first() {
            pg.wire_count.set(1);
            pg.set_bits(PG_CLEAN | PG_FAKE);
            kassert!(pg.flags() & PG_DEV == 0);
            // SAFETY: `pg` is the head of `plist`.
            unsafe { plist.remove(pg) };
            uvm_pagealloc_pg(pg, Some(obj), off + ptoa(i) as Voff, None);
            i += 1;
        }
    }
    r
}

/// `uvm_pagealloc`: allocate vm_page from a particular free list. Returns `None` if no pages
/// free; only one of obj or anon can be non-null; caller must activate/deactivate page if it is
/// not wired.
pub fn uvm_pagealloc(
    obj: Option<&UvmObject>,
    off: Voff,
    anon: Option<&VmAnon>,
    flags: i32,
) -> Option<&'static VmPage> {
    kassert!(obj.is_none() || anon.is_none());
    kassert!(anon.is_none() || off == 0);
    kassert!(off == trunc_page(off as usize) as Voff);
    kassert!(obj.is_none_or(|obj| uvm_obj_is_dummy(obj) || rw_write_held(obj.vmobjlock())));
    kassert!(anon.is_none_or(|anon| anon.an_lock().is_none_or(rw_write_held)));

    let mut pmr_flags = UVM_PLA_NOWAIT;

    // We're allowed to use the kernel reserve if the page is being allocated to a kernel
    // object.
    if flags & UVM_PGA_USERESERVE != 0 || obj.is_some_and(uvm_obj_is_kern_object) {
        pmr_flags |= UVM_PLA_USERESERVE;
    }

    if flags & UVM_PGA_ZERO != 0 {
        pmr_flags |= UVM_PLA_ZERO;
    }

    let pg = uvm_pmr_cache_get(pmr_flags)?;
    uvm_pagealloc_pg(pg, obj, off, anon);
    kassert!(pg.flags() & PG_DEV == 0);
    if flags & UVM_PGA_ZERO != 0 {
        pg.clear_bits(PG_CLEAN);
    } else {
        pg.set_bits(PG_CLEAN);
    }

    Some(pg)
}

/// `uvm_pagerealloc`: reallocate a page from one object to another.
pub fn uvm_pagerealloc(pg: &VmPage, newobj: Option<&UvmObject>, newoff: Voff) {
    // remove it from the old object
    if pg.uobject().is_some() {
        uvm_pageremove(pg);
    }

    // put it in the new object
    if let Some(newobj) = newobj {
        pg.uobject.set(ptr::from_ref(newobj));
        pg.offset.set(newoff);
        pg.pg_version.set(pg.pg_version.get().wrapping_add(1));
        uvm_pageinsert(pg);
    }
}

/// `uvm_pageclean`: clean page. Erases the page's identity (i.e. removes it from its object);
/// assumes all valid mappings of pg are gone.
pub fn uvm_pageclean(pg: &VmPage) {
    kassert!(pg.flags() & PG_DEV == 0);
    kassert!(
        pg.uobject()
            .is_none_or(|obj| uvm_obj_is_dummy(obj) || rw_write_held(obj.vmobjlock()))
    );
    kassert!(
        pg.uobject().is_some()
            || pg
                .uanon()
                .is_none_or(|anon| anon.an_lock().is_some_and(rw_write_held))
    );

    // if the page was an object page (and thus "TABLED"), remove it from the object.
    if pg.flags() & PG_TABLED != 0 {
        uvm_pageremove(pg);
    }

    // now remove the page from the queues
    if pg.flags() & (PQ_ACTIVE | PQ_INACTIVE) != 0 {
        uvm_lock_pageq();
        uvm_pagedequeue(pg);
        uvm_unlock_pageq();
    }

    // if the page was wired, unwire it now.
    if pg.wire_count.get() != 0 {
        pg.wire_count.set(0);
        UVMEXP.wired.fetch_sub(1, Ordering::Relaxed);
    }
    if let Some(anon) = pg.uanon() {
        anon.an_page.set(ptr::null());
        pg.uanon.set(ptr::null());
    }

    // Clean page state bits.
    let flags_to_clear = PQ_ANON
        | PQ_AOBJ
        | PQ_ENCRYPT
        | PG_ZERO
        | PG_FAKE
        | PG_BUSY
        | PG_RELEASED
        | PG_CLEAN
        | PG_CLEANCHK;
    pg.clear_bits(flags_to_clear);

    // DEBUG: the 0xdeadbeef poisoning of uobject/offset/uanon is not compiled in.
}

/// `uvm_pagefree`: free page. Erases the page's identity, puts it on the free list; caller must
/// lock page queues if `pg` is managed; assumes all valid mappings of pg are gone.
pub fn uvm_pagefree(pg: &VmPage) {
    uvm_pageclean(pg);
    uvm_pmr_cache_put(pg);
}

/// `uvm_page_unbusy`: unbusy an array of pages. Pages must either all belong to the same
/// object, or all belong to anons; if pages are object-owned, object must be locked; if pages
/// are anon-owned, anons must have 0 refcount; caller must make sure that anon-owned pages are
/// not `PG_RELEASED`.
pub fn uvm_page_unbusy(pgs: &[Option<&VmPage>]) {
    for pg in pgs.iter().flatten() {
        kassert!(uvm_page_owner_locked_p(pg, true));
        kassert!(pg.flags() & PG_BUSY != 0);

        if pg.flags() & PG_WANTED != 0 {
            crate::kern::kern_synch::wakeup(ptr::from_ref(*pg));
        }
        if pg.flags() & PG_RELEASED != 0 {
            kassert!(pg.uobject().is_some() || pg.uanon().is_some_and(|a| a.an_ref.get() > 0));
            pg.clear_bits(PG_WANTED);
            Machine::pmap_page_protect(pg, PROT_NONE);
            uvm_pagefree(pg);
        } else {
            kassert!(pg.flags() & PG_FAKE == 0);
            pg.clear_bits(PG_WANTED | PG_BUSY);
            // UVM_PAGE_OWN(pg, NULL): not configured.
        }
    }
}

/// `uvm_pagewait`: wait for a busy page. Page must be known `PG_BUSY`; object must be locked;
/// object will be unlocked on return.
pub fn uvm_pagewait(pg: &VmPage, lock: &Rwlock, wmesg: &'static str) {
    kassert!(rw_lock_held(lock));
    kassert!(pg.flags() & PG_BUSY != 0);
    kassert!(uvm_page_owner_locked_p(pg, false));

    pg.set_bits(PG_WANTED);
    let _ = rwsleep_nsec(ptr::from_ref(pg), lock, PVM | PNORELOCK, wmesg, INFSLP);
}

/// `vm_physseg_find`: find the vm_physseg structure that belongs to a PA: the segment index
/// and the offset of the frame inside it.
pub fn vm_physseg_find(pframe: usize) -> Option<(usize, usize)> {
    let segs = vm_physmem();
    if VM_PHYSSEG_STRAT == VM_PSTRAT_BSEARCH {
        // binary search for it
        //
        // if try is too large (thus target is less than try) we reduce the length to
        // trunc(len/2) [i.e. everything smaller than "try"]
        //
        // if the try is too small (thus target is greater than try) then we set the new start
        // to be (try + 1). this means we need to reduce the length to (round(len/2) - 1).
        //
        // note "adjust" below which takes advantage of the fact that
        //  (round(len/2) - 1) == trunc((len - 1) / 2)
        // for any value of len we may have
        let mut start = 0;
        let mut len = segs.len();
        while len != 0 {
            let r#try = start + (len / 2); // try in the middle
            let seg = &segs[r#try];

            // start past our try?
            if pframe >= seg.start {
                // was try correct?
                if pframe < seg.end {
                    return Some((r#try, pframe - seg.start)); // got it
                }
                start = r#try + 1; // next time, start here
                len -= 1; // "adjust"
            }
            // else: pframe before try, just reduce length of region, done in "for" loop
            len /= 2;
        }
        None
    } else {
        // linear search for it
        segs.iter()
            .position(|seg| pframe >= seg.start && pframe < seg.end)
            .map(|lcv| (lcv, pframe - segs[lcv].start)) // got it
    }
}

/// `PHYS_TO_VM_PAGE`: find vm_page for a PA. Used by MI code to get vm_pages back from an I/O
/// mapping (ugh!); used in some MD code as well.
#[allow(non_snake_case)] // the C name, used in MD code too
pub fn PHYS_TO_VM_PAGE(pa: Paddr) -> Option<&'static VmPage> {
    let pf = pa.atop();
    let (psi, off) = vm_physseg_find(pf)?;
    let seg = &vm_physmem()[psi];
    if seg.pgs.is_null() {
        return None;
    }
    // SAFETY: `off < end - start`, and the array is set once `uvm_page_init` ran.
    Some(unsafe { seg.page(off) })
}

/// `uvm_pagelookup`: look up a page.
pub fn uvm_pagelookup(obj: &UvmObject, off: Voff) -> Option<&VmPage> {
    // XXX if stack is too much, handroll
    let p = VmPage::new(Paddr::new(0));
    p.offset.set(off);
    let pg = obj.memt.find(&p);

    kassert!(pg.is_none() || obj.uo_npages.get() != 0);
    kassert!(pg.is_none_or(|pg| pg.flags() & PG_RELEASED == 0 || pg.flags() & PG_BUSY != 0));
    pg
}

/// `uvm_pagewire`: wire the page, thus removing it from the daemon's grasp.
pub fn uvm_pagewire(pg: &VmPage) {
    kassert!(uvm_page_owner_locked_p(pg, true));

    if pg.wire_count.get() == 0 {
        uvm_lock_pageq();
        uvm_pagedequeue(pg);
        uvm_unlock_pageq();
        UVMEXP.wired.fetch_add(1, Ordering::Relaxed);
    }
    kassert!(pg.flags() & (PQ_INACTIVE | PQ_ACTIVE) == 0);
    pg.wire_count.set(pg.wire_count.get() + 1);
    kassert!(pg.wire_count.get() > 0); // detect wraparound
}

/// `uvm_pageunwire`: unwire the page. Activates it if the wire count goes to zero.
pub fn uvm_pageunwire(pg: &VmPage) {
    kassert!(uvm_page_owner_locked_p(pg, true));
    kassert!(pg.wire_count.get() != 0);

    pg.wire_count.set(pg.wire_count.get() - 1);
    if pg.wire_count.get() == 0 {
        uvm_pageactivate(pg);
        UVMEXP.wired.fetch_sub(1, Ordering::Relaxed);
    }
}

/// `uvm_pagedeactivate`: deactivate page (unless wired). Object that page belongs to must be
/// locked.
pub fn uvm_pagedeactivate(pg: &VmPage) {
    kassert!(uvm_page_owner_locked_p(pg, false));

    if pg.wire_count.get() > 0 {
        kassert!(pg.flags() & (PQ_INACTIVE | PQ_ACTIVE) == 0);
        return;
    }

    uvm_lock_pageq();
    if pg.flags() & PQ_INACTIVE != 0 {
        uvm_unlock_pageq();
        return;
    }

    // Make sure next access to this page will fault.
    Machine::pmap_page_protect(pg, PROT_NONE);

    uvm_pagedequeue(pg);
    // SAFETY: the page was dequeued just above, so it is on no queue.
    unsafe { UVM.page_inactive.insert_tail(pg) };
    pg.set_bits(PQ_INACTIVE);
    UVMEXP.inactive.fetch_add(1, Ordering::Relaxed);
    uvm_unlock_pageq();

    let _ = crate::machine::pmap::pmap_clear_reference(pg);
    // update the "clean" bit. this isn't 100% accurate, and doesn't have to be. we'll re-sync
    // it after we zap all mappings when scanning the inactive list.
    if pg.flags() & PG_CLEAN != 0 && crate::machine::pmap::pmap_is_modified(pg) {
        pg.clear_bits(PG_CLEAN);
    }
}

/// `uvm_pageactivate`: activate page (unless wired).
pub fn uvm_pageactivate(pg: &VmPage) {
    kassert!(uvm_page_owner_locked_p(pg, false));

    if pg.wire_count.get() > 0 {
        kassert!(pg.flags() & (PQ_INACTIVE | PQ_ACTIVE) == 0);
        return;
    }

    uvm_lock_pageq();
    uvm_pagedequeue(pg);
    // SAFETY: the page was dequeued just above, so it is on no queue.
    unsafe { UVM.page_active.insert_tail(pg) };
    pg.set_bits(PQ_ACTIVE);
    UVMEXP.active.fetch_add(1, Ordering::Relaxed);
    uvm_unlock_pageq();
}

/// `uvm_pagedequeue`: remove a page from any paging queue.
pub fn uvm_pagedequeue(pg: &VmPage) {
    kassert!(uvm_page_owner_locked_p(pg, false));
    mutex_assert_locked(&UVM.pageqlock, "uvm_pagedequeue");
    kassert!(pg.wire_count.get() == 0);

    if pg.flags() & PQ_ACTIVE != 0 {
        // SAFETY: PQ_ACTIVE says the page is on the active queue.
        unsafe { UVM.page_active.remove(pg) };
        pg.clear_bits(PQ_ACTIVE);
        UVMEXP.active.fetch_sub(1, Ordering::Relaxed);
    }
    if pg.flags() & PQ_INACTIVE != 0 {
        // SAFETY: PQ_INACTIVE says the page is on the inactive queue.
        unsafe { UVM.page_inactive.remove(pg) };
        pg.clear_bits(PQ_INACTIVE);
        UVMEXP.inactive.fetch_sub(1, Ordering::Relaxed);
    }
}

/// `uvm_pagezero`: zero fill a page.
pub fn uvm_pagezero(pg: &VmPage) {
    pg.clear_bits(PG_CLEAN);
    Machine::pmap_zero_page(pg);
}

/// `uvm_pagecopy`: copy a page.
pub fn uvm_pagecopy(src: &VmPage, dst: &VmPage) {
    dst.clear_bits(PG_CLEAN);
    Machine::pmap_copy_page(src, dst);
}

/// `uvm_page_owner_locked_p`: return true if object associated with page is locked. This is a
/// weak check for runtime assertions only.
pub fn uvm_page_owner_locked_p(pg: &VmPage, exclusive: bool) -> bool {
    if let Some(obj) = pg.uobject() {
        if uvm_obj_is_dummy(obj) {
            return true;
        }
        return if exclusive {
            rw_write_held(obj.vmobjlock())
        } else {
            rw_lock_held(obj.vmobjlock())
        };
    }
    if let Some(anon) = pg.uanon() {
        let Some(lock) = anon.an_lock() else {
            panic(format_args!("uvm_page_owner_locked_p: anon without a lock"));
        };
        return if exclusive {
            rw_write_held(lock)
        } else {
            rw_lock_held(lock)
        };
    }
    true
}

/// `uvm_pagecount`: count the number of physical pages in the address range.
pub fn uvm_pagecount(constraint: &UvmConstraintRange) -> usize {
    // Algorithm uses page numbers.
    let low = constraint.ucr_low.atop();
    let high = constraint.ucr_high.atop();

    vm_physmem()
        .iter()
        .map(|seg| {
            let ps_low = low.max(seg.avail_start);
            let ps_high = high.min(seg.avail_end);
            ps_high.saturating_sub(ps_low)
        })
        .sum()
}

/// Forgets every segment and counter, so a host test can set up its own memory. Tests only.
#[cfg(test)]
pub fn uvm_page_test_reset() {
    // SAFETY: tests holding `crate::uvm::uvm_pmemrange::tests::LOCK` are the only users.
    unsafe { *vm_physmem_mut() = [VmPhysseg::EMPTY; VM_PHYSSEG_MAX] };
    VM_NPHYSSEG.store(0, Ordering::Relaxed);
    UVM.page_init_done.store(false, Ordering::Relaxed);
    UVMEXP.npages.store(0, Ordering::Relaxed);
    UVMEXP.free.store(0, Ordering::Relaxed);
    UVMEXP.zeropages.store(0, Ordering::Relaxed);
}
/* </CODE> */
