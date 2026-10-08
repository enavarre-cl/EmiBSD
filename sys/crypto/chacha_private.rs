/*	$OpenBSD: chacha_private.h,v 1.4 2020/07/22 13:54:30 tobhe Exp $	*/
/* <LICENSES> */
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
        let mut x = ChachaCtx::default();
        // RFC: counter 1, nonce 00:00:00:09:00:00:00:4a:00:00:00:00, that is, in this layout, an
        // 8-byte counter 01 00 00 00 | 00 00 00 09 and an 8-byte IV 00 00 00 4a | 00 00 00 00.
        chacha_keysetup(&mut x, &key32(), 256);
        chacha_ivsetup(
            &mut x,
            &[0, 0, 0, 0x4a, 0, 0, 0, 0],
            Some(&[1, 0, 0, 0, 0, 0, 0, 9]),
        );
        let mut ks = [0u8; 64];
        chacha_keystream_bytes(&mut x, &mut ks);
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
        let mut x = ChachaCtx::default();
        chacha_keysetup(&mut x, &key32(), 256);
        // counter 1, nonce 00:00:00:00:00:00:00:4a:00:00:00:00
        chacha_ivsetup(
            &mut x,
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
        chacha_encrypt_bytes(&mut x, SUNSCREEN, &mut ct);
        assert_eq!(ct.to_vec(), hex(SUNSCREEN_CT));
        // 114 bytes started two blocks.
        assert_eq!(x.input[12], 3);

        // The same, in place, and decrypting back.
        let mut x = rfc8439_2_4_2_ctx();
        let mut buf = [0u8; 114];
        buf.copy_from_slice(SUNSCREEN);
        chacha_encrypt_bytes_inplace(&mut x, &mut buf);
        assert_eq!(buf.to_vec(), hex(SUNSCREEN_CT));
        let mut x = rfc8439_2_4_2_ctx();
        chacha_encrypt_bytes_inplace(&mut x, &mut buf);
        assert_eq!(buf.as_slice(), SUNSCREEN);
    }

    #[test]
    fn keystream_is_encryption_of_zeros_in_any_split() {
        let mut a = rfc8439_2_4_2_ctx();
        let mut whole = [0u8; 150];
        chacha_keystream_bytes(&mut a, &mut whole);

        let mut b = rfc8439_2_4_2_ctx();
        let mut zeros = [0u8; 150];
        chacha_encrypt_bytes_inplace(&mut b, &mut zeros);
        assert_eq!(whole, zeros);
        assert_eq!(a, b);

        // A call per block is the same stream; a partial block spends the whole block's counter.
        let mut c = rfc8439_2_4_2_ctx();
        let mut parts = [0u8; 128];
        chacha_keystream_bytes(&mut c, &mut parts[..64]);
        chacha_keystream_bytes(&mut c, &mut parts[64..]);
        assert_eq!(parts[..], whole[..128]);
        assert_eq!(c.input[12], 3);
        chacha_keystream_bytes(&mut c, &mut []);
        assert_eq!(c.input[12], 3);
    }

    #[test]
    fn hchacha20_draft_irtf_cfrg_xchacha_2_2_1() {
        let nonce: [u8; 16] = hexn("000000090000004a0000000031415927");
        let mut out = [0u32; 8];
        hchacha20(&mut out, &nonce, &key32());
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
        let mut x = ChachaCtx::default();
        chacha_keysetup(&mut x, &key32()[..16], 128);
        assert_eq!(
            &x.input[..4],
            &[0x6170_7865, 0x3120_646e, 0x7962_2d36, 0x6b20_6574]
        );
        assert_eq!(x.input[4..8], x.input[8..12]);
        // counter 5, IV 11 11 11 11 22 22 22 22
        chacha_ivsetup(
            &mut x,
            &hexn::<8>("1111111122222222"),
            Some(&[5, 0, 0, 0, 0, 0, 0, 0]),
        );
        let mut ks = [0u8; 64];
        chacha_keystream_bytes(&mut x, &mut ks);
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
        let mut x = ChachaCtx::default();
        chacha_keysetup(&mut x, &key32(), 256);
        chacha_ivsetup(
            &mut x,
            &[1, 2, 3, 4, 5, 6, 7, 8],
            Some(&[0xff, 0xff, 0xff, 0xff, 0, 0, 0, 0]),
        );
        let mut ks = [0u8; 128];
        chacha_keystream_bytes(&mut x, &mut ks);
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
}
/* </TESTS> */
