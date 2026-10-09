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
 *	@(#)ffs_alloc.c	8.11 (Berkeley) 10/27/94
 */
/* </LICENSES> */

/* <CODE> */
//! The fast file system's allocator: blocks and fragments (`ffs_alloc`, `ffs_realloccg`,
//! `ffs_blkfree`), inodes (`ffs_inode_alloc`, `ffs_freefile`), the placement policies
//! (`ffs_dirpref`, `ffs1_blkpref`, `ffs2_blkpref`) and the cylinder group searches under
//! them (`ffs_hashalloc`, `ffs_alloccg`, `ffs_alloccgblk`, `ffs_nodealloccg`,
//! `ffs_mapsearch`, `ffs_clusteracct`).
//!
//! Upstream: sys/ufs/ffs/ffs_alloc.c @ 3ce1f3f79392
//!
//! ## Deviations
//! - A cylinder group is reached through `fs.rs`'s [`CgBuf`]; the maps are byte slices.
//! - `ffs1_blkpref`/`ffs2_blkpref`'s `bap` (the block pointer array the C passes, either the
//!   dinode's `di_db` or an indirect block's data) is a function that reads entry `i`, or
//!   `None` for the C's NULL.
//! - `ffs_hashalloc`'s allocator is a `fn` pointer of the C's type.
//! - The `static struct timeval` rate limits of the "file system full" messages are
//!   `StaticCell`s changed under the kernel lock.
//! - `uprintf` (a message on the process's terminal, `tty.c`) is reported: the console
//!   message of `ffs_fserr` (`log(LOG_ERR, ...)`) remains.
//! - The quota calls are `quota.rs`'s: `ufs_quota.rs`'s with feature `quota` (`option QUOTA`),
//!   the no-quota answers of `ufs_quota_stub.c` without it.

use crate::dev::rnd::{arc4random, arc4random_uniform};
use crate::kern::kern_tc::nanotime;
use crate::kern::kern_time::ratecheck;
use crate::kern::subr_prf::panic;
use crate::kern::vfs_bio::{bawrite, bdwrite, bread, brelse, buf_adjcnt, getblk};
use crate::kprintf;
use crate::log;
use crate::sys::buf::{B_DONE, Buf};
use crate::sys::errno::Errno;
use crate::sys::mount::VFS_VGET;
use crate::sys::param::{btodb, clrbit, howmany, isclr, isset, setbit};
use crate::sys::syslog::LOG_ERR;
use crate::sys::systm::INFSLP;
use crate::sys::time::Timeval;
use crate::sys::types::{Daddr, Mode, Uid};
use crate::sys::ucred::Ucred;
use crate::sys::vnode::Vnode;
use crate::ufs::ffs::ffs_subr::{
    ffs_clrblock, ffs_fragacct, ffs_isblock, ffs_isfreeblock, ffs_setblock,
};
use crate::ufs::ffs::fs::{
    AROUND, CgBuf, FRAGTBL, FS_OPTSPACE, FS_OPTTIME, FS_UFS2_MAGIC, Fs, INSIDE, add, blkmap,
    blknum, cbtocylno, cbtorpos, cgbase, cgdata, cgmeta, cgtod, dtog, dtogd, fragnum, fragoff,
    fragstoblks, freespace, fsbtodb, ino_to_cg, ino_to_fsba, inopb, nindir, numfrags, sub,
};
use crate::ufs::ufs::dinode::{IFDIR, IFMT, NDADDR, Ufsino};
use crate::ufs::ufs::inode::{IN_CHANGE, IN_UPDATE, Inode, vtoi};
use crate::ufs::ufs::quota::{ufs_quota_alloc_blocks, ufs_quota_free_blocks};
use crate::uvm::uvm_vnode::uvm_vnp_uncache;
use libkern::{StaticCell, scanc, skpc};

/// `fserr_interval`: at most one "file system full" message every 2 seconds.
const FSERR_INTERVAL: Timeval = Timeval {
    tv_sec: 2,
    tv_usec: 0,
};

/// `ffs_alloc`'s `fsfull_last`.
static FFS_ALLOC_FSFULL_LAST: StaticCell<Timeval> = StaticCell::new(Timeval {
    tv_sec: 0,
    tv_usec: 0,
});
/// `ffs_realloccg`'s `fsfull_last`.
static FFS_REALLOCCG_FSFULL_LAST: StaticCell<Timeval> = StaticCell::new(Timeval {
    tv_sec: 0,
    tv_usec: 0,
});
/// `ffs_inode_alloc`'s `fsnoinodes_last`.
static FFS_INODE_ALLOC_FSNOINODES_LAST: StaticCell<Timeval> = StaticCell::new(Timeval {
    tv_sec: 0,
    tv_usec: 0,
});

/// `ratecheck(&last, &fserr_interval)` on one of the statics above.
fn fserr_ratecheck(last: &StaticCell<Timeval>) -> bool {
    // SAFETY: the rate limits are changed under the kernel lock (ffs runs under it), and the
    // reference does not outlive the call.
    ratecheck(unsafe { last.get_mut() }, &FSERR_INTERVAL)
}

/// `ffs_fserr(fs, uid, cp)`: report a file system error.
fn ffs_fserr(fs: &Fs, uid: Uid, cp: &str) {
    log!(LOG_ERR, "uid {} on {}: {}\n", uid, fs.fsmnt(), cp);
}

/// `uprintf(fmt, ...)` on the thread's terminal (`tty.c`, not ported).
fn uprintf_unported() {
    let _ = crate::unported!("uprintf (tty.c)");
}

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

/// `ffs_alloc`: allocate a block in the file system.
///
/// The size of the requested block is given, which must be some multiple of `fs_fsize` and
/// <= `fs_bsize`. A preference may be optionally specified. If a preference is given the
/// following hierarchy is used to allocate a block:
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
pub fn ffs_alloc(
    ip: &Inode,
    _lbn: Daddr,
    mut bpref: Daddr,
    size: i32,
    cred: *const Ucred,
    bnp: &mut Daddr,
) -> Result<(), Errno> {
    *bnp = 0;
    let fs = ip.fs();
    if cfg!(feature = "diagnostic")
        && (size as u32 > fs.fs_bsize.get() as u32 || fragoff(fs, i64::from(size)) != 0)
    {
        kprintf!(
            "dev = 0x{:x}, bsize = {}, size = {}, fs = {}\n",
            ip.i_dev.get(),
            fs.fs_bsize.get(),
            size,
            fs.fsmnt()
        );
        panic(format_args!("ffs_alloc: bad size"));
    }
    let uid = cred_uid(cred, "ffs_alloc");

    'nospace: {
        if size == fs.fs_bsize.get() && fs.fs_cstotal.cs_nbfree.get() == 0 {
            break 'nospace;
        }
        if uid != 0 && freespace(fs, fs.fs_minfree.get()) <= 0 {
            break 'nospace;
        }

        ufs_quota_alloc_blocks(ip, btodb(size as usize) as Daddr, cred)?;

        // Start allocation in the preferred block's cylinder group or the file's inode's
        // cylinder group if no preferred block was specified.
        if bpref >= fs.fs_size.get() {
            bpref = 0;
        }
        let cg = if bpref == 0 {
            ino_to_cg(fs, ip.i_number.get())
        } else {
            dtog(fs, bpref)
        };

        // Try allocating a block.
        let bno = ffs_hashalloc(ip, cg, bpref, size, ffs_alloccg);
        if bno > 0 {
            // allocation successful, update inode data
            ip.dip_set_blocks(ip.dip_blocks() + btodb(size as usize) as i64);
            ip.set_flag(IN_CHANGE | IN_UPDATE);
            *bnp = bno;
            return Ok(());
        }

        // Restore user's disk quota because allocation failed.
        let _ = ufs_quota_free_blocks(ip, btodb(size as usize) as Daddr, cred);
    }

    // nospace:
    if fserr_ratecheck(&FFS_ALLOC_FSFULL_LAST) {
        ffs_fserr(fs, uid, "file system full");
        // uprintf("\n%s: write failed, file system is full\n", fs->fs_fsmnt)
        uprintf_unported();
    }
    Err(Errno::ENOSPC)
}

/// `ffs_realloccg`: reallocate a fragment to a bigger size.
///
/// The number and size of the old block is given, and a preference and new size is also
/// specified. The allocator attempts to extend the original block. Failing that, the regular
/// block allocator is invoked to get an appropriate block. `bpp`, when given, receives the
/// file's buffer for the block, grown and zero-filled; `blknop` the new block.
#[allow(clippy::too_many_arguments)] // the C signature
pub fn ffs_realloccg(
    ip: &Inode,
    lbprev: Daddr,
    mut bpref: Daddr,
    osize: i32,
    nsize: i32,
    cred: *const Ucred,
    mut bpp: Option<&mut Option<&'static Buf>>,
    blknop: Option<&mut Daddr>,
) -> Result<(), Errno> {
    let mut bp: Option<&'static Buf> = None;
    let mut quota_updated: Daddr = 0;

    if let Some(b) = bpp.as_deref_mut() {
        *b = None;
    }
    let fs = ip.fs();
    if cfg!(feature = "diagnostic")
        && (osize as u32 > fs.fs_bsize.get() as u32
            || fragoff(fs, i64::from(osize)) != 0
            || nsize as u32 > fs.fs_bsize.get() as u32
            || fragoff(fs, i64::from(nsize)) != 0)
    {
        kprintf!(
            "dev = 0x{:x}, bsize = {}, osize = {}, nsize = {}, fs = {}\n",
            ip.i_dev.get(),
            fs.fs_bsize.get(),
            osize,
            nsize,
            fs.fsmnt()
        );
        panic(format_args!("ffs_realloccg: bad size"));
    }
    let uid = cred_uid(cred, "ffs_realloccg");

    let error: Errno = 'error: {
        'nospace: {
            if uid != 0 && freespace(fs, fs.fs_minfree.get()) <= 0 {
                break 'nospace;
            }

            let bprev = ip.dip_db(lbprev as usize);

            if bprev == 0 {
                kprintf!(
                    "dev = 0x{:x}, bsize = {}, bprev = {}, fs = {}\n",
                    ip.i_dev.get(),
                    fs.fs_bsize.get(),
                    bprev,
                    fs.fsmnt()
                );
                panic(format_args!("ffs_realloccg: bad bprev"));
            }

            // Allocate the extra space in the buffer.
            if bpp.is_some() {
                let (b, r) = bread(ip.itov(), lbprev, fs.fs_bsize.get());
                bp = Some(b);
                if let Err(e) = r {
                    break 'error e;
                }
                buf_adjcnt(b, i64::from(osize));
            }

            if let Err(e) =
                ufs_quota_alloc_blocks(ip, btodb((nsize - osize) as usize) as Daddr, cred)
            {
                break 'error e;
            }

            quota_updated = btodb((nsize - osize) as usize) as Daddr;

            // Check for extension in the existing location.
            let cg = dtog(fs, bprev);
            let bno = ffs_fragextend(ip, cg, bprev, osize, nsize);
            if bno != 0 {
                ip.dip_set_blocks(ip.dip_blocks() + btodb((nsize - osize) as usize) as i64);
                ip.set_flag(IN_CHANGE | IN_UPDATE);
                if let (Some(out), Some(b)) = (bpp.as_deref_mut(), bp) {
                    if b.b_blkno.get() != fsbtodb(fs, bno) {
                        panic(format_args!("ffs_realloccg: bad blockno"));
                    }
                    if cfg!(feature = "diagnostic") && i64::from(nsize) > b.b_bufsize.get() {
                        panic(format_args!("ffs_realloccg: small buf"));
                    }
                    buf_adjcnt(b, i64::from(nsize));
                    b.set(B_DONE);
                    // SAFETY: the buffer is ours (busy from bread) and mapped.
                    (unsafe { b.data() })[osize as usize..nsize as usize].fill(0);
                    *out = Some(b);
                }
                if let Some(blknop) = blknop {
                    *blknop = bno;
                }
                return Ok(());
            }
            // Allocate a new disk location.
            if bpref >= fs.fs_size.get() {
                bpref = 0;
            }
            let request = match fs.fs_optim.get() {
                FS_OPTSPACE => {
                    // Allocate an exact sized fragment. Although this makes best use of
                    // space, we will waste time relocating it if the file continues to grow.
                    // If the fragmentation is less than half of the minimum free reserve, we
                    // choose to begin optimizing for time.
                    if !(fs.fs_minfree.get() < 5
                        || fs.fs_cstotal.cs_nffree.get()
                            > fs.fs_dsize.get() * i64::from(fs.fs_minfree.get()) / (2 * 100))
                    {
                        fs.fs_optim.set(FS_OPTTIME);
                    }
                    nsize
                }
                FS_OPTTIME => {
                    // At this point we have discovered a file that is trying to grow a small
                    // fragment to a larger fragment. To save time, we allocate a full sized
                    // block, then free the unused portion. If the file continues to grow,
                    // the `ffs_fragextend' call above will be able to grow it in place
                    // without further copying. If aberrant programs cause disk fragmentation
                    // to grow within 2% of the free reserve, we choose to begin optimizing
                    // for space.
                    if fs.fs_cstotal.cs_nffree.get()
                        >= fs.fs_dsize.get() * i64::from(fs.fs_minfree.get() - 2) / 100
                    {
                        fs.fs_optim.set(FS_OPTSPACE);
                    }
                    fs.fs_bsize.get()
                }
                optim => {
                    kprintf!(
                        "dev = 0x{:x}, optim = {}, fs = {}\n",
                        ip.i_dev.get(),
                        optim,
                        fs.fsmnt()
                    );
                    panic(format_args!("ffs_realloccg: bad optim"));
                }
            };
            let bno = ffs_hashalloc(ip, cg, bpref, request, ffs_alloccg);
            if bno <= 0 {
                break 'nospace;
            }

            let _ = uvm_vnp_uncache(ip.itov());
            ffs_blkfree(ip, bprev, i64::from(osize));
            if nsize < request {
                ffs_blkfree(
                    ip,
                    bno + numfrags(fs, i64::from(nsize)),
                    i64::from(request - nsize),
                );
            }
            ip.dip_set_blocks(ip.dip_blocks() + btodb((nsize - osize) as usize) as i64);
            ip.set_flag(IN_CHANGE | IN_UPDATE);
            if let (Some(out), Some(b)) = (bpp, bp) {
                b.b_blkno.set(fsbtodb(fs, bno));
                if cfg!(feature = "diagnostic") && i64::from(nsize) > b.b_bufsize.get() {
                    panic(format_args!("ffs_realloccg: small buf 2"));
                }
                buf_adjcnt(b, i64::from(nsize));
                b.set(B_DONE);
                // SAFETY: the buffer is ours (busy from bread) and mapped.
                (unsafe { b.data() })[osize as usize..nsize as usize].fill(0);
                *out = Some(b);
            }
            if let Some(blknop) = blknop {
                *blknop = bno;
            }
            return Ok(());
        }

        // nospace:
        if fserr_ratecheck(&FFS_REALLOCCG_FSFULL_LAST) {
            ffs_fserr(fs, uid, "file system full");
            // uprintf("\n%s: write failed, file system is full\n", fs->fs_fsmnt)
            uprintf_unported();
        }
        Errno::ENOSPC
    };

    // error:
    if let Some(b) = bp {
        brelse(b);
    }

    // Restore user's disk quota because allocation failed.
    if quota_updated != 0 {
        let _ = ufs_quota_free_blocks(ip, quota_updated, cred);
    }

    Err(error)
}

/// `ffs_inode_alloc` (`iv_inode_alloc`): allocate an inode in the file system, and return
/// its vnode, referenced and locked.
///
/// If allocating a directory, use `ffs_dirpref` to select the inode. If allocating in a
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
pub fn ffs_inode_alloc(
    pip: &Inode,
    mode: Mode,
    cred: *const Ucred,
) -> Result<&'static Vnode, Errno> {
    let pvp = pip.itov();
    let fs = pip.fs();

    'noinodes: {
        if fs.fs_cstotal.cs_nifree.get() == 0 {
            break 'noinodes;
        }

        let mut ipref = if mode & IFMT == IFDIR {
            ffs_dirpref(pip)
        } else {
            pip.i_number.get()
        };
        if ipref >= fs.fs_ncg.get() * fs.fs_ipg.get() {
            ipref = 0;
        }
        let cg = ino_to_cg(fs, ipref);

        // Track number of dirs created one after another in a same cg without intervening by
        // files.
        if mode & IFMT == IFDIR {
            if fs.contigdirs(cg) < 255 {
                fs.set_contigdirs(cg, fs.contigdirs(cg) + 1);
            }
        } else if fs.contigdirs(cg) > 0 {
            fs.set_contigdirs(cg, fs.contigdirs(cg) - 1);
        }
        let ino =
            ffs_hashalloc(pip, cg, Daddr::from(ipref), mode as i32, ffs_nodealloccg) as Ufsino;
        if ino == 0 {
            break 'noinodes;
        }
        let Some(mp) = pvp.v_mount.get() else {
            panic(format_args!("ffs_inode_alloc: vnode without a mount"));
        };
        let vp = match VFS_VGET(mp, u64::from(ino)) {
            Ok(vp) => vp,
            Err(e) => {
                let _ = ffs_inode_free(pip, ino, mode);
                return Err(e);
            }
        };

        let ip = vtoi(vp);

        if ip.dip_mode() != 0 {
            kprintf!(
                "mode = 0{:o}, inum = {}, fs = {}\n",
                ip.dip_mode(),
                ip.i_number.get(),
                fs.fsmnt()
            );
            panic(format_args!("ffs_valloc: dup alloc"));
        }

        if ip.dip_blocks() != 0 {
            kprintf!(
                "free inode {}/{} had {} blocks\n",
                fs.fsmnt(),
                ino,
                ip.dip_blocks()
            );
            ip.dip_set_blocks(0);
        }

        ip.dip_set_flags(0);

        // Set up a new generation number for this inode. On wrap, we make sure to assign a
        // number != 0 and != UINT_MAX (the original value).
        if ip.dip_gen() != 0 {
            ip.dip_set_gen(ip.dip_gen().wrapping_add(1));
        }
        while ip.dip_gen() == 0 {
            ip.dip_set_gen(arc4random_uniform(u32::MAX));
        }

        return Ok(vp);
    }

    // noinodes:
    if fserr_ratecheck(&FFS_INODE_ALLOC_FSNOINODES_LAST) {
        ffs_fserr(fs, cred_uid(cred, "ffs_inode_alloc"), "out of inodes");
        // uprintf("\n%s: create/symlink failed, no inodes free\n", fs->fs_fsmnt)
        uprintf_unported();
    }
    Err(Errno::ENOSPC)
}

/// `ffs_dirpref`: find a cylinder group to place a directory.
///
/// The policy implemented by this algorithm is to allocate a directory inode in the same
/// cylinder group as its parent directory, but also to reserve space for its files inodes
/// and data. Restrict the number of directories which may be allocated one after another in
/// the same cylinder group without intervening allocation of files.
///
/// If we allocate a first level directory then force allocation in another cylinder group.
pub fn ffs_dirpref(pip: &Inode) -> Ufsino {
    let fs = pip.fs();
    let ncg = fs.fs_ncg.get();

    let avgifree = (fs.fs_cstotal.cs_nifree.get() / i64::from(ncg)) as u32;
    let avgbfree = (fs.fs_cstotal.cs_nbfree.get() / i64::from(ncg)) as u32;
    let avgndir = (fs.fs_cstotal.cs_ndir.get() / i64::from(ncg)) as u32;
    let cs = |cg: u32| fs.fs_cs(cg);

    let cg: u32 = 'end: {
        // Force allocation in another cg if creating a first level dir.
        let prefcg = if pip.itov().v_flag.get() & crate::sys::vnode::VROOT != 0 {
            let prefcg = arc4random_uniform(ncg);
            let mut mincg = prefcg;
            let mut minndir = fs.fs_ipg.get();
            for cg in (prefcg..ncg).chain(0..prefcg) {
                if (cs(cg).cs_ndir.get() as u32) < minndir
                    && cs(cg).cs_nifree.get() as u32 >= avgifree
                    && cs(cg).cs_nbfree.get() as u32 >= avgbfree
                {
                    mincg = cg;
                    minndir = cs(cg).cs_ndir.get() as u32;
                }
            }
            break 'end mincg;
        } else {
            ino_to_cg(fs, pip.i_number.get())
        };

        // Count various limits which used for optimal allocation of a directory inode.
        let maxndir = (avgndir + fs.fs_ipg.get() / 16).min(fs.fs_ipg.get());
        let minifree = (avgifree - avgifree / 4).max(1);
        let minbfree = (avgbfree - avgbfree / 4).max(1);

        let cgsize = (fs.fs_fsize.get() as u32).wrapping_mul(fs.fs_fpg.get() as u32);
        let mut dirsize = fs.fs_avgfilesize.get().wrapping_mul(fs.fs_avgfpdir.get());
        let curdirsize = cgsize
            .wrapping_sub(avgbfree.wrapping_mul(fs.fs_bsize.get() as u32))
            .checked_div(avgndir)
            .unwrap_or(0);
        if dirsize < curdirsize {
            dirsize = curdirsize;
        }
        let mut maxcontigdirs = match avgbfree
            .wrapping_mul(fs.fs_bsize.get() as u32)
            .checked_div(dirsize)
        {
            Some(n) => n.min(255),
            None => 0, // dirsize overflowed
        };
        if fs.fs_avgfpdir.get() > 0 {
            maxcontigdirs = maxcontigdirs.min(fs.fs_ipg.get() / fs.fs_avgfpdir.get());
        }
        if maxcontigdirs == 0 {
            maxcontigdirs = 1;
        }

        // Limit number of dirs in one cg and reserve space for regular files, but only if
        // we have no deficit in inodes or space.
        //
        // We are trying to find a suitable cylinder group nearby our preferred cylinder
        // group to place a new directory. We scan from our preferred cylinder group forward
        // looking for a cylinder group that meets our criterion. If we get to the final
        // cylinder group and do not find anything, we start scanning forwards from the
        // beginning of the filesystem. While it might seem sensible to start scanning
        // backwards or even to alternate looking forward and backward, this approach fails
        // badly when the filesystem is nearly full. Specifically, we first search all the
        // areas that have no space and finally try the one preceding that. We repeat this on
        // every request and in the case of the final block end up searching the entire
        // filesystem. By jumping to the front of the filesystem, our future forward searches
        // always look in new cylinder groups so finds every possible block after one pass
        // over the filesystem.
        for cg in (prefcg..ncg).chain(0..prefcg) {
            if (cs(cg).cs_ndir.get() as u32) < maxndir
                && cs(cg).cs_nifree.get() as u32 >= minifree
                && cs(cg).cs_nbfree.get() as u32 >= minbfree
                && u32::from(fs.contigdirs(cg)) < maxcontigdirs
            {
                break 'end cg;
            }
        }
        // This is a backstop when we have deficit in space.
        for cg in (prefcg..ncg).chain(0..prefcg) {
            if cs(cg).cs_nifree.get() as u32 >= avgifree {
                break 'end cg;
            }
        }
        // The C falls out of its last loop with cg == prefcg.
        prefcg
    };
    // end:
    fs.fs_ipg.get() * cg
}

/// `ffs1_blkpref`: select the desired position for the next block in a file.
///
/// The file is logically divided into sections. The first section is composed of the direct
/// blocks. Each additional section contains `fs_maxbpg` blocks.
///
/// If no blocks have been allocated in the first section, the policy is to request a block
/// in the same cylinder group as the inode that describes the file. The first indirect is
/// allocated immediately following the last direct block and the data blocks for the first
/// indirect immediately follow it.
///
/// If no blocks have been allocated in any other section, the indirect block(s) are
/// allocated in the same cylinder group as its inode in an area reserved immediately
/// following the inode blocks. The policy for the data blocks is to place them in a cylinder
/// group with a greater than average number of free blocks. An appropriate cylinder group is
/// found by using a rotor that sweeps the cylinder groups. When a new group of blocks is
/// needed, the sweep begins in the cylinder group following the cylinder group from which
/// the previous allocation was made. The sweep continues until a cylinder group with greater
/// than the average number of free blocks is found. If the allocation is for the first block
/// in an indirect block, the information on the previous allocation is unavailable; here a
/// best guess is made based upon the logical block number being allocated.
pub fn ffs1_blkpref(
    ip: &Inode,
    lbn: Daddr,
    indx: i32,
    bap: Option<&dyn Fn(usize) -> Daddr>,
) -> Daddr {
    crate::kassert!(indx <= 0 || bap.is_some());
    let fs = ip.fs();
    // Allocation of indirect blocks is indicated by passing negative values in indx: -1 for
    // single indirect, -2 for double indirect, -3 for triple indirect. As noted below, we
    // attempt to allocate the first indirect inline with the file data. For all later
    // indirect blocks, the data is often allocated in other cylinder groups. However to
    // speed random file access and to speed up fsck, the filesystem reserves the first
    // fs_metaspace blocks (typically half of fs_minfree) of the data area of each cylinder
    // group to hold these later indirect blocks.
    let inocg = ino_to_cg(fs, ip.i_number.get());
    if indx < 0 {
        // Our preference for indirect blocks is the zone at the beginning of the inode's
        // cylinder group data area that we try to reserve for indirect blocks.
        let mut pref = cgmeta(fs, inocg) as u32;
        // If we are allocating the first indirect block, try to place it immediately
        // following the last direct block.
        if indx == -1 && lbn < NDADDR as Daddr + nindir(fs) && ip.dip_db(NDADDR - 1) != 0 {
            pref = (ip.dip_db(NDADDR - 1) + Daddr::from(fs.fs_frag.get())) as u32;
        }
        return Daddr::from(pref as i32);
    }
    // The C returns an int32_t.
    Daddr::from(blkpref_data(ip, fs, inocg, lbn, indx, bap, false) as i32)
}

/// `ffs2_blkpref`: same as above, for UFS2.
#[cfg(feature = "ffs2")]
pub fn ffs2_blkpref(
    ip: &Inode,
    lbn: Daddr,
    indx: i32,
    bap: Option<&dyn Fn(usize) -> Daddr>,
) -> Daddr {
    crate::kassert!(indx <= 0 || bap.is_some());
    let fs = ip.fs();
    // Allocation of indirect blocks is indicated by passing negative values in indx (see
    // ffs1_blkpref).
    let inocg = ino_to_cg(fs, ip.i_number.get());
    if indx < 0 {
        // Our preference for indirect blocks is the zone at the beginning of the inode's
        // cylinder group data area that we try to reserve for indirect blocks.
        let mut pref = cgmeta(fs, inocg);
        // If we are allocating the first indirect block, try to place it immediately
        // following the last direct block.
        if indx == -1 && lbn < NDADDR as Daddr + nindir(fs) && ip.dip_db(NDADDR - 1) != 0 {
            pref = ip.dip_db(NDADDR - 1) + Daddr::from(fs.fs_frag.get());
        }
        return pref;
    }
    blkpref_data(ip, fs, inocg, lbn, indx, bap, true)
}

/// The data block half of `ffs1_blkpref`/`ffs2_blkpref` (`indx >= 0`), which the two share
/// but for the start of the cylinder group sweep and where the sweep's answer lies.
fn blkpref_data(
    ip: &Inode,
    fs: &Fs,
    inocg: u32,
    lbn: Daddr,
    indx: i32,
    bap: Option<&dyn Fn(usize) -> Daddr>,
    ufs2: bool,
) -> Daddr {
    let bap_at = |i: i32| match bap {
        Some(f) => f(i as usize),
        None => panic(format_args!("ffs_blkpref: no block array")),
    };
    // If we are allocating the first data block in the first indirect block and the
    // indirect has been allocated in the data block area, try to place it immediately
    // following the indirect block.
    if lbn == NDADDR as Daddr {
        let pref = ip.dip_ib(0);
        if pref != 0 && pref >= cgdata(fs, inocg) && pref < cgbase(fs, inocg + 1) {
            return pref + Daddr::from(fs.fs_frag.get());
        }
    }
    // If we are the beginning of a file, or we have already allocated the maximum number of
    // blocks per cylinder group, or we do not have a block allocated immediately preceding
    // us, then we need to decide where to start allocating new blocks.
    if indx % fs.fs_maxbpg.get() == 0 || bap_at(indx - 1) == 0 {
        // If we are allocating a directory data block, we want to place it in the metadata
        // area.
        if ip.dip_mode() & IFMT == IFDIR {
            return cgmeta(fs, inocg);
        }
        // Until we fill all the direct and all the first indirect's blocks, we try to
        // allocate in the data area of the inode's cylinder group.
        if lbn < NDADDR as Daddr + nindir(fs) {
            return cgdata(fs, inocg);
        }
        // Find a cylinder with greater than average number of unused data blocks.
        let mut startcg = if indx == 0 || bap_at(indx - 1) == 0 {
            inocg + (lbn / Daddr::from(fs.fs_maxbpg.get())) as u32
        } else if ufs2 {
            dtog(fs, bap_at(indx - 1) + 1)
        } else {
            dtog(fs, bap_at(indx - 1)) + 1
        };
        let ncg = fs.fs_ncg.get();
        startcg %= ncg;
        let avgbfree = (fs.fs_cstotal.cs_nbfree.get() / i64::from(ncg)) as u32;

        if ufs2 {
            for cg in (startcg..ncg).chain(0..startcg) {
                if fs.fs_cs(cg).cs_nbfree.get() as u32 >= avgbfree {
                    return cgbase(fs, cg) + Daddr::from(fs.fs_frag.get());
                }
            }
        } else {
            for cg in (startcg..ncg).chain(0..=startcg) {
                if fs.fs_cs(cg).cs_nbfree.get() as u32 >= avgbfree {
                    fs.fs_cgrotor.set(cg as i32);
                    return cgdata(fs, cg);
                }
            }
        }
        return 0;
    }
    // Otherwise, we just always try to lay things out contiguously.
    bap_at(indx - 1) + Daddr::from(fs.fs_frag.get())
}

/// The allocator `ffs_hashalloc` tries in each cylinder group: the inode, the cylinder
/// group, the preference and the size (or the mode, for inodes); 0 when it found nothing.
pub type FfsAllocator = fn(&Inode, u32, Daddr, i32) -> Daddr;

/// `ffs_hashalloc`: implement the cylinder overflow algorithm.
///
/// The policy implemented by this algorithm is:
///   1) allocate the block in its requested cylinder group.
///   2) quadratically rehash on the cylinder group number.
///   3) brute force search for a free block.
pub fn ffs_hashalloc(
    ip: &Inode,
    mut cg: u32,
    pref: Daddr,
    size: i32,
    allocator: FfsAllocator,
) -> Daddr {
    let fs = ip.fs();
    let ncg = fs.fs_ncg.get();
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

/// `ffs_cgread`: the cylinder group block of `cg`, or `None` when it cannot be read or is
/// not one.
pub fn ffs_cgread(fs: &Fs, ip: &Inode, cg: u32) -> Option<&'static Buf> {
    let (bp, error) = bread(ip.i_devvp(), fsbtodb(fs, cgtod(fs, cg)), fs.fs_cgsize.get());
    if error.is_err() {
        brelse(bp);
        return None;
    }

    // SAFETY: the buffer is ours (busy from bread) and mapped; the view dies here.
    if !unsafe { CgBuf::new(bp) }.cg_chkmagic() {
        brelse(bp);
        return None;
    }

    Some(bp)
}

/// `cgp->cg_ffs2_time = cgp->cg_time = now.tv_sec`.
fn cg_settime(cgp: &CgBuf<'_>) {
    let now = nanotime();
    cgp.cg().cg_ffs2_time.set(now.tv_sec);
    cgp.cg().cg_time.set(now.tv_sec as i32);
}

/// `ffs_fragextend`: determine whether a fragment can be extended. Check to see if the
/// necessary fragments are available, and if they are, allocate them; `bprev`, or 0.
pub fn ffs_fragextend(ip: &Inode, cg: u32, bprev: Daddr, osize: i32, nsize: i32) -> Daddr {
    let fs = ip.fs();
    if i64::from(fs.fs_cs(cg).cs_nffree.get()) < numfrags(fs, i64::from(nsize - osize)) {
        return 0;
    }
    let frags = numfrags(fs, i64::from(nsize));
    let bbase = fragnum(fs, bprev);
    if bbase > fragnum(fs, bprev + frags - 1) {
        // cannot extend across a block boundary
        return 0;
    }

    let Some(bp) = ffs_cgread(fs, ip, cg) else {
        return 0;
    };

    // SAFETY: the buffer is ours (busy from ffs_cgread) and mapped, until bdwrite/brelse.
    let mut cgp = unsafe { CgBuf::new(bp) };
    cg_settime(&cgp);

    let bno = dtogd(fs, bprev);
    let ofrags = numfrags(fs, i64::from(osize));
    for i in ofrags..frags {
        if isclr(cgp.cg_blksfree(), (bno + i) as usize) {
            brelse(bp);
            return 0;
        }
    }
    // the current fragment can be extended
    // deduct the count on fragment being extended into
    // increase the count on the remaining fragment (if any)
    // allocate the extended piece
    let mut i = frags;
    while i < i64::from(fs.fs_frag.get()) - bbase {
        if isclr(cgp.cg_blksfree(), (bno + i) as usize) {
            break;
        }
        i += 1;
    }
    sub(&cgp.cg().cg_frsum[(i - ofrags) as usize], 1);
    if i != frags {
        add(&cgp.cg().cg_frsum[(i - frags) as usize], 1);
    }
    for i in ofrags..frags {
        clrbit(cgp.cg_blksfree(), (bno + i) as usize);
        sub(&cgp.cg().cg_cs.cs_nffree, 1);
        sub(&fs.fs_cstotal.cs_nffree, 1);
        sub(&fs.fs_cs(cg).cs_nffree, 1);
    }
    fs.fs_fmod.set(1);

    bdwrite(bp);
    bprev
}

/// `ffs_alloccg`: determine whether a block can be allocated. Check to see if a block of
/// the appropriate size is available, and if it is, allocate it.
pub fn ffs_alloccg(ip: &Inode, cg: u32, mut bpref: Daddr, size: i32) -> Daddr {
    let fs = ip.fs();
    if fs.fs_cs(cg).cs_nbfree.get() == 0 && size == fs.fs_bsize.get() {
        return 0;
    }

    let Some(bp) = ffs_cgread(fs, ip, cg) else {
        return 0;
    };

    // SAFETY: the buffer is ours (busy from ffs_cgread) and mapped, until bdwrite/brelse.
    let mut cgp = unsafe { CgBuf::new(bp) };
    if cgp.cg().cg_cs.cs_nbfree.get() == 0 && size == fs.fs_bsize.get() {
        brelse(bp);
        return 0;
    }

    cg_settime(&cgp);

    if size == fs.fs_bsize.get() {
        // allocate and return a complete data block
        let bno = ffs_alloccgblk(ip, &mut cgp, bpref);
        bdwrite(bp);
        return bno;
    }
    // check to see if any fragments are already available
    // allocsiz is the size which will be allocated, hacking it down to a smaller size if
    // necessary
    let frags = numfrags(fs, i64::from(size)) as i32;
    let mut allocsiz = frags;
    while allocsiz < fs.fs_frag.get() {
        if cgp.cg().cg_frsum[allocsiz as usize].get() != 0 {
            break;
        }
        allocsiz += 1;
    }
    if allocsiz == fs.fs_frag.get() {
        // no fragments were available, so a block will be allocated, and hacked up
        if cgp.cg().cg_cs.cs_nbfree.get() == 0 {
            brelse(bp);
            return 0;
        }
        let bno = ffs_alloccgblk(ip, &mut cgp, bpref);
        bpref = dtogd(fs, bno);
        for i in frags..fs.fs_frag.get() {
            setbit(cgp.cg_blksfree(), (bpref + Daddr::from(i)) as usize);
        }
        let i = fs.fs_frag.get() - frags;
        add(&cgp.cg().cg_cs.cs_nffree, i);
        add(&fs.fs_cstotal.cs_nffree, i64::from(i));
        add(&fs.fs_cs(cg).cs_nffree, i);
        fs.fs_fmod.set(1);
        add(&cgp.cg().cg_frsum[i as usize], 1);
        bdwrite(bp);
        return bno;
    }
    let bno = ffs_mapsearch(fs, &mut cgp, bpref, allocsiz);
    if bno < 0 {
        brelse(bp);
        return 0;
    }

    for i in 0..frags {
        clrbit(cgp.cg_blksfree(), (bno + Daddr::from(i)) as usize);
    }
    sub(&cgp.cg().cg_cs.cs_nffree, frags);
    sub(&fs.fs_cstotal.cs_nffree, i64::from(frags));
    sub(&fs.fs_cs(cg).cs_nffree, frags);
    fs.fs_fmod.set(1);
    sub(&cgp.cg().cg_frsum[allocsiz as usize], 1);
    if frags != allocsiz {
        add(&cgp.cg().cg_frsum[(allocsiz - frags) as usize], 1);
    }

    let blkno = cgbase(fs, cg) + bno;
    bdwrite(bp);
    blkno
}

/// `ffs_alloccgblk`: allocate a block in a cylinder group. Note that this routine only
/// allocates `fs_bsize` blocks; these blocks may be fragmented by the routine that allocates
/// them.
pub fn ffs_alloccgblk(ip: &Inode, cgp: &mut CgBuf<'_>, mut bpref: Daddr) -> Daddr {
    let fs = ip.fs();
    let cgx = cgp.cg().cg_cgx.get();

    if bpref == 0 {
        bpref = Daddr::from(cgp.cg().cg_rotor.get());
    } else {
        let cgbpref = dtog(fs, bpref);
        if cgbpref != cgx {
            // map bpref to correct zone in this cg
            if bpref < cgdata(fs, cgbpref) {
                bpref = cgmeta(fs, cgx);
            } else {
                bpref = cgdata(fs, cgx);
            }
        }
    }
    // If the requested block is available, use it.
    let mut bno = dtogd(fs, blknum(fs, bpref));
    if !ffs_isblock(fs, cgp.cg_blksfree(), fragstoblks(fs, bno)) {
        // Take the next available block in this cylinder group.
        bno = ffs_mapsearch(fs, cgp, bpref, fs.fs_frag.get());
        if bno < 0 {
            return 0;
        }

        // Update cg_rotor only if allocated from the data zone
        if bno >= dtogd(fs, cgdata(fs, cgx)) {
            cgp.cg().cg_rotor.set(bno as u32);
        }
    }

    // gotit:
    let blkno = fragstoblks(fs, bno);
    ffs_clrblock(fs, cgp.cg_blksfree(), blkno);
    ffs_clusteracct(fs, cgp, blkno, -1);
    sub(&cgp.cg().cg_cs.cs_nbfree, 1);
    sub(&fs.fs_cstotal.cs_nbfree, 1);
    sub(&fs.fs_cs(cgx).cs_nbfree, 1);

    if fs.fs_magic.get() != FS_UFS2_MAGIC {
        let cylno = cbtocylno(fs, bno);
        cgp.cg_blks_add(fs, cylno, cbtorpos(fs, bno), -1);
        cgp.cg_blktot_add(cylno, -1);
    }

    fs.fs_fmod.set(1);
    cgbase(fs, cgx) + bno
}

/// `ffs_nodealloccg`: inode allocation routine; the inode number, or 0.
pub fn ffs_nodealloccg(ip: &Inode, cg: u32, mut ipref: Daddr, mode: i32) -> Daddr {
    // For efficiency, before looking at the bitmaps for free inodes, check the counters kept
    // in the superblock cylinder group summaries, and in the cylinder group itself.
    let fs = ip.fs();
    if fs.fs_cs(cg).cs_nifree.get() == 0 {
        return 0;
    }

    let Some(bp) = ffs_cgread(fs, ip, cg) else {
        return 0;
    };

    // SAFETY: the buffer is ours (busy from ffs_cgread) and mapped, until bdwrite/brelse.
    let mut cgp = unsafe { CgBuf::new(bp) };
    if cgp.cg().cg_cs.cs_nifree.get() == 0 {
        brelse(bp);
        return 0;
    }

    // We are committed to the allocation from now on, so update the time on the cylinder
    // group.
    cg_settime(&cgp);

    'gotit: {
        // If there was a preferred location for the new inode, try to find it.
        if ipref != 0 {
            ipref %= Daddr::from(fs.fs_ipg.get());
            if isclr(cgp.cg_inosused(), ipref as usize) {
                break 'gotit; // inode is free, grab it.
            }
        }

        // Otherwise, look for the next available inode, starting at cg_irotor (the position
        // in the bitmap of the last used inode).
        let irotor = cgp.cg().cg_irotor.get();
        let mut start = (irotor / 8) as usize;
        let mut len = howmany((fs.fs_ipg.get() - irotor) as usize, 8);
        let mut loc = skpc(0xff, &cgp.cg_inosused()[start..start + len]);
        if loc == 0 {
            // If we didn't find a free inode in the upper part of the bitmap (from cg_irotor
            // to the end), then look at the bottom part (from 0 to cg_irotor).
            len = start + 1;
            start = 0;
            loc = skpc(0xff, &cgp.cg_inosused()[..len]);
            if loc == 0 {
                // If we failed again, then either the bitmap or the counters kept for the
                // cylinder group are wrong.
                kprintf!(
                    "cg = {}, irotor = {}, fs = {}\n",
                    cg,
                    cgp.cg().cg_irotor.get(),
                    fs.fsmnt()
                );
                panic(format_args!("ffs_nodealloccg: map corrupted"));
            }
        }

        // skpc() returns the position relative to the end
        let i = start + len - loc;

        // Okay, so now in 'i' we have the location in the bitmap of a byte holding a free
        // inode. Find the corresponding bit and set it, updating cg_irotor as well,
        // accordingly.
        let map = cgp.cg_inosused()[i];
        ipref = (i * 8) as Daddr;
        let mut bit = 1;
        while bit < 1 << 8 {
            if map & bit as u8 == 0 {
                cgp.cg().cg_irotor.set(ipref as u32);
                break 'gotit;
            }
            bit <<= 1;
            ipref += 1;
        }

        kprintf!("fs = {}\n", fs.fsmnt());
        panic(format_args!("ffs_nodealloccg: block not in map"));
    }

    // gotit:
    #[cfg(feature = "ffs2")]
    let mut ibp: Option<&'static Buf> = None;
    #[cfg(feature = "ffs2")]
    {
        // For FFS2, check if all inodes in this cylinder group have been used at least once.
        // If they haven't, and we are allocating an inode past the last allocated block of
        // inodes, read in a block and initialize all inodes in it.
        let c = cgp.cg();
        if fs.fs_magic.get() == FS_UFS2_MAGIC
            // Inode is beyond last initialized block of inodes?
            && ipref + Daddr::from(inopb(fs)) > Daddr::from(c.cg_initediblk.get())
            // Has any inode not been used at least once?
            && c.cg_initediblk.get() < c.cg_ffs2_niblk.get()
        {
            let b = loop {
                if let Some(b) = getblk(
                    ip.i_devvp(),
                    fsbtodb(
                        fs,
                        ino_to_fsba(fs, cg * fs.fs_ipg.get() + c.cg_initediblk.get()),
                    ),
                    fs.fs_bsize.get(),
                    0,
                    INFSLP,
                ) {
                    break b;
                }
            };

            // SAFETY: the buffer is ours (busy from getblk) and mapped.
            let data = unsafe { b.data() };
            data[..fs.fs_bsize.get() as usize].fill(0);

            // Give each inode a generation number
            for i in 0..inopb(fs) as usize {
                let off = i * size_of::<crate::ufs::ufs::dinode::Ufs2Dinode>()
                    + core::mem::offset_of!(crate::ufs::ufs::dinode::Ufs2Dinode, di_gen);
                let mut generation = 0i32;
                while generation == 0 {
                    generation = arc4random() as i32;
                }
                data[off..off + 4].copy_from_slice(&generation.to_ne_bytes());
            }

            // Update the counter of initialized inodes
            c.cg_initediblk.set(c.cg_initediblk.get() + inopb(fs));
            ibp = Some(b);
        }
    }

    setbit(cgp.cg_inosused(), ipref as usize);

    // Update the counters we keep on free inodes
    sub(&cgp.cg().cg_cs.cs_nifree, 1);
    sub(&fs.fs_cstotal.cs_nifree, 1);
    sub(&fs.fs_cs(cg).cs_nifree, 1);
    fs.fs_fmod.set(1); // file system was modified

    // Update the counters we keep on allocated directories
    if mode as Mode & IFMT == IFDIR {
        add(&cgp.cg().cg_cs.cs_ndir, 1);
        add(&fs.fs_cstotal.cs_ndir, 1);
        add(&fs.fs_cs(cg).cs_ndir, 1);
    }

    bdwrite(bp);

    #[cfg(feature = "ffs2")]
    if let Some(ibp) = ibp {
        bawrite(ibp);
    }

    // Return the allocated inode number
    Daddr::from(cg * fs.fs_ipg.get()) + ipref
}

/// `ffs_blkfree`: free a block or fragment.
///
/// The specified block or fragment is placed back in the free map. If a fragment is
/// deallocated, a possible block reassembly is checked.
pub fn ffs_blkfree(ip: &Inode, mut bno: Daddr, size: i64) {
    let fs = ip.fs();
    if size as u64 > fs.fs_bsize.get() as u64
        || fragoff(fs, size) != 0
        || fragnum(fs, bno) + numfrags(fs, size) > i64::from(fs.fs_frag.get())
    {
        kprintf!(
            "dev = 0x{:x}, bsize = {}, size = {}, fs = {}\n",
            ip.i_dev.get(),
            fs.fs_bsize.get(),
            size,
            fs.fsmnt()
        );
        panic(format_args!("ffs_blkfree: bad size"));
    }
    let cg = dtog(fs, bno);
    if i64::from(bno as u32) >= fs.fs_size.get() {
        kprintf!("bad block {}, ino {}\n", bno, ip.i_number.get());
        ffs_fserr(fs, ip.dip_uid(), "bad block");
        return;
    }
    let Some(bp) = ffs_cgread(fs, ip, cg) else {
        return;
    };

    // SAFETY: the buffer is ours (busy from ffs_cgread) and mapped, until bdwrite.
    let mut cgp = unsafe { CgBuf::new(bp) };
    cg_settime(&cgp);

    bno = dtogd(fs, bno);
    if size == i64::from(fs.fs_bsize.get()) {
        let blkno = fragstoblks(fs, bno);
        if !ffs_isfreeblock(fs, cgp.cg_blksfree(), blkno) {
            kprintf!(
                "dev = 0x{:x}, block = {}, fs = {}\n",
                ip.i_dev.get(),
                bno,
                fs.fsmnt()
            );
            panic(format_args!("ffs_blkfree: freeing free block"));
        }
        ffs_setblock(fs, cgp.cg_blksfree(), blkno);
        ffs_clusteracct(fs, &mut cgp, blkno, 1);
        add(&cgp.cg().cg_cs.cs_nbfree, 1);
        add(&fs.fs_cstotal.cs_nbfree, 1);
        add(&fs.fs_cs(cg).cs_nbfree, 1);

        if fs.fs_magic.get() != FS_UFS2_MAGIC {
            let i = cbtocylno(fs, bno);
            cgp.cg_blks_add(fs, i, cbtorpos(fs, bno), 1);
            cgp.cg_blktot_add(i, 1);
        }
    } else {
        let bbase = bno - fragnum(fs, bno);
        // decrement the counts associated with the old frags
        let blk = blkmap(fs, cgp.cg_blksfree(), bbase);
        ffs_fragacct(fs, blk, &cgp.cg().cg_frsum, -1);
        // deallocate the fragment
        let frags = numfrags(fs, size);
        let mut i = 0;
        while i < frags {
            if isset(cgp.cg_blksfree(), (bno + i) as usize) {
                kprintf!(
                    "dev = 0x{:x}, block = {}, fs = {}\n",
                    ip.i_dev.get(),
                    bno + i,
                    fs.fsmnt()
                );
                panic(format_args!("ffs_blkfree: freeing free frag"));
            }
            setbit(cgp.cg_blksfree(), (bno + i) as usize);
            i += 1;
        }
        add(&cgp.cg().cg_cs.cs_nffree, i as i32);
        add(&fs.fs_cstotal.cs_nffree, i);
        add(&fs.fs_cs(cg).cs_nffree, i as i32);
        // add back in counts associated with the new frags
        let blk = blkmap(fs, cgp.cg_blksfree(), bbase);
        ffs_fragacct(fs, blk, &cgp.cg().cg_frsum, 1);
        // if a complete block has been reassembled, account for it
        let blkno = fragstoblks(fs, bbase);
        if ffs_isblock(fs, cgp.cg_blksfree(), blkno) {
            sub(&cgp.cg().cg_cs.cs_nffree, fs.fs_frag.get());
            sub(&fs.fs_cstotal.cs_nffree, i64::from(fs.fs_frag.get()));
            sub(&fs.fs_cs(cg).cs_nffree, fs.fs_frag.get());
            ffs_clusteracct(fs, &mut cgp, blkno, 1);
            add(&cgp.cg().cg_cs.cs_nbfree, 1);
            add(&fs.fs_cstotal.cs_nbfree, 1);
            add(&fs.fs_cs(cg).cs_nbfree, 1);

            if fs.fs_magic.get() != FS_UFS2_MAGIC {
                let i = cbtocylno(fs, bbase);
                cgp.cg_blks_add(fs, i, cbtorpos(fs, bbase), 1);
                cgp.cg_blktot_add(i, 1);
            }
        }
    }
    fs.fs_fmod.set(1);
    bdwrite(bp);
}

/// `ffs_inode_free` (`iv_inode_free`).
pub fn ffs_inode_free(pip: &Inode, ino: Ufsino, mode: Mode) -> Result<(), Errno> {
    ffs_freefile(pip, ino, mode)
}

/// `ffs_freefile`: do the actual free operation. The specified inode is placed back in the
/// free map.
pub fn ffs_freefile(pip: &Inode, ino: Ufsino, mode: Mode) -> Result<(), Errno> {
    let fs = pip.fs();
    if ino >= fs.fs_ipg.get() * fs.fs_ncg.get() {
        panic(format_args!(
            "ffs_freefile: range: dev = 0x{:x}, ino = {}, fs = {}",
            pip.i_dev.get(),
            ino,
            fs.fsmnt()
        ));
    }

    let cg = ino_to_cg(fs, ino);
    let Some(bp) = ffs_cgread(fs, pip, cg) else {
        return Ok(());
    };

    // SAFETY: the buffer is ours (busy from ffs_cgread) and mapped, until bdwrite.
    let mut cgp = unsafe { CgBuf::new(bp) };
    cg_settime(&cgp);

    let ino = ino % fs.fs_ipg.get();
    if isclr(cgp.cg_inosused(), ino as usize) {
        kprintf!(
            "dev = 0x{:x}, ino = {}, fs = {}\n",
            pip.i_dev.get(),
            ino,
            fs.fsmnt()
        );
        if fs.fs_ronly.get() == 0 {
            panic(format_args!("ffs_freefile: freeing free inode"));
        }
    }
    clrbit(cgp.cg_inosused(), ino as usize);
    if ino < cgp.cg().cg_irotor.get() {
        cgp.cg().cg_irotor.set(ino);
    }
    add(&cgp.cg().cg_cs.cs_nifree, 1);
    add(&fs.fs_cstotal.cs_nifree, 1);
    add(&fs.fs_cs(cg).cs_nifree, 1);
    if mode & IFMT == IFDIR {
        sub(&cgp.cg().cg_cs.cs_ndir, 1);
        sub(&fs.fs_cstotal.cs_ndir, 1);
        sub(&fs.fs_cs(cg).cs_ndir, 1);
    }
    fs.fs_fmod.set(1);
    bdwrite(bp);
    Ok(())
}

/// `ffs_mapsearch`: find a block of the specified size in the specified cylinder group.
///
/// It is a panic if a request is made to find a block if none are available.
pub fn ffs_mapsearch(fs: &Fs, cgp: &mut CgBuf<'_>, bpref: Daddr, allocsiz: i32) -> Daddr {
    let frag = fs.fs_frag.get();
    let Some(tbl) = FRAGTBL[frag as usize] else {
        panic(format_args!("ffs_mapsearch: fs_frag {}", frag));
    };
    let mask = (1u32 << (allocsiz - 1 + (frag % 8))) as u8;

    // find the fragment by searching through the free block map for an appropriate bit
    // pattern
    let mut start = if bpref != 0 {
        (dtogd(fs, bpref) / 8) as usize
    } else {
        (cgp.cg().cg_frotor.get() / 8) as usize
    };
    let mut len = howmany(fs.fs_fpg.get() as usize, 8) - start;
    let mut loc = scanc(&cgp.cg_blksfree()[start..start + len], tbl, mask);
    if loc == 0 {
        len = start + 1;
        start = 0;
        loc = scanc(&cgp.cg_blksfree()[..len], tbl, mask);
        if loc == 0 {
            kprintf!("start = {}, len = {}, fs = {}\n", start, len, fs.fsmnt());
            panic(format_args!("ffs_alloccg: map corrupted"));
        }
    }
    let mut bno = ((start + len - loc) * 8) as Daddr;
    cgp.cg().cg_frotor.set(bno as u32);
    // found the byte in the map
    // sift through the bits to find the selected frag
    let end = bno + 8;
    while bno < end {
        let blk = blkmap(fs, cgp.cg_blksfree(), bno) << 1;
        let mut field = AROUND[allocsiz as usize];
        let mut subfield = INSIDE[allocsiz as usize];
        for pos in 0..=(frag - allocsiz) {
            if blk & field == subfield {
                return bno + Daddr::from(pos);
            }
            field <<= 1;
            subfield <<= 1;
        }
        bno += Daddr::from(frag);
    }
    kprintf!("bno = {}, fs = {}\n", bno, fs.fsmnt());
    panic(format_args!("ffs_alloccg: block not in map"));
}

/// `ffs_clusteracct`: update the cluster map because of an allocation or free.
///
/// `cnt == 1` means free; `cnt == -1` means allocating.
pub fn ffs_clusteracct(fs: &Fs, cgp: &mut CgBuf<'_>, blkno: Daddr, cnt: i32) {
    let contigsumsize = fs.fs_contigsumsize.get();
    if contigsumsize <= 0 {
        return;
    }
    let nclusterblks = i64::from(cgp.cg().cg_nclusterblks.get());
    // Allocate or clear the actual block.
    if cnt > 0 {
        setbit(cgp.cg_clustersfree(), blkno as usize);
    } else {
        clrbit(cgp.cg_clustersfree(), blkno as usize);
    }
    // Find the size of the cluster going forward.
    let start = blkno + 1;
    let mut end = start + i64::from(contigsumsize);
    if end >= nclusterblks {
        end = nclusterblks;
    }
    let mut i = start;
    {
        let freemapp = cgp.cg_clustersfree();
        while i < end {
            if !isset(freemapp, i as usize) {
                break;
            }
            i += 1;
        }
    }
    let forw = i - start;
    // Find the size of the cluster going backward.
    let start = blkno - 1;
    let mut end = start - i64::from(contigsumsize);
    if end < 0 {
        end = -1;
    }
    let mut i = start;
    {
        let freemapp = cgp.cg_clustersfree();
        while i > end {
            if !isset(freemapp, i as usize) {
                break;
            }
            i -= 1;
        }
    }
    let back = start - i;
    // Account for old cluster and the possibly new forward and back clusters.
    let mut i = back + forw + 1;
    if i > i64::from(contigsumsize) {
        i = i64::from(contigsumsize);
    }
    cgp.cg_clustersum_add(i as usize, cnt);
    if back > 0 {
        cgp.cg_clustersum_add(back as usize, -cnt);
    }
    if forw > 0 {
        cgp.cg_clustersum_add(forw as usize, -cnt);
    }
    // Update cluster summary information.
    let mut i = contigsumsize;
    while i > 0 {
        if cgp.cg_clustersum(i as usize) > 0 {
            break;
        }
        i -= 1;
    }
    let cgx = cgp.cg().cg_cgx.get();
    fs.set_maxcluster(cgx, i);
}
/* </CODE> */
