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
//! LZ: sys/lib/libz/crc32.rs@f5985f1d055a
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
//!
//! ## Redesign
//! - Tests only: more published and zlib 1.2.12 vectors (start values other than 0 too), the
//!   table against its bitwise definition, and chaining over random splits. The code is LZ's.

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

    /// More vectors: the CRC-32 of 32 zero, 32 0xff, ascending and descending bytes (the RFC 3720
    /// test patterns, here with the plain CRC-32), and zlib 1.2.12's values (Python's
    /// `zlib.crc32`) for longer inputs.
    #[test]
    fn more_vectors() {
        let up: std::vec::Vec<u8> = (0..32).collect();
        let down: std::vec::Vec<u8> = (0..32).rev().collect();
        let a_million = std::vec![b'a'; 1_000_000];
        let cases: &[(&[u8], u32)] = &[
            (&[0; 32], 0x190a_55ad),
            (&[0xff; 32], 0xff6c_ab0b),
            (&up, 0x9126_7e8a),
            (&down, 0x9ab0_ef72),
            (&a_million, 0xdc25_bfbc),
            (b"message digest", 0x2015_9d7f),
            (b"abcdefghijklmnopqrstuvwxyz", 0x4c27_50bd),
            (
                b"12345678901234567890123456789012345678901234567890123456789012345678901234567890",
                0x7ca9_4a72,
            ),
        ];
        for &(input, want) in cases {
            assert_eq!(crc32(0, input), want, "len {}", input.len());
        }
    }

    /// Start values other than 0 continue as zlib 1.2.12 does (`zlib.crc32(data, start)`).
    #[test]
    fn start_values_follow_zlib() {
        let ramp: std::vec::Vec<u8> = (0..22).flat_map(|_| 0..=255u8).collect();
        let inputs: [&[u8]; 5] = [b"\xff", b"\x00", b"abcde", &[0xff; 16], &ramp];
        let table: [(u32, [u32; 5]); 4] = [
            (
                0x0000_0000,
                [0xff000000, 0xd202ef8d, 0x8587d865, 0x3fb3c61a, 0x9b9a2e36],
            ),
            (
                0x0000_0001,
                [0x88073096, 0xa505df1b, 0xb8e7f1d5, 0x91db578b, 0x1735d36c],
            ),
            (
                0xffff_ffff,
                [0xd2fd1072, 0xffffffff, 0xbc5ad087, 0x2cf772b0, 0xddbfac2b],
            ),
            (
                0xdead_beef,
                [0xcf6b5257, 0xe269bdda, 0x1cb59760, 0x115d71fb, 0x8459e813],
            ),
        ];
        for (start, wants) in table {
            for (input, want) in inputs.iter().zip(wants) {
                assert_eq!(
                    crc32(start, input),
                    want,
                    "start {start:#010x} len {}",
                    input.len()
                );
            }
        }
    }

    /// Every table entry is the CRC of its byte value computed bit by bit (and so the CRC of a
    /// single byte agrees with a bitwise CRC for every value).
    #[test]
    fn table_matches_the_bitwise_definition() {
        fn bitwise(crc: u32, buf: &[u8]) -> u32 {
            let mut c = !crc;
            for &b in buf {
                c ^= u32::from(b);
                for _ in 0..8 {
                    c = if c & 1 != 0 { (c >> 1) ^ POLY } else { c >> 1 };
                }
            }
            !c
        }
        for (i, &entry) in CRC_TABLE.iter().enumerate() {
            let mut c = i as u32;
            for _ in 0..8 {
                c = if c & 1 != 0 { (c >> 1) ^ POLY } else { c >> 1 };
            }
            assert_eq!(entry, c, "entry {i}");
            let b = [i as u8];
            assert_eq!(crc32(0, &b), bitwise(0, &b));
            assert_eq!(crc32(0x5a5a_5a5a, &b), bitwise(0x5a5a_5a5a, &b));
        }
    }

    /// Random data cut at random points: chaining the pieces gives the one-call CRC.
    #[test]
    fn random_splits_chain() {
        let mut x = 0x2545_f491_4f6c_dd1du64;
        let mut next = move || {
            x ^= x << 13;
            x ^= x >> 7;
            x ^= x << 17;
            x
        };
        for round in 0..200 {
            let len = (next() % if round % 10 == 0 { 100_000 } else { 2000 }) as usize;
            let data: std::vec::Vec<u8> = (0..len).map(|_| next() as u8).collect();
            let whole = crc32(0, &data);
            let mut cuts: std::vec::Vec<usize> = (0..next() % 8)
                .map(|_| (next() % (len as u64 + 1)) as usize)
                .collect();
            cuts.push(0);
            cuts.push(len);
            cuts.sort_unstable();
            let chained = cuts
                .windows(2)
                .fold(0, |crc, w| crc32(crc, &data[w[0]..w[1]]));
            assert_eq!(chained, whole, "round {round} {cuts:?}");
        }
    }
}
/* </TESTS> */
