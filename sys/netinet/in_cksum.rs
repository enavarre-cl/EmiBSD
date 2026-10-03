/*	$OpenBSD: in_cksum.c,v 1.9 2019/04/22 22:47:49 bluhm Exp $	*/
/*	$NetBSD: in_cksum.c,v 1.11 1996/04/08 19:55:37 jonathan Exp $	*/
/* <LICENSES> */
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

//! The Internet checksum over an mbuf chain: `in_cksum`.
//!
//! Upstream: sys/netinet/in_cksum.c @ 3ce1f3f79392
//!
//! Checksum routine for Internet Protocol family headers (portable version): the one's
//! complement of the one's complement sum of the data's 16-bit words (RFC 1071). Both amd64
//! and arm64 build this portable C file (`files.amd64`, `files.arm64`); neither has a
//! machine-dependent `in_cksum`.
//!
//! Status: `ported` (M7b).
//!
//! ## Deviations
//! - The C sums 16-bit words straight from memory and, when a word straddles two mbufs or the
//!   data starts at an odd address, saves the odd byte and byte-swaps its running sum so the
//!   following aligned words land in the right half. [`cksum_add`] expresses the same sum by
//!   the position of each byte in the stream: an even-positioned byte is the first byte of a
//!   native word, an odd-positioned one the second. The sum is the C's; it is accumulated in
//!   64 bits and folded at the end instead of being reduced as it goes.
//! - `cksum_add` is shared with `netinet/in4_cksum.rs`, whose loop is a copy of this one in C.
//! - The result is a `u16` (the C's `int` holds `~sum & 0xffff`).

use crate::kern::subr_prf::panic;
use crate::sys::mbuf::{Mbuf, mtod};

/// Adds `len` bytes of the chain `m`, from `off` bytes into its first mbuf, to the
/// one's-complement accumulator `sum`; `odd` says whether the next byte is the second byte
/// of a 16-bit word, and is updated. Returns the number of bytes the chain was short of
/// `len`.
pub fn cksum_add(m: Option<&Mbuf>, off: usize, len: usize, sum: &mut u64, odd: &mut bool) -> usize {
    let mut len = len;
    let mut off = off;
    let mut m = m;
    while let Some(mm) = m {
        if len == 0 {
            break;
        }
        let mlen = mm.m_len().get() as usize;
        if mlen > off {
            let n = (mlen - off).min(len);
            // SAFETY: an mbuf's data area holds `m_len` bytes at `m_data`, and `off + n` is
            // within them.
            let data = unsafe { core::slice::from_raw_parts(mtod::<u8>(mm).add(off), n) };
            let mut bytes = data;
            if *odd && !bytes.is_empty() {
                *sum += u64::from(u16::from_ne_bytes([0, bytes[0]]));
                bytes = &bytes[1..];
                *odd = false;
            }
            let (pairs, rest) = bytes.as_chunks::<2>();
            for w in pairs {
                *sum += u64::from(u16::from_ne_bytes(*w));
            }
            if let [b] = rest {
                *sum += u64::from(u16::from_ne_bytes([*b, 0]));
                *odd = true;
            }
            len -= n;
            off = 0;
        } else {
            off -= mlen;
        }
        m = mm.m_next().get();
    }
    len
}

/// The one's complement of the folded sum.
pub fn cksum_fold(sum: u64) -> u16 {
    let mut sum = sum;
    while sum >> 16 != 0 {
        sum = (sum & 0xffff) + (sum >> 16);
    }
    !(sum as u16)
}

/// `in_cksum`: the Internet checksum of the first `len` bytes of chain `m`. Panics when the
/// chain is shorter, as the C does.
pub fn in_cksum(m: &Mbuf, len: i32) -> u16 {
    let mut sum = 0u64;
    let mut odd = false;

    let short = cksum_add(Some(m), 0, len as usize, &mut sum, &mut odd);
    if short != 0 {
        panic(format_args!("in_cksum: out of data, len {short}"));
    }
    cksum_fold(sum)
}

#[cfg(test)]
mod tests;
