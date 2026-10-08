/*	$OpenBSD: des_locl.h,v 1.7 2015/12/10 21:00:51 naddy Exp $	*/
/* <LICENSES> */
/* lib/des/des_locl.h */

/* Copyright (C) 1995 Eric Young (eay@mincom.oz.au)
 * All rights reserved.
 *
 * This file is part of an SSL implementation written
 * by Eric Young (eay@mincom.oz.au).
 * The implementation was written so as to conform with Netscapes SSL
 * specification.  This library and applications are
 * FREE FOR COMMERCIAL AND NON-COMMERCIAL USE
 * as long as the following conditions are aheared to.
 *
 * Copyright remains Eric Young's, and as such any Copyright notices in
 * the code are not to be removed.  If this code is used in a product,
 * Eric Young should be given attribution as the author of the parts used.
 * This can be in the form of a textual message at program startup or
 * in documentation (online or textual) provided with the package.
 *
 * Redistribution and use in source and binary forms, with or without
 * modification, are permitted provided that the following conditions
 * are met:
 * 1. Redistributions of source code must retain the copyright
 *    notice, this list of conditions and the following disclaimer.
 * 2. Redistributions in binary form must reproduce the above copyright
 *    notice, this list of conditions and the following disclaimer in the
 *    documentation and/or other materials provided with the distribution.
 * 3. All advertising materials mentioning features or use of this software
 *    must display the following acknowledgement:
 *    This product includes software developed by Eric Young (eay@mincom.oz.au)
 *
 * THIS SOFTWARE IS PROVIDED BY ERIC YOUNG ``AS IS'' AND
 * ANY EXPRESS OR IMPLIED WARRANTIES, INCLUDING, BUT NOT LIMITED TO, THE
 * IMPLIED WARRANTIES OF MERCHANTABILITY AND FITNESS FOR A PARTICULAR PURPOSE
 * ARE DISCLAIMED.  IN NO EVENT SHALL THE AUTHOR OR CONTRIBUTORS BE LIABLE
 * FOR ANY DIRECT, INDIRECT, INCIDENTAL, SPECIAL, EXEMPLARY, OR CONSEQUENTIAL
 * DAMAGES (INCLUDING, BUT NOT LIMITED TO, PROCUREMENT OF SUBSTITUTE GOODS
 * OR SERVICES; LOSS OF USE, DATA, OR PROFITS; OR BUSINESS INTERRUPTION)
 * HOWEVER CAUSED AND ON ANY THEORY OF LIABILITY, WHETHER IN CONTRACT, STRICT
 * LIABILITY, OR TORT (INCLUDING NEGLIGENCE OR OTHERWISE) ARISING IN ANY WAY
 * OUT OF THE USE OF THIS SOFTWARE, EVEN IF ADVISED OF THE POSSIBILITY OF
 * SUCH DAMAGE.
 *
 * The licence and distribution terms for any publically available version or
 * derivative of this code cannot be changed.  i.e. this code cannot simply be
 * copied and put under another distribution licence
 * [including the GNU Public Licence.]
 */
/* </LICENSES> */

/* <CODE> */
//! The private header of the DES code (Eric Young's libdes, as in OpenBSD's `lib/des`): the
//! types of a key and of a key schedule, the byte/word helpers, and the permutation and round
//! macros `ecb_enc.c`, `ecb3_enc.c` and `set_key.c` share.
//!
//! Upstream: sys/crypto/des_locl.h @ 3ce1f3f79392
//!
//! ## Deviations
//! - `des_cblock` is [`DesCblock`]. `des_key_schedule` (sixteen `struct des_ks_struct`, each a
//!   union of an 8-byte block and two words, so 128 bytes that the code only ever reads and
//!   writes as 32 words) is [`DesKeySchedule`], an array of 32 words.
//! - The macros are functions: `c2l` and `l2c` read and write one little-endian word at the
//!   start of a slice instead of advancing a pointer; `PERM_OP` takes the two words by
//!   `&mut` (the scratch `t` is a local); `IP` and `FP` are [`ip`] and [`fp`]; `D_ENCRYPT` is
//!   [`d_encrypt`], reading the schedule as a slice and the round at the word index `s`.

use super::spr::DES_SPTRANS;

/// `des_cblock`: an 8-byte block or key.
pub type DesCblock = [u8; 8];

/// `des_key_schedule`: sixteen 8-byte subkeys, as 32 words.
pub type DesKeySchedule = [u32; 32];

/// `DES_KEY_SZ`: `sizeof(des_cblock)`.
pub const DES_KEY_SZ: usize = 8;
/// `DES_SCHEDULE_SZ`: `sizeof(des_key_schedule)`, in bytes.
pub const DES_SCHEDULE_SZ: usize = 128;

/// `ITERATIONS`.
pub const ITERATIONS: usize = 16;
/// `HALF_ITERATIONS`.
pub const HALF_ITERATIONS: usize = 8;

/// `c2l`: the little-endian word at the start of `c`.
pub fn c2l(c: &[u8]) -> u32 {
    u32::from_le_bytes([c[0], c[1], c[2], c[3]])
}

/// `l2c`: stores `l` little-endian at the start of `c`.
pub fn l2c(l: u32, c: &mut [u8]) {
    c[..4].copy_from_slice(&l.to_le_bytes());
}

/// `D_ENCRYPT`: one DES round. `q ^= f(r, subkey s)`, the subkey being the words `s` and `s + 1`
/// of the schedule.
pub fn d_encrypt(q: &mut u32, r: u32, s: &[u32], i: usize) {
    let u = r ^ s[i];
    let mut t = r ^ s[i + 1];
    t = t.rotate_right(4);
    *q ^= DES_SPTRANS[1][(t & 0x3f) as usize]
        | DES_SPTRANS[3][((t >> 8) & 0x3f) as usize]
        | DES_SPTRANS[5][((t >> 16) & 0x3f) as usize]
        | DES_SPTRANS[7][((t >> 24) & 0x3f) as usize]
        | DES_SPTRANS[0][(u & 0x3f) as usize]
        | DES_SPTRANS[2][((u >> 8) & 0x3f) as usize]
        | DES_SPTRANS[4][((u >> 16) & 0x3f) as usize]
        | DES_SPTRANS[6][((u >> 24) & 0x3f) as usize];
}

// IP and FP
// The problem is more of a geometric problem that random bit fiddling.
//  0  1  2  3  4  5  6  7      62 54 46 38 30 22 14  6
//  8  9 10 11 12 13 14 15      60 52 44 36 28 20 12  4
// 16 17 18 19 20 21 22 23      58 50 42 34 26 18 10  2
// 24 25 26 27 28 29 30 31  to  56 48 40 32 24 16  8  0
//
// 32 33 34 35 36 37 38 39      63 55 47 39 31 23 15  7
// 40 41 42 43 44 45 46 47      61 53 45 37 29 21 13  5
// 48 49 50 51 52 53 54 55      59 51 43 35 27 19 11  3
// 56 57 58 59 60 61 62 63      57 49 41 33 25 17  9  1
//
// The output has been subject to swaps of the form 0 1 -> 3 1 but the odd and even bits have
// been put into 2 3 2 0 different words. The main trick is to remember that
//   t=((l>>size)^r)&(mask); r^=t; l^=(t<<size);
// can be used to swap and move bits between words (see the C for the worked 2-D example).

/// `PERM_OP`: exchanges the bits of `a` and `b` selected by `m`, `n` positions apart.
pub fn perm_op(a: &mut u32, b: &mut u32, n: u32, m: u32) {
    let t = ((*a >> n) ^ *b) & m;
    *b ^= t;
    *a ^= t << n;
}

/// `IP`: the initial permutation.
pub fn ip(l: &mut u32, r: &mut u32) {
    perm_op(r, l, 4, 0x0f0f0f0f);
    perm_op(l, r, 16, 0x0000ffff);
    perm_op(r, l, 2, 0x33333333);
    perm_op(l, r, 8, 0x00ff00ff);
    perm_op(r, l, 1, 0x55555555);
}

/// `FP`: the final permutation.
pub fn fp(l: &mut u32, r: &mut u32) {
    perm_op(l, r, 1, 0x55555555);
    perm_op(r, l, 8, 0x00ff00ff);
    perm_op(l, r, 2, 0x33333333);
    perm_op(r, l, 16, 0x0000ffff);
    perm_op(l, r, 4, 0x0f0f0f0f);
}
/* </CODE> */
