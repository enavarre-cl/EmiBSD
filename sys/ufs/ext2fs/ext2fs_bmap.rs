/*	$OpenBSD: ext2fs_bmap.c,v 1.29 2024/04/13 23:44:11 jsg Exp $	*/
/*	$NetBSD: ext2fs_bmap.c,v 1.5 2000/03/30 12:41:11 augustss Exp $	*/
/* <LICENSES> */
/*
 * Copyright (c) 1997 Manuel Bouyer.
 * Copyright (c) 1989, 1991, 1993
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
 *	@(#)ufs_bmap.c	8.6 (Berkeley) 1/21/94
 * Modified for ext2fs by Manuel Bouyer.
 */
/* </LICENSES> */

/* <CODE> */
//! ext2fs block mapping: `ext2fs_bmap` converts the logical block number of a file to its
//! physical block number on the disk, through the inode's block pointers and indirect blocks
//! (`ext2fs_bmaparray`) or through its ext4 extent tree (`ext4_bmapext`).
//!
//! Upstream: sys/ufs/ext2fs/ext2fs_bmap.c @ 3ce1f3f79392
//!
//! Indirect blocks are on the vnode for the file. They are given negative logical block
//! numbers. Indirect blocks are addressed by the negative address of the first data block
//! to which they point. Double indirect blocks are addressed by one less than the address of
//! the first indirect block to which they point. Triple indirect blocks are addressed by one
//! less than the address of the first double indirect block to which they point.
//!
//! ## Deviations
//! - `ext4_bmapext` and `ext2fs_bmaparray` take the `struct indir` array as a slice and its
//!   count as `Option<&mut i32>`, as `ufs_bmaparray` does; the block pointers of an indirect
//!   block are read at their index in the buffer's bytes, little-endian (`letoh32`).
//! - `ext4_bmapext` starts from an empty path (the C's is uninitialised) and releases the
//!   leaf buffer `ext4_ext_find_extent` may hold once the extent is read; the C keeps it
//!   busy for ever.
//! - `curproc->p_ru.ru_inblock++` is skipped when no thread runs, as `vfs_bio.rs` does.

use core::sync::atomic::Ordering;

use crate::kern::subr_prf::panic;
use crate::kern::vfs_bio::{BCSTATS, biowait, brelse, getblk, incore};
use crate::kern::vfs_vops::VOP_STRATEGY;
use crate::machine::cpu::curproc;
use crate::sys::buf::{B_DELWRI, B_DONE, B_READ, Buf};
use crate::sys::errno::Errno;
use crate::sys::param::MAXBSIZE;
use crate::sys::systm::INFSLP;
use crate::sys::types::Daddr;
use crate::sys::vnode::{Vnode, VopBmapArgs};
use crate::ufs::ext2fs::ext2fs::fsbtodb;
use crate::ufs::ext2fs::ext2fs_dinode::{EXT4_EXTENTS, NDADDR, NIADDR};
use crate::ufs::ext2fs::ext2fs_extents::{Ext4ExtentPath, ext4_ext_find_extent};
use crate::ufs::ufs::inode::{Indir, vtoi};
use crate::ufs::ufs::ufs_bmap::ufs_getlbns;
use crate::ufs::ufs::ufsmount::{blkptrtodb, is_sequential, mnindir, vfstoufs};

/// `ext2fs_bmap` (`vop_bmap`): the device vnode and the disk block of logical block
/// `a_bn`.
pub fn ext2fs_bmap(ap: &mut VopBmapArgs<'_>) -> Result<(), Errno> {
    // Check for underlying vnode requests and ensure that logical to physical mapping is
    // requested.
    if let Some(vpp) = ap.a_vpp.as_deref_mut() {
        *vpp = Some(vtoi(ap.a_vp).i_devvp());
    }
    let Some(bnp) = ap.a_bnp.as_deref_mut() else {
        return Ok(());
    };

    if vtoi(ap.a_vp).i_e2fs_flags() & EXT4_EXTENTS != 0 {
        return ext4_bmapext(ap.a_vp, ap.a_bn, bnp, None, None, ap.a_runp.as_deref_mut());
    }
    ext2fs_bmaparray(ap.a_vp, ap.a_bn, bnp, None, None, ap.a_runp.as_deref_mut())
}

/// `ext4_bmapext`: logical block number of a file -> physical block number on disk within
/// ext4 extents. `*bnp` is -1 when the extent maps the block to disk block 0.
pub fn ext4_bmapext(
    vp: &'static Vnode,
    bn: Daddr,
    bnp: &mut Daddr,
    _ap: Option<&mut [Indir]>,
    nump: Option<&mut i32>,
    runp: Option<&mut i32>,
) -> Result<(), Errno> {
    let ip = vtoi(vp);
    let fs = ip.e2fs();

    if let Some(runp) = runp {
        *runp = 0;
    }
    if let Some(nump) = nump {
        *nump = 0;
    }

    let mut path = Ext4ExtentPath::new();
    let ep = ext4_ext_find_extent(fs, ip, bn, &mut path).and_then(|p| p.ext());
    if let Some(bp) = path.ep_bp.take() {
        brelse(bp);
    }
    let Some(ep) = ep else {
        return Err(Errno::EIO);
    };

    let pos = bn - Daddr::from(ep.e_blk)
        + ((Daddr::from(ep.e_start_hi) << 32) | Daddr::from(ep.e_start_lo));
    *bnp = fsbtodb(fs, pos);
    if *bnp == 0 {
        *bnp = -1;
    }
    Ok(())
}

/// The buffer of an indirect block, from `getblk` (which cannot fail without `slpflag`).
fn getblk_wait(vp: &'static Vnode, blkno: Daddr, size: i32) -> &'static Buf {
    loop {
        if let Some(bp) = getblk(vp, blkno, size, 0, INFSLP) {
            return bp;
        }
    }
}

/// `((u_int32_t *)b_data)[i]` as stored (little-endian on disk): the raw block pointer.
fn bap32(b: &[u8], i: usize) -> u32 {
    u32::from_ne_bytes([b[4 * i], b[4 * i + 1], b[4 * i + 2], b[4 * i + 3]])
}

/// `ext2fs_bmaparray`: the bmap conversion, and if requested the array of logical blocks
/// which must be traversed to get to a block. Each entry contains the offset into that
/// block that gets you to the next block and the disk address of the block (if it is
/// assigned). `*bnp` is -1 for a block that is not allocated (a hole); `*runp`, if asked
/// for, the number of blocks that follow it contiguously on the disk.
pub fn ext2fs_bmaparray(
    vp: &'static Vnode,
    mut bn: Daddr,
    bnp: &mut Daddr,
    ap: Option<&mut [Indir]>,
    nump: Option<&mut i32>,
    mut runp: Option<&mut i32>,
) -> Result<(), Errno> {
    let ip = vtoi(vp);
    let Some(mp) = vp.v_mount.get() else {
        panic(format_args!("ext2fs_bmaparray: vnode without a mount"));
    };
    let ump = vfstoufs(mp);

    #[cfg(feature = "diagnostic")]
    if ap.is_some() != nump.is_some() {
        panic(format_args!("ext2fs_bmaparray: invalid arguments"));
    }

    let mut maxrun = 0;
    if let Some(runp) = runp.as_deref_mut() {
        // XXX
        // If MAXBSIZE is the largest transfer the disks can handle, we probably want maxrun
        // to be 1 block less so that we don't create a block larger than the device can
        // handle.
        *runp = 0;
        maxrun = MAXBSIZE as i32 / mp.mnt_stat.get().f_iosize as i32 - 1;
    }

    let mut a = [Indir::default(); NIADDR + 1];
    let xap: &mut [Indir] = match ap {
        Some(ap) => ap,
        None => &mut a,
    };
    let mut num_local = 0;
    let nump = match nump {
        Some(n) => n,
        None => &mut num_local,
    };
    ufs_getlbns(vp, bn, xap, Some(&mut *nump))?;

    let mut num = *nump;
    if num == 0 {
        let blk = |i: Daddr| Daddr::from(u32::from_le(ip.i_e2fs_block(i as usize)));
        *bnp = blkptrtodb(ump, blk(bn));
        if *bnp == 0 {
            *bnp = -1;
        } else if let Some(runp) = runp {
            bn += 1;
            while bn < NDADDR as Daddr && *runp < maxrun && is_sequential(ump, blk(bn - 1), blk(bn))
            {
                bn += 1;
                *runp += 1;
            }
        }
        return Ok(());
    }

    // Get disk address out of indirect block array
    let mut daddr = u32::from_le(ip.i_e2fs_block(NDADDR + xap[0].in_off as usize)) as i32;

    #[cfg(feature = "diagnostic")]
    if num > NIADDR as i32 + 1 || num < 1 {
        crate::kprintf!("ext2fs_bmaparray: num={}\n", num);
        panic(format_args!("ext2fs_bmaparray: num"));
    }

    let mut bp: Option<&'static Buf> = None;
    let mut x = 1;
    loop {
        num -= 1;
        if num == 0 {
            break;
        }
        // Exit the loop if there is no disk address assigned yet and the indirect block
        // isn't in the cache, or if we were looking for an indirect block and we've found
        // it.
        let metalbn = xap[x].in_lbn;
        if (daddr == 0 && incore(vp, metalbn).is_none()) || metalbn == bn {
            break;
        }
        // If we get here, we've either got the block in the cache or we have a disk address
        // for it, go fetch it.
        if let Some(b) = bp {
            brelse(b);
        }

        xap[x].in_exists = 1;
        let b = getblk_wait(vp, metalbn, mp.mnt_stat.get().f_iosize as i32);
        bp = Some(b);
        if b.isset(B_DONE | B_DELWRI) {
            // Already there.
        } else if cfg!(feature = "diagnostic") && daddr == 0 {
            panic(format_args!("ext2fs_bmaparry: indirect block not in cache"));
        } else {
            b.b_blkno.set(blkptrtodb(ump, Daddr::from(daddr)));
            b.set(B_READ);
            if let Some(bvp) = b.b_vp.get() {
                let _ = VOP_STRATEGY(bvp, b);
            }
            if let Some(p) = curproc() {
                p.p_ru.ru_inblock.set(p.p_ru.ru_inblock.get() + 1); // XXX
            }
            BCSTATS.pendingreads.fetch_add(1, Ordering::Relaxed);
            if let Err(e) = biowait(b) {
                brelse(b);
                return Err(e);
            }
        }

        // SAFETY: the buffer is ours (busy, from getblk) and mapped, and no other slice of
        // it is alive.
        let data = unsafe { b.data() };
        let off = xap[x].in_off as usize;
        daddr = u32::from_le(bap32(data, off)) as i32;
        if num == 1
            && daddr != 0
            && let Some(runp) = runp.as_deref_mut()
        {
            let mut i = off + 1;
            while (i as u64) < mnindir(ump)
                && *runp < maxrun
                && is_sequential(
                    ump,
                    Daddr::from(bap32(data, i - 1)),
                    Daddr::from(bap32(data, i)),
                )
            {
                i += 1;
                *runp += 1;
            }
        }
        x += 1;
    }
    if let Some(b) = bp {
        brelse(b);
    }

    let daddr = blkptrtodb(ump, Daddr::from(daddr));
    *bnp = if daddr == 0 { -1 } else { daddr };
    Ok(())
}
/* </CODE> */
