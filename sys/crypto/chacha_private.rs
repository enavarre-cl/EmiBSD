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
chacha-merged.c version 20080118
D. J. Bernstein
Public domain.
*/
/* </LICENSES> */

/* <CODE> */
//! ChaCha, D. J. Bernstein's stream cipher, in its "merged" reference form: the context, the
//! key and IV setup, the keystream/encrypt loop, and `hchacha20` (the key derivation XChaCha20
//! is built on). `chachapoly.c` and `dev/rnd.c` both include the header.
//!
//! Upstream: sys/crypto/chacha_private.h @ 3ce1f3f79392
//! LZ: sys/crypto/chacha_private.rs@f5985f1d055a
//!
//! ## Deviations
//! - The header's functions are `static` and included by each user; here they are `pub` in one
//!   module. `dev/rnd.c` defines `KEYSTREAM_ONLY` before the include, which drops the XOR with
//!   the message: that variant is [`ChachaCtx::keystream_bytes`], the XORing one is
//!   [`ChachaCtx::encrypt_bytes`].
//! - `encrypt_bytes` takes the message and the output as two slices of one length (the C takes
//!   two pointers and a count, and allows them to be the same buffer);
//!   [`ChachaCtx::encrypt_bytes_inplace`] is the `m == c` call.
//!
//! ## Redesign
//! - The functions over a `chacha_ctx` are methods of [`ChachaCtx`] (LZ: free functions with
//!   the context first). `chacha_keysetup` is split by its `kbits` argument into the
//!   constructors [`ChachaCtx::new`] (256-bit key, `sigma`) and [`ChachaCtx::new_128`]
//!   (128-bit key used twice, `tau`), which take the key as an array of exactly the bytes the
//!   C reads (LZ: a slice and `kbits`, any value other than 256 meaning 128). A fresh context
//!   has a zero counter and IV, which is what every C caller's context holds when it is keyed.
//! - `chacha_ivsetup` is [`ChachaCtx::ivsetup`], with the IV and counter as `&[u8; 8]`;
//!   `hchacha20` returns its eight words (LZ: an `&mut [u32; 8]` out parameter).
//! - [`ChachaCtx`] zeroes its state (key words included) when dropped (`docs/IDIOMS.md`), so it
//!   is no longer `Copy` or `PartialEq`; `chachapoly.c`'s `explicit_bzero` of its contexts
//!   happens as they drop.
//! - Constant time: the rounds are additions, xors and fixed rotations on the state words; the
//!   loops run over the round count and the message length (public), as in the C.

use super::wipe;

/// `chacha_ctx`: the sixteen-word state (constants, key, counter, IV). Zeroed when dropped.
#[derive(Clone, Debug, Default)]
pub struct ChachaCtx {
    /// `input`: the state words.
    pub input: [u32; 16],
}

impl Drop for ChachaCtx {
    /// Wipes the state (`docs/IDIOMS.md`: a key schedule is zeroed when dropped).
    fn drop(&mut self) {
        self.zeroize();
    }
}

const SIGMA: &[u8; 16] = b"expand 32-byte k";
const TAU: &[u8; 16] = b"expand 16-byte k";

impl ChachaCtx {
    /// Zeroes the state, with stores the compiler keeps (`crate::crypto::wipe`).
    pub(crate) fn zeroize(&mut self) {
        self.input.iter_mut().for_each(wipe);
    }

    /// `chacha_keysetup(x, k, 256)`: a context with the constants and the 32-byte key, the
    /// counter and IV zero.
    pub fn new(k: &[u8; 32]) -> Self {
        let mut x = Self::default();
        load_words(&mut x.input[..4], SIGMA);
        load_words(&mut x.input[4..12], k);
        x
    }

    /// `chacha_keysetup(x, k, 128)`: a context with the constants and the 16-byte key, used
    /// for both halves of the key words, the counter and IV zero.
    pub fn new_128(k: &[u8; 16]) -> Self {
        let mut x = Self::default();
        load_words(&mut x.input[..4], TAU);
        load_words(&mut x.input[4..8], k);
        load_words(&mut x.input[8..12], k);
        x
    }

    /// `chacha_ivsetup`: the 8-byte IV and the 8-byte block counter (`None` is counter zero).
    pub fn ivsetup(&mut self, iv: &[u8; 8], counter: Option<&[u8; 8]>) {
        load_words(&mut self.input[12..14], counter.unwrap_or(&[0; 8]));
        load_words(&mut self.input[14..16], iv);
    }

    /// `chacha_encrypt_bytes`: XORs the keystream into `m`, writing `c`, and advances the
    /// counter by one per 64 bytes started. `m` and `c` must be the same length.
    pub fn encrypt_bytes(&mut self, m: &[u8], c: &mut [u8]) {
        let mut j = self.input;
        for (mc, cc) in m.chunks(64).zip(c.chunks_mut(64)) {
            let ks = chacha_block(&mut j);
            for ((c, m), k) in cc.iter_mut().zip(mc).zip(ks) {
                *c = m ^ k;
            }
        }
        self.input[12] = j[12];
        self.input[13] = j[13];
    }

    /// `chacha_encrypt_bytes` with `m` and `c` the same buffer, which the C permits.
    pub fn encrypt_bytes_inplace(&mut self, data: &mut [u8]) {
        let mut j = self.input;
        for chunk in data.chunks_mut(64) {
            let ks = chacha_block(&mut j);
            for (d, k) in chunk.iter_mut().zip(ks) {
                *d ^= k;
            }
        }
        self.input[12] = j[12];
        self.input[13] = j[13];
    }

    /// `chacha_encrypt_bytes` as `KEYSTREAM_ONLY` compiles it (`dev/rnd.c`): the keystream
    /// itself is written to `c`, whatever it held.
    pub fn keystream_bytes(&mut self, c: &mut [u8]) {
        let mut j = self.input;
        for chunk in c.chunks_mut(64) {
            let ks = chacha_block(&mut j);
            chunk.copy_from_slice(&ks[..chunk.len()]);
        }
        self.input[12] = j[12];
        self.input[13] = j[13];
    }
}

/// `U8TO32_LITTLE` over a run of words: `words[i]` is the little-endian word at `4 * i` of
/// `bytes`.
fn load_words(words: &mut [u32], bytes: &[u8]) {
    for (w, b) in words.iter_mut().zip(bytes.as_chunks::<4>().0) {
        *w = u32::from_le_bytes(*b);
    }
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
pub fn hchacha20(nonce: &[u8; 16], key: &[u8; 32]) -> [u32; 8] {
    let mut x = [0u32; 16];

    load_words(&mut x[..4], SIGMA);
    load_words(&mut x[4..12], key);
    load_words(&mut x[12..], nonce);

    chacha_rounds(&mut x);

    let mut derived_key = [0u32; 8];
    derived_key[..4].copy_from_slice(&x[..4]);
    derived_key[4..].copy_from_slice(&x[12..]);
    derived_key
}

/// One block of keystream for the state in `j`, advancing its 64-bit counter (words 12, 13).
fn chacha_block(j: &mut [u32; 16]) -> [u8; 64] {
    let mut x = *j;
    chacha_rounds(&mut x);

    let mut out = [0u8; 64];
    for ((o, x), j) in out.as_chunks_mut::<4>().0.iter_mut().zip(&x).zip(j.iter()) {
        *o = x.wrapping_add(*j).to_le_bytes();
    }

    j[12] = j[12].wrapping_add(1);
    if j[12] == 0 {
        j[13] = j[13].wrapping_add(1);
        // stopping at 2^70 bytes per nonce is user's responsibility
    }
    out
}
/* </CODE> */

/* <TESTS> */
#[cfg(test)]
mod tests {
    // Known-answer tests for ChaCha20 and HChaCha20: RFC 8439 sections 2.3.2 and 2.4.2,
    // draft-irtf-cfrg-xchacha section 2.2.1, and vectors computed with an independent reference
    // for the 128-bit key and the 64-bit counter carry.

    use super::*;
    use crate::crypto::testutil::{hex, hexn};

    /// Key `00 01 .. 1f`.
    fn key32() -> [u8; 32] {
        let mut k = [0u8; 32];
        for (i, b) in k.iter_mut().enumerate() {
            *b = i as u8;
        }
        k
    }

    const SUNSCREEN: &[u8] = b"Ladies and Gentlemen of the class of '99: If I could offer you only one tip for the future, sunscreen would be it.";

    #[test]
    fn rfc8439_2_3_2_block_function() {
        // RFC: counter 1, nonce 00:00:00:09:00:00:00:4a:00:00:00:00, that is, in this layout, an
        // 8-byte counter 01 00 00 00 | 00 00 00 09 and an 8-byte IV 00 00 00 4a | 00 00 00 00.
        let mut x = ChachaCtx::new(&key32());
        x.ivsetup(
            &[0, 0, 0, 0x4a, 0, 0, 0, 0],
            Some(&[1, 0, 0, 0, 0, 0, 0, 9]),
        );
        let mut ks = [0u8; 64];
        x.keystream_bytes(&mut ks);
        assert_eq!(
            ks.to_vec(),
            hex(
                "10f1e7e4d13b5915500fdd1fa32071c4c7d1f4c733c068030422aa9ac3d46c4e
             d2826446079faa0914c2d705d98b02a2b5129cd1de164eb9cbd083e8a2503c4e"
            )
        );
        // The counter moved on by one block.
        assert_eq!(x.input[12], 2);
        assert_eq!(x.input[13], 0x0900_0000);
    }

    fn rfc8439_2_4_2_ctx() -> ChachaCtx {
        let mut x = ChachaCtx::new(&key32());
        // counter 1, nonce 00:00:00:00:00:00:00:4a:00:00:00:00
        x.ivsetup(
            &[0, 0, 0, 0x4a, 0, 0, 0, 0],
            Some(&[1, 0, 0, 0, 0, 0, 0, 0]),
        );
        x
    }

    const SUNSCREEN_CT: &str = "6e2e359a2568f98041ba0728dd0d6981e97e7aec1d4360c20a27afccfd9fae0b
    f91b65c5524733ab8f593dabcd62b3571639d624e65152ab8f530c359f0861d8
    07ca0dbf500d6a6156a38e088a22b65e52bc514d16ccf806818ce91ab7793736
    5af90bbf74a35be6b40b8eedf2785e42874d";

    #[test]
    fn rfc8439_2_4_2_encryption() {
        let mut x = rfc8439_2_4_2_ctx();
        let mut ct = [0u8; 114];
        x.encrypt_bytes(SUNSCREEN, &mut ct);
        assert_eq!(ct.to_vec(), hex(SUNSCREEN_CT));
        // 114 bytes started two blocks.
        assert_eq!(x.input[12], 3);

        // The same, in place, and decrypting back.
        let mut x = rfc8439_2_4_2_ctx();
        let mut buf = [0u8; 114];
        buf.copy_from_slice(SUNSCREEN);
        x.encrypt_bytes_inplace(&mut buf);
        assert_eq!(buf.to_vec(), hex(SUNSCREEN_CT));
        let mut x = rfc8439_2_4_2_ctx();
        x.encrypt_bytes_inplace(&mut buf);
        assert_eq!(buf.as_slice(), SUNSCREEN);
    }

    #[test]
    fn keystream_is_encryption_of_zeros_in_any_split() {
        let mut a = rfc8439_2_4_2_ctx();
        let mut whole = [0u8; 150];
        a.keystream_bytes(&mut whole);

        let mut b = rfc8439_2_4_2_ctx();
        let mut zeros = [0u8; 150];
        b.encrypt_bytes_inplace(&mut zeros);
        assert_eq!(whole, zeros);
        assert_eq!(a.input, b.input);

        // A call per block is the same stream; a partial block spends the whole block's counter.
        let mut c = rfc8439_2_4_2_ctx();
        let mut parts = [0u8; 128];
        c.keystream_bytes(&mut parts[..64]);
        c.keystream_bytes(&mut parts[64..]);
        assert_eq!(parts[..], whole[..128]);
        assert_eq!(c.input[12], 3);
        c.keystream_bytes(&mut []);
        assert_eq!(c.input[12], 3);
    }

    #[test]
    fn hchacha20_draft_irtf_cfrg_xchacha_2_2_1() {
        let nonce: [u8; 16] = hexn("000000090000004a0000000031415927");
        let out = hchacha20(&nonce, &key32());
        assert_eq!(
            out,
            [
                0x423b4182, 0xfe7bb227, 0x50420ed3, 0x737d878a, 0xd5e4f9a0, 0x53a8748a, 0x13c42ec1,
                0xdcecd326
            ]
        );
        let mut bytes = [0u8; 32];
        for (i, w) in out.iter().enumerate() {
            bytes[4 * i..4 * i + 4].copy_from_slice(&w.to_le_bytes());
        }
        assert_eq!(
            bytes.to_vec(),
            hex("82413b4227b27bfed30e42508a877d73a0f9e4d58a74a853c12ec41326d3ecdc")
        );
    }

    #[test]
    fn key_128_uses_tau_and_repeats_the_key() {
        let mut k16 = [0u8; 16];
        k16.copy_from_slice(&key32()[..16]);
        let mut x = ChachaCtx::new_128(&k16);
        assert_eq!(
            &x.input[..4],
            &[0x6170_7865, 0x3120_646e, 0x7962_2d36, 0x6b20_6574]
        );
        assert_eq!(x.input[4..8], x.input[8..12]);
        // counter 5, IV 11 11 11 11 22 22 22 22
        x.ivsetup(
            &hexn::<8>("1111111122222222"),
            Some(&[5, 0, 0, 0, 0, 0, 0, 0]),
        );
        let mut ks = [0u8; 64];
        x.keystream_bytes(&mut ks);
        assert_eq!(
            ks.to_vec(),
            hex(
                "b71c8ffaded961029736107779034e5c354468322a0aac6911a8eab739478fcc
             8d97174c5f014417bb111814e7effb306ae55734c51173c4a976fcc3f7ade46e"
            )
        );
    }

    #[test]
    fn counter_carries_into_the_high_word() {
        let mut x = ChachaCtx::new(&key32());
        x.ivsetup(
            &[1, 2, 3, 4, 5, 6, 7, 8],
            Some(&[0xff, 0xff, 0xff, 0xff, 0, 0, 0, 0]),
        );
        let mut ks = [0u8; 128];
        x.keystream_bytes(&mut ks);
        assert_eq!(
            ks.to_vec(),
            hex(
                "3b6550a12f42a6bc3c696dfa385e898f5db8bb3d08902ae6a37d320cf856254c
             28bf3490780956d9131f7b5b0d4005a5f1264332bbf464b45fcc4bcb6d5f6c43
             04220a5961510e72677e0d3339946e4f9592160ac17cef9e822009b7d5488b50
             c2a0fcefdb8209f9443b3ed9d85308cf1d546c9f08b31b81e9ad5cd8f5a039ee"
            )
        );
        assert_eq!((x.input[12], x.input[13]), (1, 1));
    }

    /// xorshift64: the property tests' generator.
    fn next(state: &mut u64) -> u64 {
        *state ^= *state << 13;
        *state ^= *state >> 7;
        *state ^= *state << 17;
        *state
    }

    #[test]
    fn any_split_encrypts_like_one_call_and_decrypts_back() {
        // A message cut at block boundaries into several calls is the one-call ciphertext, and
        // the same context set up again decrypts it.
        let mut st = 0x1357_9bdf_0246_8aceu64;
        for _ in 0..50 {
            let mut key = [0u8; 32];
            key.iter_mut().for_each(|b| *b = next(&mut st) as u8);
            let iv = next(&mut st).to_le_bytes();
            let len = (next(&mut st) % 300) as usize;
            let msg: std::vec::Vec<u8> = (0..len).map(|_| next(&mut st) as u8).collect();
            let fresh = || {
                let mut x = ChachaCtx::new(&key);
                x.ivsetup(&iv, None);
                x
            };

            let mut whole = std::vec![0u8; len];
            fresh().encrypt_bytes(&msg, &mut whole);

            let cut = 64 * (next(&mut st) as usize % (len / 64 + 1));
            let mut x = fresh();
            let mut parts = msg.clone();
            x.encrypt_bytes_inplace(&mut parts[..cut]);
            x.encrypt_bytes_inplace(&mut parts[cut..]);
            assert_eq!(parts, whole, "cut at {cut} of {len}");

            fresh().encrypt_bytes_inplace(&mut parts);
            assert_eq!(parts, msg);
        }
    }

    #[test]
    fn zeroize_clears_the_state() {
        // What `Drop` runs.
        let mut x = ChachaCtx::new(&key32());
        x.zeroize();
        assert_eq!(x.input, [0; 16]);
    }
}
/* </TESTS> */
