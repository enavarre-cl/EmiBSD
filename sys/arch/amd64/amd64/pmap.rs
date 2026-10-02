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
//! Status: `wip`. Milestone M3 ports what the kernel's own memory needs before user space
//! exists: the direct map (`PMAP_DIRECT_MAP`, `pmap_map_direct`), `pmap_bootstrap`'s
//! kernel-pmap setup with the recursive mapping, `pmap_steal_memory`, `pmap_zero_page`,
//! `pmap_copy_page`, `pmap_init`, the kernel mapping functions (`pmap_kenter_pa`,
//! `pmap_kremove`, `pmap_extract`, `pmap_pdes_valid`, `pmap_find_pte_direct`),
//! `pmap_growkernel` with `pmap_alloc_level`/`pmap_get_physpage`, and the single-CPU TLB
//! shootdowns. The user pmaps (`pmap_create`, `pmap_enter`, `pmap_remove`, the pv lists, the
//! PTP management, `pmap_map_ptes`), PCID, `pmap_randomize` and the MP shootdowns come with
//! M4 to M6.
//!
//! ## Deviations
//! - `pmap_direct_base` is the bootloader's higher-half direct map (`BootInfo::hhdm_offset`),
//!   set by `init_x86_64` where the C's `init_x86_64` derives it from `L4_SLOT_DIRECT`;
//!   `pmap_bootstrap` does not build the direct map's page tables (`dmpdp`/`dmpd`): the
//!   bootloader's serve until the kernel owns its page tables. Limine maps at least 4 GiB and
//!   every memory-map region, which is all `pmap_steal_memory` and the page allocator touch.
//! - The kernel runs on the bootloader's PML4 (`CR3`), which `pmap_bootstrap` adopts as
//!   `pm_pdir` (the C's is `proc0`'s, built by `locore0.S`); it installs the recursive mapping
//!   in `PDIR_SLOT_PTE` itself, where the C's `locore0.S` does.
//! - The managed kernel range starts at `virtual_avail`, above the direct map, when that map
//!   sits inside `[VM_MIN_KERNEL_ADDRESS, VM_MAX_KERNEL_ADDRESS)` as Limine's default
//!   placement does; `pmap_growkernel` counts its PTPs from there (`pmap_kva_start`), not from
//!   `VM_MIN_KERNEL_ADDRESS`, and `pmap_alloc_level` keeps the page-table pages the bootloader
//!   already installed on the way (the direct map shares the PML4 slot). A large page met
//!   there is a panic.
//! - `pg_nx` comes from `EFER.NXE` as the bootloader left it; the C's `locore0.S` probes CPUID
//!   and enables it. `pg_g_kern`, `pg_xo` (PKU), `pg_crypt` (SEV) and `pmap_pg_wc` (PAT) keep
//!   their "not available" values until CPU identification (M4); PCID is off.
//! - `pmap_steal_memory`'s `vm_physmem[]` bookkeeping is `uvm_page_physsteal`
//!   (`uvm/uvm_page.rs`), shared with arm64 and the host double; the direct-map half is here.
//! - `pagezero` (`locore.S`) is `ptr::write_bytes`; `pmap_flush_cache` waits for `cpu_info`
//!   (`ci_cflushsz`, M5).
//! - `pmap_virtual_space` is not in the C (amd64 has `PMAP_STEAL_MEMORY`); the trait needs one
//!   and it reports the range `pmap_steal_memory` reports.
//! - `pmap_growkernel` has no user pmaps to update yet (`pmaps`, M6); the `splhigh` around
//!   it waits for `spl(9)` (M4). `pmap_get_physpage` after `uvm_init` allocates the PTP from
//!   `pm_obj`, whose objects have no pager yet.

use core::ptr;
use core::sync::atomic::{AtomicBool, AtomicU64, AtomicUsize, Ordering};

use libkern::StaticCell;

use crate::arch::amd64::include::cpufunc::{invlpg, rcr3, rdmsr, tlbflush, wbinvd_on_all_cpus};
use crate::arch::amd64::include::param::{PAGE_MASK, PAGE_SIZE};
use crate::arch::amd64::include::pmap::{
    NBPD_INITIALIZER, NKPTP_INITIALIZER, NKPTPMAX_INITIALIZER, PDES_INITIALIZER, PDIR_SLOT_PTE,
    PG_PVLIST, PMAP_NOCACHE, PMAP_NOCRYPT, PMAP_PA_MASK, PMAP_TYPE_NORMAL, PMAP_WC, PTP_LEVELS,
    Pmap, kvtopte, pl_i, pmap_valid_entry, ptp_va2o, va_sign_pos,
};
use crate::arch::amd64::include::pte::{
    L4_MASK, L4_SHIFT, NBPD_L2, PAGE_MASK_L2, PG_FRAME, PG_LGFRAME, PG_N, PG_NX, PG_PS, PG_RO,
    PG_RW, PG_UCMINUS, PG_V, PdEntry, PtEntry, x86_round_pdr,
};
use crate::arch::amd64::include::specialreg::{EFER_NXE, MSR_EFER};
use crate::arch::amd64::include::vmparam::{VM_MAX_KERNEL_ADDRESS, VM_MIN_KERNEL_ADDRESS};
use crate::sys::mman::{PROT_EXEC, PROT_WRITE};
use crate::sys::types::{Paddr, Vaddr, Vsize};
use crate::uvm::uvm_extern::{UVM_PGA_USERESERVE, UVM_PGA_ZERO, VmProt, Voff};
use crate::uvm::uvm_init::UVM;
use crate::uvm::uvm_page::{
    PG_BUSY, PHYS_TO_VM_PAGE, VmPage, uvm_page_physsteal, uvm_pagealloc, vm_page_to_phys,
};
use crate::uvm::uvm_param::atop;
use crate::{kassert, unported};

/// `normal_pdes[]`: the level 2, 3 and 4 tables of the current pmap through the recursive
/// mapping.
const NORMAL_PDES: [usize; PTP_LEVELS - 1] = PDES_INITIALIZER;

/// `nkptp[]`: how many page-table pages the kernel has at each level.
static NKPTP: [AtomicUsize; PTP_LEVELS] = [
    AtomicUsize::new(NKPTP_INITIALIZER[0]),
    AtomicUsize::new(NKPTP_INITIALIZER[1]),
    AtomicUsize::new(NKPTP_INITIALIZER[2]),
    AtomicUsize::new(NKPTP_INITIALIZER[3]),
];
/// `pmap_direct_base`: where physical address 0 is mapped (see the module's deviations).
pub static PMAP_DIRECT_BASE: AtomicUsize = AtomicUsize::new(0);
/// `pmap_direct_end`: the end of the direct map.
pub static PMAP_DIRECT_END: AtomicUsize = AtomicUsize::new(0);
/// `pg_nx`: NX PTE bit (if CPU supports).
pub static PG_NX_BIT: AtomicU64 = AtomicU64::new(0);
/// `pg_g_kern`: PG_G if global pages should be used in kernel mappings, 0 otherwise (for
/// insecure CPUs).
pub static PG_G_KERN: AtomicU64 = AtomicU64::new(0);
/// `pg_xo`: XO PTE bits, set to PKU key1 (if cpu supports PKU).
pub static PG_XO_BITS: AtomicU64 = AtomicU64::new(0);
/// `pg_crypt`: the memory encryption bit (SEV), 0 without it.
pub static PG_CRYPT: AtomicU64 = AtomicU64::new(0);
/// `pg_frame`: the page frame bits, narrowed by CPUID when encryption takes some.
pub static PG_FRAME_MASK: AtomicU64 = AtomicU64::new(PG_FRAME);
/// `pg_lgframe`: the large page frame bits.
pub static PG_LGFRAME_MASK: AtomicU64 = AtomicU64::new(PG_LGFRAME);
/// `pmap_pg_wc`: if our processor supports PAT then we set this to be the pte bits for Write
/// Combining. Else we fall back to UC- so mtrrs can override the cacheability.
pub static PMAP_PG_WC: AtomicU64 = AtomicU64::new(PG_UCMINUS);
/// `pmap_use_pcid`: nonzero if PCID use is enabled (currently we require INVPCID).
pub static PMAP_USE_PCID: AtomicBool = AtomicBool::new(false);
/// `protection_codes[]`: maps MI prot to i386 prot code.
static PROTECTION_CODES: StaticCell<[PtEntry; 8]> = StaticCell::new([0; 8]);
/// `pmap_initialized`: pmap_init done yet?
static PMAP_INITIALIZED: AtomicBool = AtomicBool::new(false);
/// `virtual_avail`: the first free kernel virtual address.
static VIRTUAL_AVAIL: AtomicUsize = AtomicUsize::new(0);
/// `pmap_maxkvaddr`: how far the kernel page tables reach.
static PMAP_MAXKVADDR: AtomicUsize = AtomicUsize::new(VM_MIN_KERNEL_ADDRESS);
/// Where the managed kernel range begins: `VM_MIN_KERNEL_ADDRESS` in the C (see the module's
/// deviations).
static PMAP_KVA_START: AtomicUsize = AtomicUsize::new(VM_MIN_KERNEL_ADDRESS);
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

/// `pmap_maxkvaddr`: how far the kernel page tables reach.
pub fn pmap_maxkvaddr() -> Vaddr {
    Vaddr::new(PMAP_MAXKVADDR.load(Ordering::Relaxed))
}

fn pg_nx() -> u64 {
    PG_NX_BIT.load(Ordering::Relaxed)
}

fn pg_crypt() -> u64 {
    PG_CRYPT.load(Ordering::Relaxed)
}

fn pg_frame() -> u64 {
    PG_FRAME_MASK.load(Ordering::Relaxed)
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

/// `pmap_pte_set(p, n)`: `atomic_swap_64`.
///
/// # Safety
///
/// `p` must point at a page-table entry that is mapped writable (through the recursive
/// mapping or the direct map).
unsafe fn pmap_pte_set(p: *mut PtEntry, n: PtEntry) -> PtEntry {
    // SAFETY: the caller's guarantee; entries are 8-byte aligned and live as long as their
    // table page.
    unsafe { AtomicU64::from_ptr(p) }.swap(n, Ordering::SeqCst)
}

/// Reads entry `index` of the page-directory page at `base`.
///
/// # Safety
///
/// The table must exist and be mapped at `base`: through the recursive mapping, every level
/// above it must be valid; through the direct map, `base` must be a table's address.
unsafe fn pde_at(base: usize, index: usize) -> PdEntry {
    // SAFETY: the caller's guarantee.
    unsafe { ptr::read_volatile((base as *const PdEntry).add(index)) }
}

/// Writes entry `index` of the page-directory page at `base`.
///
/// # Safety
///
/// As for [`pde_at`], and the entry must be one the caller owns.
unsafe fn pde_set(base: usize, index: usize, e: PdEntry) {
    // SAFETY: the caller's guarantee.
    unsafe { ptr::write_volatile((base as *mut PdEntry).add(index), e) };
}

/// `pmap_update_pg(va)`: drops the TLB entry of one page.
fn pmap_update_pg(va: usize) {
    invlpg(va as u64);
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
    PMAP_KVA_START.store(virtual_avail, Ordering::Relaxed);
    PMAP_MAXKVADDR.store(virtual_avail, Ordering::Relaxed);

    // pg_nx: whether the bootloader enabled NX (the C's locore0.S does, after CPUID).
    // SAFETY: MSR_EFER exists on every x86-64 CPU.
    if unsafe { rdmsr(MSR_EFER) } & EFER_NXE != 0 {
        PG_NX_BIT.store(PG_NX, Ordering::Relaxed);
    }
    let pg_nx = pg_nx();
    let pg_xo = PG_XO_BITS.load(Ordering::Relaxed);

    // PKU: with CPU identification (M4).

    // set up protection_codes: we need to be able to convert from a MI protection code (some
    // combo of VM_PROT...) to something we can jam into a i386 PTE.
    let codes: [PtEntry; 8] = [
        pg_nx,         // ---
        PG_RO | pg_nx, // -r-
        PG_RW | pg_nx, // w--
        PG_RW | pg_nx, // wr-
        pg_xo,         // --x
        PG_RO,         // -rx
        PG_RW,         // w-x
        PG_RW,         // wrx
    ];
    // SAFETY: once, on the boot CPU, before anything reads the codes.
    unsafe { PROTECTION_CODES.write(codes) };

    // now we init the kernel's pmap
    //
    // the kernel pmap's pm_obj is not used for much. however, in user pmaps the pm_obj
    // contains the list of active PTPs. the pm_obj currently does not have a pager.
    let kpm = pmap_kernel();
    for hint in &kpm.pm_ptphint {
        hint.set(ptr::null());
    }
    let pdirpa = Paddr::new(rcr3() as usize & PMAP_PA_MASK);
    let pdir = pmap_direct_map(pdirpa).as_usize();
    kpm.pm_pdir.set(pdir as *mut PdEntry);
    kpm.pm_pdirpa.set(pdirpa);
    let resident = atop(kva_start - VM_MIN_KERNEL_ADDRESS) as i64;
    kpm.pm_stats.wired_count.set(resident);
    kpm.pm_stats.resident_count.set(resident);
    // the above is just a rough estimate and not critical to the proper operation of the
    // system.

    kpm.pm_type.set(PMAP_TYPE_NORMAL);

    // The recursive mapping, which the C's locore0.S installs (see the module's deviations).
    // SAFETY: the PML4 is RAM the direct map covers; the slot is the caller's to use once it
    // is seen empty.
    unsafe {
        if pmap_valid_entry(pde_at(pdir, PDIR_SLOT_PTE)) {
            #[allow(clippy::panic)] // the bootloader broke the protocol's contract
            {
                panic!("pmap_bootstrap: PML4 slot {} is in use", PDIR_SLOT_PTE);
            }
        }
        pde_set(
            pdir,
            PDIR_SLOT_PTE,
            pdirpa.as_usize() as u64 | PG_V | PG_RW | pg_nx | pg_crypt(),
        );
    }
    tlbflush();

    // curpcb->pcb_pmap = kpm (proc0's pcb): M5. PCID, pmap_randomize, the direct map's own
    // page tables, the early PTE pages and the low-memory PTPs: with M4 to M6.

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

/// `pmap_pdes_valid`: whether every page-directory entry above `va`'s PTE is valid; gives
/// the last one (the level-2 entry).
pub fn pmap_pdes_valid(va: usize) -> Option<PdEntry> {
    let mut pde = 0;
    for i in (2..=PTP_LEVELS).rev() {
        let index = pl_i(va, i);
        // SAFETY: level `i`'s table is reachable through the recursive mapping once the entry
        // one level up is valid, which the previous iteration checked (the recursive slot
        // itself for level 4).
        pde = unsafe { pde_at(NORMAL_PDES[i - 2], index) };
        if !pmap_valid_entry(pde) {
            return None;
        }
    }
    Some(pde)
}

/// `pmap_find_pte_direct`: walks `pm`'s tables through the direct map down to `va`'s entry.
/// Returns the level the walk stopped at (0: the PTE was reached; 1: a 2M page or an invalid
/// level-2 entry; ...), the direct-map address of that table and the index in it.
pub fn pmap_find_pte_direct(pm: &Pmap, va: usize) -> (usize, usize, usize) {
    let mut pdpa = pm.pm_pdirpa.get().as_usize();
    let mut shift = L4_SHIFT;
    let mut mask = L4_MASK;
    let mut pd = 0;
    let mut offs = 0;
    for lev in (1..=PTP_LEVELS).rev() {
        pd = pmap_direct_map(Paddr::new(pdpa)).as_usize();
        offs = (va_sign_pos(va) & mask) >> shift;
        // SAFETY: a page-table page in RAM, reached through the direct map.
        let pde = unsafe { pde_at(pd, offs) };

        // Large pages are different, break early if we run into one.
        if pde & (PG_PS | PG_V) != PG_V {
            return (lev - 1, pd, offs);
        }

        pdpa = (pde & pg_frame()) as usize;
        // 4096/8 == 512 == 2^9 entries per level
        shift -= 9;
        mask >>= 9;
    }

    (0, pd, offs)
}

/// `pmap_kenter_pa`: enter a kernel mapping without R/M (pv_entry) tracking. No need to lock
/// anything, assume va is already allocated; should be faster than normal pmap enter
/// function.
///
/// # Safety
///
/// `va` must be kernel virtual space the caller owns, covered by `pmap_growkernel`, and `pa`
/// a page it may map there.
pub unsafe fn pmap_kenter_pa(va: Vaddr, pa: Paddr, prot: VmProt) {
    let va = va.as_usize();
    let pa = pa.as_usize();
    let pte = kvtopte(va);

    let mut npte = (pa & PMAP_PA_MASK) as u64
        | if prot & PROT_WRITE != 0 { PG_RW } else { PG_RO }
        | if pa & PMAP_NOCACHE as usize != 0 {
            PG_N
        } else {
            0
        }
        | if pa & PMAP_WC as usize != 0 {
            PMAP_PG_WC.load(Ordering::Relaxed)
        } else {
            0
        }
        | PG_V
        | if pa & PMAP_NOCRYPT as usize != 0 {
            0
        } else {
            pg_crypt()
        };

    // special 1:1 mappings in the first 2MB must not be global
    if va >= NBPD_L2 {
        npte |= PG_G_KERN.load(Ordering::Relaxed);
    }

    if prot & PROT_EXEC == 0 {
        npte |= pg_nx();
    }
    // SAFETY: the caller's guarantee makes `va`'s level-1 table exist, and the recursive
    // mapping exposes it writable.
    let opte = unsafe { pmap_pte_set(pte, npte) };
    // LARGEPAGES: not an option here.
    if pmap_valid_entry(opte) {
        if (pa & PMAP_NOCACHE as usize != 0 && opte & PG_N == 0) || pa & PMAP_NOCRYPT as usize != 0
        {
            wbinvd_on_all_cpus();
        }
        // This shouldn't happen
        pmap_tlb_shootpage(pmap_kernel(), va, true);
        pmap_tlb_shootwait();
    }
}

/// `pmap_kremove`: remove a kernel mapping(s) without R/M (pv_entry) tracking. No need to
/// lock anything; caller must dispose of any vm_page mapped in the va range; we assume the va
/// is page aligned and the len is a multiple of PAGE_SIZE; we assume kernel only unmaps valid
/// addresses and thus don't bother checking the valid bit before doing TLB flushing.
///
/// # Safety
///
/// The range must have been mapped by [`pmap_kenter_pa`] and nothing may use it afterwards.
pub unsafe fn pmap_kremove(sva: Vaddr, len: Vsize) {
    let sva = sva.as_usize();
    let eva = sva + len.as_usize();

    let mut va = sva;
    while va != eva {
        let pte = kvtopte(va);

        // SAFETY: the caller's guarantee: the range was entered, so its tables exist.
        let opte = unsafe { pmap_pte_set(pte, 0) };
        // LARGEPAGES: not an option here.
        kassert!(opte & PG_PVLIST == 0);
        va += PAGE_SIZE;
    }

    pmap_tlb_shootrange(pmap_kernel(), sva, eva, true);
    pmap_tlb_shootwait();
}

/// `pmap_extract`: extract a PA for the given VA.
pub fn pmap_extract(pmap: &Pmap, va: Vaddr) -> Option<Paddr> {
    if ptr::eq(pmap, pmap_kernel()) && pmap_direct_mapped(va) {
        return Some(pmap_direct_unmap(va));
    }

    // mtx_enter(&pmap->pm_mtx) for user pmaps: M5.

    let va = va.as_usize();
    let (level, ptes, offs) = pmap_find_pte_direct(pmap, va);
    // SAFETY: `pmap_find_pte_direct` returned a table page's direct-map address.
    let pte = unsafe { pde_at(ptes, offs) };

    if level == 0 && pmap_valid_entry(pte) {
        return Some(Paddr::new((pte & pg_frame()) as usize | (va & PAGE_MASK)));
    }
    if level == 1 && pte & (PG_PS | PG_V) == (PG_PS | PG_V) {
        let lgframe = PG_LGFRAME_MASK.load(Ordering::Relaxed);
        return Some(Paddr::new((pte & lgframe) as usize | (va & PAGE_MASK_L2)));
    }

    None
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

/// `pmap_get_physpage`: a zeroed page for a level-`level` PTP mapping `va`.
fn pmap_get_physpage(va: usize, level: usize) -> Paddr {
    let kpm = pmap_kernel();

    let pa = if !UVM.page_init_done.load(Ordering::Relaxed) {
        // we're growing the kernel pmap early (from uvm_pageboot_alloc()). this case must be
        // handled a little differently.
        // SAFETY: before page_init_done, on the boot CPU.
        let va = unsafe { pmap_steal_memory(Vsize::new(PAGE_SIZE), None, None) };
        pmap_direct_unmap(va)
    } else {
        let Some(ptp) = uvm_pagealloc(
            Some(&kpm.pm_obj[level - 1]),
            ptp_va2o(va, level) as Voff,
            None,
            UVM_PGA_USERESERVE | UVM_PGA_ZERO,
        ) else {
            #[allow(clippy::panic)] // the C panics here too
            {
                panic!("pmap_get_physpage: out of memory");
            }
        };
        ptp.clear_bits(PG_BUSY);
        ptp.wire_count.set(1);
        vm_page_to_phys(ptp)
    };
    kpm.pm_stats
        .resident_count
        .set(kpm.pm_stats.resident_count.get() + 1);
    pa
}

/// `pmap_alloc_level`: allocate the amount of specified ptps for a ptp level, and populate
/// all levels below accordingly, mapping virtual addresses starting at kva. Used by
/// `pmap_growkernel`.
fn pmap_alloc_level(kva: usize, lvl: usize, needed_ptps: &[isize; PTP_LEVELS]) {
    for level in (2..=lvl).rev() {
        let pdep = if level == PTP_LEVELS {
            pmap_kernel().pm_pdir.get() as usize
        } else {
            NORMAL_PDES[level - 2]
        };
        let mut va = kva;
        let mut index = pl_i(kva, level);
        let mut endindex = index as isize + needed_ptps[level - 1];
        // XXX special case for first time call.
        if NKPTP[level - 1].load(Ordering::Relaxed) != 0 {
            index += 1;
        } else {
            endindex -= 1;
        }

        let mut i = index;
        while (i as isize) <= endindex {
            // SAFETY: level `level`'s table for `va` exists: the PML4 (direct map) at level 4,
            // otherwise the entry one level up was made or kept by the previous iteration and
            // the recursive mapping exposes the table.
            let pde = unsafe { pde_at(pdep, i) };
            if pmap_valid_entry(pde) {
                // The bootloader's page-table pages are kept (see the module's deviations).
                if pde & PG_PS != 0 {
                    #[allow(clippy::panic)] // nothing below a large page can be managed
                    {
                        panic!(
                            "pmap_alloc_level: large page at level {} index {}",
                            level, i
                        );
                    }
                }
            } else {
                let pa = pmap_get_physpage(va, level - 1);
                // SAFETY: as above; the entry is empty, so it is ours.
                unsafe {
                    pde_set(
                        pdep,
                        i,
                        pa.as_usize() as u64 | PG_RW | PG_V | pg_nx() | pg_crypt(),
                    )
                };
            }
            NKPTP[level - 1].fetch_add(1, Ordering::Relaxed);
            va += NBPD_INITIALIZER[level - 1];
            i += 1;
        }
    }
}

/// `pmap_growkernel`: increase usage of KVM space. We allocate new PTPs for the kernel and
/// install them in all the pmaps on the system.
pub fn pmap_growkernel(maxkvaddr: Vaddr) -> Vaddr {
    let cur = PMAP_MAXKVADDR.load(Ordering::Relaxed);
    if maxkvaddr.as_usize() <= cur {
        return Vaddr::new(cur);
    }

    let maxkvaddr = x86_round_pdr(maxkvaddr.as_usize());
    let kva_start = PMAP_KVA_START.load(Ordering::Relaxed);
    let old = NKPTP[PTP_LEVELS - 1].load(Ordering::Relaxed);
    // This loop could be optimized more, but pmap_growkernel() is called infrequently.
    let mut needed_kptp = [0isize; PTP_LEVELS];
    for i in (1..PTP_LEVELS).rev() {
        let target_nptp = pl_i(maxkvaddr, i + 1) - pl_i(kva_start, i + 1);
        // XXX only need to check toplevel.
        if target_nptp > NKPTPMAX_INITIALIZER[i] {
            #[allow(clippy::panic)] // the C panics here too
            {
                panic!("pmap_growkernel: out of KVA space");
            }
        }
        needed_kptp[i] = target_nptp as isize - NKPTP[i].load(Ordering::Relaxed) as isize + 1;
    }

    // splhigh() (to be safe): M4.
    pmap_alloc_level(cur, PTP_LEVELS, &needed_kptp);

    // If the number of top level entries changed, update all pmaps.
    if needed_kptp[PTP_LEVELS - 1] != 0 {
        let _newpdes = NKPTP[PTP_LEVELS - 1].load(Ordering::Relaxed) - old;
        // LIST_FOREACH(pm, &pmaps, pm_list) memcpy(PDIR_SLOT_KERN + old, ...): no user pmaps
        // until M6.
    }
    PMAP_MAXKVADDR.store(maxkvaddr, Ordering::Relaxed);
    // splx(s): M4.

    Vaddr::new(maxkvaddr)
}

/// `pmap_tlb_shootpage`: drops one page's TLB entry; on one CPU, locally when `shootself`.
pub fn pmap_tlb_shootpage(_pm: &Pmap, va: usize, shootself: bool) {
    // MULTIPROCESSOR: the IPIs to the other CPUs are not configured.
    if !PMAP_USE_PCID.load(Ordering::Relaxed) {
        if shootself {
            pmap_update_pg(va);
        }
    } else {
        let _ = unported!("invpcid (pmap_tlb_shootpage with PCID)");
    }
}

/// `pmap_tlb_shootrange`: drops a range's TLB entries.
pub fn pmap_tlb_shootrange(_pm: &Pmap, sva: usize, eva: usize, shootself: bool) {
    // MULTIPROCESSOR: not configured.
    if !PMAP_USE_PCID.load(Ordering::Relaxed) {
        if shootself {
            let mut va = sva;
            while va < eva {
                pmap_update_pg(va);
                va += PAGE_SIZE;
            }
        }
    } else {
        let _ = unported!("invpcid (pmap_tlb_shootrange with PCID)");
    }
}

/// `pmap_tlb_shoottlb`: drops the whole TLB.
pub fn pmap_tlb_shoottlb(_pm: &Pmap, shootself: bool) {
    // MULTIPROCESSOR: not configured.
    if shootself {
        if !PMAP_USE_PCID.load(Ordering::Relaxed) {
            tlbflush();
        } else {
            let _ = unported!("invpcid (pmap_tlb_shoottlb with PCID)");
        }
    }
}

/// `pmap_tlb_shootwait`: nothing without `MULTIPROCESSOR`.
pub fn pmap_tlb_shootwait() {}
