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
 * Copyright (c) 2010 Mike Belopuhov
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
/* </LICENSES> */

/* <CODE> */
//! The message authentication part of the Galois/Counter Mode (as described in RFC 4543)
//! using the AES cipher: GHASH over the data, then the tag is the hash xor the encrypted
//! first counter block. FIPS SP 800-38D describes the algorithm details. IPsec's
//! `AES-GMAC` and `AES-GCM` ESP transforms (RFC 4106, 4543) use it through `xform.c`.
//!
//! Upstream: sys/crypto/gmac.h @ 3ce1f3f79392, sys/crypto/gmac.c @ 3ce1f3f79392
//! LZ: sys/crypto/gmac.rs@f5985f1d055a
//!
//! ## Deviations
//! - The header and the file share this module; `GHASH_CTX` is [`GhashCtx`] and
//!   `AES_GMAC_CTX` is [`AesGmacCtx`].
//! - `ghash_update` is a `void (*)(GHASH_CTX *, uint8_t *, size_t)` global that machine
//!   dependent code may override with an optimised routine (amd64 does with PCLMULQDQ,
//!   `ghash_update_pclmul`, in `arch/amd64/amd64/aesni.c`, which is not ported). Here it is
//!   the method [`GhashCtx::update`], which is `ghash_update_mi`; the override waits for
//!   aesni.
//! - `ghash_gfmul` and `ghash_update_mi` work on the 16-byte blocks as byte arrays. The C
//!   casts the arrays to `uint32_t *` and uses the words only for the xors and as big-endian
//!   words in the multiplication, so the results are the same.
//! - `AES_GMAC_Setkey` returns `Result<(), Errno>` (`EINVAL` for an AES key of a size other
//!   than 16, 24 or 32 bytes) where the C returns `void` and ignores `AES_Setkey`'s result.
//!   The arguments are slices for pointer-and-length pairs. `Final` wipes the keystream block
//!   with `explicit_bzero`.
//!
//! ## Redesign
//! - The functions over a context are methods: `ghash_update_mi` and `ghash_update` are
//!   [`GhashCtx::update_mi`] and [`GhashCtx::update`]; `AES_GMAC_Init`, `_Setkey`, `_Reinit`,
//!   `_Update` and `_Final` are [`AesGmacCtx::init`], [`AesGmacCtx::setkey`],
//!   [`AesGmacCtx::reinit`], [`AesGmacCtx::update`] and [`AesGmacCtx::finalize`] (LZ: free
//!   functions with the context first). The framework's `Init`, `Setkey`, `Reinit`, `Update`,
//!   `Final` order is unchanged, so keying stays a method on an initialised context.
//! - `ghash_gfmul` returns the product (LZ: an `&mut` out parameter); `update` returns nothing
//!   (LZ: `Result<(), Errno>` for the C's `int` that is always 0, which `xform.rs` still
//!   reports); `finalize` returns the tag and wipes the context in place, keys included (LZ:
//!   an `&mut [u8; 16]` out parameter and the context kept as it was).
//! - [`GhashCtx`] (the hash subkey `H = E(K, 0)` and the running hash) zeroes itself when
//!   dropped, as the AES key in [`AesGmacCtx`] does (`docs/IDIOMS.md`); neither is `Copy` or
//!   `PartialEq` any more.
//! - Constant time: `ghash_gfmul` keeps the C's masks (each bit of `x` turned into an all-ones
//!   or all-zero word, the reduction by the low bit of `V` likewise), with no branch on the
//!   data or the key; the loops run over the 128 bit positions and the data length.

use libkern::explicit_bzero;

use super::aes::AesCtx;
use super::wipe;
use crate::sys::errno::Errno;

/// `GMAC_BLOCK_LEN`.
pub const GMAC_BLOCK_LEN: usize = 16;
/// `GMAC_DIGEST_LEN`.
pub const GMAC_DIGEST_LEN: usize = 16;

/// `AESCTR_NONCESIZE`: bytes of salt at the end of the key material.
const AESCTR_NONCESIZE: usize = 4;

/// `GHASH_CTX`: the hash subkey and the running hash. Zeroed when dropped.
#[derive(Clone, Debug, Default)]
pub struct GhashCtx {
    /// `H`: hash subkey.
    pub h: [u8; GMAC_BLOCK_LEN],
    /// `S`: state.
    pub s: [u8; GMAC_BLOCK_LEN],
    /// `Z`: initial state.
    pub z: [u8; GMAC_BLOCK_LEN],
}

impl Drop for GhashCtx {
    /// Wipes the subkey and the state (`docs/IDIOMS.md`: a key schedule is zeroed when
    /// dropped).
    fn drop(&mut self) {
        self.zeroize();
    }
}

impl GhashCtx {
    /// Zeroes the subkey and the state with `explicit_bzero`.
    pub(crate) fn zeroize(&mut self) {
        explicit_bzero(&mut self.h);
        explicit_bzero(&mut self.s);
        explicit_bzero(&mut self.z);
    }

    /// `ghash_update_mi`: the machine independent GHASH update: absorbs the whole blocks of
    /// `x` (a trailing partial block is ignored).
    pub fn update_mi(&mut self, x: &[u8]) {
        let mut y = self.z;

        for blk in x.as_chunks::<GMAC_BLOCK_LEN>().0 {
            let mut s = [0u8; GMAC_BLOCK_LEN];
            for ((s, y), b) in s.iter_mut().zip(y).zip(blk) {
                *s = y ^ b;
            }
            self.s = ghash_gfmul(&s, &self.h);
            y = self.s;
        }
        self.z = self.s;
    }

    /// `ghash_update`: the GHASH update in use (see the deviations: always `update_mi`).
    pub fn update(&mut self, x: &[u8]) {
        self.update_mi(x);
    }
}

/// `AES_GMAC_CTX`: an authentication in progress. Its key and hash state are zeroed when it
/// drops.
#[derive(Clone, Debug, Default)]
pub struct AesGmacCtx {
    /// `ghash`.
    pub ghash: GhashCtx,
    /// `K`: the AES key.
    pub k: AesCtx,
    /// `J`: counter block.
    pub j: [u8; GMAC_BLOCK_LEN],
}

impl AesGmacCtx {
    /// `AES_GMAC_Init`: clears the hash state and the counter block (not the key).
    pub fn init(&mut self) {
        wipe(&mut self.ghash.h);
        wipe(&mut self.ghash.s);
        wipe(&mut self.ghash.z);
        wipe(&mut self.j);
    }

    /// `AES_GMAC_Setkey`: the AES key (16, 24 or 32 bytes) followed by the 4-byte salt;
    /// `EINVAL` for another AES key size.
    pub fn setkey(&mut self, key: &[u8]) -> Result<(), Errno> {
        let klen = key.len();
        if klen < AESCTR_NONCESIZE {
            return Err(Errno::EINVAL);
        }
        let (aes_key, salt) = key.split_at(klen - AESCTR_NONCESIZE);
        self.k = AesCtx::new(aes_key)?;

        // copy out salt to the counter block
        self.j[..AESCTR_NONCESIZE].copy_from_slice(salt);

        // prepare a hash subkey
        self.ghash.h = self.k.encrypt(&self.ghash.h);
        Ok(())
    }

    /// `AES_GMAC_Reinit`: starts a message under the IV (8 bytes in IPsec), copied into the
    /// counter block after the salt.
    pub fn reinit(&mut self, iv: &[u8]) {
        // copy out IV to the counter block
        self.j[AESCTR_NONCESIZE..AESCTR_NONCESIZE + iv.len()].copy_from_slice(iv);
    }

    /// `AES_GMAC_Update`: authenticates `data`, zero-padding a last partial block.
    pub fn update(&mut self, data: &[u8]) {
        let (blocks, rest) = data.as_chunks::<GMAC_BLOCK_LEN>();

        if !blocks.is_empty() {
            self.ghash.update(blocks.as_flattened());
        }
        if !rest.is_empty() {
            let mut blk = [0u8; GMAC_BLOCK_LEN];
            blk[..rest.len()].copy_from_slice(rest);
            self.ghash.update(&blk);
        }
    }

    /// `AES_GMAC_Final`: the 16-byte tag: the hash xor the encryption of the counter block 1.
    /// The context, keys included, is wiped in place.
    pub fn finalize(&mut self) -> [u8; GMAC_DIGEST_LEN] {
        // do one round of GCTR
        self.j[GMAC_BLOCK_LEN - 1] = 1;
        let mut keystream = self.k.encrypt(&self.j);
        let mut digest = [0u8; GMAC_DIGEST_LEN];
        for ((d, s), k) in digest.iter_mut().zip(self.ghash.s).zip(keystream) {
            *d = s ^ k;
        }
        explicit_bzero(&mut keystream);
        self.zeroize();
        digest
    }

    /// Zeroes the hash state, the AES key and the counter block.
    pub(crate) fn zeroize(&mut self) {
        self.ghash.zeroize();
        self.k.zeroize();
        explicit_bzero(&mut self.j);
    }
}

/// `ghash_gfmul`: the product of `x` and `y` in GF(2^128).
pub fn ghash_gfmul(x: &[u8; GMAC_BLOCK_LEN], y: &[u8; GMAC_BLOCK_LEN]) -> [u8; GMAC_BLOCK_LEN] {
    let mut v = [0u32; 4];
    let mut z = [0u32; 4];

    for (w, b) in v.iter_mut().zip(y.as_chunks::<4>().0) {
        *w = u32::from_be_bytes(*b);
    }

    for i in 0..GMAC_BLOCK_LEN * 8 {
        // update Z
        let mut mask = u32::from(x[i >> 3] & (1 << (!i & 7)) != 0);
        mask = !mask.wrapping_sub(1);
        z[0] ^= v[0] & mask;
        z[1] ^= v[1] & mask;
        z[2] ^= v[2] & mask;
        z[3] ^= v[3] & mask;

        // update V
        mask = !(v[3] & 1).wrapping_sub(1);
        v[3] = (v[2] << 31) | (v[3] >> 1);
        v[2] = (v[1] << 31) | (v[2] >> 1);
        v[1] = (v[0] << 31) | (v[1] >> 1);
        v[0] = (v[0] >> 1) ^ (0xe1000000 & mask);
    }

    let mut product = [0u8; GMAC_BLOCK_LEN];
    for (b, w) in product.as_chunks_mut::<4>().0.iter_mut().zip(z) {
        *b = w.to_be_bytes();
    }
    product
}
/* </CODE> */

/* <TESTS> */
#[cfg(test)]
mod tests {
    // Known-answer tests for GHASH and the GMAC context: the Galois/Counter Mode test cases 1 to
    // 4 of McGrew and Viega's specification (and its 192 and 256-bit key cases 10 and 16), run
    // the way `swcr_authenc` drives the authenticator: salt and key together, the IV, the
    // additional data, the ciphertext, the length block. GMAC-only tags (AAD with no ciphertext,
    // RFC 4543) come from an independent big-integer GHASH over `openssl`'s AES.

    use super::*;
    use crate::crypto::testutil::{hex, hexn};

    extern crate std;
    use std::vec::Vec;

    /// Authenticates `aad` and `ct` under `key` and the 12-byte `iv` (4 of salt, 8 of IV): the
    /// tag the way the software crypto driver computes it.
    fn tag(key: &[u8], iv: &[u8], aad: &[u8], ct: &[u8]) -> [u8; GMAC_DIGEST_LEN] {
        let mut ctx = AesGmacCtx::default();
        let mut material = key.to_vec();
        material.extend_from_slice(&iv[..4]);

        ctx.init();
        assert_eq!(ctx.setkey(&material), Ok(()));
        ctx.reinit(&iv[4..]);
        ctx.update(aad);
        ctx.update(ct);

        // The length block: the bit lengths, big-endian, in the low word of each half.
        let mut blk = [0u8; GMAC_BLOCK_LEN];
        blk[4..8].copy_from_slice(&((aad.len() * 8) as u32).to_be_bytes());
        blk[12..16].copy_from_slice(&((ct.len() * 8) as u32).to_be_bytes());
        ctx.update(&blk);

        ctx.finalize()
    }

    fn k128() -> Vec<u8> {
        hex("feffe9928665731c6d6a8f9467308308")
    }

    fn iv() -> Vec<u8> {
        hex("cafebabefacedbaddecaf888")
    }

    fn a() -> Vec<u8> {
        hex("feedfacedeadbeeffeedfacedeadbeefabaddad2")
    }

    #[test]
    fn gcm_spec_test_case_1() {
        // Zero key, no data: the tag is the encrypted counter block alone (the hash is zero).
        assert_eq!(
            tag(&[0; 16], &[0; 12], &[], &[]).to_vec(),
            hex("58e2fccefa7e3061367f1d57a4e7455a")
        );
    }

    #[test]
    fn gcm_spec_test_case_2() {
        let ct = hex("0388dace60b6a392f328c2b971b2fe78");
        assert_eq!(
            tag(&[0; 16], &[0; 12], &[], &ct).to_vec(),
            hex("ab6e47d42cec13bdf53a67b21257bddf")
        );
    }

    #[test]
    fn gcm_spec_test_case_3() {
        let ct = hex(
            "42831ec2217774244b7221b784d0d49ce3aa212f2c02a4e035c17e2329aca12e
                  21d514b25466931c7d8f6a5aac84aa051ba30b396a0aac973d58e091473f5985",
        );
        assert_eq!(
            tag(&k128(), &iv(), &[], &ct).to_vec(),
            hex("4d5c2af327cd64a62cf35abd2ba6fab4")
        );
    }

    #[test]
    fn gcm_spec_test_case_4_with_additional_data_and_a_partial_block() {
        let ct = hex(
            "42831ec2217774244b7221b784d0d49ce3aa212f2c02a4e035c17e2329aca12e
                  21d514b25466931c7d8f6a5aac84aa051ba30b396a0aac973d58e091",
        );
        assert_eq!(ct.len(), 60);
        assert_eq!(
            tag(&k128(), &iv(), &a(), &ct).to_vec(),
            hex("5bc94fbc3221a5db94fae95ae7121a47")
        );
    }

    #[test]
    fn gcm_spec_test_cases_10_and_16_with_192_and_256_bit_keys() {
        let k192 = hex("feffe9928665731c6d6a8f9467308308feffe9928665731c");
        let ct = hex(
            "3980ca0b3c00e841eb06fac4872a2757859e1ceaa6efd984628593b40ca1e19c
                  7d773d00c144c525ac619d18c84a3f4718e2448b2fe324d9ccda2710",
        );
        assert_eq!(
            tag(&k192, &iv(), &a(), &ct).to_vec(),
            hex("2519498e80f1478f37ba55bd6d27618c")
        );

        let mut k256 = k128();
        k256.extend_from_slice(&k128());
        let ct = hex(
            "522dc1f099567d07f47f37a32a84427d643a8cdcbfe5c0c97598a2bd2555d1aa
                  8cb08e48590dbb3da7b08b1056828838c5f61e6393ba7a0abcc9f662",
        );
        assert_eq!(
            tag(&k256, &iv(), &a(), &ct).to_vec(),
            hex("76fc6ece0f4e1768cddf8853bb2d551b")
        );
    }

    #[test]
    fn gmac_only_tags() {
        let k192 = hex("feffe9928665731c6d6a8f9467308308feffe9928665731c");
        let mut k256 = k128();
        k256.extend_from_slice(&k128());
        assert_eq!(
            tag(&k128(), &iv(), &a(), &[]).to_vec(),
            hex("346434fd51d5cd0c5887ec63e39b907a")
        );
        assert_eq!(
            tag(&k192, &iv(), &a(), &[]).to_vec(),
            hex("c8253387e5f78673d538a60d50527a92")
        );
        assert_eq!(
            tag(&k256, &iv(), &a(), &[]).to_vec(),
            hex("9f6be07603c0b0bd1272854063e9c9ba")
        );
    }

    #[test]
    fn ghash_multiplication() {
        // x * 1, where the field's one is the bit pattern 0x80 0 0 ..., is x.
        let mut one = [0u8; 16];
        one[0] = 0x80;
        let x: [u8; 16] = hexn("0388dace60b6a392f328c2b971b2fe78");
        assert_eq!(ghash_gfmul(&x, &one), x);
        assert_eq!(ghash_gfmul(&one, &x), x);
        // And 0 * x is 0; the product commutes.
        assert_eq!(ghash_gfmul(&[0; 16], &x), [0; 16]);
        let y: [u8; 16] = hexn("66e94bd4ef8a2c3b884cfa59ca342b2e");
        assert_eq!(ghash_gfmul(&x, &y), ghash_gfmul(&y, &x));
        // The test case 2 hash: H = E(K, 0) = 66e94bd4ef8a2c3b884cfa59ca342b2e and the ciphertext
        // block c: GHASH(c || len) = c*H^2 + len*H; the first step c * H is
        // 5e2ec746917062882c85b0685353deb7 (the intermediate value of the spec's table).
        let out = ghash_gfmul(&hexn("0388dace60b6a392f328c2b971b2fe78"), &y);
        assert_eq!(out.to_vec(), hex("5e2ec746917062882c85b0685353deb7"));
    }

    #[test]
    fn bad_key_sizes_are_rejected() {
        let mut ctx = AesGmacCtx::default();
        ctx.init();
        // 4 bytes of salt after an AES key of 16, 24 or 32: the other lengths are errors.
        for len in [0usize, 3, 4, 19, 21, 27, 31, 37, 40] {
            assert_eq!(
                ctx.setkey(&std::vec![0u8; len]),
                Err(Errno::EINVAL),
                "{len}"
            );
        }
        assert_eq!(ctx.setkey(&[0u8; 20]), Ok(()));
    }

    /// xorshift64: the property tests' generator.
    fn next(state: &mut u64) -> u64 {
        *state ^= *state << 13;
        *state ^= *state >> 7;
        *state ^= *state << 17;
        *state
    }

    fn bytes(st: &mut u64, n: usize) -> Vec<u8> {
        (0..n).map(|_| next(st) as u8).collect()
    }

    #[test]
    fn whole_block_splits_and_flipped_bits() {
        // `update` pads each call's partial block, so the data may be split anywhere on a block
        // boundary without changing the tag; and every single flipped bit of the key material,
        // the IV or the data changes it.
        let mut st = 0x5851_f42d_4c95_7f2du64;
        for i in 0..40 {
            let key = bytes(&mut st, [16, 24, 32][i % 3]);
            let iv = bytes(&mut st, 12);
            let len = (next(&mut st) % 80) as usize;
            let data = bytes(&mut st, len);
            let whole = tag(&key, &iv, &data, &[]);

            let cut = 16 * (next(&mut st) as usize % (data.len() / 16 + 1));
            let mut ctx = AesGmacCtx::default();
            let mut material = key.clone();
            material.extend_from_slice(&iv[..4]);
            ctx.init();
            assert_eq!(ctx.setkey(&material), Ok(()));
            ctx.reinit(&iv[4..]);
            ctx.update(&data[..cut]);
            ctx.update(&data[cut..]);
            let mut blk = [0u8; GMAC_BLOCK_LEN];
            blk[4..8].copy_from_slice(&((data.len() * 8) as u32).to_be_bytes());
            ctx.update(&blk);
            assert_eq!(ctx.finalize(), whole, "cut at {cut} of {}", data.len());

            let bit = next(&mut st) as usize;
            let mut k2 = key.clone();
            let at = (bit / 8) % k2.len();
            k2[at] ^= 1 << (bit % 8);
            assert_ne!(tag(&k2, &iv, &data, &[]), whole);
            let mut iv2 = iv.clone();
            iv2[(bit / 8) % 12] ^= 1 << (bit % 8);
            assert_ne!(tag(&key, &iv2, &data, &[]), whole);
            if !data.is_empty() {
                let mut d2 = data.clone();
                let at = (bit / 8) % d2.len();
                d2[at] ^= 1 << (bit % 8);
                assert_ne!(tag(&key, &iv, &d2, &[]), whole);
            }
        }
    }

    #[test]
    fn finalize_wipes_the_context_in_place() {
        let mut ctx = AesGmacCtx::default();
        ctx.init();
        assert_eq!(ctx.setkey(&[0x11u8; 20]), Ok(()));
        ctx.reinit(&[0x22; 8]);
        ctx.update(b"some additional data");
        let _ = ctx.finalize();
        assert_eq!(
            (ctx.ghash.h, ctx.ghash.s, ctx.ghash.z, ctx.j),
            ([0; 16], [0; 16], [0; 16], [0; 16])
        );
        assert!(ctx.k.sk.iter().all(|w| *w == 0));
        assert!(ctx.k.sk_exp.iter().flatten().all(|w| *w == 0));
    }

    #[test]
    fn finalizing_twice_does_not_panic() {
        // The first finalize wipes the AES key (zero rounds); a second one must still be safe.
        let mut ctx = AesGmacCtx::default();
        ctx.init();
        assert_eq!(ctx.setkey(&[0x33u8; 20]), Ok(()));
        ctx.update(b"data");
        let _ = ctx.finalize();
        let _ = ctx.finalize();
        let _ = AesGmacCtx::default().finalize();
    }
}
/* </TESTS> */
