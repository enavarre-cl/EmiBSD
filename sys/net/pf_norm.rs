/*	$OpenBSD: pf_norm.c,v 1.239 2026/08/11 14:28:59 bluhm Exp $ */
/* <LICENSES> */

/*
 * Copyright 2001 Niels Provos <provos@citi.umich.edu>
 * Copyright 2009 Henning Brauer <henning@openbsd.org>
 * Copyright 2011-2018 Alexander Bluhm <bluhm@openbsd.org>
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
/* </LICENSES> */

//! pf's normalizer (`scrub`): IPv4 fragment reassembly, TCP header sanitising, the per-state
//! TCP normalization (minimum TTL, timestamp modulation, PAWS checks), MSS clamping and the
//! `no-df`/`min-ttl`/`set-tos`/`random-id` rewrites of `pf_scrub`.
//!
//! Upstream: sys/net/pf_norm.c @ 3ce1f3f79392
//!
//! Fragments are queued per packet (`struct pf_fragment`, found by IP id in the tree of its
//! `struct pf_frnode`, one per protocol, addresses, family, direction and routing domain)
//! until the queue has no holes; then the mbufs are joined into one packet. Overlaps are
//! trimmed (IPv4); sixteen entry points into each queue (`fr_firstoff`, by offset) keep the
//! search for a fragment's neighbours short and its length bounded (`PF_FRAG_ENTRY_LIMIT`).
//! The fragment state is protected by `pf_frag_mtx` (`PF_FRAG_LOCK`).
//!
//! ## Deviations
//! - `pf_refragment6` frees the packet in hand and the rest of the fragment list when
//!   forwarding is off; the C returns `PF_DROP` leaving them allocated.
//! - `pf_frent_holes` takes the fragment as well: `TAILQ_PREV` needs the queue's head here
//!   (`sys/queue.rs`).
//! - `pf_frent_insert`'s `ENOBUFS` is `Err(Errno::ENOBUFS)`; `pf_normalize_tcp_init`'s 1 on
//!   failure is `Err(Errno::ENOMEM)` (its only failure is the scrub allocation).
//! - The IP header is read and written as a copy (`mtod_ip`/`mtod_ip_store`); the TCP options
//!   are patched in a local copy through `PfLoc::local`, as `net/pf.rs` does.
//! - The pools' `struct pool` objects and `pf_frag_mtx` are statics initialised by
//!   `pf_normalize_init`, as in C. `PfPoolItem` for `PfStateScrub` is declared in `net/pf.rs`.

use core::cell::Cell;
use core::cmp::Ordering;
use core::mem::{offset_of, size_of};
#[cfg(feature = "inet6")]
use core::ptr;

use crate::dev::rnd::arc4random;
use crate::kassert;
use crate::kern::kern_lock::mtx_init;
use crate::kern::kern_tc::{getmicrouptime, getuptime};
use crate::kern::subr_pool::{pool_sethardlimit, pool_sethiwat};
use crate::kern::subr_prf::{addlog, log, panic};
use crate::kern::uipc_mbuf::{m_adj, m_calchdrlen, m_cat, m_copyback, m_freem, m_removehdr};
use crate::machine::intr::IPL_SOFTNET;
use crate::net::if_::unhandled_af;
use crate::net::pf::{
    PF_HI, PF_LO, PF_STATUS, pf_addr_compare, pf_algnmnt, pf_cksum_fixup, pf_find_tcpopt, pf_inc,
    pf_patch_8, pf_patch_16, pf_patch_16_unaligned, pf_patch_32_unaligned, pf_print_flags,
    pf_print_state, pf_pull_hdr,
};
use crate::net::pf_ioctl::PF_DEFAULT_RULE;
use crate::net::pfvar::{
    NCNT_FRAG_INSERT, NCNT_FRAG_REMOVALS, NCNT_FRAG_SEARCH, PF_DROP, PF_FRAG_ENTRY_LIMIT,
    PF_FRAG_ENTRY_POINTS, PF_FRAG_STALE, PF_PASS, PF_REASS_NODF, PFRES_FRAG, PFRES_MEMORY,
    PFRES_NORM, PFRES_SHORT, PFRES_TS, PFSS_DATA_NOTS, PFSS_DATA_TS, PFSS_PAWS, PFSS_PAWS_IDLED,
    PFSS_TIMESTAMP, PFSTATE_NODF, PFSTATE_RANDOMID, PFSTATE_SETTOS, PFTM_FRAG, PFTM_TS_DIFF,
    PfAddr, PfPoolItem, PfStatePeer, PfStateScrub, pf_pool_get, pf_pool_init, pf_pool_put,
    pffrag_frag_hiwat, pffrag_frent_hiwat, reason_set,
};
use crate::net::pfvar_priv::{
    PfGlobal, PfLoc, PfPdesc, PfState, pf_assert_unlocked, pf_frag_lock, pf_frag_unlock,
};
use crate::netinet::ip::{IP_DF, IP_MAXPACKET, IP_MF, IP_OFFMASK, IPTOS_ECN_MASK};
use crate::netinet::ip_id::ip_randomid;
use crate::netinet::ip_var::{mtod_ip, mtod_ip_store};
use crate::netinet::tcp::{
    MAX_TCPOPTLEN, TCPOLEN_MAXSEG, TCPOLEN_TIMESTAMP, TCPOPT_MAXSEG, TCPOPT_TIMESTAMP, TH_ACK,
    TH_FIN, TH_PUSH, TH_RST, TH_SYN, TH_URG, Tcphdr,
};
use crate::netinet::tcp_fsm::TCPS_ESTABLISHED;
use crate::netinet::tcp_seq::{seq_geq, seq_gt, seq_lt};
use crate::sys::errno::Errno;
use crate::sys::malloc::M_NOWAIT;
use crate::sys::mbuf::Mbuf;
use crate::sys::mutex::Mutex;
use crate::sys::pool::{PR_NOWAIT, PR_ZERO, Pool};
use crate::sys::queue::{TailqEntry, TailqHead};
use crate::sys::socket::AF_INET;
use crate::sys::syslog::{LOG_DEBUG, LOG_INFO, LOG_NOTICE, LOG_WARNING};
use crate::sys::time::timersub;
use crate::sys::tree::{RbEntry, RbHead};
use crate::sys::types::SaFamily;

#[cfg(feature = "inet6")]
use crate::{
    kern::uipc_mbuf::{m_getptr, ml_dequeue, ml_purge},
    kern::uipc_mbuf2::{m_tag_delete, m_tag_get, m_tag_prepend},
    net::if_var::Ifnet,
    net::route::Rtentry,
    netinet::icmp6::ICMP6_PACKET_TOO_BIG,
    netinet::in_::IPPROTO_FRAGMENT,
    netinet::ip6::{IP6F_MORE_FRAG, IP6F_OFF_MASK, IPV6_MAXPACKET, Ip6Ext, Ip6Frag, Ip6Hdr},
    netinet6::frag6::frag6_deletefraghdr,
    netinet6::icmp6::icmp6_error,
    netinet6::in6::{SockaddrIn6, sin6tosa_const},
    netinet6::in6_proto::IP6_FORWARDING,
    netinet6::ip6_forward::ip6_forward,
    netinet6::ip6_output::{in6_proto_cksum_out, ip6_fragment},
    netinet6::ip6_var::{mtod_ip6, mtod_ip6_store},
    sys::mbuf::{MTag, MbufList, PACKET_TAG_PF_REASSEMBLED, PF_TAG_REFRAGMENTED, mtod},
    sys::socket::{AF_INET6, Sockaddr},
};

/// `struct pf_frent`: one fragment of a packet in reassembly.
pub struct PfFrent {
    /// `fr_next`.
    pub fr_next: TailqEntry<PfFrent>,
    /// `fe_m`.
    pub fe_m: Cell<Option<&'static Mbuf>>,
    /// `fe_hdrlen`: ipv4 header length with ip options; ipv6, extension, fragment header.
    pub fe_hdrlen: Cell<u16>,
    /// `fe_extoff`: last extension header offset or 0.
    pub fe_extoff: Cell<u16>,
    /// `fe_len`: fragment length.
    pub fe_len: Cell<u16>,
    /// `fe_off`: fragment offset.
    pub fe_off: Cell<u16>,
    /// `fe_mff`: more fragment flag.
    pub fe_mff: Cell<u16>,
}

// SAFETY: links, `Cell`s of integers and of an `Option<&Mbuf>`: all-zero is a valid, unlinked
// entry.
unsafe impl PfPoolItem for PfFrent {}

crate::queue_adapter!(
    /// `TAILQ_HEAD(pf_fragq, pf_frent)`: a packet's fragments by offset.
    pub PfFragq: PfFrent, fr_next => TailqEntry<PfFrent>
);

crate::tree_adapter!(
    /// `RB_HEAD(pf_frag_tree, pf_fragment)`: a node's packets by IP id.
    pub PfFragTree: PfFragment, fr_entry => RbEntry<PfFragment>, pf_frag_compare
);

/// `struct pf_frnode`: the packets in reassembly of one protocol, source, destination,
/// family, direction and routing domain.
pub struct PfFrnode {
    /// `fn_src`: ip source address.
    pub fn_src: Cell<PfAddr>,
    /// `fn_dst`: ip destination address.
    pub fn_dst: Cell<PfAddr>,
    /// `fn_fragments`: number of entries in `fn_tree`.
    pub fn_fragments: Cell<u32>,
    /// `fn_gen`: `fr_gen` of newest entry in `fn_tree`.
    pub fn_gen: Cell<u32>,
    /// `fn_rdomain`: routing domain.
    pub fn_rdomain: Cell<u16>,
    /// `fn_af`: address family.
    pub fn_af: Cell<SaFamily>,
    /// `fn_proto`: protocol for fragments in `fn_tree`.
    pub fn_proto: Cell<u8>,
    /// `fn_direction`: pf packet direction.
    pub fn_direction: Cell<u8>,
    /// `fn_entry`.
    pub fn_entry: RbEntry<PfFrnode>,
    /// `fn_tree`: matching fragments, lookup by id.
    pub fn_tree: RbHead<PfFragTree>,
}

impl PfFrnode {
    /// An empty node (a lookup key).
    pub const fn new() -> Self {
        Self {
            fn_src: Cell::new(PfAddr::zeroed()),
            fn_dst: Cell::new(PfAddr::zeroed()),
            fn_fragments: Cell::new(0),
            fn_gen: Cell::new(0),
            fn_rdomain: Cell::new(0),
            fn_af: Cell::new(0),
            fn_proto: Cell::new(0),
            fn_direction: Cell::new(0),
            fn_entry: RbEntry::new(),
            fn_tree: RbHead::new(),
        }
    }
}

impl Default for PfFrnode {
    fn default() -> Self {
        Self::new()
    }
}

// SAFETY: `Cell`s of integers and addresses, a tree entry and head: all-zero is a valid,
// unlinked node with an empty tree.
unsafe impl PfPoolItem for PfFrnode {}

/// `struct pf_fragment`: a packet in reassembly.
pub struct PfFragment {
    /// `fr_firstoff[PF_FRAG_ENTRY_POINTS]`: pointers to queue element.
    pub fr_firstoff: [Cell<Option<&'static PfFrent>>; PF_FRAG_ENTRY_POINTS],
    /// `fr_entries[PF_FRAG_ENTRY_POINTS]`: count entries between pointers.
    pub fr_entries: [Cell<u8>; PF_FRAG_ENTRY_POINTS],
    /// `fr_entry`.
    pub fr_entry: RbEntry<PfFragment>,
    /// `frag_next`.
    pub frag_next: TailqEntry<PfFragment>,
    /// `fr_queue`.
    pub fr_queue: TailqHead<PfFragq>,
    /// `fr_id`: fragment id for reassemble.
    pub fr_id: Cell<u32>,
    /// `fr_timeout`.
    pub fr_timeout: Cell<i32>,
    /// `fr_gen`: generation number (per pf_frnode).
    pub fr_gen: Cell<u32>,
    /// `fr_maxlen`: maximum length of single fragment.
    pub fr_maxlen: Cell<u16>,
    /// `fr_holes`: number of holes in the queue.
    pub fr_holes: Cell<u16>,
    /// `fr_node`: ip src/dst/proto/af for fragments.
    pub fr_node: Cell<Option<&'static PfFrnode>>,
}

impl PfFragment {
    /// An empty fragment (a lookup key).
    pub const fn new() -> Self {
        Self {
            fr_firstoff: [const { Cell::new(None) }; PF_FRAG_ENTRY_POINTS],
            fr_entries: [const { Cell::new(0) }; PF_FRAG_ENTRY_POINTS],
            fr_entry: RbEntry::new(),
            frag_next: TailqEntry::new(),
            fr_queue: TailqHead::new(),
            fr_id: Cell::new(0),
            fr_timeout: Cell::new(0),
            fr_gen: Cell::new(0),
            fr_maxlen: Cell::new(0),
            fr_holes: Cell::new(0),
            fr_node: Cell::new(None),
        }
    }

    /// `fr_node`, which every queued fragment has.
    fn node(&self) -> &'static PfFrnode {
        match self.fr_node.get() {
            Some(n) => n,
            None => panic(format_args!("pf_fragment without a node")),
        }
    }
}

impl Default for PfFragment {
    fn default() -> Self {
        Self::new()
    }
}

// SAFETY: `Cell`s of integers and options of references, links and an empty queue head:
// all-zero is a valid, unlinked fragment.
unsafe impl PfPoolItem for PfFragment {}

crate::queue_adapter!(
    /// `TAILQ_HEAD(pf_fragqueue, pf_fragment)`: all packets in reassembly, newest first.
    pub PfFragqueue: PfFragment, frag_next => TailqEntry<PfFragment>
);

crate::tree_adapter!(
    /// `RB_HEAD(pf_frnode_tree, pf_frnode)`.
    pub PfFrnodeTree: PfFrnode, fn_entry => RbEntry<PfFrnode>, pf_frnode_compare
);

/// `struct pf_fragment_tag`: what `pf_reassemble6` leaves on a reassembled IPv6 packet for
/// `pf_refragment6`.
#[repr(C)]
#[derive(Clone, Copy, Default, Debug)]
pub struct PfFragmentTag {
    /// `ft_hdrlen`: header length of reassembled pkt.
    pub ft_hdrlen: u16,
    /// `ft_extoff`: last extension header offset or 0.
    pub ft_extoff: u16,
    /// `ft_maxlen`: maximum fragment payload length.
    pub ft_maxlen: u16,
}

/// `pf_fragqueue`.
pub static PF_FRAGQUEUE: PfGlobal<TailqHead<PfFragqueue>> = PfGlobal(TailqHead::new());

/// `pf_frnode_tree`.
pub static PF_FRNODE_TREE: PfGlobal<RbHead<PfFrnodeTree>> = PfGlobal(RbHead::new());

/// `pf_frent_pl`.
pub static PF_FRENT_PL: Pool = Pool::new();
/// `pf_frag_pl`.
pub static PF_FRAG_PL: Pool = Pool::new();
/// `pf_frnode_pl`.
pub static PF_FRNODE_PL: Pool = Pool::new();
/// `pf_state_scrub_pl`.
pub static PF_STATE_SCRUB_PL: Pool = Pool::new();

// SAFETY: Cells of integers and of a `Timeval`: the all-zero pattern is valid.
unsafe impl PfPoolItem for PfStateScrub {}

/// `pf_frag_mtx`: protects the fragment queues (`PF_FRAG_LOCK`).
pub static PF_FRAG_MTX: Mutex = Mutex::new(IPL_SOFTNET);

/// `pf_normalize_init`.
pub fn pf_normalize_init() {
    pf_pool_init::<PfFrent>(&PF_FRENT_PL, IPL_SOFTNET, 0, "pffrent");
    pf_pool_init::<PfFrnode>(&PF_FRNODE_PL, IPL_SOFTNET, 0, "pffrnode");
    pf_pool_init::<PfFragment>(&PF_FRAG_PL, IPL_SOFTNET, 0, "pffrag");
    pf_pool_init::<PfStateScrub>(&PF_STATE_SCRUB_PL, IPL_SOFTNET, 0, "pfstscr");

    pool_sethiwat(&PF_FRAG_PL, pffrag_frag_hiwat());
    let _ = pool_sethardlimit(&PF_FRENT_PL, pffrag_frent_hiwat());

    PF_FRAGQUEUE.init();

    // PF_FRAG_LOCK_INIT()
    mtx_init(&PF_FRAG_MTX, IPL_SOFTNET);
}

/// `pf_frnode_compare`.
pub fn pf_frnode_compare(a: &PfFrnode, b: &PfFrnode) -> Ordering {
    a.fn_proto
        .get()
        .cmp(&b.fn_proto.get())
        .then(a.fn_af.get().cmp(&b.fn_af.get()))
        .then(a.fn_direction.get().cmp(&b.fn_direction.get()))
        .then(a.fn_rdomain.get().cmp(&b.fn_rdomain.get()))
        .then_with(|| pf_addr_compare(&a.fn_src.get(), &b.fn_src.get(), a.fn_af.get()).cmp(&0))
        .then_with(|| pf_addr_compare(&a.fn_dst.get(), &b.fn_dst.get(), a.fn_af.get()).cmp(&0))
}

/// `pf_frag_compare`.
pub fn pf_frag_compare(a: &PfFragment, b: &PfFragment) -> Ordering {
    a.fr_id.get().cmp(&b.fr_id.get())
}

/// `pf_status.fragments--`.
fn fragments_dec() {
    PF_STATUS
        .fragments
        .set(PF_STATUS.fragments.get().wrapping_sub(1));
}

/// `pf_purge_expired_fragments`: frees the packets whose fragments have waited longer than
/// the `frag` timeout.
pub fn pf_purge_expired_fragments() {
    pf_assert_unlocked();

    let expire = (getuptime() - i64::from(PF_DEFAULT_RULE.timeout(PFTM_FRAG))) as i32;

    pf_frag_lock();
    while let Some(frag) = PF_FRAGQUEUE.last() {
        if frag.fr_timeout.get() > expire {
            break;
        }
        crate::dpfprintf!(
            LOG_NOTICE,
            "expiring {}({:p})",
            frag.fr_id.get() as i32,
            frag
        );
        pf_free_fragment(frag);
    }
    pf_frag_unlock();
}

/// `pf_flush_fragments`: tries to flush old fragments to make space for new ones.
fn pf_flush_fragments() {
    let goal = PF_STATUS.fragments.get().wrapping_mul(9) / 10;
    crate::dpfprintf!(
        LOG_NOTICE,
        "trying to free > {} frents",
        PF_STATUS.fragments.get().wrapping_sub(goal)
    );
    while goal < PF_STATUS.fragments.get() {
        let Some(frag) = PF_FRAGQUEUE.last() else {
            break;
        };
        pf_free_fragment(frag);
    }
}

/// `pf_free_fragment`: removes a fragment from the fragment queue, frees its fragment
/// entries, and frees the fragment itself.
fn pf_free_fragment(frag: &'static PfFragment) {
    let frnode = frag.node();
    // SAFETY: a queued fragment is in its node's tree (`pf_fillup_fragment`).
    unsafe { frnode.fn_tree.remove(frag) };
    kassert!(frnode.fn_fragments.get() >= 1);
    frnode.fn_fragments.set(frnode.fn_fragments.get() - 1);
    if frnode.fn_fragments.get() == 0 {
        kassert!(frnode.fn_tree.is_empty());
        // SAFETY: a node with fragments is in the node tree.
        unsafe { PF_FRNODE_TREE.remove(frnode) };
        pf_pool_put(&PF_FRNODE_PL, frnode);
    }
    // SAFETY: a fragment with a node is on the fragment queue.
    unsafe { PF_FRAGQUEUE.remove(frag) };

    // Free all fragment entries
    while let Some(frent) = frag.fr_queue.first() {
        // SAFETY: `frent` is the queue's first entry.
        unsafe { frag.fr_queue.remove(frent) };
        pf_inc(&PF_STATUS.ncounters[NCNT_FRAG_REMOVALS]);
        m_freem(frent.fe_m.get());
        pf_pool_put(&PF_FRENT_PL, frent);
        fragments_dec();
    }
    pf_pool_put(&PF_FRAG_PL, frag);
}

/// `pf_find_fragment`: the packet in reassembly with the IP id `id` of `key`'s node, moved
/// to the head of the fragment queue; stale packets are freed instead.
fn pf_find_fragment(key: &PfFrnode, id: u32) -> Option<&'static PfFragment> {
    let frnode = PF_FRNODE_TREE.find(key);
    pf_inc(&PF_STATUS.ncounters[NCNT_FRAG_SEARCH]);
    let frnode: &'static PfFrnode = frnode?;
    kassert!(frnode.fn_fragments.get() >= 1);
    let idkey = PfFragment::new();
    idkey.fr_id.set(id);
    let frag: &'static PfFragment = frnode.fn_tree.find(&idkey)?;
    // Limit the number of fragments we accept for each (proto,src,dst,af) combination (aka
    // pf_frnode), so we can deal better with a high rate of fragments. Problem analysis is in
    // RFC 4963.
    // Store the current generation for each pf_frnode in fn_gen and on lookup discard
    // 'stale' fragments (pf_fragment, based on the fr_gen member). Instead of adding another
    // button interpret the pf fragment timeout in multiples of 200 fragments. This way the
    // default of 60s means: pf_fragment objects older than 60*200 = 12,000 generations are
    // considered stale.
    let stale = PF_DEFAULT_RULE
        .timeout(PFTM_FRAG)
        .wrapping_mul(PF_FRAG_STALE);
    if frnode.fn_gen.get().wrapping_sub(frag.fr_gen.get()) >= stale {
        crate::dpfprintf!(
            LOG_NOTICE,
            "stale fragment {}({:p}), gen {}, num {}",
            frag.fr_id.get() as i32,
            frag,
            frag.fr_gen.get(),
            frnode.fn_fragments.get()
        );
        pf_free_fragment(frag);
        return None;
    }
    // SAFETY: the fragment is on the fragment queue; it goes back on its head.
    unsafe {
        PF_FRAGQUEUE.remove(frag);
        PF_FRAGQUEUE.insert_head(frag);
    }

    Some(frag)
}

/// `pf_create_fragment`: a fragment entry, flushing old fragments when the pool is empty.
fn pf_create_fragment(reason: &mut u16) -> Option<&'static PfFrent> {
    let frent = match pf_pool_get::<PfFrent>(&PF_FRENT_PL, PR_NOWAIT) {
        Some(f) => f,
        None => {
            pf_flush_fragments();
            match pf_pool_get::<PfFrent>(&PF_FRENT_PL, PR_NOWAIT) {
                Some(f) => f,
                None => {
                    reason_set(reason, PFRES_MEMORY);
                    return None;
                }
            }
        }
    };
    pf_inc(&PF_STATUS.fragments);

    Some(frent)
}

/// `fe_off + fe_len`, without the C's `u_int16_t` wrap (the C computes it as `int`).
fn fe_end(frent: &PfFrent) -> u32 {
    u32::from(frent.fe_off.get()) + u32::from(frent.fe_len.get())
}

/// `pf_frent_holes`: calculates the additional holes that were created in the fragment
/// queue by inserting this fragment. A fragment in the middle creates one more hole by
/// splitting. For each connected side, it loses one hole. Fragment entry must be in the
/// queue when calling this function.
fn pf_frent_holes(frag: &PfFragment, frent: &PfFrent) -> i32 {
    let prev = frag.fr_queue.prev(frent);
    let next = TailqHead::<PfFragq>::next(frent);
    let mut holes = 1;

    match prev {
        None => {
            if frent.fe_off.get() == 0 {
                holes -= 1;
            }
        }
        Some(prev) => {
            kassert!(frent.fe_off.get() != 0);
            if u32::from(frent.fe_off.get()) == fe_end(prev) {
                holes -= 1;
            }
        }
    }
    match next {
        None => {
            if frent.fe_mff.get() == 0 {
                holes -= 1;
            }
        }
        Some(next) => {
            kassert!(frent.fe_mff.get() != 0);
            if u32::from(next.fe_off.get()) == fe_end(frent) {
                holes -= 1;
            }
        }
    }
    holes
}

/// `pf_frent_index`: the entry point of the fragment's offset.
///
/// We have an array of 16 entry points to the queue. A full size 65535 octet IP packet can
/// have 8192 fragments. So the queue traversal length is at most 512 and at most 16 entry
/// points are checked. We need 128 additional bytes on a 64 bit architecture.
fn pf_frent_index(frent: &PfFrent) -> usize {
    usize::from(frent.fe_off.get()) / (0x10000 / PF_FRAG_ENTRY_POINTS)
}

/// `pf_frent_insert`: queues `frent` after `prev` (first when `None`), keeping the entry
/// points and the hole count. `ENOBUFS` when the entry point is full.
fn pf_frent_insert(
    frag: &'static PfFragment,
    frent: &'static PfFrent,
    prev: Option<&'static PfFrent>,
) -> Result<(), Errno> {
    // A packet has at most 65536 octets. With 16 entry points, each one spawns 4096 octets.
    // We limit these to 64 fragments each, which means on average every fragment must have
    // at least 64 octets.
    let index = pf_frent_index(frent);
    if frag.fr_entries[index].get() >= PF_FRAG_ENTRY_LIMIT {
        return Err(Errno::ENOBUFS);
    }
    frag.fr_entries[index].set(frag.fr_entries[index].get() + 1);

    match prev {
        None => {
            // SAFETY: `frent` is a fresh or just removed entry, in no queue.
            unsafe { frag.fr_queue.insert_head(frent) };
        }
        Some(prev) => {
            kassert!(fe_end(prev) <= u32::from(frent.fe_off.get()));
            // SAFETY: `prev` is in this queue; `frent` is in none.
            unsafe { frag.fr_queue.insert_after(prev, frent) };
        }
    }
    pf_inc(&PF_STATUS.ncounters[NCNT_FRAG_INSERT]);

    match frag.fr_firstoff[index].get() {
        None => {
            kassert!(prev.is_none_or(|p| pf_frent_index(p) < index));
            frag.fr_firstoff[index].set(Some(frent));
        }
        Some(first) => {
            if frent.fe_off.get() < first.fe_off.get() {
                kassert!(prev.is_none_or(|p| pf_frent_index(p) < index));
                frag.fr_firstoff[index].set(Some(frent));
            } else {
                kassert!(prev.is_some());
                kassert!(prev.is_some_and(|p| pf_frent_index(p) == index));
            }
        }
    }

    frag.fr_holes
        .set((i32::from(frag.fr_holes.get()) + pf_frent_holes(frag, frent)) as u16);

    Ok(())
}

/// `pf_frent_remove`: unqueues `frent`, keeping the entry points and the hole count.
fn pf_frent_remove(frag: &'static PfFragment, frent: &'static PfFrent) {
    // DIAGNOSTIC: the predecessor, for the assertions.
    let prev = frag.fr_queue.prev(frent);
    let next = TailqHead::<PfFragq>::next(frent);

    frag.fr_holes
        .set((i32::from(frag.fr_holes.get()) - pf_frent_holes(frag, frent)) as u16);

    let index = pf_frent_index(frent);
    kassert!(frag.fr_firstoff[index].get().is_some());
    let first_off = frag.fr_firstoff[index].get().map_or(0, |f| f.fe_off.get());
    if first_off == frent.fe_off.get() {
        match next {
            None => frag.fr_firstoff[index].set(None),
            Some(next) => {
                kassert!(fe_end(frent) <= u32::from(next.fe_off.get()));
                if pf_frent_index(next) == index {
                    frag.fr_firstoff[index].set(Some(next));
                } else {
                    frag.fr_firstoff[index].set(None);
                }
            }
        }
    } else {
        kassert!(first_off < frent.fe_off.get());
        kassert!(prev.is_some());
        kassert!(prev.is_some_and(|p| fe_end(p) <= u32::from(frent.fe_off.get())));
        kassert!(prev.is_some_and(|p| pf_frent_index(p) == index));
    }

    // SAFETY: `frent` is in this queue.
    unsafe { frag.fr_queue.remove(frent) };
    pf_inc(&PF_STATUS.ncounters[NCNT_FRAG_REMOVALS]);

    kassert!(frag.fr_entries[index].get() > 0);
    frag.fr_entries[index].set(frag.fr_entries[index].get().wrapping_sub(1));
}

/// `pf_frent_previous`: the queued fragment `frent` goes after (`None`: it goes first).
fn pf_frent_previous(frag: &'static PfFragment, frent: &PfFrent) -> Option<&'static PfFrent> {
    // If there are no fragments after frag, take the final one. Assume that the global queue
    // is not empty.
    let last = frag.fr_queue.last();
    kassert!(last.is_some());
    let last: &'static PfFrent = last?;
    if last.fe_off.get() <= frent.fe_off.get() {
        return Some(last);
    }
    // We want to find a fragment entry that is before frag, but still close to it. Find the
    // first fragment entry that is in the same entry point or in the first entry point after
    // that. As we have already checked that there are entries behind frag, this will
    // succeed.
    let mut prev: Option<&'static PfFrent> = None;
    for index in pf_frent_index(frent)..PF_FRAG_ENTRY_POINTS {
        prev = frag.fr_firstoff[index].get();
        if prev.is_some() {
            break;
        }
    }
    kassert!(prev.is_some());
    let mut prev: &'static PfFrent = prev?;
    // In prev we may have a fragment from the same entry point that is before frent, or one
    // that is just one position behind frent. In the latter case, we go back one step and
    // have the predecessor. There may be none if the new fragment will be the first one.
    if prev.fe_off.get() > frent.fe_off.get() {
        let p = frag.fr_queue.prev(prev)?;
        kassert!(p.fe_off.get() <= frent.fe_off.get());
        return Some(p);
    }
    // In prev is the first fragment of the entry point. The offset of frag is behind it.
    // Find the closest previous fragment.
    let mut next = TailqHead::<PfFragq>::next(prev);
    while let Some(n) = next {
        if n.fe_off.get() > frent.fe_off.get() {
            break;
        }
        prev = n;
        next = TailqHead::<PfFragq>::next(n);
    }
    Some(prev)
}

/// `pf_fillup_fragment`: queues `frent` in the packet of `key` and `id` (made when it is the
/// first fragment), trimming overlaps. `None` when the fragment was dropped (with the
/// reason); its entry is freed, its mbuf left to the caller.
fn pf_fillup_fragment(
    key: &PfFrnode,
    id: u32,
    frent: &'static PfFrent,
    reason: &mut u16,
) -> Option<&'static PfFragment> {
    let found: Option<&'static PfFragment>;

    'drop_fragment: {
        'bad_fragment: {
            'free_fragment: {
                'free_ipv6_fragment: {
                    // No empty fragments
                    if frent.fe_len.get() == 0 {
                        crate::dpfprintf!(LOG_NOTICE, "bad fragment: len 0");
                        break 'bad_fragment;
                    }

                    // All fragments are 8 byte aligned
                    if frent.fe_mff.get() != 0 && (frent.fe_len.get() & 0x7) != 0 {
                        crate::dpfprintf!(
                            LOG_NOTICE,
                            "bad fragment: mff and len {}",
                            frent.fe_len.get()
                        );
                        break 'bad_fragment;
                    }

                    // Respect maximum length, IP_MAXPACKET == IPV6_MAXPACKET
                    if fe_end(frent) as usize > IP_MAXPACKET {
                        crate::dpfprintf!(LOG_NOTICE, "bad fragment: max packet {}", fe_end(frent));
                        break 'bad_fragment;
                    }

                    if key.fn_af.get() == AF_INET {
                        crate::dpfprintf!(
                            LOG_INFO,
                            "reass frag {} @ {}-{}",
                            id as i32,
                            frent.fe_off.get(),
                            fe_end(frent)
                        );
                    } else {
                        crate::dpfprintf!(
                            LOG_INFO,
                            "reass frag {:#08x} @ {}-{}",
                            id,
                            frent.fe_off.get(),
                            fe_end(frent)
                        );
                    }

                    // Fully buffer all of the fragments in this fragment queue
                    found = pf_find_fragment(key, id);

                    // Create a new reassembly queue for this packet
                    let Some(frag) = found else {
                        let frag = match pf_pool_get::<PfFragment>(&PF_FRAG_PL, PR_NOWAIT) {
                            Some(f) => f,
                            None => {
                                pf_flush_fragments();
                                match pf_pool_get::<PfFragment>(&PF_FRAG_PL, PR_NOWAIT) {
                                    Some(f) => f,
                                    None => {
                                        reason_set(reason, PFRES_MEMORY);
                                        break 'drop_fragment;
                                    }
                                }
                            }
                        };
                        let frnode = match PF_FRNODE_TREE.find(key) {
                            Some(n) => n,
                            None => {
                                let n = match pf_pool_get::<PfFrnode>(&PF_FRNODE_PL, PR_NOWAIT) {
                                    Some(n) => n,
                                    None => {
                                        pf_flush_fragments();
                                        match pf_pool_get::<PfFrnode>(&PF_FRNODE_PL, PR_NOWAIT) {
                                            Some(n) => n,
                                            None => {
                                                reason_set(reason, PFRES_MEMORY);
                                                pf_pool_put(&PF_FRAG_PL, frag);
                                                break 'drop_fragment;
                                            }
                                        }
                                    }
                                };
                                // *frnode = *key;
                                n.fn_src.set(key.fn_src.get());
                                n.fn_dst.set(key.fn_dst.get());
                                n.fn_rdomain.set(key.fn_rdomain.get());
                                n.fn_af.set(key.fn_af.get());
                                n.fn_proto.set(key.fn_proto.get());
                                n.fn_direction.set(key.fn_direction.get());
                                n.fn_tree.init();
                                n.fn_fragments.set(0);
                                n.fn_gen.set(0);
                                n
                            }
                        };
                        for c in &frag.fr_firstoff {
                            c.set(None);
                        }
                        for c in &frag.fr_entries {
                            c.set(0);
                        }
                        frag.fr_queue.init();
                        frag.fr_id.set(id);
                        frag.fr_timeout.set(getuptime() as i32);
                        frag.fr_gen.set(frnode.fn_gen.get());
                        frnode.fn_gen.set(frnode.fn_gen.get().wrapping_add(1));
                        frag.fr_maxlen.set(frent.fe_len.get());
                        frag.fr_holes.set(1);
                        frag.fr_node.set(Some(frnode));
                        // RB_INSERT cannot fail as pf_find_fragment() found nothing
                        // SAFETY: a fresh pool item, in no tree.
                        let _ = unsafe { frnode.fn_tree.insert(frag) };
                        frnode.fn_fragments.set(frnode.fn_fragments.get() + 1);
                        if frnode.fn_fragments.get() == 1 {
                            // SAFETY: a node without fragments is in no tree.
                            let _ = unsafe { PF_FRNODE_TREE.insert(frnode) };
                        }
                        // SAFETY: a fresh fragment, on no queue.
                        unsafe { PF_FRAGQUEUE.insert_head(frag) };

                        // We do not have a previous fragment, cannot fail.
                        let _ = pf_frent_insert(frag, frent, None);

                        return Some(frag);
                    };

                    kassert!(!frag.fr_queue.is_empty());
                    kassert!(frag.fr_node.get().is_some());

                    // Remember maximum fragment len for refragmentation
                    if frent.fe_len.get() > frag.fr_maxlen.get() {
                        frag.fr_maxlen.set(frent.fe_len.get());
                    }

                    // Maximum data we have seen already
                    let Some(last) = frag.fr_queue.last() else {
                        break 'free_fragment;
                    };
                    let total = fe_end(last);

                    // Non terminal fragments must have more fragments flag
                    if fe_end(frent) < total && frent.fe_mff.get() == 0 {
                        break 'free_ipv6_fragment;
                    }

                    // Check if we saw the last fragment already
                    if last.fe_mff.get() == 0 {
                        if fe_end(frent) > total
                            || (fe_end(frent) == total && frent.fe_mff.get() != 0)
                        {
                            break 'free_ipv6_fragment;
                        }
                    } else if fe_end(frent) == total && frent.fe_mff.get() == 0 {
                        break 'free_ipv6_fragment;
                    }

                    // Find neighbors for newly inserted fragment
                    let prev = pf_frent_previous(frag, frent);
                    let mut after = match prev {
                        None => {
                            let a = frag.fr_queue.first();
                            kassert!(a.is_some());
                            a
                        }
                        Some(p) => TailqHead::<PfFragq>::next(p),
                    };

                    if let Some(p) = prev
                        && fe_end(p) > u32::from(frent.fe_off.get())
                    {
                        #[cfg(feature = "inet6")]
                        if frag.node().fn_af.get() == AF_INET6 {
                            break 'free_ipv6_fragment;
                        }

                        let precut = (fe_end(p) - u32::from(frent.fe_off.get())) as u16;
                        if precut >= frent.fe_len.get() {
                            crate::dpfprintf!(LOG_NOTICE, "new frag overlapped");
                            break 'drop_fragment;
                        }
                        crate::dpfprintf!(LOG_NOTICE, "frag head overlap {}", precut);
                        m_adj(frent.fe_m.get(), i32::from(precut));
                        frent.fe_off.set(frent.fe_off.get() + precut);
                        frent.fe_len.set(frent.fe_len.get() - precut);
                    }

                    while let Some(a) = after
                        && fe_end(frent) > u32::from(a.fe_off.get())
                    {
                        #[cfg(feature = "inet6")]
                        if frag.node().fn_af.get() == AF_INET6 {
                            break 'free_ipv6_fragment;
                        }

                        let aftercut = (fe_end(frent) - u32::from(a.fe_off.get())) as u16;
                        if aftercut < a.fe_len.get() {
                            crate::dpfprintf!(LOG_NOTICE, "frag tail overlap {}", aftercut);
                            m_adj(a.fe_m.get(), i32::from(aftercut));
                            // Fragment may switch queue as fe_off changes
                            pf_frent_remove(frag, a);
                            a.fe_off.set(a.fe_off.get() + aftercut);
                            a.fe_len.set(a.fe_len.get() - aftercut);
                            // Insert into correct queue
                            if pf_frent_insert(frag, a, prev).is_err() {
                                crate::dpfprintf!(LOG_WARNING, "fragment requeue limit exceeded");
                                m_freem(a.fe_m.get());
                                pf_pool_put(&PF_FRENT_PL, a);
                                fragments_dec();
                                // There is not way to recover
                                break 'free_fragment;
                            }
                            break;
                        }

                        // This fragment is completely overlapped, lose it
                        crate::dpfprintf!(LOG_NOTICE, "old frag overlapped");
                        let next = TailqHead::<PfFragq>::next(a);
                        pf_frent_remove(frag, a);
                        m_freem(a.fe_m.get());
                        pf_pool_put(&PF_FRENT_PL, a);
                        fragments_dec();
                        after = next;
                    }

                    // If part of the queue gets too long, there is not way to recover.
                    if pf_frent_insert(frag, frent, prev).is_err() {
                        crate::dpfprintf!(LOG_WARNING, "fragment queue limit exceeded");
                        break 'free_fragment;
                    }

                    return Some(frag);
                }
                // free_ipv6_fragment:
                if found.is_none_or(|f| f.node().fn_af.get() == AF_INET) {
                    break 'bad_fragment;
                }
                // RFC 5722, Errata 3089: When reassembling an IPv6 datagram, if one or more
                // its constituent fragments is determined to be an overlapping fragment, the
                // entire datagram (and any constituent fragments) MUST be silently discarded.
                crate::dpfprintf!(LOG_NOTICE, "flush overlapping fragments");
            }
            // free_fragment:
            if let Some(frag) = found {
                pf_free_fragment(frag);
            }
        }
        // bad_fragment:
        reason_set(reason, PFRES_FRAG);
    }
    // drop_fragment:
    pf_pool_put(&PF_FRENT_PL, frent);
    fragments_dec();
    None
}

/// The mbuf of a queued fragment entry, which every entry has.
fn frent_mbuf(frent: &PfFrent) -> &'static Mbuf {
    match frent.fe_m.get() {
        Some(m) => m,
        None => panic(format_args!("pf_frent without an mbuf")),
    }
}

/// `pf_join_fragment`: the packet of the complete `frag`, its fragments' mbufs joined; frees
/// the fragment.
fn pf_join_fragment(frag: &'static PfFragment) -> &'static Mbuf {
    let Some(frent) = frag.fr_queue.first() else {
        panic(format_args!("pf_join_fragment: empty fragment queue"));
    };
    // SAFETY: `frent` is the queue's first entry.
    unsafe { frag.fr_queue.remove(frent) };
    pf_inc(&PF_STATUS.ncounters[NCNT_FRAG_REMOVALS]);

    let m = frent_mbuf(frent);
    // Strip off any trailing bytes
    let keep = i32::from(frent.fe_hdrlen.get()) + i32::from(frent.fe_len.get());
    if keep < m.m_pkthdr().len.get() {
        m_adj(m, keep - m.m_pkthdr().len.get());
    }
    // Magic from ip_input
    let m2 = m.m_next().get();
    m.m_next().set(None);
    m_cat(m, m2);
    pf_pool_put(&PF_FRENT_PL, frent);
    fragments_dec();

    while let Some(frent) = frag.fr_queue.first() {
        // SAFETY: `frent` is the queue's first entry.
        unsafe { frag.fr_queue.remove(frent) };
        pf_inc(&PF_STATUS.ncounters[NCNT_FRAG_REMOVALS]);
        let m2 = frent_mbuf(frent);
        // Strip off ip header
        m_adj(m2, i32::from(frent.fe_hdrlen.get()));
        // Strip off any trailing bytes
        let len = i32::from(frent.fe_len.get());
        if len < m2.m_pkthdr().len.get() {
            m_adj(m2, len - m2.m_pkthdr().len.get());
        }
        pf_pool_put(&PF_FRENT_PL, frent);
        fragments_dec();
        m_removehdr(m2);
        m_cat(m, Some(m2));
    }

    // Remove from fragment queue
    pf_free_fragment(frag);

    m
}

/// `pf_reassemble`: queues the IPv4 fragment `*m0`. `*m0` becomes `None` while the packet is
/// incomplete and the reassembled packet when it is complete; `PF_DROP` (with the reason)
/// when the fragment or the packet is bad.
fn pf_reassemble(m0: &mut Option<&'static Mbuf>, dir: u8, rdomain: u16, reason: &mut u16) -> u8 {
    let Some(m) = *m0 else {
        panic(format_args!("pf_reassemble: no packet"));
    };
    let ip = mtod_ip(m);

    // Get an entry for the fragment queue
    let Some(frent) = pf_create_fragment(reason) else {
        return PF_DROP;
    };

    let hl = u16::from(ip.ip_hl()) << 2;
    frent.fe_m.set(Some(m));
    frent.fe_hdrlen.set(hl);
    frent.fe_extoff.set(0);
    frent.fe_len.set(u16::from_be(ip.ip_len).wrapping_sub(hl));
    frent
        .fe_off
        .set((u16::from_be(ip.ip_off) & IP_OFFMASK) << 3);
    frent.fe_mff.set(u16::from_be(ip.ip_off) & IP_MF);

    let key = PfFrnode::new();
    key.fn_src.set(PfAddr::from_v4(ip.ip_src));
    key.fn_dst.set(PfAddr::from_v4(ip.ip_dst));
    key.fn_af.set(AF_INET);
    key.fn_proto.set(ip.ip_p);
    key.fn_direction.set(dir);
    key.fn_rdomain.set(rdomain);

    let Some(frag) = pf_fillup_fragment(&key, u32::from(ip.ip_id), frent, reason) else {
        return PF_DROP;
    };

    // The mbuf is part of the fragment entry, no direct free or access
    *m0 = None;

    if frag.fr_holes.get() != 0 {
        crate::dpfprintf!(
            LOG_DEBUG,
            "frag {}, holes {}",
            frag.fr_id.get() as i32,
            frag.fr_holes.get()
        );
        return PF_PASS; // drop because *m0 is NULL, no error
    }

    // We have all the data
    let first = frag.fr_queue.first();
    kassert!(first.is_some());
    let Some(frent) = first else {
        return PF_DROP;
    };
    let total = frag.fr_queue.last().map_or(0, fe_end);
    let hdrlen = u32::from(frent.fe_hdrlen.get());
    let m = pf_join_fragment(frag);
    *m0 = Some(m);
    m_calchdrlen(m);

    let mut ip = mtod_ip(m);
    ip.ip_len = ((hdrlen + total) as u16).to_be();
    // The C clears the host-order flags in the network-order field (`ip_off &=
    // ~(IP_MF|IP_OFFMASK)`); pf_normalize_ip's no_fragment step then keeps only IP_DF.
    ip.ip_off &= !(IP_MF | IP_OFFMASK);
    mtod_ip_store(m, &ip);

    if (hdrlen + total) as usize > IP_MAXPACKET {
        crate::dpfprintf!(LOG_NOTICE, "drop: too big: {}", total);
        ip.ip_len = 0;
        mtod_ip_store(m, &ip);
        reason_set(reason, PFRES_SHORT);
        // PF_DROP requires a valid mbuf *m0 in pf_test()
        return PF_DROP;
    }

    crate::dpfprintf!(LOG_INFO, "complete: {:p}({})", m, u16::from_be(ip.ip_len));
    PF_PASS
}

/// `pf_reassemble6`: queues an IPv6 fragment (`fraghdr`, the fragmentable part starting at
/// `hdrlen`, the last extension header before it at `extoff`). When the datagram is complete
/// `*m0` is the reassembled packet, without its fragment header, with the next header
/// restored and a `PACKET_TAG_PF_REASSEMBLED` tag for `pf_refragment6`; `None` while it is
/// incomplete. `PF_DROP` leaves a valid `*m0` for the caller to free.
#[cfg(feature = "inet6")]
#[allow(clippy::too_many_arguments)]
fn pf_reassemble6(
    m0: &mut Option<&'static Mbuf>,
    fraghdr: &Ip6Frag,
    hdrlen: u16,
    extoff: u16,
    dir: u8,
    rdomain: u16,
    reason: &mut u16,
) -> u8 {
    let Some(m) = *m0 else {
        panic(format_args!("pf_reassemble6: no packet"));
    };
    let ip6 = mtod_ip6(m);

    // Get an entry for the fragment queue
    let Some(frent) = pf_create_fragment(reason) else {
        return PF_DROP;
    };

    frent.fe_m.set(Some(m));
    frent.fe_hdrlen.set(hdrlen);
    frent.fe_extoff.set(extoff);
    frent.fe_len.set(
        (size_of::<Ip6Hdr>() as u16)
            .wrapping_add(u16::from_be(ip6.ip6_plen))
            .wrapping_sub(hdrlen),
    );
    frent
        .fe_off
        .set(u16::from_be(fraghdr.ip6f_offlg & IP6F_OFF_MASK));
    frent.fe_mff.set(fraghdr.ip6f_offlg & IP6F_MORE_FRAG);

    let key = PfFrnode::new();
    key.fn_src.set(PfAddr::from_v6(ip6.ip6_src));
    key.fn_dst.set(PfAddr::from_v6(ip6.ip6_dst));
    key.fn_af.set(AF_INET6);
    // Only the first fragment's protocol is relevant
    key.fn_proto.set(0);
    key.fn_direction.set(dir);
    key.fn_rdomain.set(rdomain);

    let Some(frag) = pf_fillup_fragment(&key, fraghdr.ip6f_ident, frent, reason) else {
        return PF_DROP;
    };

    // The mbuf is part of the fragment entry, no direct free or access
    *m0 = None;

    if frag.fr_holes.get() != 0 {
        crate::dpfprintf!(
            LOG_DEBUG,
            "frag {:#08x}, holes {}",
            frag.fr_id.get(),
            frag.fr_holes.get()
        );
        return PF_PASS; // drop because *m0 is NULL, no error
    }

    // We have all the data
    let first = frag.fr_queue.first();
    kassert!(first.is_some());
    let Some(frent) = first else {
        return PF_DROP;
    };
    let extoff = u32::from(frent.fe_extoff.get());
    let maxlen = frag.fr_maxlen.get();
    let total = frag.fr_queue.last().map_or(0, fe_end);
    let hdrlen = u32::from(frent.fe_hdrlen.get()) - size_of::<Ip6Frag>() as u32;
    let m = pf_join_fragment(frag);
    *m0 = Some(m);

    // Take protocol from first fragment header
    let Some((n, off)) = m_getptr(m, (hdrlen as usize + offset_of!(Ip6Frag, ip6f_nxt)) as i32)
    else {
        panic(format_args!("pf_reassemble6: short frag mbuf chain"));
    };
    // SAFETY: m_getptr found the byte at `off` within `n`'s data.
    let proto = unsafe { *mtod::<u8>(n).add(off as usize) };

    'fail: {
        // Delete frag6 header
        if frag6_deletefraghdr(m, hdrlen as i32).is_err() {
            break 'fail;
        }

        m_calchdrlen(m);

        let Some(mtag) = m_tag_get(
            PACKET_TAG_PF_REASSEMBLED,
            size_of::<PfFragmentTag>() as i32,
            M_NOWAIT,
        ) else {
            break 'fail;
        };
        let ftag = PfFragmentTag {
            ft_hdrlen: hdrlen as u16,
            ft_extoff: extoff as u16,
            ft_maxlen: maxlen,
        };
        // SAFETY: the tag's data has room for the `PfFragmentTag` it was made for; it may
        // be unaligned.
        unsafe { ptr::write_unaligned(mtag.data().cast::<PfFragmentTag>(), ftag) };
        m_tag_prepend(m, mtag);

        let mut ip6 = mtod_ip6(m);
        let plen = hdrlen - size_of::<Ip6Hdr>() as u32 + total;
        ip6.ip6_plen = (plen as u16).to_be();
        if extoff != 0 {
            // Write protocol into next field of last extension header
            let Some((n, off)) =
                m_getptr(m, (extoff as usize + offset_of!(Ip6Ext, ip6e_nxt)) as i32)
            else {
                panic(format_args!("pf_reassemble6: short ext mbuf chain"));
            };
            // SAFETY: m_getptr found the byte at `off` within `n`'s data.
            unsafe { *mtod::<u8>(n).add(off as usize) = proto };
        } else {
            ip6.ip6_nxt = proto;
        }
        mtod_ip6_store(m, &ip6);

        if plen as usize > IPV6_MAXPACKET {
            crate::dpfprintf!(LOG_NOTICE, "drop: too big: {}", total);
            ip6.ip6_plen = 0;
            mtod_ip6_store(m, &ip6);
            reason_set(reason, PFRES_SHORT);
            // PF_DROP requires a valid mbuf *m0 in pf_test6()
            return PF_DROP;
        }

        crate::dpfprintf!(
            LOG_INFO,
            "complete: {:p}({})",
            m,
            u16::from_be(ip6.ip6_plen)
        );
        return PF_PASS;
    }
    // fail:
    reason_set(reason, PFRES_MEMORY);
    // PF_DROP requires a valid mbuf *m0 in pf_test6(), will free later
    PF_DROP
}

/// `pf_refragment6`: splits a packet `pf_reassemble6` reassembled (its tag `mtag`) back into
/// fragments of the original size and sends them: forwarded when `ifp` is `None`, else out
/// of `ifp` towards `dst` by `rt`. `*m0` is consumed.
#[cfg(feature = "inet6")]
pub fn pf_refragment6(
    m0: &mut Option<&'static Mbuf>,
    mtag: &'static MTag,
    dst: Option<&SockaddrIn6>,
    ifp: Option<&'static Ifnet>,
    rt: Option<&'static Rtentry>,
) -> u8 {
    use crate::netinet6::ip6_var::{
        IPV6_FORWARDING, IPV6_FORWARDING_IPSEC, Ip6statCounters, ip6stat_inc,
    };

    let Some(m) = *m0 else {
        panic(format_args!("pf_refragment6: no packet"));
    };
    // SAFETY: a PACKET_TAG_PF_REASSEMBLED tag carries a `PfFragmentTag` (pf_reassemble6
    // made it); it may be unaligned.
    let ftag = unsafe { ptr::read_unaligned(mtag.data().cast::<PfFragmentTag>()) };
    let hdrlen = ftag.ft_hdrlen;
    let extoff = ftag.ft_extoff;
    let maxlen = ftag.ft_maxlen;
    // SAFETY: the tag was found on `m`'s list and is not used after this.
    unsafe { m_tag_delete(m, mtag) };

    // Checksum must be calculated for the whole packet
    in6_proto_cksum_out(m, None);

    let proto = if extoff != 0 {
        // Use protocol from next field of last extension header
        let Some((n, off)) = m_getptr(
            m,
            (usize::from(extoff) + offset_of!(Ip6Ext, ip6e_nxt)) as i32,
        ) else {
            panic(format_args!("pf_refragment6: short ext mbuf chain"));
        };
        // SAFETY: m_getptr found the byte at `off` within `n`'s data.
        unsafe {
            let p = mtod::<u8>(n).add(off as usize);
            let proto = *p;
            *p = IPPROTO_FRAGMENT as u8;
            proto
        }
    } else {
        let mut hdr = mtod_ip6(m);
        let proto = hdr.ip6_nxt;
        hdr.ip6_nxt = IPPROTO_FRAGMENT as u8;
        mtod_ip6_store(m, &hdr);
        proto
    };

    // Maxlen may be less than 8 iff there was only a single fragment. As it was fragmented
    // before, add a fragment header also for a single fragment. If total or maxlen is less
    // than 8, ip6_fragment() will return EMSGSIZE and we drop the packet.
    let mtu = u64::from(hdrlen) + size_of::<Ip6Frag>() as u64 + u64::from(maxlen);
    let ml = MbufList::new();
    let error = ip6_fragment(m, &ml, i32::from(hdrlen), proto, mtu);
    *m0 = None; // ip6_fragment() has consumed original packet.
    if let Err(e) = error {
        crate::dpfprintf!(LOG_NOTICE, "refragment error {}", e as i32);
        return PF_DROP;
    }

    while let Some(m) = ml_dequeue(&ml) {
        let pf = &m.m_pkthdr().pf;
        pf.flags.set(pf.flags.get() | PF_TAG_REFRAGMENTED);
        match ifp {
            None => {
                let flags = match IP6_FORWARDING.load(core::sync::atomic::Ordering::Relaxed) {
                    2 => IPV6_FORWARDING_IPSEC | IPV6_FORWARDING,
                    1 => IPV6_FORWARDING,
                    _ => {
                        ip6stat_inc(Ip6statCounters::Ip6sCantforward);
                        m_freem(m);
                        ml_purge(&ml);
                        return PF_DROP;
                    }
                };
                ip6_forward(m, None, flags);
            }
            Some(ifp) if m.m_pkthdr().len.get() as u64 <= u64::from(ifp.if_mtu.get()) => {
                let dst: *const Sockaddr = match dst {
                    Some(d) => sin6tosa_const(d),
                    None => ptr::null(),
                };
                if let Some(output) = ifp.if_output.get() {
                    // SAFETY: `dst` is the caller's sockaddr_in6 (pf_route6's local), `rt`
                    // its route; the hook takes the packet.
                    let _ = unsafe { output(ifp, m, dst, rt) };
                } else {
                    m_freem(m);
                }
            }
            Some(ifp) => {
                icmp6_error(m, ICMP6_PACKET_TOO_BIG, 0, ifp.if_mtu.get() as i32);
            }
        }
    }

    PF_PASS
}

/// The packet of a descriptor, which pf's normalizer is only called with.
fn pd_mbuf(pd: &PfPdesc) -> &'static Mbuf {
    match pd.m {
        Some(m) => m,
        None => panic(format_args!("pf_norm: packet descriptor without a packet")),
    }
}

/// `pf_normalize_ip`: drops bad IPv4 fragments and reassembles the others when `reass` is
/// on (`pd->m` becomes `None` while the packet is incomplete); clears the fragment bits but
/// `IP_DF`.
pub fn pf_normalize_ip(pd: &mut PfPdesc, reason: &mut u16) -> u8 {
    let mut m = pd_mbuf(pd);
    let mut h = mtod_ip(m);
    let fragoff = (u16::from_be(h.ip_off) & IP_OFFMASK) << 3;
    let mff = u16::from_be(h.ip_off) & IP_MF;

    'no_fragment: {
        if fragoff == 0 && mff == 0 {
            break 'no_fragment;
        }

        // Clear IP_DF if we're in no-df mode
        if PF_STATUS.reass.get() & PF_REASS_NODF != 0 && h.ip_off & IP_DF.to_be() != 0 {
            h.ip_off &= (!IP_DF).to_be();
            mtod_ip_store(m, &h);
        }

        // We're dealing with a fragment now. Don't allow fragments with IP_DF to enter the
        // cache. If the flag was cleared by no-df above, fine. Otherwise drop it.
        if h.ip_off & IP_DF.to_be() != 0 {
            crate::dpfprintf!(LOG_NOTICE, "bad fragment: IP_DF");
            reason_set(reason, PFRES_FRAG);
            return PF_DROP;
        }

        if PF_STATUS.reass.get() == 0 {
            return PF_PASS; // no reassembly
        }

        // Returns PF_DROP or m is NULL or completely reassembled mbuf
        pf_frag_lock();
        if pf_reassemble(&mut pd.m, pd.dir, pd.rdomain, reason) != PF_PASS {
            pf_frag_unlock();
            return PF_DROP;
        }
        pf_frag_unlock();
        let Some(nm) = pd.m else {
            return PF_PASS; // packet has been reassembled, no error
        };

        m = nm;
        h = mtod_ip(m);
    }

    // no_fragment:
    // At this point, only IP_DF is allowed in ip_off
    if h.ip_off & !IP_DF.to_be() != 0 {
        h.ip_off &= IP_DF.to_be();
        mtod_ip_store(m, &h);
    }

    PF_PASS
}

/// `pf_normalize_ip6`: reassembles an IPv6 fragment (its fragment header at `pd->fragoff`)
/// when `reass` is on; `pd->m` becomes `None` while the datagram is incomplete.
#[cfg(feature = "inet6")]
pub fn pf_normalize_ip6(pd: &mut PfPdesc, reason: &mut u16) -> u8 {
    if pd.fragoff == 0 {
        return PF_PASS; // no_fragment
    }

    let mut b = [0u8; size_of::<Ip6Frag>()];
    if !pf_pull_hdr(
        pd_mbuf(pd),
        pd.fragoff as i32,
        &mut b,
        Some(reason),
        AF_INET6,
    ) {
        return PF_DROP;
    }
    // SAFETY: `Ip6Frag` is 8 bytes of integers (`#[repr(C)]`, no padding).
    let frag = unsafe { ptr::read_unaligned(b.as_ptr().cast::<Ip6Frag>()) };

    if PF_STATUS.reass.get() == 0 {
        return PF_PASS; // no reassembly
    }

    // Returns PF_DROP or m is NULL or completely reassembled mbuf
    pf_frag_lock();
    if pf_reassemble6(
        &mut pd.m,
        &frag,
        (pd.fragoff as usize + size_of::<Ip6Frag>()) as u16,
        pd.extoff as u16,
        pd.dir,
        pd.rdomain,
        reason,
    ) != PF_PASS
    {
        pf_frag_unlock();
        return PF_DROP;
    }
    pf_frag_unlock();
    // pd->m is NULL (packet has been reassembled, no error) or the whole datagram.
    PF_PASS
}

/// `pf_state_scrub_get`: a zeroed scrub, `None` when the pool is empty.
pub fn pf_state_scrub_get() -> Option<&'static PfStateScrub> {
    pf_pool_get::<PfStateScrub>(&PF_STATE_SCRUB_PL, PR_NOWAIT | PR_ZERO)
}

/// `pf_state_scrub_put`.
pub fn pf_state_scrub_put(scrub: &'static PfStateScrub) {
    pf_pool_put(&PF_STATE_SCRUB_PL, scrub);
}

/// `pf_normalize_tcp_alloc`: gives the peer a scrub; `ENOMEM` when none is left.
pub fn pf_normalize_tcp_alloc(src: &PfStatePeer) -> Result<(), Errno> {
    src.scrub.set(pf_state_scrub_get());
    if src.scrub.get().is_none() {
        return Err(Errno::ENOMEM);
    }

    Ok(())
}

/// The offset of `th_off`/`th_x2` in the TCP header (the C's `&th->th_ack + 1`).
const TH_OFF_OFF: usize = offset_of!(Tcphdr, th_x2_off);
/// The offset of `th_flags`.
const TH_FLAGS_OFF: usize = offset_of!(Tcphdr, th_flags);
/// The offset of `th_urp`.
const TH_URP_OFF: usize = offset_of!(Tcphdr, th_urp);

/// `m_copyback(pd->m, pd->off, sizeof(struct tcphdr), &pd->hdr.tcp, M_NOWAIT)`.
fn copyback_tcphdr(pd: &mut PfPdesc) {
    let mut th = [0u8; size_of::<Tcphdr>()];
    th.copy_from_slice(&pd.hdr_bytes()[..size_of::<Tcphdr>()]);
    let _ = m_copyback(pd.m, pd.off as i32, &th, M_NOWAIT);
}

/// `pf_normalize_tcp`: drops TCP segments with illegal flag combinations, clears `FIN` on
/// a `SYN`, the reserved bits and an urgent pointer without `URG`.
pub fn pf_normalize_tcp(pd: &mut PfPdesc) -> u8 {
    let th = *pd.tcp();
    let mut reason: u16 = 0;
    let mut rewrite = false;

    let mut flags = th.th_flags;
    'tcp_drop: {
        if flags & TH_SYN != 0 {
            // Illegal packet
            if flags & TH_RST != 0 {
                break 'tcp_drop;
            }

            if flags & TH_FIN != 0 {
                // XXX why clear instead of drop?
                flags &= !TH_FIN;
            }
        } else {
            // Illegal packet
            if flags & (TH_ACK | TH_RST) == 0 {
                break 'tcp_drop;
            }
        }

        if flags & TH_ACK == 0 {
            // These flags are only valid if ACK is set
            if flags & (TH_FIN | TH_PUSH | TH_URG) != 0 {
                break 'tcp_drop;
            }
        }

        // If flags changed, or reserved data set, then adjust
        if flags != th.th_flags || th.th_x2() != 0 {
            // hack: set 4-bit th_x2 = 0
            pf_patch_8(pd, PfLoc::Hdr(TH_OFF_OFF), th.th_off() << 4, PF_HI);

            pf_patch_8(pd, PfLoc::Hdr(TH_FLAGS_OFF), flags, PF_LO);
            rewrite = true;
        }

        // Remove urgent pointer, if TH_URG is not set
        if flags & TH_URG == 0 && th.th_urp != 0 {
            pf_patch_16(pd, PfLoc::Hdr(TH_URP_OFF), 0);
            rewrite = true;
        }

        // copy back packet headers if we sanitized
        if rewrite {
            copyback_tcphdr(pd);
        }

        return PF_PASS;
    }

    // tcp_drop:
    reason_set(&mut reason, PFRES_NORM);
    PF_DROP
}

/// The TCP options of the segment (`olen` bytes after the header), when the segment has at
/// least `min` bytes of them and they can be pulled.
fn pull_tcpopts(pd: &PfPdesc, opts: &mut [u8; MAX_TCPOPTLEN], min: u8) -> Option<usize> {
    let olen = (isize::from(pd.tcp().th_off()) << 2) - size_of::<Tcphdr>() as isize;
    if olen < isize::from(min) {
        return None;
    }
    let olen = olen as usize;
    let m = pd.m?;
    if !pf_pull_hdr(
        m,
        (pd.off as usize + size_of::<Tcphdr>()) as i32,
        &mut opts[..olen],
        None,
        pd.af,
    ) {
        return None;
    }
    Some(olen)
}

/// `pf_normalize_tcp_init`: the scrub of the connection's first peer: its TTL and, on a
/// `SYN` with timestamps, the timestamp modulation.
pub fn pf_normalize_tcp_init(pd: &mut PfPdesc, src: &PfStatePeer) -> Result<(), Errno> {
    kassert!(src.scrub.get().is_none());

    pf_normalize_tcp_alloc(src)?;
    let Some(scrub) = src.scrub.get() else {
        return Err(Errno::ENOMEM);
    };

    if pd.af == AF_INET {
        let h = mtod_ip(pd_mbuf(pd));
        scrub.pfss_ttl.set(h.ip_ttl);
    } else {
        #[cfg(feature = "inet6")]
        if pd.af == AF_INET6 {
            let h = mtod_ip6(pd_mbuf(pd));
            scrub.pfss_ttl.set(h.ip6_hlim);
        } else {
            unhandled_af(i32::from(pd.af));
        }
        #[cfg(not(feature = "inet6"))]
        unhandled_af(i32::from(pd.af));
    }

    // All normalizations below are only begun if we see the start of the connections. They
    // must all set an enabled bit in pfss_flags
    if pd.tcp().th_flags & TH_SYN == 0 {
        return Ok(());
    }

    let mut opts = [0u8; MAX_TCPOPTLEN];
    let Some(olen) = pull_tcpopts(pd, &mut opts, TCPOLEN_TIMESTAMP) else {
        return Ok(());
    };
    let opts = &opts[..olen];

    let mut opt = 0;
    while let Some(o) = pf_find_tcpopt(opts, opt, TCPOPT_TIMESTAMP, TCPOLEN_TIMESTAMP) {
        scrub
            .pfss_flags
            .set(scrub.pfss_flags.get() | PFSS_TIMESTAMP);
        scrub.pfss_ts_mod.set(arc4random());
        // note PFSS_PAWS not set yet
        let tsval = u32::from_ne_bytes([opts[o + 2], opts[o + 3], opts[o + 4], opts[o + 5]]);
        let tsecr = u32::from_ne_bytes([opts[o + 6], opts[o + 7], opts[o + 8], opts[o + 9]]);
        scrub.pfss_tsval0.set(u32::from_be(tsval));
        scrub.pfss_tsval.set(u32::from_be(tsval));
        scrub.pfss_tsecr.set(u32::from_be(tsecr));
        scrub.pfss_last.set(getmicrouptime());

        opt = o + usize::from(opts[o + 1]);
    }

    Ok(())
}

/// `pf_normalize_tcp_cleanup`: frees the state's scrubs.
pub fn pf_normalize_tcp_cleanup(state: &'static PfState) {
    if let Some(s) = state.src.scrub.get() {
        pf_pool_put(&PF_STATE_SCRUB_PL, s);
    }
    if let Some(s) = state.dst.scrub.get() {
        pf_pool_put(&PF_STATE_SCRUB_PL, s);
    }

    // Someday... flush the TCP segment reassembly descriptors.
}

/// `TS_MAX_IDLE`: the fastest allowed timestamp clock is 1ms. That turns out to be about 24
/// days before it wraps.
const TS_MAX_IDLE: i64 = 24 * 24 * 60 * 60;
/// `TS_MAX_CONN`: XXX remove when better tsecr check.
const TS_MAX_CONN: i64 = 12 * 24 * 60 * 60;
/// `TS_MAXFREQ`: RFC max TS freq of 1Khz + 10% skew.
const TS_MAXFREQ: u32 = 1100;
/// `TS_MICROSECS`: microseconds per second.
const TS_MICROSECS: u32 = 1_000_000;

/// The peer's scrub flags, 0 without a scrub.
fn scrub_flags(p: &PfStatePeer) -> u16 {
    p.scrub.get().map_or(0, |s| s.pfss_flags.get())
}

/// `pf_normalize_tcp_stateful`: the per-segment normalization of a scrubbed connection:
/// the minimum TTL, the timestamp modulation (`writeback` set when the options changed) and
/// the PAWS checks. Nonzero (`PF_DROP`, with the reason) drops the segment.
pub fn pf_normalize_tcp_stateful(
    pd: &mut PfPdesc,
    reason: &mut u16,
    state: &'static PfState,
    src: &PfStatePeer,
    dst: &PfStatePeer,
    writeback: &mut i32,
) -> i32 {
    let th = *pd.tcp();
    let mut tsval: u32 = 0;
    let mut tsecr: u32 = 0;
    let mut copyback = false;
    let mut got_ts = false;

    kassert!(src.scrub.get().is_some() || dst.scrub.get().is_some());

    // Enforce the minimum TTL seen for this connection. Negate a common technique to evade
    // an intrusion detection system and confuse firewall state code.
    if pd.af == AF_INET {
        if let Some(scrub) = src.scrub.get() {
            let m = pd_mbuf(pd);
            let mut h = mtod_ip(m);
            if h.ip_ttl > scrub.pfss_ttl.get() {
                scrub.pfss_ttl.set(h.ip_ttl);
            }
            h.ip_ttl = scrub.pfss_ttl.get();
            mtod_ip_store(m, &h);
        }
    } else {
        #[cfg(feature = "inet6")]
        if pd.af == AF_INET6 {
            if let Some(scrub) = src.scrub.get() {
                let m = pd_mbuf(pd);
                let mut h = mtod_ip6(m);
                if h.ip6_hlim > scrub.pfss_ttl.get() {
                    scrub.pfss_ttl.set(h.ip6_hlim);
                }
                h.ip6_hlim = scrub.pfss_ttl.get();
                mtod_ip6_store(m, &h);
            }
        } else {
            unhandled_af(i32::from(pd.af));
        }
        #[cfg(not(feature = "inet6"))]
        unhandled_af(i32::from(pd.af));
    }

    let mut opts = [0u8; MAX_TCPOPTLEN];
    if (scrub_flags(src) & PFSS_TIMESTAMP != 0 || scrub_flags(dst) & PFSS_TIMESTAMP != 0)
        && let Some(olen) = pull_tcpopts(pd, &mut opts, TCPOLEN_TIMESTAMP)
    {
        // Modulate the timestamps. Can be used for NAT detection, OS uptime determination or
        // reboot detection.
        let mut opt = 0;
        while let Some(o) = pf_find_tcpopt(&opts[..olen], opt, TCPOPT_TIMESTAMP, TCPOLEN_TIMESTAMP)
        {
            let ts = o + 2;
            let tsr = o + 6;

            if got_ts {
                // Huh? Multiple timestamps!?
                if crate::net::pf::pf_debug(LOG_NOTICE) {
                    log(
                        LOG_NOTICE,
                        format_args!("pf: pf_normalize_tcp_stateful: multiple TS??"),
                    );
                    pf_print_state(state);
                    addlog(format_args!("\n"));
                }
                reason_set(reason, PFRES_TS);
                return i32::from(PF_DROP);
            }

            tsval = u32::from_ne_bytes([opts[ts], opts[ts + 1], opts[ts + 2], opts[ts + 3]]);
            tsecr = u32::from_ne_bytes([opts[tsr], opts[tsr + 1], opts[tsr + 2], opts[tsr + 3]]);

            // modulate TS
            if tsval != 0
                && let Some(s) = src.scrub.get()
                && s.pfss_flags.get() & PFSS_TIMESTAMP != 0
            {
                // tsval used further on
                tsval = u32::from_be(tsval);
                // SAFETY: `opts` is a local array that outlives the patch and is not
                // otherwise accessed while it runs; `ts..ts + 4` is within it (the option
                // has TCPOLEN_TIMESTAMP bytes).
                let loc = unsafe { PfLoc::local(opts.as_mut_ptr().add(ts)) };
                pf_patch_32_unaligned(
                    pd,
                    loc,
                    tsval.wrapping_add(s.pfss_ts_mod.get()).to_be(),
                    pf_algnmnt(ts),
                );
                copyback = true;
            }

            // modulate TS reply if any (!0)
            if tsecr != 0
                && let Some(d) = dst.scrub.get()
                && d.pfss_flags.get() & PFSS_TIMESTAMP != 0
            {
                // tsecr used further on
                tsecr = u32::from_be(tsecr).wrapping_sub(d.pfss_ts_mod.get());
                // SAFETY: as above, for `tsr..tsr + 4`.
                let loc = unsafe { PfLoc::local(opts.as_mut_ptr().add(tsr)) };
                pf_patch_32_unaligned(pd, loc, tsecr.to_be(), pf_algnmnt(tsr));
                copyback = true;
            }

            got_ts = true;
            opt = o + usize::from(opts[o + 1]);
        }

        if copyback {
            // Copyback the options, caller copies back header
            *writeback = 1;
            let _ = m_copyback(
                pd.m,
                (pd.off as usize + size_of::<Tcphdr>()) as i32,
                &opts[..olen],
                M_NOWAIT,
            );
        }
    }

    // Must invalidate PAWS checks on connections idle for too long. The fastest allowed
    // timestamp clock is 1ms. That turns out to be about 24 days before it wraps. XXX Right
    // now our lowerbound TS echo check only works for the first 12 days of a connection when
    // the TS has exhausted half its 32bit space
    let uptime = getmicrouptime();
    if let Some(s) = src.scrub.get()
        && s.pfss_flags.get() & PFSS_PAWS != 0
        && (uptime.tv_sec - s.pfss_last.get().tv_sec > TS_MAX_IDLE
            || getuptime() - i64::from(state.creation.get()) > TS_MAX_CONN)
    {
        if crate::net::pf::pf_debug(LOG_NOTICE) {
            log(LOG_NOTICE, format_args!("pf: src idled out of PAWS "));
            pf_print_state(state);
            addlog(format_args!("\n"));
        }
        s.pfss_flags
            .set((s.pfss_flags.get() & !PFSS_PAWS) | PFSS_PAWS_IDLED);
    }
    if let Some(d) = dst.scrub.get()
        && d.pfss_flags.get() & PFSS_PAWS != 0
        && uptime.tv_sec - d.pfss_last.get().tv_sec > TS_MAX_IDLE
    {
        if crate::net::pf::pf_debug(LOG_NOTICE) {
            log(LOG_NOTICE, format_args!("pf: dst idled out of PAWS "));
            pf_print_state(state);
            addlog(format_args!("\n"));
        }
        d.pfss_flags
            .set((d.pfss_flags.get() & !PFSS_PAWS) | PFSS_PAWS_IDLED);
    }

    if got_ts
        && let (Some(ss), Some(ds)) = (src.scrub.get(), dst.scrub.get())
        && ss.pfss_flags.get() & PFSS_PAWS != 0
        && ds.pfss_flags.get() & PFSS_PAWS != 0
    {
        // Validate that the timestamps are "in-window". RFC1323 describes TCP Timestamp
        // options that allow measurement of RTT (round trip time) and PAWS (protection
        // against wrapped sequence numbers). PAWS gives us a set of rules for rejecting
        // packets on long fat pipes (packets that were somehow delayed in transit longer than
        // the time it took to send the full TCP sequence space of 4Gb). We can use these
        // rules and infer a few others that will let us treat the 32bit timestamp and the
        // 32bit echoed timestamp as sequence numbers to prevent a blind attacker from
        // inserting packets into a connection.
        //
        // RFC1323 tells us:
        //  - The timestamp on this packet must be greater than or equal to the last value
        //    echoed by the other endpoint. The RFC says those will be discarded since it is
        //    a dup that has already been acked. This gives us a lowerbound on the timestamp.
        //        timestamp >= other last echoed timestamp
        //  - The timestamp will be less than or equal to the last timestamp plus the time
        //    between the last packet and now. The RFC defines the max clock rate as 1ms. We
        //    will allow clocks to be up to 10% fast and will allow a total difference or 30
        //    seconds due to a route change. And this gives us an upperbound on the
        //    timestamp.
        //        timestamp <= last timestamp + max ticks
        //    We have to be careful here. Windows will send an initial timestamp of zero and
        //    then initialize it to a random value after the 3whs; presumably to avoid a DoS
        //    by having to call an expensive RNG during a SYN flood. Proof MS has at least
        //    one good security geek.
        //
        //  - The TCP timestamp option must also echo the other endpoints timestamp. The
        //    timestamp echoed is the one carried on the earliest unacknowledged segment on
        //    the left edge of the sequence window. The RFC states that the host will reject
        //    any echoed timestamps that were larger than any ever sent. This gives us an
        //    upperbound on the TS echo.
        //        tescr <= largest_tsval
        //  - The lowerbound on the TS echo is a little more tricky to determine. The other
        //    endpoint's echoed values will not decrease. But there may be network conditions
        //    that re-order packets and cause our view of them to decrease. For now the only
        //    lowerbound we can safely determine is that the TS echo will never be less than
        //    the original TS. XXX There is probably a better lowerbound. Remove TS_MAX_CONN
        //    with better lowerbound check.
        //        tescr >= other original TS
        //
        // It is also important to note that the fastest timestamp clock of 1ms will wrap its
        // 32bit space in 24 days. So we just disable TS checking after 24 days of idle time.
        // We actually must use a 12d connection limit until we can come up with a better
        // lowerbound to the TS echo check.

        // PFTM_TS_DIFF is how many seconds of leeway to allow a host's timestamp. This can
        // happen if the previous packet got delayed in transit for much longer than this
        // packet.
        let mut ts_fudge = state.rule.ptr().map_or(0, |r| r.timeout(PFTM_TS_DIFF));
        if ts_fudge == 0 {
            ts_fudge = PF_DEFAULT_RULE.timeout(PFTM_TS_DIFF);
        }

        // Calculate max ticks since the last timestamp
        let delta_ts = timersub(&uptime, &ss.pfss_last.get());
        let mut tsval_from_last =
            ((delta_ts.tv_sec as u32).wrapping_add(ts_fudge)).wrapping_mul(TS_MAXFREQ);
        tsval_from_last =
            tsval_from_last.wrapping_add((delta_ts.tv_usec as u32) / (TS_MICROSECS / TS_MAXFREQ));

        if (i32::from(src.state.get()) >= TCPS_ESTABLISHED
            && i32::from(dst.state.get()) >= TCPS_ESTABLISHED)
            && (seq_lt(tsval, ds.pfss_tsecr.get())
                || seq_gt(tsval, ss.pfss_tsval.get().wrapping_add(tsval_from_last))
                || (tsecr != 0
                    && (seq_gt(tsecr, ds.pfss_tsval.get()) || seq_lt(tsecr, ds.pfss_tsval0.get()))))
        {
            // Bad RFC1323 implementation or an insertion attack.
            //
            // - Solaris 2.6 and 2.7 are known to send another ACK after the FIN,FIN|ACK,ACK
            //   closing that carries an old timestamp.

            crate::dpfprintf!(
                LOG_NOTICE,
                "Timestamp failed {}{}{}{}",
                if seq_lt(tsval, ds.pfss_tsecr.get()) {
                    '0'
                } else {
                    ' '
                },
                if seq_gt(tsval, ss.pfss_tsval.get().wrapping_add(tsval_from_last)) {
                    '1'
                } else {
                    ' '
                },
                if seq_gt(tsecr, ds.pfss_tsval.get()) {
                    '2'
                } else {
                    ' '
                },
                if seq_lt(tsecr, ds.pfss_tsval0.get()) {
                    '3'
                } else {
                    ' '
                }
            );
            crate::dpfprintf!(
                LOG_NOTICE,
                " tsval: {}  tsecr: {}  +ticks: {}  idle: {}.{:06}s",
                tsval,
                tsecr,
                tsval_from_last,
                delta_ts.tv_sec,
                delta_ts.tv_usec
            );
            crate::dpfprintf!(
                LOG_NOTICE,
                " src->tsval: {}  tsecr: {}",
                ss.pfss_tsval.get(),
                ss.pfss_tsecr.get()
            );
            crate::dpfprintf!(
                LOG_NOTICE,
                " dst->tsval: {}  tsecr: {}  tsval0: {}",
                ds.pfss_tsval.get(),
                ds.pfss_tsecr.get(),
                ds.pfss_tsval0.get()
            );
            if crate::net::pf::pf_debug(LOG_NOTICE) {
                log(LOG_NOTICE, format_args!("pf: "));
                pf_print_state(state);
                pf_print_flags(th.th_flags);
                addlog(format_args!("\n"));
            }
            reason_set(reason, PFRES_TS);
            return i32::from(PF_DROP);
        }
        // XXX I'd really like to require tsecr but it's optional
    } else if !got_ts
        && th.th_flags & TH_RST == 0
        && ((i32::from(src.state.get()) == TCPS_ESTABLISHED
            && i32::from(dst.state.get()) == TCPS_ESTABLISHED)
            || pd.p_len > 0
            || th.th_flags & TH_SYN != 0)
        && let (Some(ss), Some(_)) = (src.scrub.get(), dst.scrub.get())
        && ss.pfss_flags.get() & PFSS_PAWS != 0
        && scrub_flags(dst) & PFSS_PAWS != 0
    {
        // Didn't send a timestamp. Timestamps aren't really useful when:
        //  - connection opening or closing (often not even sent). but we must not let an
        //    attacker to put a FIN on a data packet to sneak it through our ESTABLISHED
        //    check.
        //  - on a TCP reset. RFC suggests not even looking at TS.
        //  - on an empty ACK. The TS will not be echoed so it will probably not help keep
        //    the RTT calculation in sync and there isn't as much danger when the sequence
        //    numbers got wrapped. So some stacks don't include TS on empty ACKs :-(
        //
        // To minimize the disruption to mostly RFC1323 conformant stacks, we will only
        // require timestamps on data packets.
        //
        // And what do ya know, we cannot require timestamps on data packets. There appear to
        // be devices that do legitimate TCP connection hijacking. There are HTTP devices that
        // allow a 3whs (with timestamps) and then buffer the HTTP request. If the
        // intermediate device has the HTTP response cache, it will spoof the response but not
        // bother timestamping its packets. So we can look for the presence of a timestamp in
        // the first data packet and if there, require it in all future packets.

        if pd.p_len > 0 && ss.pfss_flags.get() & PFSS_DATA_TS != 0 {
            // Hey! Someone tried to sneak a packet in. Or the stack changed its RFC1323
            // behavior?!?!
            if crate::net::pf::pf_debug(LOG_NOTICE) {
                log(
                    LOG_NOTICE,
                    format_args!("pf: did not receive expected RFC1323 timestamp"),
                );
                pf_print_state(state);
                pf_print_flags(th.th_flags);
                addlog(format_args!("\n"));
            }
            reason_set(reason, PFRES_TS);
            return i32::from(PF_DROP);
        }
    }

    // We will note if a host sends his data packets with or without timestamps. And require
    // all data packets to contain a timestamp if the first does. PAWS implicitly requires
    // that all data packets be timestamped. But I think there are middle-man devices that
    // hijack TCP streams immediately after the 3whs and don't timestamp their packets (seen
    // in a WWW accelerator or cache).
    if pd.p_len > 0
        && let Some(s) = src.scrub.get()
        && (s.pfss_flags.get() & (PFSS_TIMESTAMP | PFSS_DATA_TS | PFSS_DATA_NOTS)) == PFSS_TIMESTAMP
    {
        if got_ts {
            s.pfss_flags.set(s.pfss_flags.get() | PFSS_DATA_TS);
        } else {
            s.pfss_flags.set(s.pfss_flags.get() | PFSS_DATA_NOTS);
            if crate::net::pf::pf_debug(LOG_NOTICE) && scrub_flags(dst) & PFSS_TIMESTAMP != 0 {
                // Don't warn if other host rejected RFC1323
                log(
                    LOG_NOTICE,
                    format_args!(
                        "pf: broken RFC1323 stack did not timestamp data packet. Disabled \
                         PAWS security."
                    ),
                );
                pf_print_state(state);
                pf_print_flags(th.th_flags);
                addlog(format_args!("\n"));
            }
        }
    }

    // Update PAWS values
    if got_ts
        && let Some(s) = src.scrub.get()
        && PFSS_TIMESTAMP == (s.pfss_flags.get() & (PFSS_PAWS_IDLED | PFSS_TIMESTAMP))
    {
        s.pfss_last.set(getmicrouptime());
        if seq_geq(tsval, s.pfss_tsval.get()) || (s.pfss_flags.get() & PFSS_PAWS) == 0 {
            s.pfss_tsval.set(tsval);
        }

        if tsecr != 0 {
            if seq_geq(tsecr, s.pfss_tsecr.get()) || (s.pfss_flags.get() & PFSS_PAWS) == 0 {
                s.pfss_tsecr.set(tsecr);
            }

            if (s.pfss_flags.get() & PFSS_PAWS) == 0
                && (seq_lt(tsval, s.pfss_tsval0.get()) || s.pfss_tsval0.get() == 0)
            {
                // tsval0 MUST be the lowest timestamp
                s.pfss_tsval0.set(tsval);
            }

            // Only fully initialized after a TS gets echoed
            if (s.pfss_flags.get() & PFSS_PAWS) == 0 {
                s.pfss_flags.set(s.pfss_flags.get() | PFSS_PAWS);
            }
        }
    }

    // I have a dream....  TCP segment reassembly....
    0
}

/// `pf_normalize_mss`: clamps the segment's MSS options to `maxmss`.
pub fn pf_normalize_mss(pd: &mut PfPdesc, maxmss: u16) -> i32 {
    let mut opts = [0u8; MAX_TCPOPTLEN];
    let optsoff = pd.off as usize + size_of::<Tcphdr>();
    let Some(olen) = pull_tcpopts(pd, &mut opts, TCPOLEN_MAXSEG) else {
        return 0;
    };
    let opts = &opts[..olen];

    let mut opt = 0;
    while let Some(o) = pf_find_tcpopt(opts, opt, TCPOPT_MAXSEG, TCPOLEN_MAXSEG) {
        let mssoffopts = o + 2;
        let mut mss = u16::from_ne_bytes([opts[mssoffopts], opts[mssoffopts + 1]]);
        if u16::from_be(mss) > maxmss {
            // SAFETY: `mss` is a local that outlives the patch and is not otherwise accessed
            // while it runs.
            let loc = unsafe { PfLoc::local(&raw mut mss) };
            pf_patch_16_unaligned(pd, loc, maxmss.to_be(), pf_algnmnt(mssoffopts));
            let _ = m_copyback(
                pd.m,
                (optsoff + mssoffopts) as i32,
                &mss.to_ne_bytes(),
                M_NOWAIT,
            );
            copyback_tcphdr(pd);
        }

        opt = o + usize::from(opts[o + 1]);
    }

    0
}

/// `pf_scrub`: the packet rewrites of `scrub` rules and scrubbed states: clear `IP_DF`
/// (`no-df`), raise the TTL (`min-ttl`), set the TOS (`set-tos`) and a random IP id
/// (`random-id`, not for fragments), fixing the header checksum.
pub fn pf_scrub(m: &Mbuf, flags: u16, af: SaFamily, min_ttl: u8, tos: u8) {
    #[cfg(feature = "inet6")]
    if af == AF_INET6 {
        let mut h6 = mtod_ip6(m);
        // Enforce a minimum ttl, may cause endless packet loops
        if min_ttl != 0 && h6.ip6_hlim < min_ttl {
            h6.ip6_hlim = min_ttl;
        }
        // Enforce tos: drugs are unable to explain such idiocy
        if flags & PFSTATE_SETTOS != 0 {
            h6.ip6_flow &= !0x0fc0_0000u32.to_be();
            h6.ip6_flow |= (u32::from(tos) << 20).to_be();
        }
        mtod_ip6_store(m, &h6);
        return;
    }
    if af != AF_INET {
        return;
    }
    let mut h = mtod_ip(m);

    // Clear IP_DF if no-df was requested
    if flags & PFSTATE_NODF != 0 && h.ip_off & IP_DF.to_be() != 0 {
        let old = h.ip_off;
        h.ip_off &= (!IP_DF).to_be();
        pf_cksum_fixup(&mut h.ip_sum, old, h.ip_off, 0);
    }

    // Enforce a minimum ttl, may cause endless packet loops
    if min_ttl != 0 && h.ip_ttl < min_ttl {
        let old = u16::from(h.ip_ttl);
        h.ip_ttl = min_ttl;
        pf_cksum_fixup(&mut h.ip_sum, old, u16::from(h.ip_ttl), 0);
    }

    // Enforce tos
    if flags & PFSTATE_SETTOS != 0 {
        // ip_tos is 8 bit field at offset 1. Use 16 bit value at offset 0.
        let old = u16::from_ne_bytes([h.ip_vhl, h.ip_tos]);
        h.ip_tos = tos | (h.ip_tos & IPTOS_ECN_MASK);
        pf_cksum_fixup(
            &mut h.ip_sum,
            old,
            u16::from_ne_bytes([h.ip_vhl, h.ip_tos]),
            0,
        );
    }

    // random-id, but not for fragments
    if flags & PFSTATE_RANDOMID != 0 && h.ip_off & !IP_DF.to_be() == 0 {
        let old = h.ip_id;
        h.ip_id = ip_randomid().to_be();
        pf_cksum_fixup(&mut h.ip_sum, old, h.ip_id, 0);
    }

    mtod_ip_store(m, &h);
}

// CTASSERTs of pf_frent_index and pf_frent_insert.
const _: () = {
    assert!((0xffffusize & !7) / (0x10000 / PF_FRAG_ENTRY_POINTS) == 16 - 1);
    assert!((0xffffusize >> 3) / PF_FRAG_ENTRY_POINTS == 512 - 1);
    assert!(PF_FRAG_ENTRY_LIMIT as u32 <= 0xff);
};

#[cfg(test)]
mod tests;
