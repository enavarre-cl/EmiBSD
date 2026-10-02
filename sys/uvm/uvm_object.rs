/*	$OpenBSD: uvm_object.h,v 1.30 2022/09/04 06:49:11 jsg Exp $	*/
/*	$NetBSD: uvm_object.h,v 1.11 2001/03/09 01:02:12 chs Exp $	*/
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
 * from: Id: uvm_object.h,v 1.1.2.2 1998/01/04 22:44:51 chuck Exp
 */
/* </LICENSES> */

//! The UVM memory object interface: `<uvm/uvm_object.h>`.
//!
//! Upstream: sys/uvm/uvm_object.h @ 3ce1f3f79392
//!
//! A UVM memory object represents a list of pages, which are managed by the object's pager
//! operations (`uvm_object::pgops`). All pages belonging to an object are owned by it and thus
//! protected by the object lock.
//!
//! Status: `wip`. Milestone M3 has the structure's page tree and counters, which the page
//! allocator maintains; `vmobjlock` (an `rwlock`, M5) and `pgops` (`uvm_pager.h`, M6) are not
//! here yet, so the `UVM_OBJ_IS_*` pager tests cannot be asked. `uvm_obj_init` and the rest
//! of `uvm_object.c` arrive with M6.
//!
//! ## Deviations
//! - Without `pgops`, `UVM_OBJ_IS_DUMMY` answers true: there is no object lock to check either.

use core::cell::Cell;
use core::cmp::Ordering;

use crate::sys::tree::{RbtEntry, RbtHead};
use crate::tree_adapter;
use crate::uvm::uvm_page::VmPage;

/// `UVM_OBJ_KERN` is a 'special' `uo_refs` value which indicates that the object is a kernel
/// memory object rather than a normal one (kernel memory objects don't have reference counts:
/// they never die).
pub const UVM_OBJ_KERN: i32 = -2;

/// `uvm_pagecmp`: orders an object's pages by offset.
pub fn uvm_pagecmp(a: &VmPage, b: &VmPage) -> Ordering {
    a.offset.get().cmp(&b.offset.get())
}

tree_adapter!(
    /// `uvm_objtree`: the pages of an object, by offset.
    pub UvmObjtree: VmPage, objt => RbtEntry, uvm_pagecmp
);

/// `struct uvm_object`.
pub struct UvmObject {
    /// Pages in object.
    pub memt: RbtHead<UvmObjtree>,
    /// # of pages in memt.
    pub uo_npages: Cell<i32>,
    /// Reference count.
    pub uo_refs: Cell<i32>,
}

// SAFETY: the object lock (`vmobjlock`, M5) guards the page tree and the counters; until then
// the single boot CPU is the lock.
unsafe impl Sync for UvmObject {}

impl UvmObject {
    /// An empty object with `refs` references.
    pub const fn new(refs: i32) -> Self {
        Self {
            memt: RbtHead::new(),
            uo_npages: Cell::new(0),
            uo_refs: Cell::new(refs),
        }
    }
}

/// `UVM_OBJ_IS_KERN_OBJECT(uobj)`.
pub fn uvm_obj_is_kern_object(uobj: &UvmObject) -> bool {
    uobj.uo_refs.get() == UVM_OBJ_KERN
}

/// `UVM_OBJ_IS_DUMMY(uobj)`: a pmap or bufcache object, which has no lock of its own (see the
/// module's deviations).
pub fn uvm_obj_is_dummy(_uobj: &UvmObject) -> bool {
    true
}
