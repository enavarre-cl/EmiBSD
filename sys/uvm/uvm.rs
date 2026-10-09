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
 * from: Id: uvm.h,v 1.1.2.14 1998/02/02 20:07:19 chuck Exp
 */
/* </LICENSES> */

/* <CODE> */
//! The `uvm` structure, vm global state collected in one structure for ease of reference:
//! `<uvm/uvm.h>`.
//!
//! Upstream: sys/uvm/uvm.h @ 3ce1f3f79392
//!
//! Status: `wip`. Milestone M3 has the page queues, `page_init_done` and the pmemrange
//! control; M7a adds `kernel_object` and the `UVM_ET_*` entry types; the daemons' triggers
//! and `aio_done` arrive with the buffer cache; M7a-2 adds `kentry_free`. M11a adds
//! `fpageqlock`, M11e `pageqlock`.
//!
//! Locks used to protect struct members in this file: `Q` `uvm.pageqlock`, `F`
//! `uvm.fpageqlock`.

use core::cell::Cell;
use core::ptr;
use core::sync::atomic::AtomicBool;

use crate::machine::intr::IPL_VM;
use crate::sys::mutex::Mutex;
use crate::sys::queue::SlistHead;
use crate::uvm::uvm_map::{UvmKentryFree, VmMapEntry};
use crate::uvm::uvm_object::UvmObject;

use crate::uvm::uvm_page::Pglist;
use crate::uvm::uvm_pmemrange::UvmPmrControl;

/// `struct uvm`.
pub struct Uvm {
    // vm_page related parameters: vm_page queues
    /// \[Q\] allocated pages, in use.
    pub page_active: Pglist,
    /// \[Q\] pages inactive (reclaim/free).
    pub page_inactive: Pglist,
    // Lock order: pageqlock, then fpageqlock.
    /// `pageqlock`: lock for active/inactive page q.
    pub pageqlock: Mutex,
    /// `fpageqlock`: lock for free page q + pdaemon.
    pub fpageqlock: Mutex,
    /// TRUE if `uvm_page_init()` finished.
    pub page_init_done: AtomicBool,
    /// \[F\] pmemrange data.
    pub pmr_control: UvmPmrControl,
    /// `kentry_free`: free page pool (the static kernel map entries, guarded by
    /// `uvm_kmapent_mtx`).
    pub kentry_free: SlistHead<UvmKentryFree>,
    // kernel object
    /// `kernel_object`: the kernel's anonymous object (`uao_create` with
    /// `UAO_FLAG_KERNOBJ`), null before `uvm_km_init`.
    pub kernel_object: Cell<*const UvmObject>,
}

// SAFETY: every field is guarded by one of the locks named in the module doc (`Q`, `F`), or
// written once on the boot CPU before the other processors start (`kernel_object`,
// `kentry_free`'s initial fill, under `uvm_kmapent_mtx` afterwards); `page_init_done` is
// atomic as in C.
unsafe impl Sync for Uvm {}

impl Uvm {
    /// The state before `uvm_init`.
    pub const fn new() -> Self {
        Self {
            page_active: Pglist::new(),
            page_inactive: Pglist::new(),
            pageqlock: Mutex::new(IPL_VM),
            fpageqlock: Mutex::new(IPL_VM),
            page_init_done: AtomicBool::new(false),
            pmr_control: UvmPmrControl::new(),
            kentry_free: SlistHead::new(),
            kernel_object: Cell::new(ptr::null()),
        }
    }

    /// `uvm.kernel_object`, once `uvm_km_init` made it.
    pub fn kernel_object(&self) -> Option<&'static UvmObject> {
        // SAFETY: the kernel object is a static (`kernel_object_store`), alive forever.
        unsafe { self.kernel_object.get().as_ref() }
    }
}

impl Default for Uvm {
    fn default() -> Self {
        Self::new()
    }
}

// vm_map_entry etype bits:

/// `UVM_ET_OBJ`: it is a uvm_object.
pub const UVM_ET_OBJ: i32 = 0x0001;
/// `UVM_ET_SUBMAP`: it is a vm_map submap.
pub const UVM_ET_SUBMAP: i32 = 0x0002;
/// `UVM_ET_COPYONWRITE`: copy_on_write.
pub const UVM_ET_COPYONWRITE: i32 = 0x0004;
/// `UVM_ET_NEEDSCOPY`: needs_copy.
pub const UVM_ET_NEEDSCOPY: i32 = 0x0008;
/// `UVM_ET_HOLE`: no backend.
pub const UVM_ET_HOLE: i32 = 0x0010;
/// `UVM_ET_NOFAULT`: don't fault.
pub const UVM_ET_NOFAULT: i32 = 0x0020;
/// `UVM_ET_STACK`: this is a stack.
pub const UVM_ET_STACK: i32 = 0x0040;
/// `UVM_ET_WC`: write combining.
pub const UVM_ET_WC: i32 = 0x0080;
/// `UVM_ET_CONCEAL`: omit from dumps.
pub const UVM_ET_CONCEAL: i32 = 0x0100;
/// `UVM_ET_IMMUTABLE`: entry may not be changed.
pub const UVM_ET_IMMUTABLE: i32 = 0x0400;
/// `UVM_ET_FREEMAPPED`: map entry is on free list (DEBUG).
pub const UVM_ET_FREEMAPPED: i32 = 0x8000;

/// `UVM_ET_ISOBJ(E)`.
pub fn uvm_et_isobj(e: &VmMapEntry) -> bool {
    e.etype.get() & UVM_ET_OBJ != 0
}

/// `UVM_ET_ISSUBMAP(E)`.
pub fn uvm_et_issubmap(e: &VmMapEntry) -> bool {
    e.etype.get() & UVM_ET_SUBMAP != 0
}

/// `UVM_ET_ISCOPYONWRITE(E)`.
pub fn uvm_et_iscopyonwrite(e: &VmMapEntry) -> bool {
    e.etype.get() & UVM_ET_COPYONWRITE != 0
}

/// `UVM_ET_ISNEEDSCOPY(E)`.
pub fn uvm_et_isneedscopy(e: &VmMapEntry) -> bool {
    e.etype.get() & UVM_ET_NEEDSCOPY != 0
}

/// `UVM_ET_ISHOLE(E)`.
pub fn uvm_et_ishole(e: &VmMapEntry) -> bool {
    e.etype.get() & UVM_ET_HOLE != 0
}

/// `UVM_ET_ISNOFAULT(E)`.
pub fn uvm_et_isnofault(e: &VmMapEntry) -> bool {
    e.etype.get() & UVM_ET_NOFAULT != 0
}

/// `UVM_ET_ISSTACK(E)`.
pub fn uvm_et_isstack(e: &VmMapEntry) -> bool {
    e.etype.get() & UVM_ET_STACK != 0
}

/// `UVM_ET_ISWC(E)`.
pub fn uvm_et_iswc(e: &VmMapEntry) -> bool {
    e.etype.get() & UVM_ET_WC != 0
}

/// `UVM_ET_ISCONCEAL(E)`.
pub fn uvm_et_isconceal(e: &VmMapEntry) -> bool {
    e.etype.get() & UVM_ET_CONCEAL != 0
}
/* </CODE> */
