/*	$OpenBSD: chachapoly.h,v 1.4 2020/07/22 13:54:30 tobhe Exp $	*/
/*	$OpenBSD: chachapoly.c,v 1.6 2020/07/22 13:54:30 tobhe Exp $	*/
/* <LICENSES> */
/*
 * Copyright (c) 2015 Mike Belopuhov
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

//! ChaCha20-Poly1305 and XChaCha20-Poly1305: the AEAD constructions of RFC 8439 and
//! draft-irtf-cfrg-xchacha, in the three shapes OpenBSD uses them. IPsec drives the
//! `chacha20_*` and `Chacha20_Poly1305_*` functions through the `xform` tables (RFC 7634, ESP
//! with a salt in the key material); WireGuard calls `chacha20poly1305_*` and
//! `xchacha20poly1305_*` directly with a 64-bit counter nonce or a 24-byte nonce.
//!
//! Upstream: sys/crypto/chachapoly.h @ 3ce1f3f79392, sys/crypto/chachapoly.c @ 3ce1f3f79392
//!
//! ## Deviations
//! - The header and the file share this module.
//! - `struct chacha20_ctx` keeps its `block` as a [`ChachaCtx`] (the C declares 64 bytes and
//!   casts); `CHACHA20_POLY1305_CTX` embeds a [`Poly1305State`] where the C has `struct
//!   poly1305_ctx` (an `unsigned long state[14]` that only exists to size and cast to
//!   `poly1305_state`), which is therefore not ported, and a [`Chacha20Ctx`] named `chacha`.
//!   The functions that take `void *` take the typed context; `xform.rs` adapts them to its
//!   `AuthCtx`/`Kschedule`.
//! - `chacha20_setkey` returns `Result<(), Errno>` (`EINVAL`) for the C's `-1`; `data` is a
//!   64-byte slice. `Chacha20_Poly1305_Setkey`, `_Reinit` and `_Update` take slices where the C
//!   takes a pointer and a `u_int16_t` length; `_Update` returns `Result<(), Errno>` for the
//!   `int` that is always 0.
//! - `chacha20poly1305_encrypt` and friends take `dst` and `src` as separate slices; `dst`
//!   holds `src.len() + 16` bytes. WireGuard's `buf, buf` calls (the C allows `dst == src`)
//!   are the `_inplace` functions, whose buffer holds the message and then its tag.
//!   `chacha20poly1305_decrypt` returns `true` where the C returns 1 (authentic).
//! - Secrets on the stack are wiped by assignment through `crate::crypto::wipe`.

use libkern::{explicit_bzero, timingsafe_bcmp};

use super::chacha_private::{
    ChachaCtx, chacha_encrypt_bytes_inplace, chacha_ivsetup, chacha_keysetup, hchacha20,
};
use super::poly1305::{
    Poly1305State, poly1305_block_size, poly1305_finish, poly1305_init, poly1305_update,
};
use super::wipe;
use crate::sys::errno::Errno;

/// `CHACHA20_KEYSIZE`.
pub const CHACHA20_KEYSIZE: usize = 32;
/// `CHACHA20_CTR`: bytes of block counter in the nonce word of the key material.
pub const CHACHA20_CTR: usize = 4;
/// `CHACHA20_SALT`: bytes of salt appended to the key.
pub const CHACHA20_SALT: usize = 4;
/// `CHACHA20_NONCE`.
pub const CHACHA20_NONCE: usize = 8;
/// `CHACHA20_BLOCK_LEN`.
pub const CHACHA20_BLOCK_LEN: usize = 64;

/// `POLY1305_KEYLEN`.
pub const POLY1305_KEYLEN: usize = 32;
/// `POLY1305_TAGLEN`.
pub const POLY1305_TAGLEN: usize = 16;
/// `POLY1305_BLOCK_LEN`.
pub const POLY1305_BLOCK_LEN: usize = 16;

/// `CHACHA20POLY1305_KEY_SIZE`: WireGuard crypto.
pub const CHACHA20POLY1305_KEY_SIZE: usize = CHACHA20_KEYSIZE;
/// `CHACHA20POLY1305_AUTHTAG_SIZE`.
pub const CHACHA20POLY1305_AUTHTAG_SIZE: usize = POLY1305_TAGLEN;
/// `XCHACHA20POLY1305_NONCE_SIZE`.
pub const XCHACHA20POLY1305_NONCE_SIZE: usize = 24;

/// `pad0`.
const PAD0: [u8; 16] = [0; 16];

/// `struct chacha20_ctx`: the cipher's key schedule for the `enc_xform` (`block`) and the
/// counter and salt words (`nonce`).
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Chacha20Ctx {
    /// `block`: the ChaCha state.
    pub block: ChachaCtx,
    /// `nonce`: the block counter (4 bytes) and the salt (4 bytes).
    pub nonce: [u8; CHACHA20_NONCE],
}

/// `chacha20_setkey`: the key is 32 bytes plus the 4-byte salt; `len` is their total.
pub fn chacha20_setkey(ctx: &mut Chacha20Ctx, key: &[u8], len: i32) -> Result<(), Errno> {
    if len != (CHACHA20_KEYSIZE + CHACHA20_SALT) as i32
        || key.len() < CHACHA20_KEYSIZE + CHACHA20_SALT
    {
        return Err(Errno::EINVAL);
    }

    // initial counter is 1
    ctx.nonce[0] = 1;
    ctx.nonce[CHACHA20_CTR..CHACHA20_CTR + CHACHA20_SALT]
        .copy_from_slice(&key[CHACHA20_KEYSIZE..CHACHA20_KEYSIZE + CHACHA20_SALT]);
    chacha_keysetup(&mut ctx.block, key, (CHACHA20_KEYSIZE * 8) as u32);
    Ok(())
}

/// `chacha20_reinit`: sets the 8-byte IV (and the counter and salt) for the next message.
pub fn chacha20_reinit(ctx: &mut Chacha20Ctx, iv: &[u8]) {
    chacha_ivsetup(&mut ctx.block, iv, Some(&ctx.nonce));
}

/// `chacha20_crypt`: XORs the next keystream block into `data` (`CHACHA20_BLOCK_LEN` bytes).
pub fn chacha20_crypt(ctx: &mut Chacha20Ctx, data: &mut [u8]) {
    chacha_encrypt_bytes_inplace(&mut ctx.block, &mut data[..CHACHA20_BLOCK_LEN]);
}

/// `CHACHA20_POLY1305_CTX`: an IPsec AEAD in progress (RFC 7634).
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Chacha20Poly1305Ctx {
    /// `key`: the one-time Poly1305 key, the first keystream bytes of the message.
    pub key: [u8; POLY1305_KEYLEN],
    /// `nonce`: counter, salt.
    pub nonce: [u8; CHACHA20_NONCE],
    /// `chacha`.
    pub chacha: Chacha20Ctx,
    /// `poly`.
    pub poly: Poly1305State,
}

/// `Chacha20_Poly1305_Init`.
#[allow(non_snake_case)] // the C name
pub fn Chacha20_Poly1305_Init(ctx: &mut Chacha20Poly1305Ctx) {
    *ctx = Chacha20Poly1305Ctx::default();
}

/// `Chacha20_Poly1305_Setkey`: the key is 32 bytes followed by the salt.
#[allow(non_snake_case)] // the C name
pub fn Chacha20_Poly1305_Setkey(ctx: &mut Chacha20Poly1305Ctx, key: &[u8]) {
    // salt is provided with the key material
    ctx.nonce[CHACHA20_CTR..CHACHA20_CTR + CHACHA20_SALT]
        .copy_from_slice(&key[CHACHA20_KEYSIZE..CHACHA20_KEYSIZE + CHACHA20_SALT]);
    chacha_keysetup(&mut ctx.chacha.block, key, (CHACHA20_KEYSIZE * 8) as u32);
}

/// `Chacha20_Poly1305_Reinit`: starts a message under the 8-byte IV; the first keystream
/// bytes become the Poly1305 key.
#[allow(non_snake_case)] // the C name
pub fn Chacha20_Poly1305_Reinit(ctx: &mut Chacha20Poly1305Ctx, iv: &[u8]) {
    // initial counter is 0
    chacha_ivsetup(&mut ctx.chacha.block, iv, Some(&ctx.nonce));
    chacha_encrypt_bytes_inplace(&mut ctx.chacha.block, &mut ctx.key);
    poly1305_init(&mut ctx.poly, &ctx.key);
}

/// `Chacha20_Poly1305_Update`: authenticates `data`, then zero-pads to a 16-byte boundary.
#[allow(non_snake_case)] // the C name
pub fn Chacha20_Poly1305_Update(ctx: &mut Chacha20Poly1305Ctx, data: &[u8]) -> Result<(), Errno> {
    const ZEROES: [u8; POLY1305_BLOCK_LEN] = [0; POLY1305_BLOCK_LEN];

    poly1305_update(&mut ctx.poly, data);

    // number of bytes in the last 16 byte block
    let rem = (data.len() + POLY1305_BLOCK_LEN) & (POLY1305_BLOCK_LEN - 1);
    if rem > 0 {
        poly1305_update(&mut ctx.poly, &ZEROES[..POLY1305_BLOCK_LEN - rem]);
    }
    Ok(())
}

/// `Chacha20_Poly1305_Final`: the tag; the context is wiped.
#[allow(non_snake_case)] // the C name
pub fn Chacha20_Poly1305_Final(tag: &mut [u8; POLY1305_TAGLEN], ctx: &mut Chacha20Poly1305Ctx) {
    poly1305_finish(&mut ctx.poly, tag);
    wipe(ctx);
}

/// The zero bytes that bring `len` up to a multiple of 16: `(0x10 - len) & 0xf` of them.
fn pad(len: usize) -> &'static [u8] {
    &PAD0[..(0x10usize.wrapping_sub(len)) & 0xf]
}

/// The Poly1305 key (the first 32 bytes of ChaCha20 block 0) and the cipher positioned at
/// block 1, for `nonce` under `key`.
fn chacha20poly1305_setup(
    nonce: u64,
    key: &[u8; CHACHA20POLY1305_KEY_SIZE],
) -> (ChachaCtx, Poly1305State) {
    let mut chacha_ctx = ChachaCtx::default();
    let mut poly1305_ctx = Poly1305State::default();
    let mut b0 = [0u8; CHACHA20POLY1305_KEY_SIZE];
    let le_nonce = nonce.to_le_bytes();

    chacha_keysetup(&mut chacha_ctx, key, (CHACHA20POLY1305_KEY_SIZE * 8) as u32);
    chacha_ivsetup(&mut chacha_ctx, &le_nonce, None);
    chacha_encrypt_bytes_inplace(&mut chacha_ctx, &mut b0);
    poly1305_init(&mut poly1305_ctx, &b0);
    explicit_bzero(&mut b0);
    (chacha_ctx, poly1305_ctx)
}

/// The tag over `ad` and `ct` (RFC 8439 section 2.8).
fn chacha20poly1305_mac(
    poly1305_ctx: &mut Poly1305State,
    ad: &[u8],
    ct: &[u8],
) -> [u8; CHACHA20POLY1305_AUTHTAG_SIZE] {
    let mut lens = [0u8; 16];

    poly1305_update(poly1305_ctx, ad);
    poly1305_update(poly1305_ctx, pad(ad.len()));

    poly1305_update(poly1305_ctx, ct);
    poly1305_update(poly1305_ctx, pad(ct.len()));

    lens[..8].copy_from_slice(&(ad.len() as u64).to_le_bytes());
    lens[8..].copy_from_slice(&(ct.len() as u64).to_le_bytes());
    poly1305_update(poly1305_ctx, &lens);

    let mut mac = [0u8; CHACHA20POLY1305_AUTHTAG_SIZE];
    poly1305_finish(poly1305_ctx, &mut mac);
    mac
}

/// `chacha20poly1305_encrypt`: `dst` (`src.len() + 16` bytes) gets the ciphertext of `src`
/// followed by the tag over `ad` and the ciphertext, under `key` and the counter `nonce`.
pub fn chacha20poly1305_encrypt(
    dst: &mut [u8],
    src: &[u8],
    ad: &[u8],
    nonce: u64,
    key: &[u8; CHACHA20POLY1305_KEY_SIZE],
) {
    dst[..src.len()].copy_from_slice(src);
    chacha20poly1305_encrypt_inplace(dst, src.len(), ad, nonce, key);
}

/// `chacha20poly1305_encrypt` with `dst == src`: `buf[..src_len]` holds the message, and holds
/// the ciphertext and then the tag (`buf[src_len..src_len + 16]`) on return.
pub fn chacha20poly1305_encrypt_inplace(
    buf: &mut [u8],
    src_len: usize,
    ad: &[u8],
    nonce: u64,
    key: &[u8; CHACHA20POLY1305_KEY_SIZE],
) {
    let (mut chacha_ctx, mut poly1305_ctx) = chacha20poly1305_setup(nonce, key);

    let (data, tag) = buf[..src_len + CHACHA20POLY1305_AUTHTAG_SIZE].split_at_mut(src_len);
    chacha_encrypt_bytes_inplace(&mut chacha_ctx, data);
    tag.copy_from_slice(&chacha20poly1305_mac(&mut poly1305_ctx, ad, data));

    wipe(&mut chacha_ctx);
    wipe(&mut poly1305_ctx);
}

/// `chacha20poly1305_decrypt`: checks the tag at the end of `src` and, when it is authentic,
/// writes the plaintext (`src.len() - 16` bytes) to `dst` and returns `true`; otherwise
/// `dst` is left alone and the result is `false`.
pub fn chacha20poly1305_decrypt(
    dst: &mut [u8],
    src: &[u8],
    ad: &[u8],
    nonce: u64,
    key: &[u8; CHACHA20POLY1305_KEY_SIZE],
) -> bool {
    if src.len() < CHACHA20POLY1305_AUTHTAG_SIZE {
        return false;
    }
    let dst_len = src.len() - CHACHA20POLY1305_AUTHTAG_SIZE;

    let (mut chacha_ctx, mut poly1305_ctx) = chacha20poly1305_setup(nonce, key);
    let mut mac = chacha20poly1305_mac(&mut poly1305_ctx, ad, &src[..dst_len]);

    let ret = timingsafe_bcmp(&mac, &src[dst_len..]);
    if !ret {
        dst[..dst_len].copy_from_slice(&src[..dst_len]);
        chacha_encrypt_bytes_inplace(&mut chacha_ctx, &mut dst[..dst_len]);
    }

    wipe(&mut chacha_ctx);
    wipe(&mut poly1305_ctx);
    explicit_bzero(&mut mac);

    !ret
}

/// `chacha20poly1305_decrypt` with `dst == src`: `buf` is the ciphertext followed by the tag;
/// on `true` its first `buf.len() - 16` bytes are the plaintext.
pub fn chacha20poly1305_decrypt_inplace(
    buf: &mut [u8],
    ad: &[u8],
    nonce: u64,
    key: &[u8; CHACHA20POLY1305_KEY_SIZE],
) -> bool {
    if buf.len() < CHACHA20POLY1305_AUTHTAG_SIZE {
        return false;
    }
    let dst_len = buf.len() - CHACHA20POLY1305_AUTHTAG_SIZE;

    let (mut chacha_ctx, mut poly1305_ctx) = chacha20poly1305_setup(nonce, key);
    let mut mac = chacha20poly1305_mac(&mut poly1305_ctx, ad, &buf[..dst_len]);

    let ret = timingsafe_bcmp(&mac, &buf[dst_len..]);
    if !ret {
        chacha_encrypt_bytes_inplace(&mut chacha_ctx, &mut buf[..dst_len]);
    }

    wipe(&mut chacha_ctx);
    wipe(&mut poly1305_ctx);
    explicit_bzero(&mut mac);

    !ret
}

/// The key and counter of the inner ChaCha20-Poly1305 for an XChaCha20 `nonce` under `key`:
/// HChaCha20 over the first 16 bytes, and the last 8 as the counter nonce.
fn xchacha20_derive(
    nonce: &[u8; XCHACHA20POLY1305_NONCE_SIZE],
    key: &[u8; CHACHA20POLY1305_KEY_SIZE],
) -> (u64, [u8; CHACHA20POLY1305_KEY_SIZE]) {
    let mut derived_key = [0u32; CHACHA20POLY1305_KEY_SIZE / 4];
    let mut derived = [0u8; CHACHA20POLY1305_KEY_SIZE];
    let mut n16 = [0u8; 16];
    let mut n8 = [0u8; 8];

    n16.copy_from_slice(&nonce[..16]);
    n8.copy_from_slice(&nonce[16..]);
    let h_nonce = u64::from_le_bytes(n8);
    hchacha20(&mut derived_key, &n16, key);

    for (i, w) in derived_key.iter().enumerate() {
        derived[4 * i..4 * i + 4].copy_from_slice(&w.to_le_bytes());
    }
    wipe(&mut derived_key);
    (h_nonce, derived)
}

/// `xchacha20poly1305_encrypt`: as [`chacha20poly1305_encrypt`] with a 24-byte nonce.
pub fn xchacha20poly1305_encrypt(
    dst: &mut [u8],
    src: &[u8],
    ad: &[u8],
    nonce: &[u8; XCHACHA20POLY1305_NONCE_SIZE],
    key: &[u8; CHACHA20POLY1305_KEY_SIZE],
) {
    let (h_nonce, mut derived_key) = xchacha20_derive(nonce, key);

    chacha20poly1305_encrypt(dst, src, ad, h_nonce, &derived_key);
    explicit_bzero(&mut derived_key);
}

/// `xchacha20poly1305_decrypt`: as [`chacha20poly1305_decrypt`] with a 24-byte nonce.
pub fn xchacha20poly1305_decrypt(
    dst: &mut [u8],
    src: &[u8],
    ad: &[u8],
    nonce: &[u8; XCHACHA20POLY1305_NONCE_SIZE],
    key: &[u8; CHACHA20POLY1305_KEY_SIZE],
) -> bool {
    let (h_nonce, mut derived_key) = xchacha20_derive(nonce, key);

    let ret = chacha20poly1305_decrypt(dst, src, ad, h_nonce, &derived_key);
    explicit_bzero(&mut derived_key);

    ret
}

const _: () = assert!(poly1305_block_size == POLY1305_BLOCK_LEN);

#[cfg(test)]
mod tests;
