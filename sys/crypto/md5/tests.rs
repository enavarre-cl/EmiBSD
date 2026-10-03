//! Known-answer tests for MD5: the RFC 1321 appendix A.5 test suite, one million "a" and the
//! digest of the digests of every length around the block boundaries (`hashlib`).

use super::*;
use crate::crypto::testutil::hex;

extern crate std;
use std::vec::Vec;

fn digest(data: &[u8]) -> [u8; MD5_DIGEST_LENGTH] {
    let mut ctx = Md5Ctx::default();
    let mut out = [0u8; MD5_DIGEST_LENGTH];
    MD5Init(&mut ctx);
    MD5Update(&mut ctx, data);
    MD5Final(&mut out, &mut ctx);
    out
}

#[test]
fn rfc1321_test_suite() {
    let cases: &[(&[u8], &str)] = &[
        (b"", "d41d8cd98f00b204e9800998ecf8427e"),
        (b"a", "0cc175b9c0f1b6a831c399e269772661"),
        (b"abc", "900150983cd24fb0d6963f7d28e17f72"),
        (b"message digest", "f96b697d7cb7938d525a2f31aaf161d0"),
        (
            b"abcdefghijklmnopqrstuvwxyz",
            "c3fcd3d76192e4007dfb496cca67e13b",
        ),
        (
            b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789",
            "d174ab98d277d9f5a5611c2c9f419d9f",
        ),
        (
            b"12345678901234567890123456789012345678901234567890123456789012345678901234567890",
            "57edf4a22be3c955ac49da2e2107b67a",
        ),
    ];
    for (msg, want) in cases {
        assert_eq!(digest(msg).to_vec(), hex(want), "{:?}", msg);
    }
}

#[test]
fn longer_messages() {
    assert_eq!(
        digest(b"abcdbcdecdefdefgefghfghighijhijkijkljklmklmnlmnomnopnopq").to_vec(),
        hex("8215ef0796a20bcaaae116d3876c664a")
    );
    assert_eq!(
        digest(b"abcdefghbcdefghicdefghijdefghijkefghijklfghijklmghijklmnhijklmnoijklmnopjklmnopqklmnopqrlmnopqrsmnopqrstnopqrstu").to_vec(),
        hex("03dd8807a93175fb062dfb55dc7d359c")
    );
    let mut ctx = Md5Ctx::default();
    let mut out = [0u8; MD5_DIGEST_LENGTH];
    MD5Init(&mut ctx);
    let chunk = [b'a'; 1000];
    for _ in 0..1000 {
        MD5Update(&mut ctx, &chunk);
    }
    MD5Final(&mut out, &mut ctx);
    assert_eq!(out.to_vec(), hex("7707d6ae4e027c70eea2a935c2296f21"));
}

#[test]
fn every_length_around_the_blocks() {
    let mut acc: Vec<u8> = Vec::new();
    for n in 0..(3 * MD5_BLOCK_LENGTH + 2) {
        let msg: Vec<u8> = (0..n).map(|i| ((i * 7 + n) % 256) as u8).collect();
        acc.extend_from_slice(&digest(&msg));
    }
    assert_eq!(
        digest(&acc).to_vec(),
        hex("2cd1d612114900ee87a850b157a5c55e")
    );
}

#[test]
fn any_split_of_the_input_gives_the_same_digest() {
    let msg: Vec<u8> = (0..300).map(|i| (i * 13 % 256) as u8).collect();
    let whole = digest(&msg);
    for chunk in [1usize, 3, 7, 55, 56, 63, 64, 65, 128, 299] {
        let mut ctx = Md5Ctx::default();
        let mut out = [0u8; MD5_DIGEST_LENGTH];
        MD5Init(&mut ctx);
        for piece in msg.chunks(chunk) {
            MD5Update(&mut ctx, piece);
        }
        MD5Final(&mut out, &mut ctx);
        assert_eq!(out, whole, "chunks of {chunk}");
        // Final wipes the context.
        assert_eq!(ctx, Md5Ctx::default());
    }
}
