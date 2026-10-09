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
 *	@(#)ffs_alloc.c	8.11 (Berkeley) 10/27/94
 *  Modified for ext2fs by Manuel Bouyer.
 */
/* </LICENSES> */

/* <CODE> */
//! ext2fs block and inode allocation: blocks (`ext2fs_alloc`, with the preferred position
//! `ext2fs_blkpref`), inodes (`ext2fs_inode_alloc`, directories spread by
//! `ext2fs_dirpref`), the cylinder group search (`ext2fs_hashalloc`, `ext2fs_alloccg`,
//! `ext2fs_nodealloccg`, `ext2fs_mapsearch`) and freeing (`ext2fs_blkfree`,
//! `ext2fs_inode_free`).
//!
//! Upstream: sys/ufs/ext2fs/ext2fs_alloc.c @ 3ce1f3f79392
//!
//! ## Deviations
//! - `ext2gennumber` is the atomic [`EXT2GENNUMBER`].
//! - The bitmaps are the buffer's bytes (`char *bbp` in C), searched with `isclr`/`setbit`
//!   and `skpc` over slices.
//! - `ext2fs_blkpref` takes the block pointers `bap` as the bytes they are stored in (the
//!   inode's `e2di_blocks` or an indirect block), little-endian (`letoh32`).
//! - `ext2fs_inode_alloc` returns the new vnode (the C's `*vpp`).
//! - A missing credential panics with or without `DIAGNOSTIC` (the C dereferences `NOCRED`
//!   without it).

use core::sync::atomic::{AtomicU64, Ordering};

use crate::kern::kern_tc::gettime;
use crate::kern::subr_prf::{panic, uprintf};
use crate::kern::vfs_bio::{bdwrite, bread, brelse};
use crate::kprintf;
use crate::log;
use crate::sys::errno::Errno;
use crate::sys::mount::VFS_VGET;
use crate::sys::param::{btodb, clrbit, howmany, isclr, setbit};
use crate::sys::select::NBBY;
use crate::sys::syslog::LOG_ERR;
use crate::sys::types::{Daddr, Mode, Uid};
use crate::sys::ucred::Ucred;
use crate::sys::vnode::Vnode;
use crate::ufs::ext2fs::ext2fs::{MExt2fs, dtog, dtogd, freespace, fsbtodb, ino_to_cg};
use crate::ufs::ext2fs::ext2fs_dinode::{EXT2_FIRSTINO, Ext2fsDinode};
use crate::ufs::ufs::dinode::{IFDIR, IFMT, Ufsino};
use crate::ufs::ufs::inode::{IN_CHANGE, IN_UPDATE, Inode, vtoi};
use libkern::skpc;

/// The allocator `ext2fs_hashalloc` runs in each cylinder group (`ext2fs_alloccg` or
/// `ext2fs_nodealloccg`).
type Allocator = fn(&Inode, i32, u32, i32) -> u32;

/// `ext2gennumber`: the last generation number handed to an inode.
pub static EXT2GENNUMBER: AtomicU64 = AtomicU64::new(0);

/// The uid of a credential the allocator is handed (`NOCRED` is a caller's bug, which
/// `DIAGNOSTIC` catches as "missing credential").
fn cred_uid(cred: *const Ucred, func: &str) -> Uid {
    // SAFETY: the credentials an allocation receives are held by its caller (`cred_ref`'s
    // contract).
    match unsafe { crate::sys::vnode::cred_ref(cred) } {
        Some(c) => c.cr_uid.get(),
        None => panic(format_args!("{}: missing credential", func)),
    }
}

/// `ext2fs_alloc`: allocate a block in the file system.
///
/// A preference may be optionally specified. If a preference is given the following
/// hierarchy is used to allocate a block:
///   1) allocate the requested block.
///   2) allocate a rotationally optimal block in the same cylinder.
///   3) allocate a block in the same cylinder group.
///   4) quadratically rehash into other cylinder groups, until an available block is
///      located.
///
/// If no block preference is given the following hierarchy is used to allocate a block:
///   1) allocate a block in the cylinder group that contains the inode for the file.
///   2) quadratically rehash into other cylinder groups, until an available block is
///      located.
pub fn ext2fs_alloc(
    ip: &Inode,
    _lbn: u32,
    mut bpref: u32,
    cred: *const Ucred,
    bnp: &mut u32,
) -> Result<(), Errno> {
    *bnp = 0;
    let fs = ip.e2fs();
    let uid = cred_uid(cred, "ext2fs_alloc");
    'nospace: {
        if fs.e2fs_fbcount() == 0 {
            break 'nospace;
        }
        if uid != 0 && freespace(fs) == 0 {
            break 'nospace;
        }
        if bpref >= fs.e2fs_bcount() {
            bpref = 0;
        }
        let cg = if bpref == 0 {
            ino_to_cg(fs, ip.i_number.get())
        } else {
            dtog(fs, bpref)
        };
        let bno = ext2fs_hashalloc(ip, cg as i32, bpref, fs.e2fs_bsize.get(), ext2fs_alloccg);
        if bno > 0 {
            let nblocks = btodb(fs.e2fs_bsize.get() as usize) as u32;
            ip.set_i_e2fs_nblock(ip.i_e2fs_nblock().wrapping_add(nblocks));
            ip.set_flag(IN_CHANGE | IN_UPDATE);
            *bnp = bno;
            return Ok(());
        }
    }
    // nospace:
    ext2fs_fserr(fs, uid, "file system full");
    uprintf(format_args!(
        "\n{}: write failed, file system is full\n",
        fs.fsmnt_str()
    ));
    Err(Errno::ENOSPC)
}

/// `ext2fs_inode_alloc`: allocate an inode in the file system, and return its vnode.
///
/// If allocating a directory, use `ext2fs_dirpref` to select the inode. If allocating in a
/// directory, the following hierarchy is followed:
///   1) allocate the preferred inode.
///   2) allocate an inode in the same cylinder group.
///   3) quadratically rehash into other cylinder groups, until an available inode is
///      located.
///
/// If no inode preference is given the following hierarchy is used to allocate an inode:
///   1) allocate an inode in cylinder group 0.
///   2) quadratically rehash into other cylinder groups, until an available inode is
///      located.
pub fn ext2fs_inode_alloc(
    pip: &Inode,
    mode: Mode,
    cred: *const Ucred,
) -> Result<&'static Vnode, Errno> {
    let pvp = pip.itov();
    let fs = pip.e2fs();
    'noinodes: {
        if fs.e2fs_ficount() == 0 {
            break 'noinodes;
        }

        let cg = if mode & IFMT == IFDIR {
            ext2fs_dirpref(fs)
        } else {
            ino_to_cg(fs, pip.i_number.get()) as i32
        };
        let ipref = (cg as u32).wrapping_mul(fs.e2fs_ipg()).wrapping_add(1);
        let ino = ext2fs_hashalloc(pip, cg, ipref, mode as i32, ext2fs_nodealloccg);
        if ino == 0 {
            break 'noinodes;
        }
        let Some(mp) = pvp.v_mount.get() else {
            panic(format_args!("ext2fs_inode_alloc: vnode without a mount"));
        };
        let vp = match VFS_VGET(mp, u64::from(ino)) {
            Ok(vp) => vp,
            Err(e) => {
                ext2fs_inode_free(pip, ino, mode);
                return Err(e);
            }
        };
        let ip = vtoi(vp);
        if ip.i_e2fs_mode() != 0 && ip.i_e2fs_nlink() != 0 {
            kprintf!(
                "mode = 0{:o}, nlinks {}, inum = {}, fs = {}\n",
                ip.i_e2fs_mode(),
                ip.i_e2fs_nlink(),
                ip.i_number.get(),
                fs.fsmnt_str()
            );
            panic(format_args!("ext2fs_valloc: dup alloc"));
        }

        ip.with_e2din(|d| *d = Ext2fsDinode::default());

        // Set up a new generation number for this inode.
        ip.set_i_e2fs_gen(next_gennumber() as u32);
        return Ok(vp);
    }
    // noinodes:
    ext2fs_fserr(fs, cred_uid(cred, "ext2fs_inode_alloc"), "out of inodes");
    uprintf(format_args!(
        "\n{}: create/symlink failed, no inodes free\n",
        fs.fsmnt_str()
    ));
    Err(Errno::ENOSPC)
}

/// `if (++ext2gennumber < (u_long)gettime()) ext2gennumber = gettime();`: the next
/// generation number.
pub fn next_gennumber() -> u64 {
    let now = gettime() as u64;
    let mut g = EXT2GENNUMBER.load(Ordering::Relaxed).wrapping_add(1);
    if g < now {
        g = now;
    }
    EXT2GENNUMBER.store(g, Ordering::Relaxed);
    g
}

/// `ext2fs_dirpref`: find a cylinder to place a directory.
///
/// The policy implemented by this algorithm is to select from among those cylinder groups
/// with above the average number of free inodes, the one with the smallest number of
/// directories.
fn ext2fs_dirpref(fs: &MExt2fs) -> i32 {
    let ncg = fs.e2fs_ncg.get();
    let avgifree = (fs.e2fs_ficount() / ncg as u32) as i32;
    let mut maxspace = 0;
    let mut mincg = -1;
    for cg in 0..ncg {
        let gd = fs.gd(cg as usize);
        if i32::from(gd.ext2bgd_nifree) >= avgifree
            && (mincg == -1 || i32::from(gd.ext2bgd_nbfree) > maxspace)
        {
            mincg = cg;
            maxspace = i32::from(gd.ext2bgd_nbfree);
        }
    }
    mincg
}

/// `ext2fs_blkpref`: select the desired position for the next block in a file. The file is
/// logically divided into sections. The first section is composed of the direct blocks.
/// Each additional section contains `fs_maxbpg` blocks.
///
/// If no blocks have been allocated in the first section, the policy is to request a block
/// in the same cylinder group as the inode that describes the file. Otherwise, the policy
/// is to try to allocate the blocks contiguously. The two fields of the ext2 inode
/// extension (see `ufs/ufs/inode.h`) help this. `bap`, if given, holds the block pointers
/// as stored (little-endian), of which entries `0..=baps` are looked at.
pub fn ext2fs_blkpref(ip: &Inode, lbn: u32, baps: i32, bap: Option<&[u8]>) -> Daddr {
    let fs = ip.e2fs();
    // if we are doing contiguous lbn allocation, try to alloc blocks contiguously on disk

    let last_blk = ip.i_e2fs_last_blk().get();
    if last_blk != 0 && lbn == ip.i_e2fs_last_lblk().get().wrapping_add(1) {
        return Daddr::from(last_blk.wrapping_add(1));
    }

    // bap, if provided, gives us a list of blocks to which we want to stay close

    if let Some(bap) = bap {
        let mut i = baps;
        while i >= 0 {
            let o = i as usize * 4;
            let b = u32::from_le_bytes([bap[o], bap[o + 1], bap[o + 2], bap[o + 3]]);
            if b != 0 {
                return Daddr::from(b.wrapping_add(1));
            }
            i -= 1;
        }
    }

    // fall back to the first block of the cylinder containing the inode

    let cg = ino_to_cg(fs, ip.i_number.get());
    Daddr::from(
        fs.e2fs_bpg()
            .wrapping_mul(cg)
            .wrapping_add(fs.e2fs_first_dblock())
            .wrapping_add(1),
    )
}

/// `ext2fs_hashalloc`: implement the cylinder overflow algorithm.
///
/// The policy implemented by this algorithm is:
///   1) allocate the block in its requested cylinder group.
///   2) quadratically rehash on the cylinder group number.
///   3) brute force search for a free block.
fn ext2fs_hashalloc(ip: &Inode, mut cg: i32, pref: u32, size: i32, allocator: Allocator) -> u32 {
    let fs = ip.e2fs();
    let ncg = fs.e2fs_ncg.get();
    let icg = cg;
    // 1: preferred cylinder group
    let result = allocator(ip, cg, pref, size);
    if result != 0 {
        return result;
    }
    // 2: quadratic rehash
    let mut i = 1;
    while i < ncg {
        cg += i;
        if cg >= ncg {
            cg -= ncg;
        }
        let result = allocator(ip, cg, 0, size);
        if result != 0 {
            return result;
        }
        i *= 2;
    }
    // 3: brute force search
    // Note that we start at i == 2, since 0 was checked initially, and 1 is always checked
    // in the quadratic rehash.
    cg = (icg + 2) % ncg;
    for _ in 2..ncg {
        let result = allocator(ip, cg, 0, size);
        if result != 0 {
            return result;
        }
        cg += 1;
        if cg == ncg {
            cg = 0;
        }
    }
    0
}

/// `ext2fs_alloccg`: determine whether a block can be allocated.
///
/// Check to see if a block of the appropriate size is available, and if it is, allocate
/// it.
fn ext2fs_alloccg(ip: &Inode, cg: i32, mut bpref: u32, _size: i32) -> u32 {
    let fs = ip.e2fs();
    let cgu = cg as usize;
    if fs.gd(cgu).ext2bgd_nbfree == 0 {
        return 0;
    }
    let (bp, error) = bread(
        ip.i_devvp(),
        fsbtodb(fs, Daddr::from(fs.gd(cgu).ext2bgd_b_bitmap)),
        fs.e2fs_bsize.get(),
    );
    if error.is_err() || fs.gd(cgu).ext2bgd_nbfree == 0 {
        brelse(bp);
        return 0;
    }
    let bno;
    {
        // SAFETY: the buffer is ours (busy from bread) and mapped; the slice dies before
        // the buffer is written.
        let bbp = unsafe { bp.data() };
        'gotit: {
            if dtog(fs, bpref) != cg as u32 {
                bpref = 0;
            }
            if bpref != 0 {
                bpref = dtogd(fs, bpref);
                // if the requested block is available, use it
                if isclr(bbp, bpref as usize) {
                    bno = bpref;
                    break 'gotit;
                }
            }
            // no blocks in the requested cylinder, so take next available one in this
            // cylinder group. first try to get 8 contiguous blocks, then fall back to a
            // single block.
            let start = if bpref != 0 {
                (dtogd(fs, bpref) / NBBY as u32) as i32
            } else {
                0
            };
            let end = howmany(fs.e2fs_fpg() as usize, NBBY) as i32 - start;
            for loc in start..end {
                if bbp[loc as usize] == 0 {
                    bno = loc as u32 * NBBY as u32;
                    break 'gotit;
                }
            }
            for loc in 0..start {
                if bbp[loc as usize] == 0 {
                    bno = loc as u32 * NBBY as u32;
                    break 'gotit;
                }
            }

            bno = ext2fs_mapsearch(fs, bbp, bpref);
        }
        // gotit:
        if cfg!(feature = "diagnostic") && !isclr(bbp, bno as usize) {
            panic(format_args!(
                "ext2fs_alloccg: dup alloc: cg={} bno={} fs={}",
                cg,
                bno,
                fs.fsmnt_str()
            ));
        }
        setbit(bbp, bno as usize);
    }
    fs.set_e2fs_fbcount(fs.e2fs_fbcount().wrapping_sub(1));
    fs.with_gd_mut(cgu, |gd| {
        gd.ext2bgd_nbfree = gd.ext2bgd_nbfree.wrapping_sub(1)
    });
    fs.e2fs_fmod.set(1);
    bdwrite(bp);
    (cg as u32)
        .wrapping_mul(fs.e2fs_fpg())
        .wrapping_add(fs.e2fs_first_dblock())
        .wrapping_add(bno)
}

/// `ext2fs_nodealloccg`: determine whether an inode can be allocated.
///
/// Check to see if an inode is available, and if it is, allocate it using the following
/// policy:
///   1) allocate the requested inode.
///   2) allocate the next available inode after the requested inode in the specified
///      cylinder group.
fn ext2fs_nodealloccg(ip: &Inode, cg: i32, ipref: Ufsino, mode: i32) -> Ufsino {
    let mut ipref = ipref.wrapping_sub(1); // to avoid a lot of (ipref -1)
    let fs = ip.e2fs();
    let cgu = cg as usize;
    if fs.gd(cgu).ext2bgd_nifree == 0 {
        return 0;
    }
    let (bp, error) = bread(
        ip.i_devvp(),
        fsbtodb(fs, Daddr::from(fs.gd(cgu).ext2bgd_i_bitmap)),
        fs.e2fs_bsize.get(),
    );
    if error.is_err() {
        brelse(bp);
        return 0;
    }
    {
        // SAFETY: the buffer is ours (busy from bread) and mapped; the slice dies before
        // the buffer is written.
        let ibp = unsafe { bp.data() };
        'gotit: {
            if ipref != 0 {
                ipref %= fs.e2fs_ipg();
                if isclr(ibp, ipref as usize) {
                    break 'gotit;
                }
            }
            let mut start = (ipref / NBBY as u32) as usize;
            let mut len = howmany((fs.e2fs_ipg() - ipref) as usize, NBBY);
            let mut loc = skpc(0xff, &ibp[start..start + len]);
            if loc == 0 {
                len = start + 1;
                start = 0;
                loc = skpc(0xff, &ibp[..len]);
                if loc == 0 {
                    kprintf!("cg = {}, ipref = {}, fs = {}\n", cg, ipref, fs.fsmnt_str());
                    panic(format_args!("ext2fs_nodealloccg: map corrupted"));
                }
            }
            let i = start + len - loc;
            let map = ibp[i];
            ipref = (i * NBBY) as u32;
            let mut bit = 1u32;
            while bit < (1 << NBBY) {
                if u32::from(map) & bit == 0 {
                    break 'gotit;
                }
                bit <<= 1;
                ipref += 1;
            }
            kprintf!("fs = {}\n", fs.fsmnt_str());
            panic(format_args!("ext2fs_nodealloccg: block not in map"));
        }
        // gotit:
        setbit(ibp, ipref as usize);
    }
    fs.set_e2fs_ficount(fs.e2fs_ficount().wrapping_sub(1));
    fs.with_gd_mut(cgu, |gd| {
        gd.ext2bgd_nifree = gd.ext2bgd_nifree.wrapping_sub(1)
    });
    fs.e2fs_fmod.set(1);
    if mode as Mode & IFMT == IFDIR {
        fs.with_gd_mut(cgu, |gd| {
            gd.ext2bgd_ndirs = gd.ext2bgd_ndirs.wrapping_add(1)
        });
    }
    bdwrite(bp);
    (cg as u32)
        .wrapping_mul(fs.e2fs_ipg())
        .wrapping_add(ipref)
        .wrapping_add(1)
}

/// `ext2fs_blkfree`: free a block.
///
/// The specified block is placed back in the free map.
pub fn ext2fs_blkfree(ip: &Inode, bno: u32) {
    let fs = ip.e2fs();
    let cg = dtog(fs, bno) as usize;
    if bno >= fs.e2fs_bcount() {
        kprintf!("bad block {}, ino {}\n", bno, ip.i_number.get());
        ext2fs_fserr(fs, ip.i_e2fs_uid().get(), "bad block");
        return;
    }
    let (bp, error) = bread(
        ip.i_devvp(),
        fsbtodb(fs, Daddr::from(fs.gd(cg).ext2bgd_b_bitmap)),
        fs.e2fs_bsize.get(),
    );
    if error.is_err() {
        brelse(bp);
        return;
    }
    {
        // SAFETY: the buffer is ours (busy from bread) and mapped; the slice dies before
        // the buffer is written.
        let bbp = unsafe { bp.data() };
        let bno = dtogd(fs, bno);
        if isclr(bbp, bno as usize) {
            panic(format_args!(
                "ext2fs_blkfree: freeing free block: dev = 0x{:x}, block = {}, fs = {}",
                ip.i_dev.get(),
                bno,
                fs.fsmnt_str()
            ));
        }

        clrbit(bbp, bno as usize);
    }
    fs.set_e2fs_fbcount(fs.e2fs_fbcount().wrapping_add(1));
    fs.with_gd_mut(cg, |gd| {
        gd.ext2bgd_nbfree = gd.ext2bgd_nbfree.wrapping_add(1)
    });

    fs.e2fs_fmod.set(1);
    bdwrite(bp);
}

/// `ext2fs_inode_free`: free an inode.
///
/// The specified inode is placed back in the free map.
pub fn ext2fs_inode_free(pip: &Inode, ino: Ufsino, mode: Mode) {
    let fs = pip.e2fs();
    if ino > fs.e2fs_icount() || ino < EXT2_FIRSTINO {
        panic(format_args!(
            "ifree: range: dev = 0x{:x}, ino = {}, fs = {}",
            pip.i_dev.get(),
            ino,
            fs.fsmnt_str()
        ));
    }
    let cg = ino_to_cg(fs, ino) as usize;
    let (bp, error) = bread(
        pip.i_devvp(),
        fsbtodb(fs, Daddr::from(fs.gd(cg).ext2bgd_i_bitmap)),
        fs.e2fs_bsize.get(),
    );
    if error.is_err() {
        brelse(bp);
        return;
    }
    {
        // SAFETY: the buffer is ours (busy from bread) and mapped; the slice dies before
        // the buffer is written.
        let ibp = unsafe { bp.data() };
        let ino = (ino - 1) % fs.e2fs_ipg();
        if isclr(ibp, ino as usize) {
            kprintf!(
                "dev = 0x{:x}, ino = {}, fs = {}\n",
                pip.i_dev.get(),
                ino,
                fs.fsmnt_str()
            );
            if fs.e2fs_ronly.get() == 0 {
                panic(format_args!("ifree: freeing free inode"));
            }
        }
        clrbit(ibp, ino as usize);
    }
    fs.set_e2fs_ficount(fs.e2fs_ficount().wrapping_add(1));
    fs.with_gd_mut(cg, |gd| {
        gd.ext2bgd_nifree = gd.ext2bgd_nifree.wrapping_add(1)
    });
    if mode & IFMT == IFDIR {
        fs.with_gd_mut(cg, |gd| gd.ext2bgd_ndirs = gd.ext2bgd_ndirs.wrapping_sub(1));
    }
    fs.e2fs_fmod.set(1);
    bdwrite(bp);
}

/// `ext2fs_mapsearch`: find a block in the specified cylinder group.
///
/// It is a panic if a request is made to find a block if none are available.
fn ext2fs_mapsearch(fs: &MExt2fs, bbp: &[u8], bpref: u32) -> u32 {
    // find the fragment by searching through the free block map for an appropriate bit
    // pattern
    let mut start = if bpref != 0 {
        (dtogd(fs, bpref) / NBBY as u32) as usize
    } else {
        0
    };
    let mut len = howmany(fs.e2fs_fpg() as usize, NBBY) - start;
    let mut loc = skpc(0xff, &bbp[start..start + len]);
    if loc == 0 {
        len = start + 1;
        start = 0;
        loc = skpc(0xff, &bbp[start..start + len]);
        if loc == 0 {
            kprintf!(
                "start = {}, len = {}, fs = {}\n",
                start,
                len,
                fs.fsmnt_str()
            );
            panic(format_args!("ext2fs_alloccg: map corrupted"));
        }
    }
    let i = start + len - loc;
    let map = bbp[i];
    let mut bno = (i * NBBY) as u32;
    let mut bit = 1u32;
    while bit < (1 << NBBY) {
        if u32::from(map) & bit == 0 {
            return bno;
        }
        bit <<= 1;
        bno += 1;
    }
    kprintf!("fs = {}\n", fs.fsmnt_str());
    panic(format_args!("ext2fs_mapsearch: block not in map"));
}

/// `ext2fs_fserr`: prints the name of a file system with an error diagnostic.
///
/// The form of the error message is:
///     fs: error message
fn ext2fs_fserr(fs: &MExt2fs, uid: Uid, cp: &str) {
    log!(LOG_ERR, "uid {} on {}: {}\n", uid, fs.fsmnt_str(), cp);
}
/* </CODE> */

/* <TESTS> */
#[cfg(test)]
mod tests {
    // Host tests for the allocator's pure parts: the bitmap search (`ext2fs_mapsearch`), the
    // preferred block (`ext2fs_blkpref`), the directory group (`ext2fs_dirpref`) and the
    // order `ext2fs_hashalloc` tries the groups in. The bitmap allocations themselves
    // (`ext2fs_alloccg`, `ext2fs_nodealloccg`, the frees) run in the read-write mount test of
    // `ext2fs_vfsops`.

    use std::boxed::Box;
    use std::sync::Mutex;
    use std::vec::Vec;
    use std::{assert_eq, vec};

    use super::*;
    use crate::ufs::ext2fs::ext2fs::Ext2Gd;

    /// A file system of `ncg` groups of 64 blocks (from block 1) and 32 inodes, with these
    /// group descriptors.
    fn fs(ncg: i32, gds: Vec<Ext2Gd>) -> &'static MExt2fs {
        let fs: &'static MExt2fs = Box::leak(Box::new(MExt2fs::new()));
        fs.set_e2fs_fpg(64);
        fs.set_e2fs_bpg(64);
        fs.set_e2fs_first_dblock(1);
        fs.set_e2fs_ipg(32);
        fs.e2fs_ncg.set(ncg);
        if !gds.is_empty() {
            fs.e2fs_gd.set(gds.leak().as_mut_ptr());
        }
        fs
    }

    /// An inode numbered `ino` on `fs`.
    fn inode(fs: &'static MExt2fs, ino: Ufsino) -> &'static Inode {
        let ip: &'static Inode = Box::leak(Box::new(Inode::new()));
        ip.i_e2fs.set(Some(fs));
        ip.i_number.set(ino);
        ip
    }

    #[test]
    fn mapsearch_finds_the_first_clear_bit_from_the_preference() {
        let fs = fs(1, vec![]);
        let mut map = [0xffu8, 0xff, 0x0f, 0, 0, 0, 0, 0];
        assert_eq!(ext2fs_mapsearch(fs, &map, 0), 20);
        // From byte 3 (block 1 + 25 is bit 25).
        assert_eq!(ext2fs_mapsearch(fs, &map, 26), 24);
        // Nothing from the preference to the end: search again from the start.
        map = [0x7f, 0xff, 0xff, 0xff, 0xff, 0xff, 0xff, 0xff];
        assert_eq!(ext2fs_mapsearch(fs, &map, 34), 7);
    }

    #[test]
    fn blkpref_follows_the_last_block_then_the_pointers_then_the_group() {
        let fs = fs(2, vec![]);
        let ip = inode(fs, 40); // group 1
        ip.i_e2fs_last_blk().set(100);
        ip.i_e2fs_last_lblk().set(4);
        assert_eq!(ext2fs_blkpref(ip, 5, 0, None), 101);

        let mut bap = [0u8; 12];
        bap[..4].copy_from_slice(&10u32.to_le_bytes());
        assert_eq!(ext2fs_blkpref(ip, 7, 2, Some(&bap)), 11);
        bap[8..].copy_from_slice(&30u32.to_le_bytes());
        assert_eq!(ext2fs_blkpref(ip, 7, 2, Some(&bap)), 31);
        assert_eq!(ext2fs_blkpref(ip, 7, 1, Some(&bap)), 11);
        assert_eq!(ext2fs_blkpref(ip, 7, 2, Some(&[0; 12])), 64 + 1 + 1);
        assert_eq!(ext2fs_blkpref(ip, 7, 0, None), 64 + 1 + 1);
    }

    #[test]
    fn dirpref_picks_the_roomiest_group_with_enough_free_inodes() {
        let gd = |nifree, nbfree| Ext2Gd {
            ext2bgd_nifree: nifree,
            ext2bgd_nbfree: nbfree,
            ..Ext2Gd::default()
        };
        let fs = fs(3, vec![gd(5, 10), gd(20, 3), gd(20, 7)]);
        fs.set_e2fs_ficount(45);
        assert_eq!(ext2fs_dirpref(fs), 2);
        fs.set_e2fs_ficount(3);
        assert_eq!(ext2fs_dirpref(fs), 0);
    }

    /// The groups the recording allocators were asked for.
    static VISITED: Mutex<Vec<i32>> = Mutex::new(Vec::new());

    fn never(_ip: &Inode, cg: i32, _pref: u32, _size: i32) -> u32 {
        VISITED.lock().unwrap().push(cg);
        0
    }

    fn in_group_4(_ip: &Inode, cg: i32, _pref: u32, _size: i32) -> u32 {
        VISITED.lock().unwrap().push(cg);
        if cg == 4 { 77 } else { 0 }
    }

    #[test]
    fn hashalloc_tries_the_group_then_rehashes_then_every_group() {
        let ip = inode(fs(5, vec![]), 1);
        VISITED.lock().unwrap().clear();
        assert_eq!(ext2fs_hashalloc(ip, 1, 9, 1024, never), 0);
        assert_eq!(*VISITED.lock().unwrap(), [1, 2, 4, 3, 3, 4, 0]);
        VISITED.lock().unwrap().clear();
        assert_eq!(ext2fs_hashalloc(ip, 1, 9, 1024, in_group_4), 77);
        assert_eq!(*VISITED.lock().unwrap(), [1, 2, 4]);
    }
}
/* </TESTS> */
