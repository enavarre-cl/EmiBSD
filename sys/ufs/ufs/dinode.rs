/* <LICENSES> */
/*
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
 *	@(#)dinode.h	8.9 (Berkeley) 3/29/95
 */
/* </LICENSES> */

/* <CODE> */
//! `<ufs/ufs/dinode.h>`: the on-disk inode (`struct ufs1_dinode`, `struct ufs2_dinode`), the
//! root inode number, the number of block addresses an inode holds, and the file type and
//! permission bits of `di_mode`.
//!
//! Upstream: sys/ufs/ufs/dinode.h @ 3ce1f3f79392
//!
//! A dinode contains all the meta-data associated with a UFS file. These structures define
//! the on-disk format, so all their fields have precise widths and `#[repr(C)]` keeps the
//! C layout (pinned by the compile-time checks below and the reference-backed test that reads
//! the offsets the C header writes in its comments).
//!
//! ## Deviations
//! - `di_u` (the union of FFS1's `oldids` and LFS's `inumber`) is one `[u16; 2]` member,
//!   `di_u`; `di_ouid`/`di_ogid` read and write its halves, `di_inumber` its 32-bit view.
//! - `di_rdev` and `di_shortlink` (the `di_db` overlays) are spelled `di_db[0]` and the bytes
//!   of `di_db` at their use, through the inode's accessors (`ufs/ufs/inode.rs`).
//! - `MAXSYMLINKLEN(ip)` is `Inode::maxsymlinklen`.

use crate::sys::types::Mode;

/// `ufsino_t`: UFS directories use 32bit inode numbers internally, regardless of what the
/// system on top of it uses.
pub type Ufsino = u32;

/// `ROOTINO`: the root inode is the root of the file system. Inode 0 can't be used for normal
/// purposes and historically bad blocks were linked to inode 1, thus the root inode is 2.
/// (Inode 1 is no longer used for this purpose, however numerous dump tapes make this
/// assumption, so we are stuck with it.)
pub const ROOTINO: Ufsino = 2;

/// `NXADDR`: external addresses in inode.
pub const NXADDR: usize = 2;
/// `NDADDR`: direct addresses in inode.
pub const NDADDR: usize = 12;
/// `NIADDR`: indirect addresses in inode.
pub const NIADDR: usize = 3;

/// `struct ufs1_dinode`.
#[repr(C)]
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Ufs1Dinode {
    /// `di_mode`: IFMT, permissions; see below.
    pub di_mode: u16,
    /// `di_nlink`: file link count.
    pub di_nlink: i16,
    /// `di_u`: Ffs: old user and group ids (`oldids`); Lfs: inode number (`inumber`).
    pub di_u: [u16; 2],
    /// `di_size`: file byte count.
    pub di_size: u64,
    /// `di_atime`: last access time.
    pub di_atime: i32,
    /// `di_atimensec`: last access time.
    pub di_atimensec: i32,
    /// `di_mtime`: last modified time.
    pub di_mtime: i32,
    /// `di_mtimensec`: last modified time.
    pub di_mtimensec: i32,
    /// `di_ctime`: last inode change time.
    pub di_ctime: i32,
    /// `di_ctimensec`: last inode change time.
    pub di_ctimensec: i32,
    /// `di_db`: direct disk blocks.
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
    /// `di_spare`: reserved; currently unused.
    pub di_spare: [i32; 2],
}

impl Ufs1Dinode {
    /// `di_ouid` (`di_u.oldids[0]`).
    pub fn di_ouid(&self) -> u16 {
        self.di_u[0]
    }

    /// `di_ogid` (`di_u.oldids[1]`).
    pub fn di_ogid(&self) -> u16 {
        self.di_u[1]
    }

    /// `di_inumber` (`di_u.inumber`), the 32-bit view of the union.
    pub fn di_inumber(&self) -> u32 {
        let lo = self.di_u[0].to_ne_bytes();
        let hi = self.di_u[1].to_ne_bytes();
        u32::from_ne_bytes([lo[0], lo[1], hi[0], hi[1]])
    }
}

/// `struct ufs2_dinode`.
#[repr(C)]
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Ufs2Dinode {
    /// `di_mode`: IFMT, permissions; see below.
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
    /// `di_atime`: last access time.
    pub di_atime: i64,
    /// `di_mtime`: last modified time.
    pub di_mtime: i64,
    /// `di_ctime`: last inode change time.
    pub di_ctime: i64,
    /// `di_birthtime`: inode creation time.
    pub di_birthtime: i64,
    /// `di_mtimensec`: last modified time.
    pub di_mtimensec: i32,
    /// `di_atimensec`: last access time.
    pub di_atimensec: i32,
    /// `di_ctimensec`: last inode change time.
    pub di_ctimensec: i32,
    /// `di_birthnsec`: inode creation time.
    pub di_birthnsec: i32,
    /// `di_gen`: generation number.
    pub di_gen: i32,
    /// `di_kernflags`: kernel flags.
    pub di_kernflags: u32,
    /// `di_flags`: status flags (chflags).
    pub di_flags: u32,
    /// `di_extsize`: external attributes block.
    pub di_extsize: i32,
    /// `di_extb`: external attributes block.
    pub di_extb: [i64; NXADDR],
    /// `di_db`: direct disk blocks.
    pub di_db: [i64; NDADDR],
    /// `di_ib`: indirect disk blocks.
    pub di_ib: [i64; NIADDR],
    /// `di_spare`: reserved; currently unused.
    pub di_spare: [i64; 3],
}

/// `MAXSYMLINKLEN_UFS1`: the bytes of `di_db` and `di_ib` a short symbolic link may use.
pub const MAXSYMLINKLEN_UFS1: usize = (NDADDR + NIADDR) * size_of::<i32>();
/// `MAXSYMLINKLEN_UFS2`.
pub const MAXSYMLINKLEN_UFS2: usize = (NDADDR + NIADDR) * size_of::<i64>();

// File permissions.
/// `IEXEC`: executable.
pub const IEXEC: Mode = 0o000100;
/// `IWRITE`: writeable.
pub const IWRITE: Mode = 0o000200;
/// `IREAD`: readable.
pub const IREAD: Mode = 0o000400;
/// `ISVTX`: sticky bit.
pub const ISVTX: Mode = 0o001000;
/// `ISGID`: set-gid.
pub const ISGID: Mode = 0o002000;
/// `ISUID`: set-uid.
pub const ISUID: Mode = 0o004000;

// File types.
/// `IFMT`: mask of file type.
pub const IFMT: Mode = 0o170000;
/// `IFIFO`: named pipe (fifo).
pub const IFIFO: Mode = 0o010000;
/// `IFCHR`: character device.
pub const IFCHR: Mode = 0o020000;
/// `IFDIR`: directory file.
pub const IFDIR: Mode = 0o040000;
/// `IFBLK`: block device.
pub const IFBLK: Mode = 0o060000;
/// `IFREG`: regular file.
pub const IFREG: Mode = 0o100000;
/// `IFLNK`: symbolic link.
pub const IFLNK: Mode = 0o120000;
/// `IFSOCK`: UNIX domain socket.
pub const IFSOCK: Mode = 0o140000;
/// `IFWHT`: whiteout.
pub const IFWHT: Mode = 0o160000;

// The on-disk layout: the offsets the C header writes in its comments.
const _: () = {
    use core::mem::offset_of;
    assert!(size_of::<Ufs1Dinode>() == 128);
    assert!(offset_of!(Ufs1Dinode, di_size) == 8);
    assert!(offset_of!(Ufs1Dinode, di_db) == 40);
    assert!(offset_of!(Ufs1Dinode, di_ib) == 88);
    assert!(offset_of!(Ufs1Dinode, di_flags) == 100);
    assert!(offset_of!(Ufs1Dinode, di_spare) == 120);
    assert!(size_of::<Ufs2Dinode>() == 256);
    assert!(offset_of!(Ufs2Dinode, di_size) == 16);
    assert!(offset_of!(Ufs2Dinode, di_birthtime) == 56);
    assert!(offset_of!(Ufs2Dinode, di_gen) == 80);
    assert!(offset_of!(Ufs2Dinode, di_extb) == 96);
    assert!(offset_of!(Ufs2Dinode, di_db) == 112);
    assert!(offset_of!(Ufs2Dinode, di_ib) == 208);
    assert!(offset_of!(Ufs2Dinode, di_spare) == 232);
};
/* </CODE> */

/* <TESTS> */
#[cfg(test)]
mod tests {
    // Tests for `<ufs/ufs/dinode.h>`: the constants and the dinode layouts against the C header.

    use core::mem::offset_of;
    use std::collections::BTreeMap;
    use std::string::{String, ToString};
    use std::{assert_eq, format};

    use super::*;

    /// The `NAME: offset` pairs the header writes in the comments of `struct name { ... };`
    /// (`int32_t di_db[NDADDR]; /* 40: Direct disk blocks. */`).
    fn commented_offsets(text: &str, name: &str) -> BTreeMap<String, usize> {
        let start = format!("struct\t{name} {{");
        let mut out = BTreeMap::new();
        let mut inside = false;
        for line in text.lines() {
            if line.starts_with(&start) || line.starts_with(&format!("struct {name} {{")) {
                inside = true;
                continue;
            }
            if !inside {
                continue;
            }
            if line.starts_with("};") {
                break;
            }
            let Some((decl, comment)) = line.split_once("/*") else {
                continue;
            };
            let Some((off, _)) = comment.trim().split_once(':') else {
                continue;
            };
            let Ok(off) = off.trim().parse::<usize>() else {
                continue;
            };
            // `u_int16_t di_mode;` -> the identifier is the last word before any `[`.
            let ident = decl.trim().trim_end_matches(';');
            let ident = ident.split('[').next().unwrap_or("");
            let ident = ident.split_whitespace().last().unwrap_or("");
            if !ident.is_empty() {
                out.insert(ident.to_string(), off);
            }
        }
        out
    }

    #[test]
    #[ignore = "needs OPENBSD_SRC (just test-ref)"]
    fn constants_match_the_c_header() {
        let defs = crate::reftest::defines("sys/ufs/ufs/dinode.h");
        for (name, value) in [
            ("NXADDR", NXADDR as i64),
            ("NDADDR", NDADDR as i64),
            ("NIADDR", NIADDR as i64),
            ("IEXEC", i64::from(IEXEC)),
            ("ISVTX", i64::from(ISVTX)),
            ("ISUID", i64::from(ISUID)),
            ("IFMT", i64::from(IFMT)),
            ("IFIFO", i64::from(IFIFO)),
            ("IFDIR", i64::from(IFDIR)),
            ("IFREG", i64::from(IFREG)),
            ("IFLNK", i64::from(IFLNK)),
            ("IFWHT", i64::from(IFWHT)),
        ] {
            assert_eq!(crate::reftest::int(&defs, name), Some(value), "{name}");
        }
        // ROOTINO is `((ufsino_t)2)`.
        assert_eq!(
            defs.get("ROOTINO").map(String::as_str),
            Some("((ufsino_t)2)")
        );
    }

    #[test]
    #[ignore = "needs OPENBSD_SRC (just test-ref)"]
    fn dinode_layouts_match_the_offsets_in_the_c_header() {
        let path = crate::reftest::openbsd_src().join("sys/ufs/ufs/dinode.h");
        let text = std::fs::read_to_string(path).expect("dinode.h");

        let ufs1 = commented_offsets(&text, "ufs1_dinode");
        let want1 = [
            ("di_mode", offset_of!(Ufs1Dinode, di_mode)),
            ("di_nlink", offset_of!(Ufs1Dinode, di_nlink)),
            ("di_size", offset_of!(Ufs1Dinode, di_size)),
            ("di_atime", offset_of!(Ufs1Dinode, di_atime)),
            ("di_atimensec", offset_of!(Ufs1Dinode, di_atimensec)),
            ("di_mtime", offset_of!(Ufs1Dinode, di_mtime)),
            ("di_mtimensec", offset_of!(Ufs1Dinode, di_mtimensec)),
            ("di_ctime", offset_of!(Ufs1Dinode, di_ctime)),
            ("di_ctimensec", offset_of!(Ufs1Dinode, di_ctimensec)),
            ("di_db", offset_of!(Ufs1Dinode, di_db)),
            ("di_ib", offset_of!(Ufs1Dinode, di_ib)),
            ("di_flags", offset_of!(Ufs1Dinode, di_flags)),
            ("di_blocks", offset_of!(Ufs1Dinode, di_blocks)),
            ("di_gen", offset_of!(Ufs1Dinode, di_gen)),
            ("di_uid", offset_of!(Ufs1Dinode, di_uid)),
            ("di_gid", offset_of!(Ufs1Dinode, di_gid)),
            ("di_spare", offset_of!(Ufs1Dinode, di_spare)),
        ];
        for (name, off) in want1 {
            assert_eq!(ufs1.get(name), Some(&off), "ufs1_dinode.{name}");
        }

        let ufs2 = commented_offsets(&text, "ufs2_dinode");
        let want2 = [
            ("di_mode", offset_of!(Ufs2Dinode, di_mode)),
            ("di_nlink", offset_of!(Ufs2Dinode, di_nlink)),
            ("di_uid", offset_of!(Ufs2Dinode, di_uid)),
            ("di_gid", offset_of!(Ufs2Dinode, di_gid)),
            ("di_blksize", offset_of!(Ufs2Dinode, di_blksize)),
            ("di_size", offset_of!(Ufs2Dinode, di_size)),
            ("di_blocks", offset_of!(Ufs2Dinode, di_blocks)),
            ("di_atime", offset_of!(Ufs2Dinode, di_atime)),
            ("di_mtime", offset_of!(Ufs2Dinode, di_mtime)),
            ("di_ctime", offset_of!(Ufs2Dinode, di_ctime)),
            ("di_birthtime", offset_of!(Ufs2Dinode, di_birthtime)),
            ("di_mtimensec", offset_of!(Ufs2Dinode, di_mtimensec)),
            ("di_atimensec", offset_of!(Ufs2Dinode, di_atimensec)),
            ("di_ctimensec", offset_of!(Ufs2Dinode, di_ctimensec)),
            ("di_birthnsec", offset_of!(Ufs2Dinode, di_birthnsec)),
            ("di_gen", offset_of!(Ufs2Dinode, di_gen)),
            ("di_kernflags", offset_of!(Ufs2Dinode, di_kernflags)),
            ("di_flags", offset_of!(Ufs2Dinode, di_flags)),
            ("di_extsize", offset_of!(Ufs2Dinode, di_extsize)),
            ("di_extb", offset_of!(Ufs2Dinode, di_extb)),
            ("di_db", offset_of!(Ufs2Dinode, di_db)),
            ("di_ib", offset_of!(Ufs2Dinode, di_ib)),
            ("di_spare", offset_of!(Ufs2Dinode, di_spare)),
        ];
        for (name, off) in want2 {
            assert_eq!(ufs2.get(name), Some(&off), "ufs2_dinode.{name}");
        }
        assert_eq!(ufs2.len(), want2.len());
    }

    #[test]
    fn the_ufs1_union_views_share_the_bytes() {
        let mut d = Ufs1Dinode::default();
        d.di_u = [7, 9];
        assert_eq!((d.di_ouid(), d.di_ogid()), (7, 9));
        let mut b = [0u8; 4];
        b[..2].copy_from_slice(&7u16.to_ne_bytes());
        b[2..].copy_from_slice(&9u16.to_ne_bytes());
        assert_eq!(d.di_inumber(), u32::from_ne_bytes(b));
        assert_eq!(MAXSYMLINKLEN_UFS1, 60);
        assert_eq!(MAXSYMLINKLEN_UFS2, 120);
    }
}
/* </TESTS> */
