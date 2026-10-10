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
/* </LICENSES> */

/* <CODE> */
//! `sys/crypto`: the kernel's cryptographic primitives and the crypto framework (`crypto(9)`).

#[forbid(unsafe_code)]
pub mod aes;
#[forbid(unsafe_code)]
pub mod blake2s;
#[forbid(unsafe_code)]
pub mod blf;
#[forbid(unsafe_code)]
pub mod cast;
#[forbid(unsafe_code)]
pub mod chacha_private;
#[forbid(unsafe_code)]
pub mod chachapoly;
pub mod criov;
#[allow(clippy::module_inception)] // crypto.c, the file, in the crypto directory
pub mod crypto;
#[forbid(unsafe_code)]
pub mod cryptodev;
pub mod cryptosoft;
#[forbid(unsafe_code)]
pub mod curve25519;
#[forbid(unsafe_code)]
pub mod des_locl;
#[forbid(unsafe_code)]
pub mod ecb3_enc;
#[forbid(unsafe_code)]
pub mod ecb_enc;
#[forbid(unsafe_code)]
pub mod gmac;
#[forbid(unsafe_code)]
pub mod hmac;
#[forbid(unsafe_code)]
pub mod idgen;
#[forbid(unsafe_code)]
pub mod md5;
#[forbid(unsafe_code)]
pub mod podd;
#[forbid(unsafe_code)]
pub mod poly1305;
#[forbid(unsafe_code)]
pub mod rijndael;
#[forbid(unsafe_code)]
pub mod rmd160;
#[forbid(unsafe_code)]
pub mod set_key;
#[forbid(unsafe_code)]
pub mod sha1;
#[forbid(unsafe_code)]
pub mod sha2;
#[forbid(unsafe_code)]
pub mod siphash;
#[forbid(unsafe_code)]
pub mod sk;
#[forbid(unsafe_code)]
pub mod spr;
#[cfg(test)]
#[forbid(unsafe_code)]
pub(crate) mod testutil;
#[forbid(unsafe_code)]
pub mod xform;
#[forbid(unsafe_code)]
pub mod xform_ipcomp;

/// `explicit_bzero(&x, sizeof(x))` for a context that is a plain value: overwrites it with its
/// default (all zero) and keeps the compiler from proving the store dead, so the secret it held
/// does not outlive the call. The byte buffers use `libkern::explicit_bzero`.
pub fn wipe<T: Default>(x: &mut T) {
    *x = T::default();
    core::hint::black_box(&*x);
}
/* </CODE> */
