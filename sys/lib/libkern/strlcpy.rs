/* <LICENSES> */
/*
 * Copyright (c) 1998, 2015 Todd C. Miller <millert@openbsd.org>
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
//! `strlcpy(3)`: size-bounded string copy.
//!
//! Upstream: sys/lib/libkern/strlcpy.c @ 3ce1f3f79392
//!
//! ## Deviations
//! - Operates on byte slices: the destination size is `dst.len()`, not a separate argument, and
//!   `src` ends at its first NUL or at `src.len()`, whichever comes first.

use crate::strnlen;

/// Copies the string in `src` to `dst`. At most `dst.len() - 1` bytes are copied and the result
/// is always NUL-terminated (unless `dst` is empty). Returns the length of `src`; a result
/// `>= dst.len()` means truncation occurred.
pub fn strlcpy(dst: &mut [u8], src: &[u8]) -> usize {
    let srclen = strnlen(src, src.len());
    if let Some(room) = dst.len().checked_sub(1) {
        let n = srclen.min(room);
        dst[..n].copy_from_slice(&src[..n]);
        dst[n] = 0;
    }
    srclen
}
/* </CODE> */

/* <TESTS> */
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn table() {
        // (destination size, source, expected return, expected destination bytes)
        let cases: &[(usize, &[u8], usize, &[u8])] = &[
            (8, b"abc\0", 3, b"abc\0\xff\xff\xff\xff"),
            (8, b"abc", 3, b"abc\0\xff\xff\xff\xff"),
            (4, b"abc\0", 3, b"abc\0"),
            (3, b"abc\0", 3, b"ab\0"),
            (1, b"abc\0", 3, b"\0"),
            (0, b"abc\0", 3, b""),
            (4, b"", 0, b"\0\xff\xff\xff"),
            (8, b"ab\0cd", 2, b"ab\0\xff\xff\xff\xff\xff"),
            (2, b"abcdefghij", 10, b"a\0"),
        ];
        for &(size, src, want_ret, want_dst) in cases {
            let mut dst = std::vec![0xffu8; size];
            let ret = strlcpy(&mut dst, src);
            assert_eq!(ret, want_ret, "return of strlcpy(dst[{size}], {src:?})");
            assert_eq!(
                &dst[..],
                want_dst,
                "contents after strlcpy(dst[{size}], {src:?})"
            );
            assert_eq!(
                ret >= size,
                size == 0 || want_dst.len() == size && dst[size - 1] == 0 && ret > size - 1,
                "truncation flag for strlcpy(dst[{size}], {src:?})"
            );
        }
    }
}
/* </TESTS> */
