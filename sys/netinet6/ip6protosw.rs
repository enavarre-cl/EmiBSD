/*	$OpenBSD: ip6protosw.h,v 1.16 2022/02/22 01:02:57 guenther Exp $	*/
/*	$KAME: ip6protosw.h,v 1.22 2001/02/08 18:02:08 itojun Exp $	*/
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
 *
 */

/*-
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
 *	@(#)protosw.h	8.1 (Berkeley) 6/2/93
 */
/* </LICENSES> */

//! Protocol switch table for IPv6: the argument of the IPv6 `pr_ctlinput`s:
//! `<netinet6/ip6protosw.h>`. All other definitions refer to `<sys/protosw.h>`.
//!
//! Upstream: sys/netinet6/ip6protosw.h @ 3ce1f3f79392
//!
//! ## Deviations
//! - `struct ip6ctlparam` crosses `pr_ctlinput`'s `void *` (`PrCtlinputFn` in
//!   `sys/protosw.rs`), so its members are the C's raw pointers into the ICMPv6 error
//!   message being processed (valid for the call); `ip6c_m` is the mbuf reference.

use core::ffi::c_void;
use core::ptr;

use crate::netinet::icmp6::Icmp6Hdr;
use crate::netinet::ip6::Ip6Hdr;
use crate::netinet6::in6::{In6Addr, SockaddrIn6};
use crate::sys::mbuf::Mbuf;

/// `struct ip6ctlparam`: argument type for the last arg of `pr_ctlinput()`, consulted only
/// with the `AF_INET6` family.
///
/// ```text
/// IPv6 ICMP IPv6 [exthdrs] finalhdr payload
/// ^    ^    ^              ^
/// |    |    ip6c_ip6       ip6c_off
/// |    ip6c_icmp6
/// ip6c_m
/// ```
///
/// `ip6c_finaldst` usually points to `ip6c_ip6->ip6_dst`. If the original (internal) packet
/// carries a routing header, it may point the final destination address in the routing
/// header. `ip6c_src`: `ip6c_ip6->ip6_src` + scope info + flowlabel in `ip6c_ip6` (beware of
/// flowlabel, if you try to compare it against others). `ip6c_dst`: `ip6c_finaldst` +
/// scope info.
pub struct Ip6ctlparam {
    /// Start of mbuf chain.
    pub ip6c_m: Option<&'static Mbuf>,
    /// icmp6 header of target packet.
    pub ip6c_icmp6: *mut Icmp6Hdr,
    /// ip6 header of target packet.
    pub ip6c_ip6: *mut Ip6Hdr,
    /// Offset of the target proto header.
    pub ip6c_off: i32,
    /// srcaddr w/ additional info.
    pub ip6c_src: *mut SockaddrIn6,
    /// (final) dstaddr w/ additional info.
    pub ip6c_dst: *mut SockaddrIn6,
    /// Final destination address.
    pub ip6c_finaldst: *mut In6Addr,
    /// Control command dependent data.
    pub ip6c_cmdarg: *mut c_void,
    /// Final next header field.
    pub ip6c_nxt: u8,
}

impl Ip6ctlparam {
    /// An empty parameter block (all pointers NULL).
    pub const fn new() -> Self {
        Self {
            ip6c_m: None,
            ip6c_icmp6: ptr::null_mut(),
            ip6c_ip6: ptr::null_mut(),
            ip6c_off: 0,
            ip6c_src: ptr::null_mut(),
            ip6c_dst: ptr::null_mut(),
            ip6c_finaldst: ptr::null_mut(),
            ip6c_cmdarg: ptr::null_mut(),
            ip6c_nxt: 0,
        }
    }
}

impl Default for Ip6ctlparam {
    fn default() -> Self {
        Self::new()
    }
}
