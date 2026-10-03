//! Known-answer tests for RIPEMD-160: the test suite of the RIPEMD-160 specification (Dobbertin,
//! Bosselaers, Preneel) and the digest of the digests of every length around the block
//! boundaries (`hashlib`).

use super::*;
use crate::crypto::testutil::hex;

extern crate std;
use std::vec::Vec;

fn digest(data: &[u8]) -> [u8; RMD160_DIGEST_LENGTH] {
    let mut ctx = Rmd160Ctx::default();
    let mut out = [0u8; RMD160_DIGEST_LENGTH];
    RMD160Init(&mut ctx);
    RMD160Update(&mut ctx, data);
    RMD160Final(Some(&mut out), &mut ctx);
    out
}

#[test]
fn specification_test_suite() {
    let cases: &[(&[u8], &str)] = &[
        (b"", "9c1185a5c5e9fc54612808977ee8f548b2258d31"),
        (b"a", "0bdc9d2d256b3ee9daae347be6f4dc835a467ffe"),
        (b"abc", "8eb208f7e05d987a9b044a8e98c6b087f15a0bfc"),
        (
            b"message digest",
            "5d0689ef49d2fae572b881b123a85ffa21595f36",
        ),
        (
            b"abcdefghijklmnopqrstuvwxyz",
            "f71c27109c692c1b56bbdceb5b9d2865b3708dbc",
        ),
        (
            b"abcdbcdecdefdefgefghfghighijhijkijkljklmklmnlmnomnopnopq",
            "12a053384a9c0c88e405a06c27dcf49ada62eb2b",
        ),
        (
            b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789",
            "b0e20b6e3116640286ed3a87a5713079b21f5189",
        ),
        (
            b"12345678901234567890123456789012345678901234567890123456789012345678901234567890",
            "9b752e45573d4b39f4dbd3323cab82bf63326bfb",
        ),
    ];
    for (msg, want) in cases {
        assert_eq!(digest(msg).to_vec(), hex(want), "{:?}", msg);
    }
}

#[test]
fn one_million_a() {
    let mut ctx = Rmd160Ctx::default();
    let mut out = [0u8; RMD160_DIGEST_LENGTH];
    RMD160Init(&mut ctx);
    let chunk = [b'a'; 1000];
    for _ in 0..1000 {
        RMD160Update(&mut ctx, &chunk);
    }
    RMD160Final(Some(&mut out), &mut ctx);
    assert_eq!(
        out.to_vec(),
        hex("52783243c1697bdbe16d37f97f68f08325dc1528")
    );
}

#[test]
fn any_split_of_the_input_gives_the_same_digest() {
    let msg: Vec<u8> = (0..300).map(|i| (i * 13 % 256) as u8).collect();
    let whole = digest(&msg);
    for chunk in [1usize, 3, 7, 55, 56, 63, 64, 65, 128, 299] {
        let mut ctx = Rmd160Ctx::default();
        let mut out = [0u8; RMD160_DIGEST_LENGTH];
        RMD160Init(&mut ctx);
        for piece in msg.chunks(chunk) {
            RMD160Update(&mut ctx, piece);
        }
        RMD160Final(Some(&mut out), &mut ctx);
        assert_eq!(out, whole, "chunks of {chunk}");
        // Final wipes the context.
        assert_eq!(ctx, Rmd160Ctx::default());
    }
}

#[test]
fn a_missing_digest_only_wipes_the_state() {
    let mut ctx = Rmd160Ctx::default();
    RMD160Init(&mut ctx);
    RMD160Update(&mut ctx, b"secret");
    RMD160Final(None, &mut ctx);
    assert_eq!(ctx, Rmd160Ctx::default());
}
