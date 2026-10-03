/*	$OpenBSD: scanc.c,v 1.5 2004/08/07 00:38:33 deraadt Exp $	*/
/*	$NetBSD: scanc.c,v 1.3 1996/03/14 18:52:16 christos Exp $	*/
/* <LICENSES> */
/*
 * Copyright (c) 1982, 1986, 1989 Regents of the University of California.
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
 *	@(#)ufs_subr.c	7.13 (Berkeley) 6/28/90
 */
/* </LICENSES> */

//! `scanc(9)`: scan a byte string for the first byte whose class, looked up in a table,
//! shares a bit with a mask.
//!
//! Upstream: sys/lib/libkern/scanc.c @ 3ce1f3f79392
//!
//! ## Deviations
//! - Operates on a byte slice: the C's `size` and `cp` are `cp.len()` and `cp`, and the
//!   count returned is a `usize`. The table is `&[u8; 256]`, so every byte has a class and
//!   the lookup cannot go out of bounds.

/// The number of bytes left in `cp` from the first one whose `table` entry has a bit of
/// `mask` set (that byte included); 0 when there is none.
pub fn scanc(cp: &[u8], table: &[u8; 256], mask: u8) -> usize {
    let skipped = cp
        .iter()
        .position(|&c| table[usize::from(c)] & mask != 0)
        .unwrap_or(cp.len());
    cp.len() - skipped
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn table() {
        let mut classes = [0u8; 256];
        classes[usize::from(b'\n')] = 0x3;
        classes[usize::from(b'\t')] = 0x4;
        let cases: &[(&[u8], u8, usize)] = &[
            (b"", 0x3f, 0),
            (b"abc", 0x3f, 0),
            (b"abc\n", 0x3f, 1),
            (b"\nabc", 0x3f, 4),
            (b"ab\tc\n", 0x3f, 3),
            (b"ab\tc\n", 0x3, 1),
            (b"ab\tc\n", 0x80, 0),
        ];
        for &(s, mask, want) in cases {
            assert_eq!(scanc(s, &classes, mask), want, "scanc({s:?}, {mask:#x})");
        }
    }
}
