/*	$OpenBSD: nd6_rtr.c,v 1.176 2025/07/08 00:47:41 jsg Exp $	*/
/*	$KAME: nd6_rtr.c,v 1.97 2001/02/07 11:09:13 itojun Exp $	*/
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

//! Router advertisements and solicitations seen by a host, and route flushing:
//! `netinet6/nd6_rtr.c`.
//!
//! Upstream: sys/netinet6/nd6_rtr.c @ 3ce1f3f79392
//!
//! Status: skeleton from the INET6 foundation step: the globals are defined, every
//! function has its final signature and a placeholder body that reports itself through
//! `unported!` until the file is ported.
//!
//! ## Deviations
//! - None yet: the file is a skeleton (see `Status`).

use crate::net::if_var::Ifnet;
use crate::netinet6::in6::In6Addr;
use crate::sys::errno::Errno;
use crate::sys::mbuf::Mbuf;

/// `nd6_rtr_cache`: caches the link-layer address of the router that sent the router
/// solicitation or advertisement of `icmp6_type` (`icmp6len` bytes at `off` of `m`).
pub fn nd6_rtr_cache(m: &'static Mbuf, off: i32, icmp6len: i32, icmp6_type: u8) {
    let _ = (m, off, icmp6len, icmp6_type);
    let _ = crate::unported!("nd6_rtr_cache: placeholder");
}

/// `rt6_flush`: removes the routes of `ifp` through `gateway`.
pub fn rt6_flush(gateway: &In6Addr, ifp: &Ifnet) -> Result<(), Errno> {
    let _ = (gateway, ifp);
    Err(crate::unported!("rt6_flush: placeholder"))
}
