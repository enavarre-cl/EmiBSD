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
 * The author of this code is Angelos D. Keromytis (angelos@cis.upenn.edu)
 *
 * This code was written by Angelos D. Keromytis in Athens, Greece, in
 * February 2000. Network Security Technologies Inc. (NSTI) kindly
 * supported the development of this code.
 *
 * Copyright (c) 2000 Angelos D. Keromytis
 *
 * Permission to use, copy, and modify this software with or without fee
 * is hereby granted, provided that this entire notice is included in
 * all source code copies of any software which is or includes a copy or
 * modification of this software.
 *
 * THIS SOFTWARE IS BEING PROVIDED "AS IS", WITHOUT ANY EXPRESS OR
 * IMPLIED WARRANTY. IN PARTICULAR, NONE OF THE AUTHORS MAKES ANY
 * REPRESENTATION OR WARRANTY OF ANY KIND CONCERNING THE
 * MERCHANTABILITY OF THIS SOFTWARE OR ITS FITNESS FOR ANY PARTICULAR
 * PURPOSE.
 */

/*
 * The authors of this code are John Ioannidis (ji@tla.org),
 * Angelos D. Keromytis (kermit@csd.uch.gr),
 * Niels Provos (provos@physnet.uni-hamburg.de),
 * Damien Miller (djm@mindrot.org) and
 * Mike Belopuhov (mikeb@openbsd.org).
 *
 * This code was written by John Ioannidis for BSD/OS in Athens, Greece,
 * in November 1995.
 *
 * Ported to OpenBSD and NetBSD, with additional transforms, in December 1996,
 * by Angelos D. Keromytis.
 *
 * Additional transforms and features in 1997 and 1998 by Angelos D. Keromytis
 * and Niels Provos.
 *
 * Additional features in 1999 by Angelos D. Keromytis.
 *
 * AES XTS implementation in 2008 by Damien Miller
 *
 * AES-GCM-16 and Chacha20-Poly1305 AEAD modes by Mike Belopuhov.
 *
 * Copyright (C) 1995, 1996, 1997, 1998, 1999 by John Ioannidis,
 * Angelos D. Keromytis and Niels Provos.
 *
 * Copyright (C) 2001, Angelos D. Keromytis.
 *
 * Copyright (C) 2008, Damien Miller
 *
 * Copyright (C) 2010, 2015, Mike Belopuhov
 *
 * Permission to use, copy, and modify this software with or without fee
 * is hereby granted, provided that this entire notice is included in
 * all copies of any software which is or includes a copy or
 * modification of this software.
 * You may use this code under the GNU public license if you so wish. Please
 * contribute changes back to the authors under this freer than GPL license
 * so that we may further the use of strong encryption without limitations to
 * all.
 *
 * THIS SOFTWARE IS BEING PROVIDED "AS IS", WITHOUT ANY EXPRESS OR
 * IMPLIED WARRANTY. IN PARTICULAR, NONE OF THE AUTHORS MAKES ANY
 * REPRESENTATION OR WARRANTY OF ANY KIND CONCERNING THE
 * MERCHANTABILITY OF THIS SOFTWARE OR ITS FITNESS FOR ANY PARTICULAR
 * PURPOSE.
 */
/* </LICENSES> */

/* <CODE> */
//! The transform tables of the software crypto driver: for each cipher an [`EncXform`] (block
//! and IV sizes, key limits, the encrypt/decrypt/setkey/reinit functions over a key schedule),
//! for each authenticator an [`AuthHash`] (sizes and the Init/Setkey/Reinit/Update/Final
//! functions over a context). `cryptosoft.c` drives them; `ip_esp.c`, `ip_ah.c` and
//! `xform_ipcomp.c` pick them by name.
//!
//! Upstream: sys/crypto/xform.h @ 3ce1f3f79392, sys/crypto/xform.c @ 3ce1f3f79392
//!
//! ## Deviations
//! - The header and the file share this module.
//! - The `void *` contexts: `union authctx` is [`AuthCtx`], an enum of the hash and MAC
//!   contexts (`Init` makes the variant, the other functions match it); the key schedule
//!   (`caddr_t key`, `ctxsize` bytes malloc'd by `swcr_newsession`) is [`Kschedule`], an enum of
//!   the ciphers' schedules, `setkey` making the variant. A function that finds the wrong
//!   variant panics: it would be a table wired to the wrong function. `ctxsize` stays in the
//!   tables as the C's `sizeof` (of the Rust types) and as `0` for a transform with no
//!   schedule.
//! - The function types take slices where the C takes a pointer and a length (`setkey` the key
//!   and `len` is `key.len()`; `Update` the data and a `u_int16_t` length, so the 16-bit
//!   truncation of a long run is gone) and the blocks are `&mut [u8]` of at least the block
//!   size. `setkey` returns `Result<(), Errno>` for the `int` (< 0 is failure), `Update`
//!   likewise for the `int` that is 0. `AuthHash.Final` writes `hashsize` bytes at the front of
//!   its slice.
//! - `des3_setkey` and `blf_setkey` reject a key shorter than the C reads (24 bytes, and 1
//!   byte), `AES_GMAC_Setkey` and the AES setkeys report an invalid AES key size.
//! - [`CompAlgo`]'s `compress` and `decompress` return the output as a `Vec<u8>` or an error
//!   where the C returns its length (0 for failure) and the buffer through `u_int8_t **`;
//!   `deflate_compress` and `deflate_decompress` call `xform_ipcomp.c`'s `deflate_global`.
//! - The names of the tables are the C's, in lower case (`enc_xform_aes`); the `int` returns of
//!   the `*Update_int` functions are `Result`s.

use alloc::boxed::Box;
use alloc::vec::Vec;

use super::aes::{AES_Decrypt, AES_Encrypt, AES_Setkey, AesCtx};
use super::blf::{BlfCtx, blf_ecb_decrypt, blf_ecb_encrypt, blf_key};
use super::cast::{CastKey, cast_decrypt, cast_encrypt, cast_setkey};
use super::chachapoly::{
    CHACHA20_KEYSIZE, CHACHA20_SALT, Chacha20_Poly1305_Final, Chacha20_Poly1305_Init,
    Chacha20_Poly1305_Reinit, Chacha20_Poly1305_Setkey, Chacha20_Poly1305_Update, Chacha20Ctx,
    Chacha20Poly1305Ctx, POLY1305_BLOCK_LEN, POLY1305_TAGLEN, chacha20_crypt, chacha20_reinit,
    chacha20_setkey,
};
use super::cryptodev::{
    CHACHA20_BLOCK_LEN, CRYPTO_3DES_CBC, CRYPTO_AES_128_GMAC, CRYPTO_AES_192_GMAC,
    CRYPTO_AES_256_GMAC, CRYPTO_AES_CBC, CRYPTO_AES_CTR, CRYPTO_AES_GCM_16, CRYPTO_AES_GMAC,
    CRYPTO_AES_XTS, CRYPTO_BLF_CBC, CRYPTO_CAST_CBC, CRYPTO_CHACHA20_POLY1305,
    CRYPTO_CHACHA20_POLY1305_MAC, CRYPTO_DEFLATE_COMP, CRYPTO_MD5_HMAC, CRYPTO_NULL,
    CRYPTO_RIPEMD160_HMAC, CRYPTO_SHA1_HMAC, CRYPTO_SHA2_256_HMAC, CRYPTO_SHA2_384_HMAC,
    CRYPTO_SHA2_512_HMAC, HMAC_MD5_BLOCK_LEN, HMAC_RIPEMD160_BLOCK_LEN, HMAC_SHA1_BLOCK_LEN,
    HMAC_SHA2_256_BLOCK_LEN, HMAC_SHA2_384_BLOCK_LEN, HMAC_SHA2_512_BLOCK_LEN,
};
use super::ecb3_enc::des_ecb3_encrypt;
use super::gmac::{
    AES_GMAC_Final, AES_GMAC_Init, AES_GMAC_Reinit, AES_GMAC_Setkey, AES_GMAC_Update, AesGmacCtx,
    GMAC_BLOCK_LEN, GMAC_DIGEST_LEN,
};
use super::md5::{MD5_DIGEST_LENGTH, Md5Ctx};
use super::rijndael::{RijndaelCtx, rijndael_decrypt, rijndael_encrypt, rijndael_set_key};
use super::rmd160::{RMD160_DIGEST_LENGTH, Rmd160Ctx};
use super::set_key::des_set_key;
use super::sha1::{SHA1_DIGEST_LENGTH, Sha1Ctx};
use super::sha2::{
    SHA256_DIGEST_LENGTH, SHA384_DIGEST_LENGTH, SHA512_DIGEST_LENGTH, Sha256Ctx, Sha384Ctx,
    Sha512Ctx,
};
use super::xform_ipcomp::deflate_global;
use crate::kern::subr_prf::panic;
use crate::sys::errno::Errno;

/// `AESCTR_NONCESIZE`.
pub const AESCTR_NONCESIZE: usize = 4;
/// `AESCTR_IVSIZE`.
pub const AESCTR_IVSIZE: usize = 8;
/// `AESCTR_BLOCKSIZE`.
pub const AESCTR_BLOCKSIZE: usize = 16;
/// `AES_XTS_BLOCKSIZE`.
pub const AES_XTS_BLOCKSIZE: usize = 16;
/// `AES_XTS_IVSIZE`.
pub const AES_XTS_IVSIZE: usize = 8;
/// `AES_XTS_ALPHA`: GF(2^128) generator polynomial.
pub const AES_XTS_ALPHA: u8 = 0x87;

/// `union authctx`: the context of a hash or MAC in progress, the variant set by the
/// transform's `Init`.
// The C union is as big as its largest member too; the contexts live in a `Box` in a session.
#[allow(clippy::large_enum_variant)]
#[derive(Clone, Copy, Debug, Default)]
pub enum AuthCtx {
    /// A context not initialised yet (the C's freshly allocated storage).
    #[default]
    None,
    /// `md5ctx`.
    Md5(Md5Ctx),
    /// `sha1ctx`.
    Sha1(Sha1Ctx),
    /// `rmd160ctx`.
    Rmd160(Rmd160Ctx),
    /// `sha2_ctx` of SHA-256.
    Sha256(Sha256Ctx),
    /// `sha2_ctx` of SHA-384.
    Sha384(Sha384Ctx),
    /// `sha2_ctx` of SHA-512.
    Sha512(Sha512Ctx),
    /// `aes_gmac_ctx`.
    AesGmac(AesGmacCtx),
    /// `CHACHA20_POLY1305_CTX`.
    Chacha20Poly1305(Chacha20Poly1305Ctx),
}

/// `struct aes_ctr_ctx`: the key schedule of AES-CTR and AES-GCM.
#[derive(Clone, Copy, Debug, Default)]
pub struct AesCtrCtx {
    /// `ac_key`.
    pub ac_key: AesCtx,
    /// `ac_block`: the counter block (nonce, IV, counter).
    pub ac_block: [u8; AESCTR_BLOCKSIZE],
}

/// `struct aes_xts_ctx`: the key schedule of AES-XTS.
#[derive(Clone, Copy, Debug, Default)]
pub struct AesXtsCtx {
    /// `key1`: the data key.
    pub key1: RijndaelCtx,
    /// `key2`: the tweak key.
    pub key2: RijndaelCtx,
    /// `tweak`.
    pub tweak: [u8; AES_XTS_BLOCKSIZE],
}

/// The key schedule of a cipher (`sw_kschedule`), the variant set by `setkey`: three DES key
/// schedules for `des3`, a Blowfish context, a CAST-128 key, an AES context, and the contexts
/// of the other modes.
// The C schedule is `ctxsize` bytes of the transform's own size; a session `Box`es this.
#[allow(clippy::large_enum_variant)]
#[derive(Clone, Debug, Default)]
pub enum Kschedule {
    /// Not set yet.
    #[default]
    None,
    /// 3DES: the three DES key schedules (384 bytes in the C).
    Des3([[u32; 32]; 3]),
    /// Blowfish (a 4 KiB context, boxed so that the enum stays small).
    Blf(Box<BlfCtx>),
    /// CAST-128.
    Cast(CastKey),
    /// AES-CBC.
    Aes(AesCtx),
    /// AES-CTR and AES-GCM.
    AesCtr(AesCtrCtx),
    /// AES-XTS.
    AesXts(AesXtsCtx),
    /// ChaCha20-Poly1305's cipher side.
    Chacha20(Chacha20Ctx),
}

/// `Setkey` of an authenticator.
pub type AuthSetkeyFn = fn(&mut AuthCtx, &[u8]) -> Result<(), Errno>;

/// `setkey` of a cipher.
pub type EncSetkeyFn = fn(&mut Kschedule, &[u8]) -> Result<(), Errno>;

/// `struct auth_hash`: an authenticator.
#[allow(non_snake_case)] // the C names of the function members
pub struct AuthHash {
    /// `type`: the `CRYPTO_*` algorithm.
    pub type_: i32,
    /// `name`.
    pub name: &'static str,
    /// `keysize`.
    pub keysize: u16,
    /// `hashsize`.
    pub hashsize: u16,
    /// `authsize`: the bytes of the MAC that go on the wire.
    pub authsize: u16,
    /// `ctxsize`.
    pub ctxsize: u16,
    /// `blocksize`.
    pub blocksize: u16,
    /// `Init`.
    pub Init: fn(&mut AuthCtx),
    /// `Setkey`.
    pub Setkey: Option<AuthSetkeyFn>,
    /// `Reinit`.
    pub Reinit: Option<fn(&mut AuthCtx, &[u8])>,
    /// `Update`.
    pub Update: fn(&mut AuthCtx, &[u8]) -> Result<(), Errno>,
    /// `Final`.
    pub Final: fn(&mut [u8], &mut AuthCtx),
}

/// `struct enc_xform`: a cipher.
pub struct EncXform {
    /// `type`: the `CRYPTO_*` algorithm.
    pub type_: i32,
    /// `name`.
    pub name: &'static str,
    /// `blocksize`.
    pub blocksize: u16,
    /// `ivsize`.
    pub ivsize: u16,
    /// `minkey`.
    pub minkey: u16,
    /// `maxkey`.
    pub maxkey: u16,
    /// `ctxsize`: 0 when the transform keeps no schedule.
    pub ctxsize: u16,
    /// `encrypt`.
    pub encrypt: Option<fn(&mut Kschedule, &mut [u8])>,
    /// `decrypt`.
    pub decrypt: Option<fn(&mut Kschedule, &mut [u8])>,
    /// `setkey`.
    pub setkey: Option<EncSetkeyFn>,
    /// `reinit`.
    pub reinit: Option<fn(&mut Kschedule, &[u8])>,
}

/// `struct comp_algo`: a compression algorithm.
pub struct CompAlgo {
    /// `type`: the `CRYPTO_*` algorithm.
    pub type_: i32,
    /// `name`.
    pub name: &'static str,
    /// `minlen`.
    pub minlen: usize,
    /// `compress`: the compressed form of the data, or why not.
    pub compress: fn(&[u8]) -> Result<Vec<u8>, Errno>,
    /// `decompress`.
    pub decompress: fn(&[u8]) -> Result<Vec<u8>, Errno>,
}

/// A context of the wrong kind: a table entry wired to another transform's functions.
fn bad_ctx(who: &str) -> ! {
    panic(format_args!("xform: {}: wrong kind of context", who))
}

/// The first `N` bytes of `digest` as an array: the output of a `Final` is `hashsize` bytes.
fn digest_out<const N: usize>(digest: &mut [u8]) -> &mut [u8; N] {
    match digest.first_chunk_mut::<N>() {
        Some(d) => d,
        None => panic(format_args!("xform: digest buffer shorter than {}", N)),
    }
}

impl Kschedule {
    fn des3(&mut self) -> &mut [[u32; 32]; 3] {
        match self {
            Kschedule::Des3(c) => c,
            _ => bad_ctx("3des"),
        }
    }

    fn blf(&mut self) -> &mut BlfCtx {
        match self {
            Kschedule::Blf(c) => c,
            _ => bad_ctx("blowfish"),
        }
    }

    fn cast(&mut self) -> &mut CastKey {
        match self {
            Kschedule::Cast(c) => c,
            _ => bad_ctx("cast"),
        }
    }

    fn aes(&mut self) -> &mut AesCtx {
        match self {
            Kschedule::Aes(c) => c,
            _ => bad_ctx("aes"),
        }
    }

    fn aes_ctr(&mut self) -> &mut AesCtrCtx {
        match self {
            Kschedule::AesCtr(c) => c,
            _ => bad_ctx("aes-ctr"),
        }
    }

    fn aes_xts(&mut self) -> &mut AesXtsCtx {
        match self {
            Kschedule::AesXts(c) => c,
            _ => bad_ctx("aes-xts"),
        }
    }

    fn chacha20(&mut self) -> &mut Chacha20Ctx {
        match self {
            Kschedule::Chacha20(c) => c,
            _ => bad_ctx("chacha20"),
        }
    }
}

// Encryption wrapper routines.

/// `des3_encrypt`.
fn des3_encrypt(key: &mut Kschedule, blk: &mut [u8]) {
    let ks = key.des3();
    let mut input = [0u8; 8];
    let mut out = [0u8; 8];
    input.copy_from_slice(&blk[..8]);
    des_ecb3_encrypt(&input, &mut out, &ks[0], &ks[1], &ks[2], true);
    blk[..8].copy_from_slice(&out);
}

/// `des3_decrypt`: the schedules in the opposite order.
fn des3_decrypt(key: &mut Kschedule, blk: &mut [u8]) {
    let ks = key.des3();
    let mut input = [0u8; 8];
    let mut out = [0u8; 8];
    input.copy_from_slice(&blk[..8]);
    des_ecb3_encrypt(&input, &mut out, &ks[2], &ks[1], &ks[0], false);
    blk[..8].copy_from_slice(&out);
}

/// `des3_setkey`: three DES keys of 8 bytes.
fn des3_setkey(sched: &mut Kschedule, key: &[u8]) -> Result<(), Errno> {
    if key.len() < 24 {
        return Err(Errno::EINVAL);
    }
    let mut ks = [[0u32; 32]; 3];
    for (i, s) in ks.iter_mut().enumerate() {
        let mut k = [0u8; 8];
        k.copy_from_slice(&key[8 * i..8 * i + 8]);
        des_set_key(&k, s)?;
    }
    *sched = Kschedule::Des3(ks);
    Ok(())
}

/// `blf_encrypt`.
fn blf_encrypt(key: &mut Kschedule, blk: &mut [u8]) {
    blf_ecb_encrypt(key.blf(), &mut blk[..8]);
}

/// `blf_decrypt`.
fn blf_decrypt(key: &mut Kschedule, blk: &mut [u8]) {
    blf_ecb_decrypt(key.blf(), &mut blk[..8]);
}

/// `blf_setkey`.
fn blf_setkey(sched: &mut Kschedule, key: &[u8]) -> Result<(), Errno> {
    if key.is_empty() {
        return Err(Errno::EINVAL);
    }
    let mut ctx = Box::<BlfCtx>::default();
    blf_key(&mut ctx, key);
    *sched = Kschedule::Blf(ctx);
    Ok(())
}

/// `null_setkey`.
fn null_setkey(_sched: &mut Kschedule, _key: &[u8]) -> Result<(), Errno> {
    Ok(())
}

/// `null_encrypt`.
fn null_encrypt(_key: &mut Kschedule, _blk: &mut [u8]) {}

/// `null_decrypt`.
fn null_decrypt(_key: &mut Kschedule, _blk: &mut [u8]) {}

/// `cast5_encrypt`.
fn cast5_encrypt(key: &mut Kschedule, blk: &mut [u8]) {
    let mut input = [0u8; 8];
    let mut out = [0u8; 8];
    input.copy_from_slice(&blk[..8]);
    cast_encrypt(key.cast(), &input, &mut out);
    blk[..8].copy_from_slice(&out);
}

/// `cast5_decrypt`.
fn cast5_decrypt(key: &mut Kschedule, blk: &mut [u8]) {
    let mut input = [0u8; 8];
    let mut out = [0u8; 8];
    input.copy_from_slice(&blk[..8]);
    cast_decrypt(key.cast(), &input, &mut out);
    blk[..8].copy_from_slice(&out);
}

/// `cast5_setkey`.
fn cast5_setkey(sched: &mut Kschedule, key: &[u8]) -> Result<(), Errno> {
    let mut ck = CastKey::default();
    cast_setkey(&mut ck, key);
    *sched = Kschedule::Cast(ck);
    Ok(())
}

/// `aes_encrypt`.
fn aes_encrypt(key: &mut Kschedule, blk: &mut [u8]) {
    let mut input = [0u8; 16];
    let mut out = [0u8; 16];
    input.copy_from_slice(&blk[..16]);
    AES_Encrypt(key.aes(), &input, &mut out);
    blk[..16].copy_from_slice(&out);
}

/// `aes_decrypt`.
fn aes_decrypt(key: &mut Kschedule, blk: &mut [u8]) {
    let mut input = [0u8; 16];
    let mut out = [0u8; 16];
    input.copy_from_slice(&blk[..16]);
    AES_Decrypt(key.aes(), &input, &mut out);
    blk[..16].copy_from_slice(&out);
}

/// `aes_setkey`.
fn aes_setkey(sched: &mut Kschedule, key: &[u8]) -> Result<(), Errno> {
    let mut ctx = AesCtx::default();
    AES_Setkey(&mut ctx, key)?;
    *sched = Kschedule::Aes(ctx);
    Ok(())
}

/// `aes_ctr_reinit`.
fn aes_ctr_reinit(key: &mut Kschedule, iv: &[u8]) {
    let ctx = key.aes_ctr();
    ctx.ac_block[AESCTR_NONCESIZE..AESCTR_NONCESIZE + AESCTR_IVSIZE]
        .copy_from_slice(&iv[..AESCTR_IVSIZE]);

    // reset counter
    ctx.ac_block[AESCTR_NONCESIZE + AESCTR_IVSIZE..].fill(0);
}

/// `aes_gcm_reinit`.
fn aes_gcm_reinit(key: &mut Kschedule, iv: &[u8]) {
    let ctx = key.aes_ctr();
    ctx.ac_block[AESCTR_NONCESIZE..AESCTR_NONCESIZE + AESCTR_IVSIZE]
        .copy_from_slice(&iv[..AESCTR_IVSIZE]);

    // reset counter
    ctx.ac_block[AESCTR_NONCESIZE + AESCTR_IVSIZE..].fill(0);
    ctx.ac_block[AESCTR_BLOCKSIZE - 1] = 1; // GCM starts with 1
}

/// `aes_ctr_crypt`: increments the counter and XORs the keystream block into `data`.
fn aes_ctr_crypt(key: &mut Kschedule, data: &mut [u8]) {
    let ctx = key.aes_ctr();
    let mut keystream = [0u8; AESCTR_BLOCKSIZE];

    // increment counter
    for i in (AESCTR_NONCESIZE + AESCTR_IVSIZE..AESCTR_BLOCKSIZE).rev() {
        ctx.ac_block[i] = ctx.ac_block[i].wrapping_add(1);
        if ctx.ac_block[i] != 0 {
            // continue on overflow
            break;
        }
    }
    let block = ctx.ac_block;
    AES_Encrypt(&ctx.ac_key, &block, &mut keystream);
    for i in 0..AESCTR_BLOCKSIZE {
        data[i] ^= keystream[i];
    }
    libkern::explicit_bzero(&mut keystream);
}

/// `aes_ctr_setkey`: the AES key followed by the 4-byte nonce.
fn aes_ctr_setkey(sched: &mut Kschedule, key: &[u8]) -> Result<(), Errno> {
    let len = key.len();
    if len < AESCTR_NONCESIZE {
        return Err(Errno::EINVAL);
    }

    let mut ctx = AesCtrCtx::default();
    AES_Setkey(&mut ctx.ac_key, &key[..len - AESCTR_NONCESIZE])?;
    ctx.ac_block[..AESCTR_NONCESIZE].copy_from_slice(&key[len - AESCTR_NONCESIZE..]);
    *sched = Kschedule::AesCtr(ctx);
    Ok(())
}

/// `aes_xts_reinit`.
fn aes_xts_reinit(key: &mut Kschedule, iv: &[u8]) {
    let ctx = key.aes_xts();

    // Prepare tweak as E_k2(IV). IV is specified as LE representation of a 64-bit block
    // number which we allow to be passed in directly.
    let mut b = [0u8; 8];
    b.copy_from_slice(&iv[..AES_XTS_IVSIZE]);
    let mut blocknum = u64::from_ne_bytes(b);
    for i in 0..AES_XTS_IVSIZE {
        ctx.tweak[i] = (blocknum & 0xff) as u8;
        blocknum >>= 8;
    }
    // Last 64 bits of IV are always zero
    ctx.tweak[AES_XTS_IVSIZE..].fill(0);

    let tweak = ctx.tweak;
    rijndael_encrypt(&ctx.key2, &tweak, &mut ctx.tweak);
}

/// `aes_xts_crypt`.
fn aes_xts_crypt(ctx: &mut AesXtsCtx, data: &mut [u8], do_encrypt: bool) {
    let mut block = [0u8; AES_XTS_BLOCKSIZE];
    let mut out = [0u8; AES_XTS_BLOCKSIZE];

    for i in 0..AES_XTS_BLOCKSIZE {
        block[i] = data[i] ^ ctx.tweak[i];
    }

    if do_encrypt {
        rijndael_encrypt(&ctx.key1, &block, &mut out);
    } else {
        rijndael_decrypt(&ctx.key1, &block, &mut out);
    }

    for i in 0..AES_XTS_BLOCKSIZE {
        data[i] = out[i] ^ ctx.tweak[i];
    }

    // Exponentiate tweak
    let mut carry_in = 0u8;
    for i in 0..AES_XTS_BLOCKSIZE {
        let carry_out = ctx.tweak[i] & 0x80;
        ctx.tweak[i] = (ctx.tweak[i] << 1) | carry_in;
        carry_in = carry_out >> 7;
    }
    if carry_in != 0 {
        ctx.tweak[0] ^= AES_XTS_ALPHA;
    }
    libkern::explicit_bzero(&mut block);
    libkern::explicit_bzero(&mut out);
}

/// `aes_xts_encrypt`.
fn aes_xts_encrypt(key: &mut Kschedule, data: &mut [u8]) {
    aes_xts_crypt(key.aes_xts(), data, true);
}

/// `aes_xts_decrypt`.
fn aes_xts_decrypt(key: &mut Kschedule, data: &mut [u8]) {
    aes_xts_crypt(key.aes_xts(), data, false);
}

/// `aes_xts_setkey`: two AES keys of 16 or 32 bytes.
fn aes_xts_setkey(sched: &mut Kschedule, key: &[u8]) -> Result<(), Errno> {
    let len = key.len();
    if len != 32 && len != 64 {
        return Err(Errno::EINVAL);
    }

    let mut ctx = AesXtsCtx::default();
    rijndael_set_key(&mut ctx.key1, key, len as i32 * 4)?;
    rijndael_set_key(&mut ctx.key2, &key[len / 2..], len as i32 * 4)?;
    *sched = Kschedule::AesXts(ctx);
    Ok(())
}

/// `chacha20_poly1305`'s cipher side: `chacha20_setkey` over the schedule enum.
fn chacha20_setkey_ks(sched: &mut Kschedule, key: &[u8]) -> Result<(), Errno> {
    let mut ctx = Chacha20Ctx::default();
    chacha20_setkey(&mut ctx, key, key.len() as i32)?;
    *sched = Kschedule::Chacha20(ctx);
    Ok(())
}

/// `chacha20_reinit` over the schedule enum.
fn chacha20_reinit_ks(key: &mut Kschedule, iv: &[u8]) {
    chacha20_reinit(key.chacha20(), iv);
}

/// `chacha20_crypt` over the schedule enum.
fn chacha20_crypt_ks(key: &mut Kschedule, data: &mut [u8]) {
    chacha20_crypt(key.chacha20(), data);
}

// And now for auth.

/// The three functions of a hash that go in an `auth_hash`: `Init` makes the context variant,
/// the `*Update_int` of the C (the `void *` adaptor) and the `Final` over `AuthCtx`.
macro_rules! hash_wrappers {
    (
        $variant:ident, $ctx:ty, $dlen:expr,
        $init_fn:ident, $update_fn:ident, $final_fn:ident, $who:literal
    ) => {
        fn $init_fn(c: &mut AuthCtx) {
            *c = AuthCtx::$variant(<$ctx>::new());
        }

        #[allow(non_snake_case)] // the C name
        fn $update_fn(c: &mut AuthCtx, buf: &[u8]) -> Result<(), Errno> {
            match c {
                AuthCtx::$variant(x) => {
                    x.update(buf);
                    Ok(())
                }
                _ => bad_ctx($who),
            }
        }

        fn $final_fn(digest: &mut [u8], c: &mut AuthCtx) {
            match c {
                AuthCtx::$variant(x) => *digest_out::<$dlen>(digest) = x.finalize(),
                _ => bad_ctx($who),
            }
        }
    };
}

hash_wrappers!(
    Md5,
    Md5Ctx,
    MD5_DIGEST_LENGTH,
    md5_init,
    MD5Update_int,
    md5_final,
    "md5"
);

hash_wrappers!(
    Sha1,
    Sha1Ctx,
    SHA1_DIGEST_LENGTH,
    sha1_init,
    SHA1Update_int,
    sha1_final,
    "sha1"
);

/// `RMD160Update_int`'s family.
fn rmd160_init(c: &mut AuthCtx) {
    *c = AuthCtx::Rmd160(Rmd160Ctx::new());
}

/// `RMD160Update_int`.
#[allow(non_snake_case)] // the C name
fn RMD160Update_int(c: &mut AuthCtx, buf: &[u8]) -> Result<(), Errno> {
    match c {
        AuthCtx::Rmd160(x) => {
            x.update(buf);
            Ok(())
        }
        _ => bad_ctx("rmd160"),
    }
}

fn rmd160_final(digest: &mut [u8], c: &mut AuthCtx) {
    match c {
        AuthCtx::Rmd160(x) => *digest_out::<RMD160_DIGEST_LENGTH>(digest) = x.finalize(),
        _ => bad_ctx("rmd160"),
    }
}

hash_wrappers!(
    Sha256,
    Sha256Ctx,
    SHA256_DIGEST_LENGTH,
    sha256_init,
    SHA256Update_int,
    sha256_final,
    "sha256"
);

hash_wrappers!(
    Sha384,
    Sha384Ctx,
    SHA384_DIGEST_LENGTH,
    sha384_init,
    SHA384Update_int,
    sha384_final,
    "sha384"
);

hash_wrappers!(
    Sha512,
    Sha512Ctx,
    SHA512_DIGEST_LENGTH,
    sha512_init,
    SHA512Update_int,
    sha512_final,
    "sha512"
);

/// `AES_GMAC_Init` over the context enum.
fn gmac_init(c: &mut AuthCtx) {
    let mut x = AesGmacCtx::default();
    AES_GMAC_Init(&mut x);
    *c = AuthCtx::AesGmac(x);
}

/// `AES_GMAC_Setkey` over the context enum.
fn gmac_setkey(c: &mut AuthCtx, key: &[u8]) -> Result<(), Errno> {
    match c {
        AuthCtx::AesGmac(x) => AES_GMAC_Setkey(x, key),
        _ => bad_ctx("gmac"),
    }
}

/// `AES_GMAC_Reinit` over the context enum.
fn gmac_reinit(c: &mut AuthCtx, iv: &[u8]) {
    match c {
        AuthCtx::AesGmac(x) => AES_GMAC_Reinit(x, iv),
        _ => bad_ctx("gmac"),
    }
}

/// `AES_GMAC_Update` over the context enum.
fn gmac_update(c: &mut AuthCtx, data: &[u8]) -> Result<(), Errno> {
    match c {
        AuthCtx::AesGmac(x) => AES_GMAC_Update(x, data),
        _ => bad_ctx("gmac"),
    }
}

/// `AES_GMAC_Final` over the context enum.
fn gmac_final(digest: &mut [u8], c: &mut AuthCtx) {
    match c {
        AuthCtx::AesGmac(x) => AES_GMAC_Final(digest_out::<GMAC_DIGEST_LEN>(digest), x),
        _ => bad_ctx("gmac"),
    }
}

/// `Chacha20_Poly1305_Init` over the context enum.
fn chachapoly_init(c: &mut AuthCtx) {
    let mut x = Chacha20Poly1305Ctx::default();
    Chacha20_Poly1305_Init(&mut x);
    *c = AuthCtx::Chacha20Poly1305(x);
}

/// `Chacha20_Poly1305_Setkey` over the context enum.
fn chachapoly_setkey(c: &mut AuthCtx, key: &[u8]) -> Result<(), Errno> {
    if key.len() < CHACHA20_KEYSIZE + CHACHA20_SALT {
        return Err(Errno::EINVAL);
    }
    match c {
        AuthCtx::Chacha20Poly1305(x) => {
            Chacha20_Poly1305_Setkey(x, key);
            Ok(())
        }
        _ => bad_ctx("chacha20-poly1305"),
    }
}

/// `Chacha20_Poly1305_Reinit` over the context enum.
fn chachapoly_reinit(c: &mut AuthCtx, iv: &[u8]) {
    match c {
        AuthCtx::Chacha20Poly1305(x) => Chacha20_Poly1305_Reinit(x, iv),
        _ => bad_ctx("chacha20-poly1305"),
    }
}

/// `Chacha20_Poly1305_Update` over the context enum.
fn chachapoly_update(c: &mut AuthCtx, data: &[u8]) -> Result<(), Errno> {
    match c {
        AuthCtx::Chacha20Poly1305(x) => Chacha20_Poly1305_Update(x, data),
        _ => bad_ctx("chacha20-poly1305"),
    }
}

/// `Chacha20_Poly1305_Final` over the context enum.
fn chachapoly_final(digest: &mut [u8], c: &mut AuthCtx) {
    match c {
        AuthCtx::Chacha20Poly1305(x) => {
            Chacha20_Poly1305_Final(digest_out::<POLY1305_TAGLEN>(digest), x)
        }
        _ => bad_ctx("chacha20-poly1305"),
    }
}

/// `deflate_compress`: compression through `xform_ipcomp.c`'s `deflate_global`.
fn deflate_compress(data: &[u8]) -> Result<Vec<u8>, Errno> {
    deflate_global(data, false)
}

/// `deflate_decompress`: decompression through `deflate_global`.
fn deflate_decompress(data: &[u8]) -> Result<Vec<u8>, Errno> {
    deflate_global(data, true)
}

// Encryption instances

/// `enc_xform_3des`.
#[allow(non_upper_case_globals)] // the C name
pub static enc_xform_3des: EncXform = EncXform {
    type_: CRYPTO_3DES_CBC,
    name: "3DES",
    blocksize: 8,
    ivsize: 8,
    minkey: 24,
    maxkey: 24,
    ctxsize: 384,
    encrypt: Some(des3_encrypt),
    decrypt: Some(des3_decrypt),
    setkey: Some(des3_setkey),
    reinit: None,
};

/// `enc_xform_blf`.
#[allow(non_upper_case_globals)] // the C name
pub static enc_xform_blf: EncXform = EncXform {
    type_: CRYPTO_BLF_CBC,
    name: "Blowfish",
    blocksize: 8,
    ivsize: 8,
    minkey: 5,
    maxkey: 56, // 448 bits, max key
    ctxsize: size_of::<BlfCtx>() as u16,
    encrypt: Some(blf_encrypt),
    decrypt: Some(blf_decrypt),
    setkey: Some(blf_setkey),
    reinit: None,
};

/// `enc_xform_cast5`.
#[allow(non_upper_case_globals)] // the C name
pub static enc_xform_cast5: EncXform = EncXform {
    type_: CRYPTO_CAST_CBC,
    name: "CAST-128",
    blocksize: 8,
    ivsize: 8,
    minkey: 5,
    maxkey: 16,
    ctxsize: size_of::<CastKey>() as u16,
    encrypt: Some(cast5_encrypt),
    decrypt: Some(cast5_decrypt),
    setkey: Some(cast5_setkey),
    reinit: None,
};

/// `enc_xform_aes`.
#[allow(non_upper_case_globals)] // the C name
pub static enc_xform_aes: EncXform = EncXform {
    type_: CRYPTO_AES_CBC,
    name: "AES",
    blocksize: 16,
    ivsize: 16,
    minkey: 16,
    maxkey: 32,
    ctxsize: size_of::<AesCtx>() as u16,
    encrypt: Some(aes_encrypt),
    decrypt: Some(aes_decrypt),
    setkey: Some(aes_setkey),
    reinit: None,
};

/// `enc_xform_aes_ctr`.
#[allow(non_upper_case_globals)] // the C name
pub static enc_xform_aes_ctr: EncXform = EncXform {
    type_: CRYPTO_AES_CTR,
    name: "AES-CTR",
    blocksize: 16,
    ivsize: 8,
    minkey: 16 + 4,
    maxkey: 32 + 4,
    ctxsize: size_of::<AesCtrCtx>() as u16,
    encrypt: Some(aes_ctr_crypt),
    decrypt: Some(aes_ctr_crypt),
    setkey: Some(aes_ctr_setkey),
    reinit: Some(aes_ctr_reinit),
};

/// `enc_xform_aes_gcm`.
#[allow(non_upper_case_globals)] // the C name
pub static enc_xform_aes_gcm: EncXform = EncXform {
    type_: CRYPTO_AES_GCM_16,
    name: "AES-GCM",
    blocksize: 1,
    ivsize: 8,
    minkey: 16 + 4,
    maxkey: 32 + 4,
    ctxsize: size_of::<AesCtrCtx>() as u16,
    encrypt: Some(aes_ctr_crypt),
    decrypt: Some(aes_ctr_crypt),
    setkey: Some(aes_ctr_setkey),
    reinit: Some(aes_gcm_reinit),
};

/// `enc_xform_aes_gmac`.
#[allow(non_upper_case_globals)] // the C name
pub static enc_xform_aes_gmac: EncXform = EncXform {
    type_: CRYPTO_AES_GMAC,
    name: "AES-GMAC",
    blocksize: 1,
    ivsize: 8,
    minkey: 16 + 4,
    maxkey: 32 + 4,
    ctxsize: 0,
    encrypt: None,
    decrypt: None,
    setkey: None,
    reinit: None,
};

/// `enc_xform_aes_xts`.
#[allow(non_upper_case_globals)] // the C name
pub static enc_xform_aes_xts: EncXform = EncXform {
    type_: CRYPTO_AES_XTS,
    name: "AES-XTS",
    blocksize: 16,
    ivsize: 8,
    minkey: 32,
    maxkey: 64,
    ctxsize: size_of::<AesXtsCtx>() as u16,
    encrypt: Some(aes_xts_encrypt),
    decrypt: Some(aes_xts_decrypt),
    setkey: Some(aes_xts_setkey),
    reinit: Some(aes_xts_reinit),
};

/// `enc_xform_chacha20_poly1305`.
#[allow(non_upper_case_globals)] // the C name
pub static enc_xform_chacha20_poly1305: EncXform = EncXform {
    type_: CRYPTO_CHACHA20_POLY1305,
    name: "CHACHA20-POLY1305",
    blocksize: 1,
    ivsize: 8,
    minkey: 32 + 4,
    maxkey: 32 + 4,
    ctxsize: size_of::<Chacha20Ctx>() as u16,
    encrypt: Some(chacha20_crypt_ks),
    decrypt: Some(chacha20_crypt_ks),
    setkey: Some(chacha20_setkey_ks),
    reinit: Some(chacha20_reinit_ks),
};

/// `enc_xform_null`.
#[allow(non_upper_case_globals)] // the C name
pub static enc_xform_null: EncXform = EncXform {
    type_: CRYPTO_NULL,
    name: "NULL",
    blocksize: 4,
    ivsize: 0,
    minkey: 0,
    maxkey: 256,
    ctxsize: 0,
    encrypt: Some(null_encrypt),
    decrypt: Some(null_decrypt),
    setkey: Some(null_setkey),
    reinit: None,
};

// Authentication instances

/// `auth_hash_hmac_md5_96`.
#[allow(non_upper_case_globals)] // the C name
pub static auth_hash_hmac_md5_96: AuthHash = AuthHash {
    type_: CRYPTO_MD5_HMAC,
    name: "HMAC-MD5",
    keysize: 16,
    hashsize: 16,
    authsize: 12,
    ctxsize: size_of::<Md5Ctx>() as u16,
    blocksize: HMAC_MD5_BLOCK_LEN as u16,
    Init: md5_init,
    Setkey: None,
    Reinit: None,
    Update: MD5Update_int,
    Final: md5_final,
};

/// `auth_hash_hmac_sha1_96`.
#[allow(non_upper_case_globals)] // the C name
pub static auth_hash_hmac_sha1_96: AuthHash = AuthHash {
    type_: CRYPTO_SHA1_HMAC,
    name: "HMAC-SHA1",
    keysize: 20,
    hashsize: 20,
    authsize: 12,
    ctxsize: size_of::<Sha1Ctx>() as u16,
    blocksize: HMAC_SHA1_BLOCK_LEN as u16,
    Init: sha1_init,
    Setkey: None,
    Reinit: None,
    Update: SHA1Update_int,
    Final: sha1_final,
};

/// `auth_hash_hmac_ripemd_160_96`.
#[allow(non_upper_case_globals)] // the C name
pub static auth_hash_hmac_ripemd_160_96: AuthHash = AuthHash {
    type_: CRYPTO_RIPEMD160_HMAC,
    name: "HMAC-RIPEMD-160",
    keysize: 20,
    hashsize: 20,
    authsize: 12,
    ctxsize: size_of::<Rmd160Ctx>() as u16,
    blocksize: HMAC_RIPEMD160_BLOCK_LEN as u16,
    Init: rmd160_init,
    Setkey: None,
    Reinit: None,
    Update: RMD160Update_int,
    Final: rmd160_final,
};

/// `auth_hash_hmac_sha2_256_128`.
#[allow(non_upper_case_globals)] // the C name
pub static auth_hash_hmac_sha2_256_128: AuthHash = AuthHash {
    type_: CRYPTO_SHA2_256_HMAC,
    name: "HMAC-SHA2-256",
    keysize: 32,
    hashsize: 32,
    authsize: 16,
    ctxsize: size_of::<Sha256Ctx>() as u16,
    blocksize: HMAC_SHA2_256_BLOCK_LEN as u16,
    Init: sha256_init,
    Setkey: None,
    Reinit: None,
    Update: SHA256Update_int,
    Final: sha256_final,
};

/// `auth_hash_hmac_sha2_384_192`.
#[allow(non_upper_case_globals)] // the C name
pub static auth_hash_hmac_sha2_384_192: AuthHash = AuthHash {
    type_: CRYPTO_SHA2_384_HMAC,
    name: "HMAC-SHA2-384",
    keysize: 48,
    hashsize: 48,
    authsize: 24,
    ctxsize: size_of::<Sha384Ctx>() as u16,
    blocksize: HMAC_SHA2_384_BLOCK_LEN as u16,
    Init: sha384_init,
    Setkey: None,
    Reinit: None,
    Update: SHA384Update_int,
    Final: sha384_final,
};

/// `auth_hash_hmac_sha2_512_256`.
#[allow(non_upper_case_globals)] // the C name
pub static auth_hash_hmac_sha2_512_256: AuthHash = AuthHash {
    type_: CRYPTO_SHA2_512_HMAC,
    name: "HMAC-SHA2-512",
    keysize: 64,
    hashsize: 64,
    authsize: 32,
    ctxsize: size_of::<Sha512Ctx>() as u16,
    blocksize: HMAC_SHA2_512_BLOCK_LEN as u16,
    Init: sha512_init,
    Setkey: None,
    Reinit: None,
    Update: SHA512Update_int,
    Final: sha512_final,
};

/// `auth_hash_gmac_aes_128`.
#[allow(non_upper_case_globals)] // the C name
pub static auth_hash_gmac_aes_128: AuthHash = AuthHash {
    type_: CRYPTO_AES_128_GMAC,
    name: "GMAC-AES-128",
    keysize: 16 + 4,
    hashsize: GMAC_BLOCK_LEN as u16,
    authsize: GMAC_DIGEST_LEN as u16,
    ctxsize: size_of::<AesGmacCtx>() as u16,
    blocksize: AESCTR_BLOCKSIZE as u16,
    Init: gmac_init,
    Setkey: Some(gmac_setkey),
    Reinit: Some(gmac_reinit),
    Update: gmac_update,
    Final: gmac_final,
};

/// `auth_hash_gmac_aes_192`.
#[allow(non_upper_case_globals)] // the C name
pub static auth_hash_gmac_aes_192: AuthHash = AuthHash {
    type_: CRYPTO_AES_192_GMAC,
    name: "GMAC-AES-192",
    keysize: 24 + 4,
    hashsize: GMAC_BLOCK_LEN as u16,
    authsize: GMAC_DIGEST_LEN as u16,
    ctxsize: size_of::<AesGmacCtx>() as u16,
    blocksize: AESCTR_BLOCKSIZE as u16,
    Init: gmac_init,
    Setkey: Some(gmac_setkey),
    Reinit: Some(gmac_reinit),
    Update: gmac_update,
    Final: gmac_final,
};

/// `auth_hash_gmac_aes_256`.
#[allow(non_upper_case_globals)] // the C name
pub static auth_hash_gmac_aes_256: AuthHash = AuthHash {
    type_: CRYPTO_AES_256_GMAC,
    name: "GMAC-AES-256",
    keysize: 32 + 4,
    hashsize: GMAC_BLOCK_LEN as u16,
    authsize: GMAC_DIGEST_LEN as u16,
    ctxsize: size_of::<AesGmacCtx>() as u16,
    blocksize: AESCTR_BLOCKSIZE as u16,
    Init: gmac_init,
    Setkey: Some(gmac_setkey),
    Reinit: Some(gmac_reinit),
    Update: gmac_update,
    Final: gmac_final,
};

/// `auth_hash_chacha20_poly1305`.
#[allow(non_upper_case_globals)] // the C name
pub static auth_hash_chacha20_poly1305: AuthHash = AuthHash {
    type_: CRYPTO_CHACHA20_POLY1305_MAC,
    name: "CHACHA20-POLY1305",
    keysize: (CHACHA20_KEYSIZE + CHACHA20_SALT) as u16,
    hashsize: POLY1305_BLOCK_LEN as u16,
    authsize: POLY1305_TAGLEN as u16,
    ctxsize: size_of::<Chacha20Poly1305Ctx>() as u16,
    blocksize: CHACHA20_BLOCK_LEN as u16,
    Init: chachapoly_init,
    Setkey: Some(chachapoly_setkey),
    Reinit: Some(chachapoly_reinit),
    Update: chachapoly_update,
    Final: chachapoly_final,
};

// Compression instance

/// `comp_algo_deflate`.
#[allow(non_upper_case_globals)] // the C name
pub static comp_algo_deflate: CompAlgo = CompAlgo {
    type_: CRYPTO_DEFLATE_COMP,
    name: "Deflate",
    minlen: 90,
    compress: deflate_compress,
    decompress: deflate_decompress,
};
/* </CODE> */

/* <TESTS> */
#[cfg(test)]
mod tests {
    // Tests of the transform tables: the sizes each entry declares (which `ip_esp.c` and
    // `ip_ah.c` read), and the functions called directly (the software driver's tests cover them
    // through sessions).

    use std::vec::Vec;
    use std::{assert, assert_eq, vec};

    use super::*;
    use crate::crypto::testutil::hex;

    #[test]
    fn encryption_transform_sizes() {
        // (transform, algorithm, blocksize, ivsize, minkey, maxkey)
        let table: &[(&EncXform, i32, u16, u16, u16, u16)] = &[
            (&enc_xform_3des, CRYPTO_3DES_CBC, 8, 8, 24, 24),
            (&enc_xform_blf, CRYPTO_BLF_CBC, 8, 8, 5, 56),
            (&enc_xform_cast5, CRYPTO_CAST_CBC, 8, 8, 5, 16),
            (&enc_xform_aes, CRYPTO_AES_CBC, 16, 16, 16, 32),
            (&enc_xform_aes_ctr, CRYPTO_AES_CTR, 16, 8, 20, 36),
            (&enc_xform_aes_gcm, CRYPTO_AES_GCM_16, 1, 8, 20, 36),
            (&enc_xform_aes_gmac, CRYPTO_AES_GMAC, 1, 8, 20, 36),
            (&enc_xform_aes_xts, CRYPTO_AES_XTS, 16, 8, 32, 64),
            (
                &enc_xform_chacha20_poly1305,
                CRYPTO_CHACHA20_POLY1305,
                1,
                8,
                36,
                36,
            ),
            (&enc_xform_null, CRYPTO_NULL, 4, 0, 0, 256),
        ];
        for (x, alg, blocksize, ivsize, minkey, maxkey) in table {
            assert_eq!(x.type_, *alg, "{}", x.name);
            assert_eq!(
                (x.blocksize, x.ivsize, x.minkey, x.maxkey),
                (*blocksize, *ivsize, *minkey, *maxkey),
                "{}",
                x.name
            );
            // A transform has a schedule exactly when it has a setkey that needs one.
            assert_eq!(
                x.ctxsize == 0,
                x.name == "NULL" || x.name == "AES-GMAC",
                "{}",
                x.name
            );
        }
        assert_eq!(enc_xform_3des.ctxsize, 384);
        // The GMAC "cipher" has no functions at all.
        assert!(enc_xform_aes_gmac.encrypt.is_none() && enc_xform_aes_gmac.setkey.is_none());
        // The modes with their own IV handling have a reinit.
        let reinit: Vec<&str> = [
            &enc_xform_aes_ctr,
            &enc_xform_aes_gcm,
            &enc_xform_aes_xts,
            &enc_xform_chacha20_poly1305,
        ]
        .iter()
        .map(|x| x.name)
        .collect();
        assert_eq!(
            reinit,
            ["AES-CTR", "AES-GCM", "AES-XTS", "CHACHA20-POLY1305"]
        );
        assert!(enc_xform_aes.reinit.is_none() && enc_xform_3des.reinit.is_none());
    }

    #[test]
    fn authentication_transform_sizes() {
        // (transform, algorithm, keysize, hashsize, authsize, blocksize)
        let table: &[(&AuthHash, i32, u16, u16, u16, u16)] = &[
            (&auth_hash_hmac_md5_96, CRYPTO_MD5_HMAC, 16, 16, 12, 64),
            (&auth_hash_hmac_sha1_96, CRYPTO_SHA1_HMAC, 20, 20, 12, 64),
            (
                &auth_hash_hmac_ripemd_160_96,
                CRYPTO_RIPEMD160_HMAC,
                20,
                20,
                12,
                64,
            ),
            (
                &auth_hash_hmac_sha2_256_128,
                CRYPTO_SHA2_256_HMAC,
                32,
                32,
                16,
                64,
            ),
            (
                &auth_hash_hmac_sha2_384_192,
                CRYPTO_SHA2_384_HMAC,
                48,
                48,
                24,
                128,
            ),
            (
                &auth_hash_hmac_sha2_512_256,
                CRYPTO_SHA2_512_HMAC,
                64,
                64,
                32,
                128,
            ),
            (&auth_hash_gmac_aes_128, CRYPTO_AES_128_GMAC, 20, 16, 16, 16),
            (&auth_hash_gmac_aes_192, CRYPTO_AES_192_GMAC, 28, 16, 16, 16),
            (&auth_hash_gmac_aes_256, CRYPTO_AES_256_GMAC, 36, 16, 16, 16),
            (
                &auth_hash_chacha20_poly1305,
                CRYPTO_CHACHA20_POLY1305_MAC,
                36,
                16,
                16,
                64,
            ),
        ];
        for (x, alg, keysize, hashsize, authsize, blocksize) in table {
            assert_eq!(x.type_, *alg, "{}", x.name);
            assert_eq!(
                (x.keysize, x.hashsize, x.authsize, x.blocksize),
                (*keysize, *hashsize, *authsize, *blocksize),
                "{}",
                x.name
            );
            // Only the combined-mode authenticators take a key and an IV themselves.
            let aead = x.name.starts_with("GMAC") || x.name.starts_with("CHACHA");
            assert_eq!(x.Setkey.is_some(), aead, "{}", x.name);
            assert_eq!(x.Reinit.is_some(), aead, "{}", x.name);
        }
    }

    fn mac(axf: &AuthHash, data: &[u8]) -> Vec<u8> {
        let mut ctx = AuthCtx::None;
        (axf.Init)(&mut ctx);
        (axf.Update)(&mut ctx, data).expect("update");
        let mut out = vec![0u8; usize::from(axf.hashsize)];
        (axf.Final)(&mut out, &mut ctx);
        out
    }

    #[test]
    fn the_hash_transforms_are_the_plain_hashes() {
        assert_eq!(
            mac(&auth_hash_hmac_md5_96, b"abc"),
            hex("900150983cd24fb0d6963f7d28e17f72")
        );
        assert_eq!(
            mac(&auth_hash_hmac_sha1_96, b"abc"),
            hex("a9993e364706816aba3e25717850c26c9cd0d89d")
        );
        assert_eq!(
            mac(&auth_hash_hmac_ripemd_160_96, b"abc"),
            hex("8eb208f7e05d987a9b044a8e98c6b087f15a0bfc")
        );
        assert_eq!(
            mac(&auth_hash_hmac_sha2_256_128, b"abc"),
            hex("ba7816bf8f01cfea414140de5dae2223b00361a396177a9cb410ff61f20015ad")
        );
        assert_eq!(
            mac(&auth_hash_hmac_sha2_384_192, b"abc"),
            hex(
                "cb00753f45a35e8bb5a03d699ac65007272c32ab0eded1631a8b605a43ff5bed8086072ba1e7cc2358baeca134c825a7"
            )
        );
        assert_eq!(
            mac(&auth_hash_hmac_sha2_512_256, b"abc"),
            hex(
                "ddaf35a193617abacc417349ae20413112e6fa4e89a97ea20a9eeee64b55d39a2192992a274fc1a836ba3c23a3feebbd454d4423643ce80e2a9ac94fa54ca49f"
            )
        );
    }

    #[test]
    fn aes_ctr_counter_and_gcm_start() {
        let mut ks = Kschedule::None;
        let mut key = hex("ae6852f8121067cc4bf7a5765577f39e");
        key.extend_from_slice(&hex("00000030"));
        (enc_xform_aes_ctr.setkey.expect("setkey"))(&mut ks, &key).expect("setkey");
        (enc_xform_aes_ctr.reinit.expect("reinit"))(&mut ks, &[0; 8]);
        let Kschedule::AesCtr(ctx) = &ks else {
            panic!("an AES-CTR schedule")
        };
        assert_eq!(ctx.ac_block, hex("00000030000000000000000000000000")[..]);
        // GCM's counter starts at 1.
        let mut ks = Kschedule::None;
        (enc_xform_aes_gcm.setkey.expect("setkey"))(&mut ks, &key).expect("setkey");
        (enc_xform_aes_gcm.reinit.expect("reinit"))(&mut ks, &[0; 8]);
        let Kschedule::AesCtr(ctx) = &ks else {
            panic!("an AES-CTR schedule")
        };
        assert_eq!(ctx.ac_block, hex("00000030000000000000000000000001")[..]);
        // Each block advances the counter, with carry out of the low byte.
        let mut blk = [0u8; 16];
        (enc_xform_aes_ctr.encrypt.expect("encrypt"))(&mut ks, &mut blk);
        let Kschedule::AesCtr(ctx) = &ks else {
            panic!("an AES-CTR schedule")
        };
        assert_eq!(ctx.ac_block[15], 2);
    }

    #[test]
    fn setkey_refuses_what_the_c_would_read_past() {
        let mut ks = Kschedule::None;
        assert_eq!(
            (enc_xform_3des.setkey.expect("setkey"))(&mut ks, &[0; 23]),
            Err(Errno::EINVAL)
        );
        assert_eq!(
            (enc_xform_blf.setkey.expect("setkey"))(&mut ks, &[]),
            Err(Errno::EINVAL)
        );
        assert_eq!(
            (enc_xform_aes_ctr.setkey.expect("setkey"))(&mut ks, &[0; 3]),
            Err(Errno::EINVAL)
        );
        assert_eq!(
            (enc_xform_aes_ctr.setkey.expect("setkey"))(&mut ks, &[0; 17]),
            Err(Errno::EINVAL)
        );
        assert_eq!(
            (enc_xform_chacha20_poly1305.setkey.expect("setkey"))(&mut ks, &[0; 35]),
            Err(Errno::EINVAL)
        );
        let mut ctx = AuthCtx::None;
        (auth_hash_gmac_aes_128.Init)(&mut ctx);
        assert_eq!(
            (auth_hash_gmac_aes_128.Setkey.expect("setkey"))(&mut ctx, &[0; 18]),
            Err(Errno::EINVAL)
        );
        assert_eq!(
            (auth_hash_gmac_aes_128.Setkey.expect("setkey"))(&mut ctx, &[0; 20]),
            Ok(())
        );
    }
}
/* </TESTS> */
