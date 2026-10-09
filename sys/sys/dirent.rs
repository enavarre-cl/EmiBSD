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

/*-
 * Copyright (c) 1989, 1993
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
 *	@(#)dirent.h	8.3 (Berkeley) 8/10/94
 */
/* </LICENSES> */

/* <CODE> */
//! `<sys/dirent.h>`: `struct dirent`, the format of the directory entries `getdents(2)`
//! returns, the `DT_*` file types and the record-size macros.
//!
//! A directory entry has a `struct dirent` at the front of it, containing its inode number,
//! the length of the entry, and the length of the name contained in the entry. These are
//! followed by the name padded to an 8 byte boundary with null bytes. All names are
//! guaranteed null terminated. The maximum length of a name in a directory is `MAXNAMLEN`.
//!
//! Upstream: sys/sys/dirent.h @ 3ce1f3f79392
//!
//! ## Deviations
//! - `IFTODT`, `DTTOIF`, `DIRENT_RECSIZE` and `DIRENT_SIZE` are `const fn`s with lowercase
//!   names; `dirent_size` takes the structure.
//! - [`Dirent::from_bytes`] reads an entry out of a directory buffer (what the C does by
//!   casting a `char *` into it); the name stays in the buffer.

use core::mem::offset_of;

use crate::sys::types::{Ino, Off};

/// `MAXNAMLEN`: the longest name of a directory entry.
pub const MAXNAMLEN: usize = 255;

/// `struct dirent`.
#[repr(C)]
#[derive(Clone, Copy)]
pub struct Dirent {
    /// `d_fileno`: file number of entry.
    pub d_fileno: Ino,
    /// `d_off`: offset after this entry.
    pub d_off: Off,
    /// `d_reclen`: length of this record.
    pub d_reclen: u16,
    /// `d_type`: file type, see below.
    pub d_type: u8,
    /// `d_namlen`: length of string in `d_name`.
    pub d_namlen: u8,
    /// `__d_padding`: suppress padding after `d_name`.
    pub __d_padding: [u8; 4],
    /// `d_name`: name must be no longer than this.
    pub d_name: [u8; MAXNAMLEN + 1],
}

impl Dirent {
    /// `offsetof(struct dirent, d_name)`: the size of the fixed header.
    pub const NAME_OFFSET: usize = offset_of!(Dirent, d_name);

    /// The fixed header of the entry at the start of `buf` (`(struct dirent *)cpos`), with an
    /// empty name, or `None` when `buf` is shorter than the header.
    pub fn from_bytes(buf: &[u8]) -> Option<Dirent> {
        if buf.len() < Self::NAME_OFFSET {
            return None;
        }
        let u64_at = |off: usize| {
            let mut b = [0u8; 8];
            b.copy_from_slice(&buf[off..off + 8]);
            u64::from_ne_bytes(b)
        };
        Some(Dirent {
            d_fileno: u64_at(offset_of!(Dirent, d_fileno)),
            d_off: u64_at(offset_of!(Dirent, d_off)) as Off,
            d_reclen: u16::from_ne_bytes([
                buf[offset_of!(Dirent, d_reclen)],
                buf[offset_of!(Dirent, d_reclen) + 1],
            ]),
            d_type: buf[offset_of!(Dirent, d_type)],
            d_namlen: buf[offset_of!(Dirent, d_namlen)],
            __d_padding: [0; 4],
            d_name: [0; MAXNAMLEN + 1],
        })
    }
}

/// `DT_UNKNOWN`.
pub const DT_UNKNOWN: u8 = 0;
/// `DT_FIFO`.
pub const DT_FIFO: u8 = 1;
/// `DT_CHR`.
pub const DT_CHR: u8 = 2;
/// `DT_DIR`.
pub const DT_DIR: u8 = 4;
/// `DT_BLK`.
pub const DT_BLK: u8 = 6;
/// `DT_REG`.
pub const DT_REG: u8 = 8;
/// `DT_LNK`.
pub const DT_LNK: u8 = 10;
/// `DT_SOCK`.
pub const DT_SOCK: u8 = 12;

/// `IFTODT(mode)`: convert a stat structure type to a directory type.
pub const fn iftodt(mode: u32) -> u8 {
    ((mode & 0o170000) >> 12) as u8
}

/// `DTTOIF(dirtype)`: convert a directory type to a stat structure type.
pub const fn dttoif(dirtype: u8) -> u32 {
    (dirtype as u32) << 12
}

/// `DIRENT_RECSIZE(namelen)`: the minimum record length which will hold a directory entry
/// with a name of the given length, including the terminating nul byte, rounded up to proper
/// alignment.
pub const fn dirent_recsize(namelen: usize) -> usize {
    (Dirent::NAME_OFFSET + namelen + 1 + 7) & !7
}

/// `DIRENT_SIZE(dp)`: `DIRENT_RECSIZE` of the entry's own name length.
pub const fn dirent_size(dp: &Dirent) -> usize {
    dirent_recsize(dp.d_namlen as usize)
}

const _: () = {
    assert!(Dirent::NAME_OFFSET == 24);
    assert!(size_of::<Dirent>() == 280);
};
/* </CODE> */

/* <TESTS> */
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn recsize_rounds_to_eight_with_the_nul() {
        assert_eq!(dirent_recsize(1), 32);
        assert_eq!(dirent_recsize(7), 32);
        assert_eq!(dirent_recsize(8), 40);
        assert_eq!(iftodt(0o040755), DT_DIR);
        assert_eq!(dttoif(DT_REG), 0o100000);
    }

    #[test]
    #[ignore = "needs OPENBSD_SRC (just test-ref)"]
    fn values_match_the_c_header() {
        let defs = crate::reftest::defines("sys/sys/dirent.h");
        for (name, value) in [
            ("DT_UNKNOWN", DT_UNKNOWN),
            ("DT_FIFO", DT_FIFO),
            ("DT_CHR", DT_CHR),
            ("DT_DIR", DT_DIR),
            ("DT_BLK", DT_BLK),
            ("DT_REG", DT_REG),
            ("DT_LNK", DT_LNK),
            ("DT_SOCK", DT_SOCK),
        ] {
            assert_eq!(
                crate::reftest::int(&defs, name),
                Some(i64::from(value)),
                "{name}"
            );
        }
        assert_eq!(
            crate::reftest::int(&defs, "MAXNAMLEN"),
            Some(MAXNAMLEN as i64)
        );
    }
}
/* </TESTS> */
