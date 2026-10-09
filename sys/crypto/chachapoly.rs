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

/* <CODE> */
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
/* </CODE> */

/* <TESTS> */
#[cfg(test)]
mod tests {
    // Known-answer tests for ChaCha20-Poly1305: RFC 8439 section 2.8.2 through the IPsec-style
    // interface (RFC 7634: the salt is the first word of the nonce), XChaCha20-Poly1305 of
    // draft-irtf-cfrg-xchacha appendix A.3.1, and WireGuard-style (64-bit counter nonce) vectors
    // computed with an independent reference built on RFC 8439's primitives.

    use super::*;
    use crate::crypto::testutil::{hex, hexn};

    extern crate std;
    use std::vec::Vec;

    const SUNSCREEN: &[u8] = b"Ladies and Gentlemen of the class of '99: If I could offer you only one tip for the future, sunscreen would be it.";

    fn key_80() -> [u8; 32] {
        let mut k = [0u8; 32];
        for (i, b) in k.iter_mut().enumerate() {
            *b = 0x80 + i as u8;
        }
        k
    }

    fn key_00() -> [u8; 32] {
        let mut k = [0u8; 32];
        for (i, b) in k.iter_mut().enumerate() {
            *b = i as u8;
        }
        k
    }

    fn data() -> [u8; 200] {
        let mut d = [0u8; 200];
        for (i, b) in d.iter_mut().enumerate() {
            *b = ((i * 7 + 1) % 256) as u8;
        }
        d
    }

    fn aad() -> [u8; 37] {
        let mut a = [0u8; 37];
        for (i, b) in a.iter_mut().enumerate() {
            *b = ((i * 3 + 5) % 256) as u8;
        }
        a
    }

    const RFC8439_2_8_2_CT: &str =
        "d31a8d34648e60db7b86afbc53ef7ec2a4aded51296e08fea9e2b5a736ee62d6
    3dbea45e8ca9671282fafb69da92728b1a71de0a9e060b2905d6a5b67ecd3b36
    92ddbd7f2d778b8c9803aee328091b58fab324e4fad675945585808b4831d7bc
    3ff4def08e4b7a9de576d26586cec64b6116";

    #[test]
    fn rfc8439_2_8_2_through_the_ipsec_interface() {
        // Key material: the key followed by the salt (the RFC's 07 00 00 00); IV 40 .. 47.
        let mut material = [0u8; 36];
        material[..32].copy_from_slice(&key_80());
        material[32..].copy_from_slice(&[7, 0, 0, 0]);
        let iv: [u8; 8] = hexn("4041424344454647");
        let aad: [u8; 12] = hexn("50515253c0c1c2c3c4c5c6c7");

        // The authenticator side (`auth_hash_chacha20_poly1305`) ...
        let mut auth = Chacha20Poly1305Ctx::default();
        Chacha20_Poly1305_Init(&mut auth);
        Chacha20_Poly1305_Setkey(&mut auth, &material);
        Chacha20_Poly1305_Reinit(&mut auth, &iv);
        assert_eq!(Chacha20_Poly1305_Update(&mut auth, &aad), Ok(()));

        // ... and the cipher side (`enc_xform_chacha20_poly1305`), a block at a time as
        // swcr_authenc does it.
        let mut enc = Chacha20Ctx::default();
        assert_eq!(chacha20_setkey(&mut enc, &material, 36), Ok(()));
        chacha20_reinit(&mut enc, &iv);
        let mut ct = Vec::new();
        for chunk in SUNSCREEN.chunks(CHACHA20_BLOCK_LEN) {
            let mut blk = [0u8; CHACHA20_BLOCK_LEN];
            blk[..chunk.len()].copy_from_slice(chunk);
            chacha20_crypt(&mut enc, &mut blk);
            Chacha20_Poly1305_Update(&mut auth, &blk[..chunk.len()]).unwrap();
            ct.extend_from_slice(&blk[..chunk.len()]);
        }
        assert_eq!(ct, hex(RFC8439_2_8_2_CT));

        // The length block: aad length, then ciphertext length, little-endian.
        let mut blk = [0u8; 16];
        blk[0..4].copy_from_slice(&(aad.len() as u32).to_le_bytes());
        blk[8..12].copy_from_slice(&(ct.len() as u32).to_le_bytes());
        Chacha20_Poly1305_Update(&mut auth, &blk).unwrap();

        let mut tag = [0u8; POLY1305_TAGLEN];
        Chacha20_Poly1305_Final(&mut tag, &mut auth);
        assert_eq!(tag.to_vec(), hex("1ae10b594f09e26a7e902ecbd0600691"));
        // Final wipes the context.
        assert_eq!(auth, Chacha20Poly1305Ctx::default());
    }

    #[test]
    fn chacha20_setkey_wants_key_and_salt() {
        let mut enc = Chacha20Ctx::default();
        assert_eq!(chacha20_setkey(&mut enc, &[0; 32], 32), Err(Errno::EINVAL));
        assert_eq!(chacha20_setkey(&mut enc, &[0; 40], 40), Err(Errno::EINVAL));
        assert_eq!(chacha20_setkey(&mut enc, &[0; 36], 36), Ok(()));
        // The initial counter is 1, the salt follows it.
        assert_eq!(enc.nonce, [1, 0, 0, 0, 0, 0, 0, 0]);
    }

    /// `(src length, ad length, nonce, ciphertext and tag)`, `key_00`, `data`, `aad`.
    fn wg_vectors() -> Vec<(usize, usize, u64, &'static str)> {
        std::vec![
            (0, 0, 0, "10324f800a160bd9a1794255be7ec29d"),
            (1, 0, 1, "9e235d163cb0cc57452874cd334b650751"),
            (
                16,
                16,
                2,
                "a8abf143face875cf20f783e999202df33fab8c6f99eb8e01176ac66f3a9253b"
            ),
            (
                64,
                13,
                0x0807060504030201,
                "eeeeaaeeb8a883ae29ffaf98dfb04434e4f41bd791d9b3beb265835ca924765fd6f8e07b976b5d9d6439a020ef45adf81fd8c519066a8c804e14ade2b4cb6357aa95a58c727b9276c7468a9a43131cc2"
            ),
            (
                130,
                37,
                u64::MAX,
                "518b5a55bf21991eaa63b54c7e989517f2f7ef6090c854916de189bf3d978398f07913a3f81e242f87cbb8554f73c9519a50eda9635d0c1f0dba6b1ec4a50637ec5bffbd5537c508f04ec929e974dbe96d4a800da95a32c3b55af9065d5516d8fe7e087fc62bcce9c45d9068eb6973bcc1503358444a366057ec11533c251703f0e2762ff096de91f8b6223e7465e06c5619"
            ),
            (
                200,
                0,
                7,
                "f0078b5e3fcc51bb5b76097da9d473f043e5ddc77c7992a7032a8459222504c6f1c579ef37e3f80ba03866c661d83950853f78886403a3a676f39b3e4af987bcc15278edfa457d9e5fde67c9244a554c9b5e4d96a00151d51640007958de938c6214b7572bc88b22cb4aa353170ff37b76a3e9dbe5f1537c1f4bb9db44c72bfb99e467100d610819430c055d05d4779356b76459ab60a5d40ad805ec9a831b291751b305571d58309707645b9aa17ad3a66b9628c72cf0244bae0b1f2e3587122324eb14e95aa71d4c1f82802e3bbb783b370c90d1644532"
            ),
        ]
    }

    #[test]
    fn chacha20poly1305_known_answers() {
        let d = data();
        let a = aad();
        let key = key_00();
        for (n, alen, nonce, want) in wg_vectors() {
            let want = hex(want);

            let mut dst = std::vec![0u8; n + 16];
            chacha20poly1305_encrypt(&mut dst, &d[..n], &a[..alen], nonce, &key);
            assert_eq!(dst, want, "encrypt {n} {alen}");

            // In place: the message first, room for the tag after it.
            let mut buf = std::vec![0u8; n + 16];
            buf[..n].copy_from_slice(&d[..n]);
            chacha20poly1305_encrypt_inplace(&mut buf, n, &a[..alen], nonce, &key);
            assert_eq!(buf, want, "encrypt in place {n} {alen}");

            let mut out = std::vec![0u8; n];
            assert!(chacha20poly1305_decrypt(
                &mut out,
                &want,
                &a[..alen],
                nonce,
                &key
            ));
            assert_eq!(out, &d[..n]);

            assert!(chacha20poly1305_decrypt_inplace(
                &mut buf,
                &a[..alen],
                nonce,
                &key
            ));
            assert_eq!(&buf[..n], &d[..n]);
        }
    }

    #[test]
    fn chacha20poly1305_rejects_forgeries() {
        let d = data();
        let a = aad();
        let key = key_00();
        let mut sealed = std::vec![0u8; 60 + 16];
        chacha20poly1305_encrypt(&mut sealed, &d[..60], &a[..20], 99, &key);

        let mut out = std::vec![0xaa_u8; 60];
        assert!(chacha20poly1305_decrypt(
            &mut out,
            &sealed,
            &a[..20],
            99,
            &key
        ));
        assert_eq!(out, &d[..60]);

        // Every byte of the ciphertext and of the tag counts, as do ad, nonce and key.
        for i in 0..sealed.len() {
            let mut bad = sealed.clone();
            bad[i] ^= 1;
            let mut out = std::vec![0xaa_u8; 60];
            assert!(
                !chacha20poly1305_decrypt(&mut out, &bad, &a[..20], 99, &key),
                "byte {i}"
            );
            assert_eq!(
                out,
                std::vec![0xaa_u8; 60],
                "plaintext released for byte {i}"
            );
            let mut inplace = bad.clone();
            assert!(!chacha20poly1305_decrypt_inplace(
                &mut inplace,
                &a[..20],
                99,
                &key
            ));
            assert_eq!(inplace, bad, "in-place buffer touched for byte {i}");
        }
        let mut out = std::vec![0u8; 60];
        assert!(!chacha20poly1305_decrypt(
            &mut out,
            &sealed,
            &a[..19],
            99,
            &key
        ));
        assert!(!chacha20poly1305_decrypt(
            &mut out,
            &sealed,
            &a[..20],
            98,
            &key
        ));
        let mut other = key;
        other[31] ^= 0x80;
        assert!(!chacha20poly1305_decrypt(
            &mut out,
            &sealed,
            &a[..20],
            99,
            &other
        ));

        // Shorter than a tag.
        assert!(!chacha20poly1305_decrypt(
            &mut out,
            &sealed[..15],
            &[],
            99,
            &key
        ));
        assert!(!chacha20poly1305_decrypt_inplace(
            &mut [0u8; 15],
            &[],
            99,
            &key
        ));
        // Just a tag: the empty message.
        let mut empty = [0u8; 16];
        chacha20poly1305_encrypt(&mut empty, &[], &[], 3, &key);
        assert!(chacha20poly1305_decrypt(&mut [], &empty, &[], 3, &key));
    }

    #[test]
    fn xchacha20poly1305_draft_irtf_cfrg_xchacha_a_3_1() {
        let key = key_80();
        let mut nonce = [0u8; XCHACHA20POLY1305_NONCE_SIZE];
        for (i, b) in nonce.iter_mut().enumerate() {
            *b = 0x40 + i as u8;
        }
        let aad: [u8; 12] = hexn("50515253c0c1c2c3c4c5c6c7");
        let want = hex(
            "bd6d179d3e83d43b9576579493c0e939572a1700252bfaccbed2902c21396cbb
        731c7f1b0b4aa6440bf3a82f4eda7e39ae64c6708c54c216cb96b72e1213b4522f8c9ba4
        0db5d945b11b69b982c1bb9e3f3fac2bc369488f76b2383565d3fff921f9664c97637da9
        768812f615c68b13b52ec0875924c1c7987947deafd8780acf49",
        );

        let mut sealed = std::vec![0u8; SUNSCREEN.len() + 16];
        xchacha20poly1305_encrypt(&mut sealed, SUNSCREEN, &aad, &nonce, &key);
        assert_eq!(sealed, want);

        let mut out = std::vec![0u8; SUNSCREEN.len()];
        assert!(xchacha20poly1305_decrypt(
            &mut out, &sealed, &aad, &nonce, &key
        ));
        assert_eq!(out, SUNSCREEN);

        sealed[3] ^= 1;
        assert!(!xchacha20poly1305_decrypt(
            &mut out, &sealed, &aad, &nonce, &key
        ));
    }

    #[test]
    fn xchacha20poly1305_other_nonce() {
        let d = data();
        let a = aad();
        let key = key_00();
        let mut nonce = [0u8; XCHACHA20POLY1305_NONCE_SIZE];
        for (i, b) in nonce.iter_mut().enumerate() {
            *b = ((i * 11 + 2) % 256) as u8;
        }
        let cases = [
            (0, 0, "08cbe4e4c9ee8cf702ad3e7898818fcd"),
            (5, 3, "a5cb532f8854e6b9073f4334f1550821440a952488"),
            (
                100,
                20,
                "a5cb532f886c7f16664ade2b6436496fdfb88c32edab4b32ae4507e5ebb7160d546b410e4abd817637fb15bca8a6dcc07ddfcf5e3cc99c804c2af956d2bc8b5a800ed1dd9fec533472f97a699690904712f0176d5962f5c8db56d2de8939abf5389f05439e051bece7ca28ec644b8303ca0c0faa",
            ),
        ];
        for (n, alen, want) in cases {
            let mut sealed = std::vec![0u8; n + 16];
            xchacha20poly1305_encrypt(&mut sealed, &d[..n], &a[..alen], &nonce, &key);
            assert_eq!(sealed, hex(want), "{n} {alen}");
            let mut out = std::vec![0u8; n];
            assert!(xchacha20poly1305_decrypt(
                &mut out,
                &sealed,
                &a[..alen],
                &nonce,
                &key
            ));
            assert_eq!(out, &d[..n]);
        }
    }
}
/* </TESTS> */
