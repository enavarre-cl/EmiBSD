//! Known-answer tests for GHASH and the GMAC context: the Galois/Counter Mode test cases 1 to
//! 4 of McGrew and Viega's specification (and its 192 and 256-bit key cases 10 and 16), run
//! the way `swcr_authenc` drives the authenticator: salt and key together, the IV, the
//! additional data, the ciphertext, the length block. GMAC-only tags (AAD with no ciphertext,
//! RFC 4543) come from an independent big-integer GHASH over `openssl`'s AES.

use super::*;
use crate::crypto::testutil::{hex, hexn};

extern crate std;
use std::vec::Vec;

/// Authenticates `aad` and `ct` under `key` and the 12-byte `iv` (4 of salt, 8 of IV): the
/// tag the way the software crypto driver computes it.
fn tag(key: &[u8], iv: &[u8], aad: &[u8], ct: &[u8]) -> [u8; GMAC_DIGEST_LEN] {
    let mut ctx = AesGmacCtx::default();
    let mut material = key.to_vec();
    material.extend_from_slice(&iv[..4]);

    AES_GMAC_Init(&mut ctx);
    assert_eq!(AES_GMAC_Setkey(&mut ctx, &material), Ok(()));
    AES_GMAC_Reinit(&mut ctx, &iv[4..]);
    assert_eq!(AES_GMAC_Update(&mut ctx, aad), Ok(()));
    assert_eq!(AES_GMAC_Update(&mut ctx, ct), Ok(()));

    // The length block: the bit lengths, big-endian, in the low word of each half.
    let mut blk = [0u8; GMAC_BLOCK_LEN];
    blk[4..8].copy_from_slice(&((aad.len() * 8) as u32).to_be_bytes());
    blk[12..16].copy_from_slice(&((ct.len() * 8) as u32).to_be_bytes());
    assert_eq!(AES_GMAC_Update(&mut ctx, &blk), Ok(()));

    let mut out = [0u8; GMAC_DIGEST_LEN];
    AES_GMAC_Final(&mut out, &mut ctx);
    out
}

fn k128() -> Vec<u8> {
    hex("feffe9928665731c6d6a8f9467308308")
}

fn iv() -> Vec<u8> {
    hex("cafebabefacedbaddecaf888")
}

fn a() -> Vec<u8> {
    hex("feedfacedeadbeeffeedfacedeadbeefabaddad2")
}

#[test]
fn gcm_spec_test_case_1() {
    // Zero key, no data: the tag is the encrypted counter block alone (the hash is zero).
    assert_eq!(
        tag(&[0; 16], &[0; 12], &[], &[]).to_vec(),
        hex("58e2fccefa7e3061367f1d57a4e7455a")
    );
}

#[test]
fn gcm_spec_test_case_2() {
    let ct = hex("0388dace60b6a392f328c2b971b2fe78");
    assert_eq!(
        tag(&[0; 16], &[0; 12], &[], &ct).to_vec(),
        hex("ab6e47d42cec13bdf53a67b21257bddf")
    );
}

#[test]
fn gcm_spec_test_case_3() {
    let ct = hex(
        "42831ec2217774244b7221b784d0d49ce3aa212f2c02a4e035c17e2329aca12e
                  21d514b25466931c7d8f6a5aac84aa051ba30b396a0aac973d58e091473f5985",
    );
    assert_eq!(
        tag(&k128(), &iv(), &[], &ct).to_vec(),
        hex("4d5c2af327cd64a62cf35abd2ba6fab4")
    );
}

#[test]
fn gcm_spec_test_case_4_with_additional_data_and_a_partial_block() {
    let ct = hex(
        "42831ec2217774244b7221b784d0d49ce3aa212f2c02a4e035c17e2329aca12e
                  21d514b25466931c7d8f6a5aac84aa051ba30b396a0aac973d58e091",
    );
    assert_eq!(ct.len(), 60);
    assert_eq!(
        tag(&k128(), &iv(), &a(), &ct).to_vec(),
        hex("5bc94fbc3221a5db94fae95ae7121a47")
    );
}

#[test]
fn gcm_spec_test_cases_10_and_16_with_192_and_256_bit_keys() {
    let k192 = hex("feffe9928665731c6d6a8f9467308308feffe9928665731c");
    let ct = hex(
        "3980ca0b3c00e841eb06fac4872a2757859e1ceaa6efd984628593b40ca1e19c
                  7d773d00c144c525ac619d18c84a3f4718e2448b2fe324d9ccda2710",
    );
    assert_eq!(
        tag(&k192, &iv(), &a(), &ct).to_vec(),
        hex("2519498e80f1478f37ba55bd6d27618c")
    );

    let mut k256 = k128();
    k256.extend_from_slice(&k128());
    let ct = hex(
        "522dc1f099567d07f47f37a32a84427d643a8cdcbfe5c0c97598a2bd2555d1aa
                  8cb08e48590dbb3da7b08b1056828838c5f61e6393ba7a0abcc9f662",
    );
    assert_eq!(
        tag(&k256, &iv(), &a(), &ct).to_vec(),
        hex("76fc6ece0f4e1768cddf8853bb2d551b")
    );
}

#[test]
fn gmac_only_tags() {
    let k192 = hex("feffe9928665731c6d6a8f9467308308feffe9928665731c");
    let mut k256 = k128();
    k256.extend_from_slice(&k128());
    assert_eq!(
        tag(&k128(), &iv(), &a(), &[]).to_vec(),
        hex("346434fd51d5cd0c5887ec63e39b907a")
    );
    assert_eq!(
        tag(&k192, &iv(), &a(), &[]).to_vec(),
        hex("c8253387e5f78673d538a60d50527a92")
    );
    assert_eq!(
        tag(&k256, &iv(), &a(), &[]).to_vec(),
        hex("9f6be07603c0b0bd1272854063e9c9ba")
    );
}

#[test]
fn ghash_multiplication() {
    // x * 1, where the field's one is the bit pattern 0x80 0 0 ..., is x.
    let mut one = [0u8; 16];
    one[0] = 0x80;
    let x: [u8; 16] = hexn("0388dace60b6a392f328c2b971b2fe78");
    let mut out = [0u8; 16];
    ghash_gfmul(&x, &one, &mut out);
    assert_eq!(out, x);
    ghash_gfmul(&one, &x, &mut out);
    assert_eq!(out, x);
    // And 0 * x is 0; the product commutes.
    ghash_gfmul(&[0; 16], &x, &mut out);
    assert_eq!(out, [0; 16]);
    let y: [u8; 16] = hexn("66e94bd4ef8a2c3b884cfa59ca342b2e");
    let (mut xy, mut yx) = ([0u8; 16], [0u8; 16]);
    ghash_gfmul(&x, &y, &mut xy);
    ghash_gfmul(&y, &x, &mut yx);
    assert_eq!(xy, yx);
    // The test case 2 hash: H = E(K, 0) = 66e94bd4ef8a2c3b884cfa59ca342b2e and the ciphertext
    // block c: GHASH(c || len) = c*H^2 + len*H; the first step c * H is
    // 5e2ec746917062882c85b0685353deb7 (the intermediate value of the spec's table).
    ghash_gfmul(&hexn("0388dace60b6a392f328c2b971b2fe78"), &y, &mut out);
    assert_eq!(out.to_vec(), hex("5e2ec746917062882c85b0685353deb7"));
}

#[test]
fn bad_key_sizes_are_rejected() {
    let mut ctx = AesGmacCtx::default();
    AES_GMAC_Init(&mut ctx);
    // 4 bytes of salt after an AES key of 16, 24 or 32: the other lengths are errors.
    for len in [0usize, 3, 4, 19, 21, 27, 31, 37, 40] {
        assert_eq!(
            AES_GMAC_Setkey(&mut ctx, &std::vec![0u8; len]),
            Err(Errno::EINVAL),
            "{len}"
        );
    }
    assert_eq!(AES_GMAC_Setkey(&mut ctx, &[0u8; 20]), Ok(()));
}
