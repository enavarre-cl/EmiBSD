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
 * from: Id: uvm_fault.h,v 1.1.2.2 1997/12/08 16:07:12 chuck Exp
 */
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
 * from: Id: uvm_fault.c,v 1.1.2.23 1998/02/06 05:29:05 chs Exp
 */
/* </LICENSES> */

/* <CODE> */
//! `uvm_fault.h` / `uvm_fault.c`: fault handler.
//!
//! Upstream: sys/uvm/uvm_fault.h @ 3ce1f3f79392
//! Upstream: sys/uvm/uvm_fault.c @ 3ce1f3f79392
//!
//! A word on page faults. Types of page faults we handle:
//!
//! - case 0: layerless fault: no amap or uobj is present. This is an error.
//! - case 1: upper layer fault (anon active): 1A, read or write with `an_ref == 1`: I/O
//!   takes place in upper level anon and uobj is not touched; 1B, write with `an_ref > 1`:
//!   new anon is alloc'd and data is copied off (COW).
//! - case 2: lower layer fault (uobj): 2A, read on non-NULL uobj or write to
//!   non-copy_on_write area: I/O takes place directly in object; 2B, write to copy_on_write
//!   or read on NULL uobj: data is "promoted" from uobj to a new anon; if uobj is null, then
//!   we zero fill.
//!
//! We follow the standard UVM locking protocol ordering: MAPS => AMAP => UOBJ => ANON =>
//! PAGE QUEUES (PQ). We hold a `PG_BUSY` page if we unlock for I/O.
//!
//! The code is structured as follows: init the "IN" params in the ufi structure; ReFault
//! (`ERESTART` returned to the loop in `uvm_fault`): do lookups (locks maps), check
//! protection, handle needs_copy; check for case 0 fault (error); establish "range" of
//! fault; if we have an amap lock it and extract the anons; if sequential advice deactivate
//! pages behind us; at the same time check pmap for unmapped areas and anon for pages that
//! we could map in (and do map it if found); check object for resident pages that we could
//! map in; if (case 2) goto Case2; handle case 1: ensure source anon is resident in RAM, if
//! case 1B alloc new anon and copy from source, map the correct page in; Case2: ensure
//! source page is resident (if uobj), if case 2B alloc new anon and copy from source (could
//! be zero fill if uobj == NULL), map the correct page in; done!
//!
//! Note on paging: if we have to do I/O we place a `PG_BUSY` page in the correct object,
//! unlock everything, and do the I/O. When I/O is done we must reverify the state of the
//! world before assuming that our data structures are valid (because mappings could change
//! while the map is unlocked). The system keeps a "version" number of a map (every time you
//! write-lock a map you bump it), so we save the version number before we release the lock
//! and start I/O, then relock and check the version numbers to see if anything changed.
//!
//! Status: `wip`. Milestone M7a (part 3) ports the handler whole; the swap paths it reaches
//! stay reported until `uvm_swap.c` (M7).
//!
//! ## Deviations
//! - `uvmadvice[]` is a constant table: `PAGE_SIZE` is a constant here, so the values
//!   `uvmfault_init` computes are too; `uvmfault_init` only checks them.
//! - Out parameters become return values: `uvmfault_promote` yields the anon and the
//!   page, `uvm_fault_check` the offset into the anon array (the C advances `*ranons`),
//!   `uvm_fault_lower_io` the object page; `ufi` is an `Option` where the C passes `NULL`
//!   (paging in anon data without a fault).
//! - `uvm_swap_get` and `uvm_swap_markbad` (`uvmfault_anonget`) are reported: nothing has a
//!   swap slot before `uvm_swap.c`.
//! - The fault starts with shared (`RW_READ`) upper and lower locks on every machine, as the
//!   C does on amd64 and arm64 (the host stands in for them).
//! - `pmap_nested` is 0 (no vmm), `__HAVE_PMAP_POPULATE` is not configured (the
//!   `MULTIPROCESSOR` COW paths need `!__HAVE_PMAP_MPSAFE_ENTER_COW`, which both machines
//!   define), `UVM_PAGE_OWN` is not
//!   configured, `TRACEPOINT` (dt) is not configured.

use core::ptr;

use crate::kern::kern_rwlock::{rw_enter, rw_enter_write, rw_exit, rw_status};
use crate::kern::kern_synch::{nowake, tsleep_nsec, wakeup};
use crate::kern::subr_prf::panic;
use crate::machine::cpu::curproc;
use crate::machine::pmap::{
    pmap_clear_modify, pmap_enter, pmap_extract, pmap_page_protect, pmap_resident_count,
    pmap_unwire, pmap_update,
};
use crate::machine::{Machine, Pmap};
use crate::sys::errno::Errno;
use crate::sys::malloc::M_NOWAIT;
use crate::sys::mman::{MADV_NORMAL, MADV_SEQUENTIAL, PROT_NONE, PROT_WRITE};
use crate::sys::param::{PAGE_SHIFT, PAGE_SIZE, PVM};
use crate::sys::rwlock::{RW_NOSLEEP, RW_READ, RW_UPGRADE, RW_WRITE, rw_lock_held, rw_write_held};
use crate::sys::systm::{kernel_lock, kernel_unlock};
use crate::sys::time::msec_to_nsec;
use crate::sys::tree::RbtHead;
use crate::sys::types::{Paddr, Vaddr};
use crate::uvm::uvm::{
    uvm_et_iscopyonwrite, uvm_et_isneedscopy, uvm_et_isnofault, uvm_et_isstack, uvm_et_issubmap,
    uvm_et_iswc,
};
use crate::uvm::uvm_amap::{
    AMAP_SHARED, VmAmap, amap_add, amap_copy, amap_flags, amap_lock, amap_lookup, amap_lookups,
    amap_populate, amap_unlock,
};
use crate::uvm::uvm_anon::{
    VmAnon, uvm_analloc, uvm_anfree, uvm_anon_dropswap, uvm_anon_release, uvm_anwait,
};
use crate::uvm::uvm_aobj::uao_dropswap;
use crate::uvm::uvm_extern::{MADV_MASK, PROT_MASK, UVM_PGA_ZERO, VmFault, VmProt, Voff};
use crate::uvm::uvm_map::{
    UvmMapAddr, VM_MAP_INTRSAFE, VM_MAP_ISVMSPACE, VmMap, VmMapEntry, uvm_map_lock_entry,
    uvm_map_lookup_entry, uvm_map_unlock_entry, vm_map_assert_anylock, vm_map_downgrade,
    vm_map_lock, vm_map_lock_read, vm_map_unlock, vm_map_unlock_read, vm_map_upgrade,
    vm_mapent_iswired,
};
use crate::uvm::uvm_object::UvmObject;
use crate::uvm::uvm_page::{
    PG_BUSY, PG_CLEAN, PG_FAKE, PG_RELEASED, PG_WANTED, PHYS_TO_VM_PAGE, PQ_ANON, PQ_AOBJ, VmPage,
    uvm_pageactivate, uvm_pagealloc, uvm_pagecopy, uvm_pagedeactivate, uvm_pagefree,
    uvm_pageunwire, uvm_pagewait, uvm_pagewire, vm_page_to_phys,
};
use crate::uvm::uvm_pager::{
    PGO_DEACTIVATE, PGO_DONTCARE, PGO_LOCKED, PGO_SYNCIO, VM_PAGER_AGAIN, VM_PAGER_ERROR,
    VM_PAGER_OK, VM_PAGER_PEND, pgo_dontcare,
};
use crate::uvm::uvm_param::{atop, trunc_page};
use crate::uvm::uvm_pdaemon::uvm_wait;
use crate::uvm::uvm_pmap::{PMAP_CANFAIL, PMAP_WIRED};
use crate::uvm::uvm_swap::{SWSLOT_BAD, uvm_swapisfull};
use crate::uvm::uvmexp::{UvmExpCounters, counters_inc};
use crate::{kassert, unported};

/// `VM_FAULT_INVALID`: invalid mapping.
pub const VM_FAULT_INVALID: VmFault = 0x0;
/// `VM_FAULT_PROTECT`: protection.
pub const VM_FAULT_PROTECT: VmFault = 0x1;
/// `VM_FAULT_WIRE`: wire mapping.
pub const VM_FAULT_WIRE: VmFault = 0x2;

/// `struct uvm_faultinfo`: to load one of these fill in all `orig_*` fields and then call
/// `uvmfault_lookup` on it.
///
/// `map` and `entry` are set by `uvmfault_lookup` with the map locked and read only while
/// it is (or after `uvmfault_relock` found the same map version).
pub struct UvmFaultinfo {
    /// IN: original map.
    pub orig_map: *const VmMap,
    /// IN: original rounded VA.
    pub orig_rvaddr: usize,
    /// IN: original size of interest.
    pub orig_size: usize,
    /// map (could be a submap).
    pub map: *const VmMap,
    /// map's version number.
    pub mapv: u32,
    /// map entry (from 'map').
    pub entry: *const VmMapEntry,
    /// size of interest.
    pub size: usize,
}

impl UvmFaultinfo {
    /// A fault info for `orig_map`, `orig_rvaddr` and `orig_size`, before `uvmfault_lookup`.
    pub fn new(orig_map: &VmMap, orig_rvaddr: usize, orig_size: usize) -> Self {
        Self {
            orig_map: ptr::from_ref(orig_map),
            orig_rvaddr,
            orig_size,
            map: ptr::null(),
            mapv: 0,
            entry: ptr::null(),
            size: 0,
        }
    }

    /// `ufi->orig_map`.
    pub fn orig_map(&self) -> &'static VmMap {
        // SAFETY: set from a live map by `new`; a map outlives every fault on it.
        unsafe { &*self.orig_map }
    }

    /// `ufi->map`: the map `uvmfault_lookup` found (a submap or the original).
    pub fn map(&self) -> &'static VmMap {
        kassert!(!self.map.is_null());
        // SAFETY: `uvmfault_lookup` set it from a live map; submaps are never destroyed.
        unsafe { &*self.map }
    }

    /// `ufi->entry`: the entry `uvmfault_lookup` found, valid while the map stays locked (or
    /// relocked at the same version).
    pub fn entry(&self) -> &'static VmMapEntry {
        kassert!(!self.entry.is_null());
        // SAFETY: `uvmfault_lookup` set it under the map lock; the callers hold that lock or
        // reverified the map's version (`uvmfault_relock`) before reading it again.
        unsafe { &*self.entry }
    }
}

/*
 * local data structures
 */

/// `struct uvm_advice`: how many pages behind and ahead of the fault to map in.
#[derive(Clone, Copy)]
struct UvmAdvice {
    nback: usize,
    nforw: usize,
}

/// `UVM_MAXRANGE`: must be max() of nback+nforw+1.
pub const UVM_MAXRANGE: usize = 16;

/// `uvmadvice[MADV_MASK + 1]`: page range array (the values `uvmfault_init` computes).
const UVMADVICE: [UvmAdvice; MADV_MASK as usize + 1] = uvmadvice_table();

/// `uvmfault_init`'s computation, as a constant.
const fn uvmadvice_table() -> [UvmAdvice; MADV_MASK as usize + 1] {
    let mut table = [UvmAdvice { nback: 0, nforw: 0 }; MADV_MASK as usize + 1];

    let npages = atop(16384);
    if npages > 0 {
        assert!(npages <= UVM_MAXRANGE / 2);
        table[MADV_NORMAL as usize].nforw = npages;
        table[MADV_NORMAL as usize].nback = npages - 1;
    }

    let npages = atop(32768);
    if npages > 0 {
        assert!(npages <= UVM_MAXRANGE / 2);
        table[MADV_SEQUENTIAL as usize].nforw = npages - 1;
        table[MADV_SEQUENTIAL as usize].nback = npages;
    }
    table
}

/// `MASK(entry)`: the protection a copy-on-write entry may be entered with.
fn mask(entry: &VmMapEntry) -> VmProt {
    if uvm_et_iscopyonwrite(entry) {
        !PROT_WRITE
    } else {
        PROT_MASK
    }
}

/// `struct uvm_faultctx`: the following members are set up by `uvm_fault_check()` and
/// read-only after that.
struct UvmFaultctx {
    enter_prot: VmProt,
    access_type: VmProt,
    startva: usize,
    npages: usize,
    centeridx: usize,
    narrow: bool,
    wired: bool,
    pa_flags: usize,
    promote: bool,
    upper_lock_type: i32,
    lower_lock_type: i32,
}

/// The anon behind a slot of the anon array (filled by `amap_lookups` under the amap lock).
fn anon_at(anons: &[*const VmAnon], i: usize) -> Option<&'static VmAnon> {
    // SAFETY: `amap_lookups` filled the slot under the amap lock, which the fault still
    // holds; an anon stays alive while its amap references it.
    unsafe { anons[i].as_ref() }
}

/// The page behind a slot of the page array (`NULL` and `PGO_DONTCARE` are not pages).
fn page_at(p: *const VmPage) -> Option<&'static VmPage> {
    if p.is_null() || pgo_dontcare(p) {
        return None;
    }
    // SAFETY: a page the object's pager handed out under the object lock, or an anon's page
    // under the amap lock; `vm_page`s live forever.
    Some(unsafe { &*p })
}

/// `curproc`'s rusage counter bump (`curproc->p_ru.ru_xxx++`), when there is a process.
fn ru_minflt_inc() {
    if let Some(p) = curproc() {
        p.p_ru.ru_minflt.set(p.p_ru.ru_minflt.get() + 1);
    }
}

/// `curproc->p_ru.ru_majflt++`.
fn ru_majflt_inc() {
    if let Some(p) = curproc() {
        p.p_ru.ru_majflt.set(p.p_ru.ru_majflt.get() + 1);
    }
}

/// The physical address with the pmap flags OR'ed in (`VM_PAGE_TO_PHYS(pg) | pa_flags`).
fn pa_with_flags(pg: &VmPage, pa_flags: usize) -> Paddr {
    Paddr::new(vm_page_to_phys(pg).as_usize() | pa_flags)
}

/*
 * inline functions
 */

/// `uvmfault_anonflush`: try and deactivate pages in specified anons.
///
/// - does not have to deactivate page if it is busy
#[inline]
fn uvmfault_anonflush(anons: &[*const VmAnon]) {
    for lcv in 0..anons.len() {
        let Some(anon) = anon_at(anons, lcv) else {
            continue;
        };
        kassert!(anon.an_lock().is_some_and(rw_lock_held));
        if let Some(pg) = anon.page()
            && pg.flags() & PG_BUSY == 0
        {
            uvm_pagedeactivate(pg);
        }
    }
}

/*
 * normal functions
 */

/// `uvmfault_init`: compute proper values for the `uvmadvice[]` array (a constant here,
/// see the module's deviations).
pub fn uvmfault_init() {
    kassert!(
        UVMADVICE[MADV_NORMAL as usize].nforw + UVMADVICE[MADV_NORMAL as usize].nback
            < UVM_MAXRANGE
    );
    kassert!(
        UVMADVICE[MADV_SEQUENTIAL as usize].nforw + UVMADVICE[MADV_SEQUENTIAL as usize].nback
            < UVM_MAXRANGE
    );
}

/// `uvmfault_anonget`: get data in an anon into a non-busy, non-released page in that anon.
///
/// - Map, amap and thus anon should be locked by caller.
/// - If we fail, we unlock everything and error is returned.
/// - If we are successful, return with everything still locked.
/// - We do not move the page on the queues (gets moved later). If we allocate a new page
///   (we_own), it gets put on the queues. Either way, the result is that the page is on
///   the queues at return time.
pub fn uvmfault_anonget(
    ufi: Option<&UvmFaultinfo>,
    amap: &VmAmap,
    anon: &VmAnon,
) -> Result<(), Errno> {
    let Some(lock) = anon.an_lock() else {
        panic(format_args!("uvmfault_anonget: anon without a lock"));
    };
    kassert!(rw_lock_held(lock));
    kassert!(ptr::eq(anon.an_lock.get(), amap.am_lock.get()));

    // Increment the counters.
    counters_inc(UvmExpCounters::FltAnget);
    if anon.page().is_some() {
        ru_minflt_inc();
    } else {
        ru_majflt_inc();
    }
    let mut error = VM_PAGER_OK;

    // Loop until we get the anon data, or fail.
    loop {
        // Note: 'we_own' will become true if we set PG_BUSY on a page.
        let mut we_own = false;
        let mut pg = anon.page();

        // Is page resident? Make sure it is not busy/released.
        let lock_type = rw_status(lock);
        match pg {
            Some(page) => {
                kassert!(page.flags() & PQ_ANON != 0);
                kassert!(ptr::eq(page.uanon.get(), anon));

                // if the page is busy, we drop all the locks and try again.
                if page.flags() & (PG_BUSY | PG_RELEASED) == 0 {
                    return Ok(());
                }
                counters_inc(UvmExpCounters::FltPgwait);

                // The last unlock must be an atomic unlock and wait on the owner of page.
                kassert!(page.uobject().is_none());
                uvmfault_unlockall(ufi, None, None);
                uvm_pagewait(page, lock, "anonget");
            }
            None => {
                // No page, therefore allocate one. A write lock is required for this. If
                // the caller didn't supply one, fail now and have them retry.
                if lock_type == RW_READ {
                    return Err(Errno::ENOLCK);
                }
                match uvm_pagealloc(None, 0, Some(anon), 0) {
                    None => {
                        // Out of memory. Wait a little.
                        uvmfault_unlockall(ufi, Some(amap), None);
                        counters_inc(UvmExpCounters::FltNoram);
                        uvm_wait("flt_noram1");
                    }
                    Some(page) => {
                        // PG_BUSY bit is set.
                        we_own = true;
                        pg = Some(page);
                        uvmfault_unlockall(ufi, Some(amap), None);

                        // Pass a PG_BUSY+PG_FAKE+PG_CLEAN page into the uvm_swap_get()
                        // function with all data structures unlocked. Note that it is OK to
                        // read an_swslot here, because we hold PG_BUSY on the page.
                        counters_inc(UvmExpCounters::Pageins);
                        // error = uvm_swap_get(pg, anon->an_swslot, PGO_SYNCIO): no swap
                        // yet (see the module's deviations); the I/O fails.
                        let _ = unported!("uvmfault_anonget: uvm_swap_get (uvm_swap.c, M7)");
                        error = VM_PAGER_ERROR;

                        // We clean up after the I/O below in the 'we_own' case.
                    }
                }
            }
        }

        // Re-lock the map and anon.
        let locked = uvmfault_relock(ufi);
        if locked || we_own {
            let _ = rw_enter(lock, lock_type);
        }

        // If we own the page (i.e. we set PG_BUSY), then we need to clean up after the I/O.
        // There are three cases to consider:
        //
        // 1) Page was released during I/O: free anon and ReFault.
        // 2) I/O not OK. Free the page and cause the fault to fail.
        // 3) I/O OK! Activate the page and sync with the non-we_own case (i.e. drop anon
        //    lock if not locked).
        if let (true, Some(page)) = (we_own, pg) {
            if page.flags() & PG_WANTED != 0 {
                wakeup(ptr::from_ref(page));
            }

            // if we were RELEASED during I/O, then our anon is no longer part of an amap.
            // we need to free the anon and try again.
            if page.flags() & PG_RELEASED != 0 {
                kassert!(anon.an_ref.get() == 0);
                // Released while we had unlocked amap.
                if locked {
                    uvmfault_unlockall(ufi, None, None);
                }
                uvm_anon_release(anon); // frees page for us
                counters_inc(UvmExpCounters::FltPgrele);
                return Err(Errno::ERESTART); // refault!
            }

            if error != VM_PAGER_OK {
                kassert!(error != VM_PAGER_PEND);

                // remove page from anon
                anon.an_page.set(ptr::null());

                // Remove the swap slot from the anon and mark the anon as having no real
                // slot. Do not free the swap slot, thus preventing it from being used again.
                // uvm_swap_markbad(anon->an_swslot, 1): no swap yet.
                anon.an_swslot.set(SWSLOT_BAD);

                // Note: page was never !PG_BUSY, so it cannot be mapped and thus no need to
                // pmap_page_protect() it.
                uvm_pagefree(page);

                if locked {
                    uvmfault_unlockall(ufi, None, None);
                }
                rw_exit(lock);
                // An error occurred while trying to bring in the page -- this is the only
                // error we return right now.
                return Err(Errno::EACCES); // XXX
            }

            // We have successfully read the page, activate it.
            pmap_clear_modify(page);
            uvm_pageactivate(page);
            page.clear_bits(PG_WANTED | PG_BUSY | PG_FAKE);
            // UVM_PAGE_OWN(pg, NULL): not configured.
        }

        // We were not able to re-lock the map - restart the fault.
        if !locked {
            if we_own {
                rw_exit(lock);
            }
            return Err(Errno::ERESTART);
        }

        // Verify that no one has touched the amap and moved the anon on us.
        if let Some(ufi) = ufi {
            let entry = ufi.entry();
            let found = amap_lookup(&entry.aref, ufi.orig_rvaddr - entry.start.get());
            if !found.is_some_and(|a| ptr::eq(a, anon)) {
                uvmfault_unlockall(Some(ufi), Some(amap), None);
                return Err(Errno::ERESTART);
            }
        }

        // Retry..
        counters_inc(UvmExpCounters::FltAnretry);
    }
}

/// `uvmfault_promote`: promote data to a new anon. Used for 1B and 2B.
///
/// 1. allocate an anon and a page.
/// 2. fill its contents.
///
/// - if we fail we unlock everything.
/// - on success, return a new locked anon and its page.
/// - it's caller's responsibility to put the promoted page to the page queue.
pub fn uvmfault_promote(
    ufi: &UvmFaultinfo,
    uobjpage: *const VmPage,
) -> Result<(&'static VmAnon, &'static VmPage), Errno> {
    let Some(amap) = ufi.entry().aref.amap() else {
        panic(format_args!("uvmfault_promote: entry without an amap"));
    };
    let uobjpg = page_at(uobjpage);
    let uobj = uobjpg.and_then(VmPage::uobject);

    kassert!(rw_write_held(amap.lock()));
    kassert!(uobj.is_none_or(|o| rw_lock_held(o.vmobjlock())));

    let anon = uvm_analloc();
    let pg = anon.and_then(|anon| {
        anon.an_lock.set(amap.am_lock.get());
        uvm_pagealloc(
            None,
            0,
            Some(anon),
            if pgo_dontcare(uobjpage) {
                UVM_PGA_ZERO
            } else {
                0
            },
        )
    });

    // check for out of RAM
    let (anon, pg) = match (anon, pg) {
        (Some(anon), Some(pg)) => (anon, pg),
        (anon, _) => {
            uvmfault_unlockall(Some(ufi), Some(amap), uobj);
            match anon {
                None => counters_inc(UvmExpCounters::FltNoanon),
                Some(anon) => {
                    anon.an_lock.set(ptr::null());
                    anon.an_ref.set(anon.an_ref.get() - 1);
                    uvm_anfree(anon);
                    counters_inc(UvmExpCounters::FltNoram);
                }
            }

            if uvm_swapisfull() {
                return Err(Errno::ENOMEM);
            }

            // out of RAM, wait for more
            if anon.is_none() {
                uvm_anwait();
            } else {
                uvm_wait("flt_noram3");
            }
            return Err(Errno::ERESTART);
        }
    };

    // copy the page (pg now dirty)
    if let Some(src) = uobjpg {
        uvm_pagecopy(src, pg);
    }

    Ok((anon, pg))
}

/// `uvmfault_update_stats`: update statistics after fault resolution: maxrss.
pub fn uvmfault_update_stats(ufi: &UvmFaultinfo) {
    let map = ufi.orig_map();

    // If this is a nested pmap (eg, a virtual machine pmap managed by vmm(4) on
    // amd64/i386), don't do any updating, just return. pmap_nested() is 0 here (no vmm).

    // Update the maxrss for the process.
    if map.flags.get() & VM_MAP_ISVMSPACE != 0 {
        let Some(p) = curproc() else {
            return;
        };
        kassert!(ptr::eq(&p.vmspace().vm_map, map));

        let res = pmap_resident_count(map.pmap());
        // Convert res from pages to kilobytes.
        let res = res << (PAGE_SHIFT - 10);

        if p.p_ru.ru_maxrss.get() < res {
            p.p_ru.ru_maxrss.set(res);
        }
    }
}

/*
 *   F A U L T   -   m a i n   e n t r y   p o i n t
 */

/// `uvm_fault`: page fault handler.
///
/// - called from MD code to resolve a page fault
/// - VM data structures usually should be unlocked. However, it is possible to call here
///   with the main map locked if the caller gets a write lock, sets it recursive, and
///   then calls us (c.f. `uvm_map_pageable`). This should be avoided because it keeps the
///   map locked off during I/O.
/// - MUST NEVER BE CALLED IN INTERRUPT CONTEXT
pub fn uvm_fault(
    orig_map: &VmMap,
    vaddr: usize,
    fault_type: VmFault,
    access_type: VmProt,
) -> Result<(), Errno> {
    counters_inc(UvmExpCounters::Faults);
    // TRACEPOINT(uvm, fault, ...): dt is not configured.

    // init the IN parameters in the ufi
    let mut ufi = UvmFaultinfo::new(orig_map, trunc_page(vaddr), PAGE_SIZE);
    let mut flt = UvmFaultctx {
        enter_prot: 0,
        access_type,
        startva: 0,
        npages: 0,
        centeridx: 0,
        narrow: false, // assume normal fault for now
        wired: false,  // assume non-wired fault for now
        pa_flags: 0,
        promote: false,
        // shared lock for now (amd64, arm64; see the module's deviations)
        upper_lock_type: RW_READ,
        lower_lock_type: RW_READ,
    };
    let mut anons_store: [*const VmAnon; UVM_MAXRANGE] = [ptr::null(); UVM_MAXRANGE];
    let mut pages: [*const VmPage; UVM_MAXRANGE] = [ptr::null(); UVM_MAXRANGE];

    loop {
        // ReFault:
        let anons_off = match uvm_fault_check(&mut ufi, &mut flt, &mut anons_store, fault_type) {
            Ok(off) => off,
            Err(Errno::ERESTART) => continue,
            Err(e) => return Err(e),
        };
        let anons = &anons_store[anons_off..];

        // True if there is an anon at the faulting address
        let shadowed = uvm_fault_upper_lookup(&ufi, &flt, anons, &mut pages);
        let result = if shadowed {
            // case 1: fault on an anon in our amap
            uvm_fault_upper(&ufi, &mut flt, anons)
        } else {
            let uobj = ufi.entry().uvm_obj();

            // if the desired page is not shadowed by the amap and we have a backing object,
            // then we check to see if the backing object would prefer to handle the fault
            // itself (rather than letting us do it with the usual pgo_get hook). the backing
            // object signals this by providing a pgo_fault routine.
            match uobj.and_then(|uobj| uobj.pgops().pgo_fault.map(|f| (uobj, f))) {
                Some((uobj, pgo_fault)) => {
                    rw_enter_write(uobj.vmobjlock());
                    kernel_lock(); // KERNEL_LOCK()
                    let npages = flt.npages;
                    let rv = pgo_fault(
                        &mut ufi,
                        Vaddr::new(flt.startva),
                        &mut pages[..npages],
                        npages as i32,
                        flt.centeridx as i32,
                        fault_type,
                        flt.access_type,
                        PGO_LOCKED,
                    );
                    kernel_unlock(); // KERNEL_UNLOCK()
                    match Errno::from_raw(rv) {
                        Some(e) => Err(e),
                        None => Ok(()),
                    }
                }
                None => {
                    // case 2: fault on backing obj or zero fill
                    uvm_fault_lower(&ufi, &mut flt, &mut pages)
                }
            }
        };
        match result {
            Err(Errno::ERESTART) => continue,
            other => return other,
        }
    }
}

/// `uvm_fault_check`: check prot, handle needs-copy, etc.
///
/// 1. lookup entry.
/// 2. check protection.
/// 3. adjust fault condition (mainly for simulated fault).
/// 4. handle needs-copy (lazy amap copy).
/// 5. establish range of interest for neighbor fault (aka pre-fault).
/// 6. look up anons (if amap exists).
/// 7. flush pages (if MADV_SEQUENTIAL)
///
/// - called with nothing locked.
/// - if we fail we unlock everything.
/// - initialize/adjust many members of flt.
/// - yields the offset into `anons_store` of the first anon of interest (the C advances
///   `*ranons`).
fn uvm_fault_check(
    ufi: &mut UvmFaultinfo,
    flt: &mut UvmFaultctx,
    anons_store: &mut [*const VmAnon; UVM_MAXRANGE],
    fault_type: VmFault,
) -> Result<usize, Errno> {
    let mut write_locked = false;

    // lookup and lock the maps
    loop {
        // lookup:
        if !uvmfault_lookup(ufi, write_locked) {
            return Err(Errno::EFAULT);
        }

        #[cfg(feature = "diagnostic")]
        if ufi.map().flags.get() & crate::uvm::uvm_map::VM_MAP_PAGEABLE == 0 {
            panic(format_args!(
                "uvm_fault: fault on non-pageable map ({:p}, {:#x})",
                ufi.map, ufi.orig_rvaddr
            ));
        }
        let entry = ufi.entry();

        // check protection
        if entry.protection.get() & flt.access_type != flt.access_type {
            uvmfault_unlockmaps(Some(ufi), write_locked);
            return Err(Errno::EACCES);
        }

        // "enter_prot" is the protection we want to enter the page in at. for certain
        // pages (e.g. copy-on-write pages) this protection can be more strict than
        // ufi->entry->protection. "wired" means either the entry is wired or we are
        // fault-wiring the pg.
        flt.enter_prot = entry.protection.get();
        flt.pa_flags = if uvm_et_iswc(entry) {
            <Machine as Pmap>::PMAP_WC
        } else {
            0
        };
        if vm_mapent_iswired(entry) || fault_type == VM_FAULT_WIRE {
            flt.wired = true;
            flt.access_type = flt.enter_prot; // full access for wired
            // don't look for neighborhood pages on "wire" fault
            flt.narrow = true;
            // wiring pages requires a write lock.
            flt.upper_lock_type = RW_WRITE;
            flt.lower_lock_type = RW_WRITE;
        }

        // handle "needs_copy" case.
        if uvm_et_isneedscopy(entry) {
            if flt.access_type & PROT_WRITE != 0 || entry.uvm_obj().is_none() {
                // modifying `ufi->entry' requires write lock
                if !write_locked {
                    write_locked = true;
                    if !vm_map_upgrade(ufi.map()) {
                        uvmfault_unlockmaps(Some(ufi), false);
                        continue; // goto lookup
                    }
                }

                amap_copy(
                    ufi.map(),
                    entry,
                    M_NOWAIT,
                    !uvm_et_isstack(entry),
                    ufi.orig_rvaddr,
                    ufi.orig_rvaddr + 1,
                );

                // didn't work? must be out of RAM.
                if uvm_et_isneedscopy(entry) {
                    uvmfault_unlockmaps(Some(ufi), write_locked);
                    uvm_wait("fltamapcopy");
                    return Err(Errno::ERESTART);
                }

                counters_inc(UvmExpCounters::FltAmcopy);
            } else {
                // ensure that we pmap_enter page R/O since needs_copy is still true
                flt.enter_prot &= !PROT_WRITE;
            }
        }
        break;
    }

    if write_locked {
        vm_map_downgrade(ufi.map());
    }
    let entry = ufi.entry();

    // identify the players
    let amap = entry.aref.amap(); // upper layer
    let uobj = entry.uvm_obj(); // lower layer

    // check for a case 0 fault. if nothing backing the entry then error now.
    if amap.is_none() && uobj.is_none() {
        uvmfault_unlockmaps(Some(ufi), false);
        return Err(Errno::EFAULT);
    }

    // for a case 2B fault waste no time on adjacent pages because they are likely already
    // entered.
    if uobj.is_some() && amap.is_some() && flt.access_type & PROT_WRITE != 0 {
        // wide fault (!narrow)
        flt.narrow = true;
    }

    // establish range of interest based on advice from mapper and then clip to fit map
    // entry. note that we only want to do this the first time through the fault. if we
    // ReFault we will disable this by setting "narrow" to true.
    let nback;
    if !flt.narrow {
        // wide fault (!narrow)
        let advice = &UVMADVICE[(entry.advice.get() & MADV_MASK) as usize];
        nback = advice
            .nback
            .min((ufi.orig_rvaddr - entry.start.get()) >> PAGE_SHIFT);
        flt.startva = ufi.orig_rvaddr - (nback << PAGE_SHIFT);
        let nforw = advice
            .nforw
            .min(((entry.end.get() - ufi.orig_rvaddr) >> PAGE_SHIFT) - 1);
        // note: "-1" because we don't want to count the faulting page as forw
        flt.npages = nback + nforw + 1;
        flt.centeridx = nback;

        flt.narrow = true; // ensure only once per-fault
    } else {
        // narrow fault!
        nback = 0;
        flt.startva = ufi.orig_rvaddr;
        flt.npages = 1;
        flt.centeridx = 0;
    }

    // if we've got an amap then lock it and extract current anons.
    let mut anons_off = 0;
    if let Some(amap) = amap {
        if flt.access_type & PROT_WRITE != 0 {
            // assume we're about to COW.
            flt.upper_lock_type = RW_WRITE;
        }
        amap_lock(amap, flt.upper_lock_type);
        amap_lookups(
            &entry.aref,
            flt.startva - entry.start.get(),
            &mut anons_store[..flt.npages],
        );
    } else {
        anons_store.fill(ptr::null()); // to be safe
    }

    if flt.access_type & PROT_WRITE != 0 {
        // we are about to dirty the object and that requires a write lock.
        flt.lower_lock_type = RW_WRITE;
    }

    // for MADV_SEQUENTIAL mappings we want to deactivate the back pages now and then
    // forget about them (for the rest of the fault).
    if entry.advice.get() == MADV_SEQUENTIAL && nback != 0 {
        // flush back-page anons?
        if amap.is_some() {
            uvmfault_anonflush(&anons_store[..nback]);
        }

        // flush object? change lock type to RW_WRITE, to avoid excessive competition
        // between read/write locks if many threads doing "sequential access".
        if let Some(uobj) = uobj {
            let uoff = (flt.startva - entry.start.get()) as Voff + entry.offset.get();
            flt.lower_lock_type = RW_WRITE;
            rw_enter_write(uobj.vmobjlock());
            if let Some(flush) = uobj.pgops().pgo_flush {
                let _ = flush(
                    uobj,
                    uoff,
                    uoff + (nback << PAGE_SHIFT) as Voff,
                    PGO_DEACTIVATE,
                );
            }
            rw_exit(uobj.vmobjlock());
        }

        // now forget about the backpages
        if amap.is_some() {
            anons_off = nback;
        }
        flt.startva += nback << PAGE_SHIFT;
        flt.npages -= nback;
        flt.centeridx = 0;
    }

    Ok(anons_off)
}

/// `uvm_fault_upper_upgrade`: upgrade upper lock, reader -> writer.
#[inline]
fn uvm_fault_upper_upgrade(flt: &mut UvmFaultctx, amap: &VmAmap) -> Result<(), Errno> {
    kassert!(flt.upper_lock_type == rw_status(amap.lock()));

    // fast path.
    if flt.upper_lock_type == RW_WRITE {
        return Ok(());
    }

    // otherwise try for the upgrade. if we don't get it, unlock everything, restart the
    // fault and next time around get a writer lock.
    flt.upper_lock_type = RW_WRITE;
    if rw_enter(amap.lock(), RW_UPGRADE | RW_NOSLEEP).is_err() {
        counters_inc(UvmExpCounters::FltNoup);
        return Err(Errno::ERESTART);
    }
    counters_inc(UvmExpCounters::FltUp);
    kassert!(flt.upper_lock_type == rw_status(amap.lock()));
    Ok(())
}

/// `uvm_fault_upper_lookup`: look up existing h/w mapping and amap.
///
/// Iterate range of interest:
/// 1. check if h/w mapping exists. If yes, we don't care.
/// 2. check if anon exists. If not, page is lower.
/// 3. if anon exists, enter h/w mapping for neighbors.
///
/// - called with amap locked (if exists).
fn uvm_fault_upper_lookup(
    ufi: &UvmFaultinfo,
    flt: &UvmFaultctx,
    anons: &[*const VmAnon],
    pages: &mut [*const VmPage; UVM_MAXRANGE],
) -> bool {
    let entry = ufi.entry();
    let amap = entry.aref.amap();
    let pmap = ufi.orig_map().pmap();
    let mut entered = 0;

    kassert!(amap.is_none_or(|amap| rw_status(amap.lock()) == flt.upper_lock_type));

    // map in the backpages and frontpages we found in the amap in hopes of preventing
    // future faults. we also init the pages[] array as we go.
    let mut currva = flt.startva;
    let mut shadowed = false;
    for (lcv, slot) in pages.iter_mut().enumerate().take(flt.npages) {
        let va = currva;
        currva += PAGE_SIZE;

        // unmapped or center page. check if any anon at this level.
        let Some(anon) = amap.and_then(|_| anon_at(anons, lcv)) else {
            *slot = ptr::null();
            continue;
        };

        // check for present page and map if possible.
        *slot = PGO_DONTCARE;
        if lcv == flt.centeridx {
            // save center for later!
            shadowed = true;
            continue;
        }

        kassert!(ptr::eq(
            anon.an_lock.get(),
            amap.map_or(ptr::null(), |a| a.am_lock.get())
        ));

        // ignore busy pages. don't play with VAs that are already mapped.
        if let Some(pg) = anon.page()
            && pg.flags() & (PG_RELEASED | PG_BUSY) == 0
            && pmap_extract(pmap, Vaddr::new(va)).is_none()
        {
            uvm_pageactivate(pg); // reactivate
            counters_inc(UvmExpCounters::FltNamap);

            // No fault-ahead when wired.
            kassert!(!flt.wired);

            // Since this isn't the page that's actually faulting, ignore pmap_enter()
            // failures; it's not critical that we enter these right now.
            let prot = if anon.an_ref.get() > 1 {
                flt.enter_prot & !PROT_WRITE
            } else {
                flt.enter_prot
            };
            let _ = pmap_enter(
                pmap,
                Vaddr::new(va),
                pa_with_flags(pg, flt.pa_flags),
                prot,
                PMAP_CANFAIL,
            );
            entered += 1;
        }
    }
    if entered > 0 {
        pmap_update(pmap);
    }

    shadowed
}

/// `uvm_fault_upper`: handle upper fault.
///
/// 1. acquire anon lock.
/// 2. get anon. Let `uvmfault_anonget` do the dirty work.
/// 3. if COW, promote data to new anon.
/// 4. enter h/w mapping.
fn uvm_fault_upper(
    ufi: &UvmFaultinfo,
    flt: &mut UvmFaultctx,
    anons: &[*const VmAnon],
) -> Result<(), Errno> {
    let entry = ufi.entry();
    let Some(amap) = entry.aref.amap() else {
        panic(format_args!("uvm_fault_upper: entry without an amap"));
    };
    let Some(mut anon) = anon_at(anons, flt.centeridx) else {
        panic(format_args!("uvm_fault_upper: no anon at the center"));
    };
    let pmap = ufi.orig_map().pmap();

    kassert!(rw_status(amap.lock()) == flt.upper_lock_type);
    kassert!(ptr::eq(anon.an_lock.get(), amap.am_lock.get()));

    // no matter if we have case 1A or case 1B we are going to need to have the anon's
    // memory resident. ensure that now.
    //
    // let uvmfault_anonget do the dirty work. if it fails it will unlock everything for us.
    // if it succeeds, locks are still valid and locked. also, if it is OK, then the anon's
    // page is on the queues.
    loop {
        // retry:
        match uvmfault_anonget(Some(ufi), amap, anon) {
            Ok(()) => break,
            Err(Errno::ERESTART) => return Err(Errno::ERESTART),
            Err(Errno::ENOLCK) => {
                // it needs a write lock: retry
                if let Err(e) = uvm_fault_upper_upgrade(flt, amap) {
                    uvmfault_unlockall(Some(ufi), Some(amap), None);
                    return Err(e);
                }
                kassert!(rw_write_held(amap.lock()));
            }
            Err(e) => return Err(e),
        }
    }

    kassert!(rw_status(amap.lock()) == flt.upper_lock_type);
    kassert!(ptr::eq(anon.an_lock.get(), amap.am_lock.get()));

    // if we are case 1B then we will need to allocate a new blank anon to transfer the
    // data into. note that we have a lock on anon, so no one can busy or release the page
    // until we are done. also note that the ref count can't drop to zero here because it
    // is > 1 and we are only dropping one ref.
    //
    // in the (hopefully very rare) case that we are out of RAM we will unlock, wait for
    // more RAM, and refault.
    //
    // if we are out of anon VM we wait for RAM to become available.
    let pg;
    if flt.access_type & PROT_WRITE != 0 && anon.an_ref.get() > 1 {
        // promoting requires a write lock.
        if let Err(e) = uvm_fault_upper_upgrade(flt, amap) {
            uvmfault_unlockall(Some(ufi), Some(amap), None);
            return Err(e);
        }
        kassert!(rw_write_held(amap.lock()));

        counters_inc(UvmExpCounters::FltAcow);
        let oanon = anon; // oanon = old

        let (nanon, npg) = uvmfault_promote(ufi, oanon.an_page.get())?;
        anon = nanon;
        pg = npg;

        // un-busy! new page
        pg.clear_bits(PG_BUSY | PG_FAKE);
        // UVM_PAGE_OWN(pg, NULL): not configured.
        let ret = amap_add(&entry.aref, ufi.orig_rvaddr - entry.start.get(), anon, true);
        kassert!(ret);

        kassert!(ptr::eq(anon.an_lock.get(), oanon.an_lock.get()));

        // deref: can not drop to zero here by defn!
        kassert!(oanon.an_ref.get() > 1);
        oanon.an_ref.set(oanon.an_ref.get() - 1);

        // note: oanon is still locked, as is the new anon. we need to check for this later
        // when we unlock oanon; if oanon != anon, we'll have to unlock anon, too.
        kassert!(ptr::eq(anon.an_lock.get(), amap.am_lock.get()));
        kassert!(ptr::eq(oanon.an_lock.get(), amap.am_lock.get()));

        // MULTIPROCESSOR && !__HAVE_PMAP_MPSAFE_ENTER_COW: both machines define the latter.
    } else {
        counters_inc(UvmExpCounters::FltAnon);
        let Some(page) = anon.page() else {
            panic(format_args!(
                "uvm_fault_upper: anon without a page after anonget"
            ));
        };
        pg = page;
        if anon.an_ref.get() > 1 {
            // disallow writes to ref > 1 anons
            flt.enter_prot &= !PROT_WRITE;
        }
    }

    // now map the page in .
    if pmap_enter(
        pmap,
        Vaddr::new(ufi.orig_rvaddr),
        pa_with_flags(pg, flt.pa_flags),
        flt.enter_prot,
        flt.access_type | PMAP_CANFAIL | if flt.wired { PMAP_WIRED } else { 0 },
    )
    .is_err()
    {
        // No need to undo what we did; we can simply think of this as the pmap throwing
        // away the mapping information.
        //
        // We do, however, have to go through the ReFault path, as the map may change while
        // we're asleep.
        uvmfault_unlockall(Some(ufi), Some(amap), None);
        if uvm_swapisfull() {
            // XXX instrumentation
            return Err(Errno::ENOMEM);
        }
        // __HAVE_PMAP_POPULATE: not configured.
        // XXX instrumentation
        uvm_wait("flt_pmfail1");
        return Err(Errno::ERESTART);
    }

    // ... update the page queues.
    if flt.wired {
        uvm_pagewire(pg);
    } else {
        uvm_pageactivate(pg);
    }

    if flt.wired {
        // since the now-wired page cannot be paged out, release its swap resources for
        // others to use. since an anon with no swap cannot be PG_CLEAN, clear its clean
        // flag now.
        pg.clear_bits(PG_CLEAN);
        uvm_anon_dropswap(anon);
    }

    // done case 1! finish up by unlocking everything and returning success
    uvmfault_unlockall(Some(ufi), Some(amap), None);
    pmap_update(pmap);
    Ok(())
}

/// `uvm_fault_lower_lookup`: look up on-memory uobj pages.
///
/// 1. get on-memory pages.
/// 2. if failed, give up (get only center page later).
/// 3. if succeeded, enter h/w mapping of neighbor pages.
fn uvm_fault_lower_lookup(
    ufi: &UvmFaultinfo,
    flt: &UvmFaultctx,
    pages: &mut [*const VmPage; UVM_MAXRANGE],
) -> Option<&'static VmPage> {
    let entry = ufi.entry();
    let Some(uobj) = entry.uvm_obj() else {
        panic(format_args!(
            "uvm_fault_lower_lookup: entry without an object"
        ));
    };
    let pmap = ufi.orig_map().pmap();

    let _ = rw_enter(uobj.vmobjlock(), flt.lower_lock_type);

    counters_inc(UvmExpCounters::FltLget);
    let mut gotpages = flt.npages as i32;
    let Some(pgo_get) = uobj.pgops().pgo_get else {
        panic(format_args!(
            "uvm_fault_lower_lookup: object without pgo_get"
        ));
    };
    let npages = flt.npages;
    let _ = pgo_get(
        uobj,
        entry.offset.get() + (flt.startva - entry.start.get()) as Voff,
        &mut pages[..npages],
        &mut gotpages,
        flt.centeridx as i32,
        flt.access_type & mask(entry),
        entry.advice.get(),
        PGO_LOCKED,
    );

    // check for pages to map, if we got any
    if gotpages == 0 {
        return None;
    }

    let mut entered = 0;
    let mut uobjpage = None;
    let mut currva = flt.startva;
    for (lcv, &slot) in pages.iter().enumerate().take(flt.npages) {
        let va = currva;
        currva += PAGE_SIZE;
        let Some(pg) = page_at(slot) else {
            continue;
        };

        kassert!(pg.flags() & PG_BUSY == 0);
        kassert!(pg.flags() & PG_RELEASED == 0);

        // if center page is resident and not PG_BUSY, then pgo_get gave us a handle to it.
        // remember this page as "uobjpage." (for later use).
        if lcv == flt.centeridx {
            uobjpage = Some(pg);
            continue;
        }

        if pmap_extract(pmap, Vaddr::new(va)).is_some() {
            continue;
        }

        // calling pgo_get with PGO_LOCKED returns us pages which are neither busy nor
        // released, so we don't need to check for this. we can just directly enter the
        // pages.
        uvm_pageactivate(pg);
        counters_inc(UvmExpCounters::FltNomap);

        // No fault-ahead when wired.
        kassert!(!flt.wired);

        // Since this page isn't the page that's actually faulting, ignore pmap_enter()
        // failures; it's not critical that we enter these right now. NOTE: page can't be
        // PG_WANTED or PG_RELEASED because we've held the lock the whole time we've had the
        // handle.
        let _ = pmap_enter(
            pmap,
            Vaddr::new(va),
            pa_with_flags(pg, flt.pa_flags),
            flt.enter_prot & mask(entry),
            PMAP_CANFAIL,
        );
        entered += 1;
    }
    if entered > 0 {
        pmap_update(pmap);
    }

    uobjpage
}

/// `uvm_fault_lower_upgrade`: upgrade lower lock, reader -> writer.
#[inline]
fn uvm_fault_lower_upgrade(flt: &mut UvmFaultctx, uobj: &UvmObject) -> Result<(), Errno> {
    kassert!(flt.lower_lock_type == rw_status(uobj.vmobjlock()));

    // fast path.
    if flt.lower_lock_type == RW_WRITE {
        return Ok(());
    }

    // otherwise try for the upgrade. if we don't get it, unlock everything, restart the
    // fault and next time around get a writer lock.
    flt.lower_lock_type = RW_WRITE;
    if rw_enter(uobj.vmobjlock(), RW_UPGRADE | RW_NOSLEEP).is_err() {
        counters_inc(UvmExpCounters::FltNoup);
        return Err(Errno::ERESTART);
    }
    counters_inc(UvmExpCounters::FltUp);
    kassert!(flt.lower_lock_type == rw_status(uobj.vmobjlock()));
    Ok(())
}

/// `uvm_fault_lower`: handle lower fault.
///
/// 1. check uobj: 1.1 if null, ZFOD; 1.2 if not null, look up unmapped neighbor pages.
/// 2. for center page, check if promote: 2.1 ZFOD always needs promotion; 2.2 other uobjs,
///    when entry is marked COW (usually `MAP_PRIVATE` vnode).
/// 3. if uobj is not ZFOD and page is not found, do i/o.
/// 4. dispatch either direct / promote fault.
fn uvm_fault_lower(
    ufi: &UvmFaultinfo,
    flt: &mut UvmFaultctx,
    pages: &mut [*const VmPage; UVM_MAXRANGE],
) -> Result<(), Errno> {
    let entry = ufi.entry();
    let amap = entry.aref.amap();
    let mut uobj = entry.uvm_obj();
    let pmap = ufi.orig_map().pmap();
    let mut dropswap = false;

    // now, if the desired page is not shadowed by the amap and we have a backing object
    // that does not have a special fault routine, then we ask (with pgo_get) the object for
    // resident pages that we care about and attempt to map them in. we do not let pgo_get
    // block (PGO_LOCKED).
    let mut uobjpage: *const VmPage = match uobj {
        // zero fill; don't care neighbor pages
        None => ptr::null(),
        Some(_) => uvm_fault_lower_lookup(ufi, flt, pages).map_or(ptr::null(), ptr::from_ref),
    };

    // note that at this point we are done with any front or back pages. we are now going to
    // focus on the center page (i.e. the one we've faulted on). if we have faulted on the
    // bottom (uobj) layer [i.e. case 2] and the page was both present and available, then
    // we've got a pointer to it as "uobjpage" and we've already made it BUSY.
    kassert!(amap.is_none_or(|amap| rw_status(amap.lock()) == flt.upper_lock_type));
    kassert!(uobj.is_none_or(|uobj| rw_status(uobj.vmobjlock()) == flt.lower_lock_type));

    // note that uobjpage can not be PGO_DONTCARE at this point. we now set uobjpage to
    // PGO_DONTCARE if we are doing a zero fill. if we have a backing object, check and see
    // if we are going to promote the data up to an anon during the fault.
    if uobj.is_none() {
        uobjpage = PGO_DONTCARE;
        flt.promote = true; // always need anon here
    } else {
        kassert!(!pgo_dontcare(uobjpage));
        flt.promote = flt.access_type & PROT_WRITE != 0 && uvm_et_iscopyonwrite(entry);
    }

    // if uobjpage is not null then we do not need to do I/O to get the uobjpage.
    //
    // if uobjpage is null, then we need to ask the pager to get the data for us. once we
    // have the data, we need to reverify the state the world. we are currently not holding
    // any resources.
    if !uobjpage.is_null() {
        // update rusage counters
        ru_minflt_inc();
        if let Some(pg) = page_at(uobjpage) {
            uvm_pageactivate(pg);
        }
    } else {
        uobjpage = uvm_fault_lower_io(ufi, flt, &mut uobj)?;
    }

    // notes:
    //  - at this point uobjpage can not be NULL
    //  - at this point uobjpage could be PG_WANTED (handle later)
    let pg: &'static VmPage;
    let mut anon: Option<&'static VmAnon> = None;
    if !flt.promote {
        // we are not promoting. if the mapping is COW ensure that we don't give more access
        // than we should (e.g. when doing a read fault on a COPYONWRITE mapping we want to
        // map the COW page in R/O even though the entry protection could be R/W).
        //
        // set "pg" to the page we want to map in (uobjpage, usually)
        counters_inc(UvmExpCounters::FltObj);
        if uvm_et_iscopyonwrite(entry) {
            flt.enter_prot &= !PROT_WRITE;
        }
        let Some(page) = page_at(uobjpage) else {
            panic(format_args!("uvm_fault_lower: no object page to map"));
        };
        pg = page; // map in the actual object

    // we are faulting directly on the page.
    } else {
        let Some(amap) = amap else {
            panic(format_args!("uvm_fault_lower: promoting without an amap"));
        };

        // promoting requires a write lock.
        if let Err(e) = uvm_fault_upper_upgrade(flt, amap) {
            uvmfault_unlockall(Some(ufi), Some(amap), uobj);
            return Err(e);
        }
        kassert!(rw_write_held(amap.lock()));
        kassert!(uobj.is_none_or(|uobj| rw_status(uobj.vmobjlock()) == flt.lower_lock_type));

        // if we are going to promote the data to an anon we allocate a blank anon here and
        // plug it into our amap.
        let (nanon, npg) = uvmfault_promote(ufi, uobjpage)?;
        anon = Some(nanon);
        pg = npg;

        // fill in the data
        if let Some(src) = page_at(uobjpage) {
            counters_inc(UvmExpCounters::FltPrcopy);

            // promote to shared amap? make sure all sharing procs see it
            if amap_flags(amap) & AMAP_SHARED != 0 {
                pmap_page_protect(src, PROT_NONE);
            }
            // MULTIPROCESSOR && !__HAVE_PMAP_MPSAFE_ENTER_COW: both machines define the latter.
            // done with copied uobjpage.
            if let Some(obj) = uobj {
                rw_exit(obj.vmobjlock());
            }
            uobj = None;
        } else {
            counters_inc(UvmExpCounters::FltPrzero);
            // Page is zero'd and marked dirty by uvm_pagealloc(), called in
            // uvmfault_promote() above.
        }

        if !amap_add(
            &entry.aref,
            ufi.orig_rvaddr - entry.start.get(),
            nanon,
            false,
        ) {
            if pg.flags() & PG_WANTED != 0 {
                wakeup(ptr::from_ref(pg));
            }

            pg.clear_bits(PG_BUSY | PG_FAKE | PG_WANTED);
            // UVM_PAGE_OWN(pg, NULL): not configured.
            uvmfault_unlockall(Some(ufi), Some(amap), uobj);
            uvm_anfree(nanon);
            counters_inc(UvmExpCounters::FltNoamap);

            if uvm_swapisfull() {
                return Err(Errno::ENOMEM);
            }

            amap_populate(&entry.aref, ufi.orig_rvaddr - entry.start.get());
            return Err(Errno::ERESTART);
        }
    }

    // anon must be write locked (promotion). uobj can be either.
    //
    // Note: pg is either the uobjpage or the new page in the new anon.
    kassert!(amap.is_none_or(|amap| rw_status(amap.lock()) == flt.upper_lock_type));
    kassert!(uobj.is_none_or(|uobj| rw_status(uobj.vmobjlock()) == flt.lower_lock_type));
    kassert!(anon.is_none_or(|anon| {
        amap.is_some_and(|amap| ptr::eq(anon.an_lock.get(), amap.am_lock.get()))
    }));

    // all resources are present. we can now map it in and free our resources.
    if pmap_enter(
        pmap,
        Vaddr::new(ufi.orig_rvaddr),
        pa_with_flags(pg, flt.pa_flags),
        flt.enter_prot,
        flt.access_type | PMAP_CANFAIL | if flt.wired { PMAP_WIRED } else { 0 },
    )
    .is_err()
    {
        // No need to undo what we did; we can simply think of this as the pmap throwing
        // away the mapping information.
        //
        // We do, however, have to go through the ReFault path, as the map may change while
        // we're asleep.
        if pg.flags() & PG_WANTED != 0 {
            wakeup(ptr::from_ref(pg));
        }

        pg.clear_bits(PG_BUSY | PG_FAKE | PG_WANTED);
        // UVM_PAGE_OWN(pg, NULL): not configured.
        uvmfault_unlockall(Some(ufi), amap, uobj);
        if uvm_swapisfull() {
            // XXX instrumentation
            return Err(Errno::ENOMEM);
        }
        // __HAVE_PMAP_POPULATE: not configured.
        // XXX instrumentation
        uvm_wait("flt_pmfail2");
        return Err(Errno::ERESTART);
    }

    if flt.wired {
        uvm_pagewire(pg);
        if pg.flags() & PQ_AOBJ != 0 {
            // since the now-wired page cannot be paged out, release its swap resources for
            // others to use. since an aobj page with no swap cannot be clean, mark it dirty
            // now.
            //
            // use pg->uobject here. if the page is from a tmpfs vnode, the pages are backed
            // by its UAO and not the vnode.
            kassert!(uobj.is_some());
            kassert!(uobj.is_some_and(|uobj| {
                pg.uobject()
                    .is_some_and(|po| ptr::eq(po.vmobjlock(), uobj.vmobjlock()))
            }));
            pg.clear_bits(PG_CLEAN);
            dropswap = true;
        }
    } else {
        uvm_pageactivate(pg);
    }

    if let (true, Some(obj)) = (dropswap, uobj) {
        uao_dropswap(obj, (pg.offset.get() >> PAGE_SHIFT) as i32);
    }

    if pg.flags() & PG_WANTED != 0 {
        wakeup(ptr::from_ref(pg));
    }

    pg.clear_bits(PG_BUSY | PG_FAKE | PG_WANTED);
    // UVM_PAGE_OWN(pg, NULL): not configured.
    uvmfault_unlockall(Some(ufi), amap, uobj);
    pmap_update(pmap);

    Ok(())
}

/// `uvm_fault_lower_io`: get lower page from backing store.
///
/// 1. unlock everything, because i/o will block.
/// 2. call pgo_get.
/// 3. if failed, recover.
/// 4. if succeeded, relock everything and verify things.
///
/// Yields the object page (`PGO_DONTCARE` when a `UVM_ET_NOFAULT` entry's read failed) and
/// updates `uobj` to the page's object (`None` in that case).
fn uvm_fault_lower_io(
    ufi: &UvmFaultinfo,
    flt: &mut UvmFaultctx,
    ruobj: &mut Option<&'static UvmObject>,
) -> Result<*const VmPage, Errno> {
    let entry = ufi.entry();
    let amap = entry.aref.amap();
    let Some(uobj0) = *ruobj else {
        panic(format_args!("uvm_fault_lower_io: no object to read from"));
    };

    // grab everything we need from the entry before we unlock
    let uoff = (ufi.orig_rvaddr - entry.start.get()) as Voff + entry.offset.get();
    let access_type = flt.access_type & mask(entry);
    let advice = entry.advice.get();

    // Upgrade to a write lock if needed.
    if let Err(e) = uvm_fault_lower_upgrade(flt, uobj0) {
        uvmfault_unlockall(Some(ufi), amap, Some(uobj0));
        return Err(e);
    }
    uvmfault_unlockall(Some(ufi), amap, None);

    // update rusage counters
    ru_majflt_inc();

    kassert!(rw_write_held(uobj0.vmobjlock()));

    counters_inc(UvmExpCounters::FltGet);
    let mut gotpages = 1;
    let mut pgs: [*const VmPage; 1] = [ptr::null()];
    let Some(pgo_get) = uobj0.pgops().pgo_get else {
        panic(format_args!("uvm_fault_lower_io: object without pgo_get"));
    };
    let result = pgo_get(
        uobj0,
        uoff,
        &mut pgs,
        &mut gotpages,
        0,
        access_type,
        advice,
        PGO_SYNCIO,
    );
    let mut pg = pgs[0];
    let mut uobj = Some(uobj0);

    // recover from I/O
    if result != VM_PAGER_OK {
        kassert!(result != VM_PAGER_PEND);

        if result == VM_PAGER_AGAIN {
            let _ = tsleep_nsec(nowake(), PVM, "fltagain2", msec_to_nsec(5));
            return Err(Errno::ERESTART);
        }

        if !uvm_et_isnofault(entry) {
            return Err(Errno::EIO);
        }

        pg = PGO_DONTCARE;
        uobj = None;
        flt.promote = true;
    }

    // re-verify the state of the world.
    let mut locked = uvmfault_relock(Some(ufi));
    if locked && let Some(amap) = amap {
        amap_lock(amap, flt.upper_lock_type);
    }

    // might be changed
    if let Some(page) = page_at(pg) {
        uobj = page.uobject();
        if let Some(obj) = uobj {
            let _ = rw_enter(obj.vmobjlock(), flt.lower_lock_type);
        }
        kassert!(page.flags() & PG_BUSY != 0);
        kassert!(flt.lower_lock_type == RW_WRITE);
    }

    // Re-verify that amap slot is still free. if there is a problem, we clean up.
    if locked
        && amap.is_some()
        && amap_lookup(&entry.aref, ufi.orig_rvaddr - entry.start.get()).is_some()
    {
        uvmfault_unlockall(Some(ufi), amap, None);
        locked = false;
    }

    // release the page now, still holding object lock
    if let Some(page) = page_at(pg) {
        uvm_pageactivate(page);

        if page.flags() & PG_WANTED != 0 {
            wakeup(ptr::from_ref(page));
        }
        page.clear_bits(PG_BUSY | PG_WANTED);
        // UVM_PAGE_OWN(pg, NULL): not configured.
    }

    if !locked {
        if let (Some(_), Some(obj)) = (page_at(pg), uobj) {
            rw_exit(obj.vmobjlock());
        }
        return Err(Errno::ERESTART);
    }

    // we have the data in pg. we are holding object lock (so the page can't be released on
    // us).
    *ruobj = uobj;
    Ok(pg)
}

/// `uvm_fault_wire`: wire down a range of virtual addresses in a map.
///
/// - map may be read-locked by caller, but MUST NOT be write-locked.
/// - if map is read-locked, any operations which may cause map to be write-locked in
///   `uvm_fault()` must be taken care of by the caller. See `uvm_map_pageable()`.
pub fn uvm_fault_wire(
    map: &VmMap,
    start: usize,
    end: usize,
    access_type: VmProt,
) -> Result<(), Errno> {
    // now fault it in a page at a time. if the fault fails then we have to undo what we
    // have done. note that in uvm_fault PROT_NONE is replaced with the max protection if
    // fault_type is VM_FAULT_WIRE.
    let mut va = start;
    while va < end {
        if let Err(rv) = uvm_fault(map, va, VM_FAULT_WIRE, access_type) {
            if va != start {
                uvm_fault_unwire(map, start, va);
            }
            return Err(rv);
        }
        va += PAGE_SIZE;
    }

    Ok(())
}

/// `uvm_fault_unwire`: unwire range of virtual space.
pub fn uvm_fault_unwire(map: &VmMap, start: usize, end: usize) {
    vm_map_lock_read(map);
    uvm_fault_unwire_locked(map, start, end);
    vm_map_unlock_read(map);
}

/// `uvm_fault_unwire_locked`: the guts of `uvm_fault_unwire()`.
///
/// - map must be at least read-locked.
pub fn uvm_fault_unwire_locked(map: &VmMap, start: usize, end: usize) {
    let pmap = map.pmap();

    kassert!(map.flags.get() & VM_MAP_INTRSAFE == 0);
    vm_map_assert_anylock(map);

    // we assume that the area we are unwiring has actually been wired in the first place.
    // this means that we should be able to extract the PAs from the pmap.

    // find the beginning map entry for the region.
    kassert!(start >= map.min_offset.get() && end <= map.max_offset.get());
    let Some(mut entry) = uvm_map_lookup_entry(map, start) else {
        panic(format_args!("uvm_fault_unwire_locked: address not in map"));
    };

    let mut oentry: Option<&VmMapEntry> = None;
    let mut va = start;
    while va < end {
        // find the map entry for the current address.
        kassert!(va >= entry.start.get());
        while va >= entry.end.get() {
            let next = RbtHead::<UvmMapAddr>::next(entry);
            kassert!(next.is_some_and(|n| n.start.get() <= entry.end.get()));
            let Some(next) = next else {
                panic(format_args!("uvm_fault_unwire_locked: ran off the map"));
            };
            entry = next;
        }

        // lock it.
        if !oentry.is_some_and(|o| ptr::eq(o, entry)) {
            if let Some(o) = oentry {
                uvm_map_unlock_entry(o);
            }
            uvm_map_lock_entry(entry);
            oentry = Some(entry);
        }

        if let Some(pa) = pmap_extract(pmap, Vaddr::new(va)) {
            // if the entry is no longer wired, tell the pmap.
            if !vm_mapent_iswired(entry) {
                pmap_unwire(pmap, Vaddr::new(va));
            }

            if let Some(pg) = PHYS_TO_VM_PAGE(pa) {
                uvm_pageunwire(pg);
            }
        }
        va += PAGE_SIZE;
    }

    if oentry.is_some() {
        uvm_map_unlock_entry(entry);
    }
}

/// `uvmfault_unlockmaps`: unlock the maps.
pub fn uvmfault_unlockmaps(ufi: Option<&UvmFaultinfo>, write_locked: bool) {
    // ufi can be NULL when this isn't really a fault, but merely paging in anon data.
    let Some(ufi) = ufi else {
        return;
    };

    uvmfault_update_stats(ufi);
    if write_locked {
        vm_map_unlock(ufi.map());
    } else {
        vm_map_unlock_read(ufi.map());
    }
}

/// `uvmfault_unlockall`: unlock everything passed in.
///
/// - maps must be read-locked (not write-locked).
pub fn uvmfault_unlockall(
    ufi: Option<&UvmFaultinfo>,
    amap: Option<&VmAmap>,
    uobj: Option<&UvmObject>,
) {
    if let Some(uobj) = uobj {
        rw_exit(uobj.vmobjlock());
    }
    if let Some(amap) = amap {
        amap_unlock(amap);
    }
    uvmfault_unlockmaps(ufi, false);
}

/// `uvmfault_lookup`: lookup a virtual address in a map.
///
/// - caller must provide a `uvm_faultinfo` structure with the IN params properly filled in
/// - we will lookup the map entry (handling submaps) as we go
/// - if the lookup is a success we will return with the maps locked
/// - if "write_lock" is true, we write_lock the map, otherwise we only get a read lock.
/// - note that submaps can only appear in the kernel and they are required to use the same
///   virtual addresses as the map they are referenced by (thus address translation between
///   the main map and the submap is unnecessary).
pub fn uvmfault_lookup(ufi: &mut UvmFaultinfo, write_lock: bool) -> bool {
    // init ufi values for lookup.
    ufi.map = ufi.orig_map;
    ufi.size = ufi.orig_size;

    // keep going down levels until we are done. note that there can only be two levels so
    // we won't loop very long.
    loop {
        let map = ufi.map();
        if ufi.orig_rvaddr < map.min_offset.get() || ufi.orig_rvaddr >= map.max_offset.get() {
            return false;
        }

        // lock map
        if write_lock {
            vm_map_lock(map);
        } else {
            vm_map_lock_read(map);
        }

        // lookup
        let Some(entry) = uvm_map_lookup_entry(map, ufi.orig_rvaddr) else {
            uvmfault_unlockmaps(Some(ufi), write_lock);
            return false;
        };
        ufi.entry = ptr::from_ref(entry);

        // reduce size if necessary
        if entry.end.get() - ufi.orig_rvaddr < ufi.size {
            ufi.size = entry.end.get() - ufi.orig_rvaddr;
        }

        // submap? replace map with the submap and lookup again. note: VAs in submaps must
        // match VAs in main map.
        if uvm_et_issubmap(entry) {
            let Some(tmpmap) = entry.sub_map() else {
                panic(format_args!("uvmfault_lookup: submap entry without a map"));
            };
            uvmfault_unlockmaps(Some(ufi), write_lock);
            ufi.map = ptr::from_ref(tmpmap);
            continue;
        }

        // got it!
        ufi.mapv = map.timestamp.get();
        return true;
    }
}

/// `uvmfault_relock`: attempt to relock the same version of the map.
///
/// - fault data structures should be unlocked before calling.
/// - if a success (true) maps will be locked after call.
pub fn uvmfault_relock(ufi: Option<&UvmFaultinfo>) -> bool {
    // ufi can be NULL when this isn't really a fault, but merely paging in anon data.
    let Some(ufi) = ufi else {
        return true;
    };

    // relock map. fail if version mismatch (in which case nothing gets locked).
    let map = ufi.map();
    vm_map_lock_read(map);
    if ufi.mapv != map.timestamp.get() {
        vm_map_unlock_read(map);
        counters_inc(UvmExpCounters::FltNorelck);
        return false;
    }

    counters_inc(UvmExpCounters::FltRelck);
    true // got it!
}
/* </CODE> */

/* <TESTS> */
#[cfg(test)]
mod tests {
    // Host tests for the fault handler: a page fault on an anonymous mapping of a user vmspace,
    // served against the host pmap double (see `uvm_map.rs` for the map side).

    use std::sync::MutexGuard;
    use std::{assert, assert_eq, assert_ne};

    use super::*;
    use crate::kern::kern_rwlock::rw_obj_init;
    use crate::kern::subr_pool::tests::setup_real_memory;
    use crate::machine::vmparam::VmParam;
    use crate::sys::mman::{MAP_INHERIT_COPY, PROT_EXEC, PROT_READ};
    use crate::sys::proc::Process;
    use crate::uvm::uvm_amap::{amap_init, amap_refs};
    use crate::uvm::uvm_anon::uvm_anon_init;
    use crate::uvm::uvm_extern::{UVM_FLAG_COPYONW, UVM_FLAG_FIXED, Vmspace, uvm_mapflag};
    use crate::uvm::uvm_map::{
        uvm_map_init, uvm_mapanon, uvmspace_alloc, uvmspace_fork, uvmspace_free,
    };
    use crate::uvm::uvmexp::counters_read;

    const RW: VmProt = PROT_READ | PROT_WRITE;

    /// Real memory, the locks, the anon, amap and map pools and the console.
    fn setup() -> MutexGuard<'static, ()> {
        let guard = setup_real_memory();
        crate::machine::cons::consinit();
        rw_obj_init();
        uvm_anon_init();
        amap_init();
        uvm_map_init();
        uvmfault_init();
        guard
    }

    /// A vmspace with `len` bytes of private anonymous read-write memory at `addr`.
    fn vmspace_with_anon(addr: usize, len: usize) -> &'static Vmspace {
        let vm = uvmspace_alloc(
            <Machine as VmParam>::VM_MIN_ADDRESS,
            <Machine as VmParam>::VM_MAXUSER_ADDRESS,
            true,
            false,
        );
        vm.vm_daddr.set(256 << 20);
        vm.vm_minsaddr.set(<Machine as VmParam>::USRSTACK);
        vm.vm_maxsaddr
            .set(<Machine as VmParam>::USRSTACK - <Machine as VmParam>::MAXSSIZ);
        let mut a = addr;
        let flags = uvm_mapflag(
            RW,
            RW | PROT_EXEC,
            MAP_INHERIT_COPY,
            MADV_NORMAL,
            UVM_FLAG_FIXED | UVM_FLAG_COPYONW,
        );
        uvm_mapanon(&vm.vm_map, &mut a, len, 0, flags).expect("mapanon");
        vm
    }

    /// The anon the amap of `vm` holds at `va`, under the map lock.
    fn anon_of(vm: &Vmspace, va: usize) -> Option<&'static VmAnon> {
        let map = &vm.vm_map;
        vm_map_lock_read(map);
        let anon =
            uvm_map_lookup_entry(map, va).and_then(|e| amap_lookup(&e.aref, va - e.start.get()));
        vm_map_unlock_read(map);
        anon
    }

    #[test]
    fn zero_fill_fault_maps_a_fresh_anon_page() {
        let _g = setup();
        let va = 0x2000_0000;
        let vm = vmspace_with_anon(va, 4 * PAGE_SIZE);
        let pmap = vm.vm_map.pmap();
        assert!(pmap_extract(pmap, Vaddr::new(va)).is_none());
        let faults = counters_read(UvmExpCounters::Faults);

        // A read fault: the lazy amap is made, an anon with a zeroed page is promoted and
        // mapped (case 2B), the neighbours are not (no anons yet).
        uvm_fault(&vm.vm_map, va + 8, VM_FAULT_INVALID, PROT_READ).expect("read fault");
        let pa = pmap_extract(pmap, Vaddr::new(va)).expect("mapped");
        let anon = anon_of(vm, va).expect("an anon");
        let pg = anon.page().expect("its page");
        assert_eq!(vm_page_to_phys(pg), pa.trunc_page());
        assert_ne!(pg.flags() & PQ_ANON, 0);
        assert_eq!(pg.flags() & (PG_BUSY | PG_FAKE), 0);
        assert_eq!(anon.an_ref.get(), 1);
        assert_eq!(counters_read(UvmExpCounters::Faults), faults + 1);

        // A write fault on the same page is case 1A: the same anon and page stay.
        uvm_fault(&vm.vm_map, va, VM_FAULT_INVALID, PROT_WRITE).expect("write fault");
        assert!(anon_of(vm, va).is_some_and(|a| core::ptr::eq(a, anon)));
        assert_eq!(pmap_extract(pmap, Vaddr::new(va)), Some(pa));

        // The next page faults in with its own anon; the first stays.
        uvm_fault(&vm.vm_map, va + PAGE_SIZE, VM_FAULT_INVALID, PROT_WRITE).expect("second page");
        let anon2 = anon_of(vm, va + PAGE_SIZE).expect("second anon");
        assert!(!core::ptr::eq(anon, anon2));
        uvmspace_free(vm);
    }

    #[test]
    fn faults_outside_or_beyond_protection_fail() {
        let _g = setup();
        let va = 0x3000_0000;
        let vm = vmspace_with_anon(va, PAGE_SIZE);

        // Free space: a case 0 fault.
        assert_eq!(
            uvm_fault(&vm.vm_map, va + 0x10_0000, VM_FAULT_INVALID, PROT_READ),
            Err(Errno::EFAULT)
        );
        // Outside the map.
        assert_eq!(
            uvm_fault(&vm.vm_map, 0, VM_FAULT_INVALID, PROT_READ),
            Err(Errno::EFAULT)
        );
        // More access than the entry allows.
        assert_eq!(
            uvm_fault(&vm.vm_map, va, VM_FAULT_INVALID, PROT_EXEC),
            Err(Errno::EACCES)
        );
        uvmspace_free(vm);
    }

    #[test]
    fn copy_on_write_after_fork_promotes_a_new_anon() {
        let _g = setup();
        let va = 0x4000_0000;
        let vm = vmspace_with_anon(va, PAGE_SIZE);
        uvm_fault(&vm.vm_map, va, VM_FAULT_INVALID, PROT_WRITE).expect("parent fault");
        let parent_anon = anon_of(vm, va).expect("parent anon");
        let parent_pa = pmap_extract(vm.vm_map.pmap(), Vaddr::new(va)).expect("parent pa");

        let pr = Process::new();
        pr.ps_vmspace.set(vm);
        let child = uvmspace_fork(&pr);
        // The amap is shared and the anon has two references until the child writes.
        let parent_amap = {
            let map = &vm.vm_map;
            vm_map_lock_read(map);
            let amap = uvm_map_lookup_entry(map, va).and_then(|e| e.aref.amap());
            vm_map_unlock_read(map);
            amap.expect("parent amap")
        };
        assert_eq!(amap_refs(parent_amap), 2);

        // The child's read fault sees the parent's page, read-only (case 1A, an_ref > 1).
        uvm_fault(&child.vm_map, va, VM_FAULT_INVALID, PROT_READ).expect("child read");
        assert_eq!(
            pmap_extract(child.vm_map.pmap(), Vaddr::new(va)),
            Some(parent_pa)
        );

        // The child's write fault copies the amap (needs_copy), then promotes a new anon with
        // a copy of the page (case 1B): the two maps now differ.
        uvm_fault(&child.vm_map, va, VM_FAULT_INVALID, PROT_WRITE).expect("child write");
        let child_anon = anon_of(child, va).expect("child anon");
        assert!(!core::ptr::eq(child_anon, parent_anon));
        assert_eq!(parent_anon.an_ref.get(), 1);
        assert_eq!(child_anon.an_ref.get(), 1);
        let child_pa = pmap_extract(child.vm_map.pmap(), Vaddr::new(va)).expect("child pa");
        assert_ne!(child_pa, parent_pa);
        assert_eq!(
            pmap_extract(vm.vm_map.pmap(), Vaddr::new(va)),
            Some(parent_pa)
        );
        assert_eq!(amap_refs(parent_amap), 1);
        assert!(counters_read(UvmExpCounters::FltAcow) >= 1);

        uvmspace_free(child);
        uvmspace_free(vm);
    }

    #[test]
    fn wire_and_unwire_a_range() {
        let _g = setup();
        let va = 0x5000_0000;
        let len = 3 * PAGE_SIZE;
        let vm = vmspace_with_anon(va, len);
        let pmap = vm.vm_map.pmap();

        uvm_fault_wire(&vm.vm_map, va, va + len, RW).expect("wire");
        for i in 0..3 {
            let pa = pmap_extract(pmap, Vaddr::new(va + i * PAGE_SIZE)).expect("mapped");
            let pg = PHYS_TO_VM_PAGE(pa).expect("a managed page");
            assert_eq!(pg.wire_count.get(), 1, "page {i} wired");
            assert_eq!(pg.flags() & PG_CLEAN, 0);
        }

        uvm_fault_unwire(&vm.vm_map, va, va + len);
        for i in 0..3 {
            let pa = pmap_extract(pmap, Vaddr::new(va + i * PAGE_SIZE)).expect("still mapped");
            let pg = PHYS_TO_VM_PAGE(pa).expect("a managed page");
            assert_eq!(pg.wire_count.get(), 0, "page {i} unwired");
        }
        uvmspace_free(vm);
    }

    #[test]
    fn neighbour_anons_are_mapped_ahead() {
        let _g = setup();
        let va = 0x6000_0000;
        let vm = vmspace_with_anon(va, 8 * PAGE_SIZE);
        let pmap = vm.vm_map.pmap();

        // Fault the pages in one by one, then drop their mappings: the anons stay.
        for i in 0..8 {
            uvm_fault(&vm.vm_map, va + i * PAGE_SIZE, VM_FAULT_INVALID, PROT_WRITE).expect("fault");
        }
        crate::machine::pmap::pmap_remove(pmap, Vaddr::new(va), Vaddr::new(va + 8 * PAGE_SIZE));
        assert!(pmap_extract(pmap, Vaddr::new(va + 4 * PAGE_SIZE)).is_none());

        // One read fault in the middle maps the MADV_NORMAL window (3 back, 4 ahead) too.
        uvm_fault(&vm.vm_map, va + 4 * PAGE_SIZE, VM_FAULT_INVALID, PROT_READ).expect("refault");
        for i in 1..8 {
            assert!(
                pmap_extract(pmap, Vaddr::new(va + i * PAGE_SIZE)).is_some(),
                "page {i} mapped by the fault-ahead"
            );
        }
        assert!(
            pmap_extract(pmap, Vaddr::new(va)).is_none(),
            "page 0 is behind the window"
        );
        assert!(counters_read(UvmExpCounters::FltNamap) >= 6);
        uvmspace_free(vm);
    }
}
/* </TESTS> */
