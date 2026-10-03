/*	$OpenBSD: in.h,v 1.149 2025/03/02 21:28:32 bluhm Exp $	*/
/*	$NetBSD: in.h,v 1.20 1996/02/13 23:41:47 christos Exp $	*/
/* <LICENSES> */
/*
 * Copyright (c) 1982, 1986, 1990, 1993
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
 */
/* </LICENSES> */

//! Constants and structures defined by the internet system, per RFC 790, September 1981, and
//! numerous additions: `<netinet/in.h>`.
//!
//! Upstream: sys/netinet/in.h @ 3ce1f3f79392
//!
//! `in` is a Rust keyword, so the module is `in_` (`docs/C_TO_RUST.md`).
//!
//! Byte order: as in the C kernel, addresses and ports stay in network order wherever they are
//! stored (`in_addr.s_addr`, `sin_port`). The kernel's `__IPADDR(x)` is `htonl(x)`: "by
//! byte-swapping the constants, we avoid ever having to byte-swap IP addresses inside the
//! kernel", so `INADDR_*`, `IN_CLASS*_NET`/`_HOST` and the `in_class*` tests are network-order
//! values and take network-order arguments. (User-level programs rely on these macros not
//! doing byte-swapping; this is the kernel's half.)
//!
//! Status: `wip`.
//!
//! ## Deviations
//! - `CTL_IPPROTO_NAMES` and `IPCTL_NAMES` (`struct ctlname` tables) come with
//!   `<sys/sysctl.h>`.
//! - `ifatoia` comes with `<netinet/in_var.h>` (`struct in_ifaddr`); `inetctlerrmap` and
//!   `zeroin_addr` with the `.c` files that define them, as do the prototypes (`in_cksum`,
//!   `in_broadcast`, `ipv4_input`, ...).
//! - The `#include <netinet6/in6.h>` at the end of the C header is not mirrored: `in6.h` is a
//!   module of its own when it is ported.
//! - `INADDR_NONE` (userland only), `htons` and friends outside the kernel, and the userland
//!   prototypes (`bindresvport`) are not kernel material; the kernel's byte-order functions
//!   are in `sys/endian.rs`.
//! - `struct in_addr`'s `s_addr` is a `u32`: `in_addr_t` is `crate::sys::types::InAddr`, a
//!   name this struct also needs.
//! - `satosin`, `satosin_const` and `sintosa` are pointer casts (`docs/C_TO_RUST.md`);
//!   `in_hosteq` and `in_nullhost` are `const fn`s.

use core::mem::size_of;

use crate::sys::endian::htonl;
use crate::sys::socket::Sockaddr;
use crate::sys::types::{InPort, SaFamily};

// Protocols

/// Dummy for IP.
pub const IPPROTO_IP: i32 = 0;
/// Hop-by-hop option header.
pub const IPPROTO_HOPOPTS: i32 = IPPROTO_IP;
/// Control message protocol.
pub const IPPROTO_ICMP: i32 = 1;
/// Group mgmt protocol.
pub const IPPROTO_IGMP: i32 = 2;
/// Gateway^2 (deprecated).
pub const IPPROTO_GGP: i32 = 3;
/// IP inside IP.
pub const IPPROTO_IPIP: i32 = 4;
/// IP inside IP.
pub const IPPROTO_IPV4: i32 = IPPROTO_IPIP;
/// TCP.
pub const IPPROTO_TCP: i32 = 6;
/// Exterior gateway protocol.
pub const IPPROTO_EGP: i32 = 8;
/// Pup.
pub const IPPROTO_PUP: i32 = 12;
/// User datagram protocol.
pub const IPPROTO_UDP: i32 = 17;
/// XNS IDP.
pub const IPPROTO_IDP: i32 = 22;
/// TP-4 w/ class negotiation.
pub const IPPROTO_TP: i32 = 29;
/// IPv6 in IPv6.
pub const IPPROTO_IPV6: i32 = 41;
/// Routing header.
pub const IPPROTO_ROUTING: i32 = 43;
/// Fragmentation/reassembly header.
pub const IPPROTO_FRAGMENT: i32 = 44;
/// Resource reservation.
pub const IPPROTO_RSVP: i32 = 46;
/// GRE encap, RFCs 1701/1702.
pub const IPPROTO_GRE: i32 = 47;
/// Encap. Security Payload.
pub const IPPROTO_ESP: i32 = 50;
/// Authentication header.
pub const IPPROTO_AH: i32 = 51;
/// IP Mobility, RFC 2004.
pub const IPPROTO_MOBILE: i32 = 55;
/// ICMP for IPv6.
pub const IPPROTO_ICMPV6: i32 = 58;
/// No next header.
pub const IPPROTO_NONE: i32 = 59;
/// Destination options header.
pub const IPPROTO_DSTOPTS: i32 = 60;
/// ISO cnlp.
pub const IPPROTO_EON: i32 = 80;
/// Ethernet in IPv4.
pub const IPPROTO_ETHERIP: i32 = 97;
/// Encapsulation header.
pub const IPPROTO_ENCAP: i32 = 98;
/// Protocol indep. multicast.
pub const IPPROTO_PIM: i32 = 103;
/// IP Payload Comp. Protocol.
pub const IPPROTO_IPCOMP: i32 = 108;
/// CARP.
pub const IPPROTO_CARP: i32 = 112;
/// SCTP, RFC 4960.
pub const IPPROTO_SCTP: i32 = 132;
/// UDP-Lite, RFC 3828.
pub const IPPROTO_UDPLITE: i32 = 136;
/// Unicast MPLS packet.
pub const IPPROTO_MPLS: i32 = 137;
/// PFSYNC.
pub const IPPROTO_PFSYNC: i32 = 240;
/// Raw IP packet.
pub const IPPROTO_RAW: i32 = 255;

/// One past the highest IP protocol number.
pub const IPPROTO_MAX: i32 = 256;

/// Divert sockets. Only used internally, so it can be outside the range of valid IP
/// protocols.
pub const IPPROTO_DIVERT: i32 = 258;

// Local port number conventions (from FreeBSD): when a user does a bind(2) or connect(2) with
// a port number of zero, a non-conflicting local port address is chosen, by default between
// IPPORT_RESERVED and IPPORT_USERRESERVED; IP_PORTRANGE changes the range. Ports below
// IPPORT_RESERVED are reserved for privileged processes (e.g. root); ports above
// IPPORT_USERRESERVED are reserved for servers, not necessarily privileged.

/// First unprivileged port.
pub const IPPORT_RESERVED: i32 = 1024;
/// Last port of the default range.
pub const IPPORT_USERRESERVED: i32 = 49151;

/// Default local port range to use by setting `IP_PORTRANGE_HIGH`: first.
pub const IPPORT_HIFIRSTAUTO: i32 = 49152;
/// Default local port range to use by setting `IP_PORTRANGE_HIGH`: last.
pub const IPPORT_HILASTAUTO: i32 = 65535;

/// Last return value of `*_input()`, meaning "all job for this pkt is done".
pub const IPPROTO_DONE: i32 = 257;

/// `IN_CLASSA_NET`.
pub const IN_CLASSA_NET: u32 = htonl(0xff00_0000);
/// `IN_CLASSA_NSHIFT`.
pub const IN_CLASSA_NSHIFT: u32 = 24;
/// `IN_CLASSA_HOST`.
pub const IN_CLASSA_HOST: u32 = htonl(0x00ff_ffff);
/// `IN_CLASSA_MAX`.
pub const IN_CLASSA_MAX: u32 = 128;

/// `IN_CLASSB_NET`.
pub const IN_CLASSB_NET: u32 = htonl(0xffff_0000);
/// `IN_CLASSB_NSHIFT`.
pub const IN_CLASSB_NSHIFT: u32 = 16;
/// `IN_CLASSB_HOST`.
pub const IN_CLASSB_HOST: u32 = htonl(0x0000_ffff);
/// `IN_CLASSB_MAX`.
pub const IN_CLASSB_MAX: u32 = 65536;

/// `IN_CLASSC_NET`.
pub const IN_CLASSC_NET: u32 = htonl(0xffff_ff00);
/// `IN_CLASSC_NSHIFT`.
pub const IN_CLASSC_NSHIFT: u32 = 8;
/// `IN_CLASSC_HOST`.
pub const IN_CLASSC_HOST: u32 = htonl(0x0000_00ff);

/// Not really a net field, but routing needn't know.
pub const IN_CLASSD_NET: u32 = htonl(0xf000_0000);
/// `IN_CLASSD_NSHIFT`.
pub const IN_CLASSD_NSHIFT: u32 = 28;
/// Not really a host field, but routing needn't know.
pub const IN_CLASSD_HOST: u32 = htonl(0x0fff_ffff);

/// `IN_RFC3021_NET`: the mask of a /31 point-to-point subnet.
pub const IN_RFC3021_NET: u32 = htonl(0xffff_fffe);
/// `IN_RFC3021_NSHIFT`.
pub const IN_RFC3021_NSHIFT: u32 = 31;
/// `IN_RFC3021_HOST`.
pub const IN_RFC3021_HOST: u32 = htonl(0x0000_0001);

/// 0.0.0.0.
pub const INADDR_ANY: u32 = htonl(0x0000_0000);
/// 127.0.0.1.
pub const INADDR_LOOPBACK: u32 = htonl(0x7f00_0001);
/// 255.255.255.255 (must be masked).
pub const INADDR_BROADCAST: u32 = htonl(0xffff_ffff);

/// 224.0.0.0.
pub const INADDR_UNSPEC_GROUP: u32 = htonl(0xe000_0000);
/// 224.0.0.1.
pub const INADDR_ALLHOSTS_GROUP: u32 = htonl(0xe000_0001);
/// 224.0.0.2.
pub const INADDR_ALLROUTERS_GROUP: u32 = htonl(0xe000_0002);
/// 224.0.0.18.
pub const INADDR_CARP_GROUP: u32 = htonl(0xe000_0012);
/// 224.0.0.240.
pub const INADDR_PFSYNC_GROUP: u32 = htonl(0xe000_00f0);
/// 224.0.0.255.
pub const INADDR_MAX_LOCAL_GROUP: u32 = htonl(0xe000_00ff);

/// Official!
pub const IN_LOOPBACKNET: u32 = 127;

// Options for use with [gs]etsockopt at the IP level. First word of comment is data type;
// bool is stored in int.

/// buf/ip_opts; set/get IP options.
pub const IP_OPTIONS: i32 = 1;
/// int; header is included with data.
pub const IP_HDRINCL: i32 = 2;
/// int; IP type of service and preced.
pub const IP_TOS: i32 = 3;
/// int; IP time to live.
pub const IP_TTL: i32 = 4;
/// bool; receive all IP opts w/dgram.
pub const IP_RECVOPTS: i32 = 5;
/// bool; receive IP opts for response.
pub const IP_RECVRETOPTS: i32 = 6;
/// bool; receive IP dst addr w/dgram.
pub const IP_RECVDSTADDR: i32 = 7;
/// ip_opts; set/get IP options.
pub const IP_RETOPTS: i32 = 8;
/// in_addr; set/get IP multicast i/f.
pub const IP_MULTICAST_IF: i32 = 9;
/// u_char; set/get IP multicast ttl.
pub const IP_MULTICAST_TTL: i32 = 10;
/// u_char; set/get IP multicast loopback.
pub const IP_MULTICAST_LOOP: i32 = 11;
/// ip_mreq; add an IP group membership.
pub const IP_ADD_MEMBERSHIP: i32 = 12;
/// ip_mreq; drop an IP group membership.
pub const IP_DROP_MEMBERSHIP: i32 = 13;
/// int; range to choose for unspec port.
pub const IP_PORTRANGE: i32 = 19;
/// int; authentication used.
pub const IP_AUTH_LEVEL: i32 = 20;
/// int; transport encryption.
pub const IP_ESP_TRANS_LEVEL: i32 = 21;
/// int; full-packet encryption.
pub const IP_ESP_NETWORK_LEVEL: i32 = 22;
/// buf; IPsec local ID.
pub const IP_IPSEC_LOCAL_ID: i32 = 23;
/// buf; IPsec remote ID.
pub const IP_IPSEC_REMOTE_ID: i32 = 24;
/// buf; was: IPsec local credentials.
pub const IP_IPSEC_LOCAL_CRED: i32 = 25;
/// buf; was: IPsec remote credentials.
pub const IP_IPSEC_REMOTE_CRED: i32 = 26;
/// buf; was: IPsec local auth material.
pub const IP_IPSEC_LOCAL_AUTH: i32 = 27;
/// buf; was: IPsec remote auth material.
pub const IP_IPSEC_REMOTE_AUTH: i32 = 28;
/// int; compression used.
pub const IP_IPCOMP_LEVEL: i32 = 29;
/// bool; receive reception if w/dgram.
pub const IP_RECVIF: i32 = 30;
/// bool; receive IP TTL w/dgram.
pub const IP_RECVTTL: i32 = 31;
/// Minimum TTL for packet or drop.
pub const IP_MINTTL: i32 = 32;
/// bool; receive IP dst port w/dgram.
pub const IP_RECVDSTPORT: i32 = 33;
/// bool; using PIPEX.
pub const IP_PIPEX: i32 = 34;
/// bool; receive rdomain w/dgram.
pub const IP_RECVRTABLE: i32 = 35;
/// bool; IPsec flow info for dgram.
pub const IP_IPSECFLOWINFO: i32 = 36;
/// int; IP TTL system default.
pub const IP_IPDEFTTL: i32 = 37;
/// struct in_addr; source address to use.
pub const IP_SENDSRCADDR: i32 = IP_RECVDSTADDR;

/// int; routing table, see `SO_RTABLE`.
pub const IP_RTABLE: i32 = 0x1021;

// Security levels - IPsec, not IPSO

/// Bypass policy altogether.
pub const IPSEC_LEVEL_BYPASS: i32 = 0x00;
/// Send clear, accept any.
pub const IPSEC_LEVEL_NONE: i32 = 0x00;
/// Send secure if SA available.
pub const IPSEC_LEVEL_AVAIL: i32 = 0x01;
/// Send secure, accept any.
pub const IPSEC_LEVEL_USE: i32 = 0x02;
/// Require secure inbound, also use.
pub const IPSEC_LEVEL_REQUIRE: i32 = 0x03;
/// Use outbound SA that is unique.
pub const IPSEC_LEVEL_UNIQUE: i32 = 0x04;
/// `IPSEC_LEVEL_DEFAULT`.
pub const IPSEC_LEVEL_DEFAULT: i32 = IPSEC_LEVEL_AVAIL;

/// `IPSEC_AUTH_LEVEL_DEFAULT`.
pub const IPSEC_AUTH_LEVEL_DEFAULT: i32 = IPSEC_LEVEL_DEFAULT;
/// `IPSEC_ESP_TRANS_LEVEL_DEFAULT`.
pub const IPSEC_ESP_TRANS_LEVEL_DEFAULT: i32 = IPSEC_LEVEL_DEFAULT;
/// `IPSEC_ESP_NETWORK_LEVEL_DEFAULT`.
pub const IPSEC_ESP_NETWORK_LEVEL_DEFAULT: i32 = IPSEC_LEVEL_DEFAULT;
/// `IPSEC_IPCOMP_LEVEL_DEFAULT`.
pub const IPSEC_IPCOMP_LEVEL_DEFAULT: i32 = IPSEC_LEVEL_DEFAULT;

// Defaults and limits for options

/// Normally limit m'casts to 1 hop.
pub const IP_DEFAULT_MULTICAST_TTL: u8 = 1;
/// Normally hear sends if a member.
pub const IP_DEFAULT_MULTICAST_LOOP: u8 = 1;
/// The `imo_membership` vector for each socket starts at `IP_MIN_MEMBERSHIPS` and is
/// dynamically allocated at run-time, bounded by `IP_MAX_MEMBERSHIPS`, and is reallocated
/// when needed, sized according to a power-of-two increment.
pub const IP_MIN_MEMBERSHIPS: u16 = 15;
/// Upper bound of the `imo_membership` vector.
pub const IP_MAX_MEMBERSHIPS: u16 = 4095;

// Argument for IP_PORTRANGE: which range to search when port is unspecified at bind() or
// connect().

/// Default range.
pub const IP_PORTRANGE_DEFAULT: i32 = 0;
/// "high" - request firewall bypass.
pub const IP_PORTRANGE_HIGH: i32 = 1;
/// "low" - vouchsafe security.
pub const IP_PORTRANGE_LOW: i32 = 2;

/// Buffer length for strings containing printable IP addresses.
pub const INET_ADDRSTRLEN: usize = 16;

// Definitions for inet sysctl operations. Third level is protocol number; fourth level is
// desired variable within that protocol.

/// Don't list to `IPPROTO_MAX`.
pub const IPPROTO_MAXID: i32 = IPPROTO_DIVERT + 1;

// Names for IP sysctl objects

/// Act as router.
pub const IPCTL_FORWARDING: i32 = 1;
/// May send redirects when forwarding.
pub const IPCTL_SENDREDIRECTS: i32 = 2;
/// Default TTL.
pub const IPCTL_DEFTTL: i32 = 3;
/// May perform source routes.
pub const IPCTL_SOURCEROUTE: i32 = 5;
/// Default broadcast behavior.
pub const IPCTL_DIRECTEDBCAST: i32 = 6;
/// `IPCTL_IPPORT_FIRSTAUTO`.
pub const IPCTL_IPPORT_FIRSTAUTO: i32 = 7;
/// `IPCTL_IPPORT_LASTAUTO`.
pub const IPCTL_IPPORT_LASTAUTO: i32 = 8;
/// `IPCTL_IPPORT_HIFIRSTAUTO`.
pub const IPCTL_IPPORT_HIFIRSTAUTO: i32 = 9;
/// `IPCTL_IPPORT_HILASTAUTO`.
pub const IPCTL_IPPORT_HILASTAUTO: i32 = 10;
/// `IPCTL_IPPORT_MAXQUEUE`.
pub const IPCTL_IPPORT_MAXQUEUE: i32 = 11;
/// `IPCTL_ENCDEBUG`.
pub const IPCTL_ENCDEBUG: i32 = 12;
/// `IPCTL_IPSEC_STATS`.
pub const IPCTL_IPSEC_STATS: i32 = 13;
/// How long to wait for key mgmt.
pub const IPCTL_IPSEC_EXPIRE_ACQUIRE: i32 = 14;
/// New SA lifetime.
pub const IPCTL_IPSEC_EMBRYONIC_SA_TIMEOUT: i32 = 15;
/// `IPCTL_IPSEC_REQUIRE_PFS`.
pub const IPCTL_IPSEC_REQUIRE_PFS: i32 = 16;
/// `IPCTL_IPSEC_SOFT_ALLOCATIONS`.
pub const IPCTL_IPSEC_SOFT_ALLOCATIONS: i32 = 17;
/// `IPCTL_IPSEC_ALLOCATIONS`.
pub const IPCTL_IPSEC_ALLOCATIONS: i32 = 18;
/// `IPCTL_IPSEC_SOFT_BYTES`.
pub const IPCTL_IPSEC_SOFT_BYTES: i32 = 19;
/// `IPCTL_IPSEC_BYTES`.
pub const IPCTL_IPSEC_BYTES: i32 = 20;
/// `IPCTL_IPSEC_TIMEOUT`.
pub const IPCTL_IPSEC_TIMEOUT: i32 = 21;
/// `IPCTL_IPSEC_SOFT_TIMEOUT`.
pub const IPCTL_IPSEC_SOFT_TIMEOUT: i32 = 22;
/// `IPCTL_IPSEC_SOFT_FIRSTUSE`.
pub const IPCTL_IPSEC_SOFT_FIRSTUSE: i32 = 23;
/// `IPCTL_IPSEC_FIRSTUSE`.
pub const IPCTL_IPSEC_FIRSTUSE: i32 = 24;
/// `IPCTL_IPSEC_ENC_ALGORITHM`.
pub const IPCTL_IPSEC_ENC_ALGORITHM: i32 = 25;
/// `IPCTL_IPSEC_AUTH_ALGORITHM`.
pub const IPCTL_IPSEC_AUTH_ALGORITHM: i32 = 26;
/// Allow path MTU discovery.
pub const IPCTL_MTUDISC: i32 = 27;
/// Allow path MTU discovery.
pub const IPCTL_MTUDISCTIMEOUT: i32 = 28;
/// `IPCTL_IPSEC_IPCOMP_ALGORITHM`.
pub const IPCTL_IPSEC_IPCOMP_ALGORITHM: i32 = 29;
/// `IPCTL_IFQUEUE`.
pub const IPCTL_IFQUEUE: i32 = 30;
/// `IPCTL_MFORWARDING`.
pub const IPCTL_MFORWARDING: i32 = 31;
/// `IPCTL_MULTIPATH`.
pub const IPCTL_MULTIPATH: i32 = 32;
/// IP statistics.
pub const IPCTL_STATS: i32 = 33;
/// Type of multicast.
pub const IPCTL_MRTPROTO: i32 = 34;
/// `IPCTL_MRTSTATS`.
pub const IPCTL_MRTSTATS: i32 = 35;
/// `IPCTL_ARPQUEUED`.
pub const IPCTL_ARPQUEUED: i32 = 36;
/// `IPCTL_MRTMFC`.
pub const IPCTL_MRTMFC: i32 = 37;
/// `IPCTL_MRTVIF`.
pub const IPCTL_MRTVIF: i32 = 38;
/// `IPCTL_ARPTIMEOUT`.
pub const IPCTL_ARPTIMEOUT: i32 = 39;
/// `IPCTL_ARPDOWN`.
pub const IPCTL_ARPDOWN: i32 = 40;
/// `IPCTL_ARPQUEUE`.
pub const IPCTL_ARPQUEUE: i32 = 41;
/// `IPCTL_MAXID`.
pub const IPCTL_MAXID: i32 = 42;

/// `struct in_addr`: IP Version 4 Internet address (a structure for historical reasons), in
/// network order.
#[repr(C)]
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash)]
pub struct InAddr {
    /// The address (`in_addr_t`), network order.
    pub s_addr: u32,
}

/// `struct sockaddr_in`: IP Version 4 socket address.
#[repr(C)]
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct SockaddrIn {
    /// Total length.
    pub sin_len: u8,
    /// `AF_INET`.
    pub sin_family: SaFamily,
    /// Port, network order.
    pub sin_port: InPort,
    /// Address.
    pub sin_addr: InAddr,
    /// Zero.
    pub sin_zero: [i8; 8],
}

/// `struct ip_opts`: structure used to describe IP options. Used to store options internally,
/// to pass them to a process, or to restore options retrieved earlier. The `ip_dst` is used
/// for the first-hop gateway when using a source route (this gets put into the header proper).
#[repr(C)]
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct IpOpts {
    /// First hop, 0 w/o src rt.
    pub ip_dst: InAddr,
    /// Actually variable in size.
    pub ip_opts: [i8; 40],
}

/// `struct ip_mreq`: argument structure for `IP_ADD_MEMBERSHIP` and `IP_DROP_MEMBERSHIP`.
#[repr(C)]
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct IpMreq {
    /// IP multicast address of group.
    pub imr_multiaddr: InAddr,
    /// Local IP address of interface.
    pub imr_interface: InAddr,
}

/// `struct ip_mreqn`: `ip_mreq` with an interface index.
#[repr(C)]
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct IpMreqn {
    /// IP multicast address of group.
    pub imr_multiaddr: InAddr,
    /// Local IP address of interface.
    pub imr_address: InAddr,
    /// Interface index.
    pub imr_ifindex: i32,
}

/// `IN_CLASSA(i)`: whether network-order address `i` is class A.
pub const fn in_classa(i: u32) -> bool {
    i & htonl(0x8000_0000) == htonl(0x0000_0000)
}

/// `IN_CLASSB(i)`: whether network-order address `i` is class B.
pub const fn in_classb(i: u32) -> bool {
    i & htonl(0xc000_0000) == htonl(0x8000_0000)
}

/// `IN_CLASSC(i)`: whether network-order address `i` is class C.
pub const fn in_classc(i: u32) -> bool {
    i & htonl(0xe000_0000) == htonl(0xc000_0000)
}

/// `IN_CLASSD(i)`: whether network-order address `i` is class D (multicast).
pub const fn in_classd(i: u32) -> bool {
    i & htonl(0xf000_0000) == htonl(0xe000_0000)
}

/// `IN_MULTICAST(i)`: whether network-order address `i` is multicast.
pub const fn in_multicast(i: u32) -> bool {
    in_classd(i)
}

/// `IN_RFC3021_SUBNET(n)`: whether netmask `n` is a /31.
pub const fn in_rfc3021_subnet(n: u32) -> bool {
    n & IN_RFC3021_NET == IN_RFC3021_NET
}

/// `IN_EXPERIMENTAL(i)`: whether network-order address `i` is in 240.0.0.0/4.
pub const fn in_experimental(i: u32) -> bool {
    i & htonl(0xf000_0000) == htonl(0xf000_0000)
}

/// `IN_BADCLASS(i)`: whether network-order address `i` is in 240.0.0.0/4.
pub const fn in_badclass(i: u32) -> bool {
    i & htonl(0xf000_0000) == htonl(0xf000_0000)
}

/// `IN_LOCAL_GROUP(i)`: whether network-order address `i` is in 224.0.0.0/24.
pub const fn in_local_group(i: u32) -> bool {
    i & htonl(0xffff_ff00) == htonl(0xe000_0000)
}

/// `IN_CLASSFULBROADCAST(i, b)`: whether `i` is the classful broadcast address of `b`, both
/// network order.
pub const fn in_classfulbroadcast(i: u32, b: u32) -> bool {
    (in_classc(b) && (b | IN_CLASSC_HOST) == i)
        || (in_classb(b) && (b | IN_CLASSB_HOST) == i)
        || (in_classa(b) && (b | IN_CLASSA_HOST) == i)
}

/// `in_hosteq(s, t)`: whether two addresses are equal.
pub const fn in_hosteq(s: InAddr, t: InAddr) -> bool {
    s.s_addr == t.s_addr
}

/// `in_nullhost(x)`: whether `x` is `INADDR_ANY`.
pub const fn in_nullhost(x: InAddr) -> bool {
    x.s_addr == INADDR_ANY
}

/// `satosin(sa)`: a generic `sockaddr` seen as a `sockaddr_in`.
pub const fn satosin(sa: *mut Sockaddr) -> *mut SockaddrIn {
    sa.cast()
}

/// `satosin_const(sa)`: a generic `sockaddr` seen as a `sockaddr_in`, read-only.
pub const fn satosin_const(sa: *const Sockaddr) -> *const SockaddrIn {
    sa.cast()
}

/// `sintosa(sin)`: a `sockaddr_in` seen as a generic `sockaddr`.
pub const fn sintosa(sin: *mut SockaddrIn) -> *mut Sockaddr {
    sin.cast()
}

// LP64 sizes of the C structures.
const _: () = {
    assert!(size_of::<InAddr>() == 4);
    assert!(size_of::<SockaddrIn>() == 16);
    assert!(size_of::<SockaddrIn>() == size_of::<Sockaddr>());
    assert!(size_of::<IpOpts>() == 44);
    assert!(size_of::<IpMreq>() == 8);
    assert!(size_of::<IpMreqn>() == 12);
};

#[cfg(test)]
mod tests;
