/*	$OpenBSD: pfvar_priv.h,v 1.43 2026/09/10 12:28:04 deraadt Exp $	*/
/* <LICENSES> */

/*
 * Copyright (c) 2001 Daniel Hartmeier
 * Copyright (c) 2002 - 2013 Henning Brauer <henning@openbsd.org>
 * Copyright (c) 2016 Alexander Bluhm <bluhm@openbsd.org>
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
 */
/* </LICENSES> */

//! `<net/pfvar_priv.h>`: pf's kernel-private types: states, state keys, the limiters, the
//! packet descriptor and the locks.
//!
//! Upstream: sys/net/pfvar_priv.h @ 3ce1f3f79392
//!
//! None of these structures crosses the user/kernel boundary, so they use the house idiom for
//! pool objects (`docs/C_TO_RUST.md`): `&'static T` items with `Cell` members, links to other
//! objects as `Cell<Option<&'static T>>`.
//!
//! ## Deviations
//! - `struct pf_pdesc` keeps the C's pointers into the packet (`src`, `dst`, `sport`, `dport`,
//!   `pcksum`) as [`PfLoc`] values: an offset into the descriptor's own copy of the protocol
//!   header (`hdr`), an offset into the first mbuf of the packet, or (built `unsafe`ly) the
//!   address of a header copy on the caller's stack. The descriptor's methods (`ld16`,
//!   `st16`, `ld_addr`, `st_addr`, ...) read and write through them; a pointer into the
//!   descriptor itself would not survive Rust's aliasing rules.
//! - The `kstat(4)` members of the limiters (`pfstlim_ks`, `pfsrlim_ks`) are left out: kstat is
//!   not configured (`NKSTAT` 0), so they would always be NULL.
//! - `pf_anchor_stackframe`'s union of `sf_r` and `sf_stack_top` is two members, the stack top
//!   an index into the stack (the C's pointer into the same per-CPU array).
//! - The `PF_LOCK()` family are functions; the locks themselves live where the C defines them
//!   (`net/pf_ioctl.rs` and `net/pf.rs`).

use core::cell::Cell;
use core::ptr;
use core::sync::atomic::{AtomicPtr, Ordering};

use crate::kassert;
use crate::net::pfvar::{
    PF_ANCHOR_STACK_MAX, PF_SOURCELIM_NAME_LEN, PF_STATELIM_NAME_LEN, PF_TABLE_NAME_SIZE, PfAddr,
    PfAnchor, PfRule, PfRulePtr, PfRuleSlist, PfRuleset, PfSnHead, PfStateKeyCmp, PfStatePeer,
    PfiKif, PfrKtable,
};
#[cfg(feature = "inet6")]
use crate::netinet::icmp6::{Icmp6Hdr, MldHdr, NdNeighborSolicit};
use crate::netinet::ip_icmp::Icmp;
use crate::netinet::tcp::Tcphdr;
use crate::netinet::udp::Udphdr;
use crate::sys::mbuf::{Mbuf, mtod};
use crate::sys::mutex::Mutex;
use crate::sys::pclock::PcLock;
use crate::sys::queue::{ListEntry, SlistEntry, SlistHead, TailqEntry, TailqHead};
use crate::sys::refcnt::Refcnt;
use crate::sys::rwlock::Rwlock;
use crate::sys::tree::{RbtEntry, RbtHead};
use crate::sys::types::{Gid, Pid, SaFamily, Time, Uid};

/// `PF_STATE_LINK_TYPE_STATELIM`.
pub const PF_STATE_LINK_TYPE_STATELIM: u32 = 1;
/// `PF_STATE_LINK_TYPE_SOURCELIM`.
pub const PF_STATE_LINK_TYPE_SOURCELIM: u32 = 2;

/// `struct pf_state_link`: wires a state into a state or source limiter's list. Each limiter
/// keeps the states it "owns" on `pfl_link`; a state finds its links through `pfl_linkage`
/// to unwire itself when it goes away.
pub struct PfStateLink {
    /// `pfl_link`: used by source/state pools to get to states.
    pub pfl_link: TailqEntry<PfStateLink>,
    /// `pfl_linkage`: used by pf_state to get to source/state pools.
    pub pfl_linkage: SlistEntry<PfStateLink>,
    /// `pfl_state`.
    pub pfl_state: Cell<Option<&'static PfState>>,
    /// `pfl_type`: `PF_STATE_LINK_TYPE_*`.
    pub pfl_type: Cell<u32>,
}

crate::queue_adapter!(
    /// `TAILQ_HEAD(pf_state_link_list, pf_state_link)`.
    pub PfStateLinkList: PfStateLink, pfl_link => TailqEntry<PfStateLink>
);
crate::queue_adapter!(
    /// `SLIST_HEAD(pf_state_linkage, pf_state_link)`.
    pub PfStateLinkage: PfStateLink, pfl_linkage => SlistEntry<PfStateLink>
);

/// `struct pf_state_item`: links a state key to one of its states.
pub struct PfStateItem {
    /// `si_entry`.
    pub si_entry: TailqEntry<PfStateItem>,
    /// `si_st`.
    pub si_st: Cell<Option<&'static PfState>>,
}

crate::queue_adapter!(
    /// `TAILQ_HEAD(pf_statelisthead, pf_state_item)`.
    pub PfStatelisthead: PfStateItem, si_entry => TailqEntry<PfStateItem>
);

/// `sk_reverse`: the key of the other direction of a forwarded connection, linked with
/// `atomic_cas_ptr` by the packets of either direction (`pf_state_key_link_reverse`).
pub struct PfSkReverse(AtomicPtr<PfStateKey>);

impl PfSkReverse {
    /// No reverse key.
    pub const fn new() -> Self {
        Self(AtomicPtr::new(ptr::null_mut()))
    }

    /// The reverse key.
    pub fn get(&self) -> Option<&'static PfStateKey> {
        // SAFETY: NULL or a key this one holds a reference on (`pf_state_key_link_reverse`),
        // which keeps it alive until `pf_state_key_unlink_reverse` clears the link.
        unsafe { self.0.load(Ordering::Acquire).as_ref() }
    }

    /// Sets the link (`pf_state_key_unlink_reverse` clears it).
    pub fn set(&self, sk: Option<&'static PfStateKey>) {
        self.0.store(
            sk.map_or(ptr::null_mut(), |k| ptr::from_ref(k).cast_mut()),
            Ordering::Release,
        );
    }

    /// `atomic_cas_ptr(&sk_reverse, NULL, sk)`: links `sk` if there is no link yet and
    /// returns the link that was there (`None` when this call linked `sk`).
    pub fn cas_null(&self, sk: &'static PfStateKey) -> Option<&'static PfStateKey> {
        match self.0.compare_exchange(
            ptr::null_mut(),
            ptr::from_ref(sk).cast_mut(),
            Ordering::AcqRel,
            Ordering::Acquire,
        ) {
            Ok(_) => None,
            // SAFETY: as for `get`.
            Err(old) => unsafe { old.as_ref() },
        }
    }
}

impl Default for PfSkReverse {
    fn default() -> Self {
        Self::new()
    }
}

/// `struct pf_state_key`: looks up states by address. Its first members are
/// `struct pf_state_key_cmp`'s.
pub struct PfStateKey {
    /// `addr[2]`.
    pub addr: [Cell<PfAddr>; 2],
    /// `port[2]`.
    pub port: [Cell<u16>; 2],
    /// `rdomain`.
    pub rdomain: Cell<u16>,
    /// `hash`.
    pub hash: Cell<u16>,
    /// `af`.
    pub af: Cell<SaFamily>,
    /// `proto`.
    pub proto: Cell<u8>,
    /// `sk_entry`.
    pub sk_entry: RbtEntry,
    /// `sk_states`.
    pub sk_states: TailqHead<PfStatelisthead>,
    /// `sk_reverse`.
    pub sk_reverse: PfSkReverse,
    /// `sk_inp`: \[L\] the socket.
    pub sk_inp: Cell<Option<&'static crate::netinet::in_pcb::Inpcb>>,
    /// `sk_refcnt`.
    pub sk_refcnt: Refcnt,
    /// `sk_removed`.
    pub sk_removed: Cell<u8>,
}

// SAFETY: changed under the net lock, `pf_state_lock` and `pf_inp_mtx` (`sk_inp`), as in C.
unsafe impl Sync for PfStateKey {}

impl PfStateKey {
    /// A key with the lookup members of `c` (the C's `RB_FIND` with a cast `pf_state_key_cmp`).
    pub fn from_cmp(c: &PfStateKeyCmp) -> Self {
        Self {
            addr: [Cell::new(c.addr[0]), Cell::new(c.addr[1])],
            port: [Cell::new(c.port[0]), Cell::new(c.port[1])],
            rdomain: Cell::new(c.rdomain),
            hash: Cell::new(c.hash),
            af: Cell::new(c.af),
            proto: Cell::new(c.proto),
            sk_entry: RbtEntry::new(),
            sk_states: TailqHead::new(),
            sk_reverse: PfSkReverse::new(),
            sk_inp: Cell::new(None),
            sk_refcnt: Refcnt::new(),
            sk_removed: Cell::new(0),
        }
    }

    /// The lookup members as a `struct pf_state_key_cmp`.
    pub fn cmp_key(&self) -> PfStateKeyCmp {
        PfStateKeyCmp {
            addr: [self.addr[0].get(), self.addr[1].get()],
            port: [self.port[0].get(), self.port[1].get()],
            rdomain: self.rdomain.get(),
            hash: self.hash.get(),
            af: self.af.get(),
            proto: self.proto.get(),
            ..PfStateKeyCmp::default()
        }
    }
}

crate::tree_adapter!(
    /// `RBT_HEAD(pf_state_tree, pf_state_key)`.
    pub PfStateTree: PfStateKey, sk_entry => RbtEntry, crate::net::pf::pf_state_compare_key
);

/// `PF_REVERSED_KEY(key, family)`: the wire and stack keys of an af-to state differ in
/// family, and the wire key is not of `family`.
pub fn pf_reversed_key(key: &[Cell<Option<&'static PfStateKey>>; 2], family: SaFamily) -> bool {
    use crate::net::pfvar::{PF_SK_STACK, PF_SK_WIRE};
    let (Some(w), Some(s)) = (key[PF_SK_WIRE].get(), key[PF_SK_STACK].get()) else {
        return false;
    };
    w.af.get() != s.af.get() && w.af.get() != family
}

/// `struct pf_state`: a state. Protection of the members, as in C: \[I\] immutable after
/// `pf_state_insert`, \[M\] the state's `mtx`, \[P\] `PF_STATE_LOCK`, \[S\] pfsync, \[L\]
/// `pf_state_list`, \[g\] the purge gc.
pub struct PfState {
    /// `id`: \[I\].
    pub id: Cell<u64>,
    /// `creatorid`: \[I\].
    pub creatorid: Cell<u32>,
    /// `direction`: \[I\].
    pub direction: Cell<u8>,
    /// `sync_list`: \[S\].
    pub sync_list: TailqEntry<PfState>,
    /// `sync_defer`: \[S\] pfsync's deferral of the state's first packet.
    pub sync_defer: Cell<Option<&'static crate::net::if_pfsync::PfsyncDeferral>>,
    /// `entry_list`: \[L\].
    pub entry_list: TailqEntry<PfState>,
    /// `gc_list`: \[g\].
    pub gc_list: SlistEntry<PfState>,
    /// `entry_id`: \[P\].
    pub entry_id: RbtEntry,
    /// `src`.
    pub src: PfStatePeer,
    /// `dst`.
    pub dst: PfStatePeer,
    /// `match_rules`: \[I\].
    pub match_rules: SlistHead<PfRuleSlist>,
    /// `rule`: \[I\].
    pub rule: PfRulePtr,
    /// `anchor`: \[I\].
    pub anchor: PfRulePtr,
    /// `natrule`: \[I\].
    pub natrule: PfRulePtr,
    /// `rt_addr`: \[I\].
    pub rt_addr: Cell<PfAddr>,
    /// `src_nodes`: \[I\].
    pub src_nodes: SlistHead<PfSnHead>,
    /// `key[2]`: \[I\] stack and wire.
    pub key: [Cell<Option<&'static PfStateKey>>; 2],
    /// `kif`: \[I\].
    pub kif: Cell<Option<&'static PfiKif>>,
    /// `mtx`.
    pub mtx: Mutex,
    /// `refcnt`.
    pub refcnt: Refcnt,
    /// `packets[2]`.
    pub packets: [Cell<u64>; 2],
    /// `bytes[2]`.
    pub bytes: [Cell<u64>; 2],
    /// `creation`: \[I\].
    pub creation: Cell<i32>,
    /// `expire`.
    pub expire: Cell<i32>,
    /// `pfsync_time`: \[S\].
    pub pfsync_time: Cell<i32>,
    /// `rtableid[2]`: \[I\] stack and wire.
    pub rtableid: [Cell<i32>; 2],
    /// `qid`: \[I\].
    pub qid: Cell<u16>,
    /// `pqid`: \[I\].
    pub pqid: Cell<u16>,
    /// `tag`: \[I\].
    pub tag: Cell<u16>,
    /// `state_flags`: \[M\].
    pub state_flags: Cell<u16>,
    /// `log`: \[I\].
    pub log: Cell<u8>,
    /// `timeout`.
    pub timeout: Cell<u8>,
    /// `sync_state`: \[S\] `PFSYNC_S_x`.
    pub sync_state: Cell<u8>,
    /// `sync_updates`: \[S\].
    pub sync_updates: Cell<u8>,
    /// `min_ttl`: \[I\].
    pub min_ttl: Cell<u8>,
    /// `set_tos`: \[I\].
    pub set_tos: Cell<u8>,
    /// `set_prio[2]`: \[I\].
    pub set_prio: [Cell<u8>; 2],
    /// `max_mss`: \[I\].
    pub max_mss: Cell<u16>,
    /// `if_index_in`: \[I\].
    pub if_index_in: Cell<u16>,
    /// `if_index_out`: \[I\].
    pub if_index_out: Cell<u16>,
    /// `delay`: \[I\].
    pub delay: Cell<u16>,
    /// `rt`: \[I\].
    pub rt: Cell<u8>,
    /// `statelim`.
    pub statelim: Cell<u8>,
    /// `sourcelim`.
    pub sourcelim: Cell<u8>,
    /// `linkage`.
    pub linkage: SlistHead<PfStateLinkage>,
}

// SAFETY: changed under the locks the members' docs name, as in C.
unsafe impl Sync for PfState {}

crate::queue_adapter!(
    /// `TAILQ_HEAD(pf_state_queue, pf_state)` through `entry_list`.
    pub PfStateQueue: PfState, entry_list => TailqEntry<PfState>
);
crate::queue_adapter!(
    /// `TAILQ_HEAD(pf_state_queue, pf_state)` through `sync_list`: pfsync's queues.
    pub PfStateSyncQueue: PfState, sync_list => TailqEntry<PfState>
);
crate::queue_adapter!(
    /// The purge gc's `SLIST_HEAD(, pf_state)` through `gc_list`.
    pub PfStateGcList: PfState, gc_list => SlistEntry<PfState>
);
crate::tree_adapter!(
    /// `RBT_HEAD(pf_state_tree_id, pf_state)`.
    pub PfStateTreeId: PfState, entry_id => RbtEntry, crate::net::pf::pf_state_compare_id
);

/// `struct pf_state_list`: the global list of states, for garbage collection, pfsync bulk
/// sends, `DIOCGETSTATES` and `DIOCCLRSTATES`.
///
/// States are inserted once they are in the state trees and removed only by the garbage
/// collector. The head and tail pointers are protected by `pfs_mtx`, the links between states
/// by `pfs_rwl`: an inserter takes the mutex, a traverser takes the rwlock for reading and a
/// snapshot of head and tail under the mutex, and the collector takes the rwlock for writing
/// and then the mutex to unlink. Lock order: `KERNEL_LOCK`, `NET_LOCK`, `pfs_rwl`, `PF_LOCK`,
/// `PF_STATE_LOCK`, `pfs_mtx`.
pub struct PfStateList {
    /// `pfs_list`: the list of states in the system.
    pub pfs_list: TailqHead<PfStateQueue>,
    /// `pfs_mtx`: serialise `pfs_list` head/tail access.
    pub pfs_mtx: Mutex,
    /// `pfs_rwl`: serialise access to pointers between `pfs_list` entries.
    pub pfs_rwl: Rwlock,
}

// SAFETY: see the type's doc.
unsafe impl Sync for PfStateList {}

impl PfStateList {
    /// `PF_STATE_LIST_INITIALIZER`.
    pub const fn new() -> Self {
        Self {
            pfs_list: TailqHead::new(),
            pfs_mtx: Mutex::new(crate::machine::intr::IPL_SOFTNET),
            pfs_rwl: Rwlock::new("pfstates"),
        }
    }
}

impl Default for PfStateList {
    fn default() -> Self {
        Self::new()
    }
}

/// The `rate` configuration of a limiter.
#[derive(Default)]
pub struct PfLimiterRate {
    /// `limit`.
    pub limit: Cell<u32>,
    /// `seconds`.
    pub seconds: Cell<u32>,
}

/// The `admitted`/`hardlimited`/`ratelimited` counters of a limiter or a source.
#[derive(Default)]
pub struct PfLimiterCounters {
    /// `admitted`.
    pub admitted: Cell<u64>,
    /// `hardlimited`.
    pub hardlimited: Cell<u64>,
    /// `ratelimited`.
    pub ratelimited: Cell<u64>,
}

/// The `pfstlim_timestamps` member of a state limiter.
#[derive(Default)]
pub struct PfStatelimTimestamps {
    /// `created`.
    pub created: Cell<Time>,
    /// `updated`.
    pub updated: Cell<Time>,
    /// `cleared`.
    pub cleared: Cell<Time>,
}

/// `struct pf_statelim`: a state limiter (`state limiter` in pf.conf).
pub struct PfStatelim {
    /// `pfstlim_id_tree`.
    pub pfstlim_id_tree: RbtEntry,
    /// `pfstlim_nm_tree`.
    pub pfstlim_nm_tree: RbtEntry,
    /// `pfstlim_list`.
    pub pfstlim_list: TailqEntry<PfStatelim>,
    /// `pfstlim_id`.
    pub pfstlim_id: Cell<u32>,
    /// `pfstlim_nm`.
    pub pfstlim_nm: Cell<[u8; PF_STATELIM_NAME_LEN]>,
    /// `pfstlim_limit`: config.
    pub pfstlim_limit: Cell<u32>,
    /// `pfstlim_rate`: config.
    pub pfstlim_rate: PfLimiterRate,
    /// `pfstlim_lock`: run state.
    pub pfstlim_lock: PcLock,
    /// `pfstlim_rate_ts`: rate limiter.
    pub pfstlim_rate_ts: Cell<u64>,
    /// `pfstlim_rate_token`.
    pub pfstlim_rate_token: Cell<u64>,
    /// `pfstlim_rate_bucket`.
    pub pfstlim_rate_bucket: Cell<u64>,
    /// `pfstlim_inuse`.
    pub pfstlim_inuse: Cell<u32>,
    /// `pfstlim_states`.
    pub pfstlim_states: TailqHead<PfStateLinkList>,
    /// `pfstlim_counters`.
    pub pfstlim_counters: PfLimiterCounters,
    /// `pfstlim_timestamps`.
    pub pfstlim_timestamps: PfStatelimTimestamps,
}

// SAFETY: changed under `pf_lock` and the limiter's `pc_lock`, as in C.
unsafe impl Sync for PfStatelim {}

crate::tree_adapter!(
    /// `RBT_HEAD(pf_statelim_id_tree, pf_statelim)`.
    pub PfStatelimIdTree: PfStatelim, pfstlim_id_tree => RbtEntry,
    crate::net::pf::pf_statelim_id_cmp
);
crate::tree_adapter!(
    /// `RBT_HEAD(pf_statelim_nm_tree, pf_statelim)`.
    pub PfStatelimNmTree: PfStatelim, pfstlim_nm_tree => RbtEntry,
    crate::net::pf::pf_statelim_nm_cmp
);
crate::queue_adapter!(
    /// `TAILQ_HEAD(pf_statelim_list, pf_statelim)`.
    pub PfStatelimList: PfStatelim, pfstlim_list => TailqEntry<PfStatelim>
);

/// `pf_statelim_enter`: begins an update of the limiter's run state.
pub fn pf_statelim_enter(pfstlim: &PfStatelim) -> u32 {
    crate::kern::kern_lock::pc_sprod_enter(&pfstlim.pfstlim_lock)
}

/// `pf_statelim_leave`.
pub fn pf_statelim_leave(pfstlim: &PfStatelim, generation: u32) {
    crate::kern::kern_lock::pc_sprod_leave(&pfstlim.pfstlim_lock, generation);
}

/// `struct pf_source`: an address entry of a source limiter.
pub struct PfSource {
    /// `pfsr_tree`.
    pub pfsr_tree: RbtEntry,
    /// `pfsr_ioc_tree`.
    pub pfsr_ioc_tree: RbtEntry,
    /// `pfsr_parent`.
    pub pfsr_parent: Cell<Option<&'static PfSourcelim>>,
    /// `pfsr_af`.
    pub pfsr_af: Cell<SaFamily>,
    /// `pfsr_rdomain`.
    pub pfsr_rdomain: Cell<u16>,
    /// `pfsr_addr`.
    pub pfsr_addr: Cell<PfAddr>,
    /// `pfsr_inuse`: run state.
    pub pfsr_inuse: Cell<u32>,
    /// `pfsr_intable`.
    pub pfsr_intable: Cell<u32>,
    /// `pfsr_states`.
    pub pfsr_states: TailqHead<PfStateLinkList>,
    /// `pfsr_empty_ts`.
    pub pfsr_empty_ts: Cell<Time>,
    /// `pfsr_empty_gc`.
    pub pfsr_empty_gc: TailqEntry<PfSource>,
    /// `pfsr_rate_ts`: rate limiter.
    pub pfsr_rate_ts: Cell<u64>,
    /// `pfsr_counters`.
    pub pfsr_counters: PfLimiterCounters,
}

// SAFETY: changed under `pf_lock` and the limiter's `pc_lock`, as in C.
unsafe impl Sync for PfSource {}

crate::tree_adapter!(
    /// `RBT_HEAD(pf_source_tree, pf_source)`.
    pub PfSourceTree: PfSource, pfsr_tree => RbtEntry, crate::net::pf::pf_source_cmp
);
crate::tree_adapter!(
    /// `RBT_HEAD(pf_source_ioc_tree, pf_source)`.
    pub PfSourceIocTree: PfSource, pfsr_ioc_tree => RbtEntry,
    crate::net::pf::pf_source_ioc_cmp
);
crate::queue_adapter!(
    /// `TAILQ_HEAD(pf_source_list, pf_source)`.
    pub PfSourceList: PfSource, pfsr_empty_gc => TailqEntry<PfSource>
);

/// The `pfsrlim_overload` member of a source limiter.
pub struct PfSourcelimOverload {
    /// `name`.
    pub name: Cell<[u8; PF_TABLE_NAME_SIZE]>,
    /// `hwm`.
    pub hwm: Cell<u32>,
    /// `lwm`.
    pub lwm: Cell<u32>,
    /// `table`.
    pub table: Cell<Option<&'static PfrKtable>>,
}

/// The `pfsrlim_counters` member of a source limiter.
#[derive(Default)]
pub struct PfSourcelimCounters {
    /// `addrallocs`: number of times pf_source was allocated.
    pub addrallocs: Cell<u64>,
    /// `addrlimited`: state was rejected because the address limit was hit.
    pub addrlimited: Cell<u64>,
    /// `addrnomem`: no memory to create address thing.
    pub addrnomem: Cell<u64>,
    /// `inuse`: sum of pf_source inuse gauges.
    pub inuse: Cell<u64>,
    /// `admitted`: sum of pf_source admitted counters.
    pub admitted: Cell<u64>,
    /// `hardlimited`: sum of pf_source hardlimited counters.
    pub hardlimited: Cell<u64>,
    /// `ratelimited`: sum of pf_source ratelimited counters.
    pub ratelimited: Cell<u64>,
}

/// `struct pf_sourcelim`: a source address limiter (`source limiter` in pf.conf).
pub struct PfSourcelim {
    /// `pfsrlim_id_tree`.
    pub pfsrlim_id_tree: RbtEntry,
    /// `pfsrlim_nm_tree`.
    pub pfsrlim_nm_tree: RbtEntry,
    /// `pfsrlim_list`.
    pub pfsrlim_list: TailqEntry<PfSourcelim>,
    /// `pfsrlim_id`.
    pub pfsrlim_id: Cell<u32>,
    /// `pfsrlim_nm`.
    pub pfsrlim_nm: Cell<[u8; PF_SOURCELIM_NAME_LEN]>,
    /// `pfsrlim_disabled`.
    pub pfsrlim_disabled: Cell<u32>,
    /// `pfsrlim_entries`: config.
    pub pfsrlim_entries: Cell<u32>,
    /// `pfsrlim_limit`.
    pub pfsrlim_limit: Cell<u32>,
    /// `pfsrlim_ipv4_prefix`.
    pub pfsrlim_ipv4_prefix: Cell<u32>,
    /// `pfsrlim_ipv6_prefix`.
    pub pfsrlim_ipv6_prefix: Cell<u32>,
    /// `pfsrlim_rate`.
    pub pfsrlim_rate: PfLimiterRate,
    /// `pfsrlim_overload`.
    pub pfsrlim_overload: PfSourcelimOverload,
    /// `pfsrlim_lock`: run state.
    pub pfsrlim_lock: PcLock,
    /// `pfsrlim_ipv4_mask`.
    pub pfsrlim_ipv4_mask: Cell<PfAddr>,
    /// `pfsrlim_ipv6_mask`.
    pub pfsrlim_ipv6_mask: Cell<PfAddr>,
    /// `pfsrlim_rate_token`.
    pub pfsrlim_rate_token: Cell<u64>,
    /// `pfsrlim_rate_bucket`.
    pub pfsrlim_rate_bucket: Cell<u64>,
    /// `pfsrlim_nsources`: number of pf_sources.
    pub pfsrlim_nsources: Cell<u32>,
    /// `pfsrlim_sources`.
    pub pfsrlim_sources: RbtHead<PfSourceTree>,
    /// `pfsrlim_ioc_sources`.
    pub pfsrlim_ioc_sources: RbtHead<PfSourceIocTree>,
    /// `pfsrlim_counters`.
    pub pfsrlim_counters: PfSourcelimCounters,
}

// SAFETY: changed under `pf_lock` and the limiter's `pc_lock`, as in C.
unsafe impl Sync for PfSourcelim {}

crate::tree_adapter!(
    /// `RBT_HEAD(pf_sourcelim_id_tree, pf_sourcelim)`.
    pub PfSourcelimIdTree: PfSourcelim, pfsrlim_id_tree => RbtEntry,
    crate::net::pf::pf_sourcelim_id_cmp
);
crate::tree_adapter!(
    /// `RBT_HEAD(pf_sourcelim_nm_tree, pf_sourcelim)`.
    pub PfSourcelimNmTree: PfSourcelim, pfsrlim_nm_tree => RbtEntry,
    crate::net::pf::pf_sourcelim_nm_cmp
);
crate::queue_adapter!(
    /// `TAILQ_HEAD(pf_sourcelim_list, pf_sourcelim)`.
    pub PfSourcelimList: PfSourcelim, pfsrlim_list => TailqEntry<PfSourcelim>
);

/// `pf_sourcelim_enter`.
pub fn pf_sourcelim_enter(pfsrlim: &PfSourcelim) -> u32 {
    crate::kern::kern_lock::pc_sprod_enter(&pfsrlim.pfsrlim_lock)
}

/// `pf_sourcelim_leave`.
pub fn pf_sourcelim_leave(pfsrlim: &PfSourcelim, generation: u32) {
    crate::kern::kern_lock::pc_sprod_leave(&pfsrlim.pfsrlim_lock, generation);
}

/// Where a `struct pf_pdesc` pointer points (see the module's deviations).
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum PfLoc {
    /// NULL.
    #[default]
    None,
    /// An offset into the descriptor's `hdr`.
    Hdr(usize),
    /// An offset into the data of the packet's first mbuf (`mtod(pd->m, caddr_t) + off`).
    Mbuf(usize),
    /// A header copy on the caller's stack; made by [`PfLoc::local`].
    Ptr(*mut u8),
}

impl PfLoc {
    /// The location of `field` in a header copy on the caller's stack.
    ///
    /// # Safety
    ///
    /// The object behind `field` must outlive every use of the location, and must not be
    /// moved, nor accessed through another path while a write through the location could
    /// still come (the C's pointer into a local).
    pub unsafe fn local<T>(field: *mut T) -> Self {
        Self::Ptr(field.cast())
    }

    /// The location `n` bytes further.
    pub fn offset(self, n: usize) -> Self {
        match self {
            Self::None => Self::None,
            Self::Hdr(o) => Self::Hdr(o + n),
            Self::Mbuf(o) => Self::Mbuf(o + n),
            // SAFETY: a byte offset within the same header copy (the callers step over members
            // of one structure).
            Self::Ptr(p) => Self::Ptr(unsafe { p.add(n) }),
        }
    }

    /// `ptr == NULL`.
    pub fn is_none(self) -> bool {
        self == Self::None
    }
}

/// `pf_pdesc.hdr`: the copy of the protocol header.
#[repr(C)]
#[derive(Clone, Copy)]
pub union PfPdescHdr {
    /// `tcp`.
    pub tcp: Tcphdr,
    /// `udp`.
    pub udp: Udphdr,
    /// `icmp`.
    pub icmp: Icmp,
    /// `icmp6`.
    #[cfg(feature = "inet6")]
    pub icmp6: Icmp6Hdr,
    /// `mld`.
    #[cfg(feature = "inet6")]
    pub mld: MldHdr,
    /// `nd_ns`.
    #[cfg(feature = "inet6")]
    pub nd_ns: NdNeighborSolicit,
    /// The bytes.
    pub bytes: [u8; core::mem::size_of::<Icmp>()],
}

impl PfPdescHdr {
    /// All zero.
    pub const fn zeroed() -> Self {
        Self {
            bytes: [0; core::mem::size_of::<Icmp>()],
        }
    }
}

/// The `lookup` member of a `struct pf_pdesc`: the socket's credentials, once looked up.
#[derive(Clone, Copy, Default, Debug)]
pub struct PfPdescLookup {
    /// `done`.
    pub done: i32,
    /// `uid`.
    pub uid: Uid,
    /// `gid`.
    pub gid: Gid,
    /// `pid`.
    pub pid: Pid,
}

/// `PF_OPT_OTHER`.
pub const PF_OPT_OTHER: u32 = 0x0001;
/// `PF_OPT_JUMBO`.
pub const PF_OPT_JUMBO: u32 = 0x0002;
/// `PF_OPT_ROUTER_ALERT`.
pub const PF_OPT_ROUTER_ALERT: u32 = 0x0004;
/// `PF_VPROTO_FRAGMENT`.
pub const PF_VPROTO_FRAGMENT: u16 = 256;

/// `struct pf_pdesc`: what pf knows about the packet it is testing.
pub struct PfPdesc {
    /// `lookup`.
    pub lookup: PfPdescLookup,
    /// `tot_len`: make Mickey money.
    pub tot_len: u64,
    /// `nsaddr`: src address after NAT.
    pub nsaddr: PfAddr,
    /// `ndaddr`: dst address after NAT.
    pub ndaddr: PfAddr,
    /// `kif`: incoming interface.
    pub kif: Option<&'static PfiKif>,
    /// `m`: mbuf containing the packet.
    pub m: Option<&'static Mbuf>,
    /// `src`: src address.
    pub src: PfLoc,
    /// `dst`: dst address.
    pub dst: PfLoc,
    /// `pcksum`: proto cksum.
    pub pcksum: PfLoc,
    /// `sport`.
    pub sport: PfLoc,
    /// `dport`.
    pub dport: PfLoc,
    /// `osport`.
    pub osport: u16,
    /// `odport`.
    pub odport: u16,
    /// `hash`.
    pub hash: u16,
    /// `nsport`: src port after NAT.
    pub nsport: u16,
    /// `ndport`: dst port after NAT.
    pub ndport: u16,
    /// `off`: protocol header offset.
    pub off: u32,
    /// `hdrlen`: protocol header length.
    pub hdrlen: u32,
    /// `p_len`: length of protocol payload.
    pub p_len: u32,
    /// `extoff`: extension header offset.
    pub extoff: u32,
    /// `fragoff`: fragment header offset.
    pub fragoff: u32,
    /// `jumbolen`: length from v6 jumbo header.
    pub jumbolen: u32,
    /// `badopts`: v4 options or v6 routing headers (`PF_OPT_*`).
    pub badopts: u32,
    /// `rdomain`: original routing domain.
    pub rdomain: u16,
    /// `virtual_proto`.
    pub virtual_proto: u16,
    /// `af`.
    pub af: SaFamily,
    /// `naf`.
    pub naf: SaFamily,
    /// `proto`.
    pub proto: u8,
    /// `tos`.
    pub tos: u8,
    /// `ttl`.
    pub ttl: u8,
    /// `dir`: direction.
    pub dir: u8,
    /// `sidx`: key index for source.
    pub sidx: u8,
    /// `didx`: key index for destination.
    pub didx: u8,
    /// `destchg`: flag set when destination changed.
    pub destchg: u8,
    /// `pflog`: flags for packet logging.
    pub pflog: u8,
    /// `hdr`.
    pub hdr: PfPdescHdr,
}

impl Default for PfPdesc {
    fn default() -> Self {
        Self::new()
    }
}

impl PfPdesc {
    /// `memset(pd, 0, sizeof(*pd))`.
    pub const fn new() -> Self {
        Self {
            lookup: PfPdescLookup {
                done: 0,
                uid: 0,
                gid: 0,
                pid: 0,
            },
            tot_len: 0,
            nsaddr: PfAddr::zeroed(),
            ndaddr: PfAddr::zeroed(),
            kif: None,
            m: None,
            src: PfLoc::None,
            dst: PfLoc::None,
            pcksum: PfLoc::None,
            sport: PfLoc::None,
            dport: PfLoc::None,
            osport: 0,
            odport: 0,
            hash: 0,
            nsport: 0,
            ndport: 0,
            off: 0,
            hdrlen: 0,
            p_len: 0,
            extoff: 0,
            fragoff: 0,
            jumbolen: 0,
            badopts: 0,
            rdomain: 0,
            virtual_proto: 0,
            af: 0,
            naf: 0,
            proto: 0,
            tos: 0,
            ttl: 0,
            dir: 0,
            sidx: 0,
            didx: 0,
            destchg: 0,
            pflog: 0,
            hdr: PfPdescHdr::zeroed(),
        }
    }

    /// The address of `n` bytes at `l`, checked against the object it is in.
    fn loc_ptr(&mut self, l: PfLoc, n: usize) -> *mut u8 {
        match l {
            #[allow(clippy::panic)] // the C dereferences NULL here; this is the panic it gets
            PfLoc::None => panic!("pf_pdesc: NULL location"),
            PfLoc::Hdr(o) => {
                // SAFETY: every member of the union is plain data; the byte view is valid.
                let b = unsafe { &mut self.hdr.bytes };
                b[o..o + n].as_mut_ptr()
            }
            PfLoc::Mbuf(o) => {
                #[allow(clippy::panic)] // a location in the packet of a pdesc without one
                let Some(m) = self.m else {
                    panic!("pf_pdesc: mbuf location without a packet")
                };
                kassert!(o + n <= m.m_len().get() as usize);
                // SAFETY: the bytes are within the first mbuf's data (asserted above; pf
                // pulled the headers it points at into it).
                unsafe { mtod::<u8>(m).add(o) }
            }
            PfLoc::Ptr(p) => p,
        }
    }

    /// `ALIGNED_POINTER(l, T)` for a `T` of `align` bytes.
    pub fn loc_aligned(&mut self, l: PfLoc, align: usize) -> bool {
        (self.loc_ptr(l, 1) as usize).is_multiple_of(align)
    }

    /// `*(u_int8_t *)l`.
    pub fn ld8(&mut self, l: PfLoc) -> u8 {
        let p = self.loc_ptr(l, 1);
        // SAFETY: `loc_ptr` checked the byte; `Ptr` locations are the caller's (`local`).
        unsafe { p.read() }
    }

    /// `*(u_int8_t *)l = v`.
    pub fn st8(&mut self, l: PfLoc, v: u8) {
        let p = self.loc_ptr(l, 1);
        // SAFETY: as for `ld8`.
        unsafe { p.write(v) }
    }

    /// `*(u_int16_t *)l` (any alignment).
    pub fn ld16(&mut self, l: PfLoc) -> u16 {
        let p = self.loc_ptr(l, 2);
        // SAFETY: as for `ld8`, two bytes, read unaligned.
        unsafe { p.cast::<u16>().read_unaligned() }
    }

    /// `*(u_int16_t *)l = v` (any alignment).
    pub fn st16(&mut self, l: PfLoc, v: u16) {
        let p = self.loc_ptr(l, 2);
        // SAFETY: as for `ld16`.
        unsafe { p.cast::<u16>().write_unaligned(v) }
    }

    /// `*(u_int32_t *)l` (any alignment).
    pub fn ld32(&mut self, l: PfLoc) -> u32 {
        let p = self.loc_ptr(l, 4);
        // SAFETY: as for `ld8`, four bytes, read unaligned.
        unsafe { p.cast::<u32>().read_unaligned() }
    }

    /// `*(u_int32_t *)l = v` (any alignment).
    pub fn st32(&mut self, l: PfLoc, v: u32) {
        let p = self.loc_ptr(l, 4);
        // SAFETY: as for `ld32`.
        unsafe { p.cast::<u32>().write_unaligned(v) }
    }

    /// `*(struct pf_addr *)l`: the 16 bytes at `l` (an IPv4 address uses the first 4, and the
    /// C reads only those; the rest of the result is zero).
    pub fn ld_addr(&mut self, l: PfLoc) -> PfAddr {
        let n = if self.af == crate::sys::socket::AF_INET6 {
            16
        } else {
            4
        };
        self.ld_addr_n(l, n)
    }

    /// The first `n` bytes of the address at `l`.
    pub fn ld_addr_n(&mut self, l: PfLoc, n: usize) -> PfAddr {
        let p = self.loc_ptr(l, n);
        let mut a = PfAddr::zeroed();
        // SAFETY: `loc_ptr` checked `n` bytes; `a` is a distinct local.
        unsafe { ptr::copy_nonoverlapping(p, a.addr8.as_mut_ptr(), n) };
        a
    }

    /// `*(struct pf_addr *)l = *a` for an address of family `af` (4 or 16 bytes).
    pub fn st_addr(&mut self, l: PfLoc, a: &PfAddr, af: SaFamily) {
        let n = if af == crate::sys::socket::AF_INET6 {
            16
        } else {
            4
        };
        let p = self.loc_ptr(l, n);
        // SAFETY: as for `ld_addr_n`.
        unsafe { ptr::copy_nonoverlapping(a.addr8.as_ptr(), p, n) };
    }

    /// `pd->hdr.tcp`.
    pub fn tcp(&self) -> &Tcphdr {
        // SAFETY: every member of the union is plain data.
        unsafe { &self.hdr.tcp }
    }

    /// `pd->hdr.tcp`, writable.
    pub fn tcp_mut(&mut self) -> &mut Tcphdr {
        // SAFETY: as for `tcp`.
        unsafe { &mut self.hdr.tcp }
    }

    /// `pd->hdr.udp`.
    pub fn udp(&self) -> &Udphdr {
        // SAFETY: as for `tcp`.
        unsafe { &self.hdr.udp }
    }

    /// `pd->hdr.udp`, writable.
    pub fn udp_mut(&mut self) -> &mut Udphdr {
        // SAFETY: as for `tcp`.
        unsafe { &mut self.hdr.udp }
    }

    /// `pd->hdr.icmp`.
    pub fn icmp(&self) -> &Icmp {
        // SAFETY: as for `tcp`.
        unsafe { &self.hdr.icmp }
    }

    /// `pd->hdr.icmp`, writable.
    pub fn icmp_mut(&mut self) -> &mut Icmp {
        // SAFETY: as for `tcp`.
        unsafe { &mut self.hdr.icmp }
    }

    /// `pd->hdr.icmp6`.
    #[cfg(feature = "inet6")]
    pub fn icmp6(&self) -> &Icmp6Hdr {
        // SAFETY: as for `tcp`.
        unsafe { &self.hdr.icmp6 }
    }

    /// `pd->hdr.icmp6`, writable.
    #[cfg(feature = "inet6")]
    pub fn icmp6_mut(&mut self) -> &mut Icmp6Hdr {
        // SAFETY: as for `tcp`.
        unsafe { &mut self.hdr.icmp6 }
    }

    /// `pd->hdr.mld`.
    #[cfg(feature = "inet6")]
    pub fn mld(&self) -> &MldHdr {
        // SAFETY: as for `tcp`.
        unsafe { &self.hdr.mld }
    }

    /// `pd->hdr.nd_ns`.
    #[cfg(feature = "inet6")]
    pub fn nd_ns(&self) -> &NdNeighborSolicit {
        // SAFETY: as for `tcp`.
        unsafe { &self.hdr.nd_ns }
    }

    /// The bytes of `pd->hdr`.
    pub fn hdr_bytes(&mut self) -> &mut [u8] {
        // SAFETY: as for `tcp`.
        unsafe { &mut self.hdr.bytes }
    }
}

/// `struct pf_anchor_stackframe`: one level of the anchor evaluation stack.
pub struct PfAnchorStackframe {
    /// `sf_rs`.
    pub sf_rs: Cell<Option<&'static PfRuleset>>,
    /// `sf_anchor`.
    pub sf_anchor: Cell<Option<&'static PfRule>>,
    /// `sf_r` (`u.u_r`).
    pub sf_r: Cell<Option<&'static PfRule>>,
    /// `sf_stack_top` (`u.u_stack_top`): the index of the top frame, kept in the frame after
    /// the last usable one.
    pub sf_stack_top: Cell<usize>,
    /// `sf_child`.
    pub sf_child: Cell<Option<&'static PfAnchor>>,
    /// `sf_jump_target`.
    pub sf_jump_target: Cell<i32>,
}

impl PfAnchorStackframe {
    /// An empty frame.
    pub const fn new() -> Self {
        Self {
            sf_rs: Cell::new(None),
            sf_anchor: Cell::new(None),
            sf_r: Cell::new(None),
            sf_stack_top: Cell::new(0),
            sf_child: Cell::new(None),
            sf_jump_target: Cell::new(0),
        }
    }
}

impl Default for PfAnchorStackframe {
    fn default() -> Self {
        Self::new()
    }
}

/// `PF_NEXT_RULE`.
pub const PF_NEXT_RULE: i32 = 0;
/// `PF_NEXT_CHILD`.
pub const PF_NEXT_CHILD: i32 = 1;

/// The anchor stack: `PF_ANCHOR_STACK_MAX + 2` frames (`pf_anchor_stack`, a per-CPU array in
/// the C; one array here, used only under `pf_lock`, see `net/pf.rs`).
pub struct PfAnchorStack(pub [PfAnchorStackframe; PF_ANCHOR_STACK_MAX + 2]);

// SAFETY: used only by pf_test's rule evaluation (`pf_match_rule`), with `pf_lock` held
// exclusively.
unsafe impl Sync for PfAnchorStack {}

/// `enum pf_trans_type`.
pub type PfTransType = u32;
/// `PF_TRANS_NONE`.
pub const PF_TRANS_NONE: PfTransType = 0;
/// `PF_TRANS_GETRULE`.
pub const PF_TRANS_GETRULE: PfTransType = 1;
/// `PF_TRANS_MAX`.
pub const PF_TRANS_MAX: PfTransType = 2;

/// `struct pf_trans`: an ioctl transaction (today: walking the rules with `DIOCGETRULE`).
pub struct PfTrans {
    /// `pft_entry`.
    pub pft_entry: ListEntry<PfTrans>,
    /// `pft_unit`: process id (the minor of the clone device).
    pub pft_unit: Cell<u32>,
    /// `pft_ticket`.
    pub pft_ticket: Cell<u64>,
    /// `pft_type`.
    pub pft_type: Cell<PfTransType>,
    /// `pftgr_version` (`u.u_getrule.gr_version`).
    pub pftgr_version: Cell<u32>,
    /// `pftgr_anchor` (`u.u_getrule.gr_anchor`).
    pub pftgr_anchor: Cell<Option<&'static PfAnchor>>,
    /// `pftgr_rule` (`u.u_getrule.gr_rule`).
    pub pftgr_rule: Cell<Option<&'static PfRule>>,
}

crate::queue_adapter!(
    /// `LIST_HEAD(, pf_trans)`.
    pub PfTransList: PfTrans, pft_entry => ListEntry<PfTrans>
);

/// `PF_LOCK()`.
pub fn pf_lock() {
    crate::kern::kern_rwlock::rw_enter_write(&crate::net::pf_ioctl::PF_LOCK);
}

/// `PF_UNLOCK()`.
pub fn pf_unlock() {
    pf_assert_locked();
    crate::kern::kern_rwlock::rw_exit_write(&crate::net::pf_ioctl::PF_LOCK);
}

/// `PF_ASSERT_LOCKED()`.
pub fn pf_assert_locked() {
    use crate::sys::rwlock::RW_WRITE;
    let s = crate::kern::kern_rwlock::rw_status(&crate::net::pf_ioctl::PF_LOCK);
    if s != RW_WRITE {
        crate::kern::subr_prf::splassert_fail(RW_WRITE, s, "pf_assert_locked");
    }
}

/// `PF_ASSERT_UNLOCKED()`.
pub fn pf_assert_unlocked() {
    use crate::sys::rwlock::RW_WRITE;
    let s = crate::kern::kern_rwlock::rw_status(&crate::net::pf_ioctl::PF_LOCK);
    if s == RW_WRITE {
        crate::kern::subr_prf::splassert_fail(0, s, "pf_assert_unlocked");
    }
}

/// `PF_STATE_ENTER_READ()`.
pub fn pf_state_enter_read() {
    crate::kern::kern_rwlock::rw_enter_read(&crate::net::pf_ioctl::PF_STATE_LOCK);
}

/// `PF_STATE_EXIT_READ()`.
pub fn pf_state_exit_read() {
    crate::kern::kern_rwlock::rw_exit_read(&crate::net::pf_ioctl::PF_STATE_LOCK);
}

/// `PF_STATE_ENTER_WRITE()`.
pub fn pf_state_enter_write() {
    crate::kern::kern_rwlock::rw_enter_write(&crate::net::pf_ioctl::PF_STATE_LOCK);
}

/// `PF_STATE_EXIT_WRITE()`.
pub fn pf_state_exit_write() {
    pf_state_assert_locked();
    crate::kern::kern_rwlock::rw_exit_write(&crate::net::pf_ioctl::PF_STATE_LOCK);
}

/// `PF_STATE_ASSERT_LOCKED()`.
pub fn pf_state_assert_locked() {
    use crate::sys::rwlock::RW_WRITE;
    let s = crate::kern::kern_rwlock::rw_status(&crate::net::pf_ioctl::PF_STATE_LOCK);
    if s != RW_WRITE {
        crate::kern::subr_prf::splassert_fail(RW_WRITE, s, "pf_state_assert_locked");
    }
}

/// `PF_FRAG_LOCK()`.
pub fn pf_frag_lock() {
    crate::kern::kern_lock::mtx_enter(&crate::net::pf_norm::PF_FRAG_MTX);
}

/// `PF_FRAG_UNLOCK()`.
pub fn pf_frag_unlock() {
    crate::kern::kern_lock::mtx_leave(&crate::net::pf_norm::PF_FRAG_MTX);
}

/// One of pf's global heads or tables (`pf_statetbl`, `tree_id`, `pf_anchors`, ...), made
/// `Sync`: like the C globals, they are changed under the net lock, `pf_lock` and
/// `pf_state_lock` (or the list's own lock or SMR, as its doc says).
pub struct PfGlobal<T>(pub T);

// SAFETY: see the type's doc.
unsafe impl<T> Sync for PfGlobal<T> {}

impl<T> core::ops::Deref for PfGlobal<T> {
    type Target = T;

    fn deref(&self) -> &T {
        &self.0
    }
}

// The byte view covers every member of `pf_pdesc.hdr`.
#[cfg(feature = "inet6")]
const _: () = {
    use core::mem::size_of;
    assert!(size_of::<MldHdr>() <= size_of::<Icmp>());
    assert!(size_of::<NdNeighborSolicit>() <= size_of::<Icmp>());
    assert!(size_of::<PfPdescHdr>() == size_of::<Icmp>());
};
