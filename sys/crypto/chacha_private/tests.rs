//! Known-answer tests for ChaCha20 and HChaCha20: RFC 8439 sections 2.3.2 and 2.4.2,
//! draft-irtf-cfrg-xchacha section 2.2.1, and vectors computed with an independent reference
//! for the 128-bit key and the 64-bit counter carry.

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
