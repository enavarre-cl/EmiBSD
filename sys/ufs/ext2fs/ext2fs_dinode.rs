/*	$OpenBSD: ext2fs_dinode.h,v 1.17 2014/07/31 17:37:52 pelikan Exp $	*/
/*	$NetBSD: ext2fs_dinode.h,v 1.6 2000/01/26 16:21:33 bouyer Exp $	*/
/* <LICENSES> */
/*
 * Copyright (c) 1997 Manuel Bouyer.
 * Copyright (c) 1982, 1989, 1993
 *	The Regents of the University of California.  All rights reserved.
 * (c) UNIX System Laboratories, Inc.
 * All or some portions of this file are derived from material licensed
 * to the University of California by American Telephone and Telegraph
 * Co. or Unix System Laboratories, Inc. and are reproduced herein with
 * the permission of UNIX System Laboratories, Inc.
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
 *	@(#)dinode.h	8.6 (Berkeley) 9/13/94
 *  Modified for ext2fs by Manuel Bouyer.
 */
/* </LICENSES> */

//! `<ufs/ext2fs/ext2fs_dinode.h>`: the on-disk inode of an ext2fs (`struct ext2fs_dinode`), its
//! file types, permissions and flags.
//!
//! Upstream: sys/ufs/ext2fs/ext2fs_dinode.h @ 3ce1f3f79392
//!
//! A dinode contains all the meta-data associated with a UFS file. This structure defines the
//! on-disk format of a dinode. Since this structure describes an on-disk structure, all its
//! fields are defined by types with precise widths.
//!
//! ## Deviations
//! - `struct ext2fs_dinode` is the plain `#[repr(C)]` image [`Ext2fsDinode`] (156 bytes, with
//!   the large-inode fields of the C; the first 128 are the revision 0 inode), its size
//!   pinned below. The in-core inode points at one (`inode.rs`'s `i_e2din`).
//! - `e2di_rdev` and `e2di_shortlink` (macros for `e2di_blocks[0]` and `e2di_blocks`) are the
//!   methods [`Ext2fsDinode::e2di_rdev`], [`Ext2fsDinode::set_e2di_rdev`],
//!   [`Ext2fsDinode::e2di_shortlink`] and [`Ext2fsDinode::e2di_shortlink_mut`].
//! - `EXT2_DINODE_SIZE(fs)` is [`ext2_dinode_size`]; `e2fs_iload`/`e2fs_isave` are functions
//!   that are always compiled: they copy `MIN(EXT2_DINODE_SIZE(fs), sizeof(*new))` bytes on a
//!   little-endian machine and go through `e2fs_i_bswap` (`ext2fs_bswap.rs`) on a big-endian
//!   one, where the C picks one with `#if BYTE_ORDER`. They take the buffer's bytes, not a
//!   `struct ext2fs_dinode *` into it.
//! - `NDADDR` and `NIADDR` (also defined by `ufs/ufs/dinode.h`) are re-exported from there.

use core::cmp::min;
use core::mem::size_of;
use core::ptr;

use crate::kern::subr_prf::panic;
use crate::ufs::ext2fs::ext2fs::MExt2fs;
use crate::ufs::ext2fs::ext2fs_bswap::e2fs_i_bswap;
use crate::ufs::ufs::dinode::Ufsino;

pub use crate::ufs::ufs::dinode::{NDADDR, NIADDR};

/// `EXT2_ROOTINO`: the root inode is the root of the file system. Inode 0 can not be used for
/// normal purposes and bad blocks are normally linked to inode 1, thus the root inode is 2.
/// Inode 3 to 10 are reserved in ext2fs.
pub const EXT2_ROOTINO: Ufsino = 2;
/// `EXT2_RESIZEINO`.
pub const EXT2_RESIZEINO: Ufsino = 7;
/// `EXT2_FIRSTINO`.
pub const EXT2_FIRSTINO: Ufsino = 11;

/// `EXT2_MAXSYMLINKLEN`.
pub const EXT2_MAXSYMLINKLEN: usize = (NDADDR + NIADDR) * size_of::<u32>();
/// `E2MAXSYMLINKLEN`.
pub const E2MAXSYMLINKLEN: usize = (NDADDR + NIADDR) * size_of::<u32>();

/// `struct ext2fs_dinode`: the on-disk inode (little-endian on disk).
#[repr(C)]
#[allow(non_snake_case)] // `e2di__reserved`, the C's name with its double underscore
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Ext2fsDinode {
    /// `e2di_mode`: 0: IFMT, permissions; see below.
    pub e2di_mode: u16,
    /// `e2di_uid_low`: 2: owner UID, bits 15:0.
    pub e2di_uid_low: u16,
    /// `e2di_size`: 4: file size (bytes) bits 31:0.
    pub e2di_size: u32,
    /// `e2di_atime`: 8: access time.
    pub e2di_atime: u32,
    /// `e2di_ctime`: 12: change time.
    pub e2di_ctime: u32,
    /// `e2di_mtime`: 16: modification time.
    pub e2di_mtime: u32,
    /// `e2di_dtime`: 20: deletion time.
    pub e2di_dtime: u32,
    /// `e2di_gid_low`: 24: owner GID, lowest bits.
    pub e2di_gid_low: u16,
    /// `e2di_nlink`: 26: file link count.
    pub e2di_nlink: u16,
    /// `e2di_nblock`: 28: blocks count.
    pub e2di_nblock: u32,
    /// `e2di_flags`: 32: status flags (chflags).
    pub e2di_flags: u32,
    /// `e2di_version_lo`: 36: inode version, bits 31:0.
    pub e2di_version_lo: u32,
    /// `e2di_blocks`: 40: disk blocks.
    pub e2di_blocks: [u32; NDADDR + NIADDR],
    /// `e2di_gen`: 100: generation number.
    pub e2di_gen: u32,
    /// `e2di_facl`: 104: file ACL, bits 31:0.
    pub e2di_facl: u32,
    /// `e2di_size_hi`: 108: file size (bytes), bits 63:32.
    pub e2di_size_hi: u32,
    /// `e2di_faddr`: 112: fragment address (obsolete).
    pub e2di_faddr: u32,
    /// `e2di_nblock_hi`: 116: blocks count, bits 47:32.
    pub e2di_nblock_hi: u16,
    /// `e2di_facl_hi`: 118: file ACL, bits 47:32.
    pub e2di_facl_hi: u16,
    /// `e2di_uid_high`: 120: owner UID, bits 31:16.
    pub e2di_uid_high: u16,
    /// `e2di_gid_high`: 122: owner GID, bits 31:16.
    pub e2di_gid_high: u16,
    /// `e2di_chksum_lo`: 124: inode checksum, bits 15:0.
    pub e2di_chksum_lo: u16,
    /// `e2di__reserved`: 126: unused.
    pub e2di__reserved: u16,
    /// `e2di_isize`: 128: size of this inode.
    pub e2di_isize: u16,
    /// `e2di_chksum_hi`: 130: inode checksum, bits 31:16.
    pub e2di_chksum_hi: u16,
    /// `e2di_x_ctime`: 132: extra change time.
    pub e2di_x_ctime: u32,
    /// `e2di_x_mtime`: 136: extra modification time.
    pub e2di_x_mtime: u32,
    /// `e2di_x_atime`: 140: extra access time.
    pub e2di_x_atime: u32,
    /// `e2di_crtime`: 144: creation (birth) time.
    pub e2di_crtime: u32,
    /// `e2di_x_crtime`: 148: extra creation (birth) time.
    pub e2di_x_crtime: u32,
    /// `e2di_version_hi`: 152: inode version, bits 63:31.
    pub e2di_version_hi: u32,
}

impl Ext2fsDinode {
    /// `e2di_rdev` (`e2di_blocks[0]`): block and character devices overlay the first data
    /// block with their `dev_t` value.
    pub fn e2di_rdev(&self) -> u32 {
        self.e2di_blocks[0]
    }

    /// `e2di_rdev = v`.
    pub fn set_e2di_rdev(&mut self, v: u32) {
        self.e2di_blocks[0] = v;
    }

    /// `e2di_shortlink` (`e2di_blocks`): a short symbolic link places its path in the block
    /// pointers' area, `EXT2_MAXSYMLINKLEN` bytes.
    pub fn e2di_shortlink(&self) -> &[u8] {
        // SAFETY: `e2di_blocks` is `EXT2_MAXSYMLINKLEN` initialised bytes, `u8` has no
        // alignment requirement, and the slice borrows `self`.
        unsafe {
            core::slice::from_raw_parts(self.e2di_blocks.as_ptr().cast::<u8>(), EXT2_MAXSYMLINKLEN)
        }
    }

    /// `e2di_shortlink`, writable.
    pub fn e2di_shortlink_mut(&mut self) -> &mut [u8] {
        // SAFETY: as in `e2di_shortlink`; every byte pattern is a valid `u32`.
        unsafe {
            core::slice::from_raw_parts_mut(
                self.e2di_blocks.as_mut_ptr().cast::<u8>(),
                EXT2_MAXSYMLINKLEN,
            )
        }
    }
}

/// `EXT2_IEXEC`: executable.
pub const EXT2_IEXEC: u16 = 0o000100;
/// `EXT2_IWRITE`: writeable.
pub const EXT2_IWRITE: u16 = 0o000200;
/// `EXT2_IREAD`: readable.
pub const EXT2_IREAD: u16 = 0o000400;
/// `EXT2_ISVTX`: sticky bit.
pub const EXT2_ISVTX: u16 = 0o001000;
/// `EXT2_ISGID`: set-gid.
pub const EXT2_ISGID: u16 = 0o002000;
/// `EXT2_ISUID`: set-uid.
pub const EXT2_ISUID: u16 = 0o004000;

/// `EXT2_IFMT`: mask of file type.
pub const EXT2_IFMT: u16 = 0o170000;
/// `EXT2_IFIFO`: named pipe (fifo).
pub const EXT2_IFIFO: u16 = 0o010000;
/// `EXT2_IFCHR`: character device.
pub const EXT2_IFCHR: u16 = 0o020000;
/// `EXT2_IFDIR`: directory file.
pub const EXT2_IFDIR: u16 = 0o040000;
/// `EXT2_IFBLK`: block device.
pub const EXT2_IFBLK: u16 = 0o060000;
/// `EXT2_IFREG`: regular file.
pub const EXT2_IFREG: u16 = 0o100000;
/// `EXT2_IFLNK`: symbolic link.
pub const EXT2_IFLNK: u16 = 0o120000;
/// `EXT2_IFSOCK`: UNIX domain socket.
pub const EXT2_IFSOCK: u16 = 0o140000;

/// `EXT2_SECRM`: secure deletion.
pub const EXT2_SECRM: u32 = 0x0000_0001;
/// `EXT2_UNRM`: undelete.
pub const EXT2_UNRM: u32 = 0x0000_0002;
/// `EXT2_COMPR`: compress file.
pub const EXT2_COMPR: u32 = 0x0000_0004;
/// `EXT2_SYNC`: synchronous updates.
pub const EXT2_SYNC: u32 = 0x0000_0008;
/// `EXT2_IMMUTABLE`: immutable file.
pub const EXT2_IMMUTABLE: u32 = 0x0000_0010;
/// `EXT2_APPEND`: writes to file may only append.
pub const EXT2_APPEND: u32 = 0x0000_0020;
/// `EXT2_NODUMP`: do not dump file.
pub const EXT2_NODUMP: u32 = 0x0000_0040;
/// `EXT2_NOATIME`: do not update access time.
pub const EXT2_NOATIME: u32 = 0x0000_0080;
/// `EXT4_INDEX`: hash-indexed directory.
pub const EXT4_INDEX: u32 = 0x0000_1000;
/// `EXT4_JOURNAL_DATA`: file data should be journaled.
pub const EXT4_JOURNAL_DATA: u32 = 0x0000_4000;
/// `EXT4_DIRSYNC`: all dirent updates done synchronously.
pub const EXT4_DIRSYNC: u32 = 0x0001_0000;
/// `EXT4_TOPDIR`: top of directory hierarchies.
pub const EXT4_TOPDIR: u32 = 0x0002_0000;
/// `EXT4_HUGE_FILE`: nblocks unit is fsb, not db.
pub const EXT4_HUGE_FILE: u32 = 0x0004_0000;
/// `EXT4_EXTENTS`: inode uses extents.
pub const EXT4_EXTENTS: u32 = 0x0008_0000;
/// `EXT4_EOFBLOCKS`: blocks allocated beyond EOF.
pub const EXT4_EOFBLOCKS: u32 = 0x0040_0000;

/// `EXT2_REV0_DINODE_SIZE`: size of the revision 0 on-disk inode.
pub const EXT2_REV0_DINODE_SIZE: usize = 128;

/// `EXT2_DINODE_SIZE(fs)`: the size of an on-disk inode.
pub fn ext2_dinode_size(fs: &MExt2fs) -> usize {
    fs.dinode_size()
}

/// `e2fs_iload(fs, old, new)`: loads the inode in the buffer bytes `old` into `new`:
/// `MIN(EXT2_DINODE_SIZE(fs), sizeof(*new))` bytes, little-endian on disk.
pub fn e2fs_iload(fs: &MExt2fs, old: &[u8], new: &mut Ext2fsDinode) {
    let n = min(ext2_dinode_size(fs), size_of::<Ext2fsDinode>());
    let Some(src) = old.get(..n) else {
        panic(format_args!("e2fs_iload: {} bytes, want {}", old.len(), n));
    };
    if cfg!(target_endian = "big") {
        let mut disk = Ext2fsDinode::default();
        copy_into(&mut disk, src);
        e2fs_i_bswap(fs, &disk, new);
    } else {
        copy_into(new, src);
    }
}

/// `e2fs_isave(fs, old, new)`: stores the inode `old` in the buffer bytes `new`:
/// `MIN(EXT2_DINODE_SIZE(fs), sizeof(*old))` bytes, little-endian on disk.
pub fn e2fs_isave(fs: &MExt2fs, old: &Ext2fsDinode, new: &mut [u8]) {
    let n = min(ext2_dinode_size(fs), size_of::<Ext2fsDinode>());
    let Some(dst) = new.get_mut(..n) else {
        panic(format_args!("e2fs_isave: {} bytes, want {}", new.len(), n));
    };
    let mut disk = *old;
    if cfg!(target_endian = "big") {
        e2fs_i_bswap(fs, old, &mut disk);
    }
    // SAFETY: `disk` is `size_of::<Ext2fsDinode>()` >= `n` initialised bytes (the struct has
    // no padding: its size is the sum of its fields', checked below), and `dst` is `n` bytes
    // that cannot overlap the local `disk`.
    unsafe { ptr::copy_nonoverlapping(ptr::from_ref(&disk).cast::<u8>(), dst.as_mut_ptr(), n) }
}

/// Copies `src` over the first bytes of `dino`.
fn copy_into(dino: &mut Ext2fsDinode, src: &[u8]) {
    debug_assert!(src.len() <= size_of::<Ext2fsDinode>());
    // SAFETY: `src.len()` is at most the size of `Ext2fsDinode` (the callers cut it to
    // that), the destination is exclusively borrowed so the two cannot overlap, and the struct
    // is integers only, so any bytes make a valid value.
    unsafe { ptr::copy_nonoverlapping(src.as_ptr(), ptr::from_mut(dino).cast::<u8>(), src.len()) }
}

const _: () = {
    assert!(size_of::<Ext2fsDinode>() == 156);
    assert!(core::mem::offset_of!(Ext2fsDinode, e2di_blocks) == 40);
    assert!(core::mem::offset_of!(Ext2fsDinode, e2di_gen) == 100);
    assert!(core::mem::offset_of!(Ext2fsDinode, e2di_nblock_hi) == 116);
    assert!(core::mem::offset_of!(Ext2fsDinode, e2di_isize) == 128);
    assert!(core::mem::offset_of!(Ext2fsDinode, e2di_version_hi) == 152);
};

#[cfg(test)]
mod tests;
