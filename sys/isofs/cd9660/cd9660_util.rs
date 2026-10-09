/* <LICENSES> */
/*-
 * Copyright (c) 1994
 *	The Regents of the University of California.  All rights reserved.
 *
 * This code is derived from software contributed to Berkeley
 * by Pace Willisson (pace@blitz.com).  The Rock Ridge Extension
 * Support code is derived from software contributed to Berkeley
 * by Atsushi Murai (amurai@spec.co.jp). Joliet support was added by
 * Joachim Kuebart (joki@kuebart.stuttgart.netsurf.de).
 *
 * Redistribution and use in source and binary forms, with or without
 * modification, are permitted provided that the following conditions
 * are met:
 * 1. Redistributions of source code must retain the above copyright
 *    notice, this list of conditions and the following disclaimer.
 * 2. Redistributions in binary form must reproduce the above copyright
 *    notice, this list of conditions and the following disclaimer in the
 *    documentation and/or other materials provided with the distribution.
 * 3. Neither the name of the University nor the names of its contributors
 *    may be used to endorse or promote products derived from this software
 *    without specific prior written permission.
 *
 * THIS SOFTWARE IS PROVIDED BY THE REGENTS AND CONTRIBUTORS ``AS IS'' AND
 * ANY EXPRESS OR IMPLIED WARRANTIES, INCLUDING, BUT NOT LIMITED TO, THE
 * IMPLIED WARRANTIES OF MERCHANTABILITY AND FITNESS FOR A PARTICULAR PURPOSE
 * ARE DISCLAIMED.  IN NO EVENT SHALL THE REGENTS OR CONTRIBUTORS BE LIABLE
 * FOR ANY DIRECT, INDIRECT, INCIDENTAL, SPECIAL, EXEMPLARY, OR CONSEQUENTIAL
 * DAMAGES (INCLUDING, BUT NOT LIMITED TO, PROCUREMENT OF SUBSTITUTE GOODS
 * OR SERVICES; LOSS OF USE, DATA, OR PROFITS; OR BUSINESS INTERRUPTION)
 * HOWEVER CAUSED AND ON ANY THEORY OF LIABILITY, WHETHER IN CONTRACT, STRICT
 * LIABILITY, OR TORT (INCLUDING NEGLIGENCE OR OTHERWISE) ARISING IN ANY WAY
 * OUT OF THE USE OF THIS SOFTWARE, EVEN IF ADVISED OF THE POSSIBILITY OF
 * SUCH DAMAGE.
 *
 *	@(#)cd9660_util.c	8.3 (Berkeley) 12/5/94
 */
/* </LICENSES> */

/* <CODE> */
//! ISO 9660 file names: reading one character of a name in the record's encoding (plain or
//! Joliet), comparing a path component with a name (`isofncmp`) and translating a name into
//! the one shown to the user (`isofntrans`).
//!
//! Upstream: sys/isofs/cd9660/cd9660_util.c @ 3ce1f3f79392
//!
//! ## Deviations
//! - Names are byte slices. `isochar` takes the bytes from the character to the end of its
//!   buffer and the number of them that belong to the name (the C's `isoend - isofn`): the
//!   C reads past the name's end in some callers, never past the buffer, and neither does
//!   this. A byte past the buffer reads as zero.
//! - `isofncmp` and `isofntrans` take the names as slices; their end tests are `<` where
//!   the C compares pointers for equality (a two-byte Joliet step cannot run past the end).
//!   `isofntrans`'s `original` and `assoc` are `bool`, and it writes no byte past `outfn`.
//! - `cd9660_wchar2char`, a conversion routine a module could load, is a `StaticCell` that
//!   nothing sets, as in OpenBSD.

use crate::isofs::cd9660::iso::ASSOCCHAR;
use libkern::StaticCell;

/// A Unicode conversion routine: the character of a UCS-2 code.
pub type Wchar2char = fn(u32) -> u8;

/// `cd9660_wchar2char`: limited support for loading of a Unicode conversion routine at
/// run-time; should be removed when native Unicode kernel interfaces have been introduced.
/// Nothing sets it.
pub static CD9660_WCHAR2CHAR: StaticCell<Option<Wchar2char>> = StaticCell::new(None);

/// `isochar`: get one character out of an iso filename, obeying `joliet_level`; returns the
/// number of bytes consumed. `isofn` runs from the character to the end of its buffer, `len`
/// of its bytes belong to the name (see the module's deviations).
pub fn isochar(isofn: &[u8], len: usize, joliet_level: i32, c: &mut u8) -> usize {
    let at = |i: usize| isofn.get(i).copied().unwrap_or(0);

    *c = at(0);
    if joliet_level == 0 || len == 1 {
        // (00) and (01) are one byte in Joliet, too
        return 1;
    }

    // No Unicode support yet :-(
    *c = match *c {
        0 => at(1),
        _ => b'?',
    };

    // XXX: if Unicode conversion routine is loaded then use it
    // SAFETY: nothing writes the cell (the module's deviations).
    if let Some(wchar2char) = unsafe { *CD9660_WCHAR2CHAR.get() } {
        *c = wchar2char((u32::from(at(0)) << 8) | u32::from(at(1)));
    }

    2
}

/// `isofncmp`: translate and compare a filename; returns `fn - isofn` (zero when they
/// match). Note: Version number plus ';' may be omitted.
pub fn isofncmp(fname: &[u8], isofn: &[u8], joliet_level: i32) -> i32 {
    let isolen = isofn.len();
    let mut c = 0u8;
    let mut f = 0;
    let mut k = 0;

    while f < fname.len() {
        if k >= isolen {
            return i32::from(fname[f]);
        }
        k += isochar(&isofn[k..], isolen - k, joliet_level, &mut c);
        if c == b';' {
            let ch = fname[f];
            f += 1;
            if ch != b';' {
                return i32::from(ch);
            }
            let mut i: i32 = 0;
            while f < fname.len() {
                if !fname[f].is_ascii_digit() {
                    return -1;
                }
                i = i.wrapping_mul(10).wrapping_add(i32::from(fname[f] - b'0'));
                f += 1;
            }
            let mut j: i32 = 0;
            while k < isolen {
                k += isochar(&isofn[k..], isolen - k, joliet_level, &mut c);
                j = j
                    .wrapping_mul(10)
                    .wrapping_add(i32::from(c))
                    .wrapping_sub(i32::from(b'0'));
            }
            return i.wrapping_sub(j);
        }
        let ch = fname[f];
        if c != ch {
            if c.is_ascii_uppercase() {
                if c + (b'a' - b'A') != ch {
                    if ch.is_ascii_lowercase() {
                        return i32::from(ch) - i32::from(b'a' - b'A') - i32::from(c);
                    } else {
                        return i32::from(ch) - i32::from(c);
                    }
                }
            } else {
                return i32::from(ch) - i32::from(c);
            }
        }
        f += 1;
    }
    if k < isolen {
        k += isochar(&isofn[k..], isolen - k, joliet_level, &mut c);
        match c {
            b'.' => {
                if k < isolen {
                    isochar(&isofn[k..], isolen - k, joliet_level, &mut c);
                    if c == b';' {
                        return 0;
                    }
                }
                return -1;
            }
            b';' => return 0,
            _ => return -i32::from(c),
        }
    }
    0
}

/// `isofntrans`: translate a filename of length > 0 into `outfn`, setting `outfnlen`.
/// Unless `original`, the version (`;1`) and a `.` just before it are dropped; `assoc`
/// prefixes `ASSOCCHAR`.
pub fn isofntrans(
    infn: &[u8],
    outfn: &mut [u8],
    outfnlen: &mut u16,
    original: bool,
    assoc: bool,
    joliet_level: i32,
) {
    let mut fnidx: i32 = 0;
    let mut o = 0;
    let mut put = |c: u8| {
        if let Some(b) = outfn.get_mut(o) {
            *b = c;
        }
        o += 1;
    };
    let mut d = 0u8;

    if assoc {
        put(ASSOCCHAR);
        fnidx += 1;
    }
    let mut i = 0;
    while i < infn.len() {
        let mut c = 0u8;
        i += isochar(&infn[i..], infn.len() - i, joliet_level, &mut c);

        if !original && c == b';' {
            fnidx -= i32::from(d == b'.');
            break;
        }
        put(c);
        d = c;
        fnidx += 1;
    }
    *outfnlen = fnidx as u16;
}
/* </CODE> */

/* <TESTS> */
#[cfg(test)]
mod tests {
    // Host tests for ISO 9660 name handling: one character of a plain or Joliet name, the
    // comparison of a path component with a recorded name, and the translation shown to users.

    use std::vec::Vec;

    use super::*;

    /// `isofntrans` into a fresh buffer: the name and the length it reports.
    fn trans(infn: &[u8], original: bool, assoc: bool, joliet_level: i32) -> (Vec<u8>, u16) {
        let mut out = [0u8; 300];
        let mut len = 0u16;
        isofntrans(infn, &mut out, &mut len, original, assoc, joliet_level);
        (out[..usize::from(len)].to_vec(), len)
    }

    /// A Joliet (UCS-2 big-endian) name of ASCII characters.
    fn ucs2(s: &[u8]) -> Vec<u8> {
        s.iter().flat_map(|&c| [0, c]).collect()
    }

    #[test]
    fn isochar_reads_plain_and_joliet_characters() {
        let mut c = 0u8;
        assert_eq!(isochar(b"AB", 2, 0, &mut c), 1);
        assert_eq!(c, b'A');
        // Joliet: two bytes, the high one zero for ASCII, '?' otherwise (no Unicode support)
        assert_eq!(isochar(&[0x00, b'x'], 2, 1, &mut c), 2);
        assert_eq!(c, b'x');
        assert_eq!(isochar(&[0x30, 0x42], 2, 2, &mut c), 2);
        assert_eq!(c, b'?');
        // (00) and (01) are one byte in Joliet, too
        assert_eq!(isochar(&[0x01], 1, 1, &mut c), 1);
        assert_eq!(c, 1);
        // past the buffer reads as zero
        assert_eq!(isochar(&[], 0, 0, &mut c), 1);
        assert_eq!(c, 0);
    }

    #[test]
    fn isofncmp_matches_names_without_case_or_version() {
        assert_eq!(isofncmp(b"m10c_iso.txt", b"M10C_ISO.TXT;1", 0), 0);
        assert_eq!(isofncmp(b"M10C_ISO.TXT", b"M10C_ISO.TXT;1", 0), 0);
        // an explicit version must match
        assert_eq!(isofncmp(b"foo.txt;1", b"FOO.TXT;1", 0), 0);
        assert_eq!(isofncmp(b"foo.txt;12", b"FOO.TXT;1", 0), 11);
        assert_eq!(isofncmp(b"foo.txt;x", b"FOO.TXT;1", 0), -1);
        assert_eq!(isofncmp(b"foo.txt.", b"FOO.TXT;1", 0), i32::from(b'.'));
        // a name without extension is recorded with a trailing dot
        assert_eq!(isofncmp(b"foo", b"FOO.;1", 0), 0);
        assert_eq!(isofncmp(b"foo", b"FOO.TXT;1", 0), -1);
        assert_eq!(isofncmp(b"foo", b"FOO", 0), 0);
    }

    #[test]
    fn isofncmp_orders_mismatches_like_the_c() {
        // lowercase input against an uppercase record compares case-insensitively
        assert_eq!(
            isofncmp(b"fop", b"FOO;1", 0),
            i32::from(b'p' - 32) - i32::from(b'O')
        );
        // anything else compares the bytes
        assert_eq!(
            isofncmp(b"F_A", b"F-A;1", 0),
            i32::from(b'_') - i32::from(b'-')
        );
        assert_eq!(isofncmp(b"A", b"b;1", 0), i32::from(b'A') - i32::from(b'b'));
        // the recorded name ran out first: the next input character
        assert_eq!(isofncmp(b"foo.txt", b"FOO", 0), i32::from(b'.'));
        // the input ran out first: minus the next recorded character
        assert_eq!(isofncmp(b"fo", b"FOX;1", 0), -i32::from(b'X'));
    }

    #[test]
    fn isofncmp_reads_joliet_names() {
        assert_eq!(isofncmp(b"foo.txt", &ucs2(b"Foo.txt;1"), 1), 0);
        assert_eq!(isofncmp(b"foo.txt;1", &ucs2(b"foo.txt;1"), 3), 0);
        assert_ne!(isofncmp(b"foo.txu", &ucs2(b"foo.txt;1"), 2), 0);
    }

    #[test]
    fn isofntrans_strips_versions_unless_original() {
        assert_eq!(
            trans(b"M10C_ISO.TXT;1", false, false, 0),
            (b"M10C_ISO.TXT".to_vec(), 12)
        );
        // the dot before the version of a name without extension is dropped too
        assert_eq!(
            trans(b"README.;1", false, false, 0),
            (b"README".to_vec(), 6)
        );
        assert_eq!(
            trans(b"M10C_ISO.TXT;1", true, false, 0),
            (b"M10C_ISO.TXT;1".to_vec(), 14)
        );
        // associated files get a leading '='
        assert_eq!(trans(b"ICON.;1", false, true, 0), (b"=ICON".to_vec(), 5));
        // Joliet
        assert_eq!(
            trans(&ucs2(b"Mixed.case;1"), false, false, 1),
            (b"Mixed.case".to_vec(), 10)
        );
        // no byte is written past the output
        let mut out = [0u8; 4];
        let mut len = 0u16;
        isofntrans(b"LONGNAME", &mut out, &mut len, true, false, 0);
        assert_eq!((&out, len), (b"LONG", 8));
    }
}
/* </TESTS> */
