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
 * Copyright (c) 2010 Todd C. Miller <millert@openbsd.org>
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
//! `strnlen(3)`: bounded string length.
//!
//! Upstream: sys/lib/libkern/strnlen.c @ 3ce1f3f79392
//!
//! ## Deviations
//! - Operates on a byte slice: the scan stops at the first NUL, at `maxlen`, or at the end of
//!   the slice, whichever comes first. The C version has no way to know the slice end.

/// The length of the string in `s`, counting at most `maxlen` bytes.
pub fn strnlen(s: &[u8], maxlen: usize) -> usize {
    let limit = maxlen.min(s.len());
    s[..limit].iter().position(|&b| b == 0).unwrap_or(limit)
}
/* </CODE> */

/* <TESTS> */
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn table() {
        let cases: &[(&[u8], usize, usize)] = &[
            (b"", 0, 0),
            (b"", 10, 0),
            (b"\0", 10, 0),
            (b"abc\0", 10, 3),
            (b"abc\0", 3, 3),
            (b"abc\0", 2, 2),
            (b"abc\0", 0, 0),
            (b"abc", 10, 3),
            (b"ab\0cd", 10, 2),
        ];
        for &(s, maxlen, want) in cases {
            assert_eq!(strnlen(s, maxlen), want, "strnlen({s:?}, {maxlen})");
        }
    }
}
/* </TESTS> */
