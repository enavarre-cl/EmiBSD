/*	$OpenBSD: in4_cksum.c,v 1.11 2022/02/01 15:30:10 miod Exp $	*/
/*	$KAME: in4_cksum.c,v 1.10 2001/11/30 10:06:15 itojun Exp $	*/
/*	$NetBSD: in_cksum.c,v 1.13 1996/10/13 02:03:03 christos Exp $	*/
/* <LICENSES> */
/*
 * Copyright (C) 1999 WIDE Project.
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

//! The Internet checksum of an IPv4 payload, with or without the pseudo header:
//! `in4_cksum`.
//!
//! Upstream: sys/netinet/in4_cksum.c @ 3ce1f3f79392
//!
//! Checksum routine for Internet Protocol family headers (portable version), for the IPv4
//! pseudo header checksum: no need to clear non-pseudo-header fields in the IPv4 header. `len`
//! is the actual payload size, not including the IPv4 header and skipped header chain (`off
//! + len` should be equal to the whole packet). With `nxt` 0 there is no pseudo header (ICMP).
//!
//! Status: `ported` (M7b).
//!
//! ## Deviations
//! - The word loop is `netinet/in_cksum.rs`'s [`cksum_add`] (the C file carries a copy of
//!   `in_cksum`'s loop); the pseudo header's words are summed from a byte array laid out as
//!   `struct ipovly`.
//! - The result is a `u16`.

use core::mem::size_of;

use crate::kern::subr_prf::{panic, printf};
use crate::netinet::in_cksum::{cksum_add, cksum_fold};
use crate::netinet::ip::Ip;
use crate::netinet::ip_var::Ipovly;
use crate::sys::endian::htons;
use crate::sys::mbuf::{Mbuf, mtod};

/// `in4_cksum`: the checksum of `len` bytes of `m` from `off`, over the pseudo header of
/// protocol `nxt` when it is not 0.
pub fn in4_cksum(m: &Mbuf, nxt: u8, off: i32, len: i32) -> u16 {
    let mut sum = 0u64;
    let mut odd = false;

    if nxt != 0 {
        // pseudo header
        if (off as usize) < size_of::<Ipovly>() {
            panic(format_args!("in4_cksum: offset too short"));
        }
        if (m.m_len().get() as usize) < size_of::<Ip>() {
            panic(format_args!("in4_cksum: bad mbuf chain"));
        }
        // SAFETY: the first mbuf holds at least an IP header (checked above).
        let ip = unsafe { core::ptr::read_unaligned(mtod::<Ip>(m)) };
        // ih_x1[8] = 0, ih_pr = nxt, ih_len, ih_src, ih_dst: the words 4..9 of the overlay,
        // whose first 8 bytes are zeroes.
        let mut u = [0u8; 12];
        u[1] = nxt;
        u[2..4].copy_from_slice(&htons(len as u16).to_ne_bytes());
        u[4..8].copy_from_slice(&ip.ip_src.s_addr.to_ne_bytes());
        u[8..12].copy_from_slice(&ip.ip_dst.s_addr.to_ne_bytes());
        let (words, _) = u.as_chunks::<2>();
        for w in words {
            sum += u64::from(u16::from_ne_bytes(*w));
        }
    }

    // skip unnecessary part and sum the payload
    if cksum_add(Some(m), off as usize, len as usize, &mut sum, &mut odd) != 0 {
        printf(format_args!("cksum4: out of data\n"));
    }
    cksum_fold(sum)
}
