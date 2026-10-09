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
//!
//! ## Deviations
//! - The header and the file share this module; `GHASH_CTX` is [`GhashCtx`] and
//!   `AES_GMAC_CTX` is [`AesGmacCtx`].
//! - `ghash_update` is a `void (*)(GHASH_CTX *, uint8_t *, size_t)` global that machine
//!   dependent code may override with an optimised routine (amd64 does with PCLMULQDQ,
//!   `ghash_update_pclmul`, in `arch/amd64/amd64/aesni.c`, which is not ported). Here it is
//!   the function [`ghash_update`], which is `ghash_update_mi`; the override waits for aesni.
//! - `ghash_gfmul` and `ghash_update_mi` work on the 16-byte blocks as byte arrays. The C
//!   casts the arrays to `uint32_t *` and uses the words only for the xors and as big-endian
//!   words in the multiplication, so the results are the same.
//! - `AES_GMAC_Setkey` returns `Result<(), Errno>` (`EINVAL` for an AES key of a size other
//!   than 16, 24 or 32 bytes) where the C returns `void` and ignores `AES_Setkey`'s result;
//!   `AES_GMAC_Update` returns `Result<(), Errno>` for the `int` that is always 0. The
//!   arguments are slices for pointer-and-length pairs. `Final` wipes the keystream block with
//!   `explicit_bzero`.

use libkern::explicit_bzero;

use super::aes::AesCtx;
use crate::sys::errno::Errno;

/// `GMAC_BLOCK_LEN`.
pub const GMAC_BLOCK_LEN: usize = 16;
/// `GMAC_DIGEST_LEN`.
pub const GMAC_DIGEST_LEN: usize = 16;

/// `AESCTR_NONCESIZE`: bytes of salt at the end of the key material.
const AESCTR_NONCESIZE: usize = 4;

/// `GHASH_CTX`.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct GhashCtx {
    /// `H`: hash subkey.
    pub h: [u8; GMAC_BLOCK_LEN],
    /// `S`: state.
    pub s: [u8; GMAC_BLOCK_LEN],
    /// `Z`: initial state.
    pub z: [u8; GMAC_BLOCK_LEN],
}

/// `AES_GMAC_CTX`.
#[derive(Clone, Debug, Default)]
pub struct AesGmacCtx {
    /// `ghash`.
    pub ghash: GhashCtx,
    /// `K`: the AES key.
    pub k: AesCtx,
    /// `J`: counter block.
    pub j: [u8; GMAC_BLOCK_LEN],
}

/// `ghash_gfmul`: computes a block multiplication in the GF(2^128).
pub fn ghash_gfmul(
    x: &[u8; GMAC_BLOCK_LEN],
    y: &[u8; GMAC_BLOCK_LEN],
    product: &mut [u8; GMAC_BLOCK_LEN],
) {
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

    for (b, w) in product.as_chunks_mut::<4>().0.iter_mut().zip(z) {
        *b = w.to_be_bytes();
    }
}

/// `ghash_update_mi`: the machine independent GHASH update: absorbs the whole blocks of `x`
/// (a trailing partial block is ignored).
pub fn ghash_update_mi(ctx: &mut GhashCtx, x: &[u8]) {
    let mut y = ctx.z;

    for blk in x.as_chunks::<GMAC_BLOCK_LEN>().0 {
        let mut s = [0u8; GMAC_BLOCK_LEN];
        for i in 0..GMAC_BLOCK_LEN {
            s[i] = y[i] ^ blk[i];
        }
        ghash_gfmul(&s, &ctx.h, &mut ctx.s);
        y = ctx.s;
    }
    ctx.z = ctx.s;
}

/// `ghash_update`: the GHASH update in use (see the deviations: always `ghash_update_mi`).
pub fn ghash_update(ctx: &mut GhashCtx, x: &[u8]) {
    ghash_update_mi(ctx, x);
}

/// `AES_GMAC_Init`.
#[allow(non_snake_case)] // the C name
pub fn AES_GMAC_Init(ctx: &mut AesGmacCtx) {
    ctx.ghash.h = [0; GMAC_BLOCK_LEN];
    ctx.ghash.s = [0; GMAC_BLOCK_LEN];
    ctx.ghash.z = [0; GMAC_BLOCK_LEN];
    ctx.j = [0; GMAC_BLOCK_LEN];
}

/// `AES_GMAC_Setkey`: the AES key (16, 24 or 32 bytes) followed by the 4-byte salt.
#[allow(non_snake_case)] // the C name
pub fn AES_GMAC_Setkey(ctx: &mut AesGmacCtx, key: &[u8]) -> Result<(), Errno> {
    let klen = key.len();
    if klen < AESCTR_NONCESIZE {
        return Err(Errno::EINVAL);
    }
    ctx.k = AesCtx::new(&key[..klen - AESCTR_NONCESIZE])?;

    // copy out salt to the counter block
    ctx.j[..AESCTR_NONCESIZE].copy_from_slice(&key[klen - AESCTR_NONCESIZE..]);

    // prepare a hash subkey
    ctx.ghash.h = ctx.k.encrypt(&ctx.ghash.h);
    Ok(())
}

/// `AES_GMAC_Reinit`: starts a message under the 8-byte IV.
#[allow(non_snake_case)] // the C name
pub fn AES_GMAC_Reinit(ctx: &mut AesGmacCtx, iv: &[u8]) {
    // copy out IV to the counter block
    ctx.j[AESCTR_NONCESIZE..AESCTR_NONCESIZE + iv.len()].copy_from_slice(iv);
}

/// `AES_GMAC_Update`: authenticates `data`, zero-padding a last partial block.
#[allow(non_snake_case)] // the C name
pub fn AES_GMAC_Update(ctx: &mut AesGmacCtx, data: &[u8]) -> Result<(), Errno> {
    let len = data.len();
    let mut blk = [0u8; GMAC_BLOCK_LEN];

    if len > 0 {
        let plen = len % GMAC_BLOCK_LEN;
        if len >= GMAC_BLOCK_LEN {
            ghash_update(&mut ctx.ghash, &data[..len - plen]);
        }
        if plen != 0 {
            blk[..plen].copy_from_slice(&data[len - plen..]);
            ghash_update(&mut ctx.ghash, &blk);
        }
    }
    Ok(())
}

/// `AES_GMAC_Final`: the 16-byte tag: the hash xor the encryption of the counter block 1.
#[allow(non_snake_case)] // the C name
pub fn AES_GMAC_Final(digest: &mut [u8; GMAC_DIGEST_LEN], ctx: &mut AesGmacCtx) {
    // do one round of GCTR
    ctx.j[GMAC_BLOCK_LEN - 1] = 1;
    let mut keystream = ctx.k.encrypt(&ctx.j);
    for i in 0..GMAC_DIGEST_LEN {
        digest[i] = ctx.ghash.s[i] ^ keystream[i];
    }
    explicit_bzero(&mut keystream);
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

        AES_GMAC_Init(&mut ctx);
        assert_eq!(AES_GMAC_Setkey(&mut ctx, &material), Ok(()));
        AES_GMAC_Reinit(&mut ctx, &iv[4..]);
        assert_eq!(AES_GMAC_Update(&mut ctx, aad), Ok(()));
        assert_eq!(AES_GMAC_Update(&mut ctx, ct), Ok(()));

        // The length block: the bit lengths, big-endian, in the low word of each half.
        let mut blk = [0u8; GMAC_BLOCK_LEN];
        blk[4..8].copy_from_slice(&((aad.len() * 8) as u32).to_be_bytes());
        blk[12..16].copy_from_slice(&((ct.len() * 8) as u32).to_be_bytes());
        assert_eq!(AES_GMAC_Update(&mut ctx, &blk), Ok(()));

        let mut out = [0u8; GMAC_DIGEST_LEN];
        AES_GMAC_Final(&mut out, &mut ctx);
        out
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
        let mut out = [0u8; 16];
        ghash_gfmul(&x, &one, &mut out);
        assert_eq!(out, x);
        ghash_gfmul(&one, &x, &mut out);
        assert_eq!(out, x);
        // And 0 * x is 0; the product commutes.
        ghash_gfmul(&[0; 16], &x, &mut out);
        assert_eq!(out, [0; 16]);
        let y: [u8; 16] = hexn("66e94bd4ef8a2c3b884cfa59ca342b2e");
        let (mut xy, mut yx) = ([0u8; 16], [0u8; 16]);
        ghash_gfmul(&x, &y, &mut xy);
        ghash_gfmul(&y, &x, &mut yx);
        assert_eq!(xy, yx);
        // The test case 2 hash: H = E(K, 0) = 66e94bd4ef8a2c3b884cfa59ca342b2e and the ciphertext
        // block c: GHASH(c || len) = c*H^2 + len*H; the first step c * H is
        // 5e2ec746917062882c85b0685353deb7 (the intermediate value of the spec's table).
        ghash_gfmul(&hexn("0388dace60b6a392f328c2b971b2fe78"), &y, &mut out);
        assert_eq!(out.to_vec(), hex("5e2ec746917062882c85b0685353deb7"));
    }

    #[test]
    fn bad_key_sizes_are_rejected() {
        let mut ctx = AesGmacCtx::default();
        AES_GMAC_Init(&mut ctx);
        // 4 bytes of salt after an AES key of 16, 24 or 32: the other lengths are errors.
        for len in [0usize, 3, 4, 19, 21, 27, 31, 37, 40] {
            assert_eq!(
                AES_GMAC_Setkey(&mut ctx, &std::vec![0u8; len]),
                Err(Errno::EINVAL),
                "{len}"
            );
        }
        assert_eq!(AES_GMAC_Setkey(&mut ctx, &[0u8; 20]), Ok(()));
    }
}
/* </TESTS> */
