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

#[cfg(test)]
mod tests;
