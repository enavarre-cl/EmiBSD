/* $OpenBSD: pmap.c,v 1.113 2025/07/12 08:35:32 kettenis Exp $ */
/* <LICENSES> */
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
/* </LICENSES> */

//! arm64 physical map: `arch/arm64/arm64/pmap.c`.
//!
//! Upstream: sys/arch/arm64/arm64/pmap.c @ 3ce1f3f79392
//!
//! Status: `wip`. Milestone M3 ports `struct pte_desc`, the `pmapvp*` tables, the kernel
//! pmap, `pmap_bootstrap`, `pmap_vp_lookup`/`pmap_vp_enter`, `pmap_set_l1`/`l2`/`l3`,
//! `pmap_kvp_alloc`/`pmap_kpted_alloc`, `pmap_growkernel`, `pmap_kenter_pa`/`pmap_kenter_cache`,
//! `pmap_kremove_pg`/`pmap_kremove`, `pmap_fill_pte`, `pmap_pte_insert`/`update`/`remove`,
//! `pmap_extract`, `pmap_zero_page`, `pmap_copy_page`, `pmap_virtual_space` and `ttlb_flush`.
//! The user pmaps (`pmap_create`, `pmap_enter`, `pmap_remove`, `pmap_vp_populate`, the pv
//! lists, the ASIDs, `pmap_fault_fixup`, the pools, the icache sync) come with M4 to M6.
//!
//! ## Deviations
//! - OpenBSD arm64 has no direct map and no `PMAP_STEAL_MEMORY`: boot allocations go through
//!   `pmap_steal_avail` (a bump allocator over `pmap_avail[]`, mapped by `pmap_map_stolen`)
//!   and `uvm_pageboot_alloc` maps page by page with `pmap_kenter_pa`. Both need page tables
//!   the kernel does not own yet, so until it does the bootloader's higher-half direct map
//!   serves as `__HAVE_PMAP_DIRECT` and boot memory is stolen through it: `pmap_steal_memory`
//!   takes contiguous frames out of `vm_physmem[]` with `uvm_page_physsteal` (the
//!   `vm_physmem[]` half of amd64's `pmap_steal_memory`, `uvm/uvm_page.rs`).
//! - The kernel's page tables are the bootloader's, adopted: `pmap_bootstrap` copies its
//!   level-0 table and the level-1 table of the kernel range's slot into fresh `pmapvp0` and
//!   `pmapvp1` (so they have the software shadow the vp walks need), points `TTBR1_EL1` at the
//!   copy, and leaves the level-2/3 tables of the kernel image and of the direct map as the
//!   bootloader built them: those pages have no `pte_desc`, so `pmap_extract` answers for
//!   them through the direct map or not at all. The C builds the kernel tables from scratch
//!   (`pmap_setup_avail`, `pmap_map_stolen`, `switch_mmu_kernel`) and pre-populates the first
//!   GiB in `pmap_bootstrap`; here `initarm`'s `pmap_growkernel` populates it, so
//!   `pmap_maxkvaddr` starts at `VM_MIN_KERNEL_ADDRESS`. `initarm` therefore loads the memory
//!   map into `uvm` before calling `pmap_bootstrap`, which the C does after.
//! - The kernel pmap is four-level (`pm_vp.l0`, `TCR_EL1.T1SZ` = 16 as the bootloader left
//!   it); the C's is three-level (a 39-bit kernel), so where the C reaches `pm_vp.l1`
//!   directly, the walks here go through `l0->vp[VP_IDX0(va)]`.
//! - `MAIR_EL1` indices 2, 3 and 4 are programmed by `pmap_bootstrap` around the bootloader's
//!   0 and 1 (`include/pte.rs`, deviations); the C's `locore.S` writes the whole register.
//! - `pmap_kvp_alloc` and `pmap_kpted_alloc` take their pages through the direct map
//!   (`uvm_page_physsteal` before `uvm_page_init`, `uvm_pglistalloc` after) instead of
//!   mapping them at `virtual_avail` with `pmap_kenter_pa` (before) or allocating from the
//!   `kv_kvp` submap with `km_alloc` (after, which waits for `uvm_map`).
//! - `pmap_vp_enter` allocates the kernel pmap's tables with `pmap_kvp_alloc`; user pmaps
//!   need `pmap_vp_pool` (`subr_pool.c`), reported as unported, as is `pmap_vp_populate`.
//! - `pmap_zero_page`/`pmap_copy_page` use the direct map instead of the per-CPU
//!   `zero_page`/`copy_src_page`/`copy_dst_page` windows.
//! - `pmap_init`: the TTBR0 switch to `pm_pt0pa` and the pools wait for the page tables to
//!   map devices (`initarm`'s bootstrap device map lives in TTBR0). Reported as unported.
//! - `splvm` in `pmap_kremove_pg` waits for `spl(9)` (M4); `pmap_remove_pv` (managed pages)
//!   and `cpu_idcache_wbinv_range` (non-cacheable mappings of managed pages) are reported
//!   when reached; kernel mappings made here are never managed, so nothing is dropped.
//! - User pmaps (M6): `pmap_create`/`pmap_pinit`/`pmap_destroy`/`pmap_release`,
//!   `pmap_vp_destroy`, `pmap_enter`/`pmap_remove`/`pmap_remove_pted`, the pv lists
//!   (`pmap_enter_pv`/`pmap_remove_pv`), `pmap_activate`/`pmap_deactivate`/`pmap_setttb`
//!   and the ASIDs. Every vp table (the kernel's and the users') comes from the two-page
//!   allocator `pmap_vp_pages` instead of `pmap_vp_pool`, and goes back page by page
//!   (`pmap_vp_free`); the pteds of user pmaps come from `pmap_pted_pool`. The ASID bitmap
//!   is 8-bit (`pmap_nasid` 256, the generation never rolls over: `pmap_rollover_asid` is
//!   M7 and running out panics). `pmap_setpauthkeys` (pointer authentication) is M7.
//! - `pmap_init` also resizes `TCR_EL1.T0SZ` to `USER_SPACE_BITS` and remaps the console
//!   (`pluartcn_remap`) before pointing `TTBR0_EL1` at the empty table: the C's locore did
//!   both at boot, here the bootstrap device map (`machdep.rs`) lived in the lower half.

use core::arch::asm;
use core::cell::{Cell, UnsafeCell};
use core::ptr::{self, NonNull};
use core::sync::atomic::{AtomicBool, AtomicU32, AtomicU64, AtomicUsize, Ordering};

use crate::arch::arm64::arm64::cpufunc::{
    cpu_icache_sync_range, cpu_setttb, cpu_tlb_flush, cpu_tlb_flush_all_asid, cpu_tlb_flush_asid,
    cpu_tlb_flush_asid_all,
};
use crate::arch::arm64::include::cpu::curcpu;
use crate::arch::arm64::include::param::{PAGE_MASK, PAGE_SHIFT, PAGE_SIZE};
use crate::arch::arm64::include::pmap::{
    PG_PMAP_EXE, PG_PMAP_MOD, PG_PMAP_REF, PMAP_CACHE_BITS, PMAP_CACHE_CI, PMAP_CACHE_DEV_NGNRE,
    PMAP_CACHE_DEV_NGNRNE, PMAP_CACHE_WB, PMAP_CACHE_WT, PMAP_DEVICE, PMAP_NOCACHE,
    PTED_VA_MANAGED_M, PTED_VA_WIRED_M, PmVp, Pmap, VP_IDX0_CNT, VP_IDX0_MASK, VP_IDX0_POS,
    VP_IDX1_CNT, VP_IDX1_MASK, VP_IDX1_POS, VP_IDX2_CNT, VP_IDX2_MASK, VP_IDX2_POS, VP_IDX3_CNT,
    VP_IDX3_MASK, VP_IDX3_POS,
};
use crate::arch::arm64::include::pte::{
    ATTR_AF, ATTR_GP, ATTR_PXN, ATTR_UXN, ATTR_nG, L3_P, Lx_TABLE_ALIGN, Lx_TYPE_MASK, Lx_TYPE_PT,
    MAIR_CI, MAIR_DEV_NGNRNE, MAIR_WT, PTE_ATTR_CI, PTE_ATTR_DEV_NGNRE, PTE_ATTR_DEV_NGNRNE,
    PTE_ATTR_WB, PTE_ATTR_WT, PTE_RPGN, SH_INNER, attr_ap, attr_idx, attr_sh, mair_attr,
};
use crate::arch::arm64::include::vmparam::USER_SPACE_BITS;
use crate::arch::arm64::include::vmparam::{VM_MAX_KERNEL_ADDRESS, VM_MIN_KERNEL_ADDRESS};
use crate::dev::ic::pluart::pluartcn_remap;
use crate::kern::subr_pool::{pool_get, pool_init, pool_put};
use crate::machine::intr::IPL_VM;
use crate::sys::errno::Errno;
use crate::sys::mman::{PROT_EXEC, PROT_WRITE};
use crate::sys::pool::{PR_NOWAIT, PR_WAITOK, PR_ZERO, Pool};
use crate::sys::proc::Proc;
use crate::sys::proc::Process;
use crate::sys::queue::ListEntry;
use crate::sys::queue::ListHead;
use crate::sys::types::{Paddr, Vaddr, Vsize};
use crate::uvm::uvm_extern::Vmspace;
use crate::uvm::uvm_extern::{PROT_MASK, UVM_PLA_NOWAIT, UVM_PLA_ZERO, VmProt};
use crate::uvm::uvm_init::UVM;
use crate::uvm::uvm_page::uvm_pglistfree;
use crate::uvm::uvm_page::{
    PHYS_TO_VM_PAGE, Pglist, VmPage, uvm_page_physsteal, uvm_pglistalloc, vm_page_to_phys,
};
use crate::uvm::uvm_param::{atop, round_page, trunc_page};
use crate::uvm::uvm_pmap::{PMAP_CANFAIL, PMAP_WIRED};
use crate::{kassert, queue_adapter, unported};

/// `TTBR1_EL1.BADDR`: bits 47:1 of the register hold the table's physical address.
const TTBR_BADDR_MASK: u64 = 0x0000_ffff_ffff_fffe;
/// `TCR_EL1.T1SZ`: the field's position and width.
const TCR_T1SZ_SHIFT: u32 = 16;
/// `T1SZ` under 4-level paging (48-bit upper half).
const T1SZ_4LEVEL: u64 = 16;
/// `ASID_USER`: we run userland code with ASIDs that have the low bit set.
const ASID_USER: u64 = 1;
/// The entries of one translation table.
const TABLE_ENTRIES: usize = 512;

/// `struct pte_desc`: one mapping, on its page's pv list.
pub struct PteDesc {
    /// `pted_pv_list`: the page's mappings.
    pub pted_pv_list: ListEntry<PteDesc>,
    /// The PTE as it should be: the frame and the `PROT_*` bits.
    pub pted_pte: Cell<u64>,
    /// The pmap this mapping belongs to.
    pub pted_pmap: Cell<*const Pmap>,
    /// The virtual address, with the `PMAP_CACHE_*`, `PROT_*` and `PTED_VA_*` flags in its
    /// low bits.
    pub pted_va: Cell<usize>,
}

queue_adapter!(
    /// `LIST_HEAD(, pte_desc)`: the pv list of a page.
    pub PvList: PteDesc, pted_pv_list => ListEntry<PteDesc>
);

macro_rules! vp_table {
    ($(#[$meta:meta])* $name:ident, $table:ident, $child:ty) => {
        $(#[$meta])*
        #[repr(C, align(4096))]
        pub struct $name {
            /// The hardware table: one page the MMU reads.
            $table: UnsafeCell<[u64; TABLE_ENTRIES]>,
            /// The software pointers to the next level.
            vp: [Cell<*mut $child>; TABLE_ENTRIES],
        }

        // SAFETY: the pmap's lock guards the tables (M5); the boot CPU is alone until then.
        unsafe impl Sync for $name {}

        impl $name {
            /// Entry `i` of the hardware table.
            pub fn table(&self, i: usize) -> u64 {
                // SAFETY: `addr_of!` bounds-checks `i` without making a reference; the page is
                // RAM only this CPU and the MMU read.
                unsafe { ptr::read_volatile(ptr::addr_of!((*self.$table.get())[i])) }
            }

            /// Sets entry `i` of the hardware table.
            pub fn set_table(&self, i: usize, e: u64) {
                // SAFETY: as for `table`; the write reaches the MMU through the barriers of
                // the TLB flush that follows every table change.
                unsafe { ptr::write_volatile(ptr::addr_of_mut!((*self.$table.get())[i]), e) };
            }

            /// The next-level table (or pted) at `i`. The tables live as long as the kernel
            /// (user pmaps return theirs to a pool in M6).
            pub fn vp(&self, i: usize) -> Option<&'static $child> {
                // SAFETY: a non-null pointer names a table allocated for the kernel's life.
                unsafe { self.vp[i].get().as_ref() }
            }

            /// Sets the next-level pointer at `i`.
            pub fn set_vp(&self, i: usize, child: Option<&$child>) {
                self.vp[i].set(child.map_or(ptr::null_mut(), |c| ptr::from_ref(c).cast_mut()));
            }
        }
    };
}

vp_table!(
    /// `struct pmapvp0`: level 0, 512 GiB per entry.
    Pmapvp0, l0, Pmapvp1
);
vp_table!(
    /// `struct pmapvp1`: level 1, 1 GiB per entry.
    Pmapvp1, l1, Pmapvp2
);
vp_table!(
    /// `struct pmapvp2`: level 2, 2 MiB per entry.
    Pmapvp2, l2, Pmapvp3
);
vp_table!(
    /// `struct pmapvp3`: level 3, 4 KiB per entry; the pointers are the pteds.
    Pmapvp3, l3, PteDesc
);

/// The direct map's base: physical address 0 is mapped here (see the module's deviations).
pub static PMAP_DIRECT_BASE: AtomicUsize = AtomicUsize::new(0);
/// The end of the direct map.
pub static PMAP_DIRECT_END: AtomicUsize = AtomicUsize::new(0);
/// `virtual_avail`: the first free kernel virtual address.
static VIRTUAL_AVAIL: AtomicUsize = AtomicUsize::new(0);
/// `pmap_virtual_space_called`: prevent further KVA stealing.
static PMAP_VIRTUAL_SPACE_CALLED: AtomicBool = AtomicBool::new(false);
/// `pmap_maxkvaddr`: how far the kernel's vp tables and pteds reach (see the module's
/// deviations for its start).
static PMAP_MAXKVADDR: AtomicUsize = AtomicUsize::new(VM_MIN_KERNEL_ADDRESS);
/// `pmap_initialized`.
static PMAP_INITIALIZED: AtomicBool = AtomicBool::new(false);
/// `mappings_allocated`: vp tables allocated for the kernel.
static MAPPINGS_ALLOCATED: AtomicUsize = AtomicUsize::new(0);
/// `pted_allocated`: pted pages allocated for the kernel.
static PTED_ALLOCATED: AtomicUsize = AtomicUsize::new(0);
/// `vmmap`: a kernel virtual page kept for `pmap_bootstrap_bs_map` (M4).
pub static VMMAP: AtomicUsize = AtomicUsize::new(0);
/// `pmap_kpted_alloc`'s batch: the next pted (`pted`) ...
static KPTED_NEXT: AtomicUsize = AtomicUsize::new(0);
/// ... and how many are left in its page (`npted`).
static KPTED_LEFT: AtomicUsize = AtomicUsize::new(0);
/// `kernel_pmap_`: the kernel's pmap.
static KERNEL_PMAP: Pmap = Pmap::new();
/// `pmap_pmap_pool`: the pool of `struct pmap`.
static PMAP_PMAP_POOL: Pool = Pool::new();
/// `pmap_pted_pool`: the pool of `struct pte_desc` for user pmaps.
static PMAP_PTED_POOL: Pool = Pool::new();
/// `PMAP_ASID_MASK`: the ASID bits of `pm_asid`; the generation sits above.
const PMAP_ASID_MASK: u64 = 0xffff;
/// `pmap_nasid`: how many ASIDs the CPU has (8-bit, what every arm64 CPU implements;
/// `id_aa64mmfr0_el1`'s 16-bit probe is M7).
const PMAP_NASID: usize = 256;
/// `pmap_asid[]`: the ASID bitmap, two bits per pmap (the kernel's and the user's view).
static PMAP_ASID: [AtomicU32; PMAP_NASID / 32] = [const { AtomicU32::new(0) }; PMAP_NASID / 32];
/// `pmap_asid_gen`: the current ASID generation (above the mask); a fresh pmap's zero never
/// matches it.
static PMAP_ASID_GEN: AtomicU64 = AtomicU64::new(PMAP_ASID_MASK + 1);

/// `ap_bits_user[]`: the access bits of a user mapping, by `PROT_*`.
const AP_BITS_USER: [u64; 8] = [
    0,                                          // PROT_NONE
    ATTR_PXN | ATTR_UXN | ATTR_AF | attr_ap(3), // PROT_READ
    ATTR_PXN | ATTR_UXN | ATTR_AF | attr_ap(1), // PROT_WRITE
    ATTR_PXN | ATTR_UXN | ATTR_AF | attr_ap(1), // PROT_WRITE|PROT_READ
    ATTR_PXN | ATTR_AF | attr_ap(2),            // PROT_EXEC
    ATTR_PXN | ATTR_AF | attr_ap(3),            // PROT_EXEC|PROT_READ
    ATTR_PXN | ATTR_AF | attr_ap(1),            // PROT_EXEC|PROT_WRITE
    ATTR_PXN | ATTR_AF | attr_ap(1),            // PROT_EXEC|PROT_WRITE|PROT_READ
];

/// `ap_bits_kern[]`: the access bits of a kernel mapping, by `PROT_*`.
const AP_BITS_KERN: [u64; 8] = [
    0,                                          // PROT_NONE
    ATTR_PXN | ATTR_UXN | ATTR_AF | attr_ap(2), // PROT_READ
    ATTR_PXN | ATTR_UXN | ATTR_AF | attr_ap(0), // PROT_WRITE
    ATTR_PXN | ATTR_UXN | ATTR_AF | attr_ap(0), // PROT_WRITE|PROT_READ
    ATTR_UXN | ATTR_AF | attr_ap(2),            // PROT_EXEC
    ATTR_UXN | ATTR_AF | attr_ap(2),            // PROT_EXEC|PROT_READ
    ATTR_UXN | ATTR_AF | attr_ap(0),            // PROT_EXEC|PROT_WRITE
    ATTR_UXN | ATTR_AF | attr_ap(0),            // PROT_EXEC|PROT_WRITE|PROT_READ
];

/// `pmap_kernel()`.
pub fn pmap_kernel() -> &'static Pmap {
    &KERNEL_PMAP
}

/// `pmap_activate`: activate a pmap entry: `p`'s address space is in use by one more thread,
/// and switched in now if `p` is the running thread.
pub fn pmap_activate(p: &Proc) {
    let pm = p.vmspace().vm_map.pmap();

    pm.pm_active.fetch_add(1, Ordering::Relaxed);
    if ptr::eq(p, curcpu().ci_curproc.get()) && !ptr::eq(pm, curcpu().ci_curpm.get()) {
        pmap_setttb(ptr::from_ref(p).cast());
    }
}

/// `pmap_deactivate`: deactivate a pmap entry.
pub fn pmap_deactivate(p: &Proc) {
    let pm = p.vmspace().vm_map.pmap();

    kassert!(ptr::eq(p, curcpu().ci_curproc.get()));

    if pm.pm_active.load(Ordering::Relaxed) == 0 {
        return;
    }

    // SAFETY: the kernel's empty lower-half table; nothing of the kernel lives there.
    unsafe {
        asm!("msr ttbr0_el1, {}", "isb", in(reg) pmap_kernel().pm_pt0pa.get(), options(nostack, preserves_flags));
    }

    pm.pm_active.fetch_sub(1, Ordering::Relaxed);
}

/// `pmap_setttb`: loads `p`'s address space into `TTBR0_EL1`: called by `cpu_switchto_asm`
/// for every switch (hence `extern "C"` by its C name) and by `pmap_activate`.
///
/// `pmap_setpauthkeys` (pointer authentication) is M7.
#[unsafe(no_mangle)]
pub extern "C" fn pmap_setttb(p: *const core::ffi::c_void) {
    let ci = curcpu();
    // SAFETY: the switch hands over the thread it makes current (a `struct proc *`, opaque
    // to the ABI), alive by definition.
    let p = unsafe { &*p.cast::<Proc>() };
    let pm = p.vmspace().vm_map.pmap();

    // If the generation of the ASID for the new pmap doesn't match the current generation,
    // allocate a new ASID.
    if !ptr::eq(pm, pmap_kernel())
        && (pm.pm_asid.get() & !PMAP_ASID_MASK) != PMAP_ASID_GEN.load(Ordering::Relaxed)
    {
        pmap_allocate_asid(pm);
    }

    // SAFETY: the kernel's empty lower-half table first, so no translation of the old
    // address space can be cached under the new ASID; then the new tables.
    unsafe {
        asm!("msr ttbr0_el1, {}", "isb", in(reg) pmap_kernel().pm_pt0pa.get(), options(nostack, preserves_flags));
        cpu_setttb(pm.pm_asid.get(), pm.pm_pt0pa.get());
    }
    ci.ci_curpm.set(pm);
    if let Some(flush_bp) = ci.ci_flush_bp.get() {
        flush_bp();
    }
}

/// `pmap_initialized`.
pub fn pmap_initialized() -> bool {
    PMAP_INITIALIZED.load(Ordering::Relaxed)
}

/// `pmap_maxkvaddr`: how far the kernel's tables reach.
pub fn pmap_maxkvaddr() -> Vaddr {
    Vaddr::new(PMAP_MAXKVADDR.load(Ordering::Relaxed))
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

// virtual to physical helpers

/// `VP_IDX0(va)`.
fn vp_idx0(va: usize) -> usize {
    (va >> VP_IDX0_POS) & VP_IDX0_MASK
}

/// `VP_IDX1(va)`.
fn vp_idx1(va: usize) -> usize {
    (va >> VP_IDX1_POS) & VP_IDX1_MASK
}

/// `VP_IDX2(va)`.
fn vp_idx2(va: usize) -> usize {
    (va >> VP_IDX2_POS) & VP_IDX2_MASK
}

/// `VP_IDX3(va)`.
fn vp_idx3(va: usize) -> usize {
    (va >> VP_IDX3_POS) & VP_IDX3_MASK
}

/// `VP_Lx(pa)`: this function takes the pa address given and manipulates it into the form
/// that should be inserted into the VM table.
fn vp_lx(pa: Paddr) -> u64 {
    pa.as_usize() as u64 | Lx_TYPE_PT
}

/// `PTED_MANAGED(pted)`.
fn pted_managed(pted: &PteDesc) -> bool {
    pted.pted_va.get() & PTED_VA_MANAGED_M as usize != 0
}

/// `PTED_WIRED(pted)`.
fn pted_wired(pted: &PteDesc) -> bool {
    pted.pted_va.get() & PTED_VA_WIRED_M as usize != 0
}

/// `PTED_VALID(pted)`.
fn pted_valid(pted: &PteDesc) -> bool {
    pted.pted_pte.get() != 0
}

/// The pmap a filled pted belongs to.
fn pted_pmap(pted: &PteDesc) -> &'static Pmap {
    // SAFETY: `pmap_fill_pte` stores the pmap, which outlives its mappings (a pmap is
    // destroyed only once empty).
    unsafe { &*pted.pted_pmap.get() }
}

/// `ttlb_flush`: drops the TLB entries of one page of `pm`.
fn ttlb_flush(pm: &Pmap, va: usize) {
    if pm.pm_active.load(Ordering::Relaxed) == 0 {
        return;
    }

    let resva = ((va >> PAGE_SHIFT) & ((1u64 << 44) - 1) as usize) as u64;
    if ptr::eq(pm, pmap_kernel()) {
        cpu_tlb_flush_all_asid(resva);
    } else {
        let asid = pm.pm_asid.get();
        cpu_tlb_flush_asid(resva | asid << 48);
        cpu_tlb_flush_asid(resva | (asid | ASID_USER) << 48);
    }
}

/// The level-1 table of `va` in `pm`: through `l0` on a four-level pmap, `l1` itself on a
/// three-level one.
fn pmap_vp1(pm: &Pmap, va: usize) -> Option<&'static Pmapvp1> {
    match pm.pm_vp.get() {
        // SAFETY: a non-null `pm_vp` names a table allocated for the pmap's life.
        PmVp::L0(l0) => unsafe { l0.as_ref() }.and_then(|l0| l0.vp(vp_idx0(va))),
        // SAFETY: as above.
        PmVp::L1(l1) => unsafe { l1.as_ref() },
    }
}

/// `pmap_vp_lookup`: the pted of `va`, and the level-3 table and index that hold its PTE.
/// This is used for `pmap_kernel()` mappings, they are not to be removed from the vp table
/// because they were statically initialized at the initial pmap initialization. This is so
/// that memory allocation is not necessary in the `pmap_kernel()` mappings. Otherwise bad race
/// conditions can appear.
pub fn pmap_vp_lookup(
    pm: &Pmap,
    va: usize,
) -> (Option<&'static PteDesc>, Option<(&'static Pmapvp3, usize)>) {
    let Some(vp1) = pmap_vp1(pm, va) else {
        return (None, None);
    };
    let Some(vp2) = vp1.vp(vp_idx1(va)) else {
        return (None, None);
    };
    let Some(vp3) = vp2.vp(vp_idx2(va)) else {
        return (None, None);
    };
    (vp3.vp(vp_idx3(va)), Some((vp3, vp_idx3(va))))
}

/// `pool_get(&pmap_vp_pool, PR_NOWAIT | PR_ZERO)`: a zeroed vp table for `pm` (see the
/// module's deviations).
fn pmap_vp_alloc(_pm: &Pmap, flags: i32, level: &str) -> Result<*mut Pmapvp0, Errno> {
    // The kernel's and the user pmaps' tables come from the same two-page allocator (see the
    // module's deviations).
    let vp = pmap_kvp_alloc();
    match vp {
        Some(vp) => Ok(vp),
        None => {
            if flags & PMAP_CANFAIL == 0 {
                #[allow(clippy::panic)] // the C panics here too
                {
                    panic!("pmap_vp_enter: unable to allocate {}", level);
                }
            }
            Err(Errno::ENOMEM)
        }
    }
}

/// `pmap_vp_enter`: create a V -> P mapping for the given pmap and virtual address with
/// reference to the pte descriptor that is used to map the page. This code should track
/// allocations of vp table allocations so they can be freed efficiently.
pub fn pmap_vp_enter(pm: &Pmap, va: usize, pted: &PteDesc, flags: i32) -> Result<(), Errno> {
    // PMAP_ASSERT_LOCKED(pm): M5.

    let vp1 = match pm.pm_vp.get() {
        PmVp::L0(l0) => {
            // SAFETY: a four-level pmap's `l0` is set when the pmap is made.
            let Some(l0) = (unsafe { l0.as_ref() }) else {
                #[allow(clippy::panic)] // the C dereferences the null pointer
                {
                    panic!("pmap_vp_enter: pmap without a level-0 table");
                }
            };
            match l0.vp(vp_idx0(va)) {
                Some(vp1) => vp1,
                None => {
                    let vp1 = pmap_vp_alloc(pm, flags, "L1")?.cast::<Pmapvp1>();
                    // SAFETY: a fresh, zeroed table that lives for the pmap's life.
                    let vp1 = unsafe { &*vp1 };
                    pmap_set_l1(pm, va, vp1);
                    vp1
                }
            }
        }
        PmVp::L1(l1) => {
            // SAFETY: a three-level pmap's `l1` is set when the pmap is made.
            let Some(l1) = (unsafe { l1.as_ref() }) else {
                #[allow(clippy::panic)] // the C dereferences the null pointer
                {
                    panic!("pmap_vp_enter: pmap without a level-1 table");
                }
            };
            l1
        }
    };

    let vp2 = match vp1.vp(vp_idx1(va)) {
        Some(vp2) => vp2,
        None => {
            let vp2 = pmap_vp_alloc(pm, flags, "L2")?.cast::<Pmapvp2>();
            // SAFETY: as above.
            let vp2 = unsafe { &*vp2 };
            pmap_set_l2(pm, va, vp1, vp2);
            vp2
        }
    };

    let vp3 = match vp2.vp(vp_idx2(va)) {
        Some(vp3) => vp3,
        None => {
            let vp3 = pmap_vp_alloc(pm, flags, "L3")?.cast::<Pmapvp3>();
            // SAFETY: as above.
            let vp3 = unsafe { &*vp3 };
            pmap_set_l3(pm, va, vp2, vp3);
            vp3
        }
    };

    vp3.set_vp(vp_idx3(va), Some(pted));
    Ok(())
}

/// `pmap_vp_populate`: pre-allocates the vp tables and the pted of `va`; needs the pools.
pub fn pmap_vp_populate(_pm: &Pmap, _va: usize) {
    let _ = unported!("pmap_vp_populate (pool_get, subr_pool.c)");
}

/// Two zeroed, physically contiguous pages through the direct map (see the module's
/// deviations): the shape of every vp table.
fn pmap_vp_pages() -> Option<Vaddr> {
    let va = if !UVM.page_init_done.load(Ordering::Relaxed)
        && !PMAP_VIRTUAL_SPACE_CALLED.load(Ordering::Relaxed)
    {
        pmap_direct_map(uvm_page_physsteal(2)?)
    } else {
        // km_alloc(sizeof(struct pmapvp0), &kv_kvp, &kp_zero, &kd_nowait) in the C.
        let pgl = Pglist::new();
        pgl.init();
        uvm_pglistalloc(
            2 * PAGE_SIZE,
            Paddr::new(0),
            Paddr::new(usize::MAX),
            Paddr::new(0),
            Paddr::new(0),
            &pgl,
            1,
            UVM_PLA_NOWAIT | UVM_PLA_ZERO,
        )
        .ok()?;
        pmap_map_direct(pgl.first()?)
    };
    // SAFETY: two RAM pages just taken from the free segments, through the direct map.
    unsafe { ptr::write_bytes(va.as_usize() as *mut u8, 0, 2 * PAGE_SIZE) };
    Some(va)
}

/// `pmap_kvp_alloc`: allocator for growing the kernel page tables.
pub fn pmap_kvp_alloc() -> Option<*mut Pmapvp0> {
    let va = pmap_vp_pages()?;
    MAPPINGS_ALLOCATED.fetch_add(1, Ordering::Relaxed);
    // All-zero is a valid table: invalid descriptors and null pointers.
    Some(va.as_usize() as *mut Pmapvp0)
}

/// `pmap_kpted_alloc`: a zeroed pted for the kernel, a page of them at a time.
pub fn pmap_kpted_alloc() -> Option<&'static PteDesc> {
    let mut npted = KPTED_LEFT.load(Ordering::Relaxed);
    let mut pted = KPTED_NEXT.load(Ordering::Relaxed) as *mut PteDesc;

    if npted == 0 {
        let va = if !UVM.page_init_done.load(Ordering::Relaxed)
            && !PMAP_VIRTUAL_SPACE_CALLED.load(Ordering::Relaxed)
        {
            pmap_direct_map(uvm_page_physsteal(1)?)
        } else {
            // km_alloc(PAGE_SIZE, &kv_kvp, &kp_zero, &kd_nowait) in the C.
            let pgl = Pglist::new();
            pgl.init();
            uvm_pglistalloc(
                PAGE_SIZE,
                Paddr::new(0),
                Paddr::new(usize::MAX),
                Paddr::new(0),
                Paddr::new(0),
                &pgl,
                1,
                UVM_PLA_NOWAIT | UVM_PLA_ZERO,
            )
            .ok()?;
            pmap_map_direct(pgl.first()?)
        };
        // SAFETY: a RAM page just taken from the free segments, through the direct map.
        unsafe { ptr::write_bytes(va.as_usize() as *mut u8, 0, PAGE_SIZE) };
        pted = va.as_usize() as *mut PteDesc;
        npted = PAGE_SIZE / size_of::<PteDesc>();
        PTED_ALLOCATED.fetch_add(1, Ordering::Relaxed);
    }

    npted -= 1;
    KPTED_LEFT.store(npted, Ordering::Relaxed);
    KPTED_NEXT.store(pted.wrapping_add(1) as usize, Ordering::Relaxed);
    // SAFETY: inside the zeroed page; all-zero is a valid, empty pted.
    Some(unsafe { &*pted })
}

/// `pmap_set_l1`: hangs the level-1 table `l1_va` under `va`'s level-0 entry.
pub fn pmap_set_l1(pm: &Pmap, va: usize, l1_va: &Pmapvp1) {
    let Some(l1_pa) = pmap_extract(pmap_kernel(), Vaddr::new(ptr::from_ref(l1_va) as usize)) else {
        #[allow(clippy::panic)] // the C panics here too
        {
            panic!("unable to find vp pa mapping {:p}", l1_va);
        }
    };

    if l1_pa.as_usize() & (Lx_TABLE_ALIGN - 1) != 0 {
        #[allow(clippy::panic)] // the C panics here too
        {
            panic!("misaligned L2 table");
        }
    }

    let pg_entry = vp_lx(l1_pa);

    let idx0 = vp_idx0(va);
    let PmVp::L0(l0) = pm.pm_vp.get() else {
        #[allow(clippy::panic)] // the C dereferences pm_vp.l0 of a three-level pmap
        {
            panic!("pmap_set_l1: three-level pmap");
        }
    };
    // SAFETY: a four-level pmap's `l0` is set when the pmap is made.
    let l0 = unsafe { &*l0 };
    l0.set_vp(idx0, Some(l1_va));
    l0.set_table(idx0, pg_entry);
}

/// `pmap_set_l2`: hangs the level-2 table `l2_va` under `va`'s level-1 entry.
pub fn pmap_set_l2(_pm: &Pmap, va: usize, vp1: &Pmapvp1, l2_va: &Pmapvp2) {
    let Some(l2_pa) = pmap_extract(pmap_kernel(), Vaddr::new(ptr::from_ref(l2_va) as usize)) else {
        #[allow(clippy::panic)] // the C panics here too
        {
            panic!("unable to find vp pa mapping {:p}", l2_va);
        }
    };

    if l2_pa.as_usize() & (Lx_TABLE_ALIGN - 1) != 0 {
        #[allow(clippy::panic)] // the C panics here too
        {
            panic!("misaligned L2 table");
        }
    }

    let pg_entry = vp_lx(l2_pa);

    let idx1 = vp_idx1(va);
    vp1.set_vp(idx1, Some(l2_va));
    vp1.set_table(idx1, pg_entry);
}

/// `pmap_set_l3`: hangs the level-3 table `l3_va` under `va`'s level-2 entry.
pub fn pmap_set_l3(_pm: &Pmap, va: usize, vp2: &Pmapvp2, l3_va: &Pmapvp3) {
    let Some(l3_pa) = pmap_extract(pmap_kernel(), Vaddr::new(ptr::from_ref(l3_va) as usize)) else {
        #[allow(clippy::panic)] // the C panics here too
        {
            panic!("unable to find vp pa mapping {:p}", l3_va);
        }
    };

    if l3_pa.as_usize() & (Lx_TABLE_ALIGN - 1) != 0 {
        #[allow(clippy::panic)] // the C panics here too
        {
            panic!("misaligned L2 table");
        }
    }

    let pg_entry = vp_lx(l3_pa);

    let idx2 = vp_idx2(va);
    vp2.set_vp(idx2, Some(l3_va));
    vp2.set_table(idx2, pg_entry);
}

/// `pmap_growkernel`: makes the kernel's vp tables and pteds reach `maxkvaddr`.
pub fn pmap_growkernel(maxkvaddr: Vaddr) -> Vaddr {
    let maxkvaddr = maxkvaddr.as_usize();
    let mut cur = PMAP_MAXKVADDR.load(Ordering::Relaxed);
    if maxkvaddr <= cur {
        return Vaddr::new(cur);
    }
    let pm = pmap_kernel();
    // The C reaches pm_vp.l1 directly (see the module's deviations).
    let Some(vp1) = pmap_vp1(pm, cur) else {
        #[allow(clippy::panic)] // pmap_bootstrap made it
        {
            panic!("pmap_growkernel: no level-1 table for the kernel range");
        }
    };

    // Not strictly necessary, but we use an interrupt-safe map and uvm asserts that we're
    // at IPL_VM: splvm(), M4.

    'fail: {
        for i in vp_idx1(cur)..=vp_idx1(maxkvaddr - 1) {
            let vp2 = match vp1.vp(i) {
                Some(vp2) => vp2,
                None => {
                    let Some(vp2) = pmap_kvp_alloc() else {
                        break 'fail;
                    };
                    // SAFETY: a fresh, zeroed table that lives for the kernel's life.
                    let vp2 = unsafe { &*vp2.cast::<Pmapvp2>() };
                    pmap_set_l2(pm, i << VP_IDX1_POS, vp1, vp2);
                    vp2
                }
            };

            let lb_idx2 = if i == vp_idx1(cur) { vp_idx2(cur) } else { 0 };
            let ub_idx2 = if i == vp_idx1(maxkvaddr - 1) {
                vp_idx2(maxkvaddr - 1)
            } else {
                VP_IDX2_CNT - 1
            };

            for j in lb_idx2..=ub_idx2 {
                let vp3 = match vp2.vp(j) {
                    Some(vp3) => vp3,
                    None => {
                        let Some(vp3) = pmap_kvp_alloc() else {
                            break 'fail;
                        };
                        // SAFETY: as above.
                        let vp3 = unsafe { &*vp3.cast::<Pmapvp3>() };
                        pmap_set_l3(pm, j << VP_IDX2_POS, vp2, vp3);
                        vp3
                    }
                };

                for k in 0..VP_IDX3_CNT {
                    if vp3.vp(k).is_none() {
                        let Some(pted) = pmap_kpted_alloc() else {
                            break 'fail;
                        };
                        vp3.set_vp(k, Some(pted));
                        cur += PAGE_SIZE;
                    }
                }
            }
        }
        kassert!(cur >= maxkvaddr);
    }

    PMAP_MAXKVADDR.store(cur, Ordering::Relaxed);
    // splx(s): M4.

    Vaddr::new(cur)
}

/// `pmap_bootstrap`: adopts the bootloader's kernel page tables into the kernel pmap (see the
/// module's deviations) and reserves `vmmap`; returns the first free kernel virtual address.
///
/// # Safety
///
/// Call once, on the boot CPU, after `initarm` has set the direct map's base and end and
/// loaded the physical memory into `uvm`, with the MMU on and `TTBR1_EL1` holding the tables
/// the kernel runs on.
pub unsafe fn pmap_bootstrap(_ram_start: Paddr, _ram_end: Paddr) -> Vaddr {
    // pmap_setup_avail, pmap_remove_avail: the memory map is the bootloader's.

    let (tcr, ttbr1): (u64, u64);
    // SAFETY: reading system registers has no side effects.
    unsafe {
        asm!("mrs {}, tcr_el1", out(reg) tcr, options(nomem, nostack, preserves_flags));
        asm!("mrs {}, ttbr1_el1", out(reg) ttbr1, options(nomem, nostack, preserves_flags));
    }
    if (tcr >> TCR_T1SZ_SHIFT) & 0x3f != T1SZ_4LEVEL {
        #[allow(clippy::panic)] // the bootloader broke the protocol's contract
        {
            panic!("pmap_bootstrap: TTBR1_EL1 walk is not 4-level (TCR_EL1.T1SZ != 16)");
        }
    }
    let l0_pa = Paddr::new((ttbr1 & TTBR_BADDR_MASK) as usize);

    // KERNEL IS ASSUMED TO BE 39 bits (or less) in the C; here the bootloader's four-level
    // tables are copied into vp tables (see the module's deviations).
    let Some(vp0) = pmap_kvp_alloc() else {
        #[allow(clippy::panic)] // what pmap_steal_avail does when it cannot allocate
        {
            panic!("pmap_bootstrap: out of memory");
        }
    };
    // SAFETY: a fresh, zeroed table that lives for the kernel's life.
    let vp0 = unsafe { &*vp0 };
    let src_l0 = pmap_direct_map(l0_pa).as_usize() as *const u64;
    for i in 0..TABLE_ENTRIES {
        // SAFETY: the bootloader's level-0 table is RAM the direct map covers.
        vp0.set_table(i, unsafe { ptr::read_volatile(src_l0.add(i)) });
    }

    let kslot = vp_idx0(VM_MIN_KERNEL_ADDRESS);
    let l1_desc = vp0.table(kslot);
    if l1_desc & Lx_TYPE_MASK != Lx_TYPE_PT {
        #[allow(clippy::panic)] // the kernel image is mapped under that slot
        {
            panic!("pmap_bootstrap: level-0 entry {} is not a table", kslot);
        }
    }
    let Some(vp1) = pmap_kvp_alloc() else {
        #[allow(clippy::panic)] // as above
        {
            panic!("pmap_bootstrap: out of memory");
        }
    };
    // SAFETY: as for `vp0`.
    let vp1 = unsafe { &*vp1.cast::<Pmapvp1>() };
    let src_l1 =
        pmap_direct_map(Paddr::new((l1_desc & PTE_RPGN) as usize)).as_usize() as *const u64;
    for i in 0..TABLE_ENTRIES {
        // SAFETY: as for the level-0 table.
        vp1.set_table(i, unsafe { ptr::read_volatile(src_l1.add(i)) });
    }
    let vp1_pa = pmap_direct_unmap(Vaddr::new(ptr::from_ref(vp1) as usize));
    vp0.set_vp(kslot, Some(vp1));
    vp0.set_table(kslot, vp_lx(vp1_pa));

    let pm = pmap_kernel();
    pm.pm_vp.set(PmVp::L0(ptr::from_ref(vp0).cast_mut()));
    pm.have_4_level_pt.set(true);
    pm.pm_privileged.set(true);
    pm.pm_active.store(1, Ordering::Relaxed);
    pm.pm_guarded.set(ATTR_GP);
    pm.pm_asid.set(0);
    pm.pm_refs.set(1);
    // pmap_tramp and the ASID bitmap (ASID 0 in use): M6.

    // MAIR_EL1 indices 2, 3 and 4 (see include/pte.rs).
    let mair: u64;
    // SAFETY: the fields written are the ones no running mapping uses (index 2 was set by
    // initarm to the same value); `isb` makes the new attributes visible.
    unsafe {
        asm!("mrs {}, mair_el1", out(reg) mair, options(nomem, nostack, preserves_flags));
        let keep = !(mair_attr(0xff, 2) | mair_attr(0xff, 3) | mair_attr(0xff, 4));
        let mair = (mair & keep)
            | mair_attr(MAIR_DEV_NGNRNE, 2)
            | mair_attr(MAIR_CI, 3)
            | mair_attr(MAIR_WT, 4);
        asm!("msr mair_el1, {}", "isb", in(reg) mair, options(nostack, preserves_flags));
    }

    // Switch to the copy: the same translations, so nothing running changes.
    let vp0_pa = pmap_direct_unmap(Vaddr::new(ptr::from_ref(vp0) as usize)).as_usize() as u64;
    // SAFETY: `vp0` holds a copy of the live level-0 table; the writes are complete before
    // the register changes (`dsb ishst`), and the TLB is flushed after.
    unsafe {
        asm!(
            "dsb ishst",
            "msr ttbr1_el1, {}",
            "isb",
            in(reg) vp0_pa | (ttbr1 & !TTBR_BADDR_MASK),
            options(nostack, preserves_flags)
        );
    }
    cpu_tlb_flush();

    // The empty lower-half table the C keeps in pm_pt0pa (pmap_init loads it into TTBR0).
    let Some(pt0pa) = uvm_page_physsteal(1) else {
        #[allow(clippy::panic)] // as above
        {
            panic!("pmap_bootstrap: out of memory");
        }
    };
    // SAFETY: a RAM page just taken from the free segments, through the direct map.
    unsafe { ptr::write_bytes(pmap_direct_map(pt0pa).as_usize() as *mut u8, 0, PAGE_SIZE) };
    pm.pm_pt0pa.set(pt0pa.as_usize() as u64);

    // pmap_avail_fixup, pmap_map_stolen, switch_mmu_kernel: the bootloader's tables stay.
    // curcpu()->ci_curpm (M5) and the ASID width from id_aa64mmfr0_el1 (M6).

    let vstart = VM_MIN_KERNEL_ADDRESS;
    VMMAP.store(vstart, Ordering::Relaxed);
    let vstart = vstart + PAGE_SIZE;
    VIRTUAL_AVAIL.store(vstart, Ordering::Relaxed);

    Vaddr::new(vstart)
}

/// `pmap_init`: the pmap and pted pools, then `TTBR0_EL1` is pointed at the kernel's empty
/// lower-half table (the bootstrap device map goes away: the console is remapped into the
/// kernel half first) and `TCR_EL1.T0SZ` set for the user address space width.
pub fn pmap_init() {
    pool_init(
        &PMAP_PMAP_POOL,
        size_of::<Pmap>(),
        0,
        IPL_VM,
        PR_WAITOK,
        "pmap",
        None,
    );
    pool_init(
        &PMAP_PTED_POOL,
        size_of::<PteDesc>(),
        0,
        IPL_VM,
        PR_WAITOK,
        "pted",
        None,
    );
    // pmap_vp_pool: the two-page allocator serves (see the module's deviations).

    PMAP_INITIALIZED.store(true, Ordering::Relaxed);

    // The bus space now maps through the kernel half: move the console there before the
    // identity map in TTBR0 disappears (see the module's deviations).
    pluartcn_remap();

    // WRITE_SPECIALREG(ttbr0_el1, pmap_kernel()->pm_pt0pa); and the C's locore
    // TCR_EL1.T0SZ for USER_SPACE_BITS, which the bootstrap device map (4-level) postponed.
    let t0sz = 64 - u64::from(USER_SPACE_BITS);
    // SAFETY: the kernel runs entirely in the upper half (TTBR1): the lower half is switched
    // to an empty table and the walk resized under it; the TLB is flushed afterwards.
    unsafe {
        let mut tcr: u64;
        asm!("mrs {}, tcr_el1", out(reg) tcr, options(nomem, nostack, preserves_flags));
        tcr = (tcr & !0x3f) | t0sz;
        asm!(
            "msr ttbr0_el1, {pt0}",
            "isb",
            "msr tcr_el1, {tcr}",
            "isb",
            "tlbi vmalle1",
            "dsb ish",
            "isb",
            pt0 = in(reg) pmap_kernel().pm_pt0pa.get(),
            tcr = in(reg) tcr,
            options(nostack, preserves_flags)
        );
    }
    curcpu().ci_curpm.set(pmap_kernel());
}

/// `pmap_fill_pte`: computes the pted of a mapping.
pub fn pmap_fill_pte(
    pm: &Pmap,
    va: usize,
    pa: usize,
    pted: &PteDesc,
    prot: VmProt,
    flags: i32,
    cache: i32,
) {
    pted.pted_va.set(va);
    pted.pted_pmap.set(ptr::from_ref(pm));

    match cache {
        PMAP_CACHE_WB
        | PMAP_CACHE_WT
        | PMAP_CACHE_CI
        | PMAP_CACHE_DEV_NGNRNE
        | PMAP_CACHE_DEV_NGNRE => {}
        _ => {
            #[allow(clippy::panic)] // the C panics here too
            {
                panic!("pmap_fill_pte: invalid cache mode");
            }
        }
    }
    pted.pted_va.set(pted.pted_va.get() | cache as usize);

    pted.pted_va
        .set(pted.pted_va.get() | (prot & PROT_MASK) as usize);

    if flags & PMAP_WIRED != 0 {
        pted.pted_va
            .set(pted.pted_va.get() | PTED_VA_WIRED_M as usize);
        pm.pm_stats
            .wired_count
            .set(pm.pm_stats.wired_count.get() + 1);
    }

    pted.pted_pte.set(pa as u64 & PTE_RPGN);
    pted.pted_pte
        .set(pted.pted_pte.get() | (flags & PROT_MASK) as u64);
}

/// `pmap_pte_insert`: writes a pted's PTE into its level-3 table.
pub fn pmap_pte_insert(pted: &PteDesc) {
    let pm = pted_pmap(pted);
    let (_, pl3) = pmap_vp_lookup(pm, pted.pted_va.get());
    let Some(pl3) = pl3 else {
        #[allow(clippy::panic)] // the C panics here too
        {
            panic!(
                "pmap_pte_insert: have a pted, but missing a vp for {:#x} va pmap {:p}",
                pted.pted_va.get(),
                pm
            );
        }
    };

    pmap_pte_update(pted, pl3);
}

/// `pmap_pte_update`: computes the hardware PTE of a pted and stores it at `pl3`.
pub fn pmap_pte_update(pted: &PteDesc, pl3: (&Pmapvp3, usize)) {
    let pm = pted_pmap(pted);
    let mut attr = ATTR_nG;

    // see mair in locore.S (and include/pte.rs for the indices)
    match pted.pted_va.get() as i32 & PMAP_CACHE_BITS {
        PMAP_CACHE_WB => {
            // inner and outer writeback
            attr |= attr_idx(PTE_ATTR_WB);
            attr |= attr_sh(SH_INNER);
        }
        PMAP_CACHE_WT => {
            // inner and outer writethrough
            attr |= attr_idx(PTE_ATTR_WT);
            attr |= attr_sh(SH_INNER);
        }
        PMAP_CACHE_CI => {
            attr |= attr_idx(PTE_ATTR_CI);
            attr |= attr_sh(SH_INNER);
        }
        PMAP_CACHE_DEV_NGNRNE => {
            attr |= attr_idx(PTE_ATTR_DEV_NGNRNE);
            attr |= attr_sh(SH_INNER);
        }
        PMAP_CACHE_DEV_NGNRE => {
            attr |= attr_idx(PTE_ATTR_DEV_NGNRE);
            attr |= attr_sh(SH_INNER);
        }
        _ => {
            #[allow(clippy::panic)] // the C panics here too
            {
                panic!("pmap_pte_update: invalid cache mode");
            }
        }
    }

    let prot = (pted.pted_pte.get() & PROT_MASK as u64) as usize;
    let mut access_bits = if pm.pm_privileged.get() {
        AP_BITS_KERN[prot]
    } else {
        AP_BITS_USER[prot]
    };

    // !SMALL_KERNEL
    access_bits |= pm.pm_guarded.get();

    let pte = (pted.pted_pte.get() & PTE_RPGN) | attr | access_bits | L3_P;
    pl3.0
        .set_table(pl3.1, if access_bits != 0 { pte } else { 0 });
}

/// `pmap_pte_remove`: clears a pted's PTE, and the pted's slot too when `remove_pted`.
pub fn pmap_pte_remove(pted: &PteDesc, remove_pted: bool) {
    let pm = pted_pmap(pted);
    let va = pted.pted_va.get();

    let Some(vp1) = pmap_vp1(pm, va) else {
        #[allow(clippy::panic)] // the C panics here too
        {
            panic!(
                "have a pted, but missing the l1 for {:#x} va pmap {:p}",
                va, pm
            );
        }
    };
    let Some(vp2) = vp1.vp(vp_idx1(va)) else {
        #[allow(clippy::panic)] // the C panics here too
        {
            panic!(
                "have a pted, but missing the l2 for {:#x} va pmap {:p}",
                va, pm
            );
        }
    };
    let Some(vp3) = vp2.vp(vp_idx2(va)) else {
        #[allow(clippy::panic)] // the C panics here too
        {
            panic!(
                "have a pted, but missing the l3 for {:#x} va pmap {:p}",
                va, pm
            );
        }
    };
    vp3.set_table(vp_idx3(va), 0);
    if remove_pted {
        vp3.set_vp(vp_idx3(va), None);
    }
}

/// `_pmap_kenter_pa`: enter a kernel mapping for the given page. Kernel mappings have a
/// larger set of prerequisites than normal mappings: no memory should be allocated to create a
/// kernel mapping; a vp mapping should already exist, even if invalid (see 1); all vp tree
/// mappings should already exist (see 1).
fn _pmap_kenter_pa(va: usize, pa: usize, prot: VmProt, flags: i32, cache: i32) {
    let pm = pmap_kernel();
    let (pted, _) = pmap_vp_lookup(pm, va);
    let Some(pted) = pted else {
        #[allow(clippy::panic)] // the C panics here too
        {
            panic!(
                "pted not preallocated in pmap_kernel() va {:#x} pa {:#x}",
                va, pa
            );
        }
    };

    if pted_valid(pted) {
        pmap_kremove_pg(va); // pted is reused
    }

    pm.pm_stats
        .resident_count
        .set(pm.pm_stats.resident_count.get() + 1);

    let flags = flags | PMAP_WIRED; // kernel mappings are always wired.
    // Calculate PTE
    pmap_fill_pte(pm, va, pa, pted, prot, flags, cache);

    // Insert into table
    pmap_pte_insert(pted);
    ttlb_flush(pm, va & !PAGE_MASK);

    let pg = PHYS_TO_VM_PAGE(Paddr::new((pted.pted_pte.get() & PTE_RPGN) as usize));
    if pg.is_some() && (cache == PMAP_CACHE_CI || cache == PMAP_CACHE_DEV_NGNRNE) {
        let _ = unported!("cpu_idcache_wbinv_range (M4)");
    }
}

/// `pmap_kenter_pa`: enter a kernel mapping, write-back unless `pa` carries `PMAP_NOCACHE`
/// or `PMAP_DEVICE`.
///
/// # Safety
///
/// `va` must be kernel virtual space the caller owns, below `pmap_maxkvaddr`, and `pa` a page
/// it may map there.
pub unsafe fn pmap_kenter_pa(va: Vaddr, pa: Paddr, prot: VmProt) {
    let pa = pa.as_usize();
    let mut cache = PMAP_CACHE_WB;

    if pa & PMAP_NOCACHE as usize != 0 {
        cache = PMAP_CACHE_CI;
    }
    if pa & PMAP_DEVICE as usize != 0 {
        cache = PMAP_CACHE_DEV_NGNRNE;
    }

    _pmap_kenter_pa(va.as_usize(), pa, prot, prot, cache);
}

/// `pmap_kenter_cache`: enter a kernel mapping with the given cache mode.
///
/// # Safety
///
/// As for [`pmap_kenter_pa`].
pub unsafe fn pmap_kenter_cache(va: Vaddr, pa: Paddr, prot: VmProt, cacheable: i32) {
    _pmap_kenter_pa(va.as_usize(), pa.as_usize(), prot, prot, cacheable);
}

/// `pmap_kremove_pg`: remove kernel (`pmap_kernel()`) mapping, one page.
pub fn pmap_kremove_pg(va: usize) {
    let pm = pmap_kernel();
    let (pted, _) = pmap_vp_lookup(pm, va);
    let Some(pted) = pted else {
        return;
    };

    if !pted_valid(pted) {
        return; // not mapped
    }

    // splvm(): M4.

    pm.pm_stats
        .resident_count
        .set(pm.pm_stats.resident_count.get() - 1);

    pmap_pte_remove(pted, false);
    ttlb_flush(pm, pted.pted_va.get() & !PAGE_MASK);

    if pted_managed(pted) {
        pmap_remove_pv(pted);
    }

    if pted_wired(pted) {
        pm.pm_stats
            .wired_count
            .set(pm.pm_stats.wired_count.get() - 1);
    }

    // invalidate pted;
    pted.pted_pte.set(0);
    pted.pted_va.set(0);

    // splx(s): M4.
}

/// `pmap_kremove`: remove kernel (`pmap_kernel()`) mappings.
///
/// # Safety
///
/// The range must have been mapped by [`pmap_kenter_pa`] and nothing may use it afterwards.
pub unsafe fn pmap_kremove(va: Vaddr, len: Vsize) {
    let mut va = va.as_usize();
    for _ in 0..len.as_usize() >> PAGE_SHIFT {
        pmap_kremove_pg(va);
        va += PAGE_SIZE;
    }
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

/// `pmap_extract`: get the physical page address for the given pmap/virtual address.
pub fn pmap_extract(pm: &Pmap, va: Vaddr) -> Option<Paddr> {
    if ptr::eq(pm, pmap_kernel()) && pmap_direct_mapped(va) {
        return Some(pmap_direct_unmap(va));
    }

    // pmap_lock(pm): M5.
    let (pted, _) = pmap_vp_lookup(pm, va.as_usize());
    let pted = pted.filter(|pted| pted_valid(pted))?;
    Some(Paddr::new(
        (pted.pted_pte.get() & PTE_RPGN) as usize | (va.as_usize() & PAGE_MASK),
    ))
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

const _: () = {
    assert!(size_of::<Pmapvp0>() == size_of::<Pmapvp1>());
    assert!(size_of::<Pmapvp0>() == size_of::<Pmapvp2>());
    assert!(size_of::<Pmapvp0>() == size_of::<Pmapvp3>());
    assert!(size_of::<Pmapvp0>() == 2 * PAGE_SIZE);
};

/// `pmap_find_asid`: a free pair of ASIDs (even: the kernel's view, odd: the user's), from
/// 2 up (0 is the kernel pmap's).
fn pmap_find_asid() -> Option<usize> {
    (2..PMAP_NASID).step_by(2).find(|&asid| {
        let bit = asid & (32 - 1);
        PMAP_ASID[asid / 32].load(Ordering::Relaxed) & (3u32 << bit) == 0
    })
}

/// `pmap_allocate_asid`: gives `pm` an ASID of the current generation.
pub fn pmap_allocate_asid(pm: &Pmap) {
    // mtx_enter(&pmap_asid_mtx): M5 (one CPU).
    let Some(asid) = pmap_find_asid() else {
        // We have no free ASIDs. Do a rollover to clear all inactive ASIDs and pick a fresh
        // one: pmap_rollover_asid (M7).
        #[allow(clippy::panic)] // the rollover is not here yet
        {
            panic!("pmap_allocate_asid: out of ASIDs (pmap_rollover_asid, M7)");
        }
    };
    kassert!(asid > 0 && asid < PMAP_NASID);
    let bit = asid & (32 - 1);
    PMAP_ASID[asid / 32].fetch_or(3u32 << bit, Ordering::Relaxed);
    pm.pm_asid
        .set(asid as u64 | PMAP_ASID_GEN.load(Ordering::Relaxed));
}

/// `pmap_free_asid`: drops `pm`'s TLB entries and returns its ASIDs.
pub fn pmap_free_asid(pm: &Pmap) {
    kassert!(!ptr::eq(pm, curcpu().ci_curpm.get()));
    cpu_tlb_flush_asid_all(pm.pm_asid.get() << 48);
    cpu_tlb_flush_asid_all((pm.pm_asid.get() | ASID_USER) << 48);

    if pm.pm_asid.get() & !PMAP_ASID_MASK == PMAP_ASID_GEN.load(Ordering::Relaxed) {
        let asid = (pm.pm_asid.get() & PMAP_ASID_MASK) as usize;
        let bit = asid & (32 - 1);
        PMAP_ASID[asid / 32].fetch_and(!(3u32 << bit), Ordering::Relaxed);
    }
}

/// `pmap_enter_pv`: puts the mapping on its page's pv list.
fn pmap_enter_pv(pted: &PteDesc, pg: &VmPage) {
    // mtx_enter(&pg->mdpage.pv_mtx): M5.
    // SAFETY: a pted on no list yet; the page's list is the pmap's (one CPU).
    unsafe { pg.mdpage.pv_list.insert_head(pted) };
    pted.pted_va
        .set(pted.pted_va.get() | PTED_VA_MANAGED_M as usize);
}

/// `pmap_remove_pv`: takes the mapping off its page's pv list.
fn pmap_remove_pv(pted: &PteDesc) {
    // pg = PHYS_TO_VM_PAGE(pted->pted_pte & PTE_RPGN); mtx_enter(&pg->mdpage.pv_mtx): M5.
    // SAFETY: a managed pted is on its page's list.
    unsafe { ListHead::<PvList>::remove(pted) };
}

/// `pmap_icache_sync_page`: makes the page's contents visible to instruction fetch, through
/// the direct map (the C maps it at `zero_page`).
fn pmap_icache_sync_page(_pm: &Pmap, pa: usize) {
    cpu_icache_sync_range(pmap_direct_map(Paddr::new(pa)).as_usize(), PAGE_SIZE);
}

/// A pted for `pm`: the kernel's from its page of pteds, a user pmap's from the pool.
fn pmap_pted_alloc(pm: &Pmap) -> Option<&'static PteDesc> {
    if ptr::eq(pm, pmap_kernel()) {
        return pmap_kpted_alloc();
    }
    let mem = pool_get(&PMAP_PTED_POOL, PR_NOWAIT | PR_ZERO)?;
    // SAFETY: a zeroed pool item of `size_of::<PteDesc>()` bytes, suitably aligned; all-zero
    // is a valid, empty pted (a null list entry, no pmap, no address).
    Some(unsafe { mem.cast::<PteDesc>().as_ref() })
}

/// `pool_put(&pmap_pted_pool, pted)`.
fn pmap_pted_free(pted: &PteDesc) {
    pool_put(&PMAP_PTED_POOL, NonNull::from(pted).cast::<u8>());
}

/// `pmap_enter`: enter a mapping into a pmap.
pub fn pmap_enter(pm: &Pmap, va: Vaddr, pa: Paddr, prot: VmProt, flags: i32) -> Result<(), Errno> {
    let va = va.as_usize();
    let pa = pa.as_usize();
    let mut cache = PMAP_CACHE_WB;

    if pa & PMAP_NOCACHE as usize != 0 {
        cache = PMAP_CACHE_CI;
    }
    if pa & PMAP_DEVICE as usize != 0 {
        cache = PMAP_CACHE_DEV_NGNRNE;
    }
    let pg = PHYS_TO_VM_PAGE(Paddr::new(pa));

    // pmap_lock(pm): M5.
    let (mut pted, _) = pmap_vp_lookup(pm, va);
    if let Some(old) = pted.filter(|pted| pted_valid(pted)) {
        if old.pted_pte.get() & PTE_RPGN == pa as u64 & PTE_RPGN
            && old.pted_va.get() & PROT_MASK as usize == (prot & PROT_MASK) as usize
            && old.pted_va.get() & PMAP_CACHE_BITS as usize == cache as usize
        {
            return Ok(());
        }
        pmap_remove_pted(pm, old);
        // we lost our pted if it was user
        if !ptr::eq(pm, pmap_kernel()) {
            pted = pmap_vp_lookup(pm, va).0;
        }
    }

    pm.pm_stats
        .resident_count
        .set(pm.pm_stats.resident_count.get() + 1);

    // Do not have pted for this, get one and put it in VP
    let pted = match pted {
        Some(pted) => pted,
        None => {
            let Some(pted) = pmap_pted_alloc(pm) else {
                if flags & PMAP_CANFAIL == 0 {
                    #[allow(clippy::panic)] // the C panics here too
                    {
                        panic!("pmap_enter: failed to allocate pted");
                    }
                }
                return Err(Errno::ENOMEM);
            };
            if pmap_vp_enter(pm, va, pted, flags).is_err() {
                if flags & PMAP_CANFAIL == 0 {
                    #[allow(clippy::panic)] // the C panics here too
                    {
                        panic!("pmap_enter: failed to allocate L2/L3");
                    }
                }
                if !ptr::eq(pm, pmap_kernel()) {
                    pmap_pted_free(pted);
                }
                return Err(Errno::ENOMEM);
            }
            pted
        }
    };

    // If it should be enabled _right now_, we can skip doing ref/mod emulation. Any access
    // includes reference, modified only by write.
    if let Some(pg) = pg
        && (flags & PROT_MASK != 0 || pg.flags() & PG_PMAP_REF != 0)
    {
        pg.set_bits(PG_PMAP_REF);
        if prot & PROT_WRITE != 0 && flags & PROT_WRITE != 0 {
            pg.set_bits(PG_PMAP_MOD);
            pg.clear_bits(PG_PMAP_EXE);
        }
    }

    pmap_fill_pte(pm, va, pa, pted, prot, flags, cache);

    if let Some(pg) = pg {
        pmap_enter_pv(pted, pg); // only managed mem
    }

    if let Some(pg) = pg
        && flags & PROT_EXEC != 0
    {
        if pg.flags() & PG_PMAP_EXE == 0 {
            pmap_icache_sync_page(pm, pa);
        }
        pg.set_bits(PG_PMAP_EXE);
    }

    // Insert into table, if this mapping said it needed to be mapped now.
    if flags & (PROT_MASK | PMAP_WIRED) != 0 {
        pmap_pte_insert(pted);
        ttlb_flush(pm, va & !PAGE_MASK);
    }

    // pmap_unlock(pm): M5.
    Ok(())
}

/// `pmap_remove`: remove the given range of mapping entries.
pub fn pmap_remove(pm: &Pmap, sva: Vaddr, eva: Vaddr) {
    // pmap_lock(pm): M5.
    let mut va = sva.as_usize();
    while va < eva.as_usize() {
        let (pted, _) = pmap_vp_lookup(pm, va);
        if let Some(pted) = pted {
            if pted_wired(pted) {
                pm.pm_stats
                    .wired_count
                    .set(pm.pm_stats.wired_count.get() - 1);
                pted.pted_va
                    .set(pted.pted_va.get() & !(PTED_VA_WIRED_M as usize));
            }

            if pted_valid(pted) {
                pmap_remove_pted(pm, pted);
            }
        }
        va += PAGE_SIZE;
    }
    // pmap_unlock(pm): M5.
}

/// `pmap_unwire`: clears the wired bit of the mapping at `va`.
pub fn pmap_unwire(pm: &Pmap, va: Vaddr) {
    // pmap_lock(pm): M5.
    let (pted, _) = pmap_vp_lookup(pm, va.as_usize());
    if let Some(pted) = pted.filter(|pted| pted_wired(pted)) {
        pm.pm_stats
            .wired_count
            .set(pm.pm_stats.wired_count.get() - 1);
        pted.pted_va
            .set(pted.pted_va.get() & !(PTED_VA_WIRED_M as usize));
    }
    // pmap_unlock(pm): M5.
}

/// `pmap_remove_pted`: remove a single mapping, notice that this code is O(1).
pub fn pmap_remove_pted(pm: &Pmap, pted: &PteDesc) {
    pm.pm_stats
        .resident_count
        .set(pm.pm_stats.resident_count.get() - 1);

    if pted_wired(pted) {
        pm.pm_stats
            .wired_count
            .set(pm.pm_stats.wired_count.get() - 1);
        pted.pted_va
            .set(pted.pted_va.get() & !(PTED_VA_WIRED_M as usize));
    }

    pmap_pte_remove(pted, !ptr::eq(pm, pmap_kernel()));
    ttlb_flush(pm, pted.pted_va.get() & !PAGE_MASK);

    if pted_managed(pted) {
        pmap_remove_pv(pted);
    }

    pted.pted_pte.set(0);
    pted.pted_va.set(0);

    if !ptr::eq(pm, pmap_kernel()) {
        pmap_pted_free(pted);
    }
}

/// Returns the two pages of a vp table (see the module's deviations).
fn pmap_vp_free(vp: usize) {
    let pgl = Pglist::new();
    pgl.init();
    for off in [0, PAGE_SIZE] {
        if let Some(pg) = pmap_unmap_direct(Vaddr::new(vp + off)) {
            // SAFETY: a page of ours, on no queue (it came out of uvm_pglistalloc).
            unsafe { pgl.insert_tail(pg) };
        }
    }
    uvm_pglistfree(&pgl);
}

/// `pmap_pinit`: allocates a full L0/L1 table for `pm` and takes the first reference.
pub fn pmap_pinit(pm: &Pmap) {
    // Allocate a full L0/L1 table: user pmaps are three-level (USER_SPACE_BITS 39).
    let l0va = match pm.pm_vp.get() {
        PmVp::L0(_) => {
            #[allow(clippy::panic)] // user pmaps are never four-level here
            {
                panic!("pmap_pinit: four-level user pmap");
            }
        }
        PmVp::L1(_) => {
            let Some(l1) = pmap_kvp_alloc() else {
                #[allow(clippy::panic)] // the C loops on pool_get(PR_WAITOK) instead
                {
                    panic!("pmap_pinit: out of memory for the L1 table");
                }
            };
            pm.pm_vp.set(PmVp::L1(l1.cast::<Pmapvp1>()));
            l1 as usize // top level is l1
        }
    };

    pm.pm_pt0pa
        .set(pmap_direct_unmap(Vaddr::new(l0va)).as_usize() as u64);

    pmap_reference(pm);
}

/// `pmap_create`: create and return a physical map.
pub fn pmap_create() -> &'static Pmap {
    let Some(mem) = pool_get(&PMAP_PMAP_POOL, PR_WAITOK | PR_ZERO) else {
        #[allow(clippy::panic)] // pool_get(PR_WAITOK) never fails in C
        {
            panic!("pmap_create: pmap_pmap_pool is empty");
        }
    };
    let pp = mem.cast::<Pmap>();
    // SAFETY: a fresh, suitably aligned pool item of `size_of::<Pmap>()` bytes, written once
    // before anything else sees it; it lives until `pmap_destroy` returns it.
    let pmap: &'static Pmap = unsafe {
        pp.as_ptr().write(Pmap::new());
        pp.as_ref()
    };

    // mtx_init(&pmap->pm_mtx, IPL_VM): M5.
    pmap_pinit(pmap);
    // pool_setlowat(&pmap_vp_pool, 20): no vp pool.
    pmap
}

/// `pmap_reference`: add a reference to a given pmap.
pub fn pmap_reference(pm: &Pmap) {
    pm.pm_refs.set(pm.pm_refs.get() + 1);
}

/// `pmap_destroy`: retire the given pmap from service. Should only be called if the map
/// contains no valid mappings.
pub fn pmap_destroy(pm: &'static Pmap) {
    let refs = pm.pm_refs.get() - 1;
    pm.pm_refs.set(refs);
    if refs > 0 {
        return;
    }

    // reference count is zero, free pmap resources and free pmap.
    pmap_release(pm);
    pmap_free_asid(pm);
    pool_put(&PMAP_PMAP_POOL, NonNull::from(pm).cast::<u8>());
}

/// `pmap_release`: release any resources held by the given physical map. Called when a pmap
/// initialized by `pmap_pinit` is being released.
pub fn pmap_release(pm: &Pmap) {
    pmap_vp_destroy(pm);
}

/// `pmap_vp_destroy_l2_l3`: frees the level-2 and level-3 tables and the pteds under `vp1`.
fn pmap_vp_destroy_l2_l3(_pm: &Pmap, vp1: &Pmapvp1) {
    for j in 0..VP_IDX1_CNT {
        let Some(vp2) = vp1.vp(j) else {
            continue;
        };
        vp1.set_vp(j, None);

        for k in 0..VP_IDX2_CNT {
            let Some(vp3) = vp2.vp(k) else {
                continue;
            };
            vp2.set_vp(k, None);

            for l in 0..VP_IDX3_CNT {
                let Some(pted) = vp3.vp(l) else {
                    continue;
                };
                vp3.set_vp(l, None);

                pmap_pted_free(pted);
            }
            pmap_vp_free(ptr::from_ref(vp3) as usize);
        }
        pmap_vp_free(ptr::from_ref(vp2) as usize);
    }
}

/// `pmap_vp_destroy`: frees every table of `pm`.
fn pmap_vp_destroy(pm: &Pmap) {
    match pm.pm_vp.get() {
        PmVp::L1(l1) => {
            // SAFETY: a non-null `l1` is the table `pmap_pinit` made, alive until here.
            if let Some(vp1) = unsafe { l1.as_ref() } {
                pmap_vp_destroy_l2_l3(pm, vp1);
                pmap_vp_free(l1 as usize);
            }
            pm.pm_vp.set(PmVp::L1(ptr::null_mut()));
        }
        PmVp::L0(l0) => {
            // SAFETY: as above.
            if let Some(vp0) = unsafe { l0.as_ref() } {
                for i in 0..VP_IDX0_CNT {
                    let Some(vp1) = vp0.vp(i) else {
                        continue;
                    };
                    vp0.set_vp(i, None);

                    pmap_vp_destroy_l2_l3(pm, vp1);
                    pmap_vp_free(ptr::from_ref(vp1) as usize);
                }
                pmap_vp_free(l0 as usize);
            }
            pm.pm_vp.set(PmVp::L0(ptr::null_mut()));
        }
    }
}

/// `pmap_remove_holes`: nothing on arm64.
pub fn pmap_remove_holes(_vm: &Vmspace) {}

/// `pmap_proc_iflush`: makes instructions written to `[va, va+len)` of `pr`'s address space
/// visible to instruction fetch, through the direct-map alias of each page (the C syncs the
/// user range directly for the current process and aliases at `zero_page` otherwise).
pub fn pmap_proc_iflush(pr: &Process, va: Vaddr, len: Vsize) {
    let pm = pr.vmspace().vm_map.pmap();
    let mut va = va.as_usize();
    let mut len = len.as_usize();

    while len > 0 {
        // add one to always round up to the next page
        let mut clen = round_page(va + 1) - va;
        if clen > len {
            clen = len;
        }

        let off = va - trunc_page(va);
        if let Some(pa) = pmap_extract(pm, Vaddr::new(trunc_page(va))) {
            cpu_icache_sync_range(pmap_direct_map(pa).as_usize() + off, clen);
        }

        len -= clen;
        va += clen;
    }
}
