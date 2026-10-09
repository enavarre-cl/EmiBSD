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
 * FILE:	sha2.h
 * AUTHOR:	Aaron D. Gifford <me@aarongifford.com>
 *
 * Copyright (c) 2000-2001, Aaron D. Gifford
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
 * 3. Neither the name of the copyright holder nor the names of contributors
 *    may be used to endorse or promote products derived from this software
 *    without specific prior written permission.
 *
 * THIS SOFTWARE IS PROVIDED BY THE AUTHOR AND CONTRIBUTOR(S) ``AS IS'' AND
 * ANY EXPRESS OR IMPLIED WARRANTIES, INCLUDING, BUT NOT LIMITED TO, THE
 * IMPLIED WARRANTIES OF MERCHANTABILITY AND FITNESS FOR A PARTICULAR PURPOSE
 * ARE DISCLAIMED.  IN NO EVENT SHALL THE AUTHOR OR CONTRIBUTOR(S) BE LIABLE
 * FOR ANY DIRECT, INDIRECT, INCIDENTAL, SPECIAL, EXEMPLARY, OR CONSEQUENTIAL
 * DAMAGES (INCLUDING, BUT NOT LIMITED TO, PROCUREMENT OF SUBSTITUTE GOODS
 * OR SERVICES; LOSS OF USE, DATA, OR PROFITS; OR BUSINESS INTERRUPTION)
 * HOWEVER CAUSED AND ON ANY THEORY OF LIABILITY, WHETHER IN CONTRACT, STRICT
 * LIABILITY, OR TORT (INCLUDING NEGLIGENCE OR OTHERWISE) ARISING IN ANY WAY
 * OUT OF THE USE OF THIS SOFTWARE, EVEN IF ADVISED OF THE POSSIBILITY OF
 * SUCH DAMAGE.
 *
 * $From: sha2.h,v 1.1 2001/11/08 00:02:01 adg Exp adg $
 */

/*
 * FILE:	sha2.c
 * AUTHOR:	Aaron D. Gifford <me@aarongifford.com>
 *
 * Copyright (c) 2000-2001, Aaron D. Gifford
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
 * 3. Neither the name of the copyright holder nor the names of contributors
 *    may be used to endorse or promote products derived from this software
 *    without specific prior written permission.
 *
 * THIS SOFTWARE IS PROVIDED BY THE AUTHOR AND CONTRIBUTOR(S) ``AS IS'' AND
 * ANY EXPRESS OR IMPLIED WARRANTIES, INCLUDING, BUT NOT LIMITED TO, THE
 * IMPLIED WARRANTIES OF MERCHANTABILITY AND FITNESS FOR A PARTICULAR PURPOSE
 * ARE DISCLAIMED.  IN NO EVENT SHALL THE AUTHOR OR CONTRIBUTOR(S) BE LIABLE
 * FOR ANY DIRECT, INDIRECT, INCIDENTAL, SPECIAL, EXEMPLARY, OR CONSEQUENTIAL
 * DAMAGES (INCLUDING, BUT NOT LIMITED TO, PROCUREMENT OF SUBSTITUTE GOODS
 * OR SERVICES; LOSS OF USE, DATA, OR PROFITS; OR BUSINESS INTERRUPTION)
 * HOWEVER CAUSED AND ON ANY THEORY OF LIABILITY, WHETHER IN CONTRACT, STRICT
 * LIABILITY, OR TORT (INCLUDING NEGLIGENCE OR OTHERWISE) ARISING IN ANY WAY
 * OUT OF THE USE OF THIS SOFTWARE, EVEN IF ADVISED OF THE POSSIBILITY OF
 * SUCH DAMAGE.
 *
 * $From: sha2.c,v 1.1 2001/11/08 00:01:51 adg Exp adg $
 */
/* </LICENSES> */

/* <CODE> */
//! SHA-256, SHA-384 and SHA-512 (FIPS 180-4), Aaron Gifford's implementation: the hashes of
//! IPsec's `HMAC-SHA2-*` and of `hmac.c`.
//!
//! Upstream: sys/crypto/sha2.h @ 3ce1f3f79392, sys/crypto/sha2.c @ 3ce1f3f79392
//! LZ: sys/crypto/sha2.rs@f5985f1d055a
//!
//! ## Deviations
//! - The header and the file share this module.
//! - The round constants and initial hash values are FIPS 180-4's (the fractional parts of
//!   the cube and square roots of the first primes), written as `const`s; they were
//!   regenerated from that definition with exact integer arithmetic and compared with the C
//!   file's tables, and the hash vectors in the tests exercise every one of them.
//! - Only the compact transform loop is ported. The C selects an unrolled, macro-generated
//!   transform (`SHA2_UNROLL_TRANSFORM`) on amd64 and i386 unless `SMALL_KERNEL`; both compute
//!   the same function.
//! - The `Final` functions write big-endian digests directly instead of swapping the state in
//!   place first.
//!
//! ## Redesign
//! - `SHA2_CTX` is one struct for three hashes, its state a union of eight 32-bit words
//!   (SHA-256) and eight 64-bit words (SHA-384, SHA-512), its `bitcount` two words of which
//!   SHA-256 uses one. Here each hash has its own context type: [`Sha256Ctx`] (32-bit state,
//!   one count word, a 64-byte buffer), and [`Sha384Ctx`] and [`Sha512Ctx`] over the private
//!   `Sha512Core` they share (64-bit state, the 128-bit count, a 128-byte buffer). Updating a
//!   SHA-256 context with SHA-512's function, or finishing it with SHA-384's, is a type error
//!   instead of a silent wrong digest; the union (`Sha2State`) is gone.
//! - The contexts and their functions are types with methods (`docs/IDIOMS.md`, "a hash
//!   context"): `SHA256Init`/`SHA384Init`/`SHA512Init` are `new`, the `Update` functions
//!   `update` over a slice, and the `Final` functions `finalize`, which returns the digest and
//!   wipes the context in place (the C's `explicit_bzero(context, ..)`). `SHA512Last`, shared
//!   by SHA-384 and SHA-512, is `Sha512Core::last`, private.
//! - The fields are private. The whole-block loops walk `as_chunks` and transform the buffer
//!   in place instead of copying each block into a stack array first.
//! - `SHA256Transform` and `SHA512Transform` take the state words and one block, not the
//!   context, and keep their names.

use super::wipe;

/// `SHA256_BLOCK_LENGTH`.
pub const SHA256_BLOCK_LENGTH: usize = 64;
/// `SHA256_DIGEST_LENGTH`.
pub const SHA256_DIGEST_LENGTH: usize = 32;
/// `SHA256_DIGEST_STRING_LENGTH`.
pub const SHA256_DIGEST_STRING_LENGTH: usize = SHA256_DIGEST_LENGTH * 2 + 1;
/// `SHA384_BLOCK_LENGTH`.
pub const SHA384_BLOCK_LENGTH: usize = 128;
/// `SHA384_DIGEST_LENGTH`.
pub const SHA384_DIGEST_LENGTH: usize = 48;
/// `SHA384_DIGEST_STRING_LENGTH`.
pub const SHA384_DIGEST_STRING_LENGTH: usize = SHA384_DIGEST_LENGTH * 2 + 1;
/// `SHA512_BLOCK_LENGTH`.
pub const SHA512_BLOCK_LENGTH: usize = 128;
/// `SHA512_DIGEST_LENGTH`.
pub const SHA512_DIGEST_LENGTH: usize = 64;
/// `SHA512_DIGEST_STRING_LENGTH`.
pub const SHA512_DIGEST_STRING_LENGTH: usize = SHA512_DIGEST_LENGTH * 2 + 1;

/// `SHA256_SHORT_BLOCK_LENGTH`: where the length goes.
const SHA256_SHORT_BLOCK_LENGTH: usize = SHA256_BLOCK_LENGTH - 8;
/// `SHA512_SHORT_BLOCK_LENGTH`.
const SHA512_SHORT_BLOCK_LENGTH: usize = SHA512_BLOCK_LENGTH - 16;

/// Hash constant words K for SHA-256.
const K256: [u32; 64] = [
    0x428a2f98, 0x71374491, 0xb5c0fbcf, 0xe9b5dba5, 0x3956c25b, 0x59f111f1, 0x923f82a4, 0xab1c5ed5,
    0xd807aa98, 0x12835b01, 0x243185be, 0x550c7dc3, 0x72be5d74, 0x80deb1fe, 0x9bdc06a7, 0xc19bf174,
    0xe49b69c1, 0xefbe4786, 0x0fc19dc6, 0x240ca1cc, 0x2de92c6f, 0x4a7484aa, 0x5cb0a9dc, 0x76f988da,
    0x983e5152, 0xa831c66d, 0xb00327c8, 0xbf597fc7, 0xc6e00bf3, 0xd5a79147, 0x06ca6351, 0x14292967,
    0x27b70a85, 0x2e1b2138, 0x4d2c6dfc, 0x53380d13, 0x650a7354, 0x766a0abb, 0x81c2c92e, 0x92722c85,
    0xa2bfe8a1, 0xa81a664b, 0xc24b8b70, 0xc76c51a3, 0xd192e819, 0xd6990624, 0xf40e3585, 0x106aa070,
    0x19a4c116, 0x1e376c08, 0x2748774c, 0x34b0bcb5, 0x391c0cb3, 0x4ed8aa4a, 0x5b9cca4f, 0x682e6ff3,
    0x748f82ee, 0x78a5636f, 0x84c87814, 0x8cc70208, 0x90befffa, 0xa4506ceb, 0xbef9a3f7, 0xc67178f2,
];

/// Initial hash value H for SHA-256.
const SHA256_INITIAL_HASH_VALUE: [u32; 8] = [
    0x6a09e667, 0xbb67ae85, 0x3c6ef372, 0xa54ff53a, 0x510e527f, 0x9b05688c, 0x1f83d9ab, 0x5be0cd19,
];

/// Hash constant words K for SHA-384 and SHA-512.
const K512: [u64; 80] = [
    0x428a2f98d728ae22,
    0x7137449123ef65cd,
    0xb5c0fbcfec4d3b2f,
    0xe9b5dba58189dbbc,
    0x3956c25bf348b538,
    0x59f111f1b605d019,
    0x923f82a4af194f9b,
    0xab1c5ed5da6d8118,
    0xd807aa98a3030242,
    0x12835b0145706fbe,
    0x243185be4ee4b28c,
    0x550c7dc3d5ffb4e2,
    0x72be5d74f27b896f,
    0x80deb1fe3b1696b1,
    0x9bdc06a725c71235,
    0xc19bf174cf692694,
    0xe49b69c19ef14ad2,
    0xefbe4786384f25e3,
    0x0fc19dc68b8cd5b5,
    0x240ca1cc77ac9c65,
    0x2de92c6f592b0275,
    0x4a7484aa6ea6e483,
    0x5cb0a9dcbd41fbd4,
    0x76f988da831153b5,
    0x983e5152ee66dfab,
    0xa831c66d2db43210,
    0xb00327c898fb213f,
    0xbf597fc7beef0ee4,
    0xc6e00bf33da88fc2,
    0xd5a79147930aa725,
    0x06ca6351e003826f,
    0x142929670a0e6e70,
    0x27b70a8546d22ffc,
    0x2e1b21385c26c926,
    0x4d2c6dfc5ac42aed,
    0x53380d139d95b3df,
    0x650a73548baf63de,
    0x766a0abb3c77b2a8,
    0x81c2c92e47edaee6,
    0x92722c851482353b,
    0xa2bfe8a14cf10364,
    0xa81a664bbc423001,
    0xc24b8b70d0f89791,
    0xc76c51a30654be30,
    0xd192e819d6ef5218,
    0xd69906245565a910,
    0xf40e35855771202a,
    0x106aa07032bbd1b8,
    0x19a4c116b8d2d0c8,
    0x1e376c085141ab53,
    0x2748774cdf8eeb99,
    0x34b0bcb5e19b48a8,
    0x391c0cb3c5c95a63,
    0x4ed8aa4ae3418acb,
    0x5b9cca4f7763e373,
    0x682e6ff3d6b2b8a3,
    0x748f82ee5defb2fc,
    0x78a5636f43172f60,
    0x84c87814a1f0ab72,
    0x8cc702081a6439ec,
    0x90befffa23631e28,
    0xa4506cebde82bde9,
    0xbef9a3f7b2c67915,
    0xc67178f2e372532b,
    0xca273eceea26619c,
    0xd186b8c721c0c207,
    0xeada7dd6cde0eb1e,
    0xf57d4f7fee6ed178,
    0x06f067aa72176fba,
    0x0a637dc5a2c898a6,
    0x113f9804bef90dae,
    0x1b710b35131c471b,
    0x28db77f523047d84,
    0x32caab7b40c72493,
    0x3c9ebe0a15c9bebc,
    0x431d67c49c100d4c,
    0x4cc5d4becb3e42b6,
    0x597f299cfc657e2a,
    0x5fcb6fab3ad6faec,
    0x6c44198c4a475817,
];

/// Initial hash value H for SHA-384.
const SHA384_INITIAL_HASH_VALUE: [u64; 8] = [
    0xcbbb9d5dc1059ed8,
    0x629a292a367cd507,
    0x9159015a3070dd17,
    0x152fecd8f70e5939,
    0x67332667ffc00b31,
    0x8eb44a8768581511,
    0xdb0c2e0d64f98fa7,
    0x47b5481dbefa4fa4,
];

/// Initial hash value H for SHA-512.
const SHA512_INITIAL_HASH_VALUE: [u64; 8] = [
    0x6a09e667f3bcc908,
    0xbb67ae8584caa73b,
    0x3c6ef372fe94f82b,
    0xa54ff53a5f1d36f1,
    0x510e527fade682d1,
    0x9b05688c2b3e6c1f,
    0x1f83d9abfb41bd6b,
    0x5be0cd19137e2179,
];

/// `SHA2_CTX` as SHA-256 uses it: a SHA-256 hash in progress, from [`Sha256Ctx::new`] to
/// [`Sha256Ctx::finalize`].
///
/// `Default` is the wiped, all-zero context that `finalize` leaves behind, not the start of a
/// hash.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Sha256Ctx {
    /// `state.st32`: the eight chaining words.
    state: [u32; 8],
    /// `bitcount[0]`: the message length so far, in bits.
    bitcount: u64,
    /// `buffer`: the partial block.
    buffer: [u8; SHA256_BLOCK_LENGTH],
}

impl Default for Sha256Ctx {
    fn default() -> Self {
        Self {
            state: [0; 8],
            bitcount: 0,
            buffer: [0; SHA256_BLOCK_LENGTH],
        }
    }
}

impl Sha256Ctx {
    /// `SHA256Init`.
    pub fn new() -> Self {
        Self {
            state: SHA256_INITIAL_HASH_VALUE,
            bitcount: 0,
            buffer: [0; SHA256_BLOCK_LENGTH],
        }
    }

    /// The bytes waiting in `buffer`: the message length mod 64. The value is below 64, so
    /// the conversion to `usize` is exact.
    fn buffered(&self) -> usize {
        ((self.bitcount >> 3) % SHA256_BLOCK_LENGTH as u64) as usize
    }

    /// `SHA256Update`.
    pub fn update(&mut self, data: &[u8]) {
        let mut data = data;

        // Calling with no data is valid (we do nothing)
        if data.is_empty() {
            return;
        }

        let usedspace = self.buffered();
        if usedspace > 0 {
            // Calculate how much free space is available in the buffer
            let freespace = SHA256_BLOCK_LENGTH - usedspace;

            if data.len() >= freespace {
                // Fill the buffer completely and process it
                let (head, rest) = data.split_at(freespace);
                self.buffer[usedspace..].copy_from_slice(head);
                self.bitcount = self.bitcount.wrapping_add((freespace as u64) << 3);
                data = rest;
                SHA256Transform(&mut self.state, &self.buffer);
            } else {
                // The buffer is not yet full
                self.buffer[usedspace..usedspace + data.len()].copy_from_slice(data);
                self.bitcount = self.bitcount.wrapping_add((data.len() as u64) << 3);
                return;
            }
        }
        // Process as many complete blocks as we can
        let (blocks, rest) = data.as_chunks::<SHA256_BLOCK_LENGTH>();
        for block in blocks {
            SHA256Transform(&mut self.state, block);
            self.bitcount = self
                .bitcount
                .wrapping_add((SHA256_BLOCK_LENGTH as u64) << 3);
        }
        if !rest.is_empty() {
            // There's left-overs, so save 'em
            self.buffer[..rest.len()].copy_from_slice(rest);
            self.bitcount = self.bitcount.wrapping_add((rest.len() as u64) << 3);
        }
    }

    /// `SHA256Final`: pads, returns the digest (big-endian) and wipes the context.
    pub fn finalize(&mut self) -> [u8; SHA256_DIGEST_LENGTH] {
        let mut usedspace = self.buffered();
        let bitcount = self.bitcount.to_be_bytes();

        if usedspace > 0 {
            // Begin padding with a 1 bit:
            self.buffer[usedspace] = 0x80;
            usedspace += 1;

            if usedspace <= SHA256_SHORT_BLOCK_LENGTH {
                // Set-up for the last transform:
                self.buffer[usedspace..SHA256_SHORT_BLOCK_LENGTH].fill(0);
            } else {
                self.buffer[usedspace..].fill(0);
                // Do second-to-last transform:
                SHA256Transform(&mut self.state, &self.buffer);

                // And set-up for the last transform:
                self.buffer[..SHA256_SHORT_BLOCK_LENGTH].fill(0);
            }
        } else {
            // Set-up for the last transform:
            self.buffer[..SHA256_SHORT_BLOCK_LENGTH].fill(0);

            // Begin padding with a 1 bit:
            self.buffer[0] = 0x80;
        }
        // Set the bit count:
        self.buffer[SHA256_SHORT_BLOCK_LENGTH..].copy_from_slice(&bitcount);

        // Final transform:
        SHA256Transform(&mut self.state, &self.buffer);

        let mut digest = [0u8; SHA256_DIGEST_LENGTH];
        for (out, word) in digest.as_chunks_mut::<4>().0.iter_mut().zip(self.state) {
            *out = word.to_be_bytes();
        }
        // Clean up state data:
        wipe(self);
        digest
    }
}

/// `SHA2_CTX` as SHA-384 and SHA-512 use it: the 64-bit state, the 128-bit bit count and the
/// 128-byte buffer, with the update and padding the two hashes share.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
struct Sha512Core {
    /// `state.st64`: the eight chaining words.
    state: [u64; 8],
    /// `bitcount`: the message length so far, in bits, a 128-bit value (low word first).
    bitcount: [u64; 2],
    /// `buffer`: the partial block.
    buffer: [u8; SHA512_BLOCK_LENGTH],
}

impl Default for Sha512Core {
    fn default() -> Self {
        Self {
            state: [0; 8],
            bitcount: [0; 2],
            buffer: [0; SHA512_BLOCK_LENGTH],
        }
    }
}

impl Sha512Core {
    /// `SHA384Init`/`SHA512Init`: the initial hash value `iv`, nothing hashed yet.
    fn new(iv: [u64; 8]) -> Self {
        Self {
            state: iv,
            bitcount: [0; 2],
            buffer: [0; SHA512_BLOCK_LENGTH],
        }
    }

    /// The bytes waiting in `buffer`: the message length mod 128. The value is below 128, so
    /// the conversion to `usize` is exact.
    fn buffered(&self) -> usize {
        ((self.bitcount[0] >> 3) % SHA512_BLOCK_LENGTH as u64) as usize
    }

    /// `SHA512Update` (and `SHA384Update`).
    fn update(&mut self, data: &[u8]) {
        let mut data = data;

        // Calling with no data is valid (we do nothing)
        if data.is_empty() {
            return;
        }

        let usedspace = self.buffered();
        if usedspace > 0 {
            // Calculate how much free space is available in the buffer
            let freespace = SHA512_BLOCK_LENGTH - usedspace;

            if data.len() >= freespace {
                // Fill the buffer completely and process it
                let (head, rest) = data.split_at(freespace);
                self.buffer[usedspace..].copy_from_slice(head);
                addinc128(&mut self.bitcount, (freespace as u64) << 3);
                data = rest;
                SHA512Transform(&mut self.state, &self.buffer);
            } else {
                // The buffer is not yet full
                self.buffer[usedspace..usedspace + data.len()].copy_from_slice(data);
                addinc128(&mut self.bitcount, (data.len() as u64) << 3);
                return;
            }
        }
        // Process as many complete blocks as we can
        let (blocks, rest) = data.as_chunks::<SHA512_BLOCK_LENGTH>();
        for block in blocks {
            SHA512Transform(&mut self.state, block);
            addinc128(&mut self.bitcount, (SHA512_BLOCK_LENGTH as u64) << 3);
        }
        if !rest.is_empty() {
            // There's left-overs, so save 'em
            self.buffer[..rest.len()].copy_from_slice(rest);
            addinc128(&mut self.bitcount, (rest.len() as u64) << 3);
        }
    }

    /// `SHA512Last`: pads the last block with the 128-bit length and runs the final transform.
    fn last(&mut self) {
        let mut usedspace = self.buffered();
        let bitcount = self.bitcount;

        if usedspace > 0 {
            // Begin padding with a 1 bit:
            self.buffer[usedspace] = 0x80;
            usedspace += 1;

            if usedspace <= SHA512_SHORT_BLOCK_LENGTH {
                // Set-up for the last transform:
                self.buffer[usedspace..SHA512_SHORT_BLOCK_LENGTH].fill(0);
            } else {
                self.buffer[usedspace..].fill(0);
                // Do second-to-last transform:
                SHA512Transform(&mut self.state, &self.buffer);

                // And set-up for the last transform:
                self.buffer[..SHA512_BLOCK_LENGTH - 2].fill(0);
            }
        } else {
            // Prepare for final transform:
            self.buffer[..SHA512_SHORT_BLOCK_LENGTH].fill(0);

            // Begin padding with a 1 bit:
            self.buffer[0] = 0x80;
        }
        // Store the length of input data (in bits):
        let (high, low) = self.buffer[SHA512_SHORT_BLOCK_LENGTH..].split_at_mut(8);
        high.copy_from_slice(&bitcount[1].to_be_bytes());
        low.copy_from_slice(&bitcount[0].to_be_bytes());

        // Final transform:
        SHA512Transform(&mut self.state, &self.buffer);
    }

    /// The first `N` bytes of the big-endian state: the digest once [`Sha512Core::last`] ran.
    fn digest<const N: usize>(&self) -> [u8; N] {
        let mut digest = [0u8; N];
        for (out, word) in digest.chunks_mut(8).zip(self.state) {
            out.copy_from_slice(&word.to_be_bytes()[..out.len()]);
        }
        digest
    }
}

/// `SHA2_CTX` as SHA-384 uses it: a SHA-384 hash in progress, from [`Sha384Ctx::new`] to
/// [`Sha384Ctx::finalize`].
///
/// `Default` is the wiped, all-zero context that `finalize` leaves behind, not the start of a
/// hash.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Sha384Ctx(Sha512Core);

impl Sha384Ctx {
    /// `SHA384Init`.
    pub fn new() -> Self {
        Self(Sha512Core::new(SHA384_INITIAL_HASH_VALUE))
    }

    /// `SHA384Update`: the same as SHA-512's.
    pub fn update(&mut self, data: &[u8]) {
        self.0.update(data);
    }

    /// `SHA384Final`: SHA-512's padding, the first six state words as the digest; the context
    /// is wiped.
    pub fn finalize(&mut self) -> [u8; SHA384_DIGEST_LENGTH] {
        self.0.last();

        // Save the hash data for output:
        let digest = self.0.digest();

        // Zero out state data
        wipe(self);
        digest
    }
}

/// `SHA2_CTX` as SHA-512 uses it: a SHA-512 hash in progress, from [`Sha512Ctx::new`] to
/// [`Sha512Ctx::finalize`].
///
/// `Default` is the wiped, all-zero context that `finalize` leaves behind, not the start of a
/// hash.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Sha512Ctx(Sha512Core);

impl Sha512Ctx {
    /// `SHA512Init`.
    pub fn new() -> Self {
        Self(Sha512Core::new(SHA512_INITIAL_HASH_VALUE))
    }

    /// `SHA512Update`.
    pub fn update(&mut self, data: &[u8]) {
        self.0.update(data);
    }

    /// `SHA512Final`: pads, returns the digest (big-endian) and wipes the context.
    pub fn finalize(&mut self) -> [u8; SHA512_DIGEST_LENGTH] {
        self.0.last();

        // Save the hash data for output:
        let digest = self.0.digest();

        // Zero out state data
        wipe(self);
        digest
    }
}

/// `ADDINC128`: incrementally adds the unsigned 64-bit integer `n` to the unsigned 128-bit
/// integer `w` (a two-element array of 64-bit words, low first).
fn addinc128(w: &mut [u64; 2], n: u64) {
    w[0] = w[0].wrapping_add(n);
    if w[0] < n {
        w[1] = w[1].wrapping_add(1);
    }
}

/// `Ch`.
fn ch32(x: u32, y: u32, z: u32) -> u32 {
    (x & y) ^ (!x & z)
}

/// `Maj`.
fn maj32(x: u32, y: u32, z: u32) -> u32 {
    (x & y) ^ (x & z) ^ (y & z)
}

/// `Ch` on 64 bits.
fn ch64(x: u64, y: u64, z: u64) -> u64 {
    (x & y) ^ (!x & z)
}

/// `Maj` on 64 bits.
fn maj64(x: u64, y: u64, z: u64) -> u64 {
    (x & y) ^ (x & z) ^ (y & z)
}

/// `Sigma0_256`.
fn big_sigma0_256(x: u32) -> u32 {
    x.rotate_right(2) ^ x.rotate_right(13) ^ x.rotate_right(22)
}

/// `Sigma1_256`.
fn big_sigma1_256(x: u32) -> u32 {
    x.rotate_right(6) ^ x.rotate_right(11) ^ x.rotate_right(25)
}

/// `sigma0_256`.
fn sigma0_256(x: u32) -> u32 {
    x.rotate_right(7) ^ x.rotate_right(18) ^ (x >> 3)
}

/// `sigma1_256`.
fn sigma1_256(x: u32) -> u32 {
    x.rotate_right(17) ^ x.rotate_right(19) ^ (x >> 10)
}

/// `Sigma0_512`.
fn big_sigma0_512(x: u64) -> u64 {
    x.rotate_right(28) ^ x.rotate_right(34) ^ x.rotate_right(39)
}

/// `Sigma1_512`.
fn big_sigma1_512(x: u64) -> u64 {
    x.rotate_right(14) ^ x.rotate_right(18) ^ x.rotate_right(41)
}

/// `sigma0_512`.
fn sigma0_512(x: u64) -> u64 {
    x.rotate_right(1) ^ x.rotate_right(8) ^ (x >> 7)
}

/// `sigma1_512`.
fn sigma1_512(x: u64) -> u64 {
    x.rotate_right(19) ^ x.rotate_right(61) ^ (x >> 6)
}

/// `SHA256Transform`: the compression function over one 64-byte block.
#[allow(non_snake_case)] // the C name
pub fn SHA256Transform(state: &mut [u32; 8], data: &[u8; SHA256_BLOCK_LENGTH]) {
    let mut w256 = [0u32; 16];

    // Initialize registers with the prev. intermediate value
    let [mut a, mut b, mut c, mut d, mut e, mut f, mut g, mut h] = *state;

    for j in 0..64 {
        if j < 16 {
            w256[j] = u32::from_be_bytes([
                data[4 * j],
                data[4 * j + 1],
                data[4 * j + 2],
                data[4 * j + 3],
            ]);
        } else {
            // Part of the message block expansion:
            let s0 = sigma0_256(w256[(j + 1) & 0x0f]);
            let s1 = sigma1_256(w256[(j + 14) & 0x0f]);
            w256[j & 0x0f] = w256[j & 0x0f]
                .wrapping_add(s1)
                .wrapping_add(w256[(j + 9) & 0x0f])
                .wrapping_add(s0);
        }
        // Apply the SHA-256 compression function to update a..h
        let t1 = h
            .wrapping_add(big_sigma1_256(e))
            .wrapping_add(ch32(e, f, g))
            .wrapping_add(K256[j])
            .wrapping_add(w256[j & 0x0f]);
        let t2 = big_sigma0_256(a).wrapping_add(maj32(a, b, c));
        h = g;
        g = f;
        f = e;
        e = d.wrapping_add(t1);
        d = c;
        c = b;
        b = a;
        a = t1.wrapping_add(t2);
    }

    // Compute the current intermediate hash value
    for (s, v) in state.iter_mut().zip([a, b, c, d, e, f, g, h]) {
        *s = s.wrapping_add(v);
    }
}

/// `SHA512Transform`: the compression function over one 128-byte block.
#[allow(non_snake_case)] // the C name
pub fn SHA512Transform(state: &mut [u64; 8], data: &[u8; SHA512_BLOCK_LENGTH]) {
    let mut w512 = [0u64; 16];

    // Initialize registers with the prev. intermediate value
    let [mut a, mut b, mut c, mut d, mut e, mut f, mut g, mut h] = *state;

    for j in 0..80 {
        if j < 16 {
            let mut word = [0u8; 8];
            word.copy_from_slice(&data[8 * j..8 * j + 8]);
            w512[j] = u64::from_be_bytes(word);
        } else {
            // Part of the message block expansion:
            let s0 = sigma0_512(w512[(j + 1) & 0x0f]);
            let s1 = sigma1_512(w512[(j + 14) & 0x0f]);
            w512[j & 0x0f] = w512[j & 0x0f]
                .wrapping_add(s1)
                .wrapping_add(w512[(j + 9) & 0x0f])
                .wrapping_add(s0);
        }
        // Apply the SHA-512 compression function to update a..h
        let t1 = h
            .wrapping_add(big_sigma1_512(e))
            .wrapping_add(ch64(e, f, g))
            .wrapping_add(K512[j])
            .wrapping_add(w512[j & 0x0f]);
        let t2 = big_sigma0_512(a).wrapping_add(maj64(a, b, c));
        h = g;
        g = f;
        f = e;
        e = d.wrapping_add(t1);
        d = c;
        c = b;
        b = a;
        a = t1.wrapping_add(t2);
    }

    // Compute the current intermediate hash value
    for (s, v) in state.iter_mut().zip([a, b, c, d, e, f, g, h]) {
        *s = s.wrapping_add(v);
    }
}
/* </CODE> */

/* <TESTS> */
#[cfg(test)]
mod tests {
    // Known-answer tests for SHA-256, SHA-384 and SHA-512: the FIPS 180-4 examples ("abc", the
    // 448-bit and 896-bit messages, one million "a"), the empty message, and the digest of the
    // digests of every length around the block boundaries (`hashlib`); a property test that
    // any split of a random message gives the one-shot digest.

    use super::*;
    use crate::crypto::testutil::{XorShift, c_table, hex};

    extern crate std;
    use std::vec::Vec;

    fn sha256(data: &[u8]) -> [u8; SHA256_DIGEST_LENGTH] {
        let mut ctx = Sha256Ctx::new();
        ctx.update(data);
        ctx.finalize()
    }

    fn sha384(data: &[u8]) -> [u8; SHA384_DIGEST_LENGTH] {
        let mut ctx = Sha384Ctx::new();
        ctx.update(data);
        ctx.finalize()
    }

    fn sha512(data: &[u8]) -> [u8; SHA512_DIGEST_LENGTH] {
        let mut ctx = Sha512Ctx::new();
        ctx.update(data);
        ctx.finalize()
    }

    const M448: &[u8] = b"abcdbcdecdefdefgefghfghighijhijkijkljklmklmnlmnomnopnopq";
    const M896: &[u8] = b"abcdefghbcdefghicdefghijdefghijkefghijklfghijklmghijklmnhijklmnoijklmnopjklmnopqklmnopqrlmnopqrsmnopqrstnopqrstu";

    #[test]
    fn sha256_fips_180_4() {
        assert_eq!(
            sha256(b"abc").to_vec(),
            hex("ba7816bf8f01cfea414140de5dae2223b00361a396177a9cb410ff61f20015ad")
        );
        assert_eq!(
            sha256(M448).to_vec(),
            hex("248d6a61d20638b8e5c026930c3e6039a33ce45964ff2167f6ecedd419db06c1")
        );
        assert_eq!(
            sha256(M896).to_vec(),
            hex("cf5b16a778af8380036ce59e7b0492370b249b11e8f07a51afac45037afee9d1")
        );
        assert_eq!(
            sha256(b"").to_vec(),
            hex("e3b0c44298fc1c149afbf4c8996fb92427ae41e4649b934ca495991b7852b855")
        );
    }

    #[test]
    fn sha384_fips_180_4() {
        assert_eq!(
            sha384(b"abc").to_vec(),
            hex(
                "cb00753f45a35e8bb5a03d699ac65007272c32ab0eded1631a8b605a43ff5bed8086072ba1e7cc2358baeca134c825a7"
            )
        );
        assert_eq!(
            sha384(M448).to_vec(),
            hex(
                "3391fdddfc8dc7393707a65b1b4709397cf8b1d162af05abfe8f450de5f36bc6b0455a8520bc4e6f5fe95b1fe3c8452b"
            )
        );
        assert_eq!(
            sha384(M896).to_vec(),
            hex(
                "09330c33f71147e83d192fc782cd1b4753111b173b3b05d22fa08086e3b0f712fcc7c71a557e2db966c3e9fa91746039"
            )
        );
        assert_eq!(
            sha384(b"").to_vec(),
            hex(
                "38b060a751ac96384cd9327eb1b1e36a21fdb71114be07434c0cc7bf63f6e1da274edebfe76f65fbd51ad2f14898b95b"
            )
        );
    }

    #[test]
    fn sha512_fips_180_4() {
        assert_eq!(
            sha512(b"abc").to_vec(),
            hex(
                "ddaf35a193617abacc417349ae20413112e6fa4e89a97ea20a9eeee64b55d39a2192992a274fc1a836ba3c23a3feebbd454d4423643ce80e2a9ac94fa54ca49f"
            )
        );
        assert_eq!(
            sha512(M448).to_vec(),
            hex(
                "204a8fc6dda82f0a0ced7beb8e08a41657c16ef468b228a8279be331a703c33596fd15c13b1b07f9aa1d3bea57789ca031ad85c7a71dd70354ec631238ca3445"
            )
        );
        assert_eq!(
            sha512(M896).to_vec(),
            hex(
                "8e959b75dae313da8cf4f72814fc143f8f7779c6eb9f7fa17299aeadb6889018501d289e4900f7e4331b99dec4b5433ac7d329eeb6dd26545e96e55b874be909"
            )
        );
        assert_eq!(
            sha512(b"").to_vec(),
            hex(
                "cf83e1357eefb8bdf1542850d66d8007d620e4050b5715dc83f4a921d36ce9ce47d0d13c5d85f2b0ff8318d2877eec2f63b931bd47417a81a538327af927da3e"
            )
        );
    }

    #[test]
    fn one_million_a() {
        let chunk = [b'a'; 1000];
        let mut ctx = Sha256Ctx::new();
        for _ in 0..1000 {
            ctx.update(&chunk);
        }
        let out = ctx.finalize();
        assert_eq!(
            out.to_vec(),
            hex("cdc76e5c9914fb9281a1c7e284d73e67f1809a48a497200e046d39ccc7112cd0")
        );

        let mut ctx = Sha384Ctx::new();
        for _ in 0..1000 {
            ctx.update(&chunk);
        }
        let out = ctx.finalize();
        assert_eq!(
            out.to_vec(),
            hex(
                "9d0e1809716474cb086e834e310a4a1ced149e9c00f248527972cec5704c2a5b07b8b3dc38ecc4ebae97ddd87f3d8985"
            )
        );

        let mut ctx = Sha512Ctx::new();
        for _ in 0..1000 {
            ctx.update(&chunk);
        }
        let out = ctx.finalize();
        assert_eq!(
            out.to_vec(),
            hex(
                "e718483d0ce769644e2e42c7bc15b4638e1f98b13b2044285632a803afa973ebde0ff244877ea60a4cb0432ce577c31beb009c5c2c49aa2e4eadb217ad8cc09b"
            )
        );
    }

    fn sweep<const N: usize>(blk: usize, digest: fn(&[u8]) -> [u8; N]) -> [u8; N] {
        let mut acc: Vec<u8> = Vec::new();
        for n in 0..(3 * blk + 2) {
            let msg: Vec<u8> = (0..n).map(|i| ((i * 7 + n) % 256) as u8).collect();
            acc.extend_from_slice(&digest(&msg));
        }
        digest(&acc)
    }

    #[test]
    fn every_length_around_the_blocks() {
        assert_eq!(
            sweep(SHA256_BLOCK_LENGTH, sha256).to_vec(),
            hex("175cf2a39abf9ec5bbb7e311fb9566d1c1b9cc7dc09580b1df88e18dfb14750b")
        );
        assert_eq!(
            sweep(SHA384_BLOCK_LENGTH, sha384).to_vec(),
            hex(
                "732b07eefd9a412195c2952aaae076bdd31d960cea58a03d2696ccba24df572ce3ee71990c5579cbf4f19867acb38526"
            )
        );
        assert_eq!(
            sweep(SHA512_BLOCK_LENGTH, sha512).to_vec(),
            hex(
                "6f71e9c06ce7c180c8ba541024f90374c6935148385579c4edf7bfed6c98b2f08056c35f118427b9ecd35334b44b6432e55b60de9845cd087445ed5586f777de"
            )
        );
    }

    #[test]
    fn any_split_of_the_input_gives_the_same_digest() {
        let msg: Vec<u8> = (0..400).map(|i| (i * 13 % 256) as u8).collect();
        let (w256, w384, w512) = (sha256(&msg), sha384(&msg), sha512(&msg));
        for chunk in [
            1usize, 3, 7, 55, 56, 63, 64, 65, 111, 112, 127, 128, 129, 399,
        ] {
            let mut ctx = Sha256Ctx::new();
            for piece in msg.chunks(chunk) {
                ctx.update(piece);
            }
            let o256 = ctx.finalize();
            assert_eq!(o256, w256, "sha256 by {chunk}");
            // Final wipes the context.
            assert_eq!(ctx, Sha256Ctx::default());

            let mut ctx = Sha384Ctx::new();
            for piece in msg.chunks(chunk) {
                ctx.update(piece);
            }
            let o384 = ctx.finalize();
            assert_eq!(o384, w384, "sha384 by {chunk}");

            let mut ctx = Sha512Ctx::new();
            for piece in msg.chunks(chunk) {
                ctx.update(piece);
            }
            let o512 = ctx.finalize();
            assert_eq!(o512, w512, "sha512 by {chunk}");
        }
    }

    #[test]
    fn random_splits_give_the_one_shot_digests() {
        let mut rng = XorShift::new(0x7368_6132_7370_6c74);
        for _ in 0..200 {
            let len = rng.below(700);
            let msg = rng.bytes(len);
            let (mut c256, mut c384, mut c512) =
                (Sha256Ctx::new(), Sha384Ctx::new(), Sha512Ctx::new());
            for piece in rng.split(&msg) {
                c256.update(piece);
            }
            for piece in rng.split(&msg) {
                c384.update(piece);
            }
            for piece in rng.split(&msg) {
                c512.update(piece);
            }
            assert_eq!(c256.finalize(), sha256(&msg), "sha256, {len} bytes");
            assert_eq!(c384.finalize(), sha384(&msg), "sha384, {len} bytes");
            assert_eq!(c512.finalize(), sha512(&msg), "sha512, {len} bytes");
            // finalize wipes each context where it lives.
            assert_eq!(c256, Sha256Ctx::default());
            assert_eq!(c384, Sha384Ctx::default());
            assert_eq!(c512, Sha512Ctx::default());
        }
    }

    #[test]
    #[ignore = "reads the C tables from $OPENBSD_SRC (just test-ref)"]
    fn constants_match_the_c_file() {
        let f = "sys/crypto/sha2.c";
        let same32 = |name: &str, table: &[u32]| {
            let c = c_table(f, name);
            assert_eq!(c.len(), table.len(), "{name}");
            for (i, (a, b)) in c.iter().zip(table).enumerate() {
                assert_eq!(*a, u64::from(*b), "{name}[{i}]");
            }
        };
        let same64 = |name: &str, table: &[u64]| {
            assert_eq!(c_table(f, name), table, "{name}");
        };
        same32("K256", &K256);
        same32("sha256_initial_hash_value", &SHA256_INITIAL_HASH_VALUE);
        same64("K512", &K512);
        same64("sha384_initial_hash_value", &SHA384_INITIAL_HASH_VALUE);
        same64("sha512_initial_hash_value", &SHA512_INITIAL_HASH_VALUE);
    }
}
/* </TESTS> */
