/*	$OpenBSD: bpb.h,v 1.7 2015/10/23 10:45:31 krw Exp $	*/
/*	$NetBSD: bpb.h,v 1.6 1997/10/17 11:23:35 ws Exp $	*/
/* <LICENSES> */
/*
 * Written by Paul Popelka (paulp@uts.amdahl.com)
 *
 * You can do anything you want with this software, just don't say you wrote
 * it, and don't remove this notice.
 *
 * This software is provided "as is".
 *
 * The author supplies this software to be publicly redistributed on the
 * understanding that the author is not responsible for the correct
 * functioning of this software in any circumstances and is not liable for
 * any damages caused by this software.
 *
 * October 1992
 */
/* </LICENSES> */

//! `<msdosfs/bpb.h>`: the BIOS Parameter Block (BPB) of DOS 3.3, 5.0 and 7.10 (FAT32), in
//! core and as it lies on the disk, the FAT32 FSInfo block, and the little-endian accessors
//! `getushort`/`getulong`/`putushort`/`putulong`.
//!
//! Upstream: sys/msdosfs/bpb.h @ 3ce1f3f79392
//!
//! The on-disk structures (`struct byte_bpb*`, `struct fsinfo`, and the boot sectors and
//! directory entries of `bootsect.rs` and `direntry.rs`) are made of bytes only, so that the
//! compiler adds no padding; they are viewed in place over a buffer's bytes with `at` and
//! `at_mut` (the C's cast of a pointer into `b_data`).
//!
//! ## Deviations
//! - The `int8_t` members of `struct byte_bpb33` and `struct byte_bpb50` are `u8`: every
//!   multi-byte one is read through `getushort`/`getulong`, which take bytes as `u_int8_t`, and
//!   the single-byte ones (`bpbSecPerClust`, `bpbFATs`, `bpbMedia`) are stored into the
//!   `u_int8_t` members of `struct bpb50`, where the conversion gives back the same bits.
//! - `getushort`/`getulong`/`putushort`/`putulong` are functions over byte slices. The C picks
//!   a direct (possibly unaligned) load on little-endian machines without
//!   `__STRICT_ALIGNMENT`, and byte arithmetic elsewhere; both read and write little-endian
//!   values, which is what `u16::from_le_bytes` and friends do on every machine. The C's `v`
//!   argument is truncated by the store; here the caller passes the stored width.
//! - `byte_view!` (not in the C) generates the `at`/`at_mut` views and the alignment check of
//!   each on-disk structure.

/// `FATNUM`: mask for numbering active FAT (in `bpbExtFlags`).
pub const FATNUM: u16 = 0xf;
/// `FATMIRROR`: FAT is mirrored (like it always was).
pub const FATMIRROR: u16 = 0x80;
/// `FSVERS`: currently only 0 is understood (`bpbFSVers`).
pub const FSVERS: u16 = 0;

/// `struct bpb33`: BIOS Parameter Block (BPB) for DOS 3.3.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
#[allow(non_snake_case)] // the C field names
pub struct Bpb33 {
    /// `bpbBytesPerSec`: bytes per sector.
    pub bpbBytesPerSec: u16,
    /// `bpbSecPerClust`: sectors per cluster.
    pub bpbSecPerClust: u8,
    /// `bpbResSectors`: number of reserved sectors.
    pub bpbResSectors: u16,
    /// `bpbFATs`: number of FATs.
    pub bpbFATs: u8,
    /// `bpbRootDirEnts`: number of root directory entries.
    pub bpbRootDirEnts: u16,
    /// `bpbSectors`: total number of sectors.
    pub bpbSectors: u16,
    /// `bpbMedia`: media descriptor.
    pub bpbMedia: u8,
    /// `bpbFATsecs`: number of sectors per FAT.
    pub bpbFATsecs: u16,
    /// `bpbSecPerTrack`: sectors per track.
    pub bpbSecPerTrack: u16,
    /// `bpbHeads`: number of heads.
    pub bpbHeads: u16,
    /// `bpbHiddenSecs`: number of hidden sectors.
    pub bpbHiddenSecs: u16,
}

/// `struct bpb50`: BPB for DOS 5.0. The difference is `bpbHiddenSecs` is a short for DOS 3.3,
/// and `bpbHugeSectors` is not in the 3.3 bpb.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
#[allow(non_snake_case)] // the C field names
pub struct Bpb50 {
    /// `bpbBytesPerSec`: bytes per sector.
    pub bpbBytesPerSec: u16,
    /// `bpbSecPerClust`: sectors per cluster.
    pub bpbSecPerClust: u8,
    /// `bpbResSectors`: number of reserved sectors.
    pub bpbResSectors: u16,
    /// `bpbFATs`: number of FATs.
    pub bpbFATs: u8,
    /// `bpbRootDirEnts`: number of root directory entries.
    pub bpbRootDirEnts: u16,
    /// `bpbSectors`: total number of sectors.
    pub bpbSectors: u16,
    /// `bpbMedia`: media descriptor.
    pub bpbMedia: u8,
    /// `bpbFATsecs`: number of sectors per FAT.
    pub bpbFATsecs: u16,
    /// `bpbSecPerTrack`: sectors per track.
    pub bpbSecPerTrack: u16,
    /// `bpbHeads`: number of heads.
    pub bpbHeads: u16,
    /// `bpbHiddenSecs`: # of hidden sectors.
    pub bpbHiddenSecs: u32,
    /// `bpbHugeSectors`: # of sectors if `bpbSectors == 0`.
    pub bpbHugeSectors: u32,
}

impl Bpb50 {
    /// A zeroed BPB, as `malloc(M_ZERO)` leaves it in a `struct msdosfsmount`.
    pub const fn new() -> Self {
        Self {
            bpbBytesPerSec: 0,
            bpbSecPerClust: 0,
            bpbResSectors: 0,
            bpbFATs: 0,
            bpbRootDirEnts: 0,
            bpbSectors: 0,
            bpbMedia: 0,
            bpbFATsecs: 0,
            bpbSecPerTrack: 0,
            bpbHeads: 0,
            bpbHiddenSecs: 0,
            bpbHugeSectors: 0,
        }
    }
}

/// `struct bpb710`: BPB for DOS 7.10 (FAT32). This one has a few extensions to bpb50.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
#[allow(non_snake_case)] // the C field names
pub struct Bpb710 {
    /// `bpbBytesPerSec`: bytes per sector.
    pub bpbBytesPerSec: u16,
    /// `bpbSecPerClust`: sectors per cluster.
    pub bpbSecPerClust: u8,
    /// `bpbResSectors`: number of reserved sectors.
    pub bpbResSectors: u16,
    /// `bpbFATs`: number of FATs.
    pub bpbFATs: u8,
    /// `bpbRootDirEnts`: number of root directory entries.
    pub bpbRootDirEnts: u16,
    /// `bpbSectors`: total number of sectors.
    pub bpbSectors: u16,
    /// `bpbMedia`: media descriptor.
    pub bpbMedia: u8,
    /// `bpbFATsecs`: number of sectors per FAT.
    pub bpbFATsecs: u16,
    /// `bpbSecPerTrack`: sectors per track.
    pub bpbSecPerTrack: u16,
    /// `bpbHeads`: number of heads.
    pub bpbHeads: u16,
    /// `bpbHiddenSecs`: # of hidden sectors.
    pub bpbHiddenSecs: u32,
    /// `bpbHugeSectors`: # of sectors if `bpbSectors == 0`.
    pub bpbHugeSectors: u32,
    /// `bpbBigFATsecs`: like `bpbFATsecs` for FAT32.
    pub bpbBigFATsecs: u32,
    /// `bpbExtFlags`: extended flags (`FATNUM`, `FATMIRROR`).
    pub bpbExtFlags: u16,
    /// `bpbFSVers`: filesystem version (`FSVERS`).
    pub bpbFSVers: u16,
    /// `bpbRootClust`: start cluster for root directory.
    pub bpbRootClust: u32,
    /// `bpbFSInfo`: filesystem info structure sector.
    pub bpbFSInfo: u16,
    /// `bpbBackup`: backup boot sector.
    pub bpbBackup: u16,
    // There is a 12 byte filler here, but we ignore it.
}

/// `getushort(x)`: the little-endian 16-bit value at the start of `x`.
pub fn getushort(x: &[u8]) -> u16 {
    u16::from_le_bytes([x[0], x[1]])
}

/// `getulong(x)`: the little-endian 32-bit value at the start of `x`.
pub fn getulong(x: &[u8]) -> u32 {
    u32::from_le_bytes([x[0], x[1], x[2], x[3]])
}

/// `putushort(p, v)`: stores `v` little-endian at the start of `p`.
pub fn putushort(p: &mut [u8], v: u16) {
    p[..2].copy_from_slice(&v.to_le_bytes());
}

/// `putulong(p, v)`: stores `v` little-endian at the start of `p`.
pub fn putulong(p: &mut [u8], v: u32) {
    p[..4].copy_from_slice(&v.to_le_bytes());
}

/// Gives an on-disk structure made of bytes only its in-place views: `SIZE`, `at(buf, off)` and
/// `at_mut(buf, off)` (the C's `(struct foo *)(bp->b_data + off)`), and checks at compile time
/// that the structure has the alignment of a byte, which is what makes the views sound.
macro_rules! byte_view {
    ($t:ty) => {
        impl $t {
            /// The size of the structure on disk.
            pub const SIZE: usize = ::core::mem::size_of::<$t>();

            /// The structure that starts at byte `off` of `buf`. Panics when `buf` is too short.
            pub fn at(buf: &[u8], off: usize) -> &$t {
                let b = &buf[off..off + Self::SIZE];
                // SAFETY: `b` holds `SIZE` bytes (the slice bounds); the structure is
                // `#[repr(C)]` of bytes only, so its alignment is 1 (checked below) and every
                // bit pattern is a valid value; the reference borrows `buf`.
                unsafe { &*b.as_ptr().cast::<$t>() }
            }

            /// The structure that starts at byte `off` of `buf`, writable. Panics when `buf` is
            /// too short.
            pub fn at_mut(buf: &mut [u8], off: usize) -> &mut $t {
                let b = &mut buf[off..off + Self::SIZE];
                // SAFETY: as in `at`; the reference borrows `buf` mutably, so it is the only
                // one to these bytes.
                unsafe { &mut *b.as_mut_ptr().cast::<$t>() }
            }
        }

        const _: () = assert!(::core::mem::align_of::<$t>() == 1);
    };
}
pub(crate) use byte_view;

/// `struct byte_bpb33`: the DOS 3.3 BPB as it lies on the disk. Shorts and longs are just
/// character arrays of the appropriate length.
#[repr(C)]
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
#[allow(non_snake_case)] // the C field names
pub struct ByteBpb33 {
    /// `bpbBytesPerSec`: bytes per sector.
    pub bpbBytesPerSec: [u8; 2],
    /// `bpbSecPerClust`: sectors per cluster.
    pub bpbSecPerClust: u8,
    /// `bpbResSectors`: number of reserved sectors.
    pub bpbResSectors: [u8; 2],
    /// `bpbFATs`: number of FATs.
    pub bpbFATs: u8,
    /// `bpbRootDirEnts`: number of root directory entries.
    pub bpbRootDirEnts: [u8; 2],
    /// `bpbSectors`: total number of sectors.
    pub bpbSectors: [u8; 2],
    /// `bpbMedia`: media descriptor.
    pub bpbMedia: u8,
    /// `bpbFATsecs`: number of sectors per FAT.
    pub bpbFATsecs: [u8; 2],
    /// `bpbSecPerTrack`: sectors per track.
    pub bpbSecPerTrack: [u8; 2],
    /// `bpbHeads`: number of heads.
    pub bpbHeads: [u8; 2],
    /// `bpbHiddenSecs`: number of hidden sectors.
    pub bpbHiddenSecs: [u8; 2],
}

byte_view!(ByteBpb33);

/// `struct byte_bpb50`: the DOS 5.0 BPB as it lies on the disk.
#[repr(C)]
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
#[allow(non_snake_case)] // the C field names
pub struct ByteBpb50 {
    /// `bpbBytesPerSec`: bytes per sector.
    pub bpbBytesPerSec: [u8; 2],
    /// `bpbSecPerClust`: sectors per cluster.
    pub bpbSecPerClust: u8,
    /// `bpbResSectors`: number of reserved sectors.
    pub bpbResSectors: [u8; 2],
    /// `bpbFATs`: number of FATs.
    pub bpbFATs: u8,
    /// `bpbRootDirEnts`: number of root directory entries.
    pub bpbRootDirEnts: [u8; 2],
    /// `bpbSectors`: total number of sectors.
    pub bpbSectors: [u8; 2],
    /// `bpbMedia`: media descriptor.
    pub bpbMedia: u8,
    /// `bpbFATsecs`: number of sectors per FAT.
    pub bpbFATsecs: [u8; 2],
    /// `bpbSecPerTrack`: sectors per track.
    pub bpbSecPerTrack: [u8; 2],
    /// `bpbHeads`: number of heads.
    pub bpbHeads: [u8; 2],
    /// `bpbHiddenSecs`: number of hidden sectors.
    pub bpbHiddenSecs: [u8; 4],
    /// `bpbHugeSectors`: # of sectors if `bpbSectors == 0`.
    pub bpbHugeSectors: [u8; 4],
}

byte_view!(ByteBpb50);

/// `struct byte_bpb710`: the DOS 7.10 (FAT32) BPB as it lies on the disk.
#[repr(C)]
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
#[allow(non_snake_case)] // the C field names
pub struct ByteBpb710 {
    /// `bpbBytesPerSec`: bytes per sector.
    pub bpbBytesPerSec: [u8; 2],
    /// `bpbSecPerClust`: sectors per cluster.
    pub bpbSecPerClust: u8,
    /// `bpbResSectors`: number of reserved sectors.
    pub bpbResSectors: [u8; 2],
    /// `bpbFATs`: number of FATs.
    pub bpbFATs: u8,
    /// `bpbRootDirEnts`: number of root directory entries.
    pub bpbRootDirEnts: [u8; 2],
    /// `bpbSectors`: total number of sectors.
    pub bpbSectors: [u8; 2],
    /// `bpbMedia`: media descriptor.
    pub bpbMedia: u8,
    /// `bpbFATsecs`: number of sectors per FAT.
    pub bpbFATsecs: [u8; 2],
    /// `bpbSecPerTrack`: sectors per track.
    pub bpbSecPerTrack: [u8; 2],
    /// `bpbHeads`: number of heads.
    pub bpbHeads: [u8; 2],
    /// `bpbHiddenSecs`: # of hidden sectors.
    pub bpbHiddenSecs: [u8; 4],
    /// `bpbHugeSectors`: # of sectors if `bpbSectors == 0`.
    pub bpbHugeSectors: [u8; 4],
    /// `bpbBigFATsecs`: like `bpbFATsecs` for FAT32.
    pub bpbBigFATsecs: [u8; 4],
    /// `bpbExtFlags`: extended flags.
    pub bpbExtFlags: [u8; 2],
    /// `bpbFSVers`: filesystem version.
    pub bpbFSVers: [u8; 2],
    /// `bpbRootClust`: start cluster for root directory.
    pub bpbRootClust: [u8; 4],
    /// `bpbFSInfo`: filesystem info structure sector.
    pub bpbFSInfo: [u8; 2],
    /// `bpbBackup`: backup boot sector.
    pub bpbBackup: [u8; 2],
    // There is a 12 byte filler here, but we ignore it.
}

byte_view!(ByteBpb710);

/// `struct fsinfo`: the FAT32 FSInfo block.
#[repr(C)]
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Fsinfo {
    /// `fsisig1`.
    pub fsisig1: [u8; 4],
    /// `fsifill1`.
    pub fsifill1: [u8; 480],
    /// `fsisig2`.
    pub fsisig2: [u8; 4],
    /// `fsinfree`: the free cluster count.
    pub fsinfree: [u8; 4],
    /// `fsinxtfree`: the next free cluster hint.
    pub fsinxtfree: [u8; 4],
    /// `fsifill2`.
    pub fsifill2: [u8; 12],
    /// `fsisig3`.
    pub fsisig3: [u8; 4],
    /// `fsifill3`.
    pub fsifill3: [u8; 508],
    /// `fsisig4`.
    pub fsisig4: [u8; 4],
}

byte_view!(Fsinfo);

const _: () = {
    assert!(ByteBpb33::SIZE == 19);
    assert!(ByteBpb50::SIZE == 25);
    assert!(ByteBpb710::SIZE == 41);
    assert!(Fsinfo::SIZE == 1024);
    assert!(core::mem::offset_of!(Fsinfo, fsinfree) == 488);
    assert!(core::mem::offset_of!(Fsinfo, fsisig3) == 508);
};

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn little_endian_accessors() {
        let mut b = [0u8; 6];
        putushort(&mut b[1..], 0x1234);
        putulong(&mut b[2..], 0xdead_beef);
        assert_eq!(b, [0, 0x34, 0xef, 0xbe, 0xad, 0xde]);
        assert_eq!(getushort(&b[2..]), 0xbeef);
        assert_eq!(getulong(&b[2..]), 0xdead_beef);
    }

    #[test]
    fn views_read_in_place() {
        let mut sector = [0u8; 64];
        // A FAT32 BPB at offset 11 of a boot sector: 512 bytes per sector, root cluster 2.
        sector[11..13].copy_from_slice(&512u16.to_le_bytes());
        sector[11 + 33..11 + 37].copy_from_slice(&2u32.to_le_bytes());
        let b = ByteBpb710::at(&sector, 11);
        assert_eq!(getushort(&b.bpbBytesPerSec), 512);
        assert_eq!(getulong(&b.bpbRootClust), 2);
        ByteBpb710::at_mut(&mut sector, 11).bpbFATs = 2;
        assert_eq!(sector[11 + 5], 2);
    }
}
