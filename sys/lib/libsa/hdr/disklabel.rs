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
//! `<sys/disklabel.h>` (and amd64's `<machine/disklabel.h>`) for libsa and the boot programs:
//! `struct disklabel`, the `DL_*` accessors, and the MBR and GPT layouts the boot loaders read
//! to find the OpenBSD partition.

use super::param::DEV_BSIZE;
use super::uuid::Uuid;

/// `MAXPARTITIONS` of amd64 and arm64 (`<machine/disklabel.h>`).
pub const MAXPARTITIONS: usize = 16;
/// `MAXPARTITIONSUNIT`: the partition slots of a label (52 in use at most).
pub const MAXPARTITIONSUNIT: usize = 64;
/// `MAXPARTITIONS16`: the partitions of the "skinny" label the boot loaders read.
pub const MAXPARTITIONS16: usize = 16;
/// `RAW_PART`: the `c` partition, the whole disk.
pub const RAW_PART: usize = 2;
/// `DISKMAGIC`: the disk label magic number.
pub const DISKMAGIC: u32 = 0x8256_4557;

/// `DTYPE_SCSI`.
pub const DTYPE_SCSI: u16 = 4;
/// `DTYPE_ESDI`.
pub const DTYPE_ESDI: u16 = 5;
/// `DTYPE_ST506`.
pub const DTYPE_ST506: u16 = 6;
/// `DTYPE_ATAPI`.
pub const DTYPE_ATAPI: u16 = 13;

/// `FS_UNUSED`: unused partition.
pub const FS_UNUSED: u8 = 0;
/// `FS_SWAP`: swap partition.
pub const FS_SWAP: u8 = 1;
/// `FS_BSDFFS`: 4.2BSD fast file system.
pub const FS_BSDFFS: u8 = 7;

/// `GPTSECTOR`: the sector of the GPT header.
pub const GPTSECTOR: u64 = 1;
/// `GPTSIGNATURE`: "EFI PART" as a little-endian 64-bit number.
pub const GPTSIGNATURE: u64 = 0x5452_4150_2049_4645;
/// `GPTREVISION`: GPT header version 1.0.
pub const GPTREVISION: u32 = 0x10000;
/// `GPTMINHDRSIZE`.
pub const GPTMINHDRSIZE: u32 = 92;
/// `GPT_UUID_OPENBSD`: the OpenBSD partition type, as the big-endian bytes the C spells.
pub const GPT_UUID_OPENBSD: [u8; 16] = [
    0x82, 0x4c, 0xc7, 0xa0, 0x36, 0xa8, 0x11, 0xe3, 0x89, 0x0a, 0x95, 0x25, 0x19, 0xad, 0x3f, 0x61,
];

/// `DOS_LABELSECTOR`: the label's sector inside the OpenBSD MBR partition.
pub const DOS_LABELSECTOR: u32 = 1;
/// `DOSBBSECTOR`: the MBR's sector.
pub const DOSBBSECTOR: u32 = 0;
/// `DOSPARTOFF`: the partition table's offset in the MBR.
pub const DOSPARTOFF: usize = 446;
/// `NDOSPART`: partitions in an MBR.
pub const NDOSPART: usize = 4;
/// `DOS_MAXEBR`: extended boot records followed at most.
pub const DOS_MAXEBR: i32 = 256;
/// `DOSPTYP_UNUSED`.
pub const DOSPTYP_UNUSED: u8 = 0x00;
/// `DOSPTYP_EXTEND`.
pub const DOSPTYP_EXTEND: u8 = 0x05;
/// `DOSPTYP_EXTENDL`.
pub const DOSPTYP_EXTENDL: u8 = 0x0f;
/// `DOSPTYP_OPENBSD`.
pub const DOSPTYP_OPENBSD: u8 = 0xa6;
/// `DOSPTYP_EFI`: the GPT protective partition.
pub const DOSPTYP_EFI: u8 = 0xee;

/// `struct partition`: one entry of the label's partition table.
#[repr(C)]
#[derive(Clone, Copy, Default, Debug, PartialEq, Eq)]
pub struct Partition {
    /// `p_size`: number of sectors (low part).
    pub p_size: u32,
    /// `p_offset`: starting sector (low part).
    pub p_offset: u32,
    /// `p_offseth`: starting sector (high part).
    pub p_offseth: u16,
    /// `p_sizeh`: number of sectors (high part).
    pub p_sizeh: u16,
    /// `p_fstype`: file system type.
    pub p_fstype: u8,
    /// `p_fragblock`: encoded file system frag/block.
    pub p_fragblock: u8,
    /// `p_cpg`: UFS cylinders per group.
    pub p_cpg: u16,
}

/// `struct disklabel`.
#[repr(C)]
#[derive(Clone, Copy, Debug)]
pub struct Disklabel {
    /// `d_magic`.
    pub d_magic: u32,
    /// `d_type`: drive type.
    pub d_type: u16,
    /// `d_subtype`.
    pub d_subtype: u16,
    /// `d_typename`: type name, e.g. "eagle".
    pub d_typename: [u8; 16],
    /// `d_packname`: pack identifier.
    pub d_packname: [u8; 16],
    /// `d_secsize`: bytes per sector.
    pub d_secsize: u32,
    /// `d_nsectors`: data sectors per track.
    pub d_nsectors: u32,
    /// `d_ntracks`: tracks per cylinder.
    pub d_ntracks: u32,
    /// `d_ncylinders`: data cylinders per unit.
    pub d_ncylinders: u32,
    /// `d_secpercyl`: data sectors per cylinder.
    pub d_secpercyl: u32,
    /// `d_secperunit`: data sectors (low part).
    pub d_secperunit: u32,
    /// `d_uid`: unique label identifier (the DUID).
    pub d_uid: [u8; 8],
    /// `d_acylinders`: alternate cylinders per unit.
    pub d_acylinders: u32,
    /// `d_bstarth`: start of useable region (high part).
    pub d_bstarth: u16,
    /// `d_bendh`: size of useable region (high part).
    pub d_bendh: u16,
    /// `d_bstart`: start of useable region.
    pub d_bstart: u32,
    /// `d_bend`: end of useable region.
    pub d_bend: u32,
    /// `d_flags`.
    pub d_flags: u32,
    /// `d_spare4`.
    pub d_spare4: [u32; 5],
    /// `d_secperunith`: data sectors (high part).
    pub d_secperunith: u16,
    /// `d_version`: 1 = 48-bit addressing.
    pub d_version: u16,
    /// `d_spare`.
    pub d_spare: [u32; 4],
    /// `d_magic2`.
    pub d_magic2: u32,
    /// `d_checksum`: xor of the data, partitions included.
    pub d_checksum: u16,
    /// `d_npartitions`: number of partitions in the table.
    pub d_npartitions: u16,
    /// `d_spare2`.
    pub d_spare2: u32,
    /// `d_spare3`.
    pub d_spare3: u32,
    /// `d_partitions`.
    pub d_partitions: [Partition; MAXPARTITIONSUNIT],
}

impl Disklabel {
    /// A label of zeroes (`bzero`).
    pub const fn zeroed() -> Self {
        Self {
            d_magic: 0,
            d_type: 0,
            d_subtype: 0,
            d_typename: [0; 16],
            d_packname: [0; 16],
            d_secsize: 0,
            d_nsectors: 0,
            d_ntracks: 0,
            d_ncylinders: 0,
            d_secpercyl: 0,
            d_secperunit: 0,
            d_uid: [0; 8],
            d_acylinders: 0,
            d_bstarth: 0,
            d_bendh: 0,
            d_bstart: 0,
            d_bend: 0,
            d_flags: 0,
            d_spare4: [0; 5],
            d_secperunith: 0,
            d_version: 0,
            d_spare: [0; 4],
            d_magic2: 0,
            d_checksum: 0,
            d_npartitions: 0,
            d_spare2: 0,
            d_spare3: 0,
            d_partitions: [Partition {
                p_size: 0,
                p_offset: 0,
                p_offseth: 0,
                p_sizeh: 0,
                p_fstype: 0,
                p_fragblock: 0,
                p_cpg: 0,
            }; MAXPARTITIONSUNIT],
        }
    }

    /// The label's bytes.
    pub fn as_bytes(&self) -> &[u8; core::mem::size_of::<Disklabel>()] {
        // SAFETY: `Disklabel` is `#[repr(C)]`, made of integers and arrays of integers whose
        // sizes add up with no padding (the compile-time check below pins the size), so every
        // byte is initialised and the array has the struct's size and an alignment of 1.
        unsafe { &*(self as *const Self).cast() }
    }

    /// The label's bytes, writable.
    pub fn as_bytes_mut(&mut self) -> &mut [u8; core::mem::size_of::<Disklabel>()] {
        // SAFETY: as for `as_bytes`; any byte pattern is a valid `Disklabel` (integers only).
        unsafe { &mut *(self as *mut Self).cast() }
    }
}

/// `DL_GETPSIZE(p)`.
pub const fn dl_getpsize(p: &Partition) -> u64 {
    ((p.p_sizeh as u64) << 32) + p.p_size as u64
}

/// `DL_SETPSIZE(p, n)`.
pub const fn dl_setpsize(p: &mut Partition, n: u64) {
    p.p_sizeh = (n >> 32) as u16;
    p.p_size = n as u32;
}

/// `DL_GETPOFFSET(p)`.
pub const fn dl_getpoffset(p: &Partition) -> u64 {
    ((p.p_offseth as u64) << 32) + p.p_offset as u64
}

/// `DL_SETPOFFSET(p, n)`.
pub const fn dl_setpoffset(p: &mut Partition, n: u64) {
    p.p_offseth = (n >> 32) as u16;
    p.p_offset = n as u32;
}

/// `DL_GETDSIZE(d)`.
pub const fn dl_getdsize(d: &Disklabel) -> u64 {
    ((d.d_secperunith as u64) << 32) + d.d_secperunit as u64
}

/// `DL_SETDSIZE(d, n)`.
pub const fn dl_setdsize(d: &mut Disklabel, n: u64) {
    d.d_secperunith = (n >> 32) as u16;
    d.d_secperunit = n as u32;
}

/// `DL_BLKSPERSEC(d)`: [`DEV_BSIZE`] blocks per sector.
pub const fn dl_blkspersec(d: &Disklabel) -> u64 {
    d.d_secsize as u64 / DEV_BSIZE as u64
}

/// `DL_SECTOBLK(d, n)`: sectors to [`DEV_BSIZE`] blocks.
pub const fn dl_sectoblk(d: &Disklabel, n: u64) -> u64 {
    n * dl_blkspersec(d)
}

/// `DL_PARTNUM2NAME(partnum)`: the letter of a partition, or `None` (the C's -1).
pub const fn dl_partnum2name(partnum: usize) -> Option<u8> {
    if partnum >= MAXPARTITIONS {
        None
    } else if partnum <= (b'z' - b'a') as usize {
        Some(b'a' + partnum as u8)
    } else if partnum - 26 <= (b'Z' - b'A') as usize {
        Some(b'A' + (partnum - 26) as u8)
    } else {
        None
    }
}

/// `DL_PARTNAME2NUM(partname)`: the number of a partition letter, or `None` (the C's -1).
pub const fn dl_partname2num(partname: u8) -> Option<usize> {
    let partnum = match partname {
        b'a'..=b'z' => (partname - b'a') as usize,
        b'A'..=b'Z' => (partname - b'A') as usize + 26,
        _ => return None,
    };
    if partnum >= MAXPARTITIONS {
        None
    } else {
        Some(partnum)
    }
}

/// `struct gpt_header` (little-endian on disk).
#[repr(C)]
#[derive(Clone, Copy, Default, Debug)]
pub struct GptHeader {
    /// `gh_sig`: "EFI PART".
    pub gh_sig: u64,
    /// `gh_rev`.
    pub gh_rev: u32,
    /// `gh_size`.
    pub gh_size: u32,
    /// `gh_csum`: CRC32 with this field as 0.
    pub gh_csum: u32,
    /// `gh_rsvd`.
    pub gh_rsvd: u32,
    /// `gh_lba_self`.
    pub gh_lba_self: u64,
    /// `gh_lba_alt`.
    pub gh_lba_alt: u64,
    /// `gh_lba_start`.
    pub gh_lba_start: u64,
    /// `gh_lba_end`.
    pub gh_lba_end: u64,
    /// `gh_guid`.
    pub gh_guid: Uuid,
    /// `gh_part_lba`.
    pub gh_part_lba: u64,
    /// `gh_part_num`.
    pub gh_part_num: u32,
    /// `gh_part_size`.
    pub gh_part_size: u32,
    /// `gh_part_csum`.
    pub gh_part_csum: u32,
}

/// `struct gpt_partition`.
#[repr(C)]
#[derive(Clone, Copy, Debug)]
pub struct GptPartition {
    /// `gp_type`: partition type GUID.
    pub gp_type: Uuid,
    /// `gp_guid`: unique partition GUID.
    pub gp_guid: Uuid,
    /// `gp_lba_start`.
    pub gp_lba_start: u64,
    /// `gp_lba_end`.
    pub gp_lba_end: u64,
    /// `gp_attrs`.
    pub gp_attrs: u64,
    /// `gp_name`: UTF-16LE.
    pub gp_name: [u16; 36],
}

/// `struct dos_partition`.
#[repr(C, packed)]
#[derive(Clone, Copy, Default, Debug)]
pub struct DosPartition {
    /// `dp_flag`: bootstrap flags.
    pub dp_flag: u8,
    /// `dp_shd`: starting head.
    pub dp_shd: u8,
    /// `dp_ssect`: starting sector.
    pub dp_ssect: u8,
    /// `dp_scyl`: starting cylinder.
    pub dp_scyl: u8,
    /// `dp_typ`: partition type.
    pub dp_typ: u8,
    /// `dp_ehd`: end head.
    pub dp_ehd: u8,
    /// `dp_esect`: end sector.
    pub dp_esect: u8,
    /// `dp_ecyl`: end cylinder.
    pub dp_ecyl: u8,
    /// `dp_start`: absolute starting sector number.
    pub dp_start: u32,
    /// `dp_size`: partition size in sectors.
    pub dp_size: u32,
}

impl DosPartition {
    /// The four entries of the partition table of an MBR or EBR sector (`DOSPARTOFF`).
    pub fn table(sector: &[u8]) -> [Self; NDOSPART] {
        let mut t = [Self::default(); NDOSPART];
        for (i, e) in t.iter_mut().enumerate() {
            let b = &sector[DOSPARTOFF + 16 * i..DOSPARTOFF + 16 * (i + 1)];
            let le = |o: usize| u32::from_le_bytes([b[o], b[o + 1], b[o + 2], b[o + 3]]);
            *e = Self {
                dp_flag: b[0],
                dp_shd: b[1],
                dp_ssect: b[2],
                dp_scyl: b[3],
                dp_typ: b[4],
                dp_ehd: b[5],
                dp_esect: b[6],
                dp_ecyl: b[7],
                dp_start: le(8),
                dp_size: le(12),
            };
        }
        t
    }
}

const _: () = assert!(core::mem::size_of::<Partition>() == 16);
const _: () = assert!(core::mem::size_of::<Disklabel>() == 148 + 16 * MAXPARTITIONSUNIT);
const _: () = assert!(core::mem::size_of::<DosPartition>() == 16);
const _: () = assert!(core::mem::size_of::<GptPartition>() == 128);
/* </CODE> */
