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
//! `strlcpy(3)`: size-bounded string copy.
//!
//! Upstream: sys/lib/libkern/strlcpy.c @ 3ce1f3f79392
//! LZ: sys/lib/libkern/strlcpy.rs@f5985f1d055a
//!
//! ## Deviations
//! - Operates on byte slices: the destination size is `dst.len()`, not a separate argument, and
//!   `src` ends at its first NUL or at `src.len()`, whichever comes first.
//!
//! ## Redesign
//! - Tests only: a property test against a model written from the man page (copy
//!   `min(strlen(src), size - 1)` bytes, terminate, return `strlen(src)`), over random buffers,
//!   sizes 0..=12 and sources with and without an embedded NUL.

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

    #[test]
    fn matches_the_model_on_random_input() {
        let mut x = 0x1234_5679u32;
        let mut next = move || {
            x ^= x << 13;
            x ^= x >> 17;
            x ^= x << 5;
            x
        };
        for _ in 0..2000 {
            let srclen = (next() % 14) as usize;
            // Small alphabet with plenty of NULs, so embedded terminators are common.
            let src: std::vec::Vec<u8> = (0..srclen).map(|_| (next() % 3) as u8).collect();
            let size = (next() % 13) as usize;
            let mut dst = std::vec![0xeeu8; size];
            let ret = strlcpy(&mut dst, &src);

            let slen = src.iter().position(|&b| b == 0).unwrap_or(src.len());
            assert_eq!(ret, slen);
            if size == 0 {
                continue;
            }
            let n = slen.min(size - 1);
            assert_eq!(&dst[..n], &src[..n]);
            assert_eq!(dst[n], 0);
            assert!(
                dst[n + 1..].iter().all(|&b| b == 0xee),
                "bytes past the NUL untouched"
            );
        }
    }
}
/* </TESTS> */
