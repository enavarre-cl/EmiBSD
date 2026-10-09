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
//! `strlcat(3)`: size-bounded string concatenation.
//!
//! Upstream: sys/lib/libkern/strlcat.c @ 3ce1f3f79392
//!
//! ## Deviations
//! - Operates on byte slices: the destination size is `dst.len()`, not a separate argument, and
//!   `src` ends at its first NUL or at `src.len()`, whichever comes first.

use crate::strnlen;

/// Appends the string in `src` to the string in `dst`. Unlike `strncat`, `dst.len()` is the full
/// size of the buffer, not the space left. At most `dst.len() - 1` bytes end up in `dst` and the
/// result is NUL-terminated, unless `dst` holds no NUL within its size (then nothing is written).
/// Returns `strlen(src) + min(dst.len(), strlen(initial dst))`; a result `>= dst.len()` means
/// truncation occurred.
pub fn strlcat(dst: &mut [u8], src: &[u8]) -> usize {
    let dsize = dst.len();
    // Find the end of dst, but don't go past its size.
    let dlen = strnlen(dst, dsize);
    let srclen = strnlen(src, src.len());
    let Some(room) = (dsize - dlen).checked_sub(1) else {
        return dlen + srclen;
    };
    let n = srclen.min(room);
    dst[dlen..dlen + n].copy_from_slice(&src[..n]);
    dst[dlen + n] = 0;
    dlen + srclen
}
/* </CODE> */

/* <TESTS> */
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn table() {
        // (initial destination, source, expected return, expected destination bytes)
        let cases: &[(&[u8], &[u8], usize, &[u8])] = &[
            (
                b"ab\0\xff\xff\xff\xff\xff",
                b"cd\0",
                4,
                b"abcd\0\xff\xff\xff",
            ),
            (b"ab\0\xff\xff\xff\xff\xff", b"cd", 4, b"abcd\0\xff\xff\xff"),
            (b"ab\0\xff\xff", b"cd", 4, b"abcd\0"),
            (b"ab\0\xff", b"cde", 5, b"abc\0"),
            (b"ab\0", b"cde", 5, b"ab\0"),
            (b"abc", b"d", 4, b"abc"),
            (b"", b"abc", 3, b""),
            (b"\0\xff\xff\xff", b"ab", 2, b"ab\0\xff"),
            (b"ab\0\xff\xff", b"", 2, b"ab\0\xff\xff"),
            (b"ab\0\xff\xff\xff\xff", b"c\0d", 3, b"abc\0\xff\xff\xff"),
        ];
        for &(initial, src, want_ret, want_dst) in cases {
            let mut dst = initial.to_vec();
            let ret = strlcat(&mut dst, src);
            assert_eq!(ret, want_ret, "return of strlcat({initial:?}, {src:?})");
            assert_eq!(
                &dst[..],
                want_dst,
                "contents after strlcat({initial:?}, {src:?})"
            );
        }
    }
}
/* </TESTS> */
