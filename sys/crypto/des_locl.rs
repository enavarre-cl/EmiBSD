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
//! LZ: sys/crypto/des_locl.rs@f5985f1d055a
//!
//! ## Deviations
//! - `des_cblock` is [`DesCblock`]. `des_key_schedule` (sixteen `struct des_ks_struct`, each a
//!   union of an 8-byte block and two words, so 128 bytes that the code only ever reads and
//!   writes as 32 words) is [`DesKeySchedule`], sixteen subkeys of two words.
//! - `DES_USE_PTR` (an alternative way of indexing `des_SPtrans`, undefined in the C) is not
//!   ported.
//!
//! ## Redesign
//! - [`DesKeySchedule`] is a type of its own (LZ: the alias `[u32; 32]`): its sixteen subkeys
//!   are `[[u32; 2]; 16]`, so a round takes its subkey by value of type instead of a word index
//!   into a slice; it is built only by `DesKeySchedule::new` (`set_key.rs`), and it zeroes its
//!   words when it is dropped (`docs/IDIOMS.md`), which the C leaves to the framework's
//!   `explicit_bzero` of the session.
//! - The macros return their results instead of updating their arguments: [`c2l`] reads both
//!   little-endian halves of a block and [`l2c`] writes them back (the C's two pointer-advancing
//!   calls); [`perm_op`], [`ip`] and [`fp`] take and return the pair of words; [`d_encrypt`]
//!   returns the round function's output, which the caller xors into its half.

use super::spr::DES_SPTRANS;
use super::wipe;

/// `des_cblock`: an 8-byte block or key.
pub type DesCblock = [u8; 8];

/// `DES_KEY_SZ`: `sizeof(des_cblock)`.
pub const DES_KEY_SZ: usize = 8;
/// `DES_SCHEDULE_SZ`: `sizeof(des_key_schedule)`, in bytes.
pub const DES_SCHEDULE_SZ: usize = 128;

/// `ITERATIONS`.
pub const ITERATIONS: usize = 16;
/// `HALF_ITERATIONS`.
pub const HALF_ITERATIONS: usize = 8;

/// `des_key_schedule`: the sixteen subkeys of one key, two words each, in the order the rounds
/// of an encryption use them. Built by `DesKeySchedule::new`; zeroed when dropped.
#[derive(Clone, Debug)]
pub struct DesKeySchedule {
    /// The subkeys (`ks[i].deslong` in the C).
    pub(crate) ks: [[u32; 2]; ITERATIONS],
}

impl DesKeySchedule {
    /// Zeroes the subkeys, with stores the compiler keeps (`crate::crypto::wipe`).
    pub(crate) fn zeroize(&mut self) {
        self.ks.iter_mut().flatten().for_each(wipe);
    }
}

impl Drop for DesKeySchedule {
    /// Wipes the subkeys (`docs/IDIOMS.md`: a key schedule is zeroed when dropped).
    fn drop(&mut self) {
        self.zeroize();
    }
}

/// `c2l`: the two little-endian words of the block `c`, low half first.
pub fn c2l(c: &DesCblock) -> [u32; 2] {
    let [a, b, c0, d, e, f, g, h] = *c;
    [
        u32::from_le_bytes([a, b, c0, d]),
        u32::from_le_bytes([e, f, g, h]),
    ]
}

/// `l2c`: the block whose two little-endian halves are `l`, low half first.
pub fn l2c(l: [u32; 2]) -> DesCblock {
    let [a, b, c, d] = l[0].to_le_bytes();
    let [e, f, g, h] = l[1].to_le_bytes();
    [a, b, c, d, e, f, g, h]
}

/// `D_ENCRYPT`: the DES round function of `r` under one subkey; the round xors it into the
/// other half.
pub fn d_encrypt(r: u32, subkey: &[u32; 2]) -> u32 {
    // The six-bit S-box inputs are the low bits of each byte, least significant byte first.
    let u = (r ^ subkey[0]).to_le_bytes();
    let t = (r ^ subkey[1]).rotate_right(4).to_le_bytes();
    let sp = |sbox: usize, x: u8| DES_SPTRANS[sbox][usize::from(x & 0x3f)];
    sp(1, t[0])
        | sp(3, t[1])
        | sp(5, t[2])
        | sp(7, t[3])
        | sp(0, u[0])
        | sp(2, u[1])
        | sp(4, u[2])
        | sp(6, u[3])
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

/// `PERM_OP`: exchanges the bits of `a` and `b` selected by `m`, `n` positions apart; the new
/// `(a, b)`.
pub fn perm_op(a: u32, b: u32, n: u32, m: u32) -> (u32, u32) {
    let t = ((a >> n) ^ b) & m;
    (a ^ (t << n), b ^ t)
}

/// `IP`: the initial permutation of the halves `(l, r)`.
pub fn ip(l: u32, r: u32) -> (u32, u32) {
    let (r, l) = perm_op(r, l, 4, 0x0f0f0f0f);
    let (l, r) = perm_op(l, r, 16, 0x0000ffff);
    let (r, l) = perm_op(r, l, 2, 0x33333333);
    let (l, r) = perm_op(l, r, 8, 0x00ff00ff);
    let (r, l) = perm_op(r, l, 1, 0x55555555);
    (l, r)
}

/// `FP`: the final permutation of the halves `(l, r)`.
pub fn fp(l: u32, r: u32) -> (u32, u32) {
    let (l, r) = perm_op(l, r, 1, 0x55555555);
    let (r, l) = perm_op(r, l, 8, 0x00ff00ff);
    let (l, r) = perm_op(l, r, 2, 0x33333333);
    let (r, l) = perm_op(r, l, 16, 0x0000ffff);
    let (l, r) = perm_op(l, r, 4, 0x0f0f0f0f);
    (l, r)
}

const _: () = assert!(core::mem::size_of::<DesKeySchedule>() == DES_SCHEDULE_SZ);
/* </CODE> */
