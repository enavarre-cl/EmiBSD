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

/*-
 * Copyright (c) 1994
 *	The Regents of the University of California.  All rights reserved.
 *
 * This code is derived from software contributed to Berkeley
 * by Pace Willisson (pace@blitz.com).  The Rock Ridge Extension
 * Support code is derived from software contributed to Berkeley
 * by Atsushi Murai (amurai@spec.co.jp).
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
 *	@(#)iso.h	8.4 (Berkeley) 12/5/94
 */
/* </LICENSES> */

/* <CODE> */
//! `<isofs/cd9660/iso.h>`: the structures of the ISO 9660 file system as they are on the disc
//! (volume descriptors, directory records, extended attribute records) and the functions
//! that read the numbers stored in them.
//!
//! Upstream: sys/isofs/cd9660/iso.h @ 3ce1f3f79392
//!
//! The C declares every field as a byte array, `char x[ISODCL(from, to)]` with the 1-based
//! byte positions of the standard, and reads the numbers with `isonum_7xx` (the section of
//! ECMA-119 that defines each encoding). The fixed-size structures are `#[repr(C)]` structures
//! of byte arrays here too (alignment 1, any bit pattern valid), viewed in place over a buffer
//! with `from_bytes`.
//!
//! ## Deviations
//! - `isonum_7xx` take a byte slice and return the field's own type (`u8`, `i8`, `u16`,
//!   `u32`) instead of `int`; a caller converts where the C's `int` arithmetic matters.
//!   `isonum_722`, which the kernel does not use, reads an unsigned big-endian value; the
//!   C's `(char)*p << 8` sign-extends where `char` is signed (amd64) and not on arm64.
//! - `struct iso_directory_record` has a variable-length tail (the name, then the system use
//!   area), so a pointer to one is [`IsoDirectoryRecord`], a view over the bytes from the
//!   record to the end of its buffer, with one accessor per field. A view is never shorter
//!   than `ISO_DIRECTORY_RECORD_SIZE`, so the fixed fields are always there.
//! - The C's `type` member is `type_` (a Rust keyword).

/// `cdino_t`: the inode number of an ISO 9660 file, the byte offset of its directory record
/// (or, for a directory, of its first data block) on the disc.
pub type Cdino = u32;

/// `ISODCL(from, to)`: the size of the field at the 1-based byte positions `from..=to`.
pub const fn isodcl(from: usize, to: usize) -> usize {
    to - from + 1
}

/// Implements `from_bytes` for a `#[repr(C)]` structure made only of byte arrays.
macro_rules! from_bytes_impl {
    ($t:ident) => {
        impl $t {
            /// The structure at the start of `b`, `None` when `b` is shorter than it.
            pub fn from_bytes(b: &[u8]) -> Option<&$t> {
                if b.len() < size_of::<$t>() {
                    return None;
                }
                // SAFETY: the structure is byte arrays only (size checked above, alignment 1,
                // every bit pattern valid), and the reference borrows `b`.
                Some(unsafe { &*b.as_ptr().cast::<$t>() })
            }
        }
    };
}

pub(crate) use from_bytes_impl;

/// `struct iso_volume_descriptor`.
#[repr(C)]
pub struct IsoVolumeDescriptor {
    /// `type`: 711.
    pub type_: [u8; isodcl(1, 1)],
    /// `id`.
    pub id: [u8; isodcl(2, 6)],
    /// `version`.
    pub version: [u8; isodcl(7, 7)],
    /// `data`.
    pub data: [u8; isodcl(8, 2048)],
}

from_bytes_impl!(IsoVolumeDescriptor);

/// `ISO_VD_PRIMARY`: volume descriptor type.
pub const ISO_VD_PRIMARY: u8 = 1;
/// `ISO_VD_SUPPLEMENTARY`.
pub const ISO_VD_SUPPLEMENTARY: u8 = 2;
/// `ISO_VD_END`.
pub const ISO_VD_END: u8 = 255;

/// `ISO_STANDARD_ID`.
pub const ISO_STANDARD_ID: &[u8; 5] = b"CD001";
/// `ISO_ECMA_ID`.
pub const ISO_ECMA_ID: &[u8; 5] = b"CDW01";

/// `struct iso_primary_descriptor`.
#[repr(C)]
pub struct IsoPrimaryDescriptor {
    /// `type`: 711.
    pub type_: [u8; isodcl(1, 1)],
    /// `id`.
    pub id: [u8; isodcl(2, 6)],
    /// `version`: 711.
    pub version: [u8; isodcl(7, 7)],
    /// `unused1`.
    pub unused1: [u8; isodcl(8, 8)],
    /// `system_id`: achars.
    pub system_id: [u8; isodcl(9, 40)],
    /// `volume_id`: dchars.
    pub volume_id: [u8; isodcl(41, 72)],
    /// `unused2`.
    pub unused2: [u8; isodcl(73, 80)],
    /// `volume_space_size`: 733.
    pub volume_space_size: [u8; isodcl(81, 88)],
    /// `unused3`.
    pub unused3: [u8; isodcl(89, 120)],
    /// `volume_set_size`: 723.
    pub volume_set_size: [u8; isodcl(121, 124)],
    /// `volume_sequence_number`: 723.
    pub volume_sequence_number: [u8; isodcl(125, 128)],
    /// `logical_block_size`: 723.
    pub logical_block_size: [u8; isodcl(129, 132)],
    /// `path_table_size`: 733.
    pub path_table_size: [u8; isodcl(133, 140)],
    /// `type_l_path_table`: 731.
    pub type_l_path_table: [u8; isodcl(141, 144)],
    /// `opt_type_l_path_table`: 731.
    pub opt_type_l_path_table: [u8; isodcl(145, 148)],
    /// `type_m_path_table`: 732.
    pub type_m_path_table: [u8; isodcl(149, 152)],
    /// `opt_type_m_path_table`: 732.
    pub opt_type_m_path_table: [u8; isodcl(153, 156)],
    /// `root_directory_record`: 9.1.
    pub root_directory_record: [u8; isodcl(157, 190)],
    /// `volume_set_id`: dchars.
    pub volume_set_id: [u8; isodcl(191, 318)],
    /// `publisher_id`: achars.
    pub publisher_id: [u8; isodcl(319, 446)],
    /// `preparer_id`: achars.
    pub preparer_id: [u8; isodcl(447, 574)],
    /// `application_id`: achars.
    pub application_id: [u8; isodcl(575, 702)],
    /// `copyright_file_id`: 7.5 dchars.
    pub copyright_file_id: [u8; isodcl(703, 739)],
    /// `abstract_file_id`: 7.5 dchars.
    pub abstract_file_id: [u8; isodcl(740, 776)],
    /// `bibliographic_file_id`: 7.5 dchars.
    pub bibliographic_file_id: [u8; isodcl(777, 813)],
    /// `creation_date`: 8.4.26.1.
    pub creation_date: [u8; isodcl(814, 830)],
    /// `modification_date`: 8.4.26.1.
    pub modification_date: [u8; isodcl(831, 847)],
    /// `expiration_date`: 8.4.26.1.
    pub expiration_date: [u8; isodcl(848, 864)],
    /// `effective_date`: 8.4.26.1.
    pub effective_date: [u8; isodcl(865, 881)],
    /// `file_structure_version`: 711.
    pub file_structure_version: [u8; isodcl(882, 882)],
    /// `unused4`.
    pub unused4: [u8; isodcl(883, 883)],
    /// `application_data`.
    pub application_data: [u8; isodcl(884, 1395)],
    /// `unused5`.
    pub unused5: [u8; isodcl(1396, 2048)],
}

from_bytes_impl!(IsoPrimaryDescriptor);

/// `ISO_DEFAULT_BLOCK_SHIFT`.
pub const ISO_DEFAULT_BLOCK_SHIFT: usize = 11;
/// `ISO_DEFAULT_BLOCK_SIZE`.
pub const ISO_DEFAULT_BLOCK_SIZE: usize = 1 << ISO_DEFAULT_BLOCK_SHIFT;

/// `struct iso_supplementary_descriptor`: used by Microsoft Joliet extension to ISO9660.
/// Almost the same as PVD, but byte position 8 is a flag, and 89-120 is for escape.
#[repr(C)]
pub struct IsoSupplementaryDescriptor {
    /// `type`: 711.
    pub type_: [u8; isodcl(1, 1)],
    /// `id`.
    pub id: [u8; isodcl(2, 6)],
    /// `version`: 711.
    pub version: [u8; isodcl(7, 7)],
    /// `flags`.
    pub flags: [u8; isodcl(8, 8)],
    /// `system_id`: achars.
    pub system_id: [u8; isodcl(9, 40)],
    /// `volume_id`: dchars.
    pub volume_id: [u8; isodcl(41, 72)],
    /// `unused2`.
    pub unused2: [u8; isodcl(73, 80)],
    /// `volume_space_size`: 733.
    pub volume_space_size: [u8; isodcl(81, 88)],
    /// `escape`.
    pub escape: [u8; isodcl(89, 120)],
    /// `volume_set_size`: 723.
    pub volume_set_size: [u8; isodcl(121, 124)],
    /// `volume_sequence_number`: 723.
    pub volume_sequence_number: [u8; isodcl(125, 128)],
    /// `logical_block_size`: 723.
    pub logical_block_size: [u8; isodcl(129, 132)],
    /// `path_table_size`: 733.
    pub path_table_size: [u8; isodcl(133, 140)],
    /// `type_l_path_table`: 731.
    pub type_l_path_table: [u8; isodcl(141, 144)],
    /// `opt_type_l_path_table`: 731.
    pub opt_type_l_path_table: [u8; isodcl(145, 148)],
    /// `type_m_path_table`: 732.
    pub type_m_path_table: [u8; isodcl(149, 152)],
    /// `opt_type_m_path_table`: 732.
    pub opt_type_m_path_table: [u8; isodcl(153, 156)],
    /// `root_directory_record`: 9.1.
    pub root_directory_record: [u8; isodcl(157, 190)],
    /// `volume_set_id`: dchars.
    pub volume_set_id: [u8; isodcl(191, 318)],
    /// `publisher_id`: achars.
    pub publisher_id: [u8; isodcl(319, 446)],
    /// `preparer_id`: achars.
    pub preparer_id: [u8; isodcl(447, 574)],
    /// `application_id`: achars.
    pub application_id: [u8; isodcl(575, 702)],
    /// `copyright_file_id`: 7.5 dchars.
    pub copyright_file_id: [u8; isodcl(703, 739)],
    /// `abstract_file_id`: 7.5 dchars.
    pub abstract_file_id: [u8; isodcl(740, 776)],
    /// `bibliographic_file_id`: 7.5 dchars.
    pub bibliographic_file_id: [u8; isodcl(777, 813)],
    /// `creation_date`: 8.4.26.1.
    pub creation_date: [u8; isodcl(814, 830)],
    /// `modification_date`: 8.4.26.1.
    pub modification_date: [u8; isodcl(831, 847)],
    /// `expiration_date`: 8.4.26.1.
    pub expiration_date: [u8; isodcl(848, 864)],
    /// `effective_date`: 8.4.26.1.
    pub effective_date: [u8; isodcl(865, 881)],
    /// `file_structure_version`: 711.
    pub file_structure_version: [u8; isodcl(882, 882)],
    /// `unused4`.
    pub unused4: [u8; isodcl(883, 883)],
    /// `application_data`.
    pub application_data: [u8; isodcl(884, 1395)],
    /// `unused5`.
    pub unused5: [u8; isodcl(1396, 2048)],
}

from_bytes_impl!(IsoSupplementaryDescriptor);

/// `ISO_DIRECTORY_RECORD_SIZE`: the fixed part of a directory record. Can't take
/// `sizeof(iso_directory_record)`, because of possible alignment of the last entry (34
/// instead of 33).
pub const ISO_DIRECTORY_RECORD_SIZE: usize = 33;

/// `struct iso_directory_record *`: a directory record in a buffer, viewed from its first
/// byte to the end of the buffer (see the module's deviations).
#[derive(Clone, Copy)]
pub struct IsoDirectoryRecord<'a> {
    /// The record's bytes and whatever follows it in the buffer.
    b: &'a [u8],
}

impl<'a> IsoDirectoryRecord<'a> {
    /// The record at the start of `b`, `None` when `b` cannot hold its fixed part.
    pub fn new(b: &'a [u8]) -> Option<Self> {
        if b.len() < ISO_DIRECTORY_RECORD_SIZE {
            return None;
        }
        Some(Self { b })
    }

    /// The bytes from the record to the end of the buffer.
    pub fn bytes(&self) -> &'a [u8] {
        self.b
    }

    /// `length`: 711.
    pub fn length(&self) -> &'a [u8] {
        &self.b[0..1]
    }

    /// `ext_attr_length`: 711.
    pub fn ext_attr_length(&self) -> &'a [u8] {
        &self.b[1..2]
    }

    /// `extent`: 733.
    pub fn extent(&self) -> &'a [u8] {
        &self.b[2..10]
    }

    /// `size`: 733.
    pub fn size(&self) -> &'a [u8] {
        &self.b[10..18]
    }

    /// `date`: 7 by 711.
    pub fn date(&self) -> &'a [u8] {
        &self.b[18..25]
    }

    /// `flags`.
    pub fn flags(&self) -> &'a [u8] {
        &self.b[25..26]
    }

    /// `file_unit_size`: 711.
    pub fn file_unit_size(&self) -> &'a [u8] {
        &self.b[26..27]
    }

    /// `interleave`: 711.
    pub fn interleave(&self) -> &'a [u8] {
        &self.b[27..28]
    }

    /// `volume_sequence_number`: 723.
    pub fn volume_sequence_number(&self) -> &'a [u8] {
        &self.b[28..32]
    }

    /// `name_len`: 711.
    pub fn name_len(&self) -> &'a [u8] {
        &self.b[32..33]
    }

    /// `name`: the bytes from the name on (the C's `char *`; `name_len` says how many are
    /// the name).
    pub fn name(&self) -> &'a [u8] {
        &self.b[ISO_DIRECTORY_RECORD_SIZE..]
    }

    /// `name[0]`, zero when the buffer ends before it.
    pub fn name0(&self) -> u8 {
        self.name().first().copied().unwrap_or(0)
    }
}

/// `struct iso_extended_attributes`.
#[repr(C)]
pub struct IsoExtendedAttributes {
    /// `owner`: 723.
    pub owner: [u8; isodcl(1, 4)],
    /// `group`: 723.
    pub group: [u8; isodcl(5, 8)],
    /// `perm`: 9.5.3.
    pub perm: [u8; isodcl(9, 10)],
    /// `ctime`: 8.4.26.1.
    pub ctime: [u8; isodcl(11, 27)],
    /// `mtime`: 8.4.26.1.
    pub mtime: [u8; isodcl(28, 44)],
    /// `xtime`: 8.4.26.1.
    pub xtime: [u8; isodcl(45, 61)],
    /// `ftime`: 8.4.26.1.
    pub ftime: [u8; isodcl(62, 78)],
    /// `recfmt`: 711.
    pub recfmt: [u8; isodcl(79, 79)],
    /// `recattr`: 711.
    pub recattr: [u8; isodcl(80, 80)],
    /// `reclen`: 723.
    pub reclen: [u8; isodcl(81, 84)],
    /// `system_id`: achars.
    pub system_id: [u8; isodcl(85, 116)],
    /// `system_use`.
    pub system_use: [u8; isodcl(117, 180)],
    /// `version`: 711.
    pub version: [u8; isodcl(181, 181)],
    /// `len_esc`: 711.
    pub len_esc: [u8; isodcl(182, 182)],
    /// `reserved`.
    pub reserved: [u8; isodcl(183, 246)],
    /// `len_au`: 723.
    pub len_au: [u8; isodcl(247, 250)],
}

from_bytes_impl!(IsoExtendedAttributes);

/// `ASSOCCHAR`: associated files have a leading '='.
pub const ASSOCCHAR: u8 = b'=';

/// `isonum_711`: 7.1.1, unsigned char.
pub fn isonum_711(p: &[u8]) -> u8 {
    p[0]
}

/// `isonum_712`: 7.1.2, signed(?) char.
pub fn isonum_712(p: &[u8]) -> i8 {
    p[0] as i8
}

/// `isonum_721`: 7.2.1, unsigned little-endian 16-bit value. NOT USED IN KERNEL.
pub fn isonum_721(p: &[u8]) -> u16 {
    u16::from_le_bytes([p[0], p[1]])
}

/// `isonum_722`: 7.2.2, unsigned big-endian 16-bit value. NOT USED IN KERNEL.
pub fn isonum_722(p: &[u8]) -> u16 {
    u16::from_be_bytes([p[0], p[1]])
}

/// `isonum_723`: 7.2.3, unsigned both-endian (little, then big) 16-bit value; the
/// little-endian half is read.
pub fn isonum_723(p: &[u8]) -> u16 {
    u16::from_le_bytes([p[0], p[1]])
}

/// `isonum_731`: 7.3.1, unsigned little-endian 32-bit value. NOT USED IN KERNEL.
pub fn isonum_731(p: &[u8]) -> u32 {
    u32::from_le_bytes([p[0], p[1], p[2], p[3]])
}

/// `isonum_732`: 7.3.2, unsigned big-endian 32-bit value. NOT USED IN KERNEL.
pub fn isonum_732(p: &[u8]) -> u32 {
    u32::from_be_bytes([p[0], p[1], p[2], p[3]])
}

/// `isonum_733`: 7.3.3, unsigned both-endian (little, then big) 32-bit value; the
/// little-endian half is read.
pub fn isonum_733(p: &[u8]) -> u32 {
    u32::from_le_bytes([p[0], p[1], p[2], p[3]])
}

const _: () = {
    assert!(size_of::<IsoVolumeDescriptor>() == 2048);
    assert!(size_of::<IsoPrimaryDescriptor>() == 2048);
    assert!(size_of::<IsoSupplementaryDescriptor>() == 2048);
    assert!(size_of::<IsoExtendedAttributes>() == 250);
    assert!(core::mem::offset_of!(IsoPrimaryDescriptor, root_directory_record) == 156);
    assert!(core::mem::offset_of!(IsoSupplementaryDescriptor, escape) == 88);
};
/* </CODE> */

/* <TESTS> */
#[cfg(test)]
pub(crate) mod tests {
    // Host tests for the on-disc structures and their number encodings, and the fixtures the
    // other cd9660 tests share: directory records out of an image made by OpenBSD's makefs
    // (`makefs -t cd9660 -o rockridge,label=M10C` of a tree with `m10c-iso.txt`, holding
    // "m10c-iso-42\n", and `sub/` with a symbolic link `link -> ../m10c-iso.txt` and
    // `Mixed_Case-name.long.txt`), and a mount to analyse them against.

    use std::boxed::Box;

    use super::*;
    use crate::isofs::cd9660::cd9660_extern::{ISO_FTYPE_RRIP, IsoFtype, IsoMnt};
    use crate::isofs::cd9660::cd9660_node::IsoNode;
    use crate::sys::mount::Mount;
    use crate::sys::vnode::{VT_ISOFS, Vnode};

    /// Records of the test image.
    pub(crate) mod image {
        /// The root directory's `.` record (SP, ER and a CE to block 24).
        pub(crate) const ROOT_DOT: [u8; 254] = [
            0xfe, 0x00, 0x14, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x14, 0x00, 0x08, 0x00, 0x00,
            0x00, 0x00, 0x08, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x02, 0x00, 0x00,
            0x01, 0x00, 0x00, 0x01, 0x01, 0x00, 0x53, 0x50, 0x07, 0x01, 0xbe, 0xef, 0x00, 0x45,
            0x52, 0xb9, 0x01, 0x0a, 0x49, 0x5e, 0x01, 0x49, 0x45, 0x45, 0x45, 0x5f, 0x50, 0x31,
            0x32, 0x38, 0x32, 0x54, 0x48, 0x45, 0x20, 0x49, 0x45, 0x45, 0x45, 0x20, 0x50, 0x31,
            0x32, 0x38, 0x32, 0x20, 0x50, 0x52, 0x4f, 0x54, 0x4f, 0x43, 0x4f, 0x4c, 0x20, 0x50,
            0x52, 0x4f, 0x56, 0x49, 0x44, 0x45, 0x53, 0x20, 0x53, 0x55, 0x50, 0x50, 0x4f, 0x52,
            0x54, 0x20, 0x46, 0x4f, 0x52, 0x20, 0x50, 0x4f, 0x53, 0x49, 0x58, 0x20, 0x46, 0x49,
            0x4c, 0x45, 0x20, 0x53, 0x59, 0x53, 0x54, 0x45, 0x4d, 0x20, 0x53, 0x45, 0x4d, 0x41,
            0x4e, 0x54, 0x49, 0x43, 0x53, 0x2e, 0x50, 0x4c, 0x45, 0x41, 0x53, 0x45, 0x20, 0x43,
            0x4f, 0x4e, 0x54, 0x41, 0x43, 0x54, 0x20, 0x54, 0x48, 0x45, 0x20, 0x49, 0x45, 0x45,
            0x45, 0x20, 0x53, 0x54, 0x41, 0x4e, 0x44, 0x41, 0x52, 0x44, 0x53, 0x20, 0x44, 0x45,
            0x50, 0x41, 0x52, 0x54, 0x4d, 0x45, 0x4e, 0x54, 0x2c, 0x20, 0x50, 0x49, 0x53, 0x43,
            0x41, 0x54, 0x41, 0x57, 0x41, 0x59, 0x2c, 0x20, 0x4e, 0x4a, 0x2c, 0x20, 0x55, 0x53,
            0x41, 0x20, 0x46, 0x4f, 0x52, 0x20, 0x54, 0x48, 0x45, 0x20, 0x50, 0x31, 0x32, 0x38,
            0x32, 0x20, 0x53, 0x50, 0x45, 0x43, 0x49, 0x46, 0x49, 0x43, 0x41, 0x54, 0x49, 0x4f,
            0x4e, 0x2e, 0x43, 0x45, 0x1c, 0x01, 0x18, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x18,
            0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x24, 0x00, 0x00, 0x00, 0x00, 0x00,
            0x00, 0x24,
        ];

        /// The root directory's `..` record (PX).
        pub(crate) const ROOT_DOTDOT: [u8; 70] = [
            0x46, 0x00, 0x14, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x14, 0x00, 0x08, 0x00, 0x00,
            0x00, 0x00, 0x08, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x02, 0x00, 0x00,
            0x01, 0x00, 0x00, 0x01, 0x01, 0x01, 0x50, 0x58, 0x24, 0x01, 0xed, 0x41, 0x00, 0x00,
            0x00, 0x00, 0x41, 0xed, 0x04, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x04, 0x00, 0x00,
            0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00,
        ];

        /// `M10C_ISO.TXT;1`: PX, TF, NM "m10c-iso.txt".
        pub(crate) const FILE_REC: [u8; 128] = [
            0x80, 0x00, 0x17, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x17, 0x0c, 0x00, 0x00, 0x00,
            0x00, 0x00, 0x00, 0x0c, 0x7e, 0x0a, 0x04, 0x07, 0x24, 0x30, 0xf4, 0x00, 0x00, 0x00,
            0x01, 0x00, 0x00, 0x01, 0x0e, 0x4d, 0x31, 0x30, 0x43, 0x5f, 0x49, 0x53, 0x4f, 0x2e,
            0x54, 0x58, 0x54, 0x3b, 0x31, 0x00, 0x50, 0x58, 0x24, 0x01, 0xa4, 0x81, 0x00, 0x00,
            0x00, 0x00, 0x81, 0xa4, 0x01, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x01, 0x00, 0x00,
            0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00,
            0x54, 0x46, 0x1a, 0x01, 0x07, 0x7e, 0x0a, 0x04, 0x07, 0x24, 0x30, 0xf4, 0x7e, 0x0a,
            0x04, 0x07, 0x24, 0x30, 0xf4, 0x7e, 0x0a, 0x04, 0x07, 0x24, 0x30, 0xf4, 0x4e, 0x4d,
            0x11, 0x01, 0x00, 0x6d, 0x31, 0x30, 0x63, 0x2d, 0x69, 0x73, 0x6f, 0x2e, 0x74, 0x78,
            0x74, 0x00,
        ];

        /// `SUB`: a directory with PX, TF, NM "sub".
        pub(crate) const SUB_REC: [u8; 106] = [
            0x6a, 0x00, 0x15, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x15, 0x00, 0x08, 0x00, 0x00,
            0x00, 0x00, 0x08, 0x00, 0x7e, 0x0a, 0x04, 0x07, 0x24, 0x30, 0xf4, 0x02, 0x00, 0x00,
            0x01, 0x00, 0x00, 0x01, 0x03, 0x53, 0x55, 0x42, 0x50, 0x58, 0x24, 0x01, 0xed, 0x41,
            0x00, 0x00, 0x00, 0x00, 0x41, 0xed, 0x04, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x04,
            0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00,
            0x00, 0x00, 0x54, 0x46, 0x1a, 0x01, 0x07, 0x7e, 0x0a, 0x04, 0x07, 0x24, 0x30, 0xf4,
            0x7e, 0x0a, 0x04, 0x07, 0x24, 0x30, 0xf4, 0x7e, 0x0a, 0x04, 0x07, 0x24, 0x30, 0xf4,
            0x4e, 0x4d, 0x08, 0x01, 0x00, 0x73, 0x75, 0x62,
        ];

        /// `/SUB/LINK.;1`: PX, TF, SL "../m10c-iso.txt", NM "link".
        pub(crate) const LINK_REC: [u8; 132] = [
            0x84, 0x00, 0x16, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x16, 0x0f, 0x00, 0x00, 0x00,
            0x00, 0x00, 0x00, 0x0f, 0x7e, 0x0a, 0x04, 0x07, 0x24, 0x30, 0xf4, 0x00, 0x00, 0x00,
            0x01, 0x00, 0x00, 0x01, 0x07, 0x4c, 0x49, 0x4e, 0x4b, 0x2e, 0x3b, 0x31, 0x50, 0x58,
            0x24, 0x01, 0xed, 0xa1, 0x00, 0x00, 0x00, 0x00, 0xa1, 0xed, 0x01, 0x00, 0x00, 0x00,
            0x00, 0x00, 0x00, 0x01, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00,
            0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x54, 0x46, 0x1a, 0x01, 0x07, 0x7e, 0x0a, 0x04,
            0x07, 0x24, 0x30, 0xf4, 0x7e, 0x0a, 0x04, 0x07, 0x24, 0x30, 0xf4, 0x7e, 0x0a, 0x04,
            0x07, 0x24, 0x30, 0xf4, 0x53, 0x4c, 0x15, 0x01, 0x00, 0x04, 0x00, 0x00, 0x0c, 0x6d,
            0x31, 0x30, 0x63, 0x2d, 0x69, 0x73, 0x6f, 0x2e, 0x74, 0x78, 0x74, 0x4e, 0x4d, 0x09,
            0x01, 0x00, 0x6c, 0x69, 0x6e, 0x6b,
        ];

        /// `/SUB/MIXED_CASE_NAME.LONG_TXT;1`: NM "Mixed_Case-name.long.txt".
        pub(crate) const MIXED_REC: [u8; 152] = [
            0x98, 0x00, 0x16, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x16, 0x06, 0x00, 0x00, 0x00,
            0x00, 0x00, 0x00, 0x06, 0x7e, 0x0a, 0x04, 0x07, 0x24, 0x30, 0xf4, 0x00, 0x00, 0x00,
            0x01, 0x00, 0x00, 0x01, 0x1a, 0x4d, 0x49, 0x58, 0x45, 0x44, 0x5f, 0x43, 0x41, 0x53,
            0x45, 0x5f, 0x4e, 0x41, 0x4d, 0x45, 0x2e, 0x4c, 0x4f, 0x4e, 0x47, 0x5f, 0x54, 0x58,
            0x54, 0x3b, 0x31, 0x00, 0x50, 0x58, 0x24, 0x01, 0xa4, 0x81, 0x00, 0x00, 0x00, 0x00,
            0x81, 0xa4, 0x01, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x01, 0x00, 0x00, 0x00, 0x00,
            0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x54, 0x46,
            0x1a, 0x01, 0x07, 0x7e, 0x0a, 0x04, 0x07, 0x24, 0x30, 0xf4, 0x7e, 0x0a, 0x04, 0x07,
            0x24, 0x30, 0xf4, 0x7e, 0x0a, 0x04, 0x07, 0x24, 0x30, 0xf4, 0x4e, 0x4d, 0x1d, 0x01,
            0x00, 0x4d, 0x69, 0x78, 0x65, 0x64, 0x5f, 0x43, 0x61, 0x73, 0x65, 0x2d, 0x6e, 0x61,
            0x6d, 0x65, 0x2e, 0x6c, 0x6f, 0x6e, 0x67, 0x2e, 0x74, 0x78, 0x74, 0x00,
        ];

        /// The root `.` record's continuation area (block 24, 36 bytes: PX).
        pub(crate) const ROOT_CE: [u8; 36] = [
            0x50, 0x58, 0x24, 0x01, 0xed, 0x41, 0x00, 0x00, 0x00, 0x00, 0x41, 0xed, 0x04, 0x00,
            0x00, 0x00, 0x00, 0x00, 0x00, 0x04, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00,
            0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00,
        ];

        /// The first 190 bytes of the primary volume descriptor.
        pub(crate) const PVD_HEAD: [u8; 190] = [
            0x01, 0x43, 0x44, 0x30, 0x30, 0x31, 0x01, 0x00, 0x4f, 0x70, 0x65, 0x6e, 0x42, 0x53,
            0x44, 0x20, 0x20, 0x20, 0x20, 0x20, 0x20, 0x20, 0x20, 0x20, 0x20, 0x20, 0x20, 0x20,
            0x20, 0x20, 0x20, 0x20, 0x20, 0x20, 0x20, 0x20, 0x20, 0x20, 0x20, 0x20, 0x4d, 0x31,
            0x30, 0x43, 0x20, 0x20, 0x20, 0x20, 0x20, 0x20, 0x20, 0x20, 0x20, 0x20, 0x20, 0x20,
            0x20, 0x20, 0x20, 0x20, 0x20, 0x20, 0x20, 0x20, 0x20, 0x20, 0x20, 0x20, 0x20, 0x20,
            0x20, 0x20, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x19, 0x00, 0x00, 0x00,
            0x00, 0x00, 0x00, 0x19, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00,
            0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00,
            0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x01, 0x00, 0x00, 0x01, 0x01, 0x00,
            0x00, 0x01, 0x00, 0x08, 0x08, 0x00, 0x16, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x16,
            0x12, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x13, 0x00, 0x00,
            0x00, 0x00, 0x22, 0x00, 0x14, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x14, 0x00, 0x08,
            0x00, 0x00, 0x00, 0x00, 0x08, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x02,
            0x00, 0x00, 0x01, 0x00, 0x00, 0x01, 0x01, 0x00,
        ];

        /// The nonzero bytes of the image's first 25 blocks (its volume space), as runs at
        /// their byte offsets.
        pub(crate) const IMAGE_RUNS: &[(usize, &[u8])] = &[
            (
                32768,
                b"\x01CD001\x01\x00OpenBSD                         M10C                       \
              \x20    ",
            ),
            (32848, b"\x19\x00\x00\x00\x00\x00\x00\x19"),
            (
                32888,
                b"\x01\x00\x00\x01\x01\x00\x00\x01\x00\x08\x08\x00\x16\x00\x00\x00\x00\x00\x00\
              \x16\x12",
            ),
            (
                32919,
                b"\x13\x00\x00\x00\x00\x22\x00\x14\x00\x00\x00\x00\x00\x00\x14\x00\x08\x00\x00\
              \x00\x00\x08",
            ),
            (
                32949,
                b"\x02\x00\x00\x01\x00\x00\x01\x01\x00                                        \
              \x20                                                                        \
              \x20                                                                        \
              \x20                                                                        \
              \x20                                                                        \
              \x20                                                                        \
              \x20                                                                        \
              \x20                                                                        \
              \x20                                                                       2\
              026100407365200\xf42026100407365200\xf40000000000000000\x002026100407365200\
              \xf4\x01",
            ),
            (34816, b"\xffCD001\x01"),
            (
                36864,
                b"\x01\x00\x14\x00\x00\x00\x01\x00\x00\x00\x03\x00\x15\x00\x00\x00\x01\x00SUB",
            ),
            (
                38912,
                b"\x01\x00\x00\x00\x00\x14\x00\x01\x00\x00\x03\x00\x00\x00\x00\x15\x00\x01SUB",
            ),
            (
                40960,
                b"\xfe\x00\x14\x00\x00\x00\x00\x00\x00\x14\x00\x08\x00\x00\x00\x00\x08",
            ),
            (
                40985,
                b"\x02\x00\x00\x01\x00\x00\x01\x01\x00SP\x07\x01\xbe\xef\x00ER\xb9\x01\x0aI^\
              \x01IEEE_P1282THE IEEE P1282 PROTOCOL PROVIDES SUPPORT FOR POSIX FILE SYSTEM\
              \x20SEMANTICS.PLEASE CONTACT THE IEEE STANDARDS DEPARTMENT, PISCATAWAY, NJ, \
              USA FOR THE P1282 SPECIFICATION.CE\x1c\x01\x18\x00\x00\x00\x00\x00\x00\x18",
            ),
            (
                41206,
                b"$\x00\x00\x00\x00\x00\x00$F\x00\x14\x00\x00\x00\x00\x00\x00\x14\x00\x08\x00\
              \x00\x00\x00\x08",
            ),
            (
                41239,
                b"\x02\x00\x00\x01\x00\x00\x01\x01\x01PX$\x01\xedA\x00\x00\x00\x00A\xed\x04\
              \x00\x00\x00\x00\x00\x00\x04",
            ),
            (
                41284,
                b"\x80\x00\x17\x00\x00\x00\x00\x00\x00\x17\x0c\x00\x00\x00\x00\x00\x00\x0c~\
              \x0a\x04\x07$0\xf4\x00\x00\x00\x01\x00\x00\x01\x0eM10C_ISO.TXT;1\x00PX$\x01\
              \xa4\x81\x00\x00\x00\x00\x81\xa4\x01\x00\x00\x00\x00\x00\x00\x01",
            ),
            (
                41368,
                b"TF\x1a\x01\x07~\x0a\x04\x07$0\xf4~\x0a\x04\x07$0\xf4~\x0a\x04\x07$0\xf4NM\
              \x11\x01\x00m10c-iso.txt\x00j\x00\x15\x00\x00\x00\x00\x00\x00\x15\x00\x08\
              \x00\x00\x00\x00\x08\x00~\x0a\x04\x07$0\xf4\x02\x00\x00\x01\x00\x00\x01\x03S\
              UBPX$\x01\xedA\x00\x00\x00\x00A\xed\x04\x00\x00\x00\x00\x00\x00\x04",
            ),
            (
                41484,
                b"TF\x1a\x01\x07~\x0a\x04\x07$0\xf4~\x0a\x04\x07$0\xf4~\x0a\x04\x07$0\xf4NM\
              \x08\x01\x00sub",
            ),
            (
                43008,
                b"F\x00\x15\x00\x00\x00\x00\x00\x00\x15\x00\x08\x00\x00\x00\x00\x08\x00~\x0a\
              \x04\x07$0\xf4\x02\x00\x00\x01\x00\x00\x01\x01\x00PX$\x01\xedA\x00\x00\x00\
              \x00A\xed\x04\x00\x00\x00\x00\x00\x00\x04",
            ),
            (
                43078,
                b"F\x00\x14\x00\x00\x00\x00\x00\x00\x14\x00\x08\x00\x00\x00\x00\x08",
            ),
            (
                43103,
                b"\x02\x00\x00\x01\x00\x00\x01\x01\x01PX$\x01\xedA\x00\x00\x00\x00A\xed\x04\
              \x00\x00\x00\x00\x00\x00\x04",
            ),
            (
                43148,
                b"\x84\x00\x16\x00\x00\x00\x00\x00\x00\x16\x0f\x00\x00\x00\x00\x00\x00\x0f~\
              \x0a\x04\x07$0\xf4\x00\x00\x00\x01\x00\x00\x01\x07LINK.;1PX$\x01\xed\xa1\x00\
              \x00\x00\x00\xa1\xed\x01\x00\x00\x00\x00\x00\x00\x01",
            ),
            (
                43224,
                b"TF\x1a\x01\x07~\x0a\x04\x07$0\xf4~\x0a\x04\x07$0\xf4~\x0a\x04\x07$0\xf4SL\
              \x15\x01\x00\x04\x00\x00\x0cm10c-iso.txtNM\x09\x01\x00link\x98\x00\x16\x00\
              \x00\x00\x00\x00\x00\x16\x06\x00\x00\x00\x00\x00\x00\x06~\x0a\x04\x07$0\xf4\
              \x00\x00\x00\x01\x00\x00\x01\x1aMIXED_CASE_NAME.LONG_TXT;1\x00PX$\x01\xa4\
              \x81\x00\x00\x00\x00\x81\xa4\x01\x00\x00\x00\x00\x00\x00\x01",
            ),
            (
                43376,
                b"TF\x1a\x01\x07~\x0a\x04\x07$0\xf4~\x0a\x04\x07$0\xf4~\x0a\x04\x07$0\xf4NM\
              \x1d\x01\x00Mixed_Case-name.long.txt",
            ),
            (45056, b"hello\x0a"),
            (47104, b"m10c-iso-42\x0a"),
            (
                49152,
                b"PX$\x01\xedA\x00\x00\x00\x00A\xed\x04\x00\x00\x00\x00\x00\x00\x04",
            ),
        ];
        /// The image's size: 25 blocks of 2048 bytes.
        pub(crate) const IMAGE_SIZE: usize = 25 * 2048;

        /// The image: its volume space, zeros but for the runs.
        pub(crate) fn image_bytes() -> std::vec::Vec<u8> {
            let mut img = std::vec![0u8; IMAGE_SIZE];
            for &(off, b) in IMAGE_RUNS {
                img[off..off + b.len()].copy_from_slice(b);
            }
            img
        }
    }

    /// A mount of the test image as `iso_mountfs` sets it up (2048-byte blocks, the root at
    /// block 20, Rock Ridge with no skip), leaked so that nodes may point at it.
    pub(crate) fn test_mnt(ftype: IsoFtype) -> &'static mut IsoMnt {
        let mp: &'static Mount = Box::leak(Box::new(Mount::new()));
        let devvp: &'static Vnode = Box::leak(Box::new(Vnode::new()));
        let mut root = [0u8; 34];
        root.copy_from_slice(&image::PVD_HEAD[156..190]);
        Box::leak(Box::new(IsoMnt {
            im_flags: 0,
            im_mountp: mp,
            im_dev: 0x0e01,
            im_devvp: devvp,
            logical_block_size: 2048,
            im_bshift: 11,
            im_bmask: 2047,
            volume_space_size: 25,
            im_export: crate::sys::mount::Netexport::new(),
            root,
            root_extent: 20,
            root_size: 2048,
            iso_ftype: ftype,
            rr_skip: 0,
            rr_skip0: 0,
            joliet_level: 0,
        }))
    }

    /// A node of `imp` with a vnode of its own, as `cd9660_vget_internal` links them.
    pub(crate) fn test_node(imp: &'static IsoMnt) -> (&'static IsoNode, &'static Vnode) {
        let ip: &'static IsoNode = Box::leak(Box::new(IsoNode::new()));
        let vp: &'static Vnode = Box::leak(Box::new(Vnode::new()));
        vp.v_tag.set(VT_ISOFS);
        vp.v_data.set(core::ptr::from_ref(ip).cast_mut().cast());
        ip.i_vnode.set(Some(vp));
        ip.i_mnt.set(Some(imp));
        (ip, vp)
    }

    /// A record view of a fixture.
    pub(crate) fn rec(b: &[u8]) -> IsoDirectoryRecord<'_> {
        IsoDirectoryRecord::new(b).expect("a record")
    }

    #[test]
    fn isonum_reads_each_encoding() {
        let b = [0x78, 0x56, 0x34, 0x12, 0x12, 0x34, 0x56, 0x78];
        assert_eq!(isonum_711(&[0xfe]), 0xfe);
        assert_eq!(isonum_712(&[0xfe]), -2);
        assert_eq!(isonum_721(&b), 0x5678);
        assert_eq!(isonum_722(&b), 0x7856);
        assert_eq!(isonum_722(&[0x80, 0x01]), 0x8001);
        // both-endian: the little-endian half is the one read
        assert_eq!(isonum_723(&[0x00, 0x08, 0x08, 0x00]), 2048);
        assert_eq!(isonum_731(&b), 0x1234_5678);
        assert_eq!(isonum_732(&b), 0x7856_3412);
        assert_eq!(isonum_733(&b), 0x1234_5678);
        assert_eq!(isonum_733(&[0xff, 0xff, 0xff, 0xff, 0, 0, 0, 0]), u32::MAX);
    }

    #[test]
    fn the_primary_descriptor_reads_in_place() {
        let mut vd = [0u8; 2048];
        vd[..190].copy_from_slice(&image::PVD_HEAD);
        let v = IsoVolumeDescriptor::from_bytes(&vd).expect("2048 bytes");
        assert_eq!(isonum_711(&v.type_), ISO_VD_PRIMARY);
        assert_eq!(&v.id, ISO_STANDARD_ID);
        let pri = IsoPrimaryDescriptor::from_bytes(&vd).expect("2048 bytes");
        assert_eq!(isonum_723(&pri.logical_block_size), 2048);
        assert_eq!(isonum_733(&pri.volume_space_size), 25);
        assert_eq!(&pri.volume_id[..4], b"M10C");
        let root = rec(&pri.root_directory_record);
        assert_eq!(isonum_711(root.length()), 34);
        assert_eq!(isonum_733(root.extent()), 20);
        assert_eq!(isonum_733(root.size()), 2048);
        assert_eq!(isonum_711(root.flags()) & 2, 2);
        assert_eq!(root.name0(), 0);
        assert!(IsoPrimaryDescriptor::from_bytes(&vd[..2047]).is_none());
        assert!(IsoExtendedAttributes::from_bytes(&vd[..250]).is_some());
    }

    #[test]
    fn directory_records_view_their_fields() {
        let r = rec(&image::FILE_REC);
        assert_eq!(usize::from(isonum_711(r.length())), image::FILE_REC.len());
        assert_eq!(isonum_711(r.ext_attr_length()), 0);
        assert_eq!(isonum_733(r.extent()), 23);
        assert_eq!(isonum_733(r.size()), 12);
        assert_eq!(isonum_711(r.flags()), 0);
        assert_eq!(isonum_723(r.volume_sequence_number()), 1);
        let n = usize::from(isonum_711(r.name_len()));
        assert_eq!(&r.name()[..n], b"M10C_ISO.TXT;1");
        // 2026-10-04 07:36:48, 12 quarter hours behind GMT, as makefs wrote it
        assert_eq!(r.date()[0], 126);
        assert!(IsoDirectoryRecord::new(&image::FILE_REC[..32]).is_none());
        let short = rec(&image::FILE_REC[..ISO_DIRECTORY_RECORD_SIZE]);
        assert_eq!(short.name0(), 0);
        assert!(short.name().is_empty());
    }

    #[test]
    fn the_fixtures_are_rock_ridge_records() {
        // the root's '.' entry starts its system use area with SP right after the name
        assert_eq!(&image::ROOT_DOT[34..40], b"SP\x07\x01\xbe\xef");
        assert_eq!(rec(&image::ROOT_DOTDOT).name0(), 1);
        assert_eq!(test_mnt(ISO_FTYPE_RRIP).root_extent, 20);
    }

    #[test]
    fn the_fixtures_are_cut_from_the_image() {
        let img = image::image_bytes();
        let at =
            |blk: usize, off: usize, len: usize| &img[blk * 2048 + off..blk * 2048 + off + len];
        assert_eq!(at(16, 0, 190), image::PVD_HEAD);
        assert_eq!(at(20, 0, 254), image::ROOT_DOT);
        assert_eq!(at(20, 324, 128), image::FILE_REC);
        assert_eq!(at(21, 140, 132), image::LINK_REC);
        assert_eq!(at(23, 0, 12), b"m10c-iso-42\n");
        assert_eq!(at(24, 0, 36), image::ROOT_CE);
    }

    #[test]
    #[ignore = "needs OPENBSD_SRC (just test-ref)"]
    fn values_match_the_c_header() {
        let defs = crate::reftest::defines("sys/isofs/cd9660/iso.h");
        for (name, value) in [
            ("ISO_VD_PRIMARY", i64::from(ISO_VD_PRIMARY)),
            ("ISO_VD_SUPPLEMENTARY", i64::from(ISO_VD_SUPPLEMENTARY)),
            ("ISO_VD_END", i64::from(ISO_VD_END)),
            ("ISO_DEFAULT_BLOCK_SHIFT", ISO_DEFAULT_BLOCK_SHIFT as i64),
            (
                "ISO_DIRECTORY_RECORD_SIZE",
                ISO_DIRECTORY_RECORD_SIZE as i64,
            ),
        ] {
            assert_eq!(crate::reftest::int(&defs, name), Some(value), "{name}");
        }
    }
}
/* </TESTS> */
