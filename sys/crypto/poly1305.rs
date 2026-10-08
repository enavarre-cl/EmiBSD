/*	$OpenBSD: poly1305.h,v 1.2 2020/07/22 13:54:30 tobhe Exp $	*/
/*	$OpenBSD: poly1305.c,v 1.2 2020/07/22 13:54:30 tobhe Exp $	*/
/* <LICENSES> */
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
//!
//! ## Deviations
//! - The header and the file share this module.
//! - The limbs are `u64` where the C has `unsigned long`: both are 64 bits on the two LP64
//!   targets, and the code relies on that width (the sign test in [`poly1305_finish`] reads
//!   bit 63). Arithmetic that wraps in C (`g4 - (1 << 26)`) is spelled `wrapping_*`.
//! - The key is `&[u8; 32]`, the tag `&mut [u8; 16]`, the message a slice.
//! - [`poly1305_finish`] clears the limbs by assignment, as the C does one by one; `buffer`
//!   keeps its last block (as in C).

/// `poly1305_block_size`.
#[allow(non_upper_case_globals)] // the C's lower-case macro name, verbatim
pub const poly1305_block_size: usize = 16;

/// `poly1305_state`: the clamped key `r`, the accumulator `h`, the final pad, and the partial
/// block waiting for more input.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
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
    pub final_: u8,
}

/// `U8TO32`: the little-endian 32-bit word at the start of `p`.
fn u8to32(p: &[u8]) -> u64 {
    u64::from(u32::from_le_bytes([p[0], p[1], p[2], p[3]]))
}

/// `poly1305_init`: starts a MAC under `key` (`r` clamped, the pad saved).
pub fn poly1305_init(st: &mut Poly1305State, key: &[u8; 32]) {
    // r &= 0xffffffc0ffffffc0ffffffc0fffffff
    st.r[0] = u8to32(&key[0..]) & 0x3ffffff;
    st.r[1] = (u8to32(&key[3..]) >> 2) & 0x3ffff03;
    st.r[2] = (u8to32(&key[6..]) >> 4) & 0x3ffc0ff;
    st.r[3] = (u8to32(&key[9..]) >> 6) & 0x3f03fff;
    st.r[4] = (u8to32(&key[12..]) >> 8) & 0x00fffff;

    // h = 0
    st.h = [0; 5];

    // save pad for later
    st.pad[0] = u8to32(&key[16..]);
    st.pad[1] = u8to32(&key[20..]);
    st.pad[2] = u8to32(&key[24..]);
    st.pad[3] = u8to32(&key[28..]);

    st.leftover = 0;
    st.final_ = 0;
}

/// `poly1305_blocks`: absorbs `m`, a whole number of 16-byte blocks (a trailing partial block
/// is ignored, as the C's loop does).
fn poly1305_blocks(st: &mut Poly1305State, m: &[u8]) {
    let hibit: u64 = if st.final_ != 0 { 0 } else { 1 << 24 }; // 1 << 128
    let [r0, r1, r2, r3, r4] = st.r;
    let s1 = r1 * 5;
    let s2 = r2 * 5;
    let s3 = r3 * 5;
    let s4 = r4 * 5;
    let [mut h0, mut h1, mut h2, mut h3, mut h4] = st.h;

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

    st.h = [h0, h1, h2, h3, h4];
}

/// `poly1305_update`: adds `m` to the message.
pub fn poly1305_update(st: &mut Poly1305State, m: &[u8]) {
    let mut m = m;

    // handle leftover
    if st.leftover != 0 {
        let want = (poly1305_block_size - st.leftover).min(m.len());
        st.buffer[st.leftover..st.leftover + want].copy_from_slice(&m[..want]);
        m = &m[want..];
        st.leftover += want;
        if st.leftover < poly1305_block_size {
            return;
        }
        let buffer = st.buffer;
        poly1305_blocks(st, &buffer);
        st.leftover = 0;
    }

    // process full blocks
    if m.len() >= poly1305_block_size {
        let want = m.len() & !(poly1305_block_size - 1);
        poly1305_blocks(st, &m[..want]);
        m = &m[want..];
    }

    // store leftover
    if !m.is_empty() {
        st.buffer[st.leftover..st.leftover + m.len()].copy_from_slice(m);
        st.leftover += m.len();
    }
}

/// `poly1305_finish`: pads and absorbs the last block, writes the 16-byte tag and clears the
/// key material from the state.
pub fn poly1305_finish(st: &mut Poly1305State, mac: &mut [u8; 16]) {
    // process the remaining block
    if st.leftover != 0 {
        let mut i = st.leftover;
        st.buffer[i] = 1;
        i += 1;
        st.buffer[i..].fill(0);
        st.final_ = 1;
        let buffer = st.buffer;
        poly1305_blocks(st, &buffer);
    }

    // fully carry h
    let [mut h0, mut h1, mut h2, mut h3, mut h4] = st.h;

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
    let mut f = h0 + st.pad[0];
    h0 = f & 0xffffffff;
    f = h1 + st.pad[1] + (f >> 32);
    h1 = f & 0xffffffff;
    f = h2 + st.pad[2] + (f >> 32);
    h2 = f & 0xffffffff;
    f = h3 + st.pad[3] + (f >> 32);
    h3 = f & 0xffffffff;

    mac[0..4].copy_from_slice(&(h0 as u32).to_le_bytes());
    mac[4..8].copy_from_slice(&(h1 as u32).to_le_bytes());
    mac[8..12].copy_from_slice(&(h2 as u32).to_le_bytes());
    mac[12..16].copy_from_slice(&(h3 as u32).to_le_bytes());

    // zero out the state
    st.h = [0; 5];
    st.r = [0; 5];
    st.pad = [0; 4];
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

    fn mac(key: &[u8; 32], msg: &[u8]) -> [u8; 16] {
        let mut st = Poly1305State::default();
        let mut tag = [0u8; 16];
        poly1305_init(&mut st, key);
        poly1305_update(&mut st, msg);
        poly1305_finish(&mut st, &mut tag);
        tag
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
                let mut st = Poly1305State::default();
                let mut tag = [0u8; 16];
                poly1305_init(&mut st, &key());
                for piece in d[..*n].chunks(chunk) {
                    poly1305_update(&mut st, piece);
                }
                poly1305_finish(&mut st, &mut tag);
                assert_eq!(tag.to_vec(), hex(want), "length {n} in chunks of {chunk}");
            }
        }
    }

    #[test]
    fn finish_clears_the_key_material() {
        let mut st = Poly1305State::default();
        let mut tag = [0u8; 16];
        poly1305_init(&mut st, &key());
        poly1305_update(&mut st, b"abc");
        poly1305_finish(&mut st, &mut tag);
        assert_eq!((st.r, st.h, st.pad), ([0; 5], [0; 5], [0; 4]));
    }
}
/* </TESTS> */
