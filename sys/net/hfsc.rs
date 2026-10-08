/*	$OpenBSD: hfsc.h,v 1.15 2026/03/19 14:59:05 sthen Exp $	*/
/*	$OpenBSD: hfsc.c,v 1.53 2026/03/19 14:59:05 sthen Exp $	*/
/* <LICENSES> */
/*
 * Copyright (c) 2012-2013 Henning Brauer <henning@openbsd.org>
 * Copyright (c) 1997-1999 Carnegie Mellon University. All Rights Reserved.
 *
 * Permission to use, copy, modify, and distribute this software and
 * its documentation is hereby granted (including for commercial or
 * for-profit use), provided that both the copyright notice and this
 * permission notice appear in all copies of the software, derivative
 * works, or modified versions, and any portions thereof.
 *
 * THIS SOFTWARE IS EXPERIMENTAL AND IS KNOWN TO HAVE BUGS, SOME OF
 * WHICH MAY HAVE SERIOUS CONSEQUENCES.  CARNEGIE MELLON PROVIDES THIS
 * SOFTWARE IN ITS ``AS IS'' CONDITION, AND ANY EXPRESS OR IMPLIED
 * WARRANTIES, INCLUDING, BUT NOT LIMITED TO, THE IMPLIED WARRANTIES
 * OF MERCHANTABILITY AND FITNESS FOR A PARTICULAR PURPOSE ARE
 * DISCLAIMED.  IN NO EVENT SHALL CARNEGIE MELLON UNIVERSITY BE LIABLE
 * FOR ANY DIRECT, INDIRECT, INCIDENTAL, SPECIAL, EXEMPLARY, OR
 * CONSEQUENTIAL DAMAGES (INCLUDING, BUT NOT LIMITED TO, PROCUREMENT
 * OF SUBSTITUTE GOODS OR SERVICES; LOSS OF USE, DATA, OR PROFITS; OR
 * BUSINESS INTERRUPTION) HOWEVER CAUSED AND ON ANY THEORY OF
 * LIABILITY, WHETHER IN CONTRACT, STRICT LIABILITY, OR TORT
 * (INCLUDING NEGLIGENCE OR OTHERWISE) ARISING IN ANY WAY OUT OF THE
 * USE OF THIS SOFTWARE, EVEN IF ADVISED OF THE POSSIBILITY OF SUCH
 * DAMAGE.
 *
 * Carnegie Mellon encourages (but does not require) users of this
 * software to return any improvements or extensions that they make,
 * and to grant Carnegie Mellon the rights to redistribute these
 * changes without encumbrance.
 */
/* </LICENSES> */

/* <CODE> */
//! Hierarchical fair service curve packet scheduling: `<net/hfsc.h>` and `net/hfsc.c`, the
//! queueing discipline behind pf's `queue ... bandwidth` rules (`pf.conf(5)`).
//!
//! Upstream: sys/net/hfsc.h @ 3ce1f3f79392
//! Upstream: sys/net/hfsc.c @ 3ce1f3f79392
//!
//! H-FSC is described in Proceedings of SIGCOMM'97, "A Hierarchical Fair Service Curve
//! Algorithm for Link-Sharing, Real-Time and Priority Service" by Ion Stoica, Hui Zhang and
//! T. S. Eugene Ng. Oleg Cherevko added the upper limit for link-sharing: when a class has an
//! upper-limit curve its fit-time is computed from that curve, and the link-sharing scheduler
//! does not pick a class whose fit-time is later than the current time.
//!
//! pf builds the class tree of an interface through [`PFQ_HFSC_OPS`] (`hfsc_pf_alloc`, one
//! `hfsc_pf_addqueue` per queue) and then switches the interface's first send queue to
//! [`IFQ_HFSC_OPS`] with `ifq_attach`, handing over the softc. A leaf class keeps its packets
//! in its own FIFO (`struct hfsc_classq`, driven through the same `pfq_ops` table), or, for a
//! class without a real-time curve that asks for `flows`, in the queue manager
//! `pf_queue_manager` names (FQ-CoDel, `net/fq_codel.rs`).
//!
//! Service curves are integers: x is time in nanoseconds (`nsecuptime`, `HFSC_FREQ` ticks a
//! second), y is bytes; slopes are scaled up by `SM_SHIFT`, inverse slopes by `ISM_SHIFT`.
//! The arithmetic is the C's unsigned 64-bit arithmetic and wraps where the C wraps; there is
//! no floating point in the kernel.
//!
//! ## Deviations
//! - Classes are `&'static` items of `hfsc_class_pl` whose members are `Cell`s, linked through
//!   `Cell<Option<&'static HfscClass>>`; the softc (`struct hfsc_if`) is a `malloc`ed value
//!   reached from the opaque pointer `ifq_q` holds. The runtime curves are `Copy` values in
//!   `Cell`s: `hfsc_rtsc_*` work on a `&mut HfscRuntimeSc` the caller takes out of the class
//!   and stores back, which is what `cl->cl_eligible = cl->cl_deadline` needs anyway.
//! - An internal service curve (`hfsc_internal_sc_pl`) is written once, through the fresh pool
//!   item, before the class points at it; it is read-only afterwards.
//! - `pool_get(PR_WAITOK)` and `malloc(M_WAITOK)` can fail here (they do not sleep yet,
//!   `kern/kern_malloc.rs`): a failed class or curve allocation makes `hfsc_class_create`
//!   return NULL, its existing failure; a failed `malloc` of the softc or the class table
//!   panics, as `priq_alloc` does.
//! - `hfsc_class_destroy` returns `Result<(), Errno>` (`EBUSY`); `hfsc_clh2cph`,
//!   `hfsc_nextclass`, `hfsc_class_create`, `hfsc_ellist_get_mindl` and
//!   `hfsc_actlist_firstfit` return `Option` (NULL is `None`). `HFSC_ENABLED(ifq)` is the
//!   function [`hfsc_enabled`], `PKTCNTR_INC` the function `pktcntr_inc`.
//! - The `pfq_ops` functions take the discipline as `*mut c_void`, the types of the `PfqOps`
//!   table (`net/pfvar.rs`). They are private and reached only through [`PFQ_HFSC_OPS`],
//!   whose contract is that the pointer is what the table's `pfq_alloc` returned (or, for the
//!   per-class functions, the class queue `hfsc_pf_addqueue` installed).
//! - `hfsc_pf_qstats` zeroes `stats` before filling it (the C copies out its padding
//!   uninitialised); the holes of `struct hfsc_class_stats` are named `_pad` members.
//! - `hfsc_pf_addqueue` returns `EINVAL` for a queue whose kif has no interface (the C
//!   dereferences it; `pf_create_queues` never adds such a queue).
//! - `struct hfsc_if`'s `hif_next` (interface state list) is left out: nothing in the C sets
//!   or reads it.

use core::cell::Cell;
use core::ffi::c_void;
use core::ptr::{self, NonNull};
use core::sync::atomic::Ordering;

use crate::conf::param::TICK_NSEC;
use crate::kassert;
use crate::kern::kern_malloc::{free, malloc, mallocarray};
use crate::kern::kern_tc::nsecuptime;
use crate::kern::kern_timeout::{timeout_add, timeout_del, timeout_set};
use crate::kern::subr_pool::{pool_get, pool_put};
use crate::kern::subr_prf::panic;
use crate::kern::uipc_mbuf::{ml_dequeue, ml_enlist, ml_enqueue, ml_init};
use crate::machine::copy::{AbiPod, copyout_obj};
use crate::machine::intr::{IPL_NONE, splnet, splx};
use crate::net::if_var::Ifnet;
use crate::net::ifq::{
    IfqOps, Ifqueue, ifq_empty, ifq_mfreeml, ifq_q_enter, ifq_q_leave, ifq_start,
};
use crate::net::pf_ioctl::pf_queue_manager;
use crate::net::pfvar::{PfPoolItem, PfQueuespec, PfqOps, pf_pool_get, pf_pool_init, pf_pool_put};
use crate::sys::errno::Errno;
use crate::sys::malloc::{M_DEVBUF, M_WAITOK, M_ZERO};
use crate::sys::mbuf::{Mbuf, MbufList, mbuf_list_first, ml_empty, ml_len};
use crate::sys::pool::{PR_WAITOK, Pool};
use crate::sys::queue::{TailqEntry, TailqHead};
use crate::sys::systm::kernel_assert_locked;
use crate::sys::timeout::{Timeout, timeout_pending};

// hfsc class flags
/// `HFSC_DEFAULTCLASS`: the default class.
pub const HFSC_DEFAULTCLASS: i32 = 0x1000;

/// `struct hfsc_pktcntr`: a packet and byte counter.
#[repr(C)]
#[derive(Clone, Copy, Default, Debug, PartialEq, Eq)]
pub struct HfscPktcntr {
    /// `packets`.
    pub packets: u64,
    /// `bytes`.
    pub bytes: u64,
}

// SAFETY: `#[repr(C)]`, two `u64`s: no padding, every bit pattern valid.
unsafe impl AbiPod for HfscPktcntr {}

/// `struct hfsc_sc`: a service curve as pf and pfctl give it.
#[repr(C)]
#[derive(Clone, Copy, Default, Debug, PartialEq, Eq)]
pub struct HfscSc {
    /// `m1`: slope of the first segment in bits/sec.
    pub m1: u64,
    /// `m2`: slope of the second segment in bits/sec.
    pub m2: u64,
    /// `d`: the x-projection of the first segment in msec.
    pub d: u32,
    /// The hole the C compiler leaves before the next 8-byte member.
    _pad: u32,
}

impl HfscSc {
    /// A curve of slopes `m1` (for `d` msec) then `m2`.
    pub const fn new(m1: u64, d: u32, m2: u64) -> Self {
        Self { m1, m2, d, _pad: 0 }
    }
}

// SAFETY: `#[repr(C)]` integers, the trailing hole named: every bit pattern valid.
unsafe impl AbiPod for HfscSc {}

// special class handles
/// `HFSC_ROOT_CLASS`.
pub const HFSC_ROOT_CLASS: u32 = 0x10000;
/// `HFSC_DEFAULT_CLASSES`: the initial size of the class table.
pub const HFSC_DEFAULT_CLASSES: u32 = 64;
/// `HFSC_MAX_CLASSES`.
pub const HFSC_MAX_CLASSES: u32 = 65535;

// service curve types
/// `HFSC_REALTIMESC`.
pub const HFSC_REALTIMESC: i32 = 1;
/// `HFSC_LINKSHARINGSC`.
pub const HFSC_LINKSHARINGSC: i32 = 2;
/// `HFSC_UPPERLIMITSC`.
pub const HFSC_UPPERLIMITSC: i32 = 4;
/// `HFSC_DEFAULTSC`.
pub const HFSC_DEFAULTSC: i32 = HFSC_REALTIMESC | HFSC_LINKSHARINGSC;

/// `struct hfsc_class_stats`: what `DIOCGETQSTATS` copies out for an HFSC queue (`pfctl -vsq`).
#[repr(C)]
#[derive(Clone, Copy, Default, Debug)]
pub struct HfscClassStats {
    /// `xmit_cnt`.
    pub xmit_cnt: HfscPktcntr,
    /// `drop_cnt`.
    pub drop_cnt: HfscPktcntr,

    /// `qlength`.
    pub qlength: u32,
    /// `qlimit`.
    pub qlimit: u32,
    /// `period`.
    pub period: u32,

    /// `class_id`.
    pub class_id: u32,
    /// `class_handle`.
    pub class_handle: u32,
    /// The hole before `rsc`.
    _pad0: u32,
    /// `rsc`.
    pub rsc: HfscSc,
    /// `fsc`.
    pub fsc: HfscSc,
    /// `usc`: upper limit service curve.
    pub usc: HfscSc,

    /// `total`: total work in bytes.
    pub total: u64,
    /// `cumul`: cumulative work in bytes done by real-time criteria.
    pub cumul: u64,
    /// `d`: deadline.
    pub d: u64,
    /// `e`: eligible time.
    pub e: u64,
    /// `vt`: virtual time.
    pub vt: u64,
    /// `f`: fit time for upper-limit.
    pub f: u64,

    // info helpful for debugging
    /// `initvt`: init virtual time.
    pub initvt: u64,
    /// `vtoff`: `cl_vt_ipoff`.
    pub vtoff: u64,
    /// `cvtmax`: `cl_maxvt`.
    pub cvtmax: u64,
    /// `myf`: `cl_myf`.
    pub myf: u64,
    /// `cfmin`: `cl_mincf`.
    pub cfmin: u64,
    /// `cvtmin`: `cl_mincvt`.
    pub cvtmin: u64,
    /// `myfadj`: `cl_myfadj`.
    pub myfadj: u64,
    /// `vtadj`: `cl_vtadj`.
    pub vtadj: u64,
    /// `cur_time`.
    pub cur_time: u64,
    /// `machclk_freq`.
    pub machclk_freq: u32,

    /// `vtperiod`: vt period sequence no.
    pub vtperiod: u32,
    /// `parentperiod`: parent's vt period seqno.
    pub parentperiod: u32,
    /// `nactive`: number of active children.
    pub nactive: i32,

    // red and rio related info
    /// `qtype`.
    pub qtype: i32,
    // struct redstats red[3]; (commented out in the C)
    /// The tail padding to the 8-byte alignment.
    _pad1: u32,
}

// SAFETY: `#[repr(C)]` integers and `AbiPod` structures, the holes named: every bit pattern
// valid.
unsafe impl AbiPod for HfscClassStats {}

/// `HFSC_DEFAULT_QLIMIT`.
pub const HFSC_DEFAULT_QLIMIT: i32 = 50;

/// `PKTCNTR_INC(cntr, len)`.
fn pktcntr_inc(cntr: &Cell<HfscPktcntr>, len: i32) {
    let mut c = cntr.get();
    c.packets = c.packets.wrapping_add(1);
    c.bytes = c.bytes.wrapping_add(len as u64);
    cntr.set(c);
}

/// `struct hfsc_internal_sc`: the kernel internal representation of a service curve.
///
/// Coordinates are 64-bit unsigned integers. x-axis: the unit is the clock count (here
/// nanoseconds); virtual time is computed on the same scale. y-axis: the unit is the byte.
/// The slopes are scaled to avoid overflow; the inverse slopes and the y-projection of the
/// first segment are kept to avoid 64-bit divisions, expensive on 32-bit machines. The
/// x-axis does not wrap around for 1089 years with a 1GHz clock, the y-axis not for 4358
/// years at 1Gbps.
#[derive(Clone, Copy, Default, Debug, PartialEq, Eq)]
struct HfscInternalSc {
    /// `sm1`: scaled slope of the 1st segment.
    sm1: u64,
    /// `ism1`: scaled inverse-slope of the 1st segment.
    ism1: u64,
    /// `dx`: the x-projection of the 1st segment.
    dx: u64,
    /// `dy`: the y-projection of the 1st segment.
    dy: u64,
    /// `sm2`: scaled slope of the 2nd segment.
    sm2: u64,
    /// `ism2`: scaled inverse-slope of the 2nd segment.
    ism2: u64,
}

// SAFETY: plain `u64`s: the all-zero value is valid.
unsafe impl PfPoolItem for HfscInternalSc {}

/// `struct hfsc_runtime_sc`: a runtime service curve.
#[derive(Clone, Copy, Default, Debug, PartialEq, Eq)]
struct HfscRuntimeSc {
    /// `x`: current starting position on x-axis.
    x: u64,
    /// `y`: current starting position on y-axis.
    y: u64,
    /// `sm1`: scaled slope of the 1st segment.
    sm1: u64,
    /// `ism1`: scaled inverse-slope of the 1st segment.
    ism1: u64,
    /// `dx`: the x-projection of the 1st segment.
    dx: u64,
    /// `dy`: the y-projection of the 1st segment.
    dy: u64,
    /// `sm2`: scaled slope of the 2nd segment.
    sm2: u64,
    /// `ism2`: scaled inverse-slope of the 2nd segment.
    ism2: u64,
}

/// `struct hfsc_classq`: a leaf class's own packet queue.
struct HfscClassq {
    /// `q`: queue of packets.
    q: MbufList,
    /// `qlimit`: queue limit.
    qlimit: Cell<i32>,
}

/// `cl_stats` of a class.
struct HfscClStats {
    /// `xmit_cnt`.
    xmit_cnt: Cell<HfscPktcntr>,
    /// `drop_cnt`.
    drop_cnt: Cell<HfscPktcntr>,
    /// `period`.
    period: Cell<u32>,
}

/// `struct hfsc_class`. Protected by: the send queue's `ifq_mtx` once the softc is attached;
/// before that, by pf's lock (`pf_create_queues`).
struct HfscClass {
    /// `cl_id`: class id (just for debug).
    cl_id: Cell<u32>,
    /// `cl_handle`: class handle.
    cl_handle: Cell<u32>,
    /// `cl_flags`: misc flags.
    cl_flags: Cell<i32>,

    /// `cl_parent`: parent class.
    cl_parent: Cell<Option<&'static HfscClass>>,
    /// `cl_siblings`: sibling classes.
    cl_siblings: Cell<Option<&'static HfscClass>>,
    /// `cl_children`: child classes.
    cl_children: Cell<Option<&'static HfscClass>>,

    /// `cl_q`: class queue structure.
    cl_q: HfscClassq,

    /// `cl_qops`: queue manager.
    cl_qops: Cell<Option<&'static PfqOps>>,
    /// `cl_qdata`: queue manager data.
    cl_qdata: Cell<*mut c_void>,
    /// `cl_cookie`: queue manager cookie.
    cl_cookie: Cell<*mut c_void>,

    /// `cl_total`: total work in bytes.
    cl_total: Cell<u64>,
    /// `cl_cumul`: cumulative work in bytes done by real-time criteria.
    cl_cumul: Cell<u64>,
    /// `cl_d`: deadline.
    cl_d: Cell<u64>,
    /// `cl_e`: eligible time.
    cl_e: Cell<u64>,
    /// `cl_vt`: virtual time.
    cl_vt: Cell<u64>,
    /// `cl_f`: time when this class will fit for link-sharing, max(myf, cfmin).
    cl_f: Cell<u64>,
    /// `cl_myf`: my fit-time (as calculated from this class's own upperlimit curve).
    cl_myf: Cell<u64>,
    /// `cl_myfadj`: my fit-time adjustment (to cancel history dependence).
    cl_myfadj: Cell<u64>,
    /// `cl_cfmin`: earliest children's fit-time (used with `cl_myf` to obtain `cl_f`).
    cl_cfmin: Cell<u64>,
    /// `cl_cvtmin`: minimal virtual time among the children fit for link-sharing (monotonic
    /// within a period).
    cl_cvtmin: Cell<u64>,
    /// `cl_vtadj`: intra-period cumulative vt adjustment.
    cl_vtadj: Cell<u64>,
    /// `cl_vtoff`: inter-period cumulative vt offset.
    cl_vtoff: Cell<u64>,
    /// `cl_cvtmax`: max child's vt in the last period.
    cl_cvtmax: Cell<u64>,

    /// `cl_initvt`: init virtual time (for debugging).
    cl_initvt: Cell<u64>,

    /// `cl_rsc`: internal real-time service curve.
    cl_rsc: Cell<Option<&'static HfscInternalSc>>,
    /// `cl_fsc`: internal fair service curve.
    cl_fsc: Cell<Option<&'static HfscInternalSc>>,
    /// `cl_usc`: internal upperlimit service curve.
    cl_usc: Cell<Option<&'static HfscInternalSc>>,
    /// `cl_deadline`: deadline curve.
    cl_deadline: Cell<HfscRuntimeSc>,
    /// `cl_eligible`: eligible curve.
    cl_eligible: Cell<HfscRuntimeSc>,
    /// `cl_virtual`: virtual curve.
    cl_virtual: Cell<HfscRuntimeSc>,
    /// `cl_ulimit`: upperlimit curve.
    cl_ulimit: Cell<HfscRuntimeSc>,

    /// `cl_vtperiod`: vt period sequence no.
    cl_vtperiod: Cell<u32>,
    /// `cl_parentperiod`: parent's vt period seqno.
    cl_parentperiod: Cell<u32>,
    /// `cl_nactive`: number of active children.
    cl_nactive: Cell<i32>,
    /// `cl_actc`: active children list.
    cl_actc: TailqHead<HfscActive>,

    /// `cl_actlist`: active children list entry.
    cl_actlist: TailqEntry<HfscClass>,
    /// `cl_ellist`: eligible list entry.
    cl_ellist: TailqEntry<HfscClass>,

    /// `cl_stats`.
    cl_stats: HfscClStats,
}

// SAFETY: `Cell`s of integers, of `Copy` integer structures, of `Option<&T>` and of raw
// pointers, an `MbufList` and queue links and heads: the all-zero value is valid.
unsafe impl PfPoolItem for HfscClass {}

// for TAILQ based ellist and actlist implementation
crate::queue_adapter!(
    /// `TAILQ_HEAD(hfsc_eligible, hfsc_class)`.
    HfscEligible: HfscClass, cl_ellist => TailqEntry<HfscClass>
);
crate::queue_adapter!(
    /// `TAILQ_HEAD(hfsc_active, hfsc_class)`.
    HfscActive: HfscClass, cl_actlist => TailqEntry<HfscClass>
);

/// `struct hfsc_if`: hfsc interface state. Protected by: the send queue's `ifq_mtx` once
/// attached.
struct HfscIf {
    /// `hif_rootclass`: root class.
    hif_rootclass: Cell<Option<&'static HfscClass>>,
    /// `hif_defaultclass`: default class.
    hif_defaultclass: Cell<Option<&'static HfscClass>>,
    /// `hif_class_tbl`: `hif_allocated` slots.
    hif_class_tbl: Cell<*const Cell<Option<&'static HfscClass>>>,

    /// `hif_microtime`: time at deq_begin.
    hif_microtime: Cell<u64>,

    /// `hif_allocated`: # of slots in `hif_class_tbl`.
    hif_allocated: Cell<u32>,
    /// `hif_classes`: # of classes in the tree.
    hif_classes: Cell<u32>,
    /// `hif_classid`: class id sequence number.
    hif_classid: Cell<u32>,

    /// `hif_eligible`: eligible list.
    hif_eligible: TailqHead<HfscEligible>,
    /// `hif_defer`: for queues that weren't ready.
    hif_defer: Timeout,
}

impl HfscIf {
    /// The class table, `hif_allocated` slots.
    fn class_tbl(&self) -> &[Cell<Option<&'static HfscClass>>] {
        let tbl = self.hif_class_tbl.get();
        if tbl.is_null() {
            return &[];
        }
        // SAFETY: `hif_class_tbl` is a `mallocarray` of `hif_allocated` slots, kept in step
        // by `hfsc_grow_class_tbl`, and lives until `hfsc_free`.
        unsafe { core::slice::from_raw_parts(tbl, self.hif_allocated.get() as usize) }
    }
}

/// `HFSC_FREQ`: clock ticks a second (nanoseconds).
const HFSC_FREQ: u64 = 1_000_000_000;
/// `HFSC_HT_INFINITY`: infinite time value.
const HFSC_HT_INFINITY: u64 = 0xffff_ffff_ffff_ffff;

/// `HFSC_CLK_PER_TICK`: `tick_nsec`.
fn hfsc_clk_per_tick() -> u64 {
    TICK_NSEC.load(Ordering::Relaxed) as u64
}

/// `hfsc_class_pl`.
pub static HFSC_CLASS_PL: Pool = Pool::new();
/// `hfsc_internal_sc_pl`.
pub static HFSC_INTERNAL_SC_PL: Pool = Pool::new();

//
// ifqueue glue.
//

/// `hfsc_ops`.
static HFSC_OPS: IfqOps = IfqOps {
    ifqop_idx: hfsc_idx,
    ifqop_enq: hfsc_enq,
    ifqop_deq_begin: hfsc_deq_begin,
    ifqop_deq_commit: hfsc_deq_commit,
    ifqop_purge: hfsc_purge,
    ifqop_alloc: hfsc_alloc,
    ifqop_free: hfsc_free,
};

/// `ifq_hfsc_ops`: the HFSC conditioner, for `ifq_attach`.
pub static IFQ_HFSC_OPS: &IfqOps = &HFSC_OPS;

//
// pf queue glue.
//

/// `hfsc_pf_ops`.
static HFSC_PF_OPS: PfqOps = PfqOps {
    pfq_alloc: hfsc_pf_alloc,
    pfq_addqueue: hfsc_pf_addqueue,
    pfq_free: hfsc_pf_free,
    pfq_qstats: hfsc_pf_qstats,
    pfq_qlength: hfsc_pf_qlength,
    pfq_enqueue: hfsc_pf_enqueue,
    pfq_deq_begin: hfsc_pf_deq_begin,
    pfq_deq_commit: hfsc_pf_deq_commit,
    pfq_purge: hfsc_pf_purge,
};

/// `pfq_hfsc_ops`: HFSC as pf configures it.
pub static PFQ_HFSC_OPS: &PfqOps = &HFSC_PF_OPS;

/// `HFSC_ENABLED(ifq)`: whether the send queue runs HFSC.
pub fn hfsc_enabled(ifq: &Ifqueue) -> bool {
    ifq.ifq_ops
        .get()
        .is_some_and(|ops| ptr::eq(ops, IFQ_HFSC_OPS))
}

/// The softc behind an opaque discipline pointer.
///
/// # Safety
///
/// `p` came from `hfsc_pf_alloc` and `hfsc_free` has not run on it.
unsafe fn hif_of(p: *mut c_void) -> &'static HfscIf {
    // SAFETY: the caller's contract; the softc stays allocated until `hfsc_free`.
    unsafe { &*p.cast::<HfscIf>() }
}

/// The softc of an HFSC send queue.
///
/// # Safety
///
/// `ifq_q` belongs to `IFQ_HFSC_OPS` (`ifq_attach` installed it with the softc).
unsafe fn ifq_hif(ifq: &Ifqueue) -> &'static HfscIf {
    // SAFETY: the caller's contract.
    unsafe { hif_of(ifq.ifq_q.get()) }
}

/// The class queue behind an opaque per-class pointer.
///
/// # Safety
///
/// `p` is the `cl_q` of a live class (`hfsc_pf_addqueue` set it as `cl_qdata`).
unsafe fn classq_of(p: *mut c_void) -> &'static HfscClassq {
    // SAFETY: the caller's contract; the class outlives its queue's use.
    unsafe { &*p.cast::<HfscClassq>() }
}

//
// shortcuts for repeated use
//

/// `hfsc_class_qlength`.
fn hfsc_class_qlength(cl: &HfscClass) -> u32 {
    // Only leaf classes have a queue
    match cl.cl_qops.get() {
        Some(qops) => (qops.pfq_qlength)(cl.cl_qdata.get()),
        None => 0,
    }
}

/// The queue manager of a leaf class (`cl->cl_qops`, which the C dereferences).
fn hfsc_class_qops(cl: &HfscClass) -> &'static PfqOps {
    match cl.cl_qops.get() {
        Some(qops) => qops,
        None => panic(format_args!(
            "hfsc: class {} has no queue",
            cl.cl_handle.get()
        )),
    }
}

/// `hfsc_class_enqueue`.
fn hfsc_class_enqueue(cl: &HfscClass, m: &'static Mbuf) -> Option<&'static Mbuf> {
    (hfsc_class_qops(cl).pfq_enqueue)(cl.cl_qdata.get(), m)
}

/// `hfsc_class_deq_begin`.
fn hfsc_class_deq_begin(cl: &HfscClass, ml: &MbufList) -> Option<&'static Mbuf> {
    let mut cookie = cl.cl_cookie.get();
    let m = (hfsc_class_qops(cl).pfq_deq_begin)(cl.cl_qdata.get(), &mut cookie, ml);
    cl.cl_cookie.set(cookie);
    m
}

/// `hfsc_class_deq_commit`.
fn hfsc_class_deq_commit(cl: &HfscClass, m: &'static Mbuf) {
    (hfsc_class_qops(cl).pfq_deq_commit)(cl.cl_qdata.get(), m, cl.cl_cookie.get());
}

/// `hfsc_class_purge`.
fn hfsc_class_purge(cl: &HfscClass, ml: &MbufList) {
    // Only leaf classes have a queue
    if let Some(qops) = cl.cl_qops.get() {
        (qops.pfq_purge)(cl.cl_qdata.get(), ml);
    }
}

/// `hfsc_more_slots`: twice the slots, at most `HFSC_MAX_CLASSES`.
fn hfsc_more_slots(current: u32) -> u32 {
    let want = current.wrapping_mul(2);

    if want > HFSC_MAX_CLASSES {
        HFSC_MAX_CLASSES
    } else {
        want
    }
}

/// A zeroed `mallocarray` of `n` class slots.
fn hfsc_class_tbl_alloc(n: u32) -> *const Cell<Option<&'static HfscClass>> {
    let slot = size_of::<Cell<Option<&'static HfscClass>>>();
    let Some(tbl) = mallocarray(n as usize, slot, M_DEVBUF, M_WAITOK | M_ZERO) else {
        panic(format_args!("hfsc: out of memory for {} classes", n));
    };
    // All-zero slots are `None`.
    tbl.as_ptr().cast_const().cast()
}

/// Frees a class table of `n` slots.
fn hfsc_class_tbl_free(tbl: *const Cell<Option<&'static HfscClass>>, n: u32) {
    if let Some(tbl) = NonNull::new(tbl.cast_mut().cast::<u8>()) {
        free(
            tbl,
            M_DEVBUF,
            n as usize * size_of::<Cell<Option<&'static HfscClass>>>(),
        );
    }
}

/// `hfsc_grow_class_tbl`: moves the class table to one of `howmany` slots.
fn hfsc_grow_class_tbl(hif: &HfscIf, howmany: u32) {
    let old = hif.hif_class_tbl.get();
    let oldlen = hif.hif_allocated.get();

    let newtbl = hfsc_class_tbl_alloc(howmany);

    // SAFETY: the old table has `oldlen` slots, the new one at least as many (`howmany` is
    // larger), and they are distinct allocations.
    unsafe { ptr::copy_nonoverlapping(old, newtbl.cast_mut(), oldlen as usize) };
    hif.hif_class_tbl.set(newtbl);
    hif.hif_allocated.set(howmany);

    hfsc_class_tbl_free(old, oldlen);
}

/// `hfsc_initialize`: sets up the class and curve pools (`pfattach`).
pub fn hfsc_initialize() {
    pf_pool_init::<HfscClass>(&HFSC_CLASS_PL, IPL_NONE, PR_WAITOK, "hfscclass");
    pf_pool_init::<HfscInternalSc>(&HFSC_INTERNAL_SC_PL, IPL_NONE, PR_WAITOK, "hfscintsc");
}

/// `hfsc_pf_alloc`: a new, empty softc for `ifp`.
fn hfsc_pf_alloc(ifp: &'static Ifnet) -> *mut c_void {
    let Some(mem) = malloc(size_of::<HfscIf>(), M_DEVBUF, M_WAITOK | M_ZERO) else {
        panic(format_args!("hfsc_pf_alloc: out of memory"));
    };
    let hif = mem.as_ptr().cast::<HfscIf>();
    // SAFETY: a fresh allocation of `size_of::<HfscIf>()` bytes, aligned (malloc's chunks are
    // aligned to their size), written once before use.
    unsafe {
        hif.write(HfscIf {
            hif_rootclass: Cell::new(None),
            hif_defaultclass: Cell::new(None),
            hif_class_tbl: Cell::new(ptr::null()),
            hif_microtime: Cell::new(0),
            hif_allocated: Cell::new(0),
            hif_classes: Cell::new(0),
            hif_classid: Cell::new(0),
            hif_eligible: TailqHead::new(),
            hif_defer: Timeout::zeroed(),
        })
    };
    // SAFETY: just written; it lives until `hfsc_free`.
    let hif_ref = unsafe { &*hif };

    hif_ref.hif_eligible.init();
    hif_ref
        .hif_class_tbl
        .set(hfsc_class_tbl_alloc(HFSC_DEFAULT_CLASSES));
    hif_ref.hif_allocated.set(HFSC_DEFAULT_CLASSES);

    timeout_set(
        &hif_ref.hif_defer,
        hfsc_deferred,
        ptr::from_ref(ifp).cast_mut().cast(),
    );

    hif.cast()
}

/// `hfsc_pf_addqueue`: adds the class of queue `q` (creating the root class first for a
/// root queue) and attaches its queue manager.
fn hfsc_pf_addqueue(arg: *mut c_void, q: &'static PfQueuespec) -> Result<(), Errno> {
    kassert!(!arg.is_null());
    // SAFETY: the `pfq_ops` contract: `arg` came from `hfsc_pf_alloc`.
    let hif = unsafe { hif_of(arg) };
    let mut np = None;
    kassert!(q.qid != 0);

    // Root queue must have non-zero linksharing parameters
    if q.linkshare.m1.absolute == 0 && q.linkshare.m2.absolute == 0 && q.parent_qid == 0 {
        return Err(Errno::EINVAL);
    }

    let parent = if q.parent_qid == 0 && hif.hif_rootclass.get().is_none() {
        np = hfsc_class_create(hif, None, None, None, None, 0, 0, HFSC_ROOT_CLASS | q.qid);
        match np {
            Some(np) => np,
            None => return Err(Errno::EINVAL),
        }
    } else {
        match hfsc_clh2cph(hif, q.parent_qid) {
            Some(parent) => parent,
            None => return Err(Errno::EINVAL),
        }
    };

    if hfsc_clh2cph(hif, q.qid).is_some() {
        let _ = hfsc_class_destroy(hif, np);
        return Err(Errno::EBUSY);
    }

    let rtsc = HfscSc::new(q.realtime.m1.absolute, q.realtime.d, q.realtime.m2.absolute);
    let lssc = HfscSc::new(
        q.linkshare.m1.absolute,
        q.linkshare.d,
        q.linkshare.m2.absolute,
    );
    let ulsc = HfscSc::new(
        q.upperlimit.m1.absolute,
        q.upperlimit.d,
        q.upperlimit.m2.absolute,
    );

    let Some(cl) = hfsc_class_create(
        hif,
        Some(&rtsc),
        Some(&lssc),
        Some(&ulsc),
        Some(parent),
        q.qlimit as i32,
        q.flags as i32,
        q.qid,
    ) else {
        let _ = hfsc_class_destroy(hif, np);
        return Err(Errno::ENOMEM);
    };

    // Attach a queue manager if specified
    cl.cl_qops.set(pf_queue_manager(q));
    // Realtime class cannot be used with an external queue manager
    match cl.cl_qops.get() {
        Some(qops) if cl.cl_rsc.get().is_none() => {
            // pf_create_queues adds only the queues of attached interfaces.
            let Some(ifp) = q.kif().and_then(|kif| kif.pfik_ifp()) else {
                cl.cl_qops.set(None);
                let _ = hfsc_class_destroy(hif, Some(cl));
                let _ = hfsc_class_destroy(hif, np);
                return Err(Errno::EINVAL);
            };
            cl.cl_qdata.set((qops.pfq_alloc)(ifp));
            if cl.cl_qdata.get().is_null() {
                cl.cl_qops.set(None);
                let _ = hfsc_class_destroy(hif, Some(cl));
                let _ = hfsc_class_destroy(hif, np);
                return Err(Errno::ENOMEM);
            }
            if let Err(error) = (qops.pfq_addqueue)(cl.cl_qdata.get(), q) {
                (qops.pfq_free)(cl.cl_qdata.get());
                cl.cl_qops.set(None);
                let _ = hfsc_class_destroy(hif, Some(cl));
                let _ = hfsc_class_destroy(hif, np);
                return Err(error);
            }
        }
        _ => {
            cl.cl_qops.set(Some(PFQ_HFSC_OPS));
            cl.cl_qdata.set(ptr::from_ref(&cl.cl_q).cast_mut().cast());
        }
    }

    kassert!(cl.cl_qops.get().is_some());
    kassert!(!cl.cl_qdata.get().is_null());

    Ok(())
}

/// `hfsc_pf_qstats`: copies the statistics of queue `q`'s class out to the user buffer
/// `ubuf` of `*nbytes` bytes, and sets `*nbytes` to their size.
fn hfsc_pf_qstats(q: &'static PfQueuespec, ubuf: usize, nbytes: &mut i32) -> Result<(), Errno> {
    let Some(ifp) = q.kif().and_then(|kif| kif.pfik_ifp()) else {
        return Err(Errno::EBADF);
    };
    let mut stats = HfscClassStats::default();

    if (*nbytes as usize) < size_of::<HfscClassStats>() {
        return Err(Errno::EINVAL);
    }

    let Some(qp) = ifq_q_enter(&ifp.if_snd, IFQ_HFSC_OPS) else {
        return Err(Errno::EBADF);
    };
    // SAFETY: `ifq_q_enter` returned the state of the HFSC conditioner, under `ifq_mtx`.
    let hif = unsafe { hif_of(qp.as_ptr()) };

    let Some(cl) = hfsc_clh2cph(hif, q.qid) else {
        ifq_q_leave(&ifp.if_snd, qp);
        return Err(Errno::EINVAL);
    };

    hfsc_getclstats(&mut stats, cl);
    ifq_q_leave(&ifp.if_snd, qp);

    copyout_obj(&stats, ubuf)?;

    *nbytes = size_of::<HfscClassStats>() as i32;
    Ok(())
}

/// `hfsc_pf_free`.
fn hfsc_pf_free(arg: *mut c_void) {
    // SAFETY: the `pfq_ops` contract: `arg` came from `hfsc_pf_alloc` and is no longer used.
    unsafe { hfsc_free(0, arg) };
}

/// `hfsc_pf_qlength`: the length of a class queue.
fn hfsc_pf_qlength(arg: *mut c_void) -> u32 {
    // SAFETY: the `pfq_ops` contract: `arg` is a class queue.
    let cq = unsafe { classq_of(arg) };

    ml_len(&cq.q)
}

/// `hfsc_pf_enqueue`: queues `m` on a class queue, or returns it when the queue is full.
fn hfsc_pf_enqueue(arg: *mut c_void, m: &'static Mbuf) -> Option<&'static Mbuf> {
    // SAFETY: the `pfq_ops` contract: `arg` is a class queue.
    let cq = unsafe { classq_of(arg) };

    if ml_len(&cq.q) >= cq.qlimit.get() as u32 {
        return Some(m);
    }

    ml_enqueue(&cq.q, m);
    None
}

/// `hfsc_pf_deq_begin`: the head of a class queue.
fn hfsc_pf_deq_begin(
    arg: *mut c_void,
    _cookiep: &mut *mut c_void,
    _free_ml: &MbufList,
) -> Option<&'static Mbuf> {
    // SAFETY: the `pfq_ops` contract: `arg` is a class queue.
    let cq = unsafe { classq_of(arg) };

    mbuf_list_first(&cq.q)
}

/// `hfsc_pf_deq_commit`: takes the head off a class queue.
fn hfsc_pf_deq_commit(arg: *mut c_void, _m: &'static Mbuf, _cookie: *mut c_void) {
    // SAFETY: the `pfq_ops` contract: `arg` is a class queue.
    let cq = unsafe { classq_of(arg) };

    let _ = ml_dequeue(&cq.q);
}

/// `hfsc_pf_purge`: moves a class queue's packets to `ml`.
fn hfsc_pf_purge(arg: *mut c_void, ml: &MbufList) {
    // SAFETY: the `pfq_ops` contract: `arg` is a class queue.
    let cq = unsafe { classq_of(arg) };

    ml_enlist(ml, &cq.q);
}

/// `hfsc_idx`: hfsc can only function on a single ifq and the stack understands this. When
/// the first ifq on an interface is switched to hfsc, this maps all mbufs to the first and
/// only ifq that is set up for hfsc.
fn hfsc_idx(_nqueues: u32, _m: &Mbuf) -> u32 {
    0
}

/// `hfsc_alloc`: the softc pf built is the conditioner's state.
fn hfsc_alloc(idx: u32, q: *mut c_void) -> *mut c_void {
    kassert!(idx == 0); // when hfsc is enabled we only use the first ifq
    kassert!(!q.is_null());
    let _ = idx;
    q
}

/// `hfsc_free`: destroys every class (children before their parents) and the softc.
///
/// # Safety
///
/// `q` came from `hfsc_pf_alloc` and is no longer used.
unsafe fn hfsc_free(idx: u32, q: *mut c_void) {
    // SAFETY: the caller's contract.
    let hif = unsafe { hif_of(q) };

    kernel_assert_locked();
    kassert!(idx == 0); // when hfsc is enabled we only use the first ifq
    let _ = idx;

    let _ = timeout_del(&hif.hif_defer);

    loop {
        let mut restart = 0;
        for i in 0..hif.hif_allocated.get() as usize {
            let cl = hif.class_tbl()[i].get();
            if hfsc_class_destroy(hif, cl) == Err(Errno::EBUSY) {
                restart += 1;
            }
        }
        if restart == 0 {
            break;
        }
    }

    hfsc_class_tbl_free(hif.hif_class_tbl.get(), hif.hif_allocated.get());
    if let Some(p) = NonNull::new(q.cast::<u8>()) {
        free(p, M_DEVBUF, size_of::<HfscIf>());
    }
}

/// `hfsc_purge`: moves every class's packets to `ml`.
///
/// # Safety
///
/// As for [`crate::net::ifq::IfqopPurgeFn`].
unsafe fn hfsc_purge(ifq: &Ifqueue, ml: &MbufList) {
    // SAFETY: the caller's contract: the queue's conditioner is HFSC.
    let hif = unsafe { ifq_hif(ifq) };

    let mut cl = hif.hif_rootclass.get();
    while let Some(c) = cl {
        hfsc_cl_purge(hif, c, ml);
        cl = hfsc_nextclass(c);
    }
}

/// Gives a class's internal curves and the class itself back to their pools.
fn hfsc_class_free(cl: &'static HfscClass) {
    for isc in [cl.cl_fsc.get(), cl.cl_rsc.get(), cl.cl_usc.get()]
        .into_iter()
        .flatten()
    {
        pool_put(&HFSC_INTERNAL_SC_PL, NonNull::from(isc).cast());
    }
    pf_pool_put(&HFSC_CLASS_PL, cl);
}

/// `pool_get(&hfsc_internal_sc_pl, PR_WAITOK)` then `hfsc_sc2isc(sc, isc)`.
fn hfsc_isc_get(sc: &HfscSc) -> Option<&'static HfscInternalSc> {
    let mem = pool_get(&HFSC_INTERNAL_SC_PL, PR_WAITOK)?;
    let isc = mem.as_ptr().cast::<HfscInternalSc>();
    let mut v = HfscInternalSc::default();
    hfsc_sc2isc(sc, &mut v);
    // SAFETY: the pool was initialised with items of `HfscInternalSc`'s size and alignment
    // (`hfsc_initialize`); the item is ours until `pool_put`, written once here before it is
    // shared.
    unsafe {
        isc.write(v);
        Some(&*isc)
    }
}

/// `hfsc_class_create`: a new class of handle `qid` under `parent` (the root class when
/// `None`), with the given curves (an all-zero or missing curve is no curve).
#[allow(clippy::too_many_arguments)]
fn hfsc_class_create(
    hif: &'static HfscIf,
    rsc: Option<&HfscSc>,
    fsc: Option<&HfscSc>,
    usc: Option<&HfscSc>,
    parent: Option<&'static HfscClass>,
    qlimit: i32,
    flags: i32,
    qid: u32,
) -> Option<&'static HfscClass> {
    let qlimit = if qlimit == 0 {
        HFSC_DEFAULT_QLIMIT
    } else {
        qlimit
    };

    if hif.hif_classes.get() >= hif.hif_allocated.get() {
        let newslots = hfsc_more_slots(hif.hif_allocated.get());

        if newslots == hif.hif_allocated.get() {
            return None;
        }
        hfsc_grow_class_tbl(hif, newslots);
    }

    let cl: &'static HfscClass = pf_pool_get(&HFSC_CLASS_PL, PR_WAITOK)?;
    cl.cl_actc.init();

    ml_init(&cl.cl_q.q);
    cl.cl_q.qlimit.set(qlimit);
    cl.cl_flags.set(flags);

    if let Some(rsc) = rsc.filter(|sc| sc.m1 != 0 || sc.m2 != 0) {
        let Some(isc) = hfsc_isc_get(rsc) else {
            hfsc_class_free(cl);
            return None;
        };
        cl.cl_rsc.set(Some(isc));
        let mut rt = HfscRuntimeSc::default();
        hfsc_rtsc_init(&mut rt, isc, 0, 0);
        cl.cl_deadline.set(rt);
        cl.cl_eligible.set(rt);
    }
    if let Some(fsc) = fsc.filter(|sc| sc.m1 != 0 || sc.m2 != 0) {
        let Some(isc) = hfsc_isc_get(fsc) else {
            hfsc_class_free(cl);
            return None;
        };
        cl.cl_fsc.set(Some(isc));
        let mut rt = HfscRuntimeSc::default();
        hfsc_rtsc_init(&mut rt, isc, 0, 0);
        cl.cl_virtual.set(rt);
    }
    if let Some(usc) = usc.filter(|sc| sc.m1 != 0 || sc.m2 != 0) {
        let Some(isc) = hfsc_isc_get(usc) else {
            hfsc_class_free(cl);
            return None;
        };
        cl.cl_usc.set(Some(isc));
        let mut rt = HfscRuntimeSc::default();
        hfsc_rtsc_init(&mut rt, isc, 0, 0);
        cl.cl_ulimit.set(rt);
    }

    cl.cl_id.set(hif.hif_classid.get());
    hif.hif_classid.set(hif.hif_classid.get().wrapping_add(1));
    cl.cl_handle.set(qid);
    cl.cl_parent.set(parent);

    let s = splnet();
    hif.hif_classes.set(hif.hif_classes.get() + 1);

    // Find a free slot in the class table. If the slot matching the lower bits of qid is
    // free, use this slot. Otherwise, use the first free slot.
    let tbl = hif.class_tbl();
    let i = (qid % hif.hif_allocated.get()) as usize;
    if tbl[i].get().is_none() {
        tbl[i].set(Some(cl));
    } else {
        match tbl.iter().find(|slot| slot.get().is_none()) {
            Some(slot) => slot.set(Some(cl)),
            None => {
                splx(s);
                // err_ret:
                hfsc_class_free(cl);
                return None;
            }
        }
    }

    if flags & HFSC_DEFAULTCLASS != 0 {
        hif.hif_defaultclass.set(Some(cl));
    }

    match parent {
        None => hif.hif_rootclass.set(Some(cl)),
        Some(parent) => {
            // add this class to the children list of the parent
            match parent.cl_children.get() {
                None => parent.cl_children.set(Some(cl)),
                Some(mut p) => {
                    while let Some(next) = p.cl_siblings.get() {
                        p = next;
                    }
                    p.cl_siblings.set(Some(cl));
                }
            }
        }
    }
    splx(s);

    Some(cl)
}

/// `hfsc_class_destroy`: unlinks and frees a leaf class (`EBUSY` while it has children).
/// `None` is nothing to do.
fn hfsc_class_destroy(hif: &'static HfscIf, cl: Option<&'static HfscClass>) -> Result<(), Errno> {
    let Some(cl) = cl else {
        return Ok(());
    };

    if cl.cl_children.get().is_some() {
        return Err(Errno::EBUSY);
    }

    let s = splnet();
    kassert!(hfsc_class_qlength(cl) == 0);

    if let Some(parent) = cl.cl_parent.get() {
        let mut p = parent.cl_children.get();

        if p.is_some_and(|p| ptr::eq(p, cl)) {
            parent.cl_children.set(cl.cl_siblings.get());
        } else {
            while let Some(pc) = p {
                if pc.cl_siblings.get().is_some_and(|sib| ptr::eq(sib, cl)) {
                    pc.cl_siblings.set(cl.cl_siblings.get());
                    break;
                }
                p = pc.cl_siblings.get();
            }
        }
    }

    if let Some(slot) = hif
        .class_tbl()
        .iter()
        .find(|slot| slot.get().is_some_and(|c| ptr::eq(c, cl)))
    {
        slot.set(None);
    }

    hif.hif_classes.set(hif.hif_classes.get().wrapping_sub(1));
    splx(s);

    kassert!(cl.cl_actc.is_empty());

    if hif.hif_rootclass.get().is_some_and(|c| ptr::eq(c, cl)) {
        hif.hif_rootclass.set(None);
    }
    if hif.hif_defaultclass.get().is_some_and(|c| ptr::eq(c, cl)) {
        hif.hif_defaultclass.set(None);
    }

    // Free external queue manager resources
    if let Some(qops) = cl.cl_qops.get()
        && !ptr::eq(qops, PFQ_HFSC_OPS)
    {
        (qops.pfq_free)(cl.cl_qdata.get());
    }

    for isc in [cl.cl_usc.get(), cl.cl_fsc.get(), cl.cl_rsc.get()]
        .into_iter()
        .flatten()
    {
        pool_put(&HFSC_INTERNAL_SC_PL, NonNull::from(isc).cast());
    }
    pf_pool_put(&HFSC_CLASS_PL, cl);

    Ok(())
}

/// `hfsc_nextclass`: the next class in the tree, depth first:
/// `cl = root; while let Some(c) = cl { ...; cl = hfsc_nextclass(c) }`.
fn hfsc_nextclass(cl: &'static HfscClass) -> Option<&'static HfscClass> {
    if let Some(child) = cl.cl_children.get() {
        Some(child)
    } else if let Some(sib) = cl.cl_siblings.get() {
        Some(sib)
    } else {
        let mut cl = cl.cl_parent.get();
        while let Some(c) = cl {
            if let Some(sib) = c.cl_siblings.get() {
                return Some(sib);
            }
            cl = c.cl_parent.get();
        }
        None
    }
}

/// `hfsc_enq`: queues `m` on the leaf class of its pf queue id (or the default class).
///
/// # Safety
///
/// As for [`crate::net::ifq::IfqopEnqFn`].
unsafe fn hfsc_enq(ifq: &Ifqueue, m: &'static Mbuf) -> Option<&'static Mbuf> {
    // SAFETY: the caller's contract: the queue's conditioner is HFSC.
    let hif = unsafe { ifq_hif(ifq) };

    let cl = match hfsc_clh2cph(hif, m.m_pkthdr().pf.qid.get()) {
        Some(cl) if cl.cl_children.get().is_none() => cl,
        _ => match hif.hif_defaultclass.get() {
            Some(cl) => cl,
            None => return Some(m),
        },
    };

    let dm = hfsc_class_enqueue(cl, m);

    // successfully queued.
    if !dm.is_some_and(|dm| ptr::eq(dm, m)) && hfsc_class_qlength(cl) == 1 {
        hfsc_set_active(hif, cl, m.m_pkthdr().len.get());
        if !timeout_pending(&hif.hif_defer) {
            let _ = timeout_add(&hif.hif_defer, 1);
        }
    }

    // drop occurred.
    if let Some(dm) = dm {
        pktcntr_inc(&cl.cl_stats.drop_cnt, dm.m_pkthdr().len.get());
    }

    dm
}

/// `hfsc_deq_begin`: the next packet: from the eligible class with the earliest deadline
/// (real-time criteria), else from the fitting class of minimal virtual time (link-sharing).
/// The cookie is the class.
///
/// # Safety
///
/// As for [`crate::net::ifq::IfqopDeqBeginFn`].
unsafe fn hfsc_deq_begin(ifq: &Ifqueue) -> Option<(&'static Mbuf, *mut c_void)> {
    let free_ml = MbufList::new();
    // SAFETY: the caller's contract: the queue's conditioner is HFSC.
    let hif = unsafe { ifq_hif(ifq) };

    let cur_time = nsecuptime();

    // If there are eligible classes, use real-time criteria: find the class with the
    // minimum deadline among the eligible classes.
    let cl = match hfsc_ellist_get_mindl(hif, cur_time) {
        Some(cl) => cl,
        None => {
            // use link-sharing criteria: get the class with the minimum vt in the hierarchy
            let mut cl = None;
            let mut tcl = hif.hif_rootclass.get();

            while let Some(t) = tcl
                && t.cl_children.get().is_some()
            {
                tcl = hfsc_actlist_firstfit(t, cur_time);
                let Some(t) = tcl else {
                    continue;
                };

                // Update parent's cl_cvtmin. Don't update if the new vt is smaller.
                if let Some(parent) = t.cl_parent.get()
                    && parent.cl_cvtmin.get() < t.cl_vt.get()
                {
                    parent.cl_cvtmin.set(t.cl_vt.get());
                }

                cl = Some(t);
            }
            // XXX HRTIMER plan hfsc_deferred precisely here.
            cl?
        }
    };

    let m = hfsc_class_deq_begin(cl, &free_ml);
    ifq_mfreeml(ifq, &free_ml);
    let Some(m) = m else {
        hfsc_update_sc(hif, cl, 0);
        return None;
    };

    hif.hif_microtime.set(cur_time);
    Some((m, ptr::from_ref(cl).cast_mut().cast()))
}

/// `hfsc_deq_commit`: takes the packet off its class and charges the class for it.
///
/// # Safety
///
/// As for [`crate::net::ifq::IfqopDeqCommitFn`]: `cookie` is the class `hfsc_deq_begin`
/// returned.
unsafe fn hfsc_deq_commit(ifq: &Ifqueue, m: &'static Mbuf, cookie: *mut c_void) {
    // SAFETY: the caller's contract: the queue's conditioner is HFSC.
    let hif = unsafe { ifq_hif(ifq) };
    // SAFETY: the cookie is a class of this softc (`hfsc_deq_begin`), alive under the mutex
    // the caller holds.
    let cl: &'static HfscClass = unsafe { &*cookie.cast::<HfscClass>() };

    hfsc_class_deq_commit(cl, m);
    hfsc_update_sc(hif, cl, m.m_pkthdr().len.get());

    pktcntr_inc(&cl.cl_stats.xmit_cnt, m.m_pkthdr().len.get());
}

/// `hfsc_update_sc`: charges `len` bytes to `cl` and updates its curves (`len` 0 when the
/// class had nothing to send).
fn hfsc_update_sc(hif: &'static HfscIf, cl: &'static HfscClass, len: i32) {
    let mut realtime = false;
    let cur_time = hif.hif_microtime.get();

    // check if the class was scheduled by real-time criteria
    if cl.cl_rsc.get().is_some() {
        realtime = cl.cl_e.get() <= cur_time;
    }

    hfsc_update_vf(cl, len, cur_time);
    if realtime {
        cl.cl_cumul.set(cl.cl_cumul.get().wrapping_add(len as u64));
    }

    if hfsc_class_qlength(cl) > 0 {
        // Realtime queue needs to look into the future and make calculations based on
        // that. This is the reason it can't be used with an external queue manager.
        if cl.cl_rsc.get().is_some() {
            // update ed
            kassert!(cl.cl_qops.get().is_some_and(|q| ptr::eq(q, PFQ_HFSC_OPS)));
            let next_len = mbuf_list_first(&cl.cl_q.q).map_or(0, |m0| m0.m_pkthdr().len.get());

            if realtime {
                hfsc_update_ed(hif, cl, next_len);
            } else {
                hfsc_update_d(cl, next_len);
            }
        }
    } else {
        // the class becomes passive
        hfsc_set_passive(hif, cl);
    }
}

/// `hfsc_deferred`: the softc's timeout: kicks the send queue every tick while HFSC runs,
/// for classes that were not allowed to send yet.
fn hfsc_deferred(arg: *mut c_void) {
    // SAFETY: `hfsc_pf_alloc` set the timeout with the interface, which outlives the softc
    // (`hfsc_free` deletes the timeout).
    let ifp: &'static Ifnet = unsafe { &*arg.cast::<Ifnet>() };
    let ifq = &ifp.if_snd;

    if !hfsc_enabled(ifq) {
        return;
    }

    if !ifq_empty(ifq) {
        ifq_start(ifq);
    }

    let Some(qp) = ifq_q_enter(&ifp.if_snd, IFQ_HFSC_OPS) else {
        return;
    };
    // SAFETY: `ifq_q_enter` returned the state of the HFSC conditioner, under `ifq_mtx`.
    let hif = unsafe { hif_of(qp.as_ptr()) };
    // XXX HRTIMER nearest virtual/fit time is likely less than 1/HZ.
    let _ = timeout_add(&hif.hif_defer, 1);
    ifq_q_leave(&ifp.if_snd, qp);
}

/// `hfsc_cl_purge`: moves a class's packets to `ml`; the class goes passive.
fn hfsc_cl_purge(hif: &'static HfscIf, cl: &'static HfscClass, ml: &MbufList) {
    let ml2 = MbufList::new();

    hfsc_class_purge(cl, &ml2);
    if ml_empty(&ml2) {
        return;
    }

    ml_enlist(ml, &ml2);

    hfsc_update_vf(cl, 0, 0); // remove cl from the actlist
    hfsc_set_passive(hif, cl);
}

/// `hfsc_set_active`: a class gets its first packet (of `len` bytes).
fn hfsc_set_active(hif: &'static HfscIf, cl: &'static HfscClass, len: i32) {
    if cl.cl_rsc.get().is_some() {
        hfsc_init_ed(hif, cl, len);
    }
    if cl.cl_fsc.get().is_some() {
        hfsc_init_vf(cl, len);
    }

    cl.cl_stats
        .period
        .set(cl.cl_stats.period.get().wrapping_add(1));
}

/// `hfsc_set_passive`: a class has no packet left. The actlist is handled in
/// `hfsc_update_vf`, so `hfsc_update_vf(cl, 0, 0)` must be called explicitly to remove a
/// class from it.
fn hfsc_set_passive(hif: &'static HfscIf, cl: &'static HfscClass) {
    if cl.cl_rsc.get().is_some() {
        hfsc_ellist_remove(hif, cl);
    }
}

/// The real-time curve of a class (`cl->cl_rsc`, which the callers have checked).
fn hfsc_rsc(cl: &HfscClass) -> &'static HfscInternalSc {
    match cl.cl_rsc.get() {
        Some(isc) => isc,
        None => panic(format_args!(
            "hfsc: class {} has no real-time curve",
            cl.cl_handle.get()
        )),
    }
}

/// `hfsc_init_ed`: starts the eligible and deadline times of a class that became active.
fn hfsc_init_ed(hif: &'static HfscIf, cl: &'static HfscClass, next_len: i32) {
    let rsc = hfsc_rsc(cl);
    let cur_time = nsecuptime();

    // update the deadline curve
    let mut deadline = cl.cl_deadline.get();
    hfsc_rtsc_min(&mut deadline, rsc, cur_time, cl.cl_cumul.get());
    cl.cl_deadline.set(deadline);

    // Update the eligible curve. For concave, it is equal to the deadline curve. For
    // convex, it is a linear curve with slope m2.
    let mut eligible = deadline;
    if rsc.sm1 <= rsc.sm2 {
        eligible.dx = 0;
        eligible.dy = 0;
    }
    cl.cl_eligible.set(eligible);

    // compute e and d
    cl.cl_e.set(hfsc_rtsc_y2x(&eligible, cl.cl_cumul.get()));
    cl.cl_d.set(hfsc_rtsc_y2x(
        &deadline,
        cl.cl_cumul.get().wrapping_add(next_len as u64),
    ));

    hfsc_ellist_insert(hif, cl);
}

/// `hfsc_update_ed`.
fn hfsc_update_ed(hif: &'static HfscIf, cl: &'static HfscClass, next_len: i32) {
    cl.cl_e
        .set(hfsc_rtsc_y2x(&cl.cl_eligible.get(), cl.cl_cumul.get()));
    cl.cl_d.set(hfsc_rtsc_y2x(
        &cl.cl_deadline.get(),
        cl.cl_cumul.get().wrapping_add(next_len as u64),
    ));

    hfsc_ellist_update(hif, cl);
}

/// `hfsc_update_d`.
fn hfsc_update_d(cl: &HfscClass, next_len: i32) {
    cl.cl_d.set(hfsc_rtsc_y2x(
        &cl.cl_deadline.get(),
        cl.cl_cumul.get().wrapping_add(next_len as u64),
    ));
}

/// The fair (link-sharing) curve of a class.
fn hfsc_fsc(cl: &HfscClass) -> &'static HfscInternalSc {
    match cl.cl_fsc.get() {
        Some(isc) => isc,
        None => panic(format_args!(
            "hfsc: class {} has no link-share curve",
            cl.cl_handle.get()
        )),
    }
}

/// The parent of a class below the root (the loops of `hfsc_init_vf`/`hfsc_update_vf` stop
/// at the root).
fn hfsc_parent(cl: &HfscClass) -> &'static HfscClass {
    match cl.cl_parent.get() {
        Some(p) => p,
        None => panic(format_args!(
            "hfsc: class {} has no parent",
            cl.cl_handle.get()
        )),
    }
}

/// `hfsc_init_vf`: activates `cl` and the ancestors that become active with it in the
/// link-sharing hierarchy.
fn hfsc_init_vf(cl: &'static HfscClass, _len: i32) {
    let mut cur_time = 0;
    let mut go_active = true;

    let mut cl = cl;
    while let Some(parent) = cl.cl_parent.get() {
        if go_active {
            let nactive = cl.cl_nactive.get();
            cl.cl_nactive.set(nactive.wrapping_add(1));
            go_active = nactive == 0;
        }

        if go_active {
            if let Some(max_cl) = parent.cl_actc.last() {
                // Set vt to the average of the min and max classes. If the parent's period
                // didn't change, don't decrease vt of the class.
                let mut vt = max_cl.cl_vt.get();
                if parent.cl_cvtmin.get() != 0 {
                    vt = parent.cl_cvtmin.get().wrapping_add(vt) / 2;
                }

                if parent.cl_vtperiod.get() != cl.cl_parentperiod.get() || vt > cl.cl_vt.get() {
                    cl.cl_vt.set(vt);
                }
            } else {
                // First child for a new parent backlog period. Add parent's cvtmax to vtoff
                // of children to make a new vt (vtoff + vt) larger than the vt in the last
                // period for all children.
                let vt = parent.cl_cvtmax.get();
                let mut p = parent.cl_children.get();
                while let Some(c) = p {
                    c.cl_vtoff.set(c.cl_vtoff.get().wrapping_add(vt));
                    p = c.cl_siblings.get();
                }
                cl.cl_vt.set(0);
                parent.cl_cvtmax.set(0);
                parent.cl_cvtmin.set(0);
            }
            cl.cl_initvt.set(cl.cl_vt.get());

            // update the virtual curve
            let vt = cl.cl_vt.get().wrapping_add(cl.cl_vtoff.get());
            let mut virt = cl.cl_virtual.get();
            hfsc_rtsc_min(&mut virt, hfsc_fsc(cl), vt, cl.cl_total.get());
            if virt.x == vt {
                virt.x = virt.x.wrapping_sub(cl.cl_vtoff.get());
                cl.cl_vtoff.set(0);
            }
            cl.cl_virtual.set(virt);
            cl.cl_vtadj.set(0);

            cl.cl_vtperiod.set(cl.cl_vtperiod.get().wrapping_add(1)); // increment vt period
            cl.cl_parentperiod.set(parent.cl_vtperiod.get());
            if parent.cl_nactive.get() == 0 {
                cl.cl_parentperiod
                    .set(cl.cl_parentperiod.get().wrapping_add(1));
            }
            cl.cl_f.set(0);

            hfsc_actlist_insert(cl);

            if let Some(usc) = cl.cl_usc.get() {
                // class has upper limit curve
                if cur_time == 0 {
                    cur_time = nsecuptime();
                }

                // update the ulimit curve
                let mut ulimit = cl.cl_ulimit.get();
                hfsc_rtsc_min(&mut ulimit, usc, cur_time, cl.cl_total.get());
                cl.cl_ulimit.set(ulimit);
                // compute myf
                cl.cl_myf.set(hfsc_rtsc_y2x(&ulimit, cl.cl_total.get()));
                cl.cl_myfadj.set(0);
            }
        }

        let f = if cl.cl_myf.get() > cl.cl_cfmin.get() {
            cl.cl_myf.get()
        } else {
            cl.cl_cfmin.get()
        };
        if f != cl.cl_f.get() {
            cl.cl_f.set(f);
            hfsc_update_cfmin(parent);
        }

        cl = parent;
    }
}

/// `hfsc_update_vf`: charges `len` bytes to `cl` and its ancestors in the link-sharing
/// hierarchy, moving them along their virtual curves, and makes passive the classes that
/// have nothing left.
fn hfsc_update_vf(cl: &'static HfscClass, len: i32, cur_time: u64) {
    let mut go_passive = hfsc_class_qlength(cl) == 0;

    let mut cl = cl;
    while let Some(parent) = cl.cl_parent.get() {
        cl.cl_total.set(cl.cl_total.get().wrapping_add(len as u64));

        if cl.cl_fsc.get().is_none() || cl.cl_nactive.get() == 0 {
            cl = parent;
            continue;
        }

        if go_passive {
            let nactive = cl.cl_nactive.get().wrapping_sub(1);
            cl.cl_nactive.set(nactive);
            go_passive = nactive == 0;
        }

        if go_passive {
            // no more active child, going passive

            // update cvtmax of the parent class
            if cl.cl_vt.get() > parent.cl_cvtmax.get() {
                parent.cl_cvtmax.set(cl.cl_vt.get());
            }

            // remove this class from the vt list
            hfsc_actlist_remove(cl);

            hfsc_update_cfmin(parent);

            cl = parent;
            continue;
        }

        // update vt and f
        cl.cl_vt.set(
            hfsc_rtsc_y2x(&cl.cl_virtual.get(), cl.cl_total.get())
                .wrapping_sub(cl.cl_vtoff.get())
                .wrapping_add(cl.cl_vtadj.get()),
        );

        // If vt of the class is smaller than cvtmin, the class was skipped in the past due
        // to non-fit. If so, we need to adjust vtadj.
        if cl.cl_vt.get() < parent.cl_cvtmin.get() {
            cl.cl_vtadj.set(
                cl.cl_vtadj
                    .get()
                    .wrapping_add(parent.cl_cvtmin.get() - cl.cl_vt.get()),
            );
            cl.cl_vt.set(parent.cl_cvtmin.get());
        }

        // update the vt list
        hfsc_actlist_update(cl);

        if cl.cl_usc.get().is_some() {
            cl.cl_myf.set(
                cl.cl_myfadj
                    .get()
                    .wrapping_add(hfsc_rtsc_y2x(&cl.cl_ulimit.get(), cl.cl_total.get())),
            );

            // If myf lags behind by more than one clock tick from the current time, adjust
            // myfadj to prevent a rate-limited class from going greedy. In a steady state
            // under rate-limiting, myf fluctuates within one clock tick.
            let myf_bound = cur_time.wrapping_sub(hfsc_clk_per_tick());
            if cl.cl_myf.get() < myf_bound {
                let delta = cur_time.wrapping_sub(cl.cl_myf.get());
                cl.cl_myfadj.set(cl.cl_myfadj.get().wrapping_add(delta));
                cl.cl_myf.set(cl.cl_myf.get().wrapping_add(delta));
            }
        }

        // cl_f is max(cl_myf, cl_cfmin)
        let f = if cl.cl_myf.get() > cl.cl_cfmin.get() {
            cl.cl_myf.get()
        } else {
            cl.cl_cfmin.get()
        };
        if f != cl.cl_f.get() {
            cl.cl_f.set(f);
            hfsc_update_cfmin(parent);
        }

        cl = parent;
    }
}

/// `hfsc_update_cfmin`: the earliest fit-time of the active children (0 if one has none).
fn hfsc_update_cfmin(cl: &HfscClass) {
    if cl.cl_actc.is_empty() {
        cl.cl_cfmin.set(0);
        return;
    }
    let mut cfmin = HFSC_HT_INFINITY;
    for p in cl.cl_actc.iter() {
        if p.cl_f.get() == 0 {
            cl.cl_cfmin.set(0);
            return;
        }
        if p.cl_f.get() < cfmin {
            cfmin = p.cl_f.get();
        }
    }
    cl.cl_cfmin.set(cfmin);
}

//
// The eligible list holds backlogged classes sorted by their eligible times. There is one
// eligible list per interface.
//

/// `hfsc_ellist_insert`.
fn hfsc_ellist_insert(hif: &'static HfscIf, cl: &'static HfscClass) {
    // check the last entry first
    if hif
        .hif_eligible
        .last()
        .is_none_or(|p| p.cl_e.get() <= cl.cl_e.get())
    {
        // SAFETY: a class enters the eligible list when it becomes active and leaves it when
        // it goes passive, so it is in no eligible list now; a pool item stays in place
        // until it is freed, after it left the list.
        unsafe { hif.hif_eligible.insert_tail(cl) };
        return;
    }

    if let Some(p) = hif
        .hif_eligible
        .iter()
        .find(|p| cl.cl_e.get() < p.cl_e.get())
    {
        // SAFETY: as above; `p` is in the list.
        unsafe { TailqHead::<HfscEligible>::insert_before(p, cl) };
    }
}

/// `hfsc_ellist_remove`.
fn hfsc_ellist_remove(hif: &'static HfscIf, cl: &HfscClass) {
    // SAFETY: an active real-time class is in the eligible list (`hfsc_init_ed`).
    unsafe { hif.hif_eligible.remove(cl) };
}

/// `hfsc_ellist_update`: moves a class whose eligible time grew to its new position.
fn hfsc_ellist_update(hif: &'static HfscIf, cl: &'static HfscClass) {
    // The eligible time of a class increases monotonically. If the next entry has a larger
    // eligible time, nothing to do.
    let Some(mut p) = TailqHead::<HfscEligible>::next(cl) else {
        return;
    };
    if cl.cl_e.get() <= p.cl_e.get() {
        return;
    }

    // check the last entry
    if let Some(last) = hif.hif_eligible.last()
        && last.cl_e.get() <= cl.cl_e.get()
    {
        // SAFETY: `cl` is in the list (it has a next entry); it is relinked at the tail.
        unsafe {
            hif.hif_eligible.remove(cl);
            hif.hif_eligible.insert_tail(cl);
        }
        return;
    }

    // the new position must be between the next entry and the last entry
    while let Some(n) = TailqHead::<HfscEligible>::next(p) {
        p = n;
        if cl.cl_e.get() < p.cl_e.get() {
            // SAFETY: `cl` is in the list; it is relinked before `p`, another member.
            unsafe {
                hif.hif_eligible.remove(cl);
                TailqHead::<HfscEligible>::insert_before(p, cl);
            }
            return;
        }
    }
}

/// `hfsc_ellist_get_mindl`: the class with the minimum deadline among the eligible classes.
fn hfsc_ellist_get_mindl(hif: &'static HfscIf, cur_time: u64) -> Option<&'static HfscClass> {
    let mut cl: Option<&'static HfscClass> = None;

    for p in hif.hif_eligible.iter() {
        if p.cl_e.get() > cur_time {
            break;
        }
        if cl.is_none_or(|c| p.cl_d.get() < c.cl_d.get()) {
            cl = Some(p);
        }
    }
    cl
}

//
// The active children list holds backlogged child classes sorted by their virtual time.
// Each intermediate class has one active children list.
//

/// `hfsc_actlist_insert`.
fn hfsc_actlist_insert(cl: &'static HfscClass) {
    let parent = hfsc_parent(cl);

    // check the last entry first
    if parent
        .cl_actc
        .last()
        .is_none_or(|p| p.cl_vt.get() <= cl.cl_vt.get())
    {
        // SAFETY: a class enters its parent's active list when it becomes active and leaves
        // it when it goes passive (`hfsc_update_vf`); it stays in place while linked.
        unsafe { parent.cl_actc.insert_tail(cl) };
        return;
    }

    if let Some(p) = parent
        .cl_actc
        .iter()
        .find(|p| cl.cl_vt.get() < p.cl_vt.get())
    {
        // SAFETY: as above; `p` is in the list.
        unsafe { TailqHead::<HfscActive>::insert_before(p, cl) };
    }
}

/// `hfsc_actlist_remove`.
fn hfsc_actlist_remove(cl: &HfscClass) {
    // SAFETY: an active class is in its parent's active list (`hfsc_init_vf`).
    unsafe { hfsc_parent(cl).cl_actc.remove(cl) };
}

/// `hfsc_actlist_update`: moves a class whose virtual time grew to its new position.
fn hfsc_actlist_update(cl: &'static HfscClass) {
    let parent = hfsc_parent(cl);

    // The virtual time of a class increases monotonically during its backlogged period. If
    // the next entry has a larger virtual time, nothing to do.
    let Some(mut p) = TailqHead::<HfscActive>::next(cl) else {
        return;
    };
    if cl.cl_vt.get() < p.cl_vt.get() {
        return;
    }

    // check the last entry
    if let Some(last) = parent.cl_actc.last()
        && last.cl_vt.get() <= cl.cl_vt.get()
    {
        // SAFETY: `cl` is in the list (it has a next entry); it is relinked at the tail.
        unsafe {
            parent.cl_actc.remove(cl);
            parent.cl_actc.insert_tail(cl);
        }
        return;
    }

    // the new position must be between the next entry and the last entry
    while let Some(n) = TailqHead::<HfscActive>::next(p) {
        p = n;
        if cl.cl_vt.get() < p.cl_vt.get() {
            // SAFETY: `cl` is in the list; it is relinked before `p`, another member.
            unsafe {
                parent.cl_actc.remove(cl);
                TailqHead::<HfscActive>::insert_before(p, cl);
            }
            return;
        }
    }
}

/// `hfsc_actlist_firstfit`: the active child of minimal virtual time whose fit-time has
/// come.
fn hfsc_actlist_firstfit(cl: &'static HfscClass, cur_time: u64) -> Option<&'static HfscClass> {
    cl.cl_actc.iter().find(|p| p.cl_f.get() <= cur_time)
}

//
// Service curve support functions.
//
//  external service curve parameters
//	m: bits/sec
//	d: msec
//  internal service curve parameters
//	sm: (bytes/tsc_interval) << SM_SHIFT
//	ism: (tsc_count/byte) << ISM_SHIFT
//	dx: tsc_count
//
// SM_SHIFT and ISM_SHIFT are scaled in order to keep effective digits. We should be able to
// handle 100K-1Gbps linkspeed with 200Hz-1GHz CPU speed. SM_SHIFT and ISM_SHIFT are selected
// to have at least 3 effective digits in decimal using the following table.
//
//  bits/sec    100Kbps     1Mbps     10Mbps     100Mbps    1Gbps
//  ----------+-------------------------------------------------------
//  bytes/nsec  12.5e-6    125e-6     1250e-6    12500e-6   125000e-6
//  sm(500MHz)  25.0e-6    250e-6     2500e-6    25000e-6   250000e-6
//  sm(200MHz)  62.5e-6    625e-6     6250e-6    62500e-6   625000e-6
//
//  nsec/byte   80000      8000       800        80         8
//  ism(500MHz) 40000      4000       400        40         4
//  ism(200MHz) 16000      1600       160        16         1.6
//

/// `SM_SHIFT`.
const SM_SHIFT: u32 = 24;
/// `ISM_SHIFT`.
const ISM_SHIFT: u32 = 10;

/// `SM_MASK`.
const SM_MASK: u64 = (1 << SM_SHIFT) - 1;
/// `ISM_MASK`.
const ISM_MASK: u64 = (1 << ISM_SHIFT) - 1;

/// `seg_x2y`: `y = x * sm >> SM_SHIFT`, computed on the upper and lower bits of `x`
/// separately to avoid overflow.
fn seg_x2y(x: u64, sm: u64) -> u64 {
    (x >> SM_SHIFT)
        .wrapping_mul(sm)
        .wrapping_add((x & SM_MASK).wrapping_mul(sm) >> SM_SHIFT)
}

/// `seg_y2x`: `x = y * ism >> ISM_SHIFT` (infinite for an infinite inverse slope).
fn seg_y2x(y: u64, ism: u64) -> u64 {
    if y == 0 {
        0
    } else if ism == HFSC_HT_INFINITY {
        HFSC_HT_INFINITY
    } else {
        (y >> ISM_SHIFT)
            .wrapping_mul(ism)
            .wrapping_add((y & ISM_MASK).wrapping_mul(ism) >> ISM_SHIFT)
    }
}

/// `m2sm`: bits/sec to the scaled slope.
fn m2sm(m: u64) -> u64 {
    (m << SM_SHIFT) / 8 / HFSC_FREQ
}

/// `m2ism`: bits/sec to the scaled inverse slope (infinite for 0).
fn m2ism(m: u64) -> u64 {
    ((HFSC_FREQ << ISM_SHIFT) * 8)
        .checked_div(m)
        .unwrap_or(HFSC_HT_INFINITY)
}

/// `d2dx`: msec to clock ticks.
fn d2dx(d: u32) -> u64 {
    u64::from(d) * HFSC_FREQ / 1000
}

/// `sm2m`: the scaled slope back to bits/sec.
fn sm2m(sm: u64) -> u64 {
    sm.wrapping_mul(8).wrapping_mul(HFSC_FREQ) >> SM_SHIFT
}

/// `dx2d`: clock ticks back to msec.
fn dx2d(dx: u64) -> u32 {
    (dx.wrapping_mul(1000) / HFSC_FREQ) as u32
}

/// `hfsc_sc2isc`: a service curve in its internal representation.
fn hfsc_sc2isc(sc: &HfscSc, isc: &mut HfscInternalSc) {
    isc.sm1 = m2sm(sc.m1);
    isc.ism1 = m2ism(sc.m1);
    isc.dx = d2dx(sc.d);
    isc.dy = seg_x2y(isc.dx, isc.sm1);
    isc.sm2 = m2sm(sc.m2);
    isc.ism2 = m2ism(sc.m2);
}

/// `hfsc_rtsc_init`: initializes the runtime service curve with the given internal service
/// curve starting at (x, y).
fn hfsc_rtsc_init(rtsc: &mut HfscRuntimeSc, isc: &HfscInternalSc, x: u64, y: u64) {
    rtsc.x = x;
    rtsc.y = y;
    rtsc.sm1 = isc.sm1;
    rtsc.ism1 = isc.ism1;
    rtsc.dx = isc.dx;
    rtsc.dy = isc.dy;
    rtsc.sm2 = isc.sm2;
    rtsc.ism2 = isc.ism2;
}

/// `hfsc_rtsc_y2x`: the x-projection of the runtime service curve at y-projection `y`.
fn hfsc_rtsc_y2x(rtsc: &HfscRuntimeSc, y: u64) -> u64 {
    if y < rtsc.y {
        rtsc.x
    } else if y <= rtsc.y.wrapping_add(rtsc.dy) {
        // x belongs to the 1st segment
        if rtsc.dy == 0 {
            rtsc.x.wrapping_add(rtsc.dx)
        } else {
            rtsc.x.wrapping_add(seg_y2x(y - rtsc.y, rtsc.ism1))
        }
    } else {
        // x belongs to the 2nd segment
        rtsc.x.wrapping_add(rtsc.dx).wrapping_add(seg_y2x(
            y.wrapping_sub(rtsc.y).wrapping_sub(rtsc.dy),
            rtsc.ism2,
        ))
    }
}

/// `hfsc_rtsc_x2y`: the y-projection of the runtime service curve at x-projection `x`.
fn hfsc_rtsc_x2y(rtsc: &HfscRuntimeSc, x: u64) -> u64 {
    if x <= rtsc.x {
        rtsc.y
    } else if x <= rtsc.x.wrapping_add(rtsc.dx) {
        // y belongs to the 1st segment
        rtsc.y.wrapping_add(seg_x2y(x - rtsc.x, rtsc.sm1))
    } else {
        // y belongs to the 2nd segment
        rtsc.y.wrapping_add(rtsc.dy).wrapping_add(seg_x2y(
            x.wrapping_sub(rtsc.x).wrapping_sub(rtsc.dx),
            rtsc.sm2,
        ))
    }
}

/// `hfsc_rtsc_min`: updates the runtime service curve to the minimum of itself and the
/// service curve `isc` starting at (x, y).
fn hfsc_rtsc_min(rtsc: &mut HfscRuntimeSc, isc: &HfscInternalSc, x: u64, y: u64) {
    if isc.sm1 <= isc.sm2 {
        // service curve is convex
        let y1 = hfsc_rtsc_x2y(rtsc, x);
        if y1 < y {
            // the current rtsc is smaller
            return;
        }
        rtsc.x = x;
        rtsc.y = y;
        return;
    }

    // Service curve is concave. Compute the two y values of the current rtsc: y1 at x, y2 at
    // (x + dx).
    let y1 = hfsc_rtsc_x2y(rtsc, x);
    if y1 <= y {
        // rtsc is below isc, no change to rtsc
        return;
    }

    let y2 = hfsc_rtsc_x2y(rtsc, x.wrapping_add(isc.dx));
    if y2 >= y.wrapping_add(isc.dy) {
        // rtsc is above isc, replace rtsc by isc
        rtsc.x = x;
        rtsc.y = y;
        rtsc.dx = isc.dx;
        rtsc.dy = isc.dy;
        return;
    }

    // The two curves intersect. Compute the offsets (dx, dy) using the reverse function of
    // seg_x2y(): seg_x2y(dx, sm1) == seg_x2y(dx, sm2) + (y1 - y).
    let mut dx = ((y1 - y) << SM_SHIFT) / (isc.sm1 - isc.sm2);
    // Check if (x, y1) belongs to the 1st segment of rtsc. If so, add the offset.
    if rtsc.x.wrapping_add(rtsc.dx) > x {
        dx = dx.wrapping_add(rtsc.x.wrapping_add(rtsc.dx) - x);
    }
    let dy = seg_x2y(dx, isc.sm1);

    rtsc.x = x;
    rtsc.y = y;
    rtsc.dx = dx;
    rtsc.dy = dy;
}

/// The external form of an internal curve (all zero when the class has none).
fn hfsc_isc2sc(isc: Option<&HfscInternalSc>) -> HfscSc {
    match isc {
        Some(isc) => HfscSc::new(sm2m(isc.sm1), dx2d(isc.dx), sm2m(isc.sm2)),
        None => HfscSc::new(0, 0, 0),
    }
}

/// `hfsc_getclstats`: a class's statistics for `DIOCGETQSTATS`.
fn hfsc_getclstats(sp: &mut HfscClassStats, cl: &HfscClass) {
    sp.class_id = cl.cl_id.get();
    sp.class_handle = cl.cl_handle.get();

    sp.rsc = hfsc_isc2sc(cl.cl_rsc.get());
    sp.fsc = hfsc_isc2sc(cl.cl_fsc.get());
    sp.usc = hfsc_isc2sc(cl.cl_usc.get());

    sp.total = cl.cl_total.get();
    sp.cumul = cl.cl_cumul.get();

    sp.d = cl.cl_d.get();
    sp.e = cl.cl_e.get();
    sp.vt = cl.cl_vt.get();
    sp.f = cl.cl_f.get();

    sp.initvt = cl.cl_initvt.get();
    sp.vtperiod = cl.cl_vtperiod.get();
    sp.parentperiod = cl.cl_parentperiod.get();
    sp.nactive = cl.cl_nactive.get();
    sp.vtoff = cl.cl_vtoff.get();
    sp.cvtmax = cl.cl_cvtmax.get();
    sp.myf = cl.cl_myf.get();
    sp.cfmin = cl.cl_cfmin.get();
    sp.cvtmin = cl.cl_cvtmin.get();
    sp.myfadj = cl.cl_myfadj.get();
    sp.vtadj = cl.cl_vtadj.get();

    sp.cur_time = nsecuptime();
    sp.machclk_freq = HFSC_FREQ as u32;

    sp.qlength = hfsc_class_qlength(cl);
    sp.qlimit = cl.cl_q.qlimit.get() as u32;
    sp.xmit_cnt = cl.cl_stats.xmit_cnt.get();
    sp.drop_cnt = cl.cl_stats.drop_cnt.get();
    sp.period = cl.cl_stats.period.get();

    sp.qtype = 0;
}

/// `hfsc_clh2cph`: converts a class handle to the corresponding class.
fn hfsc_clh2cph(hif: &'static HfscIf, chandle: u32) -> Option<&'static HfscClass> {
    if chandle == 0 {
        return None;
    }
    let tbl = hif.class_tbl();
    // First, try the slot corresponding to the lower bits of the handle. If it does not
    // match, do the linear table search.
    let i = (chandle % hif.hif_allocated.get()) as usize;
    if let Some(cl) = tbl[i].get()
        && cl.cl_handle.get() == chandle
    {
        return Some(cl);
    }
    tbl.iter()
        .filter_map(Cell::get)
        .find(|cl| cl.cl_handle.get() == chandle)
}

const _: () = {
    assert!(size_of::<HfscPktcntr>() == 16);
    assert!(size_of::<HfscSc>() == 24);
    assert!(size_of::<HfscClassStats>() == 272);
    assert!(core::mem::offset_of!(HfscClassStats, rsc) == 56);
    assert!(core::mem::offset_of!(HfscClassStats, total) == 128);
    assert!(core::mem::offset_of!(HfscClassStats, machclk_freq) == 248);
    assert!(core::mem::offset_of!(HfscClassStats, qtype) == 264);
};
/* </CODE> */

/* <TESTS> */
#[cfg(test)]
mod tests {
    // Host tests for HFSC: the service-curve arithmetic, the statistics ABI and a two-class
    // link-sharing hierarchy on a send queue.

    use std::boxed::Box;
    use std::sync::MutexGuard;

    use super::*;
    use crate::kern::uipc_mbuf::m_freem;
    use crate::net::if_::tests::{setup_net, test_ifnet, test_packet, zeroed_static};
    use crate::net::ifq::{IFQ_PRIQ_OPS, ifq_attach, ifq_dequeue, ifq_enqueue, ifq_init, ifq_len};
    use crate::net::pfvar::{PFQS_DEFAULT, PfiKif, pf_abi_zeroed};

    #[test]
    fn m2sm_and_sm2m_round_trip() {
        // 1 byte per nanosecond is exactly 1 << SM_SHIFT, and back.
        assert_eq!(m2sm(8_000_000_000), 1 << SM_SHIFT);
        assert_eq!(sm2m(1 << SM_SHIFT), 8_000_000_000);
        assert_eq!(m2ism(8_000_000_000), 1 << ISM_SHIFT);
        assert_eq!(m2ism(0), HFSC_HT_INFINITY);

        // 100Kbps to 1Gbps keep at least three significant digits (the table in the C: at a
        // 1GHz clock 100Kbps is a scaled slope of 209).
        for m in [
            100_000u64,
            1_000_000,
            10_000_000,
            100_000_000,
            1_000_000_000,
        ] {
            let back = sm2m(m2sm(m));
            assert!(back <= m, "{m}: {back}");
            assert!(m - back <= m / 200, "{m}: {back}");
        }
        assert!(1_000_000_000 - sm2m(m2sm(1_000_000_000)) <= 1_000_000_000 / 100_000);

        assert_eq!(d2dx(10), 10_000_000);
        assert_eq!(dx2d(d2dx(10)), 10);
        assert_eq!(dx2d(d2dx(u32::MAX)), u32::MAX);
    }

    #[test]
    fn seg_x2y_and_seg_y2x_are_inverse() {
        // At 1 byte/ns the segment is the identity, even for coordinates whose product with
        // the slope would overflow 64 bits (the split into upper and lower bits).
        let sm = m2sm(8_000_000_000);
        let ism = m2ism(8_000_000_000);
        for x in [0u64, 1, 12345, 1 << 40, u64::MAX >> 8] {
            assert_eq!(seg_x2y(x, sm), x);
            assert_eq!(seg_y2x(x, ism), x);
        }

        // 100Mbps: 12.5 bytes per microsecond.
        let sm = m2sm(100_000_000);
        let ism = m2ism(100_000_000);
        let y = seg_x2y(1_000_000, sm); // 1ms
        assert!((12_499..=12_500).contains(&y), "{y}");
        let x = seg_y2x(12_500, ism);
        assert!((999_000..=1_001_000).contains(&x), "{x}");

        assert_eq!(seg_y2x(0, HFSC_HT_INFINITY), 0);
        assert_eq!(seg_y2x(1, HFSC_HT_INFINITY), HFSC_HT_INFINITY);
    }

    #[test]
    fn runtime_curves_follow_both_segments() {
        // Concave: 800Mbps (100 bytes/us) for 1ms, then 80Mbps.
        let mut isc = HfscInternalSc::default();
        hfsc_sc2isc(&HfscSc::new(800_000_000, 1, 80_000_000), &mut isc);
        assert_eq!(isc.dx, 1_000_000);
        assert!((99_999..=100_000).contains(&isc.dy), "{}", isc.dy);

        let mut rt = HfscRuntimeSc::default();
        hfsc_rtsc_init(&mut rt, &isc, 1000, 500);
        assert_eq!(hfsc_rtsc_x2y(&rt, 0), 500);
        assert_eq!(hfsc_rtsc_y2x(&rt, 0), 1000);
        // In the first segment, then in the second; y2x inverts x2y to within a nanosecond per
        // byte of rounding.
        for dx in [10_000u64, 500_000, 2_000_000, 7_000_000] {
            let y = hfsc_rtsc_x2y(&rt, 1000 + dx);
            let x = hfsc_rtsc_y2x(&rt, y);
            assert!(x.abs_diff(1000 + dx) <= 200, "{dx}: {y} -> {x}");
        }
        // Past the first segment the slope is the second one (10 bytes/us).
        let y1 = hfsc_rtsc_x2y(&rt, 1000 + 2_000_000);
        let y2 = hfsc_rtsc_x2y(&rt, 1000 + 3_000_000);
        assert!((9_999..=10_000).contains(&(y2 - y1)), "{}", y2 - y1);

        // The minimum with the same curve started later and lower takes the new start.
        hfsc_rtsc_min(&mut rt, &isc, 5_000_000, 0);
        assert_eq!((rt.x, rt.y), (5_000_000, 0));
    }

    #[test]
    fn class_stats_have_the_c_layout() {
        assert_eq!(size_of::<HfscClassStats>(), 272);
        assert_eq!(size_of::<HfscSc>(), 24);
        assert_eq!(core::mem::offset_of!(HfscClassStats, class_handle), 48);
        assert_eq!(core::mem::offset_of!(HfscClassStats, rsc), 56);
        assert_eq!(core::mem::offset_of!(HfscClassStats, cur_time), 240);
        assert_eq!(core::mem::offset_of!(HfscClassStats, nactive), 260);
    }

    /// The network test setup with the timeout wheel reset (HFSC arms `hif_defer`), under the
    /// wheel's test lock too.
    fn setup() -> (MutexGuard<'static, ()>, MutexGuard<'static, ()>) {
        let g = setup_net();
        let t = crate::kern::kern_timeout::tests::LOCK
            .lock()
            .unwrap_or_else(|e| e.into_inner());
        crate::kern::kern_timeout::timeout_startup();
        (g, t)
    }

    /// A kernel queue spec on `kif`: link-share `ls` bits/sec under `parent_qid`.
    fn queue(
        kif: &'static PfiKif,
        qid: u32,
        parent_qid: u32,
        ls: u64,
        flags: u32,
        qlimit: u32,
    ) -> &'static PfQueuespec {
        let mut q = pf_abi_zeroed::<PfQueuespec>();
        q.qid = qid;
        q.parent_qid = parent_qid;
        q.linkshare.m2.absolute = ls;
        q.flags = flags;
        q.qlimit = qlimit;
        q.set_kif(Some(kif));
        Box::leak(q)
    }

    /// A 100-byte packet for pf queue `qid`.
    fn packet(qid: u32) -> &'static Mbuf {
        let m = test_packet(&[0u8; 100]);
        m.m_pkthdr().pf.qid.set(qid);
        m
    }

    /// An interface with an HFSC tree: root queue 1 (100Mbps), its children 2 (75Mbps) and the
    /// default 3 (25Mbps), both limited to 50 packets.
    fn hfsc_interface() -> (&'static Ifnet, &'static HfscIf) {
        hfsc_initialize();
        let ifp = test_ifnet(b"thfsc0");
        ifq_init(&ifp.if_snd, ifp, 0);
        // SAFETY: the all-zero kif is valid (`PfAbi`).
        let kif: &'static PfiKif = unsafe { zeroed_static() };
        kif.set_pfik_ifp(Some(ifp));

        let disc = (PFQ_HFSC_OPS.pfq_alloc)(ifp);
        // Without link-share bandwidth a root queue is refused.
        assert_eq!(
            (PFQ_HFSC_OPS.pfq_addqueue)(disc, queue(kif, 1, 0, 0, 0, 0)),
            Err(Errno::EINVAL)
        );
        for q in [
            queue(kif, 1, 0, 100_000_000, 0, 0),
            queue(kif, 2, 1, 75_000_000, 0, 0),
            queue(kif, 3, 1, 25_000_000, PFQS_DEFAULT, 0),
        ] {
            assert_eq!((PFQ_HFSC_OPS.pfq_addqueue)(disc, q), Ok(()));
        }
        // A queue id is used once.
        assert_eq!(
            (PFQ_HFSC_OPS.pfq_addqueue)(disc, queue(kif, 2, 1, 1_000_000, 0, 0)),
            Err(Errno::EBUSY)
        );
        // An unknown parent.
        assert_eq!(
            (PFQ_HFSC_OPS.pfq_addqueue)(disc, queue(kif, 4, 9, 1_000_000, 0, 0)),
            Err(Errno::EINVAL)
        );

        ifq_attach(&ifp.if_snd, IFQ_HFSC_OPS, disc);
        assert!(hfsc_enabled(&ifp.if_snd));
        // SAFETY: the softc `hfsc_pf_alloc` made, attached to the queue.
        (ifp, unsafe { hif_of(disc) })
    }

    #[test]
    fn link_share_divides_the_link_by_the_curves() {
        let _g = setup();
        let (ifp, hif) = hfsc_interface();
        let ifq = &ifp.if_snd;

        let root = hif.hif_rootclass.get().expect("root");
        assert_eq!(root.cl_handle.get(), HFSC_ROOT_CLASS | 1);
        let a = hfsc_clh2cph(hif, 2).expect("class 2");
        let b = hfsc_clh2cph(hif, 3).expect("class 3");
        assert!(hif.hif_defaultclass.get().is_some_and(|d| ptr::eq(d, b)));
        // Queue 1 is a class below the hidden root class pf's root queue creates.
        let q1 = hfsc_clh2cph(hif, 1).expect("class 1");
        assert!(ptr::eq(q1.cl_parent.get().expect("parent"), root));
        assert!(ptr::eq(a.cl_parent.get().expect("parent"), q1));
        assert_eq!(hif.hif_classes.get(), 4);

        for _ in 0..40 {
            assert_eq!(ifq_enqueue(ifq, packet(2)), Ok(()));
            assert_eq!(ifq_enqueue(ifq, packet(3)), Ok(()));
        }
        // An unknown queue id goes to the default class.
        assert_eq!(ifq_enqueue(ifq, packet(77)), Ok(()));
        assert_eq!(ifq_len(ifq), 81);
        assert_eq!(hfsc_class_qlength(b), 41);
        // Both leaves are active, on queue 1's active list; queue 1 is on the hidden root's.
        assert_eq!((a.cl_nactive.get(), b.cl_nactive.get()), (1, 1));
        assert_eq!((q1.cl_nactive.get(), q1.cl_actc.iter().count()), (2, 2));
        assert_eq!(root.cl_actc.iter().count(), 1);

        let (mut na, mut nb) = (0, 0);
        for _ in 0..40 {
            let m = ifq_dequeue(ifq).expect("a packet");
            match m.m_pkthdr().pf.qid.get() {
                2 => na += 1,
                _ => nb += 1,
            }
            m_freem(m);
        }
        // 75% and 25% of the link, to within a packet or two.
        assert!((29..=31).contains(&na), "{na} {nb}");
        assert_eq!(na + nb, 40);
        assert_eq!(a.cl_stats.xmit_cnt.get().packets, na);
        assert_eq!(a.cl_stats.xmit_cnt.get().bytes, 100 * na);

        let mut st = HfscClassStats::default();
        hfsc_getclstats(&mut st, a);
        assert_eq!(st.class_handle, 2);
        assert_eq!(st.qlength, 40 - na as u32);
        assert_eq!(st.qlimit, HFSC_DEFAULT_QLIMIT as u32);
        assert_eq!(st.fsc.m2, sm2m(m2sm(75_000_000)));
        assert_eq!(st.rsc, HfscSc::default());
        assert_eq!(st.total, 100 * na);
        assert_eq!(st.machclk_freq, 1_000_000_000);
        assert_eq!(st.period, 1);

        // Back to priq: the softc goes, the waiting packets move over.
        let left = ifq_len(ifq);
        ifq_attach(ifq, IFQ_PRIQ_OPS, ptr::null_mut());
        assert!(!hfsc_enabled(ifq));
        assert_eq!(ifq_len(ifq), left);
        while let Some(m) = ifq_dequeue(ifq) {
            m_freem(m);
        }
    }

    #[test]
    fn a_full_class_drops_and_counts() {
        let _g = setup();
        let (ifp, hif) = hfsc_interface();
        let ifq = &ifp.if_snd;
        let a = hfsc_clh2cph(hif, 2).expect("class 2");

        let limit = HFSC_DEFAULT_QLIMIT as usize;
        for _ in 0..limit {
            assert_eq!(ifq_enqueue(ifq, packet(2)), Ok(()));
        }
        assert_eq!(ifq_enqueue(ifq, packet(2)), Err(Errno::ENOBUFS));
        assert_eq!(
            a.cl_stats.drop_cnt.get(),
            HfscPktcntr {
                packets: 1,
                bytes: 100
            }
        );
        assert_eq!(ifq.ifq_qdrops.get(), 1);

        // Purging (as ifq_purge does) makes the class passive again.
        assert_eq!(crate::net::ifq::ifq_purge(ifq), limit as u32);
        assert_eq!(hfsc_class_qlength(a), 0);
        assert!(hif.hif_rootclass.get().expect("root").cl_actc.is_empty());

        ifq_attach(ifq, IFQ_PRIQ_OPS, ptr::null_mut());
    }
}
/* </TESTS> */
