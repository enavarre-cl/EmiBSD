//! Tests of the software crypto driver through the framework (`crypto_newsession` and
//! `crypto_invoke`) with known answers: AES-CBC (NIST SP 800-38A), AES-CTR (RFC 3686), AES-XTS
//! (IEEE 1619), the HMACs, AES-GCM (the GCM specification's test case 4 in the layout of an ESP
//! packet), ChaCha20-Poly1305 (RFC 8439 section 2.8.2), 3DES, Blowfish and CAST-128 in CBC
//! mode (`openssl enc`), over buffers that are one iovec, several iovecs and chains of mbufs
//! with blocks straddling the segments.

use core::ffi::c_void;
use std::sync::MutexGuard;
use std::vec::Vec;
use std::{assert, assert_eq, vec};

use super::*;
use crate::crypto::crypto::crypto_freesession;
use crate::crypto::crypto::{
    crypto_freereq, crypto_getreq, crypto_invoke, crypto_newsession, crypto_reset,
};
use crate::crypto::cryptodev::CRYPTO_F_IOV;
use crate::crypto::testutil::hex;
use crate::kern::uipc_mbuf::tests::setup as mbuf_setup;
use crate::kern::uipc_mbuf::{m_copydata as mbuf_copydata, m_get, m_gethdr};
use crate::sys::mbuf::{M_DONTWAIT, MT_DATA, Mbuf, mclgetl};
use crate::sys::uio::{Iovec, UioRw, UioSeg};

/// The framework's state is global: one test at a time, each from an empty driver table with
/// the software driver registered.
fn fw() -> MutexGuard<'static, ()> {
    let g = crate::crypto::testutil::serial();
    crypto_reset();
    swcr_reset();
    swcr_init();
    g
}

fn ini<'a>(alg: i32, key: &'a [u8], next: Option<&'a Cryptoini<'a>>) -> Cryptoini<'a> {
    Cryptoini {
        cri_alg: alg,
        cri_klen: key.len() as i32 * 8,
        cri_key: key,
        cri_next: next,
        ..Cryptoini::default()
    }
}

fn desc(alg: i32, skip: i32, len: i32, inject: i32, flags: i32) -> Cryptodesc<'static> {
    Cryptodesc {
        crd_skip: skip,
        crd_len: len,
        crd_inject: inject,
        crd_flags: flags,
        CRD_INI: Cryptoini {
            cri_alg: alg,
            ..Cryptoini::default()
        },
    }
}

fn with_iv(mut d: Cryptodesc<'static>, iv: &[u8]) -> Cryptodesc<'static> {
    d.crd_iv_mut()[..iv.len()].copy_from_slice(iv);
    d
}

/// Runs the request over `buf` split into iovecs of the given lengths.
fn run_iov(
    sid: u64,
    buf: &mut [u8],
    cuts: &[usize],
    descs: &[Cryptodesc<'static>],
    mac: Option<&mut [u8]>,
) -> Result<(), Errno> {
    let len = buf.len();
    let base = buf.as_mut_ptr();
    let mut iov: Vec<Iovec> = Vec::new();
    let mut off = 0;
    for &c in cuts {
        iov.push(Iovec {
            iov_base: base.wrapping_add(off).cast::<c_void>(),
            iov_len: c,
        });
        off += c;
    }
    assert_eq!(off, len, "the cuts cover the buffer");
    let mut uio = Uio {
        uio_iov: &mut iov,
        uio_offset: 0,
        uio_resid: len,
        uio_segflg: UioSeg::UIO_SYSSPACE,
        uio_rw: UioRw::UIO_WRITE,
        uio_procp: None,
    };
    let mut crp = crypto_getreq(descs.len() as i32).expect("a request");
    crp.crp_desc.copy_from_slice(descs);
    crp.crp_sid = sid;
    crp.crp_ilen = len as i32;
    crp.crp_flags = CRYPTO_F_IOV;
    crp.crp_buf = CryptoBuf::Iov(&mut uio);
    crp.crp_mac = mac;
    let r = crypto_invoke(&mut crp);
    crypto_freereq(Some(crp));
    r
}

/// An mbuf chain with `data` split over segments of the given lengths (and room to grow).
fn chain(data: &[u8], cuts: &[usize]) -> &'static Mbuf {
    let mut top: Option<&'static Mbuf> = None;
    let mut last: Option<&'static Mbuf> = None;
    let mut off = 0;
    for &c in cuts {
        let m = if top.is_none() {
            m_gethdr(M_DONTWAIT, MT_DATA).expect("a header mbuf")
        } else {
            m_get(M_DONTWAIT, MT_DATA).expect("an mbuf")
        };
        if c > crate::kern::uipc_mbuf::m_trailingspace(m) as usize {
            assert!(mclgetl(m, M_DONTWAIT, c as u32).is_some());
        }
        m.m_len().set(c as u32);
        crate::kern::uipc_mbuf::m_copyback(m, 0, &data[off..off + c], M_DONTWAIT)
            .expect("copyback");
        off += c;
        match last {
            None => top = Some(m),
            Some(l) => l.m_next().set(Some(m)),
        }
        last = Some(m);
    }
    assert_eq!(off, data.len());
    top.expect("a chain")
}

fn chain_bytes(m: &Mbuf) -> Vec<u8> {
    let mut n = 0;
    let mut p = Some(m);
    while let Some(x) = p {
        n += x.m_len().get() as usize;
        p = x.m_next().get();
    }
    let mut v = vec![0u8; n];
    mbuf_copydata(m, 0, &mut v);
    v
}

/// Runs the request over the mbuf chain `m`.
fn run_mbuf(
    sid: u64,
    m: &'static Mbuf,
    len: usize,
    descs: &[Cryptodesc<'static>],
) -> Result<(), Errno> {
    let mut crp = crypto_getreq(descs.len() as i32).expect("a request");
    crp.crp_desc.copy_from_slice(descs);
    crp.crp_sid = sid;
    crp.crp_ilen = len as i32;
    crp.crp_flags = CRYPTO_F_IMBUF;
    crp.crp_buf = CryptoBuf::Mbuf(m);
    let r = crypto_invoke(&mut crp);
    crypto_freereq(Some(crp));
    r
}

fn pt48() -> Vec<u8> {
    (0..48).map(|i| ((i * 7 + 3) % 256) as u8).collect()
}

const ENC_EXPLICIT: i32 = CRD_F_ENCRYPT | CRD_F_IV_EXPLICIT | CRD_F_IV_PRESENT;
const DEC_EXPLICIT: i32 = CRD_F_IV_EXPLICIT;

/// CBC known answers: `(algorithm, key, ciphertext of pt48 under IV 00..)`.
fn cbc_cases() -> Vec<(i32, Vec<u8>, usize, Vec<u8>)> {
    let k16: Vec<u8> = (0..16).map(|i| ((i * 3 + 1) % 256) as u8).collect();
    let aes = |n: usize| -> Vec<u8> { (0..n).map(|i| ((i * 11 + 2) % 256) as u8).collect() };
    vec![
        (
            CRYPTO_3DES_CBC,
            hex("0123456789abcdef23456789abcdef01456789abcdef0123"),
            8,
            hex(
                "c1397d01f9d38a1c41b1b50eee2fba9b723441959e72ad963bf9e2da15a5038f8ad1a388f5275b36d17c8ee71c8bac06",
            ),
        ),
        (
            CRYPTO_BLF_CBC,
            k16.clone(),
            8,
            hex(
                "22b7a722c76dcffdc66dbfd14485b0a1e1057ffa301f38d94236fdf655bf70a417e675cfd291cde2f254b88d5fa46d81",
            ),
        ),
        (
            CRYPTO_CAST_CBC,
            k16,
            8,
            hex(
                "9dc49d75637f30360174714cfae00e0337e008494367c6702b2df927950d05ba627abb6297818c3051746f0f68c4c3ec",
            ),
        ),
        (
            CRYPTO_AES_CBC,
            aes(16),
            16,
            hex(
                "399373ba48aa7aad4eef24f0fa0695894e7eefaa20301c0554766e08ce2c5df58c335a96b08765f39303d49ae70aac7b",
            ),
        ),
        (
            CRYPTO_AES_CBC,
            aes(24),
            16,
            hex(
                "29481826e36ac4c5173b0ddfe1dde381dfdac2ace9f9603903445a2731466063431c4a852c2d64d47f9efc5b5f5ab5e5",
            ),
        ),
        (
            CRYPTO_AES_CBC,
            aes(32),
            16,
            hex(
                "e9e2eb941a89270076e2dba770763c262a89e5c16cffa117ff754e0523da217efe6c3a49d188fdfa75716e01be75e537",
            ),
        ),
    ]
}

#[test]
fn cbc_ciphers_over_one_and_several_iovecs() {
    let _g = fw();
    for (alg, key, ivlen, want) in cbc_cases() {
        let c = ini(alg, &key, None);
        let sid = crypto_newsession(&c, 0).expect("a session");
        let iv: Vec<u8> = (0..ivlen as u8).collect();
        let enc = with_iv(desc(alg, 0, 48, 0, ENC_EXPLICIT), &iv);
        let dec = with_iv(desc(alg, 0, 48, 0, DEC_EXPLICIT), &iv);

        // Odd cuts put blocks across the iovecs.
        for cuts in [
            vec![48usize],
            vec![7, 25, 16],
            vec![1, 47],
            vec![20, 0, 28],
            vec![3, 3, 3, 39],
        ] {
            let mut buf = pt48();
            run_iov(sid, &mut buf, &cuts, &[enc], None).expect("encrypt");
            assert_eq!(buf, want, "alg {alg} key {} cuts {cuts:?}", key.len());
            run_iov(sid, &mut buf, &cuts, &[dec], None).expect("decrypt");
            assert_eq!(buf, pt48(), "alg {alg} back, cuts {cuts:?}");
        }
        crypto_freesession(sid).expect("free");
    }
}

#[test]
fn cbc_over_mbuf_chains_with_straddling_blocks() {
    let _g = fw();
    let _m = mbuf_setup();
    for (alg, key, ivlen, want) in cbc_cases() {
        let c = ini(alg, &key, None);
        let sid = crypto_newsession(&c, 0).expect("a session");
        let iv: Vec<u8> = (0..ivlen as u8).collect();
        let enc = with_iv(desc(alg, 0, 48, 0, ENC_EXPLICIT), &iv);
        let dec = with_iv(desc(alg, 0, 48, 0, DEC_EXPLICIT), &iv);

        for cuts in [
            vec![48usize],
            vec![5, 16, 11, 16],
            vec![1, 47],
            vec![17, 17, 14],
            vec![16, 0, 16, 0, 16],
            vec![0, 48],
        ] {
            let m = chain(&pt48(), &cuts);
            run_mbuf(sid, m, 48, &[enc]).expect("encrypt");
            assert_eq!(chain_bytes(m), want, "alg {alg} cuts {cuts:?}");
            run_mbuf(sid, m, 48, &[dec]).expect("decrypt");
            assert_eq!(chain_bytes(m), pt48(), "alg {alg} back, cuts {cuts:?}");
            crate::kern::uipc_mbuf::m_freem(m);
        }
        crypto_freesession(sid).expect("free");
    }
}

#[test]
fn skip_and_a_partial_range_leave_the_rest_alone() {
    let _g = fw();
    let key: Vec<u8> = (0..16).map(|i| ((i * 11 + 2) % 256) as u8).collect();
    let c = ini(CRYPTO_AES_CBC, &key, None);
    let sid = crypto_newsession(&c, 0).expect("a session");
    let iv: Vec<u8> = (0..16u8).collect();
    let mut buf: Vec<u8> = (0..70).map(|i| (i as u8).wrapping_mul(3)).collect();
    let orig = buf.clone();
    // Encrypt bytes 10..42 (two blocks) only.
    let enc = with_iv(desc(CRYPTO_AES_CBC, 10, 32, 0, ENC_EXPLICIT), &iv);
    run_iov(sid, &mut buf, &[30, 40], &[enc], None).expect("encrypt");
    assert_eq!(buf[..10], orig[..10]);
    assert_eq!(buf[42..], orig[42..]);
    assert_ne!(buf[10..42], orig[10..42]);
    // A length that is not a multiple of the block is refused before anything is touched.
    let bad = with_iv(desc(CRYPTO_AES_CBC, 0, 20, 0, ENC_EXPLICIT), &iv);
    let mut b2 = orig.clone();
    assert_eq!(
        run_iov(sid, &mut b2, &[70], &[bad], None),
        Err(Errno::EINVAL)
    );
    assert_eq!(b2, orig);
}

#[test]
fn the_iv_is_written_to_and_read_from_the_buffer() {
    let _g = fw();
    let key: Vec<u8> = (0..16).map(|i| ((i * 11 + 2) % 256) as u8).collect();
    let c = ini(CRYPTO_AES_CBC, &key, None);
    let sid = crypto_newsession(&c, 0).expect("a session");
    let iv: Vec<u8> = (0..16u8).collect();
    // IV explicit but not present: it is injected before the data, which starts at 16.
    let mut buf = vec![0u8; 16 + 48];
    buf[16..].copy_from_slice(&pt48());
    let enc = with_iv(
        desc(CRYPTO_AES_CBC, 16, 48, 0, CRD_F_ENCRYPT | CRD_F_IV_EXPLICIT),
        &iv,
    );
    run_iov(sid, &mut buf, &[64], &[enc], None).expect("encrypt");
    assert_eq!(buf[..16], iv[..]);
    assert_eq!(buf[16..], cbc_cases()[3].3[..]);
    // Decrypting without an explicit IV takes it from the buffer.
    let dec = desc(CRYPTO_AES_CBC, 16, 48, 0, 0);
    run_iov(sid, &mut buf, &[64], &[dec], None).expect("decrypt");
    assert_eq!(buf[16..], pt48()[..]);
}

#[test]
fn aes_ctr_rfc3686() {
    let _g = fw();
    // Test vector 1: 16-byte key and nonce, one block.
    let mut key = hex("ae6852f8121067cc4bf7a5765577f39e");
    key.extend_from_slice(&hex("00000030"));
    let c = ini(CRYPTO_AES_CTR, &key, None);
    let sid = crypto_newsession(&c, 0).expect("a session");
    let mut buf = b"Single block msg".to_vec();
    let d = with_iv(desc(CRYPTO_AES_CTR, 0, 16, 0, ENC_EXPLICIT), &[0; 8]);
    run_iov(sid, &mut buf, &[16], &[d], None).expect("encrypt");
    assert_eq!(buf, hex("e4095d4fb7a7b3792d6175a3261311b8"));
    // Test vector 2: two blocks; CTR is its own inverse.
    let mut key = hex("7e24067817fae0d743d6ce1f32539163");
    key.extend_from_slice(&hex("006cb6db"));
    let c = ini(CRYPTO_AES_CTR, &key, None);
    let sid = crypto_newsession(&c, 0).expect("a session");
    let mut buf: Vec<u8> = (0..32).collect();
    let d = with_iv(
        desc(CRYPTO_AES_CTR, 0, 32, 0, ENC_EXPLICIT),
        &hex("c0543b59da48d90b"),
    );
    run_iov(sid, &mut buf, &[11, 21], &[d], None).expect("encrypt");
    assert_eq!(
        buf,
        hex("5104a106168a72d9790d41ee8edad388eb2e1efc46da57c8fce630df9141be28")
    );
    let d = with_iv(
        desc(CRYPTO_AES_CTR, 0, 32, 0, DEC_EXPLICIT),
        &hex("c0543b59da48d90b"),
    );
    run_iov(sid, &mut buf, &[32], &[d], None).expect("decrypt");
    assert_eq!(buf, (0..32).collect::<Vec<u8>>());
    // A 256-bit key and 96 of 100 bytes (the length must be a multiple of the block).
    let mut key: Vec<u8> = (0..32).collect();
    key.extend_from_slice(&hex("01020304"));
    let c = ini(CRYPTO_AES_CTR, &key, None);
    let sid = crypto_newsession(&c, 0).expect("a session");
    let data: Vec<u8> = (0..100).map(|i| ((i * 5 + 1) % 256) as u8).collect();
    let want = hex(
        "da560817d9101eebd67de41557ac33a0e7f818c7817c306a347cb1fee5190d41d3533d079b2397d6d61bd7248ac8c3562f6210bab010f17c1dfd4a56a1f5229d27b7c8476d7ea8333d37bd993d6f548c881c06c88fd34aa4d1805defc92eff66",
    );
    let mut buf = data[..96].to_vec();
    let d = with_iv(
        desc(CRYPTO_AES_CTR, 0, 96, 0, ENC_EXPLICIT),
        &hex("1112131415161718"),
    );
    run_iov(sid, &mut buf, &[96], &[d], None).expect("encrypt");
    assert_eq!(buf, want);
    let mut bad = data.clone();
    let d = with_iv(
        desc(CRYPTO_AES_CTR, 0, 100, 0, ENC_EXPLICIT),
        &hex("1112131415161718"),
    );
    assert_eq!(
        run_iov(sid, &mut bad, &[100], &[d], None),
        Err(Errno::EINVAL)
    );
}

#[test]
fn aes_xts_ieee_1619() {
    let _g = fw();
    // Vector 1: zero keys, sector 0, 32 zero bytes.
    let c = ini(CRYPTO_AES_XTS, &[0; 32], None);
    let sid = crypto_newsession(&c, 0).expect("a session");
    let mut buf = vec![0u8; 32];
    let d = with_iv(desc(CRYPTO_AES_XTS, 0, 32, 0, ENC_EXPLICIT), &[0; 8]);
    run_iov(sid, &mut buf, &[32], &[d], None).expect("encrypt");
    assert_eq!(
        buf,
        hex("917cf69ebd68b2ec9b9fe9a3eadda692cd43d2f59598ed858c02c2652fbf922e")
    );
    let d = with_iv(desc(CRYPTO_AES_XTS, 0, 32, 0, DEC_EXPLICIT), &[0; 8]);
    run_iov(sid, &mut buf, &[32], &[d], None).expect("decrypt");
    assert_eq!(buf, vec![0u8; 32]);

    // The other key sizes and sectors against an independent XTS built on openssl's ECB.
    let pt: Vec<u8> = (0..64).map(|i| ((i * 3 + 1) % 256) as u8).collect();
    let k32: Vec<u8> = (0..32).map(|i| ((i * 13 + 5) % 256) as u8).collect();
    let k64: Vec<u8> = (0..64).map(|i| ((i * 13 + 5) % 256) as u8).collect();
    let cases = [
        (
            k32,
            0x1234u64,
            "74a72892904edcc17d28d41ddb175bb51febe143bfa6661c779b3ca05587fb679409eadc94e0769b62df63aed5bdff4cb1ea7493bf3865d8d32639bf7a8824d0",
        ),
        (
            k64,
            7,
            "cc6d52ebeb292ad3824b0e33e4a89f1b9925551c4347549fbe9ef6d3aac71b217c5bffe04ebdd79528b7ff82d745967955ae8c7b04de05794c21d0554e817661",
        ),
    ];
    for (key, sector, want) in cases {
        let c = ini(CRYPTO_AES_XTS, &key, None);
        let sid = crypto_newsession(&c, 0).expect("a session");
        let mut buf = pt.clone();
        let d = with_iv(
            desc(CRYPTO_AES_XTS, 0, 64, 0, ENC_EXPLICIT),
            &sector.to_le_bytes(),
        );
        run_iov(sid, &mut buf, &[9, 55], &[d], None).expect("encrypt");
        assert_eq!(buf, hex(want), "key {}", key.len());
    }
    // A key that is not 32 or 64 bytes is refused when the session is made.
    let c = ini(CRYPTO_AES_XTS, &[0; 48], None);
    assert_eq!(crypto_newsession(&c, 0), Err(Errno::EINVAL));
}

#[test]
fn null_transform_leaves_the_data() {
    let _g = fw();
    let c = ini(CRYPTO_NULL, &[], None);
    let sid = crypto_newsession(&c, 0).expect("a session");
    let mut buf = pt48();
    let d = with_iv(desc(CRYPTO_NULL, 0, 48, 0, ENC_EXPLICIT), &[0; 4]);
    run_iov(sid, &mut buf, &[48], &[d], None).expect("process");
    assert_eq!(buf, pt48());
}

/// `(algorithm, MAC of 77 bytes under a 24-byte key, bytes on the wire)`.
fn hmac_cases() -> Vec<(i32, &'static str, usize)> {
    vec![
        (CRYPTO_MD5_HMAC, "686a920987b128c29f2168fff2f292e4", 12),
        (
            CRYPTO_SHA1_HMAC,
            "566ab0b17c5b5e6ba9383fb40f66129075aaebde",
            12,
        ),
        (
            CRYPTO_RIPEMD160_HMAC,
            "f108a58c711e47cc18cd396761dc61e0e7f8830c",
            12,
        ),
        (
            CRYPTO_SHA2_256_HMAC,
            "273b13b4dba37652f2689dddb7b80f681ca6c77efb33bae7eab7f54e16a2d385",
            16,
        ),
        (
            CRYPTO_SHA2_384_HMAC,
            "544afb91fb4d2b8e08351dc4778f26e0373349e6e3d49d2d13c2a30a74217b532b3e702fa522be4deeded334525b5ac2",
            24,
        ),
        (
            CRYPTO_SHA2_512_HMAC,
            "db8ef2a017e6f2d7fd3385ad28224c68edef96d329df08f73f08c18d103c078fed91e2bd1b54718e583de1190c7c5c77d3b648effb8e2154d9634317e85f4fae",
            32,
        ),
    ]
}

fn hmac_msg() -> Vec<u8> {
    (0..77).map(|i| ((i * 9 + 4) % 256) as u8).collect()
}

fn hmac_key() -> Vec<u8> {
    (0..24).map(|i| ((i * 5 + 7) % 256) as u8).collect()
}

#[test]
fn hmacs_into_the_mac_slot_of_an_iovec_request() {
    let _g = fw();
    for (alg, want, authsize) in hmac_cases() {
        let key = hmac_key();
        let c = ini(alg, &key, None);
        let sid = crypto_newsession(&c, 0).expect("a session");
        let mut buf = hmac_msg();
        let mut mac = vec![0xeeu8; 40];
        // Authenticate bytes 0..77, in three iovecs.
        let d = desc(alg, 0, 77, 0, 0);
        run_iov(sid, &mut buf, &[10, 40, 27], &[d], Some(&mut mac)).expect("hmac");
        assert_eq!(buf, hmac_msg(), "the data is untouched");
        assert_eq!(mac[..authsize], hex(want)[..authsize], "alg {alg}");
        assert!(
            mac[authsize..].iter().all(|b| *b == 0xee),
            "nothing past authsize"
        );
        // A skip: authenticating a suffix equals authenticating it alone.
        let mut data = vec![0xaau8; 5];
        data.extend_from_slice(&hmac_msg());
        let d = desc(alg, 5, 77, 0, 0);
        let mut mac2 = vec![0u8; 40];
        run_iov(sid, &mut data, &[30, 52], &[d], Some(&mut mac2)).expect("hmac");
        assert_eq!(mac2[..authsize], mac[..authsize]);
    }
}

#[test]
fn hmacs_injected_into_an_mbuf_chain() {
    let _g = fw();
    let _m = mbuf_setup();
    for (alg, want, authsize) in hmac_cases() {
        let key = hmac_key();
        let c = ini(alg, &key, None);
        let sid = crypto_newsession(&c, 0).expect("a session");
        let m = chain(&hmac_msg(), &[13, 30, 34]);
        // Inject at the end: the chain grows by the MAC.
        let d = desc(alg, 0, 77, 77, 0);
        run_mbuf(sid, m, 77, &[d]).expect("hmac");
        let bytes = chain_bytes(m);
        assert_eq!(bytes.len(), 77 + authsize, "alg {alg}");
        assert_eq!(bytes[..77], hmac_msg()[..]);
        assert_eq!(bytes[77..], hex(want)[..authsize], "alg {alg}");
        crate::kern::uipc_mbuf::m_freem(m);
    }
}

#[test]
fn hmac_keys_of_up_to_a_block_and_not_more() {
    let _g = fw();
    // MD5's block is 64 bytes: a 64-byte key is used as is.
    let key64: Vec<u8> = (0..64).map(|i| ((i * 5 + 7) % 256) as u8).collect();
    let c = ini(CRYPTO_MD5_HMAC, &key64, None);
    let sid = crypto_newsession(&c, 0).expect("a session");
    let mut buf = hmac_msg();
    let mut mac = [0u8; 16];
    run_iov(
        sid,
        &mut buf,
        &[77],
        &[desc(CRYPTO_MD5_HMAC, 0, 77, 0, 0)],
        Some(&mut mac),
    )
    .expect("hmac");
    assert_eq!(mac[..12], hex("4ba61595f9dde4746b3bf94d9a2dff16")[..12]);
    // SHA-512's is 128.
    let key128: Vec<u8> = (0..128).map(|i| ((i * 5 + 7) % 256) as u8).collect();
    let c = ini(CRYPTO_SHA2_512_HMAC, &key128, None);
    let sid = crypto_newsession(&c, 0).expect("a session");
    let mut mac = [0u8; 32];
    run_iov(
        sid,
        &mut buf,
        &[77],
        &[desc(CRYPTO_SHA2_512_HMAC, 0, 77, 0, 0)],
        Some(&mut mac),
    )
    .expect("hmac");
    assert_eq!(
        mac[..],
        hex("2ea55a48adee55b97bbc0ad77b5f27f991a9057207918d7d5ddda0685a365c34")[..]
    );
    // Longer than the block (the C computes a negative pad length): refused.
    let c = ini(CRYPTO_MD5_HMAC, &[1u8; 65], None);
    assert_eq!(crypto_newsession(&c, 0), Err(Errno::EINVAL));
    // The caller's key is not modified (the C XORs it in place and back).
    let key = hmac_key();
    let c = ini(CRYPTO_SHA1_HMAC, &key, None);
    crypto_newsession(&c, 0).expect("a session");
    assert_eq!(key, hmac_key());
}

#[test]
fn esn_is_authenticated_after_the_data() {
    let _g = fw();
    // With CRD_F_ESN the 4 high bytes of the sequence number go in after the data: the MAC is
    // that of data || esn.
    let key = hmac_key();
    let c = ini(CRYPTO_SHA1_HMAC, &key, None);
    let sid = crypto_newsession(&c, 0).expect("a session");
    let mut with_esn = hmac_msg();
    let mut mac_a = [0u8; 12];
    let mut d = desc(CRYPTO_SHA1_HMAC, 0, 77, 0, CRD_F_ESN);
    d.set_crd_esn([1, 2, 3, 4]);
    run_iov(sid, &mut with_esn, &[77], &[d], Some(&mut mac_a)).expect("hmac");
    // The same MAC over the 81 bytes, directly.
    let mut data = hmac_msg();
    data.extend_from_slice(&[1, 2, 3, 4]);
    let mut mac_b = [0u8; 12];
    run_iov(
        sid,
        &mut data,
        &[81],
        &[desc(CRYPTO_SHA1_HMAC, 0, 81, 0, 0)],
        Some(&mut mac_b),
    )
    .expect("hmac");
    assert_eq!(mac_a, mac_b);
}

const GCM_KEY: &str = "feffe9928665731c6d6a8f9467308308";
const GCM_SALT: &str = "cafebabe";
const GCM_IV: &str = "facedbaddecaf888";
const GCM_AAD: &str = "feedfacedeadbeeffeedfacedeadbeefabaddad2";
const GCM_P: &str = "d9313225f88406e5a55909c5aff5269a86a7a9531534f7da2e4c303d8a318a721c3c0c95956809532fcf0e2449a6b525b16aedf5aa0de657ba637b391aafd255";
const GCM_C: &str = "42831ec2217774244b7221b784d0d49ce3aa212f2c02a4e035c17e2329aca12e21d514b25466931c7d8f6a5aac84aa051ba30b396a0aac973d58e091";
const GCM_TAG: &str = "5bc94fbc3221a5db94fae95ae7121a47";

/// A packet in the layout of ESP with an AEAD: `AAD (20) | IV (8) | payload (60) | tag (16)`.
fn gcm_packet(payload: &[u8]) -> Vec<u8> {
    let mut p = hex(GCM_AAD);
    p.extend_from_slice(&[0; 8]);
    p.extend_from_slice(payload);
    p.extend_from_slice(&[0; 16]);
    p
}

#[test]
fn aes_gcm_the_gcm_specification_test_case_4_in_an_esp_layout() {
    let _g = fw();
    let mut material = hex(GCM_KEY);
    material.extend_from_slice(&hex(GCM_SALT));
    let gmac = ini(CRYPTO_AES_128_GMAC, &material, None);
    let c = ini(CRYPTO_AES_GCM_16, &material, Some(&gmac));
    let sid = crypto_newsession(&c, 0).expect("a session");

    // Encrypt: the IV is injected before the payload; the tag of an iovec request goes to
    // `crp_mac` (an mbuf request gets it at `crd_inject`).
    let enc = with_iv(
        desc(
            CRYPTO_AES_GCM_16,
            28,
            60,
            20,
            CRD_F_ENCRYPT | CRD_F_IV_EXPLICIT,
        ),
        &hex(GCM_IV),
    );
    let auth = desc(CRYPTO_AES_128_GMAC, 0, 20, 88, 0);
    let mut pkt = gcm_packet(&hex(GCM_P)[..60]);
    let mut tag = [0u8; 16];
    run_iov(sid, &mut pkt, &[104], &[auth, enc], Some(&mut tag)).expect("encrypt");
    assert_eq!(pkt[20..28], hex(GCM_IV)[..], "the IV was written");
    assert_eq!(pkt[28..88], hex(GCM_C)[..]);
    assert_eq!(tag.to_vec(), hex(GCM_TAG));
    // Over several iovecs, the descriptors in the other order.
    let mut pkt = gcm_packet(&hex(GCM_P)[..60]);
    let mut tag = [0u8; 16];
    run_iov(sid, &mut pkt, &[50, 54], &[enc, auth], Some(&mut tag)).expect("encrypt");
    assert_eq!(pkt[28..88], hex(GCM_C)[..]);
    assert_eq!(tag.to_vec(), hex(GCM_TAG));
    // No MAC slot, no tag.
    let mut pkt = gcm_packet(&hex(GCM_P)[..60]);
    assert_eq!(
        run_iov(sid, &mut pkt, &[104], &[auth, enc], None),
        Err(Errno::EINVAL)
    );

    // Decrypt: the tag is computed over the ciphertext (the caller compares it), the payload
    // comes out as plaintext; the IV is taken from the packet.
    let mut pkt = gcm_packet(&hex(GCM_C));
    pkt[20..28].copy_from_slice(&hex(GCM_IV));
    let dec = desc(CRYPTO_AES_GCM_16, 28, 60, 20, 0);
    let mut tag = [0u8; 16];
    run_iov(sid, &mut pkt, &[104], &[auth, dec], Some(&mut tag)).expect("decrypt");
    assert_eq!(pkt[28..88], hex(GCM_P)[..60]);
    assert_eq!(tag.to_vec(), hex(GCM_TAG));
}

#[test]
fn aes_gcm_over_an_mbuf_chain() {
    let _g = fw();
    let _m = mbuf_setup();
    let mut material = hex(GCM_KEY);
    material.extend_from_slice(&hex(GCM_SALT));
    let gmac = ini(CRYPTO_AES_128_GMAC, &material, None);
    let c = ini(CRYPTO_AES_GCM_16, &material, Some(&gmac));
    let sid = crypto_newsession(&c, 0).expect("a session");
    let enc = with_iv(
        desc(
            CRYPTO_AES_GCM_16,
            28,
            60,
            20,
            CRD_F_ENCRYPT | CRD_F_IV_EXPLICIT,
        ),
        &hex(GCM_IV),
    );
    let auth = desc(CRYPTO_AES_128_GMAC, 0, 20, 88, 0);
    let pkt = gcm_packet(&hex(GCM_P)[..60]);
    let m = chain(&pkt, &[11, 25, 9, 59]);
    run_mbuf(sid, m, 104, &[auth, enc]).expect("encrypt");
    let out = chain_bytes(m);
    assert_eq!(out[28..88], hex(GCM_C)[..]);
    assert_eq!(out[88..], hex(GCM_TAG)[..]);
    crate::kern::uipc_mbuf::m_freem(m);
}

#[test]
fn gmac_only_with_the_esp_gmac_pair_and_the_esn_kludge() {
    let _g = fw();
    // ESP with AES-GMAC: the "encryption" descriptor has length 0 and the authenticator covers
    // the whole packet; the tag is that of an AAD-only GCM (RFC 4543).
    let mut material = hex(GCM_KEY);
    material.extend_from_slice(&hex(GCM_SALT));
    let gmac = ini(CRYPTO_AES_128_GMAC, &material, None);
    let c = ini(CRYPTO_AES_GMAC, &material, Some(&gmac));
    let sid = crypto_newsession(&c, 0).expect("a session");
    // Packet: AAD (20) | IV (8) | tag; the hashed data is the 20 bytes (the IV is a nonce
    // that is not authenticated as data).
    let mut pkt = hex(GCM_AAD);
    pkt.extend_from_slice(&[0; 8]);
    pkt.extend_from_slice(&[0; 16]);
    let enc = with_iv(
        desc(
            CRYPTO_AES_GMAC,
            28,
            0,
            20,
            CRD_F_ENCRYPT | CRD_F_IV_EXPLICIT,
        ),
        &hex(GCM_IV),
    );
    let auth = desc(CRYPTO_AES_128_GMAC, 0, 20, 28, 0);
    let mut tag = [0u8; 16];
    run_iov(sid, &mut pkt, &[44], &[auth, enc], Some(&mut tag)).expect("gmac");
    assert_eq!(pkt[20..28], hex(GCM_IV)[..]);
    assert_eq!(pkt[..20], hex(GCM_AAD)[..]);
    assert_eq!(tag.to_vec(), hex("346434fd51d5cd0c5887ec63e39b907a"));
}

#[test]
fn chacha20_poly1305_rfc8439_2_8_2_in_an_esp_layout() {
    let _g = fw();
    let mut material: Vec<u8> = (0..32).map(|i| 0x80 + i as u8).collect();
    material.extend_from_slice(&hex("07000000"));
    let mac = ini(CRYPTO_CHACHA20_POLY1305_MAC, &material, None);
    let c = ini(CRYPTO_CHACHA20_POLY1305, &material, Some(&mac));
    let sid = crypto_newsession(&c, 0).expect("a session");

    let aad = hex("50515253c0c1c2c3c4c5c6c7");
    let pt = b"Ladies and Gentlemen of the class of '99: If I could offer you only one tip for the future, sunscreen would be it.";
    let want = hex(
        "d31a8d34648e60db7b86afbc53ef7ec2a4aded51296e08fea9e2b5a736ee62d63dbea45e8ca9671282fafb69da92728b1a71de0a9e060b2905d6a5b67ecd3b3692ddbd7f2d778b8c9803aee328091b58fab324e4fad675945585808b4831d7bc3ff4def08e4b7a9de576d26586cec64b6116",
    );
    // AAD (12) | IV (8) | payload (114) | tag (16)
    let mut pkt = aad.clone();
    pkt.extend_from_slice(&[0; 8]);
    pkt.extend_from_slice(pt);
    pkt.extend_from_slice(&[0; 16]);
    let enc = with_iv(
        desc(
            CRYPTO_CHACHA20_POLY1305,
            20,
            114,
            12,
            CRD_F_ENCRYPT | CRD_F_IV_EXPLICIT,
        ),
        &hex("4041424344454647"),
    );
    let auth = desc(CRYPTO_CHACHA20_POLY1305_MAC, 0, 12, 134, 0);
    let mut tag = [0u8; 16];
    let total = pkt.len();
    run_iov(
        sid,
        &mut pkt,
        &[40, total - 40],
        &[auth, enc],
        Some(&mut tag),
    )
    .expect("encrypt");
    assert_eq!(pkt[20..134], want[..114]);
    assert_eq!(tag.to_vec(), hex("1ae10b594f09e26a7e902ecbd0600691"));

    // And back.
    let dec = desc(CRYPTO_CHACHA20_POLY1305, 20, 114, 12, 0);
    let mut tag2 = [0u8; 16];
    run_iov(sid, &mut pkt, &[total], &[auth, dec], Some(&mut tag2)).expect("decrypt");
    assert_eq!(pkt[20..134], pt[..]);
    assert_eq!(tag2, tag);
}

#[test]
fn session_errors() {
    let _g = fw();
    // An algorithm no driver supports.
    let c = ini(99, &[0; 16], None);
    assert_eq!(crypto_newsession(&c, 0), Err(Errno::EINVAL));
    // Deflate is not ported: the driver does not advertise it.
    let c = ini(CRYPTO_DEFLATE_COMP, &[], None);
    assert_eq!(crypto_newsession(&c, 0), Err(Errno::EINVAL));
    // Bad key sizes.
    let c = ini(CRYPTO_AES_CBC, &[0; 17], None);
    assert_eq!(crypto_newsession(&c, 0), Err(Errno::EINVAL));
    let c = ini(CRYPTO_3DES_CBC, &[0; 16], None);
    assert_eq!(crypto_newsession(&c, 0), Err(Errno::EINVAL));
    let c = ini(CRYPTO_AES_128_GMAC, &[0; 21], None);
    assert_eq!(crypto_newsession(&c, 0), Err(Errno::EINVAL));
    // A failed session does not use up a slot.
    assert_eq!(swcr_sesnum() as usize, CRYPTO_SW_SESSIONS);
    let used = with_sessions(|s| s.iter().filter(|l| !l.is_empty()).count());
    assert_eq!(used, 0);

    // A request for a session that was freed or never made.
    let key = [7u8; 16];
    let c = ini(CRYPTO_AES_CBC, &key, None);
    let sid = crypto_newsession(&c, 0).expect("a session");
    crypto_freesession(sid).expect("free");
    let mut buf = vec![0u8; 16];
    let d = with_iv(desc(CRYPTO_AES_CBC, 0, 16, 0, ENC_EXPLICIT), &[0; 16]);
    assert_eq!(
        run_iov(sid, &mut buf, &[16], &[d], None),
        Err(Errno::ENOENT)
    );
    assert_eq!(crypto_freesession(sid), Err(Errno::EINVAL));
    // A descriptor whose algorithm is not in the session.
    let sid = crypto_newsession(&c, 0).expect("a session");
    let d = with_iv(desc(CRYPTO_BLF_CBC, 0, 16, 0, ENC_EXPLICIT), &[0; 8]);
    assert_eq!(
        run_iov(sid, &mut buf, &[16], &[d], None),
        Err(Errno::EINVAL)
    );
    // No buffer.
    let mut crp = crypto_getreq(1).expect("a request");
    crp.crp_sid = sid;
    crp.crp_desc[0] = d;
    assert_eq!(crypto_invoke(&mut crp), Err(Errno::EINVAL));
    // A buffer that is not the kind the flags say.
    let mut iov = [Iovec {
        iov_base: buf.as_mut_ptr().cast(),
        iov_len: 16,
    }];
    let mut uio = Uio {
        uio_iov: &mut iov,
        uio_offset: 0,
        uio_resid: 16,
        uio_segflg: UioSeg::UIO_SYSSPACE,
        uio_rw: UioRw::UIO_WRITE,
        uio_procp: None,
    };
    let mut crp = crypto_getreq(1).expect("a request");
    crp.crp_sid = sid;
    crp.crp_desc[0] = d;
    crp.crp_flags = CRYPTO_F_IMBUF;
    crp.crp_buf = CryptoBuf::Iov(&mut uio);
    assert_eq!(crypto_invoke(&mut crp), Err(Errno::EINVAL));
}

#[test]
fn sessions_are_reused_and_the_table_grows() {
    let _g = fw();
    let key = [3u8; 16];
    let c = ini(CRYPTO_AES_CBC, &key, None);
    let mut sids = Vec::new();
    // 32 slots with 0 unused: the 32nd session makes the table double.
    for _ in 0..40 {
        sids.push(crypto_newsession(&c, 0).expect("a session"));
    }
    assert_eq!(swcr_sesnum(), 2 * CRYPTO_SW_SESSIONS as u32);
    let numbers: Vec<u32> = sids.iter().map(|s| (*s & 0xffff_ffff) as u32).collect();
    assert_eq!(numbers, (1..=40).collect::<Vec<u32>>());
    // Freed numbers are used again, lowest first.
    crypto_freesession(sids[4]).expect("free");
    crypto_freesession(sids[1]).expect("free");
    let again = crypto_newsession(&c, 0).expect("a session");
    assert_eq!(again & 0xffff_ffff, 2);
    let again = crypto_newsession(&c, 0).expect("a session");
    assert_eq!(again & 0xffff_ffff, 5);
}

#[test]
fn free_wipes_the_contexts() {
    let _g = fw();
    let key = hmac_key();
    let c = ini(CRYPTO_SHA1_HMAC, &key, None);
    let sid = crypto_newsession(&c, 0).expect("a session");
    let list = with_sessions(|s| core::mem::take(&mut s[(sid & 0xffff_ffff) as usize]));
    assert_eq!(list.len(), 1);
    let SwcrUn::Auth(a) = &list[0].SWCR_UN else {
        panic!("an authenticator")
    };
    let ictx = a.sw_ictx.as_deref().copied();
    assert!(matches!(ictx, Some(AuthCtx::Sha1(_))));
    drop(list);
    // The driver's own free of a taken slot reports the empty session.
    assert_eq!(swcr_freesession(sid), Err(Errno::EINVAL));
}

#[test]
fn deflate_is_reported_not_silently_dropped() {
    let _g = fw();
    let d = desc(CRYPTO_DEFLATE_COMP, 0, 4, 0, 0);
    let mut sw = SwcrData {
        sw_alg: CRYPTO_DEFLATE_COMP,
        SWCR_UN: SwcrUn::None,
    };
    let mut buf = CryptoBuf::None;
    assert_eq!(swcr_compdec(&d, &mut sw, &mut buf), Err(Errno::ENOSYS));
}
