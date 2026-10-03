//! Tests of the transform tables: the sizes each entry declares (which `ip_esp.c` and
//! `ip_ah.c` read), and the functions called directly (the software driver's tests cover them
//! through sessions).

use std::vec::Vec;
use std::{assert, assert_eq, vec};

use super::*;
use crate::crypto::testutil::hex;

#[test]
fn encryption_transform_sizes() {
    // (transform, algorithm, blocksize, ivsize, minkey, maxkey)
    let table: &[(&EncXform, i32, u16, u16, u16, u16)] = &[
        (&enc_xform_3des, CRYPTO_3DES_CBC, 8, 8, 24, 24),
        (&enc_xform_blf, CRYPTO_BLF_CBC, 8, 8, 5, 56),
        (&enc_xform_cast5, CRYPTO_CAST_CBC, 8, 8, 5, 16),
        (&enc_xform_aes, CRYPTO_AES_CBC, 16, 16, 16, 32),
        (&enc_xform_aes_ctr, CRYPTO_AES_CTR, 16, 8, 20, 36),
        (&enc_xform_aes_gcm, CRYPTO_AES_GCM_16, 1, 8, 20, 36),
        (&enc_xform_aes_gmac, CRYPTO_AES_GMAC, 1, 8, 20, 36),
        (&enc_xform_aes_xts, CRYPTO_AES_XTS, 16, 8, 32, 64),
        (
            &enc_xform_chacha20_poly1305,
            CRYPTO_CHACHA20_POLY1305,
            1,
            8,
            36,
            36,
        ),
        (&enc_xform_null, CRYPTO_NULL, 4, 0, 0, 256),
    ];
    for (x, alg, blocksize, ivsize, minkey, maxkey) in table {
        assert_eq!(x.type_, *alg, "{}", x.name);
        assert_eq!(
            (x.blocksize, x.ivsize, x.minkey, x.maxkey),
            (*blocksize, *ivsize, *minkey, *maxkey),
            "{}",
            x.name
        );
        // A transform has a schedule exactly when it has a setkey that needs one.
        assert_eq!(
            x.ctxsize == 0,
            x.name == "NULL" || x.name == "AES-GMAC",
            "{}",
            x.name
        );
    }
    assert_eq!(enc_xform_3des.ctxsize, 384);
    // The GMAC "cipher" has no functions at all.
    assert!(enc_xform_aes_gmac.encrypt.is_none() && enc_xform_aes_gmac.setkey.is_none());
    // The modes with their own IV handling have a reinit.
    let reinit: Vec<&str> = [
        &enc_xform_aes_ctr,
        &enc_xform_aes_gcm,
        &enc_xform_aes_xts,
        &enc_xform_chacha20_poly1305,
    ]
    .iter()
    .map(|x| x.name)
    .collect();
    assert_eq!(
        reinit,
        ["AES-CTR", "AES-GCM", "AES-XTS", "CHACHA20-POLY1305"]
    );
    assert!(enc_xform_aes.reinit.is_none() && enc_xform_3des.reinit.is_none());
}

#[test]
fn authentication_transform_sizes() {
    // (transform, algorithm, keysize, hashsize, authsize, blocksize)
    let table: &[(&AuthHash, i32, u16, u16, u16, u16)] = &[
        (&auth_hash_hmac_md5_96, CRYPTO_MD5_HMAC, 16, 16, 12, 64),
        (&auth_hash_hmac_sha1_96, CRYPTO_SHA1_HMAC, 20, 20, 12, 64),
        (
            &auth_hash_hmac_ripemd_160_96,
            CRYPTO_RIPEMD160_HMAC,
            20,
            20,
            12,
            64,
        ),
        (
            &auth_hash_hmac_sha2_256_128,
            CRYPTO_SHA2_256_HMAC,
            32,
            32,
            16,
            64,
        ),
        (
            &auth_hash_hmac_sha2_384_192,
            CRYPTO_SHA2_384_HMAC,
            48,
            48,
            24,
            128,
        ),
        (
            &auth_hash_hmac_sha2_512_256,
            CRYPTO_SHA2_512_HMAC,
            64,
            64,
            32,
            128,
        ),
        (&auth_hash_gmac_aes_128, CRYPTO_AES_128_GMAC, 20, 16, 16, 16),
        (&auth_hash_gmac_aes_192, CRYPTO_AES_192_GMAC, 28, 16, 16, 16),
        (&auth_hash_gmac_aes_256, CRYPTO_AES_256_GMAC, 36, 16, 16, 16),
        (
            &auth_hash_chacha20_poly1305,
            CRYPTO_CHACHA20_POLY1305_MAC,
            36,
            16,
            16,
            64,
        ),
    ];
    for (x, alg, keysize, hashsize, authsize, blocksize) in table {
        assert_eq!(x.type_, *alg, "{}", x.name);
        assert_eq!(
            (x.keysize, x.hashsize, x.authsize, x.blocksize),
            (*keysize, *hashsize, *authsize, *blocksize),
            "{}",
            x.name
        );
        // Only the combined-mode authenticators take a key and an IV themselves.
        let aead = x.name.starts_with("GMAC") || x.name.starts_with("CHACHA");
        assert_eq!(x.Setkey.is_some(), aead, "{}", x.name);
        assert_eq!(x.Reinit.is_some(), aead, "{}", x.name);
    }
}

fn mac(axf: &AuthHash, data: &[u8]) -> Vec<u8> {
    let mut ctx = AuthCtx::None;
    (axf.Init)(&mut ctx);
    (axf.Update)(&mut ctx, data).expect("update");
    let mut out = vec![0u8; usize::from(axf.hashsize)];
    (axf.Final)(&mut out, &mut ctx);
    out
}

#[test]
fn the_hash_transforms_are_the_plain_hashes() {
    assert_eq!(
        mac(&auth_hash_hmac_md5_96, b"abc"),
        hex("900150983cd24fb0d6963f7d28e17f72")
    );
    assert_eq!(
        mac(&auth_hash_hmac_sha1_96, b"abc"),
        hex("a9993e364706816aba3e25717850c26c9cd0d89d")
    );
    assert_eq!(
        mac(&auth_hash_hmac_ripemd_160_96, b"abc"),
        hex("8eb208f7e05d987a9b044a8e98c6b087f15a0bfc")
    );
    assert_eq!(
        mac(&auth_hash_hmac_sha2_256_128, b"abc"),
        hex("ba7816bf8f01cfea414140de5dae2223b00361a396177a9cb410ff61f20015ad")
    );
    assert_eq!(
        mac(&auth_hash_hmac_sha2_384_192, b"abc"),
        hex(
            "cb00753f45a35e8bb5a03d699ac65007272c32ab0eded1631a8b605a43ff5bed8086072ba1e7cc2358baeca134c825a7"
        )
    );
    assert_eq!(
        mac(&auth_hash_hmac_sha2_512_256, b"abc"),
        hex(
            "ddaf35a193617abacc417349ae20413112e6fa4e89a97ea20a9eeee64b55d39a2192992a274fc1a836ba3c23a3feebbd454d4423643ce80e2a9ac94fa54ca49f"
        )
    );
}

#[test]
fn aes_ctr_counter_and_gcm_start() {
    let mut ks = Kschedule::None;
    let mut key = hex("ae6852f8121067cc4bf7a5765577f39e");
    key.extend_from_slice(&hex("00000030"));
    (enc_xform_aes_ctr.setkey.expect("setkey"))(&mut ks, &key).expect("setkey");
    (enc_xform_aes_ctr.reinit.expect("reinit"))(&mut ks, &[0; 8]);
    let Kschedule::AesCtr(ctx) = &ks else {
        panic!("an AES-CTR schedule")
    };
    assert_eq!(ctx.ac_block, hex("00000030000000000000000000000000")[..]);
    // GCM's counter starts at 1.
    let mut ks = Kschedule::None;
    (enc_xform_aes_gcm.setkey.expect("setkey"))(&mut ks, &key).expect("setkey");
    (enc_xform_aes_gcm.reinit.expect("reinit"))(&mut ks, &[0; 8]);
    let Kschedule::AesCtr(ctx) = &ks else {
        panic!("an AES-CTR schedule")
    };
    assert_eq!(ctx.ac_block, hex("00000030000000000000000000000001")[..]);
    // Each block advances the counter, with carry out of the low byte.
    let mut blk = [0u8; 16];
    (enc_xform_aes_ctr.encrypt.expect("encrypt"))(&mut ks, &mut blk);
    let Kschedule::AesCtr(ctx) = &ks else {
        panic!("an AES-CTR schedule")
    };
    assert_eq!(ctx.ac_block[15], 2);
}

#[test]
fn setkey_refuses_what_the_c_would_read_past() {
    let mut ks = Kschedule::None;
    assert_eq!(
        (enc_xform_3des.setkey.expect("setkey"))(&mut ks, &[0; 23]),
        Err(Errno::EINVAL)
    );
    assert_eq!(
        (enc_xform_blf.setkey.expect("setkey"))(&mut ks, &[]),
        Err(Errno::EINVAL)
    );
    assert_eq!(
        (enc_xform_aes_ctr.setkey.expect("setkey"))(&mut ks, &[0; 3]),
        Err(Errno::EINVAL)
    );
    assert_eq!(
        (enc_xform_aes_ctr.setkey.expect("setkey"))(&mut ks, &[0; 17]),
        Err(Errno::EINVAL)
    );
    assert_eq!(
        (enc_xform_chacha20_poly1305.setkey.expect("setkey"))(&mut ks, &[0; 35]),
        Err(Errno::EINVAL)
    );
    let mut ctx = AuthCtx::None;
    (auth_hash_gmac_aes_128.Init)(&mut ctx);
    assert_eq!(
        (auth_hash_gmac_aes_128.Setkey.expect("setkey"))(&mut ctx, &[0; 18]),
        Err(Errno::EINVAL)
    );
    assert_eq!(
        (auth_hash_gmac_aes_128.Setkey.expect("setkey"))(&mut ctx, &[0; 20]),
        Ok(())
    );
}
