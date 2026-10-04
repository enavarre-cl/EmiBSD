/*	$OpenBSD: ip6_output.c,v 1.309 2026/09/17 15:56:59 bluhm Exp $	*/
/*	$KAME: ip6_output.c,v 1.172 2001/03/25 09:55:56 itojun Exp $	*/
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
 * Copyright (c) 1982, 1986, 1988, 1990, 1993
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
 *	@(#)ip_output.c	8.3 (Berkeley) 1/21/94
 */
/* </LICENSES> */

//! IPv6 output: the extension headers, fragmentation, path MTU, the socket options of
//! the IPv6 level, multicast loopback, checksum offload and the IPsec output path:
//! `netinet6/ip6_output.c`.
//!
//! Upstream: sys/netinet6/ip6_output.c @ 3ce1f3f79392
//!
//! Missing infrastructure: `ip6_id_ctx` is a `struct idgen32_ctx` of
//! `crypto/idgen.c`, which is not ported yet; the port of this file brings it or
//! records the gap.
//!
//! Status: skeleton from the INET6 foundation step: the globals are defined, every
//! function has its final signature and a placeholder body that reports itself through
//! `unported!` until the file is ported.
//!
//! ## Deviations
//! - None yet: the file is a skeleton (see `Status`).

use crate::net::if_var::Ifnet;
use crate::net::route::Route;
use crate::netinet::ip_ipsp::{IpsecLevel, Tdb};
use crate::netinet::ip_spd::SpdError;
use crate::netinet6::in6::SockaddrIn6;
use crate::netinet6::ip6_var::{Ip6Moptions, Ip6Pktopts};
use crate::sys::errno::Errno;
use crate::sys::mbuf::Mbuf;
use crate::sys::mbuf::MbufList;
use crate::sys::socketvar::Socket;
use core::ptr::NonNull;

/// `ip6_output`: sends the IPv6 packet `m0` (header filled in up to the addresses)
/// with packet options `opt`, cached route `ro`, `IPV6_*` output flags, multicast
/// options `im6o` and the socket's IPsec level. Consumes the packet.
pub fn ip6_output(
    m0: &'static Mbuf,
    opt: Option<&Ip6Pktopts>,
    ro: Option<&Route>,
    flags: i32,
    im6o: Option<&Ip6Moptions>,
    seclevel: Option<&IpsecLevel>,
) -> Result<(), Errno> {
    let _ = (m0, opt, ro, flags, im6o, seclevel);
    Err(crate::unported!("ip6_output: placeholder"))
}

/// `ip6_fragment`: fragments `m0` (unfragmentable part `hlen` bytes, next header
/// `nextproto`) to `mtu` into `ml`. Consumes the packet; on error `ml` is purged.
pub fn ip6_fragment(
    m0: &'static Mbuf,
    ml: &MbufList,
    hlen: i32,
    nextproto: u8,
    mtu: u64,
) -> Result<(), Errno> {
    let _ = (m0, ml, hlen, nextproto, mtu);
    Err(crate::unported!("ip6_fragment: placeholder"))
}

/// `ip6_ctloutput`: the IPv6-level socket options.
pub fn ip6_ctloutput(
    op: i32,
    so: &'static Socket,
    level: i32,
    optname: i32,
    m: Option<&'static Mbuf>,
) -> Result<(), Errno> {
    let _ = (op, so, level, optname, m);
    Err(crate::unported!("ip6_ctloutput: placeholder"))
}

/// `ip6_raw_ctloutput`: the raw-socket IPv6 options (`IPV6_CHECKSUM`).
pub fn ip6_raw_ctloutput(
    op: i32,
    so: &'static Socket,
    level: i32,
    optname: i32,
    m: Option<&'static Mbuf>,
) -> Result<(), Errno> {
    let _ = (op, so, level, optname, m);
    Err(crate::unported!("ip6_raw_ctloutput: placeholder"))
}

/// `ip6_initpktopts`: initializes packet options to their defaults.
pub fn ip6_initpktopts(opt: &mut Ip6Pktopts) {
    let _ = opt;
    let _ = crate::unported!("ip6_initpktopts: placeholder");
}

/// `ip6_setpktopts`: sets the packet options of the control messages in `control`
/// into `opt`, starting from the sticky options `stickyopt`; `priv_`: privileged socket,
/// `uproto`: the upper-layer protocol.
pub fn ip6_setpktopts(
    control: &Mbuf,
    opt: &mut Ip6Pktopts,
    stickyopt: Option<&Ip6Pktopts>,
    priv_: bool,
    uproto: i32,
) -> Result<(), Errno> {
    let _ = (control, opt, stickyopt, priv_, uproto);
    Err(crate::unported!("ip6_setpktopts: placeholder"))
}

/// `ip6_clearpktopts`: frees and clears option `optname` of `pktopt` (-1: all of them).
pub fn ip6_clearpktopts(pktopt: &mut Ip6Pktopts, optname: i32) {
    let _ = (pktopt, optname);
    let _ = crate::unported!("ip6_clearpktopts: placeholder");
}

/// `ip6_freepcbopts`: frees a pcb's packet options.
///
/// # Safety
///
/// `pktopt` is `None` or a `malloc(9)` allocation of the socket being torn down,
/// not used again.
pub unsafe fn ip6_freepcbopts(pktopt: Option<NonNull<Ip6Pktopts>>) {
    let _ = pktopt;
    let _ = crate::unported!("ip6_freepcbopts: placeholder");
}

/// `ip6_freemoptions`: frees a pcb's multicast options and leaves their groups.
///
/// # Safety
///
/// `im6o` is `None` or a `malloc(9)` allocation of the socket being torn down,
/// not used again.
pub unsafe fn ip6_freemoptions(im6o: Option<NonNull<Ip6Moptions>>) {
    let _ = im6o;
    let _ = crate::unported!("ip6_freemoptions: placeholder");
}

/// `ip6_mloopback`: loops a copy of multicast packet `m` back to `ifp` (destination
/// `dst`).
pub fn ip6_mloopback(ifp: &'static Ifnet, m: &Mbuf, dst: &SockaddrIn6) {
    let _ = (ifp, m, dst);
    let _ = crate::unported!("ip6_mloopback: placeholder");
}

/// `ip6_randomid_init`: seeds the IPv6 fragment identification generator.
pub fn ip6_randomid_init() {
    let _ = crate::unported!("ip6_randomid_init: placeholder");
}

/// `ip6_randomid`: a random fragment identification.
pub fn ip6_randomid() -> u32 {
    let _ = crate::unported!("ip6_randomid: placeholder");
    0
}

/// `in6_proto_cksum_out`: computes or offloads the upper-layer checksum of `m` for
/// output on `ifp`.
pub fn in6_proto_cksum_out(m: &Mbuf, ifp: Option<&Ifnet>) {
    let _ = (m, ifp);
    let _ = crate::unported!("in6_proto_cksum_out: placeholder");
}

/// `ip6_output_ipsec_lookup`: the SA the policy wants applied to `m`, with a reference;
/// `None` when no IPsec is needed.
pub fn ip6_output_ipsec_lookup(
    m: &Mbuf,
    seclevel: Option<&IpsecLevel>,
) -> Result<Option<&'static Tdb>, SpdError> {
    let _ = (m, seclevel);
    let _ = crate::unported!("ip6_output_ipsec_lookup: placeholder");
    Ok(None)
}

/// `ip6_output_ipsec_send`: hands `m` to the SA `tdb` (after the path MTU check);
/// `tunalready`: already tunneled, `fwd`: forwarded. Consumes the packet.
pub fn ip6_output_ipsec_send(
    tdb: &'static Tdb,
    m: &'static Mbuf,
    ro: Option<&Route>,
    tunalready: u32,
    fwd: bool,
) -> Result<(), Errno> {
    let _ = (tdb, m, ro, tunalready, fwd);
    Err(crate::unported!("ip6_output_ipsec_send: placeholder"))
}
