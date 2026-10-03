/*	$OpenBSD: ufs_bmap.c,v 1.37 2021/12/12 09:14:59 visa Exp $	*/
/*	$NetBSD: ufs_bmap.c,v 1.3 1996/02/09 22:36:00 christos Exp $	*/
/* <LICENSES> */
/*
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
 */
/* </LICENSES> */

//! Block mapping: `bmap` converts the logical block number of a file to its physical block
//! number on the disk. The conversion is done by using the logical block number to index
//! into the array of block pointers described by the dinode.
//!
//! Upstream: sys/ufs/ufs/ufs_bmap.c @ 3ce1f3f79392
//!
//! Indirect blocks are on the vnode for the file. They are given negative logical block
//! numbers. Indirect blocks are addressed by the negative address of the first data block
//! to which they point. Double indirect blocks are addressed by one less than the address of
//! the first indirect block to which they point. Triple indirect blocks are addressed by one
//! less than the address of the first double indirect block to which they point.
//!
//! ## Deviations
//! - `ufs_bmaparray` and `ufs_getlbns` take the `struct indir` array as a slice and its count
//!   as `Option<&mut i32>` (the C's NULL `ap`/`nump` is `None`); the block pointers in an
//!   indirect block are read at their index in the buffer's bytes
//!   (`inode.rs`, `daddr32_at`/`daddr64_at`).
//! - `curproc->p_ru.ru_inblock++` is skipped when no thread runs, as `vfs_bio.rs` does.

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
use crate::ufs::ufs::dinode::{NDADDR, NIADDR};
#[cfg(feature = "ffs2")]
use crate::ufs::ufs::inode::daddr64_at;
use crate::ufs::ufs::inode::{Indir, daddr32_at, vtoi};
#[cfg(feature = "ffs2")]
use crate::ufs::ufs::ufsmount::UM_UFS2;
use crate::ufs::ufs::ufsmount::{blkptrtodb, is_sequential, mnindir, vfstoufs};

/// `ufs_bmap` (`vop_bmap`): the device vnode and the disk block of logical block `a_bn`.
pub fn ufs_bmap(ap: &mut VopBmapArgs<'_>) -> Result<(), Errno> {
    // Check for underlying vnode requests and ensure that logical to physical mapping is
    // requested.
    if let Some(vpp) = ap.a_vpp.as_deref_mut() {
        *vpp = Some(vtoi(ap.a_vp).i_devvp());
    }
    let Some(bnp) = ap.a_bnp.as_deref_mut() else {
        return Ok(());
    };

    ufs_bmaparray(ap.a_vp, ap.a_bn, bnp, None, None, ap.a_runp.as_deref_mut())
}

/// The buffer of an indirect block, from `getblk` (which cannot fail without `slpflag`).
fn getblk_wait(vp: &'static Vnode, blkno: Daddr, size: i32) -> &'static Buf {
    loop {
        if let Some(bp) = getblk(vp, blkno, size, 0, INFSLP) {
            return bp;
        }
    }
}

/// `ufs_bmaparray`: the bmap conversion, and if requested the array of logical blocks which
/// must be traversed to get to a block. Each entry contains the offset into that block that
/// gets you to the next block and the disk address of the block (if it is assigned).
/// `*bnp` is -1 for a block that is not allocated (a hole); `*runp`, if asked for, the
/// number of blocks that follow it contiguously on the disk.
pub fn ufs_bmaparray(
    vp: &'static Vnode,
    mut bn: Daddr,
    bnp: &mut Daddr,
    ap: Option<&mut [Indir]>,
    nump: Option<&mut i32>,
    mut runp: Option<&mut i32>,
) -> Result<(), Errno> {
    let ip = vtoi(vp);
    let Some(mp) = vp.v_mount.get() else {
        panic(format_args!("ufs_bmaparray: vnode without a mount"));
    };
    let ump = vfstoufs(mp);

    #[cfg(feature = "diagnostic")]
    if ap.is_some() != nump.is_some() {
        panic(format_args!("ufs_bmaparray: invalid arguments"));
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
        *bnp = blkptrtodb(ump, ip.dip_db(bn as usize));
        if *bnp == 0 {
            *bnp = -1;
        } else if let Some(runp) = runp {
            bn += 1;
            while bn < NDADDR as Daddr
                && *runp < maxrun
                && is_sequential(ump, ip.dip_db(bn as usize - 1), ip.dip_db(bn as usize))
            {
                bn += 1;
                *runp += 1;
            }
        }
        return Ok(());
    }

    // Get disk address out of indirect block array.
    let mut daddr = ip.dip_ib(xap[0].in_off as usize);

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
            panic(format_args!("ufs_bmaparray: indirect block not in cache"));
        } else {
            b.b_blkno.set(blkptrtodb(ump, daddr));
            b.set(B_READ);
            BCSTATS
                .pendingreads
                .fetch_add(1, core::sync::atomic::Ordering::Relaxed);
            BCSTATS
                .numreads
                .fetch_add(1, core::sync::atomic::Ordering::Relaxed);
            if let Some(bvp) = b.b_vp.get() {
                let _ = VOP_STRATEGY(bvp, b);
            }
            if let Some(p) = curproc() {
                p.p_ru.ru_inblock.set(p.p_ru.ru_inblock.get() + 1); // XXX
            }
            if let Err(e) = biowait(b) {
                brelse(b);
                return Err(e);
            }
        }

        // SAFETY: the buffer is ours (busy, from getblk) and mapped, and no other slice of
        // it is alive.
        let data = unsafe { b.data() };
        let off = xap[x].in_off as usize;
        #[cfg(feature = "ffs2")]
        if ip.ump().um_fstype.get() == UM_UFS2 {
            daddr = daddr64_at(data, off);
            if num == 1
                && daddr != 0
                && let Some(runp) = runp.as_deref_mut()
            {
                let mut i = off + 1;
                while (i as u64) < mnindir(ump)
                    && *runp < maxrun
                    && is_sequential(ump, daddr64_at(data, i - 1), daddr64_at(data, i))
                {
                    i += 1;
                    *runp += 1;
                }
            }
            x += 1;
            continue;
        }

        daddr = daddr32_at(data, off);
        if num == 1
            && daddr != 0
            && let Some(runp) = runp.as_deref_mut()
        {
            let mut i = off + 1;
            while (i as u64) < mnindir(ump)
                && *runp < maxrun
                && is_sequential(ump, daddr32_at(data, i - 1), daddr32_at(data, i))
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

    let daddr = blkptrtodb(ump, daddr);
    *bnp = if daddr == 0 { -1 } else { daddr };
    Ok(())
}

/// `ufs_getlbns`: an array of logical block number/offset pairs which represent the path of
/// indirect blocks required to access a data block. The first "pair" contains the logical
/// block number of the appropriate single, double or triple indirect block and the offset
/// into the inode indirect block array. Note, the logical block number of the inode
/// single/double/triple indirect block appears twice in the array, once with the offset into
/// the `i_ffs_ib` and once with the offset into the page itself. `ap` holds at least
/// `NIADDR + 1` entries.
pub fn ufs_getlbns(
    vp: &'static Vnode,
    mut bn: Daddr,
    ap: &mut [Indir],
    mut nump: Option<&mut i32>,
) -> Result<(), Errno> {
    let Some(mp) = vp.v_mount.get() else {
        panic(format_args!("ufs_getlbns: vnode without a mount"));
    };
    let ump = vfstoufs(mp);
    let nindir = mnindir(ump) as i64;
    if let Some(n) = nump.as_deref_mut() {
        *n = 0;
    }
    let mut numlevels = 0;
    let realbn = bn;
    if bn < 0 {
        bn = -bn;
    }

    #[cfg(feature = "diagnostic")]
    if realbn < 0 && realbn > -(NDADDR as Daddr) {
        panic(format_args!(
            "ufs_getlbns: Invalid indirect block {} specified",
            realbn
        ));
    }

    // The first NDADDR blocks are direct blocks.
    if bn < NDADDR as Daddr {
        return Ok(());
    }

    // Determine the number of levels of indirection. After this loop is done, blockcnt
    // indicates the number of data blocks possible at the given level of indirection, and
    // NIADDR - i is the number of levels of indirection needed to locate the requested
    // block.
    let mut blockcnt: i64 = 1;
    let mut i = NIADDR as i64;
    bn -= NDADDR as Daddr;
    loop {
        if i == 0 {
            return Err(Errno::EFBIG);
        }
        blockcnt *= nindir;
        if bn < blockcnt {
            break;
        }
        i -= 1;
        bn -= blockcnt;
    }

    // Calculate the address of the first meta-block.
    let mut metalbn = if realbn >= 0 {
        -(realbn - bn + NIADDR as i64 - i)
    } else {
        -(-realbn - bn + NIADDR as i64 - i)
    };

    // At each iteration, off is the offset into the bap array which is an array of disk
    // addresses at the current level of indirection. The logical block number and the offset
    // in that block are stored into the argument array.
    let mut k = 0;
    ap[k] = Indir {
        in_lbn: metalbn,
        in_off: (NIADDR as i64 - i) as i32,
        in_exists: 0,
    };
    k += 1;
    numlevels += 1;
    while i <= NIADDR as i64 {
        // If searching for a meta-data block, quit when found.
        if metalbn == realbn {
            break;
        }

        blockcnt /= nindir;
        let off = (bn / blockcnt) % nindir;

        numlevels += 1;
        ap[k] = Indir {
            in_lbn: metalbn,
            in_off: off as i32,
            in_exists: 0,
        };
        k += 1;

        metalbn -= -1 + off * blockcnt;
        i += 1;
    }

    #[cfg(feature = "diagnostic")]
    if realbn < 0 && metalbn != realbn {
        panic(format_args!(
            "ufs_getlbns: indirect block {} not found",
            realbn
        ));
    }

    if let Some(n) = nump {
        *n = numlevels;
    }
    Ok(())
}
