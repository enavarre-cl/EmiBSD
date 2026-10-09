/* <LICENSES> */
/*
 * Copyright (c) 2012-2013 Sylvestre Gallon <ccna.syl@gmail.com>
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

/* <CODE> */
//! The FUSE vnode operations (`fusefs_vops`): each one becomes a request to the daemon
//! (`fb_setup`, `fb_queue`), and the kqueue filters.
//!
//! Upstream: sys/miscfs/fuse/fuse_vnops.c @ 3ce1f3f79392
//!
//! FUSE file systems can maintain a file handle for each VFS file descriptor that is opened.
//! The OpenBSD VFS does not make file descriptors visible to us so we fake it by mapping
//! open flags to file handles. There is no way for FUSE to know which file descriptor is
//! being used by an application for a file operation. We only maintain 3 descriptors, one
//! each for `O_RDONLY`, `O_WRONLY` and `O_RDWR`. When reading and writing, the first open
//! descriptor is used and this may well not be the one that was set by FUSE open and may
//! have even been opened by another process.
//!
//! ## Deviations
//! - `vop_lookup` is `fusefs_lookup` of `fuse_lookup.rs`; the helpers the C installs
//!   (`vop_generic_abortop`, `vop_generic_bmap`, `spec_pathconf`) are the ported ones;
//!   `vop_revoke` and `vop_bwrite` are NULL (`None`), as in C.
//! - `pool_put(&namei_pool, cnp->cn_pnbuf)` is `pnbuf_free`. The thread the C reads from
//!   `cnp->cn_proc` is `cn_proc` (detached from `cnp`, as `vn_open` does), from
//!   `uio->uio_procp` and a `struct proc *` argument that may be NULL (`a_p` of close,
//!   inactive and reclaim) the given one or else `curproc` (`proc_or_cur`), which panics
//!   without a thread where the C would follow NULL.
//! - Credentials the C dereferences (`ap->a_cred` in `fusefs_access` and `fusefs_setattr`)
//!   go through `ucred`, which panics on `NOCRED`/`FSCRED`.
//! - `fusefs_readdir` and `fusefs_mkdir` free the fusebuf on the two paths where the C
//!   forgets it (a dirent that does not fit the caller's buffer, `goto out`; a `mkdir` reply
//!   with inode 0 or `FUSE_ROOT_ID`).
//! - `fusefs_readdir` builds each `struct dirent` in a byte array (`dirent_bytes`) for
//!   `uiomove`, the name NUL-padded to `d_reclen` as the C's `memset` does.
//! - `fusefs_readlink` and `fusefs_read` copy the daemon's data from the fusebuf's buffer
//!   (`fb_dat_slice`); a reply may carry fewer bytes than asked for.
//! - `fusefs_print`'s `#if defined(DEBUG) || defined(DIAGNOSTIC) || defined(VFSLCKDEBUG)`
//!   is the `debug`/`diagnostic` features (`VFSLCKDEBUG` has no feature).
//! - The filters reach their vnode through `kn_hook` (`kn_vnode`), as tmpfs's do.

use core::ptr::{self, NonNull};

use crate::kern::kern_event::{klist_insert_locked, klist_remove_locked};
use crate::kern::kern_rwlock::{rrw_enter, rrw_exit, rrw_status};
use crate::kern::kern_subr::uiomove;
use crate::kern::spec_vnops::spec_pathconf;
use crate::kern::subr_pool::pool_put;
use crate::kern::subr_prf::panic;
use crate::kern::vfs_default::{vop_generic_abortop, vop_generic_bmap};
use crate::kern::vfs_init::NAMEI_POOL;
use crate::kern::vfs_lockf::lf_advlock;
use crate::kern::vfs_subr::{vaccess, vgone, vput, vrele};
use crate::kern::vfs_vnops::vn_lock;
use crate::kern::vfs_vops::{VOP_ABORTOP, VOP_GETATTR, VOP_UNLOCK};
use crate::machine::cpu::curproc;
use crate::miscfs::fuse::fuse_device::fuse_device_queue_fbuf;
use crate::miscfs::fuse::fuse_file::{fusefs_fd_get, fusefs_file_close, fusefs_file_open};
use crate::miscfs::fuse::fuse_lookup::fusefs_lookup;
use crate::miscfs::fuse::fusebuf::{fb_delete, fb_queue, fb_setup};
use crate::miscfs::fuse::fusefs::{
    UNDEF_FLUSH, UNDEF_FSYNC, UNDEF_LINK, UNDEF_MKDIR, UNDEF_MKNOD, UNDEF_READLINK, UNDEF_REMOVE,
    UNDEF_RENAME, UNDEF_RMDIR, UNDEF_SETATTR, UNDEF_SYMLINK,
};
use crate::miscfs::fuse::fusefs_node::{
    FUFH_INVALID, FUFH_MAXTYPE, FUFH_RDONLY, FUFH_RDWR, FUFH_WRONLY, FufhType, VTOI,
};
use crate::sys::dirent::{Dirent, MAXNAMLEN, dirent_recsize};
use crate::sys::errno::Errno;
use crate::sys::event::{
    __EV_POLL, __EV_SELECT, EV_EOF, EV_ONESHOT, EVFILT_READ, EVFILT_VNODE, EVFILT_WRITE,
    FILTEROP_ISFD, Filterops, Knote, NOTE_ATTRIB, NOTE_DELETE, NOTE_EOF, NOTE_LINK, NOTE_RENAME,
    NOTE_REVOKE, NOTE_WRITE,
};
use crate::sys::fcntl::{
    FREAD, FWRITE, O_CREAT, O_EXCL, O_RDONLY, O_RDWR, O_TRUNC, O_WRONLY, oflags,
};
use crate::sys::file::foffset;
use crate::sys::fusebuf::{
    FUSE_FATTR_ATIME, FUSE_FATTR_GID, FUSE_FATTR_MODE, FUSE_FATTR_MTIME, FUSE_FATTR_SIZE,
    FUSE_FATTR_UID, FUSE_FLUSH, FUSE_FORGET, FUSE_FSYNC, FUSE_GETATTR, FUSE_LINK, FUSE_MKDIR,
    FUSE_MKNOD, FUSE_NAME_OFFSET, FUSE_READ, FUSE_READDIR, FUSE_READLINK, FUSE_RENAME, FUSE_RMDIR,
    FUSE_ROOT_ID, FUSE_SETATTR, FUSE_SYMLINK, FUSE_UNLINK, FUSE_WRITE, FuseAttrOut, FuseDirent,
    FuseEntryOut, FuseFlushIn, FuseForgetIn, FuseFsyncIn, FuseLinkIn, FuseMkdirIn, FuseMknodIn,
    FuseReadIn, FuseRenameIn, FuseSetattrIn, FuseWriteIn, FuseWriteOut, fuse_dirent_size,
};
use crate::sys::lock::{LK_EXCLUSIVE, LK_RETRY, LK_RWFLAGS};
use crate::sys::mount::{MNT_RDONLY, Mount, VFS_VGET};
use crate::sys::namei::{Componentname, ISDOTDOT};
use crate::sys::param::BLKDEV_IOSIZE;
use crate::sys::proc::Proc;
use crate::sys::stat::{ALLPERMS, S_BLKSIZE, S_IFMT, S_IRUSR, S_IRWXU, S_ISTXT, S_IXUSR};
use crate::sys::types::{Gid, Mode, Nlink, Uid};
use crate::sys::ucred::Ucred;
use crate::sys::vnode::{
    IO_APPEND, VDIR, VLNK, VN_KNOTE, VNON, VNOVAL, VREG, VWRITE, Vattr, Vnode, VopAccessArgs,
    VopAdvlockArgs, VopCloseArgs, VopCreateArgs, VopFsyncArgs, VopGetattrArgs, VopInactiveArgs,
    VopIoctlArgs, VopIslockedArgs, VopKqfilterArgs, VopLinkArgs, VopLockArgs, VopMkdirArgs,
    VopMknodArgs, VopOpenArgs, VopPrintArgs, VopReadArgs, VopReaddirArgs, VopReadlinkArgs,
    VopReclaimArgs, VopRemoveArgs, VopRenameArgs, VopRmdirArgs, VopSetattrArgs, VopStrategyArgs,
    VopSymlinkArgs, VopUnlockArgs, VopWriteArgs, Vops, cred_ref, iftovt, makeimode,
};
use crate::uvm::uvm_vnode::{uvm_vnp_setsize, uvm_vnp_uncache};

/// `fusefs_vops`.
pub static FUSEFS_VOPS: Vops = Vops {
    vop_lookup: Some(fusefs_lookup),
    vop_create: Some(fusefs_create),
    vop_mknod: Some(fusefs_mknod),
    vop_open: Some(fusefs_open),
    vop_close: Some(fusefs_close),
    vop_access: Some(fusefs_access),
    vop_getattr: Some(fusefs_getattr),
    vop_setattr: Some(fusefs_setattr),
    vop_read: Some(fusefs_read),
    vop_write: Some(fusefs_write),
    vop_ioctl: Some(fusefs_ioctl),
    vop_kqfilter: Some(fusefs_kqfilter),
    vop_revoke: None,
    vop_fsync: Some(fusefs_fsync),
    vop_remove: Some(fusefs_remove),
    vop_link: Some(fusefs_link),
    vop_rename: Some(fusefs_rename),
    vop_mkdir: Some(fusefs_mkdir),
    vop_rmdir: Some(fusefs_rmdir),
    vop_symlink: Some(fusefs_symlink),
    vop_readdir: Some(fusefs_readdir),
    vop_readlink: Some(fusefs_readlink),
    vop_abortop: Some(vop_generic_abortop),
    vop_inactive: Some(fusefs_inactive),
    vop_reclaim: Some(fusefs_reclaim),
    vop_lock: Some(fusefs_lock),
    vop_unlock: Some(fusefs_unlock),
    vop_bmap: Some(vop_generic_bmap),
    vop_strategy: Some(fusefs_strategy),
    vop_print: Some(fusefs_print),
    vop_islocked: Some(fusefs_islocked),
    vop_pathconf: Some(spec_pathconf),
    vop_advlock: Some(fusefs_advlock),
    vop_bwrite: None,
};

/// `fusefsread_filtops`.
pub static FUSEFSREAD_FILTOPS: Filterops = Filterops {
    f_flags: FILTEROP_ISFD,
    f_attach: None,
    f_detach: Some(filt_fusefsdetach),
    f_event: Some(filt_fusefsread),
    f_modify: None,
    f_process: None,
};

/// `fusefswrite_filtops`.
pub static FUSEFSWRITE_FILTOPS: Filterops = Filterops {
    f_flags: FILTEROP_ISFD,
    f_attach: None,
    f_detach: Some(filt_fusefsdetach),
    f_event: Some(filt_fusefswrite),
    f_modify: None,
    f_process: None,
};

/// `fusefsvnode_filtops`.
pub static FUSEFSVNODE_FILTOPS: Filterops = Filterops {
    f_flags: FILTEROP_ISFD,
    f_attach: None,
    f_detach: Some(filt_fusefsdetach),
    f_event: Some(filt_fusefsvnode),
    f_modify: None,
    f_process: None,
};

/// The vnode's mount, which a FUSE vnode always has.
pub(crate) fn vmount(vp: &Vnode) -> &'static Mount {
    match vp.v_mount.get() {
        Some(mp) => mp,
        None => panic(format_args!("fusefs: vnode {:p} without a mount", vp)),
    }
}

/// `curproc`: the thread a vnode operation runs in.
pub(crate) fn curp() -> &'static Proc {
    match curproc() {
        Some(p) => p,
        None => panic(format_args!("fusefs: no curproc")),
    }
}

/// A `struct proc *` the C dereferences although it may be NULL: the given thread, or else
/// `curproc`.
pub(crate) fn proc_or_cur(p: Option<&Proc>) -> &Proc {
    match p {
        Some(p) => p,
        None => curp(),
    }
}

/// `cnp->cn_proc`, detached from `cnp` so that `cnp` can be changed while it is used.
pub(crate) fn cn_proc<'p>(cnp: &Componentname) -> &'p Proc {
    if cnp.cn_proc.is_null() {
        panic(format_args!("fusefs: componentname without a thread"));
    }
    // SAFETY: `cn_proc` is the thread doing the lookup (`ndinitat`, or the caller that
    // builds the component), which outlives the vnode operation that uses it.
    unsafe { &*cnp.cn_proc }
}

/// `*cred` of a credential the C dereferences: a real one (`NOCRED`/`FSCRED` panic).
fn ucred<'a>(cred: *const Ucred) -> &'a Ucred {
    // SAFETY: the credentials a vnode operation receives are held by its caller for the
    // operation's duration (`cred_ref`'s contract).
    match unsafe { cred_ref(cred) } {
        Some(c) => c,
        None => panic(format_args!(
            "fusefs: credential {:p} is not a real one",
            cred
        )),
    }
}

/// `pool_put(&namei_pool, cnp->cn_pnbuf)`: give back the pathname buffer.
pub(crate) fn pnbuf_free(cnp: &Componentname) {
    if let Some(buf) = NonNull::new(cnp.cn_pnbuf) {
        pool_put(&NAMEI_POOL, buf);
    }
}

/// Copies `name` and a NUL into the start of a fresh fusebuf's data (the C's
/// `memcpy(fbuf->fb_dat, cnp->cn_nameptr, cnp->cn_namelen); fbuf->fb_dat[namelen] = '\0'`).
fn fb_put_name(fbuf: &crate::sys::fusebuf::Fusebuf, off: usize, name: &[u8]) {
    // SAFETY: a fresh fusebuf, not queued yet: its data is the requester's.
    let dat = unsafe { fbuf.fb_dat_slice() };
    dat[off..off + name.len()].copy_from_slice(name);
    dat[off + name.len()] = 0;
}

/// `fusefs_kqfilter` (`vop_kqfilter`): attach a knote to the vnode.
pub fn fusefs_kqfilter(ap: &mut VopKqfilterArgs<'_>) -> Result<(), Errno> {
    let vp = ap.a_vp;
    let kn = ap.a_kn;

    match kn.kn_filter().get() {
        EVFILT_READ => kn.kn_fop.set(Some(&FUSEFSREAD_FILTOPS)),
        EVFILT_WRITE => kn.kn_fop.set(Some(&FUSEFSWRITE_FILTOPS)),
        EVFILT_VNODE => kn.kn_fop.set(Some(&FUSEFSVNODE_FILTOPS)),
        _ => return Err(Errno::EINVAL),
    }

    kn.kn_hook.set(ptr::from_ref(vp).cast_mut().cast());

    klist_insert_locked(&vp.v_klist, kn);

    Ok(())
}

/// `kn->kn_hook` of a FUSE knote: its vnode.
fn kn_vnode(kn: &Knote) -> &'static Vnode {
    // SAFETY: `fusefs_kqfilter` points `kn_hook` at the vnode, a `vnode_pool` item that is
    // never freed.
    match unsafe { kn.kn_hook.get().cast::<Vnode>().as_ref() } {
        Some(vp) => vp,
        None => panic(format_args!("knote {:p}: no vnode", kn)),
    }
}

/// `filt_fusefsdetach`.
pub fn filt_fusefsdetach(kn: &Knote) {
    let vp = kn_vnode(kn);

    klist_remove_locked(&vp.v_klist, kn);
}

/// `filt_fusefsread`: the bytes past the file offset; always ready for poll and select.
pub fn filt_fusefsread(kn: &Knote, hint: i64) -> bool {
    let vp = kn_vnode(kn);
    let ip = VTOI(vp);

    // filesystem is gone, so set the EOF flag and schedule the knote for deletion
    if hint == i64::from(NOTE_REVOKE) {
        kn.set_flags(EV_EOF | EV_ONESHOT);
        return true;
    }

    kn.kn_data().set(ip.filesize.get() - foffset(kn.fp()));
    if kn.kn_data().get() == 0 && kn.kn_sfflags.get() & NOTE_EOF != 0 {
        kn.kn_fflags().set(kn.kn_fflags().get() | NOTE_EOF);
        return true;
    }

    if kn.has_flags(__EV_POLL | __EV_SELECT) {
        return true;
    }

    kn.kn_data().get() != 0
}

/// `filt_fusefswrite`.
pub fn filt_fusefswrite(kn: &Knote, hint: i64) -> bool {
    // filesystem is gone, so set the EOF flag and schedule the knote for deletion
    if hint == i64::from(NOTE_REVOKE) {
        kn.set_flags(EV_EOF | EV_ONESHOT);
        return true;
    }

    kn.kn_data().set(0);
    true
}

/// `filt_fusefsvnode`.
pub fn filt_fusefsvnode(kn: &Knote, hint: i64) -> bool {
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

/// `fusefs_open` (`vop_open`): makes sure the node has a daemon handle of the open's kind.
pub fn fusefs_open(ap: &mut VopOpenArgs<'_>) -> Result<(), Errno> {
    let vp = ap.a_vp;
    let ip = VTOI(vp);
    let fmp = ip.i_fmp;
    let mut fufh_type = FUFH_RDONLY;

    if fmp.sess_init.get() == 0 {
        return Err(Errno::ENXIO);
    }

    let isdir = if vp.v_type.get() == VDIR {
        true
    } else {
        if ap.a_mode & FREAD != 0 && ap.a_mode & FWRITE != 0 {
            fufh_type = FUFH_RDWR;
        } else if ap.a_mode & FWRITE != 0 {
            fufh_type = FUFH_WRONLY;
        }

        // Due to possible attribute caching, there is no reliable way to determine if the
        // file was modified externally (e.g. network file system) so clear the UVM cache to
        // ensure that it is not stale. The file can still become stale later on read but
        // this will satisfy most situations.
        uvm_vnp_uncache(vp);
        false
    };

    // already open i think all is ok
    if ip.fufh(fufh_type).fh_type != FUFH_INVALID {
        return Ok(());
    }

    // The file has already been created and/or truncated so FUSE dictates that no creation
    // and truncation flags are passed to open.
    let flags = oflags(ap.a_mode) & !(O_CREAT | O_EXCL | O_TRUNC);
    fusefs_file_open(fmp, ip, fufh_type, flags, isdir, ap.a_p)
}

/// `fusefs_close` (`vop_close`): asks the daemon to flush a writable file (`FUSE_FLUSH`);
/// handles are released on `VOP_INACTIVE`.
pub fn fusefs_close(ap: &mut VopCloseArgs<'_>) -> Result<(), Errno> {
    let ip = VTOI(ap.a_vp);
    let fmp = ip.i_fmp;

    if fmp.sess_init.get() == 0 {
        return Ok(());
    }

    // The file or directory may have been opened more than once so there is no reliable
    // way to determine when to ask the FUSE daemon to release its file descriptor. For
    // files, ask the daemon to flush any buffers to disk now. All open file descriptors
    // will be released on VOP_INACTIVE(9).

    if ap.a_vp.v_type.get() == VDIR {
        return Ok(());
    }

    // Implementing flush is optional so don't error.
    if fmp.undef_op.get() & UNDEF_FLUSH != 0 {
        return Ok(());
    }

    // Only flush writeable file descriptors.
    let fufh_type = if ap.a_fflag & FREAD != 0 && ap.a_fflag & FWRITE != 0 {
        FUFH_RDWR
    } else if ap.a_fflag & FWRITE != 0 {
        FUFH_WRONLY
    } else {
        return Ok(());
    };

    if ip.fufh(fufh_type).fh_type == FUFH_INVALID {
        return Err(Errno::EBADF);
    }

    let fbuf = fb_setup(0, ip.i_number, FUSE_FLUSH, proc_or_cur(ap.a_p));
    fbuf.op_set(&FuseFlushIn {
        fh: ip.fufh(fufh_type).fh_id,
        ..FuseFlushIn::default()
    });
    let error = fb_queue(fmp.dev, fbuf);
    fb_delete(fbuf);
    if error == Err(Errno::ENOSYS) {
        fmp.undef_op.set(fmp.undef_op.get() | UNDEF_FLUSH);

        // Implementing flush is optional so don't error.
        return Ok(());
    }

    error
}

/// `fusefs_access` (`vop_access`): only the user who mounted the file system (or anyone
/// with `allow_other`), then the permissions `FUSE_GETATTR` reports.
pub fn fusefs_access(ap: &mut VopAccessArgs<'_>) -> Result<(), Errno> {
    let p = ap.a_p;
    let cred = p.ucred();
    let ip = VTOI(ap.a_vp);
    let fmp = ip.i_fmp;

    // Only user that mounted the file system can access it unless allow_other mount option
    // was specified.
    if fmp.allow_other == 0 && cred.cr_uid.get() != fmp.mp.mnt_stat.get().f_owner {
        return Err(Errno::EACCES);
    }

    if fmp.sess_init.get() == 0 {
        return Err(Errno::ENXIO);
    }

    // Disallow write attempts on filesystems mounted read-only; unless the file is a
    // socket, fifo, or a block or character device resident on the filesystem.
    if ap.a_mode & VWRITE != 0 && fmp.mp.mnt_flag.get() & MNT_RDONLY != 0 {
        let t = ap.a_vp.v_type.get();
        if t == VREG || t == VDIR || t == VLNK {
            return Err(Errno::EROFS);
        }
    }

    let mut vattr = Vattr::new();
    VOP_GETATTR(ap.a_vp, &mut vattr, ptr::from_ref(cred), p)?;

    vaccess(
        ap.a_vp.v_type.get(),
        vattr.va_mode & ALLPERMS,
        vattr.va_uid,
        vattr.va_gid,
        ap.a_mode,
        ucred(ap.a_cred),
    )
}

/// `fusefs_getattr` (`vop_getattr`): the daemon's `FUSE_GETATTR` answer; dummy values for
/// users other than the one who mounted, unless `allow_other`.
pub fn fusefs_getattr(ap: &mut VopGetattrArgs<'_>) -> Result<(), Errno> {
    let vp = ap.a_vp;
    let p = ap.a_p;
    let cred = p.ucred();
    let ip = VTOI(vp);
    let fmp = ip.i_fmp;
    let vap = &mut *ap.a_vap;

    // Only user that mounted the file system can access it unless allow_other mount option
    // was specified. Return dummy values for the root inode in this situation.
    let stat = fmp.mp.mnt_stat.get();
    if fmp.allow_other == 0 && cred.cr_uid.get() != stat.f_owner {
        *vap = Vattr::new();
        vap.va_type = VNON;
        if vmount(vp).mnt_flag.get() & MNT_RDONLY != 0 {
            vap.va_mode = S_IRUSR | S_IXUSR;
        } else {
            vap.va_mode = S_IRWXU;
        }
        vap.va_nlink = 2;
        vap.va_uid = stat.f_owner;
        vap.va_gid = stat.f_owner;
        vap.va_fsid = i64::from(stat.f_fsid.val[0]);
        vap.va_fileid = ip.i_number;
        vap.va_size = S_BLKSIZE as u64;
        vap.va_blocksize = S_BLKSIZE as i64;
        vap.va_atime.tv_sec = stat.f_ctime as i64;
        vap.va_mtime.tv_sec = stat.f_ctime as i64;
        vap.va_ctime.tv_sec = stat.f_ctime as i64;
        vap.va_rdev = fmp.dev;
        vap.va_bytes = S_BLKSIZE as u64;
        return Ok(());
    }

    if fmp.sess_init.get() == 0 {
        return Err(Errno::ENXIO);
    }

    let fbuf = fb_setup(0, ip.i_number, FUSE_GETATTR, p);

    if let Err(error) = fb_queue(fmp.dev, fbuf) {
        fb_delete(fbuf);
        return Err(error);
    }

    let mut st = fbuf.op_get::<FuseAttrOut>().attr;

    // opendir(3) expects blocksize to be greater than zero.
    if st.blksize == 0 {
        st.blksize = S_BLKSIZE as u32;
    } else if st.blksize as usize > BLKDEV_IOSIZE {
        st.blksize = BLKDEV_IOSIZE as u32;
    }

    // calculate blocks of held disk space if fs didn't do it
    if st.blocks == 0 && st.size > 0 {
        st.blocks = st.size.div_ceil(S_BLKSIZE as u64);
    }

    *vap = Vattr::new();
    vap.va_type = iftovt(st.mode);
    vap.va_mode = st.mode & !S_IFMT;
    vap.va_nlink = st.nlink as Nlink;
    vap.va_uid = st.uid as Uid;
    vap.va_gid = st.gid as Gid;
    vap.va_fsid = i64::from(stat.f_fsid.val[0]);
    vap.va_fileid = st.ino;
    vap.va_size = st.size;
    vap.va_blocksize = i64::from(st.blksize);
    vap.va_atime.tv_sec = st.atime as i64;
    vap.va_atime.tv_nsec = i64::from(st.atimensec);
    vap.va_mtime.tv_sec = st.mtime as i64;
    vap.va_mtime.tv_nsec = i64::from(st.mtimensec);
    vap.va_ctime.tv_sec = st.ctime as i64;
    vap.va_ctime.tv_nsec = i64::from(st.ctimensec);
    vap.va_rdev = st.rdev as i32;
    vap.va_bytes = st.blocks.wrapping_mul(S_BLKSIZE as u64);

    fb_delete(fbuf);
    Ok(())
}

/// `fusefs_setattr` (`vop_setattr`): `FUSE_SETATTR` with the attributes that are set.
pub fn fusefs_setattr(ap: &mut VopSetattrArgs<'_>) -> Result<(), Errno> {
    let vap = &*ap.a_vap;
    let vp = ap.a_vp;
    let ip = VTOI(vp);
    let p = ap.a_p;
    let fmp = ip.i_fmp;

    // Setting of flags is not supported.
    if vap.va_flags != VNOVAL as u64 {
        return Err(Errno::EOPNOTSUPP);
    }

    // Check for unsettable attributes.
    if vap.va_type != VNON
        || vap.va_nlink != VNOVAL as Nlink
        || vap.va_fsid != i64::from(VNOVAL)
        || vap.va_fileid != VNOVAL as u64
        || vap.va_blocksize != i64::from(VNOVAL)
        || vap.va_rdev != VNOVAL
        || vap.va_bytes as i32 != VNOVAL
        || vap.va_gen != VNOVAL as u64
    {
        return Err(Errno::EINVAL);
    }

    if fmp.sess_init.get() == 0 {
        return Err(Errno::ENXIO);
    }

    if fmp.undef_op.get() & UNDEF_SETATTR != 0 {
        return Err(Errno::ENOSYS);
    }

    let fbuf = fb_setup(0, ip.i_number, FUSE_SETATTR, p);
    let mut setattr = FuseSetattrIn {
        valid: 0,
        ..FuseSetattrIn::default()
    };
    let rdonly = vmount(vp).mnt_flag.get() & MNT_RDONLY != 0;

    let error = 'out: {
        if vap.va_uid != VNOVAL as Uid {
            if rdonly {
                break 'out Err(Errno::EROFS);
            }
            setattr.uid |= vap.va_uid;
            setattr.valid |= FUSE_FATTR_UID;
        }

        if vap.va_gid != VNOVAL as Gid {
            if rdonly {
                break 'out Err(Errno::EROFS);
            }
            setattr.gid |= vap.va_gid;
            setattr.valid |= FUSE_FATTR_GID;
        }

        if vap.va_size != VNOVAL as u64 {
            // Disallow write attempts on read-only file systems; unless the file is a
            // socket, fifo, or a block or character device resident on the file system.
            match vp.v_type.get() {
                VDIR => break 'out Err(Errno::EISDIR),
                VLNK | VREG if rdonly => break 'out Err(Errno::EROFS),
                _ => {}
            }

            setattr.size |= vap.va_size;
            setattr.valid |= FUSE_FATTR_SIZE;
        }

        if vap.va_atime.tv_nsec != i64::from(VNOVAL) {
            if rdonly {
                break 'out Err(Errno::EROFS);
            }
            setattr.atime = vap.va_atime.tv_sec as u64;
            setattr.atimensec = vap.va_atime.tv_nsec as u32;
            setattr.valid |= FUSE_FATTR_ATIME;
        }

        if vap.va_mtime.tv_nsec != i64::from(VNOVAL) {
            if rdonly {
                break 'out Err(Errno::EROFS);
            }
            setattr.mtime = vap.va_mtime.tv_sec as u64;
            setattr.mtimensec = vap.va_mtime.tv_nsec as u32;
            setattr.valid |= FUSE_FATTR_MTIME;
        }
        // XXX should set a flag if (vap->va_vaflags & VA_UTIMES_CHANGE)

        if vap.va_mode != VNOVAL as Mode {
            if rdonly {
                break 'out Err(Errno::EROFS);
            }

            // chmod returns EFTYPE if the effective user ID is not the super-user, the mode
            // includes the sticky bit (S_ISVTX), and path does not refer to a directory
            if ucred(ap.a_cred).cr_uid.get() != 0
                && vp.v_type.get() != VDIR
                && vap.va_mode & S_ISTXT != 0
            {
                break 'out Err(Errno::EFTYPE);
            }

            setattr.mode = vap.va_mode & ALLPERMS;
            setattr.valid |= FUSE_FATTR_MODE;
        }

        if setattr.valid == 0 {
            break 'out Ok(());
        }

        fbuf.op_set(&setattr);
        if let Err(error) = fb_queue(fmp.dev, fbuf) {
            if error == Errno::ENOSYS {
                fmp.undef_op.set(fmp.undef_op.get() | UNDEF_SETATTR);
            }
            break 'out Err(error);
        }

        // truncate was successful, let uvm know
        if vap.va_size != VNOVAL as u64 && vap.va_size as i64 != ip.filesize.get() {
            ip.filesize.set(vap.va_size as i64);
            uvm_vnp_setsize(vp, vap.va_size as i64);
        }

        VN_KNOTE(ap.a_vp, NOTE_ATTRIB);
        Ok(())
    };

    fb_delete(fbuf);
    error
}

/// `fusefs_ioctl` (`vop_ioctl`).
pub fn fusefs_ioctl(_ap: &mut VopIoctlArgs<'_>) -> Result<(), Errno> {
    Err(Errno::ENOTTY)
}

/// `fusefs_link` (`vop_link`): `FUSE_LINK`.
pub fn fusefs_link(ap: &mut VopLinkArgs<'_>) -> Result<(), Errno> {
    let dvp = ap.a_dvp;
    let vp = ap.a_vp;
    let p = cn_proc(ap.a_cnp);

    let ip = VTOI(vp);
    let dip = VTOI(dvp);
    let fmp = ip.i_fmp;

    let error = 'out2: {
        if fmp.sess_init.get() == 0 {
            let _ = VOP_ABORTOP(dvp, ap.a_cnp);
            break 'out2 Err(Errno::ENXIO);
        }
        if fmp.undef_op.get() & UNDEF_LINK != 0 {
            let _ = VOP_ABORTOP(dvp, ap.a_cnp);
            break 'out2 Err(Errno::ENOSYS);
        }
        if !ptr::eq(dvp, vp)
            && let Err(e) = vn_lock(vp, LK_EXCLUSIVE)
        {
            let _ = VOP_ABORTOP(dvp, ap.a_cnp);
            break 'out2 Err(e);
        }

        let name = ap.a_cnp.name();
        let fbuf = fb_setup(name.len() + 1, dip.i_number, FUSE_LINK, p);

        fbuf.op_set(&FuseLinkIn {
            oldnodeid: ip.i_number,
        });
        fb_put_name(fbuf, 0, name);

        let error = fb_queue(fmp.dev, fbuf);

        if let Err(e) = error {
            if e == Errno::ENOSYS {
                fmp.undef_op.set(fmp.undef_op.get() | UNDEF_LINK);
            }

            fb_delete(fbuf);
        } else {
            fb_delete(fbuf);
            VN_KNOTE(vp, NOTE_LINK);
            VN_KNOTE(dvp, NOTE_WRITE);
        }

        // out1:
        pnbuf_free(ap.a_cnp);
        if !ptr::eq(dvp, vp) {
            let _ = VOP_UNLOCK(vp);
        }
        error
    };

    // out2:
    vput(dvp);
    error
}

/// `fusefs_symlink` (`vop_symlink`): `FUSE_SYMLINK` with the name and the target.
pub fn fusefs_symlink(ap: &mut VopSymlinkArgs<'_>) -> Result<(), Errno> {
    let dvp = ap.a_dvp;
    let p = cn_proc(ap.a_cnp);
    let target = ap.a_target;

    let dp = VTOI(dvp);
    let fmp = dp.i_fmp;

    let error = 'bad: {
        if fmp.sess_init.get() == 0 {
            break 'bad Err(Errno::ENXIO);
        }

        if fmp.undef_op.get() & UNDEF_SYMLINK != 0 {
            break 'bad Err(Errno::ENOSYS);
        }

        let target = &target[..target.iter().position(|&c| c == 0).unwrap_or(target.len())];
        let len = target.len() + 1;

        let name = ap.a_cnp.name();
        let fbuf = fb_setup(len + name.len() + 1, dp.i_number, FUSE_SYMLINK, p);

        fb_put_name(fbuf, 0, name);
        fb_put_name(fbuf, name.len() + 1, target);

        if let Err(error) = fb_queue(fmp.dev, fbuf) {
            if error == Errno::ENOSYS {
                fmp.undef_op.set(fmp.undef_op.get() | UNDEF_SYMLINK);
            }

            fb_delete(fbuf);
            break 'bad Err(error);
        }

        // symlink returns a fuse_entry_out with the ino of the new link
        let nodeid = fbuf.op_get::<FuseEntryOut>().nodeid;
        if nodeid == 0 || nodeid == FUSE_ROOT_ID {
            fb_delete(fbuf);
            break 'bad Err(Errno::EIO);
        }

        let tdp = match VFS_VGET(fmp.mp, nodeid) {
            Ok(tdp) => tdp,
            Err(e) => {
                fb_delete(fbuf);
                break 'bad Err(e);
            }
        };

        tdp.v_type.set(VLNK);
        VN_KNOTE(ap.a_dvp, NOTE_WRITE);

        *ap.a_vpp = Some(tdp);
        fb_delete(fbuf);
        vput(tdp);
        Ok(())
    };

    // bad:
    pnbuf_free(ap.a_cnp);
    vput(dvp);
    error
}

/// The first `d_reclen` bytes of a `struct dirent` with `d_namlen` bytes of `name`, the
/// rest of the name NUL-padded, as `uiomove(&de, de.d_reclen, uio)` copies them out.
fn dirent_bytes(de: &Dirent, name: &[u8]) -> ([u8; size_of::<Dirent>()], usize) {
    let mut b = [0u8; size_of::<Dirent>()];
    let reclen = usize::from(de.d_reclen).min(b.len());
    b[0..8].copy_from_slice(&de.d_fileno.to_ne_bytes());
    b[8..16].copy_from_slice(&de.d_off.to_ne_bytes());
    b[16..18].copy_from_slice(&de.d_reclen.to_ne_bytes());
    b[18] = de.d_type;
    b[19] = de.d_namlen;
    let namlen = usize::from(de.d_namlen);
    b[Dirent::NAME_OFFSET..Dirent::NAME_OFFSET + namlen].copy_from_slice(&name[..namlen]);
    (b, reclen)
}

/// `fusefs_readdir` (`vop_readdir`): `FUSE_READDIR` replies of at most `max_read` bytes,
/// each `struct fuse_dirent` checked and converted into a `struct dirent`.
pub fn fusefs_readdir(ap: &mut VopReaddirArgs<'_, '_>) -> Result<(), Errno> {
    let vp = ap.a_vp;
    let uio = &mut *ap.a_uio;
    let p = proc_or_cur(uio.uio_procp);
    let mut foffset = uio.uio_offset;
    let mut eofflag = 0;
    let mut diropen = false;

    let ip = VTOI(vp);
    let fmp = ip.i_fmp;

    if fmp.sess_init.get() == 0 {
        return Err(Errno::ENXIO);
    }

    // Some file systems expect that the kernel always uses the same read buffer size. In
    // the event that uio_resid is a multiple of max_read, we need to ensure that we use a
    // consistent buffer size in the loop.
    let read_size = uio.uio_resid.min(fmp.max_read as usize);

    // Basic check to ensure buffer is large enough for at least one dirent with maximum
    // allowed name length.
    if read_size < size_of::<Dirent>() {
        return Err(Errno::EINVAL);
    }

    if ip.fufh(FUFH_RDONLY).fh_type == FUFH_INVALID {
        fusefs_file_open(fmp, ip, FUFH_RDONLY, O_RDONLY, true, p)?;
        diropen = true;
    }

    let mut error: Result<(), Errno> = Ok(());

    // loop until we run out of buffer space
    'out: while error.is_ok() && uio.uio_resid >= read_size {
        let fbuf = fb_setup(0, ip.i_number, FUSE_READDIR, p);
        fbuf.op_set(&FuseReadIn {
            fh: ip.fufh(FUFH_RDONLY).fh_id,
            offset: foffset as u64,
            size: read_size as u32,
            ..FuseReadIn::default()
        });

        if let Err(e) = fb_queue(fmp.dev, fbuf) {
            error = Err(e);
            fb_delete(fbuf);
            break;
        }

        // Ack end of readdir. Only used by getcwd(3), getdents(3) relies on a repeat call
        // that returns no entries.
        if fbuf.fb_len() == 0 {
            eofflag = 1;
            fb_delete(fbuf);
            break;
        }

        // validate and convert the returned dirents
        // SAFETY: the reply is in, the fusebuf is off the device's queues: its data is ours.
        let dat: &[u8] = unsafe { fbuf.fb_dat_slice() };
        let mut fresid = fbuf.fb_len() as u32;
        let mut pos = 0usize;

        while uio.uio_resid > 0 && fresid as usize > FUSE_NAME_OFFSET {
            let Some(fdp) = FuseDirent::from_bytes(&dat[pos..]) else {
                break;
            };

            // get the size of the FUSE dirent
            let freclen = fuse_dirent_size(&fdp);

            // check for partial dirent
            if (fresid as usize) < freclen {
                break;
            }

            // check for sane name length
            if fdp.namelen == 0 || fdp.namelen as usize > MAXNAMLEN {
                error = Err(Errno::EIO);
                break;
            }

            let name = &dat[pos + FUSE_NAME_OFFSET..pos + FUSE_NAME_OFFSET + fdp.namelen as usize];

            // check for illegal character in file name
            if name.contains(&b'/') {
                error = Err(Errno::EIO);
                break;
            }

            // copy FUSE dirent into struct dirent
            let d_namlen = fdp.namelen as u8;
            let de = Dirent {
                d_fileno: fdp.ino,
                // d_off is used by telldir, seekdir, readdir libc functions and expect the
                // file system's offset. This will be passed back to this function so needs
                // to be the FUSE offset.
                d_off: fdp.off as i64,
                d_reclen: dirent_recsize(usize::from(d_namlen)) as u16,
                d_type: fdp.r#type as u8,
                d_namlen,
                __d_padding: [0; 4],
                d_name: [0; MAXNAMLEN + 1],
            };
            if uio.uio_resid < usize::from(de.d_reclen) {
                fb_delete(fbuf);
                break 'out;
            }
            // pad with NUL: dirent_bytes leaves the bytes past the name zero.
            let (mut b, reclen) = dirent_bytes(&de, name);

            if let Err(e) = uiomove(&mut b[..reclen], uio) {
                error = Err(e);
                break;
            }

            // advance to next FUSE dirent
            fresid -= freclen as u32;
            foffset = fdp.off as i64;
            pos += freclen;
        }

        fb_delete(fbuf);
    }

    // out:
    uio.uio_offset = foffset;

    if error.is_ok() {
        *ap.a_eofflag = eofflag;
    }

    if diropen {
        let _ = fusefs_file_close(fmp, ip, FUFH_RDONLY, O_RDONLY, true, p);
    }

    error
}

/// `fusefs_inactive` (`vop_inactive`): releases every daemon handle of the node.
pub fn fusefs_inactive(ap: &mut VopInactiveArgs<'_>) -> Result<(), Errno> {
    let vp = ap.a_vp;
    let p = proc_or_cur(ap.a_p);
    let ip = VTOI(vp);
    let fmp = ip.i_fmp;

    // Close all open file handles.
    for i in 0..FUFH_MAXTYPE.idx() {
        let t = FufhType::from_idx(i);
        let fufh = ip.fufh(t);
        if fufh.fh_type != FUFH_INVALID {
            // FUSE file systems expect the same flags to be sent on release that were sent
            // on open. We don't have a record of them so make a best guess.
            let flags = match t {
                FUFH_RDONLY => O_RDONLY,
                FUFH_WRONLY => O_WRONLY,
                _ => O_RDWR,
            };

            let _ = fusefs_file_close(fmp, ip, fufh.fh_type, flags, vp.v_type.get() == VDIR, p);
        }
    }

    let _ = VOP_UNLOCK(vp);

    // Don't return error to prevent kernel panic in vclean(9).
    Ok(())
}

/// `fusefs_readlink` (`vop_readlink`): `FUSE_READLINK`.
pub fn fusefs_readlink(ap: &mut VopReadlinkArgs<'_, '_>) -> Result<(), Errno> {
    let vp = ap.a_vp;
    let ip = VTOI(vp);
    let fmp = ip.i_fmp;
    let uio = &mut *ap.a_uio;
    let p = proc_or_cur(uio.uio_procp);

    if fmp.sess_init.get() == 0 {
        return Err(Errno::ENXIO);
    }
    if uio.uio_resid == 0 {
        return Ok(());
    }
    if uio.uio_offset < 0 {
        return Err(Errno::EINVAL);
    }

    if fmp.undef_op.get() & UNDEF_READLINK != 0 {
        return Err(Errno::ENOSYS);
    }

    let fbuf = fb_setup(0, ip.i_number, FUSE_READLINK, p);

    if let Err(error) = fb_queue(fmp.dev, fbuf) {
        if error == Errno::ENOSYS {
            fmp.undef_op.set(fmp.undef_op.get() | UNDEF_READLINK);
        }

        fb_delete(fbuf);
        return Err(error);
    }

    // SAFETY: the reply is in, the fusebuf is off the device's queues: its data is ours.
    let dat = unsafe { fbuf.fb_dat_slice() };
    if dat.contains(&0) {
        // DPRINTF("symbolic link contains embedded NUL: %s\n", fbuf->fb_dat);

        fb_delete(fbuf);
        return Err(Errno::EIO);
    }

    let error = uiomove(dat, uio);
    fb_delete(fbuf);

    error
}

/// `fusefs_reclaim` (`vop_reclaim`): releases the handles, tells the daemon to forget the
/// node (`FUSE_FORGET` with its lookup count), unhashes and frees it.
pub fn fusefs_reclaim(ap: &mut VopReclaimArgs<'_>) -> Result<(), Errno> {
    let vp = ap.a_vp;
    let p = proc_or_cur(ap.a_p);
    let ip = VTOI(vp);
    let fmp = ip.i_fmp;

    // Close opened files.
    for i in 0..FUFH_MAXTYPE.idx() {
        let fufh = ip.fufh(FufhType::from_idx(i));
        if fufh.fh_type != FUFH_INVALID {
            // DPRINTF("vnode being reclaimed is valid\n");
            let _ = fusefs_file_close(fmp, ip, fufh.fh_type, i as i32, vp.v_type.get() == VDIR, p);
        }
    }

    // If the fuse connection is opened ask libfuse to free the vnodes.
    if fmp.sess_init.get() != 0 && ip.i_number != FUSE_ROOT_ID {
        let fbuf = fb_setup(0, ip.i_number, FUSE_FORGET, p);
        fbuf.op_set(&FuseForgetIn {
            nlookup: ip.nlookup.get(),
        });
        fuse_device_queue_fbuf(fmp.dev, fbuf);
        // FUSE_FORGET has no response
    }

    // Remove the inode from its hash chain.
    crate::miscfs::fuse::fuse_ihash::fuse_ihashrem(ip);

    crate::kern::kern_malloc::free(
        NonNull::from(ip).cast(),
        crate::sys::malloc::M_FUSEFS,
        size_of::<crate::miscfs::fuse::fusefs_node::FusefsNode>(),
    );
    vp.v_data.set(ptr::null_mut());

    // Must return success otherwise kernel panic in vclean(9).
    Ok(())
}

/// `fusefs_print` (`vop_print`): describe the node (`DEBUG`/`DIAGNOSTIC` kernels only).
pub fn fusefs_print(ap: &mut VopPrintArgs) -> Result<(), Errno> {
    #[cfg(any(feature = "debug", feature = "diagnostic"))]
    {
        let vp = ap.a_vp;
        let ip = VTOI(vp);

        // Complete the information given by vprint().
        crate::kern::subr_prf::printf(format_args!("tag VT_FUSE, hash id {} ", ip.i_number));
        crate::kern::subr_prf::printf(format_args!("\n"));
    }
    #[cfg(not(any(feature = "debug", feature = "diagnostic")))]
    let _ = ap;
    Ok(())
}

/// `fusefs_create` (`vop_create`): `FUSE_MKNOD` of a regular file.
pub fn fusefs_create(ap: &mut VopCreateArgs<'_>) -> Result<(), Errno> {
    let dvp = ap.a_dvp;
    let p = cn_proc(ap.a_cnp);

    let ip = VTOI(dvp);
    let fmp = ip.i_fmp;
    let mode = makeimode(ap.a_vap.va_type, ap.a_vap.va_mode);

    if fmp.sess_init.get() == 0 {
        let _ = VOP_ABORTOP(dvp, ap.a_cnp);
        return Err(Errno::ENXIO);
    }

    if fmp.undef_op.get() & UNDEF_MKNOD != 0 {
        let _ = VOP_ABORTOP(dvp, ap.a_cnp);
        return Err(Errno::ENOSYS);
    }

    let name = ap.a_cnp.name();
    let fbuf = fb_setup(name.len() + 1, ip.i_number, FUSE_MKNOD, p);

    fbuf.op_set(&FuseMknodIn {
        mode,
        umask: p.fd().fd_cmask.get(),
        ..FuseMknodIn::default()
    });
    fb_put_name(fbuf, 0, name);

    let error = 'out: {
        if let Err(error) = fb_queue(fmp.dev, fbuf) {
            if error == Errno::ENOSYS {
                fmp.undef_op.set(fmp.undef_op.get() | UNDEF_MKNOD);
            }

            break 'out Err(error);
        }

        // mknod returns a fuse_entry_out with the ino of the new file
        let nodeid = fbuf.op_get::<FuseEntryOut>().nodeid;
        if nodeid == 0 || nodeid == FUSE_ROOT_ID {
            break 'out Err(Errno::EIO);
        }

        let tdp = match VFS_VGET(fmp.mp, nodeid) {
            Ok(tdp) => tdp,
            Err(e) => break 'out Err(e),
        };

        tdp.v_type.set(VREG);

        *ap.a_vpp = Some(tdp);
        VN_KNOTE(ap.a_dvp, NOTE_WRITE);
        Ok(())
    };

    // out:
    fb_delete(fbuf);
    pnbuf_free(ap.a_cnp);
    error
}

/// `fusefs_mknod` (`vop_mknod`): `FUSE_MKNOD`; the new node is dropped from the cache so
/// that it is reloaded (and checked for an alias) by `VFS_VGET`.
pub fn fusefs_mknod(ap: &mut VopMknodArgs<'_>) -> Result<(), Errno> {
    let dvp = ap.a_dvp;
    let p = cn_proc(ap.a_cnp);

    let ip = VTOI(dvp);
    let fmp = ip.i_fmp;

    if fmp.sess_init.get() == 0 {
        let _ = VOP_ABORTOP(dvp, ap.a_cnp);
        return Err(Errno::ENXIO);
    }

    if fmp.undef_op.get() & UNDEF_MKNOD != 0 {
        let _ = VOP_ABORTOP(dvp, ap.a_cnp);
        return Err(Errno::ENOSYS);
    }

    let name = ap.a_cnp.name();
    let fbuf = fb_setup(name.len() + 1, ip.i_number, FUSE_MKNOD, p);

    let mut mknod = FuseMknodIn {
        mode: makeimode(ap.a_vap.va_type, ap.a_vap.va_mode),
        umask: p.fd().fd_cmask.get(),
        ..FuseMknodIn::default()
    };
    if ap.a_vap.va_rdev != VNOVAL {
        mknod.rdev = ap.a_vap.va_rdev as u32;
    }
    fbuf.op_set(&mknod);
    fb_put_name(fbuf, 0, name);

    let error = 'out: {
        if let Err(error) = fb_queue(fmp.dev, fbuf) {
            if error == Errno::ENOSYS {
                fmp.undef_op.set(fmp.undef_op.get() | UNDEF_MKNOD);
            }

            break 'out Err(error);
        }

        // mknod returns a fuse_entry_out with the ino of the new file
        let nodeid = fbuf.op_get::<FuseEntryOut>().nodeid;
        if nodeid == 0 || nodeid == FUSE_ROOT_ID {
            break 'out Err(Errno::EIO);
        }

        let tdp = match VFS_VGET(fmp.mp, nodeid) {
            Ok(tdp) => tdp,
            Err(e) => break 'out Err(e),
        };

        // Don't trust the type returned by the file system and assume it created what it
        // was asked.
        tdp.v_type.set(ap.a_vap.va_type);

        *ap.a_vpp = Some(tdp);
        VN_KNOTE(ap.a_dvp, NOTE_WRITE);

        // Remove inode so that it will be reloaded by VFS_VGET and checked to see if it is
        // an alias of an existing entry in the inode cache.
        vput(tdp);
        tdp.v_type.set(VNON);
        vgone(tdp);
        *ap.a_vpp = None;
        Ok(())
    };

    // out:
    fb_delete(fbuf);
    pnbuf_free(ap.a_cnp);
    error
}

/// `fusefs_read` (`vop_read`): `FUSE_READ`s of at most `max_read` bytes until the request
/// is done or the daemon returns less than asked.
pub fn fusefs_read(ap: &mut VopReadArgs<'_, '_>) -> Result<(), Errno> {
    let vp = ap.a_vp;
    let uio = &mut *ap.a_uio;
    let p = proc_or_cur(uio.uio_procp);

    let ip = VTOI(vp);
    let fmp = ip.i_fmp;

    if fmp.sess_init.get() == 0 {
        return Err(Errno::ENXIO);
    }
    if uio.uio_resid == 0 {
        return Ok(());
    }
    if uio.uio_offset < 0 {
        return Err(Errno::EINVAL);
    }

    let mut fbuf = None;
    let mut error = Ok(());
    while uio.uio_resid > 0 {
        let f = fb_setup(0, ip.i_number, FUSE_READ, p);
        fbuf = Some(f);

        let size = uio.uio_resid.min(fmp.max_read as usize);
        f.op_set(&FuseReadIn {
            fh: fusefs_fd_get(ip, FUFH_RDONLY),
            offset: uio.uio_offset as u64,
            size: size as u32,
            ..FuseReadIn::default()
        });

        error = fb_queue(fmp.dev, f);

        if error.is_err() {
            break;
        }

        // SAFETY: the reply is in, the fusebuf is off the device's queues: its data is ours.
        let dat = unsafe { f.fb_dat_slice() };
        let n = (size as u64).min(f.fb_len()) as usize;
        error = uiomove(&mut dat[..n], uio);
        if error.is_err() {
            break;
        }

        if f.fb_len() < size as u64 {
            break;
        }

        fb_delete(f);
        fbuf = None;
    }

    fb_delete(fbuf);
    error
}

/// `fusefs_write` (`vop_write`): `FUSE_WRITE`s of at most `max_write` bytes; the daemon may
/// write less than asked, which the uio gets back.
pub fn fusefs_write(ap: &mut VopWriteArgs<'_, '_>) -> Result<(), Errno> {
    let vp = ap.a_vp;
    let ioflag = ap.a_ioflag;
    let uio = &mut *ap.a_uio;
    let p = proc_or_cur(uio.uio_procp);
    let cred = p.ucred();

    let ip = VTOI(vp);
    let fmp = ip.i_fmp;

    // XXX fmp->max_write will not be set to the value wanted by the file system if
    // sess_init == PENDING. It is not currently possible for this to be the case since a
    // write operation cannot take place on a vnode until after VOP_LOOKUP(9) has
    // successfully returned. At which point, the file system must have responded to
    // FUSE_INIT and sess_init will be 1 and max_write will be correctly set.
    if fmp.sess_init.get() == 0 {
        return Err(Errno::ENXIO);
    }
    if uio.uio_resid == 0 {
        return Ok(());
    }

    if ioflag & IO_APPEND != 0 {
        let mut vattr = Vattr::new();
        VOP_GETATTR(vp, &mut vattr, ptr::from_ref(cred), p)?;

        uio.uio_offset = vattr.va_size as i64;
    }

    let mut fbuf = None;
    let mut error = Ok(());
    while uio.uio_resid > 0 {
        let len = uio.uio_resid.min(fmp.max_write.get() as usize);
        let f = fb_setup(len, ip.i_number, FUSE_WRITE, p);
        fbuf = Some(f);

        f.op_set(&FuseWriteIn {
            fh: fusefs_fd_get(ip, FUFH_WRONLY),
            offset: uio.uio_offset as u64,
            size: len as u32,
            ..FuseWriteIn::default()
        });

        // SAFETY: a fresh fusebuf, not queued yet: its data is ours.
        error = uiomove(unsafe { f.fb_dat_slice() }, uio);
        if error.is_err() {
            // DPRINTF("uio error %i\n", error);
            break;
        }

        error = fb_queue(fmp.dev, f);

        if error.is_err() {
            break;
        }

        let written = f.op_get::<FuseWriteOut>().size as usize;
        let diff = len.wrapping_sub(written);
        if written > len {
            error = Err(Errno::EINVAL);
            break;
        }

        uio.uio_resid += diff;
        uio.uio_offset -= diff as i64;

        if uio.uio_offset > ip.filesize.get() {
            ip.filesize.set(uio.uio_offset);
            uvm_vnp_setsize(vp, uio.uio_offset);
        }
        uvm_vnp_uncache(vp);

        fb_delete(f);
        fbuf = None;
    }

    fb_delete(fbuf);
    error
}

/// The C's `abortit:` sequence of `fusefs_rename`: release everything the rename was given.
fn fusefs_rename_abort(ap: &mut VopRenameArgs<'_>) {
    let tdvp = ap.a_tdvp;
    let tvp = ap.a_tvp;
    let _ = VOP_ABORTOP(tdvp, ap.a_tcnp); // XXX, why not in NFS?
    if tvp.is_some_and(|tvp| ptr::eq(tdvp, tvp)) {
        vrele(tdvp);
    } else {
        vput(tdvp);
    }
    if let Some(tvp) = tvp {
        vput(tvp);
    }
    let _ = VOP_ABORTOP(ap.a_fdvp, ap.a_fcnp); // XXX, why not in NFS?
    vrele(ap.a_fdvp);
    vrele(ap.a_fvp);
}

/// `fusefs_rename` (`vop_rename`): `FUSE_RENAME` with both names and the target directory.
pub fn fusefs_rename(ap: &mut VopRenameArgs<'_>) -> Result<(), Errno> {
    let tvp = ap.a_tvp;
    let tdvp = ap.a_tdvp;
    let fvp = ap.a_fvp;
    let fdvp = ap.a_fdvp;
    let p = cn_proc(ap.a_fcnp);

    #[cfg(feature = "diagnostic")]
    {
        use crate::sys::namei::HASBUF;
        if ap.a_tcnp.cn_flags & HASBUF == 0 || ap.a_fcnp.cn_flags & HASBUF == 0 {
            panic(format_args!("fusefs_rename: no name"));
        }
    }

    // Check for cross-device rename.
    let mnt_of = |vp: &Vnode| vp.v_mount.get().map_or(ptr::null(), ptr::from_ref);
    if !ptr::eq(mnt_of(fvp), mnt_of(tdvp))
        || tvp.is_some_and(|tvp| !ptr::eq(mnt_of(fvp), mnt_of(tvp)))
    {
        fusefs_rename_abort(ap);
        return Err(Errno::EXDEV);
    }

    // If source and dest are the same, do nothing.
    if tvp.is_some_and(|tvp| ptr::eq(tvp, fvp)) {
        fusefs_rename_abort(ap);
        return Ok(());
    }

    if let Err(error) = vn_lock(fvp, LK_EXCLUSIVE | LK_RETRY) {
        fusefs_rename_abort(ap);
        return Err(error);
    }
    let dp = VTOI(fdvp);
    let ip = VTOI(fvp);
    let fmp = ip.i_fmp;

    // Be sure we are not renaming ".", "..", or an alias of ".". This leads to a crippled
    // directory tree. It's pretty tough to do a "ls" or "pwd" with the "." directory entry
    // missing, and "cd .." doesn't work if the ".." entry is missing.
    if fvp.v_type.get() == VDIR {
        // Avoid ".", "..", and aliases of "." for obvious reasons.
        if ap.a_fcnp.name() == b"."
            || ptr::eq(dp, ip)
            || ap.a_fcnp.cn_flags & ISDOTDOT != 0
            || ap.a_tcnp.cn_flags & ISDOTDOT != 0
        {
            let _ = VOP_UNLOCK(fvp);
            fusefs_rename_abort(ap);
            return Err(Errno::EINVAL);
        }
    }
    VN_KNOTE(fdvp, NOTE_WRITE); // XXX right place?

    if fmp.sess_init.get() == 0 {
        let _ = VOP_UNLOCK(fvp);
        fusefs_rename_abort(ap);
        return Err(Errno::ENXIO);
    }

    if fmp.undef_op.get() & UNDEF_RENAME != 0 {
        let _ = VOP_UNLOCK(fvp);
        fusefs_rename_abort(ap);
        return Err(Errno::ENOSYS);
    }

    let fname = ap.a_fcnp.name();
    let tname = ap.a_tcnp.name();
    let fbuf = fb_setup(fname.len() + tname.len() + 2, dp.i_number, FUSE_RENAME, p);

    fb_put_name(fbuf, 0, fname);
    fb_put_name(fbuf, fname.len() + 1, tname);
    fbuf.op_set(&FuseRenameIn {
        newdir: VTOI(tdvp).i_number,
    });

    if let Err(error) = fb_queue(fmp.dev, fbuf) {
        if error == Errno::ENOSYS {
            fmp.undef_op.set(fmp.undef_op.get() | UNDEF_RENAME);
        }

        fb_delete(fbuf);
        let _ = VOP_UNLOCK(fvp);
        fusefs_rename_abort(ap);
        return Err(error);
    }

    fb_delete(fbuf);
    VN_KNOTE(fvp, NOTE_RENAME);

    let _ = VOP_UNLOCK(fvp);
    if tvp.is_some_and(|tvp| ptr::eq(tdvp, tvp)) {
        vrele(tdvp);
    } else {
        vput(tdvp);
    }
    if let Some(tvp) = tvp {
        vput(tvp);
    }
    vrele(fdvp);
    vrele(fvp);

    Ok(())
}

/// `fusefs_mkdir` (`vop_mkdir`): `FUSE_MKDIR`.
pub fn fusefs_mkdir(ap: &mut VopMkdirArgs<'_>) -> Result<(), Errno> {
    let dvp = ap.a_dvp;
    let p = cn_proc(ap.a_cnp);

    let ip = VTOI(dvp);
    let fmp = ip.i_fmp;

    let error = 'out: {
        if fmp.sess_init.get() == 0 {
            break 'out Err(Errno::ENXIO);
        }

        if fmp.undef_op.get() & UNDEF_MKDIR != 0 {
            break 'out Err(Errno::ENOSYS);
        }

        let name = ap.a_cnp.name();
        let fbuf = fb_setup(name.len() + 1, ip.i_number, FUSE_MKDIR, p);

        fbuf.op_set(&FuseMkdirIn {
            mode: makeimode(ap.a_vap.va_type, ap.a_vap.va_mode),
            umask: p.fd().fd_cmask.get(),
        });
        fb_put_name(fbuf, 0, name);

        if let Err(error) = fb_queue(fmp.dev, fbuf) {
            if error == Errno::ENOSYS {
                fmp.undef_op.set(fmp.undef_op.get() | UNDEF_MKDIR);
            }

            fb_delete(fbuf);
            break 'out Err(error);
        }

        // mkdir returns a fuse_entry_out with the ino of the new directory
        let nodeid = fbuf.op_get::<FuseEntryOut>().nodeid;
        if nodeid == 0 || nodeid == FUSE_ROOT_ID {
            fb_delete(fbuf);
            break 'out Err(Errno::EIO);
        }

        let tdp = match VFS_VGET(fmp.mp, nodeid) {
            Ok(tdp) => tdp,
            Err(e) => {
                fb_delete(fbuf);
                break 'out Err(e);
            }
        };

        tdp.v_type.set(VDIR);

        *ap.a_vpp = Some(tdp);
        VN_KNOTE(ap.a_dvp, NOTE_WRITE | NOTE_LINK);
        fb_delete(fbuf);
        Ok(())
    };

    // out:
    pnbuf_free(ap.a_cnp);
    vput(dvp);
    error
}

/// `fusefs_rmdir` (`vop_rmdir`): `FUSE_RMDIR`.
pub fn fusefs_rmdir(ap: &mut VopRmdirArgs<'_>) -> Result<(), Errno> {
    let vp = ap.a_vp;
    let mut dvp = Some(ap.a_dvp);
    let p = cn_proc(ap.a_cnp);

    let ip = VTOI(vp);
    let dp = VTOI(ap.a_dvp);
    let fmp = ip.i_fmp;

    let error = 'out: {
        if fmp.sess_init.get() == 0 {
            break 'out Err(Errno::ENXIO);
        }

        if fmp.undef_op.get() & UNDEF_RMDIR != 0 {
            break 'out Err(Errno::ENOSYS);
        }

        // Don't delete parent since it's clearly not empty.
        let name = ap.a_cnp.name();
        if name == b".." {
            break 'out Err(Errno::ENOTEMPTY);
        }

        VN_KNOTE(ap.a_dvp, NOTE_WRITE | NOTE_LINK);

        let fbuf = fb_setup(name.len() + 1, dp.i_number, FUSE_RMDIR, p);
        fb_put_name(fbuf, 0, name);

        if let Err(error) = fb_queue(fmp.dev, fbuf) {
            if error == Errno::ENOSYS {
                fmp.undef_op.set(fmp.undef_op.get() | UNDEF_RMDIR);
            }
            if error != Errno::ENOTEMPTY {
                VN_KNOTE(ap.a_dvp, NOTE_WRITE | NOTE_LINK);
            }

            fb_delete(fbuf);
            break 'out Err(error);
        }

        vput(ap.a_dvp);
        dvp = None;

        fb_delete(fbuf);
        Ok(())
    };

    // out:
    if let Some(dvp) = dvp {
        vput(dvp);
    }
    VN_KNOTE(vp, NOTE_DELETE);
    pnbuf_free(ap.a_cnp);
    vput(vp);
    error
}

/// `fusefs_remove` (`vop_remove`): `FUSE_UNLINK`.
pub fn fusefs_remove(ap: &mut VopRemoveArgs<'_>) -> Result<(), Errno> {
    let vp = ap.a_vp;
    let dvp = ap.a_dvp;
    let p = cn_proc(ap.a_cnp);

    let ip = VTOI(vp);
    let dp = VTOI(dvp);
    let fmp = ip.i_fmp;

    let error = 'out: {
        if fmp.sess_init.get() == 0 {
            break 'out Err(Errno::ENXIO);
        }

        if fmp.undef_op.get() & UNDEF_REMOVE != 0 {
            break 'out Err(Errno::ENOSYS);
        }

        let name = ap.a_cnp.name();
        let fbuf = fb_setup(name.len() + 1, dp.i_number, FUSE_UNLINK, p);
        fb_put_name(fbuf, 0, name);

        if let Err(error) = fb_queue(fmp.dev, fbuf) {
            if error == Errno::ENOSYS {
                fmp.undef_op.set(fmp.undef_op.get() | UNDEF_REMOVE);
            }

            fb_delete(fbuf);
            break 'out Err(error);
        }

        VN_KNOTE(vp, NOTE_DELETE);
        VN_KNOTE(dvp, NOTE_WRITE);
        fb_delete(fbuf);
        Ok(())
    };

    // out:
    pnbuf_free(ap.a_cnp);
    error
}

/// `fusefs_strategy` (`vop_strategy`): FUSE has no buffers.
pub fn fusefs_strategy(_ap: &mut VopStrategyArgs) -> Result<(), Errno> {
    Ok(())
}

/// `fusefs_lock` (`vop_lock`): take the node's lock.
pub fn fusefs_lock(ap: &mut VopLockArgs) -> Result<(), Errno> {
    let vp = ap.a_vp;

    rrw_enter(&VTOI(vp).i_lock, ap.a_flags & LK_RWFLAGS)
}

/// `fusefs_unlock` (`vop_unlock`).
pub fn fusefs_unlock(ap: &mut VopUnlockArgs) -> Result<(), Errno> {
    let vp = ap.a_vp;

    rrw_exit(&VTOI(vp).i_lock);
    Ok(())
}

/// `fusefs_islocked` (`vop_islocked`).
pub fn fusefs_islocked(ap: &mut VopIslockedArgs) -> i32 {
    rrw_status(&VTOI(ap.a_vp).i_lock)
}

/// `fusefs_advlock` (`vop_advlock`): advisory record locking on the node.
pub fn fusefs_advlock(ap: &mut VopAdvlockArgs<'_>) -> Result<(), Errno> {
    let ip = VTOI(ap.a_vp);

    lf_advlock(
        &ip.i_lockf,
        ip.filesize.get(),
        ap.a_id,
        ap.a_op,
        ap.a_fl,
        ap.a_flags,
    )
}

/// `fusefs_fsync` (`vop_fsync`): `FUSE_FSYNC` of every writable handle.
pub fn fusefs_fsync(ap: &mut VopFsyncArgs<'_>) -> Result<(), Errno> {
    let vp = ap.a_vp;
    let p = ap.a_p;
    let mut error = Ok(());

    // Can't write to directory file handles so no need to fsync. FUSE has fsyncdir but it
    // doesn't make sense on OpenBSD.
    if vp.v_type.get() == VDIR {
        return Ok(());
    }

    let ip = VTOI(vp);
    let fmp = ip.i_fmp;

    if fmp.sess_init.get() == 0 {
        return Err(Errno::ENXIO);
    }

    // Implementing fsync is optional so don't error.
    if fmp.undef_op.get() & UNDEF_FSYNC != 0 {
        return Ok(());
    }

    // Sync all writeable file descriptors.
    for i in 0..FUFH_MAXTYPE.idx() {
        let fufh = ip.fufh(FufhType::from_idx(i));
        if fufh.fh_type == FUFH_WRONLY || fufh.fh_type == FUFH_RDWR {
            let fbuf = fb_setup(0, ip.i_number, FUSE_FSYNC, p);
            // fdatasync(2) is just a wrapper around fsync(2) so datasync is always false.
            fbuf.op_set(&FuseFsyncIn {
                fh: fufh.fh_id,
                fsync_flags: 0,
                ..FuseFsyncIn::default()
            });

            // Always behave as if ap->a_waitfor = MNT_WAIT.
            error = fb_queue(fmp.dev, fbuf);
            fb_delete(fbuf);
            if error.is_err() {
                break;
            }
        }
    }

    if error == Err(Errno::ENOSYS) {
        fmp.undef_op.set(fmp.undef_op.get() | UNDEF_FSYNC);

        // Implementing fsync is optional so don't error.
        return Ok(());
    }

    error
}
/* </CODE> */
