/*	$OpenBSD: ext2fs_dir.h,v 1.12 2024/01/09 03:16:00 guenther Exp $	*/
/*	$NetBSD: ext2fs_dir.h,v 1.4 2000/01/28 16:00:23 bouyer Exp $	*/
/* <LICENSES> */
/*
 * Copyright (c) 1997 Manuel Bouyer.
 * Copyright (c) 1982, 1986, 1989, 1993
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
 *	@(#)dir.h	8.4 (Berkeley) 8/10/94
 * Modified for ext2fs by Manuel Bouyer.
 */
/* </LICENSES> */

//! `<ufs/ext2fs/ext2fs_dir.h>`: the format of an ext2fs directory (`struct ext2fs_direct`), the
//! directory entry file types and their conversion from inode modes.
//!
//! Upstream: sys/ufs/ext2fs/ext2fs_dir.h @ 3ce1f3f79392
//!
//! A directory consists of some number of blocks of `e2fs_bsize` bytes. Each block contains
//! some number of directory entry structures, which are of variable length. Each directory
//! entry has a `struct ext2fs_direct` at the front of it, containing its inode number, the
//! length of the entry, and the length of the name contained in the entry. These are followed
//! by the name padded to a 4 byte boundary with null bytes. All names are guaranteed null
//! terminated. The maximum length of a name in a directory is `EXT2FS_MAXNAMLEN`.
//!
//! [`ext2fs_dirsiz`] gives the amount of space required to represent a directory entry. Free
//! space in a directory is represented by entries which have `dp->e2d_reclen >
//! EXT2FS_DIRSIZ(dp->e2d_namlen)`. All `e2fs_bsize` bytes in a directory block are claimed by
//! the directory entries. This usually results in the last entry in a directory having a large
//! `dp->e2d_reclen`. When entries are deleted from a directory, the space is returned to the
//! previous entry in the same directory block by increasing its `dp->e2d_reclen`. If the first
//! entry of a directory block is free, then its `dp->e2d_ino` is set to 0. Entries other than
//! the first in a directory do not normally have `dp->e2d_ino` set to 0.
//!
//! Ext2 rev 0 has a 16 bits `e2d_namlen`. For Ext2 rev 1 this has been split into an 8 bits
//! `e2d_namlen` and 8 bits `e2d_type`. It is safe to use this for rev 0 as well because all
//! ext2 are little-endian.
//!
//! ## Deviations
//! - `doff_t` is [`Doff`] (`ufs/ufs/dir.rs`), the same `int32_t`.
//! - `enum slotstatus` is [`Slotstatus`] with the variants `None`, `Compact` and `Found` (the
//!   C's `NONE`, `COMPACT`, `FOUND`).
//! - `E2IFTODT` and `EXT2FS_DIRSIZ` are the `const fn`s [`e2iftodt`] and [`ext2fs_dirsiz`];
//!   `inot2ext2dt` keeps its name.

use core::mem::size_of;

use crate::ufs::ext2fs::ext2fs_dinode::{
    EXT2_IFBLK, EXT2_IFCHR, EXT2_IFDIR, EXT2_IFIFO, EXT2_IFLNK, EXT2_IFREG, EXT2_IFSOCK,
};
pub use crate::ufs::ufs::dir::Doff;

/// `EXT2FS_MAXDIRSIZE`: directories can theoretically be more than 2Gb in length; in practice
/// this seems unlikely, so offsets are 32-bit.
pub const EXT2FS_MAXDIRSIZE: i32 = 0x7fff_ffff;

/// `EXT2FS_MAXNAMLEN`: the maximum length of a name in a directory.
pub const EXT2FS_MAXNAMLEN: usize = 255;

/// `struct ext2fs_direct`: a directory entry.
#[repr(C)]
#[derive(Clone, Copy, Debug)]
pub struct Ext2fsDirect {
    /// `e2d_ino`: inode number of entry.
    pub e2d_ino: u32,
    /// `e2d_reclen`: length of this record.
    pub e2d_reclen: u16,
    /// `e2d_namlen`: length of string in `e2d_name`.
    pub e2d_namlen: u8,
    /// `e2d_type`: file type.
    pub e2d_type: u8,
    /// `e2d_name`: name with length <= `EXT2FS_MAXNAMLEN`.
    pub e2d_name: [u8; EXT2FS_MAXNAMLEN],
}

impl Ext2fsDirect {
    /// A zeroed entry.
    pub const fn new() -> Self {
        Self {
            e2d_ino: 0,
            e2d_reclen: 0,
            e2d_namlen: 0,
            e2d_type: 0,
            e2d_name: [0; EXT2FS_MAXNAMLEN],
        }
    }
}

impl Default for Ext2fsDirect {
    fn default() -> Self {
        Self::new()
    }
}

/// `enum slotstatus`: how far a directory search for free space got.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum Slotstatus {
    /// `NONE`.
    #[default]
    None,
    /// `COMPACT`.
    Compact,
    /// `FOUND`.
    Found,
}

/// `struct ext2fs_searchslot`.
#[derive(Clone, Copy, Debug, Default)]
pub struct Ext2fsSearchslot {
    /// `slotstatus`.
    pub slotstatus: Slotstatus,
    /// `slotoffset`: offset of area with free space.
    pub slotoffset: Doff,
    /// `slotsize`: size of area at `slotoffset`.
    pub slotsize: i32,
    /// `slotfreespace`: amount of space free in slot.
    pub slotfreespace: i32,
    /// `slotneeded`: sizeof the entry we are seeking.
    pub slotneeded: i32,
}

/// `EXT2_FT_UNKNOWN`: Ext2 directory file types (not the same as FFS).
pub const EXT2_FT_UNKNOWN: u8 = 0;
/// `EXT2_FT_REG_FILE`.
pub const EXT2_FT_REG_FILE: u8 = 1;
/// `EXT2_FT_DIR`.
pub const EXT2_FT_DIR: u8 = 2;
/// `EXT2_FT_CHRDEV`.
pub const EXT2_FT_CHRDEV: u8 = 3;
/// `EXT2_FT_BLKDEV`.
pub const EXT2_FT_BLKDEV: u8 = 4;
/// `EXT2_FT_FIFO`.
pub const EXT2_FT_FIFO: u8 = 5;
/// `EXT2_FT_SOCK`.
pub const EXT2_FT_SOCK: u8 = 6;
/// `EXT2_FT_SYMLINK`.
pub const EXT2_FT_SYMLINK: u8 = 7;

/// `EXT2_FT_MAX`.
pub const EXT2_FT_MAX: u8 = 8;

/// `E2IFTODT(mode)`: the file type bits of an inode mode, as a small number.
pub const fn e2iftodt(mode: u16) -> u16 {
    (mode & 0o170000) >> 12
}

const DT_FIFO: u16 = e2iftodt(EXT2_IFIFO);
const DT_CHR: u16 = e2iftodt(EXT2_IFCHR);
const DT_DIR: u16 = e2iftodt(EXT2_IFDIR);
const DT_BLK: u16 = e2iftodt(EXT2_IFBLK);
const DT_REG: u16 = e2iftodt(EXT2_IFREG);
const DT_LNK: u16 = e2iftodt(EXT2_IFLNK);
const DT_SOCK: u16 = e2iftodt(EXT2_IFSOCK);

/// `inot2ext2dt(type)`: the directory entry file type (`EXT2_FT_*`) of an [`e2iftodt`] file
/// type.
pub fn inot2ext2dt(r#type: u16) -> u8 {
    match r#type {
        DT_FIFO => EXT2_FT_FIFO,
        DT_CHR => EXT2_FT_CHRDEV,
        DT_DIR => EXT2_FT_DIR,
        DT_BLK => EXT2_FT_BLKDEV,
        DT_REG => EXT2_FT_REG_FILE,
        DT_LNK => EXT2_FT_SYMLINK,
        DT_SOCK => EXT2_FT_SOCK,
        _ => 0,
    }
}

/// `EXT2FS_DIRSIZ(len)`: the minimum record length which will hold the directory entry for a
/// name of length `len` (without the terminating null byte): the space in `struct
/// ext2fs_direct` without the name field, plus enough space for the name, rounded up to a 4
/// byte boundary.
pub const fn ext2fs_dirsiz(len: usize) -> usize {
    (8 + len + 3) & !3
}

/// `struct ext2fs_dirtemplate`: template for manipulating directories. It should use `struct
/// ext2fs_direct`s, but the name field is `EXT2FS_MAXNAMLEN - 1`, and this just does not do.
#[repr(C)]
#[derive(Clone, Copy, Debug, Default)]
pub struct Ext2fsDirtemplate {
    /// `dot_ino`.
    pub dot_ino: u32,
    /// `dot_reclen`.
    pub dot_reclen: i16,
    /// `dot_namlen`.
    pub dot_namlen: u8,
    /// `dot_type`.
    pub dot_type: u8,
    /// `dot_name`: must be multiple of 4.
    pub dot_name: [u8; 4],
    /// `dotdot_ino`.
    pub dotdot_ino: u32,
    /// `dotdot_reclen`.
    pub dotdot_reclen: i16,
    /// `dotdot_namlen`.
    pub dotdot_namlen: u8,
    /// `dotdot_type`.
    pub dotdot_type: u8,
    /// `dotdot_name`: ditto.
    pub dotdot_name: [u8; 4],
}

const _: () = {
    assert!(size_of::<Ext2fsDirect>() == 264);
    assert!(size_of::<Ext2fsDirtemplate>() == 24);
};

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn dirsiz_rounds_up_to_four_bytes() {
        assert_eq!(ext2fs_dirsiz(0), 8);
        assert_eq!(ext2fs_dirsiz(1), 12);
        assert_eq!(ext2fs_dirsiz(4), 12);
        assert_eq!(ext2fs_dirsiz(5), 16);
        assert_eq!(ext2fs_dirsiz(255), 264);
    }

    #[test]
    fn modes_map_to_directory_types() {
        for (mode, ft) in [
            (EXT2_IFIFO, EXT2_FT_FIFO),
            (EXT2_IFCHR, EXT2_FT_CHRDEV),
            (EXT2_IFDIR, EXT2_FT_DIR),
            (EXT2_IFBLK, EXT2_FT_BLKDEV),
            (EXT2_IFREG, EXT2_FT_REG_FILE),
            (EXT2_IFLNK, EXT2_FT_SYMLINK),
            (EXT2_IFSOCK, EXT2_FT_SOCK),
            (0o030000, EXT2_FT_UNKNOWN),
            (0, EXT2_FT_UNKNOWN),
        ] {
            assert_eq!(inot2ext2dt(e2iftodt(mode | 0o644)), ft, "{mode:o}");
        }
        assert_eq!(e2iftodt(EXT2_IFDIR), 4);
    }

    #[test]
    #[ignore = "needs OPENBSD_SRC (just test-ref)"]
    fn constants_match_the_c_header() {
        let defs = crate::reftest::defines("sys/ufs/ext2fs/ext2fs_dir.h");
        for (name, value) in [
            ("EXT2FS_MAXDIRSIZE", i64::from(EXT2FS_MAXDIRSIZE)),
            ("EXT2FS_MAXNAMLEN", EXT2FS_MAXNAMLEN as i64),
            ("EXT2_FT_UNKNOWN", i64::from(EXT2_FT_UNKNOWN)),
            ("EXT2_FT_REG_FILE", i64::from(EXT2_FT_REG_FILE)),
            ("EXT2_FT_DIR", i64::from(EXT2_FT_DIR)),
            ("EXT2_FT_CHRDEV", i64::from(EXT2_FT_CHRDEV)),
            ("EXT2_FT_BLKDEV", i64::from(EXT2_FT_BLKDEV)),
            ("EXT2_FT_FIFO", i64::from(EXT2_FT_FIFO)),
            ("EXT2_FT_SOCK", i64::from(EXT2_FT_SOCK)),
            ("EXT2_FT_SYMLINK", i64::from(EXT2_FT_SYMLINK)),
            ("EXT2_FT_MAX", i64::from(EXT2_FT_MAX)),
        ] {
            assert_eq!(crate::reftest::int(&defs, name), Some(value), "{name}");
        }
    }
}
