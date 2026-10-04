/*	$OpenBSD: frag6.c,v 1.97 2026/08/11 14:28:59 bluhm Exp $	*/
/*	$KAME: frag6.c,v 1.40 2002/05/27 21:40:31 itojun Exp $	*/
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

//! IPv6 reassembly: `netinet6/frag6.c`.
//!
//! Upstream: sys/netinet6/frag6.c @ 3ce1f3f79392
//!
//! Status: skeleton from the INET6 foundation step: the globals are defined, every
//! function has its final signature and a placeholder body that reports itself through
//! `unported!` until the file is ported.
//!
//! ## Deviations
//! - None yet: the file is a skeleton (see `Status`).

use crate::net::if_var::Netstack;
use crate::sys::errno::Errno;
use crate::sys::mbuf::Mbuf;

/// `frag6_init`: initialises the reassembly queue and pools.
pub fn frag6_init() {
    let _ = crate::unported!("frag6_init: placeholder");
}

/// `frag6_input`: the fragment header's `pr_input`: queues the fragment, and returns the
/// next header of the reassembled packet once complete (`IPPROTO_DONE` otherwise).
pub fn frag6_input(
    mp: &mut Option<&'static Mbuf>,
    offp: &mut i32,
    proto: i32,
    af: i32,
    ns: Option<&Netstack>,
) -> i32 {
    let _ = (mp, offp, proto, af, ns);
    let _ = crate::unported!("frag6_input: placeholder");
    crate::netinet::in_::IPPROTO_DONE
}

/// `frag6_deletefraghdr`: removes the fragment header at `offset` from `m`.
pub fn frag6_deletefraghdr(m: &Mbuf, offset: i32) -> Result<(), Errno> {
    let _ = (m, offset);
    Err(crate::unported!("frag6_deletefraghdr: placeholder"))
}

/// `frag6_slowtimo`: the IPv6 reassembly timer: ages the queues, drops the expired.
pub fn frag6_slowtimo() {
    let _ = crate::unported!("frag6_slowtimo: placeholder");
}
