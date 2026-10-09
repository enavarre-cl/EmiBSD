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

/* <CODE> */
//! The IPv6 protocol switch `inet6sw[]`, `ip6_protox[]` and `inet6domain`, and the IPv6
//! configuration variables: `netinet6/in6_proto.c`.
//!
//! Upstream: sys/netinet6/in6_proto.c @ 3ce1f3f79392
//!
//! IPv6, raw IPv6, ICMPv6, the destination options, routing and fragment headers, AH, ESP
//! and IPComp, IPv4 and IPv6 in IPv6, divert and the raw wildcard, in the C's order.
//! `ip6_init` fills `ip6_protox[]`, which maps an IP protocol number to its entry in
//! `inet6sw[]`; every number without an entry goes to the raw IPv6 handler.
//!
//! Locks: \[a\] atomic operations. The sysctl variables are atomics, as
//! `netinet/ip_input.rs`'s are.
//!
//! ## Deviations
//! - `IPSEC` is configured (M9c): AH, ESP and IPComp take `ah46_input`, `esp46_input` and
//!   `ipcomp46_input`, as in C.
//! - `NGIF` is 0 (`ipip_input` serves `IPPROTO_IPV4` and `IPPROTO_IPV6`); `MPLS`, `NCARP`,
//!   `NETHERIP` and `NGRE` are not configured: their entries are comments. `NPF` is:
//!   `IPPROTO_DIVERT` (`netinet6/ip6_divert.c`) has its entry. `SMALL_KERNEL` is not set,
//!   so the sysctl handlers are in the table.
//! - `ip6_protox[]` holds atomics (`ip6_init` writes it once, every input reads it).
//! - `rip6_sendspace`/`rip6_recvspace` (`u_long`, no sysctl) are constants of the same
//!   names, as `rip_sendspace` is.

use core::mem::{offset_of, size_of};
use core::sync::atomic::{AtomicI32, AtomicU8};

use crate::netinet::in_::{
    IPPROTO_AH, IPPROTO_DIVERT, IPPROTO_DSTOPTS, IPPROTO_ESP, IPPROTO_FRAGMENT, IPPROTO_ICMPV6,
    IPPROTO_IPCOMP, IPPROTO_IPV4, IPPROTO_IPV6, IPPROTO_MAX, IPPROTO_RAW, IPPROTO_ROUTING,
    IPPROTO_TCP, IPPROTO_UDP,
};
use crate::netinet::ip_ipip::ipip_input;
use crate::netinet::ip_var::IPMTUDISCTIMEOUT;
use crate::netinet::ip6::IPV6_DEFHLIM;
use crate::netinet::ipsec_input::{
    ah_sysctl, ah46_input, esp_sysctl, esp46_input, ipcomp_sysctl, ipcomp46_input,
};
use crate::netinet::tcp_input::tcp_input;
use crate::netinet::tcp_subr::tcp6_ctlinput;
use crate::netinet::tcp_usrreq::{TCP6_USRREQS, tcp_ctloutput, tcp_sysctl};
use crate::netinet::udp_usrreq::{UDP6_USRREQS, udp_input, udp_sysctl, udp6_ctlinput};
use crate::netinet6::dest6::dest6_input;
use crate::netinet6::frag6::{frag6_input, frag6_slowtimo};
use crate::netinet6::icmp6::{icmp6_fasttimo, icmp6_init, icmp6_input, icmp6_sysctl};
use crate::netinet6::in6::{IPV6_DEFAULT_MULTICAST_HOPS, SockaddrIn6};
use crate::netinet6::ip6_divert::{DIVERT6_USRREQS, divert6_init};
use crate::netinet6::ip6_input::{ip6_init, ip6_sysctl};
use crate::netinet6::ip6_output::ip6_ctloutput;
use crate::netinet6::raw_ip6::{
    RIP6_USRREQS, rip6_ctlinput, rip6_ctloutput, rip6_init, rip6_input, rip6_sysctl,
};
use crate::netinet6::route6::route6_input;
use crate::sys::domain::Domain;
use crate::sys::protosw::{
    PR_ABRTACPTDIS, PR_ADDR, PR_ATOMIC, PR_CONNREQUIRED, PR_MPINPUT, PR_MPSYSCTL, PR_SPLICE,
    PR_WANTRCVD, Protosw,
};
use crate::sys::socket::{AF_INET6, SOCK_DGRAM, SOCK_RAW, SOCK_STREAM};

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

/// `inet6sw[]`: TCP/IP protocol family: IP6, ICMP6, UDP, TCP.
pub static INET6SW: [Protosw; 15] = [
    Protosw {
        pr_protocol: IPPROTO_IPV6 as i16,
        pr_flags: PR_MPSYSCTL,
        pr_init: Some(ip6_init),
        pr_slowtimo: Some(frag6_slowtimo),
        pr_sysctl: Some(ip6_sysctl),
        ..Protosw::new(&INET6DOMAIN)
    },
    Protosw {
        pr_type: SOCK_DGRAM as i16,
        pr_protocol: IPPROTO_UDP as i16,
        pr_flags: PR_ATOMIC | PR_ADDR | PR_SPLICE | PR_MPINPUT | PR_MPSYSCTL,
        pr_input: Some(udp_input),
        pr_ctlinput: Some(udp6_ctlinput),
        pr_ctloutput: Some(ip6_ctloutput),
        pr_usrreqs: Some(&UDP6_USRREQS),
        pr_sysctl: Some(udp_sysctl),
        ..Protosw::new(&INET6DOMAIN)
    },
    Protosw {
        pr_type: SOCK_STREAM as i16,
        pr_protocol: IPPROTO_TCP as i16,
        pr_flags: PR_CONNREQUIRED
            | PR_WANTRCVD
            | PR_ABRTACPTDIS
            | PR_SPLICE
            | PR_MPINPUT
            | PR_MPSYSCTL,
        pr_input: Some(tcp_input),
        pr_ctlinput: Some(tcp6_ctlinput),
        pr_ctloutput: Some(tcp_ctloutput),
        pr_usrreqs: Some(&TCP6_USRREQS),
        pr_sysctl: Some(tcp_sysctl),
        ..Protosw::new(&INET6DOMAIN)
    },
    Protosw {
        pr_type: SOCK_RAW as i16,
        pr_protocol: IPPROTO_RAW as i16,
        pr_flags: PR_ATOMIC | PR_ADDR | PR_MPINPUT | PR_MPSYSCTL,
        pr_input: Some(rip6_input),
        pr_ctlinput: Some(rip6_ctlinput),
        pr_ctloutput: Some(rip6_ctloutput),
        pr_usrreqs: Some(&RIP6_USRREQS),
        pr_sysctl: Some(rip6_sysctl),
        ..Protosw::new(&INET6DOMAIN)
    },
    Protosw {
        pr_type: SOCK_RAW as i16,
        pr_protocol: IPPROTO_ICMPV6 as i16,
        pr_flags: PR_ATOMIC | PR_ADDR | PR_MPSYSCTL,
        pr_input: Some(icmp6_input),
        pr_ctlinput: Some(rip6_ctlinput),
        pr_ctloutput: Some(rip6_ctloutput),
        pr_usrreqs: Some(&RIP6_USRREQS),
        pr_init: Some(icmp6_init),
        pr_fasttimo: Some(icmp6_fasttimo),
        pr_sysctl: Some(icmp6_sysctl),
        ..Protosw::new(&INET6DOMAIN)
    },
    Protosw {
        pr_type: SOCK_RAW as i16,
        pr_protocol: IPPROTO_DSTOPTS as i16,
        pr_flags: PR_ATOMIC | PR_ADDR | PR_MPINPUT,
        pr_input: Some(dest6_input),
        ..Protosw::new(&INET6DOMAIN)
    },
    Protosw {
        pr_type: SOCK_RAW as i16,
        pr_protocol: IPPROTO_ROUTING as i16,
        pr_flags: PR_ATOMIC | PR_ADDR | PR_MPINPUT,
        pr_input: Some(route6_input),
        ..Protosw::new(&INET6DOMAIN)
    },
    Protosw {
        pr_type: SOCK_RAW as i16,
        pr_protocol: IPPROTO_FRAGMENT as i16,
        pr_flags: PR_ATOMIC | PR_ADDR | PR_MPINPUT,
        pr_input: Some(frag6_input),
        ..Protosw::new(&INET6DOMAIN)
    },
    Protosw {
        pr_type: SOCK_RAW as i16,
        pr_protocol: IPPROTO_AH as i16,
        pr_flags: PR_ATOMIC | PR_ADDR | PR_MPSYSCTL,
        pr_input: Some(ah46_input),
        pr_ctloutput: Some(rip6_ctloutput),
        pr_usrreqs: Some(&RIP6_USRREQS),
        pr_sysctl: Some(ah_sysctl),
        ..Protosw::new(&INET6DOMAIN)
    },
    Protosw {
        pr_type: SOCK_RAW as i16,
        pr_protocol: IPPROTO_ESP as i16,
        pr_flags: PR_ATOMIC | PR_ADDR | PR_MPSYSCTL,
        pr_input: Some(esp46_input),
        pr_ctloutput: Some(rip6_ctloutput),
        pr_usrreqs: Some(&RIP6_USRREQS),
        pr_sysctl: Some(esp_sysctl),
        ..Protosw::new(&INET6DOMAIN)
    },
    Protosw {
        pr_type: SOCK_RAW as i16,
        pr_protocol: IPPROTO_IPCOMP as i16,
        pr_flags: PR_ATOMIC | PR_ADDR | PR_MPSYSCTL,
        pr_input: Some(ipcomp46_input),
        pr_ctloutput: Some(rip6_ctloutput),
        pr_usrreqs: Some(&RIP6_USRREQS),
        pr_sysctl: Some(ipcomp_sysctl),
        ..Protosw::new(&INET6DOMAIN)
    },
    Protosw {
        pr_type: SOCK_RAW as i16,
        pr_protocol: IPPROTO_IPV4 as i16,
        pr_flags: PR_ATOMIC | PR_ADDR,
        // NGIF > 0: in6_gif_input; not configured.
        pr_input: Some(ipip_input),
        pr_ctloutput: Some(rip6_ctloutput),
        pr_usrreqs: Some(&RIP6_USRREQS), // XXX
        ..Protosw::new(&INET6DOMAIN)
    },
    Protosw {
        pr_type: SOCK_RAW as i16,
        pr_protocol: IPPROTO_IPV6 as i16,
        pr_flags: PR_ATOMIC | PR_ADDR,
        // NGIF > 0: in6_gif_input; not configured.
        pr_input: Some(ipip_input),
        pr_ctloutput: Some(rip6_ctloutput),
        pr_usrreqs: Some(&RIP6_USRREQS), // XXX
        ..Protosw::new(&INET6DOMAIN)
    },
    // MPLS && NGIF > 0: IPPROTO_MPLS through in6_gif_input; NCARP > 0: IPPROTO_CARP
    // (carp6_proto_input); neither configured.
    Protosw {
        pr_type: SOCK_RAW as i16,
        pr_protocol: IPPROTO_DIVERT as i16,
        pr_flags: PR_ATOMIC | PR_ADDR | PR_MPSYSCTL,
        pr_ctloutput: Some(rip6_ctloutput),
        pr_usrreqs: Some(&DIVERT6_USRREQS),
        pr_init: Some(divert6_init),
        ..Protosw::new(&INET6DOMAIN)
    },
    // NETHERIP > 0: IPPROTO_ETHERIP (ip6_etherip_input); NGRE > 0: IPPROTO_GRE
    // (gre_input6); neither configured.
    Protosw {
        // raw wildcard
        pr_type: SOCK_RAW as i16,
        pr_flags: PR_ATOMIC | PR_ADDR | PR_MPINPUT,
        pr_input: Some(rip6_input),
        pr_ctloutput: Some(rip6_ctloutput),
        pr_usrreqs: Some(&RIP6_USRREQS),
        pr_init: Some(rip6_init),
        ..Protosw::new(&INET6DOMAIN)
    },
];

/// `inet6domain`.
pub static INET6DOMAIN: Domain = Domain {
    dom_family: AF_INET6 as i32,
    dom_name: b"inet6",
    dom_init: None,
    dom_externalize: None,
    dom_dispose: None,
    dom_protosw: &INET6SW,
    dom_sasize: size_of::<SockaddrIn6>() as u32,
    dom_rtoffset: offset_of!(SockaddrIn6, sin6_addr) as u32,
    dom_maxplen: 128,
};

// Internet configuration info.

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

// ICMPV6 parameters.

/// `icmp6_redirtimeout`: cache time for redirect routes, 10 minutes.
pub static ICMP6_REDIRTIMEOUT: AtomicI32 = AtomicI32::new(10 * 60);
/// \[a\] `icmp6errppslim`: 100pps.
pub static ICMP6ERRPPSLIM: AtomicI32 = AtomicI32::new(100);
/// \[a\] `ip6_mtudisc_timeout`: mtu discovery.
pub static IP6_MTUDISC_TIMEOUT: AtomicI32 = AtomicI32::new(IPMTUDISCTIMEOUT);
/* </CODE> */

/* <TESTS> */
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_entry_is_of_the_inet6_domain_and_the_wildcard_is_last() {
        assert!(
            INET6SW
                .iter()
                .all(|pr| core::ptr::eq(pr.pr_domain, &INET6DOMAIN))
        );
        let last = &INET6SW[INET6SW.len() - 1];
        assert_eq!((last.pr_type, last.pr_protocol), (SOCK_RAW as i16, 0));
        assert!(last.pr_init.is_some());
        assert_eq!(INET6DOMAIN.dom_sasize, 28);
        assert_eq!(INET6DOMAIN.dom_rtoffset, 8);
        let icmp6 = INET6SW
            .iter()
            .find(|pr| i32::from(pr.pr_protocol) == IPPROTO_ICMPV6);
        assert!(icmp6.is_some_and(|pr| pr.pr_type == SOCK_RAW as i16 && pr.pr_usrreqs.is_some()));
    }
}
/* </TESTS> */
