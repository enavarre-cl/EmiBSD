/*	$OpenBSD: frag6.c,v 1.97 2026/08/11 14:28:59 bluhm Exp $	*/
/*	$KAME: frag6.c,v 1.40 2002/05/27 21:40:31 itojun Exp $	*/
/* <LICENSES> */
/*
 * Copyright (C) 1995, 1996, 1997, and 1998 WIDE Project.
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
 * 3. Neither the name of the project nor the names of its contributors
 *    may be used to endorse or promote products derived from this software
 *    without specific prior written permission.
 *
 * THIS SOFTWARE IS PROVIDED BY THE PROJECT AND CONTRIBUTORS ``AS IS'' AND
 * ANY EXPRESS OR IMPLIED WARRANTIES, INCLUDING, BUT NOT LIMITED TO, THE
 * IMPLIED WARRANTIES OF MERCHANTABILITY AND FITNESS FOR A PARTICULAR PURPOSE
 * ARE DISCLAIMED.  IN NO EVENT SHALL THE PROJECT OR CONTRIBUTORS BE LIABLE
 * FOR ANY DIRECT, INDIRECT, INCIDENTAL, SPECIAL, EXEMPLARY, OR CONSEQUENTIAL
 * DAMAGES (INCLUDING, BUT NOT LIMITED TO, PROCUREMENT OF SUBSTITUTE GOODS
 * OR SERVICES; LOSS OF USE, DATA, OR PROFITS; OR BUSINESS INTERRUPTION)
 * HOWEVER CAUSED AND ON ANY THEORY OF LIABILITY, WHETHER IN CONTRACT, STRICT
 * LIABILITY, OR TORT (INCLUDING NEGLIGENCE OR OTHERWISE) ARISING IN ANY WAY
 * OUT OF THE USE OF THIS SOFTWARE, EVEN IF ADVISED OF THE POSSIBILITY OF
 * SUCH DAMAGE.
 */
/* </LICENSES> */

//! IPv6 reassembly: `netinet6/frag6.c`.
//!
//! Upstream: sys/netinet6/frag6.c @ 3ce1f3f79392
//!
//! `frag6_input` is the fragment header's `pr_input`. An atomic fragment (offset 0, no more
//! fragments, RFC 6946) is processed in isolation; every other fragment is queued on the
//! reassembly queue of its datagram (`struct ip6q`, found by identification, addresses and
//! routing domain), sorted by offset. Overlapping fragments discard the whole datagram
//! (RFC 5722). Once the fragments cover the datagram, they are concatenated, the fragment
//! header is removed and the next header of the reassembled packet is returned.
//! `frag6_slowtimo` ages the queues and sends an ICMPv6 time exceeded error for the first
//! fragment of an expired datagram.
//!
//! Locks: \[Q\] `frag6_mutex` protects the queue, the counters and every `ip6q` and
//! `ip6asfrag` on it.
//!
//! ## Deviations
//! - `frag6_queue` (`TAILQ_HEAD`) is a `Sync` wrapper around the queue head; the counters
//!   `frag6_nfragpackets` and `frag6_nfrags` are atomics only changed under `frag6_mutex`.
//!   The `ip6q` and `ip6asfrag` entries are pool items written whole when allocated (the
//!   C's `PR_ZERO` plus the field stores).
//! - The IPv6 and fragment headers are read and written as copies (`mtod_ip6`, unaligned
//!   reads): mbuf data has no alignment guarantee.
//! - The C's `struct mbuf **` is `&mut Option<&'static Mbuf>`: when the fragment was queued
//!   (or consumed by `icmp6_error`), `*mp` is `None`; the C returns `IPPROTO_DONE` with
//!   `*mp` still pointing at the mbuf the queue now owns.
//! - `frag6_deletefraghdr` takes the mbuf as `&'static Mbuf` (`m_split` hands out the tail of
//!   a chain that lives on) and returns `Result` (`ENOBUFS` for the C's non-zero).
//! - The C's `goto insert`/`flushfrags`/`dropfrag` are labelled blocks.

use core::mem::{offset_of, size_of};
use core::ptr::{self, NonNull};
use core::sync::atomic::{AtomicU32, Ordering};

use crate::kern::kern_lock::{mtx_enter, mtx_leave};
use crate::kern::subr_pool::{pool_get, pool_init, pool_put};
use crate::kern::uipc_mbuf::{m_adj, m_calchdrlen, m_cat, m_freem, m_removehdr, m_split};
use crate::machine::intr::IPL_SOFTNET;
use crate::net::if_var::Netstack;
use crate::net::rtable::rtable_l2;
use crate::netinet::icmp6::{
    ICMP6_PARAM_PROB, ICMP6_PARAMPROB_HEADER, ICMP6_TIME_EXCEED_REASSEMBLY, ICMP6_TIME_EXCEEDED,
};
use crate::netinet::in_::IPPROTO_DONE;
use crate::netinet::ip::{IPTOS_ECN_CE, IPTOS_ECN_MASK, IPTOS_ECN_NOTECT};
use crate::netinet::ip6::{
    IP6F_MORE_FRAG, IP6F_OFF_MASK, IPV6_FRAGTTL, IPV6_MAXPACKET, Ip6Frag, Ip6Hdr, ip6_exthdr_get,
};
use crate::netinet6::icmp6::icmp6_error;
use crate::netinet6::in6::in6_are_addr_equal;
use crate::netinet6::in6_proto::{IP6_MAXFRAGPACKETS, IP6_MAXFRAGS};
use crate::netinet6::ip6_input::ip6_get_prevhdr;
use crate::netinet6::ip6_var::{
    Ip6asfrag, Ip6asfragList, Ip6q, Ip6qList, Ip6statCounters, ip6stat_add, ip6stat_inc, mtod_ip6,
    mtod_ip6_store,
};
use crate::sys::endian::{htonl, htons, ntohl, ntohs};
use crate::sys::errno::Errno;
use crate::sys::mbuf::{M_DONTWAIT, Mbuf, m_freemp, mtod};
use crate::sys::mutex::{Mutex, mutex_assert_locked};
use crate::sys::pool::{PR_NOWAIT, PR_ZERO, Pool};
use crate::sys::queue::{ListEntry, ListHead, TailqEntry, TailqHead};
use crate::sys::systm::{net_lock_shared, net_unlock_shared};

/// The size of the IPv6 header in offset arithmetic.
const IP6_HDR_LEN: i32 = size_of::<Ip6Hdr>() as i32;
/// The size of the fragment header in offset arithmetic.
const IP6_FRAG_LEN: i32 = size_of::<Ip6Frag>() as i32;

/// `TAILQ_HEAD(ip6q_head, ip6q)`, made `Sync`: the reassembly queues, protected by
/// `frag6_mutex`.
struct Ip6qHead(TailqHead<Ip6qList>);

// SAFETY: see the type's doc.
unsafe impl Sync for Ip6qHead {}

/// `frag6_mutex`.
static FRAG6_MUTEX: Mutex = Mutex::new(IPL_SOFTNET);

/// \[Q\] `frag6_nfragpackets`.
static FRAG6_NFRAGPACKETS: AtomicU32 = AtomicU32::new(0);
/// \[Q\] `frag6_nfrags`.
static FRAG6_NFRAGS: AtomicU32 = AtomicU32::new(0);
/// \[Q\] `frag6_queue`: ip6 reassemble queue.
static FRAG6_QUEUE: Ip6qHead = Ip6qHead(TailqHead::new());

/// `ip6af_pool`.
static IP6AF_POOL: Pool = Pool::new();
/// `ip6q_pool`.
static IP6Q_POOL: Pool = Pool::new();

/// `frag6_init`: initialises the reassembly queue and pools.
pub fn frag6_init() {
    pool_init(
        &IP6AF_POOL,
        size_of::<Ip6asfrag>(),
        0,
        IPL_SOFTNET,
        0,
        "ip6af",
        None,
    );
    pool_init(
        &IP6Q_POOL,
        size_of::<Ip6q>(),
        0,
        IPL_SOFTNET,
        0,
        "ip6q",
        None,
    );

    FRAG6_QUEUE.0.init();
}

/// Frees a reassembly queue header.
fn ip6q_put(q6: &Ip6q) {
    pool_put(&IP6Q_POOL, NonNull::from(q6).cast());
}

/// Frees a fragment entry.
fn ip6af_put(af6: &Ip6asfrag) {
    pool_put(&IP6AF_POOL, NonNull::from(af6).cast());
}

/// `TAILQ_REMOVE(&frag6_queue, q6, ip6q_queue)` with the counter updates every exit of
/// `frag6_input` that drops a datagram does.
fn frag6_dequeue(q6: &Ip6q) {
    // SAFETY: `q6` is on `frag6_queue`, under `frag6_mutex`.
    unsafe { FRAG6_QUEUE.0.remove(q6) };
    FRAG6_NFRAGS.fetch_sub(q6.ip6q_nfrag.get() as u32, Ordering::Relaxed);
    FRAG6_NFRAGPACKETS.fetch_sub(1, Ordering::Relaxed);
}

// In RFC2460, fragment and reassembly rule do not agree with each other, in terms of next
// header field handling in fragment header. While the sender will use the same value for
// all of the fragmented packets, receiver is suggested not to check the consistency.
//
// fragment rule (p20):
//	(2) A Fragment header containing:
//	The Next Header value that identifies the first header of
//	the Fragmentable Part of the original packet.
//		-> next header field is same for all fragments
//
// reassembly rule (p21):
//	The Next Header field of the last header of the Unfragmentable
//	Part is obtained from the Next Header field of the first
//	fragment's Fragment header.
//		-> should grab it from the first fragment only
//
// The following note also contradicts with fragment rule - noone is going to send different
// fragment with different next header field.
//
// additional note (p22):
//	The Next Header values in the Fragment headers of different
//	fragments of the same original packet may differ.  Only the value
//	from the Offset zero fragment packet is used for reassembly.
//		-> should grab it from the first fragment only
//
// There is no explicit reason given in the RFC. Historical reason maybe?

/// `frag6_input`: fragment input, the fragment header's `pr_input`: queues the fragment,
/// and returns the next header of the reassembled packet once complete (`IPPROTO_DONE`
/// otherwise, `*mp` cleared).
pub fn frag6_input(
    mp: &mut Option<&'static Mbuf>,
    offp: &mut i32,
    proto: i32,
    af: i32,
    ns: Option<&Netstack>,
) -> i32 {
    let _ = (proto, af, ns);
    let mut offset = *offp;

    let Some(p) = ip6_exthdr_get(mp, offset, IP6_FRAG_LEN) else {
        return IPPROTO_DONE;
    };
    // SAFETY: `ip6_exthdr_get` made the fragment header contiguous at `p`.
    let ip6f = unsafe { ptr::read_unaligned(p.cast::<Ip6Frag>()) };
    let Some(m) = *mp else {
        return IPPROTO_DONE;
    };
    let ip6 = mtod_ip6(m);
    let plen = i32::from(ntohs(ip6.ip6_plen));

    // jumbo payload can't contain a fragment header
    if ip6.ip6_plen == 0 {
        *mp = None;
        icmp6_error(m, ICMP6_PARAM_PROB, ICMP6_PARAMPROB_HEADER, offset);
        return IPPROTO_DONE;
    }

    // check whether fragment packet's fragment length is multiple of 8 octets.
    // sizeof(struct ip6_frag) == 8
    // sizeof(struct ip6_hdr) = 40
    if ip6f.ip6f_offlg & IP6F_MORE_FRAG != 0 && (plen - offset) & 0x7 != 0 {
        *mp = None;
        icmp6_error(
            m,
            ICMP6_PARAM_PROB,
            ICMP6_PARAMPROB_HEADER,
            offset_of!(Ip6Hdr, ip6_plen) as i32,
        );
        return IPPROTO_DONE;
    }

    ip6stat_inc(Ip6statCounters::Ip6sFragments);

    // offset now points to data portion
    offset += IP6_FRAG_LEN;

    // RFC6946: A host that receives an IPv6 packet which includes a Fragment Header with the
    // "Fragment Offset" equal to 0 and the "M" bit equal to 0 MUST process such packet in
    // isolation from any other packets/fragments.
    let fragoff = i32::from(ntohs(ip6f.ip6f_offlg & IP6F_OFF_MASK));
    if fragoff == 0 && ip6f.ip6f_offlg & IP6F_MORE_FRAG == 0 {
        ip6stat_inc(Ip6statCounters::Ip6sReassembled);
        *offp = offset;
        return i32::from(ip6f.ip6f_nxt);
    }

    // Ignore empty non atomic fragment, do not classify as overlapping.
    if IP6_HDR_LEN + plen <= offset {
        m_freemp(mp);
        return IPPROTO_DONE;
    }

    mtx_enter(&FRAG6_MUTEX);

    'dropfrag: {
        // Enforce upper bound on number of fragments. If maxfrag is 0, never accept
        // fragments.
        let maxfrags = IP6_MAXFRAGS.load(Ordering::Relaxed);
        if i64::from(FRAG6_NFRAGS.load(Ordering::Relaxed)) >= i64::from(maxfrags) {
            mtx_leave(&FRAG6_MUTEX);
            break 'dropfrag;
        }

        let rdomain = rtable_l2(m.m_pkthdr().ph_rtableid.get());
        let found = FRAG6_QUEUE.0.iter().find(|q6| {
            ip6f.ip6f_ident == q6.ip6q_ident.get()
                && in6_are_addr_equal(&ip6.ip6_src, &q6.ip6q_src.get())
                && in6_are_addr_equal(&ip6.ip6_dst, &q6.ip6q_dst.get())
                && rdomain == q6.ip6q_rdomain.get()
        });
        // SAFETY: a queue on `frag6_queue` lives until it is unlinked and freed under
        // `frag6_mutex`, which is held.
        let found: Option<&'static Ip6q> = found.map(|q| unsafe { &*ptr::from_ref(q) });

        let first_frag = found.is_none();
        let q6: &'static Ip6q = match found {
            Some(q6) => q6,
            None => {
                // the first fragment to arrive, create a reassembly queue.

                // Enforce upper bound on number of fragmented packets for which we attempt
                // reassembly; If maxfragpackets is 0, never accept fragments.
                let maxfragpackets = IP6_MAXFRAGPACKETS.load(Ordering::Relaxed);
                if i64::from(FRAG6_NFRAGPACKETS.load(Ordering::Relaxed))
                    >= i64::from(maxfragpackets)
                {
                    mtx_leave(&FRAG6_MUTEX);
                    break 'dropfrag;
                }
                FRAG6_NFRAGPACKETS.fetch_add(1, Ordering::Relaxed);
                let Some(mem) = pool_get(&IP6Q_POOL, PR_NOWAIT | PR_ZERO) else {
                    mtx_leave(&FRAG6_MUTEX);
                    break 'dropfrag;
                };
                let qp = mem.as_ptr().cast::<Ip6q>();
                // SAFETY: a fresh pool item of `size_of::<Ip6q>()` bytes, written whole; it
                // lives until it is put back after leaving the queue.
                let q6: &'static Ip6q = unsafe {
                    qp.write(Ip6q {
                        ip6q_queue: TailqEntry::new(),
                        // ip6q_nxt will be filled afterwards, from 1st fragment
                        ip6q_asfrag: ListHead::new(),
                        ip6q_src: core::cell::Cell::new(ip6.ip6_src),
                        ip6q_dst: core::cell::Cell::new(ip6.ip6_dst),
                        // The 1st fragment has not arrived.
                        ip6q_unfrglen: core::cell::Cell::new(-1),
                        ip6q_nfrag: core::cell::Cell::new(0),
                        ip6q_rdomain: core::cell::Cell::new(rdomain),
                        ip6q_ident: core::cell::Cell::new(ip6f.ip6f_ident),
                        ip6q_nxt: core::cell::Cell::new(0),
                        ip6q_ecn: core::cell::Cell::new(
                            (ntohl(ip6.ip6_flow) >> 20) as u8 & IPTOS_ECN_MASK,
                        ),
                        ip6q_ttl: core::cell::Cell::new(IPV6_FRAGTTL),
                    });
                    &*qp
                };

                // SAFETY: `q6` is new and on no queue; the queue changes under
                // `frag6_mutex`.
                unsafe { FRAG6_QUEUE.0.insert_head(q6) };
                q6
            }
        };

        // If it's the 1st fragment, record the length of the unfragmentable part and the
        // next header of the fragment header.
        if fragoff == 0 {
            q6.ip6q_unfrglen.set(offset - IP6_HDR_LEN - IP6_FRAG_LEN);
            q6.ip6q_nxt.set(ip6f.ip6f_nxt);
        }

        // Check that the reassembled packet would not exceed 65535 bytes in size. If it would
        // exceed, discard the fragment and return an ICMP error.
        let frgpartlen = IP6_HDR_LEN + plen - offset;
        let erroff = offset - IP6_FRAG_LEN + offset_of!(Ip6Frag, ip6f_offlg) as i32;
        let toobig = if q6.ip6q_unfrglen.get() >= 0 {
            // The 1st fragment has already arrived.
            q6.ip6q_unfrglen.get() + fragoff + frgpartlen > IPV6_MAXPACKET as i32
        } else {
            fragoff + frgpartlen > IPV6_MAXPACKET as i32
        };
        if toobig {
            mtx_leave(&FRAG6_MUTEX);
            *mp = None;
            icmp6_error(m, ICMP6_PARAM_PROB, ICMP6_PARAMPROB_HEADER, erroff);
            return IPPROTO_DONE;
        }
        // If it's the first fragment, do the above check for each fragment already stored in
        // the reassembly queue.
        if fragoff == 0 {
            for af6 in q6.ip6q_asfrag.iter() {
                if q6.ip6q_unfrglen.get() + af6.ip6af_off.get() + af6.ip6af_frglen.get()
                    > IPV6_MAXPACKET as i32
                {
                    let merr = af6.ip6af_m.get();
                    let erroff = af6.ip6af_offset.get();

                    // dequeue the fragment.
                    // SAFETY: `af6` is on this queue, under `frag6_mutex`; the iterator
                    // already read the next entry.
                    unsafe { ListHead::<Ip6asfragList>::remove(af6) };
                    ip6af_put(af6);
                    FRAG6_NFRAGS.fetch_sub(1, Ordering::Relaxed);
                    q6.ip6q_nfrag.set(q6.ip6q_nfrag.get() - 1);

                    let Some(merr) = merr else {
                        continue;
                    };
                    // Restore source and destination addresses in the erroneous IPv6
                    // header.
                    let mut ip6err = mtod_ip6(merr);
                    ip6err.ip6_src = q6.ip6q_src.get();
                    ip6err.ip6_dst = q6.ip6q_dst.get();
                    mtod_ip6_store(merr, &ip6err);

                    icmp6_error(
                        merr,
                        ICMP6_PARAM_PROB,
                        ICMP6_PARAMPROB_HEADER,
                        erroff - IP6_FRAG_LEN + offset_of!(Ip6Frag, ip6f_offlg) as i32,
                    );
                }
            }
        }

        let Some(mem) = pool_get(&IP6AF_POOL, PR_NOWAIT | PR_ZERO) else {
            mtx_leave(&FRAG6_MUTEX);
            break 'dropfrag;
        };
        let ap = mem.as_ptr().cast::<Ip6asfrag>();
        // SAFETY: a fresh pool item of `size_of::<Ip6asfrag>()` bytes, written whole; it lives
        // until it is put back after leaving its datagram's list.
        let ip6af: &'static Ip6asfrag = unsafe {
            ap.write(Ip6asfrag {
                ip6af_list: ListEntry::new(),
                ip6af_m: core::cell::Cell::new(Some(m)),
                ip6af_offset: core::cell::Cell::new(offset),
                ip6af_frglen: core::cell::Cell::new(frgpartlen),
                ip6af_off: core::cell::Cell::new(fragoff),
                ip6af_mff: core::cell::Cell::new(ip6f.ip6f_offlg & IP6F_MORE_FRAG),
            });
            &*ap
        };

        'flushfrags: {
            let mut paf6: Option<&'static Ip6asfrag> = None;
            if !first_frag {
                // Handle ECN by comparing this segment with the first one; if CE is set, do
                // not lose CE. drop if CE and not-ECT are mixed for the same packet.
                let ecn = (ntohl(ip6.ip6_flow) >> 20) as u8 & IPTOS_ECN_MASK;
                let ecn0 = q6.ip6q_ecn.get();
                if ecn == IPTOS_ECN_CE {
                    if ecn0 == IPTOS_ECN_NOTECT {
                        mtx_leave(&FRAG6_MUTEX);
                        ip6af_put(ip6af);
                        break 'dropfrag;
                    }
                    if ecn0 != IPTOS_ECN_CE {
                        q6.ip6q_ecn.set(IPTOS_ECN_CE);
                    }
                }
                if ecn == IPTOS_ECN_NOTECT && ecn0 != IPTOS_ECN_NOTECT {
                    mtx_leave(&FRAG6_MUTEX);
                    ip6af_put(ip6af);
                    break 'dropfrag;
                }

                // Find a segment which begins after this one does.
                let mut af6 = q6.ip6q_asfrag.first();
                while let Some(a) = af6 {
                    if a.ip6af_off.get() > ip6af.ip6af_off.get() {
                        break;
                    }
                    // SAFETY: an entry on the list lives until removed under
                    // `frag6_mutex`, which is held.
                    paf6 = Some(unsafe { &*ptr::from_ref(a) });
                    af6 = ListHead::<Ip6asfragList>::next(a);
                }

                // RFC 5722, Errata 3089: When reassembling an IPv6 datagram, if one or more
                // its constituent fragments is determined to be an overlapping fragment, the
                // entire datagram (and any constituent fragments) MUST be silently discarded.
                if let Some(p) = paf6
                    && (p.ip6af_off.get() + p.ip6af_frglen.get()) - ip6af.ip6af_off.get() > 0
                {
                    break 'flushfrags;
                }
                if let Some(a) = af6
                    && (ip6af.ip6af_off.get() + ip6af.ip6af_frglen.get()) - a.ip6af_off.get() > 0
                {
                    break 'flushfrags;
                }
            }

            // insert:
            // Stick new segment in its place; check for complete reassembly. Move to front of
            // packet queue, as we are the most recently active fragmented packet.
            // SAFETY: `ip6af` is on no list; `paf6` is on this datagram's list; the list
            // changes under `frag6_mutex`.
            unsafe {
                match paf6 {
                    Some(p) => ListHead::<Ip6asfragList>::insert_after(p, ip6af),
                    None => q6.ip6q_asfrag.insert_head(ip6af),
                }
            }
            FRAG6_NFRAGS.fetch_add(1, Ordering::Relaxed);
            q6.ip6q_nfrag.set(q6.ip6q_nfrag.get() + 1);
            // The queue owns the fragment now.
            *mp = None;
            let mut next = 0;
            let mut last: Option<&Ip6asfrag> = None;
            for af6 in q6.ip6q_asfrag.iter() {
                if af6.ip6af_off.get() != next {
                    mtx_leave(&FRAG6_MUTEX);
                    return IPPROTO_DONE;
                }
                next += af6.ip6af_frglen.get();
                last = Some(af6);
            }
            if last.is_some_and(|l| l.ip6af_mff.get() != 0) {
                mtx_leave(&FRAG6_MUTEX);
                return IPPROTO_DONE;
            }

            // Reassembly is complete; concatenate fragments.
            let Some(first) = q6.ip6q_asfrag.first() else {
                // The loop above saw the new fragment on the list.
                mtx_leave(&FRAG6_MUTEX);
                return IPPROTO_DONE;
            };
            // SAFETY: on the list, under `frag6_mutex`; freed below after its last use.
            let first: &'static Ip6asfrag = unsafe { &*ptr::from_ref(first) };
            // SAFETY: as above.
            unsafe { ListHead::<Ip6asfragList>::remove(first) };
            *mp = first.ip6af_m.get();
            let Some(m0) = *mp else {
                mtx_leave(&FRAG6_MUTEX);
                ip6af_put(first);
                break 'dropfrag;
            };
            let mut t: &'static Mbuf = m0;
            while let Some(af6) = q6.ip6q_asfrag.first() {
                // SAFETY: on the list, under `frag6_mutex`.
                unsafe { ListHead::<Ip6asfragList>::remove(af6) };
                while let Some(n) = t.m_next().get() {
                    t = n;
                }
                let n = af6.ip6af_m.get();
                t.m_next().set(n);
                if let Some(n) = n {
                    m_adj(n, af6.ip6af_offset.get());
                    m_removehdr(n);
                }
                ip6af_put(af6);
            }

            // adjust offset to point where the original next header starts
            let offset = first.ip6af_offset.get() - IP6_FRAG_LEN;
            ip6af_put(first);
            next += offset - IP6_HDR_LEN;
            if next as u32 > IPV6_MAXPACKET as u32 {
                frag6_dequeue(q6);
                mtx_leave(&FRAG6_MUTEX);
                ip6q_put(q6);
                break 'dropfrag;
            }
            let mut ip6 = mtod_ip6(m0);
            ip6.ip6_plen = htons(next as u16);
            ip6.ip6_src = q6.ip6q_src.get();
            ip6.ip6_dst = q6.ip6q_dst.get();
            if q6.ip6q_ecn.get() == IPTOS_ECN_CE {
                ip6.ip6_flow |= htonl(u32::from(IPTOS_ECN_CE) << 20);
            }
            mtod_ip6_store(m0, &ip6);
            let nxt = q6.ip6q_nxt.get();

            // Delete frag6 header
            if frag6_deletefraghdr(m0, offset).is_err() {
                frag6_dequeue(q6);
                mtx_leave(&FRAG6_MUTEX);
                ip6q_put(q6);
                break 'dropfrag;
            }

            frag6_dequeue(q6);

            mtx_leave(&FRAG6_MUTEX);

            ip6q_put(q6);

            m_calchdrlen(m0);

            // Restore NXT to the original.
            let prvnxt = ip6_get_prevhdr(m0, offset);
            let Some(prvnxtp) = ip6_exthdr_get(mp, prvnxt, size_of::<u8>() as i32) else {
                break 'dropfrag;
            };
            // SAFETY: `ip6_exthdr_get` made the byte at `prvnxt` addressable at `prvnxtp`.
            unsafe { *prvnxtp = nxt };

            ip6stat_inc(Ip6statCounters::Ip6sReassembled);

            // Tell launch routine the next header
            *offp = offset;
            return i32::from(nxt);
        }
        // flushfrags:
        frag6_dequeue(q6);

        mtx_leave(&FRAG6_MUTEX);

        ip6af_put(ip6af);

        while let Some(af6) = q6.ip6q_asfrag.first() {
            // SAFETY: the datagram is off the queue: nothing else reaches its list.
            unsafe { ListHead::<Ip6asfragList>::remove(af6) };
            m_freem(af6.ip6af_m.get());
            ip6af_put(af6);
        }
        ip6stat_add(
            Ip6statCounters::Ip6sFragdropped,
            q6.ip6q_nfrag.get() as u64 + 1,
        );
        ip6q_put(q6);
        m_freemp(mp);
        return IPPROTO_DONE;
    }
    // dropfrag:
    ip6stat_inc(Ip6statCounters::Ip6sFragdropped);
    m_freemp(mp);
    IPPROTO_DONE
}

/// `frag6_deletefraghdr`: deletes the fragment header after the unfragmentable header
/// portions (`offset` bytes) of `m`.
pub fn frag6_deletefraghdr(m: &'static Mbuf, offset: i32) -> Result<(), Errno> {
    let flen = size_of::<Ip6Frag>();

    if m.m_len().get() as usize >= offset as usize + flen {
        let data = mtod::<u8>(m);
        // SAFETY: the first mbuf holds the `offset` bytes of headers and the fragment
        // header after them (checked above); `copy` handles the overlap as `memmove`.
        unsafe { ptr::copy(data, data.add(flen), offset as usize) };
        m.m_data().set(data.wrapping_add(flen));
        m.m_len().set(m.m_len().get() - flen as u32);
    } else {
        // this comes with no copy if the boundary is on cluster
        let Some(t) = m_split(m, offset, M_DONTWAIT) else {
            return Err(Errno::ENOBUFS);
        };
        m_adj(t, flen as i32);
        m_cat(m, Some(t));
    }

    Ok(())
}

/// `frag6_freef`: frees a fragment reassembly header and all associated datagrams. The
/// header must not be in any queue.
fn frag6_freef(q6: &'static Ip6q) {
    while let Some(af6) = q6.ip6q_asfrag.first() {
        let m = af6.ip6af_m.get();

        // SAFETY: `q6` is in no queue: its list is reached only from here.
        unsafe { ListHead::<Ip6asfragList>::remove(af6) };

        // Return ICMP time exceeded error for the 1st fragment. Just free other fragments.
        if let Some(m) = m
            && af6.ip6af_off.get() == 0
        {
            // restore source and destination addresses
            let mut ip6 = mtod_ip6(m);
            ip6.ip6_src = q6.ip6q_src.get();
            ip6.ip6_dst = q6.ip6q_dst.get();
            mtod_ip6_store(m, &ip6);

            net_lock_shared();
            icmp6_error(m, ICMP6_TIME_EXCEEDED, ICMP6_TIME_EXCEED_REASSEMBLY, 0);
            net_unlock_shared();
        } else {
            m_freem(m);
        }
        ip6af_put(af6);
    }
    ip6q_put(q6);
}

/// `frag6_unlink`: unlinks a fragment reassembly header from the reassembly queue and
/// inserts it into a given remove queue.
fn frag6_unlink(q6: &'static Ip6q, rmq6: &TailqHead<Ip6qList>) {
    mutex_assert_locked(&FRAG6_MUTEX, "frag6_unlink");

    // SAFETY: `q6` is on `frag6_queue`, under `frag6_mutex`; once unlinked it may join the
    // remove queue, which outlives it there.
    unsafe {
        FRAG6_QUEUE.0.remove(q6);
        rmq6.insert_head(q6);
    }
    FRAG6_NFRAGS.fetch_sub(q6.ip6q_nfrag.get() as u32, Ordering::Relaxed);
    FRAG6_NFRAGPACKETS.fetch_sub(1, Ordering::Relaxed);
}

/// `frag6_slowtimo`: IPv6 reassembling timer processing; if a timer expires on a
/// reassembly queue, discard it.
pub fn frag6_slowtimo() {
    let rmq6: TailqHead<Ip6qList> = TailqHead::new();
    let ip6_maxfragpackets_local = IP6_MAXFRAGPACKETS.load(Ordering::Relaxed);

    rmq6.init();

    mtx_enter(&FRAG6_MUTEX);

    for q6 in FRAG6_QUEUE.0.iter() {
        let ttl = q6.ip6q_ttl.get().wrapping_sub(1);
        q6.ip6q_ttl.set(ttl);
        if ttl == 0 {
            ip6stat_inc(Ip6statCounters::Ip6sFragtimeout);
            // SAFETY: on the queue, under `frag6_mutex`; the iterator already read the next.
            frag6_unlink(unsafe { &*ptr::from_ref(q6) }, &rmq6);
        }
    }

    // If we are over the maximum number of fragments (due to the limit being lowered), drain
    // off enough to get down to the new limit.
    while i64::from(FRAG6_NFRAGPACKETS.load(Ordering::Relaxed))
        > i64::from(ip6_maxfragpackets_local)
    {
        let Some(q6) = FRAG6_QUEUE.0.last() else {
            break;
        };
        ip6stat_inc(Ip6statCounters::Ip6sFragoverflow);
        // SAFETY: on the queue, under `frag6_mutex`.
        frag6_unlink(unsafe { &*ptr::from_ref(q6) }, &rmq6);
    }

    mtx_leave(&FRAG6_MUTEX);

    while let Some(q6) = rmq6.first() {
        // SAFETY: `q6` is on the local remove queue.
        let q6: &'static Ip6q = unsafe { &*ptr::from_ref(q6) };
        // SAFETY: as above.
        unsafe { rmq6.remove(q6) };
        frag6_freef(q6);
    }
}

#[cfg(test)]
mod tests;
