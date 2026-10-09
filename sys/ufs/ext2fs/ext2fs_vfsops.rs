/* <LICENSES> */
/*
 * Copyright (c) 1997 Manuel Bouyer.
 * Copyright (c) 1989, 1991, 1993, 1994
 *	The Regents of the University of California.  All rights reserved.
 *
 * Redistribution and use in source and binary forms, with or without
 * modification, are permitted provided that the following conditions
 * are met:
 * 1. Redistributions of source code must retain the above copyright
 *	notice, this list of conditions and the following disclaimer.
 * 2. Redistributions in binary form must reproduce the above copyright
 *	notice, this list of conditions and the following disclaimer in the
 *	documentation and/or other materials provided with the distribution.
 * 3. Neither the name of the University nor the names of its contributors
 *	may be used to endorse or promote products derived from this software
 *	without specific prior written permission.
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
 *	@(#)ffs_vfsops.c	8.14 (Berkeley) 11/28/94
 * Modified for ext2fs by Manuel Bouyer.
 */
/* </LICENSES> */

/* <CODE> */
//! The second extended file system's file-system-type operations: mounting
//! (`ext2fs_mountroot`, `ext2fs_mount`, `ext2fs_mountfs` with the super block checks of
//! `e2fs_sbcheck` and the in-core set-up of `e2fs_sbfill`), reloading, unmounting,
//! `statfs`, `sync`, the inode cache's `ext2fs_vget`, file handles, the write-back of the
//! super block and the group descriptors (`ext2fs_sbupdate`, `ext2fs_cgupdate`) and
//! `ext2fs_init`.
//!
//! Upstream: sys/ufs/ext2fs/ext2fs_vfsops.c @ 3ce1f3f79392
//!
//! ## Deviations
//! - `ext2fs_vfsops` has no `vfs_sysctl` (the C has none either); `vfs_quotactl` is the UFS
//!   layer's (`option QUOTA`), as in C.
//! - The in-core super block (`struct m_ext2fs`) is `malloc(M_ZERO)`ed and used as a zeroed
//!   [`MExt2fs`], which every member allows; the group descriptors are allocated with
//!   `M_ZERO` too (the C's `mallocarray` leaves them uninitialised past the last group, and
//!   they are read through Rust slices here).
//! - `e2fs_sbcheck` reads the little-endian super block from the buffer's bytes at the
//!   members' offsets (the C's `letoh16`/`letoh32` over the cast buffer).
//! - `ext2fs_mount` looks the device name up in a `Nameidata` of its own (`ndinit` builds one
//!   around the kernel copy of the name), as `ffs_mount` does; a mount that is not an update
//!   and has no arguments is `EINVAL` (the C reads through the NULL `args`). `swapdev` and
//!   `nblkdev` come from the machine's `conf.c`.
//! - `ext2fs_reload` releases the super block's buffer and the old group descriptors before
//!   `e2fs_sbfill` reads them again; the C keeps the buffer busy and loses the descriptors.
//! - `ext2fs_vget` takes its reference on the device vnode as soon as the inode points at the
//!   mount, as `ffs_vget` does, instead of after `ext2fs_vinit`: `ext2fs_reclaim` releases
//!   one, and an inode whose `vget` fails half-way is reclaimed too. The pool dinode is
//!   zeroed before `e2fs_iload` fills it, so the large-inode fields of a revision 0 file
//!   system (128-byte inodes) are zero, not left over.
//! - The `struct ext2fs_reload_args`/`struct ext2fs_sync_args` callbacks of
//!   `vfs_mount_foreach_vnode` are closures over those structures.
//! - `rootdev` is `sys/systm.rs`'s `ROOTDEV`, `swapdev` the machine's (`conf.c`);
//!   `rootvp`/`swapdev_vp` are `init_main.rs`'s.

use core::mem::offset_of;
use core::ptr::{self, NonNull};
use core::sync::atomic::Ordering;

use crate::kern::init_main::{rootvp, set_rootvp, set_swapdev_vp};
use crate::kern::kern_malloc::{free, malloc};
use crate::kern::kern_rwlock::rrw_init_flags;
use crate::kern::kern_tc::gettime;
use crate::kern::subr_disk::disk_map;
use crate::kern::subr_pool::{pool_get, pool_init};
use crate::kern::subr_prf::{Str, panic};
use crate::kern::vfs_bio::{bawrite, bread, brelse, bwrite, getblk};
use crate::kern::vfs_lookup::{namei, ndinit};
use crate::kern::vfs_subr::{
    MOUNTLIST, bdevvp, copy_statfs_info, getnewvnode, vcount, vflush, vfs_export,
    vfs_mount_foreach_vnode, vfs_mount_free, vfs_mountedon, vfs_rootmountalloc, vfs_unbusy, vget,
    vgonel, vinvalbuf, vput, vref, vrele,
};
use crate::kern::vfs_vnops::vn_lock;
use crate::kern::vfs_vops::{VOP_CLOSE, VOP_FSYNC, VOP_OPEN, VOP_UNLOCK};
use crate::kprintf;
use crate::machine::conf::{nblkdev, swapdev};
use crate::machine::copy::copyinstr;
use crate::machine::cpu::curproc;
use crate::machine::intr::{IPL_NONE, splbio, splx};
use crate::sys::buf::Buf;
use crate::sys::disk::DM_OPENBLCK;
use crate::sys::errno::Errno;
use crate::sys::fcntl::{FREAD, FWRITE};
use crate::sys::lock::{LK_EXCLUSIVE, LK_NOWAIT, LK_RETRY};
use crate::sys::malloc::{M_UFSMNT, M_WAITOK, M_ZERO};
use crate::sys::mount::{
    Fid, MNAMELEN, MNT_FORCE, MNT_LAZY, MNT_LOCAL, MNT_RDONLY, MNT_RELOAD, MNT_UPDATE, MNT_WAIT,
    MNT_WANTRDWR, Mount, Statfs, UfsArgs, VFS_VGET, Vfsconf, Vfsops,
};
use crate::sys::namei::{FOLLOW, LOOKUP, Nameidata, NiDirp};
use crate::sys::param::{DEV_BSIZE, dbtob, howmany};
use crate::sys::pool::{PR_WAITOK, PR_ZERO, Pool};
use crate::sys::proc::Proc;
use crate::sys::rwlock::{RWL_DUPOK, RWL_IS_VNODE};
use crate::sys::systm::{INFSLP, ROOTDEV};
use crate::sys::types::{Daddr, Ino, Off, major};
use crate::sys::ucred::{FSCRED, NOCRED, Ucred};
use crate::sys::vnode::{FORCECLOSE, V_SAVE, VBAD, VBLK, VNON, VT_EXT2FS, Vnode, WRITECLOSE};
use crate::ufs::ext2fs::ext2fs::{
    E2FS_ERRORS, E2FS_ISCLEAN, E2FS_MAGIC, E2FS_REV0, E2FS_REV1, EXT2_GD_SIZE,
    EXT2F_INCOMPAT_EXTENTS, EXT2F_INCOMPAT_RECOVER, EXT2F_INCOMPAT_SUPP, EXT2F_ROCOMPAT_HUGE_FILE,
    EXT2F_ROCOMPAT_LARGE_FILE, EXT2F_ROCOMPAT_SPARSE_SUPER, EXT2F_ROCOMPAT_SUPP,
    EXT4F_RO_INCOMPAT_SUPP, Ext2Gd, Ext2fs, INCOMPAT, LOG_MINBSIZE, MAXMNTLEN, MExt2fs, RO_COMPAT,
    SBLOCK, SBOFF, SBSIZE, cg_has_sb, e2fs_cgload, e2fs_cgsave, e2fs_sbload, e2fs_sbsave, fsbtodb,
    ino_to_fsba, ino_to_fsbo, nindir,
};
use crate::ufs::ext2fs::ext2fs_alloc::next_gennumber;
use crate::ufs::ext2fs::ext2fs_dinode::{
    EXT2_FIRSTINO, EXT2_MAXSYMLINKLEN, EXT2_ROOTINO, Ext2fsDinode, e2fs_iload,
};
use crate::ufs::ext2fs::ext2fs_inode::ext2fs_setsize;
use crate::ufs::ext2fs::ext2fs_subr::ext2fs_vinit;
use crate::ufs::ext2fs::ext2fs_vnops::EXT2FS_VOPS;
use crate::ufs::ufs::dinode::Ufsino;
use crate::ufs::ufs::dir::MAXNAMLEN;
use crate::ufs::ufs::inode::{IN_ACCESS, IN_CHANGE, IN_MODIFIED, IN_UPDATE, Inode, Ufid, vtoi};
use crate::ufs::ufs::quota::ufs_quotactl;
use crate::ufs::ufs::ufs_ihash::{ufs_ihashget, ufs_ihashins};
use crate::ufs::ufs::ufs_vfsops::{ufs_check_export, ufs_init, ufs_root, ufs_start};
use crate::ufs::ufs::ufsmount::{UM_EXT2FS, Ufsmount, vfstoufs};

/// The super block members `e2fs_sbfill` computes from.
struct SbScalars {
    bcount: u32,
    first_dblock: u32,
    bpg: u32,
    log_bsize: u32,
    log_fsize: u32,
    ipg: u32,
    rev: u32,
    rocompat: u32,
    incompat: u32,
}

/// `ext2fs_vfsops`.
pub static EXT2FS_VFSOPS: Vfsops = Vfsops {
    vfs_mount: ext2fs_mount,
    vfs_start: ufs_start,
    vfs_unmount: ext2fs_unmount,
    vfs_root: ufs_root,
    vfs_quotactl: ufs_quotactl,
    vfs_statfs: ext2fs_statfs,
    vfs_sync: ext2fs_sync,
    vfs_vget: ext2fs_vget,
    vfs_fhtovp: ext2fs_fhtovp,
    vfs_vptofh: ext2fs_vptofh,
    vfs_init: Some(ext2fs_init),
    vfs_sysctl: None,
    vfs_checkexp: ufs_check_export,
};

/// `ext2fs_inode_pool`: memory pool for inodes.
pub static EXT2FS_INODE_POOL: Pool = Pool::new();
/// `ext2fs_dinode_pool`: memory pool for dinodes.
pub static EXT2FS_DINODE_POOL: Pool = Pool::new();

/// `ext2fs_init` (`vfs_init`): the inode and dinode pools, then the UFS layer.
pub fn ext2fs_init(vfsp: &'static Vfsconf) -> Result<(), Errno> {
    pool_init(
        &EXT2FS_INODE_POOL,
        size_of::<Inode>(),
        0,
        IPL_NONE,
        PR_WAITOK,
        "ext2inopl",
        None,
    );
    pool_init(
        &EXT2FS_DINODE_POOL,
        size_of::<Ext2fsDinode>(),
        0,
        IPL_NONE,
        PR_WAITOK,
        "ext2dinopl",
        None,
    );

    ufs_init(vfsp)
}

/// `memset(dst, 0, sizeof(dst)); strlcpy(dst, src, sizeof(dst))` over a byte array.
fn strlcpy_zero(dst: &mut [u8], src: &[u8]) {
    dst.fill(0);
    let src = src.split(|&c| c == 0).next().unwrap_or(&[]);
    let n = src.len().min(dst.len().saturating_sub(1));
    dst[..n].copy_from_slice(&src[..n]);
}

/// `memset(fs->e2fs_fsmnt, 0, ...); strlcpy(fs->e2fs_fsmnt, name, ...)`.
fn e2fs_set_fsmnt(fs: &MExt2fs, name: &[u8]) {
    let mut m = [0u8; MAXMNTLEN];
    strlcpy_zero(&mut m, name);
    fs.e2fs_fsmnt.set(m);
}

/// `memset(fs->e2fs.e2fs_fsmnt, 0, ...); strlcpy(fs->e2fs.e2fs_fsmnt, name, ...)`: the "last
/// mounted on" name in the super block, kept by revision 1 file systems.
fn e2fs_set_sb_fsmnt(fs: &MExt2fs, name: &[u8]) {
    fs.with_e2fs_mut(|e| strlcpy_zero(&mut e.e2fs_fsmnt, name));
}

/// `ext2fs_mountroot`: called by `main()` when ext2fs is going to be mounted as root.
pub fn ext2fs_mountroot() -> Result<(), Errno> {
    let Some(p) = curproc() else {
        panic(format_args!("ext2fs_mountroot: no curproc"));
    };

    // Get vnodes for swapdev and rootdev.
    set_swapdev_vp(None);
    let rootdev = ROOTDEV.load(Ordering::Relaxed);
    let rvp = match bdevvp(swapdev()).and_then(|svp| {
        set_swapdev_vp(svp);
        bdevvp(rootdev)
    }) {
        Ok(Some(rvp)) => rvp,
        Ok(None) | Err(_) => panic(format_args!("ext2fs_mountroot: can't setup bdevvp's")),
    };
    set_rootvp(Some(rvp));

    let mp = match vfs_rootmountalloc(b"ext2fs", b"root_device") {
        Ok(mp) => mp,
        Err(e) => {
            vrele(rvp);
            return Err(e);
        }
    };

    if let Err(e) = ext2fs_mountfs(rvp, mp, p) {
        vfs_unbusy(mp);
        vfs_mount_free(mp);
        vrele(rvp);
        return Err(e);
    }

    // SAFETY: a new mount on no list, under the kernel lock.
    unsafe { MOUNTLIST.0.insert_tail(mp) };
    let ump = vfstoufs(mp);
    let fs = ump.e2fs();
    let (name, len) = mp.mntonname();
    e2fs_set_fsmnt(fs, &name[..len]);
    if fs.e2fs_rev() > E2FS_REV0 {
        e2fs_set_sb_fsmnt(fs, &name[..len]);
    }
    let mut st = mp.mnt_stat.get();
    let _ = ext2fs_statfs(mp, &mut st, p);
    mp.mnt_stat.set(st);
    vfs_unbusy(mp);
    crate::kern::kern_time::inittodr(i64::from(fs.e2fs_wtime()));
    Ok(())
}

/// `ext2fs_mount` (`vfs_mount`): mount system call. `data` is the kernel copy of the user's
/// `struct ufs_args` (empty for the C's NULL).
pub fn ext2fs_mount(
    mp: &'static Mount,
    path: &[u8],
    data: &mut [u8],
    ndp: &mut Nameidata<'_>,
    p: &Proc,
) -> Result<(), Errno> {
    let args = UfsArgs::from_bytes(data);
    let mut ump: Option<&'static Ufsmount> = None;
    let mut fname = [0u8; MNAMELEN];
    let mut fspec = [0u8; MNAMELEN];

    // If updating, check whether changing from read-only to read/write; if there is no
    // device name, that's all we do.
    if mp.mnt_flag.get() & MNT_UPDATE != 0 {
        let u = vfstoufs(mp);
        ump = Some(u);
        let fs = u.e2fs();
        if fs.e2fs_ronly.get() == 0 && mp.mnt_flag.get() & MNT_RDONLY != 0 {
            let mut flags = WRITECLOSE;
            if mp.mnt_flag.get() & MNT_FORCE != 0 {
                flags |= FORCECLOSE;
            }
            let error = ext2fs_flushfiles(mp, flags, p);
            if error.is_ok()
                && ext2fs_cgupdate(u, MNT_WAIT).is_ok()
                && fs.e2fs_state() & E2FS_ERRORS == 0
            {
                fs.set_e2fs_state(E2FS_ISCLEAN);
                let _ = ext2fs_sbupdate(u, MNT_WAIT);
            }
            error?;
            fs.e2fs_ronly.set(1);
        }
        if mp.mnt_flag.get() & MNT_RELOAD != 0 {
            ext2fs_reload(mp, ndp.ni_cnd.cn_cred, p)?;
        }
        if fs.e2fs_ronly.get() != 0 && mp.mnt_flag.get() & MNT_WANTRDWR != 0 {
            fs.e2fs_ronly.set(0);
            if fs.e2fs_state() == E2FS_ISCLEAN {
                fs.set_e2fs_state(0);
            } else {
                fs.set_e2fs_state(E2FS_ERRORS);
            }
            fs.e2fs_fmod.set(1);
        }
        match args {
            // Process export requests.
            Some(a) if a.fspec == 0 => {
                return vfs_export(mp, &u.um_export, &a.export_info);
            }
            None => return Ok(()), // success
            Some(_) => {}
        }
    }
    // Not an update, or updating the name: look up the name and verify that it refers to a
    // sensible block device.
    let Some(a) = args else {
        return Err(Errno::EINVAL);
    };
    copyinstr(a.fspec, &mut fspec)?;

    if !disk_map(&fspec, &mut fname, DM_OPENBLCK) {
        fname = fspec;
    }

    let flen = fname.iter().position(|&c| c == 0).unwrap_or(MNAMELEN);
    let mut nd = ndinit(LOOKUP, FOLLOW, NiDirp::Sys(&fname[..flen]), p);
    namei(&mut nd)?;
    let Some(devvp) = nd.ni_vp else {
        return Err(Errno::ENOENT);
    };

    let error: Errno = 'error_devvp: {
        if devvp.v_type.get() != VBLK {
            break 'error_devvp Errno::ENOTBLK;
        }
        if major(devvp.v_rdev()) >= nblkdev() {
            break 'error_devvp Errno::ENXIO;
        }
        if mp.mnt_flag.get() & MNT_UPDATE == 0 {
            if let Err(e) = ext2fs_mountfs(devvp, mp, p) {
                break 'error_devvp e;
            }
        } else {
            let Some(u) = ump else {
                panic(format_args!("ext2fs_mount: update without ufsmount"));
            };
            if !ptr::eq(devvp, u.devvp()) {
                break 'error_devvp Errno::EINVAL; // XXX needs translation
            }
            vrele(devvp);
        }
        let u = vfstoufs(mp);
        let fs = u.e2fs();

        e2fs_set_fsmnt(fs, path);
        if fs.e2fs_rev() > E2FS_REV0 {
            let (name, len) = mp.mntonname();
            e2fs_set_sb_fsmnt(fs, &name[..len]);
        }
        let fsmnt = fs.fsmnt();
        mp.update_stat(|sp| {
            sp.f_mntonname.copy_from_slice(&fsmnt[..MNAMELEN]);
            strlcpy_zero(&mut sp.f_mntfromname, &fname);
            strlcpy_zero(&mut sp.f_mntfromspec, &fspec);
            sp.mount_info.__align[..UfsArgs::SIZE].copy_from_slice(&data[..UfsArgs::SIZE]);
        });

        if fs.e2fs_fmod.get() != 0 {
            // XXX
            fs.e2fs_fmod.set(0);
            if fs.e2fs_state() == 0 {
                fs.set_e2fs_wtime(gettime() as u32);
            } else {
                kprintf!(
                    "{}: file system not clean; please fsck(8)\n",
                    Str(&mp.mnt_stat.get().f_mntfromname)
                );
            }
            let _ = ext2fs_cgupdate(u, MNT_WAIT);
        }

        return Ok(());
    };

    // error_devvp: error with devvp held.
    vrele(devvp);
    Err(error)
}

/// `ext2fs_reload_vnode`: steps 4 to 6 of `ext2fs_reload` for one vnode.
fn ext2fs_reload_vnode(
    vp: &'static Vnode,
    fs: &MExt2fs,
    p: &Proc,
    cred: *const Ucred,
    devvp: &'static Vnode,
) -> Result<(), Errno> {
    // Step 4: invalidate all inactive vnodes.
    if vp.v_usecount.get() == 0 {
        vgonel(vp, Some(p));
        return Ok(());
    }

    // Step 5: invalidate all cached file data.
    if vget(vp, LK_EXCLUSIVE).is_err() {
        return Ok(());
    }

    if vinvalbuf(vp, 0, cred, Some(p), 0, INFSLP).is_err() {
        panic(format_args!("ext2fs_reload: dirty2"));
    }
    // Step 6: re-read inode data for all active vnodes.
    let ip = vtoi(vp);
    let (bp, error) = bread(
        devvp,
        fsbtodb(fs, Daddr::from(ino_to_fsba(fs, ip.i_number.get()))),
        fs.e2fs_bsize.get(),
    );
    if let Err(e) = error {
        // The C returns without releasing the buffer.
        brelse(bp);
        vput(vp);
        return Err(e);
    }
    {
        // SAFETY: the buffer is ours (busy from bread) and mapped; the slice dies before it
        // is released.
        let data = unsafe { bp.data() };
        let off = ino_to_fsbo(fs, ip.i_number.get()) as usize * fs.dinode_size();
        ip.with_e2din(|d| e2fs_iload(fs, &data[off..], d));
    }
    brelse(bp);
    vput(vp);
    Ok(())
}

/// `ext2fs_maxfilesize`: the largest file the block pointers can map, and the disk
/// addresses can reach.
fn ext2fs_maxfilesize(fs: &MExt2fs) -> Off {
    let huge = fs.e2fs_features_rocompat() & EXT2F_ROCOMPAT_HUGE_FILE != 0;
    let bsize = Off::from(fs.e2fs_bsize.get());
    let b = bsize / 4;

    let physically = dbtob(if huge {
        (1usize << 48) - 1
    } else {
        u32::MAX as usize
    }) as Off;
    let logically = (12 + b + b * b + b * b * b) * bsize;

    logically.min(physically)
}

/// The group descriptor area of `fs`: `e2fs_ngdb` blocks of descriptors from `e2fs_gd`
/// (more than `e2fs_ncg` descriptors: the last block is padded).
///
/// # Safety
///
/// `e2fs_gd` is the area `e2fs_sbfill` allocated for the current `e2fs_ngdb` and
/// `e2fs_bsize`, and nothing else reaches the descriptors while the slice lives (the
/// accessors of `MExt2fs` copy and do not nest).
unsafe fn e2fs_gd_area<'a>(fs: &MExt2fs) -> &'a mut [Ext2Gd] {
    let n = fs.e2fs_ngdb.get() as usize * fs.e2fs_bsize.get() as usize / EXT2_GD_SIZE;
    // SAFETY: the caller's contract; the area is `ngdb * bsize` zeroed bytes from malloc,
    // aligned for `Ext2Gd`, whose every bit pattern is valid.
    unsafe { core::slice::from_raw_parts_mut(fs.e2fs_gd.get(), n) }
}

/// `e2fs_sbfill`: compute the in-memory values of the super block just loaded into `fs`,
/// and load the group descriptors.
fn e2fs_sbfill(devvp: &'static Vnode, fs: &MExt2fs) -> Result<(), Errno> {
    // XXX assume hardware block size == 512
    let e = sb_scalars(fs);
    fs.e2fs_ncg.set(howmany(
        e.bcount.wrapping_sub(e.first_dblock) as usize,
        e.bpg as usize,
    ) as i32);
    fs.e2fs_fsbtodb.set(e.log_bsize as i32 + 1);
    fs.e2fs_bsize.set(1024 << e.log_bsize);
    fs.e2fs_bshift.set((LOG_MINBSIZE + e.log_bsize) as i32);
    fs.e2fs_fsize.set(1024 << e.log_fsize);

    fs.e2fs_qbmask.set(i64::from(fs.e2fs_bsize.get() - 1));
    fs.e2fs_bmask.set(!(fs.e2fs_bsize.get() - 1));

    fs.e2fs_ipb
        .set(fs.e2fs_bsize.get() / fs.dinode_size() as i32);
    fs.e2fs_itpg.set((e.ipg / fs.e2fs_ipb.get() as u32) as i32);

    // Re-read group descriptors from the disk.
    let bsize = fs.e2fs_bsize.get();
    fs.e2fs_ngdb
        .set(howmany(fs.e2fs_ncg.get() as usize, bsize as usize / EXT2_GD_SIZE) as i32);
    let gdescs_space = fs.e2fs_ngdb.get() as usize * bsize as usize;
    let Some(gd) = malloc(gdescs_space, M_UFSMNT, M_WAITOK | M_ZERO) else {
        panic(format_args!("e2fs_sbfill: no memory"));
    };
    fs.e2fs_gd.set(gd.as_ptr().cast::<Ext2Gd>());

    let per_block = bsize as usize / EXT2_GD_SIZE;
    for i in 0..fs.e2fs_ngdb.get() as usize {
        let dblk = Daddr::from(i32::from(bsize <= 1024)) + i as Daddr + 1;
        let gdesc = i * per_block;

        let (bp, error) = bread(devvp, fsbtodb(fs, dblk), bsize);
        if let Err(err) = error {
            free(gd, M_UFSMNT, gdescs_space);
            fs.e2fs_gd.set(ptr::null_mut());
            brelse(bp);
            return Err(err);
        }

        {
            // SAFETY: the area was just allocated for these `ngdb` blocks; nothing else
            // reaches it yet.
            let area = unsafe { e2fs_gd_area(fs) };
            // SAFETY: the buffer is ours (busy from bread) and mapped.
            let data = unsafe { bp.data() };
            e2fs_cgload(data, &mut area[gdesc..gdesc + per_block], bsize as usize);
        }
        brelse(bp);
    }

    if e.rocompat & EXT2F_ROCOMPAT_LARGE_FILE == 0 || e.rev == E2FS_REV0 {
        fs.e2fs_maxfilesize.set(Off::from(i32::MAX));
    } else {
        fs.e2fs_maxfilesize.set(ext2fs_maxfilesize(fs));
    }

    if e.incompat & EXT2F_INCOMPAT_EXTENTS != 0 {
        fs.e2fs_maxfilesize.set(fs.e2fs_maxfilesize.get() * 4);
    }

    Ok(())
}

/// The super block members `e2fs_sbfill` computes from, without copying the 1024 bytes.
fn sb_scalars(fs: &MExt2fs) -> SbScalars {
    fs.with_e2fs(|e| SbScalars {
        bcount: e.e2fs_bcount,
        first_dblock: e.e2fs_first_dblock,
        bpg: e.e2fs_bpg,
        log_bsize: e.e2fs_log_bsize,
        log_fsize: e.e2fs_log_fsize,
        ipg: e.e2fs_ipg,
        rev: e.e2fs_rev,
        rocompat: e.e2fs_features_rocompat,
        incompat: e.e2fs_features_incompat,
    })
}

/// `ext2fs_reload`: reload all incore data for a filesystem (used after running fsck on
/// the root filesystem and finding things to fix). The filesystem must be mounted
/// read-only.
///
/// Things to do to update the mount:
///  1) invalidate all cached meta-data.
///  2) re-read superblock from disk.
///  3) re-read summary information from disk.
///  4) invalidate all inactive vnodes.
///  5) invalidate all cached file data.
///  6) re-read inode data for all active vnodes.
pub fn ext2fs_reload(mountp: &'static Mount, cred: *const Ucred, p: &Proc) -> Result<(), Errno> {
    if mountp.mnt_flag.get() & MNT_RDONLY == 0 {
        return Err(Errno::EINVAL);
    }
    // Step 1: invalidate all cached meta-data.
    let ump = vfstoufs(mountp);
    let devvp = ump.devvp();
    let _ = vn_lock(devvp, LK_EXCLUSIVE | LK_RETRY);
    let error = vinvalbuf(devvp, 0, cred, Some(p), 0, INFSLP);
    let _ = VOP_UNLOCK(devvp);
    if error.is_err() {
        panic(format_args!("ext2fs_reload: dirty1"));
    }

    // Step 2: re-read superblock from disk.
    let (bp, error) = bread(devvp, SBOFF / DEV_BSIZE as Off, SBSIZE as i32);
    if let Err(e) = error {
        brelse(bp);
        return Err(e);
    }
    {
        // SAFETY: the buffer is ours (busy from bread) and mapped, SBSIZE bytes.
        let newfs = unsafe { bp.data() };
        if let Err(e) = e2fs_sbcheck(newfs, mountp.mnt_flag.get() & MNT_RDONLY != 0) {
            brelse(bp);
            return Err(e);
        }

        let fs = ump.e2fs();
        // Copy in the new superblock, compute in-memory values and load group
        // descriptors.
        fs.with_e2fs_mut(|e| e2fs_sbload(newfs, e));
    }
    brelse(bp);
    let fs = ump.e2fs();
    if let Some(gd) = NonNull::new(fs.e2fs_gd.get().cast::<u8>()) {
        let gdescs_space = fs.e2fs_ngdb.get() as usize * fs.e2fs_bsize.get() as usize;
        free(gd, M_UFSMNT, gdescs_space);
        fs.e2fs_gd.set(ptr::null_mut());
    }
    e2fs_sbfill(devvp, fs)?;

    vfs_mount_foreach_vnode(mountp, &mut |vp| {
        ext2fs_reload_vnode(vp, fs, p, cred, devvp)
    })
}

/// `ext2fs_mountfs`: common code for mount and mountroot.
pub fn ext2fs_mountfs(devvp: &'static Vnode, mp: &'static Mount, p: &Proc) -> Result<(), Errno> {
    let dev = devvp.v_rdev();
    let cred = p.p_ucred.get();
    // Disallow multiple mounts of the same device. Disallow mounting of a device that is
    // currently in use (except for root, which might share swap device for miniroot).
    // Flush out any old buffers remaining from a previous use.
    vfs_mountedon(devvp)?;
    if vcount(devvp) > 1 && !rootvp().is_some_and(|r| ptr::eq(r, devvp)) {
        return Err(Errno::EBUSY);
    }
    let _ = vn_lock(devvp, LK_EXCLUSIVE | LK_RETRY);
    let error = vinvalbuf(devvp, V_SAVE, cred, Some(p), 0, INFSLP);
    let _ = VOP_UNLOCK(devvp);
    error?;

    let ronly = mp.mnt_flag.get() & MNT_RDONLY != 0;
    let omode = if ronly { FREAD } else { FREAD | FWRITE };
    VOP_OPEN(devvp, omode, FSCRED, p)?;

    let bp: Option<&'static Buf>;
    let mut ump: Option<&'static Ufsmount> = None;

    let error: Errno = 'out: {
        // Read the superblock from disk.
        let (b, error) = bread(devvp, SBOFF / DEV_BSIZE as Off, SBSIZE as i32);
        bp = Some(b);
        if let Err(e) = error {
            break 'out e;
        }
        // SAFETY: the buffer is ours (busy from bread) and mapped, SBSIZE bytes; the slice
        // dies before the buffer is released.
        let sb = unsafe { b.data() };
        if let Err(e) = e2fs_sbcheck(sb, ronly) {
            break 'out e;
        }

        let Some(um) = malloc(size_of::<Ufsmount>(), M_UFSMNT, M_WAITOK | M_ZERO) else {
            panic(format_args!("ext2fs_mountfs: no memory"));
        };
        let um = um.cast::<Ufsmount>();
        // SAFETY: a fresh allocation of a `Ufsmount`'s size from malloc, aligned for it.
        unsafe { ptr::write(um.as_ptr(), Ufsmount::new()) };
        // SAFETY: just initialised; freed only by ext2fs_unmount or the error path below.
        let u: &'static Ufsmount = unsafe { &*um.as_ptr() };
        ump = Some(u);
        let Some(fsmem) = malloc(size_of::<MExt2fs>(), M_UFSMNT, M_WAITOK | M_ZERO) else {
            panic(format_args!("ext2fs_mountfs: no memory"));
        };
        // SAFETY: a fresh zeroed allocation of an `MExt2fs`'s size from malloc, aligned for
        // it; every member of `MExt2fs` (integers, byte arrays, a null pointer, in `Cell`s
        // and an `UnsafeCell`) is valid as zero bytes, which is `MExt2fs::new()`. It lives
        // until the unmount (or the error path below) frees it.
        let fs: &'static MExt2fs = unsafe { &*fsmem.as_ptr().cast::<MExt2fs>() };
        u.um_e2fs.set(Some(fs));

        // Copy in the superblock, compute in-memory values and load group descriptors.
        fs.with_e2fs_mut(|e| e2fs_sbload(sb, e));
        if let Err(e) = e2fs_sbfill(devvp, fs) {
            break 'out e;
        }
        brelse(b);
        fs.e2fs_ronly.set(i8::from(ronly));
        u.um_fstype.set(UM_EXT2FS);

        if !ronly {
            if fs.e2fs_state() == E2FS_ISCLEAN {
                fs.set_e2fs_state(0);
            } else {
                fs.set_e2fs_state(E2FS_ERRORS);
            }
            fs.e2fs_fmod.set(1);
        }

        mp.mnt_data.set(um.as_ptr().cast());
        mp.update_stat(|sp| {
            sp.f_fsid.val[0] = dev;
            sp.f_fsid.val[1] = mp.vfc().vfc_typenum;
            sp.f_namemax = MAXNAMLEN as u32;
        });
        mp.mnt_flag.set(mp.mnt_flag.get() | MNT_LOCAL);
        u.um_mountp.set(Some(mp));
        u.um_dev.set(dev);
        u.um_devvp.set(Some(devvp));
        u.um_nindir.set(nindir(fs) as u64);
        u.um_bptrtodb.set(fs.e2fs_fsbtodb.get() as u64);
        u.um_seqinc.set(1); // no frags
        u.um_maxsymlinklen.set(EXT2_MAXSYMLINKLEN as u32);
        if let Some(si) = devvp.v_specinfo() {
            si.si_mountpoint.set(Some(mp));
        }
        return Ok(());
    };

    // out:
    if let Some(si) = devvp.v_specinfo() {
        si.si_mountpoint.set(None);
    }
    if let Some(b) = bp {
        brelse(b);
    }
    let _ = vn_lock(devvp, LK_EXCLUSIVE | LK_RETRY);
    let _ = VOP_CLOSE(devvp, omode, cred, Some(p));
    let _ = VOP_UNLOCK(devvp);
    if let Some(u) = ump {
        if let Some(fs) = u.um_e2fs.get() {
            free(NonNull::from(fs).cast(), M_UFSMNT, size_of::<MExt2fs>());
        }
        free(NonNull::from(u).cast(), M_UFSMNT, size_of::<Ufsmount>());
        mp.mnt_data.set(ptr::null_mut());
    }
    Err(error)
}

/// `ext2fs_unmount` (`vfs_unmount`): unmount system call.
pub fn ext2fs_unmount(mp: &'static Mount, mntflags: i32, p: &Proc) -> Result<(), Errno> {
    let mut flags = 0;
    if mntflags & MNT_FORCE != 0 {
        flags |= FORCECLOSE;
    }
    ext2fs_flushfiles(mp, flags, p)?;
    let ump = vfstoufs(mp);
    let fs = ump.e2fs();
    let gdescs_space = fs.e2fs_ngdb.get() as usize * fs.e2fs_bsize.get() as usize;

    if fs.e2fs_ronly.get() == 0
        && ext2fs_cgupdate(ump, MNT_WAIT).is_ok()
        && fs.e2fs_state() & E2FS_ERRORS == 0
    {
        fs.set_e2fs_state(E2FS_ISCLEAN);
        let _ = ext2fs_sbupdate(ump, MNT_WAIT);
    }

    let devvp = ump.devvp();
    if devvp.v_type.get() != VBAD
        && let Some(si) = devvp.v_specinfo()
    {
        si.si_mountpoint.set(None);
    }
    let _ = vn_lock(devvp, LK_EXCLUSIVE | LK_RETRY);
    let omode = if fs.e2fs_ronly.get() != 0 {
        FREAD
    } else {
        FREAD | FWRITE
    };
    let _ = VOP_CLOSE(devvp, omode, NOCRED, Some(p));
    vput(devvp);
    if let Some(gd) = NonNull::new(fs.e2fs_gd.get().cast::<u8>()) {
        free(gd, M_UFSMNT, gdescs_space);
    }
    free(NonNull::from(fs).cast(), M_UFSMNT, size_of::<MExt2fs>());
    free(NonNull::from(ump).cast(), M_UFSMNT, size_of::<Ufsmount>());
    mp.mnt_data.set(ptr::null_mut());
    mp.mnt_flag.set(mp.mnt_flag.get() & !MNT_LOCAL);
    Ok(())
}

/// `ext2fs_flushfiles`: flush out all the files in a filesystem.
pub fn ext2fs_flushfiles(mp: &'static Mount, flags: i32, p: &Proc) -> Result<(), Errno> {
    let ump = vfstoufs(mp);
    // Flush all the files.
    vflush(mp, None, flags)?;
    // Flush filesystem metadata.
    let devvp = ump.devvp();
    let _ = vn_lock(devvp, LK_EXCLUSIVE | LK_RETRY);
    let error = VOP_FSYNC(devvp, p.p_ucred.get(), MNT_WAIT, p);
    let _ = VOP_UNLOCK(devvp);
    error
}

/// `ext2fs_statfs` (`vfs_statfs`): get file system statistics.
pub fn ext2fs_statfs(mp: &'static Mount, sbp: &mut Statfs, _p: &Proc) -> Result<(), Errno> {
    let ump = vfstoufs(mp);
    let fs = ump.e2fs();
    if fs.e2fs_magic() != E2FS_MAGIC {
        panic(format_args!("ext2fs_statfs"));
    }

    // Compute the overhead (FS structures)
    let ncg = fs.e2fs_ncg.get();
    let overhead_per_group: u32 = 1 /* block bitmap */ + 1 /* inode bitmap */
        + fs.e2fs_itpg.get() as u32;
    let mut overhead = fs
        .e2fs_first_dblock()
        .wrapping_add((ncg as u32).wrapping_mul(overhead_per_group));
    let ngroups = if fs.e2fs_rev() > E2FS_REV0
        && fs.e2fs_features_rocompat() & EXT2F_ROCOMPAT_SPARSE_SUPER != 0
    {
        (0..ncg).filter(|&i| cg_has_sb(i)).count() as u32
    } else {
        ncg as u32
    };
    overhead = overhead.wrapping_add(ngroups.wrapping_mul(1 + fs.e2fs_ngdb.get() as u32));

    sbp.f_bsize = fs.e2fs_bsize.get() as u32;
    sbp.f_iosize = fs.e2fs_bsize.get() as u32;
    sbp.f_blocks = u64::from(fs.e2fs_bcount().wrapping_sub(overhead));
    sbp.f_bfree = u64::from(fs.e2fs_fbcount());
    sbp.f_bavail = sbp.f_bfree as i64 - i64::from(fs.e2fs_rbcount());
    sbp.f_files = u64::from(fs.e2fs_icount());
    sbp.f_ffree = u64::from(fs.e2fs_ficount());
    sbp.f_favail = i64::from(fs.e2fs_ficount());
    copy_statfs_info(sbp, mp);

    Ok(())
}

/// `struct ext2fs_sync_args`.
struct Ext2fsSyncArgs<'a> {
    /// `allerror`.
    allerror: Result<(), Errno>,
    /// `waitfor`.
    waitfor: i32,
    /// `nlink0`.
    nlink0: i32,
    /// `inflight`.
    inflight: i32,
    /// `p`.
    p: &'a Proc,
    /// `cred`.
    cred: *const Ucred,
}

/// `ext2fs_sync_vnode`: write back one (modified) inode for `ext2fs_sync`.
fn ext2fs_sync_vnode(vp: &'static Vnode, esa: &mut Ext2fsSyncArgs<'_>) -> Result<(), Errno> {
    if vp.v_type.get() == VNON {
        return Ok(());
    }

    let ip = vtoi(vp);
    let mut nlink0 = 0;

    if ip.i_e2fs_nlink() == 0 {
        nlink0 = 1;
    }

    let s = splbio();
    let skip = ip.i_flag.get() & (IN_ACCESS | IN_CHANGE | IN_MODIFIED | IN_UPDATE) == 0
        && vp.v_dirtyblkhd.is_empty();
    splx(s);

    'end: {
        if skip {
            break 'end;
        }

        if vget(vp, LK_EXCLUSIVE | LK_NOWAIT).is_err() {
            esa.inflight = (esa.inflight + 1).min(65536);
            break 'end;
        }

        if let Err(e) = VOP_FSYNC(vp, esa.cred, esa.waitfor, esa.p) {
            esa.allerror = Err(e);
        }
        vput(vp);
    }
    // end:
    esa.nlink0 = (esa.nlink0 + nlink0).min(65536);
    Ok(())
}

/// `ext2fs_sync` (`vfs_sync`): go through the disk queues to initiate sandbagged IO; go
/// through the inodes to write those that have been modified; initiate the writing of the
/// super block if it has been modified.
///
/// Should always be called with the mount point locked.
pub fn ext2fs_sync(
    mp: &'static Mount,
    waitfor: i32,
    stall: i32,
    cred: *const Ucred,
    p: &Proc,
) -> Result<(), Errno> {
    let ump = vfstoufs(mp);
    let fs = ump.e2fs();
    let mut allerror = Ok(());
    if fs.e2fs_ronly.get() != 0 {
        // XXX
        kprintf!("fs = {}\n", fs.fsmnt_str());
        panic(format_args!("update: rofs mod"));
    }

    // Write back each (modified) inode.
    let mut esa = Ext2fsSyncArgs {
        allerror: Ok(()),
        waitfor,
        nlink0: 0,
        inflight: 0,
        p,
        cred,
    };

    let _ = vfs_mount_foreach_vnode(mp, &mut |vp| ext2fs_sync_vnode(vp, &mut esa));
    if esa.allerror.is_err() {
        allerror = esa.allerror;
    }

    // Force stale file system control information to be flushed.
    if waitfor != MNT_LAZY {
        let devvp = ump.devvp();
        let _ = vn_lock(devvp, LK_EXCLUSIVE | LK_RETRY);
        if let Err(e) = VOP_FSYNC(devvp, cred, waitfor, p) {
            allerror = Err(e);
        }
        let _ = VOP_UNLOCK(devvp);
    }
    // Write back modified superblock.
    let state = fs.e2fs_state();
    let fmod = fs.e2fs_fmod.get();
    if stall != 0 && fs.e2fs_ronly.get() == 0 {
        fs.e2fs_fmod.set(1);
        if allerror.is_ok() && esa.nlink0 == 0 && esa.inflight == 0 {
            if fs.e2fs_state() & E2FS_ERRORS == 0 {
                fs.set_e2fs_state(E2FS_ISCLEAN);
            }
        } else {
            fs.set_e2fs_state(0);
        }
    }
    if fs.e2fs_fmod.get() != 0 {
        fs.e2fs_fmod.set(0);
        fs.set_e2fs_wtime(gettime() as u32);
        if let Err(e) = ext2fs_cgupdate(ump, waitfor) {
            allerror = Err(e);
        }
    }
    fs.set_e2fs_state(state);
    fs.e2fs_fmod.set(fmod);
    allerror
}

/// `ext2fs_vget` (`vfs_vget`): look up an EXT2FS dinode number to find its incore vnode,
/// otherwise read it in from disk. If it is in core, wait for the lock bit to clear, then
/// return the inode locked. Detection and handling of mount points must be done by the
/// calling routine.
pub fn ext2fs_vget(mp: &'static Mount, ino: Ino) -> Result<&'static Vnode, Errno> {
    if ino > u64::from(Ufsino::MAX) {
        panic(format_args!("ext2fs_vget: alien ino_t {}", ino));
    }
    let ino = ino as Ufsino;

    let ump = vfstoufs(mp);
    let dev = ump.um_dev.get();

    loop {
        // retry:
        if let Some(vp) = ufs_ihashget(dev, ino) {
            return Ok(vp);
        }

        // Allocate a new vnode/inode.
        let vp = getnewvnode(VT_EXT2FS, Some(mp), &EXT2FS_VOPS)?;

        let Some(mem) = pool_get(&EXT2FS_INODE_POOL, PR_WAITOK | PR_ZERO) else {
            panic(format_args!("ext2fs_vget: no inode"));
        };
        let ipp = mem.cast::<Inode>();
        // SAFETY: a fresh `ext2fs_inode_pool` item, sized and aligned for an `Inode`.
        unsafe { ptr::write(ipp.as_ptr(), Inode::new()) };
        // SAFETY: just initialised; freed only by `ext2fs_reclaim`.
        let ip: &'static Inode = unsafe { &*ipp.as_ptr() };
        rrw_init_flags(&ip.i_lock, "inode", RWL_DUPOK | RWL_IS_VNODE);
        vp.v_data.set(ipp.as_ptr().cast());
        ip.i_vnode.set(Some(vp));
        ip.i_ump.set(Some(ump));
        // The C takes this reference after ext2fs_vinit (see the module's deviations).
        vref(ump.devvp());
        let fs = ump.e2fs();
        ip.i_e2fs.set(Some(fs));
        ip.i_dev.set(dev);
        ip.i_number.set(ino);
        ip.i_e2fs_last_lblk().set(0);
        ip.i_e2fs_last_blk().set(0);

        // Put it onto its hash chain and lock it so that other requests for this inode will
        // block if they arrive while we are sleeping waiting for old data structures to be
        // purged or for the contents of the disk portion of this inode to be read.
        if let Err(error) = ufs_ihashins(ip) {
            vrele(vp);

            if error == Errno::EEXIST {
                continue;
            }

            return Err(error);
        }

        // Read in the disk contents for the inode, copy into the inode.
        let (bp, error) = bread(
            ump.devvp(),
            fsbtodb(fs, Daddr::from(ino_to_fsba(fs, ino))),
            fs.e2fs_bsize.get(),
        );
        if let Err(e) = error {
            // The inode does not contain anything useful, so it would be misleading to leave
            // it on its hash chain. With mode still zero, it will be unlinked and returned
            // to the free list by vput().
            vput(vp);
            brelse(bp);
            return Err(e);
        }

        let Some(d) = pool_get(&EXT2FS_DINODE_POOL, PR_WAITOK) else {
            panic(format_args!("ext2fs_vget: no dinode"));
        };
        let d = d.cast::<Ext2fsDinode>();
        // SAFETY: a fresh `ext2fs_dinode_pool` item, sized and aligned for it.
        unsafe { ptr::write(d.as_ptr(), Ext2fsDinode::default()) };
        ip.dinode_u.set(d.as_ptr().cast());
        {
            // SAFETY: the buffer is ours (busy from bread) and mapped; the slice dies
            // before it is released.
            let data = unsafe { bp.data() };
            let off = fs.dinode_size() * ino_to_fsbo(fs, ino) as usize;
            ip.with_e2din(|din| e2fs_iload(fs, &data[off..], din));
        }
        brelse(bp);

        ip.i_effnlink.set(i32::from(ip.i_e2fs_nlink()));

        // The fields for storing the UID and GID of an ext2fs inode are limited to 16 bits.
        // To overcome this limitation, Linux decided to scatter the highest bits of these
        // values into a previously reserved area on the disk inode. We deal with this
        // situation by having two 32-bit fields *out* of the disk inode to hold the
        // complete values. Now that we are reading in the inode, compute these fields.
        ip.i_e2fs_uid()
            .set(u32::from(ip.i_e2fs_uid_low()) | (u32::from(ip.i_e2fs_uid_high()) << 16));
        ip.i_e2fs_gid()
            .set(u32::from(ip.i_e2fs_gid_low()) | (u32::from(ip.i_e2fs_gid_high()) << 16));

        // If the inode was deleted, reset all fields
        if ip.i_e2fs_dtime() != 0 {
            ip.set_i_e2fs_nblock(0);
            ip.set_i_e2fs_mode(0);
            let _ = ext2fs_setsize(ip, 0);
        }

        // Initialize the vnode from the inode, check for aliases. Note that the underlying
        // vnode may have changed.
        let vp = match ext2fs_vinit(mp, vp) {
            Ok(vp) => vp,
            Err(e) => {
                vput(vp);
                return Err(e);
            }
        };

        // Set up a generation number for this inode if it does not already have one. This
        // should only happen on old filesystems.
        if ip.i_e2fs_gen() == 0 {
            ip.set_i_e2fs_gen(next_gennumber() as u32);
            if vp
                .v_mount
                .get()
                .is_some_and(|m| m.mnt_flag.get() & MNT_RDONLY == 0)
            {
                ip.set_flag(IN_MODIFIED);
            }
        }

        return Ok(vp);
    }
}

/// `ext2fs_fhtovp` (`vfs_fhtovp`): file handle to vnode.
///
/// Have to be really careful about stale file handles:
/// - check that the inode number is valid
/// - call `ext2fs_vget()` to get the locked inode
/// - check for an unallocated inode (`i_mode == 0`)
/// - check that the given client host has export rights and return those rights via
///   `exflagsp` and `credanonp`
pub fn ext2fs_fhtovp(mp: &'static Mount, fhp: &Fid) -> Result<&'static Vnode, Errno> {
    let ufhp = Ufid::from_fid(fhp);
    let fs = vfstoufs(mp).e2fs();
    if (ufhp.ufid_ino < EXT2_FIRSTINO && ufhp.ufid_ino != EXT2_ROOTINO)
        || ufhp.ufid_ino > (fs.e2fs_ncg.get() as u32).wrapping_mul(fs.e2fs_ipg())
    {
        return Err(Errno::ESTALE);
    }

    let nvp = VFS_VGET(mp, u64::from(ufhp.ufid_ino))?;
    let ip = vtoi(nvp);
    if ip.i_e2fs_mode() == 0 || ip.i_e2fs_dtime() != 0 || ip.i_e2fs_gen() != ufhp.ufid_gen {
        vput(nvp);
        return Err(Errno::ESTALE);
    }
    Ok(nvp)
}

/// `ext2fs_vptofh` (`vfs_vptofh`): vnode pointer to file handle.
pub fn ext2fs_vptofh(vp: &'static Vnode, fhp: &mut Fid) -> Result<(), Errno> {
    let ip = vtoi(vp);
    Ufid {
        ufid_len: size_of::<Ufid>() as u16,
        ufid_pad: fhp.fid_reserved,
        ufid_ino: ip.i_number.get(),
        ufid_gen: ip.i_e2fs_gen(),
    }
    .to_fid(fhp);
    Ok(())
}

/// `getblk(vp, blkno, size, 0, INFSLP)`, which cannot fail without `slpflag`.
fn getblk_wait(vp: &'static Vnode, blkno: Daddr, size: i32) -> &'static Buf {
    loop {
        if let Some(bp) = getblk(vp, blkno, size, 0, INFSLP) {
            return bp;
        }
    }
}

/// `ext2fs_sbupdate`: write a superblock back to disk.
pub fn ext2fs_sbupdate(mp: &Ufsmount, waitfor: i32) -> Result<(), Errno> {
    let fs = mp.e2fs();

    let bp = getblk_wait(mp.devvp(), SBLOCK, SBSIZE as i32);
    // SAFETY: the buffer is ours (busy from getblk) and mapped, SBSIZE bytes.
    fs.with_e2fs(|e| e2fs_sbsave(e, unsafe { bp.data() }));
    let error = if waitfor == MNT_WAIT {
        bwrite(bp)
    } else {
        bawrite(bp);
        Ok(())
    };
    fs.e2fs_fmod.set(0);
    error
}

/// `ext2fs_cgupdate`: write the superblock and the group descriptors back to disk.
pub fn ext2fs_cgupdate(mp: &Ufsmount, waitfor: i32) -> Result<(), Errno> {
    let fs = mp.e2fs();
    let mut error = Ok(());

    let allerror = ext2fs_sbupdate(mp, waitfor);
    let bsize = fs.e2fs_bsize.get();
    let per_block = bsize as usize / EXT2_GD_SIZE;
    for i in 0..fs.e2fs_ngdb.get() as usize {
        let dblk = Daddr::from(i32::from(bsize <= 1024)) + i as Daddr + 1;
        let bp = getblk_wait(mp.devvp(), fsbtodb(fs, dblk), bsize);
        {
            // SAFETY: the mount's descriptor area (`e2fs_sbfill`); nothing else reaches it
            // while the slice lives.
            let area = unsafe { e2fs_gd_area(fs) };
            // SAFETY: the buffer is ours (busy from getblk) and mapped.
            let data = unsafe { bp.data() };
            e2fs_cgsave(
                &area[i * per_block..(i + 1) * per_block],
                data,
                bsize as usize,
            );
        }
        if waitfor == MNT_WAIT {
            error = bwrite(bp);
        } else {
            bawrite(bp);
        }
    }

    if allerror.is_ok() && error.is_err() {
        return error;
    }
    allerror
}

/// `letoh16` of the super block member at `off` in the buffer's bytes.
fn sb_le16(sb: &[u8], off: usize) -> u32 {
    u32::from(u16::from_le_bytes([sb[off], sb[off + 1]]))
}

/// `letoh32` of the super block member at `off` in the buffer's bytes.
fn sb_le32(sb: &[u8], off: usize) -> u32 {
    u32::from_le_bytes([sb[off], sb[off + 1], sb[off + 2], sb[off + 3]])
}

/// `e2fs_sbcheck`: whether the super block in `sb` (the buffer's bytes, before the copy:
/// watch out for endianness!) can be mounted, read-only if `ronly`.
fn e2fs_sbcheck(sb: &[u8], ronly: bool) -> Result<(), Errno> {
    let tmp = sb_le16(sb, offset_of!(Ext2fs, e2fs_magic));
    if tmp != u32::from(E2FS_MAGIC) {
        kprintf!("ext2fs: wrong magic number 0x{:x}\n", tmp);
        return Err(Errno::EIO); // XXX needs translation
    }

    let mut tmp = sb_le32(sb, offset_of!(Ext2fs, e2fs_log_bsize));
    if tmp > 2 {
        // skewed log(block size): 1024 -> 0 | 2048 -> 1 | 4096 -> 2
        tmp += 10;
        kprintf!("ext2fs: wrong log2(block size) {}\n", tmp);
        return Err(Errno::EIO); // XXX needs translation
    }

    if sb_le32(sb, offset_of!(Ext2fs, e2fs_bpg)) == 0 {
        kprintf!("ext2fs: zero blocks per group\n");
        return Err(Errno::EIO);
    }

    let tmp = sb_le32(sb, offset_of!(Ext2fs, e2fs_rev));
    if tmp > E2FS_REV1 {
        kprintf!("ext2fs: wrong revision number 0x{:x}\n", tmp);
        return Err(Errno::EIO); // XXX needs translation
    } else if tmp == E2FS_REV0 {
        return Ok(());
    }

    let tmp = sb_le32(sb, offset_of!(Ext2fs, e2fs_first_ino));
    if tmp != EXT2_FIRSTINO {
        kprintf!("ext2fs: first inode at 0x{:x}\n", tmp);
        return Err(Errno::EINVAL); // XXX needs translation
    }

    let tmp = sb_le32(sb, offset_of!(Ext2fs, e2fs_features_incompat));
    let mask = tmp & !(EXT2F_INCOMPAT_SUPP | EXT4F_RO_INCOMPAT_SUPP);
    if mask != 0 {
        kprintf!("ext2fs: unsupported incompat features: ");
        for f in INCOMPAT.iter().filter(|f| mask & f.mask != 0) {
            kprintf!("{} ", f.name);
        }
        kprintf!("\n");
        return Err(Errno::EINVAL); // XXX needs translation
    }

    if !ronly && tmp & EXT4F_RO_INCOMPAT_SUPP != 0 {
        kprintf!("ext4fs: only read-only support right now\n");
        return Err(Errno::EROFS); // XXX needs translation
    }

    if tmp & EXT2F_INCOMPAT_RECOVER != 0 {
        kprintf!("ext2fs: your file system says it needs recovery\n");
        if !ronly {
            return Err(Errno::EROFS); // XXX needs translation
        }
    }

    let tmp = sb_le32(sb, offset_of!(Ext2fs, e2fs_features_rocompat)) & !EXT2F_ROCOMPAT_SUPP;
    if !ronly && tmp != 0 {
        kprintf!("ext2fs: unsupported R/O compat features: ");
        for f in RO_COMPAT.iter().filter(|f| tmp & f.mask != 0) {
            kprintf!("{} ", f.name);
        }
        kprintf!("\n");
        return Err(Errno::EROFS); // XXX needs translation
    }

    Ok(())
}
/* </CODE> */

/* <TESTS> */
#[cfg(test)]
pub(crate) mod tests {
    // Host tests for ext2fs: a small ext2 file system built in memory (1 KiB blocks, one block
    // group: super block, group descriptor, bitmaps, inode table, a root directory, a short
    // file, a file with an indirect block and, on the read-only image, a file mapped by a
    // two-level extent tree), a block device vnode whose strategy reads and writes it, and
    // tests that mount it with `ext2fs_mountfs`, get vnodes with `ext2fs_vget`, read through
    // `ext2fs_read`, map blocks, take `statfs`, write, allocate and truncate on a read-write
    // mount, and unmount it clean; plus the super block checks.

    use core::mem::offset_of;
    use core::sync::atomic::Ordering;
    use std::sync::MutexGuard;
    use std::vec::Vec;
    use std::{assert, assert_eq, vec};

    use super::*;
    use crate::kern::subr_xxx::nullop;
    use crate::kern::vfs_bio::{BCSTATS, BUFHEAD, BUFKVM, CLEANCACHE, biodone, bufinit};
    use crate::kern::vfs_default::vop_generic_bwrite;
    use crate::kern::vfs_init::vfs_byname;
    use crate::kern::vfs_subr::{vflushbuf, vfs_mount_alloc};
    use crate::kern::vfs_vops::{VOP_BMAP, VOP_READ, VOP_WRITE};
    use crate::machine::Machine;
    use crate::machine::cpu::Cpu;
    use crate::sys::buf::{B_ERROR, B_READ};
    use crate::sys::mount::VFS_ROOT;
    use crate::sys::types::makedev;
    use crate::sys::uio::{Iovec, Uio, UioRw, UioSeg};
    use crate::sys::vnode::{
        VDIR, VREG, VROOT, VopFsyncArgs, VopInactiveArgs, VopStrategyArgs, Vops,
    };
    use crate::ufs::ext2fs::ext2fs::{
        E2FS_REV1, EXT2F_INCOMPAT_FTYPE, EXT2F_INCOMPAT_RECOVER, EXT2F_ROCOMPAT_BTREE_DIR, Ext2fs,
    };
    use crate::ufs::ext2fs::ext2fs_alloc::ext2fs_inode_alloc;
    use crate::ufs::ext2fs::ext2fs_dinode::EXT4_EXTENTS;
    use crate::ufs::ext2fs::ext2fs_extents::EXT4_EXT_MAGIC;
    use crate::ufs::ext2fs::ext2fs_inode::{ext2fs_size, ext2fs_truncate};
    use crate::ufs::ext2fs::ext2fs_subr::ext2fs_bufatoff;
    use crate::ufs::ufs::dinode::{IFREG, ROOTINO};

    const B: usize = 1024;
    /// Blocks in the file system.
    const NBLK: u32 = 64;
    /// Inodes per group (and in all).
    const IPG: u32 = 32;
    /// The inode size.
    const ISZ: usize = 128;
    /// Where things are (block numbers).
    const GDT: usize = 2;
    const BBITMAP: usize = 3;
    const IBITMAP: usize = 4;
    const ITABLE: usize = 5;
    const ROOTDIR: usize = 9;
    const HELLO: usize = 10;
    /// `big`: 12 direct blocks at 11..=22, its indirect block at 23 pointing at 24 and 25.
    const BIG0: usize = 11;
    const BIGIND: usize = 23;
    /// `ext`: its leaf at 26, extents {0, 2 blocks at 27} and {2, 1 block at 30}.
    const EXTLEAF: usize = 26;
    /// The free blocks: 29 and 31..=63.
    const NFREE: u32 = 34;
    /// The free inodes: 15..=32.
    const NIFREE: u32 = 18;

    const HELLO_DATA: &[u8] = b"hello, ext2\n";
    const BIG_SIZE: usize = 14 * B - 100;
    const EXT_SIZE: usize = 3 * B - 10;

    /// `n` bytes of a pattern that differs per `seed`.
    fn pattern(n: usize, seed: u8) -> Vec<u8> {
        (0..n)
            .map(|i| ((i * 7 + usize::from(seed)) % 251) as u8)
            .collect()
    }

    /// The image being built, 1 KiB blocks.
    struct Img(Vec<u8>);

    impl Img {
        fn put(&mut self, blk: usize, off: usize, b: &[u8]) {
            let o = blk * B + off;
            self.0[o..o + b.len()].copy_from_slice(b);
        }

        fn u16(&mut self, blk: usize, off: usize, v: u16) {
            self.put(blk, off, &v.to_le_bytes());
        }

        fn u32(&mut self, blk: usize, off: usize, v: u32) {
            self.put(blk, off, &v.to_le_bytes());
        }

        fn setbit(&mut self, blk: usize, bit: usize) {
            self.0[blk * B + bit / 8] |= 1 << (bit % 8);
        }

        /// The byte offset of inode `ino` (from 1) in the image.
        fn ioff(ino: u32) -> (usize, usize) {
            let i = (ino - 1) as usize;
            (ITABLE + i * ISZ / B, (i * ISZ) % B)
        }

        /// Inode `ino`: mode, links, size, block pointers (`blocks`), `DEV_BSIZE` blocks, flags.
        fn inode(
            &mut self,
            ino: u32,
            mode: u16,
            nlink: u16,
            size: u32,
            blocks: &[u32],
            flags: u32,
        ) {
            let (blk, off) = Self::ioff(ino);
            self.u16(blk, off + offset_of!(Ext2fsDinode, e2di_mode), mode);
            self.u16(blk, off + offset_of!(Ext2fsDinode, e2di_uid_low), 1000);
            self.u16(blk, off + offset_of!(Ext2fsDinode, e2di_uid_high), 1);
            self.u16(blk, off + offset_of!(Ext2fsDinode, e2di_gid_low), 20);
            self.u32(blk, off + offset_of!(Ext2fsDinode, e2di_size), size);
            self.u16(blk, off + offset_of!(Ext2fsDinode, e2di_nlink), nlink);
            self.u32(blk, off + offset_of!(Ext2fsDinode, e2di_flags), flags);
            self.u32(blk, off + offset_of!(Ext2fsDinode, e2di_gen), 7 + ino);
            let mut n = 0;
            for (i, &b) in blocks.iter().enumerate() {
                self.u32(blk, off + offset_of!(Ext2fsDinode, e2di_blocks) + 4 * i, b);
                if b != 0 && flags & EXT4_EXTENTS == 0 {
                    n += 2;
                }
            }
            self.u32(blk, off + offset_of!(Ext2fsDinode, e2di_nblock), n);
        }

        /// A directory entry at `off` of block `blk`.
        fn dirent(&mut self, blk: usize, off: usize, ino: u32, reclen: u16, name: &[u8], t: u8) {
            self.u32(blk, off, ino);
            self.u16(blk, off + 4, reclen);
            self.put(blk, off + 6, &[name.len() as u8, t]);
            self.put(blk, off + 8, name);
        }

        /// An extent tree node header.
        fn eh(&mut self, blk: usize, off: usize, ecount: u16, max: u16, depth: u16) {
            self.u16(blk, off, EXT4_EXT_MAGIC);
            self.u16(blk, off + 2, ecount);
            self.u16(blk, off + 4, max);
            self.u16(blk, off + 6, depth);
        }

        /// An extent: `len` blocks from logical `lblk` at disk block `start`.
        fn extent(&mut self, blk: usize, off: usize, lblk: u32, len: u16, start: u32) {
            self.u32(blk, off, lblk);
            self.u16(blk, off + 4, len);
            self.u16(blk, off + 6, 0);
            self.u32(blk, off + 8, start);
        }
    }

    /// The test file system; with `extents`, it has the `EXTENTS` incompatible feature (and
    /// mounts only read-only).
    fn image(extents: bool) -> Vec<u8> {
        let mut img = Img(vec![0; NBLK as usize * B]);

        // Super block.
        let mut sb = Ext2fs::new();
        sb.e2fs_icount = IPG;
        sb.e2fs_bcount = NBLK;
        sb.e2fs_rbcount = 3;
        sb.e2fs_fbcount = NFREE;
        sb.e2fs_ficount = NIFREE;
        sb.e2fs_first_dblock = 1;
        sb.e2fs_bpg = 8192;
        sb.e2fs_fpg = 8192;
        sb.e2fs_ipg = IPG;
        sb.e2fs_wtime = 1_700_000_000;
        sb.e2fs_max_mnt_count = 20;
        sb.e2fs_magic = E2FS_MAGIC;
        sb.e2fs_state = E2FS_ISCLEAN;
        sb.e2fs_beh = 1;
        sb.e2fs_rev = E2FS_REV1;
        sb.e2fs_first_ino = EXT2_FIRSTINO;
        sb.e2fs_inode_size = ISZ as u16;
        sb.e2fs_features_incompat =
            EXT2F_INCOMPAT_FTYPE | if extents { EXT2F_INCOMPAT_EXTENTS } else { 0 };
        sb.e2fs_features_rocompat = EXT2F_ROCOMPAT_SPARSE_SUPER;
        e2fs_sbsave(&sb, &mut img.0[1024..2048]);

        // Group descriptor.
        let gd = Ext2Gd {
            ext2bgd_b_bitmap: BBITMAP as u32,
            ext2bgd_i_bitmap: IBITMAP as u32,
            ext2bgd_i_tables: ITABLE as u32,
            ext2bgd_nbfree: NFREE as u16,
            ext2bgd_nifree: NIFREE as u16,
            ext2bgd_ndirs: 1,
            ..Ext2Gd::default()
        };
        img.put(GDT, 0, &gd.to_le_bytes());

        // Block bitmap: bit b is block b + 1; blocks 1..=28 and 30 are used, and the bits past
        // the last block.
        for b in 1..=28 {
            img.setbit(BBITMAP, b - 1);
        }
        img.setbit(BBITMAP, 30 - 1);
        for bit in (NBLK as usize - 1)..8 * B {
            img.setbit(BBITMAP, bit);
        }
        // Inode bitmap: inodes 1..=14, and the bits past the last inode.
        for i in 1..=14 {
            img.setbit(IBITMAP, i - 1);
        }
        for bit in IPG as usize..8 * B {
            img.setbit(IBITMAP, bit);
        }

        // The root directory.
        let mut root = [0u32; 15];
        root[0] = ROOTDIR as u32;
        img.inode(EXT2_ROOTINO, 0o40755, 2, B as u32, &root, 0);
        img.dirent(ROOTDIR, 0, 2, 12, b".", 2);
        img.dirent(ROOTDIR, 12, 2, 12, b"..", 2);
        img.dirent(ROOTDIR, 24, 12, 20, b"hello.txt", 1);
        img.dirent(ROOTDIR, 44, 13, 12, b"big", 1);
        img.dirent(ROOTDIR, 56, 14, (B - 56) as u16, b"ext", 1);

        // hello.txt (inode 12).
        let mut hello = [0u32; 15];
        hello[0] = HELLO as u32;
        img.inode(12, 0o100644, 1, HELLO_DATA.len() as u32, &hello, 0);
        img.put(HELLO, 0, HELLO_DATA);

        // big (inode 13): 14 blocks, two of them behind the indirect block.
        let mut big = [0u32; 15];
        for (i, b) in big.iter_mut().take(12).enumerate() {
            *b = (BIG0 + i) as u32;
        }
        big[12] = BIGIND as u32;
        img.inode(13, 0o100644, 1, BIG_SIZE as u32, &big, 0);
        img.u32(BIGIND, 0, 24);
        img.u32(BIGIND, 4, 25);
        let data = pattern(BIG_SIZE, 13);
        let blocks: Vec<usize> = (BIG0..BIG0 + 12).chain([24, 25]).collect();
        for (i, chunk) in data.chunks(B).enumerate() {
            img.put(blocks[i], 0, chunk);
        }

        // ext (inode 14): an index root in the inode, one leaf with two extents.
        img.inode(14, 0o100644, 1, EXT_SIZE as u32, &[], EXT4_EXTENTS);
        let (iblk, ioff) = Img::ioff(14);
        let broot = ioff + offset_of!(Ext2fsDinode, e2di_blocks);
        img.eh(iblk, broot, 1, 4, 1);
        img.u32(iblk, broot + 12, 0); // ei_blk
        img.u32(iblk, broot + 16, EXTLEAF as u32); // ei_leaf_lo
        img.eh(EXTLEAF, 0, 2, 84, 0);
        img.extent(EXTLEAF, 12, 0, 2, 27);
        img.extent(EXTLEAF, 24, 2, 1, 30);
        let data = pattern(EXT_SIZE, 14);
        for (chunk, blk) in data.chunks(B).zip([27, 28, 30]) {
            img.put(blk, 0, chunk);
        }

        img.0
    }

    /// The disk the strategy below reads and writes.
    pub(crate) static DISK: std::sync::Mutex<Vec<u8>> = std::sync::Mutex::new(Vec::new());

    /// A synchronous transfer between the buffer and the image at `b_blkno`, then `biodone`.
    fn disk_strategy(ap: &mut VopStrategyArgs) -> Result<(), Errno> {
        let bp = ap.a_bp;
        let off = bp.b_blkno.get() as usize * DEV_BSIZE;
        let len = bp.b_bcount.get() as usize;
        {
            let mut d = DISK.lock().unwrap_or_else(|e| e.into_inner());
            if off + len > d.len() {
                bp.b_error.set(Some(Errno::EIO));
                bp.set(B_ERROR);
            } else {
                // SAFETY: the buffer is busy for this transfer and mapped.
                let data = unsafe { bp.data() };
                if bp.isset(B_READ) {
                    data.copy_from_slice(&d[off..off + len]);
                } else {
                    d[off..off + len].copy_from_slice(data);
                }
                bp.b_resid.set(0);
            }
        }
        let s = splbio();
        biodone(bp);
        splx(s);
        Ok(())
    }

    fn disk_fsync(ap: &mut VopFsyncArgs<'_>) -> Result<(), Errno> {
        vflushbuf(ap.a_vp, ap.a_waitfor == MNT_WAIT);
        Ok(())
    }

    fn disk_inactive(ap: &mut VopInactiveArgs<'_>) -> Result<(), Errno> {
        VOP_UNLOCK(ap.a_vp)
    }

    /// The fake disk's block device vnode operations.
    static DISK_VOPS: Vops = Vops {
        vop_open: Some(|_| nullop()),
        vop_close: Some(|_| nullop()),
        vop_lock: Some(|_| nullop()),
        vop_unlock: Some(|_| nullop()),
        vop_islocked: Some(|_| 0),
        vop_inactive: Some(disk_inactive),
        vop_reclaim: Some(|_| nullop()),
        vop_strategy: Some(disk_strategy),
        vop_bwrite: Some(vop_generic_bwrite),
        vop_fsync: Some(disk_fsync),
        ..Vops::EMPTY
    };

    /// The fake disk's device number.
    const DISKDEV: i32 = makedev(17, 5);

    /// Memory, the vfs (whose `vfsinit` runs `ext2fs_init`), a fresh buffer cache, the image as
    /// the disk, the thread as `curproc`.
    pub(crate) fn setup(image: Vec<u8>) -> (MutexGuard<'static, ()>, &'static Proc) {
        let (g, p) = crate::kern::vfs_subr::tests::setup();
        Machine::set_curproc(Machine::curcpu(), p);
        BUFHEAD.0.init();
        for c in [
            &BCSTATS.numbufs,
            &BCSTATS.numbufpages,
            &BCSTATS.numdirtypages,
            &BCSTATS.numcleanpages,
            &BCSTATS.pendingwrites,
            &BCSTATS.pendingreads,
            &BCSTATS.numwrites,
            &BCSTATS.numreads,
            &BCSTATS.cachehits,
            &BCSTATS.busymapped,
            &BCSTATS.delwribufs,
        ] {
            c.store(0, Ordering::Relaxed);
        }
        CLEANCACHE.hotbufpages.set(0);
        CLEANCACHE.warmbufpages.set(0);
        CLEANCACHE.cachepages.set(0);
        BUFKVM.store(0, Ordering::Relaxed);
        crate::conf::param::bufpages.store(0, Ordering::Relaxed);
        bufinit();
        *DISK.lock().unwrap_or_else(|e| e.into_inner()) = image;
        (g, p)
    }

    pub(crate) fn teardown() {
        Machine::set_curproc(Machine::curcpu(), ptr::null());
    }

    /// `ext2fs_mountfs` of the disk on a fresh mount, and the `statfs` `sys_mount` follows it
    /// with.
    pub(crate) fn mount(p: &'static Proc, ronly: bool) -> Result<&'static Mount, Errno> {
        let devvp = bdevvp(DISKDEV).unwrap().unwrap();
        devvp.v_op.set(Some(&DISK_VOPS));
        let mp = vfs_mount_alloc(None, vfs_byname(b"ext2fs").unwrap());
        if ronly {
            mp.mnt_flag.set(mp.mnt_flag.get() | MNT_RDONLY);
        }
        match ext2fs_mountfs(devvp, mp, p) {
            Ok(()) => {
                let mut st = mp.mnt_stat.get();
                ext2fs_statfs(mp, &mut st, p).unwrap();
                mp.mnt_stat.set(st);
                Ok(mp)
            }
            Err(e) => {
                vrele(devvp);
                vfs_mount_free(mp);
                Err(e)
            }
        }
    }

    /// A `Uio` over `buf` at `offset`.
    fn uio<'a>(iov: &'a mut [Iovec; 1], offset: Off, rw: UioRw) -> Uio<'a> {
        let len = iov[0].iov_len;
        Uio {
            uio_iov: iov,
            uio_offset: offset,
            uio_resid: len,
            uio_segflg: UioSeg::UIO_SYSSPACE,
            uio_rw: rw,
            uio_procp: None,
        }
    }

    /// The whole file through `VOP_READ`, `chunk` bytes at a time.
    fn read_all(vp: &'static Vnode, chunk: usize) -> Vec<u8> {
        let mut out = Vec::new();
        loop {
            let mut buf = vec![0u8; chunk];
            let mut iov = [Iovec {
                iov_base: buf.as_mut_ptr().cast(),
                iov_len: chunk,
            }];
            let mut u = uio(&mut iov, out.len() as Off, UioRw::UIO_READ);
            VOP_READ(vp, &mut u, 0, ptr::null()).unwrap();
            let n = chunk - u.uio_resid;
            if n == 0 {
                return out;
            }
            out.extend_from_slice(&buf[..n]);
        }
    }

    /// `VOP_WRITE` of `data` at `offset`.
    fn write_at(p: &Proc, vp: &'static Vnode, offset: Off, data: &[u8]) {
        let mut buf = data.to_vec();
        let mut iov = [Iovec {
            iov_base: buf.as_mut_ptr().cast(),
            iov_len: buf.len(),
        }];
        let mut u = uio(&mut iov, offset, UioRw::UIO_WRITE);
        VOP_WRITE(vp, &mut u, 0, p.p_ucred.get()).unwrap();
        assert_eq!(u.uio_resid, 0);
    }

    /// `VOP_BMAP`: the disk block and run of logical block `bn`.
    fn bmap(vp: &'static Vnode, bn: Daddr) -> (Daddr, i32) {
        let mut bnp = 0;
        let mut run = 0;
        VOP_BMAP(vp, bn, None, Some(&mut bnp), Some(&mut run)).unwrap();
        (bnp, run)
    }

    /// A little-endian `u32` of the disk image.
    fn disk_u32(off: usize) -> u32 {
        let d = DISK.lock().unwrap();
        u32::from_le_bytes([d[off], d[off + 1], d[off + 2], d[off + 3]])
    }

    /// A little-endian `u16` of the disk image.
    fn disk_u16(off: usize) -> u16 {
        let d = DISK.lock().unwrap();
        u16::from_le_bytes([d[off], d[off + 1]])
    }

    /// Whether bit `bit` of the bitmap in block `blk` is set on the disk.
    fn disk_bit(blk: usize, bit: usize) -> bool {
        DISK.lock().unwrap()[blk * B + bit / 8] & (1 << (bit % 8)) != 0
    }

    #[test]
    fn mount_read_map_statfs_and_unmount() {
        let (_g, p) = setup(image(true));
        let mp = mount(p, true).unwrap();
        let ump = vfstoufs(mp);
        let fs = ump.e2fs();
        assert_eq!(ump.um_fstype.get(), UM_EXT2FS);
        assert_eq!(ump.um_nindir.get(), 256);
        assert_eq!(ump.um_bptrtodb.get(), 1);
        assert_eq!(ump.um_seqinc.get(), 1);
        assert_eq!(ump.um_maxsymlinklen.get(), 60);
        assert_eq!(fs.e2fs_ncg.get(), 1);
        assert_eq!(fs.e2fs_ngdb.get(), 1);
        assert_eq!(fs.e2fs_bsize.get(), 1024);
        assert_eq!(fs.e2fs_ipb.get(), 8);
        assert_eq!(fs.e2fs_itpg.get(), 4);
        assert_eq!(fs.gd(0).ext2bgd_i_tables, ITABLE as u32);
        assert_eq!(fs.e2fs_ronly.get(), 1);
        assert_eq!(fs.e2fs_maxfilesize.get(), Off::from(i32::MAX) * 4);
        assert_eq!(mp.mnt_flag.get() & MNT_LOCAL, MNT_LOCAL);

        // statfs: the overhead is the boot block, two bitmaps and four inode table blocks, and
        // one super block and one descriptor block.
        let st = mp.mnt_stat.get();
        assert_eq!(st.f_bsize, 1024);
        assert_eq!(st.f_iosize, 1024);
        assert_eq!(st.f_blocks, u64::from(NBLK) - 9);
        assert_eq!(st.f_bfree, u64::from(NFREE));
        assert_eq!(st.f_bavail, i64::from(NFREE) - 3);
        assert_eq!(st.f_files, u64::from(IPG));
        assert_eq!(st.f_ffree, u64::from(NIFREE));
        assert_eq!(&st.f_fstypename[..7], b"ext2fs\0");
        assert_eq!(st.f_fsid.val[1], 17);

        // The root directory.
        let root = VFS_ROOT(mp).unwrap();
        assert_eq!(root.v_type.get(), VDIR);
        assert_eq!(root.v_flag.get() & VROOT, VROOT);
        let rip = vtoi(root);
        assert_eq!(rip.i_number.get(), ROOTINO);
        assert_eq!(rip.i_e2fs_uid().get(), 1000 | 1 << 16);
        assert_eq!(rip.i_e2fs_gid().get(), 20);
        assert_eq!(rip.i_effnlink.get(), 2);
        let dir = read_all(root, 300);
        assert_eq!(dir.len(), B);
        assert_eq!(&dir[24..28], &12u32.to_le_bytes());
        assert_eq!(&dir[32..41], b"hello.txt");

        // hello.txt
        let hello = VFS_VGET(mp, 12).unwrap();
        assert_eq!(hello.v_type.get(), VREG);
        assert_eq!(read_all(hello, 5), HELLO_DATA);
        assert_eq!(bmap(hello, 0), (2 * HELLO as Daddr, 0));
        assert_eq!(bmap(hello, 1).0, -1);

        // big: direct blocks in one run, then the two behind the indirect block.
        let big = VFS_VGET(mp, 13).unwrap();
        assert_eq!(read_all(big, 1000), pattern(BIG_SIZE, 13));
        assert_eq!(bmap(big, 0), (2 * BIG0 as Daddr, 11));
        assert_eq!(bmap(big, 12), (48, 1));
        assert_eq!(bmap(big, 13), (50, 0));
        assert_eq!(bmap(big, 14).0, -1);

        // ext: through the extent tree (and its cache).
        let ext = VFS_VGET(mp, 14).unwrap();
        assert_eq!(read_all(ext, 700), pattern(EXT_SIZE, 14));
        assert_eq!(bmap(ext, 1).0, 2 * 28);
        assert_eq!(bmap(ext, 2).0, 2 * 30);
        assert_eq!(
            ext2fs_bufatoff(vtoi(ext), 2 * B as Off + 5).map(|(bp, off)| {
                // SAFETY: the buffer is ours (busy) and mapped.
                let b = unsafe { bp.data() }[off];
                brelse(bp);
                b
            }),
            Ok(pattern(EXT_SIZE, 14)[2 * B + 5])
        );

        // File handles.
        let mut fid = Fid::default();
        ext2fs_vptofh(hello, &mut fid).unwrap();
        let u = Ufid::from_fid(&fid);
        assert_eq!((u.ufid_len, u.ufid_ino, u.ufid_gen), (12, 12, 19));
        for ino in [5, IPG + 1] {
            let bad = Ufid { ufid_ino: ino, ..u };
            bad.to_fid(&mut fid);
            assert_eq!(ext2fs_fhtovp(mp, &fid).err(), Some(Errno::ESTALE));
        }

        vput(ext);
        vput(big);
        vput(hello);
        vput(root);
        ext2fs_unmount(mp, 0, p).unwrap();
        vfs_mount_free(mp);
        // A read-only unmount writes nothing.
        assert_eq!(*DISK.lock().unwrap(), image(true));
        teardown();
    }

    #[test]
    fn a_read_write_mount_allocates_truncates_and_unmounts_clean() {
        let (_g, p) = setup(image(false));
        let mp = mount(p, false).unwrap();
        let fs = vfstoufs(mp).e2fs();
        assert_eq!(fs.e2fs_state(), 0);
        assert_eq!(fs.e2fs_fmod.get(), 1);
        assert_eq!(fs.e2fs_maxfilesize.get(), Off::from(i32::MAX));

        let root = VFS_ROOT(mp).unwrap();
        let hello = VFS_VGET(mp, 12).unwrap();
        let ip = vtoi(hello);
        let cred = p.p_ucred.get();

        // Grow hello.txt to three blocks: two new ones, the first 8 free ones of the group.
        let data = pattern(3000, 1);
        write_at(p, hello, 0, &data);
        assert_eq!(ext2fs_size(ip), 3000);
        assert_eq!(fs.e2fs_fbcount(), NFREE - 2);
        assert_eq!(fs.gd(0).ext2bgd_nbfree as u32, NFREE - 2);
        assert_eq!(ip.i_e2fs_nblock(), 6);
        assert_eq!(ip.i_e2fs_block(1), 33);
        assert_eq!(ip.i_e2fs_block(2), 34);
        assert_eq!(read_all(hello, 512), data);

        // Truncate it back into its first block.
        ext2fs_truncate(ip, 12, 0, cred).unwrap();
        assert_eq!(ext2fs_size(ip), 12);
        assert_eq!(fs.e2fs_fbcount(), NFREE);
        assert_eq!(ip.i_e2fs_nblock(), 2);
        assert_eq!(ip.i_e2fs_blocks()[1..], [0; 14]);
        assert_eq!(read_all(hello, 512), data[..12]);

        // A byte at logical block 13: an indirect block and a data block, holes before. With no
        // block to follow, both go to the first wholly free byte of the bitmap: blocks 33..=40,
        // then 41..=48.
        write_at(p, hello, 13 * B as Off, b"Z");
        assert_eq!(fs.e2fs_fbcount(), NFREE - 2);
        assert_eq!(ip.i_e2fs_block(12), 33);
        assert_eq!(bmap(hello, 13).0, 2 * 41);
        let all = read_all(hello, 4096);
        assert_eq!(all.len(), 13 * B + 1);
        assert_eq!(all[..12], data[..12]);
        assert!(all[12..13 * B].iter().all(|&b| b == 0));
        assert_eq!(all[13 * B], b'Z');

        // Truncate to nothing: the data block under the indirect block, the indirect block and
        // the first block go back.
        ext2fs_truncate(ip, 0, 0, cred).unwrap();
        assert_eq!(ext2fs_size(ip), 0);
        assert_eq!(fs.e2fs_fbcount(), NFREE + 1);
        assert_eq!(ip.i_e2fs_nblock(), 0);
        assert_eq!(ip.i_e2fs_blocks(), [0; 15]);

        // A new inode: the first free one, 15; let go with no links, it is freed again.
        let nvp = ext2fs_inode_alloc(vtoi(root), IFREG | 0o644, cred).unwrap();
        let nip = vtoi(nvp);
        assert_eq!(nip.i_number.get(), 15);
        assert_eq!(fs.e2fs_ficount(), NIFREE - 1);
        assert_ne!(nip.i_e2fs_gen(), 0);
        nip.set_i_e2fs_mode((IFREG | 0o644) as u16);
        vput(nvp);
        assert_eq!(fs.e2fs_ficount(), NIFREE);

        vput(hello);
        vput(root);
        ext2fs_unmount(mp, 0, p).unwrap();
        vfs_mount_free(mp);

        // On the disk: clean, the counts and bitmaps back, hello.txt empty, inode 15 free.
        let sb = 1024;
        assert_eq!(disk_u16(sb + offset_of!(Ext2fs, e2fs_state)), E2FS_ISCLEAN);
        assert_eq!(disk_u32(sb + offset_of!(Ext2fs, e2fs_fbcount)), NFREE + 1);
        assert_eq!(disk_u32(sb + offset_of!(Ext2fs, e2fs_ficount)), NIFREE);
        assert_eq!(disk_u16(GDT * B + 12), (NFREE + 1) as u16);
        assert_eq!(disk_u16(GDT * B + 14), NIFREE as u16);
        assert!(!disk_bit(BBITMAP, HELLO - 1));
        assert!(!disk_bit(BBITMAP, 32) && !disk_bit(BBITMAP, 33) && !disk_bit(BBITMAP, 40));
        assert!(!disk_bit(IBITMAP, 14));
        let (blk, off) = Img::ioff(12);
        let at = blk * B + off;
        assert_eq!(disk_u32(at + offset_of!(Ext2fsDinode, e2di_size)), 0);
        assert_eq!(disk_u32(at + offset_of!(Ext2fsDinode, e2di_nblock)), 0);
        assert_eq!(disk_u32(at + offset_of!(Ext2fsDinode, e2di_blocks)), 0);
        assert_eq!(disk_u16(at + offset_of!(Ext2fsDinode, e2di_uid_high)), 1);
        // Inode 15 went to its block as it was let go (its `dtime` is the host clock's, 0 here).
        let (blk, off) = Img::ioff(15);
        let at = blk * B + off;
        assert_eq!(disk_u16(at + offset_of!(Ext2fsDinode, e2di_mode)), 0o100644);
        assert_eq!(disk_u16(at + offset_of!(Ext2fsDinode, e2di_nlink)), 0);
        assert_ne!(disk_u32(at + offset_of!(Ext2fsDinode, e2di_gen)), 0);
        teardown();
    }

    #[test]
    fn ext4_needs_a_read_only_mount_and_bad_super_blocks_are_refused() {
        let (_g, p) = setup(image(true));
        assert_eq!(mount(p, false).err(), Some(Errno::EROFS));
        teardown();

        /// A change to the super block.
        type Edit = dyn Fn(&mut Ext2fs);
        let sb = |f: &Edit| {
            let mut img = image(false);
            let mut s = Ext2fs::new();
            e2fs_sbload(&img[1024..2048], &mut s);
            f(&mut s);
            e2fs_sbsave(&s, &mut img[1024..2048]);
            img
        };
        assert_eq!(e2fs_sbcheck(&sb(&|_| {})[1024..2048], false), Ok(()));
        let cases: [(&Edit, bool, Errno); 7] = [
            (&|s| s.e2fs_magic = 0x1234, true, Errno::EIO),
            (&|s| s.e2fs_log_bsize = 3, true, Errno::EIO),
            (&|s| s.e2fs_bpg = 0, true, Errno::EIO),
            (&|s| s.e2fs_rev = 2, true, Errno::EIO),
            (&|s| s.e2fs_first_ino = 12, true, Errno::EINVAL),
            (&|s| s.e2fs_features_incompat |= 0x8000, true, Errno::EINVAL),
            (
                &|s| s.e2fs_features_incompat |= EXT2F_INCOMPAT_RECOVER,
                false,
                Errno::EROFS,
            ),
        ];
        for (f, ronly, e) in cases {
            assert_eq!(e2fs_sbcheck(&sb(f)[1024..2048], ronly), Err(e));
        }
        // A file system to recover mounts read-only; an unknown read-only feature too.
        let recover = sb(&|s| s.e2fs_features_incompat |= EXT2F_INCOMPAT_RECOVER);
        assert_eq!(e2fs_sbcheck(&recover[1024..2048], true), Ok(()));
        let btree = sb(&|s| s.e2fs_features_rocompat |= EXT2F_ROCOMPAT_BTREE_DIR);
        assert_eq!(e2fs_sbcheck(&btree[1024..2048], true), Ok(()));
        assert_eq!(e2fs_sbcheck(&btree[1024..2048], false), Err(Errno::EROFS));
        // Revision 0 skips the feature checks.
        let rev0 = sb(&|s| {
            s.e2fs_rev = 0;
            s.e2fs_first_ino = 0;
        });
        assert_eq!(e2fs_sbcheck(&rev0[1024..2048], false), Ok(()));
    }

    #[test]
    fn the_largest_file_follows_the_block_size_and_huge_files() {
        let fs: &'static MExt2fs = std::boxed::Box::leak(std::boxed::Box::new(MExt2fs::new()));
        fs.e2fs_bsize.set(1024);
        assert_eq!(
            ext2fs_maxfilesize(fs),
            (12 + 256 + 256 * 256 + 256 * 256 * 256) * 1024
        );
        fs.e2fs_bsize.set(4096);
        // Logically 4 TiB and more; the 32-bit sector count of a disk address stops at 2 TiB.
        assert_eq!(ext2fs_maxfilesize(fs), Off::from(u32::MAX) * 512);
        fs.set_e2fs_features_rocompat(EXT2F_ROCOMPAT_HUGE_FILE);
        assert_eq!(
            ext2fs_maxfilesize(fs),
            (12 + 1024 + 1024 * 1024 + 1024 * 1024 * 1024) * 4096
        );
    }
}
/* </TESTS> */
