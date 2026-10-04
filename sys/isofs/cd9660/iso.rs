/*	$OpenBSD: iso.h,v 1.16 2021/03/05 07:01:36 jsg Exp $	*/
/*	$NetBSD: iso.h,v 1.20 1997/07/07 22:45:34 cgd Exp $	*/
/* <LICENSES> */
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

#[cfg(test)]
pub(crate) mod tests;
