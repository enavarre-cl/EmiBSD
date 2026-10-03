/*	$OpenBSD: disklabel.h,v 1.94 2025/11/13 20:59:14 deraadt Exp $	*/
/*	$NetBSD: disklabel.h,v 1.41 1996/05/10 23:07:37 mark Exp $	*/
/* <LICENSES> */
/*
 * Copyright (c) 1987, 1988, 1993
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
 *	@(#)disklabel.h	8.2 (Berkeley) 7/10/94
 */
/* </LICENSES> */

//! `<sys/disklabel.h>`: the disk label (`struct disklabel`, `struct partition`), the
//! `dev_t` to disk unit/partition split, the `DL_*` accessors of its split 48-bit fields,
//! the drive and file system types, `struct partinfo` and the DOS (MBR) and GPT partition
//! table constants.
//!
//! Upstream: sys/sys/disklabel.h @ 3ce1f3f79392
//!
//! The machine-dependent half (`<machine/disklabel.h>`: `LABELSECTOR`, `LABELOFFSET`,
//! `MAXPARTITIONS`) comes from `machine::disklabel` and is re-exported here, as C includes
//! it from this header. The functions the header declares live where the C defines them:
//! `kern/subr_disk.rs` (`dkcksum`, `initdisklabel`, `checkdisklabel`, `setdisklabel`,
//! `bounds_check_with_label`, `readdisksector`, `readdoslabel`, `diskerr`) and the machine's
//! `disksubr.rs` (`readdisklabel`, `writedisklabel`, through `machine::disklabel`).
//!
//! ## Deviations
//! - The function-like macros are `const fn`s in lower case (`DISKUNIT(dev)` is
//!   [`diskunit`], `DL_GETPSIZE(p)` is [`dl_getpsize`], `DL_SETPSIZE(p, n)` is
//!   [`dl_setpsize`]), as `docs/C_TO_RUST.md` says for capitalised macros.
//! - `DL_PARTNUM2NAME` and `DL_PARTNAME2NUM` return `Option` where the C returns `-1`.
//! - `DISKLABELV1_FFS_FRAGBLOCK` uses `trailing_zeros() + 1` for libkern's `ffs()` (not
//!   ported); both give the 1-based index of the lowest set bit of a non-zero value.
//! - [`Disklabel`] is `#[repr(C)]` with the C's layout (a compile-time check pins its size),
//!   so a label read from a disk sector is the sector's bytes ([`Disklabel::from_bytes`],
//!   [`Disklabel::as_bytes`]), as the C's cast of `b_data` is.
//! - `struct partinfo` holds raw pointers, as in C: `DIOCGPART` hands the in-core label and
//!   partition to a file system (`ffs_mountfs`); [`Partinfo::store`] and [`Partinfo::load`]
//!   move it through an `ioctl` data buffer.
//! - `struct gpt_header` and `struct gpt_partition` are read from a sector's bytes field by
//!   field ([`GptHeader::from_bytes`], [`GptPartition::from_bytes`]), the C's `memcpy` into
//!   the struct; their fields stay little-endian as on disk (the C's `letoh*` at each use).
//! - [`DosPartition::table`] is the C's `memcpy(dp, dosbb + DOSPARTOFF, sizeof(dp))` from a
//!   sector buffer.
//! - `getdiskbyname` is the C library's (`!_KERNEL`); `_PATH_DISKTAB`/`DISKTAB` are kept.

use crate::machine::{Machine, MachineDisklabel};
use crate::sys::param::DEV_BSIZE;
use crate::sys::types::{Dev, major, makedev, minor};
use crate::sys::uuid::Uuid;

/// `_PATH_DISKTAB`: disk description table, see disktab(5).
pub const _PATH_DISKTAB: &str = "/etc/disktab";
/// `DISKTAB` (deprecated).
pub const DISKTAB: &str = "/etc/disktab";

/// `LABELSECTOR` (`<machine/disklabel.h>`): sector containing label.
pub const LABELSECTOR: u64 = <Machine as MachineDisklabel>::LABELSECTOR;
/// `LABELOFFSET` (`<machine/disklabel.h>`): offset of label in sector.
pub const LABELOFFSET: usize = <Machine as MachineDisklabel>::LABELOFFSET;
/// `MAXPARTITIONS` (`<machine/disklabel.h>`): number of partitions.
pub const MAXPARTITIONS: usize = <Machine as MachineDisklabel>::MAXPARTITIONS;

/// `MAXPARTITIONSUNIT`: the `dev_t` split has 64 partitions, but only 52 are visible and
/// easily useable in userland (a-z and A-Z). The MD variable `MAXPARTITIONS` remains 52 (or
/// less).
pub const MAXPARTITIONSUNIT: u32 = 64;

/// `MAXPARTITIONS16`: various situations still have structures limited to 16 partitions.
pub const MAXPARTITIONS16: usize = 16;

/// `DISKUNIT(dev)`: the disk unit of a device number.
pub const fn diskunit(dev: Dev) -> u32 {
    minor(dev) / MAXPARTITIONSUNIT
}

/// `DISKPART(dev)`: the partition of a device number.
pub const fn diskpart(dev: Dev) -> u32 {
    minor(dev) % MAXPARTITIONSUNIT
}

/// `RAW_PART`: the 'c' partition.
pub const RAW_PART: u32 = 2;

/// `DISKMINOR(unit, part)`.
pub const fn diskminor(unit: u32, part: u32) -> u32 {
    unit * MAXPARTITIONSUNIT + part
}

/// `MAKEDISKDEV(maj, unit, part)`.
pub const fn makediskdev(maj: u32, unit: u32, part: u32) -> Dev {
    makedev(maj, diskminor(unit, part))
}

/// `DISKLABELDEV(dev)`: the raw partition of the disk `dev` is on.
pub const fn disklabeldev(dev: Dev) -> Dev {
    makediskdev(major(dev), diskunit(dev), RAW_PART)
}

/// `DISKMAGIC`: the disk magic number.
pub const DISKMAGIC: u32 = 0x8256_4557;

/// `MAXDISKSIZE`: 47 bits of reach.
pub const MAXDISKSIZE: u64 = 0x7fff_ffff_ffff;

/// `NDDATA`.
pub const NDDATA: usize = 5;
/// `NSPARE`.
pub const NSPARE: usize = 4;

/// `struct partition`: one entry of the partition table.
#[repr(C)]
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Partition {
    /// `p_size`: number of sectors (low part).
    pub p_size: u32,
    /// `p_offset`: starting sector (low part).
    pub p_offset: u32,
    /// `p_offseth`: starting sector (high part).
    pub p_offseth: u16,
    /// `p_sizeh`: number of sectors (high part).
    pub p_sizeh: u16,
    /// `p_fstype`: filesystem type, see below.
    pub p_fstype: u8,
    /// `p_fragblock`: encoded filesystem frag/block.
    pub p_fragblock: u8,
    /// `p_cpg`: UFS: FS cylinders per group.
    pub p_cpg: u16,
}

impl Partition {
    /// An unused partition (all zero).
    pub const fn new() -> Self {
        Self {
            p_size: 0,
            p_offset: 0,
            p_offseth: 0,
            p_sizeh: 0,
            p_fstype: 0,
            p_fragblock: 0,
            p_cpg: 0,
        }
    }
}

/// `struct disklabel`: each disk has a label which includes information about the hardware
/// disk geometry, filesystem partitions, and drive specific information. The location of the
/// label, as well as the number of partitions the label can describe and the number of the
/// "whole disk" (raw) partition are machine dependent.
#[repr(C)]
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Disklabel {
    /// `d_magic`: the magic number.
    pub d_magic: u32,
    /// `d_type`: drive type.
    pub d_type: u16,
    /// `d_subtype`: controller/d_type specific.
    pub d_subtype: u16,
    /// `d_typename`: type name, e.g. "eagle".
    pub d_typename: [u8; 16],
    /// `d_packname`: pack identifier.
    pub d_packname: [u8; 16],

    // disk geometry:
    /// `d_secsize`: # of bytes per sector.
    pub d_secsize: u32,
    /// `d_nsectors`: # of data sectors per track.
    pub d_nsectors: u32,
    /// `d_ntracks`: # of tracks per cylinder.
    pub d_ntracks: u32,
    /// `d_ncylinders`: # of data cylinders per unit.
    pub d_ncylinders: u32,
    /// `d_secpercyl`: # of data sectors per cylinder.
    pub d_secpercyl: u32,
    /// `d_secperunit`: # of data sectors (low part).
    pub d_secperunit: u32,

    /// `d_uid`: Unique label identifier.
    pub d_uid: [u8; 8],

    /// `d_acylinders`: # of alt. cylinders per unit. Alternate cylinders include
    /// maintenance, replacement, configuration description areas, etc.
    pub d_acylinders: u32,

    // hardware characteristics:
    /// `d_bstarth`: start of useable region (high part).
    pub d_bstarth: u16,
    /// `d_bendh`: size of useable region (high part).
    pub d_bendh: u16,
    /// `d_bstart`: start of useable region.
    pub d_bstart: u32,
    /// `d_bend`: end of useable region.
    pub d_bend: u32,
    /// `d_flags`: generic flags.
    pub d_flags: u32,
    /// `d_spare4`.
    pub d_spare4: [u32; NDDATA],
    /// `d_secperunith`: # of data sectors (high part).
    pub d_secperunith: u16,
    /// `d_version`: version # (1=48 bit addressing).
    pub d_version: u16,
    /// `d_spare`: reserved for future use.
    pub d_spare: [u32; NSPARE],
    /// `d_magic2`: the magic number (again).
    pub d_magic2: u32,
    /// `d_checksum`: xor of data incl. partitions.
    pub d_checksum: u16,

    // filesystem and partition information:
    /// `d_npartitions`: number of partitions in following.
    pub d_npartitions: u16,
    /// `d_spare2`.
    pub d_spare2: u32,
    /// `d_spare3`.
    pub d_spare3: u32,
    /// `d_partitions`: the partition table; maximum 52 in use.
    pub d_partitions: [Partition; MAXPARTITIONSUNIT as usize],
}

/// `sizeof(struct disklabel)`.
pub const DISKLABEL_SIZE: usize = core::mem::size_of::<Disklabel>();

/// The offset of `d_partitions` in the label: the header `dkcksum` always covers.
pub const DISKLABEL_PARTITIONS_OFFSET: usize = core::mem::offset_of!(Disklabel, d_partitions);

impl Disklabel {
    /// An all-zero label (`bzero(lp, sizeof(struct disklabel))`).
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
            d_spare4: [0; NDDATA],
            d_secperunith: 0,
            d_version: 0,
            d_spare: [0; NSPARE],
            d_magic2: 0,
            d_checksum: 0,
            d_npartitions: 0,
            d_spare2: 0,
            d_spare3: 0,
            d_partitions: [Partition::new(); MAXPARTITIONSUNIT as usize],
        }
    }

    /// The label's bytes, in memory order: what the C sees through `(u_int16_t *)lp` or
    /// writes to a sector with `*dlp = *lp`.
    pub fn as_bytes(&self) -> &[u8; DISKLABEL_SIZE] {
        // SAFETY: `Disklabel` is `#[repr(C)]` of integers and arrays of integers with no
        // padding (the size checks at the bottom of the file), so all its bytes are
        // initialised; the array borrows the label for the same lifetime.
        unsafe { &*core::ptr::from_ref(self).cast::<[u8; DISKLABEL_SIZE]>() }
    }

    /// The label's bytes, writable: every bit pattern is a valid `Disklabel`.
    pub fn as_bytes_mut(&mut self) -> &mut [u8; DISKLABEL_SIZE] {
        // SAFETY: as for `as_bytes`; any bytes written make a valid label (integers only).
        unsafe { &mut *core::ptr::from_mut(self).cast::<[u8; DISKLABEL_SIZE]>() }
    }

    /// The label at the start of `bytes`, as the C casts a sector buffer to
    /// `struct disklabel *`; bytes past the end of `bytes` read as zero.
    pub fn from_bytes(bytes: &[u8]) -> Self {
        let mut lp = Self::zeroed();
        let n = bytes.len().min(DISKLABEL_SIZE);
        lp.as_bytes_mut()[..n].copy_from_slice(&bytes[..n]);
        lp
    }

    /// The `u_int16_t` words from the start of the label to `&d_partitions[npartitions]`,
    /// the range `dkcksum` XORs (`npartitions` is clamped to the table).
    pub fn cksum_words(&self, npartitions: usize) -> impl Iterator<Item = u16> + '_ {
        let end = DISKLABEL_PARTITIONS_OFFSET
            + npartitions.min(MAXPARTITIONSUNIT as usize) * core::mem::size_of::<Partition>();
        self.as_bytes()[..end]
            .as_chunks::<2>()
            .0
            .iter()
            .map(|&w| u16::from_ne_bytes(w))
    }
}

impl Default for Disklabel {
    fn default() -> Self {
        Self::zeroed()
    }
}

/// `DISKLABELV1_FFS_FRAGBLOCK(fsize, frag)`: the encoded frag/block of a partition.
pub const fn disklabelv1_ffs_fragblock(fsize: u32, frag: u32) -> u8 {
    if fsize * frag == 0 {
        0
    } else {
        // ffs(x) is the 1-based index of the lowest set bit.
        let ffs_block = (fsize * frag).trailing_zeros() + 1;
        let ffs_frag = frag.trailing_zeros() + 1;
        (((ffs_block - 13) << 3) | ffs_frag) as u8
    }
}

/// `DISKLABELV1_FFS_BSIZE(i)`.
pub const fn disklabelv1_ffs_bsize(i: u8) -> u32 {
    if i == 0 { 0 } else { 1 << ((i >> 3) + 12) }
}

/// `DISKLABELV1_FFS_FRAG(i)`.
pub const fn disklabelv1_ffs_frag(i: u8) -> u32 {
    if i == 0 { 0 } else { 1 << ((i & 0x07) - 1) }
}

/// `DISKLABELV1_FFS_FSIZE(i)`.
pub const fn disklabelv1_ffs_fsize(i: u8) -> u32 {
    if disklabelv1_ffs_frag(i) == 0 {
        0
    } else {
        disklabelv1_ffs_bsize(i) / disklabelv1_ffs_frag(i)
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

/// `DL_GETBSTART(d)`.
pub const fn dl_getbstart(d: &Disklabel) -> u64 {
    ((d.d_bstarth as u64) << 32) + d.d_bstart as u64
}

/// `DL_SETBSTART(d, n)`.
pub const fn dl_setbstart(d: &mut Disklabel, n: u64) {
    d.d_bstarth = (n >> 32) as u16;
    d.d_bstart = n as u32;
}

/// `DL_GETBEND(d)`.
pub const fn dl_getbend(d: &Disklabel) -> u64 {
    ((d.d_bendh as u64) << 32) + d.d_bend as u64
}

/// `DL_SETBEND(d, n)`.
pub const fn dl_setbend(d: &mut Disklabel, n: u64) {
    d.d_bendh = (n >> 32) as u16;
    d.d_bend = n as u32;
}

/// `DL_BLKSPERSEC(d)`: `DEV_BSIZE` blocks per sector.
pub const fn dl_blkspersec(d: &Disklabel) -> u64 {
    d.d_secsize as u64 / DEV_BSIZE as u64
}

/// `DL_SECTOBLK(d, n)`: sectors to `DEV_BSIZE` blocks.
pub const fn dl_sectoblk(d: &Disklabel, n: u64) -> u64 {
    n * dl_blkspersec(d)
}

/// `DL_BLKTOSEC(d, n)`: `DEV_BSIZE` blocks to sectors.
pub const fn dl_blktosec(d: &Disklabel, n: u64) -> u64 {
    n / dl_blkspersec(d)
}

/// `DL_BLKOFFSET(d, n)`: the byte offset of block `n` in its sector.
pub const fn dl_blkoffset(d: &Disklabel, n: u64) -> u64 {
    (n % dl_blkspersec(d)) * DEV_BSIZE as u64
}

/// `DL_PARTNUM2NAME(partnum)`: the letter of a partition, `None` where the C returns `-1`.
pub const fn dl_partnum2name(partnum: usize) -> Option<u8> {
    if partnum >= MAXPARTITIONS {
        return None;
    }
    if partnum <= (b'z' - b'a') as usize {
        Some(b'a' + partnum as u8)
    } else if partnum - 26 <= (b'Z' - b'A') as usize {
        Some(b'A' + (partnum - 26) as u8)
    } else {
        None
    }
}

/// `DL_PARTNAME2NUM(partname)`: the partition of a letter, `None` where the C returns `-1`.
pub const fn dl_partname2num(partname: u8) -> Option<usize> {
    let partnum = if partname.is_ascii_lowercase() {
        (partname - b'a') as usize
    } else if partname.is_ascii_uppercase() {
        (partname - b'A') as usize + 26
    } else {
        return None;
    };
    if partnum >= MAXPARTITIONS {
        None
    } else {
        Some(partnum)
    }
}

// d_type values:

/// `DTYPE_SMD`: SMD, XSMD; VAX hp/up.
pub const DTYPE_SMD: u16 = 1;
/// `DTYPE_MSCP`: MSCP.
pub const DTYPE_MSCP: u16 = 2;
/// `DTYPE_DEC`: other DEC (rk, rl).
pub const DTYPE_DEC: u16 = 3;
/// `DTYPE_SCSI`: SCSI.
pub const DTYPE_SCSI: u16 = 4;
/// `DTYPE_ESDI`: ESDI interface.
pub const DTYPE_ESDI: u16 = 5;
/// `DTYPE_ST506`: ST506 etc.
pub const DTYPE_ST506: u16 = 6;
/// `DTYPE_HPIB`: CS/80 on HP-IB.
pub const DTYPE_HPIB: u16 = 7;
/// `DTYPE_HPFL`: HP Fiber-link.
pub const DTYPE_HPFL: u16 = 8;
/// `DTYPE_FLOPPY`: floppy.
pub const DTYPE_FLOPPY: u16 = 10;
/// `DTYPE_CCD`: was: concatenated disk device.
pub const DTYPE_CCD: u16 = 11;
/// `DTYPE_VND`: vnode pseudo-disk.
pub const DTYPE_VND: u16 = 12;
/// `DTYPE_ATAPI`: ATAPI.
pub const DTYPE_ATAPI: u16 = 13;
/// `DTYPE_RAID`: was: RAIDframe.
pub const DTYPE_RAID: u16 = 14;
/// `DTYPE_RDROOT`: ram disk root.
pub const DTYPE_RDROOT: u16 = 15;

/// `dktypenames[]` (`DKTYPENAMES`), without the C's terminating NULL; "ccd" is deprecated.
pub const DKTYPENAMES: [&str; 16] = [
    "unknown", "SMD", "MSCP", "old DEC", "SCSI", "ESDI", "ST506", "HP-IB", "HP-FL", "type 9",
    "floppy", "ccd", "vnd", "ATAPI", "RAID", "rdroot",
];
/// `DKMAXTYPES`.
pub const DKMAXTYPES: usize = DKTYPENAMES.len();

// Filesystem type and version. Used to interpret other filesystem-specific per-partition
// information.

/// `FS_UNUSED`: unused.
pub const FS_UNUSED: u8 = 0;
/// `FS_SWAP`: swap.
pub const FS_SWAP: u8 = 1;
/// `FS_V6`: Sixth Edition.
pub const FS_V6: u8 = 2;
/// `FS_V7`: Seventh Edition.
pub const FS_V7: u8 = 3;
/// `FS_SYSV`: System V.
pub const FS_SYSV: u8 = 4;
/// `FS_V71K`: V7 with 1K blocks (4.1, 2.9).
pub const FS_V71K: u8 = 5;
/// `FS_V8`: Eighth Edition, 4K blocks.
pub const FS_V8: u8 = 6;
/// `FS_BSDFFS`: 4.2BSD fast file system.
pub const FS_BSDFFS: u8 = 7;
/// `FS_MSDOS`: MSDOS file system.
pub const FS_MSDOS: u8 = 8;
/// `FS_BSDLFS`: 4.4BSD log-structured file system.
pub const FS_BSDLFS: u8 = 9;
/// `FS_OTHER`: in use, but unknown/unsupported.
pub const FS_OTHER: u8 = 10;
/// `FS_HPFS`: OS/2 high-performance file system.
pub const FS_HPFS: u8 = 11;
/// `FS_ISO9660`: ISO 9660, normally CD-ROM.
pub const FS_ISO9660: u8 = 12;
/// `FS_BOOT`: partition contains bootstrap.
pub const FS_BOOT: u8 = 13;
/// `FS_ADOS`: AmigaDOS fast file system.
pub const FS_ADOS: u8 = 14;
/// `FS_HFS`: Macintosh HFS.
pub const FS_HFS: u8 = 15;
/// `FS_ADFS`: Acorn Disk Filing System.
pub const FS_ADFS: u8 = 16;
/// `FS_EXT2FS`: ext2fs.
pub const FS_EXT2FS: u8 = 17;
/// `FS_CCD`: ccd component.
pub const FS_CCD: u8 = 18;
/// `FS_RAID`: RAIDframe or softraid.
pub const FS_RAID: u8 = 19;
/// `FS_NTFS`: Windows/NT file system.
pub const FS_NTFS: u8 = 20;
/// `FS_UDF`: UDF (DVD) filesystem.
pub const FS_UDF: u8 = 21;

/// `fstypenames[]` (`DKTYPENAMES`), without the C's terminating NULL.
pub const FSTYPENAMES: [&str; 22] = [
    "unused",
    "swap",
    "Version6",
    "Version7",
    "SystemV",
    "4.1BSD",
    "Eighth-Edition",
    "4.2BSD",
    "MSDOS",
    "4.4LFS",
    "unknown",
    "HPFS",
    "ISO9660",
    "boot",
    "ADOS",
    "HFS",
    "ADFS",
    "ext2fs",
    "ccd",
    "RAID",
    "NTFS",
    "UDF",
];

/// `fstypesnames[]` (`DKTYPENAMES`): similar to the above, but used for things like the
/// mount command.
pub const FSTYPESNAMES: [&str; 22] = [
    "", "", "", "", "", "", "", "ffs", "msdos", "lfs", "", "", "cd9660", "", "ados", "", "",
    "ext2fs", "", "", "ntfs", "udf",
];

/// `FSMAXTYPES`.
pub const FSMAXTYPES: usize = FSTYPENAMES.len();

// flags shared by various drives:

/// `D_VENDOR`: vendor disklabel.
pub const D_VENDOR: u32 = 0x08;

/// `struct partinfo`: structure used internally to retrieve information about a partition
/// on a disk (`DIOCGPART`).
#[repr(C)]
#[derive(Clone, Copy, Debug)]
pub struct Partinfo {
    /// `disklab`: the disk's in-core label.
    pub disklab: *mut Disklabel,
    /// `part`: the partition's entry in it.
    pub part: *mut Partition,
}

impl Partinfo {
    /// Writes the structure into an `ioctl` data buffer, as the driver's
    /// `((struct partinfo *)data)->disklab = ...` does. A buffer too short is left alone.
    pub fn store(&self, data: &mut [u8]) {
        if data.len() >= core::mem::size_of::<Self>() {
            // SAFETY: the buffer holds at least `size_of::<Partinfo>()` bytes; the write is
            // unaligned because an ioctl buffer has no alignment guarantee.
            unsafe { data.as_mut_ptr().cast::<Self>().write_unaligned(*self) };
        }
    }

    /// Reads the structure back out of an `ioctl` data buffer; `None` if it is too short.
    pub fn load(data: &[u8]) -> Option<Self> {
        if data.len() < core::mem::size_of::<Self>() {
            return None;
        }
        // SAFETY: as for `store`; any bit pattern is a pair of raw pointers.
        Some(unsafe { data.as_ptr().cast::<Self>().read_unaligned() })
    }
}

// GUID partition table -- located at sector 1 of some disks.

/// `GPTSECTOR`: DOS boot block relative sector #.
pub const GPTSECTOR: u64 = 1;
/// `GPTSIGNATURE`: ASCII string "EFI PART" encoded as 64-bit.
pub const GPTSIGNATURE: u64 = 0x5452_4150_2049_4645;
/// `GPTREVISION`: GPT header version 1.0.
pub const GPTREVISION: u32 = 0x10000;
/// `NGPTPARTITIONS`.
pub const NGPTPARTITIONS: u32 = 128;
/// `GPTPARTATTR_REQUIRED`.
pub const GPTPARTATTR_REQUIRED: u64 = 1 << 0;
/// `GPTPARTATTR_IGNORE`.
pub const GPTPARTATTR_IGNORE: u64 = 1 << 1;
/// `GPTPARTATTR_BOOTABLE`.
pub const GPTPARTATTR_BOOTABLE: u64 = 1 << 2;
/// `GPTPARTATTR_MS_READONLY`.
pub const GPTPARTATTR_MS_READONLY: u64 = 1 << 60;
/// `GPTPARTATTR_MS_SHADOW`.
pub const GPTPARTATTR_MS_SHADOW: u64 = 1 << 61;
/// `GPTPARTATTR_MS_HIDDEN`.
pub const GPTPARTATTR_MS_HIDDEN: u64 = 1 << 62;
/// `GPTPARTATTR_MS_NOAUTOMOUNT`.
pub const GPTPARTATTR_MS_NOAUTOMOUNT: u64 = 1 << 63;

/// `GPTMINHDRSIZE`.
pub const GPTMINHDRSIZE: u32 = 92;
/// `GPTMINPARTSIZE`.
pub const GPTMINPARTSIZE: u32 = 128;
/// `GPTPARTNAMESIZE`.
pub const GPTPARTNAMESIZE: usize = 36;

/// `struct gpt_header`: the fields as on disk (little-endian), read through
/// [`GptHeader::from_bytes`].
#[repr(C)]
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct GptHeader {
    /// `gh_sig`: "EFI PART".
    pub gh_sig: u64,
    /// `gh_rev`: GPT Version 1.0: 0x00000100.
    pub gh_rev: u32,
    /// `gh_size`: Little-Endian.
    pub gh_size: u32,
    /// `gh_csum`: CRC32: with this field as 0.
    pub gh_csum: u32,
    /// `gh_rsvd`: always zero.
    pub gh_rsvd: u32,
    /// `gh_lba_self`: LBA of this header.
    pub gh_lba_self: u64,
    /// `gh_lba_alt`: LBA of alternate header.
    pub gh_lba_alt: u64,
    /// `gh_lba_start`: first usable LBA.
    pub gh_lba_start: u64,
    /// `gh_lba_end`: last usable LBA.
    pub gh_lba_end: u64,
    /// `gh_guid`: disk GUID used to identify the disk.
    pub gh_guid: Uuid,
    /// `gh_part_lba`: starting LBA of GPT partition entries.
    pub gh_part_lba: u64,
    /// `gh_part_num`: # of partition entries.
    pub gh_part_num: u32,
    /// `gh_part_size`: size per entry, shall be 128*(2**n) with n >= 0.
    pub gh_part_size: u32,
    /// `gh_part_csum`: CRC32 checksum of all partition entries: starts at gh_part_lba and
    /// is computed over a byte length of gh_part_num*gh_part_size.
    pub gh_part_csum: u32,
    // the rest of the block is reserved by UEFI and must be zero
}

/// Reads a native-order integer of `N` bytes at `off` (the C's struct fields over a
/// `memcpy`'d sector).
fn ne<const N: usize>(b: &[u8], off: usize) -> [u8; N] {
    let mut out = [0u8; N];
    out.copy_from_slice(&b[off..off + N]);
    out
}

impl GptHeader {
    /// The header in the first `GPTMINHDRSIZE` bytes of `b`: the C's
    /// `memcpy(&ngh, bp->b_data, sizeof(ngh))`.
    pub fn from_bytes(b: &[u8; GPTMINHDRSIZE as usize]) -> Self {
        Self {
            gh_sig: u64::from_ne_bytes(ne(b, 0)),
            gh_rev: u32::from_ne_bytes(ne(b, 8)),
            gh_size: u32::from_ne_bytes(ne(b, 12)),
            gh_csum: u32::from_ne_bytes(ne(b, 16)),
            gh_rsvd: u32::from_ne_bytes(ne(b, 20)),
            gh_lba_self: u64::from_ne_bytes(ne(b, 24)),
            gh_lba_alt: u64::from_ne_bytes(ne(b, 32)),
            gh_lba_start: u64::from_ne_bytes(ne(b, 40)),
            gh_lba_end: u64::from_ne_bytes(ne(b, 48)),
            gh_guid: Uuid::from_bytes(ne(b, 56)),
            gh_part_lba: u64::from_ne_bytes(ne(b, 72)),
            gh_part_num: u32::from_ne_bytes(ne(b, 80)),
            gh_part_size: u32::from_ne_bytes(ne(b, 84)),
            gh_part_csum: u32::from_ne_bytes(ne(b, 88)),
        }
    }

    /// The offset of `gh_csum` in the on-disk header.
    pub const CSUM_OFF: usize = 16;
}

/// `struct gpt_partition`, read through [`GptPartition::from_bytes`].
#[repr(C)]
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct GptPartition {
    /// `gp_type`: partition type GUID.
    pub gp_type: Uuid,
    /// `gp_guid`: unique partition GUID.
    pub gp_guid: Uuid,
    /// `gp_lba_start`: starting LBA of this partition.
    pub gp_lba_start: u64,
    /// `gp_lba_end`: ending LBA of this partition, inclusive, usually odd.
    pub gp_lba_end: u64,
    /// `gp_attrs`: attribute flags.
    pub gp_attrs: u64,
    /// `gp_name`: partition name, utf-16le.
    pub gp_name: [u16; GPTPARTNAMESIZE],
    // the rest of the GPT partition entry, if any, is reserved by UEFI and must be zero
}

impl GptPartition {
    /// The entry in the first `GPTMINPARTSIZE` bytes of `b`.
    pub fn from_bytes(b: &[u8; GPTMINPARTSIZE as usize]) -> Self {
        Self {
            gp_type: Uuid::from_bytes(ne(b, 0)),
            gp_guid: Uuid::from_bytes(ne(b, 16)),
            gp_lba_start: u64::from_ne_bytes(ne(b, 32)),
            gp_lba_end: u64::from_ne_bytes(ne(b, 40)),
            gp_attrs: u64::from_ne_bytes(ne(b, 48)),
            gp_name: core::array::from_fn(|i| u16::from_ne_bytes(ne(b, 56 + 2 * i))),
        }
    }
}

/// `GPT_UUID_EFI_SYSTEM`.
pub const GPT_UUID_EFI_SYSTEM: [u8; 16] = [
    0xc1, 0x2a, 0x73, 0x28, 0xf8, 0x1f, 0x11, 0xd2, 0xba, 0x4b, 0x00, 0xa0, 0xc9, 0x3e, 0xc9, 0x3b,
];
/// `GPT_UUID_OPENBSD`.
pub const GPT_UUID_OPENBSD: [u8; 16] = [
    0x82, 0x4c, 0xc7, 0xa0, 0x36, 0xa8, 0x11, 0xe3, 0x89, 0x0a, 0x95, 0x25, 0x19, 0xad, 0x3f, 0x61,
];

// DOS partition table -- located at start of some disks.

/// `DOS_LABELSECTOR`.
pub const DOS_LABELSECTOR: i64 = 1;
/// `DOSBBSECTOR`: DOS boot block relative sector #.
pub const DOSBBSECTOR: u64 = 0;
/// `DOSPARTOFF`.
pub const DOSPARTOFF: usize = 446;
/// `DOSDISKOFF`.
pub const DOSDISKOFF: usize = 444;
/// `NDOSPART`.
pub const NDOSPART: usize = 4;
/// `DOSACTIVE`: active partition.
pub const DOSACTIVE: u8 = 0x80;

/// `DOSMBR_SIGNATURE`.
pub const DOSMBR_SIGNATURE: u16 = 0xaa55;
/// `DOSMBR_SIGNATURE_OFF`.
pub const DOSMBR_SIGNATURE_OFF: usize = 0x1fe;

/// `DOS_MAXEBR`: maximum number of Extended Boot Records (EBRs) to traverse.
pub const DOS_MAXEBR: u32 = 256;

/// `struct dos_partition`: one entry of an MBR partition table. The multi-byte members
/// hold their on-disk (little endian) values; `letoh32` reads them.
#[repr(C)]
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct DosPartition {
    /// `dp_flag`: bootstrap flags.
    pub dp_flag: u8,
    /// `dp_shd`: starting head.
    pub dp_shd: u8,
    /// `dp_ssect`: starting sector.
    pub dp_ssect: u8,
    /// `dp_scyl`: starting cylinder.
    pub dp_scyl: u8,
    /// `dp_typ`: partition type (see below).
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
    /// The `NDOSPART` entries at `DOSPARTOFF` of a boot sector: the C's
    /// `memcpy(dp, dosbb + DOSPARTOFF, sizeof(dp))`. A short buffer leaves zero entries.
    pub fn table(sector: &[u8]) -> [Self; NDOSPART] {
        let mut dp = [Self::default(); NDOSPART];
        for (i, d) in dp.iter_mut().enumerate() {
            let off = DOSPARTOFF + i * 16;
            let Some(e) = sector.get(off..off + 16) else {
                break;
            };
            *d = Self {
                dp_flag: e[0],
                dp_shd: e[1],
                dp_ssect: e[2],
                dp_scyl: e[3],
                dp_typ: e[4],
                dp_ehd: e[5],
                dp_esect: e[6],
                dp_ecyl: e[7],
                dp_start: u32::from_ne_bytes([e[8], e[9], e[10], e[11]]),
                dp_size: u32::from_ne_bytes([e[12], e[13], e[14], e[15]]),
            };
        }
        dp
    }
}

// Known DOS partition types.

/// `DOSPTYP_UNUSED`: Unused partition.
pub const DOSPTYP_UNUSED: u8 = 0x00;
/// `DOSPTYP_FAT12`: 12-bit FAT.
pub const DOSPTYP_FAT12: u8 = 0x01;
/// `DOSPTYP_FAT16S`: 16-bit FAT, less than 32M.
pub const DOSPTYP_FAT16S: u8 = 0x04;
/// `DOSPTYP_EXTEND`: Extended; contains sub-partitions.
pub const DOSPTYP_EXTEND: u8 = 0x05;
/// `DOSPTYP_FAT16B`: 16-bit FAT, more than 32M.
pub const DOSPTYP_FAT16B: u8 = 0x06;
/// `DOSPTYP_NTFS`: NTFS.
pub const DOSPTYP_NTFS: u8 = 0x07;
/// `DOSPTYP_FAT32`: 32-bit FAT.
pub const DOSPTYP_FAT32: u8 = 0x0b;
/// `DOSPTYP_FAT32L`: 32-bit FAT, LBA-mapped.
pub const DOSPTYP_FAT32L: u8 = 0x0c;
/// `DOSPTYP_FAT16L`: 16-bit FAT, LBA-mapped.
pub const DOSPTYP_FAT16L: u8 = 0x0e;
/// `DOSPTYP_EXTENDL`: Extended, LBA-mapped; (sub-partitions).
pub const DOSPTYP_EXTENDL: u8 = 0x0f;
/// `DOSPTYP_ONTRACK`.
pub const DOSPTYP_ONTRACK: u8 = 0x54;
/// `DOSPTYP_LINUX`: That other thing.
pub const DOSPTYP_LINUX: u8 = 0x83;
/// `DOSPTYP_FREEBSD`: FreeBSD partition type.
pub const DOSPTYP_FREEBSD: u8 = 0xa5;
/// `DOSPTYP_OPENBSD`: OpenBSD partition type.
pub const DOSPTYP_OPENBSD: u8 = 0xa6;
/// `DOSPTYP_NETBSD`: NetBSD partition type.
pub const DOSPTYP_NETBSD: u8 = 0xa9;
/// `DOSPTYP_EFI`: EFI Protective Partition.
pub const DOSPTYP_EFI: u8 = 0xee;
/// `DOSPTYP_EFISYS`: EFI System Partition.
pub const DOSPTYP_EFISYS: u8 = 0xef;

/// `struct dos_mbr` (`__packed`).
#[repr(C, packed)]
#[derive(Clone, Copy)]
pub struct DosMbr {
    /// `dmbr_boot`.
    pub dmbr_boot: [u8; DOSPARTOFF],
    /// `dmbr_parts`.
    pub dmbr_parts: [DosPartition; NDOSPART],
    /// `dmbr_sign`.
    pub dmbr_sign: u16,
}

// The layouts the C's casts and sector I/O rely on.
const _: () = {
    assert!(core::mem::size_of::<Partition>() == 16);
    assert!(DISKLABEL_PARTITIONS_OFFSET == 148);
    assert!(DISKLABEL_SIZE == 148 + 16 * MAXPARTITIONSUNIT as usize);
    assert!(core::mem::size_of::<DosPartition>() == 16);
    assert!(core::mem::size_of::<DosMbr>() == 512);
    assert!(MAXPARTITIONS <= MAXPARTITIONSUNIT as usize);
};

#[cfg(test)]
mod tests;
