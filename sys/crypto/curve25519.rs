/*	$OpenBSD: curve25519.h,v 1.2 2020/07/22 13:54:30 tobhe Exp $	*/
/*	$OpenBSD: curve25519.c,v 1.2 2020/07/22 13:54:30 tobhe Exp $	*/
/* <LICENSES> */
/*
 * Copyright (C) 2019-2020 Matt Dunwoodie <ncon@noconroy.net>
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
 * Copyright (C) 2018-2020 Jason A. Donenfeld <Jason@zx2c4.com>. All Rights Reserved.
 * Copyright (C) 2015-2016 The fiat-crypto Authors.
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
 *
 * This contains two implementation: a machine-generated formally verified
 * implementation of Curve25519 ECDH from:
 * <https://github.com/mit-plv/fiat-crypto>. Though originally machine
 * generated, it has been tweaked to be suitable for use in the kernel.  It is
 * optimized for 32-bit machines and machines that cannot work efficiently with
 * 128-bit integer types.
 */
/* </LICENSES> */

/* <CODE> */
//! Curve25519 (X25519, RFC 7748): the Diffie-Hellman function of WireGuard's handshake. The
//! 32-bit implementation: field elements of ten limbs of alternating 26 and 25 bits, products
//! summed in 64 bits, a Montgomery ladder over the 255 scalar bits with a constant-time swap.
//!
//! Upstream: sys/crypto/curve25519.h @ 3ce1f3f79392, sys/crypto/curve25519.c @ 3ce1f3f79392
//!
//! ## Deviations
//! - The header and the file share this module.
//! - The C's field multiplication, squaring and `* 121666` are fiat-crypto output: straight-line
//!   code with one variable per partial product. Here each is a double loop over the limb
//!   pairs that forms the same sums (a product of two odd-indexed limbs counts twice, a
//!   product past limb 9 wraps with the factor 19, both from the radix 2^25.5) and then the
//!   same carry chain, so every limb that [`fe_mul_impl`] returns is the one the C returns.
//!   `fe_sqr_impl` is that multiplication with both operands equal (fiat's squaring forms the
//!   identical sums with the symmetry folded in). `fe_mul_121666_impl` is the multiplication by
//!   the element `(121666, 0, ..., 0)`, which is how the C's copy was derived.
//! - `fe_freeze`, `fe_tobytes` and `fe_frombytes` are loops over the limb widths instead of
//!   unrolled steps; `addcarryx_*`/`subborrow_*` are one function each, parameterised by the
//!   limb width.
//! - The helpers that fill an `fe` through a pointer (`fe_add`, `fe_sub`, `fe_mul_*`, `fe_sq_*`,
//!   `fe_invert`) return it by value: the C calls alias their output with an input
//!   (`fe_mul_ttt(&t0, &t0, &t1)`), which a `&mut` and a `&` of one object cannot express.
//! - `curve25519` and `curve25519_generate_public` return `bool`, `true` where the C returns
//!   nonzero (the result is not the all-zero point); keys are `[u8; 32]`.
//! - `explicit_bzero` of the temporaries is a wipe by assignment (`crate::crypto::wipe`).

use libkern::{explicit_bzero, timingsafe_bcmp};

use super::wipe;
use crate::dev::rnd::arc4random_buf;

/// `CURVE25519_KEY_SIZE`.
pub const CURVE25519_KEY_SIZE: usize = 32;

/// `null_point`.
const NULL_POINT: [u8; CURVE25519_KEY_SIZE] = [0; CURVE25519_KEY_SIZE];

/// `base_point`: the generator, `u = 9`.
const BASE_POINT: [u8; CURVE25519_KEY_SIZE] = {
    let mut p = [0u8; CURVE25519_KEY_SIZE];
    p[0] = 9;
    p
};

/// The width in bits of each of the ten limbs.
const LIMB_BITS: [u32; 10] = [26, 25, 26, 25, 26, 25, 26, 25, 26, 25];

/// The limbs of `p = 2^255 - 19`.
const P_LIMBS: [u32; 10] = [
    0x3ffffed, 0x1ffffff, 0x3ffffff, 0x1ffffff, 0x3ffffff, 0x1ffffff, 0x3ffffff, 0x1ffffff,
    0x3ffffff, 0x1ffffff,
];

/// The limbs of `2p`, plus a little slack, that `fe_sub` adds so no limb goes negative.
const SUB_BIAS: [u32; 10] = [
    0x7ffffda, 0x3fffffe, 0x7fffffe, 0x3fffffe, 0x7fffffe, 0x3fffffe, 0x7fffffe, 0x3fffffe,
    0x7fffffe, 0x3fffffe,
];

/// `fe` means field element. Here the field is Z/(2^255-19). An element t, entries
/// t[0]...t[9], represents the integer t[0]+2^26 t[1]+2^51 t[2]+2^77 t[3]+2^102 t[4]+...+2^230
/// t[9]. fe limbs are bounded by 1.125*2^26,1.125*2^25,1.125*2^26,1.125*2^25,etc.
/// Multiplication and carrying produce fe from fe_loose.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Fe {
    /// `v`: the limbs.
    pub v: [u32; 10],
}

/// `fe_loose`: limbs are bounded by 3.375*2^26,3.375*2^25,3.375*2^26,3.375*2^25,etc. Addition
/// and subtraction produce fe_loose from (fe, fe).
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct FeLoose {
    /// `v`: the limbs.
    pub v: [u32; 10],
}

/// `curve25519_generate_public`: the public key of `secret` (the multiple of the base point);
/// `false` for the all-zero secret and, as for any other, a zero result.
pub fn curve25519_generate_public(
    pub_: &mut [u8; CURVE25519_KEY_SIZE],
    secret: &[u8; CURVE25519_KEY_SIZE],
) -> bool {
    if !timingsafe_bcmp(secret, &NULL_POINT) {
        return false;
    }
    curve25519(pub_, secret, &BASE_POINT)
}

/// `curve25519_clamp_secret`: clears the three low bits, clears bit 255 and sets bit 254.
pub fn curve25519_clamp_secret(secret: &mut [u8; CURVE25519_KEY_SIZE]) {
    secret[0] &= 248;
    secret[31] = (secret[31] & 127) | 64;
}

/// `curve25519_generate_secret`: a random clamped secret key.
pub fn curve25519_generate_secret(secret: &mut [u8; CURVE25519_KEY_SIZE]) {
    arc4random_buf(secret);
    curve25519_clamp_secret(secret);
}

/// `fe_frombytes_impl`: ignores the top bit of `s`.
fn fe_frombytes_impl(h: &mut [u32; 10], s: &[u8; 32]) {
    let mut t = [0u8; 40];
    let mut pos = 0usize;

    t[..32].copy_from_slice(s);
    for (limb, bits) in h.iter_mut().zip(LIMB_BITS) {
        let mut w = [0u8; 8];
        w.copy_from_slice(&t[pos / 8..pos / 8 + 8]);
        *limb = ((u64::from_le_bytes(w) >> (pos % 8)) & ((1u64 << bits) - 1)) as u32;
        pos += bits as usize;
    }
}

/// `fe_frombytes`.
fn fe_frombytes(h: &mut Fe, s: &[u8; 32]) {
    fe_frombytes_impl(&mut h.v, s);
}

/// `addcarryx_u25` and `addcarryx_u26`: `a + b + c` reduced to `bits` bits, with the carry out.
/// The sum extracts `bits` bits of result and one bit of carry, so 32 bits are enough.
fn addcarryx(bits: u32, c: u32, a: u32, b: u32, low: &mut u32) -> u32 {
    let x = a.wrapping_add(b).wrapping_add(c);
    *low = x & ((1 << bits) - 1);
    (x >> bits) & 1
}

/// `subborrow_u25` and `subborrow_u26`: `a - b - c` reduced to `bits` bits, with the borrow.
fn subborrow(bits: u32, c: u32, a: u32, b: u32, low: &mut u32) -> u32 {
    let x = a.wrapping_sub(b).wrapping_sub(c);
    *low = x & ((1 << bits) - 1);
    x >> 31
}

/// `cmovznz32`: `z` when `t` is zero, `nz` otherwise, without a branch.
fn cmovznz32(t: u32, z: u32, nz: u32) -> u32 {
    let t = 0u32.wrapping_sub(u32::from(t != 0)); // all set if nonzero, 0 if 0
    (t & nz) | (!t & z)
}

/// `fe_freeze`: the fully reduced limbs of `in1`: subtract `p`, and add it back when that
/// borrowed.
fn fe_freeze(out: &mut [u32; 10], in1: &[u32; 10]) {
    let mut diff = [0u32; 10];
    let mut borrow = 0u32;
    for i in 0..10 {
        borrow = subborrow(LIMB_BITS[i], borrow, in1[i], P_LIMBS[i], &mut diff[i]);
    }
    let x49 = cmovznz32(borrow, 0x0, 0xffffffff);
    let mut carry = 0u32;
    for i in 0..10 {
        carry = addcarryx(LIMB_BITS[i], carry, diff[i], x49 & P_LIMBS[i], &mut out[i]);
    }
}

/// `fe_tobytes`: the canonical 32-byte little-endian encoding.
fn fe_tobytes(s: &mut [u8; 32], f: &Fe) {
    let mut h = [0u32; 10];
    let mut acc = 0u64;
    let mut bits = 0u32;
    let mut idx = 0usize;

    fe_freeze(&mut h, &f.v);
    for i in 0..10 {
        acc |= u64::from(h[i]) << bits;
        bits += LIMB_BITS[i];
        while bits >= 8 {
            s[idx] = acc as u8;
            acc >>= 8;
            bits -= 8;
            idx += 1;
        }
    }
    // 255 bits: seven are left for the last byte.
    s[idx] = acc as u8;
}

/// `fe_0`: h = 0.
fn fe_0() -> Fe {
    Fe::default()
}

/// `fe_1`: h = 1.
fn fe_1() -> Fe {
    let mut h = Fe::default();
    h.v[0] = 1;
    h
}

/// `fe_add_impl`.
fn fe_add_impl(in1: &[u32; 10], in2: &[u32; 10]) -> [u32; 10] {
    let mut out = [0u32; 10];
    for i in 0..10 {
        out[i] = in1[i].wrapping_add(in2[i]);
    }
    out
}

/// `fe_add`: h = f + g.
fn fe_add(f: &Fe, g: &Fe) -> FeLoose {
    FeLoose {
        v: fe_add_impl(&f.v, &g.v),
    }
}

/// `fe_sub_impl`.
fn fe_sub_impl(in1: &[u32; 10], in2: &[u32; 10]) -> [u32; 10] {
    let mut out = [0u32; 10];
    for i in 0..10 {
        out[i] = SUB_BIAS[i].wrapping_add(in1[i]).wrapping_sub(in2[i]);
    }
    out
}

/// `fe_sub`: h = f - g.
fn fe_sub(f: &Fe, g: &Fe) -> FeLoose {
    FeLoose {
        v: fe_sub_impl(&f.v, &g.v),
    }
}

/// Carries the ten sums `d` (limb `k` at weight `2^ceil(25.5 k)`) down to limbs of
/// [`LIMB_BITS`] width, the top carry going back in times 19, as `fe_mul_impl` does.
fn fe_carry(d: &[u64; 10]) -> [u32; 10] {
    let mut r = [0u32; 10];
    let mut carry = 0u64;

    for k in 0..10 {
        let t = d[k] + carry;
        r[k] = (t & ((1u64 << LIMB_BITS[k]) - 1)) as u32;
        carry = t >> LIMB_BITS[k];
    }
    // `carry` is the part above 2^255: it is worth 19 times itself at limb 0.
    let t = u64::from(r[0]) + 19 * carry;
    let c0 = (t >> 26) as u32;
    r[0] = (t & 0x3ffffff) as u32;
    let t1 = c0.wrapping_add(r[1]);
    r[1] = t1 & 0x1ffffff;
    r[2] = r[2].wrapping_add(t1 >> 25);
    r
}

/// `fe_mul_impl`: the product of `in1` and `in2`, carried.
fn fe_mul_impl(in1: &[u32; 10], in2: &[u32; 10]) -> [u32; 10] {
    // c[k]: the sum of the partial products that land on limb k; the products of two
    // odd-indexed limbs are worth two (2^ceil(25.5 i) * 2^ceil(25.5 j) is 2^(25.5 (i+j) + 1)).
    let mut c = [0u64; 19];
    for i in 0..10 {
        for j in 0..10 {
            let mut t = u64::from(in1[i]) * u64::from(in2[j]);
            if i & j & 1 == 1 {
                t *= 2;
            }
            c[i + j] += t;
        }
    }
    // Limbs 10 to 18 wrap around 2^255 = 19.
    let mut d = [0u64; 10];
    for k in 0..9 {
        d[k] = c[k] + 19 * c[k + 10];
    }
    d[9] = c[9];
    fe_carry(&d)
}

/// `fe_mul_ttt`: fe = fe * fe.
fn fe_mul_ttt(f: &Fe, g: &Fe) -> Fe {
    Fe {
        v: fe_mul_impl(&f.v, &g.v),
    }
}

/// `fe_mul_tlt`: fe = fe_loose * fe.
fn fe_mul_tlt(f: &FeLoose, g: &Fe) -> Fe {
    Fe {
        v: fe_mul_impl(&f.v, &g.v),
    }
}

/// `fe_mul_tll`: fe = fe_loose * fe_loose.
fn fe_mul_tll(f: &FeLoose, g: &FeLoose) -> Fe {
    Fe {
        v: fe_mul_impl(&f.v, &g.v),
    }
}

/// `fe_sqr_impl`: the square of `in1`, carried.
fn fe_sqr_impl(in1: &[u32; 10]) -> [u32; 10] {
    fe_mul_impl(in1, in1)
}

/// `fe_sq_tl`: fe = fe_loose ^ 2.
fn fe_sq_tl(f: &FeLoose) -> Fe {
    Fe {
        v: fe_sqr_impl(&f.v),
    }
}

/// `fe_sq_tt`: fe = fe ^ 2.
fn fe_sq_tt(f: &Fe) -> Fe {
    Fe {
        v: fe_sqr_impl(&f.v),
    }
}

/// `fe_loose_invert`: `z^(p-2)`, by the addition chain for 2^255 - 21.
fn fe_loose_invert(z: &FeLoose) -> Fe {
    let mut t0 = fe_sq_tl(z);
    let mut t1 = fe_sq_tt(&t0);
    for _ in 1..2 {
        t1 = fe_sq_tt(&t1);
    }
    t1 = fe_mul_tlt(z, &t1);
    t0 = fe_mul_ttt(&t0, &t1);
    let mut t2 = fe_sq_tt(&t0);
    t1 = fe_mul_ttt(&t1, &t2);
    t2 = fe_sq_tt(&t1);
    for _ in 1..5 {
        t2 = fe_sq_tt(&t2);
    }
    t1 = fe_mul_ttt(&t2, &t1);
    t2 = fe_sq_tt(&t1);
    for _ in 1..10 {
        t2 = fe_sq_tt(&t2);
    }
    t2 = fe_mul_ttt(&t2, &t1);
    let mut t3 = fe_sq_tt(&t2);
    for _ in 1..20 {
        t3 = fe_sq_tt(&t3);
    }
    t2 = fe_mul_ttt(&t3, &t2);
    t2 = fe_sq_tt(&t2);
    for _ in 1..10 {
        t2 = fe_sq_tt(&t2);
    }
    t1 = fe_mul_ttt(&t2, &t1);
    t2 = fe_sq_tt(&t1);
    for _ in 1..50 {
        t2 = fe_sq_tt(&t2);
    }
    t2 = fe_mul_ttt(&t2, &t1);
    t3 = fe_sq_tt(&t2);
    for _ in 1..100 {
        t3 = fe_sq_tt(&t3);
    }
    t2 = fe_mul_ttt(&t3, &t2);
    t2 = fe_sq_tt(&t2);
    for _ in 1..50 {
        t2 = fe_sq_tt(&t2);
    }
    t1 = fe_mul_ttt(&t2, &t1);
    t1 = fe_sq_tt(&t1);
    for _ in 1..5 {
        t1 = fe_sq_tt(&t1);
    }
    let out = fe_mul_ttt(&t1, &t0);

    wipe(&mut t0);
    wipe(&mut t1);
    wipe(&mut t2);
    wipe(&mut t3);
    out
}

/// `fe_invert`.
fn fe_invert(z: &Fe) -> Fe {
    let l = FeLoose { v: z.v };
    fe_loose_invert(&l)
}

/// Replace (f,g) with (g,f) if b == 1; replace (f,g) with (f,g) if b == 0.
///
/// Preconditions: b in {0,1}
fn fe_cswap(f: &mut Fe, g: &mut Fe, b: u32) {
    let b = 0u32.wrapping_sub(b);
    for i in 0..10 {
        let mut x = f.v[i] ^ g.v[i];
        x &= b;
        f.v[i] ^= x;
        g.v[i] ^= x;
    }
}

/// `fe_mul_121666_impl`: `in1 * 121666`, carried (`fe_mul_impl` with `in2 = (121666, 0, ...)`).
fn fe_mul_121666_impl(in1: &[u32; 10]) -> [u32; 10] {
    let mut d = [0u64; 10];
    for i in 0..10 {
        d[i] = u64::from(in1[i]) * 121666;
    }
    fe_carry(&d)
}

/// `fe_mul121666`: fe = fe_loose * 121666.
fn fe_mul121666(f: &FeLoose) -> Fe {
    Fe {
        v: fe_mul_121666_impl(&f.v),
    }
}

/// `curve25519`: the X25519 function: `out = scalar * point`, `scalar` clamped first. `false`
/// when `out` is the all-zero point (`point` of low order).
pub fn curve25519(
    out: &mut [u8; CURVE25519_KEY_SIZE],
    scalar: &[u8; CURVE25519_KEY_SIZE],
    point: &[u8; CURVE25519_KEY_SIZE],
) -> bool {
    let mut x1 = Fe::default();
    let mut swap = 0u32;
    let mut e = *scalar;

    curve25519_clamp_secret(&mut e);

    // The C's comment: this implementation was transcribed to Coq and proven to correspond to
    // unary scalar multiplication in affine coordinates given that x1 != 0 is the x coordinate
    // of some point on the curve, and it was checked that doing a ladderstep with x1 = x3 = 0
    // gives z2' = z3' = 0, and z2 = z3 = 0 gives z2' = z3' = 0 (fiat-crypto,
    // src/Curves/Montgomery/XZ.v and XZProofs.v). preconditions: 0 <= e < 2^255 (not
    // necessarily e < order), fe_invert(0) = 0
    fe_frombytes(&mut x1, point);
    let mut x2 = fe_1();
    let mut z2 = fe_0();
    let mut x3 = x1;
    let mut z3 = fe_1();
    let mut x2l = FeLoose::default();
    let mut z2l = FeLoose::default();
    let mut x3l = FeLoose::default();

    for pos in (0..=254usize).rev() {
        // loop invariant as of right before the test, for the case where x1 != 0:
        //   pos >= -1; if z2 = 0 then x2 is nonzero; if z3 = 0 then x3 is nonzero
        //   let r := e >> (pos+1) in the following equalities of projective points:
        //   to_xz (r*P)     === if swap then (x3, z3) else (x2, z2)
        //   to_xz ((r+1)*P) === if swap then (x2, z2) else (x3, z3)
        //   x1 is the nonzero x coordinate of the nonzero point (r*P-(r+1)*P)
        let b = 1 & u32::from(e[pos / 8] >> (pos & 7));
        swap ^= b;
        fe_cswap(&mut x2, &mut x3, swap);
        fe_cswap(&mut z2, &mut z3, swap);
        swap = b;
        // ladderstep formula
        let mut tmp0l = fe_sub(&x3, &z3);
        let mut tmp1l = fe_sub(&x2, &z2);
        x2l = fe_add(&x2, &z2);
        z2l = fe_add(&x3, &z3);
        z3 = fe_mul_tll(&tmp0l, &x2l);
        z2 = fe_mul_tll(&z2l, &tmp1l);
        let tmp0 = fe_sq_tl(&tmp1l);
        let tmp1 = fe_sq_tl(&x2l);
        x3l = fe_add(&z3, &z2);
        z2l = fe_sub(&z3, &z2);
        x2 = fe_mul_ttt(&tmp1, &tmp0);
        tmp1l = fe_sub(&tmp1, &tmp0);
        z2 = fe_sq_tl(&z2l);
        z3 = fe_mul121666(&tmp1l);
        x3 = fe_sq_tl(&x3l);
        tmp0l = fe_add(&tmp0, &z3);
        z3 = fe_mul_ttt(&x1, &z2);
        z2 = fe_mul_tll(&tmp1l, &tmp0l);
    }
    // here pos=-1, so r=e, so to_xz (e*P) === if swap then (x3, z3) else (x2, z2)
    fe_cswap(&mut x2, &mut x3, swap);
    fe_cswap(&mut z2, &mut z3, swap);

    z2 = fe_invert(&z2);
    x2 = fe_mul_ttt(&x2, &z2);
    fe_tobytes(out, &x2);

    wipe(&mut x1);
    wipe(&mut x2);
    wipe(&mut z2);
    wipe(&mut x3);
    wipe(&mut z3);
    wipe(&mut x2l);
    wipe(&mut z2l);
    wipe(&mut x3l);
    explicit_bzero(&mut e);
    timingsafe_bcmp(out, &NULL_POINT)
}
/* </CODE> */

/* <TESTS> */
#[cfg(test)]
mod tests {
    // Known-answer tests for X25519: RFC 7748 section 5.2 (two vectors and the iterated test, 1
    // and 1000 iterations; the million-iteration one is left out for its running time) and
    // section 6.1 (the Diffie-Hellman example), plus the properties of the field code.

    use super::*;
    use crate::crypto::testutil::{hex, hexn};

    fn x25519(scalar: &[u8; 32], point: &[u8; 32]) -> [u8; 32] {
        let mut out = [0u8; 32];
        assert!(curve25519(&mut out, scalar, point));
        out
    }

    #[test]
    fn rfc7748_5_2_vectors() {
        let out = x25519(
            &hexn("a546e36bf0527c9d3b16154b82465edd62144c0ac1fc5a18506a2244ba449ac4"),
            &hexn("e6db6867583030db3594c1a424b15f7c726624ec26b3353b10a903a6d0ab1c4c"),
        );
        assert_eq!(
            out.to_vec(),
            hex("c3da55379de9c6908e94ea4df28d084f32eccf03491c71f754b4075577a28552")
        );
        let out = x25519(
            &hexn("4b66e9d4d1b4673c5ad22691957d6af5c11b6421e0ea01d42ca4169e7918ba0d"),
            &hexn("e5210f12786811d3f4b7959d0538ae2c31dbe7106fc03c3efc4cd549c715a493"),
        );
        assert_eq!(
            out.to_vec(),
            hex("95cbde9476e8907d7aade45cb4b873f88b595a68799fa152e6f8f7647aac7957")
        );
    }

    /// The iterated test: `k = u = 9`, then `(k, u) = (X25519(k, u), k)` the given number of times.
    fn iterate(times: usize) -> [u8; 32] {
        let mut k = [0u8; 32];
        k[0] = 9;
        let mut u = k;
        for _ in 0..times {
            let next = x25519(&k, &u);
            u = k;
            k = next;
        }
        k
    }

    #[test]
    fn rfc7748_5_2_iterated_1() {
        assert_eq!(
            iterate(1).to_vec(),
            hex("422c8e7a6227d7bca1350b3e2bb7279f7897b87bb6854b783c60e80311ae3079")
        );
    }

    #[test]
    fn rfc7748_5_2_iterated_1000() {
        assert_eq!(
            iterate(1000).to_vec(),
            hex("684cf59ba83309552800ef566f2f4d3c1c3887c49360e3875f2eb94d99532c51")
        );
    }

    #[test]
    fn rfc7748_6_1_diffie_hellman() {
        let alice_priv: [u8; 32] =
            hexn("77076d0a7318a57d3c16c17251b26645df4c2f87ebc0992ab177fba51db92c2a");
        let bob_priv: [u8; 32] =
            hexn("5dab087e624a8a4b79e17f8b83800ee66f3bb1292618b6fd1c2f8b27ff88e0eb");

        let mut alice_pub = [0u8; 32];
        let mut bob_pub = [0u8; 32];
        assert!(curve25519_generate_public(&mut alice_pub, &alice_priv));
        assert!(curve25519_generate_public(&mut bob_pub, &bob_priv));
        assert_eq!(
            alice_pub.to_vec(),
            hex("8520f0098930a754748b7ddcb43ef75a0dbf3a0d26381af4eba4a98eaa9b4e6a")
        );
        assert_eq!(
            bob_pub.to_vec(),
            hex("de9edb7d7b7dc1b4d35b61c2ece435373f8343c85b78674dadfc7e146f882b4f")
        );

        let shared_a = x25519(&alice_priv, &bob_pub);
        let shared_b = x25519(&bob_priv, &alice_pub);
        assert_eq!(shared_a, shared_b);
        assert_eq!(
            shared_a.to_vec(),
            hex("4a5d9d5ba4ce2de1728e3bf480350f25e07e21c947d19e3376f09b3c1e161742")
        );
    }

    #[test]
    fn low_order_points_give_the_zero_result() {
        let scalar: [u8; 32] =
            hexn("77076d0a7318a57d3c16c17251b26645df4c2f87ebc0992ab177fba51db92c2a");
        let mut out = [0xaa; 32];
        // u = 0 (order 1) and u = 1 (order 4)
        assert!(!curve25519(&mut out, &scalar, &[0; 32]));
        assert_eq!(out, [0; 32]);
        let mut one = [0u8; 32];
        one[0] = 1;
        assert!(!curve25519(&mut out, &scalar, &one));
        assert_eq!(out, [0; 32]);
        // The all-zero secret has no public key.
        assert!(!curve25519_generate_public(&mut out, &[0; 32]));
    }

    #[test]
    fn the_scalar_is_clamped_and_the_top_bit_of_u_ignored() {
        let scalar: [u8; 32] =
            hexn("a546e36bf0527c9d3b16154b82465edd62144c0ac1fc5a18506a2244ba449ac4");
        let point: [u8; 32] =
            hexn("e6db6867583030db3594c1a424b15f7c726624ec26b3353b10a903a6d0ab1c4c");
        let want = x25519(&scalar, &point);

        // The low three bits and the top two do not matter: they are overwritten.
        let mut other = scalar;
        other[0] ^= 7;
        other[31] ^= 0xc0;
        assert_eq!(x25519(&other, &point), want);
        // Bit 255 of the point is ignored.
        let mut high = point;
        high[31] ^= 0x80;
        assert_eq!(x25519(&scalar, &high), want);

        let mut s = [0xffu8; 32];
        curve25519_clamp_secret(&mut s);
        assert_eq!((s[0], s[31]), (248, 127));
        let mut s = [0u8; 32];
        curve25519_clamp_secret(&mut s);
        assert_eq!((s[0], s[31]), (0, 64));
    }

    #[test]
    fn field_encoding_round_trips_and_freezes() {
        // p - 1 and p + 1 (as unreduced 255-bit patterns) come back as p - 1 and 1: bit 255 is
        // dropped on the way in, and the encoding is the reduced one.
        let mut pm1 = [0xffu8; 32];
        pm1[0] = 0xec;
        pm1[31] = 0x7f;
        let mut f = Fe::default();
        fe_frombytes(&mut f, &pm1);
        let mut out = [0u8; 32];
        fe_tobytes(&mut out, &f);
        assert_eq!(out, pm1);

        let mut p = pm1;
        p[0] = 0xed; // p itself is 0
        fe_frombytes(&mut f, &p);
        fe_tobytes(&mut out, &f);
        assert_eq!(out, [0; 32]);

        let mut pp1 = pm1;
        pp1[0] = 0xee; // p + 1 is 1
        fe_frombytes(&mut f, &pp1);
        fe_tobytes(&mut out, &f);
        let mut one = [0u8; 32];
        one[0] = 1;
        assert_eq!(out, one);
    }

    #[test]
    fn field_inverse() {
        let mut x = Fe::default();
        fe_frombytes(
            &mut x,
            &hexn("e6db6867583030db3594c1a424b15f7c726624ec26b3353b10a903a6d0ab1c4c"),
        );
        let inv = fe_invert(&x);
        let prod = fe_mul_ttt(&x, &inv);
        let mut out = [0u8; 32];
        fe_tobytes(&mut out, &prod);
        let mut one = [0u8; 32];
        one[0] = 1;
        assert_eq!(out, one);
        // fe_invert(0) = 0, which the ladder relies on.
        let zero = fe_invert(&fe_0());
        fe_tobytes(&mut out, &zero);
        assert_eq!(out, [0; 32]);
    }

    #[test]
    fn mul121666_matches_the_general_product() {
        let mut x = Fe::default();
        fe_frombytes(
            &mut x,
            &hexn("a546e36bf0527c9d3b16154b82465edd62144c0ac1fc5a18506a2244ba449ac4"),
        );
        let loose = FeLoose { v: x.v };
        let mut c = Fe::default();
        c.v[0] = 121666;
        let (mut a, mut b) = ([0u8; 32], [0u8; 32]);
        fe_tobytes(&mut a, &fe_mul121666(&loose));
        fe_tobytes(&mut b, &fe_mul_ttt(&x, &c));
        assert_eq!(a, b);
    }
}
/* </TESTS> */
