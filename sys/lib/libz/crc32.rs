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

/* crc32.c -- compute the CRC-32 of a data stream
 * Copyright (C) 1995-2026 Mark Adler
 * For conditions of distribution and use, see copyright notice in zlib.h
 *
 * This interleaved implementation of a CRC makes use of pipelined multiple
 * arithmetic-logic units, commonly found in modern CPU cores. It is due to
 * Kadatch and Jenkins (2010). See doc/crc-doc.1.0.pdf in this distribution.
 */

/* zlib.h -- interface of the 'zlib' general purpose compression library
  version 1.3.2, February 17th, 2026

  Copyright (C) 1995-2026 Jean-loup Gailly and Mark Adler

  This software is provided 'as-is', without any express or implied
  warranty.  In no event will the authors be held liable for any damages
  arising from the use of this software.

  Permission is granted to anyone to use this software for any purpose,
  including commercial applications, and to alter it and redistribute it
  freely, subject to the following restrictions:

  1. The origin of this software must not be misrepresented; you must not
     claim that you wrote the original software. If you use this software
     in a product, an acknowledgment in the product documentation would be
     appreciated but is not required.
  2. Altered source versions must be plainly marked as such, and must not be
     misrepresented as being the original software.
  3. This notice may not be removed or altered from any source distribution.

  Jean-loup Gailly        Mark Adler
  jloup@gzip.org          madler@alumni.caltech.edu


  The data format used by the zlib library is described by RFCs (Request for
  Comments) 1950 to 1952 at https://datatracker.ietf.org/doc/html/rfc1950
  (zlib format), rfc1951 (deflate format) and rfc1952 (gzip format).
*/
/* </LICENSES> */

/* <CODE> */
//! The plain CRC-32 (polynomial `0xedb88320`, reflected), as zlib's `crc32()` computes it.
//!
//! Upstream: sys/lib/libz/crc32.c @ 3ce1f3f79392
//!
//! **This is an altered source version, not the original zlib `crc32.c`** (zlib licence, clause
//! 2): it is a Rust rewrite of the file's observable behaviour, written for EmiBSD. The
//! original's notice is kept above in full (clause 3). Do not mistake it for the zlib
//! distribution; bugs here are not Mark Adler's.
//!
//! `crc32(crc, buf)` starts from `0` and may be chained: feeding consecutive chunks through
//! successive calls yields the CRC of their concatenation. The kernel's one user is
//! `subr_disk.c` (`gpt_get_hdr` and `gpt_get_parts`, which checksum the GPT header and the
//! partition entries). The kernel's own CRC-32C is `lib/libkern/crc32c.h`, a different polynomial.
//!
//! ## Deviations
//! - Only the byte-at-a-time algorithm is ported. The original's braided (Kadatch and Jenkins),
//!   word-at-a-time implementation, the `ARMCRC32` and `HAVE_S390X_VX` hooks are not: they only
//!   make the same function faster on large buffers, and the results are identical. The 256-entry
//!   table is computed at compile time from `POLY`, so `crc_table` is never written out as a
//!   header and `DYNAMIC_CRC_TABLE`, `MAKECRCH` (which writes `crc32.h`) and `get_crc_table` have
//!   no counterpart; the crate has no `z_once` and no run-time initialisation.
//! - `crc32_z` and `crc32` are one function: `uInt len` against `z_size_t len` only matters for
//!   buffers over 4 GiB, and a slice carries its length. `crc` and the result are `u32`, not
//!   `uLong` (the value never exceeds 32 bits).
//! - `crc32(crc, Z_NULL, 0)` returning the initial value `0` has no counterpart: a slice is never
//!   null, and an empty slice leaves the CRC unchanged (`crc32(0, &[]) == 0`).
//! - `crc32_combine`, `crc32_combine_gen`, `crc32_combine_op` and their `64` variants (GF(2)
//!   matrix arithmetic with `x2nmodp`/`multmodp`) are not ported: no kernel code calls them
//!   (`subr_disk.c` only uses `crc32`). They are the only code paths of the file left out; port
//!   them when a caller appears.

/// `POLY`: the CRC-32 polynomial, reflected, with `x^32` implied.
const POLY: u32 = 0xedb8_8320;

/// `crc_table`: the CRC of each byte value.
const CRC_TABLE: [u32; 256] = build_table();

const fn build_table() -> [u32; 256] {
    let mut table = [0u32; 256];
    let mut i = 0;
    while i < table.len() {
        let mut crc = i as u32;
        let mut bit = 0;
        while bit < 8 {
            crc = if crc & 1 != 0 {
                (crc >> 1) ^ POLY
            } else {
                crc >> 1
            };
            bit += 1;
        }
        table[i] = crc;
        i += 1;
    }
    table
}

/// `crc32`: the CRC-32 of `buf`, continuing from `crc` (pass `0` to start). The running value is
/// inverted on entry and on exit, so feeding consecutive chunks through successive calls yields
/// the CRC of their concatenation.
pub fn crc32(crc: u32, buf: &[u8]) -> u32 {
    let mut crc = !crc;
    for &b in buf {
        crc = CRC_TABLE[((crc ^ b as u32) & 0xff) as usize] ^ (crc >> 8);
    }
    !crc
}
/* </CODE> */

/* <TESTS> */
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn known_vectors() {
        let cases: &[(&[u8], u32)] = &[
            (b"", 0),
            (b"a", 0xe8b7_be43),
            (b"abc", 0x3524_41c2),
            (b"123456789", 0xcbf4_3926),
            (b"The quick brown fox jumps over the lazy dog", 0x414f_a339),
        ];
        for &(input, want) in cases {
            assert_eq!(crc32(0, input), want, "{:?}", core::str::from_utf8(input));
        }
    }

    #[test]
    fn table_spot_checks() {
        assert_eq!(CRC_TABLE[0], 0x0000_0000);
        assert_eq!(CRC_TABLE[1], 0x7707_3096);
        assert_eq!(CRC_TABLE[2], 0xee0e_612c);
        assert_eq!(CRC_TABLE[128], 0xedb8_8320);
        assert_eq!(CRC_TABLE[255], 0x2d02_ef8d);
    }

    #[test]
    fn chaining_chunks_equals_one_call() {
        let whole = b"The quick brown fox jumps over the lazy dog";
        for split in 0..=whole.len() {
            let (a, b) = whole.split_at(split);
            assert_eq!(crc32(crc32(0, a), b), crc32(0, whole), "split at {split}");
        }
    }

    #[test]
    fn empty_buffer_leaves_the_crc_alone() {
        assert_eq!(crc32(0xdead_beef, b""), 0xdead_beef);
    }
}
/* </TESTS> */
