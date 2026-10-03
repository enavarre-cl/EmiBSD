/*	$OpenBSD: ffs_subr.c,v 1.35 2024/10/08 02:58:26 jsg Exp $	*/
/*	$NetBSD: ffs_subr.c,v 1.6 1996/03/17 02:16:23 christos Exp $	*/
/* <LICENSES> */
/*
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
 */
/* </LICENSES> */

//! Fast file system helpers: the directory block of an offset (`ffs_bufatoff`), the
//! fragment and block map operations the allocator and `fsck` share (`ffs_fragacct`,
//! `ffs_isblock`, `ffs_clrblock`, `ffs_setblock`, `ffs_isfreeblock`), and the vnode set-up
//! of a new inode (`ffs_vinit`).
//!
//! Upstream: sys/ufs/ffs/ffs_subr.c @ 3ce1f3f79392
//!
//! ## Deviations
//! - The userland prototypes (`!_KERNEL`) are left out.
//! - The maps are byte slices (`u_char *cp` in C); `ffs_fragacct`'s `fraglist` is the
//!   cylinder group's `cg_frsum` counters (`&[Cell<i32>]`).
//! - `ffs_bufatoff` returns the buffer and the offset of `*res` in its data.
//! - `ffs_vinit` takes the vnode and returns the one to use (an alias may replace it). A
//!   fifo is refused with `EOPNOTSUPP`, the C's answer without `option FIFO`: `fifofs` is
//!   not ported (`ffs_fifovops`).

use core::cell::Cell;
use core::ptr;

use crate::kern::kern_tc::getmicrouptime;
use crate::kern::spec_vnops::SPEC_VOPS;
use crate::kern::subr_prf::panic;
use crate::kern::vfs_bio::{bread, brelse, buf_adjcnt};
use crate::kern::vfs_subr::{checkalias, vgone, vrele};
use crate::sys::buf::Buf;
use crate::sys::errno::Errno;
use crate::sys::mount::Mount;
use crate::sys::types::{Daddr, Dev, Off};
use crate::sys::vnode::{
    VBAD, VBLK, VCHR, VDIR, VFIFO, VLNK, VNON, VREG, VROOT, VSOCK, Vnode, iftovt,
};
use crate::ufs::ffs::ffs_vnops::FFS_SPECVOPS;
use crate::ufs::ffs::fs::{AROUND, FRAGTBL, Fs, INSIDE, blkoff, blksize, lblkno};
use crate::ufs::ufs::dinode::ROOTINO;
use crate::ufs::ufs::inode::{Inode, vtoi};

/// `ffs_bufatoff`: the buffer with the contents of block `offset` from the beginning of
/// directory `ip`, and the offset in it of the remaining space in the directory (`*res`).
pub fn ffs_bufatoff(ip: &Inode, offset: Off) -> Result<(&'static Buf, usize), Errno> {
    let vp = ip.itov();
    let fs = ip.fs();
    let lbn = lblkno(fs, offset);
    let bsize = blksize(fs, ip, lbn);

    let (bp, error) = bread(vp, lbn, fs.fs_bsize.get());
    if let Err(e) = error {
        brelse(bp);
        return Err(e);
    }
    buf_adjcnt(bp, bsize as i64);
    Ok((bp, blkoff(fs, offset) as usize))
}

/// `ffs_fragacct`: update the frsum fields to reflect addition or deletion of some frags.
pub fn ffs_fragacct(fs: &Fs, fragmap: i32, fraglist: &[Cell<i32>], cnt: i32) {
    let frag = fs.fs_frag.get();
    let Some(tbl) = FRAGTBL[frag as usize] else {
        panic(format_args!("ffs_fragacct: fs_frag {}", frag));
    };
    let inblk = i32::from(tbl[fragmap as usize]) << 1;
    let fragmap = fragmap << 1;
    for siz in 1..frag {
        if inblk & (1 << (siz + (frag % 8))) == 0 {
            continue;
        }
        let mut field = AROUND[siz as usize];
        let mut subfield = INSIDE[siz as usize];
        let mut pos = siz;
        while pos <= frag {
            if fragmap & field == subfield {
                let c = &fraglist[siz as usize];
                c.set(c.get() + cnt);
                pos += siz;
                field <<= siz;
                subfield <<= siz;
            }
            field <<= 1;
            subfield <<= 1;
            pos += 1;
        }
    }
}

/// `ffs_isblock`: block operations: check if a block is available.
pub fn ffs_isblock(fs: &Fs, cp: &[u8], h: Daddr) -> bool {
    let h = h as usize;
    match fs.fs_frag.get() {
        4 => {
            let mask = 0x0f << ((h & 0x1) << 2);
            cp[h >> 1] & mask == mask
        }
        2 => {
            let mask = 0x03 << ((h & 0x3) << 1);
            cp[h >> 2] & mask == mask
        }
        1 => {
            let mask = 0x01 << (h & 0x7);
            cp[h >> 3] & mask == mask
        }
        _ => cp[h] == 0xff,
    }
}

/// `ffs_clrblock`: take a block out of the map.
pub fn ffs_clrblock(fs: &Fs, cp: &mut [u8], h: Daddr) {
    let h = h as usize;
    match fs.fs_frag.get() {
        4 => cp[h >> 1] &= !(0x0f << ((h & 0x1) << 2)),
        2 => cp[h >> 2] &= !(0x03 << ((h & 0x3) << 1)),
        1 => cp[h >> 3] &= !(0x01 << (h & 0x7)),
        _ => cp[h] = 0,
    }
}

/// `ffs_setblock`: put a block into the map.
pub fn ffs_setblock(fs: &Fs, cp: &mut [u8], h: Daddr) {
    let h = h as usize;
    match fs.fs_frag.get() {
        4 => cp[h >> 1] |= 0x0f << ((h & 0x1) << 2),
        2 => cp[h >> 2] |= 0x03 << ((h & 0x3) << 1),
        1 => cp[h >> 3] |= 0x01 << (h & 0x7),
        _ => cp[h] = 0xff,
    }
}

/// `ffs_isfreeblock`: check if a block is free.
pub fn ffs_isfreeblock(fs: &Fs, cp: &[u8], h: Daddr) -> bool {
    let h = h as usize;
    match fs.fs_frag.get() {
        4 => cp[h >> 1] & (0x0f << ((h & 0x1) << 2)) == 0,
        2 => cp[h >> 2] & (0x03 << ((h & 0x3) << 1)) == 0,
        1 => cp[h >> 3] & (0x01 << (h & 0x7)) == 0,
        _ => cp[h] == 0,
    }
}

/// `ffs_vinit`: initialize the vnode associated with a new inode, handle aliased vnodes.
/// Returns the vnode to use, which replaces `vp` when a device vnode had an alias.
pub fn ffs_vinit(mntp: &'static Mount, vp: &'static Vnode) -> Result<&'static Vnode, Errno> {
    let mut vp = vp;
    let ip = vtoi(vp);
    vp.v_type.set(iftovt(ip.dip_mode()));
    match vp.v_type.get() {
        VCHR | VBLK => {
            vp.v_op.set(Some(&FFS_SPECVOPS));
            if let Some(nvp) = checkalias(vp, ip.dip_rdev() as Dev, Some(mntp)) {
                // Discard unneeded vnode, but save its inode. Note that the lock is carried
                // over in the inode to the replacement vnode.
                nvp.v_data.set(vp.v_data.get());
                vp.v_data.set(ptr::null_mut());
                vp.v_op.set(Some(&SPEC_VOPS));
                vrele(vp);
                vgone(vp);
                // Reinitialize aliased inode.
                vp = nvp;
                ip.i_vnode.set(Some(vp));
            }
        }
        // FIFO: vp->v_op = &ffs_fifovops (miscfs/fifofs, not ported); without FIFO:
        VFIFO => return Err(Errno::EOPNOTSUPP),
        VNON | VBAD | VSOCK | VLNK | VDIR | VREG => {}
    }
    if ip.i_number.get() == ROOTINO {
        vp.v_flag.set(vp.v_flag.get() | VROOT);
    }
    // Initialize modrev times.
    let mtv = getmicrouptime();
    let modrev = ((mtv.tv_sec as u64) << 32) | (mtv.tv_usec as u64).wrapping_mul(4294);
    ip.i_modrev.set(modrev);
    Ok(vp)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_block_maps_agree_with_each_other() {
        // A fake super-block with only fs_frag set: the map operations read nothing else.
        #[repr(C, align(8))]
        struct Raw([u8; size_of::<Fs>()]);
        let mut raw = Raw([0u8; size_of::<Fs>()]);
        for frag in [1, 2, 4, 8] {
            raw.0[56..60].copy_from_slice(&(frag as i32).to_ne_bytes());
            // SAFETY: `Fs` is `Cell`s of integers and raw pointers, valid for any bytes, and
            // `raw` has its size and alignment; nothing writes `raw` while `fs` lives.
            let fs = unsafe { &*raw.0.as_ptr().cast::<Fs>() };
            let mut map = [0u8; 4];
            for h in 0..(32 / frag) as i64 {
                assert!(ffs_isfreeblock(fs, &map, h));
                ffs_setblock(fs, &mut map, h);
                assert!(ffs_isblock(fs, &map, h));
                assert!(!ffs_isfreeblock(fs, &map, h));
                ffs_clrblock(fs, &mut map, h);
                assert!(ffs_isfreeblock(fs, &map, h));
            }
        }
    }
}
