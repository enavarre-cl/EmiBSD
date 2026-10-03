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

//! `<uvm/uvm_pager.h>`: the pager operations every memory object implements, and
//! `uvm_pager.c`: generic functions used to assist the pagers.
//!
//! Upstream: sys/uvm/uvm_pager.h @ 3ce1f3f79392
//! Upstream: sys/uvm/uvm_pager.c @ 3ce1f3f79392
//!
//! Status: `wip`. Milestone M7a ports `struct uvm_pagerops`, the `PGO_*` flags, the
//! `VM_PAGER_*` results, `PGO_DONTCARE` and `uvm_pager_init`'s call of the pagers' init
//! functions. The pager map (`uvm_pseg_*`, `uvm_pagermapin`/`out`), the cluster building
//! (`uvm_mk_pcluster`, `uvm_pager_put`, `uvm_pager_dropcluster`) and `uvm_aio_aiodone` are
//! swap and vnode I/O (M7); `uvm_pager_init` reports the pager map.
//!
//! ## Deviations
//! - `pgo_get`'s page array is `&mut [*const VmPage]`: the C passes `vm_page_t *` with the
//!   sentinel `PGO_DONTCARE` (`(struct vm_page *)-1`) for pages the caller does not want,
//!   which a `Option<&VmPage>` cannot carry; [`pgo_dontcare`] tests for it.
//! - Every operation is an `Option<fn>`: the C leaves unimplemented ones NULL.

use core::ptr;

use crate::kern::kern_lock::mtx_init;
use crate::machine::intr::IPL_VM;
use crate::sys::mutex::Mutex;
use crate::sys::types::Vaddr;
use crate::unported;
use crate::uvm::uvm_aobj::AOBJ_PAGER;
use crate::uvm::uvm_extern::{VmFault, VmProt, Voff};
use crate::uvm::uvm_fault::UvmFaultinfo;
use crate::uvm::uvm_object::UvmObject;
use crate::uvm::uvm_page::VmPage;

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
/// `pgo_put`'s signature: `(uobj, pps, npages, flush)`.
pub type PgoPut = fn(&UvmObject, &mut [*const VmPage], i32, bool) -> i32;
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

/// `uvmpagerops[]`: the pagers (the device and vnode pagers do not exist yet).
static UVMPAGEROPS: [&UvmPagerops; 1] = [&AOBJ_PAGER];

/// `uvm_pseg_lck`: the pager map's lock.
static UVM_PSEG_LCK: Mutex = Mutex::new(IPL_VM);

/// `uvm_pager_init`: init pagers (at boot time).
pub fn uvm_pager_init() {
    // init pager map: uvm_pseg_init(&psegs[0]), (&psegs[1]) need km_alloc(kv_any, kp_none)
    // (M7, with the pager I/O).
    let _ = unported!("uvm_pager_init: the pager map (uvm_pseg_init, M7)");
    mtx_init(&UVM_PSEG_LCK, IPL_VM);

    // init ASYNC I/O queue: TAILQ_INIT(&uvm.aio_done) (M7).

    // call pager init functions
    for ops in UVMPAGEROPS {
        if let Some(init) = ops.pgo_init {
            init();
        }
    }
}
