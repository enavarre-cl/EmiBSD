/*	$OpenBSD: nd6_nbr.c,v 1.165 2025/09/16 09:52:49 florian Exp $	*/
/*	$KAME: nd6_nbr.c,v 1.61 2001/02/10 16:06:14 jinmei Exp $	*/
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
/* </LICENSES> */

//! Neighbor solicitations and advertisements, and Duplicate Address Detection:
//! `netinet6/nd6_nbr.c`.
//!
//! Upstream: sys/netinet6/nd6_nbr.c @ 3ce1f3f79392
//!
//! Status: skeleton from the INET6 foundation step: the globals are defined, every
//! function has its final signature and a placeholder body that reports itself through
//! `unported!` until the file is ported.
//!
//! ## Deviations
//! - None yet: the file is a skeleton (see `Status`).

use crate::net::if_dl::SockaddrDl;
use crate::net::if_var::Ifaddr;
use crate::net::if_var::Ifnet;
use crate::netinet::if_ether::ETHER_ADDR_LEN;
use crate::netinet6::in6::In6Addr;
use crate::sys::mbuf::Mbuf;

/// `nd6_ns_input`: a received neighbor solicitation (`icmp6len` bytes at `off` of `m`,
/// consumed).
pub fn nd6_ns_input(m: &'static Mbuf, off: i32, icmp6len: i32) {
    let _ = (m, off, icmp6len);
    let _ = crate::unported!("nd6_ns_input: placeholder");
}

/// `nd6_ns_output`: sends a neighbor solicitation for `taddr6` to `daddr6` (`None`:
/// the solicited-node multicast address) from `saddr6` (`None`: chosen); `dad`: a DAD
/// probe (from `::`).
pub fn nd6_ns_output(
    ifp: &'static Ifnet,
    daddr6: Option<&In6Addr>,
    taddr6: &In6Addr,
    saddr6: Option<&In6Addr>,
    dad: bool,
) {
    let _ = (ifp, daddr6, taddr6, saddr6, dad);
    let _ = crate::unported!("nd6_ns_output: placeholder");
}

/// `nd6_na_input`: a received neighbor advertisement (`icmp6len` bytes at `off` of
/// `m`, consumed).
pub fn nd6_na_input(m: &'static Mbuf, off: i32, icmp6len: i32) {
    let _ = (m, off, icmp6len);
    let _ = crate::unported!("nd6_na_input: placeholder");
}

/// `nd6_na_output`: sends a neighbor advertisement for `taddr6` to `daddr6` with the
/// `ND_NA_FLAG_*` `flags`; `tlladdr`: include the target link-layer address (that of
/// `sdl0` for a proxy, else the interface's).
pub fn nd6_na_output(
    ifp: &'static Ifnet,
    daddr6: &In6Addr,
    taddr6: &In6Addr,
    flags: u64,
    tlladdr: bool,
    sdl0: Option<&SockaddrDl>,
) {
    let _ = (ifp, daddr6, taddr6, flags, tlladdr, sdl0);
    let _ = crate::unported!("nd6_na_output: placeholder");
}

/// `nd6_ifptomac`: the link-layer address of `ifp` for ND options (Ethernet-like
/// interfaces only).
pub fn nd6_ifptomac(ifp: &Ifnet) -> Option<[u8; ETHER_ADDR_LEN]> {
    let _ = ifp;
    let _ = crate::unported!("nd6_ifptomac: placeholder");
    None
}

/// `nd6_dad_start`: starts Duplicate Address Detection of a tentative address.
pub fn nd6_dad_start(ifa: &'static Ifaddr) {
    let _ = ifa;
    let _ = crate::unported!("nd6_dad_start: placeholder");
}

/// `nd6_dad_stop`: stops Duplicate Address Detection of an address.
pub fn nd6_dad_stop(ifa: &'static Ifaddr) {
    let _ = ifa;
    let _ = crate::unported!("nd6_dad_stop: placeholder");
}
