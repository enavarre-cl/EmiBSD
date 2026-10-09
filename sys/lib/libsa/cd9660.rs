/* <LICENSES> */
/*
 * Copyright (C) 1996 Wolfgang Solfrank.
 * Copyright (C) 1996 TooLs GmbH.
 * All rights reserved.
 *
 * Redistribution and use in source and binary forms, with or without
 * modification, are permitted provided that the following conditions
 * are met:
 * 1. Redistributions of source code must retain the above copyright
 *    notice, this list of conditions and the following disclaimer.
 * 2. Redistributions in binary form must reproduce the above copyright
 *    notice, this list of conditions and the following disclaimer in the
 *    documentation and/or other materials provided with the distribution.
 * 3. All advertising materials mentioning features or use of this software
 *    must display the following acknowledgement:
 *	This product includes software developed by TooLs GmbH.
 * 4. The name of TooLs GmbH may not be used to endorse or promote products
 *    derived from this software without specific prior written permission.
 *
 * THIS SOFTWARE IS PROVIDED BY TOOLS GMBH ``AS IS'' AND ANY EXPRESS OR
 * IMPLIED WARRANTIES, INCLUDING, BUT NOT LIMITED TO, THE IMPLIED WARRANTIES
 * OF MERCHANTABILITY AND FITNESS FOR A PARTICULAR PURPOSE ARE DISCLAIMED.
 * IN NO EVENT SHALL TOOLS GMBH BE LIABLE FOR ANY DIRECT, INDIRECT, INCIDENTAL,
 * SPECIAL, EXEMPLARY, OR CONSEQUENTIAL DAMAGES (INCLUDING, BUT NOT LIMITED TO,
 * PROCUREMENT OF SUBSTITUTE GOODS OR SERVICES; LOSS OF USE, DATA, OR PROFITS;
 * OR BUSINESS INTERRUPTION) HOWEVER CAUSED AND ON ANY THEORY OF LIABILITY,
 * WHETHER IN CONTRACT, STRICT LIABILITY, OR TORT (INCLUDING NEGLIGENCE OR
 * OTHERWISE) ARISING IN ANY WAY OUT OF THE USE OF THIS SOFTWARE, EVEN IF
 * ADVISED OF THE POSSIBILITY OF SUCH DAMAGE.
 */

/*
 * Stand-alone ISO9660 file reading package.
 *
 * Note: This doesn't support Rock Ridge extensions, extended attributes,
 * blocksizes other than 2048 bytes, multi-extent files, etc.
 */

/*
 * Copyright (C) 1996 Wolfgang Solfrank.
 * Copyright (C) 1996 TooLs GmbH.
 * All rights reserved.
 *
 * Redistribution and use in source and binary forms, with or without
 * modification, are permitted provided that the following conditions
 * are met:
 * 1. Redistributions of source code must retain the above copyright
 *    notice, this list of conditions and the following disclaimer.
 * 2. Redistributions in binary form must reproduce the above copyright
 *    notice, this list of conditions and the following disclaimer in the
 *    documentation and/or other materials provided with the distribution.
 * 3. All advertising materials mentioning features or use of this software
 *    must display the following acknowledgement:
 *	This product includes software developed by TooLs GmbH.
 * 4. The name of TooLs GmbH may not be used to endorse or promote products
 *    derived from this software without specific prior written permission.
 *
 * THIS SOFTWARE IS PROVIDED BY TOOLS GMBH ``AS IS'' AND ANY EXPRESS OR
 * IMPLIED WARRANTIES, INCLUDING, BUT NOT LIMITED TO, THE IMPLIED WARRANTIES
 * OF MERCHANTABILITY AND FITNESS FOR A PARTICULAR PURPOSE ARE DISCLAIMED.
 * IN NO EVENT SHALL TOOLS GMBH BE LIABLE FOR ANY DIRECT, INDIRECT, INCIDENTAL,
 * SPECIAL, EXEMPLARY, OR CONSEQUENTIAL DAMAGES (INCLUDING, BUT NOT LIMITED TO,
 * PROCUREMENT OF SUBSTITUTE GOODS OR SERVICES; LOSS OF USE, DATA, OR PROFITS;
 * OR BUSINESS INTERRUPTION) HOWEVER CAUSED AND ON ANY THEORY OF LIABILITY,
 * WHETHER IN CONTRACT, STRICT LIABILITY, OR TORT (INCLUDING NEGLIGENCE OR
 * OTHERWISE) ARISING IN ANY WAY OUT OF THE USE OF THIS SOFTWARE, EVEN IF
 * ADVISED OF THE POSSIBILITY OF SUCH DAMAGE.
 */
/* </LICENSES> */

/* <CODE> */
//! Stand-alone ISO 9660 file reading package: find the directory through the path table,
//! the file in the directory, and read its extent.
//!
//! Upstream: sys/lib/libsa/cd9660.c @ 3ce1f3f79392, sys/lib/libsa/cd9660.h @ 3ce1f3f79392
//!
//! ## Deviations
//! - `struct file` is [`Cd9660File`], owned by `f_fsdata`; the buffers are `Vec`s.
//! - The path table entries (`struct ptable_ent`) and directory records are read as byte
//!   ranges of the blocks (`hdr/iso.rs`); a record or entry that would run past its block is
//!   treated as the end of the table (the C reads past the buffer).
//! - `cd9660_read` always reads through its own block buffer (the C reads whole aligned
//!   blocks straight into the caller's buffer).

use alloc::boxed::Box;
use alloc::vec;

use crate::hdr::iso::{
    ISO_DEFAULT_BLOCK_SHIFT, ISO_DEFAULT_BLOCK_SIZE, ISO_STANDARD_ID, ISO_VD_END, ISO_VD_PRIMARY,
    iso_directory_record as dr, iso_primary_descriptor as pd, isonum_711, isonum_722, isonum_723,
    isonum_732, isonum_733,
};
use crate::hdr::param::{DEV_BSIZE, roundup};
use crate::hdr::stat::{S_IFREG, S_IRGRP, S_IROTH, S_IRUSR, Stat};
use crate::hdr::types::{Daddr, Daddr32, Off};
use crate::saerrno::Errno;
use crate::stand::{F_READ, OpenFile, SEEK_CUR, SEEK_END, SEEK_SET, toupper};

/// `PTFIXSZ`: the fixed part of a path table entry.
const PTFIXSZ: usize = 8;

/// `struct file`.
pub struct Cd9660File {
    /// `off`: current offset within file.
    off: Off,
    /// `bno`: starting block number.
    bno: Daddr32,
    /// `size`: size of file.
    size: Off,
}

/// `cdb2devb(bno)`: an ISO block to `DEV_BSIZE` blocks.
const fn cdb2devb(bno: Daddr32) -> Daddr {
    bno as Daddr * ISO_DEFAULT_BLOCK_SIZE as Daddr / DEV_BSIZE as Daddr
}

/// `PTSIZE(pp)`: the size of the path table entry at `pp`.
fn ptsize(pp: &[u8]) -> usize {
    roundup((PTFIXSZ + isonum_711(&pp[0..1]) as usize) as u64, 2) as usize
}

/// `pnmatch(path, pp)`: does the path table entry `pp` name the first component of `path`?
fn pnmatch(path: &[u8], pp: &[u8]) -> bool {
    let namlen = isonum_711(&pp[0..1]) as usize;
    let at = |i: usize| path.get(i).copied().unwrap_or(0);
    for i in 0..namlen {
        if pp.get(PTFIXSZ + i).copied() != Some(toupper(at(i))) {
            return false;
        }
    }
    at(namlen) == b'/'
}

/// `dirmatch(path, dp)`: does the directory record `dp` name the regular file `path`?
fn dirmatch(path: &[u8], dp: &[u8]) -> bool {
    // This needs to be a regular file
    if dp[dr::FLAGS.start] & 6 != 0 {
        return false;
    }

    let name = &dp[dr::NAME..];
    let at = |i: usize| path.get(i).copied().unwrap_or(0);
    let mut i = isonum_711(&dp[dr::NAME_LEN]) as isize;
    let mut p = 0;
    let mut cp = 0;
    loop {
        i -= 1;
        if i < 0 {
            break;
        }
        if at(p) == 0 {
            break;
        }
        if toupper(at(p)) == name.get(cp).copied().unwrap_or(0) {
            p += 1;
            cp += 1;
            continue;
        }
        return false;
    }
    if at(p) != 0 {
        return false;
    }
    // Allow stripping of trailing dots and the version number. Note that this will find the
    // first instead of the last version of a file.
    let c = name.get(cp).copied().unwrap_or(0);
    if i >= 0 && (c == b';' || c == b'.') {
        // This is to prevent matching of numeric extensions
        if c == b'.' && name.get(cp + 1).copied() != Some(b';') {
            return false;
        }
        loop {
            i -= 1;
            if i < 0 {
                break;
            }
            cp += 1;
            let c = name.get(cp).copied().unwrap_or(0);
            if c != b';' && !c.is_ascii_digit() {
                return false;
            }
        }
    }
    true
}

/// `cd9660_open(path, f)`.
pub fn cd9660_open(path: &[u8], f: &mut OpenFile) -> Result<(), Errno> {
    let io = f.io();
    let path = &path[..path.iter().position(|&c| c == 0).unwrap_or(path.len())];

    // First find the volume descriptor
    let mut buf = vec![0u8; ISO_DEFAULT_BLOCK_SIZE];
    let mut nread = 0;
    let mut bno: Daddr32 = 16;
    loop {
        io.strategy(F_READ, cdb2devb(bno), &mut buf, Some(&mut nread))?;
        if nread != ISO_DEFAULT_BLOCK_SIZE {
            return Err(Errno::EIO);
        }
        if buf[pd::ID] != ISO_STANDARD_ID[..] {
            return Err(Errno::EINVAL);
        }
        if isonum_711(&buf[pd::TYPE]) == u32::from(ISO_VD_END) {
            return Err(Errno::EINVAL);
        }
        if isonum_711(&buf[pd::TYPE]) == u32::from(ISO_VD_PRIMARY) {
            break;
        }
        bno += 1;
    }
    if isonum_723(&buf[pd::LOGICAL_BLOCK_SIZE]) as usize != ISO_DEFAULT_BLOCK_SIZE {
        return Err(Errno::EINVAL);
    }

    // Now get the path table and lookup the directory of the file
    bno = isonum_732(&buf[pd::TYPE_M_PATH_TABLE]) as Daddr32;
    let psize = isonum_733(&buf[pd::PATH_TABLE_SIZE]) as usize;

    if psize > ISO_DEFAULT_BLOCK_SIZE {
        buf = vec![0u8; roundup(psize as u64, ISO_DEFAULT_BLOCK_SIZE as u64) as usize];
    }

    io.strategy(F_READ, cdb2devb(bno), &mut buf, Some(&mut nread))?;
    if nread != buf.len() {
        return Err(Errno::EIO);
    }

    let mut parent = 1;
    let mut pp = 0usize;
    let mut ent = 1;
    bno = (isonum_732(&buf[2..6]) + isonum_711(&buf[1..2])) as Daddr32;

    // Remove extra separators
    let mut path = path;
    while path.first() == Some(&b'/') {
        path = &path[1..];
    }

    let entry = |pp: usize| buf.get(pp..).filter(|e| e.len() >= PTFIXSZ);
    while !path.is_empty() {
        let Some(e) = entry(pp).filter(|_| pp < psize) else {
            break;
        };
        if isonum_722(&e[6..8]) != parent {
            break;
        }
        if !pnmatch(path, e) {
            pp += ptsize(e);
            ent += 1;
            continue;
        }
        path = &path[(isonum_711(&e[0..1]) as usize + 1).min(path.len())..];
        parent = ent;
        bno = (isonum_732(&e[2..6]) + isonum_711(&e[1..2])) as Daddr32;
        while pp < psize {
            let Some(e) = entry(pp) else {
                break;
            };
            if isonum_722(&e[6..8]) == parent {
                break;
            }
            pp += ptsize(e);
            ent += 1;
        }
    }

    // Now bno has the start of the directory that supposedly contains the file
    let mut block = vec![0u8; ISO_DEFAULT_BLOCK_SIZE];
    bno -= 1;
    let mut dsize = 1usize; // Something stupid, but > 0	XXX
    let mut psize = 0usize;
    let mut dp = 0usize;
    let mut found = None;
    while psize < dsize {
        if psize.is_multiple_of(ISO_DEFAULT_BLOCK_SIZE) {
            bno += 1;
            io.strategy(F_READ, cdb2devb(bno), &mut block, Some(&mut nread))?;
            if nread != ISO_DEFAULT_BLOCK_SIZE {
                return Err(Errno::EIO);
            }
            dp = 0;
        }
        let len = isonum_711(&block[dp..dp + 1]) as usize;
        if len == 0 || dp + len > block.len() || len <= dr::NAME {
            if dp == 0 {
                psize += ISO_DEFAULT_BLOCK_SIZE;
            } else {
                psize = roundup(psize as u64, ISO_DEFAULT_BLOCK_SIZE as u64) as usize;
            }
            continue;
        }
        let rec = &block[dp..dp + len];
        if dsize == 1 {
            dsize = isonum_733(&rec[dr::SIZE]) as usize;
        }
        if dirmatch(path, rec) {
            found = Some((isonum_733(&rec[dr::EXTENT]), isonum_733(&rec[dr::SIZE])));
            break;
        }
        psize += len;
        dp += len;
    }

    let Some((extent, size)) = found.filter(|_| psize < dsize) else {
        return Err(Errno::ENOENT);
    };

    f.f_fsdata = Some(Box::new(Cd9660File {
        off: 0,
        bno: extent as Daddr32,
        size: Off::from(size),
    }));
    Ok(())
}

/// `cd9660_close(f)`.
pub fn cd9660_close(f: &mut OpenFile) -> Result<(), Errno> {
    f.f_fsdata = None;
    Ok(())
}

/// `cd9660_read(f, start, size, &resid)`.
pub fn cd9660_read(f: &mut OpenFile, start: &mut [u8], resid: &mut usize) -> Result<(), Errno> {
    let io = f.io();
    let fp = f.fsdata::<Cd9660File>().ok_or(Errno::EBADF)?;
    let mut buf = vec![0u8; ISO_DEFAULT_BLOCK_SIZE];
    let mut st = 0;
    while st < start.len() {
        if fp.off < 0 || fp.off >= fp.size {
            break;
        }
        let bno = (fp.off >> ISO_DEFAULT_BLOCK_SHIFT) as Daddr32 + fp.bno;
        let mut nread = 0;
        io.strategy(F_READ, cdb2devb(bno), &mut buf, Some(&mut nread))?;
        if nread != ISO_DEFAULT_BLOCK_SIZE {
            return Err(Errno::EIO);
        }

        // the offset to the interesting data in the block
        let off = (fp.off & (ISO_DEFAULT_BLOCK_SIZE as Off - 1)) as usize;
        let n = (nread - off)
            .min(start.len() - st)
            .min((fp.size - fp.off) as usize);
        start[st..st + n].copy_from_slice(&buf[off..off + n]);
        st += n;
        fp.off += n as Off;
    }
    *resid = start.len() - st;
    Ok(())
}

/// `cd9660_write`.
pub fn cd9660_write(_f: &mut OpenFile, _start: &[u8], _resid: &mut usize) -> Result<(), Errno> {
    Err(Errno::EROFS)
}

/// `cd9660_seek(f, offset, where)`.
pub fn cd9660_seek(f: &mut OpenFile, offset: Off, whence: i32) -> Result<Off, Errno> {
    let fp = f.fsdata::<Cd9660File>().ok_or(Errno::EBADF)?;
    match whence {
        SEEK_SET => fp.off = offset,
        SEEK_CUR => fp.off += offset,
        SEEK_END => fp.off = fp.size - offset,
        _ => return Err(Errno::EINVAL),
    }
    Ok(fp.off)
}

/// `cd9660_stat(f, sb)`: only important stuff.
pub fn cd9660_stat(f: &mut OpenFile, sb: &mut Stat) -> Result<(), Errno> {
    let fp = f.fsdata::<Cd9660File>().ok_or(Errno::EBADF)?;
    sb.st_mode = S_IFREG | S_IRUSR | S_IRGRP | S_IROTH;
    sb.st_uid = 0;
    sb.st_gid = 0;
    sb.st_size = fp.size;
    Ok(())
}

/// `cd9660_readdir`: not implemented.
pub fn cd9660_readdir(_f: &mut OpenFile, _name: Option<&mut [u8]>) -> Result<(), Errno> {
    Err(Errno::EROFS)
}
/* </CODE> */
