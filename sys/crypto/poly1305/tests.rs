//! Known-answer tests for Poly1305: RFC 8439 section 2.5.2 and appendix A.3 (the edge cases
//! of the final reduction), and tags of messages around the block size computed with an
//! independent big-integer reference.

use super::*;
use crate::crypto::testutil::{hex, hexn};

fn mac(key: &[u8; 32], msg: &[u8]) -> [u8; 16] {
    let mut st = Poly1305State::default();
    let mut tag = [0u8; 16];
    poly1305_init(&mut st, key);
    poly1305_update(&mut st, msg);
    poly1305_finish(&mut st, &mut tag);
    tag
}

#[test]
fn rfc8439_2_5_2() {
    let key: [u8; 32] = hexn("85d6be7857556d337f4452fe42d506a80103808afb0db2fd4abff6af4149f51b");
    let tag = mac(&key, b"Cryptographic Forum Research Group");
    assert_eq!(tag.to_vec(), hex("a8061dc1305136c6c22b8baf0c0127a9"));
}

#[test]
fn rfc8439_a_3_edge_cases() {
    // #5: the sum is 2^130 - 5 + 1 plus the pad (the carry out of h reaches the tag).
    let mut key = [0u8; 32];
    key[0] = 2;
    assert_eq!(
        mac(&key, &[0xff; 16]).to_vec(),
        hex("03000000000000000000000000000000")
    );
    // #1: an all-zero key and message give an all-zero tag.
    assert_eq!(mac(&[0; 32], &[0; 64]), [0; 16]);
    // The all-ones key and message exercise the clamp and the h >= p select.
    assert_eq!(
        mac(&[0xff; 32], &[0xff; 48]).to_vec(),
        hex("5efc6a6b51fcec4c787c5075997c95e4")
    );
}

fn data() -> [u8; 200] {
    let mut d = [0u8; 200];
    for (i, b) in d.iter_mut().enumerate() {
        *b = ((i * 7 + 1) % 256) as u8;
    }
    d
}

fn key() -> [u8; 32] {
    let mut k = [0u8; 32];
    for (i, b) in k.iter_mut().enumerate() {
        *b = ((i * 5 + 9) % 256) as u8;
    }
    k
}

const CASES: &[(usize, &str)] = &[
    (0, "595e63686d72777c81868b90959a9fa4"),
    (1, "6775848391b0c0afbdecfccbd92839f8"),
    (15, "acb8dc8611e1dc9fa4ca616590b862a7"),
    (16, "2b4f1870f1db48d1e829cac2387cfb5f"),
    (17, "a2cb8397776305a94979964cfe0d323e"),
    (32, "e26590e86fc347feb25758ff022adee4"),
    (33, "eb5af15115885af57f75dfa92f15b28b"),
    (100, "ef2fbc449ff768235384312a4c3bf256"),
    (200, "607c03d8d2120d8530ef647ec38da4cd"),
];

#[test]
fn lengths_around_the_block_size() {
    let d = data();
    for (n, want) in CASES {
        assert_eq!(mac(&key(), &d[..*n]).to_vec(), hex(want), "length {n}");
    }
}

#[test]
fn any_split_of_the_input_gives_the_same_tag() {
    let d = data();
    for (n, want) in CASES {
        for chunk in [1usize, 3, 7, 15, 16, 17, 31, 64] {
            let mut st = Poly1305State::default();
            let mut tag = [0u8; 16];
            poly1305_init(&mut st, &key());
            for piece in d[..*n].chunks(chunk) {
                poly1305_update(&mut st, piece);
            }
            poly1305_finish(&mut st, &mut tag);
            assert_eq!(tag.to_vec(), hex(want), "length {n} in chunks of {chunk}");
        }
    }
}

#[test]
fn finish_clears_the_key_material() {
    let mut st = Poly1305State::default();
    let mut tag = [0u8; 16];
    poly1305_init(&mut st, &key());
    poly1305_update(&mut st, b"abc");
    poly1305_finish(&mut st, &mut tag);
    assert_eq!((st.r, st.h, st.pad), ([0; 5], [0; 5], [0; 4]));
}
