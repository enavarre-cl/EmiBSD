//! Host test helper for `sys/crypto`: the known-answer vectors are written as hex strings.

extern crate std;

use std::vec::Vec;

/// The bytes of a hex string; spaces and line breaks are ignored.
pub(crate) fn hex(s: &str) -> Vec<u8> {
    let digits: Vec<u8> = s
        .bytes()
        .filter(|b| !b.is_ascii_whitespace())
        .map(|b| match b {
            b'0'..=b'9' => b - b'0',
            b'a'..=b'f' => b - b'a' + 10,
            b'A'..=b'F' => b - b'A' + 10,
            _ => panic!("not a hex digit: {}", b as char),
        })
        .collect();
    assert!(digits.len() % 2 == 0, "odd number of hex digits");
    digits.chunks(2).map(|p| p[0] << 4 | p[1]).collect()
}

/// The bytes of a hex string as a fixed-size array.
pub(crate) fn hexn<const N: usize>(s: &str) -> [u8; N] {
    let v = hex(s);
    let mut a = [0u8; N];
    a.copy_from_slice(&v);
    a
}
