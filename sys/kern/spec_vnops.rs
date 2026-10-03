/*	$OpenBSD: spec_vnops.c,v 1.114 2025/03/27 23:30:54 tedu Exp $	*/
/*	$NetBSD: spec_vnops.c,v 1.29 1996/04/22 01:42:38 christos Exp $	*/
/* <LICENSES> */
/*
 * Copyright (c) 1989, 1993
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
 *	@(#)spec_vnops.c	8.8 (Berkeley) 11/21/94
 */
/*
 * Copyright (c) 2006 Pedro Martelletto <pedro@ambientworks.net>
 * Copyright (c) 2006 Thordur Bjornsson <thib@openbsd.org>
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
/* </LICENSES> */

//! The vnode operations of special files (`spec_vops`): device vnodes, which pass open,
//! close, read, write and ioctl to the device's driver through the device switch
//! (`cdevsw[]`, `bdevsw[]`), the hash of device vnodes (`speclisth`) and the cloning of
//! `D_CLONE` character devices (`spec_open_clone`).
//!
//! Upstream: sys/kern/spec_vnops.c @ 3ce1f3f79392
//!
//! ## Deviations
//! - The device switch (`struct bdevsw`/`struct cdevsw` of `<sys/conf.h>`, the tables of each
//!   arch's `conf.c`) is not ported. `nchrdev()`/`nblkdev()` answer 0, as for a kernel with
//!   no device configured, so `spec_open` fails with `ENXIO` exactly where the C checks the
//!   major; every other access to a switch entry (`d_open`, `d_close`, `d_read`, `d_write`,
//!   `d_ioctl`, `d_kqfilter`, `d_strategy`, `d_type`, `d_flags`, `chrtoblk`, `iskmemdev`) is
//!   reported with `unported!` where the C makes it (the `cdevsw_d_*` helpers answer as for
//!   a device that is neither a tty, a disk nor a cloning device).
//! - Block-device reads and writes go through the buffer cache as in C; the block size
//!   comes from the disk label (`DIOCGPART` through `bdevsw[].d_ioctl`, `<sys/disklabel.h>`),
//!   which is reported for a configured major and leaves `BLKDEV_IOSIZE`, as when the C's
//!   ioctl fails. `spec_strategy` reports `bdevsw[].d_strategy` and fails the buffer with
//!   `ENXIO` through `biodone`, so a waiter does not sleep forever. `spec_kqfilter`'s
//!   `seltrue_kqfilter` is `kern_event.c`: reported.
//! - `speclisth[]` is a `static` newtype around the buckets with `unsafe impl Sync`, as the
//!   other global list heads.
//! - The operations take their argument structures; the generic ones that fill many slots
//!   are closures (`Some(|_| nullop())`), see `sys/vnode.rs`.

use core::ffi::c_void;
use core::ptr::{self, NonNull};
use core::slice;
use core::sync::atomic::Ordering;

use crate::kern::kern_lock::{mtx_enter, mtx_leave};
use crate::kern::kern_malloc::malloc;
use crate::kern::kern_subr::uiomove;
use crate::kern::kern_sysctl::SECURELEVEL;
use crate::kern::subr_prf::panic;
use crate::kern::subr_xxx::nullop;
use crate::kern::vfs_bio::{bawrite, bdwrite, biodone, bread, breadn, brelse, bufcache_take};
use crate::kern::vfs_biomem::buf_acquire;
use crate::kern::vfs_default::{
    vop_generic_badop, vop_generic_bmap, vop_generic_bwrite, vop_generic_lookup, vop_generic_revoke,
};
use crate::kern::vfs_lockf::lf_advlock;
use crate::kern::vfs_subr::{
    VNODE_MTX, cdevvp, vcount, vfs_mountedon, vinvalbuf, vput, vrele, vwaitforio,
};
use crate::kern::vfs_vnops::vn_lock;
use crate::kern::vfs_vops::{VOP_ACCESS, VOP_GETATTR, VOP_ISLOCKED, VOP_SETATTR, VOP_UNLOCK};
use crate::machine::intr::{splbio, splx};
use crate::sys::buf::{B_BUSY, B_DELWRI, B_ERROR};
use crate::sys::errno::Errno;
use crate::sys::fcntl::FWRITE;
use crate::sys::lock::{LK_EXCLUSIVE, LK_RETRY};
use crate::sys::malloc::{M_TEMP, M_WAITOK};
use crate::sys::mount::{MNT_NODEV, MNT_WAIT};
use crate::sys::param::{BLKDEV_IOSIZE, btodb, clrbit, isclr, setbit};
use crate::sys::queue::SlistHead;
use crate::sys::specdev::{CLONE_MAPSZ, CLONE_SHIFT, Cloneinfo, SPECHSZ, Vnodechain};
use crate::sys::syslimits::{LINK_MAX, MAX_CANON, MAX_INPUT};
use crate::sys::systm::INFSLP;
use crate::sys::types::{Daddr, Register, major, makedev, minor};
use crate::sys::ucred::FSCRED;
use crate::sys::uio::UioRw;
use crate::sys::unistd::{
    _PC_CHOWN_RESTRICTED, _PC_LINK_MAX, _PC_MAX_CANON, _PC_MAX_INPUT, _PC_TIMESTAMP_RESOLUTION,
    _PC_VDISABLE, _POSIX_VDISABLE,
};
use crate::sys::vnode::{
    V_SAVE, VBAD, VBLK, VCHR, VCLONE, VCLONED, VDIR, VFIFO, VISTTY, VLNK, VNON, VREG, VSOCK,
    VXLOCK, Vnode, VopAccessArgs, VopAdvlockArgs, VopCloseArgs, VopFsyncArgs, VopGetattrArgs,
    VopInactiveArgs, VopIoctlArgs, VopKqfilterArgs, VopOpenArgs, VopPathconfArgs, VopPrintArgs,
    VopReadArgs, VopSetattrArgs, VopStrategyArgs, VopWriteArgs, Vops,
};
use crate::unported;

/// `speclisth[SPECHSZ]`: the special device vnodes, hashed by device (`SPECHASH`).
pub struct Speclisth(pub [Vnodechain; SPECHSZ]);

// SAFETY: the chains are changed under the kernel lock, as in C; the kernel runs one CPU.
unsafe impl Sync for Speclisth {}

/// `speclisth[]`.
pub static SPECLISTH: Speclisth = Speclisth([const { SlistHead::new() }; SPECHSZ]);

/// `spec_vops`: the operations of a special (device) vnode.
pub static SPEC_VOPS: Vops = Vops {
    vop_lookup: Some(vop_generic_lookup),
    vop_create: Some(|_| vop_generic_badop()),
    vop_mknod: Some(|_| vop_generic_badop()),
    vop_open: Some(spec_open),
    vop_close: Some(spec_close),
    vop_access: Some(spec_access),
    vop_getattr: Some(spec_getattr),
    vop_setattr: Some(spec_setattr),
    vop_read: Some(spec_read),
    vop_write: Some(spec_write),
    vop_ioctl: Some(spec_ioctl),
    vop_kqfilter: Some(spec_kqfilter),
    vop_revoke: Some(vop_generic_revoke),
    vop_fsync: Some(spec_fsync),
    vop_remove: Some(|_| vop_generic_badop()),
    vop_link: Some(|_| vop_generic_badop()),
    vop_rename: Some(|_| vop_generic_badop()),
    vop_mkdir: Some(|_| vop_generic_badop()),
    vop_rmdir: Some(|_| vop_generic_badop()),
    vop_symlink: Some(|_| vop_generic_badop()),
    vop_readdir: Some(|_| vop_generic_badop()),
    vop_readlink: Some(|_| vop_generic_badop()),
    vop_abortop: Some(|_| vop_generic_badop()),
    vop_inactive: Some(spec_inactive),
    vop_reclaim: Some(|_| nullop()),
    vop_lock: Some(|_| nullop()),
    vop_unlock: Some(|_| nullop()),
    vop_islocked: Some(|_| 0), // nullop
    vop_bmap: Some(vop_generic_bmap),
    vop_strategy: Some(spec_strategy),
    vop_print: Some(spec_print),
    vop_pathconf: Some(spec_pathconf),
    vop_advlock: Some(spec_advlock),
    vop_bwrite: Some(vop_generic_bwrite),
};

/// `nchrdev`: the entries of `cdevsw[]` (`conf.c`, not ported: none).
pub fn nchrdev() -> u32 {
    0
}

/// `nblkdev`: the entries of `bdevsw[]` (`conf.c`, not ported: none).
pub fn nblkdev() -> u32 {
    0
}

/// `cdevsw[maj].d_type == D_TTY` (`conf.c`, not ported).
pub fn cdevsw_d_type_tty(_maj: u32) -> bool {
    let _ = unported!("cdevsw[].d_type (conf.c)");
    false
}

/// `cdevsw[maj].d_type == D_DISK` (`conf.c`, not ported).
fn cdevsw_d_type_disk(_maj: u32) -> bool {
    let _ = unported!("cdevsw[].d_type D_DISK (conf.c)");
    false
}

/// `bdevsw[maj].d_type == D_DISK` (`conf.c`, not ported).
fn bdevsw_d_type_disk(_maj: u32) -> bool {
    let _ = unported!("bdevsw[].d_type D_DISK (conf.c)");
    false
}

/// `cdevsw[maj].d_flags & D_CLONE` (`conf.c`, not ported).
pub fn cdevsw_d_flags_clone(_maj: u32) -> bool {
    let _ = unported!("cdevsw[].d_flags (conf.c)");
    false
}

/// Open a special file.
pub fn spec_open(ap: &mut VopOpenArgs<'_>) -> Result<(), Errno> {
    let p = ap.a_p;
    let vp = ap.a_vp;
    let dev = vp.v_rdev();
    let maj = major(dev);

    // Don't allow open if fs is mounted -nodev.
    if vp
        .v_mount
        .get()
        .is_some_and(|mp| mp.mnt_flag.get() & MNT_NODEV != 0)
    {
        return Err(Errno::ENXIO);
    }

    match vp.v_type.get() {
        VCHR => {
            if maj >= nchrdev() {
                return Err(Errno::ENXIO);
            }
            if !ptr::eq(ap.a_cred, FSCRED) && ap.a_mode & FWRITE != 0 {
                // When running in very secure mode, do not allow opens for writing of any
                // disk character devices.
                if SECURELEVEL.load(Ordering::Relaxed) >= 2 && cdevsw_d_type_disk(maj) {
                    return Err(Errno::EPERM);
                }
                // When running in secure mode, do not allow opens for writing of /dev/mem,
                // /dev/kmem, or character devices whose corresponding block devices are
                // currently mounted.
                if SECURELEVEL.load(Ordering::Relaxed) >= 1 {
                    // chrtoblk(dev), iskmemdev(dev): conf.c.
                    return Err(unported!("spec_open: chrtoblk, iskmemdev (conf.c)"));
                }
            }
            if cdevsw_d_type_tty(maj) {
                vp.v_flag.set(vp.v_flag.get() | VISTTY);
            }
            if cdevsw_d_flags_clone(maj) {
                return spec_open_clone(ap);
            }
            let _ = VOP_UNLOCK(vp);
            // (*cdevsw[maj].d_open)(dev, ap->a_mode, S_IFCHR, p)
            let error = Err(unported!("cdevsw[].d_open (conf.c)"));
            let _ = p;
            let _ = vn_lock(vp, LK_EXCLUSIVE | LK_RETRY);
            error
        }

        VBLK => {
            if maj >= nblkdev() {
                return Err(Errno::ENXIO);
            }
            // When running in very secure mode, do not allow opens for writing of any disk
            // block devices.
            if SECURELEVEL.load(Ordering::Relaxed) >= 2
                && !ptr::eq(ap.a_cred, FSCRED)
                && ap.a_mode & FWRITE != 0
                && bdevsw_d_type_disk(maj)
            {
                return Err(Errno::EPERM);
            }
            // Do not allow opens of block devices that are currently mounted.
            vfs_mountedon(vp)?;
            Err(unported!("bdevsw[].d_open (conf.c)"))
        }
        VNON | VLNK | VDIR | VREG | VBAD | VFIFO | VSOCK => Ok(()),
    }
}

/// Vnode op for read.
pub fn spec_read(ap: &mut VopReadArgs<'_, '_>) -> Result<(), Errno> {
    let vp = ap.a_vp;
    let uio = &mut *ap.a_uio;

    #[cfg(feature = "diagnostic")]
    if uio.uio_rw != UioRw::UIO_READ {
        panic(format_args!("spec_read mode"));
    }
    #[cfg(not(feature = "diagnostic"))]
    let _ = UioRw::UIO_READ;
    if uio.uio_resid == 0 {
        return Ok(());
    }

    match vp.v_type.get() {
        VCHR => {
            let _ = VOP_UNLOCK(vp);
            let error = Err(unported!("cdevsw[].d_read (conf.c)"));
            let _ = vn_lock(vp, LK_EXCLUSIVE | LK_RETRY);
            error
        }

        VBLK => {
            if uio.uio_offset < 0 {
                return Err(Errno::EINVAL);
            }
            let bsize = spec_bsize(vp, "spec_read");
            let bscale = btodb(bsize as usize) as Daddr;
            let Some(si) = vp.v_specinfo() else {
                panic(format_args!("spec_read: no specinfo"));
            };
            loop {
                let bn = btodb(uio.uio_offset as usize) as Daddr & !(bscale - 1);
                let on = (uio.uio_offset % i64::from(bsize)) as usize;
                let mut n = (bsize as usize - on).min(uio.uio_resid);
                let (bp, error) = if si.si_lastr.get() + bscale == bn {
                    let nextbn = bn + bscale;
                    breadn(vp, bn, bsize, &[nextbn], &[bsize])
                } else {
                    bread(vp, bn, bsize)
                };
                si.si_lastr.set(bn);
                n = n.min(bsize as usize - bp.b_resid.get());
                if let Err(error) = error {
                    brelse(bp);
                    return Err(error);
                }
                // SAFETY: `bread` returned the buffer busy and mapped, for us alone.
                let data = unsafe { bp.data() };
                let error = uiomove(&mut data[on..on + n], uio);
                brelse(bp);
                error?;
                if uio.uio_resid == 0 || n == 0 {
                    return Ok(());
                }
            }
        }

        _ => panic(format_args!("spec_read type")),
    }
}

/// `spec_inactive`: nothing to do but unlock.
pub fn spec_inactive(ap: &mut VopInactiveArgs<'_>) -> Result<(), Errno> {
    let _ = VOP_UNLOCK(ap.a_vp);
    Ok(())
}

/// Vnode op for write.
pub fn spec_write(ap: &mut VopWriteArgs<'_, '_>) -> Result<(), Errno> {
    let vp = ap.a_vp;
    let uio = &mut *ap.a_uio;

    #[cfg(feature = "diagnostic")]
    if uio.uio_rw != UioRw::UIO_WRITE {
        panic(format_args!("spec_write mode"));
    }

    match vp.v_type.get() {
        VCHR => {
            let _ = VOP_UNLOCK(vp);
            let error = Err(unported!("cdevsw[].d_write (conf.c)"));
            let _ = vn_lock(vp, LK_EXCLUSIVE | LK_RETRY);
            error
        }

        VBLK => {
            if uio.uio_resid == 0 {
                return Ok(());
            }
            if uio.uio_offset < 0 {
                return Err(Errno::EINVAL);
            }
            let bsize = spec_bsize(vp, "spec_write");
            let bscale = btodb(bsize as usize) as Daddr;
            loop {
                let bn = btodb(uio.uio_offset as usize) as Daddr & !(bscale - 1);
                let on = (uio.uio_offset % i64::from(bsize)) as usize;
                let mut n = (bsize as usize - on).min(uio.uio_resid);
                let (bp, error) = bread(vp, bn, bsize);
                n = n.min(bsize as usize - bp.b_resid.get());
                if let Err(error) = error {
                    brelse(bp);
                    return Err(error);
                }
                // SAFETY: `bread` returned the buffer busy and mapped, for us alone.
                let data = unsafe { bp.data() };
                let error = uiomove(&mut data[on..on + n], uio);
                if n + on == bsize as usize {
                    bawrite(bp);
                } else {
                    bdwrite(bp);
                }
                error?;
                if uio.uio_resid == 0 || n == 0 {
                    return Ok(());
                }
            }
        }

        _ => panic(format_args!("spec_write type")),
    }
}

/// The block size of a block device's I/O: `BLKDEV_IOSIZE`, or the FFS block of its partition
/// that `DIOCGPART` reports (`frag * fsize` of an `FS_BSDFFS` partition).
fn spec_bsize(vp: &'static Vnode, who: &'static str) -> i32 {
    let bsize = BLKDEV_IOSIZE as i32;
    if major(vp.v_rdev()) < nblkdev() {
        // (*bdevsw[majordev].d_ioctl)(vp->v_rdev, DIOCGPART, &dpart, FREAD, p): the device
        // switch (conf.c) and struct partinfo (disklabel.h). Without them the C's ioctl
        // failure path: BLKDEV_IOSIZE.
        let _ = who;
        let _ = unported!("spec_read/spec_write: DIOCGPART (conf.c, disklabel.h)");
    }
    bsize
}

/// Device ioctl operation.
pub fn spec_ioctl(ap: &mut VopIoctlArgs<'_>) -> Result<(), Errno> {
    match ap.a_vp.v_type.get() {
        VCHR => Err(unported!("cdevsw[].d_ioctl (conf.c)")),
        VBLK => Err(unported!("bdevsw[].d_ioctl (conf.c)")),
        _ => panic(format_args!("spec_ioctl")),
    }
}

/// `spec_kqfilter`: the driver's `d_kqfilter`, or `seltrue_kqfilter` for poll/select.
pub fn spec_kqfilter(ap: &mut VopKqfilterArgs) -> Result<(), Errno> {
    match ap.a_vp.v_type.get() {
        VCHR => Err(unported!("cdevsw[].d_kqfilter (conf.c)")),
        // kn_flags & (__EV_POLL | __EV_SELECT): seltrue_kqfilter (kern_event.c).
        _ => Err(unported!("spec_kqfilter: seltrue_kqfilter (kern_event.c)")),
    }
}

/// Synch buffers associated with a block device.
pub fn spec_fsync(ap: &mut VopFsyncArgs<'_>) -> Result<(), Errno> {
    let vp = ap.a_vp;

    if vp.v_type.get() == VCHR {
        return Ok(());
    }
    // Flush all dirty buffers associated with a block device.
    'restart: loop {
        // loop:
        let s = splbio();
        for bp in vp.v_dirtyblkhd.iter() {
            if bp.isset(B_BUSY) {
                continue;
            }
            if !bp.isset(B_DELWRI) {
                panic(format_args!("spec_fsync: not dirty"));
            }
            bufcache_take(bp);
            buf_acquire(bp);
            splx(s);
            bawrite(bp);
            continue 'restart;
        }
        if ap.a_waitfor == MNT_WAIT {
            let _ = vwaitforio(vp, 0, "spec_fsync", INFSLP);

            #[cfg(feature = "diagnostic")]
            if !vp.v_dirtyblkhd.is_empty() {
                splx(s);
                crate::kern::vfs_subr::vprint(Some("spec_fsync: dirty"), vp);
                continue 'restart;
            }
        }
        splx(s);
        return Ok(());
    }
}

/// `spec_strategy`: `bdevsw[major(bp->b_dev)].d_strategy(bp)`.
pub fn spec_strategy(ap: &mut VopStrategyArgs) -> Result<(), Errno> {
    let bp = ap.a_bp;
    let _maj = major(bp.b_dev.get());

    // (*bdevsw[maj].d_strategy)(bp): the device switch (conf.c). No driver takes the buffer,
    // so it completes with an error, as a driver does for a device that is gone.
    let _ = unported!("spec_strategy: bdevsw[].d_strategy (conf.c)");
    bp.b_error.set(Some(Errno::ENXIO));
    bp.set(B_ERROR);
    let s = splbio();
    biodone(bp);
    splx(s);
    Ok(())
}

/// Device close routine.
pub fn spec_close(ap: &mut VopCloseArgs<'_>) -> Result<(), Errno> {
    let p = ap.a_p;
    let vp = ap.a_vp;
    let dev = vp.v_rdev();
    let mut clone = false;

    mtx_enter(&VNODE_MTX);
    let xlocked = vp.v_lflag.get() & VXLOCK != 0;
    mtx_leave(&VNODE_MTX);

    match vp.v_type.get() {
        VCHR => {
            // Hack: a tty device that is a controlling terminal has a reference from the
            // session structure. We cannot easily tell that a character device is a
            // controlling terminal, unless it is the closing process' controlling terminal.
            // In that case, if the reference count is 2 (this last descriptor plus the
            // session), release the reference from the session.
            if vcount(vp) == 2
                && let Some(p) = p
                && !p.process().ps_pgrp.get().is_null()
            {
                let s = p.process().session();
                // SAFETY: the process's group and session are alive while it is.
                let s = unsafe { &*s };
                if ptr::eq(s.s_ttyvp.get().cast(), vp) {
                    vrele(vp);
                    s.s_ttyvp.set(ptr::null());
                }
            }
            if cdevsw_d_flags_clone(major(dev)) {
                clone = true;
            } else {
                // If the vnode is locked, then we are in the midst of forcibly closing the
                // device, otherwise we only close on last reference.
                if vcount(vp) > 1 && !xlocked {
                    return Ok(());
                }
            }
        }

        VBLK => {
            // On last close of a block device (that isn't mounted) we must invalidate any in
            // core blocks, so that we can, for instance, change floppy disks. In order to do
            // that, we must lock the vnode. If we are coming from vclean(), the vnode is
            // already locked.
            if !xlocked {
                let _ = vn_lock(vp, LK_EXCLUSIVE | LK_RETRY);
            }
            let error = vinvalbuf(vp, V_SAVE, ap.a_cred, p, 0, INFSLP);
            if !xlocked {
                let _ = VOP_UNLOCK(vp);
            }
            error?;
            // We do not want to really close the device if it is still in use unless we are
            // trying to close it forcibly. Since every use (buffer, vnode, swap, cmap) holds
            // a reference to the vnode, and because we mark any other vnodes that alias this
            // device, when the sum of the reference counts on all the aliased vnodes descends
            // to one, we are on last close.
            if vcount(vp) > 1 && !xlocked {
                return Ok(());
            }
        }

        _ => panic(format_args!("spec_close: not special")),
    }

    // release lock if held and this isn't coming from vclean()
    let relock = VOP_ISLOCKED(vp) != 0 && !xlocked;
    if relock {
        let _ = VOP_UNLOCK(vp);
    }
    let error = Err(if vp.v_type.get() == VCHR {
        unported!("cdevsw[].d_close (conf.c)")
    } else {
        unported!("bdevsw[].d_close (conf.c)")
    });
    if relock {
        let _ = vn_lock(vp, LK_EXCLUSIVE | LK_RETRY);
    }

    if error.is_ok()
        && clone
        && let Some(pvp) = vp.v_specparent()
    {
        // get parent device
        if let Some(map) = clone_bitmap(pvp.v_specinfo().map(|si| si.si_ci_bitmap.get())) {
            clrbit(map, (minor(dev) >> CLONE_SHIFT) as usize);
        }
        vrele(pvp);
    }

    error
}

/// The `CLONE_MAPSZ` bytes of a cloning device's bitmap.
fn clone_bitmap<'b>(map: Option<*mut u8>) -> Option<&'b mut [u8]> {
    let map = NonNull::new(map?)?;
    // SAFETY: `si_ci_bitmap` is null or the `CLONE_MAPSZ`-byte allocation `checkalias` made
    // for the cloning device (shared by its aliases), alive until `vgonel` frees it.
    Some(unsafe { slice::from_raw_parts_mut(map.as_ptr(), CLONE_MAPSZ) })
}

/// `spec_getattr`: a clone answers with its parent's attributes; other device vnodes have
/// none of their own.
pub fn spec_getattr(ap: &mut VopGetattrArgs<'_>) -> Result<(), Errno> {
    let vp = ap.a_vp;

    if vp.v_flag.get() & VCLONE == 0 {
        return Err(Errno::EBADF);
    }
    let Some(parent) = vp.v_specparent() else {
        return Err(Errno::EBADF);
    };

    let _ = vn_lock(parent, LK_EXCLUSIVE | LK_RETRY);
    let error = VOP_GETATTR(parent, ap.a_vap, ap.a_cred, ap.a_p);
    let _ = VOP_UNLOCK(parent);

    error
}

/// `spec_setattr`: a clone sets its parent's attributes.
pub fn spec_setattr(ap: &mut VopSetattrArgs<'_>) -> Result<(), Errno> {
    let p = ap.a_p;
    let vp = ap.a_vp;

    if vp.v_flag.get() & VCLONE == 0 {
        return Err(Errno::EBADF);
    }
    let Some(parent) = vp.v_specparent() else {
        return Err(Errno::EBADF);
    };

    let _ = vn_lock(parent, LK_EXCLUSIVE | LK_RETRY);
    let error = VOP_SETATTR(parent, ap.a_vap, ap.a_cred, p);
    let _ = VOP_UNLOCK(parent);

    error
}

/// `spec_access`: a clone checks its parent's permissions.
pub fn spec_access(ap: &mut VopAccessArgs<'_>) -> Result<(), Errno> {
    let vp = ap.a_vp;

    if vp.v_flag.get() & VCLONE == 0 {
        return Err(Errno::EBADF);
    }
    let Some(parent) = vp.v_specparent() else {
        return Err(Errno::EBADF);
    };

    let _ = vn_lock(parent, LK_EXCLUSIVE | LK_RETRY);
    let error = VOP_ACCESS(parent, ap.a_mode, ap.a_cred, ap.a_p);
    let _ = VOP_UNLOCK(parent);

    error
}

/// Print out the contents of a special device vnode.
pub fn spec_print(ap: &mut VopPrintArgs) -> Result<(), Errno> {
    #[cfg(any(feature = "debug", feature = "diagnostic"))]
    crate::kprintf!(
        "tag VT_NON, dev {}, {}\n",
        major(ap.a_vp.v_rdev()),
        minor(ap.a_vp.v_rdev())
    );
    #[cfg(not(any(feature = "debug", feature = "diagnostic")))]
    let _ = ap;
    Ok(())
}

/// Return POSIX pathconf information applicable to special devices.
pub fn spec_pathconf(ap: &mut VopPathconfArgs<'_>) -> Result<(), Errno> {
    *ap.a_retval = match ap.a_name {
        _PC_LINK_MAX => LINK_MAX as Register,
        _PC_MAX_CANON => MAX_CANON as Register,
        _PC_MAX_INPUT => MAX_INPUT as Register,
        _PC_CHOWN_RESTRICTED => 1,
        _PC_VDISABLE => Register::from(_POSIX_VDISABLE),
        _PC_TIMESTAMP_RESOLUTION => 1,
        _ => return Err(Errno::EINVAL),
    };

    Ok(())
}

/// Special device advisory byte-level locks.
pub fn spec_advlock(ap: &mut VopAdvlockArgs<'_>) -> Result<(), Errno> {
    let vp = ap.a_vp;

    let Some(si) = vp.v_specinfo() else {
        panic(format_args!("spec_advlock: no specinfo"));
    };
    lf_advlock(&si.si_lockf, 0, ap.a_id, ap.a_op, ap.a_fl, ap.a_flags)
}

/// `spec_open_clone(ap)`: opens a new instance of a `D_CLONE` character device: a new vnode
/// whose minor carries the instance number, recorded in the parent's bitmap.
pub fn spec_open_clone(ap: &mut VopOpenArgs<'_>) -> Result<(), Errno> {
    let vp = ap.a_vp;
    let rdev = vp.v_rdev();

    // CLONE_DEBUG: not configured.

    if minor(rdev) >= 1 << CLONE_SHIFT {
        return Err(Errno::ENXIO);
    }

    let Some(map) = clone_bitmap(vp.v_specinfo().map(|si| si.si_ci_bitmap.get())) else {
        return Err(Errno::ENXIO);
    };
    let mut i = 1;
    while i < CLONE_MAPSZ * u8::BITS as usize {
        if isclr(map, i) {
            setbit(map, i);
            break;
        }
        i += 1;
    }

    if i == CLONE_MAPSZ * u8::BITS as usize {
        return Err(Errno::EBUSY); // too many open instances
    }

    let cvp = match cdevvp(makedev(
        major(rdev),
        ((i as u32) << CLONE_SHIFT) | minor(rdev),
    )) {
        Ok(Some(cvp)) => cvp,
        Ok(None) | Err(_) => {
            clrbit(map, i);
            return Err(Errno::ENFILE); // out of vnodes
        }
    };

    let _ = VOP_UNLOCK(vp);

    let error: Result<(), Errno> = Err(unported!("cdevsw[].d_open (conf.c)"));

    let _ = vn_lock(vp, LK_EXCLUSIVE | LK_RETRY);

    if let Err(error) = error {
        vput(cvp);
        clrbit(map, i);
        return Err(error); // device open failed
    }

    cvp.v_flag.set(cvp.v_flag.get() | VCLONE);

    let Some(mem) = malloc(size_of::<Cloneinfo>(), M_TEMP, M_WAITOK) else {
        panic(format_args!("spec_open_clone: out of memory"));
    };
    let cip = mem.cast::<Cloneinfo>();
    // SAFETY: a fresh, suitably aligned allocation of `size_of::<Cloneinfo>()` bytes; `vn_open`
    // reads it back from `v_data` and frees it.
    unsafe {
        cip.as_ptr().write(Cloneinfo {
            ci_vp: cvp,
            ci_data: vp.v_data.get(),
        })
    };

    if let Some(si) = cvp.v_specinfo() {
        si.si_ci_parent.set(Some(vp));
    }
    vp.v_flag.set(vp.v_flag.get() | VCLONED);
    vp.v_data.set(cip.as_ptr().cast::<c_void>());

    Ok(()) // device cloned
}
