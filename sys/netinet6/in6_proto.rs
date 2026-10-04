/*	$OpenBSD: in6_proto.c,v 1.153 2025/10/24 11:51:49 mvs Exp $	*/
/*	$KAME: in6_proto.c,v 1.66 2000/10/10 15:35:47 itojun Exp $	*/
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
 *	@(#)in_proto.c	8.1 (Berkeley) 6/10/93
 */
/* </LICENSES> */

//! The IPv6 protocol switch and domain, and the IPv6 configuration variables:
//! `netinet6/in6_proto.c`.
//!
//! Upstream: sys/netinet6/in6_proto.c @ 3ce1f3f79392
//!
//! Locks: \[a\] atomic operations. The sysctl variables are atomics, as
//! `netinet/ip_input.rs`'s are. `inet6sw[]` and `inet6domain` come with the port of
//! this file.
//!
//! Status: skeleton from the INET6 foundation step: the globals are defined, every
//! function has its final signature and a placeholder body that reports itself through
//! `unported!` until the file is ported.
//!
//! ## Deviations
//! - None yet: the file is a skeleton (see `Status`).

use crate::netinet::in_::IPPROTO_MAX;
use crate::netinet::ip_var::IPMTUDISCTIMEOUT;
use crate::netinet::ip6::IPV6_DEFHLIM;
use crate::netinet6::in6::IPV6_DEFAULT_MULTICAST_HOPS;
use core::sync::atomic::{AtomicI32, AtomicU8};

/// Nominal space allocated to a raw ip6 socket: send.
pub const RIPV6SNDQ: u64 = 8192;
/// Nominal space allocated to a raw ip6 socket: receive.
pub const RIPV6RCVQ: u64 = 8192;

/// `rip6_sendspace`.
pub const RIP6_SENDSPACE: u64 = RIPV6SNDQ;
/// `rip6_recvspace`.
pub const RIP6_RECVSPACE: u64 = RIPV6RCVQ;

/// `ip6_protox[]`: the index in `inet6sw[]` of each protocol (`ip6_init` fills it).
pub static IP6_PROTOX: [AtomicU8; IPPROTO_MAX as usize] =
    [const { AtomicU8::new(0) }; IPPROTO_MAX as usize];

/// \[a\] `ip6_forwarding`: no forwarding unless sysctl to enable.
pub static IP6_FORWARDING: AtomicI32 = AtomicI32::new(0);
/// \[a\] `ip6_mforwarding`: no multicast forwarding unless ...
pub static IP6_MFORWARDING: AtomicI32 = AtomicI32::new(0);
/// \[a\] `ip6_multipath`: no using multipath routes unless ...
pub static IP6_MULTIPATH: AtomicI32 = AtomicI32::new(0);
/// \[a\] `ip6_sendredirects`.
pub static IP6_SENDREDIRECTS: AtomicI32 = AtomicI32::new(1);
/// \[a\] `ip6_defhlim`: default hop limit.
pub static IP6_DEFHLIM: AtomicI32 = AtomicI32::new(IPV6_DEFHLIM as i32);
/// \[a\] `ip6_defmcasthlim`: default multicast hop limit.
pub static IP6_DEFMCASTHLIM: AtomicI32 = AtomicI32::new(IPV6_DEFAULT_MULTICAST_HOPS);
/// \[a\] `ip6_maxfragpackets`: maximum packets in reassembly queue.
pub static IP6_MAXFRAGPACKETS: AtomicI32 = AtomicI32::new(200);
/// \[a\] `ip6_maxfrags`: maximum fragments in reassembly queue.
pub static IP6_MAXFRAGS: AtomicI32 = AtomicI32::new(200);
/// \[a\] `ip6_hdrnestlimit`: upper limit of # of extension headers (appropriate?).
pub static IP6_HDRNESTLIMIT: AtomicI32 = AtomicI32::new(10);
/// \[a\] `ip6_dad_count`: DupAddrDetectionTransmits.
pub static IP6_DAD_COUNT: AtomicI32 = AtomicI32::new(1);
/// `ip6_dad_pending`: number of currently running DADs.
pub static IP6_DAD_PENDING: AtomicI32 = AtomicI32::new(0);
/// \[a\] `ip6_mcast_pmtu`: enable pMTU discovery for multicast?
pub static IP6_MCAST_PMTU: AtomicI32 = AtomicI32::new(0);
/// \[a\] `ip6_neighborgcthresh`: threshold # of NDP entries for GC.
pub static IP6_NEIGHBORGCTHRESH: AtomicI32 = AtomicI32::new(2048);
/// \[a\] `ip6_maxdynroutes`: max # of routes created via redirect.
pub static IP6_MAXDYNROUTES: AtomicI32 = AtomicI32::new(4096);

/// `icmp6_redirtimeout`: cache time for redirect routes, 10 minutes.
pub static ICMP6_REDIRTIMEOUT: AtomicI32 = AtomicI32::new(10 * 60);
/// \[a\] `icmp6errppslim`: 100pps.
pub static ICMP6ERRPPSLIM: AtomicI32 = AtomicI32::new(100);
/// \[a\] `ip6_mtudisc_timeout`: mtu discovery.
pub static IP6_MTUDISC_TIMEOUT: AtomicI32 = AtomicI32::new(IPMTUDISCTIMEOUT);
