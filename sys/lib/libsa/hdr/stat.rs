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
//! `<sys/stat.h>` for libsa: the mode bits and `struct stat`, as far as the standalone file
//! systems fill it.

use super::types::{Gid, Mode, Off, Uid};

/// `S_ISUID`: set user id on execution.
pub const S_ISUID: Mode = 0o004000;
/// `S_ISGID`: set group id on execution.
pub const S_ISGID: Mode = 0o002000;
/// `S_ISTXT`: sticky bit.
pub const S_ISTXT: Mode = 0o001000;
/// `S_IRUSR`: read for owner.
pub const S_IRUSR: Mode = 0o000400;
/// `S_IWUSR`: write for owner.
pub const S_IWUSR: Mode = 0o000200;
/// `S_IXUSR`: execute for owner.
pub const S_IXUSR: Mode = 0o000100;
/// `S_IRGRP`: read for group.
pub const S_IRGRP: Mode = 0o000040;
/// `S_IWGRP`: write for group.
pub const S_IWGRP: Mode = 0o000020;
/// `S_IXGRP`: execute for group.
pub const S_IXGRP: Mode = 0o000010;
/// `S_IROTH`: read for other.
pub const S_IROTH: Mode = 0o000004;
/// `S_IWOTH`: write for other.
pub const S_IWOTH: Mode = 0o000002;
/// `S_IXOTH`: execute for other.
pub const S_IXOTH: Mode = 0o000001;
/// `S_IFMT`: type of file mask.
pub const S_IFMT: Mode = 0o170000;
/// `S_IFDIR`: directory.
pub const S_IFDIR: Mode = 0o040000;
/// `S_IFREG`: regular.
pub const S_IFREG: Mode = 0o100000;
/// `S_IFLNK`: symbolic link.
pub const S_IFLNK: Mode = 0o120000;

/// `S_ISDIR(m)`.
pub const fn s_isdir(m: Mode) -> bool {
    m & S_IFMT == S_IFDIR
}

/// `struct stat`, the members the standalone file systems set (`st_mode`, `st_nlink`,
/// `st_uid`, `st_gid`, `st_size`); the kernel's has the rest.
#[derive(Clone, Copy, Default, Debug)]
pub struct Stat {
    /// `st_mode`: inode protection mode.
    pub st_mode: Mode,
    /// `st_nlink`: number of hard links.
    pub st_nlink: u32,
    /// `st_uid`: user ID of the file's owner.
    pub st_uid: Uid,
    /// `st_gid`: group ID of the file's group.
    pub st_gid: Gid,
    /// `st_size`: file size, in bytes.
    pub st_size: Off,
}
/* </CODE> */
