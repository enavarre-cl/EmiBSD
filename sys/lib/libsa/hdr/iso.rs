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
/* </LICENSES> */

/* <CODE> */
//! `<isofs/cd9660/iso.h>` for libsa: the ISO 9660 volume descriptor and directory record
//! fields the standalone reader uses, and the `isonum_7xx` decoders.
//!
//! The C declares the on-disk structures as `char` arrays sized with `ISODCL(from, to)`
//! (1-based byte positions of ECMA-119); here each field is its byte range.

use core::ops::Range;

/// `ISO_DEFAULT_BLOCK_SHIFT`.
pub const ISO_DEFAULT_BLOCK_SHIFT: u32 = 11;
/// `ISO_DEFAULT_BLOCK_SIZE`.
pub const ISO_DEFAULT_BLOCK_SIZE: usize = 1 << ISO_DEFAULT_BLOCK_SHIFT;
/// `ISO_VD_PRIMARY`.
pub const ISO_VD_PRIMARY: u8 = 1;
/// `ISO_VD_END`.
pub const ISO_VD_END: u8 = 255;
/// `ISO_STANDARD_ID`.
pub const ISO_STANDARD_ID: &[u8; 5] = b"CD001";

/// `ISODCL(from, to)`: the byte range of a field, from ECMA-119's 1-based positions.
const fn isodcl(from: usize, to: usize) -> Range<usize> {
    from - 1..to
}

/// `struct iso_primary_descriptor`: the fields' byte ranges.
pub mod iso_primary_descriptor {
    use super::{Range, isodcl};
    /// `type` (711).
    pub const TYPE: Range<usize> = isodcl(1, 1);
    /// `id`.
    pub const ID: Range<usize> = isodcl(2, 6);
    /// `logical_block_size` (723).
    pub const LOGICAL_BLOCK_SIZE: Range<usize> = isodcl(129, 132);
    /// `path_table_size` (733).
    pub const PATH_TABLE_SIZE: Range<usize> = isodcl(133, 140);
    /// `type_m_path_table` (732).
    pub const TYPE_M_PATH_TABLE: Range<usize> = isodcl(149, 152);
}

/// `struct iso_directory_record`: the fields' byte ranges.
pub mod iso_directory_record {
    use super::{Range, isodcl};
    /// `length` (711).
    pub const LENGTH: Range<usize> = isodcl(1, 1);
    /// `extent` (733).
    pub const EXTENT: Range<usize> = isodcl(3, 10);
    /// `size` (733).
    pub const SIZE: Range<usize> = isodcl(11, 18);
    /// `flags`.
    pub const FLAGS: Range<usize> = isodcl(26, 26);
    /// `name_len` (711).
    pub const NAME_LEN: Range<usize> = isodcl(33, 33);
    /// `name`: the first byte of the name.
    pub const NAME: usize = 33;
}

/// `isonum_711`: an 8-bit unsigned number.
pub const fn isonum_711(p: &[u8]) -> u32 {
    p[0] as u32
}

/// `isonum_722`: a 16-bit big-endian number.
pub const fn isonum_722(p: &[u8]) -> u32 {
    ((p[0] as u32) << 8) | p[1] as u32
}

/// `isonum_723`: a 16-bit both-endian number (the little-endian half is read).
pub const fn isonum_723(p: &[u8]) -> u32 {
    p[0] as u32 | ((p[1] as u32) << 8)
}

/// `isonum_732`: a 32-bit big-endian number.
pub const fn isonum_732(p: &[u8]) -> u32 {
    ((p[0] as u32) << 24) | ((p[1] as u32) << 16) | ((p[2] as u32) << 8) | p[3] as u32
}

/// `isonum_733`: a 32-bit both-endian number (the little-endian half is read).
pub const fn isonum_733(p: &[u8]) -> u32 {
    p[0] as u32 | ((p[1] as u32) << 8) | ((p[2] as u32) << 16) | ((p[3] as u32) << 24)
}
/* </CODE> */
