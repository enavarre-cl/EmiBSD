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
 * Copyright (C) 2015-2020 Jason A. Donenfeld <Jason@zx2c4.com>. All Rights Reserved.
 * Copyright (C) 2019-2020 Matt Dunwoodie <ncon@noconroy.net>.
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
 * Copyright (C) 2012 Samuel Neves <sneves@dei.uc.pt>. All Rights Reserved.
 * Copyright (C) 2015-2020 Jason A. Donenfeld <Jason@zx2c4.com>. All Rights Reserved.
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
 * This is an implementation of the BLAKE2s hash and PRF functions.
 * Information: https://blake2.net/
 */
/* </LICENSES> */

/* <CODE> */
//! BLAKE2s (RFC 7693), the hash and keyed PRF WireGuard builds its Noise handshake and cookies
//! on, and an HMAC over it (`blake2s_hmac`, the `HMAC-BLAKE2s` of the Noise `KDF`).
//!
//! Upstream: sys/crypto/blake2s.h @ 3ce1f3f79392, sys/crypto/blake2s.c @ 3ce1f3f79392
//! LZ: sys/crypto/blake2s.rs@f5985f1d055a
//!
//! ## Deviations
//! - The header and the file share this module.
//! - `enum blake2s_lengths` is three constants.
//! - The `KASSERT`s are `kassert!`; the stack buffers holding key material are wiped with
//!   `explicit_bzero`, the state through `crate::crypto::wipe`.
//!
//! ## Redesign
//! - `struct blake2s_state` and its functions are a type with methods (`docs/IDIOMS.md`, "a
//!   hash context"): `blake2s_init(state, outlen)` is [`Blake2sState::new`],
//!   `blake2s_init_key(state, outlen, key, keylen)` is [`Blake2sState::new_keyed`],
//!   `blake2s_update` is [`Blake2sState::update`] over a slice, and `blake2s_final(state, out)`
//!   is [`Blake2sState::finalize`], which returns the digest and wipes the state in place (the
//!   C's `explicit_bzero(state, ..)`). The static helpers (`blake2s_init_param`,
//!   `blake2s_increment_counter`, `blake2s_set_lastblock`, `blake2s_compress`) are private
//!   methods.
//! - The digest length `outlen`, a run-time parameter stored in the state and checked by
//!   `KASSERT(outlen && outlen <= BLAKE2S_HASH_SIZE)`, is the state's const parameter
//!   `Blake2sState<N>`: the check is a compile-time assertion (`new`, `new_keyed`), the
//!   `outlen` field is gone, and `finalize` returns exactly `[u8; N]` instead of writing
//!   `outlen` bytes into a caller's buffer of unchecked length. Every caller's length was a
//!   constant (`NOISE_HASH_LEN`, `COOKIE_MAC_SIZE`, `COOKIE_COOKIE_SIZE`, `COOKIE_KEY_SIZE`).
//! - The one-call [`blake2s`] and [`blake2s_hmac`] return `[u8; N]` the same way (the HMAC's
//!   `outlen <= BLAKE2S_HASH_SIZE`, unchecked in the C, is the same compile-time assertion).
//!   LZ's `blake2s_hmac_inplace` (the C's `blake2s_hmac(out, out, ..)`, input and output one
//!   buffer, as `noise_kdf` uses it) and its `blake2s_hmac_digest` helper merge into
//!   `blake2s_hmac`: the result is a value, so the caller reads the message from its buffer
//!   and then writes the MAC over it, with no aliasing to describe.
//! - `blake2s_compress(state, block, nblocks, inc)` takes whole blocks
//!   (`&[[u8; BLAKE2S_BLOCK_SIZE]]`), so `nblocks` is their count and no block can be short;
//!   its `KASSERT` on `inc` is kept. `buflen` is a `usize`.
//! - A keyed state holds key-derived material (the chained value after the key block, and
//!   the key block itself in `buf` until the next block arrives), so the state is not `Copy`
//!   and zeroes itself when dropped (`docs/IDIOMS.md`, "a hash context"): a by-value pass or
//!   a state dropped before `finalize` leaves no copy of it.

use libkern::explicit_bzero;

use super::wipe;
use crate::kassert;

/// `BLAKE2S_BLOCK_SIZE`: `enum blake2s_lengths`.
pub const BLAKE2S_BLOCK_SIZE: usize = 64;
/// `BLAKE2S_HASH_SIZE`.
pub const BLAKE2S_HASH_SIZE: usize = 32;
/// `BLAKE2S_KEY_SIZE`.
pub const BLAKE2S_KEY_SIZE: usize = 32;

/// `blake2s_iv`.
const BLAKE2S_IV: [u32; 8] = [
    0x6A09E667, 0xBB67AE85, 0x3C6EF372, 0xA54FF53A, 0x510E527F, 0x9B05688C, 0x1F83D9AB, 0x5BE0CD19,
];

/// `blake2s_sigma`.
const BLAKE2S_SIGMA: [[u8; 16]; 10] = [
    [0, 1, 2, 3, 4, 5, 6, 7, 8, 9, 10, 11, 12, 13, 14, 15],
    [14, 10, 4, 8, 9, 15, 13, 6, 1, 12, 0, 2, 11, 7, 5, 3],
    [11, 8, 12, 0, 5, 2, 15, 13, 10, 14, 3, 6, 7, 1, 9, 4],
    [7, 9, 3, 1, 13, 12, 11, 14, 2, 6, 5, 10, 4, 0, 15, 8],
    [9, 0, 5, 7, 2, 4, 10, 15, 14, 1, 11, 12, 6, 8, 3, 13],
    [2, 12, 6, 10, 0, 11, 8, 3, 4, 13, 7, 5, 15, 14, 1, 9],
    [12, 5, 1, 15, 14, 13, 4, 10, 0, 7, 6, 3, 9, 2, 8, 11],
    [13, 11, 7, 14, 12, 1, 3, 9, 5, 0, 15, 4, 8, 6, 2, 10],
    [6, 15, 14, 9, 11, 3, 0, 8, 12, 2, 13, 7, 1, 4, 10, 5],
    [10, 2, 8, 4, 7, 6, 1, 5, 15, 11, 9, 14, 3, 12, 13, 0],
];

/// `struct blake2s_state`: a hash in progress with an `N`-byte digest (1 to
/// [`BLAKE2S_HASH_SIZE`], the C's `outlen`), from [`Blake2sState::new`] or
/// [`Blake2sState::new_keyed`] to [`Blake2sState::finalize`].
///
/// `Default` is the wiped, all-zero state `finalize` leaves behind, not the start of a hash.
/// Not `Copy`: dropping a state wipes it.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Blake2sState<const N: usize> {
    /// `h`: the chained value.
    h: [u32; 8],
    /// `t`: the 64-bit byte counter.
    t: [u32; 2],
    /// `f`: the finalisation flags.
    f: [u32; 2],
    /// `buf`: the block not yet compressed.
    buf: [u8; BLAKE2S_BLOCK_SIZE],
    /// `buflen`: bytes in `buf`, at most a block.
    buflen: usize,
}

impl<const N: usize> Default for Blake2sState<N> {
    fn default() -> Self {
        Self {
            h: [0; 8],
            t: [0; 2],
            f: [0; 2],
            buf: [0; BLAKE2S_BLOCK_SIZE],
            buflen: 0,
        }
    }
}

impl<const N: usize> Drop for Blake2sState<N> {
    /// Wipes the state, as `finalize` does, on every path that frees it.
    fn drop(&mut self) {
        self.zeroize();
    }
}

impl<const N: usize> Blake2sState<N> {
    /// Zeroes every field in place: the state becomes the `Default` (wiped) value.
    pub(crate) fn zeroize(&mut self) {
        wipe(&mut self.h);
        wipe(&mut self.t);
        wipe(&mut self.f);
        explicit_bzero(&mut self.buf);
        wipe(&mut self.buflen);
    }

    /// `blake2s_set_lastblock`.
    fn set_lastblock(&mut self) {
        self.f[0] = u32::MAX;
    }

    /// `blake2s_increment_counter`.
    fn increment_counter(&mut self, inc: u32) {
        self.t[0] = self.t[0].wrapping_add(inc);
        self.t[1] = self.t[1].wrapping_add(u32::from(self.t[0] < inc));
    }

    /// `blake2s_init_param`: the zeroed state with the IV xor the parameter block's first word.
    fn init_param(param: u32) -> Self {
        let mut h = BLAKE2S_IV;
        h[0] ^= param;
        Self {
            h,
            ..Self::default()
        }
    }

    /// The parameter block's first word: digest length, key length, fanout 1, depth 1. Both
    /// lengths are at most 32 (checked by the callers), so the conversions are exact.
    fn param(keylen: usize) -> u32 {
        0x01010000 | (keylen as u32) << 8 | N as u32
    }

    /// `blake2s_init`: an unkeyed hash with an `N`-byte digest.
    pub fn new() -> Self {
        // The C's `KASSERT(outlen && outlen <= BLAKE2S_HASH_SIZE)`, at compile time.
        const { assert!(N >= 1 && N <= BLAKE2S_HASH_SIZE) };
        Self::init_param(Self::param(0))
    }

    /// `blake2s_init_key`: a keyed hash (the PRF) under a key of 1 to [`BLAKE2S_KEY_SIZE`]
    /// bytes; the key is padded with zeros to a block that is hashed first.
    pub fn new_keyed(key: &[u8]) -> Self {
        // The C's `KASSERT(outlen && outlen <= BLAKE2S_HASH_SIZE ..)`, at compile time.
        const { assert!(N >= 1 && N <= BLAKE2S_HASH_SIZE) };
        let keylen = key.len();
        let mut block = [0u8; BLAKE2S_BLOCK_SIZE];

        kassert!(!(keylen == 0 || keylen > BLAKE2S_KEY_SIZE));

        let mut state = Self::init_param(Self::param(keylen));
        block[..keylen].copy_from_slice(key);
        state.update(&block);
        explicit_bzero(&mut block);
        state
    }

    /// `blake2s_compress`: absorbs the whole `blocks`, counting `inc` bytes for each.
    fn compress(&mut self, blocks: &[[u8; BLAKE2S_BLOCK_SIZE]], inc: u32) {
        let mut m = [0u32; 16];
        let mut v = [0u32; 16];

        kassert!(!(blocks.len() > 1 && inc != BLAKE2S_BLOCK_SIZE as u32));

        for blk in blocks {
            self.increment_counter(inc);
            for (w, bytes) in m.iter_mut().zip(blk.as_chunks::<4>().0) {
                *w = u32::from_le_bytes(*bytes);
            }
            v[..8].copy_from_slice(&self.h);
            v[8..].copy_from_slice(&BLAKE2S_IV);
            v[12] ^= self.t[0];
            v[13] ^= self.t[1];
            v[14] ^= self.f[0];
            v[15] ^= self.f[1];

            // G(r, i, a, b, c, d)
            let g =
                |v: &mut [u32; 16], r: usize, i: usize, a: usize, b: usize, c: usize, d: usize| {
                    let s = &BLAKE2S_SIGMA[r];
                    v[a] = v[a]
                        .wrapping_add(v[b])
                        .wrapping_add(m[usize::from(s[2 * i])]);
                    v[d] = (v[d] ^ v[a]).rotate_right(16);
                    v[c] = v[c].wrapping_add(v[d]);
                    v[b] = (v[b] ^ v[c]).rotate_right(12);
                    v[a] = v[a]
                        .wrapping_add(v[b])
                        .wrapping_add(m[usize::from(s[2 * i + 1])]);
                    v[d] = (v[d] ^ v[a]).rotate_right(8);
                    v[c] = v[c].wrapping_add(v[d]);
                    v[b] = (v[b] ^ v[c]).rotate_right(7);
                };

            for r in 0..10 {
                g(&mut v, r, 0, 0, 4, 8, 12);
                g(&mut v, r, 1, 1, 5, 9, 13);
                g(&mut v, r, 2, 2, 6, 10, 14);
                g(&mut v, r, 3, 3, 7, 11, 15);
                g(&mut v, r, 4, 0, 5, 10, 15);
                g(&mut v, r, 5, 1, 6, 11, 12);
                g(&mut v, r, 6, 2, 7, 8, 13);
                g(&mut v, r, 7, 3, 4, 9, 14);
            }

            for (i, h) in self.h.iter_mut().enumerate() {
                *h ^= v[i] ^ v[i + 8];
            }
        }
    }

    /// `blake2s_update`: adds `input` to the message.
    pub fn update(&mut self, input: &[u8]) {
        let mut input = input;
        let fill = BLAKE2S_BLOCK_SIZE - self.buflen;

        if input.is_empty() {
            return;
        }
        if input.len() > fill {
            let (head, rest) = input.split_at(fill);
            self.buf[self.buflen..].copy_from_slice(head);
            let buf = self.buf;
            self.compress(&[buf], BLAKE2S_BLOCK_SIZE as u32);
            self.buflen = 0;
            input = rest;
        }
        if input.len() > BLAKE2S_BLOCK_SIZE {
            // Hash one less (full) block than strictly possible
            let nblocks = input.len().div_ceil(BLAKE2S_BLOCK_SIZE);
            let (blocks, _) = input.as_chunks::<BLAKE2S_BLOCK_SIZE>();
            self.compress(&blocks[..nblocks - 1], BLAKE2S_BLOCK_SIZE as u32);
            input = &input[BLAKE2S_BLOCK_SIZE * (nblocks - 1)..];
        }
        self.buf[self.buflen..self.buflen + input.len()].copy_from_slice(input);
        self.buflen += input.len();
    }

    /// `blake2s_final`: pads and compresses the last block and returns the `N`-byte digest;
    /// the state is wiped.
    pub fn finalize(&mut self) -> [u8; N] {
        self.set_lastblock();
        self.buf[self.buflen..].fill(0); // Padding
        let buf = self.buf;
        // `buflen` is at most a block, so the conversion is exact.
        self.compress(&[buf], self.buflen as u32);

        let mut full = [0u8; BLAKE2S_HASH_SIZE];
        for (out, word) in full.as_chunks_mut::<4>().0.iter_mut().zip(self.h) {
            *out = word.to_le_bytes();
        }
        let mut digest = [0u8; N];
        digest.copy_from_slice(&full[..N]);
        explicit_bzero(&mut full);
        self.zeroize();
        digest
    }
}

/// `blake2s`: the `N`-byte hash of `input` in one call, keyed when `key` is not empty.
pub fn blake2s<const N: usize>(input: &[u8], key: &[u8]) -> [u8; N] {
    kassert!(key.len() <= BLAKE2S_KEY_SIZE);

    let mut state = if !key.is_empty() {
        Blake2sState::<N>::new_keyed(key)
    } else {
        Blake2sState::<N>::new()
    };

    state.update(input);
    state.finalize()
}

/// `blake2s_hmac`: HMAC (RFC 2104) with BLAKE2s-256 as the hash; the first `N` bytes of the
/// MAC (`N` at most [`BLAKE2S_HASH_SIZE`], checked at compile time).
pub fn blake2s_hmac<const N: usize>(input: &[u8], key: &[u8]) -> [u8; N] {
    const { assert!(N <= BLAKE2S_HASH_SIZE) };
    let mut x_key = [0u8; BLAKE2S_BLOCK_SIZE];

    if key.len() > BLAKE2S_BLOCK_SIZE {
        let mut state = Blake2sState::<BLAKE2S_HASH_SIZE>::new();
        state.update(key);
        let mut hashed = state.finalize();
        x_key[..BLAKE2S_HASH_SIZE].copy_from_slice(&hashed);
        explicit_bzero(&mut hashed);
    } else {
        x_key[..key.len()].copy_from_slice(key);
    }

    for b in x_key.iter_mut() {
        *b ^= 0x36;
    }

    let mut state = Blake2sState::<BLAKE2S_HASH_SIZE>::new();
    state.update(&x_key);
    state.update(input);
    let mut i_hash = state.finalize();

    for b in x_key.iter_mut() {
        *b ^= 0x5c ^ 0x36;
    }

    let mut state = Blake2sState::<BLAKE2S_HASH_SIZE>::new();
    state.update(&x_key);
    state.update(&i_hash);
    let mut o_hash = state.finalize();

    let mut out = [0u8; N];
    out.copy_from_slice(&o_hash[..N]);
    explicit_bzero(&mut x_key);
    explicit_bzero(&mut i_hash);
    explicit_bzero(&mut o_hash);
    out
}
/* </CODE> */

/* <TESTS> */
#[cfg(test)]
mod tests {
    // Known-answer tests for BLAKE2s: RFC 7693 appendix B ("abc") and digests of messages around
    // the block size, plain and keyed, with the HMAC built on it; the expected values come from
    // Python's `hashlib.blake2s` and `hmac`. A property test: random keys (none or 1 to 32
    // bytes) and random splits give the one-call hash, for 32- and 16-byte digests.

    use super::*;
    use crate::crypto::testutil::{XorShift, c_table, hex};

    extern crate std;
    use std::vec::Vec;

    fn data() -> [u8; 300] {
        let mut d = [0u8; 300];
        for (i, b) in d.iter_mut().enumerate() {
            *b = (i % 251) as u8;
        }
        d
    }

    fn key() -> [u8; 32] {
        let mut k = [0u8; 32];
        for (i, b) in k.iter_mut().enumerate() {
            *b = i as u8;
        }
        k
    }

    fn hash<const N: usize>(input: &[u8], key: &[u8]) -> Vec<u8> {
        blake2s::<N>(input, key).to_vec()
    }

    #[test]
    fn rfc7693_appendix_b() {
        assert_eq!(
            hash::<32>(b"abc", &[]),
            hex("508c5e8c327c14e2e1a72ba34eeb452f37458b209ed63a294d999b4c86675982")
        );
        assert_eq!(
            hash::<32>(b"", &[]),
            hex("69217a3079908094e11121d042354a7c1f55b6482ca1a51e1b250dfd1ed0eef9")
        );
    }

    const PLAIN_AND_KEYED: &[(usize, &str, &str)] = &[
        (
            1,
            "e34d74dbaf4ff4c6abd871cc220451d2ea2648846c7757fbaac82fe51ad64bea",
            "40d15fee7c328830166ac3f918650f807e7e01e177258cdc0a39b11f598066f1",
        ),
        (
            63,
            "e57cb79487dd57902432b250733813bd96a84efce59f650fac26e6696aefafc3",
            "c65382513f07460da39833cb666c5ed82e61b9e998f4b0c4287cee56c3cc9bcd",
        ),
        (
            64,
            "56f34e8b96557e90c1f24b52d0c89d51086acf1b00f634cf1dde9233b8eaaa3e",
            "8975b0577fd35566d750b362b0897a26c399136df07bababbde6203ff2954ed4",
        ),
        (
            65,
            "1b53ee94aaf34e4b159d48de352c7f0661d0a40edff95a0b1639b4090e974472",
            "21fe0ceb0052be7fb0f004187cacd7de67fa6eb0938d927677f2398c132317a8",
        ),
        (
            127,
            "f18417b39d617ab1c18fdf91ebd0fc6d5516bb34cf39364037bce81fa04cecb1",
            "ddbfea75cc467882eb3483ce5e2e756a4f4701b76b445519e89f22d60fa86e06",
        ),
        (
            128,
            "1fa877de67259d19863a2a34bcc6962a2b25fcbf5cbecd7ede8f1fa36688a796",
            "0c311f38c35a4fb90d651c289d486856cd1413df9b0677f53ece2cd9e477c60a",
        ),
        (
            129,
            "5bd169e67c82c2c2e98ef7008bdf261f2ddf30b1c00f9e7f275bb3e8a28dc9a2",
            "46a73a8dd3e70f59d3942c01df599def783c9da82fd83222cd662b53dce7dbdf",
        ),
        (
            300,
            "203d6441349213c6b2b727d0ac6beb365f32938cef2cd0677f2a6b7f8cb02dd7",
            "dc125084fe1a9fcbe234e1c350f6a9efe99fbf16ca47d8488ae39c1c9434037e",
        ),
    ];

    #[test]
    fn lengths_around_the_block_size() {
        let d = data();
        let k = key();
        for (n, plain, keyed) in PLAIN_AND_KEYED {
            assert_eq!(hash::<32>(&d[..*n], &[]), hex(plain), "plain {n}");
            assert_eq!(hash::<32>(&d[..*n], &k), hex(keyed), "keyed {n}");
        }
        assert_eq!(
            hash::<32>(b"", &k),
            hex("48a8997da407876b3d79c0d92325ad3b89cbb754d86ab71aee047ad345fd2c49")
        );
    }

    #[test]
    fn any_split_of_the_input_gives_the_same_digest() {
        let d = data();
        let k = key();
        for (n, plain, keyed) in PLAIN_AND_KEYED {
            for chunk in [1usize, 5, 63, 64, 65, 100] {
                let mut st = Blake2sState::<32>::new();
                for piece in d[..*n].chunks(chunk) {
                    st.update(piece);
                }
                assert_eq!(st.finalize().to_vec(), hex(plain), "plain {n} by {chunk}");

                let mut st = Blake2sState::<32>::new_keyed(&k);
                for piece in d[..*n].chunks(chunk) {
                    st.update(piece);
                }
                assert_eq!(st.finalize().to_vec(), hex(keyed), "keyed {n} by {chunk}");
            }
        }
    }

    #[test]
    fn a_shorter_digest_is_a_different_hash_not_a_truncation() {
        // WireGuard's cookies use 16-byte keyed outputs: the digest length is in the parameter block.
        let d = data();
        assert_eq!(
            hash::<16>(&d[..100], &key()[..16]),
            hex("56301549e674c3b72a0e1dafb7a2c620")
        );
    }

    #[test]
    fn final_wipes_the_state() {
        let mut st = Blake2sState::<32>::new_keyed(&key());
        st.update(b"secret");
        let _ = st.finalize();
        assert_eq!(st, Blake2sState::default());
    }

    #[test]
    fn hmac_blake2s() {
        let d = data();
        let cases: &[(usize, &str)] = &[
            (
                0,
                "2fda1d93aa5545d8fc2fff27f5d9685d838540afc60d81226b0ddb7d7200433b",
            ),
            (
                1,
                "5473d20550a8622769f364621e88a7c734efc383c91fe7c66caa75d9c3816a95",
            ),
            (
                32,
                "5dc616eb1304306337c30760c617b5f46079606bfb1a67c044466f6e44aec6df",
            ),
            (
                64,
                "9878e5c3b65df5de866861ccf4dfe23ce4720eb7dea83031d707a0465681e77d",
            ),
            // longer than a block: the key is hashed first
            (
                65,
                "a91ced146548f3fa32eaf3f2d44511b1e9a0491f3754b03f326c05144f583c04",
            ),
            (
                100,
                "0c155b58fcd935d18d7f706e9930fbbf823c09fc3c9149ee3140f050994e90e6",
            ),
        ];
        for (klen, want) in cases {
            let k: Vec<u8> = (0..*klen).map(|i| ((7 * i + 3) % 256) as u8).collect();
            let out: [u8; 32] = blake2s_hmac(&d[..77], &k);
            assert_eq!(out.to_vec(), hex(want), "key length {klen}");

            // The pattern of WireGuard's KDF (the C's `blake2s_hmac(out, out, ..)`): the MAC of
            // the message at the front of a buffer, written over that front.
            let mut big = [0u8; 77];
            big.copy_from_slice(&d[..77]);
            let mac: [u8; 32] = blake2s_hmac(&big, &k);
            big[..32].copy_from_slice(&mac);
            assert_eq!(
                big[..32].to_vec(),
                hex(want),
                "over the input, key length {klen}"
            );
            assert_eq!(big[32..], d[32..77], "bytes past the output are left alone");

            // A truncated output is the front of the full one.
            let short: [u8; 20] = blake2s_hmac(&d[..77], &k);
            assert_eq!(short.to_vec(), hex(want)[..20].to_vec());
        }
    }

    #[test]
    fn random_splits_give_the_one_call_hash() {
        let mut rng = XorShift::new(0x626c_616b_6532_7370);
        for _ in 0..200 {
            let key_len = rng.below(BLAKE2S_KEY_SIZE + 1);
            let k = rng.bytes(key_len);
            let len = rng.below(400);
            let msg = rng.bytes(len);

            let mut long = if k.is_empty() {
                Blake2sState::<32>::new()
            } else {
                Blake2sState::<32>::new_keyed(&k)
            };
            for piece in rng.split(&msg) {
                long.update(piece);
            }
            assert_eq!(
                long.finalize(),
                blake2s::<32>(&msg, &k),
                "32, {key_len}, {len}"
            );

            let mut short = if k.is_empty() {
                Blake2sState::<16>::new()
            } else {
                Blake2sState::<16>::new_keyed(&k)
            };
            for piece in rng.split(&msg) {
                short.update(piece);
            }
            assert_eq!(
                short.finalize(),
                blake2s::<16>(&msg, &k),
                "16, {key_len}, {len}"
            );
        }
    }

    #[test]
    #[ignore = "reads the C tables from $OPENBSD_SRC (just test-ref)"]
    fn constants_match_the_c_file() {
        let f = "sys/crypto/blake2s.c";
        let iv: Vec<u64> = BLAKE2S_IV.iter().map(|w| u64::from(*w)).collect();
        assert_eq!(c_table(f, "blake2s_iv"), iv);
        let sigma: Vec<u64> = BLAKE2S_SIGMA
            .iter()
            .flatten()
            .map(|b| u64::from(*b))
            .collect();
        assert_eq!(c_table(f, "blake2s_sigma"), sigma);
    }

    #[test]
    fn the_state_is_not_copy() {
        crate::crypto::testutil::assert_not_copy!(Blake2sState<32>, Blake2sState<16>);
    }

    #[test]
    fn zeroize_leaves_the_wiped_state() {
        // What `Drop` runs on a state freed before `finalize`. A keyed state holds the key
        // block itself until the next block arrives, so this is key material.
        let mut st = Blake2sState::<32>::new_keyed(&key());
        assert_eq!(st.buf[..32], key());
        st.zeroize();
        assert_eq!(st, Blake2sState::default());

        let mut st = Blake2sState::<32>::new();
        st.update(&data());
        assert_ne!(st, Blake2sState::default());
        st.zeroize();
        assert_eq!(st, Blake2sState::default());
    }

    #[test]
    fn a_clone_forks_the_hash() {
        let mut st = Blake2sState::<32>::new_keyed(&key());
        st.update(&data()[..100]);
        let mut fork = st.clone();
        fork.update(&data()[100..]);
        assert_eq!(fork.finalize().to_vec(), hash::<32>(&data(), &key()));
        assert_eq!(fork, Blake2sState::default());
        assert_ne!(st, Blake2sState::default());
        assert_eq!(st.finalize().to_vec(), hash::<32>(&data()[..100], &key()));
    }
}
/* </TESTS> */
