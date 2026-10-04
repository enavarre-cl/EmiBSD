/*	$OpenBSD: cd9660_vfsops.c,v 1.99 2025/09/20 13:53:36 mpi Exp $	*/
/*	$NetBSD: cd9660_vfsops.c,v 1.26 1997/06/13 15:38:58 pk Exp $	*/
/* <LICENSES> */
/*-
 * Copyright (c) 1994
 *	The Regents of the University of California.  All rights reserved.
 *
 * This code is derived from software contributed to Berkeley
 * by Pace Willisson (pace@blitz.com).  The Rock Ridge Extension
 * Support code is derived from software contributed to Berkeley
 * by Atsushi Murai (amurai@spec.co.jp).
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
 *	@(#)cd9660_vfsops.c	8.9 (Berkeley) 12/5/94
 */
/* </LICENSES> */

//! The ISO 9660 file system's file-system-type operations: mounting (`cd9660_mountroot`,
//! `cd9660_mount`, `iso_mountfs` with the volume descriptor search, the Rock Ridge and
//! Joliet detection), the disk label a CD gets (`iso_disklabelspoof`), unmounting,
//! `statfs`, the node cache's `cd9660_vget_internal`, and file handles.
//!
//! Upstream: sys/isofs/cd9660/cd9660_vfsops.c @ 3ce1f3f79392
//!
//! ## Deviations
//! - `cd9660_mount` looks the device name up in a `Nameidata` of its own (`ndinit` around
//!   the kernel copy of the name) instead of reinitialising the caller's `ndp`, as
//!   `ffs_mount` does. A mount without arguments (the C's NULL `data`, which it would
//!   dereference) is `EINVAL`. The mount arguments copied into `mnt_stat.mount_info` are the
//!   caller's bytes with the `flags` that `iso_mountfs` changed (the C changes the kernel
//!   copy in place).
//! - `im_export` is not kept (`cd9660_extern.rs`): the export update passes a NULL table to
//!   `vfs_export`, which answers `ENOTSUP` without `NFSSERVER`, and `cd9660_check_export`
//!   finds no `struct netcred`, as `ufs_check_export` does.
//! - `iso_mountfs` builds `struct iso_mnt` in a local and allocates it once it is complete
//!   (the C allocates it before the Rock Ridge check and frees it on error); the error path
//!   also releases the primary descriptor's buffer, which the C leaks when the logical block
//!   size is bad.
//! - `cd9660_mountroot`: a `rootdev` without a device (`bdevvp` returning no vnode, which
//!   the C would pass on as NULL) is `ENODEV`, as in `ffs_mountroot`.
//! - `option FIFO` is in GENERIC but `miscfs/fifofs` is not ported: `cd9660_fifovops` comes
//!   with it, and `cd9660_vget_internal` takes the C's `#else` branch for a fifo meanwhile
//!   (`EOPNOTSUPP`), as `ffs_vinit` does.
//! - `struct ifid` is read from and written to the `struct fid` it overlays by
//!   [`Ifid::from_fid`]/[`Ifid::to_fid`].
//! - `CDIOREADMSADDR`'s `int` travels as its four bytes through `VOP_IOCTL`.
//! - `ISOFS_DBG` is not defined.

use core::ffi::c_void;
use core::ptr::{self, NonNull};
use core::sync::atomic::Ordering;

use crate::isofs::cd9660::cd9660_extern::{
    ISO_FTYPE_9660, ISO_FTYPE_DEFAULT, ISO_FTYPE_RRIP, ISOFSMNT_ROOT, IsoMnt, blkoff, lblkno,
    vfstoisofs,
};
use crate::isofs::cd9660::cd9660_lookup::cd9660_bufatoff;
use crate::isofs::cd9660::cd9660_node::{
    Doff, IsoNode, cd9660_defattr, cd9660_deftstamp, cd9660_ihashget, cd9660_ihashins, cd9660_init,
    isodirino,
};
use crate::isofs::cd9660::cd9660_rrip::{cd9660_rrip_analyze, cd9660_rrip_offset};
use crate::isofs::cd9660::cd9660_vnops::{CD9660_SPECVOPS, CD9660_VOPS};
use crate::isofs::cd9660::iso::{
    Cdino, ISO_DEFAULT_BLOCK_SIZE, ISO_DIRECTORY_RECORD_SIZE, ISO_STANDARD_ID, ISO_VD_END,
    ISO_VD_PRIMARY, ISO_VD_SUPPLEMENTARY, IsoDirectoryRecord, IsoPrimaryDescriptor,
    IsoSupplementaryDescriptor, IsoVolumeDescriptor, isonum_711, isonum_723, isonum_733,
};
use crate::kern::init_main::{rootvp, set_rootvp, set_swapdev_vp};
use crate::kern::kern_malloc::{free, malloc};
use crate::kern::kern_rwlock::rrw_init_flags;
use crate::kern::kern_time::inittodr;
use crate::kern::spec_vnops::SPEC_VOPS;
use crate::kern::subr_disk::dkcksum;
use crate::kern::subr_prf::panic;
use crate::kern::subr_xxx::eopnotsupp;
use crate::kern::vfs_bio::{biowait, bread, brelse, geteblk};
use crate::kern::vfs_lookup::{namei, ndinit};
use crate::kern::vfs_subr::{
    MOUNTLIST, bdevvp, checkalias, copy_statfs_info, getnewvnode, vcount, vflush, vfs_export,
    vfs_export_lookup, vfs_mount_free, vfs_mountedon, vfs_rootmountalloc, vfs_unbusy, vgone,
    vinvalbuf, vput, vref, vrele,
};
use crate::kern::vfs_vnops::vn_lock;
use crate::kern::vfs_vops::{VOP_CLOSE, VOP_IOCTL, VOP_OPEN, VOP_UNLOCK};
use crate::kprintf;
use crate::machine::conf::{nblkdev, swapdev};
use crate::machine::copy::copyinstr;
use crate::machine::cpu::curproc;
use crate::sys::buf::{B_BUSY, B_DONE, B_INVAL, B_RAW, B_READ, B_WRITE, Buf};
use crate::sys::cdio::CDIOREADMSADDR;
use crate::sys::conf::DevTypeStrategy;
use crate::sys::disklabel::{
    DISKMAGIC, Disklabel, FS_ISO9660, MAXPARTITIONS, RAW_PART, dl_getdsize, dl_setpoffset,
    dl_setpsize,
};
use crate::sys::errno::Errno;
use crate::sys::fcntl::{FREAD, FWRITE};
use crate::sys::lock::{LK_EXCLUSIVE, LK_RETRY};
use crate::sys::malloc::{M_ISOFSMNT, M_ISOFSNODE, M_WAITOK, M_ZERO};
use crate::sys::mbuf::Mbuf;
use crate::sys::mount::{
    Fid, ISOFSMNT_EXTATT, ISOFSMNT_GENS, ISOFSMNT_NOJOLIET, ISOFSMNT_NORRIP, ISOFSMNT_SESS,
    IsoArgs, MNAMELEN, MNT_FORCE, MNT_LOCAL, MNT_RDONLY, MNT_UPDATE, Mount, Statfs, VFS_VGET,
    Vfsops,
};
use crate::sys::namei::{FOLLOW, LOOKUP, Nameidata, NiDirp};
use crate::sys::param::{DEV_BSHIFT, DEV_BSIZE, MAXBSIZE, btodb};
use crate::sys::proc::Proc;
use crate::sys::rwlock::{RWL_DUPOK, RWL_IS_VNODE};
use crate::sys::syslimits::NAME_MAX;
use crate::sys::systm::{INFSLP, ROOTDEV};
use crate::sys::types::{Dev, Ino, Uid, major};
use crate::sys::ucred::{FSCRED, NOCRED, Ucred};
use crate::sys::vnode::{
    FORCECLOSE, V_SAVE, VBAD, VBLK, VCHR, VDIR, VFIFO, VLNK, VNON, VREG, VROOT, VSOCK, VT_ISOFS,
    Vnode, iftovt,
};
use crate::uvm::uvm_vnode::uvm_vnp_setsize;

/// `struct ifid`: an ISO 9660 file identifier, overlaid on `struct fid`.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Ifid {
    /// `ifid_len`.
    pub ifid_len: u16,
    /// `ifid_pad`.
    pub ifid_pad: u16,
    /// `ifid_ino`.
    pub ifid_ino: i32,
    /// `ifid_start`.
    pub ifid_start: i64,
}

impl Ifid {
    /// `(struct ifid *)fhp`: the file identifier read as an ISO 9660 one (`ifid_ino` at
    /// offset 4, `ifid_start` aligned at offset 8).
    pub fn from_fid(fid: &Fid) -> Self {
        let d = &fid.fid_data;
        let mut start = [0u8; 8];
        start.copy_from_slice(&d[4..12]);
        Self {
            ifid_len: fid.fid_len,
            ifid_pad: fid.fid_reserved,
            ifid_ino: i32::from_ne_bytes([d[0], d[1], d[2], d[3]]),
            ifid_start: i64::from_ne_bytes(start),
        }
    }

    /// Stores the identifier into the `struct fid` it overlays.
    pub fn to_fid(&self, fid: &mut Fid) {
        fid.fid_len = self.ifid_len;
        fid.fid_reserved = self.ifid_pad;
        fid.fid_data[0..4].copy_from_slice(&self.ifid_ino.to_ne_bytes());
        fid.fid_data[4..12].copy_from_slice(&self.ifid_start.to_ne_bytes());
    }
}

/// `sizeof(struct ifid)`.
const IFID_SIZE: u16 = 16;

/// `cd9660_vfsops`.
pub static CD9660_VFSOPS: Vfsops = Vfsops {
    vfs_mount: cd9660_mount,
    vfs_start: cd9660_start,
    vfs_unmount: cd9660_unmount,
    vfs_root: cd9660_root,
    vfs_quotactl: cd9660_quotactl,
    vfs_statfs: cd9660_statfs,
    vfs_sync: cd9660_sync,
    vfs_vget: cd9660_vget,
    vfs_fhtovp: cd9660_fhtovp,
    vfs_vptofh: cd9660_vptofh,
    vfs_init: Some(cd9660_init),
    vfs_sysctl: Some(|_, _, _, _, _, _| eopnotsupp()),
    vfs_checkexp: cd9660_check_export,
};

/// `memset(dst, 0, MNAMELEN); strlcpy(dst, src, MNAMELEN)`.
fn mname_copy(dst: &mut [u8; MNAMELEN], src: &[u8]) {
    *dst = [0; MNAMELEN];
    let src = src.split(|&c| c == 0).next().unwrap_or(&[]);
    let n = src.len().min(MNAMELEN - 1);
    dst[..n].copy_from_slice(&src[..n]);
}

/// `cd9660_mountroot`: called by `vfs_mountroot` (`dk_mountroot`) when iso is going to be
/// mounted as root.
pub fn cd9660_mountroot() -> Result<(), Errno> {
    let Some(p) = curproc() else {
        panic(format_args!("cd9660_mountroot: no curproc"));
    };

    // Get vnodes for swapdev and rootdev.
    let rvp = match bdevvp(swapdev()).and_then(|svp| {
        set_swapdev_vp(svp);
        bdevvp(ROOTDEV.load(Ordering::Relaxed))
    }) {
        Ok(Some(rvp)) => rvp,
        Ok(None) => {
            kprintf!("cd9660_mountroot: can't setup bdevvp's");
            return Err(Errno::ENODEV);
        }
        Err(e) => {
            kprintf!("cd9660_mountroot: can't setup bdevvp's");
            return Err(e);
        }
    };
    set_rootvp(Some(rvp));

    let mp = vfs_rootmountalloc(b"cd9660", b"root_device")?;
    let Some(mut args) = IsoArgs::from_bytes(&[0u8; IsoArgs::SIZE]) else {
        panic(format_args!("cd9660_mountroot: no iso_args"));
    };
    args.flags = ISOFSMNT_ROOT;
    if let Err(e) = iso_mountfs(rvp, mp, p, &mut args) {
        vfs_unbusy(mp);
        vfs_mount_free(mp);
        return Err(e);
    }

    // SAFETY: a new mount on no list, under the kernel lock.
    unsafe { MOUNTLIST.0.insert_tail(mp) };
    let mut st = mp.mnt_stat.get();
    let _ = cd9660_statfs(mp, &mut st, p);
    mp.mnt_stat.set(st);
    vfs_unbusy(mp);
    inittodr(0);

    Ok(())
}

/// `cd9660_mount` (`vfs_mount`): mount system call. `data` is the kernel copy of the
/// user's `struct iso_args` (empty for the C's NULL).
pub fn cd9660_mount(
    mp: &'static Mount,
    path: &[u8],
    data: &mut [u8],
    _ndp: &mut Nameidata<'_>,
    p: &Proc,
) -> Result<(), Errno> {
    let args = IsoArgs::from_bytes(data);

    if mp.mnt_flag.get() & MNT_RDONLY == 0 {
        return Err(Errno::EROFS);
    }

    // If updating, check whether changing from read-only to read/write; if there is no
    // device name, that's all we do.
    if mp.mnt_flag.get() & MNT_UPDATE != 0 {
        let _imp = vfstoisofs(mp);
        if let Some(a) = args
            && a.fspec == 0
        {
            return vfs_export(mp, ptr::null_mut(), ptr::from_ref(&a.export_info).cast());
        }
        return Ok(());
    }

    // Not an update, or updating the name: look up the name and verify that it refers to a
    // sensible block device.
    let Some(mut args) = args else {
        return Err(Errno::EINVAL);
    };
    let mut fspec = [0u8; MNAMELEN];
    copyinstr(args.fspec, &mut fspec)?;
    let flen = fspec.iter().position(|&c| c == 0).unwrap_or(MNAMELEN);
    let mut nd = ndinit(LOOKUP, FOLLOW, NiDirp::Sys(&fspec[..flen]), p);
    namei(&mut nd)?;
    let Some(devvp) = nd.ni_vp else {
        return Err(Errno::ENOENT);
    };

    if devvp.v_type.get() != VBLK {
        vrele(devvp);
        return Err(Errno::ENOTBLK);
    }
    if major(devvp.v_rdev()) >= nblkdev() {
        vrele(devvp);
        return Err(Errno::ENXIO);
    }

    let error = if mp.mnt_flag.get() & MNT_UPDATE == 0 {
        iso_mountfs(devvp, mp, p, &mut args)
    } else if !ptr::eq(devvp, vfstoisofs(mp).im_devvp) {
        Err(Errno::EINVAL) // needs translation
    } else {
        vrele(devvp);
        Ok(())
    };
    if let Err(e) = error {
        vrele(devvp);
        return Err(e);
    }

    let mut info = [0u8; IsoArgs::SIZE];
    info.copy_from_slice(&data[..IsoArgs::SIZE]);
    let flags = core::mem::offset_of!(IsoArgs, flags);
    info[flags..flags + 4].copy_from_slice(&args.flags.to_ne_bytes());
    mp.update_stat(|sp| {
        mname_copy(&mut sp.f_mntonname, path);
        mname_copy(&mut sp.f_mntfromname, &fspec);
        mname_copy(&mut sp.f_mntfromspec, &fspec);
        sp.mount_info.__align[..IsoArgs::SIZE].copy_from_slice(&info);
    });

    let mut st = mp.mnt_stat.get();
    let _ = cd9660_statfs(mp, &mut st, p);
    mp.mnt_stat.set(st);

    Ok(())
}

/// `iso_mountfs`: common code for mount and mountroot.
fn iso_mountfs(
    devvp: &'static Vnode,
    mp: &'static Mount,
    p: &Proc,
    argp: &mut IsoArgs,
) -> Result<(), Errno> {
    let dev = devvp.v_rdev();
    let ronly = mp.mnt_flag.get() & MNT_RDONLY != 0;

    if !ronly {
        return Err(Errno::EROFS);
    }

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

    let omode = if ronly { FREAD } else { FREAD | FWRITE };
    VOP_OPEN(devvp, omode, FSCRED, p)?;

    // This is the "logical sector size". The standard says this should be 2048 or the
    // physical sector size on the device, whichever is greater. For now, we'll just use a
    // constant.
    let iso_bsize = ISO_DEFAULT_BLOCK_SIZE;

    let sess = if argp.flags & ISOFSMNT_SESS != 0 {
        argp.sess.max(0)
    } else {
        let mut b = 0i32.to_ne_bytes();
        match VOP_IOCTL(devvp, CDIOREADMSADDR, &mut b, 0, FSCRED, p) {
            Ok(()) => i32::from_ne_bytes(b),
            Err(_) => 0,
        }
    };

    let mut bp: Option<&'static Buf> = None;
    let mut pribp: Option<&'static Buf> = None;
    let mut supbp: Option<&'static Buf> = None;

    let error: Errno = 'out: {
        let mut joliet_level = 0;
        for iso_blknum in 16..100 {
            let (b, error) = bread(
                devvp,
                i64::from(iso_blknum + sess) * btodb(iso_bsize) as i64,
                iso_bsize as i32,
            );
            bp = Some(b);
            if let Err(e) = error {
                break 'out e;
            }

            // SAFETY: the buffer is ours (busy from `bread`) and mapped; the views die
            // before it is released.
            let data: &[u8] = unsafe { b.data() };
            let Some(vdp) = IsoVolumeDescriptor::from_bytes(data) else {
                break 'out Errno::EINVAL;
            };
            if vdp.id != *ISO_STANDARD_ID {
                break 'out Errno::EINVAL;
            }

            match isonum_711(&vdp.type_) {
                ISO_VD_PRIMARY => {
                    if pribp.is_none() {
                        pribp = bp.take();
                    }
                }
                ISO_VD_SUPPLEMENTARY => {
                    if supbp.is_none() {
                        supbp = bp.take();
                        if argp.flags & ISOFSMNT_NOJOLIET == 0
                            && let Some(sup) = IsoSupplementaryDescriptor::from_bytes(data)
                        {
                            match &sup.escape[..3] {
                                b"%/@" => joliet_level = 1,
                                b"%/C" => joliet_level = 2,
                                b"%/E" => joliet_level = 3,
                                _ => {}
                            }

                            if isonum_711(&sup.flags) & 1 != 0 {
                                joliet_level = 0;
                            }
                        }
                    }
                }
                ISO_VD_END => break, // vd_end
                _ => {}
            }
            if let Some(b) = bp.take() {
                brelse(b);
            }
        }
        // vd_end:
        if let Some(b) = bp.take() {
            brelse(b);
        }

        let Some(pb) = pribp else {
            break 'out Errno::EINVAL;
        };
        // SAFETY: as above; `pribp` is ours until released below.
        let Some(pri) = IsoPrimaryDescriptor::from_bytes(unsafe { pb.data() }) else {
            break 'out Errno::EINVAL;
        };

        let logical_block_size = i32::from(isonum_723(&pri.logical_block_size));

        if logical_block_size < DEV_BSIZE as i32
            || logical_block_size > MAXBSIZE as i32
            || logical_block_size & (logical_block_size - 1) != 0
        {
            break 'out Errno::EINVAL;
        }

        let Some(rootp) = IsoDirectoryRecord::new(&pri.root_directory_record) else {
            break 'out Errno::EINVAL;
        };

        let mut isomp = IsoMnt {
            im_flags: 0,
            im_mountp: mp,
            im_dev: dev,
            im_devvp: devvp,
            logical_block_size,
            im_bshift: logical_block_size.trailing_zeros() as i32,
            im_bmask: logical_block_size - 1,
            // Since an ISO9660 multi-session CD can also access previous sessions, we have
            // to include them into the space considerations.
            volume_space_size: (isonum_733(&pri.volume_space_size) as i32).wrapping_add(sess),
            root: pri.root_directory_record,
            root_extent: isonum_733(rootp.extent()),
            root_size: isonum_733(rootp.size()),
            iso_ftype: ISO_FTYPE_DEFAULT,
            rr_skip: 0,
            rr_skip0: 0,
            joliet_level: 0,
        };
        let root_eal = u32::from(isonum_711(rootp.ext_attr_length()));

        brelse(pb);
        pribp = None;

        let typenum = mp.vfc().vfc_typenum;
        mp.update_stat(|sp| {
            sp.f_fsid.val[0] = dev;
            sp.f_fsid.val[1] = typenum;
            sp.f_namemax = NAME_MAX as u32;
        });
        mp.mnt_flag.set(mp.mnt_flag.get() | MNT_LOCAL);

        // Check the Rock Ridge Extension support
        if argp.flags & ISOFSMNT_NORRIP == 0 {
            let (b, error) = bread(
                isomp.im_devvp,
                i64::from(isomp.root_extent.wrapping_add(root_eal))
                    << (isomp.im_bshift - DEV_BSHIFT as i32),
                isomp.logical_block_size,
            );
            bp = Some(b);
            if let Err(e) = error {
                break 'out e;
            }

            // SAFETY: as above.
            let data: &[u8] = unsafe { b.data() };
            let Some(rootp) = IsoDirectoryRecord::new(data) else {
                break 'out Errno::EINVAL;
            };

            let rr_skip = cd9660_rrip_offset(&rootp, &mut isomp);
            isomp.rr_skip = rr_skip;
            if rr_skip < 0 {
                argp.flags |= ISOFSMNT_NORRIP;
            } else {
                argp.flags &= !ISOFSMNT_GENS;
            }

            // The contents are valid, but they will get reread as part of another vnode,
            // so...
            if let Some(b) = bp.take() {
                brelse(b);
            }
        }
        isomp.im_flags =
            argp.flags & (ISOFSMNT_NORRIP | ISOFSMNT_GENS | ISOFSMNT_EXTATT | ISOFSMNT_NOJOLIET);
        isomp.iso_ftype = match isomp.im_flags & (ISOFSMNT_NORRIP | ISOFSMNT_GENS) {
            f if f == ISOFSMNT_GENS | ISOFSMNT_NORRIP => ISO_FTYPE_9660,
            0 => ISO_FTYPE_RRIP,
            _ => ISO_FTYPE_DEFAULT,
        };

        // Decide whether to use the Joliet descriptor
        if isomp.iso_ftype != ISO_FTYPE_RRIP
            && joliet_level != 0
            && let Some(sb) = supbp
        {
            // SAFETY: as above; `supbp` is ours until released below.
            if let Some(sup) = IsoSupplementaryDescriptor::from_bytes(unsafe { sb.data() })
                && let Some(rootp) = IsoDirectoryRecord::new(&sup.root_directory_record)
            {
                isomp.root = sup.root_directory_record;
                isomp.root_extent = isonum_733(rootp.extent());
                isomp.root_size = isonum_733(rootp.size());
                isomp.joliet_level = joliet_level;
            }
        }

        if let Some(b) = supbp.take() {
            brelse(b);
        }

        let Some(mem) = malloc(size_of::<IsoMnt>(), M_ISOFSMNT, M_WAITOK) else {
            panic(format_args!("iso_mountfs: no memory"));
        };
        let imp = mem.cast::<IsoMnt>();
        // SAFETY: a fresh allocation of `size_of::<IsoMnt>()` bytes, aligned for any kernel
        // structure (`malloc(9)`); `cd9660_unmount` frees it.
        unsafe { ptr::write(imp.as_ptr(), isomp) };
        mp.mnt_data.set(imp.as_ptr().cast::<c_void>());

        if let Some(si) = devvp.v_specinfo() {
            si.si_mountpoint.set(Some(mp));
        }

        return Ok(());
    };

    // out:
    if let Some(si) = devvp.v_specinfo() {
        si.si_mountpoint.set(None);
    }
    for b in [bp, supbp, pribp].into_iter().flatten() {
        brelse(b);
    }

    let _ = vn_lock(devvp, LK_EXCLUSIVE | LK_RETRY);
    let _ = VOP_CLOSE(devvp, omode, NOCRED, Some(p));
    let _ = VOP_UNLOCK(devvp);

    Err(error)
}

/// `strncpy(dst, src, sizeof(dst))`: copy up to the first NUL, zero-fill the rest.
fn strncpy(dst: &mut [u8], src: &[u8]) {
    let n = src
        .iter()
        .take(dst.len())
        .position(|&c| c == 0)
        .unwrap_or(dst.len().min(src.len()));
    dst[..n].copy_from_slice(&src[..n]);
    dst[n..].fill(0);
}

/// `iso_disklabelspoof`: test to see if the device is an ISOFS filesystem, and if so build
/// a disk label for it (the whole disc in partitions `a` and `c`).
pub fn iso_disklabelspoof(
    dev: Dev,
    strat: DevTypeStrategy,
    lp: &mut Disklabel,
) -> Result<(), Errno> {
    let bp = geteblk(ISO_DEFAULT_BLOCK_SIZE);
    bp.b_dev.set(dev);

    let error: Result<(), Errno> = 'out: {
        let mut vdtype = 0;
        for iso_blknum in 16..100 {
            bp.b_blkno
                .set((iso_blknum * btodb(ISO_DEFAULT_BLOCK_SIZE)) as i64);
            bp.b_bcount.set(ISO_DEFAULT_BLOCK_SIZE as i64);
            bp.clr(B_READ | B_WRITE | B_DONE);
            bp.set(B_BUSY | B_READ | B_RAW);

            strat(bp);

            if biowait(bp).is_err() {
                break 'out Err(Errno::EINVAL);
            }

            // SAFETY: the buffer is ours (busy since `geteblk`) and mapped.
            let data: &[u8] = unsafe { bp.data() };
            let Some(vdp) = IsoVolumeDescriptor::from_bytes(data) else {
                break 'out Err(Errno::EINVAL);
            };
            vdtype = isonum_711(&vdp.type_);
            if vdp.id != *ISO_STANDARD_ID || vdtype == ISO_VD_END {
                break 'out Err(Errno::EINVAL);
            }

            if vdtype == ISO_VD_PRIMARY {
                break;
            }
        }

        if vdtype != ISO_VD_PRIMARY {
            break 'out Err(Errno::EINVAL);
        }

        // SAFETY: as above.
        let Some(pri) = IsoPrimaryDescriptor::from_bytes(unsafe { bp.data() }) else {
            break 'out Err(Errno::EINVAL);
        };
        let logical_block_size = usize::from(isonum_723(&pri.logical_block_size));
        if !(DEV_BSIZE..=MAXBSIZE).contains(&logical_block_size)
            || logical_block_size & (logical_block_size - 1) != 0
        {
            break 'out Err(Errno::EINVAL);
        }

        // build a disklabel for the CD
        strncpy(&mut lp.d_typename, &pri.volume_id);
        strncpy(&mut lp.d_packname, &pri.volume_id[16..]);
        for i in 0..MAXPARTITIONS {
            dl_setpsize(&mut lp.d_partitions[i], 0);
            dl_setpoffset(&mut lp.d_partitions[i], 0);
        }
        let dsize = dl_getdsize(lp);
        dl_setpoffset(&mut lp.d_partitions[0], 0);
        dl_setpsize(&mut lp.d_partitions[0], dsize);
        lp.d_partitions[0].p_fstype = FS_ISO9660;
        dl_setpoffset(&mut lp.d_partitions[RAW_PART as usize], 0);
        dl_setpsize(&mut lp.d_partitions[RAW_PART as usize], dsize);
        lp.d_partitions[RAW_PART as usize].p_fstype = FS_ISO9660;
        lp.d_npartitions = MAXPARTITIONS as u16;
        lp.d_version = 1;

        lp.d_magic = DISKMAGIC;
        lp.d_magic2 = DISKMAGIC;
        lp.d_checksum = dkcksum(lp);
        Ok(())
    };

    bp.set(B_INVAL);
    brelse(bp);
    error
}

/// `cd9660_start` (`vfs_start`): make a filesystem operational. Nothing to do at the
/// moment.
pub fn cd9660_start(_mp: &'static Mount, _flags: i32, _p: &Proc) -> Result<(), Errno> {
    Ok(())
}

/// `cd9660_unmount` (`vfs_unmount`): unmount system call.
pub fn cd9660_unmount(mp: &'static Mount, mntflags: i32, p: &Proc) -> Result<(), Errno> {
    let mut flags = 0;
    if mntflags & MNT_FORCE != 0 {
        flags |= FORCECLOSE;
    }
    // #if 0: mntflushbuf(mp, 0); if (mntinvalbuf(mp)) return (EBUSY);
    vflush(mp, None, flags)?;

    let isomp = vfstoisofs(mp);
    let devvp = isomp.im_devvp;

    if let Some(si) = devvp.v_specinfo() {
        si.si_mountpoint.set(None);
    }
    let _ = vn_lock(devvp, LK_EXCLUSIVE | LK_RETRY);
    let _ = VOP_CLOSE(devvp, FREAD, NOCRED, Some(p));
    vput(devvp);
    mp.mnt_data.set(ptr::null_mut());
    free(NonNull::from(isomp).cast(), M_ISOFSMNT, size_of::<IsoMnt>());
    mp.mnt_flag.set(mp.mnt_flag.get() & !MNT_LOCAL);
    Ok(())
}

/// `cd9660_root` (`vfs_root`): return root of a filesystem.
pub fn cd9660_root(mp: &'static Mount) -> Result<&'static Vnode, Errno> {
    let imp = vfstoisofs(mp);
    let Some(dp) = IsoDirectoryRecord::new(&imp.root) else {
        panic(format_args!("cd9660_root: no root record"));
    };
    let ino = isodirino(&dp, imp);

    // With RRIP we must use the `.' entry of the root directory. Simply tell vget, that
    // it's a relocated directory.
    cd9660_vget_internal(mp, ino, imp.iso_ftype == ISO_FTYPE_RRIP, Some(dp))
}

/// `cd9660_quotactl` (`vfs_quotactl`): do operations associated with quotas, not
/// supported.
pub fn cd9660_quotactl(
    _mp: &'static Mount,
    _cmd: i32,
    _uid: Uid,
    _arg: usize,
    _p: &Proc,
) -> Result<(), Errno> {
    Err(Errno::EOPNOTSUPP)
}

/// `cd9660_statfs` (`vfs_statfs`): get file system statistics.
pub fn cd9660_statfs(mp: &'static Mount, sbp: &mut Statfs, _p: &Proc) -> Result<(), Errno> {
    let isomp = vfstoisofs(mp);

    sbp.f_bsize = isomp.logical_block_size as u32;
    sbp.f_iosize = sbp.f_bsize; // XXX
    sbp.f_blocks = i64::from(isomp.volume_space_size) as u64;
    sbp.f_bfree = 0; // total free blocks
    sbp.f_bavail = 0; // blocks free for non superuser
    sbp.f_files = 0; // total files
    sbp.f_ffree = 0; // free file nodes
    sbp.f_favail = 0; // file nodes free for non superuser
    copy_statfs_info(sbp, mp);

    Ok(())
}

/// `cd9660_sync` (`vfs_sync`): nothing to write back.
pub fn cd9660_sync(
    _mp: &'static Mount,
    _waitfor: i32,
    _stall: i32,
    _cred: *const Ucred,
    _p: &Proc,
) -> Result<(), Errno> {
    Ok(())
}

/// `cd9660_fhtovp` (`vfs_fhtovp`): file handle to vnode.
///
/// Have to be really careful about stale file handles:
/// - check that the inode number is in range
/// - call iget() to get the locked inode
/// - check for an unallocated inode (i_mode == 0)
/// - check that the generation number matches
pub fn cd9660_fhtovp(mp: &'static Mount, fhp: &Fid) -> Result<&'static Vnode, Errno> {
    let ifhp = Ifid::from_fid(fhp);

    let nvp = VFS_VGET(mp, i64::from(ifhp.ifid_ino) as Ino)?;
    let ip = crate::isofs::cd9660::cd9660_node::vtoi(nvp);
    if ip.inode.get().iso_mode == 0 {
        vput(nvp);
        return Err(Errno::ESTALE);
    }
    Ok(nvp)
}

/// `cd9660_vget` (`vfs_vget`).
pub fn cd9660_vget(mp: &'static Mount, ino: Ino) -> Result<&'static Vnode, Errno> {
    if ino > Ino::from(Cdino::MAX) {
        panic(format_args!("cd9660_vget: alien ino_t {}", ino));
    }

    // XXXX
    // It would be nice if we didn't always set the `relocated' flag and force the extra
    // read, but I don't want to think about fixing that right now.
    // (#if 0: VFSTOISOFS(mp)->iso_ftype == ISO_FTYPE_RRIP.)
    cd9660_vget_internal(mp, ino as Cdino, false, None)
}

/// `cd9660_vget_internal`: the vnode of the node `ino`, referenced and locked, from the
/// node cache or read in from its directory record (`isodir`, or read here). `relocated`
/// directories take their attributes from their own `.` entry.
pub fn cd9660_vget_internal(
    mp: &'static Mount,
    ino: Cdino,
    relocated: bool,
    isodir: Option<IsoDirectoryRecord<'_>>,
) -> Result<&'static Vnode, Errno> {
    loop {
        // retry:
        let imp = vfstoisofs(mp);
        let dev = imp.im_dev;
        if let Some(vp) = cd9660_ihashget(dev, ino) {
            return Ok(vp);
        }

        // Allocate a new vnode/iso_node.
        let vp = getnewvnode(VT_ISOFS, Some(mp), &CD9660_VOPS)?;
        let Some(mem) = malloc(size_of::<IsoNode>(), M_ISOFSNODE, M_WAITOK | M_ZERO) else {
            panic(format_args!("cd9660_vget_internal: no memory"));
        };
        let ipp = mem.cast::<IsoNode>();
        // SAFETY: a fresh allocation of `size_of::<IsoNode>()` bytes, aligned for any kernel
        // structure (`malloc(9)`); `cd9660_reclaim` frees it.
        unsafe { ptr::write(ipp.as_ptr(), IsoNode::new()) };
        // SAFETY: just initialised; freed only by `cd9660_reclaim`, once the vnode is
        // done with.
        let ip: &'static IsoNode = unsafe { &*ipp.as_ptr() };
        rrw_init_flags(&ip.i_lock, "isoinode", RWL_DUPOK | RWL_IS_VNODE);
        vp.v_data.set(ipp.as_ptr().cast());
        ip.i_vnode.set(Some(vp));
        ip.i_dev.set(dev);
        ip.i_number.set(ino);

        // Put it onto its hash chain and lock it so that other requests for this inode will
        // block if they arrive while we are sleeping waiting for old data structures to be
        // purged or for the contents of the disk portion of this inode to be read.
        if let Err(error) = cd9660_ihashins(ip) {
            vrele(vp);

            if error == Errno::EEXIST {
                continue;
            }

            return Err(error);
        }

        let lbs = imp.logical_block_size;
        let mut bp: Option<&'static Buf> = None;
        let mut isodir = isodir;
        if isodir.is_none() {
            let lbn = lblkno(imp, i64::from(ino));
            if lbn >= i64::from(imp.volume_space_size) {
                vput(vp);
                kprintf!("fhtovp: lbn exceed volume space {}\n", lbn);
                return Err(Errno::ESTALE);
            }

            let off = blkoff(imp, i64::from(ino)) as usize;
            if off + ISO_DIRECTORY_RECORD_SIZE > lbs as usize {
                vput(vp);
                kprintf!(
                    "fhtovp: crosses block boundary {}\n",
                    off + ISO_DIRECTORY_RECORD_SIZE
                );
                return Err(Errno::ESTALE);
            }

            let (b, error) = bread(
                imp.im_devvp,
                lbn << (imp.im_bshift - DEV_BSHIFT as i32),
                lbs,
            );
            if let Err(e) = error {
                vput(vp);
                brelse(b);
                kprintf!("fhtovp: bread error {}\n", e as i32);
                return Err(e);
            }
            bp = Some(b);
            // SAFETY: the buffer is ours (busy from `bread`) and mapped; the record view
            // dies before it is released.
            let data: &'static [u8] = unsafe { b.data() };
            let Some(rec) = data.get(off..).and_then(IsoDirectoryRecord::new) else {
                vput(vp);
                brelse(b);
                return Err(Errno::ESTALE);
            };

            let reclen = usize::from(isonum_711(rec.length()));
            if off + reclen > lbs as usize {
                vput(vp);
                brelse(b);
                kprintf!(
                    "fhtovp: directory crosses block boundary {}[off={}/len={}]\n",
                    off + reclen,
                    off,
                    reclen
                );
                return Err(Errno::ESTALE);
            }

            // #if 0: compare the record's start with the file handle's ifid_start.
            isodir = Some(rec);
        }

        ip.i_mnt.set(Some(imp));
        ip.i_devvp.set(Some(imp.im_devvp));
        vref(imp.im_devvp);

        if relocated {
            // On relocated directories we must read the `.' entry out of a dir.
            ip.iso_start.set(Doff::from(ino >> imp.im_bshift));
            if let Some(b) = bp.take() {
                brelse(b);
            }
            let b = match cd9660_bufatoff(ip, 0) {
                Ok((b, _)) => b,
                Err(e) => {
                    vput(vp);
                    return Err(e);
                }
            };
            bp = Some(b);
            // SAFETY: as above.
            let data: &'static [u8] = unsafe { b.data() };
            isodir = IsoDirectoryRecord::new(data);
        }
        let Some(isodir) = isodir else {
            panic(format_args!("cd9660_vget_internal: no directory record"));
        };

        ip.iso_extent.set(Doff::from(isonum_733(isodir.extent())));
        ip.i_size.set(Doff::from(isonum_733(isodir.size())));
        ip.iso_start
            .set(Doff::from(isonum_711(isodir.ext_attr_length())) + ip.iso_extent.get());

        // Setup time stamp, attribute
        vp.v_type.set(VNON);
        if imp.iso_ftype == ISO_FTYPE_RRIP {
            cd9660_rrip_analyze(&isodir, ip, imp);
        } else {
            // ISO_FTYPE_9660 (and the default)
            let off = isonum_711(isodir.ext_attr_length());
            let bp2 = if imp.im_flags & ISOFSMNT_EXTATT != 0 && off != 0 {
                cd9660_bufatoff(ip, -(i64::from(off) << imp.im_bshift))
                    .ok()
                    .map(|(b, _)| b)
            } else {
                None
            };
            cd9660_defattr(&isodir, ip, bp2);
            cd9660_deftstamp(&isodir, ip, bp2);
            if let Some(b2) = bp2 {
                brelse(b2);
            }
        }

        if let Some(b) = bp {
            brelse(b);
        }

        // Initialize the associated vnode
        let mut vp = vp;
        let vtype = iftovt(u32::from(ip.inode.get().iso_mode));
        vp.v_type.set(vtype);
        match vtype {
            // FIFO: vp->v_op = &cd9660_fifovops (miscfs/fifofs, not ported); without FIFO:
            VFIFO => {
                vput(vp);
                return Err(Errno::EOPNOTSUPP);
            }
            VCHR | VBLK => {
                // if device, look at device number table for translation
                vp.v_op.set(Some(&CD9660_SPECVOPS));
                if let Some(nvp) = checkalias(vp, ip.inode.get().iso_rdev, Some(mp)) {
                    // Discard unneeded vnode, but save its iso_node. Note that the lock is
                    // carried over in the iso_node
                    nvp.v_data.set(vp.v_data.get());
                    vp.v_data.set(ptr::null_mut());
                    vp.v_op.set(Some(&SPEC_VOPS));
                    vrele(vp);
                    vgone(vp);
                    // Reinitialize aliased inode.
                    vp = nvp;
                    ip.i_vnode.set(Some(vp));
                }
            }
            VLNK | VNON | VSOCK | VDIR | VBAD => {}
            VREG => uvm_vnp_setsize(vp, ip.i_size.get() as i64),
        }

        if ip.iso_extent.get() == Doff::from(imp.root_extent) {
            vp.v_flag.set(vp.v_flag.get() | VROOT);
        }

        // XXX need generation number?

        return Ok(vp);
    }
}

/// `cd9660_vptofh` (`vfs_vptofh`): vnode pointer to file handle.
pub fn cd9660_vptofh(vp: &'static Vnode, fhp: &mut Fid) -> Result<(), Errno> {
    let ip = crate::isofs::cd9660::cd9660_node::vtoi(vp);
    Ifid {
        ifid_len: IFID_SIZE,
        ifid_pad: fhp.fid_reserved,
        ifid_ino: ip.i_number.get() as i32,
        ifid_start: ip.iso_start.get() as i64,
    }
    .to_fid(fhp);

    Ok(())
}

/// `cd9660_check_export` (`vfs_checkexp`): verify a remote client has export rights and
/// return these rights via `exflagsp` and `credanonp`.
pub fn cd9660_check_export(
    mp: &'static Mount,
    nam: &Mbuf,
    _exflagsp: &mut i32,
    _credanonp: &mut *const Ucred,
) -> Result<(), Errno> {
    // Get the export permission structure for this <mp, client> tuple. `im_export` is not
    // kept without NFSSERVER (cd9660_extern.rs).
    let _imp = vfstoisofs(mp);
    let np = vfs_export_lookup(mp, ptr::null_mut(), ptr::from_ref(nam).cast());
    if np.is_null() {
        return Err(Errno::EACCES);
    }

    // *exflagsp = np->netc_exflags; *credanonp = &np->netc_anon: struct netcred
    // (NFSSERVER, not configured; vfs_export_lookup never finds one).
    Err(crate::unported!(
        "cd9660_check_export: struct netcred (NFSSERVER)"
    ))
}

const _: () = {
    assert!(size_of::<Ifid>() == IFID_SIZE as usize);
    assert!(IFID_SIZE as usize <= size_of::<Fid>());
};

#[cfg(test)]
mod tests;
