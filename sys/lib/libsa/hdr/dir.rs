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
//! `<ufs/ufs/dir.h>` for libsa: the on-disk directory entry, `struct direct`.

/// `MAXNAMLEN`: the longest file name.
pub const MAXNAMLEN: usize = 255;

/// The fixed part of `struct direct` (`d_ino`, `d_reclen`, `d_type`, `d_namlen`), followed
/// on disk by `d_name`, NUL-terminated and padded to four bytes.
pub const DIRECT_HDRSIZE: usize = 8;

/// `struct direct`, decoded from the bytes of a directory block (the C casts the buffer;
/// entries are not aligned for Rust, so the fields are read out).
#[derive(Clone, Copy, Debug)]
pub struct Direct<'a> {
    /// `d_ino`: inode number of entry.
    pub d_ino: u32,
    /// `d_reclen`: length of this record.
    pub d_reclen: u16,
    /// `d_type`: file type.
    pub d_type: u8,
    /// `d_namlen`: length of the name.
    pub d_namlen: u8,
    /// `d_name`: the rest of the record, from the name on.
    pub d_name: &'a [u8],
}

impl<'a> Direct<'a> {
    /// The entry at the start of `buf`, or `None` if `buf` cannot hold its fixed part.
    pub fn parse(buf: &'a [u8]) -> Option<Self> {
        if buf.len() < DIRECT_HDRSIZE {
            return None;
        }
        Some(Self {
            d_ino: u32::from_ne_bytes([buf[0], buf[1], buf[2], buf[3]]),
            d_reclen: u16::from_ne_bytes([buf[4], buf[5]]),
            d_type: buf[6],
            d_namlen: buf[7],
            d_name: &buf[DIRECT_HDRSIZE..],
        })
    }
}
/* </CODE> */
