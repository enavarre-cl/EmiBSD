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
 * This code implements the MD5 message-digest algorithm.
 * The algorithm is due to Ron Rivest.  This code was
 * written by Colin Plumb in 1993, no copyright is claimed.
 * This code is in the public domain; do with it what you wish.
 *
 * Equivalent code is available from RSA Data Security, Inc.
 * This code has been tested against that, and is equivalent,
 * except that you don't need to include two pages of legalese
 * with every copy.
 */

/*
 * This code implements the MD5 message-digest algorithm.
 * The algorithm is due to Ron Rivest.	This code was
 * written by Colin Plumb in 1993, no copyright is claimed.
 * This code is in the public domain; do with it what you wish.
 *
 * Equivalent code is available from RSA Data Security, Inc.
 * This code has been tested against that, and is equivalent,
 * except that you don't need to include two pages of legalese
 * with every copy.
 *
 * To compute the message digest of a chunk of bytes, declare an
 * MD5Context structure, pass it to MD5Init, call MD5Update as
 * needed on buffers full of bytes, and then call MD5Final, which
 * will fill a supplied 16-byte array with the digest.
 */
/* </LICENSES> */

/* <CODE> */
//! MD5 (RFC 1321), Colin Plumb's public domain implementation: the hash of IPsec's
//! `HMAC-MD5-96` and of the TCP MD5 signature option.
//!
//! Upstream: sys/crypto/md5.h @ 3ce1f3f79392, sys/crypto/md5.c @ 3ce1f3f79392
//! LZ: sys/crypto/md5.rs@f5985f1d055a
//!
//! ## Deviations
//! - The header and the file share this module; `MD5_CTX` (`struct MD5Context`) is [`Md5Ctx`].
//! - `MD5Transform` keeps the C's step order but states the four round functions and the
//!   per-round constants and shifts as tables read by a loop, where the C writes sixty-four
//!   `MD5STEP` lines. The words are always read little-endian (the C copies on little-endian
//!   machines and assembles bytes otherwise).
//!
//! ## Redesign
//! - The context and its three functions are a type with methods (`docs/IDIOMS.md`, "a hash
//!   context"): `MD5Init(&mut ctx)` is [`Md5Ctx::new`], `MD5Update(ctx, buf, len)` is
//!   [`Md5Ctx::update`] over a slice, and `MD5Final(digest, ctx)` is [`Md5Ctx::finalize`],
//!   which returns the digest instead of filling an out parameter. `finalize` takes
//!   `&mut self` and wipes the context where it lives (the C's `explicit_bzero(ctx, ..)`), so
//!   the caller's context holds no message state after it.
//! - The fields are private: the chaining words and the partial block change only through
//!   the methods.
//! - `MD5Transform` takes the state words and one block, not the context, and keeps its name.
//! - The context is not `Copy` and zeroes itself when dropped (`docs/IDIOMS.md`, "a hash
//!   context"), so a by-value pass or a context dropped before `finalize` leaves no copy of
//!   the message state behind; `Clone` stays for a caller that forks a prefixed hash.

use libkern::explicit_bzero;

use super::wipe;

/// `MD5_BLOCK_LENGTH`.
pub const MD5_BLOCK_LENGTH: usize = 64;
/// `MD5_DIGEST_LENGTH`.
pub const MD5_DIGEST_LENGTH: usize = 16;

/// `MD5_CTX`: a hash in progress, from [`Md5Ctx::new`] to [`Md5Ctx::finalize`].
///
/// `Default` is the wiped, all-zero context that `finalize` leaves behind, not the start of a
/// hash. Not `Copy`: dropping a context wipes it.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Md5Ctx {
    /// `state`: the four chaining words.
    state: [u32; 4],
    /// `count`: number of bits, mod 2^64.
    count: u64,
    /// `buffer`: input buffer.
    buffer: [u8; MD5_BLOCK_LENGTH],
}

impl Default for Md5Ctx {
    fn default() -> Self {
        Self {
            state: [0; 4],
            count: 0,
            buffer: [0; MD5_BLOCK_LENGTH],
        }
    }
}

impl Drop for Md5Ctx {
    /// Wipes the context, as `finalize` does, on every path that frees it.
    fn drop(&mut self) {
        self.zeroize();
    }
}

impl Md5Ctx {
    /// Zeroes every field in place: the context becomes the `Default` (wiped) value.
    pub(crate) fn zeroize(&mut self) {
        wipe(&mut self.state);
        wipe(&mut self.count);
        explicit_bzero(&mut self.buffer);
    }

    /// `MD5Init`: start MD5 accumulation. Set bit count to 0 and buffer to mysterious
    /// initialization constants.
    pub fn new() -> Self {
        Self {
            state: [0x67452301, 0xefcdab89, 0x98badcfe, 0x10325476],
            count: 0,
            buffer: [0; MD5_BLOCK_LENGTH],
        }
    }

    /// The bytes waiting in `buffer`: the message length mod 64. The value is below 64, so
    /// the conversion to `usize` is exact.
    fn buffered(&self) -> usize {
        ((self.count >> 3) % MD5_BLOCK_LENGTH as u64) as usize
    }

    /// `MD5Update`: update context to reflect the concatenation of another buffer full of
    /// bytes.
    pub fn update(&mut self, input: &[u8]) {
        let mut input = input;

        // Check how many bytes we already have and how many more we need.
        let mut have = self.buffered();
        let need = MD5_BLOCK_LENGTH - have;

        // Update bitcount
        self.count = self.count.wrapping_add((input.len() as u64) << 3);

        if input.len() >= need {
            if have != 0 {
                self.buffer[have..].copy_from_slice(&input[..need]);
                MD5Transform(&mut self.state, &self.buffer);
                input = &input[need..];
                have = 0;
            }

            // Process data in MD5_BLOCK_LENGTH-byte chunks.
            let (blocks, rest) = input.as_chunks::<MD5_BLOCK_LENGTH>();
            for block in blocks {
                MD5Transform(&mut self.state, block);
            }
            input = rest;
        }

        // Handle any remaining bytes of data.
        self.buffer[have..have + input.len()].copy_from_slice(input);
    }

    /// `MD5Final`: final wrapup - pad to 64-byte boundary with the bit pattern 1 0* (64-bit
    /// count of bits processed, LSB-first) and return the digest; the context is wiped.
    pub fn finalize(&mut self) -> [u8; MD5_DIGEST_LENGTH] {
        // Convert count to 8 bytes in little endian order.
        let count = self.count.to_le_bytes();

        // Pad out to 56 mod 64.
        let mut padlen = MD5_BLOCK_LENGTH - self.buffered();
        if padlen < 1 + 8 {
            padlen += MD5_BLOCK_LENGTH;
        }
        self.update(&PADDING[..padlen - 8]); // padlen - 8 <= 64
        self.update(&count);

        let mut digest = [0u8; MD5_DIGEST_LENGTH];
        for (out, word) in digest.as_chunks_mut::<4>().0.iter_mut().zip(self.state) {
            *out = word.to_le_bytes();
        }
        self.zeroize(); // in case it's sensitive
        digest
    }
}

/// `PADDING`: a 1 bit and zeros.
static PADDING: [u8; MD5_BLOCK_LENGTH] = {
    let mut p = [0u8; MD5_BLOCK_LENGTH];
    p[0] = 0x80;
    p
};

/// The sine-derived addend of each of the sixty-four steps.
const MD5_K: [u32; 64] = [
    0xd76aa478, 0xe8c7b756, 0x242070db, 0xc1bdceee, 0xf57c0faf, 0x4787c62a, 0xa8304613, 0xfd469501,
    0x698098d8, 0x8b44f7af, 0xffff5bb1, 0x895cd7be, 0x6b901122, 0xfd987193, 0xa679438e, 0x49b40821,
    0xf61e2562, 0xc040b340, 0x265e5a51, 0xe9b6c7aa, 0xd62f105d, 0x02441453, 0xd8a1e681, 0xe7d3fbc8,
    0x21e1cde6, 0xc33707d6, 0xf4d50d87, 0x455a14ed, 0xa9e3e905, 0xfcefa3f8, 0x676f02d9, 0x8d2a4c8a,
    0xfffa3942, 0x8771f681, 0x6d9d6122, 0xfde5380c, 0xa4beea44, 0x4bdecfa9, 0xf6bb4b60, 0xbebfbc70,
    0x289b7ec6, 0xeaa127fa, 0xd4ef3085, 0x04881d05, 0xd9d4d039, 0xe6db99e5, 0x1fa27cf8, 0xc4ac5665,
    0xf4292244, 0x432aff97, 0xab9423a7, 0xfc93a039, 0x655b59c3, 0x8f0ccc92, 0xffeff47d, 0x85845dd1,
    0x6fa87e4f, 0xfe2ce6e0, 0xa3014314, 0x4e0811a1, 0xf7537e82, 0xbd3af235, 0x2ad7d2bb, 0xeb86d391,
];

/// The left rotation of each of the sixty-four steps.
const MD5_S: [u32; 64] = [
    7, 12, 17, 22, 7, 12, 17, 22, 7, 12, 17, 22, 7, 12, 17, 22, 5, 9, 14, 20, 5, 9, 14, 20, 5, 9,
    14, 20, 5, 9, 14, 20, 4, 11, 16, 23, 4, 11, 16, 23, 4, 11, 16, 23, 4, 11, 16, 23, 6, 10, 15,
    21, 6, 10, 15, 21, 6, 10, 15, 21, 6, 10, 15, 21,
];

/// The round function of step `i` (the C's `F1` to `F4`): `F1` is optimized somewhat.
fn md5_f(i: usize, x: u32, y: u32, z: u32) -> u32 {
    match i / 16 {
        0 => z ^ (x & (y ^ z)),
        1 => y ^ (z & (x ^ y)),
        2 => x ^ y ^ z,
        _ => y ^ (x | !z),
    }
}

/// The index of the message word step `i` reads.
fn md5_g(i: usize) -> usize {
    match i / 16 {
        0 => i,
        1 => (5 * i + 1) % 16,
        2 => (3 * i + 5) % 16,
        _ => (7 * i) % 16,
    }
}

/// `MD5Transform`: the core of the MD5 algorithm, this alters an existing MD5 hash to reflect
/// the addition of 16 longwords of new data. [`Md5Ctx::update`] blocks the data and converts
/// bytes into longwords for this routine.
#[allow(non_snake_case)] // the C name
pub fn MD5Transform(state: &mut [u32; 4], block: &[u8; MD5_BLOCK_LENGTH]) {
    let mut input = [0u32; MD5_BLOCK_LENGTH / 4];

    for (w, bytes) in input.iter_mut().zip(block.as_chunks::<4>().0) {
        *w = u32::from_le_bytes(*bytes);
    }

    let [mut a, mut b, mut c, mut d] = *state;

    // The C rotates the roles of a, b, c and d between steps (MD5STEP(f, a, b, c, d, ..), then
    // (f, d, a, b, c, ..)); here the values move instead.
    for i in 0..64 {
        let f = md5_f(i, b, c, d)
            .wrapping_add(a)
            .wrapping_add(input[md5_g(i)])
            .wrapping_add(MD5_K[i]);
        a = d;
        d = c;
        c = b;
        b = b.wrapping_add(f.rotate_left(MD5_S[i]));
    }

    for (s, v) in state.iter_mut().zip([a, b, c, d]) {
        *s = s.wrapping_add(v);
    }
}
/* </CODE> */

/* <TESTS> */
#[cfg(test)]
mod tests {
    // Known-answer tests for MD5: the RFC 1321 appendix A.5 test suite, one million "a" and the
    // digest of the digests of every length around the block boundaries (`hashlib`); a
    // property test that any split of a random message gives the one-shot digest.

    use super::*;
    use crate::crypto::testutil::{XorShift, hex};

    extern crate std;
    use std::vec::Vec;

    fn digest(data: &[u8]) -> [u8; MD5_DIGEST_LENGTH] {
        let mut ctx = Md5Ctx::new();
        ctx.update(data);
        ctx.finalize()
    }

    #[test]
    fn rfc1321_test_suite() {
        let cases: &[(&[u8], &str)] = &[
            (b"", "d41d8cd98f00b204e9800998ecf8427e"),
            (b"a", "0cc175b9c0f1b6a831c399e269772661"),
            (b"abc", "900150983cd24fb0d6963f7d28e17f72"),
            (b"message digest", "f96b697d7cb7938d525a2f31aaf161d0"),
            (
                b"abcdefghijklmnopqrstuvwxyz",
                "c3fcd3d76192e4007dfb496cca67e13b",
            ),
            (
                b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789",
                "d174ab98d277d9f5a5611c2c9f419d9f",
            ),
            (
                b"12345678901234567890123456789012345678901234567890123456789012345678901234567890",
                "57edf4a22be3c955ac49da2e2107b67a",
            ),
        ];
        for (msg, want) in cases {
            assert_eq!(digest(msg).to_vec(), hex(want), "{:?}", msg);
        }
    }

    #[test]
    fn longer_messages() {
        assert_eq!(
            digest(b"abcdbcdecdefdefgefghfghighijhijkijkljklmklmnlmnomnopnopq").to_vec(),
            hex("8215ef0796a20bcaaae116d3876c664a")
        );
        assert_eq!(
            digest(b"abcdefghbcdefghicdefghijdefghijkefghijklfghijklmghijklmnhijklmnoijklmnopjklmnopqklmnopqrlmnopqrsmnopqrstnopqrstu").to_vec(),
            hex("03dd8807a93175fb062dfb55dc7d359c")
        );
        let mut ctx = Md5Ctx::new();
        let chunk = [b'a'; 1000];
        for _ in 0..1000 {
            ctx.update(&chunk);
        }
        assert_eq!(
            ctx.finalize().to_vec(),
            hex("7707d6ae4e027c70eea2a935c2296f21")
        );
    }

    #[test]
    fn every_length_around_the_blocks() {
        let mut acc: Vec<u8> = Vec::new();
        for n in 0..(3 * MD5_BLOCK_LENGTH + 2) {
            let msg: Vec<u8> = (0..n).map(|i| ((i * 7 + n) % 256) as u8).collect();
            acc.extend_from_slice(&digest(&msg));
        }
        assert_eq!(
            digest(&acc).to_vec(),
            hex("2cd1d612114900ee87a850b157a5c55e")
        );
    }

    #[test]
    fn any_split_of_the_input_gives_the_same_digest() {
        let msg: Vec<u8> = (0..300).map(|i| (i * 13 % 256) as u8).collect();
        let whole = digest(&msg);
        for chunk in [1usize, 3, 7, 55, 56, 63, 64, 65, 128, 299] {
            let mut ctx = Md5Ctx::new();
            for piece in msg.chunks(chunk) {
                ctx.update(piece);
            }
            assert_eq!(ctx.finalize(), whole, "chunks of {chunk}");
            // finalize wipes the context where it lives.
            assert_eq!(ctx, Md5Ctx::default());
        }
    }

    #[test]
    fn random_splits_give_the_one_shot_digest() {
        let mut rng = XorShift::new(0x6d64_355f_7370_6c74);
        for _ in 0..200 {
            let len = rng.below(600);
            let msg = rng.bytes(len);
            let whole = digest(&msg);
            let mut ctx = Md5Ctx::new();
            for piece in rng.split(&msg) {
                ctx.update(piece);
            }
            assert_eq!(ctx.finalize(), whole, "{} bytes", msg.len());
        }
    }

    #[test]
    fn the_context_is_not_copy() {
        crate::crypto::testutil::assert_not_copy!(Md5Ctx);
    }

    #[test]
    fn zeroize_leaves_the_wiped_context() {
        // What `Drop` runs on a context freed before `finalize`: every field zeroed.
        let mut ctx = Md5Ctx::new();
        ctx.update(b"a message that stays partly in the buffer");
        assert_ne!(ctx, Md5Ctx::default());
        ctx.zeroize();
        assert_eq!(ctx, Md5Ctx::default());
    }

    #[test]
    fn a_clone_forks_the_hash() {
        // `Clone` replaces the `Copy` a caller used to fork a prefixed hash; each side wipes
        // only itself.
        let mut ctx = Md5Ctx::new();
        ctx.update(b"abc");
        let mut fork = ctx.clone();
        fork.update(b"def");
        assert_eq!(fork.finalize(), digest(b"abcdef"));
        assert_eq!(fork, Md5Ctx::default());
        assert_ne!(ctx, Md5Ctx::default());
        assert_eq!(ctx.finalize(), digest(b"abc"));
    }
}
/* </TESTS> */
