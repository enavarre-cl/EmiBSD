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

/* <CODE> */
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
//!   lifetime cannot hold a local name. `swapdev` and `nblkdev` come from the machine's
//!   `conf.c` (`crate::machine::conf`).
//! - `ffs_vars[]` holds only the `UFS_DIRHASH` variables (feature `ufs_dirhash`); without
//!   the feature it is empty and `ffs_sysctl` answers every name as `sysctl_bounded_arr`
//!   does for an unknown one.
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
use crate::kern::subr_disk::disk_map;
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
use crate::sys::disk::DM_OPENBLCK;
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

/// `ffs_vars[]`: the `UFS_DIRHASH` variables `vfs.ffs.dirhash_dirsize`, `dirhash_maxmem`
/// and `dirhash_mem` (read-only).
#[cfg(feature = "ufs_dirhash")]
static FFS_VARS: [crate::sys::sysctl::SysctlBoundedArgs; 3] = {
    use crate::sys::sysctl::SysctlBoundedArgs;
    use crate::ufs::ffs::ffs_extern::{FFS_DIRHASH_DIRSIZE, FFS_DIRHASH_MAXMEM, FFS_DIRHASH_MEM};
    use crate::ufs::ufs::ufs_dirhash::{UFS_DIRHASHMAXMEM, UFS_DIRHASHMEM, UFS_MINDIRHASHSIZE};
    [
        SysctlBoundedArgs::new(FFS_DIRHASH_DIRSIZE, &UFS_MINDIRHASHSIZE, 0, i32::MAX),
        SysctlBoundedArgs::new(FFS_DIRHASH_MAXMEM, &UFS_DIRHASHMAXMEM, 0, i32::MAX),
        SysctlBoundedArgs::readonly(FFS_DIRHASH_MEM, &UFS_DIRHASHMEM),
    ]
};
/// `ffs_vars[]`: empty without `UFS_DIRHASH`.
#[cfg(not(feature = "ufs_dirhash"))]
static FFS_VARS: [crate::sys::sysctl::SysctlBoundedArgs; 0] = [];

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
    let mut fname = [0u8; MNAMELEN];
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
                    if let Err(e) = vfs_export(mp, &u.um_export, &a.export_info) {
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

            if !disk_map(&fspec, &mut fname, DM_OPENBLCK) {
                fname = fspec;
            }

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

/// `ffs_sysctl` (`vfs_sysctl`): fast filesystem related variables (`ffs_vars[]`).
pub fn ffs_sysctl(
    name: &[i32],
    oldp: usize,
    oldlenp: &mut usize,
    newp: usize,
    newlen: usize,
    _p: &Proc,
) -> Result<(), Errno> {
    sysctl_bounded_arr(&FFS_VARS, name, oldp, oldlenp, newp, newlen)
}
/* </CODE> */

/* <TESTS> */
#[cfg(test)]
pub(crate) mod tests {
    // Host tests for the fast file system: `newfs`, a small `newfs(8)`/`mkfs.c` equivalent that
    // lays out an FFS1 or FFS2 image in memory (super-block, cylinder groups, root directory,
    // and files in the root directory), a block device vnode whose strategy reads and writes
    // that image, and tests that mount it with `ffs_mountfs`, read, write, create and remove
    // files and directories through the system calls, sync, unmount, check the image's
    // counters and mount it again.

    use core::ptr;
    use core::sync::atomic::Ordering;
    use std::sync::MutexGuard;
    use std::vec::Vec;
    use std::{assert_eq, vec};

    use super::*;
    use crate::kern::kern_descrip::sys_close;
    use crate::kern::subr_xxx::nullop;
    use crate::kern::sys_generic::{sys_read, sys_write};
    use crate::kern::vfs_bio::{BCSTATS, BUFHEAD, BUFKVM, CLEANCACHE, biodone, bufinit};
    use crate::kern::vfs_default::vop_generic_bwrite;
    use crate::kern::vfs_init::{set_rootvnode, vfs_byname};
    use crate::kern::vfs_subr::{vflushbuf, vfs_busy, vfs_mount_alloc};
    use crate::kern::vfs_syscalls::{
        dounmount, sys_fsync, sys_getdents, sys_mkdir, sys_open, sys_readlink, sys_rename,
        sys_rmdir, sys_symlink, sys_sync, sys_unlink,
    };
    use crate::kern::vfs_vops::VOP_UNLOCK;
    use crate::machine::Machine;
    use crate::machine::cpu::Cpu;
    use crate::sys::buf::{B_ERROR, B_READ};
    use crate::sys::fcntl::{O_CREAT, O_RDONLY, O_RDWR};
    use crate::sys::mount::{MNT_WAIT, VB_WAIT, VB_WRITE, VFS_ROOT};
    use crate::sys::systm::{SyCall, SysArgs};
    use crate::sys::types::{Register, makedev};
    use crate::sys::vnode::{VopFsyncArgs, VopInactiveArgs, VopStrategyArgs, Vops};

    /// `newfs`: an FFS image built in memory the way `newfs(8)` (`sbin/newfs/mkfs.c`) builds
    /// one, for the tests and as a record of the on-disk layout.
    pub(crate) mod newfs {
        use std::boxed::Box;
        use std::vec;
        use std::vec::Vec;

        use crate::sys::param::{DEV_BSIZE, MAXBSIZE, howmany};
        use crate::ufs::ffs::ffs_subr::{ffs_clrblock, ffs_isblock, ffs_setblock};
        use crate::ufs::ffs::fs::*;
        use crate::ufs::ufs::dinode::*;
        use crate::ufs::ufs::dir::{DIRBLKSIZ, DT_DIR, DT_REG, Direct, dirsiz};
        use crate::ufs::ufs::inode::{set_dinode1_at, set_dinode2_at};

        /// A block-aligned buffer as big as the largest block.
        #[repr(C, align(4096))]
        pub(crate) struct Blk(pub(crate) [u8; MAXBSIZE]);

        /// The geometry to build: the format, the image size, the block and fragment sizes, the
        /// fragments and inodes per cylinder group.
        #[derive(Clone, Copy)]
        pub(crate) struct Params {
            /// FFS2 (UFS2 dinodes, super-block at 64 KB) or FFS1 (super-block at 8 KB).
            pub(crate) ufs2: bool,
            /// The image's size in bytes (a multiple of `fpg * fsize` keeps every group full).
            pub(crate) size: usize,
            /// `fs_bsize`.
            pub(crate) bsize: i32,
            /// `fs_fsize`.
            pub(crate) fsize: i32,
            /// `fs_fpg`.
            pub(crate) fpg: i32,
            /// `fs_ipg` (a multiple of `INOPB`).
            pub(crate) ipg: u32,
        }

        /// FFS2, 4 MB: 8 KB blocks, 1 KB fragments, two cylinder groups of 512 inodes.
        pub(crate) const FFS2_4M: Params = Params {
            ufs2: true,
            size: 4 << 20,
            bsize: 8192,
            fsize: 1024,
            fpg: 2048,
            ipg: 512,
        };

        /// FFS1, 4 MB, as `makefs` builds the install ramdisks: 4 KB blocks, 512-byte fragments,
        /// two cylinder groups of 512 inodes.
        pub(crate) const FFS1_4M: Params = Params {
            ufs2: false,
            size: 4 << 20,
            bsize: 4096,
            fsize: 512,
            fpg: 4096,
            ipg: 512,
        };

        /// The fixed time stamp of everything `newfs` writes.
        pub(crate) const TIME: i64 = 1_700_000_000;

        /// An image under construction: the bytes, the super-block, each cylinder group block
        /// and the summaries, which `finish` writes into the bytes.
        pub(crate) struct Image {
            /// The disk.
            pub(crate) disk: Vec<u8>,
            /// The super-block.
            sb: Box<Blk>,
            /// Each cylinder group block.
            cgs: Vec<Box<Blk>>,
            /// The next generation number handed out.
            gen_seed: u32,
        }

        /// `ilog2`.
        fn ilog2(v: i32) -> i32 {
            31 - v.leading_zeros() as i32
        }

        impl Image {
            /// The super-block.
            pub(crate) fn fs(&self) -> &Fs {
                // SAFETY: `sb` is an aligned buffer larger than `Fs`, which is `Cell`s of
                // integers and raw pointers, valid for any bytes.
                unsafe { &*self.sb.0.as_ptr().cast::<Fs>() }
            }

            /// The header of cylinder group `cg`.
            fn cg(&self, cg: u32) -> &Cg {
                // SAFETY: as for `fs`.
                unsafe { &*self.cgs[cg as usize].0.as_ptr().cast::<Cg>() }
            }

            /// The bytes of cylinder group `cg` from `off` on (a map).
            fn cgmap(&mut self, cg: u32, off: u32) -> &mut [u8] {
                &mut self.cgs[cg as usize].0[off as usize..]
            }

            /// `fs_cs(fs, cg)`, kept in the image (the in-core array does not exist here).
            fn cs_add(&mut self, cg: u32, field: usize, d: i32) {
                let off = self.cs_off(cg) + field * 4;
                let v = i32::from_ne_bytes(self.disk[off..off + 4].try_into().unwrap()) + d;
                self.disk[off..off + 4].copy_from_slice(&v.to_ne_bytes());
            }

            /// The byte offset in the image of `fs_cs(fs, cg)`.
            fn cs_off(&self, cg: u32) -> usize {
                let fs = self.fs();
                fsbtodb(fs, fs.fs_csaddr.get()) as usize * DEV_BSIZE + cg as usize * 16
            }

            /// `mkfs` and `fsinit`: an empty file system with a root directory.
            pub(crate) fn new(p: Params) -> Self {
                let mut img = Image {
                    disk: vec![0u8; p.size],
                    sb: Box::new(Blk([0; MAXBSIZE])),
                    cgs: Vec::new(),
                    gen_seed: 0x1234_5678,
                };
                let fs = img.fs();
                let frag = p.bsize / p.fsize;
                fs.fs_postblformat.set(FS_DYNAMICPOSTBLFMT);
                fs.fs_avgfilesize.set(AVFILESIZ);
                fs.fs_avgfpdir.set(AFPDIR);
                fs.fs_bsize.set(p.bsize);
                fs.fs_fsize.set(p.fsize);
                fs.fs_bmask.set(!(p.bsize - 1));
                fs.fs_fmask.set(!(p.fsize - 1));
                fs.fs_qbmask.set(i64::from(!fs.fs_bmask.get()));
                fs.fs_qfmask.set(i64::from(!fs.fs_fmask.get()));
                fs.fs_bshift.set(ilog2(p.bsize));
                fs.fs_fshift.set(ilog2(p.fsize));
                fs.fs_frag.set(frag);
                fs.fs_fragshift.set(ilog2(frag));
                fs.fs_fsbtodb.set(ilog2(p.fsize / DEV_BSIZE as i32));
                fs.fs_size.set((p.size / p.fsize as usize) as i64);
                fs.fs_nspf.set(p.fsize / DEV_BSIZE as i32);
                fs.fs_maxcontig.set(1);
                fs.fs_nrpos.set(1);
                fs.fs_cpg.set(1);
                if p.ufs2 {
                    fs.fs_inodefmt.set(FS_44INODEFMT);
                    fs.fs_sblockloc.set(i64::from(SBLOCK_UFS2));
                    fs.fs_nindir.set(p.bsize / 8);
                    fs.fs_inopb.set((p.bsize / 256) as u32);
                    fs.fs_maxsymlinklen.set(MAXSYMLINKLEN_UFS2 as i32);
                } else {
                    fs.fs_sblockloc.set(i64::from(SBLOCK_UFS1));
                    fs.fs_nindir.set(p.bsize / 4);
                    fs.fs_inopb.set((p.bsize / 128) as u32);
                    fs.fs_maxsymlinklen.set(MAXSYMLINKLEN_UFS1 as i32);
                    fs.fs_inodefmt.set(FS_44INODEFMT);
                    fs.fs_cgoffset.set(0);
                    fs.fs_cgmask.set(-1);
                    fs.fs_ffs1_size.set(fs.fs_size.get() as i32);
                    fs.fs_rotdelay.set(0);
                    fs.fs_rps.set(60);
                    fs.fs_interleave.set(1);
                    fs.fs_trackskew.set(0);
                    fs.fs_cpc.set(0);
                }
                let div_ceil = |x: i64, y: i64| (x + y - 1) / y;
                let roundup = |x: i64, y: i64| div_ceil(x, y) * y;
                fs.fs_sblkno.set(roundup(
                    div_ceil(
                        fs.fs_sblockloc.get() + SBLOCKSIZE as i64,
                        i64::from(p.fsize),
                    ),
                    i64::from(frag),
                ) as i32);
                fs.fs_cblkno.set(
                    fs.fs_sblkno.get()
                        + roundup(div_ceil(SBSIZE as i64, i64::from(p.fsize)), i64::from(frag))
                            as i32,
                );
                fs.fs_iblkno.set(fs.fs_cblkno.get() + frag);
                let mut maxfilesize = p.bsize as u64 * NDADDR as u64 - 1;
                let mut sizepb = p.bsize as u64;
                for _ in 0..NIADDR {
                    sizepb *= fs.fs_nindir.get() as u64;
                    maxfilesize += sizepb;
                }
                fs.fs_maxfilesize.set(maxfilesize);
                fs.fs_fpg.set(p.fpg);
                fs.fs_ipg.set(p.ipg);
                let ncg = (fs.fs_size.get() as usize).div_ceil(p.fpg as usize) as u32;
                fs.fs_ncg.set(ncg);
                if !p.ufs2 {
                    fs.fs_spc.set(p.fpg * fs.fs_nspf.get());
                    fs.fs_nsect.set(fs.fs_spc.get());
                    fs.fs_npsect.set(fs.fs_spc.get());
                    fs.fs_ncyl.set(ncg as i32);
                }
                fs.fs_cgsize.set(fragroundup(fs, cgsize(fs) as i64) as i32);
                fs.fs_dblkno
                    .set(fs.fs_iblkno.get() + (p.ipg / inopf(fs)) as i32);
                fs.fs_csaddr.set(cgdmin(fs, 0));
                fs.fs_cssize
                    .set(fragroundup(fs, (ncg as usize * 16) as i64) as i32);
                fs.fs_sbsize
                    .set(fragroundup(fs, size_of::<Fs>() as i64).min(SBLOCKSIZE as i64) as i32);
                fs.fs_minfree.set(MINFREE);
                fs.fs_maxbpg
                    .set(if p.ufs2 { p.bsize / 8 } else { p.bsize / 4 });
                fs.fs_optim.set(FS_OPTTIME);
                fs.fs_clean.set(1);
                fs.fs_id[0].set(TIME as i32);
                fs.fs_id[1].set(0x0eb5_d00d);

                let csfrags = howmany(fs.fs_cssize.get() as usize, p.fsize as usize) as i64;
                let dsize = fs.fs_size.get()
                    - i64::from(fs.fs_sblkno.get())
                    - i64::from(ncg) * i64::from(fs.fs_dblkno.get() - fs.fs_sblkno.get());
                fs.fs_dsize.set(dsize);
                fs.fs_cstotal
                    .cs_nbfree
                    .set(fragstoblks(fs, dsize) - div_ceil(csfrags, i64::from(frag)));
                fs.fs_cstotal.cs_nffree.set(
                    fragnum(fs, fs.fs_size.get())
                        + if fragnum(fs, csfrags) > 0 {
                            i64::from(frag) - fragnum(fs, csfrags)
                        } else {
                            0
                        },
                );
                fs.fs_cstotal
                    .cs_nifree
                    .set(i64::from(ncg * p.ipg - ROOTINO));
                fs.fs_cstotal.cs_ndir.set(0);
                fs.fs_dsize.set(dsize - csfrags);
                fs.fs_time.set(TIME);
                fs.fs_magic
                    .set(if p.ufs2 { FS_UFS2_MAGIC } else { FS_UFS1_MAGIC });

                for cg in 0..ncg {
                    img.initcg(cg, p);
                }
                img.fsinit();
                img
            }

            /// `initcg`: initialize a cylinder group.
            fn initcg(&mut self, cg: u32, p: Params) {
                let fs = self.fs();
                let ufs2 = p.ufs2;
                let frag = fs.fs_frag.get();
                let ipg = fs.fs_ipg.get();
                let cbase = cgbase(fs, cg);
                let dmax = (cbase + i64::from(fs.fs_fpg.get())).min(fs.fs_size.get());
                let dlower = cgsblock(fs, cg) - cbase;
                let mut dupper = cgdmin(fs, cg) - cbase;
                if cg == 0 {
                    dupper +=
                        howmany(fs.fs_cssize.get() as usize, fs.fs_fsize.get() as usize) as i64;
                }
                let inopb = inopb(fs);
                let cpg = fs.fs_cpg.get();
                let ncg = fs.fs_ncg.get();
                let fpg = fs.fs_fpg.get();
                self.cgs.push(Box::new(Blk([0; MAXBSIZE])));
                let c = self.cg(cg);
                c.cg_ffs2_time.set(TIME);
                c.cg_magic.set(CG_MAGIC);
                c.cg_cgx.set(cg);
                c.cg_ffs2_niblk.set(ipg);
                c.cg_initediblk.set(ipg.min(2 * inopb));
                c.cg_ndblk.set((dmax - cbase) as u32);
                let start = size_of::<Cg>() as u32;
                if !ufs2 {
                    // Hack to maintain compatibility with old fsck.
                    c.cg_ncyl.set(if cg == ncg - 1 { 0 } else { cpg as i16 });
                    c.cg_time.set(TIME as i32);
                    c.cg_ffs2_time.set(0);
                    c.cg_niblk.set(c.cg_ffs2_niblk.get() as i16);
                    c.cg_ffs2_niblk.set(0);
                    c.cg_initediblk.set(0);
                    c.cg_btotoff.set(start as i32);
                    c.cg_boff.set(c.cg_btotoff.get() + cpg * 4);
                    c.cg_iusedoff.set((c.cg_boff.get() + cpg * 2) as u32);
                } else {
                    c.cg_iusedoff.set(start);
                }
                c.cg_freeoff
                    .set(c.cg_iusedoff.get() + howmany(ipg as usize, 8) as u32);
                c.cg_nextfreeoff
                    .set(c.cg_freeoff.get() + howmany(fpg as usize, 8) as u32);
                c.cg_cs.cs_nifree.set(c.cg_cs.cs_nifree.get() + ipg as i32);
                let iusedoff = c.cg_iusedoff.get();
                let freeoff = c.cg_freeoff.get();
                if cg == 0 {
                    for i in 0..ROOTINO as usize {
                        self.cgmap(cg, iusedoff)[i / 8] |= 1 << (i % 8);
                        let c = self.cg(cg);
                        c.cg_cs.cs_nifree.set(c.cg_cs.cs_nifree.get() - 1);
                    }
                }
                let blocks_free = |img: &mut Self, d: i64| {
                    let blkno = d / i64::from(frag);
                    // SAFETY-free: the map is the image's own bytes.
                    let fsp: *const Fs = img.fs();
                    // SAFETY: `fsp` points into `img.sb`, which `cgmap` does not borrow.
                    ffs_setblock(unsafe { &*fsp }, img.cgmap(cg, freeoff), blkno);
                    let c = img.cg(cg);
                    c.cg_cs.cs_nbfree.set(c.cg_cs.cs_nbfree.get() + 1);
                    if !ufs2 {
                        img.cg_blktot_add(cg, 0, 1);
                    }
                };
                if cg > 0 {
                    // In cg 0, space is reserved for boot and super blocks.
                    let mut d = 0;
                    while d < dlower {
                        blocks_free(self, d);
                        d += i64::from(frag);
                    }
                }
                let r = dupper % i64::from(frag);
                if r != 0 {
                    let c = self.cg(cg);
                    let k = (i64::from(frag) - r) as usize;
                    c.cg_frsum[k].set(c.cg_frsum[k].get() + 1);
                    let end = dupper + i64::from(frag) - r;
                    while dupper < end {
                        self.cgmap(cg, freeoff)[(dupper / 8) as usize] |= 1 << (dupper % 8);
                        let c = self.cg(cg);
                        c.cg_cs.cs_nffree.set(c.cg_cs.cs_nffree.get() + 1);
                        dupper += 1;
                    }
                }
                let ndblk = i64::from(self.cg(cg).cg_ndblk.get());
                let mut d = dupper;
                while d + i64::from(frag) <= ndblk {
                    blocks_free(self, d);
                    d += i64::from(frag);
                }
                if d < ndblk {
                    let c = self.cg(cg);
                    let k = (ndblk - d) as usize;
                    c.cg_frsum[k].set(c.cg_frsum[k].get() + 1);
                    while d < ndblk {
                        self.cgmap(cg, freeoff)[(d / 8) as usize] |= 1 << (d % 8);
                        let c = self.cg(cg);
                        c.cg_cs.cs_nffree.set(c.cg_cs.cs_nffree.get() + 1);
                        d += 1;
                    }
                }
                // *cs = acg.cg_cs
                let c = self.cg(cg);
                let cs = [
                    c.cg_cs.cs_ndir.get(),
                    c.cg_cs.cs_nbfree.get(),
                    c.cg_cs.cs_nifree.get(),
                    c.cg_cs.cs_nffree.get(),
                ];
                for (i, v) in cs.iter().enumerate() {
                    self.cs_add(cg, i, *v);
                }

                // Generation numbers for the inodes newfs initialises: the first two blocks of
                // each group for FFS2 (the rest lazily, ffs_nodealloccg), all of them for FFS1.
                let n = if ufs2 { ipg.min(2 * inopb) } else { ipg };
                for i in 0..n {
                    let ino = cg * ipg + i;
                    let g = self.next_gen();
                    self.with_dinode(ino, |d| match d {
                        Din::Ufs1(d) => d.di_gen = g,
                        Din::Ufs2(d) => d.di_gen = g as i32,
                    });
                }
            }

            /// `cg_blktot(cgp)[cylno] += d` (FFS1).
            fn cg_blktot_add(&mut self, cg: u32, cylno: usize, d: i32) {
                let off = self.cg(cg).cg_btotoff.get() as u32 + cylno as u32 * 4;
                let b = self.cgmap(cg, off);
                let v = i32::from_ne_bytes(b[..4].try_into().unwrap()) + d;
                b[..4].copy_from_slice(&v.to_ne_bytes());
                let boff = self.cg(cg).cg_boff.get() as u32;
                let b = self.cgmap(cg, boff);
                let v = i16::from_ne_bytes(b[..2].try_into().unwrap()) + d as i16;
                b[..2].copy_from_slice(&v.to_ne_bytes());
            }

            /// A fresh nonzero generation number.
            fn next_gen(&mut self) -> u32 {
                self.gen_seed = self
                    .gen_seed
                    .wrapping_mul(1_103_515_245)
                    .wrapping_add(12345)
                    | 1;
                self.gen_seed
            }

            /// `f` on the dinode of `ino` in the image.
            fn with_dinode(&mut self, ino: Ufsino, f: impl FnOnce(&mut Din)) {
                let fs = self.fs();
                let blk = fsbtodb(fs, ino_to_fsba(fs, ino)) as usize * DEV_BSIZE;
                let idx = ino_to_fsbo(fs, ino);
                let ufs2 = fs.fs_magic.get() == FS_UFS2_MAGIC;
                let bsize = fs.fs_bsize.get() as usize;
                let b = &mut self.disk[blk..blk + bsize];
                if ufs2 {
                    let mut d = Din::Ufs2(crate::ufs::ufs::inode::dinode2_at(b, idx));
                    f(&mut d);
                    if let Din::Ufs2(d) = d {
                        set_dinode2_at(b, idx, &d);
                    }
                } else {
                    let mut d = Din::Ufs1(crate::ufs::ufs::inode::dinode1_at(b, idx));
                    f(&mut d);
                    if let Din::Ufs1(d) = d {
                        set_dinode1_at(b, idx, &d);
                    }
                }
            }

            /// `alloc(size, mode)`: a block (or its first fragments) in cylinder group 0.
            fn alloc(&mut self, size: i32, mode: u32) -> i64 {
                let fs = self.fs();
                let frag = fs.fs_frag.get();
                let fsize = fs.fs_fsize.get();
                let bsize = fs.fs_bsize.get();
                let ufs2 = fs.fs_magic.get() == FS_UFS2_MAGIC;
                let freeoff = self.cg(0).cg_freeoff.get();
                let ndblk = i64::from(self.cg(0).cg_ndblk.get());
                let fsp: *const Fs = self.fs();
                // SAFETY: `fsp` points into `self.sb`, which `cgmap` does not borrow.
                let fs = unsafe { &*fsp };
                let mut d = 0;
                while d < ndblk {
                    if ffs_isblock(fs, self.cgmap(0, freeoff), d / i64::from(frag)) {
                        break;
                    }
                    d += i64::from(frag);
                }
                assert!(d < ndblk, "newfs: cg 0 is full");
                ffs_clrblock(fs, self.cgmap(0, freeoff), d / i64::from(frag));
                let c = self.cg(0);
                c.cg_cs.cs_nbfree.set(c.cg_cs.cs_nbfree.get() - 1);
                fs.fs_cstotal
                    .cs_nbfree
                    .set(fs.fs_cstotal.cs_nbfree.get() - 1);
                self.cs_add(0, 1, -1);
                if mode & IFMT == IFDIR {
                    let c = self.cg(0);
                    c.cg_cs.cs_ndir.set(c.cg_cs.cs_ndir.get() + 1);
                    fs.fs_cstotal.cs_ndir.set(fs.fs_cstotal.cs_ndir.get() + 1);
                    self.cs_add(0, 0, 1);
                }
                if !ufs2 {
                    self.cg_blktot_add(0, 0, -1);
                }
                if size != bsize {
                    let used = howmany(size as usize, fsize as usize) as i32;
                    let c = self.cg(0);
                    c.cg_cs.cs_nffree.set(c.cg_cs.cs_nffree.get() + frag - used);
                    fs.fs_cstotal
                        .cs_nffree
                        .set(fs.fs_cstotal.cs_nffree.get() + i64::from(frag - used));
                    self.cs_add(0, 3, frag - used);
                    let k = (frag - used) as usize;
                    let c = self.cg(0);
                    c.cg_frsum[k].set(c.cg_frsum[k].get() + 1);
                    for i in used..frag {
                        let bit = (d + i64::from(i)) as usize;
                        self.cgmap(0, freeoff)[bit / 8] |= 1 << (bit % 8);
                    }
                }
                d
            }

            /// `iput`: allocate inode `ino` in the map and write its dinode.
            fn iput(&mut self, ino: Ufsino, f: impl FnOnce(&mut Din)) {
                let iusedoff = self.cg(0).cg_iusedoff.get();
                let c = self.cg(0);
                c.cg_cs.cs_nifree.set(c.cg_cs.cs_nifree.get() - 1);
                self.cgmap(0, iusedoff)[ino as usize / 8] |= 1 << (ino % 8);
                let fs = self.fs();
                fs.fs_cstotal
                    .cs_nifree
                    .set(fs.fs_cstotal.cs_nifree.get() - 1);
                self.cs_add(0, 2, -1);
                let g = self.next_gen();
                self.with_dinode(ino, |d| {
                    match d {
                        Din::Ufs1(d) => d.di_gen = g,
                        Din::Ufs2(d) => d.di_gen = g as i32,
                    }
                    f(d);
                });
            }

            /// The byte offset in the image of fragment `frag`.
            fn frag_off(&self, frag: i64) -> usize {
                fsbtodb(self.fs(), frag) as usize * DEV_BSIZE
            }

            /// `fsinit`: the root directory (`.` and `..` in one `DIRBLKSIZ` block).
            fn fsinit(&mut self) {
                let fsize = self.fs().fs_fsize.get();
                let mode = IFDIR | 0o755;
                let db0 = self.alloc(fsize, mode);
                let off = self.frag_off(db0);
                let mut dot = Direct::new();
                dot.d_ino = ROOTINO;
                dot.d_type = DT_DIR;
                dot.d_namlen = 1;
                dot.d_name[0] = b'.';
                dot.d_reclen = dirsiz(1) as u16;
                dot.write_to(&mut self.disk, off);
                let mut dotdot = dot;
                dotdot.d_namlen = 2;
                dotdot.d_name[1] = b'.';
                dotdot.d_reclen = (DIRBLKSIZ - dirsiz(1)) as u16;
                dotdot.write_to(&mut self.disk, off + dirsiz(1));
                let blocks = (fragroundup(self.fs(), DIRBLKSIZ as i64) / DEV_BSIZE as i64) as i64;
                self.iput(ROOTINO, |d| match d {
                    Din::Ufs1(d) => {
                        d.di_mode = mode as u16;
                        d.di_nlink = 2;
                        d.di_size = DIRBLKSIZ as u64;
                        d.di_db[0] = db0 as i32;
                        d.di_blocks = blocks as i32;
                        (d.di_atime, d.di_mtime, d.di_ctime) =
                            (TIME as i32, TIME as i32, TIME as i32);
                    }
                    Din::Ufs2(d) => {
                        d.di_mode = mode as u16;
                        d.di_nlink = 2;
                        d.di_size = DIRBLKSIZ as u64;
                        d.di_db[0] = db0;
                        d.di_blocks = blocks as u64;
                        (d.di_atime, d.di_mtime, d.di_ctime) = (TIME, TIME, TIME);
                    }
                });
            }

            /// A regular file `name` holding `data` in the root directory (direct blocks only;
            /// the last block is fragments when it is the file's only partial one): its inode.
            pub(crate) fn add_file(&mut self, name: &[u8], data: &[u8]) -> Ufsino {
                let fs = self.fs();
                let bsize = fs.fs_bsize.get() as usize;
                let nblk = data.len().div_ceil(bsize);
                assert!(nblk <= NDADDR, "newfs: add_file is direct blocks only");
                // The next free inode of cylinder group 0.
                let iusedoff = self.cg(0).cg_iusedoff.get();
                let map = self.cgmap(0, iusedoff);
                let ino = (0..)
                    .find(|&i: &u32| map[i as usize / 8] & (1 << (i % 8)) == 0)
                    .unwrap();
                let mut db = [0i64; NDADDR];
                let mut used = 0i64;
                for (b, chunk) in data.chunks(bsize).enumerate() {
                    let size = if chunk.len() == bsize {
                        bsize as i32
                    } else {
                        fragroundup(self.fs(), chunk.len() as i64) as i32
                    };
                    db[b] = self.alloc(size, IFREG);
                    used += i64::from(size);
                    let off = self.frag_off(db[b]);
                    self.disk[off..off + chunk.len()].copy_from_slice(chunk);
                }
                let mode = IFREG | 0o644;
                let blocks = used / DEV_BSIZE as i64;
                self.iput(ino, |d| match d {
                    Din::Ufs1(d) => {
                        d.di_mode = mode as u16;
                        d.di_nlink = 1;
                        d.di_size = data.len() as u64;
                        for (i, b) in db.iter().enumerate() {
                            d.di_db[i] = *b as i32;
                        }
                        d.di_blocks = blocks as i32;
                        (d.di_atime, d.di_mtime, d.di_ctime) =
                            (TIME as i32, TIME as i32, TIME as i32);
                    }
                    Din::Ufs2(d) => {
                        d.di_mode = mode as u16;
                        d.di_nlink = 1;
                        d.di_size = data.len() as u64;
                        d.di_db = db;
                        d.di_blocks = blocks as u64;
                        (d.di_atime, d.di_mtime, d.di_ctime) = (TIME, TIME, TIME);
                    }
                });
                self.dir_add(name, ino, DT_REG);
                ino
            }

            /// Adds an entry to the root directory's block, splitting the last entry's space.
            fn dir_add(&mut self, name: &[u8], ino: Ufsino, dtype: u8) {
                let mut root_db0 = 0;
                self.with_dinode(ROOTINO, |d| {
                    root_db0 = match d {
                        Din::Ufs1(d) => i64::from(d.di_db[0]),
                        Din::Ufs2(d) => d.di_db[0],
                    }
                });
                let base = self.frag_off(root_db0);
                let blk = &mut self.disk[base..base + DIRBLKSIZ];
                let mut off = 0;
                loop {
                    let reclen = usize::from(crate::ufs::ufs::dir::d_reclen(blk, off));
                    if off + reclen >= DIRBLKSIZ {
                        break;
                    }
                    off += reclen;
                }
                let used = dirsiz(crate::ufs::ufs::dir::d_namlen(blk, off));
                let reclen = usize::from(crate::ufs::ufs::dir::d_reclen(blk, off));
                assert!(
                    reclen - used >= dirsiz(name.len() as u8),
                    "newfs: root directory full"
                );
                crate::ufs::ufs::dir::set_d_reclen(blk, off, used as u16);
                let mut e = Direct::new();
                e.d_ino = ino;
                e.d_type = dtype;
                e.d_namlen = name.len() as u8;
                e.d_name[..name.len()].copy_from_slice(name);
                e.d_reclen = (reclen - used) as u16;
                e.write_to(blk, off + used);
            }

            /// Writes the super-block (and its copy in each group), the cylinder groups and the
            /// summaries into the image, and returns it.
            pub(crate) fn finish(mut self) -> Vec<u8> {
                let fs = self.fs();
                if fs.fs_magic.get() == FS_UFS1_MAGIC {
                    fs.fs_ffs1_time.set(fs.fs_time.get() as i32);
                    fs.fs_ffs1_dsize.set(fs.fs_dsize.get() as i32);
                    fs.fs_ffs1_csaddr.set(fs.fs_csaddr.get() as i32);
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
                let loc = fs.fs_sblockloc.get() as usize;
                let cgsz = fs.fs_cgsize.get() as usize;
                let ncg = fs.fs_ncg.get();
                let mut offs = Vec::new();
                for cg in 0..ncg {
                    offs.push((
                        self.frag_off(cgsblock(self.fs(), cg)),
                        self.frag_off(cgtod(self.fs(), cg)),
                    ));
                }
                let sb = self.sb.0[..SBLOCKSIZE].to_vec();
                self.disk[loc..loc + SBLOCKSIZE].copy_from_slice(&sb);
                for (cg, (sboff, cgoff)) in offs.into_iter().enumerate() {
                    self.disk[sboff..sboff + SBLOCKSIZE].copy_from_slice(&sb);
                    let blk = self.cgs[cg].0[..cgsz].to_vec();
                    self.disk[cgoff..cgoff + cgsz].copy_from_slice(&blk);
                }
                self.disk
            }
        }

        /// A dinode of either format, as `with_dinode` hands it.
        pub(crate) enum Din {
            /// FFS1.
            Ufs1(Ufs1Dinode),
            /// FFS2.
            Ufs2(Ufs2Dinode),
        }

        /// `fsck`'s counter check: the summaries of each cylinder group against its maps, and
        /// their sum against the super-block's totals. Panics on a mismatch.
        pub(crate) fn check(disk: &[u8], ufs2: bool) {
            let loc = if ufs2 { SBLOCK_UFS2 } else { SBLOCK_UFS1 } as usize;
            let mut sb = Box::new(Blk([0; MAXBSIZE]));
            sb.0[..SBLOCKSIZE].copy_from_slice(&disk[loc..loc + SBLOCKSIZE]);
            // SAFETY: an aligned buffer larger than `Fs`, valid for any bytes.
            let fs = unsafe { &*sb.0.as_ptr().cast::<Fs>() };
            let frag = fs.fs_frag.get();
            let cs_off = fsbtodb(fs, fs.fs_csaddr.get()) as usize * DEV_BSIZE;
            let (mut tb, mut tf, mut ti, mut td) = (0i64, 0i64, 0i64, 0i64);
            for cg in 0..fs.fs_ncg.get() {
                let off = fsbtodb(fs, cgtod(fs, cg)) as usize * DEV_BSIZE;
                let mut blk = Box::new(Blk([0; MAXBSIZE]));
                let sz = fs.fs_cgsize.get() as usize;
                blk.0[..sz].copy_from_slice(&disk[off..off + sz]);
                // SAFETY: as above.
                let c = unsafe { &*blk.0.as_ptr().cast::<Cg>() };
                assert_eq!(c.cg_magic.get(), CG_MAGIC, "cg {cg} magic");
                let freemap = &blk.0[c.cg_freeoff.get() as usize..];
                let inomap = &blk.0[c.cg_iusedoff.get() as usize..];
                let (mut nb, mut nf) = (0, 0);
                let ndblk = c.cg_ndblk.get() as i64;
                let mut d = 0;
                while d < ndblk {
                    if ffs_isblock(fs, freemap, d / i64::from(frag)) {
                        nb += 1;
                    } else {
                        for i in 0..i64::from(frag) {
                            if d + i < ndblk
                                && freemap[((d + i) / 8) as usize] & (1 << ((d + i) % 8)) != 0
                            {
                                nf += 1;
                            }
                        }
                    }
                    d += i64::from(frag);
                }
                let ni = (0..fs.fs_ipg.get())
                    .filter(|&i| inomap[i as usize / 8] & (1 << (i % 8)) == 0)
                    .count() as i32;
                assert_eq!(c.cg_cs.cs_nbfree.get(), nb, "cg {cg} nbfree");
                assert_eq!(c.cg_cs.cs_nffree.get(), nf, "cg {cg} nffree");
                assert_eq!(c.cg_cs.cs_nifree.get(), ni, "cg {cg} nifree");
                let s = |k: usize| {
                    let o = cs_off + cg as usize * 16 + k * 4;
                    i32::from_ne_bytes(disk[o..o + 4].try_into().unwrap())
                };
                assert_eq!(
                    [s(0), s(1), s(2), s(3)],
                    [c.cg_cs.cs_ndir.get(), nb, ni, nf],
                    "cg {cg} summary"
                );
                tb += i64::from(nb);
                tf += i64::from(nf);
                ti += i64::from(ni);
                td += i64::from(c.cg_cs.cs_ndir.get());
            }
            assert_eq!(fs.fs_cstotal.cs_nbfree.get(), tb, "total nbfree");
            assert_eq!(fs.fs_cstotal.cs_nffree.get(), tf, "total nffree");
            assert_eq!(fs.fs_cstotal.cs_nifree.get(), ti, "total nifree");
            assert_eq!(fs.fs_cstotal.cs_ndir.get(), td, "total ndir");
        }

        /// The super-block's `fs_clean` in an image.
        pub(crate) fn clean(disk: &[u8], ufs2: bool) -> i8 {
            let loc = if ufs2 { SBLOCK_UFS2 } else { SBLOCK_UFS1 } as usize;
            disk[loc + core::mem::offset_of!(Fs, fs_clean)] as i8
        }
    }

    /// The disk the strategy below reads and writes.
    pub(crate) static DISK: std::sync::Mutex<Vec<u8>> = std::sync::Mutex::new(Vec::new());

    /// `diskvn`'s strategy: a synchronous transfer between the buffer and the image at
    /// `b_blkno`, then `biodone`.
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

    /// The fake disk's fsync: `vflushbuf`, as `spec_fsync` does.
    fn disk_fsync(ap: &mut VopFsyncArgs<'_>) -> Result<(), Errno> {
        vflushbuf(ap.a_vp, ap.a_waitfor == MNT_WAIT);
        Ok(())
    }

    fn disk_inactive(ap: &mut VopInactiveArgs<'_>) -> Result<(), Errno> {
        VOP_UNLOCK(ap.a_vp)
    }

    /// The operations of the fake disk's block device vnode: the device switch's open, close
    /// and strategy over `DISK`.
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

    /// The device number of the fake disk (`rd0a`-like).
    const DISKDEV: i32 = makedev(17, 0);

    /// Memory, the vfs and a fresh buffer cache, the image as the disk, and the thread as
    /// `curproc`.
    pub(crate) fn setup(image: Vec<u8>) -> (MutexGuard<'static, ()>, &'static Proc) {
        let (g, p) = crate::kern::vfs_subr::tests::setup();
        Machine::set_curproc(Machine::curcpu(), p);
        // No resource limits (write(2) checks RLIMIT_FSIZE).
        let limit: &'static crate::sys::resourcevar::Plimit =
            std::boxed::Box::leak(std::boxed::Box::new(crate::sys::resourcevar::Plimit::new()));
        for l in &limit.pl_rlimit {
            l.set(crate::sys::resource::Rlimit {
                rlim_cur: crate::sys::resource::RLIM_INFINITY,
                rlim_max: crate::sys::resource::RLIM_INFINITY,
            });
        }
        limit.pl_rlimit[crate::sys::resource::RLIMIT_NOFILE].set(crate::sys::resource::Rlimit {
            rlim_cur: 128,
            rlim_max: 128,
        });
        p.process().ps_limit.set(limit);
        p.p_limit.set(limit);

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

    /// The fake disk's block device vnode, referenced.
    fn diskvp() -> &'static Vnode {
        let vp = bdevvp(DISKDEV).unwrap().unwrap();
        vp.v_op.set(Some(&DISK_VOPS));
        vp
    }

    /// Mounts the disk at `/` the way `ffs_mount` and `main` do: `ffs_mountfs`, the mount list,
    /// the root vnode and the thread's current directory.
    pub(crate) fn mount_root(p: &'static Proc, ronly: bool) -> &'static Mount {
        let devvp = diskvp();
        let mp = vfs_mount_alloc(None, vfs_byname(b"ffs").unwrap());
        if ronly {
            mp.mnt_flag.set(mp.mnt_flag.get() | MNT_RDONLY);
        }
        mp.update_stat(|sp| sp.f_mntonname[0] = b'/');
        ffs_mountfs(devvp, mp, p).unwrap();
        let mut st = mp.mnt_stat.get();
        ffs_statfs(mp, &mut st, p).unwrap();
        mp.mnt_stat.set(st);
        vfs_unbusy(mp);
        // SAFETY: a new mount on no list.
        unsafe { MOUNTLIST.0.insert_tail(mp) };
        let root = VFS_ROOT(mp).unwrap();
        set_rootvnode(Some(root));
        p.fd().fd_cdir.set(Some(root));
        vref(root);
        let _ = VOP_UNLOCK(root);
        mp
    }

    /// Undoes `mount_root` and unmounts.
    pub(crate) fn unmount_root(p: &'static Proc, mp: &'static Mount) {
        if let Some(cdir) = p.fd().fd_cdir.take() {
            vrele(cdir);
        }
        if let Some(root) = crate::kern::vfs_init::rootvnode() {
            set_rootvnode(None);
            vrele(root);
        }
        vfs_busy(mp, VB_WRITE | VB_WAIT).unwrap();
        dounmount(mp, 0, p).unwrap();
    }

    /// A system call with up to six arguments; `retval[0]`.
    pub(crate) fn sys(f: SyCall, p: &Proc, args: &[usize]) -> Result<isize, Errno> {
        let mut v: SysArgs = [0; 6];
        for (slot, a) in v.iter_mut().zip(args) {
            *slot = *a as Register;
        }
        let mut rv = [0; 2];
        f(p, &v, &mut rv)?;
        Ok(rv[0])
    }

    /// A NUL-terminated path as a "user" address (the host's copyin reads it directly).
    pub(crate) fn path(s: &'static [u8]) -> usize {
        assert_eq!(s.last(), Some(&0));
        s.as_ptr() as usize
    }

    /// The whole contents of the file at `name`.
    pub(crate) fn read_file(p: &Proc, name: &'static [u8]) -> Result<Vec<u8>, Errno> {
        let fd = sys(sys_open, p, &[path(name), O_RDONLY as usize, 0])?;
        let mut out = Vec::new();
        let mut buf = vec![0u8; 3000];
        loop {
            let n = sys(
                sys_read,
                p,
                &[fd as usize, buf.as_mut_ptr() as usize, buf.len()],
            )?;
            if n == 0 {
                break;
            }
            out.extend_from_slice(&buf[..n as usize]);
        }
        sys(sys_close, p, &[fd as usize])?;
        Ok(out)
    }

    /// Creates (or truncates nothing: the file must be new) `name` with `data`.
    fn write_file(p: &Proc, name: &'static [u8], data: &[u8]) -> Result<(), Errno> {
        let fd = sys(
            sys_open,
            p,
            &[path(name), (O_RDWR | O_CREAT) as usize, 0o644],
        )?;
        // Write in uneven pieces, to cross block and fragment boundaries.
        let mut off = 0;
        for chunk in data.chunks(7001) {
            let n = sys(
                sys_write,
                p,
                &[fd as usize, chunk.as_ptr() as usize, chunk.len()],
            )?;
            assert_eq!(n as usize, chunk.len());
            off += chunk.len();
        }
        assert_eq!(off, data.len());
        sys(sys_fsync, p, &[fd as usize])?;
        sys(sys_close, p, &[fd as usize])?;
        Ok(())
    }

    /// The names in the directory `name` (without `.` and `..`), sorted.
    fn list_dir(p: &Proc, name: &'static [u8]) -> Vec<Vec<u8>> {
        let fd = sys(sys_open, p, &[path(name), O_RDONLY as usize, 0]).unwrap();
        let mut buf = vec![0u8; 4096];
        let mut names = Vec::new();
        loop {
            let n = sys(
                sys_getdents,
                p,
                &[fd as usize, buf.as_mut_ptr() as usize, buf.len()],
            )
            .unwrap();
            if n == 0 {
                break;
            }
            let mut off = 0;
            while off < n as usize {
                let reclen = u16::from_ne_bytes([buf[off + 16], buf[off + 17]]) as usize;
                let namlen = buf[off + 19] as usize;
                let nm = &buf[off + 24..off + 24 + namlen];
                if nm != b"." && nm != b".." {
                    names.push(nm.to_vec());
                }
                off += reclen;
            }
        }
        sys(sys_close, p, &[fd as usize]).unwrap();
        names.sort();
        names
    }

    /// A pattern of `n` bytes that differs block to block.
    fn pattern(n: usize, seed: u8) -> Vec<u8> {
        (0..n)
            .map(|i| (i as u8).wrapping_mul(31).wrapping_add(seed) ^ (i >> 9) as u8)
            .collect()
    }

    #[test]
    fn newfs_images_are_consistent() {
        for params in [newfs::FFS2_4M, newfs::FFS1_4M] {
            let mut img = newfs::Image::new(params);
            img.add_file(b"motd", b"hello, world\n");
            img.add_file(b"big", &pattern(30000, 1));
            let disk = img.finish();
            newfs::check(&disk, params.ufs2);
            assert_eq!(newfs::clean(&disk, params.ufs2), 1);
        }
    }

    #[test]
    fn ffs2_mount_read_write_and_remount() {
        let mut img = newfs::Image::new(newfs::FFS2_4M);
        img.add_file(b"motd", b"hello, world\n");
        let big = pattern(30000, 1);
        img.add_file(b"big", &big);
        let (_g, p) = setup(img.finish());

        let mp = mount_root(p, false);
        let fs = vfstoufs(mp).fs();
        assert_eq!(fs.fs_magic.get(), FS_UFS2_MAGIC);
        assert_eq!(
            fs.fs_clean.get(),
            0,
            "a read-write mount marks the file system dirty"
        );
        assert_eq!(newfs::clean(&DISK.lock().unwrap(), true), 0);

        // Reading what newfs wrote.
        assert_eq!(read_file(p, b"/motd\0").unwrap(), b"hello, world\n");
        assert_eq!(read_file(p, b"/big\0").unwrap(), big);
        assert_eq!(read_file(p, b"/nothere\0"), Err(Errno::ENOENT));

        // A file past the direct blocks (an indirect block), a directory with a file in it,
        // renames, removals and a symbolic link.
        let large = pattern(12 * 8192 + 20000, 7);
        write_file(p, b"/large\0", &large).unwrap();
        sys(sys_mkdir, p, &[path(b"/dir\0"), 0o755]).unwrap();
        write_file(p, b"/dir/f\0", b"in a directory\n").unwrap();
        sys(sys_rename, p, &[path(b"/dir/f\0"), path(b"/dir/g\0")]).unwrap();
        sys(sys_mkdir, p, &[path(b"/dir/sub\0"), 0o755]).unwrap();
        assert_eq!(sys(sys_rmdir, p, &[path(b"/dir\0")]), Err(Errno::ENOTEMPTY));
        sys(sys_rmdir, p, &[path(b"/dir/sub\0")]).unwrap();
        sys(sys_unlink, p, &[path(b"/motd\0")]).unwrap();
        sys(sys_symlink, p, &[path(b"/dir/g\0"), path(b"/lnk\0")]).unwrap();
        let mut lbuf = [0u8; 64];
        let n = sys(
            sys_readlink,
            p,
            &[path(b"/lnk\0"), lbuf.as_mut_ptr() as usize, lbuf.len()],
        )
        .unwrap();
        assert_eq!(&lbuf[..n as usize], b"/dir/g");
        assert_eq!(read_file(p, b"/lnk\0").unwrap(), b"in a directory\n");
        assert_eq!(read_file(p, b"/large\0").unwrap(), large);
        assert_eq!(
            list_dir(p, b"/\0"),
            vec![
                b"big".to_vec(),
                b"dir".to_vec(),
                b"large".to_vec(),
                b"lnk".to_vec()
            ]
        );
        assert_eq!(list_dir(p, b"/dir\0"), vec![b"g".to_vec()]);

        // sync(2), unmount: the image is clean and its counters agree with its maps.
        sys(sys_sync, p, &[]).unwrap();
        unmount_root(p, mp);
        {
            let disk = DISK.lock().unwrap();
            assert_eq!(newfs::clean(&disk, true), 1);
            newfs::check(&disk, true);
        }

        // Mount it again: everything is where it was left.
        let mp = mount_root(p, true);
        assert_eq!(read_file(p, b"/large\0").unwrap(), large);
        assert_eq!(read_file(p, b"/big\0").unwrap(), big);
        assert_eq!(read_file(p, b"/dir/g\0").unwrap(), b"in a directory\n");
        assert_eq!(read_file(p, b"/motd\0"), Err(Errno::ENOENT));
        assert_eq!(read_file(p, b"/lnk\0").unwrap(), b"in a directory\n");
        // A read-only mount refuses to create.
        assert_eq!(write_file(p, b"/new\0", b"x"), Err(Errno::EROFS));

        // Remove everything on a read-write mount: the space comes back.
        unmount_root(p, mp);
        let mp = mount_root(p, false);
        let fs = vfstoufs(mp).fs();
        sys(sys_unlink, p, &[path(b"/large\0")]).unwrap();
        sys(sys_unlink, p, &[path(b"/lnk\0")]).unwrap();
        sys(sys_unlink, p, &[path(b"/dir/g\0")]).unwrap();
        sys(sys_rmdir, p, &[path(b"/dir\0")]).unwrap();
        sys(sys_unlink, p, &[path(b"/big\0")]).unwrap();
        assert_eq!(list_dir(p, b"/\0"), Vec::<Vec<u8>>::new());
        sys(sys_sync, p, &[]).unwrap();
        let ndir = fs.fs_cstotal.cs_ndir.get();
        unmount_root(p, mp);
        let disk = DISK.lock().unwrap();
        newfs::check(&disk, true);
        assert_eq!(ndir, 1, "only the root directory is left");
        drop(disk);
        teardown();
    }

    #[test]
    fn ffs1_mount_read_write_and_remount() {
        let mut img = newfs::Image::new(newfs::FFS1_4M);
        img.add_file(b"motd", b"FFS1, as makefs builds the ramdisks\n");
        let (_g, p) = setup(img.finish());

        let mp = mount_root(p, false);
        let ump = vfstoufs(mp);
        assert_eq!(ump.um_fstype.get(), crate::ufs::ufs::ufsmount::UM_UFS1);
        assert_eq!(
            ump.fs().fs_sblockloc.get(),
            i64::from(crate::ufs::ffs::fs::SBLOCK_UFS1)
        );
        assert_eq!(
            read_file(p, b"/motd\0").unwrap(),
            b"FFS1, as makefs builds the ramdisks\n"
        );
        // Small file (fragments), then one past the direct blocks (12 * 4 KB).
        write_file(p, b"/small\0", b"tiny").unwrap();
        let large = pattern(12 * 4096 + 5000, 3);
        write_file(p, b"/large\0", &large).unwrap();
        sys(sys_mkdir, p, &[path(b"/etc\0"), 0o755]).unwrap();
        write_file(p, b"/etc/rc\0", b"#!/bin/sh\n").unwrap();
        sys(sys_sync, p, &[]).unwrap();
        unmount_root(p, mp);
        {
            let disk = DISK.lock().unwrap();
            newfs::check(&disk, false);
            assert_eq!(newfs::clean(&disk, false), 1);
        }

        let mp = mount_root(p, true);
        assert_eq!(read_file(p, b"/small\0").unwrap(), b"tiny");
        assert_eq!(read_file(p, b"/large\0").unwrap(), large);
        assert_eq!(read_file(p, b"/etc/rc\0").unwrap(), b"#!/bin/sh\n");
        unmount_root(p, mp);
        teardown();
    }

    #[test]
    fn ufs_getlbns_finds_the_indirect_path() {
        let (_g, p) = setup(newfs::Image::new(newfs::FFS2_4M).finish());
        let mp = mount_root(p, true);
        let root = crate::kern::vfs_init::rootvnode().unwrap();
        let nindir = vfstoufs(mp).um_nindir.get() as i64; // 1024
        let mut a = [crate::ufs::ufs::inode::Indir::default(); NIADDR + 2];
        let mut num = -1;
        crate::ufs::ufs::ufs_bmap::ufs_getlbns(root, 5, &mut a, Some(&mut num)).unwrap();
        assert_eq!(num, 0, "a direct block");
        crate::ufs::ufs::ufs_bmap::ufs_getlbns(root, NDADDR as i64, &mut a, Some(&mut num))
            .unwrap();
        assert_eq!(num, 2);
        assert_eq!((a[0].in_lbn, a[0].in_off), (-(NDADDR as i64), 0));
        assert_eq!((a[1].in_lbn, a[1].in_off), (-(NDADDR as i64), 0));
        let lbn = NDADDR as i64 + nindir + 3;
        crate::ufs::ufs::ufs_bmap::ufs_getlbns(root, lbn, &mut a, Some(&mut num)).unwrap();
        assert_eq!(num, 3, "a double indirect path");
        assert_eq!(a[0].in_off, 1);
        assert_eq!(a[2].in_off, 3);
        unmount_root(p, mp);
        teardown();
    }

    /// `mount -u` with `ufs_args` whose `fspec` is NULL and whose `export_info` names a network, as
    /// `mountd(8)` does it: `mount(2)` reaches `ffs_mount`, which hands the export list to
    /// `vfs_export`; `ufs_check_export` then answers the flags and the anonymous credentials for a
    /// client's address, and refuses one outside the list.
    #[cfg(feature = "nfsserver")]
    #[test]
    fn mount_update_exports_the_file_system() {
        use crate::kern::uipc_mbuf::tests::mbinit_again;
        use crate::kern::vfs_subr::tests::exports::{args, check_export, sin};
        use crate::kern::vfs_syscalls::sys_mount;
        use crate::sys::mount::{MNT_DELEXPORT, MNT_EXPORTED, MNT_EXRDONLY, MNT_UPDATE, UfsArgs};

        let img = newfs::Image::new(newfs::FFS2_4M);
        let (_g, p) = setup(img.finish());
        mbinit_again();
        let mp = mount_root(p, false);

        let net = sin(2, [10, 0, 2, 0]);
        let mask = sin(2, [255, 255, 255, 0]);
        let mut args = UfsArgs {
            fspec: 0,
            export_info: args(MNT_EXPORTED | MNT_EXRDONLY, 32767, Some(net), Some(mask)),
        };
        let check = |a: [u8; 4]| check_export(mp, a);

        // Not exported yet: every client is refused.
        assert_eq!(check([10, 0, 2, 9]), Err(Errno::EACCES));

        let update = |args: &mut UfsArgs| {
            sys(
                sys_mount,
                p,
                &[
                    b"ffs\0".as_ptr() as usize,
                    path(b"/\0"),
                    MNT_UPDATE as usize,
                    ptr::from_mut(args) as usize,
                ],
            )
        };
        update(&mut args).unwrap();
        assert!(mp.mnt_flag.get() & MNT_EXPORTED != 0);
        assert_eq!(mp.mnt_flag.get() & MNT_UPDATE, 0, "the update flag is gone");
        assert_eq!(
            check([10, 0, 2, 9]),
            Ok((MNT_EXPORTED | MNT_EXRDONLY, 32767))
        );
        assert_eq!(check([10, 0, 3, 9]), Err(Errno::EACCES));

        // The same network again is refused, and the mount keeps its flags.
        assert_eq!(update(&mut args), Err(Errno::EPERM));
        assert!(mp.mnt_flag.get() & MNT_EXPORTED != 0);

        // mountd deletes the list before it loads another.
        args.export_info.ex_flags = MNT_DELEXPORT;
        update(&mut args).unwrap();
        assert_eq!(mp.mnt_flag.get() & MNT_EXPORTED, 0);
        assert_eq!(check([10, 0, 2, 9]), Err(Errno::EACCES));

        unmount_root(p, mp);
        teardown();
    }

    #[test]
    #[ignore = "needs OPENBSD_SRC (just test-ref)"]
    fn fs_h_constants_match_the_c_header() {
        use crate::ufs::ffs::fs::*;
        let defs = crate::reftest::defines("sys/ufs/ffs/fs.h");
        for (name, value) in [
            ("BBSIZE", BBSIZE as i64),
            ("SBSIZE", SBSIZE as i64),
            ("SBLOCK_UFS1", i64::from(SBLOCK_UFS1)),
            ("SBLOCK_UFS2", i64::from(SBLOCK_UFS2)),
            ("SBLOCK_PIGGY", i64::from(SBLOCK_PIGGY)),
            ("SBLOCKSIZE", SBLOCKSIZE as i64),
            ("MAXFRAG", MAXFRAG as i64),
            ("MINBSIZE", MINBSIZE as i64),
            ("MAXMNTLEN", MAXMNTLEN as i64),
            ("MAXVOLLEN", MAXVOLLEN as i64),
            ("FS_MAXCONTIG", FS_MAXCONTIG as i64),
            ("MINFREE", i64::from(MINFREE)),
            ("AVFILESIZ", i64::from(AVFILESIZ)),
            ("AFPDIR", i64::from(AFPDIR)),
            ("FSMAXSNAP", FSMAXSNAP as i64),
            ("FS_MAGIC", i64::from(FS_MAGIC)),
            ("FS_UFS1_MAGIC", i64::from(FS_UFS1_MAGIC)),
            ("FS_UFS2_MAGIC", i64::from(FS_UFS2_MAGIC)),
            ("FS_OKAY", i64::from(FS_OKAY)),
            ("FS_44INODEFMT", i64::from(FS_44INODEFMT)),
            ("FS_ISCLEAN", i64::from(FS_ISCLEAN)),
            ("FS_WASCLEAN", i64::from(FS_WASCLEAN)),
            ("FS_OPTTIME", i64::from(FS_OPTTIME)),
            ("FS_OPTSPACE", i64::from(FS_OPTSPACE)),
            ("FS_UNCLEAN", i64::from(FS_UNCLEAN)),
            ("FS_FLAGS_UPDATED", i64::from(FS_FLAGS_UPDATED)),
            ("FS_DYNAMICPOSTBLFMT", i64::from(FS_DYNAMICPOSTBLFMT)),
            ("CG_MAGIC", i64::from(CG_MAGIC)),
        ] {
            assert_eq!(crate::reftest::int(&defs, name), Some(value), "{name}");
        }
        // The negative ones are written `-1`.
        assert_eq!(defs.get("FS_42INODEFMT").map(|s| s.as_str()), Some("-1"));
        assert_eq!(defs.get("FS_42POSTBLFMT").map(|s| s.as_str()), Some("-1"));
        let ufsmount = crate::reftest::defines("sys/ufs/ufs/ufsmount.h");
        assert_eq!(crate::reftest::int(&ufsmount, "UM_UFS1"), Some(1));
        assert_eq!(crate::reftest::int(&ufsmount, "UM_UFS2"), Some(2));
    }
}
/* </TESTS> */
