/*	$OpenBSD: ntfs_conv.c,v 1.9 2013/11/24 16:02:30 jsing Exp $	*/
/*	$NetBSD: ntfs_conv.c,v 1.1 2002/12/23 17:38:32 jdolecek Exp $	*/
/* <LICENSES> */
/*-
 * Copyright (c) 2001 The NetBSD Foundation, Inc.
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
 *
 * THIS SOFTWARE IS PROVIDED BY THE NETBSD FOUNDATION, INC. AND CONTRIBUTORS
 * ``AS IS'' AND ANY EXPRESS OR IMPLIED WARRANTIES, INCLUDING, BUT NOT LIMITED
 * TO, THE IMPLIED WARRANTIES OF MERCHANTABILITY AND FITNESS FOR A PARTICULAR
 * PURPOSE ARE DISCLAIMED.  IN NO EVENT SHALL THE FOUNDATION OR CONTRIBUTORS
 * BE LIABLE FOR ANY DIRECT, INDIRECT, INCIDENTAL, SPECIAL, EXEMPLARY, OR
 * CONSEQUENTIAL DAMAGES (INCLUDING, BUT NOT LIMITED TO, PROCUREMENT OF
 * SUBSTITUTE GOODS OR SERVICES; LOSS OF USE, DATA, OR PROFITS; OR BUSINESS
 * INTERRUPTION) HOWEVER CAUSED AND ON ANY THEORY OF LIABILITY, WHETHER IN
 * CONTRACT, STRICT LIABILITY, OR TORT (INCLUDING NEGLIGENCE OR OTHERWISE)
 * ARISING IN ANY WAY OUT OF THE USE OF THIS SOFTWARE, EVEN IF ADVISED OF THE
 * POSSIBILITY OF SUCH DAMAGE.
 */
/* </LICENSES> */

//! File name recode stuff: the UTF-8 hooks a mount uses to decode the names it is asked for
//! into UTF-16 (`ntm_wget`), to encode the names it returns (`ntm_wput`), and to compare two
//! wide characters (`ntm_wcmp`).
//!
//! Upstream: sys/ntfs/ntfs_conv.c @ 3ce1f3f79392
//!
//! The utf-8 routines were derived from src/lib/libc/locale/utf2.c.
//!
//! ## Deviations
//! - `ntfs_utf8_wget(const char **str)` takes the bytes and a position in them, which it
//!   advances; a byte past the end of the bytes reads as 0 (the C reads the bytes that follow
//!   in memory: in a lookup, the `/` or NUL after the component, which fails the same tests).
//!   Three-byte sequences keep the C's `s[0] & 0x1F` mask.
//! - `ntfs_utf8_wput(s, n, wc)` takes the room as a slice (`n` is its length) and returns the
//!   bytes written as a `usize`.
//! - The `DDPRINTF`s (`NTFS_DEBUG`, off) are left out.

use crate::ntfs::ntfs::Wchar;

/// `_utf_count`: the length of a UTF-8 sequence by its first byte's high nibble (0: not a
/// first byte).
static UTF_COUNT: [usize; 16] = [1, 1, 1, 1, 1, 1, 1, 1, 0, 0, 0, 0, 2, 2, 3, 0];

/// `ntfs_utf8_wget(&s)`: read one wide character off the string at `*pos`, shift the
/// position and return the character.
pub fn ntfs_utf8_wget(s: &[u8], pos: &mut usize) -> Wchar {
    let at = |i: usize| u16::from(s.get(*pos + i).copied().unwrap_or(0));
    let mut rune: Wchar = 0;

    let mut c = UTF_COUNT[usize::from((at(0) >> 4) & 0xf)];
    'decode: {
        match c {
            0 => {
                c = 1;
                break 'decode; // encoding_error
            }
            1 => rune = at(0) & 0xff,
            2 => {
                if at(1) & 0xc0 != 0x80 {
                    break 'decode;
                }
                rune = ((at(0) & 0x1F) << 6) | (at(1) & 0x3F);
            }
            _ => {
                if at(1) & 0xC0 != 0x80 || at(2) & 0xC0 != 0x80 {
                    break 'decode;
                }
                rune = ((at(0) & 0x1F) << 12) | ((at(1) & 0x3F) << 6) | (at(2) & 0x3F);
            }
        }
    }

    // encoding_error:
    *pos += c;
    rune
}

/// `ntfs_utf8_wput(s, n, wc)`: encode wide character and write it to the string. `s.len()`
/// specifies how much space there is in the string. Returns number of bytes written to the
/// target string (0 when it does not fit).
pub fn ntfs_utf8_wput(s: &mut [u8], wc: Wchar) -> usize {
    if wc & 0xf800 != 0 {
        if s.len() < 3 {
            // bound check failure
            return 0;
        }

        s[0] = 0xE0 | ((wc >> 12) & 0x0F) as u8;
        s[1] = 0x80 | ((wc >> 6) & 0x3F) as u8;
        s[2] = 0x80 | (wc & 0x3F) as u8;
        3
    } else if wc & 0x0780 != 0 {
        if s.len() < 2 {
            // bound check failure
            return 0;
        }

        s[0] = 0xC0 | ((wc >> 6) & 0x1F) as u8;
        s[1] = 0x80 | (wc & 0x3F) as u8;
        2
    } else {
        if s.is_empty() {
            // bound check failure
            return 0;
        }

        s[0] = wc as u8;
        1
    }
}

/// `ntfs_utf8_wcmp(wc1, wc2)`: compare two wide characters, returning a positive, zero or
/// negative value if the first is bigger, equal or lower than the second.
pub fn ntfs_utf8_wcmp(wc1: Wchar, wc2: Wchar) -> i32 {
    // no conversions needed for utf8

    if wc1 == wc2 {
        0
    } else {
        i32::from(wc1) - i32::from(wc2)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn decode(s: &[u8]) -> std::vec::Vec<(Wchar, usize)> {
        let mut out = std::vec::Vec::new();
        let mut pos = 0;
        while pos < s.len() {
            let before = pos;
            let w = ntfs_utf8_wget(s, &mut pos);
            out.push((w, pos - before));
        }
        out
    }

    #[test]
    fn wget_decodes_one_two_and_three_byte_sequences() {
        assert_eq!(decode(b"a"), [(0x61, 1)]);
        assert_eq!(decode("é".as_bytes()), [(0xe9, 2)]);
        assert_eq!(decode("€".as_bytes()), [(0x20ac, 3)]);
        // A continuation byte first, and a broken two-byte sequence: errors, rune 0.
        assert_eq!(decode(&[0x80]), [(0, 1)]);
        assert_eq!(decode(&[0xc3, 0x41]), [(0, 2)]);
        // A truncated three-byte sequence reads zeros past the end.
        assert_eq!(decode(&[0xe2, 0x82]), [(0, 3)]);
    }

    #[test]
    fn wput_encodes_and_checks_the_room() {
        let mut b = [0u8; 3];
        assert_eq!(ntfs_utf8_wput(&mut b, 0x41), 1);
        assert_eq!(b[0], b'A');
        assert_eq!(ntfs_utf8_wput(&mut b, 0xe9), 2);
        assert_eq!(&b[..2], "é".as_bytes());
        assert_eq!(ntfs_utf8_wput(&mut b, 0x20ac), 3);
        assert_eq!(&b, "€".as_bytes());
        assert_eq!(ntfs_utf8_wput(&mut b[..2], 0x20ac), 0);
        assert_eq!(ntfs_utf8_wput(&mut b[..1], 0xe9), 0);
        assert_eq!(ntfs_utf8_wput(&mut [], 0x41), 0);
    }

    #[test]
    fn wcmp_orders_by_code_unit() {
        assert_eq!(ntfs_utf8_wcmp(5, 5), 0);
        assert!(ntfs_utf8_wcmp(6, 5) > 0);
        assert!(ntfs_utf8_wcmp(0x41, 0x20ac) < 0);
    }
}
