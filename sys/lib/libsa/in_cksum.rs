/*	$OpenBSD: in_cksum.c,v 1.7 2020/05/19 12:54:37 patrick Exp $	*/
/*	$NetBSD: in_cksum.c,v 1.3 1995/04/22 13:53:48 cgd Exp $	*/
/* <LICENSES> */
/*
 * Copyright (c) 1992 Regents of the University of California.
 * All rights reserved.
 *
 * This software was developed by the Computer Systems Engineering group
 * at Lawrence Berkeley Laboratory under DARPA contract BG 91-66 and
 * contributed to Berkeley.
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
 * @(#) Header: in_cksum.c,v 1.1 92/09/11 01:15:55 leres Exp  (LBL)
 */
/* </LICENSES> */

//! `in_cksum()`: the Internet checksum of the IP and UDP headers.
//!
//! Upstream: sys/lib/libsa/in_cksum.c @ 3ce1f3f79392
//!
//! ## Deviations
//! - The bytes are a slice. The C sums 16-bit words through a `u_short` load when the
//!   buffer is even-aligned and byte by byte otherwise; both sum the big-endian words, which
//!   is what this does whatever the alignment.

use crate::hdr::endian::ntohs;

/// `in_cksum(p, l)`: the checksum of `p`, as the C returns it: the one's complement of the
/// one's complement sum, in network order (stored as is, it is the bytes of the header
/// field). 0 over data that carry their own valid checksum.
///
/// This routine is very heavily used in the network code and should be modified for each
/// CPU to be as fast as possible. In particular, it should not be this one.
pub fn in_cksum(p: &[u8]) -> u16 {
    // ensure that < 2^16 bytes being summed
    if p.len() >= (1 << 16) {
        crate::exit::panic(format_args!("in_cksum: packet too big"));
    }

    let mut sum: u32 = 0;
    let (words, rest) = p.as_chunks::<2>();
    for &w in words {
        sum += u32::from(u16::from_be_bytes(w));
    }
    if let [last] = rest {
        sum += u32::from(*last) << 8;
    }

    sum = (sum >> 16) + (sum & 0xffff); // add in accumulated carries
    sum += sum >> 16; // add potential last carry
    !ntohs(sum as u16)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn known_vectors() {
        // RFC 1071's example: the words 0001 f203 f4f5 f6f7 sum to 0xddf2.
        let v = [0x00, 0x01, 0xf2, 0x03, 0xf4, 0xf5, 0xf6, 0xf7];
        assert_eq!(in_cksum(&v).to_ne_bytes(), [0x22, 0x0d]);
        // An IP header (Wikipedia's IPv4 header checksum example): 0xb861.
        let mut ip = [
            0x45, 0x00, 0x00, 0x73, 0x00, 0x00, 0x40, 0x00, 0x40, 0x11, 0x00, 0x00, 0xc0, 0xa8,
            0x00, 0x01, 0xc0, 0xa8, 0x00, 0xc7,
        ];
        let sum = in_cksum(&ip);
        assert_eq!(sum.to_ne_bytes(), [0xb8, 0x61]);
        // With the checksum in place, the sum checks to 0.
        ip[10..12].copy_from_slice(&sum.to_ne_bytes());
        assert_eq!(in_cksum(&ip), 0);
        // Odd lengths pad with a zero byte; the alignment of the slice does not matter.
        let buf = [0u8, 0x12, 0x34, 0x56];
        assert_eq!(in_cksum(&buf[1..]), in_cksum(&[0x12, 0x34, 0x56, 0x00]));
        assert_eq!(in_cksum(&buf[1..]).to_ne_bytes(), [!0x68, !0x34]);
        assert_eq!(in_cksum(&[]), 0xffff);
    }
}
