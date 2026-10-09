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
 * Copyright (c) 2001 Markus Friedl.  All rights reserved.
 *
 * Redistribution and use in source and binary forms, with or without
 * modification, are permitted provided that the following conditions
 * are met:
 * 1. Redistributions of source code must retain the above copyright
 *    notice, this list of conditions and the following disclaimer.
 * 2. Redistributions in binary form must reproduce the above copyright
 *    notice, this list of conditions and the following disclaimer in the
 *    documentation and/or other materials provided with the distribution.
 *
 * THIS SOFTWARE IS PROVIDED BY THE AUTHOR ``AS IS'' AND ANY EXPRESS OR
 * IMPLIED WARRANTIES, INCLUDING, BUT NOT LIMITED TO, THE IMPLIED WARRANTIES
 * OF MERCHANTABILITY AND FITNESS FOR A PARTICULAR PURPOSE ARE DISCLAIMED.
 * IN NO EVENT SHALL THE AUTHOR BE LIABLE FOR ANY DIRECT, INDIRECT,
 * INCIDENTAL, SPECIAL, EXEMPLARY, OR CONSEQUENTIAL DAMAGES (INCLUDING, BUT
 * NOT LIMITED TO, PROCUREMENT OF SUBSTITUTE GOODS OR SERVICES; LOSS OF USE,
 * DATA, OR PROFITS; OR BUSINESS INTERRUPTION) HOWEVER CAUSED AND ON ANY
 * THEORY OF LIABILITY, WHETHER IN CONTRACT, STRICT LIABILITY, OR TORT
 * (INCLUDING NEGLIGENCE OR OTHERWISE) ARISING IN ANY WAY OUT OF THE USE OF
 * THIS SOFTWARE, EVEN IF ADVISED OF THE POSSIBILITY OF SUCH DAMAGE.
 */
/* </LICENSES> */

/* <CODE> */
//! RIPEMD-160, the hash of IPsec's `HMAC-RIPEMD-160-96`.
//!
//! Upstream: sys/crypto/rmd160.h @ 3ce1f3f79392, sys/crypto/rmd160.c @ 3ce1f3f79392
//! LZ: sys/crypto/rmd160.rs@f5985f1d055a
//!
//! ## Deviations
//! - The header and the file share this module; `RMD160_CTX` is [`Rmd160Ctx`].
//! - `RMD160Transform` runs the eighty steps of each of the two lines in a loop over tables
//!   (message word, rotation, boolean function and constant of each step), taken from the
//!   `R(a, b, c, d, e, F, K, s, r)` lines of the C, and moves the five values along instead of
//!   renaming the variables from step to step as the C does. The words are always read
//!   little-endian.
//!
//! ## Redesign
//! - The context and its three functions are a type with methods (`docs/IDIOMS.md`, "a hash
//!   context"): `RMD160Init` is [`Rmd160Ctx::new`], `RMD160Update` is [`Rmd160Ctx::update`]
//!   over a slice, and `RMD160Final(digest, ctx)` is [`Rmd160Ctx::finalize`], which returns
//!   the digest and wipes the context in place (the C's `explicit_bzero(ctx, ..)`).
//! - The C's `RMD160Final(NULL, ctx)`, which pads and wipes without writing a digest (LZ's
//!   `Option` digest), has no caller; `finalize` always returns the digest, and a caller that
//!   wants only the wipe drops it: the context ends wiped either way.
//! - The fields are private. The whole-block loop of `update` walks `as_chunks` and
//!   transforms the buffer in place instead of copying each block to the stack first.
//! - `RMD160Transform` takes the state words and one block, not the context, and keeps its
//!   name.

use super::wipe;

/// `RMD160_BLOCK_LENGTH`.
pub const RMD160_BLOCK_LENGTH: usize = 64;
/// `RMD160_DIGEST_LENGTH`.
pub const RMD160_DIGEST_LENGTH: usize = 20;

const H0: u32 = 0x67452301;
const H1: u32 = 0xEFCDAB89;
const H2: u32 = 0x98BADCFE;
const H3: u32 = 0x10325476;
const H4: u32 = 0xC3D2E1F0;

/// `K0` to `K4`: the constants of the five rounds of the left line.
const K: [u32; 5] = [0x00000000, 0x5A827999, 0x6ED9EBA1, 0x8F1BBCDC, 0xA953FD4E];

/// `KK0` to `KK4`: the constants of the five rounds of the parallel line.
const KK: [u32; 5] = [0x50A28BE6, 0x5C4DD124, 0x6D703EF3, 0x7A6D76E9, 0x00000000];

/// The message word each step of the left line reads (`X(rj)`).
const R_LEFT: [u8; 80] = [
    0, 1, 2, 3, 4, 5, 6, 7, 8, 9, 10, 11, 12, 13, 14, 15, 7, 4, 13, 1, 10, 6, 15, 3, 12, 0, 9, 5,
    2, 14, 11, 8, 3, 10, 14, 4, 9, 15, 8, 1, 2, 7, 0, 6, 13, 11, 5, 12, 1, 9, 11, 10, 0, 8, 12, 4,
    13, 3, 7, 15, 14, 5, 6, 2, 4, 0, 5, 9, 7, 12, 2, 10, 14, 1, 3, 8, 11, 6, 15, 13,
];

/// The rotation of each step of the left line (`sj`).
const S_LEFT: [u8; 80] = [
    11, 14, 15, 12, 5, 8, 7, 9, 11, 13, 14, 15, 6, 7, 9, 8, 7, 6, 8, 13, 11, 9, 7, 15, 7, 12, 15,
    9, 11, 7, 13, 12, 11, 13, 6, 7, 14, 9, 13, 15, 14, 8, 13, 6, 5, 12, 7, 5, 11, 12, 14, 15, 14,
    15, 9, 8, 9, 14, 5, 6, 8, 6, 5, 12, 9, 15, 5, 11, 6, 8, 13, 12, 5, 12, 13, 14, 11, 8, 5, 6,
];

/// The message word each step of the parallel line reads.
const R_RIGHT: [u8; 80] = [
    5, 14, 7, 0, 9, 2, 11, 4, 13, 6, 15, 8, 1, 10, 3, 12, 6, 11, 3, 7, 0, 13, 5, 10, 14, 15, 8, 12,
    4, 9, 1, 2, 15, 5, 1, 3, 7, 14, 6, 9, 11, 8, 12, 2, 10, 0, 4, 13, 8, 6, 4, 1, 3, 11, 15, 0, 5,
    12, 2, 13, 9, 7, 10, 14, 12, 15, 10, 4, 1, 5, 8, 7, 6, 2, 13, 14, 0, 3, 9, 11,
];

/// The rotation of each step of the parallel line.
const S_RIGHT: [u8; 80] = [
    8, 9, 9, 11, 13, 15, 15, 5, 7, 7, 8, 11, 14, 14, 12, 6, 9, 13, 15, 7, 12, 8, 9, 11, 7, 7, 12,
    7, 6, 15, 13, 11, 9, 7, 15, 11, 8, 6, 6, 14, 12, 13, 5, 14, 13, 13, 7, 5, 15, 5, 8, 11, 14, 14,
    6, 14, 6, 9, 12, 9, 12, 5, 15, 8, 8, 5, 12, 9, 12, 5, 14, 6, 8, 13, 6, 5, 15, 13, 11, 11,
];

/// `PADDING`: a 1 bit and zeros.
static PADDING: [u8; 64] = {
    let mut p = [0u8; 64];
    p[0] = 0x80;
    p
};

/// `RMD160_CTX`: a hash in progress, from [`Rmd160Ctx::new`] to [`Rmd160Ctx::finalize`].
///
/// `Default` is the wiped, all-zero context that `finalize` leaves behind, not the start of a
/// hash.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Rmd160Ctx {
    /// `state`: the five chaining words.
    state: [u32; 5],
    /// `count`: number of bits, mod 2^64.
    count: u64,
    /// `buffer`: input buffer.
    buffer: [u8; RMD160_BLOCK_LENGTH],
}

impl Default for Rmd160Ctx {
    fn default() -> Self {
        Self {
            state: [0; 5],
            count: 0,
            buffer: [0; RMD160_BLOCK_LENGTH],
        }
    }
}

impl Rmd160Ctx {
    /// `RMD160Init`.
    pub fn new() -> Self {
        Self {
            state: [H0, H1, H2, H3, H4],
            count: 0,
            buffer: [0; RMD160_BLOCK_LENGTH],
        }
    }

    /// The bytes waiting in `buffer`: the message length mod 64. The value is below 64, so
    /// the conversion to `usize` is exact.
    fn buffered(&self) -> usize {
        ((self.count / 8) % RMD160_BLOCK_LENGTH as u64) as usize
    }

    /// `RMD160Update`.
    pub fn update(&mut self, input: &[u8]) {
        let mut input = input;
        let mut have = self.buffered();
        let need = RMD160_BLOCK_LENGTH - have;
        self.count = self.count.wrapping_add(8 * input.len() as u64);

        if input.len() >= need {
            if have != 0 {
                let (head, rest) = input.split_at(need);
                self.buffer[have..].copy_from_slice(head);
                RMD160Transform(&mut self.state, &self.buffer);
                input = rest;
                have = 0;
            }
            // now the buffer is empty
            let (blocks, rest) = input.as_chunks::<RMD160_BLOCK_LENGTH>();
            for block in blocks {
                RMD160Transform(&mut self.state, block);
            }
            input = rest;
        }
        self.buffer[have..have + input.len()].copy_from_slice(input);
    }

    /// `RMD160Final`: pads, returns the digest and wipes the context.
    pub fn finalize(&mut self) -> [u8; RMD160_DIGEST_LENGTH] {
        let size = self.count.to_le_bytes();

        // pad to 64 byte blocks, at least one byte from PADDING plus 8 bytes for the size
        let mut padlen = RMD160_BLOCK_LENGTH - self.buffered();
        if padlen < 1 + 8 {
            padlen += RMD160_BLOCK_LENGTH;
        }
        self.update(&PADDING[..padlen - 8]); // padlen - 8 <= 64
        self.update(&size);

        let mut digest = [0u8; RMD160_DIGEST_LENGTH];
        for (out, word) in digest.as_chunks_mut::<4>().0.iter_mut().zip(self.state) {
            *out = word.to_le_bytes();
        }
        wipe(self);
        digest
    }
}

/// The boolean function `Fj` (`F0` to `F4`) of the group of sixteen steps `j`.
fn f(j: usize, x: u32, y: u32, z: u32) -> u32 {
    match j {
        0 => x ^ y ^ z,
        1 => (x & y) | (!x & z),
        2 => (x | !y) ^ z,
        3 => (x & z) | (y & !z),
        _ => x ^ (y | !z),
    }
}

/// `RMD160Transform`: the compression function over one 64-byte block.
#[allow(non_snake_case)] // the C name
pub fn RMD160Transform(state: &mut [u32; 5], block: &[u8; RMD160_BLOCK_LENGTH]) {
    let mut x = [0u32; 16];

    for (i, w) in block.as_chunks::<4>().0.iter().enumerate() {
        x[i] = u32::from_le_bytes(*w);
    }

    // The left line, rounds 1 to 5, then the parallel line, in the opposite order of the
    // boolean functions.
    let mut left = *state;
    let mut right = *state;
    for i in 0..80 {
        let j = i / 16;

        let [a, b, c, d, e] = left;
        let t = a
            .wrapping_add(f(j, b, c, d))
            .wrapping_add(x[R_LEFT[i] as usize])
            .wrapping_add(K[j])
            .rotate_left(u32::from(S_LEFT[i]))
            .wrapping_add(e);
        left = [e, t, b, c.rotate_left(10), d];

        let [a, b, c, d, e] = right;
        let t = a
            .wrapping_add(f(4 - j, b, c, d))
            .wrapping_add(x[R_RIGHT[i] as usize])
            .wrapping_add(KK[j])
            .rotate_left(u32::from(S_RIGHT[i]))
            .wrapping_add(e);
        right = [e, t, b, c.rotate_left(10), d];
    }
    let [aa, bb, cc, dd, ee] = left;
    let [a, b, c, d, e] = right;

    let t = state[1].wrapping_add(cc).wrapping_add(d);
    state[1] = state[2].wrapping_add(dd).wrapping_add(e);
    state[2] = state[3].wrapping_add(ee).wrapping_add(a);
    state[3] = state[4].wrapping_add(aa).wrapping_add(b);
    state[4] = state[0].wrapping_add(bb).wrapping_add(c);
    state[0] = t;
}
/* </CODE> */

/* <TESTS> */
#[cfg(test)]
mod tests {
    // Known-answer tests for RIPEMD-160: the test suite of the RIPEMD-160 specification (Dobbertin,
    // Bosselaers, Preneel) and the digest of the digests of every length around the block
    // boundaries (`hashlib`); a property test that any split of a random message gives the
    // one-shot digest.

    use super::*;
    use crate::crypto::testutil::{XorShift, hex};

    extern crate std;
    use std::vec::Vec;

    fn digest(data: &[u8]) -> [u8; RMD160_DIGEST_LENGTH] {
        let mut ctx = Rmd160Ctx::new();
        ctx.update(data);
        ctx.finalize()
    }

    #[test]
    fn specification_test_suite() {
        let cases: &[(&[u8], &str)] = &[
            (b"", "9c1185a5c5e9fc54612808977ee8f548b2258d31"),
            (b"a", "0bdc9d2d256b3ee9daae347be6f4dc835a467ffe"),
            (b"abc", "8eb208f7e05d987a9b044a8e98c6b087f15a0bfc"),
            (
                b"message digest",
                "5d0689ef49d2fae572b881b123a85ffa21595f36",
            ),
            (
                b"abcdefghijklmnopqrstuvwxyz",
                "f71c27109c692c1b56bbdceb5b9d2865b3708dbc",
            ),
            (
                b"abcdbcdecdefdefgefghfghighijhijkijkljklmklmnlmnomnopnopq",
                "12a053384a9c0c88e405a06c27dcf49ada62eb2b",
            ),
            (
                b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789",
                "b0e20b6e3116640286ed3a87a5713079b21f5189",
            ),
            (
                b"12345678901234567890123456789012345678901234567890123456789012345678901234567890",
                "9b752e45573d4b39f4dbd3323cab82bf63326bfb",
            ),
        ];
        for (msg, want) in cases {
            assert_eq!(digest(msg).to_vec(), hex(want), "{:?}", msg);
        }
    }

    #[test]
    fn one_million_a() {
        let mut ctx = Rmd160Ctx::new();
        let chunk = [b'a'; 1000];
        for _ in 0..1000 {
            ctx.update(&chunk);
        }
        let out = ctx.finalize();
        assert_eq!(
            out.to_vec(),
            hex("52783243c1697bdbe16d37f97f68f08325dc1528")
        );
    }

    #[test]
    fn any_split_of_the_input_gives_the_same_digest() {
        let msg: Vec<u8> = (0..300).map(|i| (i * 13 % 256) as u8).collect();
        let whole = digest(&msg);
        for chunk in [1usize, 3, 7, 55, 56, 63, 64, 65, 128, 299] {
            let mut ctx = Rmd160Ctx::new();
            for piece in msg.chunks(chunk) {
                ctx.update(piece);
            }
            let out = ctx.finalize();
            assert_eq!(out, whole, "chunks of {chunk}");
            // Final wipes the context.
            assert_eq!(ctx, Rmd160Ctx::default());
        }
    }

    #[test]
    fn finalize_wipes_the_state() {
        // The C's `RMD160Final(NULL, ctx)` pads and wipes only; `finalize` wipes the same way
        // whether or not the caller keeps the digest.
        let mut ctx = Rmd160Ctx::new();
        ctx.update(b"secret");
        let _ = ctx.finalize();
        assert_eq!(ctx, Rmd160Ctx::default());
    }

    #[test]
    fn random_splits_give_the_one_shot_digest() {
        let mut rng = XorShift::new(0x726d_6431_3630_7370);
        for _ in 0..200 {
            let len = rng.below(600);
            let msg = rng.bytes(len);
            let whole = digest(&msg);
            let mut ctx = Rmd160Ctx::new();
            for piece in rng.split(&msg) {
                ctx.update(piece);
            }
            assert_eq!(ctx.finalize(), whole, "{len} bytes");
        }
    }
}
/* </TESTS> */
