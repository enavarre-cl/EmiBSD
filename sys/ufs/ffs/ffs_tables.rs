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
 * Copyright (c) 1982, 1986, 1993
 *	The Regents of the University of California.  All rights reserved.
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
 *	@(#)ffs_tables.c	8.1 (Berkeley) 6/11/93
 */
/* </LICENSES> */

/* <CODE> */
//! The bit-pattern tables of the fast file system's fragment allocator: `around[]` and
//! `inside[]`, which identify a run of free fragments in the block map as `(map & around) ==
//! inside`, and `fragtbl[]`, which tells for a block map byte whether a free run of a given
//! size is available.
//!
//! Upstream: sys/ufs/ffs/ffs_tables.c @ 3ce1f3f79392
//!
//! The frag tables are used as: if `(1 << (size - 1)) & fragtbl[fs->fs_frag][map]` then at
//! least one fragment of the indicated size is available. These tables are used by the
//! `scanc` instruction on the VAX to quickly find an appropriate fragment.
//!
//! ## Deviations
//! - The tables are computed at compile time from their definition (`docs/C_TO_RUST.md`,
//!   constant tables): `fragtbl8[map]` has bit `n - 1` set for every maximal run of `n` free
//!   fragments in the 8-fragment block `map` describes; `fragtbl124[map]` has, for each block
//!   size `f` of 1, 2 and 4 fragments and each `f`-bit block in `map`, bit `n + f - 1` set for
//!   every maximal run of `n` free fragments in it. The reference-backed test compares every
//!   entry with the C file.
//! - `fragtbl[]` holds `Option<&[u8; 256]>`, `None` for the C's NULL entries; the names are
//!   in capitals (`AROUND`, `INSIDE`, `FRAGTBL`, `FRAGTBL124`, `FRAGTBL8`) as Rust statics.

use crate::ufs::ffs::fs::MAXFRAG;

/// `around[]`: bit patterns for identifying fragments in the block map, used as
/// `((map & around) == inside)`.
pub const AROUND: [i32; 9] = [0x3, 0x7, 0xf, 0x1f, 0x3f, 0x7f, 0xff, 0x1ff, 0x3ff];
/// `inside[]`.
pub const INSIDE: [i32; 9] = [0x0, 0x2, 0x6, 0xe, 0x1e, 0x3e, 0x7e, 0xfe, 0x1fe];

/// For each `f`-bit block of the byte `map`, the bits `n + shift` (`n` from 0) for every
/// maximal run of `n + 1` set bits in it.
const fn runs(map: u8, f: u32, shift: u32) -> u8 {
    let mut out = 0u8;
    let mut blk = 0;
    while blk < 8 / f {
        let bits = (map >> (blk * f)) as u32 & ((1u32 << f) - 1);
        let mut run = 0;
        let mut i = 0;
        while i <= f {
            if i < f && bits & (1 << i) != 0 {
                run += 1;
            } else {
                if run > 0 {
                    out |= 1 << (run - 1 + shift);
                }
                run = 0;
            }
            i += 1;
        }
        blk += 1;
    }
    out
}

/// Builds `fragtbl124`.
const fn build_fragtbl124() -> [u8; 256] {
    let mut t = [0u8; 256];
    let mut map = 0;
    while map < 256 {
        t[map] = runs(map as u8, 1, 1) | runs(map as u8, 2, 2) | runs(map as u8, 4, 4);
        map += 1;
    }
    t
}

/// Builds `fragtbl8`.
const fn build_fragtbl8() -> [u8; 256] {
    let mut t = [0u8; 256];
    let mut map = 0;
    while map < 256 {
        t[map] = runs(map as u8, 8, 0);
        map += 1;
    }
    t
}

/// `fragtbl124[]`: the frag table for blocks of 1, 2 and 4 fragments.
pub static FRAGTBL124: [u8; 256] = build_fragtbl124();

/// `fragtbl8[]`: the frag table for blocks of 8 fragments.
pub static FRAGTBL8: [u8; 256] = build_fragtbl8();

/// `fragtbl[]`: the frag table of each `fs_frag`.
pub static FRAGTBL: [Option<&[u8; 256]>; MAXFRAG + 1] = [
    None,
    Some(&FRAGTBL124),
    Some(&FRAGTBL124),
    None,
    Some(&FRAGTBL124),
    None,
    None,
    None,
    Some(&FRAGTBL8),
];
/* </CODE> */

/* <TESTS> */
#[cfg(test)]
mod tests {
    use std::vec::Vec;

    use super::*;

    /// The hexadecimal bytes of the C array `name` in `ffs_tables.c`.
    fn c_table(text: &str, name: &str) -> Vec<u8> {
        let start = text.find(name).expect("table");
        let body = &text[start..];
        let open = body.find('{').expect("{");
        let close = body.find("};").expect("};");
        body[open + 1..close]
            .split(',')
            .map(str::trim)
            .filter(|s| !s.is_empty())
            .map(|s| u8::from_str_radix(s.trim_start_matches("0x"), 16).expect("hex"))
            .collect()
    }

    #[test]
    #[ignore = "needs OPENBSD_SRC (just test-ref)"]
    fn tables_match_the_c_file() {
        let path = crate::reftest::openbsd_src().join("sys/ufs/ffs/ffs_tables.c");
        let text = std::fs::read_to_string(path).expect("ffs_tables.c");
        assert_eq!(c_table(&text, "fragtbl124[256]"), FRAGTBL124.to_vec());
        assert_eq!(c_table(&text, "fragtbl8[256]"), FRAGTBL8.to_vec());
    }

    #[test]
    fn spot_checks() {
        // One free fragment of 8: a run of 1; all free: a run of 8.
        assert_eq!(FRAGTBL8[0x01], 0x01);
        assert_eq!(FRAGTBL8[0xff], 0x80);
        assert_eq!(FRAGTBL8[0x05], 0x01);
        assert_eq!(FRAGTBL124[0x00], 0x00);
        assert_eq!(FRAGTBL124[0x01], 0x16);
        assert_eq!(FRAGTBL124[0x03], 0x2a);
        assert_eq!(FRAGTBL124[0xff], 0x8a);
        assert!(FRAGTBL[3].is_none() && FRAGTBL[8].is_some());
    }
}
/* </TESTS> */
