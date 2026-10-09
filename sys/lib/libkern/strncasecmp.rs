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
 * Copyright (c) 1994 Christian E. Hopps
 * All rights reserved.
 *
 * Redistribution and use in source and binary forms, with or without
 * modification, are permitted provided that the following conditions
 * are met:
 * 1. Redistributions of source code must retain the above copyright
 *    notice, this list of conditions and the following disclaimer.
 * 2. Redistributions in binary form must reproduce the above copyright
 *    notice, this list of conditions and the following disclaimer in the
 *    documentation and/or other materials provided with the distribution.
 * 3. All advertising materials mentioning features or use of this software
 *    must display the following acknowledgement:
 *      This product includes software developed by Christian E. Hopps.
 * 4. The name of the author may not be used to endorse or promote products
 *    derived from this software without specific prior written permission
 *
 * THIS SOFTWARE IS PROVIDED BY THE AUTHOR ``AS IS'' AND ANY EXPRESS OR
 * IMPLIED WARRANTIES, INCLUDING, BUT NOT LIMITED TO, THE IMPLIED WARRANTIES
 * OF MERCHANTABILITY AND FITNESS FOR A PARTICULAR PURPOSE ARE DISCLAIMED.
 * IN NO EVENT SHALL THE AUTHOR BE LIABLE FOR ANY DIRECT, INDIRECT,
 * INCIDENTAL, SPECIAL, EXEMPLARY, OR CONSEQUENTIAL DAMAGES (INCLUDING, BUT
 * NOT LIMITED TO, PROCUREMENT OF SUBSTITUTE GOODS OR SERVICES; LOSS OF USE,
 * DATA, OR PROFITS; OR BUSINESS INTERRUPTION) HOWEVER CAUSED AND ON ANY
 * THEORY OF LIABILITY, WHETHER IN CONTRACT, STRICT LIABILITY, OR TORT
 * (INCLUDING NEGLIGENCE OR OTHERWISE) ARISING IN ANY WAY OUT OF THE USE OF
 * THIS SOFTWARE, EVEN IF ADVISED OF THE POSSIBILITY OF SUCH DAMAGE.
 */
/* </LICENSES> */

/* <CODE> */
//! `strncasecmp(3)`: compare at most `n` bytes of two strings, ignoring ASCII case.
//!
//! Upstream: sys/lib/libkern/strncasecmp.c @ 3ce1f3f79392
//!
//! ## Deviations
//! - Operates on byte slices: a string ends at its first NUL or at the end of its slice,
//!   whichever comes first (the C reads to the NUL). The result keeps the C's sign and value:
//!   the difference of the first differing bytes after case folding.

/// Compares `s1` and `s2` over at most `n` bytes, folding `A`-`Z` onto `a`-`z` when the two
/// bytes differ; negative, zero or positive as `s1` sorts before, with or after `s2`.
pub fn strncasecmp(s1: &[u8], s2: &[u8], n: usize) -> i32 {
    let byte = |s: &[u8], i: usize| s.get(i).copied().unwrap_or(0);

    for i in 0..n {
        let mut c1 = byte(s1, i);
        let mut c2 = byte(s2, i);

        if c1 != c2 {
            if c1.is_ascii_uppercase() && c2.is_ascii_lowercase() {
                c1 += b'a' - b'A';
            } else if c1.is_ascii_lowercase() && c2.is_ascii_uppercase() {
                c2 += b'a' - b'A';
            }
            if c1 != c2 {
                return i32::from(c1) - i32::from(c2);
            }
        }
        if c1 == 0 {
            break;
        }
    }

    0
}
/* </CODE> */

/* <TESTS> */
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn table() {
        let cases: &[(&[u8], &[u8], usize, i32)] = &[
            (b"", b"", 0, 0),
            (b"abc", b"xyz", 0, 0),
            (b"AES", b"aes", 3, 0),
            (b"aes\0", b"aes\0", 10, 0),
            (b"hmac-SHA1", b"hmac-sha1", 9, 0),
            (b"aesctr", b"aes", 3, 0),
            (b"aes", b"aesctr", 4, -i32::from(b'c')),
            (b"b", b"A", 1, 1),
            (b"[", b"a", 1, i32::from(b'[') - i32::from(b'a')),
            (b"abc", b"abd", 3, -1),
        ];
        for &(s1, s2, n, want) in cases {
            assert_eq!(
                strncasecmp(s1, s2, n),
                want,
                "strncasecmp({s1:?}, {s2:?}, {n})"
            );
        }
    }
}
/* </TESTS> */
