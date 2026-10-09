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
 *	@(#)ffs_inode.c	8.8 (Berkeley) 10/19/94
 * Modified for ext2fs by Manuel Bouyer.
 */
/* </LICENSES> */

/* <CODE> */
//! ext2fs inodes: the size of a file (`ext2fs_size`, `ext2fs_setsize`, with the high 32
//! bits of a regular file's size), the last reference (`ext2fs_inactive`), writing an inode
//! back to its block (`ext2fs_update`) and truncating a file (`ext2fs_truncate`, with
//! `ext2fs_indirtrunc` freeing the blocks under an indirect block).
//!
//! Upstream: sys/ufs/ext2fs/ext2fs_inode.c @ 3ce1f3f79392
//!
//! ## Deviations
//! - The dinode goes to its block through `e2fs_isave` over the buffer's bytes at the
//!   inode's offset; the block pointers in an indirect block are read and written at their
//!   index in the bytes, little-endian (`letoh32`). `ext2fs_indirtrunc`'s copy of the block
//!   is a heap buffer (`malloc(M_TEMP)`), as in C.
//! - `curproc->p_ru.ru_inblock++` is skipped when no thread runs, as `vfs_bio.rs` does.
//! - `ext2fs_truncate`'s `allerror` starts as success: the C leaves it unset when
//!   `ext2fs_update` succeeds, and `vinvalbuf`'s result replaces it anyway.
//! - The block counts are kept in `i64` (`blocksreleased`, `count`), the C's `long`.

use core::ptr::NonNull;
use core::sync::atomic::Ordering;

use crate::kern::kern_malloc::{free, malloc};
use crate::kern::kern_tc::getnanotime;
use crate::kern::subr_prf::panic;
use crate::kern::vfs_bio::{BCSTATS, bawrite, bdwrite, biowait, bread, brelse, bwrite, getblk};
use crate::kern::vfs_subr::{vinvalbuf, vrecycle};
use crate::kern::vfs_vops::{VOP_STRATEGY, VOP_UNLOCK};
use crate::machine::cpu::curproc;
use crate::sys::buf::{B_CLRBUF, B_DELWRI, B_DONE, B_INVAL, B_READ, B_SYNC};
use crate::sys::errno::Errno;
use crate::sys::malloc::{M_TEMP, M_WAITOK};
use crate::sys::mount::MNT_RDONLY;
use crate::sys::param::btodb;
use crate::sys::systm::INFSLP;
use crate::sys::types::{Daddr, Off};
use crate::sys::ucred::{NOCRED, Ucred};
use crate::sys::vnode::{IO_SYNC, V_SAVE, V_SAVEMETA, VDIR, VLNK, VREG, VopInactiveArgs};
use crate::ufs::ext2fs::ext2fs::{
    E2FS_REV0, EXT2F_ROCOMPAT_LARGE_FILE, blkoff, fsbtodb, ino_to_fsba, ino_to_fsbo, lblkno, nindir,
};
use crate::ufs::ext2fs::ext2fs_alloc::{ext2fs_blkfree, ext2fs_inode_free};
use crate::ufs::ext2fs::ext2fs_balloc::ext2fs_buf_alloc;
use crate::ufs::ext2fs::ext2fs_dinode::{EXT2_MAXSYMLINKLEN, NDADDR, NIADDR, e2fs_isave};
use crate::ufs::ufs::dinode::{IFMT, IFREG};
use crate::ufs::ufs::inode::{IN_ACCESS, IN_CHANGE, IN_MODIFIED, IN_UPDATE, Inode, vtoi};
use crate::uvm::uvm_vnode::{uvm_vnp_setsize, uvm_vnp_uncache};

/// `SINGLE`: index of single indirect block.
const SINGLE: usize = 0;
/// `DOUBLE`: index of double indirect block.
const DOUBLE: usize = 1;
/// `TRIPLE`: index of triple indirect block.
const TRIPLE: usize = 2;

/// `ext2fs_size`: get the size of an inode (the high 32 bits count for a regular file).
pub fn ext2fs_size(ip: &Inode) -> u64 {
    let mut size = u64::from(ip.i_e2fs_size());

    if u32::from(ip.i_e2fs_mode()) & IFMT == IFREG {
        size |= u64::from(ip.i_e2fs_size_hi()) << 32;
    }

    size
}

/// `ext2fs_setsize`: set the size of an inode. `EFBIG` past the file system's maximum (and
/// then the file system is marked as holding large files, as Linux does).
pub fn ext2fs_setsize(ip: &Inode, size: u64) -> Result<(), Errno> {
    let fs = ip.e2fs();

    if size <= fs.e2fs_maxfilesize.get() as u64 {
        // If HUGE_FILEs are off, e2fs_maxfilesize will protect us.
        if u32::from(ip.i_e2fs_mode()) & IFMT == IFREG || ip.i_e2fs_mode() == 0 {
            ip.set_i_e2fs_size_hi((size >> 32) as u32);
        }

        ip.set_i_e2fs_size(size as u32);
        return Ok(());
    }

    // Linux automagically upgrades to REV1 here! (`<= E2FS_REV0` in C; 0 is the least.)
    if fs.e2fs_rev() == E2FS_REV0 {
        return Err(Errno::EFBIG);
    }

    if fs.e2fs_features_rocompat() & EXT2F_ROCOMPAT_LARGE_FILE == 0 {
        fs.set_e2fs_features_rocompat(fs.e2fs_features_rocompat() | EXT2F_ROCOMPAT_LARGE_FILE);
        fs.e2fs_fmod.set(1);
    }
    Err(Errno::EFBIG)
}

/// `ext2fs_inactive` (`vop_inactive`): last reference to an inode. If necessary, write or
/// delete it.
pub fn ext2fs_inactive(ap: &mut VopInactiveArgs<'_>) -> Result<(), Errno> {
    let vp = ap.a_vp;
    let ip = vtoi(vp);
    let mut error = Ok(());

    #[cfg(feature = "diagnostic")]
    if crate::kern::vfs_subr::PRTACTIVE.load(Ordering::Relaxed) != 0 && vp.v_usecount.get() != 0 {
        crate::kern::vfs_subr::vprint(Some("ext2fs_inactive: pushing active"), vp);
    }

    // Get rid of inodes related to stale file handles.
    if !(ip.din_is_null() || ip.i_e2fs_mode() == 0 || ip.i_e2fs_dtime() != 0) {
        let rdonly = vp
            .v_mount
            .get()
            .is_some_and(|mp| mp.mnt_flag.get() & MNT_RDONLY != 0);
        if ip.i_e2fs_nlink() == 0 && !rdonly {
            if ext2fs_size(ip) != 0 {
                error = ext2fs_truncate(ip, 0, 0, NOCRED);
            }
            let ts = getnanotime();
            ip.set_i_e2fs_dtime(ts.tv_sec as u32);
            ip.set_flag(IN_CHANGE | IN_UPDATE);
            ext2fs_inode_free(ip, ip.i_number.get(), ip.i_e2fs_mode().into());
        }
        if ip.i_flag.get() & (IN_ACCESS | IN_CHANGE | IN_MODIFIED | IN_UPDATE) != 0 {
            let _ = ext2fs_update(ip, 0);
        }
    }
    // out:
    let _ = VOP_UNLOCK(vp);
    // If we are done with the inode, reclaim it so that it can be reused immediately.
    if ip.din_is_null() || ip.i_e2fs_dtime() != 0 {
        vrecycle(vp, ap.a_p);
    }
    error
}

/// `ext2fs_update`: update the access, modified, and inode change times as specified by
/// the `IN_ACCESS`, `IN_UPDATE`, and `IN_CHANGE` flags respectively, and write the inode
/// back to its block if it was modified. The `IN_MODIFIED` flag is used to specify that the
/// inode needs to be updated but that the times have already been set. If `waitfor` is
/// set, then wait for the disk write of the inode to complete.
pub fn ext2fs_update(ip: &Inode, waitfor: i32) -> Result<(), Errno> {
    if ip
        .itov()
        .v_mount
        .get()
        .is_some_and(|mp| mp.mnt_flag.get() & MNT_RDONLY != 0)
    {
        return Ok(());
    }
    ip.ext2fs_itimes();
    if ip.i_flag.get() & IN_MODIFIED == 0 {
        return Ok(());
    }
    ip.clr_flag(IN_MODIFIED);
    let fs = ip.e2fs();
    let (bp, error) = bread(
        ip.i_devvp(),
        fsbtodb(fs, Daddr::from(ino_to_fsba(fs, ip.i_number.get()))),
        fs.e2fs_bsize.get(),
    );
    if let Err(e) = error {
        brelse(bp);
        return Err(e);
    }
    ip.clr_flag(IN_MODIFIED);

    // See note about 16-bit UID/GID limitation in ext2fs_vget(). Now that we are about to
    // write the inode, construct the split UID and GID fields out of the two 32-bit fields
    // we kept in memory.
    let uid = ip.i_e2fs_uid().get();
    let gid = ip.i_e2fs_gid().get();
    ip.set_i_e2fs_uid_low(uid as u16);
    ip.set_i_e2fs_gid_low(gid as u16);
    ip.set_i_e2fs_uid_high((uid >> 16) as u16);
    ip.set_i_e2fs_gid_high((gid >> 16) as u16);

    {
        // SAFETY: the buffer is ours (busy from bread) and mapped; the slice dies before it
        // is written.
        let data = unsafe { bp.data() };
        let off = ino_to_fsbo(fs, ip.i_number.get()) as usize * fs.dinode_size();
        ip.with_e2din(|d| e2fs_isave(fs, d, &mut data[off..]));
    }
    if waitfor != 0 {
        bwrite(bp)
    } else {
        bdwrite(bp);
        Ok(())
    }
}

/// `ext2fs_truncate`: truncate the inode `oip` to at most `length` size, freeing the disk
/// blocks.
pub fn ext2fs_truncate(
    oip: &Inode,
    length: Off,
    flags: i32,
    cred: *const Ucred,
) -> Result<(), Errno> {
    let ovp = oip.itov();

    if length < 0 {
        return Err(Errno::EINVAL);
    }

    let t = ovp.v_type.get();
    if t != VREG && t != VDIR && t != VLNK {
        return Ok(());
    }

    if t == VLNK && ext2fs_size(oip) < EXT2_MAXSYMLINKLEN as u64 {
        #[cfg(feature = "diagnostic")]
        if length != 0 {
            panic(format_args!("ext2fs_truncate: partial truncate of symlink"));
        }
        let n = ext2fs_size(oip) as usize;
        oip.with_e2din(|d| d.e2di_shortlink_mut()[..n].fill(0));
        let _ = ext2fs_setsize(oip, 0);
        oip.set_flag(IN_CHANGE | IN_UPDATE);
        return ext2fs_update(oip, 1);
    }

    if ext2fs_size(oip) == length as u64 {
        oip.set_flag(IN_CHANGE | IN_UPDATE);
        return ext2fs_update(oip, 0);
    }
    let fs = oip.e2fs();
    let bsize = fs.e2fs_bsize.get();
    let osize = ext2fs_size(oip) as Off;
    // Lengthen the size of the file. We must ensure that the last byte of the file is
    // allocated. Since the smallest value of osize is 0, length will be at least 1.
    if osize < length {
        let offset = blkoff(fs, length - 1);
        let lbn = lblkno(fs, length - 1);
        let mut aflags = B_CLRBUF;
        if flags & IO_SYNC != 0 {
            aflags |= B_SYNC;
        }
        let bp = ext2fs_buf_alloc(oip, lbn as u32, offset as i32 + 1, cred, aflags)?;
        let _ = ext2fs_setsize(oip, length as u64);
        uvm_vnp_setsize(ovp, length);
        let _ = uvm_vnp_uncache(ovp);
        if aflags & B_SYNC != 0 {
            let _ = bwrite(bp);
        } else {
            bawrite(bp);
        }
        oip.set_flag(IN_CHANGE | IN_UPDATE);
        return ext2fs_update(oip, 1);
    }
    // Shorten the size of the file. If the file is not being truncated to a block
    // boundary, the contents of the partial block following the end of the file must be
    // zero'ed in case it ever become accessible again because of subsequent file growth.
    let offset = blkoff(fs, length);
    if offset == 0 {
        let _ = ext2fs_setsize(oip, length as u64);
    } else {
        let lbn = lblkno(fs, length);
        let mut aflags = B_CLRBUF;
        if flags & IO_SYNC != 0 {
            aflags |= B_SYNC;
        }
        let bp = ext2fs_buf_alloc(oip, lbn as u32, offset as i32, cred, aflags)?;
        let _ = ext2fs_setsize(oip, length as u64);
        let size = bsize as usize;
        uvm_vnp_setsize(ovp, length);
        let _ = uvm_vnp_uncache(ovp);
        // SAFETY: the buffer is ours (busy from ext2fs_buf_alloc) and mapped.
        (unsafe { bp.data() })[offset as usize..size].fill(0);
        bp.b_bcount.set(size as i64);
        if aflags & B_SYNC != 0 {
            let _ = bwrite(bp);
        } else {
            bawrite(bp);
        }
    }
    // Calculate index into inode's block list of last direct and indirect blocks (if any)
    // which we want to keep. Lastblock is -1 when the file is truncated to 0.
    let (lastblock, mut lastiblock) =
        ext2fs_truncate_lastblocks(lblkno(fs, length + Off::from(bsize) - 1) - 1, nindir(fs));
    let nblocks = btodb(bsize as usize) as i64;
    // Update file and block pointers on disk before we start freeing blocks. If we crash
    // before free'ing blocks below, the blocks will be returned to the free list.
    // lastiblock values are also normalized to -1 for calls to ext2fs_indirtrunc below.
    let oldblks = oip.i_e2fs_blocks();
    for level in (SINGLE..=TRIPLE).rev() {
        if lastiblock[level] < 0 {
            oip.set_i_e2fs_block(NDADDR + level, 0);
            lastiblock[level] = -1;
        }
    }
    let mut i = NDADDR as Daddr - 1;
    while i > lastblock {
        oip.set_i_e2fs_block(i as usize, 0);
        i -= 1;
    }
    oip.set_flag(IN_CHANGE | IN_UPDATE);
    // The C keeps this error in allerror, which vinvalbuf's result replaces below.
    let _ = ext2fs_update(oip, 1);

    // Having written the new inode to disk, save its new configuration and put back the old
    // block pointers long enough to process them. Note that we save the new block
    // configuration so we can check it when we are done.
    let newblks = oip.i_e2fs_blocks();
    oip.with_e2din(|d| d.e2di_blocks = oldblks);
    let _ = ext2fs_setsize(oip, osize as u64);
    let vflags = (if length > 0 { V_SAVE } else { 0 }) | V_SAVEMETA;
    let mut allerror = vinvalbuf(ovp, vflags, cred, curproc(), 0, INFSLP);

    let mut blocksreleased: i64 = 0;
    'done: {
        // Indirect blocks first.
        let mut indir_lbn = [0 as Daddr; NIADDR];
        indir_lbn[SINGLE] = -(NDADDR as Daddr);
        indir_lbn[DOUBLE] = indir_lbn[SINGLE] - nindir(fs) - 1;
        indir_lbn[TRIPLE] = indir_lbn[DOUBLE] - nindir(fs) * nindir(fs) - 1;
        for level in (SINGLE..=TRIPLE).rev() {
            let bn = u32::from_le(oip.i_e2fs_block(NDADDR + level));
            if bn != 0 {
                let mut count = 0;
                if let Err(e) = ext2fs_indirtrunc(
                    oip,
                    indir_lbn[level],
                    fsbtodb(fs, Daddr::from(bn)),
                    lastiblock[level],
                    level,
                    &mut count,
                ) {
                    allerror = Err(e);
                }
                blocksreleased += count;
                if lastiblock[level] < 0 {
                    oip.set_i_e2fs_block(NDADDR + level, 0);
                    ext2fs_blkfree(oip, bn);
                    blocksreleased += nblocks;
                }
            }
            if lastiblock[level] >= 0 {
                break 'done;
            }
        }

        // All whole direct blocks or frags.
        let mut i = NDADDR as Daddr - 1;
        while i > lastblock {
            let bn = u32::from_le(oip.i_e2fs_block(i as usize));
            if bn != 0 {
                oip.set_i_e2fs_block(i as usize, 0);
                ext2fs_blkfree(oip, bn);
                blocksreleased += btodb(bsize as usize) as i64;
            }
            i -= 1;
        }
    }
    // done:
    #[cfg(feature = "diagnostic")]
    {
        for level in SINGLE..=TRIPLE {
            if newblks[NDADDR + level] != oip.i_e2fs_block(NDADDR + level) {
                panic(format_args!("ext2fs_truncate1"));
            }
        }
        for (i, &blk) in newblks[..NDADDR].iter().enumerate() {
            if blk != oip.i_e2fs_block(i) {
                panic(format_args!("ext2fs_truncate2"));
            }
        }
        if length == 0 {
            let s = crate::machine::intr::splbio();
            if !ovp.v_cleanblkhd.is_empty() || !ovp.v_dirtyblkhd.is_empty() {
                panic(format_args!("ext2fs_truncate3"));
            }
            crate::machine::intr::splx(s);
        }
    }
    #[cfg(not(feature = "diagnostic"))]
    let _ = newblks;
    // Put back the real size.
    let _ = ext2fs_setsize(oip, length as u64);
    oip.set_i_e2fs_nblock(ext2fs_truncate_nblock(oip.i_e2fs_nblock(), blocksreleased));
    oip.set_flag(IN_CHANGE);
    allerror
}

/// The last direct block to keep (-1 when the file is truncated to 0) and, per level of
/// indirection, the last block to keep below the indirect block (negative: none), from
/// `lastblock = lblkno(fs, length + bsize - 1) - 1` and `NINDIR(fs)`, as `ext2fs_truncate`
/// computes them in `int32_t`.
fn ext2fs_truncate_lastblocks(lastblock: Daddr, nindir: i64) -> (Daddr, [Daddr; NIADDR]) {
    let lastblock = Daddr::from(lastblock as i32);
    let mut lastiblock = [0 as Daddr; NIADDR];
    lastiblock[SINGLE] = Daddr::from((lastblock - NDADDR as Daddr) as i32);
    lastiblock[DOUBLE] = Daddr::from((lastiblock[SINGLE] - nindir) as i32);
    lastiblock[TRIPLE] = Daddr::from((lastiblock[DOUBLE] - nindir * nindir) as i32);
    (lastblock, lastiblock)
}

/// The inode's block count once `blocksreleased` `DEV_BSIZE` blocks are released (none left
/// when more are released than it counts).
fn ext2fs_truncate_nblock(nblock: u32, blocksreleased: i64) -> u32 {
    if blocksreleased >= i64::from(nblock) {
        0
    } else {
        nblock - blocksreleased as u32
    }
}

/// `ext2fs_indirtrunc`: release blocks associated with the inode `ip` and stored in the
/// indirect block `lbn` (at disk block `dbn`). Blocks are free'd in LIFO order up to (but
/// not including) `lastbn`. If level is greater than `SINGLE`, the block is an indirect
/// block and recursive calls to indirtrunc must be used to cleanse other indirect blocks.
/// `*countp` is the number of `DEV_BSIZE` blocks released.
///
/// NB: triple indirect blocks are untested.
fn ext2fs_indirtrunc(
    ip: &Inode,
    lbn: Daddr,
    dbn: Daddr,
    lastbn: Daddr,
    level: usize,
    countp: &mut i64,
) -> Result<(), Errno> {
    let fs = ip.e2fs();
    let bsize = fs.e2fs_bsize.get();
    let mut blocksreleased: i64 = 0;
    let mut allerror = Ok(());

    // Calculate index in current block of last block to be kept. -1 indicates the entire
    // block so we need not calculate the index.
    let mut factor: i64 = 1;
    for _ in SINGLE..level {
        factor *= nindir(fs);
    }
    let mut last = lastbn;
    if lastbn > 0 {
        last /= factor;
    }
    let nblocks = btodb(bsize as usize) as i64;
    // Get buffer of block pointers, zero those entries corresponding to blocks to be
    // free'd, and update on disk copy first. Since double(triple) indirect before
    // single(double) indirect, calls to bmap on these blocks will fail. However, we already
    // have the on disk address, so we have to set the b_blkno field explicitly instead of
    // letting bread do everything for us.
    let vp = ip.itov();
    let bp = loop {
        if let Some(bp) = getblk(vp, lbn, bsize, 0, INFSLP) {
            break bp;
        }
    };
    let mut error = Ok(());
    if !bp.isset(B_DONE | B_DELWRI) {
        if let Some(p) = curproc() {
            p.p_ru.ru_inblock.set(p.p_ru.ru_inblock.get() + 1); // pay for read
        }
        BCSTATS.pendingreads.fetch_add(1, Ordering::Relaxed);
        BCSTATS.numreads.fetch_add(1, Ordering::Relaxed);
        bp.set(B_READ);
        if bp.b_bcount.get() > bp.b_bufsize.get() {
            panic(format_args!("ext2fs_indirtrunc: bad buffer size"));
        }
        bp.b_blkno.set(dbn);
        if let Some(bvp) = bp.b_vp.get() {
            let _ = VOP_STRATEGY(bvp, bp);
        }
        error = biowait(bp);
    }
    if let Err(e) = error {
        brelse(bp);
        *countp = 0;
        return Err(e);
    }

    // The block pointers: the buffer's, or a copy of them once the buffer is written.
    let mut copy: Option<NonNull<u8>> = None;
    if lastbn >= 0 {
        let Some(c) = malloc(bsize as usize, M_TEMP, M_WAITOK) else {
            panic(format_args!("ext2fs_indirtrunc: no memory"));
        };
        {
            // SAFETY: the buffer is ours (busy) and mapped.
            let data = unsafe { bp.data() };
            // SAFETY: `c` is a fresh allocation of `bsize` bytes, disjoint from the buffer.
            let cs = unsafe { core::slice::from_raw_parts_mut(c.as_ptr(), bsize as usize) };
            cs.copy_from_slice(&data[..bsize as usize]);
            data[(last + 1) as usize * 4..nindir(fs) as usize * 4].fill(0);
        }
        copy = Some(c);
        if let Err(e) = bwrite(bp) {
            allerror = Err(e);
        }
    }
    let baps: &[u8] = match copy {
        // SAFETY: the copy holds `bsize` bytes and lives until freed below.
        Some(c) => unsafe { core::slice::from_raw_parts(c.as_ptr(), bsize as usize) },
        // SAFETY: the buffer is still ours (busy) and mapped; it is released below.
        None => unsafe { bp.data() },
    };
    let bap = |i: i64| {
        let o = i as usize * 4;
        u32::from_le_bytes([baps[o], baps[o + 1], baps[o + 2], baps[o + 3]])
    };

    // Recursively free totally unused blocks.
    let mut i = nindir(fs) - 1;
    let mut nlbn = lbn + 1 - i * factor;
    while i > last {
        let nb = bap(i);
        if nb != 0 {
            if level > SINGLE {
                let mut blkcount = 0;
                if let Err(e) = ext2fs_indirtrunc(
                    ip,
                    nlbn,
                    fsbtodb(fs, Daddr::from(nb)),
                    -1,
                    level - 1,
                    &mut blkcount,
                ) {
                    allerror = Err(e);
                }
                blocksreleased += blkcount;
            }
            ext2fs_blkfree(ip, nb);
            blocksreleased += nblocks;
        }
        i -= 1;
        nlbn += factor;
    }

    // Recursively free last partial block.
    if level > SINGLE && lastbn >= 0 {
        let last = lastbn % factor;
        let nb = bap(i);
        if nb != 0 {
            let mut blkcount = 0;
            if let Err(e) = ext2fs_indirtrunc(
                ip,
                nlbn,
                fsbtodb(fs, Daddr::from(nb)),
                last,
                level - 1,
                &mut blkcount,
            ) {
                allerror = Err(e);
            }
            blocksreleased += blkcount;
        }
    }

    match copy {
        Some(c) => free(c, M_TEMP, bsize as usize),
        None => {
            bp.set(B_INVAL);
            brelse(bp);
        }
    }

    *countp = blocksreleased;
    allerror
}
/* </CODE> */

/* <TESTS> */
#[cfg(test)]
mod tests {
    // Host tests for the size of an inode (the high 32 bits of a regular file's size, the
    // file system's limit) and the block arithmetic of `ext2fs_truncate`. Truncating through
    // the buffer cache runs in the read-write mount test of `ext2fs_vfsops`.

    use std::assert_eq;
    use std::boxed::Box;

    use super::*;
    use crate::ufs::ext2fs::ext2fs::{E2FS_REV1, MExt2fs};
    use crate::ufs::ext2fs::ext2fs_dinode::Ext2fsDinode;
    use crate::ufs::ufs::dinode::IFDIR;

    /// An inode of `mode` with its own dinode, on a file system of revision `rev` whose files
    /// may grow to `maxfilesize`.
    fn inode(mode: u32, rev: u32, maxfilesize: Off) -> &'static Inode {
        let fs: &'static MExt2fs = Box::leak(Box::new(MExt2fs::new()));
        fs.set_e2fs_rev(rev);
        fs.e2fs_maxfilesize.set(maxfilesize);
        let din: &'static mut Ext2fsDinode = Box::leak(Box::new(Ext2fsDinode::default()));
        din.e2di_mode = mode as u16;
        let ip: &'static Inode = Box::leak(Box::new(Inode::new()));
        ip.i_e2fs.set(Some(fs));
        ip.dinode_u.set(core::ptr::from_mut(din).cast());
        ip
    }

    #[test]
    fn the_high_size_word_counts_for_regular_files_only() {
        let ip = inode(IFREG | 0o644, E2FS_REV1, 1 << 40);
        ext2fs_setsize(ip, (5 << 32) | 7).unwrap();
        assert_eq!((ip.i_e2fs_size(), ip.i_e2fs_size_hi()), (7, 5));
        assert_eq!(ext2fs_size(ip), (5 << 32) | 7);

        let dir = inode(IFDIR | 0o755, E2FS_REV1, 1 << 40);
        dir.set_i_e2fs_size_hi(3);
        ext2fs_setsize(dir, 1024).unwrap();
        assert_eq!((ext2fs_size(dir), dir.i_e2fs_size_hi()), (1024, 3));
    }

    #[test]
    fn growing_past_the_limit_is_efbig_and_asks_for_large_files() {
        let ip = inode(IFREG, E2FS_REV0, 1000);
        assert_eq!(ext2fs_setsize(ip, 1001), Err(Errno::EFBIG));
        assert_eq!(ip.e2fs().e2fs_features_rocompat(), 0);

        let ip = inode(IFREG, E2FS_REV1, 1000);
        assert_eq!(ext2fs_setsize(ip, 1001), Err(Errno::EFBIG));
        assert_eq!(
            ip.e2fs().e2fs_features_rocompat(),
            EXT2F_ROCOMPAT_LARGE_FILE
        );
        assert_eq!(ip.e2fs().e2fs_fmod.get(), 1);
        assert_eq!(ext2fs_size(ip), 0);
    }

    #[test]
    fn truncate_keeps_the_blocks_below_the_new_end() {
        // To nothing: no direct block, no block under any indirect one.
        assert_eq!(
            ext2fs_truncate_lastblocks(-1, 256),
            (-1, [-13, -13 - 256, -13 - 256 - 65536])
        );
        // One byte into logical block 13: blocks 0..=13, so two under the single indirect.
        assert_eq!(
            ext2fs_truncate_lastblocks(13, 256),
            (13, [1, 1 - 256, 1 - 256 - 65536])
        );
        // Into the double indirect range.
        let (_, l) = ext2fs_truncate_lastblocks(12 + 256 + 300, 256);
        assert_eq!((l[SINGLE], l[DOUBLE]), (556, 300));

        assert_eq!(ext2fs_truncate_nblock(6, 4), 2);
        assert_eq!(ext2fs_truncate_nblock(6, 6), 0);
        assert_eq!(ext2fs_truncate_nblock(6, 10), 0);
    }
}
/* </TESTS> */
