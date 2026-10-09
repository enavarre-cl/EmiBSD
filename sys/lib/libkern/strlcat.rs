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
//! LZ: sys/lib/libkern/strlcat.rs@f5985f1d055a
//!
//! ## Deviations
//! - Operates on byte slices: the destination size is `dst.len()`, not a separate argument, and
//!   `src` ends at its first NUL or at `src.len()`, whichever comes first.
//!
//! ## Redesign
//! - Tests only: a property test against a model written from the man page (append
//!   `min(strlen(src), size - strlen(dst) - 1)` bytes, terminate, return
//!   `strlen(src) + min(size, strlen(dst))`), including a destination with no NUL within its size.

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

    #[test]
    fn matches_the_model_on_random_input() {
        let mut x = 0x9e37_79b9u32;
        let mut next = move || {
            x ^= x << 13;
            x ^= x >> 17;
            x ^= x << 5;
            x
        };
        for _ in 0..3000 {
            let size = (next() % 13) as usize;
            let init: std::vec::Vec<u8> = (0..size).map(|_| (next() % 4) as u8).collect();
            let srclen = (next() % 10) as usize;
            let src: std::vec::Vec<u8> = (0..srclen).map(|_| (next() % 3) as u8).collect();

            let mut dst = init.clone();
            let ret = strlcat(&mut dst, &src);

            let dlen = init.iter().position(|&b| b == 0).unwrap_or(size);
            let slen = src.iter().position(|&b| b == 0).unwrap_or(src.len());
            assert_eq!(ret, dlen + slen, "return, dst {init:?} src {src:?}");
            if dlen == size {
                assert_eq!(dst, init, "no NUL within size: untouched");
                continue;
            }
            let n = slen.min(size - dlen - 1);
            assert_eq!(&dst[..dlen], &init[..dlen]);
            assert_eq!(&dst[dlen..dlen + n], &src[..n]);
            assert_eq!(dst[dlen + n], 0);
            assert_eq!(
                &dst[dlen + n + 1..],
                &init[dlen + n + 1..],
                "tail untouched"
            );
        }
    }
}
/* </TESTS> */
