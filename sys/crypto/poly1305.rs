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
 * Public Domain poly1305 from Andrew Moon
 *
 * poly1305 implementation using 32 bit * 32 bit = 64 bit multiplication
 * and 64 bit addition from https://github.com/floodyberry/poly1305-donna
 */

/*
 * Public Domain poly1305 from Andrew Moon
 * Based on poly1305-donna.c, poly1305-donna-32.h and poly1305-donna.h from:
 *   https://github.com/floodyberry/poly1305-donna
 */
/* </LICENSES> */

/* <CODE> */
//! Poly1305, the one-time authenticator of D. J. Bernstein, in Andrew Moon's `poly1305-donna`
//! form with 26-bit limbs: 32 bit by 32 bit multiplications into 64 bit sums.
//!
//! Upstream: sys/crypto/poly1305.h @ 3ce1f3f79392, sys/crypto/poly1305.c @ 3ce1f3f79392
//! LZ: sys/crypto/poly1305.rs@f5985f1d055a
//!
//! ## Deviations
//! - The header and the file share this module.
//! - The limbs are `u64` where the C has `unsigned long`: both are 64 bits on the two LP64
//!   targets, and the code relies on that width (the sign test in
//!   [`Poly1305State::finalize`] reads bit 63). Arithmetic that wraps in C (`g4 - (1 << 26)`)
//!   is spelled `wrapping_*`.
//!
//! ## Redesign
//! - The functions over a `poly1305_state` are methods of [`Poly1305State`] (LZ: free
//!   functions with the state first): `poly1305_init` is the constructor
//!   [`Poly1305State::new`], which returns the started MAC (LZ: it filled an `&mut`);
//!   `poly1305_update` is [`Poly1305State::update`], `poly1305_blocks` the private `blocks`,
//!   and `poly1305_finish` is [`Poly1305State::finalize`], which returns the tag (LZ: an
//!   `&mut [u8; 16]` out parameter).
//! - The C clears `h`, `r` and `pad` at the end of `poly1305_finish` with plain stores; here
//!   `finalize` zeroes the whole state in place (the partial block included) with stores the
//!   compiler keeps, and so does dropping it (`docs/IDIOMS.md`). [`Poly1305State`] is
//!   therefore not `Copy` or `PartialEq`.
//! - `final` is a `bool` (LZ: a `u8`); the tag's words are the low four bytes of each
//!   reduced limb instead of an `as u32` cast.
//! - Constant time: the block function is the same multiply-and-carry chain, and the final
//!   reduction still selects `h` or `h - p` with a mask from bit 63, without a branch; the
//!   only branches test the public `final` flag and the lengths of the data.

use super::wipe;

/// `poly1305_block_size`.
#[allow(non_upper_case_globals)] // the C's lower-case macro name, verbatim
pub const poly1305_block_size: usize = 16;

/// `poly1305_state`: the clamped key `r`, the accumulator `h`, the final pad, and the partial
/// block waiting for more input. Started by [`Poly1305State::new`]; zeroed when dropped.
#[derive(Clone, Debug, Default)]
pub struct Poly1305State {
    /// `r`: the clamped first half of the key, five 26-bit limbs.
    pub r: [u64; 5],
    /// `h`: the accumulator.
    pub h: [u64; 5],
    /// `pad`: the second half of the key, added after the last block.
    pub pad: [u64; 4],
    /// `leftover`: bytes in `buffer`.
    pub leftover: usize,
    /// `buffer`: the partial block.
    pub buffer: [u8; poly1305_block_size],
    /// `final`: set while the last, padded block is processed (it has no high bit).
    pub final_: bool,
}

impl Drop for Poly1305State {
    /// Wipes the key, the accumulator and the partial block (`docs/IDIOMS.md`: a key
    /// schedule is zeroed when dropped).
    fn drop(&mut self) {
        self.zeroize();
    }
}

impl Poly1305State {
    /// Zeroes the whole state, with stores the compiler keeps (`crate::crypto::wipe`).
    pub(crate) fn zeroize(&mut self) {
        self.r
            .iter_mut()
            .chain(self.h.iter_mut())
            .chain(self.pad.iter_mut())
            .for_each(wipe);
        self.buffer.iter_mut().for_each(wipe);
        wipe(&mut self.leftover);
        wipe(&mut self.final_);
    }

    /// `poly1305_init`: a MAC started under `key` (`r` clamped, the pad saved).
    pub fn new(key: &[u8; 32]) -> Self {
        Self {
            // r &= 0xffffffc0ffffffc0ffffffc0fffffff
            r: [
                u8to32(&key[0..]) & 0x3ffffff,
                (u8to32(&key[3..]) >> 2) & 0x3ffff03,
                (u8to32(&key[6..]) >> 4) & 0x3ffc0ff,
                (u8to32(&key[9..]) >> 6) & 0x3f03fff,
                (u8to32(&key[12..]) >> 8) & 0x00fffff,
            ],
            // h = 0
            h: [0; 5],
            // save pad for later
            pad: [
                u8to32(&key[16..]),
                u8to32(&key[20..]),
                u8to32(&key[24..]),
                u8to32(&key[28..]),
            ],
            leftover: 0,
            buffer: [0; poly1305_block_size],
            final_: false,
        }
    }

    /// `poly1305_blocks`: absorbs `m`, a whole number of 16-byte blocks (a trailing partial
    /// block is ignored, as the C's loop does).
    fn blocks(&mut self, m: &[u8]) {
        let hibit: u64 = if self.final_ { 0 } else { 1 << 24 }; // 1 << 128
        let [r0, r1, r2, r3, r4] = self.r;
        let s1 = r1 * 5;
        let s2 = r2 * 5;
        let s3 = r3 * 5;
        let s4 = r4 * 5;
        let [mut h0, mut h1, mut h2, mut h3, mut h4] = self.h;

        for blk in m.as_chunks::<poly1305_block_size>().0 {
            // h += m[i]
            h0 += u8to32(&blk[0..]) & 0x3ffffff;
            h1 += (u8to32(&blk[3..]) >> 2) & 0x3ffffff;
            h2 += (u8to32(&blk[6..]) >> 4) & 0x3ffffff;
            h3 += (u8to32(&blk[9..]) >> 6) & 0x3ffffff;
            h4 += (u8to32(&blk[12..]) >> 8) | hibit;

            // h *= r
            let d0 = h0 * r0 + h1 * s4 + h2 * s3 + h3 * s2 + h4 * s1;
            let mut d1 = h0 * r1 + h1 * r0 + h2 * s4 + h3 * s3 + h4 * s2;
            let mut d2 = h0 * r2 + h1 * r1 + h2 * r0 + h3 * s4 + h4 * s3;
            let mut d3 = h0 * r3 + h1 * r2 + h2 * r1 + h3 * r0 + h4 * s4;
            let mut d4 = h0 * r4 + h1 * r3 + h2 * r2 + h3 * r1 + h4 * r0;

            // (partial) h %= p
            let mut c = d0 >> 26;
            h0 = d0 & 0x3ffffff;
            d1 += c;
            c = d1 >> 26;
            h1 = d1 & 0x3ffffff;
            d2 += c;
            c = d2 >> 26;
            h2 = d2 & 0x3ffffff;
            d3 += c;
            c = d3 >> 26;
            h3 = d3 & 0x3ffffff;
            d4 += c;
            c = d4 >> 26;
            h4 = d4 & 0x3ffffff;
            h0 += c * 5;
            c = h0 >> 26;
            h0 &= 0x3ffffff;
            h1 += c;
        }

        self.h = [h0, h1, h2, h3, h4];
    }

    /// `poly1305_update`: adds `m` to the message.
    pub fn update(&mut self, m: &[u8]) {
        let mut m = m;

        // handle leftover
        if self.leftover != 0 {
            let want = (poly1305_block_size - self.leftover).min(m.len());
            self.buffer[self.leftover..self.leftover + want].copy_from_slice(&m[..want]);
            m = &m[want..];
            self.leftover += want;
            if self.leftover < poly1305_block_size {
                return;
            }
            let buffer = self.buffer;
            self.blocks(&buffer);
            self.leftover = 0;
        }

        // process full blocks
        if m.len() >= poly1305_block_size {
            let want = m.len() & !(poly1305_block_size - 1);
            self.blocks(&m[..want]);
            m = &m[want..];
        }

        // store leftover
        if !m.is_empty() {
            self.buffer[self.leftover..self.leftover + m.len()].copy_from_slice(m);
            self.leftover += m.len();
        }
    }

    /// `poly1305_finish`: pads and absorbs the last block and returns the 16-byte tag; the
    /// state, key included, is wiped in place.
    pub fn finalize(&mut self) -> [u8; 16] {
        // process the remaining block
        if self.leftover != 0 {
            let i = self.leftover;
            self.buffer[i] = 1;
            self.buffer[i + 1..].fill(0);
            self.final_ = true;
            let buffer = self.buffer;
            self.blocks(&buffer);
        }

        // fully carry h
        let [mut h0, mut h1, mut h2, mut h3, mut h4] = self.h;

        let mut c = h1 >> 26;
        h1 &= 0x3ffffff;
        h2 += c;
        c = h2 >> 26;
        h2 &= 0x3ffffff;
        h3 += c;
        c = h3 >> 26;
        h3 &= 0x3ffffff;
        h4 += c;
        c = h4 >> 26;
        h4 &= 0x3ffffff;
        h0 += c * 5;
        c = h0 >> 26;
        h0 &= 0x3ffffff;
        h1 += c;

        // compute h + -p
        let mut g0 = h0 + 5;
        c = g0 >> 26;
        g0 &= 0x3ffffff;
        let mut g1 = h1 + c;
        c = g1 >> 26;
        g1 &= 0x3ffffff;
        let mut g2 = h2 + c;
        c = g2 >> 26;
        g2 &= 0x3ffffff;
        let mut g3 = h3 + c;
        c = g3 >> 26;
        g3 &= 0x3ffffff;
        let mut g4 = (h4 + c).wrapping_sub(1 << 26);

        // select h if h < p, or h + -p if h >= p
        let mut mask = (g4 >> (u64::BITS - 1)).wrapping_sub(1);
        g0 &= mask;
        g1 &= mask;
        g2 &= mask;
        g3 &= mask;
        g4 &= mask;
        mask = !mask;
        h0 = (h0 & mask) | g0;
        h1 = (h1 & mask) | g1;
        h2 = (h2 & mask) | g2;
        h3 = (h3 & mask) | g3;
        h4 = (h4 & mask) | g4;

        // h = h % (2^128)
        h0 = (h0 | (h1 << 26)) & 0xffffffff;
        h1 = ((h1 >> 6) | (h2 << 20)) & 0xffffffff;
        h2 = ((h2 >> 12) | (h3 << 14)) & 0xffffffff;
        h3 = ((h3 >> 18) | (h4 << 8)) & 0xffffffff;

        // mac = (h + pad) % (2^128)
        let mut f = h0 + self.pad[0];
        h0 = f & 0xffffffff;
        f = h1 + self.pad[1] + (f >> 32);
        h1 = f & 0xffffffff;
        f = h2 + self.pad[2] + (f >> 32);
        h2 = f & 0xffffffff;
        f = h3 + self.pad[3] + (f >> 32);
        h3 = f & 0xffffffff;

        let mut mac = [0u8; 16];
        for (out, h) in mac.as_chunks_mut::<4>().0.iter_mut().zip([h0, h1, h2, h3]) {
            out.copy_from_slice(&h.to_le_bytes()[..4]);
        }

        // zero out the state
        self.zeroize();
        mac
    }
}

/// `U8TO32`: the little-endian 32-bit word at the start of `p`.
fn u8to32(p: &[u8]) -> u64 {
    u64::from(u32::from_le_bytes([p[0], p[1], p[2], p[3]]))
}
/* </CODE> */

/* <TESTS> */
#[cfg(test)]
mod tests {
    // Known-answer tests for Poly1305: RFC 8439 section 2.5.2 and appendix A.3 (the edge cases
    // of the final reduction), and tags of messages around the block size computed with an
    // independent big-integer reference.

    use super::*;
    use crate::crypto::testutil::{hex, hexn};

    extern crate std;

    fn mac(key: &[u8; 32], msg: &[u8]) -> [u8; 16] {
        let mut st = Poly1305State::new(key);
        st.update(msg);
        st.finalize()
    }

    #[test]
    fn rfc8439_2_5_2() {
        let key: [u8; 32] =
            hexn("85d6be7857556d337f4452fe42d506a80103808afb0db2fd4abff6af4149f51b");
        let tag = mac(&key, b"Cryptographic Forum Research Group");
        assert_eq!(tag.to_vec(), hex("a8061dc1305136c6c22b8baf0c0127a9"));
    }

    #[test]
    fn rfc8439_a_3_edge_cases() {
        // #5: the sum is 2^130 - 5 + 1 plus the pad (the carry out of h reaches the tag).
        let mut key = [0u8; 32];
        key[0] = 2;
        assert_eq!(
            mac(&key, &[0xff; 16]).to_vec(),
            hex("03000000000000000000000000000000")
        );
        // #1: an all-zero key and message give an all-zero tag.
        assert_eq!(mac(&[0; 32], &[0; 64]), [0; 16]);
        // The all-ones key and message exercise the clamp and the h >= p select.
        assert_eq!(
            mac(&[0xff; 32], &[0xff; 48]).to_vec(),
            hex("5efc6a6b51fcec4c787c5075997c95e4")
        );
    }

    fn data() -> [u8; 200] {
        let mut d = [0u8; 200];
        for (i, b) in d.iter_mut().enumerate() {
            *b = ((i * 7 + 1) % 256) as u8;
        }
        d
    }

    fn key() -> [u8; 32] {
        let mut k = [0u8; 32];
        for (i, b) in k.iter_mut().enumerate() {
            *b = ((i * 5 + 9) % 256) as u8;
        }
        k
    }

    const CASES: &[(usize, &str)] = &[
        (0, "595e63686d72777c81868b90959a9fa4"),
        (1, "6775848391b0c0afbdecfccbd92839f8"),
        (15, "acb8dc8611e1dc9fa4ca616590b862a7"),
        (16, "2b4f1870f1db48d1e829cac2387cfb5f"),
        (17, "a2cb8397776305a94979964cfe0d323e"),
        (32, "e26590e86fc347feb25758ff022adee4"),
        (33, "eb5af15115885af57f75dfa92f15b28b"),
        (100, "ef2fbc449ff768235384312a4c3bf256"),
        (200, "607c03d8d2120d8530ef647ec38da4cd"),
    ];

    #[test]
    fn lengths_around_the_block_size() {
        let d = data();
        for (n, want) in CASES {
            assert_eq!(mac(&key(), &d[..*n]).to_vec(), hex(want), "length {n}");
        }
    }

    #[test]
    fn any_split_of_the_input_gives_the_same_tag() {
        let d = data();
        for (n, want) in CASES {
            for chunk in [1usize, 3, 7, 15, 16, 17, 31, 64] {
                let mut st = Poly1305State::new(&key());
                for piece in d[..*n].chunks(chunk) {
                    st.update(piece);
                }
                let tag = st.finalize();
                assert_eq!(tag.to_vec(), hex(want), "length {n} in chunks of {chunk}");
            }
        }
    }

    #[test]
    fn finalize_clears_the_key_material() {
        // In place, as the C clears r, h and pad (and what `Drop` runs).
        let mut st = Poly1305State::new(&key());
        st.update(b"abc");
        assert_eq!(st.finalize(), mac(&key(), b"abc"));
        assert_eq!((st.r, st.h, st.pad), ([0; 5], [0; 5], [0; 4]));
        assert_eq!((st.buffer, st.leftover, st.final_), ([0; 16], 0, false));
    }

    /// xorshift64: the property tests' generator.
    fn next(state: &mut u64) -> u64 {
        *state ^= *state << 13;
        *state ^= *state >> 7;
        *state ^= *state << 17;
        *state
    }

    #[test]
    fn random_splits_and_flipped_bits() {
        // Random messages fed in random pieces give the one-call tag; one flipped bit of the
        // message or the key changes it.
        let mut st = 0x0bad_5eed_dead_beefu64;
        for _ in 0..100 {
            let mut k = [0u8; 32];
            k.iter_mut().for_each(|b| *b = next(&mut st) as u8);
            let len = (next(&mut st) % 200) as usize;
            let msg: std::vec::Vec<u8> = (0..len).map(|_| next(&mut st) as u8).collect();
            let whole = mac(&k, &msg);

            let mut s = Poly1305State::new(&k);
            let mut rest = &msg[..];
            while !rest.is_empty() {
                let n = 1 + (next(&mut st) as usize % rest.len());
                s.update(&rest[..n]);
                rest = &rest[n..];
            }
            assert_eq!(s.finalize(), whole);

            let bit = next(&mut st) as usize;
            if len > 0 {
                let mut m2 = msg.clone();
                m2[(bit / 8) % len] ^= 1 << (bit % 8);
                assert_ne!(mac(&k, &m2), whole);
            }
            // Any bit of the pad (the key's second half) changes the tag.
            let mut k2 = k;
            k2[16 + (bit / 8) % 16] ^= 1 << (bit % 8);
            assert_ne!(mac(&k2, &msg), whole);
        }
    }
}
/* </TESTS> */
