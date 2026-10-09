/* <LICENSES> */
/*
 * Copyright (c) 2026 Emilio Navarrete Lineros <enavarre@outlook.com>
 *
 * Permission to use, copy, modify, and distribute this software for any
 * purpose with or without fee is hereby granted, provided that the above
 * copyright notice and this permission notice appear in all copies.
 *
 * THE SOFTWARE IS PROVIDED "AS IS" AND THE AUTHOR DISCLAIMS ALL WARRANTIES
 * WITH REGARD TO THIS SOFTWARE INCLUDING ALL IMPLIED WARRANTIES OF
 * MERCHANTABILITY AND FITNESS. IN NO EVENT SHALL THE AUTHOR BE LIABLE FOR
 * ANY SPECIAL, DIRECT, INDIRECT, OR CONSEQUENTIAL DAMAGES OR ANY DAMAGES
 * WHATSOEVER RESULTING FROM LOSS OF USE, DATA OR PROFITS, WHETHER IN AN
 * ACTION OF CONTRACT, NEGLIGENCE OR OTHER TORTIOUS ACTION, ARISING OUT OF
 * OR IN CONNECTION WITH THE USE OR PERFORMANCE OF THIS SOFTWARE.
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

/* <CODE> */
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
/* </CODE> */

/* <TESTS> */
#[cfg(test)]
mod tests {
    // Host tests for the Internet checksum: the RFC 1071 example, an IPv4 header, chains split at
    // odd and even offsets, and the pseudo header of `in4_cksum`.

    use std::sync::MutexGuard;
    use std::vec::Vec;

    use super::*;
    use crate::kern::uipc_mbuf::{m_cat, m_freem, m_get, m_gethdr};
    use crate::netinet::in4_cksum::in4_cksum;
    use crate::sys::mbuf::{M_DONTWAIT, MLEN, MT_DATA};

    fn setup() -> MutexGuard<'static, ()> {
        crate::kern::uipc_mbuf::tests::setup()
    }

    /// A chain holding `bytes`, cut into mbufs of the given lengths (the rest in the last one).
    fn chain(bytes: &[u8], cuts: &[usize]) -> &'static Mbuf {
        let mut pieces = Vec::new();
        let mut rest = bytes;
        for &c in cuts {
            let (a, b) = rest.split_at(c);
            pieces.push(a);
            rest = b;
        }
        pieces.push(rest);

        let head = m_gethdr(M_DONTWAIT, MT_DATA).expect("mbuf");
        head.m_len().set(0);
        let mut first = true;
        for p in pieces {
            assert!(p.len() <= MLEN - 8);
            let m = if first {
                head
            } else {
                m_get(M_DONTWAIT, MT_DATA).expect("mbuf")
            };
            if !first {
                // Start the data at an odd address to exercise the C's "force to even boundary".
                m.m_data().set(m.m_data().get().wrapping_add(1));
            }
            // SAFETY: the mbuf has room for `p` (checked above) at its data pointer.
            unsafe { core::ptr::copy_nonoverlapping(p.as_ptr(), m.m_data().get(), p.len()) };
            m.m_len().set(p.len() as u32);
            if !first {
                m_cat(head, Some(m));
            }
            first = false;
        }
        head.m_pkthdr().len.set(bytes.len() as i32);
        head
    }

    #[test]
    fn rfc1071_example() {
        let _g = setup();
        // RFC 1071 section 3: 0001 f203 f4f5 f6f7 sums to ddf2, whose complement is 220d.
        let data = [0x00, 0x01, 0xf2, 0x03, 0xf4, 0xf5, 0xf6, 0xf7];
        let m = chain(&data, &[]);
        assert_eq!(in_cksum(m, data.len() as i32).to_ne_bytes(), [0x22, 0x0d]);
        m_freem(m);
    }

    #[test]
    fn ipv4_header_checksum() {
        let _g = setup();
        // A textbook header (192.168.0.1 -> 192.168.0.199, TTL 64, UDP): checksum b861.
        let mut hdr = [
            0x45, 0x00, 0x00, 0x73, 0x00, 0x00, 0x40, 0x00, 0x40, 0x11, 0x00, 0x00, 0xc0, 0xa8,
            0x00, 0x01, 0xc0, 0xa8, 0x00, 0xc7,
        ];
        let m = chain(&hdr, &[]);
        assert_eq!(in_cksum(m, 20).to_ne_bytes(), [0xb8, 0x61]);
        m_freem(m);

        // A header with its checksum verifies to 0.
        hdr[10] = 0xb8;
        hdr[11] = 0x61;
        let m = chain(&hdr, &[]);
        assert_eq!(in_cksum(m, 20), 0);
        m_freem(m);
    }

    #[test]
    fn split_chains_sum_like_one_buffer() {
        let _g = setup();
        let data: Vec<u8> = (0..101u32).map(|i| (i * 37 + 11) as u8).collect();
        let whole = chain(&data, &[]);
        let want = in_cksum(whole, data.len() as i32);
        m_freem(whole);

        for cuts in [&[1usize][..], &[2], &[3, 5], &[7, 7, 7], &[50, 1, 1]] {
            let m = chain(&data, cuts);
            assert_eq!(in_cksum(m, data.len() as i32), want, "cuts {cuts:?}");
            // A prefix of odd length pads its last byte with zero.
            let m2 = chain(&data[..33], &[]);
            assert_eq!(in_cksum(m, 33), in_cksum(m2, 33), "prefix, cuts {cuts:?}");
            m_freem(m2);
            m_freem(m);
        }
    }

    #[test]
    fn in4_cksum_covers_the_pseudo_header() {
        let _g = setup();
        // An IPv4 header (no options) followed by an 8-byte UDP header and 4 bytes of payload.
        let mut pkt = [
            0x45, 0x00, 0x00, 0x20, 0x00, 0x00, 0x00, 0x00, 0x40, 0x11, 0x00, 0x00, 10, 0, 2, 15,
            10, 0, 2, 2, // ip
            0x30, 0x39, 0x00, 0x35, 0x00, 0x0c, 0x00, 0x00, // udp, sum 0
            0xde, 0xad, 0xbe, 0xef,
        ];
        let m = chain(&pkt, &[]);
        let sum = in4_cksum(m, 17, 20, 12);
        m_freem(m);

        // The pseudo header plus the UDP datagram, summed by hand as one buffer.
        let mut manual = Vec::new();
        manual.extend_from_slice(&pkt[12..20]); // src, dst
        manual.extend_from_slice(&[0, 17, 0, 12]); // zero, proto, udp length
        manual.extend_from_slice(&pkt[20..]);
        let m = chain(&manual, &[]);
        assert_eq!(sum, in_cksum(m, manual.len() as i32));
        m_freem(m);

        // With the checksum in place the datagram verifies.
        pkt[26..28].copy_from_slice(&sum.to_ne_bytes());
        let m = chain(&pkt, &[23]);
        assert_eq!(in4_cksum(m, 17, 20, 12), 0);
        m_freem(m);
    }
}
/* </TESTS> */
