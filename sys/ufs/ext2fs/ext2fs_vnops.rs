/*	$OpenBSD: ext2fs_vnops.c,v 1.95 2025/06/01 00:32:54 rsadowski Exp $	*/
/*	$NetBSD: ext2fs_vnops.c,v 1.1 1997/06/11 09:34:09 bouyer Exp $	*/
/* <LICENSES> */
/*
 * Copyright (c) 1997 Manuel Bouyer.
 * Copyright (c) 1982, 1986, 1989, 1993
 *	The Regents of the University of California.  All rights reserved.
 * (c) UNIX System Laboratories, Inc.
 * All or some portions of this file are derived from material licensed
 * to the University of California by American Telephone and Telegraph
 * Co. or Unix System Laboratories, Inc. and are reproduced herein with
 * the permission of UNIX System Laboratories, Inc.
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
 *	@(#)ufs_vnops.c	8.14 (Berkeley) 10/26/94
 * Modified for ext2fs by Manuel Bouyer.
 */
/* </LICENSES> */

/* <CODE> */
//! The second extended file system's vnode operations: create, mknod, open, access,
//! attributes (with `ext2fs_chmod` and `ext2fs_chown`), remove, link, rename, mkdir, rmdir,
//! symlink, readlink, pathconf, advisory locks, `ext2fs_makeinode`, fsync and reclaim, and the
//! tables for files (`ext2fs_vops`) and for the special files that live on an ext2fs
//! (`ext2fs_specvops`). The directory operations they build on (`ext2fs_lookup`,
//! `ext2fs_readdir`, `ext2fs_direnter`, ...) are `ext2fs_lookup.rs`.
//!
//! Upstream: sys/ufs/ext2fs/ext2fs_vnops.c @ 3ce1f3f79392
//!
//! ## Deviations
//! - `option FIFO` is in GENERIC but `miscfs/fifofs` is not ported: `ext2fs_fifovops` and
//!   `ext2fsfifo_reclaim` come with it. Meanwhile `mknod`/`mkfifo` refuse fifos
//!   (`vfs_syscalls.rs`) and `ext2fs_vinit` refuses a fifo already on the disk
//!   (`ext2fs_subr.rs`), both with `EOPNOTSUPP`, as a kernel without `FIFO` does.
//! - The helpers the C installs in many slots (`vop_generic_badop`) are closures, as in
//!   `spec_vops` (`docs/C_TO_RUST.md`).
//! - `pool_put(&namei_pool, cnp->cn_pnbuf)` is `ufs_vnops.rs`'s [`pnbuf_free`]; credentials
//!   the C dereferences go through its `ucred`, which panics on `NOCRED`/`FSCRED`.
//! - `ext2fs_rename`'s `vfs_relookup` calls keep the references balanced when the lookup
//!   fails, as `ufs_rename` does: the C ignores the result of the two lookups of the source
//!   (then follows a NULL vnode, or releases `fdvp` once more than it holds), and lets
//!   `ext2fs_checkpath` consume the reference on `tdvp` that a failed lookup of the target
//!   releases again. Here `tdvp` gets `ufs_rename`'s compensating reference around
//!   `ext2fs_checkpath`; the source's lookup of the `fvp == tvp` case returns its error
//!   (`ENOENT` when the name went away); the final lookup of the source holds `fdvp` across
//!   the call and, when it fails, ends the rename as when the name has disappeared (0, as the
//!   C's ignored result gives).
//! - `ext2fs_setattr` keeps the C's precedence in the flags it sets: `SF_APPEND` sets
//!   `EXT2_APPEND` only, otherwise `SF_IMMUTABLE` sets `EXT2_IMMUTABLE`.

use core::ptr::{self, NonNull};
use core::sync::atomic::Ordering;

use crate::kern::kern_prot::{groupmember, suser_ucred};
use crate::kern::kern_subr::uiomove;
use crate::kern::kern_sysctl::SECURELEVEL;
use crate::kern::spec_vnops::{
    spec_advlock, spec_ioctl, spec_kqfilter, spec_open, spec_pathconf, spec_strategy,
};
use crate::kern::subr_pool::pool_put;
use crate::kern::subr_prf::panic;
use crate::kern::vfs_cache::cache_purge;
use crate::kern::vfs_default::{
    vop_generic_abortop, vop_generic_badop, vop_generic_bmap, vop_generic_bwrite,
    vop_generic_lookup, vop_generic_revoke,
};
use crate::kern::vfs_lookup::vfs_relookup;
use crate::kern::vfs_subr::{vaccess, vflushbuf, vgone, vput, vref, vrele};
use crate::kern::vfs_vnops::{vn_lock, vn_rdwr};
use crate::kern::vfs_vops::{VOP_ABORTOP, VOP_ACCESS, VOP_READ, VOP_REMOVE, VOP_UNLOCK};
use crate::machine::cpu::curproc;
use crate::sys::errno::Errno;
use crate::sys::fcntl::{FWRITE, O_APPEND};
use crate::sys::lock::LK_EXCLUSIVE;
use crate::sys::lockf::lf_advlock;
use crate::sys::mount::{MNT_NOATIME, MNT_RDONLY, MNT_WAIT, Mount};
#[cfg(feature = "diagnostic")]
use crate::sys::namei::HASBUF;
use crate::sys::namei::{
    Componentname, DELETE, ISDOTDOT, LOCKLEAF, LOCKPARENT, MODMASK, SAVESTART,
};
use crate::sys::param::{BLKDEV_IOSIZE, MAXBSIZE, dbtob};
use crate::sys::stat::{
    ACCESSPERMS, ALLPERMS, APPEND, IMMUTABLE, S_ISTXT, SF_APPEND, SF_IMMUTABLE,
};
use crate::sys::syslimits::LINK_MAX;
use crate::sys::time::Timespec;
use crate::sys::types::{Dev, Gid, Mode, Nlink, Off, Register, Uid};
use crate::sys::ucred::Ucred;
use crate::sys::uio::{UioRw, UioSeg};
use crate::sys::unistd::_PC_TIMESTAMP_RESOLUTION;
use crate::sys::vnode::{
    IO_NODELOCKED, IO_SYNC, VA_UTIMES_CHANGE, VA_UTIMES_NULL, VBLK, VCHR, VDIR, VLNK, VNON, VNOVAL,
    VREG, VTEXT, VWRITE, Vnode, VopAccessArgs, VopAdvlockArgs, VopCreateArgs, VopFsyncArgs,
    VopGetattrArgs, VopLinkArgs, VopMkdirArgs, VopMknodArgs, VopOpenArgs, VopPathconfArgs,
    VopReadlinkArgs, VopReclaimArgs, VopRemoveArgs, VopRenameArgs, VopRmdirArgs, VopSetattrArgs,
    VopSymlinkArgs, Vops, iftovt, makeimode,
};
use crate::ufs::ext2fs::ext2fs_alloc::ext2fs_inode_alloc;
use crate::ufs::ext2fs::ext2fs_bmap::ext2fs_bmap;
use crate::ufs::ext2fs::ext2fs_dinode::{EXT2_APPEND, EXT2_IMMUTABLE, EXT2_MAXSYMLINKLEN};
use crate::ufs::ext2fs::ext2fs_dir::{EXT2_FT_DIR, Ext2fsDirtemplate};
use crate::ufs::ext2fs::ext2fs_inode::{
    ext2fs_inactive, ext2fs_setsize, ext2fs_size, ext2fs_truncate, ext2fs_update,
};
use crate::ufs::ext2fs::ext2fs_lookup::{
    ext2fs_checkpath, ext2fs_dirempty, ext2fs_direnter, ext2fs_dirremove, ext2fs_dirrewrite,
    ext2fs_lookup, ext2fs_readdir, has_ftype,
};
use crate::ufs::ext2fs::ext2fs_readwrite::{ext2fs_read, ext2fs_write};
use crate::ufs::ext2fs::ext2fs_vfsops::{EXT2FS_DINODE_POOL, EXT2FS_INODE_POOL};
use crate::ufs::ufs::dinode::{IFDIR, IFLNK, IFMT, IFREG, ISGID, ISUID, Ufsino};
use crate::ufs::ufs::inode::{IN_ACCESS, IN_CHANGE, IN_RENAME, IN_UPDATE, Inode, vtoi};
use crate::ufs::ufs::ufs_ihash::ufs_ihashrem;
use crate::ufs::ufs::ufs_lookup::ufs_dirbad;
use crate::ufs::ufs::ufs_vnops::{
    pnbuf_free, ucred, ufs_close, ufs_ioctl, ufs_islocked, ufs_kqfilter, ufs_lock, ufs_pathconf,
    ufs_print, ufs_strategy, ufs_unlock, ufsspec_close, ufsspec_read, ufsspec_write,
};
use crate::uvm::uvm_vnode::uvm_vnp_uncache;

/// `ext2fs_vops`: the operations of a file, directory or symbolic link on an ext2fs.
pub static EXT2FS_VOPS: Vops = Vops {
    vop_lookup: Some(ext2fs_lookup),
    vop_create: Some(ext2fs_create),
    vop_mknod: Some(ext2fs_mknod),
    vop_open: Some(ext2fs_open),
    vop_close: Some(ufs_close),
    vop_access: Some(ext2fs_access),
    vop_getattr: Some(ext2fs_getattr),
    vop_setattr: Some(ext2fs_setattr),
    vop_read: Some(ext2fs_read),
    vop_write: Some(ext2fs_write),
    vop_ioctl: Some(ufs_ioctl),
    vop_kqfilter: Some(ufs_kqfilter),
    vop_revoke: None,
    vop_fsync: Some(ext2fs_fsync),
    vop_remove: Some(ext2fs_remove),
    vop_link: Some(ext2fs_link),
    vop_rename: Some(ext2fs_rename),
    vop_mkdir: Some(ext2fs_mkdir),
    vop_rmdir: Some(ext2fs_rmdir),
    vop_symlink: Some(ext2fs_symlink),
    vop_readdir: Some(ext2fs_readdir),
    vop_readlink: Some(ext2fs_readlink),
    vop_abortop: Some(vop_generic_abortop),
    vop_inactive: Some(ext2fs_inactive),
    vop_reclaim: Some(ext2fs_reclaim),
    vop_lock: Some(ufs_lock),
    vop_unlock: Some(ufs_unlock),
    vop_bmap: Some(ext2fs_bmap),
    vop_strategy: Some(ufs_strategy),
    vop_print: Some(ufs_print),
    vop_islocked: Some(ufs_islocked),
    vop_pathconf: Some(ext2fs_pathconf),
    vop_advlock: Some(ext2fs_advlock),
    vop_bwrite: Some(vop_generic_bwrite),
};

/// `ext2fs_specvops`: the operations of a device special file on an ext2fs.
pub static EXT2FS_SPECVOPS: Vops = Vops {
    vop_close: Some(ufsspec_close),
    vop_access: Some(ext2fs_access),
    vop_getattr: Some(ext2fs_getattr),
    vop_setattr: Some(ext2fs_setattr),
    vop_read: Some(ufsspec_read),
    vop_write: Some(ufsspec_write),
    vop_fsync: Some(ext2fs_fsync),
    vop_inactive: Some(ext2fs_inactive),
    vop_reclaim: Some(ext2fs_reclaim),
    vop_lock: Some(ufs_lock),
    vop_unlock: Some(ufs_unlock),
    vop_print: Some(ufs_print),
    vop_islocked: Some(ufs_islocked),

    // XXX: Keep in sync with spec_vops.
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

/// The vnode's mount, which an ext2fs vnode always has.
fn vmount(vp: &Vnode) -> &'static Mount {
    match vp.v_mount.get() {
        Some(mp) => mp,
        None => panic(format_args!("ext2fs: vnode {:p} without a mount", vp)),
    }
}

/// Whether the vnode's file system is mounted read-only.
fn rdonly(vp: &Vnode) -> bool {
    vmount(vp).mnt_flag.get() & MNT_RDONLY != 0
}

/// `ip->i_e2fs_nlink += d` (`d` is -1, 1 or -2).
fn nlink_add(ip: &Inode, d: i32) {
    ip.set_i_e2fs_nlink((i32::from(ip.i_e2fs_nlink()) + d) as u16);
}

/// `ip->i_e2fs_mode` as a `mode_t`.
fn e2mode(ip: &Inode) -> Mode {
    Mode::from(ip.i_e2fs_mode())
}

/// `ext2fs_create` (`vop_create`): create a regular file.
pub fn ext2fs_create(ap: &mut VopCreateArgs<'_>) -> Result<(), Errno> {
    ext2fs_makeinode(
        makeimode(ap.a_vap.va_type, ap.a_vap.va_mode),
        ap.a_dvp,
        ap.a_vpp,
        ap.a_cnp,
    )
}

/// `ext2fs_mknod` (`vop_mknod`): mknod vnode call.
pub fn ext2fs_mknod(ap: &mut VopMknodArgs<'_>) -> Result<(), Errno> {
    let vap = &*ap.a_vap;

    ext2fs_makeinode(
        makeimode(vap.va_type, vap.va_mode),
        ap.a_dvp,
        ap.a_vpp,
        ap.a_cnp,
    )?;
    let Some(vp) = *ap.a_vpp else {
        panic(format_args!("ext2fs_mknod: no vnode"));
    };
    let ip = vtoi(vp);
    ip.set_flag(IN_ACCESS | IN_CHANGE | IN_UPDATE);
    if vap.va_rdev != VNOVAL as Dev {
        // Want to be able to use this to make badblock inodes, so don't truncate the dev
        // number.
        ip.with_e2din(|d| d.set_e2di_rdev((vap.va_rdev as u32).to_le()));
    }
    // Remove inode so that it will be reloaded by VFS_VGET and checked to see if it is an
    // alias of an existing entry in the inode cache.
    vput(vp);
    vp.v_type.set(VNON);
    vgone(vp);
    vgone(vp);
    *ap.a_vpp = None;
    Ok(())
}

/// `ext2fs_open` (`vop_open`): open called. Just check the `APPEND` flag.
pub fn ext2fs_open(ap: &mut VopOpenArgs<'_>) -> Result<(), Errno> {
    // Files marked append-only must be opened for appending.
    if vtoi(ap.a_vp).i_e2fs_flags() & EXT2_APPEND != 0 && ap.a_mode & (FWRITE | O_APPEND) == FWRITE
    {
        return Err(Errno::EPERM);
    }
    Ok(())
}

/// `ext2fs_access` (`vop_access`).
pub fn ext2fs_access(ap: &mut VopAccessArgs<'_>) -> Result<(), Errno> {
    let vp = ap.a_vp;
    let ip = vtoi(vp);
    let mode = ap.a_mode;

    // If immutable bit set, nobody gets to write it.
    if mode & VWRITE != 0 && ip.i_e2fs_flags() & EXT2_IMMUTABLE != 0 {
        return Err(Errno::EPERM);
    }

    vaccess(
        vp.v_type.get(),
        e2mode(ip),
        ip.i_e2fs_uid().get(),
        ip.i_e2fs_gid().get(),
        mode,
        ucred(ap.a_cred),
    )
}

/// `ext2fs_getattr` (`vop_getattr`): copy from inode table.
pub fn ext2fs_getattr(ap: &mut VopGetattrArgs<'_>) -> Result<(), Errno> {
    let vp = ap.a_vp;
    let ip = vtoi(vp);
    let vap = &mut *ap.a_vap;

    ip.ext2fs_itimes();
    // Copy from inode table
    vap.va_fsid = i64::from(ip.i_dev.get());
    vap.va_fileid = u64::from(ip.i_number.get());
    vap.va_mode = e2mode(ip) & ALLPERMS;
    vap.va_nlink = Nlink::from(ip.i_e2fs_nlink());
    vap.va_uid = ip.i_e2fs_uid().get();
    vap.va_gid = ip.i_e2fs_gid().get();
    vap.va_rdev = u32::from_le(ip.with_e2din(|d| d.e2di_rdev())) as Dev;
    vap.va_size = ext2fs_size(ip);
    vap.va_atime = Timespec::new(i64::from(ip.i_e2fs_atime()), 0);
    vap.va_mtime = Timespec::new(i64::from(ip.i_e2fs_mtime()), 0);
    vap.va_ctime = Timespec::new(i64::from(ip.i_e2fs_ctime()), 0);
    vap.va_flags = if ip.i_e2fs_flags() & EXT2_APPEND != 0 {
        u64::from(SF_APPEND)
    } else {
        0
    };
    if ip.i_e2fs_flags() & EXT2_IMMUTABLE != 0 {
        vap.va_flags |= u64::from(SF_IMMUTABLE);
    }
    vap.va_gen = u64::from(ip.i_e2fs_gen());
    // this doesn't belong here
    vap.va_blocksize = match vp.v_type.get() {
        VBLK => BLKDEV_IOSIZE as i64,
        VCHR => MAXBSIZE as i64,
        _ => i64::from(vmount(vp).mnt_stat.get().f_iosize),
    };
    vap.va_bytes = dbtob(ip.i_e2fs_nblock() as usize) as u64;
    vap.va_type = vp.v_type.get();
    vap.va_filerev = ip.i_modrev.get();
    Ok(())
}

/// `ext2fs_setattr` (`vop_setattr`): set attribute vnode op. called from several syscalls.
pub fn ext2fs_setattr(ap: &mut VopSetattrArgs<'_>) -> Result<(), Errno> {
    let vap = &mut *ap.a_vap;
    let vp = ap.a_vp;
    let ip = vtoi(vp);
    let cred = ap.a_cred;

    // Check for unsettable attributes.
    if vap.va_type != VNON
        || vap.va_nlink != VNOVAL as Nlink
        || vap.va_fsid != i64::from(VNOVAL)
        || vap.va_fileid != VNOVAL as u64
        || vap.va_blocksize != i64::from(VNOVAL)
        || vap.va_rdev != VNOVAL as Dev
        || vap.va_bytes as i32 != VNOVAL
        || vap.va_gen != VNOVAL as u64
    {
        return Err(Errno::EINVAL);
    }
    if vap.va_flags != VNOVAL as u64 {
        if rdonly(vp) {
            return Err(Errno::EROFS);
        }
        let c = ucred(cred);
        if c.cr_uid.get() != ip.i_e2fs_uid().get() {
            suser_ucred(c)?;
        }
        if c.cr_uid.get() == 0 {
            if ip.i_e2fs_flags() & (EXT2_APPEND | EXT2_IMMUTABLE) != 0
                && SECURELEVEL.load(Ordering::Relaxed) > 0
            {
                return Err(Errno::EPERM);
            }
            let mut f = ip.i_e2fs_flags() & !(EXT2_APPEND | EXT2_IMMUTABLE);
            f |= if vap.va_flags & u64::from(SF_APPEND) != 0 {
                EXT2_APPEND
            } else if vap.va_flags & u64::from(SF_IMMUTABLE) != 0 {
                EXT2_IMMUTABLE
            } else {
                0
            };
            ip.set_i_e2fs_flags(f);
        } else {
            return Err(Errno::EPERM);
        }
        ip.set_flag(IN_CHANGE);
        if vap.va_flags & u64::from(IMMUTABLE | APPEND) != 0 {
            return Ok(());
        }
    }
    if ip.i_e2fs_flags() & (EXT2_APPEND | EXT2_IMMUTABLE) != 0 {
        return Err(Errno::EPERM);
    }
    // Go through the fields and update iff not VNOVAL.
    if vap.va_uid != VNOVAL as Uid || vap.va_gid != VNOVAL as Gid {
        if rdonly(vp) {
            return Err(Errno::EROFS);
        }
        ext2fs_chown(vp, vap.va_uid, vap.va_gid, cred)?;
    }
    if vap.va_size != VNOVAL as u64 {
        // Disallow write attempts on read-only file systems; unless the file is a socket,
        // fifo, or a block or character device resident on the file system.
        match vp.v_type.get() {
            VDIR => return Err(Errno::EISDIR),
            VLNK | VREG if rdonly(vp) => return Err(Errno::EROFS),
            _ => {}
        }
        ext2fs_truncate(ip, vap.va_size as Off, 0, cred)?;
    }
    if vap.va_vaflags & VA_UTIMES_CHANGE != 0
        || vap.va_atime.tv_nsec != i64::from(VNOVAL)
        || vap.va_mtime.tv_nsec != i64::from(VNOVAL)
    {
        if rdonly(vp) {
            return Err(Errno::EROFS);
        }
        let c = ucred(cred);
        if c.cr_uid.get() != ip.i_e2fs_uid().get()
            && let Err(e) = suser_ucred(c)
        {
            if vap.va_vaflags & VA_UTIMES_NULL == 0 {
                return Err(e);
            }
            VOP_ACCESS(vp, VWRITE, cred, ap.a_p)?;
        }
        if vap.va_mtime.tv_nsec != i64::from(VNOVAL) {
            ip.set_flag(IN_CHANGE | IN_UPDATE);
        } else if vap.va_vaflags & VA_UTIMES_CHANGE != 0 {
            ip.set_flag(IN_CHANGE);
        }
        if vap.va_atime.tv_nsec != i64::from(VNOVAL)
            && (vmount(vp).mnt_flag.get() & MNT_NOATIME == 0
                || ip.i_flag.get() & (IN_CHANGE | IN_UPDATE) != 0)
        {
            ip.set_flag(IN_ACCESS);
        }
        ip.ext2fs_itimes();
        if vap.va_mtime.tv_nsec != i64::from(VNOVAL) {
            ip.set_i_e2fs_mtime(vap.va_mtime.tv_sec as u32);
        }
        if vap.va_atime.tv_nsec != i64::from(VNOVAL) {
            ip.set_i_e2fs_atime(vap.va_atime.tv_sec as u32);
        }
        ext2fs_update(ip, 1)?;
    }
    if vap.va_mode != VNOVAL as Mode {
        if rdonly(vp) {
            return Err(Errno::EROFS);
        }
        return ext2fs_chmod(vp, vap.va_mode, cred);
    }
    Ok(())
}

/// `ext2fs_chmod`: change the mode on a file. Inode must be locked before calling.
fn ext2fs_chmod(vp: &'static Vnode, mode: Mode, cred: *const Ucred) -> Result<(), Errno> {
    let ip = vtoi(vp);
    let c = ucred(cred);

    if c.cr_uid.get() != ip.i_e2fs_uid().get() {
        suser_ucred(c)?;
    }
    if c.cr_uid.get() != 0 {
        if vp.v_type.get() != VDIR && mode & S_ISTXT != 0 {
            return Err(Errno::EFTYPE);
        }
        if !groupmember(ip.i_e2fs_gid().get(), c) && mode & ISGID != 0 {
            return Err(Errno::EPERM);
        }
    }
    let m = (e2mode(ip) & !ALLPERMS) | (mode & ALLPERMS);
    ip.set_i_e2fs_mode(m as u16);
    ip.set_flag(IN_CHANGE);
    if vp.v_flag.get() & VTEXT != 0 && e2mode(ip) & S_ISTXT == 0 {
        let _ = uvm_vnp_uncache(vp);
    }
    Ok(())
}

/// `ext2fs_chown`: perform chown operation on inode ip; inode must be locked prior to call.
fn ext2fs_chown(vp: &'static Vnode, uid: Uid, gid: Gid, cred: *const Ucred) -> Result<(), Errno> {
    let ip = vtoi(vp);
    let c = ucred(cred);

    let uid = if uid == VNOVAL as Uid {
        ip.i_e2fs_uid().get()
    } else {
        uid
    };
    let gid = if gid == VNOVAL as Gid {
        ip.i_e2fs_gid().get()
    } else {
        gid
    };
    // If we don't own the file, are trying to change the owner of the file, or are not a
    // member of the target group, the caller must be superuser or the call fails.
    if c.cr_uid.get() != ip.i_e2fs_uid().get()
        || uid != ip.i_e2fs_uid().get()
        || (gid != ip.i_e2fs_gid().get() && !groupmember(gid, c))
    {
        suser_ucred(c)?;
    }
    let ogid = ip.i_e2fs_gid().get();
    let ouid = ip.i_e2fs_uid().get();

    ip.i_e2fs_gid().set(gid);
    ip.i_e2fs_uid().set(uid);
    if ouid != uid || ogid != gid {
        ip.set_flag(IN_CHANGE);
    }
    if ouid != uid && c.cr_uid.get() != 0 {
        ip.set_i_e2fs_mode((e2mode(ip) & !ISUID) as u16);
    }
    if ogid != gid && c.cr_uid.get() != 0 {
        ip.set_i_e2fs_mode((e2mode(ip) & !ISGID) as u16);
    }
    Ok(())
}

/// `ext2fs_remove` (`vop_remove`): remove the entry of a file that is not a directory.
pub fn ext2fs_remove(ap: &mut VopRemoveArgs<'_>) -> Result<(), Errno> {
    let vp = ap.a_vp;
    let dvp = ap.a_dvp;

    let ip = vtoi(vp);
    if vp.v_type.get() == VDIR
        || ip.i_e2fs_flags() & (EXT2_IMMUTABLE | EXT2_APPEND) != 0
        || vtoi(dvp).i_e2fs_flags() & EXT2_APPEND != 0
    {
        return Err(Errno::EPERM);
    }
    let error = ext2fs_dirremove(dvp, ap.a_cnp);
    if error.is_ok() {
        nlink_add(ip, -1);
        ip.set_flag(IN_CHANGE);
    }
    error
}

/// `ext2fs_link` (`vop_link`): link vnode call.
pub fn ext2fs_link(ap: &mut VopLinkArgs<'_>) -> Result<(), Errno> {
    let dvp = ap.a_dvp;
    let vp = ap.a_vp;
    let cnp = &mut *ap.a_cnp;

    #[cfg(feature = "diagnostic")]
    if cnp.cn_flags & HASBUF == 0 {
        panic(format_args!("ext2fs_link: no name"));
    }
    let error: Result<(), Errno> = 'out2: {
        if !ptr::eq(dvp, vp)
            && let Err(e) = vn_lock(vp, LK_EXCLUSIVE)
        {
            let _ = VOP_ABORTOP(dvp, cnp);
            break 'out2 Err(e);
        }
        let ip = vtoi(vp);
        let error: Result<(), Errno> = 'out1: {
            if Nlink::from(ip.i_e2fs_nlink()) >= LINK_MAX {
                let _ = VOP_ABORTOP(dvp, cnp);
                break 'out1 Err(Errno::EMLINK);
            }
            if ip.i_e2fs_flags() & (EXT2_IMMUTABLE | EXT2_APPEND) != 0 {
                let _ = VOP_ABORTOP(dvp, cnp);
                break 'out1 Err(Errno::EPERM);
            }
            nlink_add(ip, 1);
            ip.set_flag(IN_CHANGE);
            let mut error = ext2fs_update(ip, 1);
            if error.is_ok() {
                error = ext2fs_direnter(ip, dvp, cnp);
            }
            if error.is_err() {
                nlink_add(ip, -1);
                ip.set_flag(IN_CHANGE);
            }
            pnbuf_free(cnp);
            error
        };
        // out1:
        if !ptr::eq(dvp, vp) {
            let _ = VOP_UNLOCK(vp);
        }
        error
    };
    // out2:
    vput(dvp);
    error
}

/// `abortit:` of `ext2fs_rename`: abort both lookups and release every vnode.
#[allow(clippy::too_many_arguments)] // the C label's state
fn rename_abortit(
    error: Errno,
    tdvp: &'static Vnode,
    tvp: Option<&'static Vnode>,
    tcnp: &mut Componentname,
    fdvp: &'static Vnode,
    fvp: &'static Vnode,
    fcnp: &mut Componentname,
) -> Result<(), Errno> {
    let _ = VOP_ABORTOP(tdvp, tcnp); // XXX, why not in NFS?
    if tvp.is_some_and(|t| ptr::eq(t, tdvp)) {
        vrele(tdvp);
    } else {
        vput(tdvp);
    }
    if let Some(tvp) = tvp {
        vput(tvp);
    }
    let _ = VOP_ABORTOP(fdvp, fcnp); // XXX, why not in NFS?
    vrele(fdvp);
    vrele(fvp);
    Err(error)
}

/// How `ext2fs_rename` leaves: the C's `bad:` and `out:` labels.
enum RenameExit {
    /// `goto bad`: release the target directory (and target) first.
    Bad(Errno),
    /// `goto out`.
    Out(Errno),
}

/// `ext2fs_rename` (`vop_rename`): rename system call.
///
/// `rename("foo", "bar");` is essentially `unlink("bar"); link("foo", "bar");
/// unlink("foo");` but "atomically". Can't do full commit without saving state in the inode
/// on disk which isn't feasible at this time. Best we can do is always guarantee the target
/// exists.
///
/// Basic algorithm is:
///
/// 1. Bump link count on source while we're linking it to the target. This also ensure the
///    inode won't be deleted out from underneath us while we work (it may be truncated by a
///    concurrent `trunc` or `open` for creation).
/// 2. Link source to destination. If destination already exists, delete it first.
/// 3. Unlink source reference to inode if still around. If a directory was moved and the
///    parent of the destination is different from the source, patch the ".." entry in the
///    directory.
pub fn ext2fs_rename(ap: &mut VopRenameArgs<'_>) -> Result<(), Errno> {
    let mut tvp = ap.a_tvp;
    let tdvp = ap.a_tdvp;
    let fvp = ap.a_fvp;
    let fdvp = ap.a_fdvp;
    let tcnp = &mut *ap.a_tcnp;
    let fcnp = &mut *ap.a_fcnp;
    let mut doingdirectory = false;
    let mut oldparent: Ufsino = 0;
    let mut newparent: Ufsino = 0;

    #[cfg(feature = "diagnostic")]
    if tcnp.cn_flags & HASBUF == 0 || fcnp.cn_flags & HASBUF == 0 {
        panic(format_args!("ext2fs_rename: no name"));
    }
    // Check for cross-device rename.
    let fmp = fvp.v_mount.get().map(ptr::from_ref);
    if fmp != tdvp.v_mount.get().map(ptr::from_ref)
        || tvp.is_some_and(|t| fmp != t.v_mount.get().map(ptr::from_ref))
    {
        return rename_abortit(Errno::EXDEV, tdvp, tvp, tcnp, fdvp, fvp, fcnp);
    }

    // Check if just deleting a link name.
    if let Some(t) = tvp
        && (vtoi(t).i_e2fs_flags() & (EXT2_IMMUTABLE | EXT2_APPEND) != 0
            || vtoi(tdvp).i_e2fs_flags() & EXT2_APPEND != 0)
    {
        return rename_abortit(Errno::EPERM, tdvp, tvp, tcnp, fdvp, fvp, fcnp);
    }
    if let Some(t) = tvp
        && ptr::eq(fvp, t)
    {
        if fvp.v_type.get() == VDIR {
            return rename_abortit(Errno::EINVAL, tdvp, tvp, tcnp, fdvp, fvp, fcnp);
        }

        // Release destination completely.
        let _ = VOP_ABORTOP(tdvp, tcnp);
        vput(tdvp);
        vput(t);

        // Delete source.
        vrele(fvp);
        fcnp.cn_flags &= !MODMASK;
        fcnp.cn_flags |= LOCKPARENT | LOCKLEAF;
        if fcnp.cn_flags & SAVESTART == 0 {
            panic(format_args!("ext2fs_rename: lost from startdir"));
        }
        fcnp.cn_nameiop = DELETE;
        let mut nfvp = None;
        vfs_relookup(fdvp, &mut nfvp, fcnp)?; // relookup did vrele()
        vrele(fdvp);
        let Some(nfvp) = nfvp else {
            return Err(Errno::ENOENT);
        };
        return VOP_REMOVE(fdvp, nfvp, fcnp);
    }
    if let Err(e) = vn_lock(fvp, LK_EXCLUSIVE) {
        return rename_abortit(e, tdvp, tvp, tcnp, fdvp, fvp, fcnp);
    }
    let mut dp = vtoi(fdvp);
    let ip = vtoi(fvp);
    if Nlink::from(ip.i_e2fs_nlink()) >= LINK_MAX {
        let _ = VOP_UNLOCK(fvp);
        return rename_abortit(Errno::EMLINK, tdvp, tvp, tcnp, fdvp, fvp, fcnp);
    }
    if ip.i_e2fs_flags() & (EXT2_IMMUTABLE | EXT2_APPEND) != 0
        || dp.i_e2fs_flags() & EXT2_APPEND != 0
    {
        let _ = VOP_UNLOCK(fvp);
        return rename_abortit(Errno::EPERM, tdvp, tvp, tcnp, fdvp, fvp, fcnp);
    }
    if e2mode(ip) & IFMT == IFDIR {
        let mut error = VOP_ACCESS(fvp, VWRITE, tcnp.cn_cred, tcnp.proc());
        if error.is_ok()
            && let Some(t) = tvp
        {
            error = VOP_ACCESS(t, VWRITE, tcnp.cn_cred, tcnp.proc());
        }
        if error.is_err() {
            let _ = VOP_UNLOCK(fvp);
            return rename_abortit(Errno::EACCES, tdvp, tvp, tcnp, fdvp, fvp, fcnp);
        }
        // Avoid ".", "..", and aliases of "." for obvious reasons.
        if (fcnp.cn_namelen == 1 && fcnp.name() == b".")
            || ptr::eq(dp, ip)
            || fcnp.cn_flags & ISDOTDOT != 0
            || tcnp.cn_flags & ISDOTDOT != 0
            || ip.i_flag.get() & IN_RENAME != 0
        {
            let _ = VOP_UNLOCK(fvp);
            return rename_abortit(Errno::EINVAL, tdvp, tvp, tcnp, fdvp, fvp, fcnp);
        }
        ip.set_flag(IN_RENAME);
        oldparent = dp.i_number.get();
        doingdirectory = true;
    }
    vrele(fdvp);

    // When the target exists, both the directory and target vnodes are returned locked.
    dp = vtoi(tdvp);
    let mut xp: Option<&Inode> = tvp.map(vtoi);
    let mut error: Result<(), Errno> = Ok(());

    let exit: Result<(), RenameExit> = 'body: {
        // 1) Bump link count while we're moving stuff around. If we crash somewhere before
        //    completing our work, the link count may be wrong, but correctable.
        nlink_add(ip, 1);
        ip.set_flag(IN_CHANGE);
        if let Err(e) = ext2fs_update(ip, 1) {
            let _ = VOP_UNLOCK(fvp);
            break 'body Err(RenameExit::Bad(e));
        }

        // If ".." must be changed (ie the directory gets a new parent) then the source
        // directory must not be in the directory hierarchy above the target, as this would
        // orphan everything below the source directory. Also the user must have write
        // permission in the source so as to be able to change "..". We must repeat the call
        // to namei, as the parent directory is unlocked by the call to checkpath().
        let access = VOP_ACCESS(fvp, VWRITE, tcnp.cn_cred, tcnp.proc());
        let _ = VOP_UNLOCK(fvp);
        if oldparent != dp.i_number.get() {
            newparent = dp.i_number.get();
        }
        if doingdirectory && newparent != 0 {
            if let Err(e) = access {
                // write access check above
                break 'body Err(RenameExit::Bad(e));
            }
            if xp.is_some()
                && let Some(t) = tvp
            {
                vput(t);
            }
            // Compensate for the reference ext2fs_checkpath() loses (the module's
            // deviations).
            vref(tdvp);
            if let Err(e) = ext2fs_checkpath(ip, dp, tcnp.cn_cred) {
                vrele(tdvp);
                break 'body Err(RenameExit::Out(e));
            }
            if tcnp.cn_flags & SAVESTART == 0 {
                panic(format_args!("ext2fs_rename: lost to startdir"));
            }
            let mut ntvp = None;
            if let Err(e) = vfs_relookup(tdvp, &mut ntvp, tcnp) {
                break 'body Err(RenameExit::Out(e));
            }
            tvp = ntvp;
            vrele(tdvp); // relookup() acquired a reference
            dp = vtoi(tdvp);
            xp = tvp.map(vtoi);
        }
        // 2) If target doesn't exist, link the target to the source and unlink the source.
        //    Otherwise, rewrite the target directory entry to reference the source inode and
        //    expunge the original entry's existence.
        match xp {
            None => {
                if dp.i_dev.get() != ip.i_dev.get() {
                    panic(format_args!("rename: EXDEV"));
                }
                // Account for ".." in new directory. When source and destination have the
                // same parent we don't fool with the link count.
                if doingdirectory && newparent != 0 {
                    if Nlink::from(dp.i_e2fs_nlink()) >= LINK_MAX {
                        break 'body Err(RenameExit::Bad(Errno::EMLINK));
                    }
                    nlink_add(dp, 1);
                    dp.set_flag(IN_CHANGE);
                    if let Err(e) = ext2fs_update(dp, 1) {
                        break 'body Err(RenameExit::Bad(e));
                    }
                }
                if let Err(e) = ext2fs_direnter(ip, tdvp, tcnp) {
                    if doingdirectory && newparent != 0 {
                        nlink_add(dp, -1);
                        dp.set_flag(IN_CHANGE);
                        let _ = ext2fs_update(dp, 1);
                    }
                    break 'body Err(RenameExit::Bad(e));
                }
                vput(tdvp);
            }
            Some(x) => {
                if x.i_dev.get() != dp.i_dev.get() || x.i_dev.get() != ip.i_dev.get() {
                    panic(format_args!("rename: EXDEV"));
                }
                // Short circuit rename(foo, foo).
                if x.i_number.get() == ip.i_number.get() {
                    panic(format_args!("rename: same file"));
                }
                // If the parent directory is "sticky", then the user must own the parent
                // directory, or the destination of the rename, otherwise the destination may
                // not be changed (except by root). This implements append-only directories.
                let tuid = ucred(tcnp.cn_cred).cr_uid.get();
                if e2mode(dp) & S_ISTXT != 0
                    && tuid != 0
                    && tuid != dp.i_e2fs_uid().get()
                    && x.i_e2fs_uid().get() != tuid
                {
                    break 'body Err(RenameExit::Bad(Errno::EPERM));
                }
                // Target must be empty if a directory and have no links to it. Also, ensure
                // source and target are compatible (both directories, or both not
                // directories).
                if e2mode(x) & IFMT == IFDIR {
                    if !ext2fs_dirempty(x, dp.i_number.get(), tcnp.cn_cred) || x.i_e2fs_nlink() > 2
                    {
                        break 'body Err(RenameExit::Bad(Errno::ENOTEMPTY));
                    }
                    if !doingdirectory {
                        break 'body Err(RenameExit::Bad(Errno::ENOTDIR));
                    }
                    cache_purge(tdvp);
                } else if doingdirectory {
                    break 'body Err(RenameExit::Bad(Errno::EISDIR));
                }
                if let Err(e) = ext2fs_dirrewrite(dp, ip, tcnp) {
                    break 'body Err(RenameExit::Bad(e));
                }
                // If the target directory is in the same directory as the source directory,
                // decrement the link count on the parent of the target directory.
                if doingdirectory && newparent == 0 {
                    nlink_add(dp, -1);
                    dp.set_flag(IN_CHANGE);
                }
                vput(tdvp);
                // Adjust the link count of the target to reflect the dirrewrite above. If
                // this is a directory it is empty and there are no links to it, so we can
                // squash the inode and any space associated with it. We disallowed renaming
                // over top of a directory with links to it above, as the remaining link
                // would point to a directory without "." or ".." entries.
                nlink_add(x, -1);
                if doingdirectory {
                    nlink_add(x, -1);
                    if x.i_e2fs_nlink() != 0 {
                        panic(format_args!("rename: linked directory"));
                    }
                    error = ext2fs_truncate(x, 0, IO_SYNC, tcnp.cn_cred);
                }
                x.set_flag(IN_CHANGE);
                if let Some(t) = tvp {
                    vput(t);
                }
                xp = None;
            }
        }
        Ok(())
    };

    match exit {
        Ok(()) => {}
        Err(RenameExit::Bad(e)) => {
            if let Some(x) = xp {
                vput(x.itov());
            }
            vput(dp.itov());
            return rename_out(e, fvp, ip, doingdirectory);
        }
        Err(RenameExit::Out(e)) => return rename_out(e, fvp, ip, doingdirectory),
    }

    // 3) Unlink the source.
    fcnp.cn_flags &= !MODMASK;
    fcnp.cn_flags |= LOCKPARENT | LOCKLEAF;
    if fcnp.cn_flags & SAVESTART == 0 {
        panic(format_args!("ext2fs_rename: lost from startdir"));
    }
    let mut nfvp = None;
    // Held across the lookup, which releases it when it fails (the module's deviations).
    vref(fdvp);
    if vfs_relookup(fdvp, &mut nfvp, fcnp).is_ok() {
        vrele(fdvp);
    }
    let Some(nfvp) = nfvp else {
        // From name has disappeared.
        if doingdirectory {
            panic(format_args!("ext2fs_rename: lost dir entry"));
        }
        vrele(ap.a_fvp);
        return Ok(());
    };
    let x = vtoi(nfvp);
    let dp = vtoi(fdvp);

    // Ensure that the directory entry still exists and has not changed while the new name
    // has been entered. If the source is a file then the entry may have been unlinked or
    // renamed. In either case there is no further work to be done. If the source is a
    // directory then it cannot have been rmdir'ed; its link count of three would cause a
    // rmdir to fail with ENOTEMPTY. The IRENAME flag ensures that it cannot be moved by
    // another rename.
    if !ptr::eq(x, ip) {
        if doingdirectory {
            panic(format_args!("ext2fs_rename: lost dir entry"));
        }
    } else {
        // If the source is a directory with a new parent, the link count of the old parent
        // directory must be decremented and ".." set to point to the new parent.
        if doingdirectory && newparent != 0 {
            nlink_add(dp, -1);
            dp.set_flag(IN_CHANGE);
            let mut dirbuf = [0u8; Ext2fsDirtemplate::SIZE];
            let read = vn_rdwr(
                UioRw::UIO_READ,
                nfvp,
                dirbuf.as_mut_ptr().cast(),
                Ext2fsDirtemplate::SIZE,
                0,
                UioSeg::UIO_SYSSPACE,
                IO_NODELOCKED,
                tcnp.cn_cred,
                None,
                curproc(),
            );
            if read.is_ok() {
                let mut t = Ext2fsDirtemplate::from_le_bytes(&dirbuf);
                if t.dotdot_namlen != 2 || t.dotdot_name[0] != b'.' || t.dotdot_name[1] != b'.' {
                    ufs_dirbad(x, 12, "ext2fs_rename: mangled dir");
                } else {
                    t.dotdot_ino = newparent;
                    let mut out = t.to_le_bytes();
                    let _ = vn_rdwr(
                        UioRw::UIO_WRITE,
                        nfvp,
                        out.as_mut_ptr().cast(),
                        Ext2fsDirtemplate::SIZE,
                        0,
                        UioSeg::UIO_SYSSPACE,
                        IO_NODELOCKED | IO_SYNC,
                        tcnp.cn_cred,
                        None,
                        curproc(),
                    );
                    cache_purge(fdvp);
                }
            }
        }
        error = ext2fs_dirremove(fdvp, fcnp);
        if error.is_ok() {
            nlink_add(x, -1);
            x.set_flag(IN_CHANGE);
        }
        x.clr_flag(IN_RENAME);
    }
    vput(fdvp);
    vput(nfvp);
    vrele(ap.a_fvp);
    error
}

/// `out:` of `ext2fs_rename`: undo the link count bump on the source.
fn rename_out(
    error: Errno,
    fvp: &'static Vnode,
    ip: &Inode,
    doingdirectory: bool,
) -> Result<(), Errno> {
    if doingdirectory {
        ip.clr_flag(IN_RENAME);
    }
    if vn_lock(fvp, LK_EXCLUSIVE).is_ok() {
        nlink_add(ip, -1);
        ip.set_flag(IN_CHANGE);
        vput(fvp);
    } else {
        vrele(fvp);
    }
    Err(error)
}

/// `ext2fs_mkdir` (`vop_mkdir`): mkdir system call.
pub fn ext2fs_mkdir(ap: &mut VopMkdirArgs<'_>) -> Result<(), Errno> {
    let dvp = ap.a_dvp;
    let vap = &*ap.a_vap;
    let cnp = &mut *ap.a_cnp;

    #[cfg(feature = "diagnostic")]
    if cnp.cn_flags & HASBUF == 0 {
        panic(format_args!("ext2fs_mkdir: no name"));
    }
    let dp = vtoi(dvp);
    let error: Result<(), Errno> = 'out: {
        if Nlink::from(dp.i_e2fs_nlink()) >= LINK_MAX {
            break 'out Err(Errno::EMLINK);
        }
        let dmode = (vap.va_mode & ACCESSPERMS) | IFDIR;
        // Must simulate part of ext2fs_makeinode here to acquire the inode, but not have it
        // entered in the parent directory. The entry is made later after writing "." and ".."
        // entries.
        let tvp = match ext2fs_inode_alloc(dp, dmode, cnp.cn_cred) {
            Ok(tvp) => tvp,
            Err(e) => break 'out Err(e),
        };
        let ip = vtoi(tvp);
        ip.i_e2fs_uid().set(ucred(cnp.cn_cred).cr_uid.get());
        ip.i_e2fs_gid().set(dp.i_e2fs_gid().get());
        ip.set_flag(IN_ACCESS | IN_CHANGE | IN_UPDATE);
        ip.set_i_e2fs_mode(dmode as u16);
        tvp.v_type.set(VDIR); // Rest init'd in getnewvnode().
        ip.set_i_e2fs_nlink(2);
        // The C stores this result and overwrites it at once with the parent's.
        let _ = ext2fs_update(ip, 1);

        let error: Result<(), Errno> = 'bad: {
            // Bump link count in parent directory to reflect work done below. Should be done
            // before reference is created so reparation is possible if we crash.
            nlink_add(dp, 1);
            dp.set_flag(IN_CHANGE);
            if let Err(e) = ext2fs_update(dp, 1) {
                break 'bad Err(e);
            }

            // Initialize directory with "." and ".." from static template.
            let ftype = has_ftype(ip);
            let dirtemplate = Ext2fsDirtemplate {
                dot_ino: ip.i_number.get(),
                dot_reclen: 12,
                dot_namlen: 1,
                dot_type: if ftype { EXT2_FT_DIR } else { 0 },
                dot_name: *b".\0\0\0",
                dotdot_ino: dp.i_number.get(),
                dotdot_reclen: (dp.e2fs().e2fs_bsize.get() - 12) as i16,
                dotdot_namlen: 2,
                dotdot_type: if ftype { EXT2_FT_DIR } else { 0 },
                dotdot_name: *b"..\0\0",
            };
            let mut tb = dirtemplate.to_le_bytes();
            if let Err(e) = vn_rdwr(
                UioRw::UIO_WRITE,
                tvp,
                tb.as_mut_ptr().cast(),
                Ext2fsDirtemplate::SIZE,
                0,
                UioSeg::UIO_SYSSPACE,
                IO_NODELOCKED | IO_SYNC,
                cnp.cn_cred,
                None,
                curproc(),
            ) {
                nlink_add(dp, -1);
                dp.set_flag(IN_CHANGE);
                break 'bad Err(e);
            }
            let bsize = dp.e2fs().e2fs_bsize.get();
            if i64::from(bsize) > vmount(dvp).mnt_stat.get().f_bsize as i64 {
                // XXX should grow with balloc()
                panic(format_args!("ext2fs_mkdir: blksize"));
            } else {
                if let Err(e) = ext2fs_setsize(ip, bsize as u64) {
                    nlink_add(dp, -1);
                    dp.set_flag(IN_CHANGE);
                    break 'bad Err(e);
                }
                ip.set_flag(IN_CHANGE);
            }

            // Directory set up, now install its entry in the parent directory.
            let error = ext2fs_direnter(ip, dvp, cnp);
            if error.is_err() {
                nlink_add(dp, -1);
                dp.set_flag(IN_CHANGE);
            }
            error
        };
        // bad:
        // No need to do an explicit VOP_TRUNCATE here, vrele will do this for us because we
        // set the link count to 0.
        if error.is_err() {
            ip.set_i_e2fs_nlink(0);
            ip.set_flag(IN_CHANGE);
            vput(tvp);
        } else {
            *ap.a_vpp = Some(tvp);
        }
        error
    };
    // out:
    pnbuf_free(cnp);
    vput(dvp);
    error
}

/// `ext2fs_rmdir` (`vop_rmdir`): rmdir system call.
pub fn ext2fs_rmdir(ap: &mut VopRmdirArgs<'_>) -> Result<(), Errno> {
    let vp = ap.a_vp;
    let mut dvp = Some(ap.a_dvp);
    let cnp = &mut *ap.a_cnp;

    let ip = vtoi(vp);
    let dp = vtoi(ap.a_dvp);
    // Verify the directory is empty (and valid). (Rmdir ".." won't be valid since ".." will
    // contain a reference to the current directory and thus be non-empty.)
    let error: Result<(), Errno> = 'out: {
        if ip.i_e2fs_nlink() != 2 || !ext2fs_dirempty(ip, dp.i_number.get(), cnp.cn_cred) {
            break 'out Err(Errno::ENOTEMPTY);
        }
        if dp.i_e2fs_flags() & EXT2_APPEND != 0
            || ip.i_e2fs_flags() & (EXT2_IMMUTABLE | EXT2_APPEND) != 0
        {
            break 'out Err(Errno::EPERM);
        }
        // Delete reference to directory before purging inode. If we crash in between, the
        // directory will be reattached to lost+found,
        if let Err(e) = ext2fs_dirremove(ap.a_dvp, cnp) {
            break 'out Err(e);
        }
        nlink_add(dp, -1);
        dp.set_flag(IN_CHANGE);
        cache_purge(ap.a_dvp);
        vput(ap.a_dvp);
        dvp = None;
        // Truncate inode. The only stuff left in the directory is "." and "..". The "."
        // reference is inconsequential since we're quashing it. The ".." reference has
        // already been adjusted above. We've removed the "." reference and the reference in
        // the parent directory, but there may be other hard links so decrement by 2 and
        // worry about them later.
        nlink_add(ip, -2);
        let error = ext2fs_truncate(ip, 0, IO_SYNC, cnp.cn_cred);
        cache_purge(ip.itov());
        error
    };
    // out:
    if let Some(dvp) = dvp {
        vput(dvp);
    }
    vput(vp);
    error
}

/// `ext2fs_symlink` (`vop_symlink`): symlink -- make a symbolic link.
pub fn ext2fs_symlink(ap: &mut VopSymlinkArgs<'_>) -> Result<(), Errno> {
    let vpp = &mut *ap.a_vpp;

    let error = ext2fs_makeinode(IFLNK | ap.a_vap.va_mode, ap.a_dvp, vpp, ap.a_cnp);
    vput(ap.a_dvp);
    error?;
    let Some(vp) = *vpp else {
        panic(format_args!("ext2fs_symlink: no vnode"));
    };
    let target = ap.a_target;
    let len = target.len();
    let error = if len < EXT2_MAXSYMLINKLEN {
        let ip = vtoi(vp);
        ip.with_e2din(|d| d.e2di_shortlink_mut()[..len].copy_from_slice(target));
        ext2fs_setsize(ip, len as u64).map(|()| ip.set_flag(IN_CHANGE | IN_UPDATE))
    } else {
        vn_rdwr(
            UioRw::UIO_WRITE,
            vp,
            target.as_ptr().cast_mut().cast(),
            len,
            0,
            UioSeg::UIO_SYSSPACE,
            IO_NODELOCKED,
            ap.a_cnp.cn_cred,
            None,
            curproc(),
        )
    };
    // bad:
    vput(vp);
    error
}

/// `ext2fs_readlink` (`vop_readlink`): return target name of a symbolic link.
pub fn ext2fs_readlink(ap: &mut VopReadlinkArgs<'_, '_>) -> Result<(), Errno> {
    let vp = ap.a_vp;
    let ip = vtoi(vp);

    let isize = ext2fs_size(ip);
    if isize < EXT2_MAXSYMLINKLEN as u64 {
        let mut link = [0u8; EXT2_MAXSYMLINKLEN];
        let n = isize as usize;
        ip.with_e2din(|d| link[..n].copy_from_slice(&d.e2di_shortlink()[..n]));
        return uiomove(&mut link[..n], ap.a_uio);
    }
    VOP_READ(vp, ap.a_uio, 0, ap.a_cred)
}

/// `ext2fs_pathconf` (`vop_pathconf`): return POSIX pathconf information applicable to ext2
/// filesystems.
pub fn ext2fs_pathconf(ap: &mut VopPathconfArgs<'_>) -> Result<(), Errno> {
    match ap.a_name {
        _PC_TIMESTAMP_RESOLUTION => {
            *ap.a_retval = 1_000_000_000 as Register; // 1 billion nanoseconds
            Ok(())
        }
        _ => ufs_pathconf(ap),
    }
}

/// `ext2fs_advlock` (`vop_advlock`): advisory record locking support.
pub fn ext2fs_advlock(ap: &mut VopAdvlockArgs<'_>) -> Result<(), Errno> {
    let ip = vtoi(ap.a_vp);

    lf_advlock(
        &ip.i_lockf,
        ext2fs_size(ip) as Off,
        ap.a_id,
        ap.a_op,
        ap.a_fl,
        ap.a_flags,
    )
}

/// `ext2fs_makeinode`: allocate a new inode of type `mode` and enter it in the directory
/// `dvp` under the name in `cnp`; `*vpp` is its vnode, referenced and locked.
pub fn ext2fs_makeinode(
    mut mode: Mode,
    dvp: &'static Vnode,
    vpp: &mut Option<&'static Vnode>,
    cnp: &mut Componentname,
) -> Result<(), Errno> {
    let pdir = vtoi(dvp);
    #[cfg(feature = "diagnostic")]
    if cnp.cn_flags & HASBUF == 0 {
        panic(format_args!("ext2fs_makeinode: no name"));
    }
    *vpp = None;
    if mode & IFMT == 0 {
        mode |= IFREG;
    }

    let tvp = match ext2fs_inode_alloc(pdir, mode, cnp.cn_cred) {
        Ok(tvp) => tvp,
        Err(e) => {
            pnbuf_free(cnp);
            return Err(e);
        }
    };
    let ip = vtoi(tvp);
    let c = ucred(cnp.cn_cred);
    ip.i_e2fs_gid().set(pdir.i_e2fs_gid().get());
    ip.i_e2fs_uid().set(c.cr_uid.get());
    ip.set_flag(IN_ACCESS | IN_CHANGE | IN_UPDATE);
    ip.set_i_e2fs_mode(mode as u16);
    tvp.v_type.set(iftovt(mode)); // Rest init'd in getnewvnode().
    ip.set_i_e2fs_nlink(1);
    if e2mode(ip) & ISGID != 0 && !groupmember(ip.i_e2fs_gid().get(), c) && suser_ucred(c).is_err()
    {
        ip.set_i_e2fs_mode((e2mode(ip) & !ISGID) as u16);
    }

    let error: Result<(), Errno> = 'bad: {
        // Make sure inode goes to disk before directory entry.
        if let Err(e) = ext2fs_update(ip, 1) {
            break 'bad Err(e);
        }
        if let Err(e) = ext2fs_direnter(ip, dvp, cnp) {
            break 'bad Err(e);
        }
        if cnp.cn_flags & SAVESTART == 0 {
            pnbuf_free(cnp);
        }
        *vpp = Some(tvp);
        return Ok(());
    };

    // bad:
    // Write error occurred trying to update the inode or the directory so must deallocate
    // the inode.
    pnbuf_free(cnp);
    ip.set_i_e2fs_nlink(0);
    ip.set_flag(IN_CHANGE);
    tvp.v_type.set(VNON);
    vput(tvp);
    error
}

/// `ext2fs_fsync` (`vop_fsync`): synch an open file.
pub fn ext2fs_fsync(ap: &mut VopFsyncArgs<'_>) -> Result<(), Errno> {
    let vp = ap.a_vp;

    vflushbuf(vp, ap.a_waitfor == MNT_WAIT);
    ext2fs_update(vtoi(vp), i32::from(ap.a_waitfor == MNT_WAIT))
}

/// `ext2fs_reclaim` (`vop_reclaim`): reclaim an inode so that it can be used for other
/// purposes.
pub fn ext2fs_reclaim(ap: &mut VopReclaimArgs<'_>) -> Result<(), Errno> {
    let vp = ap.a_vp;

    #[cfg(feature = "diagnostic")]
    if crate::kern::vfs_subr::PRTACTIVE.load(Ordering::Relaxed) != 0 && vp.v_usecount.get() != 0 {
        crate::kern::vfs_subr::vprint(Some("ext2fs_reclaim: pushing active"), vp);
    }

    // Remove the inode from its hash chain.
    let ip = vtoi(vp);
    ufs_ihashrem(ip);

    // Purge old data structures associated with the inode.
    cache_purge(vp);
    if let Some(ump) = ip.i_ump.get()
        && let Some(devvp) = ump.um_devvp.get()
    {
        vrele(devvp);
    }

    if let Some(din) = NonNull::new(ip.dinode_u.get().cast::<u8>()) {
        pool_put(&EXT2FS_DINODE_POOL, din);
    }

    let ipp = NonNull::from(ip).cast::<u8>();
    vp.v_data.set(ptr::null_mut());
    pool_put(&EXT2FS_INODE_POOL, ipp);

    Ok(())
}

// `ext2fs_fifovops` and `ext2fsfifo_reclaim` (`#ifdef FIFO`) wait for `miscfs/fifofs` (the
// module's deviations).
/* </CODE> */

/* <TESTS> */
#[cfg(test)]
mod tests {
    // Host tests for the ext2fs vnode and directory operations (`ext2fs_vnops.rs` and
    // `ext2fs_lookup.rs`), through the system calls' own paths (`namei`, `vn_open`, `vn_rdwr`,
    // `domkdirat`, `dorenameat`, `dounlinkat`, `dolinkat`, `dosymlinkat`, ...) over a fresh ext2
    // file system built in memory (`mkfs`) on the fake disk of `ext2fs_vfsops.rs`'s tests,
    // mounted read-write as the root. After `dounmount` every test runs `fsck` below on the
    // disk: a small `fsck_ext2fs` that checks what `e2fsck -fn` would (directory record chains,
    // link counts, block counts in 512-byte units, duplicate blocks, the bitmaps, and the free
    // and directory counts of the super block and the group descriptor).

    use core::mem::offset_of;
    use std::collections::BTreeMap;
    use std::sync::MutexGuard;
    use std::vec::Vec;
    use std::{assert, assert_eq, assert_ne, vec};

    use super::*;
    use crate::kern::vfs_init::set_rootvnode;
    use crate::kern::vfs_lookup::{namei, ndinit};
    use crate::kern::vfs_subr::{MOUNTLIST, vattr_null, vfs_busy, vfs_unbusy};
    use crate::kern::vfs_syscalls::{
        dolinkat, domkdirat, domknodat, dorenameat, dosymlinkat, dounlinkat, dounmount,
    };
    use crate::kern::vfs_vnops::vn_close;
    use crate::kern::vfs_vnops::vn_open;
    use crate::kern::vfs_vops::{
        VOP_GETATTR, VOP_PATHCONF, VOP_READDIR, VOP_READLINK, VOP_SETATTR,
    };
    use crate::sys::dirent::Dirent;
    use crate::sys::fcntl::{AT_FDCWD, AT_REMOVEDIR, FREAD, O_CREAT};
    use crate::sys::lock::LK_RETRY;
    use crate::sys::mount::VFS_VGET;
    use crate::sys::mount::{VB_WAIT, VB_WRITE, VFS_ROOT};
    use crate::sys::namei::{FOLLOW, LOCKLEAF, LOOKUP, NOFOLLOW, NiDirp};
    use crate::sys::proc::Proc;
    use crate::sys::stat::{S_IFCHR, S_IFIFO};
    use crate::sys::syslimits::LINK_MAX;
    use crate::sys::types::makedev;
    use crate::sys::uio::{Iovec, Uio};
    use crate::sys::unistd::_PC_LINK_MAX;
    use crate::sys::vnode::{VCHR, Vattr};
    use crate::ufs::ext2fs::ext2fs::{
        E2FS_ISCLEAN, E2FS_MAGIC, E2FS_REV1, EXT2F_INCOMPAT_FTYPE, EXT2F_ROCOMPAT_SPARSE_SUPER,
        Ext2Gd, Ext2fs, e2fs_sbsave,
    };
    use crate::ufs::ext2fs::ext2fs_dinode::{EXT2_FIRSTINO, EXT2_ROOTINO, Ext2fsDinode};
    use crate::ufs::ext2fs::ext2fs_dir::{
        EXT2_FT_REG_FILE, EXT2_FT_SYMLINK, e2d_type, e2iftodt, ext2fs_dirsiz, inot2ext2dt,
    };
    use crate::ufs::ext2fs::ext2fs_vfsops::tests::{DISK, mount, setup, teardown};
    use crate::ufs::ufs::ufs_ihash::ufs_ihashget;
    use crate::ufs::ufs::ufsmount::vfstoufs;

    /// The block size.
    const B: usize = 1024;
    /// Blocks in the file system (one group).
    const NBLK: usize = 1024;
    /// Inodes in the file system (one group).
    const IPG: usize = 64;
    /// Where things are (block numbers).
    const GDT: usize = 2;
    const BBITMAP: usize = 3;
    const IBITMAP: usize = 4;
    const ITABLE: usize = 5;

    /// The inode table's size, in blocks, for inodes of `isz` bytes.
    const fn itblocks(isz: usize) -> usize {
        IPG * isz / B
    }

    /// A fresh ext2 file system (revision 1, `FILETYPE`, sparse super blocks, 1 KiB blocks,
    /// `isz`-byte inodes): the super block, one group descriptor, the bitmaps, the inode table and
    /// an empty root directory owned by root, as `newfs_ext2fs` leaves it (without lost+found).
    fn mkfs(isz: usize) -> Vec<u8> {
        let mut img = vec![0u8; NBLK * B];
        let rootblk = ITABLE + itblocks(isz);
        let used_blocks = rootblk; // blocks 1..=rootblk
        let nfree = (NBLK - 1 - used_blocks) as u32;
        let reserved = (EXT2_FIRSTINO - 1) as usize; // inodes 1..=10
        let nifree = (IPG - reserved) as u32;

        let mut sb = Ext2fs::new();
        sb.e2fs_icount = IPG as u32;
        sb.e2fs_bcount = NBLK as u32;
        sb.e2fs_rbcount = 50;
        sb.e2fs_fbcount = nfree;
        sb.e2fs_ficount = nifree;
        sb.e2fs_first_dblock = 1;
        sb.e2fs_bpg = 8192;
        sb.e2fs_fpg = 8192;
        sb.e2fs_ipg = IPG as u32;
        sb.e2fs_wtime = 1_700_000_000;
        sb.e2fs_max_mnt_count = 20;
        sb.e2fs_magic = E2FS_MAGIC;
        sb.e2fs_state = E2FS_ISCLEAN;
        sb.e2fs_beh = 1;
        sb.e2fs_rev = E2FS_REV1;
        sb.e2fs_first_ino = EXT2_FIRSTINO;
        sb.e2fs_inode_size = isz as u16;
        sb.e2fs_features_incompat = EXT2F_INCOMPAT_FTYPE;
        sb.e2fs_features_rocompat = EXT2F_ROCOMPAT_SPARSE_SUPER;
        e2fs_sbsave(&sb, &mut img[B..2 * B]);

        let gd = Ext2Gd {
            ext2bgd_b_bitmap: BBITMAP as u32,
            ext2bgd_i_bitmap: IBITMAP as u32,
            ext2bgd_i_tables: ITABLE as u32,
            ext2bgd_nbfree: nfree as u16,
            ext2bgd_nifree: nifree as u16,
            ext2bgd_ndirs: 1,
            ..Ext2Gd::default()
        };
        img[GDT * B..GDT * B + gd.to_le_bytes().len()].copy_from_slice(&gd.to_le_bytes());

        let setbit =
            |img: &mut Vec<u8>, blk: usize, bit: usize| img[blk * B + bit / 8] |= 1 << (bit % 8);
        // Block bitmap: bit b is block b + 1.
        for b in 1..=used_blocks {
            setbit(&mut img, BBITMAP, b - 1);
        }
        for bit in (NBLK - 1)..8 * B {
            setbit(&mut img, BBITMAP, bit);
        }
        for i in 0..reserved {
            setbit(&mut img, IBITMAP, i);
        }
        for bit in IPG..8 * B {
            setbit(&mut img, IBITMAP, bit);
        }

        // The root directory.
        let at = ITABLE * B + (EXT2_ROOTINO as usize - 1) * isz;
        let put16 =
            |img: &mut Vec<u8>, o: usize, v: u16| img[o..o + 2].copy_from_slice(&v.to_le_bytes());
        let put32 =
            |img: &mut Vec<u8>, o: usize, v: u32| img[o..o + 4].copy_from_slice(&v.to_le_bytes());
        put16(&mut img, at + offset_of!(Ext2fsDinode, e2di_mode), 0o40755);
        put16(&mut img, at + offset_of!(Ext2fsDinode, e2di_nlink), 2);
        put32(&mut img, at + offset_of!(Ext2fsDinode, e2di_size), B as u32);
        put32(&mut img, at + offset_of!(Ext2fsDinode, e2di_nblock), 2);
        put32(
            &mut img,
            at + offset_of!(Ext2fsDinode, e2di_blocks),
            rootblk as u32,
        );
        if isz > 128 {
            put16(&mut img, at + offset_of!(Ext2fsDinode, e2di_isize), 32);
        }
        let d = rootblk * B;
        put32(&mut img, d, EXT2_ROOTINO);
        put16(&mut img, d + 4, 12);
        img[d + 6] = 1;
        img[d + 7] = 2;
        img[d + 8] = b'.';
        put32(&mut img, d + 12, EXT2_ROOTINO);
        put16(&mut img, d + 16, (B - 12) as u16);
        img[d + 18] = 2;
        img[d + 19] = 2;
        img[d + 20..d + 22].copy_from_slice(b"..");
        img
    }

    /// What `fsck` found: every allocated inode with its mode, link count, size and data, and
    /// every directory's entries.
    struct Fs {
        inodes: BTreeMap<u32, Ino>,
        dirs: BTreeMap<u32, Vec<(Vec<u8>, u32, u8)>>,
    }

    /// An allocated inode, as `fsck` read it.
    struct Ino {
        mode: u16,
        nlink: u16,
        uid: u32,
        gid: u32,
        size: u64,
        data: Vec<u8>,
    }

    /// The disk, as bytes.
    fn disk() -> Vec<u8> {
        DISK.lock().unwrap_or_else(|e| e.into_inner()).clone()
    }

    fn le16(d: &[u8], o: usize) -> u16 {
        u16::from_le_bytes([d[o], d[o + 1]])
    }

    fn le32(d: &[u8], o: usize) -> u32 {
        u32::from_le_bytes([d[o], d[o + 1], d[o + 2], d[o + 3]])
    }

    fn bit(d: &[u8], blk: usize, bit: usize) -> bool {
        d[blk * B + bit / 8] & (1 << (bit % 8)) != 0
    }

    /// A small `fsck_ext2fs -n` of the unmounted disk: panics on any inconsistency `e2fsck -fn`
    /// would report, and returns what it read.
    fn fsck() -> Fs {
        let d = disk();
        let sb = B;
        let isz = usize::from(le16(&d, sb + offset_of!(Ext2fs, e2fs_inode_size)));
        assert_eq!(
            le16(&d, sb + offset_of!(Ext2fs, e2fs_state)),
            E2FS_ISCLEAN,
            "clean"
        );
        let first_ino = le32(&d, sb + offset_of!(Ext2fs, e2fs_first_ino));
        let itend = ITABLE + itblocks(isz);

        let mut claimed = vec![false; NBLK];
        for c in claimed.iter_mut().take(itend).skip(1) {
            *c = true;
        }
        let claim = |claimed: &mut Vec<bool>, b: u32, who: u32| {
            let b = b as usize;
            assert!(
                b >= itend && b < NBLK,
                "inode {who}: block {b} out of range"
            );
            assert!(!claimed[b], "inode {who}: block {b} claimed twice");
            claimed[b] = true;
        };

        let mut inodes = BTreeMap::new();
        for ino in 1..=IPG as u32 {
            let at = ITABLE * B + (ino as usize - 1) * isz;
            let f = |o: usize| le32(&d, at + o);
            let mode = le16(&d, at + offset_of!(Ext2fsDinode, e2di_mode));
            let nlink = le16(&d, at + offset_of!(Ext2fsDinode, e2di_nlink));
            let dtime = f(offset_of!(Ext2fsDinode, e2di_dtime));
            if !bit(&d, IBITMAP, ino as usize - 1) {
                // e2fsck takes an inode with links for one in use, and wants a deletion time on
                // a freed one.
                assert_eq!(nlink, 0, "free inode {ino} with links");
                assert!(
                    mode == 0 || dtime != 0,
                    "deleted inode {ino} has zero dtime"
                );
                continue;
            }
            if ino < first_ino && ino != EXT2_ROOTINO {
                continue;
            }
            assert_ne!(mode, 0, "inode {ino} allocated with mode 0");
            assert_ne!(nlink, 0, "inode {ino} allocated with no links");
            assert_eq!(dtime, 0, "inode {ino} in use, but has dtime set");
            let size = u64::from(f(offset_of!(Ext2fsDinode, e2di_size)));
            let nblock = f(offset_of!(Ext2fsDinode, e2di_nblock));
            let blocks: Vec<u32> = (0..15)
                .map(|i| f(offset_of!(Ext2fsDinode, e2di_blocks) + 4 * i))
                .collect();
            let uid = u32::from(le16(&d, at + offset_of!(Ext2fsDinode, e2di_uid_low)))
                | u32::from(le16(&d, at + offset_of!(Ext2fsDinode, e2di_uid_high))) << 16;
            let gid = u32::from(le16(&d, at + offset_of!(Ext2fsDinode, e2di_gid_low)))
                | u32::from(le16(&d, at + offset_of!(Ext2fsDinode, e2di_gid_high))) << 16;

            let fast = u32::from(mode) & IFMT == IFLNK && size < EXT2_MAXSYMLINKLEN as u64;
            let special = !matches!(u32::from(mode) & IFMT, IFREG | IFDIR | IFLNK);
            let mut data = Vec::new();
            let mut nblk = 0u32;
            if special {
                // A device keeps its number in the first block pointer.
                assert_eq!((size, nblock), (0, 0), "special file {ino} with data");
                assert!(
                    blocks[1..].iter().all(|&b| b == 0),
                    "special file {ino}: pointers"
                );
                data.extend_from_slice(&blocks[0].to_le_bytes());
            } else if fast {
                assert_eq!(nblock, 0, "fast symlink {ino} has blocks");
                let s = at + offset_of!(Ext2fsDinode, e2di_blocks);
                data.extend_from_slice(&d[s..s + size as usize]);
            } else {
                // The logical blocks in order (0 for a hole), claiming each block.
                let mut lblocks: Vec<u32> = blocks[..12].to_vec();
                let ptrs = |b: u32| -> Vec<u32> {
                    (0..B / 4)
                        .map(|i| le32(&d, b as usize * B + 4 * i))
                        .collect()
                };
                for &b in blocks[..12].iter().filter(|&&b| b != 0) {
                    claim(&mut claimed, b, ino);
                    nblk += 1;
                }
                if blocks[12] != 0 {
                    claim(&mut claimed, blocks[12], ino);
                    nblk += 1;
                    lblocks.extend(ptrs(blocks[12]));
                }
                if blocks[13] != 0 {
                    claim(&mut claimed, blocks[13], ino);
                    nblk += 1;
                    for b in ptrs(blocks[13]).into_iter().filter(|&b| b != 0) {
                        claim(&mut claimed, b, ino);
                        nblk += 1;
                        lblocks.extend(ptrs(b));
                    }
                }
                assert_eq!(blocks[14], 0, "inode {ino}: triple indirect block");
                for &b in lblocks[12..].iter().filter(|&&b| b != 0) {
                    claim(&mut claimed, b, ino);
                    nblk += 1;
                }
                assert_eq!(nblock, 2 * nblk, "inode {ino}: i_blocks in 512-byte units");
                let nlb = (size as usize).div_ceil(B);
                assert!(
                    lblocks.iter().skip(nlb).all(|&b| b == 0),
                    "inode {ino}: blocks past its size"
                );
                for i in 0..nlb {
                    match lblocks.get(i).copied().unwrap_or(0) {
                        0 => data.extend_from_slice(&[0; B]),
                        b => data.extend_from_slice(&d[b as usize * B..(b as usize + 1) * B]),
                    }
                }
                data.truncate(size as usize);
            }
            inodes.insert(
                ino,
                Ino {
                    mode,
                    nlink,
                    uid,
                    gid,
                    size,
                    data,
                },
            );
        }

        // Directories: record chains, entries, and the references they make.
        let mut refs: BTreeMap<u32, u16> = BTreeMap::new();
        let mut dirs = BTreeMap::new();
        for (&ino, ip) in inodes
            .iter()
            .filter(|(_, ip)| u32::from(ip.mode) & IFMT == IFDIR)
        {
            assert!(
                ip.size > 0 && ip.size % B as u64 == 0,
                "dir {ino}: size {}",
                ip.size
            );
            let mut ents = Vec::new();
            for blk in ip.data.chunks(B) {
                let mut off = 0;
                while off < B {
                    let e = le32(blk, off);
                    let reclen = usize::from(le16(blk, off + 4));
                    let namlen = usize::from(blk[off + 6]);
                    assert!(
                        reclen >= 8 && reclen % 4 == 0,
                        "dir {ino}: rec_len {reclen}"
                    );
                    assert!(off + reclen <= B, "dir {ino}: entry across blocks");
                    if e != 0 {
                        assert!(
                            reclen >= ext2fs_dirsiz(namlen),
                            "dir {ino}: rec_len too small"
                        );
                        let target = inodes
                            .get(&e)
                            .unwrap_or_else(|| std::panic!("dir {ino}: entry to free inode {e}"));
                        let want = inot2ext2dt(e2iftodt(target.mode));
                        assert_eq!(e2d_type(blk, off), want, "dir {ino}: file type of {e}");
                        ents.push((blk[off + 8..off + 8 + namlen].to_vec(), e, blk[off + 7]));
                        *refs.entry(e).or_default() += 1;
                    }
                    off += reclen;
                }
                assert_eq!(
                    off, B,
                    "dir {ino}: the record chain ends at the block's end"
                );
            }
            assert_eq!(ents[0].0, b".", "dir {ino}: first entry");
            assert_eq!(ents[0].1, ino, "dir {ino}: `.`");
            assert_eq!(ents[1].0, b"..", "dir {ino}: second entry");
            dirs.insert(ino, ents);
        }
        for (&ino, ip) in &inodes {
            assert_eq!(
                ip.nlink,
                refs.get(&ino).copied().unwrap_or(0),
                "inode {ino}: links"
            );
        }
        // Every directory's ".." is the directory that names it (the root's is itself).
        for (&ino, ents) in &dirs {
            let parents: Vec<u32> = dirs
                .iter()
                .filter(|(_, es)| es[2..].iter().any(|e| e.1 == ino))
                .map(|(&p, _)| p)
                .collect();
            let want = if ino == EXT2_ROOTINO {
                vec![]
            } else {
                vec![ents[1].1]
            };
            assert_eq!(parents, want, "dir {ino}: `..` and the entries naming it");
        }

        // Bitmaps and counters.
        let mut freeb = 0;
        for (b, &c) in claimed.iter().enumerate().skip(1) {
            assert_eq!(bit(&d, BBITMAP, b - 1), c, "block {b} in the bitmap");
            freeb += u32::from(!c);
        }
        let mut freei = 0;
        for i in 1..=IPG as u32 {
            let used = bit(&d, IBITMAP, i as usize - 1);
            if i >= first_ino {
                assert_eq!(used, inodes.contains_key(&i), "inode {i} in the bitmap");
            }
            freei += u32::from(!used);
        }
        let ndirs = dirs.len() as u16;
        assert_eq!(
            le32(&d, sb + offset_of!(Ext2fs, e2fs_fbcount)),
            freeb,
            "sb free blocks"
        );
        assert_eq!(
            le32(&d, sb + offset_of!(Ext2fs, e2fs_ficount)),
            freei,
            "sb free inodes"
        );
        assert_eq!(le16(&d, GDT * B + 12), freeb as u16, "gd free blocks");
        assert_eq!(le16(&d, GDT * B + 14), freei as u16, "gd free inodes");
        assert_eq!(le16(&d, GDT * B + 16), ndirs, "gd directories");
        Fs { inodes, dirs }
    }

    impl Fs {
        /// The inode `path` names (absolute, no symbolic links).
        fn lookup(&self, path: &str) -> Option<u32> {
            let mut ino = EXT2_ROOTINO;
            for c in path.split('/').filter(|c| !c.is_empty()) {
                ino = self.dirs.get(&ino)?.iter().find(|e| e.0 == c.as_bytes())?.1;
            }
            Some(ino)
        }

        fn ino(&self, path: &str) -> &Ino {
            let i = self
                .lookup(path)
                .unwrap_or_else(|| std::panic!("{path} on disk"));
            &self.inodes[&i]
        }

        /// The names in directory `path`, without `.` and `..`.
        fn names(&self, path: &str) -> Vec<Vec<u8>> {
            let i = self.lookup(path).unwrap();
            self.dirs[&i][2..].iter().map(|e| e.0.clone()).collect()
        }
    }

    /// The locks a test holds: the timecounter tests' and the memory's.
    type Guards = (MutexGuard<'static, ()>, MutexGuard<'static, ()>);

    /// The test setup plus a fresh file system of `isz`-byte inodes mounted read-write as "/"
    /// and made the thread's current directory, on the mount list as `sys_mount` leaves it.
    ///
    /// The clock is set to 2023 first (under the timecounter tests' lock, taken before the
    /// memory's as `wg_noise`'s tests do): `ext2fs_inactive` marks a freed inode by its deletion
    /// time, and a time of 0 would leave it looking alive, to be freed again by the next
    /// `ext2fs_inactive`, which a booted kernel never sees.
    fn setup_root(isz: usize) -> (Guards, &'static Proc, &'static Mount) {
        let t = crate::kern::kern_tc::tests::LOCK
            .lock()
            .unwrap_or_else(|e| e.into_inner());
        let (g, p) = setup(mkfs(isz));
        crate::kern::kern_tc::tc_setrealtimeclock(&Timespec::new(1_700_000_000, 0));
        assert!(crate::kern::kern_tc::getnanotime().tv_sec >= 1_700_000_000);
        let mp = mount(p, false).unwrap();
        vfs_unbusy(mp);
        // SAFETY: a new mount on no list.
        unsafe { MOUNTLIST.0.insert_tail(mp) };
        let root = VFS_ROOT(mp).unwrap();
        set_rootvnode(Some(root));
        p.fd().fd_cdir.set(Some(root));
        vref(root);
        let _ = VOP_UNLOCK(root);
        ((t, g), p, mp)
    }

    /// Lets go of the root and unmounts (`dounmount`: `ext2fs_sync`, `ext2fs_unmount`).
    fn unmount(p: &'static Proc, mp: &'static Mount) {
        let root = p.fd().fd_cdir.get().unwrap();
        p.fd().fd_cdir.set(None);
        set_rootvnode(None);
        vrele(root);
        vrele(root);
        vfs_busy(mp, VB_WRITE | VB_WAIT).unwrap();
        dounmount(mp, 0, p).unwrap();
        teardown();
    }

    /// A user-space path for the `do*at` functions: the bytes and a NUL.
    fn c(path: &str) -> Vec<u8> {
        let mut v = path.as_bytes().to_vec();
        v.push(0);
        v
    }

    /// `open(path, O_RDWR | O_CREAT, mode)`: the vnode, unlocked and referenced.
    fn create(p: &'static Proc, path: &str, mode: Mode) -> &'static Vnode {
        let path = c(path);
        let mut nd = ndinit(0, 0, NiDirp::Sys(&path), p);
        vn_open(&mut nd, FREAD | FWRITE | O_CREAT, mode).expect("create");
        let vp = nd.ni_vp.expect("a vnode");
        let _ = VOP_UNLOCK(vp);
        vp
    }

    /// `close` of a vnode `create` gave.
    fn close(p: &'static Proc, vp: &'static Vnode) {
        vn_close(vp, FREAD | FWRITE, p.ucred(), Some(p)).expect("close");
    }

    /// The vnode `path` names, unlocked and referenced.
    fn lookup(p: &'static Proc, path: &str) -> Result<&'static Vnode, Errno> {
        let path = c(path);
        let mut nd = ndinit(LOOKUP, FOLLOW, NiDirp::Sys(&path), p);
        namei(&mut nd)?;
        Ok(nd.ni_vp.expect("a vnode"))
    }

    /// `stat(path)` (`lstat` with `nofollow`).
    fn stat_flags(p: &'static Proc, path: &str, follow: u64) -> Result<Vattr, Errno> {
        let path = c(path);
        let mut nd = ndinit(LOOKUP, LOCKLEAF | follow, NiDirp::Sys(&path), p);
        namei(&mut nd)?;
        let vp = nd.ni_vp.expect("a vnode");
        let mut va = Vattr::new();
        let error = VOP_GETATTR(vp, &mut va, p.ucred(), p);
        vput(vp);
        error.map(|()| va)
    }

    fn stat(p: &'static Proc, path: &str) -> Result<Vattr, Errno> {
        stat_flags(p, path, FOLLOW)
    }

    /// `vn_rdwr` of `buf` at `off` of `vp` (unlocked): the bytes moved.
    fn rdwr(p: &'static Proc, rw: UioRw, vp: &'static Vnode, buf: &mut [u8], off: i64) -> usize {
        let mut resid = 0;
        let procp = if rw == UioRw::UIO_WRITE {
            None
        } else {
            Some(p)
        };
        vn_rdwr(
            rw,
            vp,
            buf.as_mut_ptr().cast(),
            buf.len(),
            off,
            UioSeg::UIO_SYSSPACE,
            0,
            p.ucred(),
            Some(&mut resid),
            procp,
        )
        .expect("vn_rdwr");
        buf.len() - resid
    }

    /// The whole contents of the file `path`.
    fn read_file(p: &'static Proc, path: &str) -> Vec<u8> {
        let size = stat(p, path).expect("stat").va_size as usize;
        let vp = lookup(p, path).expect("lookup");
        let mut buf = vec![0u8; size + 100];
        let n = rdwr(p, UioRw::UIO_READ, vp, &mut buf, 0);
        vrele(vp);
        buf.truncate(n);
        buf
    }

    /// A file `path` holding `data`.
    fn write_file(p: &'static Proc, path: &str, data: &[u8]) {
        let vp = create(p, path, 0o644);
        let mut w = data.to_vec();
        assert_eq!(rdwr(p, UioRw::UIO_WRITE, vp, &mut w, 0), data.len());
        close(p, vp);
    }

    /// `VOP_SETATTR` with the vnode locked.
    fn setattr(p: &'static Proc, path: &str, f: impl FnOnce(&mut Vattr)) -> Result<(), Errno> {
        let vp = lookup(p, path)?;
        let mut va = Vattr::new();
        vattr_null(&mut va);
        f(&mut va);
        let _ = vn_lock(vp, LK_EXCLUSIVE | LK_RETRY);
        let error = VOP_SETATTR(vp, &mut va, p.ucred(), p);
        vput(vp);
        error
    }

    /// One `struct dirent` `getdents` returned.
    #[derive(Debug)]
    struct Ent {
        name: Vec<u8>,
        off: i64,
        fileno: u64,
    }

    /// One `VOP_READDIR` of the directory `path` at `offset` into a buffer of `size` bytes: the
    /// entries, the new offset and the EOF flag.
    fn readdir(
        p: &'static Proc,
        path: &str,
        offset: i64,
        size: usize,
    ) -> Result<(Vec<Ent>, i64, i32), Errno> {
        let cpath = c(path);
        let mut nd = ndinit(LOOKUP, LOCKLEAF | FOLLOW, NiDirp::Sys(&cpath), p);
        namei(&mut nd)?;
        let vp = nd.ni_vp.expect("a vnode");
        let mut buf = vec![0u8; size];
        let mut iov = [Iovec {
            iov_base: buf.as_mut_ptr().cast(),
            iov_len: size,
        }];
        let mut uio = Uio {
            uio_iov: &mut iov,
            uio_offset: offset,
            uio_resid: size,
            uio_segflg: UioSeg::UIO_SYSSPACE,
            uio_rw: UioRw::UIO_READ,
            uio_procp: None,
        };
        let mut eof = 0;
        let error = VOP_READDIR(vp, &mut uio, p.ucred(), &mut eof);
        let used = size - uio.uio_resid;
        let newoff = uio.uio_offset;
        vput(vp);
        error?;

        let mut ents = Vec::new();
        let mut off = 0;
        while off < used {
            let d = Dirent::from_bytes(&buf[off..]).expect("a dirent");
            let name = &buf[off + Dirent::NAME_OFFSET..][..usize::from(d.d_namlen)];
            assert_eq!(
                buf[off + Dirent::NAME_OFFSET + name.len()],
                0,
                "NUL-terminated"
            );
            ents.push(Ent {
                name: name.to_vec(),
                off: d.d_off,
                fileno: d.d_fileno,
            });
            off += usize::from(d.d_reclen);
        }
        Ok((ents, newoff, eof))
    }

    /// Every name `readdir` gives for `path`, `size` bytes at a time from offset 0, without the
    /// free entries (`d_fileno` 0, which `readdir(3)` skips): the names and how many free entries
    /// there were.
    fn readdir_all(p: &'static Proc, path: &str, size: usize) -> (Vec<Vec<u8>>, usize) {
        let mut names = Vec::new();
        let mut free = 0;
        let mut off = 0;
        loop {
            let (ents, noff, eof) = readdir(p, path, off, size).expect("readdir");
            for e in &ents {
                if e.fileno == 0 {
                    free += 1;
                } else {
                    names.push(e.name.clone());
                }
            }
            if let Some(last) = ents.last() {
                assert_eq!(last.off, noff, "the last d_off is the new offset");
            }
            off = noff;
            if eof != 0 {
                return (names, free);
            }
            assert!(!ents.is_empty(), "progress");
        }
    }

    /// `n` bytes of a pattern that differs per `seed`.
    fn pattern(n: usize, seed: u8) -> Vec<u8> {
        (0..n)
            .map(|i| ((i * 7 + usize::from(seed)) % 251) as u8)
            .collect()
    }

    #[test]
    fn lookups_creates_writes_links_and_removes() {
        let (_g, p, mp) = setup_root(128);

        // Lookups of what is there and what is not.
        assert_eq!(stat(p, "/").unwrap().va_fileid, u64::from(EXT2_ROOTINO));
        assert_eq!(stat(p, "/.").unwrap().va_fileid, u64::from(EXT2_ROOTINO));
        assert_eq!(stat(p, "/..").unwrap().va_fileid, u64::from(EXT2_ROOTINO));
        assert_eq!(stat(p, "/missing").err(), Some(Errno::ENOENT));
        assert_eq!(stat(p, "/missing/x").err(), Some(Errno::ENOENT));

        // A file, written across a few blocks and read back.
        let data = pattern(3000, 1);
        write_file(p, "/hello", &data);
        let va = stat(p, "/hello").unwrap();
        assert_eq!(va.va_size, 3000);
        assert_eq!(va.va_type, VREG);
        assert_eq!(va.va_mode, 0o644);
        assert_eq!(va.va_nlink, 1);
        assert_eq!(va.va_bytes, 3 * B as u64);
        assert_eq!(read_file(p, "/hello"), data);
        assert_eq!(stat(p, "/hello/x").err(), Some(Errno::ENOTDIR));
        // A second, big enough for an indirect block.
        let big = pattern(14 * B + 7, 2);
        write_file(p, "/big", &big);
        assert_eq!(read_file(p, "/big"), big);
        assert_eq!(stat(p, "/big").unwrap().va_bytes, 16 * B as u64);

        // Hard links.
        dolinkat(
            p,
            AT_FDCWD,
            c("/hello").as_ptr(),
            AT_FDCWD,
            c("/hl").as_ptr(),
            0,
        )
        .unwrap();
        assert_eq!(stat(p, "/hl").unwrap().va_nlink, 2);
        assert_eq!(stat(p, "/hl").unwrap().va_fileid, va.va_fileid);
        assert_eq!(
            dolinkat(
                p,
                AT_FDCWD,
                c("/hello").as_ptr(),
                AT_FDCWD,
                c("/big").as_ptr(),
                0
            ),
            Err(Errno::EEXIST)
        );
        dounlinkat(p, AT_FDCWD, c("/hello").as_ptr(), 0).unwrap();
        assert_eq!(stat(p, "/hello").err(), Some(Errno::ENOENT));
        assert_eq!(stat(p, "/hl").unwrap().va_nlink, 1);
        assert_eq!(read_file(p, "/hl"), data);

        // A removed file's blocks and inode go back.
        let fs = vfstoufs(mp).e2fs();
        let (fb, fi) = (fs.e2fs_fbcount(), fs.e2fs_ficount());
        dounlinkat(p, AT_FDCWD, c("/big").as_ptr(), 0).unwrap();
        assert_eq!(fs.e2fs_fbcount(), fb + 16);
        assert_eq!(fs.e2fs_ficount(), fi + 1);

        // pathconf
        let vp = lookup(p, "/hl").unwrap();
        let mut r = 0;
        VOP_PATHCONF(vp, _PC_TIMESTAMP_RESOLUTION, &mut r).unwrap();
        assert_eq!(r, 1_000_000_000);
        VOP_PATHCONF(vp, _PC_LINK_MAX, &mut r).unwrap();
        assert_eq!(r, LINK_MAX as Register);
        vrele(vp);

        unmount(p, mp);
        let fs = fsck();
        assert_eq!(fs.names("/"), [b"hl".to_vec()]);
        assert_eq!(fs.ino("/hl").data, data);
        assert_eq!(fs.ino("/hl").nlink, 1);
    }

    #[test]
    fn directories_are_made_renamed_and_removed() {
        let (_g, p, mp) = setup_root(256);

        domkdirat(p, AT_FDCWD, c("/d").as_ptr(), 0o755).unwrap();
        domkdirat(p, AT_FDCWD, c("/d/e").as_ptr(), 0o700).unwrap();
        domkdirat(p, AT_FDCWD, c("/f").as_ptr(), 0o755).unwrap();
        assert_eq!(
            domkdirat(p, AT_FDCWD, c("/d").as_ptr(), 0o755),
            Err(Errno::EEXIST)
        );
        let d = stat(p, "/d").unwrap();
        assert_eq!(
            (d.va_type, d.va_mode, d.va_nlink, d.va_size),
            (VDIR, 0o755, 3, B as u64)
        );
        assert_eq!(stat(p, "/").unwrap().va_nlink, 4);
        assert_eq!(stat(p, "/d/e/..").unwrap().va_fileid, d.va_fileid);
        write_file(p, "/d/x", b"x data");
        write_file(p, "/d/e/inner", b"inner");

        // A file, within a directory, then across directories.
        dorenameat(
            p,
            AT_FDCWD,
            c("/d/x").as_ptr(),
            AT_FDCWD,
            c("/d/x2").as_ptr(),
        )
        .unwrap();
        dorenameat(
            p,
            AT_FDCWD,
            c("/d/x2").as_ptr(),
            AT_FDCWD,
            c("/f/y").as_ptr(),
        )
        .unwrap();
        assert_eq!(stat(p, "/d/x2").err(), Some(Errno::ENOENT));
        assert_eq!(read_file(p, "/f/y"), b"x data");
        // Over an existing file, whose inode goes.
        write_file(p, "/f/z", b"zzz");
        let fi = vfstoufs(mp).e2fs().e2fs_ficount();
        dorenameat(
            p,
            AT_FDCWD,
            c("/f/y").as_ptr(),
            AT_FDCWD,
            c("/f/z").as_ptr(),
        )
        .unwrap();
        assert_eq!(read_file(p, "/f/z"), b"x data");
        assert_eq!(vfstoufs(mp).e2fs().e2fs_ficount(), fi + 1);
        // Two names of one file: rename(2) does nothing.
        dolinkat(
            p,
            AT_FDCWD,
            c("/f/z").as_ptr(),
            AT_FDCWD,
            c("/f/w").as_ptr(),
            0,
        )
        .unwrap();
        dorenameat(
            p,
            AT_FDCWD,
            c("/f/w").as_ptr(),
            AT_FDCWD,
            c("/f/z").as_ptr(),
        )
        .unwrap();
        assert_eq!(stat(p, "/f/z").unwrap().va_nlink, 2);
        dounlinkat(p, AT_FDCWD, c("/f/w").as_ptr(), 0).unwrap();
        assert_eq!(stat(p, "/f/z").unwrap().va_nlink, 1);

        // A directory across directories: ".." follows, the parents' link counts too.
        dorenameat(
            p,
            AT_FDCWD,
            c("/d/e").as_ptr(),
            AT_FDCWD,
            c("/f/e").as_ptr(),
        )
        .unwrap();
        assert_eq!(stat(p, "/d").unwrap().va_nlink, 2);
        assert_eq!(stat(p, "/f").unwrap().va_nlink, 3);
        assert_eq!(
            stat(p, "/f/e/..").unwrap().va_fileid,
            stat(p, "/f").unwrap().va_fileid
        );
        assert_eq!(read_file(p, "/f/e/inner"), b"inner");
        // Into its own subtree, onto a file, onto a non-empty directory: refused.
        assert_eq!(
            dorenameat(
                p,
                AT_FDCWD,
                c("/f").as_ptr(),
                AT_FDCWD,
                c("/f/e/g").as_ptr()
            ),
            Err(Errno::EINVAL)
        );
        assert_eq!(
            dorenameat(
                p,
                AT_FDCWD,
                c("/f/e").as_ptr(),
                AT_FDCWD,
                c("/f/z").as_ptr()
            ),
            Err(Errno::ENOTDIR)
        );
        domkdirat(p, AT_FDCWD, c("/d/full").as_ptr(), 0o755).unwrap();
        write_file(p, "/d/full/f", b"");
        assert_eq!(
            dorenameat(
                p,
                AT_FDCWD,
                c("/f/e").as_ptr(),
                AT_FDCWD,
                c("/d/full").as_ptr()
            ),
            Err(Errno::ENOTEMPTY)
        );
        // A directory within its parent, then over an empty directory elsewhere.
        dorenameat(
            p,
            AT_FDCWD,
            c("/f/e").as_ptr(),
            AT_FDCWD,
            c("/f/e2").as_ptr(),
        )
        .unwrap();
        assert_eq!(stat(p, "/f").unwrap().va_nlink, 3);
        domkdirat(p, AT_FDCWD, c("/d/empty").as_ptr(), 0o755).unwrap();
        assert_eq!(stat(p, "/d").unwrap().va_nlink, 4);
        dorenameat(
            p,
            AT_FDCWD,
            c("/f/e2").as_ptr(),
            AT_FDCWD,
            c("/d/empty").as_ptr(),
        )
        .unwrap();
        assert_eq!(stat(p, "/d").unwrap().va_nlink, 4);
        assert_eq!(stat(p, "/f").unwrap().va_nlink, 2);
        assert_eq!(read_file(p, "/d/empty/inner"), b"inner");
        assert_eq!(stat(p, "/d/empty/..").unwrap().va_fileid, d.va_fileid);

        // rmdir: not a non-empty one; unlink: not a directory.
        assert_eq!(
            dounlinkat(p, AT_FDCWD, c("/d/full").as_ptr(), AT_REMOVEDIR),
            Err(Errno::ENOTEMPTY)
        );
        assert_eq!(
            dounlinkat(p, AT_FDCWD, c("/d/full").as_ptr(), 0),
            Err(Errno::EPERM)
        );
        dounlinkat(p, AT_FDCWD, c("/d/full/f").as_ptr(), 0).unwrap();
        dounlinkat(p, AT_FDCWD, c("/d/full").as_ptr(), AT_REMOVEDIR).unwrap();
        assert_eq!(stat(p, "/d/full").err(), Some(Errno::ENOENT));
        assert_eq!(stat(p, "/d").unwrap().va_nlink, 3);

        unmount(p, mp);
        let fs = fsck();
        assert_eq!(fs.names("/"), [b"d".to_vec(), b"f".to_vec()]);
        assert_eq!(fs.names("/d"), [b"empty".to_vec()]);
        assert_eq!(fs.names("/f"), [b"z".to_vec()]);
        assert_eq!(fs.names("/d/empty"), [b"inner".to_vec()]);
        assert_eq!(fs.ino("/d/empty/inner").data, b"inner");
        assert_eq!(fs.ino("/d").nlink, 3);
        assert_eq!(fs.ino("/").nlink, 4);
        assert_eq!(fs.ino("/d/empty").mode, 0o40700);
    }

    #[test]
    fn symbolic_links_short_and_long() {
        let (_g, p, mp) = setup_root(128);
        write_file(p, "/target", b"through the link");
        dosymlinkat(p, c("target").as_ptr(), AT_FDCWD, c("/s").as_ptr()).unwrap();
        let long: std::string::String = (0..100).map(|i| (b'a' + (i % 26) as u8) as char).collect();
        dosymlinkat(p, c(&long).as_ptr(), AT_FDCWD, c("/l").as_ptr()).unwrap();
        assert_eq!(
            dosymlinkat(p, c("x").as_ptr(), AT_FDCWD, c("/s").as_ptr()),
            Err(Errno::EEXIST)
        );

        // readlink of both, lstat, and a lookup through the short one.
        let readlink = |path: &str| {
            let cpath = c(path);
            let mut nd = ndinit(LOOKUP, LOCKLEAF | NOFOLLOW, NiDirp::Sys(&cpath), p);
            namei(&mut nd).unwrap();
            let vp = nd.ni_vp.unwrap();
            let mut buf = vec![0u8; 300];
            let mut iov = [Iovec {
                iov_base: buf.as_mut_ptr().cast(),
                iov_len: buf.len(),
            }];
            let mut uio = Uio {
                uio_iov: &mut iov,
                uio_offset: 0,
                uio_resid: 300,
                uio_segflg: UioSeg::UIO_SYSSPACE,
                uio_rw: UioRw::UIO_READ,
                uio_procp: None,
            };
            VOP_READLINK(vp, &mut uio, p.ucred()).unwrap();
            let n = 300 - uio.uio_resid;
            vput(vp);
            buf.truncate(n);
            buf
        };
        assert_eq!(readlink("/s"), b"target");
        assert_eq!(readlink("/l"), long.as_bytes());
        let s = stat_flags(p, "/s", NOFOLLOW).unwrap();
        assert_eq!((s.va_type, s.va_size, s.va_bytes), (VLNK, 6, 0));
        let l = stat_flags(p, "/l", NOFOLLOW).unwrap();
        assert_eq!((l.va_type, l.va_size, l.va_bytes), (VLNK, 100, B as u64));
        assert_eq!(read_file(p, "/s"), b"through the link");

        unmount(p, mp);
        let fs = fsck();
        assert_eq!(fs.ino("/s").data, b"target");
        assert_eq!(fs.ino("/l").data, long.as_bytes());
        assert_eq!(u32::from(fs.ino("/s").mode) & IFMT, IFLNK);
        let root = &fs.dirs[&EXT2_ROOTINO];
        assert!(root.iter().any(|e| e.0 == b"s" && e.2 == EXT2_FT_SYMLINK));
        assert!(
            root.iter()
                .any(|e| e.0 == b"target" && e.2 == EXT2_FT_REG_FILE)
        );
    }

    #[test]
    fn readdir_spans_blocks_and_skips_deleted_entries() {
        let (_g, p, mp) = setup_root(128);
        domkdirat(p, AT_FDCWD, c("/t").as_ptr(), 0o755).unwrap();
        write_file(p, "/t/base", b"b");
        // 40 links with 60-byte names: 68-byte entries, 15 to a block, three blocks.
        let name = |i: usize| std::format!("{i:02}{}", "n".repeat(58));
        for i in 0..40 {
            let to = std::format!("/t/{}", name(i));
            dolinkat(
                p,
                AT_FDCWD,
                c("/t/base").as_ptr(),
                AT_FDCWD,
                c(&to).as_ptr(),
                0,
            )
            .unwrap();
        }
        assert_eq!(stat(p, "/t").unwrap().va_size, 3 * B as u64);
        assert_eq!(stat(p, "/t/base").unwrap().va_nlink, 41);
        let mut want: Vec<Vec<u8>> = vec![b".".to_vec(), b"..".to_vec(), b"base".to_vec()];
        want.extend((0..40).map(|i| name(i).into_bytes()));
        // A buffer must reach the end of a directory block; 1024 bytes of 68-byte ext2 entries
        // are more than 1024 bytes of dirents, so the smaller reads stop and resume mid-block.
        for size in [1024, 1500, 4096] {
            let (names, free) = readdir_all(p, "/t", size);
            assert_eq!(names, want, "{size}-byte reads");
            assert_eq!(free, 0);
        }

        // Remove the first entry of the second block (its inode is zeroed, the entry stays) and
        // one in the middle of the third (its space goes to the entry before it).
        let first2 = 14; // ".", "..", "base" (36 bytes) and 14 names fill the first block
        for i in [first2, 33] {
            let path = std::format!("/t/{}", name(i));
            dounlinkat(p, AT_FDCWD, c(&path).as_ptr(), 0).unwrap();
            assert_eq!(stat(p, &path).err(), Some(Errno::ENOENT));
        }
        want.retain(|n| *n != name(first2).into_bytes() && *n != name(33).into_bytes());
        let (names, free) = readdir_all(p, "/t", 1024);
        assert_eq!(names, want);
        assert_eq!(
            free, 1,
            "the free first entry of a block is listed with d_fileno 0"
        );
        // A new name fits in the freed space: the directory does not grow.
        dolinkat(
            p,
            AT_FDCWD,
            c("/t/base").as_ptr(),
            AT_FDCWD,
            c("/t/new").as_ptr(),
            0,
        )
        .unwrap();
        assert_eq!(stat(p, "/t").unwrap().va_size, 3 * B as u64);
        // A partial entry is refused, a read past the end is empty.
        assert_eq!(readdir(p, "/t", 0, 10).err(), Some(Errno::EINVAL));
        let (ents, _, eof) = readdir(p, "/t", 3 * B as i64, 1024).unwrap();
        assert!(ents.is_empty() && eof != 0);
        assert_eq!(readdir(p, "/t/base", 0, 1024).err(), Some(Errno::ENOTDIR));

        unmount(p, mp);
        let fs = fsck();
        assert_eq!(fs.ino("/t/base").nlink, 40);
        assert_eq!(fs.names("/t").len(), 40);
    }

    #[test]
    fn entries_are_compacted_to_make_room() {
        let (_g, p, mp) = setup_root(128);
        domkdirat(p, AT_FDCWD, c("/c").as_ptr(), 0o755).unwrap();
        // 168 names of 12-byte entries: 83 after "." and ".." fill the first block but 4 bytes,
        // 85 the second.
        write_file(p, "/c/x0", b"x");
        for i in 1..168 {
            let to = std::format!("/c/x{i}");
            dolinkat(
                p,
                AT_FDCWD,
                c("/c/x0").as_ptr(),
                AT_FDCWD,
                c(&to).as_ptr(),
                0,
            )
            .unwrap();
        }
        assert_eq!(stat(p, "/c").unwrap().va_size, 2 * B as u64);
        let unlink = |name: &str| {
            let path = std::format!("/c/{name}");
            dounlinkat(p, AT_FDCWD, c(&path).as_ptr(), 0).unwrap();
        };
        let link = |name: &str| {
            let path = std::format!("/c/{name}");
            dolinkat(
                p,
                AT_FDCWD,
                c("/c/x0").as_ptr(),
                AT_FDCWD,
                c(&path).as_ptr(),
                0,
            )
            .unwrap();
        };

        // Two 12-byte holes in the first block, none big enough for a 16-byte entry alone: the
        // entries between them move up to make one.
        unlink("x10");
        unlink("x20");
        link("yyyyy");
        // In the second block the hole starts with a free first entry (inode 0).
        unlink("x83");
        unlink("x85");
        link("zzzzz");
        assert_eq!(stat(p, "/c").unwrap().va_size, 2 * B as u64, "no new block");
        assert_eq!(read_file(p, "/c/yyyyy"), b"x");
        assert_eq!(read_file(p, "/c/zzzzz"), b"x");
        let (names, free) = readdir_all(p, "/c", 4096);
        assert_eq!(names.len(), 2 + 168 - 4 + 2);
        assert_eq!(free, 0);

        unmount(p, mp);
        let fs = fsck();
        let names = fs.names("/c");
        let at = |n: &[u8]| names.iter().position(|x| x == n).unwrap();
        // yyyyy took the place of x11..x20 moved up, before x21; zzzzz followed x84.
        assert!(at(b"x9") < at(b"yyyyy") && at(b"yyyyy") < at(b"x21"));
        assert!(at(b"x84") < at(b"zzzzz") && at(b"zzzzz") < at(b"x86"));
        assert_eq!(fs.ino("/c/x0").nlink, 166);
    }

    #[test]
    fn attributes_are_set_and_read() {
        let (_g, p, mp) = setup_root(128);
        write_file(p, "/f", &pattern(5000, 3));

        // chmod and chown (as root).
        setattr(p, "/f", |va| va.va_mode = 0o4751).unwrap();
        assert_eq!(stat(p, "/f").unwrap().va_mode, 0o4751);
        setattr(p, "/f", |va| {
            va.va_uid = 70_000;
            va.va_gid = 20;
        })
        .unwrap();
        let va = stat(p, "/f").unwrap();
        assert_eq!((va.va_uid, va.va_gid), (70_000, 20));
        // A directory cannot be truncated.
        assert_eq!(setattr(p, "/", |va| va.va_size = 0), Err(Errno::EISDIR));
        // Unsettable attributes.
        assert_eq!(setattr(p, "/f", |va| va.va_nlink = 3), Err(Errno::EINVAL));

        // Truncate: shorter, then empty; the blocks go back.
        let fs = vfstoufs(mp).e2fs();
        let fb = fs.e2fs_fbcount();
        setattr(p, "/f", |va| va.va_size = 100).unwrap();
        assert_eq!(read_file(p, "/f"), pattern(100, 3));
        assert_eq!(fs.e2fs_fbcount(), fb + 4);
        assert_eq!(stat(p, "/f").unwrap().va_bytes, B as u64);

        // Times.
        setattr(p, "/f", |va| {
            va.va_mtime = Timespec::new(1_234_567_890, 0);
            va.va_atime = Timespec::new(1_000_000_000, 0);
        })
        .unwrap();
        let va = stat(p, "/f").unwrap();
        assert_eq!(va.va_mtime.tv_sec, 1_234_567_890);
        assert_eq!(va.va_atime.tv_sec, 1_000_000_000);

        // Immutable files cannot be written or removed.
        setattr(p, "/f", |va| va.va_flags = u64::from(SF_IMMUTABLE)).unwrap();
        assert_eq!(stat(p, "/f").unwrap().va_flags, u64::from(SF_IMMUTABLE));
        assert_eq!(setattr(p, "/f", |va| va.va_mode = 0o600), Err(Errno::EPERM));
        assert_eq!(
            dounlinkat(p, AT_FDCWD, c("/f").as_ptr(), 0),
            Err(Errno::EPERM)
        );
        setattr(p, "/f", |va| va.va_flags = 0).unwrap();
        setattr(p, "/f", |va| va.va_size = 0).unwrap();
        assert_eq!(stat(p, "/f").unwrap().va_bytes, 0);

        // A device node: mknod reloads it as a special file. Fifos need option FIFO.
        let dev = makedev(2, 7);
        domknodat(p, AT_FDCWD, c("/tty").as_ptr(), S_IFCHR | 0o600, dev).unwrap();
        let va = stat(p, "/tty").unwrap();
        assert_eq!((va.va_type, va.va_rdev), (VCHR, dev));
        assert_eq!(
            domknodat(p, AT_FDCWD, c("/fifo").as_ptr(), S_IFIFO | 0o600, 0),
            Err(Errno::EOPNOTSUPP)
        );

        unmount(p, mp);
        let fs = fsck();
        let f = fs.ino("/f");
        assert_eq!((f.uid, f.gid, f.size, f.mode), (70_000, 20, 0, 0o104751));
        assert_eq!(fs.ino("/tty").mode, 0o20600);
        assert_eq!(fs.names("/"), [b"f".to_vec(), b"tty".to_vec()]);
    }

    #[test]
    fn a_second_vget_of_a_cached_root_owned_inode_returns() {
        // ufs_ihashget's link count check reads i_e2fs_nlink for an ext2fs inode; read as a UFS1
        // dinode, the nlink of a file owned by uid 0 is its uid_low, 0, and the lookup would wait
        // for the inode to go away forever.
        let (_g, p, mp) = setup_root(128);
        write_file(p, "/root-owned", b"r");
        let ino = stat(p, "/root-owned").unwrap().va_fileid;
        let vp = lookup(p, "/root-owned").unwrap();
        assert_eq!(vtoi(vp).i_e2fs_uid().get(), 0);
        let dev = vtoi(vp).i_dev.get();
        vrele(vp);
        for _ in 0..2 {
            let vp = VFS_VGET(mp, ino).unwrap();
            assert_eq!(vtoi(vp).i_number.get() as u64, ino);
            vput(vp);
        }
        let vp = ufs_ihashget(dev, ino as Ufsino).expect("in the hash");
        vput(vp);
        unmount(p, mp);
        fsck();
    }
}
/* </TESTS> */
