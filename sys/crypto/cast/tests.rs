//! Known-answer tests for CAST-128: RFC 2144 appendix B.1 (the single-block vectors for the
//! 128, 80 and 40-bit keys, the last two using the 12-round variant), round trips, and a
//! reference-backed comparison of the eight S-boxes with the C file's.

use super::*;
use crate::crypto::testutil::{c_table, hex, hexn};

const KEY: &str = "0123456712345678234567893456789a";
const PT: &str = "0123456789abcdef";

fn encrypt(keybytes: usize) -> ([u8; 8], CastKey) {
    let k: [u8; 16] = hexn(KEY);
    let mut key = CastKey::default();
    cast_setkey(&mut key, &k[..keybytes]);
    let mut out = [0u8; 8];
    cast_encrypt(&key, &hexn(PT), &mut out);
    (out, key)
}

#[test]
fn rfc2144_b_1() {
    let want = [
        (16, "238b4fe5847e44b2", 16),
        (10, "eb6a711a2c02271b", 12),
        (5, "7ac816d16e9b302e", 12),
    ];
    for (keybytes, ct, rounds) in want {
        let (out, key) = encrypt(keybytes);
        assert_eq!(out.to_vec(), hex(ct), "{keybytes}-byte key");
        assert_eq!(key.rounds, rounds);
        let mut back = [0u8; 8];
        cast_decrypt(&key, &out, &mut back);
        assert_eq!(back.to_vec(), hex(PT));
    }
}

#[test]
fn keys_between_the_sizes_use_the_zero_padded_schedule() {
    // 11 to 16 bytes use 16 rounds, up to 10 bytes 12; a short key is the same as the key
    // padded with zeros.
    let k: [u8; 16] = hexn(KEY);
    let mut a = CastKey::default();
    let mut b = CastKey::default();
    cast_setkey(&mut a, &k[..11]);
    let mut padded = [0u8; 16];
    padded[..11].copy_from_slice(&k[..11]);
    cast_setkey(&mut b, &padded);
    assert_eq!((a.rounds, b.rounds), (16, 16));
    assert_eq!(a.xkey, b.xkey);
    let mut c = CastKey::default();
    cast_setkey(&mut c, &k[..4]);
    assert_eq!(c.rounds, 12);
}

#[test]
fn blocks_in_chains_round_trip() {
    let (_, key) = encrypt(16);
    let mut blk = [0u8; 8];
    for round in 0..50u8 {
        let mut next = [0u8; 8];
        cast_encrypt(&key, &blk, &mut next);
        let mut back = [0u8; 8];
        cast_decrypt(&key, &next, &mut back);
        assert_eq!(back, blk, "round {round}");
        blk = next;
    }
}

#[test]
#[ignore = "reads the C tables from $OPENBSD_SRC (just test-ref)"]
fn sboxes_match_the_c_file() {
    let boxes = [
        ("cast_sbox1", &CAST_SBOX1),
        ("cast_sbox2", &CAST_SBOX2),
        ("cast_sbox3", &CAST_SBOX3),
        ("cast_sbox4", &CAST_SBOX4),
        ("cast_sbox5", &CAST_SBOX5),
        ("cast_sbox6", &CAST_SBOX6),
        ("cast_sbox7", &CAST_SBOX7),
        ("cast_sbox8", &CAST_SBOX8),
    ];
    for (name, table) in boxes {
        let c = c_table("sys/crypto/castsb.h", name);
        assert_eq!(c.len(), 256, "{name}");
        for (i, (a, b)) in c.iter().zip(table.iter()).enumerate() {
            assert_eq!(*a, u64::from(*b), "{name}[{i}]");
        }
    }
}
