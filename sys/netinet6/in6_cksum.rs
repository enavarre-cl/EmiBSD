/*	$OpenBSD: in6_cksum.c,v 1.18 2019/04/22 22:47:49 bluhm Exp $	*/
/*	$KAME: in6_cksum.c,v 1.10 2000/12/03 00:53:59 itojun Exp $	*/
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
 * Copyright (c) 1988, 1992, 1993
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
 *	@(#)in_cksum.c	8.1 (Berkeley) 6/10/93
 */
/* </LICENSES> */

//! Checksum of an IPv6 upper-layer packet with its pseudo header:
//! `netinet6/in6_cksum.c`.
//!
//! Upstream: sys/netinet6/in6_cksum.c @ 3ce1f3f79392
//!
//! Checksum routine for Internet Protocol family headers (portable version). `m` must contain
//! a continuous IP6 header; `off` is the offset where the TCP/UDP/ICMP6 header starts; `len` is
//! the total length of a transport segment (e.g. TCP header + TCP payload). With `nxt` 0 there
//! is no pseudo header.
//!
//! ## Deviations
//! - The word loop is `netinet/in_cksum.rs`'s [`cksum_add`] (the C file carries a copy of
//!   `in_cksum`'s loop, as `in4_cksum.c` does); the pseudo header's words are summed from a
//!   byte array laid out as the C's `uph` union plus the two addresses.
//! - An `off` beyond the chain panics with "out of data" like a chain shorter than `off +
//!   len`; the C distinguishes "out of header" from "out of data" for those two cases.
//! - The result is a `u16` (the C's `int` holds `~sum & 0xffff`).

use crate::kern::subr_prf::panic;
use crate::netinet::in_cksum::{cksum_add, cksum_fold};
use crate::netinet6::in6::in6_is_scope_embed;
use crate::netinet6::ip6_var::mtod_ip6;
use crate::sys::endian::htonl;
use crate::sys::mbuf::Mbuf;

/// `in6_cksum`: the checksum of `len` bytes from offset `off` of `m`, an IPv6 packet, with the
/// pseudo header of next header `nxt` (0: no pseudo header).
pub fn in6_cksum(m: &Mbuf, nxt: u8, off: u32, len: u32) -> u16 {
    let mut sum = 0u64;
    let mut odd = false;

    // sanity check
    if (m.m_pkthdr().len.get() as i64) < i64::from(off) + i64::from(len) {
        panic(format_args!(
            "in6_cksum: mbuf len ({}) < off+len ({}+{})",
            m.m_pkthdr().len.get(),
            off,
            len
        ));
    }

    // Skip pseudo-header if nxt == 0.
    if nxt != 0 {
        // First create IP6 pseudo header and calculate a summary.
        let ip6 = mtod_ip6(m);
        let mut words = [0u16; 8 + 8 + 4];
        let mut put = |i: usize, bytes: [u8; 2]| words[i] = u16::from_ne_bytes(bytes);

        // IPv6 source address
        let src = &ip6.ip6_src.s6_addr;
        for i in 0..8 {
            put(i, [src[2 * i], src[2 * i + 1]]);
        }
        if in6_is_scope_embed(&ip6.ip6_src) {
            put(1, [0, 0]);
        }
        // IPv6 destination address
        let dst = &ip6.ip6_dst.s6_addr;
        for i in 0..8 {
            put(8 + i, [dst[2 * i], dst[2 * i + 1]]);
        }
        if in6_is_scope_embed(&ip6.ip6_dst) {
            put(9, [0, 0]);
        }
        // Payload length and upper layer identifier: `ph_len` (4 bytes, network order),
        // `ph_zero[3]` and `ph_nxt`.
        let l = htonl(len).to_ne_bytes();
        put(16, [l[0], l[1]]);
        put(17, [l[2], l[3]]);
        put(18, [0, 0]);
        put(19, [0, nxt]);

        for w in words {
            sum += u64::from(w);
        }
    }

    // Secondly calculate a summary of the first mbuf excluding offset, and lastly of the rest
    // of the mbufs.
    let short = cksum_add(Some(m), off as usize, len as usize, &mut sum, &mut odd);
    if short != 0 {
        panic(format_args!("in6_cksum: out of data, len {short}"));
    }

    cksum_fold(sum)
}

#[cfg(test)]
mod tests;
