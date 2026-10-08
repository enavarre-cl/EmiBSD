/*	$OpenBSD: uvm_init.c,v 1.46 2026/05/17 10:46:25 mpi Exp $	*/
/*	$NetBSD: uvm_init.c,v 1.14 2000/06/27 17:29:23 mrg Exp $	*/
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
 * from: Id: uvm_init.c,v 1.1.2.3 1998/02/06 05:15:27 chs Exp
 */
/* </LICENSES> */

/* <CODE> */
//! Init the vm system: `uvm/uvm_init.c`. All global vars are stored in `struct uvm` to make
//! them easier to spot.
//!
//! Upstream: sys/uvm/uvm_init.c @ 3ce1f3f79392
//!
//! Status: `wip`. Milestone M3 brings up the page system; every later step of `uvm_init`
//! reports itself through `unported!` until its file lands (the map, the kernel object, the
//! pagers, amaps, anons, the kmem allocators).
//!
//! ## Deviations
//! - `uvmexp_counters` (the C's per-CPU `COUNTERS_BOOT_MEMORY`) are the static atomics of
//!   `uvmexp` (`docs/C_TO_RUST.md`, the `struct cpumem *` counters row), so `uvm_init_percpu`
//!   does not call `counters_alloc_ncpus` (which exists since M11a, `kern/subr_percpu.rs`).
//! - `averunnable.fscale` waits for `kern_synch.c` (M5).

use core::sync::atomic::{AtomicUsize, Ordering};

use crate::kern::kern_malloc::kmeminit;
use crate::machine::{Machine, Pmap, VmParam};
use crate::sys::types::{Vaddr, Vsize};
use crate::unported;
use crate::uvm::uvm::Uvm;
use crate::uvm::uvm_addr::uaddr_bestfit_create;
use crate::uvm::uvm_amap::amap_init;
use crate::uvm::uvm_anon::{uvm_anon_init, uvm_anon_init_percpu};
use crate::uvm::uvm_aobj::{UAO_FLAG_KERNSWAP, uao_create};
use crate::uvm::uvm_fault::uvmfault_init;
use crate::uvm::uvm_km::{kernel_map, uvm_km_init};
use crate::uvm::uvm_map::{UVM_MAXKADDR, UvmMapUaddrSlot, uvm_map_init, uvm_map_set_uaddr};
use crate::uvm::uvm_page::uvm_page_init;
use crate::uvm::uvm_pager::uvm_pager_init;
use crate::uvm::uvm_param::VM_KERNEL_SPACE_SIZE;
use crate::uvm::uvmexp::Uvmexp;

/// `uvm`: the VM's global state.
pub static UVM: Uvm = Uvm::new();
/// `uvmexp`: the exported statistics, under its C name for the interrupt stubs' `V_INTR`.
#[unsafe(export_name = "uvmexp")]
pub static UVMEXP: Uvmexp = Uvmexp::new();
/// `vm_min_kernel_address`: base of kernel virtual memory.
pub static VM_MIN_KERNEL_ADDRESS: AtomicUsize =
    AtomicUsize::new(<Machine as VmParam>::VM_MIN_KERNEL_ADDRESS);

/// `uvm_init`: init the VM system. Called from `kern/init_main.c`.
pub fn uvm_init() {
    let mut kvm_start = Vaddr::new(0);
    let mut kvm_end = Vaddr::new(0);

    // Ensure that the hardware set the page size.
    if UVMEXP.pagesize.load(Ordering::Relaxed) == 0 {
        #[allow(clippy::panic)] // the C panics here too
        {
            panic!("uvm_init: page size not set");
        }
    }

    // averunnable.fscale = FSCALE: M5.

    // Init the page sub-system. This includes allocating the vm_page structures, and setting
    // up all the page queues (and locks). Available memory will be put in the "free" queue,
    // kvm_start and kvm_end will be set to the area of kernel virtual memory which is
    // available for general use.
    uvm_page_init(&mut kvm_start, &mut kvm_end);

    // Init the map sub-system. Allocates the static pool of vm_map_entry structures that are
    // used for "special" kernel maps (e.g. kernel_map, kmem_map, etc...).
    uvm_map_init();

    // Setup the kernel's virtual memory data structures. This includes setting up the
    // kernel_map/kernel_object.
    // uvm_maxkaddr's static initialiser is VM_MIN_KERNEL_ADDRESS; where that base is a
    // runtime value (amd64 above its direct map) uvm_maxkaddr starts there too.
    let base = VM_MIN_KERNEL_ADDRESS.load(Ordering::Relaxed);
    UVM_MAXKADDR.fetch_max(base, Ordering::Relaxed);
    uvm_km_init(Vaddr::new(base), kvm_start, kvm_end);

    // step 4.5: init (tune) the fault recovery code.
    uvmfault_init();

    // Init the pmap module. The pmap module is free to allocate memory for its private use
    // (e.g. pvlists).
    Machine::pmap_init();

    // step 6: init uvm_km_page allocator memory.
    let _ = unported!("uvm_km_page_init");

    // Make kernel memory allocators ready for use. After this call the malloc memory allocator
    // can be used.
    kmeminit();

    // step 7.5: init the dma allocator, which is backed by pools.
    let _ = unported!("dma_alloc_init");

    // Init all pagers and the pager_map.
    uvm_pager_init();

    // step 9: init anonymous memory system
    amap_init(); // init amap module

    // step 10: start uvm_km_page allocator thread.
    let _ = unported!("uvm_km_page_lateinit");

    // the VM system is now up! now that malloc is up we can enable paging of kernel objects.
    let _ = uao_create(Vsize::new(VM_KERNEL_SPACE_SIZE), UAO_FLAG_KERNSWAP);

    // DEADBEEF0 / DEADBEEF1: not configured.

    // Init anonymous memory systems.
    uvm_anon_init();

    // Switch kernel and kmem_map over to a best-fit allocator, instead of walking the tree
    // (SMALL_KERNEL is not configured; kmem_map waits for kmeminit's uvm_km_suballoc).
    let kmap = kernel_map();
    uvm_map_set_uaddr(
        kmap,
        UvmMapUaddrSlot::Any(3),
        Some(uaddr_bestfit_create(
            kmap.min_offset.get(),
            kmap.max_offset.get(),
        )),
    );
}

/// `uvm_init_percpu`: the per-CPU parts, once the CPUs are known.
pub fn uvm_init_percpu() {
    // uvmexp_counters = counters_alloc_ncpus(uvmexp_counters, exp_ncounters): the uvmexp
    // counters are static atomics here (see the module's deviations), so there is nothing to
    // move to per-CPU memory.
    uvm_anon_init_percpu();
}
/* </CODE> */
