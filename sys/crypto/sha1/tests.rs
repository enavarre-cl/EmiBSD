//! Known-answer tests for SHA-1: the FIPS 180-4 examples ("abc", the 448-bit message, one
//! million "a"), and digests around the block boundaries computed with Python's `hashlib`.

use super::*;
use crate::crypto::testutil::hex;

extern crate std;
use std::vec::Vec;

fn digest(data: &[u8]) -> [u8; SHA1_DIGEST_LENGTH] {
    let mut ctx = Sha1Ctx::default();
    let mut out = [0u8; SHA1_DIGEST_LENGTH];
    SHA1Init(&mut ctx);
    SHA1Update(&mut ctx, data);
    SHA1Final(&mut out, &mut ctx);
    out
}

#[test]
fn fips_180_4_examples() {
    assert_eq!(
        digest(b"abc").to_vec(),
        hex("a9993e364706816aba3e25717850c26c9cd0d89d")
    );
    assert_eq!(
        digest(b"abcdbcdecdefdefgefghfghighijhijkijkljklmklmnlmnomnopnopq").to_vec(),
        hex("84983e441c3bd26ebaae4aa1f95129e5e54670f1")
    );
    assert_eq!(
        digest(b"abcdefghbcdefghicdefghijdefghijkefghijklfghijklmghijklmnhijklmnoijklmnopjklmnopqklmnopqrlmnopqrsmnopqrstnopqrstu").to_vec(),
        hex("a49b2446a02c645bf419f995b67091253a04a259")
    );
}

#[test]
fn one_million_a() {
    let mut ctx = Sha1Ctx::default();
    let mut out = [0u8; SHA1_DIGEST_LENGTH];
    SHA1Init(&mut ctx);
    let chunk = [b'a'; 1000];
    for _ in 0..1000 {
        SHA1Update(&mut ctx, &chunk);
    }
    SHA1Final(&mut out, &mut ctx);
    assert_eq!(
        out.to_vec(),
        hex("34aa973cd4c4daa4f61eeb2bdbad27316534016f")
    );
}

#[test]
fn short_messages() {
    let cases: &[(&[u8], &str)] = &[
        (b"", "da39a3ee5e6b4b0d3255bfef95601890afd80709"),
        (b"a", "86f7e437faa5a7fce15d1ddcb9eaeaea377667b8"),
        (
            b"message digest",
            "c12252ceda8be8994d5fa0290a47231c1d16aae3",
        ),
        (
            b"abcdefghijklmnopqrstuvwxyz",
            "32d10c7b8cf96570ca04ce37f2a19d84240d3a89",
        ),
        (
            b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789",
            "761c457bf73b14d27e9e9265c46f4b4dda11f940",
        ),
        (
            b"12345678901234567890123456789012345678901234567890123456789012345678901234567890",
            "50abf5706a150990a08b2c5ea40fa0e585554732",
        ),
    ];
    for (msg, want) in cases {
        assert_eq!(digest(msg).to_vec(), hex(want), "{:?}", msg);
    }
}

/// The digest of the digests of the messages of every length from 0 to 3 blocks + 1.
#[test]
fn every_length_around_the_blocks() {
    let mut acc: Vec<u8> = Vec::new();
    for n in 0..(3 * SHA1_BLOCK_LENGTH + 2) {
        let msg: Vec<u8> = (0..n).map(|i| ((i * 7 + n) % 256) as u8).collect();
        acc.extend_from_slice(&digest(&msg));
    }
    assert_eq!(
        digest(&acc).to_vec(),
        hex("5bc3728d76facfc871e5e0ccc48f64b392d96902")
    );
}

#[test]
fn any_split_of_the_input_gives_the_same_digest() {
    let msg: Vec<u8> = (0..300).map(|i| (i * 13 % 256) as u8).collect();
    let whole = digest(&msg);
    for chunk in [1usize, 3, 7, 55, 56, 63, 64, 65, 128, 299] {
        let mut ctx = Sha1Ctx::default();
        let mut out = [0u8; SHA1_DIGEST_LENGTH];
        SHA1Init(&mut ctx);
        for piece in msg.chunks(chunk) {
            SHA1Update(&mut ctx, piece);
        }
        SHA1Final(&mut out, &mut ctx);
        assert_eq!(out, whole, "chunks of {chunk}");
        // Final wipes the context.
        assert_eq!(ctx, Sha1Ctx::default());
    }
}
