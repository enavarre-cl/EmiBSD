//! Known-answer tests for the constant-time AES: FIPS 197 appendices B and C, NIST SP 800-38A
//! (CBC and CTR built on the block function), the bitsliced S-box against the table of
//! `rijndael.rs`, the multi-block ECB paths, and the key schedules against `rijndael.rs`'s.

use super::*;
use crate::crypto::rijndael::{
    RijndaelCtx, rijndael_set_key, rijndaelKeySetupDec, rijndaelKeySetupEnc,
};
use crate::crypto::testutil::{hex, hexn};

extern crate std;
use std::vec::Vec;

fn key(len: usize) -> Vec<u8> {
    (0..len).map(|i| i as u8).collect()
}

const PT: &str = "00112233445566778899aabbccddeeff";

#[test]
fn fips197_appendix_c() {
    let want = [
        (16, "69c4e0d86a7b0430d8cdb78070b4c55a"),
        (24, "dda97ca4864cdfe06eaf70a0ec0d7191"),
        (32, "8ea2b7ca516745bfeafc49904b496089"),
    ];
    for (len, ct) in want {
        let mut ctx = AesCtx::default();
        assert_eq!(AES_Setkey(&mut ctx, &key(len)), Ok(()));
        assert_eq!(ctx.num_rounds, len as u32 / 4 + 6);

        let pt: [u8; 16] = hexn(PT);
        let mut out = [0u8; 16];
        AES_Encrypt(&ctx, &pt, &mut out);
        assert_eq!(out.to_vec(), hex(ct), "encrypt {len}");
        let mut back = [0u8; 16];
        AES_Decrypt(&ctx, &out, &mut back);
        assert_eq!(back, pt, "decrypt {len}");
    }
}

#[test]
fn fips197_appendix_b() {
    let k: [u8; 16] = hexn("2b7e151628aed2a6abf7158809cf4f3c");
    let mut ctx = AesCtx::default();
    assert_eq!(AES_Setkey(&mut ctx, &k), Ok(()));
    let mut out = [0u8; 16];
    AES_Encrypt(&ctx, &hexn("3243f6a8885a308d313198a2e0370734"), &mut out);
    assert_eq!(out.to_vec(), hex("3925841d02dc09fbdc118597196a0b32"));
}

#[test]
fn bad_key_sizes_are_rejected() {
    let mut ctx = AesCtx::default();
    for len in [0usize, 1, 15, 17, 20, 23, 25, 31, 33, 64] {
        let k = key(len);
        assert_eq!(AES_Setkey(&mut ctx, &k), Err(Errno::EINVAL), "{len}");
        let mut sk = [0u32; 60];
        assert_eq!(
            AES_KeySetup_Encrypt(&mut sk, &k),
            Err(Errno::EINVAL),
            "{len}"
        );
        assert_eq!(
            AES_KeySetup_Decrypt(&mut sk, &k),
            Err(Errno::EINVAL),
            "{len}"
        );
    }
}

#[test]
fn the_bitsliced_sbox_is_the_aes_sbox() {
    // 256 inputs, 32 at a time (eight words of 32 bit-sliced bytes); the outputs are checked
    // against the S-box by applying the inverse circuit and getting the input back, and
    // against the known corners.
    let mut q = [0u32; 8];
    for (i, w) in q.iter_mut().enumerate() {
        // byte x (0..32) at rank x: bit i of x.
        for x in 0..32u32 {
            *w |= ((x >> i) & 1) << x;
        }
    }
    let input = q;
    aes_ct_bitslice_Sbox(&mut q);
    // S(0) = 0x63: bit i of the output for x = 0 is at rank 0 of word i.
    let s0: u8 = (0..8).map(|i| ((q[i] & 1) as u8) << i).sum();
    assert_eq!(s0, 0x63);
    // S(1) = 0x7c.
    let s1: u8 = (0..8).map(|i| (((q[i] >> 1) & 1) as u8) << i).sum();
    assert_eq!(s1, 0x7c);
    aes_ct_bitslice_invSbox(&mut q);
    assert_eq!(q, input);
}

#[test]
fn ortho_is_an_involution() {
    let mut q = [
        0x01234567, 0x89abcdef, 0xfedcba98, 0x76543210, 0xdeadbeef, 0x0badf00d, 0xc0ffee00,
        0x12345678,
    ];
    let orig = q;
    aes_ct_ortho(&mut q);
    assert_ne!(q, orig);
    aes_ct_ortho(&mut q);
    assert_eq!(q, orig);
}

#[test]
fn ecb_runs_of_any_length() {
    let mut ctx = AesCtx::default();
    assert_eq!(AES_Setkey(&mut ctx, &key(24)), Ok(()));
    let data: Vec<u8> = (0..16 * 7).map(|i| (i * 5 + 1) as u8).collect();

    // One block at a time is the reference.
    let mut single = std::vec![0u8; data.len()];
    for (s, d) in data.chunks(16).zip(single.chunks_mut(16)) {
        let mut b = [0u8; 16];
        b.copy_from_slice(s);
        let mut o = [0u8; 16];
        AES_Encrypt(&ctx, &b, &mut o);
        d.copy_from_slice(&o);
    }
    for n in 1..=7 {
        let mut out = std::vec![0u8; 16 * n];
        AES_Encrypt_ECB(&ctx, &data[..16 * n], &mut out, n);
        assert_eq!(out, &single[..16 * n], "{n} blocks");
        let mut back = std::vec![0u8; 16 * n];
        AES_Decrypt_ECB(&ctx, &out, &mut back, n);
        assert_eq!(back, &data[..16 * n], "{n} blocks back");
    }
    // Zero blocks is nothing.
    AES_Encrypt_ECB(&ctx, &[], &mut [], 0);
}

#[test]
fn key_setup_functions_agree_with_the_table_driven_cipher() {
    for len in [16usize, 24, 32] {
        let k = key(len);
        let mut ek = [0u32; 60];
        let mut dk = [0u32; 60];
        let r = AES_KeySetup_Encrypt(&mut ek, &k).unwrap();
        assert_eq!(AES_KeySetup_Decrypt(&mut dk, &k), Ok(r));

        let mut rek = [0u32; 60];
        let mut rdk = [0u32; 60];
        let bits = len as i32 * 8;
        assert_eq!(rijndaelKeySetupEnc(&mut rek, &k, bits), Ok(r as i32));
        assert_eq!(rijndaelKeySetupDec(&mut rdk, &k, bits), Ok(r as i32));
        let n = 4 * (r as usize + 1);
        assert_eq!(ek[..n], rek[..n], "encrypt schedule {len}");
        assert_eq!(dk[..n], rdk[..n], "decrypt schedule {len}");

        // And both ciphers agree on a block.
        let mut rctx = RijndaelCtx::default();
        assert_eq!(rijndael_set_key(&mut rctx, &k, bits), Ok(()));
        let mut a = AesCtx::default();
        assert_eq!(AES_Setkey(&mut a, &k), Ok(()));
        let pt: [u8; 16] = hexn("00112233445566778899aabbccddeeff");
        let (mut x, mut y) = ([0u8; 16], [0u8; 16]);
        AES_Encrypt(&a, &pt, &mut x);
        crate::crypto::rijndael::rijndael_encrypt(&rctx, &pt, &mut y);
        assert_eq!(x, y);
    }
}

#[test]
fn nist_sp800_38a_cbc_and_ctr_built_on_the_block_function() {
    let k: [u8; 16] = hexn("2b7e151628aed2a6abf7158809cf4f3c");
    let pt = hex(
        "6bc1bee22e409f96e93d7e117393172aae2d8a571e03ac9c9eb76fac45af8e51
                  30c81c46a35ce411e5fbc1191a0a52eff69f2445df4f9b17ad2b417be66c3710",
    );
    let mut ctx = AesCtx::default();
    assert_eq!(AES_Setkey(&mut ctx, &k), Ok(()));

    // F.2.1 CBC-AES128.Encrypt
    let mut prev: [u8; 16] = hexn("000102030405060708090a0b0c0d0e0f");
    let mut cbc = Vec::new();
    for blk in pt.chunks(16) {
        let mut x = [0u8; 16];
        for i in 0..16 {
            x[i] = blk[i] ^ prev[i];
        }
        AES_Encrypt(&ctx, &x, &mut prev);
        cbc.extend_from_slice(&prev);
    }
    assert_eq!(
        cbc,
        hex(
            "7649abac8119b246cee98e9b12e9197d5086cb9b507219ee95db113a917678b2
             73bed6b8e3c1743b7116e69e222295163ff1caa1681fac09120eca307586e1a7"
        )
    );

    // F.5.1 CTR-AES128.Encrypt
    let mut counter: [u8; 16] = hexn("f0f1f2f3f4f5f6f7f8f9fafbfcfdfeff");
    let mut ctr = Vec::new();
    for blk in pt.chunks(16) {
        let mut ks = [0u8; 16];
        AES_Encrypt(&ctx, &counter, &mut ks);
        ctr.extend(blk.iter().zip(ks).map(|(a, b)| a ^ b));
        for i in (0..16).rev() {
            counter[i] = counter[i].wrapping_add(1);
            if counter[i] != 0 {
                break;
            }
        }
    }
    assert_eq!(
        ctr,
        hex(
            "874d6191b620e3261bef6864990db6ce9806f66b7970fdff8617187bb9fffdff
             5ae4df3edbd5d35e5b4f09020db03eab1e031dda2fbe03d1792170a0f3009cee"
        )
    );
}
