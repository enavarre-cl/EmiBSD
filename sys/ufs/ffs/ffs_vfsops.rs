/*	$OpenBSD: ffs_vfsops.c,v 1.201 2025/09/20 13:53:36 mpi Exp $	*/
/*	$NetBSD: ffs_vfsops.c,v 1.19 1996/02/09 22:22:26 christos Exp $	*/
/* <LICENSES> */
/*
 * Copyright (c) 1989, 1991, 1993, 1994
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
 *	@(#)ffs_vfsops.c	8.14 (Berkeley) 11/28/94
 */
/* </LICENSES> */

//! The fast file system's file-system-type operations: mounting (`ffs_mountroot`,
//! `ffs_mount`, `ffs_mountfs` with the FFS1/FFS2 super-block search and compatibility code),
//! reloading, unmounting, `statfs`, `sync`, the inode cache's `ffs_vget`, file handles, the
//! super-block write-back (`ffs_sbupdate`) and `ffs_init`.
//!
//! Upstream: sys/ufs/ffs/ffs_vfsops.c @ 3ce1f3f79392
//!
//! ## Deviations
//! - The in-core super-block is allocated with at least `size_of::<Fs>()` bytes (and zeroed
//!   beyond `fs_sbsize`), so that `&Fs` never covers memory past the allocation when an old
//!   file system has a smaller `fs_sbsize`; the C allocates `fs_sbsize` bytes.
//! - `ffs_mount` looks the device name up in a `Nameidata` of its own (`ndinit` builds one
//!   around the kernel copy of the name) instead of reinitialising the caller's `ndp`, whose
//!   lifetime cannot hold a local name. `disk_map` (`subr_disk.c`, DUID names) is reported
//!   and the name is used as given, the C's answer when `disk_map` fails. `swapdev` and
//!   `nblkdev` come from the machine's `conf.c` (`crate::machine::conf`).
//! - `um_export` (`NFSSERVER`) is not kept, so the export update of `ffs_mount` passes a
//!   NULL table to `vfs_export`, which answers `ENOTSUP` without `NFSSERVER`.
//! - `ffs_vars[]` holds only the `UFS_DIRHASH` variables, so with `ufs_dirhash.c` not ported
//!   it is empty and `ffs_sysctl` answers every name as `sysctl_bounded_arr` does for an
//!   unknown one.
//! - `ffs_init`'s `static int done` is the atomic [`FFS_INIT_DONE`]; the host tests clear it
//!   to initialise again over fresh memory.
//! - The `struct ffs_reload_args`/`struct ffs_sync_args` callbacks of
//!   `vfs_mount_foreach_vnode` are closures over those structures.
//! - `rootdev` is `sys/systm.rs`'s `ROOTDEV`, `swapdev` the machine's (`conf.c`);
//!   `rootvp`/`swapdev_vp` are `init_main.rs`'s.

use core::ptr::{self, NonNull};
use core::sync::atomic::{AtomicBool, Ordering};

use crate::dev::rnd::arc4random;
use crate::kern::init_main::{rootvp, set_rootvp, set_swapdev_vp, swapdev_vp};
use crate::kern::kern_malloc::{free, malloc};
use crate::kern::kern_rwlock::rrw_init_flags;
use crate::kern::kern_sysctl::sysctl_bounded_arr;
use crate::kern::kern_tc::gettime;
use crate::kern::subr_pool::{pool_get, pool_init};
use crate::kern::subr_prf::panic;
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
use crate::sys::buf::{B_INVAL, B_NOCACHE, Buf};
use crate::sys::errno::Errno;
use crate::sys::fcntl::{FREAD, FWRITE};
use crate::sys::lock::{LK_EXCLUSIVE, LK_NOWAIT, LK_RETRY};
use crate::sys::malloc::{M_UFSMNT, M_WAITOK, M_ZERO};
use crate::sys::mount::{
    Fid, MNAMELEN, MNT_FORCE, MNT_LAZY, MNT_LOCAL, MNT_QUOTA, MNT_RDONLY, MNT_RELOAD, MNT_UPDATE,
    MNT_WAIT, MNT_WANTRDWR, Mount, Statfs, UfsArgs, VFS_STATFS, VFS_SYNC, Vfsconf, Vfsops,
};
use crate::sys::namei::{FOLLOW, LOOKUP, Nameidata, NiDirp};
use crate::sys::param::{DEV_BSIZE, MAXBSIZE, PAGE_SIZE, howmany};
use crate::sys::pool::{PR_WAITOK, PR_ZERO, Pool};
use crate::sys::proc::Proc;
use crate::sys::rwlock::{RWL_DUPOK, RWL_IS_VNODE};
use crate::sys::systm::{INFSLP, ROOTDEV};
use crate::sys::types::{Ino, major};
use crate::sys::ucred::{FSCRED, NOCRED, Ucred};
use crate::sys::vnode::{
    FORCECLOSE, IGNORECLEAN, SKIPSYSTEM, V_SAVE, VBLK, VNON, VT_UFS, Vnode, WRITECLOSE,
};
use crate::ufs::ffs::ffs_alloc::{ffs_inode_alloc, ffs_inode_free};
use crate::ufs::ffs::ffs_balloc::ffs_balloc;
use crate::ufs::ffs::ffs_inode::{ffs_truncate, ffs_update};
use crate::ufs::ffs::ffs_subr::{ffs_bufatoff, ffs_vinit};
use crate::ufs::ffs::ffs_vnops::FFS_VOPS;
use crate::ufs::ffs::fs::{
    AFPDIR, AVFILESIZ, CgBuf, FRAGTBL, FS_42INODEFMT, FS_42POSTBLFMT, FS_44INODEFMT,
    FS_FLAGS_UPDATED, FS_MAGIC, FS_UFS1_MAGIC, FS_UFS2_MAGIC, FS_UNCLEAN, Fs, MAXFRAG, SBLOCK_UFS2,
    SBLOCKSEARCH, SBSIZE, cgtod, fs_kernmaxfilesize, fsbtodb, ino_to_cg, ino_to_fsba, ino_to_fsbo,
    nindir,
};
use crate::ufs::ufs::dinode::{NDADDR, NIADDR, ROOTINO, Ufs1Dinode, Ufs2Dinode, Ufsino};
use crate::ufs::ufs::dir::MAXNAMLEN;
use crate::ufs::ufs::inode::{
    IN_ACCESS, IN_CHANGE, IN_LAZYMOD, IN_MODIFIED, IN_UPDATE, Inode, InodeVtbl, UFS_UPDATE, Ufid,
    dinode1_at, dinode2_at, vtoi,
};
use crate::ufs::ufs::quota::{MAXQUOTAS, qsync, quotaoff, ufs_quotactl};
use crate::ufs::ufs::ufs_ihash::{ufs_ihashget, ufs_ihashins};
use crate::ufs::ufs::ufs_vfsops::{ufs_check_export, ufs_fhtovp, ufs_init, ufs_root, ufs_start};
#[cfg(feature = "ffs2")]
use crate::ufs::ufs::ufsmount::UM_UFS2;
use crate::ufs::ufs::ufsmount::{UM_UFS1, Ufsmount, vfstoufs};

/// `ffs_vfsops`.
pub static FFS_VFSOPS: Vfsops = Vfsops {
    vfs_mount: ffs_mount,
    vfs_start: ufs_start,
    vfs_unmount: ffs_unmount,
    vfs_root: ufs_root,
    vfs_quotactl: ufs_quotactl,
    vfs_statfs: ffs_statfs,
    vfs_sync: ffs_sync,
    vfs_vget: ffs_vget,
    vfs_fhtovp: ffs_fhtovp,
    vfs_vptofh: ffs_vptofh,
    vfs_init: Some(ffs_init),
    vfs_sysctl: Some(ffs_sysctl),
    vfs_checkexp: ufs_check_export,
};

/// `ffs_vtbl`: the inode operations FFS gives the UFS layer.
pub static FFS_VTBL: InodeVtbl = InodeVtbl {
    iv_truncate: ffs_truncate,
    iv_update: ffs_update,
    iv_inode_alloc: ffs_inode_alloc,
    iv_inode_free: ffs_inode_free,
    iv_buf_alloc: ffs_balloc,
    iv_bufatoff: ffs_bufatoff,
};

/// `ffs_ino_pool`: memory pool for inodes.
pub static FFS_INO_POOL: Pool = Pool::new();
/// `ffs_dinode1_pool`: memory pool for UFS1 dinodes.
pub static FFS_DINODE1_POOL: Pool = Pool::new();
/// `ffs_dinode2_pool`: memory pool for UFS2 dinodes.
#[cfg(feature = "ffs2")]
pub static FFS_DINODE2_POOL: Pool = Pool::new();

/// `ffs_init`'s `done`: the pools are initialised.
pub static FFS_INIT_DONE: AtomicBool = AtomicBool::new(false);

/// The bytes of the in-core super-block allocation (see the module's deviations).
fn fs_allocsize(sbsize: i32) -> usize {
    (sbsize.max(0) as usize).max(size_of::<Fs>())
}

/// The super-block in a buffer the caller owns.
///
/// # Safety
///
/// The buffer is busy for the caller and mapped, it holds at least `size_of::<Fs>()` bytes
/// (its mapping is page-rounded), and nothing else reaches its data while the reference
/// lives.
unsafe fn fs_in_buf(bp: &Buf) -> &Fs {
    let p = bp.b_data.get();
    if p.is_null() {
        panic(format_args!("ffs: unmapped super-block buffer"));
    }
    // SAFETY: the caller's contract; the mapping is page-aligned and `Fs` is `Cell`s of
    // integers and raw pointers, valid for any bytes.
    unsafe { &*p.cast::<Fs>() }
}

/// `ffs_checkrange`: whether `ino` can name an inode of the mount (`ESTALE` if not), with
/// FFS2's lazy inode initialisation taken into account.
pub fn ffs_checkrange(mp: &'static Mount, ino: u32) -> Result<(), Errno> {
    let ump = vfstoufs(mp);
    let fs = ump.fs();
    if ino < ROOTINO || ino >= fs.fs_ncg.get() * fs.fs_ipg.get() {
        return Err(Errno::ESTALE);
    }

    // Need to check if inode is initialized because ffsv2 does lazy initialization and we
    // can get here from nfs_fhtovp
    if fs.fs_magic.get() != FS_UFS2_MAGIC {
        return Ok(());
    }

    let cg = ino_to_cg(fs, ino);

    let (bp, error) = bread(ump.devvp(), fsbtodb(fs, cgtod(fs, cg)), fs.fs_cgsize.get());
    if let Err(e) = error {
        // The C returns without releasing the buffer.
        brelse(bp);
        return Err(e);
    }

    // SAFETY: the buffer is ours (busy from bread) and mapped; the view dies before it is
    // released.
    let cgp = unsafe { CgBuf::new(bp) };
    if !cgp.cg_chkmagic() {
        brelse(bp);
        return Err(Errno::ESTALE);
    }
    let initediblk = cgp.cg().cg_initediblk.get();

    brelse(bp);

    if cg * fs.fs_ipg.get() + initediblk < ino {
        return Err(Errno::ESTALE);
    }

    Ok(())
}

/// `ffs_mountroot`: called by `main()` when ufs is going to be mounted as root.
pub fn ffs_mountroot() -> Result<(), Errno> {
    let Some(p) = curproc() else {
        panic(format_args!("ffs_mountroot: no curproc"));
    };

    // Get vnodes for swapdev and rootdev.
    set_swapdev_vp(None);
    let swapdev = swapdev();
    let rootdev = ROOTDEV.load(Ordering::Relaxed);
    let rvp = match bdevvp(swapdev).and_then(|svp| {
        set_swapdev_vp(svp);
        bdevvp(rootdev)
    }) {
        Ok(Some(rvp)) => rvp,
        Ok(None) | Err(_) => {
            kprintf!("ffs_mountroot: can't setup bdevvp's\n");
            if let Some(svp) = swapdev_vp() {
                vrele(svp);
            }
            return Err(Errno::ENODEV);
        }
    };
    set_rootvp(Some(rvp));

    let release = || {
        if let Some(svp) = swapdev_vp() {
            vrele(svp);
        }
        vrele(rvp);
    };

    let mp = match vfs_rootmountalloc(b"ffs", b"root_device") {
        Ok(mp) => mp,
        Err(e) => {
            release();
            return Err(e);
        }
    };

    if let Err(e) = ffs_mountfs(rvp, mp, p) {
        vfs_unbusy(mp);
        vfs_mount_free(mp);
        release();
        return Err(e);
    }

    // SAFETY: a new mount on no list, under the kernel lock.
    unsafe { MOUNTLIST.0.insert_tail(mp) };
    let ump = vfstoufs(mp);
    let fs = ump.fs();
    let (name, len) = mp.mntonname();
    fs_strlcpy_fsmnt(fs, &name[..len]);
    let mut st = mp.mnt_stat.get();
    let _ = ffs_statfs(mp, &mut st, p);
    mp.mnt_stat.set(st);
    vfs_unbusy(mp);
    crate::kern::kern_time::inittodr(fs.fs_time.get());

    Ok(())
}

/// `strlcpy(fs->fs_fsmnt, name, sizeof(fs->fs_fsmnt))`.
fn fs_strlcpy_fsmnt(fs: &Fs, name: &[u8]) {
    let mut m = [0u8; crate::ufs::ffs::fs::MAXMNTLEN];
    let name = name.split(|&c| c == 0).next().unwrap_or(&[]);
    let n = name.len().min(m.len() - 1);
    m[..n].copy_from_slice(&name[..n]);
    fs.fs_fsmnt.set(m);
}

/// `memset(dst, 0, MNAMELEN); strlcpy(dst, src, MNAMELEN)`.
fn mname_copy(dst: &mut [u8; MNAMELEN], src: &[u8]) {
    *dst = [0; MNAMELEN];
    let src = src.split(|&c| c == 0).next().unwrap_or(&[]);
    let n = src.len().min(MNAMELEN - 1);
    dst[..n].copy_from_slice(&src[..n]);
}

/// `ffs_mount` (`vfs_mount`): mount system call. `data` is the kernel copy of the user's
/// `struct ufs_args` (empty for the C's NULL).
pub fn ffs_mount(
    mp: &'static Mount,
    path: &[u8],
    data: &mut [u8],
    ndp: &mut Nameidata<'_>,
    p: &Proc,
) -> Result<(), Errno> {
    let args = UfsArgs::from_bytes(data);
    let mut ump: Option<&'static Ufsmount> = None;
    let mut ronly = 0;
    let fname: [u8; MNAMELEN];
    let mut fspec = [0u8; MNAMELEN];

    // If updating, check whether changing from read-only to read/write; if there is no
    // device name, that's all we do.
    let error: Result<(), Errno> = 'error_1: {
        let devvp: &'static Vnode;
        'success: {
            if mp.mnt_flag.get() & MNT_UPDATE != 0 {
                let u = vfstoufs(mp);
                ump = Some(u);
                let fs = u.fs();
                ronly = fs.fs_ronly.get();
                let mut error = Ok(());

                if ronly == 0 && mp.mnt_flag.get() & MNT_RDONLY != 0 {
                    // Flush any dirty data
                    let _ = VFS_SYNC(mp, MNT_WAIT, 0, p.p_ucred.get(), p);

                    // Get rid of files open for writing.
                    let mut flags = WRITECLOSE;
                    if args.is_none() {
                        flags |= IGNORECLEAN;
                    }
                    if mp.mnt_flag.get() & MNT_FORCE != 0 {
                        flags |= FORCECLOSE;
                    }
                    error = ffs_flushfiles(mp, flags, p);
                    mp.mnt_flag.set(mp.mnt_flag.get() | MNT_RDONLY);
                    ronly = 1;
                }

                if error.is_ok() && mp.mnt_flag.get() & MNT_RELOAD != 0 {
                    error = ffs_reload(mp, ndp.ni_cnd.cn_cred, p);
                }
                if let Err(e) = error {
                    break 'error_1 Err(e);
                }

                if ronly != 0 && mp.mnt_flag.get() & MNT_WANTRDWR != 0 {
                    if fs.fs_clean.get() == 0 {
                        if mp.mnt_flag.get() & MNT_FORCE != 0 {
                            kprintf!("WARNING: {} was not properly unmounted\n", fs.fsmnt());
                        } else {
                            kprintf!(
                                "WARNING: R/W mount of {} denied.  Filesystem is not clean - run fsck\n",
                                fs.fsmnt()
                            );
                            break 'error_1 Err(Errno::EROFS);
                        }
                    }

                    let Some(cd) = malloc(fs.fs_ncg.get() as usize, M_UFSMNT, M_WAITOK | M_ZERO)
                    else {
                        panic(format_args!("ffs_mount: no memory"));
                    };
                    fs.fs_contigdirs.set(cd.as_ptr());

                    ronly = 0;
                }
                let Some(a) = args else {
                    break 'success;
                };
                if a.fspec == 0 {
                    // Process export requests.
                    if let Err(e) =
                        vfs_export(mp, ptr::null_mut(), ptr::from_ref(&a.export_info).cast())
                    {
                        break 'error_1 Err(e);
                    }
                    break 'success;
                }
            }

            // Not an update, or updating the name: look up the name and verify that it
            // refers to a sensible block device.
            let Some(a) = args else {
                break 'error_1 Err(Errno::EINVAL);
            };
            if let Err(e) = copyinstr(a.fspec, &mut fspec) {
                break 'error_1 Err(e);
            }

            // disk_map(fspec, fname, MNAMELEN, DM_OPENBLCK) == -1: subr_disk.c is not ported,
            // so the name is used as given.
            let _ = crate::unported!("ffs_mount: disk_map (subr_disk.c)");
            fname = fspec;

            let flen = fname.iter().position(|&c| c == 0).unwrap_or(MNAMELEN);
            let mut nd = ndinit(LOOKUP, FOLLOW, NiDirp::Sys(&fname[..flen]), p);
            if let Err(e) = namei(&mut nd) {
                break 'error_1 Err(e);
            }
            let Some(vp) = nd.ni_vp else {
                break 'error_1 Err(Errno::ENOENT);
            };
            devvp = vp;

            let error: Result<(), Errno> = 'error_2: {
                if devvp.v_type.get() != VBLK {
                    break 'error_2 Err(Errno::ENOTBLK);
                }

                if major(devvp.v_rdev()) >= nblkdev() {
                    break 'error_2 Err(Errno::ENXIO);
                }

                let mut error = Ok(());
                if mp.mnt_flag.get() & MNT_UPDATE != 0 {
                    // UPDATE
                    // If it's not the same vnode, or at least the same device then it's not
                    // correct.
                    let Some(u) = ump else {
                        panic(format_args!("ffs_mount: update without ufsmount"));
                    };
                    if !ptr::eq(devvp, u.devvp()) {
                        if devvp.v_rdev() == u.devvp().v_rdev() {
                            vrele(devvp);
                        } else {
                            error = Err(Errno::EINVAL); // needs translation
                        }
                    } else {
                        vrele(devvp);
                    }
                    // Update device name only on success
                    if error.is_ok() {
                        // Save "mounted from" info for mount point (NULL pad)
                        mp.update_stat(|sp| {
                            mname_copy(&mut sp.f_mntfromname, &fname);
                            mname_copy(&mut sp.f_mntfromspec, &fspec);
                        });
                    }
                } else {
                    // Since this is a new mount, we want the names for the device and the
                    // mount point copied in. If an error occurs, the mountpoint is discarded
                    // by the upper level code.
                    mp.update_stat(|sp| {
                        mname_copy(&mut sp.f_mntonname, path);
                        mname_copy(&mut sp.f_mntfromname, &fname);
                        mname_copy(&mut sp.f_mntfromspec, &fspec);
                    });

                    error = ffs_mountfs(devvp, mp, p);
                }

                if error.is_err() {
                    break 'error_2 error;
                }

                // Initialize FS stat information in mount struct; uses both
                // mp->mnt_stat.f_mntonname and mp->mnt_stat.f_mntfromname
                //
                // This code is common to root and non-root mounts
                mp.update_stat(|sp| {
                    sp.mount_info.__align[..UfsArgs::SIZE].copy_from_slice(&data[..UfsArgs::SIZE]);
                });
                let mut st = mp.mnt_stat.get();
                let _ = VFS_STATFS(mp, &mut st, p);
                mp.mnt_stat.set(st);
                Ok(())
            };
            if let Err(e) = error {
                // error_2: error with devvp held
                vrele(devvp);
                break 'error_1 Err(e);
            }
        }

        // success:
        if !path.is_empty() && mp.mnt_flag.get() & MNT_UPDATE != 0 {
            // Update clean flag after changing read-onlyness.
            let Some(u) = ump else {
                panic(format_args!("ffs_mount: update without ufsmount"));
            };
            let fs = u.fs();
            if ronly != fs.fs_ronly.get() {
                fs.fs_ronly.set(ronly);
                fs.fs_clean
                    .set(i8::from(ronly != 0 && fs.fs_flags.get() & FS_UNCLEAN == 0));
                if ronly != 0
                    && let Some(cd) = NonNull::new(fs.fs_contigdirs.get())
                {
                    free(cd, M_UFSMNT, fs.fs_ncg.get() as usize);
                }
            }
            let _ = ffs_sbupdate(u, MNT_WAIT);
        }
        return Ok(());
    };

    // error_1: no state to back out
    error
}

/// `ffs_reload_vnode`: steps 4 to 6 of `ffs_reload` for one vnode.
fn ffs_reload_vnode(
    vp: &'static Vnode,
    fs: &Fs,
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
        panic(format_args!("ffs_reload: dirty2"));
    }

    // Step 6: re-read inode data for all active vnodes.
    let ip = vtoi(vp);

    let (bp, error) = bread(
        devvp,
        fsbtodb(fs, ino_to_fsba(fs, ip.i_number.get())),
        fs.fs_bsize.get(),
    );
    if let Err(e) = error {
        brelse(bp);
        vput(vp);
        return Err(e);
    }

    {
        // SAFETY: the buffer is ours (busy from bread) and mapped; the slice dies before it
        // is released.
        let data = unsafe { bp.data() };
        let idx = ino_to_fsbo(fs, ip.i_number.get());
        if fs.fs_magic.get() == FS_UFS1_MAGIC {
            let d = dinode1_at(data, idx);
            ip.with_din1(|din| *din = d);
        } else {
            #[cfg(feature = "ffs2")]
            {
                let d = dinode2_at(data, idx);
                ip.with_din2(|din| *din = d);
            }
        }
    }
    ip.i_effnlink.set(ip.dip_nlink());
    brelse(bp);
    vput(vp);
    Ok(())
}

/// `ffs_reload`: reload all incore data for a filesystem (used after running fsck on the
/// root filesystem and finding things to fix). The filesystem must be mounted read-only.
///
/// Things to do to update the mount:
///  1) invalidate all cached meta-data.
///  2) re-read superblock from disk.
///  3) re-read summary information from disk.
///  4) invalidate all inactive vnodes.
///  5) invalidate all cached file data.
///  6) re-read inode data for all active vnodes.
pub fn ffs_reload(mountp: &'static Mount, cred: *const Ucred, p: &Proc) -> Result<(), Errno> {
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
        panic(format_args!("ffs_reload: dirty1"));
    }

    // Step 2: re-read superblock from disk.
    let fs = ump.fs();

    let (bp, error) = bread(
        devvp,
        fs.fs_sblockloc.get() / DEV_BSIZE as i64,
        SBSIZE as i32,
    );
    if let Err(e) = error {
        brelse(bp);
        return Err(e);
    }

    {
        // SAFETY: the buffer is ours (busy from bread) and mapped, SBSIZE bytes.
        let newfs = unsafe { fs_in_buf(bp) };
        if !ffs_validate(newfs) {
            brelse(bp);
            return Err(Errno::EINVAL);
        }

        // Copy pointer fields back into superblock before copying in new superblock. These
        // should really be in the ufsmount. Note that important parameters (eg fs_ncg) are
        // unchanged.
        newfs.fs_csp.set(fs.fs_csp.get());
        newfs.fs_maxcluster.set(fs.fs_maxcluster.get());
        newfs.fs_ronly.set(fs.fs_ronly.get());
    }
    let sbsize = fs.fs_sbsize.get() as usize;
    // SAFETY: the in-core super-block holds at least `fs_sbsize` bytes (`fs_allocsize`) and
    // the buffer `SBSIZE` >= `fs_sbsize` (ffs_validate); they do not overlap.
    unsafe {
        ptr::copy_nonoverlapping(
            bp.b_data.get(),
            ptr::from_ref(fs).cast_mut().cast::<u8>(),
            sbsize,
        );
    }
    if (fs.fs_sbsize.get() as usize) < SBSIZE {
        bp.set(B_INVAL);
    }
    brelse(bp);
    ump.um_maxsymlinklen.set(fs.fs_maxsymlinklen.get() as u32);
    ffs1_compat_read(fs, ump, fs.fs_sblockloc.get());
    ffs_oldfscompat(fs);
    let mut st = mountp.mnt_stat.get();
    let _ = ffs_statfs(mountp, &mut st, p);
    mountp.mnt_stat.set(st);
    // Step 3: re-read summary information from disk.
    read_summary(fs, devvp)?;
    // We no longer know anything about clusters per cylinder group.
    if fs.fs_contigsumsize.get() > 0 {
        for i in 0..fs.fs_ncg.get() {
            fs.set_maxcluster(i, fs.fs_contigsumsize.get());
        }
    }

    vfs_mount_foreach_vnode(mountp, &mut |vp| ffs_reload_vnode(vp, fs, p, cred, devvp))
}

/// Reads the cylinder group summaries (`fs_cssize` bytes at `fs_csaddr`) into `fs_csp`.
fn read_summary(fs: &Fs, devvp: &'static Vnode) -> Result<(), Errno> {
    let blks = howmany(fs.fs_cssize.get() as usize, fs.fs_fsize.get() as usize) as i32;
    let mut space = fs.fs_csp.get().cast::<u8>();
    let mut i = 0;
    while i < blks {
        let mut size = fs.fs_bsize.get();
        if i + fs.fs_frag.get() > blks {
            size = (blks - i) * fs.fs_fsize.get();
        }
        let (bp, error) = bread(devvp, fsbtodb(fs, fs.fs_csaddr.get() + i64::from(i)), size);
        if let Err(e) = error {
            brelse(bp);
            return Err(e);
        }
        // SAFETY: `fs_csp` holds `howmany(fs_cssize, fs_fsize)` fragments' worth of bytes
        // (`ffs_mountfs` allocates `fs_cssize` rounded to fragments), and the buffer `size`;
        // they do not overlap.
        unsafe {
            ptr::copy_nonoverlapping(bp.b_data.get(), space, size as usize);
            space = space.add(size as usize);
        }
        brelse(bp);
        i += fs.fs_frag.get();
    }
    Ok(())
}

/// `ffs_validate`: checks if a super block is sane enough to be mounted.
pub fn ffs_validate(fsp: &Fs) -> bool {
    #[cfg(feature = "ffs2")]
    if fsp.fs_magic.get() != FS_UFS2_MAGIC && fsp.fs_magic.get() != FS_UFS1_MAGIC {
        return false; // Invalid magic
    }
    #[cfg(not(feature = "ffs2"))]
    if fsp.fs_magic.get() != FS_UFS1_MAGIC {
        return false; // Invalid magic
    }

    if fsp.fs_bsize.get() as u32 > MAXBSIZE as u32 {
        return false; // Invalid block size
    }

    if (fsp.fs_bsize.get() as u32) < size_of::<Fs>() as u32 {
        return false; // Invalid block size
    }

    if fsp.fs_sbsize.get() as u32 > SBSIZE as u32 {
        return false; // Invalid super block size
    }

    let frag = fsp.fs_frag.get() as u32;
    if frag > MAXFRAG as u32 || FRAGTBL[frag as usize].is_none() {
        return false; // Invalid number of fragments
    }

    if fsp.fs_inodefmt.get() == FS_42INODEFMT {
        return false; // Obsolete format, support broken in 2014
    }
    if fsp.fs_maxsymlinklen.get() <= 0 {
        return false; // Invalid max size of short symlink
    }

    true // Super block is okay
}

/// `sbtry[]`: possible locations for the super-block.
pub const SBTRY: [i32; 4] = SBLOCKSEARCH;

/// `ffs_mountfs`: common code for mount and mountroot: read and check the super-block on
/// `devvp`, set up the in-core super-block and summaries and the `ufsmount`, and mark the
/// file system dirty when mounting read-write.
pub fn ffs_mountfs(devvp: &'static Vnode, mp: &'static Mount, p: &Proc) -> Result<(), Errno> {
    let dev = devvp.v_rdev();
    let cred = p.p_ucred.get();
    // Disallow multiple mounts of the same device. Disallow mounting of a device that is
    // currently in use (except for root, which might share swap device for miniroot). Flush
    // out any old buffers remaining from a previous use.
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

    let mut bp: Option<&'static Buf> = None;
    let mut ump: Option<&'static Ufsmount> = None;

    let error: Errno = 'out: {
        // Try reading the super-block in each of its possible locations.
        let mut i = 0;
        let mut sbloc = 0;
        while SBTRY[i] != -1 {
            if let Some(b) = bp.take() {
                b.set(B_NOCACHE);
                brelse(b);
            }

            let (b, error) = bread(devvp, i64::from(SBTRY[i]) / DEV_BSIZE as i64, SBSIZE as i32);
            bp = Some(b);
            if let Err(e) = error {
                break 'out e;
            }

            // SAFETY: the buffer is ours (busy from bread) and mapped, SBSIZE bytes.
            let fs = unsafe { fs_in_buf(b) };
            sbloc = SBTRY[i];

            // Do not look for an FFS1 file system at SBLOCK_UFS2. Doing so will find the
            // wrong super-block for file systems with 64k block size.
            if !(fs.fs_magic.get() == FS_UFS1_MAGIC && sbloc == SBLOCK_UFS2) && ffs_validate(fs) {
                break; // Super block validated
            }
            i += 1;
        }

        if SBTRY[i] == -1 {
            break 'out Errno::EINVAL;
        }
        let Some(b) = bp else {
            panic(format_args!("ffs_mountfs: no super-block buffer"));
        };
        // SAFETY: as above.
        let bfs = unsafe { fs_in_buf(b) };

        bfs.fs_fmod.set(0);
        bfs.fs_flags.set(bfs.fs_flags.get() & !FS_UNCLEAN);
        if bfs.fs_clean.get() == 0 {
            if ronly || mp.mnt_flag.get() & MNT_FORCE != 0 {
                kprintf!("WARNING: {} was not properly unmounted\n", bfs.fsmnt());
            } else {
                kprintf!(
                    "WARNING: R/W mount of {} denied.  Filesystem is not clean - run fsck\n",
                    bfs.fsmnt()
                );
                break 'out Errno::EROFS;
            }
        }

        if bfs.fs_postblformat.get() == FS_42POSTBLFMT && !ronly {
            kprintf!(
                "ffs_mountfs(): obsolete rotational table format, please use fsck_ffs(8) -c 1\n"
            );
            break 'out Errno::EROFS;
        }

        let Some(um) = malloc(size_of::<Ufsmount>(), M_UFSMNT, M_WAITOK | M_ZERO) else {
            panic(format_args!("ffs_mountfs: no memory"));
        };
        let um = um.cast::<Ufsmount>();
        // SAFETY: a fresh allocation of a `Ufsmount`'s size from malloc, aligned for it.
        unsafe { ptr::write(um.as_ptr(), Ufsmount::new()) };
        // SAFETY: just initialised; freed only by ffs_unmount or the error path below.
        let u: &'static Ufsmount = unsafe { &*um.as_ptr() };
        ump = Some(u);
        let sbsize = bfs.fs_sbsize.get();
        let Some(fsmem) = malloc(fs_allocsize(sbsize), M_UFSMNT, M_WAITOK | M_ZERO) else {
            panic(format_args!("ffs_mountfs: no memory"));
        };

        if bfs.fs_magic.get() == FS_UFS1_MAGIC {
            u.um_fstype.set(UM_UFS1);
        } else {
            #[cfg(feature = "ffs2")]
            u.um_fstype.set(UM_UFS2);
        }

        // SAFETY: `fsmem` holds at least `fs_sbsize` bytes, the buffer `SBSIZE` >=
        // `fs_sbsize` (ffs_validate); they do not overlap.
        unsafe { ptr::copy_nonoverlapping(b.b_data.get(), fsmem.as_ptr(), sbsize as usize) };
        // SAFETY: `fsmem` is at least `size_of::<Fs>()` bytes from malloc (aligned), and
        // `Fs` is valid for any bytes; it lives until the unmount frees it.
        let fs: &'static Fs = unsafe { &*fsmem.as_ptr().cast::<Fs>() };
        u.um_fs.set(Some(fs));
        if (sbsize as usize) < SBSIZE {
            b.set(B_INVAL);
        }
        brelse(b);
        bp = None;

        ffs1_compat_read(fs, u, i64::from(sbloc));

        if fs.fs_clean.get() == 0 {
            fs.fs_flags.set(fs.fs_flags.get() | FS_UNCLEAN);
        }
        fs.fs_ronly.set(i8::from(ronly));
        let mut size = fs.fs_cssize.get() as usize;
        if fs.fs_contigsumsize.get() > 0 {
            size += fs.fs_ncg.get() as usize * size_of::<i32>();
        }
        // The summaries are read a fragment at a time: room for whole fragments.
        let cssize_frags = howmany(fs.fs_cssize.get() as usize, fs.fs_fsize.get() as usize)
            * fs.fs_fsize.get() as usize;
        let allocsize = size.max(cssize_frags + (size - fs.fs_cssize.get() as usize));
        let Some(space) = malloc(allocsize, M_UFSMNT, M_WAITOK) else {
            panic(format_args!("ffs_mountfs: no memory"));
        };
        fs.fs_csp.set(space.as_ptr().cast());
        if let Err(e) = read_summary(fs, devvp) {
            free(space, M_UFSMNT, allocsize);
            break 'out e;
        }
        if fs.fs_contigsumsize.get() > 0 {
            // SAFETY: the counters follow the `fs_cssize` bytes of summaries in `space`.
            let lp = unsafe { space.as_ptr().add(allocsize - fs.fs_ncg.get() as usize * 4) };
            fs.fs_maxcluster.set(lp.cast());
            for i in 0..fs.fs_ncg.get() {
                fs.set_maxcluster(i, fs.fs_contigsumsize.get());
            }
        }
        mp.mnt_data.set(um.as_ptr().cast());
        mp.update_stat(|sp| {
            sp.f_fsid.val[0] = dev;
            // Use on-disk fsid if it exists, else fake it
            if fs.fs_id[0].get() != 0 && fs.fs_id[1].get() != 0 {
                sp.f_fsid.val[1] = fs.fs_id[1].get();
            } else {
                sp.f_fsid.val[1] = mp.vfc().vfc_typenum;
            }
            sp.f_namemax = MAXNAMLEN as u32;
        });
        mp.mnt_flag.set(mp.mnt_flag.get() | MNT_LOCAL);
        u.um_mountp.set(Some(mp));
        u.um_dev.set(dev);
        u.um_devvp.set(Some(devvp));
        u.um_nindir.set(fs.fs_nindir.get() as u64);
        u.um_bptrtodb.set(fs.fs_fsbtodb.get() as u64);
        u.um_seqinc.set(fs.fs_frag.get() as u64);
        u.um_maxsymlinklen.set(fs.fs_maxsymlinklen.get() as u32);
        for q in 0..MAXQUOTAS {
            u.um_quotas[q].set(None);
        }

        if let Some(si) = devvp.v_specinfo() {
            si.si_mountpoint.set(Some(mp));
        }
        ffs_oldfscompat(fs);

        if ronly {
            fs.fs_contigdirs.set(ptr::null_mut());
        } else {
            let Some(cd) = malloc(fs.fs_ncg.get() as usize, M_UFSMNT, M_WAITOK | M_ZERO) else {
                panic(format_args!("ffs_mountfs: no memory"));
            };
            fs.fs_contigdirs.set(cd.as_ptr());
        }

        // Set FS local "last mounted on" information (NULL pad)
        let (name, len) = mp.mntonname();
        fs_strlcpy_fsmnt(fs, &name[..len]);

        // XXX
        // Limit max file size. Even though ffs can handle files up to 16TB, we do limit the
        // max file to 2^31 pages to prevent overflow of a 32-bit unsigned int. The buffer
        // cache has its own checks but a little added paranoia never hurts.
        u.um_savedmaxfilesize.set(fs.fs_maxfilesize.get()); // XXX
        let maxfilesize = fs_kernmaxfilesize(PAGE_SIZE as u64, fs);
        if fs.fs_maxfilesize.get() > maxfilesize {
            fs.fs_maxfilesize.set(maxfilesize); // XXX
        }
        if !ronly {
            fs.fs_fmod.set(1);
            fs.fs_clean.set(0);
            if let Err(Errno::EROFS) = ffs_sbupdate(u, MNT_WAIT) {
                break 'out Errno::EROFS;
            }
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
        if let Some(fs) = u.um_fs.get() {
            let size = fs_allocsize(fs.fs_sbsize.get());
            free(NonNull::from(fs).cast(), M_UFSMNT, size);
        }
        free(NonNull::from(u).cast(), M_UFSMNT, size_of::<Ufsmount>());
        mp.mnt_data.set(ptr::null_mut());
    }
    Err(error)
}

/// `ffs_oldfscompat`: sanity checks for old file systems.
pub fn ffs_oldfscompat(fs: &Fs) -> i32 {
    fs.fs_npsect.set(fs.fs_npsect.get().max(fs.fs_nsect.get())); // XXX
    fs.fs_interleave.set(fs.fs_interleave.get().max(1)); // XXX
    if fs.fs_postblformat.get() == FS_42POSTBLFMT {
        fs.fs_nrpos.set(8); // XXX
    }
    if fs.fs_inodefmt.get() < FS_44INODEFMT {
        let mut sizepb = fs.fs_bsize.get() as u64; // XXX
        let mut maxfilesize = (fs.fs_bsize.get() as u64) * NDADDR as u64 - 1; // XXX
        for _ in 0..NIADDR {
            sizepb = sizepb.wrapping_mul(nindir(fs) as u64); // XXX
            maxfilesize = maxfilesize.wrapping_add(sizepb); // XXX
        }
        fs.fs_maxfilesize.set(maxfilesize);
        fs.fs_qbmask.set(i64::from(!fs.fs_bmask.get())); // XXX
        fs.fs_qfmask.set(i64::from(!fs.fs_fmask.get())); // XXX
    } // XXX
    if fs.fs_avgfilesize.get() == 0 {
        fs.fs_avgfilesize.set(AVFILESIZ); // XXX
    }
    if fs.fs_avgfpdir.get() == 0 {
        fs.fs_avgfpdir.set(AFPDIR); // XXX
    }
    0
}

/// `ffs1_compat_read`: auxiliary function for reading FFS1 super blocks: copy the old
/// fields into the FFS2 ones the kernel uses.
pub fn ffs1_compat_read(fs: &Fs, _ump: &Ufsmount, sbloc: i64) {
    if fs.fs_magic.get() == FS_UFS2_MAGIC {
        return; // UFS2
    }
    fs.fs_flags.set(fs.fs_ffs1_flags.get() as u8 as u32);
    fs.fs_sblockloc.set(sbloc);
    fs.fs_maxbsize.set(fs.fs_bsize.get());
    fs.fs_time.set(i64::from(fs.fs_ffs1_time.get()));
    fs.fs_size.set(i64::from(fs.fs_ffs1_size.get()));
    fs.fs_dsize.set(i64::from(fs.fs_ffs1_dsize.get()));
    fs.fs_csaddr.set(i64::from(fs.fs_ffs1_csaddr.get()));
    fs.fs_cstotal
        .cs_ndir
        .set(i64::from(fs.fs_ffs1_cstotal.cs_ndir.get()));
    fs.fs_cstotal
        .cs_nbfree
        .set(i64::from(fs.fs_ffs1_cstotal.cs_nbfree.get()));
    fs.fs_cstotal
        .cs_nifree
        .set(i64::from(fs.fs_ffs1_cstotal.cs_nifree.get()));
    fs.fs_cstotal
        .cs_nffree
        .set(i64::from(fs.fs_ffs1_cstotal.cs_nffree.get()));
    fs.fs_ffs1_flags
        .set((fs.fs_ffs1_flags.get() as u8 | FS_FLAGS_UPDATED as u8) as i8);
}

/// `ffs1_compat_write`: auxiliary function for writing FFS1 super blocks.
pub fn ffs1_compat_write(fs: &Fs, _ump: &Ufsmount) {
    if fs.fs_magic.get() != FS_UFS1_MAGIC {
        return; // UFS2
    }

    fs.fs_ffs1_time.set(fs.fs_time.get() as i32);
    fs.fs_ffs1_cstotal
        .cs_ndir
        .set(fs.fs_cstotal.cs_ndir.get() as i32);
    fs.fs_ffs1_cstotal
        .cs_nbfree
        .set(fs.fs_cstotal.cs_nbfree.get() as i32);
    fs.fs_ffs1_cstotal
        .cs_nifree
        .set(fs.fs_cstotal.cs_nifree.get() as i32);
    fs.fs_ffs1_cstotal
        .cs_nffree
        .set(fs.fs_cstotal.cs_nffree.get() as i32);
}

/// The bytes of the summary area allocation of a mounted file system (`ffs_mountfs`).
fn summary_allocsize(fs: &Fs) -> usize {
    let mut size = fs.fs_cssize.get() as usize;
    let mut extra = 0;
    if fs.fs_contigsumsize.get() > 0 {
        extra = fs.fs_ncg.get() as usize * size_of::<i32>();
        size += extra;
    }
    let cssize_frags = howmany(fs.fs_cssize.get() as usize, fs.fs_fsize.get() as usize)
        * fs.fs_fsize.get() as usize;
    size.max(cssize_frags + extra)
}

/// `ffs_unmount` (`vfs_unmount`): unmount system call.
pub fn ffs_unmount(mp: &'static Mount, mntflags: i32, p: &Proc) -> Result<(), Errno> {
    let mut flags = 0;
    if mntflags & MNT_FORCE != 0 {
        flags |= FORCECLOSE;
    }

    let ump = vfstoufs(mp);
    let fs = ump.fs();
    ffs_flushfiles(mp, flags, p)?;

    if fs.fs_ronly.get() == 0 {
        fs.fs_clean
            .set(i8::from(fs.fs_flags.get() & FS_UNCLEAN == 0));
        let error = ffs_sbupdate(ump, MNT_WAIT);
        // ignore write errors if mounted RW on read-only device
        if let Err(e) = error
            && e != Errno::EROFS
        {
            fs.fs_clean.set(0);
            return Err(e);
        }
        if let Some(cd) = NonNull::new(fs.fs_contigdirs.get()) {
            free(cd, M_UFSMNT, fs.fs_ncg.get() as usize);
        }
    }
    let devvp = ump.devvp();
    if let Some(si) = devvp.v_specinfo() {
        si.si_mountpoint.set(None);
    }

    let _ = vn_lock(devvp, LK_EXCLUSIVE | LK_RETRY);
    let _ = vinvalbuf(devvp, V_SAVE, NOCRED, Some(p), 0, INFSLP);
    let omode = if fs.fs_ronly.get() != 0 {
        FREAD
    } else {
        FREAD | FWRITE
    };
    let _ = VOP_CLOSE(devvp, omode, NOCRED, Some(p));
    vput(devvp);
    if let Some(csp) = NonNull::new(fs.fs_csp.get().cast::<u8>()) {
        free(csp, M_UFSMNT, summary_allocsize(fs));
    }
    let fssize = fs_allocsize(fs.fs_sbsize.get());
    free(NonNull::from(fs).cast(), M_UFSMNT, fssize);
    free(NonNull::from(ump).cast(), M_UFSMNT, size_of::<Ufsmount>());
    mp.mnt_data.set(ptr::null_mut());
    mp.mnt_flag.set(mp.mnt_flag.get() & !MNT_LOCAL);
    Ok(())
}

/// `ffs_flushfiles`: flush out all the files in a filesystem.
pub fn ffs_flushfiles(mp: &'static Mount, flags: i32, p: &Proc) -> Result<(), Errno> {
    let ump = vfstoufs(mp);
    if mp.mnt_flag.get() & MNT_QUOTA != 0 {
        vflush(mp, None, SKIPSYSTEM | flags)?;
        for i in 0..MAXQUOTAS {
            if ump.um_quotas[i].get().is_none() {
                continue;
            }
            let _ = quotaoff(p, mp, i);
        }
        // Here we fall through to vflush again to ensure that we have gotten rid of all the
        // system vnodes.
    }

    // Flush all the files.
    vflush(mp, None, flags)?;
    // Flush filesystem metadata.
    let devvp = ump.devvp();
    let _ = vn_lock(devvp, LK_EXCLUSIVE | LK_RETRY);
    let error = VOP_FSYNC(devvp, p.p_ucred.get(), MNT_WAIT, p);
    let _ = VOP_UNLOCK(devvp);
    error
}

/// `ffs_statfs` (`vfs_statfs`): get file system statistics.
pub fn ffs_statfs(mp: &'static Mount, sbp: &mut Statfs, _p: &Proc) -> Result<(), Errno> {
    let ump = vfstoufs(mp);
    let fs = ump.fs();

    #[cfg(feature = "ffs2")]
    if fs.fs_magic.get() != FS_MAGIC && fs.fs_magic.get() != FS_UFS2_MAGIC {
        panic(format_args!("ffs_statfs"));
    }
    #[cfg(not(feature = "ffs2"))]
    if fs.fs_magic.get() != FS_MAGIC {
        panic(format_args!("ffs_statfs"));
    }

    sbp.f_bsize = fs.fs_fsize.get() as u32;
    sbp.f_iosize = fs.fs_bsize.get() as u32;
    sbp.f_blocks = fs.fs_dsize.get() as u64;
    sbp.f_bfree = (fs.fs_cstotal.cs_nbfree.get() * i64::from(fs.fs_frag.get())
        + fs.fs_cstotal.cs_nffree.get()) as u64;
    sbp.f_bavail = sbp.f_bfree as i64 - (fs.fs_dsize.get() * i64::from(fs.fs_minfree.get()) / 100);
    sbp.f_files = u64::from(fs.fs_ncg.get() * fs.fs_ipg.get() - ROOTINO);
    sbp.f_ffree = fs.fs_cstotal.cs_nifree.get() as u64;
    sbp.f_favail = sbp.f_ffree as i64;
    copy_statfs_info(sbp, mp);

    Ok(())
}

/// `struct ffs_sync_args`.
struct FfsSyncArgs<'a> {
    /// `allerror`.
    allerror: Result<(), Errno>,
    /// `p`.
    p: &'a Proc,
    /// `waitfor`.
    waitfor: i32,
    /// `nlink0`.
    nlink0: i32,
    /// `inflight`.
    inflight: i32,
    /// `cred`.
    cred: *const Ucred,
}

/// `ffs_sync_vnode`: write back one (modified) inode for `ffs_sync`.
fn ffs_sync_vnode(vp: &'static Vnode, fsa: &mut FfsSyncArgs<'_>) -> Result<(), Errno> {
    if vp.v_type.get() == VNON {
        return Ok(());
    }

    let ip = vtoi(vp);
    let mut nlink0 = 0;

    // If unmounting or converting rw to ro, then stop deferring timestamp writes.
    if fsa.waitfor == MNT_WAIT && ip.i_flag.get() & IN_LAZYMOD != 0 {
        ip.set_flag(IN_MODIFIED);
        let _ = UFS_UPDATE(ip, 1);
    }

    if ip.i_effnlink.get() == 0 {
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
            fsa.inflight = (fsa.inflight + 1).min(65536);
            break 'end;
        }

        if let Err(e) = VOP_FSYNC(vp, fsa.cred, fsa.waitfor, fsa.p) {
            fsa.allerror = Err(e);
        }
        let _ = VOP_UNLOCK(vp);
        vrele(vp);
    }

    // end:
    fsa.nlink0 = (fsa.nlink0 + nlink0).min(65536);
    Ok(())
}

/// `ffs_sync` (`vfs_sync`): go through the disk queues to initiate sandbagged IO; go
/// through the inodes to write those that have been modified; initiate the writing of the
/// super block if it has been modified.
///
/// Should always be called with the mount point locked.
pub fn ffs_sync(
    mp: &'static Mount,
    waitfor: i32,
    stall: i32,
    cred: *const Ucred,
    p: &Proc,
) -> Result<(), Errno> {
    let ump = vfstoufs(mp);
    let fs = ump.fs();
    let mut allerror = Ok(());
    // Write back modified superblock. Consistency check that the superblock is still in the
    // buffer cache.
    if fs.fs_fmod.get() != 0 && fs.fs_ronly.get() != 0 {
        kprintf!("fs = {}\n", fs.fsmnt());
        panic(format_args!("update: rofs mod"));
    }

    // Write back each (modified) inode.
    let mut fsa = FfsSyncArgs {
        allerror: Ok(()),
        p,
        cred,
        waitfor,
        nlink0: 0,
        inflight: 0,
    };

    // Don't traverse the vnode list if we want to skip all of them.
    if waitfor != MNT_LAZY {
        let _ = vfs_mount_foreach_vnode(mp, &mut |vp| ffs_sync_vnode(vp, &mut fsa));
        allerror = fsa.allerror;
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
    let _ = qsync(mp);
    // Write back modified superblock.
    let clean = fs.fs_clean.get();
    let fmod = fs.fs_fmod.get();
    if stall != 0 && fs.fs_ronly.get() == 0 {
        fs.fs_fmod.set(1);
        if allerror.is_ok() && fsa.nlink0 == 0 && fsa.inflight == 0 {
            fs.fs_clean
                .set(i8::from(fs.fs_flags.get() & FS_UNCLEAN == 0));
        } else {
            fs.fs_clean.set(0);
        }
    }
    if fs.fs_fmod.get() != 0
        && let Err(e) = ffs_sbupdate(ump, waitfor)
    {
        allerror = Err(e);
    }
    fs.fs_clean.set(clean);
    fs.fs_fmod.set(fmod);

    allerror
}

/// `ffs_vget` (`vfs_vget`): look up a FFS dinode number to find its incore vnode, otherwise
/// read it in from disk. If it is in core, wait for the lock bit to clear, then return the
/// inode locked. Detection and handling of mount points must be done by the calling routine.
pub fn ffs_vget(mp: &'static Mount, ino: Ino) -> Result<&'static Vnode, Errno> {
    if ino > u64::from(Ufsino::MAX) {
        panic(format_args!("ffs_vget: alien ino_t {}", ino));
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
        let vp = getnewvnode(VT_UFS, Some(mp), &FFS_VOPS)?;

        let Some(mem) = pool_get(&FFS_INO_POOL, PR_WAITOK | PR_ZERO) else {
            panic(format_args!("ffs_vget: no inode"));
        };
        let ipp = mem.cast::<Inode>();
        // SAFETY: a fresh `ffs_ino_pool` item, sized and aligned for an `Inode`.
        unsafe { ptr::write(ipp.as_ptr(), Inode::new()) };
        // SAFETY: just initialised; freed only by `ffs_reclaim`.
        let ip: &'static Inode = unsafe { &*ipp.as_ptr() };
        rrw_init_flags(&ip.i_lock, "inode", RWL_DUPOK | RWL_IS_VNODE);
        ip.i_ump.set(Some(ump));
        vref(ump.devvp());
        vp.v_data.set(ipp.as_ptr().cast());
        ip.i_vnode.set(Some(vp));
        let fs = ump.fs();
        ip.i_fs.set(Some(fs));
        ip.i_dev.set(dev);
        ip.i_number.set(ino);
        ip.i_vtbl.set(Some(&FFS_VTBL));

        // Put it onto its hash chain and lock it so that other requests for this inode will
        // block if they arrive while we are sleeping waiting for old data structures to be
        // purged or for the contents of the disk portion of this inode to be read.
        if let Err(error) = ufs_ihashins(ip) {
            // VOP_INACTIVE will treat this as a stale file and recycle it quickly
            vrele(vp);

            if error == Errno::EEXIST {
                continue;
            }

            return Err(error);
        }

        // Read in the disk contents for the inode, copy into the inode.
        let (bp, error) = bread(
            ump.devvp(),
            fsbtodb(fs, ino_to_fsba(fs, ino)),
            fs.fs_bsize.get(),
        );
        if let Err(e) = error {
            // The inode does not contain anything useful, so it would be misleading to leave
            // it on its hash chain. With mode still zero, it will be unlinked and returned
            // to the free list by vput().
            vput(vp);
            brelse(bp);
            return Err(e);
        }

        {
            // SAFETY: the buffer is ours (busy from bread) and mapped; the slice dies
            // before it is released.
            let data = unsafe { bp.data() };
            let idx = ino_to_fsbo(fs, ino);
            #[cfg(feature = "ffs2")]
            let ufs2 = ump.um_fstype.get() == UM_UFS2;
            #[cfg(not(feature = "ffs2"))]
            let ufs2 = false;
            if ufs2 {
                #[cfg(feature = "ffs2")]
                {
                    let Some(d) = pool_get(&FFS_DINODE2_POOL, PR_WAITOK) else {
                        panic(format_args!("ffs_vget: no dinode"));
                    };
                    let d = d.cast::<Ufs2Dinode>();
                    // SAFETY: a fresh `ffs_dinode2_pool` item, sized and aligned for it.
                    unsafe { ptr::write(d.as_ptr(), dinode2_at(data, idx)) };
                    ip.dinode_u.set(d.as_ptr().cast());
                }
            } else {
                let Some(d) = pool_get(&FFS_DINODE1_POOL, PR_WAITOK) else {
                    panic(format_args!("ffs_vget: no dinode"));
                };
                let d = d.cast::<Ufs1Dinode>();
                // SAFETY: a fresh `ffs_dinode1_pool` item, sized and aligned for it.
                unsafe { ptr::write(d.as_ptr(), dinode1_at(data, idx)) };
                ip.dinode_u.set(d.as_ptr().cast());
            }
        }

        brelse(bp);

        ip.i_effnlink.set(ip.dip_nlink());

        // Initialize the vnode from the inode, check for aliases. Note that the underlying
        // vnode may have changed.
        let vp = match ffs_vinit(mp, vp) {
            Ok(vp) => vp,
            Err(e) => {
                vput(vp);
                return Err(e);
            }
        };

        // Set up a generation number for this inode if it does not already have one. This
        // should only happen on old filesystems.
        if ip.dip_gen() == 0 {
            while ip.dip_gen() == 0 {
                ip.dip_set_gen(arc4random());
            }
            if vp
                .v_mount
                .get()
                .is_some_and(|m| m.mnt_flag.get() & MNT_RDONLY == 0)
            {
                ip.set_flag(IN_MODIFIED);
            }
        }

        // Ensure that uid and gid are correct. This is a temporary fix until fsck has been
        // changed to do the update.
        if fs.fs_magic.get() == FS_UFS1_MAGIC && fs.fs_inodefmt.get() < FS_44INODEFMT {
            ip.with_din1(|d| {
                d.di_uid = u32::from(d.di_ouid());
                d.di_gid = u32::from(d.di_ogid());
            });
        }

        return Ok(vp);
    }
}

/// `ffs_fhtovp` (`vfs_fhtovp`): file handle to vnode. Have to be really careful about stale
/// file handles.
pub fn ffs_fhtovp(mp: &'static Mount, fhp: &Fid) -> Result<&'static Vnode, Errno> {
    let ufhp = Ufid::from_fid(fhp);
    if usize::from(ufhp.ufid_len) != size_of::<Ufid>() {
        return Err(Errno::EINVAL);
    }

    ffs_checkrange(mp, ufhp.ufid_ino)?;

    ufs_fhtovp(mp, &ufhp)
}

/// `ffs_vptofh` (`vfs_vptofh`): vnode pointer to file handle.
pub fn ffs_vptofh(vp: &'static Vnode, fhp: &mut Fid) -> Result<(), Errno> {
    let ip = vtoi(vp);
    Ufid {
        ufid_len: size_of::<Ufid>() as u16,
        ufid_pad: fhp.fid_reserved,
        ufid_ino: ip.i_number.get(),
        ufid_gen: ip.dip_gen(),
    }
    .to_fid(fhp);

    Ok(())
}

/// `getblk(vp, blkno, size, 0, INFSLP)`, which cannot fail without `slpflag`.
fn getblk_wait(vp: &'static Vnode, blkno: i64, size: i32) -> &'static Buf {
    loop {
        if let Some(bp) = getblk(vp, blkno, size, 0, INFSLP) {
            return bp;
        }
    }
}

/// `ffs_sbupdate`: write a superblock and associated information back to disk.
pub fn ffs_sbupdate(mp: &Ufsmount, waitfor: i32) -> Result<(), Errno> {
    let fs = mp.fs();
    let devvp = mp.devvp();
    let mut allerror = Ok(());

    // First write back the summary information.
    let blks = howmany(fs.fs_cssize.get() as usize, fs.fs_fsize.get() as usize) as i32;
    let mut space = fs.fs_csp.get().cast::<u8>().cast_const();
    let mut i = 0;
    while i < blks {
        let mut size = fs.fs_bsize.get();
        if i + fs.fs_frag.get() > blks {
            size = (blks - i) * fs.fs_fsize.get();
        }
        let bp = getblk_wait(devvp, fsbtodb(fs, fs.fs_csaddr.get() + i64::from(i)), size);
        // SAFETY: `fs_csp` holds whole fragments of summaries (`ffs_mountfs`), and the busy
        // buffer `size` mapped bytes; they do not overlap.
        unsafe {
            ptr::copy_nonoverlapping(space, bp.b_data.get(), size as usize);
            space = space.add(size as usize);
        }
        if waitfor != MNT_WAIT {
            bawrite(bp);
        } else if let Err(e) = bwrite(bp) {
            allerror = Err(e);
        }
        i += fs.fs_frag.get();
    }

    // Now write back the superblock itself. If any errors occurred up to this point, then
    // fail so that the superblock avoids being written out as clean.
    allerror?;

    let bp = getblk_wait(
        devvp,
        fs.fs_sblockloc.get() >> (fs.fs_fshift.get() - fs.fs_fsbtodb.get()),
        fs.fs_sbsize.get(),
    );
    fs.fs_fmod.set(0);
    fs.fs_time.set(gettime());
    // SAFETY: the in-core super-block holds at least `fs_sbsize` bytes (`fs_allocsize`), the
    // busy buffer `fs_sbsize` mapped bytes; they do not overlap.
    unsafe {
        ptr::copy_nonoverlapping(
            ptr::from_ref(fs).cast::<u8>(),
            bp.b_data.get(),
            fs.fs_sbsize.get() as usize,
        );
    }
    // Restore compatibility to old file systems. XXX
    // SAFETY: the buffer is ours (busy from getblk) and mapped; its page-rounded mapping
    // holds a `struct fs`.
    let dfs = unsafe { fs_in_buf(bp) }; // XXX
    if fs.fs_postblformat.get() == FS_42POSTBLFMT {
        dfs.fs_nrpos.set(-1); // XXX
    }
    if fs.fs_inodefmt.get() < FS_44INODEFMT {
        // XXX
        // The five 32-bit words from fs_qbmask on rotate by one. XXX
        let base = core::mem::offset_of!(Fs, fs_qbmask);
        // SAFETY: the 20 bytes from `fs_qbmask` are inside the super-block in the busy,
        // mapped buffer; `dfs` is not used while the slice lives.
        let lp = unsafe { core::slice::from_raw_parts_mut(bp.b_data.get().add(base), 20) };
        let word = |lp: &[u8], k: usize| [lp[k * 4], lp[k * 4 + 1], lp[k * 4 + 2], lp[k * 4 + 3]];
        let tmp = word(lp, 4);
        for k in (1..=4).rev() {
            let w = word(lp, k - 1);
            lp[k * 4..k * 4 + 4].copy_from_slice(&w);
        }
        lp[..4].copy_from_slice(&tmp);
    } // XXX
    dfs.fs_maxfilesize.set(mp.um_savedmaxfilesize.get()); // XXX

    ffs1_compat_write(dfs, mp);

    if waitfor != MNT_WAIT {
        bawrite(bp);
        Ok(())
    } else {
        bwrite(bp)
    }
}

/// `ffs_init` (`vfs_init`): the inode and dinode pools, then the UFS layer.
pub fn ffs_init(vfsp: &'static Vfsconf) -> Result<(), Errno> {
    if FFS_INIT_DONE.swap(true, Ordering::Relaxed) {
        return Ok(());
    }

    pool_init(
        &FFS_INO_POOL,
        size_of::<Inode>(),
        0,
        IPL_NONE,
        PR_WAITOK,
        "ffsino",
        None,
    );
    pool_init(
        &FFS_DINODE1_POOL,
        size_of::<Ufs1Dinode>(),
        0,
        IPL_NONE,
        PR_WAITOK,
        "dino1pl",
        None,
    );
    #[cfg(feature = "ffs2")]
    pool_init(
        &FFS_DINODE2_POOL,
        size_of::<Ufs2Dinode>(),
        0,
        IPL_NONE,
        PR_WAITOK,
        "dino2pl",
        None,
    );

    ufs_init(vfsp)
}

/// `ffs_sysctl` (`vfs_sysctl`): fast filesystem related variables (`ffs_vars[]`, empty
/// without `UFS_DIRHASH`).
pub fn ffs_sysctl(
    name: &[i32],
    oldp: usize,
    oldlenp: &mut usize,
    newp: usize,
    newlen: usize,
    _p: &Proc,
) -> Result<(), Errno> {
    sysctl_bounded_arr(&[], name, oldp, oldlenp, newp, newlen)
}

#[cfg(test)]
mod tests;
