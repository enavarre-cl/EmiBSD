/*	$OpenBSD: ext2fs_balloc.c,v 1.27 2019/07/19 00:24:31 cheloha Exp $	*/
/*	$NetBSD: ext2fs_balloc.c,v 1.10 2001/07/04 21:16:01 chs Exp $	*/
/* <LICENSES> */
/*
 * Copyright (c) 1997 Manuel Bouyer.
 * Copyright (c) 1982, 1986, 1989, 1993
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
 *	@(#)ffs_balloc.c	8.4 (Berkeley) 9/23/93
 * Modified for ext2fs by Manuel Bouyer.
 */
/* </LICENSES> */

//! ext2fs block allocation for a file: `ext2fs_buf_alloc` defines the structure of file
//! system storage by allocating the physical blocks on a device given the inode and the
//! logical block number in a file, indirect blocks included.
//!
//! Upstream: sys/ufs/ext2fs/ext2fs_balloc.c @ 3ce1f3f79392
//!
//! ## Deviations
//! - The buffer comes back as the `Ok` value (the C's `*bpp`).
//! - The block pointers of an indirect block are read and written at their index in the
//!   buffer's bytes (`bap[i]`), little-endian (`letoh32`/`htole32`).
//! - On failure the C marks the change in `i_e2fs_flags` (the on-disk `e2di_flags`, where
//!   `IN_CHANGE | IN_UPDATE` are `EXT2_UNRM | EXT2_COMPR`); here it goes to `i_flag`, where
//!   every other caller puts it.

use crate::kern::subr_prf::panic;
use crate::kern::vfs_bio::{bdwrite, bread, brelse, bwrite, getblk};
use crate::sys::buf::{B_CLRBUF, B_INVAL, B_SYNC, Buf, clrbuf};
use crate::sys::errno::Errno;
use crate::sys::param::btodb;
use crate::sys::systm::INFSLP;
use crate::sys::types::Daddr;
use crate::sys::ucred::Ucred;
use crate::sys::vnode::Vnode;
use crate::ufs::ext2fs::ext2fs::fsbtodb;
use crate::ufs::ext2fs::ext2fs_alloc::{ext2fs_alloc, ext2fs_blkfree, ext2fs_blkpref};
use crate::ufs::ext2fs::ext2fs_dinode::{NDADDR, NIADDR};
use crate::ufs::ufs::inode::{IN_CHANGE, IN_UPDATE, Indir, Inode};
use crate::ufs::ufs::ufs_bmap::ufs_getlbns;

/// `getblk(vp, blkno, size, 0, INFSLP)`, which cannot fail without `slpflag`.
fn getblk_wait(vp: &'static Vnode, blkno: Daddr, size: i32) -> &'static Buf {
    loop {
        if let Some(bp) = getblk(vp, blkno, size, 0, INFSLP) {
            return bp;
        }
    }
}

/// `bap[i]` of an indirect block's bytes, host order (`letoh32`).
fn bap_get(b: &[u8], i: usize) -> u32 {
    u32::from_le_bytes([b[4 * i], b[4 * i + 1], b[4 * i + 2], b[4 * i + 3]])
}

/// `bap[i] = htole32(v)`.
fn bap_set(b: &mut [u8], i: usize, v: u32) {
    b[4 * i..4 * i + 4].copy_from_slice(&v.to_le_bytes());
}

/// The inode's block pointers as the bytes they are stored in, for `ext2fs_blkpref`.
fn inode_bap(ip: &Inode) -> [u8; (NDADDR + NIADDR) * 4] {
    let mut b = [0u8; (NDADDR + NIADDR) * 4];
    for (i, v) in ip.i_e2fs_blocks().iter().enumerate() {
        b[4 * i..4 * i + 4].copy_from_slice(&v.to_ne_bytes());
    }
    b
}

/// `ext2fs_buf_alloc`: the buffer of logical block `bn` of `ip`, allocating it and the
/// indirect blocks on its path as needed. `flags` holds `B_CLRBUF` (clear a new buffer)
/// and `B_SYNC` (write the indirect blocks synchronously).
pub fn ext2fs_buf_alloc(
    ip: &Inode,
    bn: u32,
    _size: i32,
    cred: *const Ucred,
    flags: i32,
) -> Result<&'static Buf, Errno> {
    let fs = ip.e2fs();
    let vp = ip.itov();
    let bsize = fs.e2fs_bsize.get();
    let lbn = Daddr::from(bn);

    // The first NDADDR blocks are direct blocks
    if (bn as usize) < NDADDR {
        let nb = u32::from_le(ip.i_e2fs_block(bn as usize));
        if nb != 0 {
            let (bp, error) = bread(vp, Daddr::from(bn), bsize);
            if let Err(e) = error {
                brelse(bp);
                return Err(e);
            }
            return Ok(bp);
        }

        // allocate a new direct block.
        let bap = inode_bap(ip);
        let pref = ext2fs_blkpref(ip, bn, bn as i32, Some(&bap));
        let mut newb = 0;
        ext2fs_alloc(ip, bn, pref as u32, cred, &mut newb)?;
        ip.i_e2fs_last_lblk().set(lbn as u32);
        ip.i_e2fs_last_blk().set(newb);
        ip.set_i_e2fs_block(bn as usize, newb.to_le());
        ip.set_flag(IN_CHANGE | IN_UPDATE);
        let bp = getblk_wait(vp, Daddr::from(bn), bsize);
        bp.b_blkno.set(fsbtodb(fs, Daddr::from(newb)));
        if flags & B_CLRBUF != 0 {
            // SAFETY: the buffer is ours (busy from getblk) and mapped.
            unsafe { clrbuf(bp) };
        }
        return Ok(bp);
    }
    // Determine the number of levels of indirection.
    let mut indirs = [Indir::default(); NIADDR + 2];
    let mut num = 0;
    ufs_getlbns(vp, Daddr::from(bn), &mut indirs, Some(&mut num))?;
    #[cfg(feature = "diagnostic")]
    if num < 1 {
        panic(format_args!(
            "ext2fs_balloc: ufs_getlbns returned indirect block"
        ));
    }
    let num = num as usize - 1;

    // The blocks allocated so far (allociblk) and the inode's slot of the first indirect
    // block, if it was allocated (allocib).
    let mut allociblk = [0u32; NIADDR + 1];
    let mut nalloc = 0;
    let mut allocib: Option<usize> = None;
    let mut unwindidx: isize = -1;

    let error: Errno = 'fail: {
        // Fetch the first indirect block allocating if necessary.
        let mut nb = u32::from_le(ip.i_e2fs_block(NDADDR + indirs[0].in_off as usize));
        if nb == 0 {
            let pref = ext2fs_blkpref(ip, lbn as u32, 0, None);
            let mut newb = 0;
            ext2fs_alloc(ip, lbn as u32, pref as u32, cred, &mut newb)?;
            nb = newb;
            allociblk[nalloc] = nb;
            nalloc += 1;
            ip.i_e2fs_last_blk().set(newb);
            let bp = getblk_wait(vp, indirs[1].in_lbn, bsize);
            bp.b_blkno.set(fsbtodb(fs, Daddr::from(newb)));
            // SAFETY: the buffer is ours (busy from getblk) and mapped.
            unsafe { clrbuf(bp) };
            // Write synchronously so that indirect blocks never point at garbage.
            if let Err(e) = bwrite(bp) {
                break 'fail e;
            }
            unwindidx = 0;
            let slot = NDADDR + indirs[0].in_off as usize;
            allocib = Some(slot);
            ip.set_i_e2fs_block(slot, newb.to_le());
            ip.set_flag(IN_CHANGE | IN_UPDATE);
        }

        // Fetch through the indirect blocks, allocating as necessary.
        let mut i = 1;
        let bp = loop {
            let (bp, error) = bread(vp, indirs[i].in_lbn, bsize);
            if let Err(e) = error {
                brelse(bp);
                break 'fail e;
            }
            // SAFETY: the buffer is ours (busy from bread) and mapped.
            nb = bap_get(unsafe { bp.data() }, indirs[i].in_off as usize);
            if i == num {
                break bp;
            }
            i += 1;
            if nb != 0 {
                brelse(bp);
                continue;
            }
            let pref = ext2fs_blkpref(ip, lbn as u32, 0, None);
            let mut newb = 0;
            if let Err(e) = ext2fs_alloc(ip, lbn as u32, pref as u32, cred, &mut newb) {
                brelse(bp);
                break 'fail e;
            }
            nb = newb;
            allociblk[nalloc] = nb;
            nalloc += 1;
            ip.i_e2fs_last_blk().set(newb);
            let nbp = getblk_wait(vp, indirs[i].in_lbn, bsize);
            nbp.b_blkno.set(fsbtodb(fs, Daddr::from(nb)));
            // SAFETY: the buffer is ours (busy from getblk) and mapped.
            unsafe { clrbuf(nbp) };
            // Write synchronously so that indirect blocks never point at garbage.
            if let Err(e) = bwrite(nbp) {
                brelse(bp);
                break 'fail e;
            }
            if unwindidx < 0 {
                unwindidx = i as isize - 1;
            }
            // SAFETY: the buffer is ours (busy from bread) and mapped.
            bap_set(unsafe { bp.data() }, indirs[i - 1].in_off as usize, nb);
            // If required, write synchronously, otherwise use delayed write.
            if flags & B_SYNC != 0 {
                let _ = bwrite(bp);
            } else {
                bdwrite(bp);
            }
        };

        // Get the data block, allocating if necessary.
        if nb == 0 {
            let pref = {
                // SAFETY: the buffer is ours (busy from bread) and mapped.
                let bap = unsafe { bp.data() };
                ext2fs_blkpref(ip, lbn as u32, indirs[num].in_off, Some(bap))
            };
            let mut newb = 0;
            if let Err(e) = ext2fs_alloc(ip, lbn as u32, pref as u32, cred, &mut newb) {
                brelse(bp);
                break 'fail e;
            }
            nb = newb;
            // The C also records the data block in allociblk, which nothing reads once the
            // block is in place.
            ip.i_e2fs_last_lblk().set(lbn as u32);
            ip.i_e2fs_last_blk().set(newb);
            // SAFETY: the buffer is ours (busy from bread) and mapped.
            bap_set(unsafe { bp.data() }, indirs[num].in_off as usize, nb);
            // If required, write synchronously, otherwise use delayed write.
            if flags & B_SYNC != 0 {
                let _ = bwrite(bp);
            } else {
                bdwrite(bp);
            }
            let nbp = getblk_wait(vp, lbn, bsize);
            nbp.b_blkno.set(fsbtodb(fs, Daddr::from(nb)));
            if flags & B_CLRBUF != 0 {
                // SAFETY: the buffer is ours (busy from getblk) and mapped.
                unsafe { clrbuf(nbp) };
            }
            return Ok(nbp);
        }
        brelse(bp);
        let nbp = if flags & B_CLRBUF != 0 {
            let (nbp, error) = bread(vp, lbn, bsize);
            if let Err(e) = error {
                brelse(nbp);
                break 'fail e;
            }
            nbp
        } else {
            let nbp = getblk_wait(vp, lbn, bsize);
            nbp.b_blkno.set(fsbtodb(fs, Daddr::from(nb)));
            nbp
        };

        return Ok(nbp);
    };

    // fail:
    // If we have failed part way through block allocation, we have to deallocate any
    // indirect blocks that we have allocated.
    let mut deallocated: u32 = 0;
    for &blk in &allociblk[..nalloc] {
        ext2fs_blkfree(ip, blk);
        deallocated += bsize as u32;
    }
    if unwindidx >= 0 {
        let unwindidx = unwindidx as usize;
        if unwindidx == 0 {
            if let Some(slot) = allocib {
                ip.set_i_e2fs_block(slot, 0);
            }
        } else {
            let (bp, r) = bread(vp, indirs[unwindidx].in_lbn, bsize);
            if let Err(r) = r {
                panic(format_args!(
                    "Could not unwind indirect block, error {}",
                    r as i32
                ));
            }
            // SAFETY: the buffer is ours (busy from bread) and mapped.
            bap_set(unsafe { bp.data() }, indirs[unwindidx].in_off as usize, 0);
            if flags & B_SYNC != 0 {
                let _ = bwrite(bp);
            } else {
                bdwrite(bp);
            }
        }
        for ind in &indirs[unwindidx + 1..=num] {
            let bp = getblk_wait(vp, ind.in_lbn, bsize);
            bp.set(B_INVAL);
            brelse(bp);
        }
    }
    if deallocated != 0 {
        let nblock = btodb(deallocated as usize) as u32;
        ip.set_i_e2fs_nblock(ip.i_e2fs_nblock().wrapping_sub(nblock));
        ip.set_flag(IN_CHANGE | IN_UPDATE);
    }
    Err(error)
}
