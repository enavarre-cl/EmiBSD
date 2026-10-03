/*	$OpenBSD: pmap.c,v 1.191 2026/06/04 05:22:04 mlarkin Exp $	*/
/*	$NetBSD: pmap.c,v 1.3 2003/05/08 18:13:13 thorpej Exp $	*/
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
/* </LICENSES> */

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
//!   there is a panic. The window keeps the C's size: `pmap_virtual_space` ends at
//!   `pmap_kva_start + (VM_MAX_KERNEL_ADDRESS - VM_MIN_KERNEL_ADDRESS)` and `pmap_bootstrap`
//!   moves `uvm`'s runtime `vm_min_kernel_address` (the kernel map's base) up to
//!   `virtual_avail`, so the kernel map is `[virtual_avail, virtual_avail + 512 GiB)`.
//! - `pg_nx` comes from `EFER.NXE` as the bootloader left it; the C's `locore0.S` probes CPUID
//!   and enables it. `pg_g_kern`, `pg_xo` (PKU), `pg_crypt` (SEV) and `pmap_pg_wc` (PAT) keep
//!   their "not available" values until CPU identification (M4); PCID is off.
//! - `pmap_steal_memory`'s `vm_physmem[]` bookkeeping is `uvm_page_physsteal`
//!   (`uvm/uvm_page.rs`), shared with arm64 and the host double; the direct-map half is here.
//! - `pagezero` (`locore.S`) is `ptr::write_bytes`; `pmap_flush_cache` waits for `cpu_info`
//!   (`ci_cflushsz`, M5).
//! - `pmap_virtual_space` is not in the C (amd64 has `PMAP_STEAL_MEMORY`); the trait needs one
//!   and it reports the range `pmap_steal_memory` reports.
//! - User pmaps (M6): `pmap_create`/`pmap_destroy`/`pmap_enter`/`pmap_remove` walk the
//!   tables through the direct map instead of `pmap_map_ptes` (the recursive mapping of a
//!   borrowed `%cr3`), so `pmap_pdes_valid`/`normal_pdes` are only used on the current pmap.
//!   `pmap_pdp_ctor` copies the kernel's whole upper half of the PML4 (the C copies the kernel
//!   VM, direct-map and `KERNBASE` slots one by one). The pmap list (`pmaps`) waits
//!   for `pmap_growkernel` to need it. The PDP comes from `uvm_pagealloc` rather than `pmap_pdp_pool`; the pmap pool is
//!   initialised in `pmap_init`. `cpu_meltdown` is not configured: no `pm_pdir_intel`.
//! - The pv lists (M7a): `pmap_page_remove`, `pmap_test_attrs`, `pmap_clear_attrs` and
//!   `pmap_write_protect` reach each PTE through `pmap_find_pte_direct` instead of
//!   `PTE_BASE` under `pmap_map_ptes`. `pmap_remove_pte` (the single-page shortcut of
//!   `pmap_do_remove`) is folded into the block loop over `pmap_remove_ptes`.
//! - `pmap_growkernel` has no user pmaps to update yet (`pmaps`, M6); the `splhigh` around
//!   it waits for `spl(9)` (M4). `pmap_get_physpage` after `uvm_init` allocates the PTP from
//!   `pm_obj`, whose objects have no pager yet.

use core::ptr;
use core::sync::atomic::{AtomicBool, AtomicU64, AtomicUsize, Ordering};

use libkern::StaticCell;

use crate::arch::amd64::include::cpu::curcpu;
use crate::arch::amd64::include::cpufunc::{
    invlpg, lcr3, rcr3, rdmsr, tlbflush, wbinvd_on_all_cpus,
};
use crate::arch::amd64::include::param::{PAGE_MASK, PAGE_SIZE};
use crate::arch::amd64::include::pmap::{
    NBPD_INITIALIZER, NKPTP_INITIALIZER, NKPTPMAX_INITIALIZER, NTOPLEVEL_PDES, PDES_INITIALIZER,
    PDIR_SLOT_PTE, PG_PMAP_MOD, PG_PMAP_REF, PG_PVLIST, PG_W, PMAP_NOCACHE, PMAP_NOCRYPT,
    PMAP_PA_MASK, PMAP_TYPE_NORMAL, PMAP_WC, PTE_BASE, PTP_LEVELS, Pmap, PvEntry, kvtopte, pl_i,
    pmap_valid_entry, ptp_va2o, va_sign_pos,
};
use crate::arch::amd64::include::pte::{
    L4_MASK, L4_SHIFT, NBPD_L2, PAGE_MASK_L2, PG_FRAME, PG_LGFRAME, PG_M, PG_N, PG_NX, PG_PS,
    PG_RO, PG_RW, PG_U, PG_UCMINUS, PG_V, PG_u, PdEntry, PtEntry, x86_round_pdr,
};
use crate::arch::amd64::include::specialreg::{EFER_NXE, MSR_EFER};
use crate::arch::amd64::include::vmparam::{VM_MAX_ADDRESS, VM_MAXUSER_ADDRESS};
use crate::arch::amd64::include::vmparam::{VM_MAX_KERNEL_ADDRESS, VM_MIN_KERNEL_ADDRESS};
use crate::kern::kern_lock::{mtx_enter, mtx_leave};
use crate::kern::subr_pool::{pool_get, pool_init, pool_put};
use crate::machine::intr::IPL_VM;
use crate::sys::errno::Errno;
use crate::sys::mman::{PROT_EXEC, PROT_READ, PROT_WRITE};
use crate::sys::pool::{PR_NOWAIT, PR_WAITOK, Pool};
use crate::sys::proc::{P_SYSTEM, Proc, Process};
use crate::sys::types::{Paddr, Vaddr, Vsize};
use crate::uvm::uvm_extern::Vmspace;
use crate::uvm::uvm_extern::{UVM_PGA_USERESERVE, UVM_PGA_ZERO, VmProt, Voff};
use crate::uvm::uvm_init::UVM;
use crate::uvm::uvm_page::{
    PG_BUSY, PHYS_TO_VM_PAGE, VmPage, uvm_page_physsteal, uvm_pagealloc, vm_page_to_phys,
};
use crate::uvm::uvm_page::{PG_FAKE, Pglist, uvm_pagefree, uvm_pagelookup, uvm_pagerealloc};
use crate::uvm::uvm_param::atop;
use crate::uvm::uvm_pmap::{PMAP_CANFAIL, PMAP_WIRED};
use crate::{kassert, kprintf, unported};
use core::cell::Cell;
use core::ptr::NonNull;

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
/// `pmap_pmap_pool`: the pool of `struct pmap`.
static PMAP_PMAP_POOL: Pool = Pool::new();
/// `pmap_pv_pool`: pool for pv entries.
static PMAP_PV_POOL: Pool = Pool::new();

/// `PMAP_REMOVE_ALL`: `pmap_do_remove` removes every mapping.
const PMAP_REMOVE_ALL: i32 = 0;
/// `PMAP_REMOVE_SKIPWIRED`: `pmap_do_remove` leaves wired mappings alone.
const PMAP_REMOVE_SKIPWIRED: i32 = 1;

/// `pmap_kernel()`.
pub fn pmap_kernel() -> &'static Pmap {
    &KERNEL_PMAP_STORE
}

/// `pmap_activate`: activate the address space of `p`: its pcb gets the pmap and `%cr3`
/// value `cpu_switchto` loads; if `p` is the running thread the switch happens now.
///
/// Kernel threads run on the kernel pmap (there is no `vmspace` before user mode, M6).
pub fn pmap_activate(p: &Proc) {
    let pcb = p.pcb();
    let pmap = p.vmspace().vm_map.pmap();

    pcb.pcb_pmap.set(pmap);
    // PCID is not enabled (cr3_pcid_proc, PCID_KERN and cr3_reuse_pcid are 0).
    pcb.pcb_cr3.set(pmap.pm_pdirpa.get().as_usize() as u64);

    if !ptr::eq(p, curcpu().ci_curproc.get()) {
        return;
    }

    if p.p_flag.load(Ordering::Relaxed) & P_SYSTEM == 0 {
        // mark the pmap in use by this processor
        curcpu().ci_proc_pmap.set(pmap);
        // in case we return to userspace without context switching: cpu_meltdown's
        // ci_kern_cr3/ci_user_cr3 (not configured).
    }

    // SAFETY: `pcb_cr3` is a page directory `pmap_pdp_ctor` or `pmap_bootstrap` built, with
    // the kernel half this code runs in.
    unsafe { lcr3(pcb.pcb_cr3.get()) };
}

/// `pmap_deactivate`: deactivate a process' pmap.
pub fn pmap_deactivate(p: &Proc) {
    if p.p_flag.load(Ordering::Relaxed) & P_SYSTEM == 0 {
        let this = curcpu();

        // mark the pmap no longer in use by this processor.
        kassert!(ptr::eq(this.ci_proc_pmap.get(), p.vmspace().vm_map.pmap()));
        this.ci_proc_pmap.set(ptr::null());
    }
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

/// `pmap_pte_setbits(p, set)`: `atomic_setbits_u64`.
///
/// # Safety
///
/// As for [`pmap_pte_set`].
unsafe fn pmap_pte_setbits(p: *mut PtEntry, set: PtEntry) {
    // SAFETY: the caller's guarantee.
    unsafe { AtomicU64::from_ptr(p) }.fetch_or(set, Ordering::SeqCst);
}

/// `pmap_pte_clearbits(p, clr)`: `atomic_clearbits_u64`.
///
/// # Safety
///
/// As for [`pmap_pte_set`].
unsafe fn pmap_pte_clearbits(p: *mut PtEntry, clr: PtEntry) {
    // SAFETY: the caller's guarantee.
    unsafe { AtomicU64::from_ptr(p) }.fetch_and(!clr, Ordering::SeqCst);
}

/// `pmap_pte2flags`: the `PG_PMAP_*` page flags matching a PTE's R/M bits.
fn pmap_pte2flags(pte: u64) -> u32 {
    (if pte & PG_U != 0 { PG_PMAP_REF } else { 0 })
        | (if pte & PG_M != 0 { PG_PMAP_MOD } else { 0 })
}

/// `pmap_sync_flags_pte`: copies a PTE's R/M bits into the page's flags.
fn pmap_sync_flags_pte(pg: &VmPage, pte: u64) {
    if pte & (PG_U | PG_M) != 0 {
        pg.set_bits(pmap_pte2flags(pte));
    }
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
    // The kernel map's base follows (`vm_min_kernel_address`, as sparc64's runtime
    // VM_MIN_KERNEL_ADDRESS): the window is [virtual_avail, virtual_avail + the C's size).
    crate::uvm::uvm_init::VM_MIN_KERNEL_ADDRESS.store(virtual_avail, Ordering::Relaxed);

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

/// `pmap_init`: no further initialization required on this platform (the C); here the
/// pmap pool, which the C's `pmap_bootstrap` initialises (see the module's deviations).
pub fn pmap_init() {
    pool_init(
        &PMAP_PMAP_POOL,
        size_of::<Pmap>(),
        0,
        IPL_VM,
        PR_WAITOK,
        "pmappl",
        None,
    );
    pool_init(
        &PMAP_PV_POOL,
        size_of::<PvEntry>(),
        0,
        IPL_VM,
        0,
        "pvpl",
        None,
    );
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

/// `pmap_unwire`: clear the wired bit in the PTE.
pub fn pmap_unwire(pmap: &Pmap, va: Vaddr) {
    let va = va.as_usize();
    let (level, ptes, offs) = pmap_find_pte_direct(pmap, va);

    if level == 0 {
        // SAFETY: `pmap_find_pte_direct` returned a table page's direct-map address.
        let pte = unsafe { pde_at(ptes, offs) };
        if pmap_valid_entry(pte) {
            if pte & PG_W != 0 {
                let p = (ptes + offs * size_of::<PtEntry>()) as *mut PtEntry;
                // SAFETY: the PTE slot `pmap_find_pte_direct` found, in the direct map.
                unsafe { pmap_pte_set(p, pte & !PG_W) };
                pmap.pm_stats
                    .wired_count
                    .set(pmap.pm_stats.wired_count.get() - 1);
            } else {
                kprintf!(
                    "pmap_unwire: wiring for pmap {:p} va {:#x} didn't change!\n",
                    ptr::from_ref(pmap),
                    va
                );
            }
        } else {
            kprintf!("pmap_unwire: invalid (unmapped) va {:#x}\n", va);
        }
    } else {
        #[allow(clippy::panic)] // the C panics here too
        {
            panic!("pmap_unwire: invalid PDE");
        }
    }
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
        *end = Vaddr::new(kernel_virtual_end());
    }

    va
}

/// `pmap_virtual_space`: the free kernel virtual range (see the module's deviations).
pub fn pmap_virtual_space(start: &mut Vaddr, end: &mut Vaddr) {
    *start = Vaddr::new(VIRTUAL_AVAIL.load(Ordering::Relaxed));
    *end = Vaddr::new(kernel_virtual_end());
}

/// The end of the managed kernel window: it keeps the C's size but starts at
/// `pmap_kva_start` (see the module's deviations), so it is `VM_MAX_KERNEL_ADDRESS` when the
/// direct map does not overlap.
fn kernel_virtual_end() -> usize {
    PMAP_KVA_START.load(Ordering::Relaxed) + (VM_MAX_KERNEL_ADDRESS - VM_MIN_KERNEL_ADDRESS)
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

// main pv_entry manipulation functions:
//   pmap_enter_pv: enter a mapping onto a pv list
//   pmap_remove_pv: remove a mapping from a pv list

/// `pmap_enter_pv`: enter a mapping onto a pv list.
///
/// The caller should adjust ptp's wire_count before calling. `pve` is a preallocated pve for
/// us to use; `ptp` the PTP in pmap that maps this VA (`None` for the kernel pmap).
fn pmap_enter_pv(pg: &VmPage, pve: NonNull<PvEntry>, pmap: &Pmap, va: usize, ptp: Option<&VmPage>) {
    // SAFETY: `pve` is a pool item the caller owns and has initialised; nothing else sees it
    // until it is on the list, which `pv_mtx` protects.
    let pve = unsafe { pve.as_ref() };
    pve.pv_pmap.set(pmap);
    pve.pv_va.set(Vaddr::new(va));
    pve.pv_ptp.set(ptp.map_or(ptr::null(), ptr::from_ref));
    mtx_enter(&pg.mdpage.pv_mtx);
    pve.pv_next.set(pg.mdpage.pv_list.get()); // add to ...
    pg.mdpage.pv_list.set(pve); // ... list
    mtx_leave(&pg.mdpage.pv_mtx);
}

/// `pmap_remove_pv`: try to remove a mapping from a pv_list.
///
/// The caller should adjust ptp's wire_count and free PTP if needed. We return the removed
/// pve.
fn pmap_remove_pv(pg: &VmPage, pmap: &Pmap, va: usize) -> Option<NonNull<PvEntry>> {
    mtx_enter(&pg.mdpage.pv_mtx);
    let mut prevptr: &Cell<*const PvEntry> = &pg.mdpage.pv_list;
    let mut found = None;
    while let Some(pve) = NonNull::new(prevptr.get().cast_mut()) {
        // SAFETY: entries on a pv list are live pool items, protected by `pv_mtx`.
        let e = unsafe { pve.as_ref() };
        if ptr::eq(e.pv_pmap.get(), pmap) && e.pv_va.get().as_usize() == va {
            // match?
            prevptr.set(e.pv_next.get()); // remove it!
            found = Some(pve);
            break;
        }
        prevptr = &e.pv_next; // previous pointer
    }
    mtx_leave(&pg.mdpage.pv_mtx);
    found // return removed pve
}

/// Puts a list of pv entries linked through `pv_next` back into `pmap_pv_pool`.
fn pmap_free_pvs(mut free_pvs: *const PvEntry) {
    while let Some(pve) = NonNull::new(free_pvs.cast_mut()) {
        // SAFETY: the entries were unlinked from their pv list by the caller, which owns them.
        free_pvs = unsafe { pve.as_ref() }.pv_next.get();
        pool_put(&PMAP_PV_POOL, pve.cast::<u8>());
    }
}

/// `pl<lvl>_pi(va)`: the index of `va`'s entry within one level-`lvl` table (`pl_i` gives the
/// index into the recursive mapping's linear view of the whole level).
const fn pl_pi(va: usize, lvl: usize) -> usize {
    pl_i(va, lvl) & (NTOPLEVEL_PDES - 1)
}

/// `curpcb->pcb_pmap`: the pmap of the running thread (the kernel's before there is one).
fn curpcb_pmap() -> &'static Pmap {
    // SAFETY: `ci_curproc` names the thread running on this CPU, alive by definition; its
    // `pcb_pmap` is the pmap `pmap_activate` gave it, alive while the thread runs on it.
    unsafe { curcpu().ci_curproc.get().as_ref() }
        // SAFETY: as above.
        .and_then(|p| unsafe { p.pcb().pcb_pmap.get().as_ref() })
        .unwrap_or(pmap_kernel())
}

/// `pmap_is_curpmap`: is this pmap the one currently loaded \[in `%cr3`\]? Of course the
/// kernel is always loaded: its half of the address space is in every page directory.
fn pmap_is_curpmap(pm: &Pmap) -> bool {
    ptr::eq(pm, pmap_kernel()) || pm.pm_pdirpa.get().as_usize() as u64 == rcr3() & pg_frame()
}

/// The direct-map address of the level-`level` table of `pm` that holds `va`'s entry (the
/// PML4 for level 4), or `None` where a higher level is not valid.
fn pmap_table_direct(pm: &Pmap, va: usize, level: usize) -> Option<usize> {
    let mut table = pmap_direct_map(pm.pm_pdirpa.get()).as_usize();
    for lvl in ((level + 1)..=PTP_LEVELS).rev() {
        // SAFETY: a page-table page in RAM, reached through the direct map.
        let pde = unsafe { pde_at(table, pl_pi(va, lvl)) };
        if pde & (PG_PS | PG_V) != PG_V {
            return None;
        }
        table = pmap_direct_map(Paddr::new((pde & pg_frame()) as usize)).as_usize();
    }
    Some(table)
}

/// `pmap_find_ptp`: the PTP of level `level` that maps `va`, by `pa` when the hint has it,
/// else by looking it up in the pmap's object for that level.
fn pmap_find_ptp(
    pmap: &Pmap,
    va: usize,
    pa: Option<Paddr>,
    level: usize,
) -> Option<&'static VmPage> {
    let lidx = level - 1;

    if let Some(pa) = pa {
        // SAFETY: a non-null hint is a PTP of this pmap, alive until `pmap_freepage` drops
        // it (which resets the hint).
        if let Some(hint) = unsafe { pmap.pm_ptphint[lidx].get().as_ref() }
            && pa == vm_page_to_phys(hint)
        {
            return Some(hint);
        }
    }

    uvm_pagelookup(&pmap.pm_obj[lidx], ptp_va2o(va, level) as Voff)
        // SAFETY: the object's pages are the pmap's PTPs, alive while the pmap is.
        .map(|pg| unsafe { &*ptr::from_ref(pg) })
}

/// `pmap_freepage`: drops the PTP `ptp` of level `level` from its object and queues it for
/// freeing once the TLB is flushed.
fn pmap_freepage(pmap: &Pmap, ptp: &VmPage, level: usize, pagelist: &Pglist) {
    let lidx = level - 1;

    let obj = &pmap.pm_obj[lidx];
    pmap.pm_stats
        .resident_count
        .set(pmap.pm_stats.resident_count.get() - 1);
    if ptr::eq(pmap.pm_ptphint[lidx].get(), ptp) {
        pmap.pm_ptphint[lidx].set(obj.memt.root().map_or(ptr::null(), ptr::from_ref));
    }
    ptp.wire_count.set(0);
    uvm_pagerealloc(ptp, None, 0);
    // SAFETY: the page just left its object and is on no queue; the list is the caller's.
    unsafe { pagelist.insert_tail(ptp) };
}

/// `pmap_free_ptp`: frees the level-1 PTP `ptp` (which no longer maps anything) and, level
/// by level, the tables above it that it was the last user of.
fn pmap_free_ptp(pmap: &Pmap, ptp: &VmPage, va: usize, pagelist: &Pglist) {
    let mut ptp = ptp;
    let mut level = 1;
    loop {
        pmap_freepage(pmap, ptp, level, pagelist);
        let index = pl_pi(va, level + 1);
        let Some(table) = pmap_table_direct(pmap, va, level + 1) else {
            #[allow(clippy::panic)] // a PTP whose parent vanished: the tables are corrupt
            {
                panic!(
                    "pmap_free_ptp: level {} table of {:#x} is gone",
                    level + 1,
                    va
                );
            }
        };
        // SAFETY: the parent table through the direct map; the entry is this PTP's.
        unsafe { pde_set(table, index, 0) };
        // pm_pdir_intel (the Meltdown PML4e): not configured.
        // The page of the recursive mapping that showed the freed table: `invlpg` on it also
        // drops the paging-structure caches that still point at the table.
        let invaladdr = if level == 1 {
            PTE_BASE
        } else {
            NORMAL_PDES[level - 2]
        };
        pmap_tlb_shootpage(
            pmap,
            invaladdr + pl_i(va, level + 1) * PAGE_SIZE,
            pmap_is_curpmap(curpcb_pmap()),
        );
        if level < PTP_LEVELS - 1 {
            let Some(parent) = pmap_find_ptp(pmap, va, None, level + 1) else {
                #[allow(clippy::panic)] // as above
                {
                    panic!(
                        "pmap_free_ptp: missing level {} PTP of {:#x}",
                        level + 1,
                        va
                    );
                }
            };
            parent.wire_count.set(parent.wire_count.get() - 1);
            if parent.wire_count.get() > 1 {
                break;
            }
            ptp = parent;
        }
        level += 1;
        if level >= PTP_LEVELS {
            break;
        }
    }
}

/// `pmap_get_ptp`: get a PTP (if there isn't one, allocate a new one).
///
/// pmap should NOT be `pmap_kernel()`.
fn pmap_get_ptp(pmap: &Pmap, va: usize) -> Option<&'static VmPage> {
    let mut ptp: Option<&'static VmPage> = None;
    let mut pa: Option<Paddr> = None;
    let mut table = pmap_direct_map(pmap.pm_pdirpa.get()).as_usize();

    // Loop through all page table levels seeing if we need to add a new page to that level.
    for i in (2..=PTP_LEVELS).rev() {
        // Save values from previous round.
        let pptp = ptp;
        let ppa = pa;

        let index = pl_pi(va, i);

        // SAFETY: a page-table page in RAM, reached through the direct map.
        let pde = unsafe { pde_at(table, index) };
        if pmap_valid_entry(pde) {
            pa = Some(Paddr::new((pde & pg_frame()) as usize));
            ptp = None;
            table = pmap_direct_map(Paddr::new((pde & pg_frame()) as usize)).as_usize();
            continue;
        }

        let obj = &pmap.pm_obj[i - 2];
        let new = uvm_pagealloc(
            Some(obj),
            ptp_va2o(va, i - 1) as Voff,
            None,
            UVM_PGA_USERESERVE | UVM_PGA_ZERO,
        )?;

        new.clear_bits(PG_BUSY | PG_FAKE);
        new.wire_count.set(1);
        pmap.pm_ptphint[i - 2].set(new);
        let newpa = vm_page_to_phys(new);
        pa = Some(newpa);
        // SAFETY: as above; the entry is empty, so it is ours.
        unsafe {
            pde_set(
                table,
                index,
                newpa.as_usize() as u64 | PG_u | PG_RW | PG_V | pg_crypt(),
            )
        };
        // Meltdown special case (pm_pdir_intel): not configured.

        pmap.pm_stats
            .resident_count
            .set(pmap.pm_stats.resident_count.get() + 1);
        // If we're not in the top level, increase the wire count of the parent page.
        if i < PTP_LEVELS {
            let parent = match pptp {
                Some(p) => Some(p),
                None => pmap_find_ptp(pmap, va, ppa, i),
            };
            let Some(parent) = parent else {
                #[allow(clippy::panic)] // the C panics here too (DIAGNOSTIC)
                {
                    panic!("pmap_get_ptp: pde page disappeared");
                }
            };
            parent.wire_count.set(parent.wire_count.get() + 1);
        }
        ptp = Some(new);
        table = pmap_direct_map(newpa).as_usize();
    }

    // ptp is not NULL if we just allocated a new ptp. If it's still NULL, we must look up
    // the existing one.
    let ptp = match ptp {
        Some(p) => p,
        None => {
            let Some(p) = pmap_find_ptp(pmap, va, pa, 1) else {
                #[allow(clippy::panic)] // the C panics here too (DIAGNOSTIC)
                {
                    panic!("pmap_get_ptp: unmanaged user PTP for va {:#x}", va);
                }
            };
            p
        }
    };

    pmap.pm_ptphint[0].set(ptp);
    Some(ptp)
}

/// `pmap_pdp_ctor`: constructor for the PDP cache: the page directory at `pdir` (physical
/// `pdirpa`) gets the recursive entry and the kernel half.
fn pmap_pdp_ctor(pdir: usize, pdirpa: Paddr) {
    let kpm = pmap_kernel();
    let kpdir = kpm.pm_pdir.get() as usize;

    // zero init area
    // SAFETY: a fresh page through the direct map, ours.
    unsafe { ptr::write_bytes(pdir as *mut PdEntry, 0, PDIR_SLOT_PTE) };

    // put in recursive PDE to map the PTEs
    // SAFETY: as above.
    unsafe {
        pde_set(
            pdir,
            PDIR_SLOT_PTE,
            pdirpa.as_usize() as u64 | PG_V | PG_RW | pg_nx() | pg_crypt(), // PG_KW
        )
    };

    // put in kernel VM PDEs, the direct map and KERNBASE: the whole upper half of the
    // kernel's PML4 (see the module's deviations); the rest of the lower half stays zero.
    for i in (PDIR_SLOT_PTE + 1)..NTOPLEVEL_PDES {
        // SAFETY: the kernel's PML4 is readable at `pm_pdir`; the new one is ours.
        unsafe { pde_set(pdir, i, pde_at(kpdir, i)) };
    }
}

/// `pmap_create`: create a pmap.
///
/// Note: old pmap interface took a "size" args which allowed for the creation of "software
/// only" pmaps (not in bsd).
pub fn pmap_create() -> &'static Pmap {
    let Some(mem) = pool_get(&PMAP_PMAP_POOL, PR_WAITOK) else {
        crate::kern::subr_prf::panic(format_args!("pmap_create: pmap_pmap_pool is empty"));
    };
    let pp = mem.cast::<Pmap>();
    // SAFETY: a fresh, suitably aligned pool item of `size_of::<Pmap>()` bytes, written once
    // before anything else sees it; it lives until `pmap_destroy` returns it.
    let pmap: &'static Pmap = unsafe {
        pp.as_ptr().write(Pmap::new());
        pp.as_ref()
    };

    // mtx_init(&pmap->pm_mtx, IPL_VM): the pmap lock (M5 note). The uvm_objects are
    // initialised with one reference by `Pmap::new` (uvm_obj_init(&pm_obj[i], &pmap_pager,
    // 1)); the hints are null.
    pmap.pm_stats.wired_count.set(0);
    pmap.pm_stats.resident_count.set(1); // count the PDP allocd below
    pmap.pm_type.set(PMAP_TYPE_NORMAL);
    pmap.eptp.set(0);

    // allocate PDP: a zeroed page through the direct map (pmap_pdp_pool in C).
    let Some(pdp) = uvm_pagealloc(None, 0, None, UVM_PGA_USERESERVE | UVM_PGA_ZERO) else {
        crate::kern::subr_prf::panic(format_args!("pmap_create: no page for the PDP"));
    };
    pdp.clear_bits(PG_BUSY | PG_FAKE);
    pdp.wire_count.set(1);
    let pdirpa = vm_page_to_phys(pdp);
    let pdir = pmap_direct_map(pdirpa).as_usize();
    pmap_pdp_ctor(pdir, pdirpa);

    pmap.pm_pdir.set(pdir as *mut PdEntry);
    pmap.pm_pdirpa.set(pdirpa);

    // Intel CPUs need a special page table to be used during usermode execution, one that
    // lacks all kernel mappings: cpu_meltdown is not configured.
    pmap.pm_pdir_intel.set(ptr::null_mut());
    pmap.pm_pdirpa_intel.set(Paddr::new(0));

    // LIST_INSERT_HEAD(&pmaps, pmap, pm_list): see the module's deviations.
    pmap
}

/// `pmap_destroy`: drop reference count on pmap. free pmap if reference count goes to zero.
pub fn pmap_destroy(pmap: &'static Pmap) {
    // drop reference count
    let refs = pmap.pm_obj[0].uo_refs.get() - 1;
    pmap.pm_obj[0].uo_refs.set(refs);
    if refs > 0 {
        return;
    }

    // remove it from global list of pmaps: see the module's deviations.

    // free any remaining PTPs
    for obj in &pmap.pm_obj {
        while let Some(pg) = obj.memt.root() {
            kassert!(pg.flags() & PG_BUSY == 0);

            pg.wire_count.set(0);
            pmap.pm_stats
                .resident_count
                .set(pmap.pm_stats.resident_count.get() - 1);

            uvm_pagefree(pg);
        }
    }

    // pool_put(&pmap_pdp_pool, pmap->pm_pdir): the PDP page.
    if let Some(pdp) = pmap_unmap_direct(Vaddr::new(pmap.pm_pdir.get() as usize)) {
        pdp.wire_count.set(0);
        uvm_pagefree(pdp);
    }
    pmap.pm_pdir.set(ptr::null_mut());

    // pm_pdir_intel: not configured.

    pool_put(&PMAP_PMAP_POOL, NonNull::from(pmap).cast::<u8>());
}

/// Add a reference to the specified pmap.
pub fn pmap_reference(pmap: &Pmap) {
    pmap.pm_obj[0].uo_refs.set(pmap.pm_obj[0].uo_refs.get() + 1);
}

/// `pmap_remove_ptes`: remove a range of PTEs from a PTP.
///
/// Note that `ptpva` points to the PTE that maps `startva`. This may or may not be the first
/// PTE in the PTP. We loop through the PTP while there are still PTEs to look at and the
/// wire_count is greater than 1 (because we use the wire_count to keep track of the number of
/// real PTEs in the PTP).
fn pmap_remove_ptes(
    pmap: &Pmap,
    ptp: Option<&VmPage>,
    ptpva: usize,
    startva: usize,
    endva: usize,
    flags: i32,
    free_pvs: &mut *const PvEntry,
) {
    let mut pte = ptpva as *mut PtEntry;
    let mut va = startva;
    while va < endva && ptp.is_none_or(|ptp| ptp.wire_count.get() > 1) {
        // SAFETY: `pte` walks the level-1 table through the direct map, within the block
        // `pmap_do_remove` bounded.
        let cur = unsafe { ptr::read_volatile(pte) };
        if !pmap_valid_entry(cur) || (flags & PMAP_REMOVE_SKIPWIRED != 0 && cur & PG_W != 0) {
            // VA not mapped, or wired and skipped
            pte = pte.wrapping_add(1);
            va += PAGE_SIZE;
            continue;
        }

        // atomically save the old PTE and zap! it
        // SAFETY: as above.
        let opte = unsafe { pmap_pte_set(pte, 0) };

        if opte & PG_W != 0 {
            pmap.pm_stats
                .wired_count
                .set(pmap.pm_stats.wired_count.get() - 1);
        }
        pmap.pm_stats
            .resident_count
            .set(pmap.pm_stats.resident_count.get() - 1);

        if let Some(ptp) = ptp {
            ptp.wire_count.set(ptp.wire_count.get() - 1); // dropping a PTE
        }

        let pg = PHYS_TO_VM_PAGE(Paddr::new((opte & pg_frame()) as usize));

        // if we are not on a pv list we are done.
        if opte & PG_PVLIST == 0 {
            #[cfg(feature = "diagnostic")]
            if pg.is_some() {
                crate::kern::subr_prf::panic(format_args!(
                    "pmap_remove_ptes: managed page without PG_PVLIST: va {va:#x}, opte {opte:#x}"
                ));
            }
            pte = pte.wrapping_add(1);
            va += PAGE_SIZE;
            continue;
        }

        let Some(pg) = pg else {
            crate::kern::subr_prf::panic(format_args!(
                "pmap_remove_ptes: unmanaged page marked PG_PVLIST: va {va:#x}, opte {opte:#x}"
            ));
        };

        // sync R/M bits
        pmap_sync_flags_pte(pg, opte);
        if let Some(pve) = pmap_remove_pv(pg, pmap, va) {
            // SAFETY: just unlinked; this function owns it now.
            unsafe { pve.as_ref() }.pv_next.set(*free_pvs);
            *free_pvs = pve.as_ptr();
        }

        // end of "for" loop: time for next pte
        pte = pte.wrapping_add(1);
        va += PAGE_SIZE;
    }
}

/// `pmap_remove`: top level mapping removal function.
///
/// caller should not be holding any pmap locks
pub fn pmap_remove(pmap: &Pmap, sva: Vaddr, eva: Vaddr) {
    // NVMM: pmap_is_ept → pmap_remove_ept: vmm is not configured.
    pmap_do_remove(pmap, sva.as_usize(), eva.as_usize(), PMAP_REMOVE_ALL);
}

/// `pmap_do_remove`: mapping removal guts.
///
/// caller should not be holding any pmap locks
fn pmap_do_remove(pmap: &Pmap, sva: usize, eva: usize, flags: i32) {
    let mut free_pvs: *const PvEntry = ptr::null();
    let empty_ptps = Pglist::new();
    empty_ptps.init();

    // pmap_map_ptes(pmap): the direct-map walk needs no borrowed %cr3.
    let shootself = pmap_is_curpmap(pmap);
    let is_kernel = ptr::eq(pmap, pmap_kernel());

    // The single-page shortcut and the block loop share the same body here.
    let shootall = (eva - sva > 32 * PAGE_SIZE) && sva < VM_MIN_KERNEL_ADDRESS;

    let mut va = sva;
    while va < eva {
        // determine range of block
        let mut blkendva = x86_round_pdr(va + 1);
        if blkendva > eva {
            blkendva = eva;
        }

        // XXXCDC: our PTE mappings should never be removed with pmap_remove! if we allow
        // this (and why would we?) then we end up freeing the pmap's page directory page
        // (PDP) before we are finished using it when we hit it in the recursive mapping.
        // this is BAD.
        if pl_i(va, PTP_LEVELS) == PDIR_SLOT_PTE {
            // XXXCDC: ugly hack to avoid freeing PDP here
            va = blkendva;
            continue;
        }

        // pmap_pdes_valid(va, &pde), through the direct map
        let (level, table, offs) = pmap_find_pte_direct(pmap, va);
        if level != 0 {
            va = blkendva;
            continue;
        }

        // PA of the PTP
        let ptppa = pmap_direct_unmap(Vaddr::new(table));

        // get PTP if non-kernel mapping
        let ptp = if is_kernel {
            // we never free kernel PTPs
            None
        } else {
            let Some(ptp) = pmap_find_ptp(pmap, va, Some(ptppa), 1) else {
                #[allow(clippy::panic)] // the C panics here too (DIAGNOSTIC)
                {
                    panic!("pmap_do_remove: unmanaged PTP detected");
                }
            };
            Some(ptp)
        };
        pmap_remove_ptes(
            pmap,
            ptp,
            table + offs * size_of::<PtEntry>(),
            va,
            blkendva,
            flags,
            &mut free_pvs,
        );

        // if PTP is no longer being used, free it!
        if let Some(ptp) = ptp
            && ptp.wire_count.get() <= 1
        {
            pmap_free_ptp(pmap, ptp, va, &empty_ptps);
        }
        va = blkendva;
    }

    if shootall {
        pmap_tlb_shoottlb(pmap, shootself);
    } else {
        pmap_tlb_shootrange(pmap, sva, eva, shootself);
    }

    // pmap_unmap_ptes(pmap, scr3): nothing borrowed.
    pmap_tlb_shootwait();

    // cleanup:
    pmap_free_pvs(free_pvs);

    while let Some(ptp) = empty_ptps.first() {
        // SAFETY: the page is on this list, which only this function sees.
        unsafe { empty_ptps.remove(ptp) };
        uvm_pagefree(ptp);
    }
}

/// `pmap_page_remove`: remove a managed vm_page from all pmaps that map it.
///
/// R/M bits are sync'd back to attrs.
pub fn pmap_page_remove(pg: &VmPage) {
    let empty_ptps = Pglist::new();
    empty_ptps.init();

    mtx_enter(&pg.mdpage.pv_mtx);
    while let Some(pve) = NonNull::new(pg.mdpage.pv_list.get().cast_mut()) {
        // SAFETY: an entry on the pv list, protected by `pv_mtx`.
        let pmp = unsafe { pve.as_ref() }.pv_pmap.get();
        // SAFETY: a pmap with a mapping on a pv list is live; the reference taken here keeps it
        // so until the `pmap_destroy` below.
        let pm: &'static Pmap = unsafe { &*pmp };
        pmap_reference(pm);
        mtx_leave(&pg.mdpage.pv_mtx);

        // pmap_map_ptes(pm): the direct-map walk needs no borrowed %cr3 (see the module's
        // deviations); locks pmap.
        let shootself = pmap_is_curpmap(pm);

        // We dropped the pvlist lock before grabbing the pmap lock to avoid lock ordering
        // problems. This means we have to check the pvlist again since somebody else might
        // have modified it. All we care about is that the pvlist entry matches the pmap we
        // just locked. If it doesn't, unlock the pmap and try again.
        mtx_enter(&pg.mdpage.pv_mtx);
        let Some(pve) = NonNull::new(pg.mdpage.pv_list.get().cast_mut()) else {
            mtx_leave(&pg.mdpage.pv_mtx);
            pmap_destroy(pm);
            mtx_enter(&pg.mdpage.pv_mtx);
            continue;
        };
        // SAFETY: as above.
        let e = unsafe { pve.as_ref() };
        if !ptr::eq(e.pv_pmap.get(), pm) {
            mtx_leave(&pg.mdpage.pv_mtx);
            pmap_destroy(pm);
            mtx_enter(&pg.mdpage.pv_mtx);
            continue;
        }

        pg.mdpage.pv_list.set(e.pv_next.get());
        mtx_leave(&pg.mdpage.pv_mtx);

        let va = e.pv_va.get().as_usize();
        // SAFETY: a PTP recorded in a pv entry stays allocated while the mapping exists.
        let ptp = unsafe { e.pv_ptp.get().as_ref() };
        let (level, table, offs) = pmap_find_pte_direct(pm, va);

        #[cfg(feature = "diagnostic")]
        if let Some(ptp) = ptp
            && level == 0
            && pmap_direct_unmap(Vaddr::new(table)) != vm_page_to_phys(ptp)
        {
            kprintf!(
                "pmap_page_remove: pg={:p}: va={:#x}, pv_ptp={:p}\n",
                ptr::from_ref(pg),
                va,
                ptr::from_ref(ptp)
            );
            crate::kern::subr_prf::panic(format_args!(
                "pmap_page_remove: mapped managed page has invalid pv_ptp field"
            ));
        }
        kassert!(level == 0);

        // atomically save the old PTE and zap it
        // SAFETY: the PTE of a mapping on the pv list, through the direct map.
        let opte =
            unsafe { pmap_pte_set((table + offs * size_of::<PtEntry>()) as *mut PtEntry, 0) };

        if opte & PG_W != 0 {
            pm.pm_stats
                .wired_count
                .set(pm.pm_stats.wired_count.get() - 1);
        }
        pm.pm_stats
            .resident_count
            .set(pm.pm_stats.resident_count.get() - 1);

        pmap_tlb_shootpage(pm, va, shootself);

        pmap_sync_flags_pte(pg, opte);

        // update the PTP reference count. free if last reference.
        if let Some(ptp) = ptp {
            ptp.wire_count.set(ptp.wire_count.get() - 1);
            if ptp.wire_count.get() <= 1 {
                pmap_free_ptp(pm, ptp, va, &empty_ptps);
            }
        }
        // pmap_unmap_ptes(pm, scr3): unlocks pmap.
        pmap_destroy(pm);
        pool_put(&PMAP_PV_POOL, pve.cast::<u8>());
        mtx_enter(&pg.mdpage.pv_mtx);
    }
    mtx_leave(&pg.mdpage.pv_mtx);

    pmap_tlb_shootwait();

    while let Some(ptp) = empty_ptps.first() {
        // SAFETY: the page is on this list, which only this function sees.
        unsafe { empty_ptps.remove(ptp) };
        uvm_pagefree(ptp);
    }
}

// p m a p   a t t r i b u t e  f u n c t i o n s
// functions that test/change managed page's attributes
// since a page can be mapped multiple times we must check each PTE that maps it by going
// down the pv lists.

/// `pmap_test_attrs`: test a page's attributes.
pub fn pmap_test_attrs(pg: &VmPage, testbits: u64) -> bool {
    let testflags = pmap_pte2flags(testbits);

    if pg.flags() & testflags != 0 {
        return true;
    }

    let mut mybits = 0;
    mtx_enter(&pg.mdpage.pv_mtx);
    let mut cur = pg.mdpage.pv_list.get();
    // SAFETY: entries on the pv list are live pool items, protected by `pv_mtx`.
    while let Some(pve) = unsafe { cur.as_ref() }
        && mybits == 0
    {
        // SAFETY: the pmap of a mapping on the pv list is live.
        let pm = unsafe { &*pve.pv_pmap.get() };
        let (_level, ptes, offs) = pmap_find_pte_direct(pm, pve.pv_va.get().as_usize());
        // SAFETY: the table holding the mapping's PTE, through the direct map.
        mybits |= unsafe { pde_at(ptes, offs) } & testbits;
        cur = pve.pv_next.get();
    }
    mtx_leave(&pg.mdpage.pv_mtx);

    if mybits == 0 {
        return false;
    }

    pg.set_bits(pmap_pte2flags(mybits));

    true
}

/// `pmap_clear_attrs`: change a page's attributes.
///
/// We return true if we cleared one of the bits we were asked to.
pub fn pmap_clear_attrs(pg: &VmPage, clearbits: u64) -> bool {
    let clearflags = pmap_pte2flags(clearbits);

    let mut result = pg.flags() & clearflags != 0;
    if result {
        pg.clear_bits(clearflags);
    }

    mtx_enter(&pg.mdpage.pv_mtx);
    let mut cur = pg.mdpage.pv_list.get();
    // SAFETY: entries on the pv list are live pool items, protected by `pv_mtx`.
    while let Some(pve) = unsafe { cur.as_ref() } {
        // SAFETY: the pmap of a mapping on the pv list is live.
        let pm = unsafe { &*pve.pv_pmap.get() };
        let va = pve.pv_va.get().as_usize();
        let (_level, ptes, offs) = pmap_find_pte_direct(pm, va);
        // SAFETY: the table holding the mapping's PTE, through the direct map.
        let opte = unsafe { pde_at(ptes, offs) };
        if opte & clearbits != 0 {
            result = true;
            // SAFETY: as above.
            unsafe {
                pmap_pte_clearbits(
                    (ptes + offs * size_of::<PtEntry>()) as *mut PtEntry,
                    opte & clearbits,
                );
            }
            pmap_tlb_shootpage(pm, va, pmap_is_curpmap(pm));
        }
        cur = pve.pv_next.get();
    }
    mtx_leave(&pg.mdpage.pv_mtx);

    pmap_tlb_shootwait();

    result
}

// p m a p   p r o t e c t i o n   f u n c t i o n s
//
// pmap_page_protect and pmap_protect are the inline functions of `include/pmap.rs`.

/// `pmap_write_protect`: write-protect pages in a pmap.
pub fn pmap_write_protect(pmap: &Pmap, sva: Vaddr, eva: Vaddr, prot: VmProt) {
    let (sva, eva) = (sva.as_usize(), eva.as_usize());
    let mut clear: PtEntry = 0;
    let mut set: PtEntry = 0;

    // pmap_map_ptes(pmap): the direct-map walk needs no borrowed %cr3.
    let shootself = pmap_is_curpmap(pmap);

    if prot & PROT_READ == 0 {
        set |= PG_XO_BITS.load(Ordering::Relaxed);
    }
    if prot & PROT_WRITE == 0 {
        clear = PG_RW;
    }
    if prot & PROT_EXEC == 0 {
        set |= pg_nx();
    }

    let shootall = (eva - sva > 32 * PAGE_SIZE) && sva < VM_MIN_KERNEL_ADDRESS;

    let mut va = sva;
    while va < eva {
        // determine range of block
        let mut blkendva = x86_round_pdr(va + 1);
        if blkendva > eva {
            blkendva = eva;
        }

        // XXXCDC: our PTE mappings should never be write-protected!
        //
        // long term solution is to move the PTEs out of user address space. and into kernel
        // address space (up with APTE). then we can set VM_MAXUSER_ADDRESS to be
        // VM_MAX_ADDRESS.

        // XXXCDC: ugly hack to avoid freeing PDP here
        if pl_i(va, PTP_LEVELS) == PDIR_SLOT_PTE {
            va = blkendva;
            continue;
        }

        // empty block?
        let (level, table, offs) = pmap_find_pte_direct(pmap, va);
        if level != 0 {
            va = blkendva;
            continue;
        }

        #[cfg(feature = "diagnostic")]
        if (VM_MAXUSER_ADDRESS..VM_MAX_ADDRESS).contains(&va) {
            crate::kern::subr_prf::panic(format_args!("pmap_write_protect: PTE space"));
        }

        let mut spte = (table + offs * size_of::<PtEntry>()) as *mut PtEntry;
        let epte = spte.wrapping_add((blkendva - va) / PAGE_SIZE);

        while spte < epte {
            // SAFETY: entries of the level-1 table of this block, through the direct map.
            unsafe {
                if pmap_valid_entry(ptr::read_volatile(spte)) {
                    pmap_pte_clearbits(spte, clear);
                    pmap_pte_setbits(spte, set);
                }
            }
            spte = spte.wrapping_add(1);
        }
        va = blkendva;
    }

    if shootall {
        pmap_tlb_shoottlb(pmap, shootself);
    } else {
        pmap_tlb_shootrange(pmap, sva, eva, shootself);
    }

    // pmap_unmap_ptes(pmap, scr3): nothing borrowed.
    pmap_tlb_shootwait();
}

// end of protection functions

/// `pmap_enter`: enter a mapping into a pmap.
///
/// Returns `ENOMEM` under `PMAP_CANFAIL` when a page-table page cannot be had; panics
/// otherwise, as the C.
pub fn pmap_enter(
    pmap: &Pmap,
    va: Vaddr,
    pa: Paddr,
    prot: VmProt,
    flags: i32,
) -> Result<(), Errno> {
    let va = va.as_usize();
    let pa_raw = pa.as_usize();
    let wired = flags & PMAP_WIRED != 0;
    let crypt = flags & PMAP_NOCRYPT == 0;
    let nocache = pa_raw & PMAP_NOCACHE as usize != 0;
    let mut wc = pa_raw & PMAP_WC as usize != 0;
    let is_kernel = ptr::eq(pmap, pmap_kernel());

    // NVMM: pmap_is_ept → pmap_enter_ept: vmm is not configured.

    kassert!(!(wc && nocache));
    let pa = pa_raw & PMAP_PA_MASK;

    #[cfg(feature = "diagnostic")]
    {
        if va == crate::arch::amd64::include::pmap::PDP_BASE {
            crate::kern::subr_prf::panic(format_args!("pmap_enter: trying to map over PDP!"));
        }
        // sanity check: kernel PTPs should already have been pre-allocated
        if va >= VM_MIN_KERNEL_ADDRESS
            // SAFETY: the kernel PML4 is readable at `pm_pdir`.
            && !pmap_valid_entry(unsafe { pde_at(pmap.pm_pdir.get() as usize, pl_i(va, PTP_LEVELS)) })
        {
            crate::kern::subr_prf::panic(format_args!(
                "pmap_enter: missing kernel PTP for va {va:#x}!"
            ));
        }
    }

    let Some(pvmem) = pool_get(&PMAP_PV_POOL, PR_NOWAIT) else {
        if flags & PMAP_CANFAIL != 0 {
            return Err(Errno::ENOMEM);
        }
        crate::kern::subr_prf::panic(format_args!("pmap_enter: no pv entries available"));
    };
    let pve_new = pvmem.cast::<PvEntry>();
    // SAFETY: a fresh pool item of `size_of::<PvEntry>()` bytes, suitably aligned; written
    // once before use.
    unsafe {
        pve_new.as_ptr().write(PvEntry {
            pv_next: Cell::new(ptr::null()),
            pv_pmap: Cell::new(ptr::null()),
            pv_va: Cell::new(Vaddr::new(0)),
            pv_ptp: Cell::new(ptr::null()),
        });
    }
    let mut pve = Some(pve_new);
    let mut opve: Option<NonNull<PvEntry>> = None;

    // map in ptes and get a pointer to our PTP (unless we are the kernel)
    let shootself = pmap_is_curpmap(pmap);
    let ptp = if is_kernel {
        None
    } else {
        match pmap_get_ptp(pmap, va) {
            Some(ptp) => Some(ptp),
            None => {
                if flags & PMAP_CANFAIL != 0 {
                    // pmap_unmap_ptes(pmap, scr3): nothing borrowed.
                    pool_put(&PMAP_PV_POOL, pve_new.cast::<u8>());
                    return Err(Errno::ENOMEM);
                }
                crate::kern::subr_prf::panic(format_args!("pmap_enter: get ptp failed"));
            }
        }
    };
    let (level, table, offs) = pmap_find_pte_direct(pmap, va);
    kassert!(level == 0);
    let pte = (table + offs * size_of::<PtEntry>()) as *mut PtEntry;
    // SAFETY: the level-1 table of `va` exists (the kernel's were pre-allocated, the user
    // one `pmap_get_ptp` just made or found), reached through the direct map.
    let opte = unsafe { ptr::read_volatile(pte) }; // old PTE

    let resdelta: i64;
    let wireddelta: i64;
    let ptpdelta: u32;
    let mut pg: Option<&VmPage> = None;
    let mut enter_now = false;
    // is there currently a valid mapping at our VA?
    if pmap_valid_entry(opte) {
        // first, calculate pm_stats updates. resident count will not change since we are
        // replacing/changing a valid mapping. wired count might change...
        resdelta = 0;
        wireddelta = if wired && opte & PG_W == 0 {
            1
        } else if !wired && opte & PG_W != 0 {
            -1
        } else {
            0
        };
        ptpdelta = 0;

        // is the currently mapped PA the same as the one we want to map?
        if (opte & pg_frame()) as usize == pa {
            // if this is on the PVLIST, sync R/M bit
            if opte & PG_PVLIST != 0 {
                pg = PHYS_TO_VM_PAGE(Paddr::new(pa));
                let Some(p) = pg else {
                    crate::kern::subr_prf::panic(format_args!(
                        "pmap_enter: same pa, PG_PVLIST mapping with unmanaged page: \
                         va {va:#x}, opte {opte:#x}, pa {pa:#x}"
                    ));
                };
                pmap_sync_flags_pte(p, opte);
            } else {
                #[cfg(feature = "diagnostic")]
                if PHYS_TO_VM_PAGE(Paddr::new(pa)).is_some() {
                    crate::kern::subr_prf::panic(format_args!(
                        "pmap_enter: same pa, no PG_PVLIST mapping with managed page: \
                         va {va:#x}, opte {opte:#x}, pa {pa:#x}"
                    ));
                }
            }
            enter_now = true;
        } else if opte & PG_PVLIST != 0 {
            // changing PAs: we must remove the old one first

            // if current mapping is on a pvlist, remove it (sync R/M bits)
            let Some(opg) = PHYS_TO_VM_PAGE(Paddr::new((opte & pg_frame()) as usize)) else {
                crate::kern::subr_prf::panic(format_args!(
                    "pmap_enter: PG_PVLIST mapping with unmanaged page: \
                     va {va:#x}, opte {opte:#x}, pa {pa:#x}"
                ));
            };
            pmap_sync_flags_pte(opg, opte);
            opve = pmap_remove_pv(opg, pmap, va);
            // pg = NULL: this is not the page we are looking for
        }
    } else {
        // opte not valid
        resdelta = 1;
        wireddelta = if wired { 1 } else { 0 };
        ptpdelta = if ptp.is_some() { 1 } else { 0 };
    }

    // pve is either NULL or points to a now-free pv_entry structure (the latter case is if
    // we called pmap_remove_pv above).
    //
    // if this entry is to be on a pvlist, enter it now.
    if !enter_now {
        if PMAP_INITIALIZED.load(Ordering::Relaxed) {
            pg = PHYS_TO_VM_PAGE(Paddr::new(pa));
        }

        if let Some(p) = pg
            && let Some(e) = pve.take()
        {
            pmap_enter_pv(p, e, pmap, va, ptp);
        }
    }

    // enter_now:
    // at this point pg is !NULL if we want the PG_PVLIST bit set
    pmap.pm_stats
        .resident_count
        .set(pmap.pm_stats.resident_count.get() + resdelta);
    pmap.pm_stats
        .wired_count
        .set(pmap.pm_stats.wired_count.get() + wireddelta);
    if let Some(ptp) = ptp {
        ptp.wire_count.set(ptp.wire_count.get() + ptpdelta);
    }

    kassert!(
        pg.map(ptr::from_ref) == PHYS_TO_VM_PAGE(Paddr::new(pa)).map(ptr::from_ref)
            || !PMAP_INITIALIZED.load(Ordering::Relaxed)
    );

    // SAFETY: `pmap_bootstrap` filled the table once, before any mapping.
    let protection_codes = unsafe { PROTECTION_CODES.get() };
    let mut npte = pa as u64 | protection_codes[(prot & 7) as usize] | PG_V;
    if let Some(pg) = pg {
        npte |= PG_PVLIST;
        // make sure that if the page is write combined all instances of pmap_enter make it
        // so.
        if pg.flags() & crate::arch::amd64::include::pmap::PG_PMAP_WC != 0 {
            kassert!(!nocache);
            wc = true;
        }
    }
    if wc {
        npte |= PMAP_PG_WC.load(Ordering::Relaxed);
    }
    if wired {
        npte |= PG_W;
    }
    if nocache {
        npte |= PG_N;
    }
    if va < VM_MAXUSER_ADDRESS {
        npte |= PG_u; // PMAP_EFI is never set here
    } else if va < VM_MAX_ADDRESS {
        npte |= PG_u | PG_RW; // XXXCDC: no longer needed?
    }
    if is_kernel {
        npte |= PG_G_KERN.load(Ordering::Relaxed);
    }
    if crypt {
        npte |= pg_crypt();
    }

    // If the old entry wasn't valid, we can just update it and go. If it was valid, and
    // this isn't a read->write transition, then we can safely just update it and flush any
    // old TLB entries.
    //
    // If it _was_ valid and this _is_ a read->write transition, then this could be a CoW
    // resolution and we need to make sure no CPU can see the new writable mapping while
    // another still has the old mapping in its TLB, so insert a correct but unwritable
    // mapping, flush any old TLB entries, then make it writable.
    // SAFETY: as for the read above; the entry is this pmap's.
    unsafe {
        if !pmap_valid_entry(opte) {
            ptr::write_volatile(pte, npte);
        } else if (opte | (npte ^ PG_RW)) & PG_RW != 0 {
            // previously writable or not making writable
            ptr::write_volatile(pte, npte);
            if nocache && opte & PG_N == 0 {
                wbinvd_on_all_cpus();
            }
            pmap_tlb_shootpage(pmap, va, shootself);
        } else {
            ptr::write_volatile(pte, npte ^ PG_RW);
            if nocache && opte & PG_N == 0 {
                // XXX impossible?
                wbinvd_on_all_cpus();
            }
            pmap_tlb_shootpage(pmap, va, shootself);
            pmap_tlb_shootwait();
            ptr::write_volatile(pte, npte);
        }
    }

    // pmap_unmap_ptes(pmap, scr3): nothing borrowed.
    pmap_tlb_shootwait();

    // out:
    if let Some(e) = pve {
        pool_put(&PMAP_PV_POOL, e.cast::<u8>());
    }
    if let Some(e) = opve {
        pool_put(&PMAP_PV_POOL, e.cast::<u8>());
    }

    Ok(())
}

/// `pmap_remove_holes`: nothing on amd64 (the C macro).
pub fn pmap_remove_holes(_vm: &Vmspace) {}

/// `pmap_proc_iflush`: nothing on amd64: the instruction cache is coherent.
pub fn pmap_proc_iflush(_pr: &Process, _va: Vaddr, _len: Vsize) {}
