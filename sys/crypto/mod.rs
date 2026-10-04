//! `sys/crypto`: the kernel's cryptographic primitives and the crypto framework (`crypto(9)`).

pub mod aes;
pub mod blake2s;
pub mod blf;
pub mod cast;
pub mod chacha_private;
pub mod chachapoly;
pub mod criov;
#[allow(clippy::module_inception)] // crypto.c, the file, in the crypto directory
pub mod crypto;
pub mod cryptodev;
pub mod cryptosoft;
pub mod curve25519;
pub mod des_locl;
pub mod ecb3_enc;
pub mod ecb_enc;
pub mod gmac;
pub mod hmac;
pub mod md5;
pub mod podd;
pub mod poly1305;
pub mod rijndael;
pub mod rmd160;
pub mod set_key;
pub mod sha1;
pub mod sha2;
pub mod siphash;
pub mod sk;
pub mod spr;
#[cfg(test)]
pub(crate) mod testutil;
pub mod xform;
pub mod xform_ipcomp;

/// `explicit_bzero(&x, sizeof(x))` for a context that is a plain value: overwrites it with its
/// default (all zero) and keeps the compiler from proving the store dead, so the secret it held
/// does not outlive the call. The byte buffers use `libkern::explicit_bzero`.
pub fn wipe<T: Default>(x: &mut T) {
    *x = T::default();
    core::hint::black_box(&*x);
}
