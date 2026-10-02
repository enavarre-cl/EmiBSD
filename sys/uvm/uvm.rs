/*	$OpenBSD: uvm.h,v 1.73 2024/04/02 08:39:17 deraadt Exp $	*/
/*	$NetBSD: uvm.h,v 1.24 2000/11/27 08:40:02 chs Exp $	*/

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

//! The `uvm` structure, vm global state collected in one structure for ease of reference:
//! `<uvm/uvm.h>`.
//!
//! Upstream: sys/uvm/uvm.h @ 3ce1f3f79392
//!
//! Status: `wip`. Milestone M3 has the page queues, `page_init_done` and the pmemrange
//! control; the locks (M5), the daemons' triggers (M5), `kentry_free`, `aio_done` and
//! `kernel_object` arrive with the map, the buffer cache and the kernel object (M6).
//!
//! Locks used to protect struct members in this file: `Q` `uvm.pageqlock`, `F`
//! `uvm.fpageqlock`.

use core::sync::atomic::AtomicBool;

use crate::uvm::uvm_page::Pglist;
use crate::uvm::uvm_pmemrange::UvmPmrControl;

/// `struct uvm`.
pub struct Uvm {
    // vm_page related parameters: vm_page queues
    /// \[Q\] allocated pages, in use.
    pub page_active: Pglist,
    /// \[Q\] pages inactive (reclaim/free).
    pub page_inactive: Pglist,
    // Lock order: pageqlock, then fpageqlock. (M5)
    /// TRUE if `uvm_page_init()` finished.
    pub page_init_done: AtomicBool,
    /// \[F\] pmemrange data.
    pub pmr_control: UvmPmrControl,
}

// SAFETY: every field is guarded by one of the locks named in the module doc (M5); until then
// the single boot CPU is the lock, and `page_init_done` is atomic as in C.
unsafe impl Sync for Uvm {}

impl Uvm {
    /// The state before `uvm_init`.
    pub const fn new() -> Self {
        Self {
            page_active: Pglist::new(),
            page_inactive: Pglist::new(),
            page_init_done: AtomicBool::new(false),
            pmr_control: UvmPmrControl::new(),
        }
    }
}

impl Default for Uvm {
    fn default() -> Self {
        Self::new()
    }
}

// vm_map_entry etype bits:

/// It is a uvm_object.
pub const UVM_ET_OBJ: i32 = 0x0001;
/// It is a vm_map submap.
pub const UVM_ET_SUBMAP: i32 = 0x0002;
/// Copy_on_write.
pub const UVM_ET_COPYONWRITE: i32 = 0x0004;
/// Needs_copy.
pub const UVM_ET_NEEDSCOPY: i32 = 0x0008;
/// No backend.
pub const UVM_ET_HOLE: i32 = 0x0010;
/// Don't fault.
pub const UVM_ET_NOFAULT: i32 = 0x0020;
/// This is a stack.
pub const UVM_ET_STACK: i32 = 0x0040;
/// Write combining.
pub const UVM_ET_WC: i32 = 0x0080;
/// Omit from dumps.
pub const UVM_ET_CONCEAL: i32 = 0x0100;
/// Entry may not be changed.
pub const UVM_ET_IMMUTABLE: i32 = 0x0400;
/// Map entry is on free list (DEBUG).
pub const UVM_ET_FREEMAPPED: i32 = 0x8000;
