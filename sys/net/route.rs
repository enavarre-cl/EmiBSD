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

//! Routing tables and the routing socket's messages: `<net/route.h>` and `net/route.c`.
//!
//! Upstream: sys/net/route.h @ 3ce1f3f79392
//! Upstream: sys/net/route.c @ 3ce1f3f79392
//!
//! Kernel resident routing tables. The routing tables are initialized when interface addresses
//! are set by making entries for all directly connected interfaces. Routes to hosts are
//! distinguished from routes to networks, preferring the former if available; for each route
//! the interface to use is inferred from the gateway address supplied when the route was
//! entered. Routes that forward packets through gateways are marked with `RTF_GATEWAY` so that
//! the output routines know to address the gateway rather than the ultimate destination. With
//! `RTF_GATEWAY` set, `rt_gwroute` points at the `rtentry` of the gateway (the next hop's
//! link-layer route); without it, `rt_cachecnt` counts the `RTF_GATEWAY` entries whose
//! `rt_gwroute` points here.
//!
//! The constants take the type of the field that stores them: `RTF_*` are `u32` (`rt_flags`),
//! `RTM_*` message types `u8` (`rtm_type`), `RTA_*` `i32` (`rtm_addrs`), `RTAX_*` `usize`
//! (indices into `rti_info`), `RTP_*` `u8` (`rt_priority`), `RTV_*` `u32` (`rtm_inits`).
//!
//! The locks named in the field docs are the C's: \[I\] immutable after creation, \[N\] net
//! lock, \[X\] exclusive net lock (or shared net lock and kernel lock), \[R\] the rtable lock,
//! \[r\] the route's `rt_mtx`, \[L\] the ARP/ND lock for updates, net lock for reads, \[T\]
//! `rttimer_mtx`, \[a\] atomic operations.
//!
//! Status: `route.h` `wip` (`struct route`'s IPv6 members, see below), `route.c` `ported`
//! (M7b). The licence block carries the NRL notice with its advertising clause, accepted as
//! BSD-4 (`.claude/rules/scope-and-stubs.md`).
//!
//! ## Deviations
//! - `struct rtentry` is shared through `&'static Rtentry`, as `struct rtentry *` is: a route
//!   is a pool item that lives until its last `rtfree` (`docs/C_TO_RUST.md`, reference-counted
//!   pool objects). Its members are `Cell`s and atomics (the C changes them through the
//!   pointer under the locks above), so the all-zero value `PR_ZERO` gives is valid. The
//!   `rt_use`, `rt_expire`, `rt_locks` and `rt_mtu` shorthands are methods returning the
//!   metric's cell; `rt->rt_ifa`, which the C dereferences without a check, is
//!   [`Rtentry::ifa`], which panics on a route without an address (one `rtrequest` did not
//!   make).
//! - `struct rt_kmetrics` (kernel only) holds cells and `rmx_mtu` is an `AtomicU32` (the C's
//!   `[a]`); `rt->rt_rmx = parent->rt_rmx` is [`RtKmetrics::assign`].
//! - `struct route` has `Cell` members so a cache inside a shared structure (`struct
//!   netstack`'s `ns_route`) can be refreshed through `&`; its unions are `#[repr(C)]` unions
//!   with accessors. `struct sockaddr_in6` and `struct in6_addr` (`<netinet6/in6.h>`) are not
//!   ported, so `ro_dstsin6` and `ro_srcin6` are byte arrays of their sizes and the `INET6`
//!   functions (`route6_cache`, `route6_mpath`) are not configured. A mask buffer (`struct
//!   sockaddr_in6 sa_mask` in C, big enough for any family) is a `sockaddr_storage`.
//! - `struct rt_addrinfo`'s `rti_flags` is a `u32` (`int` in C), the type of the `RTF_*`
//!   values it carries; `rti_info[]` holds raw socket addresses, so the functions that read
//!   them (`rtrequest`, `rt_ifa_add`, `rtalloc`, ...) are `unsafe fn`s whose contract is that
//!   every non-NULL address is readable for its `sa_len` (`docs/C_TO_RUST.md`).
//! - `struct rttimer` (private to `route.c` in C) is here with the queue it links to, because
//!   `struct rtentry`'s `rt_timer` list names it; its queue functions keep the C's names.
//! - Out parameters: `struct rtentry **ret_nrt` is `Option<&mut Option<&'static Rtentry>>`
//!   (`RTM_RESOLVE` reads the cloning route from it); `rt_copysa` returns the copy; errors are
//!   `Result<(), Errno>` (`route_cache`'s "miss" is `Err(ESRCH)`, as its `int` is).
//!   `rt_hash`'s -1 is `None`.
//! - `rtcounters` (`struct cpumem *`, `<sys/percpu.h>` not ported) is one static array of
//!   atomics, as `uvmexp` and `mbstat` are; `rtstat_inc` bumps it.
//! - `MPLS` and `BFD` are not configured: `rt_mpls_set`/`rt_mpls_clear`, `bfdinit` and
//!   `bfdclear` are comments at their sites. `INET6` likewise (`rt_hash`'s, `rt_plentosa`'s
//!   and `rt_ifa_addlocal`'s `AF_INET6` cases).
//! - `KERNEL_LOCK()`/`KERNEL_UNLOCK()` are nothing without `MULTIPROCESSOR`; `membar_producer()`
//!   before bumping `rtgeneration` is a release fence.
//! - The routing socket side (`rtm_send`, `rtm_miss`, `rtm_addr`, ...) is `net/rtsock.rs`;
//!   `rtlabel_id2name` fills a caller's byte buffer and returns the name's slice.
//! - `ifafree` frees the address with `free(ifa, M_IFADDR, 0)`, as the C does, so an address
//!   that reaches it was `malloc`ed by its protocol.

use core::cell::Cell;
use core::ffi::c_void;
use core::mem::size_of;
use core::ptr::{self, NonNull};
use core::slice;
use core::sync::atomic::{AtomicI32, AtomicU32, AtomicU64, Ordering, fence};

use crate::db_printf;
use crate::dev::rnd::arc4random;
use crate::kassert;
use crate::kern::kern_lock::{mtx_enter, mtx_init, mtx_init_flags, mtx_leave};
use crate::kern::kern_malloc::{free, malloc};
use crate::kern::kern_synch::{refcnt_init_trace, refcnt_rele, refcnt_take};
use crate::kern::kern_tc::getuptime;
use crate::kern::kern_timeout::{timeout_add_sec, timeout_del, timeout_set_proc};
use crate::kern::subr_pool::{pool_get, pool_init, pool_put};
use crate::kern::uipc_domain::DOMAINS;
use crate::kern::uipc_mbuf::{m_free, m_get};
use crate::machine::intr::{IPL_MPFLOOR, IPL_NET, IPL_SOFTNET};
use crate::net::if_::{
    IFF_LOOPBACK, IFF_POINTOPOINT, IFF_UP, if_get, if_group_routechange, if_put, ifa_ifwithaddr,
    link_state_is_up,
};
use crate::net::if_dl::{SockaddrDl, sdltosa};
use crate::net::if_var::{Ifaddr, Ifnet};
use crate::net::rtable::{
    RTMAP_LIMIT, rt_key, rt_plen, rt_root, rtable_clearsource, rtable_delete, rtable_exists,
    rtable_insert, rtable_iterate, rtable_l2, rtable_lookup, rtable_match, rtable_mpath_capable,
    rtable_mpath_reprio, rtable_satoplen, rtable_walk,
};
use crate::net::rtsock::{rtm_addr, rtm_miss, rtm_send};
use crate::netinet::in_::{
    INADDR_ANY, InAddr, SockaddrIn, in_broadcast, in_prefixlen2mask, satosin_const,
};
use crate::netinet::ip_input::IPMULTIPATH;
use crate::netinet::ip_var::{IpstatCounters, ipstat_inc};
use crate::queue_adapter;
use crate::sys::errno::Errno;
use crate::sys::malloc::{M_IFADDR, M_NOWAIT, M_RTABLE, M_ZERO};
use crate::sys::mbuf::{M_DONTWAIT, MT_SONAME, mtod};
use crate::sys::mutex::Mutex;
use crate::sys::pool::{PR_NOWAIT, PR_ZERO, Pool};
use crate::sys::queue::{ListEntry, ListHead, TailqEntry, TailqHead};
use crate::sys::refcnt::{DT_REFCNT_IDX_RTENTRY, Refcnt};
use crate::sys::socket::{AF_INET, AF_LINK, AF_MAX, AF_UNSPEC, Sockaddr, SockaddrStorage};
use crate::sys::systm::net_assert_locked;
use crate::sys::timeout::Timeout;
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

/// `ROUNDUP(a)`: `a` rounded up to a multiple of `sizeof(long)`, `sizeof(long)` for 0.
const fn roundup_long(a: usize) -> usize {
    if a > 0 {
        1 + ((a - 1) | (size_of::<u64>() - 1))
    } else {
        size_of::<u64>()
    }
}

/// `LABELID_MAX`.
const LABELID_MAX: u16 = 50000;

/// `struct rt_kmetrics`: these numbers are used by reliable protocols for determining
/// retransmission behavior and are included in the routing structure.
#[repr(C)]
pub struct RtKmetrics {
    /// `rmx_pksent`: packets sent using this route.
    pub rmx_pksent: Cell<u64>,
    /// `rmx_expire`: lifetime for route, e.g. redirect.
    pub rmx_expire: Cell<i64>,
    /// `rmx_locks`: kernel must leave these values.
    pub rmx_locks: Cell<u32>,
    /// \[a\] `rmx_mtu`: MTU for this path.
    pub rmx_mtu: AtomicU32,
}

impl RtKmetrics {
    /// `rt->rt_rmx = from->rt_rmx`: a structure assignment of the metrics.
    pub fn assign(&self, from: &RtKmetrics) {
        self.rmx_pksent.set(from.rmx_pksent.get());
        self.rmx_expire.set(from.rmx_expire.get());
        self.rmx_locks.set(from.rmx_locks.get());
        self.rmx_mtu
            .store(from.rmx_mtu.load(Ordering::Relaxed), Ordering::Relaxed);
    }
}

/// `struct rtentry`: a route. All-zero is a valid value (`PR_ZERO`; see the module's
/// deviations).
pub struct Rtentry {
    /// \[I\] `rt_dest`: destination (`rt_key(rt)`).
    pub rt_dest: Cell<*mut Sockaddr>,
    /// \[R\] `rt_next`: next mpath entry to our dst.
    pub rt_next: Cell<Option<&'static Rtentry>>,
    /// \[X\] `rt_gateway`: gateway address.
    pub rt_gateway: Cell<*mut Sockaddr>,
    /// \[N\] `rt_ifa`: interface addr to use.
    pub rt_ifa: Cell<Option<&'static Ifaddr>>,
    /// \[L\] `rt_llinfo`: pointer to link level info or an MPLS structure.
    pub rt_llinfo: Cell<*mut c_void>,
    /// \[X\] `rt_gwroute`: rtentry for `rt_gateway`.
    pub rt_gwroute: Cell<Option<&'static Rtentry>>,
    /// \[N\] `rt_parent`: if cloned, parent rtentry.
    pub rt_parent: Cell<Option<&'static Rtentry>>,
    /// \[T\] `rt_timer`: queue of timeouts for misc funcs.
    pub rt_timer: ListHead<RttimerLink>,
    /// `rt_mtx`: lock members of this struct.
    pub rt_mtx: Mutex,
    /// `rt_refcnt`: # held references.
    pub rt_refcnt: Refcnt,
    /// `rt_rmx`: metrics used by rx'ing protocols.
    pub rt_rmx: RtKmetrics,
    /// \[r\] `rt_cachecnt`: # gateway rtentry refs.
    pub rt_cachecnt: Cell<u32>,
    /// \[N\] `rt_ifidx`: interface to use.
    pub rt_ifidx: Cell<u32>,
    /// \[X\] `rt_flags`: up/down?, host/net.
    pub rt_flags: Cell<u32>,
    /// \[I\] `rt_plen`: prefix length.
    pub rt_plen: Cell<i32>,
    /// \[N\] `rt_labelid`: route label ID.
    pub rt_labelid: Cell<u16>,
    /// \[N\] `rt_priority`: routing priority to use.
    pub rt_priority: Cell<u8>,
}

// SAFETY: the members change under the locks their docs name, as in C; the reference count is
// atomic.
unsafe impl Sync for Rtentry {}

impl Rtentry {
    /// `rt_use` (`rt_rmx.rmx_pksent`).
    pub fn rt_use(&self) -> &Cell<u64> {
        &self.rt_rmx.rmx_pksent
    }

    /// `rt_expire` (`rt_rmx.rmx_expire`).
    pub fn rt_expire(&self) -> &Cell<i64> {
        &self.rt_rmx.rmx_expire
    }

    /// `rt_locks` (`rt_rmx.rmx_locks`).
    pub fn rt_locks(&self) -> &Cell<u32> {
        &self.rt_rmx.rmx_locks
    }

    /// `rt_mtu` (`rt_rmx.rmx_mtu`): MTU for this path, changed with `atomic_cas_uint`.
    pub fn rt_mtu(&self) -> &AtomicU32 {
        &self.rt_rmx.rmx_mtu
    }

    /// `rt->rt_ifa` where the C dereferences it: every route `rtrequest` made has an address.
    pub fn ifa(&self) -> &'static Ifaddr {
        match self.rt_ifa.get() {
            Some(ifa) => ifa,
            None => crate::kern::subr_prf::panic(format_args!("route {:p} without address", self)),
        }
    }
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

/// `struct rt_addrinfo`: the addresses and flags of a route request.
pub struct RtAddrinfo {
    /// `rti_addrs`: `RTA_*` bits of the addresses present (routing messages).
    pub rti_addrs: i32,
    /// `rti_info[]`: the addresses, by `RTAX_*`; NULL when absent.
    pub rti_info: [*const Sockaddr; RTAX_MAX],
    /// `rti_flags`: `RTF_*`.
    pub rti_flags: u32,
    /// `rti_ifa`: the interface address of the route.
    pub rti_ifa: Option<&'static Ifaddr>,
    /// `rti_rtm`: the routing message, if any.
    pub rti_rtm: *mut RtMsghdr,
    /// `rti_mpls`: MPLS operation.
    pub rti_mpls: u8,
}

impl RtAddrinfo {
    /// `memset(&info, 0, sizeof(info))`.
    pub const fn new() -> Self {
        Self {
            rti_addrs: 0,
            rti_info: [ptr::null(); RTAX_MAX],
            rti_flags: 0,
            rti_ifa: None,
            rti_rtm: ptr::null_mut(),
            rti_mpls: 0,
        }
    }
}

impl Default for RtAddrinfo {
    fn default() -> Self {
        Self::new()
    }
}

/// `struct route`'s destination union: `ro_dstsa`, `ro_dstsin`, `ro_dstsin6`.
#[repr(C)]
#[derive(Clone, Copy)]
pub union RouteDst {
    /// `ro_dstsa`.
    pub ro_dstsa: Sockaddr,
    /// `ro_dstsin`.
    pub ro_dstsin: SockaddrIn,
    /// `ro_dstsin6`: a `struct sockaddr_in6` (28 bytes) once `<netinet6/in6.h>` is ported.
    pub ro_dstsin6: [u32; 7],
}

/// `struct route`'s source union: `ro_srcin`, `ro_srcin6`.
#[repr(C)]
#[derive(Clone, Copy)]
pub union RouteSrc {
    /// `ro_srcin`.
    pub ro_srcin: InAddr,
    /// `ro_srcin6`: a `struct in6_addr` (16 bytes) once `<netinet6/in6.h>` is ported.
    pub ro_srcin6: [u32; 4],
}

/// `struct route`: a destination address and a reference to a routing entry. These are often
/// held by protocols in their control blocks, e.g. inpcb.
#[repr(C)]
pub struct Route {
    /// `ro_rt`: the cached route, referenced.
    pub ro_rt: Cell<Option<&'static Rtentry>>,
    /// `ro_generation`: `rtgeneration` when cached.
    pub ro_generation: Cell<u64>,
    /// `ro_tableid`: u_long because of alignment.
    pub ro_tableid: Cell<u64>,
    /// The destination union.
    pub ro_dst: Cell<RouteDst>,
    /// The source union.
    pub ro_src: Cell<RouteSrc>,
}

impl Route {
    /// An empty cache (`memset(ro, 0, sizeof(*ro))`).
    pub const fn new() -> Self {
        Self {
            ro_rt: Cell::new(None),
            ro_generation: Cell::new(0),
            ro_tableid: Cell::new(0),
            ro_dst: Cell::new(RouteDst { ro_dstsin6: [0; 7] }),
            ro_src: Cell::new(RouteSrc { ro_srcin6: [0; 4] }),
        }
    }

    /// `&ro->ro_dstsa`: the destination as a generic socket address, valid while `self` is.
    pub fn ro_dstsa(&self) -> *const Sockaddr {
        self.ro_dst.as_ptr().cast()
    }

    /// `ro->ro_dstsin`.
    pub fn ro_dstsin(&self) -> SockaddrIn {
        // SAFETY: every member of the union is plain data, valid for any bit pattern.
        unsafe { self.ro_dst.get().ro_dstsin }
    }

    /// `ro->ro_dstsa.sa_family`.
    pub fn ro_dst_family(&self) -> SaFamily {
        // SAFETY: as above.
        unsafe { self.ro_dst.get().ro_dstsa.sa_family }
    }

    /// `ro->ro_srcin`.
    pub fn ro_srcin(&self) -> InAddr {
        // SAFETY: as above.
        unsafe { self.ro_src.get().ro_srcin }
    }
}

impl Default for Route {
    fn default() -> Self {
        Self::new()
    }
}

/// `void (*rtq_func)(struct rtentry *, u_int)`: the callback of a timer queue.
pub type RtqFn = fn(&'static Rtentry, u32);

/// `struct rttimer_queue`: functions called for routes at specific times (used with the kind
/// permission of BSDI).
pub struct RttimerQueue {
    /// \[T\] `rtq_head`.
    pub rtq_head: TailqHead<RttimerNext>,
    /// \[T\] `rtq_link`.
    pub rtq_link: ListEntry<RttimerQueue>,
    /// \[I\] `rtq_func`: callback, NULL to delete dynamic host routes.
    pub rtq_func: Cell<Option<RtqFn>>,
    /// \[T\] `rtq_count`.
    pub rtq_count: Cell<u64>,
    /// \[T\] `rtq_timeout`: seconds.
    pub rtq_timeout: Cell<i32>,
}

// SAFETY: the members change under `rttimer_mtx` (`rtq_func` once, in `rt_timer_queue_init`).
unsafe impl Sync for RttimerQueue {}

impl RttimerQueue {
    /// A queue before `rt_timer_queue_init`.
    pub const fn new() -> Self {
        Self {
            rtq_head: TailqHead::new(),
            rtq_link: ListEntry::new(),
            rtq_func: Cell::new(None),
            rtq_count: Cell::new(0),
            rtq_timeout: Cell::new(0),
        }
    }
}

impl Default for RttimerQueue {
    fn default() -> Self {
        Self::new()
    }
}

/// `struct rttimer`: a timer of a route on a queue. All-zero is a valid value (`PR_ZERO`).
pub struct Rttimer {
    /// \[T\] `rtt_next`: entry on timer queue.
    rtt_next: TailqEntry<Rttimer>,
    /// \[T\] `rtt_link`: timers per rtentry.
    rtt_link: ListEntry<Rttimer>,
    /// \[I\] `rtt_timeout`: timeout for this entry.
    rtt_timeout: Timeout,
    /// \[I\] `rtt_queue`: back pointer to queue.
    rtt_queue: Cell<*const RttimerQueue>,
    /// \[T\] `rtt_rt`: back pointer to route.
    rtt_rt: Cell<Option<&'static Rtentry>>,
    /// \[I\] `rtt_expire`: rt expire time.
    rtt_expire: Cell<i64>,
    /// \[I\] `rtt_tableid`: rtable id of `rtt_rt`.
    rtt_tableid: Cell<u32>,
}

impl Rttimer {
    /// The timer's queue.
    fn queue(&self) -> &'static RttimerQueue {
        // SAFETY: `rt_timer_add` set the queue, a static of its protocol, before linking the
        // timer anywhere.
        unsafe { &*self.rtt_queue.get() }
    }
}

queue_adapter!(
    /// `TAILQ_ENTRY(rttimer) rtt_next`: a queue's timers.
    pub RttimerNext: Rttimer, rtt_next => TailqEntry<Rttimer>
);

queue_adapter!(
    /// `LIST_ENTRY(rttimer) rtt_link`: a route's timers.
    pub RttimerLink: Rttimer, rtt_link => ListEntry<Rttimer>
);

/// `struct rt_label`: a route label and its id.
struct RtLabel {
    /// \[L\] `rtl_entry`.
    rtl_entry: TailqEntry<RtLabel>,
    /// \[I\] `rtl_name`, NUL-terminated.
    rtl_name: Cell<[u8; RTLABEL_LEN]>,
    /// \[I\] `rtl_id`.
    rtl_id: Cell<u16>,
    /// \[L\] `rtl_ref`.
    rtl_ref: Cell<i32>,
}

queue_adapter!(
    /// `TAILQ_HEAD(rt_labels, rt_label)`.
    RtLabels: RtLabel, rtl_entry => TailqEntry<RtLabel>
);

/// `rt_labels`, made `Sync`: changed and read under `rtlabel_mtx`.
struct RtLabelHead(TailqHead<RtLabels>);

// SAFETY: see the type's doc.
unsafe impl Sync for RtLabelHead {}

/// `TAILQ_HEAD(, rttimer)` on the stack (`rt_timer_queue_flush`, `rt_timer_remove_all`).
type RttimerList = TailqHead<RttimerNext>;

/// `rt_hashjitter`: some jitter for the hash, to avoid synchronization between routers.
static RT_HASHJITTER: AtomicU32 = AtomicU32::new(0);

/// `rtcounters`: the routing statistics (see the module's deviations).
pub static RTCOUNTERS: [AtomicU64; RtstatCounters::RtsNcounters as usize] =
    [const { AtomicU64::new(0) }; RtstatCounters::RtsNcounters as usize];
/// \[a\] `rttrash`: routes not in table but not freed.
pub static RTTRASH: AtomicI32 = AtomicI32::new(0);
/// \[a\] `rtgeneration`: generation number, routes changed.
pub static RTGENERATION: AtomicU64 = AtomicU64::new(0);

/// `rtentry_pool`: pool for rtentry structures.
static RTENTRY_POOL: Pool = Pool::new();
/// `rttimer_pool`: pool for rttimer structures.
static RTTIMER_POOL: Pool = Pool::new();

/// \[L\] `rt_labels`.
static RT_LABELS: RtLabelHead = RtLabelHead(TailqHead::new());
/// `rtlabel_mtx`.
static RTLABEL_MTX: Mutex = Mutex::new(IPL_NET);

/// `rttimer_mtx`.
static RTTIMER_MTX: Mutex = Mutex::new(IPL_MPFLOOR);

/// `ROUTE_FILTER(m)`: the `ROUTE_MSGFILTER` bit of message type `m`.
pub const fn route_filter(m: u8) -> u32 {
    1 << m
}

/// `srtdnstosa(sdns)`: a `sockaddr_rtdns` seen as a generic `sockaddr`.
pub const fn srtdnstosa(sdns: *mut SockaddrRtdns) -> *mut Sockaddr {
    sdns.cast()
}

/// `rtstat_inc(c)`.
pub fn rtstat_inc(c: RtstatCounters) {
    RTCOUNTERS[c as usize].fetch_add(1, Ordering::Relaxed);
}

/// `membar_producer(); atomic_inc_long(&rtgeneration)`: routes changed.
fn rtgeneration_bump() {
    fence(Ordering::Release);
    RTGENERATION.fetch_add(1, Ordering::Relaxed);
}

/// `route_init`: the routing domain's initialisation.
pub fn route_init() {
    // rtcounters = counters_alloc(rts_ncounters): a static (the module's deviations).

    pool_init(
        &RTENTRY_POOL,
        size_of::<Rtentry>(),
        0,
        IPL_MPFLOOR,
        0,
        "rtentry",
        None,
    );

    while RT_HASHJITTER.load(Ordering::Relaxed) == 0 {
        RT_HASHJITTER.store(arc4random(), Ordering::Relaxed);
    }

    // BFD: bfdinit(), not configured.
}

/// `route_cache`: whether `ro` caches a valid route to `dst` (from `src`, if given) in table
/// `rtableid`; on a miss (`ESRCH`), resets the cache to that destination.
pub fn route_cache(
    ro: &Route,
    dst: &InAddr,
    src: Option<&InAddr>,
    rtableid: u32,
) -> Result<(), Errno> {
    let generation = RTGENERATION.load(Ordering::Relaxed);
    fence(Ordering::Acquire);

    if rtisvalid(ro.ro_rt.get())
        && ro.ro_generation.get() == generation
        && ro.ro_tableid.get() == u64::from(rtableid)
        && ro.ro_dst_family() == AF_INET
        && ro.ro_dstsin().sin_addr.s_addr == dst.s_addr
        && (src.is_none()
            || IPMULTIPATH.load(Ordering::Relaxed) == 0
            || ro
                .ro_rt
                .get()
                .is_some_and(|rt| rt.rt_flags.get() & RTF_MPATH == 0)
            || src.is_some_and(|s| {
                ro.ro_srcin().s_addr != INADDR_ANY && ro.ro_srcin().s_addr == s.s_addr
            }))
    {
        ipstat_inc(IpstatCounters::IpsRtcachehit);
        return Ok(());
    }

    ipstat_inc(IpstatCounters::IpsRtcachemiss);
    rtfree(ro.ro_rt.get());
    ro.ro_rt.set(None);
    ro.ro_generation.set(generation);
    ro.ro_tableid.set(u64::from(rtableid));

    ro.ro_dst.set(RouteDst {
        ro_dstsin: SockaddrIn {
            sin_len: size_of::<SockaddrIn>() as u8,
            sin_family: AF_INET,
            sin_addr: *dst,
            ..SockaddrIn::default()
        },
    });
    ro.ro_src.set(RouteSrc { ro_srcin6: [0; 4] });
    if let Some(src) = src {
        ro.ro_src.set(RouteSrc { ro_srcin: *src });
    }

    Err(Errno::ESRCH)
}

/// `route_mpath`: checks the cache for the route, else allocates a new one, potentially using
/// multipath to select the peer. Updates the cache and returns a valid route or `None`.
pub fn route_mpath(
    ro: &Route,
    dst: &InAddr,
    src: Option<&InAddr>,
    rtableid: u32,
) -> Option<&'static Rtentry> {
    if route_cache(ro, dst, src, rtableid).is_err() {
        let s = ro.ro_srcin().s_addr;
        let words = [s];
        let src = (s != INADDR_ANY).then_some(&words[..]);
        // SAFETY: `ro_dstsa` is the `sockaddr_in` `route_cache` just wrote.
        ro.ro_rt
            .set(unsafe { rtalloc_mpath(ro.ro_dstsa(), src, ro.ro_tableid.get() as u32) });
    }
    ro.ro_rt.get()
}

// INET6: route6_cache and route6_mpath, not configured.

/// `rtisvalid`: whether the (cached) route is still valid.
pub fn rtisvalid(rt: Option<&Rtentry>) -> bool {
    let Some(rt) = rt else {
        return false;
    };

    if rt.rt_flags.get() & RTF_UP == 0 {
        return false;
    }

    if rt.rt_flags.get() & RTF_GATEWAY != 0 {
        let Some(gw) = rt.rt_gwroute.get() else {
            return false;
        };
        kassert!(gw.rt_flags.get() & RTF_GATEWAY == 0);
        if gw.rt_flags.get() & RTF_UP == 0 {
            return false;
        }
    }

    true
}

/// `rt_match`: the lookup of `rtalloc(9)`, not to be used directly: the best matching route
/// for `dst`. With `RT_RESOLVE` a corresponding L2 entry is added to the routing table and
/// resolved (via ARP or NDP) if it does not exist.
///
/// # Safety
///
/// `dst` points at a readable socket address of its `sa_len` bytes.
unsafe fn rt_match(
    dst: *const Sockaddr,
    src: Option<&[u32]>,
    flags: i32,
    tableid: u32,
) -> Option<&'static Rtentry> {
    // SAFETY: the caller's contract.
    let Some(mut rt) = (unsafe { rtable_match(tableid, dst, src) }) else {
        rtstat_inc(RtstatCounters::RtsUnreach);
        return None;
    };

    if rt.rt_flags.get() & RTF_CLONING != 0 && flags & RT_RESOLVE != 0 {
        // SAFETY: the caller's contract.
        let _ = unsafe { rt_clone(&mut rt, dst, tableid) };
    }

    rt.rt_use().set(rt.rt_use().get() + 1);
    Some(rt)
}

/// `rt_clone`: resolves cloning route `*rtp` for `dst`; on success `*rtp` is the clone (the
/// cloning route's reference is dropped).
///
/// # Safety
///
/// As for [`rt_match`].
unsafe fn rt_clone(
    rtp: &mut &'static Rtentry,
    dst: *const Sockaddr,
    rtableid: u32,
) -> Result<(), Errno> {
    let mut info = RtAddrinfo::new();
    let mut rt = Some(*rtp);

    info.rti_info[RTAX_DST] = dst;

    // The priority of cloned route should be different to avoid conflict with /32 cloning
    // routes. It should also be higher to let the ARP layer find cloned routes instead of the
    // cloning one.
    // KERNEL_LOCK(): no kernel lock without MULTIPROCESSOR.
    let prio = rtp.rt_priority.get().wrapping_sub(1);
    // SAFETY: `dst` is the caller's; the other addresses are `rtrequest`'s own.
    let error = unsafe { rtrequest(RTM_RESOLVE, &mut info, prio, Some(&mut rt), rtableid) };
    match (error, rt) {
        (Err(e), _) => {
            // The gateway and label `rtrequest` put in `info` were its locals: the C's message
            // would carry dead stack addresses; it carries none here.
            info.rti_info[RTAX_GATEWAY] = ptr::null();
            info.rti_info[RTAX_LABEL] = ptr::null();
            // SAFETY: `dst` is the caller's, the only address left.
            unsafe { rtm_miss(RTM_MISS, &mut info, 0, RTP_NONE, 0, e as i32, rtableid) };
            Err(e)
        }
        (Ok(()), Some(nrt)) => {
            // Inform listeners of the new route
            rtm_send(nrt, RTM_ADD, 0, rtableid);
            rtfree(Some(*rtp));
            *rtp = nrt;
            Ok(())
        }
        (Ok(()), None) => Ok(()),
    }
}

/// `mix(a, b, c)`: the mixing step of `bridge_hash()` in `if_bridge.c`.
fn mix(a: &mut u32, b: &mut u32, c: &mut u32) {
    *a = a.wrapping_sub(*b).wrapping_sub(*c) ^ (*c >> 13);
    *b = b.wrapping_sub(*c).wrapping_sub(*a) ^ (*a << 8);
    *c = c.wrapping_sub(*a).wrapping_sub(*b) ^ (*b >> 13);
    *a = a.wrapping_sub(*b).wrapping_sub(*c) ^ (*c >> 12);
    *b = b.wrapping_sub(*c).wrapping_sub(*a) ^ (*a << 16);
    *c = c.wrapping_sub(*a).wrapping_sub(*b) ^ (*b >> 5);
    *a = a.wrapping_sub(*b).wrapping_sub(*c) ^ (*c >> 3);
    *b = b.wrapping_sub(*c).wrapping_sub(*a) ^ (*a << 10);
    *c = c.wrapping_sub(*a).wrapping_sub(*b) ^ (*b >> 15);
}

/// `rt_hash`: the 16-bit multipath hash of `dst` and `src` for `rt`, `None` (the C's -1)
/// when no hash applies.
///
/// # Safety
///
/// `dst` points at a readable socket address of its family's size.
pub unsafe fn rt_hash(rt: &Rtentry, dst: *const Sockaddr, src: Option<&[u32]>) -> Option<u32> {
    let src = src?;
    if !rtisvalid(Some(rt)) || rt.rt_flags.get() & RTF_MPATH == 0 {
        return None;
    }

    let (mut a, mut b) = (0x9e37_79b9u32, 0x9e37_79b9u32);
    let mut c = RT_HASHJITTER.load(Ordering::Relaxed);

    // SAFETY: the caller's contract.
    if unsafe { (*dst).sa_family } == AF_INET {
        if IPMULTIPATH.load(Ordering::Relaxed) == 0 {
            return None;
        }

        // SAFETY: an `AF_INET` address is a `sockaddr_in` (the caller's contract).
        let sin = unsafe { *satosin_const(dst) };
        a = a.wrapping_add(sin.sin_addr.s_addr);
        b = b.wrapping_add(src[0]);
        mix(&mut a, &mut b, &mut c);
    }
    // INET6: the four words of the IPv6 addresses for AF_INET6; not configured.

    Some(c & 0xffff)
}

/// `rtalloc_mpath`: allocates a route, potentially using multipath to select the peer.
///
/// # Safety
///
/// As for [`rt_match`].
pub unsafe fn rtalloc_mpath(
    dst: *const Sockaddr,
    src: Option<&[u32]>,
    rtableid: u32,
) -> Option<&'static Rtentry> {
    // SAFETY: the caller's contract.
    unsafe { rt_match(dst, src, RT_RESOLVE, rtableid) }
}

/// `rtalloc`: looks in the routing table for the best matching entry for `dst`, referenced.
///
/// # Safety
///
/// As for [`rt_match`].
pub unsafe fn rtalloc(dst: *const Sockaddr, flags: i32, rtableid: u32) -> Option<&'static Rtentry> {
    // SAFETY: the caller's contract.
    unsafe { rt_match(dst, None, flags, rtableid) }
}

/// `rt_setgwroute`: caches the route of a reachable next hop for gateway `gate` in the
/// gateway entry `rt`.
///
/// # Safety
///
/// `gate` points at a readable socket address of its `sa_len` bytes.
unsafe fn rt_setgwroute(
    rt: &'static Rtentry,
    gate: *const Sockaddr,
    rtableid: u32,
) -> Result<(), Errno> {
    let rdomain = rtable_l2(rtableid);

    net_assert_locked("rt_setgwroute");

    // If we cannot find a valid next hop bail.
    // SAFETY: the caller's contract.
    let Some(mut nhrt) = (unsafe { rt_match(gate, None, RT_RESOLVE, rdomain) }) else {
        return Err(Errno::ENOENT);
    };

    // Next hop entry must be on the same interface.
    if nhrt.rt_ifidx.get() != rt.rt_ifidx.get() {
        let mut sa_mask = SockaddrStorage::zeroed();

        if nhrt.rt_flags.get() & RTF_LLINFO == 0 || nhrt.rt_flags.get() & RTF_CLONED == 0 {
            rtfree(Some(nhrt));
            return Err(Errno::EHOSTUNREACH);
        }

        // We found a L2 entry, so we might have multiple RTF_CLONING routes for the same
        // subnet. Query the first route of the multipath chain and iterate until we find the
        // correct one.
        let Some(parent) = nhrt.rt_parent.get() else {
            rtfree(Some(nhrt));
            return Err(Errno::EHOSTUNREACH);
        };
        // SAFETY: the parent's key and the mask buffer are readable socket addresses.
        let mut prt = unsafe {
            rtable_lookup(
                rdomain,
                rt_key(parent),
                rt_plen2mask(parent, &mut sa_mask),
                ptr::null(),
                RTP_ANY,
            )
        };
        rtfree(Some(nhrt));

        while let Some(p) = prt {
            if p.rt_ifidx.get() == rt.rt_ifidx.get() {
                break;
            }
            prt = rtable_iterate(p);
        }

        // We found nothing or a non-cloning MPATH route.
        let Some(mut p) = prt.filter(|p| p.rt_flags.get() & RTF_CLONING != 0) else {
            rtfree(prt);
            return Err(Errno::EHOSTUNREACH);
        };

        // SAFETY: the caller's contract.
        if let Err(error) = unsafe { rt_clone(&mut p, gate, rdomain) } {
            rtfree(Some(p));
            return Err(error);
        }
        nhrt = p;
    }

    // Next hop must be reachable, this also prevents rtentry loops for example when
    // rt->rt_gwroute points to rt.
    if nhrt.rt_flags.get() & (RTF_CLONING | RTF_GATEWAY) != 0 {
        rtfree(Some(nhrt));
        return Err(Errno::ENETUNREACH);
    }

    // If the MTU of next hop is 0, this will reset the MTU of the route to run PMTUD again
    // from scratch.
    if rt.rt_locks().get() & RTV_MTU == 0 {
        let mtu = rt.rt_mtu().load(Ordering::Relaxed);
        let nhmtu = nhrt.rt_mtu().load(Ordering::Relaxed);
        if mtu > nhmtu {
            let _ = rt
                .rt_mtu()
                .compare_exchange(mtu, nhmtu, Ordering::Relaxed, Ordering::Relaxed);
        }
    }

    // commit
    rt_putgwroute(rt, Some(nhrt));

    Ok(())
}

/// `rt_putgwroute`: replaces the cached next hop of gateway entry `rt` with `nhrt` (whose
/// reference it takes over), releasing the old one.
pub fn rt_putgwroute(rt: &Rtentry, nhrt: Option<&'static Rtentry>) {
    net_assert_locked("rt_putgwroute");

    if rt.rt_flags.get() & RTF_GATEWAY == 0 {
        return;
    }

    // To avoid reference counting problems when writing link-layer addresses in an outgoing
    // packet, we ensure that the lifetime of a cached entry is greater than the bigger
    // lifetime of the gateway entries it is pointed by.
    if let Some(nh) = nhrt {
        mtx_enter(&nh.rt_mtx);
        nh.rt_flags.set(nh.rt_flags.get() | RTF_CACHED);
        nh.rt_cachecnt.set(nh.rt_cachecnt.get() + 1);
        mtx_leave(&nh.rt_mtx);
    }

    let onhrt = rt.rt_gwroute.get();
    rt.rt_gwroute.set(nhrt);

    if let Some(onh) = onhrt {
        mtx_enter(&onh.rt_mtx);
        kassert!(onh.rt_cachecnt.get() > 0);
        kassert!(onh.rt_flags.get() & RTF_CACHED != 0);
        onh.rt_cachecnt.set(onh.rt_cachecnt.get() - 1);
        if onh.rt_cachecnt.get() == 0 {
            onh.rt_flags.set(onh.rt_flags.get() & !RTF_CACHED);
        }
        mtx_leave(&onh.rt_mtx);

        rtfree(Some(onh));
    }
}

/// `rtref`: takes a reference on a route.
pub fn rtref(rt: &Rtentry) {
    refcnt_take(&rt.rt_refcnt);
}

/// `rtfree`: drops a reference on a route (NULL is ignored), freeing it with the last one.
pub fn rtfree(rt: Option<&Rtentry>) {
    let Some(rt) = rt else {
        return;
    };

    if !refcnt_rele(&rt.rt_refcnt) {
        return;
    }

    kassert!(rt.rt_flags.get() & RTF_UP == 0);
    kassert!(!rt_root(rt));
    RTTRASH.fetch_sub(1, Ordering::Relaxed);

    rt_timer_remove_all(rt);
    if let Some(ifa) = rt.rt_ifa.get() {
        ifafree(ifa);
    }
    rtlabel_unref(rt.rt_labelid.get());
    // MPLS: rt_mpls_clear(rt), not configured.
    rt_free_sa(rt.rt_gateway.get(), true);
    rt_free_sa(rt_key(rt), false);

    pool_put(&RTENTRY_POOL, NonNull::from(rt).cast());
}

/// Frees a route's gateway (`ROUNDUP(sa_len)` bytes) or key (`sa_len` bytes), if any.
fn rt_free_sa(sa: *mut Sockaddr, gateway: bool) {
    let Some(sa) = NonNull::new(sa) else {
        return;
    };
    // SAFETY: a route's key and gateway are socket addresses `rt_copysa`/`rt_setgate`
    // allocated; they are readable until freed here.
    let len = usize::from(unsafe { sa.as_ref() }.sa_len);
    let size = if gateway { roundup_long(len) } else { len };
    free(sa.cast(), M_RTABLE, size);
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

/// `equal(a1, a2)`: the two socket addresses are the same `a1->sa_len` bytes.
///
/// # Safety
///
/// Both point at readable socket addresses at least `a1->sa_len` bytes long.
unsafe fn equal(a1: *const Sockaddr, a2: *const Sockaddr) -> bool {
    // SAFETY: the caller's contract.
    unsafe {
        let len = usize::from((*a1).sa_len);
        usize::from((*a2).sa_len) == len
            && slice::from_raw_parts(a1.cast::<u8>(), len)
                == slice::from_raw_parts(a2.cast::<u8>(), len)
    }
}

/// `rtredirect`: forces a routing table entry to the specified destination to go through the
/// given gateway. Normally called as a result of a routing redirect message from the network
/// layer. The new or changed route goes to `rtp` (referenced) when given.
///
/// # Safety
///
/// `dst`, `gateway` and `src` point at readable socket addresses of their `sa_len` bytes.
pub unsafe fn rtredirect(
    dst: *const Sockaddr,
    gateway: *const Sockaddr,
    src: *const Sockaddr,
    rtp: Option<&mut Option<&'static Rtentry>>,
    rdomain: u32,
) {
    let mut error: Result<(), Errno> = Ok(());
    let mut stat = RtstatCounters::RtsNcounters;
    let mut info: RtAddrinfo;
    let mut ifidx = 0;
    let mut flags = RTF_GATEWAY | RTF_HOST;
    let mut prio = RTP_NONE;

    net_assert_locked("rtredirect");

    'out: {
        // verify the gateway is directly reachable
        // SAFETY: the caller's contract.
        let rt = unsafe { rtalloc(gateway, 0, rdomain) };
        let Some(r) = rt.filter(|r| rtisvalid(Some(r)) && r.rt_flags.get() & RTF_GATEWAY == 0)
        else {
            rtfree(rt);
            error = Err(Errno::ENETUNREACH);
            break 'out;
        };
        ifidx = r.rt_ifidx.get();
        let ifa = r.rt_ifa.get();
        rtfree(Some(r));

        // SAFETY: the caller's contract.
        let mut rt = unsafe { rtable_lookup(rdomain, dst, ptr::null(), ptr::null(), RTP_ANY) };
        // If the redirect isn't from our current router for this dst, it's either old or
        // wrong. If it redirects us to ourselves, we have a routing loop, perhaps as a result
        // of an interface going down recently.
        // SAFETY: the caller's contract for `src` and `gateway`; a route's gateway is readable.
        unsafe {
            if let Some(r) = rt
                && (!equal(src, r.rt_gateway.get()) || !opt_ptr_eq(r.rt_ifa.get(), ifa))
            {
                error = Err(Errno::EINVAL);
            } else if ifa_ifwithaddr(gateway, rdomain).is_some()
                || ((*gateway).sa_family == AF_INET
                    && in_broadcast((*satosin_const(gateway)).sin_addr, rdomain))
            {
                error = Err(Errno::EHOSTUNREACH);
            }
        }
        'done: {
            if error.is_err() {
                break 'done;
            }
            // Create a new entry if we just got back a wildcard entry or the lookup failed.
            // This is necessary for hosts which use routing redirects generated by smart
            // gateways to dynamically build the routing tables.
            let create = match rt {
                None => true,
                // Don't listen to the redirect if it's for a route to an interface.
                Some(r) if r.rt_flags.get() & RTF_GATEWAY != 0 => r.rt_flags.get() & RTF_HOST == 0,
                Some(_) => {
                    error = Err(Errno::EHOSTUNREACH);
                    break 'done;
                }
            };
            if create {
                // Changing from route to net => route to host. Create new route, rather than
                // smashing route to net.
                rtfree(rt);
                flags |= RTF_DYNAMIC;
                info = RtAddrinfo::new();
                info.rti_info[RTAX_DST] = dst;
                info.rti_info[RTAX_GATEWAY] = gateway;
                info.rti_ifa = ifa;
                info.rti_flags = flags;
                rt = None;
                // SAFETY: the caller's addresses.
                error =
                    unsafe { rtrequest(RTM_ADD, &mut info, RTP_DEFAULT, Some(&mut rt), rdomain) };
                if error.is_ok()
                    && let Some(r) = rt
                {
                    flags = r.rt_flags.get();
                    prio = r.rt_priority.get();
                }
                stat = RtstatCounters::RtsDynamic;
            } else if let Some(r) = rt {
                // Smash the current notion of the gateway to this destination. Should check
                // about netmask!!!
                r.rt_flags.set(r.rt_flags.get() | RTF_MODIFIED);
                flags |= RTF_MODIFIED;
                prio = r.rt_priority.get();
                stat = RtstatCounters::RtsNewgateway;
                // SAFETY: the caller's contract.
                let _ = unsafe { rt_setgate(r, gateway, rdomain) };
            }
        }
        // done:
        if let Some(r) = rt {
            match rtp {
                Some(p) if error.is_ok() => *p = Some(r),
                _ => rtfree(Some(r)),
            }
        }
    }
    // out:
    if error.is_err() {
        rtstat_inc(RtstatCounters::RtsBadredirect);
    } else if stat != RtstatCounters::RtsNcounters {
        rtstat_inc(stat);
    }
    info = RtAddrinfo::new();
    info.rti_info[RTAX_DST] = dst;
    info.rti_info[RTAX_GATEWAY] = gateway;
    info.rti_info[RTAX_AUTHOR] = src;
    let errno = error.err().map_or(0, |e| e as i32);
    // SAFETY: the caller's addresses.
    unsafe { rtm_miss(RTM_REDIRECT, &mut info, flags, prio, ifidx, errno, rdomain) };
}

/// Whether two optional references are the same object (`a == b` on the C's pointers).
fn opt_ptr_eq<T>(a: Option<&T>, b: Option<&T>) -> bool {
    match (a, b) {
        (Some(a), Some(b)) => ptr::eq(a, b),
        (None, None) => true,
        _ => false,
    }
}

/// `rtdeletemsg`: deletes a route and generates a message.
pub fn rtdeletemsg(rt: &'static Rtentry, ifp: &Ifnet, tableid: u32) -> Result<(), Errno> {
    let mut info = RtAddrinfo::new();
    let mut sa_rl = SockaddrRtlabel::default();
    let mut sa_mask = SockaddrStorage::zeroed();

    kassert!(rt.rt_ifidx.get() == ifp.if_index.get());

    // Request the new route so that the entry is not actually deleted. That will allow the
    // information being reported to be accurate (and consistent with route_output()).
    info.rti_info[RTAX_DST] = rt_key(rt);
    info.rti_info[RTAX_GATEWAY] = rt.rt_gateway.get();
    if rt.rt_flags.get() & RTF_HOST == 0 {
        info.rti_info[RTAX_NETMASK] = rt_plen2mask(rt, &mut sa_mask);
    }
    info.rti_info[RTAX_LABEL] = rtlabel_id2sa(rt.rt_labelid.get(), &mut sa_rl);
    info.rti_flags = rt.rt_flags.get();
    info.rti_info[RTAX_IFP] = sdltosa(ifp.if_sadl.get());
    info.rti_info[RTAX_IFA] = rt.ifa().ifa_addr.get();
    // KERNEL_LOCK(): no kernel lock without MULTIPROCESSOR.
    let mut nrt = Some(rt);
    // SAFETY: every address in `info` is the route's, the interface's or a local buffer, all
    // alive for the call.
    let error = unsafe {
        rtrequest_delete(
            &mut info,
            rt.rt_priority.get(),
            ifp,
            Some(&mut nrt),
            tableid,
        )
    };
    let rt = nrt.unwrap_or(rt);
    let errno = error.err().map_or(0, |e| e as i32);
    let flags = info.rti_flags;
    // SAFETY: as above.
    unsafe {
        rtm_miss(
            RTM_DELETE,
            &mut info,
            flags,
            rt.rt_priority.get(),
            rt.rt_ifidx.get(),
            errno,
            tableid,
        )
    };
    if error.is_ok() {
        rtfree(Some(rt));
    }
    error
}

/// `rtequal`: the same route, or the same key and prefix length.
fn rtequal(a: &Rtentry, b: &Rtentry) -> bool {
    if ptr::eq(a, b) {
        return true;
    }

    // SAFETY: a route's key is a socket address of `sa_len` readable bytes.
    unsafe {
        let len = usize::from((*rt_key(a)).sa_len);
        slice::from_raw_parts(rt_key(a).cast::<u8>(), len)
            == slice::from_raw_parts(rt_key(b).cast::<u8>(), len)
            && rt_plen(a) == rt_plen(b)
    }
}

/// `rtflushclone1`: `EEXIST` for a clone of `cloningrt` that may go.
fn rtflushclone1(rt: &Rtentry, cloningrt: &Rtentry) -> Result<(), Errno> {
    if rt.rt_flags.get() & RTF_CLONED == 0 {
        return Ok(());
    }

    // Cached route must stay alive as long as their parent are alive.
    if rt.rt_flags.get() & RTF_CACHED != 0 && !opt_ptr_eq(rt.rt_parent.get(), Some(cloningrt)) {
        return Ok(());
    }

    if !rt.rt_parent.get().is_some_and(|p| rtequal(p, cloningrt)) {
        return Ok(());
    }
    // This happens when an interface with a RTF_CLONING route is being detached. In this case
    // it's safe to bail because all the routes are being purged by rt_ifa_purge().
    let Some(ifp) = if_get(rt.rt_ifidx.get()) else {
        return Ok(());
    };

    if_put(ifp);
    Err(Errno::EEXIST)
}

/// `rtflushclone`: deletes the clones of cloning route `parent`.
fn rtflushclone(parent: &'static Rtentry, rtableid: u32) -> Result<(), Errno> {
    #[cfg(feature = "diagnostic")]
    if parent.rt_flags.get() & RTF_CLONING == 0 {
        crate::kern::subr_prf::panic(format_args!(
            "rtflushclone: called with a non-cloning route"
        ));
    }

    loop {
        let mut rt = None;
        // SAFETY: the key of a route in a table is readable.
        let family = unsafe { (*rt_key(parent)).sa_family };
        let mut error = rtable_walk(rtableid, family, Some(&mut rt), |r, _| {
            rtflushclone1(r, parent)
        });
        if let Some(r) = rt
            && error == Err(Errno::EEXIST)
        {
            match if_get(r.rt_ifidx.get()) {
                None => error = Err(Errno::EAGAIN),
                Some(ifp) => {
                    error = rtdeletemsg(r, ifp, rtableid);
                    if error.is_ok() {
                        error = Err(Errno::EAGAIN);
                    }
                    if_put(ifp);
                }
            }
        }
        rtfree(rt);
        if error != Err(Errno::EAGAIN) {
            return error;
        }
    }
}

/// `rtrequest_delete`: removes the route `info` describes from interface `ifp`; the removed
/// route goes to `ret_nrt` (referenced) when given.
///
/// # Safety
///
/// Every non-NULL `rti_info[]` address is readable for its `sa_len` bytes.
pub unsafe fn rtrequest_delete(
    info: &mut RtAddrinfo,
    prio: u8,
    ifp: &Ifnet,
    ret_nrt: Option<&mut Option<&'static Rtentry>>,
    tableid: u32,
) -> Result<(), Errno> {
    net_assert_locked("rtrequest_delete");

    if !rtable_exists(tableid) {
        return Err(Errno::EAFNOSUPPORT);
    }
    // SAFETY: the caller's contract.
    let Some(rt) = (unsafe {
        rtable_lookup(
            tableid,
            info.rti_info[RTAX_DST],
            info.rti_info[RTAX_NETMASK],
            info.rti_info[RTAX_GATEWAY],
            prio,
        )
    }) else {
        return Err(Errno::ESRCH);
    };

    // Make sure that's the route the caller want to delete.
    if ifp.if_index.get() != rt.rt_ifidx.get() {
        rtfree(Some(rt));
        return Err(Errno::ESRCH);
    }

    // BFD: bfdclear(rt) for RTF_BFD routes, not configured.

    // SAFETY: the caller's contract.
    if unsafe {
        rtable_delete(
            tableid,
            info.rti_info[RTAX_DST],
            info.rti_info[RTAX_NETMASK],
            rt,
        )
    }
    .is_err()
    {
        rtfree(Some(rt));
        return Err(Errno::ESRCH);
    }

    // Release next hop cache before flushing cloned entries.
    rt_putgwroute(rt, None);

    // Clean up any cloned children.
    if rt.rt_flags.get() & RTF_CLONING != 0 {
        let _ = rtflushclone(rt, tableid);
    }

    rtfree(rt.rt_parent.get());
    rt.rt_parent.set(None);

    rt.rt_flags.set(rt.rt_flags.get() & !RTF_UP);

    kassert!(ifp.if_index.get() == rt.rt_ifidx.get());
    if let Some(rtrequest) = ifp.if_rtrequest.get() {
        // SAFETY: an attached interface lives until `if_detach`, which purges its routes
        // first; its `if_rtrequest` takes it as `&'static`.
        rtrequest(
            unsafe { &*ptr::from_ref(ifp) },
            i32::from(RTM_DELETE),
            Some(rt),
        );
    }

    RTTRASH.fetch_add(1, Ordering::Relaxed);

    match ret_nrt {
        Some(p) => *p = Some(rt),
        None => rtfree(Some(rt)),
    }

    rtgeneration_bump();

    Ok(())
}

/// `rtrequest`: adds a route (`RTM_ADD`) or a clone of a cloning route (`RTM_RESOLVE`, which
/// reads the cloning route from `ret_nrt`). The new route goes to `ret_nrt` (referenced) when
/// given.
///
/// # Safety
///
/// Every non-NULL `rti_info[]` address is readable for its `sa_len` bytes.
pub unsafe fn rtrequest(
    req: u8,
    info: &mut RtAddrinfo,
    prio: u8,
    ret_nrt: Option<&mut Option<&'static Rtentry>>,
    tableid: u32,
) -> Result<(), Errno> {
    let mut sa_rl2 = SockaddrRtlabel::default();
    let mut sa_dl = SockaddrDl {
        sdl_len: size_of::<SockaddrDl>() as u8,
        sdl_family: AF_LINK,
        ..SockaddrDl::default()
    };
    let mut prio = prio;

    net_assert_locked("rtrequest");

    if !rtable_exists(tableid) {
        return Err(Errno::EAFNOSUPPORT);
    }
    if info.rti_flags & RTF_HOST != 0 {
        info.rti_info[RTAX_NETMASK] = ptr::null();
    }
    let cloned_from = match req {
        RTM_DELETE => return Err(Errno::EINVAL),
        RTM_RESOLVE => {
            let Some(rt) = ret_nrt.as_deref().copied().flatten() else {
                return Err(Errno::EINVAL);
            };
            if rt.rt_flags.get() & RTF_CLONING == 0 {
                return Err(Errno::EINVAL);
            }
            info.rti_ifa = rt.rt_ifa.get();
            info.rti_flags = rt.rt_flags.get() | (RTF_CLONED | RTF_HOST);
            info.rti_flags &= !(RTF_CLONING | RTF_CONNECTED | RTF_STATIC);
            info.rti_info[RTAX_GATEWAY] = sdltosa(&mut sa_dl);
            info.rti_info[RTAX_LABEL] = rtlabel_id2sa(rt.rt_labelid.get(), &mut sa_rl2);
            Some(rt)
        }
        RTM_ADD => None,
        _ => return Ok(()),
    };

    // RTM_ADD (and RTM_RESOLVE falling through):
    let Some(ifa) = info.rti_ifa else {
        return Err(Errno::EINVAL);
    };
    kassert!(ifa.ifa_ifp.get().is_some());
    let Some(ifp) = ifa.ifa_ifp.get() else {
        return Err(Errno::EINVAL);
    };
    if prio == 0 {
        prio = ifp.if_priority.get() + RTP_STATIC;
    }

    // SAFETY: the caller's contract.
    let ndst = unsafe { rt_copysa(info.rti_info[RTAX_DST], info.rti_info[RTAX_NETMASK]) }?;
    // SAFETY: `rt_copysa` allocated `sa_len` bytes.
    let ndst_len = usize::from(unsafe { (*ndst).sa_len });

    let Some(mem) = pool_get(&RTENTRY_POOL, PR_NOWAIT | PR_ZERO) else {
        free_sa(ndst, ndst_len);
        return Err(Errno::ENOBUFS);
    };
    // SAFETY: a zeroed pool item of `size_of::<Rtentry>()` bytes; all-zero is a valid
    // `Rtentry`. It lives until its last `rtfree`.
    let rt: &'static Rtentry = unsafe { &*mem.as_ptr().cast::<Rtentry>() };

    mtx_init_flags(&rt.rt_mtx, IPL_SOFTNET, Some("rtentry"), 0);
    refcnt_init_trace(&rt.rt_refcnt, DT_REFCNT_IDX_RTENTRY);
    rt.rt_flags.set(info.rti_flags | RTF_UP);
    rt.rt_priority.set(prio); // init routing priority
    rt.rt_timer.init();

    // Check the link state if the table supports it.
    // SAFETY: `rt_copysa` wrote the family.
    let family = unsafe { (*ndst).sa_family };
    if rtable_mpath_capable(tableid, family)
        && rt.rt_flags.get() & RTF_LOCAL == 0
        && (!link_state_is_up(ifp.if_link_state.get()) || ifp.if_flags.get() & IFF_UP == 0)
    {
        rt.rt_flags.set(rt.rt_flags.get() & !RTF_UP);
        rt.rt_priority.set(rt.rt_priority.get() | RTP_DOWN);
    }

    if !info.rti_info[RTAX_LABEL].is_null() {
        // SAFETY: a label address is a `sockaddr_rtlabel` (the caller's contract).
        let sa_rl = unsafe { &*info.rti_info[RTAX_LABEL].cast::<SockaddrRtlabel>() };
        rt.rt_labelid.set(rtlabel_name2id(&sa_rl.sr_label));
    }

    // MPLS: rt_mpls_set/rt_mpls_clear for RTF_MPLS routes, not configured.

    rt.rt_ifa.set(Some(ifaref(ifa)));
    rt.rt_ifidx.set(ifp.if_index.get());
    // Copy metrics and a back pointer from the cloned route's parent.
    if rt.rt_flags.get() & RTF_CLONED != 0
        && let Some(parent) = cloned_from
    {
        rtref(parent);
        rt.rt_parent.set(Some(parent));
        rt.rt_rmx.assign(&parent.rt_rmx);
    }

    // We must set rt->rt_gateway before adding ``rt'' to the routing table because the radix
    // MPATH code use it to (re)order routes.
    // SAFETY: the caller's contract.
    if let Err(error) = unsafe { rt_setgate(rt, info.rti_info[RTAX_GATEWAY], tableid) } {
        rtrequest_unwind(rt, ifa, ndst, ndst_len);
        return Err(error);
    }

    // SAFETY: the caller's contract; `ndst` lives as long as the route.
    let mut error = unsafe {
        rtable_insert(
            tableid,
            ndst,
            info.rti_info[RTAX_NETMASK],
            info.rti_info[RTAX_GATEWAY],
            rt.rt_priority.get(),
            rt,
        )
    };
    if error.is_err()
        // SAFETY: `ndst` is readable.
        && let Some(crt) = unsafe { rtable_match(tableid, ndst, None) }
    {
        // overwrite cloned route
        if crt.rt_flags.get() & RTF_CLONED != 0 && crt.rt_flags.get() & RTF_CACHED == 0 {
            let cifp = if_get(crt.rt_ifidx.get());
            kassert!(cifp.is_some());
            if let Some(cifp) = cifp {
                let _ = rtdeletemsg(crt, cifp, tableid);
            }
            if_put(cifp);

            // SAFETY: as above.
            error = unsafe {
                rtable_insert(
                    tableid,
                    ndst,
                    info.rti_info[RTAX_NETMASK],
                    info.rti_info[RTAX_GATEWAY],
                    rt.rt_priority.get(),
                    rt,
                )
            };
        }
        rtfree(Some(crt));
    }
    if error.is_err() {
        rtrequest_unwind(rt, ifa, ndst, ndst_len);
        return Err(Errno::EEXIST);
    }
    if let Some(rtrequest) = ifp.if_rtrequest.get() {
        rtrequest(ifp, i32::from(req), Some(rt));
    }

    // SAFETY: the caller's contract.
    unsafe { if_group_routechange(info.rti_info[RTAX_DST], info.rti_info[RTAX_NETMASK]) };

    match ret_nrt {
        Some(p) => *p = Some(rt),
        None => rtfree(Some(rt)),
    }

    rtgeneration_bump();

    Ok(())
}

/// The error path of `rtrequest` after the route was allocated: undoes what was set and
/// frees it.
fn rtrequest_unwind(rt: &'static Rtentry, ifa: &Ifaddr, ndst: *mut Sockaddr, ndst_len: usize) {
    ifafree(ifa);
    rtfree(rt.rt_parent.get());
    rt_putgwroute(rt, None);
    rt_free_sa(rt.rt_gateway.get(), true);
    free_sa(ndst, ndst_len);
    pool_put(&RTENTRY_POOL, NonNull::from(rt).cast());
}

/// Frees a socket address of `len` bytes allocated with `M_RTABLE`.
fn free_sa(sa: *mut Sockaddr, len: usize) {
    if let Some(sa) = NonNull::new(sa) {
        free(sa.cast(), M_RTABLE, len);
    }
}

/// `rt_setgate`: sets the gateway of `rt` to a copy of `gate` (and, for a gateway route, its
/// next hop cache).
///
/// # Safety
///
/// `gate` points at a readable socket address of its `sa_len` bytes.
pub unsafe fn rt_setgate(
    rt: &'static Rtentry,
    gate: *const Sockaddr,
    rtableid: u32,
) -> Result<(), Errno> {
    kassert!(!gate.is_null());
    // SAFETY: the caller's contract.
    let gate_len = usize::from(unsafe { (*gate).sa_len });
    let glen = roundup_long(gate_len);

    if ptr::eq(rt.rt_gateway.get(), gate) {
        // nop
        return Ok(());
    }

    let Some(sa) = malloc(glen, M_RTABLE, M_NOWAIT | M_ZERO) else {
        return Err(Errno::ENOBUFS);
    };
    let sa = sa.cast::<Sockaddr>().as_ptr();
    // SAFETY: `glen >= gate_len` fresh bytes; the caller's contract for `gate`.
    unsafe { ptr::copy_nonoverlapping(gate.cast::<u8>(), sa.cast::<u8>(), gate_len) };

    // KERNEL_LOCK(): see [X] in route.h; no kernel lock without MULTIPROCESSOR.
    let osa = rt.rt_gateway.get();
    rt.rt_gateway.set(sa);

    let mut error = Ok(());
    if rt.rt_flags.get() & RTF_GATEWAY != 0 {
        // SAFETY: the caller's contract.
        error = unsafe { rt_setgwroute(rt, gate, rtableid) };
    }

    rt_free_sa(osa, true);

    error
}

/// `rt_getll`: the route holding the next hop link-layer address of `rt` (`None` for a
/// gateway route whose next hop is not cached).
pub fn rt_getll(rt: &'static Rtentry) -> Option<&'static Rtentry> {
    if rt.rt_flags.get() & RTF_GATEWAY != 0 {
        // We may return NULL here.
        return rt.rt_gwroute.get();
    }

    Some(rt)
}

/// `rt_maskedcopy`: copies `src` to `dst` under `netmask` (length and family included).
///
/// # Safety
///
/// `src` and `netmask` are readable socket addresses of their `sa_len` bytes; `dst` has room
/// for `src`'s `sa_len` bytes.
pub unsafe fn rt_maskedcopy(src: *const Sockaddr, dst: *mut Sockaddr, netmask: *const Sockaddr) {
    // SAFETY: the caller's contract.
    unsafe {
        let srclen = usize::from((*src).sa_len);
        let masklen = usize::from((*netmask).sa_len);
        let cp1 = slice::from_raw_parts(src.cast::<u8>(), srclen);
        let cp3 = slice::from_raw_parts(netmask.cast::<u8>(), masklen);
        let cp2 = slice::from_raw_parts_mut(dst.cast::<u8>(), srclen);

        // copies sa_len & sa_family
        cp2[0] = cp1[0];
        cp2[1] = cp1[1];
        let lim = masklen.min(srclen);
        for i in 2..srclen {
            cp2[i] = if i < lim { cp1[i] & cp3[i] } else { 0 };
        }
    }
}

/// `rt_copysa`: a new socket address for the routing table from `src` and `mask`: the
/// family's size, the key masked to the prefix length.
///
/// # Safety
///
/// `src` points at a readable socket address of its `sa_len` bytes, `mask` too unless NULL.
unsafe fn rt_copysa(src: *const Sockaddr, mask: *const Sockaddr) -> Result<*mut Sockaddr, Errno> {
    const MASKARRAY: [u8; 8] = [0x0, 0x80, 0xc0, 0xe0, 0xf0, 0xf8, 0xfc, 0xfe];

    // SAFETY: the caller's contract.
    let (family, len) = unsafe { ((*src).sa_family, (*src).sa_len) };
    let Some(dp) = DOMAINS
        .iter()
        .find(|dp| dp.dom_rtoffset != 0 && dp.dom_family == i32::from(family))
    else {
        return Err(Errno::EAFNOSUPPORT);
    };

    if u32::from(len) < dp.dom_sasize {
        return Err(Errno::EINVAL);
    }

    // SAFETY: the caller's contract.
    let plen = unsafe { rtable_satoplen(family, mask) }? as usize;

    let Some(ndst) = malloc(dp.dom_sasize as usize, M_RTABLE, M_NOWAIT | M_ZERO) else {
        return Err(Errno::ENOBUFS);
    };
    let ndst = ndst.cast::<Sockaddr>().as_ptr();

    let off = dp.dom_rtoffset as usize;
    // SAFETY: `ndst` is `dom_sasize` fresh bytes and `src` at least as long; the key's
    // `plen` bits fit in both past the offset.
    unsafe {
        (*ndst).sa_family = family;
        (*ndst).sa_len = dp.dom_sasize as u8;

        let csrc = src.cast::<u8>().add(off);
        let cdst = ndst.cast::<u8>().add(off);

        ptr::copy_nonoverlapping(csrc, cdst, plen / 8);
        if !plen.is_multiple_of(8) {
            *cdst.add(plen / 8) = *csrc.add(plen / 8) & MASKARRAY[plen % 8];
        }
    }

    Ok(ndst)
}

/// `rt_ifa_add`: adds the route of interface address `ifa` to `dst` with `flags`.
///
/// # Safety
///
/// `dst` points at a readable socket address of its `sa_len` bytes; the address's own
/// socket addresses are readable (`ifa_add`'s contract).
pub unsafe fn rt_ifa_add(
    ifa: &'static Ifaddr,
    flags: u32,
    dst: *mut Sockaddr,
    rdomain: u32,
) -> Result<(), Errno> {
    let Some(ifp) = ifa.ifa_ifp.get() else {
        return Err(Errno::EINVAL);
    };
    let mut sa_rl = SockaddrRtlabel::default();
    let mut info = RtAddrinfo::new();
    let mut prio = ifp.if_priority.get() + RTP_STATIC;

    kassert!(rdomain == rtable_l2(rdomain));

    info.rti_ifa = Some(ifa);
    info.rti_flags = flags;
    info.rti_info[RTAX_DST] = dst;
    if flags & RTF_LLINFO != 0 {
        info.rti_info[RTAX_GATEWAY] = sdltosa(ifp.if_sadl.get());
    } else {
        info.rti_info[RTAX_GATEWAY] = ifa.ifa_addr.get();
    }
    info.rti_info[RTAX_LABEL] = rtlabel_id2sa(ifp.if_rtlabelid.get(), &mut sa_rl);

    // MPLS: MPLS_OP_POP for RTF_MPLS, not configured.

    if flags & RTF_HOST == 0 {
        info.rti_info[RTAX_NETMASK] = ifa.ifa_netmask.get();
    }

    if flags & (RTF_LOCAL | RTF_BROADCAST) != 0 {
        prio = RTP_LOCAL;
    }

    if flags & RTF_CONNECTED != 0 {
        prio = ifp.if_priority.get() + RTP_CONNECTED;
    }

    let mut rt = None;
    // SAFETY: the caller's contract and the local label.
    let error = unsafe { rtrequest(RTM_ADD, &mut info, prio, Some(&mut rt), rdomain) };
    if error.is_ok()
        && let Some(r) = rt
    {
        // A local route is created for every address configured on an interface, so use this
        // information to notify userland that a new address has been added.
        if flags & RTF_LOCAL != 0 {
            rtm_addr(RTM_NEWADDR, ifa);
        }
        rtm_send(r, RTM_ADD, 0, rdomain);
        rtfree(Some(r));
    }
    error
}

/// `rt_ifa_del`: removes the route of interface address `ifa` to `dst` with `flags`.
///
/// # Safety
///
/// As for [`rt_ifa_add`].
pub unsafe fn rt_ifa_del(
    ifa: &'static Ifaddr,
    flags: u32,
    dst: *mut Sockaddr,
    rdomain: u32,
) -> Result<(), Errno> {
    let Some(ifp) = ifa.ifa_ifp.get() else {
        return Err(Errno::EINVAL);
    };
    let mut m = None;
    let mut dst = dst;
    let mut info = RtAddrinfo::new();
    let mut sa_rl = SockaddrRtlabel::default();
    let mut prio = ifp.if_priority.get() + RTP_STATIC;

    kassert!(rdomain == rtable_l2(rdomain));

    if flags & RTF_HOST == 0 && !ifa.ifa_netmask.get().is_null() {
        let Some(mm) = m_get(M_DONTWAIT, MT_SONAME) else {
            return Err(Errno::ENOBUFS);
        };
        m = Some(mm);
        let deldst = mtod::<Sockaddr>(mm);
        // SAFETY: an mbuf holds MLEN bytes, more than any socket address `dst` can be; the
        // caller's contract for the rest.
        unsafe { rt_maskedcopy(dst, deldst, ifa.ifa_netmask.get()) };
        dst = deldst;
    }

    info.rti_ifa = Some(ifa);
    info.rti_flags = flags;
    info.rti_info[RTAX_DST] = dst;
    if flags & RTF_LLINFO == 0 {
        info.rti_info[RTAX_GATEWAY] = ifa.ifa_addr.get();
    }
    info.rti_info[RTAX_LABEL] = rtlabel_id2sa(ifp.if_rtlabelid.get(), &mut sa_rl);

    if flags & RTF_HOST == 0 {
        info.rti_info[RTAX_NETMASK] = ifa.ifa_netmask.get();
    }

    if flags & (RTF_LOCAL | RTF_BROADCAST) != 0 {
        prio = RTP_LOCAL;
    }

    if flags & RTF_CONNECTED != 0 {
        prio = ifp.if_priority.get() + RTP_CONNECTED;
    }

    // SAFETY: the address's own socket address.
    unsafe { rtable_clearsource(rdomain, ifa.ifa_addr.get()) };
    // KERNEL_LOCK(): no kernel lock without MULTIPROCESSOR.
    let mut rt = None;
    // SAFETY: the caller's contract, the mbuf copy and the local label.
    let error = unsafe { rtrequest_delete(&mut info, prio, ifp, Some(&mut rt), rdomain) };
    if error.is_ok()
        && let Some(r) = rt
    {
        rtm_send(r, RTM_DELETE, 0, rdomain);
        if flags & RTF_LOCAL != 0 {
            rtm_addr(RTM_DELADDR, ifa);
        }
        rtfree(Some(r));
    }
    m_free(m);

    error
}

/// The local route flags of interface address `ifa`, or `None` for the "any" address (which
/// gets no local route because it is the key of the default routes).
fn rt_ifa_localflags(ifa: &Ifaddr, ifp: &Ifnet) -> Option<u32> {
    let mut flags = RTF_HOST | RTF_LOCAL;

    // If the configured address correspond to the magical "any" address do not add a local
    // route entry because that might corrupt the routing tree which uses this value for the
    // default routes.
    let addr = ifa.ifa_addr.get();
    // SAFETY: an interface address's socket address is readable (`ifa_add`'s contract).
    if unsafe { (*addr).sa_family } == AF_INET
        // SAFETY: an `AF_INET` address is a `sockaddr_in`.
        && unsafe { (*satosin_const(addr)).sin_addr.s_addr } == INADDR_ANY
    {
        return None;
    }
    // INET6: the same for in6addr_any; not configured.

    if ifp.if_flags.get() & (IFF_LOOPBACK | IFF_POINTOPOINT) == 0 {
        flags |= RTF_LLINFO;
    }
    Some(flags)
}

/// `rt_ifa_addlocal`: adds `ifa`'s address as a local rtentry.
pub fn rt_ifa_addlocal(ifa: &'static Ifaddr) -> Result<(), Errno> {
    let Some(ifp) = ifa.ifa_ifp.get() else {
        return Err(Errno::EINVAL);
    };
    let Some(flags) = rt_ifa_localflags(ifa, ifp) else {
        return Ok(());
    };
    let mut error = Ok(());

    // If there is no local entry, allocate one.
    // SAFETY: the address's own socket address.
    let rt = unsafe { rtalloc(ifa.ifa_addr.get(), 0, ifp.if_rdomain.get()) };
    if rt.is_none_or(|r| r.rt_flags.get() & flags != flags) {
        // SAFETY: the address's own socket address.
        error = unsafe {
            rt_ifa_add(
                ifa,
                flags | RTF_MPATH,
                ifa.ifa_addr.get(),
                ifp.if_rdomain.get(),
            )
        };
    }
    rtfree(rt);

    error
}

/// `rt_ifa_dellocal`: removes the local rtentry of `ifa`'s address if it exists.
pub fn rt_ifa_dellocal(ifa: &'static Ifaddr) -> Result<(), Errno> {
    let Some(ifp) = ifa.ifa_ifp.get() else {
        return Err(Errno::EINVAL);
    };
    // We do not add local routes for such address, so do not bother removing them.
    let Some(flags) = rt_ifa_localflags(ifa, ifp) else {
        return Ok(());
    };
    let mut error = Ok(());

    // Before deleting, check if a corresponding local host route surely exists. With this
    // check, we can avoid to delete an interface direct route whose destination is same as the
    // address being removed. This can happen when removing a subnet-router anycast address on
    // an interface attached to a shared medium.
    // SAFETY: the address's own socket address.
    let rt = unsafe { rtalloc(ifa.ifa_addr.get(), 0, ifp.if_rdomain.get()) };
    if rt.is_some_and(|r| r.rt_flags.get() & flags == flags) {
        // SAFETY: the address's own socket address.
        error = unsafe { rt_ifa_del(ifa, flags, ifa.ifa_addr.get(), ifp.if_rdomain.get()) };
    }
    rtfree(rt);

    error
}

/// `rt_ifa_purge`: removes all routes attached to `ifa`.
pub fn rt_ifa_purge(ifa: &'static Ifaddr) {
    let Some(ifp) = ifa.ifa_ifp.get() else {
        crate::kern::subr_prf::panic(format_args!("rt_ifa_purge: address without interface"));
    };
    // SAFETY: an interface address's socket address is readable.
    let af = unsafe { (*ifa.ifa_addr.get()).sa_family };

    for rtableid in 0..RTMAP_LIMIT.load(Ordering::Relaxed) {
        // skip rtables that are not in the rdomain of the ifp
        if rtable_l2(rtableid) != ifp.if_rdomain.get() {
            continue;
        }

        let mut error;
        loop {
            let mut rt = None;
            error = rtable_walk(rtableid, af, Some(&mut rt), |r, _| {
                rt_ifa_purge_walker(r, ifa)
            });
            if let Some(r) = rt
                && error == Err(Errno::EEXIST)
            {
                error = rtdeletemsg(r, ifp, rtableid);
                if error.is_ok() {
                    error = Err(Errno::EAGAIN);
                }
            }
            rtfree(rt);
            if error != Err(Errno::EAGAIN) {
                break;
            }
        }

        if error == Err(Errno::EAFNOSUPPORT) {
            error = Ok(());
        }

        if error.is_err() {
            break;
        }
    }
}

/// `rt_ifa_purge_walker`: `EEXIST` for a route of `ifa`.
fn rt_ifa_purge_walker(rt: &Rtentry, ifa: &Ifaddr) -> Result<(), Errno> {
    if opt_ptr_eq(rt.rt_ifa.get(), Some(ifa)) {
        return Err(Errno::EEXIST);
    }

    Ok(())
}

// Route timer routines. These routines allow functions to be called for various routes at
// any time. This is useful in supporting path MTU discovery and redirect route deletion. This
// is similar to some BSDI internal functions, but it provides for multiple queues for
// efficiency's sake...

/// `RTTIMER_CALLOUT(r)`: the timer's queue function, or deleting a dynamic host route.
fn rttimer_callout(r: &Rttimer) {
    let Some(rt) = r.rtt_rt.get() else {
        return;
    };
    match r.queue().rtq_func.get() {
        Some(func) => func(rt, r.rtt_tableid.get()),
        None => {
            let ifp = if_get(rt.rt_ifidx.get());
            if let Some(ifp) = ifp
                && rt.rt_flags.get() & (RTF_DYNAMIC | RTF_HOST) == (RTF_DYNAMIC | RTF_HOST)
            {
                let _ = rtdeletemsg(rt, ifp, r.rtt_tableid.get());
            }
            if_put(ifp);
        }
    }
}

/// `rt_timer_init`.
pub fn rt_timer_init() {
    pool_init(
        &RTTIMER_POOL,
        size_of::<Rttimer>(),
        0,
        IPL_MPFLOOR,
        0,
        "rttmr",
        None,
    );
    mtx_init(&RTTIMER_MTX, IPL_MPFLOOR);
}

/// `rt_timer_queue_init`: a queue whose timers fire after `timeout` seconds and call `func`.
pub fn rt_timer_queue_init(rtq: &RttimerQueue, timeout: i32, func: Option<RtqFn>) {
    rtq.rtq_timeout.set(timeout);
    rtq.rtq_count.set(0);
    rtq.rtq_func.set(func);
    rtq.rtq_head.init();
}

/// `rt_timer_queue_change`.
pub fn rt_timer_queue_change(rtq: &RttimerQueue, timeout: i32) {
    mtx_enter(&RTTIMER_MTX);
    rtq.rtq_timeout.set(timeout);
    mtx_leave(&RTTIMER_MTX);
}

/// `rt_timer_queue_flush`: fires and frees every timer of the queue.
pub fn rt_timer_queue_flush(rtq: &RttimerQueue) {
    let rttlist = RttimerList::new();

    net_assert_locked("rt_timer_queue_flush");

    mtx_enter(&RTTIMER_MTX);
    while let Some(r) = rtq.rtq_head.first() {
        // SAFETY: `r` is on the queue and on its route's list, under `rttimer_mtx`; it moves
        // to the local list, which lives until the end of this function.
        unsafe {
            ListHead::<RttimerLink>::remove(r);
            rtq.rtq_head.remove(r);
            rttlist.insert_tail(r);
        }
        kassert!(rtq.rtq_count.get() > 0);
        rtq.rtq_count.set(rtq.rtq_count.get() - 1);
    }
    mtx_leave(&RTTIMER_MTX);

    while let Some(r) = rttlist.first() {
        // SAFETY: `r` is on the local list.
        unsafe { rttlist.remove(r) };
        rttimer_callout(r);
        pool_put(&RTTIMER_POOL, NonNull::from(r).cast());
    }
}

/// `rt_timer_queue_count`.
pub fn rt_timer_queue_count(rtq: &RttimerQueue) -> u64 {
    rtq.rtq_count.get()
}

/// `rt_timer_unlink`: takes `r` off its route; also off its queue, returned, unless its
/// timeout already fired (then `rt_timer_timer` does the cleanup).
fn rt_timer_unlink(r: &Rttimer) -> Option<&Rttimer> {
    crate::sys::mutex::mutex_assert_locked(&RTTIMER_MTX, "rt_timer_unlink");

    // SAFETY: `r` is on its route's list, under `rttimer_mtx`.
    unsafe { ListHead::<RttimerLink>::remove(r) };
    r.rtt_rt.set(None);

    if !timeout_del(&r.rtt_timeout) {
        // timeout fired, so rt_timer_timer will do the cleanup
        return None;
    }

    let q = r.queue();
    // SAFETY: `r` is on its queue, under `rttimer_mtx`.
    unsafe { q.rtq_head.remove(r) };
    kassert!(q.rtq_count.get() > 0);
    q.rtq_count.set(q.rtq_count.get() - 1);
    Some(r)
}

/// `rt_timer_remove_all`: removes and frees every timer of `rt`.
pub fn rt_timer_remove_all(rt: &Rtentry) {
    let rttlist = RttimerList::new();

    mtx_enter(&RTTIMER_MTX);
    while let Some(r) = rt.rt_timer.first() {
        if let Some(r) = rt_timer_unlink(r) {
            // SAFETY: `r` left every list; the local list lives until the end of this
            // function.
            unsafe { rttlist.insert_tail(r) };
        }
    }
    mtx_leave(&RTTIMER_MTX);

    while let Some(r) = rttlist.first() {
        // SAFETY: `r` is on the local list.
        unsafe { rttlist.remove(r) };
        pool_put(&RTTIMER_POOL, NonNull::from(r).cast());
    }
}

/// `rt_timer_get_expire`: the earliest expiry of `rt`'s timers, 0 if none.
pub fn rt_timer_get_expire(rt: &Rtentry) -> i64 {
    let mut expire = 0;

    mtx_enter(&RTTIMER_MTX);
    for r in rt.rt_timer.iter() {
        if expire == 0 || expire > r.rtt_expire.get() {
            expire = r.rtt_expire.get();
        }
    }
    mtx_leave(&RTTIMER_MTX);

    expire
}

/// `rt_timer_add`: arms a timer of `queue` for `rt`, replacing one it already has there.
pub fn rt_timer_add(
    rt: &'static Rtentry,
    queue: &'static RttimerQueue,
    rtableid: u32,
) -> Result<(), Errno> {
    let Some(mem) = pool_get(&RTTIMER_POOL, PR_NOWAIT | PR_ZERO) else {
        return Err(Errno::ENOBUFS);
    };
    // SAFETY: a zeroed pool item of `size_of::<Rttimer>()` bytes; all-zero is a valid
    // `Rttimer`. It lives until `rt_timer_timer`, `rt_timer_remove_all` or the flush frees it.
    let rnew: &'static Rttimer = unsafe { &*mem.as_ptr().cast::<Rttimer>() };

    rnew.rtt_rt.set(Some(rt));
    rnew.rtt_queue.set(queue);
    rnew.rtt_tableid.set(rtableid);
    rnew.rtt_expire
        .set(getuptime() + i64::from(queue.rtq_timeout.get()));
    timeout_set_proc(
        &rnew.rtt_timeout,
        rt_timer_timer,
        ptr::from_ref(rnew).cast_mut().cast(),
    );

    let mut old = None;
    mtx_enter(&RTTIMER_MTX);
    // If there's already a timer with this action, destroy it before we add a new one.
    for r in rt.rt_timer.iter() {
        if ptr::eq(r.rtt_queue.get(), queue) {
            old = rt_timer_unlink(r);
            break; // only one per list, so we can quit...
        }
    }

    // SAFETY: `rnew` is on no list yet; the route and the queue outlive it (the route's
    // `rtfree` and the queue's flush take it off).
    unsafe {
        rt.rt_timer.insert_head(rnew);
        queue.rtq_head.insert_tail(rnew);
    }
    let _ = timeout_add_sec(&rnew.rtt_timeout, queue.rtq_timeout.get());
    queue.rtq_count.set(queue.rtq_count.get() + 1);
    mtx_leave(&RTTIMER_MTX);

    if let Some(r) = old {
        pool_put(&RTTIMER_POOL, NonNull::from(r).cast());
    }

    Ok(())
}

/// `rt_timer_timer`: a timer fired; `arg` is the timer.
pub fn rt_timer_timer(arg: *mut c_void) {
    // SAFETY: `rt_timer_add` armed the timeout with the timer as its argument; a fired timer
    // is freed only here.
    let r: &Rttimer = unsafe { &*arg.cast::<Rttimer>() };
    let rtq = r.queue();

    crate::sys::systm::net_lock();
    mtx_enter(&RTTIMER_MTX);

    // SAFETY: under `rttimer_mtx`: `r` is on its route's list while `rtt_rt` is set, and on
    // its queue until here.
    unsafe {
        if r.rtt_rt.get().is_some() {
            ListHead::<RttimerLink>::remove(r);
        }
        rtq.rtq_head.remove(r);
    }
    kassert!(rtq.rtq_count.get() > 0);
    rtq.rtq_count.set(rtq.rtq_count.get() - 1);

    mtx_leave(&RTTIMER_MTX);

    if r.rtt_rt.get().is_some() {
        rttimer_callout(r);
    }
    crate::sys::systm::net_unlock();

    pool_put(&RTTIMER_POOL, NonNull::from(r).cast());
}

// MPLS: rt_mpls_set and rt_mpls_clear, not configured.

/// The bytes of a NUL-terminated name, without the NUL.
fn cstr(name: &[u8]) -> &[u8] {
    let end = name.iter().position(|&c| c == 0).unwrap_or(name.len());
    &name[..end]
}

/// `rtlabel_name2id`: the id of route label `name` (registered and referenced), 0 for the
/// empty name or when no id is free.
pub fn rtlabel_name2id(name: &[u8]) -> u16 {
    let name = cstr(name);
    let mut new_id: u16 = 1;
    let mut id = 0;

    if name.is_empty() {
        return 0;
    }

    mtx_enter(&RTLABEL_MTX);
    'out: {
        for label in RT_LABELS.0.iter() {
            if cstr(&label.rtl_name.get()) == name {
                label.rtl_ref.set(label.rtl_ref.get() + 1);
                id = label.rtl_id.get();
                break 'out;
            }
        }

        // to avoid fragmentation, we do a linear search from the beginning and take the first
        // free slot we find. if there is none or the list is empty, append a new entry at the
        // end.
        let mut p = None;
        for q in RT_LABELS.0.iter() {
            if q.rtl_id.get() != new_id {
                p = Some(q);
                break;
            }
            new_id = q.rtl_id.get() + 1;
        }
        if new_id > LABELID_MAX {
            break 'out;
        }

        let Some(mem) = malloc(size_of::<RtLabel>(), M_RTABLE, M_NOWAIT | M_ZERO) else {
            break 'out;
        };
        // SAFETY: a zeroed block of `size_of::<RtLabel>()` bytes; all-zero is a valid
        // `RtLabel` (a link and cells). It lives until `rtlabel_unref` frees it.
        let label: &RtLabel = unsafe { &*mem.as_ptr().cast::<RtLabel>() };
        let mut buf = [0u8; RTLABEL_LEN];
        let n = name.len().min(RTLABEL_LEN - 1);
        buf[..n].copy_from_slice(&name[..n]);
        label.rtl_name.set(buf);
        label.rtl_id.set(new_id);
        label.rtl_ref.set(label.rtl_ref.get() + 1);

        // SAFETY: `label` is on no list; the list is changed under `rtlabel_mtx`.
        unsafe {
            match p {
                // insert new entry before p
                Some(p) => TailqHead::<RtLabels>::insert_before(p, label),
                // either list empty or no free slot in between
                None => RT_LABELS.0.insert_tail(label),
            }
        }

        id = label.rtl_id.get();
    }
    mtx_leave(&RTLABEL_MTX);

    id
}

/// `rtlabel_id2name_locked`: the name of label `id`, NUL-terminated.
fn rtlabel_id2name_locked(id: u16) -> Option<[u8; RTLABEL_LEN]> {
    crate::sys::mutex::mutex_assert_locked(&RTLABEL_MTX, "rtlabel_id2name_locked");

    RT_LABELS
        .0
        .iter()
        .find(|label| label.rtl_id.get() == id)
        .map(|label| label.rtl_name.get())
}

/// `rtlabel_id2name`: copies the name of label `id` into `rtlabelbuf` (NUL-terminated,
/// truncated as `strlcpy` does) and returns the name; `None` for 0 or an unknown id.
pub fn rtlabel_id2name(id: u16, rtlabelbuf: &mut [u8]) -> Option<&[u8]> {
    if id == 0 {
        return None;
    }

    mtx_enter(&RTLABEL_MTX);
    let label = rtlabel_id2name_locked(id);
    if let Some(name) = &label {
        libkern::strlcpy(rtlabelbuf, name);
    }
    mtx_leave(&RTLABEL_MTX);

    label?;

    Some(cstr(rtlabelbuf))
}

/// `rtlabel_id2sa`: label `labelid` as a socket address in `sa_rl`, NULL for 0 or an unknown
/// id.
pub fn rtlabel_id2sa(labelid: u16, sa_rl: &mut SockaddrRtlabel) -> *const Sockaddr {
    if labelid == 0 {
        return ptr::null();
    }

    mtx_enter(&RTLABEL_MTX);
    let label = rtlabel_id2name_locked(labelid);
    if let Some(name) = &label {
        *sa_rl = SockaddrRtlabel::default();
        sa_rl.sr_len = size_of::<SockaddrRtlabel>() as u8;
        sa_rl.sr_family = AF_UNSPEC;
        libkern::strlcpy(&mut sa_rl.sr_label, name);
    }
    mtx_leave(&RTLABEL_MTX);

    if label.is_none() {
        return ptr::null();
    }

    ptr::from_ref(sa_rl).cast()
}

/// `rtlabel_unref`: drops a reference on label `id`, freeing it with the last.
pub fn rtlabel_unref(id: u16) {
    if id == 0 {
        return;
    }

    mtx_enter(&RTLABEL_MTX);
    for p in RT_LABELS.0.iter() {
        if id == p.rtl_id.get() {
            p.rtl_ref.set(p.rtl_ref.get() - 1);
            if p.rtl_ref.get() == 0 {
                // SAFETY: `p` is on the list, changed under `rtlabel_mtx`.
                unsafe { RT_LABELS.0.remove(p) };
                free(NonNull::from(p).cast(), M_RTABLE, size_of::<RtLabel>());
            }
            break;
        }
    }
    mtx_leave(&RTLABEL_MTX);
}

/// `rt_if_track`: brings the routes of `ifp` up or down with its link state, deleting the
/// cloned and dynamic routes of a down interface.
pub fn rt_if_track(ifp: &Ifnet) -> Result<(), Errno> {
    let mut error = Ok(());

    'tables: for rtableid in 0..RTMAP_LIMIT.load(Ordering::Relaxed) {
        // skip rtables that are not in the rdomain of the ifp
        if rtable_l2(rtableid) != ifp.if_rdomain.get() {
            continue;
        }
        for i in 1..=AF_MAX {
            if !rtable_mpath_capable(rtableid, i) {
                continue;
            }

            loop {
                let mut rt = None;
                error = rtable_walk(rtableid, i, Some(&mut rt), |r, id| {
                    rt_if_linkstate_change(r, ifp, id)
                });
                if let Some(r) = rt
                    && error == Err(Errno::EEXIST)
                {
                    error = rtdeletemsg(r, ifp, rtableid);
                    if error.is_ok() {
                        error = Err(Errno::EAGAIN);
                    }
                }
                rtfree(rt);
                if error != Err(Errno::EAGAIN) {
                    break;
                }
            }

            if error == Err(Errno::EAFNOSUPPORT) {
                error = Ok(());
            }

            if error.is_err() {
                break 'tables;
            }
        }
    }

    error
}

/// `rt_if_linkstate_change`: brings `rt` up or down with `ifp`'s link state; `EEXIST` for a
/// cloned or dynamic route of a down interface, which the caller deletes.
pub fn rt_if_linkstate_change(rt: &'static Rtentry, ifp: &Ifnet, id: u32) -> Result<(), Errno> {
    let mut sa_mask = SockaddrStorage::zeroed();

    if rt.rt_ifidx.get() != ifp.if_index.get() {
        return Ok(());
    }

    // Local routes are always usable.
    if rt.rt_flags.get() & RTF_LOCAL != 0 {
        rt.rt_flags.set(rt.rt_flags.get() | RTF_UP);
        return Ok(());
    }

    let error = if link_state_is_up(ifp.if_link_state.get()) && ifp.if_flags.get() & IFF_UP != 0 {
        if rt.rt_flags.get() & RTF_UP != 0 {
            return Ok(());
        }

        // bring route up
        rt.rt_flags.set(rt.rt_flags.get() | RTF_UP);
        // SAFETY: the key of a route in a table is readable.
        unsafe {
            rtable_mpath_reprio(
                id,
                rt_key(rt),
                rt_plen(rt),
                rt.rt_priority.get() & RTP_MASK,
                rt,
            )
        }
    } else {
        // Remove redirected and cloned routes (mainly ARP) from down interfaces so we have a
        // chance to get new routes from a better source.
        if rt.rt_flags.get() & (RTF_CLONED | RTF_DYNAMIC) != 0
            && rt.rt_flags.get() & (RTF_CACHED | RTF_BFD) == 0
        {
            return Err(Errno::EEXIST);
        }

        if rt.rt_flags.get() & RTF_UP == 0 {
            return Ok(());
        }

        // take route down
        rt.rt_flags.set(rt.rt_flags.get() & !RTF_UP);
        // SAFETY: as above.
        unsafe {
            rtable_mpath_reprio(
                id,
                rt_key(rt),
                rt_plen(rt),
                rt.rt_priority.get() | RTP_DOWN,
                rt,
            )
        }
    };
    // SAFETY: the key and the mask buffer are readable.
    unsafe { if_group_routechange(rt_key(rt), rt_plen2mask(rt, &mut sa_mask)) };

    rtgeneration_bump();

    error
}

/// `rt_plentosa`: the netmask of prefix length `plen` in family `af`, written into `sa_mask`;
/// NULL for -1 or a family without masks.
pub fn rt_plentosa(af: SaFamily, plen: i32, sa_mask: &mut SockaddrStorage) -> *const Sockaddr {
    kassert!(plen >= 0 || plen == -1);

    if plen == -1 {
        return ptr::null();
    }

    *sa_mask = SockaddrStorage::zeroed();

    match af {
        AF_INET => {
            let mut sin = SockaddrIn {
                sin_family: AF_INET,
                sin_len: size_of::<SockaddrIn>() as u8,
                ..SockaddrIn::default()
            };
            in_prefixlen2mask(&mut sin.sin_addr, plen);
            let p = ptr::from_mut(sa_mask).cast::<SockaddrIn>();
            // SAFETY: a `sockaddr_storage` is larger than a `sockaddr_in` and aligned for it.
            unsafe { p.write(sin) };
        }
        // INET6: in6_prefixlen2mask into a sockaddr_in6; not configured.
        _ => return ptr::null(),
    }

    ptr::from_ref(sa_mask).cast()
}

/// `rt_plen2mask`: the netmask of `rt`, written into `sa_mask`.
pub fn rt_plen2mask(rt: &Rtentry, sa_mask: &mut SockaddrStorage) -> *const Sockaddr {
    // SAFETY: the key of a route is readable.
    let af = unsafe { (*rt_key(rt)).sa_family };
    rt_plentosa(af, rt_plen(rt), sa_mask)
}

/// `db_print_sa`: a socket address as its bytes.
///
/// # Safety
///
/// `sa` is NULL or points at a readable socket address of its `sa_len` bytes.
pub unsafe fn db_print_sa(sa: *const Sockaddr) {
    if sa.is_null() {
        db_printf!("[NULL]");
        return;
    }

    // SAFETY: the caller's contract.
    let bytes = unsafe { slice::from_raw_parts(sa.cast::<u8>(), usize::from((*sa).sa_len)) };
    db_printf!("[");
    for (i, b) in bytes.iter().enumerate() {
        db_printf!("{}", b);
        if i + 1 < bytes.len() {
            db_printf!(",");
        }
    }
    db_printf!("]\n");
}

/// `db_print_ifa`: an interface address's addresses and counters.
pub fn db_print_ifa(ifa: Option<&Ifaddr>) {
    let Some(ifa) = ifa else {
        return;
    };
    // SAFETY: an interface address's socket addresses are NULL or readable.
    unsafe {
        db_printf!("  ifa_addr=");
        db_print_sa(ifa.ifa_addr.get());
        db_printf!("  ifa_dsta=");
        db_print_sa(ifa.ifa_dstaddr.get());
        db_printf!("  ifa_mask=");
        db_print_sa(ifa.ifa_netmask.get());
    }
    db_printf!(
        "  flags=0x{:x}, refcnt={}, metric={}\n",
        ifa.ifa_flags.get(),
        ifa.ifa_refcnt.r_refs.load(Ordering::Relaxed),
        ifa.ifa_metric.get()
    );
}

/// `db_show_rtentry`: prints a route; a `rtable_walk` callback.
pub fn db_show_rtentry(rt: &Rtentry, _id: u32) -> Result<(), Errno> {
    db_printf!("rtentry={:p}", rt);

    db_printf!(
        " flags=0x{:x} refcnt={} use={} expire={}\n",
        rt.rt_flags.get(),
        rt.rt_refcnt.r_refs.load(Ordering::Relaxed),
        rt.rt_use().get(),
        rt.rt_expire().get()
    );

    // SAFETY: a route's key and gateway are readable socket addresses.
    unsafe {
        db_printf!(" key=");
        db_print_sa(rt_key(rt));
        db_printf!(" plen={}", rt_plen(rt));
        db_printf!(" gw=");
        db_print_sa(rt.rt_gateway.get());
    }
    db_printf!(" ifidx={} ", rt.rt_ifidx.get());
    db_printf!(
        " ifa={:p}\n",
        rt.rt_ifa.get().map_or(ptr::null(), ptr::from_ref)
    );
    db_print_ifa(rt.rt_ifa.get());

    db_printf!(
        " gwroute={:p} llinfo={:p} priority={}\n",
        rt.rt_gwroute.get().map_or(ptr::null(), ptr::from_ref),
        rt.rt_llinfo.get(),
        rt.rt_priority.get()
    );
    Ok(())
}

/// `db_show_rtable`: prints every route of `af` in table `rtableid`.
pub fn db_show_rtable(af: SaFamily, rtableid: u32) -> Result<(), Errno> {
    db_printf!("Route tree for af {}, rtableid {}\n", af, rtableid);
    let _ = rtable_walk(rtableid, af, None, db_show_rtentry);
    Ok(())
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
