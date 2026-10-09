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
 */
/* </LICENSES> */

/* <CODE> */
//! The vnode operations the UFS file systems share: create, link, rename, remove, mkdir,
//! rmdir, symlink, readdir, readlink, attributes and permissions, the inode lock, the
//! strategy that maps a file's logical blocks to the disk, pathconf, advisory locks, and the
//! wrappers of the special file operations that keep an inode's times.
//!
//! Upstream: sys/ufs/ufs/ufs_vnops.c @ 3ce1f3f79392
//!
//! ## Deviations
//! - The filters reach their vnode through `kn_hook` (`kn_vnode`); `filt_ufsread`'s
//!   `EXT2FS` branch (`ext2fs_size`) is under feature `ext2fs`.
//! - `option FIFO` is in GENERIC, but `miscfs/fifofs` is not ported: `ufsfifo_read`,
//!   `ufsfifo_write`, `ufsfifo_close` (and `ffs_fifovops`, `ffsfifo_reclaim`) come with it;
//!   until then `ffs_vinit` refuses a fifo with `EOPNOTSUPP`, as a kernel without `FIFO`
//!   does.
//! - `EXT2FS`'s branch of `ufs_itimes` (`EXT2FS_ITIMES`, `Inode::ext2fs_itimes`) is under
//!   feature `ext2fs`.
//! - `pool_put(&namei_pool, cnp->cn_pnbuf)` is [`pnbuf_free`].
//! - Credentials that the C dereferences (`cred->cr_uid`) go through [`ucred`], which panics
//!   on `NOCRED`/`FSCRED`, where the C would follow a bad pointer.
//! - `UFS_DIRHASH`'s `ufsdirhash_free` in `ufs_rmdir` is under feature `ufs_dirhash`.

use core::ptr::{self, NonNull};
use core::sync::atomic::Ordering;

use crate::kern::kern_event::{klist_insert_locked, klist_remove_locked};
use crate::kern::kern_prot::{groupmember, suser_ucred};
use crate::kern::kern_rwlock::{rrw_enter, rrw_exit, rrw_status};
use crate::kern::kern_subr::uiomove;
use crate::kern::kern_sysctl::SECURELEVEL;
use crate::kern::kern_tc::getnanotime;
use crate::kern::spec_vnops::{spec_close, spec_read, spec_write};
use crate::kern::subr_pool::pool_put;
use crate::kern::subr_prf::panic;
use crate::kern::vfs_bio::biodone;
use crate::kern::vfs_cache::cache_purge;
use crate::kern::vfs_init::NAMEI_POOL;
use crate::kern::vfs_lookup::vfs_relookup;
use crate::kern::vfs_subr::{vaccess, vgone, vnoperm, vput, vref, vrele};
use crate::kern::vfs_vnops::{vn_lock, vn_rdwr};
use crate::kern::vfs_vops::{
    VOP_ABORTOP, VOP_ACCESS, VOP_BMAP, VOP_BWRITE, VOP_READ, VOP_REMOVE, VOP_STRATEGY, VOP_UNLOCK,
};
use crate::machine::cpu::curproc;
use crate::machine::intr::{splbio, splx};
use crate::sys::buf::{B_CLRBUF, B_ERROR, clrbuf};
use crate::sys::dirent::Dirent;
use crate::sys::errno::Errno;
use crate::sys::event::{
    __EV_POLL, __EV_SELECT, EV_EOF, EV_ONESHOT, EVFILT_READ, EVFILT_VNODE, EVFILT_WRITE,
    FILTEROP_ISFD, Filterops, Knote, NOTE_ATTRIB, NOTE_DELETE, NOTE_EOF, NOTE_LINK, NOTE_RENAME,
    NOTE_REVOKE, NOTE_TRUNCATE, NOTE_WRITE,
};
use crate::sys::fcntl::{FWRITE, O_APPEND, O_TRUNC};
use crate::sys::file::foffset;
use crate::sys::lock::{LK_EXCLUSIVE, LK_RWFLAGS};
use crate::sys::lockf::lf_advlock;
use crate::sys::mount::{MNT_NOATIME, MNT_RDONLY, Mount};
use crate::sys::namei::{
    Componentname, DELETE, ISDOTDOT, LOCKLEAF, LOCKPARENT, MODMASK, SAVESTART,
};
use crate::sys::param::{BLKDEV_IOSIZE, MAXBSIZE, MAXPATHLEN, PAGE_SIZE, dbtob};
use crate::sys::stat::{
    ALLPERMS, APPEND, IMMUTABLE, S_ISTXT, S_IXGRP, S_IXOTH, S_IXUSR, SF_APPEND, SF_IMMUTABLE,
    SF_SETTABLE, UF_SETTABLE,
};
use crate::sys::syslimits::{LINK_MAX, NAME_MAX};
use crate::sys::time::Timespec;
use crate::sys::types::{Dev, Gid, Mode, Nlink, Register, Uid};
use crate::sys::ucred::{FSCRED, NOCRED, Ucred};
use crate::sys::uio::{UioRw, UioSeg};
use crate::sys::unistd::{
    _PC_2_SYMLINKS, _PC_ALLOC_SIZE_MIN, _PC_CHOWN_RESTRICTED, _PC_FILESIZEBITS, _PC_LINK_MAX,
    _PC_NAME_MAX, _PC_NO_TRUNC, _PC_REC_INCR_XFER_SIZE, _PC_REC_MAX_XFER_SIZE,
    _PC_REC_MIN_XFER_SIZE, _PC_REC_XFER_ALIGN, _PC_SYMLINK_MAX, _PC_TIMESTAMP_RESOLUTION,
};
use crate::sys::vnode::{
    IO_NODELOCKED, IO_SYNC, VA_UTIMES_CHANGE, VA_UTIMES_NULL, VBAD, VBLK, VCHR, VDIR, VEXEC, VFIFO,
    VLNK, VN_KNOTE, VNON, VNOVAL, VREG, VSOCK, VTEXT, VWRITE, Vnode, VopAccessArgs, VopAdvlockArgs,
    VopCloseArgs, VopCreateArgs, VopGetattrArgs, VopIoctlArgs, VopIslockedArgs, VopKqfilterArgs,
    VopLinkArgs, VopLockArgs, VopMkdirArgs, VopMknodArgs, VopOpenArgs, VopPathconfArgs,
    VopPrintArgs, VopReadArgs, VopReaddirArgs, VopReadlinkArgs, VopRemoveArgs, VopRenameArgs,
    VopRmdirArgs, VopSetattrArgs, VopStrategyArgs, VopSymlinkArgs, VopUnlockArgs, VopWriteArgs,
    cred_ref, iftovt, makeimode,
};
use crate::ufs::ufs::dinode::{IFDIR, IFLNK, IFMT, IFREG, ISGID, ISUID, MAXSYMLINKLEN_UFS2};
use crate::ufs::ufs::dir::{DIRBLKSIZ, DT_DIR, Direct, Dirtemplate, iftodt};
use crate::ufs::ufs::inode::{
    IN_ACCESS, IN_CHANGE, IN_LAZYMOD, IN_MODIFIED, IN_RENAME, IN_UPDATE, Inode, UFS_BUF_ALLOC,
    UFS_BUFATOFF, UFS_INODE_ALLOC, UFS_INODE_FREE, UFS_TRUNCATE, UFS_UPDATE, doingasync, vtoi,
};
use crate::ufs::ufs::quota::{
    UFS_QUOTA_FORCE, UFS_QUOTA_NOGID, UFS_QUOTA_NOUID, UfsQuotaFlags, getinoquota,
    ufs_quota_alloc_blocks2, ufs_quota_alloc_inode, ufs_quota_alloc_inode2, ufs_quota_delete,
    ufs_quota_free_blocks2, ufs_quota_free_inode2,
};
use crate::ufs::ufs::ufs_lookup::{
    ufs_checkpath, ufs_dirempty, ufs_direnter, ufs_dirremove, ufs_dirrewrite, ufs_makedirentry,
};
use crate::uvm::uvm_vnode::{uvm_vnp_setsize, uvm_vnp_uncache};

/// `mastertemplate`: a virgin directory (no blushing please).
pub const MASTERTEMPLATE: Dirtemplate = Dirtemplate {
    dot_ino: 0,
    dot_reclen: 12,
    dot_type: DT_DIR,
    dot_namlen: 1,
    dot_name: *b".\0\0\0",
    dotdot_ino: 0,
    dotdot_reclen: DIRBLKSIZ as i16 - 12,
    dotdot_type: DT_DIR,
    dotdot_namlen: 2,
    dotdot_name: *b"..\0\0",
};

/// `ufsread_filtops`.
pub static UFSREAD_FILTOPS: Filterops = Filterops {
    f_flags: FILTEROP_ISFD,
    f_attach: None,
    f_detach: Some(filt_ufsdetach),
    f_event: Some(filt_ufsread),
    f_modify: None,
    f_process: None,
};

/// `ufswrite_filtops`.
pub static UFSWRITE_FILTOPS: Filterops = Filterops {
    f_flags: FILTEROP_ISFD,
    f_attach: None,
    f_detach: Some(filt_ufsdetach),
    f_event: Some(filt_ufswrite),
    f_modify: None,
    f_process: None,
};

/// `ufsvnode_filtops`.
pub static UFSVNODE_FILTOPS: Filterops = Filterops {
    f_flags: FILTEROP_ISFD,
    f_attach: None,
    f_detach: Some(filt_ufsdetach),
    f_event: Some(filt_ufsvnode),
    f_modify: None,
    f_process: None,
};

/// The vnode's mount, which a UFS vnode always has.
fn vmount(vp: &Vnode) -> &'static Mount {
    match vp.v_mount.get() {
        Some(mp) => mp,
        None => panic(format_args!("ufs: vnode {:p} without a mount", vp)),
    }
}

/// Whether the vnode's file system is mounted read-only.
fn rdonly(vp: &Vnode) -> bool {
    vmount(vp).mnt_flag.get() & MNT_RDONLY != 0
}

/// `*cred` of a credential the C dereferences: a real one (`NOCRED`/`FSCRED` panic).
pub(crate) fn ucred<'a>(cred: *const Ucred) -> &'a Ucred {
    // SAFETY: the credentials a vnode operation receives are held by its caller for the
    // operation's duration (`cred_ref`'s contract).
    match unsafe { cred_ref(cred) } {
        Some(c) => c,
        None => panic(format_args!(
            "ufs: {} credential",
            if core::ptr::eq(cred, NOCRED) {
                "missing"
            } else if core::ptr::eq(cred, FSCRED) {
                "kernel"
            } else {
                "NULL"
            }
        )),
    }
}

/// `pool_put(&namei_pool, cnp->cn_pnbuf)`: give back the pathname buffer.
pub fn pnbuf_free(cnp: &Componentname) {
    if let Some(buf) = NonNull::new(cnp.cn_pnbuf) {
        pool_put(&NAMEI_POOL, buf);
    }
}

/// `ufs_itimes`: update the times in the inode.
pub fn ufs_itimes(vp: &'static Vnode) {
    let ip = vtoi(vp);
    if ip.i_flag.get() & (IN_ACCESS | IN_CHANGE | IN_UPDATE) == 0 {
        return;
    }

    if !rdonly(vp) {
        #[cfg(feature = "ext2fs")]
        if crate::ufs::ext2fs::ext2fs_extern::is_ext2_vnode(vp) {
            ip.ext2fs_itimes();
            // goto out
            ip.clr_flag(IN_ACCESS | IN_CHANGE | IN_UPDATE);
            return;
        }

        if vp.v_type.get() == VBLK || vp.v_type.get() == VCHR {
            ip.set_flag(IN_LAZYMOD);
        } else {
            ip.set_flag(IN_MODIFIED);
        }

        let ts = getnanotime();
        if ip.i_flag.get() & IN_ACCESS != 0 {
            ip.dip_set_atime(ts.tv_sec);
            ip.dip_set_atimensec(ts.tv_nsec as i32);
        }
        if ip.i_flag.get() & IN_UPDATE != 0 {
            ip.dip_set_mtime(ts.tv_sec);
            ip.dip_set_mtimensec(ts.tv_nsec as i32);
        }
        if ip.i_flag.get() & IN_CHANGE != 0 {
            ip.dip_set_ctime(ts.tv_sec);
            ip.dip_set_ctimensec(ts.tv_nsec as i32);
            ip.i_modrev.set(ip.i_modrev.get() + 1);
        }
    }

    // out:
    ip.clr_flag(IN_ACCESS | IN_CHANGE | IN_UPDATE);
}

/// `ufs_create` (`vop_create`): create a regular file.
pub fn ufs_create(ap: &mut VopCreateArgs<'_>) -> Result<(), Errno> {
    let error = ufs_makeinode(
        makeimode(ap.a_vap.va_type, ap.a_vap.va_mode),
        ap.a_dvp,
        ap.a_vpp,
        ap.a_cnp,
    );
    if error.is_ok() {
        VN_KNOTE(ap.a_dvp, NOTE_WRITE);
    }
    error
}

/// `ufs_mknod` (`vop_mknod`): make a device special file (or a fifo, a socket's node).
pub fn ufs_mknod(ap: &mut VopMknodArgs<'_>) -> Result<(), Errno> {
    let vap = &*ap.a_vap;
    ufs_makeinode(
        makeimode(vap.va_type, vap.va_mode),
        ap.a_dvp,
        ap.a_vpp,
        ap.a_cnp,
    )?;
    VN_KNOTE(ap.a_dvp, NOTE_WRITE);
    let Some(vp) = *ap.a_vpp else {
        panic(format_args!("ufs_mknod: no vnode"));
    };
    let ip = vtoi(vp);
    ip.set_flag(IN_ACCESS | IN_CHANGE | IN_UPDATE);
    if vap.va_rdev != VNOVAL as Dev {
        // Want to be able to use this to make badblock inodes, so don't truncate the dev
        // number.
        ip.dip_set_rdev(i64::from(vap.va_rdev));
    }
    // Remove inode so that it will be reloaded by VFS_VGET and checked to see if it is an
    // alias of an existing entry in the inode cache.
    vput(vp);
    vp.v_type.set(VNON);
    vgone(vp);
    *ap.a_vpp = None;
    Ok(())
}

/// `ufs_open` (`vop_open`): nothing to do but check the append-only flag.
pub fn ufs_open(ap: &mut VopOpenArgs<'_>) -> Result<(), Errno> {
    let ip = vtoi(ap.a_vp);

    // Files marked append-only must be opened for appending.
    if ip.dip_flags() & APPEND != 0 && ap.a_mode & (FWRITE | O_APPEND) == FWRITE {
        return Err(Errno::EPERM);
    }

    if ap.a_mode & O_TRUNC != 0 {
        ip.set_flag(IN_CHANGE | IN_UPDATE);
    }

    Ok(())
}

/// `ufs_close` (`vop_close`): update the times on the inode.
pub fn ufs_close(ap: &mut VopCloseArgs<'_>) -> Result<(), Errno> {
    let vp = ap.a_vp;

    if vp.v_usecount.get() > 1 {
        ufs_itimes(vp);
    }
    Ok(())
}

/// `ufs_access` (`vop_access`).
pub fn ufs_access(ap: &mut VopAccessArgs<'_>) -> Result<(), Errno> {
    let vp = ap.a_vp;
    let ip = vtoi(vp);
    let mode = ap.a_mode;

    // Disallow write attempts on read-only file systems; unless the file is a socket, fifo,
    // or a block or character device resident on the file system.
    if mode & VWRITE != 0 {
        match vp.v_type.get() {
            VDIR | VLNK | VREG => {
                if rdonly(vp) {
                    return Err(Errno::EROFS);
                }
                getinoquota(ip)?;
            }
            VBAD | VBLK | VCHR | VSOCK | VFIFO | VNON => {}
        }
    }

    // If immutable bit set, nobody gets to write it.
    if mode & VWRITE != 0 && ip.dip_flags() & IMMUTABLE != 0 {
        return Err(Errno::EPERM);
    }

    if vnoperm(vp) {
        // For VEXEC, at least one of the execute bits must be set.
        if mode & VEXEC != 0
            && vp.v_type.get() != VDIR
            && ip.dip_mode() & (S_IXUSR | S_IXGRP | S_IXOTH) == 0
        {
            return Err(Errno::EACCES);
        }
        return Ok(());
    }

    vaccess(
        vp.v_type.get(),
        ip.dip_mode(),
        ip.dip_uid(),
        ip.dip_gid(),
        mode,
        ucred(ap.a_cred),
    )
}

/// `ufs_getattr` (`vop_getattr`): copy from inode table.
pub fn ufs_getattr(ap: &mut VopGetattrArgs<'_>) -> Result<(), Errno> {
    let vp = ap.a_vp;
    let ip = vtoi(vp);
    let vap = &mut *ap.a_vap;

    ufs_itimes(vp);

    // Copy from inode table.
    vap.va_fsid = i64::from(ip.i_dev.get());
    vap.va_fileid = u64::from(ip.i_number.get());
    vap.va_mode = ip.dip_mode() & !IFMT;
    vap.va_nlink = ip.i_effnlink.get() as Nlink;
    vap.va_uid = ip.dip_uid();
    vap.va_gid = ip.dip_gid();
    vap.va_rdev = ip.dip_rdev() as Dev;
    vap.va_size = ip.dip_size();
    vap.va_atime = Timespec::new(ip.dip_atime(), i64::from(ip.dip_atimensec()));
    vap.va_mtime = Timespec::new(ip.dip_mtime(), i64::from(ip.dip_mtimensec()));
    vap.va_ctime = Timespec::new(ip.dip_ctime(), i64::from(ip.dip_ctimensec()));
    vap.va_flags = u64::from(ip.dip_flags());
    vap.va_gen = u64::from(ip.dip_gen());
    // this doesn't belong here
    vap.va_blocksize = match vp.v_type.get() {
        VBLK => BLKDEV_IOSIZE as i64,
        VCHR => MAXBSIZE as i64,
        _ => i64::from(vmount(vp).mnt_stat.get().f_iosize),
    };
    vap.va_bytes = dbtob(ip.dip_blocks() as usize) as u64;
    vap.va_type = vp.v_type.get();
    vap.va_filerev = ip.i_modrev.get();
    Ok(())
}

/// `ufs_setattr` (`vop_setattr`): set attribute vnode op. called from several syscalls.
pub fn ufs_setattr(ap: &mut VopSetattrArgs<'_>) -> Result<(), Errno> {
    let vap = &mut *ap.a_vap;
    let vp = ap.a_vp;
    let ip = vtoi(vp);
    let cred = ap.a_cred;
    let mut hint = NOTE_ATTRIB;

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
        if c.cr_uid.get() != ip.dip_uid() && !vnoperm(vp) {
            suser_ucred(c)?;
        }
        if c.cr_uid.get() == 0 || vnoperm(vp) {
            if ip.dip_flags() & (SF_IMMUTABLE | SF_APPEND) != 0
                && SECURELEVEL.load(Ordering::Relaxed) > 0
            {
                return Err(Errno::EPERM);
            }
            ip.dip_set_flags(vap.va_flags as u32);
        } else {
            if ip.dip_flags() & (SF_IMMUTABLE | SF_APPEND) != 0
                || (vap.va_flags as u32 & UF_SETTABLE) as u64 != vap.va_flags
            {
                return Err(Errno::EPERM);
            }
            ip.dip_set_flags(ip.dip_flags() & SF_SETTABLE);
            ip.dip_set_flags(ip.dip_flags() | (vap.va_flags as u32 & UF_SETTABLE));
        }
        ip.set_flag(IN_CHANGE);
        if vap.va_flags as u32 & (IMMUTABLE | APPEND) != 0 {
            return Ok(());
        }
    }
    if ip.dip_flags() & (IMMUTABLE | APPEND) != 0 {
        return Err(Errno::EPERM);
    }
    // Go through the fields and update if not VNOVAL.
    if vap.va_uid != VNOVAL as Uid || vap.va_gid != VNOVAL as Gid {
        if rdonly(vp) {
            return Err(Errno::EROFS);
        }
        ufs_chown(vp, vap.va_uid, vap.va_gid, cred)?;
    }
    if vap.va_size != VNOVAL as u64 {
        let oldsize = ip.dip_size();
        // Disallow write attempts on read-only file systems; unless the file is a socket,
        // fifo, or a block or character device resident on the file system.
        match vp.v_type.get() {
            VDIR => return Err(Errno::EISDIR),
            VLNK | VREG if rdonly(vp) => return Err(Errno::EROFS),
            _ => {}
        }
        UFS_TRUNCATE(ip, vap.va_size as i64, 0, cred)?;
        if vap.va_size < oldsize {
            hint |= NOTE_TRUNCATE;
        }
    }
    if vap.va_vaflags & VA_UTIMES_CHANGE != 0
        || vap.va_atime.tv_nsec != i64::from(VNOVAL)
        || vap.va_mtime.tv_nsec != i64::from(VNOVAL)
    {
        if rdonly(vp) {
            return Err(Errno::EROFS);
        }
        let c = ucred(cred);
        if c.cr_uid.get() != ip.dip_uid()
            && !vnoperm(vp)
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
        ufs_itimes(vp);
        if vap.va_mtime.tv_nsec != i64::from(VNOVAL) {
            ip.dip_set_mtime(vap.va_mtime.tv_sec);
            ip.dip_set_mtimensec(vap.va_mtime.tv_nsec as i32);
        }
        if vap.va_atime.tv_nsec != i64::from(VNOVAL) {
            ip.dip_set_atime(vap.va_atime.tv_sec);
            ip.dip_set_atimensec(vap.va_atime.tv_nsec as i32);
        }
        UFS_UPDATE(ip, 0)?;
    }
    let mut error = Ok(());
    if vap.va_mode != VNOVAL as Mode {
        if rdonly(vp) {
            return Err(Errno::EROFS);
        }
        error = ufs_chmod(vp, vap.va_mode as i32, cred);
    }
    VN_KNOTE(vp, hint);
    error
}

/// `ufs_chmod`: change the mode on a file. Inode must be locked before calling.
pub fn ufs_chmod(vp: &'static Vnode, mode: i32, cred: *const Ucred) -> Result<(), Errno> {
    let ip = vtoi(vp);
    let c = ucred(cred);
    let mode = mode as Mode;

    if c.cr_uid.get() != ip.dip_uid() && !vnoperm(vp) {
        suser_ucred(c)?;
    }
    if c.cr_uid.get() != 0 && !vnoperm(vp) {
        if vp.v_type.get() != VDIR && mode & S_ISTXT != 0 {
            return Err(Errno::EFTYPE);
        }
        if !groupmember(ip.dip_gid(), c) && mode & ISGID != 0 {
            return Err(Errno::EPERM);
        }
    }
    ip.dip_set_mode(ip.dip_mode() & !ALLPERMS);
    ip.dip_set_mode(ip.dip_mode() | (mode & ALLPERMS));
    ip.set_flag(IN_CHANGE);
    if vp.v_flag.get() & VTEXT != 0 && ip.dip_mode() & S_ISTXT == 0 {
        let _ = uvm_vnp_uncache(vp);
    }
    Ok(())
}

/// `ufs_chown`: perform chown operation on inode ip; inode must be locked prior to call.
pub fn ufs_chown(vp: &'static Vnode, uid: Uid, gid: Gid, cred: *const Ucred) -> Result<(), Errno> {
    let ip = vtoi(vp);
    let c = ucred(cred);
    let mut quota_flags: UfsQuotaFlags = 0;

    let uid = if uid == VNOVAL as Uid {
        ip.dip_uid()
    } else {
        uid
    };
    let gid = if gid == VNOVAL as Gid {
        ip.dip_gid()
    } else {
        gid
    };
    // If we don't own the file, are trying to change the owner of the file, or are not a
    // member of the target group, the caller must be superuser or the call fails.
    if (c.cr_uid.get() != ip.dip_uid()
        || uid != ip.dip_uid()
        || (gid != ip.dip_gid() && !groupmember(gid, c)))
        && !vnoperm(vp)
    {
        suser_ucred(c)?;
    }
    let ogid = ip.dip_gid();
    let ouid = ip.dip_uid();
    let change = ip.dip_blocks();

    if ouid == uid {
        quota_flags |= UFS_QUOTA_NOUID;
    }

    if ogid == gid {
        quota_flags |= UFS_QUOTA_NOGID;
    }

    getinoquota(ip)?;
    let _ = ufs_quota_free_blocks2(ip, change, cred, quota_flags);
    let _ = ufs_quota_free_inode2(ip, cred, quota_flags);
    let _ = ufs_quota_delete(ip);

    ip.dip_set_gid(gid);
    ip.dip_set_uid(uid);

    let error: Result<(), Errno> = 'error: {
        if let Err(e) = getinoquota(ip) {
            break 'error Err(e);
        }

        if let Err(e) = ufs_quota_alloc_blocks2(ip, change, cred, quota_flags) {
            break 'error Err(e);
        }

        if let Err(e) = ufs_quota_alloc_inode2(ip, cred, quota_flags) {
            let _ = ufs_quota_free_blocks2(ip, change, cred, quota_flags);
            break 'error Err(e);
        }

        if getinoquota(ip).is_err() {
            panic(format_args!("chown: lost quota"));
        }

        if ouid != uid || ogid != gid {
            ip.set_flag(IN_CHANGE);
        }
        if !vnoperm(vp) {
            if ouid != uid && c.cr_uid.get() != 0 {
                ip.dip_set_mode(ip.dip_mode() & !ISUID);
            }
            if ogid != gid && c.cr_uid.get() != 0 {
                ip.dip_set_mode(ip.dip_mode() & !ISGID);
            }
        }
        return Ok(());
    };

    // error:
    let _ = ufs_quota_delete(ip);

    ip.dip_set_gid(ogid);
    ip.dip_set_uid(ouid);

    if getinoquota(ip).is_ok() {
        let _ = ufs_quota_alloc_blocks2(ip, change, cred, quota_flags | UFS_QUOTA_FORCE);
        let _ = ufs_quota_alloc_inode2(ip, cred, quota_flags | UFS_QUOTA_FORCE);
        let _ = getinoquota(ip);
    }
    error
}

/// `ufs_ioctl` (`vop_ioctl`): no ioctls on UFS files.
pub fn ufs_ioctl(_ap: &mut VopIoctlArgs<'_>) -> Result<(), Errno> {
    Err(Errno::ENOTTY)
}

/// `ufs_remove` (`vop_remove`): remove the entry of a file that is not a directory.
pub fn ufs_remove(ap: &mut VopRemoveArgs<'_>) -> Result<(), Errno> {
    let vp = ap.a_vp;
    let dvp = ap.a_dvp;

    let ip = vtoi(vp);
    if vp.v_type.get() == VDIR
        || ip.dip_flags() & (IMMUTABLE | APPEND) != 0
        || vtoi(dvp).dip_flags() & APPEND != 0
    {
        return Err(Errno::EPERM);
    }
    let error = ufs_dirremove(dvp, Some(ip), ap.a_cnp.cn_flags, 0);
    VN_KNOTE(vp, NOTE_DELETE);
    VN_KNOTE(dvp, NOTE_WRITE);
    error
}

/// `ufs_link` (`vop_link`): link vnode call.
pub fn ufs_link(ap: &mut VopLinkArgs<'_>) -> Result<(), Errno> {
    let dvp = ap.a_dvp;
    let vp = ap.a_vp;
    let cnp = &mut *ap.a_cnp;

    #[cfg(feature = "diagnostic")]
    if cnp.cn_flags & crate::sys::namei::HASBUF == 0 {
        panic(format_args!("ufs_link: no name"));
    }
    let error: Result<(), Errno> = 'out2: {
        if !core::ptr::eq(dvp, vp)
            && let Err(e) = vn_lock(vp, LK_EXCLUSIVE)
        {
            let _ = VOP_ABORTOP(dvp, cnp);
            break 'out2 Err(e);
        }
        let ip = vtoi(vp);
        let error: Result<(), Errno> = 'out1: {
            if ip.dip_nlink() as Nlink >= LINK_MAX {
                let _ = VOP_ABORTOP(dvp, cnp);
                break 'out1 Err(Errno::EMLINK);
            }
            if ip.dip_flags() & (IMMUTABLE | APPEND) != 0 {
                let _ = VOP_ABORTOP(dvp, cnp);
                break 'out1 Err(Errno::EPERM);
            }
            ip.i_effnlink.set(ip.i_effnlink.get() + 1);
            ip.dip_set_nlink(ip.dip_nlink() + 1);
            ip.set_flag(IN_CHANGE);
            let mut error = UFS_UPDATE(ip, 1);
            if error.is_ok() {
                let mut newdir = Direct::new();
                ufs_makedirentry(ip, cnp, &mut newdir);
                error = ufs_direnter(dvp, Some(vp), &mut newdir, cnp, None);
            }
            if error.is_err() {
                ip.i_effnlink.set(ip.i_effnlink.get() - 1);
                ip.dip_set_nlink(ip.dip_nlink() - 1);
                ip.set_flag(IN_CHANGE);
            }
            pnbuf_free(cnp);
            VN_KNOTE(vp, NOTE_LINK);
            VN_KNOTE(dvp, NOTE_WRITE);
            error
        };
        // out1:
        if !core::ptr::eq(dvp, vp) {
            let _ = VOP_UNLOCK(vp);
        }
        error
    };
    // out2:
    vput(dvp);
    error
}

/// How `ufs_rename` leaves: the C's `bad:` and `out:` labels.
enum RenameExit {
    /// `goto bad`: release the target directory (and target) first.
    Bad(Errno),
    /// `goto out`.
    Out(Errno),
}

/// `abortit:` of `ufs_rename`: abort both lookups and release every vnode.
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
    let _ = VOP_ABORTOP(tdvp, tcnp);
    if tvp.is_some_and(|t| core::ptr::eq(t, tdvp)) {
        vrele(tdvp);
    } else {
        vput(tdvp);
    }
    if let Some(tvp) = tvp {
        vput(tvp);
    }
    let _ = VOP_ABORTOP(fdvp, fcnp);
    vrele(fdvp);
    vrele(fvp);
    Err(error)
}

/// `ufs_rename` (`vop_rename`): rename system call.
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
pub fn ufs_rename(ap: &mut VopRenameArgs<'_>) -> Result<(), Errno> {
    let mut tvp = ap.a_tvp;
    let tdvp = ap.a_tdvp;
    let mut fvp = ap.a_fvp;
    let fdvp = ap.a_fdvp;
    let tcnp = &mut *ap.a_tcnp;
    let fcnp = &mut *ap.a_fcnp;
    let mut doingdirectory = false;
    let mut oldparent = 0;
    let mut newparent = 0;

    #[cfg(feature = "diagnostic")]
    if tcnp.cn_flags & crate::sys::namei::HASBUF == 0
        || fcnp.cn_flags & crate::sys::namei::HASBUF == 0
    {
        panic(format_args!("ufs_rename: no name"));
    }
    // Check for cross-device rename.
    let fmp = fvp.v_mount.get().map(core::ptr::from_ref);
    if fmp != tdvp.v_mount.get().map(core::ptr::from_ref)
        || tvp.is_some_and(|t| fmp != t.v_mount.get().map(core::ptr::from_ref))
    {
        return rename_abortit(Errno::EXDEV, tdvp, tvp, tcnp, fdvp, fvp, fcnp);
    }

    if let Some(t) = tvp
        && (vtoi(t).dip_flags() & (IMMUTABLE | APPEND) != 0 || vtoi(tdvp).dip_flags() & APPEND != 0)
    {
        return rename_abortit(Errno::EPERM, tdvp, tvp, tcnp, fdvp, fvp, fcnp);
    }

    // Check if just deleting a link name or if we've lost a race. If another process
    // completes the same rename after we've looked up the source and have blocked looking up
    // the target, then the source and target inodes may be identical now although the names
    // were never linked.
    if let Some(t) = tvp
        && core::ptr::eq(fvp, t)
    {
        if fvp.v_type.get() == VDIR {
            // Linked directories are impossible, so we must have lost the race. Pretend
            // that the rename completed before the lookup.
            return rename_abortit(Errno::ENOENT, tdvp, tvp, tcnp, fdvp, fvp, fcnp);
        }

        // Release destination completely.
        let _ = VOP_ABORTOP(tdvp, tcnp);
        vput(tdvp);
        vput(t);

        // Delete source. There is another race now that everything is unlocked, but this
        // doesn't cause any new complications. relookup() may find a file that is unrelated
        // to the original one, or it may fail. Too bad.
        vrele(fvp);
        fcnp.cn_flags &= !MODMASK;
        fcnp.cn_flags |= LOCKPARENT | LOCKLEAF;
        if fcnp.cn_flags & SAVESTART == 0 {
            panic(format_args!("ufs_rename: lost from startdir"));
        }
        fcnp.cn_nameiop = DELETE;
        let mut nfvp = None;
        vfs_relookup(fdvp, &mut nfvp, fcnp)?; // relookup did vrele()
        vrele(fdvp);
        // A successful DELETE lookup of the last component returns its vnode.
        let Some(nfvp) = nfvp else {
            return Err(Errno::ENOENT);
        };
        return VOP_REMOVE(fdvp, nfvp, fcnp);
    }

    if let Err(e) = vn_lock(fvp, LK_EXCLUSIVE) {
        return rename_abortit(e, tdvp, tvp, tcnp, fdvp, fvp, fcnp);
    }

    // fvp, tdvp, tvp now locked
    let mut dp = vtoi(fdvp);
    let ip = vtoi(fvp);
    if ip.dip_nlink() as Nlink >= LINK_MAX {
        let _ = VOP_UNLOCK(fvp);
        return rename_abortit(Errno::EMLINK, tdvp, tvp, tcnp, fdvp, fvp, fcnp);
    }
    if ip.dip_flags() & (IMMUTABLE | APPEND) != 0 || dp.dip_flags() & APPEND != 0 {
        let _ = VOP_UNLOCK(fvp);
        return rename_abortit(Errno::EPERM, tdvp, tvp, tcnp, fdvp, fvp, fcnp);
    }
    if ip.dip_mode() & IFMT == IFDIR {
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
            || core::ptr::eq(dp, ip)
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
    VN_KNOTE(fdvp, NOTE_WRITE); // XXX right place?

    // When the target exists, both the directory and target vnodes are returned locked.
    dp = vtoi(tdvp);
    let mut xp: Option<&Inode> = tvp.map(vtoi);

    let exit: Result<(), RenameExit> = 'body: {
        // 1) Bump link count while we're moving stuff around. If we crash somewhere before
        //    completing our work, the link count may be wrong, but correctable.
        ip.i_effnlink.set(ip.i_effnlink.get() + 1);
        ip.dip_set_nlink(ip.dip_nlink() + 1);
        ip.set_flag(IN_CHANGE);
        if let Err(e) = UFS_UPDATE(ip, 1) {
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

        // tdvp and tvp locked
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
            // Compensate for the reference ufs_checkpath() loses.
            vref(tdvp);
            // Only tdvp is locked
            if let Err(e) = ufs_checkpath(ip, dp, tcnp.cn_cred) {
                vrele(tdvp);
                break 'body Err(RenameExit::Out(e));
            }
            if tcnp.cn_flags & SAVESTART == 0 {
                panic(format_args!("ufs_rename: lost to startdir"));
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
                    if dp.dip_nlink() as Nlink >= LINK_MAX {
                        break 'body Err(RenameExit::Bad(Errno::EMLINK));
                    }
                    dp.i_effnlink.set(dp.i_effnlink.get() + 1);
                    dp.dip_set_nlink(dp.dip_nlink() + 1);
                    dp.set_flag(IN_CHANGE);
                    if let Err(e) = UFS_UPDATE(dp, 1) {
                        dp.i_effnlink.set(dp.i_effnlink.get() - 1);
                        dp.dip_set_nlink(dp.dip_nlink() - 1);
                        dp.set_flag(IN_CHANGE);
                        break 'body Err(RenameExit::Bad(e));
                    }
                }
                let mut newdir = Direct::new();
                ufs_makedirentry(ip, tcnp, &mut newdir);
                if let Err(e) = ufs_direnter(tdvp, None, &mut newdir, tcnp, None) {
                    if doingdirectory && newparent != 0 {
                        dp.i_effnlink.set(dp.i_effnlink.get() - 1);
                        dp.dip_set_nlink(dp.dip_nlink() - 1);
                        dp.set_flag(IN_CHANGE);
                        let _ = UFS_UPDATE(dp, 1);
                    }
                    break 'body Err(RenameExit::Bad(e));
                }
                VN_KNOTE(tdvp, NOTE_WRITE);
                vput(tdvp);
            }
            Some(x) => {
                if x.i_dev.get() != dp.i_dev.get() || x.i_dev.get() != ip.i_dev.get() {
                    panic(format_args!("rename: EXDEV"));
                }
                // Short circuit rename(foo, foo).
                if x.i_number.get() == ip.i_number.get() {
                    panic(format_args!("ufs_rename: same file"));
                }
                // If the parent directory is "sticky", then the user must own the parent
                // directory, or the destination of the rename, otherwise the destination may
                // not be changed (except by root). This implements append-only directories.
                let tuid = ucred(tcnp.cn_cred).cr_uid.get();
                if dp.dip_mode() & S_ISTXT != 0
                    && tuid != 0
                    && tuid != dp.dip_uid()
                    && x.dip_uid() != tuid
                    && !vnoperm(tdvp)
                {
                    break 'body Err(RenameExit::Bad(Errno::EPERM));
                }
                // Target must be empty if a directory and have no links to it. Also, ensure
                // source and target are compatible (both directories, or both not
                // directories).
                if x.dip_mode() & IFMT == IFDIR {
                    if x.i_effnlink.get() > 2 || !ufs_dirempty(x, dp.i_number.get(), tcnp.cn_cred) {
                        break 'body Err(RenameExit::Bad(Errno::ENOTEMPTY));
                    }
                    if !doingdirectory {
                        break 'body Err(RenameExit::Bad(Errno::ENOTDIR));
                    }
                    cache_purge(tdvp);
                } else if doingdirectory {
                    break 'body Err(RenameExit::Bad(Errno::EISDIR));
                }

                let isrmdir = if doingdirectory && newparent != 0 {
                    newparent as i32
                } else {
                    i32::from(doingdirectory)
                };
                if let Err(e) =
                    ufs_dirrewrite(dp, x, ip.i_number.get(), iftodt(ip.dip_mode()), isrmdir)
                {
                    break 'body Err(RenameExit::Bad(e));
                }
                if doingdirectory {
                    if newparent == 0 {
                        dp.i_effnlink.set(dp.i_effnlink.get() - 1);
                    }
                    x.i_effnlink.set(x.i_effnlink.get() - 1);
                }
                if doingdirectory {
                    // Truncate inode. The only stuff left in the directory is "." and "..".
                    // The "." reference is inconsequential since we are quashing it. We have
                    // removed the "." reference and the reference in the parent directory,
                    // but there may be other hard links. The soft dependency code will
                    // arrange to do these operations after the parent directory entry has
                    // been deleted on disk, so when running with that code we avoid doing
                    // them now.
                    if newparent == 0 {
                        dp.dip_set_nlink(dp.dip_nlink() - 1);
                        dp.set_flag(IN_CHANGE);
                    }

                    x.dip_set_nlink(x.dip_nlink() - 1);
                    x.set_flag(IN_CHANGE);
                    let Some(t) = tvp else {
                        panic(format_args!("ufs_rename: no target vnode"));
                    };
                    if let Err(e) = UFS_TRUNCATE(vtoi(t), 0, IO_SYNC, tcnp.cn_cred) {
                        break 'body Err(RenameExit::Bad(e));
                    }
                }
                VN_KNOTE(tdvp, NOTE_WRITE);
                vput(tdvp);
                if let Some(t) = tvp {
                    VN_KNOTE(t, NOTE_DELETE);
                    vput(t);
                }
                // xp = NULL: step 3 looks the source up again.
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
            return rename_out(e, fdvp, fvp, ip, doingdirectory);
        }
        Err(RenameExit::Out(e)) => {
            return rename_out(e, fdvp, fvp, ip, doingdirectory);
        }
    }

    // 3) Unlink the source.
    fcnp.cn_flags &= !MODMASK;
    fcnp.cn_flags |= LOCKPARENT | LOCKLEAF;
    if fcnp.cn_flags & SAVESTART == 0 {
        panic(format_args!("ufs_rename: lost from startdir"));
    }
    let mut nfvp = None;
    if let Err(e) = vfs_relookup(fdvp, &mut nfvp, fcnp) {
        vrele(ap.a_fvp);
        return Err(e);
    }
    vrele(fdvp);
    let Some(nfvp) = nfvp else {
        // From name has disappeared.
        if doingdirectory {
            panic(format_args!("ufs_rename: lost dir entry"));
        }
        vrele(ap.a_fvp);
        return Ok(());
    };
    fvp = nfvp;

    let xp = vtoi(fvp);
    let dp = vtoi(fdvp);

    // Ensure that the directory entry still exists and has not changed while the new name
    // has been entered. If the source is a file then the entry may have been unlinked or
    // renamed. In either case there is no further work to be done. If the source is a
    // directory then it cannot have been rmdir'ed; the IN_RENAME flag ensures that it cannot
    // be moved by another rename or removed by a rmdir.
    let mut error = Ok(());
    if !core::ptr::eq(xp, ip) {
        if doingdirectory {
            panic(format_args!("ufs_rename: lost dir entry"));
        }
    } else {
        // If the source is a directory with a new parent, the link count of the old parent
        // directory must be decremented and ".." set to point to the new parent.
        if doingdirectory && newparent != 0 {
            xp.i_offset.set(i32::from(MASTERTEMPLATE.dot_reclen));
            let _ = ufs_dirrewrite(xp, dp, newparent, DT_DIR, 0);
            cache_purge(fdvp);
        }
        error = ufs_dirremove(fdvp, Some(xp), fcnp.cn_flags, 0);
        xp.clr_flag(IN_RENAME);
    }
    VN_KNOTE(fvp, NOTE_RENAME);
    vput(fdvp);
    vput(fvp);
    vrele(ap.a_fvp);
    error
}

/// `out:` of `ufs_rename`: release the source directory and undo the link count bump.
fn rename_out(
    error: Errno,
    fdvp: &'static Vnode,
    fvp: &'static Vnode,
    ip: &Inode,
    doingdirectory: bool,
) -> Result<(), Errno> {
    vrele(fdvp);
    if doingdirectory {
        ip.clr_flag(IN_RENAME);
    }
    if vn_lock(fvp, LK_EXCLUSIVE).is_ok() {
        ip.i_effnlink.set(ip.i_effnlink.get() - 1);
        ip.dip_set_nlink(ip.dip_nlink() - 1);
        ip.set_flag(IN_CHANGE);
        ip.clr_flag(IN_RENAME);
        vput(fvp);
    } else {
        vrele(fvp);
    }
    Err(error)
}

/// `ufs_mkdir` (`vop_mkdir`): mkdir system call.
pub fn ufs_mkdir(ap: &mut VopMkdirArgs<'_>) -> Result<(), Errno> {
    let dvp = ap.a_dvp;
    let vap = &*ap.a_vap;
    let cnp = &mut *ap.a_cnp;

    #[cfg(feature = "diagnostic")]
    if cnp.cn_flags & crate::sys::namei::HASBUF == 0 {
        panic(format_args!("ufs_mkdir: no name"));
    }
    let dp = vtoi(dvp);
    let error: Result<(), Errno> = 'out: {
        if dp.dip_nlink() as Nlink >= LINK_MAX {
            break 'out Err(Errno::EMLINK);
        }
        let dmode = (vap.va_mode & 0o777) | IFDIR;
        // Must simulate part of ufs_makeinode here to acquire the inode, but not have it
        // entered in the parent directory. The entry is made later after writing "." and
        // ".." entries.
        let tvp = match UFS_INODE_ALLOC(dp, dmode, cnp.cn_cred) {
            Ok(tvp) => tvp,
            Err(e) => break 'out Err(e),
        };

        let ip = vtoi(tvp);

        ip.dip_set_uid(ucred(cnp.cn_cred).cr_uid.get());
        ip.dip_set_gid(dp.dip_gid());

        if let Err(e) = getinoquota(ip).and_then(|()| ufs_quota_alloc_inode(ip, cnp.cn_cred)) {
            pnbuf_free(cnp);
            let _ = UFS_INODE_FREE(ip, ip.i_number.get(), dmode);
            vput(tvp);
            vput(dvp);
            return Err(e);
        }

        ip.set_flag(IN_ACCESS | IN_CHANGE | IN_UPDATE);
        ip.dip_set_mode(dmode);
        tvp.v_type.set(VDIR); // Rest init'd in getnewvnode().
        ip.i_effnlink.set(2);
        ip.dip_set_nlink(2);

        let error: Result<(), Errno> = 'bad: {
            // Bump link count in parent directory to reflect work done below. Should be done
            // before reference is create so cleanup is possible if we crash.
            dp.i_effnlink.set(dp.i_effnlink.get() + 1);
            dp.dip_set_nlink(dp.dip_nlink() + 1);
            dp.set_flag(IN_CHANGE);
            if let Err(e) = UFS_UPDATE(dp, 1) {
                break 'bad Err(e);
            }

            // Initialize directory with "." and ".." from static template.
            let mut dirtemplate = MASTERTEMPLATE;
            dirtemplate.dot_ino = ip.i_number.get();
            dirtemplate.dotdot_ino = dp.i_number.get();

            let bp = match UFS_BUF_ALLOC(ip, 0, DIRBLKSIZ as i32, cnp.cn_cred, B_CLRBUF) {
                Ok(bp) => bp,
                Err(e) => break 'bad Err(e),
            };
            ip.dip_set_size(DIRBLKSIZ as u64);
            ip.set_flag(IN_CHANGE | IN_UPDATE);
            uvm_vnp_setsize(tvp, ip.dip_size() as i64);
            // SAFETY: the buffer is ours (busy from UFS_BUF_ALLOC) and mapped; the slice
            // dies before it is written.
            (unsafe { bp.data() })[..Dirtemplate::SIZE].copy_from_slice(&dirtemplate.to_bytes());
            if let Err(e) = UFS_UPDATE(ip, 1) {
                let _ = VOP_BWRITE(bp);
                break 'bad Err(e);
            }

            // Directory set up, now install its entry in the parent directory.
            //
            // If we are not doing soft dependencies, then we must write out the buffer
            // containing the new directory body before entering the new name in the
            // parent. If we are doing soft dependencies, then the buffer containing the new
            // directory body will be passed to and released in the soft dependency code
            // after the code has attached an appropriate ordering dependency to the buffer
            // which ensures that the buffer is written before the new name is written in the
            // parent.
            if let Err(e) = VOP_BWRITE(bp) {
                break 'bad Err(e);
            }
            let mut newdir = Direct::new();
            ufs_makedirentry(ip, cnp, &mut newdir);
            ufs_direnter(dvp, Some(tvp), &mut newdir, cnp, Some(bp))
        };

        // bad:
        if error.is_ok() {
            VN_KNOTE(dvp, NOTE_WRITE | NOTE_LINK);
            *ap.a_vpp = Some(tvp);
        } else {
            dp.i_effnlink.set(dp.i_effnlink.get() - 1);
            dp.dip_set_nlink(dp.dip_nlink() - 1);
            dp.set_flag(IN_CHANGE);
            // No need to do an explicit VOP_TRUNCATE here, vrele will do this for us because
            // we set the link count to 0.
            ip.i_effnlink.set(0);
            ip.dip_set_nlink(0);
            ip.set_flag(IN_CHANGE);
            vput(tvp);
        }
        error
    };
    // out:
    pnbuf_free(cnp);
    vput(dvp);

    error
}

/// `ufs_rmdir` (`vop_rmdir`): rmdir system call.
pub fn ufs_rmdir(ap: &mut VopRmdirArgs<'_>) -> Result<(), Errno> {
    let vp = ap.a_vp;
    let dvp = ap.a_dvp;
    let cnp = &mut *ap.a_cnp;

    let ip = vtoi(vp);
    let dp = vtoi(dvp);
    // Do not remove a directory that is in the process of being renamed. Verify the
    // directory is empty (and valid). Rmdir ".." will not be valid since ".." will contain a
    // reference to the current directory and thus be non-empty.
    let error: Result<(), Errno> = 'out: {
        if ip.i_flag.get() & IN_RENAME != 0 {
            break 'out Err(Errno::EINVAL);
        }
        if ip.i_effnlink.get() != 2 || !ufs_dirempty(ip, dp.i_number.get(), cnp.cn_cred) {
            break 'out Err(Errno::ENOTEMPTY);
        }
        if dp.dip_flags() & APPEND != 0 || ip.dip_flags() & (IMMUTABLE | APPEND) != 0 {
            break 'out Err(Errno::EPERM);
        }
        // Delete reference to directory before purging inode. If we crash in between, the
        // directory will be reattached to lost+found,
        dp.i_effnlink.set(dp.i_effnlink.get() - 1);
        ip.i_effnlink.set(ip.i_effnlink.get() - 1);
        if let Err(e) = ufs_dirremove(dvp, Some(ip), cnp.cn_flags, 1) {
            dp.i_effnlink.set(dp.i_effnlink.get() + 1);
            ip.i_effnlink.set(ip.i_effnlink.get() + 1);
            break 'out Err(e);
        }

        VN_KNOTE(dvp, NOTE_WRITE | NOTE_LINK);
        cache_purge(dvp);
        // Truncate inode. The only stuff left in the directory is "." and "..". The "."
        // reference is inconsequential since we are quashing it.
        dp.dip_set_nlink(dp.dip_nlink() - 1);
        dp.set_flag(IN_CHANGE);
        ip.dip_set_nlink(ip.dip_nlink() - 1);
        ip.set_flag(IN_CHANGE);
        let error = UFS_TRUNCATE(ip, 0, if doingasync(vp) { 0 } else { IO_SYNC }, cnp.cn_cred);

        cache_purge(vp);
        // Kill any active hash; i_effnlink == 0, so it will not come back.
        #[cfg(feature = "ufs_dirhash")]
        if ip.i_dirhash.get().is_some() {
            crate::ufs::ufs::ufs_dirhash::ufsdirhash_free(ip);
        }
        error
    };
    // out:
    VN_KNOTE(vp, NOTE_DELETE);
    vput(dvp);
    vput(vp);
    error
}

/// `ufs_symlink` (`vop_symlink`): make a symbolic link.
pub fn ufs_symlink(ap: &mut VopSymlinkArgs<'_>) -> Result<(), Errno> {
    let vpp = &mut *ap.a_vpp;

    if let Err(e) = ufs_makeinode(IFLNK | ap.a_vap.va_mode, ap.a_dvp, vpp, ap.a_cnp) {
        vput(ap.a_dvp);
        return Err(e);
    }
    VN_KNOTE(ap.a_dvp, NOTE_WRITE);
    vput(ap.a_dvp);
    let Some(vp) = *vpp else {
        panic(format_args!("ufs_symlink: no vnode"));
    };
    let ip = vtoi(vp);
    let target = ap.a_target;
    let len = target.len();
    let error = if len < ip.ump().um_maxsymlinklen.get() as usize {
        ip.with_shortlink(|s| s[..len].copy_from_slice(target));
        ip.dip_set_size(len as u64);
        ip.set_flag(IN_CHANGE | IN_UPDATE);
        Ok(())
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
    vput(vp);
    error
}

/// `ufs_readdir` (`vop_readdir`): vnode op for reading directories. This routine converts
/// the on-disk `struct direct` entries to the `struct dirent` entries expected by userland
/// and the rest of the kernel.
pub fn ufs_readdir(ap: &mut VopReaddirArgs<'_, '_>) -> Result<(), Errno> {
    let uio = &mut *ap.a_uio;
    let vp = ap.a_vp;
    let ip = vtoi(vp);
    // u: a struct dirent, rounded up to 8 bytes.
    let mut u = [0u8; size_of::<Dirent>().next_multiple_of(8)];
    const NAME_OFF: usize = Dirent::NAME_OFFSET;
    const D_NAME: usize = crate::ufs::ufs::dir::Direct::NAME_OFFSET;

    if uio.uio_rw != UioRw::UIO_READ {
        return Err(Errno::EINVAL);
    }

    let count = uio.uio_resid;
    let entries = (uio.uio_offset as usize + count) & (DIRBLKSIZ - 1);

    // Make sure we don't return partial entries.
    if count <= entries {
        return Err(Errno::EINVAL);
    }

    // read from disk, stopping on a block boundary, max 64kB
    let mut readcnt = count.min(64 * 1024) as i64 - entries as i64;

    let mut off = uio.uio_offset;
    let mut done = false;
    let mut error: Result<(), Errno> = Ok(());

    while error.is_ok() && !done && readcnt > 0 {
        let bytesinfile = ip.dip_size() as i64 - off;
        if bytesinfile <= 0 {
            break;
        }
        let (bp, boff) = match UFS_BUFATOFF(ip, off) {
            Ok(r) => r,
            Err(e) => {
                error = Err(e);
                break;
            }
        };
        let size = bp.b_bcount.get() - bp.b_resid.get() as i64 - boff as i64;
        if size <= 0 {
            crate::kern::vfs_bio::brelse(bp);
            break;
        }

        let mut xfersize = readcnt;
        if size < xfersize {
            xfersize = size;
        }
        if bytesinfile < xfersize {
            xfersize = bytesinfile;
        }

        {
            // SAFETY: the buffer is ours (busy from UFS_BUFATOFF) and mapped; the slice dies
            // before it is released.
            let dirbuf = &unsafe { bp.data() }[boff..];
            let edp = xfersize as usize;
            // Convert and copy back the on-disk struct direct format to the user-space
            // struct dirent format, one entry at a time.
            let mut dp = 0usize;

            // While
            //  - we haven't failed to read or uiomove()
            //  - there's space in the read buf for the head of an entry
            //  - that entry has a valid d_reclen, and
            //  - there's space for the *entire* entry
            // then we're good to process this one.
            while error.is_ok()
                && dp + D_NAME < edp
                && usize::from(crate::ufs::ufs::dir::d_reclen(dirbuf, dp)) > D_NAME
                && dp + usize::from(crate::ufs::ufs::dir::d_reclen(dirbuf, dp)) <= edp
            {
                let reclen = usize::from(crate::ufs::ufs::dir::d_reclen(dirbuf, dp));
                let namlen = usize::from(crate::ufs::ufs::dir::d_namlen(dirbuf, dp));
                let d_reclen = (namlen + 1 + NAME_OFF).next_multiple_of(8);
                if d_reclen > uio.uio_resid {
                    done = true;
                    break;
                }
                off += reclen as i64;
                u[0..8].copy_from_slice(
                    &u64::from(crate::ufs::ufs::dir::d_ino(dirbuf, dp)).to_ne_bytes(),
                );
                u[8..16].copy_from_slice(&off.to_ne_bytes());
                u[16..18].copy_from_slice(&(d_reclen as u16).to_ne_bytes());
                u[18] = crate::ufs::ufs::dir::d_type(dirbuf, dp);
                u[19] = namlen as u8;
                let name = crate::ufs::ufs::dir::d_name(dirbuf, dp, namlen);
                u[NAME_OFF..NAME_OFF + name.len()].copy_from_slice(name);
                u[NAME_OFF + name.len()..d_reclen].fill(0);

                if name.contains(&b'/') {
                    error = Err(Errno::EINVAL);
                    break;
                }

                error = uiomove(&mut u[..d_reclen], uio);
                dp += reclen;
            }

            // If there was room for an entry in what we read but its d_reclen is bogus, fail
            if dp + D_NAME < edp
                && usize::from(crate::ufs::ufs::dir::d_reclen(dirbuf, dp)) <= D_NAME
            {
                error = Err(Errno::EIO);
            }
        }

        crate::kern::vfs_bio::brelse(bp);
        readcnt -= xfersize;
    }

    uio.uio_offset = off;
    *ap.a_eofflag = i32::from(ip.dip_size() as i64 <= off);

    if vmount(vp).mnt_flag.get() & MNT_NOATIME == 0
        || ip.i_flag.get() & (IN_CHANGE | IN_UPDATE) != 0
    {
        ip.set_flag(IN_ACCESS);
    }

    error
}

/// `ufs_readlink` (`vop_readlink`): return target name of a symbolic link.
pub fn ufs_readlink(ap: &mut VopReadlinkArgs<'_, '_>) -> Result<(), Errno> {
    let vp = ap.a_vp;
    let ip = vtoi(vp);

    let isize = ip.dip_size();
    if isize < u64::from(ip.ump().um_maxsymlinklen.get()) {
        let mut link = [0u8; MAXSYMLINKLEN_UFS2];
        let n = isize as usize;
        ip.with_shortlink(|s| link[..n].copy_from_slice(&s[..n]));
        return uiomove(&mut link[..n], ap.a_uio);
    }
    VOP_READ(vp, ap.a_uio, 0, ap.a_cred)
}

/// `ufs_lock` (`vop_lock`): lock an inode. If its already locked, set the WANT bit and
/// sleep.
pub fn ufs_lock(ap: &mut VopLockArgs) -> Result<(), Errno> {
    rrw_enter(&vtoi(ap.a_vp).i_lock, ap.a_flags & LK_RWFLAGS)
}

/// `ufs_unlock` (`vop_unlock`): unlock an inode. If WANT bit is on, wakeup.
pub fn ufs_unlock(ap: &mut VopUnlockArgs) -> Result<(), Errno> {
    rrw_exit(&vtoi(ap.a_vp).i_lock);
    Ok(())
}

/// `ufs_islocked` (`vop_islocked`): check for a locked inode.
pub fn ufs_islocked(ap: &mut VopIslockedArgs) -> i32 {
    rrw_status(&vtoi(ap.a_vp).i_lock)
}

/// `ufs_strategy` (`vop_strategy`): calculate the logical to physical mapping if not done
/// already, then call the device strategy routine.
pub fn ufs_strategy(ap: &mut VopStrategyArgs) -> Result<(), Errno> {
    let bp = ap.a_bp;
    let Some(vp) = bp.b_vp.get() else {
        panic(format_args!("ufs_strategy: buffer without a vnode"));
    };

    let ip = vtoi(vp);
    if vp.v_type.get() == VBLK || vp.v_type.get() == VCHR {
        panic(format_args!("ufs_strategy: spec"));
    }
    if bp.b_blkno.get() == bp.b_lblkno.get() {
        let mut blkno = bp.b_blkno.get();
        let error = VOP_BMAP(vp, bp.b_lblkno.get(), None, Some(&mut blkno), None);
        bp.b_blkno.set(blkno);
        if let Err(e) = error {
            bp.b_error.set(Some(e));
            bp.set(B_ERROR);
            let s = splbio();
            biodone(bp);
            splx(s);
            return Err(e);
        }
        if bp.b_blkno.get() == -1 {
            // SAFETY: a buffer handed to the strategy routine is busy for this I/O and
            // mapped; the strategy owns it until biodone.
            unsafe { clrbuf(bp) };
        }
    }
    if bp.b_blkno.get() == -1 {
        let s = splbio();
        biodone(bp);
        splx(s);
        return Ok(());
    }
    let devvp = ip.i_devvp();
    bp.b_dev.set(devvp.v_rdev());
    let _ = VOP_STRATEGY(devvp, bp);
    Ok(())
}

/// `ufs_print` (`vop_print`): print out the contents of an inode.
pub fn ufs_print(ap: &mut VopPrintArgs) -> Result<(), Errno> {
    #[cfg(any(feature = "debug", feature = "diagnostic"))]
    {
        use crate::sys::types::{major, minor};
        let vp = ap.a_vp;
        let ip = vtoi(vp);

        crate::kprintf!(
            "tag VT_UFS, ino {}, on dev {}, {}",
            ip.i_number.get(),
            major(ip.i_dev.get()),
            minor(ip.i_dev.get())
        );
        crate::kprintf!(
            " flags 0x{:x}, effnlink {}, nlink {}\n",
            ip.i_flag.get(),
            ip.i_effnlink.get(),
            ip.dip_nlink()
        );
        crate::kprintf!(
            "\tmode 0{:o}, owner {}, group {}, size {}",
            ip.dip_mode(),
            ip.dip_uid(),
            ip.dip_gid(),
            ip.dip_size()
        );
        // FIFO: fifo_printinfo(vp) for a VFIFO (fifofs, not ported).
        crate::kprintf!("\n");
    }
    #[cfg(not(any(feature = "debug", feature = "diagnostic")))]
    let _ = ap;

    Ok(())
}

/// `ufsspec_read`: read wrapper for special devices: set access flag.
pub fn ufsspec_read(ap: &mut VopReadArgs<'_, '_>) -> Result<(), Errno> {
    vtoi(ap.a_vp).set_flag(IN_ACCESS);
    spec_read(ap)
}

/// `ufsspec_write`: write wrapper for special devices: set update and change flags.
pub fn ufsspec_write(ap: &mut VopWriteArgs<'_, '_>) -> Result<(), Errno> {
    vtoi(ap.a_vp).set_flag(IN_CHANGE | IN_UPDATE);
    spec_write(ap)
}

/// `ufsspec_close`: close wrapper for special devices. Update the times on the inode then
/// do device close.
pub fn ufsspec_close(ap: &mut VopCloseArgs<'_>) -> Result<(), Errno> {
    let vp = ap.a_vp;

    if vp.v_usecount.get() > 1 {
        ufs_itimes(vp);
    }
    spec_close(ap)
}

/// `ufs_pathconf` (`vop_pathconf`): return POSIX pathconf information applicable to ufs
/// filesystems.
pub fn ufs_pathconf(ap: &mut VopPathconfArgs<'_>) -> Result<(), Errno> {
    let stat = || vmount(ap.a_vp).mnt_stat.get();
    *ap.a_retval = match ap.a_name {
        _PC_LINK_MAX => LINK_MAX as Register,
        _PC_NAME_MAX => NAME_MAX as Register,
        _PC_CHOWN_RESTRICTED => 1,
        _PC_NO_TRUNC => 1,
        _PC_ALLOC_SIZE_MIN => stat().f_bsize as Register,
        _PC_FILESIZEBITS => 64,
        _PC_REC_INCR_XFER_SIZE => stat().f_iosize as Register,
        _PC_REC_MAX_XFER_SIZE => -1, // means ``unlimited''
        _PC_REC_MIN_XFER_SIZE => stat().f_iosize as Register,
        _PC_REC_XFER_ALIGN => PAGE_SIZE as Register,
        _PC_SYMLINK_MAX => MAXPATHLEN as Register,
        _PC_2_SYMLINKS => 1,
        _PC_TIMESTAMP_RESOLUTION => 1,
        _ => return Err(Errno::EINVAL),
    };

    Ok(())
}

/// `ufs_advlock` (`vop_advlock`): advisory record locking support.
pub fn ufs_advlock(ap: &mut VopAdvlockArgs<'_>) -> Result<(), Errno> {
    let ip = vtoi(ap.a_vp);

    lf_advlock(
        &ip.i_lockf,
        ip.dip_size() as i64,
        ap.a_id,
        ap.a_op,
        ap.a_fl,
        ap.a_flags,
    )
}

/// `ufs_makeinode`: allocate a new inode of type `mode` and enter it in the directory
/// `dvp` under the name in `cnp`; `*vpp` is its vnode, referenced and locked.
pub fn ufs_makeinode(
    mut mode: Mode,
    dvp: &'static Vnode,
    vpp: &mut Option<&'static Vnode>,
    cnp: &mut Componentname,
) -> Result<(), Errno> {
    let pdir = vtoi(dvp);
    #[cfg(feature = "diagnostic")]
    if cnp.cn_flags & crate::sys::namei::HASBUF == 0 {
        panic(format_args!("ufs_makeinode: no name"));
    }
    *vpp = None;
    if mode & IFMT == 0 {
        mode |= IFREG;
    }

    let tvp = match UFS_INODE_ALLOC(pdir, mode, cnp.cn_cred) {
        Ok(tvp) => tvp,
        Err(e) => {
            pnbuf_free(cnp);
            return Err(e);
        }
    };

    let ip = vtoi(tvp);

    ip.dip_set_gid(pdir.dip_gid());
    let c = ucred(cnp.cn_cred);
    ip.dip_set_uid(c.cr_uid.get());

    if let Err(e) = getinoquota(ip).and_then(|()| ufs_quota_alloc_inode(ip, cnp.cn_cred)) {
        pnbuf_free(cnp);
        let _ = UFS_INODE_FREE(ip, ip.i_number.get(), mode);
        vput(tvp);
        return Err(e);
    }

    ip.set_flag(IN_ACCESS | IN_CHANGE | IN_UPDATE);
    ip.dip_set_mode(mode);
    tvp.v_type.set(iftovt(mode)); // Rest init'd in getnewvnode().
    ip.i_effnlink.set(1);
    ip.dip_set_nlink(1);
    if ip.dip_mode() & ISGID != 0
        && !groupmember(ip.dip_gid(), c)
        && !vnoperm(dvp)
        && suser_ucred(c).is_err()
    {
        ip.dip_set_mode(ip.dip_mode() & !ISGID);
    }

    let error: Result<(), Errno> = 'bad: {
        // Make sure inode goes to disk before directory entry.
        if let Err(e) = UFS_UPDATE(ip, 1) {
            break 'bad Err(e);
        }

        let mut newdir = Direct::new();
        ufs_makedirentry(ip, cnp, &mut newdir);
        if let Err(e) = ufs_direnter(dvp, Some(tvp), &mut newdir, cnp, None) {
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
    ip.i_effnlink.set(0);
    ip.dip_set_nlink(0);
    ip.set_flag(IN_CHANGE);
    tvp.v_type.set(VNON);
    vput(tvp);

    error
}

/// `ufs_kqfilter` (`vop_kqfilter`): attach a knote to the vnode.
pub fn ufs_kqfilter(ap: &mut VopKqfilterArgs<'_>) -> Result<(), Errno> {
    let vp = ap.a_vp;
    let kn = ap.a_kn;

    match kn.kn_filter().get() {
        EVFILT_READ => kn.kn_fop.set(Some(&UFSREAD_FILTOPS)),
        EVFILT_WRITE => kn.kn_fop.set(Some(&UFSWRITE_FILTOPS)),
        EVFILT_VNODE => kn.kn_fop.set(Some(&UFSVNODE_FILTOPS)),
        _ => return Err(Errno::EINVAL),
    }

    kn.kn_hook.set(ptr::from_ref(vp).cast_mut().cast());

    klist_insert_locked(&vp.v_klist, kn);

    Ok(())
}

/// `kn->kn_hook` of a UFS knote: its vnode.
fn kn_vnode(kn: &Knote) -> &'static Vnode {
    // SAFETY: `ufs_kqfilter` points `kn_hook` at the vnode, a `vnode_pool` item that is never
    // freed.
    match unsafe { kn.kn_hook.get().cast::<Vnode>().as_ref() } {
        Some(vp) => vp,
        None => panic(format_args!("knote {:p}: no vnode", kn)),
    }
}

/// `filt_ufsdetach`: unhooks the knote from the vnode.
pub fn filt_ufsdetach(kn: &Knote) {
    let vp = kn_vnode(kn);

    klist_remove_locked(&vp.v_klist, kn);
}

/// `filt_ufsread`: the bytes past the file offset; always ready for poll and select.
pub fn filt_ufsread(kn: &Knote, hint: i64) -> bool {
    let vp = kn_vnode(kn);
    let ip = vtoi(vp);

    // filesystem is gone, so set the EOF flag and schedule the knote for deletion.
    if hint == i64::from(NOTE_REVOKE) {
        kn.set_flags(EV_EOF | EV_ONESHOT);
        return true;
    }

    #[cfg(feature = "ext2fs")]
    let size = if crate::ufs::ext2fs::ext2fs_extern::is_ext2_vnode(vp) {
        crate::ufs::ext2fs::ext2fs_inode::ext2fs_size(ip) as i64
    } else {
        ip.dip_size() as i64
    };
    #[cfg(not(feature = "ext2fs"))]
    let size = ip.dip_size() as i64;
    kn.kn_data().set(size - foffset(kn.fp()));
    if kn.kn_data().get() == 0 && kn.kn_sfflags.get() & NOTE_EOF != 0 {
        kn.kn_fflags().set(kn.kn_fflags().get() | NOTE_EOF);
        return true;
    }

    if kn.has_flags(__EV_POLL | __EV_SELECT) {
        return true;
    }

    kn.kn_data().get() != 0
}

/// `filt_ufswrite`: a file is always writable.
pub fn filt_ufswrite(kn: &Knote, hint: i64) -> bool {
    // filesystem is gone, so set the EOF flag and schedule the knote for deletion.
    if hint == i64::from(NOTE_REVOKE) {
        kn.set_flags(EV_EOF | EV_ONESHOT);
        return true;
    }

    kn.kn_data().set(0);
    true
}

/// `filt_ufsvnode`: records the vnode events (`NOTE_*`) the user asked for.
pub fn filt_ufsvnode(kn: &Knote, hint: i64) -> bool {
    let hint32 = hint as u32;
    if kn.kn_sfflags.get() & hint32 != 0 {
        kn.kn_fflags().set(kn.kn_fflags().get() | hint32);
    }
    if hint == i64::from(NOTE_REVOKE) {
        kn.set_flags(EV_EOF);
        return true;
    }
    kn.kn_fflags().get() != 0
}
/* </CODE> */

/* <TESTS> */
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn vnode_filters_record_the_events_asked_for() {
        // EVFILT_VNODE: only the subscribed notes are recorded; revocation ends it.
        let kn = Knote::new();
        kn.kn_sfflags.set(NOTE_WRITE | NOTE_DELETE);
        assert!(!filt_ufsvnode(&kn, i64::from(NOTE_ATTRIB)));
        assert!(filt_ufsvnode(&kn, i64::from(NOTE_WRITE)));
        assert_eq!(kn.kn_fflags().get(), NOTE_WRITE);
        assert!(filt_ufsvnode(&kn, i64::from(NOTE_REVOKE)) && kn.has_flags(EV_EOF));

        // EVFILT_WRITE: always writable, EOF and one-shot once revoked.
        let kn = Knote::new();
        assert!(filt_ufswrite(&kn, 0) && kn.kn_data().get() == 0);
        assert!(filt_ufswrite(&kn, i64::from(NOTE_REVOKE)));
        assert!(kn.has_flags(EV_EOF) && kn.has_flags(EV_ONESHOT));
    }
}
/* </TESTS> */
