//! Known-answer tests for HMAC-MD5, HMAC-SHA-1 (RFC 2202, test cases 1 to 7) and
//! HMAC-SHA-256 (RFC 4231, test cases 1 to 7); the cases are the RFCs', their expected
//! values agree with Python's `hmac`.

use super::*;
use crate::crypto::testutil::hex;

extern crate std;
use std::vec;
use std::vec::Vec;

/// The key and the data of the seven test cases of RFC 4231; RFC 2202 shares the shapes with
/// shorter keys (`keylen`: 16 for MD5, 20 for SHA-1, and 80 instead of 131 for the long ones).
fn cases(short_key: usize, long_key: usize, long_msg_7: &'static [u8]) -> Vec<(Vec<u8>, Vec<u8>)> {
    vec![
        (vec![0x0b; short_key], b"Hi There".to_vec()),
        (b"Jefe".to_vec(), b"what do ya want for nothing?".to_vec()),
        (vec![0xaa; short_key], vec![0xdd; 50]),
        ((1..=25).collect(), vec![0xcd; 50]),
        (vec![0x0c; short_key], b"Test With Truncation".to_vec()),
        (
            vec![0xaa; long_key],
            b"Test Using Larger Than Block-Size Key - Hash Key First".to_vec(),
        ),
        (vec![0xaa; long_key], long_msg_7.to_vec()),
    ]
}

fn hmac_md5(key: &[u8], data: &[u8]) -> [u8; MD5_DIGEST_LENGTH] {
    let mut ctx = HmacMd5Ctx::default();
    let mut out = [0u8; MD5_DIGEST_LENGTH];
    HMAC_MD5_Init(&mut ctx, key);
    HMAC_MD5_Update(&mut ctx, data);
    HMAC_MD5_Final(&mut out, &mut ctx);
    out
}

fn hmac_sha1(key: &[u8], data: &[u8]) -> [u8; SHA1_DIGEST_LENGTH] {
    let mut ctx = HmacSha1Ctx::default();
    let mut out = [0u8; SHA1_DIGEST_LENGTH];
    HMAC_SHA1_Init(&mut ctx, key);
    HMAC_SHA1_Update(&mut ctx, data);
    HMAC_SHA1_Final(&mut out, &mut ctx);
    out
}

fn hmac_sha256(key: &[u8], data: &[u8]) -> [u8; SHA256_DIGEST_LENGTH] {
    let mut ctx = HmacSha256Ctx::default();
    let mut out = [0u8; SHA256_DIGEST_LENGTH];
    HMAC_SHA256_Init(&mut ctx, key);
    HMAC_SHA256_Update(&mut ctx, data);
    HMAC_SHA256_Final(&mut out, &mut ctx);
    out
}

#[test]
fn rfc2202_hmac_md5() {
    let want = [
        "9294727a3638bb1c13f48ef8158bfc9d",
        "750c783e6ab0b503eaa86e310a5db738",
        "56be34521d144c88dbb8c733f0e8b3f6",
        "697eaf0aca3a3aea3a75164746ffaa79",
        "56461ef2342edc00f9bab995690efd4c",
        "6b1ab7fe4bd7bf8f0b62e6ce61b9d0cd",
        "6f630fad67cda0ee1fb1f562db3aa53e",
    ];
    let c = cases(
        16,
        80,
        b"Test Using Larger Than Block-Size Key and Larger Than One Block-Size Data",
    );
    for (i, ((key, data), want)) in c.iter().zip(want).enumerate() {
        assert_eq!(hmac_md5(key, data).to_vec(), hex(want), "case {}", i + 1);
    }
}

#[test]
fn rfc2202_hmac_sha1() {
    let want = [
        "b617318655057264e28bc0b6fb378c8ef146be00",
        "effcdf6ae5eb2fa2d27416d5f184df9c259a7c79",
        "125d7342b9ac11cd91a39af48aa17b4f63f175d3",
        "4c9007f4026250c6bc8414f9bf50c86c2d7235da",
        "4c1a03424b55e07fe7f27be1d58bb9324a9a5a04",
        "aa4ae5e15272d00e95705637ce8a3b55ed402112",
        "e8e99d0f45237d786d6bbaa7965c7808bbff1a91",
    ];
    let c = cases(
        20,
        80,
        b"Test Using Larger Than Block-Size Key and Larger Than One Block-Size Data",
    );
    for (i, ((key, data), want)) in c.iter().zip(want).enumerate() {
        assert_eq!(hmac_sha1(key, data).to_vec(), hex(want), "case {}", i + 1);
    }
}

#[test]
fn rfc4231_hmac_sha256() {
    let want = [
        "b0344c61d8db38535ca8afceaf0bf12b881dc200c9833da726e9376c2e32cff7",
        "5bdcc146bf60754e6a042426089575c75a003f089d2739839dec58b964ec3843",
        "773ea91e36800e46854db8ebd09181a72959098b3ef8c122d9635514ced565fe",
        "82558a389a443c0ea4cc819899f2083a85f0faa3e578f8077a2e3ff46729665b",
        "a3b6167473100ee06e0c796c2955552bfa6f7c0a6a8aef8b93f860aab0cd20c5",
        "60e431591ee0b67f0d8a26aacbf5b77f8e0bc6213728c5140546040f0ee37f54",
        "9b09ffa71b942fcb27635fbcd5b0e944bfdc63644f0713938a7f51535c3a35e2",
    ];
    let c = cases(
        20,
        131,
        b"This is a test using a larger than block-size key and a larger than block-size data. The key needs to be hashed before being used by the HMAC algorithm.",
    );
    for (i, ((key, data), want)) in c.iter().zip(want).enumerate() {
        assert_eq!(hmac_sha256(key, data).to_vec(), hex(want), "case {}", i + 1);
    }
}

#[test]
fn any_split_of_the_data_gives_the_same_mac() {
    let key = [0x42u8; 40];
    let data: Vec<u8> = (0..200).map(|i| (i * 3) as u8).collect();
    let whole = hmac_sha1(&key, &data);
    for chunk in [1usize, 13, 64, 65] {
        let mut ctx = HmacSha1Ctx::default();
        let mut out = [0u8; SHA1_DIGEST_LENGTH];
        HMAC_SHA1_Init(&mut ctx, &key);
        for piece in data.chunks(chunk) {
            HMAC_SHA1_Update(&mut ctx, piece);
        }
        HMAC_SHA1_Final(&mut out, &mut ctx);
        assert_eq!(out, whole, "chunks of {chunk}");
    }
}

#[test]
fn a_key_longer_than_the_block_is_hashed_first() {
    // The two are the same key to HMAC (RFC 2104): a long key is replaced by its digest.
    let long = [0x55u8; 100];
    let mut digest = [0u8; SHA1_DIGEST_LENGTH];
    let mut ctx = Sha1Ctx::default();
    SHA1Init(&mut ctx);
    SHA1Update(&mut ctx, &long);
    SHA1Final(&mut digest, &mut ctx);
    assert_eq!(hmac_sha1(&long, b"data"), hmac_sha1(&digest, b"data"));
    let mut ctx = HmacSha1Ctx::default();
    HMAC_SHA1_Init(&mut ctx, &long);
    assert_eq!(ctx.key_len as usize, SHA1_DIGEST_LENGTH);
}
