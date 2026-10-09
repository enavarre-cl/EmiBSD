/* <LICENSES> */
/*
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
 */
/* </LICENSES> */

/* <CODE> */
//! `<ufs/ufs/dir.h>`: the on-disk directory format. A directory consists of some number of
//! blocks of `DIRBLKSIZ` bytes, where `DIRBLKSIZ` is chosen such that it can be transferred
//! to disk in a single atomic operation (e.g. 512 bytes on most machines).
//!
//! Upstream: sys/ufs/ufs/dir.h @ 3ce1f3f79392
//!
//! Each `DIRBLKSIZ` byte block contains some number of directory entry structures, which are
//! of variable length. Each directory entry has a `struct direct` at the front of it,
//! containing its inode number, the length of the entry, and the length of the name contained
//! in the entry. These are followed by the name padded to a 4 byte boundary with null bytes.
//! All names are guaranteed null terminated. The maximum length of a name in a directory is
//! `MAXNAMLEN`.
//!
//! The macro `DIRSIZ(dp)` gives the amount of space required to represent a directory entry.
//! Free space in a directory is represented by entries which have `dp->d_reclen >
//! DIRSIZ(dp)`. All `DIRBLKSIZ` bytes in a directory block are claimed by the directory
//! entries. This usually results in the last entry in a directory having a large
//! `dp->d_reclen`. When entries are deleted from a directory, the space is returned to the
//! previous entry in the same directory block by increasing its `dp->d_reclen`. If the first
//! entry of a directory block is free, then its `dp->d_ino` is set to 0. Entries other than the
//! first in a directory do not normally have `dp->d_ino` set to 0.
//!
//! ## Deviations
//! - `doff_t` (a `#define` for `int32_t`) is the alias [`Doff`].
//! - A `struct direct` inside a directory block is not a Rust reference: the C casts a
//!   `char *` into the buffer, which need not stay aligned in a corrupted directory. The
//!   members are read and written at their C offsets in the block's bytes by [`d_ino`],
//!   [`set_d_ino`], [`d_reclen`], ... (`docs/C_TO_RUST.md`, wire headers). A reader past the
//!   end of the bytes gets 0, which the lookup treats as a mangled entry. A `struct direct`
//!   built in memory (`newdir`) is a [`Direct`] value, copied in with [`Direct::write_to`].
//! - The `DT_*` constants and `IFTODT`/`DTTOIF`, which `<sys/dirent.h>` also defines with the
//!   same values, are re-exported from `sys/dirent.rs`.
//! - `DIRSIZ(dp)` is [`dirsiz`] of the entry's `d_namlen`; `DIRECTSIZ(namlen)` is
//!   [`directsiz`].
//! - `struct dirtemplate` is [`Dirtemplate`], laid out as in C, written to and read from bytes
//!   with `to_bytes`/`from_bytes`.

pub use crate::sys::dirent::{
    DT_BLK, DT_CHR, DT_DIR, DT_FIFO, DT_LNK, DT_REG, DT_SOCK, DT_UNKNOWN, dttoif, iftodt,
};
use crate::sys::param::DEV_BSIZE;

/// `doff_t`: theoretically, directories can be more than 2Gb in length, however, in practice
/// this seems unlikely. So, we define the type `doff_t` as a 32-bit quantity to keep down the
/// cost of doing lookup on a 32-bit machine.
pub type Doff = i32;

/// `MAXDIRSIZE`.
pub const MAXDIRSIZE: Doff = 0x7fff_ffff;

/// `DIRBLKSIZ`.
pub const DIRBLKSIZ: usize = DEV_BSIZE;
/// `MAXNAMLEN`.
pub const MAXNAMLEN: usize = 255;

/// `struct direct`: a directory entry, as built in memory.
#[repr(C)]
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Direct {
    /// `d_ino`: inode number of entry.
    pub d_ino: u32,
    /// `d_reclen`: length of this record.
    pub d_reclen: u16,
    /// `d_type`: file type, see below.
    pub d_type: u8,
    /// `d_namlen`: length of string in `d_name`.
    pub d_namlen: u8,
    /// `d_name`: name with length <= `MAXNAMLEN`.
    pub d_name: [u8; MAXNAMLEN + 1],
}

impl Direct {
    /// `offsetof(struct direct, d_name)`.
    pub const NAME_OFFSET: usize = core::mem::offset_of!(Direct, d_name);

    /// An all-zero entry.
    pub const fn new() -> Self {
        Self {
            d_ino: 0,
            d_reclen: 0,
            d_type: 0,
            d_namlen: 0,
            d_name: [0; MAXNAMLEN + 1],
        }
    }

    /// `memcpy(dst, dirp, DIRSIZ(dirp))`: the entry's first `DIRSIZ` bytes at `off` in `dst`.
    pub fn write_to(&self, dst: &mut [u8], off: usize) {
        let size = dirsiz(self.d_namlen);
        let b = &mut dst[off..off + size];
        b[0..4].copy_from_slice(&self.d_ino.to_ne_bytes());
        b[4..6].copy_from_slice(&self.d_reclen.to_ne_bytes());
        b[6] = self.d_type;
        b[7] = self.d_namlen;
        b[Self::NAME_OFFSET..].copy_from_slice(&self.d_name[..size - Self::NAME_OFFSET]);
    }
}

impl Default for Direct {
    fn default() -> Self {
        Self::new()
    }
}

/// `DIR_ROUNDUP`: directory name roundup size.
pub const DIR_ROUNDUP: usize = 4;

/// `DIRECTSIZ(namlen)`: the minimum record length which will hold an entry whose name is
/// `namlen` bytes long: the fixed part plus the name and its NUL, rounded up to 4 bytes.
pub const fn directsiz(namlen: usize) -> usize {
    (Direct::NAME_OFFSET + (namlen + 1) + 3) & !3
}

/// `DIRSIZ(dp)`: the minimum record length of the entry `dp`, from its `d_namlen`.
pub const fn dirsiz(d_namlen: u8) -> usize {
    (size_of::<Direct>() - (MAXNAMLEN + 1)) + ((d_namlen as usize + 1 + 3) & !3)
}

/// `struct dirtemplate`: template for manipulating directories (the `.` and `..` entries of
/// a new directory). Should use `struct direct`s, but the name field is `MAXNAMLEN - 1`, and
/// this just won't do.
#[repr(C)]
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Dirtemplate {
    /// `dot_ino`.
    pub dot_ino: u32,
    /// `dot_reclen`.
    pub dot_reclen: i16,
    /// `dot_type`.
    pub dot_type: u8,
    /// `dot_namlen`.
    pub dot_namlen: u8,
    /// `dot_name`: must be multiple of 4.
    pub dot_name: [u8; 4],
    /// `dotdot_ino`.
    pub dotdot_ino: u32,
    /// `dotdot_reclen`.
    pub dotdot_reclen: i16,
    /// `dotdot_type`.
    pub dotdot_type: u8,
    /// `dotdot_namlen`.
    pub dotdot_namlen: u8,
    /// `dotdot_name`: ditto.
    pub dotdot_name: [u8; 4],
}

impl Dirtemplate {
    /// `sizeof(struct dirtemplate)`.
    pub const SIZE: usize = size_of::<Dirtemplate>();

    /// The structure's bytes, as the C's `memcpy` from it copies them.
    pub fn to_bytes(&self) -> [u8; Self::SIZE] {
        let mut b = [0u8; Self::SIZE];
        b[0..4].copy_from_slice(&self.dot_ino.to_ne_bytes());
        b[4..6].copy_from_slice(&self.dot_reclen.to_ne_bytes());
        b[6] = self.dot_type;
        b[7] = self.dot_namlen;
        b[8..12].copy_from_slice(&self.dot_name);
        b[12..16].copy_from_slice(&self.dotdot_ino.to_ne_bytes());
        b[16..18].copy_from_slice(&self.dotdot_reclen.to_ne_bytes());
        b[18] = self.dotdot_type;
        b[19] = self.dotdot_namlen;
        b[20..24].copy_from_slice(&self.dotdot_name);
        b
    }

    /// The structure read from its bytes (`b` holds at least `SIZE` of them).
    pub fn from_bytes(b: &[u8]) -> Self {
        let u32_at = |o: usize| u32::from_ne_bytes([b[o], b[o + 1], b[o + 2], b[o + 3]]);
        let i16_at = |o: usize| i16::from_ne_bytes([b[o], b[o + 1]]);
        Self {
            dot_ino: u32_at(0),
            dot_reclen: i16_at(4),
            dot_type: b[6],
            dot_namlen: b[7],
            dot_name: [b[8], b[9], b[10], b[11]],
            dotdot_ino: u32_at(12),
            dotdot_reclen: i16_at(16),
            dotdot_type: b[18],
            dotdot_namlen: b[19],
            dotdot_name: [b[20], b[21], b[22], b[23]],
        }
    }
}

/// `ep->d_ino` of the entry at `off` in a directory block (0 past the end).
pub fn d_ino(b: &[u8], off: usize) -> u32 {
    match b.get(off..off + 4) {
        Some(s) => u32::from_ne_bytes([s[0], s[1], s[2], s[3]]),
        None => 0,
    }
}

/// `ep->d_ino = v`.
pub fn set_d_ino(b: &mut [u8], off: usize, v: u32) {
    b[off..off + 4].copy_from_slice(&v.to_ne_bytes());
}

/// `ep->d_reclen` (0 past the end).
pub fn d_reclen(b: &[u8], off: usize) -> u16 {
    match b.get(off + 4..off + 6) {
        Some(s) => u16::from_ne_bytes([s[0], s[1]]),
        None => 0,
    }
}

/// `ep->d_reclen = v`.
pub fn set_d_reclen(b: &mut [u8], off: usize, v: u16) {
    b[off + 4..off + 6].copy_from_slice(&v.to_ne_bytes());
}

/// `ep->d_type` (0 past the end).
pub fn d_type(b: &[u8], off: usize) -> u8 {
    b.get(off + 6).copied().unwrap_or(0)
}

/// `ep->d_type = v`.
pub fn set_d_type(b: &mut [u8], off: usize, v: u8) {
    b[off + 6] = v;
}

/// `ep->d_namlen` (0 past the end).
pub fn d_namlen(b: &[u8], off: usize) -> u8 {
    b.get(off + 7).copied().unwrap_or(0)
}

/// `ep->d_name`: the `len` bytes of the name (fewer if the block ends first).
pub fn d_name(b: &[u8], off: usize, len: usize) -> &[u8] {
    let start = (off + Direct::NAME_OFFSET).min(b.len());
    let end = (start + len).min(b.len());
    &b[start..end]
}

const _: () = {
    assert!(Direct::NAME_OFFSET == 8);
    assert!(size_of::<Direct>() == 264);
    assert!(Dirtemplate::SIZE == 24);
    assert!(directsiz(1) == 12);
    assert!(dirsiz(2) == 12);
    assert!(dirsiz(4) == 16);
};
/* </CODE> */

/* <TESTS> */
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    #[ignore = "needs OPENBSD_SRC (just test-ref)"]
    fn constants_match_the_c_header() {
        let defs = crate::reftest::defines("sys/ufs/ufs/dir.h");
        let param = crate::reftest::defines("sys/sys/param.h");
        assert_eq!(defs.get("DIRBLKSIZ").map(|s| s.as_str()), Some("DEV_BSIZE"));
        assert_eq!(
            crate::reftest::int(&param, "DEV_BSIZE"),
            Some(DIRBLKSIZ as i64)
        );
        assert_eq!(
            crate::reftest::int(&defs, "MAXNAMLEN"),
            Some(MAXNAMLEN as i64)
        );
        assert_eq!(
            crate::reftest::int(&defs, "DIR_ROUNDUP"),
            Some(DIR_ROUNDUP as i64)
        );
        assert_eq!(
            crate::reftest::int(&defs, "DT_LNK"),
            Some(i64::from(DT_LNK))
        );
        assert_eq!(
            crate::reftest::int(&defs, "MAXDIRSIZE"),
            Some(i64::from(MAXDIRSIZE))
        );
    }

    #[test]
    fn entries_round_trip_through_a_block() {
        let mut blk = [0u8; DIRBLKSIZ];
        let mut d = Direct::new();
        d.d_ino = 42;
        d.d_reclen = DIRBLKSIZ as u16;
        d.d_type = DT_REG;
        d.d_namlen = 3;
        d.d_name[..3].copy_from_slice(b"abc");
        d.write_to(&mut blk, 0);
        assert_eq!(d_ino(&blk, 0), 42);
        assert_eq!(usize::from(d_reclen(&blk, 0)), DIRBLKSIZ);
        assert_eq!((d_type(&blk, 0), d_namlen(&blk, 0)), (DT_REG, 3));
        assert_eq!(d_name(&blk, 0, 3), b"abc");
        assert_eq!(blk[11], 0, "the name is NUL padded");
        assert_eq!(d_reclen(&blk, DIRBLKSIZ - 2), 0, "past the end reads 0");
        let t = Dirtemplate {
            dot_ino: 2,
            dot_reclen: 12,
            dot_type: DT_DIR,
            dot_namlen: 1,
            dot_name: *b".\0\0\0",
            dotdot_ino: 2,
            dotdot_reclen: (DIRBLKSIZ - 12) as i16,
            dotdot_type: DT_DIR,
            dotdot_namlen: 2,
            dotdot_name: *b"..\0\0",
        };
        assert_eq!(Dirtemplate::from_bytes(&t.to_bytes()), t);
        assert_eq!(d_name(&t.to_bytes(), 12, 2), b"..");
    }
}
/* </TESTS> */
