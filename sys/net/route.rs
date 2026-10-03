/*	$OpenBSD: route.h,v 1.218 2025/07/14 08:48:51 dlg Exp $	*/
/*	$NetBSD: route.h,v 1.9 1996/02/13 22:00:49 christos Exp $	*/
/*	$OpenBSD: route.c,v 1.451 2026/04/22 15:17:43 claudio Exp $	*/
/*	$NetBSD: route.c,v 1.14 1996/02/13 22:00:46 christos Exp $	*/
/* <LICENSES> */
/*
 * Copyright (c) 1980, 1986, 1993
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
 *	@(#)route.h	8.3 (Berkeley) 4/19/94
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
 * Copyright (c) 1980, 1986, 1991, 1993
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
 *	@(#)route.c	8.2 (Berkeley) 11/15/93
 */

/*
 *	@(#)COPYRIGHT	1.1 (NRL) 17 January 1995
 *
 * NRL grants permission for redistribution and use in source and binary
 * forms, with or without modification, of the software and documentation
 * created at NRL provided that the following conditions are met:
 *
 * 1. Redistributions of source code must retain the above copyright
 *    notice, this list of conditions and the following disclaimer.
 * 2. Redistributions in binary form must reproduce the above copyright
 *    notice, this list of conditions and the following disclaimer in the
 *    documentation and/or other materials provided with the distribution.
 * 3. All advertising materials mentioning features or use of this software
 *    must display the following acknowledgements:
 *	This product includes software developed by the University of
 *	California, Berkeley and its contributors.
 *	This product includes software developed at the Information
 *	Technology Division, US Naval Research Laboratory.
 * 4. Neither the name of the NRL nor the names of its contributors
 *    may be used to endorse or promote products derived from this software
 *    without specific prior written permission.
 *
 * THE SOFTWARE PROVIDED BY NRL IS PROVIDED BY NRL AND CONTRIBUTORS ``AS
 * IS'' AND ANY EXPRESS OR IMPLIED WARRANTIES, INCLUDING, BUT NOT LIMITED
 * TO, THE IMPLIED WARRANTIES OF MERCHANTABILITY AND FITNESS FOR A
 * PARTICULAR PURPOSE ARE DISCLAIMED.  IN NO EVENT SHALL NRL OR
 * CONTRIBUTORS BE LIABLE FOR ANY DIRECT, INDIRECT, INCIDENTAL, SPECIAL,
 * EXEMPLARY, OR CONSEQUENTIAL DAMAGES (INCLUDING, BUT NOT LIMITED TO,
 * PROCUREMENT OF SUBSTITUTE GOODS OR SERVICES; LOSS OF USE, DATA, OR
 * PROFITS; OR BUSINESS INTERRUPTION) HOWEVER CAUSED AND ON ANY THEORY OF
 * LIABILITY, WHETHER IN CONTRACT, STRICT LIABILITY, OR TORT (INCLUDING
 * NEGLIGENCE OR OTHERWISE) ARISING IN ANY WAY OUT OF THE USE OF THIS
 * SOFTWARE, EVEN IF ADVISED OF THE POSSIBILITY OF SUCH DAMAGE.
 *
 * The views and conclusions contained in the software and documentation
 * are those of the authors and should not be interpreted as representing
 * official policies, either expressed or implied, of the US Naval
 * Research Laboratory (NRL).
 */
/* </LICENSES> */

//! Routing tables and the routing socket's messages: `<net/route.h>`.
//!
//! Upstream: sys/net/route.h @ 3ce1f3f79392
//! Upstream: sys/net/route.c @ 3ce1f3f79392
//!
//! Kernel resident routing tables. The routing tables are initialized when interface addresses
//! are set by making entries for all directly connected interfaces. Routes to hosts are
//! distinguished from routes to networks, preferring the former if available; routes that
//! forward packets through gateways are marked with `RTF_GATEWAY` so that the output routines
//! know to address the gateway rather than the ultimate destination.
//!
//! The constants take the type of the field that stores them: `RTF_*` are `u32` (`rt_flags`),
//! `RTM_*` message types `u8` (`rtm_type`), `RTA_*` `i32` (`rtm_addrs`), `RTAX_*` `usize`
//! (indices into `rti_info`), `RTP_*` `u8` (`rt_priority`), `RTV_*` `u32` (`rtm_inits`).
//!
//! Status: `wip`.
//!
//! ## Deviations
//! - `struct rtentry` (with the `rt_use`, `rt_expire`, `rt_locks`, `rt_mtu` shorthands),
//!   `struct rt_addrinfo`, `struct rttimer_queue` and the `rtstat`/`rtgeneration` globals come
//!   with `net/route.c`: they embed `struct rttimer`, which that port brings. Until then
//!   [`Rtentry`] stands in for the C's forward declaration `struct rtentry;` (which
//!   `<net/if_var.h>` also makes): an uninhabited type, so every `struct rtentry *` is NULL
//!   (`None`), with the accessors the interface layer reads (`rt_flags`, `rt_mtu`, `rt_key`,
//!   `rt_ifa`, `rt_ifidx`, `rt_gateway`), whose bodies can never run.
//! - `struct route` comes with `<netinet6/in6.h>` (it holds a `sockaddr_in6` and an
//!   `in6_addr`).
//! - `rtstat_inc` comes with the per-CPU counters (`<sys/percpu.h>`); the
//!   [`RtstatCounters`] it indexes are here.
//! - `RTTTOPRHZ(r)` comes with `<sys/protosw.h>` (`PR_SLOWHZ`).
//! - `srtdnstosa` is a pointer cast (`docs/C_TO_RUST.md`).
//! - The prototypes (`rtalloc`, `rtrequest`, `rt_timer_*`, `rtm_*`, ...) come with
//!   `net/route.c` and `net/rtsock.c`. Of `net/route.c` only `ifaref` and `ifafree` are here
//!   (M7b, for `net/if.c`); the rest of the file is not ported yet. `ifafree` frees the
//!   address with `free(ifa, M_IFADDR, 0)`, as the C does, so an address that reaches it was
//!   `malloc`ed by its protocol.

use core::cell::Cell;
use core::mem::size_of;
use core::ptr::NonNull;
use core::sync::atomic::AtomicU32;

use crate::kern::kern_malloc::free;
use crate::kern::kern_synch::{refcnt_rele, refcnt_take};
use crate::sys::malloc::M_IFADDR;

use crate::net::if_var::Ifaddr;
use crate::sys::socket::Sockaddr;
use crate::sys::types::{Pid, SaFamily};

/// Units for rtt, rttvar, as units per sec: `rmx_rtt` and `rmx_rttvar` are stored as
/// microseconds.
pub const RTM_RTTUNIT: u32 = 1_000_000;

// Bitmask values for rtm_flags.

/// Route usable.
pub const RTF_UP: u32 = 0x1;
/// Destination is a gateway.
pub const RTF_GATEWAY: u32 = 0x2;
/// Host entry (net otherwise).
pub const RTF_HOST: u32 = 0x4;
/// Host or net unreachable.
pub const RTF_REJECT: u32 = 0x8;
/// Created dynamically (by redirect).
pub const RTF_DYNAMIC: u32 = 0x10;
/// Modified dynamically (by redirect).
pub const RTF_MODIFIED: u32 = 0x20;
/// Message confirmed.
pub const RTF_DONE: u32 = 0x40;
/// Generate new routes on use.
pub const RTF_CLONING: u32 = 0x100;
/// Route associated to a mcast addr.
pub const RTF_MULTICAST: u32 = 0x200;
/// Generated by ARP or ND.
pub const RTF_LLINFO: u32 = 0x400;
/// Manually added.
pub const RTF_STATIC: u32 = 0x800;
/// Just discard pkts (during updates).
pub const RTF_BLACKHOLE: u32 = 0x1000;
/// Protocol specific routing flag.
pub const RTF_PROTO3: u32 = 0x2000;
/// Protocol specific routing flag.
pub const RTF_PROTO2: u32 = 0x4000;
/// Announce L2 entry.
pub const RTF_ANNOUNCE: u32 = RTF_PROTO2;
/// Protocol specific routing flag.
pub const RTF_PROTO1: u32 = 0x8000;
/// This is a cloned route.
pub const RTF_CLONED: u32 = 0x10000;
/// Cached by a `RTF_GATEWAY` entry.
pub const RTF_CACHED: u32 = 0x20000;
/// Multipath route or operation.
pub const RTF_MPATH: u32 = 0x40000;
/// MPLS additional infos.
pub const RTF_MPLS: u32 = 0x100000;
/// Route to a local address.
pub const RTF_LOCAL: u32 = 0x200000;
/// Route associated to a bcast addr.
pub const RTF_BROADCAST: u32 = 0x400000;
/// Interface route.
pub const RTF_CONNECTED: u32 = 0x800000;
/// Link state controlled by BFD.
pub const RTF_BFD: u32 = 0x1000000;

/// Mask of RTF flags that are allowed to be modified by `RTM_CHANGE`.
pub const RTF_FMASK: u32 = RTF_LLINFO
    | RTF_PROTO1
    | RTF_PROTO2
    | RTF_PROTO3
    | RTF_BLACKHOLE
    | RTF_REJECT
    | RTF_STATIC
    | RTF_MPLS
    | RTF_BFD;

// Routing priorities used by the different routing protocols.

/// Unset priority use sane default.
pub const RTP_NONE: u8 = 0;
/// Local address routes (must be the highest).
pub const RTP_LOCAL: u8 = 1;
/// Directly connected routes.
pub const RTP_CONNECTED: u8 = 4;
/// Static routes base priority.
pub const RTP_STATIC: u8 = 8;
/// EIGRP routes.
pub const RTP_EIGRP: u8 = 28;
/// OSPF routes.
pub const RTP_OSPF: u8 = 32;
/// IS-IS routes.
pub const RTP_ISIS: u8 = 36;
/// RIP routes.
pub const RTP_RIP: u8 = 40;
/// BGP routes.
pub const RTP_BGP: u8 = 48;
/// Routes that have nothing set.
pub const RTP_DEFAULT: u8 = 56;
/// `RTP_PROPOSAL_STATIC`.
pub const RTP_PROPOSAL_STATIC: u8 = 57;
/// `RTP_PROPOSAL_DHCLIENT`.
pub const RTP_PROPOSAL_DHCLIENT: u8 = 58;
/// `RTP_PROPOSAL_SLAAC`.
pub const RTP_PROPOSAL_SLAAC: u8 = 59;
/// `RTP_PROPOSAL_UMB`.
pub const RTP_PROPOSAL_UMB: u8 = 60;
/// `RTP_PROPOSAL_PPP`.
pub const RTP_PROPOSAL_PPP: u8 = 61;
/// Request reply of all `RTM_PROPOSAL`.
pub const RTP_PROPOSAL_SOLICIT: u8 = 62;
/// Maximum priority.
pub const RTP_MAX: u8 = 63;
/// Any of the above.
pub const RTP_ANY: u8 = 64;
/// `RTP_MASK`.
pub const RTP_MASK: u8 = 0x7f;
/// Route/link is down.
pub const RTP_DOWN: u8 = 0x80;

/// Up the ante and ignore older versions.
pub const RTM_VERSION: u8 = 5;

/// Maximum size of an accepted route msg.
pub const RTM_MAXSIZE: usize = 2048;

// Values for rtm_type.

/// Add Route.
pub const RTM_ADD: u8 = 0x1;
/// Delete Route.
pub const RTM_DELETE: u8 = 0x2;
/// Change Metrics or flags.
pub const RTM_CHANGE: u8 = 0x3;
/// Report Metrics.
pub const RTM_GET: u8 = 0x4;
/// Kernel Suspects Partitioning.
pub const RTM_LOSING: u8 = 0x5;
/// Told to use different route.
pub const RTM_REDIRECT: u8 = 0x6;
/// Lookup failed on this address.
pub const RTM_MISS: u8 = 0x7;
/// Req to resolve dst to LL addr.
pub const RTM_RESOLVE: u8 = 0xb;
/// Address being added to iface.
pub const RTM_NEWADDR: u8 = 0xc;
/// Address being removed from iface.
pub const RTM_DELADDR: u8 = 0xd;
/// Iface going up/down etc.
pub const RTM_IFINFO: u8 = 0xe;
/// Iface arrival/departure.
pub const RTM_IFANNOUNCE: u8 = 0xf;
/// Route socket buffer overflow.
pub const RTM_DESYNC: u8 = 0x10;
/// Invalidate cache of L2 route.
pub const RTM_INVALIDATE: u8 = 0x11;
/// Bidirectional forwarding detection.
pub const RTM_BFD: u8 = 0x12;
/// Proposal for resolvd(8).
pub const RTM_PROPOSAL: u8 = 0x13;
/// Address attribute change.
pub const RTM_CHGADDRATTR: u8 = 0x14;
/// 80211 iface change.
pub const RTM_80211INFO: u8 = 0x15;
/// Set source address.
pub const RTM_SOURCE: u8 = 0x16;

/// Init or lock `_mtu`.
pub const RTV_MTU: u32 = 0x1;
/// Init or lock `_hopcount`.
pub const RTV_HOPCOUNT: u32 = 0x2;
/// Init or lock `_expire`.
pub const RTV_EXPIRE: u32 = 0x4;
/// Init or lock `_recvpipe`.
pub const RTV_RPIPE: u32 = 0x8;
/// Init or lock `_sendpipe`.
pub const RTV_SPIPE: u32 = 0x10;
/// Init or lock `_ssthresh`.
pub const RTV_SSTHRESH: u32 = 0x20;
/// Init or lock `_rtt`.
pub const RTV_RTT: u32 = 0x40;
/// Init or lock `_rttvar`.
pub const RTV_RTTVAR: u32 = 0x80;

// Bitmask values for rtm_addrs.

/// Destination sockaddr present.
pub const RTA_DST: i32 = 0x1;
/// Gateway sockaddr present.
pub const RTA_GATEWAY: i32 = 0x2;
/// Netmask sockaddr present.
pub const RTA_NETMASK: i32 = 0x4;
/// Cloning mask sockaddr present.
pub const RTA_GENMASK: i32 = 0x8;
/// Interface name sockaddr present.
pub const RTA_IFP: i32 = 0x10;
/// Interface addr sockaddr present.
pub const RTA_IFA: i32 = 0x20;
/// Sockaddr for author of redirect.
pub const RTA_AUTHOR: i32 = 0x40;
/// For NEWADDR, broadcast or p-p dest addr.
pub const RTA_BRD: i32 = 0x80;
/// Source sockaddr present.
pub const RTA_SRC: i32 = 0x100;
/// Source netmask present.
pub const RTA_SRCMASK: i32 = 0x200;
/// Route label present.
pub const RTA_LABEL: i32 = 0x400;
/// BFD present.
pub const RTA_BFD: i32 = 0x800;
/// DNS Servers sockaddr present.
pub const RTA_DNS: i32 = 0x1000;
/// RFC 3442 encoded static routes present.
pub const RTA_STATIC: i32 = 0x2000;
/// RFC 3397 encoded search path present.
pub const RTA_SEARCH: i32 = 0x4000;

// Index offsets for sockaddr array for alternate internal encoding.

/// Destination sockaddr present.
pub const RTAX_DST: usize = 0;
/// Gateway sockaddr present.
pub const RTAX_GATEWAY: usize = 1;
/// Netmask sockaddr present.
pub const RTAX_NETMASK: usize = 2;
/// Cloning mask sockaddr present.
pub const RTAX_GENMASK: usize = 3;
/// Interface name sockaddr present.
pub const RTAX_IFP: usize = 4;
/// Interface addr sockaddr present.
pub const RTAX_IFA: usize = 5;
/// Sockaddr for author of redirect.
pub const RTAX_AUTHOR: usize = 6;
/// For NEWADDR, broadcast or p-p dest addr.
pub const RTAX_BRD: usize = 7;
/// Source sockaddr present.
pub const RTAX_SRC: usize = 8;
/// Source netmask present.
pub const RTAX_SRCMASK: usize = 9;
/// Route label present.
pub const RTAX_LABEL: usize = 10;
/// BFD present.
pub const RTAX_BFD: usize = 11;
/// DNS Server(s) sockaddr present.
pub const RTAX_DNS: usize = 12;
/// RFC 3442 encoded static routes present.
pub const RTAX_STATIC: usize = 13;
/// RFC 3397 encoded search path present.
pub const RTAX_SEARCH: usize = 14;
/// Size of array to allocate.
pub const RTAX_MAX: usize = 15;

// setsockopt defines used for the filtering.

/// Bitmask to specify which types should be sent to the client.
pub const ROUTE_MSGFILTER: i32 = 1;
/// Change routing table the socket is listening on, `RTABLE_ANY` listens on all tables.
pub const ROUTE_TABLEFILTER: i32 = 2;
/// Only pass updates with a priority higher or equal (actual value lower) to the specified
/// priority.
pub const ROUTE_PRIOFILTER: i32 = 3;
/// Do not pass updates for routes with flags in this bitmask.
pub const ROUTE_FLAGFILTER: i32 = 4;

/// Every routing table.
pub const RTABLE_ANY: u32 = 0xffff_ffff;

/// Length of a route label.
pub const RTLABEL_LEN: usize = 32;

/// Length of the DNS server data of a `sockaddr_rtdns`.
pub const RTDNS_LEN: usize = 128;

/// Length of the static routes of a `sockaddr_rtstatic`.
pub const RTSTATIC_LEN: usize = 128;

/// Length of the search path of a `sockaddr_rtsearch`.
pub const RTSEARCH_LEN: usize = 128;

/// Values for additional argument to `rtalloc()`.
pub const RT_RESOLVE: i32 = 1;

/// `struct rtentry`, as the forward declaration `struct rtentry;`: the routing table entries
/// come with `net/route.c`. The type has no values yet, so `Option<&Rtentry>` is always
/// `None`, and the accessors below (the members the interface layer reads) cannot be called.
pub enum Rtentry {}

impl Rtentry {
    /// `rt_flags`: up/down?, host/net.
    pub fn rt_flags(&self) -> &Cell<u32> {
        match *self {}
    }

    /// `rt_mtu` (`rt_rmx.rmx_mtu`): MTU for this path, changed with `atomic_cas_uint`.
    pub fn rt_mtu(&self) -> &AtomicU32 {
        match *self {}
    }

    /// `rt_key(rt)` (`rt_dest`): the destination.
    pub fn rt_key(&self) -> *const Sockaddr {
        match *self {}
    }

    /// `rt_gateway`: gateway address.
    pub fn rt_gateway(&self) -> *const Sockaddr {
        match *self {}
    }

    /// `rt_ifa`: interface address to use.
    pub fn rt_ifa(&self) -> Option<&'static Ifaddr> {
        match *self {}
    }

    /// `rt_ifidx`: interface to use.
    pub fn rt_ifidx(&self) -> u32 {
        match *self {}
    }
}

/// `struct rt_kmetrics`: these numbers are used by reliable protocols for determining
/// retransmission behavior and are included in the routing structure.
#[repr(C)]
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct RtKmetrics {
    /// Packets sent using this route.
    pub rmx_pksent: u64,
    /// Lifetime for route, e.g. redirect.
    pub rmx_expire: i64,
    /// Kernel must leave these values.
    pub rmx_locks: u32,
    /// MTU for this path.
    pub rmx_mtu: u32,
}

/// `struct rt_metrics`: huge version for userland compatibility.
#[repr(C)]
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct RtMetrics {
    /// Packets sent using this route.
    pub rmx_pksent: u64,
    /// Lifetime for route, e.g. redirect.
    pub rmx_expire: i64,
    /// Kernel must leave these values.
    pub rmx_locks: u32,
    /// MTU for this path.
    pub rmx_mtu: u32,
    /// # references hold.
    pub rmx_refcnt: u32,
    /// Max hops expected (no longer used; some apps may still need it).
    pub rmx_hopcount: u32,
    /// Inbound delay-bandwidth product.
    pub rmx_recvpipe: u32,
    /// Outbound delay-bandwidth product.
    pub rmx_sendpipe: u32,
    /// Outbound gateway buffer limit.
    pub rmx_ssthresh: u32,
    /// Estimated round trip time.
    pub rmx_rtt: u32,
    /// Estimated rtt variance.
    pub rmx_rttvar: u32,
    /// Padding.
    pub rmx_pad: u32,
}

/// `struct rtstat`: routing statistics.
#[repr(C)]
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Rtstat {
    /// Bogus redirect calls.
    pub rts_badredirect: u32,
    /// Routes created by redirects.
    pub rts_dynamic: u32,
    /// Routes modified by redirects.
    pub rts_newgateway: u32,
    /// Lookups which failed.
    pub rts_unreach: u32,
    /// Lookups satisfied by a wildcard.
    pub rts_wildcard: u32,
}

/// `struct rt_tableinfo`: routing table info.
#[repr(C)]
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct RtTableinfo {
    /// Routing table id.
    pub rti_tableid: u16,
    /// Routing domain id.
    pub rti_domainid: u16,
}

/// `struct rt_msghdr`: structures for routing messages.
#[repr(C)]
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct RtMsghdr {
    /// To skip over non-understood messages.
    pub rtm_msglen: u16,
    /// Future binary compatibility.
    pub rtm_version: u8,
    /// Message type.
    pub rtm_type: u8,
    /// sizeof(rt_msghdr) to skip over the header.
    pub rtm_hdrlen: u16,
    /// Index for associated ifp.
    pub rtm_index: u16,
    /// Routing table id.
    pub rtm_tableid: u16,
    /// Routing priority.
    pub rtm_priority: u8,
    /// MPLS additional infos.
    pub rtm_mpls: u8,
    /// Bitmask identifying sockaddrs in msg.
    pub rtm_addrs: i32,
    /// Flags, incl. kern & message, e.g. DONE.
    pub rtm_flags: i32,
    /// Bitmask used in `RTM_CHANGE` message.
    pub rtm_fmask: i32,
    /// Identify sender.
    pub rtm_pid: Pid,
    /// For sender to identify action.
    pub rtm_seq: i32,
    /// Why failed.
    pub rtm_errno: i32,
    /// Which metrics we are initializing.
    pub rtm_inits: u32,
    /// Metrics themselves.
    pub rtm_rmx: RtMetrics,
}

impl RtMsghdr {
    /// `rtm_use`: overload of the no longer used field `rtm_rmx.rmx_pksent`.
    pub fn rtm_use(&self) -> u64 {
        self.rtm_rmx.rmx_pksent
    }
}

/// `struct sockaddr_rtlabel`: a route label as a socket address.
#[repr(C)]
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct SockaddrRtlabel {
    /// Total length.
    pub sr_len: u8,
    /// Address family.
    pub sr_family: SaFamily,
    /// The label.
    pub sr_label: [u8; RTLABEL_LEN],
}

/// `struct sockaddr_rtdns`: DNS servers as a socket address.
#[repr(C)]
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct SockaddrRtdns {
    /// Total length.
    pub sr_len: u8,
    /// Address family.
    pub sr_family: SaFamily,
    /// The servers.
    pub sr_dns: [u8; RTDNS_LEN],
}

/// `struct sockaddr_rtstatic`: RFC 3442 static routes as a socket address.
#[repr(C)]
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct SockaddrRtstatic {
    /// Total length.
    pub sr_len: u8,
    /// Address family.
    pub sr_family: SaFamily,
    /// The routes.
    pub sr_static: [u8; RTSTATIC_LEN],
}

/// `struct sockaddr_rtsearch`: an RFC 3397 search path as a socket address.
#[repr(C)]
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct SockaddrRtsearch {
    /// Total length.
    pub sr_len: u8,
    /// Address family.
    pub sr_family: SaFamily,
    /// The search path.
    pub sr_search: [u8; RTSEARCH_LEN],
}

/// `enum rtstat_counters`: the per-CPU routing statistics, one per field of [`Rtstat`].
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[repr(u32)]
pub enum RtstatCounters {
    /// Bogus redirect calls.
    RtsBadredirect,
    /// Routes created by redirects.
    RtsDynamic,
    /// Routes modified by redirects.
    RtsNewgateway,
    /// Lookups which failed.
    RtsUnreach,
    /// Lookups satisfied by a wildcard.
    RtsWildcard,
    /// The number of counters.
    RtsNcounters,
}

/// `ROUTE_FILTER(m)`: the `ROUTE_MSGFILTER` bit of message type `m`.
pub const fn route_filter(m: u8) -> u32 {
    1 << m
}

/// `srtdnstosa(sdns)`: a `sockaddr_rtdns` seen as a generic `sockaddr`.
pub const fn srtdnstosa(sdns: *mut SockaddrRtdns) -> *mut Sockaddr {
    sdns.cast()
}

/// `ifaref`: takes a reference to an interface address (a route's `rt_ifa`).
pub fn ifaref(ifa: &Ifaddr) -> &Ifaddr {
    refcnt_take(&ifa.ifa_refcnt);
    ifa
}

/// `ifafree`: drops a reference to an interface address, freeing it with the last one.
pub fn ifafree(ifa: &Ifaddr) {
    if !refcnt_rele(&ifa.ifa_refcnt) {
        return;
    }
    free(NonNull::from(ifa).cast(), M_IFADDR, 0);
}

// LP64 sizes of the C structures.
const _: () = {
    assert!(size_of::<RtKmetrics>() == 24);
    assert!(size_of::<RtMetrics>() == 56);
    assert!(size_of::<Rtstat>() == 20);
    assert!(size_of::<RtTableinfo>() == 4);
    assert!(size_of::<RtMsghdr>() == 96);
    assert!(size_of::<SockaddrRtlabel>() == 34);
    assert!(size_of::<SockaddrRtdns>() == 130);
    assert!(size_of::<SockaddrRtstatic>() == 130);
    assert!(size_of::<SockaddrRtsearch>() == 130);
};

#[cfg(test)]
mod tests;
