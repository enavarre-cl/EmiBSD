/* $OpenBSD: pmap.h,v 1.29 2025/05/21 09:42:59 kettenis Exp $ */
/* <LICENSES> */
/*
 * Copyright (c) 2008,2009,2014 Dale Rahn <drahn@dalerahn.com>
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

/* <CODE> */
//! arm64 `<machine/pmap.h>`: the physical map's types and constants.
//!
//! Upstream: sys/arch/arm64/include/pmap.h @ 3ce1f3f79392
//!
//! Status: `wip`. Milestone M3 ports `struct pmap` (its kernel subset, with the `pm_vp`
//! tables), `struct vm_page_md`, the `VP_IDX*` layout and the cache and pv flags. The pointer
//! authentication keys and the zero/copy windows arrive with M6.
//!
//! ## Deviations
//! - `pm_refs` is an atomic (the C's `atomic_inc_int`/`atomic_dec_int_nv` on an `int`).
//! - `pm_vp` is an enum of the two union members (`l0` for four-level tables, `l1` for three);
//!   the C picks by `have_4_level_pt`.

use core::cell::Cell;
use core::sync::atomic::{AtomicI32, AtomicU64};

use core::ptr;

use crate::arch::arm64::arm64::pmap::{Pmapvp0, Pmapvp1, PvList};
use crate::arch::arm64::include::param::PAGE_MASK;
use crate::machine::intr::IPL_VM;
use crate::sys::mutex::Mutex;
use crate::sys::queue::ListHead;
use crate::uvm::uvm_page::{PG_PMAP0, PG_PMAP1, PG_PMAP2};
use crate::uvm::uvm_pmap::{PMAP_MD0, PMAP_MD1, PMAP_MD2, PMAP_MD3, PmapStatistics};

// V->P mapping data

/// `VP_IDX0_CNT`: entries in a level-0 table.
pub const VP_IDX0_CNT: usize = 512;
/// `VP_IDX0_MASK`.
pub const VP_IDX0_MASK: usize = VP_IDX0_CNT - 1;
/// `VP_IDX0_POS`: the level-0 index starts at this bit of the virtual address.
pub const VP_IDX0_POS: u32 = 39;
/// `VP_IDX1_CNT`: entries in a level-1 table.
pub const VP_IDX1_CNT: usize = 512;
/// `VP_IDX1_MASK`.
pub const VP_IDX1_MASK: usize = VP_IDX1_CNT - 1;
/// `VP_IDX1_POS`.
pub const VP_IDX1_POS: u32 = 30;
/// `VP_IDX2_CNT`: entries in a level-2 table.
pub const VP_IDX2_CNT: usize = 512;
/// `VP_IDX2_MASK`.
pub const VP_IDX2_MASK: usize = VP_IDX2_CNT - 1;
/// `VP_IDX2_POS`.
pub const VP_IDX2_POS: u32 = 21;
/// `VP_IDX3_CNT`: entries in a level-3 table.
pub const VP_IDX3_CNT: usize = 512;
/// `VP_IDX3_MASK`.
pub const VP_IDX3_MASK: usize = VP_IDX3_CNT - 1;
/// `VP_IDX3_POS`.
pub const VP_IDX3_POS: u32 = 12;

// cache flags

/// `PMAP_CACHE_CI`: cache inhibit.
pub const PMAP_CACHE_CI: i32 = PMAP_MD0;
/// `PMAP_CACHE_WT`: writethru.
pub const PMAP_CACHE_WT: i32 = PMAP_MD1;
/// `PMAP_CACHE_WB`: writeback.
pub const PMAP_CACHE_WB: i32 = PMAP_MD1 | PMAP_MD0;
/// `PMAP_CACHE_DEV_NGNRNE`: device nGnRnE.
pub const PMAP_CACHE_DEV_NGNRNE: i32 = PMAP_MD2;
/// `PMAP_CACHE_DEV_NGNRE`: device nGnRE.
pub const PMAP_CACHE_DEV_NGNRE: i32 = PMAP_MD2 | PMAP_MD0;
/// `PMAP_CACHE_BITS`: the cache flags.
pub const PMAP_CACHE_BITS: i32 = PMAP_MD0 | PMAP_MD1 | PMAP_MD2;

/// `PTED_VA_MANAGED_M`: the mapping is of a managed page.
pub const PTED_VA_MANAGED_M: i32 = PMAP_MD3;
/// `PTED_VA_WIRED_M`: the mapping is wired.
pub const PTED_VA_WIRED_M: i32 = PMAP_MD3 << 1;

/// `PMAP_PA_MASK`: to remove the flags.
pub const PMAP_PA_MASK: usize = !PAGE_MASK;
/// `PMAP_NOCACHE`: non-cacheable memory.
pub const PMAP_NOCACHE: i32 = 0x1;
/// `PMAP_DEVICE`: device memory.
pub const PMAP_DEVICE: i32 = 0x2;
/// `PMAP_WC`: write combining is device memory here.
pub const PMAP_WC: i32 = PMAP_DEVICE;

/// `PG_PMAP_MOD`: the page was modified.
pub const PG_PMAP_MOD: u32 = PG_PMAP0;
/// `PG_PMAP_REF`: the page was referenced.
pub const PG_PMAP_REF: u32 = PG_PMAP1;
/// `PG_PMAP_EXE`: the page was mapped executable (and is I-cache clean).
pub const PG_PMAP_EXE: u32 = PG_PMAP2;

/// `pm_vp`: the virtual to physical tables, 4 lvl (`l0`) or 3 lvl (`l1`).
#[derive(Clone, Copy, Debug)]
pub enum PmVp {
    /// `pm_vp.l0`: four-level tables.
    L0(*mut Pmapvp0),
    /// `pm_vp.l1`: three-level tables.
    L1(*mut Pmapvp1),
}

/// `struct pmap`: the kernel subset (see the module doc).
pub struct Pmap {
    /// `pm_mtx`: guards the tables, the pteds and `pm_stats` of a user pmap (`pmap_lock`);
    /// the kernel pmap never takes it.
    pub pm_mtx: Mutex,
    /// The virtual to physical tables.
    pub pm_vp: Cell<PmVp>,
    /// Physical address of the lower-half (`TTBR0_EL1`) table.
    pub pm_pt0pa: Cell<u64>,
    /// The address space id and its generation: an atomic because `pmap_rollover_asid`
    /// (under `pmap_asid_mtx`) rewrites the pmaps other CPUs run on while they read it.
    pub pm_asid: AtomicU64,
    /// Guarded control stack enabled.
    pub pm_guarded: Cell<u64>,
    /// Four-level page tables (`have_4_level_pt`).
    pub have_4_level_pt: Cell<bool>,
    /// A privileged (kernel) pmap.
    pub pm_privileged: Cell<bool>,
    /// Active on a CPU.
    pub pm_active: AtomicI32,
    /// Ref count.
    pub pm_refs: AtomicI32,
    /// pmap statistics.
    pub pm_stats: PmapStatistics,
    // pm_apiakey, pm_apdakey, pm_apibkey, pm_apdbkey, pm_apgakey (pointer authentication): M6.
}

// SAFETY: a user pmap's tables, pteds and statistics are guarded by `pm_mtx`, the kernel
// pmap's by the kernel map's lock and `pmap_growkernel`'s callers, as in C; `pm_asid`,
// `pm_active` and `pm_refs` are atomics; `pm_vp`, `have_4_level_pt` and `pm_privileged` are
// written once by `pmap_pinit`/`pmap_bootstrap` before the pmap is shared, and `pm_pt0pa`
// besides by `pmap_purge` on the last thread of a dying process.
unsafe impl Sync for Pmap {}

impl Pmap {
    /// A pmap with nothing mapped.
    pub const fn new() -> Self {
        Self {
            pm_mtx: Mutex::new(IPL_VM),
            pm_vp: Cell::new(PmVp::L1(ptr::null_mut())),
            pm_pt0pa: Cell::new(0),
            pm_asid: AtomicU64::new(0),
            pm_guarded: Cell::new(0),
            have_4_level_pt: Cell::new(false),
            pm_privileged: Cell::new(false),
            pm_active: AtomicI32::new(0),
            pm_refs: AtomicI32::new(0),
            pm_stats: PmapStatistics::new(),
        }
    }
}

impl Default for Pmap {
    fn default() -> Self {
        Self::new()
    }
}

/// `struct vm_page_md`: the pmap's per-page data.
pub struct VmPageMd {
    /// Protects `pv_list`.
    pub pv_mtx: Mutex,
    /// The mappings of this page.
    pub pv_list: ListHead<PvList>,
}

/// `VM_MDPAGE_INIT`: no mappings.
#[allow(clippy::declare_interior_mutable_const)] // an initializer, copied into every vm_page
pub const VM_MDPAGE_INIT: VmPageMd = VmPageMd {
    pv_mtx: Mutex::new(IPL_VM),
    pv_list: ListHead::new(),
};
/* </CODE> */
