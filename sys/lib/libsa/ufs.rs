/*	$OpenBSD: ufs.c,v 1.27 2019/08/03 15:22:17 deraadt Exp $	*/
/*	$NetBSD: ufs.c,v 1.16 1996/09/30 16:01:22 ws Exp $	*/
/*	$OpenBSD: ufs.h,v 1.7 2019/08/03 15:22:17 deraadt Exp $	*/
/*	$NetBSD: ufs.h,v 1.5 1995/10/20 01:35:25 cgd Exp $	*/
/* <LICENSES> */
/*-
 * Copyright (c) 1993
 *	The Regents of the University of California.  All rights reserved.
 *
 * This code is derived from software contributed to Berkeley by
 * The Mach Operating System project at Carnegie-Mellon University.
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
 *
 * Copyright (c) 1990, 1991 Carnegie Mellon University
 * All Rights Reserved.
 *
 * Author: David Golub
 *
 * Permission to use, copy, modify and distribute this software and its
 * documentation is hereby granted, provided that both the copyright
 * notice and this permission notice appear in all copies of the
 * software, derivative works or modified versions, and any portions
 * thereof, and that both notices appear in supporting documentation.
 *
 * CARNEGIE MELLON ALLOWS FREE USE OF THIS SOFTWARE IN ITS "AS IS"
 * CONDITION.  CARNEGIE MELLON DISCLAIMS ANY LIABILITY OF ANY KIND FOR
 * ANY DAMAGES WHATSOEVER RESULTING FROM THE USE OF THIS SOFTWARE.
 *
 * Carnegie Mellon requests users of this software to return to
 *
 *  Software Distribution Coordinator  or  Software.Distribution@CS.CMU.EDU
 *  School of Computer Science
 *  Carnegie Mellon University
 *  Pittsburgh PA 15213-3890
 *
 * any improvements or extensions that they make and grant Carnegie the
 * rights to redistribute these changes.
 */

/*
 *	Stand-alone file reading package.
 */

/*-
 * Copyright (c) 1993
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
 *	@(#)ufs.h	8.1 (Berkeley) 6/11/93
 */
/* </LICENSES> */

/* <CODE> */
//! Stand-alone file reading package for the fast file system (FFS1, `struct ufs1_dinode`):
//! look a path up from the root inode, following symbolic links, and read files and
//! directories block by block through the device's strategy routine.
//!
//! Upstream: sys/lib/libsa/ufs.c @ 3ce1f3f79392, sys/lib/libsa/ufs.h @ 3ce1f3f79392
//!
//! `ufs2.c` is this file with `struct ufs2_dinode`, 64-bit block addresses and the FFS2
//! super-block location and magic. Both are one generic reader here, [`Ufs<D>`], over the
//! [`Dinode`] trait; `ufs2.rs` instantiates it for FFS2 and keeps its own entry points.
//!
//! ## Deviations
//! - `struct file` is [`UfsFile`], owned by `f_fsdata`; its buffers are `Vec`s (the C
//!   `alloc()`s them; libsa's allocator is the global one). `ufs_close_internal` is the drop.
//! - The path is copied into one `MAXPATHLEN + 1` buffer that the walk and the symbolic-link
//!   splice work in (the C walks the caller's string, writing NULs it then restores, and
//!   splices links into `namebuf`). A path longer than that is `ENOENT`.
//! - Directory entries are decoded from the block bytes ([`Direct`]); an entry whose
//!   `d_reclen` is 0 ends the block's scan (the C would loop on it forever).
//! - `COMPAT_UFS` (`ffs_oldfscompat`) is not defined for efiboot and not ported.
//! - FFS1 arithmetic keeps the C's 32-bit `daddr32_t` casts (`Dinode::fsbtodb`, the
//!   indirect block addresses), and the indirect block counts its `int` (`f_nindir`).

use alloc::boxed::Box;
use alloc::vec;
use alloc::vec::Vec;
use core::marker::PhantomData;

use crate::hdr::dinode::{IFDIR, IFLNK, IFMT, NDADDR, NIADDR, ROOTINO, Ufs1Dinode};
use crate::hdr::dir::{Direct, MAXNAMLEN};
use crate::hdr::fs::{
    FS_MAGIC, Fs, SBLOCK, SBSIZE, blkoff, dblksize, ino_to_fsba, ino_to_fsbo, lblkno, nindir,
};
use crate::hdr::param::{MAXBSIZE, MAXPATHLEN, MAXSYMLINKS};
use crate::hdr::stat::Stat;
use crate::hdr::types::{Daddr, Mode, Off, Ufsino};
use crate::saerrno::Errno;
use crate::stand::{DevIo, F_READ, F_WRITE, OpenFile, SEEK_CUR, SEEK_END, SEEK_SET};

/// What `ufs.c` and `ufs2.c` differ in: the inode, the block address width, and where the
/// super-block is and what its magic is.
pub trait Dinode: Copy + Default + Send + 'static {
    /// The super-block's `DEV_BSIZE` block.
    const SBLOCK: Daddr;
    /// `fs_magic`.
    const MAGIC: i32;
    /// The size of an indirect block entry (`daddr32_t` or `daddr_t`).
    const DADDR_SIZE: usize;
    /// `di_mode`.
    fn mode(&self) -> u16;
    /// Sets `di_mode`.
    fn set_mode(&mut self, mode: u16);
    /// `di_size`.
    fn size(&self) -> u64;
    /// `di_uid`.
    fn uid(&self) -> u32;
    /// `di_gid`.
    fn gid(&self) -> u32;
    /// `di_db[i]`.
    fn db(&self, i: usize) -> Daddr;
    /// `di_ib[i]`.
    fn ib(&self, i: usize) -> Daddr;
    /// `di_shortlink`: the bytes of `di_db` and `di_ib`, which hold a short symbolic link.
    fn shortlink(&self) -> &[u8];
    /// `fsbtodb(fs, b)` with the C's types for this flavour.
    fn fsbtodb(fs: &Fs, b: Daddr) -> Daddr;
    /// Entry `i` of an indirect block.
    fn indirect(buf: &[u8], i: usize) -> Daddr;
}

impl Dinode for Ufs1Dinode {
    const SBLOCK: Daddr = SBLOCK;
    const MAGIC: i32 = FS_MAGIC;
    const DADDR_SIZE: usize = 4;

    fn mode(&self) -> u16 {
        self.di_mode
    }

    fn set_mode(&mut self, mode: u16) {
        self.di_mode = mode;
    }

    fn size(&self) -> u64 {
        self.di_size
    }

    fn uid(&self) -> u32 {
        self.di_uid
    }

    fn gid(&self) -> u32 {
        self.di_gid
    }

    fn db(&self, i: usize) -> Daddr {
        Daddr::from(self.di_db[i])
    }

    fn ib(&self, i: usize) -> Daddr {
        Daddr::from(self.di_ib[i])
    }

    fn shortlink(&self) -> &[u8] {
        // SAFETY: `di_db` and `di_ib` are adjacent `i32` arrays in a `#[repr(C)]` struct
        // (offsets 40 and 88), so their 60 bytes are initialised and contiguous within `self`.
        unsafe {
            core::slice::from_raw_parts(self.di_db.as_ptr().cast::<u8>(), 4 * (NDADDR + NIADDR))
        }
    }

    fn fsbtodb(fs: &Fs, b: Daddr) -> Daddr {
        // daddr32_t arithmetic, as in the C
        Daddr::from((b as i32).wrapping_shl(fs.fs_fsbtodb as u32))
    }

    fn indirect(buf: &[u8], i: usize) -> Daddr {
        let b = &buf[4 * i..4 * i + 4];
        Daddr::from(i32::from_ne_bytes([b[0], b[1], b[2], b[3]]))
    }
}

/// `struct file`: the in-core open file.
pub struct UfsFile<D: Dinode> {
    /// `f_seekp`: seek pointer.
    f_seekp: Off,
    /// `f_fs`: the super-block.
    f_fs: Box<Fs>,
    /// `f_di`: copy of the on-disk inode.
    f_di: D,
    /// `f_ino`: our inode number.
    f_ino: Ufsino,
    /// `f_nindir`: number of blocks mapped by an indirect block at level i.
    f_nindir: [i32; NIADDR],
    /// `f_blk`: buffer for the indirect block at level i.
    f_blk: [Option<Vec<u8>>; NIADDR],
    /// `f_blksize`: size of the buffer.
    f_blksize: [usize; NIADDR],
    /// `f_blkno`: disk address of the block in the buffer.
    f_blkno: [Daddr; NIADDR],
    /// `f_buf`: buffer for a data block.
    f_buf: Option<Vec<u8>>,
    /// `f_buf_size`: size of the data block.
    f_buf_size: usize,
    /// `f_buf_blkno`: block number of the data block.
    f_buf_blkno: Daddr,
}

/// The reader for one inode flavour.
pub struct Ufs<D: Dinode>(PhantomData<D>);

impl<D: Dinode> Ufs<D> {
    /// `read_inode(inumber, f)`: read a new inode into a file structure.
    fn read_inode(inumber: Ufsino, io: DevIo, fp: &mut UfsFile<D>) -> Result<(), Errno> {
        let fs = &*fp.f_fs;
        let bsize = fs.fs_bsize as usize;
        let mut buf = vec![0u8; bsize];
        let mut rsize = 0;
        io.strategy(
            F_READ,
            D::fsbtodb(fs, ino_to_fsba(fs, inumber)),
            &mut buf,
            Some(&mut rsize),
        )?;
        if rsize != bsize {
            return Err(Errno::EIO);
        }

        let off = ino_to_fsbo(fs, inumber) * core::mem::size_of::<D>();
        fp.f_di = dinode_at(&buf, off).ok_or(Errno::EIO)?;

        // Clear out the old buffers
        fp.f_blkno = [-1; NIADDR];
        fp.f_buf_blkno = -1;
        fp.f_seekp = 0;
        Ok(())
    }

    /// `chmod_inode(inumber, f, mode)`: set an inode's mode on the disk.
    fn chmod_inode(inumber: Ufsino, io: DevIo, fp: &UfsFile<D>, mode: Mode) -> Result<(), Errno> {
        let fs = &*fp.f_fs;
        let bsize = fs.fs_bsize as usize;
        let mut buf = vec![0u8; bsize];
        let mut rsize = 0;
        let blk = D::fsbtodb(fs, ino_to_fsba(fs, inumber));
        io.strategy(F_READ, blk, &mut buf, Some(&mut rsize))?;
        if rsize != bsize {
            return Err(Errno::EIO);
        }

        let off = ino_to_fsbo(fs, inumber) * core::mem::size_of::<D>();
        let mut dp = dinode_at::<D>(&buf, off).ok_or(Errno::EIO)?;
        dp.set_mode(mode as u16);
        put_dinode(&mut buf, off, &dp);

        io.strategy(F_WRITE, blk, &mut buf, None)
    }

    /// `block_map(f, file_block, &disk_block)`: the disk block that holds logical block
    /// `file_block` of the file (0 for a hole).
    fn block_map(io: DevIo, fp: &mut UfsFile<D>, file_block: Daddr) -> Result<Daddr, Errno> {
        // di_db[0..NDADDR-1] map blocks 0..NDADDR-1 directly; di_ib[0] is the single
        // indirect block (NINDIR blocks), di_ib[1] the double indirect block (NINDIR**2),
        // di_ib[2] the triple indirect block (NINDIR**3).
        if file_block < NDADDR as Daddr {
            // Direct block.
            return Ok(fp.f_di.db(file_block as usize));
        }

        let mut file_block = file_block - NDADDR as Daddr;

        // nindir[0] = NINDIR, nindir[1] = NINDIR**2, nindir[2] = NINDIR**3
        let mut level = 0;
        while level < NIADDR {
            let n = Daddr::from(fp.f_nindir[level]);
            if file_block < n {
                break;
            }
            file_block -= n;
            level += 1;
        }
        if level == NIADDR {
            // Block number too high
            return Err(Errno::EFBIG);
        }

        let bsize = fp.f_fs.fs_bsize as usize;
        let mut ind_block_num = fp.f_di.ib(level);

        loop {
            if ind_block_num == 0 {
                return Ok(0); // missing
            }

            if fp.f_blkno[level] != ind_block_num {
                let blk = D::fsbtodb(&fp.f_fs, ind_block_num);
                let buf = fp.f_blk[level].get_or_insert_with(|| vec![0u8; bsize]);
                let mut rsize = 0;
                io.strategy(F_READ, blk, buf, Some(&mut rsize))?;
                fp.f_blksize[level] = rsize;
                if rsize != bsize {
                    return Err(Errno::EIO);
                }
                fp.f_blkno[level] = ind_block_num;
            }

            let idx;
            if level > 0 {
                let n = Daddr::from(fp.f_nindir[level - 1]);
                idx = file_block / n;
                file_block %= n;
            } else {
                idx = file_block;
            }

            let ind = fp.f_blk[level].as_deref().ok_or(Errno::EIO)?;
            if (idx as usize + 1) * D::DADDR_SIZE > ind.len() {
                return Err(Errno::EIO);
            }
            ind_block_num = D::indirect(ind, idx as usize);
            if level == 0 {
                return Ok(ind_block_num);
            }
            level -= 1;
        }
    }

    /// `buf_read_file(f, &buf, &size)`: read the block at the seek pointer into the data
    /// buffer; the offset of the seek pointer in it and the bytes that follow, up to the end
    /// of the file.
    fn buf_read_file(io: DevIo, fp: &mut UfsFile<D>) -> Result<(usize, usize), Errno> {
        let off = blkoff(&fp.f_fs, fp.f_seekp) as usize;
        let file_block = lblkno(&fp.f_fs, fp.f_seekp);
        let block_size = dblksize(&fp.f_fs, fp.f_di.size(), file_block as u64) as usize;

        if file_block != fp.f_buf_blkno {
            let disk_block = Self::block_map(io, fp, file_block)?;
            let bsize = fp.f_fs.fs_bsize as usize;
            let blk = D::fsbtodb(&fp.f_fs, disk_block);
            let buf = fp.f_buf.get_or_insert_with(|| vec![0u8; bsize]);

            if disk_block == 0 {
                buf[..block_size].fill(0);
                fp.f_buf_size = block_size;
            } else {
                let mut rsize = 0;
                io.strategy(F_READ, blk, &mut buf[..block_size], Some(&mut rsize))?;
                fp.f_buf_size = rsize;
            }

            fp.f_buf_blkno = file_block;
        }

        // The offset of the byte in the buffer and the size of the remainder of the buffer
        // after it, but truncated at the end of the file.
        let mut size = block_size - off;
        let left = fp.f_di.size().saturating_sub(fp.f_seekp as u64);
        if size as u64 > left {
            size = left as usize;
        }
        Ok((off, size))
    }

    /// `d_namlen`, or `d_type` on an old file system (`fs_maxsymlinklen <= 0`, where a
    /// little-endian machine finds the old 16-bit name length's low byte there).
    fn namlen(fs: &Fs, dp: &Direct<'_>) -> usize {
        if fs.fs_maxsymlinklen <= 0 {
            usize::from(dp.d_type)
        } else {
            usize::from(dp.d_namlen)
        }
    }

    /// `search_directory(name, f, &inumber)`: the inode of `name` in the directory open in
    /// `fp`.
    fn search_directory(name: &[u8], io: DevIo, fp: &mut UfsFile<D>) -> Result<Ufsino, Errno> {
        fp.f_seekp = 0;
        while (fp.f_seekp as u64) < fp.f_di.size() {
            let (off, size) = Self::buf_read_file(io, fp)?;
            let buf = fp.f_buf.as_deref().ok_or(Errno::EIO)?;
            let block = &buf[off..off + size];
            let mut pos = 0;
            while pos < block.len() {
                let Some(dp) = Direct::parse(&block[pos..]) else {
                    break;
                };
                if dp.d_reclen == 0 {
                    break;
                }
                if dp.d_ino != 0 {
                    let namlen = Self::namlen(&fp.f_fs, &dp);
                    if namlen == name.len()
                        && dp
                            .d_name
                            .get(..=namlen)
                            .is_some_and(|n| n[..namlen] == *name && n[namlen] == 0)
                    {
                        // found entry
                        return Ok(dp.d_ino);
                    }
                }
                pos += usize::from(dp.d_reclen);
            }
            fp.f_seekp += size as Off;
        }
        Err(Errno::ENOENT)
    }

    /// `ufs_open(path, f)`: open a file.
    pub fn open(path: &[u8], f: &mut OpenFile) -> Result<(), Errno> {
        f.f_fsdata = None;
        let io = f.io();

        // read the super block
        let mut sb = vec![0u8; SBSIZE];
        let mut buf_size = 0;
        io.strategy(F_READ, D::SBLOCK, &mut sb, Some(&mut buf_size))?;
        let fs = Fs::from_bytes(&sb).ok_or(Errno::EINVAL)?;
        if buf_size != SBSIZE
            || fs.fs_magic != D::MAGIC
            || fs.fs_bsize as usize > MAXBSIZE
            || (fs.fs_bsize as usize) < core::mem::size_of::<Fs>()
        {
            return Err(Errno::EINVAL);
        }

        // Calculate indirect block levels (in an `int`, as the C does).
        let mut f_nindir = [0i32; NIADDR];
        let mut mult: i32 = 1;
        for n in &mut f_nindir {
            mult = mult.wrapping_mul(nindir(&fs));
            *n = mult;
        }

        let mut fp = Box::new(UfsFile::<D> {
            f_seekp: 0,
            f_fs: Box::new(fs),
            f_di: D::default(),
            f_ino: 0,
            f_nindir,
            f_blk: [None, None, None],
            f_blksize: [0; NIADDR],
            f_blkno: [-1; NIADDR],
            f_buf: None,
            f_buf_size: 0,
            f_buf_blkno: -1,
        });

        let mut inumber = ROOTINO;
        Self::read_inode(inumber, io, &mut fp)?;

        // The path, NUL-terminated, in the buffer symbolic links are spliced into.
        let plen = path.iter().position(|&c| c == 0).unwrap_or(path.len());
        if plen > MAXPATHLEN {
            return Err(Errno::ENOENT);
        }
        let mut namebuf = [0u8; MAXPATHLEN + 1];
        namebuf[..plen].copy_from_slice(&path[..plen]);
        let mut cp = 0;
        let mut nlinks = 0;

        while namebuf[cp] != 0 {
            // Remove extra separators
            while namebuf[cp] == b'/' {
                cp += 1;
            }
            if namebuf[cp] == 0 {
                break;
            }

            // Check that current node is a directory.
            if (fp.f_di.mode() & IFMT) != IFDIR {
                return Err(Errno::ENOTDIR);
            }

            // Get next component of path name.
            let ncp = cp;
            while namebuf[cp] != 0 && namebuf[cp] != b'/' {
                if cp - ncp + 1 > MAXNAMLEN {
                    return Err(Errno::ENOENT);
                }
                cp += 1;
            }

            // Look up component in current directory. Save directory inumber in case we
            // find a symbolic link.
            let parent_inumber = inumber;
            let component = namebuf[ncp..cp].to_vec();
            inumber = Self::search_directory(&component, io, &mut fp)?;

            // Open next component.
            Self::read_inode(inumber, io, &mut fp)?;

            // Check for symbolic link.
            if (fp.f_di.mode() & IFMT) == IFLNK {
                let link_len = fp.f_di.size();
                let len = namebuf[cp..].iter().position(|&c| c == 0).unwrap_or(0);

                nlinks += 1;
                if link_len + len as u64 > MAXPATHLEN as u64 || nlinks > MAXSYMLINKS {
                    return Err(Errno::ENOENT);
                }
                let link_len = link_len as usize;

                namebuf.copy_within(cp..cp + len + 1, link_len);

                if link_len < fp.f_fs.fs_maxsymlinklen as usize {
                    let short = fp.f_di.shortlink();
                    namebuf[..link_len].copy_from_slice(&short[..link_len]);
                } else {
                    // Read file for symbolic link
                    let disk_block = Self::block_map(io, &mut fp, 0)?;
                    let bsize = fp.f_fs.fs_bsize as usize;
                    let mut buf = vec![0u8; bsize];
                    let mut rsize = 0;
                    io.strategy(
                        F_READ,
                        D::fsbtodb(&fp.f_fs, disk_block),
                        &mut buf,
                        Some(&mut rsize),
                    )?;
                    if link_len > buf.len() {
                        return Err(Errno::ENOENT);
                    }
                    namebuf[..link_len].copy_from_slice(&buf[..link_len]);
                }

                // If relative pathname, restart at parent directory. If absolute pathname,
                // restart at root.
                cp = 0;
                inumber = if namebuf[0] != b'/' {
                    parent_inumber
                } else {
                    ROOTINO
                };
                Self::read_inode(inumber, io, &mut fp)?;
            }
        }

        // Found terminal component.
        fp.f_ino = inumber;
        f.f_fsdata = Some(fp);
        Ok(())
    }

    /// `ufs_close(f)`.
    pub fn close(f: &mut OpenFile) -> Result<(), Errno> {
        f.f_fsdata = None;
        Ok(())
    }

    /// `ufs_read(f, start, size, &resid)`: copy a portion of a file, crossing block
    /// boundaries when necessary.
    pub fn read(f: &mut OpenFile, start: &mut [u8], resid: &mut usize) -> Result<(), Errno> {
        let io = f.io();
        let fp = f.fsdata::<UfsFile<D>>().ok_or(Errno::EBADF)?;
        let mut addr = 0;
        let mut rc = Ok(());
        while addr < start.len() {
            if fp.f_seekp as u64 >= fp.f_di.size() {
                break;
            }
            let (off, buf_size) = match Self::buf_read_file(io, fp) {
                Ok(r) => r,
                Err(e) => {
                    rc = Err(e);
                    break;
                }
            };
            let csize = (start.len() - addr).min(buf_size);
            let buf = fp.f_buf.as_deref().ok_or(Errno::EIO)?;
            start[addr..addr + csize].copy_from_slice(&buf[off..off + csize]);
            fp.f_seekp += csize as Off;
            addr += csize;
            if csize == 0 {
                break;
            }
        }
        *resid = start.len() - addr;
        rc
    }

    /// `ufs_write`: not implemented.
    pub fn write(_f: &mut OpenFile, _start: &[u8], _resid: &mut usize) -> Result<(), Errno> {
        Err(Errno::EROFS)
    }

    /// `ufs_seek(f, offset, where)`.
    pub fn seek(f: &mut OpenFile, offset: Off, whence: i32) -> Result<Off, Errno> {
        let fp = f.fsdata::<UfsFile<D>>().ok_or(Errno::EBADF)?;
        match whence {
            SEEK_SET => fp.f_seekp = offset,
            SEEK_CUR => fp.f_seekp += offset,
            SEEK_END => fp.f_seekp = fp.f_di.size() as Off - offset,
            _ => return Err(Errno::EINVAL),
        }
        Ok(fp.f_seekp)
    }

    /// `ufs_stat(f, sb)`: only the important stuff.
    pub fn stat(f: &mut OpenFile, sb: &mut Stat) -> Result<(), Errno> {
        let fp = f.fsdata::<UfsFile<D>>().ok_or(Errno::EBADF)?;
        sb.st_mode = Mode::from(fp.f_di.mode());
        sb.st_uid = fp.f_di.uid();
        sb.st_gid = fp.f_di.gid();
        sb.st_size = fp.f_di.size() as Off;
        Ok(())
    }

    /// `ufs_fchmod(f, mode)`.
    pub fn fchmod(f: &mut OpenFile, mode: Mode) -> Result<(), Errno> {
        let io = f.io();
        let fp = f.fsdata::<UfsFile<D>>().ok_or(Errno::EBADF)?;
        Self::chmod_inode(fp.f_ino, io, fp, mode)
    }

    /// `ufs_readdir(f, name)`: the next name in the directory (NUL-terminated), the end being
    /// `Err(Errno(-1))` with an empty name; `None` rewinds.
    pub fn readdir(f: &mut OpenFile, name: Option<&mut [u8]>) -> Result<(), Errno> {
        let io = f.io();
        let fp = f.fsdata::<UfsFile<D>>().ok_or(Errno::EBADF)?;
        let Some(name) = name else {
            fp.f_seekp = 0;
            return Ok(());
        };

        loop {
            // end of dir
            if fp.f_seekp as u64 >= fp.f_di.size() {
                if let Some(c) = name.first_mut() {
                    *c = 0;
                }
                return Err(Errno(-1));
            }

            let (off, buf_size) = Self::buf_read_file(io, fp)?;
            let buf = fp.f_buf.as_deref().ok_or(Errno::EIO)?;
            let block = &buf[off..off + buf_size];
            let mut pos = 0;
            let mut found = None;
            while pos < block.len() {
                let Some(dp) = Direct::parse(&block[pos..]) else {
                    pos = block.len();
                    break;
                };
                if dp.d_ino != 0 {
                    found = Some(dp);
                    break;
                }
                if dp.d_reclen == 0 {
                    pos = block.len();
                    break;
                }
                pos += usize::from(dp.d_reclen);
            }
            fp.f_seekp += pos.min(block.len()) as Off;
            let Some(dp) = found else {
                continue;
            };

            let namlen = Self::namlen(&fp.f_fs, &dp);
            // strncpy(name, dp->d_name, namlen + 1)
            let n = (namlen + 1).min(name.len());
            for (i, c) in name[..n].iter_mut().enumerate() {
                *c = dp.d_name.get(i).copied().unwrap_or(0);
            }
            if namlen < n {
                name[namlen] = 0;
            }

            fp.f_seekp += Off::from(dp.d_reclen);
            return Ok(());
        }
    }
}

/// The inode of type `D` at byte `off` of a block, if the block holds it.
fn dinode_at<D: Dinode>(buf: &[u8], off: usize) -> Option<D> {
    if off + core::mem::size_of::<D>() > buf.len() {
        return None;
    }
    // SAFETY: the bytes `off..off + size_of::<D>()` are in `buf` (checked above), the read is
    // unaligned, and the inode types are `#[repr(C)]` integers, valid for any byte pattern.
    Some(unsafe { buf.as_ptr().add(off).cast::<D>().read_unaligned() })
}

/// Writes inode `dp` at byte `off` of a block.
fn put_dinode<D: Dinode>(buf: &mut [u8], off: usize, dp: &D) {
    if off + core::mem::size_of::<D>() <= buf.len() {
        // SAFETY: the destination bytes are in `buf` (checked above) and the write is
        // unaligned.
        unsafe { buf.as_mut_ptr().add(off).cast::<D>().write_unaligned(*dp) };
    }
}

/// `ufs_open`.
pub fn ufs_open(path: &[u8], f: &mut OpenFile) -> Result<(), Errno> {
    Ufs::<Ufs1Dinode>::open(path, f)
}

/// `ufs_close`.
pub fn ufs_close(f: &mut OpenFile) -> Result<(), Errno> {
    Ufs::<Ufs1Dinode>::close(f)
}

/// `ufs_read`.
pub fn ufs_read(f: &mut OpenFile, start: &mut [u8], resid: &mut usize) -> Result<(), Errno> {
    Ufs::<Ufs1Dinode>::read(f, start, resid)
}

/// `ufs_write`.
pub fn ufs_write(f: &mut OpenFile, start: &[u8], resid: &mut usize) -> Result<(), Errno> {
    Ufs::<Ufs1Dinode>::write(f, start, resid)
}

/// `ufs_seek`.
pub fn ufs_seek(f: &mut OpenFile, offset: Off, whence: i32) -> Result<Off, Errno> {
    Ufs::<Ufs1Dinode>::seek(f, offset, whence)
}

/// `ufs_stat`.
pub fn ufs_stat(f: &mut OpenFile, sb: &mut Stat) -> Result<(), Errno> {
    Ufs::<Ufs1Dinode>::stat(f, sb)
}

/// `ufs_readdir`.
pub fn ufs_readdir(f: &mut OpenFile, name: Option<&mut [u8]>) -> Result<(), Errno> {
    Ufs::<Ufs1Dinode>::readdir(f, name)
}

/// `ufs_fchmod`.
pub fn ufs_fchmod(f: &mut OpenFile, mode: Mode) -> Result<(), Errno> {
    Ufs::<Ufs1Dinode>::fchmod(f, mode)
}
/* </CODE> */

/* <TESTS> */
#[cfg(test)]
mod tests {
    // The FFS1, FFS2 and ISO 9660 readers, `cread` and the open file table, on the images
    // `testdata/gen_fixtures.py` made with OpenBSD's makefs (the same tree in each).

    use alloc::string::String;
    use alloc::vec;
    use alloc::vec::Vec;

    use crate::cread::{close, lseek, open, read};
    use crate::hdr::stat::{S_IFDIR, S_IFMT, S_IFREG, Stat};
    use crate::readdir::{closedir, opendir, readdir};
    use crate::saerrno::Errno;
    use crate::stand::{SEEK_SET, SOPEN_MAX};
    use crate::stat::stat;
    use crate::testutil::{add_fixture, output, setup};

    /// The whole file `path`, read through `cread`.
    fn slurp(path: &str) -> Result<Vec<u8>, Errno> {
        let fd = open(path.as_bytes(), 0)?;
        let mut out = Vec::new();
        let mut buf = vec![0u8; 1000];
        loop {
            let n = read(fd, &mut buf)?;
            if n == 0 {
                break;
            }
            out.extend_from_slice(&buf[..n]);
        }
        close(fd)?;
        Ok(out)
    }

    /// The names `opendir`/`readdir` give for `path`, sorted.
    fn ls(path: &str) -> Vec<String> {
        let fd = opendir(path.as_bytes()).unwrap();
        let mut names = Vec::new();
        let mut name = [0u8; 256];
        while readdir(fd, &mut name).is_ok() {
            let end = name.iter().position(|&c| c == 0).unwrap();
            names.push(String::from_utf8(name[..end].to_vec()).unwrap());
        }
        closedir(fd);
        names.sort();
        names
    }

    fn big() -> Vec<u8> {
        (0..10000)
            .flat_map(|i| alloc::format!("{i:06}\n").into_bytes())
            .collect()
    }

    fn check_ffs(dev: &str) {
        let p = |s: &str| alloc::format!("{dev}:{s}");
        assert_eq!(slurp(&p("/hello.txt")).unwrap(), b"hello from ffs\n");
        assert_eq!(slurp(&p("//dir/deep/sub.txt")).unwrap(), b"deep file\n");
        // symbolic links, relative and absolute
        assert_eq!(slurp(&p("/link")).unwrap(), b"deep file\n");
        assert_eq!(slurp(&p("/dir/abs")).unwrap(), b"hello from ffs\n");
        // 70000 bytes in 4096-byte blocks: the single indirect block
        assert_eq!(slurp(&p("/big.dat")).unwrap(), big());
        // gzip, inflated by cread
        assert_eq!(
            slurp(&p("/hello.gz")).unwrap(),
            b"compressed hello\n".repeat(50)
        );
        assert_eq!(slurp(&p("/nothere")), Err(Errno::ENOENT));
        assert_eq!(slurp(&p("/hello.txt/x")), Err(Errno::ENOTDIR));

        let mut sb = Stat::default();
        stat(p("/etc/boot.conf").as_bytes(), &mut sb).unwrap();
        assert_eq!((sb.st_mode & S_IFMT, sb.st_mode & 0o777), (S_IFREG, 0o644));
        assert_eq!((sb.st_uid, sb.st_size), (0, 27));
        stat(p("/dir").as_bytes(), &mut sb).unwrap();
        assert_eq!(sb.st_mode & S_IFMT, S_IFDIR);

        assert_eq!(
            ls(&p("/")),
            [
                ".",
                "..",
                "big.dat",
                "dir",
                "etc",
                "hello.gz",
                "hello.txt",
                "link"
            ]
        );
        assert_eq!(ls(&p("/dir")), [".", "..", "abs", "deep"]);

        // seeking back in a compressed file starts over; forwards reads ahead
        let fd = open(p("/hello.gz").as_bytes(), 0).unwrap();
        let mut b = [0u8; 9];
        assert_eq!(lseek(fd, 17 * 3 + 11, SEEK_SET), Ok(62));
        read(fd, &mut b).unwrap();
        assert_eq!(&b, b"hello\ncom");
        assert_eq!(lseek(fd, 0, SEEK_SET), Ok(0));
        read(fd, &mut b).unwrap();
        assert_eq!(&b, b"compresse");
        close(fd).unwrap();

        // every descriptor is free again
        let fds: Vec<usize> = (0..SOPEN_MAX)
            .map(|_| open(p("/hello.txt").as_bytes(), 0).unwrap())
            .collect();
        assert_eq!(open(p("/hello.txt").as_bytes(), 0), Err(Errno::EMFILE));
        for fd in fds {
            close(fd).unwrap();
        }
    }

    #[test]
    fn ffs1() {
        let _g = setup();
        add_fixture("ffs1", "ffs1.img.z");
        check_ffs("ffs1");
    }

    #[test]
    fn ffs2() {
        let _g = setup();
        add_fixture("ffs2", "ffs2.img.z");
        check_ffs("ffs2");
    }

    #[test]
    fn cd9660() {
        let _g = setup();
        add_fixture("cd", "cd9660.img.z");
        assert_eq!(slurp("cd:/hello.txt").unwrap(), b"hello from ffs\n");
        assert_eq!(slurp("cd:/dir/deep/sub.txt").unwrap(), b"deep file\n");
        assert_eq!(slurp("cd:/BIG.DAT").unwrap(), big());
        assert_eq!(slurp("cd:/missing.txt"), Err(Errno::ENOENT));
    }

    #[test]
    fn console_output() {
        let _g = setup();
        crate::cons::cninit();
        crate::printf!("ab\tc{:>4}|{:08x}\n", 42, 0x3f8);
        crate::putchar::putchar(0o177);
        // tabs expand to the next multiple of 8; a newline gets a carriage return; DEL erases
        assert_eq!(output(), "ab      c  42|000003f8\n\r\x08 \x08");
    }
}
/* </TESTS> */
