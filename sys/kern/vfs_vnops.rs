/*	$OpenBSD: vfs_vnops.c,v 1.128 2026/06/12 12:20:25 kirill Exp $	*/
/*	$NetBSD: vfs_vnops.c,v 1.20 1996/02/04 02:18:41 christos Exp $	*/
/* <LICENSES> */
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
 *	@(#)vfs_vnops.c	8.5 (Berkeley) 12/8/94
 */
/* </LICENSES> */

//! Common code for vnode operations: `vn_open` (permission checks and `VOP_OPEN` or
//! `VOP_CREATE`), `vn_close`, `vn_rdwr`, `vn_stat`, `vn_lock`, `vn_writechk`, `vn_fsizechk`,
//! `vn_isunder`, and the file table's vnode operations (`vnops`: `vn_read`, `vn_write`,
//! `vn_ioctl`, `vn_kqfilter`, `vn_statfile`, `vn_closefile`, `vn_seek`).
//!
//! Upstream: sys/kern/vfs_vnops.c @ 3ce1f3f79392
//!
//! ## Deviations
//! - `vn_rdwr` takes the buffer as a pointer and a length, as the C's `caddr_t base, int
//!   len`, because `segflg` says whether it is a user or a kernel address; `aresid` is an
//!   `Option<&mut usize>`. `vn_fsizechk` returns the overrun instead of filling `*overrun`.
//! - `File::vnode` is this port's accessor for `(struct vnode *)fp->f_data` of a
//!   `DTYPE_VNODE` file.
//! - `vn_ioctl`'s `TIOCSCTTY` stores the vnode in the session's `s_ttyvp`, which stays an
//!   opaque pointer until the tty layer (M10).
//! - `KERNEL_LOCK()`/`KERNEL_UNLOCK()` are nothing without `MULTIPROCESSOR`.

use core::ffi::c_void;
use core::ptr::{self, NonNull};

use crate::kassert;
use crate::kern::kern_lock::{mtx_enter, mtx_leave};
use crate::kern::kern_malloc::free;
use crate::kern::kern_resource::lim_cur_proc;
use crate::kern::kern_sig::psignal;
use crate::kern::kern_synch::{msleep_nsec, wakeup_one};
use crate::kern::vfs_getcwd::vfs_getcwd_common;
use crate::kern::vfs_lookup::namei;
use crate::kern::vfs_subr::{VNODE_MTX, uvm_vnp_uncache, vattr_null, vput, vref, vrele};
use crate::kern::vfs_vops::{
    VOP_ABORTOP, VOP_ACCESS, VOP_ADVLOCK, VOP_CLOSE, VOP_CREATE, VOP_GETATTR, VOP_IOCTL,
    VOP_KQFILTER, VOP_LOCK, VOP_OPEN, VOP_READ, VOP_SETATTR, VOP_UNLOCK, VOP_WRITE,
};
use crate::sys::errno::Errno;
use crate::sys::fcntl::{
    F_FLOCK, F_UNLCK, FFSYNC, FNONBLOCK, FREAD, FWRITE, Flock, O_APPEND, O_CREAT, O_DIRECTORY,
    O_EXCL, O_NOFOLLOW, O_TRUNC,
};
use crate::sys::file::{DTYPE_VNODE, FIF_HASLOCK, FO_POSITION, File, Fileops};
use crate::sys::filio::{FIOASYNC, FIONREAD};
use crate::sys::limits::LLONG_MAX;
use crate::sys::lock::{LK_EXCLUSIVE, LK_RETRY};
use crate::sys::malloc::M_TEMP;
use crate::sys::mount::{MNT_RDONLY, MNT_SYNCHRONOUS};
use crate::sys::namei::{
    BYPASSUNVEIL, CREATE, FOLLOW, KERNELPATH, LOCKLEAF, LOCKPARENT, LOOKUP, NOFOLLOW, Nameidata,
};
use crate::sys::param::{MAXPATHLEN, PINOD};
use crate::sys::proc::Proc;
use crate::sys::resource::RLIMIT_FSIZE;
use crate::sys::signal::SIGXFSZ;
use crate::sys::specdev::Cloneinfo;
use crate::sys::stat::{
    S_BLKSIZE, S_IFBLK, S_IFCHR, S_IFDIR, S_IFIFO, S_IFLNK, S_IFREG, S_IFSOCK, Stat,
};
use crate::sys::systm::INFSLP;
use crate::sys::ttycom::TIOCSCTTY;
use crate::sys::types::Off;
use crate::sys::ucred::Ucred;
use crate::sys::uio::{Iovec, Uio, UioRw, UioSeg};
use crate::sys::unistd::{SEEK_CUR, SEEK_END, SEEK_SET};
use crate::sys::vnode::{
    IO_APPEND, IO_NDELAY, IO_NODELOCKED, IO_NOLIMIT, IO_SYNC, IO_UNIT, VA_EXCLUSIVE, VBAD, VBLK,
    VCHR, VCLONED, VDIR, VFIFO, VLNK, VNON, VREAD, VREG, VSOCK, VTEXT, VWRITE, VXLOCK, VXWANT,
    Vattr, Vnode,
};

/// `vnops`: the file operations of a `DTYPE_VNODE` file.
pub static VNOPS: Fileops = Fileops {
    fo_read: vn_read,
    fo_write: vn_write,
    fo_ioctl: vn_ioctl,
    fo_kqfilter: vn_kqfilter,
    fo_stat: vn_statfile,
    fo_close: vn_closefile,
    fo_seek: Some(vn_seek),
};

impl File {
    /// `(struct vnode *)fp->f_data` of a `DTYPE_VNODE` file.
    pub fn vnode(&self) -> &'static Vnode {
        let vp = self.f_data.get().cast::<Vnode>();
        if self.f_type.get() != DTYPE_VNODE || vp.is_null() {
            crate::kern::subr_prf::panic(format_args!("file {:p}: not a vnode", self));
        }
        // SAFETY: a `DTYPE_VNODE` file's `f_data` is the vnode `doopenat` (or `sys_fhopen`)
        // stored, written once before the file was inserted; the file holds a use count on
        // it, and vnodes are never freed.
        unsafe { &*vp }
    }
}

/// Common code for vnode open operations. Check permissions, and call the `VOP_OPEN` or
/// `VOP_CREATE` routine.
pub fn vn_open(ndp: &mut Nameidata<'_>, mut fmode: i32, cmode: u32) -> Result<(), Errno> {
    // SAFETY: `cn_proc` is the thread doing the open (`ndinitat`), which outlives it; the
    // reference is detached from `ndp` so `ndp` can be changed below.
    let p: &Proc = unsafe { &*ndp.ni_cnd.cn_proc };
    let cred = p.p_ucred.get();
    let mut va = Vattr::new();

    // The only valid flags to pass in here from NDINIT are KERNELPATH or BYPASSUNVEIL. This
    // function will override the nameiop based on the fmode and cmode flags, so validate that
    // our caller has not set other flags or operations in the nameidata structure.
    kassert!(ndp.ni_cnd.cn_flags & !(KERNELPATH | BYPASSUNVEIL) == 0);
    kassert!(ndp.ni_cnd.cn_nameiop == 0);

    if fmode & (FREAD | FWRITE) == 0 {
        return Err(Errno::EINVAL);
    }
    if fmode & (O_TRUNC | FWRITE) == O_TRUNC {
        return Err(Errno::EINVAL);
    }
    if fmode & (O_CREAT | O_DIRECTORY) == O_CREAT | O_DIRECTORY {
        return Err(Errno::EINVAL);
    }
    let vp: &'static Vnode;
    if fmode & O_CREAT != 0 {
        ndp.ni_cnd.cn_nameiop = CREATE;
        ndp.ni_cnd.cn_flags |= LOCKPARENT | LOCKLEAF;
        if fmode & O_EXCL == 0 && fmode & O_NOFOLLOW == 0 {
            ndp.ni_cnd.cn_flags |= FOLLOW;
        }
        namei(ndp)?;
        let dvp = expect_vnode(ndp.ni_dvp);

        match ndp.ni_vp {
            None => {
                vattr_null(&mut va);
                va.va_type = VREG;
                va.va_mode = cmode;
                if fmode & O_EXCL != 0 {
                    va.va_vaflags |= VA_EXCLUSIVE;
                }
                let error = VOP_CREATE(dvp, &mut ndp.ni_vp, &mut ndp.ni_cnd, &mut va);
                vput(dvp);
                error?;
                fmode &= !O_TRUNC;
                vp = expect_vnode(ndp.ni_vp);
            }
            Some(nvp) => {
                let _ = VOP_ABORTOP(dvp, &mut ndp.ni_cnd);
                if ptr::eq(dvp, nvp) {
                    vrele(dvp);
                } else {
                    vput(dvp);
                }
                ndp.ni_dvp = None;
                vp = nvp;
                if fmode & O_EXCL != 0 {
                    vput(vp);
                    return Err(Errno::EEXIST);
                }
                if vp.v_type.get() == VDIR {
                    vput(vp);
                    return Err(Errno::EISDIR);
                }
                fmode &= !O_CREAT;
            }
        }
    } else {
        ndp.ni_cnd.cn_nameiop = LOOKUP;
        ndp.ni_cnd.cn_flags |= if fmode & O_NOFOLLOW != 0 {
            NOFOLLOW
        } else {
            FOLLOW
        } | LOCKLEAF;
        namei(ndp)?;
        vp = expect_vnode(ndp.ni_vp);
    }

    let error = 'bad: {
        if vp.v_type.get() == VSOCK {
            break 'bad Errno::EOPNOTSUPP;
        }
        if vp.v_type.get() == VLNK {
            break 'bad Errno::ELOOP;
        }
        if fmode & O_DIRECTORY != 0 && vp.v_type.get() != VDIR {
            break 'bad Errno::ENOTDIR;
        }
        if fmode & O_CREAT == 0 {
            if fmode & FREAD != 0
                && let Err(e) = VOP_ACCESS(vp, VREAD, cred, p)
            {
                break 'bad e;
            }
            if fmode & FWRITE != 0 {
                if vp.v_type.get() == VDIR {
                    break 'bad Errno::EISDIR;
                }
                if let Err(e) = vn_writechk(vp).and_then(|()| VOP_ACCESS(vp, VWRITE, cred, p)) {
                    break 'bad e;
                }
            }
        }
        if fmode & O_TRUNC != 0 && vp.v_type.get() == VREG {
            vattr_null(&mut va);
            va.va_size = 0;
            if let Err(e) = VOP_SETATTR(vp, &mut va, cred, p) {
                break 'bad e;
            }
        }
        if let Err(e) = VOP_OPEN(vp, fmode, cred, p) {
            break 'bad e;
        }

        let mut vp = vp;
        if vp.v_flag.get() & VCLONED != 0 {
            let cip = vp.v_data.get().cast::<Cloneinfo>();
            // SAFETY: a `VCLONED` vnode's `v_data` is the `struct cloneinfo`
            // `spec_open_clone` allocated, until this restores the original.
            let (ci_vp, ci_data) = unsafe { ((*cip).ci_vp, (*cip).ci_data) };

            vp.v_flag.set(vp.v_flag.get() & !VCLONED);

            ndp.ni_vp = Some(ci_vp); // return cloned vnode
            vp.v_data.set(ci_data); // restore v_data
            let _ = VOP_UNLOCK(vp); // keep a reference
            vp = ci_vp; // for the increment below

            if let Some(cip) = NonNull::new(cip) {
                free(cip.cast(), M_TEMP, size_of::<Cloneinfo>());
            }
        }

        if fmode & FWRITE != 0 {
            vp.v_writecount.set(vp.v_writecount.get() + 1);
        }
        return Ok(());
    };
    vput(vp);
    Err(error)
}

/// The vnode a successful `namei`/`VOP_CREATE` left in a `nameidata` slot.
fn expect_vnode(vp: Option<&'static Vnode>) -> &'static Vnode {
    match vp {
        Some(vp) => vp,
        None => crate::kern::subr_prf::panic(format_args!("vn_open: no vnode")),
    }
}

/// Check for write permissions on the specified vnode. Prototype text segments cannot be
/// written.
pub fn vn_writechk(vp: &'static Vnode) -> Result<(), Errno> {
    // Disallow write attempts on read-only file systems; unless the file is a socket or a
    // block or character device resident on the file system.
    if let Some(mp) = vp.v_mount.get()
        && mp.mnt_flag.get() & MNT_RDONLY != 0
    {
        match vp.v_type.get() {
            VREG | VDIR | VLNK => return Err(Errno::EROFS),
            VNON | VCHR | VSOCK | VFIFO | VBAD | VBLK => {}
        }
    }
    // If there's shared text associated with the vnode, try to free it up once. If we fail,
    // we can't allow writing.
    if vp.v_flag.get() & VTEXT != 0 && !uvm_vnp_uncache(vp) {
        return Err(Errno::ETXTBSY);
    }

    Ok(())
}

/// Check whether a write operation would exceed the file size rlimit for the process, if
/// one should be applied for this operation. If a partial write should take place, the uio
/// is adjusted and the amount by which the request would have exceeded the limit is
/// returned.
pub fn vn_fsizechk(vp: &'static Vnode, uio: &mut Uio<'_>, ioflag: i32) -> Result<isize, Errno> {
    let mut overrun = 0;
    if vp.v_type.get() == VREG
        && let Some(p) = uio.uio_procp
        && ioflag & IO_NOLIMIT == 0
    {
        let limit = lim_cur_proc(p, RLIMIT_FSIZE).min(i64::MAX as u64) as i64;

        // if already at or over the limit, send the signal and fail
        if uio.uio_offset >= limit {
            psignal(p, SIGXFSZ);
            return Err(Errno::EFBIG);
        }

        // otherwise, clamp the write to stay under the limit
        let room = (limit - uio.uio_offset) as u64;
        if uio.uio_resid as u64 > room {
            overrun = (uio.uio_resid as u64 - room) as isize;
            uio.uio_resid = room as usize;
        }
    }

    Ok(overrun)
}

/// Mark a vnode as being the text image of a running process.
pub fn vn_marktext(vp: &'static Vnode) {
    vp.v_flag.set(vp.v_flag.get() | VTEXT);
}

/// Vnode close call.
pub fn vn_close(
    vp: &'static Vnode,
    flags: i32,
    cred: *const Ucred,
    p: Option<&Proc>,
) -> Result<(), Errno> {
    if flags & FWRITE != 0 {
        vp.v_writecount.set(vp.v_writecount.get() - 1);
    }
    let _ = vn_lock(vp, LK_EXCLUSIVE | LK_RETRY);
    let error = VOP_CLOSE(vp, flags, cred, p);
    vput(vp);
    error
}

/// Package up an I/O request on a vnode into a uio and do it.
#[allow(clippy::too_many_arguments)] // the C's signature
pub fn vn_rdwr(
    rw: UioRw,
    vp: &'static Vnode,
    base: *mut c_void,
    len: usize,
    offset: Off,
    segflg: UioSeg,
    ioflg: i32,
    cred: *const Ucred,
    aresid: Option<&mut usize>,
    p: Option<&Proc>,
) -> Result<(), Errno> {
    let mut aiov = [Iovec {
        iov_base: base,
        iov_len: len,
    }];
    let mut auio = Uio {
        uio_iov: &mut aiov,
        uio_offset: offset,
        uio_resid: len,
        uio_segflg: segflg,
        uio_rw: rw,
        uio_procp: p,
    };

    if ioflg & IO_NODELOCKED == 0 {
        let _ = vn_lock(vp, LK_EXCLUSIVE | LK_RETRY);
    }
    let mut error = if rw == UioRw::UIO_READ {
        VOP_READ(vp, &mut auio, ioflg, cred)
    } else {
        VOP_WRITE(vp, &mut auio, ioflg, cred)
    };
    if ioflg & IO_NODELOCKED == 0 {
        let _ = VOP_UNLOCK(vp);
    }

    match aresid {
        Some(aresid) => *aresid = auio.uio_resid,
        None => {
            if auio.uio_resid != 0 && error.is_ok() {
                error = Err(Errno::EIO);
            }
        }
    }
    error
}

/// File table vnode read routine.
pub fn vn_read(fp: &File, uio: &mut Uio<'_>, fflags: i32) -> Result<(), Errno> {
    let vp = fp.vnode();
    let cred = fp.f_cred.get();
    let count = uio.uio_resid;

    // KERNEL_LOCK(): nothing without MULTIPROCESSOR.

    let _ = vn_lock(vp, LK_EXCLUSIVE | LK_RETRY);

    let offset = if fflags & FO_POSITION == 0 {
        uio.uio_offset = fp.f_offset.get();
        uio.uio_offset
    } else {
        uio.uio_offset
    };

    let error = 'done: {
        // no wrap around of offsets except on character devices
        if vp.v_type.get() != VCHR && count as i64 > LLONG_MAX.wrapping_sub(offset) {
            break 'done Err(Errno::EINVAL);
        }

        if vp.v_type.get() == VDIR {
            break 'done Err(Errno::EISDIR);
        }

        let ioflag = if fp.flag() & FNONBLOCK != 0 {
            IO_NDELAY
        } else {
            0
        };
        let error = VOP_READ(vp, uio, ioflag, cred);
        if fflags & FO_POSITION == 0 {
            mtx_enter(&fp.f_mtx);
            fp.f_offset
                .set(fp.f_offset.get() + (count - uio.uio_resid) as Off);
            mtx_leave(&fp.f_mtx);
        }
        error
    };
    let _ = VOP_UNLOCK(vp);
    // KERNEL_UNLOCK().
    error
}

/// File table vnode write routine.
pub fn vn_write(fp: &File, uio: &mut Uio<'_>, fflags: i32) -> Result<(), Errno> {
    let vp = fp.vnode();
    let cred = fp.f_cred.get();
    let mut ioflag = IO_UNIT;

    // KERNEL_LOCK(): nothing without MULTIPROCESSOR.

    // note: pwrite/pwritev are unaffected by O_APPEND
    if vp.v_type.get() == VREG && fp.flag() & O_APPEND != 0 && fflags & FO_POSITION == 0 {
        ioflag |= IO_APPEND;
    }
    if fp.flag() & FNONBLOCK != 0 {
        ioflag |= IO_NDELAY;
    }
    if fp.flag() & FFSYNC != 0
        || vp
            .v_mount
            .get()
            .is_some_and(|mp| mp.mnt_flag.get() & MNT_SYNCHRONOUS != 0)
    {
        ioflag |= IO_SYNC;
    }
    let _ = vn_lock(vp, LK_EXCLUSIVE | LK_RETRY);
    if fflags & FO_POSITION == 0 {
        uio.uio_offset = fp.f_offset.get();
    }
    let count = uio.uio_resid;
    let error = VOP_WRITE(vp, uio, ioflag, cred);
    if fflags & FO_POSITION == 0 {
        mtx_enter(&fp.f_mtx);
        if ioflag & IO_APPEND != 0 {
            fp.f_offset.set(uio.uio_offset);
        } else {
            fp.f_offset
                .set(fp.f_offset.get() + (count - uio.uio_resid) as Off);
        }
        mtx_leave(&fp.f_mtx);
    }
    let _ = VOP_UNLOCK(vp);

    // KERNEL_UNLOCK().
    error
}

/// File table wrapper for `vn_stat`.
pub fn vn_statfile(fp: &File, sb: &mut Stat, p: &Proc) -> Result<(), Errno> {
    let vp = fp.vnode();

    // KERNEL_LOCK(): nothing without MULTIPROCESSOR.
    vn_stat(vp, sb, p)
}

/// vnode stat routine.
pub fn vn_stat(vp: &'static Vnode, sb: &mut Stat, p: &Proc) -> Result<(), Errno> {
    let mut va = Vattr::new();

    VOP_GETATTR(vp, &mut va, p.p_ucred.get(), p)?;
    // Copy from vattr table
    *sb = Stat::default();
    sb.st_dev = va.va_fsid as i32;
    sb.st_ino = va.va_fileid;
    let mut mode = va.va_mode;
    mode |= match vp.v_type.get() {
        VREG => S_IFREG,
        VDIR => S_IFDIR,
        VBLK => S_IFBLK,
        VCHR => S_IFCHR,
        VLNK => S_IFLNK,
        VSOCK => S_IFSOCK,
        VFIFO => S_IFIFO,
        _ => return Err(Errno::EBADF),
    };
    sb.st_mode = mode;
    sb.st_nlink = va.va_nlink;
    sb.st_uid = va.va_uid;
    sb.st_gid = va.va_gid;
    sb.st_rdev = va.va_rdev;
    sb.st_size = va.va_size as Off;
    sb.st_atim.tv_sec = va.va_atime.tv_sec;
    sb.st_atim.tv_nsec = va.va_atime.tv_nsec;
    sb.st_mtim.tv_sec = va.va_mtime.tv_sec;
    sb.st_mtim.tv_nsec = va.va_mtime.tv_nsec;
    sb.st_ctim.tv_sec = va.va_ctime.tv_sec;
    sb.st_ctim.tv_nsec = va.va_ctime.tv_nsec;
    sb.st_blksize = va.va_blocksize as i32;
    sb.st_flags = va.va_flags as u32;
    sb.st_gen = va.va_gen as u32;
    sb.st_blocks = (va.va_bytes / S_BLKSIZE as u64) as i64;
    Ok(())
}

/// File table vnode ioctl routine.
pub fn vn_ioctl(fp: &File, com: u64, data: &mut [u8], p: &Proc) -> Result<(), Errno> {
    let vp = fp.vnode();
    let mut vattr = Vattr::new();
    let mut error = Err(Errno::ENOTTY);

    // KERNEL_LOCK(): nothing without MULTIPROCESSOR.
    match vp.v_type.get() {
        VREG | VDIR => {
            if com == FIONREAD {
                error = VOP_GETATTR(vp, &mut vattr, p.p_ucred.get(), p);
                if error.is_ok() {
                    let n = (vattr.va_size as i64).wrapping_sub(crate::sys::file::foffset(fp));
                    if let Some(out) = data.get_mut(..size_of::<i32>()) {
                        out.copy_from_slice(&(n as i32).to_ne_bytes());
                    }
                }
            } else if com == FIOASYNC {
                // XXX
                error = Ok(()); // XXX
            }
        }

        VFIFO | VCHR | VBLK => {
            error = VOP_IOCTL(vp, com, data, fp.flag(), p.p_ucred.get(), p);
            if error.is_ok() && com == TIOCSCTTY {
                // SAFETY: the process's group and session are alive while it is.
                let s = unsafe { &*p.process().session() };
                let ovp = s.s_ttyvp.get();
                s.s_ttyvp.set(ptr::from_ref(vp).cast());
                vref(vp);
                if !ovp.is_null() {
                    // SAFETY: `s_ttyvp` only ever holds a vnode stored here, which holds a
                    // use count on it; vnodes are never freed.
                    vrele(unsafe { &*ovp.cast::<Vnode>() });
                }
            }
        }

        _ => {}
    }
    // KERNEL_UNLOCK().

    error
}

/// Check that the vnode is still valid, and if so acquire requested lock.
pub fn vn_lock(vp: &'static Vnode, flags: i32) -> Result<(), Errno> {
    loop {
        let error;
        mtx_enter(&VNODE_MTX);
        if vp.v_lflag.get() & VXLOCK != 0 {
            vp.v_lflag.set(vp.v_lflag.get() | VXWANT);
            let _ = msleep_nsec(ptr::from_ref(vp), &VNODE_MTX, PINOD, "vn_lock", INFSLP);
            mtx_leave(&VNODE_MTX);
            error = Errno::ENOENT;
        } else {
            vp.v_lockcount.set(vp.v_lockcount.get() + 1);
            mtx_leave(&VNODE_MTX);

            let lock = VOP_LOCK(vp, flags);

            mtx_enter(&VNODE_MTX);
            vp.v_lockcount.set(vp.v_lockcount.get() - 1);
            let do_wakeup = vp.v_lockcount.get() == 0;
            let xlocked = vp.v_lflag.get() & VXLOCK != 0;
            mtx_leave(&VNODE_MTX);

            match lock {
                Ok(()) => {
                    if !xlocked {
                        return Ok(());
                    }

                    // The vnode was exclusively locked while acquiring the requested lock.
                    // Release it and try again.
                    error = Errno::ENOENT;
                    let _ = VOP_UNLOCK(vp);
                }
                Err(e) => error = e,
            }
            if do_wakeup && xlocked {
                wakeup_one(ptr::from_ref(&vp.v_lockcount));
            }
        }
        if flags & LK_RETRY == 0 {
            return Err(error);
        }
    }
}

/// File table vnode close routine.
pub fn vn_closefile(fp: &File, p: Option<&Proc>) -> Result<(), Errno> {
    let vp = fp.vnode();

    // KERNEL_LOCK(): nothing without MULTIPROCESSOR.
    if fp.f_iflags.load(core::sync::atomic::Ordering::SeqCst) & FIF_HASLOCK != 0 {
        let mut lf = Flock {
            l_whence: SEEK_SET as i16,
            l_start: 0,
            l_len: 0,
            l_type: F_UNLCK,
            ..Flock::default()
        };
        let _ = VOP_ADVLOCK(
            vp,
            ptr::from_ref(fp).cast(),
            i32::from(F_UNLCK),
            &mut lf,
            F_FLOCK,
        );
    }
    let error = vn_close(vp, fp.flag(), fp.f_cred.get(), p);
    // KERNEL_UNLOCK().
    error
}

/// `vn_kqfilter(fp, kn)`: `kn` is a `struct knote *` (`kern_event.c`).
pub fn vn_kqfilter(fp: &File, kn: *mut c_void) -> Result<(), Errno> {
    // KERNEL_LOCK(): nothing without MULTIPROCESSOR.
    VOP_KQFILTER(fp.vnode(), fp.flag(), kn)
}

/// `vn_seek(fp, offset, whence, p)`: the file table's `lseek` of a vnode.
pub fn vn_seek(fp: &File, offset: &mut Off, whence: i32, p: &Proc) -> Result<(), Errno> {
    let cred = p.p_ucred.get();
    let vp = fp.vnode();
    let mut vattr = Vattr::new();

    if vp.v_type.get() == VFIFO {
        return Err(Errno::ESPIPE);
    }

    let _ = vn_lock(vp, LK_EXCLUSIVE | LK_RETRY);

    let special = vp.v_type.get() == VCHR;

    let error = 'out: {
        let newoff = match whence {
            SEEK_CUR => fp.f_offset.get().wrapping_add(*offset),
            SEEK_END => {
                // KERNEL_LOCK().
                if let Err(e) = VOP_GETATTR(vp, &mut vattr, cred, p) {
                    break 'out Err(e);
                }
                // KERNEL_UNLOCK().
                offset.wrapping_add(vattr.va_size as Off)
            }
            SEEK_SET => *offset,
            _ => break 'out Err(Errno::EINVAL),
        };
        if !special && newoff < 0 {
            break 'out Err(Errno::EINVAL);
        }
        mtx_enter(&fp.f_mtx);
        fp.f_offset.set(newoff);
        mtx_leave(&fp.f_mtx);
        *offset = newoff;
        Ok(())
    };

    let _ = VOP_UNLOCK(vp);
    error
}

/// Common code for vnode access operations.
///
/// Check if a directory can be found inside another in the hierarchy.
pub fn vn_isunder(lvp: &'static Vnode, rvp: &'static Vnode, p: &Proc) -> bool {
    vfs_getcwd_common(lvp, Some(rvp), None, MAXPATHLEN as i32 / 2, 0, p).is_ok()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::kern::vfs_lookup::ndinit;
    use crate::kern::vfs_subr::tests::testfs::{self, B, any_locked, node_of};
    use crate::kern::vfs_vops::VOP_ISLOCKED;
    use crate::sys::namei::NiDirp;

    #[test]
    fn open_stat_close() {
        let (_g, p, _mp) = testfs::setup_root();
        let mut nd = ndinit(0, 0, NiDirp::Sys(b"/a/b"), p);
        vn_open(&mut nd, FREAD, 0).unwrap();
        let vp = nd.ni_vp.unwrap();
        assert_eq!(node_of(vp), B);
        assert_eq!(VOP_ISLOCKED(vp), 1, "vn_open returns the vnode locked");
        let mut st = Stat::default();
        vn_stat(vp, &mut st, p).unwrap();
        assert_eq!(st.st_mode, S_IFREG | 0o755);
        assert_eq!((st.st_ino, st.st_size, st.st_blocks), (102, 512, 1));
        let _ = VOP_UNLOCK(vp);
        vn_close(vp, FREAD, p.p_ucred.get(), Some(p)).unwrap();
        assert!(!any_locked());

        // A directory cannot be opened for writing; O_DIRECTORY wants one; a symbolic link
        // with O_NOFOLLOW is a loop; O_TRUNC needs write access.
        let mut nd = ndinit(0, 0, NiDirp::Sys(b"/a"), p);
        assert_eq!(vn_open(&mut nd, FWRITE, 0), Err(Errno::EISDIR));
        let mut nd = ndinit(0, 0, NiDirp::Sys(b"/a/b"), p);
        assert_eq!(
            vn_open(&mut nd, FREAD | O_DIRECTORY, 0),
            Err(Errno::ENOTDIR)
        );
        let mut nd = ndinit(0, 0, NiDirp::Sys(b"/l"), p);
        assert_eq!(vn_open(&mut nd, FREAD | O_NOFOLLOW, 0), Err(Errno::ELOOP));
        let mut nd = ndinit(0, 0, NiDirp::Sys(b"/a/b"), p);
        assert_eq!(vn_open(&mut nd, O_TRUNC, 0), Err(Errno::EINVAL));
        assert!(!any_locked());
    }
}
