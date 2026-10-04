/*	$OpenBSD: ip6_input.c,v 1.301 2026/09/17 15:56:59 bluhm Exp $	*/
/*	$KAME: ip6_input.c,v 1.188 2001/03/29 05:34:31 itojun Exp $	*/
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
 *	@(#)ip_input.c	8.2 (Berkeley) 1/4/94
 */
/* </LICENSES> */

//! IPv6 input: header checks, the hop-by-hop options, local delivery through the
//! protocol switch, forwarding, extension header walking, the control messages
//! of received packets and the `net.inet6.ip6` sysctls: `netinet6/ip6_input.c`.
//!
//! Upstream: sys/netinet6/ip6_input.c @ 3ce1f3f79392
//!
//! `ip6counters` (`struct cpumem *`) is the static array of atomics [`IP6COUNTERS`]
//! (`netinet6/ip6_var.rs`'s `ip6stat_inc` bumps it).
//!
//! Status: skeleton from the INET6 foundation step: the globals are defined, every
//! function has its final signature and a placeholder body that reports itself through
//! `unported!` until the file is ported.
//!
//! ## Deviations
//! - None yet: the file is a skeleton (see `Status`).

use crate::net::if_var::Ifnet;
use crate::net::if_var::{Netstack, Niqueue};
use crate::net::netisr::NETISR_IPV6;
use crate::netinet::in_pcb::Inpcb;
use crate::netinet::ip::IPQ_MAXLEN;
use crate::netinet6::ip6_var::IP6S_NCOUNTERS;
use crate::sys::errno::Errno;
use crate::sys::mbuf::Mbuf;
use crate::sys::protosw::PRC_NCMDS;
use core::sync::atomic::AtomicU64;

/// `ip6intrq`.
pub static IP6INTRQ: Niqueue = Niqueue::new(IPQ_MAXLEN as u32, NETISR_IPV6);

/// `ip6counters`: the IPv6 statistics.
pub static IP6COUNTERS: [AtomicU64; IP6S_NCOUNTERS] = [const { AtomicU64::new(0) }; IP6S_NCOUNTERS];

/// `inet6ctlerrmap[]`: the errno of each `PRC_*` control command.
pub static INET6CTLERRMAP: [Option<Errno>; PRC_NCMDS] = [
    None,
    None,
    None,
    None,
    None,
    Some(Errno::EMSGSIZE),
    Some(Errno::EHOSTDOWN),
    Some(Errno::EHOSTUNREACH),
    Some(Errno::EHOSTUNREACH),
    Some(Errno::EHOSTUNREACH),
    Some(Errno::ECONNREFUSED),
    Some(Errno::ECONNREFUSED),
    Some(Errno::EMSGSIZE),
    Some(Errno::EHOSTUNREACH),
    None,
    None,
    None,
    None,
    None,
    None,
    Some(Errno::ENOPROTOOPT),
];

/// `ip6_init`: IP6 initialization: fills in the IP6 protocol switch table; all protocols
/// not implemented in kernel go to the raw IP6 protocol handler.
pub fn ip6_init() {
    let _ = crate::unported!("ip6_init: placeholder");
}

/// `ip6intr`: the IPv6 software interrupt: delivers the packets queued on `ip6intrq`.
pub fn ip6intr() {
    let _ = crate::unported!("ip6intr: placeholder");
}

/// `ipv6_input`: an IPv6 packet from `ifp` (`ether_input`, `if_input_local`).
pub fn ipv6_input(ifp: &'static Ifnet, m: &'static Mbuf, ns: Option<&Netstack>) {
    let _ = (ifp, m, ns);
    let _ = crate::unported!("ipv6_input: placeholder");
}

/// `ipv6_check`: the header checks of `ipv6_input`; `None` when the packet was
/// dropped (and freed).
pub fn ipv6_check(ifp: &Ifnet, m: &'static Mbuf) -> Option<&'static Mbuf> {
    let _ = (ifp, m);
    let _ = crate::unported!("ipv6_check: placeholder");
    None
}

/// `ip6_input_if`: processes the IPv6 packet `*mp` received on `ifp` (`nxt` is
/// `IPPROTO_IPV6`, `af` `AF_INET6` or `AF_UNSPEC` for a forwarded one); returns the next
/// protocol or `IPPROTO_DONE`, `*mp` cleared when the packet was consumed.
pub fn ip6_input_if(
    mp: &mut Option<&'static Mbuf>,
    offp: &mut i32,
    nxt: i32,
    af: i32,
    ifp: &'static Ifnet,
    ns: Option<&Netstack>,
) -> i32 {
    let _ = (mp, offp, nxt, af, ifp, ns);
    let _ = crate::unported!("ip6_input_if: placeholder");
    crate::netinet::in_::IPPROTO_DONE
}

/// `ip6_ours_enqueue`: queues a packet for `ip6intr` (protocols that need the exclusive
/// net lock); returns `IPPROTO_DONE`.
pub fn ip6_ours_enqueue(mp: &mut Option<&'static Mbuf>, offp: &mut i32, nxt: i32) -> i32 {
    let _ = (mp, offp, nxt);
    let _ = crate::unported!("ip6_ours_enqueue: placeholder");
    crate::netinet::in_::IPPROTO_DONE
}

/// `ip6_unknown_opt`: handles the unknown option at `optp` (offset `off` of `*mp`) by its
/// action bits; `true` to skip it (the C's 0), `false` when the packet was dropped or an
/// ICMPv6 error sent (the C's -1, `*mp` cleared).
///
/// # Safety
///
/// `optp` points at the option inside `*mp`'s first mbuf (at least two readable
/// bytes).
pub unsafe fn ip6_unknown_opt(mp: &mut Option<&'static Mbuf>, optp: *const u8, off: i32) -> bool {
    let _ = (mp, optp, off);
    let _ = crate::unported!("ip6_unknown_opt: placeholder");
    false
}

/// `ip6_get_prevhdr`: the offset of the next header field that precedes offset `off`
/// (the field to rewrite when the header at `off` is removed).
pub fn ip6_get_prevhdr(m: &Mbuf, off: i32) -> i32 {
    let _ = (m, off);
    let _ = crate::unported!("ip6_get_prevhdr: placeholder");
    0
}

/// `ip6_nexthdr`: the offset of the header after the one of protocol `proto` at `off`,
/// its protocol stored in `nxtp`; `None` where the C returns -1.
pub fn ip6_nexthdr(m: &Mbuf, off: i32, proto: i32, nxtp: &mut i32) -> Option<i32> {
    let _ = (m, off, proto, nxtp);
    let _ = crate::unported!("ip6_nexthdr: placeholder");
    None
}

/// `ip6_lasthdr`: the offset of the last header (the upper-layer protocol), its protocol
/// stored in `nxtp`; `None` where the C returns -1.
pub fn ip6_lasthdr(m: &Mbuf, off: i32, proto: i32, nxtp: &mut i32) -> Option<i32> {
    let _ = (m, off, proto, nxtp);
    let _ = crate::unported!("ip6_lasthdr: placeholder");
    None
}

/// `ip6_process_hopopts`: processes the `hbhlen` bytes of hop-by-hop options at
/// `opthead` (router alert into `rtalertp`, jumbo payload length into `plenp`); `true`
/// on success (the C's 0), `false` when the packet was dropped (the C's -1).
///
/// # Safety
///
/// `opthead` points at `hbhlen` readable bytes of options inside `*mp`'s first
/// mbuf.
pub unsafe fn ip6_process_hopopts(
    mp: &mut Option<&'static Mbuf>,
    opthead: *const u8,
    hbhlen: i32,
    rtalertp: &mut u32,
    plenp: &mut u32,
) -> bool {
    let _ = (mp, opthead, hbhlen, rtalertp, plenp);
    let _ = crate::unported!("ip6_process_hopopts: placeholder");
    false
}

/// `ip6_savecontrol`: appends to `*mp` the control messages `inp` asked for about the
/// received packet `m`.
pub fn ip6_savecontrol(inp: &Inpcb, m: &Mbuf, mp: &mut Option<&'static Mbuf>) {
    let _ = (inp, m, mp);
    let _ = crate::unported!("ip6_savecontrol: placeholder");
}

/// `ip6_sysctl`: the `net.inet6.ip6` sysctls.
pub fn ip6_sysctl(
    name: &[i32],
    oldp: usize,
    oldlenp: &mut usize,
    newp: usize,
    newlen: usize,
) -> Result<(), Errno> {
    let _ = (name, oldp, oldlenp, newp, newlen);
    Err(crate::unported!("ip6_sysctl: placeholder"))
}

/// `ip6_send`: queues `m` for output by `ip6_send_dispatch` (from contexts that cannot
/// call `ip6_output` directly).
pub fn ip6_send(m: &'static Mbuf) {
    let _ = m;
    let _ = crate::unported!("ip6_send: placeholder");
}
