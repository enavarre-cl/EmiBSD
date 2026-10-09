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
 * Copyright (c) 2002 Networks Associates Technology, Inc.
 * All rights reserved.
 *
 * This software was developed for the FreeBSD Project by Marshall
 * Kirk McKusick and Network Associates Laboratories, the Security
 * Research Division of Network Associates, Inc. under DARPA/SPAWAR
 * contract N66001-01-C-8035 ("CBOSS"), as part of the DARPA CHATS
 * research program.
 *
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
 */
/* </LICENSES> */

/* <CODE> */
//! Block allocation: `balloc` defines the structure of file system storage by allocating
//! the physical blocks on a device given the inode and the logical block number in a file,
//! growing the last fragment into a block and building the indirect blocks on the way.
//!
//! Upstream: sys/ufs/ffs/ffs_balloc.c @ 3ce1f3f79392
//!
//! ## Deviations
//! - `bpp` is the C's out-parameter as `Option<&mut Option<&'static Buf>>` (NULL when the
//!   caller wants no buffer). A buffer `ffs_realloccg` handed back and that is written at
//!   once is cleared from `*bpp`, where the C leaves a stale pointer that every later path
//!   overwrites.
//! - `allocib` (a pointer into the dinode's `di_ib`) is the index it points at; `allociblk`
//!   is an array and a count; the block pointers in an indirect block are read and written at
//!   their index in the buffer's bytes (`inode.rs`).
//! - `VOP_FSYNC(vp, p->p_ucred, MNT_WAIT, p)` of the failure path is skipped when no thread
//!   runs (there is no `p` to pass).
//! - The quota calls are `quota.rs`'s: `ufs_quota.rs`'s with feature `quota` (`option QUOTA`),
//!   the no-quota answers of `ufs_quota_stub.c` without it.

use crate::kern::subr_prf::panic;
use crate::kern::vfs_bio::{bawrite, bdwrite, bread, brelse, buf_adjcnt, bwrite, getblk};
use crate::kern::vfs_vops::VOP_FSYNC;
use crate::machine::cpu::curproc;
use crate::sys::buf::{B_CLRBUF, B_DELWRI, B_INVAL, B_SYNC, Buf, clrbuf};
use crate::sys::errno::Errno;
use crate::sys::mount::MNT_WAIT;
use crate::sys::param::btodb;
use crate::sys::systm::INFSLP;
use crate::sys::types::{Daddr, Off};
use crate::sys::ucred::Ucred;
use crate::sys::vnode::Vnode;
#[cfg(feature = "ffs2")]
use crate::ufs::ffs::ffs_alloc::ffs2_blkpref;
use crate::ufs::ffs::ffs_alloc::{ffs_alloc, ffs_blkfree, ffs_realloccg, ffs1_blkpref};
#[cfg(feature = "ffs2")]
use crate::ufs::ffs::fs::{FS_UFS2_MAGIC, cgtod, dbtofsb, dtog};
use crate::ufs::ffs::fs::{blkoff, blksize, fragroundup, fsbtodb, lblkno, lblktosize};
use crate::ufs::ufs::dinode::{NDADDR, NIADDR};
use crate::ufs::ufs::inode::{IN_CHANGE, IN_UPDATE, Indir, Inode, daddr32_at, set_daddr32_at};
#[cfg(feature = "ffs2")]
use crate::ufs::ufs::inode::{daddr64_at, set_daddr64_at};
use crate::ufs::ufs::quota::ufs_quota_free_blocks;
use crate::ufs::ufs::ufs_bmap::ufs_getlbns;
use crate::uvm::uvm_vnode::uvm_vnp_setsize;

/// `getblk(vp, blkno, size, 0, INFSLP)`, which cannot fail without `slpflag`.
fn getblk_wait(vp: &'static Vnode, blkno: Daddr, size: i32) -> &'static Buf {
    loop {
        if let Some(bp) = getblk(vp, blkno, size, 0, INFSLP) {
            return bp;
        }
    }
}

/// The data of a buffer the caller owns.
///
/// # Safety
///
/// As `Buf::data`: the buffer is busy for the caller and mapped, and no other slice of it
/// is alive.
#[allow(clippy::mut_from_ref)] // the B_BUSY owner's view, as the C's b_data
unsafe fn bdata(bp: &Buf) -> &mut [u8] {
    // SAFETY: the caller's contract.
    unsafe { bp.data() }
}

/// `VOP_FSYNC(vp, p->p_ucred, MNT_WAIT, p)` with `curproc`.
fn fsync_wait(vp: &'static Vnode) {
    if let Some(p) = curproc() {
        let _ = VOP_FSYNC(vp, p.p_ucred.get(), MNT_WAIT, p);
    }
}

/// Puts the buffer `ffs_realloccg` handed back on its way to the disk and clears `*bpp`.
fn write_realloc_buf(bpp: &mut Option<&mut Option<&'static Buf>>, flags: i32) {
    if let Some(b) = bpp.as_deref_mut()
        && let Some(bp) = b.take()
    {
        if flags & B_SYNC != 0 {
            let _ = bwrite(bp);
        } else {
            bawrite(bp);
        }
    }
}

/// `ffs1_balloc`: `ffs_balloc` for an FFS1 inode.
pub fn ffs1_balloc(
    ip: &Inode,
    startoffset: Off,
    size: i32,
    cred: *const Ucred,
    flags: i32,
    mut bpp: Option<&mut Option<&'static Buf>>,
) -> Result<(), Errno> {
    let vp = ip.itov();
    let fs = ip.fs();
    let bsize = fs.fs_bsize.get();
    let lbn = lblkno(fs, startoffset);
    let size = blkoff(fs, startoffset) as i32 + size;
    if size > bsize {
        panic(format_args!("ffs1_balloc: blk too big"));
    }
    if let Some(b) = bpp.as_deref_mut() {
        *b = None;
    }
    if lbn < 0 {
        return Err(Errno::EFBIG);
    }
    let db = |i: usize| ip.dip_db(i);

    // If the next write will extend the file into a new block, and the file is currently
    // composed of a fragment this fragment has to be extended to be a full block.
    let nb = lblkno(fs, ip.dip_size() as Off);
    if nb < NDADDR as Daddr && nb < lbn {
        let osize = blksize(fs, ip, nb) as i32;
        if osize < bsize && osize > 0 {
            let mut newb = 0;
            ffs_realloccg(
                ip,
                nb,
                ffs1_blkpref(ip, nb, nb as i32, Some(&db)),
                osize,
                bsize,
                cred,
                bpp.as_deref_mut(),
                Some(&mut newb),
            )?;

            ip.dip_set_size(lblktosize(fs, nb + 1) as u64);
            uvm_vnp_setsize(vp, ip.dip_size() as i64);
            ip.dip_set_db(nb as usize, newb);
            ip.set_flag(IN_CHANGE | IN_UPDATE);
            write_realloc_buf(&mut bpp, flags);
        }
    }
    // The first NDADDR blocks are direct blocks
    if lbn < NDADDR as Daddr {
        let nb = ip.dip_db(lbn as usize);
        let newb;
        if nb != 0 && ip.dip_size() as Off >= lblktosize(fs, lbn + 1) {
            // The block is an already-allocated direct block and the file already extends
            // past this block, thus this must be a whole block. Just read the block (if
            // requested).
            if let Some(b) = bpp {
                let (bp, error) = bread(vp, lbn, bsize);
                if let Err(e) = error {
                    brelse(bp);
                    return Err(e);
                }
                *b = Some(bp);
            }
            return Ok(());
        }
        if nb != 0 {
            // Consider need to reallocate a fragment.
            let osize = fragroundup(fs, blkoff(fs, ip.dip_size() as Off)) as i32;
            let nsize = fragroundup(fs, i64::from(size)) as i32;
            if nsize <= osize {
                // The existing block is already at least as big as we want. Just read the
                // block (if requested).
                if let Some(b) = bpp {
                    let (bp, error) = bread(vp, lbn, bsize);
                    if let Err(e) = error {
                        brelse(bp);
                        return Err(e);
                    }
                    buf_adjcnt(bp, i64::from(osize));
                    *b = Some(bp);
                }
                return Ok(());
            } else {
                // The existing block is smaller than we want, grow it.
                let mut nb2 = 0;
                ffs_realloccg(
                    ip,
                    lbn,
                    ffs1_blkpref(ip, lbn, lbn as i32, Some(&db)),
                    osize,
                    nsize,
                    cred,
                    bpp,
                    Some(&mut nb2),
                )?;
                newb = nb2;
            }
        } else {
            // The block was not previously allocated, allocate a new block or fragment.
            let nsize = if (ip.dip_size() as Off) < lblktosize(fs, lbn + 1) {
                fragroundup(fs, i64::from(size)) as i32
            } else {
                bsize
            };
            let mut nb2 = 0;
            ffs_alloc(
                ip,
                lbn,
                ffs1_blkpref(ip, lbn, lbn as i32, Some(&db)),
                nsize,
                cred,
                &mut nb2,
            )?;
            newb = nb2;
            if let Some(b) = bpp {
                let bp = getblk_wait(vp, lbn, bsize);
                if nsize < bsize {
                    bp.b_bcount.set(i64::from(nsize));
                }
                bp.b_blkno.set(fsbtodb(fs, newb));
                if flags & B_CLRBUF != 0 {
                    // SAFETY: the buffer is ours (busy from getblk) and mapped.
                    unsafe { clrbuf(bp) };
                }
                *b = Some(bp);
            }
        }
        ip.dip_set_db(lbn as usize, newb);
        ip.set_flag(IN_CHANGE | IN_UPDATE);
        return Ok(());
    }

    // Determine the number of levels of indirection.
    let mut pref: Daddr = 0;
    let mut indirs = [Indir::default(); NIADDR + 2];
    let mut num = 0;
    ufs_getlbns(vp, lbn, &mut indirs, Some(&mut num))?;
    #[cfg(feature = "diagnostic")]
    if num < 1 {
        panic(format_args!(
            "ffs1_balloc: ufs_bmaparray returned indirect block"
        ));
    }
    // Fetch the first indirect block allocating if necessary.
    num -= 1;
    let mut nb = ip.dip_ib(indirs[0].in_off as usize);

    let mut allocib: Option<usize> = None;
    let mut allociblk = [0 as Daddr; NIADDR + 1];
    let mut nalloc = 0;
    let mut unwindidx: i32 = -1;

    let error: Errno = 'fail: {
        if nb == 0 {
            pref = ffs1_blkpref(ip, lbn, -indirs[0].in_off - 1, None);
            let mut newb = 0;
            if let Err(e) = ffs_alloc(ip, lbn, pref, bsize, cred, &mut newb) {
                break 'fail e;
            }
            nb = newb;

            allociblk[nalloc] = nb;
            nalloc += 1;
            let bp = getblk_wait(vp, indirs[1].in_lbn, bsize);
            bp.b_blkno.set(fsbtodb(fs, nb));
            // SAFETY: the buffer is ours (busy from getblk) and mapped.
            unsafe { clrbuf(bp) };

            // Write synchronously so that indirect blocks never point at garbage.
            if let Err(e) = bwrite(bp) {
                break 'fail e;
            }
            allocib = Some(indirs[0].in_off as usize);
            ip.dip_set_ib(indirs[0].in_off as usize, nb);
            ip.set_flag(IN_CHANGE | IN_UPDATE);
        }

        // Fetch through the indirect blocks, allocating as necessary.
        let mut i = 1usize;
        let bp = loop {
            let (bp, error) = bread(vp, indirs[i].in_lbn, bsize);
            if let Err(e) = error {
                brelse(bp);
                break 'fail e;
            }
            // SAFETY: the buffer is ours (busy from bread) and mapped.
            nb = daddr32_at(unsafe { bdata(bp) }, indirs[i].in_off as usize);
            if i as i32 == num {
                break bp;
            }
            i += 1;
            if nb != 0 {
                brelse(bp);
                continue;
            }
            if pref == 0 {
                pref = ffs1_blkpref(ip, lbn, i as i32 - num - 1, None);
            }
            let mut newb = 0;
            if let Err(e) = ffs_alloc(ip, lbn, pref, bsize, cred, &mut newb) {
                brelse(bp);
                break 'fail e;
            }
            nb = newb;
            allociblk[nalloc] = nb;
            nalloc += 1;
            let nbp = getblk_wait(vp, indirs[i].in_lbn, bsize);
            nbp.b_blkno.set(fsbtodb(fs, nb));
            // SAFETY: the buffer is ours (busy from getblk) and mapped.
            unsafe { clrbuf(nbp) };

            // Write synchronously so that indirect blocks never point at garbage.
            if let Err(e) = bwrite(nbp) {
                brelse(bp);
                break 'fail e;
            }
            // SAFETY: the buffer is ours (busy from bread) and mapped.
            set_daddr32_at(unsafe { bdata(bp) }, indirs[i - 1].in_off as usize, nb);
            if allocib.is_none() && unwindidx < 0 {
                unwindidx = i as i32 - 1;
            }
            // If required, write synchronously, otherwise use delayed write.
            if flags & B_SYNC != 0 {
                let _ = bwrite(bp);
            } else {
                bdwrite(bp);
            }
        };
        // Get the data block, allocating if necessary.
        if nb == 0 {
            {
                // SAFETY: the buffer is ours (busy from bread) and mapped; the slice dies
                // before the buffer is written.
                let data = unsafe { bdata(bp) };
                let bap = |k: usize| daddr32_at(data, k);
                pref = ffs1_blkpref(ip, lbn, indirs[i].in_off, Some(&bap));
            }
            let mut newb = 0;
            if let Err(e) = ffs_alloc(ip, lbn, pref, bsize, cred, &mut newb) {
                brelse(bp);
                break 'fail e;
            }
            nb = newb;
            // *allocblk++ = nb: nothing reads it on the way out.
            if let Some(b) = bpp {
                let nbp = getblk_wait(vp, lbn, bsize);
                nbp.b_blkno.set(fsbtodb(fs, nb));
                if flags & B_CLRBUF != 0 {
                    // SAFETY: the buffer is ours (busy from getblk) and mapped.
                    unsafe { clrbuf(nbp) };
                }
                *b = Some(nbp);
            }
            // SAFETY: the buffer is ours (busy from bread) and mapped.
            set_daddr32_at(unsafe { bdata(bp) }, indirs[i].in_off as usize, nb);
            // If required, write synchronously, otherwise use delayed write.
            if flags & B_SYNC != 0 {
                let _ = bwrite(bp);
            } else {
                bdwrite(bp);
            }
            return Ok(());
        }
        brelse(bp);
        if let Some(b) = bpp {
            let nbp = if flags & B_CLRBUF != 0 {
                let (nbp, error) = bread(vp, lbn, bsize);
                if let Err(e) = error {
                    brelse(nbp);
                    break 'fail e;
                }
                nbp
            } else {
                let nbp = getblk_wait(vp, lbn, bsize);
                nbp.b_blkno.set(fsbtodb(fs, nb));
                nbp
            };
            *b = Some(nbp);
        }
        return Ok(());
    };

    // fail:
    // If we have failed to allocate any blocks, simply return the error. This is the usual
    // case and avoids the need to fsync the file.
    if nalloc == 0 && allocib.is_none() && unwindidx == -1 {
        return Err(error);
    }
    // If we have failed part way through block allocation, we have to deallocate any
    // indirect blocks that we have allocated. We have to fsync the file before we start to
    // get rid of all of its dependencies so that we do not leave them dangling. We have to
    // sync it at the end so that the softdep code does not find any untracked changes.
    // Although this is really slow, running out of disk space is not expected to be a
    // common occurrence. The error return from fsync is ignored as we already have an error
    // to return to the user.
    fsync_wait(vp);
    let mut deallocated: i64 = 0;
    for &blk in &allociblk[..nalloc] {
        ffs_blkfree(ip, blk, i64::from(bsize));
        deallocated += i64::from(bsize);
    }
    if let Some(k) = allocib {
        ip.dip_set_ib(k, 0);
    } else if unwindidx >= 0 {
        let (bp, r) = bread(vp, indirs[unwindidx as usize].in_lbn, bsize);
        if let Err(r) = r {
            panic(format_args!(
                "Could not unwind indirect block, error {}",
                r as i32
            ));
        }
        // SAFETY: the buffer is ours (busy from bread) and mapped.
        set_daddr32_at(
            unsafe { bdata(bp) },
            indirs[unwindidx as usize].in_off as usize,
            0,
        );
        if flags & B_SYNC != 0 {
            let _ = bwrite(bp);
        } else {
            bdwrite(bp);
        }
    }
    if deallocated != 0 {
        // Restore user's disk quota because allocation failed.
        let _ = ufs_quota_free_blocks(ip, btodb(deallocated as usize) as Daddr, cred);

        ip.dip_set_blocks(ip.dip_blocks() - btodb(deallocated as usize) as i64);
        ip.set_flag(IN_CHANGE | IN_UPDATE);
    }
    fsync_wait(vp);
    Err(error)
}

/// `ffs2_balloc`: `ffs_balloc` for an FFS2 inode.
#[cfg(feature = "ffs2")]
pub fn ffs2_balloc(
    ip: &Inode,
    off: Off,
    size: i32,
    cred: *const Ucred,
    flags: i32,
    mut bpp: Option<&mut Option<&'static Buf>>,
) -> Result<(), Errno> {
    let vp = ip.itov();
    let fs = ip.fs();
    let bsize = fs.fs_bsize.get();
    let mut unwindidx: i32 = -1;

    let lbn = lblkno(fs, off);
    let size = blkoff(fs, off) as i32 + size;

    if size > bsize {
        panic(format_args!("ffs2_balloc: block too big"));
    }

    if let Some(b) = bpp.as_deref_mut() {
        *b = None;
    }

    if lbn < 0 {
        return Err(Errno::EFBIG);
    }
    let db = |i: usize| ip.dip_db(i);

    // If the next write will extend the file into a new block, and the file is currently
    // composed of a fragment, this fragment has to be extended to be a full block.
    let lastlbn = lblkno(fs, ip.dip_size() as Off);
    if lastlbn < NDADDR as Daddr && lastlbn < lbn {
        let nb = lastlbn;
        let osize = blksize(fs, ip, nb) as i32;
        if osize < bsize && osize > 0 {
            let mut newb = 0;
            ffs_realloccg(
                ip,
                nb,
                ffs2_blkpref(ip, lastlbn, nb as i32, Some(&db)),
                osize,
                bsize,
                cred,
                bpp.as_deref_mut(),
                Some(&mut newb),
            )?;

            ip.dip_set_size(lblktosize(fs, nb + 1) as u64);
            uvm_vnp_setsize(vp, ip.dip_size() as i64);
            ip.dip_set_db(nb as usize, newb);
            ip.set_flag(IN_CHANGE | IN_UPDATE);

            write_realloc_buf(&mut bpp, flags);
        }
    }

    // The first NDADDR blocks are direct.
    if lbn < NDADDR as Daddr {
        let nb = ip.dip_db(lbn as usize);
        let newb;

        if nb != 0 && ip.dip_size() as Off >= lblktosize(fs, lbn + 1) {
            // The direct block is already allocated and the file extends past this block,
            // thus this must be a whole block. Just read it, if requested.
            if let Some(b) = bpp {
                let (bp, error) = bread(vp, lbn, bsize);
                if let Err(e) = error {
                    brelse(bp);
                    return Err(e);
                }
                *b = Some(bp);
            }

            return Ok(());
        }

        if nb != 0 {
            // Consider the need to allocate a fragment.
            let osize = fragroundup(fs, blkoff(fs, ip.dip_size() as Off)) as i32;
            let nsize = fragroundup(fs, i64::from(size)) as i32;

            if nsize <= osize {
                // The existing block is already at least as big as we want. Just read it,
                // if requested.
                if let Some(b) = bpp {
                    let (bp, error) = bread(vp, lbn, bsize);
                    if let Err(e) = error {
                        brelse(bp);
                        return Err(e);
                    }
                    buf_adjcnt(bp, i64::from(osize));
                    *b = Some(bp);
                }

                return Ok(());
            } else {
                // The existing block is smaller than we want, grow it.
                let mut nb2 = 0;
                ffs_realloccg(
                    ip,
                    lbn,
                    ffs2_blkpref(ip, lbn, lbn as i32, Some(&db)),
                    osize,
                    nsize,
                    cred,
                    bpp,
                    Some(&mut nb2),
                )?;
                newb = nb2;
            }
        } else {
            // The block was not previously allocated, allocate a new block or fragment.
            let nsize = if (ip.dip_size() as Off) < lblktosize(fs, lbn + 1) {
                fragroundup(fs, i64::from(size)) as i32
            } else {
                bsize
            };

            let mut nb2 = 0;
            ffs_alloc(
                ip,
                lbn,
                ffs2_blkpref(ip, lbn, lbn as i32, Some(&db)),
                nsize,
                cred,
                &mut nb2,
            )?;
            newb = nb2;

            if let Some(b) = bpp {
                let bp = getblk_wait(vp, lbn, bsize);
                if nsize < bsize {
                    bp.b_bcount.set(i64::from(nsize));
                }
                bp.b_blkno.set(fsbtodb(fs, newb));
                if flags & B_CLRBUF != 0 {
                    // SAFETY: the buffer is ours (busy from getblk) and mapped.
                    unsafe { clrbuf(bp) };
                }
                *b = Some(bp);
            }
        }

        ip.dip_set_db(lbn as usize, newb);
        ip.set_flag(IN_CHANGE | IN_UPDATE);

        return Ok(());
    }

    // Determine the number of levels of indirection.
    let mut pref: Daddr = 0;
    let mut indirs = [Indir::default(); NIADDR + 2];
    let mut num = 0;
    ufs_getlbns(vp, lbn, &mut indirs, Some(&mut num))?;

    #[cfg(feature = "diagnostic")]
    if num < 1 {
        panic(format_args!(
            "ffs2_balloc: ufs_bmaparray returned indirect block"
        ));
    }

    // Fetch the first indirect block allocating it necessary.
    num -= 1;
    let mut nb = ip.dip_ib(indirs[0].in_off as usize);
    let mut allocib: Option<usize> = None;
    let mut allociblk = [0 as Daddr; NIADDR + 1];
    let mut nalloc = 0;

    let error: Errno = 'fail: {
        if nb == 0 {
            pref = ffs2_blkpref(ip, lbn, -indirs[0].in_off - 1, None);
            let mut newb = 0;
            if let Err(e) = ffs_alloc(ip, lbn, pref, bsize, cred, &mut newb) {
                break 'fail e;
            }

            nb = newb;
            allociblk[nalloc] = nb;
            nalloc += 1;
            let bp = getblk_wait(vp, indirs[1].in_lbn, bsize);
            bp.b_blkno.set(fsbtodb(fs, nb));
            // SAFETY: the buffer is ours (busy from getblk) and mapped.
            unsafe { clrbuf(bp) };

            // Write synchronously so that indirect blocks never point at garbage.
            if let Err(e) = bwrite(bp) {
                break 'fail e;
            }

            unwindidx = 0;
            allocib = Some(indirs[0].in_off as usize);
            ip.dip_set_ib(indirs[0].in_off as usize, nb);
            ip.set_flag(IN_CHANGE | IN_UPDATE);
        }

        // Fetch through the indirect blocks, allocating as necessary.
        let mut i = 1usize;
        let bp = loop {
            let (bp, error) = bread(vp, indirs[i].in_lbn, bsize);
            if let Err(e) = error {
                brelse(bp);
                break 'fail e;
            }

            // SAFETY: the buffer is ours (busy from bread) and mapped.
            nb = daddr64_at(unsafe { bdata(bp) }, indirs[i].in_off as usize);

            if i as i32 == num {
                break bp;
            }

            i += 1;

            if nb != 0 {
                brelse(bp);
                continue;
            }

            if pref == 0 {
                pref = ffs2_blkpref(ip, lbn, i as i32 - num - 1, None);
            }

            let mut newb = 0;
            if let Err(e) = ffs_alloc(ip, lbn, pref, bsize, cred, &mut newb) {
                brelse(bp);
                break 'fail e;
            }

            nb = newb;
            allociblk[nalloc] = nb;
            nalloc += 1;
            let nbp = getblk_wait(vp, indirs[i].in_lbn, bsize);
            nbp.b_blkno.set(fsbtodb(fs, nb));
            // SAFETY: the buffer is ours (busy from getblk) and mapped.
            unsafe { clrbuf(nbp) };

            // Write synchronously so that indirect blocks never point at garbage.
            if let Err(e) = bwrite(nbp) {
                brelse(bp);
                break 'fail e;
            }

            if unwindidx < 0 {
                unwindidx = i as i32 - 1;
            }

            // SAFETY: the buffer is ours (busy from bread) and mapped.
            set_daddr64_at(unsafe { bdata(bp) }, indirs[i - 1].in_off as usize, nb);

            // If required, write synchronously, otherwise use delayed write.
            if flags & B_SYNC != 0 {
                let _ = bwrite(bp);
            } else {
                bdwrite(bp);
            }
        };

        // Get the data block, allocating if necessary.
        if nb == 0 {
            {
                // SAFETY: the buffer is ours (busy from bread) and mapped; the slice dies
                // before the buffer is written.
                let data = unsafe { bdata(bp) };
                let bap = |k: usize| daddr64_at(data, k);
                pref = ffs2_blkpref(ip, lbn, indirs[num as usize].in_off, Some(&bap));
            }

            let mut newb = 0;
            if let Err(e) = ffs_alloc(ip, lbn, pref, bsize, cred, &mut newb) {
                brelse(bp);
                break 'fail e;
            }

            nb = newb;
            // *allocblk++ = nb: nothing reads it on the way out.

            if let Some(b) = bpp {
                let nbp = getblk_wait(vp, lbn, bsize);
                nbp.b_blkno.set(fsbtodb(fs, nb));
                if flags & B_CLRBUF != 0 {
                    // SAFETY: the buffer is ours (busy from getblk) and mapped.
                    unsafe { clrbuf(nbp) };
                }
                *b = Some(nbp);
            }

            // SAFETY: the buffer is ours (busy from bread) and mapped.
            set_daddr64_at(
                unsafe { bdata(bp) },
                indirs[num as usize].in_off as usize,
                nb,
            );

            // if (allocib == NULL && unwindidx < 0) unwindidx = i - 1: nothing reads it on
            // the way out.

            // If required, write synchronously, otherwise use delayed write.
            if flags & B_SYNC != 0 {
                let _ = bwrite(bp);
            } else {
                bdwrite(bp);
            }

            return Ok(());
        }

        brelse(bp);

        if let Some(b) = bpp {
            let nbp = if flags & B_CLRBUF != 0 {
                let (nbp, error) = bread(vp, lbn, bsize);
                if let Err(e) = error {
                    brelse(nbp);
                    break 'fail e;
                }
                nbp
            } else {
                let nbp = getblk_wait(vp, lbn, bsize);
                nbp.b_blkno.set(fsbtodb(fs, nb));
                // SAFETY: the buffer is ours (busy from getblk) and mapped.
                unsafe { clrbuf(nbp) };
                nbp
            };

            *b = Some(nbp);
        }

        return Ok(());
    };

    // fail:
    // If we have failed to allocate any blocks, simply return the error. This is the usual
    // case and avoids the need to fsync the file.
    if nalloc == 0 && allocib.is_none() && unwindidx == -1 {
        return Err(error);
    }
    // If we have failed part way through block allocation, we have to deallocate any
    // indirect blocks that we have allocated. We have to fsync the file before we start to
    // get rid of all of its dependencies so that we do not leave them dangling. We have to
    // sync it at the end so that the softdep code does not find any untracked changes.
    // Although this is really slow, running out of disk space is not expected to be a
    // common occurrence. The error return from fsync is ignored as we already have an error
    // to return to the user.
    fsync_wait(vp);
    if unwindidx >= 0 {
        // First write out any buffers we've created to resolve their softdeps. This must be
        // done in reverse order of creation so that we resolve the dependencies in one pass.
        // Write the cylinder group buffers for these buffers too.
        let mut i = num;
        while i >= unwindidx {
            if i == 0 {
                break;
            }

            let bp = getblk_wait(vp, indirs[i as usize].in_lbn, bsize);
            if bp.isset(B_DELWRI) {
                let nb = fsbtodb(fs, cgtod(fs, dtog(fs, dbtofsb(fs, bp.b_blkno.get()))));
                let _ = bwrite(bp);
                let bp = getblk_wait(ip.i_devvp(), nb, fs.fs_cgsize.get());
                if bp.isset(B_DELWRI) {
                    let _ = bwrite(bp);
                } else {
                    bp.set(B_INVAL);
                    brelse(bp);
                }
            } else {
                bp.set(B_INVAL);
                brelse(bp);
            }
            i -= 1;
        }

        // Now that any dependencies that we created have been resolved, we can undo the
        // partial allocation.
        if unwindidx == 0 {
            if let Some(k) = allocib {
                ip.dip_set_ib(k, 0);
            }
            ip.set_flag(IN_CHANGE | IN_UPDATE);
        } else {
            let (bp, r) = bread(vp, indirs[unwindidx as usize].in_lbn, bsize);
            if r.is_err() {
                panic(format_args!("ffs2_balloc: unwind failed"));
            }

            // SAFETY: the buffer is ours (busy from bread) and mapped.
            set_daddr64_at(
                unsafe { bdata(bp) },
                indirs[unwindidx as usize].in_off as usize,
                0,
            );
            let _ = bwrite(bp);
        }

        for i in (unwindidx + 1)..=num {
            let bp = getblk_wait(vp, indirs[i as usize].in_lbn, bsize);
            bp.set(B_INVAL);
            brelse(bp);
        }
    }

    let mut deallocated: i64 = 0;
    for &blk in &allociblk[..nalloc] {
        ffs_blkfree(ip, blk, i64::from(bsize));
        deallocated += i64::from(bsize);
    }

    if deallocated != 0 {
        // Restore user's disk quota because allocation failed.
        let _ = ufs_quota_free_blocks(ip, btodb(deallocated as usize) as Daddr, cred);

        ip.dip_set_blocks(ip.dip_blocks() - btodb(deallocated as usize) as i64);
        ip.set_flag(IN_CHANGE | IN_UPDATE);
    }
    fsync_wait(vp);
    Err(error)
}

/// `ffs_balloc` (`iv_buf_alloc`): balloc defines the structure of file system storage by
/// allocating the physical blocks given the inode and the logical block number in a file.
pub fn ffs_balloc(
    ip: &Inode,
    off: Off,
    size: i32,
    cred: *const Ucred,
    flags: i32,
    bpp: Option<&mut Option<&'static Buf>>,
) -> Result<(), Errno> {
    #[cfg(feature = "ffs2")]
    if ip.fs().fs_magic.get() == FS_UFS2_MAGIC {
        return ffs2_balloc(ip, off, size, cred, flags, bpp);
    }
    ffs1_balloc(ip, off, size, cred, flags, bpp)
}
/* </CODE> */
