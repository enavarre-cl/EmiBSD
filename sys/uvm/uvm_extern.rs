/*	$OpenBSD: uvm_extern.h,v 1.190 2026/07/24 15:03:50 kettenis Exp $	*/
/*	$NetBSD: uvm_extern.h,v 1.57 2001/03/09 01:02:12 chs Exp $	*/
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
/* </LICENSES> */

//! The external interface of `uvm`: `<uvm/uvm_extern.h>`.
//!
//! Upstream: sys/uvm/uvm_extern.h @ 3ce1f3f79392
//!
//! Status: `wip`. Milestone M3 has the types and flags the page allocator and `km_alloc` use:
//! `voff_t`, `vm_prot_t`, the map flag encoding, the `UVM_PGA_*`/`UVM_PLA_*` flags,
//! `PHYSLOAD_DEVICE`, `struct uvm_constraint_range` and the `kmem_*_mode` structures.
//! M6 adds `struct vmspace`. The kernel maps and the prototypes arrive with the files that
//! implement them.
//!
//! ## Deviations
//! - `UVM_MAPFLAG` and its extractors are `const fn`s.
//! - In `kmem_va_mode`, `kv_map` (a pointer to a map pointer) waits for `vm_map`; the mode
//!   says which map by name until then.
//! - `exec_map`, which every machine's `machdep.c` defines and makes identically in
//!   `cpu_startup`, is one static here ([`EXEC_MAP`]) that the machines set.

use core::cell::Cell;
use core::ptr;
use core::sync::atomic::{AtomicI32, AtomicPtr, Ordering};

use crate::sys::errno::Errno;
use crate::sys::mman::{PROT_EXEC, PROT_READ, PROT_WRITE};
use crate::sys::types::{Off, Paddr, Segsz};
use crate::uvm::uvm_map::VmMap;

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

/// `struct vmspace`: shareable process virtual address space. May eventually be merged with
/// `vm_map`. Several fields are temporary (text, data stuff).
///
/// Locks used to protect struct members: `K` kernel lock, `I` immutable after creation, `a`
/// atomic operations, `v` `vm_map`'s lock.
///
/// `#[repr(C)]` so that `vm_map` is at offset 0, as the C relies on (`(struct vmspace *)map`).
#[repr(C)]
pub struct Vmspace {
    /// `vm_map`: VM address map.
    pub vm_map: VmMap,
    /// \[a\] `vm_refcnt`: number of references.
    pub vm_refcnt: AtomicI32,
    // vm_shm: SYSVSHM is not configured.
    // We copy from vm_startcopy (= vm_rssize) to the end of the structure on fork.
    /// `vm_rssize`: current resident set size in pages.
    pub vm_rssize: Cell<Segsz>,
    /// `vm_swrss`: resident set size before last swap.
    pub vm_swrss: Cell<Segsz>,
    /// `vm_tsize`: text size (pages) XXX.
    pub vm_tsize: Cell<Segsz>,
    /// `vm_dsize`: data size (pages) XXX.
    pub vm_dsize: Cell<Segsz>,
    /// `vm_dused`: data segment length (pages) XXX.
    pub vm_dused: Cell<Segsz>,
    /// \[v\] `vm_ssize`: stack size (pages).
    pub vm_ssize: Cell<Segsz>,
    /// \[I\] `vm_taddr`: user virtual address of text.
    pub vm_taddr: Cell<usize>,
    /// \[I\] `vm_daddr`: user virtual address of data.
    pub vm_daddr: Cell<usize>,
    /// \[I\] `vm_maxsaddr`: user VA at max stack growth.
    pub vm_maxsaddr: Cell<usize>,
    /// \[I\] `vm_minsaddr`: user VA at top of stack.
    pub vm_minsaddr: Cell<usize>,
}

impl Vmspace {
    /// A zero vmspace, before `uvmspace_init` (what a `static struct vmspace` holds).
    pub const fn new() -> Self {
        Self {
            vm_map: VmMap::new(),
            vm_refcnt: AtomicI32::new(0),
            vm_rssize: Cell::new(0),
            vm_swrss: Cell::new(0),
            vm_tsize: Cell::new(0),
            vm_dsize: Cell::new(0),
            vm_dused: Cell::new(0),
            vm_ssize: Cell::new(0),
            vm_taddr: Cell::new(0),
            vm_daddr: Cell::new(0),
            vm_maxsaddr: Cell::new(0),
            vm_minsaddr: Cell::new(0),
        }
    }

    /// `memcpy(&vm2->vm_startcopy, &vm1->vm_startcopy, ...)`: copies the statistics and
    /// boundaries from `other` (fork).
    pub fn copy_startcopy_from(&self, other: &Vmspace) {
        self.vm_rssize.set(other.vm_rssize.get());
        self.vm_swrss.set(other.vm_swrss.get());
        self.vm_tsize.set(other.vm_tsize.get());
        self.vm_dsize.set(other.vm_dsize.get());
        self.vm_dused.set(other.vm_dused.get());
        self.vm_ssize.set(other.vm_ssize.get());
        self.vm_taddr.set(other.vm_taddr.get());
        self.vm_daddr.set(other.vm_daddr.get());
        self.vm_maxsaddr.set(other.vm_maxsaddr.get());
        self.vm_minsaddr.set(other.vm_minsaddr.get());
    }

    /// `memset(&vm->vm_startcopy, 0, ...)`: nukes the statistics and boundaries (exec).
    pub fn clear_startcopy(&self) {
        self.copy_startcopy_from(&Vmspace::new());
    }
}

impl Default for Vmspace {
    fn default() -> Self {
        Self::new()
    }
}

// SAFETY: a vmspace is shared between the threads of its process; the fields are `Cell`s and
// atomics whose protection the field docs name (the map's lock, the kernel lock).
unsafe impl Sync for Vmspace {}

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

/// `exec_map`: the submap of `kernel_map` that holds `execve`'s argument buffers (16
/// `NCARGS`, which effectively limits the number of processes exec'ing at any time); null
/// until the machine's `cpu_startup` made it.
pub static EXEC_MAP: AtomicPtr<crate::uvm::uvm_map::VmMap> = AtomicPtr::new(ptr::null_mut());

/// `exec_map`, once `cpu_startup` made it.
pub fn exec_map() -> &'static crate::uvm::uvm_map::VmMap {
    let map = EXEC_MAP.load(Ordering::Acquire);
    // SAFETY: a non-null pointer is the submap `uvm_km_suballoc` returned, never freed.
    match unsafe { map.as_ref() } {
        Some(map) => map,
        None => crate::kern::subr_prf::panic(format_args!("exec_map used before cpu_startup")),
    }
}

/// Which kernel map a `kmem_va_mode` allocates from (`kv_map` in C is a pointer to the map
/// pointer; the maps themselves arrive with `uvm_map`).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum KvMap {
    /// `kernel_map`.
    Kernel,
    /// `kmem_map`.
    Kmem,
    /// `exec_map` (`kv_exec`, `kern_exec.c`).
    Exec,
    /// No map: the single page allocator (`kv_singlepage`).
    None,
}

/// `struct kmem_va_mode`: allocation mode for virtual space.
#[derive(Clone, Copy)]
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
#[derive(Clone, Copy)]
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
#[derive(Clone, Copy)]
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

/// `uvm_coredump_setup_cb`: called once with the number of segments a core dump will hold.
pub type UvmCoredumpSetupCb =
    fn(nsegment: i32, cookie: *mut core::ffi::c_void) -> Result<(), Errno>;

/// `uvm_coredump_walk_cb`: called for each segment of a core dump: `[start, realend)` holds
/// data, `[realend, end)` is absent (zero) memory.
pub type UvmCoredumpWalkCb = fn(
    start: usize,
    realend: usize,
    end: usize,
    prot: VmProt,
    isvnode: bool,
    nsegment: i32,
    cookie: *mut core::ffi::c_void,
) -> Result<(), Errno>;

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
