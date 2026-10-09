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
 * SHA-1 in C
 * By Steve Reid <steve@edmweb.com>
 * 100% Public Domain
 */

/*
 * SHA-1 in C
 * By Steve Reid <steve@edmweb.com>
 * 100% Public Domain
 *
 * Test Vectors (from FIPS PUB 180-1)
 * "abc"
 *   A9993E36 4706816A BA3E2571 7850C26C 9CD0D89D
 * "abcdbcdecdefdefgefghfghighijhijkijkljklmklmnlmnomnopnopq"
 *   84983E44 1C3BD26E BAAE4AA1 F95129E5 E54670F1
 * A million repetitions of "a"
 *   34AA973C D4C4DAA4 F61EEB2B DBAD2731 6534016F
*/
/* </LICENSES> */

/* <CODE> */
//! SHA-1 (FIPS 180-4), Steve Reid's public domain implementation: the hash of IPsec's
//! `HMAC-SHA1-96` and of `hmac.c`.
//!
//! Upstream: sys/crypto/sha1.h @ 3ce1f3f79392, sys/crypto/sha1.c @ 3ce1f3f79392
//! LZ: sys/crypto/sha1.rs@f5985f1d055a
//!
//! ## Deviations
//! - The header and the file share this module; `SHA1_CTX` is [`Sha1Ctx`].
//! - The C's `len << 3` is 32-bit arithmetic (`unsigned int`) and wraps for a message of
//!   512 MiB or more before it is added to the 64-bit count; here the shift is done in 64
//!   bits.
//! - `SHA1Transform` expands the message schedule into an 80-word array and runs the rounds
//!   in a loop; the C expands in place in a 16-word window with the rounds unrolled by macro
//!   (`SHA1HANDSOFF` is always on: the block is copied). Both compute FIPS 180-4's function.
//!
//! ## Redesign
//! - The context and its three functions are a type with methods (`docs/IDIOMS.md`, "a hash
//!   context"): `SHA1Init` is [`Sha1Ctx::new`], `SHA1Update` is [`Sha1Ctx::update`] over a
//!   slice, and `SHA1Final(digest, ctx)` is [`Sha1Ctx::finalize`], which returns the digest
//!   and wipes the context in place (and the length block, as the C wipes `finalcount`).
//! - The fields are private. `update` walks whole blocks with `as_chunks` instead of index
//!   arithmetic over `i` and `j`; `finalize` appends the padding with one `update` of the
//!   needed zeros instead of one byte at a time, which feeds the same bytes.
//! - `SHA1Transform` takes the state words and one block, not the context, and keeps its name.

use libkern::explicit_bzero;

use super::wipe;

/// `SHA1_BLOCK_LENGTH`.
pub const SHA1_BLOCK_LENGTH: usize = 64;
/// `SHA1_DIGEST_LENGTH`.
pub const SHA1_DIGEST_LENGTH: usize = 20;

/// `SHA1_CTX`: a hash in progress, from [`Sha1Ctx::new`] to [`Sha1Ctx::finalize`].
///
/// `Default` is the wiped, all-zero context that `finalize` leaves behind, not the start of a
/// hash.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Sha1Ctx {
    /// `state`: the five chaining words.
    state: [u32; 5],
    /// `count`: the message length so far, in bits.
    count: u64,
    /// `buffer`: the partial block.
    buffer: [u8; SHA1_BLOCK_LENGTH],
}

impl Default for Sha1Ctx {
    fn default() -> Self {
        Self {
            state: [0; 5],
            count: 0,
            buffer: [0; SHA1_BLOCK_LENGTH],
        }
    }
}

impl Sha1Ctx {
    /// `SHA1Init`: initialize new context.
    pub fn new() -> Self {
        Self {
            // SHA1 initialization constants
            state: [0x67452301, 0xEFCDAB89, 0x98BADCFE, 0x10325476, 0xC3D2E1F0],
            count: 0,
            buffer: [0; SHA1_BLOCK_LENGTH],
        }
    }

    /// The bytes waiting in `buffer`: the message length mod 64. The value is below 64, so
    /// the conversion to `usize` is exact.
    fn buffered(&self) -> usize {
        ((self.count >> 3) % SHA1_BLOCK_LENGTH as u64) as usize
    }

    /// `SHA1Update`: run your data through this.
    pub fn update(&mut self, data: &[u8]) {
        let mut data = data;
        let mut j = self.buffered();

        self.count = self.count.wrapping_add((data.len() as u64) << 3);
        if j + data.len() >= SHA1_BLOCK_LENGTH {
            let (head, rest) = data.split_at(SHA1_BLOCK_LENGTH - j);
            self.buffer[j..].copy_from_slice(head);
            SHA1Transform(&mut self.state, &self.buffer);
            let (blocks, tail) = rest.as_chunks::<SHA1_BLOCK_LENGTH>();
            for block in blocks {
                SHA1Transform(&mut self.state, block);
            }
            data = tail;
            j = 0;
        }
        self.buffer[j..j + data.len()].copy_from_slice(data);
    }

    /// `SHA1Final`: add padding and return the message digest; the context is wiped.
    pub fn finalize(&mut self) -> [u8; SHA1_DIGEST_LENGTH] {
        // Endian independent
        let mut finalcount = self.count.to_be_bytes();

        // A 1 bit, then zeros up to 56 mod 64 (the C feeds the zeros one at a time).
        self.update(b"\x80");
        let zeros = (SHA1_BLOCK_LENGTH + 56 - self.buffered()) % SHA1_BLOCK_LENGTH;
        self.update(&[0u8; SHA1_BLOCK_LENGTH][..zeros]);
        self.update(&finalcount); // Should cause a SHA1Transform()

        let mut digest = [0u8; SHA1_DIGEST_LENGTH];
        for (out, word) in digest.as_chunks_mut::<4>().0.iter_mut().zip(self.state) {
            *out = word.to_be_bytes();
        }
        explicit_bzero(&mut finalcount);
        wipe(self);
        digest
    }
}

/// `SHA1Transform`: hash a single 512-bit block. This is the core of the algorithm.
#[allow(non_snake_case)] // the C name
pub fn SHA1Transform(state: &mut [u32; 5], buffer: &[u8; SHA1_BLOCK_LENGTH]) {
    let mut w = [0u32; 80];

    for (wi, word) in w.iter_mut().zip(buffer.as_chunks::<4>().0) {
        *wi = u32::from_be_bytes(*word);
    }
    for i in 16..80 {
        w[i] = (w[i - 3] ^ w[i - 8] ^ w[i - 14] ^ w[i - 16]).rotate_left(1);
    }

    // Copy context->state[] to working vars
    let [mut a, mut b, mut c, mut d, mut e] = *state;

    // 4 rounds of 20 operations each.
    for (i, wi) in w.iter().enumerate() {
        let (f, k) = match i {
            0..=19 => (((b & (c ^ d)) ^ d), 0x5A827999u32),
            20..=39 => (b ^ c ^ d, 0x6ED9EBA1),
            40..=59 => ((((b | c) & d) | (b & c)), 0x8F1BBCDC),
            _ => (b ^ c ^ d, 0xCA62C1D6),
        };
        let t = a
            .rotate_left(5)
            .wrapping_add(f)
            .wrapping_add(e)
            .wrapping_add(k)
            .wrapping_add(*wi);
        e = d;
        d = c;
        c = b.rotate_left(30);
        b = a;
        a = t;
    }

    // Add the working vars back into context.state[]
    for (s, v) in state.iter_mut().zip([a, b, c, d, e]) {
        *s = s.wrapping_add(v);
    }
}
/* </CODE> */

/* <TESTS> */
#[cfg(test)]
mod tests {
    // Known-answer tests for SHA-1: the FIPS 180-4 examples ("abc", the 448-bit message, one
    // million "a"), and digests around the block boundaries computed with Python's `hashlib`;
    // a property test that any split of a random message gives the one-shot digest.

    use super::*;
    use crate::crypto::testutil::{XorShift, hex};

    extern crate std;
    use std::vec::Vec;

    fn digest(data: &[u8]) -> [u8; SHA1_DIGEST_LENGTH] {
        let mut ctx = Sha1Ctx::new();
        ctx.update(data);
        ctx.finalize()
    }

    #[test]
    fn fips_180_4_examples() {
        assert_eq!(
            digest(b"abc").to_vec(),
            hex("a9993e364706816aba3e25717850c26c9cd0d89d")
        );
        assert_eq!(
            digest(b"abcdbcdecdefdefgefghfghighijhijkijkljklmklmnlmnomnopnopq").to_vec(),
            hex("84983e441c3bd26ebaae4aa1f95129e5e54670f1")
        );
        assert_eq!(
        digest(b"abcdefghbcdefghicdefghijdefghijkefghijklfghijklmghijklmnhijklmnoijklmnopjklmnopqklmnopqrlmnopqrsmnopqrstnopqrstu").to_vec(),
        hex("a49b2446a02c645bf419f995b67091253a04a259")
    );
    }

    #[test]
    fn one_million_a() {
        let mut ctx = Sha1Ctx::new();
        let chunk = [b'a'; 1000];
        for _ in 0..1000 {
            ctx.update(&chunk);
        }
        let out = ctx.finalize();
        assert_eq!(
            out.to_vec(),
            hex("34aa973cd4c4daa4f61eeb2bdbad27316534016f")
        );
    }

    #[test]
    fn short_messages() {
        let cases: &[(&[u8], &str)] = &[
            (b"", "da39a3ee5e6b4b0d3255bfef95601890afd80709"),
            (b"a", "86f7e437faa5a7fce15d1ddcb9eaeaea377667b8"),
            (
                b"message digest",
                "c12252ceda8be8994d5fa0290a47231c1d16aae3",
            ),
            (
                b"abcdefghijklmnopqrstuvwxyz",
                "32d10c7b8cf96570ca04ce37f2a19d84240d3a89",
            ),
            (
                b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789",
                "761c457bf73b14d27e9e9265c46f4b4dda11f940",
            ),
            (
                b"12345678901234567890123456789012345678901234567890123456789012345678901234567890",
                "50abf5706a150990a08b2c5ea40fa0e585554732",
            ),
        ];
        for (msg, want) in cases {
            assert_eq!(digest(msg).to_vec(), hex(want), "{:?}", msg);
        }
    }

    /// The digest of the digests of the messages of every length from 0 to 3 blocks + 1.
    #[test]
    fn every_length_around_the_blocks() {
        let mut acc: Vec<u8> = Vec::new();
        for n in 0..(3 * SHA1_BLOCK_LENGTH + 2) {
            let msg: Vec<u8> = (0..n).map(|i| ((i * 7 + n) % 256) as u8).collect();
            acc.extend_from_slice(&digest(&msg));
        }
        assert_eq!(
            digest(&acc).to_vec(),
            hex("5bc3728d76facfc871e5e0ccc48f64b392d96902")
        );
    }

    #[test]
    fn any_split_of_the_input_gives_the_same_digest() {
        let msg: Vec<u8> = (0..300).map(|i| (i * 13 % 256) as u8).collect();
        let whole = digest(&msg);
        for chunk in [1usize, 3, 7, 55, 56, 63, 64, 65, 128, 299] {
            let mut ctx = Sha1Ctx::new();
            for piece in msg.chunks(chunk) {
                ctx.update(piece);
            }
            let out = ctx.finalize();
            assert_eq!(out, whole, "chunks of {chunk}");
            // Final wipes the context.
            assert_eq!(ctx, Sha1Ctx::default());
        }
    }

    #[test]
    fn random_splits_give_the_one_shot_digest() {
        let mut rng = XorShift::new(0x7368_6131_7370_6c74);
        for _ in 0..200 {
            let len = rng.below(600);
            let msg = rng.bytes(len);
            let whole = digest(&msg);
            let mut ctx = Sha1Ctx::new();
            for piece in rng.split(&msg) {
                ctx.update(piece);
            }
            assert_eq!(ctx.finalize(), whole, "{len} bytes");
        }
    }
}
/* </TESTS> */
