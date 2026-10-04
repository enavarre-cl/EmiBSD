/*	$OpenBSD: in6_src.c,v 1.104 2025/07/18 08:39:14 mvs Exp $	*/
/*	$KAME: in6_src.c,v 1.36 2001/02/06 04:08:17 itojun Exp $	*/
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
 * Copyright (c) 1982, 1986, 1991, 1993
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
 *	@(#)in_pcb.c	8.2 (Berkeley) 1/4/94
 */
/* </LICENSES> */

//! IPv6 source address and route selection, the hop limit of a pcb, and the
//! embedding of scope zone ids in addresses: `netinet6/in6_src.c`.
//!
//! Upstream: sys/netinet6/in6_src.c @ 3ce1f3f79392
//!
//! Status: skeleton from the INET6 foundation step: the globals are defined, every
//! function has its final signature and a placeholder body that reports itself through
//! `unported!` until the file is ported.
//!
//! ## Deviations
//! - None yet: the file is a skeleton (see `Status`).

use crate::net::route::Route;
use crate::net::route::Rtentry;
use crate::netinet::in_pcb::Inpcb;
use crate::netinet6::in6::{In6Addr, SockaddrIn6};
use crate::netinet6::ip6_var::{Ip6Moptions, Ip6Pktopts};
use crate::sys::errno::Errno;

/// `in6_pcbselsrc`: the source address for `inp` sending to `dstsock` with options
/// `opts`, stored in `in6src` (the C returns a pointer to it).
pub fn in6_pcbselsrc(
    in6src: &mut In6Addr,
    dstsock: &SockaddrIn6,
    inp: &Inpcb,
    opts: Option<&Ip6Pktopts>,
) -> Result<(), Errno> {
    let _ = (in6src, dstsock, inp, opts);
    Err(crate::unported!("in6_pcbselsrc: placeholder"))
}

/// `in6_selectsrc`: the source address for a socketless sender (the multicast options
/// `mopts`, routing table `rtableid`), stored in `in6src`.
pub fn in6_selectsrc(
    in6src: &mut In6Addr,
    dstsock: &SockaddrIn6,
    mopts: Option<&Ip6Moptions>,
    rtableid: u32,
) -> Result<(), Errno> {
    let _ = (in6src, dstsock, mopts, rtableid);
    Err(crate::unported!("in6_selectsrc: placeholder"))
}

/// `in6_selectroute`: the route to `dst` (the next hop of `opts` if any), cached in `ro`.
pub fn in6_selectroute(
    dst: &In6Addr,
    opts: Option<&Ip6Pktopts>,
    ro: &Route,
    rtableid: u32,
) -> Option<&'static Rtentry> {
    let _ = (dst, opts, ro, rtableid);
    let _ = crate::unported!("in6_selectroute: placeholder");
    None
}

/// `in6_selecthlim`: the hop limit for packets of `inp` (`inp_hops`, the interface's or
/// `ip6_defhlim`).
pub fn in6_selecthlim(inp: &Inpcb) -> i32 {
    let _ = inp;
    let _ = crate::unported!("in6_selecthlim: placeholder");
    0
}

/// `in6_embedscope`: copies the address of `sin6` into `in6` with its scope zone
/// embedded (from `sin6_scope_id`, the packet info of `outputopts`, or the multicast
/// interface of `moptions`).
pub fn in6_embedscope(
    in6: &mut In6Addr,
    sin6: &SockaddrIn6,
    outputopts: Option<&Ip6Pktopts>,
    moptions: Option<&Ip6Moptions>,
) -> Result<(), Errno> {
    let _ = (in6, sin6, outputopts, moptions);
    Err(crate::unported!("in6_embedscope: placeholder"))
}

/// `in6_recoverscope`: fills `sin6` from `in6`, moving an embedded scope zone into
/// `sin6_scope_id`.
pub fn in6_recoverscope(sin6: &mut SockaddrIn6, in6: &In6Addr) {
    let _ = (sin6, in6);
    let _ = crate::unported!("in6_recoverscope: placeholder");
}

/// `in6_clearscope`: clears the embedded scope zone of `addr`.
pub fn in6_clearscope(addr: &mut In6Addr) {
    let _ = addr;
    let _ = crate::unported!("in6_clearscope: placeholder");
}
