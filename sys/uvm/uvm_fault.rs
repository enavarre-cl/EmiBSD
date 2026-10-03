/*	$OpenBSD: uvm_fault.h,v 1.16 2020/11/06 11:52:39 mpi Exp $	*/
/*	$NetBSD: uvm_fault.h,v 1.14 2000/06/26 14:21:17 mrg Exp $	*/
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
/* </LICENSES> */

//! `<uvm/uvm_fault.h>`: the fault types and `struct uvm_faultinfo`.
//!
//! Upstream: sys/uvm/uvm_fault.h @ 3ce1f3f79392
//!
//! Status: `wip`. Milestone M7a (part 1) has the types the pager operations name; the
//! fault handler itself (`uvm_fault.c`: `uvmfault_lookup`, `uvmfault_relock`,
//! `uvmfault_unlockall`, `uvmfault_anonget`, `uvm_fault`, `uvm_fault_wire`,
//! `uvm_fault_unwire`) is M7a part 3.

use core::ptr;

use crate::sys::errno::Errno;
use crate::sys::types::{Vaddr, Vsize};
use crate::unported;
use crate::uvm::uvm_extern::{VmFault, VmProt};
use crate::uvm::uvm_map::{VmMap, VmMapEntry};

/// `VM_FAULT_INVALID`: invalid mapping.
pub const VM_FAULT_INVALID: VmFault = 0x0;
/// `VM_FAULT_PROTECT`: protection.
pub const VM_FAULT_PROTECT: VmFault = 0x1;
/// `VM_FAULT_WIRE`: wire mapping.
pub const VM_FAULT_WIRE: VmFault = 0x2;

/// `struct uvm_faultinfo`: to load one of these fill in all `orig_*` fields and then call
/// `uvmfault_lookup` on it.
pub struct UvmFaultinfo {
    /// IN: original map.
    pub orig_map: *const VmMap,
    /// IN: original rounded VA.
    pub orig_rvaddr: Vaddr,
    /// IN: original size of interest.
    pub orig_size: Vsize,
    /// map (could be a submap).
    pub map: *const VmMap,
    /// map's version number.
    pub mapv: u32,
    /// map entry (from 'map').
    pub entry: *const VmMapEntry,
    /// size of interest.
    pub size: Vsize,
}

impl UvmFaultinfo {
    /// A fault info for `orig_map`, `orig_rvaddr` and `orig_size`, before `uvmfault_lookup`.
    pub fn new(orig_map: &VmMap, orig_rvaddr: Vaddr, orig_size: Vsize) -> Self {
        Self {
            orig_map: ptr::from_ref(orig_map),
            orig_rvaddr,
            orig_size,
            map: ptr::null(),
            mapv: 0,
            entry: ptr::null(),
            size: Vsize::new(0),
        }
    }
}

/// `uvm_fault_wire(map, start, end, access_type)`: wires the pages of `[start, end)` in;
/// reported until the fault handler exists (M7a-3).
pub fn uvm_fault_wire(
    _map: &VmMap,
    _start: usize,
    _end: usize,
    _access_type: VmProt,
) -> Result<(), Errno> {
    Err(unported!("uvm_fault_wire (uvm_fault.c, M7a-3)"))
}

/// `uvm_fault_unwire_locked(map, start, end)`: unwires the pages of `[start, end)` with the
/// map locked; reported until the fault handler exists (M7a-3).
pub fn uvm_fault_unwire_locked(_map: &VmMap, _start: usize, _end: usize) {
    let _ = unported!("uvm_fault_unwire_locked (uvm_fault.c, M7a-3)");
}
