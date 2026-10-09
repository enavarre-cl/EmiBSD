/* <LICENSES> */
/*
 * Copyright (c) 2026 Emilio Navarrete Lineros <enavarre@outlook.com>
 *
 * Permission to use, copy, modify, and distribute this software for any
 * purpose with or without fee is hereby granted, provided that the above
 * copyright notice and this permission notice appear in all copies.
 *
 * THE SOFTWARE IS PROVIDED "AS IS" AND THE AUTHOR DISCLAIMS ALL WARRANTIES
 * WITH REGARD TO THIS SOFTWARE INCLUDING ALL IMPLIED WARRANTIES OF
 * MERCHANTABILITY AND FITNESS. IN NO EVENT SHALL THE AUTHOR BE LIABLE FOR
 * ANY SPECIAL, DIRECT, INDIRECT, OR CONSEQUENTIAL DAMAGES OR ANY DAMAGES
 * WHATSOEVER RESULTING FROM LOSS OF USE, DATA OR PROFITS, WHETHER IN AN
 * ACTION OF CONTRACT, NEGLIGENCE OR OTHER TORTIOUS ACTION, ARISING OUT OF
 * OR IN CONNECTION WITH THE USE OR PERFORMANCE OF THIS SOFTWARE.
 */

/*
 * Copyright (c) 2010 Damien Miller.  All rights reserved.
 *
 * Permission to use, copy, modify, and distribute this software for any
 * purpose with or without fee is hereby granted, provided that the above
 * copyright notice and this permission notice appear in all copies.
 *
 * THE SOFTWARE IS PROVIDED "AS IS" AND THE AUTHOR DISCLAIMS ALL WARRANTIES
 * WITH REGARD TO THIS SOFTWARE INCLUDING ALL IMPLIED WARRANTIES OF
 * MERCHANTABILITY AND FITNESS. IN NO EVENT SHALL THE AUTHOR BE LIABLE FOR
 * ANY SPECIAL, DIRECT, INDIRECT, OR CONSEQUENTIAL DAMAGES OR ANY DAMAGES
 * WHATSOEVER RESULTING FROM LOSS OF USE, DATA OR PROFITS, WHETHER IN AN
 * ACTION OF CONTRACT, NEGLIGENCE OR OTHER TORTIOUS ACTION, ARISING OUT OF
 * OR IN CONNECTION WITH THE USE OR PERFORMANCE OF THIS SOFTWARE.
 */
/* </LICENSES> */

/* <CODE> */
//! `timingsafe_bcmp(3)`: constant-time byte comparison.
//!
//! Upstream: sys/lib/libkern/timingsafe_bcmp.c @ 3ce1f3f79392
//! LZ: sys/lib/libkern/timingsafe_bcmp.rs@f5985f1d055a
//!
//! ## Deviations
//! - Takes two slices instead of two pointers and a length. The C caller guarantees both buffers
//!   hold `n` bytes; here `n` is the shorter length and a length mismatch counts as a difference.
//!   Lengths are not secret, so folding them in costs nothing.
//! - Returns `bool` for the C `int`: `true` where C returns 1 (the buffers differ).
//!
//! ## Redesign
//! - Tests only: every length 0..=40, a single flipped bit at every position (the result must be
//!   `true` wherever it lands), equal buffers, and mismatched lengths in both argument orders.

/// Compares `b1` and `b2` in time that depends only on their lengths, never on their contents.
/// Returns `true` if they differ (C returns 1), `false` if they are identical (C returns 0).
pub fn timingsafe_bcmp(b1: &[u8], b2: &[u8]) -> bool {
    let n = b1.len().min(b2.len());
    let mut ret: u8 = 0;
    for (x, y) in b1[..n].iter().zip(&b2[..n]) {
        ret |= x ^ y;
    }
    (ret != 0) | (b1.len() != b2.len())
}
/* </CODE> */

/* <TESTS> */
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn table() {
        let cases: &[(&[u8], &[u8], bool)] = &[
            (b"", b"", false),
            (b"a", b"a", false),
            (b"abc", b"abc", false),
            (b"abc", b"abd", true),
            (b"abc", b"bbc", true),
            (b"abc", b"ab", true),
            (b"", b"a", true),
            (b"\x80", b"\x00", true),
            (b"\xff\xff", b"\xff\xff", false),
        ];
        for &(a, b, want) in cases {
            assert_eq!(timingsafe_bcmp(a, b), want, "timingsafe_bcmp({a:?}, {b:?})");
            assert_eq!(timingsafe_bcmp(b, a), want, "timingsafe_bcmp({b:?}, {a:?})");
        }
    }

    #[test]
    fn one_flipped_bit_anywhere_is_found() {
        for len in 0..=40usize {
            let a: std::vec::Vec<u8> = (0..len).map(|i| (i as u8).wrapping_mul(31)).collect();
            assert!(!timingsafe_bcmp(&a, &a.clone()), "equal, len {len}");
            for pos in 0..len {
                for bit in 0..8 {
                    let mut b = a.clone();
                    b[pos] ^= 1 << bit;
                    assert!(timingsafe_bcmp(&a, &b), "len {len} pos {pos} bit {bit}");
                    assert!(
                        timingsafe_bcmp(&b, &a),
                        "len {len} pos {pos} bit {bit} swapped"
                    );
                }
            }
        }
    }

    #[test]
    fn a_length_mismatch_differs_even_when_the_prefix_matches() {
        let a = [7u8; 16];
        for n in 0..16 {
            assert!(timingsafe_bcmp(&a[..n], &a));
            assert!(timingsafe_bcmp(&a, &a[..n]));
        }
    }
}
/* </TESTS> */
