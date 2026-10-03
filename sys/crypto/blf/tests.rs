//! Known-answer tests for Blowfish: Eric Young's ECB test vectors (Schneier's `vectors.txt`),
//! longer and shorter keys checked against an independent implementation (pi computed with
//! big integers, the key schedule written from the specification), ECB and CBC runs, and a
//! reference-backed comparison of the initial state with the C file's.

use super::*;
use crate::crypto::testutil::{hex, hexn};

extern crate std;
use std::vec::Vec;

fn ecb_block(key: &[u8], pt: &str) -> Vec<u8> {
    let mut c = BlfCtx::default();
    blf_key(&mut c, key);
    let mut blk: [u8; 8] = hexn(pt);
    blf_ecb_encrypt(&c, &mut blk);
    blk.to_vec()
}

#[test]
fn ecb_test_vectors() {
    let cases = [
        ("0000000000000000", "0000000000000000", "4ef997456198dd78"),
        ("ffffffffffffffff", "ffffffffffffffff", "51866fd5b85ecb8a"),
        ("3000000000000000", "1000000000000001", "7d856f9a613063f2"),
        ("1111111111111111", "1111111111111111", "2466dd878b963c9d"),
        ("0123456789abcdef", "1111111111111111", "61f9c3802281b096"),
        ("fedcba9876543210", "0123456789abcdef", "0aceab0fc6a0a28d"),
    ];
    for (key, pt, ct) in cases {
        assert_eq!(ecb_block(&hex(key), pt), hex(ct), "key {key}");
    }
}

#[test]
fn key_sizes_from_40_to_448_bits() {
    let k56: Vec<u8> = (0..BLF_MAXKEYLEN)
        .map(|i| ((i * 3 + 1) % 256) as u8)
        .collect();
    assert_eq!(ecb_block(&k56, "0123456789abcdef"), hex("0ea1b05262fd079e"));
    assert_eq!(
        ecb_block(&[1, 2, 3, 4, 5], "0123456789abcdef"),
        hex("c27c55d0b4796e71")
    );
    let k16: Vec<u8> = (0..16).map(|i| ((i * 7 + 2) % 256) as u8).collect();
    assert_eq!(ecb_block(&k16, "fedcba9876543210"), hex("b51614c8d92cf9cb"));
}

#[test]
fn word_functions_round_trip() {
    let mut c = BlfCtx::default();
    blf_key(&mut c, b"a secret key");
    let orig = [0x0123_4567u32, 0x89ab_cdef, 0xfedc_ba98, 0x7654_3210];
    let mut d = orig;
    blf_enc(&c, &mut d, 2);
    assert_ne!(d, orig);
    let mut one = [orig[0], orig[1]];
    Blowfish_encipher(&c, &mut one);
    assert_eq!(&d[..2], &one);
    blf_dec(&c, &mut d, 2);
    assert_eq!(d, orig);
    Blowfish_decipher(&c, &mut one);
    assert_eq!(one, [orig[0], orig[1]]);
}

#[test]
fn ecb_runs() {
    let mut c = BlfCtx::default();
    blf_key(&mut c, b"another key");
    let data: Vec<u8> = (0..64).collect();
    let mut enc = data.clone();
    blf_ecb_encrypt(&c, &mut enc);
    assert_ne!(enc, data);
    // Blocks are independent in ECB; a trailing partial block is left alone.
    let mut first = data[..8].to_vec();
    blf_ecb_encrypt(&c, &mut first);
    assert_eq!(enc[..8], first[..]);
    let mut partial = data[..21].to_vec();
    blf_ecb_encrypt(&c, &mut partial);
    assert_eq!(partial[..16], enc[..16]);
    assert_eq!(partial[16..], data[16..21]);
    blf_ecb_decrypt(&c, &mut enc);
    assert_eq!(enc, data);
}

#[test]
fn cbc_known_answer_and_round_trip() {
    let k56: Vec<u8> = (0..BLF_MAXKEYLEN)
        .map(|i| ((i * 3 + 1) % 256) as u8)
        .collect();
    let mut c = BlfCtx::default();
    blf_key(&mut c, &k56);
    let iv: [u8; 8] = [0, 1, 2, 3, 4, 5, 6, 7];
    let data: Vec<u8> = (0..40).collect();
    let mut buf = data.clone();
    blf_cbc_encrypt(&c, &iv, &mut buf);
    assert_eq!(
        buf,
        hex("4bbbe6564c602d32aa3102b9bbe480d5d9ea3c1e29a4b2bb6018deb002905180c15eae033eba97fb")
    );
    blf_cbc_decrypt(&c, &iv, &mut buf);
    assert_eq!(buf, data);

    // One block, and nothing.
    let mut one = data[..8].to_vec();
    blf_cbc_encrypt(&c, &iv, &mut one);
    blf_cbc_decrypt(&c, &iv, &mut one);
    assert_eq!(one, data[..8]);
    let mut none: [u8; 0] = [];
    blf_cbc_decrypt(&c, &iv, &mut none);
}

#[test]
fn stream2word_wraps_around_the_key() {
    let mut j = 0u16;
    let key = [1u8, 2, 3];
    assert_eq!(Blowfish_stream2word(&key, &mut j), 0x0102_0301);
    assert_eq!(j, 1);
    assert_eq!(Blowfish_stream2word(&key, &mut j), 0x0203_0102);
    assert_eq!(j, 2);
}

#[test]
fn expandstate_with_zero_salt_equals_expand0state() {
    // The salt words are xored into the chained block: all zero, they change nothing.
    let mut a = BlfCtx::default();
    let mut b = BlfCtx::default();
    Blowfish_initstate(&mut a);
    Blowfish_initstate(&mut b);
    Blowfish_expand0state(&mut a, b"key");
    Blowfish_expandstate(&mut b, &[0u8; 16], b"key");
    assert_eq!(a, b);
}

#[test]
fn initial_state_is_the_digits_of_pi() {
    let mut c = BlfCtx::default();
    Blowfish_initstate(&mut c);
    assert_eq!(c.p[0], 0x243f6a88);
    assert_eq!(c.p[17], 0x8979fb1b);
    assert_eq!(c.s[0][0], 0xd1310ba6);
    assert_eq!(c.s[3][255], 0x3ac372e6);
}

#[test]
#[ignore = "reads the C table from $OPENBSD_SRC (just test-ref)"]
fn initial_state_matches_the_c_file() {
    let path = crate::reftest::openbsd_src().join("sys/crypto/blf.c");
    let text = std::fs::read_to_string(path).expect("blf.c");
    let a = text
        .find("static const blf_ctx initstate")
        .expect("initstate");
    let b = text.find("*c = initstate;").expect("end");
    let words: Vec<u64> = text[a..b]
        .split(|c: char| !c.is_ascii_alphanumeric())
        .filter_map(|t| t.strip_prefix("0x"))
        .map(|h| u64::from_str_radix(h, 16).expect("hex"))
        .collect();
    assert_eq!(words.len(), 1024 + 18);
    let c = &INITSTATE;
    let mut mine: Vec<u64> = c.s.iter().flatten().map(|w| u64::from(*w)).collect();
    mine.extend(c.p.iter().map(|w| u64::from(*w)));
    assert_eq!(words, mine);
}
