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
 * Copyright (c) 2001 Wasabi Systems, Inc.
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

/* <CODE> */
//! amd64 `<machine/pmap.h>`: the physical map's types and constants.
//!
//! Upstream: sys/arch/amd64/include/pmap.h @ 3ce1f3f79392
//!
//! Status: `wip`. Milestone M3 ports `struct pmap` (its bootstrap subset), `struct pv_entry`,
//! `struct vm_page_md`, the PML4 slot layout, the page-table geometry constants, the recursive
//! mapping (`PTE_BASE`, `L*_BASE`, `pl*_i`, `kvtopte`), the PCID and pmap-type constants and
//! the `PG_PMAP_*` bits. M7a adds the inline frontends `pmap_page_protect`, `pmap_protect`
//! and the attribute macros (`pmap_clear_modify`, `pmap_is_modified`, ...).
//!
//! ## Deviations
//! - `pm_obj` has no pager (`pmap_pager`, M6): the objects are built with `UvmObject::new`.

use core::cell::Cell;
use core::ptr;

use crate::arch::amd64::amd64::pmap::{
    pmap_clear_attrs, pmap_page_remove, pmap_remove, pmap_test_attrs, pmap_write_protect,
};
use crate::arch::amd64::include::param::{PAGE_MASK, PAGE_SIZE};
use crate::arch::amd64::include::pte::{
    L1_FRAME, L1_MASK, L1_SHIFT, L2_FRAME, L2_MASK, L2_SHIFT, L3_FRAME, L3_MASK, L3_SHIFT,
    L4_FRAME, L4_MASK, L4_SHIFT, NBPD_L1, NBPD_L2, NBPD_L3, NBPD_L4, PG_AVAIL1, PG_AVAIL2, PG_M,
    PG_RW, PG_U, PdEntry, PtEntry,
};
use crate::kassert;
use crate::machine::intr::IPL_VM;
use crate::sys::mman::{PROT_NONE, PROT_READ};
use crate::sys::mutex::Mutex;
use crate::sys::types::{Paddr, Vaddr};
use crate::uvm::uvm_extern::VmProt;
use crate::uvm::uvm_object::UvmObject;
use crate::uvm::uvm_page::{PG_PMAP0, PG_PMAP1, PG_PMAP2, VmPage};
use crate::uvm::uvm_pmap::{PMAP_MD0, PMAP_MD1, PmapStatistics};

/// `VA_SIGN_MASK`: the sign-extension bits of a canonical address.
pub const VA_SIGN_MASK: usize = 0xffff_0000_0000_0000;

/// `VA_SIGN_NEG(va)`: sign-extends a kernel virtual address.
pub const fn va_sign_neg(va: usize) -> usize {
    va | VA_SIGN_MASK
}

/// `VA_SIGN_POS(va)`: strips the sign extension.
pub const fn va_sign_pos(va: usize) -> usize {
    va & !VA_SIGN_MASK
}

/// `L4_SLOT_PTE`: the PML4 slot of the recursive mapping.
pub const L4_SLOT_PTE: usize = 255;
/// `L4_SLOT_KERN`: the first PML4 slot of kernel virtual space.
pub const L4_SLOT_KERN: usize = 256;
/// `L4_SLOT_KERNBASE`: the PML4 slot of the kernel image.
pub const L4_SLOT_KERNBASE: usize = 511;

/// `DIRECT_MAP_PML4_SLOTS`: PML4 slots the direct map takes.
pub const DIRECT_MAP_PML4_SLOTS: usize = 4;
/// `DIRECT_MAP_START_CHOICES`: how many PML4 slots the direct map may start at.
pub const DIRECT_MAP_START_CHOICES: usize = 32;
/// `DIRECT_MAP_START_MASK`.
pub const DIRECT_MAP_START_MASK: usize = DIRECT_MAP_START_CHOICES - 1;
/// `DIRECT_MAP_RESERVED_PML4_SLOTS`.
pub const DIRECT_MAP_RESERVED_PML4_SLOTS: usize =
    DIRECT_MAP_PML4_SLOTS + DIRECT_MAP_START_CHOICES - 1;
/// `DIRECT_MAP_SIZE`: bytes the direct map covers.
pub const DIRECT_MAP_SIZE: usize = DIRECT_MAP_PML4_SLOTS * NBPD_L4;
/// `L4_SLOT_DIRECT`: the lowest PML4 slot of the direct map.
pub const L4_SLOT_DIRECT: usize = L4_SLOT_KERNBASE - DIRECT_MAP_RESERVED_PML4_SLOTS;
/// `L4_SLOT_EARLY`: the PML4 slot of the early mapping.
pub const L4_SLOT_EARLY: usize = L4_SLOT_DIRECT - 1;

/// `PDIR_SLOT_KERN`.
pub const PDIR_SLOT_KERN: usize = L4_SLOT_KERN;
/// `PDIR_SLOT_PTE`.
pub const PDIR_SLOT_PTE: usize = L4_SLOT_PTE;
/// `PDIR_SLOT_DIRECT`.
pub const PDIR_SLOT_DIRECT: usize = L4_SLOT_DIRECT;
/// `PDIR_SLOT_EARLY`.
pub const PDIR_SLOT_EARLY: usize = L4_SLOT_EARLY;

// The following defines give the virtual addresses of the page tables and the page directory
// through the recursive mapping in slot L4_SLOT_PTE: PTE_BASE is the level-1 table of every
// address, L2_BASE the level-2 tables, and so on up to the PML4 itself at L4_BASE.

/// `PTE_BASE`: where every level-1 table is visible.
pub const PTE_BASE: usize = L4_SLOT_PTE * NBPD_L4;
/// `L1_BASE`.
pub const L1_BASE: usize = PTE_BASE;
/// `L2_BASE`: the level-2 tables.
pub const L2_BASE: usize = L1_BASE + L4_SLOT_PTE * NBPD_L3;
/// `L3_BASE`: the level-3 tables.
pub const L3_BASE: usize = L2_BASE + L4_SLOT_PTE * NBPD_L2;
/// `L4_BASE`: the PML4.
pub const L4_BASE: usize = L3_BASE + L4_SLOT_PTE * NBPD_L1;
/// `PDP_PDE`: the recursive entry itself.
pub const PDP_PDE: usize = L4_BASE + L4_SLOT_PTE * size_of::<PdEntry>();
/// `PDP_BASE`.
pub const PDP_BASE: usize = L4_BASE;

/// `pl1_pi(VA)`: the index inside the level-1 table.
pub const fn pl1_pi(va: usize) -> usize {
    (va_sign_pos(va) & L1_MASK) >> L1_SHIFT
}

/// `pl2_pi(VA)`.
pub const fn pl2_pi(va: usize) -> usize {
    (va_sign_pos(va) & L2_MASK) >> L2_SHIFT
}

/// `pl3_pi(VA)`.
pub const fn pl3_pi(va: usize) -> usize {
    (va_sign_pos(va) & L3_MASK) >> L3_SHIFT
}

/// `pl4_pi(VA)`.
pub const fn pl4_pi(va: usize) -> usize {
    (va_sign_pos(va) & L4_MASK) >> L4_SHIFT
}

/// `pl1_i(VA)`: the index of `va`'s level-1 entry counted from `PTE_BASE`.
pub const fn pl1_i(va: usize) -> usize {
    (va_sign_pos(va) & L1_FRAME) >> L1_SHIFT
}

/// `pl2_i(VA)`.
pub const fn pl2_i(va: usize) -> usize {
    (va_sign_pos(va) & L2_FRAME) >> L2_SHIFT
}

/// `pl3_i(VA)`.
pub const fn pl3_i(va: usize) -> usize {
    (va_sign_pos(va) & L3_FRAME) >> L3_SHIFT
}

/// `pl4_i(VA)`.
pub const fn pl4_i(va: usize) -> usize {
    (va_sign_pos(va) & L4_FRAME) >> L4_SHIFT
}

/// `PTP_MASK_INITIALIZER`: `ptp_masks[]`.
pub const PTP_MASKS: [usize; PTP_LEVELS] = [L1_FRAME, L2_FRAME, L3_FRAME, L4_FRAME];
/// `PTP_SHIFT_INITIALIZER`: `ptp_shifts[]`.
pub const PTP_SHIFTS: [u32; PTP_LEVELS] = [L1_SHIFT, L2_SHIFT, L3_SHIFT, L4_SHIFT];
/// `NKPTP_INITIALIZER`: `nkptp[]` at boot.
pub const NKPTP_INITIALIZER: [usize; PTP_LEVELS] = [
    NKL1_START_ENTRIES,
    NKL2_START_ENTRIES,
    NKL3_START_ENTRIES,
    NKL4_START_ENTRIES,
];
/// `NKPTPMAX_INITIALIZER`: `nkptpmax[]`.
pub const NKPTPMAX_INITIALIZER: [usize; PTP_LEVELS] = [
    NKL1_MAX_ENTRIES,
    NKL2_MAX_ENTRIES,
    NKL3_MAX_ENTRIES,
    NKL4_MAX_ENTRIES,
];
/// `NBPD_INITIALIZER`: `nbpd[]`.
pub const NBPD_INITIALIZER: [usize; PTP_LEVELS] = [NBPD_L1, NBPD_L2, NBPD_L3, NBPD_L4];
/// `PDES_INITIALIZER`: `normal_pdes[]`, the level 2, 3 and 4 tables through the recursive map.
pub const PDES_INITIALIZER: [usize; PTP_LEVELS - 1] = [L2_BASE, L3_BASE, L4_BASE];

/// `pl_i(va, lvl)`: the index of `va`'s level-`lvl` entry counted from that level's base.
pub const fn pl_i(va: usize, lvl: usize) -> usize {
    (va_sign_pos(va) & PTP_MASKS[lvl - 1]) >> PTP_SHIFTS[lvl - 1]
}

/// `ptp_va2o(va, lvl)`: the offset of `va`'s level-`lvl` PTP in `pm_obj[lvl - 1]`.
pub const fn ptp_va2o(va: usize, lvl: usize) -> usize {
    pl_i(va, lvl + 1) * PAGE_SIZE
}

/// `kvtopte(va)`: the level-1 entry of `va` through the recursive mapping (`LARGEPAGES` is
/// not an option here, so there is no 2M case).
pub fn kvtopte(va: usize) -> *mut PtEntry {
    (PTE_BASE as *mut PtEntry).wrapping_add(pl1_i(va))
}

// NK*_MAX_ENTRIES: the maximum number of PTPs per level the kernel may use. NK*_KIMG_ENTRIES
// and ND*_ENTRIES: how many the kernel image and the direct map take at boot. NK*_START_ENTRIES:
// how many are there to begin with.

/// `NKL4_MAX_ENTRIES`.
pub const NKL4_MAX_ENTRIES: usize = 1;
/// `NKL3_MAX_ENTRIES`.
pub const NKL3_MAX_ENTRIES: usize = NKL4_MAX_ENTRIES * 512;
/// `NKL2_MAX_ENTRIES`.
pub const NKL2_MAX_ENTRIES: usize = NKL3_MAX_ENTRIES * 512;
/// `NKL1_MAX_ENTRIES`.
pub const NKL1_MAX_ENTRIES: usize = NKL2_MAX_ENTRIES * 512;

/// `NKL4_KIMG_ENTRIES`.
pub const NKL4_KIMG_ENTRIES: usize = 1;
/// `NKL3_KIMG_ENTRIES`.
pub const NKL3_KIMG_ENTRIES: usize = 1;
/// `NKL2_KIMG_ENTRIES`.
pub const NKL2_KIMG_ENTRIES: usize = 64;

/// `NDML4_ENTRIES`.
pub const NDML4_ENTRIES: usize = 1;
/// `NDML3_ENTRIES`.
pub const NDML3_ENTRIES: usize = 1;
/// `NDML2_ENTRIES`: 4GB.
pub const NDML2_ENTRIES: usize = 4;

/// `NKL4_START_ENTRIES`.
pub const NKL4_START_ENTRIES: usize = 0;
/// `NKL3_START_ENTRIES`.
pub const NKL3_START_ENTRIES: usize = 0;
/// `NKL2_START_ENTRIES`.
pub const NKL2_START_ENTRIES: usize = 0;
/// `NKL1_START_ENTRIES`: XXX.
pub const NKL1_START_ENTRIES: usize = 0;

/// `NTOPLEVEL_PDES`: entries in the top-level table.
pub const NTOPLEVEL_PDES: usize = PAGE_SIZE / size_of::<PdEntry>();
/// `NPDPG`: entries in a page of page-table entries.
pub const NPDPG: usize = PAGE_SIZE / size_of::<PdEntry>();

/// `PTP_LEVELS`: how many levels of page tables.
pub const PTP_LEVELS: usize = 4;

/// `PG_W`: "wired" mapping.
pub const PG_W: u64 = PG_AVAIL1;
/// `PG_PVLIST`: mapping has entry on pvlist.
pub const PG_PVLIST: u64 = PG_AVAIL2;

/// `PCID_KERN`: for `pmap_kernel()`.
pub const PCID_KERN: u64 = 0;
/// `PCID_PROC`: non-`pmap_kernel()`, U+K.
pub const PCID_PROC: u64 = 1;
/// `PCID_PROC_INTEL`: non-`pmap_kernel()`, U-K (meltdown).
pub const PCID_PROC_INTEL: u64 = 2;
/// `PCID_TEMP`: temp mapping of another non-`pmap_kernel()`.
pub const PCID_TEMP: u64 = 3;
/// `PCID_EFI`: EFI runtime services.
pub const PCID_EFI: u64 = 4;

/// `NPTECL`: number of PTEs per cache line.
pub const NPTECL: usize = 8;

/// `PMAP_TYPE_NORMAL`: a plain pmap.
pub const PMAP_TYPE_NORMAL: i32 = 1;
/// `PMAP_TYPE_EPT`: an Intel nested pmap (vmm).
pub const PMAP_TYPE_EPT: i32 = 2;
/// `PMAP_TYPE_RVI`: an AMD nested pmap (vmm).
pub const PMAP_TYPE_RVI: i32 = 3;

/// `PMAP_EFI`: an EFI runtime mapping.
pub const PMAP_EFI: i32 = PMAP_MD0;
/// `PMAP_NOCRYPT`: no memory encryption.
pub const PMAP_NOCRYPT: i32 = PMAP_MD1;

/// `PMAP_PA_MASK`: to remove the flags.
pub const PMAP_PA_MASK: usize = !PAGE_MASK;
/// `PMAP_NOCACHE`: set the non-cacheable bit.
pub const PMAP_NOCACHE: i32 = 0x1;
/// `PMAP_WC`: set page write combining.
pub const PMAP_WC: i32 = 0x2;

/// `PG_PMAP_MOD`: the page was modified.
pub const PG_PMAP_MOD: u32 = PG_PMAP0;
/// `PG_PMAP_REF`: the page was referenced.
pub const PG_PMAP_REF: u32 = PG_PMAP1;
/// `PG_PMAP_WC`: the page is mapped write-combining.
pub const PG_PMAP_WC: u32 = PG_PMAP2;

/// `struct pmap`: the bootstrap subset (see the module doc).
pub struct Pmap {
    /// `pm_mtx`: guards a user pmap's page tables, PTP objects, hints and statistics
    /// (`pmap_map_ptes`/`pmap_unmap_ptes`); the kernel pmap never takes it.
    pub pm_mtx: Mutex,
    /// Objects for lvl >= 1.
    pub pm_obj: [UvmObject; PTP_LEVELS - 1],
    // pm_list (lck by pm_list lock): M6.
    /// VA of page table to be used when executing in privileged mode.
    pub pm_pdir: Cell<*mut PdEntry>,
    /// VA of special page table to be used when executing on an Intel CPU in usermode (no
    /// kernel mappings).
    pub pm_pdir_intel: Cell<*mut PdEntry>,
    /// PA of page table to be used when executing in privileged mode.
    pub pm_pdirpa: Cell<Paddr>,
    /// PA of special page table to be used when executing on an Intel CPU in usermode (no
    /// kernel mappings).
    pub pm_pdirpa_intel: Cell<Paddr>,
    /// Pointer to a PTP in our pmap.
    pub pm_ptphint: [Cell<*const VmPage>; PTP_LEVELS - 1],
    /// pmap stats (lck by object lock).
    pub pm_stats: PmapStatistics,
    /// Type of pmap this is (`PMAP_TYPE_x`).
    pub pm_type: Cell<i32>,
    /// Cached EPTP (used by vmm).
    pub eptp: Cell<u64>,
}

// SAFETY: a user pmap is guarded by `pm_mtx`, as in C; the kernel pmap's tables by the
// kernel map's lock and `pmap_growkernel`'s callers, its statistics are updated racily as in
// C (single-word counters). `pm_pdir`, `pm_pdirpa`, `pm_type` and `eptp` are written once by
// `pmap_create`/`pmap_bootstrap` before the pmap is shared; `pm_obj[0].uo_refs` is atomic.
unsafe impl Sync for Pmap {}

impl Pmap {
    /// A pmap with nothing mapped.
    pub const fn new() -> Self {
        Self {
            pm_mtx: Mutex::new(IPL_VM),
            pm_obj: [UvmObject::new(1), UvmObject::new(1), UvmObject::new(1)],
            pm_pdir: Cell::new(ptr::null_mut()),
            pm_pdir_intel: Cell::new(ptr::null_mut()),
            pm_pdirpa: Cell::new(Paddr::new(0)),
            pm_pdirpa_intel: Cell::new(Paddr::new(0)),
            pm_ptphint: [
                Cell::new(ptr::null()),
                Cell::new(ptr::null()),
                Cell::new(ptr::null()),
            ],
            pm_stats: PmapStatistics::new(),
            pm_type: Cell::new(PMAP_TYPE_NORMAL),
            eptp: Cell::new(0),
        }
    }

    /// `pmap_nested(pm)`: a nested (vmm) pmap.
    pub fn pmap_nested(&self) -> bool {
        self.pm_type.get() != PMAP_TYPE_NORMAL
    }

    /// `pmap_is_ept(pm)`.
    pub fn pmap_is_ept(&self) -> bool {
        self.pm_type.get() == PMAP_TYPE_EPT
    }

    /// `pmap_resident_count(pmap)`.
    pub fn pmap_resident_count(&self) -> i64 {
        self.pm_stats.resident_count.get()
    }

    /// `pmap_wired_count(pmap)`.
    pub fn pmap_wired_count(&self) -> i64 {
        self.pm_stats.wired_count.get()
    }
}

impl Default for Pmap {
    fn default() -> Self {
        Self::new()
    }
}

/// `struct pv_entry`: one mapping of a page, locked by its list's `pvh_lock`.
pub struct PvEntry {
    /// Next entry.
    pub pv_next: Cell<*const PvEntry>,
    /// The pmap.
    pub pv_pmap: Cell<*const Pmap>,
    /// The virtual address.
    pub pv_va: Cell<Vaddr>,
    /// The vm_page of the PTP.
    pub pv_ptp: Cell<*const VmPage>,
}

/// `struct vm_page_md`: the pmap's per-page data.
pub struct VmPageMd {
    /// Protects `pv_list`.
    pub pv_mtx: Mutex,
    /// The mappings of this page.
    pub pv_list: Cell<*const PvEntry>,
}

/// `VM_MDPAGE_INIT`: no mappings.
#[allow(clippy::declare_interior_mutable_const)] // an initializer, copied into every vm_page
pub const VM_MDPAGE_INIT: VmPageMd = VmPageMd {
    pv_mtx: Mutex::new(IPL_VM),
    pv_list: Cell::new(ptr::null()),
};

/// `pmap_clear_modify(pg)`.
pub fn pmap_clear_modify(pg: &VmPage) -> bool {
    pmap_clear_attrs(pg, PG_M)
}

/// `pmap_clear_reference(pg)`.
pub fn pmap_clear_reference(pg: &VmPage) -> bool {
    pmap_clear_attrs(pg, PG_U)
}

/// `pmap_is_modified(pg)`.
pub fn pmap_is_modified(pg: &VmPage) -> bool {
    pmap_test_attrs(pg, PG_M)
}

/// `pmap_is_referenced(pg)`.
pub fn pmap_is_referenced(pg: &VmPage) -> bool {
    pmap_test_attrs(pg, PG_U)
}

/// `pmap_page_protect`: change the protection of all recorded mappings of a managed page.
///
/// This function is a frontend for `pmap_page_remove`/`pmap_clear_attrs`. We only have to
/// worry about making the page more protected; unprotecting a page is done on-demand at
/// fault time.
pub fn pmap_page_protect(pg: &VmPage, prot: VmProt) {
    if prot == PROT_READ {
        let _ = pmap_clear_attrs(pg, PG_RW);
    } else {
        kassert!(prot == PROT_NONE);
        pmap_page_remove(pg);
    }
}

/// `pmap_protect`: change the protection of pages in a pmap.
///
/// This function is a frontend for `pmap_remove`/`pmap_write_protect`. We only have to
/// worry about making the page more protected; unprotecting a page is done on-demand at
/// fault time.
pub fn pmap_protect(pmap: &Pmap, sva: Vaddr, eva: Vaddr, prot: VmProt) {
    if prot != PROT_NONE {
        pmap_write_protect(pmap, sva, eva, prot);
    } else {
        pmap_remove(pmap, sva, eva);
    }
}

/// `pmap_valid_entry(E)`: is PDE or PTE valid?
pub const fn pmap_valid_entry(e: PdEntry) -> bool {
    e & crate::arch::amd64::include::pte::PG_V != 0
}

/// `pmap_resident_count(pmap)`.
pub fn pmap_resident_count(pmap: &Pmap) -> i64 {
    pmap.pm_stats.resident_count.get()
}

/// `pmap_wired_count(pmap)`.
pub fn pmap_wired_count(pmap: &Pmap) -> i64 {
    pmap.pm_stats.wired_count.get()
}
/* </CODE> */

/* <TESTS> */
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn slot_layout() {
        assert_eq!(L4_SLOT_DIRECT, 511 - 35);
        assert_eq!(L4_SLOT_EARLY, L4_SLOT_DIRECT - 1);
        assert_eq!(DIRECT_MAP_SIZE, 4 << 39);
        assert_eq!(NTOPLEVEL_PDES, 512);
        assert!(DIRECT_MAP_START_CHOICES & DIRECT_MAP_START_MASK == 0);
    }

    #[test]
    fn recursive_mapping_indices() {
        // The PML4 maps itself in slot 255: its own address is L4_BASE + 255 * 8.
        assert_eq!(PTE_BASE, 0x0000_7f80_0000_0000);
        assert_eq!(
            L4_BASE,
            PTE_BASE + 255 * NBPD_L3 + 255 * NBPD_L2 + 255 * NBPD_L1
        );
        assert_eq!(PDP_PDE, L4_BASE + 255 * 8);
        let va = 0xffff_8000_0040_1000;
        assert_eq!(pl4_pi(va), 256);
        assert_eq!(pl3_pi(va), 0);
        assert_eq!(pl2_pi(va), 2);
        assert_eq!(pl1_pi(va), 1);
        assert_eq!(pl1_i(va), (256 << 27) | (2 << 9) | 1);
        assert_eq!(pl_i(va, 4), pl4_i(va));
        assert_eq!(pl4_i(va), 256);
        assert_eq!(kvtopte(va) as usize, PTE_BASE + pl1_i(va) * 8);
        assert_eq!(ptp_va2o(va, 1), pl2_i(va) * PAGE_SIZE);
    }

    #[test]
    #[ignore = "needs OPENBSD_SRC (just test-ref)"]
    fn values_match_the_c_header() {
        let defs = crate::reftest::defines("sys/arch/amd64/include/pmap.h");
        let ours: &[(&str, i64)] = &[
            ("L4_SLOT_PTE", L4_SLOT_PTE as i64),
            ("L4_SLOT_KERN", L4_SLOT_KERN as i64),
            ("L4_SLOT_KERNBASE", L4_SLOT_KERNBASE as i64),
            ("DIRECT_MAP_PML4_SLOTS", DIRECT_MAP_PML4_SLOTS as i64),
            ("DIRECT_MAP_START_CHOICES", DIRECT_MAP_START_CHOICES as i64),
            ("NKL2_KIMG_ENTRIES", NKL2_KIMG_ENTRIES as i64),
            ("NDML2_ENTRIES", NDML2_ENTRIES as i64),
            ("PTP_LEVELS", PTP_LEVELS as i64),
            ("PCID_KERN", PCID_KERN as i64),
            ("PCID_PROC", PCID_PROC as i64),
            ("PCID_PROC_INTEL", PCID_PROC_INTEL as i64),
            ("PCID_TEMP", PCID_TEMP as i64),
            ("PCID_EFI", PCID_EFI as i64),
            ("NPTECL", NPTECL as i64),
            ("PMAP_TYPE_NORMAL", i64::from(PMAP_TYPE_NORMAL)),
            ("PMAP_TYPE_EPT", i64::from(PMAP_TYPE_EPT)),
            ("PMAP_TYPE_RVI", i64::from(PMAP_TYPE_RVI)),
            ("PMAP_NOCACHE", i64::from(PMAP_NOCACHE)),
            ("PMAP_WC", i64::from(PMAP_WC)),
        ];
        for (name, value) in ours {
            assert_eq!(crate::reftest::int(&defs, name), Some(*value), "{name}");
        }
    }
}
/* </TESTS> */
