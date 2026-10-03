/*	$OpenBSD: hmac.h,v 1.3 2012/12/05 23:20:15 deraadt Exp $	*/
/*	$OpenBSD: hmac.c,v 1.4 2016/09/19 18:09:40 tedu Exp $	*/
/* <LICENSES> */
/*-
 * Copyright (c) 2008 Damien Bergamini <damien.bergamini@free.fr>
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

//! HMAC (RFC 2104) over MD5, SHA-1 and SHA-256, with the hash's state kept in the context
//! so that a message can be fed in pieces. (IPsec's `HMAC-*-96` does not use this file: the
//! software crypto driver precomputes the inner and outer states itself, see `cryptosoft.c`.)
//!
//! Upstream: sys/crypto/hmac.h @ 3ce1f3f79392, sys/crypto/hmac.c @ 3ce1f3f79392
//!
//! ## Deviations
//! - The header and the file share this module; the contexts are [`HmacMd5Ctx`],
//!   [`HmacSha1Ctx`] and [`HmacSha256Ctx`].
//! - The three families are one `macro_rules!` expansion each, so the C names stay
//!   `grep`-able while the three bodies, identical but for the hash, are written once. The
//!   keys and data are slices (`key_len`/`len` in C); `key_len` stays a context field, as the
//!   C's `Final` needs it.
//! - The scratch pads are wiped with `explicit_bzero`.

use libkern::explicit_bzero;

use super::md5::{MD5_BLOCK_LENGTH, MD5_DIGEST_LENGTH, MD5Final, MD5Init, MD5Update, Md5Ctx};
use super::sha1::{
    SHA1_BLOCK_LENGTH, SHA1_DIGEST_LENGTH, SHA1Final, SHA1Init, SHA1Update, Sha1Ctx,
};
use super::sha2::{
    SHA256_BLOCK_LENGTH, SHA256_DIGEST_LENGTH, SHA256Final, SHA256Init, SHA256Update, Sha2Ctx,
};

/// Defines one HMAC family: the context type and the `Init`, `Update` and `Final` functions
/// over a hash given by its context type, block and digest lengths and its three functions.
macro_rules! hmac_family {
    (
        $ctx_ty:ident, $hash_ctx:ty, $block:ident, $digest:ident,
        $hash_init:ident, $hash_update:ident, $hash_final:ident,
        $init:ident, $update:ident, $final:ident, $name:literal
    ) => {
        #[doc = concat!("`", $name, "`: an HMAC in progress.")]
        #[derive(Clone, Copy, Debug, PartialEq, Eq)]
        pub struct $ctx_ty {
            /// `ctx`: the inner hash.
            pub ctx: $hash_ctx,
            /// `key`: the key, hashed first if it was longer than a block.
            pub key: [u8; $block],
            /// `key_len`: bytes of `key` in use.
            pub key_len: u32,
        }

        impl Default for $ctx_ty {
            fn default() -> Self {
                Self {
                    ctx: <$hash_ctx>::default(),
                    key: [0; $block],
                    key_len: 0,
                }
            }
        }

        #[doc = concat!("`", stringify!($init), "`: starts an HMAC under `key`.")]
        #[allow(non_snake_case)] // the C name
        pub fn $init(ctx: &mut $ctx_ty, key: &[u8]) {
            let mut k_ipad = [0u8; $block];

            if key.len() > $block {
                $hash_init(&mut ctx.ctx);
                $hash_update(&mut ctx.ctx, key);
                let mut digest = [0u8; $digest];
                $hash_final(&mut digest, &mut ctx.ctx);
                ctx.key[..$digest].copy_from_slice(&digest);
                ctx.key_len = $digest as u32;
            } else {
                ctx.key[..key.len()].copy_from_slice(key);
                ctx.key_len = key.len() as u32;
            }

            k_ipad[..ctx.key_len as usize].copy_from_slice(&ctx.key[..ctx.key_len as usize]);
            for b in k_ipad.iter_mut() {
                *b ^= 0x36;
            }

            $hash_init(&mut ctx.ctx);
            $hash_update(&mut ctx.ctx, &k_ipad);

            explicit_bzero(&mut k_ipad);
        }

        #[doc = concat!("`", stringify!($update), "`: adds `data` to the message.")]
        #[allow(non_snake_case)] // the C name
        pub fn $update(ctx: &mut $ctx_ty, data: &[u8]) {
            $hash_update(&mut ctx.ctx, data);
        }

        #[doc = concat!("`", stringify!($final), "`: the MAC of the message.")]
        #[allow(non_snake_case)] // the C name
        pub fn $final(digest: &mut [u8; $digest], ctx: &mut $ctx_ty) {
            let mut k_opad = [0u8; $block];

            $hash_final(digest, &mut ctx.ctx);

            k_opad[..ctx.key_len as usize].copy_from_slice(&ctx.key[..ctx.key_len as usize]);
            for b in k_opad.iter_mut() {
                *b ^= 0x5c;
            }

            $hash_init(&mut ctx.ctx);
            $hash_update(&mut ctx.ctx, &k_opad);
            let inner = *digest;
            $hash_update(&mut ctx.ctx, &inner);
            $hash_final(digest, &mut ctx.ctx);

            explicit_bzero(&mut k_opad);
        }
    };
}

hmac_family!(
    HmacMd5Ctx,
    Md5Ctx,
    MD5_BLOCK_LENGTH,
    MD5_DIGEST_LENGTH,
    MD5Init,
    MD5Update,
    MD5Final,
    HMAC_MD5_Init,
    HMAC_MD5_Update,
    HMAC_MD5_Final,
    "HMAC_MD5_CTX"
);

hmac_family!(
    HmacSha1Ctx,
    Sha1Ctx,
    SHA1_BLOCK_LENGTH,
    SHA1_DIGEST_LENGTH,
    SHA1Init,
    SHA1Update,
    SHA1Final,
    HMAC_SHA1_Init,
    HMAC_SHA1_Update,
    HMAC_SHA1_Final,
    "HMAC_SHA1_CTX"
);

hmac_family!(
    HmacSha256Ctx,
    Sha2Ctx,
    SHA256_BLOCK_LENGTH,
    SHA256_DIGEST_LENGTH,
    SHA256Init,
    SHA256Update,
    SHA256Final,
    HMAC_SHA256_Init,
    HMAC_SHA256_Update,
    HMAC_SHA256_Final,
    "HMAC_SHA256_CTX"
);

#[cfg(test)]
mod tests;
