/*	$OpenBSD: uvm_pager.h,v 1.35 2026/07/07 17:32:56 kettenis Exp $	*/
/*	$NetBSD: uvm_pager.h,v 1.20 2000/11/27 08:40:05 chs Exp $	*/
/*	$OpenBSD: uvm_pager.c,v 1.99 2026/07/11 13:13:16 kettenis Exp $	*/
/*	$NetBSD: uvm_pager.c,v 1.36 2000/11/27 18:26:41 chs Exp $	*/
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
 * from: Id: uvm_pager.h,v 1.1.2.14 1998/01/13 19:00:50 chuck Exp
 */

/*
 * Copyright (c) 1990 University of Utah.
 * Copyright (c) 1991, 1993
 *	The Regents of the University of California.  All rights reserved.
 *
 * This code is derived from software contributed to Berkeley by
 * the Systems Programming Group of the University of Utah Computer
 * Science Department.
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
 *	@(#)vm_pager.h	8.5 (Berkeley) 7/7/94
 */
/* </LICENSES> */

/* <CODE> */
//! `<uvm/uvm_pager.h>`: the pager operations every memory object implements, and
//! `uvm_pager.c`: generic functions used to assist the pagers.
//!
//! Upstream: sys/uvm/uvm_pager.h @ 3ce1f3f79392
//! Upstream: sys/uvm/uvm_pager.c @ 3ce1f3f79392
//!
//! Status: `wip`. Milestone M7a ports `struct uvm_pagerops`, the `PGO_*` flags, the
//! `VM_PAGER_*` results, `PGO_DONTCARE` and `uvm_pager_init`'s call of the pagers' init
//! functions; the vnode pager (VFS stage 2) the pager map (`uvm_pseg_*`,
//! `uvm_pagermapin`/`out`) and the cluster building (`uvm_mk_pcluster`, `uvm_pager_put`,
//! `uvm_pager_dropcluster`). The async pageout (`uvm_aio_biodone`, `uvm_swap_dropcluster`,
//! `uvm_aio_aiodone`, `uvm.aio_done`) is the swap pager's (M7) and not here.
//!
//! ## Deviations
//! - `pgo_get`'s page array is `&mut [*const VmPage]`: the C passes `vm_page_t *` with the
//!   sentinel `PGO_DONTCARE` (`(struct vm_page *)-1`) for pages the caller does not want,
//!   which a `Option<&VmPage>` cannot carry; [`pgo_dontcare`] tests for it.
//! - Every operation is an `Option<fn>`: the C leaves unimplemented ones NULL.
//! - `pgo_put`'s last argument is the `PGO_*` flags as an `i32`: the C's prototype says
//!   `boolean_t`, but `uvm_pager_put` passes its flags and `uvn_put` reads them.
//! - `pgo_mk_pcluster` and `uvm_mk_pcluster` return the index in `pps` where the cluster
//!   starts, and `uvm_pager_put` takes `pps` and a `&mut usize` for the C's `struct vm_page
//!   ***ppsp_ptr`.
//! - `uvmpagerops[]` has the aobj and vnode pagers; the device pager (`uvm_device.c`) is not
//!   ported. Nothing is the page daemon yet (`uvm_pdaemon.c`), so `uvm_pseg_get` never digs
//!   into the reserve and `uvm_pseg_reserve_available` asserts a caller that cannot exist.

use core::cell::Cell;
use core::ptr::{self, NonNull};

use crate::kern::kern_lock::{mtx_enter, mtx_init, mtx_leave};

use crate::kern::kern_synch::{msleep_nsec, wakeup};
use crate::kern::subr_prf::panic;
use crate::machine::Machine;
use crate::machine::intr::IPL_VM;
use crate::machine::pmap::{
    Pmap, pmap_clear_modify, pmap_clear_reference, pmap_enter, pmap_is_modified, pmap_kernel,
    pmap_map_direct, pmap_page_protect, pmap_remove, pmap_unmap_direct, pmap_update,
};
use crate::sys::mman::{PROT_READ, PROT_WRITE};
use crate::sys::mutex::Mutex;
use crate::sys::param::{MAXBSIZE, PAGE_SHIFT, PAGE_SIZE, PVM};
use crate::sys::rwlock::rw_write_held;
use crate::sys::systm::INFSLP;
use crate::sys::types::Vaddr;
use crate::uvm::uvm_anon::uvm_anon_release;
use crate::uvm::uvm_aobj::AOBJ_PAGER;
use crate::uvm::uvm_extern::{VmFault, VmProt, Voff};
use crate::uvm::uvm_fault::UvmFaultinfo;
use crate::uvm::uvm_km::{KD_TRYLOCK, KP_NONE, KV_ANY, km_alloc, km_free};
use crate::uvm::uvm_object::UvmObject;
use crate::uvm::uvm_page::{
    PG_BUSY, PG_CLEAN, PG_CLEANCHK, PG_FAKE, PG_RELEASED, PG_WANTED, PQ_ANON, PQ_INACTIVE, VmPage,
    uvm_pagelookup, uvm_unlock_pageq, vm_page_to_phys,
};
use crate::uvm::uvm_param::ptoa;
use crate::uvm::uvm_pdaemon::uvm_wait;
use crate::uvm::uvm_pmap::{PMAP_CANFAIL, PMAP_WIRED, pmap_prefer_align};
use crate::uvm::uvm_vnode::UVM_VNODEOPS;
use crate::{kassert, kdassert};

/// `pgo_fault`'s signature: `(ufi, vaddr, pps, npages, centeridx, fault_type, access_type,
/// flags)`.
pub type PgoFault =
    fn(&mut UvmFaultinfo, Vaddr, &mut [*const VmPage], i32, i32, VmFault, VmProt, i32) -> i32;
/// `pgo_flush`'s signature: `(uobj, start, stop, flags)`.
pub type PgoFlush = fn(&UvmObject, Voff, Voff, i32) -> bool;
/// `pgo_get`'s signature: `(uobj, offset, pps, npagesp, centeridx, access_type, advice,
/// flags)`.
pub type PgoGet =
    fn(&UvmObject, Voff, &mut [*const VmPage], &mut i32, i32, VmProt, i32, i32) -> i32;
/// `pgo_put`'s signature: `(uobj, pps, npages, flags)`.
pub type PgoPut = fn(&UvmObject, &mut [*const VmPage], i32, i32) -> i32;
/// `pgo_cluster`'s signature: `(uobj, offset, loffset, hoffset)`.
pub type PgoCluster = fn(&UvmObject, Voff, &mut Voff, &mut Voff);
/// `pgo_mk_pcluster`'s signature: `(uobj, pps, npages, center, flags, mlo, mhi)`.
pub type PgoMkPcluster =
    fn(&UvmObject, &mut [*const VmPage], &mut i32, &VmPage, i32, Voff, Voff) -> usize;

/// `struct uvm_pagerops`.
pub struct UvmPagerops {
    /// `pgo_init`: init pager.
    pub pgo_init: Option<fn()>,
    /// `pgo_reference`: add reference to obj.
    pub pgo_reference: Option<fn(&UvmObject)>,
    /// `pgo_detach`: drop reference to obj.
    pub pgo_detach: Option<fn(&UvmObject)>,
    /// `pgo_fault`: special nonstd fault fn.
    pub pgo_fault: Option<PgoFault>,
    /// `pgo_flush`: flush pages out of obj.
    pub pgo_flush: Option<PgoFlush>,
    /// `pgo_get`: get/read page.
    pub pgo_get: Option<PgoGet>,
    /// `pgo_put`: put/write page.
    pub pgo_put: Option<PgoPut>,
    /// `pgo_cluster`: return range of cluster.
    pub pgo_cluster: Option<PgoCluster>,
    /// `pgo_mk_pcluster`: make "put" cluster.
    pub pgo_mk_pcluster: Option<PgoMkPcluster>,
}

impl UvmPagerops {
    /// A pager with no operations (the C's `{ /* nothing */ }`).
    pub const fn empty() -> Self {
        Self {
            pgo_init: None,
            pgo_reference: None,
            pgo_detach: None,
            pgo_fault: None,
            pgo_flush: None,
            pgo_get: None,
            pgo_put: None,
            pgo_cluster: None,
            pgo_mk_pcluster: None,
        }
    }
}

// pager flags [mostly for flush]

/// `PGO_CLEANIT`: write dirty pages to backing store.
pub const PGO_CLEANIT: i32 = 0x001;
/// `PGO_SYNCIO`: if PGO_CLEANIT: use sync I/O?
pub const PGO_SYNCIO: i32 = 0x002;
/// `PGO_DEACTIVATE`: deactivate flushed pages.
pub const PGO_DEACTIVATE: i32 = 0x004;
/// `PGO_FREE`: free flushed pages (if not set then the pages stay where they are).
pub const PGO_FREE: i32 = 0x008;
/// `PGO_ALLPAGES`: flush whole object/get all pages.
pub const PGO_ALLPAGES: i32 = 0x010;
/// `PGO_DOACTCLUST`: flag to mk_pcluster to include active.
pub const PGO_DOACTCLUST: i32 = 0x020;
/// `PGO_LOCKED`: fault data structures are locked \[get\].
pub const PGO_LOCKED: i32 = 0x040;
/// `PGO_PDFREECLUST`: daemon's free cluster flag \[uvm_pager_put\].
pub const PGO_PDFREECLUST: i32 = 0x080;
/// `PGO_NOWAIT`: do not wait for inode lock.
pub const PGO_NOWAIT: i32 = 0x200;

/// `PGO_DONTCARE`: page we are not interested in getting \[get only\].
pub const PGO_DONTCARE: *const VmPage = usize::MAX as *const VmPage;

/// Whether a `pgo_get` slot is `PGO_DONTCARE`.
pub fn pgo_dontcare(p: *const VmPage) -> bool {
    ptr::eq(p, PGO_DONTCARE)
}

/// `UVMPAGER_MAPIN_WAITOK`: it's okay to wait.
pub const UVMPAGER_MAPIN_WAITOK: i32 = 0x01;
/// `UVMPAGER_MAPIN_READ`: host <- device.
pub const UVMPAGER_MAPIN_READ: i32 = 0x02;
/// `UVMPAGER_MAPIN_WRITE`: device -> host (pseudo flag).
pub const UVMPAGER_MAPIN_WRITE: i32 = 0x00;

// get/put return values

/// `VM_PAGER_OK`: operation was successful.
pub const VM_PAGER_OK: i32 = 0;
/// `VM_PAGER_BAD`: specified data was out of the accepted range.
pub const VM_PAGER_BAD: i32 = 1;
/// `VM_PAGER_FAIL`: specified data was in range, but doesn't exist.
pub const VM_PAGER_FAIL: i32 = 2;
/// `VM_PAGER_PEND`: operations was initiated but not completed.
pub const VM_PAGER_PEND: i32 = 3;
/// `VM_PAGER_ERROR`: error while accessing data that is in range and exists.
pub const VM_PAGER_ERROR: i32 = 4;
/// `VM_PAGER_AGAIN`: temporary resource shortage prevented operation from happening.
pub const VM_PAGER_AGAIN: i32 = 5;
/// `VM_PAGER_UNLOCK`: unlock the map and try again.
pub const VM_PAGER_UNLOCK: i32 = 6;
/// `VM_PAGER_REFAULT`: \[uvm_fault internal use only!\] unable to relock data structures,
/// thus the mapping needs to be reverified before we can proceed.
pub const VM_PAGER_REFAULT: i32 = 7;

/// `PAGER_MAP_SIZE`: XXX this is needed until the device strategy interface is changed to do
/// physically-addressed i/o.
pub const PAGER_MAP_SIZE: usize = 16 * 1024 * 1024;

/// `uvmpagerops[]`: the pagers (the device pager, `uvm_device.c`, does not exist yet).
static UVMPAGEROPS: [&UvmPagerops; 2] = [&AOBJ_PAGER, &UVM_VNODEOPS];

// the pager map: provides KVA for I/O
//
// Each uvm_pseg has room for MAX_PAGERMAP_SEGS pager io space of MAXBSIZE bytes.
//
// The number of uvm_pseg instances is dynamic using an array segs. At most UVM_PSEG_COUNT
// instances can exist.
//
// psegs[0/1] always exist (so that the pager can always map in pages). psegs[0/1] element 0/1
// are always reserved for the pagedaemon.
//
// Any other pseg is automatically created when no space is available and automatically
// destroyed when it is no longer in use.

/// `MAX_PAGER_SEGS`.
pub const MAX_PAGER_SEGS: usize = 16;
/// `PSEG_NUMSEGS`.
pub const PSEG_NUMSEGS: usize = PAGER_MAP_SIZE / MAX_PAGER_SEGS / MAXBSIZE;

/// `struct uvm_pseg`.
///
/// Protected by: `uvm_pseg_lck`.
pub struct UvmPseg {
    /// `start`: start of virtual space; 0 if not inited.
    pub start: Cell<usize>,
    /// `use`: bitmap of the segments in use in this pseg.
    pub use_: Cell<i32>,
}

// SAFETY: changed under `uvm_pseg_lck`, as in C.
unsafe impl Sync for UvmPseg {}

impl UvmPseg {
    /// An uninitialised pseg.
    const fn new() -> Self {
        Self {
            start: Cell::new(0),
            use_: Cell::new(0),
        }
    }

    /// `UVM_PSEG_FULL(pseg)`.
    fn full(&self) -> bool {
        self.use_.get() == (1 << MAX_PAGER_SEGS) - 1
    }

    /// `UVM_PSEG_EMPTY(pseg)`.
    fn empty(&self) -> bool {
        self.use_.get() == 0
    }

    /// `UVM_PSEG_INUSE(pseg, id)`.
    fn inuse(&self, id: usize) -> bool {
        self.use_.get() & (1 << id) != 0
    }
}

/// `uvm_pseg_lck`: the pager map's lock.
pub static UVM_PSEG_LCK: Mutex = Mutex::new(IPL_VM);
/// `psegs[PSEG_NUMSEGS]`.
pub static PSEGS: [UvmPseg; PSEG_NUMSEGS] = [const { UvmPseg::new() }; PSEG_NUMSEGS];

/// `uvm_pager_init`: init pagers (at boot time).
pub fn uvm_pager_init() {
    // init pager map
    uvm_pseg_init(&PSEGS[0]);
    uvm_pseg_init(&PSEGS[1]);
    mtx_init(&UVM_PSEG_LCK, IPL_VM);

    // init ASYNC I/O queue: TAILQ_INIT(&uvm.aio_done): the async pageout path (uvm_aio_*,
    // the swap pager) is not here yet.

    // call pager init functions
    for ops in UVMPAGEROPS {
        if let Some(init) = ops.pgo_init {
            init();
        }
    }
}

/// Initialize a uvm_pseg.
///
/// May fail, in which case seg->start == 0.
///
/// Caller locks uvm_pseg_lck.
pub fn uvm_pseg_init(pseg: &UvmPseg) {
    kassert!(pseg.start.get() == 0);
    kassert!(pseg.use_.get() == 0);
    pseg.start.set(
        km_alloc(MAX_PAGER_SEGS * MAXBSIZE, &KV_ANY, &KP_NONE, &KD_TRYLOCK)
            .map_or(0, |p| p.as_ptr() as usize),
    );
}

/// Whether the caller is the page daemon (`uvm_pdaemon.c` is not here, so never).
fn curproc_is_pagedaemon() -> bool {
    false
}

/// `uvm_pseg_reserve_available()`: the pagedaemon's reserved segments that are free.
pub fn uvm_pseg_reserve_available() -> i32 {
    let mut count = 0;

    kassert!(curproc_is_pagedaemon());

    for pseg in &PSEGS[..2] {
        for j in 0..2 {
            if !pseg.inuse(j) {
                count += 1;
            }
        }
    }

    count
}

/// Acquire a pager map segment.
///
/// Returns a vaddr for paging. 0 on failure.
///
/// Caller does not lock.
pub fn uvm_pseg_get(flags: i32) -> usize {
    let mut use_reserve = false;

    mtx_enter(&UVM_PSEG_LCK);

    'restart: loop {
        // pager_seg_restart:
        let fail = 'search: {
            // Find first pseg that has room.
            for (n, pseg) in PSEGS.iter().enumerate() {
                if pseg.full() {
                    continue;
                }

                if pseg.start.get() == 0 {
                    // Need initialization.
                    uvm_pseg_init(pseg);
                    if pseg.start.get() == 0 {
                        break 'search true;
                    }
                }

                // Keep indexes 0,1 reserved for pagedaemon.
                let first = if n < 2 && !use_reserve { 2 } else { 0 };

                for i in first..MAX_PAGER_SEGS {
                    if !pseg.inuse(i) {
                        pseg.use_.set(pseg.use_.get() | 1 << i);
                        mtx_leave(&UVM_PSEG_LCK);
                        return pseg.start.get() + i * MAXBSIZE;
                    }
                }
            }
            false
        };

        // Let the pagdeamon dig into its reserve if we couldn't get a "normal" slot.
        if !fail && curproc_is_pagedaemon() && !use_reserve {
            use_reserve = true;
            continue 'restart;
        }

        // pager_seg_fail:
        if flags & UVMPAGER_MAPIN_WAITOK != 0 {
            let _ = msleep_nsec(
                ptr::from_ref(&PSEGS),
                &UVM_PSEG_LCK,
                PVM,
                "pagerseg",
                INFSLP,
            );
            continue 'restart;
        }

        mtx_leave(&UVM_PSEG_LCK);
        return 0;
    }
}

/// Release a pager map segment.
///
/// Caller does not lock.
///
/// Deallocates pseg if it is no longer in use.
pub fn uvm_pseg_release(segaddr: usize) {
    let mut va = 0;

    mtx_enter(&UVM_PSEG_LCK);
    let Some((n, pseg)) = PSEGS.iter().enumerate().find(|(_, p)| {
        p.start.get() <= segaddr && segaddr < p.start.get() + MAX_PAGER_SEGS * MAXBSIZE
    }) else {
        panic(format_args!(
            "uvm_pseg_release: {segaddr:#x} is no pager segment"
        ));
    };

    let id = (segaddr - pseg.start.get()) / MAXBSIZE;
    kassert!(id < MAX_PAGER_SEGS);

    // test for no remainder
    kdassert!(segaddr == pseg.start.get() + id * MAXBSIZE);

    kassert!(pseg.inuse(id));

    pseg.use_.set(pseg.use_.get() & !(1 << id));
    wakeup(ptr::from_ref(&PSEGS));

    if n >= 2 && pseg.empty() {
        va = pseg.start.get();
        pseg.start.set(0);
    }

    mtx_leave(&UVM_PSEG_LCK);

    if let Some(va) = NonNull::new(va as *mut u8) {
        km_free(va, MAX_PAGER_SEGS * MAXBSIZE, &KV_ANY, &KP_NONE);
    }
}

/// The page in a slot of a pager's page array, which the C dereferences: not `NULL`, not
/// `PGO_DONTCARE`.
fn pps_page(p: *const VmPage) -> &'static VmPage {
    if p.is_null() || pgo_dontcare(p) {
        panic(format_args!("pager: no page in the slot"));
    }
    // SAFETY: the pagers fill their arrays with pages of the object they hold busy;
    // `vm_page`s live forever in the page array.
    unsafe { &*p }
}

/// uvm_pagermapin: map pages into KVA for I/O that needs mappings
///
/// We basically just km_valloc a blank map entry to reserve the space in the kernel map and
/// then use pmap_enter() to put the mappings in by hand.
pub fn uvm_pagermapin(pps: &[*const VmPage], npages: usize, flags: i32) -> usize {
    // __HAVE_PMAP_DIRECT: use direct mappings for single page, unless there is a risk of
    // aliasing.
    if <Machine as Pmap>::HAVE_PMAP_DIRECT && npages == 1 && pmap_prefer_align() == 0 {
        let pg = pps_page(pps[0]);
        kassert!(pg.flags() & PG_BUSY != 0);
        return pmap_map_direct(pg).as_usize();
    }

    let mut prot = PROT_READ;
    if flags & UVMPAGER_MAPIN_READ != 0 {
        prot |= PROT_WRITE;
    }
    let size = ptoa(npages);

    kassert!(size <= MAXBSIZE);

    let kva = uvm_pseg_get(flags);
    if kva == 0 {
        return 0;
    }

    for (i, &p) in pps[..npages].iter().enumerate() {
        let cva = kva + ptoa(i);
        let pp = pps_page(p);
        kassert!(pp.flags() & PG_BUSY != 0);
        while pmap_enter(
            pmap_kernel(),
            Vaddr::new(cva),
            vm_page_to_phys(pp),
            prot,
            PMAP_WIRED | PMAP_CANFAIL | prot,
        )
        .is_err()
        {
            if flags & UVMPAGER_MAPIN_WAITOK != 0 {
                uvm_wait("pgrmapin");
            } else {
                pmap_remove(pmap_kernel(), Vaddr::new(kva), Vaddr::new(cva));
                pmap_update(pmap_kernel());
                uvm_pseg_release(kva);
                return 0;
            }
        }
    }
    pmap_update(pmap_kernel());
    kva
}

/// uvm_pagermapout: remove KVA mapping
///
/// We remove our mappings by hand and then remove the mapping.
pub fn uvm_pagermapout(kva: usize, npages: usize) {
    // __HAVE_PMAP_DIRECT: use direct mappings for single page, unless there is a risk of
    // aliasing.
    if <Machine as Pmap>::HAVE_PMAP_DIRECT && npages == 1 && pmap_prefer_align() == 0 {
        let _ = pmap_unmap_direct(Vaddr::new(kva));
        return;
    }

    pmap_remove(
        pmap_kernel(),
        Vaddr::new(kva),
        Vaddr::new(kva + ptoa(npages)),
    );
    pmap_update(pmap_kernel());
    uvm_pseg_release(kva);
}

/// uvm_mk_pcluster
///
/// generic "make 'pager put' cluster" function. a pager can either \[1\] set pgo_mk_pcluster
/// to NULL (never cluster), \[2\] set it to this generic function, or \[3\] set it to a pager
/// specific function.
///
/// => caller must lock object _and_ pagequeues (since we need to look at active vs. inactive
///    bits, etc.)
/// => caller must make center page busy and write-protect it
/// => we mark all cluster pages busy for the caller
/// => the caller must unbusy all pages (and check wanted/released status if it drops the
///    object lock)
/// => flags:
///      PGO_ALLPAGES:  all pages in object are valid targets
///      !PGO_ALLPAGES: use "lo" and "hi" to limit range of cluster
///      PGO_DOACTCLUST: include active pages in cluster.
///      PGO_FREE: set the PG_RELEASED bits on the cluster so they'll be freed in async io
///      (caller must clean on error).
///        NOTE: the caller should clear PG_CLEANCHK bits if PGO_DOACTCLUST. PG_CLEANCHK is
///        only a hint, but clearing will help reduce the number of calls we make to the pmap
///        layer.
///
/// Returns the index in `pps` where the cluster starts (the C returns that element's
/// address); `*npages` is its length.
pub fn uvm_mk_pcluster(
    uobj: &UvmObject,
    pps: &mut [*const VmPage],
    npages: &mut i32,
    center: &VmPage,
    flags: i32,
    mlo: Voff,
    mhi: Voff,
) -> usize {
    // center page should already be busy and write protected. XXX: suppose page is wired? if
    // we lock, then a process could fault/block on it. if we don't lock, a process could write
    // the pages in the middle of an I/O. (consider an msync()). let's lock it for now (better
    // to delay than corrupt data?).
    // get cluster boundaries, check sanity, and apply our limits as well.
    let (mut lo, mut hi) = (0, 0);
    let Some(cluster) = uobj.pgops().pgo_cluster else {
        panic(format_args!("uvm_mk_pcluster: pager without pgo_cluster"));
    };
    cluster(uobj, center.offset.get(), &mut lo, &mut hi);
    if flags & PGO_ALLPAGES == 0 {
        lo = lo.max(mlo);
        hi = hi.min(mhi);
    }
    if (hi - lo) >> PAGE_SHIFT > Voff::from(*npages) {
        // pps too small, bail out!
        pps[0] = center;
        *npages = 1;
        return 0;
    }

    // now determine the center and attempt to cluster around the edges
    let center_idx = ((center.offset.get() - lo) >> PAGE_SHIFT) as usize;
    pps[center_idx] = center; // plug in the center page
    let mut ppsp = center_idx;
    *npages = 1;

    // attempt to cluster around the left [backward], and then the right side [forward].
    //
    // note that for inactive pages (pages that have been deactivated) there are no valid
    // mappings and PG_CLEAN should be up to date. [i.e. there is no need to query the pmap
    // with pmap_is_modified since there are no mappings].
    for forward in [false, true] {
        let incr: Voff = if forward {
            PAGE_SIZE as Voff
        } else {
            -(PAGE_SIZE as Voff)
        };
        let mut curoff = center.offset.get() + incr;
        while (!forward && curoff >= lo) || (forward && curoff < hi) {
            let Some(pclust) = uvm_pagelookup(uobj, curoff) else {
                break; // no page
            };
            // handle active pages
            // NOTE: inactive pages don't have pmap mappings
            if pclust.flags() & PQ_INACTIVE == 0 {
                if flags & PGO_DOACTCLUST == 0 {
                    // dont want mapped pages at all
                    break;
                }

                // make sure "clean" bit is sync'd
                if pclust.flags() & PG_CLEANCHK == 0 {
                    if pclust.flags() & (PG_CLEAN | PG_BUSY) == PG_CLEAN && pmap_is_modified(pclust)
                    {
                        pclust.clear_bits(PG_CLEAN);
                    }
                    // now checked
                    pclust.set_bits(PG_CLEANCHK);
                }
            }

            // is page available for cleaning and does it need it
            if pclust.flags() & (PG_CLEAN | PG_BUSY) != 0 {
                break; // page is already clean or is busy
            }

            // yes! enroll the page in our array
            pclust.set_bits(PG_BUSY);
            // UVM_PAGE_OWN(pclust, "uvm_mk_pcluster"): not configured.

            // If we want to free after io is done, and we're async, set the released flag
            if flags & (PGO_FREE | PGO_SYNCIO) == PGO_FREE {
                pclust.set_bits(PG_RELEASED);
            }

            // XXX: protect wired page? see above comment.
            pmap_page_protect(pclust, PROT_READ);
            if !forward {
                ppsp -= 1; // back up one page
                pps[ppsp] = pclust;
            } else {
                // move forward one page
                pps[ppsp + *npages as usize] = pclust;
            }
            *npages += 1;
            curoff += incr;
        }
    }

    // done! return the cluster array to the caller!!!
    ppsp
}

/// uvm_pager_put: high level pageout routine
///
/// we want to pageout page "pg" to backing store, clustering if possible.
///
/// => page queues must be locked by caller
/// => "uobj" points to the object backing it.
/// => "pg" should be PG_BUSY (by caller), and !PG_CLEAN
/// => "pps" is an array of `*npages` vm_page pointers for possible cluster building; the
///    cluster's start index comes back in `ppsp`
/// => flags
///    PGO_ALLPAGES: all pages in uobj are valid targets
///    PGO_DOACTCLUST: include "PQ_ACTIVE" pages as valid targets
///    PGO_SYNCIO: do SYNC I/O (no async)
///    PGO_PDFREECLUST: pagedaemon: drop cluster on successful I/O
///    PGO_FREE: tell the aio daemon to free pages in the async case.
/// => start/stop: if !PGO_ALLPAGES limit targets to this range
/// => return state:
///    1. we return the VM_PAGER status code of the pageout
///    2. we return with the page queues unlocked
///    3. on errors we always drop the cluster. thus, if we return !PEND, !OK, then the caller
///       only has to worry about un-busying the main page (not the cluster pages).
///    4. on success, if !PGO_PDFREECLUST, we return the cluster with all pages busy (caller
///       must un-busy and check wanted/released flags).
#[allow(clippy::too_many_arguments)] // the C's signature
pub fn uvm_pager_put(
    uobj: &UvmObject,
    pg: &VmPage,
    pps: &mut [*const VmPage],
    ppsp: &mut usize,
    npages: &mut i32,
    flags: i32,
    start: Voff,
    stop: Voff,
) -> i32 {
    // note that the page queues must be locked to cluster.

    // attempt to build a cluster for pageout using its make-put-cluster function (if it has
    // one).
    if let Some(mk) = uobj.pgops().pgo_mk_pcluster {
        *ppsp = mk(uobj, pps, npages, pg, flags, start, stop); // update caller's index
    } else {
        *ppsp = 0;
        pps[0] = pg;
        *npages = 1;
    }

    // now that we've clustered we can unlock the page queues
    uvm_unlock_pageq();

    // now attempt the I/O. if we have a failure and we are clustered, we will drop the
    // cluster and try again.
    let Some(put) = uobj.pgops().pgo_put else {
        panic(format_args!("uvm_pager_put: pager without pgo_put"));
    };
    let n = *npages as usize;
    let result = put(uobj, &mut pps[*ppsp..*ppsp + n], *npages, flags);

    // we have attempted the I/O.
    //
    // if the I/O was a success then:
    //	if !PGO_PDFREECLUST, we return the cluster to the caller (who must un-busy all pages)
    //	else we un-busy cluster pages for the pagedaemon
    //
    // if I/O is pending (async i/o) then we return the pending code. [in this case the async
    // i/o done function must clean up when i/o is done...]
    if result == VM_PAGER_PEND || result == VM_PAGER_OK {
        if result == VM_PAGER_OK && flags & PGO_PDFREECLUST != 0 {
            // drop cluster
            uvm_pager_dropcluster(uobj, &mut pps[*ppsp..*ppsp + n], npages, PGO_PDFREECLUST);
        }
        return result;
    }

    // a pager error occurred (even after dropping the cluster, if there was one).
    uvm_pager_dropcluster(uobj, &mut pps[*ppsp..*ppsp + n], npages, 0);

    result
}

/// uvm_pager_dropcluster: drop a cluster we have built (because we got an error, or, if
/// PGO_PDFREECLUST we are un-busying the cluster pages on behalf of the pagedaemon).
///
/// => uobj is a non-swap-backed object
/// => page queues are not locked
/// => ppsp/npages is our current cluster
/// => flags: PGO_PDFREECLUST: pageout was a success: un-busy cluster pages on behalf of the
///    pagedaemon.
pub fn uvm_pager_dropcluster(
    uobj: &UvmObject,
    ppsp: &mut [*const VmPage],
    npages: &mut i32,
    flags: i32,
) {
    kassert!(rw_write_held(uobj.vmobjlock()));

    for &p in ppsp.iter().take(*npages as usize) {
        // skip empty slot
        if p.is_null() {
            continue;
        }
        let pg = pps_page(p);

        // did someone want the page while we had it busy-locked?
        if pg.flags() & PG_WANTED != 0 {
            wakeup(ptr::from_ref(pg));
        }

        // if page was released, release it. otherwise un-busy it
        if pg.flags() & PG_RELEASED != 0 && pg.flags() & PQ_ANON != 0 {
            // kills anon and frees pg
            match pg.uanon() {
                Some(anon) => uvm_anon_release(anon),
                None => panic(format_args!(
                    "uvm_pager_dropcluster: anon page without anon"
                )),
            }
            continue;
        } else {
            // if we were planning on async io then we would have PG_RELEASED set, clear that
            // with the others.
            pg.clear_bits(PG_BUSY | PG_WANTED | PG_FAKE | PG_RELEASED);
            // UVM_PAGE_OWN(ppsp[lcv], NULL): not configured.
        }

        // if we are operating on behalf of the pagedaemon and we had a successful pageout
        // update the page!
        if flags & PGO_PDFREECLUST != 0 {
            let _ = pmap_clear_reference(pg);
            let _ = pmap_clear_modify(pg);
            pg.set_bits(PG_CLEAN);
        }
    }
}

// uvm_aio_biodone, uvm_swap_dropcluster, uvm_aio_aiodone: the async (swap) pageout, which
// needs uvm_swap.c (M7): not here yet.
/* </CODE> */
