//! Known-answer tests for DES and triple DES: the classic DES example (key 133457799BBCDFF1)
//! as EDE with one key thrice, three-key vectors computed with `openssl`'s `des-ede3`, the
//! decryption order `xform.c` uses (the schedules reversed, `encrypt` false), and a
//! reference-backed comparison of the tables with the C files'.

use super::*;
use crate::crypto::set_key::des_set_key;
use crate::crypto::testutil::{c_table, hex, hexn};

extern crate std;
use std::vec::Vec;

fn schedule(key: &str) -> DesKeySchedule {
    let k: DesCblock = hexn(key);
    let mut ks = [0u32; 32];
    assert_eq!(des_set_key(&k, &mut ks), Ok(()));
    ks
}

fn ede3(blk: &[u8], k1: &str, k2: &str, k3: &str, encrypt: bool) -> Vec<u8> {
    let (s1, s2, s3) = (schedule(k1), schedule(k2), schedule(k3));
    let mut input: DesCblock = [0; 8];
    input.copy_from_slice(blk);
    let mut out = [0u8; 8];
    if encrypt {
        des_ecb3_encrypt(&input, &mut out, &s1, &s2, &s3, true);
    } else {
        // des3_decrypt: the schedules in the opposite order, encrypt false.
        des_ecb3_encrypt(&input, &mut out, &s3, &s2, &s1, false);
    }
    out.to_vec()
}

#[test]
fn des_known_answer_as_ede_with_one_key() {
    let k = "133457799bbcdff1";
    assert_eq!(
        ede3(&hex("0123456789abcdef"), k, k, k, true),
        hex("85e813540f0ab405")
    );
    assert_eq!(
        ede3(&hex("85e813540f0ab405"), k, k, k, false),
        hex("0123456789abcdef")
    );
}

const K1: &str = "0123456789abcdef";
const K2: &str = "23456789abcdef01";
const K3: &str = "456789abcdef0123";

#[test]
fn three_key_vectors() {
    assert_eq!(
        ede3(&hex("6bc1bee22e409f96"), K1, K2, K3, true),
        hex("714772f339841d34")
    );
    // "Now is the time " in two blocks.
    assert_eq!(ede3(b"Now is t", K1, K2, K3, true), hex("314f8327fa7a09a8"));
    assert_eq!(ede3(b"he time ", K1, K2, K3, true), hex("4362760cc13ba7da"));
    for ct in ["714772f339841d34", "314f8327fa7a09a8", "4362760cc13ba7da"] {
        let pt = ede3(&hex(ct), K1, K2, K3, false);
        assert_eq!(ede3(&pt, K1, K2, K3, true), hex(ct));
    }
    assert_eq!(
        ede3(&hex("714772f339841d34"), K1, K2, K3, false),
        hex("6bc1bee22e409f96")
    );
}

#[test]
fn encrypt_flag_false_is_the_inverse_with_the_same_schedule_order() {
    let (s1, s2, s3) = (schedule(K1), schedule(K2), schedule(K3));
    let pt: DesCblock = hexn("0011223344556677");
    let mut ct = [0u8; 8];
    des_ecb3_encrypt(&pt, &mut ct, &s1, &s2, &s3, true);
    // Decryption runs ks1 backwards, ks2 forwards, ks3 backwards: undoing ks3, ks2, ks1 needs
    // the order reversed, as xform.c passes them.
    let mut back = [0u8; 8];
    des_ecb3_encrypt(&ct, &mut back, &s3, &s2, &s1, false);
    assert_eq!(back, pt);
}

#[test]
#[ignore = "reads the C tables from $OPENBSD_SRC (just test-ref)"]
fn tables_match_the_c_files() {
    use crate::crypto::podd::ODD_PARITY;
    use crate::crypto::sk::DES_SKB;
    use crate::crypto::spr::DES_SPTRANS;

    let skb: Vec<u64> = DES_SKB.iter().flatten().map(|w| u64::from(*w)).collect();
    assert_eq!(c_table("sys/crypto/sk.h", "des_skb"), skb);
    let sp: Vec<u64> = DES_SPTRANS
        .iter()
        .flatten()
        .map(|w| u64::from(*w))
        .collect();
    assert_eq!(c_table("sys/crypto/spr.h", "des_SPtrans"), sp);
    let odd: Vec<u64> = ODD_PARITY.iter().map(|b| u64::from(*b)).collect();
    assert_eq!(c_table("sys/crypto/podd.h", "odd_parity"), odd);
}
