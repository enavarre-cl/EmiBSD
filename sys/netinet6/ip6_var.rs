/*	$KAME: ip6_var.h,v 1.33 2000/06/11 14:59:20 jinmei Exp $	*/
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

/*
 * Copyright (c) 1982, 1986, 1993
 *	The Regents of the University of California.  All rights reserved.
 *
 * Redistribution and use in source and binary forms, with or without
 * modification, are permitted provided that the following conditions
 * are met:
 * 1. Redistributions of source code must retain the above copyright
 *    notice, this list of conditions and the following disclaimer.
 * 2. Redistributions in binary form must reproduce the above copyright
 *    notice, this list of conditions and the following disclaimer in the
 *    documentation and/or other materials provided with the distribution.
 * 3. Neither the name of the University nor the names of its contributors
 *    may be used to endorse or promote products derived from this software
 *    without specific prior written permission.
 *
 * THIS SOFTWARE IS PROVIDED BY THE REGENTS AND CONTRIBUTORS ``AS IS'' AND
 * ANY EXPRESS OR IMPLIED WARRANTIES, INCLUDING, BUT NOT LIMITED TO, THE
 * IMPLIED WARRANTIES OF MERCHANTABILITY AND FITNESS FOR A PARTICULAR PURPOSE
 * ARE DISCLAIMED.  IN NO EVENT SHALL THE REGENTS OR CONTRIBUTORS BE LIABLE
 * FOR ANY DIRECT, INDIRECT, INCIDENTAL, SPECIAL, EXEMPLARY, OR CONSEQUENTIAL
 * DAMAGES (INCLUDING, BUT NOT LIMITED TO, PROCUREMENT OF SUBSTITUTE GOODS
 * OR SERVICES; LOSS OF USE, DATA, OR PROFITS; OR BUSINESS INTERRUPTION)
 * HOWEVER CAUSED AND ON ANY THEORY OF LIABILITY, WHETHER IN CONTRACT, STRICT
 * LIABILITY, OR TORT (INCLUDING NEGLIGENCE OR OTHERWISE) ARISING IN ANY WAY
 * OUT OF THE USE OF THIS SOFTWARE, EVEN IF ADVISED OF THE POSSIBILITY OF
 * SUCH DAMAGE.
 *
 *	@(#)ip_var.h	8.1 (Berkeley) 6/10/93
 */
/* </LICENSES> */

/* <CODE> */
//! IPv6 implementation variables: statistics, the reassembly queues, the multicast and
//! packet options, the `ip6_output` flags: `<netinet6/ip6_var.h>`.
//!
//! Upstream: sys/netinet6/ip6_var.h @ 3ce1f3f79392
//!
//! ## Deviations
//! - `enum ip6stat_counters` is [`Ip6statCounters`] with the C's values; the C indexes its
//!   arrays with arithmetic (`ip6s_nxthist + nxt`, `ip6s_m2m + i`, `ip6s_sources_*` +
//!   scope), which [`ip6stat_inc_idx`] does. `ip6counters` (`struct cpumem *`,
//!   `<sys/percpu.h>` not ported) is the static array of atomics `IP6COUNTERS` in
//!   `netinet6/ip6_input.rs`, which defines the C's pointer.
//! - `struct ip6q` and `struct ip6asfrag` hold `Cell`s (they change under `frag6_mutex`);
//!   `struct ip6_moptions` and `struct ip6_pktopts` are plain structures owned by their
//!   socket (changed under the socket lock), their pointers `Option<NonNull<_>>`
//!   (`malloc(9)` allocations, as `ip_moptions`' are).
//! - `mtod_ip6` and `mtod_ip6_store` read and write the IPv6 header at the start of an
//!   mbuf as a copy, as `mtod_ip` does for `struct ip` (`netinet/ip_var.rs`).
//! - The globals (`ip6_defhlim`, `ip6_forwarding`, `ip6_mtudisc_timeout`, ...) are defined by
//!   `netinet6/in6_proto.rs`, `icmp6_mtudisc_timeout_q` by `netinet6/icmp6.rs`,
//!   `ip6_dad_pending` by `netinet6/in6_proto.rs`, `rip6_usrreqs` by
//!   `netinet6/raw_ip6.rs`; the prototypes by their `.c` files (`ip6_output` in
//!   `netinet6/ip6_output.rs`, `ip6_input_if` in `netinet6/ip6_input.rs`, ...). `ip6_mrouter[]`, `ip6_mrouter_active` and `ip6_mforward`
//!   (`netinet6/ip6_mroute.c`) are not compiled: `MROUTING` is not configured.

use core::cell::Cell;
use core::mem::size_of;
use core::ptr::NonNull;
use core::sync::atomic::Ordering;

use crate::kern::subr_prf::panic;
use crate::netinet::ip6::{Ip6Dest, Ip6Hbh, Ip6Hdr};
use crate::netinet6::in6::{In6Addr, In6Pktinfo};
use crate::netinet6::in6_var::In6MultiMshipList;
use crate::netinet6::ip6_input::IP6COUNTERS;
use crate::queue_adapter;
use crate::sys::mbuf::{Mbuf, mtod};
use crate::sys::queue::{ListEntry, ListHead, TailqEntry};

/// Default: send at min MTU for multicast.
pub const IP6PO_MINMTU_MCASTONLY: i32 = -1;
/// Always perform pmtu disc.
pub const IP6PO_MINMTU_DISABLE: i32 = 0;
/// Always send at min MTU.
pub const IP6PO_MINMTU_ALL: i32 = 1;

/// Disable fragmentation (`IPV6_DONTFRAG`).
pub const IP6PO_DONTFRAG: i32 = 0x04;

// flags passed to ip6_output or ip6_forward as last parameter

/// Allow :: as the source address.
pub const IPV6_UNSPECSRC: i32 = 0x01;
/// Most of IPv6 header exists.
pub const IPV6_FORWARDING: i32 = 0x02;
/// Use minimum MTU (`IPV6_USE_MIN_MTU`).
pub const IPV6_MINMTU: i32 = 0x04;
/// Redirected by pf.
pub const IPV6_REDIRECT: i32 = 0x08;
/// Only packets processed by IPsec.
pub const IPV6_FORWARDING_IPSEC: i32 = 0x10;

/// `struct ip6stat`: IPv6 statistics, as `sysctl(2)` returns them.
#[repr(C)]
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Ip6stat {
    /// Total packets received.
    pub ip6s_total: u64,
    /// Packet too short.
    pub ip6s_tooshort: u64,
    /// Not enough data.
    pub ip6s_toosmall: u64,
    /// Fragments received.
    pub ip6s_fragments: u64,
    /// Frags dropped (dups, out of space).
    pub ip6s_fragdropped: u64,
    /// Fragments timed out.
    pub ip6s_fragtimeout: u64,
    /// Fragments that exceeded limit.
    pub ip6s_fragoverflow: u64,
    /// Packets forwarded.
    pub ip6s_forward: u64,
    /// Packets rcvd for unreachable dest.
    pub ip6s_cantforward: u64,
    /// Packets forwarded on same net.
    pub ip6s_redirectsent: u64,
    /// Datagrams delivered to upper level.
    pub ip6s_delivered: u64,
    /// Total ip packets generated here.
    pub ip6s_localout: u64,
    /// Lost output due to nobufs, etc.
    pub ip6s_odropped: u64,
    /// Total packets reassembled ok.
    pub ip6s_reassembled: u64,
    /// Datagrams successfully fragmented.
    pub ip6s_fragmented: u64,
    /// Output fragments created.
    pub ip6s_ofragments: u64,
    /// Don't fragment flag was set, etc.
    pub ip6s_cantfrag: u64,
    /// Error in option processing.
    pub ip6s_badoptions: u64,
    /// Packets discarded due to no route.
    pub ip6s_noroute: u64,
    /// ip6 version != 6.
    pub ip6s_badvers: u64,
    /// Total raw ip packets generated.
    pub ip6s_rawout: u64,
    /// Scope error.
    pub ip6s_badscope: u64,
    /// Don't join this multicast group.
    pub ip6s_notmember: u64,
    /// Next header history.
    pub ip6s_nxthist: [u64; 256],
    /// One mbuf.
    pub ip6s_m1: u64,
    /// Two or more mbuf.
    pub ip6s_m2m: [u64; 32],
    /// One ext mbuf.
    pub ip6s_mext1: u64,
    /// Two or more ext mbuf.
    pub ip6s_mext2m: u64,
    /// No match gif found.
    pub ip6s_nogif: u64,
    /// Discarded due to too many headers.
    pub ip6s_toomanyhdr: u64,
    // statistics for improvement of the source address selection algorithm
    // (XXX: hardcoded 16 = # of ip6 multicast scope types + 1)
    /// Number of times that address selection fails.
    pub ip6s_sources_none: u64,
    /// Number of times that an address on the outgoing I/F is chosen.
    pub ip6s_sources_sameif: [u64; 16],
    /// Number of times that an address on a non-outgoing I/F is chosen.
    pub ip6s_sources_otherif: [u64; 16],
    /// Number of times that an address that has the same scope from the destination is
    /// chosen.
    pub ip6s_sources_samescope: [u64; 16],
    /// Number of times that an address that has a different scope from the destination is
    /// chosen.
    pub ip6s_sources_otherscope: [u64; 16],
    /// Number of times that a deprecated address is chosen.
    pub ip6s_sources_deprecated: [u64; 16],
    /// Valid route found in cache.
    pub ip6s_rtcachehit: u64,
    /// Route cache with new destination.
    pub ip6s_rtcachemiss: u64,
    /// Packet received on wrong interface.
    pub ip6s_wrongif: u64,
    /// Lost input due to nobufs, etc.
    pub ip6s_idropped: u64,
}

/// `struct ip6q`: IP6 reassembly queue structure. Each fragment being reassembled is
/// attached to one of these structures.
pub struct Ip6q {
    /// `ip6q_queue`.
    pub ip6q_queue: TailqEntry<Ip6q>,
    /// `ip6q_asfrag`: the fragments.
    pub ip6q_asfrag: ListHead<Ip6asfragList>,
    /// `ip6q_src`.
    pub ip6q_src: Cell<In6Addr>,
    /// `ip6q_dst`.
    pub ip6q_dst: Cell<In6Addr>,
    /// Len of unfragmentable part.
    pub ip6q_unfrglen: Cell<i32>,
    /// # of fragments.
    pub ip6q_nfrag: Cell<i32>,
    /// Routing domain for reassembly.
    pub ip6q_rdomain: Cell<u32>,
    /// Fragment identification.
    pub ip6q_ident: Cell<u32>,
    /// `ip6f_nxt` in first fragment.
    pub ip6q_nxt: Cell<u8>,
    /// `ip6q_ecn`.
    pub ip6q_ecn: Cell<u8>,
    /// Time to live in slowtimo units.
    pub ip6q_ttl: Cell<u8>,
}

queue_adapter!(
    /// `TAILQ_HEAD(ip6q_head, ip6q)` through `ip6q_queue`: `frag6_queue`.
    pub Ip6qList: Ip6q, ip6q_queue => TailqEntry<Ip6q>
);

/// `struct ip6asfrag`: a fragment on a reassembly queue.
pub struct Ip6asfrag {
    /// `ip6af_list`.
    pub ip6af_list: ListEntry<Ip6asfrag>,
    /// `ip6af_m`.
    pub ip6af_m: Cell<Option<&'static Mbuf>>,
    /// Offset in `ip6af_m` to next header.
    pub ip6af_offset: Cell<i32>,
    /// Fragmentable part length.
    pub ip6af_frglen: Cell<i32>,
    /// Fragment offset.
    pub ip6af_off: Cell<i32>,
    /// More fragment bit in frag off.
    pub ip6af_mff: Cell<u16>,
}

queue_adapter!(
    /// `LIST_HEAD(ip6asfrag_list, ip6asfrag)` through `ip6af_list`.
    pub Ip6asfragList: Ip6asfrag, ip6af_list => ListEntry<Ip6asfrag>
);

/// `struct ip6_moptions`: attached to `inpcb.inp_moptions6` and passed to `ip6_output` when
/// IPv6 multicast options are in use.
pub struct Ip6Moptions {
    /// `im6o_memberships`.
    pub im6o_memberships: ListHead<In6MultiMshipList>,
    /// ifp index for outgoing multicasts.
    pub im6o_ifidx: u16,
    /// Hoplimit for outgoing multicasts.
    pub im6o_hlim: u8,
    /// 1 >= hear sends if a member.
    pub im6o_loop: u8,
}

/// `struct ip6_pktopts`: control options for outgoing packets.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Ip6Pktopts {
    /// Hoplimit for outgoing packets.
    pub ip6po_hlim: i32,
    /// Outgoing IF/address information.
    pub ip6po_pktinfo: Option<NonNull<In6Pktinfo>>,
    /// Hop-by-Hop options header.
    pub ip6po_hbh: Option<NonNull<Ip6Hbh>>,
    /// Destination options header (before a routing header).
    pub ip6po_dest1: Option<NonNull<Ip6Dest>>,
    /// Destination options header (after a routing header).
    pub ip6po_dest2: Option<NonNull<Ip6Dest>>,
    /// Traffic class.
    pub ip6po_tclass: i32,
    /// Fragment vs PMTU discovery policy (`IP6PO_MINMTU_*`).
    pub ip6po_minmtu: i32,
    /// `IP6PO_*` flags.
    pub ip6po_flags: i32,
}

/// The number of entries of the next header history.
const NXTHIST: usize = 256;
/// The number of entries of the mbuf chain length history.
const M2M: usize = 32;
/// The number of entries of each source selection history.
const SOURCES: usize = 16;

/// `enum ip6stat_counters`: the per-CPU IPv6 counters, one per word of [`Ip6stat`].
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[repr(usize)]
pub enum Ip6statCounters {
    /// `ip6s_total`.
    Ip6sTotal,
    /// `ip6s_tooshort`.
    Ip6sTooshort,
    /// `ip6s_toosmall`.
    Ip6sToosmall,
    /// `ip6s_fragments`.
    Ip6sFragments,
    /// `ip6s_fragdropped`.
    Ip6sFragdropped,
    /// `ip6s_fragtimeout`.
    Ip6sFragtimeout,
    /// `ip6s_fragoverflow`.
    Ip6sFragoverflow,
    /// `ip6s_forward`.
    Ip6sForward,
    /// `ip6s_cantforward`.
    Ip6sCantforward,
    /// `ip6s_redirectsent`.
    Ip6sRedirectsent,
    /// `ip6s_delivered`.
    Ip6sDelivered,
    /// `ip6s_localout`.
    Ip6sLocalout,
    /// `ip6s_odropped`.
    Ip6sOdropped,
    /// `ip6s_reassembled`.
    Ip6sReassembled,
    /// `ip6s_fragmented`.
    Ip6sFragmented,
    /// `ip6s_ofragments`.
    Ip6sOfragments,
    /// `ip6s_cantfrag`.
    Ip6sCantfrag,
    /// `ip6s_badoptions`.
    Ip6sBadoptions,
    /// `ip6s_noroute`.
    Ip6sNoroute,
    /// `ip6s_badvers`.
    Ip6sBadvers,
    /// `ip6s_rawout`.
    Ip6sRawout,
    /// `ip6s_badscope`.
    Ip6sBadscope,
    /// `ip6s_notmember`.
    Ip6sNotmember,
    /// `ip6s_nxthist`: the first of 256.
    Ip6sNxthist,
    /// `ip6s_m1`.
    Ip6sM1 = 23 + NXTHIST,
    /// `ip6s_m2m`: the first of 32.
    Ip6sM2m,
    /// `ip6s_mext1`.
    Ip6sMext1 = 23 + NXTHIST + 1 + M2M,
    /// `ip6s_mext2m`.
    Ip6sMext2m,
    /// `ip6s_nogif`.
    Ip6sNogif,
    /// `ip6s_toomanyhdr`.
    Ip6sToomanyhdr,
    /// `ip6s_sources_none`.
    Ip6sSourcesNone,
    /// `ip6s_sources_sameif`: the first of 16.
    Ip6sSourcesSameif,
    /// `ip6s_sources_otherif`: the first of 16.
    Ip6sSourcesOtherif = 23 + NXTHIST + 1 + M2M + 5 + SOURCES,
    /// `ip6s_sources_samescope`: the first of 16.
    Ip6sSourcesSamescope = 23 + NXTHIST + 1 + M2M + 5 + 2 * SOURCES,
    /// `ip6s_sources_otherscope`: the first of 16.
    Ip6sSourcesOtherscope = 23 + NXTHIST + 1 + M2M + 5 + 3 * SOURCES,
    /// `ip6s_sources_deprecated`: the first of 16.
    Ip6sSourcesDeprecated = 23 + NXTHIST + 1 + M2M + 5 + 4 * SOURCES,
    /// `ip6s_rtcachehit`.
    Ip6sRtcachehit = 23 + NXTHIST + 1 + M2M + 5 + 5 * SOURCES,
    /// `ip6s_rtcachemiss`.
    Ip6sRtcachemiss,
    /// `ip6s_wrongif`.
    Ip6sWrongif,
    /// `ip6s_idropped`.
    Ip6sIdropped,
    /// `ip6s_ncounters`.
    Ip6sNcounters,
}

/// `ip6s_ncounters`.
pub const IP6S_NCOUNTERS: usize = Ip6statCounters::Ip6sNcounters as usize;

/// `ip6stat_inc(c)`.
pub fn ip6stat_inc(c: Ip6statCounters) {
    IP6COUNTERS[c as usize].fetch_add(1, Ordering::Relaxed);
}

/// `ip6stat_add(c, v)`.
pub fn ip6stat_add(c: Ip6statCounters, v: u64) {
    IP6COUNTERS[c as usize].fetch_add(v, Ordering::Relaxed);
}

/// `ip6stat_inc(base + i)`: bumps entry `i` of the array counter starting at `base`
/// (`ip6s_nxthist`, `ip6s_m2m`, `ip6s_sources_*`).
pub fn ip6stat_inc_idx(base: Ip6statCounters, i: usize) {
    IP6COUNTERS[base as usize + i].fetch_add(1, Ordering::Relaxed);
}

/// `*mtod(m, struct ip6_hdr *)`: the IPv6 header at the start of `m`'s data, as a value.
/// The data may sit at any alignment in the mbuf, so the header is copied out. Panics if the
/// first mbuf is shorter than a header.
pub fn mtod_ip6(m: &Mbuf) -> Ip6Hdr {
    if (m.m_len().get() as usize) < size_of::<Ip6Hdr>() {
        panic(format_args!("mtod_ip6: mbuf shorter than an ip6 header"));
    }
    // SAFETY: the first mbuf holds at least `size_of::<Ip6Hdr>()` bytes (checked above); an
    // `Ip6Hdr` is integers and bytes, valid for any bit pattern.
    unsafe { core::ptr::read_unaligned(mtod::<Ip6Hdr>(m)) }
}

/// Writes `ip6` back as the IPv6 header at the start of `m`'s data (see [`mtod_ip6`]).
pub fn mtod_ip6_store(m: &Mbuf, ip6: &Ip6Hdr) {
    if (m.m_len().get() as usize) < size_of::<Ip6Hdr>() {
        panic(format_args!(
            "mtod_ip6_store: mbuf shorter than an ip6 header"
        ));
    }
    // SAFETY: as in `mtod_ip6`.
    unsafe { core::ptr::write_unaligned(mtod::<Ip6Hdr>(m), *ip6) };
}

// The counters are the statistics' words.
const _: () = assert!(size_of::<Ip6stat>() == IP6S_NCOUNTERS * size_of::<u64>());
/* </CODE> */

/* <TESTS> */
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn counters_match_the_structure() {
        use core::mem::offset_of;
        let w = |off: usize| off / size_of::<u64>();
        assert_eq!(
            w(offset_of!(Ip6stat, ip6s_nxthist)),
            Ip6statCounters::Ip6sNxthist as usize
        );
        assert_eq!(
            w(offset_of!(Ip6stat, ip6s_m1)),
            Ip6statCounters::Ip6sM1 as usize
        );
        assert_eq!(
            w(offset_of!(Ip6stat, ip6s_mext1)),
            Ip6statCounters::Ip6sMext1 as usize
        );
        assert_eq!(
            w(offset_of!(Ip6stat, ip6s_sources_sameif)),
            Ip6statCounters::Ip6sSourcesSameif as usize
        );
        assert_eq!(
            w(offset_of!(Ip6stat, ip6s_sources_deprecated)),
            Ip6statCounters::Ip6sSourcesDeprecated as usize
        );
        assert_eq!(
            w(offset_of!(Ip6stat, ip6s_idropped)),
            Ip6statCounters::Ip6sIdropped as usize
        );
    }

    #[test]
    #[ignore = "needs OPENBSD_SRC (just test-ref)"]
    fn values_match_the_c_header() {
        let defs = crate::reftest::defines("sys/netinet6/ip6_var.h");
        let ipv6 = crate::reftest::assert_defines!(defs;
            IPV6_UNSPECSRC, IPV6_FORWARDING, IPV6_MINMTU, IPV6_REDIRECT, IPV6_FORWARDING_IPSEC);
        crate::reftest::assert_complete(&defs, "IPV6_", &ipv6);
        let po = crate::reftest::assert_defines!(defs;
            IP6PO_MINMTU_MCASTONLY, IP6PO_MINMTU_DISABLE, IP6PO_MINMTU_ALL, IP6PO_DONTFRAG);
        crate::reftest::assert_complete(&defs, "IP6PO_", &po);
    }
}
/* </TESTS> */
