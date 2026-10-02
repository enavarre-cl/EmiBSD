/* $OpenBSD: pmap.c,v 1.113 2025/07/12 08:35:32 kettenis Exp $ */
/*
 * Copyright (c) 2008-2009,2014-2016 Dale Rahn <drahn@dalerahn.com>
 *
 * Permission to use, copy, modify, and distribute this software for any
 * purpose with or without fee is hereby granted, provided that the above
 * copyright notice and this permission notice appear in all copies.
 *
 * THE SOFTWARE IS PROVIDED "AS IS" AND THE AUTHOR DISCLAIMS ALL WARRANTIES
 * WITH REGARD TO THIS SOFTWARE INCLUDING ALL IMPLIED WARRANTIES OF
 * MERCHANTABILITY AND FITNESS. IN NO EVENT SHALL THE AUTHOR BE LIABLE FOR
 * ANY SPECIAL, DIRECT, INDIRECT, OR CONSEQUENTIAL DAMAGES OR ANY DAMAGES
 * WHATSOEVER RESULTING FROM LOSS OF USE, DATA OR PROFITS, WHETHER IN AN
 * ACTION OF CONTRACT, NEGLIGENCE OR OTHER TORTIOUS ACTION, ARISING OUT OF
 * OR IN CONNECTION WITH THE USE OR PERFORMANCE OF THIS SOFTWARE.
 */

//! arm64 physical map: `arch/arm64/arm64/pmap.c`.
//!
//! Upstream: sys/arch/arm64/arm64/pmap.c @ 3ce1f3f79392
//!
//! Status: `wip`. Milestone M3 ports `struct pte_desc`, the kernel pmap, `pmap_bootstrap`'s
//! kernel-pmap setup, `pmap_zero_page`, `pmap_copy_page` and `pmap_virtual_space`. The
//! virtual-to-physical tables (`pmap_vp_*`), `pmap_kenter_*`, `pmap_extract`,
//! `pmap_growkernel`, the pools and `pmap_init`'s TTBR0 switch follow with the page tables.
//!
//! ## Deviations
//! - OpenBSD arm64 has no direct map and no `PMAP_STEAL_MEMORY`: boot allocations go through
//!   `pmap_steal_avail` (a bump allocator over `pmap_avail[]`, mapped by `pmap_map_stolen`)
//!   and `uvm_pageboot_alloc` maps page by page with `pmap_kenter_pa`. Both need page tables
//!   the kernel does not own yet, so until it does the bootloader's higher-half direct map
//!   serves as `__HAVE_PMAP_DIRECT` and boot memory is stolen through it: `pmap_steal_memory`
//!   takes contiguous frames out of `vm_physmem[]` with `uvm_page_physsteal` (the
//!   `vm_physmem[]` half of amd64's `pmap_steal_memory`, `uvm/uvm_page.rs`).
//! - `pmap_zero_page`/`pmap_copy_page` use the direct map instead of the per-CPU
//!   `zero_page`/`copy_src_page`/`copy_dst_page` windows (which need `pmap_kenter_pa`).
//! - `pmap_bootstrap`: the kernel pmap's `pm_pt0pa` is `TTBR1_EL1` (the bootloader's tables);
//!   `pmap_setup_avail`, `pmap_map_stolen`, the ASID and the pointer authentication keys come
//!   with the page tables and M6.
//! - `pmap_init`: the TTBR0 switch to the kernel's own level-0 table and the pools wait for
//!   the page tables; `initarm`'s bootstrap device map stays. Reported as unported.
//! - `pmap_extract` answers for the direct map only; `pmap_kenter_pa`, `pmap_kremove` and
//!   `pmap_growkernel` are reported as unported.

use core::arch::asm;
use core::cell::Cell;
use core::ptr;
use core::sync::atomic::{AtomicBool, AtomicUsize, Ordering};

use crate::arch::arm64::include::param::PAGE_SIZE;
use crate::arch::arm64::include::pmap::Pmap;
use crate::arch::arm64::include::vmparam::{VM_MAX_KERNEL_ADDRESS, VM_MIN_KERNEL_ADDRESS};
use crate::sys::queue::ListEntry;
use crate::sys::types::{Paddr, Vaddr, Vsize};
use crate::uvm::uvm_extern::VmProt;
use crate::uvm::uvm_page::{PHYS_TO_VM_PAGE, VmPage, uvm_page_physsteal, vm_page_to_phys};
use crate::uvm::uvm_param::atop;
use crate::{queue_adapter, unported};

/// `TTBR1_EL1.BADDR`: bits 47:1 of the register hold the table's physical address.
const TTBR_BADDR_MASK: u64 = 0x0000_ffff_ffff_fffe;

/// `struct pte_desc`: one mapping, on its page's pv list.
pub struct PteDesc {
    /// `pted_pv_list`: the page's mappings.
    pub pted_pv_list: ListEntry<PteDesc>,
    /// The PTE as it should be.
    pub pted_pte: Cell<u64>,
    /// The pmap this mapping belongs to.
    pub pted_pmap: Cell<*const Pmap>,
    /// The virtual address, with the `PTED_VA_*` flags in the low bits.
    pub pted_va: Cell<Vaddr>,
}

queue_adapter!(
    /// `LIST_HEAD(, pte_desc)`: the pv list of a page.
    pub PvList: PteDesc, pted_pv_list => ListEntry<PteDesc>
);

/// The direct map's base: physical address 0 is mapped here (see the module's deviations).
pub static PMAP_DIRECT_BASE: AtomicUsize = AtomicUsize::new(0);
/// The end of the direct map.
pub static PMAP_DIRECT_END: AtomicUsize = AtomicUsize::new(0);
/// `virtual_avail`: the first free kernel virtual address.
static VIRTUAL_AVAIL: AtomicUsize = AtomicUsize::new(0);
/// `pmap_virtual_space_called`: prevent further KVA stealing.
static PMAP_VIRTUAL_SPACE_CALLED: AtomicBool = AtomicBool::new(false);
/// `kernel_pmap_`: the kernel's pmap.
static KERNEL_PMAP: Pmap = Pmap::new();

/// `pmap_kernel()`.
pub fn pmap_kernel() -> &'static Pmap {
    &KERNEL_PMAP
}

/// The direct-map address of a physical address.
pub fn pmap_direct_map(pa: Paddr) -> Vaddr {
    Vaddr::new(PMAP_DIRECT_BASE.load(Ordering::Relaxed) + pa.as_usize())
}

/// The physical address behind a direct-map address.
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

/// `pmap_bootstrap`: sets up the kernel pmap and the kernel virtual range; returns the first
/// free kernel virtual address.
///
/// # Safety
///
/// Call once, on the boot CPU, after `initarm` has set the direct map's base and end, with the
/// MMU on and `TTBR1_EL1` holding the tables the kernel runs on.
pub unsafe fn pmap_bootstrap(_ram_start: Paddr, _ram_end: Paddr) -> Vaddr {
    // pmap_setup_avail, pmap_map_stolen: with the page tables (see the module's deviations).
    let ttbr1: u64;
    // SAFETY: reading a system register has no side effects.
    unsafe { asm!("mrs {}, ttbr1_el1", out(reg) ttbr1, options(nomem, nostack, preserves_flags)) };

    let pmap = pmap_kernel();
    pmap.pm_pt0pa.set(ttbr1 & TTBR_BADDR_MASK);
    pmap.have_4_level_pt.set(true);
    pmap.pm_privileged.set(true);
    pmap.pm_asid.set(0);
    pmap.pm_refs.set(1);

    let vstart = VM_MIN_KERNEL_ADDRESS;
    VIRTUAL_AVAIL.store(vstart, Ordering::Relaxed);
    Vaddr::new(vstart)
}

/// `pmap_init`: the pools and the TTBR0 switch wait for the page tables.
pub fn pmap_init() {
    let _ = unported!("pmap_init (arm64: pools, TTBR0 switch)");
}

/// `pagezero_cache`: zeroes the page at `va` (the C uses `dc zva`).
fn pagezero_cache(va: Vaddr) {
    // SAFETY: the caller passes the direct-map address of a RAM page it owns.
    unsafe { ptr::write_bytes(va.as_usize() as *mut u8, 0, PAGE_SIZE) };
}

/// `pmap_zero_page`: fill the given physical page with zeros.
pub fn pmap_zero_page(pg: &VmPage) {
    pagezero_cache(pmap_map_direct(pg));
}

/// `pmap_copy_page`: copy the given physical page.
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

/// `pmap_steal_memory` (not in the C, see the module's deviations): `size` bytes of free
/// physical memory, zeroed, through the direct map.
///
/// # Safety
///
/// Only before `uvm_page_init` has run, on the boot CPU.
pub unsafe fn pmap_steal_memory(
    size: Vsize,
    start: Option<&mut Vaddr>,
    end: Option<&mut Vaddr>,
) -> Vaddr {
    let size = size.round_page().as_usize();
    let npg = atop(size);

    // The segment bookkeeping is `uvm_page_physsteal` (see the module's deviations).
    let Some(pa) = uvm_page_physsteal(npg) else {
        #[allow(clippy::panic)] // what pmap_steal_avail does when it cannot allocate
        {
            panic!(
                "pmap_steal_memory: unable to allocate region with size {:#x}",
                size
            );
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

/// `pmap_virtual_space`: the free kernel virtual range; prevents further KVA stealing.
pub fn pmap_virtual_space(start: &mut Vaddr, end: &mut Vaddr) {
    *start = Vaddr::new(VIRTUAL_AVAIL.load(Ordering::Relaxed));
    *end = Vaddr::new(VM_MAX_KERNEL_ADDRESS);

    // Prevent further KVA stealing.
    PMAP_VIRTUAL_SPACE_CALLED.store(true, Ordering::Relaxed);
}

/// `pmap_kenter_pa`: enter a kernel mapping; waits for the page tables.
///
/// # Safety
///
/// As for `machine::Pmap::pmap_kenter_pa`.
pub unsafe fn pmap_kenter_pa(_va: Vaddr, _pa: Paddr, _prot: VmProt) {
    let _ = unported!("pmap_kenter_pa (arm64 page tables)");
}

/// `pmap_kremove`: remove kernel mappings; waits for the page tables.
///
/// # Safety
///
/// As for `machine::Pmap::pmap_kremove`.
pub unsafe fn pmap_kremove(_va: Vaddr, _len: Vsize) {
    let _ = unported!("pmap_kremove (arm64 page tables)");
}

/// `pmap_extract`: the physical address behind `va`. Only the direct map is known yet.
pub fn pmap_extract(_pm: &Pmap, va: Vaddr) -> Option<Paddr> {
    if pmap_direct_mapped(va) {
        return Some(pmap_direct_unmap(va));
    }
    let _ = unported!("pmap_extract outside the direct map (arm64 page tables)");
    None
}

/// `pmap_growkernel`: waits for the page tables; reports how far they reach now (nowhere).
pub fn pmap_growkernel(_maxkvaddr: Vaddr) -> Vaddr {
    let _ = unported!("pmap_growkernel (arm64 page tables)");
    Vaddr::new(VIRTUAL_AVAIL.load(Ordering::Relaxed))
}
