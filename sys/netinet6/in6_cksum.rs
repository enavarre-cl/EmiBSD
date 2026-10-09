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

/* <CODE> */
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
/* </CODE> */

/* <TESTS> */
#[cfg(test)]
mod tests {
    // Host tests for the IPv6 checksum: a hand-computed ICMPv6 vector, an independent
    // flat-buffer oracle over chains cut at odd and even offsets, odd payload lengths, an
    // embedded scope zone, and the no-pseudo-header form.

    use std::sync::MutexGuard;
    use std::vec::Vec;

    use super::*;
    use crate::kern::uipc_mbuf::{m_cat, m_freem, m_get, m_gethdr};
    use crate::sys::mbuf::{M_DONTWAIT, MLEN, MT_DATA};

    fn setup() -> MutexGuard<'static, ()> {
        crate::kern::uipc_mbuf::tests::setup()
    }

    const IPPROTO_ICMPV6: u8 = 58;

    /// An IPv6 header (version 6, `plen`, next header `nxt`, hop limit 255).
    fn ip6_header(src: &[u8; 16], dst: &[u8; 16], plen: u16, nxt: u8) -> [u8; 40] {
        let mut h = [0u8; 40];
        h[0] = 0x60;
        h[4..6].copy_from_slice(&plen.to_be_bytes());
        h[6] = nxt;
        h[7] = 255;
        h[8..24].copy_from_slice(src);
        h[24..40].copy_from_slice(dst);
        h
    }

    /// A packet holding `bytes` (the IPv6 header first), cut into mbufs of the given lengths
    /// (the rest in the last one); every mbuf after the first starts at an odd address.
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
        for (i, p) in pieces.iter().enumerate() {
            assert!(p.len() <= MLEN - 8);
            let m = if i == 0 {
                head
            } else {
                m_get(M_DONTWAIT, MT_DATA).expect("mbuf")
            };
            if i != 0 {
                m.m_data().set(m.m_data().get().wrapping_add(1));
            }
            // SAFETY: the mbuf has room for `p` (checked above) at its data pointer.
            unsafe { core::ptr::copy_nonoverlapping(p.as_ptr(), m.m_data().get(), p.len()) };
            m.m_len().set(p.len() as u32);
            if i != 0 {
                m_cat(head, Some(m));
            }
        }
        head.m_pkthdr().len.set(bytes.len() as i32);
        head
    }

    /// The checksum of RFC 8200 section 8.1, over a flat buffer, as the bytes to put on the
    /// wire.
    fn oracle(src: &[u8; 16], dst: &[u8; 16], nxt: u8, payload: &[u8]) -> [u8; 2] {
        let mut buf = Vec::new();
        buf.extend_from_slice(src);
        buf.extend_from_slice(dst);
        buf.extend_from_slice(&(payload.len() as u32).to_be_bytes());
        buf.extend_from_slice(&[0, 0, 0, nxt]);
        buf.extend_from_slice(payload);
        if buf.len() % 2 != 0 {
            buf.push(0);
        }
        let mut sum = 0u32;
        for w in buf.chunks(2) {
            sum += u32::from(u16::from_be_bytes([w[0], w[1]]));
        }
        while sum >> 16 != 0 {
            sum = (sum & 0xffff) + (sum >> 16);
        }
        (!(sum as u16)).to_be_bytes()
    }

    const LOOPBACK: [u8; 16] = [0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 1];
    const A: [u8; 16] = [0xfd, 0x00, 0x00, 0x77, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 1];
    const B: [u8; 16] = [0xfd, 0x00, 0x00, 0x77, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 2];

    #[test]
    fn icmp6_echo_hand_computed_vector() {
        let _g = setup();
        // ::1 -> ::1, echo request (type 128, code 0), id 0, seq 0, no payload:
        // 0x0001 + 0x0001 (addresses) + 0x0008 (length) + 0x003a (next header) + 0x8000 =
        // 0x8044, whose complement is 0x7fbb.
        let mut pkt = ip6_header(&LOOPBACK, &LOOPBACK, 8, IPPROTO_ICMPV6).to_vec();
        pkt.extend_from_slice(&[128, 0, 0, 0, 0, 0, 0, 0]);
        let m = chain(&pkt, &[]);
        let c = in6_cksum(m, IPPROTO_ICMPV6, 40, 8);
        assert_eq!(c.to_ne_bytes(), [0x7f, 0xbb]);
        assert_eq!(
            oracle(&LOOPBACK, &LOOPBACK, IPPROTO_ICMPV6, &pkt[40..]),
            [0x7f, 0xbb]
        );
        m_freem(m);
    }

    #[test]
    fn chains_cut_anywhere_match_the_oracle() {
        let _g = setup();
        for plen in [8usize, 9, 17, 40, 63, 64, 101] {
            let mut payload: Vec<u8> = (0..plen).map(|i| (i * 37 + 11) as u8).collect();
            payload[0] = 128;
            payload[1] = 0;
            let mut pkt = ip6_header(&A, &B, plen as u16, IPPROTO_ICMPV6).to_vec();
            pkt.extend_from_slice(&payload);
            let want = oracle(&A, &B, IPPROTO_ICMPV6, &payload);

            let cut_sets: [&[usize]; 7] = [
                &[],
                &[40],
                &[41],
                &[40, 1],
                &[43, 3, 5],
                &[40, 7, 1, 2],
                &[60.min(pkt.len() - 1)],
            ];
            for cuts in cut_sets {
                if cuts.iter().sum::<usize>() >= pkt.len() {
                    continue;
                }
                let m = chain(&pkt, cuts);
                assert_eq!(
                    in6_cksum(m, IPPROTO_ICMPV6, 40, plen as u32).to_ne_bytes(),
                    want,
                    "plen {plen}, cuts {cuts:?}"
                );
                m_freem(m);
            }
        }
    }

    #[test]
    fn scope_zone_embedded_in_the_address_is_not_summed() {
        let _g = setup();
        let ll_src = [
            0xfe, 0x80, 0x00, 0x05, 0, 0, 0, 0, 0x50, 0x54, 0x00, 0xff, 0xfe, 0xbb, 0, 2,
        ];
        let ll_dst = [0xff, 0x02, 0x00, 0x05, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 1];
        let payload = [135u8, 0, 0, 0, 0, 0, 0, 0, 1, 2, 3, 4];
        let mut pkt = ip6_header(&ll_src, &ll_dst, payload.len() as u16, IPPROTO_ICMPV6).to_vec();
        pkt.extend_from_slice(&payload);

        let mut src0 = ll_src;
        src0[2] = 0;
        src0[3] = 0;
        let mut dst0 = ll_dst;
        dst0[2] = 0;
        dst0[3] = 0;
        let want = oracle(&src0, &dst0, IPPROTO_ICMPV6, &payload);

        let m = chain(&pkt, &[]);
        assert_eq!(
            in6_cksum(m, IPPROTO_ICMPV6, 40, payload.len() as u32).to_ne_bytes(),
            want
        );
        m_freem(m);
    }

    #[test]
    fn nxt_zero_skips_the_pseudo_header() {
        let _g = setup();
        // RFC 1071's example, behind an IPv6 header: 0001 f203 f4f5 f6f7 -> 220d.
        let payload = [0x00, 0x01, 0xf2, 0x03, 0xf4, 0xf5, 0xf6, 0xf7];
        let mut pkt = ip6_header(&A, &B, 8, 59).to_vec();
        pkt.extend_from_slice(&payload);
        for cuts in [&[][..], &[41], &[40, 3]] {
            let m = chain(&pkt, cuts);
            assert_eq!(in6_cksum(m, 0, 40, 8).to_ne_bytes(), [0x22, 0x0d]);
            m_freem(m);
        }
        // From an odd offset into the first mbuf.
        let m = chain(&pkt, &[]);
        assert_eq!(in6_cksum(m, 0, 41, 7), {
            let flat = &payload[1..];
            let mut sum = 0u32;
            let mut it = flat.chunks(2);
            for w in &mut it {
                sum += u32::from(u16::from_be_bytes([w[0], *w.get(1).unwrap_or(&0)]));
            }
            while sum >> 16 != 0 {
                sum = (sum & 0xffff) + (sum >> 16);
            }
            u16::from_ne_bytes((!(sum as u16)).to_be_bytes())
        });
        m_freem(m);
    }

    #[test]
    fn zero_length_payload_is_the_pseudo_header_alone() {
        let _g = setup();
        let pkt = ip6_header(&A, &B, 0, IPPROTO_ICMPV6).to_vec();
        let m = chain(&pkt, &[]);
        assert_eq!(
            in6_cksum(m, IPPROTO_ICMPV6, 40, 0).to_ne_bytes(),
            oracle(&A, &B, IPPROTO_ICMPV6, &[])
        );
        m_freem(m);
    }
}
/* </TESTS> */
