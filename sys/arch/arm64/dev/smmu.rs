/* $OpenBSD: smmu.c,v 1.29 2026/01/06 11:57:33 patrick Exp $ */
/* <LICENSES> */
/*
 * Copyright (c) 2008-2009,2014-2016 Dale Rahn <drahn@dalerahn.com>
 * Copyright (c) 2021 Patrick Wildt <patrick@blueri.se>
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
//! The ARM System MMU, versions 2 (MMU-500) and 3: `arch/arm64/dev/smmu.c`.
//!
//! Upstream: sys/arch/arm64/dev/smmu.c @ 3ce1f3f79392
//!
//! The SMMU translates the DMA of the devices behind it. Each stream ID gets a domain
//! ([`SmmuDomain`]) with its own I/O page tables, four or three levels of 4 KB pages laid out
//! like the CPU's (`smmuvp0`..`smmuvp3`, filled by a small pmap of their own), and an extent
//! of I/O virtual addresses. A device's DMA tag becomes a copy of its tag whose map functions
//! allocate an IOVA range per map at create time and enter the map's pages there at load
//! time ([`smmu_device_map`]), so the device sees the IOVAs in `ds_addr`. SMMUv2 points a
//! stream (by index or match register) at a context bank programmed with the tables; SMMUv3
//! writes a stream table entry pointing at a context descriptor, through its command queue.
//!
//! ## Deviations
//! - The PTE memory attribute indices are this file's own `SMMU_ATTR_*`: the C uses pte.h's
//!   `PTE_ATTR_*`, whose values there (nGnRnE 0, nGnRE 1, CI 2, WB 3, WT 4) are the MAIR the
//!   SMMU's context is programmed with; EmiBSD's `PTE_ATTR_*` follow its own `MAIR_EL1`
//!   (`include/pte.rs`), so they cannot be used for the SMMU's tables.
//! - The page tables are `#[repr(C)]` arrays of `AtomicU64` (the descriptors the SMMU walks)
//!   and `AtomicPtr` (the software links), so `smmu_vp_enter`'s unlocked check before
//!   taking `sd_pmap_mtx` is a pair of atomic loads; pool items are zeroed, which is a valid
//!   empty table, and never returned to their pool (the C has no garbage collection either:
//!   `smmu_remove` is empty there too).
//! - `membar_producer()` is `fence(Release)` and `membar_sync()` `fence(SeqCst)`
//!   (`docs/C_TO_RUST.md`).
//! - The six SMMUv3 command functions share their queue insertion ([`smmu_v3_cmd_put`]),
//!   which the C repeats in each; the commands, their order and the syncs are the C's.
//! - Functions return `Result<_, Errno>`; `smmu_v2_attach`'s bare `return 1` (no
//!   translation stages, no streams, bad bank counts) is `ENXIO`. Lookups return `Option`.
//! - `smmu_device_map` takes and returns `Option<BusDmaTagT>` (the IORT attachment's
//!   contract); a missing tag, which the C would dereference, is handed back as `None`.
//! - `struct smmu_cb` is empty: a bank in use is a reference to the one [`SmmuCb`] value
//!   instead of a zero-byte allocation.
//! - A map's state (`struct smmu_map_state`) is `malloc`ed with the map's wait flags as in
//!   C; the domain, its tag and the dmamem descriptors are boxes, leaked like the C's
//!   allocations that are never freed (a domain that fails to set up is dropped, as the C
//!   frees it).
//! - `extent_create` cannot fail with `EX_WAITOK` in the C; here a `None` makes the domain's
//!   creation fail with `ENOMEM`. A page-table page whose physical address `pmap_extract`
//!   cannot find panics in the domain setup as it does in `smmu_set_l*`.

use alloc::boxed::Box;
use alloc::format;
use alloc::vec::Vec;
use core::cell::Cell;
use core::ffi::c_void;
use core::ptr::{self, NonNull};
use core::sync::atomic::{AtomicPtr, AtomicU64, Ordering, fence};

use crate::arch::arm64::arm64::cpufunc::cpu_dcache_wb_range;
use crate::arch::arm64::dev::smmureg::*;
use crate::arch::arm64::dev::smmuvar::{
    SmmuCb, SmmuCbIrq, SmmuDmamem, SmmuDomain, SmmuDomainV3, SmmuSmr, SmmuSoftc, SmmuV3Queue,
};
use crate::arch::arm64::include::bus::{
    BUS_DMA_ALLOCNOW, BUS_DMA_NOWAIT, BUS_DMA_WAITOK, BUS_DMASYNC_POSTREAD, BUS_DMASYNC_POSTWRITE,
    BUS_DMASYNC_PREWRITE, BusDmaSegment, BusDmaTag, BusDmaTagT, BusDmamap, BusDmamapT,
};
use crate::arch::arm64::include::pmap::{
    PMAP_CACHE_BITS, PMAP_CACHE_CI, PMAP_CACHE_DEV_NGNRE, PMAP_CACHE_DEV_NGNRNE, PMAP_CACHE_WB,
    PMAP_CACHE_WT, VP_IDX0_CNT, VP_IDX0_MASK, VP_IDX0_POS, VP_IDX1_CNT, VP_IDX1_MASK, VP_IDX1_POS,
    VP_IDX2_CNT, VP_IDX2_MASK, VP_IDX2_POS, VP_IDX3_CNT, VP_IDX3_MASK, VP_IDX3_POS,
};
use crate::arch::arm64::include::pte::{
    ATTR_AF, ATTR_PXN, ATTR_nG, L3_P, Lx_TABLE_ALIGN, Lx_TYPE_PT, PTE_MEMATTR_CI,
    PTE_MEMATTR_DEV_NGNRE, PTE_MEMATTR_DEV_NGNRNE, PTE_MEMATTR_WB, PTE_MEMATTR_WT, PTE_RPGN,
    SH_INNER, attr_ap, attr_idx, attr_sh,
};
use crate::kassert;
use crate::kern::kern_lock::{mtx_enter, mtx_init, mtx_leave};
use crate::kern::kern_malloc::{free, malloc};
use crate::kern::subr_extent::{
    extent_alloc_region, extent_alloc_with_descr, extent_create, extent_free,
};
use crate::kern::subr_pool::{pool_get, pool_init, pool_setlowat};
use crate::kern::subr_prf::{panic, printf};
use crate::machine::bus::{
    BusAddr, BusSize, bus_dmamap_create, bus_dmamap_destroy, bus_dmamap_load, bus_dmamap_sync,
    bus_dmamem_alloc, bus_dmamem_free, bus_dmamem_map, bus_dmamem_unmap, bus_space_read_4,
    bus_space_write_4,
};
use crate::machine::intr::IPL_VM;
use crate::machine::pmap::{pmap_extract, pmap_kernel};
use crate::sys::device::{Cfdriver, DV_DULL};
use crate::sys::errno::Errno;
use crate::sys::extent::{EX_CONFLICTOK, EX_NOCOALESCE, EX_NOWAIT, EX_WAITOK, ExtentRegion};
use crate::sys::malloc::{M_DEVBUF, M_NOWAIT, M_WAITOK, M_ZERO};
use crate::sys::mbuf::Mbuf;
use crate::sys::mman::{PROT_EXEC, PROT_NONE, PROT_READ, PROT_WRITE};
use crate::sys::mutex::{Mutex, mutex_assert_locked};
use crate::sys::param::{PAGE_SHIFT, PAGE_SIZE};
use crate::sys::pool::{PR_NOWAIT, PR_WAITOK, PR_ZERO, Pool};
use crate::sys::proc::Proc;
use crate::sys::queue::SimpleqEntry;
use crate::sys::types::Vaddr;
use crate::sys::uio::Uio;
use crate::uvm::uvm_param::{round_page, trunc_page};

/// `PTE_ATTR_DEV_NGNRNE` of the C's pte.h: index 0 of the SMMU context's MAIR.
const SMMU_ATTR_DEV_NGNRNE: u64 = 0;
/// `PTE_ATTR_DEV_NGNRE`: index 1.
const SMMU_ATTR_DEV_NGNRE: u64 = 1;
/// `PTE_ATTR_CI`: index 2 (normal non-cacheable).
const SMMU_ATTR_CI: u64 = 2;
/// `PTE_ATTR_WB`: index 3 (write-back).
const SMMU_ATTR_WB: u64 = 3;
/// `PTE_ATTR_WT`: index 4 (write-through).
const SMMU_ATTR_WT: u64 = 4;

/// `struct smmu_map_state`: a DMA map's IOVA range.
struct SmmuMapState {
    /// `sms_er`: the range's extent descriptor.
    sms_er: ExtentRegion,
    /// `sms_dva`: the first IOVA.
    sms_dva: Cell<u64>,
    /// `sms_len`: the range's length, without the guard page.
    sms_len: Cell<u64>,
    /// `sms_loaded`: the bytes mapped by the current load.
    sms_loaded: Cell<u64>,
}

/// `struct smmuvp0`: a 4-level root, 512 GB per entry.
#[repr(C)]
pub struct SmmuVp0 {
    /// `l0`: the descriptors the SMMU walks.
    pub l0: [AtomicU64; VP_IDX0_CNT],
    /// `vp`: the next level's tables.
    pub vp: [AtomicPtr<SmmuVp1>; VP_IDX0_CNT],
}

/// `struct smmuvp1`: 1 GB per entry (the 3-level root).
#[repr(C)]
pub struct SmmuVp1 {
    /// `l1`.
    pub l1: [AtomicU64; VP_IDX1_CNT],
    /// `vp`.
    pub vp: [AtomicPtr<SmmuVp2>; VP_IDX1_CNT],
}

/// `struct smmuvp2`: 2 MB per entry.
#[repr(C)]
pub struct SmmuVp2 {
    /// `l2`.
    pub l2: [AtomicU64; VP_IDX2_CNT],
    /// `vp`.
    pub vp: [AtomicPtr<SmmuVp3>; VP_IDX2_CNT],
}

/// `struct smmuvp3`: the page descriptors.
#[repr(C)]
pub struct SmmuVp3 {
    /// `l3`.
    pub l3: [AtomicU64; VP_IDX3_CNT],
}

/// A page-table page that is valid as zeroes (an empty table), as a pool hands it out with
/// `PR_ZERO`.
///
/// # Safety
///
/// Every all-zero bit pattern is a valid value of the type.
unsafe trait SmmuVpTable: Sized {}

// SAFETY: arrays of atomics, which are plain integers and pointers.
unsafe impl SmmuVpTable for SmmuVp0 {}
// SAFETY: as above.
unsafe impl SmmuVpTable for SmmuVp1 {}
// SAFETY: as above.
unsafe impl SmmuVpTable for SmmuVp2 {}
// SAFETY: as above.
unsafe impl SmmuVpTable for SmmuVp3 {}

/// The value of every unused `sc_cb[]` entry's referent: banks in use point here.
static SMMU_CB: SmmuCb = SmmuCb;

/// `smmu_cd`.
pub static SMMU_CD: Cfdriver = Cfdriver::new(b"smmu", DV_DULL, 0);

/// `pool_get(pp, flags | PR_ZERO)` as a page-table page: `None` when the pool is empty and
/// `flags` is `PR_NOWAIT`.
fn smmu_vp_get<T: SmmuVpTable>(pp: &Pool, flags: i32) -> Option<&'static T> {
    let p = pool_get(pp, flags | PR_ZERO)?;
    // SAFETY: the pools hand out zeroed items at least as large as `T` (`smmu_attach` sized
    // them for the largest table) aligned to a page; all-zero is a valid `T`
    // (`SmmuVpTable`), and the items are never given back.
    Some(unsafe { &*p.as_ptr().cast::<T>() })
}

/// `pmap_extract(pmap_kernel(), va, &pa)` of a page-table page.
fn smmu_vp_pa(va: usize) -> Option<usize> {
    pmap_extract(pmap_kernel(), Vaddr::new(va)).map(|pa| pa.as_usize())
}

/// `smmu_attach(sc)`: the parts shared by both versions (the domain queue, the page-table
/// pools).
fn smmu_attach(sc: &SmmuSoftc) -> Result<(), Errno> {
    // SIMPLEQ_INIT(&sc->sc_domains): the zeroed queue is empty.

    let vp: &'static Pool = Box::leak(Box::new(Pool::new()));
    pool_init(
        vp,
        size_of::<SmmuVp0>(),
        PAGE_SIZE as u32,
        IPL_VM,
        0,
        "smmu_vp",
        None,
    );
    pool_setlowat(vp, 20);
    sc.sc_vp_pool.set(Some(vp));
    let vp3: &'static Pool = Box::leak(Box::new(Pool::new()));
    pool_init(
        vp3,
        size_of::<SmmuVp3>(),
        PAGE_SIZE as u32,
        IPL_VM,
        0,
        "smmu_vp3",
        None,
    );
    pool_setlowat(vp3, 20);
    sc.sc_vp3_pool.set(Some(vp3));

    Ok(())
}

/// The pool `sel` names (`sc_vp_pool` when false, `sc_vp3_pool` when true).
fn smmu_pool(sc: &SmmuSoftc, l3: bool) -> &'static Pool {
    let pp = if l3 {
        sc.sc_vp3_pool.get()
    } else {
        sc.sc_vp_pool.get()
    };
    match pp {
        Some(pp) => pp,
        None => panic(format_args!("smmu: no page-table pool")),
    }
}

/// A leaked slice of `n` empty cells.
fn smmu_cells<T>(n: usize) -> &'static [Cell<Option<&'static T>>] {
    let v: Vec<Cell<Option<&'static T>>> = (0..n).map(|_| Cell::new(None)).collect();
    Box::leak(v.into_boxed_slice())
}

/// `smmu_v2_attach(sc)`: probes an SMMUv2, puts every stream in fault and every context
/// bank off, and enables it.
pub fn smmu_v2_attach(sc: &'static SmmuSoftc) -> Result<(), Errno> {
    smmu_attach(sc).map_err(|_| Errno::ENXIO)?;

    let mut reg = smmu_gr0_read_4(sc, SMMU_IDR0);
    if reg & SMMU_IDR0_S1TS != 0 {
        sc.sc_has_s1.set(1);
    }
    // Marvell's 8040 does not support 64-bit writes, hence it is not possible to invalidate
    // stage-2, because the ASID is part of the upper 32-bits and they'd be ignored.
    if sc.sc_is_ap806.get() != 0 {
        sc.sc_has_s1.set(0);
    }
    if reg & SMMU_IDR0_S2TS != 0 {
        sc.sc_has_s2.set(1);
    }
    if sc.sc_has_s1.get() == 0 && sc.sc_has_s2.get() == 0 {
        return Err(Errno::ENXIO);
    }
    if reg & SMMU_IDR0_EXIDS != 0 {
        sc.sc_has_exids.set(1);
    }

    sc.sc_num_streams.set(1 << smmu_idr0_numsidb(reg));
    if sc.sc_has_exids.get() != 0 {
        sc.sc_num_streams.set(1 << 16);
    }
    sc.sc_stream_mask.set((sc.sc_num_streams.get() - 1) as u16);
    if reg & SMMU_IDR0_SMS != 0 {
        sc.sc_num_streams.set(smmu_idr0_numsmrg(reg) as i32);
        if sc.sc_num_streams.get() == 0 {
            return Err(Errno::ENXIO);
        }
        sc.sc_smr.set(Some(
            smmu_cells::<SmmuSmr>(sc.sc_num_streams.get() as usize),
        ));
    }

    reg = smmu_gr0_read_4(sc, SMMU_IDR1);
    sc.sc_pagesize.set(4 * 1024);
    if reg & SMMU_IDR1_PAGESIZE_64K != 0 {
        sc.sc_pagesize.set(64 * 1024);
    }
    sc.sc_numpage.set(1 << (smmu_idr1_numpagendxb(reg) + 1));

    // 0 to NUMS2CB == stage-2, NUMS2CB to NUMCB == stage-1
    sc.sc_num_context_banks.set(smmu_idr1_numcb(reg) as i32);
    sc.sc_num_s2_context_banks
        .set(smmu_idr1_nums2cb(reg) as i32);
    if sc.sc_num_s2_context_banks.get() > sc.sc_num_context_banks.get() {
        return Err(Errno::ENXIO);
    }
    sc.sc_cb.set(Some(smmu_cells::<SmmuCb>(
        sc.sc_num_context_banks.get() as usize
    )));

    reg = smmu_gr0_read_4(sc, SMMU_IDR2);
    if reg & SMMU_IDR2_VMID16S != 0 {
        sc.sc_has_vmid16s.set(1);
    }

    sc.sc_ipa_bits.set(match smmu_idr2_ias(reg) {
        SMMU_IDR2_IAS_32BIT => 32,
        SMMU_IDR2_IAS_36BIT => 36,
        SMMU_IDR2_IAS_40BIT => 40,
        SMMU_IDR2_IAS_42BIT => 42,
        SMMU_IDR2_IAS_44BIT => 44,
        _ => 48,
    });
    sc.sc_pa_bits.set(match smmu_idr2_oas(reg) {
        SMMU_IDR2_OAS_32BIT => 32,
        SMMU_IDR2_OAS_36BIT => 36,
        SMMU_IDR2_OAS_40BIT => 40,
        SMMU_IDR2_OAS_42BIT => 42,
        SMMU_IDR2_OAS_44BIT => 44,
        _ => 48,
    });
    sc.sc_va_bits.set(match smmu_idr2_ubs(reg) {
        SMMU_IDR2_UBS_32BIT => 32,
        SMMU_IDR2_UBS_36BIT => 36,
        SMMU_IDR2_UBS_40BIT => 40,
        SMMU_IDR2_UBS_42BIT => 42,
        SMMU_IDR2_UBS_44BIT => 44,
        _ => 48,
    });

    printf(format_args!(
        ": {} CBs ({} S2-only)",
        sc.sc_num_context_banks.get(),
        sc.sc_num_s2_context_banks.get()
    ));
    let ncb = sc.sc_num_context_banks.get();
    if sc.sc_is_qcom.get() != 0 {
        // In theory we should check if bypass quirk is needed by modifying S2CR and
        // re-checking if the value is different. This does not work on the last S2CR, but on
        // the first, which is in use. Revisit this once we have other QCOM HW.
        sc.sc_bypass_quirk.set(1);
        printf(format_args!(", bypass quirk"));
        // Create special context that is turned off. This allows us to map a stream to a
        // context bank where translation is not happening, and hence bypassed.
        smmu_cb_slot(sc, ncb - 1).set(Some(&SMMU_CB));
        smmu_cb_write_4(sc, ncb - 1, SMMU_CB_SCTLR, 0);
        smmu_gr1_write_4(
            sc,
            smmu_cbar((ncb - 1) as usize),
            SMMU_CBAR_TYPE_S1_TRANS_S2_BYPASS,
        );
    }
    printf(format_args!("\n"));

    // Clear Global Fault Status Register
    smmu_gr0_write_4(sc, SMMU_SGFSR, smmu_gr0_read_4(sc, SMMU_SGFSR));

    for i in 0..sc.sc_num_streams.get() {
        let iu = i as usize;
        // On QCOM HW we need to keep current streams running.
        if sc.sc_is_qcom.get() != 0
            && sc.sc_smr.get().is_some()
            && smmu_gr0_read_4(sc, smmu_smr(iu)) & SMMU_SMR_VALID != 0
        {
            reg = smmu_gr0_read_4(sc, smmu_smr(iu));
            let smr: &'static SmmuSmr = Box::leak(Box::new(SmmuSmr {
                ss_dom: Cell::new(None),
                ss_id: Cell::new(((reg >> SMMU_SMR_ID_SHIFT) & SMMU_SMR_ID_MASK) as u16),
                ss_mask: Cell::new(((reg >> SMMU_SMR_MASK_SHIFT) & SMMU_SMR_MASK_MASK) as u16),
            }));
            smmu_smr_slot(sc, i).set(Some(smr));
            if sc.sc_bypass_quirk.get() != 0 {
                smmu_gr0_write_4(sc, smmu_s2cr(iu), SMMU_S2CR_TYPE_TRANS | (ncb - 1) as u32);
            } else {
                smmu_gr0_write_4(sc, smmu_s2cr(iu), SMMU_S2CR_TYPE_BYPASS | 0xff);
            }
            continue;
        }
        // Setup all streams to fault by default (the C's `#else`, bypass, is compiled out)
        smmu_gr0_write_4(sc, smmu_s2cr(iu), SMMU_S2CR_TYPE_FAULT);
        // Disable all stream map registers
        if sc.sc_smr.get().is_some() {
            smmu_gr0_write_4(sc, smmu_smr(iu), 0);
        }
    }

    for i in 0..ncb {
        // Disable Context Bank
        smmu_cb_write_4(sc, i, SMMU_CB_SCTLR, 0);
        // Clear Context Bank Fault Status Register
        smmu_cb_write_4(sc, i, SMMU_CB_FSR, SMMU_CB_FSR_MASK);
    }

    // Invalidate TLB
    smmu_gr0_write_4(sc, SMMU_TLBIALLH, !0);
    smmu_gr0_write_4(sc, SMMU_TLBIALLNSNH, !0);

    if sc.sc_is_mmu500.get() != 0 {
        reg = smmu_gr0_read_4(sc, SMMU_SACR);
        if smmu_idr7_major(smmu_gr0_read_4(sc, SMMU_IDR7)) >= 2 {
            reg &= !SMMU_SACR_MMU500_CACHE_LOCK;
        }
        reg |= SMMU_SACR_MMU500_SMTNMB_TLBEN | SMMU_SACR_MMU500_S2CRB_TLBEN;
        smmu_gr0_write_4(sc, SMMU_SACR, reg);
        for i in 0..ncb {
            reg = smmu_cb_read_4(sc, i, SMMU_CB_ACTLR);
            reg &= !SMMU_CB_ACTLR_CPRE;
            smmu_cb_write_4(sc, i, SMMU_CB_ACTLR, reg);
        }
    }

    // Enable SMMU
    reg = smmu_gr0_read_4(sc, SMMU_SCR0);
    reg &= !(SMMU_SCR0_CLIENTPD | SMMU_SCR0_FB | SMMU_SCR0_BSU_MASK);
    // Disable bypass for unknown streams (the C's `#else`, enabling it, is compiled out)
    reg |= SMMU_SCR0_USFCFG;
    reg |= SMMU_SCR0_GFRE
        | SMMU_SCR0_GFIE
        | SMMU_SCR0_GCFGFRE
        | SMMU_SCR0_GCFGFIE
        | SMMU_SCR0_VMIDPNE
        | SMMU_SCR0_PTM;
    if sc.sc_has_exids.get() != 0 {
        reg |= SMMU_SCR0_EXIDENABLE;
    }
    if sc.sc_has_vmid16s.get() != 0 {
        reg |= SMMU_SCR0_VMID16EN;
    }

    smmu_v2_tlb_sync_global(sc);
    smmu_gr0_write_4(sc, SMMU_SCR0, reg);

    sc.sc_domain_create.set(Some(smmu_v2_domain_create));
    sc.sc_tlbi_va.set(Some(smmu_v2_tlbi_va));
    sc.sc_tlb_sync_context.set(Some(smmu_v2_tlb_sync_context));
    Ok(())
}

/// `sc->sc_smr[i]`.
fn smmu_smr_slot(sc: &SmmuSoftc, i: i32) -> &'static Cell<Option<&'static SmmuSmr>> {
    match sc.sc_smr.get() {
        Some(smr) => &smr[i as usize],
        None => panic(format_args!("smmu: no stream match registers")),
    }
}

/// `sc->sc_cb[i]`.
fn smmu_cb_slot(sc: &SmmuSoftc, i: i32) -> &'static Cell<Option<&'static SmmuCb>> {
    match sc.sc_cb.get() {
        Some(cb) => &cb[i as usize],
        None => panic(format_args!("smmu: no context banks")),
    }
}

/// `smmu_v2_global_irq(cookie)`: reports and clears a global fault.
pub fn smmu_v2_global_irq(cookie: *mut c_void) -> i32 {
    // SAFETY: the attachments establish this handler with their softc as the cookie.
    let sc = unsafe { &*cookie.cast_const().cast::<SmmuSoftc>() };

    let reg = smmu_gr0_read_4(sc, SMMU_SGFSR);
    if reg == 0 {
        return 0;
    }

    printf(format_args!(
        "{}: SGFSR 0x{:08x} SGFSYNR0 0x{:08x} SGFSYNR1 0x{:08x} SGFSYNR2 0x{:08x}\n",
        sc.sc_dev.xname(),
        reg,
        smmu_gr0_read_4(sc, SMMU_SGFSYNR0),
        smmu_gr0_read_4(sc, SMMU_SGFSYNR1),
        smmu_gr0_read_4(sc, SMMU_SGFSYNR2)
    ));

    smmu_gr0_write_4(sc, SMMU_SGFSR, reg);

    1
}

/// `smmu_v2_context_irq(cookie)`: reports and clears a context bank's fault.
pub fn smmu_v2_context_irq(cookie: *mut c_void) -> i32 {
    // SAFETY: the attachments establish this handler with a leaked `SmmuCbIrq` as the cookie.
    let cbi = unsafe { &*cookie.cast_const().cast::<SmmuCbIrq>() };
    let sc = cbi.cbi_sc;

    let reg = smmu_cb_read_4(sc, cbi.cbi_idx, SMMU_CB_FSR);
    if reg & SMMU_CB_FSR_MASK == 0 {
        return 0;
    }

    printf(format_args!(
        "{}: FSR 0x{:08x} FSYNR0 0x{:08x} FAR 0x{:x} CBFRSYNRA 0x{:08x}\n",
        sc.sc_dev.xname(),
        reg,
        smmu_cb_read_4(sc, cbi.cbi_idx, SMMU_CB_FSYNR0),
        smmu_cb_read_8(sc, cbi.cbi_idx, SMMU_CB_FAR),
        smmu_gr1_read_4(sc, smmu_cbfrsynra(cbi.cbi_idx as usize))
    ));

    smmu_cb_write_4(sc, cbi.cbi_idx, SMMU_CB_FSR, reg);

    1
}

/// `smmu_v2_tlb_sync_global(sc)`.
fn smmu_v2_tlb_sync_global(sc: &SmmuSoftc) {
    smmu_gr0_write_4(sc, SMMU_STLBGSYNC, !0);
    for _ in 0..1000 {
        if smmu_gr0_read_4(sc, SMMU_STLBGSTATUS) & SMMU_STLBGSTATUS_GSACTIVE == 0 {
            return;
        }
    }

    printf(format_args!(
        "{}: global TLB sync timeout\n",
        sc.sc_dev.xname()
    ));
}

/// `smmu_v2_tlb_sync_context(dom)`.
fn smmu_v2_tlb_sync_context(dom: &SmmuDomain) {
    let sc = dom.sd_sc;

    smmu_cb_write_4(sc, dom.sd_cb_idx.get(), SMMU_CB_TLBSYNC, !0);
    for _ in 0..1000 {
        if smmu_cb_read_4(sc, dom.sd_cb_idx.get(), SMMU_CB_TLBSTATUS) & SMMU_CB_TLBSTATUS_SACTIVE
            == 0
        {
            return;
        }
    }

    printf(format_args!(
        "{}: context TLB sync timeout\n",
        sc.sc_dev.xname()
    ));
}

/// `smmu_gr0_read_4(sc, off)`: global register space 0.
fn smmu_gr0_read_4(sc: &SmmuSoftc, off: BusSize) -> u32 {
    bus_space_read_4(sc.iot(), sc.ioh(), off)
}

/// `smmu_gr0_write_4(sc, off, val)`.
fn smmu_gr0_write_4(sc: &SmmuSoftc, off: BusSize, val: u32) {
    bus_space_write_4(sc.iot(), sc.ioh(), off, val);
}

/// `smmu_gr1_read_4(sc, off)`: global register space 1, one page in.
fn smmu_gr1_read_4(sc: &SmmuSoftc, off: BusSize) -> u32 {
    let base = sc.sc_pagesize.get();
    bus_space_read_4(sc.iot(), sc.ioh(), base + off)
}

/// `smmu_gr1_write_4(sc, off, val)`.
fn smmu_gr1_write_4(sc: &SmmuSoftc, off: BusSize, val: u32) {
    let base = sc.sc_pagesize.get();
    bus_space_write_4(sc.iot(), sc.ioh(), base + off, val);
}

/// The base of context bank `idx`: `SMMU_CB_BASE` (after the global pages) plus `idx` pages.
fn smmu_cb_base(sc: &SmmuSoftc, idx: i32) -> usize {
    let mut base = sc.sc_numpage.get() as usize * sc.sc_pagesize.get(); // SMMU_CB_BASE
    base += idx as usize * sc.sc_pagesize.get(); // SMMU_CBn_BASE
    base
}

/// `smmu_cb_read_4(sc, idx, off)`.
fn smmu_cb_read_4(sc: &SmmuSoftc, idx: i32, off: BusSize) -> u32 {
    bus_space_read_4(sc.iot(), sc.ioh(), smmu_cb_base(sc, idx) + off)
}

/// `smmu_cb_write_4(sc, idx, off, val)`.
fn smmu_cb_write_4(sc: &SmmuSoftc, idx: i32, off: BusSize, val: u32) {
    bus_space_write_4(sc.iot(), sc.ioh(), smmu_cb_base(sc, idx) + off, val);
}

/// `smmu_cb_read_8(sc, idx, off)`: two 32-bit reads on the AP806.
fn smmu_cb_read_8(sc: &SmmuSoftc, idx: i32, off: BusSize) -> u64 {
    let base = smmu_cb_base(sc, idx);

    if sc.sc_is_ap806.get() != 0 {
        let mut reg = u64::from(bus_space_read_4(sc.iot(), sc.ioh(), base + off + 4));
        reg <<= 32;
        reg |= u64::from(bus_space_read_4(sc.iot(), sc.ioh(), base + off));
        return reg;
    }

    let t = sc.iot();
    (t._space_read_8)(t, sc.ioh(), base + off)
}

/// `smmu_cb_write_8(sc, idx, off, val)`: two 32-bit writes on the AP806.
fn smmu_cb_write_8(sc: &SmmuSoftc, idx: i32, off: BusSize, val: u64) {
    let base = smmu_cb_base(sc, idx);

    if sc.sc_is_ap806.get() != 0 {
        bus_space_write_4(sc.iot(), sc.ioh(), base + off + 4, (val >> 32) as u32);
        bus_space_write_4(sc.iot(), sc.ioh(), base + off, val as u32);
        return;
    }

    let t = sc.iot();
    (t._space_write_8)(t, sc.ioh(), base + off, val);
}

/// `smmu_device_map(cookie, sid, dmat)`: the DMA tag of stream `sid`: a copy of `dmat`
/// whose map functions go through the stream's domain, made once per domain. `dmat` comes
/// back when the domain cannot be set up.
pub fn smmu_device_map(
    cookie: *mut c_void,
    sid: u32,
    dmat: Option<BusDmaTagT>,
) -> Option<BusDmaTagT> {
    // SAFETY: the attachments register their softc as the cookie; it is never detached.
    let sc: &'static SmmuSoftc = unsafe { &*cookie.cast_const().cast::<SmmuSoftc>() };

    let Some(dom) = smmu_domain_lookup(sc, sid) else {
        return dmat;
    };

    if dom.sd_dmat.get().is_none() {
        let dmat = dmat?;
        let tag: &'static BusDmaTag = Box::leak(Box::new(BusDmaTag {
            _cookie: ptr::from_ref(dom).cast_mut().cast::<c_void>(),
            _dmamap_create: smmu_dmamap_create,
            _dmamap_destroy: smmu_dmamap_destroy,
            _dmamap_load: smmu_dmamap_load,
            _dmamap_load_mbuf: smmu_dmamap_load_mbuf,
            _dmamap_load_uio: smmu_dmamap_load_uio,
            _dmamap_load_raw: smmu_dmamap_load_raw,
            _dmamap_unload: smmu_dmamap_unload,
            ..*dmat
        }));
        dom.sd_dmat.set(Some(tag));
    }

    dom.sd_dmat.get()
}

/// `smmu_domain_lookup(sc, sid)`: the domain of stream `sid`, created on first use.
fn smmu_domain_lookup(sc: &'static SmmuSoftc, sid: u32) -> Option<&'static SmmuDomain> {
    if let Some(dom) = sc.sc_domains.iter().find(|dom| dom.sd_sid == sid) {
        return Some(dom);
    }

    smmu_domain_create(sc, sid)
}

/// `smmu_domain_create(sc, sid)`: a domain for stream `sid`, with its translation set up
/// and the first page of its IOVA space reserved.
fn smmu_domain_create(sc: &'static SmmuSoftc, sid: u32) -> Option<&'static SmmuDomain> {
    let create = sc.sc_domain_create.get()?;
    let dom: &'static SmmuDomain = Box::leak(Box::new(SmmuDomain {
        sd_sc: sc,
        sd_sid: sid,
        sd_dmat: Cell::new(None),
        sd_cb_idx: Cell::new(0),
        sd_smr_idx: Cell::new(0),
        v3: SmmuDomainV3 {
            sd_cd: Cell::new(None),
            sd_asid: Cell::new(0),
        },
        sd_stage: Cell::new(0),
        sd_4level: Cell::new(0),
        sd_exname: Cell::new(b""),
        sd_iovamap: Cell::new(None),
        sd_vp_l0: Cell::new(None),
        sd_vp_l1: Cell::new(None),
        sd_iova_mtx: Mutex::new(IPL_VM),
        sd_pmap_mtx: Mutex::new(IPL_VM),
        sd_list: SimpleqEntry::new(),
    }));
    mtx_init(&dom.sd_iova_mtx, IPL_VM);
    mtx_init(&dom.sd_pmap_mtx, IPL_VM);

    // Prefer stage 1 if possible!
    if sc.sc_has_s1.get() != 0 {
        dom.sd_stage.set(1);
    } else {
        dom.sd_stage.set(2);
    }

    if create(dom).is_err() {
        // SAFETY: the box leaked above; a failed creation stored no reference to the domain
        // (the stream match registers take it only on success), so this is the only one.
        drop(unsafe { Box::from_raw(ptr::from_ref(dom).cast_mut()) });
        return None;
    }

    // Reserve first page (to catch NULL access)
    let _ = extent_alloc_region(dom.iovamap(), 0, PAGE_SIZE as u64, EX_WAITOK);

    // SAFETY: a fresh domain in no queue, never freed once queued.
    unsafe { sc.sc_domains.insert_tail(dom) };
    Some(dom)
}

/// The domain's I/O virtual address map, named `"<smmu>:<sid>"` (`sd_exname`).
fn smmu_domain_iovamap(dom: &SmmuDomain, iovabits: u32) -> Result<(), Errno> {
    let mut name = format!("{}:{:x}", dom.sd_sc.sc_dev.xname(), dom.sd_sid).into_bytes();
    name.truncate(31); // snprintf into char sd_exname[32]
    let name: &'static [u8] = Box::leak(name.into_boxed_slice());
    dom.sd_exname.set(name);
    let ex = extent_create(
        name,
        0,
        (1u64 << iovabits) - 1,
        M_DEVBUF,
        None,
        EX_WAITOK | EX_NOCOALESCE,
    )
    .ok_or(Errno::ENOMEM)?;
    dom.sd_iovamap.set(Some(ex));
    Ok(())
}

/// The domain's root page table (`sd_vp.l0` or `sd_vp.l1`), allocated, and its physical
/// address for the TTBR or the context descriptor.
fn smmu_domain_root(dom: &SmmuDomain) -> usize {
    let sc = dom.sd_sc;
    let pp = smmu_pool(sc, false);

    let l0va = if dom.sd_4level.get() != 0 {
        let l0 = loop {
            if let Some(vp) = smmu_vp_get::<SmmuVp0>(pp, PR_WAITOK) {
                break vp;
            }
        };
        dom.sd_vp_l0.set(Some(l0));
        l0.l0.as_ptr() as usize // top level is l0
    } else {
        let l1 = loop {
            if let Some(vp) = smmu_vp_get::<SmmuVp1>(pp, PR_WAITOK) {
                break vp;
            }
        };
        dom.sd_vp_l1.set(Some(l1));
        l1.l1.as_ptr() as usize // top level is l1
    };
    match smmu_vp_pa(l0va) {
        Some(pa) => pa,
        None => panic(format_args!(
            "smmu_domain_root: unable to find vp pa mapping {:#x}",
            l0va
        )),
    }
}

/// `smmu_v2_domain_create(dom)`: takes a context bank (and a stream match register),
/// programs the bank with the domain's tables and points the stream at it.
fn smmu_v2_domain_create(dom: &'static SmmuDomain) -> Result<(), Errno> {
    let sc = dom.sd_sc;

    let (start, end) = if dom.sd_stage.get() == 1 {
        (
            sc.sc_num_s2_context_banks.get(),
            sc.sc_num_context_banks.get(),
        )
    } else {
        (0, sc.sc_num_context_banks.get())
    };

    let Some(cb) = (start..end).find(|&i| smmu_cb_slot(sc, i).get().is_none()) else {
        printf(format_args!(
            "{}: out of context blocks, I/O device will fail\n",
            sc.sc_dev.xname()
        ));
        return Err(Errno::ENXIO);
    };
    smmu_cb_slot(sc, cb).set(Some(&SMMU_CB));
    dom.sd_cb_idx.set(cb);

    // Stream indexing is easy
    dom.sd_smr_idx.set(dom.sd_sid as i32);

    // Stream mapping is a bit more effort
    if sc.sc_smr.get().is_some() {
        let mut found = false;
        for i in 0..sc.sc_num_streams.get() {
            let slot = smmu_smr_slot(sc, i);
            // Take over QCOM SMRs
            if sc.sc_is_qcom.get() != 0
                && let Some(smr) = slot.get()
                && smr.ss_dom.get().is_none()
                && (u32::from(smr.ss_id.get()) ^ dom.sd_sid) & !u32::from(smr.ss_mask.get()) == 0
            {
                smr.ss_dom.set(Some(dom));
                dom.sd_smr_idx.set(i);
                found = true;
                break;
            }
            if slot.get().is_some() {
                continue;
            }
            slot.set(Some(Box::leak(Box::new(SmmuSmr {
                ss_dom: Cell::new(Some(dom)),
                ss_id: Cell::new(dom.sd_sid as u16),
                ss_mask: Cell::new(0),
            }))));
            dom.sd_smr_idx.set(i);
            found = true;
            break;
        }

        if !found {
            smmu_cb_slot(sc, dom.sd_cb_idx.get()).set(None);
            printf(format_args!(
                "{}: out of streams, I/O device will fail\n",
                sc.sc_dev.xname()
            ));
            return Err(Errno::ENXIO);
        }
    }

    let cb_idx = dom.sd_cb_idx.get();
    let mut reg = SMMU_CBA2R_VA64;
    if sc.sc_has_vmid16s.get() != 0 {
        reg |= ((cb_idx + 1) as u32) << SMMU_CBA2R_VMID16_SHIFT;
    }
    smmu_gr1_write_4(sc, smmu_cba2r(cb_idx as usize), reg);

    if dom.sd_stage.get() == 1 {
        reg = SMMU_CBAR_TYPE_S1_TRANS_S2_BYPASS | SMMU_CBAR_BPSHCFG_NSH | SMMU_CBAR_MEMATTR_WB;
    } else {
        reg = SMMU_CBAR_TYPE_S2_TRANS;
        if sc.sc_has_vmid16s.get() == 0 {
            reg |= ((cb_idx + 1) as u32) << SMMU_CBAR_VMID_SHIFT;
        }
    }
    smmu_gr1_write_4(sc, smmu_cbar(cb_idx as usize), reg);

    if dom.sd_stage.get() == 1 {
        reg = SMMU_CB_TCR2_AS | SMMU_CB_TCR2_SEP_UPSTREAM;
        reg |= match sc.sc_ipa_bits.get() {
            32 => SMMU_CB_TCR2_PASIZE_32BIT,
            36 => SMMU_CB_TCR2_PASIZE_36BIT,
            40 => SMMU_CB_TCR2_PASIZE_40BIT,
            42 => SMMU_CB_TCR2_PASIZE_42BIT,
            44 => SMMU_CB_TCR2_PASIZE_44BIT,
            48 => SMMU_CB_TCR2_PASIZE_48BIT,
            _ => 0,
        };
        smmu_cb_write_4(sc, cb_idx, SMMU_CB_TCR2, reg);
    }

    let mut iovabits = if dom.sd_stage.get() == 1 {
        sc.sc_va_bits.get()
    } else {
        sc.sc_ipa_bits.get()
    } as u32;
    // Marvell's 8040 does not support 64-bit writes, hence we can only address 44-bits of
    // VA space for TLB invalidation.
    if sc.sc_is_ap806.get() != 0 {
        iovabits = iovabits.min(44);
    }
    if iovabits >= 40 {
        dom.sd_4level.set(1);
    }

    reg = SMMU_CB_TCR_TG0_4KB | smmu_cb_tcr_t0sz(64 - iovabits);
    if dom.sd_stage.get() == 1 {
        reg |= SMMU_CB_TCR_EPD1;
    } else {
        if dom.sd_4level.get() != 0 {
            reg |= SMMU_CB_TCR_S2_SL0_4KB_L0;
        } else {
            reg |= SMMU_CB_TCR_S2_SL0_4KB_L1;
        }
        reg |= match sc.sc_pa_bits.get() {
            32 => SMMU_CB_TCR_S2_PASIZE_32BIT,
            36 => SMMU_CB_TCR_S2_PASIZE_36BIT,
            40 => SMMU_CB_TCR_S2_PASIZE_40BIT,
            42 => SMMU_CB_TCR_S2_PASIZE_42BIT,
            44 => SMMU_CB_TCR_S2_PASIZE_44BIT,
            48 => SMMU_CB_TCR_S2_PASIZE_48BIT,
            _ => 0,
        };
    }
    if sc.sc_coherent.get() != 0 {
        reg |= SMMU_CB_TCR_IRGN0_WBWA | SMMU_CB_TCR_ORGN0_WBWA | SMMU_CB_TCR_SH0_ISH;
    } else {
        reg |= SMMU_CB_TCR_IRGN0_NC | SMMU_CB_TCR_ORGN0_NC | SMMU_CB_TCR_SH0_OSH;
    }
    smmu_cb_write_4(sc, cb_idx, SMMU_CB_TCR, reg);

    let pa = smmu_domain_root(dom) as u64;

    if dom.sd_stage.get() == 1 {
        smmu_cb_write_8(
            sc,
            cb_idx,
            SMMU_CB_TTBR0,
            ((cb_idx as u64) << SMMU_CB_TTBR_ASID_SHIFT) | pa,
        );
        smmu_cb_write_8(
            sc,
            cb_idx,
            SMMU_CB_TTBR1,
            (cb_idx as u64) << SMMU_CB_TTBR_ASID_SHIFT,
        );
    } else {
        smmu_cb_write_8(sc, cb_idx, SMMU_CB_TTBR0, pa);
    }

    if dom.sd_stage.get() == 1 {
        smmu_cb_write_4(
            sc,
            cb_idx,
            SMMU_CB_MAIR0,
            smmu_cb_mair_mair_attr(SMMU_CB_MAIR_DEVICE_nGnRnE, 0)
                | smmu_cb_mair_mair_attr(SMMU_CB_MAIR_DEVICE_nGnRE, 1)
                | smmu_cb_mair_mair_attr(SMMU_CB_MAIR_DEVICE_NC, 2)
                | smmu_cb_mair_mair_attr(SMMU_CB_MAIR_DEVICE_WB, 3),
        );
        smmu_cb_write_4(
            sc,
            cb_idx,
            SMMU_CB_MAIR1,
            smmu_cb_mair_mair_attr(SMMU_CB_MAIR_DEVICE_WT, 0),
        );
    }

    reg = SMMU_CB_SCTLR_M
        | SMMU_CB_SCTLR_TRE
        | SMMU_CB_SCTLR_AFE
        | SMMU_CB_SCTLR_CFRE
        | SMMU_CB_SCTLR_CFIE;
    if dom.sd_stage.get() == 1 {
        reg |= SMMU_CB_SCTLR_ASIDPNE;
    }
    smmu_cb_write_4(sc, cb_idx, SMMU_CB_SCTLR, reg);

    // Point stream to context block
    let smr_idx = dom.sd_smr_idx.get();
    reg = SMMU_S2CR_TYPE_TRANS | cb_idx as u32;
    if sc.sc_has_exids.get() != 0 && sc.sc_smr.get().is_some() {
        reg |= SMMU_S2CR_EXIDVALID;
    }
    smmu_gr0_write_4(sc, smmu_s2cr(smr_idx as usize), reg);

    // Map stream idx to S2CR idx
    if sc.sc_smr.get().is_some() {
        let (id, mask) = match smmu_smr_slot(sc, smr_idx).get() {
            Some(smr) => (u32::from(smr.ss_id.get()), u32::from(smr.ss_mask.get())),
            None => (0, 0),
        };
        reg = (id << SMMU_SMR_ID_SHIFT) | (mask << SMMU_SMR_MASK_SHIFT);
        if sc.sc_has_exids.get() == 0 {
            reg |= SMMU_SMR_VALID;
        }
        smmu_gr0_write_4(sc, smmu_smr(smr_idx as usize), reg);
    }

    smmu_domain_iovamap(dom, iovabits)
}

/// `smmu_reserve_region(cookie, sid, addr, size)`: keeps `[addr, addr + size)` (a PCI
/// window, an MSI doorbell) out of stream `sid`'s IOVA space, clamped to it.
pub fn smmu_reserve_region(cookie: *mut c_void, sid: u32, addr: BusAddr, size: BusSize) {
    // SAFETY: as in `smmu_device_map`.
    let sc: &'static SmmuSoftc = unsafe { &*cookie.cast_const().cast::<SmmuSoftc>() };

    let Some(dom) = smmu_domain_lookup(sc, sid) else {
        return;
    };
    let ex = dom.iovamap();
    let addr = addr as u64;
    let mut size = size as u64;

    // Some reserved regions, like PCI BARs, might not be inside our VA map. If it lies
    // outside completely: skip it; if partially: clamp it.
    if addr > ex.ex_end {
        return;
    }
    if addr.wrapping_add(size) > ex.ex_end {
        size = (ex.ex_end - addr) + 1;
    }

    let _ = extent_alloc_region(ex, addr, size, EX_WAITOK | EX_CONFLICTOK);
}

// basically pmap follows

/// `VP_IDX0(va)`.
const fn vp_idx0(va: usize) -> usize {
    (va >> VP_IDX0_POS) & VP_IDX0_MASK
}

/// `VP_IDX1(va)`.
const fn vp_idx1(va: usize) -> usize {
    (va >> VP_IDX1_POS) & VP_IDX1_MASK
}

/// `VP_IDX2(va)`.
const fn vp_idx2(va: usize) -> usize {
    (va >> VP_IDX2_POS) & VP_IDX2_MASK
}

/// `VP_IDX3(va)`.
const fn vp_idx3(va: usize) -> usize {
    (va >> VP_IDX3_POS) & VP_IDX3_MASK
}

/// `VP_Lx(pa)`: a table descriptor for the table at `pa`.
const fn vp_lx(pa: usize) -> u64 {
    pa as u64 | Lx_TYPE_PT
}

/// The table descriptor of the page-table page at `va`; panics as `smmu_set_l*` do when the
/// page is not mapped or misaligned.
fn smmu_table_entry(func: &str, va: usize) -> u64 {
    let Some(pa) = smmu_vp_pa(va) else {
        panic(format_args!("{func}: unable to find vp pa mapping {va:#x}"));
    };

    if pa & (Lx_TABLE_ALIGN - 1) != 0 {
        panic(format_args!("{func}: misaligned L2 table"));
    }

    vp_lx(pa)
}

/// Makes a descriptor the SMMU walks visible to it.
fn smmu_pte_sync(sc: &SmmuSoftc, pte: &AtomicU64) {
    fence(Ordering::Release); // membar_producer(): XXX bus dma sync?
    if sc.sc_coherent.get() == 0 {
        cpu_dcache_wb_range(ptr::from_ref(pte) as usize, size_of::<u64>());
    }
}

/// `smmu_set_l1(dom, va, l1_va)`: links `l1_va` into the 4-level root for `va`.
fn smmu_set_l1(dom: &SmmuDomain, va: usize, l1_va: &'static SmmuVp1) {
    let pg_entry = smmu_table_entry("smmu_set_l1", l1_va.l1.as_ptr() as usize);
    let Some(l0) = dom.sd_vp_l0.get() else {
        panic(format_args!("smmu_set_l1: no l0"));
    };

    let idx0 = vp_idx0(va);
    l0.vp[idx0].store(ptr::from_ref(l1_va).cast_mut(), Ordering::Release);
    l0.l0[idx0].store(pg_entry, Ordering::Relaxed);
    smmu_pte_sync(dom.sd_sc, &l0.l0[idx0]);
}

/// `smmu_set_l2(dom, va, vp1, l2_va)`.
fn smmu_set_l2(dom: &SmmuDomain, va: usize, vp1: &SmmuVp1, l2_va: &'static SmmuVp2) {
    let pg_entry = smmu_table_entry("smmu_set_l2", l2_va.l2.as_ptr() as usize);

    let idx1 = vp_idx1(va);
    vp1.vp[idx1].store(ptr::from_ref(l2_va).cast_mut(), Ordering::Release);
    vp1.l1[idx1].store(pg_entry, Ordering::Relaxed);
    smmu_pte_sync(dom.sd_sc, &vp1.l1[idx1]);
}

/// `smmu_set_l3(dom, va, vp2, l3_va)`.
fn smmu_set_l3(dom: &SmmuDomain, va: usize, vp2: &SmmuVp2, l3_va: &'static SmmuVp3) {
    let pg_entry = smmu_table_entry("smmu_set_l3", l3_va.l3.as_ptr() as usize);

    let idx2 = vp_idx2(va);
    vp2.vp[idx2].store(ptr::from_ref(l3_va).cast_mut(), Ordering::Release);
    vp2.l2[idx2].store(pg_entry, Ordering::Relaxed);
    smmu_pte_sync(dom.sd_sc, &vp2.l2[idx2]);
}

/// The table a software link points at, `None` for a null link.
fn vp_next<T: SmmuVpTable>(link: &AtomicPtr<T>) -> Option<&'static T> {
    let p = link.load(Ordering::Acquire);
    // SAFETY: links are only set (`smmu_set_l*`, release) to pool pages that are never freed.
    unsafe { p.as_ref() }
}

/// The level-1 table for `va`: the root of a 3-level domain, or the 4-level root's entry.
fn smmu_vp1(dom: &SmmuDomain, va: usize) -> Option<&'static SmmuVp1> {
    if dom.sd_4level.get() != 0 {
        vp_next(&dom.sd_vp_l0.get()?.vp[vp_idx0(va)])
    } else {
        dom.sd_vp_l1.get()
    }
}

/// `smmu_vp_lookup(dom, va, &pl3entry)`: the L3 descriptor of `va`, `ENXIO` when a level
/// is missing.
fn smmu_vp_lookup(dom: &SmmuDomain, va: usize) -> Result<&'static AtomicU64, Errno> {
    let vp1 = smmu_vp1(dom, va).ok_or(Errno::ENXIO)?;
    let vp2 = vp_next(&vp1.vp[vp_idx1(va)]).ok_or(Errno::ENXIO)?;
    let vp3 = vp_next(&vp2.vp[vp_idx2(va)]).ok_or(Errno::ENXIO)?;

    Ok(&vp3.l3[vp_idx3(va)])
}

/// `smmu_vp_enter(dom, va, &pl3entry, flags)`: the L3 descriptor of `va`, adding the
/// missing levels under `sd_pmap_mtx`; `ENOMEM` when a pool is empty.
fn smmu_vp_enter(dom: &SmmuDomain, va: usize, _flags: i32) -> Result<&'static AtomicU64, Errno> {
    let sc = dom.sd_sc;

    let vp1 = if dom.sd_4level.get() != 0 {
        let Some(l0) = dom.sd_vp_l0.get() else {
            return Err(Errno::ENXIO);
        };
        match vp_next(&l0.vp[vp_idx0(va)]) {
            Some(vp1) => vp1,
            None => {
                mtx_enter(&dom.sd_pmap_mtx);
                let vp1 = match vp_next(&l0.vp[vp_idx0(va)]) {
                    Some(vp1) => vp1,
                    None => {
                        let Some(vp1) = smmu_vp_get::<SmmuVp1>(smmu_pool(sc, false), PR_NOWAIT)
                        else {
                            mtx_leave(&dom.sd_pmap_mtx);
                            return Err(Errno::ENOMEM);
                        };
                        smmu_set_l1(dom, va, vp1);
                        vp1
                    }
                };
                mtx_leave(&dom.sd_pmap_mtx);
                vp1
            }
        }
    } else {
        match dom.sd_vp_l1.get() {
            Some(vp1) => vp1,
            None => return Err(Errno::ENXIO),
        }
    };

    let vp2 = match vp_next(&vp1.vp[vp_idx1(va)]) {
        Some(vp2) => vp2,
        None => {
            mtx_enter(&dom.sd_pmap_mtx);
            let vp2 = match vp_next(&vp1.vp[vp_idx1(va)]) {
                Some(vp2) => vp2,
                None => {
                    let Some(vp2) = smmu_vp_get::<SmmuVp2>(smmu_pool(sc, false), PR_NOWAIT) else {
                        mtx_leave(&dom.sd_pmap_mtx);
                        return Err(Errno::ENOMEM);
                    };
                    smmu_set_l2(dom, va, vp1, vp2);
                    vp2
                }
            };
            mtx_leave(&dom.sd_pmap_mtx);
            vp2
        }
    };

    let vp3 = match vp_next(&vp2.vp[vp_idx2(va)]) {
        Some(vp3) => vp3,
        None => {
            mtx_enter(&dom.sd_pmap_mtx);
            let vp3 = match vp_next(&vp2.vp[vp_idx2(va)]) {
                Some(vp3) => vp3,
                None => {
                    let Some(vp3) = smmu_vp_get::<SmmuVp3>(smmu_pool(sc, true), PR_NOWAIT) else {
                        mtx_leave(&dom.sd_pmap_mtx);
                        return Err(Errno::ENOMEM);
                    };
                    smmu_set_l3(dom, va, vp2, vp3);
                    vp3
                }
            };
            mtx_leave(&dom.sd_pmap_mtx);
            vp3
        }
    };

    Ok(&vp3.l3[vp_idx3(va)])
}

/// `smmu_fill_pte(dom, va, pa, prot, flags, cache)`: the software form of a mapping: the
/// page, the cache mode and the access flags.
fn smmu_fill_pte(
    _dom: &SmmuDomain,
    _va: usize,
    pa: usize,
    _prot: i32,
    flags: i32,
    cache: i32,
) -> u64 {
    let mut pted = pa as u64 & PTE_RPGN;

    match cache {
        PMAP_CACHE_WB
        | PMAP_CACHE_WT
        | PMAP_CACHE_CI
        | PMAP_CACHE_DEV_NGNRNE
        | PMAP_CACHE_DEV_NGNRE => {}
        _ => panic(format_args!("smmu_fill_pte: invalid cache mode")),
    }

    pted |= cache as u64;
    pted |= (flags & (PROT_READ | PROT_WRITE | PROT_EXEC)) as u64;
    pted
}

/// The descriptor attributes of a domain's mapping in cache mode `cache` (a stage-1 MAIR
/// index or a stage-2 memory attribute, inner shareable).
fn smmu_pte_attr(dom: &SmmuDomain, cache: i32) -> u64 {
    let s1 = dom.sd_stage.get() == 1;
    let idx = match cache {
        // inner and outer writeback
        PMAP_CACHE_WB => {
            if s1 {
                SMMU_ATTR_WB
            } else {
                PTE_MEMATTR_WB
            }
        }
        // inner and outer writethrough
        PMAP_CACHE_WT => {
            if s1 {
                SMMU_ATTR_WT
            } else {
                PTE_MEMATTR_WT
            }
        }
        PMAP_CACHE_CI => {
            if s1 {
                SMMU_ATTR_CI
            } else {
                PTE_MEMATTR_CI
            }
        }
        PMAP_CACHE_DEV_NGNRNE => {
            if s1 {
                SMMU_ATTR_DEV_NGNRNE
            } else {
                PTE_MEMATTR_DEV_NGNRNE
            }
        }
        PMAP_CACHE_DEV_NGNRE => {
            if s1 {
                SMMU_ATTR_DEV_NGNRE
            } else {
                PTE_MEMATTR_DEV_NGNRE
            }
        }
        _ => panic(format_args!("smmu_pte_update: invalid cache mode")),
    };
    attr_idx(idx) | attr_sh(SH_INNER)
}

/// `smmu_pte_update(dom, pted, pl3)`: writes the hardware descriptor of `pted`.
fn smmu_pte_update(dom: &SmmuDomain, pted: u64, pl3: &AtomicU64) {
    let sc = dom.sd_sc;

    // see mair in locore.S
    let mut attr = smmu_pte_attr(dom, (pted & PMAP_CACHE_BITS as u64) as i32);

    let mut access_bits = ATTR_PXN | ATTR_AF;
    if dom.sd_stage.get() == 1 {
        attr |= ATTR_nG;
        access_bits |= attr_ap(1);
        if pted & PROT_READ as u64 != 0 && pted & PROT_WRITE as u64 == 0 {
            access_bits |= attr_ap(2);
        }
    } else {
        if pted & PROT_READ as u64 != 0 {
            access_bits |= attr_ap(1);
        }
        if pted & PROT_WRITE as u64 != 0 {
            access_bits |= attr_ap(2);
        }
    }

    let pte = (pted & PTE_RPGN) | attr | access_bits | L3_P;
    pl3.store(pte, Ordering::Relaxed);
    smmu_pte_sync(sc, pl3);
}

/// `smmu_pte_remove(dom, va)`: clears the descriptor of `va`, whose tables must exist.
fn smmu_pte_remove(dom: &SmmuDomain, va: usize) {
    // put entry into table
    // need to deal with ref/change here
    let Some(vp1) = smmu_vp1(dom, va) else {
        panic(format_args!(
            "smmu_pte_remove: missing the l1 for va {va:x} domain {:p}",
            dom
        ));
    };
    let Some(vp2) = vp_next(&vp1.vp[vp_idx1(va)]) else {
        panic(format_args!(
            "smmu_pte_remove: missing the l2 for va {va:x} domain {:p}",
            dom
        ));
    };
    let Some(vp3) = vp_next(&vp2.vp[vp_idx2(va)]) else {
        panic(format_args!(
            "smmu_pte_remove: missing the l3 for va {va:x} domain {:p}",
            dom
        ));
    };
    let pl3 = &vp3.l3[vp_idx3(va)];
    pl3.store(0, Ordering::Relaxed);
    smmu_pte_sync(dom.sd_sc, pl3);
}

/// `smmu_enter(dom, va, pa, prot, flags, cache)`: makes sure the tables for `va` exist and,
/// when `flags` has an access, maps `pa` there.
fn smmu_enter(
    dom: &SmmuDomain,
    va: usize,
    pa: usize,
    prot: i32,
    flags: i32,
    cache: i32,
) -> Result<(), Errno> {
    if smmu_vp_lookup(dom, va).is_err() && smmu_vp_enter(dom, va, flags).is_err() {
        return Err(Errno::ENOMEM);
    }

    if flags & (PROT_READ | PROT_WRITE | PROT_EXEC) != 0 {
        smmu_map(dom, va, pa, prot, flags, cache);
    }

    Ok(())
}

/// `smmu_map(dom, va, pa, prot, flags, cache)`: maps `pa` at the already-entered `va`.
fn smmu_map(dom: &SmmuDomain, va: usize, pa: usize, prot: i32, flags: i32, cache: i32) {
    // IOVA must already be allocated
    let ret = smmu_vp_lookup(dom, va);
    kassert!(ret.is_ok());
    let Ok(pl3) = ret else {
        panic(format_args!("smmu_map: va {va:#x} not entered"));
    };

    // Update PTED information for physical address
    let pted = smmu_fill_pte(dom, va, pa, prot, flags, cache);

    // Insert updated information
    smmu_pte_update(dom, pted, pl3);
}

/// `smmu_unmap(dom, va)`: unmaps `va` and invalidates its TLB entry.
fn smmu_unmap(dom: &SmmuDomain, va: usize) {
    let sc = dom.sd_sc;

    // IOVA must already be allocated
    kassert!(smmu_vp_lookup(dom, va).is_ok());

    // Remove mapping from pagetable
    smmu_pte_remove(dom, va);

    if let Some(tlbi_va) = sc.sc_tlbi_va.get() {
        tlbi_va(dom, va);
    }
}

/// `smmu_v2_tlbi_va(dom, va)`: invalidates the IOTLB entry of `va`.
fn smmu_v2_tlbi_va(dom: &SmmuDomain, va: usize) {
    let sc = dom.sd_sc;
    let cb_idx = dom.sd_cb_idx.get();

    // Invalidate IOTLB
    if dom.sd_stage.get() == 1 {
        smmu_cb_write_8(
            sc,
            cb_idx,
            SMMU_CB_TLBIVAL,
            ((cb_idx as u64) << 48) | (va >> PAGE_SHIFT) as u64,
        );
    } else {
        smmu_cb_write_8(sc, cb_idx, SMMU_CB_TLBIIPAS2L, (va >> PAGE_SHIFT) as u64);
    }
}

/// `smmu_remove(dom, va)`: the C's body is empty ("TODO: garbage collect page tables?").
fn smmu_remove(_dom: &SmmuDomain, _va: usize) {}

/// A map's IOVA state, set by `smmu_dmamap_create`.
fn smmu_map_state(map: &BusDmamap) -> &SmmuMapState {
    // SAFETY: maps of an SMMU tag are made by `smmu_dmamap_create`, which sets the cookie to
    // the map's state; it lives until `smmu_dmamap_destroy`.
    unsafe { &*map._dm_cookie.get().cast_const().cast::<SmmuMapState>() }
}

/// `smmu_load_map(dom, map)`: maps the loaded segments page by page from the map's first
/// IOVA, and rewrites each segment's `ds_addr` to its IOVA.
fn smmu_load_map(dom: &SmmuDomain, map: &BusDmamap) -> Result<(), Errno> {
    let sms = smmu_map_state(map);
    let nsegs = map.dm_nsegs.get() as usize;

    let mut maplen = 0usize;
    for seg in 0..nsegs {
        let s = map.seg(seg).get();
        let pa = s._ds_paddr;
        let off = pa - trunc_page(pa);
        maplen += round_page(s.ds_len + off);
    }
    kassert!(maplen as u64 <= sms.sms_len.get());

    let mut dva = sms.sms_dva.get() as usize;
    for seg in 0..nsegs {
        let mut s = map.seg(seg).get();
        let pa = s._ds_paddr;
        let off = pa - trunc_page(pa);
        let mut len = round_page(s.ds_len + off);

        s.ds_addr = dva + off;
        map.seg(seg).set(s);

        let mut pa = trunc_page(pa);
        while len > 0 {
            smmu_map(
                dom,
                dva,
                pa,
                PROT_READ | PROT_WRITE,
                PROT_READ | PROT_WRITE,
                PMAP_CACHE_WB,
            );

            dva += PAGE_SIZE;
            pa += PAGE_SIZE;
            len -= PAGE_SIZE;
            sms.sms_loaded.set(sms.sms_loaded.get() + PAGE_SIZE as u64);
        }
    }

    Ok(())
}

/// `smmu_unload_map(dom, map)`: unmaps what the last load mapped and syncs the TLB.
fn smmu_unload_map(dom: &SmmuDomain, map: &BusDmamap) {
    let sc = dom.sd_sc;
    let sms = smmu_map_state(map);

    if sms.sms_loaded.get() == 0 {
        return;
    }

    let mut dva = sms.sms_dva.get() as usize;
    let mut len = sms.sms_loaded.get() as usize;

    while len > 0 {
        smmu_unmap(dom, dva);

        dva += PAGE_SIZE;
        len -= PAGE_SIZE;
    }

    sms.sms_loaded.set(0);

    if let Some(sync) = sc.sc_tlb_sync_context.get() {
        sync(dom);
    }
}

/// The domain of an SMMU tag.
fn smmu_tag_domain(t: BusDmaTagT) -> &'static SmmuDomain {
    // SAFETY: SMMU tags are made only by `smmu_device_map`, with their domain as the cookie;
    // a domain that has a tag is never freed.
    unsafe { &*t._cookie.cast_const().cast::<SmmuDomain>() }
}

/// `smmu_dmamap_create(t, size, nsegments, maxsegsz, boundary, flags, &map)`: the parent's
/// map, with an IOVA range (plus a guard page) allocated and its page tables entered.
pub fn smmu_dmamap_create(
    t: BusDmaTagT,
    size: BusSize,
    nsegments: i32,
    maxsegsz: BusSize,
    boundary: BusSize,
    flags: i32,
) -> Result<BusDmamapT, Errno> {
    let dom = smmu_tag_domain(t);
    let sc = dom.sd_sc;
    let pt = sc.dmat();

    let map = (pt._dmamap_create)(pt, size, nsegments, maxsegsz, boundary, flags)?;

    let mflags = if flags & BUS_DMA_NOWAIT != 0 {
        M_NOWAIT | M_ZERO
    } else {
        M_WAITOK | M_ZERO
    };
    let Some(mem) = malloc(size_of::<SmmuMapState>(), M_DEVBUF, mflags) else {
        // SAFETY: the map was just created by the parent and handed to nobody.
        unsafe { (pt._dmamap_destroy)(pt, NonNull::from(map)) };
        return Err(Errno::ENOMEM);
    };
    let sms_ptr = mem.cast::<SmmuMapState>();
    // SAFETY: a fresh allocation of the state's size, aligned (malloc's power-of-two
    // chunks), written before use.
    unsafe {
        sms_ptr.write(SmmuMapState {
            sms_er: ExtentRegion::new(),
            sms_dva: Cell::new(0),
            sms_len: Cell::new(0),
            sms_loaded: Cell::new(0),
        });
    }
    // SAFETY: written above; freed only by `smmu_dmamap_destroy` (or below on failure).
    let sms: &SmmuMapState = unsafe { sms_ptr.as_ref() };

    // Approximation of maximum pages needed.
    let mut len = round_page(size) + nsegments as usize * PAGE_SIZE;

    // Allocate IOVA, and a guard page at the end.
    let ex = dom.iovamap();
    mtx_enter(&dom.sd_iova_mtx);
    // SAFETY: `sms_er` is in no list and lives at a fixed address until
    // `smmu_dmamap_destroy` frees the region (and only then the state).
    let r = unsafe {
        extent_alloc_with_descr(
            ex,
            (len + PAGE_SIZE) as u64,
            64 * 1024,
            0,
            0,
            EX_NOWAIT,
            &sms.sms_er,
        )
    };
    mtx_leave(&dom.sd_iova_mtx);
    let dva = match r {
        Ok(dva) => dva,
        Err(e) => {
            // SAFETY: as above.
            unsafe { (pt._dmamap_destroy)(pt, NonNull::from(map)) };
            free(mem, M_DEVBUF, size_of::<SmmuMapState>());
            return Err(e);
        }
    };

    sms.sms_dva.set(dva);
    sms.sms_len.set(len as u64);

    let mut dva = dva as usize;
    while len > 0 {
        let error = smmu_enter(
            dom,
            dva,
            dva,
            PROT_READ | PROT_WRITE,
            PROT_NONE,
            PMAP_CACHE_WB,
        );
        kassert!(error.is_ok()); // FIXME: rollback smmu_enter()
        dva += PAGE_SIZE;
        len -= PAGE_SIZE;
    }

    map._dm_cookie.set(sms_ptr.as_ptr().cast::<c_void>());
    Ok(map)
}

/// `smmu_dmamap_destroy(t, map)`: unloads, frees the IOVA range and the parent's map.
///
/// # Safety
///
/// `map` came from [`smmu_dmamap_create`] on `t` and is not used afterwards.
pub unsafe fn smmu_dmamap_destroy(t: BusDmaTagT, map: NonNull<BusDmamap>) {
    let dom = smmu_tag_domain(t);
    let sc = dom.sd_sc;
    let pt = sc.dmat();
    // SAFETY: the caller's guarantee: a live map.
    let m = unsafe { map.as_ref() };
    let sms = smmu_map_state(m);

    if sms.sms_loaded.get() != 0 {
        smmu_dmamap_unload(t, m);
    }

    let mut dva = sms.sms_dva.get() as usize;
    let mut len = sms.sms_len.get() as usize;

    while len > 0 {
        smmu_remove(dom, dva);
        dva += PAGE_SIZE;
        len -= PAGE_SIZE;
    }

    let ex = dom.iovamap();
    mtx_enter(&dom.sd_iova_mtx);
    let error = extent_free(
        ex,
        sms.sms_dva.get(),
        sms.sms_len.get() + PAGE_SIZE as u64,
        EX_NOWAIT,
    );
    mtx_leave(&dom.sd_iova_mtx);
    kassert!(error.is_ok());

    let sms_ptr = m._dm_cookie.get().cast::<u8>();
    if let Some(p) = NonNull::new(sms_ptr) {
        free(p, M_DEVBUF, size_of::<SmmuMapState>());
    }
    // SAFETY: the caller's guarantee, passed on to the parent that created the map.
    unsafe { (pt._dmamap_destroy)(pt, map) };
}

/// The four loads: the parent's load into `map`, then the IOVA mappings; a failed mapping
/// unloads the parent's.
fn smmu_dmamap_load_common(
    t: BusDmaTagT,
    map: &BusDmamap,
    load: impl FnOnce(BusDmaTagT) -> Result<(), Errno>,
) -> Result<(), Errno> {
    let dom = smmu_tag_domain(t);
    let sc = dom.sd_sc;
    let pt = sc.dmat();

    load(pt)?;

    let error = smmu_load_map(dom, map);
    if error.is_err() {
        (pt._dmamap_unload)(pt, map);
    }

    error
}

/// `smmu_dmamap_load(t, map, buf, buflen, p, flags)`.
///
/// # Safety
///
/// As for `machine::bus::bus_dmamap_load`.
pub unsafe fn smmu_dmamap_load(
    t: BusDmaTagT,
    map: &BusDmamap,
    buf: *mut u8,
    buflen: BusSize,
    p: Option<&Proc>,
    flags: i32,
) -> Result<(), Errno> {
    smmu_dmamap_load_common(t, map, |pt| {
        // SAFETY: the caller's guarantee, forwarded.
        unsafe { (pt._dmamap_load)(pt, map, buf, buflen, p, flags) }
    })
}

/// `smmu_dmamap_load_mbuf(t, map, m0, flags)`.
///
/// # Safety
///
/// As for `machine::bus::bus_dmamap_load_mbuf`.
pub unsafe fn smmu_dmamap_load_mbuf(
    t: BusDmaTagT,
    map: &BusDmamap,
    m0: &Mbuf,
    flags: i32,
) -> Result<(), Errno> {
    smmu_dmamap_load_common(t, map, |pt| {
        // SAFETY: the caller's guarantee, forwarded.
        unsafe { (pt._dmamap_load_mbuf)(pt, map, m0, flags) }
    })
}

/// `smmu_dmamap_load_uio(t, map, uio, flags)`.
///
/// # Safety
///
/// As for `machine::bus::bus_dmamap_load_uio`.
pub unsafe fn smmu_dmamap_load_uio(
    t: BusDmaTagT,
    map: &BusDmamap,
    uio: &Uio<'_>,
    flags: i32,
) -> Result<(), Errno> {
    smmu_dmamap_load_common(t, map, |pt| {
        // SAFETY: the caller's guarantee, forwarded.
        unsafe { (pt._dmamap_load_uio)(pt, map, uio, flags) }
    })
}

/// `smmu_dmamap_load_raw(t, map, segs, nsegs, size, flags)`.
///
/// # Safety
///
/// As for `machine::bus::bus_dmamap_load_raw`.
pub unsafe fn smmu_dmamap_load_raw(
    t: BusDmaTagT,
    map: &BusDmamap,
    segs: &[BusDmaSegment],
    size: BusSize,
    flags: i32,
) -> Result<(), Errno> {
    smmu_dmamap_load_common(t, map, |pt| {
        // SAFETY: the caller's guarantee, forwarded.
        unsafe { (pt._dmamap_load_raw)(pt, map, segs, size, flags) }
    })
}

/// `smmu_dmamap_unload(t, map)`.
pub fn smmu_dmamap_unload(t: BusDmaTagT, map: &BusDmamap) {
    let dom = smmu_tag_domain(t);
    let sc = dom.sd_sc;
    let pt = sc.dmat();

    smmu_unload_map(dom, map);
    (pt._dmamap_unload)(pt, map);
}

/// `SMMU_DMA_DVA(sdm)`: the buffer's device address.
fn smmu_dma_dva(sdm: &SmmuDmamem) -> u64 {
    sdm.sdm_map.seg(0).get().ds_addr as u64
}

/// `SMMU_DMA_KVA(sdm) + off` as a pointer to 64-bit words.
fn smmu_dma_kva64(sdm: &SmmuDmamem, off: usize) -> *mut u64 {
    kassert!(off + size_of::<u64>() <= sdm.sdm_size);
    // SAFETY: `off` is inside the buffer (the callers index their own tables and queues).
    unsafe { sdm.sdm_kva.as_ptr().add(off).cast::<u64>() }
}

/// `smmu_dmamem_alloc(dmat, size, align)`: a zeroed, mapped and loaded DMA buffer of one
/// segment.
pub fn smmu_dmamem_alloc(
    dmat: BusDmaTagT,
    size: usize,
    align: usize,
) -> Option<&'static SmmuDmamem> {
    let map = bus_dmamap_create(dmat, size, 1, size, 0, BUS_DMA_WAITOK | BUS_DMA_ALLOCNOW).ok()?;

    let mut seg = [BusDmaSegment::default()];
    let nsegs = match bus_dmamem_alloc(dmat, size, align, 0, &mut seg, BUS_DMA_WAITOK) {
        Ok(n) => n,
        Err(_) => {
            // SAFETY: the map was just created and is not used afterwards.
            unsafe { bus_dmamap_destroy(dmat, NonNull::from(map)) };
            return None;
        }
    };

    let kva = match bus_dmamem_map(dmat, &mut seg[..nsegs], size, BUS_DMA_WAITOK) {
        Ok(kva) => kva,
        Err(_) => {
            // SAFETY: the segment was just allocated, the map just created; neither is used
            // afterwards.
            unsafe {
                bus_dmamem_free(dmat, &seg[..nsegs]);
                bus_dmamap_destroy(dmat, NonNull::from(map));
            }
            return None;
        }
    };

    // SAFETY: `kva` maps `size` bytes this function owns.
    if unsafe { bus_dmamap_load(dmat, map, kva.as_ptr(), size, None, BUS_DMA_WAITOK) }.is_err() {
        // SAFETY: as above: unmapped, freed and destroyed once, never used afterwards.
        unsafe {
            bus_dmamem_unmap(dmat, kva, size);
            bus_dmamem_free(dmat, &seg[..nsegs]);
            bus_dmamap_destroy(dmat, NonNull::from(map));
        }
        return None;
    }

    // SAFETY: `kva` maps `size` writable bytes.
    unsafe { ptr::write_bytes(kva.as_ptr(), 0, size) };
    bus_dmamap_sync(dmat, map, 0, size, BUS_DMASYNC_PREWRITE);

    Some(Box::leak(Box::new(SmmuDmamem {
        sdm_map: map,
        sdm_seg: seg[0],
        sdm_size: size,
        sdm_kva: kva,
    })))
}

/// `smmu_dmamem_free(dmat, sdm)`.
///
/// # Safety
///
/// `sdm` came from [`smmu_dmamem_alloc`] on `dmat` and is not used afterwards.
pub unsafe fn smmu_dmamem_free(dmat: BusDmaTagT, sdm: &'static SmmuDmamem) {
    // SAFETY: the caller's guarantee: the buffer's mapping, segment, map and descriptor are
    // released once.
    unsafe {
        bus_dmamem_unmap(dmat, sdm.sdm_kva, sdm.sdm_size);
        bus_dmamem_free(dmat, core::slice::from_ref(&sdm.sdm_seg));
        bus_dmamap_destroy(dmat, NonNull::from(sdm.sdm_map));
        drop(Box::from_raw(ptr::from_ref(sdm).cast_mut()));
    }
}

/// The queue's `sq_sdm`, allocated by the attach.
fn smmu_v3_q_sdm(sq: &SmmuV3Queue) -> &'static SmmuDmamem {
    match sq.sq_sdm.get() {
        Some(sdm) => sdm,
        None => panic(format_args!("smmu: queue not allocated")),
    }
}

/// `SMMU_V3_Q_IDX(sq, p)`.
fn q_idx(sq: &SmmuV3Queue, p: u32) -> u32 {
    smmu_v3_q_idx(sq.sq_size_log2.get() as u32, p)
}

/// `SMMU_V3_Q_WRP(sq, p)`.
fn q_wrp(sq: &SmmuV3Queue, p: u32) -> u32 {
    smmu_v3_q_wrp(sq.sq_size_log2.get() as u32, p)
}

/// A queue of `2^log2` entries of `words` 64-bit words, aligned to its size.
fn smmu_v3_q_alloc(sc: &SmmuSoftc, sq: &SmmuV3Queue, words: usize) -> bool {
    let size = (1usize << sq.sq_size_log2.get()) * words * size_of::<u64>();
    let sdm = smmu_dmamem_alloc(sc.dmat(), size, size);
    sq.sq_sdm.set(sdm);
    sdm.is_some()
}

/// `smmu_v3_attach(sc)`: probes an SMMUv3, sets up its queues and stream table, and
/// enables it with every stream aborting until a domain is made for it.
pub fn smmu_v3_attach(sc: &'static SmmuSoftc) -> Result<(), Errno> {
    smmu_attach(sc).map_err(|_| Errno::ENXIO)?;

    let mut reg = smmu_v3_read_4(sc, SMMU_V3_IDR0);
    if reg & SMMU_V3_TTF_AA64 == 0 {
        printf(format_args!(": no support for AA64\n"));
        return Err(Errno::ENXIO);
    }
    if reg & SMMU_V3_IDR0_S1P != 0 {
        sc.sc_has_s1.set(1);
    }
    if reg & SMMU_V3_IDR0_S2P != 0 {
        sc.sc_has_s2.set(1);
    }
    if reg & SMMU_V3_IDR0_ASID16 != 0 {
        sc.v3.sc_has_asid16s.set(1);
    }
    if reg & SMMU_V3_IDR0_PRI != 0 {
        sc.v3.sc_has_pri.set(1);
    }
    if reg & SMMU_V3_IDR0_VMID16 != 0 {
        sc.sc_has_vmid16s.set(1);
    }
    if reg & SMMU_V3_IDR0_CD2L != 0 {
        sc.v3.sc_2lvl_cdtab.set(1);
    }
    if smmu_v3_idr0_st_level(reg) == SMMU_V3_IDR0_ST_LEVEL_2 {
        sc.v3.sc_2lvl_strtab.set(1);
    }

    reg = smmu_v3_read_4(sc, SMMU_V3_IDR1);
    sc.v3
        .sc_cmdq
        .sq_size_log2
        .set(smmu_v3_idr1_cmdqs(reg) as i32);
    sc.v3
        .sc_eventq
        .sq_size_log2
        .set(smmu_v3_idr1_eventqs(reg) as i32);
    sc.v3
        .sc_priq
        .sq_size_log2
        .set(smmu_v3_idr1_priqs(reg) as i32);
    sc.v3.sc_sidsize.set(smmu_v3_idr1_sidsize(reg) as i32);
    if sc.v3.sc_sidsize.get() <= 8 {
        sc.v3.sc_2lvl_strtab.set(0);
    }

    reg = smmu_v3_read_4(sc, SMMU_V3_IDR5);
    sc.sc_pa_bits.set(match smmu_v3_idr5_oas(reg) {
        SMMU_V3_IDR5_OAS_32BIT => 32,
        SMMU_V3_IDR5_OAS_36BIT => 36,
        SMMU_V3_IDR5_OAS_40BIT => 40,
        SMMU_V3_IDR5_OAS_42BIT => 42,
        SMMU_V3_IDR5_OAS_44BIT => 44,
        SMMU_V3_IDR5_OAS_48BIT => 48,
        _ => 52,
    });
    sc.sc_va_bits.set(48);
    // `#if notyet`: SMMU_V3_IDR5_VAX would make it 52.
    // Unless there's no AA64, then it's 40.
    sc.sc_ipa_bits.set(sc.sc_pa_bits.get());

    // If IDR3.STT=0, maximum is 39.
    reg = smmu_v3_read_4(sc, SMMU_V3_IDR3);
    if reg & SMMU_V3_IDR3_STT == 0 {
        sc.sc_va_bits.set(sc.sc_va_bits.get().min(39));
        sc.sc_ipa_bits.set(sc.sc_ipa_bits.get().min(39));
    }

    mtx_init(&sc.v3.sc_cmdq_mtx, IPL_VM);
    if !smmu_v3_q_alloc(sc, &sc.v3.sc_cmdq, 2) {
        printf(format_args!(": can't allocate command queue\n"));
        return Err(Errno::ENXIO);
    }
    if !smmu_v3_q_alloc(sc, &sc.v3.sc_eventq, 4) {
        printf(format_args!(": can't allocate event queue\n"));
        smmu_v3_attach_unwind(sc, false, false);
        return Err(Errno::ENXIO);
    }
    if sc.v3.sc_has_pri.get() != 0 && !smmu_v3_q_alloc(sc, &sc.v3.sc_priq, 2) {
        printf(format_args!(": can't allocate pri queue\n"));
        smmu_v3_attach_unwind(sc, true, false);
        return Err(Errno::ENXIO);
    }

    // Abort transaction if already enabled.
    if smmu_v3_read_4(sc, SMMU_V3_CR0) & SMMU_V3_CR0_SMMUEN != 0 {
        reg = smmu_v3_read_4(sc, SMMU_V3_GBPA);
        reg |= SMMU_V3_GBPA_ABORT;
        smmu_v3_write_4(sc, SMMU_V3_GBPA, reg | SMMU_V3_GBPA_UPDATE);
        let updated =
            (0..100000).any(|_| smmu_v3_read_4(sc, SMMU_V3_GBPA) & SMMU_V3_GBPA_UPDATE == 0);
        if !updated {
            printf(format_args!(": failed waiting for update\n"));
            smmu_v3_attach_unwind(sc, true, true);
            return Err(Errno::ENXIO);
        }
    }

    // Disable SMMU
    let _ = smmu_v3_write_ack(sc, SMMU_V3_CR0, SMMU_V3_CR0ACK, 0);

    smmu_v3_write_4(
        sc,
        SMMU_V3_CR1,
        smmu_v3_cr1_table_sh(SMMU_V3_CR1_SHARE_ISH)
            | smmu_v3_cr1_table_oc(SMMU_V3_CR1_CACHE_WB)
            | smmu_v3_cr1_table_ic(SMMU_V3_CR1_CACHE_WB)
            | smmu_v3_cr1_queue_sh(SMMU_V3_CR1_SHARE_ISH)
            | smmu_v3_cr1_queue_oc(SMMU_V3_CR1_CACHE_WB)
            | smmu_v3_cr1_queue_ic(SMMU_V3_CR1_CACHE_WB),
    );
    smmu_v3_write_4(
        sc,
        SMMU_V3_CR2,
        SMMU_V3_CR2_PTM | SMMU_V3_CR2_RECINVSID | SMMU_V3_CR2_E2H,
    );

    let sidsize = sc.v3.sc_sidsize.get() as u32;
    if sc.v3.sc_2lvl_strtab.get() != 0 {
        let n = (1usize << sidsize) / 256;
        let l1size = n * size_of::<u64>();
        let Some(l1) = smmu_dmamem_alloc(sc.dmat(), l1size, l1size) else {
            printf(format_args!(": can't allocate strtab\n"));
            smmu_v3_attach_unwind(sc, true, true);
            return Err(Errno::ENXIO);
        };
        sc.v3.sc_strtab_l1.set(Some(l1));
        smmu_v3_write_8(
            sc,
            SMMU_V3_STRTAB_BASE,
            SMMU_V3_STRTAB_BASE_RA | smmu_dma_dva(l1),
        );
        smmu_v3_write_4(
            sc,
            SMMU_V3_STRTAB_BASE_CFG,
            SMMU_V3_STRTAB_BASE_CFG_FMT_L2
                | smmu_v3_strtab_base_cfg_split(8)
                | smmu_v3_strtab_base_cfg_log2size(u64::from(sidsize)) as u32,
        );
        sc.v3.sc_strtab_l2.set(Some(smmu_cells::<SmmuDmamem>(n)));
    } else {
        let size = (1usize << sidsize) * size_of::<[u64; 8]>();
        let Some(l1) = smmu_dmamem_alloc(sc.dmat(), size, size) else {
            printf(format_args!(": can't allocate strtab\n"));
            smmu_v3_attach_unwind(sc, true, true);
            return Err(Errno::ENXIO);
        };
        sc.v3.sc_strtab_l1.set(Some(l1));
        smmu_v3_write_8(
            sc,
            SMMU_V3_STRTAB_BASE,
            SMMU_V3_STRTAB_BASE_RA | smmu_dma_dva(l1),
        );
        smmu_v3_write_4(
            sc,
            SMMU_V3_STRTAB_BASE_CFG,
            SMMU_V3_STRTAB_BASE_CFG_FMT_L1
                | smmu_v3_strtab_base_cfg_log2size(u64::from(sidsize)) as u32,
        );
    }

    let cmdq = &sc.v3.sc_cmdq;
    smmu_v3_write_8(
        sc,
        SMMU_V3_CMDQ_BASE,
        SMMU_V3_CMDQ_BASE_RA
            | smmu_dma_dva(smmu_v3_q_sdm(cmdq))
            | smmu_v3_cmdq_base_log2size(cmdq.sq_size_log2.get() as u64),
    );
    smmu_v3_write_4(sc, SMMU_V3_CMDQ_PROD, 0);
    smmu_v3_write_4(sc, SMMU_V3_CMDQ_CONS, 0);
    let _ = smmu_v3_write_ack(sc, SMMU_V3_CR0, SMMU_V3_CR0ACK, SMMU_V3_CR0_CMDQEN);

    smmu_v3_cfgi_all(sc);
    smmu_v3_tlbi_all(sc, SMMU_V3_CMD_TLBI_EL2_ALL);
    smmu_v3_tlbi_all(sc, SMMU_V3_CMD_TLBI_NSNH_ALL);

    let evq = &sc.v3.sc_eventq;
    smmu_v3_write_8(
        sc,
        SMMU_V3_EVENTQ_BASE,
        SMMU_V3_EVENTQ_BASE_WA
            | smmu_dma_dva(smmu_v3_q_sdm(evq))
            | smmu_v3_eventq_base_log2size(evq.sq_size_log2.get() as u64),
    );
    smmu_v3_write_4(sc, SMMU_V3_EVENTQ_PROD, 0);
    smmu_v3_write_4(sc, SMMU_V3_EVENTQ_CONS, 0);
    let _ = smmu_v3_write_ack(
        sc,
        SMMU_V3_CR0,
        SMMU_V3_CR0ACK,
        smmu_v3_read_4(sc, SMMU_V3_CR0) | SMMU_V3_CR0_EVENTQEN,
    );

    if sc.v3.sc_has_pri.get() != 0 {
        let priq = &sc.v3.sc_priq;
        smmu_v3_write_8(
            sc,
            SMMU_V3_PRIQ_BASE,
            SMMU_V3_PRIQ_BASE_WA
                | smmu_dma_dva(smmu_v3_q_sdm(priq))
                | smmu_v3_priq_base_log2size(priq.sq_size_log2.get() as u64),
        );
        smmu_v3_write_4(sc, SMMU_V3_PRIQ_PROD, 0);
        smmu_v3_write_4(sc, SMMU_V3_PRIQ_CONS, 0);
        let _ = smmu_v3_write_ack(
            sc,
            SMMU_V3_CR0,
            SMMU_V3_CR0ACK,
            smmu_v3_read_4(sc, SMMU_V3_CR0) | SMMU_V3_CR0_PRIQEN,
        );
    }

    // Disable MSIs, use wired IRQs, re-enable IRQs.
    let _ = smmu_v3_write_ack(sc, SMMU_V3_IRQ_CTRL, SMMU_V3_IRQ_CTRLACK, 0);
    smmu_v3_write_8(sc, SMMU_V3_GERROR_IRQ_CFG0, 0);
    smmu_v3_write_8(sc, SMMU_V3_EVENTQ_IRQ_CFG0, 0);
    if sc.v3.sc_has_pri.get() != 0 {
        smmu_v3_write_8(sc, SMMU_V3_PRIQ_IRQ_CFG0, 0);
    }
    let pri = if sc.v3.sc_has_pri.get() != 0 {
        SMMU_V3_IRQ_CTRL_PRIQ
    } else {
        0
    };
    let _ = smmu_v3_write_ack(
        sc,
        SMMU_V3_IRQ_CTRL,
        SMMU_V3_IRQ_CTRLACK,
        SMMU_V3_IRQ_CTRL_GERROR | SMMU_V3_IRQ_CTRL_EVENTQ | pri,
    );

    let _ = smmu_v3_write_ack(
        sc,
        SMMU_V3_CR0,
        SMMU_V3_CR0ACK,
        smmu_v3_read_4(sc, SMMU_V3_CR0) | SMMU_V3_CR0_SMMUEN,
    );

    printf(format_args!("\n"));

    sc.sc_domain_create.set(Some(smmu_v3_domain_create));
    sc.sc_tlbi_va.set(Some(smmu_v3_tlbi_va));
    sc.sc_tlb_sync_context.set(Some(smmu_v3_tlb_sync_context));
    Ok(())
}

/// The C's `free_priq`/`free_evtq`/`free_cmdq` labels of `smmu_v3_attach`: frees the
/// command queue, and the event queue when `evtq`, and the PRI queue when `priq` (and PRI
/// is supported).
fn smmu_v3_attach_unwind(sc: &SmmuSoftc, evtq: bool, priq: bool) {
    let dmat = sc.dmat();
    let qs = [
        (&sc.v3.sc_priq, priq && sc.v3.sc_has_pri.get() != 0),
        (&sc.v3.sc_eventq, evtq),
        (&sc.v3.sc_cmdq, true),
    ];
    for (sq, free_it) in qs {
        if free_it && let Some(sdm) = sq.sq_sdm.take() {
            // SAFETY: allocated by this attach, referenced only by the queue just cleared.
            unsafe { smmu_dmamem_free(dmat, sdm) };
        }
    }
}

/// The queue walk of `smmu_v3_event_irq` and `smmu_v3_priq_irq`: prints each entry of
/// `words` 64-bit words with `what`, consumes it, then syncs the overflow flag.
fn smmu_v3_q_drain(
    sc: &SmmuSoftc,
    sq: &SmmuV3Queue,
    prod_reg: BusSize,
    cons_reg: BusSize,
    words: usize,
    what: &str,
) -> i32 {
    let mut handled = 0;
    let Some(sdm) = sq.sq_sdm.get() else {
        return 0;
    };

    loop {
        let prod = smmu_v3_read_4(sc, prod_reg);
        if smmu_v3_q_ovf(sq.sq_prod.get()) != smmu_v3_q_ovf(prod) {
            printf(format_args!(
                "{}: event queue overflow\n",
                sc.sc_dev.xname()
            ));
        }
        sq.sq_prod.set(prod);

        // Stop if empty.
        let cons = sq.sq_cons.get();
        if q_idx(sq, cons) == q_idx(sq, prod) && q_wrp(sq, cons) == q_wrp(sq, prod) {
            break;
        }

        // Print event information.
        let off = q_idx(sq, cons) as usize * words * size_of::<u64>();
        bus_dmamap_sync(
            sc.dmat(),
            sdm.sdm_map,
            off,
            words * size_of::<u64>(),
            BUS_DMASYNC_POSTWRITE,
        );
        let entry = smmu_dma_kva64(sdm, off);
        // SAFETY: the entry's `words` words are inside the queue buffer, which the device
        // wrote before moving `prod` past it.
        let e = |i: usize| unsafe { ptr::read_volatile(entry.add(i)) };
        if words == 4 {
            printf(format_args!(
                "{}: {} 0x{:x} 0x{:x} 0x{:x} 0x{:x}\n",
                sc.sc_dev.xname(),
                what,
                e(0),
                e(1),
                e(2),
                e(3)
            ));
        } else {
            printf(format_args!(
                "{}: {} 0x{:x} 0x{:x}\n",
                sc.sc_dev.xname(),
                what,
                e(0),
                e(1)
            ));
        }

        // Let HW know we consumed
        let next = (q_wrp(sq, cons) | q_idx(sq, cons)).wrapping_add(1);
        sq.sq_cons
            .set(smmu_v3_q_ovf(cons) | q_wrp(sq, next) | q_idx(sq, next));
        fence(Ordering::SeqCst); // membar_sync()
        smmu_v3_write_4(sc, cons_reg, sq.sq_cons.get());

        handled = 1;
    }

    // Sync overflow flag
    let cons = sq.sq_cons.get();
    if smmu_v3_q_ovf(sq.sq_prod.get()) != smmu_v3_q_ovf(cons) {
        sq.sq_cons
            .set(smmu_v3_q_ovf(sq.sq_prod.get()) | q_wrp(sq, cons) | q_idx(sq, cons));
        fence(Ordering::SeqCst); // membar_sync()
        smmu_v3_write_4(sc, cons_reg, sq.sq_cons.get());
    }

    handled
}

/// `smmu_v3_event_irq(cookie)`: prints the event queue's entries.
pub fn smmu_v3_event_irq(cookie: *mut c_void) -> i32 {
    // SAFETY: as in `smmu_v2_global_irq`.
    let sc = unsafe { &*cookie.cast_const().cast::<SmmuSoftc>() };
    smmu_v3_q_drain(
        sc,
        &sc.v3.sc_eventq,
        SMMU_V3_EVENTQ_PROD,
        SMMU_V3_EVENTQ_CONS,
        4,
        "event",
    )
}

/// `smmu_v3_gerr_irq(cookie)`: reports a global error and acknowledges it.
pub fn smmu_v3_gerr_irq(cookie: *mut c_void) -> i32 {
    // SAFETY: as in `smmu_v2_global_irq`.
    let sc = unsafe { &*cookie.cast_const().cast::<SmmuSoftc>() };

    let gerror = smmu_v3_read_4(sc, SMMU_V3_GERROR);
    let gerrorn = smmu_v3_read_4(sc, SMMU_V3_GERRORN);
    smmu_v3_write_4(sc, SMMU_V3_GERRORN, gerror);

    let gerror = (gerror ^ gerrorn) & SMMU_V3_GERROR_MASK;

    if gerror & SMMU_V3_GERROR_CMDQ_ERR != 0 {
        let cons = smmu_v3_read_4(sc, SMMU_V3_CMDQ_CONS);
        printf(format_args!(
            "{}: cmdq error 0x{:x} on cmd idx {}\n",
            sc.sc_dev.xname(),
            smmu_v3_cmdq_cons_err(cons),
            q_idx(&sc.v3.sc_cmdq, cons)
        ));
    } else {
        printf(format_args!(
            "{}: gerror 0x{:x}\n",
            sc.sc_dev.xname(),
            gerror
        ));
    }

    1
}

/// `smmu_v3_priq_irq(cookie)`: prints the PRI queue's entries.
pub fn smmu_v3_priq_irq(cookie: *mut c_void) -> i32 {
    // SAFETY: as in `smmu_v2_global_irq`.
    let sc = unsafe { &*cookie.cast_const().cast::<SmmuSoftc>() };
    smmu_v3_q_drain(
        sc,
        &sc.v3.sc_priq,
        SMMU_V3_PRIQ_PROD,
        SMMU_V3_PRIQ_CONS,
        2,
        "pri",
    )
}

/// `smmu_v3_read_4(sc, off)`.
fn smmu_v3_read_4(sc: &SmmuSoftc, off: BusSize) -> u32 {
    bus_space_read_4(sc.iot(), sc.ioh(), off)
}

/// `smmu_v3_write_4(sc, off, val)`.
fn smmu_v3_write_4(sc: &SmmuSoftc, off: BusSize, val: u32) {
    bus_space_write_4(sc.iot(), sc.ioh(), off, val);
}

/// `smmu_v3_read_8(sc, off)`.
#[allow(dead_code)] // the C's accessor, which nothing calls there either
fn smmu_v3_read_8(sc: &SmmuSoftc, off: BusSize) -> u64 {
    let t = sc.iot();
    (t._space_read_8)(t, sc.ioh(), off)
}

/// `smmu_v3_write_8(sc, off, val)`.
fn smmu_v3_write_8(sc: &SmmuSoftc, off: BusSize, val: u64) {
    let t = sc.iot();
    (t._space_write_8)(t, sc.ioh(), off, val);
}

/// `smmu_v3_write_ack(sc, off, ack_off, val)`: writes `val` and waits for the acknowledge
/// register to show it.
fn smmu_v3_write_ack(
    sc: &SmmuSoftc,
    off: BusSize,
    ack_off: BusSize,
    val: u32,
) -> Result<(), Errno> {
    smmu_v3_write_4(sc, off, val);

    if (0..100000).any(|_| smmu_v3_read_4(sc, ack_off) == val) {
        return Ok(());
    }

    printf(format_args!(
        "{}: failed waiting for ack\n",
        sc.sc_dev.xname()
    ));
    Err(Errno::ETIMEDOUT)
}

/// `smmu_v3_domain_create(dom)`: an ASID, a context descriptor pointing at the domain's
/// tables, and the stream's table entry pointing at the descriptor.
fn smmu_v3_domain_create(dom: &'static SmmuDomain) -> Result<(), Errno> {
    let sc = dom.sd_sc;

    if u64::from(dom.sd_sid) >= (1u64 << sc.v3.sc_sidsize.get()) {
        return Err(Errno::EINVAL);
    }

    if dom.sd_stage.get() != 1 {
        return Err(Errno::EINVAL);
    }

    let asid_max: u16 = if sc.v3.sc_has_asid16s.get() != 0 {
        u16::MAX
    } else {
        (1 << 8) - 1
    };
    if sc.v3.sc_next_asid.get() == asid_max {
        return Err(Errno::EINVAL);
    }
    dom.v3.sd_asid.set(sc.v3.sc_next_asid.get());
    sc.v3.sc_next_asid.set(sc.v3.sc_next_asid.get() + 1);

    let Some(cd) = smmu_dmamem_alloc(sc.dmat(), 64, 64) else {
        printf(format_args!(": can't allocate context descriptor\n"));
        return Err(Errno::ENOMEM);
    };
    dom.v3.sd_cd.set(Some(cd));

    let iovabits = if dom.sd_stage.get() == 1 {
        sc.sc_va_bits.get()
    } else {
        sc.sc_ipa_bits.get()
    } as u32;
    if iovabits >= 40 {
        dom.sd_4level.set(1);
    }

    let pa = smmu_domain_root(dom) as u64;

    let words = [
        smmu_v3_cd_0_tcr_t0sz(u64::from(64 - iovabits))
            | SMMU_V3_CD_0_TCR_TG0_4KB
            | SMMU_V3_CD_0_TCR_IRGN0_WBWA
            | SMMU_V3_CD_0_TCR_ORGN0_WBWA
            | SMMU_V3_CD_0_TCR_SH0_ISH
            | SMMU_V3_CD_0_TCR_EPD1
            | SMMU_V3_CD_0_V
            | SMMU_V3_CD_0_TCR_IPS_48BIT
            | SMMU_V3_CD_0_AA64
            | SMMU_V3_CD_0_R
            | SMMU_V3_CD_0_A
            | SMMU_V3_CD_0_ASET
            | smmu_v3_cd_0_asid(u64::from(dom.v3.sd_asid.get())),
        pa,
        0,
        smmu_v3_cd_3_mair_attr(SMMU_V3_CD_3_MAIR_DEVICE_nGnRnE, 0)
            | smmu_v3_cd_3_mair_attr(SMMU_V3_CD_3_MAIR_DEVICE_nGnRE, 1)
            | smmu_v3_cd_3_mair_attr(SMMU_V3_CD_3_MAIR_DEVICE_NC, 2)
            | smmu_v3_cd_3_mair_attr(SMMU_V3_CD_3_MAIR_DEVICE_WB, 3)
            | smmu_v3_cd_3_mair_attr(SMMU_V3_CD_3_MAIR_DEVICE_WT, 4),
    ];
    for (i, w) in words.into_iter().enumerate() {
        if i == 2 {
            continue; // cd[2] stays zero, as the C leaves it
        }
        // SAFETY: word `i` of the zeroed 64-byte descriptor this domain owns.
        unsafe { ptr::write_volatile(smmu_dma_kva64(cd, i * size_of::<u64>()), w) };
    }
    bus_dmamap_sync(sc.dmat(), cd.sdm_map, 0, cd.sdm_size, BUS_DMASYNC_PREWRITE);
    smmu_v3_cfgi_cd(dom);

    smmu_domain_iovamap(dom, iovabits)?;

    let Some(l1) = sc.v3.sc_strtab_l1.get() else {
        return Err(Errno::ENXIO);
    };
    let sid = dom.sd_sid as usize;
    let (ste_sdm, ste_off) = if sc.v3.sc_2lvl_strtab.get() != 0 {
        let Some(l2s) = sc.v3.sc_strtab_l2.get() else {
            return Err(Errno::ENXIO);
        };
        let slot = &l2s[sid / 256];
        if slot.get().is_none() {
            let Some(l2) = smmu_dmamem_alloc(sc.dmat(), 256 * size_of::<[u64; 8]>(), 64 * 1024)
            else {
                printf(format_args!(
                    "{}: can't allocate strtab\n",
                    sc.sc_dev.xname()
                ));
                return Err(Errno::ENXIO);
            };
            slot.set(Some(l2));
            let off = (sid / 256) * size_of::<u64>();
            // SAFETY: the L1 descriptor of this stream's group, inside the L1 table.
            unsafe {
                ptr::write_volatile(
                    smmu_dma_kva64(l1, off),
                    smmu_dma_dva(l2) | (8 /* split */ + 1),
                );
            }
            bus_dmamap_sync(
                sc.dmat(),
                l1.sdm_map,
                off,
                size_of::<u64>(),
                BUS_DMASYNC_PREWRITE,
            );
        }
        let Some(l2) = slot.get() else {
            return Err(Errno::ENXIO);
        };
        (l2, (sid % 256) * size_of::<[u64; 8]>())
    } else {
        (l1, sid * size_of::<[u64; 8]>())
    };

    let ste1 = SMMU_V3_STE_1_S1CIR_WBRA
        | SMMU_V3_STE_1_S1COR_WBRA
        | SMMU_V3_STE_1_S1CSH_ISH
        | SMMU_V3_STE_1_EATS_TRANS
        // the C's commented-out alternative: SMMU_V3_STE_1_STRW_NSEL1
        | SMMU_V3_STE_1_STRW_EL2
        | SMMU_V3_STE_1_S1DSS_SSID0;
    let ste0 = SMMU_V3_STE_0_V
        | SMMU_V3_STE_0_CFG_S1_TRANS
        | SMMU_V3_STE_0_S1FMT_LINEAR
        | smmu_dma_dva(cd);
    // SAFETY: words 1 and 0 of the stream's 64-byte entry, inside its table; word 0 (with the
    // valid bit) last.
    unsafe {
        ptr::write_volatile(smmu_dma_kva64(ste_sdm, ste_off + size_of::<u64>()), ste1);
        ptr::write_volatile(smmu_dma_kva64(ste_sdm, ste_off), ste0);
    }
    bus_dmamap_sync(
        sc.dmat(),
        ste_sdm.sdm_map,
        ste_off,
        size_of::<[u64; 8]>(),
        BUS_DMASYNC_PREWRITE,
    );
    smmu_v3_cfgi_ste(dom);

    smmu_v3_tlbi_asid(dom);
    Ok(())
}

/// Puts the command `cmd0, cmd1` in the command queue and moves `CMDQ_PROD` past it: the
/// insertion each of the C's command functions repeats. `false` (after the C's complaint)
/// when the queue is full.
fn smmu_v3_cmd_put(sc: &SmmuSoftc, cmd0: u64, cmd1: u64) -> bool {
    let sq = &sc.v3.sc_cmdq;
    mutex_assert_locked(&sc.v3.sc_cmdq_mtx, "smmu_v3_cmd_put");

    // TODO: Handle this more properly.
    sq.sq_cons.set(smmu_v3_read_4(sc, SMMU_V3_CMDQ_CONS));
    let (cons, prod) = (sq.sq_cons.get(), sq.sq_prod.get());
    if q_idx(sq, cons) == q_idx(sq, prod) && q_wrp(sq, cons) != q_wrp(sq, prod) {
        printf(format_args!(
            "{}: CMDQ ran out of space\n",
            sc.sc_dev.xname()
        ));
        return false;
    }

    let sdm = smmu_v3_q_sdm(sq);
    let off = q_idx(sq, prod) as usize * 2 * size_of::<u64>();
    let cmd = smmu_dma_kva64(sdm, off);
    bus_dmamap_sync(
        sc.dmat(),
        sdm.sdm_map,
        off,
        2 * size_of::<u64>(),
        BUS_DMASYNC_POSTREAD,
    );
    // SAFETY: the two words of the free slot at `prod`, inside the queue buffer.
    unsafe {
        ptr::write_volatile(cmd, cmd0);
        ptr::write_volatile(cmd.add(1), cmd1);
    }
    bus_dmamap_sync(
        sc.dmat(),
        sdm.sdm_map,
        off,
        2 * size_of::<u64>(),
        BUS_DMASYNC_PREWRITE,
    );

    // Let HW know we produced
    let next = (q_wrp(sq, prod) | q_idx(sq, prod)).wrapping_add(1);
    sq.sq_prod
        .set(smmu_v3_q_ovf(prod) | q_wrp(sq, next) | q_idx(sq, next));
    fence(Ordering::SeqCst); // membar_sync()
    smmu_v3_write_4(sc, SMMU_V3_CMDQ_PROD, sq.sq_prod.get());
    true
}

/// `smmu_v3_sync(sc)`: a `CMD_SYNC`, then waits until the hardware consumed every command.
fn smmu_v3_sync(sc: &SmmuSoftc) -> Result<(), Errno> {
    let sq = &sc.v3.sc_cmdq;

    mutex_assert_locked(&sc.v3.sc_cmdq_mtx, "smmu_v3_sync");

    if !smmu_v3_cmd_put(sc, SMMU_V3_CMD_SYNC | SMMU_V3_CMD_SYNC_0_CS_SEV, 0) {
        return Err(Errno::ETIMEDOUT);
    }

    // TODO: In a better world, where CPUs could concurrently put in commands, we should be
    // able to wait until it has *passed* the prod we set.
    for _ in 0..100000 {
        // Wait until HW processing caught up with us.
        sq.sq_cons.set(smmu_v3_read_4(sc, SMMU_V3_CMDQ_CONS));
        let (cons, prod) = (sq.sq_cons.get(), sq.sq_prod.get());
        if q_wrp(sq, cons) == q_wrp(sq, prod) && q_idx(sq, cons) == q_idx(sq, prod) {
            return Ok(());
        }
    }

    printf(format_args!(
        "{}: timeout waiting for SYNC\n",
        sc.sc_dev.xname()
    ));
    sq.sq_cons.set(smmu_v3_read_4(sc, SMMU_V3_CMDQ_CONS));
    Err(Errno::ETIMEDOUT)
}

/// One command and a sync under the command queue lock (the body of the C's
/// `smmu_v3_cfgi_*` and `smmu_v3_tlbi_all`/`_asid`).
fn smmu_v3_cmd_sync(sc: &SmmuSoftc, cmd0: u64, cmd1: u64) {
    mtx_enter(&sc.v3.sc_cmdq_mtx);
    if smmu_v3_cmd_put(sc, cmd0, cmd1) {
        let _ = smmu_v3_sync(sc);
    }
    mtx_leave(&sc.v3.sc_cmdq_mtx);
}

/// `smmu_v3_cfgi_all(sc)`: invalidates every cached stream table entry.
fn smmu_v3_cfgi_all(sc: &SmmuSoftc) {
    smmu_v3_cmd_sync(sc, SMMU_V3_CMD_CFGI_STE_RANGE, smmu_v3_cmd_cfgi_1_range(31));
}

/// `smmu_v3_cfgi_cd(dom)`: invalidates the domain's cached context descriptor.
fn smmu_v3_cfgi_cd(dom: &SmmuDomain) {
    smmu_v3_cmd_sync(
        dom.sd_sc,
        SMMU_V3_CMD_CFGI_CD | smmu_v3_cmd_cfgi_0_sid(u64::from(dom.sd_sid)),
        SMMU_V3_CMD_CFGI_1_LEAF,
    );
}

/// `smmu_v3_cfgi_ste(dom)`: invalidates the domain's cached stream table entry.
fn smmu_v3_cfgi_ste(dom: &SmmuDomain) {
    smmu_v3_cmd_sync(
        dom.sd_sc,
        SMMU_V3_CMD_CFGI_STE | smmu_v3_cmd_cfgi_0_sid(u64::from(dom.sd_sid)),
        SMMU_V3_CMD_CFGI_1_LEAF,
    );
}

/// `smmu_v3_tlbi_all(sc, op)`: a whole-TLB invalidation `op`.
fn smmu_v3_tlbi_all(sc: &SmmuSoftc, op: u64) {
    smmu_v3_cmd_sync(sc, op, 0);
}

/// `smmu_v3_tlbi_asid(dom)`: invalidates the domain's ASID (the C's `TLBI_EL2_ASID`; its
/// commented-out alternative is `TLBI_NH_ASID`).
fn smmu_v3_tlbi_asid(dom: &SmmuDomain) {
    smmu_v3_cmd_sync(
        dom.sd_sc,
        SMMU_V3_CMD_TLBI_EL2_ASID | smmu_v3_cmd_tlbi_0_asid(u64::from(dom.v3.sd_asid.get())),
        0,
    );
}

/// `smmu_v3_tlbi_va(dom, va)`: invalidates `va` in the domain's ASID (`TLBI_EL2_VA`; the
/// C's commented-out alternative is `TLBI_NH_VA`); the caller syncs
/// (`smmu_v3_tlb_sync_context`).
fn smmu_v3_tlbi_va(dom: &SmmuDomain, va: usize) {
    let sc = dom.sd_sc;

    mtx_enter(&sc.v3.sc_cmdq_mtx);
    let _ = smmu_v3_cmd_put(
        sc,
        SMMU_V3_CMD_TLBI_EL2_VA
            | smmu_v3_cmd_tlbi_0_vmid(0)
            | smmu_v3_cmd_tlbi_0_asid(u64::from(dom.v3.sd_asid.get())),
        va as u64 | SMMU_V3_CMD_TLBI_1_LEAF,
    );
    // callee is responsible for smmu_v3_tlb_sync_context()
    mtx_leave(&sc.v3.sc_cmdq_mtx);
}

/// `smmu_v3_tlb_sync_context(dom)`.
fn smmu_v3_tlb_sync_context(dom: &SmmuDomain) {
    let sc = dom.sd_sc;

    mtx_enter(&sc.v3.sc_cmdq_mtx);
    let _ = smmu_v3_sync(sc);
    mtx_leave(&sc.v3.sc_cmdq_mtx);
}

const _: () = {
    assert!(size_of::<SmmuVp0>() == size_of::<SmmuVp1>());
    assert!(size_of::<SmmuVp0>() == size_of::<SmmuVp2>());
    assert!(size_of::<SmmuVp0>() != size_of::<SmmuVp3>());
};
/* </CODE> */
