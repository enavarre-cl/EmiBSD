/*	$OpenBSD: pmap.c,v 1.191 2026/06/04 05:22:04 mlarkin Exp $	*/
/*	$NetBSD: pmap.c,v 1.3 2003/05/08 18:13:13 thorpej Exp $	*/

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

/*
 * Copyright 2001 (c) Wasabi Systems, Inc.
 * All rights reserved.
 *
 * Written by Frank van der Linden for Wasabi Systems, Inc.
 *
 * Redistribution and use in source and binary forms, with or without
 * modification, are permitted provided that the following conditions
 * are met:
 * 1. Redistributions of source code must retain the above copyright
 *    notice, this list of conditions and the following disclaimer.
 * 2. Redistributions in binary form must reproduce the above copyright
 *    notice, this list of conditions and the following disclaimer in the
 *    documentation and/or other materials provided with the distribution.
 * 3. All advertising materials mentioning features or use of this software
 *    must display the following acknowledgement:
 *      This product includes software developed for the NetBSD Project by
 *      Wasabi Systems, Inc.
 * 4. The name of Wasabi Systems, Inc. may not be used to endorse
 *    or promote products derived from this software without specific prior
 *    written permission.
 *
 * THIS SOFTWARE IS PROVIDED BY WASABI SYSTEMS, INC. ``AS IS'' AND
 * ANY EXPRESS OR IMPLIED WARRANTIES, INCLUDING, BUT NOT LIMITED
 * TO, THE IMPLIED WARRANTIES OF MERCHANTABILITY AND FITNESS FOR A PARTICULAR
 * PURPOSE ARE DISCLAIMED.  IN NO EVENT SHALL WASABI SYSTEMS, INC
 * BE LIABLE FOR ANY DIRECT, INDIRECT, INCIDENTAL, SPECIAL, EXEMPLARY, OR
 * CONSEQUENTIAL DAMAGES (INCLUDING, BUT NOT LIMITED TO, PROCUREMENT OF
 * SUBSTITUTE GOODS OR SERVICES; LOSS OF USE, DATA, OR PROFITS; OR BUSINESS
 * INTERRUPTION) HOWEVER CAUSED AND ON ANY THEORY OF LIABILITY, WHETHER IN
 * CONTRACT, STRICT LIABILITY, OR TORT (INCLUDING NEGLIGENCE OR OTHERWISE)
 * ARISING IN ANY WAY OUT OF THE USE OF THIS SOFTWARE, EVEN IF ADVISED OF THE
 * POSSIBILITY OF SUCH DAMAGE.
 */

//! amd64 physical map: `arch/amd64/amd64/pmap.c`.
//!
//! Upstream: sys/arch/amd64/amd64/pmap.c @ 3ce1f3f79392
//!
//! This is the i386 pmap modified and generalized to support x86-64 as well. The idea is to
//! hide the upper N levels of the page tables inside `pmap_get_ptp`, `pmap_free_ptp` and
//! `pmap_growkernel`. The rest is mostly untouched, except that it uses some more generalized
//! macros and interfaces.
//!
//! Status: `wip`. Milestone M3 ports what the page allocator needs before the kernel has its
//! own page tables: the direct map (`PMAP_DIRECT_MAP`, `pmap_map_direct`), `pmap_bootstrap`'s
//! kernel-pmap setup, `pmap_steal_memory`, `pmap_zero_page`, `pmap_copy_page` and
//! `pmap_init`. The page-table walkers (`pmap_kenter_pa`, `pmap_kremove`, `pmap_extract`,
//! `pmap_growkernel`, `pmap_enter`, ...), the pv lists, the TLB shootdown and
//! `pmap_randomize` follow.
//!
//! ## Deviations
//! - `pmap_direct_base` is the bootloader's higher-half direct map (`BootInfo::hhdm_offset`),
//!   set by `init_x86_64` where the C's `init_x86_64` derives it from `L4_SLOT_DIRECT`;
//!   `pmap_bootstrap` does not build the direct map's page tables (`dmpdp`/`dmpd`): the
//!   bootloader's serve until the kernel owns its page tables. Limine maps at least 4 GiB and
//!   every memory-map region, which is all `pmap_steal_memory` and the page allocator touch.
//! - `virtual_avail` starts above the direct map when that map sits inside
//!   `[VM_MIN_KERNEL_ADDRESS, VM_MAX_KERNEL_ADDRESS)`, as Limine's default placement
//!   (`0xffff800000000000`) does; the C's `kva_start` is `VM_MIN_KERNEL_ADDRESS` because its
//!   direct map has PML4 slots of its own.
//! - `pmap_bootstrap`: the kernel pmap's `pm_pdir` is the PML4 in `CR3` (the bootloader's), not
//!   `proc0.p_addr->u_pcb.pcb_cr3` (no `proc0` until M5); the PKU probe, `protection_codes`,
//!   PCID, `pmap_randomize`, the early PTE pages and the low-memory PTPs come with the
//!   page-table step; `pm_obj` has no pager yet (`uvm_obj_init` with `pmap_pager`, M6).
//! - `pagezero` (`locore.S`) is `ptr::write_bytes`; `pmap_flush_cache` waits for `cpu_info`
//!   (`ci_cflushsz`, M5).
//! - `pmap_steal_memory`'s `vm_physmem[]` bookkeeping is `uvm_page_physsteal`
//!   (`uvm/uvm_page.rs`), shared with arm64 and the host double; the direct-map half is here.
//! - `pmap_virtual_space` is not in the C (amd64 has `PMAP_STEAL_MEMORY`); the trait needs one
//!   and it reports the range `pmap_steal_memory` reports.
//! - `pmap_extract` answers for the direct map only; `pmap_kenter_pa`, `pmap_kremove` and
//!   `pmap_growkernel` are reported as unported.

use core::ptr;
use core::sync::atomic::{AtomicBool, AtomicUsize, Ordering};

use crate::arch::amd64::include::cpufunc::rcr3;
use crate::arch::amd64::include::param::PAGE_SIZE;
use crate::arch::amd64::include::pmap::{PMAP_PA_MASK, PMAP_TYPE_NORMAL, Pmap};
use crate::arch::amd64::include::vmparam::{VM_MAX_KERNEL_ADDRESS, VM_MIN_KERNEL_ADDRESS};
use crate::sys::types::{Paddr, Vaddr, Vsize};
use crate::unported;
use crate::uvm::uvm_extern::VmProt;
use crate::uvm::uvm_page::{PHYS_TO_VM_PAGE, VmPage, uvm_page_physsteal, vm_page_to_phys};
use crate::uvm::uvm_param::atop;

/// `pmap_direct_base`: where physical address 0 is mapped (see the module's deviations).
pub static PMAP_DIRECT_BASE: AtomicUsize = AtomicUsize::new(0);
/// `pmap_direct_end`: the end of the direct map.
pub static PMAP_DIRECT_END: AtomicUsize = AtomicUsize::new(0);
/// `virtual_avail`: the first free kernel virtual address.
static VIRTUAL_AVAIL: AtomicUsize = AtomicUsize::new(0);
/// `pmap_initialized`: pmap_init done yet?
static PMAP_INITIALIZED: AtomicBool = AtomicBool::new(false);
/// `kernel_pmap_store`: the kernel's pmap (proc 0).
static KERNEL_PMAP_STORE: Pmap = Pmap::new();

/// `pmap_kernel()`.
pub fn pmap_kernel() -> &'static Pmap {
    &KERNEL_PMAP_STORE
}

/// `pmap_initialized`.
pub fn pmap_initialized() -> bool {
    PMAP_INITIALIZED.load(Ordering::Relaxed)
}

/// `PMAP_DIRECT_MAP(pa)`: the direct-map address of a physical address.
pub fn pmap_direct_map(pa: Paddr) -> Vaddr {
    Vaddr::new(PMAP_DIRECT_BASE.load(Ordering::Relaxed) + pa.as_usize())
}

/// `PMAP_DIRECT_UNMAP(va)`: the physical address behind a direct-map address.
pub fn pmap_direct_unmap(va: Vaddr) -> Paddr {
    Paddr::new(va.as_usize() - PMAP_DIRECT_BASE.load(Ordering::Relaxed))
}

/// Whether `va` lies in the direct map.
pub fn pmap_direct_mapped(va: Vaddr) -> bool {
    let base = PMAP_DIRECT_BASE.load(Ordering::Relaxed);
    let end = PMAP_DIRECT_END.load(Ordering::Relaxed);
    base <= va.as_usize() && va.as_usize() < end
}

/// `pmap_map_direct(pg)`: the direct-map address of a page.
pub fn pmap_map_direct(pg: &VmPage) -> Vaddr {
    pmap_direct_map(vm_page_to_phys(pg))
}

/// `pmap_unmap_direct(va)`: the page behind a direct-map address.
pub fn pmap_unmap_direct(va: Vaddr) -> Option<&'static VmPage> {
    PHYS_TO_VM_PAGE(pmap_direct_unmap(va))
}

/// `pmap_bootstrap`: get the system in a state where it can run with VM properly enabled
/// (called before `main()`). The VM system is fully init'd later...
///
/// # Safety
///
/// Call once, on the boot CPU, after `init_x86_64` has set `pmap_direct_base`/`pmap_direct_end`,
/// with paging on and `CR3` pointing at the page tables the kernel runs on.
pub unsafe fn pmap_bootstrap(first_avail: Paddr, _max_pa: Paddr) -> Paddr {
    let kva_start = VM_MIN_KERNEL_ADDRESS;

    // define the boundaries of the managed kernel virtual address space.
    let direct_base = PMAP_DIRECT_BASE.load(Ordering::Relaxed);
    let direct_end = PMAP_DIRECT_END.load(Ordering::Relaxed);
    let virtual_avail = if direct_base < VM_MAX_KERNEL_ADDRESS && direct_end > kva_start {
        // The direct map overlaps the managed range (see the module's deviations).
        direct_end.max(kva_start)
    } else {
        kva_start
    };
    VIRTUAL_AVAIL.store(virtual_avail, Ordering::Relaxed); // first free KVA

    // PKU (pg_xo) and protection_codes: with the page tables.

    // now we init the kernel's pmap
    //
    // the kernel pmap's pm_obj is not used for much. however, in user pmaps the pm_obj
    // contains the list of active PTPs. the pm_obj currently does not have a pager.
    let kpm = pmap_kernel();
    for hint in &kpm.pm_ptphint {
        hint.set(ptr::null());
    }
    let pdirpa = Paddr::new(rcr3() as usize & PMAP_PA_MASK);
    kpm.pm_pdir
        .set(pmap_direct_map(pdirpa).as_usize() as *mut _);
    kpm.pm_pdirpa.set(pdirpa);
    let resident = atop(kva_start - VM_MIN_KERNEL_ADDRESS) as i64;
    kpm.pm_stats.wired_count.set(resident);
    kpm.pm_stats.resident_count.set(resident);
    // the above is just a rough estimate and not critical to the proper operation of the
    // system.

    kpm.pm_type.set(PMAP_TYPE_NORMAL);

    // curpcb->pcb_pmap = kpm (proc0's pcb): M5. PCID, pmap_randomize, the direct map's own
    // page tables, the early PTE pages and the low-memory PTPs: with the page tables.

    first_avail
}

/// `pmap_init`: no further initialization required on this platform.
pub fn pmap_init() {
    PMAP_INITIALIZED.store(true, Ordering::Relaxed);
}

/// `pagezero` (`locore.S`): zeroes the page at `va`.
fn pagezero(va: Vaddr) {
    // SAFETY: the caller passes the direct-map address of a RAM page it owns.
    unsafe { ptr::write_bytes(va.as_usize() as *mut u8, 0, PAGE_SIZE) };
}

/// `pmap_zero_page`: zero a page.
pub fn pmap_zero_page(pg: &VmPage) {
    pagezero(pmap_map_direct(pg));
}

/// `pmap_copy_page`: copy a page.
pub fn pmap_copy_page(srcpg: &VmPage, dstpg: &VmPage) {
    let srcva = pmap_map_direct(srcpg);
    let dstva = pmap_map_direct(dstpg);

    // SAFETY: both are direct-map addresses of distinct RAM pages the caller owns.
    unsafe {
        ptr::copy_nonoverlapping(
            srcva.as_usize() as *const u8,
            dstva.as_usize() as *mut u8,
            PAGE_SIZE,
        )
    };
}

/// `pmap_steal_memory`: takes `size` bytes out of `vm_physmem[]` at an unused segment boundary,
/// zeroed, through the direct map.
///
/// # Safety
///
/// Only before `uvm_page_init` has run, on the boot CPU, with no `vm_physmem()` slice live.
pub unsafe fn pmap_steal_memory(
    size: Vsize,
    start: Option<&mut Vaddr>,
    end: Option<&mut Vaddr>,
) -> Vaddr {
    let size = size.round_page().as_usize();
    let npg = atop(size);

    // The segment bookkeeping is `uvm_page_physsteal` (see the module's deviations).
    let Some(pa) = uvm_page_physsteal(npg) else {
        #[allow(clippy::panic)] // the C panics here too
        {
            panic!("pmap_steal_memory: out of memory");
        }
    };

    let va = pmap_direct_map(pa);
    // SAFETY: `size` bytes of RAM at `va`, just taken out of the free segments, which the
    // direct map covers; nothing else refers to them.
    unsafe { ptr::write_bytes(va.as_usize() as *mut u8, 0, size) };

    if let Some(start) = start {
        *start = Vaddr::new(VIRTUAL_AVAIL.load(Ordering::Relaxed));
    }
    if let Some(end) = end {
        *end = Vaddr::new(VM_MAX_KERNEL_ADDRESS);
    }

    va
}

/// `pmap_virtual_space`: the free kernel virtual range (see the module's deviations).
pub fn pmap_virtual_space(start: &mut Vaddr, end: &mut Vaddr) {
    *start = Vaddr::new(VIRTUAL_AVAIL.load(Ordering::Relaxed));
    *end = Vaddr::new(VM_MAX_KERNEL_ADDRESS);
}

/// `pmap_kenter_pa`: enter a kernel mapping without pv tracking; waits for the page tables.
///
/// # Safety
///
/// As for `machine::Pmap::pmap_kenter_pa`.
pub unsafe fn pmap_kenter_pa(_va: Vaddr, _pa: Paddr, _prot: VmProt) {
    let _ = unported!("pmap_kenter_pa (amd64 page tables)");
}

/// `pmap_kremove`: remove kernel mappings; waits for the page tables.
///
/// # Safety
///
/// As for `machine::Pmap::pmap_kremove`.
pub unsafe fn pmap_kremove(_sva: Vaddr, _len: Vsize) {
    let _ = unported!("pmap_kremove (amd64 page tables)");
}

/// `pmap_extract`: extract a PA for the given VA. Only the direct map is known yet.
pub fn pmap_extract(_pmap: &Pmap, va: Vaddr) -> Option<Paddr> {
    if pmap_direct_mapped(va) {
        return Some(pmap_direct_unmap(va));
    }
    let _ = unported!("pmap_extract outside the direct map (amd64 page tables)");
    None
}

/// `pmap_growkernel`: waits for the page tables; reports how far they reach now (nowhere).
pub fn pmap_growkernel(_maxkvaddr: Vaddr) -> Vaddr {
    let _ = unported!("pmap_growkernel (amd64 page tables)");
    Vaddr::new(VIRTUAL_AVAIL.load(Ordering::Relaxed))
}
