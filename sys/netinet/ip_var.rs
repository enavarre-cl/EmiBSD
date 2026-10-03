/*	$OpenBSD: ip_var.h,v 1.127 2026/08/11 14:28:59 bluhm Exp $	*/
/*	$NetBSD: ip_var.h,v 1.16 1996/02/13 23:43:20 christos Exp $	*/
/* <LICENSES> */
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

//! IP implementation variables: statistics, the header overlay, `ip_output` flags, the
//! multicast options and the reassembly queues: `<netinet/ip_var.h>`.
//!
//! Upstream: sys/netinet/ip_var.h @ 3ce1f3f79392
//!
//! Status: `ported` (M7b).
//!
//! ## Deviations
//! - `ipcounters` (`struct cpumem *`, `<sys/percpu.h>` not ported) is the static array of
//!   atomics `IPCOUNTERS` in `netinet/ip_input.rs`, which defines the C's pointer;
//!   `ipstat_inc`/`ipstat_add` bump it by [`IpstatCounters`], and `ipstat_dec` is
//!   `ip_deliver`'s `counters_dec`.
//! - `struct ip_moptions`'s `imo_membership` is a raw array of `Option<&InMulti>` (it grows
//!   with `imo_max_memberships`, as the C's does); `struct ipqent` and `struct ipq` hold
//!   `Cell`s (they change under `ipq_mutex`); `ipqe_ip` points at the fragment's header in its
//!   mbuf, read unaligned (`netinet/ip_input.rs`).
//! - The globals (`ip_defttl`, `ip_mtudisc`, `ipport_*`, `ip_forwarding`, `ipmultipath`,
//!   `ip_directedbcast`, `la_hold_total`, `ip_mtudisc_timeout_q`, `ipqent_pool`, ...) and the
//!   prototypes (`ip_output`, `ip_input_if`, ...) are defined by `netinet/ip_input.rs`,
//!   `ip_output.rs`, `ip_icmp.rs` and `if_ether.rs`; `rip_usrreqs` and the `rip_*`
//!   functions by `netinet/raw_ip.c`, which is not ported (`netinet/in_proto.rs` has the
//!   `rip_input` stand-in). `struct ipstat` as `sysctl(2)` returns it is here.

use core::cell::Cell;
use core::mem::size_of;
use core::sync::atomic::Ordering;

use crate::netinet::in_::InAddr;
use crate::netinet::in_var::InMulti;
use crate::netinet::ip::Ip;
use crate::netinet::ip_input::IPCOUNTERS;
use crate::queue_adapter;
use crate::sys::mbuf::{Mbuf, mtod};
use crate::sys::queue::{ListEntry, ListHead};
use crate::sys::socket::SO_BROADCAST;

/// Structure stored in mbuf in `inpcb.ip_options` and passed to `ip_output` when ip options
/// are in use: the most option bytes. The actual length of the options (including
/// `ipopt_dst`) is in `m_len`.
pub const MAX_IPOPTLEN: usize = 40;

// Flags passed to ip_output

/// Most of ip header exists.
pub const IP_FORWARDING: i32 = 0x0001;
/// Raw ip header exists.
pub const IP_RAWOUTPUT: i32 = 0x0002;
/// Redirected by pf or source route.
pub const IP_REDIRECT: i32 = 0x0004;
/// Only packets processed by IPsec.
pub const IP_FORWARDING_IPSEC: i32 = 0x0008;
/// Can send broadcast packets.
pub const IP_ALLOWBROADCAST: i32 = SO_BROADCAST;
/// Pmtu discovery, set DF.
pub const IP_MTUDISC: i32 = 0x0800;

/// As per RFC 1191: seconds before a learned path MTU expires.
pub const IPMTUDISCTIMEOUT: i32 = 10 * 60;

/// `struct ipovly`: overlay for ip header used by other protocols (tcp, udp).
#[repr(C)]
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Ipovly {
    /// (unused).
    pub ih_x1: [u8; 9],
    /// Protocol.
    pub ih_pr: u8,
    /// Protocol length.
    pub ih_len: u16,
    /// Source internet address.
    pub ih_src: InAddr,
    /// Destination internet address.
    pub ih_dst: InAddr,
}

/// `struct ipstat`: IP statistics, as `sysctl(2)` returns them.
#[repr(C)]
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Ipstat {
    /// Total packets received.
    pub ips_total: u64,
    /// Checksum bad.
    pub ips_badsum: u64,
    /// Packet too short.
    pub ips_tooshort: u64,
    /// Not enough data.
    pub ips_toosmall: u64,
    /// IP header length < data size.
    pub ips_badhlen: u64,
    /// IP length < ip header length.
    pub ips_badlen: u64,
    /// Fragments received.
    pub ips_fragments: u64,
    /// Frags dropped (dups, out of space).
    pub ips_fragdropped: u64,
    /// Fragments timed out.
    pub ips_fragtimeout: u64,
    /// Packets forwarded.
    pub ips_forward: u64,
    /// Packets rcvd for unreachable dest.
    pub ips_cantforward: u64,
    /// Packets forwarded on same net.
    pub ips_redirectsent: u64,
    /// Unknown or unsupported protocol.
    pub ips_noproto: u64,
    /// Datagrams delivered to upper level.
    pub ips_delivered: u64,
    /// Total ip packets generated here.
    pub ips_localout: u64,
    /// Lost output due to nobufs, etc.
    pub ips_odropped: u64,
    /// Total packets reassembled ok.
    pub ips_reassembled: u64,
    /// Datagrams successfully fragmented.
    pub ips_fragmented: u64,
    /// Output fragments created.
    pub ips_ofragments: u64,
    /// Don't fragment flag was set, etc.
    pub ips_cantfrag: u64,
    /// Error in option processing.
    pub ips_badoptions: u64,
    /// Packets discarded due to no route.
    pub ips_noroute: u64,
    /// IP version != 4.
    pub ips_badvers: u64,
    /// Total raw ip packets generated.
    pub ips_rawout: u64,
    /// Malformed fragments (bad length).
    pub ips_badfrags: u64,
    /// Frags dropped for lack of memory.
    pub ips_rcvmemdrop: u64,
    /// IP length > max ip packet size.
    pub ips_toolong: u64,
    /// No match gif found.
    pub ips_nogif: u64,
    /// Invalid address on header.
    pub ips_badaddr: u64,
    /// Software checksummed on input.
    pub ips_inswcsum: u64,
    /// Software checksummed on output.
    pub ips_outswcsum: u64,
    /// Multicasts for unregistered groups.
    pub ips_notmember: u64,
    /// Valid route found in cache.
    pub ips_rtcachehit: u64,
    /// Route cache with new destination.
    pub ips_rtcachemiss: u64,
    /// Packet received on wrong interface.
    pub ips_wrongif: u64,
    /// Lost input due to nobufs, etc.
    pub ips_idropped: u64,
}

/// `struct ipoption`: the options of an outgoing packet.
#[repr(C)]
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Ipoption {
    /// First-hop dst if source routed.
    pub ipopt_dst: InAddr,
    /// Options proper.
    pub ipopt_list: [i8; MAX_IPOPTLEN],
}

/// `enum ipstat_counters`: the per-CPU IP counters, one per field of [`Ipstat`] in the same
/// order.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[repr(u32)]
pub enum IpstatCounters {
    /// Total packets received.
    IpsTotal,
    /// Checksum bad.
    IpsBadsum,
    /// Packet too short.
    IpsTooshort,
    /// Not enough data.
    IpsToosmall,
    /// IP header length < data size.
    IpsBadhlen,
    /// IP length < ip header length.
    IpsBadlen,
    /// Fragments received.
    IpsFragments,
    /// Frags dropped (dups, out of space).
    IpsFragdropped,
    /// Fragments timed out.
    IpsFragtimeout,
    /// Packets forwarded.
    IpsForward,
    /// Packets rcvd for unreachable dest.
    IpsCantforward,
    /// Packets forwarded on same net.
    IpsRedirectsent,
    /// Unknown or unsupported protocol.
    IpsNoproto,
    /// Datagrams delivered to upper level.
    IpsDelivered,
    /// Total ip packets generated here.
    IpsLocalout,
    /// Lost output packets due to nobufs, etc.
    IpsOdropped,
    /// Total packets reassembled ok.
    IpsReassembled,
    /// Datagrams successfully fragmented.
    IpsFragmented,
    /// Output fragments created.
    IpsOfragments,
    /// Don't fragment flag was set, etc.
    IpsCantfrag,
    /// Error in option processing.
    IpsBadoptions,
    /// Packets discarded due to no route.
    IpsNoroute,
    /// IP version != 4.
    IpsBadvers,
    /// Total raw ip packets generated.
    IpsRawout,
    /// Malformed fragments (bad length).
    IpsBadfrags,
    /// Frags dropped for lack of memory.
    IpsRcvmemdrop,
    /// IP length > max ip packet size.
    IpsToolong,
    /// No match gif found.
    IpsNogif,
    /// Invalid address on header.
    IpsBadaddr,
    /// Software checksummed on input.
    IpsInswcsum,
    /// Software checksummed on output.
    IpsOutswcsum,
    /// Multicasts for unregistered groups.
    IpsNotmember,
    /// Valid route to destination found in cache.
    IpsRtcachehit,
    /// Route cache filled with new destination.
    IpsRtcachemiss,
    /// Packet received on wrong interface.
    IpsWrongif,
    /// Lost input packets due to nobufs, etc.
    IpsIdropped,
    /// The number of counters.
    IpsNcounters,
}

/// `struct ip_moptions`: attached to `inpcb.ip_moptions` and passed to `ip_output` when IP
/// multicast options are in use.
pub struct IpMoptions {
    /// `imo_membership`: group memberships, `imo_max_memberships` slots.
    pub imo_membership: *mut Option<&'static InMulti>,
    /// `imo_ifidx`: ifp index for outgoing multicasts.
    pub imo_ifidx: u16,
    /// `imo_ttl`: TTL for outgoing multicasts.
    pub imo_ttl: u8,
    /// `imo_loop`: 1 => hear sends if a member.
    pub imo_loop: u8,
    /// `imo_num_memberships`: no. memberships this socket.
    pub imo_num_memberships: u16,
    /// `imo_max_memberships`: max memberships this socket.
    pub imo_max_memberships: u16,
}

/// `struct ipqent`: a fragment on a reassembly queue.
pub struct Ipqent {
    /// `ipqe_q`.
    pub ipqe_q: ListEntry<Ipqent>,
    /// `ipqe_ip`: the fragment's IP header, in `ipqe_m`.
    pub ipqe_ip: Cell<*mut Ip>,
    /// `ipqe_m`: mbuf contains packet.
    pub ipqe_m: Cell<Option<&'static Mbuf>>,
    /// `ipqe_mff`: for IP fragmentation.
    pub ipqe_mff: Cell<u16>,
}

queue_adapter!(
    /// `LIST_HEAD(ipqehead, ipqent)`: a datagram's fragments.
    pub Ipqehead: Ipqent, ipqe_q => ListEntry<Ipqent>
);

/// `struct ipq`: an IP reassembly queue. Each fragment being reassembled is attached to one
/// of these structures. They are timed out after `ipq_ttl` drops to 0, and may also be
/// reclaimed if memory becomes tight.
pub struct Ipq {
    /// `ipq_q`: to other reass headers.
    pub ipq_q: ListEntry<Ipq>,
    /// `ipq_rdomain`: routing domain for reassembly.
    pub ipq_rdomain: Cell<u32>,
    /// `ipq_id`: sequence id for reassembly.
    pub ipq_id: Cell<u16>,
    /// `ipq_ttl`: time for reass q to live.
    pub ipq_ttl: Cell<u8>,
    /// `ipq_p`: protocol of this fragment.
    pub ipq_p: Cell<u8>,
    /// `ipq_fragq`: to ip fragment queue.
    pub ipq_fragq: ListHead<Ipqehead>,
    /// `ipq_src`.
    pub ipq_src: Cell<InAddr>,
    /// `ipq_dst`.
    pub ipq_dst: Cell<InAddr>,
}

queue_adapter!(
    /// `LIST_HEAD(, ipq) ipq`: the reassembly queues.
    pub IpqList: Ipq, ipq_q => ListEntry<Ipq>
);

/// `struct ipoffnxt`: a fragment's offset and next header.
#[repr(C)]
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Ipoffnxt {
    /// Offset.
    pub ion_off: i32,
    /// Next header.
    pub ion_nxt: i32,
}

/// `ipstat_inc(c)`.
pub fn ipstat_inc(c: IpstatCounters) {
    IPCOUNTERS[c as usize].fetch_add(1, Ordering::Relaxed);
}

/// `ipstat_add(c, v)`.
pub fn ipstat_add(c: IpstatCounters, v: u64) {
    IPCOUNTERS[c as usize].fetch_add(v, Ordering::Relaxed);
}

/// `counters_dec(ipcounters, c)` (`ip_deliver` takes back a delivery it queues).
pub fn ipstat_dec(c: IpstatCounters) {
    IPCOUNTERS[c as usize].fetch_sub(1, Ordering::Relaxed);
}

/// `*mtod(m, struct ip *)`: the IP header at the start of `m`'s data, as a value. The data
/// may sit at any alignment in the mbuf, so the header is copied out (`docs/C_TO_RUST.md`).
/// Panics if the first mbuf is shorter than a header.
pub fn mtod_ip(m: &Mbuf) -> Ip {
    if (m.m_len().get() as usize) < size_of::<Ip>() {
        crate::kern::subr_prf::panic(format_args!("mtod_ip: mbuf shorter than an ip header"));
    }
    // SAFETY: the first mbuf holds at least `size_of::<Ip>()` bytes (checked above); an `Ip`
    // is integers, valid for any bytes.
    unsafe { core::ptr::read_unaligned(mtod::<Ip>(m)) }
}

/// Writes `ip` back as the IP header at the start of `m`'s data (see [`mtod_ip`]).
pub fn mtod_ip_store(m: &Mbuf, ip: &Ip) {
    if (m.m_len().get() as usize) < size_of::<Ip>() {
        crate::kern::subr_prf::panic(format_args!(
            "mtod_ip_store: mbuf shorter than an ip header"
        ));
    }
    // SAFETY: as in `mtod_ip`.
    unsafe { core::ptr::write_unaligned(mtod::<Ip>(m), *ip) };
}

// LP64 sizes of the C structures.
const _: () = {
    assert!(size_of::<Ipovly>() == 20);
    assert!(size_of::<Ipstat>() == 36 * 8);
    assert!(size_of::<Ipstat>() == IpstatCounters::IpsNcounters as usize * 8);
    assert!(size_of::<Ipoption>() == 44);
    assert!(size_of::<Ipoffnxt>() == 8);
};

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn ipovly_overlays_the_ip_header() {
        use crate::netinet::ip::Ip;
        assert_eq!(size_of::<Ipovly>(), size_of::<Ip>());
        assert_eq!(
            core::mem::offset_of!(Ipovly, ih_src),
            core::mem::offset_of!(Ip, ip_src)
        );
        assert_eq!(
            core::mem::offset_of!(Ipovly, ih_pr),
            core::mem::offset_of!(Ip, ip_p)
        );
    }

    #[test]
    #[ignore = "needs OPENBSD_SRC (just test-ref)"]
    fn values_match_the_c_header() {
        let defs = crate::reftest::defines("sys/netinet/ip_var.h");
        let ip = crate::reftest::assert_defines!(defs;
            IP_FORWARDING, IP_RAWOUTPUT, IP_REDIRECT, IP_FORWARDING_IPSEC, IP_MTUDISC,
            MAX_IPOPTLEN, IPMTUDISCTIMEOUT);
        assert_eq!(defs["IP_ALLOWBROADCAST"], "SO_BROADCAST");
        crate::reftest::assert_complete(&defs, "IP_", &[&ip[..], &["IP_ALLOWBROADCAST"]].concat());
    }
}
