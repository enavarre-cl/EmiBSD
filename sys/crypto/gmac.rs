/*	$OpenBSD: gmac.h,v 1.6 2017/05/02 11:44:32 mikeb Exp $	*/
/*	$OpenBSD: gmac.c,v 1.10 2017/05/02 11:44:32 mikeb Exp $	*/
/* <LICENSES> */
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

use super::aes::{AES_Encrypt, AES_Setkey, AesCtx};
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
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
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
    AES_Setkey(&mut ctx.k, &key[..klen - AESCTR_NONCESIZE])?;

    // copy out salt to the counter block
    ctx.j[..AESCTR_NONCESIZE].copy_from_slice(&key[klen - AESCTR_NONCESIZE..]);

    // prepare a hash subkey
    let zero = ctx.ghash.h;
    AES_Encrypt(&ctx.k, &zero, &mut ctx.ghash.h);
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
    let mut keystream = [0u8; GMAC_BLOCK_LEN];

    // do one round of GCTR
    ctx.j[GMAC_BLOCK_LEN - 1] = 1;
    AES_Encrypt(&ctx.k, &ctx.j, &mut keystream);
    for i in 0..GMAC_DIGEST_LEN {
        digest[i] = ctx.ghash.s[i] ^ keystream[i];
    }
    explicit_bzero(&mut keystream);
}

#[cfg(test)]
mod tests;
