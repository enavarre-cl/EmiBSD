/*	$OpenBSD: in6_var.h,v 1.85 2026/03/22 23:14:00 bluhm Exp $	*/
/*	$KAME: in6_var.h,v 1.55 2001/02/16 12:49:45 itojun Exp $	*/
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
 * Copyright (c) 1985, 1986, 1993
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
 *	@(#)in_var.h	8.1 (Berkeley) 6/10/93
 */
/* </LICENSES> */

/* <CODE> */
//! IPv6 interface addresses, interface statistics, the address `ioctl` requests and the
//! multicast records: `<netinet6/in6_var.h>`.
//!
//! Upstream: sys/netinet6/in6_var.h @ 3ce1f3f79392
//!
//! One `struct in6_ifaddr` is allocated for each interface with an IPv6 address. The `struct
//! ifaddr` it embeds first holds the protocol-independent part; its `ifa_addr`,
//! `ifa_dstaddr` and `ifa_netmask` point at the `sockaddr_in6`s the `in6_ifaddr` reserves.
//! Locks: \[I\] immutable after creation, \[m\] the parent interface's `if_maddrlock`.
//!
//! pltime/vltime are just for future reference (required to implement the 2 hour rule for
//! hosts); they are never modified by `nd6_timeout` or anywhere else. userland -> kernel:
//! accept pltime/vltime; kernel -> userland: throw up everything; in kernel: modify
//! preferred/expire only.
//!
//! ## Deviations
//! - `struct in6_ifaddr` and `struct in6_multi` are `#[repr(C)]` with the generic structure
//!   first and `Cell` members (the C changes them through shared pointers under the net
//!   lock); the all-zero value is valid, as `malloc(M_ZERO)` needs. `ifatoia6` (`in6.h`) is
//!   in `netinet6/in6.rs`; `ifmatoin6m` is checked as `ifmatoinm` is: it takes the address
//!   family as the type tag and panics on another.
//! - The `ia_ifp`, `ia_flags`, `in6m_refcnt`, `in6m_ifidx` and `in6m_addr` shorthands are
//!   methods; the `IA6_*` and `IFA_*IN6` macros are functions returning copies of the
//!   addresses (the C's pointers into the structure are only ever read through).
//! - The `ifr_ifru` and `ifra_ifrau` unions are `#[repr(C)]` unions with accessors, as in
//!   `netinet/in_var.rs`.
//! - `SIOCGETSGCNT_IN6` and `SIOCGETMIFCNT_IN6` encode the sizes of `struct sioc_sg_req6`
//!   (80 bytes) and `struct sioc_mif_req6` (40 bytes) of `<netinet6/ip6_mroute.h>`, which is
//!   not ported (`MROUTING` is not configured).
//! - `IN6_ARE_SCOPE_CMP` and `IN6_ARE_SCOPE_EQUAL` are `const fn`s on the `int` scopes.
//! - The prototypes are `netinet6/in6.rs`'s functions (`in6.c`).

use core::cell::Cell;
use core::mem::size_of;

use crate::kern::subr_prf::panic;
use crate::net::if_::IFNAMSIZ;
use crate::net::if_var::{Ifaddr, Ifmaddr, Ifnet};
use crate::netinet6::in6::{In6Addr, SockaddrIn6};
use crate::netinet6::nd6::{In6Nbrinfo, In6Ndireq};
use crate::queue_adapter;
use crate::sys::ioccom::{_ioc, _iow, _iowr, IOC_INOUT};
use crate::sys::queue::{ListEntry, ListHead, TailqEntry};
use crate::sys::refcnt::Refcnt;
use crate::sys::socket::{AF_INET6, Sockaddr};
use crate::sys::types::Time;

/// `struct in6_addrlifetime`: the lifetimes of an address.
#[repr(C)]
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct In6Addrlifetime {
    /// Valid lifetime expiration time.
    pub ia6t_expire: Time,
    /// Preferred lifetime expiration time.
    pub ia6t_preferred: Time,
    /// Valid lifetime.
    pub ia6t_vltime: u32,
    /// Prefix lifetime.
    pub ia6t_pltime: u32,
}

/// `struct in6_ifaddr`: interface address, Internet version 6.
#[repr(C)]
pub struct In6Ifaddr {
    /// `ia_ifa`: protocol-independent info.
    pub ia_ifa: Ifaddr,
    /// `ia_addr`: interface address.
    pub ia_addr: Cell<SockaddrIn6>,
    /// `ia_gwaddr`: router we learned address from.
    pub ia_gwaddr: Cell<SockaddrIn6>,
    /// `ia_dstaddr`: space for destination addr.
    pub ia_dstaddr: Cell<SockaddrIn6>,
    /// `ia_prefixmask`: prefix mask.
    pub ia_prefixmask: Cell<SockaddrIn6>,
    /// `ia_list`: list of IP6 addresses.
    pub ia_list: TailqEntry<In6Ifaddr>,
    /// `ia6_flags`: `IN6_IFF_*`.
    pub ia6_flags: Cell<i32>,
    /// `ia6_lifetime`.
    pub ia6_lifetime: Cell<In6Addrlifetime>,
    /// `ia6_updatetime`.
    pub ia6_updatetime: Cell<Time>,
    /// `ia6_memberships`: multicast addresses joined from the kernel.
    pub ia6_memberships: ListHead<In6MultiMshipList>,
}

// SAFETY: the members change under the net lock, as in C.
unsafe impl Sync for In6Ifaddr {}

impl In6Ifaddr {
    /// `ia_ifp` (`ia_ifa.ifa_ifp`).
    pub fn ia_ifp(&self) -> &Cell<Option<&'static Ifnet>> {
        &self.ia_ifa.ifa_ifp
    }

    /// `ia_flags` (`ia_ifa.ifa_flags`).
    pub fn ia_flags(&self) -> &Cell<u32> {
        &self.ia_ifa.ifa_flags
    }
}

queue_adapter!(
    /// `TAILQ_ENTRY(in6_ifaddr) ia_list`.
    pub In6IfaddrList: In6Ifaddr, ia_list => TailqEntry<In6Ifaddr>
);

/// `struct in6_ifstat`: IPv6 interface statistics, as defined in RFC2465
/// Ipv6IfStatsEntry (p12).
#[repr(C)]
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct In6Ifstat {
    /// # of total input datagram.
    pub ifs6_in_receive: u64,
    /// # of datagrams with invalid hdr.
    pub ifs6_in_hdrerr: u64,
    /// # of datagrams exceeded MTU.
    pub ifs6_in_toobig: u64,
    /// # of datagrams with no route.
    pub ifs6_in_noroute: u64,
    /// # of datagrams with invalid dst.
    pub ifs6_in_addrerr: u64,
    /// # of datagrams with unknown proto (increment on final dst if).
    pub ifs6_in_protounknown: u64,
    /// # of truncated datagrams.
    pub ifs6_in_truncated: u64,
    /// # of discarded datagrams (fragment timeout is not here).
    pub ifs6_in_discard: u64,
    /// # of datagrams delivered to ULP (increment on final dst if).
    pub ifs6_in_deliver: u64,
    /// # of datagrams forwarded (increment on outgoing if).
    pub ifs6_out_forward: u64,
    /// # of outgoing datagrams from ULP (does not include forwards).
    pub ifs6_out_request: u64,
    /// # of discarded datagrams.
    pub ifs6_out_discard: u64,
    /// # of datagrams fragmented.
    pub ifs6_out_fragok: u64,
    /// # of datagrams failed on fragment.
    pub ifs6_out_fragfail: u64,
    /// # of fragment datagrams (this is # after fragment).
    pub ifs6_out_fragcreat: u64,
    /// # of incoming fragmented packets (increment on final dst if).
    pub ifs6_reass_reqd: u64,
    /// # of reassembled packets (this is # after reass; increment on final dst if).
    pub ifs6_reass_ok: u64,
    /// # of reass failures (may not be packet count; increment on final dst if).
    pub ifs6_reass_fail: u64,
    /// # of inbound multicast datagrams.
    pub ifs6_in_mcast: u64,
    /// # of outbound multicast datagrams.
    pub ifs6_out_mcast: u64,
}

/// `struct icmp6_ifstat`: ICMPv6 interface statistics, as defined in RFC2466
/// Ipv6IfIcmpEntry.
#[repr(C)]
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Icmp6Ifstat {
    // Input statistics
    /// ipv6IfIcmpInMsgs, total # of input messages.
    pub ifs6_in_msg: u64,
    /// ipv6IfIcmpInErrors, # of input error messages.
    pub ifs6_in_error: u64,
    /// ipv6IfIcmpInDestUnreachs, # of input dest unreach errors.
    pub ifs6_in_dstunreach: u64,
    /// ipv6IfIcmpInAdminProhibs, # of input administratively prohibited errs.
    pub ifs6_in_adminprohib: u64,
    /// ipv6IfIcmpInTimeExcds, # of input time exceeded errors.
    pub ifs6_in_timeexceed: u64,
    /// ipv6IfIcmpInParmProblems, # of input parameter problem errors.
    pub ifs6_in_paramprob: u64,
    /// ipv6IfIcmpInPktTooBigs, # of input packet too big errors.
    pub ifs6_in_pkttoobig: u64,
    /// ipv6IfIcmpInEchos, # of input echo requests.
    pub ifs6_in_echo: u64,
    /// ipv6IfIcmpInEchoReplies, # of input echo replies.
    pub ifs6_in_echoreply: u64,
    /// ipv6IfIcmpInRouterSolicits, # of input router solicitations.
    pub ifs6_in_routersolicit: u64,
    /// ipv6IfIcmpInRouterAdvertisements, # of input router advertisements.
    pub ifs6_in_routeradvert: u64,
    /// ipv6IfIcmpInNeighborSolicits, # of input neighbor solicitations.
    pub ifs6_in_neighborsolicit: u64,
    /// ipv6IfIcmpInNeighborAdvertisements, # of input neighbor advertisements.
    pub ifs6_in_neighboradvert: u64,
    /// ipv6IfIcmpInRedirects, # of input redirects.
    pub ifs6_in_redirect: u64,
    /// ipv6IfIcmpInGroupMembQueries, # of input MLD queries.
    pub ifs6_in_mldquery: u64,
    /// ipv6IfIcmpInGroupMembResponses, # of input MLD reports.
    pub ifs6_in_mldreport: u64,
    /// ipv6IfIcmpInGroupMembReductions, # of input MLD done.
    pub ifs6_in_mlddone: u64,
    // Output statistics. We should solve unresolved routing problem...
    /// ipv6IfIcmpOutMsgs, total # of output messages.
    pub ifs6_out_msg: u64,
    /// ipv6IfIcmpOutErrors, # of output error messages.
    pub ifs6_out_error: u64,
    /// ipv6IfIcmpOutDestUnreachs, # of output dest unreach errors.
    pub ifs6_out_dstunreach: u64,
    /// ipv6IfIcmpOutAdminProhibs, # of output administratively prohibited errs.
    pub ifs6_out_adminprohib: u64,
    /// ipv6IfIcmpOutTimeExcds, # of output time exceeded errors.
    pub ifs6_out_timeexceed: u64,
    /// ipv6IfIcmpOutParmProblems, # of output parameter problem errors.
    pub ifs6_out_paramprob: u64,
    /// ipv6IfIcmpOutPktTooBigs, # of output packet too big errors.
    pub ifs6_out_pkttoobig: u64,
    /// ipv6IfIcmpOutEchos, # of output echo requests.
    pub ifs6_out_echo: u64,
    /// ipv6IfIcmpOutEchoReplies, # of output echo replies.
    pub ifs6_out_echoreply: u64,
    /// ipv6IfIcmpOutRouterSolicits, # of output router solicitations.
    pub ifs6_out_routersolicit: u64,
    /// ipv6IfIcmpOutRouterAdvertisements, # of output router advertisements.
    pub ifs6_out_routeradvert: u64,
    /// ipv6IfIcmpOutNeighborSolicits, # of output neighbor solicitations.
    pub ifs6_out_neighborsolicit: u64,
    /// ipv6IfIcmpOutNeighborAdvertisements, # of output neighbor advertisements.
    pub ifs6_out_neighboradvert: u64,
    /// ipv6IfIcmpOutRedirects, # of output redirects.
    pub ifs6_out_redirect: u64,
    /// ipv6IfIcmpOutGroupMembQueries, # of output MLD queries.
    pub ifs6_out_mldquery: u64,
    /// ipv6IfIcmpOutGroupMembResponses, # of output MLD reports.
    pub ifs6_out_mldreport: u64,
    /// ipv6IfIcmpOutGroupMembReductions, # of output MLD done.
    pub ifs6_out_mlddone: u64,
}

/// `in6_ifreq`'s `ifr_ifru` union.
#[repr(C)]
#[derive(Clone, Copy)]
pub union In6IfreqIfru {
    /// `ifru_addr`.
    pub ifru_addr: SockaddrIn6,
    /// `ifru_dstaddr`.
    pub ifru_dstaddr: SockaddrIn6,
    /// `ifru_flags`.
    pub ifru_flags: i16,
    /// `ifru_flags6`.
    pub ifru_flags6: i32,
    /// `ifru_metric`.
    pub ifru_metric: i32,
    /// `ifru_data`.
    pub ifru_data: *mut u8,
    /// `ifru_lifetime`.
    pub ifru_lifetime: In6Addrlifetime,
    /// `ifru_stat`.
    pub ifru_stat: In6Ifstat,
    /// `ifru_icmp6stat`.
    pub ifru_icmp6stat: Icmp6Ifstat,
}

/// `struct in6_ifreq`: the argument of the per-address `SIOC*_IN6` requests.
#[repr(C)]
#[derive(Clone, Copy)]
pub struct In6Ifreq {
    /// `ifr_name`.
    pub ifr_name: [u8; IFNAMSIZ],
    /// `ifr_ifru`.
    pub ifr_ifru: In6IfreqIfru,
}

/// Defines `in6_ifreq` union accessor pairs for `Copy` members.
macro_rules! in6_ifreq_accessor {
    ($(#[$doc:meta] $get:ident, $set:ident => $field:ident: $ty:ty;)*) => {
        $(
            #[$doc]
            pub fn $get(&self) -> $ty {
                // SAFETY: every member of the union is plain data, valid for any bit pattern.
                unsafe { self.ifr_ifru.$field }
            }

            #[$doc]
            pub fn $set(&mut self, v: $ty) {
                self.ifr_ifru.$field = v;
            }
        )*
    };
}

impl In6Ifreq {
    /// An all-zero request.
    pub const fn zeroed() -> Self {
        Self {
            ifr_name: [0; IFNAMSIZ],
            ifr_ifru: In6IfreqIfru {
                ifru_icmp6stat: Icmp6Ifstat {
                    ifs6_in_msg: 0,
                    ifs6_in_error: 0,
                    ifs6_in_dstunreach: 0,
                    ifs6_in_adminprohib: 0,
                    ifs6_in_timeexceed: 0,
                    ifs6_in_paramprob: 0,
                    ifs6_in_pkttoobig: 0,
                    ifs6_in_echo: 0,
                    ifs6_in_echoreply: 0,
                    ifs6_in_routersolicit: 0,
                    ifs6_in_routeradvert: 0,
                    ifs6_in_neighborsolicit: 0,
                    ifs6_in_neighboradvert: 0,
                    ifs6_in_redirect: 0,
                    ifs6_in_mldquery: 0,
                    ifs6_in_mldreport: 0,
                    ifs6_in_mlddone: 0,
                    ifs6_out_msg: 0,
                    ifs6_out_error: 0,
                    ifs6_out_dstunreach: 0,
                    ifs6_out_adminprohib: 0,
                    ifs6_out_timeexceed: 0,
                    ifs6_out_paramprob: 0,
                    ifs6_out_pkttoobig: 0,
                    ifs6_out_echo: 0,
                    ifs6_out_echoreply: 0,
                    ifs6_out_routersolicit: 0,
                    ifs6_out_routeradvert: 0,
                    ifs6_out_neighborsolicit: 0,
                    ifs6_out_neighboradvert: 0,
                    ifs6_out_redirect: 0,
                    ifs6_out_mldquery: 0,
                    ifs6_out_mldreport: 0,
                    ifs6_out_mlddone: 0,
                },
            },
        }
    }

    in6_ifreq_accessor! {
        /// `ifr_addr` (`ifr_ifru.ifru_addr`).
        ifr_addr, set_ifr_addr => ifru_addr: SockaddrIn6;
        /// `ifr_dstaddr` (`ifr_ifru.ifru_dstaddr`).
        ifr_dstaddr, set_ifr_dstaddr => ifru_dstaddr: SockaddrIn6;
        /// `ifr_flags` (`ifr_ifru.ifru_flags`).
        ifr_flags, set_ifr_flags => ifru_flags: i16;
        /// `ifr_ifru.ifru_flags6`.
        ifr_flags6, set_ifr_flags6 => ifru_flags6: i32;
        /// `ifr_ifru.ifru_metric`.
        ifr_metric, set_ifr_metric => ifru_metric: i32;
        /// `ifr_ifru.ifru_data`.
        ifr_data, set_ifr_data => ifru_data: *mut u8;
        /// `ifr_ifru.ifru_lifetime`.
        ifr_lifetime, set_ifr_lifetime => ifru_lifetime: In6Addrlifetime;
        /// `ifr_ifru.ifru_stat`.
        ifr_stat, set_ifr_stat => ifru_stat: In6Ifstat;
        /// `ifr_ifru.ifru_icmp6stat`.
        ifr_icmp6stat, set_ifr_icmp6stat => ifru_icmp6stat: Icmp6Ifstat;
    }
}

/// `in6_aliasreq`'s `ifra_ifrau` union.
#[repr(C)]
#[derive(Clone, Copy)]
pub union In6AliasreqIfrau {
    /// `ifrau_addr`.
    pub ifrau_addr: SockaddrIn6,
    /// `ifrau_align`.
    pub ifrau_align: i32,
}

/// `struct in6_aliasreq`: the argument of `SIOCAIFADDR_IN6`.
#[repr(C)]
#[derive(Clone, Copy)]
pub struct In6Aliasreq {
    /// `ifra_name`.
    pub ifra_name: [u8; IFNAMSIZ],
    /// `ifra_ifrau`.
    pub ifra_ifrau: In6AliasreqIfrau,
    /// `ifra_dstaddr`.
    pub ifra_dstaddr: SockaddrIn6,
    /// `ifra_prefixmask`.
    pub ifra_prefixmask: SockaddrIn6,
    /// `ifra_flags`.
    pub ifra_flags: i32,
    /// `ifra_lifetime`.
    pub ifra_lifetime: In6Addrlifetime,
}

impl In6Aliasreq {
    /// An all-zero request.
    pub const fn zeroed() -> Self {
        Self {
            ifra_name: [0; IFNAMSIZ],
            ifra_ifrau: In6AliasreqIfrau {
                ifrau_addr: SockaddrIn6::zeroed(),
            },
            ifra_dstaddr: SockaddrIn6::zeroed(),
            ifra_prefixmask: SockaddrIn6::zeroed(),
            ifra_flags: 0,
            ifra_lifetime: In6Addrlifetime {
                ia6t_expire: 0,
                ia6t_preferred: 0,
                ia6t_vltime: 0,
                ia6t_pltime: 0,
            },
        }
    }

    /// `ifra_addr` (`ifra_ifrau.ifrau_addr`).
    pub fn ifra_addr(&self) -> &SockaddrIn6 {
        // SAFETY: both members are plain data, valid for any bit pattern; the union is the
        // size of the address.
        unsafe { &self.ifra_ifrau.ifrau_addr }
    }

    /// `ifra_addr`, writable.
    pub fn ifra_addr_mut(&mut self) -> &mut SockaddrIn6 {
        // SAFETY: as above.
        unsafe { &mut self.ifra_ifrau.ifrau_addr }
    }
}

/// `SIOCDIFADDR_IN6`.
pub const SIOCDIFADDR_IN6: u64 = _iow::<In6Ifreq>(b'i', 25);
/// `SIOCAIFADDR_IN6`.
pub const SIOCAIFADDR_IN6: u64 = _iow::<In6Aliasreq>(b'i', 26);

/// `SIOCGIFDSTADDR_IN6`.
pub const SIOCGIFDSTADDR_IN6: u64 = _iowr::<In6Ifreq>(b'i', 34);
/// `SIOCGIFNETMASK_IN6`.
pub const SIOCGIFNETMASK_IN6: u64 = _iowr::<In6Ifreq>(b'i', 37);

/// `SIOCGIFAFLAG_IN6`.
pub const SIOCGIFAFLAG_IN6: u64 = _iowr::<In6Ifreq>(b'i', 73);

/// `SIOCGIFINFO_IN6`.
pub const SIOCGIFINFO_IN6: u64 = _iowr::<In6Ndireq>(b'i', 108);
/// `SIOCGNBRINFO_IN6`.
pub const SIOCGNBRINFO_IN6: u64 = _iowr::<In6Nbrinfo>(b'i', 78);

/// `SIOCGIFALIFETIME_IN6`.
pub const SIOCGIFALIFETIME_IN6: u64 = _iowr::<In6Ifreq>(b'i', 81);

/// `SIOCGETSGCNT_IN6` (`struct sioc_sg_req6`, 80 bytes).
pub const SIOCGETSGCNT_IN6: u64 = _ioc(IOC_INOUT, b'u', 106, 80);
/// `SIOCGETMIFCNT_IN6` (`struct sioc_mif_req6`, 40 bytes).
pub const SIOCGETMIFCNT_IN6: u64 = _ioc(IOC_INOUT, b'u', 107, 40);

/// Anycast address.
pub const IN6_IFF_ANYCAST: i32 = 0x01;
/// Tentative address.
pub const IN6_IFF_TENTATIVE: i32 = 0x02;
/// DAD detected duplicate.
pub const IN6_IFF_DUPLICATED: i32 = 0x04;
/// May be detached from the link.
pub const IN6_IFF_DETACHED: i32 = 0x08;
/// Deprecated address.
pub const IN6_IFF_DEPRECATED: i32 = 0x10;
/// Autoconfigurable address.
pub const IN6_IFF_AUTOCONF: i32 = 0x40;
/// RFC 4941 temporary address.
pub const IN6_IFF_TEMPORARY: i32 = 0x80;

/// `struct in6_multi_mship`: multi-cast membership entry. One for each group/ifp that a PCB
/// belongs to.
pub struct In6MultiMship {
    /// `i6mm_maddr`: multicast address pointer.
    pub i6mm_maddr: Cell<Option<&'static In6Multi>>,
    /// `i6mm_chain`: multicast options chain.
    pub i6mm_chain: ListEntry<In6MultiMship>,
}

// SAFETY: the members change under the net lock, as in C.
unsafe impl Sync for In6MultiMship {}

queue_adapter!(
    /// `LIST_HEAD(, in6_multi_mship)` through `i6mm_chain` (`ia6_memberships`,
    /// `im6o_memberships`).
    pub In6MultiMshipList: In6MultiMship, i6mm_chain => ListEntry<In6MultiMship>
);

/// `struct in6_multi`: IPv6 multicast address structure.
#[repr(C)]
pub struct In6Multi {
    /// `in6m_ifma`: protocol-independent info.
    pub in6m_ifma: Ifmaddr,
    /// \[I\] `in6m_sin`: IPv6 multicast address.
    pub in6m_sin: Cell<SockaddrIn6>,
    /// \[m\] `in6m_state`: state of membership.
    pub in6m_state: Cell<u32>,
    /// \[m\] `in6m_timer`: MLD6 membership report.
    pub in6m_timer: Cell<u32>,
}

// SAFETY: the members change under the interface's `if_maddrlock`, as in C.
unsafe impl Sync for In6Multi {}

impl In6Multi {
    /// `in6m_refcnt` (`in6m_ifma.ifma_refcnt`).
    pub fn in6m_refcnt(&self) -> &Refcnt {
        &self.in6m_ifma.ifma_refcnt
    }

    /// `in6m_ifidx` (`in6m_ifma.ifma_ifidx`).
    pub fn in6m_ifidx(&self) -> &Cell<u32> {
        &self.in6m_ifma.ifma_ifidx
    }

    /// `in6m_addr` (`in6m_sin.sin6_addr`).
    pub fn in6m_addr(&self) -> In6Addr {
        self.in6m_sin.get().sin6_addr
    }
}

/// `IA6_IN6(ia)`: the interface address.
pub fn ia6_in6(ia: &In6Ifaddr) -> In6Addr {
    ia.ia_addr.get().sin6_addr
}

/// `IA6_DSTIN6(ia)`: the destination address.
pub fn ia6_dstin6(ia: &In6Ifaddr) -> In6Addr {
    ia.ia_dstaddr.get().sin6_addr
}

/// `IA6_MASKIN6(ia)`: the prefix mask.
pub fn ia6_maskin6(ia: &In6Ifaddr) -> In6Addr {
    ia.ia_prefixmask.get().sin6_addr
}

/// `IA6_SIN6(ia)`: the interface address as a socket address.
pub fn ia6_sin6(ia: &In6Ifaddr) -> SockaddrIn6 {
    ia.ia_addr.get()
}

/// `IA6_DSTSIN6(ia)`: the destination address as a socket address.
pub fn ia6_dstsin6(ia: &In6Ifaddr) -> SockaddrIn6 {
    ia.ia_dstaddr.get()
}

/// Reads the `sin6_addr` of an interface address's socket address.
fn sa_in6(sa: *mut Sockaddr) -> In6Addr {
    if sa.is_null() {
        panic(format_args!("IFA_IN6: no address"));
    }
    // SAFETY: an interface address's socket addresses are readable for their `sa_len`
    // (`ifa_add`'s contract); for an `AF_INET6` address that is a `sockaddr_in6`, read
    // unaligned because the generic structure has a smaller alignment.
    unsafe { core::ptr::read_unaligned(sa.cast::<SockaddrIn6>()).sin6_addr }
}

/// `IFA_IN6(x)`: the IPv6 address of an `AF_INET6` interface address.
pub fn ifa_in6(x: &Ifaddr) -> In6Addr {
    sa_in6(x.ifa_addr.get())
}

/// `IFA_DSTIN6(x)`: the IPv6 destination address of an `AF_INET6` interface address.
pub fn ifa_dstin6(x: &Ifaddr) -> In6Addr {
    sa_in6(x.ifa_dstaddr.get())
}

/// `IN6_ARE_MASKED_ADDR_EQUAL(d, a, m)`: whether `d` and `a` agree under mask `m`.
pub const fn in6_are_masked_addr_equal(d: &In6Addr, a: &In6Addr, m: &In6Addr) -> bool {
    let mut i = 0;
    while i < 4 {
        if (d.s6_addr32(i) ^ a.s6_addr32(i)) & m.s6_addr32(i) != 0 {
            return false;
        }
        i += 1;
    }
    true
}

/// `IN6_ARE_SCOPE_CMP(a, b)`.
pub const fn in6_are_scope_cmp(a: i32, b: i32) -> i32 {
    a - b
}

/// `IN6_ARE_SCOPE_EQUAL(a, b)`.
pub const fn in6_are_scope_equal(a: i32, b: i32) -> bool {
    a == b
}

/// `ifmatoin6m(ifma)`: the IPv6 multicast record an `AF_INET6` multicast address is the
/// first member of.
pub fn ifmatoin6m(ifma: &Ifmaddr) -> &In6Multi {
    let addr = ifma.ifma_addr.get();
    // SAFETY: a multicast record's address is readable (its protocol set it).
    if addr.is_null() || unsafe { (*addr).sa_family } != AF_INET6 {
        panic(format_args!("ifmatoin6m: not an inet6 record"));
    }
    // SAFETY: every `AF_INET6` multicast record is the `in6m_ifma` member, at offset 0 of
    // the `#[repr(C)]` `In6Multi` that `in6_addmulti` allocated.
    unsafe { &*core::ptr::from_ref(ifma).cast::<In6Multi>() }
}

// LP64 sizes of the user-visible structures.
const _: () = {
    assert!(size_of::<In6Addrlifetime>() == 24);
    assert!(size_of::<In6Ifstat>() == 20 * 8);
    assert!(size_of::<Icmp6Ifstat>() == 34 * 8);
    assert!(size_of::<In6Ifreq>() == IFNAMSIZ + 34 * 8);
    assert!(size_of::<In6Aliasreq>() == 128);
};
/* </CODE> */

/* <TESTS> */
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn masked_comparison_and_ioctl_numbers() {
        let a = In6Addr::new([0x20, 1, 0xd, 0xb8, 0, 0, 0, 1, 0, 0, 0, 0, 0, 0, 0, 1]);
        let b = In6Addr::new([0x20, 1, 0xd, 0xb8, 0, 0, 0, 1, 0, 0, 0, 0, 0, 0, 0, 2]);
        let m64 = crate::netinet6::in6::IN6MASK64;
        assert!(in6_are_masked_addr_equal(&a, &b, &m64));
        assert!(!in6_are_masked_addr_equal(
            &a,
            &b,
            &crate::netinet6::in6::IN6MASK128
        ));
        // The values OpenBSD's userland uses (LP64).
        assert_eq!(SIOCAIFADDR_IN6, 0x8080_691a);
        assert_eq!(SIOCDIFADDR_IN6, 0x8120_6919);
        assert_eq!(SIOCGIFAFLAG_IN6, 0xc120_6949);
    }

    #[test]
    #[ignore = "needs OPENBSD_SRC (just test-ref)"]
    fn values_match_the_c_header() {
        let defs = crate::reftest::defines("sys/netinet6/in6_var.h");
        let iff = crate::reftest::assert_defines!(defs;
            IN6_IFF_ANYCAST, IN6_IFF_TENTATIVE, IN6_IFF_DUPLICATED, IN6_IFF_DETACHED,
            IN6_IFF_DEPRECATED, IN6_IFF_AUTOCONF, IN6_IFF_TEMPORARY);
        crate::reftest::assert_complete(&defs, "IN6_IFF_", &iff);
    }
}
/* </TESTS> */
