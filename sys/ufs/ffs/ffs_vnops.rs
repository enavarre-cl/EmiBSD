/* <LICENSES> */
/*
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
 *	@(#)ffs_vnops.c	8.10 (Berkeley) 8/10/94
 */
/* </LICENSES> */

/* <CODE> */
//! The fast file system's vnode operations: the tables for files (`ffs_vops`) and for the
//! special files that live on it (`ffs_specvops`), and the operations that are FFS's own:
//! reading and writing a file through the buffer cache (`ffs_read`, `ffs_write`), flushing
//! it (`ffs_fsync`) and freeing its inode (`ffs_reclaim`).
//!
//! Upstream: sys/ufs/ffs/ffs_vnops.c @ 3ce1f3f79392
//!
//! ## Deviations
//! - `option FIFO` is in GENERIC but `miscfs/fifofs` is not ported: `ffs_fifovops` and
//!   `ffsfifo_reclaim` come with it, and `ffs_vinit` refuses fifos meanwhile
//!   (`ffs_subr.rs`).
//! - The helpers the C installs in many slots (`vop_generic_badop`) are closures, as in
//!   `spec_vops` (`docs/C_TO_RUST.md`).
//! - `ffs_fsync`'s `LIST_FOREACH_SAFE` that restarts from the head after each write is a
//!   scan that starts again from the head; the `B_SCANNED` marks keep it from visiting a
//!   buffer twice, as in C.

use core::ptr::{self, NonNull};

use crate::kern::kern_subr::uiomove;
use crate::kern::spec_vnops::{
    spec_advlock, spec_ioctl, spec_kqfilter, spec_open, spec_pathconf, spec_strategy,
};
use crate::kern::subr_pool::pool_put;
use crate::kern::subr_prf::panic;
use crate::kern::vfs_bio::{bawrite, bdwrite, bread, bread_cluster, brelse, bufcache_take, bwrite};
use crate::kern::vfs_biomem::buf_acquire;
use crate::kern::vfs_default::{
    vop_generic_abortop, vop_generic_badop, vop_generic_bmap, vop_generic_bwrite,
    vop_generic_lookup, vop_generic_revoke,
};
use crate::kern::vfs_subr::{vnoperm, vwaitforio};
use crate::kern::vfs_vnops::vn_fsizechk;
use crate::machine::intr::{splbio, splx};
use crate::sys::buf::{B_BUSY, B_CLRBUF, B_DELWRI, B_NOCACHE, B_SCANNED, B_SYNC};
use crate::sys::errno::Errno;
use crate::sys::event::{NOTE_EXTEND, NOTE_WRITE};
use crate::sys::mount::{MNT_NOATIME, MNT_WAIT};
use crate::sys::stat::APPEND;
use crate::sys::systm::INFSLP;
#[cfg(feature = "diagnostic")]
use crate::sys::uio::UioRw;
use crate::sys::vnode::VN_KNOTE;
use crate::sys::vnode::{
    IO_APPEND, IO_NOCACHE, IO_SYNC, IO_UNIT, VDIR, VLNK, VREG, VopFsyncArgs, VopReadArgs,
    VopReclaimArgs, VopWriteArgs, Vops, cred_ref,
};
#[cfg(feature = "ffs2")]
use crate::ufs::ffs::ffs_vfsops::FFS_DINODE2_POOL;
use crate::ufs::ffs::ffs_vfsops::{FFS_DINODE1_POOL, FFS_INO_POOL};
use crate::ufs::ffs::fs::{blkoff, blksize, lblkno, lblktosize};
use crate::ufs::ufs::dinode::{ISGID, ISUID, NIADDR};
use crate::ufs::ufs::inode::{
    IN_ACCESS, IN_CHANGE, IN_UPDATE, UFS_BUF_ALLOC, UFS_TRUNCATE, UFS_UPDATE, vtoi,
};
use crate::ufs::ufs::ufs_bmap::ufs_bmap;
use crate::ufs::ufs::ufs_inode::{ufs_inactive, ufs_reclaim};
use crate::ufs::ufs::ufs_lookup::ufs_lookup;
use crate::ufs::ufs::ufs_vnops::{
    ufs_access, ufs_advlock, ufs_close, ufs_create, ufs_getattr, ufs_ioctl, ufs_islocked,
    ufs_kqfilter, ufs_link, ufs_lock, ufs_mkdir, ufs_mknod, ufs_open, ufs_pathconf, ufs_print,
    ufs_readdir, ufs_readlink, ufs_remove, ufs_rename, ufs_rmdir, ufs_setattr, ufs_strategy,
    ufs_symlink, ufs_unlock, ufsspec_close, ufsspec_read, ufsspec_write,
};
use crate::uvm::uvm_vnode::{uvm_vnp_setsize, uvm_vnp_uncache};

/// `ffs_vops`: the operations of a file, directory or symbolic link on an FFS.
pub static FFS_VOPS: Vops = Vops {
    vop_lookup: Some(ufs_lookup),
    vop_create: Some(ufs_create),
    vop_mknod: Some(ufs_mknod),
    vop_open: Some(ufs_open),
    vop_close: Some(ufs_close),
    vop_access: Some(ufs_access),
    vop_getattr: Some(ufs_getattr),
    vop_setattr: Some(ufs_setattr),
    vop_read: Some(ffs_read),
    vop_write: Some(ffs_write),
    vop_ioctl: Some(ufs_ioctl),
    vop_kqfilter: Some(ufs_kqfilter),
    vop_revoke: Some(vop_generic_revoke),
    vop_fsync: Some(ffs_fsync),
    vop_remove: Some(ufs_remove),
    vop_link: Some(ufs_link),
    vop_rename: Some(ufs_rename),
    vop_mkdir: Some(ufs_mkdir),
    vop_rmdir: Some(ufs_rmdir),
    vop_symlink: Some(ufs_symlink),
    vop_readdir: Some(ufs_readdir),
    vop_readlink: Some(ufs_readlink),
    vop_abortop: Some(vop_generic_abortop),
    vop_inactive: Some(ufs_inactive),
    vop_reclaim: Some(ffs_reclaim),
    vop_lock: Some(ufs_lock),
    vop_unlock: Some(ufs_unlock),
    vop_bmap: Some(ufs_bmap),
    vop_strategy: Some(ufs_strategy),
    vop_print: Some(ufs_print),
    vop_islocked: Some(ufs_islocked),
    vop_pathconf: Some(ufs_pathconf),
    vop_advlock: Some(ufs_advlock),
    vop_bwrite: Some(vop_generic_bwrite),
};

/// `ffs_specvops`: the operations of a device special file on an FFS.
pub static FFS_SPECVOPS: Vops = Vops {
    vop_close: Some(ufsspec_close),
    vop_access: Some(ufs_access),
    vop_getattr: Some(ufs_getattr),
    vop_setattr: Some(ufs_setattr),
    vop_read: Some(ufsspec_read),
    vop_write: Some(ufsspec_write),
    vop_fsync: Some(ffs_fsync),
    vop_inactive: Some(ufs_inactive),
    vop_reclaim: Some(ffs_reclaim),
    vop_lock: Some(ufs_lock),
    vop_unlock: Some(ufs_unlock),
    vop_print: Some(ufs_print),
    vop_islocked: Some(ufs_islocked),

    // XXX: Keep in sync with spec_vops
    vop_lookup: Some(vop_generic_lookup),
    vop_create: Some(|_| vop_generic_badop()),
    vop_mknod: Some(|_| vop_generic_badop()),
    vop_open: Some(spec_open),
    vop_ioctl: Some(spec_ioctl),
    vop_kqfilter: Some(spec_kqfilter),
    vop_revoke: Some(vop_generic_revoke),
    vop_remove: Some(|_| vop_generic_badop()),
    vop_link: Some(|_| vop_generic_badop()),
    vop_rename: Some(|_| vop_generic_badop()),
    vop_mkdir: Some(|_| vop_generic_badop()),
    vop_rmdir: Some(|_| vop_generic_badop()),
    vop_symlink: Some(|_| vop_generic_badop()),
    vop_readdir: Some(|_| vop_generic_badop()),
    vop_readlink: Some(|_| vop_generic_badop()),
    vop_abortop: Some(|_| vop_generic_badop()),
    vop_bmap: Some(vop_generic_bmap),
    vop_strategy: Some(spec_strategy),
    vop_pathconf: Some(spec_pathconf),
    vop_advlock: Some(spec_advlock),
    vop_bwrite: Some(vop_generic_bwrite),
};

/// `ffs_read` (`vop_read`): vnode op for reading.
pub fn ffs_read(ap: &mut VopReadArgs<'_, '_>) -> Result<(), Errno> {
    let vp = ap.a_vp;
    let ip = vtoi(vp);
    let uio = &mut *ap.a_uio;

    #[cfg(feature = "diagnostic")]
    {
        if uio.uio_rw != UioRw::UIO_READ {
            panic(format_args!("ffs_read: mode"));
        }

        if vp.v_type.get() == VLNK {
            if ip.dip_size() < u64::from(ip.ump().um_maxsymlinklen.get()) {
                panic(format_args!("ffs_read: short symlink"));
            }
        } else if vp.v_type.get() != VREG && vp.v_type.get() != VDIR {
            panic(format_args!("ffs_read: type {}", vp.v_type.get() as i32));
        }
    }
    let fs = ip.fs();
    if uio.uio_offset < 0 {
        return Err(Errno::EINVAL);
    }
    if uio.uio_resid == 0 {
        return Ok(());
    }

    let mut error = Ok(());
    while uio.uio_resid > 0 {
        let bytesinfile = ip.dip_size() as i64 - uio.uio_offset;
        if bytesinfile <= 0 {
            break;
        }
        let lbn = lblkno(fs, uio.uio_offset);
        let nextlbn = lbn + 1;
        let mut size = i64::from(fs.fs_bsize.get()); // WAS blksize(fs, ip, lbn);
        let blkoffset = blkoff(fs, uio.uio_offset);
        let mut xfersize = i64::from(fs.fs_bsize.get()) - blkoffset;
        if (uio.uio_resid as i64) < xfersize {
            xfersize = uio.uio_resid as i64;
        }
        if bytesinfile < xfersize {
            xfersize = bytesinfile;
        }

        let (bp, r) = if lblktosize(fs, nextlbn) >= ip.dip_size() as i64 {
            bread(vp, lbn, size as i32)
        } else if lbn - 1 == ip.i_ci.get().ci_lastr || uio.uio_resid as i64 > xfersize {
            bread_cluster(vp, lbn, size as i32)
        } else {
            bread(vp, lbn, size as i32)
        };

        if let Err(e) = r {
            error = Err(e);
            brelse(bp);
            break;
        }
        let mut ci = ip.i_ci.get();
        ci.ci_lastr = lbn;
        ip.i_ci.set(ci);

        // We should only get non-zero b_resid when an I/O error has occurred, which should
        // cause us to break above. However, if the short read did not cause an error, then
        // we want to ensure that we do not uiomove bad or uninitialized data.
        size -= bp.b_resid.get() as i64;
        if size < xfersize {
            if size == 0 {
                brelse(bp);
                break;
            }
            xfersize = size;
        }
        // SAFETY: the buffer is ours (busy from bread) and mapped; the slice dies before it
        // is released.
        let data = unsafe { bp.data() };
        let r = uiomove(
            &mut data[blkoffset as usize..(blkoffset + xfersize) as usize],
            uio,
        );
        brelse(bp);
        if let Err(e) = r {
            error = Err(e);
            break;
        }
    }
    if vp
        .v_mount
        .get()
        .is_some_and(|mp| mp.mnt_flag.get() & MNT_NOATIME == 0)
        || ip.i_flag.get() & (IN_CHANGE | IN_UPDATE) != 0
    {
        ip.set_flag(IN_ACCESS);
    }
    error
}

/// `ffs_write` (`vop_write`): vnode op for writing.
pub fn ffs_write(ap: &mut VopWriteArgs<'_, '_>) -> Result<(), Errno> {
    let mut extended = false;
    let ioflag = ap.a_ioflag;
    let uio = &mut *ap.a_uio;
    let vp = ap.a_vp;
    let ip = vtoi(vp);

    #[cfg(feature = "diagnostic")]
    if uio.uio_rw != UioRw::UIO_WRITE {
        panic(format_args!("ffs_write: mode"));
    }

    // If writing 0 bytes, succeed and do not change update time or file offset (standards
    // compliance)
    if uio.uio_resid == 0 {
        return Ok(());
    }

    match vp.v_type.get() {
        VREG => {
            if ioflag & IO_APPEND != 0 {
                uio.uio_offset = ip.dip_size() as i64;
            }
            if ip.dip_flags() & APPEND != 0 && uio.uio_offset != ip.dip_size() as i64 {
                return Err(Errno::EPERM);
            }
        }
        VLNK => {}
        VDIR => {
            if ioflag & IO_SYNC == 0 {
                panic(format_args!("ffs_write: nonsync dir write"));
            }
        }
        t => panic(format_args!("ffs_write: type {}", t as i32)),
    }

    let fs = ip.fs();
    if uio.uio_offset < 0
        || (uio.uio_offset as u64).wrapping_add(uio.uio_resid as u64) > fs.fs_maxfilesize.get()
    {
        return Err(Errno::EFBIG);
    }

    // do the filesize rlimit check
    let overrun = vn_fsizechk(vp, uio, ioflag)?;

    let resid = uio.uio_resid;
    let osize = ip.dip_size() as i64;
    let mut flags = if ioflag & IO_SYNC != 0 { B_SYNC } else { 0 };

    let mut error = Ok(());
    while uio.uio_resid > 0 {
        let lbn = lblkno(fs, uio.uio_offset);
        let blkoffset = blkoff(fs, uio.uio_offset);
        let mut xfersize = i64::from(fs.fs_bsize.get()) - blkoffset;
        if (uio.uio_resid as i64) < xfersize {
            xfersize = uio.uio_resid as i64;
        }
        if i64::from(fs.fs_bsize.get()) > xfersize {
            flags |= B_CLRBUF;
        } else {
            flags &= !B_CLRBUF;
        }

        let bp = match UFS_BUF_ALLOC(ip, uio.uio_offset, xfersize as i32, ap.a_cred, flags) {
            Ok(bp) => bp,
            Err(e) => {
                error = Err(e);
                break;
            }
        };
        if (uio.uio_offset + xfersize) as u64 > ip.dip_size() {
            ip.dip_set_size((uio.uio_offset + xfersize) as u64);
            uvm_vnp_setsize(vp, ip.dip_size() as i64);
            extended = true;
        }
        let _ = uvm_vnp_uncache(vp);

        let size = blksize(fs, ip, lbn) as i64 - bp.b_resid.get() as i64;
        if size < xfersize {
            xfersize = size;
        }

        {
            // SAFETY: the buffer is ours (busy from UFS_BUF_ALLOC) and mapped; the slice
            // dies before it is written.
            let data = unsafe { bp.data() };
            let range = blkoffset as usize..(blkoffset + xfersize) as usize;
            error = uiomove(&mut data[range.clone()], uio);
            // If the buffer is not already filled and we encounter an error while trying to
            // fill it, we have to clear out any garbage data from the pages instantiated for
            // the buffer. If we do not, a failed uiomove() during a write can leave the
            // prior contents of the pages exposed to a userland mmap.
            //
            // Note that we don't need to clear buffers that were allocated with the
            // B_CLRBUF flag set.
            if error.is_err() && flags & B_CLRBUF == 0 {
                data[range].fill(0);
            }
        }

        if ioflag & IO_NOCACHE != 0 {
            bp.set(B_NOCACHE);
        }

        if ioflag & IO_SYNC != 0 {
            let _ = bwrite(bp);
        } else if xfersize + blkoffset == i64::from(fs.fs_bsize.get()) {
            bawrite(bp);
        } else {
            bdwrite(bp);
        }

        if error.is_err() || xfersize == 0 {
            break;
        }
        ip.set_flag(IN_CHANGE | IN_UPDATE);
    }
    // If we successfully wrote any data, and we are not the superuser we clear the setuid
    // and setgid bits as a precaution against tampering.
    // SAFETY: the write's credentials are held by its caller (`cred_ref`'s contract).
    let cred = unsafe { cred_ref(ap.a_cred) };
    if resid > uio.uio_resid && cred.is_some_and(|c| c.cr_uid.get() != 0) && !vnoperm(vp) {
        ip.dip_set_mode(ip.dip_mode() & !(ISUID | ISGID));
    }
    if resid > uio.uio_resid {
        VN_KNOTE(vp, NOTE_WRITE | if extended { NOTE_EXTEND } else { 0 });
    }
    if error.is_err() {
        if ioflag & IO_UNIT != 0 {
            let _ = UFS_TRUNCATE(ip, osize, ioflag & IO_SYNC, ap.a_cred);
            uio.uio_offset -= (resid - uio.uio_resid) as i64;
            uio.uio_resid = resid;
        }
    } else if resid > uio.uio_resid && ioflag & IO_SYNC != 0 {
        error = UFS_UPDATE(ip, 1);
    }
    // correct the result for writes clamped by vn_fsizechk()
    uio.uio_resid = (uio.uio_resid as isize + overrun) as usize;
    error
}

/// `ffs_fsync` (`vop_fsync`): synch an open file.
pub fn ffs_fsync(ap: &mut VopFsyncArgs<'_>) -> Result<(), Errno> {
    let vp = ap.a_vp;

    // Flush all dirty buffers associated with a vnode.
    let mut passes = NIADDR as i32 + 1;
    let mut skipmeta = ap.a_waitfor == MNT_WAIT;
    let mut s = splbio();
    'loop_: loop {
        for bp in vp.v_dirtyblkhd.iter() {
            bp.clr(B_SCANNED);
        }
        'scan: loop {
            for bp in vp.v_dirtyblkhd.iter() {
                // Reasons to skip this buffer: it has already been considered on this
                // pass, this pass is the first time through on a synchronous flush request
                // and the buffer being considered is metadata, the buffer has dependencies
                // that will cause it to be redirtied and it has not already been deferred,
                // or it is already being written.
                if bp.isset(B_BUSY | B_SCANNED) {
                    continue;
                }
                if !bp.isset(B_DELWRI) {
                    panic(format_args!("ffs_fsync: not dirty"));
                }
                if skipmeta && bp.b_lblkno.get() < 0 {
                    continue;
                }

                bufcache_take(bp);
                buf_acquire(bp);
                bp.set(B_SCANNED);
                splx(s);
                // On our final pass through, do all I/O synchronously so that we can find
                // out if our flush is failing because of write errors.
                if passes > 0 || ap.a_waitfor != MNT_WAIT {
                    bawrite(bp);
                } else {
                    bwrite(bp)?;
                }
                s = splbio();
                // Since we may have slept during the I/O, we need to start from a known
                // point.
                continue 'scan;
            }
            break;
        }
        if skipmeta {
            skipmeta = false;
            continue 'loop_;
        }
        if ap.a_waitfor == MNT_WAIT {
            let _ = vwaitforio(vp, 0, "ffs_fsync", INFSLP);

            // Ensure that any filesystem metadata associated with the vnode has been
            // written.
            splx(s);
            // XXX softdep was here. reconsider this locking dance
            s = splbio();
            if !vp.v_dirtyblkhd.is_empty() {
                // Block devices associated with filesystems may have new I/O requests
                // posted for them even if the vnode is locked, so no amount of trying will
                // get them clean. Thus we give block devices a good effort, then just give
                // up. For all other file types, go around and try again until it is clean.
                if passes > 0 {
                    passes -= 1;
                    continue 'loop_;
                }
                #[cfg(feature = "diagnostic")]
                if vp.v_type.get() != crate::sys::vnode::VBLK {
                    crate::kern::vfs_subr::vprint(Some("ffs_fsync: dirty"), vp);
                }
            }
        }
        break;
    }
    splx(s);
    UFS_UPDATE(vtoi(vp), i32::from(ap.a_waitfor == MNT_WAIT))
}

/// `ffs_reclaim` (`vop_reclaim`): reclaim an inode so that it can be used for other
/// purposes.
pub fn ffs_reclaim(ap: &mut VopReclaimArgs<'_>) -> Result<(), Errno> {
    let vp = ap.a_vp;
    let ip = vtoi(vp);

    ufs_reclaim(vp)?;

    if let Some(din) = NonNull::new(ip.dinode_u.get().cast::<u8>()) {
        #[cfg(feature = "ffs2")]
        if ip.is_ufs2() {
            pool_put(&FFS_DINODE2_POOL, din);
        } else {
            pool_put(&FFS_DINODE1_POOL, din);
        }
        #[cfg(not(feature = "ffs2"))]
        pool_put(&FFS_DINODE1_POOL, din);
    }

    let ipp = NonNull::from(ip).cast::<u8>();
    vp.v_data.set(ptr::null_mut());
    pool_put(&FFS_INO_POOL, ipp);

    Ok(())
}
/* </CODE> */
