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
//! `<ufs/ufs/dinode.h>` for libsa: the FFS1 and FFS2 on-disk inodes.

use super::types::Ufsino;

/// `ROOTINO`: the root directory's inode.
pub const ROOTINO: Ufsino = 2;
/// `NXADDR`: external addresses in an FFS2 inode.
pub const NXADDR: usize = 2;
/// `NDADDR`: direct addresses in an inode.
pub const NDADDR: usize = 12;
/// `NIADDR`: indirect addresses in an inode.
pub const NIADDR: usize = 3;
/// `IFMT`: mask of file type.
pub const IFMT: u16 = 0o170000;
/// `IFDIR`: directory.
pub const IFDIR: u16 = 0o040000;
/// `IFREG`: regular file.
pub const IFREG: u16 = 0o100000;
/// `IFLNK`: symbolic link.
pub const IFLNK: u16 = 0o120000;

/// `struct ufs1_dinode`.
#[repr(C)]
#[derive(Clone, Copy, Default, Debug)]
pub struct Ufs1Dinode {
    /// `di_mode`: IFMT, permissions.
    pub di_mode: u16,
    /// `di_nlink`: file link count.
    pub di_nlink: i16,
    /// `di_u`: FFS's old user and group ids (a union with LFS's inode number).
    pub di_u: u32,
    /// `di_size`: file byte count.
    pub di_size: u64,
    /// `di_atime`.
    pub di_atime: i32,
    /// `di_atimensec`.
    pub di_atimensec: i32,
    /// `di_mtime`.
    pub di_mtime: i32,
    /// `di_mtimensec`.
    pub di_mtimensec: i32,
    /// `di_ctime`.
    pub di_ctime: i32,
    /// `di_ctimensec`.
    pub di_ctimensec: i32,
    /// `di_db`: direct disk blocks (`di_shortlink` when a short symbolic link).
    pub di_db: [i32; NDADDR],
    /// `di_ib`: indirect disk blocks.
    pub di_ib: [i32; NIADDR],
    /// `di_flags`: status flags (chflags).
    pub di_flags: u32,
    /// `di_blocks`: blocks actually held.
    pub di_blocks: i32,
    /// `di_gen`: generation number.
    pub di_gen: u32,
    /// `di_uid`: file owner.
    pub di_uid: u32,
    /// `di_gid`: file group.
    pub di_gid: u32,
    /// `di_spare`.
    pub di_spare: [i32; 2],
}

/// `struct ufs2_dinode`.
#[repr(C)]
#[derive(Clone, Copy, Default, Debug)]
pub struct Ufs2Dinode {
    /// `di_mode`: IFMT, permissions.
    pub di_mode: u16,
    /// `di_nlink`: file link count.
    pub di_nlink: i16,
    /// `di_uid`: file owner.
    pub di_uid: u32,
    /// `di_gid`: file group.
    pub di_gid: u32,
    /// `di_blksize`: inode blocksize.
    pub di_blksize: u32,
    /// `di_size`: file byte count.
    pub di_size: u64,
    /// `di_blocks`: bytes actually held.
    pub di_blocks: u64,
    /// `di_atime`.
    pub di_atime: i64,
    /// `di_mtime`.
    pub di_mtime: i64,
    /// `di_ctime`.
    pub di_ctime: i64,
    /// `di_birthtime`.
    pub di_birthtime: i64,
    /// `di_mtimensec`.
    pub di_mtimensec: i32,
    /// `di_atimensec`.
    pub di_atimensec: i32,
    /// `di_ctimensec`.
    pub di_ctimensec: i32,
    /// `di_birthnsec`.
    pub di_birthnsec: i32,
    /// `di_gen`: generation number.
    pub di_gen: i32,
    /// `di_kernflags`.
    pub di_kernflags: u32,
    /// `di_flags`: status flags (chflags).
    pub di_flags: u32,
    /// `di_extsize`.
    pub di_extsize: i32,
    /// `di_extb`: external attributes blocks.
    pub di_extb: [i64; NXADDR],
    /// `di_db`: direct disk blocks (`di_shortlink` when a short symbolic link).
    pub di_db: [i64; NDADDR],
    /// `di_ib`: indirect disk blocks.
    pub di_ib: [i64; NIADDR],
    /// `di_spare`.
    pub di_spare: [i64; 3],
}

const _: () = assert!(core::mem::size_of::<Ufs1Dinode>() == 128);
const _: () = assert!(core::mem::size_of::<Ufs2Dinode>() == 256);
/* </CODE> */
