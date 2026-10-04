/*	$OpenBSD: cd9660_util.c,v 1.11 2021/03/05 07:01:36 jsg Exp $	*/
/*	$NetBSD: cd9660_util.c,v 1.12 1997/01/24 00:27:33 cgd Exp $	*/
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

#[cfg(test)]
mod tests;
