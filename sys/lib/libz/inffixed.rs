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

/* inffixed.h -- table for decoding fixed codes
 * Generated automatically by makefixed().
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
//! The decoding tables of deflate's fixed codes (RFC 1951, section 3.2.6), as zlib's
//! `makefixed()` generates them into `inffixed.h`.
//!
//! Upstream: sys/lib/libz/inffixed.h @ 3ce1f3f79392
//!
//! **This is an altered source version, not the original zlib `inffixed.h`** (zlib licence,
//! clause 2): the same tables in Rust, written for EmiBSD. `inffixed.h` is generated and has
//! no licence text of its own; it is part of zlib, so the zlib.h notice that covers it is kept
//! above in full (clause 3).
//!
//! `lenfix` is the literal/length table with 9 root bits, `distfix` the distance table with 5
//! (see `inftrees.rs` for what the entries mean). The entries of `lenfix` whose low seven
//! bits are 99 (`(low & 127) == 99`) hold the unused codes 286 and 287: `makefixed()` writes
//! them with `op` 64, the plain invalid-code marker, whatever `inflate_table` builds there.
//!
//! ## Deviations
//! - None: `inflate_table` rebuilds both tables exactly (`inftrees.rs` checks it), which
//!   is what `makefixed()` does. `BUILDFIXED` (building them at run time) is not defined in
//!   the kernel build, so these tables are what the kernel uses.

#![allow(non_upper_case_globals)] // the C's table names

use crate::inftrees::Code;

/// One table entry, written compactly: `op`, `bits`, `val`.
const fn c(op: u8, bits: u8, val: u16) -> Code {
    Code { op, bits, val }
}

/// `lenfix`: the fixed literal/length decoding table, 9 index bits.
#[rustfmt::skip]
pub(crate) static lenfix: [Code; 512] = [
    c(96, 7, 0), c(0, 8, 80), c(0, 8, 16), c(20, 8, 115), c(18, 7, 31), c(0, 8, 112),
    c(0, 8, 48), c(0, 9, 192), c(16, 7, 10), c(0, 8, 96), c(0, 8, 32), c(0, 9, 160),
    c(0, 8, 0), c(0, 8, 128), c(0, 8, 64), c(0, 9, 224), c(16, 7, 6), c(0, 8, 88),
    c(0, 8, 24), c(0, 9, 144), c(19, 7, 59), c(0, 8, 120), c(0, 8, 56), c(0, 9, 208),
    c(17, 7, 17), c(0, 8, 104), c(0, 8, 40), c(0, 9, 176), c(0, 8, 8), c(0, 8, 136),
    c(0, 8, 72), c(0, 9, 240), c(16, 7, 4), c(0, 8, 84), c(0, 8, 20), c(21, 8, 227),
    c(19, 7, 43), c(0, 8, 116), c(0, 8, 52), c(0, 9, 200), c(17, 7, 13), c(0, 8, 100),
    c(0, 8, 36), c(0, 9, 168), c(0, 8, 4), c(0, 8, 132), c(0, 8, 68), c(0, 9, 232),
    c(16, 7, 8), c(0, 8, 92), c(0, 8, 28), c(0, 9, 152), c(20, 7, 83), c(0, 8, 124),
    c(0, 8, 60), c(0, 9, 216), c(18, 7, 23), c(0, 8, 108), c(0, 8, 44), c(0, 9, 184),
    c(0, 8, 12), c(0, 8, 140), c(0, 8, 76), c(0, 9, 248), c(16, 7, 3), c(0, 8, 82),
    c(0, 8, 18), c(21, 8, 163), c(19, 7, 35), c(0, 8, 114), c(0, 8, 50), c(0, 9, 196),
    c(17, 7, 11), c(0, 8, 98), c(0, 8, 34), c(0, 9, 164), c(0, 8, 2), c(0, 8, 130),
    c(0, 8, 66), c(0, 9, 228), c(16, 7, 7), c(0, 8, 90), c(0, 8, 26), c(0, 9, 148),
    c(20, 7, 67), c(0, 8, 122), c(0, 8, 58), c(0, 9, 212), c(18, 7, 19), c(0, 8, 106),
    c(0, 8, 42), c(0, 9, 180), c(0, 8, 10), c(0, 8, 138), c(0, 8, 74), c(0, 9, 244),
    c(16, 7, 5), c(0, 8, 86), c(0, 8, 22), c(64, 8, 0), c(19, 7, 51), c(0, 8, 118),
    c(0, 8, 54), c(0, 9, 204), c(17, 7, 15), c(0, 8, 102), c(0, 8, 38), c(0, 9, 172),
    c(0, 8, 6), c(0, 8, 134), c(0, 8, 70), c(0, 9, 236), c(16, 7, 9), c(0, 8, 94),
    c(0, 8, 30), c(0, 9, 156), c(20, 7, 99), c(0, 8, 126), c(0, 8, 62), c(0, 9, 220),
    c(18, 7, 27), c(0, 8, 110), c(0, 8, 46), c(0, 9, 188), c(0, 8, 14), c(0, 8, 142),
    c(0, 8, 78), c(0, 9, 252), c(96, 7, 0), c(0, 8, 81), c(0, 8, 17), c(21, 8, 131),
    c(18, 7, 31), c(0, 8, 113), c(0, 8, 49), c(0, 9, 194), c(16, 7, 10), c(0, 8, 97),
    c(0, 8, 33), c(0, 9, 162), c(0, 8, 1), c(0, 8, 129), c(0, 8, 65), c(0, 9, 226),
    c(16, 7, 6), c(0, 8, 89), c(0, 8, 25), c(0, 9, 146), c(19, 7, 59), c(0, 8, 121),
    c(0, 8, 57), c(0, 9, 210), c(17, 7, 17), c(0, 8, 105), c(0, 8, 41), c(0, 9, 178),
    c(0, 8, 9), c(0, 8, 137), c(0, 8, 73), c(0, 9, 242), c(16, 7, 4), c(0, 8, 85),
    c(0, 8, 21), c(16, 8, 258), c(19, 7, 43), c(0, 8, 117), c(0, 8, 53), c(0, 9, 202),
    c(17, 7, 13), c(0, 8, 101), c(0, 8, 37), c(0, 9, 170), c(0, 8, 5), c(0, 8, 133),
    c(0, 8, 69), c(0, 9, 234), c(16, 7, 8), c(0, 8, 93), c(0, 8, 29), c(0, 9, 154),
    c(20, 7, 83), c(0, 8, 125), c(0, 8, 61), c(0, 9, 218), c(18, 7, 23), c(0, 8, 109),
    c(0, 8, 45), c(0, 9, 186), c(0, 8, 13), c(0, 8, 141), c(0, 8, 77), c(0, 9, 250),
    c(16, 7, 3), c(0, 8, 83), c(0, 8, 19), c(21, 8, 195), c(19, 7, 35), c(0, 8, 115),
    c(0, 8, 51), c(0, 9, 198), c(17, 7, 11), c(0, 8, 99), c(0, 8, 35), c(0, 9, 166),
    c(0, 8, 3), c(0, 8, 131), c(0, 8, 67), c(0, 9, 230), c(16, 7, 7), c(0, 8, 91),
    c(0, 8, 27), c(0, 9, 150), c(20, 7, 67), c(0, 8, 123), c(0, 8, 59), c(0, 9, 214),
    c(18, 7, 19), c(0, 8, 107), c(0, 8, 43), c(0, 9, 182), c(0, 8, 11), c(0, 8, 139),
    c(0, 8, 75), c(0, 9, 246), c(16, 7, 5), c(0, 8, 87), c(0, 8, 23), c(64, 8, 0),
    c(19, 7, 51), c(0, 8, 119), c(0, 8, 55), c(0, 9, 206), c(17, 7, 15), c(0, 8, 103),
    c(0, 8, 39), c(0, 9, 174), c(0, 8, 7), c(0, 8, 135), c(0, 8, 71), c(0, 9, 238),
    c(16, 7, 9), c(0, 8, 95), c(0, 8, 31), c(0, 9, 158), c(20, 7, 99), c(0, 8, 127),
    c(0, 8, 63), c(0, 9, 222), c(18, 7, 27), c(0, 8, 111), c(0, 8, 47), c(0, 9, 190),
    c(0, 8, 15), c(0, 8, 143), c(0, 8, 79), c(0, 9, 254), c(96, 7, 0), c(0, 8, 80),
    c(0, 8, 16), c(20, 8, 115), c(18, 7, 31), c(0, 8, 112), c(0, 8, 48), c(0, 9, 193),
    c(16, 7, 10), c(0, 8, 96), c(0, 8, 32), c(0, 9, 161), c(0, 8, 0), c(0, 8, 128),
    c(0, 8, 64), c(0, 9, 225), c(16, 7, 6), c(0, 8, 88), c(0, 8, 24), c(0, 9, 145),
    c(19, 7, 59), c(0, 8, 120), c(0, 8, 56), c(0, 9, 209), c(17, 7, 17), c(0, 8, 104),
    c(0, 8, 40), c(0, 9, 177), c(0, 8, 8), c(0, 8, 136), c(0, 8, 72), c(0, 9, 241),
    c(16, 7, 4), c(0, 8, 84), c(0, 8, 20), c(21, 8, 227), c(19, 7, 43), c(0, 8, 116),
    c(0, 8, 52), c(0, 9, 201), c(17, 7, 13), c(0, 8, 100), c(0, 8, 36), c(0, 9, 169),
    c(0, 8, 4), c(0, 8, 132), c(0, 8, 68), c(0, 9, 233), c(16, 7, 8), c(0, 8, 92),
    c(0, 8, 28), c(0, 9, 153), c(20, 7, 83), c(0, 8, 124), c(0, 8, 60), c(0, 9, 217),
    c(18, 7, 23), c(0, 8, 108), c(0, 8, 44), c(0, 9, 185), c(0, 8, 12), c(0, 8, 140),
    c(0, 8, 76), c(0, 9, 249), c(16, 7, 3), c(0, 8, 82), c(0, 8, 18), c(21, 8, 163),
    c(19, 7, 35), c(0, 8, 114), c(0, 8, 50), c(0, 9, 197), c(17, 7, 11), c(0, 8, 98),
    c(0, 8, 34), c(0, 9, 165), c(0, 8, 2), c(0, 8, 130), c(0, 8, 66), c(0, 9, 229),
    c(16, 7, 7), c(0, 8, 90), c(0, 8, 26), c(0, 9, 149), c(20, 7, 67), c(0, 8, 122),
    c(0, 8, 58), c(0, 9, 213), c(18, 7, 19), c(0, 8, 106), c(0, 8, 42), c(0, 9, 181),
    c(0, 8, 10), c(0, 8, 138), c(0, 8, 74), c(0, 9, 245), c(16, 7, 5), c(0, 8, 86),
    c(0, 8, 22), c(64, 8, 0), c(19, 7, 51), c(0, 8, 118), c(0, 8, 54), c(0, 9, 205),
    c(17, 7, 15), c(0, 8, 102), c(0, 8, 38), c(0, 9, 173), c(0, 8, 6), c(0, 8, 134),
    c(0, 8, 70), c(0, 9, 237), c(16, 7, 9), c(0, 8, 94), c(0, 8, 30), c(0, 9, 157),
    c(20, 7, 99), c(0, 8, 126), c(0, 8, 62), c(0, 9, 221), c(18, 7, 27), c(0, 8, 110),
    c(0, 8, 46), c(0, 9, 189), c(0, 8, 14), c(0, 8, 142), c(0, 8, 78), c(0, 9, 253),
    c(96, 7, 0), c(0, 8, 81), c(0, 8, 17), c(21, 8, 131), c(18, 7, 31), c(0, 8, 113),
    c(0, 8, 49), c(0, 9, 195), c(16, 7, 10), c(0, 8, 97), c(0, 8, 33), c(0, 9, 163),
    c(0, 8, 1), c(0, 8, 129), c(0, 8, 65), c(0, 9, 227), c(16, 7, 6), c(0, 8, 89),
    c(0, 8, 25), c(0, 9, 147), c(19, 7, 59), c(0, 8, 121), c(0, 8, 57), c(0, 9, 211),
    c(17, 7, 17), c(0, 8, 105), c(0, 8, 41), c(0, 9, 179), c(0, 8, 9), c(0, 8, 137),
    c(0, 8, 73), c(0, 9, 243), c(16, 7, 4), c(0, 8, 85), c(0, 8, 21), c(16, 8, 258),
    c(19, 7, 43), c(0, 8, 117), c(0, 8, 53), c(0, 9, 203), c(17, 7, 13), c(0, 8, 101),
    c(0, 8, 37), c(0, 9, 171), c(0, 8, 5), c(0, 8, 133), c(0, 8, 69), c(0, 9, 235),
    c(16, 7, 8), c(0, 8, 93), c(0, 8, 29), c(0, 9, 155), c(20, 7, 83), c(0, 8, 125),
    c(0, 8, 61), c(0, 9, 219), c(18, 7, 23), c(0, 8, 109), c(0, 8, 45), c(0, 9, 187),
    c(0, 8, 13), c(0, 8, 141), c(0, 8, 77), c(0, 9, 251), c(16, 7, 3), c(0, 8, 83),
    c(0, 8, 19), c(21, 8, 195), c(19, 7, 35), c(0, 8, 115), c(0, 8, 51), c(0, 9, 199),
    c(17, 7, 11), c(0, 8, 99), c(0, 8, 35), c(0, 9, 167), c(0, 8, 3), c(0, 8, 131),
    c(0, 8, 67), c(0, 9, 231), c(16, 7, 7), c(0, 8, 91), c(0, 8, 27), c(0, 9, 151),
    c(20, 7, 67), c(0, 8, 123), c(0, 8, 59), c(0, 9, 215), c(18, 7, 19), c(0, 8, 107),
    c(0, 8, 43), c(0, 9, 183), c(0, 8, 11), c(0, 8, 139), c(0, 8, 75), c(0, 9, 247),
    c(16, 7, 5), c(0, 8, 87), c(0, 8, 23), c(64, 8, 0), c(19, 7, 51), c(0, 8, 119),
    c(0, 8, 55), c(0, 9, 207), c(17, 7, 15), c(0, 8, 103), c(0, 8, 39), c(0, 9, 175),
    c(0, 8, 7), c(0, 8, 135), c(0, 8, 71), c(0, 9, 239), c(16, 7, 9), c(0, 8, 95),
    c(0, 8, 31), c(0, 9, 159), c(20, 7, 99), c(0, 8, 127), c(0, 8, 63), c(0, 9, 223),
    c(18, 7, 27), c(0, 8, 111), c(0, 8, 47), c(0, 9, 191), c(0, 8, 15), c(0, 8, 143),
    c(0, 8, 79), c(0, 9, 255),
];

/// `distfix`: the fixed distance decoding table, 5 index bits.
#[rustfmt::skip]
pub(crate) static distfix: [Code; 32] = [
    c(16, 5, 1), c(23, 5, 257), c(19, 5, 17), c(27, 5, 4097), c(17, 5, 5), c(25, 5, 1025),
    c(21, 5, 65), c(29, 5, 16385), c(16, 5, 3), c(24, 5, 513), c(20, 5, 33), c(28, 5, 8193),
    c(18, 5, 9), c(26, 5, 2049), c(22, 5, 129), c(64, 5, 0), c(16, 5, 2), c(23, 5, 385),
    c(19, 5, 25), c(27, 5, 6145), c(17, 5, 7), c(25, 5, 1537), c(21, 5, 97), c(29, 5, 24577),
    c(16, 5, 4), c(24, 5, 769), c(20, 5, 49), c(28, 5, 12289), c(18, 5, 13), c(26, 5, 3073),
    c(22, 5, 193), c(64, 5, 0),
];
/* </CODE> */
