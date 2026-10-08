/*	$OpenBSD: aes.h,v 1.4 2020/07/22 13:54:30 tobhe Exp $	*/
/*	$OpenBSD: aes.c,v 1.2 2020/07/22 13:54:30 tobhe Exp $	*/
/* <LICENSES> */
/*
 * Copyright (c) 2016 Thomas Pornin <pornin@bolet.org>
 * Copyright (c) 2016 Mike Belopuhov
 *
 * Permission is hereby granted, free of charge, to any person obtaining
 * a copy of this software and associated documentation files (the
 * "Software"), to deal in the Software without restriction, including
 * without limitation the rights to use, copy, modify, merge, publish,
 * distribute, sublicense, and/or sell copies of the Software, and to
 * permit persons to whom the Software is furnished to do so, subject to
 * the following conditions:
 *
 * The above copyright notice and this permission notice shall be
 * included in all copies or substantial portions of the Software.
 *
 * THE SOFTWARE IS PROVIDED "AS IS", WITHOUT WARRANTY OF ANY KIND,
 * EXPRESS OR IMPLIED, INCLUDING BUT NOT LIMITED TO THE WARRANTIES OF
 * MERCHANTABILITY, FITNESS FOR A PARTICULAR PURPOSE AND
 * NONINFRINGEMENT. IN NO EVENT SHALL THE AUTHORS OR COPYRIGHT HOLDERS
 * BE LIABLE FOR ANY CLAIM, DAMAGES OR OTHER LIABILITY, WHETHER IN AN
 * ACTION OF CONTRACT, TORT OR OTHERWISE, ARISING FROM, OUT OF OR IN
 * CONNECTION WITH THE SOFTWARE OR THE USE OR OTHER DEALINGS IN THE
 * SOFTWARE.
 */

/*
 * Copyright (c) 2016 Thomas Pornin <pornin@bolet.org>
 *
 * Modified for OpenBSD by Thomas Pornin and Mike Belopuhov.
 *
 * Permission is hereby granted, free of charge, to any person obtaining
 * a copy of this software and associated documentation files (the
 * "Software"), to deal in the Software without restriction, including
 * without limitation the rights to use, copy, modify, merge, publish,
 * distribute, sublicense, and/or sell copies of the Software, and to
 * permit persons to whom the Software is furnished to do so, subject to
 * the following conditions:
 *
 * The above copyright notice and this permission notice shall be
 * included in all copies or substantial portions of the Software.
 *
 * THE SOFTWARE IS PROVIDED "AS IS", WITHOUT WARRANTY OF ANY KIND,
 * EXPRESS OR IMPLIED, INCLUDING BUT NOT LIMITED TO THE WARRANTIES OF
 * MERCHANTABILITY, FITNESS FOR A PARTICULAR PURPOSE AND
 * NONINFRINGEMENT. IN NO EVENT SHALL THE AUTHORS OR COPYRIGHT HOLDERS
 * BE LIABLE FOR ANY CLAIM, DAMAGES OR OTHER LIABILITY, WHETHER IN AN
 * ACTION OF CONTRACT, TORT OR OTHERWISE, ARISING FROM, OUT OF OR IN
 * CONNECTION WITH THE SOFTWARE OR THE USE OR OTHER DEALINGS IN THE
 * SOFTWARE.
 */
/* </LICENSES> */

/* <CODE> */
//! AES (FIPS 197), the constant-time bitsliced implementation of Thomas Pornin's BearSSL
//! (`aes_ct`), which the crypto framework's `enc_xform_aes`, AES-CTR, AES-GCM and GMAC use.
//! The 128-bit state is split over eight 32-bit words so that two blocks are processed at
//! once and no table is indexed by a secret.
//!
//! Upstream: sys/crypto/aes.h @ 3ce1f3f79392, sys/crypto/aes.c @ 3ce1f3f79392
//!
//! ## Deviations
//! - The header and the file share this module; `AES_CTX` is [`AesCtx`] (arrays of 60 and 120
//!   words and the round count).
//! - The bitsliced state is `&mut [u32; 8]`; the circuit of `aes_ct_bitslice_Sbox` (Boyar and
//!   Peralta) and the shift/mix steps are the C's, step for step.
//! - Functions that return a round count or 0 on a bad key size (`aes_keysched_base`,
//!   `aes_ct_keysched`, `AES_KeySetup_Encrypt`, `AES_KeySetup_Decrypt`) return
//!   `Result<u32, Errno>` (`EINVAL`); `AES_Setkey` returns `Result<(), Errno>` for 0 or -1.
//!   Key lengths are in bytes, as in the C.
//! - Blocks are `[u8; 16]` arrays and ECB runs take slices of `16 * num_blocks` bytes; the
//!   in-place calls of `xform.c` (`AES_Encrypt(ctx, blk, blk)`) copy the block first.

use crate::sys::errno::Errno;

/// `AES_MAXROUNDS`.
pub const AES_MAXROUNDS: usize = 14;

/// `Rcon`.
const RCON: [u8; 10] = [0x01, 0x02, 0x04, 0x08, 0x10, 0x20, 0x40, 0x80, 0x1B, 0x36];

/// `AES_CTX`: the compressed and the expanded bitsliced subkeys, and the number of rounds.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct AesCtx {
    /// `sk`: the compressed subkeys.
    pub sk: [u32; 60],
    /// `sk_exp`: the expanded subkeys, as `aes_ct_bitslice_encrypt` wants them.
    pub sk_exp: [u32; 120],
    /// `num_rounds`: 10, 12 or 14.
    pub num_rounds: u32,
}

impl Default for AesCtx {
    fn default() -> Self {
        Self {
            sk: [0; 60],
            sk_exp: [0; 120],
            num_rounds: 0,
        }
    }
}

/// `dec32le`.
fn dec32le(src: &[u8]) -> u32 {
    u32::from_le_bytes([src[0], src[1], src[2], src[3]])
}

/// `enc32le`.
fn enc32le(dst: &mut [u8], x: u32) {
    dst[..4].copy_from_slice(&x.to_le_bytes());
}

// This constant-time implementation is "bitsliced": the 128-bit state is split over eight
// 32-bit words q* in the following way:
//
// -- Input block consists in 16 bytes:
//    a00 a10 a20 a30 a01 a11 a21 a31 a02 a12 a22 a32 a03 a13 a23 a33
// In the terminology of FIPS 197, this is a 4x4 matrix which is read column by column.
//
// -- Each byte is split into eight bits which are distributed over the eight words, at the
// same rank. Thus, for a byte x at rank k, bit 0 (least significant) of x will be at rank k
// in q0 (if that bit is b, then it contributes "b << k" to the value of q0), bit 1 of x will
// be at rank k in q1, and so on.
//
// -- Ranks given to bits are in "row order" and are either all even, or all odd. Two
// independent AES states are thus interleaved, one using the even ranks, the other the odd
// ranks. Row order means:
//    a00 a01 a02 a03 a10 a11 a12 a13 a20 a21 a22 a23 a30 a31 a32 a33
//
// Converting input bytes from two AES blocks to bitslice representation is done in the
// following way:
// -- Decode first block into the four words q0 q2 q4 q6, in that order, using little-endian
// convention.
// -- Decode second block into the four words q1 q3 q5 q7, in that order, using little-endian
// convention.
// -- Call aes_ct_ortho().
//
// Converting back to bytes is done by using the reverse operations. Note that aes_ct_ortho()
// is its own inverse.

/// `aes_ct_bitslice_Sbox`: the AES S-box, as a bitsliced constant-time version. The input
/// array consists in eight 32-bit words; 32 S-box instances are computed in parallel. Bits 0
/// to 7 of each S-box input (bit 0 is least significant) are spread over the words 0 to 7, at
/// the same rank.
///
/// This S-box implementation is a straightforward translation of the circuit described by
/// Boyar and Peralta in "A new combinational logic minimization technique with applications
/// to cryptology" (<https://eprint.iacr.org/2009/191.pdf>).
///
/// Note that variables x* (input) and s* (output) are numbered in "reverse" order (x0 is the
/// high bit, x7 is the low bit).
#[allow(non_snake_case)] // the C name
pub fn aes_ct_bitslice_Sbox(q: &mut [u32; 8]) {
    let x0 = q[7];
    let x1 = q[6];
    let x2 = q[5];
    let x3 = q[4];
    let x4 = q[3];
    let x5 = q[2];
    let x6 = q[1];
    let x7 = q[0];

    // Top linear transformation.
    let y14 = x3 ^ x5;
    let y13 = x0 ^ x6;
    let y9 = x0 ^ x3;
    let y8 = x0 ^ x5;
    let t0 = x1 ^ x2;
    let y1 = t0 ^ x7;
    let y4 = y1 ^ x3;
    let y12 = y13 ^ y14;
    let y2 = y1 ^ x0;
    let y5 = y1 ^ x6;
    let y3 = y5 ^ y8;
    let t1 = x4 ^ y12;
    let y15 = t1 ^ x5;
    let y20 = t1 ^ x1;
    let y6 = y15 ^ x7;
    let y10 = y15 ^ t0;
    let y11 = y20 ^ y9;
    let y7 = x7 ^ y11;
    let y17 = y10 ^ y11;
    let y19 = y10 ^ y8;
    let y16 = t0 ^ y11;
    let y21 = y13 ^ y16;
    let y18 = x0 ^ y16;

    // Non-linear section.
    let t2 = y12 & y15;
    let t3 = y3 & y6;
    let t4 = t3 ^ t2;
    let t5 = y4 & x7;
    let t6 = t5 ^ t2;
    let t7 = y13 & y16;
    let t8 = y5 & y1;
    let t9 = t8 ^ t7;
    let t10 = y2 & y7;
    let t11 = t10 ^ t7;
    let t12 = y9 & y11;
    let t13 = y14 & y17;
    let t14 = t13 ^ t12;
    let t15 = y8 & y10;
    let t16 = t15 ^ t12;
    let t17 = t4 ^ t14;
    let t18 = t6 ^ t16;
    let t19 = t9 ^ t14;
    let t20 = t11 ^ t16;
    let t21 = t17 ^ y20;
    let t22 = t18 ^ y19;
    let t23 = t19 ^ y21;
    let t24 = t20 ^ y18;

    let t25 = t21 ^ t22;
    let t26 = t21 & t23;
    let t27 = t24 ^ t26;
    let t28 = t25 & t27;
    let t29 = t28 ^ t22;
    let t30 = t23 ^ t24;
    let t31 = t22 ^ t26;
    let t32 = t31 & t30;
    let t33 = t32 ^ t24;
    let t34 = t23 ^ t33;
    let t35 = t27 ^ t33;
    let t36 = t24 & t35;
    let t37 = t36 ^ t34;
    let t38 = t27 ^ t36;
    let t39 = t29 & t38;
    let t40 = t25 ^ t39;

    let t41 = t40 ^ t37;
    let t42 = t29 ^ t33;
    let t43 = t29 ^ t40;
    let t44 = t33 ^ t37;
    let t45 = t42 ^ t41;
    let z0 = t44 & y15;
    let z1 = t37 & y6;
    let z2 = t33 & x7;
    let z3 = t43 & y16;
    let z4 = t40 & y1;
    let z5 = t29 & y7;
    let z6 = t42 & y11;
    let z7 = t45 & y17;
    let z8 = t41 & y10;
    let z9 = t44 & y12;
    let z10 = t37 & y3;
    let z11 = t33 & y4;
    let z12 = t43 & y13;
    let z13 = t40 & y5;
    let z14 = t29 & y2;
    let z15 = t42 & y9;
    let z16 = t45 & y14;
    let z17 = t41 & y8;

    // Bottom linear transformation.
    let t46 = z15 ^ z16;
    let t47 = z10 ^ z11;
    let t48 = z5 ^ z13;
    let t49 = z9 ^ z10;
    let t50 = z2 ^ z12;
    let t51 = z2 ^ z5;
    let t52 = z7 ^ z8;
    let t53 = z0 ^ z3;
    let t54 = z6 ^ z7;
    let t55 = z16 ^ z17;
    let t56 = z12 ^ t48;
    let t57 = t50 ^ t53;
    let t58 = z4 ^ t46;
    let t59 = z3 ^ t54;
    let t60 = t46 ^ t57;
    let t61 = z14 ^ t57;
    let t62 = t52 ^ t58;
    let t63 = t49 ^ t58;
    let t64 = z4 ^ t59;
    let t65 = t61 ^ t62;
    let t66 = z1 ^ t63;
    let s0 = t59 ^ t63;
    let s6 = t56 ^ !t62;
    let s7 = t48 ^ !t60;
    let t67 = t64 ^ t65;
    let s3 = t53 ^ t66;
    let s4 = t51 ^ t66;
    let s5 = t47 ^ t65;
    let s1 = t64 ^ !s3;
    let s2 = t55 ^ !t67;

    q[7] = s0;
    q[6] = s1;
    q[5] = s2;
    q[4] = s3;
    q[3] = s4;
    q[2] = s5;
    q[1] = s6;
    q[0] = s7;
}

/// `SWAPN`: exchanges the bits of `x` and `y` selected by the masks `cl` (low) and `ch`
/// (high), `s` positions apart.
fn swapn(cl: u32, ch: u32, s: u32, x: &mut u32, y: &mut u32) {
    let a = *x;
    let b = *y;
    *x = (a & cl) | ((b & cl) << s);
    *y = ((a & ch) >> s) | (b & ch);
}

/// `SWAP2`, `SWAP4` and `SWAP8` on `q[i]` and `q[j]`.
fn swap_pair(q: &mut [u32; 8], i: usize, j: usize, cl: u32, ch: u32, s: u32) {
    let (mut x, mut y) = (q[i], q[j]);
    swapn(cl, ch, s, &mut x, &mut y);
    q[i] = x;
    q[j] = y;
}

/// `aes_ct_ortho`: perform bytewise orthogonalization of eight 32-bit words. Bytes of q0..q7
/// are spread over all words: for a byte x that occurs at rank i in q[j] (byte x uses bits 8*i
/// to 8*i+7 in q[j]), the bit of rank k in x (0 <= k <= 7) goes to q[k] at rank 8*i+j.
///
/// This operation is an involution.
pub fn aes_ct_ortho(q: &mut [u32; 8]) {
    for (i, j) in [(0, 1), (2, 3), (4, 5), (6, 7)] {
        swap_pair(q, i, j, 0x55555555, 0xAAAAAAAA, 1);
    }
    for (i, j) in [(0, 2), (1, 3), (4, 6), (5, 7)] {
        swap_pair(q, i, j, 0x33333333, 0xCCCCCCCC, 2);
    }
    for (i, j) in [(0, 4), (1, 5), (2, 6), (3, 7)] {
        swap_pair(q, i, j, 0x0F0F0F0F, 0xF0F0F0F0, 4);
    }
}

/// `sub_word`: the S-box on the four bytes of `x`.
fn sub_word(x: u32) -> u32 {
    let mut q = [x; 8];

    aes_ct_ortho(&mut q);
    aes_ct_bitslice_Sbox(&mut q);
    aes_ct_ortho(&mut q);
    q[0]
}

/// `aes_keysched_base`: base key schedule code. Subkeys are produced in little-endian
/// convention (but not bitsliced). Key length is expressed in bytes. The number of rounds, or
/// `EINVAL` for a size other than 16, 24 or 32.
fn aes_keysched_base(skey: &mut [u32; 60], key: &[u8]) -> Result<u32, Errno> {
    let key_len = key.len();
    let num_rounds = match key_len {
        16 => 10,
        24 => 12,
        32 => 14,
        _ => return Err(Errno::EINVAL),
    };
    let nk = key_len >> 2;
    let nkf = ((num_rounds + 1) << 2) as usize;
    for i in 0..nk {
        skey[i] = dec32le(&key[i << 2..]);
    }
    let mut tmp = skey[(key_len >> 2) - 1];
    let (mut j, mut k) = (0, 0);
    for i in nk..nkf {
        if j == 0 {
            tmp = tmp.rotate_right(8);
            tmp = sub_word(tmp) ^ u32::from(RCON[k]);
        } else if nk > 6 && j == 4 {
            tmp = sub_word(tmp);
        }
        tmp ^= skey[i - nk];
        skey[i] = tmp;
        j += 1;
        if j == nk {
            j = 0;
            k += 1;
        }
    }
    Ok(num_rounds)
}

/// `aes_ct_keysched`: AES key schedule, constant-time version. `comp_skey` is filled with n+1
/// 128-bit subkeys, where n is the number of rounds (10 to 14, depending on key size). The
/// number of rounds is returned; `EINVAL` if the key size is invalid (not 16, 24 or 32).
pub fn aes_ct_keysched(comp_skey: &mut [u32], key: &[u8]) -> Result<u32, Errno> {
    let mut skey = [0u32; 60];

    let num_rounds = aes_keysched_base(&mut skey, key)?;
    for u in 0..=num_rounds as usize {
        let mut q = [0u32; 8];

        q[0] = skey[u << 2];
        q[1] = q[0];
        q[2] = skey[(u << 2) + 1];
        q[3] = q[2];
        q[4] = skey[(u << 2) + 2];
        q[5] = q[4];
        q[6] = skey[(u << 2) + 3];
        q[7] = q[6];
        aes_ct_ortho(&mut q);
        comp_skey[u << 2] = (q[0] & 0x55555555) | (q[1] & 0xAAAAAAAA);
        comp_skey[(u << 2) + 1] = (q[2] & 0x55555555) | (q[3] & 0xAAAAAAAA);
        comp_skey[(u << 2) + 2] = (q[4] & 0x55555555) | (q[5] & 0xAAAAAAAA);
        comp_skey[(u << 2) + 3] = (q[6] & 0x55555555) | (q[7] & 0xAAAAAAAA);
    }
    Ok(num_rounds)
}

/// `aes_ct_skey_expand`: expand AES subkeys as produced by `aes_ct_keysched()`, into a larger
/// array suitable for `aes_ct_bitslice_encrypt()` and `aes_ct_bitslice_decrypt()`.
pub fn aes_ct_skey_expand(skey: &mut [u32], num_rounds: u32, comp_skey: &[u32]) {
    let n = ((num_rounds + 1) << 2) as usize;
    for (u, v) in (0..n).zip((0..).step_by(2)) {
        let mut x = comp_skey[u];
        let mut y = x;
        x &= 0x55555555;
        skey[v] = x | (x << 1);
        y &= 0xAAAAAAAA;
        skey[v + 1] = y | (y >> 1);
    }
}

/// `add_round_key`.
fn add_round_key(q: &mut [u32; 8], sk: &[u32]) {
    for i in 0..8 {
        q[i] ^= sk[i];
    }
}

/// `shift_rows`.
fn shift_rows(q: &mut [u32; 8]) {
    for w in q.iter_mut() {
        let x = *w;
        *w = (x & 0x000000FF)
            | ((x & 0x0000FC00) >> 2)
            | ((x & 0x00000300) << 6)
            | ((x & 0x00F00000) >> 4)
            | ((x & 0x000F0000) << 4)
            | ((x & 0xC0000000) >> 6)
            | ((x & 0x3F000000) << 2);
    }
}

/// `rotr16`.
fn rotr16(x: u32) -> u32 {
    x.rotate_left(16)
}

/// `mix_columns`.
fn mix_columns(q: &mut [u32; 8]) {
    let [q0, q1, q2, q3, q4, q5, q6, q7] = *q;
    let r0 = q0.rotate_right(8);
    let r1 = q1.rotate_right(8);
    let r2 = q2.rotate_right(8);
    let r3 = q3.rotate_right(8);
    let r4 = q4.rotate_right(8);
    let r5 = q5.rotate_right(8);
    let r6 = q6.rotate_right(8);
    let r7 = q7.rotate_right(8);

    q[0] = q7 ^ r7 ^ r0 ^ rotr16(q0 ^ r0);
    q[1] = q0 ^ r0 ^ q7 ^ r7 ^ r1 ^ rotr16(q1 ^ r1);
    q[2] = q1 ^ r1 ^ r2 ^ rotr16(q2 ^ r2);
    q[3] = q2 ^ r2 ^ q7 ^ r7 ^ r3 ^ rotr16(q3 ^ r3);
    q[4] = q3 ^ r3 ^ q7 ^ r7 ^ r4 ^ rotr16(q4 ^ r4);
    q[5] = q4 ^ r4 ^ r5 ^ rotr16(q5 ^ r5);
    q[6] = q5 ^ r5 ^ r6 ^ rotr16(q6 ^ r6);
    q[7] = q6 ^ r6 ^ r7 ^ rotr16(q7 ^ r7);
}

/// `aes_ct_bitslice_encrypt`: compute AES encryption on bitsliced data. Since input is stored
/// on eight 32-bit words, two block encryptions are actually performed in parallel.
pub fn aes_ct_bitslice_encrypt(num_rounds: u32, skey: &[u32], q: &mut [u32; 8]) {
    add_round_key(q, skey);
    for u in 1..num_rounds as usize {
        aes_ct_bitslice_Sbox(q);
        shift_rows(q);
        mix_columns(q);
        add_round_key(q, &skey[u << 3..]);
    }
    aes_ct_bitslice_Sbox(q);
    shift_rows(q);
    add_round_key(q, &skey[(num_rounds as usize) << 3..]);
}

/// `aes_ct_bitslice_invSbox`: like `aes_ct_bitslice_Sbox()`, but for the inverse S-box.
///
/// AES S-box is:
///   S(x) = A(I(x)) ^ 0x63
/// where I() is inversion in GF(256), and A() is a linear transform (0 is formally defined to
/// be its own inverse). Since inversion is an involution, the inverse S-box can be computed
/// from the S-box as:
///   iS(x) = B(S(B(x ^ 0x63)) ^ 0x63)
/// where B() is the inverse of A(). Indeed, for any y in GF(256):
///   iS(S(y)) = B(A(I(B(A(I(y)) ^ 0x63 ^ 0x63))) ^ 0x63 ^ 0x63) = y
///
/// Note: the implementation of the forward S-box is reused, instead of duplicated, so that
/// total code size is lower. By merging the B() transforms into the S-box circuit CBC
/// decryption could be made faster, but it is already quite faster than CBC encryption
/// because two blocks can be processed in parallel.
#[allow(non_snake_case)] // the C name
pub fn aes_ct_bitslice_invSbox(q: &mut [u32; 8]) {
    // B() of the input, with the 0x63 added (x ^ 0x63 is the complement of bits 0, 1, 5, 6).
    fn b_transform(q: &mut [u32; 8]) {
        let q0 = !q[0];
        let q1 = !q[1];
        let q2 = q[2];
        let q3 = q[3];
        let q4 = q[4];
        let q5 = !q[5];
        let q6 = !q[6];
        let q7 = q[7];
        q[7] = q1 ^ q4 ^ q6;
        q[6] = q0 ^ q3 ^ q5;
        q[5] = q7 ^ q2 ^ q4;
        q[4] = q6 ^ q1 ^ q3;
        q[3] = q5 ^ q0 ^ q2;
        q[2] = q4 ^ q7 ^ q1;
        q[1] = q3 ^ q6 ^ q0;
        q[0] = q2 ^ q5 ^ q7;
    }

    b_transform(q);
    aes_ct_bitslice_Sbox(q);
    b_transform(q);
}

/// `inv_shift_rows`.
fn inv_shift_rows(q: &mut [u32; 8]) {
    for w in q.iter_mut() {
        let x = *w;
        *w = (x & 0x000000FF)
            | ((x & 0x00003F00) << 2)
            | ((x & 0x0000C000) >> 6)
            | ((x & 0x000F0000) << 4)
            | ((x & 0x00F00000) >> 4)
            | ((x & 0x03000000) << 6)
            | ((x & 0xFC000000) >> 2);
    }
}

/// `inv_mix_columns`.
fn inv_mix_columns(q: &mut [u32; 8]) {
    let [q0, q1, q2, q3, q4, q5, q6, q7] = *q;
    let r0 = q0.rotate_right(8);
    let r1 = q1.rotate_right(8);
    let r2 = q2.rotate_right(8);
    let r3 = q3.rotate_right(8);
    let r4 = q4.rotate_right(8);
    let r5 = q5.rotate_right(8);
    let r6 = q6.rotate_right(8);
    let r7 = q7.rotate_right(8);

    q[0] = q5 ^ q6 ^ q7 ^ r0 ^ r5 ^ r7 ^ rotr16(q0 ^ q5 ^ q6 ^ r0 ^ r5);
    q[1] = q0 ^ q5 ^ r0 ^ r1 ^ r5 ^ r6 ^ r7 ^ rotr16(q1 ^ q5 ^ q7 ^ r1 ^ r5 ^ r6);
    q[2] = q0 ^ q1 ^ q6 ^ r1 ^ r2 ^ r6 ^ r7 ^ rotr16(q0 ^ q2 ^ q6 ^ r2 ^ r6 ^ r7);
    q[3] = q0
        ^ q1
        ^ q2
        ^ q5
        ^ q6
        ^ r0
        ^ r2
        ^ r3
        ^ r5
        ^ rotr16(q0 ^ q1 ^ q3 ^ q5 ^ q6 ^ q7 ^ r0 ^ r3 ^ r5 ^ r7);
    q[4] = q1
        ^ q2
        ^ q3
        ^ q5
        ^ r1
        ^ r3
        ^ r4
        ^ r5
        ^ r6
        ^ r7
        ^ rotr16(q1 ^ q2 ^ q4 ^ q5 ^ q7 ^ r1 ^ r4 ^ r5 ^ r6);
    q[5] =
        q2 ^ q3 ^ q4 ^ q6 ^ r2 ^ r4 ^ r5 ^ r6 ^ r7 ^ rotr16(q2 ^ q3 ^ q5 ^ q6 ^ r2 ^ r5 ^ r6 ^ r7);
    q[6] = q3 ^ q4 ^ q5 ^ q7 ^ r3 ^ r5 ^ r6 ^ r7 ^ rotr16(q3 ^ q4 ^ q6 ^ q7 ^ r3 ^ r6 ^ r7);
    q[7] = q4 ^ q5 ^ q6 ^ r4 ^ r6 ^ r7 ^ rotr16(q4 ^ q5 ^ q7 ^ r4 ^ r7);
}

/// `aes_ct_bitslice_decrypt`: compute AES decryption on bitsliced data. Since input is stored
/// on eight 32-bit words, two block decryptions are actually performed in parallel.
pub fn aes_ct_bitslice_decrypt(num_rounds: u32, skey: &[u32], q: &mut [u32; 8]) {
    add_round_key(q, &skey[(num_rounds as usize) << 3..]);
    for u in (1..num_rounds as usize).rev() {
        inv_shift_rows(q);
        aes_ct_bitslice_invSbox(q);
        add_round_key(q, &skey[u << 3..]);
        inv_mix_columns(q);
    }
    inv_shift_rows(q);
    aes_ct_bitslice_invSbox(q);
    add_round_key(q, skey);
}

/// `AES_Setkey`: sets the key of 16, 24 or 32 bytes.
#[allow(non_snake_case)] // the C name
pub fn AES_Setkey(ctx: &mut AesCtx, key: &[u8]) -> Result<(), Errno> {
    ctx.num_rounds = aes_ct_keysched(&mut ctx.sk, key)?;
    aes_ct_skey_expand(&mut ctx.sk_exp, ctx.num_rounds, &ctx.sk);
    Ok(())
}

/// Loads two blocks (the second may be absent) into the bitsliced state.
fn load_blocks(src: &[u8], two: bool) -> [u32; 8] {
    let mut q = [0u32; 8];

    q[0] = dec32le(src);
    q[2] = dec32le(&src[4..]);
    q[4] = dec32le(&src[8..]);
    q[6] = dec32le(&src[12..]);
    if two {
        q[1] = dec32le(&src[16..]);
        q[3] = dec32le(&src[20..]);
        q[5] = dec32le(&src[24..]);
        q[7] = dec32le(&src[28..]);
    }
    q
}

/// Stores one or two blocks from the bitsliced state.
fn store_blocks(dst: &mut [u8], q: &[u32; 8], two: bool) {
    enc32le(dst, q[0]);
    enc32le(&mut dst[4..], q[2]);
    enc32le(&mut dst[8..], q[4]);
    enc32le(&mut dst[12..], q[6]);
    if two {
        enc32le(&mut dst[16..], q[1]);
        enc32le(&mut dst[20..], q[3]);
        enc32le(&mut dst[24..], q[5]);
        enc32le(&mut dst[28..], q[7]);
    }
}

/// `AES_Encrypt_ECB`: encrypts `num_blocks` blocks of `src` into `dst`.
#[allow(non_snake_case)] // the C name
pub fn AES_Encrypt_ECB(ctx: &AesCtx, src: &[u8], dst: &mut [u8], num_blocks: usize) {
    let mut num_blocks = num_blocks;
    let (mut src, mut dst) = (src, dst);

    while num_blocks > 0 {
        let two = num_blocks > 1;
        let mut q = load_blocks(src, two);
        aes_ct_ortho(&mut q);
        aes_ct_bitslice_encrypt(ctx.num_rounds, &ctx.sk_exp, &mut q);
        aes_ct_ortho(&mut q);
        store_blocks(dst, &q, two);
        if !two {
            break;
        }
        src = &src[32..];
        dst = &mut dst[32..];
        num_blocks -= 2;
    }
}

/// `AES_Decrypt_ECB`: decrypts `num_blocks` blocks of `src` into `dst`.
#[allow(non_snake_case)] // the C name
pub fn AES_Decrypt_ECB(ctx: &AesCtx, src: &[u8], dst: &mut [u8], num_blocks: usize) {
    let mut num_blocks = num_blocks;
    let (mut src, mut dst) = (src, dst);

    while num_blocks > 0 {
        let two = num_blocks > 1;
        let mut q = load_blocks(src, two);
        aes_ct_ortho(&mut q);
        aes_ct_bitslice_decrypt(ctx.num_rounds, &ctx.sk_exp, &mut q);
        aes_ct_ortho(&mut q);
        store_blocks(dst, &q, two);
        if !two {
            break;
        }
        src = &src[32..];
        dst = &mut dst[32..];
        num_blocks -= 2;
    }
}

/// `AES_Encrypt`: encrypts one block.
#[allow(non_snake_case)] // the C name
pub fn AES_Encrypt(ctx: &AesCtx, src: &[u8; 16], dst: &mut [u8; 16]) {
    AES_Encrypt_ECB(ctx, src, dst, 1);
}

/// `AES_Decrypt`: decrypts one block.
#[allow(non_snake_case)] // the C name
pub fn AES_Decrypt(ctx: &AesCtx, src: &[u8; 16], dst: &mut [u8; 16]) {
    AES_Decrypt_ECB(ctx, src, dst, 1);
}

/// `AES_KeySetup_Encrypt`: the encryption subkeys in big-endian words (`4 * (rounds + 1)` of
/// them); the number of rounds.
#[allow(non_snake_case)] // the C name
pub fn AES_KeySetup_Encrypt(skey: &mut [u32], key: &[u8]) -> Result<u32, Errno> {
    let mut tkey = [0u32; 60];

    let r = aes_keysched_base(&mut tkey, key)?;
    for u in 0..((r + 1) << 2) as usize {
        skey[u] = tkey[u].swap_bytes();
    }
    Ok(r)
}

/// `redgf256`: reduce value x modulo polynomial x^8+x^4+x^3+x+1. This works as long as x fits
/// on 12 bits at most.
fn redgf256(x: u32) -> u32 {
    let h = x >> 8;
    (x ^ h ^ (h << 1) ^ (h << 3) ^ (h << 4)) & 0xFF
}

/// `mul9`: multiplication by 0x09 in GF(256).
fn mul9(x: u32) -> u32 {
    redgf256(x ^ (x << 3))
}

/// `mulb`: multiplication by 0x0B in GF(256).
fn mulb(x: u32) -> u32 {
    redgf256(x ^ (x << 1) ^ (x << 3))
}

/// `muld`: multiplication by 0x0D in GF(256).
fn muld(x: u32) -> u32 {
    redgf256(x ^ (x << 2) ^ (x << 3))
}

/// `mule`: multiplication by 0x0E in GF(256).
fn mule(x: u32) -> u32 {
    redgf256((x << 1) ^ (x << 2) ^ (x << 3))
}

/// `AES_KeySetup_Decrypt`: the decryption subkeys, in the order and form the table-driven
/// decryption wants (big-endian words, `InvMixColumns` applied to all but the first and last
/// round keys); the number of rounds.
#[allow(non_snake_case)] // the C name
pub fn AES_KeySetup_Decrypt(skey: &mut [u32], key: &[u8]) -> Result<u32, Errno> {
    let mut tkey = [0u32; 60];

    // Compute encryption subkeys. We get them in big-endian notation.
    let r = AES_KeySetup_Encrypt(&mut tkey, key)? as usize;

    // Copy the subkeys in reverse order. Also, apply InvMixColumns() on the subkeys (except
    // first and last).
    skey[r << 2..(r << 2) + 4].copy_from_slice(&tkey[..4]);
    skey[..4].copy_from_slice(&tkey[r << 2..(r << 2) + 4]);
    for u in 4..(r << 2) {
        let sk = tkey[u];
        let sk0 = sk >> 24;
        let sk1 = (sk >> 16) & 0xFF;
        let sk2 = (sk >> 8) & 0xFF;
        let sk3 = sk & 0xFF;
        let tk0 = mule(sk0) ^ mulb(sk1) ^ muld(sk2) ^ mul9(sk3);
        let tk1 = mul9(sk0) ^ mule(sk1) ^ mulb(sk2) ^ muld(sk3);
        let tk2 = muld(sk0) ^ mul9(sk1) ^ mule(sk2) ^ mulb(sk3);
        let tk3 = mulb(sk0) ^ muld(sk1) ^ mul9(sk2) ^ mule(sk3);
        let tk = (tk0 << 24) ^ (tk1 << 16) ^ (tk2 << 8) ^ tk3;
        skey[((r - (u >> 2)) << 2) + (u & 3)] = tk;
    }

    Ok(r as u32)
}
/* </CODE> */

/* <TESTS> */
#[cfg(test)]
mod tests {
    // Known-answer tests for the constant-time AES: FIPS 197 appendices B and C, NIST SP 800-38A
    // (CBC and CTR built on the block function), the bitsliced S-box against the table of
    // `rijndael.rs`, the multi-block ECB paths, and the key schedules against `rijndael.rs`'s.

    use super::*;
    use crate::crypto::rijndael::{
        RijndaelCtx, rijndael_set_key, rijndaelKeySetupDec, rijndaelKeySetupEnc,
    };
    use crate::crypto::testutil::{hex, hexn};

    extern crate std;
    use std::vec::Vec;

    fn key(len: usize) -> Vec<u8> {
        (0..len).map(|i| i as u8).collect()
    }

    const PT: &str = "00112233445566778899aabbccddeeff";

    #[test]
    fn fips197_appendix_c() {
        let want = [
            (16, "69c4e0d86a7b0430d8cdb78070b4c55a"),
            (24, "dda97ca4864cdfe06eaf70a0ec0d7191"),
            (32, "8ea2b7ca516745bfeafc49904b496089"),
        ];
        for (len, ct) in want {
            let mut ctx = AesCtx::default();
            assert_eq!(AES_Setkey(&mut ctx, &key(len)), Ok(()));
            assert_eq!(ctx.num_rounds, len as u32 / 4 + 6);

            let pt: [u8; 16] = hexn(PT);
            let mut out = [0u8; 16];
            AES_Encrypt(&ctx, &pt, &mut out);
            assert_eq!(out.to_vec(), hex(ct), "encrypt {len}");
            let mut back = [0u8; 16];
            AES_Decrypt(&ctx, &out, &mut back);
            assert_eq!(back, pt, "decrypt {len}");
        }
    }

    #[test]
    fn fips197_appendix_b() {
        let k: [u8; 16] = hexn("2b7e151628aed2a6abf7158809cf4f3c");
        let mut ctx = AesCtx::default();
        assert_eq!(AES_Setkey(&mut ctx, &k), Ok(()));
        let mut out = [0u8; 16];
        AES_Encrypt(&ctx, &hexn("3243f6a8885a308d313198a2e0370734"), &mut out);
        assert_eq!(out.to_vec(), hex("3925841d02dc09fbdc118597196a0b32"));
    }

    #[test]
    fn bad_key_sizes_are_rejected() {
        let mut ctx = AesCtx::default();
        for len in [0usize, 1, 15, 17, 20, 23, 25, 31, 33, 64] {
            let k = key(len);
            assert_eq!(AES_Setkey(&mut ctx, &k), Err(Errno::EINVAL), "{len}");
            let mut sk = [0u32; 60];
            assert_eq!(
                AES_KeySetup_Encrypt(&mut sk, &k),
                Err(Errno::EINVAL),
                "{len}"
            );
            assert_eq!(
                AES_KeySetup_Decrypt(&mut sk, &k),
                Err(Errno::EINVAL),
                "{len}"
            );
        }
    }

    #[test]
    fn the_bitsliced_sbox_is_the_aes_sbox() {
        // 256 inputs, 32 at a time (eight words of 32 bit-sliced bytes); the outputs are checked
        // against the S-box by applying the inverse circuit and getting the input back, and
        // against the known corners.
        let mut q = [0u32; 8];
        for (i, w) in q.iter_mut().enumerate() {
            // byte x (0..32) at rank x: bit i of x.
            for x in 0..32u32 {
                *w |= ((x >> i) & 1) << x;
            }
        }
        let input = q;
        aes_ct_bitslice_Sbox(&mut q);
        // S(0) = 0x63: bit i of the output for x = 0 is at rank 0 of word i.
        let s0: u8 = (0..8).map(|i| ((q[i] & 1) as u8) << i).sum();
        assert_eq!(s0, 0x63);
        // S(1) = 0x7c.
        let s1: u8 = (0..8).map(|i| (((q[i] >> 1) & 1) as u8) << i).sum();
        assert_eq!(s1, 0x7c);
        aes_ct_bitslice_invSbox(&mut q);
        assert_eq!(q, input);
    }

    #[test]
    fn ortho_is_an_involution() {
        let mut q = [
            0x01234567, 0x89abcdef, 0xfedcba98, 0x76543210, 0xdeadbeef, 0x0badf00d, 0xc0ffee00,
            0x12345678,
        ];
        let orig = q;
        aes_ct_ortho(&mut q);
        assert_ne!(q, orig);
        aes_ct_ortho(&mut q);
        assert_eq!(q, orig);
    }

    #[test]
    fn ecb_runs_of_any_length() {
        let mut ctx = AesCtx::default();
        assert_eq!(AES_Setkey(&mut ctx, &key(24)), Ok(()));
        let data: Vec<u8> = (0..16 * 7).map(|i| (i * 5 + 1) as u8).collect();

        // One block at a time is the reference.
        let mut single = std::vec![0u8; data.len()];
        for (s, d) in data.chunks(16).zip(single.chunks_mut(16)) {
            let mut b = [0u8; 16];
            b.copy_from_slice(s);
            let mut o = [0u8; 16];
            AES_Encrypt(&ctx, &b, &mut o);
            d.copy_from_slice(&o);
        }
        for n in 1..=7 {
            let mut out = std::vec![0u8; 16 * n];
            AES_Encrypt_ECB(&ctx, &data[..16 * n], &mut out, n);
            assert_eq!(out, &single[..16 * n], "{n} blocks");
            let mut back = std::vec![0u8; 16 * n];
            AES_Decrypt_ECB(&ctx, &out, &mut back, n);
            assert_eq!(back, &data[..16 * n], "{n} blocks back");
        }
        // Zero blocks is nothing.
        AES_Encrypt_ECB(&ctx, &[], &mut [], 0);
    }

    #[test]
    fn key_setup_functions_agree_with_the_table_driven_cipher() {
        for len in [16usize, 24, 32] {
            let k = key(len);
            let mut ek = [0u32; 60];
            let mut dk = [0u32; 60];
            let r = AES_KeySetup_Encrypt(&mut ek, &k).unwrap();
            assert_eq!(AES_KeySetup_Decrypt(&mut dk, &k), Ok(r));

            let mut rek = [0u32; 60];
            let mut rdk = [0u32; 60];
            let bits = len as i32 * 8;
            assert_eq!(rijndaelKeySetupEnc(&mut rek, &k, bits), Ok(r as i32));
            assert_eq!(rijndaelKeySetupDec(&mut rdk, &k, bits), Ok(r as i32));
            let n = 4 * (r as usize + 1);
            assert_eq!(ek[..n], rek[..n], "encrypt schedule {len}");
            assert_eq!(dk[..n], rdk[..n], "decrypt schedule {len}");

            // And both ciphers agree on a block.
            let mut rctx = RijndaelCtx::default();
            assert_eq!(rijndael_set_key(&mut rctx, &k, bits), Ok(()));
            let mut a = AesCtx::default();
            assert_eq!(AES_Setkey(&mut a, &k), Ok(()));
            let pt: [u8; 16] = hexn("00112233445566778899aabbccddeeff");
            let (mut x, mut y) = ([0u8; 16], [0u8; 16]);
            AES_Encrypt(&a, &pt, &mut x);
            crate::crypto::rijndael::rijndael_encrypt(&rctx, &pt, &mut y);
            assert_eq!(x, y);
        }
    }

    #[test]
    fn nist_sp800_38a_cbc_and_ctr_built_on_the_block_function() {
        let k: [u8; 16] = hexn("2b7e151628aed2a6abf7158809cf4f3c");
        let pt = hex(
            "6bc1bee22e409f96e93d7e117393172aae2d8a571e03ac9c9eb76fac45af8e51
                  30c81c46a35ce411e5fbc1191a0a52eff69f2445df4f9b17ad2b417be66c3710",
        );
        let mut ctx = AesCtx::default();
        assert_eq!(AES_Setkey(&mut ctx, &k), Ok(()));

        // F.2.1 CBC-AES128.Encrypt
        let mut prev: [u8; 16] = hexn("000102030405060708090a0b0c0d0e0f");
        let mut cbc = Vec::new();
        for blk in pt.chunks(16) {
            let mut x = [0u8; 16];
            for i in 0..16 {
                x[i] = blk[i] ^ prev[i];
            }
            AES_Encrypt(&ctx, &x, &mut prev);
            cbc.extend_from_slice(&prev);
        }
        assert_eq!(
            cbc,
            hex(
                "7649abac8119b246cee98e9b12e9197d5086cb9b507219ee95db113a917678b2
             73bed6b8e3c1743b7116e69e222295163ff1caa1681fac09120eca307586e1a7"
            )
        );

        // F.5.1 CTR-AES128.Encrypt
        let mut counter: [u8; 16] = hexn("f0f1f2f3f4f5f6f7f8f9fafbfcfdfeff");
        let mut ctr = Vec::new();
        for blk in pt.chunks(16) {
            let mut ks = [0u8; 16];
            AES_Encrypt(&ctx, &counter, &mut ks);
            ctr.extend(blk.iter().zip(ks).map(|(a, b)| a ^ b));
            for i in (0..16).rev() {
                counter[i] = counter[i].wrapping_add(1);
                if counter[i] != 0 {
                    break;
                }
            }
        }
        assert_eq!(
            ctr,
            hex(
                "874d6191b620e3261bef6864990db6ce9806f66b7970fdff8617187bb9fffdff
             5ae4df3edbd5d35e5b4f09020db03eab1e031dda2fbe03d1792170a0f3009cee"
            )
        );
    }
}
/* </TESTS> */
