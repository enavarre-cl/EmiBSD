/*	$OpenBSD: uvm_extern.h,v 1.190 2026/07/24 15:03:50 kettenis Exp $	*/
/*	$NetBSD: uvm_extern.h,v 1.57 2001/03/09 01:02:12 chs Exp $	*/

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
 */

/*-
 * Copyright (c) 1991, 1992, 1993
 *	The Regents of the University of California.  All rights reserved.
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
 *	@(#)vm_extern.h	8.5 (Berkeley) 5/3/95
 */

//! The external interface of `uvm`: `<uvm/uvm_extern.h>`.
//!
//! Upstream: sys/uvm/uvm_extern.h @ 3ce1f3f79392
//!
//! Status: `wip`. Milestone M3 has the types and flags the page allocator and `km_alloc` use:
//! `voff_t`, `vm_prot_t`, the map flag encoding, the `UVM_PGA_*`/`UVM_PLA_*` flags,
//! `PHYSLOAD_DEVICE`, `struct uvm_constraint_range` and the `kmem_*_mode` structures.
//! `struct vmspace` (needs `vm_map`), the kernel maps and the prototypes arrive with the files
//! that implement them.
//!
//! ## Deviations
//! - `UVM_MAPFLAG` and its extractors are `const fn`s.
//! - In `kmem_va_mode`, `kv_map` (a pointer to a map pointer) waits for `vm_map`; the mode
//!   says which map by name until then.

use crate::sys::mman::{PROT_EXEC, PROT_READ, PROT_WRITE};
use crate::sys::types::{Off, Paddr};

/// `vm_fault_t`.
pub type VmFault = i32;
/// `vm_inherit_t`: XXX inheritance codes.
pub type VmInherit = i32;
/// `voff_t`: XXX offset within a uvm_object.
pub type Voff = Off;
/// `vm_prot_t`.
pub type VmProt = i32;

// Bit assignments assigned by UVM_MAPFLAG() and extracted by
// UVM_{PROTECTION,INHERIT,MAXPROTECTION,ADVICE}(): bits 0-2 protection, bits 4-5 inheritance,
// bits 8-10 max protection, bits 12-14 advice, bits 16-N flags.

/// Protections bits.
pub const PROT_MASK: i32 = PROT_READ | PROT_WRITE | PROT_EXEC;
/// Inherit mask.
pub const MAP_INHERIT_MASK: i32 = 0x3;
/// Advice mask.
pub const MADV_MASK: i32 = 0x7;

// mapping flags

/// Find space.
pub const UVM_FLAG_FIXED: u32 = 0x0010000;
/// Establish overlay.
pub const UVM_FLAG_OVERLAY: u32 = 0x0020000;
/// Don't merge map entries.
pub const UVM_FLAG_NOMERGE: u32 = 0x0040000;
/// Set copy_on_write flag.
pub const UVM_FLAG_COPYONW: u32 = 0x0080000;
/// Fail if we can not lock map.
pub const UVM_FLAG_TRYLOCK: u32 = 0x0100000;
/// No backend.
pub const UVM_FLAG_HOLE: u32 = 0x0200000;
/// Do everything, except actual execution.
pub const UVM_FLAG_QUERY: u32 = 0x0400000;
/// Don't fault.
pub const UVM_FLAG_NOFAULT: u32 = 0x0800000;
/// Unmap to make space.
pub const UVM_FLAG_UNMAP: u32 = 0x1000000;
/// Page may contain a stack.
pub const UVM_FLAG_STACK: u32 = 0x2000000;
/// Write combining.
pub const UVM_FLAG_WC: u32 = 0x4000000;
/// Omit from dumps.
pub const UVM_FLAG_CONCEAL: u32 = 0x8000000;
/// Sigaltstack validation required.
pub const UVM_FLAG_SIGALTSTACK: u32 = 0x20000000;

/// `UVM_PROTECTION(X)`.
pub const fn uvm_protection(x: u32) -> VmProt {
    (x as i32) & PROT_MASK
}

/// `UVM_INHERIT(X)`.
pub const fn uvm_inherit(x: u32) -> VmInherit {
    ((x >> 4) as i32) & MAP_INHERIT_MASK
}

/// `UVM_MAXPROTECTION(X)`.
pub const fn uvm_maxprotection(x: u32) -> VmProt {
    ((x >> 8) as i32) & PROT_MASK
}

/// `UVM_ADVICE(X)`.
pub const fn uvm_advice(x: u32) -> i32 {
    ((x >> 12) as i32) & MADV_MASK
}

/// `UVM_MAPFLAG(prot, maxprot, inh, advice, flags)`.
pub const fn uvm_mapflag(
    prot: VmProt,
    maxprot: VmProt,
    inh: VmInherit,
    advice: i32,
    flags: u32,
) -> u32 {
    (prot as u32) | ((maxprot as u32) << 8) | ((inh as u32) << 4) | ((advice as u32) << 12) | flags
}

/// Magic offset value: offset not known (obj) or don't care (!obj).
pub const UVM_UNKNOWN_OFFSET: Voff = -1;

// flags for uvm_pagealloc()

/// Ok to use reserve pages.
pub const UVM_PGA_USERESERVE: i32 = 0x0001;
/// Returned page must be zeroed.
pub const UVM_PGA_ZERO: i32 = 0x0002;

// flags for uvm_pglistalloc() also used by uvm_pmr_getpages()

/// May sleep.
pub const UVM_PLA_WAITOK: i32 = 0x0001;
/// Can't sleep (need one of the two).
pub const UVM_PLA_NOWAIT: i32 = 0x0002;
/// Zero all pages before returning.
pub const UVM_PLA_ZERO: i32 = 0x0004;
/// Try to allocate contig physmem.
pub const UVM_PLA_TRYCONTIG: i32 = 0x0008;
/// Don't wake page daemon on failure.
pub const UVM_PLA_NOWAKE: i32 = 0x0020;
/// Can allocate from kernel reserve.
pub const UVM_PLA_USERESERVE: i32 = 0x0040;

// lockflags that control the locking behavior of various functions.

/// Map locked on entry.
pub const UVM_LK_ENTER: i32 = 0x0000_0001;
/// Leave map locked on exit.
pub const UVM_LK_EXIT: i32 = 0x0000_0002;

/// Flag to `uvm_page_physload`: don't add to the page queue.
pub const PHYSLOAD_DEVICE: i32 = 0x01;

/// `struct uvm_constraint_range`: MD code is allowed to setup constraint ranges for memory
/// allocators, the primary use for this is to keep allocation for certain memory consumers such
/// as mbuf pools within address ranges that are reachable by devices that perform DMA. It is also
/// to discourage memory allocations from being satisfied from ranges such as the ISA memory
/// range, if they can be satisfied with allocation from other ranges.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct UvmConstraintRange {
    /// Lowest allowed address.
    pub ucr_low: Paddr,
    /// Highest allowed address.
    pub ucr_high: Paddr,
}

/// Which kernel map a `kmem_va_mode` allocates from (`kv_map` in C is a pointer to the map
/// pointer; the maps themselves arrive with `uvm_map`).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum KvMap {
    /// `kernel_map`.
    Kernel,
    /// `kmem_map`.
    Kmem,
    /// No map: the single page allocator (`kv_singlepage`).
    None,
}

/// `struct kmem_va_mode`: allocation mode for virtual space.
pub struct KmemVaMode {
    /// The map we're allocating from.
    pub kv_map: KvMap,
    /// Alignment.
    pub kv_align: usize,
    /// Wait for free space in the map if it's full. The default allocators don't wait since
    /// running out of space in kernel_map and kmem_map is usually fatal. Special maps like
    /// exec_map are specifically limited, so waiting for space in them is necessary.
    pub kv_wait: bool,
    /// Use the single page allocator.
    pub kv_singlepage: bool,
}

/// `struct kmem_pa_mode`: allocation mode for physical pages.
pub struct KmemPaMode {
    /// Allocation constraint for physical pages.
    pub kp_constraint: &'static UvmConstraintRange,
    /// If the pages should be allocated from an object: the kernel object (`kp_pageable`).
    pub kp_object: bool,
    /// Physical alignment of the first page in the allocation.
    pub kp_align: Paddr,
    /// Boundary that the physical addresses can't cross if the allocation is contiguous.
    pub kp_boundary: Paddr,
    /// Maximal amount of contiguous segments.
    pub kp_maxseg: i32,
    /// Don't allocate any backing pages.
    pub kp_nomem: bool,
    /// Zero the returned memory.
    pub kp_zero: bool,
    /// Allocate pageable memory.
    pub kp_pageable: bool,
}

/// `struct kmem_dyn_mode`: dynamic allocation parameters. Stuff that changes too often or too
/// much to create separate va and pa modes for.
pub struct KmemDynMode {
    /// Offset to feed to `PMAP_PREFER`.
    pub kd_prefer: Voff,
    /// Special parameter for the singlepage va allocator that tells the caller to sleep if
    /// possible to let the singlepage allocator catch up (`kd_slowdown` in C is a pointer the
    /// allocator writes through).
    pub kd_slowdown: bool,
    /// Is it ok to sleep?
    pub kd_waitok: bool,
    /// Don't sleep on map locks.
    pub kd_trylock: bool,
}

/// `KMEM_DYN_INITIALIZER`.
pub const KMEM_DYN_INITIALIZER: KmemDynMode = KmemDynMode {
    kd_prefer: UVM_UNKNOWN_OFFSET,
    kd_slowdown: false,
    kd_waitok: false,
    kd_trylock: false,
};

#[cfg(test)]
mod tests {
    use super::*;
    use crate::sys::mman::{MADV_RANDOM, MAP_INHERIT_NONE};

    #[test]
    fn map_flags_round_trip() {
        let f = uvm_mapflag(
            PROT_READ | PROT_WRITE,
            PROT_MASK,
            MAP_INHERIT_NONE,
            MADV_RANDOM,
            UVM_FLAG_FIXED,
        );
        assert_eq!(uvm_protection(f), PROT_READ | PROT_WRITE);
        assert_eq!(uvm_maxprotection(f), PROT_MASK);
        assert_eq!(uvm_inherit(f), MAP_INHERIT_NONE);
        assert_eq!(uvm_advice(f), MADV_RANDOM);
        assert_ne!(f & UVM_FLAG_FIXED, 0);
    }
}
