use super::*;
use crate::sys::endian::htonl;

/// The hash as the matrix product it is: column `j` of the circulant matrix is the key rotated
/// left by `j`, and bit `15 - j` of `n` selects it. Independent of the byte cache.
fn toeplitz_ref(skey: StoeplitzKey, n: u16) -> u16 {
    (0..16)
        .filter(|j| n & (0x8000 >> j) != 0)
        .fold(0, |acc, j| acc ^ skey.rotate_left(j))
}

fn cache(skey: StoeplitzKey) -> StoeplitzCache {
    let mut scache = StoeplitzCache::new();
    stoeplitz_cache_init(&mut scache, skey);
    scache
}

#[test]
fn known_answers() {
    let sc = cache(STOEPLITZ_KEYSEED);
    // The first column is the key; the last is the key rotated right by one.
    assert_eq!(stoeplitz_hash_n16(&sc, 0x8000), 0x6d5a);
    assert_eq!(stoeplitz_hash_n16(&sc, 0x0001), 0x36ad);
    assert_eq!(stoeplitz_hash_n16(&sc, 0x0080), 0x5a6d); // column 8: swap16 of the key
    assert_eq!(stoeplitz_hash_n16(&sc, 0x8001), 0x6d5a ^ 0x36ad);
    assert_eq!(stoeplitz_cache_entry(&sc, 0x80), 0x6d5a);
    assert_eq!(stoeplitz_cache_entry(&sc, 0x01), 0x6d5a_u16.rotate_left(7));
}

#[test]
fn cache_matches_the_matrix_product() {
    for skey in [STOEPLITZ_KEYSEED, 0x0001, 0xfffe, 0x1234, 0x8000] {
        let sc = cache(skey);
        for n in (0..=u16::MAX).step_by(7) {
            assert_eq!(
                stoeplitz_hash_n16(&sc, n),
                toeplitz_ref(skey, n),
                "{skey:#x} {n:#x}"
            );
        }
    }
}

#[test]
fn host_and_network_order_agree() {
    let sc = cache(STOEPLITZ_KEYSEED);
    for h in [0u16, 1, 0x1234, 0xbeef, u16::MAX] {
        assert_eq!(
            stoeplitz_hash_h16(&sc, h),
            stoeplitz_hash_n16(&sc, htons(h))
        );
    }
    for h in [0u32, 0x0a00_0202, 0xdead_beef] {
        assert_eq!(
            stoeplitz_hash_h32(&sc, h),
            stoeplitz_hash_n32(&sc, htonl(h))
        );
    }
    let h64 = 0x0123_4567_89ab_cdef_u64;
    assert_eq!(
        stoeplitz_hash_h64(&sc, h64),
        stoeplitz_hash_h32(&sc, (h64 ^ (h64 >> 32)) as u32)
    );
}

#[test]
fn symmetric_and_linear() {
    let sc = cache(0x4d2b);
    let (a, b) = (htonl(0x0a00_020f), htonl(0xc0a8_0101));
    let (p, q) = (htons(12345), htons(80));
    assert_eq!(stoeplitz_hash_ip4(&sc, a, b), stoeplitz_hash_ip4(&sc, b, a));
    assert_eq!(
        stoeplitz_hash_ip4port(&sc, a, b, p, q),
        stoeplitz_hash_ip4port(&sc, b, a, q, p)
    );
    assert_eq!(
        stoeplitz_hash_ip4port(&sc, a, b, p, q),
        stoeplitz_hash_ip4(&sc, a, b) ^ stoeplitz_hash_n16(&sc, p ^ q)
    );
    let (x, y) = ([0x20u8; 16], {
        let mut y = [0u8; 16];
        y[15] = 1;
        y
    });
    assert_eq!(
        stoeplitz_hash_ip6(&sc, &x, &y),
        stoeplitz_hash_ip6(&sc, &y, &x)
    );
    assert_eq!(
        stoeplitz_hash_ip6port(&sc, &x, &y, p, q),
        stoeplitz_hash_ip6port(&sc, &y, &x, q, p)
    );
    assert_eq!(stoeplitz_hash_ip6(&sc, &x, &x), 0);
    let ea = [0x52, 0x54, 0x00, 0x12, 0x34, 0x56];
    let ea16 = |i: usize| u16::from_ne_bytes([ea[2 * i], ea[2 * i + 1]]);
    assert_eq!(
        stoeplitz_hash_eaddr(&sc, &ea),
        stoeplitz_hash_n16(&sc, ea16(0)) ^ stoeplitz_hash_n16(&sc, ea16(1) ^ ea16(2))
    );
}

#[test]
fn odd_parity_keys_are_invertible() {
    for n in [0u16, 1, 3, 0x8000, 0x6d5a, u16::MAX] {
        assert_eq!(parity(n), (n.count_ones() & 1) as i32, "{n:#x}");
    }
    for _ in 0..8 {
        assert_eq!(parity(stoeplitz_random_seed()), 1);
    }
    // An odd-parity key permutes the 16-bit values; an even-parity one does not.
    for (skey, bijective) in [(STOEPLITZ_KEYSEED, true), (0x6d5b, false)] {
        let sc = cache(skey);
        let mut seen = [false; 1 << 16];
        for n in 0..=u16::MAX {
            seen[usize::from(stoeplitz_hash_n16(&sc, n))] = true;
        }
        assert_eq!(seen.iter().all(|&s| s), bijective, "{skey:#x}");
    }
}

#[test]
fn key_bytes() {
    let mut key = [0u8; 40];
    stoeplitz_to_key(&mut key);
    let seed = stoeplitz_keyseed.load(Ordering::Relaxed);
    for pair in key.as_chunks::<2>().0 {
        assert_eq!(*pair, htons(seed).to_be_bytes());
    }
}

#[test]
#[ignore = "needs OPENBSD_SRC (just test-ref)"]
fn values_match_the_c_header() {
    let defs = crate::reftest::defines("sys/net/toeplitz.h");
    crate::reftest::assert_defines!(defs; STOEPLITZ_KEYSEED);
}
