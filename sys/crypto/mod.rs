//! `sys/crypto`: the kernel's cryptographic primitives. Only what ported code calls is here.

pub mod blake2s;
pub mod chacha_private;
pub mod chachapoly;
pub mod curve25519;
pub mod poly1305;
pub mod siphash;
#[cfg(test)]
mod testutil;

/// `explicit_bzero(&x, sizeof(x))` for a context that is a plain value: overwrites it with its
/// default (all zero) and keeps the compiler from proving the store dead, so the secret it held
/// does not outlive the call. The byte buffers use `libkern::explicit_bzero`.
pub fn wipe<T: Default>(x: &mut T) {
    *x = T::default();
    core::hint::black_box(&*x);
}
