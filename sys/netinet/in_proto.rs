/*	$OpenBSD: in_proto.c,v 1.127 2025/07/19 16:40:40 mvs Exp $	*/
/*	$NetBSD: in_proto.c,v 1.14 1996/02/18 18:58:32 christos Exp $	*/
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

//! The TCP/IP protocol family: the protocol switch `inetsw[]`, `ip_protox[]` and
//! `inetdomain`.
//!
//! Upstream: sys/netinet/in_proto.c @ 3ce1f3f79392
//!
//! IP, ICMP, UDP, TCP, raw IP, IP-in-IP and IGMP, in the C's order. `ip_init` fills
//! `ip_protox[]`, which maps an IP protocol number to its entry in `inetsw[]`; every number
//! without an entry goes to the raw IP handler.
//!
//! Status: `ported` (M7b). The licence block carries the NRL notice with its advertising
//! clause, accepted as BSD-4 (`.claude/rules/scope-and-stubs.md`).
//!
//! ## Deviations
//! - The protocols whose files are not ported keep their entries, with stand-ins in this
//!   module named after the C functions: `udp_*` (`netinet/udp_usrreq.c`), `tcp_*`
//!   (`netinet/tcp_*.c`), `rip_*` (`netinet/raw_ip.c`), `ipip_*` (`netinet/ip_ipip.c`),
//!   `igmp_*` (`netinet/igmp.c`) and `in_init` (`netinet/in_pcb.c`). Each reports itself with
//!   `unported!`; an input stand-in drops the packet (`IPPROTO_DONE`), a sysctl one fails with
//!   `ENOSYS`. Raw IP sockets need the socket layer, so `rip_input` stays a stand-in even
//!   where `icmp_input` passes a message on to it.
//! - `pr_ctloutput` and `pr_usrreqs` are not members here (`sys/protosw.rs`).
//! - `NGIF` is 0 (`ipip_input` serves `IPPROTO_IPV4`); `INET6`, `MPLS`, `IPSEC`, `NGRE`,
//!   `NCARP`, `NPFSYNC`, `NPF` and `NETHERIP` are not configured: their entries are comments.
//!   `SMALL_KERNEL` is not set, so the sysctl handlers are in the table.
//! - `ip_protox[]` holds atomics (`ip_init` writes it once, every input reads it).

use core::ffi::c_void;
use core::mem::{offset_of, size_of};
use core::sync::atomic::AtomicU8;

use crate::net::if_var::Netstack;
use crate::netinet::in_::{
    IPPROTO_DONE, IPPROTO_ICMP, IPPROTO_IGMP, IPPROTO_IPV4, IPPROTO_MAX, IPPROTO_RAW, IPPROTO_TCP,
    IPPROTO_UDP, SockaddrIn,
};
use crate::netinet::ip_icmp::{icmp_init, icmp_input, icmp_sysctl};
use crate::netinet::ip_input::{ip_init, ip_slowtimo, ip_sysctl};
use crate::sys::domain::Domain;
use crate::sys::errno::Errno;
use crate::sys::mbuf::{Mbuf, m_freemp};
use crate::sys::protosw::{
    PR_ABRTACPTDIS, PR_ADDR, PR_ATOMIC, PR_CONNREQUIRED, PR_MPINPUT, PR_MPSYSCTL, PR_SPLICE,
    PR_WANTRCVD, Protosw,
};
use crate::sys::socket::{AF_INET, SOCK_DGRAM, SOCK_RAW, SOCK_STREAM, Sockaddr};
use crate::unported;

/// `ip_protox[]`: IP protocol number to `inetsw[]` index.
pub static IP_PROTOX: [AtomicU8; IPPROTO_MAX as usize] =
    [const { AtomicU8::new(0) }; IPPROTO_MAX as usize];

/// `inetsw[]`: the internet protocols.
pub static INETSW: [Protosw; 8] = [
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
        ..Protosw::new(&INETDOMAIN)
    },
    Protosw {
        pr_type: SOCK_RAW as i16,
        pr_protocol: IPPROTO_ICMP as i16,
        pr_flags: PR_ATOMIC | PR_ADDR | PR_MPSYSCTL,
        pr_input: Some(icmp_input),
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
        pr_sysctl: Some(ipip_sysctl),
        pr_init: Some(ipip_init),
        ..Protosw::new(&INETDOMAIN)
    },
    // INET6: IPPROTO_IPV6 through ipip_input; not configured.
    // MPLS && NGIF > 0: IPPROTO_MPLS through in_gif_input; not configured.
    Protosw {
        pr_type: SOCK_RAW as i16,
        pr_protocol: IPPROTO_IGMP as i16,
        pr_flags: PR_ATOMIC | PR_ADDR | PR_MPSYSCTL,
        pr_input: Some(igmp_input),
        pr_init: Some(igmp_init),
        pr_fasttimo: Some(igmp_fasttimo),
        pr_slowtimo: Some(igmp_slowtimo),
        pr_sysctl: Some(igmp_sysctl),
        ..Protosw::new(&INETDOMAIN)
    },
    // IPSEC: IPPROTO_AH, IPPROTO_ESP, IPPROTO_IPCOMP; not configured.
    // NGRE > 0: IPPROTO_GRE; NCARP > 0: IPPROTO_CARP; NPFSYNC > 0: IPPROTO_PFSYNC;
    // NPF > 0: IPPROTO_DIVERT; NETHERIP > 0: IPPROTO_ETHERIP; none configured.
    Protosw {
        // raw wildcard
        pr_type: SOCK_RAW as i16,
        pr_flags: PR_ATOMIC | PR_ADDR | PR_MPINPUT,
        pr_input: Some(rip_input),
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

/// The end of an input stand-in, after the report: the packet is dropped.
fn unported_input(mp: &mut Option<&'static Mbuf>) -> i32 {
    m_freemp(mp);
    IPPROTO_DONE
}

/// `in_init` (`netinet/in_pcb.c`, not ported): the `inpcb` pool.
fn in_init() {
    let _ = unported!("in_init (netinet/in_pcb.c)");
}

/// `udp_input` (`netinet/udp_usrreq.c`, not ported).
fn udp_input(
    mp: &mut Option<&'static Mbuf>,
    _offp: &mut i32,
    _proto: i32,
    _af: i32,
    _ns: Option<&Netstack>,
) -> i32 {
    let _ = unported!("udp_input (netinet/udp_usrreq.c)");
    unported_input(mp)
}

/// `udp_ctlinput` (`netinet/udp_usrreq.c`, not ported).
///
/// # Safety
///
/// `PrCtlinputFn`'s contract; nothing is read.
unsafe fn udp_ctlinput(_cmd: i32, _sa: *const Sockaddr, _rdomain: u32, _v: *mut c_void) {
    let _ = unported!("udp_ctlinput (netinet/udp_usrreq.c)");
}

/// `udp_init` (`netinet/udp_usrreq.c`, not ported).
fn udp_init() {
    let _ = unported!("udp_init (netinet/udp_usrreq.c)");
}

/// `udp_sysctl` (`netinet/udp_usrreq.c`, not ported).
fn udp_sysctl(
    _name: &[i32],
    _oldp: usize,
    _oldlenp: &mut usize,
    _newp: usize,
    _newlen: usize,
) -> Result<(), Errno> {
    Err(unported!("udp_sysctl (netinet/udp_usrreq.c)"))
}

/// `tcp_input` (`netinet/tcp_input.c`, not ported).
fn tcp_input(
    mp: &mut Option<&'static Mbuf>,
    _offp: &mut i32,
    _proto: i32,
    _af: i32,
    _ns: Option<&Netstack>,
) -> i32 {
    let _ = unported!("tcp_input (netinet/tcp_input.c)");
    unported_input(mp)
}

/// `tcp_ctlinput` (`netinet/tcp_subr.c`, not ported).
///
/// # Safety
///
/// `PrCtlinputFn`'s contract; nothing is read.
unsafe fn tcp_ctlinput(_cmd: i32, _sa: *const Sockaddr, _rdomain: u32, _v: *mut c_void) {
    let _ = unported!("tcp_ctlinput (netinet/tcp_subr.c)");
}

/// `tcp_init` (`netinet/tcp_subr.c`, not ported).
fn tcp_init() {
    let _ = unported!("tcp_init (netinet/tcp_subr.c)");
}

/// `tcp_slowtimo` (`netinet/tcp_timer.c`, not ported).
fn tcp_slowtimo() {
    let _ = unported!("tcp_slowtimo (netinet/tcp_timer.c)");
}

/// `tcp_sysctl` (`netinet/tcp_usrreq.c`, not ported).
fn tcp_sysctl(
    _name: &[i32],
    _oldp: usize,
    _oldlenp: &mut usize,
    _newp: usize,
    _newlen: usize,
) -> Result<(), Errno> {
    Err(unported!("tcp_sysctl (netinet/tcp_usrreq.c)"))
}

/// `rip_input` (`netinet/raw_ip.c`, not ported: raw sockets need the socket layer). Also the
/// end of `icmp_input`, which hands every message to the raw listeners.
pub fn rip_input(
    mp: &mut Option<&'static Mbuf>,
    _offp: &mut i32,
    _proto: i32,
    _af: i32,
    _ns: Option<&Netstack>,
) -> i32 {
    let _ = unported!("rip_input (netinet/raw_ip.c)");
    unported_input(mp)
}

/// `rip_init` (`netinet/raw_ip.c`, not ported).
fn rip_init() {
    let _ = unported!("rip_init (netinet/raw_ip.c)");
}

/// `ipip_input` (`netinet/ip_ipip.c`, not ported).
fn ipip_input(
    mp: &mut Option<&'static Mbuf>,
    _offp: &mut i32,
    _proto: i32,
    _af: i32,
    _ns: Option<&Netstack>,
) -> i32 {
    let _ = unported!("ipip_input (netinet/ip_ipip.c)");
    unported_input(mp)
}

/// `ipip_sysctl` (`netinet/ip_ipip.c`, not ported).
fn ipip_sysctl(
    _name: &[i32],
    _oldp: usize,
    _oldlenp: &mut usize,
    _newp: usize,
    _newlen: usize,
) -> Result<(), Errno> {
    Err(unported!("ipip_sysctl (netinet/ip_ipip.c)"))
}

/// `ipip_init` (`netinet/ip_ipip.c`, not ported).
fn ipip_init() {
    let _ = unported!("ipip_init (netinet/ip_ipip.c)");
}

/// `igmp_input` (`netinet/igmp.c`, not ported).
fn igmp_input(
    mp: &mut Option<&'static Mbuf>,
    _offp: &mut i32,
    _proto: i32,
    _af: i32,
    _ns: Option<&Netstack>,
) -> i32 {
    let _ = unported!("igmp_input (netinet/igmp.c)");
    unported_input(mp)
}

/// `igmp_init` (`netinet/igmp.c`, not ported).
fn igmp_init() {
    let _ = unported!("igmp_init (netinet/igmp.c)");
}

/// `igmp_fasttimo` (`netinet/igmp.c`, not ported).
fn igmp_fasttimo() {
    let _ = unported!("igmp_fasttimo (netinet/igmp.c)");
}

/// `igmp_slowtimo` (`netinet/igmp.c`, not ported).
fn igmp_slowtimo() {
    let _ = unported!("igmp_slowtimo (netinet/igmp.c)");
}

/// `igmp_sysctl` (`netinet/igmp.c`, not ported).
fn igmp_sysctl(
    _name: &[i32],
    _oldp: usize,
    _oldlenp: &mut usize,
    _newp: usize,
    _newlen: usize,
) -> Result<(), Errno> {
    Err(unported!("igmp_sysctl (netinet/igmp.c)"))
}
