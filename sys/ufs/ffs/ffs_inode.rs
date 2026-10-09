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
 *	@(#)ffs_inode.c	8.8 (Berkeley) 10/19/94
 */
/* </LICENSES> */

/* <CODE> */
//! Writing an inode back to its block (`ffs_update`) and truncating a file (`ffs_truncate`,
//! with `ffs_indirtrunc` freeing the blocks under an indirect block).
//!
//! Upstream: sys/ufs/ffs/ffs_inode.c @ 3ce1f3f79392
//!
//! ## Deviations
//! - The dinode copies between the inode and its block go through `inode.rs`'s
//!   `dinode1_at`/`set_dinode1_at` (FFS1) and `dinode2_at`/`set_dinode2_at` (FFS2); the
//!   block pointers in an indirect block are read and written at their index in the bytes
//!   (`BAP`/`BAP_ASSIGN`). `ffs_indirtrunc`'s copy of the block is a heap buffer
//!   (`malloc(M_TEMP)`), as in C.
//! - `curproc->p_ru.ru_inblock++` is skipped when no thread runs, as `vfs_bio.rs` does.
//! - The quota calls are `quota.rs`'s: `ufs_quota.rs`'s with feature `quota` (`option QUOTA`),
//!   the no-quota answers of `ufs_quota_stub.c` without it.

use core::ptr::NonNull;

use crate::kern::kern_malloc::{free, malloc};
use crate::kern::subr_prf::panic;
use crate::kern::vfs_bio::{
    BCSTATS, bawrite, bdwrite, biowait, bread, brelse, buf_adjcnt, bwrite, getblk,
};
use crate::kern::vfs_subr::vinvalbuf;
use crate::kern::vfs_vops::VOP_STRATEGY;
use crate::machine::cpu::curproc;
use crate::sys::buf::{B_CLRBUF, B_DELWRI, B_DONE, B_INVAL, B_READ, B_SYNC};
use crate::sys::errno::Errno;
use crate::sys::malloc::{M_TEMP, M_WAITOK};
use crate::sys::param::btodb;
use crate::sys::systm::INFSLP;
use crate::sys::types::{Daddr, Off};
use crate::sys::ucred::{NOCRED, Ucred};
use crate::sys::vnode::{IO_SYNC, V_SAVE, V_SAVEMETA, VDIR, VLNK, VREG};
use crate::ufs::ffs::ffs_alloc::ffs_blkfree;
use crate::ufs::ffs::fs::{
    FS_44INODEFMT, FS_UFS1_MAGIC, blkoff, blksize, fsbtodb, ino_to_fsba, ino_to_fsbo, lblkno,
    nindir, numfrags,
};
use crate::ufs::ufs::dinode::{NDADDR, NIADDR};
use crate::ufs::ufs::inode::{
    IN_CHANGE, IN_LAZYMOD, IN_MODIFIED, IN_UPDATE, Inode, UFS_BUF_ALLOC, UFS_UPDATE, daddr32_at,
    daddr64_at, doingasync, set_daddr32_at, set_daddr64_at, set_dinode1_at, set_dinode2_at,
};
use crate::ufs::ufs::quota::{getinoquota, ufs_quota_free_blocks};
use crate::ufs::ufs::ufs_vnops::ufs_itimes;
#[cfg(feature = "ffs2")]
use crate::ufs::ufs::ufsmount::UM_UFS2;
use crate::uvm::uvm_vnode::{uvm_vnp_setsize, uvm_vnp_uncache};

/// `ffs_update` (`iv_update`): write the inode back to its block.
///
/// Update the access, modified, and inode change times as specified by the `IN_ACCESS`,
/// `IN_UPDATE`, and `IN_CHANGE` flags respectively. The `IN_MODIFIED` flag is used to specify
/// that the inode needs to be updated but that the times have already been set. The
/// `IN_LAZYMOD` flag is used to specify that the inode needs to be updated at some point, by
/// reclaim if not in the course of other changes; this is used to defer writes just to
/// update device timestamps. If waitfor is set, then wait for the disk write of the inode
/// to complete.
pub fn ffs_update(ip: &Inode, waitfor: i32) -> Result<(), Errno> {
    let vp = ip.itov();
    ufs_itimes(vp);

    if ip.i_flag.get() & IN_MODIFIED == 0 && waitfor == 0 {
        return Ok(());
    }

    ip.clr_flag(IN_MODIFIED | IN_LAZYMOD);
    let fs = ip.fs();

    // Ensure that uid and gid are correct. This is a temporary fix until fsck has been
    // changed to do the update.
    if fs.fs_magic.get() == FS_UFS1_MAGIC && fs.fs_inodefmt.get() < FS_44INODEFMT {
        ip.with_din1(|d| d.di_u = [d.di_uid as u16, d.di_gid as u16]);
    }

    let (bp, error) = bread(
        ip.i_devvp(),
        fsbtodb(fs, ino_to_fsba(fs, ip.i_number.get())),
        fs.fs_bsize.get(),
    );
    if let Err(e) = error {
        brelse(bp);
        return Err(e);
    }

    if ip.i_effnlink.get() != ip.dip_nlink() {
        panic(format_args!("ffs_update: bad link cnt"));
    }

    {
        // SAFETY: the buffer is ours (busy from bread) and mapped; the slice dies before it
        // is written.
        let data = unsafe { bp.data() };
        let idx = ino_to_fsbo(fs, ip.i_number.get());
        #[cfg(feature = "ffs2")]
        let ufs2 = ip.ump().um_fstype.get() == UM_UFS2;
        #[cfg(not(feature = "ffs2"))]
        let ufs2 = false;
        if ufs2 {
            set_dinode2_at(data, idx, &ip.din2());
        } else {
            set_dinode1_at(data, idx, &ip.din1());
        }
    }

    if waitfor != 0 && !doingasync(vp) {
        bwrite(bp)
    } else {
        bdwrite(bp);
        Ok(())
    }
}

/// `SINGLE`: index of single indirect block.
const SINGLE: usize = 0;
/// `DOUBLE`: index of double indirect block.
const DOUBLE: usize = 1;
/// `TRIPLE`: index of triple indirect block.
const TRIPLE: usize = 2;

/// `ffs_truncate` (`iv_truncate`): truncate the inode `oip` to at most `length` size,
/// freeing the disk blocks.
pub fn ffs_truncate(oip: &Inode, length: Off, flags: i32, cred: *const Ucred) -> Result<(), Errno> {
    if length < 0 {
        return Err(Errno::EINVAL);
    }
    let ovp = oip.itov();

    let t = ovp.v_type.get();
    if t != VREG && t != VDIR && t != VLNK {
        return Ok(());
    }

    if oip.dip_size() as Off == length {
        return Ok(());
    }

    if t == VLNK && oip.dip_size() < u64::from(oip.ump().um_maxsymlinklen.get()) {
        #[cfg(feature = "diagnostic")]
        if length != 0 {
            panic(format_args!("ffs_truncate: partial truncate of symlink"));
        }
        let n = oip.dip_size() as usize;
        oip.with_shortlink(|s| s[..n].fill(0));
        oip.dip_set_size(0);
        oip.set_flag(IN_CHANGE | IN_UPDATE);
        return UFS_UPDATE(oip, 1);
    }

    getinoquota(oip)?;

    let fs = oip.fs();
    if length as u64 > fs.fs_maxfilesize.get() {
        return Err(Errno::EFBIG);
    }

    uvm_vnp_setsize(ovp, length);
    let mut ci = oip.i_ci.get();
    ci.ci_lasta = 0;
    ci.ci_clen = 0;
    ci.ci_cstart = 0;
    ci.ci_lastw = 0;
    oip.i_ci.set(ci);

    let osize = oip.dip_size() as Off;
    // Lengthen the size of the file. We must ensure that the last byte of the file is
    // allocated. Since the smallest value of osize is 0, length will be at least 1.
    if osize < length {
        let mut aflags = B_CLRBUF;
        if flags & IO_SYNC != 0 {
            aflags |= B_SYNC;
        }
        let bp = UFS_BUF_ALLOC(oip, length - 1, 1, cred, aflags)?;
        oip.dip_set_size(length as u64);
        uvm_vnp_setsize(ovp, length);
        let _ = uvm_vnp_uncache(ovp);
        if aflags & B_SYNC != 0 {
            let _ = bwrite(bp);
        } else {
            bawrite(bp);
        }
        oip.set_flag(IN_CHANGE | IN_UPDATE);
        return UFS_UPDATE(oip, 1);
    }
    uvm_vnp_setsize(ovp, length);

    // Shorten the size of the file. If the file is not being truncated to a block boundary,
    // the contents of the partial block following the end of the file must be zero'ed in
    // case it ever becomes accessible again because of subsequent file growth. Directories
    // however are not zero'ed as they should grow back initialized to empty.
    let offset = blkoff(fs, length);
    if offset == 0 {
        oip.dip_set_size(length as u64);
    } else {
        let lbn = lblkno(fs, length);
        let mut aflags = B_CLRBUF;
        if flags & IO_SYNC != 0 {
            aflags |= B_SYNC;
        }
        let bp = UFS_BUF_ALLOC(oip, length - 1, 1, cred, aflags)?;
        oip.dip_set_size(length as u64);
        let size = blksize(fs, oip, lbn) as i64;
        let _ = uvm_vnp_uncache(ovp);
        if t != VDIR {
            // SAFETY: the buffer is ours (busy from UFS_BUF_ALLOC) and mapped.
            (unsafe { bp.data() })[offset as usize..size as usize].fill(0);
        }
        buf_adjcnt(bp, size);
        if aflags & B_SYNC != 0 {
            let _ = bwrite(bp);
        } else {
            bawrite(bp);
        }
    }
    // Calculate index into inode's block list of last direct and indirect blocks (if any)
    // which we want to keep. Lastblock is -1 when the file is truncated to 0.
    let lastblock = lblkno(fs, length + Off::from(fs.fs_bsize.get()) - 1) - 1;
    let mut lastiblock = [0 as Daddr; NIADDR];
    lastiblock[SINGLE] = lastblock - NDADDR as Daddr;
    lastiblock[DOUBLE] = lastiblock[SINGLE] - nindir(fs);
    lastiblock[TRIPLE] = lastiblock[DOUBLE] - nindir(fs) * nindir(fs);
    let nblocks = btodb(fs.fs_bsize.get() as usize) as i64;

    // Update file and block pointers on disk before we start freeing blocks. If we crash
    // before free'ing blocks below, the blocks will be returned to the free list. lastiblock
    // values are also normalized to -1 for calls to ffs_indirtrunc below.
    let mut oldblks = [0 as Daddr; NDADDR + NIADDR];
    let mut newblks = [0 as Daddr; NDADDR + NIADDR];
    for level in (SINGLE..=TRIPLE).rev() {
        oldblks[NDADDR + level] = oip.dip_ib(level);
        if lastiblock[level] < 0 {
            oip.dip_set_ib(level, 0);
            lastiblock[level] = -1;
        }
    }

    for (i, old) in oldblks.iter_mut().enumerate().take(NDADDR) {
        *old = oip.dip_db(i);
        if i as Daddr > lastblock {
            oip.dip_set_db(i, 0);
        }
    }

    oip.set_flag(IN_CHANGE | IN_UPDATE);
    // The C keeps this error in allerror, which vinvalbuf's result replaces below.
    let _ = UFS_UPDATE(oip, 1);

    // Having written the new inode to disk, save its new configuration and put back the old
    // block pointers long enough to process them. Note that we save the new block
    // configuration so we can check it when we are done.
    for i in 0..NDADDR {
        newblks[i] = oip.dip_db(i);
        oip.dip_set_db(i, oldblks[i]);
    }

    for i in 0..NIADDR {
        newblks[NDADDR + i] = oip.dip_ib(i);
        oip.dip_set_ib(i, oldblks[NDADDR + i]);
    }

    oip.dip_set_size(osize as u64);
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
            let bn = oip.dip_ib(level);
            if bn != 0 {
                let mut count = 0;
                if let Err(e) = ffs_indirtrunc(
                    oip,
                    indir_lbn[level],
                    fsbtodb(fs, bn),
                    lastiblock[level],
                    level,
                    &mut count,
                ) {
                    allerror = Err(e);
                }
                blocksreleased += count;
                if lastiblock[level] < 0 {
                    oip.dip_set_ib(level, 0);
                    ffs_blkfree(oip, bn, i64::from(fs.fs_bsize.get()));
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
            let bn = oip.dip_db(i as usize);
            if bn != 0 {
                oip.dip_set_db(i as usize, 0);
                let bsize = blksize(fs, oip, i) as i64;
                ffs_blkfree(oip, bn, bsize);
                blocksreleased += btodb(bsize as usize) as i64;
            }
            i -= 1;
        }
        if lastblock < 0 {
            break 'done;
        }

        // Finally, look for a change in size of the last direct block; release any frags.
        let mut bn = oip.dip_db(lastblock as usize);
        if bn != 0 {
            // Calculate amount of space we're giving back as old block size minus new block
            // size.
            let oldspace = blksize(fs, oip, lastblock) as i64;
            oip.dip_set_size(length as u64);
            let newspace = blksize(fs, oip, lastblock) as i64;
            if newspace == 0 {
                panic(format_args!("ffs_truncate: newspace"));
            }
            if oldspace - newspace > 0 {
                // Block number of space to be free'd is the old block # plus the number of
                // frags required for the storage we're keeping.
                bn += numfrags(fs, newspace);
                ffs_blkfree(oip, bn, oldspace - newspace);
                blocksreleased += btodb((oldspace - newspace) as usize) as i64;
            }
        }
    }
    // done:
    #[cfg(feature = "diagnostic")]
    {
        for level in SINGLE..=TRIPLE {
            if newblks[NDADDR + level] != oip.dip_ib(level) {
                panic(format_args!("ffs_truncate1"));
            }
        }
        for (i, &blk) in newblks[..NDADDR].iter().enumerate() {
            if blk != oip.dip_db(i) {
                panic(format_args!("ffs_truncate2"));
            }
        }
    }
    #[cfg(not(feature = "diagnostic"))]
    let _ = newblks;
    // Put back the real size.
    oip.dip_set_size(length as u64);
    if oip.dip_blocks() >= blocksreleased {
        oip.dip_set_blocks(oip.dip_blocks() - blocksreleased);
    } else {
        // sanity
        oip.dip_set_blocks(0);
    }
    oip.set_flag(IN_CHANGE);
    let _ = ufs_quota_free_blocks(oip, blocksreleased, NOCRED);
    allerror
}

/// `BAP(ip, i)`: block pointer `i` of an indirect block's bytes.
fn bap(ip: &Inode, b: &[u8], i: usize) -> Daddr {
    #[cfg(feature = "ffs2")]
    if ip.ump().um_fstype.get() == UM_UFS2 {
        return daddr64_at(b, i);
    }
    let _ = ip;
    daddr32_at(b, i)
}

/// `BAP_ASSIGN(ip, i, value)`.
fn bap_assign(ip: &Inode, b: &mut [u8], i: usize, value: Daddr) {
    #[cfg(feature = "ffs2")]
    if ip.ump().um_fstype.get() == UM_UFS2 {
        return set_daddr64_at(b, i, value);
    }
    let _ = ip;
    set_daddr32_at(b, i, value)
}

/// `ffs_indirtrunc`: release blocks associated with the inode `ip` and stored in the
/// indirect block `bn`. Blocks are free'd in LIFO order up to (but not including) `lastbn`.
/// If level is greater than `SINGLE`, the block is an indirect block and recursive calls to
/// indirtrunc must be used to cleanse other indirect blocks. `*countp` is the number of
/// `DEV_BSIZE` blocks released.
///
/// NB: triple indirect blocks are untested.
pub fn ffs_indirtrunc(
    ip: &Inode,
    lbn: Daddr,
    dbn: Daddr,
    lastbn: Daddr,
    level: usize,
    countp: &mut i64,
) -> Result<(), Errno> {
    let fs = ip.fs();
    let bsize = fs.fs_bsize.get();
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
        BCSTATS
            .pendingreads
            .fetch_add(1, core::sync::atomic::Ordering::Relaxed);
        BCSTATS
            .numreads
            .fetch_add(1, core::sync::atomic::Ordering::Relaxed);
        bp.set(B_READ);
        if bp.b_bcount.get() > bp.b_bufsize.get() {
            panic(format_args!("ffs_indirtrunc: bad buffer size"));
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
    if lastbn != -1 {
        let Some(c) = malloc(bsize as usize, M_TEMP, M_WAITOK) else {
            panic(format_args!("ffs_indirtrunc: no memory"));
        };
        {
            // SAFETY: the buffer is ours (busy) and mapped; `c` holds `bsize` bytes.
            let data = unsafe { bp.data() };
            // SAFETY: `c` is a fresh allocation of `bsize` bytes, disjoint from the buffer.
            let cs = unsafe { core::slice::from_raw_parts_mut(c.as_ptr(), bsize as usize) };
            cs.copy_from_slice(&data[..bsize as usize]);

            for i in (last + 1)..nindir(fs) {
                bap_assign(ip, data, i as usize, 0);
            }
        }
        copy = Some(c);

        if !doingasync(vp) {
            if let Err(e) = bwrite(bp) {
                allerror = Err(e);
            }
        } else {
            bawrite(bp);
        }
    }
    let baps: &[u8] = match copy {
        // SAFETY: the copy holds `bsize` bytes and lives until freed below.
        Some(c) => unsafe { core::slice::from_raw_parts(c.as_ptr(), bsize as usize) },
        // SAFETY: the buffer is still ours (busy) and mapped; it is released below.
        None => unsafe { bp.data() },
    };

    // Recursively free totally unused blocks.
    let mut i = nindir(fs) - 1;
    let mut nlbn = lbn + 1 - i * factor;
    while i > last {
        let nb = bap(ip, baps, i as usize);
        if nb != 0 {
            if level > SINGLE {
                let mut blkcount = 0;
                if let Err(e) =
                    ffs_indirtrunc(ip, nlbn, fsbtodb(fs, nb), -1, level - 1, &mut blkcount)
                {
                    allerror = Err(e);
                }
                blocksreleased += blkcount;
            }
            ffs_blkfree(ip, nb, i64::from(bsize));
            blocksreleased += nblocks;
        }
        i -= 1;
        nlbn += factor;
    }

    // Recursively free last partial block.
    if level > SINGLE && lastbn >= 0 {
        let last = lastbn % factor;
        let nb = bap(ip, baps, i as usize);
        if nb != 0 {
            let mut blkcount = 0;
            if let Err(e) =
                ffs_indirtrunc(ip, nlbn, fsbtodb(fs, nb), last, level - 1, &mut blkcount)
            {
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
