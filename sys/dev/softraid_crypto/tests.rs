//! Tests of the CRYPTO discipline's key handling (the mask cipher, the HMAC check, key
//! creation, unlocking and re-masking, the user structures) and of its I/O path through
//! crypto(9) and the software driver, against references computed outside this tree
//! (FIPS-197, Python's `hmac`, an IEEE 1619 XTS built on openssl(1)'s AES).

use super::*;

extern crate std;
use std::boxed::Box;
use std::vec;
use std::vec::Vec;

use crate::crypto::crypto::crypto_reset;
use crate::crypto::cryptosoft::{swcr_init, swcr_reset};
use crate::crypto::testutil::serial;
use crate::kern::subr_pool::tests::setup_real_memory;
use crate::scsi::scsiconf::ScsiXfer;
use crate::sys::types::Daddr;

fn hex(s: &str) -> Vec<u8> {
    (0..s.len())
        .step_by(2)
        .map(|i| u8::from_str_radix(&s[i..i + 2], 16).unwrap())
        .collect()
}

fn cells(b: &[u8]) -> Vec<Cell<u8>> {
    b.iter().map(|&x| Cell::new(x)).collect()
}

fn bytes(c: &[Cell<u8>]) -> Vec<u8> {
    c.iter().map(Cell::get).collect()
}

fn sha1(b: &[u8]) -> [u8; SHA1_DIGEST_LENGTH] {
    let mut ctx = Sha1Ctx::default();
    let mut d = [0u8; SHA1_DIGEST_LENGTH];
    SHA1Init(&mut ctx);
    SHA1Update(&mut ctx, b);
    SHA1Final(&mut d, &mut ctx);
    d
}

/// A zeroed discipline of a zeroed softc, with in-memory metadata (leaked).
fn discipline() -> &'static SrDiscipline {
    // SAFETY: `SrSoftc` is a `Softc`: all-zero bytes are a valid value of it.
    let sc: &'static SrSoftc = Box::leak(Box::new(unsafe { core::mem::zeroed::<SrSoftc>() }));
    // SAFETY: a zeroed discipline (`SrZeroed`), never freed in the tests.
    let sd: &'static SrDiscipline =
        unsafe { sr_malloc::<SrDiscipline>(M_WAITOK).unwrap().as_ref() };
    sd.sd_sc.set(sc);
    sd.sd_meta.set(Some(
        sr_malloc_size::<SrMetadata>(SR_META_BYTES, M_WAITOK).unwrap(),
    ));
    sd
}

/// An optional metadata item of `type_` and `len` bytes.
fn opt_item(type_: u32, len: usize) -> &'static SrMetaOptItem {
    let omi = SrMetaOptItem::alloc(len, M_WAITOK).unwrap();
    omi.omi_som().som_type.set(type_);
    omi.omi_som().som_length.set(len as u32);
    omi
}

/// A CRYPTO discipline whose crypto metadata item is in place (as after assembly).
fn crypto_volume() -> &'static SrDiscipline {
    let sd = discipline();
    sr_crypto_discipline_init(sd);
    let omi = opt_item(SR_OPT_CRYPTO, size_of::<SrMetaCrypto>());
    sr_crypto_meta_opt_handler(sd, omi).unwrap();
    sd
}

fn key_bytes(keys: &[Cell<[u8; SR_CRYPTO_KEYBYTES]>; SR_CRYPTO_MAXKEYS]) -> Vec<u8> {
    bytes(key_cells(keys))
}

const MASK_A: [u8; 32] = [0xa5; 32];
const MASK_B: [u8; 32] = [0x3c; 32];

#[test]
fn mask_cipher_is_aes256_ecb() {
    // FIPS-197 appendix C.3
    let key: Vec<u8> = (0..32).collect();
    let p = cells(&hex("00112233445566778899aabbccddeeff"));
    let c = cells(&[0; 16]);
    sr_crypto_encrypt(&p, &c, &key, SR_CRYPTOM_AES_ECB_256).unwrap();
    assert_eq!(bytes(&c), hex("8ea2b7ca516745bfeafc49904b496089"));

    let back = cells(&[0; 16]);
    sr_crypto_decrypt(&c, &back, &key, SR_CRYPTOM_AES_ECB_256).unwrap();
    assert_eq!(bytes(&back), bytes(&p));

    // each 16-byte block on its own (ECB)
    let p2 = cells(&hex("00112233445566778899aabbccddeeff").repeat(2));
    let c2 = cells(&[0; 32]);
    sr_crypto_encrypt(&p2, &c2, &key, SR_CRYPTOM_AES_ECB_256).unwrap();
    assert_eq!(bytes(&c2[16..]), hex("8ea2b7ca516745bfeafc49904b496089"));

    // the C's -1
    assert_eq!(sr_crypto_encrypt(&p, &c, &key, 7), Err(Errno::EINVAL));
    assert_eq!(sr_crypto_decrypt(&c, &p, &key, 0), Err(Errno::EINVAL));
}

#[test]
fn check_is_hmac_sha1_keyed_with_sha1_of_the_mask_key() {
    // hmac.new(sha1(bytes(range(32))).digest(), key, sha1) in Python
    let maskkey: Vec<u8> = (0..32).collect();
    let key: Vec<u8> = (0..SR_CRYPTO_KEYSZ).map(|i| (i * 7 + 3) as u8).collect();
    let mut digest = [0u8; SHA1_DIGEST_LENGTH];
    sr_crypto_calculate_check_hmac_sha1(&maskkey, &cells(&key), &mut digest);
    assert_eq!(
        digest.to_vec(),
        hex("f6bb212fe6c7735143942ccd3448af16c3a324c0")
    );
}

#[test]
fn created_keys_unlock_with_the_mask_key_only() {
    let _g = setup_real_memory();
    let sd = crypto_volume();
    let mdd = &sd.mds().mdd_crypto;
    let meta = mdd.scr_meta().unwrap();

    mdd.scr_maskkey.set(MASK_A);
    sr_crypto_create_keys(sd, mdd).unwrap();
    assert_eq!(meta.scm_alg.get(), SR_CRYPTOA_AES_XTS_256);
    assert_eq!(meta.scm_mask_alg.get(), SR_CRYPTOM_AES_ECB_256);
    assert_eq!(meta.scm_check_alg.get(), SR_CRYPTOC_HMAC_SHA1);
    assert_eq!(meta.scm_flags.get(), SR_CRYPTOF_KEY | SR_CRYPTOF_KDFHINT);
    // the plain keys are gone; the masked ones are on the metadata
    assert!(key_bytes(&mdd.scr_key).iter().all(|&b| b == 0));
    assert!(key_bytes(&meta.scm_key).iter().any(|&b| b != 0));

    // unlock: the keys decrypt, check, and the mask key is forgotten
    sr_crypto_decrypt_key(sd, mdd).unwrap();
    assert_eq!(mdd.scr_maskkey.get(), [0; 32]);
    let plain = key_bytes(&mdd.scr_key);
    assert!(plain.iter().any(|&b| b != 0));
    // they are the masked keys under AES-256-ECB
    let again = cells(&[0; SR_CRYPTO_KEYSZ]);
    sr_crypto_encrypt(&cells(&plain), &again, &MASK_A, SR_CRYPTOM_AES_ECB_256).unwrap();
    assert_eq!(bytes(&again), key_bytes(&meta.scm_key));
    // and the stored check is their HMAC
    let mut digest = [0u8; SHA1_DIGEST_LENGTH];
    sr_crypto_calculate_check_hmac_sha1(&MASK_A, &cells(&plain), &mut digest);
    assert_eq!(meta.chk_hmac_sha1().sch_mac, digest);

    // a wrong mask key fails the check and leaves no key behind
    mdd.scr_maskkey.set(MASK_B);
    assert_eq!(sr_crypto_decrypt_key(sd, mdd), Err(Errno::EIO));
    assert!(key_bytes(&mdd.scr_key).iter().all(|&b| b == 0));
    assert_eq!(mdd.scr_maskkey.get(), [0; 32]);

    // the check algorithm must be known
    meta.scm_check_alg.set(0);
    mdd.scr_maskkey.set(MASK_A);
    assert_eq!(sr_crypto_decrypt_key(sd, mdd), Err(Errno::EIO));
}

#[test]
fn change_maskkey_rewraps_the_same_keys() {
    let _g = setup_real_memory();
    let sd = crypto_volume();
    let mdd = &sd.mds().mdd_crypto;
    let meta = mdd.scr_meta().unwrap();

    mdd.scr_maskkey.set(MASK_A);
    sr_crypto_create_keys(sd, mdd).unwrap();
    mdd.scr_maskkey.set(MASK_A);
    sr_crypto_decrypt_key(sd, mdd).unwrap();
    let plain = key_bytes(&mdd.scr_key);

    let mut k1 = SrCryptoKdfinfo {
        maskkey: MASK_A,
        ..SrCryptoKdfinfo::default()
    };
    let mut k2 = SrCryptoKdfinfo {
        flags: SR_CRYPTOKDF_KEY | SR_CRYPTOKDF_HINT,
        maskkey: MASK_B,
        ..SrCryptoKdfinfo::default()
    };
    k2._kdfhint.generic.len = size_of::<SrCryptoPbkdf>() as u32;
    k2._kdfhint.generic.r#type = SR_CRYPTOKDFT_BCRYPT_PBKDF;
    k2._kdfhint.rounds = 16;
    k2._kdfhint.salt = [0x77; 128];
    let hint = kdfinfo_to_bytes(&k2)[40..180].to_vec();
    sr_crypto_change_maskkey(sd, mdd, &mut k1, &mut k2).unwrap();
    assert_eq!(k1.maskkey, [0; 32]);
    assert_eq!(k2.maskkey, [0; 32]);
    let kdfhint = meta.scm_kdfhint.get();
    assert_eq!(&kdfhint[..140], &hint[..]);
    assert!(kdfhint[140..].iter().all(|&b| b == 0));

    // the new mask key unlocks the same keys, the old one no more
    mdd.scr_maskkey.set(MASK_B);
    sr_crypto_decrypt_key(sd, mdd).unwrap();
    assert_eq!(key_bytes(&mdd.scr_key), plain);
    mdd.scr_maskkey.set(MASK_A);
    assert_eq!(sr_crypto_decrypt_key(sd, mdd), Err(Errno::EIO));

    // a wrong old mask key is EPERM and changes nothing
    mdd.scr_maskkey.set(MASK_B);
    sr_crypto_decrypt_key(sd, mdd).unwrap();
    let masked = key_bytes(&meta.scm_key);
    let mut k1 = SrCryptoKdfinfo {
        maskkey: MASK_A,
        ..SrCryptoKdfinfo::default()
    };
    let mut k2 = SrCryptoKdfinfo {
        maskkey: [1; 32],
        ..SrCryptoKdfinfo::default()
    };
    assert_eq!(
        sr_crypto_change_maskkey(sd, mdd, &mut k1, &mut k2),
        Err(Errno::EPERM)
    );
    assert_eq!(key_bytes(&meta.scm_key), masked);
    assert_eq!((k1.maskkey, k2.maskkey), ([0; 32], [0; 32]));
}

#[test]
fn kdfinfo_bytes_round_trip() {
    let raw: [u8; KDFINFO_SIZE] = core::array::from_fn(|i| (i * 31 + 5) as u8);
    let k = kdfinfo_from_bytes(&raw);
    assert_eq!(k.len, u32::from_ne_bytes([raw[0], raw[1], raw[2], raw[3]]));
    assert_eq!(&k.maskkey[..], &raw[8..40]);
    assert_eq!(k._kdfhint.salt[127], raw[179]);
    assert_eq!(kdfinfo_to_bytes(&k), raw);

    let mut k = k;
    kdfinfo_bzero(&mut k);
    assert_eq!(kdfinfo_to_bytes(&k), [0; KDFINFO_SIZE]);

    let mut pair = [0u8; KDFPAIR_SIZE];
    pair[..8].copy_from_slice(&0x1000usize.to_ne_bytes());
    pair[8..12].copy_from_slice(&180u32.to_ne_bytes());
    pair[16..24].copy_from_slice(&0x2000usize.to_ne_bytes());
    pair[24..28].copy_from_slice(&176u32.to_ne_bytes());
    let p = kdfpair_from_bytes(&pair);
    assert_eq!(
        (p.kdfinfo1, p.kdfsize1, p.kdfinfo2, p.kdfsize2),
        (0x1000, 180, 0x2000, 176)
    );
}

/// A `bioc_createraid` handing `kdf` in (`BIOC_SOIN`).
fn createraid_in(raw: &mut [u8; KDFINFO_SIZE]) -> BiocCreateraid {
    // SAFETY: a plain C structure of integers and null pointers: all-zero bytes are a value.
    let mut bc: BiocCreateraid = unsafe { core::mem::zeroed() };
    bc.bc_key_disk = NODEV;
    bc.bc_opaque_flags = BIOC_SOIN;
    bc.bc_opaque_size = KDFINFO_SIZE as u32;
    bc.bc_opaque = raw.as_mut_ptr().cast();
    bc
}

#[test]
fn get_kdf_takes_the_hint_and_the_mask_key() {
    let _g = setup_real_memory();
    let sd = crypto_volume();
    let mdd = &sd.mds().mdd_crypto;

    let mut k = SrCryptoKdfinfo {
        len: KDFINFO_SIZE as u32,
        flags: SR_CRYPTOKDF_KEY | SR_CRYPTOKDF_HINT,
        maskkey: MASK_B,
        ..SrCryptoKdfinfo::default()
    };
    k._kdfhint.generic.len = 8;
    k._kdfhint.generic.r#type = SR_CRYPTOKDFT_KEYDISK;
    let mut raw = kdfinfo_to_bytes(&k);
    let mut bc = createraid_in(&mut raw);
    sr_crypto_get_kdf(&mut bc, sd, mdd).unwrap();
    assert_eq!(bc.bc_opaque_status, BIOC_SOINOUT_OK);
    assert_eq!(mdd.scr_maskkey.get(), MASK_B);
    assert_eq!(
        &mdd.scr_meta().unwrap().scm_kdfhint.get()[..8],
        &raw[40..48]
    );

    // the length inside must match, and the size, and the direction
    k.len = 4;
    let mut raw = kdfinfo_to_bytes(&k);
    let mut bc = createraid_in(&mut raw);
    assert_eq!(sr_crypto_get_kdf(&mut bc, sd, mdd), Err(Errno::EINVAL));
    bc.bc_opaque_size = 4;
    assert_eq!(sr_crypto_get_kdf(&mut bc, sd, mdd), Err(Errno::EINVAL));
    bc.bc_opaque_flags = BIOC_SOOUT;
    assert_eq!(sr_crypto_get_kdf(&mut bc, sd, mdd), Err(Errno::EINVAL));
}

#[test]
fn meta_create_makes_a_passphrase_volume() {
    let _g = setup_real_memory();
    let sd = discipline();
    sr_crypto_discipline_init(sd);
    sd.sd_meta().ssdi().ssd_size.set(2048);
    let mdd = &sd.mds().mdd_crypto;

    let k = SrCryptoKdfinfo {
        len: KDFINFO_SIZE as u32,
        flags: SR_CRYPTOKDF_KEY,
        maskkey: MASK_A,
        ..SrCryptoKdfinfo::default()
    };
    let mut raw = kdfinfo_to_bytes(&k);

    // bioctl first asks for a hint: none yet
    let mut bc = createraid_in(&mut raw);
    bc.bc_opaque_flags = BIOC_SOOUT;
    assert_eq!(sr_crypto_meta_create(sd, mdd, &mut bc), Err(Errno::EAGAIN));
    assert_eq!(bc.bc_opaque_status, BIOC_SOINOUT_FAILED);

    // a passphrase volume must not be auto-assembled
    let sd = discipline();
    sr_crypto_discipline_init(sd);
    let mdd = &sd.mds().mdd_crypto;
    let mut bc = createraid_in(&mut raw);
    bc.bc_flags = BIOC_SCNOAUTOASSEMBLE;
    sr_crypto_meta_create(sd, mdd, &mut bc).unwrap();
    assert_eq!(sd.sd_meta().ssdi().ssd_opt_no.get(), 1);
    let omi = sd.sd_meta_opt.first().unwrap();
    assert_eq!(omi.omi_som().som_type.get(), SR_OPT_CRYPTO);
    let meta = mdd.scr_meta().unwrap();
    assert!(core::ptr::eq(meta, omi.som_as::<SrMetaCrypto>().unwrap()));
    assert_eq!(meta.scm_flags.get(), SR_CRYPTOF_KEY | SR_CRYPTOF_KDFHINT);
    mdd.scr_maskkey.set(MASK_A);
    sr_crypto_decrypt_key(sd, mdd).unwrap();
}

#[test]
fn set_key_paths() {
    let _g = setup_real_memory();
    let sd = crypto_volume();
    let mdd = &sd.mds().mdd_crypto;
    let mut hint = [0u8; SR_CRYPTO_KDFHINTBYTES];
    hint[..4].copy_from_slice(&[1, 2, 3, 4]);
    mdd.scr_meta().unwrap().scm_kdfhint.set(hint);
    let mut raw = [0u8; KDFINFO_SIZE];

    // the boot loader's key
    let mut bc = createraid_in(&mut raw);
    sr_crypto_set_key(sd, mdd, &mut bc, 1, Some(&MASK_B)).unwrap();
    assert_eq!(mdd.scr_maskkey.get(), MASK_B);
    assert_eq!(
        sr_crypto_set_key(sd, mdd, &mut bc, 1, Some(&MASK_B[..8])),
        Err(Errno::EINVAL)
    );

    // the hint goes out first
    let mut out = [0u8; 16];
    bc.bc_opaque_flags = BIOC_SOOUT;
    bc.bc_opaque = out.as_mut_ptr().cast();
    bc.bc_opaque_size = 16;
    assert_eq!(
        sr_crypto_set_key(sd, mdd, &mut bc, 1, None),
        Err(Errno::EAGAIN)
    );
    assert_eq!(bc.bc_opaque_status, BIOC_SOINOUT_OK);
    assert_eq!(&out[..4], &[1, 2, 3, 4]);
    bc.bc_opaque_size = 300;
    assert_eq!(
        sr_crypto_set_key(sd, mdd, &mut bc, 1, None),
        Err(Errno::EINVAL)
    );

    // nothing to go by
    bc.bc_opaque_flags = 0;
    assert_eq!(
        sr_crypto_set_key(sd, mdd, &mut bc, 1, None),
        Err(Errno::EINVAL)
    );
}

#[test]
fn get_kdfhint_ioctl() {
    let _g = setup_real_memory();
    let sd = crypto_volume();
    let mut hint = [0u8; SR_CRYPTO_KDFHINTBYTES];
    hint[250] = 9;
    sd.mds()
        .mdd_crypto
        .scr_meta()
        .unwrap()
        .scm_kdfhint
        .set(hint);

    let mut out = [0u8; SR_CRYPTO_KDFHINTBYTES];
    // SAFETY: a plain C structure of integers and null pointers: all-zero bytes are a value.
    let mut bd: BiocDiscipline = unsafe { core::mem::zeroed() };
    bd.bd_cmd = SR_IOCTL_GET_KDFHINT;
    bd.bd_size = SR_CRYPTO_KDFHINTBYTES as u32;
    bd.bd_data = out.as_mut_ptr().cast();
    sr_crypto_ioctl(sd, &mut bd).unwrap();
    assert_eq!(out, hint);

    bd.bd_size += 1;
    assert_eq!(sr_crypto_ioctl(sd, &mut bd), Err(Errno::EIO));
    bd.bd_cmd = 0x77;
    assert_eq!(sr_crypto_ioctl(sd, &mut bd), Err(Errno::EIO));
}

#[test]
fn meta_opt_handler_takes_only_the_crypto_item() {
    let _g = setup_real_memory();
    let sd = discipline();
    sr_crypto_discipline_init(sd);
    let mdd = &sd.mds().mdd_crypto;
    let kd = opt_item(SR_OPT_KEYDISK, size_of::<SrMetaKeydisk>());
    assert_eq!(sr_crypto_meta_opt_handler(sd, kd), Err(Errno::EINVAL));
    assert!(mdd.scr_meta().is_none());
    // too short to be one
    let short = opt_item(SR_OPT_CRYPTO, 64);
    assert_eq!(sr_crypto_meta_opt_handler(sd, short), Err(Errno::EINVAL));
    let cr = opt_item(SR_OPT_CRYPTO, size_of::<SrMetaCrypto>());
    sr_crypto_meta_opt_handler(sd, cr).unwrap();
    assert!(core::ptr::eq(
        mdd.scr_meta().unwrap(),
        cr.som_as::<SrMetaCrypto>().unwrap()
    ));
}

#[test]
fn discipline_init_sets_up_crypto() {
    let _g = setup_real_memory();
    let sd = discipline();
    sr_crypto_discipline_init(sd);
    assert_eq!(sd.sd_type.get(), SR_MD_CRYPTO);
    assert_eq!(&sd.sd_name.get()[..7], b"CRYPTO\0");
    assert_eq!(sd.sd_max_wu.get(), SR_CRYPTO_NOWU);
    assert_eq!(sd.sd_wu_size(), size_of::<SrCryptoWuReq>());
    assert!(
        sd.mds()
            .mdd_crypto
            .scr_sid
            .iter()
            .all(|s| s.get() == u64::MAX)
    );
    assert!(sd.sd_scsi_rw.get().is_some() && sd.sd_scsi_done.get().is_some());
    assert!(sd.sd_scsi_wu_done.get().is_none());
}

/// The XTS key of the reference (`bytes(i * 3 + 1)`), its plain text (`bytes(i * 13 + 7)`)
/// over two sectors, and the reference ciphertext of those two sectors at blocks 5 and 6
/// (`d3-tools/xts.py`: IEEE 1619 with openssl's AES-256-ECB): its SHA1 and the first 16
/// bytes of each sector.
const XTS_SHA1: &str = "9cada410012f6ea3ce8a2e1fe999ca41b84ba6af";
const XTS_SECTOR0: &str = "ad119307df6b51ba05c466c1c852bc37";
const XTS_SECTOR1: &str = "4a526b9a9942f899614c13628e586b2c";

fn xts_plain() -> Vec<u8> {
    (0..1024).map(|i| (i * 13 + 7) as u8).collect()
}

/// A volume with the reference key as data key 0 and its software crypto session, and a
/// work unit (leaked) with its buffer and descriptors as `sr_crypto_alloc_resources`
/// leaves it.
fn xts_volume() -> (&'static SrDiscipline, &'static SrCryptoWuReq) {
    let sd = discipline();
    sr_crypto_discipline_init(sd);
    let mdd = &sd.mds().mdd_crypto;
    mdd.scr_alg.set(CRYPTO_AES_XTS);
    mdd.scr_klen.set(512);
    let key: [u8; 64] = core::array::from_fn(|i| (i * 3 + 1) as u8);
    mdd.scr_key[0].set(key);
    let cri = Cryptoini {
        cri_alg: CRYPTO_AES_XTS,
        cri_klen: 512,
        cri_key: &key,
        ..Cryptoini::default()
    };
    mdd.scr_sid[0].set(crypto_newsession(&cri, 0).unwrap());

    // SAFETY: a zeroed crypto work unit (`SrZeroed`), leaked.
    let crwu: &'static SrCryptoWuReq =
        unsafe { sr_malloc::<SrCryptoWuReq>(M_WAITOK).unwrap().as_ref() };
    crwu.cr.cr_wu.swu_dis.set(sd);
    crwu.cr
        .cr_dmabuf
        .set(malloc(MAXPHYS, M_DEVBUF, M_WAITOK).unwrap().as_ptr());
    let mut crp = crypto_getreq(SR_CRYPTO_NDESC as i32).unwrap();
    crwu.crp_put(core::mem::take(&mut crp.crp_desc));
    (sd, crwu)
}

#[test]
fn sectors_are_aes_xts_with_the_block_number_as_tweak() {
    let _g = setup_real_memory();
    let _s = serial();
    crypto_reset();
    swcr_reset();
    swcr_init();
    let (sd, crwu) = xts_volume();
    let mdd = &sd.mds().mdd_crypto;

    let plain = xts_plain();
    let mut buf = plain.clone();
    sr_crypto_request(crwu, mdd, &mut buf, 5, true, crypto_invoke)
        .unwrap()
        .unwrap();
    assert_eq!(sha1(&buf).to_vec(), hex(XTS_SHA1));
    assert_eq!(buf[..16].to_vec(), hex(XTS_SECTOR0));
    assert_eq!(buf[512..528].to_vec(), hex(XTS_SECTOR1));

    // the same plain text at another block is another ciphertext
    let mut other = plain.clone();
    sr_crypto_request(crwu, mdd, &mut other, 6, true, crypto_invoke)
        .unwrap()
        .unwrap();
    assert_ne!(other, buf);

    sr_crypto_request(crwu, mdd, &mut buf, 5, false, crypto_invoke)
        .unwrap()
        .unwrap();
    assert_eq!(buf, plain);

    // the descriptors were given back, empty, their allocation kept
    let descs: Vec<Cryptodesc<'_>> = crwu.crp_take().unwrap();
    assert!(descs.is_empty() && descs.capacity() >= SR_CRYPTO_NDESC);
    crwu.crp_put(descs);

    // the request looks as the C's does
    let mut seen = Vec::new();
    sr_crypto_request(crwu, mdd, &mut buf, 0x1234, true, |crp| {
        seen.push((crp.crp_ndesc, crp.crp_ilen, crp.crp_flags, crp.crp_sid));
        for (i, d) in crp.crp_desc.iter().enumerate() {
            let mut iv = [0u8; 8];
            iv.copy_from_slice(&d.crd_iv()[..8]);
            assert_eq!(Daddr::from_ne_bytes(iv), 0x1234 + i as Daddr);
            assert_eq!(d.crd_skip, (i * DEV_BSIZE) as i32);
            assert_eq!(d.crd_len, DEV_BSIZE as i32);
            assert_eq!(
                d.crd_flags,
                CRD_F_ENCRYPT | CRD_F_IV_PRESENT | CRD_F_IV_EXPLICIT
            );
            assert_eq!(d.CRD_INI.cri_alg, CRYPTO_AES_XTS);
            assert_eq!(d.CRD_INI.cri_klen, 512);
            assert_eq!(d.CRD_INI.cri_key.len(), SR_CRYPTO_KEYBYTES);
        }
        Ok(())
    })
    .unwrap()
    .unwrap();
    assert_eq!(seen, vec![(2, 1024, CRYPTO_F_IOV, mdd.scr_sid[0].get())]);

    // a block past the first 2^30 uses the next key's session (none here)
    let r = sr_crypto_request(
        crwu,
        mdd,
        &mut buf,
        1 << SR_CRYPTO_KEY_BLKSHIFT,
        true,
        crypto_invoke,
    )
    .unwrap();
    assert!(r.is_err());

    // a work unit without descriptors cannot run a request
    let none = crwu.crp_take().unwrap();
    assert!(sr_crypto_request(crwu, mdd, &mut buf, 5, true, crypto_invoke).is_none());
    crwu.crp_put(none);
}

#[test]
fn prepare_encrypts_writes_aside_and_decrypts_reads_in_place() {
    let _g = setup_real_memory();
    let _s = serial();
    crypto_reset();
    swcr_reset();
    swcr_init();
    let (sd, crwu) = xts_volume();
    let mdd = &sd.mds().mdd_crypto;
    let wu = &crwu.cr.cr_wu;
    wu.swu_blk_start.set(5);
    let xs: &'static ScsiXfer = Box::leak(Box::new(ScsiXfer::new()));
    wu.swu_xs.set(Some(xs));

    // a write: the transfer's data is left alone, the work unit's buffer is encrypted
    let plain = xts_plain();
    let data: &'static mut [u8] = plain.clone().leak();
    xs.flags.set(SCSI_DATA_OUT);
    // SAFETY: a leaked buffer of 1024 bytes that only this transfer uses.
    unsafe { xs.set_data(data.as_mut_ptr(), 1024) };
    let (got, rv) = sr_crypto_prepare(wu, mdd, true, crypto_invoke);
    rv.unwrap();
    assert!(core::ptr::eq(got, crwu));
    // SAFETY: the work unit's MAXPHYS buffer, not in use now.
    let dma = unsafe { slice::from_raw_parts(crwu.cr_dmabuf(), 1024) };
    assert_eq!(sha1(dma).to_vec(), hex(XTS_SHA1));
    // SAFETY: the test owns the transfer.
    assert_eq!(unsafe { xs.data_slice() }.to_vec(), plain);

    // the write's ccb writes the encrypted buffer
    let ccb: &'static SrCcb = Box::leak(Box::new(SrCcb::new()));
    ccb_write_crypted(ccb, Some(crwu));
    assert_eq!(ccb.ccb_buf.b_data.get(), crwu.cr_dmabuf());
    assert_eq!(
        ccb.ccb_opaque.get(),
        ptr::from_ref(crwu).cast_mut().cast::<c_void>()
    );

    // a read of what was written is decrypted in place
    let back: &'static mut [u8] = dma.to_vec().leak();
    xs.flags.set(SCSI_DATA_IN);
    // SAFETY: as above.
    unsafe { xs.set_data(back.as_mut_ptr(), 1024) };
    let (_, rv) = sr_crypto_prepare(wu, mdd, false, crypto_invoke);
    rv.unwrap();
    // SAFETY: the test owns the transfer.
    assert_eq!(unsafe { xs.data_slice() }.to_vec(), plain);

    // a rebuild's work unit is not touched (RAID 1C)
    wu.swu_flags.set(SR_WUF_REBUILD);
    xs.error.set(XS_NOERROR);
    sr_crypto_done_internal(wu, mdd);
    // SAFETY: the test owns the transfer.
    assert_eq!(unsafe { xs.data_slice() }.to_vec(), plain);
}

#[test]
fn free_sessions_forgets_them() {
    let _g = setup_real_memory();
    let _s = serial();
    crypto_reset();
    swcr_reset();
    swcr_init();
    let (sd, _crwu) = xts_volume();
    let mdd = &sd.mds().mdd_crypto;
    assert_ne!(mdd.scr_sid[0].get(), u64::MAX);
    sr_crypto_free_sessions(sd, mdd);
    assert!(mdd.scr_sid.iter().all(|s| s.get() == u64::MAX));
}

#[test]
fn old_keydisk_offset_is_the_cs_pointer_arithmetic() {
    // `omh + sizeof(struct sr_meta_opt_hdr)` with `omh` a `struct sr_meta_opt_hdr *`
    assert_eq!(size_of::<SrMetaOptHdr>(), 24);
    assert_eq!(OLD_KEYDISK_MASKKEY_OFFSET, 576);
    assert!(OLD_KEYDISK_MASKKEY_OFFSET + SR_CRYPTO_MAXKEYBYTES <= size_of::<SrMetaCrypto>());
}
