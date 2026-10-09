/* <LICENSES> */

/*
 * Copyright (c) 2001 Daniel Hartmeier
 * Copyright (c) 2002 - 2013 Henning Brauer <henning@openbsd.org>
 * All rights reserved.
 *
 * Redistribution and use in source and binary forms, with or without
 * modification, are permitted provided that the following conditions
 * are met:
 *
 *    - Redistributions of source code must retain the above copyright
 *      notice, this list of conditions and the following disclaimer.
 *    - Redistributions in binary form must reproduce the above
 *      copyright notice, this list of conditions and the following
 *      disclaimer in the documentation and/or other materials provided
 *      with the distribution.
 *
 * THIS SOFTWARE IS PROVIDED BY THE COPYRIGHT HOLDERS AND CONTRIBUTORS
 * "AS IS" AND ANY EXPRESS OR IMPLIED WARRANTIES, INCLUDING, BUT NOT
 * LIMITED TO, THE IMPLIED WARRANTIES OF MERCHANTABILITY AND FITNESS
 * FOR A PARTICULAR PURPOSE ARE DISCLAIMED. IN NO EVENT SHALL THE
 * COPYRIGHT HOLDERS OR CONTRIBUTORS BE LIABLE FOR ANY DIRECT, INDIRECT,
 * INCIDENTAL, SPECIAL, EXEMPLARY, OR CONSEQUENTIAL DAMAGES (INCLUDING,
 * BUT NOT LIMITED TO, PROCUREMENT OF SUBSTITUTE GOODS OR SERVICES;
 * LOSS OF USE, DATA, OR PROFITS; OR BUSINESS INTERRUPTION) HOWEVER
 * CAUSED AND ON ANY THEORY OF LIABILITY, WHETHER IN CONTRACT, STRICT
 * LIABILITY, OR TORT (INCLUDING NEGLIGENCE OR OTHERWISE) ARISING IN
 * ANY WAY OUT OF THE USE OF THIS SOFTWARE, EVEN IF ADVISED OF THE
 * POSSIBILITY OF SUCH DAMAGE.
 *
 * Effort sponsored in part by the Defense Advanced Research Projects
 * Agency (DARPA) and Air Force Research Laboratory, Air Force
 * Materiel Command, USAF, under agreement number F30602-01-2-0537.
 *
 */
/* </LICENSES> */

/* <CODE> */
//! `pf(4)`: the packet filter. Rule evaluation, the state table, address translation and the
//! hooks the IP stack calls (`pf_test`).
//!
//! Upstream: sys/net/pf.c @ 3ce1f3f79392
//!
//! The objects (rules, states, state keys, source nodes, limiters) follow
//! `net/pfvar.rs`/`net/pfvar_priv.rs`: `&'static` pool items with `Cell` members. The packet
//! descriptor's pointers into the packet are `PfLoc`s; the functions the C gives a
//! `struct pf_addr *` or a `u_int16_t *` that may point into the packet take a `PfLoc` and
//! read and write through the descriptor.
//!
//! ## Deviations
//! - `pf_print_host` formats through [`PfHost`] (`Display`), which host tests use.
//! - `pf_translate_icmp_af`'s `void *arg` (a `struct icmp` or `struct icmp6_hdr`) is a
//!   [`PfLoc`], as the descriptor's other pointers into headers.
//! - `carp(4)` (`NCARP`) is not configured: `carp_lsdrop` is a comment at its site. `pfsync(4)`
//!   (`net/if_pfsync.rs`) and `pflow(4)` (`net/if_pflow.rs`) are.
//! - `pf_anchor_stack` and `pf_status_fcounters` are per-CPU (`cpumem`) in the C; here they
//!   are one static array each: the counters are atomics, and the anchor stack is used only
//!   by `pf_match_rule`, which `pf_test_rule` runs with `pf_lock` held exclusively, so one
//!   CPU at a time evaluates rules.
//! - `pf_counters_inc` adds to the statistics of the interface, the rules, the state, its
//!   source nodes and tables from every softnet thread at once, holding only the shared net
//!   lock: the C's plain `++`, which may lose counts. They stay `Cell`s here (an open
//!   deviation of the MP audit, M11e): making them atomics is a change of every counter's
//!   type, and losing a count is what the C accepts.
//! - `pf_test`'s `struct mbuf **m0` is `&mut Option<&'static Mbuf>`, as `ip_input_if` passes
//!   its packet; the action is returned as `u8` (`PF_PASS`, `PF_DROP`, ...).
//! - The `STATE_INC_COUNTERS`, `BOUND_IFACE` and `REASON_SET` macros are functions.
//! - `tcp_mssdflt` (netinet's `TCP_MSSDFLT`, an atomic) is read through the function
//!   `tcp_mssdflt()`, which `pf_syncookies.rs` shares.

use core::cell::Cell;
use core::cmp::Ordering;
use core::ffi::c_void;
use core::mem::size_of;
use core::ptr;
use core::sync::atomic::{AtomicU64, Ordering as AtomicOrdering};

use crate::crypto::sha2::{SHA512Final, SHA512Init, SHA512Update, Sha2Ctx};
use crate::dev::rnd::{arc4random, arc4random_buf, arc4random_uniform};
use crate::kassert;
use crate::kern::kern_lock::{mtx_enter, mtx_init, mtx_leave};
use crate::kern::kern_rwlock::{
    rw_assert_wrlock, rw_enter_read, rw_enter_write, rw_exit_read, rw_exit_write,
};
use crate::kern::kern_smr::{smr_read_enter, smr_read_leave};
use crate::kern::kern_synch::{refcnt_init, refcnt_rele, refcnt_take};
use crate::kern::kern_tc::{gettime, getuptime};
use crate::kern::subr_prf::{addlog, log, panic};
use crate::machine::intr::{IPL_NET, IPL_SOFTNET};
use crate::net::if_::{IFF_LOOPBACK, if_get, if_get_smr, if_put, unhandled_af};
use crate::net::if_var::Ifnet;
use crate::net::pf_if::{pfi_all, pfi_kif_ref, pfi_kif_unref};
use crate::net::pf_table::{pfr_insert_kentry, pfr_remove_kentry};
use crate::net::pfvar::*;
use crate::net::pfvar_priv::*;
use crate::net::rtable::rtable_l2;
use crate::netinet::in_::{IPPROTO_ICMP, IPPROTO_ICMPV6, IPPROTO_TCP, IPPROTO_UDP};
use crate::netinet::in_pcb::Inpcb;
use crate::netinet::tcp::{
    MAX_TCPOPTLEN, TCP_MAX_WINSHIFT, TCP_MAXWIN, TCPOLEN_MAXSEG, TCPOLEN_SACK, TCPOLEN_WINDOW,
    TCPOPT_EOL, TCPOPT_MAXSEG, TCPOPT_NOP, TCPOPT_SACK, TCPOPT_SACK_PERMITTED, TCPOPT_WINDOW,
    TH_ACK, TH_CWR, TH_ECE, TH_FIN, TH_PUSH, TH_RST, TH_SYN, TH_URG, Tcphdr,
};
use crate::netinet::tcp_fsm::{
    TCPS_CLOSED, TCPS_CLOSING, TCPS_ESTABLISHED, TCPS_FIN_WAIT_2, TCPS_SYN_SENT, TCPS_TIME_WAIT,
    tcps_haveestablished,
};
use crate::netinet::tcp_seq::{seq_geq, seq_gt, seq_leq};
use crate::netinet::tcp_var::{TcpstatCounters, tcpstat_inc};
use crate::netinet::udp::Udphdr;
use crate::sys::errno::Errno;
use crate::sys::malloc::M_NOWAIT;
use crate::sys::mbuf::{
    M_COPYALL, M_DONTWAIT, M_FLOWID, M_TCP_CSUM_IN_BAD, M_TCP_CSUM_IN_OK, M_TCP_CSUM_OUT,
    MT_HEADER, MTag, Mbuf, PACKET_TAG_PF_DIVERT, PF_TAG_DIVERTED, PF_TAG_DIVERTED_PACKET,
    PF_TAG_GENERATED, PF_TAG_PROCESSED, PF_TAG_REFRAGMENTED, PF_TAG_REROUTE,
    PF_TAG_TRANSLATE_LOCALHOST,
};
use crate::sys::pool::{PR_NOWAIT, Pool};
use crate::sys::queue::{SlistHead, TailqHead};
use crate::sys::socket::{AF_INET, AF_INET6};
use crate::sys::syslog::{LOG_DEBUG, LOG_ERR, LOG_NOTICE};
use crate::sys::tree::{RbHead, RbtHead};
use crate::sys::types::{Gid, SaFamily, Time, Uid};

#[cfg(feature = "inet6")]
use crate::netinet::ip6::Ip6Hdr;

/// `enum pf_test_status`.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum PfTestStatus {
    /// `PF_TEST_FAIL`.
    Fail = -1,
    /// `PF_TEST_OK`.
    Ok = 0,
    /// `PF_TEST_QUICK`.
    Quick = 1,
}

/// `struct pf_test_ctx`: the state of one rule evaluation. The C's `rm`, `am` and `rsm`
/// out-pointers to `pf_test_rule`'s caller are the members of the same names, copied out at
/// the end.
pub struct PfTestCtx<'p> {
    /// `pd`.
    pub pd: &'p mut PfPdesc,
    /// `act`.
    pub act: PfRuleActions,
    /// `icmpcode`.
    pub icmpcode: u8,
    /// `icmptype`.
    pub icmptype: u8,
    /// `icmp_dir`.
    pub icmp_dir: i32,
    /// `state_icmp`.
    pub state_icmp: i32,
    /// `tag`.
    pub tag: i32,
    /// `limiter_drop`.
    pub limiter_drop: i32,
    /// `reason`.
    pub reason: u16,
    /// `ri`.
    pub ri: Option<&'static PfRuleItem>,
    /// `sns[PF_SN_MAX]`.
    pub sns: [Option<&'static PfSrcNode>; PF_SN_MAX],
    /// `rules`.
    pub rules: SlistHead<PfRuleSlist>,
    /// `nr`.
    pub nr: Option<&'static PfRule>,
    /// `*rm`.
    pub rm: Option<&'static PfRule>,
    /// `a`.
    pub a: Option<&'static PfRule>,
    /// `*am`.
    pub am: Option<&'static PfRule>,
    /// `*rsm`.
    pub rsm: Option<&'static PfRuleset>,
    /// `arsm`.
    pub arsm: Option<&'static PfRuleset>,
    /// `aruleset`.
    pub aruleset: Option<&'static PfRuleset>,
    /// `statelim`.
    pub statelim: Option<&'static PfStatelim>,
    /// `sourcelim`.
    pub sourcelim: Option<&'static PfSourcelim>,
    /// `source`.
    pub source: Option<&'static PfSource>,
}

/// `pf_statetbl`.
pub static PF_STATETBL: PfGlobal<RbtHead<PfStateTree>> = PfGlobal(RbtHead::new());
/// `pf_queues[2]`.
pub static PF_QUEUES: PfGlobal<[TailqHead<PfQueuehead>; 2]> =
    PfGlobal([TailqHead::new(), TailqHead::new()]);
/// `pf_queues_active`: an index into `PF_QUEUES`.
pub static PF_QUEUES_ACTIVE: PfGlobal<Cell<usize>> = PfGlobal(Cell::new(0));
/// `pf_queues_inactive`: an index into `PF_QUEUES`.
pub static PF_QUEUES_INACTIVE: PfGlobal<Cell<usize>> = PfGlobal(Cell::new(1));

crate::queue_adapter!(
    /// `TAILQ_HEAD(pf_queuehead, pf_queuespec)`.
    pub PfQueuehead: PfQueuespec, entries => crate::sys::queue::TailqEntry<PfQueuespec>
);

/// `pf_queues_active`.
pub fn pf_queues_active() -> &'static TailqHead<PfQueuehead> {
    &PF_QUEUES[PF_QUEUES_ACTIVE.get()]
}

/// `pf_queues_inactive`.
pub fn pf_queues_inactive() -> &'static TailqHead<PfQueuehead> {
    &PF_QUEUES[PF_QUEUES_INACTIVE.get()]
}

/// `pf_status`.
pub static PF_STATUS: PfStatus = PfStatus::zeroed();

/// `pf_status_fcounters`: per-CPU in the C.
static PF_STATUS_FCOUNTERS: [AtomicU64; FCNT_MAX] =
    [AtomicU64::new(0), AtomicU64::new(0), AtomicU64::new(0)];

/// `pf_inp_mtx`: links pf to inpcbs.
pub static PF_INP_MTX: crate::sys::mutex::Mutex = crate::sys::mutex::Mutex::new(IPL_SOFTNET);

/// `pf_hdr_limit`: arbitrary limit, tune in ddb.
pub static PF_HDR_LIMIT: core::sync::atomic::AtomicI32 = core::sync::atomic::AtomicI32::new(20);

/// `pf_tcp_secret_ctx`, `pf_tcp_secret`, `pf_tcp_secret_init`, `pf_tcp_iss_off`.
struct PfTcpSecret {
    ctx: Cell<Option<Sha2Ctx>>,
    secret: Cell<[u8; 16]>,
    init: Cell<bool>,
    iss_off: Cell<u32>,
}

static PF_TCP_SECRET: PfGlobal<PfTcpSecret> = PfGlobal(PfTcpSecret {
    ctx: Cell::new(None),
    secret: Cell::new([0; 16]),
    init: Cell::new(false),
    iss_off: Cell::new(0),
});

/// `pf_src_tree_pl`.
pub static PF_SRC_TREE_PL: Pool = Pool::new();
/// `pf_rule_pl`.
pub static PF_RULE_PL: Pool = Pool::new();
/// `pf_queue_pl`.
pub static PF_QUEUE_PL: Pool = Pool::new();
/// `pf_state_pl`.
pub static PF_STATE_PL: Pool = Pool::new();
/// `pf_state_key_pl`.
pub static PF_STATE_KEY_PL: Pool = Pool::new();
/// `pf_state_item_pl`.
pub static PF_STATE_ITEM_PL: Pool = Pool::new();
/// `pf_rule_item_pl`.
pub static PF_RULE_ITEM_PL: Pool = Pool::new();
/// `pf_sn_item_pl`.
pub static PF_SN_ITEM_PL: Pool = Pool::new();
/// `pf_pktdelay_pl`.
pub static PF_PKTDELAY_PL: Pool = Pool::new();
/// `pf_statelim_pl`.
pub static PF_STATELIM_PL: Pool = Pool::new();
/// `pf_sourcelim_pl`.
pub static PF_SOURCELIM_PL: Pool = Pool::new();
/// `pf_source_pl`.
pub static PF_SOURCE_PL: Pool = Pool::new();
/// `pf_state_link_pl`.
pub static PF_STATE_LINK_PL: Pool = Pool::new();

// SAFETY: all of these are integers, Cells, Options of references, raw words, queue and tree
// links and heads, refcounts, mutexes (initialised with `mtx_init` after `pool_get`, as in C)
// and timeouts (set with `timeout_set` before use): the all-zero pattern is valid.
unsafe impl PfPoolItem for PfSrcNode {}
// SAFETY: as above.
unsafe impl PfPoolItem for PfRule {}
// SAFETY: as above.
unsafe impl PfPoolItem for PfQueuespec {}
// SAFETY: as above.
unsafe impl PfPoolItem for PfState {}
// SAFETY: as above.
unsafe impl PfPoolItem for PfStateKey {}
// SAFETY: as above.
unsafe impl PfPoolItem for PfStateItem {}
// SAFETY: as above.
unsafe impl PfPoolItem for PfRuleItem {}
// SAFETY: as above.
unsafe impl PfPoolItem for PfSnItem {}
// SAFETY: as above.
unsafe impl PfPoolItem for PfPktdelay {}
// SAFETY: as above.
unsafe impl PfPoolItem for PfStatelim {}
// SAFETY: as above.
unsafe impl PfPoolItem for PfSourcelim {}
// SAFETY: as above.
unsafe impl PfPoolItem for PfSource {}
// SAFETY: as above.
unsafe impl PfPoolItem for PfStateLink {}

/// `pf_pool_limits[PF_LIMIT_MAX]`.
pub static PF_POOL_LIMITS: [PfPoolLimit; PF_LIMIT_MAX] = [
    PfPoolLimit::new(&PF_STATE_PL, PFSTATE_HIWAT),
    PfPoolLimit::new(&PF_SRC_TREE_PL, PFSNODE_HIWAT),
    PfPoolLimit::new(&crate::net::pf_norm::PF_FRENT_PL, pffrag_frent_hiwat()),
    PfPoolLimit::new(&crate::net::pf_table::PFR_KTABLE_PL, PFR_KTABLE_HIWAT),
    PfPoolLimit::new(
        &crate::net::pf_table::PFR_KENTRY_PL[PFRKE_PLAIN as usize],
        PFR_KENTRY_HIWAT,
    ),
    PfPoolLimit::new(&PF_PKTDELAY_PL, PF_PKTDELAY_MAXPKTS),
    PfPoolLimit::new(&crate::net::pf_ruleset::PF_ANCHOR_PL, PF_ANCHOR_HIWAT),
];

/// `BOUND_IFACE(r, k)`: the state's interface: the packet's for if-bound rules, else all.
fn bound_iface(r: &PfRule, k: Option<&'static PfiKif>) -> Option<&'static PfiKif> {
    if r.rule_flag.get() & PFRULE_IFBOUND != 0 {
        k
    } else {
        pfi_all()
    }
}

/// `STATE_INC_COUNTERS(s)`.
fn state_inc_counters(s: &'static PfState) {
    if let Some(r) = s.rule.ptr() {
        r.states_cur.set(r.states_cur.get().wrapping_add(1));
        r.states_tot.set(r.states_tot.get().wrapping_add(1));
    }
    if let Some(a) = s.anchor.ptr() {
        a.states_cur.set(a.states_cur.get().wrapping_add(1));
        a.states_tot.set(a.states_tot.get().wrapping_add(1));
    }
    for mrm in s.match_rules.iter() {
        mrm.r()
            .states_cur
            .set(mrm.r().states_cur.get().wrapping_add(1));
    }
}

/// `pf_status.debug >= level`.
pub fn pf_debug(level: i32) -> bool {
    PF_STATUS.debug.get() >= level as u32
}

/// `x++` on a `Cell` counter.
pub fn pf_inc<T: Copy + core::ops::Add<Output = T> + From<u8>>(c: &Cell<T>) {
    c.set(c.get() + T::from(1u8));
}

/// `pf_statelim_id_cmp`.
pub fn pf_statelim_id_cmp(a: &PfStatelim, b: &PfStatelim) -> Ordering {
    a.pfstlim_id.get().cmp(&b.pfstlim_id.get())
}

/// `pf_statelim_nm_cmp`: `strncmp` over the whole name.
pub fn pf_statelim_nm_cmp(a: &PfStatelim, b: &PfStatelim) -> Ordering {
    let (an, bn) = (a.pfstlim_nm.get(), b.pfstlim_nm.get());
    pf_cstr(&an).cmp(pf_cstr(&bn))
}

/// `pf_statelim_id_tree_active`.
pub static PF_STATELIM_ID_TREE_ACTIVE: PfGlobal<RbtHead<PfStatelimIdTree>> =
    PfGlobal(RbtHead::new());
/// `pf_statelim_list_active`.
pub static PF_STATELIM_LIST_ACTIVE: PfGlobal<TailqHead<PfStatelimList>> =
    PfGlobal(TailqHead::new());
/// `pf_statelim_id_tree_inactive`.
pub static PF_STATELIM_ID_TREE_INACTIVE: PfGlobal<RbtHead<PfStatelimIdTree>> =
    PfGlobal(RbtHead::new());
/// `pf_statelim_nm_tree_inactive`.
pub static PF_STATELIM_NM_TREE_INACTIVE: PfGlobal<RbtHead<PfStatelimNmTree>> =
    PfGlobal(RbtHead::new());
/// `pf_statelim_list_inactive`.
pub static PF_STATELIM_LIST_INACTIVE: PfGlobal<TailqHead<PfStatelimList>> =
    PfGlobal(TailqHead::new());

/// `pf_sourcelim_id_cmp`.
pub fn pf_sourcelim_id_cmp(a: &PfSourcelim, b: &PfSourcelim) -> Ordering {
    a.pfsrlim_id.get().cmp(&b.pfsrlim_id.get())
}

/// `pf_sourcelim_nm_cmp`.
pub fn pf_sourcelim_nm_cmp(a: &PfSourcelim, b: &PfSourcelim) -> Ordering {
    let (an, bn) = (a.pfsrlim_nm.get(), b.pfsrlim_nm.get());
    pf_cstr(&an).cmp(pf_cstr(&bn))
}

/// `pf_source_cmp`.
pub fn pf_source_cmp(a: &PfSource, b: &PfSource) -> Ordering {
    a.pfsr_af
        .get()
        .cmp(&b.pfsr_af.get())
        .then(a.pfsr_rdomain.get().cmp(&b.pfsr_rdomain.get()))
        .then_with(|| {
            pf_addr_compare(&a.pfsr_addr.get(), &b.pfsr_addr.get(), a.pfsr_af.get()).cmp(&0)
        })
}

/// `pf_source_ioc_cmp`: as `pf_source_cmp`, the addresses compared in host order.
pub fn pf_source_ioc_cmp(a: &PfSource, b: &PfSource) -> Ordering {
    let o = a
        .pfsr_af
        .get()
        .cmp(&b.pfsr_af.get())
        .then(a.pfsr_rdomain.get().cmp(&b.pfsr_rdomain.get()));
    if o != Ordering::Equal {
        return o;
    }
    let (aa, ba) = (a.pfsr_addr.get(), b.pfsr_addr.get());
    for i in 0..4 {
        let o = u32::from_be(aa.addr32(i)).cmp(&u32::from_be(ba.addr32(i)));
        if o != Ordering::Equal {
            return o;
        }
    }
    Ordering::Equal
}

/// `pf_sourcelim_id_tree_active`.
pub static PF_SOURCELIM_ID_TREE_ACTIVE: PfGlobal<RbtHead<PfSourcelimIdTree>> =
    PfGlobal(RbtHead::new());
/// `pf_sourcelim_list_active`.
pub static PF_SOURCELIM_LIST_ACTIVE: PfGlobal<TailqHead<PfSourcelimList>> =
    PfGlobal(TailqHead::new());
/// `pf_sourcelim_id_tree_inactive`.
pub static PF_SOURCELIM_ID_TREE_INACTIVE: PfGlobal<RbtHead<PfSourcelimIdTree>> =
    PfGlobal(RbtHead::new());
/// `pf_sourcelim_nm_tree_inactive`.
pub static PF_SOURCELIM_NM_TREE_INACTIVE: PfGlobal<RbtHead<PfSourcelimNmTree>> =
    PfGlobal(RbtHead::new());
/// `pf_sourcelim_list_inactive`.
pub static PF_SOURCELIM_LIST_INACTIVE: PfGlobal<TailqHead<PfSourcelimList>> =
    PfGlobal(TailqHead::new());

/// `pf_statelim_find`: the active state limiter `id`.
pub fn pf_statelim_find(id: u32) -> Option<&'static PfStatelim> {
    // Only the id is used in cmp, so the key need not be filled in further.
    let key = PfStatelim::key(id);
    PF_STATELIM_ID_TREE_ACTIVE.find(&key)
}

/// `pf_sourcelim_find`: the active source limiter `id`.
pub fn pf_sourcelim_find(id: u32) -> Option<&'static PfSourcelim> {
    let key = PfSourcelim::key(id);
    PF_SOURCELIM_ID_TREE_ACTIVE.find(&key)
}

/// `pf_source_gc`: sources without states, waiting to expire.
pub static PF_SOURCE_GC: PfGlobal<TailqHead<PfSourceList>> = PfGlobal(TailqHead::new());

/// `pf_source_purge`: frees the empty sources whose rate window has passed.
fn pf_source_purge() {
    let now = getuptime();

    for sr in PF_SOURCE_GC.iter() {
        let Some(srlim) = sr.pfsr_parent.get() else {
            continue;
        };

        if now <= sr.pfsr_empty_ts.get() + Time::from(srlim.pfsrlim_rate.seconds.get()) + 1 {
            continue;
        }

        // SAFETY: `sr` is on the gc list, the trees and in its pool until freed here, under
        // pf_lock.
        unsafe {
            PF_SOURCE_GC.remove(sr);
            srlim.pfsrlim_sources.remove(sr);
            srlim.pfsrlim_ioc_sources.remove(sr);
        }
        srlim
            .pfsrlim_nsources
            .set(srlim.pfsrlim_nsources.get().wrapping_sub(1));

        pf_pool_put(&PF_SOURCE_PL, sr);
    }
}

/// `pf_source_pfr_addr`: the table address of a source.
fn pf_source_pfr_addr(sr: &PfSource) -> PfrAddr {
    let mut p = PfrAddr::default();
    let Some(srlim) = sr.pfsr_parent.get() else {
        return p;
    };

    p.pfra_af = sr.pfsr_af.get();
    if sr.pfsr_af.get() == AF_INET {
        p.pfra_net = srlim.pfsrlim_ipv4_prefix.get() as u8;
        p.pfra_u.set_v4(sr.pfsr_addr.get().v4());
    }
    #[cfg(feature = "inet6")]
    if sr.pfsr_af.get() == AF_INET6 {
        p.pfra_net = srlim.pfsrlim_ipv6_prefix.get() as u8;
        p.pfra_u.set_v6(sr.pfsr_addr.get().v6());
    }
    p
}

/// `pf_source_used`: a state now uses the source.
pub fn pf_source_used(sr: &'static PfSource) {
    let Some(srlim) = sr.pfsr_parent.get() else {
        return;
    };

    let used = sr.pfsr_inuse.get();
    sr.pfsr_inuse.set(used + 1);
    sr.pfsr_rate_ts.set(
        sr.pfsr_rate_ts
            .get()
            .wrapping_add(srlim.pfsrlim_rate_token.get()),
    );

    if used == 0 {
        // SAFETY: an unused source is on the gc list (`pf_source_rele`, creation).
        unsafe { PF_SOURCE_GC.remove(sr) };
    } else if let Some(t) = srlim.pfsrlim_overload.table.get()
        && used >= srlim.pfsrlim_overload.hwm.get()
        && sr.pfsr_intable.get() == 0
    {
        let p = pf_source_pfr_addr(sr);

        let _ = pfr_insert_kentry(t, &p, gettime());
        sr.pfsr_intable.set(1);
    }
}

/// `pf_source_rele`: a state no longer uses the source.
pub fn pf_source_rele(sr: &'static PfSource) {
    let Some(srlim) = sr.pfsr_parent.get() else {
        return;
    };

    let used = sr.pfsr_inuse.get().wrapping_sub(1);
    sr.pfsr_inuse.set(used);

    if let Some(t) = srlim.pfsrlim_overload.table.get()
        && sr.pfsr_intable.get() != 0
        && used < srlim.pfsrlim_overload.lwm.get()
    {
        let p = pf_source_pfr_addr(sr);

        let _ = pfr_remove_kentry(t, &p);
        sr.pfsr_intable.set(0);
    }

    if used == 0 {
        // SAFETY: the source is in its pool and on no gc list (it was in use).
        unsafe { PF_SOURCE_GC.insert_tail(sr) };
        sr.pfsr_empty_ts
            .set(getuptime() + Time::from(srlim.pfsrlim_rate.seconds.get()));
    }
}

/// `pf_source_key`: fills `key` with the masked address to look a source up by.
pub fn pf_source_key(
    srlim: &PfSourcelim,
    key: &PfSource,
    af: SaFamily,
    rdomain: u32,
    addr: &PfAddr,
) {
    // Only af+addr is used for lookup.
    key.pfsr_af.set(af);
    key.pfsr_rdomain.set(rdomain as u16);
    let mut a = PfAddr::zeroed();
    match af {
        AF_INET => {
            a.set_addr32(
                0,
                srlim.pfsrlim_ipv4_mask.get().v4().s_addr & addr.v4().s_addr,
            );
            for i in 1..4 {
                a.set_addr32(i, 0u32.to_be());
            }
        }
        #[cfg(feature = "inet6")]
        AF_INET6 => {
            let mask = srlim.pfsrlim_ipv6_mask.get();
            for i in 0..4 {
                a.set_addr32(i, mask.addr32(i) & addr.addr32(i));
            }
        }
        _ => unhandled_af(i32::from(af)),
    }
    key.pfsr_addr.set(a);
}

/// `pf_source_find`.
pub fn pf_source_find(srlim: &'static PfSourcelim, key: &PfSource) -> Option<&'static PfSource> {
    srlim.pfsrlim_sources.find(key)
}

/// `tree_src_tracking`.
pub static TREE_SRC_TRACKING: PfGlobal<RbHead<PfSrcTree>> = PfGlobal(RbHead::new());

/// `tree_id`.
pub static TREE_ID: PfGlobal<RbtHead<PfStateTreeId>> = PfGlobal(RbtHead::new());

/// `pf_state_list`.
pub static PF_STATE_LIST: PfStateList = PfStateList::new();

crate::tree_adapter!(
    /// `RB_HEAD(pf_src_tree, pf_src_node)`.
    pub PfSrcTree: PfSrcNode, entry => crate::sys::tree::RbEntry<PfSrcNode>, pf_src_compare
);

/// `pf_addr_compare`: -1, 0 or 1, the addresses compared word by word as stored.
pub fn pf_addr_compare(a: &PfAddr, b: &PfAddr, af: SaFamily) -> i32 {
    let words: &[usize] = match af {
        AF_INET => &[0],
        #[cfg(feature = "inet6")]
        AF_INET6 => &[3, 2, 1, 0],
        _ => &[],
    };
    for &i in words {
        if a.addr32(i) > b.addr32(i) {
            return 1;
        }
        if a.addr32(i) < b.addr32(i) {
            return -1;
        }
    }
    0
}

/// `pf_src_compare`.
pub fn pf_src_compare(a: &PfSrcNode, b: &PfSrcNode) -> Ordering {
    a.rule
        .word()
        .cmp(&b.rule.word())
        .then(a.type_.get().cmp(&b.type_.get()))
        .then(a.af.get().cmp(&b.af.get()))
        .then_with(|| pf_addr_compare(&a.addr.get(), &b.addr.get(), a.af.get()).cmp(&0))
}

/// `pf_set_protostate`: sets one or both peers' protocol state, keeping the half-open count.
pub fn pf_set_protostate(st: &PfState, which: i32, newstate: u8) {
    if which == PF_PEER_DST || which == PF_PEER_BOTH {
        st.dst.state.set(newstate);
    }
    if which == PF_PEER_DST {
        return;
    }

    if st.src.state.get() == newstate {
        return;
    }
    let srcstate = i32::from(st.src.state.get());
    let ns = i32::from(newstate);
    if st.creatorid.get() == PF_STATUS.hostid.get()
        && st.key[PF_SK_STACK]
            .get()
            .is_some_and(|k| i32::from(k.proto.get()) == IPPROTO_TCP)
        && !(tcps_haveestablished(srcstate) || srcstate == TCPS_CLOSED)
        && (tcps_haveestablished(ns) || ns == TCPS_CLOSED)
    {
        let h = &PF_STATUS.states_halfopen;
        h.set(h.get().wrapping_sub(1));
    }

    st.src.state.set(newstate);
}

/// `pf_addrcpy`: copies the family's part of the address.
pub fn pf_addrcpy(dst: &mut PfAddr, src: &PfAddr, af: SaFamily) {
    match af {
        AF_INET => dst.set_addr32(0, src.addr32(0)),
        #[cfg(feature = "inet6")]
        AF_INET6 => {
            for i in 0..4 {
                dst.set_addr32(i, src.addr32(i));
            }
        }
        _ => unhandled_af(i32::from(af)),
    }
}

/// `pf_init_threshold`.
pub fn pf_init_threshold(threshold: &PfThreshold, limit: u32, seconds: u32) {
    threshold.limit.set(limit.wrapping_mul(PF_THRESHOLD_MULT));
    threshold.seconds.set(seconds);
    threshold.count.set(0);
    threshold.last.set(getuptime() as u32);
}

/// `pf_add_threshold`.
pub fn pf_add_threshold(threshold: &PfThreshold) {
    let t = getuptime() as u32;
    let diff = t.wrapping_sub(threshold.last.get());

    if diff >= threshold.seconds.get() {
        threshold.count.set(0);
    } else {
        let c = threshold.count.get();
        threshold.count.set(c.wrapping_sub(
            (u64::from(c) * u64::from(diff) / u64::from(threshold.seconds.get())) as u32,
        ));
    }
    threshold
        .count
        .set(threshold.count.get().wrapping_add(PF_THRESHOLD_MULT));
    threshold.last.set(t);
}

/// `pf_check_threshold`.
pub fn pf_check_threshold(threshold: &PfThreshold) -> bool {
    threshold.count.get() > threshold.limit.get()
}

/// `pf_state_list_insert`: we can always put states on the end of the list. Readers take
/// the read lock, then the mutex to get the head and tail, release the mutex and iterate
/// between head and tail.
pub fn pf_state_list_insert(pfs: &PfStateList, st: &'static PfState) {
    pf_state_ref(st); // get a ref for the list

    mtx_enter(&pfs.pfs_mtx);
    // SAFETY: a new state, on no list, under pfs_mtx.
    unsafe { pfs.pfs_list.insert_tail(st) };
    mtx_leave(&pfs.pfs_mtx);
}

/// `pf_state_list_remove`: states can only be removed when the write lock is held.
pub fn pf_state_list_remove(pfs: &PfStateList, st: &'static PfState) {
    rw_assert_wrlock(&pfs.pfs_rwl);

    mtx_enter(&pfs.pfs_mtx);
    // SAFETY: the state is on the list (inserted by `pf_state_list_insert`), under both locks.
    unsafe { pfs.pfs_list.remove(st) };
    mtx_leave(&pfs.pfs_mtx);

    pf_state_unref(Some(st)); // list no longer references the state
}

/// `pf_update_state_timeout`.
pub fn pf_update_state_timeout(st: &PfState, to: usize) {
    mtx_enter(&st.mtx);
    if usize::from(st.timeout.get()) != PFTM_UNLINKED {
        st.timeout.set(to as u8);
    }
    mtx_leave(&st.mtx);
}

/// `pf_src_connlimit`: counts an established connection against the source node's limits;
/// `true` when the state was killed for exceeding them.
pub fn pf_src_connlimit(st: &'static PfState) -> bool {
    let mut bad = 0;

    let Some(sn) = pf_get_src_node(st, PF_SN_NONE) else {
        return false;
    };

    // Note: conn limit is bumped on SYN_SENT->ESTABLISHED state transition.
    let sn_conn = sn.conn.get().wrapping_add(1);
    sn.conn.set(sn_conn);

    st.src.tcp_est.set(1);
    pf_add_threshold(&sn.conn_rate);

    let Some(rule) = st.rule.ptr() else {
        return false;
    };
    if rule.max_src_conn != 0 && rule.max_src_conn < sn_conn {
        pf_inc(&PF_STATUS.lcounters[LCNT_SRCCONN]);
        bad += 1;
    }

    if rule.max_src_conn_rate.limit != 0 && pf_check_threshold(&sn.conn_rate) {
        pf_inc(&PF_STATUS.lcounters[LCNT_SRCCONNRATE]);
        bad += 1;
    }

    if bad == 0 {
        return false;
    }

    let wkey = st.key[PF_SK_WIRE].get();
    let waf = wkey.map_or(0, |k| k.af.get());
    if let Some(tbl) = rule.overload_tbl() {
        let mut killed: u32 = 0;

        pf_inc(&PF_STATUS.lcounters[LCNT_OVERLOAD_TABLE]);
        if pf_debug(LOG_NOTICE) {
            log(
                LOG_NOTICE,
                format_args!("pf: pf_src_connlimit: blocking address "),
            );
            pf_print_host(&sn.addr.get(), 0, waf);
        }

        let mut p = PfrAddr {
            pfra_af: waf,
            ..PfrAddr::default()
        };
        if waf == AF_INET {
            p.pfra_net = 32;
            p.pfra_u.set_v4(sn.addr.get().v4());
        }
        #[cfg(feature = "inet6")]
        if waf == AF_INET6 {
            p.pfra_net = 128;
            p.pfra_u.set_v6(sn.addr.get().v6());
        }

        let _ = pfr_insert_kentry(tbl, &p, gettime());

        // Kill existing states if that's required.
        if rule.flush != 0 {
            pf_inc(&PF_STATUS.lcounters[LCNT_OVERLOAD_FLUSH]);
            for st2 in TREE_ID.iter() {
                let Some(sk) = st2.key[PF_SK_WIRE].get() else {
                    continue;
                };
                // Kill states from this source. (Only those from the same rule if
                // PF_FLUSH_GLOBAL is not set.)
                if sk.af.get() == waf
                    && ((st.direction.get() == PF_OUT
                        && pf_aeq(&sn.addr.get(), &sk.addr[1].get(), sk.af.get()))
                        || (st.direction.get() == PF_IN
                            && pf_aeq(&sn.addr.get(), &sk.addr[0].get(), sk.af.get())))
                    && (rule.flush & PF_FLUSH_GLOBAL != 0 || opt_eq(Some(rule), st2.rule.ptr()))
                {
                    pf_update_state_timeout(st2, PFTM_PURGE);
                    pf_set_protostate(st2, PF_PEER_BOTH, TCPS_CLOSED as u8);
                    killed += 1;
                }
            }
            if pf_debug(LOG_NOTICE) {
                addlog(format_args!(", {killed} states killed"));
            }
        }
        if pf_debug(LOG_NOTICE) {
            addlog(format_args!("\n"));
        }
    }

    // Kill this state.
    pf_update_state_timeout(st, PFTM_PURGE);
    pf_set_protostate(st, PF_PEER_BOTH, TCPS_CLOSED as u8);
    true
}

/// `pf_insert_src_node`: finds or creates the source node of `src` for `rule`.
#[allow(clippy::too_many_arguments)]
pub fn pf_insert_src_node(
    sn: &mut Option<&'static PfSrcNode>,
    rule: &'static PfRule,
    type_: PfSnTypes,
    af: SaFamily,
    src: &PfAddr,
    raddr: Option<&PfAddr>,
    kif: Option<&'static PfiKif>,
) -> bool {
    if sn.is_none() {
        let k = PfSrcNode::zeroed();
        k.af.set(af);
        k.type_.set(type_ as u8);
        let mut a = PfAddr::zeroed();
        pf_addrcpy(&mut a, src, af);
        k.addr.set(a);
        k.rule.set_ptr(Some(rule));
        pf_inc(&PF_STATUS.scounters[SCNT_SRC_NODE_SEARCH]);
        *sn = TREE_SRC_TRACKING.find(&k);
    }
    match *sn {
        None => {
            let new = if rule.max_src_nodes == 0 || rule.src_nodes.get() < rule.max_src_nodes {
                pf_pool_get::<PfSrcNode>(&PF_SRC_TREE_PL, PR_NOWAIT)
            } else {
                pf_inc(&PF_STATUS.lcounters[LCNT_SRCNODES]);
                None
            };
            let Some(n) = new else {
                return false;
            };

            pf_init_threshold(
                &n.conn_rate,
                rule.max_src_conn_rate.limit,
                rule.max_src_conn_rate.seconds,
            );

            n.type_.set(type_ as u8);
            n.af.set(af);
            n.rule.set_ptr(Some(rule));
            let mut a = PfAddr::zeroed();
            pf_addrcpy(&mut a, src, af);
            n.addr.set(a);
            if let Some(raddr) = raddr {
                let mut r = PfAddr::zeroed();
                pf_addrcpy(&mut r, raddr, af);
                n.raddr.set(r);
            }
            // SAFETY: a fresh pool item, in no tree.
            if unsafe { TREE_SRC_TRACKING.insert(n) }.is_some() {
                if pf_debug(LOG_NOTICE) {
                    log(LOG_NOTICE, format_args!("pf: src_tree insert failed: "));
                    pf_print_host(&n.addr.get(), 0, af);
                    addlog(format_args!("\n"));
                }
                pf_pool_put(&PF_SRC_TREE_PL, n);
                return false;
            }
            n.creation.set(getuptime() as i32);
            rule.src_nodes.set(rule.src_nodes.get().wrapping_add(1));
            if let Some(kif) = kif {
                n.set_kif(Some(kif));
                pfi_kif_ref(kif, PFI_KIF_REF_SRCNODE);
            }
            pf_inc(&PF_STATUS.scounters[SCNT_SRC_NODE_INSERT]);
            pf_inc(&PF_STATUS.src_nodes);
            *sn = Some(n);
        }
        Some(n) => {
            if rule.max_src_states != 0 && n.states.get() >= rule.max_src_states {
                pf_inc(&PF_STATUS.lcounters[LCNT_SRCSTATES]);
                return false;
            }
        }
    }
    true
}

/// `pf_remove_src_node`: frees an expired node without states.
pub fn pf_remove_src_node(sn: &'static PfSrcNode) {
    if sn.states.get() > 0 || i64::from(sn.expire.get()) > getuptime() {
        return;
    }

    if let Some(rule) = sn.rule.ptr() {
        rule.src_nodes.set(rule.src_nodes.get().wrapping_sub(1));
        if rule.states_cur.get() == 0 && rule.src_nodes.get() == 0 {
            crate::net::pf_ioctl::pf_rm_rule(None, rule);
        }
    }
    // SAFETY: the node is in the tree (inserted by `pf_insert_src_node`).
    unsafe { TREE_SRC_TRACKING.remove(sn) };
    pf_inc(&PF_STATUS.scounters[SCNT_SRC_NODE_REMOVALS]);
    PF_STATUS
        .src_nodes
        .set(PF_STATUS.src_nodes.get().wrapping_sub(1));
    pfi_kif_unref(sn.kif(), PFI_KIF_REF_SRCNODE);
    pf_pool_put(&PF_SRC_TREE_PL, sn);
}

/// `pf_get_src_node`: the state's source node of `type_`.
pub fn pf_get_src_node(st: &PfState, type_: PfSnTypes) -> Option<&'static PfSrcNode> {
    st.src_nodes
        .iter()
        .find(|sni| usize::from(sni.sn().type_.get()) == type_)
        .map(|sni| sni.sn())
}

/// `pf_state_rm_src_node`: unlinks `sn` from the state.
pub fn pf_state_rm_src_node(st: &PfState, sn: &PfSrcNode) {
    for sni in st.src_nodes.iter() {
        if ptr::eq(sni.sn(), sn) {
            // SAFETY: `sni` is on the state's list, under pf_lock.
            unsafe { st.src_nodes.remove(sni) };
            pf_pool_put(&PF_SN_ITEM_PL, sni);
            sn.states.set(sn.states.get().wrapping_sub(1));
        }
    }
}

/* state table stuff */

/// `pf_state_compare_key`.
pub fn pf_state_compare_key(a: &PfStateKey, b: &PfStateKey) -> Ordering {
    let af = a.af.get();
    a.hash
        .get()
        .cmp(&b.hash.get())
        .then(a.proto.get().cmp(&b.proto.get()))
        .then(a.af.get().cmp(&b.af.get()))
        .then_with(|| pf_addr_compare(&a.addr[0].get(), &b.addr[0].get(), af).cmp(&0))
        .then_with(|| pf_addr_compare(&a.addr[1].get(), &b.addr[1].get(), af).cmp(&0))
        .then(a.port[0].get().cmp(&b.port[0].get()))
        .then(a.port[1].get().cmp(&b.port[1].get()))
        .then(a.rdomain.get().cmp(&b.rdomain.get()))
}

/// `pf_state_compare_id`.
pub fn pf_state_compare_id(a: &PfState, b: &PfState) -> Ordering {
    a.id.get()
        .cmp(&b.id.get())
        .then(a.creatorid.get().cmp(&b.creatorid.get()))
}

/// `pf_state_key_attach`: links `sk` to `st` as its `idx` key, reusing an existing equal key.
/// On failure it releases the key reference and returns `None`.
pub fn pf_state_key_attach(
    sk: &'static PfStateKey,
    st: &'static PfState,
    idx: usize,
) -> Option<&'static PfStateKey> {
    let mut sk = sk;
    let mut oldst: Option<&'static PfState> = None;

    pf_assert_locked();

    kassert!(st.key[idx].get().is_none());
    sk.sk_removed.set(0);
    // SAFETY: a key on no tree, under the state lock.
    let cur = unsafe { PF_STATETBL.insert(sk) };
    if let Some(cur) = cur {
        sk.sk_removed.set(1);
        // Key exists. Check for same kif, if none, add to key.
        for si in cur.sk_states.iter() {
            let Some(sist) = si.si_st.get() else {
                continue;
            };
            let (Some(sw), Some(ss)) = (sist.key[PF_SK_WIRE].get(), sist.key[PF_SK_STACK].get())
            else {
                continue;
            };
            if opt_eq(sist.kif.get(), st.kif.get())
                && ((sw.af.get() == sk.af.get() && sist.direction.get() == st.direction.get())
                    || (sw.af.get() != ss.af.get()
                        && sk.af.get() == ss.af.get()
                        && sist.direction.get() != st.direction.get()))
            {
                let mut reuse = false;

                if i32::from(sk.proto.get()) == IPPROTO_TCP
                    && i32::from(sist.src.state.get()) >= TCPS_FIN_WAIT_2
                    && i32::from(sist.dst.state.get()) >= TCPS_FIN_WAIT_2
                {
                    reuse = true;
                }
                if pf_debug(LOG_NOTICE) {
                    log(
                        LOG_NOTICE,
                        format_args!(
                            "pf: {} key attach {} on {}: ",
                            if idx == PF_SK_WIRE { "wire" } else { "stack" },
                            if reuse { "reuse" } else { "failed" },
                            kif_name(st.kif.get())
                        ),
                    );
                    pf_print_state_parts(
                        Some(st),
                        if idx == PF_SK_WIRE { Some(sk) } else { None },
                        if idx == PF_SK_STACK { Some(sk) } else { None },
                    );
                    addlog(format_args!(", existing: "));
                    pf_print_state_parts(
                        Some(sist),
                        if idx == PF_SK_WIRE { Some(sk) } else { None },
                        if idx == PF_SK_STACK { Some(sk) } else { None },
                    );
                    addlog(format_args!("\n"));
                }
                if reuse {
                    pf_set_protostate(sist, PF_PEER_BOTH, TCPS_CLOSED as u8);
                    // Remove late or sks can go away.
                    oldst = Some(sist);
                } else {
                    pf_state_key_unref(Some(sk));
                    return None; // collision!
                }
            }
        }

        // Reuse the existing state key.
        pf_state_key_unref(Some(sk));
        sk = cur;
    }

    let Some(si) = pf_pool_get::<PfStateItem>(&PF_STATE_ITEM_PL, PR_NOWAIT) else {
        if sk.sk_states.is_empty() {
            kassert!(cur.is_none());
            // SAFETY: `sk` was inserted above.
            unsafe { PF_STATETBL.remove(sk) };
            sk.sk_removed.set(1);
            pf_state_key_unref(Some(sk));
        }
        return None;
    };

    st.key[idx].set(Some(pf_state_key_ref(sk))); // give a ref to state
    si.si_st.set(Some(pf_state_ref(st)));

    // List is sorted, if-bound states before floating.
    // SAFETY: a fresh item onto the key's list, under the state lock.
    unsafe {
        if opt_eq(st.kif.get(), pfi_all()) {
            sk.sk_states.insert_tail(si);
        } else {
            sk.sk_states.insert_head(si);
        }
    }

    if let Some(oldst) = oldst {
        pf_remove_state(oldst);
    }

    // Caller owns the pf_state ref, which owns a pf_state_key ref now.
    Some(sk)
}

/// `a == b` for optional references (the C's pointer comparison).
pub fn opt_eq<T>(a: Option<&T>, b: Option<&T>) -> bool {
    match (a, b) {
        (Some(a), Some(b)) => ptr::eq(a, b),
        (None, None) => true,
        _ => false,
    }
}

/// The interface name of a kif, for messages.
pub fn kif_name(kif: Option<&PfiKif>) -> crate::kern::subr_prf::Str<'_> {
    crate::kern::subr_prf::Str(kif.map_or(&b""[..], |k| &k.pfik_name[..]))
}

/// `pf_detach_state`: unlinks the state from its keys.
pub fn pf_detach_state(st: &'static PfState) {
    kassert!(st.key[PF_SK_WIRE].get().is_some());
    pf_state_key_detach(st, PF_SK_WIRE);

    kassert!(st.key[PF_SK_STACK].get().is_some());
    if !opt_eq(st.key[PF_SK_STACK].get(), st.key[PF_SK_WIRE].get()) {
        pf_state_key_detach(st, PF_SK_STACK);
    }
}

/// `pf_state_key_detach`: unlinks the state from its `idx` key, freeing the key with its
/// last state.
pub fn pf_state_key_detach(st: &'static PfState, idx: usize) {
    pf_assert_locked();

    let Some(sk) = st.key[idx].get() else {
        return;
    };

    let Some(si) = sk
        .sk_states
        .iter()
        .find(|si| opt_eq(si.si_st.get(), Some(st)))
    else {
        return;
    };

    // SAFETY: `si` is on the key's list, under the state lock.
    unsafe { sk.sk_states.remove(si) };
    pf_pool_put(&PF_STATE_ITEM_PL, si);

    if sk.sk_states.is_empty() {
        // SAFETY: a key with states is in the tree.
        unsafe { PF_STATETBL.remove(sk) };
        sk.sk_removed.set(1);
        pf_state_key_unlink_reverse(sk);
        pf_state_key_unlink_inpcb(sk);
        pf_state_key_unref(Some(sk));
    }

    pf_state_unref(Some(st));
}

/// `pf_alloc_state_key`.
pub fn pf_alloc_state_key(pool_flags: i32) -> Option<&'static PfStateKey> {
    let sk = pf_pool_get::<PfStateKey>(&PF_STATE_KEY_PL, pool_flags)?;

    refcnt_init(&sk.sk_refcnt);
    sk.sk_states.init();
    sk.sk_removed.set(1);

    Some(sk)
}

/// `pf_state_key_addr_setup`: copies the addresses into `addrs` (a key's `addr[2]`); for
/// ICMPv6 neighbour discovery the target address replaces one of them, and a multicast
/// lookup uses the link-local all-nodes source. -1 if the message cannot have such a state.
#[allow(clippy::too_many_arguments)]
fn pf_state_key_addr_setup(
    pd: &mut PfPdesc,
    addrs: &mut [PfAddr; 2],
    sidx: usize,
    saddr: Option<&PfAddr>,
    didx: usize,
    daddr: Option<&PfAddr>,
    af: SaFamily,
    multi: bool,
) -> i32 {
    #[cfg_attr(not(feature = "inet6"), allow(unused_mut))]
    let mut saddr = saddr.copied();
    #[cfg_attr(not(feature = "inet6"), allow(unused_mut))]
    let mut daddr = daddr.copied();
    #[cfg(feature = "inet6")]
    if af != AF_INET && i32::from(pd.proto) == IPPROTO_ICMPV6 {
        use crate::netinet::icmp6::{ND_NEIGHBOR_ADVERT, ND_NEIGHBOR_SOLICIT};
        use crate::netinet6::in6::{
            __IPV6_ADDR_INT32_MLL, __IPV6_ADDR_INT32_ONE, in6_is_addr_multicast,
        };
        match pd.icmp6().icmp6_type {
            ND_NEIGHBOR_SOLICIT => {
                if multi {
                    return -1;
                }
                daddr = Some(PfAddr::from_v6(pd.nd_ns().nd_ns_target));
            }
            ND_NEIGHBOR_ADVERT => {
                if multi {
                    return -1;
                }
                saddr = Some(PfAddr::from_v6(pd.nd_ns().nd_ns_target));
                if in6_is_addr_multicast(&pd.ld_addr(pd.dst).v6()) {
                    addrs[didx] = PfAddr::zeroed();
                    daddr = None; // overwritten
                }
            }
            _ => {
                if multi {
                    let a = &mut addrs[sidx];
                    a.set_addr32(0, __IPV6_ADDR_INT32_MLL);
                    a.set_addr32(1, 0);
                    a.set_addr32(2, 0);
                    a.set_addr32(3, __IPV6_ADDR_INT32_ONE);
                    saddr = None; // overwritten
                }
            }
        }
    }
    #[cfg(not(feature = "inet6"))]
    let _ = (&pd, multi);
    if let Some(s) = saddr {
        pf_addrcpy(&mut addrs[sidx], &s, af);
    }
    if let Some(d) = daddr {
        pf_addrcpy(&mut addrs[didx], &d, af);
    }
    0
}

/// `pf_state_key_setup`: the wire and stack keys of a new state. If returning an error the
/// keys are freed here.
pub fn pf_state_key_setup(
    pd: &mut PfPdesc,
    skw: &mut Option<&'static PfStateKey>,
    sks: &mut Option<&'static PfStateKey>,
    rtableid: i32,
) -> Result<(), Errno> {
    let mut wrdom = u32::from(pd.rdomain);
    let afto = pd.af != pd.naf;

    let Some(sk1) = pf_alloc_state_key(PR_NOWAIT) else {
        return Err(Errno::ENOMEM);
    };

    let src = pd.ld_addr(pd.src);
    let dst = pd.ld_addr(pd.dst);
    let mut a = [PfAddr::zeroed(); 2];
    pf_state_key_addr_setup(
        pd,
        &mut a,
        usize::from(pd.sidx),
        Some(&src),
        usize::from(pd.didx),
        Some(&dst),
        pd.af,
        false,
    );
    sk1.addr[0].set(a[0]);
    sk1.addr[1].set(a[1]);
    sk1.port[usize::from(pd.sidx)].set(pd.osport);
    sk1.port[usize::from(pd.didx)].set(pd.odport);
    sk1.proto.set(pd.proto);
    sk1.af.set(pd.af);
    sk1.rdomain.set(pd.rdomain);
    sk1.hash.set(pf_pkt_hash(
        sk1.af.get(),
        sk1.proto.get(),
        &a[0],
        &a[1],
        sk1.port[0].get(),
        sk1.port[1].get(),
    ));
    if rtableid >= 0 {
        wrdom = rtable_l2(rtableid as u32);
    }

    let sk2 = if pf_aneq(&pd.nsaddr, &src, pd.af)
        || pf_aneq(&pd.ndaddr, &dst, pd.af)
        || pd.nsport != pd.osport
        || pd.ndport != pd.odport
        || wrdom != u32::from(pd.rdomain)
        || afto
    {
        // NAT/NAT64
        let Some(sk2) = pf_alloc_state_key(PR_NOWAIT) else {
            pf_state_key_unref(Some(sk1));
            return Err(Errno::ENOMEM);
        };
        let (si, di) = if afto {
            (usize::from(pd.didx), usize::from(pd.sidx))
        } else {
            (usize::from(pd.sidx), usize::from(pd.didx))
        };
        let mut a2 = [PfAddr::zeroed(); 2];
        let (ns, nd) = (pd.nsaddr, pd.ndaddr);
        pf_state_key_addr_setup(pd, &mut a2, si, Some(&ns), di, Some(&nd), pd.naf, false);
        sk2.addr[0].set(a2[0]);
        sk2.addr[1].set(a2[1]);
        sk2.port[si].set(pd.nsport);
        sk2.port[di].set(pd.ndport);
        if afto {
            sk2.proto.set(match i32::from(pd.proto) {
                IPPROTO_ICMP => crate::netinet::in_::IPPROTO_ICMPV6 as u8,
                crate::netinet::in_::IPPROTO_ICMPV6 => IPPROTO_ICMP as u8,
                _ => pd.proto,
            });
        } else {
            sk2.proto.set(pd.proto);
        }
        sk2.af.set(pd.naf);
        sk2.rdomain.set(wrdom as u16);
        sk2.hash.set(pf_pkt_hash(
            sk2.af.get(),
            sk2.proto.get(),
            &a2[0],
            &a2[1],
            sk2.port[0].get(),
            sk2.port[1].get(),
        ));
        sk2
    } else {
        pf_state_key_ref(sk1)
    };

    if pd.dir == PF_IN {
        *skw = Some(sk1);
        *sks = Some(sk2);
    } else {
        *sks = Some(sk1);
        *skw = Some(sk2);
    }

    if pf_debug(LOG_DEBUG) {
        log(LOG_DEBUG, format_args!("pf: key setup: "));
        pf_print_state_parts(None, *skw, *sks);
        addlog(format_args!("\n"));
    }

    Ok(())
}

/// `pf_state_insert`: links the state with its keys, inserts the keys into the state table
/// and the state into the id tree (and would tell pfsync). It owns the key references it is
/// given; on failure they are released. On success the caller owns a state reference that
/// allows it to access the state keys.
pub fn pf_state_insert(
    kif: &'static PfiKif,
    skwp: &mut Option<&'static PfStateKey>,
    sksp: &mut Option<&'static PfStateKey>,
    st: &'static PfState,
) -> bool {
    let (Some(skw0), Some(sks0)) = (*skwp, *sksp) else {
        return false;
    };
    let mut sks = sks0;
    let same = ptr::eq(skw0, sks0);

    pf_assert_locked();

    st.kif.set(Some(kif));
    pf_state_enter_write();

    let Some(skw) = pf_state_key_attach(skw0, st, PF_SK_WIRE) else {
        pf_state_key_unref(Some(sks));
        pf_state_exit_write();
        return false;
    };

    if same {
        // pf_state_key_attach might have swapped skw.
        if !ptr::eq(skw, sks) {
            pf_state_key_unref(Some(sks));
            sks = pf_state_key_ref(skw);
        }
        st.key[PF_SK_STACK].set(Some(sks));
    } else if pf_state_key_attach(sks, st, PF_SK_STACK).is_none() {
        pf_state_key_detach(st, PF_SK_WIRE);
        pf_state_exit_write();
        return false;
    }

    if st.id.get() == 0 && st.creatorid.get() == 0 {
        let id = PF_STATUS.stateid.get();
        PF_STATUS.stateid.set(id.wrapping_add(1));
        st.id.set(id.to_be());
        st.creatorid.set(PF_STATUS.hostid.get());
    }
    // SAFETY: a new state, in no tree, under the state lock.
    if unsafe { TREE_ID.insert(st) }.is_some() {
        if pf_debug(LOG_NOTICE) {
            log(
                LOG_NOTICE,
                format_args!(
                    "pf: state insert failed: id: {:016x} creatorid: {:08x}",
                    u64::from_be(st.id.get()),
                    u32::from_be(st.creatorid.get())
                ),
            );
            addlog(format_args!("\n"));
        }
        pf_detach_state(st);
        pf_state_exit_write();
        return false;
    }
    pf_state_list_insert(&PF_STATE_LIST, st);
    PF_STATUS_FCOUNTERS[FCNT_STATE_INSERT].fetch_add(1, AtomicOrdering::Relaxed);
    pf_inc(&PF_STATUS.states);
    pfi_kif_ref(kif, PFI_KIF_REF_STATE);
    pf_state_exit_write();

    crate::net::if_pfsync::pfsync_insert_state(st);

    *skwp = Some(skw);
    *sksp = Some(sks);

    true
}

/// `pf_find_state_byid`.
pub fn pf_find_state_byid(key: &PfStateCmp) -> Option<&'static PfState> {
    PF_STATUS_FCOUNTERS[FCNT_STATE_SEARCH].fetch_add(1, AtomicOrdering::Relaxed);

    let k = PfState::key(key.id, key.creatorid);
    TREE_ID.find(&k)
}

/// `pf_compare_state_keys`: `a` (from the header) and `b` (new) must be exact opposites of
/// each other.
pub fn pf_compare_state_keys(
    a: &PfStateKey,
    b: &PfStateKey,
    kif: Option<&PfiKif>,
    dir: u8,
) -> bool {
    if a.af.get() == b.af.get()
        && a.proto.get() == b.proto.get()
        && pf_aeq(&a.addr[0].get(), &b.addr[1].get(), a.af.get())
        && pf_aeq(&a.addr[1].get(), &b.addr[0].get(), a.af.get())
        && a.port[0].get() == b.port[1].get()
        && a.port[1].get() == b.port[0].get()
        && a.rdomain.get() == b.rdomain.get()
    {
        true
    } else {
        // Mismatch. Must not happen.
        if pf_debug(LOG_ERR) {
            log(
                LOG_ERR,
                format_args!(
                    "pf: state key linking mismatch! dir={}, if={}, stored af={}, a0: ",
                    if dir == PF_OUT { "OUT" } else { "IN" },
                    kif_name(kif),
                    a.af.get()
                ),
            );
            pf_print_host(&a.addr[0].get(), a.port[0].get(), a.af.get());
            addlog(format_args!(", a1: "));
            pf_print_host(&a.addr[1].get(), a.port[1].get(), a.af.get());
            addlog(format_args!(", proto={}", a.proto.get()));
            addlog(format_args!(", found af={}, a0: ", b.af.get()));
            pf_print_host(&b.addr[0].get(), b.port[0].get(), b.af.get());
            addlog(format_args!(", a1: "));
            pf_print_host(&b.addr[1].get(), b.port[1].get(), b.af.get());
            addlog(format_args!(", proto={}", b.proto.get()));
            addlog(format_args!("\n"));
        }
        false
    }
}

/// `pf_find_state`: looks up the state of the packet `pd` describes by `key`. `PF_MATCH`
/// with `*stp` set, or `PF_DROP`.
pub fn pf_find_state(
    pd: &mut PfPdesc,
    key: &PfStateKeyCmp,
    stp: &mut Option<&'static PfState>,
) -> u8 {
    PF_STATUS_FCOUNTERS[FCNT_STATE_SEARCH].fetch_add(1, AtomicOrdering::Relaxed);
    let keysk = PfStateKey::from_cmp(key);
    if pf_debug(LOG_DEBUG) {
        log(
            LOG_DEBUG,
            format_args!(
                "pf: key search, {} on {}: ",
                if pd.dir == PF_OUT { "out" } else { "in" },
                kif_name(pd.kif)
            ),
        );
        pf_print_state_parts_key(None, Some(&keysk), None);
        addlog(format_args!("\n"));
    }

    let Some(m) = pd.m else {
        return PF_DROP;
    };
    let mut pkt_sk: Option<&'static PfStateKey> = None;
    let mut sk: Option<&'static PfStateKey> = None;
    if pd.dir == PF_OUT {
        // First if block deals with outbound forwarded packet.
        pkt_sk = pf_mbuf_statekey(m);

        if !pf_state_key_isvalid(pkt_sk) {
            pf_mbuf_unlink_state_key(m);
            pkt_sk = None;
        }

        if let Some(p) = pkt_sk
            && pf_state_key_isvalid(p.sk_reverse.get())
        {
            sk = p.sk_reverse.get();
        }

        if pkt_sk.is_none() {
            // Here we deal with local outbound packet.
            if let Some(inp) = pf_mbuf_inp(m) {
                mtx_enter(&PF_INP_MTX);
                let inp_sk = inpcb_pf_sk(inp);
                if pf_state_key_isvalid(inp_sk) {
                    sk = inp_sk;
                    mtx_leave(&PF_INP_MTX);
                } else if let Some(inp_sk) = inp_sk {
                    kassert!(opt_eq(inp_sk.sk_inp.get(), Some(inp)));
                    inp_sk.sk_inp.set(None);
                    inpcb_set_pf_sk(inp, None);
                    mtx_leave(&PF_INP_MTX);

                    pf_state_key_unref(Some(inp_sk));
                    crate::netinet::in_pcb::in_pcbunref(Some(inp));
                } else {
                    mtx_leave(&PF_INP_MTX);
                }
            }
        }
    }

    let sk = match sk {
        Some(sk) => sk,
        None => {
            let Some(found) = PF_STATETBL.find(&keysk) else {
                return PF_DROP;
            };
            if pd.dir == PF_OUT
                && let Some(p) = pkt_sk
                && pf_compare_state_keys(p, found, pd.kif, pd.dir)
            {
                pf_state_key_link_reverse(found, p);
            } else if pd.dir == PF_OUT {
                pf_state_key_link_inpcb(found, pf_mbuf_inp(m));
            }
            found
        }
    };

    // Remove firewall data from outbound packet.
    if pd.dir == PF_OUT {
        pf_pkt_addr_changed(m);
    }

    let didx = if pd.dir == PF_IN {
        PF_SK_WIRE
    } else {
        PF_SK_STACK
    };

    // List is sorted, if-bound states before floating ones.
    let mut st: Option<&'static PfState> = None;
    for si in sk.sk_states.iter() {
        let Some(sist) = si.si_st.get() else {
            continue;
        };
        if usize::from(sist.timeout.get()) == PFTM_PURGE {
            continue;
        }
        if !opt_eq(sist.kif.get(), pfi_all()) && !opt_eq(sist.kif.get(), pd.kif) {
            continue;
        }

        let (Some(sw), Some(ss)) = (sist.key[PF_SK_WIRE].get(), sist.key[PF_SK_STACK].get()) else {
            continue;
        };
        // af-to needs to handled specially.
        if sw.af.get() == ss.af.get() {
            if !opt_eq(Some(sk), sist.key[didx].get()) {
                continue;
            }
        } else {
            // af-to case: af-to creates state for incoming (PF_IN) connections, and then
            // forces forwarding without creating an outgoing state. This means the one state
            // covers both sides of the stack, so should only match when pd dir is PF_IN.
            if pd.dir != PF_IN {
                continue;
            }
            // One of the st keys has to be sk.
        }

        st = Some(sist);
        break;
    }

    let Some(st) = st else {
        return PF_DROP;
    };
    if st.state_flags.get() & PFSTATE_INP_UNLINKED != 0 {
        return PF_DROP;
    }

    if let Some(r) = st.rule.ptr()
        && r.pktrate.limit.get() != 0
        && pd.dir == st.direction.get()
    {
        pf_add_threshold(&r.pktrate);
        if pf_check_threshold(&r.pktrate) {
            return PF_DROP;
        }
    }

    *stp = Some(st);

    PF_MATCH
}

/// `pf_find_state_all`: a state of `key` in direction `dir`; `more` counts the others.
pub fn pf_find_state_all(
    key: &PfStateKeyCmp,
    dir: u8,
    more: Option<&mut i32>,
) -> Option<&'static PfState> {
    PF_STATUS_FCOUNTERS[FCNT_STATE_SEARCH].fetch_add(1, AtomicOrdering::Relaxed);

    let keysk = PfStateKey::from_cmp(key);
    let sk = PF_STATETBL.find(&keysk)?;

    let mut ret: Option<&'static PfState> = None;
    let mut more = more;
    for si in sk.sk_states.iter() {
        let Some(sist) = si.si_st.get() else {
            continue;
        };
        let k = if dir == PF_IN {
            sist.key[PF_SK_WIRE].get()
        } else {
            sist.key[PF_SK_STACK].get()
        };
        if dir == PF_INOUT || opt_eq(Some(sk), k) {
            match more.as_deref_mut() {
                None => return Some(sist),
                Some(n) => {
                    if ret.is_some() {
                        *n += 1;
                    } else {
                        ret = Some(sist);
                    }
                }
            }
        }
    }
    ret
}

/// `pf_state_peer_hton`.
pub fn pf_state_peer_hton(s: &PfStatePeer, d: &mut PfsyncStatePeer) {
    d.seqlo = s.seqlo.get().to_be();
    d.seqhi = s.seqhi.get().to_be();
    d.seqdiff = s.seqdiff.get().to_be();
    d.max_win = s.max_win.get().to_be();
    d.mss = s.mss.get().to_be();
    d.state = s.state.get();
    d.wscale = s.wscale.get();
    if let Some(scrub) = s.scrub.get() {
        d.scrub.pfss_flags = (scrub.pfss_flags.get() & PFSS_TIMESTAMP).to_be();
        d.scrub.pfss_ttl = scrub.pfss_ttl.get();
        d.scrub.pfss_ts_mod = scrub.pfss_ts_mod.get().to_be();
        d.scrub.scrub_flag = PFSYNC_SCRUB_FLAG_VALID;
    }
}

/// `pf_state_peer_ntoh`.
pub fn pf_state_peer_ntoh(s: &PfsyncStatePeer, d: &PfStatePeer) {
    d.seqlo.set(u32::from_be(s.seqlo));
    d.seqhi.set(u32::from_be(s.seqhi));
    d.seqdiff.set(u32::from_be(s.seqdiff));
    d.max_win.set(u16::from_be(s.max_win));
    d.mss.set(u16::from_be(s.mss));
    d.state.set(s.state);
    d.wscale.set(s.wscale);
    if s.scrub.scrub_flag == PFSYNC_SCRUB_FLAG_VALID
        && let Some(scrub) = d.scrub.get()
    {
        let flags = s.scrub.pfss_flags;
        scrub.pfss_flags.set(u16::from_be(flags) & PFSS_TIMESTAMP);
        scrub.pfss_ttl.set(s.scrub.pfss_ttl);
        let m = s.scrub.pfss_ts_mod;
        scrub.pfss_ts_mod.set(u32::from_be(m));
    }
}

/// `pf_state_export`: the state as `struct pfsync_state`.
pub fn pf_state_export(sp: &mut PfsyncState, st: &'static PfState) {
    *sp = PfsyncState::default();

    // Copy from state key.
    let (Some(skw), Some(sks)) = (st.key[PF_SK_WIRE].get(), st.key[PF_SK_STACK].get()) else {
        return;
    };
    let mut keys = [PfsyncStateKey::default(); 2];
    for (k, s) in [(PF_SK_WIRE, skw), (PF_SK_STACK, sks)] {
        keys[k].addr = [s.addr[0].get(), s.addr[1].get()];
        keys[k].port = [s.port[0].get(), s.port[1].get()];
        keys[k].rdomain = s.rdomain.get().to_be();
        keys[k].af = s.af.get();
    }
    sp.key = keys;
    sp.rtableid = [
        st.rtableid[PF_SK_WIRE].get().to_be(),
        st.rtableid[PF_SK_STACK].get().to_be(),
    ];
    sp.proto = skw.proto.get();
    sp.af = skw.af.get();

    // Copy from state.
    let mut ifname = [0u8; crate::net::if_::IFNAMSIZ];
    if let Some(kif) = st.kif.get() {
        pf_strlcpy(&mut ifname, &kif.pfik_name);
    }
    sp.ifname = ifname;
    sp.rt = st.rt.get();
    sp.rt_addr = st.rt_addr.get();
    let now = getuptime();
    sp.creation = ((now - i64::from(st.creation.get())) as u32).to_be();
    let expire = pf_state_expires(st, st.timeout.get());
    sp.expire = if i64::from(expire) <= now {
        0u32.to_be()
    } else {
        ((i64::from(expire) - now) as u32).to_be()
    };

    sp.direction = st.direction.get();
    sp.log = st.log.get();
    sp.timeout = st.timeout.get();
    let mut flags = st.state_flags.get().to_be();
    if st.sync_defer.get().is_some() {
        flags |= PFSTATE_ACK.to_be();
    }
    sp.state_flags = flags;
    if !st.src_nodes.is_empty() {
        sp.sync_flags |= PFSYNC_FLAG_SRCNODE;
    }

    sp.id = st.id.get();
    sp.creatorid = st.creatorid.get();
    let mut src = PfsyncStatePeer::default();
    pf_state_peer_hton(&st.src, &mut src);
    sp.src = src;
    let mut dst = PfsyncStatePeer::default();
    pf_state_peer_hton(&st.dst, &mut dst);
    sp.dst = dst;

    sp.rule = st.rule.ptr().map_or(u32::MAX, |r| r.nr.get()).to_be();
    sp.anchor = st.anchor.ptr().map_or(u32::MAX, |r| r.nr.get()).to_be();
    sp.nat_rule = u32::MAX.to_be(); // left for compat, nat_rule is gone

    sp.packets = [
        pf_state_counter_hton(st.packets[0].get()),
        pf_state_counter_hton(st.packets[1].get()),
    ];
    sp.bytes = [
        pf_state_counter_hton(st.bytes[0].get()),
        pf_state_counter_hton(st.bytes[1].get()),
    ];

    sp.max_mss = st.max_mss.get().to_be();
    sp.min_ttl = st.min_ttl.get();
    sp.set_tos = st.set_tos.get();
    sp.set_prio = [st.set_prio[0].get(), st.set_prio[1].get()];
}

/// `pf_state_alloc_scrub_memory`.
pub fn pf_state_alloc_scrub_memory(s: &PfsyncStatePeer, d: &PfStatePeer) -> Result<(), Errno> {
    if s.scrub.scrub_flag != 0 && d.scrub.get().is_none() {
        return crate::net::pf_norm::pf_normalize_tcp_alloc(d);
    }
    Ok(())
}

/// `pf_state_import`: recreates a state from a `struct pfsync_state` that came from a pfsync(4)
/// peer (`PFSYNC_SI_PFSYNC`) or `DIOCADDSTATE` (`PFSYNC_SI_IOCTL`). A state of an unknown
/// interface or without a family is skipped (`Ok`), unless the ioctl asked.
pub fn pf_state_import(sp: &PfsyncState, flags: i32) -> Result<(), Errno> {
    use crate::net::if_pfsync::{
        PFSYNC_S_NONE, PFSYNC_SI_CKSUM, PFSYNC_SI_IOCTL, pfsync_init_state,
    };
    use crate::net::pf_if::pfi_kif_get;
    use crate::netinet::in_::IPPROTO_ICMPV6;
    use crate::sys::pool::{PR_LIMITFAIL, PR_WAITOK};

    let mut skw: Option<&'static PfStateKey> = None;
    let mut sks: Option<&'static PfStateKey> = None;
    let mut error = Errno::ENOMEM;

    pf_assert_locked();

    if sp.creatorid == 0 {
        crate::dpfprintf!(
            LOG_NOTICE,
            "pf_state_import: invalid creator id: {:08x}",
            u32::from_be(sp.creatorid)
        );
        return Err(Errno::EINVAL);
    }

    let ifname = sp.ifname;
    let Some(kif) = pfi_kif_get(&ifname, None) else {
        crate::dpfprintf!(
            LOG_NOTICE,
            "pf_state_import: unknown interface: {}",
            crate::kern::subr_prf::Str(&ifname)
        );
        if flags & PFSYNC_SI_IOCTL != 0 {
            return Err(Errno::EINVAL);
        }
        return Ok(()); // skip this state
    };

    if sp.af == 0 {
        return Ok(()); // skip this state
    }

    // If the ruleset checksums match or the state is coming from the ioctl, it's safe to
    // associate the state with the rule of that number.
    let def: &'static PfRule = &crate::net::pf_ioctl::PF_DEFAULT_RULE;
    let main = crate::net::pf_ruleset::pf_main_ruleset();
    let r: &'static PfRule = if sp.rule != u32::MAX.to_be()
        && sp.anchor == u32::MAX.to_be()
        && flags & (PFSYNC_SI_IOCTL | PFSYNC_SI_CKSUM) != 0
        && u32::from_be(sp.rule) < main.active.rcount.get()
    {
        main.queues[main.active.ptr.get()]
            .iter()
            .nth(u32::from_be(sp.rule) as usize)
            .unwrap_or(def)
    } else {
        def
    };

    let pool_flags = if flags & PFSYNC_SI_IOCTL != 0 {
        PR_WAITOK | PR_LIMITFAIL
    } else {
        PR_NOWAIT | PR_LIMITFAIL
    };

    let mut st: Option<&'static PfState> = None;
    'cleanup_state: {
        'cleanup: {
            if r.max_states != 0 && r.states_cur.get() >= r.max_states {
                break 'cleanup;
            }

            // pool_get(..., PR_ZERO): pf_pool_get zeroes.
            st = pf_pool_get::<PfState>(&PF_STATE_PL, pool_flags);
            let Some(s) = st else {
                break 'cleanup;
            };

            skw = pf_alloc_state_key(pool_flags);
            let Some(kw) = skw else {
                break 'cleanup;
            };

            let keys = sp.key;
            let (wk, sk) = (&keys[PF_SK_WIRE], &keys[PF_SK_STACK]);
            if (wk.af != 0 && wk.af != sk.af)
                || pf_aneq(&wk.addr[0], &sk.addr[0], sp.af)
                || pf_aneq(&wk.addr[1], &sk.addr[1], sp.af)
                || wk.port[0] != sk.port[0]
                || wk.port[1] != sk.port[1]
                || wk.rdomain != sk.rdomain
            {
                sks = pf_alloc_state_key(pool_flags);
                if sks.is_none() {
                    break 'cleanup;
                }
            } else {
                sks = Some(pf_state_key_ref(kw));
            }
            let Some(ks) = sks else {
                break 'cleanup;
            };

            // allocate memory for scrub info
            let (src, dst) = (sp.src, sp.dst);
            if pf_state_alloc_scrub_memory(&src, &s.src).is_err()
                || pf_state_alloc_scrub_memory(&dst, &s.dst).is_err()
            {
                break 'cleanup;
            }

            // copy to state key(s)
            kw.addr[0].set(wk.addr[0]);
            kw.addr[1].set(wk.addr[1]);
            kw.port[0].set(wk.port[0]);
            kw.port[1].set(wk.port[1]);
            kw.rdomain.set(u16::from_be(wk.rdomain));
            kw.proto.set(sp.proto);
            kw.af.set(if wk.af != 0 { wk.af } else { sp.af });
            kw.hash.set(pf_pkt_hash(
                kw.af.get(),
                kw.proto.get(),
                &kw.addr[0].get(),
                &kw.addr[1].get(),
                kw.port[0].get(),
                kw.port[1].get(),
            ));

            if !ptr::eq(ks, kw) {
                ks.addr[0].set(sk.addr[0]);
                ks.addr[1].set(sk.addr[1]);
                ks.port[0].set(sk.port[0]);
                ks.port[1].set(sk.port[1]);
                ks.rdomain.set(u16::from_be(sk.rdomain));
                ks.af.set(if sk.af != 0 { sk.af } else { sp.af });
                if ks.af.get() != kw.af.get() {
                    ks.proto.set(match i32::from(sp.proto) {
                        IPPROTO_ICMP => IPPROTO_ICMPV6 as u8,
                        IPPROTO_ICMPV6 => IPPROTO_ICMP as u8,
                        _ => sp.proto,
                    });
                } else {
                    ks.proto.set(sp.proto);
                }

                if (ks.af.get() != AF_INET && ks.af.get() != AF_INET6)
                    || (kw.af.get() != AF_INET && kw.af.get() != AF_INET6)
                {
                    error = Errno::EINVAL;
                    break 'cleanup;
                }

                ks.hash.set(pf_pkt_hash(
                    ks.af.get(),
                    ks.proto.get(),
                    &ks.addr[0].get(),
                    &ks.addr[1].get(),
                    ks.port[0].get(),
                    ks.port[1].get(),
                ));
            } else if ks.af.get() != AF_INET && ks.af.get() != AF_INET6 {
                error = Errno::EINVAL;
                break 'cleanup;
            }
            let rtableid = sp.rtableid;
            s.rtableid[PF_SK_WIRE].set(i32::from_be(rtableid[PF_SK_WIRE]));
            s.rtableid[PF_SK_STACK].set(i32::from_be(rtableid[PF_SK_STACK]));

            // copy to state
            s.rt_addr.set(sp.rt_addr);
            s.rt.set(sp.rt);
            let now = getuptime();
            s.creation
                .set((now - i64::from(u32::from_be(sp.creation))) as i32);
            s.expire.set(now as i32);
            if u32::from_be(sp.expire) != 0 {
                let mut timeout = r.timeout(usize::from(sp.timeout));
                if timeout == 0 {
                    timeout = def.timeout(usize::from(sp.timeout));
                }

                // sp->expire may have been adaptively scaled by export.
                let adj = timeout.wrapping_sub(u32::from_be(sp.expire));
                s.expire
                    .set((s.expire.get() as u32).wrapping_sub(adj) as i32);
            }

            s.direction.set(sp.direction);
            s.log.set(sp.log);
            s.timeout.set(sp.timeout);
            s.state_flags.set(u16::from_be(sp.state_flags));
            s.max_mss.set(u16::from_be(sp.max_mss));
            s.min_ttl.set(sp.min_ttl);
            s.set_tos.set(sp.set_tos);
            s.set_prio[0].set(sp.set_prio[0]);
            s.set_prio[1].set(sp.set_prio[1]);

            s.id.set(sp.id);
            s.creatorid.set(sp.creatorid);
            pf_state_peer_ntoh(&src, &s.src);
            pf_state_peer_ntoh(&dst, &s.dst);

            s.rule.set_ptr(Some(r));
            s.anchor.set_ptr(None);

            refcnt_init(&s.refcnt);
            mtx_init(&s.mtx, IPL_NET);

            // XXX when we have anchors, use STATE_INC_COUNTERS
            r.states_cur.set(r.states_cur.get().wrapping_add(1));
            r.states_tot.set(r.states_tot.get().wrapping_add(1));

            s.sync_state.set(PFSYNC_S_NONE);
            s.pfsync_time.set(getuptime() as i32);
            pfsync_init_state(s, skw, sks, flags);

            if !pf_state_insert(kif, &mut skw, &mut sks, s) {
                // XXX when we have anchors, use STATE_DEC_COUNTERS
                r.states_cur.set(r.states_cur.get().wrapping_sub(1));
                error = Errno::EEXIST;
                break 'cleanup_state;
            }

            return Ok(());
        }

        // cleanup:
        pf_state_key_unref(skw);
        pf_state_key_unref(sks);
    }

    // cleanup_state: pf_state_insert frees the state keys
    if let Some(s) = st {
        if let Some(scrub) = s.dst.scrub.get() {
            pf_pool_put(&crate::net::pf_norm::PF_STATE_SCRUB_PL, scrub);
        }
        if let Some(scrub) = s.src.scrub.get() {
            pf_pool_put(&crate::net::pf_norm::PF_STATE_SCRUB_PL, scrub);
        }
        pf_pool_put(&PF_STATE_PL, s);
    }
    Err(error)
}

/* END state table stuff */

/// `pf_purge_states_task`.
static PF_PURGE_STATES_TASK: crate::sys::task::Task =
    crate::sys::task::Task::new(pf_purge_states, ptr::null_mut());

/// `pf_purge_states_to`.
pub static PF_PURGE_STATES_TO: crate::sys::timeout::Timeout =
    crate::sys::timeout::Timeout::new(pf_purge_states_tick, ptr::null_mut());

/// `pf_purge_states_limit`: how many states to scan this interval. Set when the timeout
/// fires and reduced by the task, which reschedules itself until it reaches zero and then
/// adds the timeout again.
static PF_PURGE_STATES_LIMIT: core::sync::atomic::AtomicU32 = core::sync::atomic::AtomicU32::new(0);

/// `pf_purge_states_collect`: limit how many states are processed with locks held per run of
/// the state purge task.
static PF_PURGE_STATES_COLLECT: core::sync::atomic::AtomicU32 =
    core::sync::atomic::AtomicU32::new(64);

/// `pf_purge_states_tick`: process a fraction of the state table every second.
pub fn pf_purge_states_tick(_null: *mut c_void) {
    let mut limit = PF_STATUS.states.get();
    let interval = crate::net::pf_ioctl::PF_DEFAULT_RULE.timeout(PFTM_INTERVAL);

    if limit == 0 {
        crate::kern::kern_timeout::timeout_add_sec(&PF_PURGE_STATES_TO, 1);
        return;
    }

    if interval > 1 {
        limit /= interval;
    }

    PF_PURGE_STATES_LIMIT.store(limit, AtomicOrdering::Relaxed);
    crate::kern::kern_task::task_add(crate::kern::kern_task::SYSTQMP, &PF_PURGE_STATES_TASK);
}

/// `pf_purge_states`.
pub fn pf_purge_states(_null: *mut c_void) {
    let collect = PF_PURGE_STATES_COLLECT.load(AtomicOrdering::Relaxed);
    let mut limit = PF_PURGE_STATES_LIMIT.load(AtomicOrdering::Relaxed);
    if limit < collect {
        limit = collect;
    }

    let scanned = pf_purge_expired_states(limit, collect);
    let cur = PF_PURGE_STATES_LIMIT.load(AtomicOrdering::Relaxed);
    if scanned >= cur {
        // We've run out of states to scan this "interval".
        crate::kern::kern_timeout::timeout_add_sec(&PF_PURGE_STATES_TO, 1);
        return;
    }

    PF_PURGE_STATES_LIMIT.store(cur - scanned, AtomicOrdering::Relaxed);
    crate::kern::kern_task::task_add(crate::kern::kern_task::SYSTQMP, &PF_PURGE_STATES_TASK);
}

/// `pf_purge_to`.
pub static PF_PURGE_TO: crate::sys::timeout::Timeout =
    crate::sys::timeout::Timeout::new(pf_purge_tick, ptr::null_mut());

/// `pf_purge_task`.
pub static PF_PURGE_TASK: crate::sys::task::Task =
    crate::sys::task::Task::new(pf_purge, ptr::null_mut());

/// `pf_purge_tick`.
pub fn pf_purge_tick(_null: *mut c_void) {
    crate::kern::kern_task::task_add(crate::kern::kern_task::SYSTQMP, &PF_PURGE_TASK);
}

/// `pf_purge`: expires source nodes, sources and fragments.
pub fn pf_purge(_null: *mut c_void) {
    let interval = core::cmp::max(
        1,
        crate::net::pf_ioctl::PF_DEFAULT_RULE.timeout(PFTM_INTERVAL),
    );

    pf_lock();

    pf_purge_expired_src_nodes();
    pf_source_purge();

    pf_unlock();

    // Fragments don't require PF_LOCK(), they use their own lock.
    crate::net::pf_norm::pf_purge_expired_fragments();

    // Interpret the interval as idle time between runs.
    crate::kern::kern_timeout::timeout_add_sec(&PF_PURGE_TO, interval as i32);
}

/// `pf_state_expires`: when the state expires, given the caller's view `stimeout` of its
/// timeout (the purge task reads it once while the state may be changing; a stale value only
/// delays the purge to the next pass). States in a special timeout (`>= PFTM_MAX`) expire now.
pub fn pf_state_expires(st: &'static PfState, stimeout: u8) -> i32 {
    // Handle all PFTM_* >= PFTM_MAX here.
    if usize::from(stimeout) >= PFTM_MAX {
        return 0;
    }

    kassert!(usize::from(stimeout) < PFTM_MAX);

    let def = &crate::net::pf_ioctl::PF_DEFAULT_RULE;
    let Some(rule) = st.rule.ptr() else {
        return 0;
    };
    let mut timeout = rule.timeout(usize::from(stimeout));
    if timeout == 0 {
        timeout = def.timeout(usize::from(stimeout));
    }

    let mut start = rule.timeout(PFTM_ADAPTIVE_START);
    let end;
    let states;
    if start != 0 {
        end = rule.timeout(PFTM_ADAPTIVE_END);
        states = rule.states_cur.get();
    } else {
        start = def.timeout(PFTM_ADAPTIVE_START);
        end = def.timeout(PFTM_ADAPTIVE_END);
        states = PF_STATUS.states.get();
    }
    if end != 0 && states > start && start < end {
        if states >= end {
            return 0;
        }

        timeout = (u64::from(timeout) * u64::from(end - states) / u64::from(end - start)) as u32;
    }

    st.expire.get().wrapping_add(timeout as i32)
}

/// `pf_purge_expired_src_nodes`.
pub fn pf_purge_expired_src_nodes() {
    pf_assert_locked();

    for cur in TREE_SRC_TRACKING.iter() {
        if cur.states.get() == 0 && i64::from(cur.expire.get()) <= getuptime() {
            pf_remove_src_node(cur);
        }
    }
}

/// `pf_src_tree_remove_state`: releases the state's source nodes.
pub fn pf_src_tree_remove_state(st: &'static PfState) {
    while let Some(sni) = st.src_nodes.first() {
        // SAFETY: the first item of the state's list, under pf_lock.
        unsafe { st.src_nodes.remove_head() };
        if st.src.tcp_est.get() != 0 {
            sni.sn().conn.set(sni.sn().conn.get().wrapping_sub(1));
        }
        let n = sni.sn().states.get().wrapping_sub(1);
        sni.sn().states.set(n);
        if n == 0 {
            let mut timeout = st.rule.ptr().map_or(0, |r| r.timeout(PFTM_SRC_NODE));
            if timeout == 0 {
                timeout = crate::net::pf_ioctl::PF_DEFAULT_RULE.timeout(PFTM_SRC_NODE);
            }
            sni.sn()
                .expire
                .set((getuptime() + i64::from(timeout)) as i32);
        }
        pf_pool_put(&PF_SN_ITEM_PL, sni);
    }
}

/// `pf_remove_state`: unlinks the state from the tables; `pf_free_state` frees it later.
pub fn pf_remove_state(st: &'static PfState) {
    pf_assert_locked();

    mtx_enter(&st.mtx);
    if usize::from(st.timeout.get()) == PFTM_UNLINKED {
        mtx_leave(&st.mtx);
        return;
    }
    st.timeout.set(PFTM_UNLINKED as u8);
    mtx_leave(&st.mtx);

    // Handle load balancing related tasks.
    crate::net::pf_lb::pf_postprocess_addr(st);

    let (Some(skw), Some(sks)) = (st.key[PF_SK_WIRE].get(), st.key[PF_SK_STACK].get()) else {
        return;
    };
    if st.src.state.get() == PF_TCPS_PROXY_DST {
        pf_send_tcp(
            st.rule.ptr(),
            skw.af.get(),
            &skw.addr[1].get(),
            &skw.addr[0].get(),
            skw.port[1].get(),
            skw.port[0].get(),
            st.src.seqhi.get(),
            st.src.seqlo.get().wrapping_add(1),
            TH_RST | TH_ACK,
            0,
            0,
            0,
            true,
            st.tag.get(),
            u32::from(skw.rdomain.get()),
            None,
        );
    }
    if i32::from(sks.proto.get()) == IPPROTO_TCP {
        pf_set_protostate(st, PF_PEER_BOTH, TCPS_CLOSED as u8);
    }

    while let Some(pfl) = st.linkage.first() {
        // SAFETY: the first link of the state's list, under the state lock.
        unsafe { st.linkage.remove_head() };

        let list: &TailqHead<PfStateLinkList> = match pfl.pfl_type.get() {
            PF_STATE_LINK_TYPE_STATELIM => {
                let stlim = pf_statelim_find(u32::from(st.statelim.get()));
                #[allow(clippy::panic)] // the C's KASSERTMSG
                let Some(stlim) = stlim else {
                    panic(format_args!(
                        "pf_state {:p} pfl {:p} cannot find statelim {}",
                        st,
                        pfl,
                        st.statelim.get()
                    ));
                };

                let generation = pf_statelim_enter(stlim);
                stlim
                    .pfstlim_inuse
                    .set(stlim.pfstlim_inuse.get().wrapping_sub(1));
                pf_statelim_leave(stlim, generation);

                &stlim.pfstlim_states
            }
            PF_STATE_LINK_TYPE_SOURCELIM => {
                let (sidx, kidx) = if st.direction.get() == PF_IN {
                    (0, PF_SK_WIRE)
                } else {
                    (1, PF_SK_STACK)
                };
                let Some(srlim) = pf_sourcelim_find(u32::from(st.sourcelim.get())) else {
                    panic(format_args!(
                        "pf_state {:p} pfl {:p} cannot find sourcelim {}",
                        st,
                        pfl,
                        st.sourcelim.get()
                    ));
                };

                let Some(k) = st.key[kidx].get() else {
                    panic(format_args!("pf_remove_state: state without key"));
                };
                let key = PfSource::zeroed();
                pf_source_key(
                    srlim,
                    &key,
                    k.af.get(),
                    u32::from(k.rdomain.get()),
                    &k.addr[sidx].get(),
                );

                let Some(sr) = pf_source_find(srlim, &key) else {
                    panic(format_args!(
                        "pf_state {:p} pfl {:p} cannot find source in {}",
                        st,
                        pfl,
                        st.sourcelim.get()
                    ));
                };

                let generation = pf_sourcelim_enter(srlim);
                let c = &srlim.pfsrlim_counters.inuse;
                c.set(c.get().wrapping_sub(1));
                pf_sourcelim_leave(srlim, generation);
                pf_source_rele(sr);

                &sr.pfsr_states
            }
            t => panic(format_args!(
                "pf_remove_state: unexpected link type {t} on pfl {pfl:p}"
            )),
        };

        pf_state_assert_locked();
        // SAFETY: the link is on the limiter's list (`pf_state_link_*` in pf_test_rule).
        unsafe { list.remove(pfl) };
        pf_pool_put(&PF_STATE_LINK_PL, pfl);
    }

    // SAFETY: an inserted state is in the id tree.
    unsafe { TREE_ID.remove(st) };
    if st.state_flags.get() & PFSTATE_PFLOW != 0 {
        let _ = crate::net::if_pflow::export_pflow(st);
    }
    crate::net::if_pfsync::pfsync_delete_state(st);
    pf_src_tree_remove_state(st);
    pf_detach_state(st);
}

/// `pf_remove_divert_state`: the socket of a divert-to/divert-reply state is closing.
pub fn pf_remove_divert_state(inp: &'static crate::netinet::in_pcb::Inpcb) {
    pf_assert_unlocked();

    if inpcb_pf_sk(inp).is_none() {
        return;
    }

    mtx_enter(&PF_INP_MTX);
    let sk = inpcb_pf_sk(inp).map(pf_state_key_ref);
    mtx_leave(&PF_INP_MTX);
    let Some(sk) = sk else {
        return;
    };

    pf_lock();
    pf_state_enter_write();
    for si in sk.sk_states.iter() {
        let Some(sist) = si.si_st.get() else {
            continue;
        };
        if opt_eq(Some(sk), sist.key[PF_SK_STACK].get())
            && let Some(r) = sist.rule.ptr()
            && (r.divert.type_ == PF_DIVERT_TO || r.divert.type_ == PF_DIVERT_REPLY)
        {
            if sist.key[PF_SK_STACK]
                .get()
                .is_some_and(|k| i32::from(k.proto.get()) == IPPROTO_TCP)
                && !opt_eq(sist.key[PF_SK_WIRE].get(), sist.key[PF_SK_STACK].get())
            {
                // If the local address is translated, keep the state for "tcp.closed"
                // seconds to prevent its source port from being reused.
                if i32::from(sist.src.state.get()) < TCPS_FIN_WAIT_2
                    || i32::from(sist.dst.state.get()) < TCPS_FIN_WAIT_2
                {
                    pf_set_protostate(sist, PF_PEER_BOTH, TCPS_TIME_WAIT as u8);
                    pf_update_state_timeout(sist, PFTM_TCP_CLOSED);
                    sist.expire.set(getuptime() as i32);
                }
                sist.state_flags
                    .set(sist.state_flags.get() | PFSTATE_INP_UNLINKED);
            } else {
                pf_remove_state(sist);
            }
            break;
        }
    }
    pf_state_exit_write();
    pf_unlock();

    pf_state_key_unref(Some(sk));
}

/// `pf_free_state`: frees an unlinked state's rule references and its last reference.
pub fn pf_free_state(st: &'static PfState) {
    pf_assert_locked();

    if crate::net::if_pfsync::pfsync_state_in_use(st) {
        return;
    }

    kassert!(usize::from(st.timeout.get()) == PFTM_UNLINKED);
    if let Some(r) = st.rule.ptr() {
        let n = r.states_cur.get().wrapping_sub(1);
        r.states_cur.set(n);
        if n == 0 && r.src_nodes.get() == 0 {
            crate::net::pf_ioctl::pf_rm_rule(None, r);
        }
    }
    if let Some(a) = st.anchor.ptr() {
        let n = a.states_cur.get().wrapping_sub(1);
        a.states_cur.set(n);
        if n == 0 {
            crate::net::pf_ioctl::pf_rm_rule(None, a);
        }
    }
    while let Some(ri) = st.match_rules.first() {
        // SAFETY: the first item of the state's list, under pf_lock.
        unsafe { st.match_rules.remove_head() };
        let n = ri.r().states_cur.get().wrapping_sub(1);
        ri.r().states_cur.set(n);
        if n == 0 && ri.r().src_nodes.get() == 0 {
            crate::net::pf_ioctl::pf_rm_rule(None, ri.r());
        }
        pf_pool_put(&PF_RULE_ITEM_PL, ri);
    }
    crate::net::pf_norm::pf_normalize_tcp_cleanup(st);
    pfi_kif_unref(st.kif.get(), PFI_KIF_REF_STATE);
    pf_state_list_remove(&PF_STATE_LIST, st);
    if st.tag.get() != 0 {
        crate::net::pf_ioctl::pf_tag_unref(st.tag.get());
    }
    pf_state_unref(Some(st));
    PF_STATUS_FCOUNTERS[FCNT_STATE_REMOVALS].fetch_add(1, AtomicOrdering::Relaxed);
    PF_STATUS.states.set(PF_STATUS.states.get().wrapping_sub(1));
}

/// The state `pf_purge_expired_states` stopped at (the C's function-local `static`): this
/// task is the only thing that removes states from the list, so the reference it holds
/// between calls is guaranteed to still be on it.
static PF_PURGE_CUR: PfGlobal<Cell<Option<&'static PfState>>> = PfGlobal(Cell::new(None));

/// The purge task's garbage list (the C's local `gcl`; only the purge task uses it, and it
/// is empty between runs).
static PF_PURGE_GCL: PfGlobal<SlistHead<PfStateGcList>> = PfGlobal(SlistHead::new());

/// `pf_purge_expired_states`: scans up to `limit` states, frees up to `collect` expired
/// ones; returns how many were scanned.
pub fn pf_purge_expired_states(limit: u32, collect: u32) -> u32 {
    let gcl: &'static SlistHead<PfStateGcList> = &PF_PURGE_GCL;
    let mut collected = 0;

    pf_assert_unlocked();

    rw_enter_read(&PF_STATE_LIST.pfs_rwl);

    mtx_enter(&PF_STATE_LIST.pfs_mtx);
    let head = PF_STATE_LIST.pfs_list.first();
    let tail = PF_STATE_LIST.pfs_list.last();
    mtx_leave(&PF_STATE_LIST.pfs_mtx);

    let Some(head) = head else {
        // The list is empty.
        rw_exit_read(&PF_STATE_LIST.pfs_rwl);
        return limit;
    };

    // (Re)start at the front of the list.
    let mut cur = PF_PURGE_CUR.get().unwrap_or(head);

    let now = getuptime();

    let mut scanned = 0;
    let mut wrapped = false;
    while scanned < limit {
        let stimeout = cur.timeout.get();
        let mut limited = false;

        if usize::from(stimeout) == PFTM_UNLINKED
            || i64::from(pf_state_expires(cur, stimeout)) <= now
        {
            let st = pf_state_ref(cur);
            // SAFETY: the purge task's own list; a state is on it at most once (one pass).
            unsafe { gcl.insert_head(st) };

            collected += 1;
            if collected >= collect {
                limited = true;
            }
        }

        // Don't iterate past the end of our view of the list.
        if tail.is_some_and(|t| ptr::eq(t, cur)) {
            PF_PURGE_CUR.set(None);
            wrapped = true;
            break;
        }
        let Some(next) = TailqHead::<PfStateQueue>::next(cur) else {
            PF_PURGE_CUR.set(None);
            wrapped = true;
            break;
        };
        cur = next;

        // Don't spend too much time here.
        if should_yield() || limited {
            break;
        }
        scanned += 1;
    }
    if !wrapped {
        PF_PURGE_CUR.set(Some(cur));
    }

    rw_exit_read(&PF_STATE_LIST.pfs_rwl);

    if gcl.is_empty() {
        return scanned;
    }

    rw_enter_write(&PF_STATE_LIST.pfs_rwl);
    pf_lock();
    pf_state_enter_write();
    for st in gcl.iter() {
        if usize::from(st.timeout.get()) != PFTM_UNLINKED {
            pf_remove_state(st);
        }

        pf_free_state(st);
    }
    pf_state_exit_write();
    pf_unlock();
    rw_exit_write(&PF_STATE_LIST.pfs_rwl);

    while let Some(st) = gcl.first() {
        // SAFETY: the purge task's list's first element.
        unsafe { gcl.remove_head() };
        pf_state_unref(Some(st));
    }

    scanned
}

/// `pf_tbladdr_setup`: attaches the table a `PF_ADDR_TABLE` operand names. `Err` is the C's 1.
pub fn pf_tbladdr_setup(rs: &'static PfRuleset, aw: &'static PfAddrWrap, wait: i32) -> bool {
    if aw.type_.get() != PF_ADDR_TABLE {
        return true;
    }
    let name = aw.v.get();
    let t = crate::net::pf_table::pfr_attach_table(rs, name.tblname(), wait);
    aw.set_tbl(t);
    if t.is_none() {
        return false;
    }
    true
}

/// `pf_tbladdr_remove`.
pub fn pf_tbladdr_remove(aw: &'static PfAddrWrap) {
    if aw.type_.get() != PF_ADDR_TABLE {
        return;
    }
    let Some(t) = aw.tbl() else {
        return;
    };
    crate::net::pf_table::pfr_detach_table(t);
    aw.set_tbl(None);
}

/// `pf_tbladdr_copyout`: replaces the kernel table pointer by its address count.
///
/// # Safety
///
/// `aw` is a kernel operand or a byte copy of one made under `pf_lock`, which is still held:
/// for `PF_ADDR_TABLE`, its `p` is null or the address of a live table.
pub unsafe fn pf_tbladdr_copyout(aw: &PfAddrWrap) {
    if aw.type_.get() != PF_ADDR_TABLE {
        return;
    }
    // SAFETY: the caller's contract.
    let Some(mut kt) = (unsafe { (aw.p.get() as *const PfrKtable).as_ref() }) else {
        return;
    };
    if kt.pfrkt_flags().get() & PFR_TFLAG_ACTIVE == 0
        && let Some(root) = kt.pfrkt_root.get()
    {
        kt = root;
    }
    aw.set_tbl(None);
    aw.set_cnt(if kt.pfrkt_flags().get() & PFR_TFLAG_ACTIVE != 0 {
        kt.pfrkt_cnt().get()
    } else {
        -1
    });
}

/// What `pf_print_host` prints: an address and port (network order, 0 for none) of family
/// `af`, IPv4 as `a.b.c.d:port`, IPv6 colon-separated with the longest run of zero words
/// compressed, then `[port]`.
pub struct PfHost<'a>(pub &'a PfAddr, pub u16, pub SaFamily);

impl core::fmt::Display for PfHost<'_> {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        let PfHost(addr, p, af) = *self;
        match af {
            AF_INET => {
                let a = u32::from_be(addr.addr32(0));
                write!(
                    f,
                    "{}.{}.{}.{}",
                    (a >> 24) & 255,
                    (a >> 16) & 255,
                    (a >> 8) & 255,
                    a & 255
                )?;
                if p != 0 {
                    write!(f, ":{}", u16::from_be(p))?;
                }
            }
            #[cfg(feature = "inet6")]
            AF_INET6 => {
                // The longest run of zero words; 255 is "none" and the differences are
                // taken as ints, as the C's promoted u_int8_t arithmetic.
                let (mut curstart, mut curend, mut maxstart, mut maxend) =
                    (255i32, 255i32, 255i32, 255i32);
                for i in 0..8 {
                    if addr.addr16(i) == 0 {
                        if curstart == 255 {
                            curstart = i as i32;
                        }
                        curend = i as i32;
                    } else {
                        if curend - curstart > maxend - maxstart {
                            maxstart = curstart;
                            maxend = curend;
                        }
                        curstart = 255;
                        curend = 255;
                    }
                }
                if curend - curstart > maxend - maxstart {
                    maxstart = curstart;
                    maxend = curend;
                }
                for i in 0..8i32 {
                    if i >= maxstart && i <= maxend {
                        if i == 0 {
                            f.write_str(":")?;
                        }
                        if i == maxend {
                            f.write_str(":")?;
                        }
                    } else {
                        write!(f, "{:x}", u16::from_be(addr.addr16(i as usize)))?;
                        if i < 7 {
                            f.write_str(":")?;
                        }
                    }
                }
                if p != 0 {
                    write!(f, "[{}]", u16::from_be(p))?;
                }
            }
            _ => {}
        }
        Ok(())
    }
}

/// `pf_print_host`: logs an address and port (network order, 0 for none).
pub fn pf_print_host(addr: &PfAddr, p: u16, af: SaFamily) {
    addlog(format_args!("{}", PfHost(addr, p, af)));
}

/// `pf_print_state`.
pub fn pf_print_state(st: &'static PfState) {
    pf_print_state_parts(Some(st), None, None);
}

/// `pf_print_state_parts`: logs a state, or its keys, as far as they are given.
pub fn pf_print_state_parts(
    st: Option<&'static PfState>,
    skwp: Option<&PfStateKey>,
    sksp: Option<&PfStateKey>,
) {
    pf_print_state_parts_key(st, skwp, sksp);
}

/// `pf_print_state_parts` over keys that need not be pool items (the lookup key of
/// `pf_find_state`).
fn pf_print_state_parts_key(
    st: Option<&'static PfState>,
    skwp: Option<&PfStateKey>,
    sksp: Option<&PfStateKey>,
) {
    use crate::netinet::in_::{IPPROTO_ICMPV6, IPPROTO_IPV4, IPPROTO_IPV6};

    // Do our best to fill these, but they're skipped if NULL.
    let skw: Option<&PfStateKey> = skwp.or_else(|| st.and_then(|s| s.key[PF_SK_WIRE].get()));
    let sks: Option<&PfStateKey> = sksp.or_else(|| st.and_then(|s| s.key[PF_SK_STACK].get()));
    let proto = skw.map_or_else(|| sks.map_or(0, |k| k.proto.get()), |k| k.proto.get());
    let dir = st.map_or(0, |s| s.direction.get());

    match i32::from(proto) {
        IPPROTO_IPV4 => addlog(format_args!("IPv4")),
        IPPROTO_IPV6 => addlog(format_args!("IPv6")),
        IPPROTO_TCP => addlog(format_args!("TCP")),
        IPPROTO_UDP => addlog(format_args!("UDP")),
        IPPROTO_ICMP => addlog(format_args!("ICMP")),
        IPPROTO_ICMPV6 => addlog(format_args!("ICMPv6")),
        _ => addlog(format_args!("{proto}")),
    }
    match dir {
        PF_IN => addlog(format_args!(" in")),
        PF_OUT => addlog(format_args!(" out")),
        _ => {}
    }
    if let Some(skw) = skw {
        addlog(format_args!(" wire: ({}) ", skw.rdomain.get()));
        pf_print_host(&skw.addr[0].get(), skw.port[0].get(), skw.af.get());
        addlog(format_args!(" "));
        pf_print_host(&skw.addr[1].get(), skw.port[1].get(), skw.af.get());
    }
    if let Some(sks) = sks {
        addlog(format_args!(" stack: ({}) ", sks.rdomain.get()));
        if skw.is_none_or(|w| !ptr::eq(w, sks)) {
            pf_print_host(&sks.addr[0].get(), sks.port[0].get(), sks.af.get());
            addlog(format_args!(" "));
            pf_print_host(&sks.addr[1].get(), sks.port[1].get(), sks.af.get());
        } else {
            addlog(format_args!("-"));
        }
    }
    if let Some(st) = st {
        if i32::from(proto) == IPPROTO_TCP {
            for p in [&st.src, &st.dst] {
                addlog(format_args!(
                    " [lo={} high={} win={} modulator={}",
                    p.seqlo.get(),
                    p.seqhi.get(),
                    p.max_win.get(),
                    p.seqdiff.get()
                ));
                if st.src.wscale.get() != 0 && st.dst.wscale.get() != 0 {
                    addlog(format_args!(" wscale={}", p.wscale.get() & PF_WSCALE_MASK));
                }
                addlog(format_args!("]"));
            }
        }
        addlog(format_args!(
            " {}:{}",
            st.src.state.get(),
            st.dst.state.get()
        ));
        if let Some(r) = st.rule.ptr() {
            addlog(format_args!(" @{}", r.nr.get() as i32));
        }
    }
}

/// `pf_print_flags`.
pub fn pf_print_flags(f: u8) {
    if f != 0 {
        addlog(format_args!(" "));
    }
    for (bit, c) in [
        (TH_FIN, "F"),
        (TH_SYN, "S"),
        (TH_RST, "R"),
        (TH_PUSH, "P"),
        (TH_ACK, "A"),
        (TH_URG, "U"),
        (TH_ECE, "E"),
        (TH_CWR, "W"),
    ] {
        if f & bit != 0 {
            addlog(format_args!("{c}"));
        }
    }
}

/// `pf_calc_skip_steps`: for each criterion, points every rule at the next rule that differs
/// in it, so that evaluation can skip the rules that would fail the same way.
pub fn pf_calc_skip_steps(rules: &'static TailqHead<PfRulequeue>) {
    let Some(first) = rules.first() else {
        return;
    };
    let mut head: [Option<&'static PfRule>; PF_SKIP_COUNT] = [Some(first); PF_SKIP_COUNT];

    // PF_SET_SKIP_STEPS(i): every rule from head[i] up to cur skips to cur.
    let set_skip_steps = |head: &mut [Option<&'static PfRule>; PF_SKIP_COUNT],
                          i: usize,
                          cur: Option<&'static PfRule>| {
        while let Some(h) = head[i] {
            if cur.is_some_and(|c| ptr::eq(c, h)) {
                break;
            }
            h.skip[i].set_ptr(cur);
            head[i] = TailqHead::<PfRulequeue>::next(h);
        }
    };

    let mut prev = first;
    let mut cur = Some(first);
    while let Some(c) = cur {
        if c.kif.get() != prev.kif.get() || c.ifnot != prev.ifnot {
            set_skip_steps(&mut head, PF_SKIP_IFP, cur);
        }
        if c.direction != prev.direction {
            set_skip_steps(&mut head, PF_SKIP_DIR, cur);
        }
        if c.onrdomain != prev.onrdomain || c.ifnot != prev.ifnot {
            set_skip_steps(&mut head, PF_SKIP_RDOM, cur);
        }
        if c.af != prev.af {
            set_skip_steps(&mut head, PF_SKIP_AF, cur);
        }
        if c.proto != prev.proto {
            set_skip_steps(&mut head, PF_SKIP_PROTO, cur);
        }
        if c.src.neg.get() != prev.src.neg.get() || pf_addr_wrap_neq(&c.src.addr, &prev.src.addr) {
            set_skip_steps(&mut head, PF_SKIP_SRC_ADDR, cur);
        }
        if c.dst.neg.get() != prev.dst.neg.get() || pf_addr_wrap_neq(&c.dst.addr, &prev.dst.addr) {
            set_skip_steps(&mut head, PF_SKIP_DST_ADDR, cur);
        }
        if c.src.port[0].get() != prev.src.port[0].get()
            || c.src.port[1].get() != prev.src.port[1].get()
            || c.src.port_op.get() != prev.src.port_op.get()
        {
            set_skip_steps(&mut head, PF_SKIP_SRC_PORT, cur);
        }
        if c.dst.port[0].get() != prev.dst.port[0].get()
            || c.dst.port[1].get() != prev.dst.port[1].get()
            || c.dst.port_op.get() != prev.dst.port_op.get()
        {
            set_skip_steps(&mut head, PF_SKIP_DST_PORT, cur);
        }

        prev = c;
        cur = TailqHead::<PfRulequeue>::next(c);
    }
    for i in 0..PF_SKIP_COUNT {
        set_skip_steps(&mut head, i, None);
    }
}

/// `pf_addr_wrap_neq`: the operands differ.
pub fn pf_addr_wrap_neq(aw1: &'static PfAddrWrap, aw2: &'static PfAddrWrap) -> bool {
    if aw1.type_.get() != aw2.type_.get() {
        return true;
    }
    let (v1, v2) = (aw1.v.get(), aw2.v.get());
    match aw1.type_.get() {
        PF_ADDR_ADDRMASK | PF_ADDR_RANGE => {
            pf_aneq(&v1.addr(), &v2.addr(), AF_INET6) || pf_aneq(&v1.mask(), &v2.mask(), AF_INET6)
        }
        PF_ADDR_DYNIFTL => !opt_eq(
            aw1.dyn_().and_then(|d| d.pfid_kt.get()),
            aw2.dyn_().and_then(|d| d.pfid_kt.get()),
        ),
        PF_ADDR_NONE | PF_ADDR_NOROUTE | PF_ADDR_URPFFAILED => false,
        PF_ADDR_TABLE => aw1.p.get() != aw2.p.get(),
        PF_ADDR_RTLABEL => v1.rtlabel() != v2.rtlabel(),
        t => {
            addlog(format_args!("invalid address type: {t}\n"));
            true
        }
    }
}

/// `pf_cksum_fixup`: updates a ones-complement checksum for a 16-bit word changing from `was`
/// to `now`. Computes `a + b - c` in ones complement with at most one borrow, so one
/// reduction step is enough (see the C's derivation): `(x + (x >> 16)) & 0xffff`. A zero UDP
/// checksum means "none" and is left alone; a computed zero is sent as `0xffff`.
pub fn pf_cksum_fixup(cksum: &mut u16, was: u16, now: u16, proto: u8) {
    let udp = i32::from(proto) == IPPROTO_UDP;

    let mut x: u32 = u32::from(*cksum)
        .wrapping_add(u32::from(was))
        .wrapping_sub(u32::from(now));
    x = (x.wrapping_add(x >> 16)) & 0xffff;

    // Optimise: eliminate a branch when not udp.
    if udp && *cksum == 0x0000 {
        return;
    }
    if udp && x == 0x0000 {
        x = 0xffff;
    }

    *cksum = x as u16;
}

/// `pf_cksum_fixup` of the checksum at `pd->pcksum`.
pub fn pf_cksum_fixup_pd(pd: &mut PfPdesc, was: u16, now: u16, proto: u8) {
    let pc = pd.pcksum;
    let mut c = pd.ld16(pc);
    pf_cksum_fixup(&mut c, was, now, proto);
    pd.st16(pc, c);
}

/// `pf_cksum_uncover`: takes out of `cksum` the coverage of `covered_cksum` (pre: the
/// coverage of `cksum` is a superset of it).
#[cfg(feature = "inet6")]
fn pf_cksum_uncover(cksum: &mut u16, covered_cksum: u16, proto: u8) {
    pf_cksum_fixup(cksum, !covered_cksum, 0x0, proto);
}

/// `pf_cksum_cover`: adds to `cksum` the coverage of `uncovered_cksum` (pre: their
/// coverages are disjoint).
#[cfg(feature = "inet6")]
fn pf_cksum_cover(cksum: &mut u16, uncovered_cksum: u16, proto: u8) {
    pf_cksum_fixup(cksum, 0x0, !uncovered_cksum, proto);
}

/// `NEG(x)`.
const fn neg(x: u16) -> u16 {
    !x
}

/// `pf_cksum_fixup_a`: updates a checksum for an address changing from `a` to `an`. Emulates
/// 16-bit ones-complement sums by keeping the carries in the upper bits and folding them in
/// twice; the result does not depend on the host's byte order (see the C's proof).
pub fn pf_cksum_fixup_a(cksum: &mut u16, a: &PfAddr, an: &PfAddr, af: SaFamily, proto: u8) {
    let udp = i32::from(proto) == IPPROTO_UDP;

    let mut x: u32 = match af {
        AF_INET => {
            u32::from(*cksum)
                + u32::from(a.addr16(0))
                + u32::from(neg(an.addr16(0)))
                + u32::from(a.addr16(1))
                + u32::from(neg(an.addr16(1)))
        }
        #[cfg(feature = "inet6")]
        AF_INET6 => (0..8).fold(u32::from(*cksum), |x, i| {
            x + u32::from(a.addr16(i)) + u32::from(neg(an.addr16(i)))
        }),
        _ => unhandled_af(i32::from(af)),
    };

    x = (x & 0xffff) + (x >> 16);
    x = (x & 0xffff) + (x >> 16);

    // Optimise: eliminate a branch when not udp.
    if udp && *cksum == 0x0000 {
        return;
    }
    if udp && x == 0x0000 {
        x = 0xffff;
    }

    *cksum = x as u16;
}

/// `pf_cksum_fixup_a` of the checksum at `pd->pcksum`.
pub fn pf_cksum_fixup_a_pd(pd: &mut PfPdesc, a: &PfAddr, an: &PfAddr, af: SaFamily, proto: u8) {
    let pc = pd.pcksum;
    let mut c = pd.ld16(pc);
    pf_cksum_fixup_a(&mut c, a, an, af, proto);
    pd.st16(pc, c);
}

/// `PF_HI`.
pub const PF_HI: bool = true;
/// `PF_LO`.
pub const PF_LO: bool = !PF_HI;

/// `PF_ALGNMNT(off)`: the byte's position in its 16-bit checksum word.
pub const fn pf_algnmnt(off: usize) -> bool {
    off.is_multiple_of(2)
}

/// `pf_patch_8`: sets the byte at `f` to `v`, fixing the checksum; `hi` says it is the high
/// byte of its 16-bit word. Returns 1 when it rewrote.
pub fn pf_patch_8(pd: &mut PfPdesc, f: PfLoc, v: u8, hi: bool) -> i32 {
    let cur = pd.ld8(f);
    if cur != v {
        let old = if hi {
            u16::from(cur) << 8
        } else {
            u16::from(cur)
        }
        .to_be();
        let new = if hi { u16::from(v) << 8 } else { u16::from(v) }.to_be();

        let proto = pd.proto;
        pf_cksum_fixup_pd(pd, old, new, proto);
        pd.st8(f, v);
        return 1;
    }
    0
}

/// `pf_patch_16`: sets the 16-bit word at `f` (16-bit aligned within its packet) to `v`.
pub fn pf_patch_16(pd: &mut PfPdesc, f: PfLoc, v: u16) -> i32 {
    let cur = pd.ld16(f);
    if cur != v {
        let proto = pd.proto;
        pf_cksum_fixup_pd(pd, cur, v, proto);
        pd.st16(f, v);
        return 1;
    }
    0
}

/// `pf_patch_16_unaligned`: as `pf_patch_16` for a word at any position; `hi` says where its
/// first byte falls in the checksum's words.
pub fn pf_patch_16_unaligned(pd: &mut PfPdesc, f: PfLoc, v: u16, hi: bool) -> i32 {
    if hi && pd.loc_aligned(f, 2) {
        return pf_patch_16(pd, f, v); // optimise
    }

    let vb = v.to_ne_bytes();
    let mut rewrite = 0;
    rewrite += pf_patch_8(pd, f, vb[0], hi);
    rewrite += pf_patch_8(pd, f.offset(1), vb[1], !hi);
    rewrite
}

/// `pf_patch_32`: sets the 32-bit word at `f` (16-bit aligned) to `v`; never for UDP.
pub fn pf_patch_32(pd: &mut PfPdesc, f: PfLoc, v: u32) -> i32 {
    let proto = pd.proto;

    // Optimise: inline udp fixup code is unused; let compiler scrub it.
    if i32::from(proto) == IPPROTO_UDP {
        panic(format_args!("pf_patch_32: udp"));
    }

    // Optimise: skip *f != v guard; true for all use-cases.
    let cur = pd.ld32(f);
    pf_cksum_fixup_pd(pd, (cur / (1 << 16)) as u16, (v / (1 << 16)) as u16, proto);
    pf_cksum_fixup_pd(pd, (cur % (1 << 16)) as u16, (v % (1 << 16)) as u16, proto);

    pd.st32(f, v);
    1
}

/// `pf_patch_32_unaligned`.
pub fn pf_patch_32_unaligned(pd: &mut PfPdesc, f: PfLoc, v: u32, hi: bool) -> i32 {
    if hi && pd.loc_aligned(f, 4) {
        return pf_patch_32(pd, f, v); // optimise
    }

    let vb = v.to_ne_bytes();
    let mut rewrite = 0;
    rewrite += pf_patch_8(pd, f, vb[0], hi);
    rewrite += pf_patch_8(pd, f.offset(1), vb[1], !hi);
    rewrite += pf_patch_8(pd, f.offset(2), vb[2], hi);
    rewrite += pf_patch_8(pd, f.offset(3), vb[3], !hi);
    rewrite
}

/// `pf_icmp_mapping`: the direction, virtual id and virtual type (network order) an ICMP
/// message is tracked by. ICMP types marked `PF_OUT` are typically responses to `PF_IN`
/// and match states in the opposite direction. Returns `true` for the error types, which
/// match the state of the connection they quote.
pub fn pf_icmp_mapping(
    pd: &PfPdesc,
    type_: u8,
    icmp_dir: &mut i32,
    virtual_id: &mut u16,
    virtual_type: &mut u16,
) -> bool {
    use crate::netinet::ip_icmp::*;

    *icmp_dir = i32::from(PF_OUT);

    // Queries (and responses).
    if pd.af == AF_INET {
        match type_ {
            ICMP_ECHO | ICMP_ECHOREPLY => {
                if type_ == ICMP_ECHO {
                    *icmp_dir = i32::from(PF_IN);
                }
                *virtual_type = u16::from(ICMP_ECHO);
                *virtual_id = pd.icmp().icmp_id();
            }
            ICMP_TSTAMP | ICMP_TSTAMPREPLY => {
                if type_ == ICMP_TSTAMP {
                    *icmp_dir = i32::from(PF_IN);
                }
                *virtual_type = u16::from(ICMP_TSTAMP);
                *virtual_id = pd.icmp().icmp_id();
            }
            ICMP_IREQ | ICMP_IREQREPLY => {
                if type_ == ICMP_IREQ {
                    *icmp_dir = i32::from(PF_IN);
                }
                *virtual_type = u16::from(ICMP_IREQ);
                *virtual_id = pd.icmp().icmp_id();
            }
            ICMP_MASKREQ | ICMP_MASKREPLY => {
                if type_ == ICMP_MASKREQ {
                    *icmp_dir = i32::from(PF_IN);
                }
                *virtual_type = u16::from(ICMP_MASKREQ);
                *virtual_id = pd.icmp().icmp_id();
            }
            ICMP_IPV6_WHEREAREYOU | ICMP_IPV6_IAMHERE => {
                if type_ == ICMP_IPV6_WHEREAREYOU {
                    *icmp_dir = i32::from(PF_IN);
                }
                *virtual_type = u16::from(ICMP_IPV6_WHEREAREYOU);
                *virtual_id = 0; // Nothing sane to match on!
            }
            ICMP_MOBILE_REGREQUEST | ICMP_MOBILE_REGREPLY => {
                if type_ == ICMP_MOBILE_REGREQUEST {
                    *icmp_dir = i32::from(PF_IN);
                }
                *virtual_type = u16::from(ICMP_MOBILE_REGREQUEST);
                *virtual_id = 0; // Nothing sane to match on!
            }
            ICMP_ROUTERSOLICIT | ICMP_ROUTERADVERT => {
                if type_ == ICMP_ROUTERSOLICIT {
                    *icmp_dir = i32::from(PF_IN);
                }
                *virtual_type = u16::from(ICMP_ROUTERSOLICIT);
                *virtual_id = 0; // Nothing sane to match on!
            }
            // These ICMP types map to other connections.
            ICMP_UNREACH | ICMP_SOURCEQUENCH | ICMP_REDIRECT | ICMP_TIMXCEED | ICMP_PARAMPROB => {
                // These will not be used, but set them anyway.
                *icmp_dir = i32::from(PF_IN);
                *virtual_type = u16::from(type_).to_be();
                *virtual_id = 0;
                return true; // These types match to another state.
            }
            // All remaining ICMP types get their own states, and will only match in one
            // direction.
            _ => {
                *icmp_dir = i32::from(PF_IN);
                *virtual_type = u16::from(type_);
                *virtual_id = 0;
            }
        }
    }
    #[cfg(feature = "inet6")]
    if pd.af == AF_INET6 {
        use crate::netinet::icmp6::*;
        // A fake id for MLD and neighbour discovery: the address words folded to 16 bits.
        let fold = |a: &crate::netinet6::in6::In6Addr| {
            let h = a.s6_addr32(0) ^ a.s6_addr32(1) ^ a.s6_addr32(2) ^ a.s6_addr32(3);
            ((h >> 16) ^ (h & 0xffff)) as u16
        };
        match type_ {
            ICMP6_ECHO_REQUEST | ICMP6_ECHO_REPLY => {
                if type_ == ICMP6_ECHO_REQUEST {
                    *icmp_dir = i32::from(PF_IN);
                }
                *virtual_type = u16::from(ICMP6_ECHO_REQUEST);
                *virtual_id = pd.icmp6().icmp6_id();
            }
            MLD_LISTENER_QUERY | MLD_LISTENER_REPORT => {
                // Listener Report can be sent by clients without an associated Listener
                // Query. In addition to that, when Report is sent as a reply to a Query
                // its source and destination address are different.
                *icmp_dir = i32::from(PF_IN);
                *virtual_type = u16::from(MLD_LISTENER_QUERY);
                *virtual_id = fold(&pd.mld().mld_addr);
            }
            // ICMP6_FQDN and ICMP6_NI query/reply are the same type as ICMP6_WRU.
            ICMP6_WRUREQUEST | ICMP6_WRUREPLY => {
                if type_ == ICMP6_WRUREQUEST {
                    *icmp_dir = i32::from(PF_IN);
                }
                *virtual_type = u16::from(ICMP6_WRUREQUEST);
                *virtual_id = 0; // Nothing sane to match on!
            }
            MLD_MTRACE | MLD_MTRACE_RESP => {
                if type_ == MLD_MTRACE {
                    *icmp_dir = i32::from(PF_IN);
                }
                *virtual_type = u16::from(MLD_MTRACE);
                *virtual_id = 0; // Nothing sane to match on!
            }
            ND_NEIGHBOR_SOLICIT | ND_NEIGHBOR_ADVERT => {
                if type_ == ND_NEIGHBOR_SOLICIT {
                    *icmp_dir = i32::from(PF_IN);
                }
                *virtual_type = u16::from(ND_NEIGHBOR_SOLICIT);
                *virtual_id = fold(&pd.nd_ns().nd_ns_target);
                // The extra work here deals with the 'keep state' option of a pass rule
                // for an unsolicited advertisement: returning true (state_icmp) overrides
                // 'keep state' with 'no state', so no state is created for unsolicited
                // advertisements. No one expects an answer to them.
                if type_ == ND_NEIGHBOR_ADVERT {
                    *virtual_type = virtual_type.to_be();
                    return true;
                }
            }
            // These ICMP types map to other connections. ND_REDIRECT can't be in this list
            // because the triggering packet header is optional.
            ICMP6_DST_UNREACH | ICMP6_PACKET_TOO_BIG | ICMP6_TIME_EXCEEDED | ICMP6_PARAM_PROB => {
                // These will not be used, but set them anyway.
                *icmp_dir = i32::from(PF_IN);
                *virtual_type = u16::from(type_).to_be();
                *virtual_id = 0;
                return true; // These types match to another state.
            }
            // All remaining ICMP6 types get their own states, and will only match in one
            // direction.
            _ => {
                *icmp_dir = i32::from(PF_IN);
                *virtual_type = u16::from(type_);
                *virtual_id = 0;
            }
        }
    }
    *virtual_type = virtual_type.to_be();
    false // These types match to their own state.
}

/// `pf_translate_icmp`: rewrites the address and port quoted inside an ICMP error (`qa`,
/// `qp`) and, if `oa` is given, the outer header's address. Quoted checksums are not fixed.
pub fn pf_translate_icmp(pd: &mut PfPdesc, qa: PfLoc, qp: PfLoc, oa: PfLoc, na: &PfAddr, np: u16) {
    // Change quoted protocol port.
    if !qp.is_none() {
        pf_patch_16(pd, qp, np);
    }

    // Change quoted ip address.
    let af = pd.af;
    let proto = pd.proto;
    let old = pd.ld_addr(qa);
    pf_cksum_fixup_a_pd(pd, &old, na, af, proto);
    pd.st_addr(qa, na, af);

    // Change network-header's ip address.
    if !oa.is_none() {
        pf_translate_a(pd, oa, na);
    }
}

/// `pf_translate_a`: rewrites the network header address at `a` (16-bit aligned) to `an`,
/// fixing the transport pseudo-header checksum. Returns 1 when it rewrote.
pub fn pf_translate_a(pd: &mut PfPdesc, a: PfLoc, an: &PfAddr) -> i32 {
    let af = pd.af;
    let cur = pd.ld_addr(a);

    // Warning: !PF_ANEQ != PF_AEQ.
    if !pf_aneq(&cur, an, af) {
        return 0;
    }

    // Fixup transport pseudo-header, if any.
    match i32::from(pd.proto) {
        IPPROTO_TCP | IPPROTO_UDP | crate::netinet::in_::IPPROTO_ICMPV6 => {
            let proto = pd.proto;
            pf_cksum_fixup_a_pd(pd, &cur, an, af, proto);
        }
        _ => {} // assume no pseudo-header
    }

    pd.st_addr(a, an, af);
    1
}

/// `pf_translate_af`: replaces the network header with one of the other family (`pd->naf`,
/// the addresses `pd->nsaddr`/`pd->ndaddr`) and moves the transport checksum from the old
/// pseudo-header to the new one. `pd->m` may change; 0, or -1 when the packet was lost
/// (`pd->m` is then `None`).
#[cfg(feature = "inet6")]
pub fn pf_translate_af(pd: &mut PfPdesc) -> i32 {
    use crate::netinet::ip::{IP_DF, IPVERSION, Ip};
    use crate::netinet::ip6::{IPV6_DEFHLIM, IPV6_VERSION};
    use crate::sys::mbuf::M_UDP_CSUM_OUT;

    let zero = PfAddr::zeroed();
    let hlen = if pd.naf == AF_INET {
        size_of::<Ip>() as u32
    } else {
        size_of::<Ip6Hdr>() as u32
    };
    let ohlen = pd.off;
    let dlen = (pd.tot_len - u64::from(pd.off)) as u16;
    let mut copyback = false;

    let naf_proto = pd.proto;
    let af_proto = match i32::from(naf_proto) {
        IPPROTO_ICMP => IPPROTO_ICMPV6 as u8,
        IPPROTO_ICMPV6 => IPPROTO_ICMP as u8,
        _ => naf_proto,
    };

    // Uncover the stale pseudo-header.
    let p = i32::from(af_proto);
    if p == IPPROTO_ICMPV6 || p == IPPROTO_UDP || p == IPPROTO_TCP {
        if p == IPPROTO_ICMPV6 {
            // Optimise: unchanged for TCP/UDP.
            pf_cksum_fixup_pd(pd, u16::from(af_proto).to_be(), 0x0, af_proto);
            pf_cksum_fixup_pd(pd, dlen.to_be(), 0x0, af_proto);
        }
        let (src, dst, af) = (pd.ld_addr(pd.src), pd.ld_addr(pd.dst), pd.af);
        pf_cksum_fixup_a_pd(pd, &src, &zero, af, af_proto);
        pf_cksum_fixup_a_pd(pd, &dst, &zero, af, af_proto);
        copyback = true;
    } // else assume no pseudo-header

    // Replace the network header.
    let Some(m) = pd.m else {
        return -1;
    };
    crate::kern::uipc_mbuf::m_adj(m, pd.off as i32);
    pd.src = PfLoc::None;
    pd.dst = PfLoc::None;

    let Some(m) = crate::kern::uipc_mbuf::m_prepend(m, hlen as i32, M_DONTWAIT) else {
        pd.m = None;
        return -1;
    };
    pd.m = Some(m);

    pd.off = hlen;
    pd.tot_len = pd.tot_len + u64::from(hlen) - u64::from(ohlen);

    match pd.naf {
        AF_INET => {
            let mut ip4 = Ip::default();
            ip4.set_ip_v(IPVERSION);
            ip4.set_ip_hl((hlen >> 2) as u8);
            ip4.ip_tos = pd.tos;
            ip4.ip_len = (hlen as u16).wrapping_add(dlen).to_be();
            ip4.ip_id = crate::netinet::ip_id::ip_randomid().to_be();
            ip4.ip_off = IP_DF.to_be();
            ip4.ip_ttl = pd.ttl;
            ip4.ip_p = pd.proto;
            ip4.ip_src = pd.nsaddr.v4();
            ip4.ip_dst = pd.ndaddr.v4();
            crate::netinet::ip_var::mtod_ip_store(m, &ip4);
        }
        AF_INET6 => {
            let mut ip6 = Ip6Hdr::zeroed();
            ip6.set_ip6_vfc(IPV6_VERSION);
            ip6.ip6_flow |= (u32::from(pd.tos) << 20).to_be();
            ip6.ip6_plen = dlen.to_be();
            ip6.ip6_nxt = pd.proto;
            ip6.ip6_hlim = if pd.ttl == 0 || pd.ttl > IPV6_DEFHLIM {
                IPV6_DEFHLIM
            } else {
                pd.ttl
            };
            ip6.ip6_src = pd.nsaddr.v6();
            ip6.ip6_dst = pd.ndaddr.v6();
            crate::netinet6::ip6_var::mtod_ip6_store(m, &ip6);
        }
        _ => unhandled_af(i32::from(pd.naf)),
    }

    // UDP over IPv6 must be checksummed per rfc2460 p27.
    let pc = pd.pcksum;
    if i32::from(naf_proto) == IPPROTO_UDP && pd.ld16(pc) == 0x0000 && pd.naf == AF_INET6 {
        let cf = &m.m_pkthdr().csum_flags;
        cf.set(cf.get() | M_UDP_CSUM_OUT);
    }

    // Cover the fresh pseudo-header.
    let p = i32::from(naf_proto);
    if p == IPPROTO_ICMPV6 || p == IPPROTO_UDP || p == IPPROTO_TCP {
        if p == IPPROTO_ICMPV6 {
            // Optimise: unchanged for TCP/UDP.
            pf_cksum_fixup_pd(pd, 0x0, u16::from(naf_proto).to_be(), naf_proto);
            pf_cksum_fixup_pd(pd, 0x0, dlen.to_be(), naf_proto);
        }
        let (ns, nd, naf) = (pd.nsaddr, pd.ndaddr, pd.naf);
        pf_cksum_fixup_a_pd(pd, &zero, &ns, naf, naf_proto);
        pf_cksum_fixup_a_pd(pd, &zero, &nd, naf, naf_proto);
        copyback = true;
    } // else assume no pseudo-header

    // Flush pd->pcksum.
    if copyback {
        pf_copyback_hdr(pd);
    }

    0
}

/// `pf_change_icmp_af`: replaces the IP header quoted by an ICMP error (at `ipoff2` in `m`,
/// described by `pd2`) with one of family `naf` carrying `src` and `dst`, keeping the outer
/// ICMP checksum (`pd->pcksum`) right. The outer network header is left for
/// `pf_translate_af`. 0, or -1 on failure.
#[cfg(feature = "inet6")]
#[allow(clippy::too_many_arguments)]
pub fn pf_change_icmp_af(
    m: &'static Mbuf,
    ipoff2: i32,
    pd: &mut PfPdesc,
    pd2: &mut PfPdesc,
    src: &PfAddr,
    dst: &PfAddr,
    af: SaFamily,
    naf: SaFamily,
) -> i32 {
    use crate::kern::uipc_mbuf::{m_adj, m_cat, m_prepend, m_split};
    use crate::netinet::in_cksum::in_cksum;
    use crate::netinet::ip::{IP_DF, IPVERSION, Ip};
    use crate::netinet::ip6::{IPV6_DEFHLIM, IPV6_VERSION};

    if af == naf || (af != AF_INET && af != AF_INET6) || (naf != AF_INET && naf != AF_INET6) {
        return -1;
    }

    // Split the mbuf chain on the quoted ip/ip6 header boundary.
    let Some(n) = m_split(m, ipoff2, M_DONTWAIT) else {
        return -1;
    };

    // New quoted header.
    let hlen = if naf == AF_INET {
        size_of::<Ip>() as i32
    } else {
        size_of::<Ip6Hdr>() as i32
    };
    // Old quoted header.
    let ohlen = pd2.off as i32 - ipoff2;

    // Trim the old quoted header.
    let pc = pd.pcksum;
    let proto = pd.proto;
    let mut c = pd.ld16(pc);
    pf_cksum_uncover(&mut c, in_cksum(n, ohlen), proto);
    pd.st16(pc, c);
    m_adj(n, ohlen);

    // Prepend a new, translated, quoted header.
    let Some(n) = m_prepend(n, hlen, M_DONTWAIT) else {
        return -1;
    };

    let qlen = pd2.tot_len as i64 - i64::from(ohlen);
    if naf == AF_INET {
        let mut ip4 = Ip::default();
        ip4.set_ip_v(IPVERSION);
        ip4.set_ip_hl((size_of::<Ip>() >> 2) as u8);
        ip4.ip_len = ((size_of::<Ip>() as i64 + qlen) as u16).to_be();
        ip4.ip_id = crate::netinet::ip_id::ip_randomid().to_be();
        ip4.ip_off = IP_DF.to_be();
        ip4.ip_ttl = pd2.ttl;
        ip4.ip_p = if i32::from(pd2.proto) == IPPROTO_ICMPV6 {
            IPPROTO_ICMP as u8
        } else {
            pd2.proto
        };
        ip4.ip_src = src.v4();
        ip4.ip_dst = dst.v4();
        crate::netinet::ip_var::mtod_ip_store(n, &ip4);
        crate::netinet::ip_output::in_hdr_cksum_out(n, None);
    } else {
        let mut ip6 = Ip6Hdr::zeroed();
        ip6.set_ip6_vfc(IPV6_VERSION);
        ip6.ip6_plen = (qlen as u16).to_be();
        ip6.ip6_nxt = if i32::from(pd2.proto) == IPPROTO_ICMP {
            IPPROTO_ICMPV6 as u8
        } else {
            pd2.proto
        };
        ip6.ip6_hlim = if pd2.ttl == 0 || pd2.ttl > IPV6_DEFHLIM {
            IPV6_DEFHLIM
        } else {
            pd2.ttl
        };
        ip6.ip6_src = src.v6();
        ip6.ip6_dst = dst.v6();
        crate::netinet6::ip6_var::mtod_ip6_store(n, &ip6);
    }

    // Cover the new quoted header (optimise: any new AF_INET header of ours sums to zero).
    if naf != AF_INET {
        let mut c = pd.ld16(pc);
        pf_cksum_cover(&mut c, in_cksum(n, hlen), proto);
        pd.st16(pc, c);
    }

    // Reattach the modified quoted packet to the outer header.
    let nlen = n.m_pkthdr().len.get();
    m_cat(m, Some(n));
    let ml = &m.m_pkthdr().len;
    ml.set(ml.get() + nlen);

    // Account for the altered length.
    let d = hlen - ohlen;

    if i32::from(pd.proto) == IPPROTO_ICMPV6 {
        // Fixup pseudo-header.
        let dlen = (pd.tot_len - u64::from(pd.off)) as i32;
        pf_cksum_fixup_pd(
            pd,
            (dlen as u16).to_be(),
            ((dlen + d) as u16).to_be(),
            proto,
        );
    }

    pd.tot_len = (pd.tot_len as i64 + i64::from(d)) as u64;
    pd2.tot_len = (pd2.tot_len as i64 + i64::from(d)) as u64;
    pd2.off = (pd2.off as i32 + d) as u32;

    // Note: not bothering to update network headers as these are due for rewrite by
    // pf_translate_af().

    0
}

/// `PTR_IP(field)`: the offset of a member of `struct ip`.
#[cfg(feature = "inet6")]
macro_rules! ptr_ip {
    ($f:ident) => {
        core::mem::offset_of!(crate::netinet::ip::Ip, $f) as i32
    };
}

/// `PTR_IP6(field)`: the offset of a member of `struct ip6_hdr` (`ip6_vfc` is the first
/// byte of `ip6_flow`).
#[cfg(feature = "inet6")]
macro_rules! ptr_ip6 {
    (ip6_vfc) => {
        core::mem::offset_of!(Ip6Hdr, ip6_flow) as i32
    };
    ($f:ident) => {
        core::mem::offset_of!(Ip6Hdr, $f) as i32
    };
}

/// `pf_translate_icmp_af`: rewrites the ICMP header at `arg` (in the descriptor `pd`, an
/// ICMPv6 header when translating to `af` `AF_INET`, an ICMP one towards `AF_INET6`) into
/// the other protocol's type, code, MTU and pointer. 0, or -1 if the message has no
/// equivalent.
#[cfg(feature = "inet6")]
pub fn pf_translate_icmp_af(pd: &mut PfPdesc, af: SaFamily, arg: PfLoc) -> i32 {
    use crate::netinet::icmp6::*;
    use crate::netinet::ip::Ip;
    use crate::netinet::ip_icmp::*;

    let mut ptr: i32 = -1;
    let mut type_ = pd.ld8(arg);
    let mut code = pd.ld8(arg.offset(1));

    match af {
        AF_INET => {
            // arg is a struct icmp6_hdr.
            let mut mtu = u32::from_be(pd.ld32(arg.offset(4)));

            match type_ {
                ICMP6_ECHO_REQUEST => type_ = ICMP_ECHO,
                ICMP6_ECHO_REPLY => type_ = ICMP_ECHOREPLY,
                ICMP6_DST_UNREACH => {
                    type_ = ICMP_UNREACH;
                    code = match code {
                        ICMP6_DST_UNREACH_NOROUTE
                        | ICMP6_DST_UNREACH_BEYONDSCOPE
                        | ICMP6_DST_UNREACH_ADDR => ICMP_UNREACH_HOST,
                        ICMP6_DST_UNREACH_ADMIN => ICMP_UNREACH_HOST_PROHIB,
                        ICMP6_DST_UNREACH_NOPORT => ICMP_UNREACH_PORT,
                        _ => return -1,
                    };
                }
                ICMP6_PACKET_TOO_BIG => {
                    type_ = ICMP_UNREACH;
                    code = ICMP_UNREACH_NEEDFRAG;
                    mtu = mtu.wrapping_sub(20);
                }
                ICMP6_TIME_EXCEEDED => type_ = ICMP_TIMXCEED,
                ICMP6_PARAM_PROB => match code {
                    ICMP6_PARAMPROB_HEADER => {
                        type_ = ICMP_PARAMPROB;
                        code = ICMP_PARAMPROB_ERRATPTR;
                        ptr = u32::from_be(pd.ld32(arg.offset(4))) as i32;

                        ptr = if ptr == ptr_ip6!(ip6_vfc) {
                            ptr // preserve
                        } else if ptr == ptr_ip6!(ip6_vfc) + 1 {
                            ptr_ip!(ip_tos)
                        } else if ptr == ptr_ip6!(ip6_plen) || ptr == ptr_ip6!(ip6_plen) + 1 {
                            ptr_ip!(ip_len)
                        } else if ptr == ptr_ip6!(ip6_nxt) {
                            ptr_ip!(ip_p)
                        } else if ptr == ptr_ip6!(ip6_hlim) {
                            ptr_ip!(ip_ttl)
                        } else if ptr >= ptr_ip6!(ip6_src) && ptr < ptr_ip6!(ip6_dst) {
                            ptr_ip!(ip_src)
                        } else if ptr >= ptr_ip6!(ip6_dst) && ptr < size_of::<Ip6Hdr>() as i32 {
                            ptr_ip!(ip_dst)
                        } else {
                            return -1;
                        };
                    }
                    ICMP6_PARAMPROB_NEXTHEADER => {
                        type_ = ICMP_UNREACH;
                        code = ICMP_UNREACH_PROTOCOL;
                    }
                    _ => return -1,
                },
                _ => return -1,
            }

            pf_patch_8(pd, arg, type_, PF_HI);
            pf_patch_8(pd, arg.offset(1), code, PF_LO);

            // Aligns well with an ICMPv4 nextmtu.
            pf_patch_32(pd, arg.offset(4), mtu.to_be());

            // The ICMPv4 pptr is the one most significant byte.
            if ptr >= 0 {
                pf_patch_32(pd, arg.offset(4), ((ptr as u32) << 24).to_be());
            }
        }
        AF_INET6 => {
            // arg is a struct icmp.
            let mut mtu = u32::from(u16::from_be(pd.ld16(arg.offset(6))));

            match type_ {
                ICMP_ECHO => type_ = ICMP6_ECHO_REQUEST,
                ICMP_ECHOREPLY => type_ = ICMP6_ECHO_REPLY,
                ICMP_UNREACH => {
                    type_ = ICMP6_DST_UNREACH;
                    match code {
                        ICMP_UNREACH_NET
                        | ICMP_UNREACH_HOST
                        | ICMP_UNREACH_NET_UNKNOWN
                        | ICMP_UNREACH_HOST_UNKNOWN
                        | ICMP_UNREACH_ISOLATED
                        | ICMP_UNREACH_TOSNET
                        | ICMP_UNREACH_TOSHOST => code = ICMP6_DST_UNREACH_NOROUTE,
                        ICMP_UNREACH_PORT => code = ICMP6_DST_UNREACH_NOPORT,
                        ICMP_UNREACH_NET_PROHIB
                        | ICMP_UNREACH_HOST_PROHIB
                        | ICMP_UNREACH_FILTER_PROHIB
                        | ICMP_UNREACH_PRECEDENCE_CUTOFF => code = ICMP6_DST_UNREACH_ADMIN,
                        ICMP_UNREACH_PROTOCOL => {
                            type_ = ICMP6_PARAM_PROB;
                            code = ICMP6_PARAMPROB_NEXTHEADER;
                            ptr = ptr_ip6!(ip6_nxt);
                        }
                        ICMP_UNREACH_NEEDFRAG => {
                            type_ = ICMP6_PACKET_TOO_BIG;
                            code = 0;
                            mtu += 20;
                        }
                        _ => return -1,
                    }
                }
                ICMP_TIMXCEED => type_ = ICMP6_TIME_EXCEEDED,
                ICMP_PARAMPROB => {
                    type_ = ICMP6_PARAM_PROB;
                    code = match code {
                        ICMP_PARAMPROB_ERRATPTR | ICMP_PARAMPROB_LENGTH => ICMP6_PARAMPROB_HEADER,
                        _ => return -1,
                    };

                    ptr = i32::from(pd.ld8(arg.offset(4)));
                    ptr = if ptr == 0 || ptr == ptr_ip!(ip_tos) {
                        ptr // preserve
                    } else if ptr == ptr_ip!(ip_len) || ptr == ptr_ip!(ip_len) + 1 {
                        ptr_ip6!(ip6_plen)
                    } else if ptr == ptr_ip!(ip_ttl) {
                        ptr_ip6!(ip6_hlim)
                    } else if ptr == ptr_ip!(ip_p) {
                        ptr_ip6!(ip6_nxt)
                    } else if ptr >= ptr_ip!(ip_src) && ptr < ptr_ip!(ip_dst) {
                        ptr_ip6!(ip6_src)
                    } else if ptr >= ptr_ip!(ip_dst) && ptr < size_of::<Ip>() as i32 {
                        ptr_ip6!(ip6_dst)
                    } else {
                        return -1;
                    };
                }
                _ => return -1,
            }

            pf_patch_8(pd, arg, type_, PF_HI);
            pf_patch_8(pd, arg.offset(1), code, PF_LO);
            pf_patch_16(pd, arg.offset(6), (mtu as u16).to_be());
            if ptr >= 0 {
                pf_patch_32(pd, arg.offset(4), (ptr as u32).to_be());
            }
        }
        _ => {}
    }

    0
}

/// `TCPOLEN_MINSACK`.
const TCPOLEN_MINSACK: usize = TCPOLEN_SACK as usize + 2;

/// `pf_modulate_sack`: modulates the sequence numbers in the TCP SACK option (credits to
/// Krzysztof Pfaff for report and patch). Returns 1 when the options were rewritten.
pub fn pf_modulate_sack(pd: &mut PfPdesc, dst: &PfStatePeer) -> i32 {
    let mut copyback = 0;
    let mut opts = [0u8; MAX_TCPOPTLEN];

    let olen = (usize::from(pd.tcp().th_off()) << 2).saturating_sub(size_of::<Tcphdr>());
    let optsoff = pd.off as usize + size_of::<Tcphdr>();
    let Some(m) = pd.m else {
        return 0;
    };
    if !(TCPOLEN_MINSACK..=MAX_TCPOPTLEN).contains(&olen)
        || !pf_pull_hdr(m, optsoff as i32, &mut opts[..olen], None, pd.af)
    {
        return 0;
    }

    let mut optoff = 0usize;
    while let Some(o) = pf_find_tcpopt(&opts[..olen], optoff, TCPOPT_SACK, TCPOLEN_MINSACK as u8) {
        let safelen = core::cmp::min(usize::from(opts[o + 1]), olen - o);
        let mut i = 2;
        while i + usize::from(TCPOLEN_SACK) <= safelen {
            let startoff = o + i;
            // SAFETY: `opts` is a local array that outlives the two patches below and is
            // not otherwise accessed while they run; the offsets stay within it (checked by
            // the loop bound).
            let start = unsafe { PfLoc::local(opts.as_mut_ptr().add(startoff)) };
            let s = pd.ld32(start);
            pf_patch_32_unaligned(
                pd,
                start,
                u32::from_be(s).wrapping_sub(dst.seqdiff.get()).to_be(),
                pf_algnmnt(startoff),
            );
            let end = start.offset(4);
            let e = pd.ld32(end);
            pf_patch_32_unaligned(
                pd,
                end,
                u32::from_be(e).wrapping_sub(dst.seqdiff.get()).to_be(),
                pf_algnmnt(startoff + 4),
            );
            i += usize::from(TCPOLEN_SACK);
        }
        copyback = 1;
        optoff = o + usize::from(opts[o + 1]);
    }

    if copyback != 0 {
        let _ = crate::kern::uipc_mbuf::m_copyback(m, optsoff as i32, &opts[..olen], M_NOWAIT);
    }
    copyback
}

/// `pf_build_tcp`: an mbuf with a TCP segment (with an MSS option when `mss` is set and a
/// SACK-permitted option when `sack` is).
#[allow(clippy::too_many_arguments)]
pub fn pf_build_tcp(
    r: Option<&'static PfRule>,
    af: SaFamily,
    saddr: &PfAddr,
    daddr: &PfAddr,
    sport: u16,
    dport: u16,
    seq: u32,
    ack: u32,
    flags: u8,
    win: u16,
    mss: u16,
    ttl: u8,
    tag: bool,
    rtag: u16,
    sack: bool,
    rdom: u32,
    reason: Option<&mut u16>,
) -> Option<&'static Mbuf> {
    use crate::netinet::ip::{IP_DF, IPTOS_LOWDELAY, Ip};

    // Maximum segment size tcp option.
    let mut tlen = size_of::<Tcphdr>();
    if mss != 0 {
        tlen += 4;
    }
    if sack {
        tlen += 2;
    }

    let len = match af {
        AF_INET => size_of::<Ip>() + tlen,
        #[cfg(feature = "inet6")]
        AF_INET6 => size_of::<Ip6Hdr>() + tlen,
        _ => unhandled_af(i32::from(af)),
    };

    // Create outgoing mbuf.
    let Some(m) = crate::kern::uipc_mbuf::m_gethdr(M_DONTWAIT, MT_HEADER) else {
        if let Some(reason) = reason {
            reason_set(reason, PFRES_MEMORY);
        }
        return None;
    };
    let pf = &m.m_pkthdr().pf;
    if tag {
        pf.flags.set(pf.flags.get() | PF_TAG_GENERATED);
    }
    pf.tag.set(rtag);
    m.m_pkthdr().ph_rtableid.set(rdom);
    if let Some(r) = r {
        if u32::from(r.scrub_flags) & u32::from(PFSTATE_SETPRIO) != 0 {
            pf.prio.set(r.set_prio[0]);
        }
        if r.qid != 0 {
            pf.qid.set(r.qid);
        }
    }
    let linkhdr = crate::kern::uipc_mbuf::MAX_LINKHDR.load(AtomicOrdering::Relaxed) as usize;
    // SAFETY: a fresh packet header mbuf has room for max_linkhdr plus the headers (MHLEN).
    m.m_data().set(unsafe { m.m_data().get().add(linkhdr) });
    m.m_pkthdr().len.set(len as i32);
    m.m_len().set(len as u32);
    m.m_pkthdr().ph_ifidx.set(0);
    let cf = &m.m_pkthdr().csum_flags;
    cf.set(cf.get() | M_TCP_CSUM_OUT);
    // SAFETY: `len` bytes from m_data are within the fresh mbuf (above).
    unsafe { ptr::write_bytes(m.m_data().get(), 0, len) };
    let thoff = match af {
        AF_INET => {
            let mut h = Ip {
                ip_p: IPPROTO_TCP as u8,
                ..Ip::default()
            };
            h.set_ip_v(4);
            h.set_ip_hl((size_of::<Ip>() >> 2) as u8);
            h.ip_tos = IPTOS_LOWDELAY;
            h.ip_len = (len as u16).to_be();
            h.ip_off = if crate::netinet::ip_input::ip_mtudisc.load(AtomicOrdering::Relaxed) != 0 {
                IP_DF.to_be()
            } else {
                0
            };
            h.ip_ttl = if ttl != 0 {
                ttl
            } else {
                crate::netinet::ip_input::IP_DEFTTL.load(AtomicOrdering::Relaxed) as u8
            };
            h.ip_sum = 0;
            h.ip_src.s_addr = saddr.v4().s_addr;
            h.ip_dst.s_addr = daddr.v4().s_addr;
            crate::netinet::ip_var::mtod_ip_store(m, &h);

            size_of::<Ip>()
        }
        #[cfg(feature = "inet6")]
        AF_INET6 => {
            use crate::netinet::ip6::{IPV6_DEFHLIM, IPV6_VERSION};
            let mut h6 = Ip6Hdr::zeroed();
            h6.ip6_nxt = IPPROTO_TCP as u8;
            h6.ip6_plen = (tlen as u16).to_be();
            h6.set_ip6_vfc(h6.ip6_vfc() | IPV6_VERSION);
            h6.ip6_hlim = IPV6_DEFHLIM;
            h6.ip6_src = saddr.v6();
            h6.ip6_dst = daddr.v6();
            crate::netinet6::ip6_var::mtod_ip6_store(m, &h6);

            size_of::<Ip6Hdr>()
        }
        _ => unhandled_af(i32::from(af)),
    };

    // TCP header.
    let mut th = Tcphdr {
        th_sport: sport,
        th_dport: dport,
        th_seq: seq.to_be(),
        th_ack: ack.to_be(),
        th_flags: flags,
        th_win: win.to_be(),
        ..Tcphdr::default()
    };
    th.set_th_off((tlen >> 2) as u8);
    let mut bytes = [0u8; 26];
    // SAFETY: `Tcphdr` is 20 bytes of integers (`#[repr(C)]`, no padding); `bytes` has room.
    unsafe { ptr::write_unaligned(bytes.as_mut_ptr().cast::<Tcphdr>(), th) };

    let mut o = size_of::<Tcphdr>();
    if mss != 0 {
        bytes[o] = TCPOPT_MAXSEG;
        bytes[o + 1] = 4;
        bytes[o + 2..o + 4].copy_from_slice(&mss.to_be_bytes());
        o += 4;
    }
    if sack {
        bytes[o] = TCPOPT_SACK_PERMITTED;
        bytes[o + 1] = 2;
    }
    // SAFETY: the segment's `tlen` bytes follow the network header within `len`.
    unsafe {
        ptr::copy_nonoverlapping(bytes.as_ptr(), m.m_data().get().add(thoff), tlen);
    }

    Some(m)
}

/// `pf_send_tcp`: builds a TCP segment and sends it.
#[allow(clippy::too_many_arguments)]
pub fn pf_send_tcp(
    r: Option<&'static PfRule>,
    af: SaFamily,
    saddr: &PfAddr,
    daddr: &PfAddr,
    sport: u16,
    dport: u16,
    seq: u32,
    ack: u32,
    flags: u8,
    win: u16,
    mss: u16,
    ttl: u8,
    tag: bool,
    rtag: u16,
    rdom: u32,
    reason: Option<&mut u16>,
) {
    let Some(m) = pf_build_tcp(
        r, af, saddr, daddr, sport, dport, seq, ack, flags, win, mss, ttl, tag, rtag, false, rdom,
        reason,
    ) else {
        return;
    };

    match af {
        AF_INET => crate::netinet::ip_input::ip_send(m),
        #[cfg(feature = "inet6")]
        AF_INET6 => crate::netinet6::ip6_input::ip6_send(m),
        _ => {
            crate::kern::uipc_mbuf::m_freem(m);
        }
    }
}

/// `pf_send_challenge_ack`: answers a SYN that matches an existing state (modulo the TCP
/// window check) with an ACK, on behalf of the destination. The sender is expected to stay
/// silent or send a RST, so that both the firewall and the remote peer can purge the dead
/// state.
fn pf_send_challenge_ack(
    pd: &mut PfPdesc,
    st: &'static PfState,
    src: &PfStatePeer,
    dst: &PfStatePeer,
    reason: &mut u16,
) {
    let s = pd.ld_addr(pd.src);
    let d = pd.ld_addr(pd.dst);
    let th = *pd.tcp();
    pf_send_tcp(
        st.rule.ptr(),
        pd.af,
        &d,
        &s,
        th.th_dport,
        th.th_sport,
        dst.seqlo.get(),
        src.seqlo.get(),
        TH_ACK,
        0,
        0,
        st.rule.ptr().map_or(0, |r| r.return_ttl),
        true,
        0,
        u32::from(pd.rdomain),
        Some(reason),
    );
}

/// `pf_send_icmp`: answers a copy of `m` with an ICMP error.
pub fn pf_send_icmp(
    m: &Mbuf,
    type_: u8,
    code: u8,
    param: i32,
    af: SaFamily,
    r: Option<&'static PfRule>,
    rdomain: u32,
) {
    let Some(m0) = crate::kern::uipc_mbuf::m_copym(m, 0, M_COPYALL, M_NOWAIT) else {
        return;
    };

    let pf = &m0.m_pkthdr().pf;
    pf.flags.set(pf.flags.get() | PF_TAG_GENERATED);
    m0.m_pkthdr().ph_rtableid.set(rdomain);
    if let Some(r) = r {
        if r.scrub_flags & PFSTATE_SETPRIO != 0 {
            pf.prio.set(r.set_prio[0]);
        }
        if r.qid != 0 {
            pf.qid.set(r.qid);
        }
    }

    match af {
        AF_INET => crate::netinet::ip_icmp::icmp_error(m0, type_, code, 0, param),
        #[cfg(feature = "inet6")]
        AF_INET6 => crate::netinet6::icmp6::icmp6_error(m0, type_, code, param),
        _ => {
            crate::kern::uipc_mbuf::m_freem(m0);
        }
    }
}

/// `pf_match_addr`: `(n == 0) == (a == b with mask m)`; with `n != 0` it says whether they
/// differ.
pub fn pf_match_addr(n: u8, a: &PfAddr, m: &PfAddr, b: &PfAddr, af: SaFamily) -> bool {
    let words = match af {
        AF_INET => 1,
        #[cfg(feature = "inet6")]
        AF_INET6 => 4,
        _ => return n != 0,
    };
    if (0..words).all(|i| (a.addr32(i) & m.addr32(i)) == (b.addr32(i) & m.addr32(i))) {
        return n == 0;
    }
    n != 0
}

/// `pf_match_addr_range`: `b <= a <= e`.
pub fn pf_match_addr_range(b: &PfAddr, e: &PfAddr, a: &PfAddr, af: SaFamily) -> bool {
    if af == AF_INET {
        let (a, b, e) = (
            u32::from_be(a.addr32(0)),
            u32::from_be(b.addr32(0)),
            u32::from_be(e.addr32(0)),
        );
        if a < b || a > e {
            return false;
        }
    }
    #[cfg(feature = "inet6")]
    if af == AF_INET6 {
        let w = |x: &PfAddr, i: usize| u32::from_be(x.addr32(i));
        // Check a >= b.
        for i in 0..4 {
            if w(a, i) > w(b, i) {
                break;
            } else if w(a, i) < w(b, i) {
                return false;
            }
        }
        // Check a <= e.
        for i in 0..4 {
            if w(a, i) < w(e, i) {
                break;
            } else if w(a, i) > w(e, i) {
                return false;
            }
        }
    }
    true
}

/// `pf_match`: `p` against the operator and operands of a port, uid or gid match.
pub fn pf_match(op: u8, a1: u32, a2: u32, p: u32) -> bool {
    match op {
        PF_OP_IRG => p > a1 && p < a2,
        PF_OP_XRG => p < a1 || p > a2,
        PF_OP_RRG => p >= a1 && p <= a2,
        PF_OP_EQ => p == a1,
        PF_OP_NE => p != a1,
        PF_OP_LT => p < a1,
        PF_OP_LE => p <= a1,
        PF_OP_GT => p > a1,
        PF_OP_GE => p >= a1,
        _ => false, // never reached
    }
}

/// `pf_match_port`: ports in network order.
pub fn pf_match_port(op: u8, a1: u16, a2: u16, p: u16) -> bool {
    pf_match(
        op,
        u32::from(u16::from_be(a1)),
        u32::from(u16::from_be(a2)),
        u32::from(u16::from_be(p)),
    )
}

/// `pf_match_uid`: an unknown uid (-1) only matches `=` and `!=`.
pub fn pf_match_uid(op: u8, a1: Uid, a2: Uid, u: Uid) -> bool {
    if u == Uid::MAX && op != PF_OP_EQ && op != PF_OP_NE {
        return false;
    }
    pf_match(op, a1, a2, u)
}

/// `pf_match_gid`.
pub fn pf_match_gid(op: u8, a1: Gid, a2: Gid, g: Gid) -> bool {
    if g == Gid::MAX && op != PF_OP_EQ && op != PF_OP_NE {
        return false;
    }
    pf_match(op, a1, a2, g)
}

/// `pf_match_tag`: the packet's tag (read into `*tag` the first time) against the rule's.
pub fn pf_match_tag(m: &Mbuf, r: &PfRule, tag: &mut i32) -> bool {
    if *tag == -1 {
        *tag = i32::from(m.m_pkthdr().pf.tag.get());
    }

    (r.match_tag_not == 0 && i32::from(r.match_tag) == *tag)
        || (r.match_tag_not != 0 && i32::from(r.match_tag) != *tag)
}

/// `pf_match_rcvif`: the packet was received on the rule's `received-on` interface.
pub fn pf_match_rcvif(m: &Mbuf, r: &'static PfRule) -> bool {
    if m.m_pkthdr().ph_ifidx.get() == 0 {
        return false;
    }

    smr_read_enter();
    let ifp = if_get_smr(m.m_pkthdr().ph_ifidx.get());
    let kif = ifp.and_then(|ifp| {
        // NCARP > 0: a carp interface's parent's kif; not configured.
        // SAFETY: `if_pf_kif` is null or the kif `pfi_attach_ifnet` set, which lives until
        // `pfi_detach_ifnet`.
        unsafe { ifp.if_pf_kif.get().cast::<PfiKif>().cast_const().as_ref() }
    });
    smr_read_leave();

    let Some(kif) = kif else {
        crate::dpfprintf!(
            LOG_ERR,
            "pf_match_rcvif: kif == NULL, @{} via {}",
            r.nr.get() as i32,
            crate::kern::subr_prf::Str(&r.rcv_ifname)
        );
        return false;
    };

    crate::net::pf_if::pfi_kif_match(r.rcv_kif(), Some(kif))
}

/// `pf_tag_packet`.
pub fn pf_tag_packet(m: &Mbuf, tag: i32, rtableid: i32) {
    if tag > 0 {
        m.m_pkthdr().pf.tag.set(tag as u16);
    }
    if rtableid >= 0 {
        m.m_pkthdr().ph_rtableid.set(rtableid as u32);
    }
}

/// `pf_anchor_stack`: `PF_ANCHOR_STACK_MAX + 2` frames (per CPU in the C).
pub static PF_ANCHOR_STACK: PfAnchorStack =
    PfAnchorStack([const { PfAnchorStackframe::new() }; PF_ANCHOR_STACK_MAX + 2]);

/// `pf_anchor_stack_init`.
pub fn pf_anchor_stack_init() {
    PF_ANCHOR_STACK.0[PF_ANCHOR_STACK_MAX].sf_stack_top.set(0);
}

/// `pf_anchor_stack_is_full`.
pub fn pf_anchor_stack_is_full(sf: usize) -> bool {
    sf == PF_ANCHOR_STACK_MAX
}

/// `pf_anchor_stack_is_empty`.
pub fn pf_anchor_stack_is_empty(sf: usize) -> bool {
    sf == 0
}

/// `pf_anchor_stack_top`: the index of the top frame.
pub fn pf_anchor_stack_top() -> usize {
    PF_ANCHOR_STACK.0[PF_ANCHOR_STACK_MAX].sf_stack_top.get()
}

/// `pf_anchor_stack_push`; `Err` when the stack is full (the C's -1).
pub fn pf_anchor_stack_push(
    rs: Option<&'static PfRuleset>,
    anchor: Option<&'static PfRule>,
    r: Option<&'static PfRule>,
    child: Option<&'static PfAnchor>,
    jump_target: i32,
) -> bool {
    let top_sf = pf_anchor_stack_top() + 1;
    if pf_anchor_stack_is_full(top_sf) {
        return false;
    }

    let f = &PF_ANCHOR_STACK.0[top_sf];
    f.sf_rs.set(rs);
    f.sf_anchor.set(anchor);
    f.sf_r.set(r);
    f.sf_child.set(child);
    f.sf_jump_target.set(jump_target);

    if top_sf == 0 || top_sf >= PF_ANCHOR_STACK_MAX {
        panic(format_args!(
            "pf_anchor_stack_push: top frame outside of anchor stack range"
        ));
    }

    PF_ANCHOR_STACK.0[PF_ANCHOR_STACK_MAX]
        .sf_stack_top
        .set(top_sf);

    true
}

/// A popped anchor stack frame.
pub struct PfAnchorFrame {
    /// `rs`.
    pub rs: Option<&'static PfRuleset>,
    /// `anchor`.
    pub anchor: Option<&'static PfRule>,
    /// `r`.
    pub r: Option<&'static PfRule>,
    /// `child`.
    pub child: Option<&'static PfAnchor>,
    /// `jump_target`.
    pub jump_target: i32,
}

/// `pf_anchor_stack_pop`: the top frame, `None` when the stack is empty (the C's -1).
pub fn pf_anchor_stack_pop() -> Option<PfAnchorFrame> {
    let top_sf = pf_anchor_stack_top();

    if pf_anchor_stack_is_empty(top_sf) {
        return None;
    }
    if top_sf == 0 || top_sf >= PF_ANCHOR_STACK_MAX {
        panic(format_args!(
            "pf_anchor_stack_pop: top frame outside of anchor stack range"
        ));
    }

    let f = &PF_ANCHOR_STACK.0[top_sf];
    let frame = PfAnchorFrame {
        rs: f.sf_rs.get(),
        anchor: f.sf_anchor.get(),
        r: f.sf_r.get(),
        child: f.sf_child.get(),
        jump_target: f.sf_jump_target.get(),
    };
    PF_ANCHOR_STACK.0[PF_ANCHOR_STACK_MAX]
        .sf_stack_top
        .set(top_sf - 1);
    Some(frame)
}

/// `pf_poolmask`: `naddr` = the network part of `raddr` under `rmask`, the host part of
/// `saddr`.
pub fn pf_poolmask(
    naddr: &mut PfAddr,
    raddr: &PfAddr,
    rmask: &PfAddr,
    saddr: &PfAddr,
    af: SaFamily,
) {
    match af {
        AF_INET => naddr.set_addr32(
            0,
            (raddr.addr32(0) & rmask.addr32(0))
                | ((rmask.addr32(0) ^ 0xffff_ffff) & saddr.addr32(0)),
        ),
        #[cfg(feature = "inet6")]
        AF_INET6 => {
            for i in 0..4 {
                naddr.set_addr32(
                    i,
                    (raddr.addr32(i) & rmask.addr32(i))
                        | ((rmask.addr32(i) ^ 0xffff_ffff) & saddr.addr32(i)),
                );
            }
        }
        _ => unhandled_af(i32::from(af)),
    }
}

/// `pf_addr_inc`: the next address.
pub fn pf_addr_inc(addr: &mut PfAddr, af: SaFamily) {
    match af {
        AF_INET => addr.set_addr32(0, u32::from_be(addr.addr32(0)).wrapping_add(1).to_be()),
        #[cfg(feature = "inet6")]
        AF_INET6 => {
            // The lowest word that is not all ones is incremented, the ones below it
            // (all ones) wrap to zero; the first word takes the last carry.
            let mut i = 3;
            while i > 0 && addr.addr32(i) == 0xffff_ffff {
                addr.set_addr32(i, 0);
                i -= 1;
            }
            addr.set_addr32(i, u32::from_be(addr.addr32(i)).wrapping_add(1).to_be());
        }
        _ => unhandled_af(i32::from(af)),
    }
}

/// `pf_socket_lookup`: fills `pd->lookup` with the credentials of the local socket the packet
/// belongs to. `true` (the C's 1) when found.
pub fn pf_socket_lookup(pd: &mut PfPdesc) -> bool {
    use crate::netinet::in_pcb::{in_pcblookup, in_pcblookup_listen, in_pcbunref};

    pd.lookup.uid = Uid::MAX;
    pd.lookup.gid = Gid::MAX;
    pd.lookup.pid = crate::sys::proc::NO_PID;
    let (mut sport, mut dport, table) = match i32::from(pd.virtual_proto) {
        IPPROTO_TCP => {
            let th = *pd.tcp();
            pf_assert_locked();
            crate::sys::systm::net_assert_locked("pf_socket_lookup");
            (
                th.th_sport,
                th.th_dport,
                &crate::netinet::tcp_usrreq::TCBTABLE,
            )
        }
        IPPROTO_UDP => {
            let uh = *pd.udp();
            pf_assert_locked();
            crate::sys::systm::net_assert_locked("pf_socket_lookup");
            (
                uh.uh_sport,
                uh.uh_dport,
                &crate::netinet::udp_usrreq::UDBTABLE,
            )
        }
        _ => return false,
    };
    let (saddr, daddr) = if pd.dir == PF_IN {
        (pd.ld_addr(pd.src), pd.ld_addr(pd.dst))
    } else {
        core::mem::swap(&mut sport, &mut dport);
        (pd.ld_addr(pd.dst), pd.ld_addr(pd.src))
    };
    let inp = match pd.af {
        AF_INET => {
            // Fails when rtable is changed while evaluating the ruleset: the socket looked up
            // will not match the one hit in the end.
            match in_pcblookup(
                table,
                saddr.v4(),
                sport,
                daddr.v4(),
                dport,
                u32::from(pd.rdomain),
            ) {
                Some(inp) => inp,
                None => {
                    let Some(inp) =
                        in_pcblookup_listen(table, daddr.v4(), dport, None, u32::from(pd.rdomain))
                    else {
                        return false;
                    };
                    inp
                }
            }
        }
        #[cfg(feature = "inet6")]
        AF_INET6 => {
            use crate::netinet6::in6_pcb::{in6_pcblookup, in6_pcblookup_listen};
            let table = match i32::from(pd.virtual_proto) {
                IPPROTO_UDP => &crate::netinet::udp_usrreq::UDB6TABLE,
                IPPROTO_TCP => &crate::netinet::tcp_usrreq::TCB6TABLE,
                _ => table,
            };
            let rdomain = u32::from(pd.rdomain);
            match in6_pcblookup(table, &saddr.v6(), sport, &daddr.v6(), dport, rdomain) {
                Some(inp) => inp,
                None => {
                    let Some(inp) = in6_pcblookup_listen(table, &daddr.v6(), dport, None, rdomain)
                    else {
                        return false;
                    };
                    inp
                }
            }
        }
        _ => unhandled_af(i32::from(pd.af)),
    };
    if let Some(so) = inp.inp_socket {
        pd.lookup.uid = so.so_euid.get();
        pd.lookup.gid = so.so_egid.get();
        pd.lookup.pid = so.so_cpid.get();
    }
    in_pcbunref(Some(inp));
    true
}

/// `pf_find_tcpopt`: the offset in `opts` of the first option of `type_` at or after `opt`
/// whose length is at least `min_typelen` (>= 2) and which has that many bytes left. The
/// option's own length may exceed the buffer when it is larger than `min_typelen`.
pub fn pf_find_tcpopt(opts: &[u8], opt: usize, type_: u8, min_typelen: u8) -> Option<usize> {
    let eoh = opts.len();
    let mut opt = opt;

    if min_typelen < 2 {
        return None;
    }

    while eoh.saturating_sub(opt) >= usize::from(min_typelen) {
        match opts[opt] {
            // EOL: workaround the failure of some systems to NOP-pad their bzero'd option
            // buffers, producing spurious EOLs.
            TCPOPT_EOL | TCPOPT_NOP => {
                opt += 1;
                continue;
            }
            _ => {
                if opts[opt] == type_ && opts[opt + 1] >= min_typelen {
                    return Some(opt);
                }
            }
        }

        opt += usize::from(core::cmp::max(opts[opt + 1], 2)); // evade infinite loops
    }

    None
}

/// `pf_get_wscale`: the window scale option of the SYN, with `PF_WSCALE_FLAG`, or 0.
pub fn pf_get_wscale(pd: &mut PfPdesc) -> u8 {
    let mut opts = [0u8; MAX_TCPOPTLEN];
    let mut wscale = 0u8;

    let olen = (usize::from(pd.tcp().th_off()) << 2).wrapping_sub(size_of::<Tcphdr>()) as isize;
    let Some(m) = pd.m else {
        return 0;
    };
    if olen < TCPOLEN_WINDOW as isize
        || olen as usize > MAX_TCPOPTLEN
        || !pf_pull_hdr(
            m,
            (pd.off as usize + size_of::<Tcphdr>()) as i32,
            &mut opts[..olen as usize],
            None,
            pd.af,
        )
    {
        return 0;
    }
    let opts = &opts[..olen as usize];

    let mut opt = 0;
    while let Some(o) = pf_find_tcpopt(opts, opt, TCPOPT_WINDOW, TCPOLEN_WINDOW) {
        wscale = opts[o + 2];
        wscale = core::cmp::min(wscale, TCP_MAX_WINSHIFT);
        wscale |= PF_WSCALE_FLAG;

        opt = o + usize::from(opts[o + 1]);
    }

    wscale
}

/// `pf_get_mss`: the MSS option of the segment, `mssdflt` without one.
pub fn pf_get_mss(pd: &mut PfPdesc, mssdflt: u16) -> u16 {
    let mut opts = [0u8; MAX_TCPOPTLEN];

    let olen = (usize::from(pd.tcp().th_off()) << 2).wrapping_sub(size_of::<Tcphdr>()) as isize;
    let Some(m) = pd.m else {
        return 0;
    };
    if olen < TCPOLEN_MAXSEG as isize
        || olen as usize > MAX_TCPOPTLEN
        || !pf_pull_hdr(
            m,
            (pd.off as usize + size_of::<Tcphdr>()) as i32,
            &mut opts[..olen as usize],
            None,
            pd.af,
        )
    {
        return 0;
    }
    let opts = &opts[..olen as usize];

    let mut mss = mssdflt;
    let mut opt = 0;
    while let Some(o) = pf_find_tcpopt(opts, opt, TCPOPT_MAXSEG, TCPOLEN_MAXSEG) {
        mss = u16::from_be_bytes([opts[o + 2], opts[o + 3]]);

        opt = o + usize::from(opts[o + 1]);
    }
    mss
}

/// `pf_calc_mss`: the MSS to offer towards `addr`, from the route's interface MTU.
pub fn pf_calc_mss(addr: &PfAddr, af: SaFamily, rtableid: i32, offer: u16, mssdflt: u16) -> u16 {
    use crate::netinet::in_::{SockaddrIn, sintosa};
    use crate::netinet::ip::Ip;

    let mut dst = SockaddrIn::default();
    let (hlen, rt) = match af {
        AF_INET => {
            dst.sin_family = AF_INET;
            dst.sin_len = size_of::<SockaddrIn>() as u8;
            dst.sin_addr = addr.v4();
            // SAFETY: `dst` is a complete local sockaddr_in that outlives the call.
            let rt = unsafe { crate::net::route::rtalloc(sintosa(&mut dst), 0, rtableid as u32) };
            (size_of::<Ip>() as i32, rt)
        }
        #[cfg(feature = "inet6")]
        AF_INET6 => {
            use crate::netinet6::in6::{SockaddrIn6, sin6tosa_const};
            let dst6 = SockaddrIn6::with_addr(addr.v6());
            // SAFETY: `dst6` is a complete local sockaddr_in6 that outlives the call.
            let rt =
                unsafe { crate::net::route::rtalloc(sin6tosa_const(&dst6), 0, rtableid as u32) };
            (size_of::<Ip6Hdr>() as i32, rt)
        }
        _ => (0, None),
    };

    let mut mss = i32::from(mssdflt);
    if let Some(rt) = rt
        && let Some(ifp) = if_get(rt.rt_ifidx.get())
    {
        mss = ifp.if_mtu.get() as i32 - hlen - size_of::<Tcphdr>() as i32;
        mss = core::cmp::max(mss, i32::from(mssdflt));
        if_put(Some(ifp));
    }
    crate::net::route::rtfree(rt);
    mss = core::cmp::min(mss, i32::from(offer));
    mss = core::cmp::max(mss, 64); // sanity - at least max opt space
    mss as u16
}

/// `pf_set_rt_ifp`: picks the route-to address of the state's rule. `Err` is the C's
/// nonzero.
fn pf_set_rt_ifp(
    st: &'static PfState,
    saddr: &PfAddr,
    af: SaFamily,
    sns: &mut [Option<&'static PfSrcNode>; PF_SN_MAX],
) -> bool {
    let Some(r) = st.rule.ptr() else {
        return true;
    };

    if r.rt == 0 {
        return true;
    }

    let mut rt_addr = st.rt_addr.get();
    let rv = crate::net::pf_lb::pf_map_addr(
        af,
        r,
        saddr,
        &mut rt_addr,
        None,
        sns,
        &r.route,
        PF_SN_ROUTE,
    );
    st.rt_addr.set(rt_addr);
    if rv {
        st.rt.set(r.rt);
    }

    rv
}

/// `tcp_mssdflt`: the default MSS (`net.inet.tcp.mssdflt`, `netinet/tcp_subr.rs`).
pub fn tcp_mssdflt() -> u16 {
    crate::netinet::tcp_subr::TCP_MSSDFLT.load(core::sync::atomic::Ordering::Relaxed) as u16
}

/// `pf_tcp_iss`: an initial sequence number for a modulated connection, from a keyed hash
/// of the connection.
pub fn pf_tcp_iss(pd: &mut PfPdesc) -> u32 {
    use crate::crypto::sha2::SHA512_DIGEST_LENGTH;

    let sec = &*PF_TCP_SECRET;
    if !sec.init.get() {
        let mut s = [0u8; 16];
        arc4random_buf(&mut s);
        sec.secret.set(s);
        let mut c = Sha2Ctx::default();
        SHA512Init(&mut c);
        SHA512Update(&mut c, &s);
        sec.ctx.set(Some(c));
        sec.init.set(true);
    }
    let Some(mut ctx) = sec.ctx.get() else {
        return 0;
    };

    let th = *pd.tcp();
    SHA512Update(&mut ctx, &pd.rdomain.to_ne_bytes());
    SHA512Update(&mut ctx, &th.th_sport.to_ne_bytes());
    SHA512Update(&mut ctx, &th.th_dport.to_ne_bytes());
    if pd.af == AF_INET {
        let s = pd.ld_addr(pd.src);
        let d = pd.ld_addr(pd.dst);
        SHA512Update(&mut ctx, &s.addr8[..4]);
        SHA512Update(&mut ctx, &d.addr8[..4]);
    }
    #[cfg(feature = "inet6")]
    if pd.af == AF_INET6 {
        let s = pd.ld_addr(pd.src);
        let d = pd.ld_addr(pd.dst);
        SHA512Update(&mut ctx, &s.addr8);
        SHA512Update(&mut ctx, &d.addr8);
    }
    let mut digest = [0u8; SHA512_DIGEST_LENGTH];
    SHA512Final(&mut digest, &mut ctx);
    let off = sec.iss_off.get().wrapping_add(4096);
    sec.iss_off.set(off);
    u32::from_ne_bytes([digest[0], digest[1], digest[2], digest[3]])
        .wrapping_add(crate::netinet::tcp_subr::TCP_ISS.load(core::sync::atomic::Ordering::Relaxed))
        .wrapping_add(off)
}

/// `pf_rule_to_actions`: accumulates what a matching rule asks of the packet.
pub fn pf_rule_to_actions(r: &PfRule, a: &mut PfRuleActions) {
    if r.qid != 0 {
        a.qid = r.qid as u16;
    }
    if r.pqid != 0 {
        a.pqid = r.pqid as u16;
    }
    if r.rtableid >= 0 {
        a.rtableid = r.rtableid;
    }
    a.log |= r.log;
    if r.scrub_flags & PFSTATE_SETTOS != 0 {
        a.set_tos = r.set_tos;
    }
    if r.min_ttl != 0 {
        a.min_ttl = r.min_ttl;
    }
    if r.max_mss != 0 {
        a.max_mss = r.max_mss;
    }
    a.flags |= r.scrub_flags
        & (PFSTATE_NODF | PFSTATE_RANDOMID | PFSTATE_SETTOS | PFSTATE_SCRUB_TCP | PFSTATE_SETPRIO);
    if r.scrub_flags & PFSTATE_SETPRIO != 0 {
        a.set_prio = r.set_prio;
    }
    if r.rule_flag.get() & PFRULE_SETDELAY != 0 {
        a.delay = r.delay;
    }
}

/// `pf_match_rule`: evaluates `ruleset` (and the anchors it calls) for the packet in `ctx`.
/// The C's `goto enter_ruleset`/`next_child`/`next_rule` into its loop are the anchor stack
/// driving an outer loop here; the order of evaluation is the C's.
pub fn pf_match_rule(ctx: &mut PfTestCtx<'_>, ruleset: &'static PfRuleset) -> PfTestStatus {
    let mut ruleset = ruleset;
    let mut child: Option<&'static PfAnchor> = None;

    pf_anchor_stack_init();
    // enter_ruleset:
    let mut r: Option<&'static PfRule> = ruleset.active_ptr().first();
    loop {
        'rules: while let Some(rr) = r {
            let mut stlim: Option<&'static PfStatelim> = None;
            let mut srlim: Option<&'static PfSourcelim> = None;
            let mut sr: Option<&'static PfSource> = None;
            let next = TailqHead::<PfRulequeue>::next(rr);

            // PF_TEST_ATTRIB(t, a): if t, r = a and evaluate the next rule.
            macro_rules! pf_test_attrib {
                ($t:expr, $a:expr) => {
                    if $t {
                        r = $a;
                        continue 'rules;
                    }
                };
            }
            macro_rules! skip {
                ($i:expr) => {
                    rr.skip[$i].ptr()
                };
            }

            pf_test_attrib!(rr.rule_flag.get() & PFRULE_EXPIRED != 0, next);
            pf_inc(&rr.evaluations);
            pf_test_attrib!(
                crate::net::pf_if::pfi_kif_match(rr.kif(), ctx.pd.kif) == (rr.ifnot != 0),
                skip!(PF_SKIP_IFP)
            );
            pf_test_attrib!(
                rr.direction != 0 && rr.direction != ctx.pd.dir,
                skip!(PF_SKIP_DIR)
            );
            pf_test_attrib!(
                rr.onrdomain >= 0 && (rr.onrdomain == i32::from(ctx.pd.rdomain)) == (rr.ifnot != 0),
                skip!(PF_SKIP_RDOM)
            );
            pf_test_attrib!(rr.af != 0 && rr.af != ctx.pd.af, skip!(PF_SKIP_AF));
            pf_test_attrib!(
                rr.proto != 0 && rr.proto != ctx.pd.proto,
                skip!(PF_SKIP_PROTO)
            );
            let (nsaddr, ndaddr) = (ctx.pd.nsaddr, ctx.pd.ndaddr);
            pf_test_attrib!(
                pf_mismatchaw(
                    &rr.src.addr,
                    &nsaddr,
                    ctx.pd.naf,
                    rr.src.neg.get() != 0,
                    ctx.pd.kif,
                    ctx.act.rtableid
                ),
                skip!(PF_SKIP_SRC_ADDR)
            );
            pf_test_attrib!(
                pf_mismatchaw(
                    &rr.dst.addr,
                    &ndaddr,
                    ctx.pd.af,
                    rr.dst.neg.get() != 0,
                    None,
                    ctx.act.rtableid
                ),
                skip!(PF_SKIP_DST_ADDR)
            );

            match ctx.pd.virtual_proto {
                PF_VPROTO_FRAGMENT => {
                    // tcp/udp only. port_op always 0 in other cases.
                    pf_test_attrib!(rr.src.port_op.get() != 0 || rr.dst.port_op.get() != 0, next);
                    pf_test_attrib!(
                        i32::from(ctx.pd.proto) == IPPROTO_TCP && rr.flagset != 0,
                        next
                    );
                    // icmp only. type/code always 0 in other cases.
                    pf_test_attrib!(rr.type_ != 0 || rr.code != 0, next);
                    // tcp/udp only. {uid|gid}.op always 0 in other cases.
                    pf_test_attrib!(rr.gid.op != 0 || rr.uid.op != 0, next);
                }
                vp if i32::from(vp) == IPPROTO_TCP || i32::from(vp) == IPPROTO_UDP => {
                    if i32::from(vp) == IPPROTO_TCP {
                        let th_flags = ctx.pd.tcp().th_flags;
                        pf_test_attrib!((rr.flagset & th_flags) != rr.flags, next);
                        pf_test_attrib!(
                            rr.os_fingerprint != PF_OSFP_ANY
                                && !crate::net::pf_osfp::pf_osfp_match(
                                    crate::net::pf_osfp::pf_osfp_fingerprint(ctx.pd),
                                    rr.os_fingerprint
                                ),
                            next
                        );
                    }
                    // tcp/udp only. port_op always 0 in other cases.
                    pf_test_attrib!(
                        rr.src.port_op.get() != 0
                            && !pf_match_port(
                                rr.src.port_op.get(),
                                rr.src.port[0].get(),
                                rr.src.port[1].get(),
                                ctx.pd.nsport
                            ),
                        skip!(PF_SKIP_SRC_PORT)
                    );
                    pf_test_attrib!(
                        rr.dst.port_op.get() != 0
                            && !pf_match_port(
                                rr.dst.port_op.get(),
                                rr.dst.port[0].get(),
                                rr.dst.port[1].get(),
                                ctx.pd.ndport
                            ),
                        skip!(PF_SKIP_DST_PORT)
                    );
                    // tcp/udp only. uid.op always 0 in other cases.
                    if rr.uid.op != 0 && ctx.pd.lookup.done == 0 {
                        pf_socket_lookup(ctx.pd);
                        ctx.pd.lookup.done = 1;
                    }
                    pf_test_attrib!(
                        rr.uid.op != 0
                            && !pf_match_uid(
                                rr.uid.op,
                                rr.uid.uid[0],
                                rr.uid.uid[1],
                                ctx.pd.lookup.uid
                            ),
                        next
                    );
                    // tcp/udp only. gid.op always 0 in other cases.
                    if rr.gid.op != 0 && ctx.pd.lookup.done == 0 {
                        pf_socket_lookup(ctx.pd);
                        ctx.pd.lookup.done = 1;
                    }
                    pf_test_attrib!(
                        rr.gid.op != 0
                            && !pf_match_gid(
                                rr.gid.op,
                                rr.gid.gid[0],
                                rr.gid.gid[1],
                                ctx.pd.lookup.gid
                            ),
                        next
                    );
                }
                vp if i32::from(vp) == IPPROTO_ICMP => {
                    // icmp only. type always 0 in other cases.
                    pf_test_attrib!(
                        rr.type_ != 0 && rr.type_ != u16::from(ctx.icmptype) + 1,
                        next
                    );
                    // icmp only. type always 0 in other cases.
                    pf_test_attrib!(rr.code != 0 && rr.code != u16::from(ctx.icmpcode) + 1, next);
                    // icmp only. don't create states on replies.
                    pf_test_attrib!(
                        rr.keep_state != 0
                            && ctx.state_icmp == 0
                            && (rr.rule_flag.get() & PFRULE_STATESLOPPY) == 0
                            && ctx.icmp_dir != i32::from(PF_IN),
                        next
                    );
                }
                vp if i32::from(vp) == IPPROTO_ICMPV6 => {
                    // icmp only. type always 0 in other cases.
                    pf_test_attrib!(
                        rr.type_ != 0 && rr.type_ != u16::from(ctx.icmptype) + 1,
                        next
                    );
                    // icmp only. type always 0 in other cases.
                    pf_test_attrib!(rr.code != 0 && rr.code != u16::from(ctx.icmpcode) + 1, next);
                    // icmp only. don't create states on replies.
                    pf_test_attrib!(
                        rr.keep_state != 0
                            && ctx.state_icmp == 0
                            && (rr.rule_flag.get() & PFRULE_STATESLOPPY) == 0
                            && ctx.icmp_dir != i32::from(PF_IN)
                            && ctx.icmptype != crate::netinet::icmp6::ND_NEIGHBOR_ADVERT,
                        next
                    );
                }
                _ => {}
            }

            pf_test_attrib!(
                rr.rule_flag.get() & PFRULE_FRAGMENT != 0
                    && ctx.pd.virtual_proto != PF_VPROTO_FRAGMENT,
                next
            );
            pf_test_attrib!(rr.tos != 0 && rr.tos != ctx.pd.tos, next);
            pf_test_attrib!(
                rr.prob != 0 && rr.prob <= arc4random_uniform(u32::MAX - 1) + 1,
                next
            );
            let Some(m) = ctx.pd.m else {
                break 'rules;
            };
            pf_test_attrib!(
                rr.match_tag != 0 && !pf_match_tag(m, rr, &mut ctx.tag),
                next
            );
            pf_test_attrib!(
                rr.rcv_kif().is_some() && pf_match_rcvif(m, rr) == (rr.rcvifnot != 0),
                next
            );
            pf_test_attrib!(
                rr.prio != 0
                    && (if rr.prio == PF_PRIO_ZERO { 0 } else { rr.prio })
                        != m.m_pkthdr().pf.prio.get(),
                next
            );

            if u32::from(rr.statelim.id) != PF_STATELIM_ID_NONE {
                stlim = pf_statelim_find(u32::from(rr.statelim.id));

                // Treat a missing limiter like an exhausted limiter. There is no "backend"
                // to get a resource out of so the rule can't create state.
                pf_test_attrib!(stlim.is_none(), next);
                let Some(sl) = stlim else {
                    break 'rules;
                };

                // An overcommitted pool means this rule can't create state.
                if sl.pfstlim_inuse.get() >= sl.pfstlim_limit.get() {
                    let generation = pf_statelim_enter(sl);
                    pf_inc(&sl.pfstlim_counters.hardlimited);
                    pf_statelim_leave(sl, generation);
                    if rr.statelim.limiter_action == PF_LIMITER_BLOCK {
                        ctx.limiter_drop = 1;
                        reason_set(&mut ctx.reason, PFRES_MAXSTATES);
                        break 'rules; // stop rule processing
                    }

                    r = next;
                    continue;
                }

                // Is access to the pool rate limited?
                if sl.pfstlim_rate.limit.get() != 0 {
                    let ts = crate::kern::kern_tc::getnsecuptime();
                    let diff = ts.wrapping_sub(sl.pfstlim_rate_ts.get());

                    if diff < sl.pfstlim_rate_token.get() {
                        let generation = pf_statelim_enter(sl);
                        pf_inc(&sl.pfstlim_counters.ratelimited);
                        pf_statelim_leave(sl, generation);
                        if rr.statelim.limiter_action == PF_LIMITER_BLOCK {
                            ctx.limiter_drop = 1;
                            reason_set(&mut ctx.reason, PFRES_MAXSTATES);
                            break 'rules; // stop rule processing
                        }
                        r = next;
                        continue;
                    }

                    if diff > sl.pfstlim_rate_bucket.get() {
                        sl.pfstlim_rate_ts
                            .set(ts.wrapping_sub(sl.pfstlim_rate_bucket.get()));
                    }
                }
            }

            if u32::from(rr.sourcelim.id) != PF_SOURCELIM_ID_NONE {
                srlim = pf_sourcelim_find(u32::from(rr.sourcelim.id));

                // Treat a missing pool like an overcommitted pool. There is no "backend" to
                // get a resource out of so the rule can't create state.
                pf_test_attrib!(srlim.is_none(), next);
                let Some(sl) = srlim else {
                    break 'rules;
                };

                let key = PfSource::zeroed();
                let src = ctx.pd.ld_addr(ctx.pd.src);
                pf_source_key(sl, &key, ctx.pd.af, u32::from(ctx.pd.rdomain), &src);
                sr = pf_source_find(sl, &key);
                if let Some(s) = sr {
                    // An overcommitted limiter means this rule can't create state.
                    if s.pfsr_inuse.get() >= sl.pfsrlim_limit.get() {
                        pf_inc(&s.pfsr_counters.hardlimited);
                        let generation = pf_sourcelim_enter(sl);
                        pf_inc(&sl.pfsrlim_counters.hardlimited);
                        pf_sourcelim_leave(sl, generation);
                        if rr.sourcelim.limiter_action == PF_LIMITER_BLOCK {
                            ctx.limiter_drop = 1;
                            reason_set(&mut ctx.reason, PFRES_SRCLIMIT);
                            break 'rules; // stop rule processing
                        }
                        r = next;
                        continue;
                    }

                    // Is access to the pool rate limited?
                    if sl.pfsrlim_rate.limit.get() != 0 {
                        let ts = crate::kern::kern_tc::getnsecuptime();
                        let diff = ts.wrapping_sub(s.pfsr_rate_ts.get());

                        if diff < sl.pfsrlim_rate_token.get() {
                            pf_inc(&s.pfsr_counters.ratelimited);
                            let generation = pf_sourcelim_enter(sl);
                            pf_inc(&sl.pfsrlim_counters.ratelimited);
                            pf_sourcelim_leave(sl, generation);
                            if rr.sourcelim.limiter_action == PF_LIMITER_BLOCK {
                                ctx.limiter_drop = 1;
                                reason_set(&mut ctx.reason, PFRES_SRCLIMIT);
                                break 'rules; // stop rules
                            }
                            r = next;
                            continue;
                        }

                        if diff > sl.pfsrlim_rate_bucket.get() {
                            s.pfsr_rate_ts
                                .set(ts.wrapping_sub(sl.pfsrlim_rate_bucket.get()));
                        }
                    }
                } else {
                    // A new source entry will (should) admit a state.
                    if sl.pfsrlim_nsources.get() >= sl.pfsrlim_entries.get() {
                        let generation = pf_sourcelim_enter(sl);
                        pf_inc(&sl.pfsrlim_counters.addrlimited);
                        pf_sourcelim_leave(sl, generation);
                        if rr.sourcelim.limiter_action == PF_LIMITER_BLOCK {
                            ctx.limiter_drop = 1;
                            reason_set(&mut ctx.reason, PFRES_SRCLIMIT);
                            break 'rules; // stop rules processing
                        }
                        r = next;
                        continue;
                    }
                }
            }

            // Must be last!
            if rr.pktrate.limit.get() != 0 {
                pf_add_threshold(&rr.pktrate);
                pf_test_attrib!(pf_check_threshold(&rr.pktrate), next);
            }

            // FALLTHROUGH
            if rr.tag != 0 {
                ctx.tag = i32::from(rr.tag);
            }
            match rr.anchor() {
                None => {
                    if rr.rule_flag.get() & PFRULE_ONCE != 0 {
                        let rule_flag = rr.rule_flag.get();
                        // atomic_cas_uint: a compare and a store, both under `pf_lock`
                        // (exclusive), which every writer of `rule_flag` holds.
                        if rule_flag & PFRULE_EXPIRED == 0 && rr.rule_flag.get() == rule_flag {
                            rr.rule_flag.set(rule_flag | PFRULE_EXPIRED);
                            rr.exptime.set(gettime());
                        } else {
                            r = next;
                            continue;
                        }
                    }

                    if rr.action == PF_MATCH {
                        let Some(ri) = pf_pool_get::<PfRuleItem>(&PF_RULE_ITEM_PL, PR_NOWAIT)
                        else {
                            reason_set(&mut ctx.reason, PFRES_MEMORY);
                            return PfTestStatus::Fail;
                        };
                        ri.r_set(rr);
                        // Order is irrelevant.
                        // SAFETY: a fresh item onto the context's list.
                        unsafe { ctx.rules.insert_head(ri) };
                        ctx.ri = None;
                        pf_rule_to_actions(rr, &mut ctx.act);
                        if rr.rule_flag.get() & PFRULE_AFTO != 0 {
                            ctx.pd.naf = rr.naf;
                        }
                        if !crate::net::pf_lb::pf_get_transaddr(
                            rr,
                            ctx.pd,
                            &mut ctx.sns,
                            &mut ctx.nr,
                        ) {
                            reason_set(&mut ctx.reason, PFRES_TRANSLATE);
                            return PfTestStatus::Fail;
                        }
                        if rr.log != 0 {
                            reason_set(&mut ctx.reason, PFRES_MATCH);
                            crate::net::if_pflog::pflog_packet(
                                ctx.pd,
                                ctx.reason as u8,
                                rr,
                                ctx.a,
                                Some(ruleset),
                                None,
                            );
                        }
                    } else {
                        // Found matching r.
                        ctx.rm = Some(rr);
                        // Anchor, with ruleset, where r belongs to.
                        ctx.am = ctx.a;
                        // Ruleset where r belongs to.
                        ctx.rsm = Some(ruleset);
                        // Ruleset, where anchor belongs to.
                        ctx.arsm = ctx.aruleset;
                        // State/source pools.
                        ctx.statelim = stlim;
                        ctx.sourcelim = srlim;
                        ctx.source = sr;
                    }

                    if ctx.act.log & PF_LOG_MATCHES != 0 {
                        pf_log_matches(ctx.pd, rr, ctx.a, Some(ruleset), &ctx.rules);
                    }

                    if rr.quick != 0 {
                        return PfTestStatus::Quick;
                    }
                }
                Some(anchor) => {
                    ctx.aruleset = Some(&anchor.ruleset);
                    if rr.anchor_wildcard.get() != 0 {
                        // RB_FOREACH(child, pf_anchor_node, &r->anchor->children): the first
                        // child here, the next ones when the stack pops back (next_child).
                        if let Some(c) = anchor.children.min() {
                            if !pf_anchor_stack_push(
                                Some(ruleset),
                                ctx.a,
                                Some(rr),
                                Some(c),
                                PF_NEXT_CHILD,
                            ) {
                                return PfTestStatus::Fail;
                            }

                            ctx.a = Some(rr);
                            ruleset = &c.ruleset;
                            child = Some(c);
                            r = ruleset.active_ptr().first(); // enter_ruleset
                            continue;
                        }
                    } else {
                        if !pf_anchor_stack_push(
                            Some(ruleset),
                            ctx.a,
                            Some(rr),
                            child,
                            PF_NEXT_RULE,
                        ) {
                            return PfTestStatus::Fail;
                        }

                        ctx.a = Some(rr);
                        ruleset = &anchor.ruleset;
                        child = None;
                        r = ruleset.active_ptr().first(); // enter_ruleset
                        continue;
                    }
                }
            }
            r = next;
        }

        let Some(f) = pf_anchor_stack_pop() else {
            break;
        };
        let (Some(rs), Some(fr)) = (f.rs, f.r) else {
            break;
        };
        ruleset = rs;
        ctx.a = f.anchor;
        child = f.child;

        // Stop if any rule matched within quick anchors.
        if fr.quick != 0 && opt_eq(ctx.am, Some(fr)) {
            return PfTestStatus::Quick;
        }

        match f.jump_target {
            PF_NEXT_CHILD => {
                // next_child: continue with RB_FOREACH().
                if let Some(c) = child
                    && let Some(nc) = RbHead::<PfAnchorNode>::next(c)
                {
                    if !pf_anchor_stack_push(
                        Some(ruleset),
                        ctx.a,
                        Some(fr),
                        Some(nc),
                        PF_NEXT_CHILD,
                    ) {
                        return PfTestStatus::Fail;
                    }

                    ctx.a = Some(fr);
                    ruleset = &nc.ruleset;
                    child = Some(nc);
                    r = ruleset.active_ptr().first(); // enter_ruleset
                    continue;
                }
                r = TailqHead::<PfRulequeue>::next(fr);
            }
            PF_NEXT_RULE => {
                // next_rule:
                r = TailqHead::<PfRulequeue>::next(fr);
            }
            _ => panic(format_args!("pf_match_rule: unknown jump target")),
        }
    }

    PfTestStatus::Ok
}

/// `pf_test_rule`: evaluates the ruleset for a packet without a state and creates one if
/// the matching rule keeps state. Returns the action; `rm`, `sm`, `am`, `rsm` get the
/// matching rule, the new state, the anchor rule and the ruleset.
pub fn pf_test_rule(
    pd: &mut PfPdesc,
    rm: &mut Option<&'static PfRule>,
    sm: &mut Option<&'static PfState>,
    am: &mut Option<&'static PfRule>,
    rsm: &mut Option<&'static PfRuleset>,
    reason: &mut u16,
) -> u8 {
    let mut skw: Option<&'static PfStateKey> = None;
    let mut sks: Option<&'static PfStateKey> = None;
    let mut rewrite = 0;
    let mut virtual_type: u16 = 0;
    let mut virtual_id: u16 = 0;
    let mut action = PF_DROP;

    pf_assert_locked();

    let rdomain = pd.rdomain;
    let mut ctx = PfTestCtx {
        pd,
        act: PfRuleActions {
            rtableid: i32::from(rdomain),
            ..PfRuleActions::default()
        },
        icmpcode: 0,
        icmptype: 0,
        icmp_dir: 0,
        state_icmp: 0,
        tag: -1,
        limiter_drop: 0,
        reason: 0,
        ri: None,
        sns: [None; PF_SN_MAX],
        rules: SlistHead::new(),
        nr: None,
        rm: *rm,
        a: None,
        am: *am,
        rsm: *rsm,
        arsm: None,
        aruleset: None,
        statelim: None,
        sourcelim: None,
        source: None,
    };

    if ctx.pd.dir == PF_IN && crate::net::if_::if_congested() {
        reason_set(&mut ctx.reason, PFRES_CONGEST);
        return PF_DROP;
    }

    // The type and code of an ICMP or ICMPv6 message.
    let icmp_tc = match i32::from(ctx.pd.virtual_proto) {
        IPPROTO_ICMP => {
            let icmp = *ctx.pd.icmp();
            Some((icmp.icmp_type, icmp.icmp_code))
        }
        #[cfg(feature = "inet6")]
        IPPROTO_ICMPV6 => {
            let icmp6 = *ctx.pd.icmp6();
            Some((icmp6.icmp6_type, icmp6.icmp6_code))
        }
        _ => None,
    };
    if let Some((icmptype, icmpcode)) = icmp_tc {
        ctx.icmptype = icmptype;
        ctx.icmpcode = icmpcode;
        ctx.state_icmp = i32::from(pf_icmp_mapping(
            ctx.pd,
            ctx.icmptype,
            &mut ctx.icmp_dir,
            &mut virtual_id,
            &mut virtual_type,
        ));
        if ctx.icmp_dir == i32::from(PF_IN) {
            ctx.pd.osport = virtual_id;
            ctx.pd.nsport = virtual_id;
            ctx.pd.odport = virtual_type;
            ctx.pd.ndport = virtual_type;
        } else {
            ctx.pd.osport = virtual_type;
            ctx.pd.nsport = virtual_type;
            ctx.pd.odport = virtual_id;
            ctx.pd.ndport = virtual_id;
        }
    }

    let main = crate::net::pf_ruleset::pf_main_ruleset();
    let rv = pf_match_rule(&mut ctx, main);
    *rm = ctx.rm;
    *am = ctx.am;
    *rsm = ctx.rsm;
    'cleanup: {
        if rv == PfTestStatus::Fail || ctx.limiter_drop == 1 {
            reason_set(reason, ctx.reason);
            break 'cleanup;
        }

        let Some(r) = ctx.rm else {
            break 'cleanup;
        }; // matching rule
        let a = ctx.am; // rule that defines an anchor containing 'r'
        let ruleset = ctx.rsm; // ruleset of the anchor defined by the rule 'a'
        ctx.aruleset = ctx.arsm; // ruleset of the 'a' rule itself

        // Apply actions for last matching pass/block rule.
        pf_rule_to_actions(r, &mut ctx.act);
        if r.rule_flag.get() & PFRULE_AFTO != 0 {
            ctx.pd.naf = r.naf;
        }
        if !crate::net::pf_lb::pf_get_transaddr(r, ctx.pd, &mut ctx.sns, &mut ctx.nr) {
            reason_set(&mut ctx.reason, PFRES_TRANSLATE);
            break 'cleanup;
        }
        reason_set(&mut ctx.reason, PFRES_MATCH);

        if r.log != 0 {
            crate::net::if_pflog::pflog_packet(ctx.pd, ctx.reason as u8, r, a, ruleset, None);
        }
        if ctx.act.log & PF_LOG_MATCHES != 0 {
            pf_log_matches(ctx.pd, r, a, ruleset, &ctx.rules);
        }

        let th = *ctx.pd.tcp();
        if ctx.pd.virtual_proto != PF_VPROTO_FRAGMENT
            && r.action == PF_DROP
            && (r.rule_flag.get() & (PFRULE_RETURNRST | PFRULE_RETURNICMP | PFRULE_RETURN)) != 0
        {
            if i32::from(ctx.pd.proto) == IPPROTO_TCP
                && (r.rule_flag.get() & (PFRULE_RETURNRST | PFRULE_RETURN)) != 0
                && th.th_flags & TH_RST == 0
            {
                let mut ack = u32::from_be(th.th_seq).wrapping_add(ctx.pd.p_len);

                let Some(m) = ctx.pd.m else {
                    break 'cleanup;
                };
                if pf_check_tcp_cksum(
                    m,
                    ctx.pd.off as i32,
                    (ctx.pd.tot_len - u64::from(ctx.pd.off)) as i32,
                    ctx.pd.af,
                ) {
                    reason_set(&mut ctx.reason, PFRES_PROTCKSUM);
                } else {
                    if th.th_flags & TH_SYN != 0 {
                        ack = ack.wrapping_add(1);
                    }
                    if th.th_flags & TH_FIN != 0 {
                        ack = ack.wrapping_add(1);
                    }
                    let s = ctx.pd.ld_addr(ctx.pd.src);
                    let d = ctx.pd.ld_addr(ctx.pd.dst);
                    pf_send_tcp(
                        Some(r),
                        ctx.pd.af,
                        &d,
                        &s,
                        th.th_dport,
                        th.th_sport,
                        u32::from_be(th.th_ack),
                        ack,
                        TH_RST | TH_ACK,
                        0,
                        0,
                        r.return_ttl,
                        true,
                        0,
                        u32::from(ctx.pd.rdomain),
                        Some(&mut ctx.reason),
                    );
                }
            } else if (i32::from(ctx.pd.proto) != IPPROTO_ICMP
                || crate::netinet::ip_icmp::icmp_infotype(ctx.icmptype))
                && ctx.pd.af == AF_INET
                && r.return_icmp != 0
                && let Some(m) = ctx.pd.m
            {
                pf_send_icmp(
                    m,
                    (r.return_icmp >> 8) as u8,
                    (r.return_icmp & 255) as u8,
                    0,
                    ctx.pd.af,
                    Some(r),
                    u32::from(ctx.pd.rdomain),
                );
            } else if (i32::from(ctx.pd.proto) != IPPROTO_ICMPV6
                || (ctx.icmptype >= crate::netinet::icmp6::ICMP6_ECHO_REQUEST
                    && ctx.icmptype != crate::netinet::icmp6::ND_REDIRECT))
                && ctx.pd.af == AF_INET6
                && r.return_icmp6 != 0
                && let Some(m) = ctx.pd.m
            {
                pf_send_icmp(
                    m,
                    (r.return_icmp6 >> 8) as u8,
                    (r.return_icmp6 & 255) as u8,
                    0,
                    ctx.pd.af,
                    Some(r),
                    u32::from(ctx.pd.rdomain),
                );
            }
        }

        if r.action == PF_DROP {
            break 'cleanup;
        }

        if let Some(m) = ctx.pd.m {
            pf_tag_packet(m, ctx.tag, ctx.act.rtableid);
        }
        if ctx.act.rtableid >= 0 && rtable_l2(ctx.act.rtableid as u32) != u32::from(ctx.pd.rdomain)
        {
            ctx.pd.destchg = 1;
        }

        if r.action == PF_PASS && ctx.pd.badopts != 0 && r.allow_opts == 0 {
            reason_set(&mut ctx.reason, PFRES_IPOPTIONS);
            ctx.pd.pflog |= PF_LOG_FORCE;
            crate::dpfprintf!(
                LOG_NOTICE,
                "dropping packet with ip/ipv6 options in pf_test_rule()"
            );
            break 'cleanup;
        }

        if ctx.pd.virtual_proto != PF_VPROTO_FRAGMENT && ctx.state_icmp == 0 && r.keep_state != 0 {
            if r.rule_flag.get() & PFRULE_SRCTRACK != 0 {
                let src = ctx.pd.ld_addr(ctx.pd.src);
                let af = ctx.pd.af;
                if !pf_insert_src_node(
                    &mut ctx.sns[PF_SN_NONE],
                    r,
                    PF_SN_NONE,
                    af,
                    &src,
                    None,
                    None,
                ) {
                    reason_set(&mut ctx.reason, PFRES_SRCLIMIT);
                    break 'cleanup;
                }
            }

            if r.max_states != 0 && r.states_cur.get() >= r.max_states {
                pf_inc(&PF_STATUS.lcounters[LCNT_STATES]);
                reason_set(&mut ctx.reason, PFRES_MAXSTATES);
                break 'cleanup;
            }

            action = pf_create_state(r, a, &mut skw, &mut sks, &mut rewrite, sm, &mut ctx);

            if action != PF_PASS {
                break 'cleanup;
            }

            if i32::from(ctx.pd.proto) == IPPROTO_TCP
                && r.keep_state == PF_STATE_SYNPROXY
                && ctx.pd.dir == PF_IN
            {
                let act = ctx.act;
                action = pf_synproxy_ack(r, ctx.pd, sm, &act);
                if action != PF_PASS {
                    return action; // PF_SYNPROXY_DROP
                }
            }

            if let (Some(w), Some(s)) = (skw, sks)
                && !ptr::eq(w, s)
            {
                let sk = if ctx.pd.dir == PF_IN { s } else { w };
                let same = ctx.pd.af == ctx.pd.naf;
                let (si, di) = (usize::from(ctx.pd.sidx), usize::from(ctx.pd.didx));
                let (a1, a2) = if same { (si, di) } else { (di, si) };
                let icmp_dir = ctx.icmp_dir;
                rewrite += pf_translate(
                    ctx.pd,
                    &sk.addr[a1].get(),
                    sk.port[a1].get(),
                    &sk.addr[a2].get(),
                    sk.port[a2].get(),
                    virtual_type,
                    icmp_dir,
                );
            }

            #[cfg(feature = "inet6")]
            if rewrite != 0
                && let (Some(w), Some(s)) = (skw, sks)
                && w.af.get() != s.af.get()
            {
                action = PF_AFRT;
            }
        } else {
            action = PF_PASS;

            while let Some(ri) = ctx.rules.first() {
                // SAFETY: the context's own list.
                unsafe { ctx.rules.remove_head() };
                pf_pool_put(&PF_RULE_ITEM_PL, ri);
            }
        }

        // Copy back packet headers if needed.
        if rewrite != 0
            && ctx.pd.hdrlen != 0
            && let Some(m) = ctx.pd.m
        {
            let hdrlen = ctx.pd.hdrlen as usize;
            let off = ctx.pd.off as i32;
            let _ = (m, off, hdrlen);
            pf_copyback_hdr(ctx.pd);
        }

        if let Some(s) = *sm
            && s.state_flags.get() & PFSTATE_NOSYNC == 0
            && ctx.pd.dir == PF_OUT
            && crate::net::if_pfsync::pfsync_is_up()
            && let Some(m) = ctx.pd.m
        {
            // We want the state created, but we dont want to send this in case a partner
            // firewall has to know about it to allow replies through it.
            if crate::net::if_pfsync::pfsync_defer(s, m) {
                return PF_DEFER;
            }
        }

        return action;
    }

    // cleanup:
    while let Some(ri) = ctx.rules.first() {
        // SAFETY: the context's own list.
        unsafe { ctx.rules.remove_head() };
        pf_pool_put(&PF_RULE_ITEM_PL, ri);
    }

    action
}

/// `pf_create_state`: creates the state of a packet that matched a keep-state rule.
fn pf_create_state(
    r: &'static PfRule,
    a: Option<&'static PfRule>,
    skw: &mut Option<&'static PfStateKey>,
    sks: &mut Option<&'static PfStateKey>,
    rewrite: &mut i32,
    sm: &mut Option<&'static PfState>,
    ctx: &mut PfTestCtx<'_>,
) -> u8 {
    let mut reason: u16 = 0;
    let tag = ctx.tag;
    let nr = ctx.nr;
    let act = ctx.act;
    let mut stlim: Option<&'static PfStatelim> = None;
    let mut srlim: Option<&'static PfSourcelim> = None;
    let mut sr: Option<&'static PfSource> = None;

    let st = pf_pool_get::<PfState>(&PF_STATE_PL, PR_NOWAIT);
    'csfailed: {
        let Some(st) = st else {
            reason_set(&mut reason, PFRES_MEMORY);
            break 'csfailed;
        };
        let pd = &mut *ctx.pd;
        st.rule.set_ptr(Some(r));
        st.anchor.set_ptr(a);
        st.natrule.set_ptr(nr);
        let mut sf = 0;
        if r.allow_opts != 0 {
            sf |= PFSTATE_ALLOWOPTS;
        }
        if r.rule_flag.get() & PFRULE_STATESLOPPY != 0 {
            sf |= PFSTATE_SLOPPY;
        }
        if r.rule_flag.get() & PFRULE_PFLOW != 0 {
            sf |= PFSTATE_PFLOW;
        }
        if r.rule_flag.get() & PFRULE_NOSYNC != 0 {
            sf |= PFSTATE_NOSYNC;
        }
        st.log.set(act.log & PF_LOG_ALL);
        st.qid.set(act.qid);
        st.pqid.set(act.pqid);
        st.rtableid[usize::from(pd.didx)].set(act.rtableid);
        st.rtableid[usize::from(pd.sidx)].set(-1); // return traffic is routed normally
        st.min_ttl.set(act.min_ttl);
        st.set_tos.set(act.set_tos);
        st.max_mss.set(act.max_mss);
        st.state_flags.set(sf | act.flags);
        st.sync_state.set(crate::net::if_pfsync::PFSYNC_S_NONE);
        st.set_prio[0].set(act.set_prio[0]);
        st.set_prio[1].set(act.set_prio[1]);
        st.delay.set(act.delay);
        st.src_nodes.init();
        st.linkage.init();

        // Must initialize refcnt, before pf_state_insert() gets called. pf_state_insert()
        // grabs a reference for pfsync!
        refcnt_init(&st.refcnt);
        mtx_init(&st.mtx, IPL_NET);

        let th = *pd.tcp();
        match i32::from(pd.proto) {
            IPPROTO_TCP => {
                st.src.seqlo.set(u32::from_be(th.th_seq));
                st.src
                    .seqhi
                    .set(st.src.seqlo.get().wrapping_add(pd.p_len).wrapping_add(1));
                if (th.th_flags & (TH_SYN | TH_ACK)) == TH_SYN && r.keep_state == PF_STATE_MODULATE
                {
                    // Generate sequence number modulator.
                    let mut d = pf_tcp_iss(pd).wrapping_sub(st.src.seqlo.get());
                    if d == 0 {
                        d = 1;
                    }
                    st.src.seqdiff.set(d);
                    pf_patch_32(
                        pd,
                        PfLoc::Hdr(TH_SEQ_OFF),
                        st.src.seqlo.get().wrapping_add(d).to_be(),
                    );
                    *rewrite = 1;
                } else {
                    st.src.seqdiff.set(0);
                }
                if th.th_flags & TH_SYN != 0 {
                    st.src.seqhi.set(st.src.seqhi.get().wrapping_add(1));
                    st.src.wscale.set(pf_get_wscale(pd));
                }
                st.src
                    .max_win
                    .set(core::cmp::max(u16::from_be(th.th_win), 1));
                if st.src.wscale.get() & PF_WSCALE_MASK != 0 {
                    // Remove scale factor from initial window.
                    let sc = u32::from(st.src.wscale.get() & PF_WSCALE_MASK);
                    let mut win = u32::from(st.src.max_win.get());
                    win += 1 << sc;
                    st.src.max_win.set(((win - 1) >> sc) as u16);
                }
                if th.th_flags & TH_FIN != 0 {
                    st.src.seqhi.set(st.src.seqhi.get().wrapping_add(1));
                }
                st.dst.seqhi.set(1);
                st.dst.max_win.set(1);
                pf_set_protostate(st, PF_PEER_SRC, TCPS_SYN_SENT as u8);
                pf_set_protostate(st, PF_PEER_DST, TCPS_CLOSED as u8);
                st.timeout.set(PFTM_TCP_FIRST_PACKET as u8);
                pf_inc(&PF_STATUS.states_halfopen);
            }
            IPPROTO_UDP => {
                pf_set_protostate(st, PF_PEER_SRC, PFUDPS_SINGLE);
                pf_set_protostate(st, PF_PEER_DST, PFUDPS_NO_TRAFFIC);
                st.timeout.set(PFTM_UDP_FIRST_PACKET as u8);
            }
            IPPROTO_ICMP => {
                st.timeout.set(PFTM_ICMP_FIRST_PACKET as u8);
            }
            #[cfg(feature = "inet6")]
            IPPROTO_ICMPV6 => {
                st.timeout.set(PFTM_ICMP_FIRST_PACKET as u8);
            }
            _ => {
                pf_set_protostate(st, PF_PEER_SRC, PFOTHERS_SINGLE);
                pf_set_protostate(st, PF_PEER_DST, PFOTHERS_NO_TRAFFIC);
                st.timeout.set(PFTM_OTHER_FIRST_PACKET as u8);
            }
        }

        let now = getuptime() as i32;
        st.creation.set(now);
        st.expire.set(now);

        if i32::from(pd.proto) == IPPROTO_TCP {
            if st.state_flags.get() & PFSTATE_SCRUB_TCP != 0
                && crate::net::pf_norm::pf_normalize_tcp_init(pd, &st.src).is_err()
            {
                reason_set(&mut reason, PFRES_MEMORY);
                break 'csfailed;
            }
            if st.state_flags.get() & PFSTATE_SCRUB_TCP != 0
                && st.src.scrub.get().is_some()
                && crate::net::pf_norm::pf_normalize_tcp_stateful(
                    pd,
                    &mut reason,
                    st,
                    &st.src,
                    &st.dst,
                    rewrite,
                ) != 0
            {
                // This really shouldn't happen!!!
                crate::dpfprintf!(
                    LOG_ERR,
                    "pf_create_state: tcp normalize failed on first pkt"
                );
                break 'csfailed;
            }
        }
        st.direction.set(pd.dir);

        if pf_state_key_setup(pd, skw, sks, act.rtableid).is_err() {
            reason_set(&mut reason, PFRES_MEMORY);
            break 'csfailed;
        }

        let src = pd.ld_addr(pd.src);
        let wkaf = skw.map_or(pd.af, |k| k.af.get());
        if !pf_set_rt_ifp(st, &src, wkaf, &mut ctx.sns) {
            reason_set(&mut reason, PFRES_NOROUTE);
            break 'csfailed;
        }

        for sn in ctx.sns.iter().flatten() {
            let Some(sni) = pf_pool_get::<PfSnItem>(&PF_SN_ITEM_PL, PR_NOWAIT) else {
                reason_set(&mut reason, PFRES_MEMORY);
                break 'csfailed;
            };
            sni.sn_set(sn);
            // SAFETY: a fresh item onto the new state's list.
            unsafe { st.src_nodes.insert_head(sni) };
            sn.states.set(sn.states.get().wrapping_add(1));
        }

        stlim = ctx.statelim;
        if let Some(sl) = stlim {
            pf_assert_locked();
            let Some(pfl) = pf_pool_get::<PfStateLink>(&PF_STATE_LINK_PL, PR_NOWAIT) else {
                reason_set(&mut reason, PFRES_MEMORY);
                break 'csfailed;
            };

            let generation = pf_statelim_enter(sl);
            pf_inc(&sl.pfstlim_counters.admitted);
            pf_inc(&sl.pfstlim_inuse);
            pf_statelim_leave(sl, generation);

            sl.pfstlim_rate_ts.set(
                sl.pfstlim_rate_ts
                    .get()
                    .wrapping_add(sl.pfstlim_rate_token.get()),
            );

            st.statelim.set(sl.pfstlim_id.get() as u8);
            pfl.pfl_state.set(Some(st));
            pfl.pfl_type.set(PF_STATE_LINK_TYPE_STATELIM);

            // SAFETY: a fresh link onto both lists.
            unsafe {
                sl.pfstlim_states.insert_tail(pfl);
                st.linkage.insert_head(pfl);
            }
        }

        srlim = ctx.sourcelim;
        if let Some(sl) = srlim {
            sr = ctx.source;
            let s = match sr {
                None => {
                    let Some(s) = pf_pool_get::<PfSource>(&PF_SOURCE_PL, PR_NOWAIT) else {
                        let generation = pf_sourcelim_enter(sl);
                        pf_inc(&sl.pfsrlim_counters.addrnomem);
                        pf_sourcelim_leave(sl, generation);
                        reason_set(&mut reason, PFRES_MEMORY);
                        break 'csfailed;
                    };

                    s.pfsr_parent.set(Some(sl));
                    let src = ctx.pd.ld_addr(ctx.pd.src);
                    pf_source_key(sl, s, ctx.pd.af, u32::from(ctx.pd.rdomain), &src);
                    s.pfsr_states.init();

                    // SAFETY: a fresh source onto the limiter's trees.
                    if unsafe { sl.pfsrlim_sources.insert(s) }.is_some() {
                        panic(format_args!(
                            "pf_create_state: source pool {} ({:p}) insert collision {:p}?!",
                            sl.pfsrlim_id.get(),
                            sl,
                            s
                        ));
                    }

                    // SAFETY: as above.
                    if unsafe { sl.pfsrlim_ioc_sources.insert(s) }.is_some() {
                        panic(format_args!(
                            "pf_create_state: source pool {} ({:p}) ioc insert collision ({:p})?!",
                            sl.pfsrlim_id.get(),
                            sl,
                            s
                        ));
                    }

                    s.pfsr_empty_ts.set(getuptime());
                    // SAFETY: a new, unused source goes on the gc list.
                    unsafe { PF_SOURCE_GC.insert_tail(s) };

                    let generation = pf_sourcelim_enter(sl);
                    pf_inc(&sl.pfsrlim_nsources);
                    pf_inc(&sl.pfsrlim_counters.addrallocs);
                    pf_sourcelim_leave(sl, generation);
                    sr = Some(s);
                    s
                }
                Some(s) => {
                    kassert!(opt_eq(s.pfsr_parent.get(), Some(sl)));
                    s
                }
            };

            pf_assert_locked();
            let Some(pfl) = pf_pool_get::<PfStateLink>(&PF_STATE_LINK_PL, PR_NOWAIT) else {
                reason_set(&mut reason, PFRES_MEMORY);
                break 'csfailed;
            };

            pf_source_used(s);

            pf_inc(&s.pfsr_counters.admitted);

            let generation = pf_sourcelim_enter(sl);
            pf_inc(&sl.pfsrlim_counters.inuse);
            pf_inc(&sl.pfsrlim_counters.admitted);
            pf_sourcelim_leave(sl, generation);

            st.sourcelim.set(sl.pfsrlim_id.get() as u8);
            pfl.pfl_state.set(Some(st));
            pfl.pfl_type.set(PF_STATE_LINK_TYPE_SOURCELIM);

            // SAFETY: a fresh link onto both lists.
            unsafe {
                s.pfsr_states.insert_tail(pfl);
                st.linkage.insert_head(pfl);
            }
        }

        crate::net::if_pfsync::pfsync_init_state(st, *skw, *sks, 0);

        let pd = &mut *ctx.pd;
        let Some(kif) = bound_iface(r, pd.kif) else {
            reason_set(&mut reason, PFRES_STATEINS);
            break 'csfailed;
        };
        if !pf_state_insert(kif, skw, sks, st) {
            *sks = None;
            *skw = None;
            reason_set(&mut reason, PFRES_STATEINS);
            break 'csfailed;
        }
        *sm = Some(st);

        // Make state responsible for rules it binds here.
        while let Some(ri) = ctx.rules.first() {
            // SAFETY: moved from the context's list to the new state's, one by one (the
            // C's structure copy of the head; the order of match rules is irrelevant).
            unsafe {
                ctx.rules.remove_head();
                st.match_rules.insert_head(ri);
            }
        }
        state_inc_counters(st);

        if tag > 0 {
            crate::net::pf_ioctl::pf_tag_ref(tag as u16);
            st.tag.set(tag as u16);
        }
        let th = *pd.tcp();
        if i32::from(pd.proto) == IPPROTO_TCP
            && (th.th_flags & (TH_SYN | TH_ACK)) == TH_SYN
            && r.keep_state == PF_STATE_SYNPROXY
            && pd.dir == PF_IN
        {
            let rtid = if act.rtableid >= 0 {
                act.rtableid
            } else {
                i32::from(pd.rdomain)
            };
            pf_set_protostate(st, PF_PEER_SRC, PF_TCPS_PROXY_SRC);
            st.src.seqhi.set(arc4random());
            // Find mss option.
            let mssdflt = tcp_mssdflt();
            let mut mss = pf_get_mss(pd, mssdflt);
            let s = pd.ld_addr(pd.src);
            let d = pd.ld_addr(pd.dst);
            mss = pf_calc_mss(&s, pd.af, rtid, mss, mssdflt);
            mss = pf_calc_mss(&d, pd.af, rtid, mss, mssdflt);
            st.src.mss.set(mss);
            pf_send_tcp(
                Some(r),
                pd.af,
                &d,
                &s,
                th.th_dport,
                th.th_sport,
                st.src.seqhi.get(),
                u32::from_be(th.th_seq).wrapping_add(1),
                TH_SYN | TH_ACK,
                0,
                st.src.mss.get(),
                0,
                true,
                0,
                u32::from(pd.rdomain),
                Some(&mut reason),
            );
            reason_set(&mut reason, PFRES_SYNPROXY);
            return PF_SYNPROXY_DROP;
        }

        return PF_PASS;
    }

    // csfailed:
    if let Some(st) = st {
        for pfl in st.linkage.iter() {
            // "Who needs KASSERTS when we have NULL derefs."
            let list: &TailqHead<PfStateLinkList> = match pfl.pfl_type.get() {
                PF_STATE_LINK_TYPE_STATELIM => {
                    let Some(sl) = stlim else {
                        panic(format_args!(
                            "pf_create_state: link without a state limiter"
                        ))
                    };
                    let generation = pf_statelim_enter(sl);
                    sl.pfstlim_inuse.set(sl.pfstlim_inuse.get().wrapping_sub(1));
                    pf_statelim_leave(sl, generation);

                    sl.pfstlim_rate_ts.set(
                        sl.pfstlim_rate_ts
                            .get()
                            .wrapping_sub(sl.pfstlim_rate_token.get()),
                    );
                    &sl.pfstlim_states
                }
                PF_STATE_LINK_TYPE_SOURCELIM => {
                    let (Some(sl), Some(s)) = (srlim, sr) else {
                        panic(format_args!("pf_create_state: link without a source"))
                    };
                    let generation = pf_sourcelim_enter(sl);
                    let c = &sl.pfsrlim_counters.inuse;
                    c.set(c.get().wrapping_sub(1));
                    pf_sourcelim_leave(sl, generation);

                    s.pfsr_rate_ts.set(
                        s.pfsr_rate_ts
                            .get()
                            .wrapping_sub(sl.pfsrlim_rate_token.get()),
                    );
                    pf_source_rele(s);

                    &s.pfsr_states
                }
                t => panic(format_args!(
                    "pf_create_state: unexpected link type {t} on pfl {pfl:p}"
                )),
            };

            // SAFETY: the link was put on that list above.
            unsafe { list.remove(pfl) };
            pf_assert_locked();
            pf_pool_put(&PF_STATE_LINK_PL, pfl);
        }

        crate::net::pf_norm::pf_normalize_tcp_cleanup(st); // safe even w/o init
        pf_src_tree_remove_state(st);
        pf_pool_put(&PF_STATE_PL, st);
    }

    for sn in ctx.sns.iter().flatten() {
        pf_remove_src_node(sn);
    }

    PF_DROP
}

/// `pf_translate`: rewrites the packet's addresses and ports to the state key's. Returns
/// the number of rewrites.
pub fn pf_translate(
    pd: &mut PfPdesc,
    saddr: &PfAddr,
    sport: u16,
    daddr: &PfAddr,
    dport: u16,
    virtual_type: u16,
    icmp_dir: i32,
) -> i32 {
    let mut rewrite = 0;
    let afto = pd.af != pd.naf;

    let dst = pd.ld_addr(pd.dst);
    if afto || pf_aneq(daddr, &dst, pd.af) {
        pd.destchg = 1;
    }

    match i32::from(pd.proto) {
        IPPROTO_TCP | IPPROTO_UDP => {
            let (sp, dp) = (pd.sport, pd.dport);
            rewrite += pf_patch_16(pd, sp, sport);
            rewrite += pf_patch_16(pd, dp, dport);
        }
        IPPROTO_ICMP => {
            if pd.af != AF_INET {
                return 0;
            }

            #[cfg(feature = "inet6")]
            if afto {
                if pf_translate_icmp_af(pd, AF_INET6, PfLoc::Hdr(0)) != 0 {
                    return 0;
                }
                pd.proto = IPPROTO_ICMPV6 as u8;
                rewrite = 1;
            }
            if virtual_type == u16::from(crate::netinet::ip_icmp::ICMP_ECHO).to_be() {
                let icmpid = if icmp_dir == i32::from(PF_IN) {
                    sport
                } else {
                    dport
                };
                rewrite += pf_patch_16(pd, PfLoc::Hdr(ICMP_ID_OFF), icmpid);
            }
        }
        #[cfg(feature = "inet6")]
        IPPROTO_ICMPV6 => {
            if pd.af != AF_INET6 {
                return 0;
            }

            if afto {
                if pf_translate_icmp_af(pd, AF_INET, PfLoc::Hdr(0)) != 0 {
                    return 0;
                }
                pd.proto = IPPROTO_ICMP as u8;
                rewrite = 1;
            }
            if virtual_type == u16::from(crate::netinet::icmp6::ICMP6_ECHO_REQUEST).to_be() {
                let icmpid = if icmp_dir == i32::from(PF_IN) {
                    sport
                } else {
                    dport
                };
                // icmp6_id is at the offset of icmp_id.
                rewrite += pf_patch_16(pd, PfLoc::Hdr(ICMP_ID_OFF), icmpid);
            }
        }
        _ => {}
    }

    if !afto {
        let (s, d) = (pd.src, pd.dst);
        rewrite += pf_translate_a(pd, s, saddr);
        rewrite += pf_translate_a(pd, d, daddr);
    }

    rewrite
}

/// `MAXACKWINDOW`: 1500 is an arbitrary fudge factor.
const MAXACKWINDOW: i64 = 0xffff + 1500;

/// The state's peers as seen by this packet: `(src, dst, psrc, pdst)`.
fn pf_state_peers(
    st: &'static PfState,
    forward: bool,
) -> (&'static PfStatePeer, &'static PfStatePeer, i32, i32) {
    if forward {
        (&st.src, &st.dst, PF_PEER_SRC, PF_PEER_DST)
    } else {
        (&st.dst, &st.src, PF_PEER_DST, PF_PEER_SRC)
    }
}

/// The state timeout for the peers' TCP states.
fn pf_tcp_update_timeout(st: &'static PfState, src: &PfStatePeer, dst: &PfStatePeer) {
    let (ss, ds) = (i32::from(src.state.get()), i32::from(dst.state.get()));
    if ss >= TCPS_FIN_WAIT_2 && ds >= TCPS_FIN_WAIT_2 {
        pf_update_state_timeout(st, PFTM_TCP_CLOSED);
    } else if ss >= TCPS_CLOSING && ds >= TCPS_CLOSING {
        pf_update_state_timeout(st, PFTM_TCP_FIN_WAIT);
    } else if ss < TCPS_ESTABLISHED || ds < TCPS_ESTABLISHED {
        pf_update_state_timeout(st, PFTM_TCP_OPENING);
    } else if ss >= TCPS_CLOSING || ds >= TCPS_CLOSING {
        pf_update_state_timeout(st, PFTM_TCP_CLOSING);
    } else {
        pf_update_state_timeout(st, PFTM_TCP_ESTABLISHED);
    }
}

/// `pf_tcp_track_full`: the full TCP sequence tracking of Guido van Rooij's paper
/// ("Real Stateful TCP Packet Filtering in IP Filter").
pub fn pf_tcp_track_full(
    pd: &mut PfPdesc,
    st: &'static PfState,
    reason: &mut u16,
    copyback: &mut i32,
    reverse: bool,
) -> u8 {
    let th = *pd.tcp();
    let mut win = u32::from(u16::from_be(th.th_win));

    let forward =
        (pd.dir == st.direction.get() && !reverse) || (pd.dir != st.direction.get() && reverse);
    let (src, dst, psrc, pdst) = pf_state_peers(st, forward);

    let (mut sws, mut dws) =
        if src.wscale.get() != 0 && dst.wscale.get() != 0 && th.th_flags & TH_SYN == 0 {
            (
                u32::from(src.wscale.get() & PF_WSCALE_MASK),
                u32::from(dst.wscale.get() & PF_WSCALE_MASK),
            )
        } else {
            (0, 0)
        };

    // Sequence tracking algorithm from Guido van Rooij's paper:
    //   http://www.madison-gurkha.com/publications/tcp_filtering/tcp_filtering.ps
    let mut seq = u32::from_be(th.th_seq);
    let orig_seq = seq;
    let mut ack;
    let mut end;
    let mut data_end;
    if src.seqlo.get() == 0 {
        // First packet from this end. Set its state.

        if (st.state_flags.get() & PFSTATE_SCRUB_TCP != 0 || dst.scrub.get().is_some())
            && src.scrub.get().is_none()
            && crate::net::pf_norm::pf_normalize_tcp_init(pd, src).is_err()
        {
            reason_set(reason, PFRES_MEMORY);
            return PF_DROP;
        }

        // Deferred generation of sequence number modulator.
        if dst.seqdiff.get() != 0 && src.seqdiff.get() == 0 {
            // Use random iss for the TCP server.
            loop {
                let d = arc4random().wrapping_sub(seq);
                src.seqdiff.set(d);
                if d != 0 {
                    break;
                }
            }
            ack = u32::from_be(th.th_ack).wrapping_sub(dst.seqdiff.get());
            pf_patch_32(
                pd,
                PfLoc::Hdr(TH_SEQ_OFF),
                seq.wrapping_add(src.seqdiff.get()).to_be(),
            );
            pf_patch_32(pd, PfLoc::Hdr(TH_ACK_OFF), ack.to_be());
            *copyback = 1;
        } else {
            ack = u32::from_be(th.th_ack);
        }

        end = seq.wrapping_add(pd.p_len);
        if th.th_flags & TH_SYN != 0 {
            end = end.wrapping_add(1);
            if dst.wscale.get() & PF_WSCALE_FLAG != 0 {
                src.wscale.set(pf_get_wscale(pd));
                if src.wscale.get() & PF_WSCALE_FLAG != 0 {
                    // Remove scale factor from initial window.
                    sws = u32::from(src.wscale.get() & PF_WSCALE_MASK);
                    win = (win + (1 << sws) - 1) >> sws;
                    dws = u32::from(dst.wscale.get() & PF_WSCALE_MASK);
                } else {
                    // Fixup other window.
                    dst.max_win.set(core::cmp::min(
                        TCP_MAXWIN,
                        u32::from(dst.max_win.get()) << (dst.wscale.get() & PF_WSCALE_MASK),
                    ) as u16);
                    // In case of a retrans SYN|ACK.
                    dst.wscale.set(0);
                }
            }
        }
        data_end = end;
        if th.th_flags & TH_FIN != 0 {
            end = end.wrapping_add(1);
        }

        src.seqlo.set(seq);
        if i32::from(src.state.get()) < TCPS_SYN_SENT {
            pf_set_protostate(st, psrc, TCPS_SYN_SENT as u8);
        }

        // May need to slide the window (seqhi may have been set by the crappy stack check or
        // if we picked up the connection after establishment).
        let w = end.wrapping_add(core::cmp::max(1, u32::from(dst.max_win.get()) << dws));
        if src.seqhi.get() == 1 || seq_geq(w, src.seqhi.get()) {
            src.seqhi.set(w);
        }
        if win > u32::from(src.max_win.get()) {
            src.max_win.set(win as u16);
        }
    } else {
        ack = u32::from_be(th.th_ack).wrapping_sub(dst.seqdiff.get());
        if src.seqdiff.get() != 0 {
            // Modulate sequence numbers.
            pf_patch_32(
                pd,
                PfLoc::Hdr(TH_SEQ_OFF),
                seq.wrapping_add(src.seqdiff.get()).to_be(),
            );
            pf_patch_32(pd, PfLoc::Hdr(TH_ACK_OFF), ack.to_be());
            *copyback = 1;
        }
        end = seq.wrapping_add(pd.p_len);
        if th.th_flags & TH_SYN != 0 {
            end = end.wrapping_add(1);
        }
        data_end = end;
        if th.th_flags & TH_FIN != 0 {
            end = end.wrapping_add(1);
        }
    }
    let orig_ack = ack;

    if th.th_flags & TH_ACK == 0 {
        // Let it pass through the ack skew check.
        ack = dst.seqlo.get();
    } else if (ack == 0 && (th.th_flags & (TH_ACK | TH_RST)) == (TH_ACK | TH_RST))
        // broken tcp stacks do not set ack
        || i32::from(dst.state.get()) < TCPS_SYN_SENT
    {
        // Many stacks (ours included) will set the ACK number in an FIN|ACK if the SYN times
        // out -- no sequence to ACK.
        ack = dst.seqlo.get();
    }

    if seq == end {
        // Ease sequencing restrictions on no data packets.
        seq = src.seqlo.get();
        end = seq;
        data_end = seq;
    }

    let ackskew = i64::from(dst.seqlo.get().wrapping_sub(ack) as i32);

    // Need to demodulate the sequence numbers in any TCP SACK options (Selective ACK). We
    // could optionally validate the SACK values against the current ACK window, either
    // forwards or backwards, but I'm not confident that SACK has been implemented properly
    // everywhere. There really aren't any security implications of bad SACKing unless the
    // target stack doesn't validate the option length correctly.
    if dst.seqdiff.get() != 0
        && (usize::from(th.th_off()) << 2) > size_of::<Tcphdr>()
        && pf_modulate_sack(pd, dst) != 0
    {
        *copyback = 1;
    }

    let dmw = u32::from(dst.max_win.get()) << dws;
    let seqlo = src.seqlo.get();
    if seq_geq(src.seqhi.get(), data_end)
        // Last octet inside other's window space
        && seq_geq(seq, seqlo.wrapping_sub(dmw))
        // Retrans: not more than one window back
        && ackskew >= -MAXACKWINDOW
        // Acking not more than one reassembled fragment backwards
        && ackskew <= (MAXACKWINDOW << sws)
        // Acking not more than one window forward
        && (th.th_flags & TH_RST == 0
            || orig_seq == seqlo
            || orig_seq == seqlo.wrapping_add(1)
            || orig_seq.wrapping_add(1) == seqlo
            // Require an exact/+1 sequence match on resets when possible
            || (seq_geq(orig_seq, seqlo.wrapping_sub(dmw))
                && seq_leq(orig_seq, seqlo.wrapping_add(1))
                && orig_ack == dst.seqlo.get()
                && (th.th_flags & (TH_ACK | TH_RST)) == (TH_ACK | TH_RST)))
    {
        // Allow resets to match sequence window if ack is perfect match.

        if (dst.scrub.get().is_some() || src.scrub.get().is_some())
            && crate::net::pf_norm::pf_normalize_tcp_stateful(pd, reason, st, src, dst, copyback)
                != 0
        {
            return PF_DROP;
        }

        // Update max window.
        if u32::from(src.max_win.get()) < win {
            src.max_win.set(win as u16);
        }
        // Synchronize sequencing.
        if seq_gt(end, src.seqlo.get()) {
            src.seqlo.set(end);
        }
        // Slide the window of what the other end can send.
        if seq_geq(ack.wrapping_add(win << sws), dst.seqhi.get()) {
            dst.seqhi
                .set(ack.wrapping_add(core::cmp::max(win << sws, 1)));
        }

        // Update states.
        if th.th_flags & TH_SYN != 0 && i32::from(src.state.get()) < TCPS_SYN_SENT {
            pf_set_protostate(st, psrc, TCPS_SYN_SENT as u8);
        }
        if th.th_flags & TH_FIN != 0 && i32::from(src.state.get()) < TCPS_CLOSING {
            pf_set_protostate(st, psrc, TCPS_CLOSING as u8);
        }
        if th.th_flags & TH_ACK != 0 {
            if i32::from(dst.state.get()) == TCPS_SYN_SENT {
                pf_set_protostate(st, pdst, TCPS_ESTABLISHED as u8);
                if i32::from(src.state.get()) == TCPS_ESTABLISHED
                    && !st.src_nodes.is_empty()
                    && pf_src_connlimit(st)
                {
                    reason_set(reason, PFRES_SRCLIMIT);
                    return PF_DROP;
                }
            } else if i32::from(dst.state.get()) == TCPS_CLOSING {
                pf_set_protostate(st, pdst, TCPS_FIN_WAIT_2 as u8);
            }
        }
        if th.th_flags & TH_RST != 0 {
            pf_set_protostate(st, PF_PEER_BOTH, TCPS_TIME_WAIT as u8);
        }

        // Update expire time.
        st.expire.set(getuptime() as i32);
        pf_tcp_update_timeout(st, src, dst);

        // Fall through to PASS packet.
    } else if (i32::from(dst.state.get()) < TCPS_SYN_SENT
        || i32::from(dst.state.get()) >= TCPS_FIN_WAIT_2
        || i32::from(src.state.get()) >= TCPS_FIN_WAIT_2)
        && seq_geq(src.seqhi.get().wrapping_add(MAXACKWINDOW as u32), data_end)
        // Within a window forward of the originating packet
        && seq_geq(seq, src.seqlo.get().wrapping_sub(MAXACKWINDOW as u32))
    {
        // Within a window backward of the originating packet.
        //
        // This currently handles three situations: 1) stupid stacks will shotgun SYNs before
        // their peer replies; 2) when PF catches an already established stream (the firewall
        // rebooted, the state table was flushed, routes changed...); 3) packets get funky
        // immediately after the connection closes (this should catch Solaris spurious
        // ACK|FINs that web servers like to spew after a close).
        //
        // This must be a little more careful than the above code since packet floods will
        // also be caught here. We don't update the TTL here to mitigate the damage of a
        // packet flood and so the same code can handle awkward establishment and a loosened
        // connection close. In the establishment case, a correct peer response will validate
        // the connection, go through the normal state code and keep updating the state TTL.

        if pf_debug(LOG_NOTICE) {
            log(LOG_NOTICE, format_args!("pf: loose state match: "));
            pf_print_state(st);
            pf_print_flags(th.th_flags);
            addlog(format_args!(
                " seq={} ({}) ack={} len={} ackskew={} pkts={}:{} dir={},{}\n",
                seq,
                orig_seq,
                ack,
                pd.p_len,
                ackskew,
                st.packets[0].get(),
                st.packets[1].get(),
                if pd.dir == PF_IN { "in" } else { "out" },
                if pd.dir == st.direction.get() {
                    "fwd"
                } else {
                    "rev"
                }
            ));
        }

        if (dst.scrub.get().is_some() || src.scrub.get().is_some())
            && crate::net::pf_norm::pf_normalize_tcp_stateful(pd, reason, st, src, dst, copyback)
                != 0
        {
            return PF_DROP;
        }

        // Update max window.
        if u32::from(src.max_win.get()) < win {
            src.max_win.set(win as u16);
        }
        // Synchronize sequencing.
        if seq_gt(end, src.seqlo.get()) {
            src.seqlo.set(end);
        }
        // Slide the window of what the other end can send.
        if seq_geq(ack.wrapping_add(win << sws), dst.seqhi.get()) {
            dst.seqhi
                .set(ack.wrapping_add(core::cmp::max(win << sws, 1)));
        }

        // Cannot set dst->seqhi here since this could be a shotgunned SYN and not an already
        // established connection.
        if th.th_flags & TH_FIN != 0 && i32::from(src.state.get()) < TCPS_CLOSING {
            pf_set_protostate(st, psrc, TCPS_CLOSING as u8);
        }
        if th.th_flags & TH_RST != 0 {
            pf_set_protostate(st, PF_PEER_BOTH, TCPS_TIME_WAIT as u8);
        }

        // Fall through to PASS packet.
    } else {
        if i32::from(st.dst.state.get()) == TCPS_SYN_SENT
            && i32::from(st.src.state.get()) == TCPS_SYN_SENT
        {
            // Send RST for state mismatches during handshake.
            if th.th_flags & TH_RST == 0 {
                let s = pd.ld_addr(pd.src);
                let d = pd.ld_addr(pd.dst);
                pf_send_tcp(
                    st.rule.ptr(),
                    pd.af,
                    &d,
                    &s,
                    th.th_dport,
                    th.th_sport,
                    u32::from_be(th.th_ack),
                    0,
                    TH_RST,
                    0,
                    0,
                    st.rule.ptr().map_or(0, |r| r.return_ttl),
                    true,
                    0,
                    u32::from(pd.rdomain),
                    Some(reason),
                );
            }
            src.seqlo.set(0);
            src.seqhi.set(1);
            src.max_win.set(1);
        } else if pf_debug(LOG_NOTICE) {
            log(LOG_NOTICE, format_args!("pf: BAD state: "));
            pf_print_state(st);
            pf_print_flags(th.th_flags);
            addlog(format_args!(
                " seq={} ({}) ack={} len={} ackskew={} pkts={}:{} dir={},{}\n",
                seq,
                orig_seq,
                ack,
                pd.p_len,
                ackskew,
                st.packets[0].get(),
                st.packets[1].get(),
                if pd.dir == PF_IN { "in" } else { "out" },
                if pd.dir == st.direction.get() {
                    "fwd"
                } else {
                    "rev"
                }
            ));
            let c = |b: bool, ch: char| if b { ' ' } else { ch };
            addlog(format_args!(
                "pf: State failure on: {} {} {} {} | {} {}\n",
                c(seq_geq(src.seqhi.get(), data_end), '1'),
                c(seq_geq(seq, src.seqlo.get().wrapping_sub(dmw)), '2'),
                c(ackskew >= -MAXACKWINDOW, '3'),
                c(ackskew <= (MAXACKWINDOW << sws), '4'),
                c(
                    seq_geq(src.seqhi.get().wrapping_add(MAXACKWINDOW as u32), data_end),
                    '5'
                ),
                c(
                    seq_geq(seq, src.seqlo.get().wrapping_sub(MAXACKWINDOW as u32)),
                    '6'
                )
            ));
        }
        reason_set(reason, PFRES_BADSTATE);
        return PF_DROP;
    }

    PF_PASS
}

/// `pf_tcp_track_sloppy`: follows the TCP flags only, for `sloppy` states.
pub fn pf_tcp_track_sloppy(pd: &mut PfPdesc, st: &'static PfState, reason: &mut u16) -> u8 {
    let th = *pd.tcp();

    let (src, dst, psrc, pdst) = pf_state_peers(st, pd.dir == st.direction.get());

    if th.th_flags & TH_SYN != 0 && i32::from(src.state.get()) < TCPS_SYN_SENT {
        pf_set_protostate(st, psrc, TCPS_SYN_SENT as u8);
    }
    if th.th_flags & TH_FIN != 0 && i32::from(src.state.get()) < TCPS_CLOSING {
        pf_set_protostate(st, psrc, TCPS_CLOSING as u8);
    }
    if th.th_flags & TH_ACK != 0 {
        let (ss, ds) = (i32::from(src.state.get()), i32::from(dst.state.get()));
        if ds == TCPS_SYN_SENT {
            pf_set_protostate(st, pdst, TCPS_ESTABLISHED as u8);
            if i32::from(src.state.get()) == TCPS_ESTABLISHED
                && !st.src_nodes.is_empty()
                && pf_src_connlimit(st)
            {
                reason_set(reason, PFRES_SRCLIMIT);
                return PF_DROP;
            }
        } else if ds == TCPS_CLOSING {
            pf_set_protostate(st, pdst, TCPS_FIN_WAIT_2 as u8);
        } else if ss == TCPS_SYN_SENT && ds < TCPS_SYN_SENT {
            // Handle a special sloppy case where we only see one half of the connection. If
            // there is a ACK after the initial SYN without ever seeing a packet from the
            // destination, set the connection to established.
            pf_set_protostate(st, PF_PEER_BOTH, TCPS_ESTABLISHED as u8);
            if !st.src_nodes.is_empty() && pf_src_connlimit(st) {
                reason_set(reason, PFRES_SRCLIMIT);
                return PF_DROP;
            }
        } else if ss == TCPS_CLOSING && ds == TCPS_ESTABLISHED && dst.seqlo.get() == 0 {
            // Handle the closing of half connections where we don't see the full
            // bidirectional FIN/ACK+ACK handshake.
            pf_set_protostate(st, pdst, TCPS_CLOSING as u8);
        }
    }
    if th.th_flags & TH_RST != 0 {
        pf_set_protostate(st, PF_PEER_BOTH, TCPS_TIME_WAIT as u8);
    }

    // Update expire time.
    st.expire.set(getuptime() as i32);
    pf_tcp_update_timeout(st, src, dst);

    PF_PASS
}

/// `pf_synproxy`: completes the handshakes of a synproxy state on both sides.
fn pf_synproxy(pd: &mut PfPdesc, st: &'static PfState, reason: &mut u16) -> u8 {
    let Some(sk) = st.key[usize::from(pd.didx)].get() else {
        return PF_DROP;
    };
    let (si, di) = (usize::from(pd.sidx), usize::from(pd.didx));

    if st.src.state.get() == PF_TCPS_PROXY_SRC {
        let th = *pd.tcp();

        if pd.dir != st.direction.get() {
            reason_set(reason, PFRES_SYNPROXY);
            return PF_SYNPROXY_DROP;
        }
        if th.th_flags & TH_SYN != 0 {
            if u32::from_be(th.th_seq) != st.src.seqlo.get() {
                reason_set(reason, PFRES_SYNPROXY);
                return PF_DROP;
            }
            let s = pd.ld_addr(pd.src);
            let d = pd.ld_addr(pd.dst);
            pf_send_tcp(
                st.rule.ptr(),
                pd.af,
                &d,
                &s,
                th.th_dport,
                th.th_sport,
                st.src.seqhi.get(),
                u32::from_be(th.th_seq).wrapping_add(1),
                TH_SYN | TH_ACK,
                0,
                st.src.mss.get(),
                0,
                true,
                0,
                u32::from(pd.rdomain),
                Some(reason),
            );
            reason_set(reason, PFRES_SYNPROXY);
            return PF_SYNPROXY_DROP;
        } else if (th.th_flags & (TH_ACK | TH_RST | TH_FIN)) != TH_ACK
            || u32::from_be(th.th_ack) != st.src.seqhi.get().wrapping_add(1)
            || u32::from_be(th.th_seq) != st.src.seqlo.get().wrapping_add(1)
        {
            reason_set(reason, PFRES_SYNPROXY);
            return PF_DROP;
        } else if !st.src_nodes.is_empty() && pf_src_connlimit(st) {
            reason_set(reason, PFRES_SRCLIMIT);
            return PF_DROP;
        } else {
            pf_set_protostate(st, PF_PEER_SRC, PF_TCPS_PROXY_DST);
        }
    }
    if st.src.state.get() == PF_TCPS_PROXY_DST {
        let th = *pd.tcp();

        if pd.dir == st.direction.get() {
            if (th.th_flags & (TH_SYN | TH_ACK)) != TH_ACK
                || u32::from_be(th.th_ack) != st.src.seqhi.get().wrapping_add(1)
                || u32::from_be(th.th_seq) != st.src.seqlo.get().wrapping_add(1)
            {
                reason_set(reason, PFRES_SYNPROXY);
                return PF_DROP;
            }
            st.src
                .max_win
                .set(core::cmp::max(u16::from_be(th.th_win), 1));
            if st.dst.seqhi.get() == 1 {
                st.dst.seqhi.set(arc4random());
            }
            pf_send_tcp(
                st.rule.ptr(),
                pd.af,
                &sk.addr[si].get(),
                &sk.addr[di].get(),
                sk.port[si].get(),
                sk.port[di].get(),
                st.dst.seqhi.get(),
                0,
                TH_SYN,
                0,
                st.src.mss.get(),
                0,
                false,
                st.tag.get(),
                u32::from(sk.rdomain.get()),
                Some(reason),
            );
            reason_set(reason, PFRES_SYNPROXY);
            return PF_SYNPROXY_DROP;
        } else if (th.th_flags & (TH_SYN | TH_ACK)) != (TH_SYN | TH_ACK)
            || u32::from_be(th.th_ack) != st.dst.seqhi.get().wrapping_add(1)
        {
            reason_set(reason, PFRES_SYNPROXY);
            return PF_DROP;
        } else {
            st.dst
                .max_win
                .set(core::cmp::max(u16::from_be(th.th_win), 1));
            st.dst.seqlo.set(u32::from_be(th.th_seq));
            let s = pd.ld_addr(pd.src);
            let d = pd.ld_addr(pd.dst);
            pf_send_tcp(
                st.rule.ptr(),
                pd.af,
                &d,
                &s,
                th.th_dport,
                th.th_sport,
                u32::from_be(th.th_ack),
                u32::from_be(th.th_seq).wrapping_add(1),
                TH_ACK,
                st.src.max_win.get(),
                0,
                0,
                false,
                st.tag.get(),
                u32::from(pd.rdomain),
                Some(reason),
            );
            pf_send_tcp(
                st.rule.ptr(),
                pd.af,
                &sk.addr[si].get(),
                &sk.addr[di].get(),
                sk.port[si].get(),
                sk.port[di].get(),
                st.src.seqhi.get().wrapping_add(1),
                st.src.seqlo.get().wrapping_add(1),
                TH_ACK,
                st.dst.max_win.get(),
                0,
                0,
                true,
                0,
                u32::from(sk.rdomain.get()),
                Some(reason),
            );
            st.src
                .seqdiff
                .set(st.dst.seqhi.get().wrapping_sub(st.src.seqlo.get()));
            st.dst
                .seqdiff
                .set(st.src.seqhi.get().wrapping_sub(st.dst.seqlo.get()));
            st.src.seqhi.set(
                st.src
                    .seqlo
                    .get()
                    .wrapping_add(u32::from(st.dst.max_win.get())),
            );
            st.dst.seqhi.set(
                st.dst
                    .seqlo
                    .get()
                    .wrapping_add(u32::from(st.src.max_win.get())),
            );
            st.src.wscale.set(0);
            st.dst.wscale.set(0);
            pf_set_protostate(st, PF_PEER_BOTH, TCPS_ESTABLISHED as u8);
            reason_set(reason, PFRES_SYNPROXY);
            return PF_SYNPROXY_DROP;
        }
    }
    PF_PASS
}

/// `pf_synproxy_ack`: answers the client's SYN of a new synproxy state.
fn pf_synproxy_ack(
    r: &'static PfRule,
    pd: &mut PfPdesc,
    sm: &mut Option<&'static PfState>,
    act: &PfRuleActions,
) -> u8 {
    let th = *pd.tcp();
    let mut reason: u16 = 0;

    if (th.th_flags & (TH_SYN | TH_ACK)) != TH_SYN {
        return PF_PASS;
    }

    let Some(s) = *sm else {
        return PF_PASS;
    };
    let rtid = if act.rtableid >= 0 {
        act.rtableid
    } else {
        i32::from(pd.rdomain)
    };

    pf_set_protostate(s, PF_PEER_SRC, PF_TCPS_PROXY_SRC);
    s.src.seqhi.set(arc4random());
    // Find mss option.
    let mssdflt = tcp_mssdflt();
    let mut mss = pf_get_mss(pd, mssdflt);
    let src = pd.ld_addr(pd.src);
    let dst = pd.ld_addr(pd.dst);
    mss = pf_calc_mss(&src, pd.af, rtid, mss, mssdflt);
    mss = pf_calc_mss(&dst, pd.af, rtid, mss, mssdflt);
    s.src.mss.set(mss);

    pf_send_tcp(
        Some(r),
        pd.af,
        &dst,
        &src,
        th.th_dport,
        th.th_sport,
        s.src.seqhi.get(),
        u32::from_be(th.th_seq).wrapping_add(1),
        TH_SYN | TH_ACK,
        0,
        s.src.mss.get(),
        0,
        true,
        0,
        u32::from(pd.rdomain),
        None,
    );

    reason_set(&mut reason, PFRES_SYNPROXY);
    PF_SYNPROXY_DROP
}

/// The state key the packet is translated to (`nk` in the C): the stack key for packets
/// whose state is reversed (af-to), else the key on the packet's destination side.
fn pf_state_nk(st: &'static PfState, pd: &PfPdesc) -> Option<&'static PfStateKey> {
    if pf_reversed_key(&st.key, pd.af) {
        st.key[usize::from(pd.sidx)].get()
    } else {
        st.key[usize::from(pd.didx)].get()
    }
}

/// `pf_test_state`: tracks a TCP, UDP or other packet against its state and translates it.
pub fn pf_test_state(pd: &mut PfPdesc, stp: &mut Option<&'static PfState>, reason: &mut u16) -> u8 {
    let mut copyback = 0;
    let mut action = PF_PASS;
    let Some(st) = *stp else {
        return PF_DROP;
    };

    let (src, dst, psrc, pdst) = pf_state_peers(st, pd.dir == st.direction.get());

    match i32::from(pd.virtual_proto) {
        IPPROTO_TCP => {
            action = pf_synproxy(pd, st, reason);
            if action != PF_PASS {
                return action;
            }
            let th_flags = pd.tcp().th_flags;
            if (th_flags & (TH_SYN | TH_ACK)) == TH_SYN {
                if i32::from(dst.state.get()) >= TCPS_FIN_WAIT_2
                    && i32::from(src.state.get()) >= TCPS_FIN_WAIT_2
                {
                    if pf_debug(LOG_NOTICE) {
                        log(LOG_NOTICE, format_args!("pf: state reuse "));
                        pf_print_state(st);
                        pf_print_flags(th_flags);
                        addlog(format_args!("\n"));
                    }
                    // XXX make sure it's the same direction ??
                    pf_update_state_timeout(st, PFTM_PURGE);
                    pf_state_unref(Some(st));
                    *stp = None;
                    return PF_DROP;
                } else if i32::from(dst.state.get()) >= TCPS_ESTABLISHED
                    && i32::from(src.state.get()) >= TCPS_ESTABLISHED
                {
                    // SYN matches existing state??? Typically happens when sender boots up
                    // after sudden panic. Certain protocols (NFSv3) are always using same
                    // port numbers. Challenge ACK enables all parties (firewall and peers) to
                    // get in sync again.
                    pf_send_challenge_ack(pd, st, src, dst, reason);
                    return PF_DROP;
                }
            }

            if st.state_flags.get() & PFSTATE_SLOPPY != 0 {
                if pf_tcp_track_sloppy(pd, st, reason) == PF_DROP {
                    return PF_DROP;
                }
            } else {
                let rev = pf_reversed_key(&st.key, pd.af);
                if pf_tcp_track_full(pd, st, reason, &mut copyback, rev) == PF_DROP {
                    return PF_DROP;
                }
            }
        }
        IPPROTO_UDP => {
            // Update states.
            if src.state.get() < PFUDPS_SINGLE {
                pf_set_protostate(st, psrc, PFUDPS_SINGLE);
            }
            if dst.state.get() == PFUDPS_SINGLE {
                pf_set_protostate(st, pdst, PFUDPS_MULTIPLE);
            }

            // Update expire time.
            st.expire.set(getuptime() as i32);
            if src.state.get() == PFUDPS_MULTIPLE && dst.state.get() == PFUDPS_MULTIPLE {
                pf_update_state_timeout(st, PFTM_UDP_MULTIPLE);
            } else {
                pf_update_state_timeout(st, PFTM_UDP_SINGLE);
            }
        }
        _ => {
            // Update states.
            if src.state.get() < PFOTHERS_SINGLE {
                pf_set_protostate(st, psrc, PFOTHERS_SINGLE);
            }
            if dst.state.get() == PFOTHERS_SINGLE {
                pf_set_protostate(st, pdst, PFOTHERS_MULTIPLE);
            }

            // Update expire time.
            st.expire.set(getuptime() as i32);
            if src.state.get() == PFOTHERS_MULTIPLE && dst.state.get() == PFOTHERS_MULTIPLE {
                pf_update_state_timeout(st, PFTM_OTHER_MULTIPLE);
            } else {
                pf_update_state_timeout(st, PFTM_OTHER_SINGLE);
            }
        }
    }

    // Translate source/destination address, if necessary.
    if !opt_eq(st.key[PF_SK_WIRE].get(), st.key[PF_SK_STACK].get())
        && let Some(nk) = pf_state_nk(st, pd)
    {
        let afto = pd.af != nk.af.get();
        let (sidx, didx) = if afto {
            (usize::from(pd.didx), usize::from(pd.sidx))
        } else {
            (usize::from(pd.sidx), usize::from(pd.didx))
        };

        #[cfg(feature = "inet6")]
        if afto {
            let naf = nk.af.get();
            pf_addrcpy(&mut pd.nsaddr, &nk.addr[sidx].get(), naf);
            pf_addrcpy(&mut pd.ndaddr, &nk.addr[didx].get(), naf);
            pd.naf = naf;
            action = PF_AFRT;
        }

        if !afto {
            let s = pd.src;
            pf_translate_a(pd, s, &nk.addr[sidx].get());
        }

        if !pd.sport.is_none() {
            let sp = pd.sport;
            pf_patch_16(pd, sp, nk.port[sidx].get());
        }

        let d = pd.ld_addr(pd.dst);
        if afto || pf_aneq(&d, &nk.addr[didx].get(), pd.af) || pd.rdomain != nk.rdomain.get() {
            pd.destchg = 1;
        }

        if !afto {
            let dl = pd.dst;
            pf_translate_a(pd, dl, &nk.addr[didx].get());
        }

        if !pd.dport.is_none() {
            let dp = pd.dport;
            pf_patch_16(pd, dp, nk.port[didx].get());
        }

        if let Some(m) = pd.m {
            m.m_pkthdr().ph_rtableid.set(u32::from(nk.rdomain.get()));
        }
        copyback = 1;
    }

    if copyback != 0 && pd.hdrlen > 0 {
        pf_copyback_hdr(pd);
    }

    action
}

/// `m_copyback(pd->m, pd->off, pd->hdrlen, &pd->hdr, M_NOWAIT)`.
fn pf_copyback_hdr(pd: &mut PfPdesc) {
    let Some(m) = pd.m else {
        return;
    };
    let off = pd.off as i32;
    let n = core::cmp::min(pd.hdrlen as usize, pd.hdr_bytes().len());
    let mut buf = [0u8; size_of::<PfPdescHdr>()];
    buf[..n].copy_from_slice(&pd.hdr_bytes()[..n]);
    let _ = crate::kern::uipc_mbuf::m_copyback(m, off, &buf[..n], M_NOWAIT);
}

/// `pf_icmp_state_lookup`: looks up the state of an ICMP query or reply. Returns the
/// action to take (`PF_DROP`, ...), or -1 to go on with the state found.
#[allow(clippy::too_many_arguments)]
pub fn pf_icmp_state_lookup(
    pd: &mut PfPdesc,
    stp: &mut Option<&'static PfState>,
    icmpid: u16,
    type_: u16,
    icmp_dir: i32,
    iidx: &mut usize,
    multi: bool,
    inner: bool,
) -> i32 {
    let mut key = PfStateKeyCmp {
        af: pd.af,
        proto: pd.proto,
        rdomain: pd.rdomain,
        ..PfStateKeyCmp::default()
    };
    let (si, di) = (usize::from(pd.sidx), usize::from(pd.didx));
    if icmp_dir == i32::from(PF_IN) {
        *iidx = si;
        key.port[si] = icmpid;
        key.port[di] = type_;
    } else {
        *iidx = di;
        key.port[si] = type_;
        key.port[di] = icmpid;
    }

    let s = pd.ld_addr(pd.src);
    let d = pd.ld_addr(pd.dst);
    if pf_state_key_addr_setup(pd, &mut key.addr, si, Some(&s), di, Some(&d), pd.af, multi) != 0 {
        return i32::from(PF_DROP);
    }

    key.hash = pf_pkt_hash(key.af, key.proto, &key.addr[0], &key.addr[1], 0, 0);

    let action = pf_find_state(pd, &key, stp);
    if action != PF_MATCH {
        return i32::from(action);
    }
    let Some(st) = *stp else {
        return i32::from(PF_DROP);
    };

    if st.state_flags.get() & PFSTATE_SLOPPY != 0 {
        return -1;
    }

    // Is this ICMP message flowing in right direction?
    let (Some(w), Some(s)) = (st.key[PF_SK_WIRE].get(), st.key[PF_SK_STACK].get()) else {
        return i32::from(PF_DROP);
    };
    let direction = if w.af.get() != s.af.get() {
        if pd.af == w.af.get() { PF_IN } else { PF_OUT }
    } else {
        st.direction.get()
    };
    let flow = if (!inner && direction == pd.dir) || (inner && direction != pd.dir) {
        PF_IN
    } else {
        PF_OUT
    };
    if i32::from(flow) != icmp_dir {
        if pf_debug(LOG_NOTICE) {
            log(
                LOG_NOTICE,
                format_args!(
                    "pf: icmp type {} in wrong direction ({}): ",
                    u16::from_be(type_),
                    icmp_dir
                ),
            );
            pf_print_state(st);
            addlog(format_args!("\n"));
        }
        return i32::from(PF_DROP);
    }
    -1
}

/// The offset of `th_seq` in `struct tcphdr`.
pub const TH_SEQ_OFF: usize = 4;
/// The offset of `th_ack` in `struct tcphdr`.
pub const TH_ACK_OFF: usize = 8;
/// The offset of `th_sport` in `struct tcphdr` (and `uh_sport` in `struct udphdr`).
const TH_SPORT_OFF: usize = 0;
/// The offset of `th_dport` in `struct tcphdr` (and `uh_dport` in `struct udphdr`).
const TH_DPORT_OFF: usize = 2;
/// The offset of `uh_sum` in `struct udphdr`.
const UH_SUM_OFF: usize = 6;
/// The offset of `icmp_id` in `struct icmp`.
pub const ICMP_ID_OFF: usize = 4;

/// A location in the header copy of another descriptor (the C's `&pd2.hdr.tcp.th_sport`).
fn pf_hdr_loc(pd2: &mut PfPdesc, off: usize) -> PfLoc {
    let p = pd2.hdr_bytes()[off..].as_mut_ptr();
    // SAFETY: `pd2` is a local of `pf_test_state_icmp` that outlives every use of the
    // location; each location is made right before the patch that writes through it, and
    // `pd2.hdr` is not written through `pd2` meanwhile.
    unsafe { PfLoc::local(p) }
}

/// `pf_test_state_icmp`: an ICMP packet against its query state, or an ICMP error against
/// the state of the connection it quotes, translating both as the state says.
pub fn pf_test_state_icmp(
    pd: &mut PfPdesc,
    stp: &mut Option<&'static PfState>,
    reason: &mut u16,
) -> u8 {
    use crate::netinet::ip::{IP_OFFMASK, Ip};
    use crate::netinet::ip_icmp::{ICMP_ECHO, ICMP_MINLEN};

    let mut virtual_id: u16 = 0;
    let mut virtual_type: u16 = 0;
    let mut icmp_dir: i32 = 0;
    let mut iidx: usize = 0;
    let mut copyback = 0;

    let (icmptype, icmpcode) = match i32::from(pd.proto) {
        IPPROTO_ICMP => (pd.icmp().icmp_type, pd.icmp().icmp_code),
        #[cfg(feature = "inet6")]
        IPPROTO_ICMPV6 => (pd.icmp6().icmp6_type, pd.icmp6().icmp6_code),
        p => panic(format_args!("unhandled proto {p}")),
    };

    if !pf_icmp_mapping(
        pd,
        icmptype,
        &mut icmp_dir,
        &mut virtual_id,
        &mut virtual_type,
    ) {
        // ICMP query/reply message not related to a TCP/UDP packet. Search for an ICMP
        // state.
        let mut ret = pf_icmp_state_lookup(
            pd,
            stp,
            virtual_id,
            virtual_type,
            icmp_dir,
            &mut iidx,
            false,
            false,
        );
        // IPv6? try matching a multicast address.
        if ret == i32::from(PF_DROP) && pd.af == AF_INET6 && icmp_dir == i32::from(PF_OUT) {
            ret = pf_icmp_state_lookup(
                pd,
                stp,
                virtual_id,
                virtual_type,
                icmp_dir,
                &mut iidx,
                true,
                false,
            );
        }
        if ret >= 0 {
            return ret as u8;
        }
        let Some(st) = *stp else {
            return PF_DROP;
        };

        st.expire.set(getuptime() as i32);
        pf_update_state_timeout(st, PFTM_ICMP_ERROR_REPLY);

        // Translate source/destination address, if necessary.
        if !opt_eq(st.key[PF_SK_WIRE].get(), st.key[PF_SK_STACK].get())
            && let Some(nk) = pf_state_nk(st, pd)
        {
            let afto = pd.af != nk.af.get();
            let (sidx, didx) = if afto {
                (usize::from(pd.didx), usize::from(pd.sidx))
            } else {
                (usize::from(pd.sidx), usize::from(pd.didx))
            };
            if afto {
                iidx = usize::from(iidx == 0);
            }
            #[cfg(feature = "inet6")]
            if afto {
                let naf = nk.af.get();
                pf_addrcpy(&mut pd.nsaddr, &nk.addr[sidx].get(), naf);
                pf_addrcpy(&mut pd.ndaddr, &nk.addr[didx].get(), naf);
                pd.naf = naf;
            }
            if !afto {
                let (s, d) = (pd.src, pd.dst);
                pf_translate_a(pd, s, &nk.addr[sidx].get());
                pf_translate_a(pd, d, &nk.addr[didx].get());
            }

            if pd.rdomain != nk.rdomain.get() {
                pd.destchg = 1;
            }
            let d = pd.ld_addr(pd.dst);
            if !afto && pf_aneq(&d, &nk.addr[didx].get(), pd.af) {
                pd.destchg = 1;
            }
            let Some(m) = pd.m else {
                return PF_DROP;
            };
            m.m_pkthdr().ph_rtableid.set(u32::from(nk.rdomain.get()));

            if pd.af == AF_INET {
                #[cfg(feature = "inet6")]
                if afto {
                    if pf_translate_icmp_af(pd, AF_INET6, PfLoc::Hdr(0)) != 0 {
                        return PF_DROP;
                    }
                    pd.proto = IPPROTO_ICMPV6 as u8;
                }
                pf_patch_16(pd, PfLoc::Hdr(ICMP_ID_OFF), nk.port[iidx].get());

                let off = pd.off as i32;
                let mut bytes = [0u8; ICMP_MINLEN];
                bytes.copy_from_slice(&pd.hdr_bytes()[..ICMP_MINLEN]);
                let _ = crate::kern::uipc_mbuf::m_copyback(m, off, &bytes, M_NOWAIT);
                copyback = 1;
            }
            #[cfg(feature = "inet6")]
            if pd.af == AF_INET6 {
                use crate::netinet::icmp6::Icmp6Hdr;
                if afto {
                    if pf_translate_icmp_af(pd, AF_INET, PfLoc::Hdr(0)) != 0 {
                        return PF_DROP;
                    }
                    pd.proto = IPPROTO_ICMP as u8;
                }

                // icmp6_id is at the offset of icmp_id.
                pf_patch_16(pd, PfLoc::Hdr(ICMP_ID_OFF), nk.port[iidx].get());

                let off = pd.off as i32;
                let mut bytes = [0u8; size_of::<Icmp6Hdr>()];
                bytes.copy_from_slice(&pd.hdr_bytes()[..size_of::<Icmp6Hdr>()]);
                let _ = crate::kern::uipc_mbuf::m_copyback(m, off, &bytes, M_NOWAIT);
                copyback = 1;
            }
            #[cfg(feature = "inet6")]
            if afto {
                return PF_AFRT;
            }
        }
    } else {
        // ICMP error message in response to a TCP/UDP packet. Extract the inner TCP/UDP
        // header and search for that state.
        // With INET6 an IPv6 error leaves `h2` unwritten.
        #[cfg(feature = "inet6")]
        let mut h2 = Ip::default();
        #[cfg(not(feature = "inet6"))]
        let mut h2: Ip;
        #[cfg(feature = "inet6")]
        let mut h2_6 = Ip6Hdr::zeroed();

        // Initialize pd2 fields valid for both packets with pd.
        let mut pd2 = PfPdesc::new();
        pd2.af = pd.af;
        pd2.dir = pd.dir;
        pd2.kif = pd.kif;
        pd2.m = pd.m;
        pd2.rdomain = pd.rdomain;
        // Payload packet is from the opposite direction.
        pd2.sidx = if pd2.dir == PF_IN { 1 } else { 0 };
        pd2.didx = if pd2.dir == PF_IN { 0 } else { 1 };
        let Some(m2) = pd2.m else {
            return PF_DROP;
        };
        let ipoff2 = match pd.af {
            AF_INET => {
                // Offset of h2 in mbuf chain.
                let ipoff2 = pd.off as usize + ICMP_MINLEN;

                let mut b = [0u8; size_of::<Ip>()];
                if !pf_pull_hdr(m2, ipoff2 as i32, &mut b, Some(reason), pd2.af) {
                    crate::dpfprintf!(LOG_NOTICE, "ICMP error message too short (ip)");
                    return PF_DROP;
                }
                // SAFETY: `Ip` is 20 bytes of integers (`#[repr(C)]`, no padding).
                h2 = unsafe { ptr::read_unaligned(b.as_ptr().cast::<Ip>()) };
                // ICMP error messages don't refer to non-first fragments.
                if h2.ip_off & IP_OFFMASK.to_be() != 0 {
                    reason_set(reason, PFRES_FRAG);
                    return PF_DROP;
                }

                // Offset of protocol header that follows h2.
                pd2.off = ipoff2 as u32;
                if pf_walk_header(&mut pd2, &h2, reason) != PF_PASS {
                    return PF_DROP;
                }

                pd2.tot_len = u64::from(u16::from_be(h2.ip_len));
                pd2.ttl = h2.ip_ttl;
                // SAFETY: `h2` is this function's local; it outlives `pd2` and is only read
                // directly (copybacks) while the locations may write it.
                unsafe {
                    pd2.src = PfLoc::local(ptr::addr_of_mut!(h2.ip_src));
                    pd2.dst = PfLoc::local(ptr::addr_of_mut!(h2.ip_dst));
                }
                ipoff2
            }
            #[cfg(feature = "inet6")]
            AF_INET6 => {
                let ipoff2 = pd.off as usize + size_of::<crate::netinet::icmp6::Icmp6Hdr>();

                let mut b = [0u8; size_of::<Ip6Hdr>()];
                if !pf_pull_hdr(m2, ipoff2 as i32, &mut b, Some(reason), pd2.af) {
                    crate::dpfprintf!(LOG_NOTICE, "ICMP error message too short (ip6)");
                    return PF_DROP;
                }
                // SAFETY: `Ip6Hdr` is 40 bytes of integers (`#[repr(C)]`, no padding).
                h2_6 = unsafe { ptr::read_unaligned(b.as_ptr().cast::<Ip6Hdr>()) };

                pd2.off = ipoff2 as u32;
                if pf_walk_header6(&mut pd2, &h2_6, reason) != PF_PASS {
                    return PF_DROP;
                }

                pd2.tot_len = u64::from(u16::from_be(h2_6.ip6_plen)) + size_of::<Ip6Hdr>() as u64;
                pd2.ttl = h2_6.ip6_hlim;
                // SAFETY: as for `h2` above.
                unsafe {
                    pd2.src = PfLoc::local(ptr::addr_of_mut!(h2_6.ip6_src));
                    pd2.dst = PfLoc::local(ptr::addr_of_mut!(h2_6.ip6_dst));
                }
                ipoff2
            }
            _ => unhandled_af(i32::from(pd.af)),
        };

        let pdst = pd.ld_addr(pd.dst);
        let p2src = pd2.ld_addr(pd2.src);
        if pf_aneq(&pdst, &p2src, pd.af) {
            if pf_debug(LOG_NOTICE) {
                log(
                    LOG_NOTICE,
                    format_args!("pf: BAD ICMP {icmptype}:{icmpcode} outer dst: "),
                );
                let ps = pd.ld_addr(pd.src);
                pf_print_host(&ps, 0, pd.af);
                addlog(format_args!(" -> "));
                pf_print_host(&pdst, 0, pd.af);
                addlog(format_args!(" inner src: "));
                pf_print_host(&p2src, 0, pd2.af);
                addlog(format_args!(" -> "));
                let p2dst = pd2.ld_addr(pd2.dst);
                pf_print_host(&p2dst, 0, pd2.af);
                addlog(format_args!("\n"));
            }
            reason_set(reason, PFRES_BADSTATE);
            return PF_DROP;
        }

        let (s2, d2) = (usize::from(pd2.sidx), usize::from(pd2.didx));
        let h2copy = |m: &Mbuf, h2: &Ip| {
            let mut b = [0u8; size_of::<Ip>()];
            // SAFETY: `Ip` is 20 bytes of integers; `b` has room.
            unsafe { ptr::write_unaligned(b.as_mut_ptr().cast::<Ip>(), *h2) };
            let _ = crate::kern::uipc_mbuf::m_copyback(m, ipoff2 as i32, &b, M_NOWAIT);
        };
        #[cfg(feature = "inet6")]
        let h2_6copy = |m: &Mbuf, h2_6: &Ip6Hdr| {
            let mut b = [0u8; size_of::<Ip6Hdr>()];
            // SAFETY: `Ip6Hdr` is 40 bytes of integers; `b` has room.
            unsafe { ptr::write_unaligned(b.as_mut_ptr().cast::<Ip6Hdr>(), *h2_6) };
            let _ = crate::kern::uipc_mbuf::m_copyback(m, ipoff2 as i32, &b, M_NOWAIT);
        };
        // The outer `icmp` or `icmp6_hdr` (ICMP_MINLEN and sizeof(struct icmp6_hdr) are
        // both 8).
        let icmpcopy = |pd: &mut PfPdesc| {
            if let Some(m) = pd.m {
                let off = pd.off as i32;
                let mut b = [0u8; ICMP_MINLEN];
                b.copy_from_slice(&pd.hdr_bytes()[..ICMP_MINLEN]);
                let _ = crate::kern::uipc_mbuf::m_copyback(m, off, &b, M_NOWAIT);
            }
        };

        match i32::from(pd2.proto) {
            IPPROTO_TCP => {
                // Only the first 8 bytes of the TCP header can be expected. Don't access any
                // TCP header fields after th_seq, an ackskew test is not possible.
                let mut b = [0u8; 8];
                if !pf_pull_hdr(m2, pd2.off as i32, &mut b, Some(reason), pd2.af) {
                    crate::dpfprintf!(LOG_NOTICE, "ICMP error message too short (tcp)");
                    return PF_DROP;
                }
                pd2.hdr_bytes()[..8].copy_from_slice(&b);
                let th = *pd2.tcp();

                let mut key = PfStateKeyCmp {
                    af: pd2.af,
                    proto: IPPROTO_TCP as u8,
                    rdomain: pd2.rdomain,
                    ..PfStateKeyCmp::default()
                };
                let p2d = pd2.ld_addr(pd2.dst);
                pf_addrcpy(&mut key.addr[s2], &p2src, key.af);
                pf_addrcpy(&mut key.addr[d2], &p2d, key.af);
                key.port[s2] = th.th_sport;
                key.port[d2] = th.th_dport;
                key.hash = pf_pkt_hash(pd2.af, pd2.proto, &p2src, &p2d, th.th_sport, th.th_dport);

                let action = pf_find_state(&mut pd2, &key, stp);
                if action != PF_MATCH {
                    return action;
                }
                let Some(st) = *stp else {
                    return PF_DROP;
                };

                let rev = pf_reversed_key(&st.key, pd.af);
                let (src, dst) = if pd2.dir == st.direction.get() {
                    if rev {
                        (&st.src, &st.dst)
                    } else {
                        (&st.dst, &st.src)
                    }
                } else if rev {
                    (&st.dst, &st.src)
                } else {
                    (&st.src, &st.dst)
                };

                let dws = if src.wscale.get() != 0 && dst.wscale.get() != 0 {
                    u32::from(dst.wscale.get() & PF_WSCALE_MASK)
                } else {
                    0
                };

                // Demodulate sequence number.
                let seq = u32::from_be(th.th_seq).wrapping_sub(src.seqdiff.get());
                if src.seqdiff.get() != 0 {
                    let l = pf_hdr_loc(&mut pd2, TH_SEQ_OFF);
                    pf_patch_32(pd, l, seq.to_be());
                    copyback = 1;
                }

                if st.state_flags.get() & PFSTATE_SLOPPY == 0
                    && (!seq_geq(src.seqhi.get(), seq)
                        || !seq_geq(
                            seq,
                            src.seqlo
                                .get()
                                .wrapping_sub(u32::from(dst.max_win.get()) << dws),
                        ))
                {
                    if pf_debug(LOG_NOTICE) {
                        log(
                            LOG_NOTICE,
                            format_args!("pf: BAD ICMP {icmptype}:{icmpcode} "),
                        );
                        let (ps, pdd) = (pd.ld_addr(pd.src), pd.ld_addr(pd.dst));
                        pf_print_host(&ps, 0, pd.af);
                        addlog(format_args!(" -> "));
                        pf_print_host(&pdd, 0, pd.af);
                        addlog(format_args!(" state: "));
                        pf_print_state(st);
                        addlog(format_args!(" seq={seq}\n"));
                    }
                    reason_set(reason, PFRES_BADSTATE);
                    return PF_DROP;
                } else if pf_debug(LOG_DEBUG) {
                    log(
                        LOG_DEBUG,
                        format_args!("pf: OK ICMP {icmptype}:{icmpcode} "),
                    );
                    let (ps, pdd) = (pd.ld_addr(pd.src), pd.ld_addr(pd.dst));
                    pf_print_host(&ps, 0, pd.af);
                    addlog(format_args!(" -> "));
                    pf_print_host(&pdd, 0, pd.af);
                    addlog(format_args!(" state: "));
                    pf_print_state(st);
                    addlog(format_args!(" seq={seq}\n"));
                }

                // Translate source/destination address, if necessary.
                if !opt_eq(st.key[PF_SK_WIRE].get(), st.key[PF_SK_STACK].get())
                    && let Some(nk) = pf_state_nk(st, pd)
                {
                    #[cfg(feature = "inet6")]
                    if pd.af != nk.af.get() {
                        // afto: the quoted addresses swap their indexes.
                        let (sidx, didx) = (d2, s2);
                        let naf = nk.af.get();
                        if pf_translate_icmp_af(pd, naf, PfLoc::Hdr(0)) != 0 {
                            return PF_DROP;
                        }
                        icmpcopy(pd);
                        let (sa, da) = (nk.addr[sidx].get(), nk.addr[didx].get());
                        let af = pd.af;
                        if pf_change_icmp_af(m2, ipoff2 as i32, pd, &mut pd2, &sa, &da, af, naf)
                            != 0
                        {
                            return PF_DROP;
                        }
                        m2.m_pkthdr().ph_rtableid.set(u32::from(nk.rdomain.get()));
                        pd.destchg = 1;
                        pf_addrcpy(&mut pd.nsaddr, &nk.addr[s2].get(), naf);
                        pf_addrcpy(&mut pd.ndaddr, &nk.addr[d2].get(), naf);
                        if naf == AF_INET {
                            pd.proto = IPPROTO_ICMP as u8;
                        } else {
                            pd.proto = IPPROTO_ICMPV6 as u8;
                            // IPv4 becomes IPv6 so we must copy the IPv4 src addr to the
                            // least 32 bits of the IPv6 address to keep traceroute/icmp
                            // working.
                            let src = pd.ld_addr(pd.src);
                            pd.nsaddr.set_addr32(3, src.addr32(0));
                        }
                        pd.naf = naf;

                        let l = pf_hdr_loc(&mut pd2, TH_SPORT_OFF);
                        pf_patch_16(pd, l, nk.port[sidx].get());
                        let l = pf_hdr_loc(&mut pd2, TH_DPORT_OFF);
                        pf_patch_16(pd, l, nk.port[didx].get());

                        let mut b = [0u8; 8];
                        b.copy_from_slice(&pd2.hdr_bytes()[..8]);
                        let _ =
                            crate::kern::uipc_mbuf::m_copyback(m2, pd2.off as i32, &b, M_NOWAIT);
                        return PF_AFRT;
                    }
                    let th = *pd2.tcp();
                    let p2s = pd2.ld_addr(pd2.src);
                    if pf_aneq(&p2s, &nk.addr[s2].get(), pd2.af) || nk.port[s2].get() != th.th_sport
                    {
                        let (q, o) = (pd2.src, pd.dst);
                        let qp = pf_hdr_loc(&mut pd2, TH_SPORT_OFF);
                        pf_translate_icmp(pd, q, qp, o, &nk.addr[s2].get(), nk.port[s2].get());
                    }

                    let p2d = pd2.ld_addr(pd2.dst);
                    if pf_aneq(&p2d, &nk.addr[d2].get(), pd2.af) || pd2.rdomain != nk.rdomain.get()
                    {
                        pd.destchg = 1;
                    }
                    m2.m_pkthdr().ph_rtableid.set(u32::from(nk.rdomain.get()));

                    let th = *pd2.tcp();
                    if pf_aneq(&p2d, &nk.addr[d2].get(), pd2.af) || nk.port[d2].get() != th.th_dport
                    {
                        let (q, o) = (pd2.dst, pd.src);
                        let qp = pf_hdr_loc(&mut pd2, TH_DPORT_OFF);
                        pf_translate_icmp(pd, q, qp, o, &nk.addr[d2].get(), nk.port[d2].get());
                    }
                    copyback = 1;
                }

                if copyback != 0 {
                    if pd2.af == AF_INET {
                        icmpcopy(pd);
                        h2copy(m2, &h2);
                    }
                    #[cfg(feature = "inet6")]
                    if pd2.af == AF_INET6 {
                        icmpcopy(pd);
                        h2_6copy(m2, &h2_6);
                    }
                    let mut b = [0u8; 8];
                    b.copy_from_slice(&pd2.hdr_bytes()[..8]);
                    let _ = crate::kern::uipc_mbuf::m_copyback(m2, pd2.off as i32, &b, M_NOWAIT);
                }
            }
            IPPROTO_UDP => {
                let mut b = [0u8; size_of::<Udphdr>()];
                if !pf_pull_hdr(m2, pd2.off as i32, &mut b, Some(reason), pd2.af) {
                    crate::dpfprintf!(LOG_NOTICE, "ICMP error message too short (udp)");
                    return PF_DROP;
                }
                pd2.hdr_bytes()[..b.len()].copy_from_slice(&b);
                let uh = *pd2.udp();

                let mut key = PfStateKeyCmp {
                    af: pd2.af,
                    proto: IPPROTO_UDP as u8,
                    rdomain: pd2.rdomain,
                    ..PfStateKeyCmp::default()
                };
                let p2d = pd2.ld_addr(pd2.dst);
                pf_addrcpy(&mut key.addr[s2], &p2src, key.af);
                pf_addrcpy(&mut key.addr[d2], &p2d, key.af);
                key.port[s2] = uh.uh_sport;
                key.port[d2] = uh.uh_dport;
                key.hash = pf_pkt_hash(pd2.af, pd2.proto, &p2src, &p2d, uh.uh_sport, uh.uh_dport);

                let action = pf_find_state(&mut pd2, &key, stp);
                if action != PF_MATCH {
                    return action;
                }
                let Some(st) = *stp else {
                    return PF_DROP;
                };

                // Translate source/destination address, if necessary.
                if !opt_eq(st.key[PF_SK_WIRE].get(), st.key[PF_SK_STACK].get())
                    && let Some(nk) = pf_state_nk(st, pd)
                {
                    #[cfg(feature = "inet6")]
                    if pd.af != nk.af.get() {
                        // afto: the quoted addresses swap their indexes.
                        let (sidx, didx) = (d2, s2);
                        let naf = nk.af.get();
                        if pf_translate_icmp_af(pd, naf, PfLoc::Hdr(0)) != 0 {
                            return PF_DROP;
                        }
                        icmpcopy(pd);
                        let (sa, da) = (nk.addr[sidx].get(), nk.addr[didx].get());
                        let af = pd.af;
                        if pf_change_icmp_af(m2, ipoff2 as i32, pd, &mut pd2, &sa, &da, af, naf)
                            != 0
                        {
                            return PF_DROP;
                        }
                        m2.m_pkthdr().ph_rtableid.set(u32::from(nk.rdomain.get()));
                        pd.destchg = 1;
                        pf_addrcpy(&mut pd.nsaddr, &nk.addr[s2].get(), naf);
                        pf_addrcpy(&mut pd.ndaddr, &nk.addr[d2].get(), naf);
                        if naf == AF_INET {
                            pd.proto = IPPROTO_ICMP as u8;
                        } else {
                            pd.proto = IPPROTO_ICMPV6 as u8;
                            // IPv4 becomes IPv6 so we must copy the IPv4 src addr to the
                            // least 32 bits of the IPv6 address to keep traceroute/icmp
                            // working.
                            let src = pd.ld_addr(pd.src);
                            pd.nsaddr.set_addr32(3, src.addr32(0));
                        }
                        pd.naf = naf;

                        let l = pf_hdr_loc(&mut pd2, TH_SPORT_OFF);
                        pf_patch_16(pd, l, nk.port[sidx].get());
                        let l = pf_hdr_loc(&mut pd2, TH_DPORT_OFF);
                        pf_patch_16(pd, l, nk.port[didx].get());

                        let mut b = [0u8; size_of::<Udphdr>()];
                        b.copy_from_slice(&pd2.hdr_bytes()[..size_of::<Udphdr>()]);
                        let _ =
                            crate::kern::uipc_mbuf::m_copyback(m2, pd2.off as i32, &b, M_NOWAIT);
                        return PF_AFRT;
                    }

                    let p2s = pd2.ld_addr(pd2.src);
                    if pf_aneq(&p2s, &nk.addr[s2].get(), pd2.af) || nk.port[s2].get() != uh.uh_sport
                    {
                        let (q, o) = (pd2.src, pd.dst);
                        let qp = pf_hdr_loc(&mut pd2, TH_SPORT_OFF);
                        pf_translate_icmp(pd, q, qp, o, &nk.addr[s2].get(), nk.port[s2].get());
                    }

                    let p2d = pd2.ld_addr(pd2.dst);
                    if pf_aneq(&p2d, &nk.addr[d2].get(), pd2.af) || pd2.rdomain != nk.rdomain.get()
                    {
                        pd.destchg = 1;
                    }
                    m2.m_pkthdr().ph_rtableid.set(u32::from(nk.rdomain.get()));

                    let uh = *pd2.udp();
                    if pf_aneq(&p2d, &nk.addr[d2].get(), pd2.af) || nk.port[d2].get() != uh.uh_dport
                    {
                        let (q, o) = (pd2.dst, pd.src);
                        let qp = pf_hdr_loc(&mut pd2, TH_DPORT_OFF);
                        pf_translate_icmp(pd, q, qp, o, &nk.addr[d2].get(), nk.port[d2].get());
                    }

                    if pd2.af == AF_INET {
                        icmpcopy(pd);
                        h2copy(m2, &h2);
                    }
                    #[cfg(feature = "inet6")]
                    if pd2.af == AF_INET6 {
                        icmpcopy(pd);
                        h2_6copy(m2, &h2_6);
                    }
                    // Avoid recomputing quoted UDP checksum. Note: udp6 0 csum invalid per
                    // rfc2460 p27, but presumed nothing cares in this context.
                    let l = pf_hdr_loc(&mut pd2, UH_SUM_OFF);
                    pf_patch_16(pd, l, 0);
                    let mut b = [0u8; size_of::<Udphdr>()];
                    b.copy_from_slice(&pd2.hdr_bytes()[..size_of::<Udphdr>()]);
                    let _ = crate::kern::uipc_mbuf::m_copyback(m2, pd2.off as i32, &b, M_NOWAIT);
                    copyback = 1;
                }
            }
            IPPROTO_ICMP => {
                if pd2.af != AF_INET {
                    reason_set(reason, PFRES_NORM);
                    return PF_DROP;
                }

                let mut b = [0u8; ICMP_MINLEN];
                if !pf_pull_hdr(m2, pd2.off as i32, &mut b, Some(reason), pd2.af) {
                    crate::dpfprintf!(LOG_NOTICE, "ICMP error message too short (icmp)");
                    return PF_DROP;
                }
                pd2.hdr_bytes()[..ICMP_MINLEN].copy_from_slice(&b);
                let iih_type = pd2.icmp().icmp_type;

                pf_icmp_mapping(
                    &pd2,
                    iih_type,
                    &mut icmp_dir,
                    &mut virtual_id,
                    &mut virtual_type,
                );

                let ret = pf_icmp_state_lookup(
                    &mut pd2,
                    stp,
                    virtual_id,
                    virtual_type,
                    icmp_dir,
                    &mut iidx,
                    false,
                    true,
                );
                if ret >= 0 {
                    return ret as u8;
                }
                let Some(st) = *stp else {
                    return PF_DROP;
                };

                // Translate source/destination address, if necessary.
                if !opt_eq(st.key[PF_SK_WIRE].get(), st.key[PF_SK_STACK].get())
                    && let Some(nk) = pf_state_nk(st, pd)
                {
                    let afto = pd.af != nk.af.get();
                    if afto {
                        iidx = usize::from(iidx == 0);
                    }

                    #[cfg(feature = "inet6")]
                    if afto {
                        // The quoted addresses swap their indexes.
                        let (sidx, didx) = (d2, s2);
                        let naf = nk.af.get();
                        if naf != AF_INET6 {
                            return PF_DROP;
                        }
                        if pf_translate_icmp_af(pd, naf, PfLoc::Hdr(0)) != 0 {
                            return PF_DROP;
                        }
                        icmpcopy(pd);
                        let (sa, da) = (nk.addr[sidx].get(), nk.addr[didx].get());
                        let af = pd.af;
                        if pf_change_icmp_af(m2, ipoff2 as i32, pd, &mut pd2, &sa, &da, af, naf)
                            != 0
                        {
                            return PF_DROP;
                        }
                        pd.proto = IPPROTO_ICMPV6 as u8;
                        let iih = pf_hdr_loc(&mut pd2, 0);
                        if pf_translate_icmp_af(pd, naf, iih) != 0 {
                            return PF_DROP;
                        }
                        if virtual_type == u16::from(ICMP_ECHO).to_be() {
                            let l = pf_hdr_loc(&mut pd2, ICMP_ID_OFF);
                            pf_patch_16(pd, l, nk.port[iidx].get());
                        }
                        let mut b = [0u8; ICMP_MINLEN];
                        b.copy_from_slice(&pd2.hdr_bytes()[..ICMP_MINLEN]);
                        let _ =
                            crate::kern::uipc_mbuf::m_copyback(m2, pd2.off as i32, &b, M_NOWAIT);
                        m2.m_pkthdr().ph_rtableid.set(u32::from(nk.rdomain.get()));
                        pd.destchg = 1;
                        pf_addrcpy(&mut pd.nsaddr, &nk.addr[s2].get(), naf);
                        pf_addrcpy(&mut pd.ndaddr, &nk.addr[d2].get(), naf);
                        // IPv4 becomes IPv6 so we must copy the IPv4 src addr to the least
                        // 32 bits of the IPv6 address to keep traceroute working.
                        let src = pd.ld_addr(pd.src);
                        pd.nsaddr.set_addr32(3, src.addr32(0));
                        pd.naf = naf;
                        return PF_AFRT;
                    }

                    let echo = virtual_type == u16::from(ICMP_ECHO).to_be();
                    let p2s = pd2.ld_addr(pd2.src);
                    let icmp_id = pd2.icmp().icmp_id();
                    if pf_aneq(&p2s, &nk.addr[s2].get(), pd2.af)
                        || (echo && nk.port[iidx].get() != icmp_id)
                    {
                        let (q, o) = (pd2.src, pd.dst);
                        let qp = if echo {
                            pf_hdr_loc(&mut pd2, ICMP_ID_OFF)
                        } else {
                            PfLoc::None
                        };
                        pf_translate_icmp(
                            pd,
                            q,
                            qp,
                            o,
                            &nk.addr[s2].get(),
                            if echo { nk.port[iidx].get() } else { 0 },
                        );
                    }

                    let p2d = pd2.ld_addr(pd2.dst);
                    if pf_aneq(&p2d, &nk.addr[d2].get(), pd2.af) || pd2.rdomain != nk.rdomain.get()
                    {
                        pd.destchg = 1;
                    }
                    m2.m_pkthdr().ph_rtableid.set(u32::from(nk.rdomain.get()));

                    if pf_aneq(&p2d, &nk.addr[d2].get(), pd2.af) {
                        let (q, o) = (pd2.dst, pd.src);
                        pf_translate_icmp(pd, q, PfLoc::None, o, &nk.addr[d2].get(), 0);
                    }

                    icmpcopy(pd);
                    h2copy(m2, &h2);
                    let mut b = [0u8; ICMP_MINLEN];
                    b.copy_from_slice(&pd2.hdr_bytes()[..ICMP_MINLEN]);
                    let _ = crate::kern::uipc_mbuf::m_copyback(m2, pd2.off as i32, &b, M_NOWAIT);
                    copyback = 1;
                }
            }
            #[cfg(feature = "inet6")]
            IPPROTO_ICMPV6 => {
                use crate::netinet::icmp6::{ICMP6_ECHO_REQUEST, Icmp6Hdr};
                const ICMP6_HDR_LEN: usize = size_of::<Icmp6Hdr>();

                if pd2.af != AF_INET6 {
                    reason_set(reason, PFRES_NORM);
                    return PF_DROP;
                }

                let mut b = [0u8; ICMP6_HDR_LEN];
                if !pf_pull_hdr(m2, pd2.off as i32, &mut b, Some(reason), pd2.af) {
                    crate::dpfprintf!(LOG_NOTICE, "ICMP error message too short (icmp6)");
                    return PF_DROP;
                }
                pd2.hdr_bytes()[..ICMP6_HDR_LEN].copy_from_slice(&b);
                let iih_type = pd2.icmp6().icmp6_type;

                pf_icmp_mapping(
                    &pd2,
                    iih_type,
                    &mut icmp_dir,
                    &mut virtual_id,
                    &mut virtual_type,
                );
                let mut ret = pf_icmp_state_lookup(
                    &mut pd2,
                    stp,
                    virtual_id,
                    virtual_type,
                    icmp_dir,
                    &mut iidx,
                    false,
                    true,
                );
                // IPv6? try matching a multicast address.
                if ret == i32::from(PF_DROP) && pd2.af == AF_INET6 && icmp_dir == i32::from(PF_OUT)
                {
                    ret = pf_icmp_state_lookup(
                        &mut pd2,
                        stp,
                        virtual_id,
                        virtual_type,
                        icmp_dir,
                        &mut iidx,
                        true,
                        true,
                    );
                }
                if ret >= 0 {
                    return ret as u8;
                }
                let Some(st) = *stp else {
                    return PF_DROP;
                };

                // Translate source/destination address, if necessary.
                if !opt_eq(st.key[PF_SK_WIRE].get(), st.key[PF_SK_STACK].get())
                    && let Some(nk) = pf_state_nk(st, pd)
                {
                    let afto = pd.af != nk.af.get();
                    if afto {
                        iidx = usize::from(iidx == 0);
                    }
                    let echo = virtual_type == u16::from(ICMP6_ECHO_REQUEST).to_be();

                    if afto {
                        // The quoted addresses swap their indexes.
                        let (sidx, didx) = (d2, s2);
                        let naf = nk.af.get();
                        if naf != AF_INET {
                            return PF_DROP;
                        }
                        if pf_translate_icmp_af(pd, naf, PfLoc::Hdr(0)) != 0 {
                            return PF_DROP;
                        }
                        icmpcopy(pd);
                        let (sa, da) = (nk.addr[sidx].get(), nk.addr[didx].get());
                        let af = pd.af;
                        if pf_change_icmp_af(m2, ipoff2 as i32, pd, &mut pd2, &sa, &da, af, naf)
                            != 0
                        {
                            return PF_DROP;
                        }
                        pd.proto = IPPROTO_ICMP as u8;
                        let iih = pf_hdr_loc(&mut pd2, 0);
                        if pf_translate_icmp_af(pd, naf, iih) != 0 {
                            return PF_DROP;
                        }
                        if echo {
                            let l = pf_hdr_loc(&mut pd2, ICMP_ID_OFF);
                            pf_patch_16(pd, l, nk.port[iidx].get());
                        }
                        let mut b = [0u8; ICMP6_HDR_LEN];
                        b.copy_from_slice(&pd2.hdr_bytes()[..ICMP6_HDR_LEN]);
                        let _ =
                            crate::kern::uipc_mbuf::m_copyback(m2, pd2.off as i32, &b, M_NOWAIT);
                        m2.m_pkthdr().ph_rtableid.set(u32::from(nk.rdomain.get()));
                        pd.destchg = 1;
                        pf_addrcpy(&mut pd.nsaddr, &nk.addr[s2].get(), naf);
                        pf_addrcpy(&mut pd.ndaddr, &nk.addr[d2].get(), naf);
                        pd.naf = naf;
                        return PF_AFRT;
                    }

                    let p2s = pd2.ld_addr(pd2.src);
                    // The C compares the id with the port at pd2.sidx here.
                    let icmp6_id = pd2.icmp6().icmp6_id();
                    if pf_aneq(&p2s, &nk.addr[s2].get(), pd2.af)
                        || (echo && nk.port[s2].get() != icmp6_id)
                    {
                        let (q, o) = (pd2.src, pd.dst);
                        // icmp6_id is at the offset of icmp_id.
                        let qp = if echo {
                            pf_hdr_loc(&mut pd2, ICMP_ID_OFF)
                        } else {
                            PfLoc::None
                        };
                        pf_translate_icmp(
                            pd,
                            q,
                            qp,
                            o,
                            &nk.addr[s2].get(),
                            if echo { nk.port[iidx].get() } else { 0 },
                        );
                    }

                    let p2d = pd2.ld_addr(pd2.dst);
                    if pf_aneq(&p2d, &nk.addr[d2].get(), pd2.af) || pd2.rdomain != nk.rdomain.get()
                    {
                        pd.destchg = 1;
                    }
                    m2.m_pkthdr().ph_rtableid.set(u32::from(nk.rdomain.get()));

                    if pf_aneq(&p2d, &nk.addr[d2].get(), pd2.af) {
                        let (q, o) = (pd2.dst, pd.src);
                        pf_translate_icmp(pd, q, PfLoc::None, o, &nk.addr[d2].get(), 0);
                    }

                    icmpcopy(pd);
                    h2_6copy(m2, &h2_6);
                    let mut b = [0u8; ICMP6_HDR_LEN];
                    b.copy_from_slice(&pd2.hdr_bytes()[..ICMP6_HDR_LEN]);
                    let _ = crate::kern::uipc_mbuf::m_copyback(m2, pd2.off as i32, &b, M_NOWAIT);
                    copyback = 1;
                }
            }
            _ => {
                let mut key = PfStateKeyCmp {
                    af: pd2.af,
                    proto: pd2.proto,
                    rdomain: pd2.rdomain,
                    ..PfStateKeyCmp::default()
                };
                let p2d = pd2.ld_addr(pd2.dst);
                pf_addrcpy(&mut key.addr[s2], &p2src, key.af);
                pf_addrcpy(&mut key.addr[d2], &p2d, key.af);
                key.port = [0, 0];
                key.hash = pf_pkt_hash(pd2.af, pd2.proto, &p2src, &p2d, 0, 0);

                let action = pf_find_state(&mut pd2, &key, stp);
                if action != PF_MATCH {
                    return action;
                }
                let Some(st) = *stp else {
                    return PF_DROP;
                };

                // Translate source/destination address, if necessary.
                if !opt_eq(st.key[PF_SK_WIRE].get(), st.key[PF_SK_STACK].get())
                    && let Some(nk) = st.key[usize::from(pd.didx)].get()
                {
                    let p2s = pd2.ld_addr(pd2.src);
                    if pf_aneq(&p2s, &nk.addr[s2].get(), pd2.af) {
                        let (q, o) = (pd2.src, pd.dst);
                        pf_translate_icmp(pd, q, PfLoc::None, o, &nk.addr[s2].get(), 0);
                    }

                    if pf_aneq(&p2d, &nk.addr[d2].get(), pd2.af) || pd2.rdomain != nk.rdomain.get()
                    {
                        pd.destchg = 1;
                    }
                    m2.m_pkthdr().ph_rtableid.set(u32::from(nk.rdomain.get()));

                    if pf_aneq(&p2d, &nk.addr[d2].get(), pd2.af) {
                        let (q, o) = (pd2.dst, pd.src);
                        pf_translate_icmp(pd, q, PfLoc::None, o, &nk.addr[d2].get(), 0);
                    }

                    if pd2.af == AF_INET {
                        icmpcopy(pd);
                        h2copy(m2, &h2);
                    }
                    #[cfg(feature = "inet6")]
                    if pd2.af == AF_INET6 {
                        icmpcopy(pd);
                        h2_6copy(m2, &h2_6);
                    }
                    copyback = 1;
                }
            }
        }
    }
    if copyback != 0 {
        pf_copyback_hdr(pd);
    }

    PF_PASS
}

/// `pf_pull_hdr`: copies `p.len()` bytes at `off` of the packet into `p`. `off` is measured
/// from the start of the chain, whose first mbuf holds the IP header. `false` (the C's
/// NULL) for a non-first fragment or a short packet, with the reason.
pub fn pf_pull_hdr(
    m: &Mbuf,
    off: i32,
    p: &mut [u8],
    reasonp: Option<&mut u16>,
    af: SaFamily,
) -> bool {
    let len = p.len() as i32;
    let iplen = match af {
        AF_INET => {
            let h = crate::netinet::ip_var::mtod_ip(m);
            let fragoff = (u16::from_be(h.ip_off) & crate::netinet::ip::IP_OFFMASK) << 3;

            if fragoff != 0 {
                if let Some(r) = reasonp {
                    reason_set(r, PFRES_FRAG);
                }
                return false;
            }
            i32::from(u16::from_be(h.ip_len))
        }
        #[cfg(feature = "inet6")]
        AF_INET6 => {
            let h = crate::netinet6::ip6_var::mtod_ip6(m);
            i32::from(u16::from_be(h.ip6_plen)) + size_of::<Ip6Hdr>() as i32
        }
        _ => 0,
    };
    if m.m_pkthdr().len.get() < off + len || iplen < off + len {
        if let Some(r) = reasonp {
            reason_set(r, PFRES_SHORT);
        }
        return false;
    }
    crate::kern::uipc_mbuf::m_copydata(m, off, p);
    true
}

/// `pf_routable`: `addr` has a route (`kif` absent: the no-route check), or a route through
/// `kif` (the uRPF check).
pub fn pf_routable(
    addr: &PfAddr,
    af: SaFamily,
    kif: Option<&'static PfiKif>,
    rtableid: i32,
) -> bool {
    use crate::netinet::in_::{SockaddrIn, sintosa};

    let mut check_mpath = false;
    let mut dst = SockaddrIn::default();
    if af == AF_INET {
        dst.sin_family = AF_INET;
        dst.sin_len = size_of::<SockaddrIn>() as u8;
        dst.sin_addr = addr.v4();
        if crate::netinet::ip_input::IPMULTIPATH.load(AtomicOrdering::Relaxed) != 0 {
            check_mpath = true;
        }
    }
    #[cfg(feature = "inet6")]
    let dst6 = crate::netinet6::in6::SockaddrIn6::with_addr(addr.v6());
    #[cfg(feature = "inet6")]
    if af == AF_INET6 {
        // Skip check for addresses with embedded interface scope, as they would always
        // match anyway.
        if crate::netinet6::in6::in6_is_scope_embed(&addr.v6()) {
            return true;
        }
        if crate::netinet6::in6_proto::IP6_MULTIPATH.load(AtomicOrdering::Relaxed) != 0 {
            check_mpath = true;
        }
    }
    let sa: *const crate::sys::socket::Sockaddr = sintosa(&mut dst);
    #[cfg(feature = "inet6")]
    let sa = if af == AF_INET6 {
        crate::netinet6::in6::sin6tosa_const(&dst6)
    } else {
        sa
    };

    // Skip checks for ipsec interfaces.
    if let Some(k) = kif
        && k.pfik_ifp()
            .is_some_and(|ifp| ifp.if_type.get() == crate::net::if_types::IFT_ENC)
    {
        return true;
    }

    // SAFETY: `sa` is a complete local sockaddr_in or sockaddr_in6.
    let mut rt = unsafe { crate::net::route::rtalloc(sa, 0, rtableid as u32) };
    let ret = match (rt, kif) {
        (None, _) => false,
        // No interface given, this is a no-route check.
        (Some(_), None) => true,
        (Some(_), Some(k)) => match k.pfik_ifp() {
            None => false,
            Some(kifp) => {
                // Perform uRPF check if passed input interface.
                let mut ret = false;
                while let Some(r) = rt {
                    if r.rt_ifidx.get() == kifp.if_index.get() {
                        ret = true;
                    }
                    // NCARP > 0: a carp interface on kif's parent also matches; not
                    // configured.

                    rt = crate::net::rtable::rtable_iterate(r);
                    if !(check_mpath && rt.is_some() && !ret) {
                        break;
                    }
                }
                ret
            }
        },
    };
    crate::net::route::rtfree(rt);
    ret
}

/// `pf_rtlabel_match`: the route to `addr` carries the operand's route label.
pub fn pf_rtlabel_match(addr: &PfAddr, af: SaFamily, aw: &PfAddrWrap, rtableid: i32) -> bool {
    use crate::netinet::in_::{SockaddrIn, sintosa};

    let mut dst = SockaddrIn::default();
    if af == AF_INET {
        dst.sin_family = AF_INET;
        dst.sin_len = size_of::<SockaddrIn>() as u8;
        dst.sin_addr = addr.v4();
    }
    let sa: *const crate::sys::socket::Sockaddr = sintosa(&mut dst);
    #[cfg(feature = "inet6")]
    let dst6 = crate::netinet6::in6::SockaddrIn6::with_addr(addr.v6());
    #[cfg(feature = "inet6")]
    let sa = if af == AF_INET6 {
        crate::netinet6::in6::sin6tosa_const(&dst6)
    } else {
        sa
    };

    let mut ret = false;
    // SAFETY: `sa` is a complete local sockaddr_in or sockaddr_in6.
    let rt =
        unsafe { crate::net::route::rtalloc(sa, crate::net::route::RT_RESOLVE, rtableid as u32) };
    if let Some(r) = rt {
        if u32::from(r.rt_labelid.get()) == aw.v.get().rtlabel() {
            ret = true;
        }
        crate::net::route::rtfree(rt);
    }

    ret
}

/// `PF_MISMATCHAW(aw, x, af, neg, ifp, rtid)`: the operand does not match `x` (with `neg`
/// inverting).
pub fn pf_mismatchaw(
    aw: &'static PfAddrWrap,
    x: &PfAddr,
    af: SaFamily,
    neg: bool,
    ifp: Option<&'static PfiKif>,
    rtid: i32,
) -> bool {
    let v = aw.v.get();
    let mismatch = match aw.type_.get() {
        PF_ADDR_NOROUTE => pf_routable(x, af, None, rtid),
        PF_ADDR_URPFFAILED => ifp.is_some() && pf_routable(x, af, ifp, rtid),
        PF_ADDR_RTLABEL => !pf_rtlabel_match(x, af, aw, rtid),
        PF_ADDR_TABLE => !aw
            .tbl()
            .is_some_and(|t| crate::net::pf_table::pfr_match_addr(t, x, af)),
        PF_ADDR_DYNIFTL => !aw
            .dyn_()
            .is_some_and(|d| crate::net::pf_if::pfi_match_addr(d, x, af)),
        PF_ADDR_RANGE => !pf_match_addr_range(&v.addr(), &v.mask(), x, af),
        PF_ADDR_ADDRMASK => {
            !pf_azero(&v.mask(), af) && !pf_match_addr(0, &v.addr(), &v.mask(), x, af)
        }
        _ => false,
    };
    mismatch != neg
}

/// `pf_route`: sends the packet out of the state's route-to/reply-to/dup-to address. It may
/// take `pd->m` (route-to, reply-to) and leave `None` there.
pub fn pf_route(pd: &mut PfPdesc, st: &'static PfState) {
    use crate::netinet::in_::{IN_CLASSA_NSHIFT, IN_LOOPBACKNET, SockaddrIn, sintosa};
    use crate::netinet::ip::{IP_DF, IPTTLDEC, Ip};
    use crate::netinet::ip_icmp::{
        ICMP_TIMXCEED, ICMP_TIMXCEED_INTRANS, ICMP_UNREACH, ICMP_UNREACH_HOST,
        ICMP_UNREACH_NEEDFRAG,
    };
    use crate::netinet::ip_var::{IpstatCounters, ipstat_inc, mtod_ip, mtod_ip_store};

    let Some(m) = pd.m else {
        return;
    };
    let routed = m.m_pkthdr().pf.routed.get();
    m.m_pkthdr().pf.routed.set(routed.wrapping_add(1));
    if routed > 3 {
        crate::kern::uipc_mbuf::m_freem(m);
        pd.m = None;
        return;
    }

    let m0 = if st.rt.get() == PF_DUPTO {
        let linkhdr = crate::kern::uipc_mbuf::MAX_LINKHDR.load(AtomicOrdering::Relaxed) as u32;
        match crate::kern::uipc_mbuf::m_dup_pkt(m, linkhdr, M_NOWAIT) {
            Some(m0) => m0,
            None => return,
        }
    } else {
        if (st.rt.get() == PF_REPLYTO) == (st.direction.get() == pd.dir) {
            return;
        }
        pd.m = None;
        m
    };
    let mut m0: Option<&'static Mbuf> = Some(m0);
    let mut rt: Option<&'static crate::net::route::Rtentry> = None;
    let mut ifp: Option<&'static Ifnet> = None;
    let mut dst = SockaddrIn::default();

    'done: {
        'bad: {
            let Some(mm) = m0 else {
                break 'done;
            };
            if (mm.m_len().get() as usize) < size_of::<Ip>() {
                crate::dpfprintf!(LOG_ERR, "pf_route: m0->m_len < sizeof(struct ip)");
                break 'bad;
            }

            let mut ip = mtod_ip(mm);

            if pd.dir == PF_IN {
                if ip.ip_ttl <= IPTTLDEC {
                    if st.rt.get() != PF_DUPTO {
                        pf_send_icmp(
                            mm,
                            ICMP_TIMXCEED,
                            ICMP_TIMXCEED_INTRANS,
                            0,
                            pd.af,
                            st.rule.ptr(),
                            u32::from(pd.rdomain),
                        );
                    }
                    break 'bad;
                }
                ip.ip_ttl -= IPTTLDEC;
                mtod_ip_store(mm, &ip);
            }

            dst.sin_family = AF_INET;
            dst.sin_len = size_of::<SockaddrIn>() as u8;
            dst.sin_addr = st.rt_addr.get().v4();
            let rtableid = mm.m_pkthdr().ph_rtableid.get();

            let src = [ip.ip_src.s_addr];
            // SAFETY: `dst` is a complete local sockaddr_in.
            rt = unsafe {
                crate::net::route::rtalloc_mpath(sintosa(&mut dst), Some(&src), rtableid)
            };
            if !crate::net::route::rtisvalid(rt) {
                if st.rt.get() != PF_DUPTO {
                    pf_send_icmp(
                        mm,
                        ICMP_UNREACH,
                        ICMP_UNREACH_HOST,
                        0,
                        pd.af,
                        st.rule.ptr(),
                        u32::from(pd.rdomain),
                    );
                }
                ipstat_inc(IpstatCounters::IpsNoroute);
                break 'bad;
            }
            let Some(r) = rt else {
                break 'bad;
            };

            ifp = if_get(r.rt_ifidx.get());
            let Some(ifn) = ifp else {
                break 'bad;
            };

            // A locally generated packet may have invalid source address.
            if (u32::from_be(ip.ip_src.s_addr) >> IN_CLASSA_NSHIFT) == IN_LOOPBACKNET
                && ifn.if_flags.get() & IFF_LOOPBACK == 0
                && let Some(ifa) = r.rt_ifa.get()
            {
                ip.ip_src = crate::netinet::in_var::ifatoia(ifa).ia_addr.get().sin_addr;
                mtod_ip_store(mm, &ip);
            }

            if st.rt.get() != PF_DUPTO && pd.dir == PF_IN {
                if pf_test(AF_INET, PF_OUT, ifn, &mut m0) != PF_PASS {
                    break 'bad;
                }
                let Some(mm) = m0 else {
                    break 'done;
                };
                if (mm.m_len().get() as usize) < size_of::<Ip>() {
                    crate::dpfprintf!(LOG_ERR, "pf_route: m0->m_len < sizeof(struct ip)");
                    break 'bad;
                }
                ip = mtod_ip(mm);
            }

            // SAFETY: `dst` is a complete local sockaddr_in.
            let tso = unsafe {
                crate::net::if_::if_output_tso(
                    ifn,
                    &mut m0,
                    sintosa(&mut dst),
                    rt,
                    ifn.if_mtu.get(),
                )
            };
            if tso.is_err() || m0.is_none() {
                break 'done;
            }
            let Some(mm) = m0 else {
                break 'done;
            };

            // Too large for interface; fragment if possible. Must be able to put at least 8
            // bytes per fragment.
            if ip.ip_off & IP_DF.to_be() != 0 {
                ipstat_inc(IpstatCounters::IpsCantfrag);
                if st.rt.get() != PF_DUPTO {
                    pf_send_icmp(
                        mm,
                        ICMP_UNREACH,
                        ICMP_UNREACH_NEEDFRAG,
                        ifn.if_mtu.get() as i32,
                        pd.af,
                        st.rule.ptr(),
                        u32::from(pd.rdomain),
                    );
                }
                break 'bad;
            }

            let ml = crate::sys::mbuf::MbufList::new();
            if crate::netinet::ip_output::ip_fragment(mm, &ml, ifn, u64::from(ifn.if_mtu.get()))
                .is_err()
            {
                break 'done;
            }
            // SAFETY: `dst` is a complete local sockaddr_in.
            if unsafe { crate::net::if_::if_output_ml(ifn, &ml, sintosa(&mut dst), rt) }.is_err() {
                break 'done;
            }
            ipstat_inc(IpstatCounters::IpsFragmented);
            break 'done;
        }
        // bad:
        crate::kern::uipc_mbuf::m_freem(m0);
    }
    // done:
    if_put(ifp);
    crate::net::route::rtfree(rt);
}

/// `pf_route6`: the IPv6 `route-to`/`reply-to`/`dup-to` of a state: sends the packet (or a
/// copy) out of the interface of the route to the state's `rt_addr`, refragmenting a packet
/// pf reassembled. May take `pd->m`.
#[cfg(feature = "inet6")]
pub fn pf_route6(pd: &mut PfPdesc, st: &'static PfState) {
    use crate::netinet::icmp6::{
        ICMP6_DST_UNREACH, ICMP6_DST_UNREACH_NOROUTE, ICMP6_PACKET_TOO_BIG,
        ICMP6_TIME_EXCEED_TRANSIT, ICMP6_TIME_EXCEEDED,
    };
    use crate::netinet::ip6::IPV6_HLIMDEC;
    use crate::netinet6::in6::{SockaddrIn6, ifatoia6, in6_is_addr_loopback, sin6tosa_const};
    use crate::netinet6::ip6_var::{Ip6statCounters, ip6stat_inc, mtod_ip6, mtod_ip6_store};
    use crate::sys::mbuf::PACKET_TAG_PF_REASSEMBLED;

    let Some(m) = pd.m else {
        return;
    };
    let routed = m.m_pkthdr().pf.routed.get();
    m.m_pkthdr().pf.routed.set(routed.wrapping_add(1));
    if routed > 3 {
        crate::kern::uipc_mbuf::m_freem(m);
        pd.m = None;
        return;
    }

    let m0 = if st.rt.get() == PF_DUPTO {
        let linkhdr = crate::kern::uipc_mbuf::MAX_LINKHDR.load(AtomicOrdering::Relaxed) as u32;
        match crate::kern::uipc_mbuf::m_dup_pkt(m, linkhdr, M_NOWAIT) {
            Some(m0) => m0,
            None => return,
        }
    } else {
        if (st.rt.get() == PF_REPLYTO) == (st.direction.get() == pd.dir) {
            return;
        }
        pd.m = None;
        m
    };
    let mut m0: Option<&'static Mbuf> = Some(m0);
    let mut rt: Option<&'static crate::net::route::Rtentry> = None;
    let mut ifp: Option<&'static Ifnet> = None;
    let dst = SockaddrIn6::with_addr(st.rt_addr.get().v6());

    'done: {
        'bad: {
            let Some(mm) = m0 else {
                break 'done;
            };
            if (mm.m_len().get() as usize) < size_of::<Ip6Hdr>() {
                crate::dpfprintf!(LOG_ERR, "pf_route6: m0->m_len < sizeof(struct ip6_hdr)");
                break 'bad;
            }
            let mut ip6 = mtod_ip6(mm);

            if pd.dir == PF_IN {
                if ip6.ip6_hlim <= IPV6_HLIMDEC {
                    if st.rt.get() != PF_DUPTO {
                        pf_send_icmp(
                            mm,
                            ICMP6_TIME_EXCEEDED,
                            ICMP6_TIME_EXCEED_TRANSIT,
                            0,
                            pd.af,
                            st.rule.ptr(),
                            u32::from(pd.rdomain),
                        );
                    }
                    break 'bad;
                }
                ip6.ip6_hlim -= IPV6_HLIMDEC;
                mtod_ip6_store(mm, &ip6);
            }

            let rtableid = mm.m_pkthdr().ph_rtableid.get();

            let src = [ip6.ip6_src.s6_addr32(0)];
            // SAFETY: `dst` is a complete local sockaddr_in6.
            rt = unsafe {
                crate::net::route::rtalloc_mpath(sin6tosa_const(&dst), Some(&src), rtableid)
            };
            if !crate::net::route::rtisvalid(rt) {
                if st.rt.get() != PF_DUPTO {
                    pf_send_icmp(
                        mm,
                        ICMP6_DST_UNREACH,
                        ICMP6_DST_UNREACH_NOROUTE,
                        0,
                        pd.af,
                        st.rule.ptr(),
                        u32::from(pd.rdomain),
                    );
                }
                ip6stat_inc(Ip6statCounters::Ip6sNoroute);
                break 'bad;
            }
            let Some(r) = rt else {
                break 'bad;
            };

            ifp = if_get(r.rt_ifidx.get());
            let Some(ifn) = ifp else {
                break 'bad;
            };

            // A locally generated packet may have invalid source address.
            if in6_is_addr_loopback(&ip6.ip6_src)
                && ifn.if_flags.get() & IFF_LOOPBACK == 0
                && let Some(ifa) = r.rt_ifa.get()
            {
                ip6.ip6_src = ifatoia6(ifa).ia_addr.get().sin6_addr;
                mtod_ip6_store(mm, &ip6);
            }

            if st.rt.get() != PF_DUPTO && pd.dir == PF_IN {
                if pf_test(AF_INET6, PF_OUT, ifn, &mut m0) != PF_PASS {
                    break 'bad;
                }
                let Some(mm) = m0 else {
                    break 'done;
                };
                if (mm.m_len().get() as usize) < size_of::<Ip6Hdr>() {
                    crate::dpfprintf!(LOG_ERR, "pf_route6: m0->m_len < sizeof(struct ip6_hdr)");
                    break 'bad;
                }
            }
            let Some(mm) = m0 else {
                break 'done;
            };

            // If packet has been reassembled by PF earlier, we have to use pf_refragment6()
            // here to turn it back to fragments.
            if let Some(mtag) =
                crate::kern::uipc_mbuf2::m_tag_find(mm, PACKET_TAG_PF_REASSEMBLED, None)
            {
                let _ = crate::net::pf_norm::pf_refragment6(&mut m0, mtag, Some(&dst), ifp, rt);
                break 'done;
            }

            // SAFETY: `dst` is a complete local sockaddr_in6.
            let tso = unsafe {
                crate::net::if_::if_output_tso(
                    ifn,
                    &mut m0,
                    sin6tosa_const(&dst),
                    rt,
                    ifn.if_mtu.get(),
                )
            };
            if tso.is_err() || m0.is_none() {
                break 'done;
            }
            let Some(mm) = m0 else {
                break 'done;
            };

            ip6stat_inc(Ip6statCounters::Ip6sCantfrag);
            if st.rt.get() != PF_DUPTO {
                pf_send_icmp(
                    mm,
                    ICMP6_PACKET_TOO_BIG,
                    0,
                    ifn.if_mtu.get() as i32,
                    pd.af,
                    st.rule.ptr(),
                    u32::from(pd.rdomain),
                );
            }
            break 'bad;
        }
        // bad:
        crate::kern::uipc_mbuf::m_freem(m0);
    }
    // done:
    if_put(ifp);
    crate::net::route::rtfree(rt);
}

/// `pf_check_tcp_cksum`: checks the TCP checksum of the segment at `off` (`len` bytes of
/// header and payload) and records the result in the mbuf. `true` when it is bad (the C's
/// 1); a checksum still to be computed on output (`_OUT`) counts as good.
pub fn pf_check_tcp_cksum(m: &Mbuf, off: i32, len: i32, af: SaFamily) -> bool {
    use crate::netinet::ip::Ip;

    let cf = &m.m_pkthdr().csum_flags;
    if cf.get() & (M_TCP_CSUM_IN_OK | M_TCP_CSUM_OUT) != 0 {
        return false;
    }
    if cf.get() & M_TCP_CSUM_IN_BAD != 0
        || (off as usize) < size_of::<Ip>()
        || m.m_pkthdr().len.get() < off + len
    {
        return true;
    }

    // need to do it in software
    tcpstat_inc(TcpstatCounters::TcpsInswcsum);

    let sum = match af {
        AF_INET => {
            if (m.m_len().get() as usize) < size_of::<Ip>() {
                return true;
            }

            crate::netinet::in4_cksum::in4_cksum(m, IPPROTO_TCP as u8, off, len)
        }
        #[cfg(feature = "inet6")]
        AF_INET6 => {
            if (m.m_len().get() as usize) < size_of::<Ip6Hdr>() {
                return true;
            }

            crate::netinet6::in6_cksum::in6_cksum(m, IPPROTO_TCP as u8, off as u32, len as u32)
        }
        _ => unhandled_af(i32::from(af)),
    };
    if sum != 0 {
        tcpstat_inc(TcpstatCounters::TcpsRcvbadsum);
        cf.set(cf.get() | M_TCP_CSUM_IN_BAD);
        return true;
    }

    cf.set(cf.get() | M_TCP_CSUM_IN_OK);
    false
}

/// `pf_find_divert`: the packet's divert tag, if any.
pub fn pf_find_divert(m: &Mbuf) -> Option<PfDivert> {
    let t = crate::kern::uipc_mbuf2::m_tag_find(m, PACKET_TAG_PF_DIVERT, None)?;
    // SAFETY: a PACKET_TAG_PF_DIVERT tag carries a `struct pf_divert` (pf_get_divert made it
    // with that length); read unaligned.
    Some(unsafe { ptr::read_unaligned(t.data().cast::<PfDivert>()) })
}

/// `pf_get_divert`: the packet's divert tag, made if missing; `None` without memory. The
/// tag's data is written with `pf_set_divert`.
pub fn pf_get_divert(m: &Mbuf) -> Option<&MTag> {
    use crate::kern::uipc_mbuf2::{m_tag_find, m_tag_get, m_tag_prepend};

    if let Some(t) = m_tag_find(m, PACKET_TAG_PF_DIVERT, None) {
        return Some(t);
    }
    let t = m_tag_get(PACKET_TAG_PF_DIVERT, size_of::<PfDivert>() as i32, M_NOWAIT)?;
    // SAFETY: the tag was allocated with room for a `struct pf_divert` after it.
    unsafe { ptr::write_unaligned(t.data().cast::<PfDivert>(), PfDivert::default()) };
    m_tag_prepend(m, t);
    m_tag_find(m, PACKET_TAG_PF_DIVERT, None)
}

/// Stores `d` in a divert tag from `pf_get_divert`.
fn pf_set_divert(t: &MTag, d: &PfDivert) {
    // SAFETY: a PACKET_TAG_PF_DIVERT tag has room for a `struct pf_divert`.
    unsafe { ptr::write_unaligned(t.data().cast::<PfDivert>(), *d) };
}

/// `pf_walk_option`: checks the IPv4 options at `off..end` of the packet and records what it
/// finds in `pd->badopts`.
pub fn pf_walk_option(
    pd: &mut PfPdesc,
    _h: &crate::netinet::ip::Ip,
    off: i32,
    end: i32,
    reason: &mut u16,
) -> u8 {
    use crate::netinet::ip::{IPOPT_EOL, IPOPT_NOP, IPOPT_RA, Ip};

    let mut opts = [0u8; 15 * 4 - size_of::<Ip>()];

    let Some(m) = pd.m else {
        return PF_DROP;
    };
    // IP header in payload of ICMP packet may be too short.
    if m.m_pkthdr().len.get() < end {
        crate::dpfprintf!(LOG_NOTICE, "IP option too short");
        reason_set(reason, PFRES_SHORT);
        return PF_DROP;
    }

    kassert!((end - off) as usize <= opts.len());
    let n = ((end - off) as usize).min(opts.len());
    crate::kern::uipc_mbuf::m_copydata(m, off, &mut opts[..n]);
    let end = n;
    let mut off = 0usize;

    while off < end {
        let type_ = opts[off];
        if type_ == IPOPT_EOL {
            break;
        }
        if type_ == IPOPT_NOP {
            off += 1;
            continue;
        }
        if off + 2 > end {
            crate::dpfprintf!(LOG_NOTICE, "IP length opt");
            reason_set(reason, PFRES_IPOPTIONS);
            return PF_DROP;
        }
        let length = usize::from(opts[off + 1]);
        if length < 2 {
            crate::dpfprintf!(LOG_NOTICE, "IP short opt");
            reason_set(reason, PFRES_IPOPTIONS);
            return PF_DROP;
        }
        if off + length > end {
            crate::dpfprintf!(LOG_NOTICE, "IP long opt");
            reason_set(reason, PFRES_IPOPTIONS);
            return PF_DROP;
        }
        if type_ == IPOPT_RA {
            pd.badopts |= PF_OPT_ROUTER_ALERT;
        } else {
            pd.badopts |= PF_OPT_OTHER;
        }
        off += length;
    }

    PF_PASS
}

/// `pf_walk_header`: walks the IPv4 header (options, then AH headers) to the protocol
/// header; sets `pd->off` and `pd->proto`.
pub fn pf_walk_header(pd: &mut PfPdesc, h: &crate::netinet::ip::Ip, reason: &mut u16) -> u8 {
    use crate::netinet::in_::{INADDR_ALLHOSTS_GROUP, IPPROTO_AH, IPPROTO_IGMP};
    use crate::netinet::ip::{IP_MF, IP_OFFMASK, Ip};

    let hlen = u32::from(h.ip_hl()) << 2;
    if (hlen as usize) < size_of::<Ip>() || hlen > u32::from(u16::from_be(h.ip_len)) {
        reason_set(reason, PFRES_SHORT);
        return PF_DROP;
    }
    if hlen as usize != size_of::<Ip>() {
        let off = pd.off as i32;
        if pf_walk_option(
            pd,
            h,
            off + size_of::<Ip>() as i32,
            off + hlen as i32,
            reason,
        ) != PF_PASS
        {
            return PF_DROP;
        }
        // Header options which contain only padding is fishy.
        if pd.badopts == 0 {
            pd.badopts |= PF_OPT_OTHER;
        }
    }
    let end = pd.off + u32::from(u16::from_be(h.ip_len));
    pd.off += hlen;
    pd.proto = h.ip_p;
    // IGMP packets have router alert options, allow them.
    if i32::from(pd.proto) == IPPROTO_IGMP {
        // According to RFC 1112 ttl must be set to 1 in all IGMP packets sent to 224.0.0.1.
        if h.ip_ttl != 1 && h.ip_dst.s_addr == INADDR_ALLHOSTS_GROUP {
            crate::dpfprintf!(LOG_NOTICE, "Invalid IGMP");
            reason_set(reason, PFRES_IPOPTIONS);
            return PF_DROP;
        }
        pd.badopts &= !PF_OPT_ROUTER_ALERT;
    }
    // Stop walking over non initial fragments.
    if h.ip_off & IP_OFFMASK.to_be() != 0 {
        return PF_PASS;
    }

    let Some(m) = pd.m else {
        return PF_DROP;
    };
    for _ in 0..PF_HDR_LIMIT.load(AtomicOrdering::Relaxed) {
        match i32::from(pd.proto) {
            IPPROTO_AH => {
                // `struct ip6_ext`: next header and length.
                let mut ext = [0u8; 2];
                // Fragments may be short.
                if h.ip_off & (IP_MF | IP_OFFMASK).to_be() != 0
                    && (end as usize) < pd.off as usize + ext.len()
                {
                    return PF_PASS;
                }
                if !pf_pull_hdr(m, pd.off as i32, &mut ext, Some(reason), AF_INET) {
                    crate::dpfprintf!(LOG_NOTICE, "IP short exthdr");
                    return PF_DROP;
                }
                pd.off += (u32::from(ext[1]) + 2) * 4;
                pd.proto = ext[0];
            }
            _ => return PF_PASS,
        }
    }
    crate::dpfprintf!(LOG_NOTICE, "IPv4 nested authentication header limit");
    reason_set(reason, PFRES_IPOPTIONS);
    PF_DROP
}

/// `pf_walk_option6`: walks the options of a hop-by-hop header from `off` to `end`, noting
/// the jumbo payload and router alert options in `pd->badopts`.
#[cfg(feature = "inet6")]
pub fn pf_walk_option6(pd: &mut PfPdesc, h: &Ip6Hdr, off: i32, end: i32, reason: &mut u16) -> u8 {
    use crate::netinet::ip6::{
        IP6OPT_JUMBO, IP6OPT_PAD1, IP6OPT_PADN, IP6OPT_ROUTER_ALERT, IPV6_MAXPACKET, Ip6Opt,
        Ip6OptJumbo,
    };

    let Some(m) = pd.m else {
        return PF_DROP;
    };
    let mut off = off;
    while off < end {
        // `struct ip6_opt`: type and length.
        let mut opt = [0u8; size_of::<Ip6Opt>()];
        if !pf_pull_hdr(m, off, &mut opt[..1], Some(reason), AF_INET6) {
            crate::dpfprintf!(LOG_NOTICE, "IPv6 short opt type");
            return PF_DROP;
        }
        if opt[0] == IP6OPT_PAD1 {
            off += 1;
            continue;
        }
        if !pf_pull_hdr(m, off, &mut opt, Some(reason), AF_INET6) {
            crate::dpfprintf!(LOG_NOTICE, "IPv6 short opt");
            return PF_DROP;
        }
        let (ip6o_type, ip6o_len) = (opt[0], i32::from(opt[1]));
        if off + size_of::<Ip6Opt>() as i32 + ip6o_len > end {
            crate::dpfprintf!(LOG_NOTICE, "IPv6 long opt");
            reason_set(reason, PFRES_IPOPTIONS);
            return PF_DROP;
        }
        match ip6o_type {
            IP6OPT_PADN => {}
            IP6OPT_JUMBO => {
                pd.badopts |= PF_OPT_JUMBO;
                if pd.jumbolen != 0 {
                    crate::dpfprintf!(LOG_NOTICE, "IPv6 multiple jumbo");
                    reason_set(reason, PFRES_IPOPTIONS);
                    return PF_DROP;
                }
                if u16::from_be(h.ip6_plen) != 0 {
                    crate::dpfprintf!(LOG_NOTICE, "IPv6 bad jumbo plen");
                    reason_set(reason, PFRES_IPOPTIONS);
                    return PF_DROP;
                }
                let mut jumbo = [0u8; size_of::<Ip6OptJumbo>()];
                if !pf_pull_hdr(m, off, &mut jumbo, Some(reason), AF_INET6) {
                    crate::dpfprintf!(LOG_NOTICE, "IPv6 short jumbo");
                    return PF_DROP;
                }
                // ip6oj_jumbo_len follows the type and length.
                pd.jumbolen = u32::from_be_bytes([jumbo[2], jumbo[3], jumbo[4], jumbo[5]]);
                if (pd.jumbolen as usize) < IPV6_MAXPACKET {
                    crate::dpfprintf!(LOG_NOTICE, "IPv6 short jumbolen");
                    reason_set(reason, PFRES_IPOPTIONS);
                    return PF_DROP;
                }
            }
            IP6OPT_ROUTER_ALERT => pd.badopts |= PF_OPT_ROUTER_ALERT,
            _ => pd.badopts |= PF_OPT_OTHER,
        }
        off += size_of::<Ip6Opt>() as i32 + ip6o_len;
    }

    PF_PASS
}

/// `pf_walk_header6`: walks the IPv6 header and its extension headers (hop-by-hop,
/// routing, fragment, AH, destination options) to the protocol header; sets `pd->off`,
/// `pd->proto`, `pd->fragoff`, `pd->extoff` and `pd->jumbolen`, and checks MLD messages.
#[cfg(feature = "inet6")]
pub fn pf_walk_header6(pd: &mut PfPdesc, h: &Ip6Hdr, reason: &mut u16) -> u8 {
    use crate::netinet::icmp6::{
        Icmp6Hdr, MLD_LISTENER_DONE, MLD_LISTENER_QUERY, MLD_LISTENER_REPORT, MLDV2_LISTENER_REPORT,
    };
    use crate::netinet::in_::{
        IPPROTO_AH, IPPROTO_DSTOPTS, IPPROTO_FRAGMENT, IPPROTO_HOPOPTS, IPPROTO_ROUTING,
    };
    use crate::netinet::ip6::{IP6F_MORE_FRAG, IP6F_OFF_MASK, Ip6Ext, Ip6Frag, Ip6Rthdr};
    use crate::netinet6::in6::{IPV6_RTHDR_TYPE_0, in6_is_addr_linklocal, in6_is_addr_unspecified};

    let Some(m) = pd.m else {
        return PF_DROP;
    };
    let mut fraghdr_cnt = 0;
    let mut rthdr_cnt = 0;

    pd.off += size_of::<Ip6Hdr>() as u32;
    let end = pd.off + u32::from(u16::from_be(h.ip6_plen));
    pd.fragoff = 0;
    pd.extoff = 0;
    pd.jumbolen = 0;
    pd.proto = h.ip6_nxt;

    // `struct ip6_ext`: next header and length.
    let mut ext = [0u8; size_of::<Ip6Ext>()];
    for hdr_cnt in 0..PF_HDR_LIMIT.load(AtomicOrdering::Relaxed) {
        let p = i32::from(pd.proto);
        if p == IPPROTO_ROUTING || p == IPPROTO_DSTOPTS {
            pd.badopts |= PF_OPT_OTHER;
        } else if p == IPPROTO_HOPOPTS {
            if !pf_pull_hdr(m, pd.off as i32, &mut ext, Some(reason), AF_INET6) {
                crate::dpfprintf!(LOG_NOTICE, "IPv6 short exthdr");
                return PF_DROP;
            }
            let off = pd.off as i32;
            if pf_walk_option6(
                pd,
                h,
                off + size_of::<Ip6Ext>() as i32,
                off + (i32::from(ext[1]) + 1) * 8,
                reason,
            ) != PF_PASS
            {
                return PF_DROP;
            }
            // Option header which contains only padding is fishy.
            if pd.badopts == 0 {
                pd.badopts |= PF_OPT_OTHER;
            }
        }

        // The routing header falls through to the hop-by-hop check, both to the
        // AH/destination options handling.
        let mut ext_hdr = false;
        match i32::from(pd.proto) {
            IPPROTO_FRAGMENT => {
                fraghdr_cnt += 1;
                if fraghdr_cnt > 1 {
                    crate::dpfprintf!(LOG_NOTICE, "IPv6 multiple fragment");
                    reason_set(reason, PFRES_FRAG);
                    return PF_DROP;
                }
                // Jumbo payload packets cannot be fragmented.
                if pd.jumbolen != 0 {
                    crate::dpfprintf!(LOG_NOTICE, "IPv6 fragmented jumbo");
                    reason_set(reason, PFRES_FRAG);
                    return PF_DROP;
                }
                let mut b = [0u8; size_of::<Ip6Frag>()];
                if !pf_pull_hdr(m, pd.off as i32, &mut b, Some(reason), AF_INET6) {
                    crate::dpfprintf!(LOG_NOTICE, "IPv6 short fragment");
                    return PF_DROP;
                }
                // SAFETY: `Ip6Frag` is 8 bytes of integers (`#[repr(C)]`, no padding).
                let frag = unsafe { ptr::read_unaligned(b.as_ptr().cast::<Ip6Frag>()) };
                // Stop walking over non initial fragments.
                if u16::from_be(frag.ip6f_offlg & IP6F_OFF_MASK) != 0 {
                    pd.fragoff = pd.off;
                    return PF_PASS;
                }
                // RFC6946: reassemble only non atomic fragments.
                if frag.ip6f_offlg & IP6F_MORE_FRAG != 0 {
                    pd.fragoff = pd.off;
                }
                pd.off += size_of::<Ip6Frag>() as u32;
                pd.proto = frag.ip6f_nxt;
            }
            IPPROTO_ROUTING => {
                rthdr_cnt += 1;
                if rthdr_cnt > 1 {
                    crate::dpfprintf!(LOG_NOTICE, "IPv6 multiple rthdr");
                    reason_set(reason, PFRES_IPOPTIONS);
                    return PF_DROP;
                }
                // Fragments may be short.
                if pd.fragoff != 0 && (end as usize) < pd.off as usize + size_of::<Ip6Rthdr>() {
                    pd.off = pd.fragoff;
                    pd.proto = IPPROTO_FRAGMENT as u8;
                    return PF_PASS;
                }
                let mut rthdr = [0u8; size_of::<Ip6Rthdr>()];
                if !pf_pull_hdr(m, pd.off as i32, &mut rthdr, Some(reason), AF_INET6) {
                    crate::dpfprintf!(LOG_NOTICE, "IPv6 short rthdr");
                    return PF_DROP;
                }
                // ip6r_type is the third byte.
                if i32::from(rthdr[2]) == IPV6_RTHDR_TYPE_0 {
                    crate::dpfprintf!(LOG_NOTICE, "IPv6 rthdr0");
                    reason_set(reason, PFRES_IPOPTIONS);
                    return PF_DROP;
                }
                ext_hdr = true;
            }
            IPPROTO_HOPOPTS | IPPROTO_AH | IPPROTO_DSTOPTS => ext_hdr = true,
            crate::netinet::in_::IPPROTO_ICMPV6 => {
                // Fragments may be short, ignore inner header then.
                if pd.fragoff != 0 && (end as usize) < pd.off as usize + size_of::<Icmp6Hdr>() {
                    pd.off = pd.fragoff;
                    pd.proto = IPPROTO_FRAGMENT as u8;
                    return PF_PASS;
                }
                let mut icmp6 = [0u8; size_of::<Icmp6Hdr>()];
                if !pf_pull_hdr(m, pd.off as i32, &mut icmp6, Some(reason), AF_INET6) {
                    crate::dpfprintf!(LOG_NOTICE, "IPv6 short icmp6hdr");
                    return PF_DROP;
                }
                // ICMP multicast packets have router alert options.
                let icmp6_type = icmp6[0];
                if matches!(
                    icmp6_type,
                    MLD_LISTENER_QUERY
                        | MLD_LISTENER_REPORT
                        | MLD_LISTENER_DONE
                        | MLDV2_LISTENER_REPORT
                ) {
                    // According to RFC 2710 all MLD messages are sent with hop-limit (ttl)
                    // set to 1, and link local source address. If either one is missing then
                    // the MLD message is invalid and should be discarded. RFC 3590 clarifies
                    // that during initial duplicate address detection nodes may not have an
                    // address, so are permitted to use the unspecified address, but only for
                    // Report and Done messages.
                    let ll = in6_is_addr_linklocal(&h.ip6_src);
                    if h.ip6_hlim != 1
                        || (!ll && icmp6_type == MLD_LISTENER_QUERY)
                        || (!ll && !in6_is_addr_unspecified(&h.ip6_src))
                    {
                        crate::dpfprintf!(LOG_NOTICE, "Invalid MLD");
                        reason_set(reason, PFRES_IPOPTIONS);
                        return PF_DROP;
                    }
                    pd.badopts &= !PF_OPT_ROUTER_ALERT;
                }
                return PF_PASS;
            }
            p @ (IPPROTO_TCP | IPPROTO_UDP) => {
                // Fragments may be short, ignore inner header then.
                let hl = if p == IPPROTO_TCP {
                    size_of::<Tcphdr>()
                } else {
                    size_of::<Udphdr>()
                };
                if pd.fragoff != 0 && (end as usize) < pd.off as usize + hl {
                    pd.off = pd.fragoff;
                    pd.proto = IPPROTO_FRAGMENT as u8;
                }
                return PF_PASS;
            }
            _ => return PF_PASS,
        }
        if !ext_hdr {
            continue;
        }

        // IPPROTO_ROUTING (after its checks), IPPROTO_HOPOPTS, IPPROTO_AH, IPPROTO_DSTOPTS.
        let p = i32::from(pd.proto);
        // RFC2460 4.1: Hop-by-Hop only after IPv6 header.
        if p == IPPROTO_HOPOPTS && hdr_cnt > 0 {
            crate::dpfprintf!(LOG_NOTICE, "IPv6 hopopts not first");
            reason_set(reason, PFRES_IPOPTIONS);
            return PF_DROP;
        }
        // Fragments may be short.
        if pd.fragoff != 0 && (end as usize) < pd.off as usize + size_of::<Ip6Ext>() {
            pd.off = pd.fragoff;
            pd.proto = IPPROTO_FRAGMENT as u8;
            return PF_PASS;
        }
        if !pf_pull_hdr(m, pd.off as i32, &mut ext, Some(reason), AF_INET6) {
            crate::dpfprintf!(LOG_NOTICE, "IPv6 short exthdr");
            return PF_DROP;
        }
        // Reassembly needs the ext header before the frag.
        if pd.fragoff == 0 {
            pd.extoff = pd.off;
        }
        if p == IPPROTO_HOPOPTS
            && pd.fragoff == 0
            && u16::from_be(h.ip6_plen) == 0
            && pd.jumbolen != 0
        {
            crate::dpfprintf!(LOG_NOTICE, "IPv6 missing jumbo");
            reason_set(reason, PFRES_IPOPTIONS);
            return PF_DROP;
        }
        if p == IPPROTO_AH {
            pd.off += (u32::from(ext[1]) + 2) * 4;
        } else {
            pd.off += (u32::from(ext[1]) + 1) * 8;
        }
        pd.proto = ext[0];
    }
    crate::dpfprintf!(LOG_NOTICE, "IPv6 nested extension header limit");
    reason_set(reason, PFRES_IPOPTIONS);
    PF_DROP
}

/// `pf_pkt_hash`: the stoeplitz hash of the connection, the state table's first key.
pub fn pf_pkt_hash(
    af: SaFamily,
    proto: u8,
    src: &PfAddr,
    dst: &PfAddr,
    sport: u16,
    dport: u16,
) -> u16 {
    let mut hash: u32 = src.addr32(0) ^ dst.addr32(0);
    #[cfg(feature = "inet6")]
    if af == AF_INET6 {
        hash ^= src.addr32(1) ^ dst.addr32(1);
        hash ^= src.addr32(2) ^ dst.addr32(2);
        hash ^= src.addr32(3) ^ dst.addr32(3);
    }
    #[cfg(not(feature = "inet6"))]
    let _ = af;

    match i32::from(proto) {
        IPPROTO_TCP | IPPROTO_UDP => hash ^= u32::from(sport ^ dport),
        _ => {}
    }

    crate::net::toeplitz::stoeplitz_n32(hash)
}

/// `pf_setup_pdesc`: fills the descriptor of the packet `m`, which starts with its IP
/// header. `kif` is `None` when pflog calls.
pub fn pf_setup_pdesc(
    pd: &mut PfPdesc,
    af: SaFamily,
    dir: u8,
    kif: Option<&'static PfiKif>,
    m: &'static Mbuf,
    reason: &mut u16,
) -> u8 {
    use crate::netinet::ip::{IP_MF, IP_OFFMASK, IPTOS_ECN_MASK, Ip};
    use crate::netinet::ip_icmp::ICMP_MINLEN;

    *pd = PfPdesc::new();
    pd.dir = dir;
    pd.kif = kif; // kif is NULL when called by pflog
    pd.m = Some(m);
    pd.sidx = if dir == PF_IN { 0 } else { 1 };
    pd.didx = if dir == PF_IN { 1 } else { 0 };
    pd.af = af;
    pd.naf = af;
    pd.rdomain = rtable_l2(m.m_pkthdr().ph_rtableid.get()) as u16;

    match pd.af {
        AF_INET => {
            // Check for illegal packets.
            if (m.m_pkthdr().len.get() as usize) < size_of::<Ip>() {
                reason_set(reason, PFRES_SHORT);
                return PF_DROP;
            }

            let h = crate::netinet::ip_var::mtod_ip(m);
            if m.m_pkthdr().len.get() < i32::from(u16::from_be(h.ip_len)) {
                reason_set(reason, PFRES_SHORT);
                return PF_DROP;
            }

            if pf_walk_header(pd, &h, reason) != PF_PASS {
                return PF_DROP;
            }

            pd.src = PfLoc::Mbuf(core::mem::offset_of!(Ip, ip_src));
            pd.dst = PfLoc::Mbuf(core::mem::offset_of!(Ip, ip_dst));
            pd.tot_len = u64::from(u16::from_be(h.ip_len));
            pd.tos = h.ip_tos & !IPTOS_ECN_MASK;
            pd.ttl = h.ip_ttl;
            pd.virtual_proto = if h.ip_off & (IP_MF | IP_OFFMASK).to_be() != 0 {
                PF_VPROTO_FRAGMENT
            } else {
                u16::from(pd.proto)
            };
        }
        #[cfg(feature = "inet6")]
        AF_INET6 => {
            // Check for illegal packets.
            if (m.m_pkthdr().len.get() as usize) < size_of::<Ip6Hdr>() {
                reason_set(reason, PFRES_SHORT);
                return PF_DROP;
            }

            let h = crate::netinet6::ip6_var::mtod_ip6(m);
            if (m.m_pkthdr().len.get() as usize)
                < size_of::<Ip6Hdr>() + usize::from(u16::from_be(h.ip6_plen))
            {
                reason_set(reason, PFRES_SHORT);
                return PF_DROP;
            }

            if pf_walk_header6(pd, &h, reason) != PF_PASS {
                return PF_DROP;
            }

            // We do not support jumbogram yet. If we keep going, zero ip6_plen will do
            // something bad, so drop the packet for now.
            if pd.jumbolen != 0 {
                reason_set(reason, PFRES_NORM);
                return PF_DROP;
            }

            pd.src = PfLoc::Mbuf(core::mem::offset_of!(Ip6Hdr, ip6_src));
            pd.dst = PfLoc::Mbuf(core::mem::offset_of!(Ip6Hdr, ip6_dst));
            pd.tot_len = u64::from(u16::from_be(h.ip6_plen)) + size_of::<Ip6Hdr>() as u64;
            pd.tos = ((u32::from_be(h.ip6_flow) & 0x0fc0_0000) >> 20) as u8;
            pd.ttl = h.ip6_hlim;
            pd.virtual_proto = if pd.fragoff != 0 {
                PF_VPROTO_FRAGMENT
            } else {
                u16::from(pd.proto)
            };
        }
        _ => panic(format_args!(
            "pf_setup_pdesc called with illegal af {}",
            pd.af
        )),
    }

    pd.nsaddr = pd.ld_addr(pd.src);
    pd.ndaddr = pd.ld_addr(pd.dst);

    match i32::from(pd.virtual_proto) {
        IPPROTO_TCP => {
            let mut b = [0u8; size_of::<Tcphdr>()];
            if !pf_pull_hdr(m, pd.off as i32, &mut b, Some(reason), pd.af) {
                return PF_DROP;
            }
            pd.hdr_bytes()[..b.len()].copy_from_slice(&b);
            pd.hdrlen = size_of::<Tcphdr>() as u32;
            let th = *pd.tcp();
            let thoff = u32::from(th.th_off()) << 2;
            if th.th_dport == 0
                || u64::from(pd.off + thoff) > pd.tot_len
                || (thoff as usize) < size_of::<Tcphdr>()
            {
                reason_set(reason, PFRES_SHORT);
                return PF_DROP;
            }
            pd.p_len = (pd.tot_len - u64::from(pd.off) - u64::from(thoff)) as u32;
            pd.sport = PfLoc::Hdr(TH_SPORT_OFF);
            pd.dport = PfLoc::Hdr(TH_DPORT_OFF);
            pd.pcksum = PfLoc::Hdr(core::mem::offset_of!(Tcphdr, th_sum));
        }
        IPPROTO_UDP => {
            let mut b = [0u8; size_of::<Udphdr>()];
            if !pf_pull_hdr(m, pd.off as i32, &mut b, Some(reason), pd.af) {
                return PF_DROP;
            }
            pd.hdr_bytes()[..b.len()].copy_from_slice(&b);
            pd.hdrlen = size_of::<Udphdr>() as u32;
            let uh = *pd.udp();
            let ulen = u64::from(u16::from_be(uh.uh_ulen));
            if uh.uh_dport == 0
                || u64::from(pd.off) + ulen > pd.tot_len
                || (ulen as usize) < size_of::<Udphdr>()
            {
                reason_set(reason, PFRES_SHORT);
                return PF_DROP;
            }
            pd.sport = PfLoc::Hdr(TH_SPORT_OFF);
            pd.dport = PfLoc::Hdr(TH_DPORT_OFF);
            pd.pcksum = PfLoc::Hdr(UH_SUM_OFF);
        }
        IPPROTO_ICMP => {
            let mut b = [0u8; ICMP_MINLEN];
            if !pf_pull_hdr(m, pd.off as i32, &mut b, Some(reason), pd.af) {
                return PF_DROP;
            }
            pd.hdr_bytes()[..ICMP_MINLEN].copy_from_slice(&b);
            pd.hdrlen = ICMP_MINLEN as u32;
            if u64::from(pd.off + pd.hdrlen) > pd.tot_len {
                reason_set(reason, PFRES_SHORT);
                return PF_DROP;
            }
            pd.pcksum = PfLoc::Hdr(crate::netinet::ip_icmp::ICMP_CKSUM_OFFSET);
        }
        #[cfg(feature = "inet6")]
        IPPROTO_ICMPV6 => {
            use crate::netinet::icmp6::{
                Icmp6Hdr, MLD_LISTENER_QUERY, MLD_LISTENER_REPORT, MldHdr, ND_NEIGHBOR_ADVERT,
                ND_NEIGHBOR_SOLICIT, ND_REDIRECT, ND_ROUTER_ADVERT, ND_ROUTER_SOLICIT,
                NdNeighborSolicit,
            };

            let mut icmp_hlen = size_of::<Icmp6Hdr>();
            let mut b = [0u8; size_of::<NdNeighborSolicit>()];
            if !pf_pull_hdr(m, pd.off as i32, &mut b[..icmp_hlen], Some(reason), pd.af) {
                return PF_DROP;
            }
            pd.hdr_bytes()[..icmp_hlen].copy_from_slice(&b[..icmp_hlen]);
            // ICMP headers we look further into to match state.
            match pd.icmp6().icmp6_type {
                MLD_LISTENER_QUERY | MLD_LISTENER_REPORT => icmp_hlen = size_of::<MldHdr>(),
                t @ (ND_NEIGHBOR_SOLICIT | ND_NEIGHBOR_ADVERT | ND_ROUTER_SOLICIT
                | ND_ROUTER_ADVERT | ND_REDIRECT) => {
                    if t == ND_NEIGHBOR_SOLICIT || t == ND_NEIGHBOR_ADVERT {
                        icmp_hlen = size_of::<NdNeighborSolicit>();
                    }
                    if pd.ttl != 255 {
                        reason_set(reason, PFRES_NORM);
                        return PF_DROP;
                    }
                }
                _ => {}
            }
            if icmp_hlen > size_of::<Icmp6Hdr>() {
                if !pf_pull_hdr(m, pd.off as i32, &mut b[..icmp_hlen], Some(reason), pd.af) {
                    return PF_DROP;
                }
                pd.hdr_bytes()[..icmp_hlen].copy_from_slice(&b[..icmp_hlen]);
            }
            pd.hdrlen = icmp_hlen as u32;
            if u64::from(pd.off + pd.hdrlen) > pd.tot_len {
                reason_set(reason, PFRES_SHORT);
                return PF_DROP;
            }
            pd.pcksum = PfLoc::Hdr(core::mem::offset_of!(Icmp6Hdr, icmp6_cksum));
        }
        _ => {}
    }

    if !pd.sport.is_none() {
        let sp = pd.ld16(pd.sport);
        pd.osport = sp;
        pd.nsport = sp;
    }
    if !pd.dport.is_none() {
        let dp = pd.ld16(pd.dport);
        pd.odport = dp;
        pd.ndport = dp;
    }

    let (s, d) = (pd.ld_addr(pd.src), pd.ld_addr(pd.dst));
    pd.hash = pf_pkt_hash(pd.af, pd.proto, &s, &d, pd.osport, pd.odport);

    PF_PASS
}

/// `pf_counters_inc`: the interface, rule, state, source node and table counters.
pub fn pf_counters_inc(
    action: u8,
    pd: &mut PfPdesc,
    st: Option<&'static PfState>,
    r: &'static PfRule,
    a: Option<&'static PfRule>,
) {
    let add = |c: &Cell<u64>, v: u64| c.set(c.get().wrapping_add(v));
    let tot = pd.tot_len;
    if let Some(kif) = pd.kif {
        let i6 = usize::from(pd.af == AF_INET6);
        let o = usize::from(pd.dir == PF_OUT);
        let d = usize::from(action != PF_PASS);
        add(&kif.pfik_bytes[i6][o][d], tot);
        add(&kif.pfik_packets[i6][o][d], 1);
    }

    if action == PF_PASS || action == PF_AFRT || r.action == PF_DROP {
        let mut dirndx = usize::from(pd.dir == PF_OUT);
        add(&r.packets[dirndx], 1);
        add(&r.bytes[dirndx], tot);
        if let Some(a) = a {
            add(&a.packets[dirndx], 1);
            add(&a.bytes[dirndx], tot);
        }
        let key_addr = |st: &'static PfState, which: bool| -> Option<PfAddr> {
            let k = st.key[usize::from(st.direction.get() == PF_IN)].get()?;
            Some(k.addr[usize::from(which)].get())
        };
        if let Some(st) = st {
            for sni in st.src_nodes.iter() {
                add(&sni.sn().packets[dirndx], 1);
                add(&sni.sn().bytes[dirndx], tot);
            }
            dirndx = if pd.dir == st.direction.get() { 0 } else { 1 };
            add(&st.packets[dirndx], 1);
            add(&st.bytes[dirndx], tot);

            for ri in st.match_rules.iter() {
                let rr = ri.r();
                add(&rr.packets[dirndx], 1);
                add(&rr.bytes[dirndx], tot);

                if rr.src.addr.type_.get() == PF_ADDR_TABLE
                    && let (Some(t), Some(x)) = (
                        rr.src.addr.tbl(),
                        key_addr(st, st.direction.get() == PF_OUT),
                    )
                {
                    crate::net::pf_table::pfr_update_stats(
                        t,
                        &x,
                        pd,
                        rr.action,
                        rr.src.neg.get() != 0,
                    );
                }
                if rr.dst.addr.type_.get() == PF_ADDR_TABLE
                    && let (Some(t), Some(x)) =
                        (rr.dst.addr.tbl(), key_addr(st, st.direction.get() == PF_IN))
                {
                    crate::net::pf_table::pfr_update_stats(
                        t,
                        &x,
                        pd,
                        rr.action,
                        rr.dst.neg.get() != 0,
                    );
                }
            }
        }
        if r.src.addr.type_.get() == PF_ADDR_TABLE
            && let Some(t) = r.src.addr.tbl()
        {
            let x = match st {
                None => Some(pd.ld_addr(pd.src)),
                Some(st) => key_addr(st, st.direction.get() == PF_OUT),
            };
            if let Some(x) = x {
                crate::net::pf_table::pfr_update_stats(t, &x, pd, r.action, r.src.neg.get() != 0);
            }
        }
        if r.dst.addr.type_.get() == PF_ADDR_TABLE
            && let Some(t) = r.dst.addr.tbl()
        {
            let x = match st {
                None => Some(pd.ld_addr(pd.dst)),
                Some(st) => key_addr(st, st.direction.get() == PF_IN),
            };
            if let Some(x) = x {
                crate::net::pf_table::pfr_update_stats(t, &x, pd, r.action, r.dst.neg.get() != 0);
            }
        }
    }
}

/// `pf_test`: the packet filter's hook for the IP stack. `fwdir` is `PF_IN`, `PF_OUT` or
/// `PF_FWD`; `*m0` may be replaced (reassembly, route-to) or consumed (`None`). Returns the
/// action (`PF_PASS`, `PF_DROP`, `PF_DIVERT`, ...).
#[cfg_attr(not(feature = "inet6"), allow(unused_labels))] // 'out is the C's INET6 label
pub fn pf_test(af: SaFamily, fwdir: u8, ifp: &'static Ifnet, m0: &mut Option<&'static Mbuf>) -> u8 {
    use crate::netinet::in_::{IN_CLASSA_NSHIFT, IN_LOOPBACKNET};
    use crate::netinet::ip::IPTOS_LOWDELAY;

    let mut reason: u16 = 0;
    let mut a: Option<&'static PfRule> = None;
    let def: &'static PfRule = &crate::net::pf_ioctl::PF_DEFAULT_RULE;
    let mut r: Option<&'static PfRule> = Some(def);
    let mut st: Option<&'static PfState> = None;
    let mut ruleset: Option<&'static PfRuleset> = None;
    let mut pd = PfPdesc::new();
    let dir = if fwdir == PF_FWD { PF_OUT } else { fwdir };
    let mut qid: u32 = 0;
    let mut pqid = false;
    let mut have_pf_lock = false;
    let mut action: u8;

    if PF_STATUS.running.get() == 0 {
        return PF_PASS;
    }

    // NCARP > 0: a carp interface uses its parent's kif; not configured.
    // SAFETY: `if_pf_kif` is null or the kif `pfi_attach_ifnet` set, which lives until
    // `pfi_detach_ifnet`.
    let kif = unsafe { ifp.if_pf_kif.get().cast::<PfiKif>().cast_const().as_ref() };

    let Some(kif) = kif else {
        crate::dpfprintf!(
            LOG_ERR,
            "pf_test: kif == NULL, if_xname {}",
            crate::kern::subr_prf::Str(&ifp.if_xname.get())
        );
        return PF_DROP;
    };
    if kif.pfik_flags.get() & PFI_IFLAG_SKIP != 0 {
        return PF_PASS;
    }

    let Some(m) = *m0 else {
        return PF_PASS;
    };
    #[cfg(feature = "diagnostic")]
    if m.m_flags().get() & crate::sys::mbuf::M_PKTHDR == 0 {
        panic(format_args!("non-M_PKTHDR is passed to pf_test"));
    }

    let pf = &m.m_pkthdr().pf;
    if pf.flags.get() & PF_TAG_GENERATED != 0 {
        return PF_PASS;
    }

    if pf.flags.get() & PF_TAG_DIVERTED_PACKET != 0 {
        pf.flags.set(pf.flags.get() & !PF_TAG_DIVERTED_PACKET);
        return PF_PASS;
    }

    if pf.flags.get() & PF_TAG_REFRAGMENTED != 0 {
        pf.flags.set(pf.flags.get() & !PF_TAG_REFRAGMENTED);
        return PF_PASS;
    }

    'done: {
        action = pf_setup_pdesc(&mut pd, af, dir, Some(kif), m, &mut reason);
        if action != PF_PASS {
            pd.pflog |= PF_LOG_FORCE;
            break 'done;
        }

        // Packet normalization and reassembly.
        match pd.af {
            AF_INET => action = crate::net::pf_norm::pf_normalize_ip(&mut pd, &mut reason),
            #[cfg(feature = "inet6")]
            AF_INET6 => action = crate::net::pf_norm::pf_normalize_ip6(&mut pd, &mut reason),
            _ => {}
        }
        *m0 = pd.m;
        // If packet sits in reassembly queue, return without error.
        let Some(pm) = pd.m else {
            return PF_PASS;
        };

        if action != PF_PASS {
            pd.pflog |= PF_LOG_FORCE;
            break 'done;
        }

        // If packet has been reassembled, update packet description.
        if PF_STATUS.reass.get() != 0 && pd.virtual_proto == PF_VPROTO_FRAGMENT {
            action = pf_setup_pdesc(&mut pd, af, dir, Some(kif), pm, &mut reason);
            if action != PF_PASS {
                pd.pflog |= PF_LOG_FORCE;
                break 'done;
            }
        }
        let Some(pm) = pd.m else {
            return PF_PASS;
        };
        let f = &pm.m_pkthdr().pf.flags;
        f.set(f.get() | PF_TAG_PROCESSED);

        // Avoid pcb-lookups from the forwarding path. They should never match and would
        // cause MP locking problems.
        if fwdir == PF_FWD {
            pd.lookup.done = -1;
            pd.lookup.uid = Uid::MAX;
            pd.lookup.gid = Gid::MAX;
            pd.lookup.pid = crate::sys::proc::NO_PID;
        }

        match pd.virtual_proto {
            PF_VPROTO_FRAGMENT => {
                // Handle fragments that aren't reassembled by normalization.
                pf_lock();
                have_pf_lock = true;
                action = pf_test_rule(&mut pd, &mut r, &mut st, &mut a, &mut ruleset, &mut reason);
                st = st.map(pf_state_ref);
                if action != PF_PASS {
                    reason_set(&mut reason, PFRES_FRAG);
                }
            }
            vp if i32::from(vp) == IPPROTO_ICMP => {
                if pd.af != AF_INET {
                    action = PF_DROP;
                    reason_set(&mut reason, PFRES_NORM);
                    crate::dpfprintf!(LOG_NOTICE, "dropping IPv6 packet with ICMPv4 payload");
                } else {
                    pf_state_enter_read();
                    action = pf_test_state_icmp(&mut pd, &mut st, &mut reason);
                    st = st.map(pf_state_ref);
                    pf_state_exit_read();
                    if action == PF_PASS || action == PF_AFRT {
                        if let Some(s) = st {
                            crate::net::if_pfsync::pfsync_update_state(s);
                            r = s.rule.ptr();
                            a = s.anchor.ptr();
                            pd.pflog |= s.log.get();
                        }
                    } else if st.is_none() {
                        pf_lock();
                        have_pf_lock = true;
                        action = pf_test_rule(
                            &mut pd,
                            &mut r,
                            &mut st,
                            &mut a,
                            &mut ruleset,
                            &mut reason,
                        );
                        st = st.map(pf_state_ref);
                    }
                }
            }
            #[cfg(feature = "inet6")]
            vp if i32::from(vp) == IPPROTO_ICMPV6 => {
                if pd.af != AF_INET6 {
                    action = PF_DROP;
                    reason_set(&mut reason, PFRES_NORM);
                    crate::dpfprintf!(LOG_NOTICE, "dropping IPv4 packet with ICMPv6 payload");
                } else {
                    pf_state_enter_read();
                    action = pf_test_state_icmp(&mut pd, &mut st, &mut reason);
                    st = st.map(pf_state_ref);
                    pf_state_exit_read();
                    if action == PF_PASS || action == PF_AFRT {
                        if let Some(s) = st {
                            crate::net::if_pfsync::pfsync_update_state(s);
                            r = s.rule.ptr();
                            a = s.anchor.ptr();
                            pd.pflog |= s.log.get();
                        }
                    } else if st.is_none() {
                        pf_lock();
                        have_pf_lock = true;
                        action = pf_test_rule(
                            &mut pd,
                            &mut r,
                            &mut st,
                            &mut a,
                            &mut ruleset,
                            &mut reason,
                        );
                        st = st.map(pf_state_ref);
                    }
                }
            }
            vp => {
                let mut fallthrough = true;
                if i32::from(vp) == IPPROTO_TCP {
                    let th_flags = pd.tcp().th_flags;
                    if pd.dir == PF_IN
                        && (th_flags & (TH_SYN | TH_ACK)) == TH_SYN
                        && crate::net::pf_syncookies::pf_synflood_check(&mut pd)
                    {
                        pf_lock();
                        have_pf_lock = true;
                        crate::net::pf_syncookies::pf_syncookie_send(&mut pd, &mut reason);
                        action = PF_DROP;
                        fallthrough = false;
                    } else {
                        if th_flags & TH_ACK != 0 && pd.p_len == 0 {
                            pqid = true;
                        }
                        action = crate::net::pf_norm::pf_normalize_tcp(&mut pd);
                        if action == PF_DROP {
                            fallthrough = false;
                        }
                    }
                }
                if fallthrough {
                    let mut key = PfStateKeyCmp {
                        af: pd.af,
                        proto: pd.virtual_proto as u8,
                        rdomain: pd.rdomain,
                        hash: pd.hash,
                        ..PfStateKeyCmp::default()
                    };
                    let (s, d) = (pd.ld_addr(pd.src), pd.ld_addr(pd.dst));
                    pf_addrcpy(&mut key.addr[usize::from(pd.sidx)], &s, key.af);
                    pf_addrcpy(&mut key.addr[usize::from(pd.didx)], &d, key.af);
                    key.port[usize::from(pd.sidx)] = pd.osport;
                    key.port[usize::from(pd.didx)] = pd.odport;

                    pf_state_enter_read();
                    action = pf_find_state(&mut pd, &key, &mut st);
                    st = st.map(pf_state_ref);
                    pf_state_exit_read();

                    // Check for syncookies if tcp ack and no active state.
                    let th_flags = pd.tcp().th_flags;
                    if pd.dir == PF_IN
                        && i32::from(pd.virtual_proto) == IPPROTO_TCP
                        && st.is_none_or(|s| {
                            i32::from(s.src.state.get()) >= TCPS_FIN_WAIT_2
                                && i32::from(s.dst.state.get()) >= TCPS_FIN_WAIT_2
                        })
                        && (th_flags & (TH_SYN | TH_ACK | TH_RST)) == TH_ACK
                        && crate::net::pf_syncookies::pf_syncookie_validate(&mut pd) != 0
                    {
                        let msyn = crate::net::pf_syncookies::pf_syncookie_recreate_syn(
                            &mut pd,
                            &mut reason,
                        );
                        if let Some(ms) = msyn {
                            let mut msyn = Some(ms);
                            action = pf_test(af, fwdir, ifp, &mut msyn);
                            crate::kern::uipc_mbuf::m_freem(msyn);
                            if action == PF_PASS || action == PF_AFRT {
                                pf_state_enter_read();
                                pf_state_unref(st);
                                st = None;
                                action = pf_find_state(&mut pd, &key, &mut st);
                                st = st.map(pf_state_ref);
                                pf_state_exit_read();
                                let Some(s) = st else {
                                    return PF_DROP;
                                };
                                let th = *pd.tcp();
                                let hi = u32::from_be(th.th_ack).wrapping_sub(1);
                                s.src.seqhi.set(hi);
                                s.dst.seqhi.set(hi);
                                s.src.seqlo.set(u32::from_be(th.th_seq).wrapping_sub(1));
                                pf_set_protostate(s, PF_PEER_SRC, PF_TCPS_PROXY_DST);
                            }
                        } else {
                            action = PF_DROP;
                        }
                    }

                    if action == PF_MATCH {
                        action = pf_test_state(&mut pd, &mut st, &mut reason);
                    }

                    if action == PF_PASS || action == PF_AFRT {
                        if let Some(s) = st {
                            crate::net::if_pfsync::pfsync_update_state(s);
                            r = s.rule.ptr();
                            a = s.anchor.ptr();
                            pd.pflog |= s.log.get();
                        }
                    } else if st.is_none() {
                        pf_lock();
                        have_pf_lock = true;
                        action = pf_test_rule(
                            &mut pd,
                            &mut r,
                            &mut st,
                            &mut a,
                            &mut ruleset,
                            &mut reason,
                        );
                        st = st.map(pf_state_ref);
                    }

                    if i32::from(pd.virtual_proto) == IPPROTO_TCP {
                        if let Some(s) = st {
                            if s.max_mss.get() != 0 {
                                crate::net::pf_norm::pf_normalize_mss(&mut pd, s.max_mss.get());
                            }
                        } else if let Some(rr) = r
                            && rr.max_mss != 0
                        {
                            crate::net::pf_norm::pf_normalize_mss(&mut pd, rr.max_mss);
                        }
                    }
                }
            }
        }

        if have_pf_lock {
            pf_unlock();
        }

        // At the moment, we rely on NET_LOCK() to prevent removal of items we've collected
        // above ('r', 'anchor' and 'ruleset'). They'll have to be refcounted when NET_LOCK()
        // is gone.
    }

    // done:
    let r: &'static PfRule = r.unwrap_or(def);
    if action != PF_DROP
        && let Some(pm) = pd.m
    {
        let pf = &pm.m_pkthdr().pf;
        if let Some(s) = st {
            // The non-state case is handled in pf_test_rule().
            if action == PF_PASS && pd.badopts != 0 && s.state_flags.get() & PFSTATE_ALLOWOPTS == 0
            {
                action = PF_DROP;
                reason_set(&mut reason, PFRES_IPOPTIONS);
                pd.pflog |= PF_LOG_FORCE;
                crate::dpfprintf!(
                    LOG_NOTICE,
                    "dropping packet with ip/ipv6 options in pf_test()"
                );
            }

            crate::net::pf_norm::pf_scrub(
                pm,
                s.state_flags.get(),
                pd.af,
                s.min_ttl.get(),
                s.set_tos.get(),
            );
            pf_tag_packet(
                pm,
                i32::from(s.tag.get()),
                s.rtableid[usize::from(pd.didx)].get(),
            );
            if pqid || pd.tos & IPTOS_LOWDELAY != 0 {
                qid = u32::from(s.pqid.get());
                if s.state_flags.get() & PFSTATE_SETPRIO != 0 {
                    pf.prio.set(s.set_prio[1].get());
                }
            } else {
                qid = u32::from(s.qid.get());
                if s.state_flags.get() & PFSTATE_SETPRIO != 0 {
                    pf.prio.set(s.set_prio[0].get());
                }
            }
            pf.delay.set(s.delay.get());
        } else {
            crate::net::pf_norm::pf_scrub(pm, r.scrub_flags, pd.af, r.min_ttl, r.set_tos);
            if pqid || pd.tos & IPTOS_LOWDELAY != 0 {
                qid = r.pqid;
                if r.scrub_flags & PFSTATE_SETPRIO != 0 {
                    pf.prio.set(r.set_prio[1]);
                }
            } else {
                qid = r.qid;
                if r.scrub_flags & PFSTATE_SETPRIO != 0 {
                    pf.prio.set(r.set_prio[0]);
                }
            }
            pf.delay.set(r.delay);
        }
    }

    if action == PF_PASS
        && qid != 0
        && let Some(pm) = pd.m
    {
        pm.m_pkthdr().pf.qid.set(qid);
    }
    if let Some(s) = st
        && let Some(pm) = pd.m
    {
        let inp = pf_mbuf_inp(pm);

        if pd.dir == PF_IN {
            kassert!(inp.is_none());
            if let Some(k) = s.key[PF_SK_STACK].get() {
                pf_mbuf_link_state_key(pm, k);
            }
        } else if pd.dir == PF_OUT
            && let Some(k) = s.key[PF_SK_STACK].get()
        {
            pf_state_key_link_inpcb(k, inp);
        }

        let cf = &pm.m_pkthdr().csum_flags;
        if cf.get() & M_FLOWID == 0
            && let Some(k) = s.key[PF_SK_WIRE].get()
        {
            pm.m_pkthdr().ph_flowid.set(k.hash.get());
            cf.set(cf.get() | M_FLOWID);
        }
    }

    // Connections redirected to loopback should not match sockets bound specifically to
    // loopback due to security implications, see in_pcblookup_listen().
    if pd.destchg != 0
        && let Some(pm) = pd.m
    {
        let d = pd.ld_addr(pd.dst);
        if pd.af == AF_INET && (u32::from_be(d.v4().s_addr) >> IN_CLASSA_NSHIFT) == IN_LOOPBACKNET {
            let f = &pm.m_pkthdr().pf.flags;
            f.set(f.get() | PF_TAG_TRANSLATE_LOCALHOST);
        }
        if pd.af == AF_INET6 && crate::netinet6::in6::in6_is_addr_loopback(&d.v6()) {
            let f = &pm.m_pkthdr().pf.flags;
            f.set(f.get() | PF_TAG_TRANSLATE_LOCALHOST);
        }
    }
    // We need to redo the route lookup on outgoing routes.
    if pd.destchg != 0
        && pd.dir == PF_OUT
        && let Some(pm) = pd.m
    {
        let f = &pm.m_pkthdr().pf.flags;
        f.set(f.get() | PF_TAG_REROUTE);
    }

    if pd.dir == PF_IN
        && action == PF_PASS
        && (r.divert.type_ == PF_DIVERT_TO || r.divert.type_ == PF_DIVERT_REPLY)
        && let Some(pm) = pd.m
        && let Some(t) = pf_get_divert(pm)
    {
        let f = &pm.m_pkthdr().pf.flags;
        f.set(f.get() | PF_TAG_DIVERTED);
        pf_set_divert(
            t,
            &PfDivert {
                addr: r.divert.addr,
                port: r.divert.port,
                rdomain: pd.rdomain,
                type_: r.divert.type_,
            },
        );
    }

    if action == PF_PASS && r.divert.type_ == PF_DIVERT_PACKET {
        action = PF_DIVERT;
    }

    if pd.pflog != 0 {
        if pd.pflog & PF_LOG_FORCE != 0 || r.log & PF_LOG_ALL != 0 {
            crate::net::if_pflog::pflog_packet(&mut pd, reason as u8, r, a, ruleset, None);
        }
        if let Some(s) = st {
            for ri in s.match_rules.iter() {
                if ri.r().log & PF_LOG_ALL != 0 {
                    crate::net::if_pflog::pflog_packet(
                        &mut pd,
                        reason as u8,
                        ri.r(),
                        a,
                        ruleset,
                        None,
                    );
                }
            }
        }
    }

    pf_counters_inc(action, &mut pd, st, r, a);

    'out: {
        match action {
            PF_SYNPROXY_DROP | PF_DEFER => {
                if action == PF_SYNPROXY_DROP {
                    crate::kern::uipc_mbuf::m_freem(pd.m);
                }
                pd.m = None;
                action = PF_PASS;
            }
            PF_DIVERT => {
                match pd.af {
                    AF_INET => {
                        if let Some(m) = pd.m {
                            crate::netinet::ip_divert::divert_packet(m, pd.dir, r.divert.port);
                        }
                        pd.m = None;
                    }
                    #[cfg(feature = "inet6")]
                    AF_INET6 => {
                        if let Some(m) = pd.m {
                            crate::netinet6::ip6_divert::divert6_packet(m, pd.dir, r.divert.port);
                        }
                        pd.m = None;
                    }
                    _ => {}
                }
                action = PF_PASS;
            }
            #[cfg(feature = "inet6")]
            PF_AFRT => {
                if pf_translate_af(&mut pd) != 0 {
                    action = PF_DROP;
                    break 'out;
                }
                let Some(m) = pd.m else {
                    action = PF_DROP;
                    break 'out;
                };
                let f = &m.m_pkthdr().pf.flags;
                f.set(f.get() | PF_TAG_GENERATED);
                match pd.naf {
                    AF_INET => {
                        if pd.dir == PF_IN {
                            use crate::netinet::ip_var::{
                                IP_ALLOWBROADCAST, IP_FORWARDING, IP_FORWARDING_IPSEC, IP_REDIRECT,
                                IpstatCounters, ipstat_inc,
                            };
                            let mut flags = IP_REDIRECT;
                            match crate::netinet::ip_input::ip_forwarding
                                .load(AtomicOrdering::Relaxed)
                            {
                                2 => flags |= IP_FORWARDING_IPSEC | IP_FORWARDING,
                                1 => flags |= IP_FORWARDING,
                                _ => {
                                    ipstat_inc(IpstatCounters::IpsCantforward);
                                    action = PF_DROP;
                                    break 'out;
                                }
                            }
                            if crate::netinet::ip_input::IP_DIRECTEDBCAST
                                .load(AtomicOrdering::Relaxed)
                                != 0
                            {
                                flags |= IP_ALLOWBROADCAST;
                            }
                            crate::netinet::ip_input::ip_forward(m, ifp, None, flags);
                        } else {
                            let _ = crate::netinet::ip_output::ip_output(
                                m, None, None, 0, None, None, 0,
                            );
                        }
                    }
                    AF_INET6 => {
                        if pd.dir == PF_IN {
                            use crate::netinet6::ip6_var::{
                                IPV6_FORWARDING, IPV6_FORWARDING_IPSEC, IPV6_REDIRECT,
                                Ip6statCounters, ip6stat_inc,
                            };
                            let mut flags = IPV6_REDIRECT;
                            match crate::netinet6::in6_proto::IP6_FORWARDING
                                .load(AtomicOrdering::Relaxed)
                            {
                                2 => flags |= IPV6_FORWARDING_IPSEC | IPV6_FORWARDING,
                                1 => flags |= IPV6_FORWARDING,
                                _ => {
                                    ip6stat_inc(Ip6statCounters::Ip6sCantforward);
                                    action = PF_DROP;
                                    break 'out;
                                }
                            }
                            crate::netinet6::ip6_forward::ip6_forward(m, None, flags);
                        } else {
                            let _ = crate::netinet6::ip6_output::ip6_output(
                                m, None, None, 0, None, None,
                            );
                        }
                    }
                    _ => {}
                }
                pd.m = None;
                action = PF_PASS;
            }
            PF_DROP => {
                crate::kern::uipc_mbuf::m_freem(pd.m);
                pd.m = None;
            }
            _ => {
                if let Some(s) = st
                    && s.rt.get() != 0
                {
                    match pd.af {
                        AF_INET => pf_route(&mut pd, s),
                        #[cfg(feature = "inet6")]
                        AF_INET6 => pf_route6(&mut pd, s),
                        _ => {}
                    }
                }
            }
        }

        // If reassembled packet passed, create new fragments.
        #[cfg(feature = "inet6")]
        if PF_STATUS.reass.get() != 0
            && action == PF_PASS
            && fwdir == PF_FWD
            && pd.af == AF_INET6
            && let Some(m) = pd.m
            && let Some(mtag) = crate::kern::uipc_mbuf2::m_tag_find(
                m,
                crate::sys::mbuf::PACKET_TAG_PF_REASSEMBLED,
                None,
            )
        {
            action = crate::net::pf_norm::pf_refragment6(&mut pd.m, mtag, None, None, None);
        }
        if let Some(s) = st
            && action != PF_DROP
        {
            if s.if_index_in.get() == 0 && dir == PF_IN {
                s.if_index_in.set(ifp.if_index.get() as u16);
            } else if s.if_index_out.get() == 0 && dir == PF_OUT {
                s.if_index_out.set(ifp.if_index.get() as u16);
            }
        }
    }
    // out:
    *m0 = pd.m;

    pf_state_unref(st);

    action
}

/// `pf_ouraddr`: 1 when a state or a divert rule delivers the packet locally, -1 when pf
/// does not know.
pub fn pf_ouraddr(m: &Mbuf) -> i32 {
    if m.m_pkthdr().pf.flags.get() & PF_TAG_DIVERTED != 0 {
        return 1;
    }

    if let Some(sk) = pf_mbuf_statekey(m)
        && sk.sk_inp.get().is_some()
    {
        return 1;
    }

    -1
}

/// `pf_pkt_addr_changed`: must be called whenever any addressing information such as
/// address, port, protocol has changed.
pub fn pf_pkt_addr_changed(m: &Mbuf) {
    pf_mbuf_unlink_state_key(m);
    pf_mbuf_unlink_inpcb(m);
}

/// `pf_inp_lookup`: the socket the packet's state key is linked to, referenced.
pub fn pf_inp_lookup(m: &Mbuf) -> Option<&'static Inpcb> {
    let sk = pf_mbuf_statekey(m);
    let mut inp = None;

    if !pf_state_key_isvalid(sk) {
        pf_mbuf_unlink_state_key(m);
    } else if let Some(sk) = sk
        && sk.sk_inp.get().is_some()
    {
        mtx_enter(&PF_INP_MTX);
        inp = crate::netinet::in_pcb::in_pcbref(sk.sk_inp.get());
        mtx_leave(&PF_INP_MTX);
    }

    inp
}

/// `pf_inp_link`: links the packet's state key to the socket that received it.
pub fn pf_inp_link(m: &Mbuf, inp: Option<&'static Inpcb>) {
    let sk = pf_mbuf_statekey(m);

    let Some(sk) = sk.filter(|s| pf_state_key_isvalid(Some(s))) else {
        pf_mbuf_unlink_state_key(m);
        return;
    };

    // We don't need to grab PF-lock here. At worst case we link inp to state, which might
    // be just being marked as deleted by another thread.
    pf_state_key_link_inpcb(sk, inp);

    // The statekey has finished finding the inp, it is no longer needed.
    pf_mbuf_unlink_state_key(m);
}

/// `pf_inp_unlink`: the socket is going away.
pub fn pf_inp_unlink(inp: &'static Inpcb) {
    if inpcb_pf_sk(inp).is_none() {
        return;
    }

    mtx_enter(&PF_INP_MTX);
    let Some(sk) = inpcb_pf_sk(inp) else {
        mtx_leave(&PF_INP_MTX);
        return;
    };
    kassert!(opt_eq(sk.sk_inp.get(), Some(inp)));
    sk.sk_inp.set(None);
    inpcb_set_pf_sk(inp, None);
    mtx_leave(&PF_INP_MTX);

    pf_state_key_unref(Some(sk));
    crate::netinet::in_pcb::in_pcbunref(Some(inp));
}

/// `pf_state_key_link_reverse`: links the two keys of a forwarded connection to each other.
pub fn pf_state_key_link_reverse(sk: &'static PfStateKey, skrev: &'static PfStateKey) {
    let old_reverse = sk.sk_reverse.cas_null(skrev);
    if let Some(old) = old_reverse {
        kassert!(ptr::eq(old, skrev));
    } else {
        pf_state_key_ref(skrev);

        // NOTE: if sk == skrev, then the KASSERT below holds true; we still want to grab a
        // reference in such case, because pf_state_key_unlink_reverse() does not check
        // whether keys are identical or not.
        if let Some(old) = skrev.sk_reverse.cas_null(sk) {
            kassert!(ptr::eq(old, sk));
        }

        pf_state_key_ref(sk);
    }
}

/// `pf_log_matches`: logs the packet for every `log (matches)` match rule.
pub fn pf_log_matches(
    pd: &mut PfPdesc,
    rm: &'static PfRule,
    am: Option<&'static PfRule>,
    ruleset: Option<&'static PfRuleset>,
    matchrules: &SlistHead<PfRuleSlist>,
) {
    // If this is the log(matches) rule, packet has been logged already.
    if rm.log & PF_LOG_MATCHES != 0 {
        return;
    }

    for ri in matchrules.iter() {
        if ri.r().log & PF_LOG_MATCHES != 0 {
            crate::net::if_pflog::pflog_packet(
                pd,
                PFRES_MATCH as u8,
                rm,
                am,
                ruleset,
                Some(ri.r()),
            );
        }
    }
}

/// `pf_state_key_ref`.
pub fn pf_state_key_ref(sk: &'static PfStateKey) -> &'static PfStateKey {
    refcnt_take(&sk.sk_refcnt);
    sk
}

/// `pf_state_key_unref`: frees the key with its last reference.
pub fn pf_state_key_unref(sk: Option<&'static PfStateKey>) {
    let Some(sk) = sk else {
        return;
    };
    if refcnt_rele(&sk.sk_refcnt) {
        // State key must be removed from tree.
        kassert!(!pf_state_key_isvalid(Some(sk)));
        // State key must be unlinked from reverse key.
        kassert!(sk.sk_reverse.get().is_none());
        // State key must be unlinked from socket.
        kassert!(sk.sk_inp.get().is_none());
        pf_pool_put(&PF_STATE_KEY_PL, sk);
    }
}

/// `pf_state_key_isvalid`.
pub fn pf_state_key_isvalid(sk: Option<&PfStateKey>) -> bool {
    sk.is_some_and(|s| s.sk_removed.get() == 0)
}

/// The packet's `m_pkthdr.pf.statekey`.
pub fn pf_mbuf_statekey(m: &Mbuf) -> Option<&'static PfStateKey> {
    // SAFETY: only `pf_mbuf_link_state_key` stores into it, a referenced key that stays
    // allocated until `pf_mbuf_unlink_state_key` drops the reference (uipc_mbuf.c copies
    // and unlinks it with the packet header).
    unsafe {
        m.m_pkthdr()
            .pf
            .statekey
            .get()
            .cast::<PfStateKey>()
            .cast_const()
            .as_ref()
    }
}

/// The packet's `m_pkthdr.pf.inp`.
pub fn pf_mbuf_inp(m: &Mbuf) -> Option<&'static Inpcb> {
    // SAFETY: only `pf_mbuf_link_inpcb` stores into it, a referenced control block that
    // stays allocated until `pf_mbuf_unlink_inpcb` drops the reference.
    unsafe {
        m.m_pkthdr()
            .pf
            .inp
            .get()
            .cast::<Inpcb>()
            .cast_const()
            .as_ref()
    }
}

/// `pf_mbuf_link_state_key`.
pub fn pf_mbuf_link_state_key(m: &Mbuf, sk: &'static PfStateKey) {
    kassert!(m.m_pkthdr().pf.statekey.get().is_null());
    m.m_pkthdr()
        .pf
        .statekey
        .set(ptr::from_ref(pf_state_key_ref(sk)).cast_mut().cast());
}

/// `pf_mbuf_unlink_state_key`.
pub fn pf_mbuf_unlink_state_key(m: &Mbuf) {
    if let Some(sk) = pf_mbuf_statekey(m) {
        m.m_pkthdr().pf.statekey.set(ptr::null_mut());
        pf_state_key_unref(Some(sk));
    }
}

/// `pf_mbuf_link_inpcb`.
pub fn pf_mbuf_link_inpcb(m: &Mbuf, inp: Option<&'static Inpcb>) {
    kassert!(m.m_pkthdr().pf.inp.get().is_null());
    let r = crate::netinet::in_pcb::in_pcbref(inp);
    m.m_pkthdr()
        .pf
        .inp
        .set(r.map_or(ptr::null_mut(), |i| ptr::from_ref(i).cast_mut().cast()));
}

/// `pf_mbuf_unlink_inpcb`.
pub fn pf_mbuf_unlink_inpcb(m: &Mbuf) {
    if let Some(inp) = pf_mbuf_inp(m) {
        m.m_pkthdr().pf.inp.set(ptr::null_mut());
        crate::netinet::in_pcb::in_pcbunref(Some(inp));
    }
}

/// The socket's `inp_pf_sk`.
fn inpcb_pf_sk(inp: &Inpcb) -> Option<&'static PfStateKey> {
    inp.inp_pf_sk.get()
}

/// `inp->inp_pf_sk = sk`.
fn inpcb_set_pf_sk(inp: &Inpcb, sk: Option<&'static PfStateKey>) {
    inp.inp_pf_sk.set(sk);
}

/// `pf_state_key_link_inpcb`: links the key and the socket to each other.
pub fn pf_state_key_link_inpcb(sk: &'static PfStateKey, inp: Option<&'static Inpcb>) {
    let Some(inp) = inp else {
        return;
    };
    if sk.sk_inp.get().is_some() {
        return;
    }

    mtx_enter(&PF_INP_MTX);
    if inpcb_pf_sk(inp).is_some() || sk.sk_inp.get().is_some() {
        mtx_leave(&PF_INP_MTX);
        return;
    }
    sk.sk_inp.set(crate::netinet::in_pcb::in_pcbref(Some(inp)));
    inpcb_set_pf_sk(inp, Some(pf_state_key_ref(sk)));
    mtx_leave(&PF_INP_MTX);
}

/// `pf_state_key_unlink_inpcb`.
pub fn pf_state_key_unlink_inpcb(sk: &'static PfStateKey) {
    if sk.sk_inp.get().is_none() {
        return;
    }

    mtx_enter(&PF_INP_MTX);
    let Some(inp) = sk.sk_inp.get() else {
        mtx_leave(&PF_INP_MTX);
        return;
    };
    kassert!(opt_eq(inpcb_pf_sk(inp), Some(sk)));
    sk.sk_inp.set(None);
    inpcb_set_pf_sk(inp, None);
    mtx_leave(&PF_INP_MTX);

    pf_state_key_unref(Some(sk));
    crate::netinet::in_pcb::in_pcbunref(Some(inp));
}

/// `pf_state_key_unlink_reverse`. Note that `sk` and `skrev` may be equal, then we unref
/// twice.
pub fn pf_state_key_unlink_reverse(sk: &'static PfStateKey) {
    if let Some(skrev) = sk.sk_reverse.get() {
        kassert!(opt_eq(skrev.sk_reverse.get(), Some(sk)));
        sk.sk_reverse.set(None);
        skrev.sk_reverse.set(None);
        pf_state_key_unref(Some(skrev));
        pf_state_key_unref(Some(sk));
    }
}

/// `pf_state_ref`.
pub fn pf_state_ref(st: &'static PfState) -> &'static PfState {
    refcnt_take(&st.refcnt);
    st
}

/// `pf_state_unref`: frees the state with its last reference.
pub fn pf_state_unref(st: Option<&'static PfState>) {
    let Some(st) = st else {
        return;
    };
    if refcnt_rele(&st.refcnt) {
        // Never inserted or removed.
        kassert!(
            TailqHead::<PfStateSyncQueue>::next(st).is_none()
                || st.sync_state.get() >= crate::net::if_pfsync::PFSYNC_S_NONE
        );
        kassert!(TailqHead::<PfStateQueue>::next(st).is_none());

        pf_state_key_unref(st.key[PF_SK_WIRE].get());
        pf_state_key_unref(st.key[PF_SK_STACK].get());

        kassert!(st.linkage.is_empty());

        pf_pool_put(&PF_STATE_PL, st);
    }
}

/// `pf_delay_pkt`: holds the packet back for `m_pkthdr.pf.delay` milliseconds.
pub fn pf_delay_pkt(m: &'static Mbuf, ifidx: u32) -> Result<(), Errno> {
    let Some(pdy) = pf_pool_get::<PfPktdelay>(&PF_PKTDELAY_PL, PR_NOWAIT) else {
        crate::kern::uipc_mbuf::m_freem(m);
        return Err(Errno::ENOBUFS);
    };
    pdy.ifidx.set(ifidx);
    pdy.m.set(Some(m));
    crate::kern::kern_timeout::timeout_set(
        &pdy.to,
        pf_pktenqueue_delayed,
        ptr::from_ref(pdy).cast_mut().cast(),
    );
    crate::kern::kern_timeout::timeout_add_msec(&pdy.to, u64::from(m.m_pkthdr().pf.delay.get()));
    m.m_pkthdr().pf.delay.set(0);
    Ok(())
}

/// `pf_pktenqueue_delayed`: the delay is over; send the packet.
pub fn pf_pktenqueue_delayed(arg: *mut c_void) {
    // SAFETY: the argument is the `pf_pktdelay` `pf_delay_pkt` armed the timeout with; it is
    // freed only here.
    let Some(pdy) = (unsafe { arg.cast::<PfPktdelay>().cast_const().as_ref() }) else {
        return;
    };

    let ifp = if_get(pdy.ifidx.get());
    if let Some(ifp) = ifp {
        if let Some(m) = pdy.m.get() {
            let _ = crate::net::if_::if_enqueue(ifp, m);
        }
        if_put(Some(ifp));
    } else {
        crate::kern::uipc_mbuf::m_freem(pdy.m.get());
    }

    pf_pool_put(&PF_PKTDELAY_PL, pdy);
}

/// `pf_status_init`.
pub fn pf_status_init() {
    // SAFETY: called once from pfattach, before anything reads the status.
    unsafe {
        ptr::write_bytes(
            ptr::from_ref(&PF_STATUS).cast_mut().cast::<u8>(),
            0,
            size_of::<PfStatus>(),
        )
    };
    PF_STATUS.debug.set(LOG_ERR as u32);
    PF_STATUS.reass.set(PF_REASS_ENABLED);

    // XXX do our best to avoid a conflict.
    PF_STATUS.hostid.set(arc4random());

    for c in &PF_STATUS_FCOUNTERS {
        c.store(0, AtomicOrdering::Relaxed);
    }
}

/// `pf_status_clear`.
pub fn pf_status_clear() {
    pf_assert_locked();
    for c in &PF_STATUS_FCOUNTERS {
        c.store(0, AtomicOrdering::Relaxed);
    }
}

/// `pf_status_read`: a copy of the status with the interface's counters (`pfs->ifname`).
pub fn pf_status_read(pfs: &mut PfStatus) -> Result<(), Errno> {
    let ifname = pfs.ifname.get();
    if libkern::strnlen(&ifname, ifname.len()) >= ifname.len() {
        return Err(Errno::ENAMETOOLONG);
    }
    crate::sys::systm::net_lock();
    pf_lock();
    pf_frag_lock();
    pf_abi_copy_into(pfs, &PF_STATUS);
    pf_frag_unlock();
    crate::net::pf_if::pfi_update_status(&ifname, Some(pfs));
    pf_unlock();
    crate::sys::systm::net_unlock();

    for (i, c) in PF_STATUS_FCOUNTERS.iter().enumerate() {
        pfs.fcounters[i].set(c.load(AtomicOrdering::Relaxed));
    }
    Ok(())
}

/// `crate::kern::sched_bsd`'s `SPCF_SHOULDYIELD`: the scheduler asks the running thread to
/// yield.
fn should_yield() -> bool {
    use crate::machine::Machine;
    use crate::machine::cpu::Cpu;
    let ci = crate::machine::cpu::curcpu();
    Machine::ci_schedstate(ci)
        .spc_schedflags
        .load(AtomicOrdering::Relaxed)
        & crate::sys::sched::SPCF_SHOULDYIELD
        != 0
}

impl PfStatelim {
    /// A lookup key holding only `pfstlim_id` (the C's partly filled stack object).
    pub fn key(id: u32) -> Self {
        // SAFETY: every member is a Cell, an atomic-free link or head, or a pc lock: all-zero
        // is valid (the `PfPoolItem` promise).
        let k: Self = unsafe { core::mem::MaybeUninit::zeroed().assume_init() };
        k.pfstlim_id.set(id);
        k
    }
}

impl PfSourcelim {
    /// A lookup key holding only `pfsrlim_id`.
    pub fn key(id: u32) -> Self {
        // SAFETY: as for `PfStatelim::key`.
        let k: Self = unsafe { core::mem::MaybeUninit::zeroed().assume_init() };
        k.pfsrlim_id.set(id);
        k
    }
}

impl PfSource {
    /// An all-zero source (the C's stack `struct pf_source key`).
    pub fn zeroed() -> Self {
        // SAFETY: as for `PfStatelim::key`.
        unsafe { core::mem::MaybeUninit::zeroed().assume_init() }
    }
}

impl PfState {
    /// A lookup key holding only the id and creator (the C's cast `struct pf_state_cmp`).
    pub fn key(id: u64, creatorid: u32) -> Self {
        // SAFETY: as for `PfStatelim::key`; the mutex is never taken on a key.
        let k: Self = unsafe { core::mem::MaybeUninit::zeroed().assume_init() };
        k.id.set(id);
        k.creatorid.set(creatorid);
        k
    }
}

impl PfSrcNode {
    /// An all-zero node (the C's stack `struct pf_src_node k`).
    pub fn zeroed() -> Self {
        // SAFETY: `PfSrcNode: PfAbi`: every bit pattern is valid.
        unsafe { core::mem::MaybeUninit::zeroed().assume_init() }
    }
}

impl PfPoolLimit {
    /// `{ &pool, limit, limit }`.
    pub const fn new(pp: &'static Pool, limit: u32) -> Self {
        Self {
            pp: Some(pp),
            limit: Cell::new(limit),
            limit_new: Cell::new(limit),
        }
    }
}
/* </CODE> */

/* <TESTS> */
#[cfg(all(test, feature = "inet6"))]
mod tests {
    // Host tests for pf's packet path: `pf_print_host` of IPv4 and IPv6 addresses, the 128-bit
    // address helpers, the ICMP/ICMPv6 type translation of `af-to`, and `pf_test(AF_INET6)` on
    // synthetic packets: pass and block by IPv6
    // source prefix, a state created by an outbound ICMPv6 echo that lets the reply in, and the
    // IPv6 fragment cache of `pf_normalize_ip6`.

    use std::boxed::Box;
    use std::format;
    use std::sync::MutexGuard;
    use std::vec::Vec;

    use super::*;
    use crate::kern::kern_proc::procinit;
    use crate::kern::kern_prot::{crget, crhold};
    use crate::kern::kern_timeout::timeout_del;
    use crate::net::if_::if_attach;
    use crate::net::if_::tests::{setup_net, test_ifnet, test_packet};
    use crate::net::pf_ioctl::{pfattach, pfioctl};
    use crate::net::pf_ruleset::pf_main_ruleset;
    use crate::sys::fcntl::{FREAD, FWRITE};
    use crate::sys::ioctl::ioctl_ret;
    use crate::sys::proc::{Proc, Process};
    use crate::sys::ucred::Ucred;

    /// An address from its eight 16-bit groups.
    fn pf6(g: [u16; 8]) -> PfAddr {
        let mut a = PfAddr::zeroed();
        for (i, w) in g.iter().enumerate() {
            a.addr8[2 * i..2 * i + 2].copy_from_slice(&w.to_be_bytes());
        }
        a
    }

    #[test]
    fn print_host_ipv4_and_ipv6() {
        let v4 = PfAddr::from_v4(crate::netinet::in_::InAddr {
            s_addr: u32::from_ne_bytes([192, 0, 2, 1]),
        });
        assert_eq!(
            format!("{}", PfHost(&v4, 80u16.to_be(), AF_INET)),
            "192.0.2.1:80"
        );
        assert_eq!(format!("{}", PfHost(&v4, 0, AF_INET)), "192.0.2.1");

        let a = pf6([0x2001, 0xdb8, 0, 0, 0, 0, 0, 1]);
        assert_eq!(format!("{}", PfHost(&a, 0, AF_INET6)), "2001:db8::1");
        assert_eq!(
            format!("{}", PfHost(&a, 443u16.to_be(), AF_INET6)),
            "2001:db8::1[443]"
        );
        // The longest run of zero words is compressed, the first one on a tie.
        let b = pf6([0xfe80, 0, 0, 1, 0, 0, 0, 2]);
        assert_eq!(format!("{}", PfHost(&b, 0, AF_INET6)), "fe80:0:0:1::2");
        let c = pf6([0, 0, 0, 0, 0, 0, 0, 1]);
        assert_eq!(format!("{}", PfHost(&c, 0, AF_INET6)), "::1");
        let d = pf6([0xfd00, 1, 2, 3, 4, 5, 6, 7]);
        assert_eq!(format!("{}", PfHost(&d, 0, AF_INET6)), "fd00:1:2:3:4:5:6:7");
    }

    #[test]
    fn ipv6_address_helpers() {
        let lo = pf6([0x2001, 0xdb8, 0, 0, 0, 0, 0, 0xffff]);
        let hi = pf6([0x2001, 0xdb8, 0, 0, 0, 0, 1, 0]);
        let mut x = lo;
        pf_addr_inc(&mut x, AF_INET6);
        assert_eq!(x, pf6([0x2001, 0xdb8, 0, 0, 0, 0, 1, 0]));
        let mut all = pf6([0xffff; 8]);
        pf_addr_inc(&mut all, AF_INET6);
        assert_eq!(all, PfAddr::zeroed());

        assert!(pf_match_addr_range(&lo, &hi, &x, AF_INET6));
        assert!(!pf_match_addr_range(
            &lo,
            &hi,
            &pf6([0x2001, 0xdb8, 0, 0, 0, 0, 1, 1]),
            AF_INET6
        ));
        assert_eq!(
            pf_addr_compare(&lo, &hi, AF_INET6),
            -pf_addr_compare(&hi, &lo, AF_INET6)
        );

        let m48 = pf6([0xffff, 0xffff, 0xffff, 0, 0, 0, 0, 0]);
        let net = pf6([0x2001, 0xdb8, 1, 0, 0, 0, 0, 0]);
        assert!(pf_match_addr(
            0,
            &net,
            &m48,
            &pf6([0x2001, 0xdb8, 1, 9, 9, 9, 9, 9]),
            AF_INET6
        ));
        assert!(!pf_match_addr(
            0,
            &net,
            &m48,
            &pf6([0x2001, 0xdb8, 2, 0, 0, 0, 0, 0]),
            AF_INET6
        ));

        let mut n = PfAddr::zeroed();
        pf_poolmask(
            &mut n,
            &net,
            &m48,
            &pf6([0xaaaa, 0xbbbb, 0xcccc, 1, 2, 3, 4, 5]),
            AF_INET6,
        );
        assert_eq!(n, pf6([0x2001, 0xdb8, 1, 1, 2, 3, 4, 5]));

        // The checksum fixup of an address change equals a recomputation.
        let mut ck: u16 = 0x1234;
        pf_cksum_fixup_a(&mut ck, &lo, &hi, AF_INET6, IPPROTO_TCP as u8);
        let mut back = ck;
        pf_cksum_fixup_a(&mut back, &hi, &lo, AF_INET6, IPPROTO_TCP as u8);
        assert_eq!(back, 0x1234);
    }

    #[test]
    fn icmp_types_translate_between_families() {
        use crate::netinet::icmp6::*;
        use crate::netinet::ip_icmp::*;

        let mut pd = PfPdesc::new();
        pd.proto = IPPROTO_ICMP as u8;
        pd.pcksum = PfLoc::Hdr(2);
        // Echo request towards IPv6.
        pd.hdr_bytes()[..8].copy_from_slice(&[ICMP_ECHO, 0, 0x12, 0x34, 0, 7, 0, 1]);
        assert_eq!(pf_translate_icmp_af(&mut pd, AF_INET6, PfLoc::Hdr(0)), 0);
        assert_eq!(pd.hdr_bytes()[..2], [ICMP6_ECHO_REQUEST, 0]);
        // Need-frag becomes packet-too-big with the IPv6 MTU (20 bytes more).
        pd.hdr_bytes()[..8].copy_from_slice(&[
            ICMP_UNREACH,
            ICMP_UNREACH_NEEDFRAG,
            0,
            0,
            0,
            0,
            5,
            0xc8,
        ]);
        assert_eq!(pf_translate_icmp_af(&mut pd, AF_INET6, PfLoc::Hdr(0)), 0);
        assert_eq!(pd.hdr_bytes()[..2], [ICMP6_PACKET_TOO_BIG, 0]);
        assert_eq!(pd.hdr_bytes()[6..8], 1500u16.to_be_bytes());
        // Parameter problem in the IPv6 next header field becomes protocol unreachable.
        pd.proto = crate::netinet::in_::IPPROTO_ICMPV6 as u8;
        pd.hdr_bytes()[..8].copy_from_slice(&[
            ICMP6_PARAM_PROB,
            ICMP6_PARAMPROB_NEXTHEADER,
            0,
            0,
            0,
            0,
            0,
            6,
        ]);
        assert_eq!(pf_translate_icmp_af(&mut pd, AF_INET, PfLoc::Hdr(0)), 0);
        assert_eq!(pd.hdr_bytes()[..2], [ICMP_UNREACH, ICMP_UNREACH_PROTOCOL]);
        // A router advertisement has no IPv4 counterpart.
        pd.hdr_bytes()[0] = ND_ROUTER_ADVERT;
        assert_eq!(pf_translate_icmp_af(&mut pd, AF_INET, PfLoc::Hdr(0)), -1);
    }

    /// The network test lock with fresh memory, the process tables and pf attached, and one
    /// interface `pf6test0` attached to pf.
    fn setup() -> (
        MutexGuard<'static, ()>,
        MutexGuard<'static, ()>,
        &'static Ifnet,
    ) {
        let guard = setup_net();
        let t = crate::kern::kern_timeout::tests::LOCK
            .lock()
            .unwrap_or_else(|e| e.into_inner());
        procinit();
        crate::kern::kern_timeout::timeout_startup();
        crate::net::if_::softnet_init();
        crate::net::rtable::rtable_init();
        crate::net::route::route_init();
        pfattach(1);
        let ifp = test_ifnet(b"pf6test0");
        if_attach(ifp);
        (guard, t, ifp)
    }

    /// A thread of a root process, for `/dev/pf`.
    fn thread() -> &'static Proc {
        let cr: &'static Ucred = crget();
        let pr: &'static Process = Box::leak(Box::new(Process::new()));
        let p: &'static Proc = Box::leak(Box::new(Proc::new()));
        p.p_p.set(pr);
        p.p_ucred.set(cr);
        pr.ps_ucred.set(crhold(cr));
        pr.ps_mainproc.set(p);
        p
    }

    fn ioctl(cmd: u64, data: &mut [u8], p: &Proc) -> Result<(), Errno> {
        pfioctl(0, cmd, data, FREAD | FWRITE, p)
    }

    /// A rule of `af` `AF_INET6`: `action` in `dir`, quick, for `proto` (0: any) from
    /// `src/mask` to any, keeping state with `keep`.
    fn rule6(
        action: u8,
        dir: u8,
        proto: u8,
        src: PfAddr,
        mask: PfAddr,
        keep: bool,
    ) -> Box<PfiocRule> {
        let mut pr = pf_abi_zeroed::<PfiocRule>();
        pr.rule.action = action;
        pr.rule.direction = dir;
        pr.rule.quick = 1;
        pr.rule.af = AF_INET6;
        pr.rule.proto = proto;
        pr.rule.keep_state = if keep { PF_STATE_NORMAL } else { 0 };
        pr.rule.rtableid = -1;
        pr.rule.onrdomain = -1;
        pr.rule.src.addr.type_.set(PF_ADDR_ADDRMASK);
        let mut v = pr.rule.src.addr.v.get();
        v.set_addr(&src);
        v.set_mask(&mask);
        pr.rule.src.addr.v.set(v);
        pr.rule.dst.addr.type_.set(PF_ADDR_ADDRMASK);
        // No nat-to, rdr-to or route-to pool, as pfctl leaves them.
        pr.rule.nat.addr.type_.set(PF_ADDR_NONE);
        pr.rule.rdr.addr.type_.set(PF_ADDR_NONE);
        pr.rule.route.addr.type_.set(PF_ADDR_NONE);
        pr
    }

    /// Installs `rules` as the main ruleset and starts pf.
    fn load(rules: &mut [Box<PfiocRule>], p: &Proc) {
        let mut e = Box::new(PfiocTransE {
            type_: PF_TRANS_RULESET,
            anchor: [0; crate::sys::syslimits::PATH_MAX],
            ticket: 0,
        });
        let io = PfiocTrans {
            size: 1,
            esize: size_of::<PfiocTransE>() as i32,
            array: ptr::from_mut(&mut *e) as usize,
        };
        let mut data = std::vec![0u8; size_of::<PfiocTrans>()];
        ioctl_ret(&mut data, &io);
        assert_eq!(ioctl(DIOCXBEGIN, &mut data, p), Ok(()));
        for r in rules.iter_mut() {
            r.ticket = e.ticket;
            let mut d = pf_abi_bytes(&**r).to_vec();
            assert_eq!(ioctl(DIOCADDRULE, &mut d, p), Ok(()));
        }
        let mut data = std::vec![0u8; size_of::<PfiocTrans>()];
        ioctl_ret(&mut data, &io);
        assert_eq!(ioctl(DIOCXCOMMIT, &mut data, p), Ok(()));
        assert_eq!(ioctl(DIOCSTART, &mut [], p), Ok(()));
    }

    /// Stops pf and the purge timeouts `DIOCSTART` armed.
    fn unload(p: &Proc) {
        assert_eq!(ioctl(DIOCSTOP, &mut [], p), Ok(()));
        timeout_del(&PF_PURGE_STATES_TO);
        timeout_del(&PF_PURGE_TO);
    }

    /// An IPv6 packet `src -> dst` with next header `nxt` and `payload`, hop limit 64.
    fn ip6_packet(src: PfAddr, dst: PfAddr, nxt: u8, payload: &[u8]) -> Vec<u8> {
        let mut b = std::vec![0u8; 40];
        b[0] = 0x60;
        b[4..6].copy_from_slice(&(payload.len() as u16).to_be_bytes());
        b[6] = nxt;
        b[7] = 64;
        b[8..24].copy_from_slice(&src.addr8);
        b[24..40].copy_from_slice(&dst.addr8);
        b.extend_from_slice(payload);
        b
    }

    /// An ICMPv6 echo request (128) or reply (129) with `id` and sequence 1.
    fn echo6(type_: u8, id: u16) -> [u8; 8] {
        let mut e = [0u8; 8];
        e[0] = type_;
        e[4..6].copy_from_slice(&id.to_be_bytes());
        e[6..8].copy_from_slice(&1u16.to_be_bytes());
        e
    }

    /// `pf_test(AF_INET6, dir, ifp)` on `bytes`; the action, the packet freed whatever pf did.
    fn test6(dir: u8, ifp: &'static Ifnet, bytes: &[u8]) -> u8 {
        let mut m = Some(test_packet(bytes));
        let action = pf_test(AF_INET6, dir, ifp, &mut m);
        crate::kern::uipc_mbuf::m_freem(m);
        action
    }

    #[test]
    fn ipv6_rules_and_icmp6_echo_state() {
        let (_g, _t, ifp) = setup();
        let p = thread();
        let any = PfAddr::zeroed();
        let icmp6 = crate::netinet::in_::IPPROTO_ICMPV6 as u8;
        load(
            &mut [
                // pass in quick inet6 from 2001:db8:1::/48 no state
                rule6(
                    PF_PASS,
                    PF_IN,
                    0,
                    pf6([0x2001, 0xdb8, 1, 0, 0, 0, 0, 0]),
                    pf6([0xffff, 0xffff, 0xffff, 0, 0, 0, 0, 0]),
                    false,
                ),
                // pass out quick inet6 proto ipv6-icmp keep state
                rule6(PF_PASS, PF_OUT, icmp6, any, any, true),
                // block in quick inet6
                rule6(PF_DROP, PF_IN, 0, any, any, false),
            ],
            p,
        );
        assert_eq!(pf_main_ruleset().active.rcount.get(), 3);

        let local = pf6([0xfd00, 0, 0, 0, 0, 0, 0, 1]);
        let peer = pf6([0xfd00, 0, 0, 0, 0, 0, 0, 2]);

        // By source prefix.
        let inside = pf6([0x2001, 0xdb8, 1, 0, 0, 0, 0, 5]);
        let outside = pf6([0x2001, 0xdb8, 2, 0, 0, 0, 0, 5]);
        assert_eq!(
            test6(
                PF_IN,
                ifp,
                &ip6_packet(inside, local, icmp6, &echo6(128, 7))
            ),
            PF_PASS
        );
        assert_eq!(
            test6(
                PF_IN,
                ifp,
                &ip6_packet(outside, local, icmp6, &echo6(128, 7))
            ),
            PF_DROP
        );

        // A reply before any request is blocked.
        let reply = ip6_packet(peer, local, icmp6, &echo6(129, 0x1234));
        assert_eq!(test6(PF_IN, ifp, &reply), PF_DROP);
        // The outbound echo request creates a state; its reply then passes, another id does not.
        let before = PF_STATUS.states.get();
        assert_eq!(
            test6(
                PF_OUT,
                ifp,
                &ip6_packet(local, peer, icmp6, &echo6(128, 0x1234))
            ),
            PF_PASS
        );
        assert_eq!(PF_STATUS.states.get(), before + 1);
        assert_eq!(test6(PF_IN, ifp, &reply), PF_PASS);
        let other = ip6_packet(peer, local, icmp6, &echo6(129, 0x9999));
        assert_eq!(test6(PF_IN, ifp, &other), PF_DROP);

        unload(p);
    }

    /// A fragment of the datagram `ident`: offset `off` (bytes), more fragments `mf`, carrying
    /// `data` of a UDP datagram.
    fn frag6(src: PfAddr, dst: PfAddr, off: u16, mf: bool, data: &[u8]) -> Vec<u8> {
        let mut f = std::vec![0u8; 8];
        f[0] = IPPROTO_UDP as u8;
        f[2..4].copy_from_slice(&(off | u16::from(mf)).to_be_bytes());
        f[4..8].copy_from_slice(&0x0abc_def0u32.to_be_bytes());
        f.extend_from_slice(data);
        ip6_packet(src, dst, crate::netinet::in_::IPPROTO_FRAGMENT as u8, &f)
    }

    #[test]
    fn ipv6_fragments_are_reassembled() {
        let (_g, _t, _ifp) = setup();
        let src = pf6([0xfd00, 0, 0, 0, 0, 0, 0, 1]);
        let dst = pf6([0xfd00, 0, 0, 0, 0, 0, 0, 2]);
        // A UDP datagram of 24 bytes: the header and 16 bytes of data, in two fragments.
        let mut udp = std::vec![0u8; 24];
        udp[0..2].copy_from_slice(&5353u16.to_be_bytes());
        udp[2..4].copy_from_slice(&53u16.to_be_bytes());
        udp[4..6].copy_from_slice(&24u16.to_be_bytes());
        for (i, b) in udp[8..].iter_mut().enumerate() {
            *b = i as u8;
        }
        PF_STATUS.reass.set(PF_REASS_ENABLED);

        let mut reason = 0u16;
        let mut pd = PfPdesc::new();
        let m1 = test_packet(&frag6(src, dst, 0, true, &udp[..16]));
        assert_eq!(
            pf_setup_pdesc(&mut pd, AF_INET6, PF_IN, None, m1, &mut reason),
            PF_PASS
        );
        assert_eq!((pd.fragoff, pd.virtual_proto), (40, PF_VPROTO_FRAGMENT));
        assert_eq!(
            crate::net::pf_norm::pf_normalize_ip6(&mut pd, &mut reason),
            PF_PASS
        );
        assert!(pd.m.is_none(), "the first fragment waits in the cache");

        let m2 = test_packet(&frag6(src, dst, 16, false, &udp[16..]));
        assert_eq!(
            pf_setup_pdesc(&mut pd, AF_INET6, PF_IN, None, m2, &mut reason),
            PF_PASS
        );
        assert_eq!(
            crate::net::pf_norm::pf_normalize_ip6(&mut pd, &mut reason),
            PF_PASS
        );
        let m = pd.m.expect("reassembled");
        assert_eq!(m.m_pkthdr().len.get(), 40 + 24);
        let h = crate::netinet6::ip6_var::mtod_ip6(m);
        assert_eq!(
            (u16::from_be(h.ip6_plen), i32::from(h.ip6_nxt)),
            (24, IPPROTO_UDP)
        );
        let mut got = [0u8; 24];
        crate::kern::uipc_mbuf::m_copydata(m, 40, &mut got);
        assert_eq!(&got[..], &udp[..]);
        let tag = crate::kern::uipc_mbuf2::m_tag_find(
            m,
            crate::sys::mbuf::PACKET_TAG_PF_REASSEMBLED,
            None,
        );
        assert!(tag.is_some(), "pf_refragment6's tag");
        crate::kern::uipc_mbuf::m_freem(m);
        PF_STATUS.reass.set(0);
    }
}
/* </TESTS> */
