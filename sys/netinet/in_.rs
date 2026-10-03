/*	$OpenBSD: in.h,v 1.149 2025/03/02 21:28:32 bluhm Exp $	*/
/*	$NetBSD: in.h,v 1.20 1996/02/13 23:41:47 christos Exp $	*/
/*	$OpenBSD: in.c,v 1.196 2026/09/20 20:50:29 gnezdo Exp $	*/
/*	$NetBSD: in.c,v 1.26 1996/02/13 23:41:39 christos Exp $	*/
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

/*
 * Copyright (C) 2001 WIDE Project.  All rights reserved.
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

//! Constants and structures defined by the internet system, per RFC 790, September 1981, and
//! numerous additions: `<netinet/in.h>`; and `netinet/in.c`, the Internet addresses of the
//! interfaces (the address `ioctl`s, the interface and prefix routes, multicast records).
//!
//! Upstream: sys/netinet/in.h @ 3ce1f3f79392
//! Upstream: sys/netinet/in.c @ 3ce1f3f79392
//!
//! `in` is a Rust keyword, so the module is `in_` (`docs/C_TO_RUST.md`); `in.c` shares it, as
//! `if.c` shares `if_`.
//!
//! Byte order: as in the C kernel, addresses and ports stay in network order wherever they are
//! stored (`in_addr.s_addr`, `sin_port`). The kernel's `__IPADDR(x)` is `htonl(x)`: "by
//! byte-swapping the constants, we avoid ever having to byte-swap IP addresses inside the
//! kernel", so `INADDR_*`, `IN_CLASS*_NET`/`_HOST` and the `in_class*` tests are network-order
//! values and take network-order arguments. (User-level programs rely on these macros not
//! doing byte-swapping; this is the kernel's half.)
//!
//! Status: `in.h` `wip` (the sysctl name tables), `in.c` `ported` (M7b).
//!
//! ## Deviations
//! - `CTL_IPPROTO_NAMES` and `IPCTL_NAMES` (`struct ctlname` tables) come with
//!   `<sys/sysctl.h>`.
//! - `ifatoia` is `<netinet/in_var.h>`'s (`netinet/in_var.rs`); `inetctlerrmap` and
//!   `zeroin_addr` come with the `.c` files that define them, as do the prototypes
//!   (`in_cksum` in `netinet/in_cksum.rs`, `ipv4_input` in `netinet/ip_input.rs`, ...).
//! - The `#include <netinet6/in6.h>` at the end of the C header is not mirrored: `in6.h` is a
//!   module of its own when it is ported.
//! - `INADDR_NONE` (userland only), `htons` and friends outside the kernel, and the userland
//!   prototypes (`bindresvport`) are not kernel material; the kernel's byte-order functions
//!   are in `sys/endian.rs`.
//! - `struct in_addr`'s `s_addr` is a `u32`: `in_addr_t` is `crate::sys::types::InAddr`, a
//!   name this struct also needs.
//! - `satosin`, `satosin_const` and `sintosa` are pointer casts (`docs/C_TO_RUST.md`);
//!   `in_hosteq` and `in_nullhost` are `const fn`s.
//! - `in_control`'s `struct socket *` is an opaque pointer: sockets are not ported, so the
//!   `SS_PRIV` test cannot be made. A NULL socket is the kernel's own request and privileged
//!   (the boot self-test configures an interface that way); any other socket is reported and
//!   unprivileged. `MROUTING` (`mrt_ioctl`) is not configured. `ifioctl`'s `pru_control`
//!   (`net/if_.rs`) reaches `in_control` for `AF_INET`.
//! - The address `ioctl`s are `unsafe fn`s over `caddr_t data` (`docs/C_TO_RUST.md`, the
//!   `ioctl` row); the `struct sockaddr_in *` the C keeps into the request are copies or raw
//!   pointers into it. `in_ioctl_set_ifaddr` and `in_ioctl_change_ifaddr` share the
//!   allocation of a new address (`in_ifaddr_alloc`); `malloc(M_WAITOK)` that fails panics.
//! - `in_nam2sin` and `in_sa2sin` return the `sockaddr_in` pointer instead of storing it
//!   through an out parameter. Booleans are `bool` (`in_canforward`, `in_broadcast`,
//!   `in_hasmulti`, `in_ifinit`'s `newaddr`, `in_ioctl`'s `privileged`).
//! - IGMP (`netinet/igmp.c`) is not ported: `in_addmulti` and `in_delmulti` report
//!   `igmp_joingroup` and `igmp_leavegroup`, so no membership report is sent (`struct
//!   igmp_pktinfo`'s `ipi_ifidx` stays 0, as the C's does when IGMP has nothing to send).
//! - `KERNEL_LOCK()`/`KERNEL_UNLOCK()` are nothing without `MULTIPROCESSOR`.

use core::ffi::c_void;
use core::mem::{offset_of, size_of};
use core::ptr::{self, NonNull};

use crate::kern::kern_malloc::{free, malloc};
use crate::kern::kern_rwlock::{
    rw_assert_anylock, rw_enter_read, rw_enter_write, rw_exit_read, rw_exit_write,
};
use crate::kern::kern_synch::{refcnt_init_trace, refcnt_rele, refcnt_take};
use crate::kern::subr_prf::panic;
use crate::net::if_::{
    IFF_BROADCAST, IFF_LOOPBACK, IFF_MULTICAST, IFF_POINTOPOINT, IFNETLIST, IFXF_AUTOCONF4, Ifreq,
    if_addrhooks_run, if_get, if_put, ifa_add, ifa_del, ifa_update_broadaddr, ifp_ioctl,
};
use crate::net::if_var::{Ifaddr, Ifnet};
use crate::net::route::{
    RTF_BROADCAST, RTF_CLONING, RTF_CONNECTED, RTF_HOST, RTF_MPATH, ifafree, rt_ifa_add,
    rt_ifa_addlocal, rt_ifa_del, rt_ifa_dellocal, rt_ifa_purge,
};
use crate::net::rtable::rtable_l2;
use crate::netinet::in_var::{InAliasreq, InIfaddr, InMulti, ifatoia, ifmatoinm};
use crate::sys::endian::{htonl, ntohl};
use crate::sys::errno::Errno;
use crate::sys::malloc::{M_IFADDR, M_IPMADDR, M_WAITOK, M_ZERO};
use crate::sys::mbuf::{Mbuf, mtod};
use crate::sys::refcnt::{DT_REFCNT_IDX_IFADDR, DT_REFCNT_IDX_IFMADDR};
use crate::sys::socket::{AF_INET, Sockaddr};
use crate::sys::sockio::{
    SIOCADDMULTI, SIOCAIFADDR, SIOCDELMULTI, SIOCDIFADDR, SIOCGIFADDR, SIOCGIFBRDADDR,
    SIOCGIFDSTADDR, SIOCGIFNETMASK, SIOCSIFADDR, SIOCSIFBRDADDR, SIOCSIFDSTADDR, SIOCSIFNETMASK,
};
use crate::sys::systm::{
    net_assert_locked, net_lock, net_lock_shared, net_unlock, net_unlock_shared,
};
use crate::sys::types::{InPort, SaFamily};
use crate::unported;

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

/// An interface address with the lifetime its interface's address list gives it.
fn ifa_static(ifa: &Ifaddr) -> &'static Ifaddr {
    // SAFETY: an address on an interface's list lives until `ifa_del` and its last
    // `ifafree`, as the C's pointers do (`docs/C_TO_RUST.md`, reference-counted pool objects).
    unsafe { &*ptr::from_ref(ifa) }
}

/// The Internet address of an `AF_INET` interface address on a list, with that lifetime.
fn ia_static(ifa: &Ifaddr) -> &'static InIfaddr {
    ifatoia(ifa_static(ifa))
}

/// The family of an interface address.
fn ifa_family(ifa: &Ifaddr) -> SaFamily {
    // SAFETY: an interface address's `ifa_addr` is readable (`ifa_add`'s contract).
    unsafe { (*ifa.ifa_addr.get()).sa_family }
}

/// `in_canforward`: whether datagrams to `in_` may be forwarded (it is not in a reserved set
/// of addresses that may not be).
pub fn in_canforward(in_: InAddr) -> bool {
    if in_multicast(in_.s_addr) {
        return false;
    }
    if in_classa(in_.s_addr) {
        let net = in_.s_addr & IN_CLASSA_NET;
        if net == 0 || net == htonl(IN_LOOPBACKNET << IN_CLASSA_NSHIFT) {
            return false;
        }
    }
    true
}

/// `in_socktrim`: trims a mask in a sockaddr: `sin_len` covers the address up to its last
/// non-zero byte.
pub fn in_socktrim(ap: &mut SockaddrIn) {
    let base = offset_of!(SockaddrIn, sin_addr);
    let bytes = ap.sin_addr.s_addr.to_ne_bytes();

    ap.sin_len = 0;
    if let Some(i) = bytes.iter().rposition(|&b| b != 0) {
        ap.sin_len = (base + i + 1) as u8;
    }
}

/// `in_mask2len`: the prefix length of a contiguous mask.
pub fn in_mask2len(mask: &InAddr) -> i32 {
    let p = mask.s_addr.to_ne_bytes();
    let x = p.iter().position(|&b| b != 0xff).unwrap_or(p.len());
    let mut y = 0;
    if x < p.len() {
        while y < 8 {
            if p[x] & (0x80 >> y) == 0 {
                break;
            }
            y += 1;
        }
    }
    (x * 8) as i32 + y
}

/// `in_len2mask`: the mask of prefix length `len`.
pub fn in_len2mask(mask: &mut InAddr, len: i32) {
    let mut p = [0u8; 4];
    let full = (len / 8) as usize;
    for b in p.iter_mut().take(full) {
        *b = 0xff;
    }
    if len % 8 != 0 {
        p[full] = ((0xff00u32 >> (len % 8)) & 0xff) as u8;
    }
    mask.s_addr = u32::from_ne_bytes(p);
}

/// `in_nam2sin`: the `sockaddr_in` in mbuf `nam`, checked.
pub fn in_nam2sin(nam: &Mbuf) -> Result<*mut SockaddrIn, Errno> {
    let sa = mtod::<Sockaddr>(nam);

    if (nam.m_len().get() as usize) < offset_of!(Sockaddr, sa_data) {
        return Err(Errno::EINVAL);
    }
    // SAFETY: the mbuf holds at least the length and family bytes.
    let (family, len) = unsafe { ((*sa).sa_family, (*sa).sa_len) };
    if family != AF_INET {
        return Err(Errno::EAFNOSUPPORT);
    }
    if u32::from(len) != nam.m_len().get() {
        return Err(Errno::EINVAL);
    }
    if usize::from(len) != size_of::<SockaddrIn>() {
        return Err(Errno::EINVAL);
    }
    Ok(satosin(sa))
}

/// `in_sa2sin`: `sa` as a `sockaddr_in`, checked.
///
/// # Safety
///
/// `sa` points at a readable socket address (at least its length and family).
pub unsafe fn in_sa2sin(sa: *mut Sockaddr) -> Result<*mut SockaddrIn, Errno> {
    // SAFETY: the caller's contract.
    let (family, len) = unsafe { ((*sa).sa_family, (*sa).sa_len) };
    if family != AF_INET {
        return Err(Errno::EAFNOSUPPORT);
    }
    if usize::from(len) != size_of::<SockaddrIn>() {
        return Err(Errno::EINVAL);
    }
    Ok(satosin(sa))
}

/// `in_ifp2ia`: the first Internet address of interface `ifp`.
pub fn in_ifp2ia(ifp: &Ifnet) -> Option<&'static InIfaddr> {
    net_assert_locked("in_ifp2ia");

    ifp.if_addrlist
        .iter()
        .find(|ifa| ifa_family(ifa) == AF_INET)
        .map(ia_static)
}

/// `in_control`: the `pru_control` of the Internet protocols: the address `ioctl`s on
/// interface `ifp`.
///
/// # Safety
///
/// `data` points at the kernel copy of the request, aligned for and as long as the structure
/// `cmd` encodes (`IfIoctlFn`'s contract). `so` is the socket (opaque until sockets are
/// ported); NULL is a request from the kernel itself.
pub unsafe fn in_control(
    so: *const c_void,
    cmd: u64,
    data: *mut u8,
    ifp: Option<&'static Ifnet>,
) -> Result<(), Errno> {
    // so->so_state & SS_PRIV: struct socket is not ported. The kernel's own requests (a NULL
    // socket) are privileged; a socket's privilege cannot be read yet.
    let privileged = so.is_null() || {
        let _ = unported!("in_control: struct socket (SS_PRIV)");
        false
    };

    // MROUTING: SIOCGETVIFCNT, SIOCGETSGCNT through mrt_ioctl; not configured.
    // SAFETY: the caller's contract.
    unsafe { in_ioctl(cmd, data, ifp, privileged) }
}

/// The `ifreq` of an address `ioctl`.
///
/// # Safety
///
/// `data` points at a `struct ifreq`, exclusively the caller's for the returned lifetime.
unsafe fn data_ifreq<'a>(data: *mut u8) -> &'a mut Ifreq {
    // SAFETY: the caller's contract.
    unsafe { &mut *data.cast::<Ifreq>() }
}

/// `in_ioctl`: the Internet address `ioctl`s.
///
/// # Safety
///
/// As for [`in_control`].
pub unsafe fn in_ioctl(
    cmd: u64,
    data: *mut u8,
    ifp: Option<&'static Ifnet>,
    privileged: bool,
) -> Result<(), Errno> {
    let Some(ifp) = ifp else {
        return Err(Errno::ENXIO);
    };

    match cmd {
        SIOCGIFADDR | SIOCGIFNETMASK | SIOCGIFDSTADDR | SIOCGIFBRDADDR => {
            // SAFETY: the caller's contract.
            return unsafe { in_ioctl_get(cmd, data, ifp) };
        }
        SIOCSIFADDR => {
            if !privileged {
                return Err(Errno::EPERM);
            }
            // SAFETY: the caller's contract.
            return unsafe { in_ioctl_set_ifaddr(cmd, data, ifp) };
        }
        SIOCAIFADDR | SIOCDIFADDR => {
            if !privileged {
                return Err(Errno::EPERM);
            }
            // SAFETY: the caller's contract.
            return unsafe { in_ioctl_change_ifaddr(cmd, data, ifp) };
        }
        SIOCSIFNETMASK | SIOCSIFDSTADDR | SIOCSIFBRDADDR => {}
        _ => return Err(Errno::EOPNOTSUPP),
    }

    if !privileged {
        return Err(Errno::EPERM);
    }

    // SAFETY: the caller's contract: these commands take a `struct ifreq`.
    let ifr = unsafe { data_ifreq(data) };
    let mut sin: *mut SockaddrIn = ptr::null_mut();
    if ifr.ifr_addr().sa_family == AF_INET {
        // SAFETY: the request's address.
        sin = unsafe { in_sa2sin(ifr.ifr_addr_mut()) }?;
    }

    net_lock();
    // KERNEL_LOCK(): no kernel lock without MULTIPROCESSOR.

    let mut ia: Option<&'static InIfaddr> = None;
    for ifa in ifp.if_addrlist.iter() {
        if ifa_family(ifa) != AF_INET {
            continue;
        }
        // find first address or exact match
        if ia.is_none() {
            ia = Some(ia_static(ifa));
        }
        // SAFETY: `sin` is NULL or the request's `sockaddr_in`.
        if sin.is_null() || unsafe { (*sin).sin_addr.s_addr } == INADDR_ANY {
            break;
        }
        // SAFETY: as above.
        if ia_static(ifa).ia_addr.get().sin_addr.s_addr == unsafe { (*sin).sin_addr.s_addr } {
            ia = Some(ia_static(ifa));
            break;
        }
    }

    let error = 'err: {
        let Some(ia) = ia else {
            break 'err Err(Errno::EADDRNOTAVAIL);
        };

        match cmd {
            SIOCSIFDSTADDR => {
                if ifp.if_flags.get() & IFF_POINTOPOINT == 0 {
                    break 'err Err(Errno::EINVAL);
                }
                // SAFETY: the request's destination address.
                let sin = match unsafe { in_sa2sin(ifr.ifr_dstaddr_mut()) } {
                    Ok(s) => s,
                    Err(e) => break 'err Err(e),
                };
                let oldaddr = ia.ia_dstaddr.get();
                // SAFETY: checked by `in_sa2sin`.
                ia.ia_dstaddr.set(unsafe { *sin });
                // SAFETY: the C hands the driver the `in_ifaddr` for SIOCSIFDSTADDR.
                let error =
                    unsafe { ifp_ioctl(ifp, SIOCSIFDSTADDR, ptr::from_ref(ia).cast_mut().cast()) };
                if error.is_err() {
                    ia.ia_dstaddr.set(oldaddr);
                    break 'err error;
                }
                let mut old = oldaddr;
                let _ = in_scrubhost(ia, &mut old);
                let _ = in_addhost(ia, ia.ia_dstaddr.as_ptr());
                Ok(())
            }

            SIOCSIFBRDADDR => {
                if ifp.if_flags.get() & IFF_BROADCAST == 0 {
                    break 'err Err(Errno::EINVAL);
                }
                // SAFETY: the request's broadcast address.
                let sin = match unsafe { in_sa2sin(ifr.ifr_broadaddr_mut()) } {
                    Ok(s) => s,
                    Err(e) => break 'err Err(e),
                };
                // SAFETY: a checked `sockaddr_in`; the address is on this interface.
                unsafe { ifa_update_broadaddr(ifp, &ia.ia_ifa, sintosa(sin)) };
                Ok(())
            }

            SIOCSIFNETMASK => {
                if ifr.ifr_addr().sa_len < 8 {
                    break 'err Err(Errno::EINVAL);
                }
                // do not check inet family or strict len
                let sin = satosin(ifr.ifr_addr_mut());
                // SAFETY: the request's address is 16 bytes, a `sockaddr_in`'s size.
                let mask = unsafe { (*sin).sin_addr.s_addr };
                if ntohl(mask) & (!ntohl(mask) >> 1) != 0 {
                    // non-contiguous netmask
                    break 'err Err(Errno::EINVAL);
                }
                ia.ia_netmask.set(mask);
                let mut sm = ia.ia_sockmask.get();
                sm.sin_addr.s_addr = mask;
                ia.ia_sockmask.set(sm);
                Ok(())
            }
            _ => Ok(()),
        }
    };
    // err:
    net_unlock();
    error
}

/// A new, empty Internet address for `ifp` (the C's `malloc(M_ZERO)` and setup in
/// `in_ioctl_set_ifaddr` and `in_ioctl_change_ifaddr`).
fn in_ifaddr_alloc(ifp: &'static Ifnet) -> &'static InIfaddr {
    let Some(mem) = malloc(size_of::<InIfaddr>(), M_IFADDR, M_WAITOK | M_ZERO) else {
        panic(format_args!("in_ifaddr: no memory"));
    };
    // SAFETY: a zeroed block of `size_of::<InIfaddr>()` bytes; all-zero is a valid `InIfaddr`
    // (cells, links, a reference count). It lives until the last `ifafree`.
    let ia: &'static InIfaddr = unsafe { &*mem.as_ptr().cast::<InIfaddr>() };
    refcnt_init_trace(&ia.ia_ifa.ifa_refcnt, DT_REFCNT_IDX_IFADDR);
    let mut addr = ia.ia_addr.get();
    addr.sin_family = AF_INET;
    addr.sin_len = size_of::<SockaddrIn>() as u8;
    ia.ia_addr.set(addr);
    ia.ia_ifa.ifa_addr.set(ia.ia_addr.as_ptr().cast());
    ia.ia_ifa.ifa_dstaddr.set(ia.ia_dstaddr.as_ptr().cast());
    ia.ia_ifa.ifa_netmask.set(ia.ia_sockmask.as_ptr().cast());
    let mut sm = ia.ia_sockmask.get();
    sm.sin_len = 8;
    ia.ia_sockmask.set(sm);
    if ifp.if_flags.get() & IFF_BROADCAST != 0 {
        let mut b = ia.ia_broadaddr().get();
        b.sin_len = size_of::<SockaddrIn>() as u8;
        b.sin_family = AF_INET;
        ia.ia_broadaddr().set(b);
    }
    ia.ia_ifp().set(Some(ifp));
    ia
}

/// `in_ioctl_set_ifaddr`: `SIOCSIFADDR`, the first Internet address of the interface.
///
/// # Safety
///
/// As for [`in_control`]: `data` is a `struct ifreq`.
unsafe fn in_ioctl_set_ifaddr(cmd: u64, data: *mut u8, ifp: &'static Ifnet) -> Result<(), Errno> {
    if cmd != SIOCSIFADDR {
        panic(format_args!("in_ioctl_set_ifaddr: invalid ioctl {cmd}"));
    }

    // SAFETY: the caller's contract.
    let ifr = unsafe { data_ifreq(data) };
    // SAFETY: the request's address.
    let sin = unsafe { in_sa2sin(ifr.ifr_addr_mut()) }?;
    // SAFETY: checked by `in_sa2sin`.
    let sin = unsafe { *sin };

    net_lock();
    // KERNEL_LOCK(): no kernel lock without MULTIPROCESSOR.

    // find first address
    let found = ifp
        .if_addrlist
        .iter()
        .find(|ifa| ifa_family(ifa) == AF_INET)
        .map(ia_static);
    let (ia, newifaddr) = match found {
        Some(ia) => (ia, false),
        None => (in_ifaddr_alloc(ifp), true),
    };

    in_ifscrub(ifp, ia);
    let error = in_ifinit(ifp, ia, &sin, newifaddr);
    if error.is_ok() {
        if_addrhooks_run(ifp);
    }

    net_unlock();
    error
}

/// `in_ioctl_change_ifaddr`: `SIOCAIFADDR` (add or change an address, its mask, destination
/// and broadcast address) and `SIOCDIFADDR` (delete it).
///
/// # Safety
///
/// As for [`in_control`]: `data` is a `struct in_aliasreq`.
unsafe fn in_ioctl_change_ifaddr(
    cmd: u64,
    data: *mut u8,
    ifp: &'static Ifnet,
) -> Result<(), Errno> {
    // SAFETY: the caller's contract.
    let ifra = unsafe { &mut *data.cast::<InAliasreq>() };
    let mut sin: Option<SockaddrIn> = None;
    let mut dstsin: Option<SockaddrIn> = None;
    let mut broadsin: Option<SockaddrIn> = None;
    let mut masksin: Option<SockaddrIn> = None;

    if ifra.ifra_addr().sin_family == AF_INET {
        // SAFETY: the request's address.
        let s = unsafe { in_sa2sin(sintosa(ifra.ifra_addr_mut())) }?;
        // SAFETY: checked by `in_sa2sin`.
        sin = Some(unsafe { *s });
    }

    net_lock();
    // KERNEL_LOCK(): no kernel lock without MULTIPROCESSOR.

    let mut ia: Option<&'static InIfaddr> = None;
    for ifa in ifp.if_addrlist.iter() {
        if ifa_family(ifa) != AF_INET {
            continue;
        }
        // find first address, if no exact match wanted
        if sin.is_none_or(|s| s.sin_addr.s_addr == ia_static(ifa).ia_addr.get().sin_addr.s_addr) {
            ia = Some(ia_static(ifa));
            break;
        }
    }

    let error = 'out: {
        match cmd {
            SIOCAIFADDR => {
                let mut needinit = false;

                if ifra.ifra_mask.sin_len != 0 {
                    if ifra.ifra_mask.sin_len < 8 {
                        break 'out Err(Errno::EINVAL);
                    }
                    // do not check inet family or strict len
                    let m = ifra.ifra_mask;
                    if ntohl(m.sin_addr.s_addr) & (!ntohl(m.sin_addr.s_addr) >> 1) != 0 {
                        // non-contiguous netmask
                        break 'out Err(Errno::EINVAL);
                    }
                    masksin = Some(m);
                }
                if ifp.if_flags.get() & IFF_POINTOPOINT != 0
                    && ifra.ifra_dstaddr.sin_family == AF_INET
                {
                    // SAFETY: the request's destination address.
                    match unsafe { in_sa2sin(sintosa(&mut ifra.ifra_dstaddr)) } {
                        // SAFETY: checked by `in_sa2sin`.
                        Ok(s) => dstsin = Some(unsafe { *s }),
                        Err(e) => break 'out Err(e),
                    }
                }
                if ifp.if_flags.get() & IFF_BROADCAST != 0
                    && ifra.ifra_broadaddr().sin_family == AF_INET
                {
                    // SAFETY: the request's broadcast address.
                    match unsafe { in_sa2sin(sintosa(&mut ifra.ifra_dstaddr)) } {
                        // SAFETY: checked by `in_sa2sin`.
                        Ok(s) => broadsin = Some(unsafe { *s }),
                        Err(e) => break 'out Err(e),
                    }
                }

                let (ia, newifaddr) = match ia {
                    Some(ia) => (ia, false),
                    None => (in_ifaddr_alloc(ifp), true),
                };

                let sin = match sin {
                    None => ia.ia_addr.get(),
                    Some(s) => {
                        if newifaddr || s.sin_addr.s_addr != ia.ia_addr.get().sin_addr.s_addr {
                            needinit = true;
                        }
                        s
                    }
                };
                if let Some(m) = masksin {
                    in_ifscrub(ifp, ia);
                    ia.ia_netmask.set(m.sin_addr.s_addr);
                    let mut sm = ia.ia_sockmask.get();
                    sm.sin_addr.s_addr = m.sin_addr.s_addr;
                    ia.ia_sockmask.set(sm);
                    needinit = true;
                }
                if let Some(d) = dstsin {
                    in_ifscrub(ifp, ia);
                    ia.ia_dstaddr.set(d);
                    needinit = true;
                }
                if let Some(b) = broadsin {
                    if newifaddr {
                        ia.ia_broadaddr().set(b);
                    } else {
                        let mut b = b;
                        // SAFETY: a checked `sockaddr_in`; the address is on this interface.
                        unsafe { ifa_update_broadaddr(ifp, &ia.ia_ifa, sintosa(&mut b)) };
                    }
                }
                if needinit && let Err(e) = in_ifinit(ifp, ia, &sin, newifaddr) {
                    break 'out Err(e);
                }
                if_addrhooks_run(ifp);
                Ok(())
            }
            SIOCDIFADDR => {
                let Some(ia) = ia else {
                    break 'out Err(Errno::EADDRNOTAVAIL);
                };
                // Even if the individual steps were safe, shouldn't these kinds of changes
                // happen atomically? What should happen to a packet that was routed after the
                // scrub but before the other steps?
                in_purgeaddr(&ia.ia_ifa);
                if_addrhooks_run(ifp);
                Ok(())
            }

            _ => panic(format_args!("in_ioctl_change_ifaddr: invalid ioctl {cmd}")),
        }
    };

    net_unlock();
    error
}

/// `in_ioctl_get`: the address `ioctl`s that read: `SIOCGIFADDR`, `SIOCGIFNETMASK`,
/// `SIOCGIFDSTADDR`, `SIOCGIFBRDADDR`.
///
/// # Safety
///
/// As for [`in_control`]: `data` is a `struct ifreq`.
unsafe fn in_ioctl_get(cmd: u64, data: *mut u8, ifp: &'static Ifnet) -> Result<(), Errno> {
    // SAFETY: the caller's contract.
    let ifr = unsafe { data_ifreq(data) };
    let mut sin: *mut SockaddrIn = ptr::null_mut();

    let sa = ifr.ifr_addr_mut();
    if sa.sa_family == AF_INET {
        sa.sa_len = size_of::<SockaddrIn>() as u8;
        // SAFETY: the request's address.
        sin = unsafe { in_sa2sin(sa) }?;
    }

    net_lock_shared();

    let mut ia: Option<&'static InIfaddr> = None;
    for ifa in ifp.if_addrlist.iter() {
        if ifa_family(ifa) != AF_INET {
            continue;
        }
        // find first address or exact match
        if ia.is_none() {
            ia = Some(ia_static(ifa));
        }
        // SAFETY: `sin` is NULL or the request's `sockaddr_in`.
        if sin.is_null() || unsafe { (*sin).sin_addr.s_addr } == INADDR_ANY {
            break;
        }
        // SAFETY: as above.
        if ia_static(ifa).ia_addr.get().sin_addr.s_addr == unsafe { (*sin).sin_addr.s_addr } {
            ia = Some(ia_static(ifa));
            break;
        }
    }

    let error = 'err: {
        let Some(ia) = ia else {
            break 'err Err(Errno::EADDRNOTAVAIL);
        };

        // The request's addresses are 16 bytes, a `sockaddr_in`'s size and alignment 1; the
        // writes are unaligned-safe.
        let put = |dst: *mut Sockaddr, v: SockaddrIn| {
            // SAFETY: `dst` is one of the request's 16-byte addresses.
            unsafe { ptr::write_unaligned(satosin(dst), v) };
        };
        match cmd {
            SIOCGIFADDR => {
                put(ifr.ifr_addr_mut(), ia.ia_addr.get());
                Ok(())
            }

            SIOCGIFBRDADDR => {
                if ifp.if_flags.get() & IFF_BROADCAST == 0 {
                    break 'err Err(Errno::EINVAL);
                }
                put(ifr.ifr_dstaddr_mut(), ia.ia_broadaddr().get());
                Ok(())
            }

            SIOCGIFDSTADDR => {
                if ifp.if_flags.get() & IFF_POINTOPOINT == 0 {
                    break 'err Err(Errno::EINVAL);
                }
                put(ifr.ifr_dstaddr_mut(), ia.ia_dstaddr.get());
                Ok(())
            }

            SIOCGIFNETMASK => {
                put(ifr.ifr_addr_mut(), ia.ia_sockmask.get());
                Ok(())
            }

            _ => panic(format_args!("in_ioctl_get: invalid ioctl {cmd}")),
        }
    };

    net_unlock_shared();
    error
}

/// `in_ifscrub`: deletes any existing route for an interface.
pub fn in_ifscrub(ifp: &Ifnet, ia: &'static InIfaddr) {
    if ifp.if_flags.get() & IFF_POINTOPOINT != 0 {
        let _ = in_scrubhost(ia, ia.ia_dstaddr.as_ptr());
    } else if ifp.if_flags.get() & IFF_LOOPBACK == 0 {
        in_remove_prefix(ia);
    }
}

/// `in_ifinit`: initializes an interface's internet address `sin` and routing table entry.
pub fn in_ifinit(
    ifp: &'static Ifnet,
    ia: &'static InIfaddr,
    sin: &SockaddrIn,
    newaddr: bool,
) -> Result<(), Errno> {
    let i = sin.sin_addr.s_addr;
    let mut error: Result<(), Errno> = Ok(());

    net_assert_locked("in_ifinit");

    // Always remove the address from the tree to make sure its position gets updated in case
    // the key changes.
    if !newaddr {
        let _ = rt_ifa_dellocal(&ia.ia_ifa);
        ifa_del(ifp, &ia.ia_ifa);
    }
    let oldaddr = ia.ia_addr.get();
    ia.ia_addr.set(*sin);

    if ia.ia_netmask.get() == 0 {
        let netmask = if in_classa(i) {
            IN_CLASSA_NET
        } else if in_classb(i) {
            IN_CLASSB_NET
        } else {
            IN_CLASSC_NET
        };
        ia.ia_netmask.set(netmask);
        let mut sm = ia.ia_sockmask.get();
        sm.sin_addr.s_addr = netmask;
        ia.ia_sockmask.set(sm);
    }

    // Give the interface a chance to initialize if this is its first address, and to validate
    // the address if necessary.
    // SAFETY: the C hands the driver the `in_ifaddr` for SIOCSIFADDR; drivers read it as the
    // `struct ifaddr` it starts with, if at all.
    if let Err(e) = unsafe { ifp_ioctl(ifp, SIOCSIFADDR, ptr::from_ref(ia).cast_mut().cast()) } {
        ia.ia_addr.set(oldaddr);
        error = Err(e);
    }

    'out: {
        // Add the address to the local list and the global tree. If an error occurred, put
        // back the original address.
        // SAFETY: the address lives until its last `ifafree`; its socket addresses are its
        // own members.
        unsafe { ifa_add(ifp, &ia.ia_ifa) };
        let rterror = rt_ifa_addlocal(&ia.ia_ifa);

        if let Err(e) = rterror {
            if !newaddr {
                ifa_del(ifp, &ia.ia_ifa);
            }
            if error.is_ok() {
                error = Err(e);
            }
            break 'out;
        }
        if error.is_err() {
            break 'out;
        }

        ia.ia_net.set(i & ia.ia_netmask.get());
        let mut sm = ia.ia_sockmask.get();
        in_socktrim(&mut sm);
        ia.ia_sockmask.set(sm);
        // Add route for the network.
        ia.ia_ifa.ifa_metric.set(ifp.if_metric.get() as i32);
        if ifp.if_flags.get() & IFF_BROADCAST != 0 {
            let mut b = ia.ia_broadaddr().get();
            if in_rfc3021_subnet(ia.ia_netmask.get()) {
                b.sin_addr.s_addr = 0;
            } else {
                b.sin_addr.s_addr = ia.ia_net.get() | !ia.ia_netmask.get();
            }
            ia.ia_broadaddr().set(b);
        }

        if ifp.if_flags.get() & IFF_POINTOPOINT != 0 {
            // XXX We should not even call in_ifinit() in this case.
            if ia.ia_dstaddr.get().sin_family != AF_INET {
                break 'out;
            }
            error = in_addhost(ia, ia.ia_dstaddr.as_ptr());
        } else if ifp.if_flags.get() & IFF_LOOPBACK == 0 {
            error = in_insert_prefix(ia);
        }

        // If the interface supports multicast, join the "all hosts" multicast group on that
        // interface.
        if ifp.if_flags.get() & IFF_MULTICAST != 0 && ia.ia_allhosts.get().is_none() {
            let addr = InAddr {
                s_addr: INADDR_ALLHOSTS_GROUP,
            };
            ia.ia_allhosts.set(in_addmulti(&addr, ifp));
        }
    }

    // out:
    if error.is_err() && newaddr {
        in_purgeaddr(&ia.ia_ifa);
    }

    error
}

/// `in_purgeaddr`: removes an Internet address from its interface and the routing table.
pub fn in_purgeaddr(ifa: &'static Ifaddr) {
    let Some(ifp) = ifa.ifa_ifp.get() else {
        panic(format_args!("in_purgeaddr: address without interface"));
    };
    let ia = ifatoia(ifa);

    net_assert_locked("in_purgeaddr");

    in_ifscrub(ifp, ia);

    let _ = rt_ifa_dellocal(&ia.ia_ifa);
    rt_ifa_purge(&ia.ia_ifa);
    ifa_del(ifp, &ia.ia_ifa);

    if let Some(inm) = ia.ia_allhosts.get() {
        in_delmulti(inm);
        ia.ia_allhosts.set(None);
    }

    ia.ia_ifp().set(None);
    ifafree(&ia.ia_ifa);
}

/// `in_addhost`: the host route of `dst` through `ia`.
fn in_addhost(ia: &'static InIfaddr, dst: *mut SockaddrIn) -> Result<(), Errno> {
    let rdomain = ia.ia_ifp().get().map_or(0, |ifp| ifp.if_rdomain.get());
    // SAFETY: `dst` is one of the address's own `sockaddr_in`s.
    unsafe { rt_ifa_add(&ia.ia_ifa, RTF_HOST | RTF_MPATH, sintosa(dst), rdomain) }
}

/// `in_scrubhost`: removes the host route of `dst` through `ia`.
fn in_scrubhost(ia: &'static InIfaddr, dst: *mut SockaddrIn) -> Result<(), Errno> {
    let rdomain = ia.ia_ifp().get().map_or(0, |ifp| ifp.if_rdomain.get());
    // SAFETY: `dst` is one of the address's own `sockaddr_in`s or a local copy.
    unsafe { rt_ifa_del(&ia.ia_ifa, RTF_HOST, sintosa(dst), rdomain) }
}

/// `in_insert_prefix`: inserts the cloning and broadcast routes for this subnet.
fn in_insert_prefix(ia: &'static InIfaddr) -> Result<(), Errno> {
    let ifa = &ia.ia_ifa;
    let rdomain = ifa.ifa_ifp.get().map_or(0, |ifp| ifp.if_rdomain.get());

    // SAFETY: the address's own socket addresses.
    unsafe {
        rt_ifa_add(
            ifa,
            RTF_CLONING | RTF_CONNECTED | RTF_MPATH,
            ifa.ifa_addr.get(),
            rdomain,
        )?;

        if ia.ia_broadaddr().get().sin_addr.s_addr != 0 {
            return rt_ifa_add(
                ifa,
                RTF_HOST | RTF_BROADCAST | RTF_MPATH,
                ifa.ifa_broadaddr().get(),
                rdomain,
            );
        }
    }

    Ok(())
}

/// `in_remove_prefix`: removes the cloning and broadcast routes for this subnet.
fn in_remove_prefix(ia: &'static InIfaddr) {
    let ifa = &ia.ia_ifa;
    let rdomain = ifa.ifa_ifp.get().map_or(0, |ifp| ifp.if_rdomain.get());

    // SAFETY: the address's own socket addresses.
    unsafe {
        let _ = rt_ifa_del(
            ifa,
            RTF_CLONING | RTF_CONNECTED,
            ifa.ifa_addr.get(),
            rdomain,
        );

        if ia.ia_broadaddr().get().sin_addr.s_addr != 0 {
            let _ = rt_ifa_del(
                ifa,
                RTF_HOST | RTF_BROADCAST,
                ifa.ifa_broadaddr().get(),
                rdomain,
            );
        }
    }
}

/// `in_broadcast`: whether `in_` is a local broadcast address in the routing domain of table
/// `rtableid`.
pub fn in_broadcast(in_: InAddr, rtableid: u32) -> bool {
    net_assert_locked("in_broadcast");

    let rdomain = rtable_l2(rtableid);

    for ifn in IFNETLIST.0.iter() {
        if ifn.if_rdomain.get() != rdomain {
            continue;
        }
        if ifn.if_flags.get() & IFF_BROADCAST == 0 {
            continue;
        }
        for ifa in ifn.if_addrlist.iter() {
            if ifa_family(ifa) == AF_INET {
                let ia = ifatoia(ifa);
                if in_.s_addr != ia.ia_addr.get().sin_addr.s_addr
                    && in_.s_addr == ia.ia_broadaddr().get().sin_addr.s_addr
                {
                    return true;
                }
            }
        }
    }
    false
}

/// `in_lookupmulti`: the multicast record of `addr` on `ifp`, if joined.
pub fn in_lookupmulti(addr: &InAddr, ifp: &Ifnet) -> Option<&'static InMulti> {
    rw_assert_anylock(&ifp.if_maddrlock);

    for ifma in ifp.if_maddrlist.iter() {
        // SAFETY: a record's address is readable (its protocol set it).
        if unsafe { (*ifma.ifma_addr.get()).sa_family } == AF_INET
            && ifmatoinm(ifma).inm_addr().s_addr == addr.s_addr
        {
            // SAFETY: a record on the list lives until `in_delmulti` frees it.
            return Some(unsafe { &*ptr::from_ref(ifmatoinm(ifma)) });
        }
    }
    None
}

/// `in_addmulti`: adds `addr` to the IP multicast addresses of `ifp` (or takes a reference
/// on the record if it is there); `None` when the driver refuses.
pub fn in_addmulti(addr: &InAddr, ifp: &'static Ifnet) -> Option<&'static InMulti> {
    // See if address already in list.
    rw_enter_write(&ifp.if_maddrlock);
    if let Some(inm) = in_lookupmulti(addr, ifp) {
        refcnt_take(inm.inm_refcnt());
        rw_exit_write(&ifp.if_maddrlock);
        return Some(inm);
    }
    rw_exit_write(&ifp.if_maddrlock);

    // New address; allocate a new multicast record and link it into the interface's multicast
    // list.
    let Some(mem) = malloc(size_of::<InMulti>(), M_IPMADDR, M_WAITOK | M_ZERO) else {
        panic(format_args!("in_addmulti: no memory"));
    };
    // SAFETY: a zeroed block of `size_of::<InMulti>()` bytes; all-zero is a valid `InMulti`.
    let new_inm: &'static InMulti = unsafe { &*mem.as_ptr().cast::<InMulti>() };

    // Ask the network driver to update its multicast reception filter appropriately for the
    // new address.
    let mut ifr = Ifreq::zeroed();
    let sin = SockaddrIn {
        sin_len: size_of::<SockaddrIn>() as u8,
        sin_family: AF_INET,
        sin_addr: *addr,
        ..SockaddrIn::default()
    };
    // SAFETY: `ifr_addr` is 16 bytes, a `sockaddr_in`'s size.
    unsafe { ptr::write_unaligned(satosin(ifr.ifr_addr_mut()), sin) };
    // KERNEL_LOCK(): no kernel lock without MULTIPROCESSOR.
    // SAFETY: `ifr` is a `struct ifreq`, what SIOCADDMULTI takes.
    if unsafe { ifp_ioctl(ifp, SIOCADDMULTI, ptr::from_mut(&mut ifr).cast()) }.is_err() {
        free(mem, M_IPMADDR, size_of::<InMulti>());
        return None;
    }

    rw_enter_write(&ifp.if_maddrlock);
    // check again after unlock and lock
    if let Some(inm) = in_lookupmulti(addr, ifp) {
        refcnt_take(inm.inm_refcnt());
        rw_exit_write(&ifp.if_maddrlock);
        free(mem, M_IPMADDR, size_of::<InMulti>());
        return Some(inm);
    }
    let inm = new_inm;
    inm.inm_sin.set(sin);
    refcnt_init_trace(inm.inm_refcnt(), DT_REFCNT_IDX_IFMADDR);
    inm.inm_ifidx().set(ifp.if_index.get());
    inm.inm_ifma.ifma_addr.set(inm.inm_sin.as_ptr().cast());

    // Let IGMP know that we have joined a new IP multicast group.
    // SAFETY: the record lives until `in_delmulti`, which unlinks it first.
    unsafe { ifp.if_maddrlist.insert_head(&inm.inm_ifma) };
    // struct igmp_pktinfo pkt; igmp_joingroup(inm, ifp, &pkt): netinet/igmp.c is not ported,
    // so no membership report is queued (pkt.ipi_ifidx stays 0 and igmp_sendpkt is not
    // reached).
    let _ = unported!("igmp_joingroup (netinet/igmp.c)");
    rw_exit_write(&ifp.if_maddrlock);

    Some(inm)
}

/// `in_delmulti`: drops a reference on a multicast record, deleting it with the last one.
pub fn in_delmulti(inm: &'static InMulti) {
    if !refcnt_rele(inm.inm_refcnt()) {
        return;
    }

    let ifp = if_get(inm.inm_ifidx().get());
    if let Some(ifp) = ifp {
        rw_enter_write(&ifp.if_maddrlock);
        // No remaining claims to this record; let IGMP know that we are leaving the multicast
        // group. igmp_leavegroup(inm, ifp, &pkt): netinet/igmp.c is not ported.
        let _ = unported!("igmp_leavegroup (netinet/igmp.c)");
        // SAFETY: the record is on this interface's list (`in_addmulti`).
        unsafe { ifp.if_maddrlist.remove(&inm.inm_ifma) };
        rw_exit_write(&ifp.if_maddrlock);

        // Notify the network driver to update its multicast reception filter.
        let mut ifr = Ifreq::zeroed();
        let sin = SockaddrIn {
            sin_len: size_of::<SockaddrIn>() as u8,
            sin_family: AF_INET,
            sin_addr: inm.inm_addr(),
            ..SockaddrIn::default()
        };
        // SAFETY: `ifr_addr` is 16 bytes, a `sockaddr_in`'s size.
        unsafe { ptr::write_unaligned(satosin(ifr.ifr_addr_mut()), sin) };
        // KERNEL_LOCK(): no kernel lock without MULTIPROCESSOR.
        // SAFETY: `ifr` is a `struct ifreq`, what SIOCDELMULTI takes.
        let _ = unsafe { ifp_ioctl(ifp, SIOCDELMULTI, ptr::from_mut(&mut ifr).cast()) };

        if_put(ifp);
    }

    free(NonNull::from(inm).cast(), M_IPMADDR, size_of::<InMulti>());
}

/// `in_hasmulti`: whether `ifp` joined the multicast group `addr`.
pub fn in_hasmulti(addr: &InAddr, ifp: &Ifnet) -> bool {
    rw_enter_read(&ifp.if_maddrlock);
    let joined = in_lookupmulti(addr, ifp).is_some();
    rw_exit_read(&ifp.if_maddrlock);

    joined
}

/// `in_ifdetach`: removes every IPv4 address of `ifp`.
pub fn in_ifdetach(ifp: &'static Ifnet) {
    // nuke any of IPv4 addresses we have
    for ifa in ifp.if_addrlist.iter() {
        if ifa_family(ifa) != AF_INET {
            continue;
        }
        in_purgeaddr(ifa_static(ifa));
        if_addrhooks_run(ifp);
    }

    if ifp.if_xflags.get() & IFXF_AUTOCONF4 != 0 {
        ifp.if_xflags.set(ifp.if_xflags.get() & !IFXF_AUTOCONF4);
    }
}

/// `in_prefixlen2mask`: the network-order mask of prefix length `plen`.
pub fn in_prefixlen2mask(maskp: &mut InAddr, plen: i32) {
    if plen == 0 {
        maskp.s_addr = 0;
    } else {
        maskp.s_addr = htonl(0xffff_ffffu32 << (32 - plen));
    }
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
