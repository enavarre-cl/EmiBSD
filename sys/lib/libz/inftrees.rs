/* <LICENSES> */
/* inftrees.c -- generate Huffman trees for efficient decoding
 * Copyright (C) 1995-2026 Mark Adler
 * For conditions of distribution and use, see copyright notice in zlib.h
 */

/* inftrees.h -- header to use inftrees.c
 * Copyright (C) 1995-2026 Mark Adler
 * For conditions of distribution and use, see copyright notice in zlib.h
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

//! Building inflate's Huffman decoding tables: `inflate_table` turns the code lengths of a
//! canonical Huffman code into a root table indexed by the low bits of the input, with
//! sub-tables for the longer codes; `inflate_fixed` points a state at the fixed-code tables.
//!
//! Upstream: sys/lib/libz/inftrees.c @ 3ce1f3f79392, sys/lib/libz/inftrees.h @ 3ce1f3f79392
//!
//! **This is an altered source version, not the original zlib `inftrees.c`/`inftrees.h`**
//! (zlib licence, clause 2): a Rust rewrite written for EmiBSD. The original's notice is kept
//! above in full (clause 3). Do not mistake it for the zlib distribution.
//!
//! A table entry ([`Code`]) is four bytes: `op` says what the entry is, `bits` how many input
//! bits it consumes, `val` the literal, the base length or distance, or the offset of a
//! sub-table. The values of `op` that `inflate_table` produces:
//!
//! - `00000000`: a literal;
//! - `0000tttt`: a link to a sub-table of `tttt` (non-zero) index bits;
//! - `0001eeee`: a length or a distance, with `eeee` extra bits to read after the code;
//! - `01100000`: the end of the block;
//! - `01000000`: an invalid code.
//!
//! ## Deviations
//! - `code FAR * FAR *table` (the next free entry, advanced by the call) is a slice of the
//!   whole code space plus an offset, `next`, that the call advances; the sub-table pointer
//!   `next` of the C is an index into that slice. `codes`, the number of lengths, is the
//!   length of the `lens` slice.
//! - `inflate_copyright`: this `inftrees.c` does not define the string (the comment asking to
//!   keep it in the executable is still there, and `zutil.h` still declares it), so there is
//!   nothing to port.
//! - `BUILDFIXED` and `MAKEFIXED` are not defined in the kernel build: `buildtables` (which
//!   builds the fixed tables at run time, once, under `z_once`) and `main` (which prints
//!   `inffixed.h`) are not ported; the fixed tables are `inffixed.rs`, and the host tests
//!   check that `inflate_table` rebuilds them exactly, as `makefixed` would.
//! - `inflate_fixed` takes the state; its `lencode`/`distcode` become [`CodeTable`]
//!   selectors of the fixed tables instead of pointers to them.

#![allow(non_upper_case_globals)] // the C's table names (lbase, lext, dbase, dext)
#![cfg_attr(not(test), allow(dead_code))] // inflate.rs, the next commit, uses it all

/// `MAXBITS`: the longest code length deflate allows.
const MAXBITS: usize = 15;

/// `ENOUGH_LENS`: the most table entries a literal/length code can need with a 9-bit root
/// table (852, found by the exhaustive search of zlib's `examples/enough.c`, "enough 286 9
/// 15"). If inflate's root table sizes change, this must be recomputed.
pub(crate) const ENOUGH_LENS: usize = 852;
/// `ENOUGH_DISTS`: the same for a distance code with a 6-bit root table ("enough 30 6 15").
pub(crate) const ENOUGH_DISTS: usize = 592;
/// `ENOUGH`: the size of the state's code space, both tables together (1444 entries).
pub(crate) const ENOUGH: usize = ENOUGH_LENS + ENOUGH_DISTS;

/// `code`: one entry of a decoding table. It either decodes what the bits that index it
/// begin with, or points to a sub-table that indexes more bits of the code.
#[derive(Clone, Copy, Default, PartialEq, Eq, Debug)]
pub(crate) struct Code {
    /// `op`: operation, extra bits, table bits (see the module documentation).
    pub(crate) op: u8,
    /// `bits`: bits in this part of the code.
    pub(crate) bits: u8,
    /// `val`: offset in table or code value.
    pub(crate) val: u16,
}

/// `codetype`: the kind of code `inflate_table` builds.
#[allow(clippy::upper_case_acronyms)] // the C's enumerator names
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub(crate) enum CodeType {
    /// `CODES`: the code length code of a dynamic block (symbols 0..18).
    CODES,
    /// `LENS`: a literal/length code (symbols 0..287).
    LENS,
    /// `DISTS`: a distance code (symbols 0..31).
    DISTS,
}

/// `lbase`: base lengths of the length codes 257..285.
static lbase: [u16; 31] = [
    3, 4, 5, 6, 7, 8, 9, 10, 11, 13, 15, 17, 19, 23, 27, 31, 35, 43, 51, 59, 67, 83, 99, 115, 131,
    163, 195, 227, 258, 0, 0,
];
/// `lext`: `op` of the length codes 257..285: 16 plus the extra bits. The last two (codes 286
/// and 287, which do not occur) are odd values with bit 6 set, so that they decode as invalid.
static lext: [u16; 31] = [
    16, 16, 16, 16, 16, 16, 16, 16, 17, 17, 17, 17, 18, 18, 18, 18, 19, 19, 19, 19, 20, 20, 20, 20,
    21, 21, 21, 21, 16, 199, 75,
];
/// `dbase`: base distances of the distance codes 0..29.
static dbase: [u16; 32] = [
    1, 2, 3, 4, 5, 7, 9, 13, 17, 25, 33, 49, 65, 97, 129, 193, 257, 385, 513, 769, 1025, 1537,
    2049, 3073, 4097, 6145, 8193, 12289, 16385, 24577, 0, 0,
];
/// `dext`: `op` of the distance codes 0..29: 16 plus the extra bits; codes 30 and 31 (which
/// do not occur) are invalid (64).
static dext: [u16; 32] = [
    16, 16, 16, 16, 17, 17, 18, 18, 19, 19, 20, 20, 21, 21, 22, 22, 23, 23, 24, 24, 25, 25, 26, 26,
    27, 27, 28, 28, 29, 29, 64, 64,
];

/// `inflate_table`: build the decoding tables of the canonical Huffman code whose code
/// lengths are `lens` (one per symbol, each in `0..=MAXBITS`, 0 for an unused symbol).
///
/// The tables are written to `table` starting at `*next`, which is advanced past them. `bits`
/// is the requested number of root table index bits; on return it holds the actual number,
/// which differs when the request is longer than the longest code or shorter than the
/// shortest one. `work` is scratch space of at least `lens.len()` entries.
///
/// Returns 0 on success, -1 for an over-subscribed or incomplete code, and +1 when
/// [`ENOUGH_LENS`]/[`ENOUGH_DISTS`] entries are not enough (which only a changed root size can
/// cause).
pub(crate) fn inflate_table(
    type_: CodeType,
    lens: &[u16],
    table: &mut [Code],
    next: &mut usize,
    bits: &mut u32,
    work: &mut [u16],
) -> i32 {
    let mut count = [0u16; MAXBITS + 1]; // number of codes of each length
    let mut offs = [0u16; MAXBITS + 1]; // offsets in table for each length

    // The code is canonical: sorted by length and, within a length, by symbol, the codes are
    // consecutive integers starting from all zeros, with zeros appended when the length grows.
    // Deflate stores the bits backwards, so the integer codes are incremented backwards below.
    // The lengths are counted per length, which gives the shortest and longest lengths, tells
    // whether there are codes at all, checks the set of lengths, and (as codes are entered and
    // the counts decremented) gives the size of each sub-table. The symbols are sorted by
    // length into work[] through the per-length offsets.

    // accumulate lengths for codes (lens[] is assumed to be all in 0..=MAXBITS)
    for &len in lens {
        count[len as usize] += 1;
    }

    // bound code lengths, force root to be within code lengths
    let mut root = *bits;
    let mut max = MAXBITS as u32;
    while max >= 1 {
        if count[max as usize] != 0 {
            break;
        }
        max -= 1;
    }
    if root > max {
        root = max;
    }
    if max == 0 {
        // no symbols to code at all: make a table that forces an error when decoding
        let here = Code {
            op: 64,
            bits: 1,
            val: 0,
        }; // invalid code marker
        table[*next] = here;
        table[*next + 1] = here;
        *next += 2;
        *bits = 1;
        return 0; // no symbols, but wait for decoding to report error
    }
    let mut min = 1u32;
    while min < max {
        if count[min as usize] != 0 {
            break;
        }
        min += 1;
    }
    if root < min {
        root = min;
    }

    // check for an over-subscribed or incomplete set of lengths
    let mut left: i32 = 1; // number of prefix codes available
    for &n in &count[1..] {
        left <<= 1;
        left -= i32::from(n);
        if left < 0 {
            return -1; // over-subscribed
        }
    }
    if left > 0 && (type_ == CodeType::CODES || max != 1) {
        return -1; // incomplete set
    }

    // generate offsets into symbol table for each length for sorting
    offs[1] = 0;
    for len in 1..MAXBITS {
        offs[len + 1] = offs[len] + count[len];
    }

    // sort symbols by length, by symbol order within each length
    for (sym, &len) in lens.iter().enumerate() {
        if len != 0 {
            work[offs[len as usize] as usize] = sym as u16;
            offs[len as usize] += 1;
        }
    }

    // The table being filled starts at `next_table` and has `curr` index bits. The code being
    // entered is `huff`, of length `len`, turned into an index by dropping `drop` low bits
    // (0 for the root table, `root` for sub-tables); a code shorter than `drop + curr` is
    // replicated over every value of the missing top bits. When `len` exceeds `root`, the low
    // `root` bits of `huff` select a root entry that links to a sub-table; `low` remembers the
    // entry being served, so a new sub-table starts when it changes, and its size comes from
    // looking ahead in count[]. `used` counts the entries allocated, checked against ENOUGH_*
    // for LENS and DISTS. Incomplete codes are allowed, so a last entry may be filled with an
    // invalid code marker after the loop.

    // set up for code type: symbols below `match_` are literals (or code lengths), symbols
    // from `match_` on are looked up in `base` and `extra`, `match_ - 1` is the end of block
    let (base, extra, match_): (&[u16], &[u16], u32) = match type_ {
        CodeType::CODES => (&[], &[], 20),
        CodeType::LENS => (&lbase, &lext, 257),
        CodeType::DISTS => (&dbase, &dext, 0),
    };

    // initialize state for loop
    let mut huff: u32 = 0; // starting code
    let mut sym: usize = 0; // starting code symbol
    let mut len = min; // starting code length
    let mut next_table = *next; // current table to fill in
    let mut curr = root; // current table index bits
    let mut drop: u32 = 0; // current bits to drop from code for index
    let mut low = u32::MAX; // trigger new sub-table when len > root
    let mut used: u32 = 1 << root; // use root table entries
    let mask = used - 1; // mask for comparing low

    // check available table space
    let too_many = |used: u32| match type_ {
        CodeType::LENS => used as usize > ENOUGH_LENS,
        CodeType::DISTS => used as usize > ENOUGH_DISTS,
        CodeType::CODES => false,
    };
    if too_many(used) {
        return 1;
    }

    // process all codes and make table entries
    loop {
        // create table entry
        let w = u32::from(work[sym]);
        let here = if w + 1 < match_ {
            Code {
                op: 0,
                bits: (len - drop) as u8,
                val: work[sym],
            }
        } else if w >= match_ {
            let i = (w - match_) as usize;
            Code {
                op: extra[i] as u8,
                bits: (len - drop) as u8,
                val: base[i],
            }
        } else {
            Code {
                op: 32 + 64,
                bits: (len - drop) as u8,
                val: 0,
            } // end of block
        };

        // replicate for those indices with low len bits equal to huff
        let incr = 1u32 << (len - drop);
        let mut fill = 1u32 << curr;
        min = fill; // save offset to next table
        loop {
            fill -= incr;
            table[next_table + ((huff >> drop) + fill) as usize] = here;
            if fill == 0 {
                break;
            }
        }

        // backwards increment the len-bit code huff
        let mut incr = 1u32 << (len - 1);
        while huff & incr != 0 {
            incr >>= 1;
        }
        if incr != 0 {
            huff &= incr - 1;
            huff += incr;
        } else {
            huff = 0;
        }

        // go to next symbol, update count, len
        sym += 1;
        count[len as usize] -= 1;
        if count[len as usize] == 0 {
            if len == max {
                break;
            }
            len = u32::from(lens[work[sym] as usize]);
        }

        // create new sub-table if needed
        if len > root && (huff & mask) != low {
            // if first time, transition to sub-tables
            if drop == 0 {
                drop = root;
            }

            // increment past last table
            next_table += min as usize; // here min is 1 << curr

            // determine length of next table
            curr = len - drop;
            let mut left: i32 = 1 << curr;
            while curr + drop < max {
                left -= i32::from(count[(curr + drop) as usize]);
                if left <= 0 {
                    break;
                }
                curr += 1;
                left <<= 1;
            }

            // check for enough space
            used += 1 << curr;
            if too_many(used) {
                return 1;
            }

            // point entry in root table to sub-table
            low = huff & mask;
            table[*next + low as usize] = Code {
                op: curr as u8,
                bits: root as u8,
                val: (next_table - *next) as u16,
            };
        }
    }

    // fill in remaining table entry if code is incomplete (guaranteed to have at most one
    // remaining entry, since if the code is incomplete, the maximum code length that was
    // allowed to get this far is one bit)
    if huff != 0 {
        table[next_table + huff as usize] = Code {
            op: 64,
            bits: (len - drop) as u8,
            val: 0,
        };
    }

    // set return parameters
    *next += used as usize;
    *bits = root;
    0
}

#[cfg(test)]
mod tests;
