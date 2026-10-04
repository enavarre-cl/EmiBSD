/* $OpenBSD: fq_codel.h,v 1.4 2020/06/18 23:29:59 dlg Exp $ */
/* $OpenBSD: fq_codel.c,v 1.21 2026/08/13 09:01:49 bket Exp $ */
/* <LICENSES> */
/*
 * Copyright (c) 2017 Mike Belopuhov
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

//! Flow queue CoDel: `<net/fq_codel.h>` and `net/fq_codel.c`, the queueing discipline behind
//! pf's `queue ... flows` rules (`pf.conf(5)`).
//!
//! Upstream: sys/net/fq_codel.h @ 3ce1f3f79392
//! Upstream: sys/net/fq_codel.c @ 3ce1f3f79392
//!
//! CoDel: K. Nichols, V. Jacobson, A. McGregor and J. Iyengar, Controlled Delay Active Queue
//! Management, RFC 8289, January 2018; based on the algorithm by Kathleen Nichols and Van
//! Jacobson with improvements from Dave Taht and Eric Dumazet. FQ-CoDel: T.
//! Hoeiland-Joergensen, P. McKenney, D. Taht, J. Gettys and E. Dumazet, The Flow Queue CoDel
//! Packet Scheduler and Active Queue Management Algorithm, RFC 8290, January 2018; based on
//! the implementation by Rasool Al-Saadi, Centre for Advanced Internet Architectures,
//! Swinburne University of Technology, Melbourne, Australia.
//!
//! Packets are hashed by their flow id onto `nflows` flows, each a CoDel queue. New flows are
//! served first, then old ones, round robin by a byte deficit of one quantum (an MTU unless
//! configured); CoDel drops from a flow's head while its packets have been queued longer
//! than the target for a whole interval, at intervals that shrink with the square root of the
//! drop count (`codel_intervals`, a table of `100ms / sqrt(n)`: integers, no floating point).
//! When the whole queue is over its limit, up to half (at most 64) of the packets of the
//! flow with the largest backlog are dropped at once.
//!
//! pf uses it in two ways: as the conditioner of a send queue (a root `flows` queue: pf
//! attaches [`IFQ_FQCODEL_OPS`] with the state its [`PFQ_FQCODEL_OPS`] built), or as the
//! queue manager of an HFSC leaf class (`net/hfsc.rs`, through [`PFQ_FQCODEL_OPS`]).
//!
//! ## Deviations
//! - The state is `malloc`ed and reached from an opaque pointer, as in C; its members are
//!   `Cell`s. The bit-fields keep their widths: `backlog` (31 bits, unsigned) and `deficit`
//!   (31 bits, signed) are stored truncated, as the C compiler stores them; `dropping` and
//!   `active` (one bit) are `bool`s.
//! - `codel_next_packet` returns the packet with the drop decision instead of filling
//!   `int *drop`; `codel_dequeue` takes the two drop counters as `&mut u64`, which its caller
//!   stores back into `drop_cnt`. `first_flow`/`next_flow` keep the current queue in an
//!   `Option` (`struct flowq **`). `codel_commit` returns `Option` (NULL is `None`).
//! - `classify_flow` returns `None` while the flow table is not allocated (the C's `flow ==
//!   NULL` check, which the C can only reach with `nflows` 0).
//! - `FQCODEL_DEBUG` is not defined, as in C: `DPRINTF` and the debug-only `flow.id` member
//!   are left out (comments at the sites).
//! - The `pfq_ops` functions take the state as `*mut c_void`, the types of the `PfqOps`
//!   table (`net/pfvar.rs`). They are private and reached only through
//!   [`PFQ_FQCODEL_OPS`], whose contract is that the pointer came from its `pfq_alloc`.
//! - `fqcodel_pf_addqueue` returns `EINVAL` for a queue whose kif has no interface when the
//!   quantum must come from the interface's MTU (the C dereferences it; `pf_create_queues`
//!   never adds such a queue).
//! - `malloc(M_WAITOK)` can fail here (it does not sleep yet, `kern/kern_malloc.rs`): a
//!   failed allocation panics, as `priq_alloc` does.

use core::cell::Cell;
use core::cmp::{max, min};
use core::ffi::c_void;
use core::ptr::{self, NonNull};
use core::sync::atomic::Ordering;

use crate::kassert;
use crate::kern::kern_malloc::{free, malloc, mallocarray};
use crate::kern::kern_tc::nsecuptime;
use crate::kern::subr_prf::panic;
use crate::kern::uipc_mbuf::{MAX_LINKHDR, ml_dequeue, ml_enlist, ml_enqueue, ml_init, ml_purge};
use crate::machine::copy::{AbiPod, copyout_obj};
use crate::net::if_var::Ifnet;
use crate::net::ifq::{IfqOps, Ifqueue, ifq_len, ifq_mfreeml, ifq_q_enter, ifq_q_leave};
use crate::net::pfvar::{PfQueuespec, PfqOps};
use crate::sys::errno::Errno;
use crate::sys::malloc::{M_DEVBUF, M_WAITOK, M_ZERO};
use crate::sys::mbuf::{M_FLOWID, Mbuf, MbufList, mbuf_list_first, ml_empty, ml_len};
use crate::sys::queue::{SimpleqEntry, SimpleqHead};

/// `struct fqcodel_pktcntr`: compatible with `hfsc_pktcntr`.
#[repr(C)]
#[derive(Clone, Copy, Default, Debug, PartialEq, Eq)]
pub struct FqcodelPktcntr {
    /// `packets`.
    pub packets: u64,
    /// `bytes`.
    pub bytes: u64,
}

// SAFETY: `#[repr(C)]`, two `u64`s: no padding, every bit pattern valid.
unsafe impl AbiPod for FqcodelPktcntr {}

/// `struct fqcodel_stats`: what `DIOCGETQSTATS` copies out for a flow queue (`pfctl -vsq`).
#[repr(C)]
#[derive(Clone, Copy, Default, Debug)]
pub struct FqcodelStats {
    /// `xmit_cnt`.
    pub xmit_cnt: FqcodelPktcntr,
    /// `drop_cnt`.
    pub drop_cnt: FqcodelPktcntr,

    /// `qlength`.
    pub qlength: u32,
    /// `qlimit`.
    pub qlimit: u32,

    /// `flows`.
    pub flows: u32,
    /// `_unused`: padding.
    pub _unused: u32,

    /// `target`.
    pub target: u32,
    /// `interval`.
    pub interval: u32,

    // the values below are used to calculate standard deviation
    /// `delaysum`: sum of delays, us.
    pub delaysum: u64,
    /// `delaysumsq`: sum of squared delays, us.
    pub delaysumsq: u64,
}

// SAFETY: `#[repr(C)]` integers and `AbiPod` counters, the padding a named member: no hole,
// every bit pattern valid.
unsafe impl AbiPod for FqcodelStats {}

/// The bits of the `backlog:31` bit-field.
const CODEL_BACKLOG_MASK: u32 = 0x7fff_ffff;

/// `struct codel`.
struct Codel {
    /// `q`.
    q: MbufList,

    /// `dropping:1`: dropping state.
    dropping: Cell<bool>,
    /// `backlog:31`: number of bytes in the queue.
    backlog: Cell<u32>,

    /// `drops`: free running counter of drops.
    drops: Cell<u16>,
    /// `ldrops`: value from the previous run.
    ldrops: Cell<u16>,

    /// `start`: the moment queue was above target.
    start: Cell<i64>,
    /// `next`: next interval.
    next: Cell<i64>,
    /// `delay`: delay incurred by the last packet.
    delay: Cell<i64>,
}

impl Codel {
    /// An empty CoDel queue (the zeroed C structure).
    const fn new() -> Self {
        Self {
            q: MbufList::new(),
            dropping: Cell::new(false),
            backlog: Cell::new(0),
            drops: Cell::new(0),
            ldrops: Cell::new(0),
            start: Cell::new(0),
            next: Cell::new(0),
            delay: Cell::new(0),
        }
    }

    /// `cd->backlog = v`, truncated to the bit-field.
    fn set_backlog(&self, v: u32) {
        self.backlog.set(v & CODEL_BACKLOG_MASK);
    }
}

/// `nitems(codel_intervals)`.
const NINTERVALS: usize = 399;

/// `struct codel_params`.
struct CodelParams {
    /// `target`.
    target: Cell<i64>,
    /// `interval`.
    interval: Cell<i64>,
    /// `grace`.
    grace: Cell<i64>,
    /// `quantum`.
    quantum: Cell<i32>,

    /// `intervals`: `codel_intervals` or a scaled copy of it; `None` once freed.
    intervals: Cell<Option<&'static [u32; NINTERVALS]>>,
}

impl CodelParams {
    /// Zeroed parameters.
    const fn new() -> Self {
        Self {
            target: Cell::new(0),
            interval: Cell::new(0),
            grace: Cell::new(0),
            quantum: Cell::new(0),
            intervals: Cell::new(None),
        }
    }
}

/// The bits of the `deficit:31` bit-field.
const FLOW_DEFICIT_BITS: u32 = 31;

/// `struct flow`.
struct Flow {
    /// `cd`.
    cd: Codel,
    /// `active:1`.
    active: Cell<bool>,
    /// `deficit:31`.
    deficit: Cell<i32>,
    // FQCODEL_DEBUG: `uint16_t id`; not defined.
    /// `flowentry`.
    flowentry: SimpleqEntry<Flow>,
}

impl Flow {
    /// An idle flow (the zeroed C structure).
    const fn new() -> Self {
        Self {
            cd: Codel::new(),
            active: Cell::new(false),
            deficit: Cell::new(0),
            flowentry: SimpleqEntry::new(),
        }
    }

    /// `flow->deficit = v`, truncated to the signed bit-field.
    fn set_deficit(&self, v: i32) {
        let shift = 32 - FLOW_DEFICIT_BITS;
        self.deficit.set((v << shift) >> shift);
    }
}

crate::queue_adapter!(
    /// `SIMPLEQ_HEAD(flowq, flow)`.
    Flowq: Flow, flowentry => SimpleqEntry<Flow>
);

/// `FQCF_FIXED_QUANTUM`.
const FQCF_FIXED_QUANTUM: u32 = 0x1;

/// `struct fqcodel`. Protected by: the send queue's `ifq_mtx` once attached (or the HFSC
/// softc's, as a class's queue manager).
struct Fqcodel {
    /// `newq`.
    newq: SimpleqHead<Flowq>,
    /// `oldq`.
    oldq: SimpleqHead<Flowq>,

    /// `flows`: `nflows` flows.
    flows: Cell<*const Flow>,
    /// `qlength`.
    qlength: Cell<u32>,

    /// `ifp`.
    ifp: Cell<Option<&'static Ifnet>>,

    /// `cparams`.
    cparams: CodelParams,

    /// `nflows`.
    nflows: Cell<u32>,
    /// `qlimit`.
    qlimit: Cell<u32>,
    /// `quantum`.
    quantum: Cell<i32>,

    /// `flags`: `FQCF_*`.
    flags: Cell<u32>,

    // stats
    /// `xmit_cnt`.
    xmit_cnt: Cell<FqcodelPktcntr>,
    /// `drop_cnt`.
    drop_cnt: Cell<FqcodelPktcntr>,

    /// `pending_drops`.
    pending_drops: MbufList,
}

impl Fqcodel {
    /// The flows (`fqc->flows[0 .. nflows]`), empty before `fqcodel_pf_addqueue`.
    fn flows(&self) -> &[Flow] {
        let flows = self.flows.get();
        if flows.is_null() {
            return &[];
        }
        // SAFETY: `flows` is a `mallocarray` of `nflows` initialised flows
        // (`fqcodel_pf_addqueue`), freed only with the state (`fqcodel_pf_free`).
        unsafe { core::slice::from_raw_parts(flows, self.nflows.get() as usize) }
    }
}

//
// ifqueue glue.
//

/// `fqcodel_ops`.
static FQCODEL_OPS: IfqOps = IfqOps {
    ifqop_idx: fqcodel_idx,
    ifqop_enq: fqcodel_if_enq,
    ifqop_deq_begin: fqcodel_if_deq_begin,
    ifqop_deq_commit: fqcodel_if_deq_commit,
    ifqop_purge: fqcodel_if_purge,
    ifqop_alloc: fqcodel_alloc,
    ifqop_free: fqcodel_free,
};

/// `ifq_fqcodel_ops`: the FQ-CoDel conditioner, for `ifq_attach`.
pub static IFQ_FQCODEL_OPS: &IfqOps = &FQCODEL_OPS;

//
// pf queue glue.
//

/// `fqcodel_pf_ops`.
static FQCODEL_PF_OPS: PfqOps = PfqOps {
    pfq_alloc: fqcodel_pf_alloc,
    pfq_addqueue: fqcodel_pf_addqueue,
    pfq_free: fqcodel_pf_free,
    pfq_qstats: fqcodel_pf_qstats,
    pfq_qlength: fqcodel_pf_qlength,
    pfq_enqueue: fqcodel_pf_enqueue,
    pfq_deq_begin: fqcodel_pf_deq_begin,
    pfq_deq_commit: fqcodel_pf_deq_commit,
    pfq_purge: fqcodel_pf_purge,
};

/// `pfq_fqcodel_ops`: FQ-CoDel as pf configures it.
pub static PFQ_FQCODEL_OPS: &PfqOps = &FQCODEL_PF_OPS;

/// `fqcodel_qlimit`: default aggregate queue depth.
const FQCODEL_QLIMIT: u32 = 1024;

//
// CoDel implementation
//

/// `codel_target`: delay target, 5ms.
const CODEL_TARGET: i64 = 5_000_000;

/// `codel_intervals`: the first 399 "100 / sqrt(x)" intervals, ns precision.
static CODEL_INTERVALS: [u32; NINTERVALS] = [
    100000000, 70710678, 57735027, 50000000, 44721360, 40824829, 37796447, 35355339, 33333333,
    31622777, 30151134, 28867513, 27735010, 26726124, 25819889, 25000000, 24253563, 23570226,
    22941573, 22360680, 21821789, 21320072, 20851441, 20412415, 20000000, 19611614, 19245009,
    18898224, 18569534, 18257419, 17960530, 17677670, 17407766, 17149859, 16903085, 16666667,
    16439899, 16222142, 16012815, 15811388, 15617376, 15430335, 15249857, 15075567, 14907120,
    14744196, 14586499, 14433757, 14285714, 14142136, 14002801, 13867505, 13736056, 13608276,
    13483997, 13363062, 13245324, 13130643, 13018891, 12909944, 12803688, 12700013, 12598816,
    12500000, 12403473, 12309149, 12216944, 12126781, 12038585, 11952286, 11867817, 11785113,
    11704115, 11624764, 11547005, 11470787, 11396058, 11322770, 11250879, 11180340, 11111111,
    11043153, 10976426, 10910895, 10846523, 10783277, 10721125, 10660036, 10599979, 10540926,
    10482848, 10425721, 10369517, 10314212, 10259784, 10206207, 10153462, 10101525, 10050378,
    10000000, 9950372, 9901475, 9853293, 9805807, 9759001, 9712859, 9667365, 9622504, 9578263,
    9534626, 9491580, 9449112, 9407209, 9365858, 9325048, 9284767, 9245003, 9205746, 9166985,
    9128709, 9090909, 9053575, 9016696, 8980265, 8944272, 8908708, 8873565, 8838835, 8804509,
    8770580, 8737041, 8703883, 8671100, 8638684, 8606630, 8574929, 8543577, 8512565, 8481889,
    8451543, 8421519, 8391814, 8362420, 8333333, 8304548, 8276059, 8247861, 8219949, 8192319,
    8164966, 8137885, 8111071, 8084521, 8058230, 8032193, 8006408, 7980869, 7955573, 7930516,
    7905694, 7881104, 7856742, 7832604, 7808688, 7784989, 7761505, 7738232, 7715167, 7692308,
    7669650, 7647191, 7624929, 7602859, 7580980, 7559289, 7537784, 7516460, 7495317, 7474351,
    7453560, 7432941, 7412493, 7392213, 7372098, 7352146, 7332356, 7312724, 7293250, 7273930,
    7254763, 7235746, 7216878, 7198158, 7179582, 7161149, 7142857, 7124705, 7106691, 7088812,
    7071068, 7053456, 7035975, 7018624, 7001400, 6984303, 6967330, 6950480, 6933752, 6917145,
    6900656, 6884284, 6868028, 6851887, 6835859, 6819943, 6804138, 6788442, 6772855, 6757374,
    6741999, 6726728, 6711561, 6696495, 6681531, 6666667, 6651901, 6637233, 6622662, 6608186,
    6593805, 6579517, 6565322, 6551218, 6537205, 6523281, 6509446, 6495698, 6482037, 6468462,
    6454972, 6441566, 6428243, 6415003, 6401844, 6388766, 6375767, 6362848, 6350006, 6337243,
    6324555, 6311944, 6299408, 6286946, 6274558, 6262243, 6250000, 6237829, 6225728, 6213698,
    6201737, 6189845, 6178021, 6166264, 6154575, 6142951, 6131393, 6119901, 6108472, 6097108,
    6085806, 6074567, 6063391, 6052275, 6041221, 6030227, 6019293, 6008418, 5997601, 5986843,
    5976143, 5965500, 5954913, 5944383, 5933908, 5923489, 5913124, 5902813, 5892557, 5882353,
    5872202, 5862104, 5852057, 5842062, 5832118, 5822225, 5812382, 5802589, 5792844, 5783149,
    5773503, 5763904, 5754353, 5744850, 5735393, 5725983, 5716620, 5707301, 5698029, 5688801,
    5679618, 5670480, 5661385, 5652334, 5643326, 5634362, 5625440, 5616560, 5607722, 5598925,
    5590170, 5581456, 5572782, 5564149, 5555556, 5547002, 5538488, 5530013, 5521576, 5513178,
    5504819, 5496497, 5488213, 5479966, 5471757, 5463584, 5455447, 5447347, 5439283, 5431254,
    5423261, 5415304, 5407381, 5399492, 5391639, 5383819, 5376033, 5368281, 5360563, 5352877,
    5345225, 5337605, 5330018, 5322463, 5314940, 5307449, 5299989, 5292561, 5285164, 5277798,
    5270463, 5263158, 5255883, 5248639, 5241424, 5234239, 5227084, 5219958, 5212860, 5205792,
    5198752, 5191741, 5184758, 5177804, 5170877, 5163978, 5157106, 5150262, 5143445, 5136655,
    5129892, 5123155, 5116445, 5109761, 5103104, 5096472, 5089866, 5083286, 5076731, 5070201,
    5063697, 5057217, 5050763, 5044333, 5037927, 5031546, 5025189, 5018856, 5012547, 5006262,
];

/// The state behind an opaque discipline pointer.
///
/// # Safety
///
/// `p` came from `fqcodel_pf_alloc` and `fqcodel_pf_free` has not run on it.
unsafe fn fqc_of(p: *mut c_void) -> &'static Fqcodel {
    // SAFETY: the caller's contract; the state stays allocated until `fqcodel_pf_free`.
    unsafe { &*p.cast::<Fqcodel>() }
}

/// `codel_initparams`: sets the target, the interval and the table of intervals; an interval
/// longer than 100ms scales the table up.
fn codel_initparams(cp: &CodelParams, target: u32, interval: u32, quantum: i32) {
    // Update observation intervals table according to the configured initial interval
    // value.
    if interval > CODEL_INTERVALS[0] {
        // Select either specified target or 5% of an interval (RFC 8289, section 4.3).
        cp.target.set(i64::from(max(target, interval / 20)));
        cp.interval.set(i64::from(interval));

        // The coefficient is scaled up by a 1000
        let mult = (cp.interval.get() as u64 * 1000) / u64::from(CODEL_INTERVALS[0]);

        // Prepare table of intervals
        let Some(mem) = mallocarray(NINTERVALS, size_of::<u32>(), M_DEVBUF, M_WAITOK | M_ZERO)
        else {
            panic(format_args!("codel_initparams: out of memory"));
        };
        let tbl = mem.as_ptr().cast::<[u32; NINTERVALS]>();
        let mut scaled = [0u32; NINTERVALS];
        for (s, &i) in scaled.iter_mut().zip(CODEL_INTERVALS.iter()) {
            *s = ((u64::from(i) * mult) / 1000) as u32;
        }
        // SAFETY: a fresh allocation of `NINTERVALS` `u32`s, aligned (malloc's chunks are
        // aligned to their size), written once here and read-only until
        // `codel_freeparams`.
        cp.intervals.set(Some(unsafe {
            tbl.write(scaled);
            &*tbl
        }));
    } else {
        cp.target.set(max(i64::from(target), CODEL_TARGET));
        cp.interval.set(i64::from(CODEL_INTERVALS[0]));
        cp.intervals.set(Some(&CODEL_INTERVALS));
    }

    cp.quantum.set(quantum);

    // Grace period for delta reuse (RFC 8289, section 5.5)
    cp.grace.set(16 * cp.interval.get());
}

/// `codel_freeparams`.
fn codel_freeparams(cp: &CodelParams) {
    if let Some(tbl) = cp.intervals.get()
        && !ptr::eq(tbl, &CODEL_INTERVALS)
    {
        free(
            NonNull::from(tbl).cast(),
            M_DEVBUF,
            size_of::<[u32; NINTERVALS]>(),
        );
    }
    cp.intervals.set(None);
}

/// `codel_backlog`.
fn codel_backlog(cd: &Codel) -> u32 {
    cd.backlog.get()
}

/// `codel_qlength`.
fn codel_qlength(cd: &Codel) -> u32 {
    ml_len(&cd.q)
}

/// `codel_delay`.
fn codel_delay(cd: &Codel) -> i64 {
    cd.delay.get()
}

/// `codel_enqueue`: stamps `m` with `now` and queues it.
fn codel_enqueue(cd: &Codel, now: i64, m: &'static Mbuf) {
    m.m_pkthdr().ph_timestamp.set(now);

    ml_enqueue(&cd.q, m);
    cd.set_backlog(cd.backlog.get().wrapping_add(m.m_pkthdr().len.get() as u32));
}

/// `control_law`: selects the next interval according to the number of drops in the
/// current one, relative to the timestamp `rts`.
fn control_law(cd: &Codel, cp: &CodelParams, rts: i64) {
    kassert!(cd.drops.get() > 0);
    let idx = min(usize::from(cd.drops.get().wrapping_sub(1)), NINTERVALS - 1);
    let interval = cp.intervals.get().map_or(0, |tbl| tbl[idx]);
    cd.next.set(rts.wrapping_add(i64::from(interval)));
}

/// `codel_next_packet`: picks the next enqueued packet and determines the queueing delay as
/// well as whether or not it's a good candidate for dropping from the queue.
///
/// The decision is made based on the queueing delay target of 5ms and on the current queue
/// length in bytes, which shouldn't be less than the amount of data that arrives in a
/// typical interarrival time (MTU-sized packets arriving spaced by the amount of time it
/// takes to send such a packet on the bottleneck).
fn codel_next_packet(cd: &Codel, cp: &CodelParams, now: i64) -> (Option<&'static Mbuf>, bool) {
    let Some(m) = mbuf_list_first(&cd.q) else {
        kassert!(cd.backlog.get() == 0);
        // Empty queue, reset interval
        cd.start.set(0);
        return (None, false);
    };

    let len = m.m_pkthdr().len.get();
    kassert!(cd.backlog.get() as i32 >= len);
    if now.wrapping_sub(m.m_pkthdr().ph_timestamp.get()) < cp.target.get()
        || (cd.backlog.get() as i32).wrapping_sub(len) <= cp.quantum.get()
    {
        // The minimum delay decreased below the target, reset the current observation
        // interval.
        cd.start.set(0);
        return (Some(m), false);
    }

    let mut drop = false;
    if cd.start.get() == 0 {
        // This is the first packet to be delayed for more than the target, start the first
        // observation interval after a single RTT and see if the minimum delay goes below
        // the target within the interval, otherwise punish the next packet.
        cd.start.set(now.wrapping_add(cp.interval.get()));
    } else if now >= cd.start.get() {
        drop = true;
    }
    (Some(m), drop)
}

/// The states of `codel_dequeue`.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
enum CodelState {
    /// `INITIAL`.
    Initial,
    /// `ACCEPTING`.
    Accepting,
    /// `FIRSTDROP`.
    Firstdrop,
    /// `DROPPING`.
    Dropping,
    /// `CONTROL`.
    Control,
    /// `RECOVERY`.
    Recovery,
}

/// `codel_state_change`.
fn codel_state_change(
    cd: &Codel,
    now: i64,
    m: Option<&Mbuf>,
    drop: bool,
    state: CodelState,
) -> CodelState {
    if state == CodelState::Firstdrop {
        return CodelState::Accepting;
    }

    if cd.dropping.get() {
        if !drop {
            return CodelState::Recovery;
        } else if now >= cd.next.get() {
            return if state == CodelState::Dropping {
                CodelState::Control
            } else {
                CodelState::Dropping
            };
        }
    } else if drop {
        return CodelState::Firstdrop;
    }

    if m.is_none() {
        return CodelState::Recovery;
    }

    CodelState::Accepting
}

/// `codel_dequeue`: the next packet to send, after dropping (onto `free_ml`, counted in
/// `dpkts`/`dbytes`) the head packets the control law condemns.
fn codel_dequeue(
    cd: &Codel,
    cp: &CodelParams,
    now: i64,
    free_ml: &MbufList,
    dpkts: &mut u64,
    dbytes: &mut u64,
) -> Option<&'static Mbuf> {
    let mut m;
    let mut done = false;

    let mut state = CodelState::Initial;

    loop {
        let (next, drop) = codel_next_packet(cd, cp, now);
        m = next;
        state = codel_state_change(cd, now, m, drop, state);

        match state {
            CodelState::Firstdrop => {
                if let Some(d) = codel_commit(cd, m) {
                    ml_enqueue(free_ml, d);

                    *dpkts = dpkts.wrapping_add(1);
                    *dbytes = dbytes.wrapping_add(d.m_pkthdr().len.get() as u64);
                }

                cd.dropping.set(true);

                // If we're still within the grace period and not meeting our minimal delay
                // target we treat this as a continuation of the previous observation
                // interval and shrink it further. Otherwise we start from the initial one.
                let delta = cd.drops.get().wrapping_sub(cd.ldrops.get());
                if delta > 1
                    && (now < cd.next.get() || now.wrapping_sub(cd.next.get()) < cp.grace.get())
                {
                    cd.drops.set(delta);
                } else {
                    cd.drops.set(1);
                }
                control_law(cd, cp, now);
                cd.ldrops.set(cd.drops.get());

                // fetches the next packet and goes to ACCEPTING
            }
            CodelState::Dropping => {
                if let Some(d) = codel_commit(cd, m) {
                    ml_enqueue(free_ml, d);
                    cd.drops.set(cd.drops.get().wrapping_add(1));

                    *dpkts = dpkts.wrapping_add(1);
                    *dbytes = dbytes.wrapping_add(d.m_pkthdr().len.get() as u64);
                }

                // fetches the next packet and goes to CONTROL
            }
            CodelState::Control if drop => {
                control_law(cd, cp, cd.next.get());
                continue;
            }
            CodelState::Control | CodelState::Recovery => {
                cd.dropping.set(false);
                done = true;
            }
            CodelState::Accepting => {
                done = true;
            }
            CodelState::Initial => {}
        }

        if done {
            break;
        }
    }

    if let Some(m) = m {
        cd.delay
            .set(now.wrapping_sub(m.m_pkthdr().ph_timestamp.get()));
    }

    m
}

/// `codel_commit`: takes the head packet off the queue (`m`, when given, is that packet).
fn codel_commit(cd: &Codel, m: Option<&Mbuf>) -> Option<&'static Mbuf> {
    let n = ml_dequeue(&cd.q);
    if let Some(m) = m {
        kassert!(n.is_some_and(|n| ptr::eq(n, m)));
        let _ = m;
    }
    kassert!(n.is_some());
    let n = n?;
    let len = n.m_pkthdr().len.get() as u32;
    kassert!(cd.backlog.get() >= len);
    cd.set_backlog(cd.backlog.get().wrapping_sub(len));
    Some(n)
}

/// `codel_purge`: moves the queue's packets to `ml`.
fn codel_purge(cd: &Codel, ml: &MbufList) {
    ml_enlist(ml, &cd.q);
    cd.set_backlog(0);
}

//
// FQ-CoDel implementation
//

/// `classify_flow`: the flow of `m`, by its flow id.
fn classify_flow(fqc: &'static Fqcodel, m: &Mbuf) -> Option<&'static Flow> {
    let mut index = 0;

    if m.m_pkthdr().csum_flags.get() & M_FLOWID != 0 {
        index = u32::from(m.m_pkthdr().ph_flowid.get()).checked_rem(fqc.nflows.get())?;
    }

    // FQCODEL_DEBUG: DPRINTF("%s: %u\n", __func__, index); not defined.

    fqc.flows().get(index as usize)
}

/// `fqcodel_enq`: queues `m` on its flow; returns a packet to drop (one per call, the rest
/// wait on `pending_drops`) when the queue is over its limit.
fn fqcodel_enq(fqc: &'static Fqcodel, m: &'static Mbuf) -> Option<&'static Mbuf> {
    let mut backlog = 0;

    let Some(mut flow) = classify_flow(fqc, m) else {
        return Some(m);
    };

    let now = nsecuptime() as i64;
    codel_enqueue(&flow.cd, now, m);
    fqc.qlength.set(fqc.qlength.get().wrapping_add(1));

    if !flow.active.get() {
        // SAFETY: an inactive flow is on neither flow queue (`next_flow` takes it off before
        // clearing `active`); the flows live as long as the state.
        unsafe { fqc.newq.insert_tail(flow) };
        flow.set_deficit(fqc.quantum.get());
        flow.active.set(true);
        // FQCODEL_DEBUG: DPRINTF("flow %u active deficit %d"); not defined.
    }

    // Flush pending_drops first to let PF account them individually. When batch dropping
    // (below), we queue multiple packets but can only return one per enqueue call to
    // maintain the interface contract.
    if !ml_empty(&fqc.pending_drops) {
        return ml_dequeue(&fqc.pending_drops);
    }

    // If total queue length exceeds the limit, find the flow with the largest backlog and
    // drop up to half of its packets, with a maximum of 64, from the head. Implements RFC
    // 8290, section 4.1 batch drop to handle overload efficiently. Dropped packets are
    // queued in pending_drops.
    if fqc.qlength.get() > fqc.qlimit.get() {
        for f in fqc.flows() {
            if codel_backlog(&f.cd) > backlog {
                flow = f;
                backlog = codel_backlog(&flow.cd);
            }
        }

        let ndrop = (codel_qlength(&flow.cd) / 2).clamp(1, 64);

        for _ in 0..ndrop {
            let Some(d) = codel_commit(&flow.cd, None) else {
                break;
            };
            let mut dc = fqc.drop_cnt.get();
            dc.packets = dc.packets.wrapping_add(1);
            dc.bytes = dc.bytes.wrapping_add(d.m_pkthdr().len.get() as u64);
            fqc.drop_cnt.set(dc);
            fqc.qlength.set(fqc.qlength.get().wrapping_sub(1));
            ml_enqueue(&fqc.pending_drops, d);
        }

        // FQCODEL_DEBUG: DPRINTF("batch-dropped %d/%d pkts from flow %u"); not defined.

        return ml_dequeue(&fqc.pending_drops);
    }

    None
}

/// `select_queue`: the new flows if any, else the old ones.
fn select_queue(fqc: &'static Fqcodel) -> Option<&'static SimpleqHead<Flowq>> {
    if !fqc.newq.is_empty() {
        Some(&fqc.newq)
    } else if !fqc.oldq.is_empty() {
        Some(&fqc.oldq)
    } else {
        None
    }
}

/// `first_flow`: the first flow with a positive deficit; flows out of deficit get a quantum
/// more and move to the end of the old flows. `fq` is set to the queue it was found on.
fn first_flow(
    fqc: &'static Fqcodel,
    fq: &mut Option<&'static SimpleqHead<Flowq>>,
) -> Option<&'static Flow> {
    loop {
        *fq = select_queue(fqc);
        let q = (*fq)?;
        while let Some(flow) = q.first() {
            if flow.deficit.get() <= 0 {
                flow.set_deficit(flow.deficit.get().wrapping_add(fqc.quantum.get()));
                // SAFETY: `flow` is the head of `q`; it moves to the tail of the old flows,
                // and the flows live as long as the state.
                unsafe {
                    q.remove_head();
                    fqc.oldq.insert_tail(flow);
                }
                // FQCODEL_DEBUG: DPRINTF("flow %u deficit %d"); not defined.
            } else {
                return Some(flow);
            }
        }
    }
}

/// `next_flow`: takes `flow` off the head of its queue (to the old flows, or inactive) and
/// returns the next flow to serve.
fn next_flow(
    fqc: &'static Fqcodel,
    flow: &'static Flow,
    fq: &mut Option<&'static SimpleqHead<Flowq>>,
) -> Option<&'static Flow> {
    if let Some(q) = *fq {
        // SAFETY: `flow` is the head of `q` (`first_flow` returned it from there).
        unsafe { q.remove_head() };

        if ptr::eq(q, &fqc.newq) && !fqc.oldq.is_empty() {
            // A packet was dropped, starve the queue
            // SAFETY: `flow` was just taken off the new flows.
            unsafe { fqc.oldq.insert_tail(flow) };
            // FQCODEL_DEBUG: DPRINTF("flow %u ->oldq deficit %d"); not defined.
        } else {
            // A packet was dropped on a starved queue, disable it
            flow.active.set(false);
            // FQCODEL_DEBUG: DPRINTF("flow %u inactive deficit %d"); not defined.
        }
    }

    first_flow(fqc, fq)
}

/// `fqcodel_deq_begin`: the next packet, from the first flow with a deficit that CoDel lets
/// send; packets CoDel drops go to `free_ml`. The cookie is the flow.
fn fqcodel_deq_begin(
    fqc: &'static Fqcodel,
    cookiep: &mut *mut c_void,
    free_ml: &MbufList,
) -> Option<&'static Mbuf> {
    let ml = MbufList::new();
    let mut fq = None;

    if fqc.flags.get() & FQCF_FIXED_QUANTUM == 0
        && let Some(ifp) = fqc.ifp.get()
    {
        fqc.quantum
            .set((ifp.if_mtu.get() as i32).wrapping_add(MAX_LINKHDR.load(Ordering::Relaxed)));
    }

    let now = nsecuptime() as i64;

    let mut flow = first_flow(fqc, &mut fq);
    while let Some(f) = flow {
        let mut dc = fqc.drop_cnt.get();
        let m = codel_dequeue(
            &f.cd,
            &fqc.cparams,
            now,
            &ml,
            &mut dc.packets,
            &mut dc.bytes,
        );
        fqc.drop_cnt.set(dc);

        kassert!(fqc.qlength.get() >= ml_len(&ml));
        fqc.qlength.set(fqc.qlength.get().wrapping_sub(ml_len(&ml)));

        ml_enlist(free_ml, &ml);

        if let Some(m) = m {
            f.set_deficit(f.deficit.get().wrapping_sub(m.m_pkthdr().len.get()));
            // FQCODEL_DEBUG: DPRINTF("flow %u deficit %d"); not defined.
            *cookiep = ptr::from_ref(f).cast_mut().cast();
            return Some(m);
        }

        flow = next_flow(fqc, f, &mut fq);
    }

    None
}

/// `fqcodel_deq_commit`: takes the packet off its flow and counts it.
fn fqcodel_deq_commit(fqc: &'static Fqcodel, m: &'static Mbuf, cookie: *mut c_void) {
    // SAFETY: the cookie is a flow of this state (`fqcodel_deq_begin`), alive with it.
    let flow: &Flow = unsafe { &*cookie.cast::<Flow>() };

    kassert!(fqc.qlength.get() > 0);
    fqc.qlength.set(fqc.qlength.get().wrapping_sub(1));

    let mut xc = fqc.xmit_cnt.get();
    xc.packets = xc.packets.wrapping_add(1);
    xc.bytes = xc.bytes.wrapping_add(m.m_pkthdr().len.get() as u64);
    fqc.xmit_cnt.set(xc);

    let _ = codel_commit(&flow.cd, Some(m));
}

/// `fqcodel_purge`: moves every packet, the pending drops included, to `ml`.
fn fqcodel_purge(fqc: &'static Fqcodel, ml: &MbufList) {
    for flow in fqc.flows() {
        codel_purge(&flow.cd, ml);
    }
    ml_enlist(ml, &fqc.pending_drops);
    fqc.qlength.set(0);
}

/// `fqcodel_if_enq`.
///
/// # Safety
///
/// As for [`crate::net::ifq::IfqopEnqFn`].
unsafe fn fqcodel_if_enq(ifq: &Ifqueue, m: &'static Mbuf) -> Option<&'static Mbuf> {
    // SAFETY: the caller's contract: `ifq_q` is the FQ-CoDel state `ifq_attach` installed.
    fqcodel_enq(unsafe { fqc_of(ifq.ifq_q.get()) }, m)
}

/// `fqcodel_if_deq_begin`: frees what CoDel dropped once the queue's mutex is released.
///
/// # Safety
///
/// As for [`crate::net::ifq::IfqopDeqBeginFn`].
unsafe fn fqcodel_if_deq_begin(ifq: &Ifqueue) -> Option<(&'static Mbuf, *mut c_void)> {
    let free_ml = MbufList::new();
    let mut cookie = ptr::null_mut();

    // SAFETY: the caller's contract: `ifq_q` is the FQ-CoDel state `ifq_attach` installed.
    let m = fqcodel_deq_begin(unsafe { fqc_of(ifq.ifq_q.get()) }, &mut cookie, &free_ml);
    ifq_mfreeml(ifq, &free_ml);
    m.map(|m| (m, cookie))
}

/// `fqcodel_if_deq_commit`.
///
/// # Safety
///
/// As for [`crate::net::ifq::IfqopDeqCommitFn`].
unsafe fn fqcodel_if_deq_commit(ifq: &Ifqueue, m: &'static Mbuf, cookie: *mut c_void) {
    // SAFETY: the caller's contract: `ifq_q` is the FQ-CoDel state `ifq_attach` installed.
    fqcodel_deq_commit(unsafe { fqc_of(ifq.ifq_q.get()) }, m, cookie);
}

/// `fqcodel_if_purge`.
///
/// # Safety
///
/// As for [`crate::net::ifq::IfqopPurgeFn`].
unsafe fn fqcodel_if_purge(ifq: &Ifqueue, ml: &MbufList) {
    // SAFETY: the caller's contract: `ifq_q` is the FQ-CoDel state `ifq_attach` installed.
    fqcodel_purge(unsafe { fqc_of(ifq.ifq_q.get()) }, ml);
}

/// `fqcodel_pf_alloc`: a new, empty state.
fn fqcodel_pf_alloc(_ifp: &'static Ifnet) -> *mut c_void {
    let Some(mem) = malloc(size_of::<Fqcodel>(), M_DEVBUF, M_WAITOK | M_ZERO) else {
        panic(format_args!("fqcodel_pf_alloc: out of memory"));
    };
    let fqc = mem.as_ptr().cast::<Fqcodel>();
    // SAFETY: a fresh allocation of `size_of::<Fqcodel>()` bytes, aligned (malloc's chunks
    // are aligned to their size), written once before use.
    unsafe {
        fqc.write(Fqcodel {
            newq: SimpleqHead::new(),
            oldq: SimpleqHead::new(),
            flows: Cell::new(ptr::null()),
            qlength: Cell::new(0),
            ifp: Cell::new(None),
            cparams: CodelParams::new(),
            nflows: Cell::new(0),
            qlimit: Cell::new(0),
            quantum: Cell::new(0),
            flags: Cell::new(0),
            xmit_cnt: Cell::new(FqcodelPktcntr::default()),
            drop_cnt: Cell::new(FqcodelPktcntr::default()),
            pending_drops: MbufList::new(),
        })
    };
    // SAFETY: just written; it lives until `fqcodel_pf_free`.
    let r = unsafe { &*fqc };

    r.newq.init();
    r.oldq.init();
    ml_init(&r.pending_drops);

    fqc.cast()
}

/// `fqcodel_pf_addqueue`: configures the state from queue `qs` and allocates its flows.
fn fqcodel_pf_addqueue(arg: *mut c_void, qs: &'static PfQueuespec) -> Result<(), Errno> {
    let ifp = qs.kif().and_then(|kif| kif.pfik_ifp());
    // SAFETY: the `pfq_ops` contract: `arg` came from `fqcodel_pf_alloc`.
    let fqc = unsafe { fqc_of(arg) };

    if qs.flowqueue.flows == 0 || qs.flowqueue.flows > 0xffff {
        return Err(Errno::EINVAL);
    }

    fqc.nflows.set(qs.flowqueue.flows);
    fqc.quantum.set(qs.flowqueue.quantum as i32);
    if qs.qlimit > 0 {
        fqc.qlimit.set(qs.qlimit);
    } else {
        fqc.qlimit.set(FQCODEL_QLIMIT);
    }
    if fqc.quantum.get() > 0 {
        fqc.flags.set(fqc.flags.get() | FQCF_FIXED_QUANTUM);
    } else {
        // pf_create_queues adds only the queues of attached interfaces.
        let Some(ifp) = ifp else {
            return Err(Errno::EINVAL);
        };
        fqc.quantum
            .set((ifp.if_mtu.get() as i32).wrapping_add(MAX_LINKHDR.load(Ordering::Relaxed)));
    }

    codel_initparams(
        &fqc.cparams,
        qs.flowqueue.target,
        qs.flowqueue.interval,
        fqc.quantum.get(),
    );

    let nflows = fqc.nflows.get() as usize;
    let Some(mem) = mallocarray(nflows, size_of::<Flow>(), M_DEVBUF, M_WAITOK | M_ZERO) else {
        panic(format_args!("fqcodel_pf_addqueue: out of memory"));
    };
    let flows = mem.as_ptr().cast::<Flow>();
    for i in 0..nflows {
        // SAFETY: a fresh allocation of `nflows` flows, aligned (malloc's chunks are aligned
        // to their size); each is written once before the table is published.
        unsafe { flows.add(i).write(Flow::new()) };
    }
    fqc.flows.set(flows.cast_const());

    // FQCODEL_DEBUG: number the flows (flows[i].id = i); not defined.

    fqc.ifp.set(ifp);

    // FQCODEL_DEBUG: DPRINTF("fq-codel on %s: %d queues %d deep, quantum %d target %llums
    // interval %llums"); not defined.

    Ok(())
}

/// `fqcodel_pf_free`: drops the pending packets and frees the state.
fn fqcodel_pf_free(arg: *mut c_void) {
    // SAFETY: the `pfq_ops` contract: `arg` came from `fqcodel_pf_alloc` and is no longer
    // used.
    let fqc = unsafe { fqc_of(arg) };

    let _ = ml_purge(&fqc.pending_drops);
    codel_freeparams(&fqc.cparams);
    if let Some(flows) = NonNull::new(fqc.flows.get().cast_mut().cast::<u8>()) {
        free(
            flows,
            M_DEVBUF,
            fqc.nflows.get() as usize * size_of::<Flow>(),
        );
    }
    if let Some(p) = NonNull::new(arg.cast::<u8>()) {
        free(p, M_DEVBUF, size_of::<Fqcodel>());
    }
}

/// `fqcodel_pf_qstats`: copies the statistics of the interface's flow queue out to the user
/// buffer `ubuf` of `*nbytes` bytes, and sets `*nbytes` to their size.
fn fqcodel_pf_qstats(qs: &'static PfQueuespec, ubuf: usize, nbytes: &mut i32) -> Result<(), Errno> {
    let Some(ifp) = qs.kif().and_then(|kif| kif.pfik_ifp()) else {
        return Err(Errno::EBADF);
    };

    if (*nbytes as usize) < size_of::<FqcodelStats>() {
        return Err(Errno::EINVAL);
    }

    let mut stats = FqcodelStats::default();

    // XXX: multi-q?
    let Some(qp) = ifq_q_enter(&ifp.if_snd, IFQ_FQCODEL_OPS) else {
        return Err(Errno::EBADF);
    };
    // SAFETY: `ifq_q_enter` returned the state of the FQ-CoDel conditioner, under `ifq_mtx`.
    let fqc = unsafe { fqc_of(qp.as_ptr()) };

    stats.xmit_cnt = fqc.xmit_cnt.get();
    stats.drop_cnt = fqc.drop_cnt.get();

    stats.qlength = ifq_len(&ifp.if_snd);
    stats.qlimit = fqc.qlimit.get();

    stats.flows = 0;
    stats.delaysum = 0;
    stats.delaysumsq = 0;

    for flow in fqc.flows() {
        if codel_qlength(&flow.cd) == 0 {
            continue;
        }
        // Scale down to microseconds to avoid overflows
        let delay = codel_delay(&flow.cd) / 1000;
        stats.delaysum = stats.delaysum.wrapping_add(delay as u64);
        stats.delaysumsq = stats
            .delaysumsq
            .wrapping_add(delay.wrapping_mul(delay) as u64);
        stats.flows += 1;
    }

    ifq_q_leave(&ifp.if_snd, qp);

    copyout_obj(&stats, ubuf)?;

    *nbytes = size_of::<FqcodelStats>() as i32;
    Ok(())
}

/// `fqcodel_pf_qlength`.
fn fqcodel_pf_qlength(fqc: *mut c_void) -> u32 {
    // SAFETY: the `pfq_ops` contract: the pointer came from `fqcodel_pf_alloc`.
    unsafe { fqc_of(fqc) }.qlength.get()
}

/// `fqcodel_pf_enqueue`.
fn fqcodel_pf_enqueue(fqc: *mut c_void, m: &'static Mbuf) -> Option<&'static Mbuf> {
    // SAFETY: the `pfq_ops` contract: the pointer came from `fqcodel_pf_alloc`.
    fqcodel_enq(unsafe { fqc_of(fqc) }, m)
}

/// `fqcodel_pf_deq_begin`.
fn fqcodel_pf_deq_begin(
    fqc: *mut c_void,
    cookiep: &mut *mut c_void,
    free_ml: &MbufList,
) -> Option<&'static Mbuf> {
    // SAFETY: the `pfq_ops` contract: the pointer came from `fqcodel_pf_alloc`.
    fqcodel_deq_begin(unsafe { fqc_of(fqc) }, cookiep, free_ml)
}

/// `fqcodel_pf_deq_commit`.
fn fqcodel_pf_deq_commit(fqc: *mut c_void, m: &'static Mbuf, cookie: *mut c_void) {
    // SAFETY: the `pfq_ops` contract: the pointer came from `fqcodel_pf_alloc`.
    fqcodel_deq_commit(unsafe { fqc_of(fqc) }, m, cookie);
}

/// `fqcodel_pf_purge`.
fn fqcodel_pf_purge(fqc: *mut c_void, ml: &MbufList) {
    // SAFETY: the `pfq_ops` contract: the pointer came from `fqcodel_pf_alloc`.
    fqcodel_purge(unsafe { fqc_of(fqc) }, ml);
}

/// `fqcodel_idx`: one send queue.
fn fqcodel_idx(_nqueues: u32, _m: &Mbuf) -> u32 {
    0
}

/// `fqcodel_alloc`: allocation is done in `fqcodel_pf_alloc`.
fn fqcodel_alloc(_idx: u32, arg: *mut c_void) -> *mut c_void {
    arg
}

/// `fqcodel_free`.
///
/// # Safety
///
/// `arg` came from `fqcodel_pf_alloc` and is no longer used.
unsafe fn fqcodel_free(_idx: u32, arg: *mut c_void) {
    fqcodel_pf_free(arg);
}

const _: () = {
    assert!(size_of::<FqcodelPktcntr>() == 16);
    assert!(size_of::<FqcodelStats>() == 72);
    assert!(core::mem::offset_of!(FqcodelStats, delaysum) == 56);
};

#[cfg(test)]
mod tests;
