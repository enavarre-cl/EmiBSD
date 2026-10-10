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

/*-
 * Copyright (c) 2013 Andre Oppermann <andre@FreeBSD.org>
 * All rights reserved.
 *
 * Redistribution and use in source and binary forms, with or without
 * modification, are permitted provided that the following conditions
 * are met:
 * 1. Redistributions of source code must retain the above copyright
 *    notice, this list of conditions and the following disclaimer.
 * 2. Redistributions in binary form must reproduce the above copyright
 *    notice, this list of conditions and the following disclaimer in the
 *    documentation and/or other materials provided with the distribution.
 * 3. The name of the author may not be used to endorse or promote
 *    products derived from this software without specific prior written
 *    permission.
 *
 * THIS SOFTWARE IS PROVIDED BY THE AUTHOR AND CONTRIBUTORS ``AS IS'' AND
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
 * $FreeBSD$
 */

/*-
 * Copyright (c) 2013 Andre Oppermann <andre@FreeBSD.org>
 * All rights reserved.
 *
 * Redistribution and use in source and binary forms, with or without
 * modification, are permitted provided that the following conditions
 * are met:
 * 1. Redistributions of source code must retain the above copyright
 *    notice, this list of conditions and the following disclaimer.
 * 2. Redistributions in binary form must reproduce the above copyright
 *    notice, this list of conditions and the following disclaimer in the
 *    documentation and/or other materials provided with the distribution.
 * 3. The name of the author may not be used to endorse or promote
 *    products derived from this software without specific prior written
 *    permission.
 *
 * THIS SOFTWARE IS PROVIDED BY THE AUTHOR AND CONTRIBUTORS ``AS IS'' AND
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
 */
/* </LICENSES> */

/* <CODE> */
//! SipHash, a family of pseudorandom functions (keyed hashes) optimised for speed on short
//! messages, returning a 64-bit value: SipHash-2-4 ([`SipHash24Ctx`], [`SipHash24`]) for the
//! fast and reasonably strong version, SipHash-4-8 ([`SipHash48Ctx`], [`SipHash48`]) for the
//! strong one. The kernel uses it to spread keys over hash tables an attacker cannot predict
//! (`ufs_ihash`).
//!
//! Upstream: sys/crypto/siphash.h @ 3ce1f3f79392, sys/crypto/siphash.c @ 3ce1f3f79392
//! LZ: sys/crypto/siphash.rs@f5985f1d055a
//!
//! Implemented, as the C is, from the paper "SipHash: a fast short-input PRF" by Jean-Philippe
//! Aumasson and Daniel J. Bernstein.
//!
//! ## Deviations
//! - The header and the file share this module (one name, as `docs/C_TO_RUST.md` does for
//!   `.h`/`.c` pairs).
//! - `SIPHASH_CTX`/`SIPHASH_KEY` are [`SiphashCtx`]/[`SiphashKey`].
//!
//! ## Redesign
//! - The context and its functions are a type with methods (`docs/IDIOMS.md`, "a hash
//!   context"): `SipHash_Init(ctx, key)` is [`SiphashCtx::new`], `SipHash_Update` is
//!   [`SiphashCtx::update`] over a slice, `SipHash_End` is [`SiphashCtx::end`] and
//!   `SipHash_Final(dst, ctx)` is [`SiphashCtx::finalize`], which returns the bytes; both
//!   wipe the context in place (the C's `explicit_bzero(ctx, ..)`). The one-call
//!   `SipHash(key, rc, rf, src)` is [`SiphashCtx::hash`].
//! - The C passes the compression and finalisation round counts (`rc`, `rf`) to every call,
//!   and the `SipHash24_*`/`SipHash48_*` macros fix them; nothing stops an `Update` with one
//!   count and an `End` with another. Here they are the context's const parameters,
//!   `SiphashCtx<C, D>`, and [`SipHash24Ctx`] and [`SipHash48Ctx`] are the two the kernel
//!   uses: the macros' `Init`/`Update`/`End`/`Final` merge into the generic methods, and a
//!   context cannot change its rounds midway.
//! - [`SipHash24`] and [`SipHash48`] keep their names and signatures (six callers across
//!   fs and net use the one-call form).
//! - The fields are private; the block loop of `update` walks `as_chunks`.
//! - The context holds key-derived state (`v` is the key xor the constants until the end),
//!   so it is not `Copy` and zeroes itself when dropped (`docs/IDIOMS.md`, "a hash
//!   context"): a by-value pass or a context dropped before `end` leaves no copy of it.

use super::wipe;

/// `SIPHASH_BLOCK_LENGTH`.
pub const SIPHASH_BLOCK_LENGTH: usize = 8;
/// `SIPHASH_KEY_LENGTH`.
pub const SIPHASH_KEY_LENGTH: usize = 16;
/// `SIPHASH_DIGEST_LENGTH`.
pub const SIPHASH_DIGEST_LENGTH: usize = 8;

/// `SIPHASH_CTX`: the state of a SipHash-`C`-`D` hash in progress (`C` compression rounds per
/// block, `D` finalisation rounds), from [`SiphashCtx::new`] to [`SiphashCtx::end`] or
/// [`SiphashCtx::finalize`].
///
/// `Default` is the wiped, all-zero context the end leaves behind, not a hash under any key.
/// Not `Copy`: dropping a context wipes it.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct SiphashCtx<const C: usize, const D: usize> {
    /// `v`: the four state words.
    v: [u64; 4],
    /// `buf`: the bytes of the block not yet compressed.
    buf: [u8; SIPHASH_BLOCK_LENGTH],
    /// `bytes`: the message length so far, mod 2^32 (the C's `uint32_t`).
    bytes: u32,
}

impl<const C: usize, const D: usize> Drop for SiphashCtx<C, D> {
    /// Wipes the key-derived state, as `end` does, on every path that frees it.
    fn drop(&mut self) {
        self.zeroize();
    }
}

impl<const C: usize, const D: usize> SiphashCtx<C, D> {
    /// Zeroes every field in place: the context becomes the `Default` (wiped) value.
    pub(crate) fn zeroize(&mut self) {
        wipe(&mut self.v);
        wipe(&mut self.buf);
        wipe(&mut self.bytes);
    }

    /// `SipHash_Init`: starts a hash under `key`.
    pub fn new(key: &SiphashKey) -> Self {
        // lemtoh64: the key words are stored little-endian.
        let k0 = u64::from_le(key.k0);
        let k1 = u64::from_le(key.k1);

        Self {
            v: [
                0x736f_6d65_7073_6575 ^ k0,
                0x646f_7261_6e64_6f6d ^ k1,
                0x6c79_6765_6e65_7261 ^ k0,
                0x7465_6462_7974_6573 ^ k1,
            ],
            buf: [0; SIPHASH_BLOCK_LENGTH],
            bytes: 0,
        }
    }

    /// The bytes waiting in `buf`: the message length mod 8.
    fn used(&self) -> usize {
        usize::from(self.bytes.to_le_bytes()[0]) % SIPHASH_BLOCK_LENGTH
    }

    /// `SipHash_Update`: adds `src` to the message, compressing each full block with `C`
    /// rounds.
    pub fn update(&mut self, src: &[u8]) {
        let mut ptr = src;
        if ptr.is_empty() {
            return;
        }

        let used = self.used();
        // The C adds the `size_t` length to a `uint32_t`: the count is mod 2^32, so the
        // truncation of the length is the arithmetic the C does.
        self.bytes = self.bytes.wrapping_add(ptr.len() as u32);

        if used > 0 {
            let left = SIPHASH_BLOCK_LENGTH - used;

            if ptr.len() >= left {
                let (head, rest) = ptr.split_at(left);
                self.buf[used..].copy_from_slice(head);
                self.crounds();
                ptr = rest;
            } else {
                self.buf[used..used + ptr.len()].copy_from_slice(ptr);
                return;
            }
        }

        let (blocks, rest) = ptr.as_chunks::<SIPHASH_BLOCK_LENGTH>();
        for block in blocks {
            self.buf = *block;
            self.crounds();
        }

        self.buf[..rest.len()].copy_from_slice(rest);
    }

    /// `SipHash_Final`: the hash as little-endian bytes; the state is wiped.
    pub fn finalize(&mut self) -> [u8; SIPHASH_DIGEST_LENGTH] {
        self.end().to_le_bytes()
    }

    /// `SipHash_End`: pads the last block with the length, finalises with `D` rounds and
    /// returns the hash; the state is wiped.
    pub fn end(&mut self) -> u64 {
        let used = self.used();
        self.buf[used..SIPHASH_BLOCK_LENGTH - 1].fill(0);
        // The low byte of the length, as the C's `ctx->buf[7] = ctx->bytes`.
        self.buf[SIPHASH_BLOCK_LENGTH - 1] = self.bytes.to_le_bytes()[0];

        self.crounds();
        self.v[2] ^= 0xff;
        self.rounds(D);

        let r = (self.v[0] ^ self.v[1]) ^ (self.v[2] ^ self.v[3]);
        self.zeroize();
        r
    }

    /// `SipHash`: the hash of `src` under `key`, in one call.
    pub fn hash(key: &SiphashKey, src: &[u8]) -> u64 {
        let mut ctx = Self::new(key);
        ctx.update(src);
        ctx.end()
    }

    /// `SipHash_Rounds`: `rounds` SipRounds over the state.
    fn rounds(&mut self, rounds: usize) {
        let v = &mut self.v;
        for _ in 0..rounds {
            v[0] = v[0].wrapping_add(v[1]);
            v[2] = v[2].wrapping_add(v[3]);
            v[1] = v[1].rotate_left(13);
            v[3] = v[3].rotate_left(16);

            v[1] ^= v[0];
            v[3] ^= v[2];
            v[0] = v[0].rotate_left(32);

            v[2] = v[2].wrapping_add(v[1]);
            v[0] = v[0].wrapping_add(v[3]);
            v[1] = v[1].rotate_left(17);
            v[3] = v[3].rotate_left(21);

            v[1] ^= v[2];
            v[3] ^= v[0];
            v[2] = v[2].rotate_left(32);
        }
    }

    /// `SipHash_CRounds`: compresses the buffered block with `C` rounds.
    fn crounds(&mut self) {
        let m = u64::from_le_bytes(self.buf);

        self.v[3] ^= m;
        self.rounds(C);
        self.v[0] ^= m;
    }
}

/// SipHash-2-4, the `SipHash24_*` macros' context.
pub type SipHash24Ctx = SiphashCtx<2, 4>;

/// SipHash-4-8, the `SipHash48_*` macros' context.
pub type SipHash48Ctx = SiphashCtx<4, 8>;

/// `SIPHASH_KEY`: the 128-bit key, two words stored little-endian.
#[repr(C)]
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct SiphashKey {
    /// `k0`.
    pub k0: u64,
    /// `k1`.
    pub k1: u64,
}

/// `SipHash24`: SipHash-2-4 of `src` under `key`, in one call.
#[allow(non_snake_case)] // the C macro's name
pub fn SipHash24(key: &SiphashKey, src: &[u8]) -> u64 {
    SipHash24Ctx::hash(key, src)
}

/// `SipHash48`: SipHash-4-8 of `src` under `key`, in one call.
#[allow(non_snake_case)] // the C macro's name
pub fn SipHash48(key: &SiphashKey, src: &[u8]) -> u64 {
    SipHash48Ctx::hash(key, src)
}
/* </CODE> */

/* <TESTS> */
#[cfg(test)]
mod tests {
    // Known-answer tests: the paper's appendix A and the reference implementation's
    // SipHash-2-4 vectors (key 00..0f, messages 00, 00 01, ... of 0 to 15 bytes); SipHash-4-8
    // has no published vectors, so its values come from an independent implementation of the
    // paper's algorithm (Python) that reproduces the 2-4 vectors. Property tests: random
    // splits give the one-call hash; the end wipes the context.

    use super::*;
    use crate::crypto::testutil::XorShift;

    /// The key of the paper's test vectors: the bytes 0, 1, ..., 15 in memory.
    fn paper_key() -> SiphashKey {
        let mut k0 = [0u8; 8];
        let mut k1 = [0u8; 8];
        for i in 0..8 {
            k0[i] = i as u8;
            k1[i] = (i + 8) as u8;
        }
        SiphashKey {
            k0: u64::from_ne_bytes(k0),
            k1: u64::from_ne_bytes(k1),
        }
    }

    /// SipHash-2-4 and SipHash-4-8 of the messages 00..(n-1) under [`paper_key`], n = 0..15.
    const VECTORS: [(u64, u64); 16] = [
        (0x726fdb47dd0e0e31, 0xc879052b9938da41),
        (0x74f839c593dc67fd, 0xc85914f95295b851),
        (0x0d6c8009d9a94f5a, 0x33c3ddbef0163792),
        (0x85676696d7fb7e2d, 0x05c147657dd4466a),
        (0xcf2794e0277187b7, 0x48fac14a2b5938c2),
        (0x18765564cd99a68d, 0xe14752cfd9d7c2f6),
        (0xcbc9466e58fee3ce, 0x8e5535c834bcb66b),
        (0xab0200f58b01d137, 0x4efdbe5a713fd747),
        (0x93f5f5799a932462, 0x50db2f079c8bb520),
        (0x9e0082df0ba9e4b0, 0x5312e15ef39a3136),
        (0x7a5dbbc594ddb9f3, 0x8f848d0adbd0a948),
        (0xf4b32f46226bada7, 0x810a0436603969cc),
        (0x751e8fbc860ee5fb, 0x6197a77a53686d4b),
        (0x14ea5627c0843d90, 0x6950c9f2e9963729),
        (0xf723ca908e7af2ee, 0x689a62a7ea1b4388),
        (0xa129ca6149be45e5, 0x83d389d57da9a6e0),
    ];

    #[test]
    fn matches_the_reference_vectors() {
        let key = paper_key();
        let msg: [u8; 15] = core::array::from_fn(|i| i as u8);
        // Appendix A of the paper: SipHash-2-4 of the 15-byte message 00..0e.
        assert_eq!(SipHash24(&key, &msg), 0xa129_ca61_49be_45e5);
        // The reference implementation's vectors for the empty and the 8-byte message.
        assert_eq!(SipHash24(&key, &[]), 0x726f_db47_dd0e_0e31);
        assert_eq!(SipHash24(&key, &msg[..8]), 0x93f5_f579_9a93_2462);
    }

    #[test]
    fn every_length_up_to_two_blocks() {
        let key = paper_key();
        let msg: [u8; 16] = core::array::from_fn(|i| i as u8);
        for (n, (want24, want48)) in VECTORS.iter().enumerate() {
            assert_eq!(SipHash24(&key, &msg[..n]), *want24, "2-4, {n} bytes");
            assert_eq!(SipHash48(&key, &msg[..n]), *want48, "4-8, {n} bytes");
            let mut ctx = SipHash24Ctx::new(&key);
            ctx.update(&msg[..n]);
            assert_eq!(ctx.finalize(), want24.to_le_bytes(), "2-4 bytes, {n}");
        }
    }

    #[test]
    fn incremental_updates_agree_with_one_call() {
        let key = paper_key();
        let msg: [u8; 37] = core::array::from_fn(|i| (i * 7) as u8);
        let whole = SipHash24(&key, &msg);
        for split in [0, 1, 3, 8, 9, 20, 37] {
            let mut ctx = SipHash24Ctx::new(&key);
            ctx.update(&msg[..split]);
            ctx.update(&msg[split..]);
            assert_eq!(ctx.end(), whole, "split at {split}");
            // The end wipes the context.
            assert_eq!(ctx, SipHash24Ctx::default());
        }
        let mut ctx = SipHash48Ctx::new(&key);
        ctx.update(&msg);
        let d = ctx.finalize();
        assert_eq!(u64::from_le_bytes(d), SipHash48(&key, &msg));
        assert_eq!(ctx, SipHash48Ctx::default());
    }

    #[test]
    fn random_keys_and_splits_give_the_one_call_hash() {
        let mut rng = XorShift::new(0x7369_7068_6173_6824);
        for _ in 0..300 {
            let key = SiphashKey {
                k0: rng.next_u64(),
                k1: rng.next_u64(),
            };
            let len = rng.below(100);
            let msg = rng.bytes(len);
            let mut c24 = SipHash24Ctx::new(&key);
            for piece in rng.split(&msg) {
                c24.update(piece);
            }
            let mut c48 = SipHash48Ctx::new(&key);
            for piece in rng.split(&msg) {
                c48.update(piece);
            }
            assert_eq!(c24.end(), SipHash24(&key, &msg), "2-4, {len} bytes");
            assert_eq!(c48.end(), SipHash48(&key, &msg), "4-8, {len} bytes");
        }
    }

    #[test]
    fn the_contexts_are_not_copy() {
        crate::crypto::testutil::assert_not_copy!(SipHash24Ctx, SipHash48Ctx);
    }

    #[test]
    fn zeroize_leaves_the_wiped_context() {
        // What `Drop` runs on a context freed before `end`: the key-derived words zeroed.
        let mut ctx = SipHash24Ctx::new(&paper_key());
        ctx.update(b"0123456789a");
        assert_ne!(ctx, SipHash24Ctx::default());
        ctx.zeroize();
        assert_eq!(ctx, SipHash24Ctx::default());

        // Even with nothing hashed, the state is the key xor the constants.
        let mut ctx = SipHash48Ctx::new(&paper_key());
        assert_ne!(ctx, SipHash48Ctx::default());
        ctx.zeroize();
        assert_eq!(ctx, SipHash48Ctx::default());
    }

    #[test]
    fn a_clone_forks_the_hash() {
        let key = paper_key();
        let mut ctx = SipHash24Ctx::new(&key);
        ctx.update(b"abcdefghij");
        let mut fork = ctx.clone();
        fork.update(b"klm");
        assert_eq!(fork.end(), SipHash24(&key, b"abcdefghijklm"));
        assert_eq!(fork, SipHash24Ctx::default());
        assert_ne!(ctx, SipHash24Ctx::default());
        assert_eq!(ctx.end(), SipHash24(&key, b"abcdefghij"));
    }
}
/* </TESTS> */
