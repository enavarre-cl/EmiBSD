/* <LICENSES> */
/*-
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
 *	@(#)stat.h	8.9 (Berkeley) 8/17/94
 */
/* </LICENSES> */

/* <CODE> */
//! `<sys/stat.h>`: `struct stat` (what `fstat(2)` returns), the `S_I*` mode bits and file
//! types, and the file flags (`UF_*`, `SF_*`).
//!
//! Upstream: sys/sys/stat.h @ 3ce1f3f79392
//!
//! ## Deviations
//! - The `S_IS*(m)` macros are `const fn`s with lowercase names (`s_isdir`, ...); the
//!   `st_atime`-style field aliases are the `Timespec` members (`st_atim.tv_sec`).
//! - [`Stat::to_bytes`] is the `copyout` of a `struct stat`: it writes each field at its C
//!   offset and zeroes the padding before `__st_birthtim`, so no uninitialised byte reaches
//!   user space.
//! - The userland prototypes (`stat`, `fstat`, `chmod`, ...) are not kernel material.

use core::mem::offset_of;

use crate::sys::time::Timespec;
use crate::sys::types::{Blkcnt, Blksize, Dev, Gid, Ino, Mode, Nlink, Off, Uid};

/// `S_ISUID`: set user id on execution.
pub const S_ISUID: Mode = 0o004000;
/// `S_ISGID`: set group id on execution.
pub const S_ISGID: Mode = 0o002000;
/// `S_ISTXT`: sticky bit.
pub const S_ISTXT: Mode = 0o001000;

/// `S_IRWXU`: RWX mask for owner.
pub const S_IRWXU: Mode = 0o000700;
/// `S_IRUSR`: R for owner.
pub const S_IRUSR: Mode = 0o000400;
/// `S_IWUSR`: W for owner.
pub const S_IWUSR: Mode = 0o000200;
/// `S_IXUSR`: X for owner.
pub const S_IXUSR: Mode = 0o000100;

/// `S_IREAD`.
pub const S_IREAD: Mode = S_IRUSR;
/// `S_IWRITE`.
pub const S_IWRITE: Mode = S_IWUSR;
/// `S_IEXEC`.
pub const S_IEXEC: Mode = S_IXUSR;

/// `S_IRWXG`: RWX mask for group.
pub const S_IRWXG: Mode = 0o000070;
/// `S_IRGRP`: R for group.
pub const S_IRGRP: Mode = 0o000040;
/// `S_IWGRP`: W for group.
pub const S_IWGRP: Mode = 0o000020;
/// `S_IXGRP`: X for group.
pub const S_IXGRP: Mode = 0o000010;

/// `S_IRWXO`: RWX mask for other.
pub const S_IRWXO: Mode = 0o000007;
/// `S_IROTH`: R for other.
pub const S_IROTH: Mode = 0o000004;
/// `S_IWOTH`: W for other.
pub const S_IWOTH: Mode = 0o000002;
/// `S_IXOTH`: X for other.
pub const S_IXOTH: Mode = 0o000001;

/// `S_IFMT`: type of file mask.
pub const S_IFMT: Mode = 0o170000;
/// `S_IFIFO`: named pipe (fifo).
pub const S_IFIFO: Mode = 0o010000;
/// `S_IFCHR`: character special.
pub const S_IFCHR: Mode = 0o020000;
/// `S_IFDIR`: directory.
pub const S_IFDIR: Mode = 0o040000;
/// `S_IFBLK`: block special.
pub const S_IFBLK: Mode = 0o060000;
/// `S_IFREG`: regular.
pub const S_IFREG: Mode = 0o100000;
/// `S_IFLNK`: symbolic link.
pub const S_IFLNK: Mode = 0o120000;
/// `S_IFSOCK`: socket.
pub const S_IFSOCK: Mode = 0o140000;
/// `S_ISVTX`: save swapped text even after use.
pub const S_ISVTX: Mode = 0o001000;

/// `ACCESSPERMS`: 00777.
pub const ACCESSPERMS: Mode = S_IRWXU | S_IRWXG | S_IRWXO;
/// `ALLPERMS`: 07777.
pub const ALLPERMS: Mode = S_ISUID | S_ISGID | S_ISTXT | S_IRWXU | S_IRWXG | S_IRWXO;
/// `DEFFILEMODE`: 00666.
pub const DEFFILEMODE: Mode = S_IRUSR | S_IWUSR | S_IRGRP | S_IWGRP | S_IROTH | S_IWOTH;

/// `S_BLKSIZE`: block size used in the stat struct.
pub const S_BLKSIZE: usize = 512;

/// `UF_SETTABLE`: mask of owner changeable flags.
pub const UF_SETTABLE: u32 = 0x0000_ffff;
/// `UF_NODUMP`: do not dump file.
pub const UF_NODUMP: u32 = 0x0000_0001;
/// `UF_IMMUTABLE`: file may not be changed.
pub const UF_IMMUTABLE: u32 = 0x0000_0002;
/// `UF_APPEND`: writes to file may only append.
pub const UF_APPEND: u32 = 0x0000_0004;
/// `UF_OPAQUE`: directory is opaque wrt. union.
pub const UF_OPAQUE: u32 = 0x0000_0008;
/// `SF_SETTABLE`: mask of superuser changeable flags.
pub const SF_SETTABLE: u32 = 0xffff_0000;
/// `SF_ARCHIVED`: file is archived.
pub const SF_ARCHIVED: u32 = 0x0001_0000;
/// `SF_IMMUTABLE`: file may not be changed.
pub const SF_IMMUTABLE: u32 = 0x0002_0000;
/// `SF_APPEND`: writes to file may only append.
pub const SF_APPEND: u32 = 0x0004_0000;

/// `OPAQUE`: shorthand abbreviation.
pub const OPAQUE: u32 = UF_OPAQUE;
/// `APPEND`: shorthand abbreviation.
pub const APPEND: u32 = UF_APPEND | SF_APPEND;
/// `IMMUTABLE`: shorthand abbreviation.
pub const IMMUTABLE: u32 = UF_IMMUTABLE | SF_IMMUTABLE;

/// `UTIME_NOW`.
pub const UTIME_NOW: i64 = -2;
/// `UTIME_OMIT`.
pub const UTIME_OMIT: i64 = -1;

/// `struct stat`.
#[repr(C)]
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Stat {
    /// `st_mode`: inode protection mode.
    pub st_mode: Mode,
    /// `st_dev`: inode's device.
    pub st_dev: Dev,
    /// `st_ino`: inode's number.
    pub st_ino: Ino,
    /// `st_nlink`: number of hard links.
    pub st_nlink: Nlink,
    /// `st_uid`: user ID of the file's owner.
    pub st_uid: Uid,
    /// `st_gid`: group ID of the file's group.
    pub st_gid: Gid,
    /// `st_rdev`: device type.
    pub st_rdev: Dev,
    /// `st_atim`: time of last access.
    pub st_atim: Timespec,
    /// `st_mtim`: time of last data modification.
    pub st_mtim: Timespec,
    /// `st_ctim`: time of last file status change.
    pub st_ctim: Timespec,
    /// `st_size`: file size, in bytes.
    pub st_size: Off,
    /// `st_blocks`: blocks allocated for file.
    pub st_blocks: Blkcnt,
    /// `st_blksize`: optimal blocksize for I/O.
    pub st_blksize: Blksize,
    /// `st_flags`: user defined flags for file.
    pub st_flags: u32,
    /// `st_gen`: file generation number.
    pub st_gen: u32,
    /// `__st_birthtim`: time of file creation.
    pub __st_birthtim: Timespec,
}

impl Stat {
    /// The structure's size in user space.
    pub const SIZE: usize = size_of::<Stat>();

    /// The structure's user-space bytes (`copyout`), padding zeroed.
    pub fn to_bytes(&self) -> [u8; Self::SIZE] {
        let mut b = [0u8; Self::SIZE];
        let mut put = |off: usize, bytes: &[u8]| b[off..off + bytes.len()].copy_from_slice(bytes);
        let mut put_ts = |off: usize, ts: &Timespec| {
            put(off, &ts.tv_sec.to_ne_bytes());
            put(off + 8, &ts.tv_nsec.to_ne_bytes());
        };
        put_ts(offset_of!(Stat, st_atim), &self.st_atim);
        put_ts(offset_of!(Stat, st_mtim), &self.st_mtim);
        put_ts(offset_of!(Stat, st_ctim), &self.st_ctim);
        put_ts(offset_of!(Stat, __st_birthtim), &self.__st_birthtim);
        put(offset_of!(Stat, st_mode), &self.st_mode.to_ne_bytes());
        put(offset_of!(Stat, st_dev), &self.st_dev.to_ne_bytes());
        put(offset_of!(Stat, st_ino), &self.st_ino.to_ne_bytes());
        put(offset_of!(Stat, st_nlink), &self.st_nlink.to_ne_bytes());
        put(offset_of!(Stat, st_uid), &self.st_uid.to_ne_bytes());
        put(offset_of!(Stat, st_gid), &self.st_gid.to_ne_bytes());
        put(offset_of!(Stat, st_rdev), &self.st_rdev.to_ne_bytes());
        put(offset_of!(Stat, st_size), &self.st_size.to_ne_bytes());
        put(offset_of!(Stat, st_blocks), &self.st_blocks.to_ne_bytes());
        put(offset_of!(Stat, st_blksize), &self.st_blksize.to_ne_bytes());
        put(offset_of!(Stat, st_flags), &self.st_flags.to_ne_bytes());
        put(offset_of!(Stat, st_gen), &self.st_gen.to_ne_bytes());
        b
    }
}

/// `S_ISDIR(m)`: directory.
pub const fn s_isdir(m: Mode) -> bool {
    m & 0o170000 == 0o040000
}

/// `S_ISCHR(m)`: char special.
pub const fn s_ischr(m: Mode) -> bool {
    m & 0o170000 == 0o020000
}

/// `S_ISBLK(m)`: block special.
pub const fn s_isblk(m: Mode) -> bool {
    m & 0o170000 == 0o060000
}

/// `S_ISREG(m)`: regular file.
pub const fn s_isreg(m: Mode) -> bool {
    m & 0o170000 == 0o100000
}

/// `S_ISFIFO(m)`: fifo.
pub const fn s_isfifo(m: Mode) -> bool {
    m & 0o170000 == 0o010000
}

/// `S_ISLNK(m)`: symbolic link.
pub const fn s_islnk(m: Mode) -> bool {
    m & 0o170000 == 0o120000
}

/// `S_ISSOCK(m)`: socket.
pub const fn s_issock(m: Mode) -> bool {
    m & 0o170000 == 0o140000
}

// The amd64/arm64 layout of `struct stat` (both LP64): 128 bytes, the birth time after four
// bytes of padding.
const _: () = {
    assert!(size_of::<Stat>() == 128);
    assert!(offset_of!(Stat, st_atim) == 32);
    assert!(offset_of!(Stat, st_size) == 80);
    assert!(offset_of!(Stat, st_gen) == 104);
    assert!(offset_of!(Stat, __st_birthtim) == 112);
};
/* </CODE> */

/* <TESTS> */
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn to_bytes_places_fields_at_their_offsets() {
        let st = Stat {
            st_mode: S_IFCHR | 0o620,
            st_gen: 7,
            st_size: -1,
            ..Stat::default()
        };
        let b = st.to_bytes();
        assert_eq!(&b[0..4], &(S_IFCHR | 0o620).to_ne_bytes());
        assert_eq!(&b[80..88], &(-1i64).to_ne_bytes());
        assert_eq!(&b[104..108], &7u32.to_ne_bytes());
        assert_eq!(&b[108..112], &[0; 4]);
        assert!(s_ischr(st.st_mode) && !s_isdir(st.st_mode));
    }

    #[test]
    #[ignore = "needs OPENBSD_SRC (just test-ref)"]
    fn values_match_the_c_header() {
        let defs = crate::reftest::defines("sys/sys/stat.h");
        for (name, value) in [
            ("S_IFMT", S_IFMT),
            ("S_IFCHR", S_IFCHR),
            ("S_IFSOCK", S_IFSOCK),
            ("S_IWGRP", S_IWGRP),
            ("S_ISUID", S_ISUID),
        ] {
            assert_eq!(
                crate::reftest::int(&defs, name),
                Some(i64::from(value)),
                "{name}"
            );
        }
    }
}
/* </TESTS> */
