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

/* <CODE> */
//! The TCP/IP protocol family: the protocol switch `inetsw[]`, `ip_protox[]` and
//! `inetdomain`.
//!
//! Upstream: sys/netinet/in_proto.c @ 3ce1f3f79392
//!
//! IP, ICMP, UDP, TCP, raw IP, IP-in-IP, IGMP, AH, ESP and IPComp, in the C's order. `ip_init` fills
//! `ip_protox[]`, which maps an IP protocol number to its entry in `inetsw[]`; every number
//! without an entry goes to the raw IP handler.
//!
//! Status: `ported` (M7b). The licence block carries the NRL notice with its advertising
//! clause, accepted as BSD-4 (`.claude/rules/scope-and-stubs.md`).
//!
//! ## Deviations
//! - TCP's entry has the functions of `netinet/tcp_*.rs` and IGMP's those of
//!   `netinet/igmp.rs` (M9+); IGMP's `pr_ctloutput` and `pr_usrreqs` are the raw ones, as in C.
//! - `IPSEC` is configured (M9c): AH, ESP and IPComp come after IGMP, as in C.
//! - `NGIF` is 0 (`ipip_input` serves `IPPROTO_IPV4`, and with `INET6`, feature `inet6`,
//!   `IPPROTO_IPV6`); `MPLS`, `NGRE`, `NCARP` and `NETHERIP` are not configured: their
//!   entries are comments. The length of `inetsw[]` counts the `INET6` entry. `NPFSYNC` is:
//!   `IPPROTO_PFSYNC` goes to `pfsync_input4` (`net/if_pfsync.rs`); so is `NPF`:
//!   `IPPROTO_DIVERT` (`netinet/ip_divert.c`) has its entry.
//!   `SMALL_KERNEL` is not set, so the sysctl handlers are in the table.
//! - `ip_protox[]` holds atomics (`ip_init` writes it once, every input reads it).

use core::mem::{offset_of, size_of};
use core::sync::atomic::AtomicU8;

use crate::net::if_pfsync::{pfsync_input4, pfsync_sysctl};
use crate::netinet::igmp::{igmp_fasttimo, igmp_init, igmp_input, igmp_slowtimo, igmp_sysctl};
#[cfg(feature = "inet6")]
use crate::netinet::in_::IPPROTO_IPV6;
use crate::netinet::in_::{
    IPPROTO_AH, IPPROTO_DIVERT, IPPROTO_ESP, IPPROTO_ICMP, IPPROTO_IGMP, IPPROTO_IPCOMP,
    IPPROTO_IPV4, IPPROTO_MAX, IPPROTO_PFSYNC, IPPROTO_RAW, IPPROTO_TCP, IPPROTO_UDP, SockaddrIn,
};
use crate::netinet::in_pcb::in_init;
use crate::netinet::ip_divert::{DIVERT_USRREQS, divert_init, divert_sysctl};
use crate::netinet::ip_icmp::{icmp_init, icmp_input, icmp_sysctl};
use crate::netinet::ip_input::{ip_init, ip_slowtimo, ip_sysctl};
use crate::netinet::ip_ipip::{ipip_init, ipip_input, ipip_sysctl};
use crate::netinet::ip_output::ip_ctloutput;
use crate::netinet::ipsec_input::{
    ah_sysctl, ah4_ctlinput, ah46_input, esp_sysctl, esp4_ctlinput, esp46_input, ipcomp_sysctl,
    ipcomp46_input,
};
use crate::netinet::raw_ip::{RIP_USRREQS, rip_ctloutput, rip_init, rip_input};
use crate::netinet::tcp_input::tcp_input;
use crate::netinet::tcp_subr::{tcp_ctlinput, tcp_init};
use crate::netinet::tcp_timer::tcp_slowtimo;
use crate::netinet::tcp_usrreq::{TCP_USRREQS, tcp_ctloutput, tcp_sysctl};
use crate::netinet::udp_usrreq::{UDP_USRREQS, udp_ctlinput, udp_init, udp_input, udp_sysctl};
use crate::sys::domain::Domain;
use crate::sys::protosw::{
    PR_ABRTACPTDIS, PR_ADDR, PR_ATOMIC, PR_CONNREQUIRED, PR_MPINPUT, PR_MPSYSCTL, PR_SPLICE,
    PR_WANTRCVD, Protosw,
};
use crate::sys::socket::{AF_INET, SOCK_DGRAM, SOCK_RAW, SOCK_STREAM};

/// `ip_protox[]`: IP protocol number to `inetsw[]` index.
pub static IP_PROTOX: [AtomicU8; IPPROTO_MAX as usize] =
    [const { AtomicU8::new(0) }; IPPROTO_MAX as usize];

/// `inetsw[]`: the internet protocols.
pub static INETSW: [Protosw; 13 + cfg!(feature = "inet6") as usize] = [
    Protosw {
        pr_init: Some(ip_init),
        pr_slowtimo: Some(ip_slowtimo),
        pr_flags: PR_MPSYSCTL,
        pr_sysctl: Some(ip_sysctl),
        ..Protosw::new(&INETDOMAIN)
    },
    Protosw {
        pr_type: SOCK_DGRAM as i16,
        pr_protocol: IPPROTO_UDP as i16,
        pr_flags: PR_ATOMIC | PR_ADDR | PR_SPLICE | PR_MPINPUT | PR_MPSYSCTL,
        pr_input: Some(udp_input),
        pr_ctlinput: Some(udp_ctlinput),
        pr_ctloutput: Some(ip_ctloutput),
        pr_usrreqs: Some(&UDP_USRREQS),
        pr_init: Some(udp_init),
        pr_sysctl: Some(udp_sysctl),
        ..Protosw::new(&INETDOMAIN)
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
        pr_ctlinput: Some(tcp_ctlinput),
        pr_ctloutput: Some(tcp_ctloutput),
        pr_usrreqs: Some(&TCP_USRREQS),
        pr_init: Some(tcp_init),
        pr_slowtimo: Some(tcp_slowtimo),
        pr_sysctl: Some(tcp_sysctl),
        ..Protosw::new(&INETDOMAIN)
    },
    Protosw {
        pr_type: SOCK_RAW as i16,
        pr_protocol: IPPROTO_RAW as i16,
        pr_flags: PR_ATOMIC | PR_ADDR | PR_MPINPUT,
        pr_input: Some(rip_input),
        pr_ctloutput: Some(rip_ctloutput),
        pr_usrreqs: Some(&RIP_USRREQS),
        ..Protosw::new(&INETDOMAIN)
    },
    Protosw {
        pr_type: SOCK_RAW as i16,
        pr_protocol: IPPROTO_ICMP as i16,
        pr_flags: PR_ATOMIC | PR_ADDR | PR_MPSYSCTL,
        pr_input: Some(icmp_input),
        pr_ctloutput: Some(rip_ctloutput),
        pr_usrreqs: Some(&RIP_USRREQS),
        pr_init: Some(icmp_init),
        pr_sysctl: Some(icmp_sysctl),
        ..Protosw::new(&INETDOMAIN)
    },
    Protosw {
        pr_type: SOCK_RAW as i16,
        pr_protocol: IPPROTO_IPV4 as i16,
        pr_flags: PR_ATOMIC | PR_ADDR | PR_MPSYSCTL,
        // NGIF > 0: in_gif_input; not configured.
        pr_input: Some(ipip_input),
        pr_ctloutput: Some(rip_ctloutput),
        pr_usrreqs: Some(&RIP_USRREQS),
        pr_sysctl: Some(ipip_sysctl),
        pr_init: Some(ipip_init),
        ..Protosw::new(&INETDOMAIN)
    },
    #[cfg(feature = "inet6")]
    Protosw {
        pr_type: SOCK_RAW as i16,
        pr_protocol: IPPROTO_IPV6 as i16,
        pr_flags: PR_ATOMIC | PR_ADDR,
        // NGIF > 0: in_gif_input; not configured.
        pr_input: Some(ipip_input),
        pr_ctloutput: Some(rip_ctloutput),
        pr_usrreqs: Some(&RIP_USRREQS), // XXX
        ..Protosw::new(&INETDOMAIN)
    },
    // MPLS && NGIF > 0: IPPROTO_MPLS through in_gif_input; not configured.
    Protosw {
        pr_type: SOCK_RAW as i16,
        pr_protocol: IPPROTO_IGMP as i16,
        pr_flags: PR_ATOMIC | PR_ADDR | PR_MPSYSCTL,
        pr_input: Some(igmp_input),
        pr_ctloutput: Some(rip_ctloutput),
        pr_usrreqs: Some(&RIP_USRREQS),
        pr_init: Some(igmp_init),
        pr_fasttimo: Some(igmp_fasttimo),
        pr_slowtimo: Some(igmp_slowtimo),
        pr_sysctl: Some(igmp_sysctl),
        ..Protosw::new(&INETDOMAIN)
    },
    Protosw {
        pr_type: SOCK_RAW as i16,
        pr_protocol: IPPROTO_AH as i16,
        pr_flags: PR_ATOMIC | PR_ADDR | PR_MPSYSCTL,
        pr_input: Some(ah46_input),
        pr_ctlinput: Some(ah4_ctlinput),
        pr_ctloutput: Some(rip_ctloutput),
        pr_usrreqs: Some(&RIP_USRREQS),
        pr_sysctl: Some(ah_sysctl),
        ..Protosw::new(&INETDOMAIN)
    },
    Protosw {
        pr_type: SOCK_RAW as i16,
        pr_protocol: IPPROTO_ESP as i16,
        pr_flags: PR_ATOMIC | PR_ADDR | PR_MPSYSCTL,
        pr_input: Some(esp46_input),
        pr_ctlinput: Some(esp4_ctlinput),
        pr_ctloutput: Some(rip_ctloutput),
        pr_usrreqs: Some(&RIP_USRREQS),
        pr_sysctl: Some(esp_sysctl),
        ..Protosw::new(&INETDOMAIN)
    },
    Protosw {
        pr_type: SOCK_RAW as i16,
        pr_protocol: IPPROTO_IPCOMP as i16,
        pr_flags: PR_ATOMIC | PR_ADDR | PR_MPSYSCTL,
        pr_input: Some(ipcomp46_input),
        pr_ctloutput: Some(rip_ctloutput),
        pr_usrreqs: Some(&RIP_USRREQS),
        pr_sysctl: Some(ipcomp_sysctl),
        ..Protosw::new(&INETDOMAIN)
    },
    // NGRE > 0: IPPROTO_GRE; NCARP > 0: IPPROTO_CARP; neither configured.
    Protosw {
        pr_type: SOCK_RAW as i16,
        pr_protocol: IPPROTO_PFSYNC as i16,
        pr_flags: PR_ATOMIC | PR_ADDR | PR_MPSYSCTL,
        pr_input: Some(pfsync_input4),
        pr_ctloutput: Some(rip_ctloutput),
        pr_usrreqs: Some(&RIP_USRREQS),
        pr_sysctl: Some(pfsync_sysctl),
        ..Protosw::new(&INETDOMAIN)
    },
    Protosw {
        pr_type: SOCK_RAW as i16,
        pr_protocol: IPPROTO_DIVERT as i16,
        pr_flags: PR_ATOMIC | PR_ADDR | PR_MPSYSCTL,
        pr_ctloutput: Some(rip_ctloutput),
        pr_usrreqs: Some(&DIVERT_USRREQS),
        pr_init: Some(divert_init),
        pr_sysctl: Some(divert_sysctl),
        ..Protosw::new(&INETDOMAIN)
    },
    // NETHERIP > 0: IPPROTO_ETHERIP; not configured.
    Protosw {
        // raw wildcard
        pr_type: SOCK_RAW as i16,
        pr_flags: PR_ATOMIC | PR_ADDR | PR_MPINPUT,
        pr_input: Some(rip_input),
        pr_ctloutput: Some(rip_ctloutput),
        pr_usrreqs: Some(&RIP_USRREQS),
        pr_init: Some(rip_init),
        ..Protosw::new(&INETDOMAIN)
    },
];

/// `inetdomain`.
pub static INETDOMAIN: Domain = Domain {
    dom_family: AF_INET as i32,
    dom_name: b"inet",
    dom_init: Some(in_init),
    dom_externalize: None,
    dom_dispose: None,
    dom_protosw: &INETSW,
    dom_sasize: size_of::<SockaddrIn>() as u32,
    dom_rtoffset: offset_of!(SockaddrIn, sin_addr) as u32,
    dom_maxplen: 32,
};
/* </CODE> */
