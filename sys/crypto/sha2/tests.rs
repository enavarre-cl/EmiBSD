//! Known-answer tests for SHA-256, SHA-384 and SHA-512: the FIPS 180-4 examples ("abc", the
//! 448-bit and 896-bit messages, one million "a"), the empty message, and the digest of the
//! digests of every length around the block boundaries (`hashlib`).

use super::*;
use crate::crypto::testutil::{c_table, hex};

extern crate std;
use std::vec::Vec;

fn sha256(data: &[u8]) -> [u8; SHA256_DIGEST_LENGTH] {
    let mut ctx = Sha2Ctx::default();
    let mut out = [0u8; SHA256_DIGEST_LENGTH];
    SHA256Init(&mut ctx);
    SHA256Update(&mut ctx, data);
    SHA256Final(&mut out, &mut ctx);
    out
}

fn sha384(data: &[u8]) -> [u8; SHA384_DIGEST_LENGTH] {
    let mut ctx = Sha2Ctx::default();
    let mut out = [0u8; SHA384_DIGEST_LENGTH];
    SHA384Init(&mut ctx);
    SHA384Update(&mut ctx, data);
    SHA384Final(&mut out, &mut ctx);
    out
}

fn sha512(data: &[u8]) -> [u8; SHA512_DIGEST_LENGTH] {
    let mut ctx = Sha2Ctx::default();
    let mut out = [0u8; SHA512_DIGEST_LENGTH];
    SHA512Init(&mut ctx);
    SHA512Update(&mut ctx, data);
    SHA512Final(&mut out, &mut ctx);
    out
}

const M448: &[u8] = b"abcdbcdecdefdefgefghfghighijhijkijkljklmklmnlmnomnopnopq";
const M896: &[u8] = b"abcdefghbcdefghicdefghijdefghijkefghijklfghijklmghijklmnhijklmnoijklmnopjklmnopqklmnopqrlmnopqrsmnopqrstnopqrstu";

#[test]
fn sha256_fips_180_4() {
    assert_eq!(
        sha256(b"abc").to_vec(),
        hex("ba7816bf8f01cfea414140de5dae2223b00361a396177a9cb410ff61f20015ad")
    );
    assert_eq!(
        sha256(M448).to_vec(),
        hex("248d6a61d20638b8e5c026930c3e6039a33ce45964ff2167f6ecedd419db06c1")
    );
    assert_eq!(
        sha256(M896).to_vec(),
        hex("cf5b16a778af8380036ce59e7b0492370b249b11e8f07a51afac45037afee9d1")
    );
    assert_eq!(
        sha256(b"").to_vec(),
        hex("e3b0c44298fc1c149afbf4c8996fb92427ae41e4649b934ca495991b7852b855")
    );
}

#[test]
fn sha384_fips_180_4() {
    assert_eq!(
        sha384(b"abc").to_vec(),
        hex(
            "cb00753f45a35e8bb5a03d699ac65007272c32ab0eded1631a8b605a43ff5bed8086072ba1e7cc2358baeca134c825a7"
        )
    );
    assert_eq!(
        sha384(M448).to_vec(),
        hex(
            "3391fdddfc8dc7393707a65b1b4709397cf8b1d162af05abfe8f450de5f36bc6b0455a8520bc4e6f5fe95b1fe3c8452b"
        )
    );
    assert_eq!(
        sha384(M896).to_vec(),
        hex(
            "09330c33f71147e83d192fc782cd1b4753111b173b3b05d22fa08086e3b0f712fcc7c71a557e2db966c3e9fa91746039"
        )
    );
    assert_eq!(
        sha384(b"").to_vec(),
        hex(
            "38b060a751ac96384cd9327eb1b1e36a21fdb71114be07434c0cc7bf63f6e1da274edebfe76f65fbd51ad2f14898b95b"
        )
    );
}

#[test]
fn sha512_fips_180_4() {
    assert_eq!(
        sha512(b"abc").to_vec(),
        hex(
            "ddaf35a193617abacc417349ae20413112e6fa4e89a97ea20a9eeee64b55d39a2192992a274fc1a836ba3c23a3feebbd454d4423643ce80e2a9ac94fa54ca49f"
        )
    );
    assert_eq!(
        sha512(M448).to_vec(),
        hex(
            "204a8fc6dda82f0a0ced7beb8e08a41657c16ef468b228a8279be331a703c33596fd15c13b1b07f9aa1d3bea57789ca031ad85c7a71dd70354ec631238ca3445"
        )
    );
    assert_eq!(
        sha512(M896).to_vec(),
        hex(
            "8e959b75dae313da8cf4f72814fc143f8f7779c6eb9f7fa17299aeadb6889018501d289e4900f7e4331b99dec4b5433ac7d329eeb6dd26545e96e55b874be909"
        )
    );
    assert_eq!(
        sha512(b"").to_vec(),
        hex(
            "cf83e1357eefb8bdf1542850d66d8007d620e4050b5715dc83f4a921d36ce9ce47d0d13c5d85f2b0ff8318d2877eec2f63b931bd47417a81a538327af927da3e"
        )
    );
}

#[test]
fn one_million_a() {
    let chunk = [b'a'; 1000];

    let mut ctx = Sha2Ctx::default();
    let mut out = [0u8; SHA256_DIGEST_LENGTH];
    SHA256Init(&mut ctx);
    for _ in 0..1000 {
        SHA256Update(&mut ctx, &chunk);
    }
    SHA256Final(&mut out, &mut ctx);
    assert_eq!(
        out.to_vec(),
        hex("cdc76e5c9914fb9281a1c7e284d73e67f1809a48a497200e046d39ccc7112cd0")
    );

    let mut out = [0u8; SHA384_DIGEST_LENGTH];
    SHA384Init(&mut ctx);
    for _ in 0..1000 {
        SHA384Update(&mut ctx, &chunk);
    }
    SHA384Final(&mut out, &mut ctx);
    assert_eq!(
        out.to_vec(),
        hex(
            "9d0e1809716474cb086e834e310a4a1ced149e9c00f248527972cec5704c2a5b07b8b3dc38ecc4ebae97ddd87f3d8985"
        )
    );

    let mut out = [0u8; SHA512_DIGEST_LENGTH];
    SHA512Init(&mut ctx);
    for _ in 0..1000 {
        SHA512Update(&mut ctx, &chunk);
    }
    SHA512Final(&mut out, &mut ctx);
    assert_eq!(
        out.to_vec(),
        hex(
            "e718483d0ce769644e2e42c7bc15b4638e1f98b13b2044285632a803afa973ebde0ff244877ea60a4cb0432ce577c31beb009c5c2c49aa2e4eadb217ad8cc09b"
        )
    );
}

fn sweep<const N: usize>(blk: usize, digest: fn(&[u8]) -> [u8; N]) -> [u8; N] {
    let mut acc: Vec<u8> = Vec::new();
    for n in 0..(3 * blk + 2) {
        let msg: Vec<u8> = (0..n).map(|i| ((i * 7 + n) % 256) as u8).collect();
        acc.extend_from_slice(&digest(&msg));
    }
    digest(&acc)
}

#[test]
fn every_length_around_the_blocks() {
    assert_eq!(
        sweep(SHA256_BLOCK_LENGTH, sha256).to_vec(),
        hex("175cf2a39abf9ec5bbb7e311fb9566d1c1b9cc7dc09580b1df88e18dfb14750b")
    );
    assert_eq!(
        sweep(SHA384_BLOCK_LENGTH, sha384).to_vec(),
        hex(
            "732b07eefd9a412195c2952aaae076bdd31d960cea58a03d2696ccba24df572ce3ee71990c5579cbf4f19867acb38526"
        )
    );
    assert_eq!(
        sweep(SHA512_BLOCK_LENGTH, sha512).to_vec(),
        hex(
            "6f71e9c06ce7c180c8ba541024f90374c6935148385579c4edf7bfed6c98b2f08056c35f118427b9ecd35334b44b6432e55b60de9845cd087445ed5586f777de"
        )
    );
}

#[test]
fn any_split_of_the_input_gives_the_same_digest() {
    let msg: Vec<u8> = (0..400).map(|i| (i * 13 % 256) as u8).collect();
    let (w256, w384, w512) = (sha256(&msg), sha384(&msg), sha512(&msg));
    for chunk in [
        1usize, 3, 7, 55, 56, 63, 64, 65, 111, 112, 127, 128, 129, 399,
    ] {
        let mut ctx = Sha2Ctx::default();
        let mut o256 = [0u8; SHA256_DIGEST_LENGTH];
        SHA256Init(&mut ctx);
        for piece in msg.chunks(chunk) {
            SHA256Update(&mut ctx, piece);
        }
        SHA256Final(&mut o256, &mut ctx);
        assert_eq!(o256, w256, "sha256 by {chunk}");
        // Final wipes the context.
        assert_eq!(ctx, Sha2Ctx::default());

        let mut o384 = [0u8; SHA384_DIGEST_LENGTH];
        SHA384Init(&mut ctx);
        for piece in msg.chunks(chunk) {
            SHA384Update(&mut ctx, piece);
        }
        SHA384Final(&mut o384, &mut ctx);
        assert_eq!(o384, w384, "sha384 by {chunk}");

        let mut o512 = [0u8; SHA512_DIGEST_LENGTH];
        SHA512Init(&mut ctx);
        for piece in msg.chunks(chunk) {
            SHA512Update(&mut ctx, piece);
        }
        SHA512Final(&mut o512, &mut ctx);
        assert_eq!(o512, w512, "sha512 by {chunk}");
    }
}

#[test]
#[ignore = "reads the C tables from $OPENBSD_SRC (just test-ref)"]
fn constants_match_the_c_file() {
    let f = "sys/crypto/sha2.c";
    let same32 = |name: &str, table: &[u32]| {
        let c = c_table(f, name);
        assert_eq!(c.len(), table.len(), "{name}");
        for (i, (a, b)) in c.iter().zip(table).enumerate() {
            assert_eq!(*a, u64::from(*b), "{name}[{i}]");
        }
    };
    let same64 = |name: &str, table: &[u64]| {
        assert_eq!(c_table(f, name), table, "{name}");
    };
    same32("K256", &K256);
    same32("sha256_initial_hash_value", &SHA256_INITIAL_HASH_VALUE);
    same64("K512", &K512);
    same64("sha384_initial_hash_value", &SHA384_INITIAL_HASH_VALUE);
    same64("sha512_initial_hash_value", &SHA512_INITIAL_HASH_VALUE);
}
