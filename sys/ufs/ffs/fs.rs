/* <LICENSES> */
/*
 * Copyright (c) 1982, 1986, 1993
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
 *	@(#)fs.h	8.10 (Berkeley) 10/27/94
 */
/* </LICENSES> */

/* <CODE> */
//! `<ufs/ffs/fs.h>`: the layout of a fast file system: the super-block (`struct fs`), the
//! cylinder group block (`struct cg`, and the old `struct ocg`), the cylinder group summaries
//! (`struct csum`), and the macros that turn inode numbers, logical blocks and fragments into
//! disk addresses.
//!
//! Upstream: sys/ufs/ffs/fs.h @ 3ce1f3f79392
//!
//! Each disk drive contains some number of file systems. A file system consists of a number
//! of cylinder groups. Each cylinder group has inodes and data. A file system is described
//! by its super-block, which in turn describes the cylinder groups. The super-block is
//! critical data and is replicated in each cylinder group to protect against catastrophic
//! loss. This is done at `newfs` time and the critical super-block data does not change, so
//! the copies need not be referenced further unless disaster strikes.
//!
//! For file system fs, the offsets of the various blocks of interest are given in the super
//! block as: `[fs->fs_sblkno]` super-block, `[fs->fs_cblkno]` cylinder group block,
//! `[fs->fs_iblkno]` inode blocks, `[fs->fs_dblkno]` data blocks. The beginning of cylinder
//! group cg in fs, is given by the `cgbase(fs, cg)` macro.
//!
//! ## Deviations
//! - `struct fs`, `struct csum`, `struct csum_total` and `struct cg` keep the C layout
//!   (`#[repr(C)]`, pinned by the compile-time checks below), with every member a `Cell` (or
//!   an array of them): the C changes the in-core super-block and the cylinder group in a
//!   buffer through shared pointers everywhere (`fs->fs_cstotal.cs_nbfree--`), and a `Cell`
//!   has its value's layout. The in-core super-block is `malloc`ed memory reached as
//!   `&'static Fs`; a cylinder group is reached through [`CgBuf`], a view of the buffer that
//!   holds it.
//! - `cg_frsum` is `int32_t` here (`u_int32_t` in C): `ffs_fragacct` adds signed counts to
//!   it, as the C does through an `int32_t *`; the bits are the same.
//! - The in-core pointers of `struct fs` (`fs_csp`, `fs_maxcluster`, `fs_contigdirs`) stay
//!   raw pointers in their slots, read and written through the bounds-checked accessors
//!   [`Fs::fs_cs`], [`Fs::contigdirs`], [`Fs::set_maxcluster`], ...; the old `fs_ocsp`
//!   padding is kept.
//! - The array-locating macros over a cylinder group (`cg_blktot`, `cg_blks`, `cg_inosused`,
//!   `cg_blksfree`, `cg_clustersfree`, `cg_clustersum`, `cg_chkmagic`) are methods of
//!   [`CgBuf`], which also knows the old (`struct ocg`) layout, as the macros do.
//! - The other macros are functions with the macros' names in lower case (`fsbtodb`,
//!   `cgtod`, `ino_to_fsba`, `blkoff`, `lblkno`, `fragroundup`, `blksize`, ...), and
//!   `NINDIR`/`INOPB`/`INOPF`/`NSPB`/`NSPF`/`CGSIZE`/`FS_KERNMAXFILESIZE` are `nindir`,
//!   `inopb`, `inopf`, `nspb`, `nspf`, `cgsize`, `fs_kernmaxfilesize`.
//! - `inside[]`, `around[]` and `fragtbl[]` are `ffs_tables.c`'s, re-exported here.

use core::cell::Cell;
use core::ffi::c_void;
use core::marker::PhantomData;
use core::ptr::NonNull;

use crate::kern::subr_prf::panic;
use crate::sys::buf::Buf;
use crate::sys::param::{DEV_BSIZE, howmany};
use crate::sys::types::{Daddr, Off};
use crate::ufs::ufs::dinode::{NDADDR, Ufsino};
use crate::ufs::ufs::inode::Inode;

pub use crate::ufs::ffs::ffs_tables::{AROUND, FRAGTBL, INSIDE};

/// `BBSIZE`: the boot block's size.
pub const BBSIZE: usize = 8192;
/// `SBSIZE`: the super-block's size.
pub const SBSIZE: usize = 8192;
/// `BBOFF`.
pub const BBOFF: Off = 0;
/// `SBOFF`.
pub const SBOFF: Off = BBOFF + BBSIZE as Off;
/// `BBLOCK`.
pub const BBLOCK: Daddr = 0;
/// `SBLOCK`.
pub const SBLOCK: Daddr = BBLOCK + (BBSIZE / DEV_BSIZE) as Daddr;
/// `SBLOCK_UFS1`: the byte offset of the FFS1 super-block.
pub const SBLOCK_UFS1: i32 = 8192;
/// `SBLOCK_UFS2`: the byte offset of the FFS2 super-block.
pub const SBLOCK_UFS2: i32 = 65536;
/// `SBLOCK_PIGGY`.
pub const SBLOCK_PIGGY: i32 = 262144;
/// `SBLOCKSIZE`.
pub const SBLOCKSIZE: usize = 8192;
/// `SBLOCKSEARCH`: where to look for a super-block, in order, `-1` terminated.
pub const SBLOCKSEARCH: [i32; 4] = [SBLOCK_UFS2, SBLOCK_UFS1, SBLOCK_PIGGY, -1];

/// `MAXFRAG`: blocks may be broken into at most this many fragments.
pub const MAXFRAG: usize = 8;

/// `MINBSIZE`: the smallest allowable block size. In order to insure that it is possible to
/// create files of size 2^32 with only two levels of indirection, MINBSIZE is set to 4096.
pub const MINBSIZE: usize = 4096;

/// `MAXMNTLEN`: the space in the super-block for the name it is mounted on.
pub const MAXMNTLEN: usize = 468;

/// `MAXVOLLEN`: the length of the volume name buffer.
pub const MAXVOLLEN: usize = 32;

/// `NOCSPTRS`: the padding left of the old in-core summary pointers (128 bytes less the
/// four pointers that remain).
pub const NOCSPTRS: usize = (128 / size_of::<*mut c_void>()) - 4;

/// `FS_MAXCONTIG`: a maximum summary size of contiguous blocks.
pub const FS_MAXCONTIG: usize = 16;

/// `MINFREE`: the minimum acceptable percentage of file system blocks which may be free.
pub const MINFREE: i32 = 5;
/// `DEFAULTOPT`.
pub const DEFAULTOPT: i32 = FS_OPTTIME;

/// `AVFILESIZ`: expected average file size.
pub const AVFILESIZ: u32 = 16384;
/// `AFPDIR`: expected number of files per directory.
pub const AFPDIR: u32 = 64;

/// `FSMAXSNAP`: size of superblock space reserved for snapshots.
pub const FSMAXSNAP: usize = 20;

/// `struct csum`: per cylinder group information; summarized in blocks allocated from first
/// cylinder group data blocks.
#[repr(C)]
pub struct Csum {
    /// `cs_ndir`: number of directories.
    pub cs_ndir: Cell<i32>,
    /// `cs_nbfree`: number of free blocks.
    pub cs_nbfree: Cell<i32>,
    /// `cs_nifree`: number of free inodes.
    pub cs_nifree: Cell<i32>,
    /// `cs_nffree`: number of free frags.
    pub cs_nffree: Cell<i32>,
}

/// `struct csum_total`.
#[repr(C)]
pub struct CsumTotal {
    /// `cs_ndir`: number of directories.
    pub cs_ndir: Cell<i64>,
    /// `cs_nbfree`: number of free blocks.
    pub cs_nbfree: Cell<i64>,
    /// `cs_nifree`: number of free inodes.
    pub cs_nifree: Cell<i64>,
    /// `cs_nffree`: number of free frags.
    pub cs_nffree: Cell<i64>,
    /// `cs_spare`: future expansion.
    pub cs_spare: [Cell<i64>; 4],
}

/// `x += d` on a counter of the super-block or a cylinder group.
pub fn add<T: Copy + core::ops::Add<Output = T>>(c: &Cell<T>, d: T) {
    c.set(c.get() + d);
}

/// `x -= d`.
pub fn sub<T: Copy + core::ops::Sub<Output = T>>(c: &Cell<T>, d: T) {
    c.set(c.get() - d);
}

/// `struct fs`: super block for an FFS file system.
#[repr(C)]
pub struct Fs {
    /// `fs_firstfield`: historic file system linked list, used for incore super blocks.
    pub fs_firstfield: Cell<i32>,
    /// `fs_unused_1`.
    pub fs_unused_1: Cell<i32>,
    /// `fs_sblkno`: addr of super-block / frags.
    pub fs_sblkno: Cell<i32>,
    /// `fs_cblkno`: offset of cyl-block / frags.
    pub fs_cblkno: Cell<i32>,
    /// `fs_iblkno`: offset of inode-blocks / frags.
    pub fs_iblkno: Cell<i32>,
    /// `fs_dblkno`: offset of first data / frags.
    pub fs_dblkno: Cell<i32>,
    /// `fs_cgoffset`: cylinder group offset in cylinder.
    pub fs_cgoffset: Cell<i32>,
    /// `fs_cgmask`: used to calc mod fs_ntrak.
    pub fs_cgmask: Cell<i32>,
    /// `fs_ffs1_time`: last time written.
    pub fs_ffs1_time: Cell<i32>,
    /// `fs_ffs1_size`: # of blocks in fs / frags.
    pub fs_ffs1_size: Cell<i32>,
    /// `fs_ffs1_dsize`: # of data blocks in fs.
    pub fs_ffs1_dsize: Cell<i32>,
    /// `fs_ncg`: # of cylinder groups.
    pub fs_ncg: Cell<u32>,
    /// `fs_bsize`: size of basic blocks / bytes.
    pub fs_bsize: Cell<i32>,
    /// `fs_fsize`: size of frag blocks / bytes.
    pub fs_fsize: Cell<i32>,
    /// `fs_frag`: # of frags in a block in fs.
    pub fs_frag: Cell<i32>,
    /// `fs_minfree`: minimum percentage of free blocks.
    pub fs_minfree: Cell<i32>,
    /// `fs_rotdelay`: # of ms for optimal next block.
    pub fs_rotdelay: Cell<i32>,
    /// `fs_rps`: disk revolutions per second.
    pub fs_rps: Cell<i32>,
    /// `fs_bmask`: `blkoff` calc of blk offsets.
    pub fs_bmask: Cell<i32>,
    /// `fs_fmask`: `fragoff` calc of frag offsets.
    pub fs_fmask: Cell<i32>,
    /// `fs_bshift`: `lblkno` calc of logical blkno.
    pub fs_bshift: Cell<i32>,
    /// `fs_fshift`: `numfrags` calc # of frags.
    pub fs_fshift: Cell<i32>,
    /// `fs_maxcontig`: max # of contiguous blks.
    pub fs_maxcontig: Cell<i32>,
    /// `fs_maxbpg`: max # of blks per cyl group.
    pub fs_maxbpg: Cell<i32>,
    /// `fs_fragshift`: block to frag shift.
    pub fs_fragshift: Cell<i32>,
    /// `fs_fsbtodb`: fsbtodb and dbtofsb shift constant.
    pub fs_fsbtodb: Cell<i32>,
    /// `fs_sbsize`: actual size of super block.
    pub fs_sbsize: Cell<i32>,
    /// `fs_csmask`: csum block offset (now unused).
    pub fs_csmask: Cell<i32>,
    /// `fs_csshift`: csum block number (now unused).
    pub fs_csshift: Cell<i32>,
    /// `fs_nindir`: value of NINDIR.
    pub fs_nindir: Cell<i32>,
    /// `fs_inopb`: inodes per file system block.
    pub fs_inopb: Cell<u32>,
    /// `fs_nspf`: DEV_BSIZE sectors per frag.
    pub fs_nspf: Cell<i32>,
    /// `fs_optim`: optimization preference, see below.
    pub fs_optim: Cell<i32>,
    /// `fs_npsect`: DEV_BSIZE sectors/track + spares.
    pub fs_npsect: Cell<i32>,
    /// `fs_interleave`: DEV_BSIZE sector interleave.
    pub fs_interleave: Cell<i32>,
    /// `fs_trackskew`: sector 0 skew, per track.
    pub fs_trackskew: Cell<i32>,
    /// `fs_id`: unique filesystem id (the space of the unused `fs_headswitch`, `fs_trkseek`).
    pub fs_id: [Cell<i32>; 2],
    /// `fs_ffs1_csaddr`: blk addr of cyl grp summary area.
    pub fs_ffs1_csaddr: Cell<i32>,
    /// `fs_cssize`: cyl grp summary area size / bytes.
    pub fs_cssize: Cell<i32>,
    /// `fs_cgsize`: cyl grp block size / bytes.
    pub fs_cgsize: Cell<i32>,
    /// `fs_ntrak`: tracks per cylinder.
    pub fs_ntrak: Cell<i32>,
    /// `fs_nsect`: DEV_BSIZE sectors per track.
    pub fs_nsect: Cell<i32>,
    /// `fs_spc`: DEV_BSIZE sectors per cylinder.
    pub fs_spc: Cell<i32>,
    /// `fs_ncyl`: cylinders in file system.
    pub fs_ncyl: Cell<i32>,
    /// `fs_cpg`: cylinders per group.
    pub fs_cpg: Cell<i32>,
    /// `fs_ipg`: inodes per group.
    pub fs_ipg: Cell<u32>,
    /// `fs_fpg`: blocks per group * fs_frag.
    pub fs_fpg: Cell<i32>,
    /// `fs_ffs1_cstotal`: cylinder summary information.
    pub fs_ffs1_cstotal: Csum,
    /// `fs_fmod`: super block modified flag.
    pub fs_fmod: Cell<i8>,
    /// `fs_clean`: file system is clean flag.
    pub fs_clean: Cell<i8>,
    /// `fs_ronly`: mounted read-only flag.
    pub fs_ronly: Cell<i8>,
    /// `fs_ffs1_flags`: see `FS_` below.
    pub fs_ffs1_flags: Cell<i8>,
    /// `fs_fsmnt`: name mounted on.
    pub fs_fsmnt: Cell<[u8; MAXMNTLEN]>,
    /// `fs_volname`: volume name.
    pub fs_volname: Cell<[u8; MAXVOLLEN]>,
    /// `fs_swuid`: system-wide uid.
    pub fs_swuid: Cell<u64>,
    /// `fs_pad`: due to alignment of fs_swuid.
    pub fs_pad: Cell<i32>,
    /// `fs_cgrotor`: last cg searched.
    pub fs_cgrotor: Cell<i32>,
    /// `fs_ocsp`: padding; was list of fs_cs buffers.
    pub fs_ocsp: [Cell<*mut c_void>; NOCSPTRS],
    /// `fs_contigdirs`: # of contiguously allocated dirs.
    pub fs_contigdirs: Cell<*mut u8>,
    /// `fs_csp`: cg summary info buffer for fs_cs.
    pub fs_csp: Cell<*mut Csum>,
    /// `fs_maxcluster`: max cluster in each cyl group.
    pub fs_maxcluster: Cell<*mut i32>,
    /// `fs_active`: reserved for snapshots.
    pub fs_active: Cell<*mut u8>,
    /// `fs_cpc`: cyl per cycle in postbl.
    pub fs_cpc: Cell<i32>,
    /// `fs_maxbsize`: maximum blocking factor permitted.
    pub fs_maxbsize: Cell<i32>,
    /// `fs_spareconf64`: old rotation block list head.
    pub fs_spareconf64: [Cell<i64>; 17],
    /// `fs_sblockloc`: offset of standard super block.
    pub fs_sblockloc: Cell<i64>,
    /// `fs_cstotal`: cylinder summary information.
    pub fs_cstotal: CsumTotal,
    /// `fs_time`: time last written.
    pub fs_time: Cell<i64>,
    /// `fs_size`: number of blocks in fs.
    pub fs_size: Cell<i64>,
    /// `fs_dsize`: number of data blocks in fs.
    pub fs_dsize: Cell<i64>,
    /// `fs_csaddr`: blk addr of cyl grp summary area.
    pub fs_csaddr: Cell<i64>,
    /// `fs_pendingblocks`: blocks in process of being freed.
    pub fs_pendingblocks: Cell<i64>,
    /// `fs_pendinginodes`: inodes in process of being freed.
    pub fs_pendinginodes: Cell<u32>,
    /// `fs_snapinum`: space reserved for snapshots.
    pub fs_snapinum: [Cell<u32>; FSMAXSNAP],
    /// `fs_avgfilesize`: expected average file size.
    pub fs_avgfilesize: Cell<u32>,
    /// `fs_avgfpdir`: expected # of files per directory.
    pub fs_avgfpdir: Cell<u32>,
    /// `fs_sparecon`: reserved for future constants.
    pub fs_sparecon: [Cell<i32>; 26],
    /// `fs_flags`: see `FS_` flags below.
    pub fs_flags: Cell<u32>,
    /// `fs_fscktime`: last time fsck(8)ed.
    pub fs_fscktime: Cell<i32>,
    /// `fs_contigsumsize`: size of cluster summary array.
    pub fs_contigsumsize: Cell<i32>,
    /// `fs_maxsymlinklen`: max length of an internal symlink.
    pub fs_maxsymlinklen: Cell<i32>,
    /// `fs_inodefmt`: format of on-disk inodes.
    pub fs_inodefmt: Cell<i32>,
    /// `fs_maxfilesize`: maximum representable file size.
    pub fs_maxfilesize: Cell<u64>,
    /// `fs_qbmask`: `~fs_bmask` - for use with quad size.
    pub fs_qbmask: Cell<i64>,
    /// `fs_qfmask`: `~fs_fmask` - for use with quad size.
    pub fs_qfmask: Cell<i64>,
    /// `fs_state`: validate fs_clean field.
    pub fs_state: Cell<i32>,
    /// `fs_postblformat`: format of positional layout tables.
    pub fs_postblformat: Cell<i32>,
    /// `fs_nrpos`: number of rotational positions.
    pub fs_nrpos: Cell<i32>,
    /// `fs_postbloff`: (u_int16) rotation block list head.
    pub fs_postbloff: Cell<i32>,
    /// `fs_rotbloff`: (u_int8) blocks for each rotation.
    pub fs_rotbloff: Cell<i32>,
    /// `fs_magic`: magic number.
    pub fs_magic: Cell<i32>,
    /// `fs_space`: list of blocks for each rotation (actually longer).
    pub fs_space: [Cell<u8>; 1],
}

// SAFETY: the in-core super-block is changed under the kernel lock (and the vnode locks of
// the files being allocated), as in C.
unsafe impl Sync for Fs {}

impl Fs {
    /// `fs->fs_cs(fs, indx)`: the summary of cylinder group `indx` in the in-core array.
    pub fn fs_cs(&self, indx: u32) -> &Csum {
        let csp = self.fs_csp.get();
        if csp.is_null() || indx >= self.fs_ncg.get() {
            panic(format_args!("fs_cs: cg {} of {}", indx, self.fs_ncg.get()));
        }
        // SAFETY: `ffs_mountfs` points `fs_csp` at `fs_cssize` bytes holding one `struct
        // csum` per cylinder group (checked above), kept until the unmount frees them;
        // `Csum` is `Cell`s of integers, valid for any bytes.
        unsafe { &*csp.add(indx as usize) }
    }

    /// `fs->fs_contigdirs[cg]`.
    pub fn contigdirs(&self, cg: u32) -> u8 {
        let p = self.fs_contigdirs.get();
        if p.is_null() || cg >= self.fs_ncg.get() {
            panic(format_args!("fs_contigdirs: cg {}", cg));
        }
        // SAFETY: a read-write mount allocates `fs_ncg` bytes here (checked above).
        unsafe { *p.add(cg as usize) }
    }

    /// `fs->fs_contigdirs[cg] = v`.
    pub fn set_contigdirs(&self, cg: u32, v: u8) {
        let p = self.fs_contigdirs.get();
        if p.is_null() || cg >= self.fs_ncg.get() {
            panic(format_args!("fs_contigdirs: cg {}", cg));
        }
        // SAFETY: as in `contigdirs`.
        unsafe { *p.add(cg as usize) = v };
    }

    /// `fs->fs_maxcluster[cg] = v`.
    pub fn set_maxcluster(&self, cg: u32, v: i32) {
        let p = self.fs_maxcluster.get();
        if p.is_null() || cg >= self.fs_ncg.get() {
            panic(format_args!("fs_maxcluster: cg {}", cg));
        }
        // SAFETY: `ffs_mountfs` allocates `fs_ncg` counters after the summaries when
        // `fs_contigsumsize > 0` (checked above).
        unsafe { *p.add(cg as usize) = v };
    }

    /// `fs->fs_fsmnt` as a C string (up to its NUL).
    pub fn fsmnt(&self) -> crate::kern::subr_prf::Str<'_> {
        let name = self.fs_fsmnt.as_ptr();
        // SAFETY: a `Cell<[u8; N]>` has the array's layout; the borrow lasts as long as the
        // caller's `&self`, during which nothing writes the name (both run under the kernel lock,
        // and printing a message does not mount).
        crate::kern::subr_prf::Str(unsafe { &*name })
    }
}

/// `FS_MAGIC`: the fast filesystem magic number.
pub const FS_MAGIC: i32 = 0x011954;
/// `FS_UFS1_MAGIC`: the fast filesystem magic number.
pub const FS_UFS1_MAGIC: i32 = 0x011954;
/// `FS_UFS2_MAGIC`: UFS fast filesystem magic number.
pub const FS_UFS2_MAGIC: i32 = 0x19540119;
/// `FS_OKAY`: superblock checksum.
pub const FS_OKAY: i32 = 0x7c269d38;
/// `FS_42INODEFMT`: 4.2BSD inode format.
pub const FS_42INODEFMT: i32 = -1;
/// `FS_44INODEFMT`: 4.4BSD inode format.
pub const FS_44INODEFMT: i32 = 2;

/// `FS_ISCLEAN`.
pub const FS_ISCLEAN: i8 = 0x01;
/// `FS_WASCLEAN`.
pub const FS_WASCLEAN: i8 = 0x02;

/// `FS_OPTTIME`: minimize allocation time.
pub const FS_OPTTIME: i32 = 0;
/// `FS_OPTSPACE`: minimize disk fragmentation.
pub const FS_OPTSPACE: i32 = 1;

/// `FS_UNCLEAN`: filesystem not clean at mount.
pub const FS_UNCLEAN: u32 = 0x01;
/// `FS_FLAGS_UPDATED`: an FFS1 file system that had its flags moved to the new (FFS2)
/// location for compatibility.
pub const FS_FLAGS_UPDATED: u32 = 0x80;

/// `FS_42POSTBLFMT`: 4.2BSD rotational table format.
pub const FS_42POSTBLFMT: i32 = -1;
/// `FS_DYNAMICPOSTBLFMT`: dynamic rotational table format.
pub const FS_DYNAMICPOSTBLFMT: i32 = 1;

/// `fs_rotbl(fs)`: the byte offset in the super-block of the rotational table.
pub fn fs_rotbl(fs: &Fs) -> usize {
    if fs.fs_postblformat.get() == FS_42POSTBLFMT {
        core::mem::offset_of!(Fs, fs_space)
    } else {
        fs.fs_rotbloff.get() as usize
    }
}

/// `CGSIZE(fs)`: the size of a cylinder group block: the `struct cg`, the block totals,
/// the rotational positions, the inode and block maps and, if present, the cluster summary
/// and map.
pub fn cgsize(fs: &Fs) -> usize {
    let cpg = fs.fs_cpg.get() as usize;
    let mut size = size_of::<Cg>()
        + size_of::<i32>()
        + cpg * size_of::<i32>()
        + cpg * fs.fs_nrpos.get() as usize * size_of::<i16>()
        + howmany(fs.fs_ipg.get() as usize, 8)
        + howmany(fs.fs_fpg.get() as usize, 8);
    if fs.fs_contigsumsize.get() > 0 {
        size += fs.fs_contigsumsize.get() as usize * size_of::<i32>()
            + howmany(fragstoblks(fs, i64::from(fs.fs_fpg.get())) as usize, 8);
    }
    size
}

/// `CG_MAGIC`.
pub const CG_MAGIC: i32 = 0x090255;

/// `struct cg`: cylinder group block for a file system (actually longer: the maps follow).
#[repr(C)]
pub struct Cg {
    /// `cg_firstfield`: historic cyl groups linked list.
    pub cg_firstfield: Cell<i32>,
    /// `cg_magic`: magic number.
    pub cg_magic: Cell<i32>,
    /// `cg_time`: time last written.
    pub cg_time: Cell<i32>,
    /// `cg_cgx`: we are the cgx'th cylinder group.
    pub cg_cgx: Cell<u32>,
    /// `cg_ncyl`: number of cyl's this cg.
    pub cg_ncyl: Cell<i16>,
    /// `cg_niblk`: number of inode blocks this cg.
    pub cg_niblk: Cell<i16>,
    /// `cg_ndblk`: number of data blocks this cg.
    pub cg_ndblk: Cell<u32>,
    /// `cg_cs`: cylinder summary information.
    pub cg_cs: Csum,
    /// `cg_rotor`: position of last used block.
    pub cg_rotor: Cell<u32>,
    /// `cg_frotor`: position of last used frag.
    pub cg_frotor: Cell<u32>,
    /// `cg_irotor`: position of last used inode.
    pub cg_irotor: Cell<u32>,
    /// `cg_frsum`: counts of available frags (see the module's deviations).
    pub cg_frsum: [Cell<i32>; MAXFRAG],
    /// `cg_btotoff`: (int32) block totals per cylinder.
    pub cg_btotoff: Cell<i32>,
    /// `cg_boff`: (u_int16) free block positions.
    pub cg_boff: Cell<i32>,
    /// `cg_iusedoff`: (u_int8) used inode map.
    pub cg_iusedoff: Cell<u32>,
    /// `cg_freeoff`: (u_int8) free block map.
    pub cg_freeoff: Cell<u32>,
    /// `cg_nextfreeoff`: (u_int8) next available space.
    pub cg_nextfreeoff: Cell<u32>,
    /// `cg_clustersumoff`: (u_int32) counts of avail clusters.
    pub cg_clustersumoff: Cell<u32>,
    /// `cg_clusteroff`: (u_int8) free cluster map.
    pub cg_clusteroff: Cell<u32>,
    /// `cg_nclusterblks`: number of clusters this cg.
    pub cg_nclusterblks: Cell<u32>,
    /// `cg_ffs2_niblk`: number of inode blocks this cg.
    pub cg_ffs2_niblk: Cell<u32>,
    /// `cg_initediblk`: last initialized inode.
    pub cg_initediblk: Cell<u32>,
    /// `cg_sparecon32`: reserved for future use.
    pub cg_sparecon32: [Cell<i32>; 3],
    /// `cg_ffs2_time`: time last written.
    pub cg_ffs2_time: Cell<i64>,
    /// `cg_sparecon64`: reserved for future use.
    pub cg_sparecon64: [Cell<i64>; 3],
}

/// `struct ocg`, the layout of old file systems' cylinder groups: the offsets of its arrays,
/// which [`CgBuf`] uses when `cg_magic` is not where `struct cg` has it.
pub mod ocg {
    /// `offsetof(struct ocg, cg_btot)`: `int32_t cg_btot[32]`.
    pub const CG_BTOT: usize = 84;
    /// `offsetof(struct ocg, cg_b)`: `int16_t cg_b[32][8]`.
    pub const CG_B: usize = 212;
    /// `offsetof(struct ocg, cg_iused)`: `u_int8_t cg_iused[256]`.
    pub const CG_IUSED: usize = 724;
    /// `offsetof(struct ocg, cg_magic)`.
    pub const CG_MAGIC: usize = 980;
    /// `offsetof(struct ocg, cg_free)`: the free block map.
    pub const CG_FREE: usize = 984;
}

/// A cylinder group block in a busy buffer: `(struct cg *)bp->b_data` and the macros that
/// locate its arrays. The header is read and written through [`CgBuf::cg`]; the maps are
/// byte slices that do not overlap it.
pub struct CgBuf<'a> {
    /// The buffer's data.
    base: NonNull<u8>,
    /// Its length (`b_bcount`).
    len: usize,
    /// The buffer is borrowed for `'a`, as the C's `cgp` is valid until `brelse`/`bdwrite`.
    _buf: PhantomData<&'a mut [u8]>,
}

impl<'a> CgBuf<'a> {
    /// The cylinder group in `bp`'s data.
    ///
    /// # Safety
    ///
    /// The caller owns the busy, mapped buffer (as `bread` returns it) for `'a`, nothing
    /// else reaches its data meanwhile, and it holds at least `size_of::<Cg>()` bytes.
    pub unsafe fn new(bp: &'a Buf) -> Self {
        let len = usize::try_from(bp.b_bcount.get()).unwrap_or(0);
        let Some(base) = NonNull::new(bp.b_data.get()) else {
            panic(format_args!("CgBuf: unmapped buffer"));
        };
        if len < size_of::<Cg>() {
            panic(format_args!("CgBuf: short buffer {}", len));
        }
        Self {
            base,
            len,
            _buf: PhantomData,
        }
    }

    /// `(struct cg *)bp->b_data`: the header.
    pub fn cg(&self) -> &Cg {
        // SAFETY: the buffer holds at least a `struct cg` (`new`) at its start, which is
        // page-aligned (a buffer's mapping); `Cg` is `Cell`s of integers, valid for any bytes,
        // and the map slices handed out by the `&mut self` methods never cover these bytes.
        unsafe { &*self.base.as_ptr().cast::<Cg>() }
    }

    /// The bytes from `off` to the end of the buffer, which must lie past the header.
    fn tail(&mut self, off: usize) -> &mut [u8] {
        if off < size_of::<Cg>() || off > self.len {
            panic(format_args!("cg map offset {} out of range", off));
        }
        // SAFETY: `off..len` is inside the buffer (checked) and past the header, so it
        // aliases neither `cg()` nor another live slice (`&mut self`).
        unsafe { core::slice::from_raw_parts_mut(self.base.as_ptr().add(off), self.len - off) }
    }

    /// Whether the header is the new `struct cg` (`cgp->cg_magic == CG_MAGIC`).
    fn is_cg(&self) -> bool {
        self.cg().cg_magic.get() == CG_MAGIC
    }

    /// `cg_chkmagic(cgp)`: either layout's magic number is there.
    pub fn cg_chkmagic(&self) -> bool {
        if self.is_cg() {
            return true;
        }
        if self.len < ocg::CG_MAGIC + 4 {
            return false;
        }
        // SAFETY: the 4 bytes at `ocg::CG_MAGIC` are inside the buffer (checked) and not
        // borrowed mutably (`&self`).
        let b = unsafe { core::slice::from_raw_parts(self.base.as_ptr().add(ocg::CG_MAGIC), 4) };
        i32::from_ne_bytes([b[0], b[1], b[2], b[3]]) == CG_MAGIC
    }

    /// `cg_inosused(cgp)`: the used inode map.
    pub fn cg_inosused(&mut self) -> &mut [u8] {
        let off = if self.is_cg() {
            self.cg().cg_iusedoff.get() as usize
        } else {
            ocg::CG_IUSED
        };
        self.tail(off)
    }

    /// `cg_blksfree(cgp)`: the free block (fragment) map.
    pub fn cg_blksfree(&mut self) -> &mut [u8] {
        let off = if self.is_cg() {
            self.cg().cg_freeoff.get() as usize
        } else {
            ocg::CG_FREE
        };
        self.tail(off)
    }

    /// `cg_clustersfree(cgp)`: the free cluster map.
    pub fn cg_clustersfree(&mut self) -> &mut [u8] {
        let off = self.cg().cg_clusteroff.get() as usize;
        self.tail(off)
    }

    /// `cg_clustersum(cgp)[i]`.
    pub fn cg_clustersum(&mut self, i: usize) -> i32 {
        let off = self.cg().cg_clustersumoff.get() as usize + i * 4;
        let b = self.tail(off);
        i32::from_ne_bytes([b[0], b[1], b[2], b[3]])
    }

    /// `cg_clustersum(cgp)[i] += d`.
    pub fn cg_clustersum_add(&mut self, i: usize, d: i32) {
        let v = self.cg_clustersum(i) + d;
        let off = self.cg().cg_clustersumoff.get() as usize + i * 4;
        self.tail(off)[..4].copy_from_slice(&v.to_ne_bytes());
    }

    /// `cg_blktot(cgp)[cylno] += d` (FFS1's block totals per cylinder).
    pub fn cg_blktot_add(&mut self, cylno: usize, d: i32) {
        let off = if self.is_cg() {
            self.cg().cg_btotoff.get() as usize
        } else {
            ocg::CG_BTOT
        } + cylno * 4;
        let b = self.tail(off);
        let v = i32::from_ne_bytes([b[0], b[1], b[2], b[3]]) + d;
        b[..4].copy_from_slice(&v.to_ne_bytes());
    }

    /// `cg_blks(fs, cgp, cylno)[pos] += d` (FFS1's free block positions).
    pub fn cg_blks_add(&mut self, fs: &Fs, cylno: usize, pos: usize, d: i16) {
        let off = if self.is_cg() {
            self.cg().cg_boff.get() as usize + (cylno * fs.fs_nrpos.get() as usize + pos) * 2
        } else {
            ocg::CG_B + (cylno * 8 + pos) * 2
        };
        let b = self.tail(off);
        let v = i16::from_ne_bytes([b[0], b[1]]) + d;
        b[..2].copy_from_slice(&v.to_ne_bytes());
    }
}

/// `fsbtodb(fs, b)`: file system blocks (fragments) to `DEV_BSIZE` disk blocks.
pub fn fsbtodb(fs: &Fs, b: Daddr) -> Daddr {
    b << fs.fs_fsbtodb.get()
}

/// `dbtofsb(fs, b)`.
pub fn dbtofsb(fs: &Fs, b: Daddr) -> Daddr {
    b >> fs.fs_fsbtodb.get()
}

/// `cgbase(fs, c)`: the first fragment of cylinder group `c`.
pub fn cgbase(fs: &Fs, c: u32) -> Daddr {
    Daddr::from(fs.fs_fpg.get()) * Daddr::from(c)
}

/// `cgdata(fs, c)`: data zone.
pub fn cgdata(fs: &Fs, c: u32) -> Daddr {
    cgdmin(fs, c) + Daddr::from(fs.fs_minfree.get())
}

/// `cgmeta(fs, c)`: meta data.
pub fn cgmeta(fs: &Fs, c: u32) -> Daddr {
    cgdmin(fs, c)
}

/// `cgdmin(fs, c)`: 1st data.
pub fn cgdmin(fs: &Fs, c: u32) -> Daddr {
    cgstart(fs, c) + Daddr::from(fs.fs_dblkno.get())
}

/// `cgimin(fs, c)`: inode blk.
pub fn cgimin(fs: &Fs, c: u32) -> Daddr {
    cgstart(fs, c) + Daddr::from(fs.fs_iblkno.get())
}

/// `cgsblock(fs, c)`: super blk.
pub fn cgsblock(fs: &Fs, c: u32) -> Daddr {
    cgstart(fs, c) + Daddr::from(fs.fs_sblkno.get())
}

/// `cgtod(fs, c)`: cg block.
pub fn cgtod(fs: &Fs, c: u32) -> Daddr {
    cgstart(fs, c) + Daddr::from(fs.fs_cblkno.get())
}

/// `cgstart(fs, c)`.
pub fn cgstart(fs: &Fs, c: u32) -> Daddr {
    cgbase(fs, c)
        + Daddr::from(fs.fs_cgoffset.get()) * Daddr::from(c & !(fs.fs_cgmask.get() as u32))
}

/// `ino_to_cg(fs, x)`: inode number to cylinder group number.
pub fn ino_to_cg(fs: &Fs, x: Ufsino) -> u32 {
    x / fs.fs_ipg.get()
}

/// `ino_to_fsba(fs, x)`: inode number to file system block address.
pub fn ino_to_fsba(fs: &Fs, x: Ufsino) -> Daddr {
    cgimin(fs, ino_to_cg(fs, x)) + blkstofrags(fs, Daddr::from((x % fs.fs_ipg.get()) / inopb(fs)))
}

/// `ino_to_fsbo(fs, x)`: inode number to file system block offset.
pub fn ino_to_fsbo(fs: &Fs, x: Ufsino) -> usize {
    (x % inopb(fs)) as usize
}

/// `dtog(fs, d)`: the cylinder group number of a file system block.
pub fn dtog(fs: &Fs, d: Daddr) -> u32 {
    (d / Daddr::from(fs.fs_fpg.get())) as u32
}

/// `dtogd(fs, d)`: the frag block number in its cylinder group of a file system block.
pub fn dtogd(fs: &Fs, d: Daddr) -> Daddr {
    d % Daddr::from(fs.fs_fpg.get())
}

/// `blkmap(fs, map, loc)`: the bits for the block at fragment `loc` in a map.
pub fn blkmap(fs: &Fs, map: &[u8], loc: Daddr) -> i32 {
    let loc = loc as usize;
    (i32::from(map[loc / 8]) >> (loc % 8)) & (0xff >> (8 - fs.fs_frag.get()))
}

/// `cbtocylno(fs, bno)`: the cylinder of a cylinder group block address.
pub fn cbtocylno(fs: &Fs, bno: Daddr) -> usize {
    (fsbtodb(fs, bno) / Daddr::from(fs.fs_spc.get())) as usize
}

/// `cbtorpos(fs, bno)`: the rotational position of a cylinder group block address.
pub fn cbtorpos(fs: &Fs, bno: Daddr) -> usize {
    if fs.fs_nrpos.get() <= 1 {
        return 0;
    }
    let db = fsbtodb(fs, bno);
    let spc = Daddr::from(fs.fs_spc.get());
    let nsect = Daddr::from(fs.fs_nsect.get());
    ((db % spc / nsect * Daddr::from(fs.fs_trackskew.get())
        + db % spc % nsect * Daddr::from(fs.fs_interleave.get()))
        % nsect
        * Daddr::from(fs.fs_nrpos.get())
        / Daddr::from(fs.fs_npsect.get())) as usize
}

/// `blkoff(fs, loc)`: calculates `loc % fs->fs_bsize`.
pub fn blkoff(fs: &Fs, loc: Off) -> Off {
    loc & fs.fs_qbmask.get()
}

/// `fragoff(fs, loc)`: calculates `loc % fs->fs_fsize`.
pub fn fragoff(fs: &Fs, loc: Off) -> Off {
    loc & fs.fs_qfmask.get()
}

/// `lblktosize(fs, blk)`: calculates `(off_t)blk * fs->fs_bsize`.
pub fn lblktosize(fs: &Fs, blk: Daddr) -> Off {
    blk << fs.fs_bshift.get()
}

/// `lblkno(fs, loc)`: calculates `loc / fs->fs_bsize`.
pub fn lblkno(fs: &Fs, loc: Off) -> Daddr {
    loc >> fs.fs_bshift.get()
}

/// `numfrags(fs, loc)`: calculates `loc / fs->fs_fsize`.
pub fn numfrags(fs: &Fs, loc: i64) -> i64 {
    loc >> fs.fs_fshift.get()
}

/// `blkroundup(fs, size)`: calculates `roundup(size, fs->fs_bsize)`.
pub fn blkroundup(fs: &Fs, size: i64) -> i64 {
    (size + fs.fs_qbmask.get()) & i64::from(fs.fs_bmask.get())
}

/// `fragroundup(fs, size)`: calculates `roundup(size, fs->fs_fsize)`.
pub fn fragroundup(fs: &Fs, size: i64) -> i64 {
    (size + fs.fs_qfmask.get()) & i64::from(fs.fs_fmask.get())
}

/// `fragstoblks(fs, frags)`: calculates `frags / fs->fs_frag`.
pub fn fragstoblks(fs: &Fs, frags: Daddr) -> Daddr {
    frags >> fs.fs_fragshift.get()
}

/// `blkstofrags(fs, blks)`: calculates `blks * fs->fs_frag`.
pub fn blkstofrags(fs: &Fs, blks: Daddr) -> Daddr {
    blks << fs.fs_fragshift.get()
}

/// `fragnum(fs, fsb)`: calculates `fsb % fs->fs_frag`.
pub fn fragnum(fs: &Fs, fsb: Daddr) -> Daddr {
    fsb & Daddr::from(fs.fs_frag.get() - 1)
}

/// `blknum(fs, fsb)`: calculates `rounddown(fsb, fs->fs_frag)`.
pub fn blknum(fs: &Fs, fsb: Daddr) -> Daddr {
    fsb & !Daddr::from(fs.fs_frag.get() - 1)
}

/// `freespace(fs, percentreserved)`: the number of available frags given a percentage to
/// hold in reserve.
pub fn freespace(fs: &Fs, percentreserved: i32) -> i64 {
    blkstofrags(fs, fs.fs_cstotal.cs_nbfree.get()) + fs.fs_cstotal.cs_nffree.get()
        - (fs.fs_dsize.get() * i64::from(percentreserved) / 100)
}

/// `blksize(fs, ip, lbn)`: the size of a file block in the file system: a whole block, or
/// the fragments the last direct block of a small file holds.
pub fn blksize(fs: &Fs, ip: &Inode, lbn: Daddr) -> u64 {
    sblksize(fs, ip.dip_size(), lbn)
}

/// `dblksize(fs, dip, lbn)`: as `blksize` for a dinode's `di_size`.
pub fn dblksize(fs: &Fs, di_size: u64, lbn: Daddr) -> u64 {
    sblksize(fs, di_size, lbn)
}

/// `sblksize(fs, size, lbn)`: as `blksize` for a file of `size` bytes.
pub fn sblksize(fs: &Fs, size: u64, lbn: Daddr) -> u64 {
    if lbn >= NDADDR as Daddr || size >= ((lbn + 1) << fs.fs_bshift.get()) as u64 {
        fs.fs_bsize.get() as u64
    } else {
        fragroundup(fs, blkoff(fs, size as Off)) as u64
    }
}

/// `NSPB(fs)`: number of disk sectors per block; assumes `DEV_BSIZE` byte sector size.
pub fn nspb(fs: &Fs) -> i32 {
    fs.fs_nspf.get() << fs.fs_fragshift.get()
}

/// `NSPF(fs)`: number of disk sectors per fragment.
pub fn nspf(fs: &Fs) -> i32 {
    fs.fs_nspf.get()
}

/// `INOPB(fs)`: number of inodes per file system block (`fs->fs_bsize`).
pub fn inopb(fs: &Fs) -> u32 {
    fs.fs_inopb.get()
}

/// `INOPF(fs)`: number of inodes per file system fragment (`fs->fs_fsize`).
pub fn inopf(fs: &Fs) -> u32 {
    fs.fs_inopb.get() >> fs.fs_fragshift.get()
}

/// `NINDIR(fs)`: number of indirects in a file system block.
pub fn nindir(fs: &Fs) -> i64 {
    i64::from(fs.fs_nindir.get())
}

/// `FS_KERNMAXFILESIZE(pgsiz, fs)`: the maximum file size the kernel allows. Even though ffs
/// can handle files up to 16TB, we do limit the max file to 2^31 pages to prevent overflow of
/// a 32-bit unsigned int.
pub fn fs_kernmaxfilesize(pgsiz: u64, fs: &Fs) -> u64 {
    0x8000_0000u64 * pgsiz.min(fs.fs_bsize.get() as u64) - 1
}

// The amd64/arm64 layout of `struct fs` and `struct cg` (both LP64), as the C compiler lays
// them out.
const _: () = {
    use core::mem::offset_of;
    assert!(size_of::<Csum>() == 16);
    assert!(size_of::<CsumTotal>() == 64);
    assert!(NOCSPTRS == 12);
    assert!(offset_of!(Fs, fs_sblkno) == 8);
    assert!(offset_of!(Fs, fs_ncg) == 44);
    assert!(offset_of!(Fs, fs_bsize) == 48);
    assert!(offset_of!(Fs, fs_minfree) == 60);
    assert!(offset_of!(Fs, fs_bmask) == 72);
    assert!(offset_of!(Fs, fs_fragshift) == 96);
    assert!(offset_of!(Fs, fs_sbsize) == 104);
    assert!(offset_of!(Fs, fs_nindir) == 116);
    assert!(offset_of!(Fs, fs_optim) == 128);
    assert!(offset_of!(Fs, fs_id) == 144);
    assert!(offset_of!(Fs, fs_ffs1_csaddr) == 152);
    assert!(offset_of!(Fs, fs_cgsize) == 160);
    assert!(offset_of!(Fs, fs_ipg) == 184);
    assert!(offset_of!(Fs, fs_fpg) == 188);
    assert!(offset_of!(Fs, fs_ffs1_cstotal) == 192);
    assert!(offset_of!(Fs, fs_fmod) == 208);
    assert!(offset_of!(Fs, fs_clean) == 209);
    assert!(offset_of!(Fs, fs_ronly) == 210);
    assert!(offset_of!(Fs, fs_ffs1_flags) == 211);
    assert!(offset_of!(Fs, fs_fsmnt) == 212);
    assert!(offset_of!(Fs, fs_volname) == 680);
    assert!(offset_of!(Fs, fs_swuid) == 712);
    assert!(offset_of!(Fs, fs_cgrotor) == 724);
    assert!(offset_of!(Fs, fs_ocsp) == 728);
    assert!(offset_of!(Fs, fs_contigdirs) == 824);
    assert!(offset_of!(Fs, fs_csp) == 832);
    assert!(offset_of!(Fs, fs_maxcluster) == 840);
    assert!(offset_of!(Fs, fs_active) == 848);
    assert!(offset_of!(Fs, fs_cpc) == 856);
    assert!(offset_of!(Fs, fs_maxbsize) == 860);
    assert!(offset_of!(Fs, fs_spareconf64) == 864);
    assert!(offset_of!(Fs, fs_sblockloc) == 1000);
    assert!(offset_of!(Fs, fs_cstotal) == 1008);
    assert!(offset_of!(Fs, fs_time) == 1072);
    assert!(offset_of!(Fs, fs_size) == 1080);
    assert!(offset_of!(Fs, fs_dsize) == 1088);
    assert!(offset_of!(Fs, fs_csaddr) == 1096);
    assert!(offset_of!(Fs, fs_pendinginodes) == 1112);
    assert!(offset_of!(Fs, fs_snapinum) == 1116);
    assert!(offset_of!(Fs, fs_avgfilesize) == 1196);
    assert!(offset_of!(Fs, fs_sparecon) == 1204);
    assert!(offset_of!(Fs, fs_flags) == 1308);
    assert!(offset_of!(Fs, fs_contigsumsize) == 1316);
    assert!(offset_of!(Fs, fs_maxsymlinklen) == 1320);
    assert!(offset_of!(Fs, fs_inodefmt) == 1324);
    assert!(offset_of!(Fs, fs_maxfilesize) == 1328);
    assert!(offset_of!(Fs, fs_qbmask) == 1336);
    assert!(offset_of!(Fs, fs_qfmask) == 1344);
    assert!(offset_of!(Fs, fs_state) == 1352);
    assert!(offset_of!(Fs, fs_postblformat) == 1356);
    assert!(offset_of!(Fs, fs_nrpos) == 1360);
    assert!(offset_of!(Fs, fs_rotbloff) == 1368);
    assert!(offset_of!(Fs, fs_magic) == 1372);
    assert!(offset_of!(Fs, fs_space) == 1376);
    assert!(size_of::<Fs>() == 1384);
    assert!(offset_of!(Cg, cg_magic) == 4);
    assert!(offset_of!(Cg, cg_cs) == 24);
    assert!(offset_of!(Cg, cg_rotor) == 40);
    assert!(offset_of!(Cg, cg_frsum) == 52);
    assert!(offset_of!(Cg, cg_btotoff) == 84);
    assert!(offset_of!(Cg, cg_iusedoff) == 92);
    assert!(offset_of!(Cg, cg_ffs2_niblk) == 116);
    assert!(offset_of!(Cg, cg_initediblk) == 120);
    assert!(offset_of!(Cg, cg_ffs2_time) == 136);
    assert!(size_of::<Cg>() == 168);
};
/* </CODE> */
