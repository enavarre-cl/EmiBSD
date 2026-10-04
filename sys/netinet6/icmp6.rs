/*	$OpenBSD: icmp6.c,v 1.281 2025/11/12 19:11:10 bluhm Exp $	*/
/*	$KAME: icmp6.c,v 1.217 2001/06/20 15:03:29 jinmei Exp $	*/
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
 * Copyright (c) 1982, 1986, 1988, 1993
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
 *	@(#)ip_icmp.c	8.2 (Berkeley) 1/4/94
 */
/* </LICENSES> */

//! ICMPv6: error generation, input processing (echo, MLD and ND dispatch, errors to
//! the upper layers), reflection, redirects, path MTU discovery and the
//! `net.inet6.icmp6` sysctls: `netinet6/icmp6.c`.
//!
//! Upstream: sys/netinet6/icmp6.c @ 3ce1f3f79392
//!
//! `icmp6counters` (`struct cpumem *`) is the static array of atomics
//! [`ICMP6COUNTERS`] (`netinet/icmp6.rs`'s `icmp6stat_inc` bumps it).
//!
//! Status: skeleton from the INET6 foundation step: the globals are defined, every
//! function has its final signature and a placeholder body that reports itself through
//! `unported!` until the file is ported.
//!
//! ## Deviations
//! - None yet: the file is a skeleton (see `Status`).

use crate::net::if_var::Netstack;
use crate::net::route::Rtentry;
use crate::net::route::RttimerQueue;
use crate::netinet::icmp6::ICP6S_NCOUNTERS;
use crate::netinet6::in6::SockaddrIn6;
use crate::netinet6::ip6protosw::Ip6ctlparam;
use crate::sys::errno::Errno;
use crate::sys::mbuf::Mbuf;
use crate::sys::socketvar::Socket;
use core::sync::atomic::AtomicU64;

/// `void (*)(struct sockaddr_in6 *, u_int)`: a path MTU change callback
/// (`icmp6_mtudisc_callback_register`): the destination and the routing table.
pub type Icmp6MtudiscCallbackFn = fn(&SockaddrIn6, u32);

/// `icmp6counters`: the ICMPv6 statistics.
pub static ICMP6COUNTERS: [AtomicU64; ICP6S_NCOUNTERS] =
    [const { AtomicU64::new(0) }; ICP6S_NCOUNTERS];

/// `icmp6_mtudisc_timeout_q`: the routes cloned for path MTU discovery.
pub static ICMP6_MTUDISC_TIMEOUT_Q: RttimerQueue = RttimerQueue::new();
/// `icmp6_redirect_timeout_q`: the routes created by redirects.
pub static ICMP6_REDIRECT_TIMEOUT_Q: RttimerQueue = RttimerQueue::new();

/// `icmp6_init`: initializes the counters and timeout queues.
pub fn icmp6_init() {
    let _ = crate::unported!("icmp6_init: placeholder");
}

/// `icmp6_mtudisc_callback_register`: registers a function to call on path MTU changes.
pub fn icmp6_mtudisc_callback_register(func: Icmp6MtudiscCallbackFn) {
    let _ = func;
    let _ = crate::unported!("icmp6_mtudisc_callback_register: placeholder");
}

/// `icmp6_do_error`: builds the ICMPv6 error of `type_`/`code` (`param`: pointer or
/// MTU) about packet `m`, which it consumes; `None` when no error is sent.
pub fn icmp6_do_error(m: &'static Mbuf, type_: u8, code: u8, param: i32) -> Option<&'static Mbuf> {
    let _ = (m, type_, code, param);
    let _ = crate::unported!("icmp6_do_error: placeholder");
    None
}

/// `icmp6_error`: sends the ICMPv6 error of `type_`/`code` about `m` (consumed).
pub fn icmp6_error(m: &'static Mbuf, type_: u8, code: u8, param: i32) {
    let _ = (m, type_, code, param);
    let _ = crate::unported!("icmp6_error: placeholder");
}

/// `icmp6_input`: ICMPv6's `pr_input`.
pub fn icmp6_input(
    mp: &mut Option<&'static Mbuf>,
    offp: &mut i32,
    proto: i32,
    af: i32,
    ns: Option<&Netstack>,
) -> i32 {
    let _ = (mp, offp, proto, af, ns);
    let _ = crate::unported!("icmp6_input: placeholder");
    crate::netinet::in_::IPPROTO_DONE
}

/// `icmp6_mtudisc_update`: a validated (or not) Packet Too Big: updates the path MTU
/// of the destination in `ip6cp`.
pub fn icmp6_mtudisc_update(ip6cp: &Ip6ctlparam, validated: bool) {
    let _ = (ip6cp, validated);
    let _ = crate::unported!("icmp6_mtudisc_update: placeholder");
}

/// `icmp6_reflect`: turns the ICMPv6 message at offset `off` of `*mp` around to its
/// source (source address from `sa` if given); consumes the packet.
pub fn icmp6_reflect(
    mp: &mut Option<&'static Mbuf>,
    off: usize,
    sa: Option<&SockaddrIn6>,
) -> Result<(), Errno> {
    let _ = (mp, off, sa);
    Err(crate::unported!("icmp6_reflect: placeholder"))
}

/// `icmp6_fasttimo`: the ICMPv6 fast timeout (MLD timers).
pub fn icmp6_fasttimo() {
    let _ = crate::unported!("icmp6_fasttimo: placeholder");
}

/// `icmp6_redirect_input`: processes a received redirect at offset `off` of `m`
/// (consumed).
pub fn icmp6_redirect_input(m: &'static Mbuf, off: i32) {
    let _ = (m, off);
    let _ = crate::unported!("icmp6_redirect_input: placeholder");
}

/// `icmp6_redirect_output`: sends a redirect about forwarded packet `m0` (consumed)
/// along route `rt`.
pub fn icmp6_redirect_output(m0: &'static Mbuf, rt: &'static Rtentry) {
    let _ = (m0, rt);
    let _ = crate::unported!("icmp6_redirect_output: placeholder");
}

/// `icmp6_ctloutput`: the ICMPv6-level socket options (`ICMP6_FILTER`).
pub fn icmp6_ctloutput(
    op: i32,
    so: &'static Socket,
    level: i32,
    optname: i32,
    m: Option<&'static Mbuf>,
) -> Result<(), Errno> {
    let _ = (op, so, level, optname, m);
    Err(crate::unported!("icmp6_ctloutput: placeholder"))
}

/// `icmp6_mtudisc_clone`: a host route to `dst` to hold a learned path MTU (cloned if
/// needed); `ipsec`: for an IPsec SA's destination.
pub fn icmp6_mtudisc_clone(
    dst: &SockaddrIn6,
    rtableid: u32,
    ipsec: bool,
) -> Option<&'static Rtentry> {
    let _ = (dst, rtableid, ipsec);
    let _ = crate::unported!("icmp6_mtudisc_clone: placeholder");
    None
}

/// `icmp6_sysctl`: the `net.inet6.icmp6` sysctls.
pub fn icmp6_sysctl(
    name: &[i32],
    oldp: usize,
    oldlenp: &mut usize,
    newp: usize,
    newlen: usize,
) -> Result<(), Errno> {
    let _ = (name, oldp, oldlenp, newp, newlen);
    Err(crate::unported!("icmp6_sysctl: placeholder"))
}
