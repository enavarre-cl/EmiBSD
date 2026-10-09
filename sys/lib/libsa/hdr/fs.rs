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
//! `<ufs/ffs/fs.h>` for libsa: the super-block (`struct fs`) and the address macros the
//! standalone FFS reader uses.

use super::dinode::NDADDR;
use super::types::Daddr;

/// `BBSIZE`: the boot block size.
pub const BBSIZE: usize = 8192;
/// `SBSIZE`: the super-block size.
pub const SBSIZE: usize = 8192;
/// `SBLOCK`: the FFS1 super-block's [`DEV_BSIZE`](super::param::DEV_BSIZE) block.
pub const SBLOCK: Daddr = (BBSIZE / super::param::DEV_BSIZE) as Daddr;
/// `SBLOCK_UFS1`: the FFS1 super-block's byte offset.
pub const SBLOCK_UFS1: i64 = 8192;
/// `SBLOCK_UFS2`: the FFS2 super-block's byte offset.
pub const SBLOCK_UFS2: i64 = 65536;
/// `MAXMNTLEN`.
pub const MAXMNTLEN: usize = 468;
/// `MAXVOLLEN`.
pub const MAXVOLLEN: usize = 32;
/// `NOCSPTRS`: the in-core pointer slots of the super-block.
pub const NOCSPTRS: usize = (128 / core::mem::size_of::<usize>()) - 4;
/// `FSMAXSNAP`.
pub const FSMAXSNAP: usize = 20;
/// `FS_MAGIC` (`FS_UFS1_MAGIC`).
pub const FS_MAGIC: i32 = 0x011954;
/// `FS_UFS2_MAGIC`.
pub const FS_UFS2_MAGIC: i32 = 0x1954_0119;

/// `struct csum`.
#[repr(C)]
#[derive(Clone, Copy, Default, Debug)]
pub struct Csum {
    /// `cs_ndir`.
    pub cs_ndir: i32,
    /// `cs_nbfree`.
    pub cs_nbfree: i32,
    /// `cs_nifree`.
    pub cs_nifree: i32,
    /// `cs_nffree`.
    pub cs_nffree: i32,
}

/// `struct csum_total`.
#[repr(C)]
#[derive(Clone, Copy, Default, Debug)]
pub struct CsumTotal {
    /// `cs_ndir`.
    pub cs_ndir: i64,
    /// `cs_nbfree`.
    pub cs_nbfree: i64,
    /// `cs_nifree`.
    pub cs_nifree: i64,
    /// `cs_nffree`.
    pub cs_nffree: i64,
    /// `cs_spare`.
    pub cs_spare: [i64; 4],
}

/// `struct fs`: the super-block. The in-core pointer members (`fs_ocsp`, `fs_contigdirs`,
/// `fs_csp`, `fs_maxcluster`, `fs_active`) are pointer-sized slots the standalone code never
/// follows.
#[repr(C)]
#[derive(Clone, Copy, Debug)]
#[allow(missing_docs)] // the members are documented by fs.h; their names are the C's
pub struct Fs {
    pub fs_firstfield: i32,
    pub fs_unused_1: i32,
    pub fs_sblkno: i32,
    pub fs_cblkno: i32,
    pub fs_iblkno: i32,
    pub fs_dblkno: i32,
    pub fs_cgoffset: i32,
    pub fs_cgmask: i32,
    pub fs_ffs1_time: i32,
    pub fs_ffs1_size: i32,
    pub fs_ffs1_dsize: i32,
    pub fs_ncg: u32,
    pub fs_bsize: i32,
    pub fs_fsize: i32,
    pub fs_frag: i32,
    pub fs_minfree: i32,
    pub fs_rotdelay: i32,
    pub fs_rps: i32,
    pub fs_bmask: i32,
    pub fs_fmask: i32,
    pub fs_bshift: i32,
    pub fs_fshift: i32,
    pub fs_maxcontig: i32,
    pub fs_maxbpg: i32,
    pub fs_fragshift: i32,
    pub fs_fsbtodb: i32,
    pub fs_sbsize: i32,
    pub fs_csmask: i32,
    pub fs_csshift: i32,
    pub fs_nindir: i32,
    pub fs_inopb: u32,
    pub fs_nspf: i32,
    pub fs_optim: i32,
    pub fs_npsect: i32,
    pub fs_interleave: i32,
    pub fs_trackskew: i32,
    pub fs_id: [i32; 2],
    pub fs_ffs1_csaddr: i32,
    pub fs_cssize: i32,
    pub fs_cgsize: i32,
    pub fs_ntrak: i32,
    pub fs_nsect: i32,
    pub fs_spc: i32,
    pub fs_ncyl: i32,
    pub fs_cpg: i32,
    pub fs_ipg: u32,
    pub fs_fpg: i32,
    pub fs_ffs1_cstotal: Csum,
    pub fs_fmod: i8,
    pub fs_clean: i8,
    pub fs_ronly: i8,
    pub fs_ffs1_flags: i8,
    pub fs_fsmnt: [u8; MAXMNTLEN],
    pub fs_volname: [u8; MAXVOLLEN],
    pub fs_swuid: u64,
    pub fs_pad: i32,
    pub fs_cgrotor: i32,
    pub fs_ocsp: [usize; NOCSPTRS],
    pub fs_contigdirs: usize,
    pub fs_csp: usize,
    pub fs_maxcluster: usize,
    pub fs_active: usize,
    pub fs_cpc: i32,
    pub fs_maxbsize: i32,
    pub fs_spareconf64: [i64; 17],
    pub fs_sblockloc: i64,
    pub fs_cstotal: CsumTotal,
    pub fs_time: i64,
    pub fs_size: i64,
    pub fs_dsize: i64,
    pub fs_csaddr: i64,
    pub fs_pendingblocks: i64,
    pub fs_pendinginodes: u32,
    pub fs_snapinum: [u32; FSMAXSNAP],
    pub fs_avgfilesize: u32,
    pub fs_avgfpdir: u32,
    pub fs_sparecon: [i32; 26],
    pub fs_flags: u32,
    pub fs_fscktime: i32,
    pub fs_contigsumsize: i32,
    pub fs_maxsymlinklen: i32,
    pub fs_inodefmt: i32,
    pub fs_maxfilesize: u64,
    pub fs_qbmask: i64,
    pub fs_qfmask: i64,
    pub fs_state: i32,
    pub fs_postblformat: i32,
    pub fs_nrpos: i32,
    pub fs_postbloff: i32,
    pub fs_rotbloff: i32,
    pub fs_magic: i32,
    pub fs_space: [u8; 1],
}

impl Fs {
    /// The super-block in the first `size_of::<Fs>()` bytes of `buf` (the C casts the
    /// buffer it read; this copies, since a byte buffer has no alignment).
    pub fn from_bytes(buf: &[u8]) -> Option<Self> {
        if buf.len() < core::mem::size_of::<Self>() {
            return None;
        }
        // SAFETY: `buf` holds at least `size_of::<Fs>()` bytes (checked above), the read is
        // unaligned, and `Fs` is `#[repr(C)]` of integers and arrays of integers, valid for
        // any byte pattern.
        Some(unsafe { buf.as_ptr().cast::<Self>().read_unaligned() })
    }
}

/// `fsbtodb(fs, b)`: file system blocks to disk blocks.
pub const fn fsbtodb(fs: &Fs, b: i64) -> i64 {
    b << fs.fs_fsbtodb
}

/// `cgbase(fs, c)`: the first fragment of cylinder group `c`.
pub const fn cgbase(fs: &Fs, c: i64) -> i64 {
    fs.fs_fpg as i64 * c
}

/// `cgstart(fs, c)`: the start of cylinder group `c`, with the old rotation offset.
pub const fn cgstart(fs: &Fs, c: i64) -> i64 {
    cgbase(fs, c) + fs.fs_cgoffset as i64 * (c & !(fs.fs_cgmask as i64))
}

/// `cgimin(fs, c)`: the first inode block of cylinder group `c`.
pub const fn cgimin(fs: &Fs, c: i64) -> i64 {
    cgstart(fs, c) + fs.fs_iblkno as i64
}

/// `ino_to_cg(fs, x)`.
pub const fn ino_to_cg(fs: &Fs, x: u32) -> i64 {
    (x / fs.fs_ipg) as i64
}

/// `INOPB(fs)`: inodes per block.
pub const fn inopb(fs: &Fs) -> u32 {
    fs.fs_inopb
}

/// `blkstofrags(fs, blks)`.
pub const fn blkstofrags(fs: &Fs, blks: i64) -> i64 {
    blks << fs.fs_fragshift
}

/// `ino_to_fsba(fs, x)`: the file system block holding inode `x`.
pub const fn ino_to_fsba(fs: &Fs, x: u32) -> i64 {
    cgimin(fs, ino_to_cg(fs, x)) + blkstofrags(fs, ((x % fs.fs_ipg) / inopb(fs)) as i64)
}

/// `ino_to_fsbo(fs, x)`: inode `x`'s index in its block.
pub const fn ino_to_fsbo(fs: &Fs, x: u32) -> usize {
    (x % inopb(fs)) as usize
}

/// `blkoff(fs, loc)`: `loc % fs_bsize`.
pub const fn blkoff(fs: &Fs, loc: i64) -> i64 {
    loc & fs.fs_qbmask
}

/// `lblkno(fs, loc)`: `loc / fs_bsize`.
pub const fn lblkno(fs: &Fs, loc: i64) -> i64 {
    loc >> fs.fs_bshift
}

/// `fragroundup(fs, size)`: `size` rounded up to a fragment.
pub const fn fragroundup(fs: &Fs, size: i64) -> i64 {
    (size + fs.fs_qfmask) & fs.fs_fmask as i64
}

/// `dblksize(fs, dip, lbn)`: the size of logical block `lbn` of a file of `di_size` bytes.
pub const fn dblksize(fs: &Fs, di_size: u64, lbn: u64) -> u64 {
    if lbn >= NDADDR as u64 || di_size >= (lbn + 1) << fs.fs_bshift {
        fs.fs_bsize as u64
    } else {
        fragroundup(fs, blkoff(fs, di_size as i64)) as u64
    }
}

/// `NINDIR(fs)`: block addresses per indirect block.
pub const fn nindir(fs: &Fs) -> i32 {
    fs.fs_nindir
}

const _: () = {
    assert!(core::mem::offset_of!(Fs, fs_maxsymlinklen) == 1320);
    assert!(core::mem::offset_of!(Fs, fs_qbmask) == 1336);
    assert!(core::mem::offset_of!(Fs, fs_magic) == 1372);
    assert!(core::mem::size_of::<Fs>() == 1384);
};
/* </CODE> */
