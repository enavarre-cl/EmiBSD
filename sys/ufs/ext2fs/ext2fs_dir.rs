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

/* <CODE> */
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
//! - A `struct ext2fs_direct *` into a directory block is an offset into the block's bytes:
//!   [`e2d_ino`], [`e2d_reclen`], [`e2d_namlen`], [`e2d_type`], [`e2d_name`] and their setters
//!   read and write the members there, converting from and to little-endian (the C's
//!   `letoh32`/`htole16` at each use). [`Ext2fsDirect::write_to`] and the
//!   [`Ext2fsDirtemplate`] byte conversions are the C's `memcpy`s of the structures.

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

impl Ext2fsDirect {
    /// `offsetof(struct ext2fs_direct, e2d_name)`: the size of the fixed header.
    pub const NAME_OFFSET: usize = 8;

    /// `memcpy(ep, &newdir, len)`: the first `len` bytes of the entry, little-endian, at
    /// `off` in `b`.
    pub fn write_to(&self, b: &mut [u8], off: usize, len: usize) {
        let mut h = [0u8; Self::NAME_OFFSET];
        h[0..4].copy_from_slice(&self.e2d_ino.to_le_bytes());
        h[4..6].copy_from_slice(&self.e2d_reclen.to_le_bytes());
        h[6] = self.e2d_namlen;
        h[7] = self.e2d_type;
        let hl = len.min(Self::NAME_OFFSET);
        b[off..off + hl].copy_from_slice(&h[..hl]);
        if len > Self::NAME_OFFSET {
            let n = len - Self::NAME_OFFSET;
            b[off + Self::NAME_OFFSET..off + len].copy_from_slice(&self.e2d_name[..n]);
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

impl Ext2fsDirtemplate {
    /// `sizeof(struct ext2fs_dirtemplate)`.
    pub const SIZE: usize = size_of::<Ext2fsDirtemplate>();

    /// The structure as it lies on the disk (little-endian).
    pub fn to_le_bytes(&self) -> [u8; Self::SIZE] {
        let mut b = [0u8; Self::SIZE];
        b[0..4].copy_from_slice(&self.dot_ino.to_le_bytes());
        b[4..6].copy_from_slice(&self.dot_reclen.to_le_bytes());
        b[6] = self.dot_namlen;
        b[7] = self.dot_type;
        b[8..12].copy_from_slice(&self.dot_name);
        b[12..16].copy_from_slice(&self.dotdot_ino.to_le_bytes());
        b[16..18].copy_from_slice(&self.dotdot_reclen.to_le_bytes());
        b[18] = self.dotdot_namlen;
        b[19] = self.dotdot_type;
        b[20..24].copy_from_slice(&self.dotdot_name);
        b
    }

    /// The structure from the first [`Self::SIZE`] bytes of `b` (little-endian).
    pub fn from_le_bytes(b: &[u8]) -> Self {
        let u32_at = |o: usize| u32::from_le_bytes([b[o], b[o + 1], b[o + 2], b[o + 3]]);
        let i16_at = |o: usize| i16::from_le_bytes([b[o], b[o + 1]]);
        let name_at = |o: usize| [b[o], b[o + 1], b[o + 2], b[o + 3]];
        Self {
            dot_ino: u32_at(0),
            dot_reclen: i16_at(4),
            dot_namlen: b[6],
            dot_type: b[7],
            dot_name: name_at(8),
            dotdot_ino: u32_at(12),
            dotdot_reclen: i16_at(16),
            dotdot_namlen: b[18],
            dotdot_type: b[19],
            dotdot_name: name_at(20),
        }
    }
}

/// `letoh32(ep->e2d_ino)` of the entry at `off` in `b` (0 past the end).
pub fn e2d_ino(b: &[u8], off: usize) -> u32 {
    match b.get(off..off + 4) {
        Some(s) => u32::from_le_bytes([s[0], s[1], s[2], s[3]]),
        None => 0,
    }
}

/// `ep->e2d_ino = htole32(v)`.
pub fn set_e2d_ino(b: &mut [u8], off: usize, v: u32) {
    b[off..off + 4].copy_from_slice(&v.to_le_bytes());
}

/// `letoh16(ep->e2d_reclen)` (0 past the end).
pub fn e2d_reclen(b: &[u8], off: usize) -> u16 {
    match b.get(off + 4..off + 6) {
        Some(s) => u16::from_le_bytes([s[0], s[1]]),
        None => 0,
    }
}

/// `ep->e2d_reclen = htole16(v)`.
pub fn set_e2d_reclen(b: &mut [u8], off: usize, v: u16) {
    b[off + 4..off + 6].copy_from_slice(&v.to_le_bytes());
}

/// `ep->e2d_namlen` (0 past the end).
pub fn e2d_namlen(b: &[u8], off: usize) -> u8 {
    b.get(off + 6).copied().unwrap_or(0)
}

/// `ep->e2d_type` (0 past the end).
pub fn e2d_type(b: &[u8], off: usize) -> u8 {
    b.get(off + 7).copied().unwrap_or(0)
}

/// `ep->e2d_type = v`.
pub fn set_e2d_type(b: &mut [u8], off: usize, v: u8) {
    b[off + 7] = v;
}

/// `ep->e2d_name`: the `len` bytes of the name (fewer if the block ends first).
pub fn e2d_name(b: &[u8], off: usize, len: usize) -> &[u8] {
    let start = (off + Ext2fsDirect::NAME_OFFSET).min(b.len());
    let end = (start + len).min(b.len());
    &b[start..end]
}

const _: () = {
    assert!(size_of::<Ext2fsDirect>() == 264);
    assert!(core::mem::offset_of!(Ext2fsDirect, e2d_name) == Ext2fsDirect::NAME_OFFSET);
    assert!(size_of::<Ext2fsDirtemplate>() == 24);
};
/* </CODE> */

/* <TESTS> */
#[cfg(test)]
mod tests {
    // Host tests for `<ufs/ext2fs/ext2fs_dir.h>`.

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

    #[test]
    fn entries_are_read_and_written_little_endian_in_place() {
        let mut b = [0xffu8; 64];
        let mut e = Ext2fsDirect::new();
        e.e2d_ino = 0x0102_0304;
        e.e2d_reclen = 0x0506;
        e.e2d_namlen = 5;
        e.e2d_type = EXT2_FT_DIR;
        e.e2d_name[..5].copy_from_slice(b"hello");
        e.write_to(&mut b, 4, ext2fs_dirsiz(5));
        assert_eq!(&b[4..12], &[4, 3, 2, 1, 6, 5, 5, EXT2_FT_DIR]);
        assert_eq!(&b[12..20], b"hello\0\0\0");
        assert_eq!(b[20], 0xff, "nothing past the entry");
        assert_eq!(e2d_ino(&b, 4), 0x0102_0304);
        assert_eq!(e2d_reclen(&b, 4), 0x0506);
        assert_eq!((e2d_namlen(&b, 4), e2d_type(&b, 4)), (5, EXT2_FT_DIR));
        assert_eq!(e2d_name(&b, 4, 5), b"hello");
        set_e2d_ino(&mut b, 4, 7);
        set_e2d_reclen(&mut b, 4, 12);
        set_e2d_type(&mut b, 4, EXT2_FT_REG_FILE);
        assert_eq!(&b[4..12], &[7, 0, 0, 0, 12, 0, 5, EXT2_FT_REG_FILE]);
        // Past the end: zeroes, and a name cut at the end.
        assert_eq!(
            (e2d_ino(&b, 62), e2d_reclen(&b, 60), e2d_namlen(&b, 64)),
            (0, 0, 0)
        );
        assert_eq!(e2d_name(&b, 50, 10).len(), 6);
    }

    #[test]
    fn the_template_round_trips_through_its_disk_bytes() {
        let t = Ext2fsDirtemplate {
            dot_ino: 12,
            dot_reclen: 12,
            dot_namlen: 1,
            dot_type: EXT2_FT_DIR,
            dot_name: *b".\0\0\0",
            dotdot_ino: 2,
            dotdot_reclen: 1012,
            dotdot_namlen: 2,
            dotdot_type: EXT2_FT_DIR,
            dotdot_name: *b"..\0\0",
        };
        let b = t.to_le_bytes();
        assert_eq!(e2d_ino(&b, 0), 12);
        assert_eq!(e2d_reclen(&b, 12), 1012);
        assert_eq!(e2d_name(&b, 12, 2), b"..");
        let u = Ext2fsDirtemplate::from_le_bytes(&b);
        assert_eq!(u.to_le_bytes(), b);
        assert_eq!((u.dotdot_ino, u.dotdot_reclen), (2, 1012));
    }
}
/* </TESTS> */
