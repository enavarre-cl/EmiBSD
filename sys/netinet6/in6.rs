/*	$OpenBSD: in6.h,v 1.125 2025/09/16 09:19:16 florian Exp $	*/
/*	$KAME: in6.h,v 1.83 2001/03/29 02:55:07 jinmei Exp $	*/
/*	$OpenBSD: in6.c,v 1.279 2026/03/22 23:14:00 bluhm Exp $	*/
/*	$KAME: in6.c,v 1.372 2004/06/14 08:14:21 itojun Exp $	*/
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
 *
 *	@(#)in.h	8.3 (Berkeley) 1/3/94
 */

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
 * Copyright (c) 1982, 1986, 1991, 1993
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
 *	@(#)in.c	8.2 (Berkeley) 11/15/93
 */
/* </LICENSES> */

//! IPv6 addresses and socket addresses, the `IPV6_*` socket options and the `net.inet6`
//! sysctl numbers: `<netinet6/in6.h>`; and `netinet6/in6.c`, the IPv6 addresses of the
//! interfaces (the address `ioctl`s, prefix routes, multicast records).
//!
//! Upstream: sys/netinet6/in6.h @ 3ce1f3f79392
//! Upstream: sys/netinet6/in6.c @ 3ce1f3f79392
//!
//! `in6.c` shares the module with the header, as `in.c` shares `netinet/in_.rs` with
//! `in.h`. The C header is included by `<netinet/in.h>` unconditionally, so this module is
//! compiled whether or not the `inet6` feature (OpenBSD's `option INET6`) is on.
//!
//! Byte order: as in the C kernel, an [`In6Addr`] holds the address bytes as they are on the
//! wire, and the 16- and 32-bit views (`s6_addr16`, `s6_addr32`) are the native-endian
//! readings of those bytes, so `__IPV6_ADDR_INT32_ONE` is `htonl(1)` and a comparison of
//! words is a comparison of the network-order words, as in C.
//!
//! ## Deviations
//! - `struct in6_addr`'s `__u6_addr` union is the byte array `s6_addr`; `s6_addr8`,
//!   `s6_addr16` and `s6_addr32` are `const fn` accessors (and setters) reading the bytes in
//!   native order. The structure has alignment 1 (4 in C) so that it can sit in the
//!   `__packed` protocol headers; every structure holding one keeps the C size and offsets
//!   (asserted below).
//! - The address macros (`IN6_IS_ADDR_*`, `IN6_ARE_ADDR_EQUAL`, `IN6_IS_SCOPE_*`,
//!   `__IPV6_ADDR_MC_SCOPE`, `IFA6_IS_DEPRECATED`, `IFA6_IS_INVALID`) are functions of the
//!   same name in lower case, `const fn` where they read only the address.
//! - The `*_INIT` initializers and the `in6addr_*`/`in6mask*` objects (defined in `in6.c`)
//!   are `pub const` values: `IN6ADDR_ANY_INIT` and `IN6ADDR_ANY` are the same address. The
//!   kernel masks `IN6MASK0`..`IN6MASK128` stand for both the C macros and the `in6mask*`
//!   objects, and `sa6_any` is [`SA6_ANY`].
//! - `satosin6`, `satosin6_const`, `sin6tosa` and `sin6tosa_const` are pointer casts
//!   (`docs/C_TO_RUST.md`); `ifatoia6` is checked, as `ifatoia` is: it takes the address
//!   family as the type tag and panics on another.
//! - `CTL_IPV6PROTO_NAMES` and `IPV6CTL_NAMES` are `Ctlname` tables.
//! - The userland part (`SIN6_LEN`, `socklen_t`, the `inet6_opt_*` and `inet6_rth_*`
//!   prototypes of libc) is not kernel material. `__KAME__` is not mirrored.
//! - The kernel prototypes are defined by the `.c` files: `ipv6_input`, `ipv6_check`,
//!   `inet6ctlerrmap` (`netinet6/ip6_input.rs`), `in6_cksum` (`netinet6/in6_cksum.rs`),
//!   `in6_proto_cksum_out` (`netinet6/ip6_output.rs`), `in6_embedscope`,
//!   `in6_recoverscope`, `in6_clearscope` (`netinet6/in6_src.rs`), `zeroin6_addr`
//!   (`netinet6/in6_pcb.rs`); `in6_addrscope`, `in6_ifawithscope`, `in6_mask2len`,
//!   `in6_nam2sin6` and `in6_sa2sin6` are this module's (`in6.c`).

use crate::net::if_var::Ifnet;
use crate::net::route::Rtentry;
use crate::netinet6::in6_var::{In6Aliasreq, In6Multi, In6MultiMship};
use crate::sys::errno::Errno;
use crate::sys::mbuf::Mbuf;
use crate::sys::socketvar::Socket;
use core::mem::size_of;

use crate::kern::kern_tc::getuptime;
use crate::kern::subr_prf::panic;
use crate::net::if_var::Ifaddr;
use crate::netinet::in_::IPPROTO_DIVERT;
use crate::netinet6::in6_var::In6Ifaddr;
use crate::netinet6::nd6::ND6_INFINITE_LIFETIME;
use crate::sys::endian::{htonl, htons};
use crate::sys::socket::{AF_INET6, Sockaddr};
use crate::sys::sysctl::{CTLTYPE_INT, CTLTYPE_NODE, CTLTYPE_STRUCT, Ctlname};
use crate::sys::types::{InPort, SaFamily};

/// Buffer length for strings containing printable IPv6 addresses.
pub const INET6_ADDRSTRLEN: usize = 46;

// Macros started with IPV6_ADDR is KAME local: network-order words and half-words.

/// `__IPV6_ADDR_INT32_ONE`: 1 as a network-order word.
pub const __IPV6_ADDR_INT32_ONE: u32 = htonl(1);
/// `__IPV6_ADDR_INT32_TWO`: 2 as a network-order word.
pub const __IPV6_ADDR_INT32_TWO: u32 = htonl(2);
/// `__IPV6_ADDR_INT32_MNL`: ff01:: (node-local multicast), first word.
pub const __IPV6_ADDR_INT32_MNL: u32 = htonl(0xff01_0000);
/// `__IPV6_ADDR_INT32_MLL`: ff02:: (link-local multicast), first word.
pub const __IPV6_ADDR_INT32_MLL: u32 = htonl(0xff02_0000);
/// `__IPV6_ADDR_INT32_SMP`: the third word of a v4-mapped address.
pub const __IPV6_ADDR_INT32_SMP: u32 = htonl(0x0000_ffff);
/// `__IPV6_ADDR_INT16_ULL`: fe80 (unicast link-local).
pub const __IPV6_ADDR_INT16_ULL: u16 = htons(0xfe80);
/// `__IPV6_ADDR_INT16_USL`: fec0 (unicast site-local).
pub const __IPV6_ADDR_INT16_USL: u16 = htons(0xfec0);
/// `__IPV6_ADDR_INT16_MLL`: ff02 (multicast link-local).
pub const __IPV6_ADDR_INT16_MLL: u16 = htons(0xff02);

/// `__IPV6_ADDR_SCOPE_NODELOCAL`.
pub const __IPV6_ADDR_SCOPE_NODELOCAL: u8 = 0x01;
/// `__IPV6_ADDR_SCOPE_INTFACELOCAL`.
pub const __IPV6_ADDR_SCOPE_INTFACELOCAL: u8 = 0x01;
/// `__IPV6_ADDR_SCOPE_LINKLOCAL`.
pub const __IPV6_ADDR_SCOPE_LINKLOCAL: u8 = 0x02;
/// `__IPV6_ADDR_SCOPE_SITELOCAL`.
pub const __IPV6_ADDR_SCOPE_SITELOCAL: u8 = 0x05;
/// `__IPV6_ADDR_SCOPE_ORGLOCAL`: just used in this file.
pub const __IPV6_ADDR_SCOPE_ORGLOCAL: u8 = 0x08;
/// `__IPV6_ADDR_SCOPE_GLOBAL`.
pub const __IPV6_ADDR_SCOPE_GLOBAL: u8 = 0x0e;

// Options for use with [gs]etsockopt at the IPV6 level. First word of comment is data type;
// bool is stored in int.

/// int; IP6 hops.
pub const IPV6_UNICAST_HOPS: i32 = 4;
/// u_int; set/get IP6 multicast i/f.
pub const IPV6_MULTICAST_IF: i32 = 9;
/// u_int; set/get IP6 multicast hops.
pub const IPV6_MULTICAST_HOPS: i32 = 10;
/// u_int; set/get IP6 multicast loopback.
pub const IPV6_MULTICAST_LOOP: i32 = 11;
/// ip6_mreq; join a group membership.
pub const IPV6_JOIN_GROUP: i32 = 12;
/// ip6_mreq; leave a group membership.
pub const IPV6_LEAVE_GROUP: i32 = 13;
/// int; range to choose for unspec port.
pub const IPV6_PORTRANGE: i32 = 14;
/// icmp6_filter; icmp6 filter.
pub const ICMP6_FILTER: i32 = 18;

/// int; checksum offset for raw socket.
pub const IPV6_CHECKSUM: i32 = 26;
/// bool; make `AF_INET6` sockets v6 only.
pub const IPV6_V6ONLY: i32 = 27;

// new socket options introduced in RFC3542

/// ip6_dest; send dst option before rthdr.
pub const IPV6_RTHDRDSTOPTS: i32 = 35;

/// bool; recv if, dst addr.
pub const IPV6_RECVPKTINFO: i32 = 36;
/// bool; recv hop limit.
pub const IPV6_RECVHOPLIMIT: i32 = 37;
/// bool; recv routing header.
pub const IPV6_RECVRTHDR: i32 = 38;
/// bool; recv hop-by-hop option.
pub const IPV6_RECVHOPOPTS: i32 = 39;
/// bool; recv dst option after rthdr.
pub const IPV6_RECVDSTOPTS: i32 = 40;

/// bool; send packets at the minimum MTU.
pub const IPV6_USE_MIN_MTU: i32 = 42;
/// bool; notify an according MTU.
pub const IPV6_RECVPATHMTU: i32 = 43;

/// mtuinfo; get the current path MTU (sopt), 4 bytes int; MTU notification (cmsg).
pub const IPV6_PATHMTU: i32 = 44;

// More new socket options introduced in RFC3542

/// in6_pktinfo; send if, src addr.
pub const IPV6_PKTINFO: i32 = 46;
/// int; send hop limit.
pub const IPV6_HOPLIMIT: i32 = 47;
/// sockaddr; next hop addr.
pub const IPV6_NEXTHOP: i32 = 48;
/// ip6_hbh; send hop-by-hop option.
pub const IPV6_HOPOPTS: i32 = 49;
/// ip6_dest; send dst option before rthdr.
pub const IPV6_DSTOPTS: i32 = 50;
/// ip6_rthdr; send routing header.
pub const IPV6_RTHDR: i32 = 51;

/// int; authentication used.
pub const IPV6_AUTH_LEVEL: i32 = 53;
/// int; transport encryption.
pub const IPV6_ESP_TRANS_LEVEL: i32 = 54;
/// int; full-packet encryption.
pub const IPV6_ESP_NETWORK_LEVEL: i32 = 55;
/// Set the outbound SA for a socket.
pub const IPSEC6_OUTSA: i32 = 56;
/// bool; recv traffic class values.
pub const IPV6_RECVTCLASS: i32 = 57;

/// bool; attach flowlabel automagically.
pub const IPV6_AUTOFLOWLABEL: i32 = 59;
/// int; compression.
pub const IPV6_IPCOMP_LEVEL: i32 = 60;

/// int; send traffic class value.
pub const IPV6_TCLASS: i32 = 61;
/// bool; disable IPv6 fragmentation.
pub const IPV6_DONTFRAG: i32 = 62;
/// bool; using PIPEX.
pub const IPV6_PIPEX: i32 = 63;

/// bool; receive IP dst port w/dgram.
pub const IPV6_RECVDSTPORT: i32 = 64;
/// int; minimum recv hop limit.
pub const IPV6_MINHOPCOUNT: i32 = 65;

/// int; routing table, see `SO_RTABLE`.
pub const IPV6_RTABLE: i32 = 0x1021;

/// This hop need not be a neighbor.
pub const IPV6_RTHDR_LOOSE: i32 = 0;
/// IPv6 routing header type 0.
pub const IPV6_RTHDR_TYPE_0: i32 = 0;

// Defaults and limits for options

/// Normally limit m'casts to 1 hop.
pub const IPV6_DEFAULT_MULTICAST_HOPS: i32 = 1;
/// Normally hear sends if a member.
pub const IPV6_DEFAULT_MULTICAST_LOOP: i32 = 1;

// Argument for IPV6_PORTRANGE: which range to search when port is unspecified at bind() or
// connect().

/// Default range.
pub const IPV6_PORTRANGE_DEFAULT: i32 = 0;
/// "high" - request firewall bypass.
pub const IPV6_PORTRANGE_HIGH: i32 = 1;
/// "low" - vouchsafe security.
pub const IPV6_PORTRANGE_LOW: i32 = 2;

// Definitions for inet6 sysctl operations. Third level is protocol number; fourth level is
// desired variable within that protocol.

/// Don't list to `IPV6PROTO_MAX`.
pub const IPV6PROTO_MAXID: i32 = IPPROTO_DIVERT + 1;

/// `CTL_IPV6PROTO_NAMES`: the protocols under `net.inet6`.
pub const CTL_IPV6PROTO_NAMES: [Ctlname; IPV6PROTO_MAXID as usize] = {
    let mut n = [Ctlname::NONE; IPV6PROTO_MAXID as usize];
    n[6] = Ctlname::new(b"tcp6", CTLTYPE_NODE);
    n[17] = Ctlname::new(b"udp6", CTLTYPE_NODE);
    n[41] = Ctlname::new(b"ip6", CTLTYPE_NODE);
    n[51] = Ctlname::new(b"ipsec6", CTLTYPE_NODE);
    n[58] = Ctlname::new(b"icmp6", CTLTYPE_NODE);
    n[258] = Ctlname::new(b"divert", CTLTYPE_NODE);
    n
};

// Names for IP sysctl objects

/// Act as router.
pub const IPV6CTL_FORWARDING: i32 = 1;
/// May send redirects when forwarding.
pub const IPV6CTL_SENDREDIRECTS: i32 = 2;
/// Default Hop-Limit.
pub const IPV6CTL_DEFHLIM: i32 = 3;
/// Forward source-routed dgrams.
pub const IPV6CTL_FORWSRCRT: i32 = 5;
/// Stats.
pub const IPV6CTL_STATS: i32 = 6;
/// Multicast forwarding stats.
pub const IPV6CTL_MRTSTATS: i32 = 7;
/// Multicast routing protocol.
pub const IPV6CTL_MRTPROTO: i32 = 8;
/// Max packets reassembly queue.
pub const IPV6CTL_MAXFRAGPACKETS: i32 = 9;
/// Verify source route and intf.
pub const IPV6CTL_SOURCECHECK: i32 = 10;
/// Minimum logging interval.
pub const IPV6CTL_SOURCECHECK_LOGINT: i32 = 11;
/// `IPV6CTL_ACCEPT_RTADV`.
pub const IPV6CTL_ACCEPT_RTADV: i32 = 12;
/// `IPV6CTL_LOG_INTERVAL`.
pub const IPV6CTL_LOG_INTERVAL: i32 = 14;
/// `IPV6CTL_HDRNESTLIMIT`.
pub const IPV6CTL_HDRNESTLIMIT: i32 = 15;
/// `IPV6CTL_DAD_COUNT`.
pub const IPV6CTL_DAD_COUNT: i32 = 16;
/// `IPV6CTL_AUTO_FLOWLABEL`.
pub const IPV6CTL_AUTO_FLOWLABEL: i32 = 17;
/// `IPV6CTL_DEFMCASTHLIM`.
pub const IPV6CTL_DEFMCASTHLIM: i32 = 18;
// 24 to 40: reserved
/// Max fragments.
pub const IPV6CTL_MAXFRAGS: i32 = 41;
/// `IPV6CTL_MFORWARDING`.
pub const IPV6CTL_MFORWARDING: i32 = 42;
/// `IPV6CTL_MULTIPATH`.
pub const IPV6CTL_MULTIPATH: i32 = 43;
/// Path MTU discovery for multicast.
pub const IPV6CTL_MCAST_PMTU: i32 = 44;
/// `IPV6CTL_NEIGHBORGCTHRESH`.
pub const IPV6CTL_NEIGHBORGCTHRESH: i32 = 45;
/// `IPV6CTL_MAXDYNROUTES`.
pub const IPV6CTL_MAXDYNROUTES: i32 = 48;
/// `IPV6CTL_DAD_PENDING`.
pub const IPV6CTL_DAD_PENDING: i32 = 49;
/// `IPV6CTL_MTUDISCTIMEOUT`.
pub const IPV6CTL_MTUDISCTIMEOUT: i32 = 50;
/// `IPV6CTL_IFQUEUE`.
pub const IPV6CTL_IFQUEUE: i32 = 51;
/// `IPV6CTL_MRTMIF`.
pub const IPV6CTL_MRTMIF: i32 = 52;
/// `IPV6CTL_MRTMFC`.
pub const IPV6CTL_MRTMFC: i32 = 53;
/// `IPV6CTL_MAXID`.
pub const IPV6CTL_MAXID: i32 = 54;

/// `IPV6CTL_NAMES`: the names under `net.inet6.ip6`.
pub const IPV6CTL_NAMES: [Ctlname; IPV6CTL_MAXID as usize] = {
    let mut n = [Ctlname::NONE; IPV6CTL_MAXID as usize];
    n[1] = Ctlname::new(b"forwarding", CTLTYPE_INT);
    n[2] = Ctlname::new(b"redirect", CTLTYPE_INT);
    n[3] = Ctlname::new(b"hlim", CTLTYPE_INT);
    n[5] = Ctlname::new(b"forwsrcrt", CTLTYPE_INT);
    n[8] = Ctlname::new(b"mrtproto", CTLTYPE_INT);
    n[9] = Ctlname::new(b"maxfragpackets", CTLTYPE_INT);
    n[10] = Ctlname::new(b"sourcecheck", CTLTYPE_INT);
    n[11] = Ctlname::new(b"sourcecheck_logint", CTLTYPE_INT);
    n[15] = Ctlname::new(b"hdrnestlimit", CTLTYPE_INT);
    n[16] = Ctlname::new(b"dad_count", CTLTYPE_INT);
    n[18] = Ctlname::new(b"defmcasthlim", CTLTYPE_INT);
    n[41] = Ctlname::new(b"maxfrags", CTLTYPE_INT);
    n[42] = Ctlname::new(b"mforwarding", CTLTYPE_INT);
    n[43] = Ctlname::new(b"multipath", CTLTYPE_INT);
    n[44] = Ctlname::new(b"multicast_mtudisc", CTLTYPE_INT);
    n[45] = Ctlname::new(b"neighborgcthresh", CTLTYPE_INT);
    n[48] = Ctlname::new(b"maxdynroutes", CTLTYPE_INT);
    n[49] = Ctlname::new(b"dad_pending", CTLTYPE_INT);
    n[50] = Ctlname::new(b"mtudisctimeout", CTLTYPE_INT);
    n[51] = Ctlname::new(b"ifq", CTLTYPE_NODE);
    n[52] = Ctlname::new(b"mrtmif", CTLTYPE_STRUCT);
    n[53] = Ctlname::new(b"mrtmfc", CTLTYPE_STRUCT);
    n
};

/// `struct in6_addr`: an IPv6 address, its 16 bytes in network order (see the module's
/// deviations for the views).
#[repr(C)]
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct In6Addr {
    /// `s6_addr`: the 128-bit address.
    pub s6_addr: [u8; 16],
}

impl In6Addr {
    /// The address of the 16 bytes `b`.
    pub const fn new(b: [u8; 16]) -> Self {
        Self { s6_addr: b }
    }

    /// The address whose `s6_addr32` words are `w` (network-order words).
    pub const fn from_s6_addr32(w: [u32; 4]) -> Self {
        let mut a = Self { s6_addr: [0; 16] };
        let mut i = 0;
        while i < 4 {
            a.set_s6_addr32(i, w[i]);
            i += 1;
        }
        a
    }

    /// `s6_addr8[i]`.
    pub const fn s6_addr8(&self, i: usize) -> u8 {
        self.s6_addr[i]
    }

    /// `s6_addr16[i]`: bytes `2i`, `2i + 1` read in native order.
    pub const fn s6_addr16(&self, i: usize) -> u16 {
        u16::from_ne_bytes([self.s6_addr[2 * i], self.s6_addr[2 * i + 1]])
    }

    /// `s6_addr16[i] = v`.
    pub const fn set_s6_addr16(&mut self, i: usize, v: u16) {
        let b = v.to_ne_bytes();
        self.s6_addr[2 * i] = b[0];
        self.s6_addr[2 * i + 1] = b[1];
    }

    /// `s6_addr32[i]`: bytes `4i` to `4i + 3` read in native order.
    pub const fn s6_addr32(&self, i: usize) -> u32 {
        let a = &self.s6_addr;
        u32::from_ne_bytes([a[4 * i], a[4 * i + 1], a[4 * i + 2], a[4 * i + 3]])
    }

    /// `s6_addr32[i] = v`.
    pub const fn set_s6_addr32(&mut self, i: usize, v: u32) {
        let b = v.to_ne_bytes();
        self.s6_addr[4 * i] = b[0];
        self.s6_addr[4 * i + 1] = b[1];
        self.s6_addr[4 * i + 2] = b[2];
        self.s6_addr[4 * i + 3] = b[3];
    }
}

/// `struct sockaddr_in6`: socket address for IPv6.
#[repr(C)]
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct SockaddrIn6 {
    /// Length of this struct (`sa_family_t`).
    pub sin6_len: u8,
    /// `AF_INET6`.
    pub sin6_family: SaFamily,
    /// Transport layer port #, network order.
    pub sin6_port: InPort,
    /// IP6 flow information.
    pub sin6_flowinfo: u32,
    /// IP6 address.
    pub sin6_addr: In6Addr,
    /// Interface scope id.
    pub sin6_scope_id: u32,
}

impl SockaddrIn6 {
    /// An all-zero socket address, usable in `const` contexts.
    pub const fn zeroed() -> Self {
        Self {
            sin6_len: 0,
            sin6_family: 0,
            sin6_port: 0,
            sin6_flowinfo: 0,
            sin6_addr: In6Addr { s6_addr: [0; 16] },
            sin6_scope_id: 0,
        }
    }

    /// A socket address of `addr` with its length and family set, as the C code fills one
    /// (`sin6_len = sizeof(sin6); sin6_family = AF_INET6; sin6_addr = addr`).
    pub const fn with_addr(addr: In6Addr) -> Self {
        Self {
            sin6_len: size_of::<SockaddrIn6>() as u8,
            sin6_family: AF_INET6,
            sin6_addr: addr,
            ..Self::zeroed()
        }
    }
}

/// `struct ipv6_mreq`: argument structure for `IPV6_JOIN_GROUP` and `IPV6_LEAVE_GROUP`.
#[repr(C)]
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Ipv6Mreq {
    /// `ipv6mr_multiaddr`.
    pub ipv6mr_multiaddr: In6Addr,
    /// `ipv6mr_interface`.
    pub ipv6mr_interface: u32,
}

/// `struct in6_pktinfo`: `IPV6_PKTINFO`, packet information (RFC3542 sec 6).
#[repr(C)]
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct In6Pktinfo {
    /// src/dst IPv6 address.
    pub ipi6_addr: In6Addr,
    /// send/recv interface index.
    pub ipi6_ifindex: u32,
}

/// `struct ip6_mtuinfo`: control structure for the `IPV6_RECVPATHMTU` socket option.
#[repr(C)]
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Ip6Mtuinfo {
    /// Or sockaddr_storage?
    pub ip6m_addr: SockaddrIn6,
    /// The path MTU.
    pub ip6m_mtu: u32,
}

/// `IN6ADDR_ANY_INIT`: `::`.
pub const IN6ADDR_ANY_INIT: In6Addr = In6Addr::new([0; 16]);
/// `IN6ADDR_LOOPBACK_INIT`: `::1`.
pub const IN6ADDR_LOOPBACK_INIT: In6Addr =
    In6Addr::new([0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 1]);
/// `IN6ADDR_NODELOCAL_ALLNODES_INIT`: `ff01::1`.
pub const IN6ADDR_NODELOCAL_ALLNODES_INIT: In6Addr =
    In6Addr::new([0xff, 0x01, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 1]);
/// `IN6ADDR_INTFACELOCAL_ALLNODES_INIT`: `ff01::1`.
pub const IN6ADDR_INTFACELOCAL_ALLNODES_INIT: In6Addr =
    In6Addr::new([0xff, 0x01, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 1]);
/// `IN6ADDR_LINKLOCAL_ALLNODES_INIT`: `ff02::1`.
pub const IN6ADDR_LINKLOCAL_ALLNODES_INIT: In6Addr =
    In6Addr::new([0xff, 0x02, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 1]);
/// `IN6ADDR_LINKLOCAL_ALLROUTERS_INIT`: `ff02::2`.
pub const IN6ADDR_LINKLOCAL_ALLROUTERS_INIT: In6Addr =
    In6Addr::new([0xff, 0x02, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 2]);

/// `in6addr_any` (`in6.c`).
pub const IN6ADDR_ANY: In6Addr = IN6ADDR_ANY_INIT;
/// `in6addr_loopback` (`in6.c`).
pub const IN6ADDR_LOOPBACK: In6Addr = IN6ADDR_LOOPBACK_INIT;
/// `in6addr_intfacelocal_allnodes` (`in6.c`).
pub const IN6ADDR_INTFACELOCAL_ALLNODES: In6Addr = IN6ADDR_INTFACELOCAL_ALLNODES_INIT;
/// `in6addr_linklocal_allnodes` (`in6.c`).
pub const IN6ADDR_LINKLOCAL_ALLNODES: In6Addr = IN6ADDR_LINKLOCAL_ALLNODES_INIT;
/// `in6addr_linklocal_allrouters` (`in6.c`).
pub const IN6ADDR_LINKLOCAL_ALLROUTERS: In6Addr = IN6ADDR_LINKLOCAL_ALLROUTERS_INIT;

/// `IN6MASK0` / `in6mask0`: the /0 mask.
pub const IN6MASK0: In6Addr = In6Addr::new([0; 16]);
/// `IN6MASK32` / `in6mask32`: the /32 mask.
pub const IN6MASK32: In6Addr =
    In6Addr::new([0xff, 0xff, 0xff, 0xff, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0]);
/// `IN6MASK64` / `in6mask64`: the /64 mask.
pub const IN6MASK64: In6Addr = In6Addr::new([
    0xff, 0xff, 0xff, 0xff, 0xff, 0xff, 0xff, 0xff, 0, 0, 0, 0, 0, 0, 0, 0,
]);
/// `IN6MASK96` / `in6mask96`: the /96 mask.
pub const IN6MASK96: In6Addr = In6Addr::new([
    0xff, 0xff, 0xff, 0xff, 0xff, 0xff, 0xff, 0xff, 0xff, 0xff, 0xff, 0xff, 0, 0, 0, 0,
]);
/// `IN6MASK128` / `in6mask128`: the /128 mask.
pub const IN6MASK128: In6Addr = In6Addr::new([0xff; 16]);

/// `sa6_any` (`in6.c`): the unspecified address as a socket address.
pub const SA6_ANY: SockaddrIn6 = SockaddrIn6::with_addr(IN6ADDR_ANY_INIT);

/// `IN6_ARE_ADDR_EQUAL(a, b)`.
pub const fn in6_are_addr_equal(a: &In6Addr, b: &In6Addr) -> bool {
    let mut i = 0;
    while i < 16 {
        if a.s6_addr[i] != b.s6_addr[i] {
            return false;
        }
        i += 1;
    }
    true
}

/// `IN6_IS_ADDR_UNSPECIFIED(a)`: `::`.
pub const fn in6_is_addr_unspecified(a: &In6Addr) -> bool {
    a.s6_addr32(0) == 0 && a.s6_addr32(1) == 0 && a.s6_addr32(2) == 0 && a.s6_addr32(3) == 0
}

/// `IN6_IS_ADDR_LOOPBACK(a)`: `::1`.
pub const fn in6_is_addr_loopback(a: &In6Addr) -> bool {
    a.s6_addr32(0) == 0
        && a.s6_addr32(1) == 0
        && a.s6_addr32(2) == 0
        && a.s6_addr32(3) == __IPV6_ADDR_INT32_ONE
}

/// `IN6_IS_ADDR_V4COMPAT(a)`: IPv4 compatible (`::a.b.c.d`, not `::` or `::1`).
pub const fn in6_is_addr_v4compat(a: &In6Addr) -> bool {
    a.s6_addr32(0) == 0
        && a.s6_addr32(1) == 0
        && a.s6_addr32(2) == 0
        && a.s6_addr32(3) != 0
        && a.s6_addr32(3) != __IPV6_ADDR_INT32_ONE
}

/// `IN6_IS_ADDR_V4MAPPED(a)`: `::ffff:a.b.c.d`.
pub const fn in6_is_addr_v4mapped(a: &In6Addr) -> bool {
    a.s6_addr32(0) == 0 && a.s6_addr32(1) == 0 && a.s6_addr32(2) == __IPV6_ADDR_INT32_SMP
}

/// `IN6_IS_ADDR_LINKLOCAL(a)`: unicast link-local, `fe80::/10` (the topmost 10 bits only,
/// RFC2373).
pub const fn in6_is_addr_linklocal(a: &In6Addr) -> bool {
    a.s6_addr[0] == 0xfe && (a.s6_addr[1] & 0xc0) == 0x80
}

/// `IN6_IS_ADDR_SITELOCAL(a)`: unicast site-local, `fec0::/10`.
pub const fn in6_is_addr_sitelocal(a: &In6Addr) -> bool {
    a.s6_addr[0] == 0xfe && (a.s6_addr[1] & 0xc0) == 0xc0
}

/// `__IPV6_ADDR_MC_SCOPE(a)`: the scope of a multicast address.
pub const fn __ipv6_addr_mc_scope(a: &In6Addr) -> u8 {
    a.s6_addr[1] & 0x0f
}

/// `IN6_IS_ADDR_MULTICAST(a)`: `ff00::/8`.
pub const fn in6_is_addr_multicast(a: &In6Addr) -> bool {
    a.s6_addr[0] == 0xff
}

/// `IN6_IS_ADDR_MC_NODELOCAL(a)`.
pub const fn in6_is_addr_mc_nodelocal(a: &In6Addr) -> bool {
    in6_is_addr_multicast(a) && __ipv6_addr_mc_scope(a) == __IPV6_ADDR_SCOPE_NODELOCAL
}

/// `IN6_IS_ADDR_MC_INTFACELOCAL(a)`.
pub const fn in6_is_addr_mc_intfacelocal(a: &In6Addr) -> bool {
    in6_is_addr_multicast(a) && __ipv6_addr_mc_scope(a) == __IPV6_ADDR_SCOPE_INTFACELOCAL
}

/// `IN6_IS_ADDR_MC_LINKLOCAL(a)`.
pub const fn in6_is_addr_mc_linklocal(a: &In6Addr) -> bool {
    in6_is_addr_multicast(a) && __ipv6_addr_mc_scope(a) == __IPV6_ADDR_SCOPE_LINKLOCAL
}

/// `IN6_IS_ADDR_MC_SITELOCAL(a)`.
pub const fn in6_is_addr_mc_sitelocal(a: &In6Addr) -> bool {
    in6_is_addr_multicast(a) && __ipv6_addr_mc_scope(a) == __IPV6_ADDR_SCOPE_SITELOCAL
}

/// `IN6_IS_ADDR_MC_ORGLOCAL(a)`.
pub const fn in6_is_addr_mc_orglocal(a: &In6Addr) -> bool {
    in6_is_addr_multicast(a) && __ipv6_addr_mc_scope(a) == __IPV6_ADDR_SCOPE_ORGLOCAL
}

/// `IN6_IS_ADDR_MC_GLOBAL(a)`.
pub const fn in6_is_addr_mc_global(a: &In6Addr) -> bool {
    in6_is_addr_multicast(a) && __ipv6_addr_mc_scope(a) == __IPV6_ADDR_SCOPE_GLOBAL
}

/// `IN6_IS_SCOPE_LINKLOCAL(a)`: a unicast or multicast link-local address.
pub const fn in6_is_scope_linklocal(a: &In6Addr) -> bool {
    in6_is_addr_linklocal(a) || in6_is_addr_mc_linklocal(a)
}

/// `IN6_IS_SCOPE_EMBED(a)`: an address whose scope the kernel embeds in it (link-local
/// unicast or multicast, interface-local multicast).
pub const fn in6_is_scope_embed(a: &In6Addr) -> bool {
    in6_is_addr_linklocal(a) || in6_is_addr_mc_linklocal(a) || in6_is_addr_mc_intfacelocal(a)
}

/// `IFA6_IS_DEPRECATED(a)`: the preferred lifetime of the address has run out.
pub fn ifa6_is_deprecated(a: &In6Ifaddr) -> bool {
    let lt = a.ia6_lifetime.get();
    lt.ia6t_pltime != ND6_INFINITE_LIFETIME
        && (getuptime() - a.ia6_updatetime.get()) as u32 > lt.ia6t_pltime
}

/// `IFA6_IS_INVALID(a)`: the valid lifetime of the address has run out.
pub fn ifa6_is_invalid(a: &In6Ifaddr) -> bool {
    let lt = a.ia6_lifetime.get();
    lt.ia6t_vltime != ND6_INFINITE_LIFETIME
        && (getuptime() - a.ia6_updatetime.get()) as u32 > lt.ia6t_vltime
}

/// `satosin6(sa)`: a generic `sockaddr` seen as a `sockaddr_in6`.
pub const fn satosin6(sa: *mut Sockaddr) -> *mut SockaddrIn6 {
    sa.cast()
}

/// `satosin6_const(sa)`: a generic `sockaddr` seen as a `sockaddr_in6`, read-only.
pub const fn satosin6_const(sa: *const Sockaddr) -> *const SockaddrIn6 {
    sa.cast()
}

/// `sin6tosa(sin6)`: a `sockaddr_in6` seen as a generic `sockaddr`.
pub const fn sin6tosa(sin6: *mut SockaddrIn6) -> *mut Sockaddr {
    sin6.cast()
}

/// `sin6tosa_const(sin6)`: a `sockaddr_in6` seen as a generic `sockaddr`, read-only.
pub const fn sin6tosa_const(sin6: *const SockaddrIn6) -> *const Sockaddr {
    sin6.cast()
}

/// `ifatoia6(ifa)`: the IPv6 address an `AF_INET6` interface address is the first member
/// of.
pub fn ifatoia6(ifa: &Ifaddr) -> &In6Ifaddr {
    let addr = ifa.ifa_addr.get();
    // SAFETY: an interface address's `ifa_addr` is a readable socket address (`ifa_add`'s
    // contract).
    if addr.is_null() || unsafe { (*addr).sa_family } != AF_INET6 {
        panic(format_args!("ifatoia6: not an inet6 address"));
    }
    // SAFETY: every `AF_INET6` interface address is the `ia_ifa` member, at offset 0 of the
    // `#[repr(C)]` `In6Ifaddr` that `in6_update_ifa` allocated.
    unsafe { &*core::ptr::from_ref(ifa).cast::<In6Ifaddr>() }
}

/// `in6_mask2len`: the prefix length of mask `mask`, -1 if it is not contiguous. `lim` is
/// the number of valid bytes of the mask (the C's `lim0 - mask`); `None` (`NULL`) or more
/// than 16 means the whole address, and a given limit makes the check of the remaining
/// bits stricter.
pub fn in6_mask2len(mask: &In6Addr, lim: Option<usize>) -> i32 {
    let _ = (mask, lim);
    let _ = crate::unported!("in6_mask2len: placeholder");
    -1
}

/// `in6_nam2sin6`: the `sockaddr_in6` in mbuf `nam`, checked (family, length).
pub fn in6_nam2sin6(nam: &Mbuf) -> Result<*mut SockaddrIn6, Errno> {
    let _ = nam;
    Err(crate::unported!("in6_nam2sin6: placeholder"))
}

/// `in6_sa2sin6`: `sa` as a `sockaddr_in6`, checked.
///
/// # Safety
///
/// `sa` points at a readable socket address of its `sa_len` bytes.
pub unsafe fn in6_sa2sin6(sa: *mut Sockaddr) -> Result<*mut SockaddrIn6, Errno> {
    let _ = sa;
    Err(crate::unported!("in6_sa2sin6: placeholder"))
}

/// `in6_control`: the `pru_control` of the IPv6 protocols: the address `ioctl`s, handed
/// to `in6_ioctl` with the socket's `SS_PRIV` (`MROUTING`'s `mrt6_ioctl` is not
/// configured).
pub fn in6_control(
    so: &'static Socket,
    cmd: u64,
    data: &mut [u8],
    ifp: Option<&'static Ifnet>,
) -> Result<(), Errno> {
    let _ = (so, cmd, data, ifp);
    Err(crate::unported!("in6_control: placeholder"))
}

/// `in6_ioctl`: the IPv6 address `ioctl`s (`SIOCAIFADDR_IN6`, `SIOCDIFADDR_IN6`, the
/// `SIOCG*_IN6` requests, `SIOCGIFINFO_IN6`/`SIOCGNBRINFO_IN6` through `nd6_ioctl`).
///
/// # Safety
///
/// `data` points at the kernel copy of the request, readable and writable for the
/// size `cmd` encodes and aligned for the request's type.
pub unsafe fn in6_ioctl(
    cmd: u64,
    data: *mut u8,
    ifp: Option<&'static Ifnet>,
    privileged: bool,
) -> Result<(), Errno> {
    let _ = (cmd, data, ifp, privileged);
    Err(crate::unported!("in6_ioctl: placeholder"))
}

/// `in6_update_ifa`: adds or changes the address `ifra` describes on `ifp`; `ia6` is the
/// existing address or `None` to allocate one.
pub fn in6_update_ifa(
    ifp: &'static Ifnet,
    ifra: &In6Aliasreq,
    ia6: Option<&'static In6Ifaddr>,
) -> Result<(), Errno> {
    let _ = (ifp, ifra, ia6);
    Err(crate::unported!("in6_update_ifa: placeholder"))
}

/// `in6_purgeaddr`: removes an IPv6 address and its routes and memberships.
pub fn in6_purgeaddr(ifa: &'static Ifaddr) {
    let _ = ifa;
    let _ = crate::unported!("in6_purgeaddr: placeholder");
}

/// `in6_lookupmulti`: the multicast record of `addr` on `ifp`, if joined.
pub fn in6_lookupmulti(addr: &In6Addr, ifp: &Ifnet) -> Option<&'static In6Multi> {
    let _ = (addr, ifp);
    let _ = crate::unported!("in6_lookupmulti: placeholder");
    None
}

/// `in6_addmulti`: joins `addr` on `ifp` (a new record, or a reference to the existing
/// one); the C's `*errorp` is the `Err`.
pub fn in6_addmulti(addr: &In6Addr, ifp: &'static Ifnet) -> Result<&'static In6Multi, Errno> {
    let _ = (addr, ifp);
    Err(crate::unported!("in6_addmulti: placeholder"))
}

/// `in6_delmulti`: drops a reference to a multicast record, leaving the group with the
/// last one.
pub fn in6_delmulti(in6m: &'static In6Multi) {
    let _ = in6m;
    let _ = crate::unported!("in6_delmulti: placeholder");
}

/// `in6_hasmulti`: whether `ifp` has joined `addr`.
pub fn in6_hasmulti(addr: &In6Addr, ifp: &Ifnet) -> bool {
    let _ = (addr, ifp);
    let _ = crate::unported!("in6_hasmulti: placeholder");
    false
}

/// `in6_joingroup`: joins `addr` on `ifp` and returns the membership entry (`malloc`ed,
/// freed by `in6_leavegroup`); the C's `*errorp` is the `Err`.
pub fn in6_joingroup(ifp: &'static Ifnet, addr: &In6Addr) -> Result<&'static In6MultiMship, Errno> {
    let _ = (ifp, addr);
    Err(crate::unported!("in6_joingroup: placeholder"))
}

/// `in6_leavegroup`: leaves the group of membership `imm` and frees it.
pub fn in6_leavegroup(imm: &'static In6MultiMship) {
    let _ = imm;
    let _ = crate::unported!("in6_leavegroup: placeholder");
}

/// `in6ifa_ifpforlinklocal`: the link-local address of `ifp` without any of the
/// `IN6_IFF_*` flags `ignoreflags`.
pub fn in6ifa_ifpforlinklocal(ifp: &Ifnet, ignoreflags: i32) -> Option<&'static In6Ifaddr> {
    let _ = (ifp, ignoreflags);
    let _ = crate::unported!("in6ifa_ifpforlinklocal: placeholder");
    None
}

/// `in6ifa_ifpwithaddr`: the address `addr` of `ifp`.
pub fn in6ifa_ifpwithaddr(ifp: &Ifnet, addr: &In6Addr) -> Option<&'static In6Ifaddr> {
    let _ = (ifp, addr);
    let _ = crate::unported!("in6ifa_ifpwithaddr: placeholder");
    None
}

/// `in6_addrscope`: the scope of `addr` (`__IPV6_ADDR_SCOPE_*`).
pub fn in6_addrscope(addr: &In6Addr) -> i32 {
    let _ = addr;
    let _ = crate::unported!("in6_addrscope: placeholder");
    0
}

/// `in6_addr2scopeid`: the scope zone id of `addr` on interface `ifidx` (the interface
/// index for link- and interface-local scopes, 0 otherwise).
pub fn in6_addr2scopeid(ifidx: u32, addr: &In6Addr) -> i32 {
    let _ = (ifidx, addr);
    let _ = crate::unported!("in6_addr2scopeid: placeholder");
    0
}

/// `in6_matchlen`: the length of the common prefix of `src` and `dst`, in bits.
pub fn in6_matchlen(src: &In6Addr, dst: &In6Addr) -> i32 {
    let _ = (src, dst);
    let _ = crate::unported!("in6_matchlen: placeholder");
    0
}

/// `in6_prefixlen2mask`: the mask of prefix length `len`.
pub fn in6_prefixlen2mask(maskp: &mut In6Addr, len: i32) {
    let _ = (maskp, len);
    let _ = crate::unported!("in6_prefixlen2mask: placeholder");
}

/// `in6_ifawithscope`: the best source address for `dst` on `oifp` (RFC 6724 rules as
/// the C applies them), considering the route `rt` and routing domain `rdomain`.
pub fn in6_ifawithscope(
    oifp: &Ifnet,
    dst: &In6Addr,
    rdomain: u32,
    rt: Option<&Rtentry>,
) -> Option<&'static In6Ifaddr> {
    let _ = (oifp, dst, rdomain, rt);
    let _ = crate::unported!("in6_ifawithscope: placeholder");
    None
}

/// `in6if_do_dad`: whether Duplicate Address Detection runs on `ifp`.
pub fn in6if_do_dad(ifp: &Ifnet) -> bool {
    let _ = ifp;
    let _ = crate::unported!("in6if_do_dad: placeholder");
    false
}

// LP64 sizes of the C structures.
const _: () = {
    assert!(size_of::<In6Addr>() == 16);
    assert!(size_of::<SockaddrIn6>() == 28);
    assert!(size_of::<Ipv6Mreq>() == 20);
    assert!(size_of::<In6Pktinfo>() == 20);
    assert!(size_of::<Ip6Mtuinfo>() == 32);
};

#[cfg(test)]
mod tests;
