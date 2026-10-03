/*	$OpenBSD: chacha_private.h,v 1.4 2020/07/22 13:54:30 tobhe Exp $	*/
/* <LICENSES> */
/*
chacha-merged.c version 20080118
D. J. Bernstein
Public domain.
*/
/* </LICENSES> */

//! ChaCha, D. J. Bernstein's stream cipher, in its "merged" reference form: the context, the
//! key and IV setup, the keystream/encrypt loop, and `hchacha20` (the key derivation XChaCha20
//! is built on). `chachapoly.c` and `dev/rnd.c` both include the header.
//!
//! Upstream: sys/crypto/chacha_private.h @ 3ce1f3f79392
//!
//! ## Deviations
//! - The header's functions are `static` and included by each user; here they are `pub` in one
//!   module. `dev/rnd.c` defines `KEYSTREAM_ONLY` before the include, which drops the XOR with
//!   the message: that variant is [`chacha_keystream_bytes`], the XORing one is
//!   [`chacha_encrypt_bytes`].
//! - `chacha_encrypt_bytes` takes the message and the output as two slices of one length (the C
//!   takes two pointers and a count, and allows them to be the same buffer);
//!   [`chacha_encrypt_bytes_inplace`] is the `m == c` call.
//! - The key, IV and counter are slices of exactly the bytes the C reads; a short slice is a
//!   caller bug and indexes out of range.
//! - `hchacha20` returns its eight words by `&mut [u32; 8]` as the C does; the callers store
//!   them little-endian.

/// `chacha_ctx`: the sixteen-word state (constants, key, counter, IV).
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct ChachaCtx {
    /// `input`: the state words.
    pub input: [u32; 16],
}

const SIGMA: &[u8; 16] = b"expand 32-byte k";
const TAU: &[u8; 16] = b"expand 16-byte k";

/// `U8TO32_LITTLE`: the little-endian word at the start of `p`.
#[inline]
fn u8to32_little(p: &[u8]) -> u32 {
    u32::from_le_bytes([p[0], p[1], p[2], p[3]])
}

/// `QUARTERROUND`.
#[inline(always)]
fn quarterround(x: &mut [u32; 16], a: usize, b: usize, c: usize, d: usize) {
    x[a] = x[a].wrapping_add(x[b]);
    x[d] = (x[d] ^ x[a]).rotate_left(16);
    x[c] = x[c].wrapping_add(x[d]);
    x[b] = (x[b] ^ x[c]).rotate_left(12);
    x[a] = x[a].wrapping_add(x[b]);
    x[d] = (x[d] ^ x[a]).rotate_left(8);
    x[c] = x[c].wrapping_add(x[d]);
    x[b] = (x[b] ^ x[c]).rotate_left(7);
}

/// Twenty rounds: ten double rounds, columns then diagonals.
fn chacha_rounds(x: &mut [u32; 16]) {
    for _ in 0..10 {
        quarterround(x, 0, 4, 8, 12);
        quarterround(x, 1, 5, 9, 13);
        quarterround(x, 2, 6, 10, 14);
        quarterround(x, 3, 7, 11, 15);
        quarterround(x, 0, 5, 10, 15);
        quarterround(x, 1, 6, 11, 12);
        quarterround(x, 2, 7, 8, 13);
        quarterround(x, 3, 4, 9, 14);
    }
}

/// `hchacha20`: the first and last rows of the state after twenty rounds, keyed by `key` with
/// the 16-byte `nonce` in the counter and IV words, with no final addition of the input.
pub fn hchacha20(derived_key: &mut [u32; 8], nonce: &[u8; 16], key: &[u8; 32]) {
    let mut x = [0u32; 16];

    for i in 0..4 {
        x[i] = u8to32_little(&SIGMA[4 * i..]);
        x[12 + i] = u8to32_little(&nonce[4 * i..]);
    }
    for i in 0..8 {
        x[4 + i] = u8to32_little(&key[4 * i..]);
    }

    chacha_rounds(&mut x);

    derived_key[..4].copy_from_slice(&x[..4]);
    derived_key[4..].copy_from_slice(&x[12..]);
}

/// `chacha_keysetup`: the constants and the key; `kbits` is 256 (recommended) or 128, which
/// reads 16 bytes of `k` and uses it for both halves of the key.
pub fn chacha_keysetup(x: &mut ChachaCtx, k: &[u8], kbits: u32) {
    x.input[4] = u8to32_little(&k[0..]);
    x.input[5] = u8to32_little(&k[4..]);
    x.input[6] = u8to32_little(&k[8..]);
    x.input[7] = u8to32_little(&k[12..]);
    let (k, constants) = if kbits == 256 {
        (&k[16..], SIGMA)
    } else {
        // kbits == 128
        (k, TAU)
    };
    x.input[8] = u8to32_little(&k[0..]);
    x.input[9] = u8to32_little(&k[4..]);
    x.input[10] = u8to32_little(&k[8..]);
    x.input[11] = u8to32_little(&k[12..]);
    x.input[0] = u8to32_little(&constants[0..]);
    x.input[1] = u8to32_little(&constants[4..]);
    x.input[2] = u8to32_little(&constants[8..]);
    x.input[3] = u8to32_little(&constants[12..]);
}

/// `chacha_ivsetup`: the 8-byte IV and the 8-byte block counter (`None` is counter zero).
pub fn chacha_ivsetup(x: &mut ChachaCtx, iv: &[u8], counter: Option<&[u8]>) {
    x.input[12] = counter.map_or(0, |c| u8to32_little(&c[0..]));
    x.input[13] = counter.map_or(0, |c| u8to32_little(&c[4..]));
    x.input[14] = u8to32_little(&iv[0..]);
    x.input[15] = u8to32_little(&iv[4..]);
}

/// One block of keystream for the state in `j`, advancing its 64-bit counter (words 12, 13).
fn chacha_block(j: &mut [u32; 16]) -> [u8; 64] {
    let mut x = *j;
    chacha_rounds(&mut x);

    let mut out = [0u8; 64];
    for i in 0..16 {
        out[4 * i..4 * i + 4].copy_from_slice(&x[i].wrapping_add(j[i]).to_le_bytes());
    }

    j[12] = j[12].wrapping_add(1);
    if j[12] == 0 {
        j[13] = j[13].wrapping_add(1);
        // stopping at 2^70 bytes per nonce is user's responsibility
    }
    out
}

/// `chacha_encrypt_bytes`: XORs the keystream into `m`, writing `c`, and advances the counter
/// by one per 64 bytes started. `m` and `c` must be the same length.
pub fn chacha_encrypt_bytes(x: &mut ChachaCtx, m: &[u8], c: &mut [u8]) {
    let mut j = x.input;
    for (mc, cc) in m.chunks(64).zip(c.chunks_mut(64)) {
        let ks = chacha_block(&mut j);
        for i in 0..mc.len() {
            cc[i] = mc[i] ^ ks[i];
        }
    }
    x.input[12] = j[12];
    x.input[13] = j[13];
}

/// `chacha_encrypt_bytes` with `m` and `c` the same buffer, which the C permits.
pub fn chacha_encrypt_bytes_inplace(x: &mut ChachaCtx, data: &mut [u8]) {
    let mut j = x.input;
    for chunk in data.chunks_mut(64) {
        let ks = chacha_block(&mut j);
        for i in 0..chunk.len() {
            chunk[i] ^= ks[i];
        }
    }
    x.input[12] = j[12];
    x.input[13] = j[13];
}

/// `chacha_encrypt_bytes` as `KEYSTREAM_ONLY` compiles it (`dev/rnd.c`): the keystream itself
/// is written to `c`, whatever it held.
pub fn chacha_keystream_bytes(x: &mut ChachaCtx, c: &mut [u8]) {
    let mut j = x.input;
    for chunk in c.chunks_mut(64) {
        let ks = chacha_block(&mut j);
        chunk.copy_from_slice(&ks[..chunk.len()]);
    }
    x.input[12] = j[12];
    x.input[13] = j[13];
}

#[cfg(test)]
mod tests;
