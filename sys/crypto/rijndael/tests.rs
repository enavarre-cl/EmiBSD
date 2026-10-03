//! Known-answer tests for the table-driven Rijndael: FIPS 197 appendices A.1 (key expansion),
//! B and C (the example vectors for 128, 192 and 256-bit keys), the generated tables, and a
//! reference-backed comparison of every table with the C file's.

use super::*;
use crate::crypto::testutil::{c_table, hex, hexn};

fn key() -> [u8; 32] {
    let mut k = [0u8; 32];
    for (i, b) in k.iter_mut().enumerate() {
        *b = i as u8;
    }
    k
}

const PT: &str = "00112233445566778899aabbccddeeff";

#[test]
fn fips197_appendix_c() {
    let want = [
        (128, "69c4e0d86a7b0430d8cdb78070b4c55a"),
        (192, "dda97ca4864cdfe06eaf70a0ec0d7191"),
        (256, "8ea2b7ca516745bfeafc49904b496089"),
    ];
    for (bits, ct) in want {
        let mut ctx = RijndaelCtx::default();
        let k = key();
        assert_eq!(
            rijndael_set_key(&mut ctx, &k[..bits / 8], bits as i32),
            Ok(())
        );
        assert_eq!(ctx.Nr, bits as i32 / 32 + 6);
        assert_eq!(ctx.enc_only, 0);

        let pt: [u8; 16] = hexn(PT);
        let mut out = [0u8; 16];
        rijndael_encrypt(&ctx, &pt, &mut out);
        assert_eq!(out.to_vec(), hex(ct), "encrypt {bits}");
        let mut back = [0u8; 16];
        rijndael_decrypt(&ctx, &out, &mut back);
        assert_eq!(back, pt, "decrypt {bits}");
    }
}

#[test]
fn fips197_appendix_b() {
    let k: [u8; 16] = hexn("2b7e151628aed2a6abf7158809cf4f3c");
    let pt: [u8; 16] = hexn("3243f6a8885a308d313198a2e0370734");
    let mut ctx = RijndaelCtx::default();
    assert_eq!(rijndael_set_key(&mut ctx, &k, 128), Ok(()));
    let mut out = [0u8; 16];
    rijndael_encrypt(&ctx, &pt, &mut out);
    assert_eq!(out.to_vec(), hex("3925841d02dc09fbdc118597196a0b32"));

    // Appendix A.1: the last round key of the expansion.
    assert_eq!(
        &ctx.ek[40..44],
        &[0xd014f9a8, 0xc9ee2589, 0xe13f0cc8, 0xb6630ca6]
    );
}

#[test]
fn encrypt_only_context() {
    let k = key();
    let mut ctx = RijndaelCtx::default();
    assert_eq!(rijndael_set_key_enc_only(&mut ctx, &k[..16], 128), Ok(()));
    assert_eq!((ctx.Nr, ctx.enc_only), (10, 1));
    let pt: [u8; 16] = hexn(PT);
    let mut out = [0u8; 16];
    rijndael_encrypt(&ctx, &pt, &mut out);
    assert_eq!(out.to_vec(), hex("69c4e0d86a7b0430d8cdb78070b4c55a"));
}

#[test]
fn bad_key_sizes_are_rejected() {
    let mut ctx = RijndaelCtx::default();
    for bits in [0, 64, 127, 129, 160, 224, 512] {
        assert_eq!(
            rijndael_set_key(&mut ctx, &[0; 64], bits),
            Err(Errno::EINVAL)
        );
        assert_eq!(
            rijndael_set_key_enc_only(&mut ctx, &[0; 64], bits),
            Err(Errno::EINVAL)
        );
    }
}

#[test]
fn the_decrypt_schedule_is_the_reversed_inverse_mixed_encrypt_schedule() {
    let k = key();
    let mut ek = [0u32; 60];
    let mut dk = [0u32; 60];
    assert_eq!(rijndaelKeySetupEnc(&mut ek, &k, 256), Ok(14));
    assert_eq!(rijndaelKeySetupDec(&mut dk, &k, 256), Ok(14));
    // The first and last round keys swap places unchanged.
    assert_eq!(dk[..4], ek[56..60]);
    assert_eq!(dk[56..60], ek[..4]);
}

#[test]
fn generated_tables_spot_checks() {
    assert_eq!(
        (SBOX[0x00], SBOX[0x01], SBOX[0x53], SBOX[0xff]),
        (0x63, 0x7c, 0xed, 0x16)
    );
    for x in 0..256 {
        assert_eq!(TD4[SBOX[x] as usize] as usize, x);
    }
    assert_eq!((TE0[0], TE0[1]), (0xc66363a5, 0xf87c7c84));
    assert_eq!(TD0[0], 0x51f4a750);
}

#[test]
#[ignore = "reads the C tables from $OPENBSD_SRC (just test-ref)"]
fn tables_match_the_c_file() {
    let f = "sys/crypto/rijndael.c";
    let check32 = |name: &str, table: &[u32; 256]| {
        let c = c_table(f, name);
        assert_eq!(c.len(), 256, "{name}");
        for (i, (a, b)) in c.iter().zip(table).enumerate() {
            assert_eq!(*a, u64::from(*b), "{name}[{i}]");
        }
    };
    check32("Te0", &TE0);
    check32("Te1", &TE1);
    check32("Te2", &TE2);
    check32("Te3", &TE3);
    check32("Td0", &TD0);
    check32("Td1", &TD1);
    check32("Td2", &TD2);
    check32("Td3", &TD3);

    let c = c_table(f, "Td4");
    assert_eq!(c.len(), 256);
    for (i, (a, b)) in c.iter().zip(TD4.iter()).enumerate() {
        assert_eq!(*a, u64::from(*b), "Td4[{i}]");
    }
    let c = c_table(f, "rcon");
    assert_eq!(c.len(), RCON.len());
    for (a, b) in c.iter().zip(RCON) {
        assert_eq!(*a, u64::from(b));
    }
}
