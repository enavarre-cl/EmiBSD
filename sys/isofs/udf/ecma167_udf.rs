/*	$OpenBSD: ecma167-udf.h,v 1.10 2022/01/11 03:13:59 jsg Exp $	*/
/* $NetBSD: ecma167-udf.h,v 1.10 2008/06/24 15:30:33 reinoud Exp $ */
/* <LICENSES> */
/*-
 * Copyright (c) 2003, 2004, 2005, 2006, 2008 Reinoud Zandijk
 * Copyright (c) 2001, 2002 Scott Long <scottl@freebsd.org>
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
 *
 * THIS SOFTWARE IS PROVIDED BY THE AUTHOR AND CONTRIBUTORS ``AS IS'' AND
 * ANY EXPRESS OR IMPLIED WARRANTIES, INCLUDING, BUT NOT LIMITED TO, THE
 * IMPLIED WARRANTIES OF MERCHANTABILITY AND FITNESS FOR A PARTICULAR PURPOSE
 * ARE DISCLAIMED.  IN NO EVENT SHALL THE AUTHOR OR CONTRIBUTORS BE LIABLE
 * FOR ANY DIRECT, INDIRECT, INCIDENTAL, SPECIAL, EXEMPLARY, OR CONSEQUENTIAL
 * DAMAGES (INCLUDING, BUT NOT LIMITED TO, PROCUREMENT OF SUBSTITUTE GOODS
 * OR SERVICES; LOSS OF USE, DATA, OR PROFITS; OR BUSINESS INTERRUPTION)
 * HOWEVER CAUSED AND ON ANY THEORY OF LIABILITY, WHETHER IN CONTRACT, STRICT
 * LIABILITY, OR TORT (INCLUDING NEGLIGENCE OR OTHERWISE) ARISING IN ANY WAY
 * OUT OF THE USE OF THIS SOFTWARE, EVEN IF ADVISED OF THE POSSIBILITY OF
 * SUCH DAMAGE.
 *
 *
 * Extended and adapted for UDFv2.50+ bij Reinoud Zandijk based on the
 * original by Scott Long.
 *
 * 20030508 Made some small typo and explanatory comments
 * 20030510 Added UDF 2.01 structures
 * 20030519 Added/correct comments on multi-partitioned logical volume space
 * 20050616 Added pseudo overwrite
 * 20050624 Added the missing extended attribute types and `magic values'.
 * 20051106 Reworked some implementation use parts
 *
 */
/* </LICENSES> */

//! `<isofs/udf/ecma167-udf.h>`: the descriptors of ECMA-167 rev. 3 and of the OSTA UDF
//! profile (up to UDF 2.50) as they are on the medium: tags, extents and allocation
//! descriptors, the volume and logical volume descriptors, partition maps, the file set
//! descriptor, file identifier descriptors, (extended) file entries and extended attributes.
//!
//! Upstream: sys/isofs/udf/ecma167-udf.h @ 3ce1f3f79392
//!
//! Extended and adapted for UDF 2.50+ by Reinoud Zandijk from the original by Scott Long
//! (the changelog is in the licence comment above).
//!
//! Every structure is `__packed` in the C and is read in place from a buffer, its integers
//! little-endian. Here each is `#[repr(C, packed)]` with the C's members (alignment 1, the
//! C's offsets, any bit pattern valid), viewed in place with [`Packed::at`]; an integer member
//! is read by value (`{ x.member }`, then `letoh*`), a nested structure by reference (its
//! alignment is 1 too).
//!
//! ## Deviations
//! - The file name: `ecma167-udf.h` is `ecma167_udf.rs`, a `-` cannot appear in a module
//!   name.
//! - Two lines of the licence comment end in a blank in the C (` * `); rustfmt drops it.
//! - A flexible array member declared `x[1]` (`maps[1]`, `data[1]`, `entries[1]`,
//!   `st_loc[1]`, ...) is `x: [T; 0]`: the structure is its fixed part (`size_of` is what
//!   the C writes as `UDF_FENTRY_SIZE`, `UDF_EXTFENTRY_SIZE`, `UDF_FID_SIZE`; the C's
//!   `sizeof` counts one element more), and the tail is read from the buffer past it.
//! - The anonymous unions are named after their parent (`LongAdImpl`, `LvdUse`,
//!   `ImpvolImplUse`, `PartDescImplUse`, `LogvolIntImplUse`); the member macros
//!   (`longad_uniqueid`, `lv_fsd_loc`, `pd_part_hdr`, `lvint_next_unique_id`) are methods.
//!   A member named `type` is `type_`, one named `impl` is `impl_`.
//! - `GETICB(ad_type, fentry, offset)` is `T::at(data, offset)` over the file entry's
//!   `data[]`; `GETICBLEN(ad_type, icb)` is [`geticblen`].
//! - The anonymous `enum`s of tag identifiers, domain flags and access types are constants
//!   typed as the members that hold them (`u16` for `desc_tag.id`, `u32` for
//!   `part_desc.access_type`, `u8` for the domain flags of a `regid` suffix).

use core::mem::align_of;

/// A `__packed` on-disk structure, read in place over a buffer.
///
/// # Safety
///
/// The implementor is a `#[repr(C, packed)]` structure or union whose members are integers,
/// arrays of them and other `Packed` types only: its alignment is 1 and every bit pattern
/// is a valid value.
pub unsafe trait Packed: Copy {
    /// `sizeof`: the size of the fixed part.
    const SIZE: usize = size_of::<Self>();

    /// The structure at byte `off` of `buf` (the C's cast of `&buf[off]`), `None` when it does
    /// not fit in `buf`.
    fn at(buf: &[u8], off: usize) -> Option<&Self> {
        let end = off.checked_add(Self::SIZE)?;
        let bytes = buf.get(off..end)?;
        // SAFETY: the trait's contract: alignment 1 and no invalid bit patterns, so any
        // `SIZE` initialised bytes are a valid value; the reference borrows `buf`.
        Some(unsafe { &*bytes.as_ptr().cast::<Self>() })
    }
}

/// Marks each listed type [`Packed`] and checks at compile time that its alignment is 1.
macro_rules! packed {
    ($($t:ty),* $(,)?) => {
        $(
            // SAFETY: `$t` is `#[repr(C, packed)]` and made of integers, byte arrays and
            // other `Packed` types (see its definition); the assertion below pins the
            // alignment.
            unsafe impl Packed for $t {}
            const _: () = assert!(align_of::<$t>() == 1);
        )*
    };
}

/// `struct vrs_desc`: volume recognition sequence (ECMA 167 rev. 3 16.1).
#[repr(C, packed)]
#[derive(Clone, Copy)]
pub struct VrsDesc {
    /// `struct_type`.
    pub struct_type: u8,
    /// `identifier`: one of the `VRS_*`.
    pub identifier: [u8; 5],
    /// `version`.
    pub version: u8,
    /// `data`.
    pub data: [u8; 2041],
}

/// `VRS_NSR02`.
pub const VRS_NSR02: &[u8; 5] = b"NSR02";
/// `VRS_NSR03`.
pub const VRS_NSR03: &[u8; 5] = b"NSR03";
/// `VRS_BEA01`.
pub const VRS_BEA01: &[u8; 5] = b"BEA01";
/// `VRS_TEA01`.
pub const VRS_TEA01: &[u8; 5] = b"TEA01";
/// `VRS_CD001`.
pub const VRS_CD001: &[u8; 5] = b"CD001";
/// `VRS_CDW02`.
pub const VRS_CDW02: &[u8; 5] = b"CDW02";

// Structure/definitions/constants a la ECMA 167 rev. 3

/// `MAX_TAGID_VOLUMES`.
pub const MAX_TAGID_VOLUMES: u16 = 9;

// Tag identifiers.

/// `TAGID_SPARING_TABLE`.
pub const TAGID_SPARING_TABLE: u16 = 0;
/// `TAGID_PRI_VOL`.
pub const TAGID_PRI_VOL: u16 = 1;
/// `TAGID_ANCHOR`.
pub const TAGID_ANCHOR: u16 = 2;
/// `TAGID_VOL`.
pub const TAGID_VOL: u16 = 3;
/// `TAGID_IMP_VOL`.
pub const TAGID_IMP_VOL: u16 = 4;
/// `TAGID_PARTITION`.
pub const TAGID_PARTITION: u16 = 5;
/// `TAGID_LOGVOL`.
pub const TAGID_LOGVOL: u16 = 6;
/// `TAGID_UNALLOC_SPACE`.
pub const TAGID_UNALLOC_SPACE: u16 = 7;
/// `TAGID_TERM`.
pub const TAGID_TERM: u16 = 8;
/// `TAGID_LOGVOL_INTEGRITY`.
pub const TAGID_LOGVOL_INTEGRITY: u16 = 9;
/// `TAGID_FSD`.
pub const TAGID_FSD: u16 = 256;
/// `TAGID_FID`.
pub const TAGID_FID: u16 = 257;
/// `TAGID_ALLOCEXTENT`.
pub const TAGID_ALLOCEXTENT: u16 = 258;
/// `TAGID_INDIRECTENTRY`.
pub const TAGID_INDIRECTENTRY: u16 = 259;
/// `TAGID_ICB_TERM`.
pub const TAGID_ICB_TERM: u16 = 260;
/// `TAGID_FENTRY`.
pub const TAGID_FENTRY: u16 = 261;
/// `TAGID_EXTATTR_HDR`.
pub const TAGID_EXTATTR_HDR: u16 = 262;
/// `TAGID_UNALL_SP_ENTRY`.
pub const TAGID_UNALL_SP_ENTRY: u16 = 263;
/// `TAGID_SPACE_BITMAP`.
pub const TAGID_SPACE_BITMAP: u16 = 264;
/// `TAGID_PART_INTEGRITY`.
pub const TAGID_PART_INTEGRITY: u16 = 265;
/// `TAGID_EXTFENTRY`.
pub const TAGID_EXTFENTRY: u16 = 266;
/// `TAGID_MAX`.
pub const TAGID_MAX: u16 = 266;

/// `UDF_DOMAIN_FLAG_HARD_WRITE_PROTECT`.
pub const UDF_DOMAIN_FLAG_HARD_WRITE_PROTECT: u8 = 1;
/// `UDF_DOMAIN_FLAG_SOFT_WRITE_PROTECT`.
pub const UDF_DOMAIN_FLAG_SOFT_WRITE_PROTECT: u8 = 2;

/// `UDF_ACCESSTYPE_NOT_SPECIFIED`: unknown.
pub const UDF_ACCESSTYPE_NOT_SPECIFIED: u32 = 0;
/// `UDF_ACCESSTYPE_PSEUDO_OVERWITE`: pseudo overwritable, e.g. BD-R's LOW.
pub const UDF_ACCESSTYPE_PSEUDO_OVERWITE: u32 = 0;
/// `UDF_ACCESSTYPE_READ_ONLY`: really only readable.
pub const UDF_ACCESSTYPE_READ_ONLY: u32 = 1;
/// `UDF_ACCESSTYPE_WRITE_ONCE`: write once and you're done.
pub const UDF_ACCESSTYPE_WRITE_ONCE: u32 = 2;
/// `UDF_ACCESSTYPE_REWRITABLE`: may need extra work to rewrite.
pub const UDF_ACCESSTYPE_REWRITABLE: u32 = 3;
/// `UDF_ACCESSTYPE_OVERWRITABLE`: no limits on rewriting; e.g. harddisc.
pub const UDF_ACCESSTYPE_OVERWRITABLE: u32 = 4;

/// `struct desc_tag`: descriptor tag \[3/7.2\].
#[repr(C, packed)]
#[derive(Clone, Copy)]
pub struct DescTag {
    /// `id`: one of the `TAGID_*`.
    pub id: u16,
    /// `descriptor_ver`.
    pub descriptor_ver: u16,
    /// `cksum`: the sum of the other 15 bytes of the tag.
    pub cksum: u8,
    /// `reserved`.
    pub reserved: u8,
    /// `serial_num`.
    pub serial_num: u16,
    /// `desc_crc`.
    pub desc_crc: u16,
    /// `desc_crc_len`.
    pub desc_crc_len: u16,
    /// `tag_loc`.
    pub tag_loc: u32,
}

impl DescTag {
    /// The tag's 16 bytes as they are on the medium (`(uint8_t *)tag`).
    pub fn as_bytes(&self) -> &[u8; UDF_DESC_TAG_LENGTH] {
        // SAFETY: `DescTag` is `#[repr(C, packed)]` integers of 16 bytes without padding
        // (checked below), so its storage is 16 initialised bytes; the reference borrows
        // `self`.
        unsafe { &*core::ptr::from_ref(self).cast::<[u8; UDF_DESC_TAG_LENGTH]>() }
    }
}

/// `UDF_DESC_TAG_LENGTH`.
pub const UDF_DESC_TAG_LENGTH: usize = 16;

/// `struct lb_addr`: recorded address \[4/7.1\], within partition space.
#[repr(C, packed)]
#[derive(Clone, Copy)]
pub struct LbAddr {
    /// `lb_num`.
    pub lb_num: u32,
    /// `part_num`.
    pub part_num: u16,
}

/// `struct extent_ad`: extent descriptor \[3/7.1\].
#[repr(C, packed)]
#[derive(Clone, Copy)]
pub struct ExtentAd {
    /// `len`.
    pub len: u32,
    /// `loc`.
    pub loc: u32,
}

/// `struct short_ad`: short allocation descriptor \[4/14.14.1\].
#[repr(C, packed)]
#[derive(Clone, Copy)]
pub struct ShortAd {
    /// `len`.
    pub len: u32,
    /// `lb_num`.
    pub lb_num: u32,
}

/// `struct UDF_ADImp_use`: the implementation use of a long allocation descriptor
/// \[4/14.14.2\].
#[repr(C, packed)]
#[derive(Clone, Copy)]
pub struct UdfAdimpUse {
    /// `flags`.
    pub flags: u16,
    /// `unique_id`.
    pub unique_id: u32,
}

/// `UDF_ADIMP_FLAGS_EXTENT_ERASED`.
pub const UDF_ADIMP_FLAGS_EXTENT_ERASED: u16 = 1;

/// The `impl` union of `struct long_ad`.
#[repr(C, packed)]
#[derive(Clone, Copy)]
pub union LongAdImpl {
    /// `bytes`.
    pub bytes: [u8; 6],
    /// `im_used`.
    pub im_used: UdfAdimpUse,
}

/// `struct long_ad`: long allocation descriptor \[4/14.14.2\].
#[repr(C, packed)]
#[derive(Clone, Copy)]
pub struct LongAd {
    /// `len`.
    pub len: u32,
    /// `loc`: within a logical volume mapped partition space!
    pub loc: LbAddr,
    /// `impl`.
    pub impl_: LongAdImpl,
}

impl LongAd {
    /// `longad_uniqueid` (`impl.im_used.unique_id`).
    pub fn longad_uniqueid(&self) -> u32 {
        // SAFETY: both members of the union are plain integers over the same 6 bytes, any
        // bit pattern is valid.
        let im = unsafe { self.impl_.im_used };
        im.unique_id
    }
}

/// `struct ext_ad`: extended allocation descriptor \[4/14.14.3\]; identifies an extent of
/// allocation descriptors.
#[repr(C, packed)]
#[derive(Clone, Copy)]
pub struct ExtAd {
    /// `ex_len`.
    pub ex_len: u32,
    /// `rec_len`.
    pub rec_len: u32,
    /// `inf_len`.
    pub inf_len: u32,
    /// `ex_loc`.
    pub ex_loc: LbAddr,
    /// `reserved`.
    pub reserved: [u8; 2],
}

/// `union icb`: Information Control Block; positioning.
#[repr(C, packed)]
#[derive(Clone, Copy)]
pub union Icb {
    /// `s_ad`.
    pub s_ad: ShortAd,
    /// `l_ad`.
    pub l_ad: LongAd,
    /// `e_ad`.
    pub e_ad: ExtAd,
}

// short/long/ext extent have flags encoded in length

/// `UDF_EXT_ALLOCATED`.
pub const UDF_EXT_ALLOCATED: u32 = 0 << 30;
/// `UDF_EXT_FREED`.
pub const UDF_EXT_FREED: u32 = 1 << 30;
/// `UDF_EXT_ALLOCATED_BUT_NOT_USED`.
pub const UDF_EXT_ALLOCATED_BUT_NOT_USED: u32 = 1 << 30;
/// `UDF_EXT_FREE`.
pub const UDF_EXT_FREE: u32 = 2 << 30;
/// `UDF_EXT_REDIRECT`.
pub const UDF_EXT_REDIRECT: u32 = 3 << 30;

/// `UDF_EXT_FLAGS(len)`.
pub const fn udf_ext_flags(len: u32) -> u32 {
    len & (3 << 30)
}

/// `UDF_EXT_LEN(len)`.
pub const fn udf_ext_len(len: u32) -> u32 {
    len & ((1 << 30) - 1)
}

/// `UDF_EXT_MAXLEN`.
pub const UDF_EXT_MAXLEN: u32 = (1 << 30) - 1;

/// `struct charspec`: character set spec \[1/7.2.1\].
#[repr(C, packed)]
#[derive(Clone, Copy)]
pub struct Charspec {
    /// `type`.
    pub type_: u8,
    /// `inf`.
    pub inf: [u8; 63],
}

/// `struct pathcomp`.
#[repr(C, packed)]
#[derive(Clone, Copy)]
pub struct Pathcomp {
    /// `type`: one of the `UDF_PATH_COMP_*`.
    pub type_: u8,
    /// `l_ci`.
    pub l_ci: u8,
    /// `comp_filever`.
    pub comp_filever: u16,
    /// `ident`.
    pub ident: [u8; 256],
}

/// `UDF_PATH_COMP_SIZE`.
pub const UDF_PATH_COMP_SIZE: usize = 4;
/// `UDF_PATH_COMP_RESERVED`.
pub const UDF_PATH_COMP_RESERVED: u8 = 0;
/// `UDF_PATH_COMP_ROOT`.
pub const UDF_PATH_COMP_ROOT: u8 = 1;
/// `UDF_PATH_COMP_MOUNTROOT`.
pub const UDF_PATH_COMP_MOUNTROOT: u8 = 2;
/// `UDF_PATH_COMP_PARENTDIR`.
pub const UDF_PATH_COMP_PARENTDIR: u8 = 3;
/// `UDF_PATH_COMP_CURDIR`.
pub const UDF_PATH_COMP_CURDIR: u8 = 4;
/// `UDF_PATH_COMP_NAME`.
pub const UDF_PATH_COMP_NAME: u8 = 5;

/// `struct timestamp`: timestamp \[1/7.3\].
#[repr(C, packed)]
#[derive(Clone, Copy)]
pub struct Timestamp {
    /// `type_tz`: the type (top 4 bits) and the time zone offset in minutes (12 bits, two's
    /// complement).
    pub type_tz: u16,
    /// `year`.
    pub year: u16,
    /// `month`.
    pub month: u8,
    /// `day`.
    pub day: u8,
    /// `hour`.
    pub hour: u8,
    /// `minute`.
    pub minute: u8,
    /// `second`.
    pub second: u8,
    /// `centisec`.
    pub centisec: u8,
    /// `hund_usec`.
    pub hund_usec: u8,
    /// `usec`.
    pub usec: u8,
}

/// `UDF_TIMESTAMP_SIZE`.
pub const UDF_TIMESTAMP_SIZE: usize = 12;

/// `UDF_REGID_ID_SIZE`.
pub const UDF_REGID_ID_SIZE: usize = 23;

/// `struct regid`: entity identifier \[1/7.4\].
#[repr(C, packed)]
#[derive(Clone, Copy)]
pub struct Regid {
    /// `flags`.
    pub flags: u8,
    /// `id`.
    pub id: [u8; UDF_REGID_ID_SIZE],
    /// `id_suffix`.
    pub id_suffix: [u8; 8],
}

/// `struct icb_tag`: ICB tag \[4/14.6\].
#[repr(C, packed)]
#[derive(Clone, Copy)]
pub struct IcbTag {
    /// `prev_num_dirs`.
    pub prev_num_dirs: u32,
    /// `strat_type`.
    pub strat_type: u16,
    /// `strat_param`.
    pub strat_param: [u8; 2],
    /// `max_num_entries`.
    pub max_num_entries: u16,
    /// `reserved`.
    pub reserved: u8,
    /// `file_type`: one of the `UDF_ICB_FILETYPE_*`.
    pub file_type: u8,
    /// `parent_icb`.
    pub parent_icb: LbAddr,
    /// `flags`: the `UDF_ICB_TAG_FLAGS_*`.
    pub flags: u16,
}

/// `UDF_ICB_TAG_FLAGS_ALLOC_MASK`.
pub const UDF_ICB_TAG_FLAGS_ALLOC_MASK: u16 = 0x03;
/// `UDF_ICB_SHORT_ALLOC`.
pub const UDF_ICB_SHORT_ALLOC: u16 = 0x00;
/// `UDF_ICB_LONG_ALLOC`.
pub const UDF_ICB_LONG_ALLOC: u16 = 0x01;
/// `UDF_ICB_EXT_ALLOC`.
pub const UDF_ICB_EXT_ALLOC: u16 = 0x02;
/// `UDF_ICB_INTERN_ALLOC`.
pub const UDF_ICB_INTERN_ALLOC: u16 = 0x03;

/// `UDF_ICB_TAG_FLAGS_DIRORDERED`.
pub const UDF_ICB_TAG_FLAGS_DIRORDERED: u16 = 1 << 3;
/// `UDF_ICB_TAG_FLAGS_NONRELOC`.
pub const UDF_ICB_TAG_FLAGS_NONRELOC: u16 = 1 << 4;
/// `UDF_ICB_TAG_FLAGS_CONTIGUOUS`.
pub const UDF_ICB_TAG_FLAGS_CONTIGUOUS: u16 = 1 << 9;
/// `UDF_ICB_TAG_FLAGS_MULTIPLEVERS`.
pub const UDF_ICB_TAG_FLAGS_MULTIPLEVERS: u16 = 1 << 12;

/// `UDF_ICB_TAG_FLAGS_SETUID`.
pub const UDF_ICB_TAG_FLAGS_SETUID: u16 = 1 << 6;
/// `UDF_ICB_TAG_FLAGS_SETGID`.
pub const UDF_ICB_TAG_FLAGS_SETGID: u16 = 1 << 7;
/// `UDF_ICB_TAG_FLAGS_STICKY`.
pub const UDF_ICB_TAG_FLAGS_STICKY: u16 = 1 << 8;

/// `UDF_ICB_FILETYPE_UNKNOWN`.
pub const UDF_ICB_FILETYPE_UNKNOWN: u8 = 0;
/// `UDF_ICB_FILETYPE_UNALLOCSPACE`.
pub const UDF_ICB_FILETYPE_UNALLOCSPACE: u8 = 1;
/// `UDF_ICB_FILETYPE_PARTINTEGRITY`.
pub const UDF_ICB_FILETYPE_PARTINTEGRITY: u8 = 2;
/// `UDF_ICB_FILETYPE_INDIRECTENTRY`.
pub const UDF_ICB_FILETYPE_INDIRECTENTRY: u8 = 3;
/// `UDF_ICB_FILETYPE_DIRECTORY`.
pub const UDF_ICB_FILETYPE_DIRECTORY: u8 = 4;
/// `UDF_ICB_FILETYPE_RANDOMACCESS`.
pub const UDF_ICB_FILETYPE_RANDOMACCESS: u8 = 5;
/// `UDF_ICB_FILETYPE_BLOCKDEVICE`.
pub const UDF_ICB_FILETYPE_BLOCKDEVICE: u8 = 6;
/// `UDF_ICB_FILETYPE_CHARDEVICE`.
pub const UDF_ICB_FILETYPE_CHARDEVICE: u8 = 7;
/// `UDF_ICB_FILETYPE_EXTATTRREC`.
pub const UDF_ICB_FILETYPE_EXTATTRREC: u8 = 8;
/// `UDF_ICB_FILETYPE_FIFO`.
pub const UDF_ICB_FILETYPE_FIFO: u8 = 9;
/// `UDF_ICB_FILETYPE_SOCKET`.
pub const UDF_ICB_FILETYPE_SOCKET: u8 = 10;
/// `UDF_ICB_FILETYPE_TERM`.
pub const UDF_ICB_FILETYPE_TERM: u8 = 11;
/// `UDF_ICB_FILETYPE_SYMLINK`.
pub const UDF_ICB_FILETYPE_SYMLINK: u8 = 12;
/// `UDF_ICB_FILETYPE_STREAMDIR`.
pub const UDF_ICB_FILETYPE_STREAMDIR: u8 = 13;
/// `UDF_ICB_FILETYPE_VAT`.
pub const UDF_ICB_FILETYPE_VAT: u8 = 248;
/// `UDF_ICB_FILETYPE_REALTIME`.
pub const UDF_ICB_FILETYPE_REALTIME: u8 = 249;
/// `UDF_ICB_FILETYPE_META_MAIN`.
pub const UDF_ICB_FILETYPE_META_MAIN: u8 = 250;
/// `UDF_ICB_FILETYPE_META_MIRROR`.
pub const UDF_ICB_FILETYPE_META_MIRROR: u8 = 251;

/// `struct anchor_vdp`: Anchor Volume Descriptor Pointer \[3/10.2\].
#[repr(C, packed)]
#[derive(Clone, Copy)]
pub struct AnchorVdp {
    /// `tag`.
    pub tag: DescTag,
    /// `main_vds_ex`: to main volume descriptor set; 16 sectors min.
    pub main_vds_ex: ExtentAd,
    /// `reserve_vds_ex`: copy of main volume descriptor set; 16 sectors min.
    pub reserve_vds_ex: ExtentAd,
}

/// `struct vol_desc_ptr`: Volume Descriptor Pointer \[3/10.3\].
#[repr(C, packed)]
#[derive(Clone, Copy)]
pub struct VolDescPtr {
    /// `tag`: use for extending the volume descriptor space.
    pub tag: DescTag,
    /// `vds_number`.
    pub vds_number: u32,
    /// `next_vds_ex`: points to the next block for volume descriptor space.
    pub next_vds_ex: ExtentAd,
}

/// `struct pri_vol_desc`: Primary Volume Descriptor \[3/10.1\].
#[repr(C, packed)]
#[derive(Clone, Copy)]
pub struct PriVolDesc {
    /// `tag`.
    pub tag: DescTag,
    /// `seq_num`: MAX prevail.
    pub seq_num: u32,
    /// `pvd_num`: assigned by author; 0 is special as in it may only occur once.
    pub pvd_num: u32,
    /// `vol_id`: KEY; main identifier of this disc.
    pub vol_id: [u8; 32],
    /// `vds_num`: volume descriptor number; i.e. what volume number is it.
    pub vds_num: u16,
    /// `max_vol_seq`: maximum volume descriptor number known.
    pub max_vol_seq: u16,
    /// `ichg_lvl`.
    pub ichg_lvl: u16,
    /// `max_ichg_lvl`.
    pub max_ichg_lvl: u16,
    /// `charset_list`.
    pub charset_list: u32,
    /// `max_charset_list`.
    pub max_charset_list: u32,
    /// `volset_id`: KEY; if part of a multi-disc set or a band of volumes.
    pub volset_id: [u8; 128],
    /// `desc_charset`: KEY according to ECMA 167.
    pub desc_charset: Charspec,
    /// `explanatory_charset`.
    pub explanatory_charset: Charspec,
    /// `vol_abstract`.
    pub vol_abstract: ExtentAd,
    /// `vol_copyright`.
    pub vol_copyright: ExtentAd,
    /// `app_id`.
    pub app_id: Regid,
    /// `time`.
    pub time: Timestamp,
    /// `imp_id`.
    pub imp_id: Regid,
    /// `imp_use`.
    pub imp_use: [u8; 64],
    /// `prev_vds_loc`: location of predecessor.
    pub prev_vds_loc: u32,
    /// `flags`: bit 0: if set indicates volume set name is meaningful.
    pub flags: u16,
    /// `reserved`.
    pub reserved: [u8; 22],
}

/// `struct udf_lv_info`: UDF specific implementation use part of the implementation use
/// volume descriptor.
#[repr(C, packed)]
#[derive(Clone, Copy)]
pub struct UdfLvInfo {
    /// `lvi_charset`.
    pub lvi_charset: Charspec,
    /// `logvol_id`.
    pub logvol_id: [u8; 128],
    /// `lvinfo1`.
    pub lvinfo1: [u8; 36],
    /// `lvinfo2`.
    pub lvinfo2: [u8; 36],
    /// `lvinfo3`.
    pub lvinfo3: [u8; 36],
    /// `impl_id`.
    pub impl_id: Regid,
    /// `impl_use`.
    pub impl_use: [u8; 128],
}

/// The `_impl_use` union of `struct impvol_desc`.
#[repr(C, packed)]
#[derive(Clone, Copy)]
pub union ImpvolImplUse {
    /// `lv_info`.
    pub lv_info: UdfLvInfo,
    /// `impl_use`.
    pub impl_use: [u8; 460],
}

/// `struct impvol_desc`: Implementation use Volume Descriptor.
#[repr(C, packed)]
#[derive(Clone, Copy)]
pub struct ImpvolDesc {
    /// `tag`.
    pub tag: DescTag,
    /// `seq_num`.
    pub seq_num: u32,
    /// `impl_id`.
    pub impl_id: Regid,
    /// `_impl_use`.
    pub _impl_use: ImpvolImplUse,
}

/// The `_lvd_use` union of `struct logvol_desc`.
#[repr(C, packed)]
#[derive(Clone, Copy)]
pub union LvdUse {
    /// `fsd_loc`: to fileset descriptor SEQUENCE.
    pub fsd_loc: LongAd,
    /// `logvol_content_use`.
    pub logvol_content_use: [u8; 16],
}

/// `struct logvol_desc`: Logical Volume Descriptor \[3/10.6\].
#[repr(C, packed)]
#[derive(Clone, Copy)]
pub struct LogvolDesc {
    /// `tag`.
    pub tag: DescTag,
    /// `seq_num`: MAX prevail.
    pub seq_num: u32,
    /// `desc_charset`: KEY.
    pub desc_charset: Charspec,
    /// `logvol_id`: KEY.
    pub logvol_id: [u8; 128],
    /// `lb_size`.
    pub lb_size: u32,
    /// `domain_id`.
    pub domain_id: Regid,
    /// `_lvd_use`.
    pub _lvd_use: LvdUse,
    /// `mt_l`: partition map length.
    pub mt_l: u32,
    /// `n_pm`: number of partition maps.
    pub n_pm: u32,
    /// `imp_id`.
    pub imp_id: Regid,
    /// `imp_use`.
    pub imp_use: [u8; 128],
    /// `integrity_seq_loc`.
    pub integrity_seq_loc: ExtentAd,
    /// `maps`: the partition maps follow (the module's deviations).
    pub maps: [u8; 0],
}

impl LogvolDesc {
    /// `lv_fsd_loc` (`_lvd_use.fsd_loc`).
    pub fn lv_fsd_loc(&self) -> LongAd {
        // SAFETY: both members are plain integers over the same 16 bytes, any bit pattern is
        // valid.
        unsafe { self._lvd_use.fsd_loc }
    }
}

/// `UDF_INTEGRITY_OPEN`.
pub const UDF_INTEGRITY_OPEN: u32 = 0;
/// `UDF_INTEGRITY_CLOSED`.
pub const UDF_INTEGRITY_CLOSED: u32 = 1;

/// `UDF_PMAP_SIZE`.
pub const UDF_PMAP_SIZE: usize = 64;

/// `struct part_map_1`: Type 1 Partition Map \[3/10.7.2\].
#[repr(C, packed)]
#[derive(Clone, Copy)]
pub struct PartMap1 {
    /// `type`.
    pub type_: u8,
    /// `len`.
    pub len: u8,
    /// `vol_seq_num`.
    pub vol_seq_num: u16,
    /// `part_num`.
    pub part_num: u16,
}

/// `struct part_map_2`: Type 2 Partition Map \[3/10.7.3\].
#[repr(C, packed)]
#[derive(Clone, Copy)]
pub struct PartMap2 {
    /// `type`.
    pub type_: u8,
    /// `len`.
    pub len: u8,
    /// `reserved`.
    pub reserved: [u8; 2],
    /// `part_id`.
    pub part_id: Regid,
    /// `vol_seq_num`.
    pub vol_seq_num: u16,
    /// `part_num`.
    pub part_num: u16,
    /// `reserved2`.
    pub reserved2: [u8; 24],
}

/// `struct part_map_virt`: Virtual Partition Map \[UDF 2.01/2.2.8\].
#[repr(C, packed)]
#[derive(Clone, Copy)]
pub struct PartMapVirt {
    /// `type`.
    pub type_: u8,
    /// `len`.
    pub len: u8,
    /// `reserved`.
    pub reserved: [u8; 2],
    /// `id`.
    pub id: Regid,
    /// `vol_seq_num`.
    pub vol_seq_num: u16,
    /// `part_num`.
    pub part_num: u16,
    /// `reserved1`.
    pub reserved1: [u8; 24],
}

/// `struct part_map_spare`: Sparable Partition Map \[UDF 2.01/2.2.9\].
#[repr(C, packed)]
#[derive(Clone, Copy)]
pub struct PartMapSpare {
    /// `type`.
    pub type_: u8,
    /// `len`.
    pub len: u8,
    /// `reserved`.
    pub reserved: [u8; 2],
    /// `id`.
    pub id: Regid,
    /// `vol_seq_num`.
    pub vol_seq_num: u16,
    /// `part_num`.
    pub part_num: u16,
    /// `packet_len`.
    pub packet_len: u16,
    /// `n_st`: number of redundant sparing tables range 1-4.
    pub n_st: u8,
    /// `reserved1`.
    pub reserved1: u8,
    /// `st_size`: size of EACH sparing table.
    pub st_size: u32,
    /// `st_loc`: locations of sparing tables (`u32`s follow, the module's deviations).
    pub st_loc: [u32; 0],
}

/// `struct part_map_meta`: Metadata Partition Map \[UDF 2.50/2.2.10\].
#[repr(C, packed)]
#[derive(Clone, Copy)]
pub struct PartMapMeta {
    /// `type`.
    pub type_: u8,
    /// `len`.
    pub len: u8,
    /// `reserved`.
    pub reserved: [u8; 2],
    /// `id`.
    pub id: Regid,
    /// `vol_seq_num`.
    pub vol_seq_num: u16,
    /// `part_num`.
    pub part_num: u16,
    /// `meta_file_lbn`: logical block number for file entry within `part_num`.
    pub meta_file_lbn: u32,
    /// `meta_mirror_file_lbn`.
    pub meta_mirror_file_lbn: u32,
    /// `meta_bitmap_file_lbn`.
    pub meta_bitmap_file_lbn: u32,
    /// `alloc_unit_size`: allocation unit size in blocks.
    pub alloc_unit_size: u32,
    /// `alignment_unit_size`: alignment necessary in blocks.
    pub alignment_unit_size: u16,
    /// `flags`.
    pub flags: u8,
    /// `reserved1`.
    pub reserved1: [u8; 5],
}

/// `METADATA_DUPLICATED`.
pub const METADATA_DUPLICATED: u8 = 1;

/// `union udf_pmap`.
#[repr(C, packed)]
#[derive(Clone, Copy)]
pub union UdfPmap {
    /// `data`.
    pub data: [u8; UDF_PMAP_SIZE],
    /// `pm1`.
    pub pm1: PartMap1,
    /// `pm2`.
    pub pm2: PartMap2,
    /// `pmv`.
    pub pmv: PartMapVirt,
    /// `pms`.
    pub pms: PartMapSpare,
    /// `pmm`.
    pub pmm: PartMapMeta,
}

/// `struct spare_map_entry`: Sparing Map Entry \[UDF 2.01/2.2.11\].
#[repr(C, packed)]
#[derive(Clone, Copy)]
pub struct SpareMapEntry {
    /// `org`: partition relative address.
    pub org: u32,
    /// `map`: absolute disc address (!) can be in partition, but doesn't have to be.
    pub map: u32,
}

/// `struct udf_sparing_table`: Sparing Table \[UDF 2.01/2.2.11\].
#[repr(C, packed)]
#[derive(Clone, Copy)]
pub struct UdfSparingTable {
    /// `tag`.
    pub tag: DescTag,
    /// `id`.
    pub id: Regid,
    /// `rt_l`: Relocation Table len.
    pub rt_l: u16,
    /// `reserved`.
    pub reserved: [u8; 2],
    /// `seq_num`.
    pub seq_num: u32,
    /// `entries`: the `SpareMapEntry`s follow (the module's deviations).
    pub entries: [SpareMapEntry; 0],
}

/// `UDF_NO_PREV_VAT`.
pub const UDF_NO_PREV_VAT: u32 = 0xffff_ffff;

/// `struct udf_oldvat_tail`: UDF 1.50 VAT suffix \[UDF 2.2.10 (UDF 1.50 spec)\].
#[repr(C, packed)]
#[derive(Clone, Copy)]
pub struct UdfOldvatTail {
    /// `id`: "*UDF Virtual Alloc Tbl".
    pub id: Regid,
    /// `prev_vat`.
    pub prev_vat: u32,
}

/// `struct udf_vat`: VAT table \[UDF 2.0.1/2.2.10\].
#[repr(C, packed)]
#[derive(Clone, Copy)]
pub struct UdfVat {
    /// `header_len`.
    pub header_len: u16,
    /// `impl_use_len`.
    pub impl_use_len: u16,
    /// `logvol_id`: newer version of the LVD one.
    pub logvol_id: [u8; 128],
    /// `prev_vat`.
    pub prev_vat: u32,
    /// `num_files`.
    pub num_files: u32,
    /// `num_directories`.
    pub num_directories: u32,
    /// `min_udf_readver`.
    pub min_udf_readver: u16,
    /// `min_udf_writever`.
    pub min_udf_writever: u16,
    /// `max_udf_writever`.
    pub max_udf_writever: u16,
    /// `reserved`.
    pub reserved: u16,
    /// `data`: impl.use followed by VAT entries (`uint32_t`).
    pub data: [u8; 0],
}

/// `struct space_bitmap_desc`: space bitmap descriptor as found in the partition header
/// descriptor.
#[repr(C, packed)]
#[derive(Clone, Copy)]
pub struct SpaceBitmapDesc {
    /// `tag`: TagId 264.
    pub tag: DescTag,
    /// `num_bits`: number of bits.
    pub num_bits: u32,
    /// `num_bytes`: bytes that contain it.
    pub num_bytes: u32,
    /// `data`.
    pub data: [u8; 0],
}

/// `struct space_entry_desc`: unalloc space entry as found in the partition header
/// descriptor.
#[repr(C, packed)]
#[derive(Clone, Copy)]
pub struct SpaceEntryDesc {
    /// `tag`: TagId 263.
    pub tag: DescTag,
    /// `icbtag`: type 1.
    pub icbtag: IcbTag,
    /// `l_ad`: in bytes.
    pub l_ad: u32,
    /// `entry`.
    pub entry: [u8; 0],
}

/// `struct part_hdr_desc`: partition header descriptor; in the contents_use of part_desc.
#[repr(C, packed)]
#[derive(Clone, Copy)]
pub struct PartHdrDesc {
    /// `unalloc_space_table`.
    pub unalloc_space_table: ShortAd,
    /// `unalloc_space_bitmap`.
    pub unalloc_space_bitmap: ShortAd,
    /// `part_integrity_table`: has to be ZERO for UDF.
    pub part_integrity_table: ShortAd,
    /// `freed_space_table`.
    pub freed_space_table: ShortAd,
    /// `freed_space_bitmap`.
    pub freed_space_bitmap: ShortAd,
    /// `reserved`.
    pub reserved: [u8; 88],
}

/// The `_impl_use` union of `struct part_desc`.
#[repr(C, packed)]
#[derive(Clone, Copy)]
pub union PartDescImplUse {
    /// `part_hdr`.
    pub part_hdr: PartHdrDesc,
    /// `contents_use`.
    pub contents_use: [u8; 128],
}

/// `struct part_desc`: Partition Descriptor \[3/10.5\].
#[repr(C, packed)]
#[derive(Clone, Copy)]
pub struct PartDesc {
    /// `tag`.
    pub tag: DescTag,
    /// `seq_num`: MAX prevailing.
    pub seq_num: u32,
    /// `flags`: bit 0: if set the space is allocated.
    pub flags: u16,
    /// `part_num`: KEY.
    pub part_num: u16,
    /// `contents`.
    pub contents: Regid,
    /// `_impl_use`.
    pub _impl_use: PartDescImplUse,
    /// `access_type`: R/W, WORM etc.
    pub access_type: u32,
    /// `start_loc`: start of partition with given length.
    pub start_loc: u32,
    /// `part_len`.
    pub part_len: u32,
    /// `imp_id`.
    pub imp_id: Regid,
    /// `imp_use`.
    pub imp_use: [u8; 128],
    /// `reserved`.
    pub reserved: [u8; 156],
}

impl PartDesc {
    /// `pd_part_hdr` (`_impl_use.part_hdr`).
    pub fn pd_part_hdr(&self) -> PartHdrDesc {
        // SAFETY: both members are plain integers over the same 128 bytes, any bit pattern is
        // valid.
        unsafe { self._impl_use.part_hdr }
    }
}

/// `UDF_PART_FLAG_ALLOCATED`.
pub const UDF_PART_FLAG_ALLOCATED: u16 = 1;

/// `struct unalloc_sp_desc`: Unallocated Space Descriptor (UDF 2.01/2.2.5).
#[repr(C, packed)]
#[derive(Clone, Copy)]
pub struct UnallocSpDesc {
    /// `tag`.
    pub tag: DescTag,
    /// `seq_num`: MAX prevailing.
    pub seq_num: u32,
    /// `alloc_desc_num`.
    pub alloc_desc_num: u32,
    /// `alloc_desc`: the `ExtentAd`s follow (the module's deviations).
    pub alloc_desc: [ExtentAd; 0],
}

/// `struct logvolhdr`: Logical Volume Integrity Descriptor \[3/30.10\].
#[repr(C, packed)]
#[derive(Clone, Copy)]
pub struct Logvolhdr {
    /// `next_unique_id` (rest reserved).
    pub next_unique_id: u64,
}

/// `struct udf_logvol_info`.
#[repr(C, packed)]
#[derive(Clone, Copy)]
pub struct UdfLogvolInfo {
    /// `impl_id`.
    pub impl_id: Regid,
    /// `num_files`.
    pub num_files: u32,
    /// `num_directories`.
    pub num_directories: u32,
    /// `min_udf_readver`.
    pub min_udf_readver: u16,
    /// `min_udf_writever`.
    pub min_udf_writever: u16,
    /// `max_udf_writever`.
    pub max_udf_writever: u16,
}

/// The `_impl_use` union of `struct logvol_int_desc`.
#[repr(C, packed)]
#[derive(Clone, Copy)]
pub union LogvolIntImplUse {
    /// `logvolhdr`.
    pub logvolhdr: Logvolhdr,
    /// `reserved`.
    pub reserved: [i8; 32],
}

/// `struct logvol_int_desc`.
#[repr(C, packed)]
#[derive(Clone, Copy)]
pub struct LogvolIntDesc {
    /// `tag`.
    pub tag: DescTag,
    /// `time`.
    pub time: Timestamp,
    /// `integrity_type`.
    pub integrity_type: u32,
    /// `next_extent`.
    pub next_extent: ExtentAd,
    /// `_impl_use`.
    pub _impl_use: LogvolIntImplUse,
    /// `num_part`.
    pub num_part: u32,
    /// `l_iu`.
    pub l_iu: u32,
    /// `tables`: freespace table, sizetable, implementation use (the module's deviations).
    pub tables: [u32; 0],
}

impl LogvolIntDesc {
    /// `lvint_next_unique_id` (`_impl_use.logvolhdr.next_unique_id`).
    pub fn lvint_next_unique_id(&self) -> u64 {
        // SAFETY: both members are plain integers over the same bytes, any bit pattern is
        // valid.
        let h = unsafe { self._impl_use.logvolhdr };
        h.next_unique_id
    }
}

/// `struct fileset_desc`: File Set Descriptor \[4/14.1\].
#[repr(C, packed)]
#[derive(Clone, Copy)]
pub struct FilesetDesc {
    /// `tag`.
    pub tag: DescTag,
    /// `time`.
    pub time: Timestamp,
    /// `ichg_lvl`.
    pub ichg_lvl: u16,
    /// `max_ichg_lvl`.
    pub max_ichg_lvl: u16,
    /// `charset_list`.
    pub charset_list: u32,
    /// `max_charset_list`.
    pub max_charset_list: u32,
    /// `fileset_num`: key!
    pub fileset_num: u32,
    /// `fileset_desc_num`.
    pub fileset_desc_num: u32,
    /// `logvol_id_charset`.
    pub logvol_id_charset: Charspec,
    /// `logvol_id`: for recovery.
    pub logvol_id: [u8; 128],
    /// `fileset_charset`.
    pub fileset_charset: Charspec,
    /// `fileset_id`: Mountpoint!!
    pub fileset_id: [u8; 32],
    /// `copyright_file_id`.
    pub copyright_file_id: [u8; 32],
    /// `abstract_file_id`.
    pub abstract_file_id: [u8; 32],
    /// `rootdir_icb`: to rootdir; icb->virtual?
    pub rootdir_icb: LongAd,
    /// `domain_id`.
    pub domain_id: Regid,
    /// `next_ex`: to the next fileset_desc extent.
    pub next_ex: LongAd,
    /// `streamdir_icb`: streamdir; needed?
    pub streamdir_icb: LongAd,
    /// `reserved`.
    pub reserved: [u8; 32],
}

/// `struct fileid_desc`: File Identifier Descriptor \[4/14.4\].
#[repr(C, packed)]
#[derive(Clone, Copy)]
pub struct FileidDesc {
    /// `tag`.
    pub tag: DescTag,
    /// `file_version_num`.
    pub file_version_num: u16,
    /// `file_char`: the `UDF_FILE_CHAR_*`.
    pub file_char: u8,
    /// `l_fi`: length of file identifier area.
    pub l_fi: u8,
    /// `icb`.
    pub icb: LongAd,
    /// `l_iu`: length of implementation use area.
    pub l_iu: u16,
    /// `data`: the implementation use area, then the file identifier.
    pub data: [u8; 0],
}

/// `UDF_FID_SIZE`.
pub const UDF_FID_SIZE: usize = 38;
/// `UDF_FILE_CHAR_VIS`: Invisible.
pub const UDF_FILE_CHAR_VIS: u8 = 1 << 0;
/// `UDF_FILE_CHAR_DIR`: Directory.
pub const UDF_FILE_CHAR_DIR: u8 = 1 << 1;
/// `UDF_FILE_CHAR_DEL`: Deleted.
pub const UDF_FILE_CHAR_DEL: u8 = 1 << 2;
/// `UDF_FILE_CHAR_PAR`: Parent Directory.
pub const UDF_FILE_CHAR_PAR: u8 = 1 << 3;
/// `UDF_FILE_CHAR_META`: Stream metadata.
pub const UDF_FILE_CHAR_META: u8 = 1 << 4;

/// `struct extattrhdr_desc`: extended attributes \[4/14.10.1\].
#[repr(C, packed)]
#[derive(Clone, Copy)]
pub struct ExtattrhdrDesc {
    /// `tag`.
    pub tag: DescTag,
    /// `impl_attr_loc`: offsets within this descriptor.
    pub impl_attr_loc: u32,
    /// `appl_attr_loc`: ditto.
    pub appl_attr_loc: u32,
}

/// `UDF_IMPL_ATTR_LOC_NOT_PRESENT`.
pub const UDF_IMPL_ATTR_LOC_NOT_PRESENT: u32 = 0xffff_ffff;
/// `UDF_APPL_ATTR_LOC_NOT_PRESENT`.
pub const UDF_APPL_ATTR_LOC_NOT_PRESENT: u32 = 0xffff_ffff;

/// `struct extattr_entry`: extended attribute entry \[4/48.10.2\].
#[repr(C, packed)]
#[derive(Clone, Copy)]
pub struct ExtattrEntry {
    /// `type`.
    pub type_: u32,
    /// `subtype`.
    pub subtype: u8,
    /// `reserved`.
    pub reserved: [u8; 3],
    /// `a_l`.
    pub a_l: u32,
}

/// `struct impl_extattr_entry`: extended attribute entry; type 2048 \[4/48.10.8\].
#[repr(C, packed)]
#[derive(Clone, Copy)]
pub struct ImplExtattrEntry {
    /// `hdr`.
    pub hdr: ExtattrEntry,
    /// `iu_l`.
    pub iu_l: u32,
    /// `imp_id`.
    pub imp_id: Regid,
    /// `data`.
    pub data: [u8; 0],
}

/// `struct appl_extattr_entry`: extended attribute entry; type 65 536 \[4/48.10.9\].
#[repr(C, packed)]
#[derive(Clone, Copy)]
pub struct ApplExtattrEntry {
    /// `hdr`.
    pub hdr: ExtattrEntry,
    /// `au_l`.
    pub au_l: u32,
    /// `appl_id`.
    pub appl_id: Regid,
    /// `data`.
    pub data: [u8; 0],
}

/// `struct filetimes_extattr_entry`: File Times attribute entry; type 5 or type 6
/// \[4/48.10.5\], \[4/48.10.6\].
#[repr(C, packed)]
#[derive(Clone, Copy)]
pub struct FiletimesExtattrEntry {
    /// `hdr`.
    pub hdr: ExtattrEntry,
    /// `d_l`: length of times\[\] data following.
    pub d_l: u32,
    /// `existence`: bitmask.
    pub existence: u32,
    /// `times`: in order of ascending bits (the module's deviations).
    pub times: [Timestamp; 0],
}

/// `UDF_FILETIMES_ATTR_NO`.
pub const UDF_FILETIMES_ATTR_NO: u32 = 5;
/// `UDF_FILETIMES_FILE_CREATION`.
pub const UDF_FILETIMES_FILE_CREATION: u32 = 1;
/// `UDF_FILETIMES_FILE_DELETION`.
pub const UDF_FILETIMES_FILE_DELETION: u32 = 4;
/// `UDF_FILETIMES_FILE_EFFECTIVE`.
pub const UDF_FILETIMES_FILE_EFFECTIVE: u32 = 8;
/// `UDF_FILETIMES_FILE_BACKUPED`.
pub const UDF_FILETIMES_FILE_BACKUPED: u32 = 16;

/// `UDF_FILETIMES_ATTR_SIZE(no)`.
pub const fn udf_filetimes_attr_size(no: usize) -> usize {
    20 + no * size_of::<Timestamp>()
}

/// `struct device_extattr_entry`: Device Specification Extended Attribute \[4/4.10.7\].
#[repr(C, packed)]
#[derive(Clone, Copy)]
pub struct DeviceExtattrEntry {
    /// `hdr`.
    pub hdr: ExtattrEntry,
    /// `iu_l`: length of implementation use.
    pub iu_l: u32,
    /// `major`.
    pub major: u32,
    /// `minor`.
    pub minor: u32,
    /// `data`: UDF: if nonzero length, contain developer ID regid.
    pub data: [u8; 0],
}

/// `UDF_DEVICESPEC_ATTR_NO`.
pub const UDF_DEVICESPEC_ATTR_NO: u32 = 12;

/// `struct vatlvext_extattr_entry`: VAT LV extension Extended Attribute
/// \[UDF 3.3.4.5.1.3\] 1.50 errata.
#[repr(C, packed)]
#[derive(Clone, Copy)]
pub struct VatlvextExtattrEntry {
    /// `unique_id_chk`: needs to be copy of ICB's.
    pub unique_id_chk: u64,
    /// `num_files`.
    pub num_files: u32,
    /// `num_directories`.
    pub num_directories: u32,
    /// `logvol_id`: replaces logvol name.
    pub logvol_id: [u8; 128],
}

/// `struct file_entry`: File Entry \[4/14.9\].
#[repr(C, packed)]
#[derive(Clone, Copy)]
pub struct FileEntry {
    /// `tag`.
    pub tag: DescTag,
    /// `icbtag`.
    pub icbtag: IcbTag,
    /// `uid`.
    pub uid: u32,
    /// `gid`.
    pub gid: u32,
    /// `perm`.
    pub perm: u32,
    /// `link_cnt`.
    pub link_cnt: u16,
    /// `rec_format`.
    pub rec_format: u8,
    /// `rec_disp_attr`.
    pub rec_disp_attr: u8,
    /// `rec_len`.
    pub rec_len: u32,
    /// `inf_len`.
    pub inf_len: u64,
    /// `logblks_rec`.
    pub logblks_rec: u64,
    /// `atime`.
    pub atime: Timestamp,
    /// `mtime`.
    pub mtime: Timestamp,
    /// `attrtime`.
    pub attrtime: Timestamp,
    /// `ckpoint`.
    pub ckpoint: u32,
    /// `ex_attr_icb`.
    pub ex_attr_icb: LongAd,
    /// `imp_id`.
    pub imp_id: Regid,
    /// `unique_id`.
    pub unique_id: u64,
    /// `l_ea`: length of extended attribute area.
    pub l_ea: u32,
    /// `l_ad`: length of allocation descriptors.
    pub l_ad: u32,
    /// `data`: the extended attributes, then the allocation descriptors.
    pub data: [u8; 0],
}

/// `UDF_FENTRY_SIZE`.
pub const UDF_FENTRY_SIZE: usize = 176;
/// `UDF_FENTRY_PERM_USER_MASK`.
pub const UDF_FENTRY_PERM_USER_MASK: u32 = 0x07;
/// `UDF_FENTRY_PERM_GRP_MASK`.
pub const UDF_FENTRY_PERM_GRP_MASK: u32 = 0xE0;
/// `UDF_FENTRY_PERM_OWNER_MASK`.
pub const UDF_FENTRY_PERM_OWNER_MASK: u32 = 0x1C00;

/// `struct extfile_entry`: Extended File Entry \[4/48.17\].
#[repr(C, packed)]
#[derive(Clone, Copy)]
pub struct ExtfileEntry {
    /// `tag`.
    pub tag: DescTag,
    /// `icbtag`.
    pub icbtag: IcbTag,
    /// `uid`.
    pub uid: u32,
    /// `gid`.
    pub gid: u32,
    /// `perm`.
    pub perm: u32,
    /// `link_cnt`.
    pub link_cnt: u16,
    /// `rec_format`.
    pub rec_format: u8,
    /// `rec_disp_attr`.
    pub rec_disp_attr: u8,
    /// `rec_len`.
    pub rec_len: u32,
    /// `inf_len`.
    pub inf_len: u64,
    /// `obj_size`.
    pub obj_size: u64,
    /// `logblks_rec`.
    pub logblks_rec: u64,
    /// `atime`.
    pub atime: Timestamp,
    /// `mtime`.
    pub mtime: Timestamp,
    /// `ctime`.
    pub ctime: Timestamp,
    /// `attrtime`.
    pub attrtime: Timestamp,
    /// `ckpoint`.
    pub ckpoint: u32,
    /// `reserved1`.
    pub reserved1: u32,
    /// `ex_attr_icb`.
    pub ex_attr_icb: LongAd,
    /// `streamdir_icb`.
    pub streamdir_icb: LongAd,
    /// `imp_id`.
    pub imp_id: Regid,
    /// `unique_id`.
    pub unique_id: u64,
    /// `l_ea`: length of extended attribute area.
    pub l_ea: u32,
    /// `l_ad`: length of allocation descriptors.
    pub l_ad: u32,
    /// `data`: the extended attributes, then the allocation descriptors.
    pub data: [u8; 0],
}

/// `UDF_EXTFENTRY_SIZE`.
pub const UDF_EXTFENTRY_SIZE: usize = 216;

/// `struct indirect_entry`: indirect entry \[ecma 48.7\].
#[repr(C, packed)]
#[derive(Clone, Copy)]
pub struct IndirectEntry {
    /// `tag`.
    pub tag: DescTag,
    /// `icbtag`.
    pub icbtag: IcbTag,
    /// `indirect_icb`.
    pub indirect_icb: LongAd,
}

/// `struct alloc_ext_entry`: allocation extent descriptor \[ecma 48.5\].
#[repr(C, packed)]
#[derive(Clone, Copy)]
pub struct AllocExtEntry {
    /// `tag`.
    pub tag: DescTag,
    /// `prev_entry`.
    pub prev_entry: u32,
    /// `l_ad`.
    pub l_ad: u32,
    /// `data`.
    pub data: [u8; 0],
}

/// `union dscrptr`: any descriptor.
#[repr(C, packed)]
#[derive(Clone, Copy)]
pub union Dscrptr {
    /// `tag`.
    pub tag: DescTag,
    /// `avdp`.
    pub avdp: AnchorVdp,
    /// `vdp`.
    pub vdp: VolDescPtr,
    /// `pvd`.
    pub pvd: PriVolDesc,
    /// `lvd`.
    pub lvd: LogvolDesc,
    /// `usd`.
    pub usd: UnallocSpDesc,
    /// `lvid`.
    pub lvid: LogvolIntDesc,
    /// `ivd`.
    pub ivd: ImpvolDesc,
    /// `pd`.
    pub pd: PartDesc,
    /// `fsd`.
    pub fsd: FilesetDesc,
    /// `fid`.
    pub fid: FileidDesc,
    /// `fe`.
    pub fe: FileEntry,
    /// `efe`.
    pub efe: ExtfileEntry,
    /// `eahd`.
    pub eahd: ExtattrhdrDesc,
    /// `inde`.
    pub inde: IndirectEntry,
    /// `aee`.
    pub aee: AllocExtEntry,
    /// `spt`.
    pub spt: UdfSparingTable,
    /// `sbd`.
    pub sbd: SpaceBitmapDesc,
    /// `sed`.
    pub sed: SpaceEntryDesc,
}

/// An allocation descriptor whose first member is its length (`short_ad`, `long_ad`), as
/// `GETICBLEN` reads it.
pub trait IcbLen {
    /// The `len` member, as stored.
    fn icb_len(&self) -> u32;
}

impl IcbLen for ShortAd {
    fn icb_len(&self) -> u32 {
        self.len
    }
}

impl IcbLen for LongAd {
    fn icb_len(&self) -> u32 {
        self.len
    }
}

/// `GETICBLEN(ad_type, icb)`: the length of an allocation descriptor.
pub fn geticblen<T: IcbLen>(icb: &T) -> u32 {
    crate::sys::endian::letoh32(icb.icb_len())
}

packed!(
    VrsDesc,
    DescTag,
    LbAddr,
    ExtentAd,
    ShortAd,
    UdfAdimpUse,
    LongAdImpl,
    LongAd,
    ExtAd,
    Icb,
    Charspec,
    Pathcomp,
    Timestamp,
    Regid,
    IcbTag,
    AnchorVdp,
    VolDescPtr,
    PriVolDesc,
    UdfLvInfo,
    ImpvolImplUse,
    ImpvolDesc,
    LvdUse,
    LogvolDesc,
    PartMap1,
    PartMap2,
    PartMapVirt,
    PartMapSpare,
    PartMapMeta,
    UdfPmap,
    SpareMapEntry,
    UdfSparingTable,
    UdfOldvatTail,
    UdfVat,
    SpaceBitmapDesc,
    SpaceEntryDesc,
    PartHdrDesc,
    PartDescImplUse,
    PartDesc,
    UnallocSpDesc,
    Logvolhdr,
    UdfLogvolInfo,
    LogvolIntImplUse,
    LogvolIntDesc,
    FilesetDesc,
    FileidDesc,
    ExtattrhdrDesc,
    ExtattrEntry,
    ImplExtattrEntry,
    ApplExtattrEntry,
    FiletimesExtattrEntry,
    DeviceExtattrEntry,
    VatlvextExtattrEntry,
    FileEntry,
    ExtfileEntry,
    IndirectEntry,
    AllocExtEntry,
    Dscrptr,
);

// The C's sizes (the fixed parts, see the module's deviations) and the offsets the code
// relies on.
const _: () = {
    use core::mem::offset_of;
    assert!(size_of::<VrsDesc>() == 2048);
    assert!(size_of::<DescTag>() == UDF_DESC_TAG_LENGTH);
    assert!(size_of::<LbAddr>() == 6);
    assert!(size_of::<ExtentAd>() == 8);
    assert!(size_of::<ShortAd>() == 8);
    assert!(size_of::<LongAd>() == 16);
    assert!(size_of::<ExtAd>() == 20);
    assert!(size_of::<Icb>() == 20);
    assert!(size_of::<Charspec>() == 64);
    assert!(size_of::<Pathcomp>() == 260);
    assert!(size_of::<Timestamp>() == UDF_TIMESTAMP_SIZE);
    assert!(size_of::<Regid>() == 32);
    assert!(size_of::<IcbTag>() == 20);
    assert!(size_of::<AnchorVdp>() == 32);
    assert!(size_of::<VolDescPtr>() == 28);
    assert!(size_of::<PriVolDesc>() == 512);
    assert!(size_of::<UdfLvInfo>() == 460);
    assert!(size_of::<ImpvolDesc>() == 512);
    assert!(size_of::<LogvolDesc>() == 440);
    assert!(size_of::<PartMap1>() == 6);
    assert!(size_of::<PartMap2>() == UDF_PMAP_SIZE);
    assert!(size_of::<PartMapVirt>() == UDF_PMAP_SIZE);
    assert!(size_of::<PartMapSpare>() == 48);
    assert!(size_of::<PartMapMeta>() == UDF_PMAP_SIZE);
    assert!(size_of::<UdfPmap>() == UDF_PMAP_SIZE);
    assert!(size_of::<UdfSparingTable>() == 56);
    assert!(size_of::<UdfVat>() == 152);
    assert!(size_of::<PartHdrDesc>() == 128);
    assert!(size_of::<PartDesc>() == 512);
    assert!(size_of::<LogvolIntDesc>() == 80);
    assert!(size_of::<FilesetDesc>() == 512);
    assert!(size_of::<FileidDesc>() == UDF_FID_SIZE);
    assert!(size_of::<FileEntry>() == UDF_FENTRY_SIZE);
    assert!(size_of::<ExtfileEntry>() == UDF_EXTFENTRY_SIZE);
    assert!(size_of::<IndirectEntry>() == 52);
    assert!(size_of::<Dscrptr>() == 512);
    assert!(offset_of!(PartDesc, start_loc) == 188);
    assert!(offset_of!(FilesetDesc, rootdir_icb) == 400);
    assert!(offset_of!(FileEntry, inf_len) == 56);
    assert!(offset_of!(ExtfileEntry, inf_len) == 56);
    assert!(offset_of!(FileEntry, l_ea) == 168);
    assert!(offset_of!(ExtfileEntry, l_ea) == 208);
};

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn views_read_little_endian_members_in_place() {
        let mut b = [0u8; 40];
        // A short_ad at offset 3 (misaligned on purpose): len 0x4000_0123, lb_num 7.
        b[3..7].copy_from_slice(&0x4000_0123u32.to_le_bytes());
        b[7..11].copy_from_slice(&7u32.to_le_bytes());
        let ad = ShortAd::at(&b, 3).unwrap();
        assert_eq!(geticblen(ad), 0x4000_0123);
        assert_eq!(udf_ext_len(geticblen(ad)), 0x123);
        assert_eq!(udf_ext_flags(geticblen(ad)), UDF_EXT_FREED);
        assert_eq!(crate::sys::endian::letoh32(ad.lb_num), 7);
        assert!(ShortAd::at(&b, 33).is_none());
        assert!(LongAd::at(&b, usize::MAX).is_none());
    }

    #[test]
    fn a_tag_is_its_sixteen_bytes() {
        let b: [u8; 16] = core::array::from_fn(|i| i as u8);
        let t = DescTag::at(&b, 0).unwrap();
        assert_eq!(t.as_bytes(), &b);
        assert_eq!({ t.id }, u16::from_le_bytes([0, 1]));
        assert_eq!(t.cksum, 4);
        assert_eq!({ t.tag_loc }, u32::from_le_bytes([12, 13, 14, 15]));
    }
}
