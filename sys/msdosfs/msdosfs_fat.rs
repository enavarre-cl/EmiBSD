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

/*-
 * Copyright (C) 1994, 1995, 1997 Wolfgang Solfrank.
 * Copyright (C) 1994, 1995, 1997 TooLs GmbH.
 * All rights reserved.
 * Original code by Paul Popelka (paulp@uts.amdahl.com) (see below).
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
 * Written by Paul Popelka (paulp@uts.amdahl.com)
 *
 * You can do anything you want with this software, just don't say you wrote
 * it, and don't remove this notice.
 *
 * This software is provided "as is".
 *
 * The author supplies this software to be publicly redistributed on the
 * understanding that the author is not responsible for the correct
 * functioning of this software in any circumstances and is not liable for
 * any damages caused by this software.
 *
 * October 1992
 */
/* </LICENSES> */

/* <CODE> */
//! The file allocation table: mapping a file's clusters to disk blocks (`pcbmap`) with the
//! per-denode fat cache, reading and writing FAT entries (`fatentry`, `fatchain`) and keeping
//! every copy of the FAT and the FSInfo block in step (`updatefats`), allocating and freeing
//! clusters against the in-use bitmap (`clusteralloc`, `clusterfree`, `freeclusterchain`,
//! `fillinusemap`), and growing a file (`extendfile`).
//!
//! Upstream: sys/msdosfs/msdosfs_fat.c @ 3ce1f3f79392
//!
//! Updating entries in 12 bit fats is a pain in the butt. The following picture shows where
//! nibbles go when moving from a 12 bit cluster number into the appropriate bytes in the FAT.
//!
//! ```text
//!     byte m        byte m+1      byte m+2
//!     +----+----+   +----+----+   +----+----+
//!     |  0    1 |   |  2    3 |   |  4    5 |   FAT bytes
//!     +----+----+   +----+----+   +----+----+
//!
//!     +----+----+----+   +----+----+----+
//!     |  3    0    1 |   |  4    5    2 |
//!     +----+----+----+   +----+----+----+
//!     cluster n          cluster n+1
//! ```
//!
//! Where n is even. m = n + (n >> 2).
//!
//! ## Deviations
//! - The fat cache statistics (`fc_fileextends`, `fc_lfcempty`, `fc_bmapcalls`,
//!   `fc_lmdistance[]`, `fc_largedistance`) are atomics.
//! - The reads and writes of one FAT entry, which the C spells out in `pcbmap`, `fatentry`,
//!   `fatchain`, `freeclusterchain` and `fillinusemap` (the `getushort`/`getulong`, the FAT12
//!   nibble shift and the mask; the FAT12 half-word merge and FAT32's kept high bits), are
//!   `fat_read` and `fat_write` over the FAT block's bytes.
//! - `clusteralloc`'s two identical scans of the in-use bitmap (from the pseudo-random start
//!   to the last cluster, then from 0 to the start) are one function, `scanfree`.
//! - `fatblock`'s optional out-parameters are its tuple result. The optional out-parameters of
//!   `pcbmap`, `clusterfree`, `fatentry`, `chainalloc`, `clusteralloc` and `extendfile` are
//!   `Option<&mut T>`; `extendfile`'s `bpp` is `Option<&mut Option<&'static Buf>>`.
//! - `getblk(..., 0, INFSLP)` "never fails" in C; here `getblk` returns an `Option` and the
//!   call is repeated until it yields the buffer (`getblk_wait`), as in the ffs port.
//! - `fatentry`'s two `DIAGNOSTIC` argument checks are behind feature `diagnostic`; a
//!   `FAT_GET` without a place for the result (a crash in C without `DIAGNOSTIC`) reads the
//!   entry and drops it.
//! - `fillinusemap`'s final `brelse(bp)` is skipped when no FAT block was read (a file system
//!   without clusters), where the C would release NULL.

use core::cmp::min;
use core::sync::atomic::{AtomicI32, Ordering};

use crate::kassert;
use crate::kern::kern_tc::microtime;
use crate::kern::subr_prf::{panic, printf};
use crate::kern::vfs_bio::{bdwrite, bread, brelse, bwrite, getblk};
use crate::msdosfs::bpb::{Fsinfo, getulong, getushort, putulong, putushort};
use crate::msdosfs::denode::{
    Denode, FC_LASTFC, FC_LASTMAP, FC_OLASTFC, FC_SIZE, FCE_EMPTY, fc_setcache,
};
use crate::msdosfs::direntry::ATTR_DIRECTORY;
use crate::msdosfs::fat::{
    CLUST_END, CLUST_EOFE, CLUST_FIRST, CLUST_RSRVD, DE_CLEAR, FAT_GET, FAT_GET_AND_SET, FAT_SET,
    FAT12_MASK, FAT16_MASK, FAT32_MASK, MSDOSFSFREE, MSDOSFSROOT, msdosfseof,
};
use crate::msdosfs::msdosfsmount::{
    MSDOSFS_FATMIRROR, MSDOSFSMNT_WAITONFAT, Msdosfsmount, N_INUSEBITS, cntobn, de_bn2cn, de_cn2bn,
    de_cn2off, fatofs, fsi_size,
};
use crate::sys::buf::{Buf, clrbuf};
use crate::sys::errno::Errno;
use crate::sys::param::DEV_BSIZE;
use crate::sys::systm::INFSLP;
use crate::sys::types::Daddr;
use crate::sys::vnode::Vnode;

/// `LMMAX`: the size of `fc_lmdistance[]`.
const LMMAX: usize = 20;

/// `fc_fileextends`: # of file extends.
pub static FC_FILEEXTENDS: AtomicI32 = AtomicI32::new(0);
/// `fc_lfcempty`: # of time last file cluster cache entry was empty.
pub static FC_LFCEMPTY: AtomicI32 = AtomicI32::new(0);
/// `fc_bmapcalls`: # of times pcbmap was called.
pub static FC_BMAPCALLS: AtomicI32 = AtomicI32::new(0);
/// `fc_lmdistance[]`: counters for how far off the last cluster mapped entry was.
pub static FC_LMDISTANCE: [AtomicI32; LMMAX] = [const { AtomicI32::new(0) }; LMMAX];
/// `fc_largedistance`: off by more than `LMMAX`.
pub static FC_LARGEDISTANCE: AtomicI32 = AtomicI32::new(0);

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

/// The entry of cluster `cn` at byte `bo` of a FAT block: the 16 or 32 bits there, shifted for
/// an odd FAT12 cluster, masked to the FAT's width.
fn fat_read(fatmask: u32, data: &[u8], bo: usize, cn: u32) -> u32 {
    let mut readcn = if fatmask == FAT32_MASK {
        getulong(&data[bo..])
    } else {
        u32::from(getushort(&data[bo..]))
    };
    if fatmask == FAT12_MASK && cn & 1 != 0 {
        readcn >>= 4;
    }
    readcn & fatmask
}

/// Stores `newcontents` as the entry of cluster `cn` at byte `bo` of a FAT block: a FAT12
/// entry shares its middle byte with its neighbour, and FAT32 keeps the high 4 bits.
fn fat_write(fatmask: u32, data: &mut [u8], bo: usize, cn: u32, newcontents: u32) {
    match fatmask {
        FAT12_MASK => {
            let mut readcn = u32::from(getushort(&data[bo..]));
            if cn & 1 != 0 {
                readcn &= 0x000f;
                readcn |= newcontents << 4;
            } else {
                readcn &= 0xf000;
                readcn |= newcontents & 0xfff;
            }
            putushort(&mut data[bo..], readcn as u16);
        }
        FAT16_MASK => putushort(&mut data[bo..], newcontents as u16),
        FAT32_MASK => {
            // According to spec we have to retain the high order bits of the fat entry.
            let mut readcn = getulong(&data[bo..]);
            readcn &= !FAT32_MASK;
            readcn |= newcontents & FAT32_MASK;
            putulong(&mut data[bo..], readcn);
        }
        _ => {}
    }
}

/// `fatblock(pmp, ofs, &bn, &size, &bo)`: the FAT block holding byte `ofs` of the FAT: its
/// block number on the device, its size in bytes and the offset of `ofs` in it.
fn fatblock(pmp: &Msdosfsmount, ofs: u32) -> (u32, u32, usize) {
    let mut bn = ofs / pmp.pm_fatblocksize.get() * pmp.pm_fatblocksec.get();
    let size = min(
        pmp.pm_fatblocksec.get(),
        pmp.pm_FATsecs.get().wrapping_sub(bn),
    )
    .wrapping_mul(DEV_BSIZE as u32);
    bn = bn
        .wrapping_add(pmp.pm_fatblk.get())
        .wrapping_add(pmp.pm_curfat.get().wrapping_mul(pmp.pm_FATsecs.get()));
    (bn, size, (ofs % pmp.pm_fatblocksize.get()) as usize)
}

/// `pcbmap(dep, findcn, bnp, cnp, sp)`: map the logical cluster number of a file into a
/// physical disk sector that is filesystem relative.
///
/// - `dep`: address of denode representing the file of interest
/// - `findcn`: file relative cluster whose filesystem relative cluster number and/or block
///   number are/is to be found
/// - `bnp`: where to place the file system relative block number, if given
/// - `cnp`: where to place the file system relative cluster number, if given
/// - `sp`: where to place the block size for the file/dir, if given
///
/// This function has one side effect. If the requested file relative cluster is beyond the
/// end of file, then the actual number of clusters in the file is returned in `*cnp` along
/// with `E2BIG`. This is useful for determining how long a directory is.
pub fn pcbmap(
    dep: &Denode,
    findcn: u32,
    bnp: Option<&mut Daddr>,
    cnp: Option<&mut u32>,
    sp: Option<&mut i32>,
) -> Result<(), Errno> {
    FC_BMAPCALLS.fetch_add(1, Ordering::Relaxed);

    // If they don't give us someplace to return a value then don't bother doing anything.
    if bnp.is_none() && cnp.is_none() && sp.is_none() {
        return Ok(());
    }

    let pmp = dep.pmp();
    let fatmask = pmp.pm_fatmask.get();
    let mut cn = dep.de_StartCluster.get();
    // The "file" that makes up the root directory is contiguous, permanently allocated, of
    // fixed size, and is not made up of clusters. If the cluster number is beyond the end of
    // the root directory, then return the number of clusters in the file.
    if cn == MSDOSFSROOT {
        if dep.de_Attributes.get() & ATTR_DIRECTORY != 0 {
            if de_cn2off(pmp, findcn) >= dep.de_FileSize.get() {
                if let Some(cnp) = cnp {
                    *cnp = de_bn2cn(pmp, pmp.pm_rootdirsize.get());
                }
                return Err(Errno::E2BIG);
            }
            if let Some(bnp) = bnp {
                *bnp = Daddr::from(pmp.pm_rootdirblk.get().wrapping_add(de_cn2bn(pmp, findcn)));
            }
            if let Some(cnp) = cnp {
                *cnp = MSDOSFSROOT;
            }
            if let Some(sp) = sp {
                *sp = min(
                    pmp.pm_bpcluster.get(),
                    dep.de_FileSize.get() - de_cn2off(pmp, findcn),
                ) as i32;
            }
            return Ok(());
        } else {
            // just an empty file
            if let Some(cnp) = cnp {
                *cnp = 0;
            }
            return Err(Errno::E2BIG);
        }
    }

    // All other files do I/O in cluster sized blocks.
    if let Some(sp) = sp {
        *sp = pmp.pm_bpcluster.get() as i32;
    }

    // Rummage around in the fat cache, maybe we can avoid tromping thru every fat entry for
    // the file. And, keep track of how far off the cache was from where we wanted to be.
    let mut i = 0;
    fc_lookup(dep, findcn, &mut i, &mut cn);
    let distance = findcn.wrapping_sub(i) as usize;
    if distance >= LMMAX {
        FC_LARGEDISTANCE.fetch_add(1, Ordering::Relaxed);
    } else {
        FC_LMDISTANCE[distance].fetch_add(1, Ordering::Relaxed);
    }

    // Handle all other files or directories the normal way.
    let mut bp: Option<&'static Buf> = None;
    let mut bp_bn = u32::MAX;
    let mut prevcn = 0;
    let mut hiteof = false;
    while i < findcn {
        // Stop with all reserved clusters, not just with EOF.
        if (cn | !fatmask) >= CLUST_RSRVD {
            hiteof = true;
            break;
        }
        let byteoffset = fatofs(pmp, cn);
        let (bn, bsize, bo) = fatblock(pmp, byteoffset);
        let b = match bp {
            Some(b) if bn == bp_bn => b,
            _ => {
                if let Some(old) = bp.take() {
                    brelse(old);
                }
                let (b, error) = bread(pmp.devvp(), Daddr::from(bn), bsize as i32);
                if let Err(e) = error {
                    brelse(b);
                    return Err(e);
                }
                bp = Some(b);
                bp_bn = bn;
                b
            }
        };
        prevcn = cn;
        if bo >= bsize as usize {
            brelse(b);
            return Err(Errno::EIO);
        }
        // SAFETY: the FAT block is busy for this function (from `bread`) and mapped; the slice
        // ends with the statement.
        cn = fat_read(fatmask, unsafe { bdata(b) }, bo, prevcn);

        // Force the special cluster numbers to be the same for all cluster sizes to let the
        // rest of msdosfs handle all cases the same.
        if (cn | !fatmask) >= CLUST_RSRVD {
            cn |= !fatmask;
        }
        i += 1;
    }

    if !hiteof && !msdosfseof(pmp, cn) {
        if let Some(b) = bp {
            brelse(b);
        }
        if let Some(bnp) = bnp {
            *bnp = Daddr::from(cntobn(pmp, cn));
        }
        if let Some(cnp) = cnp {
            *cnp = cn;
        }
        fc_setcache(dep, FC_LASTMAP, i, cn);
        return Ok(());
    }

    // hiteof:
    if let Some(cnp) = cnp {
        *cnp = i;
    }
    if let Some(b) = bp {
        brelse(b);
    }
    // update last file cluster entry in the fat cache
    fc_setcache(dep, FC_LASTFC, i.wrapping_sub(1), prevcn);
    Err(Errno::E2BIG)
}

/// `fc_lookup(dep, findcn, frcnp, fsrcnp)`: find the closest entry in the fat cache to the
/// cluster we are looking for. `*frcnp` and `*fsrcnp` are left alone when no entry is at or
/// before `findcn`.
pub fn fc_lookup(dep: &Denode, findcn: u32, frcnp: &mut u32, fsrcnp: &mut u32) {
    let mut closest: Option<usize> = None;
    for i in 0..FC_SIZE {
        let cn = dep.fc(i).fc_frcn;
        if cn != FCE_EMPTY && cn <= findcn && closest.is_none_or(|c| cn > dep.fc(c).fc_frcn) {
            closest = Some(i);
        }
    }
    if let Some(c) = closest {
        let fc = dep.fc(c);
        *frcnp = fc.fc_frcn;
        *fsrcnp = fc.fc_fsrcn;
    }
}

/// `fc_purge(dep, frcn)`: purge the fat cache in denode `dep` of all entries relating to file
/// relative cluster `frcn` and beyond.
pub fn fc_purge(dep: &Denode, frcn: u32) {
    for fcp in &dep.de_fc {
        let mut fc = fcp.get();
        if fc.fc_frcn >= frcn {
            fc.fc_frcn = FCE_EMPTY;
            fcp.set(fc);
        }
    }
}

/// `updatefats(pmp, bp, fatbn)`: update the fat. If mirroring the fat, update all copies,
/// with the first copy as last. Else update only the current fat (ignoring the others).
///
/// - `pmp`: msdosfsmount structure for filesystem to update
/// - `bp`: addr of modified fat block (written or released here)
/// - `fatbn`: block number relative to begin of filesystem of the modified fat block.
pub fn updatefats(pmp: &Msdosfsmount, bp: &'static Buf, mut fatbn: u32) {
    let waitonfat = pmp.pm_flags.get() & MSDOSFSMNT_WAITONFAT != 0;

    // If we have an FSInfo block, update it.
    if pmp.pm_fsinfo.get() != 0 {
        let (bpn, error) = bread(pmp.devvp(), Daddr::from(pmp.pm_fsinfo.get()), fsi_size(pmp));
        if error.is_err() {
            // Ignore the error, but turn off FSInfo update for the future.
            pmp.pm_fsinfo.set(0);
            brelse(bpn);
        } else {
            // SAFETY: the FSInfo block is busy for this function (from `bread`) and mapped;
            // the slice ends with `fp`'s last use.
            let fp = Fsinfo::at_mut(unsafe { bdata(bpn) }, 0);
            putulong(&mut fp.fsinfree, pmp.pm_freeclustercount.get());
            if waitonfat {
                let _ = bwrite(bpn);
            } else {
                bdwrite(bpn);
            }
        }
    }

    if pmp.pm_flags.get() & MSDOSFS_FATMIRROR != 0 {
        // Now copy the block(s) of the modified fat to the other copies of the fat and write
        // them out. This is faster than reading in the other fats and then writing them back
        // out. This could tie up the fat for quite a while. Preventing others from accessing
        // it. To prevent us from going after the fat quite so much we use delayed writes,
        // unless they specified "synchronous" when the filesystem was mounted. If synch is
        // asked for then use bwrite()'s and really slow things down.
        for _ in 1..pmp.pm_FATs() {
            fatbn = fatbn.wrapping_add(pmp.pm_FATsecs.get());
            // getblk() never fails
            let bcount = bp.b_bcount.get();
            let bpn = getblk_wait(pmp.devvp(), Daddr::from(fatbn), bcount as i32);
            let n = bcount as usize;
            // SAFETY: both buffers are busy for this function (`bpn` from `getblk`, `bp` from
            // the caller's `bread`) and mapped, and distinct; the slices end before either is
            // written.
            let (dst, src) = unsafe { (bdata(bpn), bdata(bp)) };
            dst[..n].copy_from_slice(&src[..n]);
            if waitonfat {
                let _ = bwrite(bpn);
            } else {
                bdwrite(bpn);
            }
        }
    }

    // Write out the first (or current) fat last.
    if waitonfat {
        let _ = bwrite(bp);
    } else {
        bdwrite(bp);
    }
    // Maybe update fsinfo sector here?
}

/// `usemap_alloc(pmp, cn)`: mark cluster `cn` in use.
fn usemap_alloc(pmp: &Msdosfsmount, cn: u32) {
    kassert!(cn <= pmp.pm_maxcluster.get());

    let w = &pmp.inusemap()[(cn / N_INUSEBITS) as usize];
    w.set(w.get() | 1 << (cn % N_INUSEBITS));
    pmp.pm_freeclustercount
        .set(pmp.pm_freeclustercount.get().wrapping_sub(1));
}

/// `usemap_free(pmp, cn)`: mark cluster `cn` free.
fn usemap_free(pmp: &Msdosfsmount, cn: u32) {
    kassert!(cn <= pmp.pm_maxcluster.get());

    pmp.pm_freeclustercount
        .set(pmp.pm_freeclustercount.get().wrapping_add(1));
    let w = &pmp.inusemap()[(cn / N_INUSEBITS) as usize];
    w.set(w.get() & !(1 << (cn % N_INUSEBITS)));
}

/// `clusterfree(pmp, cluster, oldcnp)`: free one cluster; its old FAT entry goes to `*oldcnp`
/// when given.
pub fn clusterfree(
    pmp: &Msdosfsmount,
    cluster: u32,
    oldcnp: Option<&mut u32>,
) -> Result<(), Errno> {
    let mut oldcn = 0;

    usemap_free(pmp, cluster);
    if let Err(error) = fatentry(FAT_GET_AND_SET, pmp, cluster, Some(&mut oldcn), MSDOSFSFREE) {
        usemap_alloc(pmp, cluster);
        return Err(error);
    }
    // If the cluster was successfully marked free, then update the count of free clusters,
    // and turn off the "allocated" bit in the "in use" cluster bit map.
    if let Some(oldcnp) = oldcnp {
        *oldcnp = oldcn;
    }
    Ok(())
}

/// `fatentry(function, pmp, cn, oldcontents, newcontents)`: get or set or 'get and set' the
/// cluster'th entry in the fat.
///
/// - `function`: whether to get or set a fat entry (`FAT_GET`, `FAT_SET`)
/// - `pmp`: the msdosfsmount structure for the filesystem whose fat is to be manipulated
/// - `cn`: which cluster is of interest
/// - `oldcontents`: receives the contents of the cluster'th entry if this is a get function
/// - `newcontents`: the new value to be written into the cluster'th element of the fat if
///   this is a set function
///
/// This function can also be used to free a cluster by setting the fat entry for a cluster to
/// 0. All copies of the fat are updated if this is a set function. NOTE: If fatentry() marks
/// a cluster as free it does not update the inusemap in the msdosfsmount structure. This is
/// left to the caller.
pub fn fatentry(
    function: i32,
    pmp: &Msdosfsmount,
    cn: u32,
    oldcontents: Option<&mut u32>,
    newcontents: u32,
) -> Result<(), Errno> {
    #[cfg(feature = "diagnostic")]
    {
        // Be sure they asked us to do something.
        if function & (FAT_SET | FAT_GET) == 0 {
            printf(format_args!(
                "fatentry(): function code doesn't specify get or set\n"
            ));
            return Err(Errno::EINVAL);
        }

        // If they asked us to return a cluster number but didn't tell us where to put it,
        // give them an error.
        if function & FAT_GET != 0 && oldcontents.is_none() {
            printf(format_args!(
                "fatentry(): get function with no place to put result\n"
            ));
            return Err(Errno::EINVAL);
        }
    }

    // Be sure the requested cluster is in the filesystem.
    if cn < CLUST_FIRST || cn > pmp.pm_maxcluster.get() {
        return Err(Errno::EINVAL);
    }

    let fatmask = pmp.pm_fatmask.get();
    let byteoffset = fatofs(pmp, cn);
    let (bn, bsize, bo) = fatblock(pmp, byteoffset);
    let (bp, error) = bread(pmp.devvp(), Daddr::from(bn), bsize as i32);
    if let Err(e) = error {
        brelse(bp);
        return Err(e);
    }

    if function & FAT_GET != 0 {
        // SAFETY: the FAT block is busy for this function (from `bread`) and mapped; the slice
        // ends with the statement.
        let mut readcn = fat_read(fatmask, unsafe { bdata(bp) }, bo, cn);
        // map reserved fat entries to same values for all fats
        if (readcn | !fatmask) >= CLUST_RSRVD {
            readcn |= !fatmask;
        }
        if let Some(oldcontents) = oldcontents {
            *oldcontents = readcn;
        }
    }
    if function & FAT_SET != 0 {
        // SAFETY: the FAT block is busy for this function (from `bread`) and mapped; the slice
        // ends with the statement.
        fat_write(fatmask, unsafe { bdata(bp) }, bo, cn, newcontents);
        updatefats(pmp, bp, bn);
        pmp.pm_fmod.set(1);
        return Ok(());
    }
    brelse(bp);
    Ok(())
}

/// `fatchain(pmp, start, count, fillwith)`: update a contiguous cluster chain.
///
/// - `pmp`: mount point
/// - `start`: first cluster of chain
/// - `count`: number of clusters in chain
/// - `fillwith`: what to write into fat entry of last cluster
fn fatchain(
    pmp: &Msdosfsmount,
    mut start: u32,
    mut count: u32,
    fillwith: u32,
) -> Result<(), Errno> {
    // Be sure the clusters are in the filesystem.
    if start < CLUST_FIRST || start.wrapping_add(count).wrapping_sub(1) > pmp.pm_maxcluster.get() {
        return Err(Errno::EINVAL);
    }

    let fatmask = pmp.pm_fatmask.get();
    while count > 0 {
        let byteoffset = fatofs(pmp, start);
        let (bn, bsize, mut bo) = fatblock(pmp, byteoffset);
        let (bp, error) = bread(pmp.devvp(), Daddr::from(bn), bsize as i32);
        if let Err(e) = error {
            brelse(bp);
            return Err(e);
        }
        {
            // SAFETY: the FAT block is busy for this function (from `bread`) and mapped; the
            // slice ends with this block, before `updatefats` takes the buffer.
            let data = unsafe { bdata(bp) };
            while count > 0 {
                // The entry at `bo` is cluster `start` (before the increment); it points at the
                // next cluster, or holds `fillwith` for the last one.
                start += 1;
                count -= 1;
                let newc = if count > 0 { start } else { fillwith };
                fat_write(fatmask, data, bo, start - 1, newc);
                match fatmask {
                    FAT12_MASK => {
                        bo += 1;
                        if start & 1 == 0 {
                            bo += 1;
                        }
                    }
                    FAT16_MASK => bo += 2,
                    FAT32_MASK => bo += 4,
                    _ => {}
                }
                if bo >= bsize as usize {
                    break;
                }
            }
        }
        updatefats(pmp, bp, bn);
    }
    pmp.pm_fmod.set(1);
    Ok(())
}

/// `chainlength(pmp, start, count)`: check the length of a free cluster chain starting at
/// `start`, up to `count` (the maximum interesting length).
pub fn chainlength(pmp: &Msdosfsmount, mut start: u32, count: u32) -> u32 {
    let maxcluster = pmp.pm_maxcluster.get();
    if start > maxcluster {
        return 0;
    }
    let inusemap = pmp.inusemap();
    let max_idx = maxcluster / N_INUSEBITS;
    let mut idx = start / N_INUSEBITS;
    start %= N_INUSEBITS;
    let mut map = inusemap[idx as usize].get();
    map &= !((1u32 << start) - 1);
    if map != 0 {
        let mut len = map.trailing_zeros() - start;
        len = min(len, count);
        len = min(len, maxcluster - start + 1);
        return len;
    }
    let mut len = N_INUSEBITS - start;
    if len >= count {
        len = min(count, maxcluster - start + 1);
        return len;
    }
    loop {
        idx += 1;
        if idx > max_idx {
            break;
        }
        if len >= count {
            break;
        }
        map = inusemap[idx as usize].get();
        if map != 0 {
            len += map.trailing_zeros();
            break;
        }
        len += N_INUSEBITS;
    }
    len = min(len, count);
    len = min(len, maxcluster - start + 1);
    len
}

/// `chainalloc(pmp, start, count, fillwith, retcluster, got)`: allocate the contiguous free
/// clusters `start` to `start + count - 1`.
///
/// - `fillwith`: put this value into the fat entry for the last allocated cluster
/// - `retcluster`: put the first allocated cluster's number here
/// - `got`: how many clusters were actually allocated
pub fn chainalloc(
    pmp: &Msdosfsmount,
    start: u32,
    count: u32,
    fillwith: u32,
    retcluster: Option<&mut u32>,
    got: Option<&mut u32>,
) -> Result<(), Errno> {
    for k in 0..count {
        usemap_alloc(pmp, start.wrapping_add(k));
    }
    fatchain(pmp, start, count, fillwith)?;
    if let Some(retcluster) = retcluster {
        *retcluster = start;
    }
    if let Some(got) = got {
        *got = count;
    }
    Ok(())
}

/// One of `clusteralloc`'s two scans of the in-use bitmap, over the clusters from `from` to
/// `end` (excluded): the first free run of `count` clusters, or `None` after recording the
/// longest shorter run in `foundcn`/`foundl`.
fn scanfree(
    pmp: &Msdosfsmount,
    from: u32,
    end: u64,
    count: u32,
    foundcn: &mut u32,
    foundl: &mut u32,
) -> Option<u32> {
    let inusemap = pmp.inusemap();
    let mut cn = from;
    while u64::from(cn) < end {
        let idx = cn / N_INUSEBITS;
        let mut map = inusemap[idx as usize].get();
        map |= (1u32 << (cn % N_INUSEBITS)) - 1;
        if map != u32::MAX {
            cn = idx * N_INUSEBITS + (map ^ u32::MAX).trailing_zeros();
            let l = chainlength(pmp, cn, count);
            if l >= count {
                return Some(cn);
            }
            if l > *foundl {
                *foundcn = cn;
                *foundl = l;
            }
            cn += l + 1;
            continue;
        }
        cn += N_INUSEBITS - cn % N_INUSEBITS;
    }
    None
}

/// `clusteralloc(pmp, start, count, retcluster, got)`: allocate contiguous free clusters.
///
/// - `start`: preferred start of cluster chain (0 for a new file)
/// - `count`: number of clusters requested
/// - `retcluster`: put the first allocated cluster's number here
/// - `got`: how many clusters were actually allocated
pub fn clusteralloc(
    pmp: &Msdosfsmount,
    mut start: u32,
    count: u32,
    retcluster: Option<&mut u32>,
    got: Option<&mut u32>,
) -> Result<(), Errno> {
    let fillwith = CLUST_EOFE;
    let len;

    if start != 0 {
        len = chainlength(pmp, start, count);
        if len >= count {
            return chainalloc(pmp, start, count, fillwith, retcluster, got);
        }
    } else {
        // This is a new file, initialize start.
        let tv = microtime();
        start = ((tv.tv_usec >> 10) | tv.tv_usec) as u32;
        len = 0;
    }

    // Start at a (pseudo) random place to maximize cluster runs under multiple writers.
    let maxcluster = pmp.pm_maxcluster.get();
    let newst = start.wrapping_mul(1103515245).wrapping_add(12345) % maxcluster.wrapping_add(1);
    let mut foundl = 0;
    let mut foundcn = 0;

    // From `newst` to the last cluster, then from 0 up to `newst`.
    for (from, end) in [(newst, u64::from(maxcluster) + 1), (0, u64::from(newst))] {
        if let Some(cn) = scanfree(pmp, from, end, count, &mut foundcn, &mut foundl) {
            return chainalloc(pmp, cn, count, fillwith, retcluster, got);
        }
    }

    if foundl == 0 {
        return Err(Errno::ENOSPC);
    }

    if len != 0 {
        chainalloc(pmp, start, len, fillwith, retcluster, got)
    } else {
        chainalloc(pmp, foundcn, foundl, fillwith, retcluster, got)
    }
}

/// `freeclusterchain(pmp, cluster)`: free a chain of clusters, from `cluster` (the number of
/// the 1st cluster in the chain of clusters to be freed) to its end.
pub fn freeclusterchain(pmp: &Msdosfsmount, mut cluster: u32) -> Result<(), Errno> {
    let fatmask = pmp.pm_fatmask.get();
    let mut bp: Option<&'static Buf> = None;
    let mut lbn = u32::MAX;
    let mut bn = 0;

    while cluster >= CLUST_FIRST && cluster <= pmp.pm_maxcluster.get() {
        let byteoffset = fatofs(pmp, cluster);
        let (fbn, bsize, bo) = fatblock(pmp, byteoffset);
        bn = fbn;
        let b = match bp {
            Some(b) if lbn == bn => b,
            _ => {
                if let Some(old) = bp.take() {
                    updatefats(pmp, old, lbn);
                }
                let (b, error) = bread(pmp.devvp(), Daddr::from(bn), bsize as i32);
                if let Err(e) = error {
                    brelse(b);
                    return Err(e);
                }
                bp = Some(b);
                lbn = bn;
                b
            }
        };
        usemap_free(pmp, cluster);
        // SAFETY: the FAT block is busy for this function (from `bread`) and mapped; the
        // slice ends with this iteration, before `updatefats` takes the buffer.
        let data = unsafe { bdata(b) };
        let next = fat_read(fatmask, data, bo, cluster);
        fat_write(fatmask, data, bo, cluster, MSDOSFSFREE);
        cluster = next;
        if (cluster | !fatmask) >= CLUST_RSRVD {
            cluster |= fatmask;
        }
    }
    if let Some(b) = bp {
        updatefats(pmp, b, bn);
    }
    Ok(())
}

/// `fillinusemap(pmp)`: read in fat blocks looking for free clusters. For every free cluster
/// found turn off its corresponding bit in the `pm_inusemap`.
pub fn fillinusemap(pmp: &Msdosfsmount) -> Result<(), Errno> {
    let fatmask = pmp.pm_fatmask.get();

    // Mark all clusters in use, we mark the free ones in the fat scan loop further down.
    for w in pmp.inusemap() {
        w.set(u32::MAX);
    }

    // Figure how many free clusters are in the filesystem by ripping through the fat counting
    // the number of entries whose content is zero. These represent free clusters.
    pmp.pm_freeclustercount.set(0);
    let mut bp: Option<&'static Buf> = None;
    for cn in CLUST_FIRST..=pmp.pm_maxcluster.get() {
        let byteoffset = fatofs(pmp, cn);
        let bo = (byteoffset % pmp.pm_fatblocksize.get()) as usize;
        let b = match bp {
            Some(b) if bo != 0 => b,
            _ => {
                // Read new FAT block.
                if let Some(old) = bp.take() {
                    brelse(old);
                }
                let (bn, bsize, _) = fatblock(pmp, byteoffset);
                let (b, error) = bread(pmp.devvp(), Daddr::from(bn), bsize as i32);
                if let Err(e) = error {
                    brelse(b);
                    return Err(e);
                }
                bp = Some(b);
                b
            }
        };
        // SAFETY: the FAT block is busy for this function (from `bread`) and mapped; the slice
        // ends with the statement.
        let readcn = fat_read(fatmask, unsafe { bdata(b) }, bo, cn);

        if readcn == 0 {
            usemap_free(pmp, cn);
        }
    }
    if let Some(b) = bp {
        brelse(b);
    }
    Ok(())
}

/// `extendfile(dep, count, bpp, ncp, flags)`: allocate a new cluster and chain it onto the
/// end of the file.
///
/// - `dep`: the file to extend
/// - `count`: number of clusters to allocate
/// - `bpp`: where to return the address of the buf header for the first new file block
/// - `ncp`: where to put cluster number of the first newly allocated cluster; if `None`, do
///   not return the cluster number
/// - `flags`: see fat.h (`DE_CLEAR`)
///
/// NOTE: This function is not responsible for turning on the `DE_UPDATE` bit of the
/// `de_flag` field of the denode and it does not change the `de_FileSize` field. This is left
/// for the caller to do.
pub fn extendfile(
    dep: &Denode,
    mut count: u32,
    mut bpp: Option<&mut Option<&'static Buf>>,
    mut ncp: Option<&mut u32>,
    flags: i32,
) -> Result<(), Errno> {
    let pmp = dep.pmp();

    // Don't try to extend the root directory.
    if dep.de_StartCluster.get() == MSDOSFSROOT && dep.de_Attributes.get() & ATTR_DIRECTORY != 0 {
        printf(format_args!(
            "extendfile(): attempt to extend root directory\n"
        ));
        return Err(Errno::ENOSPC);
    }

    // If the "file's last cluster" cache entry is empty, and the file is not empty, then fill
    // the cache entry by calling pcbmap().
    FC_FILEEXTENDS.fetch_add(1, Ordering::Relaxed);
    if dep.fc(FC_LASTFC).fc_frcn == FCE_EMPTY && dep.de_StartCluster.get() != 0 {
        FC_LFCEMPTY.fetch_add(1, Ordering::Relaxed);
        let mut cn = 0;
        // we expect it to return E2BIG
        match pcbmap(dep, CLUST_END, None, Some(&mut cn), None) {
            Err(Errno::E2BIG) => {}
            other => return other,
        }
    }

    // Preserve value for the last cluster before extending the file to speed up further
    // lookups.
    let lastfc = dep.fc(FC_LASTFC);
    fc_setcache(dep, FC_OLASTFC, lastfc.fc_frcn, lastfc.fc_fsrcn);

    while count > 0 {
        // Allocate a new cluster chain and cat onto the end of the file. If the file is empty
        // we make de_StartCluster point to the new block. Note that de_StartCluster being 0
        // is sufficient to be sure the file is empty since we exclude attempts to extend the
        // root directory above, and the root dir is the only file with a startcluster of 0
        // that has blocks allocated (sort of).
        let start = if dep.de_StartCluster.get() == 0 {
            0
        } else {
            dep.fc(FC_LASTFC).fc_fsrcn.wrapping_add(1)
        };
        let mut cn = 0;
        let mut got = 0;
        clusteralloc(pmp, start, count, Some(&mut cn), Some(&mut got))?;

        count -= got;

        // Give them the filesystem relative cluster number if they want it.
        if let Some(ncp) = ncp.take() {
            *ncp = cn;
        }

        let mut frcn;
        if dep.de_StartCluster.get() == 0 {
            dep.de_StartCluster.set(cn);
            frcn = 0;
        } else {
            let lastfc = dep.fc(FC_LASTFC);
            if let Err(error) = fatentry(FAT_SET, pmp, lastfc.fc_fsrcn, None, cn) {
                let _ = clusterfree(pmp, cn, None);
                return Err(error);
            }
            frcn = lastfc.fc_frcn.wrapping_add(1);
        }

        // Update the "last cluster of the file" entry in the denode's fat cache.
        fc_setcache(
            dep,
            FC_LASTFC,
            frcn.wrapping_add(got).wrapping_sub(1),
            cn.wrapping_add(got).wrapping_sub(1),
        );

        if flags & DE_CLEAR != 0 {
            while got > 0 {
                got -= 1;
                // Get the buf header for the new block of the file.
                let bp = if dep.de_Attributes.get() & ATTR_DIRECTORY != 0 {
                    let bp = getblk_wait(
                        pmp.devvp(),
                        Daddr::from(cntobn(pmp, cn)),
                        pmp.pm_bpcluster.get() as i32,
                    );
                    cn += 1;
                    bp
                } else {
                    let bp = getblk_wait(
                        dep.detov(),
                        Daddr::from(frcn),
                        pmp.pm_bpcluster.get() as i32,
                    );
                    frcn += 1;
                    // Do the bmap now, as in msdosfs_write.
                    let mut blkno = bp.b_blkno.get();
                    if pcbmap(dep, bp.b_lblkno.get() as u32, Some(&mut blkno), None, None).is_err()
                    {
                        blkno = -1;
                    }
                    bp.b_blkno.set(blkno);
                    if bp.b_blkno.get() == -1 {
                        panic(format_args!("extendfile: pcbmap"));
                    }
                    bp
                };
                // SAFETY: `getblk` returned the buffer busy to us and mapped; no slice of it
                // is alive.
                unsafe { clrbuf(bp) };
                if let Some(bpp) = bpp.take() {
                    *bpp = Some(bp);
                } else {
                    bdwrite(bp);
                }
            }
        }
    }

    Ok(())
}
/* </CODE> */

/* <TESTS> */
#[cfg(test)]
mod tests {
    // Host tests of `msdosfs_fat.rs`: the FAT entry packing of the three widths over a block's
    // bytes, the FAT block arithmetic, the fat cache, and the in-use bitmap searches.

    use super::*;
    use std::boxed::Box;
    use std::vec;
    use std::vec::Vec;

    /// A mount of `maxcluster` clusters with the given FAT width and an in-use bitmap in which
    /// every cluster is free except 0 and 1 and the bits past `maxcluster`.
    fn mount(fatmask: u32, maxcluster: u32) -> &'static Msdosfsmount {
        let pmp: &'static Msdosfsmount = Box::leak(Box::new(Msdosfsmount::new()));
        pmp.pm_fatmask.set(fatmask);
        let (mult, div) = match fatmask {
            FAT12_MASK => (3, 2),
            FAT16_MASK => (2, 1),
            _ => (4, 1),
        };
        pmp.pm_fatmult.set(mult);
        pmp.pm_fatdiv.set(div);
        pmp.pm_maxcluster.set(maxcluster);
        let words = (maxcluster as usize + 1).div_ceil(32);
        let map: &'static mut [u32] = Box::leak(vec![0u32; words].into_boxed_slice());
        pmp.pm_inusemap.set(map.as_mut_ptr());
        for cn in 0..words as u32 * 32 {
            if cn < CLUST_FIRST || cn > maxcluster {
                let w = &pmp.inusemap()[(cn / 32) as usize];
                w.set(w.get() | 1 << (cn % 32));
            }
        }
        pmp.pm_freeclustercount.set(maxcluster - 1);
        pmp
    }

    /// The FAT bytes of `entries` (cluster 0 first) for a FAT of the given width, written through
    /// `fat_write`, and checked back through `fat_read`.
    fn pack(fatmask: u32, mult: u32, div: u32, entries: &[u32]) -> Vec<u8> {
        let mut fat = vec![0u8; entries.len() * 4 + 4];
        for (cn, &v) in entries.iter().enumerate() {
            let bo = (cn as u32 * mult / div) as usize;
            fat_write(fatmask, &mut fat, bo, cn as u32, v);
        }
        for (cn, &v) in entries.iter().enumerate() {
            let bo = (cn as u32 * mult / div) as usize;
            assert_eq!(
                fat_read(fatmask, &fat, bo, cn as u32),
                v & fatmask,
                "cluster {cn}"
            );
        }
        fat
    }

    #[test]
    fn fat12_entries_share_nibbles() {
        // Clusters 0..3: media 0xff8, 0xfff, then 3 -> 0xfff (EOF), 0x123.
        let fat = pack(FAT12_MASK, 3, 2, &[0xff8, 0xfff, 0xfff, 0x123]);
        assert_eq!(&fat[..6], &[0xf8, 0xff, 0xff, 0xff, 0x3f, 0x12]);
        // Rewriting an odd entry leaves its even neighbour alone, and the reverse.
        let mut fat = fat;
        fat_write(FAT12_MASK, &mut fat, 4, 3, 0xabc);
        assert_eq!(fat_read(FAT12_MASK, &fat, 3, 2), 0xfff);
        assert_eq!(fat_read(FAT12_MASK, &fat, 4, 3), 0xabc);
        fat_write(FAT12_MASK, &mut fat, 3, 2, 0x005);
        assert_eq!(fat_read(FAT12_MASK, &fat, 4, 3), 0xabc);
        assert_eq!(&fat[3..6], &[0x05, 0xc0, 0xab]);
        // CLUST_EOFE is cut to 12 bits.
        fat_write(FAT12_MASK, &mut fat, 4, 3, CLUST_EOFE);
        assert_eq!(fat_read(FAT12_MASK, &fat, 4, 3), 0xfff);
        assert_eq!(fat_read(FAT12_MASK, &fat, 3, 2), 0x005);
    }

    #[test]
    fn fat16_and_fat32_entries() {
        let fat = pack(FAT16_MASK, 2, 1, &[0xfff8, 0xffff, 0x0003, 0xffff]);
        assert_eq!(&fat[..8], &[0xf8, 0xff, 0xff, 0xff, 0x03, 0x00, 0xff, 0xff]);
        let mut fat = pack(FAT32_MASK, 4, 1, &[0x0fff_fff8, 0x0fff_ffff, 0x0000_0003]);
        assert_eq!(&fat[8..12], &[0x03, 0, 0, 0]);
        // FAT32 keeps the high four bits of an entry.
        fat[11] = 0xa0;
        fat_write(FAT32_MASK, &mut fat, 8, 2, CLUST_EOFE);
        assert_eq!(&fat[8..12], &[0xff, 0xff, 0xff, 0xaf]);
        assert_eq!(fat_read(FAT32_MASK, &fat, 8, 2), FAT32_MASK);
        // An unknown width writes nothing.
        fat_write(0, &mut fat, 8, 2, 0);
        assert_eq!(&fat[8..12], &[0xff, 0xff, 0xff, 0xaf]);
    }

    #[test]
    fn fat_blocks() {
        let pmp = mount(FAT12_MASK, 4000);
        // 512-byte sectors: FAT12 blocks of 3 sectors, a 12-sector FAT at block 1, two copies.
        pmp.pm_fatblocksize.set(3 * 512);
        pmp.pm_fatblocksec.set(3);
        pmp.pm_FATsecs.set(12);
        pmp.pm_fatblk.set(1);
        assert_eq!(fatblock(pmp, fatofs(pmp, 0)), (1, 1536, 0));
        // Cluster 1025 is at byte 1537: the second block, offset 1.
        assert_eq!(fatblock(pmp, fatofs(pmp, 1025)), (4, 1536, 1));
        // The last block is cut at the end of the FAT.
        pmp.pm_FATsecs.set(10);
        assert_eq!(fatblock(pmp, 5000), (10, 512, 5000 - 3 * 1536));
        // The current FAT of a FAT32 file system that does not mirror.
        pmp.pm_curfat.set(1);
        assert_eq!(fatblock(pmp, 0), (11, 1536, 0));
    }

    #[test]
    fn fat_cache() {
        let dep = Denode::new();
        fc_purge(&dep, 0);
        let (mut i, mut cn) = (0, 77);
        fc_lookup(&dep, 10, &mut i, &mut cn);
        assert_eq!((i, cn), (0, 77), "an empty cache leaves the start");
        fc_setcache(&dep, FC_LASTMAP, 4, 104);
        fc_setcache(&dep, FC_LASTFC, 9, 109);
        fc_setcache(&dep, FC_OLASTFC, 6, 106);
        fc_lookup(&dep, 8, &mut i, &mut cn);
        assert_eq!((i, cn), (6, 106));
        fc_lookup(&dep, 100, &mut i, &mut cn);
        assert_eq!((i, cn), (9, 109));
        fc_purge(&dep, 6);
        assert_eq!(dep.fc(FC_LASTMAP).fc_frcn, 4);
        assert_eq!(dep.fc(FC_LASTFC).fc_frcn, FCE_EMPTY);
        assert_eq!(dep.fc(FC_OLASTFC).fc_frcn, FCE_EMPTY);
        fc_lookup(&dep, 100, &mut i, &mut cn);
        assert_eq!((i, cn), (4, 104));
    }

    #[test]
    fn usemap_and_chainlength() {
        let pmp = mount(FAT16_MASK, 100);
        assert_eq!(chainlength(pmp, 2, 10), 10);
        assert_eq!(chainlength(pmp, 101, 1), 0);
        assert_eq!(chainlength(pmp, 2, 1000), 99);
        usemap_alloc(pmp, 40);
        assert_eq!(pmp.pm_freeclustercount.get(), 98);
        assert_eq!(chainlength(pmp, 30, 20), 10);
        assert_eq!(chainlength(pmp, 40, 20), 0);
        assert_eq!(chainlength(pmp, 2, 100), 38);
        usemap_free(pmp, 40);
        assert_eq!(pmp.pm_freeclustercount.get(), 99);
        assert_eq!(chainlength(pmp, 30, 20), 20);
        // A run that starts in one word and ends in the next.
        usemap_alloc(pmp, 70);
        assert_eq!(chainlength(pmp, 60, 50), 10);
        // A run up to the last cluster stops at the in-use bits past it.
        assert_eq!(chainlength(pmp, 71, 100), 30);
    }

    #[test]
    fn scanfree_finds_runs() {
        let pmp = mount(FAT16_MASK, 100);
        for cn in 2..50 {
            usemap_alloc(pmp, cn);
        }
        usemap_alloc(pmp, 60);
        let (mut foundcn, mut foundl) = (0, 0);
        // From 10: the first free cluster is 50, a run of 10.
        assert_eq!(
            scanfree(pmp, 10, 101, 5, &mut foundcn, &mut foundl),
            Some(50)
        );
        assert_eq!(
            scanfree(pmp, 10, 101, 20, &mut foundcn, &mut foundl),
            Some(61)
        );
        assert_eq!(scanfree(pmp, 10, 101, 50, &mut foundcn, &mut foundl), None);
        assert_eq!((foundcn, foundl), (61, 40));
        let (mut foundcn, mut foundl) = (0, 0);
        assert_eq!(scanfree(pmp, 0, 55, 50, &mut foundcn, &mut foundl), None);
        assert_eq!((foundcn, foundl), (50, 10));
    }
}
/* </TESTS> */
