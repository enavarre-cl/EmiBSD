/*	$OpenBSD: blake2s.h,v 1.3 2023/02/03 18:31:16 miod Exp $	*/
/*	$OpenBSD: blake2s.c,v 1.3 2023/02/03 18:31:16 miod Exp $	*/
/* <LICENSES> */
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
//!
//! ## Deviations
//! - The header and the file share this module.
//! - Inputs and keys are slices; `inlen` and `keylen` are their lengths. `outlen` stays a
//!   parameter (the digest is truncated to it). `blake2s_final` and `blake2s_hmac` write
//!   `outlen` bytes to the front of a slice at least that long.
//! - `blake2s_hmac(out, out, ...)` (the C reads its input before it writes the output, so the
//!   two may be one buffer, as `wg_noise.c`'s `KDF` does) is [`blake2s_hmac_inplace`]: the
//!   buffer's first `inlen` bytes are the message and its first `outlen` bytes the result.
//! - `enum blake2s_lengths` is three constants.
//! - The `KASSERT`s are `kassert!`; the stack buffers holding key material are wiped with
//!   `explicit_bzero`, the state by assignment through `crate::crypto::wipe`.

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

/// `struct blake2s_state`: a hash in progress.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Blake2sState {
    /// `h`: the chained value.
    pub h: [u32; 8],
    /// `t`: the 64-bit byte counter.
    pub t: [u32; 2],
    /// `f`: the finalisation flags.
    pub f: [u32; 2],
    /// `buf`: the block not yet compressed.
    pub buf: [u8; BLAKE2S_BLOCK_SIZE],
    /// `buflen`: bytes in `buf`.
    pub buflen: u32,
    /// `outlen`: the digest length.
    pub outlen: u32,
}

impl Default for Blake2sState {
    fn default() -> Self {
        Self {
            h: [0; 8],
            t: [0; 2],
            f: [0; 2],
            buf: [0; BLAKE2S_BLOCK_SIZE],
            buflen: 0,
            outlen: 0,
        }
    }
}

/// `blake2s_set_lastblock`.
fn blake2s_set_lastblock(state: &mut Blake2sState) {
    state.f[0] = u32::MAX;
}

/// `blake2s_increment_counter`.
fn blake2s_increment_counter(state: &mut Blake2sState, inc: u32) {
    state.t[0] = state.t[0].wrapping_add(inc);
    state.t[1] = state.t[1].wrapping_add(u32::from(state.t[0] < inc));
}

/// `blake2s_init_param`: the zeroed state with the IV xor the parameter block's first word.
fn blake2s_init_param(state: &mut Blake2sState, param: u32) {
    *state = Blake2sState::default();
    state.h = BLAKE2S_IV;
    state.h[0] ^= param;
}

/// `blake2s_init`: an unkeyed hash with a digest of `outlen` bytes.
pub fn blake2s_init(state: &mut Blake2sState, outlen: usize) {
    kassert!(!(outlen == 0 || outlen > BLAKE2S_HASH_SIZE));
    blake2s_init_param(state, 0x01010000 | outlen as u32);
    state.outlen = outlen as u32;
}

/// `blake2s_init_key`: a keyed hash (the PRF); the key is padded with zeros to a block that is
/// hashed first.
pub fn blake2s_init_key(state: &mut Blake2sState, outlen: usize, key: &[u8]) {
    let keylen = key.len();
    let mut block = [0u8; BLAKE2S_BLOCK_SIZE];

    kassert!(
        !(outlen == 0 || outlen > BLAKE2S_HASH_SIZE || keylen == 0 || keylen > BLAKE2S_KEY_SIZE)
    );

    blake2s_init_param(state, 0x01010000 | (keylen as u32) << 8 | outlen as u32);
    state.outlen = outlen as u32;
    block[..keylen].copy_from_slice(key);
    blake2s_update(state, &block);
    explicit_bzero(&mut block);
}

/// `blake2s_compress`: absorbs `nblocks` whole blocks of `block`, counting `inc` bytes each.
fn blake2s_compress(state: &mut Blake2sState, block: &[u8], nblocks: usize, inc: u32) {
    let mut m = [0u32; 16];
    let mut v = [0u32; 16];

    kassert!(!(nblocks > 1 && inc != BLAKE2S_BLOCK_SIZE as u32));

    for blk in block
        .as_chunks::<BLAKE2S_BLOCK_SIZE>()
        .0
        .iter()
        .take(nblocks)
    {
        blake2s_increment_counter(state, inc);
        for i in 0..16 {
            m[i] = u32::from_le_bytes([blk[4 * i], blk[4 * i + 1], blk[4 * i + 2], blk[4 * i + 3]]);
        }
        v[..8].copy_from_slice(&state.h);
        v[8] = BLAKE2S_IV[0];
        v[9] = BLAKE2S_IV[1];
        v[10] = BLAKE2S_IV[2];
        v[11] = BLAKE2S_IV[3];
        v[12] = BLAKE2S_IV[4] ^ state.t[0];
        v[13] = BLAKE2S_IV[5] ^ state.t[1];
        v[14] = BLAKE2S_IV[6] ^ state.f[0];
        v[15] = BLAKE2S_IV[7] ^ state.f[1];

        // G(r, i, a, b, c, d)
        let g = |v: &mut [u32; 16], r: usize, i: usize, a: usize, b: usize, c: usize, d: usize| {
            let s = &BLAKE2S_SIGMA[r];
            v[a] = v[a].wrapping_add(v[b]).wrapping_add(m[s[2 * i] as usize]);
            v[d] = (v[d] ^ v[a]).rotate_right(16);
            v[c] = v[c].wrapping_add(v[d]);
            v[b] = (v[b] ^ v[c]).rotate_right(12);
            v[a] = v[a]
                .wrapping_add(v[b])
                .wrapping_add(m[s[2 * i + 1] as usize]);
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

        for i in 0..8 {
            state.h[i] ^= v[i] ^ v[i + 8];
        }
    }
}

/// `blake2s_update`: adds `input` to the message.
pub fn blake2s_update(state: &mut Blake2sState, input: &[u8]) {
    let mut input = input;
    let fill = BLAKE2S_BLOCK_SIZE - state.buflen as usize;

    if input.is_empty() {
        return;
    }
    if input.len() > fill {
        let buflen = state.buflen as usize;
        state.buf[buflen..].copy_from_slice(&input[..fill]);
        let buf = state.buf;
        blake2s_compress(state, &buf, 1, BLAKE2S_BLOCK_SIZE as u32);
        state.buflen = 0;
        input = &input[fill..];
    }
    if input.len() > BLAKE2S_BLOCK_SIZE {
        let nblocks = input.len().div_ceil(BLAKE2S_BLOCK_SIZE);
        // Hash one less (full) block than strictly possible
        blake2s_compress(state, input, nblocks - 1, BLAKE2S_BLOCK_SIZE as u32);
        input = &input[BLAKE2S_BLOCK_SIZE * (nblocks - 1)..];
    }
    let buflen = state.buflen as usize;
    state.buf[buflen..buflen + input.len()].copy_from_slice(input);
    state.buflen += input.len() as u32;
}

/// `blake2s_final`: pads and compresses the last block, writes the `outlen`-byte digest to
/// the front of `out` and wipes the state.
pub fn blake2s_final(state: &mut Blake2sState, out: &mut [u8]) {
    blake2s_set_lastblock(state);
    let buflen = state.buflen as usize;
    state.buf[buflen..].fill(0); // Padding
    let buf = state.buf;
    blake2s_compress(state, &buf, 1, state.buflen);
    let outlen = state.outlen as usize;
    for (i, w) in state.h.iter().enumerate() {
        let b = w.to_le_bytes();
        for (j, byte) in b.iter().enumerate() {
            if 4 * i + j < outlen {
                out[4 * i + j] = *byte;
            }
        }
    }
    wipe(state);
}

/// `blake2s`: the hash of `input` in one call, keyed when `key` is not empty.
pub fn blake2s(out: &mut [u8], input: &[u8], key: &[u8], outlen: usize) {
    let mut state = Blake2sState::default();

    kassert!(outlen <= BLAKE2S_HASH_SIZE && key.len() <= BLAKE2S_KEY_SIZE);

    if !key.is_empty() {
        blake2s_init_key(&mut state, outlen, key);
    } else {
        blake2s_init(&mut state, outlen);
    }

    blake2s_update(&mut state, input);
    blake2s_final(&mut state, out);
}

/// The two passes of HMAC-BLAKE2s over `input`; the digest is `outlen` bytes of `i_hash`.
fn blake2s_hmac_digest(input: &[u8], key: &[u8]) -> [u8; BLAKE2S_HASH_SIZE] {
    let mut state = Blake2sState::default();
    let mut x_key = [0u8; BLAKE2S_BLOCK_SIZE];
    let mut i_hash = [0u8; BLAKE2S_HASH_SIZE];

    if key.len() > BLAKE2S_BLOCK_SIZE {
        blake2s_init(&mut state, BLAKE2S_HASH_SIZE);
        blake2s_update(&mut state, key);
        blake2s_final(&mut state, &mut x_key);
    } else {
        x_key[..key.len()].copy_from_slice(key);
    }

    for b in x_key.iter_mut() {
        *b ^= 0x36;
    }

    blake2s_init(&mut state, BLAKE2S_HASH_SIZE);
    blake2s_update(&mut state, &x_key);
    blake2s_update(&mut state, input);
    blake2s_final(&mut state, &mut i_hash);

    for b in x_key.iter_mut() {
        *b ^= 0x5c ^ 0x36;
    }

    let i_hash_in = i_hash;
    blake2s_init(&mut state, BLAKE2S_HASH_SIZE);
    blake2s_update(&mut state, &x_key);
    blake2s_update(&mut state, &i_hash_in);
    blake2s_final(&mut state, &mut i_hash);

    explicit_bzero(&mut x_key);
    i_hash
}

/// `blake2s_hmac`: HMAC (RFC 2104) with BLAKE2s-256 as the hash; the first `outlen` bytes of
/// the MAC go to the front of `out`.
pub fn blake2s_hmac(out: &mut [u8], input: &[u8], key: &[u8], outlen: usize) {
    let mut i_hash = blake2s_hmac_digest(input, key);

    out[..outlen].copy_from_slice(&i_hash[..outlen]);
    explicit_bzero(&mut i_hash);
}

/// `blake2s_hmac(buf, buf, ...)`: the message is `buf[..inlen]`, replaced by the first
/// `outlen` bytes of its MAC.
pub fn blake2s_hmac_inplace(buf: &mut [u8], inlen: usize, key: &[u8], outlen: usize) {
    let mut i_hash = blake2s_hmac_digest(&buf[..inlen], key);

    buf[..outlen].copy_from_slice(&i_hash[..outlen]);
    explicit_bzero(&mut i_hash);
}
/* </CODE> */

/* <TESTS> */
#[cfg(test)]
mod tests {
    // Known-answer tests for BLAKE2s: RFC 7693 appendix B ("abc") and digests of messages around
    // the block size, plain and keyed, with the HMAC built on it; the expected values come from
    // Python's `hashlib.blake2s` and `hmac`.

    use super::*;
    use crate::crypto::testutil::{c_table, hex};

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

    fn hash(input: &[u8], key: &[u8], outlen: usize) -> Vec<u8> {
        let mut out = std::vec![0u8; outlen];
        blake2s(&mut out, input, key, outlen);
        out
    }

    #[test]
    fn rfc7693_appendix_b() {
        assert_eq!(
            hash(b"abc", &[], 32),
            hex("508c5e8c327c14e2e1a72ba34eeb452f37458b209ed63a294d999b4c86675982")
        );
        assert_eq!(
            hash(b"", &[], 32),
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
            assert_eq!(hash(&d[..*n], &[], 32), hex(plain), "plain {n}");
            assert_eq!(hash(&d[..*n], &k, 32), hex(keyed), "keyed {n}");
        }
        assert_eq!(
            hash(b"", &k, 32),
            hex("48a8997da407876b3d79c0d92325ad3b89cbb754d86ab71aee047ad345fd2c49")
        );
    }

    #[test]
    fn any_split_of_the_input_gives_the_same_digest() {
        let d = data();
        let k = key();
        for (n, plain, keyed) in PLAIN_AND_KEYED {
            for chunk in [1usize, 5, 63, 64, 65, 100] {
                let mut st = Blake2sState::default();
                let mut out = [0u8; 32];
                blake2s_init(&mut st, 32);
                for piece in d[..*n].chunks(chunk) {
                    blake2s_update(&mut st, piece);
                }
                blake2s_final(&mut st, &mut out);
                assert_eq!(out.to_vec(), hex(plain), "plain {n} by {chunk}");

                blake2s_init_key(&mut st, 32, &k);
                for piece in d[..*n].chunks(chunk) {
                    blake2s_update(&mut st, piece);
                }
                blake2s_final(&mut st, &mut out);
                assert_eq!(out.to_vec(), hex(keyed), "keyed {n} by {chunk}");
            }
        }
    }

    #[test]
    fn a_shorter_digest_is_a_different_hash_not_a_truncation() {
        // WireGuard's cookies use 16-byte keyed outputs: the digest length is in the parameter block.
        let d = data();
        assert_eq!(
            hash(&d[..100], &key()[..16], 16),
            hex("56301549e674c3b72a0e1dafb7a2c620")
        );
    }

    #[test]
    fn final_wipes_the_state() {
        let mut st = Blake2sState::default();
        let mut out = [0u8; 32];
        blake2s_init_key(&mut st, 32, &key());
        blake2s_update(&mut st, b"secret");
        blake2s_final(&mut st, &mut out);
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
            let mut out = [0u8; 32];
            blake2s_hmac(&mut out, &d[..77], &k, 32);
            assert_eq!(out.to_vec(), hex(want), "key length {klen}");

            // The in-place form WireGuard's KDF uses: the message at the front of the buffer.
            let mut buf = [0u8; 77];
            buf.copy_from_slice(&d[..77]);
            let mut big = [0u8; 77];
            big.copy_from_slice(&buf);
            blake2s_hmac_inplace(&mut big, 77, &k, 32);
            assert_eq!(big[..32].to_vec(), hex(want), "in place, key length {klen}");
            assert_eq!(big[32..], buf[32..], "bytes past the output are left alone");

            // A truncated output is the front of the full one.
            let mut short = [0u8; 20];
            blake2s_hmac(&mut short, &d[..77], &k, 20);
            assert_eq!(short.to_vec(), hex(want)[..20].to_vec());
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
}
/* </TESTS> */
