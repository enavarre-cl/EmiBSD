//! Known-answer tests for X25519: RFC 7748 section 5.2 (two vectors and the iterated test, 1
//! and 1000 iterations; the million-iteration one is left out for its running time) and
//! section 6.1 (the Diffie-Hellman example), plus the properties of the field code.

use super::*;
use crate::crypto::testutil::{hex, hexn};

fn x25519(scalar: &[u8; 32], point: &[u8; 32]) -> [u8; 32] {
    let mut out = [0u8; 32];
    assert!(curve25519(&mut out, scalar, point));
    out
}

#[test]
fn rfc7748_5_2_vectors() {
    let out = x25519(
        &hexn("a546e36bf0527c9d3b16154b82465edd62144c0ac1fc5a18506a2244ba449ac4"),
        &hexn("e6db6867583030db3594c1a424b15f7c726624ec26b3353b10a903a6d0ab1c4c"),
    );
    assert_eq!(
        out.to_vec(),
        hex("c3da55379de9c6908e94ea4df28d084f32eccf03491c71f754b4075577a28552")
    );
    let out = x25519(
        &hexn("4b66e9d4d1b4673c5ad22691957d6af5c11b6421e0ea01d42ca4169e7918ba0d"),
        &hexn("e5210f12786811d3f4b7959d0538ae2c31dbe7106fc03c3efc4cd549c715a493"),
    );
    assert_eq!(
        out.to_vec(),
        hex("95cbde9476e8907d7aade45cb4b873f88b595a68799fa152e6f8f7647aac7957")
    );
}

/// The iterated test: `k = u = 9`, then `(k, u) = (X25519(k, u), k)` the given number of times.
fn iterate(times: usize) -> [u8; 32] {
    let mut k = [0u8; 32];
    k[0] = 9;
    let mut u = k;
    for _ in 0..times {
        let next = x25519(&k, &u);
        u = k;
        k = next;
    }
    k
}

#[test]
fn rfc7748_5_2_iterated_1() {
    assert_eq!(
        iterate(1).to_vec(),
        hex("422c8e7a6227d7bca1350b3e2bb7279f7897b87bb6854b783c60e80311ae3079")
    );
}

#[test]
fn rfc7748_5_2_iterated_1000() {
    assert_eq!(
        iterate(1000).to_vec(),
        hex("684cf59ba83309552800ef566f2f4d3c1c3887c49360e3875f2eb94d99532c51")
    );
}

#[test]
fn rfc7748_6_1_diffie_hellman() {
    let alice_priv: [u8; 32] =
        hexn("77076d0a7318a57d3c16c17251b26645df4c2f87ebc0992ab177fba51db92c2a");
    let bob_priv: [u8; 32] =
        hexn("5dab087e624a8a4b79e17f8b83800ee66f3bb1292618b6fd1c2f8b27ff88e0eb");

    let mut alice_pub = [0u8; 32];
    let mut bob_pub = [0u8; 32];
    assert!(curve25519_generate_public(&mut alice_pub, &alice_priv));
    assert!(curve25519_generate_public(&mut bob_pub, &bob_priv));
    assert_eq!(
        alice_pub.to_vec(),
        hex("8520f0098930a754748b7ddcb43ef75a0dbf3a0d26381af4eba4a98eaa9b4e6a")
    );
    assert_eq!(
        bob_pub.to_vec(),
        hex("de9edb7d7b7dc1b4d35b61c2ece435373f8343c85b78674dadfc7e146f882b4f")
    );

    let shared_a = x25519(&alice_priv, &bob_pub);
    let shared_b = x25519(&bob_priv, &alice_pub);
    assert_eq!(shared_a, shared_b);
    assert_eq!(
        shared_a.to_vec(),
        hex("4a5d9d5ba4ce2de1728e3bf480350f25e07e21c947d19e3376f09b3c1e161742")
    );
}

#[test]
fn low_order_points_give_the_zero_result() {
    let scalar: [u8; 32] = hexn("77076d0a7318a57d3c16c17251b26645df4c2f87ebc0992ab177fba51db92c2a");
    let mut out = [0xaa; 32];
    // u = 0 (order 1) and u = 1 (order 4)
    assert!(!curve25519(&mut out, &scalar, &[0; 32]));
    assert_eq!(out, [0; 32]);
    let mut one = [0u8; 32];
    one[0] = 1;
    assert!(!curve25519(&mut out, &scalar, &one));
    assert_eq!(out, [0; 32]);
    // The all-zero secret has no public key.
    assert!(!curve25519_generate_public(&mut out, &[0; 32]));
}

#[test]
fn the_scalar_is_clamped_and_the_top_bit_of_u_ignored() {
    let scalar: [u8; 32] = hexn("a546e36bf0527c9d3b16154b82465edd62144c0ac1fc5a18506a2244ba449ac4");
    let point: [u8; 32] = hexn("e6db6867583030db3594c1a424b15f7c726624ec26b3353b10a903a6d0ab1c4c");
    let want = x25519(&scalar, &point);

    // The low three bits and the top two do not matter: they are overwritten.
    let mut other = scalar;
    other[0] ^= 7;
    other[31] ^= 0xc0;
    assert_eq!(x25519(&other, &point), want);
    // Bit 255 of the point is ignored.
    let mut high = point;
    high[31] ^= 0x80;
    assert_eq!(x25519(&scalar, &high), want);

    let mut s = [0xffu8; 32];
    curve25519_clamp_secret(&mut s);
    assert_eq!((s[0], s[31]), (248, 127));
    let mut s = [0u8; 32];
    curve25519_clamp_secret(&mut s);
    assert_eq!((s[0], s[31]), (0, 64));
}

#[test]
fn field_encoding_round_trips_and_freezes() {
    // p - 1 and p + 1 (as unreduced 255-bit patterns) come back as p - 1 and 1: bit 255 is
    // dropped on the way in, and the encoding is the reduced one.
    let mut pm1 = [0xffu8; 32];
    pm1[0] = 0xec;
    pm1[31] = 0x7f;
    let mut f = Fe::default();
    fe_frombytes(&mut f, &pm1);
    let mut out = [0u8; 32];
    fe_tobytes(&mut out, &f);
    assert_eq!(out, pm1);

    let mut p = pm1;
    p[0] = 0xed; // p itself is 0
    fe_frombytes(&mut f, &p);
    fe_tobytes(&mut out, &f);
    assert_eq!(out, [0; 32]);

    let mut pp1 = pm1;
    pp1[0] = 0xee; // p + 1 is 1
    fe_frombytes(&mut f, &pp1);
    fe_tobytes(&mut out, &f);
    let mut one = [0u8; 32];
    one[0] = 1;
    assert_eq!(out, one);
}

#[test]
fn field_inverse() {
    let mut x = Fe::default();
    fe_frombytes(
        &mut x,
        &hexn("e6db6867583030db3594c1a424b15f7c726624ec26b3353b10a903a6d0ab1c4c"),
    );
    let inv = fe_invert(&x);
    let prod = fe_mul_ttt(&x, &inv);
    let mut out = [0u8; 32];
    fe_tobytes(&mut out, &prod);
    let mut one = [0u8; 32];
    one[0] = 1;
    assert_eq!(out, one);
    // fe_invert(0) = 0, which the ladder relies on.
    let zero = fe_invert(&fe_0());
    fe_tobytes(&mut out, &zero);
    assert_eq!(out, [0; 32]);
}

#[test]
fn mul121666_matches_the_general_product() {
    let mut x = Fe::default();
    fe_frombytes(
        &mut x,
        &hexn("a546e36bf0527c9d3b16154b82465edd62144c0ac1fc5a18506a2244ba449ac4"),
    );
    let loose = FeLoose { v: x.v };
    let mut c = Fe::default();
    c.v[0] = 121666;
    let (mut a, mut b) = ([0u8; 32], [0u8; 32]);
    fe_tobytes(&mut a, &fe_mul121666(&loose));
    fe_tobytes(&mut b, &fe_mul_ttt(&x, &c));
    assert_eq!(a, b);
}
