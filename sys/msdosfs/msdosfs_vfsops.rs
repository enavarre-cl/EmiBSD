/*	$OpenBSD: msdosfs_vfsops.c,v 1.99 2025/09/20 13:53:36 mpi Exp $	*/
/*	$NetBSD: msdosfs_vfsops.c,v 1.48 1997/10/18 02:54:57 briggs Exp $	*/
/* <LICENSES> */
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
//! The msdos file system's file-system-type operations (`msdosfs_vfsops`): mounting
//! (`msdosfs_mount`, and `msdosfs_mountfs`, which reads the boot sector's BIOS parameter
//! block, tells FAT12, FAT16 and FAT32 apart, checks the FAT32 FSInfo block and builds the
//! in-use cluster bitmap), unmounting, the root vnode, `statfs`, `sync`, file handles and
//! the export check.
//!
//! Upstream: sys/msdosfs/msdosfs_vfsops.c @ 3ce1f3f79392
//!
//! ## Deviations
//! - `msdosfs_mount` looks the device name up in a `Nameidata` of its own (`ndinit` builds
//!   one around the kernel copy of the name) instead of reinitialising the caller's `ndp`,
//!   whose lifetime cannot hold a local name, as `ffs_mount` does. `nblkdev` comes from the
//!   machine's `conf.c` (`crate::machine::conf`). A mount that is not an update and has no
//!   arguments (the C would dereference NULL) answers `EINVAL`.
//! - `bcopy(args, &mp->mnt_stat.mount_info.msdosfs_args, ...)` copies the kernel copy of the
//!   arguments' bytes into `mount_info` (`sys/sys/mount.rs`).
//! - `struct msdosfs_sync_arg` is [`MsdosfsSyncArgs`] and its `allerror` a `Result`;
//!   `vfs_mount_foreach_vnode`'s callback is a closure over it. `msdosfs_sync_vnode` checks
//!   `VNON` before it looks at the denode (a vnode without a type has none).
//! - `ffs(x) - 1` of a power of two is `trailing_zeros` (libkern's `ffs` is core's).
//! - The `MSDOSFS_DEBUG` `printf`s and `vprint` are left out: the option is not in GENERIC
//!   and has no feature here.
//! - `vfs_quotactl`, `vfs_vget` and `vfs_sysctl` are `eopnotsupp` in the C: the first two
//!   are closures answering `EOPNOTSUPP`, `vfs_sysctl` is `None` (no `vfs.msdos` node, which
//!   `vfs_sysctl` answers with `EOPNOTSUPP`).
//! - `msdosfs_init` is `msdosfs_denode.rs`'s, where the C defines it.

use core::ptr::{self, NonNull};

use crate::kern::init_main::rootvp;
use crate::kern::kern_malloc::{free, malloc, mallocarray};
use crate::kern::subr_disk::disk_map;
use crate::kern::subr_prf::{panic, printf};
use crate::kern::vfs_bio::{bread, brelse};
use crate::kern::vfs_lookup::{namei, ndinit};
use crate::kern::vfs_subr::{
    copy_statfs_info, vcount, vflush, vfs_export, vfs_export_lookup, vfs_mount_foreach_vnode,
    vfs_mountedon, vget, vinvalbuf, vput, vrele,
};
use crate::kern::vfs_vnops::vn_lock;
use crate::kern::vfs_vops::{VOP_CLOSE, VOP_FSYNC, VOP_IOCTL, VOP_OPEN, VOP_UNLOCK};
use crate::machine::conf::nblkdev;
use crate::machine::copy::copyinstr;
use crate::machine::intr::{splbio, splx};
use crate::msdosfs::bootsect::Bootsector;
use crate::msdosfs::bpb::{
    ByteBpb33, ByteBpb50, ByteBpb710, FATMIRROR, FATNUM, Fsinfo, getulong, getushort,
};
use crate::msdosfs::denode::{
    DE_ACCESS, DE_CREATE, DE_MODIFIED, DE_UPDATE, Defid, MSDOSFSROOT_OFS, WIN_MAXLEN, vtode,
};
use crate::msdosfs::direntry::Direntry;
use crate::msdosfs::fat::{
    CLUST_FIRST, CLUST_RSRVD, FAT12_MASK, FAT16_MASK, FAT32_MASK, MSDOSFSROOT, fat12, fat32,
};
use crate::msdosfs::msdosfs_denode::{deget, msdosfs_init};
use crate::msdosfs::msdosfs_fat::fillinusemap;
use crate::msdosfs::msdosfsmount::{
    MSDOSFS_FATMIRROR, MSDOSFSMNT_MNTOPT, MSDOSFSMNT_RONLY, MSDOSFSMNT_WAITONFAT, Msdosfsmount,
    N_INUSEBITS, fsi_size, vfstomsdosfs,
};
use crate::sys::buf::{B_INVAL, Buf};
use crate::sys::disk::DM_OPENBLCK;
use crate::sys::dkio::DIOCCACHESYNC;
use crate::sys::errno::Errno;
use crate::sys::fcntl::{FREAD, FWRITE};
use crate::sys::lock::{LK_EXCLUSIVE, LK_NOWAIT, LK_RETRY};
use crate::sys::malloc::{M_CANFAIL, M_MSDOSFSFAT, M_MSDOSFSMNT, M_WAITOK, M_ZERO};
use crate::sys::mbuf::Mbuf;
use crate::sys::mount::{
    Fid, MNAMELEN, MNT_FORCE, MNT_LAZY, MNT_LOCAL, MNT_RDONLY, MNT_RELOAD, MNT_SYNCHRONOUS,
    MNT_UPDATE, MNT_WAIT, MNT_WANTRDWR, MSDOSFSMNT_LONGNAME, MSDOSFSMNT_NOWIN95,
    MSDOSFSMNT_SHORTNAME, Mount, MsdosfsArgs, Statfs, VFS_SYNC, Vfsops,
};
use crate::sys::namei::{FOLLOW, LOOKUP, Nameidata, NiDirp};
use crate::sys::param::{DEV_BSIZE, MAXBSIZE, howmany};
use crate::sys::proc::Proc;
use crate::sys::systm::INFSLP;
use crate::sys::types::major;
use crate::sys::ucred::{FSCRED, NOCRED, Ucred};
use crate::sys::vnode::{FORCECLOSE, V_SAVE, VBLK, VNON, Vnode, WRITECLOSE};

/// `struct msdosfs_sync_arg`: what `msdosfs_sync` hands `msdosfs_sync_vnode` for each vnode.
pub struct MsdosfsSyncArgs<'a> {
    /// `p`: the thread syncing.
    pub p: &'a Proc,
    /// `cred`: its credentials.
    pub cred: *const Ucred,
    /// `allerror`: the last error of a vnode's `VOP_FSYNC`.
    pub allerror: Result<(), Errno>,
    /// `waitfor`: `MNT_WAIT`, `MNT_NOWAIT` or `MNT_LAZY`.
    pub waitfor: i32,
}

/// `msdosfs_vfsops`.
pub static MSDOSFS_VFSOPS: Vfsops = Vfsops {
    vfs_mount: msdosfs_mount,
    vfs_start: msdosfs_start,
    vfs_unmount: msdosfs_unmount,
    vfs_root: msdosfs_root,
    vfs_quotactl: |_, _, _, _, _| Err(Errno::EOPNOTSUPP),
    vfs_statfs: msdosfs_statfs,
    vfs_sync: msdosfs_sync,
    vfs_vget: |_, _| Err(Errno::EOPNOTSUPP),
    vfs_fhtovp: msdosfs_fhtovp,
    vfs_vptofh: msdosfs_vptofh,
    vfs_init: Some(msdosfs_init),
    vfs_sysctl: None,
    vfs_checkexp: msdosfs_check_export,
};

/// `bzero(dst, MNAMELEN); strlcpy(dst, src, MNAMELEN)`.
fn mname_copy(dst: &mut [u8; MNAMELEN], src: &[u8]) {
    *dst = [0; MNAMELEN];
    let src = src.split(|&c| c == 0).next().unwrap_or(&[]);
    let n = src.len().min(MNAMELEN - 1);
    dst[..n].copy_from_slice(&src[..n]);
}

/// `msdosfs_mount` (`vfs_mount`): mount a msdos file system at `path`. `data` is the kernel
/// copy of the user's `struct msdosfs_args` (empty for the C's NULL), whose `fspec` names
/// the block special file to treat as a filesystem.
pub fn msdosfs_mount(
    mp: &'static Mount,
    path: &[u8],
    data: &mut [u8],
    _ndp: &mut Nameidata<'_>,
    p: &Proc,
) -> Result<(), Errno> {
    let args = MsdosfsArgs::from_bytes(data);
    let mut fname = [0u8; MNAMELEN];
    let mut fspec = [0u8; MNAMELEN];
    let mut updpmp: Option<&'static Msdosfsmount> = None;

    // If updating, check whether changing from read-only to read/write; if there is no
    // device name, that's all we do.
    if mp.mnt_flag.get() & MNT_UPDATE != 0 {
        let pmp = vfstomsdosfs(mp);
        updpmp = Some(pmp);
        let mut error = Ok(());
        if pmp.pm_flags.get() & MSDOSFSMNT_RONLY == 0 && mp.mnt_flag.get() & MNT_RDONLY != 0 {
            mp.mnt_flag.set(mp.mnt_flag.get() & !MNT_RDONLY);
            let _ = VFS_SYNC(mp, MNT_WAIT, 0, p.p_ucred.get(), p);
            mp.mnt_flag.set(mp.mnt_flag.get() | MNT_RDONLY);

            let mut flags = WRITECLOSE;
            if mp.mnt_flag.get() & MNT_FORCE != 0 {
                flags |= FORCECLOSE;
            }
            error = vflush(mp, None, flags);
            if error.is_ok() {
                let mut force = 0i32.to_ne_bytes();

                pmp.pm_flags.set(pmp.pm_flags.get() | MSDOSFSMNT_RONLY);
                // may be not supported, ignore error
                let _ = VOP_IOCTL(pmp.devvp(), DIOCCACHESYNC, &mut force, FWRITE, FSCRED, p);
            }
        }
        if error.is_ok() && mp.mnt_flag.get() & MNT_RELOAD != 0 {
            // not yet implemented
            error = Err(Errno::EOPNOTSUPP);
        }
        error?;
        if pmp.pm_flags.get() & MSDOSFSMNT_RONLY != 0 && mp.mnt_flag.get() & MNT_WANTRDWR != 0 {
            pmp.pm_flags.set(pmp.pm_flags.get() & !MSDOSFSMNT_RONLY);
        }

        match args {
            // Process export requests.
            Some(a) if a.fspec == 0 => {
                return vfs_export(mp, &pmp.pm_export, &a.export_info);
            }
            None => return Ok(()),
            Some(_) => {}
        }
    }

    // Not an update, or updating the name: look up the name and verify that it refers to a
    // sensible block device.
    let Some(args) = args else {
        return Err(Errno::EINVAL);
    };
    copyinstr(args.fspec, &mut fspec)?;

    if !disk_map(&fspec, &mut fname, DM_OPENBLCK) {
        fname = fspec;
    }

    let flen = fname.iter().position(|&c| c == 0).unwrap_or(MNAMELEN);
    let mut nd = ndinit(LOOKUP, FOLLOW, NiDirp::Sys(&fname[..flen]), p);
    namei(&mut nd)?;
    let Some(devvp) = nd.ni_vp else {
        return Err(Errno::ENOENT);
    };

    let error: Result<(), Errno> = 'error_devvp: {
        if devvp.v_type.get() != VBLK {
            break 'error_devvp Err(Errno::ENOTBLK);
        }
        if major(devvp.v_rdev()) >= nblkdev() {
            break 'error_devvp Err(Errno::ENXIO);
        }

        match updpmp {
            None => msdosfs_mountfs(devvp, mp, p, &args),
            Some(pmp) => {
                if !ptr::eq(devvp, pmp.devvp()) {
                    Err(Errno::EINVAL) // XXX needs translation
                } else {
                    vrele(devvp);
                    Ok(())
                }
            }
        }
    };
    if let Err(error) = error {
        // error_devvp:
        vrele(devvp);
        return Err(error);
    }

    let pmp = vfstomsdosfs(mp);
    pmp.pm_gid.set(args.gid);
    pmp.pm_uid.set(args.uid);
    pmp.pm_mask.set(args.mask);
    pmp.pm_flags
        .set(pmp.pm_flags.get() | (args.flags as u32 & MSDOSFSMNT_MNTOPT));

    if pmp.pm_flags.get() & MSDOSFSMNT_NOWIN95 as u32 != 0 {
        pmp.pm_flags
            .set(pmp.pm_flags.get() | MSDOSFSMNT_SHORTNAME as u32);
    } else if pmp.pm_flags.get() & (MSDOSFSMNT_SHORTNAME | MSDOSFSMNT_LONGNAME) as u32 == 0 {
        pmp.pm_flags
            .set(pmp.pm_flags.get() | MSDOSFSMNT_LONGNAME as u32);
    }

    let longname = pmp.pm_flags.get() & MSDOSFSMNT_LONGNAME as u32 != 0;
    mp.update_stat(|sp| {
        sp.f_namemax = if longname { WIN_MAXLEN as u32 } else { 12 };
        mname_copy(&mut sp.f_mntonname, path);
        mname_copy(&mut sp.f_mntfromname, &fname);
        mname_copy(&mut sp.f_mntfromspec, &fspec);
        sp.mount_info.__align[..MsdosfsArgs::SIZE].copy_from_slice(&data[..MsdosfsArgs::SIZE]);
    });

    Ok(())
}

/// `msdosfs_mountfs(devvp, mp, p, argp)`: mount the msdos file system on the block device
/// `devvp` at `mp`: read and check the boot sector's BIOS parameter block, work out the FAT
/// type and geometry, check the FAT32 FSInfo block, and fill the in-use cluster bitmap. On
/// success `mp.mnt_data` is the new `struct msdosfsmount` and `devvp`'s reference is the
/// mount's.
pub fn msdosfs_mountfs(
    devvp: &'static Vnode,
    mp: &'static Mount,
    p: &Proc,
    _argp: &MsdosfsArgs,
) -> Result<(), Errno> {
    let dev = devvp.v_rdev();

    // Disallow multiple mounts of the same device. Disallow mounting of a device that is
    // currently in use (except for root, which might share swap device for miniroot). Flush
    // out any old buffers remaining from a previous use.
    vfs_mountedon(devvp)?;
    if vcount(devvp) > 1 && !rootvp().is_some_and(|r| ptr::eq(r, devvp)) {
        return Err(Errno::EBUSY);
    }
    let _ = vn_lock(devvp, LK_EXCLUSIVE | LK_RETRY);
    let error = vinvalbuf(devvp, V_SAVE, p.p_ucred.get(), Some(p), 0, INFSLP);
    let _ = VOP_UNLOCK(devvp);
    error?;

    let ronly = mp.mnt_flag.get() & MNT_RDONLY != 0;
    let omode = if ronly { FREAD } else { FREAD | FWRITE };
    VOP_OPEN(devvp, omode, FSCRED, p)?;

    // Both used in error_exit.
    let mut bp: Option<&'static Buf>;
    let mut pmp: Option<&'static Msdosfsmount> = None;

    let error: Errno = 'error_exit: {
        // Read the boot sector of the filesystem, and then check the boot signature. If not
        // a dos boot sector then error out.
        let (b, error) = bread(devvp, 0, 4096);
        bp = Some(b);
        if let Err(error) = error {
            break 'error_exit error;
        }
        // SAFETY: the buffer is busy for this function (from `bread`) and mapped; the slice
        // is not used after the buffer is released below.
        let bsp = Bootsector::at(unsafe { b.data() }, 0);
        let b33 = ByteBpb33::at(&bsp.bs33().bsBPB, 0);
        let b50 = ByteBpb50::at(&bsp.bs50().bsBPB, 0);
        let b710 = ByteBpb710::at(&bsp.bs710().bsBPB, 0);

        let Some(mem) = malloc(size_of::<Msdosfsmount>(), M_MSDOSFSMNT, M_WAITOK | M_ZERO) else {
            panic(format_args!("msdosfs_mountfs: no memory"));
        };
        let m = mem.cast::<Msdosfsmount>();
        // SAFETY: a fresh allocation of `size_of::<Msdosfsmount>()` bytes, aligned by
        // malloc(9), written once before anything else sees it.
        unsafe { m.as_ptr().write(Msdosfsmount::new()) };
        // SAFETY: as above; it lives until `msdosfs_unmount` (or the error path below)
        // frees it.
        let pm: &'static Msdosfsmount = unsafe { m.as_ref() };
        pmp = Some(pm);
        pm.pm_mountp.set(Some(mp));

        // Compute several useful quantities from the bpb in the bootsector. Copy in the dos 5
        // variant of the bpb then fix up the fields that are different between dos 5 and dos
        // 3.3.
        let mut sec_per_clust = u32::from(b50.bpbSecPerClust);
        pm.set_pm_BytesPerSec(getushort(&b50.bpbBytesPerSec));
        pm.set_pm_ResSectors(getushort(&b50.bpbResSectors));
        pm.set_pm_FATs(b50.bpbFATs);
        pm.set_pm_RootDirEnts(getushort(&b50.bpbRootDirEnts));
        pm.set_pm_Sectors(getushort(&b50.bpbSectors));
        pm.pm_FATsecs.set(u32::from(getushort(&b50.bpbFATsecs)));
        pm.set_pm_SecPerTrack(getushort(&b50.bpbSecPerTrack));
        pm.set_pm_Heads(getushort(&b50.bpbHeads));
        pm.set_pm_Media(b50.bpbMedia);

        // Determine the number of DEV_BSIZE blocks in a MSDOSFS sector
        pm.pm_BlkPerSec
            .set(u32::from(pm.pm_BytesPerSec()) / DEV_BSIZE as u32);

        if pm.pm_BytesPerSec() == 0 || sec_per_clust == 0 {
            break 'error_exit Errno::EINVAL;
        }

        if pm.pm_Sectors() == 0 {
            pm.set_pm_HiddenSects(getulong(&b50.bpbHiddenSecs));
            pm.set_pm_HugeSectors(getulong(&b50.bpbHugeSectors));
        } else {
            pm.set_pm_HiddenSects(u32::from(getushort(&b33.bpbHiddenSecs)));
            pm.set_pm_HugeSectors(u32::from(pm.pm_Sectors()));
        }

        if pm.pm_RootDirEnts() == 0 {
            if pm.pm_Sectors() != 0 || pm.pm_FATsecs.get() != 0 || getushort(&b710.bpbFSVers) != 0 {
                break 'error_exit Errno::EINVAL;
            }
            pm.pm_fatmask.set(FAT32_MASK);
            pm.pm_fatmult.set(4);
            pm.pm_fatdiv.set(1);
            pm.pm_FATsecs.set(getulong(&b710.bpbBigFATsecs));
            let extflags = getushort(&b710.bpbExtFlags);
            if extflags & FATMIRROR != 0 {
                pm.pm_curfat.set(u32::from(extflags & FATNUM));
            } else {
                pm.pm_flags.set(pm.pm_flags.get() | MSDOSFS_FATMIRROR);
            }
        } else {
            pm.pm_flags.set(pm.pm_flags.get() | MSDOSFS_FATMIRROR);
        }

        // More sanity checks:
        //	MSDOSFS sectors per cluster: >0 && power of 2
        //	MSDOSFS sector size: >= DEV_BSIZE && power of 2
        //	HUGE sector count: >0
        //	FAT sectors: >0
        let bytes_per_sec = u32::from(pm.pm_BytesPerSec());
        if sec_per_clust == 0
            || !sec_per_clust.is_power_of_two()
            || bytes_per_sec < DEV_BSIZE as u32
            || !bytes_per_sec.is_power_of_two()
            || pm.pm_HugeSectors() == 0
            || pm.pm_FATsecs.get() == 0
            || sec_per_clust * pm.pm_BlkPerSec.get() > (MAXBSIZE / DEV_BSIZE) as u32
        {
            break 'error_exit Errno::EINVAL;
        }

        let blkpersec = pm.pm_BlkPerSec.get();
        pm.set_pm_HugeSectors(pm.pm_HugeSectors().wrapping_mul(blkpersec));
        pm.set_pm_HiddenSects(pm.pm_HiddenSects().wrapping_mul(blkpersec));
        pm.pm_FATsecs
            .set(pm.pm_FATsecs.get().wrapping_mul(blkpersec));
        pm.pm_fatblk.set(u32::from(pm.pm_ResSectors()) * blkpersec);
        // At most MAXBSIZE / DEV_BSIZE (checked above), as the C's u_int8_t holds it.
        sec_per_clust *= blkpersec;

        let fats_size = u32::from(pm.pm_FATs()).wrapping_mul(pm.pm_FATsecs.get());
        if fat32(pm) {
            pm.pm_rootdirblk.set(getulong(&b710.bpbRootClust));
            pm.pm_firstcluster
                .set(pm.pm_fatblk.get().wrapping_add(fats_size));
            pm.pm_fsinfo
                .set(u32::from(getushort(&b710.bpbFSInfo)) * blkpersec);
        } else {
            pm.pm_rootdirblk
                .set(pm.pm_fatblk.get().wrapping_add(fats_size));
            pm.pm_rootdirsize.set(
                (u32::from(pm.pm_RootDirEnts()) * Direntry::SIZE as u32).div_ceil(DEV_BSIZE as u32),
            );
            pm.pm_firstcluster
                .set(pm.pm_rootdirblk.get().wrapping_add(pm.pm_rootdirsize.get()));
        }

        pm.pm_nmbrofclusters
            .set(pm.pm_HugeSectors().wrapping_sub(pm.pm_firstcluster.get()) / sec_per_clust);
        pm.pm_maxcluster
            .set(pm.pm_nmbrofclusters.get().wrapping_add(1));
        pm.pm_fatsize
            .set(pm.pm_FATsecs.get().wrapping_mul(DEV_BSIZE as u32));

        if pm.pm_fatmask.get() == 0 {
            if pm.pm_maxcluster.get() <= (CLUST_RSRVD - CLUST_FIRST) & FAT12_MASK {
                // This will usually be a floppy disk. This size makes sure that one fat entry
                // will not be split across multiple blocks.
                pm.pm_fatmask.set(FAT12_MASK);
                pm.pm_fatmult.set(3);
                pm.pm_fatdiv.set(2);
            } else {
                pm.pm_fatmask.set(FAT16_MASK);
                pm.pm_fatmult.set(2);
                pm.pm_fatdiv.set(1);
            }
        }
        if fat12(pm) {
            pm.pm_fatblocksize.set(3 * bytes_per_sec);
        } else {
            pm.pm_fatblocksize.set(MAXBSIZE as u32);
        }

        // We now have the number of sectors in each FAT, so can work out how many clusters
        // can be represented in a FAT. Let's make sure the file system doesn't claim to have
        // more clusters than this.
        //
        // We perform the calculation like we do to avoid integer overflow.
        //
        // This will give us a count of clusters. They are numbered from 0, so the max cluster
        // value is one less than the value we end up with.
        let fat_max_clusters =
            (pm.pm_fatsize.get() / pm.pm_fatmult.get()).wrapping_mul(pm.pm_fatdiv.get());
        if pm.pm_maxcluster.get() >= fat_max_clusters {
            printf(format_args!(
                "msdosfs: reducing max cluster to {} from {} due to FAT size\n",
                fat_max_clusters.wrapping_sub(1) as i32,
                pm.pm_maxcluster.get() as i32
            ));
            pm.pm_maxcluster.set(fat_max_clusters.wrapping_sub(1));
        }

        pm.pm_fatblocksec
            .set(pm.pm_fatblocksize.get() / DEV_BSIZE as u32);
        pm.pm_bnshift.set(DEV_BSIZE.trailing_zeros());

        // Compute mask and shift value for isolating cluster relative byte offsets and
        // cluster numbers from a file offset.
        pm.pm_bpcluster.set(sec_per_clust * DEV_BSIZE as u32);
        pm.pm_crbomask.set(pm.pm_bpcluster.get() - 1);
        pm.pm_cnshift.set(pm.pm_bpcluster.get().trailing_zeros());

        // Check for valid cluster size; must be a power of 2
        if pm.pm_bpcluster.get() ^ (1 << pm.pm_cnshift.get()) != 0 {
            break 'error_exit Errno::EINVAL;
        }

        // Release the bootsector buffer.
        brelse(b);
        bp = None;

        // Check FSInfo
        if pm.pm_fsinfo.get() != 0 {
            let (b, error) = bread(devvp, i64::from(pm.pm_fsinfo.get()), fsi_size(pm));
            bp = Some(b);
            if let Err(error) = error {
                break 'error_exit error;
            }
            // SAFETY: the buffer is busy for this function (from `bread`) and mapped; the
            // slice ends with the statement.
            let fp = *Fsinfo::at(unsafe { b.data() }, 0);
            if &fp.fsisig1 == b"RRaA"
                && &fp.fsisig2 == b"rrAa"
                && fp.fsisig3 == [0, 0, 0o125, 0o252]
                && fp.fsisig4 == [0, 0, 0o125, 0o252]
            {
                // Valid FSInfo.
            } else {
                pm.pm_fsinfo.set(0);
            }
            // XXX make sure this tiny buf doesn't come back in fillinusemap!
            b.set(B_INVAL);
            brelse(b);
            bp = None;
        }

        // Check and validate (or perhaps invalidate?) the fsinfo structure? XXX

        // Allocate memory for the bitmap of allocated clusters, and then fill it in.
        let bmapsiz = howmany(
            pm.pm_maxcluster.get().wrapping_add(1) as usize,
            N_INUSEBITS as usize,
        );
        if bmapsiz == 0 || usize::MAX / bmapsiz < size_of::<u32>() {
            // detect multiplicative integer overflow
            break 'error_exit Errno::EINVAL;
        }
        let Some(map) = mallocarray(
            bmapsiz,
            size_of::<u32>(),
            M_MSDOSFSFAT,
            M_WAITOK | M_CANFAIL,
        ) else {
            break 'error_exit Errno::EINVAL;
        };
        pm.pm_inusemap.set(map.as_ptr().cast());

        // fillinusemap() needs pm_devvp.
        pm.pm_dev.set(dev);
        pm.pm_devvp.set(Some(devvp));

        // Have the inuse map filled in.
        if let Err(error) = fillinusemap(pm) {
            break 'error_exit error;
        }

        // If they want fat updates to be synchronous then let them suffer the performance
        // degradation in exchange for the on disk copy of the fat being correct just about
        // all the time. I suppose this would be a good thing to turn on if the kernel is
        // still flakey.
        if mp.mnt_flag.get() & MNT_SYNCHRONOUS != 0 {
            pm.pm_flags.set(pm.pm_flags.get() | MSDOSFSMNT_WAITONFAT);
        }

        // Finish up.
        if ronly {
            pm.pm_flags.set(pm.pm_flags.get() | MSDOSFSMNT_RONLY);
        } else {
            pm.pm_fmod.set(1);
        }
        mp.mnt_data.set(m.as_ptr().cast());
        mp.update_stat(|sp| {
            sp.f_fsid.val[0] = dev;
            sp.f_fsid.val[1] = mp.vfc().vfc_typenum;
        });
        // QUOTA: if we ever do quotas for DOS filesystems this would be a place to fill in
        // the info in the msdosfsmount structure. You dolt, quotas on dos filesystems make no
        // sense because files have no owners on dos filesystems. of course there is some
        // empty space in the directory entry where we could put uid's and gid's.
        if let Some(si) = devvp.v_specinfo() {
            si.si_mountpoint.set(Some(mp));
        }

        return Ok(());
    };

    // error_exit:
    if let Some(si) = devvp.v_specinfo() {
        si.si_mountpoint.set(None);
    }
    if let Some(b) = bp {
        brelse(b);
    }

    let _ = vn_lock(devvp, LK_EXCLUSIVE | LK_RETRY);
    let _ = VOP_CLOSE(devvp, omode, NOCRED, Some(p));
    let _ = VOP_UNLOCK(devvp);

    if let Some(pm) = pmp {
        if let Some(map) = NonNull::new(pm.pm_inusemap.get()) {
            free(map.cast(), M_MSDOSFSFAT, 0);
        }
        free(NonNull::from(pm).cast(), M_MSDOSFSMNT, 0);
        mp.mnt_data.set(ptr::null_mut());
    }
    Err(error)
}

/// `msdosfs_start` (`vfs_start`): make a filesystem operational; nothing to do.
pub fn msdosfs_start(_mp: &'static Mount, _flags: i32, _p: &Proc) -> Result<(), Errno> {
    Ok(())
}

/// `msdosfs_unmount` (`vfs_unmount`): unmount the filesystem described by `mp`.
pub fn msdosfs_unmount(mp: &'static Mount, mntflags: i32, p: &Proc) -> Result<(), Errno> {
    let mut flags = 0;
    if mntflags & MNT_FORCE != 0 {
        flags |= FORCECLOSE;
    }
    vflush(mp, None, flags)?;
    let pmp = vfstomsdosfs(mp);
    let vp = pmp.devvp();
    if let Some(si) = vp.v_specinfo() {
        si.si_mountpoint.set(None);
    }
    let _ = vn_lock(vp, LK_EXCLUSIVE | LK_RETRY);
    let omode = if pmp.pm_flags.get() & MSDOSFSMNT_RONLY != 0 {
        FREAD
    } else {
        FREAD | FWRITE
    };
    let _ = VOP_CLOSE(vp, omode, NOCRED, Some(p));
    vput(vp);
    if let Some(map) = NonNull::new(pmp.pm_inusemap.get()) {
        free(map.cast(), M_MSDOSFSFAT, 0);
    }
    free(NonNull::from(pmp).cast(), M_MSDOSFSMNT, 0);
    mp.mnt_data.set(ptr::null_mut());
    mp.mnt_flag.set(mp.mnt_flag.get() & !MNT_LOCAL);
    Ok(())
}

/// `msdosfs_root` (`vfs_root`): the root directory's vnode, referenced and locked.
pub fn msdosfs_root(mp: &'static Mount) -> Result<&'static Vnode, Errno> {
    let pmp = vfstomsdosfs(mp);
    let ndep = deget(pmp, MSDOSFSROOT, MSDOSFSROOT_OFS)?;
    Ok(ndep.detov())
}

/// `msdosfs_statfs` (`vfs_statfs`): get file system statistics.
pub fn msdosfs_statfs(mp: &'static Mount, sbp: &mut Statfs, _p: &Proc) -> Result<(), Errno> {
    let pmp = vfstomsdosfs(mp);
    sbp.f_bsize = pmp.pm_bpcluster.get();
    sbp.f_iosize = pmp.pm_bpcluster.get();
    sbp.f_blocks = u64::from(pmp.pm_nmbrofclusters.get());
    sbp.f_bfree = u64::from(pmp.pm_freeclustercount.get());
    sbp.f_bavail = i64::from(pmp.pm_freeclustercount.get());
    sbp.f_files = u64::from(pmp.pm_RootDirEnts()); // XXX
    sbp.f_ffree = 0; // what to put in here?
    sbp.f_favail = 0;
    copy_statfs_info(sbp, mp);

    Ok(())
}

/// `msdosfs_sync_vnode(vp, arg)`: write back one (modified) denode for `msdosfs_sync`.
pub fn msdosfs_sync_vnode(vp: &'static Vnode, msa: &mut MsdosfsSyncArgs<'_>) -> Result<(), Errno> {
    let s = splbio();
    let skip = vp.v_type.get() == VNON
        || (vtode(vp).de_flag.get() & (DE_ACCESS | DE_CREATE | DE_UPDATE | DE_MODIFIED) == 0
            && vp.v_dirtyblkhd.is_empty())
        || msa.waitfor == MNT_LAZY;
    splx(s);

    if skip {
        return Ok(());
    }

    if vget(vp, LK_EXCLUSIVE | LK_NOWAIT).is_err() {
        return Ok(());
    }

    if let Err(error) = VOP_FSYNC(vp, msa.cred, msa.waitfor, msa.p) {
        msa.allerror = Err(error);
    }
    let _ = VOP_UNLOCK(vp);
    vrele(vp);

    Ok(())
}

/// `msdosfs_sync` (`vfs_sync`): write back the modified denodes and the device's dirty
/// buffers.
pub fn msdosfs_sync(
    mp: &'static Mount,
    waitfor: i32,
    _stall: i32,
    cred: *const Ucred,
    p: &Proc,
) -> Result<(), Errno> {
    let pmp = vfstomsdosfs(mp);
    let mut msa = MsdosfsSyncArgs {
        p,
        cred,
        allerror: Ok(()),
        waitfor,
    };

    // If we ever switch to not updating all of the fats all the time, this would be the
    // place to update them from the first one.
    if pmp.pm_fmod.get() != 0 {
        if pmp.pm_flags.get() & MSDOSFSMNT_RONLY != 0 {
            panic(format_args!("msdosfs_sync: rofs mod"));
        } else {
            // update fats here
        }
    }
    // Write back each (modified) denode.
    let _ = vfs_mount_foreach_vnode(mp, &mut |vp| msdosfs_sync_vnode(vp, &mut msa));

    // Force stale file system control information to be flushed.
    if waitfor != MNT_LAZY {
        let devvp = pmp.devvp();
        let _ = vn_lock(devvp, LK_EXCLUSIVE | LK_RETRY);
        if let Err(error) = VOP_FSYNC(devvp, cred, waitfor, p) {
            msa.allerror = Err(error);
        }
        let _ = VOP_UNLOCK(devvp);
    }

    msa.allerror
}

/// `msdosfs_fhtovp` (`vfs_fhtovp`): the vnode, locked, of the file handle `fhp`.
pub fn msdosfs_fhtovp(mp: &'static Mount, fhp: &Fid) -> Result<&'static Vnode, Errno> {
    let pmp = vfstomsdosfs(mp);
    let defhp = Defid::from_fid(fhp);

    let dep = deget(pmp, defhp.defid_dirclust, defhp.defid_dirofs)?;
    Ok(dep.detov())
}

/// `msdosfs_vptofh` (`vfs_vptofh`): the file handle of `vp`, the position of its directory
/// entry.
pub fn msdosfs_vptofh(vp: &'static Vnode, fhp: &mut Fid) -> Result<(), Errno> {
    let dep = vtode(vp);
    let mut defhp = Defid::from_fid(fhp);
    defhp.defid_len = size_of::<Defid>() as u16;
    defhp.defid_dirclust = dep.de_dirclust.get();
    defhp.defid_dirofs = dep.de_diroffset.get();
    // defhp->defid_gen = dep->de_gen;
    defhp.to_fid(fhp);
    Ok(())
}

/// `msdosfs_check_export` (`vfs_checkexp`): verify a remote client has export rights and
/// return these rights via `exflagsp` and `credanonp`.
pub fn msdosfs_check_export(
    mp: &'static Mount,
    nam: &Mbuf,
    exflagsp: &mut i32,
    credanonp: &mut *const Ucred,
) -> Result<(), Errno> {
    let pmp = vfstomsdosfs(mp);

    // Get the export permission structure for this <mp, client> tuple.
    let Some(np) = vfs_export_lookup(mp, &pmp.pm_export, Some(nam)) else {
        return Err(Errno::EACCES);
    };

    *exflagsp = np.netc_exflags.get();
    *credanonp = ptr::from_ref(&np.netc_anon);
    Ok(())
}
/* </CODE> */

/* <TESTS> */
#[cfg(test)]
pub(crate) mod tests {
    // Host tests for the msdos file system's mounting: `mkfat`, a small `newfs_msdos(8)`
    // equivalent that lays out a FAT12, FAT16 or FAT32 image in memory (boot sector, FSInfo,
    // FATs, root directory, files and subdirectories with short and Win95 long names), a block
    // device vnode whose strategy reads and writes that image, and tests that mount it with
    // `msdosfs_mountfs`, check the geometry it worked out and `statfs`, reject bad boot
    // sectors, and unmount. `msdosfs_lookup.rs`'s tests use the same image and disk.

    use core::ptr;
    use core::sync::atomic::Ordering;
    use std::sync::MutexGuard;
    use std::vec::Vec;

    use super::*;
    use crate::kern::subr_xxx::nullop;
    use crate::kern::vfs_bio::{BCSTATS, BUFHEAD, BUFKVM, CLEANCACHE, biodone, bufinit};
    use crate::kern::vfs_default::vop_generic_bwrite;
    use crate::kern::vfs_init::vfs_byname;
    use crate::kern::vfs_subr::{
        MOUNTLIST, bdevvp, vflushbuf, vfs_busy, vfs_mount_alloc, vfs_unbusy,
    };
    use crate::kern::vfs_syscalls::dounmount;
    use crate::machine::Machine;
    use crate::machine::cpu::Cpu;
    use crate::msdosfs::fat::fat16;
    use crate::sys::buf::{B_ERROR, B_READ};
    use crate::sys::mount::{MOUNT_MSDOS, VB_WAIT, VB_WRITE};
    use crate::sys::types::makedev;
    use crate::sys::vnode::{VopFsyncArgs, VopInactiveArgs, VopStrategyArgs, Vops};

    /// `mkfat`: a FAT image built in memory the way `newfs_msdos(8)` and `makefs -t msdos` build
    /// one, for the tests and as a record of the on-disk layout.
    pub(crate) mod mkfat {
        use std::vec;
        use std::vec::Vec;

        /// The geometry to build.
        #[derive(Clone, Copy)]
        pub(crate) struct Params {
            /// 12, 16 or 32: the FAT width (FAT32 means no fixed root directory).
            pub(crate) fat: u32,
            /// The image's size in bytes.
            pub(crate) size: usize,
            /// Bytes per sector.
            pub(crate) bps: u16,
            /// Sectors per cluster.
            pub(crate) spc: u8,
            /// Root directory entries (0 for FAT32).
            pub(crate) rde: u16,
        }

        /// FAT12, 1 MB, 512-byte clusters, 64 root directory entries.
        pub(crate) const FAT12_1M: Params = Params {
            fat: 12,
            size: 1 << 20,
            bps: 512,
            spc: 1,
            rde: 64,
        };

        /// FAT16, 4 MB, 512-byte clusters, 512 root directory entries.
        pub(crate) const FAT16_4M: Params = Params {
            fat: 16,
            size: 4 << 20,
            bps: 512,
            spc: 1,
            rde: 512,
        };

        /// FAT32, 1 MB, 512-byte clusters (a FAT32 the kernel accepts: the type comes from the
        /// missing root directory, not from the cluster count).
        pub(crate) const FAT32_1M: Params = Params {
            fat: 32,
            size: 1 << 20,
            bps: 512,
            spc: 1,
            rde: 0,
        };

        /// Where a directory is: the fixed root directory of FAT12/16, or a cluster chain.
        #[derive(Clone, Copy, Debug, PartialEq, Eq)]
        pub(crate) enum Dir {
            /// The FAT12/16 root directory.
            Root,
            /// The directory whose first cluster is this one.
            Clust(u32),
        }

        /// An image under construction.
        pub(crate) struct Image {
            /// The disk.
            pub(crate) disk: Vec<u8>,
            /// The geometry.
            pub(crate) p: Params,
            /// Reserved sectors.
            pub(crate) res: u32,
            /// Sectors per FAT.
            pub(crate) spf: u32,
            /// Sectors of the root directory (FAT12/16).
            pub(crate) rootsecs: u32,
            /// Data clusters.
            pub(crate) nclusters: u32,
            /// The next cluster handed out.
            next: u32,
        }

        /// The Win95 checksum of a DOS name.
        pub(crate) fn chksum(name: &[u8; 11]) -> u8 {
            name.iter().fold(0u8, |s, &c| {
                ((s & 1) << 7).wrapping_add(s >> 1).wrapping_add(c)
            })
        }

        /// The Win95 long name entries of `name` for the DOS name `short`, in on-disk order (the
        /// last part first).
        pub(crate) fn lfn_entries(name: &[u8], short: &[u8; 11]) -> Vec<[u8; 32]> {
            let ck = chksum(short);
            let n = name.len().div_ceil(13);
            let mut out = Vec::new();
            for i in (1..=n).rev() {
                let mut e = [0u8; 32];
                e[0] = i as u8 | if i == n { 0x40 } else { 0 };
                e[11] = 0x0f;
                e[13] = ck;
                let offs = [1, 3, 5, 7, 9, 14, 16, 18, 20, 22, 24, 28, 30];
                for (k, &o) in offs.iter().enumerate() {
                    let idx = (i - 1) * 13 + k;
                    let c: u16 = match idx.cmp(&name.len()) {
                        core::cmp::Ordering::Less => u16::from(name[idx]),
                        core::cmp::Ordering::Equal => 0,
                        core::cmp::Ordering::Greater => 0xffff,
                    };
                    e[o..o + 2].copy_from_slice(&c.to_le_bytes());
                }
                out.push(e);
            }
            out
        }

        /// A short directory entry.
        pub(crate) fn short_entry(short: &[u8; 11], attr: u8, cluster: u32, size: u32) -> [u8; 32] {
            let mut e = [0u8; 32];
            e[..11].copy_from_slice(short);
            e[11] = attr;
            e[20..22].copy_from_slice(&((cluster >> 16) as u16).to_le_bytes());
            e[24..26].copy_from_slice(&0x21u16.to_le_bytes()); // 1980-01-01
            e[26..28].copy_from_slice(&(cluster as u16).to_le_bytes());
            e[28..32].copy_from_slice(&size.to_le_bytes());
            e
        }

        impl Image {
            /// `newfs_msdos`: an empty file system.
            pub(crate) fn new(p: Params) -> Self {
                let bps = u32::from(p.bps);
                let sectors = (p.size / p.bps as usize) as u32;
                let res = if p.fat == 32 { 32 } else { 1 };
                let rootsecs = (u32::from(p.rde) * 32).div_ceil(bps);
                let mut spf = 1;
                let nclusters = loop {
                    let data = sectors - res - 2 * spf - rootsecs;
                    let ncl = data / u32::from(p.spc);
                    let need = ((ncl + 2) * p.fat).div_ceil(8).div_ceil(bps);
                    if need <= spf {
                        break ncl;
                    }
                    spf = need;
                };
                let mut img = Image {
                    disk: vec![0u8; p.size],
                    p,
                    res,
                    spf,
                    rootsecs,
                    nclusters,
                    next: 2,
                };

                let d = &mut img.disk;
                d[0..3].copy_from_slice(&[0xeb, 0x3c, 0x90]);
                d[3..11].copy_from_slice(b"EMIBSD  ");
                d[11..13].copy_from_slice(&p.bps.to_le_bytes());
                d[13] = p.spc;
                d[14..16].copy_from_slice(&(res as u16).to_le_bytes());
                d[16] = 2;
                d[17..19].copy_from_slice(&p.rde.to_le_bytes());
                if sectors < 65536 && p.fat != 32 {
                    d[19..21].copy_from_slice(&(sectors as u16).to_le_bytes());
                } else {
                    d[32..36].copy_from_slice(&sectors.to_le_bytes());
                }
                d[21] = 0xf8;
                if p.fat != 32 {
                    d[22..24].copy_from_slice(&(spf as u16).to_le_bytes());
                }
                d[24..26].copy_from_slice(&63u16.to_le_bytes());
                d[26..28].copy_from_slice(&255u16.to_le_bytes());
                if p.fat == 32 {
                    d[36..40].copy_from_slice(&spf.to_le_bytes());
                    d[44..48].copy_from_slice(&2u32.to_le_bytes()); // root cluster
                    d[48..50].copy_from_slice(&1u16.to_le_bytes()); // FSInfo sector
                    d[50..52].copy_from_slice(&6u16.to_le_bytes()); // backup boot sector
                    d[66] = 0x29;
                    d[82..90].copy_from_slice(b"FAT32   ");
                    // FSInfo, 1024 bytes from sector 1.
                    let fsi = bps as usize;
                    d[fsi..fsi + 4].copy_from_slice(b"RRaA");
                    d[fsi + 484..fsi + 488].copy_from_slice(b"rrAa");
                    d[fsi + 488..fsi + 492].copy_from_slice(&u32::MAX.to_le_bytes());
                    d[fsi + 492..fsi + 496].copy_from_slice(&u32::MAX.to_le_bytes());
                    d[fsi + 508..fsi + 512].copy_from_slice(&[0, 0, 0x55, 0xaa]);
                    d[fsi + 1020..fsi + 1024].copy_from_slice(&[0, 0, 0x55, 0xaa]);
                } else {
                    d[38] = 0x29;
                    d[54..62].copy_from_slice(if p.fat == 12 {
                        b"FAT12   "
                    } else {
                        b"FAT16   "
                    });
                }
                d[510] = 0x55;
                d[511] = 0xaa;

                let mask = img.mask();
                img.fat_set(0, 0xffff_fff8 & mask);
                img.fat_set(1, mask);
                if p.fat == 32 {
                    let root = img.alloc(1);
                    assert_eq!(root, 2);
                }
                img
            }

            /// The FAT entry mask.
            pub(crate) fn mask(&self) -> u32 {
                match self.p.fat {
                    12 => 0xfff,
                    16 => 0xffff,
                    _ => 0x0fff_ffff,
                }
            }

            /// Bytes per cluster.
            pub(crate) fn bpc(&self) -> usize {
                usize::from(self.p.bps) * usize::from(self.p.spc)
            }

            /// The byte offset of the first data cluster.
            fn data_off(&self) -> usize {
                (self.res + 2 * self.spf + self.rootsecs) as usize * usize::from(self.p.bps)
            }

            /// The byte offset of cluster `cn`.
            pub(crate) fn clust_off(&self, cn: u32) -> usize {
                self.data_off() + (cn as usize - 2) * self.bpc()
            }

            /// The byte offset of the FAT12/16 root directory.
            pub(crate) fn root_off(&self) -> usize {
                (self.res + 2 * self.spf) as usize * usize::from(self.p.bps)
            }

            /// The entry of cluster `cn` in the first FAT.
            pub(crate) fn fat_get(&self, cn: u32) -> u32 {
                let base = self.res as usize * usize::from(self.p.bps);
                match self.p.fat {
                    12 => {
                        let o = base + cn as usize * 3 / 2;
                        let v = u16::from_le_bytes([self.disk[o], self.disk[o + 1]]);
                        u32::from(if cn & 1 != 0 { v >> 4 } else { v & 0xfff })
                    }
                    16 => {
                        let o = base + cn as usize * 2;
                        u32::from(u16::from_le_bytes([self.disk[o], self.disk[o + 1]]))
                    }
                    _ => {
                        let o = base + cn as usize * 4;
                        u32::from_le_bytes(self.disk[o..o + 4].try_into().unwrap()) & 0x0fff_ffff
                    }
                }
            }

            /// Sets the entry of cluster `cn` in both FATs.
            pub(crate) fn fat_set(&mut self, cn: u32, v: u32) {
                for f in 0..2 {
                    let base = (self.res + f * self.spf) as usize * usize::from(self.p.bps);
                    match self.p.fat {
                        12 => {
                            let o = base + cn as usize * 3 / 2;
                            let old = u16::from_le_bytes([self.disk[o], self.disk[o + 1]]);
                            let v = v as u16 & 0xfff;
                            let new = if cn & 1 != 0 {
                                (old & 0x000f) | (v << 4)
                            } else {
                                (old & 0xf000) | v
                            };
                            self.disk[o..o + 2].copy_from_slice(&new.to_le_bytes());
                        }
                        16 => {
                            let o = base + cn as usize * 2;
                            self.disk[o..o + 2].copy_from_slice(&(v as u16).to_le_bytes());
                        }
                        _ => {
                            let o = base + cn as usize * 4;
                            self.disk[o..o + 4].copy_from_slice(&v.to_le_bytes());
                        }
                    }
                }
            }

            /// Allocates a chain of `n` clusters, zeroed; its first cluster.
            pub(crate) fn alloc(&mut self, n: u32) -> u32 {
                let first = self.next;
                for i in 0..n {
                    let cn = first + i;
                    let v = if i + 1 == n { self.mask() } else { cn + 1 };
                    self.fat_set(cn, v);
                    let o = self.clust_off(cn);
                    let bpc = self.bpc();
                    self.disk[o..o + bpc].fill(0);
                }
                self.next += n;
                first
            }

            /// The byte offsets of the slots of `dir`, in order.
            pub(crate) fn slots(&self, dir: Dir) -> Vec<usize> {
                match dir {
                    Dir::Root => {
                        let o = self.root_off();
                        (0..usize::from(self.p.rde)).map(|i| o + i * 32).collect()
                    }
                    Dir::Clust(mut cn) => {
                        let mut v = Vec::new();
                        loop {
                            let o = self.clust_off(cn);
                            v.extend((0..self.bpc() / 32).map(|i| o + i * 32));
                            let next = self.fat_get(cn);
                            if next >= (0x0fff_fff8 & self.mask()) {
                                break v;
                            }
                            cn = next;
                        }
                    }
                }
            }

            /// The FAT32 root directory, or the fixed one.
            pub(crate) fn root(&self) -> Dir {
                if self.p.fat == 32 {
                    Dir::Clust(2)
                } else {
                    Dir::Root
                }
            }

            /// Appends raw entries to `dir`, after its last used slot; the byte offset of the
            /// first one in the directory.
            pub(crate) fn put(&mut self, dir: Dir, entries: &[[u8; 32]]) -> u32 {
                let slots = self.slots(dir);
                let first = slots
                    .iter()
                    .position(|&o| self.disk[o] == 0)
                    .expect("directory full");
                assert!(first + entries.len() <= slots.len(), "directory full");
                for (k, e) in entries.iter().enumerate() {
                    let o = slots[first + k];
                    self.disk[o..o + 32].copy_from_slice(e);
                }
                (first * 32) as u32
            }

            /// Adds a file: its data in new clusters and its entry (with long name entries when
            /// `long` is given) in `dir`; the directory offset of the short entry.
            pub(crate) fn add_file(
                &mut self,
                dir: Dir,
                long: Option<&[u8]>,
                short: &[u8; 11],
                data: &[u8],
            ) -> u32 {
                let n = data.len().div_ceil(self.bpc()) as u32;
                let cn = if n == 0 { 0 } else { self.alloc(n) };
                for (i, chunk) in data.chunks(self.bpc()).enumerate() {
                    let o = self.clust_off(cn + i as u32);
                    self.disk[o..o + chunk.len()].copy_from_slice(chunk);
                }
                let mut ents = long.map_or(Vec::new(), |l| lfn_entries(l, short));
                ents.push(short_entry(short, 0x20, cn, data.len() as u32));
                self.put(dir, &ents) + 32 * (ents.len() as u32 - 1)
            }

            /// Adds a directory of `nclust` clusters with its "." and ".." entries; its first
            /// cluster.
            pub(crate) fn mkdir(&mut self, parent: Dir, short: &[u8; 11], nclust: u32) -> u32 {
                let cn = self.alloc(nclust);
                let pcn = match parent {
                    Dir::Root => 0,
                    Dir::Clust(c) if self.p.fat == 32 && c == 2 => 0,
                    Dir::Clust(c) => c,
                };
                let o = self.clust_off(cn);
                self.disk[o..o + 32].copy_from_slice(&short_entry(b".          ", 0x10, cn, 0));
                self.disk[o + 32..o + 64].copy_from_slice(&short_entry(
                    b"..         ",
                    0x10,
                    pcn,
                    0,
                ));
                self.put(parent, &[short_entry(short, 0x10, cn, 0)]);
                cn
            }

            /// The finished image.
            pub(crate) fn finish(self) -> Vec<u8> {
                self.disk
            }
        }
    }

    /// The disk the strategy below reads and writes.
    pub(crate) static DISK: std::sync::Mutex<Vec<u8>> = std::sync::Mutex::new(Vec::new());

    /// The fake disk's strategy: a synchronous transfer between the buffer and the image at
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

    /// The operations of the fake disk's block device vnode.
    static DISK_VOPS: Vops = Vops {
        vop_open: Some(|_| nullop()),
        vop_close: Some(|_| nullop()),
        vop_ioctl: Some(|_| nullop()),
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

    /// The device number of the fake disk (`vnd0c`-like).
    const DISKDEV: i32 = makedev(41, 2);

    /// Memory, the vfs and a fresh buffer cache, the image as the disk, and the thread as
    /// `curproc`.
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

    /// Clears `curproc`.
    pub(crate) fn teardown() {
        Machine::set_curproc(Machine::curcpu(), ptr::null());
    }

    /// The fake disk's block device vnode, referenced.
    pub(crate) fn diskvp() -> &'static Vnode {
        let vp = bdevvp(DISKDEV).unwrap().unwrap();
        vp.v_op.set(Some(&DISK_VOPS));
        vp
    }

    /// Zeroed mount arguments.
    fn noargs() -> MsdosfsArgs {
        MsdosfsArgs::from_bytes(&[0u8; MsdosfsArgs::SIZE]).unwrap()
    }

    /// A fresh msdos mount structure, `ronly` or read-write.
    fn newmount(ronly: bool) -> &'static Mount {
        let mp = vfs_mount_alloc(None, vfs_byname(MOUNT_MSDOS).unwrap());
        if ronly {
            mp.mnt_flag.set(mp.mnt_flag.get() | MNT_RDONLY);
        }
        mp
    }

    /// Mounts the disk the way `msdosfs_mount` does after its argument checks
    /// (`msdosfs_mountfs`), and puts it on the mount list.
    pub(crate) fn mount(p: &'static Proc, ronly: bool) -> &'static Mount {
        let mp = newmount(ronly);
        msdosfs_mountfs(diskvp(), mp, p, &noargs()).unwrap();
        vfs_unbusy(mp);
        // SAFETY: a new mount on no list.
        unsafe { MOUNTLIST.0.insert_tail(mp) };
        mp
    }

    /// Unmounts what `mount` mounted (`dounmount`: `msdosfs_sync`, `msdosfs_unmount`).
    pub(crate) fn unmount(p: &'static Proc, mp: &'static Mount) {
        vfs_busy(mp, VB_WRITE | VB_WAIT).unwrap();
        dounmount(mp, 0, p).unwrap();
    }

    /// A FAT image with a file in the root directory and an empty subdirectory.
    fn sample(params: mkfat::Params) -> (Vec<u8>, u32) {
        let mut img = mkfat::Image::new(params);
        let root = img.root();
        img.add_file(root, None, b"README  TXT", &[b'r'; 3000]);
        img.mkdir(root, b"SUBDIR     ", 1);
        let used = img.nclusters;
        (img.finish(), used)
    }

    /// An export list on a mounted FAT file system (`pm_export`): `msdosfs_check_export` answers
    /// the listed client and refuses the others.
    #[cfg(feature = "nfsserver")]
    #[test]
    fn an_exported_fat_file_system_answers_check_export() {
        use crate::kern::uipc_mbuf::tests::mbinit_again;
        use crate::kern::vfs_subr::tests::exports::{args, check_export, sin};
        use crate::sys::mount::{MNT_EXPORTED, MNT_EXRDONLY};

        let (disk, _nclusters) = sample(mkfat::FAT12_1M);
        let (_g, p) = setup(disk);
        mbinit_again();
        let mp = mount(p, false);
        let pmp = vfstomsdosfs(mp);
        let ro = MNT_EXPORTED | MNT_EXRDONLY;
        assert_eq!(check_export(mp, [10, 0, 0, 5]), Err(Errno::EACCES));

        let net = sin(2, [10, 0, 0, 0]);
        let mask = sin(2, [255, 255, 255, 0]);
        vfs_export(mp, &pmp.pm_export, &args(ro, 32767, Some(net), Some(mask))).unwrap();
        assert_eq!(check_export(mp, [10, 0, 0, 5]), Ok((ro, 32767)));
        assert_eq!(check_export(mp, [10, 0, 1, 5]), Err(Errno::EACCES));

        unmount(p, mp);
    }

    #[test]
    fn fat12_mountfs_works_out_the_geometry() {
        let (disk, nclusters) = sample(mkfat::FAT12_1M);
        let (_g, p) = setup(disk);

        let mp = mount(p, false);
        let pmp = vfstomsdosfs(mp);
        assert!(fat12(pmp));
        assert_eq!(pmp.pm_fatmask.get(), FAT12_MASK);
        assert_eq!((pmp.pm_fatmult.get(), pmp.pm_fatdiv.get()), (3, 2));
        assert_eq!(pmp.pm_BytesPerSec(), 512);
        assert_eq!(pmp.pm_BlkPerSec.get(), 1);
        assert_eq!(pmp.pm_bpcluster.get(), 512);
        assert_eq!(pmp.pm_crbomask.get(), 511);
        assert_eq!(pmp.pm_cnshift.get(), 9);
        assert_eq!(pmp.pm_bnshift.get(), 9);
        assert_eq!(pmp.pm_fatblk.get(), 1);
        // 1 reserved sector and two FATs of 6 sectors; 64 entries are 4 sectors.
        assert_eq!(pmp.pm_FATsecs.get(), 6);
        assert_eq!(pmp.pm_rootdirblk.get(), 13);
        assert_eq!(pmp.pm_rootdirsize.get(), 4);
        assert_eq!(pmp.pm_firstcluster.get(), 17);
        assert_eq!(pmp.pm_nmbrofclusters.get(), nclusters);
        assert_eq!(pmp.pm_nmbrofclusters.get(), 2031);
        assert_eq!(pmp.pm_maxcluster.get(), 2032);
        assert_eq!(pmp.pm_fatblocksize.get(), 3 * 512);
        assert_eq!(pmp.pm_fatsize.get(), 6 * 512);
        assert_eq!(pmp.pm_fsinfo.get(), 0);
        assert_ne!(pmp.pm_flags.get() & MSDOSFS_FATMIRROR, 0);
        assert_eq!(pmp.pm_flags.get() & MSDOSFSMNT_RONLY, 0);
        assert_eq!(pmp.pm_fmod.get(), 1);
        // README.TXT has 6 clusters, SUBDIR 1.
        assert_eq!(pmp.pm_freeclustercount.get(), 2031 - 7);
        assert_eq!(pmp.inusemap().len(), 2033usize.div_ceil(32));
        assert!(ptr::eq(pmp.mountp(), mp));
        assert_eq!(pmp.pm_dev.get(), DISKDEV);
        let st = mp.mnt_stat.get();
        assert_eq!(st.f_fsid.val, [DISKDEV, 4]);
        let devvp = pmp.devvp();
        assert!(
            devvp
                .v_specinfo()
                .and_then(|si| si.si_mountpoint.get())
                .is_some_and(|m| ptr::eq(m, mp))
        );

        let mut sb = mp.mnt_stat.get();
        msdosfs_statfs(mp, &mut sb, p).unwrap();
        assert_eq!(sb.f_bsize, 512);
        assert_eq!(sb.f_iosize, 512);
        assert_eq!(sb.f_blocks, 2031);
        assert_eq!(sb.f_bfree, 2024);
        assert_eq!(sb.f_bavail, 2024);
        assert_eq!(sb.f_files, 64);
        assert_eq!(sb.f_ffree, 0);

        // The root vnode needs the vnode operations of msdosfs_vnops.c (deget locks it), which
        // are not here yet; sync and unmount do not.
        msdosfs_sync(mp, MNT_WAIT, 0, p.p_ucred.get(), p).unwrap();
        unmount(p, mp);
        assert!(devvp.v_specinfo().unwrap().si_mountpoint.get().is_none());
        teardown();
    }

    #[test]
    fn fat16_mount_read_only() {
        let (disk, nclusters) = sample(mkfat::FAT16_4M);
        let (_g, p) = setup(disk);
        let mp = mount(p, true);
        let pmp = vfstomsdosfs(mp);
        assert!(fat16(pmp));
        assert_eq!((pmp.pm_fatmult.get(), pmp.pm_fatdiv.get()), (2, 1));
        assert_eq!(pmp.pm_fatblocksize.get(), MAXBSIZE as u32);
        assert_eq!(pmp.pm_rootdirsize.get(), 32);
        assert_eq!(pmp.pm_nmbrofclusters.get(), nclusters);
        assert_ne!(pmp.pm_flags.get() & MSDOSFSMNT_RONLY, 0);
        assert_eq!(pmp.pm_fmod.get(), 0);
        unmount(p, mp);
        teardown();
    }

    #[test]
    fn fat32_mount() {
        let (disk, nclusters) = sample(mkfat::FAT32_1M);
        let (_g, p) = setup(disk);
        let mp = mount(p, false);
        let pmp = vfstomsdosfs(mp);
        assert!(fat32(pmp));
        assert_eq!(pmp.pm_fatmask.get(), FAT32_MASK);
        assert_eq!(pmp.pm_rootdirblk.get(), 2);
        assert_eq!(pmp.pm_rootdirsize.get(), 0);
        assert_eq!(
            pmp.pm_firstcluster.get(),
            pmp.pm_fatblk.get() + 2 * pmp.pm_FATsecs.get()
        );
        assert_eq!(pmp.pm_fatblk.get(), 32);
        assert_eq!(pmp.pm_fsinfo.get(), 1, "a valid FSInfo block is kept");
        assert_eq!(pmp.pm_curfat.get(), 0);
        assert_ne!(pmp.pm_flags.get() & MSDOSFS_FATMIRROR, 0);
        assert_eq!(pmp.pm_nmbrofclusters.get(), nclusters);
        // The root directory, README.TXT and SUBDIR.
        assert_eq!(pmp.pm_freeclustercount.get(), nclusters - 8);
        unmount(p, mp);
        teardown();
    }

    #[test]
    fn fat32_fsinfo_and_active_fat() {
        let (mut disk, _) = sample(mkfat::FAT32_1M);
        // A bad FSInfo signature is ignored, and FAT 1 is the only active one.
        disk[512 + 484] = b'X';
        disk[40..42].copy_from_slice(&0x0081u16.to_le_bytes());
        let (_g, p) = setup(disk);
        let mp = mount(p, false);
        let pmp = vfstomsdosfs(mp);
        assert_eq!(pmp.pm_fsinfo.get(), 0);
        assert_eq!(pmp.pm_curfat.get(), 1);
        assert_eq!(pmp.pm_flags.get() & MSDOSFS_FATMIRROR, 0);
        unmount(p, mp);
        teardown();
    }

    #[test]
    fn mountfs_rejects_bad_boot_sectors() {
        let (good, _) = sample(mkfat::FAT12_1M);
        let mut cases: Vec<(&str, Vec<u8>)> = Vec::new();
        let mut d = good.clone();
        d[11..13].copy_from_slice(&0u16.to_le_bytes());
        cases.push(("no bytes per sector", d));
        let mut d = good.clone();
        d[13] = 0;
        cases.push(("no sectors per cluster", d));
        let mut d = good.clone();
        d[13] = 3;
        cases.push(("sectors per cluster not a power of 2", d));
        let mut d = good.clone();
        d[11..13].copy_from_slice(&768u16.to_le_bytes());
        cases.push(("sector size not a power of 2", d));
        let mut d = good.clone();
        d[11..13].copy_from_slice(&256u16.to_le_bytes());
        cases.push(("sector smaller than DEV_BSIZE", d));
        let mut d = good.clone();
        d[22..24].copy_from_slice(&0u16.to_le_bytes());
        cases.push(("no FAT sectors", d));
        let mut d = good.clone();
        d[13] = 0x80;
        d[11..13].copy_from_slice(&1024u16.to_le_bytes());
        cases.push(("clusters larger than MAXBSIZE", d));
        let mut d = good.clone();
        d[17..19].copy_from_slice(&0u16.to_le_bytes());
        cases.push(("FAT32 with 16-bit sector counts", d));

        let (g, p) = setup(good.clone());
        let devvp = diskvp();
        for (what, disk) in cases {
            *DISK.lock().unwrap() = disk;
            let mp = newmount(false);
            assert_eq!(
                msdosfs_mountfs(devvp, mp, p, &noargs()),
                Err(Errno::EINVAL),
                "{what}"
            );
            assert!(mp.mnt_data.get().is_null(), "{what}");
            assert!(devvp.v_specinfo().unwrap().si_mountpoint.get().is_none());
            vfs_unbusy(mp);
        }

        // The device is still usable.
        *DISK.lock().unwrap() = good;
        let mp = newmount(false);
        msdosfs_mountfs(devvp, mp, p, &noargs()).unwrap();
        assert_eq!(vfstomsdosfs(mp).pm_nmbrofclusters.get(), 2031);
        vfs_unbusy(mp);
        // SAFETY: a new mount on no list.
        unsafe { MOUNTLIST.0.insert_tail(mp) };
        unmount(p, mp);
        teardown();
        drop(g);
    }

    #[test]
    fn mkfat_long_names_match_the_kernel() {
        // The builder's long name entries are what unix2winfn writes.
        let name = b"m10c-fat.txt";
        let short = *b"M10C-FATTXT";
        let ents = mkfat::lfn_entries(name, &short);
        assert_eq!(ents.len(), 1);
        assert_eq!(
            mkfat::chksum(&short),
            crate::msdosfs::msdosfs_conv::winChksum(&short)
        );
        let mut we = [0u8; 32];
        let wep = crate::msdosfs::direntry::Winentry::at_mut(&mut we, 0);
        let more = crate::msdosfs::msdosfs_conv::unix2winfn(name, wep, 1, mkfat::chksum(&short));
        assert_eq!(more, 0);
        assert_eq!(we, ents[0]);
    }
}
/* </TESTS> */
