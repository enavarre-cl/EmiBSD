/* <LICENSES> */
/*
 * Copyright (c) 2026 Emilio Navarrete Lineros <enavarre@outlook.com>
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

/*
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
//! The System MMU's software state: `arch/arm64/dev/smmuvar.h`.
//!
//! Upstream: sys/arch/arm64/dev/smmuvar.h @ 3ce1f3f79392
//!
//! An `smmu` softc (`struct smmu_softc`) is shared by the device-tree and ACPI attachments,
//! which embed it first in their own softcs. Each stream ID (a device) gets a domain
//! (`struct smmu_domain`): a context bank (v2) or a context descriptor (v3), its own I/O
//! page tables and an extent of I/O virtual addresses its DMA maps allocate from.
//!
//! ## Deviations
//! - The C's anonymous unions of the v2 and v3 members (in the softc and the domain) are
//!   structs with both members, as `docs/C_TO_RUST.md` does for `union pool_lock`; the
//!   v3 half keeps its `v3.` prefix.
//! - `sd_vp`, a union of the 4-level (`l0`) and 3-level (`l1`) roots, is two `Cell`s, one
//!   used as `sd_4level` says.
//! - The softc is zero-allocated by autoconf, so every member is valid as zeroes: `Cell`s
//!   of integers and `Option`s, the mutexes (initialised by `mtx_init` before use) and the
//!   domain queue. The two pools are allocated at attach (`Cell<Option<&'static Pool>>`),
//!   since `pool_init` wants a `'static` pool.
//! - `sc_smr` and `sc_cb` (the C's `mallocarray`ed pointer arrays) are leaked boxed slices
//!   of `Cell<Option<&'static ..>>`; `sc_strtab_l2` likewise. `struct smmu_cb` is empty, so
//!   an entry is only a marker that the bank is taken.
//! - `sd_exname` is the extent's name, kept as a leaked byte string (the extent keeps the
//!   pointer, as in C).
//! - The three per-version hooks are `Option<fn>`; errors are `Result<(), Errno>`.
//! - `v3.sc_has_hyp` is not in the C: `SMMU_IDR0.Hyp`, which picks the EL2 or the
//!   non-hypervisor translation regime (`smmu.rs`, deviations).

use core::cell::Cell;
use core::ptr::NonNull;

use crate::arch::arm64::dev::smmu::{SmmuVp0, SmmuVp1};
use crate::arch::arm64::include::bus::{
    BusDmaSegment, BusDmaTagT, BusDmamapT, BusSpace, BusSpaceHandle,
};
use crate::sys::device::{Device, Softc};
use crate::sys::errno::Errno;
use crate::sys::extent::Extent;
use crate::sys::mutex::Mutex;
use crate::sys::pool::Pool;
use crate::sys::queue::{SimpleqEntry, SimpleqHead};

/// `struct smmu_dmamem`: a physically contiguous, mapped and loaded DMA buffer the SMMU
/// itself reads (queues, stream tables, context descriptors).
pub struct SmmuDmamem {
    /// `sdm_map`.
    pub sdm_map: BusDmamapT,
    /// `sdm_seg`.
    pub sdm_seg: BusDmaSegment,
    /// `sdm_size`.
    pub sdm_size: usize,
    /// `sdm_kva`.
    pub sdm_kva: NonNull<u8>,
}

/// The v3 members of `struct smmu_domain`.
pub struct SmmuDomainV3 {
    /// `sd_cd`: the domain's context descriptor.
    pub sd_cd: Cell<Option<&'static SmmuDmamem>>,
    /// `sd_asid`.
    pub sd_asid: Cell<u16>,
}

/// `struct smmu_domain`: the translation of one stream ID.
pub struct SmmuDomain {
    /// `sd_sc`.
    pub sd_sc: &'static SmmuSoftc,
    /// `sd_sid`: the stream ID.
    pub sd_sid: u32,
    /// `sd_dmat`: the tag the domain's devices use, made by `smmu_device_map`.
    pub sd_dmat: Cell<Option<BusDmaTagT>>,
    /// `sd_cb_idx` (v2): the context bank.
    pub sd_cb_idx: Cell<i32>,
    /// `sd_smr_idx` (v2): the stream match register or stream index.
    pub sd_smr_idx: Cell<i32>,
    /// The v3 members.
    pub v3: SmmuDomainV3,
    /// `sd_stage`: 1 or 2.
    pub sd_stage: Cell<i32>,
    /// `sd_4level`: four-level page tables (40 or more address bits).
    pub sd_4level: Cell<i32>,
    /// `sd_exname`: the extent's name, `"<smmu>:<sid>"`.
    pub sd_exname: Cell<&'static [u8]>,
    /// `sd_iovamap`: the domain's I/O virtual addresses.
    pub sd_iovamap: Cell<Option<&'static Extent>>,
    /// `sd_vp.l0`: the 4-level root.
    pub sd_vp_l0: Cell<Option<&'static SmmuVp0>>,
    /// `sd_vp.l1`: the 3-level root.
    pub sd_vp_l1: Cell<Option<&'static SmmuVp1>>,
    /// `sd_iova_mtx`: guards `sd_iovamap`.
    pub sd_iova_mtx: Mutex,
    /// `sd_pmap_mtx`: guards the page-table levels being added.
    pub sd_pmap_mtx: Mutex,
    /// `sd_list`: link in `sc_domains`.
    pub sd_list: SimpleqEntry<SmmuDomain>,
}

impl SmmuDomain {
    /// `sd_iovamap`, set by the domain's creation before the domain is used.
    pub fn iovamap(&self) -> &'static Extent {
        match self.sd_iovamap.get() {
            Some(ex) => ex,
            None => crate::kern::subr_prf::panic(format_args!("smmu: domain without IOVA map")),
        }
    }
}

crate::queue_adapter!(
    /// `SIMPLEQ_HEAD(, smmu_domain)`: through `sd_list`.
    pub SmmuDomainList: SmmuDomain, sd_list => SimpleqEntry<SmmuDomain>
);

/// `struct smmu_cb`: a context bank in use (empty, as in C).
pub struct SmmuCb;

/// `struct smmu_cb_irq`: a context bank's interrupt argument.
pub struct SmmuCbIrq {
    /// `cbi_sc`.
    pub cbi_sc: &'static SmmuSoftc,
    /// `cbi_idx`.
    pub cbi_idx: i32,
}

/// `struct smmu_smr`: a stream match register in use.
pub struct SmmuSmr {
    /// `ss_dom`: the domain it maps to (`None` for a QCOM firmware stream not yet taken).
    pub ss_dom: Cell<Option<&'static SmmuDomain>>,
    /// `ss_id`.
    pub ss_id: Cell<u16>,
    /// `ss_mask`.
    pub ss_mask: Cell<u16>,
}

/// `struct smmu_v3_queue`: a command, event or PRI queue.
pub struct SmmuV3Queue {
    /// `sq_sdm`.
    pub sq_sdm: Cell<Option<&'static SmmuDmamem>>,
    /// `sq_size_log2`.
    pub sq_size_log2: Cell<i32>,
    /// `sq_prod`.
    pub sq_prod: Cell<u32>,
    /// `sq_cons`.
    pub sq_cons: Cell<u32>,
}

/// The v3 members of `struct smmu_softc`.
pub struct SmmuSoftcV3 {
    /// `sc_sidsize`.
    pub sc_sidsize: Cell<i32>,
    /// `sc_2lvl_cdtab`.
    pub sc_2lvl_cdtab: Cell<i32>,
    /// `sc_2lvl_strtab`.
    pub sc_2lvl_strtab: Cell<i32>,
    /// `sc_has_asid16s`.
    pub sc_has_asid16s: Cell<i32>,
    /// `sc_has_pri`.
    pub sc_has_pri: Cell<i32>,
    /// `sc_cmdq_mtx`: guards the command queue.
    pub sc_cmdq_mtx: Mutex,
    /// `sc_cmdq`.
    pub sc_cmdq: SmmuV3Queue,
    /// `sc_eventq`.
    pub sc_eventq: SmmuV3Queue,
    /// `sc_priq`.
    pub sc_priq: SmmuV3Queue,
    /// `sc_strtab_l1`.
    pub sc_strtab_l1: Cell<Option<&'static SmmuDmamem>>,
    /// `sc_strtab_l2`: the second-level tables, one per 256 stream IDs.
    pub sc_strtab_l2: Cell<Option<&'static [Cell<Option<&'static SmmuDmamem>>]>>,
    /// `sc_next_asid`.
    pub sc_next_asid: Cell<u16>,
    /// Not in the C: `SMMU_IDR0.Hyp`, whether the SMMU has the EL2 translation regime the C
    /// always programs (`smmu.rs`, deviations).
    pub sc_has_hyp: Cell<i32>,
}

/// `smmu_softc.sc_domain_create`.
pub type SmmuDomainCreateFn = fn(&'static SmmuDomain) -> Result<(), Errno>;

/// `smmu_softc.sc_tlbi_va`: invalidates one IOVA of a domain.
pub type SmmuTlbiVaFn = fn(&SmmuDomain, usize);

/// `struct smmu_softc`.
#[repr(C)]
pub struct SmmuSoftc {
    /// `sc_dev`.
    pub sc_dev: Device,
    /// `sc_iot`.
    pub sc_iot: Cell<Option<&'static BusSpace>>,
    /// `sc_ioh`.
    pub sc_ioh: Cell<Option<BusSpaceHandle>>,
    /// `sc_dmat`: the tag the SMMU's own tables and queues are allocated with.
    pub sc_dmat: Cell<Option<BusDmaTagT>>,

    /// `sc_is_mmu500` (v2).
    pub sc_is_mmu500: Cell<i32>,
    /// `sc_is_ap806` (v2): Marvell's 8040, no 64-bit register writes.
    pub sc_is_ap806: Cell<i32>,
    /// `sc_is_qcom` (v2).
    pub sc_is_qcom: Cell<i32>,
    /// `sc_bypass_quirk` (v2).
    pub sc_bypass_quirk: Cell<i32>,
    /// `sc_pagesize` (v2).
    pub sc_pagesize: Cell<usize>,
    /// `sc_numpage` (v2).
    pub sc_numpage: Cell<i32>,
    /// `sc_num_context_banks` (v2).
    pub sc_num_context_banks: Cell<i32>,
    /// `sc_num_s2_context_banks` (v2).
    pub sc_num_s2_context_banks: Cell<i32>,
    /// `sc_has_exids` (v2).
    pub sc_has_exids: Cell<i32>,
    /// `sc_num_streams` (v2).
    pub sc_num_streams: Cell<i32>,
    /// `sc_stream_mask` (v2).
    pub sc_stream_mask: Cell<u16>,
    /// `sc_smr` (v2): the stream match registers, `None` for stream indexing.
    pub sc_smr: Cell<Option<&'static [Cell<Option<&'static SmmuSmr>>]>>,
    /// `sc_cb` (v2): the context banks in use.
    pub sc_cb: Cell<Option<&'static [Cell<Option<&'static SmmuCb>>]>>,

    /// The v3 members.
    pub v3: SmmuSoftcV3,

    /// `sc_has_s1`.
    pub sc_has_s1: Cell<i32>,
    /// `sc_has_s2`.
    pub sc_has_s2: Cell<i32>,
    /// `sc_has_vmid16s`.
    pub sc_has_vmid16s: Cell<i32>,
    /// `sc_ipa_bits`.
    pub sc_ipa_bits: Cell<i32>,
    /// `sc_pa_bits`.
    pub sc_pa_bits: Cell<i32>,
    /// `sc_va_bits`.
    pub sc_va_bits: Cell<i32>,
    /// `sc_coherent`: the SMMU's table walks snoop the caches.
    pub sc_coherent: Cell<i32>,
    /// `sc_vp_pool`: the L0..L2 page-table pages.
    pub sc_vp_pool: Cell<Option<&'static Pool>>,
    /// `sc_vp3_pool`: the L3 page-table pages.
    pub sc_vp3_pool: Cell<Option<&'static Pool>>,
    /// `sc_domains`.
    pub sc_domains: SimpleqHead<SmmuDomainList>,

    /// `sc_domain_create`.
    pub sc_domain_create: Cell<Option<SmmuDomainCreateFn>>,
    /// `sc_tlbi_va`.
    pub sc_tlbi_va: Cell<Option<SmmuTlbiVaFn>>,
    /// `sc_tlb_sync_context`.
    pub sc_tlb_sync_context: Cell<Option<fn(&SmmuDomain)>>,
}

// SAFETY: `#[repr(C)]` with the device first; every other member is valid as zeroes (cells
// of integers, `Option`s of references and `fn`s, null-free mutexes and an empty queue).
unsafe impl Softc for SmmuSoftc {}

impl SmmuSoftc {
    /// `sc_iot`, set by the attachment before anything maps.
    pub fn iot(&self) -> &'static BusSpace {
        match self.sc_iot.get() {
            Some(t) => t,
            None => crate::kern::subr_prf::panic(format_args!("smmu: no bus space")),
        }
    }

    /// `sc_ioh`.
    pub fn ioh(&self) -> BusSpaceHandle {
        match self.sc_ioh.get() {
            Some(h) => h,
            None => crate::kern::subr_prf::panic(format_args!("smmu: registers not mapped")),
        }
    }

    /// `sc_dmat`.
    pub fn dmat(&self) -> BusDmaTagT {
        match self.sc_dmat.get() {
            Some(t) => t,
            None => crate::kern::subr_prf::panic(format_args!("smmu: no DMA tag")),
        }
    }
}
/* </CODE> */
