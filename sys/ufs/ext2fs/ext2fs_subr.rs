/*	$OpenBSD: ext2fs_subr.c,v 1.38 2024/10/08 02:58:26 jsg Exp $	*/
/*	$NetBSD: ext2fs_subr.c,v 1.1 1997/06/11 09:34:03 bouyer Exp $	*/
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
 *	@(#)ffs_subr.c	8.2 (Berkeley) 9/21/93
 * Modified for ext2fs by Manuel Bouyer.
 */
/* </LICENSES> */

//! ext2fs helpers: the directory block of an offset (`ext2fs_bufatoff`, through the extent
//! tree when the inode has one) and the vnode set-up of a new inode (`ext2fs_vinit`).
//!
//! Upstream: sys/ufs/ext2fs/ext2fs_subr.c @ 3ce1f3f79392
//!
//! ## Deviations
//! - `ext2fs_bufatoff` returns the buffer and the offset of `*res` in its data, as
//!   `ffs_bufatoff` does (the C's optional `res` is always computed).
//! - When the inode has an extent tree but no extent covers the block, the C goes on to the
//!   normal path without releasing the leaf buffer `ext4_ext_find_extent` may hold; here it
//!   is released first.
//! - `ext2fs_vinit` takes the vnode and returns the one to use (an alias may replace it). A
//!   fifo is refused with `EOPNOTSUPP`, the C's answer without `option FIFO`: `fifofs` is
//!   not ported (`ext2fs_fifovops`).

use core::ptr;

use crate::kern::kern_tc::getmicrouptime;
use crate::kern::spec_vnops::SPEC_VOPS;
use crate::kern::vfs_bio::{bread, brelse};
use crate::kern::vfs_subr::{checkalias, vgone, vrele};
use crate::sys::buf::Buf;
use crate::sys::errno::Errno;
use crate::sys::mount::Mount;
use crate::sys::types::{Daddr, Dev, Off};
use crate::sys::vnode::{
    VBAD, VBLK, VCHR, VDIR, VFIFO, VLNK, VNON, VREG, VROOT, VSOCK, Vnode, iftovt,
};
use crate::ufs::ext2fs::ext2fs::{blkoff, fsbtodb, lblkno};
use crate::ufs::ext2fs::ext2fs_dinode::{EXT2_ROOTINO, EXT4_EXTENTS};
use crate::ufs::ext2fs::ext2fs_extents::{Ext4ExtentPath, ext4_ext_find_extent};
use crate::ufs::ext2fs::ext2fs_vnops::EXT2FS_SPECVOPS;
use crate::ufs::ufs::inode::{Inode, vtoi};

/// `ext2fs_bufatoff`: the buffer with the contents of block `offset` from the beginning of
/// directory `ip`, and the offset in it of the remaining space in the directory (`*res`).
pub fn ext2fs_bufatoff(ip: &Inode, offset: Off) -> Result<(&'static Buf, usize), Errno> {
    let vp = ip.itov();
    let fs = ip.e2fs();
    let lbn = lblkno(fs, offset);

    if ip.i_e2fs_flags() & EXT4_EXTENTS != 0 {
        let mut path = Ext4ExtentPath::new();
        let ep = ext4_ext_find_extent(fs, ip, lbn, &mut path).and_then(|p| p.ext());
        if let Some(bp) = path.ep_bp.take() {
            brelse(bp);
        }

        if let Some(ep) = ep {
            let pos = lbn - Daddr::from(ep.e_blk)
                + ((Daddr::from(ep.e_start_hi) << 32) | Daddr::from(ep.e_start_lo));
            let (bp, error) = bread(ip.i_devvp(), fsbtodb(fs, pos), fs.e2fs_bsize.get());
            if let Err(e) = error {
                brelse(bp);
                return Err(e);
            }
            return Ok((bp, blkoff(fs, offset) as usize));
        }
    }

    // normal:
    let (bp, error) = bread(vp, lbn, fs.e2fs_bsize.get());
    if let Err(e) = error {
        brelse(bp);
        return Err(e);
    }
    Ok((bp, blkoff(fs, offset) as usize))
}

/// `ext2fs_vinit`: initialize the vnode associated with a new inode, handle aliased vnodes.
/// Returns the vnode to use, which replaces `vp` when a device vnode had an alias.
pub fn ext2fs_vinit(mp: &'static Mount, vp: &'static Vnode) -> Result<&'static Vnode, Errno> {
    let mut vp = vp;
    let ip = vtoi(vp);
    vp.v_type.set(iftovt(ip.i_e2fs_mode().into()));

    match vp.v_type.get() {
        VCHR | VBLK => {
            vp.v_op.set(Some(&EXT2FS_SPECVOPS));

            let rdev = u32::from_le(ip.with_e2din(|d| d.e2di_rdev())) as Dev;
            if let Some(nvp) = checkalias(vp, rdev, Some(mp)) {
                // Discard unneeded vnode, but save its inode. Note that the lock is carried
                // over in the inode to the replacement vnode.
                nvp.v_data.set(vp.v_data.get());
                vp.v_data.set(ptr::null_mut());
                vp.v_op.set(Some(&SPEC_VOPS));
                vrele(vp);
                vgone(vp);
                // Reinitialize aliased vnode.
                vp = nvp;
                ip.i_vnode.set(Some(vp));
            }
        }
        // FIFO: vp->v_op = &ext2fs_fifovops (miscfs/fifofs, not ported); without FIFO:
        VFIFO => return Err(Errno::EOPNOTSUPP),
        VNON | VBAD | VSOCK | VLNK | VDIR | VREG => {}
    }

    if ip.i_number.get() == EXT2_ROOTINO {
        vp.v_flag.set(vp.v_flag.get() | VROOT);
    }

    // Initialize modrev times
    let tv = getmicrouptime();
    let modrev = ((tv.tv_sec as u64) << 32) | (tv.tv_usec as u64).wrapping_mul(4294);
    ip.i_modrev.set(modrev);

    Ok(vp)
}
