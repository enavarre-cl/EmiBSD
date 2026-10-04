/*	$OpenBSD: vfs_syscalls.c,v 1.388 2026/08/15 22:07:04 gnezdo Exp $	*/
/*	$NetBSD: vfs_syscalls.c,v 1.71 1996/04/23 10:29:02 mycroft Exp $	*/
/* <LICENSES> */
/*
 * Copyright (c) 1989, 1993
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
 *	@(#)vfs_syscalls.c	8.28 (Berkeley) 12/10/94
 */
/* </LICENSES> */

//! Virtual File System System Calls: mounting (`mount`, `unmount`, `dounmount`, `sync`,
//! `quotactl`, `statfs`, `fstatfs`, `getfsstat`), the current and root directories (`chdir`,
//! `fchdir`, `chroot`, `__realpath`), opening (`open`, `openat`, `__pledge_open`, the file
//! handle calls), the name space (`mknod`, `mkfifo`, `link`, `symlink`, `unlink`, `rename`,
//! `mkdir`, `rmdir`), attributes (`access`, `stat`, `pathconf`, `readlink`, `chflags`,
//! `chmod`, `chown`, `utimes`, `truncate`), `lseek`, `fsync`, `getdents`, `umask`, `revoke`
//! and the positioned reads and writes.
//!
//! Upstream: sys/kern/vfs_syscalls.c @ 3ce1f3f79392
//!
//! Every call is the C's; with no file system mounted yet (stage 2), the path-based ones end
//! in `namei`'s `ENOENT` (no root vnode) and the descriptor-based ones in `getvnode`'s
//! `EINVAL` (no descriptor can be a vnode before something is opened).
//!
//! ## Deviations
//! - `VFS_STATFS(mp, &mp->mnt_stat, p)` works on a copy of `mnt_stat` that is stored back
//!   (`statfs_mnt`); `mnt_stat` is a `Cell` (`sys/mount.rs`).
//! - `getvnode` returns the file instead of filling `*fpp`; `dorenameat`'s `error = -1` ("the
//!   same file: nothing to do") is a flag.
//! - `sys_unveil`'s body after `single_thread_set` is `unveil_path`, so that every exit
//!   reaches `single_thread_clear`; the pathname buffer is a `NameiBuf` given back on drop.
//! - `option FIFO` is not configured (`miscfs/fifofs` is not ported): `mkfifo` answers
//!   `EOPNOTSUPP` as the C does without it.
//! - `KTRACE` is not configured; `KERNEL_LOCK()`/`KERNEL_UNLOCK()` are nothing without
//!   `MULTIPROCESSOR`.

use core::ffi::c_void;
use core::ptr::{self, NonNull};
use core::slice;
use core::sync::atomic::Ordering;

use crate::kern::kern_descrip::{closef, dupfdopen, falloc, fd_getfile, fdinsert, fdremove};
use crate::kern::kern_malloc::{free, malloc};
use crate::kern::kern_pledge::{pledge_chown, pledge_flock};
use crate::kern::kern_proc::ALLPROCESS;
use crate::kern::kern_prot::{crdup, crfree, suser};
use crate::kern::kern_resource::lim_cur_proc;
use crate::kern::kern_sig::{psignal, single_thread_clear, single_thread_set};
use crate::kern::kern_tc::{getnanotime, gettime};
use crate::kern::kern_unveil::{unveil_add, unveil_removevnode};
use crate::kern::subr_pool::{pool_get, pool_put};
use crate::kern::subr_prf::panic;
use crate::kern::sys_generic::{dofilereadv, dofilewritev};
use crate::kern::vfs_cache::{cache_purge, cache_purgevfs};
use crate::kern::vfs_getcwd::vfs_getcwd_common;
use crate::kern::vfs_init::{NAMEI_POOL, rootvnode, set_rootvnode, vfs_byname};
use crate::kern::vfs_lookup::{namei, ndinit, ndinitat};
use crate::kern::vfs_subr::{
    MOUNTLIST, vattr_null, vfs_busy, vfs_getvfs, vfs_mount_alloc, vfs_mount_free, vfs_unbusy,
    vgone, vinvalbuf, vnoperm, vput, vref, vrele,
};
use crate::kern::vfs_sync::vfs_allocate_syncvnode;
use crate::kern::vfs_vnops::{VNOPS, vn_lock, vn_open, vn_stat, vn_writechk};
use crate::kern::vfs_vops::{
    VOP_ABORTOP, VOP_ACCESS, VOP_ADVLOCK, VOP_FSYNC, VOP_GETATTR, VOP_LINK, VOP_MKDIR, VOP_MKNOD,
    VOP_OPEN, VOP_PATHCONF, VOP_READDIR, VOP_READLINK, VOP_REMOVE, VOP_RENAME, VOP_REVOKE,
    VOP_RMDIR, VOP_SETATTR, VOP_SYMLINK, VOP_UNLOCK,
};
use crate::machine::conf::{cdevsw, nchrdev};
use crate::machine::copy::{copyin, copyin_obj, copyinstr, copyout, copyout_obj, copyoutstr};
use crate::sys::conf::D_TTY;
use crate::sys::errno::Errno;
use crate::sys::fcntl::{
    AT_EACCESS, AT_FDCWD, AT_REMOVEDIR, AT_SYMLINK_FOLLOW, AT_SYMLINK_NOFOLLOW, F_FLOCK, F_RDLCK,
    F_SETLK, F_WAIT, F_WRLCK, FMASK, FNONBLOCK, FREAD, FWRITE, Flock, O_ACCMODE, O_CLOEXEC,
    O_CLOFORK, O_CREAT, O_DIRECTORY, O_EXLOCK, O_RDONLY, O_RDWR, O_SHLOCK, O_TRUNC, fflags,
};
use crate::sys::file::{DTYPE_VNODE, FIF_HASLOCK, FO_POSITION, File, frele};
use crate::sys::filedesc::{UF_EXCLOSE, UF_FORKCLOSE, UF_PLEDGEOPEN, fdplock, fdpunlock};
use crate::sys::limits::INT_MAX;
use crate::sys::lock::{LK_EXCLUSIVE, LK_RETRY};
use crate::sys::malloc::{M_TEMP, M_WAITOK, M_ZERO};
use crate::sys::mount::{
    Fhandle, Fsid, MFSNAMELEN, MNAMELEN, MNT_ASYNC, MNT_DOOMED, MNT_FORCE, MNT_LAZY, MNT_NOATIME,
    MNT_NODEV, MNT_NOEXEC, MNT_NOPERM, MNT_NOSUID, MNT_NOWAIT, MNT_OP_FLAGS, MNT_RDONLY,
    MNT_RELOAD, MNT_ROOTFS, MNT_SYNCHRONOUS, MNT_UPDATE, MNT_VISFLAGMASK, MNT_WAIT, MNT_WANTRDWR,
    MNT_WXALLOWED, MntDounmount, MntList, Mount, Statfs, VB_DUPOK, VB_NOWAIT, VB_READ, VB_WAIT,
    VB_WRITE, VFS_FHTOVP, VFS_MOUNT, VFS_QUOTACTL, VFS_ROOT, VFS_START, VFS_STATFS, VFS_SYNC,
    VFS_UNMOUNT, VFS_VPTOFH,
};
use crate::sys::namei::{
    BYPASSUNVEIL, CREATE, DELETE, FOLLOW, LOCKLEAF, LOCKPARENT, LOOKUP, NOCACHE, NOFOLLOW,
    Nameidata, NiDirp, REALPATH, RENAME, SAVENAME, SAVESTART, STRIPSLASHES, UNVEIL_CREATE,
    UNVEIL_PLEDGEOPEN, UNVEIL_READ, UNVEIL_WRITE, WANTPARENT,
};
use crate::sys::param::MAXPATHLEN;
use crate::sys::pledge::{
    PLEDGE_CHOWN, PLEDGE_CPATH, PLEDGE_DPATH, PLEDGE_FATTR, PLEDGE_RPATH, PLEDGE_TTY,
    PLEDGE_UNVEIL, PLEDGE_WPATH,
};
use crate::sys::pool::PR_WAITOK;
use crate::sys::proc::{PS_CHROOT, PS_PLEDGE, Proc, SINGLE_UNWIND};
use crate::sys::queue::{SlistHead, TailqHead};
use crate::sys::resource::RLIMIT_FSIZE;
use crate::sys::signal::SIGXFSZ;
use crate::sys::stat::{
    ACCESSPERMS, ALLPERMS, S_IFBLK, S_IFCHR, S_IFIFO, S_IFMT, S_ISTXT, Stat, UTIME_NOW, UTIME_OMIT,
    s_isfifo,
};
use crate::sys::syscallargs::*;
use crate::sys::systm::{INFSLP, SysArgs, sysargs};
use crate::sys::time::{Timespec, Timeval, timeval_to_timespec};
use crate::sys::types::{Dev, Gid, Mode, Off, Register, Uid, major};
use crate::sys::uio::{Iovec, UIO_SMALLIOV, Uio, UioRw, UioSeg};
use crate::sys::unistd::{R_OK, SEEK_SET, W_OK, X_OK};
use crate::sys::vnode::{
    GETCWD_CHECK_ACCESS, REVOKEALL, V_SAVE, VA_UTIMES_CHANGE, VA_UTIMES_NULL, VALIASED, VBAD, VBLK,
    VCHR, VDIR, VEXEC, VNOVAL, VREAD, VROOT, VSGID, VSUID, VWRITE, Vattr, Vnode,
};
use crate::uvm::uvm_vnode::{uvm_vnp_sync, uvm_vnp_uncache};

/// The user address of a pathname argument.
fn upath(path: *const u8) -> NiDirp<'static> {
    NiDirp::User(path as usize)
}

/// The vnode a successful lookup left in a `nameidata` slot.
fn ndvp(vp: Option<&'static Vnode>) -> &'static Vnode {
    match vp {
        Some(vp) => vp,
        None => panic(format_args!("vfs_syscalls: namei returned no vnode")),
    }
}

/// `VFS_STATFS(mp, &mp->mnt_stat, p)`: refreshes the mount's statistics (see the module's
/// deviations).
fn statfs_mnt(mp: &'static Mount, p: &Proc) -> Result<(), Errno> {
    let mut sp = mp.mnt_stat.get();
    let error = VFS_STATFS(mp, &mut sp, p);
    mp.mnt_stat.set(sp);
    error
}

/// A kernel buffer of `vfc_datasize` bytes for a file system's mount arguments.
struct MountArgs {
    mem: Option<NonNull<u8>>,
    len: usize,
}

impl MountArgs {
    /// `malloc(vfsp->vfc_datasize, M_TEMP, M_WAITOK | M_ZERO)` filled from user space.
    fn copyin(data: usize, len: usize) -> Result<MountArgs, Errno> {
        let mem = if len == 0 {
            None
        } else {
            match malloc(len, M_TEMP, M_WAITOK | M_ZERO) {
                Some(mem) => Some(mem),
                None => return Err(Errno::ENOMEM),
            }
        };
        let args = MountArgs { mem, len };
        if let Err(e) = copyin(data, args.as_mut()) {
            args.free();
            return Err(e);
        }
        Ok(args)
    }

    /// The arguments' bytes.
    fn as_mut<'b>(&self) -> &'b mut [u8] {
        match self.mem {
            // SAFETY: a `len`-byte allocation owned by this `MountArgs` until `free`.
            Some(mem) => unsafe { slice::from_raw_parts_mut(mem.as_ptr(), self.len) },
            None => &mut [],
        }
    }

    /// `free(args, M_TEMP, vfsp->vfc_datasize)`.
    fn free(self) {
        if let Some(mem) = self.mem {
            free(mem, M_TEMP, self.len);
        }
    }
}

/// Mount a file system.
pub fn sys_mount(p: &Proc, v: &SysArgs, _retval: &mut [Register; 2]) -> Result<(), Errno> {
    let uap: &SysMountArgs = sysargs(v);
    let flags = uap.flags.get();
    let mut mntflag = 0;

    suser(p)?;

    // Mount points must fit in MNAMELEN, not MAXPATHLEN.
    let mut fspath = [0u8; MNAMELEN];
    copyinstr(uap.path.get() as usize, &mut fspath)?;

    // Get vnode to be covered
    let mut nd = ndinit(LOOKUP, FOLLOW | LOCKLEAF, NiDirp::Sys(&fspath), p);
    namei(&mut nd)?;
    let vp = ndvp(nd.ni_vp);
    let mp: &'static Mount;
    let args: MountArgs;
    if flags & MNT_UPDATE != 0 {
        if vp.v_flag.get() & VROOT == 0 {
            vput(vp);
            return Err(Errno::EINVAL);
        }
        let Some(vmp) = vp.v_mount.get() else {
            vput(vp);
            return Err(Errno::EINVAL);
        };
        mp = vmp;
        let vfsp = mp.vfc();

        args = match MountArgs::copyin(uap.data.get() as usize, vfsp.vfc_datasize) {
            Ok(args) => args,
            Err(e) => {
                vput(vp);
                return Err(e);
            }
        };

        mntflag = mp.mnt_flag.get();
        // We only allow the filesystem to be reloaded if it is currently mounted read-only.
        if flags & MNT_RELOAD != 0 && mp.mnt_flag.get() & MNT_RDONLY == 0 {
            vput(vp);
            args.free();
            return Err(Errno::EOPNOTSUPP); // Needs translation
        }

        if let Err(e) = vfs_busy(mp, VB_READ | VB_NOWAIT) {
            vput(vp);
            args.free();
            return Err(e);
        }
        mp.mnt_flag
            .set(mp.mnt_flag.get() | (flags & (MNT_RELOAD | MNT_UPDATE)));
    } else {
        // Do not allow disabling of permission checks unless exec and access to device files
        // is disabled too.
        if flags & MNT_NOPERM != 0 && flags & (MNT_NODEV | MNT_NOEXEC) != (MNT_NODEV | MNT_NOEXEC) {
            vput(vp);
            return Err(Errno::EPERM);
        }
        if let Err(e) = vinvalbuf(vp, V_SAVE, p.p_ucred.get(), Some(p), 0, INFSLP) {
            vput(vp);
            return Err(e);
        }
        if vp.v_type.get() != VDIR {
            vput(vp);
            return Err(Errno::ENOTDIR);
        }
        let mut fstypename = [0u8; MFSNAMELEN];
        if let Err(e) = copyinstr(uap.r#type.get() as usize, &mut fstypename) {
            vput(vp);
            return Err(e);
        }
        let Some(vfsp) = vfs_byname(&fstypename) else {
            vput(vp);
            return Err(Errno::EOPNOTSUPP);
        };

        args = match MountArgs::copyin(uap.data.get() as usize, vfsp.vfc_datasize) {
            Ok(args) => args,
            Err(e) => {
                vput(vp);
                return Err(e);
            }
        };

        if vp.v_mountedhere().is_some() {
            vput(vp);
            args.free();
            return Err(Errno::EBUSY);
        }

        // Allocate and initialize the file system.
        mp = vfs_mount_alloc(Some(vp), vfsp);
        mp.update_stat(|sp| sp.f_owner = p.ucred().cr_uid.get());
    }

    // update:
    let Some(vmount) = vp.v_mount.get() else {
        panic(format_args!("sys_mount: covered vnode has no mount"));
    };
    // Ensure that the parent mountpoint does not get unmounted.
    if let Err(e) = vfs_busy(vmount, VB_READ | VB_NOWAIT | VB_DUPOK) {
        if mp.mnt_flag.get() & MNT_UPDATE != 0 {
            mp.mnt_flag.set(mntflag);
            vfs_unbusy(mp);
        } else {
            vfs_unbusy(mp);
            vfs_mount_free(mp);
        }
        vput(vp);
        args.free();
        return Err(e);
    }

    // Set the mount level flags.
    if flags & MNT_RDONLY != 0 {
        mp.mnt_flag.set(mp.mnt_flag.get() | MNT_RDONLY);
    } else if mp.mnt_flag.get() & MNT_RDONLY != 0 {
        mp.mnt_flag.set(mp.mnt_flag.get() | MNT_WANTRDWR);
    }
    let settable = MNT_NOSUID
        | MNT_NOEXEC
        | MNT_WXALLOWED
        | MNT_NODEV
        | MNT_SYNCHRONOUS
        | MNT_ASYNC
        | MNT_NOATIME
        | MNT_NOPERM
        | MNT_FORCE;
    mp.mnt_flag
        .set((mp.mnt_flag.get() & !settable) | (flags & settable));
    // Mount the filesystem.
    let path_len = fspath.iter().position(|&c| c == 0).unwrap_or(MNAMELEN);
    let mut error = VFS_MOUNT(mp, &fspath[..path_len], args.as_mut(), &mut nd, p);
    if error.is_ok() {
        mp.update_stat(|sp| sp.f_ctime = gettime() as u64);
    }
    if mp.mnt_flag.get() & MNT_UPDATE != 0 {
        vfs_unbusy(vmount);
        vput(vp);
        if mp.mnt_flag.get() & MNT_WANTRDWR != 0 {
            mp.mnt_flag.set(mp.mnt_flag.get() & !MNT_RDONLY);
        }
        mp.mnt_flag.set(mp.mnt_flag.get() & !MNT_OP_FLAGS);
        if error.is_err() {
            mp.mnt_flag.set(mntflag);
        }

        if mp.mnt_flag.get() & MNT_RDONLY == 0 {
            if mp.mnt_syncer.get().is_none() {
                error = vfs_allocate_syncvnode(mp);
            }
        } else {
            if let Some(syncer) = mp.mnt_syncer.get() {
                vgone(syncer);
            }
            mp.mnt_syncer.set(None);
        }

        vfs_unbusy(mp);
        args.free();
        return error;
    }

    mp.mnt_flag.set(mp.mnt_flag.get() & !MNT_OP_FLAGS);
    vp.set_v_mountedhere(Some(mp));

    // Put the new filesystem on the mount list after root.
    cache_purge(vp);
    if error.is_ok() {
        // SAFETY: a new mount on no list; mounts never move.
        unsafe { MOUNTLIST.0.insert_tail(mp) };
        checkdirs(vp);
        vfs_unbusy(vmount);
        let _ = VOP_UNLOCK(vp);
        if mp.mnt_flag.get() & MNT_RDONLY == 0 {
            // The C overwrites this error with VFS_START's below.
            let _ = vfs_allocate_syncvnode(mp);
        }
        vfs_unbusy(mp);
        let _ = statfs_mnt(mp, p);
        error = VFS_START(mp, 0, p);
        if error.is_err() {
            vrele(vp);
        }
    } else {
        if let Some(covered) = mp.mnt_vnodecovered.get() {
            covered.set_v_mountedhere(None);
        }
        vfs_unbusy(mp);
        vfs_mount_free(mp);
        vfs_unbusy(vmount);
        vput(vp);
    }
    args.free();
    error
}

/// Scan all active processes to see if any of them have a current or root directory onto
/// which the new filesystem has just been mounted. If so, replace them with the new mount
/// point, keeping track of how many were replaced. That's the number of references the old
/// vnode had that we've replaced, so finish by vrele()'ing it that many times. This puts off
/// any possible sleeping until we've finished walking the allprocess list.
pub fn checkdirs(olddp: &'static Vnode) {
    let mut free_count = 0u32;

    if olddp.v_usecount.get() == 1 {
        return;
    }
    let Some(newdp) = olddp.v_mountedhere().and_then(|mp| VFS_ROOT(mp).ok()) else {
        panic(format_args!("mount: lost mount"));
    };
    for pr in ALLPROCESS.0.iter() {
        let fdp = pr.fd();
        if fdp.fd_cdir.get().is_some_and(|d| ptr::eq(d, olddp)) {
            free_count += 1;
            vref(newdp);
            fdp.fd_cdir.set(Some(newdp));
        }
        if fdp.fd_rdir.get().is_some_and(|d| ptr::eq(d, olddp)) {
            free_count += 1;
            vref(newdp);
            fdp.fd_rdir.set(Some(newdp));
        }
    }
    if rootvnode().is_some_and(|r| ptr::eq(r, olddp)) {
        free_count += 1;
        vref(newdp);
        set_rootvnode(Some(newdp));
    }
    while free_count > 0 {
        free_count -= 1;
        vrele(olddp);
    }
    vput(newdp);
}

/// Unmount a file system.
///
/// Note: unmount takes a path to the vnode mounted on as argument, not special file (as
/// before).
pub fn sys_unmount(p: &Proc, v: &SysArgs, _retval: &mut [Register; 2]) -> Result<(), Errno> {
    let uap: &SysUnmountArgs = sysargs(v);

    suser(p)?;

    let mut nd = ndinit(LOOKUP, FOLLOW | LOCKLEAF, upath(uap.path.get()), p);
    namei(&mut nd)?;
    let vp = ndvp(nd.ni_vp);
    let Some(mp) = vp.v_mount.get() else {
        vput(vp);
        return Err(Errno::EINVAL);
    };

    // Don't allow unmounting the root file system.
    if mp.mnt_flag.get() & MNT_ROOTFS != 0 {
        vput(vp);
        return Err(Errno::EINVAL);
    }

    // Must be the root of the filesystem
    if vp.v_flag.get() & VROOT == 0 {
        vput(vp);
        return Err(Errno::EINVAL);
    }
    vput(vp);

    if vfs_busy(mp, VB_WRITE | VB_WAIT).is_err() {
        return Err(Errno::EBUSY);
    }

    dounmount(mp, uap.flags.get() & MNT_FORCE, p)
}

/// A mount on `dounmount`'s work queue, which only holds `&'static Mount`s.
fn queued(m: &Mount) -> &'static Mount {
    // SAFETY: the queue only ever holds the busied, mounted file systems inserted above,
    // which are `malloc`ed and stay allocated while mounted (`&'static Mount`, as everywhere).
    unsafe { &*ptr::from_ref(m) }
}

/// Do the actual file system unmount.
pub fn dounmount(mp: &'static Mount, flags: i32, p: &Proc) -> Result<(), Errno> {
    let mplist: SlistHead<MntDounmount> = SlistHead::new();

    // SAFETY: the mount is busied by the caller and on no unmount queue; mounts never move.
    unsafe { mplist.insert_head(mp) };

    // Collect nested mount points. This takes advantage of the mount list being ordered -
    // nested mount points come after their parent.
    let error = 'err: {
        let mut cur = mp;
        while let Some(next) = TailqHead::<MntList>::next(cur) {
            cur = next;
            for nmp in mplist.iter() {
                if !cur
                    .mnt_vnodecovered
                    .get()
                    .and_then(|vp| vp.v_mount.get())
                    .is_some_and(|m| ptr::eq(m, nmp))
                {
                    continue;
                }

                if flags & MNT_FORCE == 0 {
                    break 'err Errno::EBUSY;
                }
                if let Err(error) = vfs_busy(cur, VB_WRITE | VB_WAIT | VB_DUPOK) {
                    if flags & MNT_DOOMED != 0 {
                        // If the mount point was busy due to being unmounted, it has been
                        // removed from the mount list already. Restart the iteration from
                        // the last collected busy entry.
                        if let Some(first) = mplist.first() {
                            cur = queued(first);
                        }
                        break;
                    }
                    break 'err error;
                }
                // SAFETY: a busied mount on no unmount queue.
                unsafe { mplist.insert_head(cur) };
                break;
            }
        }

        // Nested mount points cannot appear during this loop as mounting requires a read lock
        // for the parent mount point.
        while let Some(m) = mplist.first() {
            let m = queued(m);
            // SAFETY: `m` is the queue's first element.
            unsafe { mplist.remove_head() };
            if let Err(error) = dounmount_leaf(m, flags, p) {
                break 'err error;
            }
        }
        return Ok(());
    };

    while let Some(m) = mplist.first() {
        let m = queued(m);
        // SAFETY: `m` is the queue's first element.
        unsafe { mplist.remove_head() };
        vfs_unbusy(m);
    }
    Err(error)
}

/// `dounmount_leaf(mp, flags, p)`: unmounts one file system with nothing mounted below it.
pub fn dounmount_leaf(mp: &'static Mount, flags: i32, p: &Proc) -> Result<(), Errno> {
    let mut hadsyncer = false;

    mp.mnt_flag.set(mp.mnt_flag.get() & !MNT_ASYNC);
    cache_purgevfs(mp); // remove cache entries for this file sys
    if let Some(syncer) = mp.mnt_syncer.get() {
        hadsyncer = true;
        vgone(syncer);
        mp.mnt_syncer.set(None);
    }

    // Before calling file system unmount, make sure all unveils to vnodes in here are
    // dropped.
    for vp in mp.mnt_vnodelist.iter() {
        unveil_removevnode(vp);
    }

    let mut error = if mp.mnt_flag.get() & MNT_RDONLY != 0 {
        Ok(())
    } else {
        VFS_SYNC(mp, MNT_WAIT, 0, p.p_ucred.get(), p)
    };
    if error.is_ok() || flags & MNT_FORCE != 0 {
        error = VFS_UNMOUNT(mp, flags, p);
    }

    if error.is_err() && flags & MNT_DOOMED == 0 {
        if mp.mnt_flag.get() & MNT_RDONLY == 0 && hadsyncer {
            let _ = vfs_allocate_syncvnode(mp);
        }
        vfs_unbusy(mp);
        return error;
    }

    // SAFETY: a mounted file system is on the mount list.
    unsafe { MOUNTLIST.0.remove(mp) };
    if let Some(coveredvp) = mp.mnt_vnodecovered.get() {
        coveredvp.set_v_mountedhere(None);
        vrele(coveredvp);
    }

    if !mp.mnt_vnodelist.is_empty() {
        panic(format_args!("unmount: dangling vnode"));
    }

    vfs_unbusy(mp);
    vfs_mount_free(mp);

    Ok(())
}

/// Sync each mounted filesystem.
pub fn sys_sync(p: &Proc, _v: &SysArgs, _retval: &mut [Register; 2]) -> Result<(), Errno> {
    for mp in MOUNTLIST.0.iter_reverse() {
        if vfs_busy(mp, VB_READ | VB_NOWAIT).is_err() {
            continue;
        }
        if mp.mnt_flag.get() & MNT_RDONLY == 0 {
            let asyncflag = mp.mnt_flag.get() & MNT_ASYNC;
            mp.mnt_flag.set(mp.mnt_flag.get() & !MNT_ASYNC);
            uvm_vnp_sync(Some(mp));
            let _ = VFS_SYNC(mp, MNT_NOWAIT, 0, p.p_ucred.get(), p);
            if asyncflag != 0 {
                mp.mnt_flag.set(mp.mnt_flag.get() | MNT_ASYNC);
            }
        }
        vfs_unbusy(mp);
    }

    Ok(())
}

/// Change filesystem quotas.
pub fn sys_quotactl(p: &Proc, v: &SysArgs, _retval: &mut [Register; 2]) -> Result<(), Errno> {
    let uap: &SysQuotactlArgs = sysargs(v);

    let mut nd = ndinit(LOOKUP, FOLLOW, upath(uap.path.get()), p);
    nd.ni_unveil = UNVEIL_READ | UNVEIL_WRITE;
    namei(&mut nd)?;
    let vp = ndvp(nd.ni_vp);
    let mp = vp.v_mount.get();
    vrele(vp);
    let Some(mp) = mp else {
        return Err(Errno::ENOENT);
    };
    VFS_QUOTACTL(
        mp,
        uap.cmd.get(),
        uap.uid.get() as Uid,
        uap.arg.get() as usize,
        p,
    )
}

/// The bytes of a `struct statfs`.
fn statfs_bytes(sp: &Statfs) -> &[u8] {
    // SAFETY: `Statfs` is `AbiPod` (no padding), so all of its bytes are initialised and may be
    // read as `u8`s while `sp` is borrowed.
    unsafe { slice::from_raw_parts(ptr::from_ref(sp).cast::<u8>(), size_of::<Statfs>()) }
}

/// `copyout_statfs(sp, uaddr, p)`: copies the statistics out, hiding the filesystem id from
/// non-root (for NFS security).
pub fn copyout_statfs(sp: &Statfs, uaddr: usize, p: &Proc) -> Result<(), Errno> {
    let co_sz1 = core::mem::offset_of!(Statfs, f_fsid);
    let co_off2 = co_sz1 + size_of::<Fsid>();
    let s = statfs_bytes(sp);

    // Don't let non-root see filesystem id (for NFS security)
    if suser(p).is_err() {
        let fsid = [0u8; size_of::<Fsid>()];

        copyout(&s[..co_sz1], uaddr)?;
        copyout(&fsid, uaddr + co_sz1)?;
        return copyout(&s[co_off2..], uaddr + co_off2);
    }

    copyout(s, uaddr)
}

/// Get filesystem statistics.
pub fn sys_statfs(p: &Proc, v: &SysArgs, _retval: &mut [Register; 2]) -> Result<(), Errno> {
    let uap: &SysStatfsArgs = sysargs(v);

    let mut nd = ndinit(LOOKUP, FOLLOW | BYPASSUNVEIL, upath(uap.path.get()), p);
    nd.ni_pledge = PLEDGE_RPATH;
    nd.ni_unveil = UNVEIL_READ;
    namei(&mut nd)?;
    let vp = ndvp(nd.ni_vp);
    let mp = vp.v_mount.get();
    vrele(vp);
    let Some(mp) = mp else {
        return Err(Errno::ENOENT);
    };
    statfs_mnt(mp, p)?;
    mp.update_stat(|sp| sp.f_flags = (mp.mnt_flag.get() & MNT_VISFLAGMASK) as u32);

    copyout_statfs(&mp.mnt_stat.get(), uap.buf.get() as usize, p)
}

/// Get filesystem statistics.
pub fn sys_fstatfs(p: &Proc, v: &SysArgs, _retval: &mut [Register; 2]) -> Result<(), Errno> {
    let uap: &SysFstatfsArgs = sysargs(v);

    let fp = getvnode(p, uap.fd.get())?;
    let Some(mp) = fp.vnode().v_mount.get() else {
        let _ = frele(fp, p);
        return Err(Errno::ENOENT);
    };
    let error = statfs_mnt(mp, p);
    let _ = frele(fp, p);
    error?;
    mp.update_stat(|sp| sp.f_flags = (mp.mnt_flag.get() & MNT_VISFLAGMASK) as u32);

    copyout_statfs(&mp.mnt_stat.get(), uap.buf.get() as usize, p)
}

/// Get statistics on all filesystems.
pub fn sys_getfsstat(p: &Proc, v: &SysArgs, retval: &mut [Register; 2]) -> Result<(), Errno> {
    let uap: &SysGetfsstatArgs = sysargs(v);
    let flags = uap.flags.get();

    let maxcount = uap.bufsize.get() / size_of::<Statfs>();
    let mut sfsp = uap.buf.get() as usize;
    let mut count = 0usize;

    for mp in MOUNTLIST.0.iter() {
        if vfs_busy(mp, VB_READ | VB_NOWAIT).is_err() {
            continue;
        }
        if sfsp != 0 && count < maxcount {
            // Refresh stats unless MNT_NOWAIT is specified
            if flags != MNT_NOWAIT
                && flags != MNT_LAZY
                && (flags == MNT_WAIT || flags == 0)
                && statfs_mnt(mp, p).is_err()
            {
                vfs_unbusy(mp);
                continue;
            }

            mp.update_stat(|sp| sp.f_flags = (mp.mnt_flag.get() & MNT_VISFLAGMASK) as u32);
            if let Err(error) = copyout_statfs(&mp.mnt_stat.get(), sfsp, p) {
                vfs_unbusy(mp);
                return Err(error);
            }
            sfsp += size_of::<Statfs>();
        }
        count += 1;
        vfs_unbusy(mp);
    }

    retval[0] = if sfsp != 0 && count > maxcount {
        maxcount
    } else {
        count
    } as Register;

    Ok(())
}

/// Change current working directory to a given file descriptor.
pub fn sys_fchdir(p: &Proc, v: &SysArgs, _retval: &mut [Register; 2]) -> Result<(), Errno> {
    let uap: &SysFchdirArgs = sysargs(v);
    let fdp = p.fd();

    let Some(fp) = fd_getfile(fdp, uap.fd.get()) else {
        return Err(Errno::EBADF);
    };
    if fp.f_type.get() != DTYPE_VNODE || fp.vnode().v_type.get() != VDIR {
        let _ = frele(fp, p);
        return Err(Errno::ENOTDIR);
    }
    let mut vp = fp.vnode();
    vref(vp);
    let _ = frele(fp, p);
    let _ = vn_lock(vp, LK_EXCLUSIVE | LK_RETRY);
    let mut error = VOP_ACCESS(vp, VEXEC, p.p_ucred.get(), p);

    while error.is_ok()
        && let Some(mp) = vp.v_mountedhere()
    {
        if vfs_busy(mp, VB_READ | VB_WAIT).is_err() {
            continue;
        }
        let root = VFS_ROOT(mp);
        vfs_unbusy(mp);
        match root {
            Err(e) => {
                error = Err(e);
                break;
            }
            Ok(tdp) => {
                vput(vp);
                vp = tdp;
            }
        }
    }
    if let Err(e) = error {
        vput(vp);
        return Err(e);
    }
    let _ = VOP_UNLOCK(vp);
    let old_cdir = fdp.fd_cdir.get();
    fdp.fd_cdir.set(Some(vp));
    if let Some(old) = old_cdir {
        vrele(old);
    }
    Ok(())
}

/// Change current working directory (``.'').
pub fn sys_chdir(p: &Proc, v: &SysArgs, _retval: &mut [Register; 2]) -> Result<(), Errno> {
    let uap: &SysChdirArgs = sysargs(v);
    let fdp = p.fd();

    let mut nd = ndinit(LOOKUP, FOLLOW | LOCKLEAF, upath(uap.path.get()), p);
    nd.ni_pledge = PLEDGE_RPATH;
    nd.ni_unveil = UNVEIL_READ;
    change_dir(&mut nd, p)?;
    let old_cdir = fdp.fd_cdir.get();
    fdp.fd_cdir.set(nd.ni_vp);
    if let Some(old) = old_cdir {
        vrele(old);
    }
    Ok(())
}

/// Change notion of root (``/'') directory.
pub fn sys_chroot(p: &Proc, v: &SysArgs, _retval: &mut [Register; 2]) -> Result<(), Errno> {
    let uap: &SysChrootArgs = sysargs(v);
    let fdp = p.fd();

    suser(p)?;
    let mut nd = ndinit(LOOKUP, FOLLOW | LOCKLEAF, upath(uap.path.get()), p);
    change_dir(&mut nd, p)?;
    let vp = ndvp(nd.ni_vp);
    if let Some(old_rdir) = fdp.fd_rdir.get() {
        // A chroot() done inside a changed root environment does an automatic chdir to avoid
        // the out-of-tree experience.
        vref(vp);
        let old_cdir = fdp.fd_cdir.get();
        fdp.fd_rdir.set(Some(vp));
        fdp.fd_cdir.set(Some(vp));
        vrele(old_rdir);
        if let Some(old_cdir) = old_cdir {
            vrele(old_cdir);
        }
    } else {
        fdp.fd_rdir.set(Some(vp));
    }
    p.process().ps_flags.fetch_or(PS_CHROOT, Ordering::SeqCst);
    Ok(())
}

/// Common routine for chroot and chdir.
fn change_dir(ndp: &mut Nameidata<'_>, p: &Proc) -> Result<(), Errno> {
    namei(ndp)?;
    let vp = ndvp(ndp.ni_vp);
    let error = if vp.v_type.get() != VDIR {
        Err(Errno::ENOTDIR)
    } else {
        VOP_ACCESS(vp, VEXEC, p.p_ucred.get(), p)
    };
    if error.is_err() {
        vput(vp);
    } else {
        let _ = VOP_UNLOCK(vp);
    }
    error
}

/// A `MAXPATHLEN`-byte `namei_pool` buffer, given back when dropped.
pub struct NameiBuf(NonNull<u8>);

impl NameiBuf {
    /// `pool_get(&namei_pool, PR_WAITOK)`.
    pub fn get() -> Result<NameiBuf, Errno> {
        pool_get(&NAMEI_POOL, PR_WAITOK)
            .map(NameiBuf)
            .ok_or(Errno::ENOMEM)
    }

    /// `pool_get(&namei_pool, PR_WAITOK | PR_ZERO)`.
    pub fn get_zero() -> Result<NameiBuf, Errno> {
        let buf = Self::get()?;
        buf.as_mut().fill(0);
        Ok(buf)
    }

    /// The buffer's address (`cn_rpbuf`).
    pub fn as_ptr(&self) -> *mut u8 {
        self.0.as_ptr()
    }

    /// The bytes up to the first NUL.
    pub fn as_str<'b>(&self) -> &'b [u8] {
        let all = self.as_mut();
        let len = all.iter().position(|&c| c == 0).unwrap_or(all.len());
        &all[..len]
    }

    /// The buffer's bytes.
    pub fn as_mut<'b>(&self) -> &'b mut [u8] {
        // SAFETY: a `MAXPATHLEN`-byte pool item owned by this `NameiBuf` until it drops.
        unsafe { slice::from_raw_parts_mut(self.0.as_ptr(), MAXPATHLEN) }
    }
}

impl Drop for NameiBuf {
    fn drop(&mut self) {
        pool_put(&NAMEI_POOL, self.0);
    }
}

/// `__realpath(pathname, resolved)`: the canonical absolute name of a path.
#[allow(non_snake_case)] // the C name: sys___realpath
pub fn sys___realpath(p: &Proc, v: &SysArgs, _retval: &mut [Register; 2]) -> Result<(), Errno> {
    let uap: &SysRealpathArgs = sysargs(v);

    if uap.pathname.get().is_null() {
        return Err(Errno::EINVAL);
    }

    let pathname = NameiBuf::get()?;
    let rpbuf = NameiBuf::get()?;

    let pathlen = copyinstr(uap.pathname.get() as usize, pathname.as_mut())?;

    if pathlen == 1 {
        // empty string ""
        return Err(Errno::ENOENT);
    }
    if pathlen < 2 {
        return Err(Errno::EINVAL);
    }

    // Get cwd for relative path if needed, prepend to rpbuf
    let rp = rpbuf.as_mut();
    rp[0] = 0;
    if pathname.as_mut()[0] != b'/' {
        let cwdlen = MAXPATHLEN * 4; // for vfs_getcwd_common
        let Some(mem) = malloc(cwdlen, M_TEMP, M_WAITOK) else {
            return Err(Errno::ENOMEM);
        };
        // SAFETY: a fresh `cwdlen`-byte allocation, freed below; every byte is written before
        // it is read (the path is built backwards from the NUL).
        let cwdbuf = unsafe { slice::from_raw_parts_mut(mem.as_ptr(), cwdlen) };

        // vfs_getcwd_common fills this in backwards
        let mut bp = cwdlen - 1;
        cwdbuf[bp] = 0;

        // KERNEL_LOCK(): nothing without MULTIPROCESSOR.
        let error = match p.fd().fd_cdir.get() {
            None => Err(Errno::ENOENT), // no root file system yet
            Some(cdir) => vfs_getcwd_common(
                cdir,
                None,
                Some((&mut *cwdbuf, &mut bp)),
                (cwdlen / 2) as i32,
                GETCWD_CHECK_ACCESS,
                p,
            ),
        };

        let error = error.and_then(|()| {
            let cwd = &cwdbuf[bp..];
            let len = cwd.iter().position(|&c| c == 0).unwrap_or(cwd.len());
            if len >= MAXPATHLEN {
                return Err(Errno::ENAMETOOLONG);
            }
            rp[..len].copy_from_slice(&cwd[..len]);
            rp[len] = 0;
            Ok(())
        });
        free(mem, M_TEMP, cwdlen);
        error?;
    }

    let mut nd = ndinit(
        LOOKUP,
        FOLLOW | SAVENAME | REALPATH,
        NiDirp::Sys(pathname.as_mut()),
        p,
    );

    nd.ni_cnd.cn_rpbuf = rpbuf.0.as_ptr();
    nd.ni_cnd.cn_rpi = rp.iter().position(|&c| c == 0).unwrap_or(MAXPATHLEN);

    nd.ni_pledge = PLEDGE_RPATH;
    nd.ni_unveil = UNVEIL_READ;
    // KERNEL_LOCK(): nothing without MULTIPROCESSOR.
    namei(&mut nd)?;

    // release reference from namei
    if let Some(vp) = nd.ni_vp {
        vrele(vp);
    }

    let error = copyoutstr(rp, uap.resolved.get() as usize).map(|_| ());

    // KTRACE: not configured.
    if let Some(buf) = NonNull::new(nd.ni_cnd.cn_pnbuf) {
        pool_put(&NAMEI_POOL, buf);
    }
    error
}

/// `unveil(path, permissions)`: unveils a path to the process (`kern_unveil.c` keeps the
/// table); both NULL forbids any further `unveil`.
pub fn sys_unveil(p: &Proc, v: &SysArgs, _retval: &mut [Register; 2]) -> Result<(), Errno> {
    let uap: &SysUnveilArgs = sysargs(v);
    let pr = p.process();

    if uap.path.get().is_null() && uap.permissions.get().is_null() {
        pr.ps_uvdone.set(1);
        return Ok(());
    }

    if pr.ps_uvdone.get() != 0 {
        return Err(Errno::EPERM);
    }

    let mut permissions = [0u8; 5];
    copyinstr(uap.permissions.get() as usize, &mut permissions)?;

    // System calls in other threads may sleep between unveil datastructure inspections --
    // this is the simplest way to provide consistency
    let _ = single_thread_set(p, SINGLE_UNWIND);

    let error = unveil_path(p, uap.path.get() as usize, &permissions);

    single_thread_clear(p);
    error
}

/// The body of `sys_unveil` between `single_thread_set` and `single_thread_clear` (the C's
/// jumps to `end` are the returns).
fn unveil_path(p: &Proc, path: usize, permissions: &[u8]) -> Result<(), Errno> {
    let pathname = NameiBuf::get()?;
    let pathlen = copyinstr(path, pathname.as_mut())?;

    // KTRACE: not configured.
    if pathlen < 2 {
        return Err(Errno::EINVAL);
    }

    // find root "/" or "//"
    let mut nd = if pathname.as_str().iter().all(|&c| c == b'/') {
        // root directory
        ndinit(
            LOOKUP,
            FOLLOW | LOCKLEAF | SAVENAME,
            NiDirp::Sys(pathname.as_mut()),
            p,
        )
    } else {
        ndinit(
            CREATE,
            FOLLOW | LOCKLEAF | LOCKPARENT | SAVENAME,
            NiDirp::Sys(pathname.as_mut()),
            p,
        )
    };

    nd.ni_pledge = PLEDGE_UNVEIL;
    namei(&mut nd)?;

    // XXX Any access to the file or directory will allow us to pledge path it
    let cred = p.p_ucred.get();
    let any_access = |vp: &'static Vnode| {
        [VREAD, VWRITE, VEXEC]
            .into_iter()
            .any(|mode| VOP_ACCESS(vp, mode, cred, p).is_ok())
    };
    let allow = nd.ni_vp.is_some_and(any_access) || nd.ni_dvp.is_some_and(any_access);

    // release lock from namei, but keep ref
    if let Some(vp) = nd.ni_vp {
        let _ = VOP_UNLOCK(vp);
    }
    if let Some(dvp) = nd.ni_dvp
        && !nd.ni_vp.is_some_and(|vp| ptr::eq(vp, dvp))
    {
        let _ = VOP_UNLOCK(dvp);
    }

    let error = if allow {
        unveil_add(p, &nd, permissions)
    } else {
        Err(Errno::EPERM)
    };

    // release vref from namei, but not vref from unveil_add
    if let Some(vp) = nd.ni_vp {
        vrele(vp);
    }
    if let Some(dvp) = nd.ni_dvp {
        vrele(dvp);
    }

    if let Some(buf) = NonNull::new(nd.ni_cnd.cn_pnbuf) {
        pool_put(&NAMEI_POOL, buf);
    }
    error
}

/// Check permissions, allocate an open file structure, and call the device open routine if
/// any.
pub fn sys_open(p: &Proc, v: &SysArgs, retval: &mut [Register; 2]) -> Result<(), Errno> {
    let uap: &SysOpenArgs = sysargs(v);

    doopenat(
        p,
        AT_FDCWD,
        uap.path.get(),
        uap.flags.get(),
        uap.mode.get(),
        0,
        retval,
    )
}

/// Check permissions, allocate an open file structure, and call the device open routine if
/// any.
#[allow(non_snake_case)] // the C name: sys___pledge_open
pub fn sys___pledge_open(p: &Proc, v: &SysArgs, retval: &mut [Register; 2]) -> Result<(), Errno> {
    let uap: &SysPledgeOpenArgs = sysargs(v);
    let rw = uap.flags.get() & O_ACCMODE;

    // libc only calls with O_RDONLY, O_RDWR, O_CLOEXEC, O_CLOFORK
    if uap.flags.get() & !(O_ACCMODE | O_CLOEXEC | O_CLOFORK) != 0
        || !(rw == O_RDONLY || rw == O_RDWR)
    {
        return Err(Errno::EINVAL);
    }

    doopenat(
        p,
        AT_FDCWD,
        uap.path.get(),
        uap.flags.get(),
        uap.mode.get(),
        UNVEIL_PLEDGEOPEN,
        retval,
    )
}

/// `openat(fd, path, flags, mode)`.
pub fn sys_openat(p: &Proc, v: &SysArgs, retval: &mut [Register; 2]) -> Result<(), Errno> {
    let uap: &SysOpenatArgs = sysargs(v);

    doopenat(
        p,
        uap.fd.get(),
        uap.path.get(),
        uap.flags.get(),
        uap.mode.get(),
        0,
        retval,
    )
}

/// A `struct flock` that locks the whole file (`flock(2)` semantics) for `O_EXLOCK` or
/// `O_SHLOCK`, and the `VOP_ADVLOCK` flags to go with it.
fn open_flock(flags: i32) -> (Flock, i32) {
    let lf = Flock {
        l_whence: SEEK_SET as i16,
        l_start: 0,
        l_len: 0,
        l_type: if flags & O_EXLOCK != 0 {
            F_WRLCK
        } else {
            F_RDLCK
        },
        ..Flock::default()
    };
    let mut type_ = F_FLOCK;
    if flags & FNONBLOCK == 0 {
        type_ |= F_WAIT;
    }
    (lf, type_)
}

/// `doopenat(p, fd, path, oflags, mode, pledgeopen, retval)`: the common body of `open`,
/// `openat` and `__pledge_open`.
pub fn doopenat(
    p: &Proc,
    fd: i32,
    path: *const u8,
    oflags: i32,
    mode: Mode,
    pledgeopen: u8,
    retval: &mut [Register; 2],
) -> Result<(), Errno> {
    let fdp = p.fd();
    let mut ni_pledge = 0;
    let mut ni_unveil = pledgeopen;
    let mut localtrunc = false;

    if oflags & (O_EXLOCK | O_SHLOCK) != 0 {
        pledge_flock(p)?;
    }

    let mut fdflags = if oflags & O_CLOEXEC != 0 {
        UF_EXCLOSE
    } else {
        0
    } | if oflags & O_CLOFORK != 0 {
        UF_FORKCLOSE
    } else {
        0
    };

    fdplock(fdp);
    let (fp, indx) = match falloc(p) {
        Ok(r) => r,
        Err(e) => {
            fdpunlock(fdp);
            return Err(e);
        }
    };
    fdpunlock(fdp);

    let mut flags = fflags(oflags);
    if flags & FREAD != 0 {
        ni_pledge |= PLEDGE_RPATH;
        ni_unveil |= UNVEIL_READ;
    }
    if flags & FWRITE != 0 {
        ni_pledge |= PLEDGE_WPATH;
        ni_unveil |= UNVEIL_WRITE;
    }
    if oflags & O_CREAT != 0 {
        ni_pledge |= PLEDGE_CPATH;
        ni_unveil |= UNVEIL_CREATE;
    }

    let mut cmode = ((mode & !fdp.fd_cmask.get()) & ALLPERMS) & !S_ISTXT;
    if p.process().ps_flags.load(Ordering::Relaxed) & PS_PLEDGE != 0 {
        cmode &= ACCESSPERMS;
    }
    let mut nd = ndinitat(0, 0, fd, upath(path), p);
    nd.ni_pledge = ni_pledge;
    nd.ni_unveil = ni_unveil;
    p.p_dupfd.set(-1); // XXX check for fdopen
    if flags & O_TRUNC != 0 && flags & (O_EXLOCK | O_SHLOCK) != 0 {
        localtrunc = true;
        flags &= !O_TRUNC; // Must do truncate ourselves
    }
    // KERNEL_LOCK(): nothing without MULTIPROCESSOR.
    let error: Result<(), Errno> = 'error: {
        if let Err(error) = vn_open(&mut nd, flags, cmode) {
            fdplock(fdp);
            if error == Errno::ENODEV && p.p_dupfd.get() >= 0 {
                // XXX from fdopen
                match dupfdopen(p, indx, flags) {
                    Ok(()) => {
                        retval[0] = indx as Register;
                        break 'error Ok(());
                    }
                    Err(e) => {
                        fdremove(fdp, indx);
                        break 'error Err(e);
                    }
                }
            }
            let error = if error == Errno::ERESTART {
                Errno::EINTR
            } else {
                error
            };
            fdremove(fdp, indx);
            break 'error Err(error);
        }
        p.p_dupfd.set(0);
        let vp = ndvp(nd.ni_vp);
        if pledgeopen != 0 && vp.v_type.get() != VCHR {
            fdflags |= UF_PLEDGEOPEN;
        }
        fp.f_flag.store((flags & FMASK) as u32, Ordering::SeqCst);
        fp.f_type.set(DTYPE_VNODE);
        fp.f_ops.set(Some(&VNOPS));
        fp.f_data.set(ptr::from_ref(vp).cast_mut().cast::<c_void>());
        if flags & (O_EXLOCK | O_SHLOCK) != 0 {
            let (mut lf, type_) = open_flock(flags);
            let _ = VOP_UNLOCK(vp);
            if let Err(error) = VOP_ADVLOCK(vp, ptr::from_ref(fp).cast(), F_SETLK, &mut lf, type_) {
                fdplock(fdp);
                // closef will vn_close the file for us.
                fdremove(fdp, indx);
                break 'error Err(error);
            }
            let _ = vn_lock(vp, LK_EXCLUSIVE | LK_RETRY);
            fp.f_iflags.fetch_or(FIF_HASLOCK, Ordering::SeqCst);
        }
        if localtrunc {
            let error = if fp.flag() & FWRITE == 0 {
                Err(Errno::EACCES)
            } else if vp
                .v_mount
                .get()
                .is_some_and(|mp| mp.mnt_flag.get() & MNT_RDONLY != 0)
            {
                Err(Errno::EROFS)
            } else if vp.v_type.get() == VDIR {
                Err(Errno::EISDIR)
            } else {
                vn_writechk(vp).and_then(|()| {
                    let mut vattr = Vattr::new();
                    vattr_null(&mut vattr);
                    vattr.va_size = 0;
                    VOP_SETATTR(vp, &mut vattr, fp.f_cred.get(), p)
                })
            };
            if let Err(error) = error {
                let _ = VOP_UNLOCK(vp);
                fdplock(fdp);
                // closef will close the file for us.
                fdremove(fdp, indx);
                break 'error Err(error);
            }
        }
        let _ = VOP_UNLOCK(vp);
        // KERNEL_UNLOCK().
        retval[0] = indx as Register;
        fdplock(fdp);
        fdinsert(fdp, indx, fdflags, fp);
        fdpunlock(fdp);
        let _ = frele(fp, p);
        return Ok(());
    };
    // error:
    // KERNEL_UNLOCK().
    fdpunlock(fdp);
    let _ = closef(fp, p);
    error
}

/// Get file handle system call.
pub fn sys_getfh(p: &Proc, v: &SysArgs, _retval: &mut [Register; 2]) -> Result<(), Errno> {
    let uap: &SysGetfhArgs = sysargs(v);

    // Must be super user
    suser(p)?;
    let mut nd = ndinit(LOOKUP, FOLLOW | LOCKLEAF, upath(uap.fname.get()), p);
    namei(&mut nd)?;
    let vp = ndvp(nd.ni_vp);
    let mut fh = Fhandle {
        fh_fsid: vp
            .v_mount
            .get()
            .map_or(Fsid::default(), |mp| mp.mnt_stat.get().f_fsid),
        ..Fhandle::default()
    };
    let error = VFS_VPTOFH(vp, &mut fh.fh_fid);
    vput(vp);
    error?;
    copyout_obj(&fh, uap.fhp.get() as usize)
}

/// Open a file given a file handle.
///
/// Check permissions, allocate an open file structure, and call the device open routine if
/// any.
pub fn sys_fhopen(p: &Proc, v: &SysArgs, retval: &mut [Register; 2]) -> Result<(), Errno> {
    let uap: &SysFhopenArgs = sysargs(v);
    let fdp = p.fd();
    let cred = p.p_ucred.get();

    // Must be super user
    suser(p)?;

    let flags = fflags(uap.flags.get());
    if flags & (FREAD | FWRITE) == 0 {
        return Err(Errno::EINVAL);
    }
    if flags & O_CREAT != 0 {
        return Err(Errno::EINVAL);
    }

    let fdflags = if flags & O_CLOEXEC != 0 {
        UF_EXCLOSE
    } else {
        0
    } | if flags & O_CLOFORK != 0 {
        UF_FORKCLOSE
    } else {
        0
    };

    fdplock(fdp);
    let (fp, indx) = match falloc(p) {
        Ok(r) => r,
        Err(e) => {
            fdpunlock(fdp);
            return Err(e);
        }
    };
    fdpunlock(fdp);

    let mut vp: Option<&'static Vnode> = None;
    let error: Errno = 'bad: {
        let fh: Fhandle = match copyin_obj(uap.fhp.get() as usize) {
            Ok(fh) => fh,
            Err(e) => break 'bad e,
        };

        let Some(mp) = vfs_getvfs(&fh.fh_fsid) else {
            break 'bad Errno::ESTALE;
        };

        let v = match VFS_FHTOVP(mp, &fh.fh_fid) {
            Ok(v) => v,
            Err(e) => break 'bad e, // most likely unnecessary sanity for bad:
        };
        vp = Some(v);

        // Now do an effective vn_open
        if v.v_type.get() == crate::sys::vnode::VSOCK {
            break 'bad Errno::EOPNOTSUPP;
        }
        if flags & O_DIRECTORY != 0 && v.v_type.get() != VDIR {
            break 'bad Errno::ENOTDIR;
        }
        if flags & FREAD != 0
            && let Err(e) = VOP_ACCESS(v, VREAD, cred, p)
        {
            break 'bad e;
        }
        if flags & (FWRITE | O_TRUNC) != 0 {
            if v.v_type.get() == VDIR {
                break 'bad Errno::EISDIR;
            }
            if let Err(e) = VOP_ACCESS(v, VWRITE, cred, p).and_then(|()| vn_writechk(v)) {
                break 'bad e;
            }
        }
        if flags & O_TRUNC != 0 {
            let mut va = Vattr::new();
            vattr_null(&mut va);
            va.va_size = 0;
            if let Err(e) = VOP_SETATTR(v, &mut va, cred, p) {
                break 'bad e;
            }
        }
        if let Err(e) = VOP_OPEN(v, flags, cred, p) {
            break 'bad e;
        }
        if flags & FWRITE != 0 {
            v.v_writecount.set(v.v_writecount.get() + 1);
        }

        // done with modified vn_open, now finish what sys_open does.
        fp.f_flag.store((flags & FMASK) as u32, Ordering::SeqCst);
        fp.f_type.set(DTYPE_VNODE);
        fp.f_ops.set(Some(&VNOPS));
        fp.f_data.set(ptr::from_ref(v).cast_mut().cast::<c_void>());
        if flags & (O_EXLOCK | O_SHLOCK) != 0 {
            let (mut lf, type_) = open_flock(flags);
            let _ = VOP_UNLOCK(v);
            if let Err(e) = VOP_ADVLOCK(v, ptr::from_ref(fp).cast(), F_SETLK, &mut lf, type_) {
                vp = None; // closef will vn_close the file
                break 'bad e;
            }
            let _ = vn_lock(v, LK_EXCLUSIVE | LK_RETRY);
            fp.f_iflags.fetch_or(FIF_HASLOCK, Ordering::SeqCst);
        }
        let _ = VOP_UNLOCK(v);
        retval[0] = indx as Register;
        fdplock(fdp);
        fdinsert(fdp, indx, fdflags, fp);
        fdpunlock(fdp);
        let _ = frele(fp, p);
        return Ok(());
    };

    // bad:
    fdplock(fdp);
    fdremove(fdp, indx);
    fdpunlock(fdp);
    let _ = closef(fp, p);
    if let Some(vp) = vp {
        vput(vp);
    }
    Err(error)
}

/// `fhstat(fhp, sb)`: `stat(2)` of a file handle.
pub fn sys_fhstat(p: &Proc, v: &SysArgs, _retval: &mut [Register; 2]) -> Result<(), Errno> {
    let uap: &SysFhstatArgs = sysargs(v);
    let mut sb = Stat::default();

    // Must be super user
    suser(p)?;

    let fh: Fhandle = copyin_obj(uap.fhp.get() as usize)?;

    let Some(mp) = vfs_getvfs(&fh.fh_fsid) else {
        return Err(Errno::ESTALE);
    };
    let vp = VFS_FHTOVP(mp, &fh.fh_fid)?;
    let error = vn_stat(vp, &mut sb, p);
    vput(vp);
    error?;
    copyout(&sb.to_bytes(), uap.sb.get() as usize)
}

/// `fhstatfs(fhp, buf)`: `statfs(2)` of a file handle.
pub fn sys_fhstatfs(p: &Proc, v: &SysArgs, _retval: &mut [Register; 2]) -> Result<(), Errno> {
    let uap: &SysFhstatfsArgs = sysargs(v);

    // Must be super user
    suser(p)?;

    let fh: Fhandle = copyin_obj(uap.fhp.get() as usize)?;

    let Some(mp) = vfs_getvfs(&fh.fh_fsid) else {
        return Err(Errno::ESTALE);
    };
    let vp = VFS_FHTOVP(mp, &fh.fh_fid)?;
    let mp = vp.v_mount.get();
    vput(vp);
    let Some(mp) = mp else {
        return Err(Errno::ESTALE);
    };
    statfs_mnt(mp, p)?;
    mp.update_stat(|sp| sp.f_flags = (mp.mnt_flag.get() & MNT_VISFLAGMASK) as u32);
    copyout(statfs_bytes(&mp.mnt_stat.get()), uap.buf.get() as usize)
}

/// Create a special file or named pipe.
pub fn sys_mknod(p: &Proc, v: &SysArgs, _retval: &mut [Register; 2]) -> Result<(), Errno> {
    let uap: &SysMknodArgs = sysargs(v);

    domknodat(p, AT_FDCWD, uap.path.get(), uap.mode.get(), uap.dev.get())
}

/// `mknodat(fd, path, mode, dev)`.
pub fn sys_mknodat(p: &Proc, v: &SysArgs, _retval: &mut [Register; 2]) -> Result<(), Errno> {
    let uap: &SysMknodatArgs = sysargs(v);

    domknodat(
        p,
        uap.fd.get(),
        uap.path.get(),
        uap.mode.get(),
        uap.dev.get(),
    )
}

/// `domknodat(p, fd, path, mode, dev)`: the common body of `mknod`, `mknodat` and `mkfifo`.
pub fn domknodat(p: &Proc, fd: i32, path: *const u8, mode: Mode, dev: Dev) -> Result<(), Errno> {
    let mut vattr = Vattr::new();

    if dev == VNOVAL {
        return Err(Errno::EINVAL);
    }
    let mut nd = ndinitat(CREATE, LOCKPARENT, fd, upath(path), p);
    nd.ni_pledge = PLEDGE_DPATH;
    nd.ni_unveil = UNVEIL_CREATE;
    namei(&mut nd)?;
    let vp = nd.ni_vp;
    let dvp = ndvp(nd.ni_dvp);
    let error = 'out: {
        if !s_isfifo(mode) || dev != 0 {
            if !vnoperm(dvp)
                && let Err(e) = suser(p)
            {
                break 'out Err(e);
            }
            if p.fd().fd_rdir.get().is_some() {
                break 'out Err(Errno::EINVAL);
            }
        }
        if vp.is_some() {
            break 'out Err(Errno::EEXIST);
        }
        vattr_null(&mut vattr);
        vattr.va_mode = (mode & ALLPERMS) & !p.fd().fd_cmask.get();
        if p.process().ps_flags.load(Ordering::Relaxed) & PS_PLEDGE != 0 {
            vattr.va_mode &= ACCESSPERMS;
        }
        vattr.va_rdev = dev;

        match mode & S_IFMT {
            S_IFMT => {
                // used by badsect to flag bad sectors
                vattr.va_type = VBAD;
                Ok(())
            }
            S_IFCHR => {
                vattr.va_type = VCHR;
                Ok(())
            }
            S_IFBLK => {
                vattr.va_type = VBLK;
                Ok(())
            }
            // option FIFO is not configured (see the module's deviations).
            S_IFIFO => Err(Errno::EOPNOTSUPP),
            _ => Err(Errno::EINVAL),
        }
    };
    match error {
        Ok(()) => {
            let error = VOP_MKNOD(dvp, &mut nd.ni_vp, &mut nd.ni_cnd, &mut vattr);
            vput(dvp);
            error
        }
        Err(e) => {
            let _ = VOP_ABORTOP(dvp, &mut nd.ni_cnd);
            if vp.is_some_and(|vp| ptr::eq(vp, dvp)) {
                vrele(dvp);
            } else {
                vput(dvp);
            }
            if let Some(vp) = vp {
                vrele(vp);
            }
            Err(e)
        }
    }
}

/// Create a named pipe.
pub fn sys_mkfifo(p: &Proc, v: &SysArgs, _retval: &mut [Register; 2]) -> Result<(), Errno> {
    let uap: &SysMkfifoArgs = sysargs(v);

    domknodat(
        p,
        AT_FDCWD,
        uap.path.get(),
        (uap.mode.get() & ALLPERMS) | S_IFIFO,
        0,
    )
}

/// `mkfifoat(fd, path, mode)`.
pub fn sys_mkfifoat(p: &Proc, v: &SysArgs, _retval: &mut [Register; 2]) -> Result<(), Errno> {
    let uap: &SysMkfifoatArgs = sysargs(v);

    domknodat(
        p,
        uap.fd.get(),
        uap.path.get(),
        (uap.mode.get() & ALLPERMS) | S_IFIFO,
        0,
    )
}

/// Make a hard file link.
pub fn sys_link(p: &Proc, v: &SysArgs, _retval: &mut [Register; 2]) -> Result<(), Errno> {
    let uap: &SysLinkArgs = sysargs(v);

    dolinkat(
        p,
        AT_FDCWD,
        uap.path.get(),
        AT_FDCWD,
        uap.link.get(),
        AT_SYMLINK_FOLLOW,
    )
}

/// `linkat(fd1, path1, fd2, path2, flag)`.
pub fn sys_linkat(p: &Proc, v: &SysArgs, _retval: &mut [Register; 2]) -> Result<(), Errno> {
    let uap: &SysLinkatArgs = sysargs(v);

    dolinkat(
        p,
        uap.fd1.get(),
        uap.path1.get(),
        uap.fd2.get(),
        uap.path2.get(),
        uap.flag.get(),
    )
}

/// `dolinkat(p, fd1, path1, fd2, path2, flag)`.
pub fn dolinkat(
    p: &Proc,
    fd1: i32,
    path1: *const u8,
    fd2: i32,
    path2: *const u8,
    flag: i32,
) -> Result<(), Errno> {
    if flag & !AT_SYMLINK_FOLLOW != 0 {
        return Err(Errno::EINVAL);
    }

    let follow = if flag & AT_SYMLINK_FOLLOW != 0 {
        FOLLOW
    } else {
        NOFOLLOW
    };
    let mut nd = ndinitat(LOOKUP, follow, fd1, upath(path1), p);
    nd.ni_pledge = PLEDGE_RPATH;
    nd.ni_unveil = UNVEIL_READ;
    namei(&mut nd)?;
    let vp = ndvp(nd.ni_vp);

    let error = 'out: {
        if vp.v_type.get() == VDIR {
            break 'out Err(Errno::EPERM);
        }

        let mut nd = ndinitat(CREATE, LOCKPARENT, fd2, upath(path2), p);
        nd.ni_pledge = PLEDGE_CPATH;
        nd.ni_unveil = UNVEIL_CREATE;
        if let Err(e) = namei(&mut nd) {
            break 'out Err(e);
        }
        let dvp = ndvp(nd.ni_dvp);
        if let Some(nvp) = nd.ni_vp {
            let _ = VOP_ABORTOP(dvp, &mut nd.ni_cnd);
            if ptr::eq(dvp, nvp) {
                vrele(dvp);
            } else {
                vput(dvp);
            }
            vrele(nvp);
            break 'out Err(Errno::EEXIST);
        }

        // No cross-mount links!
        if !opt_mount_eq(dvp, vp) {
            let _ = VOP_ABORTOP(dvp, &mut nd.ni_cnd);
            vput(dvp);
            break 'out Err(Errno::EXDEV);
        }

        VOP_LINK(dvp, vp, &mut nd.ni_cnd)
    };
    vrele(vp);
    error
}

/// `a->v_mount == b->v_mount`.
fn opt_mount_eq(a: &Vnode, b: &Vnode) -> bool {
    match (a.v_mount.get(), b.v_mount.get()) {
        (Some(x), Some(y)) => ptr::eq(x, y),
        (None, None) => true,
        _ => false,
    }
}

/// Make a symbolic link.
pub fn sys_symlink(p: &Proc, v: &SysArgs, _retval: &mut [Register; 2]) -> Result<(), Errno> {
    let uap: &SysSymlinkArgs = sysargs(v);

    dosymlinkat(p, uap.path.get(), AT_FDCWD, uap.link.get())
}

/// `symlinkat(path, fd, link)`.
pub fn sys_symlinkat(p: &Proc, v: &SysArgs, _retval: &mut [Register; 2]) -> Result<(), Errno> {
    let uap: &SysSymlinkatArgs = sysargs(v);

    dosymlinkat(p, uap.path.get(), uap.fd.get(), uap.link.get())
}

/// `dosymlinkat(p, upath, fd, link)`.
pub fn dosymlinkat(p: &Proc, upath_: *const u8, fd: i32, link: *const u8) -> Result<(), Errno> {
    let mut vattr = Vattr::new();

    let path = NameiBuf::get()?;
    let len = copyinstr(upath_ as usize, path.as_mut())?;
    let mut nd = ndinitat(CREATE, LOCKPARENT, fd, upath(link), p);
    nd.ni_pledge = PLEDGE_CPATH;
    nd.ni_unveil = UNVEIL_CREATE;
    namei(&mut nd)?;
    let dvp = ndvp(nd.ni_dvp);
    if let Some(vp) = nd.ni_vp {
        let _ = VOP_ABORTOP(dvp, &mut nd.ni_cnd);
        if ptr::eq(dvp, vp) {
            vrele(dvp);
        } else {
            vput(dvp);
        }
        vrele(vp);
        return Err(Errno::EEXIST);
    }
    vattr_null(&mut vattr);
    vattr.va_mode = ACCESSPERMS & !p.fd().fd_cmask.get();
    VOP_SYMLINK(
        dvp,
        &mut nd.ni_vp,
        &mut nd.ni_cnd,
        &mut vattr,
        &path.as_mut()[..len - 1],
    )
}

/// Delete a name from the filesystem.
pub fn sys_unlink(p: &Proc, v: &SysArgs, _retval: &mut [Register; 2]) -> Result<(), Errno> {
    let uap: &SysUnlinkArgs = sysargs(v);

    dounlinkat(p, AT_FDCWD, uap.path.get(), 0)
}

/// `unlinkat(fd, path, flag)`.
pub fn sys_unlinkat(p: &Proc, v: &SysArgs, _retval: &mut [Register; 2]) -> Result<(), Errno> {
    let uap: &SysUnlinkatArgs = sysargs(v);

    dounlinkat(p, uap.fd.get(), uap.path.get(), uap.flag.get())
}

/// `dounlinkat(p, fd, path, flag)`: the common body of `unlink`, `unlinkat` and `rmdir`.
pub fn dounlinkat(p: &Proc, fd: i32, path: *const u8, flag: i32) -> Result<(), Errno> {
    if flag & !AT_REMOVEDIR != 0 {
        return Err(Errno::EINVAL);
    }

    let mut nd = ndinitat(DELETE, LOCKPARENT | LOCKLEAF, fd, upath(path), p);
    nd.ni_pledge = PLEDGE_CPATH;
    nd.ni_unveil = UNVEIL_CREATE;
    namei(&mut nd)?;
    let vp = ndvp(nd.ni_vp);
    let dvp = ndvp(nd.ni_dvp);

    let error = 'out: {
        if flag & AT_REMOVEDIR != 0 {
            if vp.v_type.get() != VDIR {
                break 'out Err(Errno::ENOTDIR);
            }
            // No rmdir "." please.
            if ptr::eq(dvp, vp) {
                break 'out Err(Errno::EINVAL);
            }
            // A mounted on directory cannot be deleted.
            if vp.v_mountedhere().is_some() {
                break 'out Err(Errno::EBUSY);
            }
        }

        // The root of a mounted filesystem cannot be deleted.
        if vp.v_flag.get() & VROOT != 0 {
            break 'out Err(Errno::EBUSY);
        }
        Ok(())
    };
    match error {
        Ok(()) => {
            if flag & AT_REMOVEDIR != 0 {
                VOP_RMDIR(dvp, vp, &mut nd.ni_cnd)
            } else {
                let _ = uvm_vnp_uncache(vp);
                VOP_REMOVE(dvp, vp, &mut nd.ni_cnd)
            }
        }
        Err(e) => {
            let _ = VOP_ABORTOP(dvp, &mut nd.ni_cnd);
            if ptr::eq(dvp, vp) {
                vrele(dvp);
            } else {
                vput(dvp);
            }
            vput(vp);
            Err(e)
        }
    }
}

/// Reposition read/write file offset.
pub fn sys_lseek(p: &Proc, v: &SysArgs, retval: &mut [Register; 2]) -> Result<(), Errno> {
    let uap: &SysLseekArgs = sysargs(v);
    let fdp = p.fd();

    let Some(fp) = fd_getfile(fdp, uap.fd.get()) else {
        return Err(Errno::EBADF);
    };
    let error = 'bad: {
        let Some(seek) = fp.ops().fo_seek else {
            break 'bad Err(Errno::ESPIPE);
        };
        let mut offset: Off = uap.offset.get();

        if let Err(e) = seek(fp, &mut offset, uap.whence.get(), p) {
            break 'bad Err(e);
        }

        retval[0] = offset as Register;
        crate::kern::kern_lock::mtx_enter(&fp.f_mtx);
        fp.f_seek.set(fp.f_seek.get() + 1);
        crate::kern::kern_lock::mtx_leave(&fp.f_mtx);
        Ok(())
    };
    let _ = frele(fp, p);
    error
}

/// Check access permissions.
pub fn sys_access(p: &Proc, v: &SysArgs, _retval: &mut [Register; 2]) -> Result<(), Errno> {
    let uap: &SysAccessArgs = sysargs(v);

    dofaccessat(p, AT_FDCWD, uap.path.get(), uap.amode.get(), 0)
}

/// `faccessat(fd, path, amode, flag)`.
pub fn sys_faccessat(p: &Proc, v: &SysArgs, _retval: &mut [Register; 2]) -> Result<(), Errno> {
    let uap: &SysFaccessatArgs = sysargs(v);

    dofaccessat(
        p,
        uap.fd.get(),
        uap.path.get(),
        uap.amode.get(),
        uap.flag.get(),
    )
}

/// `dofaccessat(p, fd, path, amode, flag)`: the common body of `access` and `faccessat`.
pub fn dofaccessat(p: &Proc, fd: i32, path: *const u8, amode: i32, flag: i32) -> Result<(), Errno> {
    let mut vflags = 0;

    if amode & !(R_OK | W_OK | X_OK) != 0 {
        return Err(Errno::EINVAL);
    }
    if flag & !AT_EACCESS != 0 {
        return Err(Errno::EINVAL);
    }

    let mut newcred = None;
    let oldcred = p.p_ucred.get();
    let old = p.ucred();

    // If access as real ids was requested and they really differ, give the thread new creds
    // with them reset
    if flag & AT_EACCESS == 0
        && (old.cr_uid.get() != old.cr_ruid.get() || old.cr_gid.get() != old.cr_rgid.get())
    {
        let cr = crdup(old);
        cr.cr_uid.set(cr.cr_ruid.get());
        cr.cr_gid.set(cr.cr_rgid.get());
        p.p_ucred.set(cr);
        newcred = Some(cr);
    }

    let mut nd = ndinitat(LOOKUP, FOLLOW | LOCKLEAF, fd, upath(path), p);
    nd.ni_pledge = PLEDGE_RPATH;
    nd.ni_unveil = UNVEIL_READ;
    if amode & R_OK != 0 {
        vflags |= VREAD;
    }
    if amode & W_OK != 0 {
        vflags |= VWRITE;
        nd.ni_unveil |= UNVEIL_WRITE;
    }
    if amode & X_OK != 0 {
        vflags |= VEXEC;
    }
    let error = namei(&mut nd).and_then(|()| {
        let vp = ndvp(nd.ni_vp);

        // Flags == 0 means only check for existence.
        let mut error = Ok(());
        if amode != 0 {
            error = VOP_ACCESS(vp, vflags, p.p_ucred.get(), p);
            if error.is_ok() && vflags & VWRITE != 0 {
                error = vn_writechk(vp);
            }
        }
        vput(vp);
        error
    });
    // out:
    if let Some(newcred) = newcred {
        p.p_ucred.set(oldcred);
        crfree(newcred);
    }
    error
}

/// Get file status; this version follows links.
pub fn sys_stat(p: &Proc, v: &SysArgs, _retval: &mut [Register; 2]) -> Result<(), Errno> {
    let uap: &SysStatArgs = sysargs(v);

    dofstatat(p, AT_FDCWD, uap.path.get(), uap.ub.get() as usize, 0)
}

/// `fstatat(fd, path, buf, flag)`.
pub fn sys_fstatat(p: &Proc, v: &SysArgs, _retval: &mut [Register; 2]) -> Result<(), Errno> {
    let uap: &SysFstatatArgs = sysargs(v);

    dofstatat(
        p,
        uap.fd.get(),
        uap.path.get(),
        uap.buf.get() as usize,
        uap.flag.get(),
    )
}

/// `dofstatat(p, fd, path, buf, flag)`: the common body of `stat`, `lstat` and `fstatat`.
pub fn dofstatat(p: &Proc, fd: i32, path: *const u8, buf: usize, flag: i32) -> Result<(), Errno> {
    let mut sb = Stat::default();

    if flag & !AT_SYMLINK_NOFOLLOW != 0 {
        return Err(Errno::EINVAL);
    }

    let follow = if flag & AT_SYMLINK_NOFOLLOW != 0 {
        NOFOLLOW
    } else {
        FOLLOW
    };
    let mut nd = ndinitat(LOOKUP, follow | LOCKLEAF, fd, upath(path), p);
    nd.ni_pledge = PLEDGE_RPATH;
    nd.ni_unveil = UNVEIL_READ;
    // KERNEL_LOCK(): nothing without MULTIPROCESSOR.
    namei(&mut nd)?;
    let vp = ndvp(nd.ni_vp);
    let error = vn_stat(vp, &mut sb, p);
    vput(vp);
    // KERNEL_UNLOCK().
    error?;
    // Don't let non-root see generation numbers (for NFS security)
    if suser(p).is_err() {
        sb.st_gen = 0;
    }
    // KTRACE: not configured.
    copyout(&sb.to_bytes(), buf)
}

/// Get file status; this version does not follow links.
pub fn sys_lstat(p: &Proc, v: &SysArgs, _retval: &mut [Register; 2]) -> Result<(), Errno> {
    let uap: &SysLstatArgs = sysargs(v);

    dofstatat(
        p,
        AT_FDCWD,
        uap.path.get(),
        uap.ub.get() as usize,
        AT_SYMLINK_NOFOLLOW,
    )
}

/// Get configurable pathname variables.
pub fn sys_pathconf(p: &Proc, v: &SysArgs, retval: &mut [Register; 2]) -> Result<(), Errno> {
    let uap: &SysPathconfArgs = sysargs(v);

    dopathconfat(p, AT_FDCWD, uap.path.get(), uap.name.get(), 0, retval)
}

/// `pathconfat(fd, path, name, flag)`.
pub fn sys_pathconfat(p: &Proc, v: &SysArgs, retval: &mut [Register; 2]) -> Result<(), Errno> {
    let uap: &SysPathconfatArgs = sysargs(v);

    dopathconfat(
        p,
        uap.fd.get(),
        uap.path.get(),
        uap.name.get(),
        uap.flag.get(),
        retval,
    )
}

/// `dopathconfat(p, fd, path, name, flag, retval)`.
pub fn dopathconfat(
    p: &Proc,
    fd: i32,
    path: *const u8,
    name: i32,
    flag: i32,
    retval: &mut [Register; 2],
) -> Result<(), Errno> {
    if flag & !AT_SYMLINK_NOFOLLOW != 0 {
        return Err(Errno::EINVAL);
    }

    let follow = if flag & AT_SYMLINK_NOFOLLOW != 0 {
        NOFOLLOW
    } else {
        FOLLOW
    };
    let mut nd = ndinitat(LOOKUP, follow | LOCKLEAF, fd, upath(path), p);
    nd.ni_pledge = PLEDGE_RPATH;
    nd.ni_unveil = UNVEIL_READ;
    namei(&mut nd)?;
    let vp = ndvp(nd.ni_vp);
    let error = VOP_PATHCONF(vp, name, &mut retval[0]);
    vput(vp);
    error
}

/// Return target name of a symbolic link.
pub fn sys_readlink(p: &Proc, v: &SysArgs, retval: &mut [Register; 2]) -> Result<(), Errno> {
    let uap: &SysReadlinkArgs = sysargs(v);

    doreadlinkat(
        p,
        AT_FDCWD,
        uap.path.get(),
        uap.buf.get(),
        uap.count.get(),
        retval,
    )
}

/// `readlinkat(fd, path, buf, count)`.
pub fn sys_readlinkat(p: &Proc, v: &SysArgs, retval: &mut [Register; 2]) -> Result<(), Errno> {
    let uap: &SysReadlinkatArgs = sysargs(v);

    doreadlinkat(
        p,
        uap.fd.get(),
        uap.path.get(),
        uap.buf.get(),
        uap.count.get(),
        retval,
    )
}

/// `doreadlinkat(p, fd, path, buf, count, retval)`.
pub fn doreadlinkat(
    p: &Proc,
    fd: i32,
    path: *const u8,
    buf: *mut u8,
    count: usize,
    retval: &mut [Register; 2],
) -> Result<(), Errno> {
    let mut nd = ndinitat(LOOKUP, NOFOLLOW | LOCKLEAF, fd, upath(path), p);
    nd.ni_pledge = PLEDGE_RPATH;
    nd.ni_unveil = UNVEIL_READ;
    namei(&mut nd)?;
    let vp = ndvp(nd.ni_vp);
    let error = if vp.v_type.get() != crate::sys::vnode::VLNK {
        Err(Errno::EINVAL)
    } else {
        let mut aiov = [Iovec {
            iov_base: buf.cast::<c_void>(),
            iov_len: count,
        }];
        let mut auio = Uio {
            uio_iov: &mut aiov,
            uio_offset: 0,
            uio_resid: count,
            uio_segflg: UioSeg::UIO_USERSPACE,
            uio_rw: UioRw::UIO_READ,
            uio_procp: Some(p),
        };
        let error = VOP_READLINK(vp, &mut auio, p.p_ucred.get());
        retval[0] = (count - auio.uio_resid) as Register;
        error
    };
    vput(vp);
    error
}

/// Change flags of a file given a path name.
pub fn sys_chflags(p: &Proc, v: &SysArgs, _retval: &mut [Register; 2]) -> Result<(), Errno> {
    let uap: &SysChflagsArgs = sysargs(v);

    dochflagsat(p, AT_FDCWD, uap.path.get(), uap.flags.get(), 0)
}

/// `chflagsat(fd, path, flags, atflags)`.
pub fn sys_chflagsat(p: &Proc, v: &SysArgs, _retval: &mut [Register; 2]) -> Result<(), Errno> {
    let uap: &SysChflagsatArgs = sysargs(v);

    dochflagsat(
        p,
        uap.fd.get(),
        uap.path.get(),
        uap.flags.get(),
        uap.atflags.get(),
    )
}

/// `dochflagsat(p, fd, path, flags, atflags)`.
pub fn dochflagsat(
    p: &Proc,
    fd: i32,
    path: *const u8,
    flags: u32,
    atflags: i32,
) -> Result<(), Errno> {
    if atflags & !AT_SYMLINK_NOFOLLOW != 0 {
        return Err(Errno::EINVAL);
    }

    let follow = if atflags & AT_SYMLINK_NOFOLLOW != 0 {
        NOFOLLOW
    } else {
        FOLLOW
    };
    let mut nd = ndinitat(LOOKUP, follow, fd, upath(path), p);
    nd.ni_pledge = PLEDGE_FATTR | PLEDGE_RPATH;
    nd.ni_unveil = UNVEIL_WRITE;
    namei(&mut nd)?;
    dovchflags(p, ndvp(nd.ni_vp), flags)
}

/// Change flags of a file given a file descriptor.
pub fn sys_fchflags(p: &Proc, v: &SysArgs, _retval: &mut [Register; 2]) -> Result<(), Errno> {
    let uap: &SysFchflagsArgs = sysargs(v);

    let fp = getvnode(p, uap.fd.get())?;
    if p.fd().ofileflags(uap.fd.get() as usize) & UF_PLEDGEOPEN != 0 {
        let _ = frele(fp, p);
        return Err(Errno::EPERM);
    }
    let vp = fp.vnode();
    vref(vp);
    let _ = frele(fp, p);
    dovchflags(p, vp, uap.flags.get())
}

/// `dovchflags(p, vp, flags)`: sets the file flags of a referenced, unlocked vnode, and
/// releases it.
pub fn dovchflags(p: &Proc, vp: &'static Vnode, flags: u32) -> Result<(), Errno> {
    let mut vattr = Vattr::new();

    let _ = vn_lock(vp, LK_EXCLUSIVE | LK_RETRY);
    let error = 'out: {
        if vp
            .v_mount
            .get()
            .is_some_and(|mp| mp.mnt_flag.get() & MNT_RDONLY != 0)
        {
            break 'out Err(Errno::EROFS);
        } else if flags == VNOVAL as u32 {
            break 'out Err(Errno::EINVAL);
        }
        if suser(p).is_err() {
            if let Err(e) = VOP_GETATTR(vp, &mut vattr, p.p_ucred.get(), p) {
                break 'out Err(e);
            }
            if vattr.va_type == VCHR || vattr.va_type == VBLK {
                break 'out Err(Errno::EINVAL);
            }
        }
        vattr_null(&mut vattr);
        vattr.va_flags = u64::from(flags);
        VOP_SETATTR(vp, &mut vattr, p.p_ucred.get(), p)
    };
    vput(vp);
    error
}

/// Change mode of a file given path name.
pub fn sys_chmod(p: &Proc, v: &SysArgs, _retval: &mut [Register; 2]) -> Result<(), Errno> {
    let uap: &SysChmodArgs = sysargs(v);

    dofchmodat(p, AT_FDCWD, uap.path.get(), uap.mode.get(), 0)
}

/// `fchmodat(fd, path, mode, flag)`.
pub fn sys_fchmodat(p: &Proc, v: &SysArgs, _retval: &mut [Register; 2]) -> Result<(), Errno> {
    let uap: &SysFchmodatArgs = sysargs(v);

    dofchmodat(
        p,
        uap.fd.get(),
        uap.path.get(),
        uap.mode.get(),
        uap.flag.get(),
    )
}

/// `dofchmodat(p, fd, path, mode, flag)`.
pub fn dofchmodat(
    p: &Proc,
    fd: i32,
    path: *const u8,
    mut mode: Mode,
    flag: i32,
) -> Result<(), Errno> {
    let mut vattr = Vattr::new();

    if mode & !(S_IFMT | ALLPERMS) != 0 {
        return Err(Errno::EINVAL);
    }
    if p.process().ps_flags.load(Ordering::Relaxed) & PS_PLEDGE != 0 {
        mode &= ACCESSPERMS;
    }
    if flag & !AT_SYMLINK_NOFOLLOW != 0 {
        return Err(Errno::EINVAL);
    }

    let follow = if flag & AT_SYMLINK_NOFOLLOW != 0 {
        NOFOLLOW
    } else {
        FOLLOW
    };
    let mut nd = ndinitat(LOOKUP, follow, fd, upath(path), p);
    nd.ni_pledge = PLEDGE_FATTR | PLEDGE_RPATH;
    nd.ni_unveil = UNVEIL_WRITE;
    namei(&mut nd)?;
    let vp = ndvp(nd.ni_vp);
    let _ = vn_lock(vp, LK_EXCLUSIVE | LK_RETRY);
    let error = if vp
        .v_mount
        .get()
        .is_some_and(|mp| mp.mnt_flag.get() & MNT_RDONLY != 0)
    {
        Err(Errno::EROFS)
    } else {
        vattr_null(&mut vattr);
        vattr.va_mode = mode & ALLPERMS;
        VOP_SETATTR(vp, &mut vattr, p.p_ucred.get(), p)
    };
    vput(vp);
    error
}

/// Change mode of a file given a file descriptor.
pub fn sys_fchmod(p: &Proc, v: &SysArgs, _retval: &mut [Register; 2]) -> Result<(), Errno> {
    let uap: &SysFchmodArgs = sysargs(v);
    let mut vattr = Vattr::new();
    let mut mode = uap.mode.get();

    if mode & !(S_IFMT | ALLPERMS) != 0 {
        return Err(Errno::EINVAL);
    }
    if p.process().ps_flags.load(Ordering::Relaxed) & PS_PLEDGE != 0 {
        mode &= ACCESSPERMS;
    }

    let fp = getvnode(p, uap.fd.get())?;
    let vp = fp.vnode();
    let _ = vn_lock(vp, LK_EXCLUSIVE | LK_RETRY);
    let error = if vp
        .v_mount
        .get()
        .is_some_and(|mp| mp.mnt_flag.get() & MNT_RDONLY != 0)
    {
        Err(Errno::EROFS)
    } else if p.fd().ofileflags(uap.fd.get() as usize) & UF_PLEDGEOPEN != 0 {
        Err(Errno::EPERM)
    } else {
        vattr_null(&mut vattr);
        vattr.va_mode = mode & ALLPERMS;
        VOP_SETATTR(vp, &mut vattr, p.p_ucred.get(), p)
    };
    let _ = VOP_UNLOCK(vp);
    let _ = frele(fp, p);
    error
}

/// Set ownership given a path name.
pub fn sys_chown(p: &Proc, v: &SysArgs, _retval: &mut [Register; 2]) -> Result<(), Errno> {
    let uap: &SysChownArgs = sysargs(v);

    dofchownat(p, AT_FDCWD, uap.path.get(), uap.uid.get(), uap.gid.get(), 0)
}

/// `fchownat(fd, path, uid, gid, flag)`.
pub fn sys_fchownat(p: &Proc, v: &SysArgs, _retval: &mut [Register; 2]) -> Result<(), Errno> {
    let uap: &SysFchownatArgs = sysargs(v);

    dofchownat(
        p,
        uap.fd.get(),
        uap.path.get(),
        uap.uid.get(),
        uap.gid.get(),
        uap.flag.get(),
    )
}

/// The ownership change on a locked vnode shared by the `chown` family: the attributes
/// `VOP_SETATTR` gets, the set-id bits cleared when the owner changes.
fn chown_vnode(p: &Proc, vp: &'static Vnode, uid: Uid, gid: Gid) -> Result<(), Errno> {
    let mut vattr = Vattr::new();

    pledge_chown(p, uid, gid)?;
    let mode = if (uid != VNOVAL as Uid || gid != VNOVAL as Gid) && !vnoperm(vp) {
        VOP_GETATTR(vp, &mut vattr, p.p_ucred.get(), p)?;
        let mode = vattr.va_mode & !(VSUID | VSGID);
        if mode == vattr.va_mode {
            VNOVAL as Mode
        } else {
            mode
        }
    } else {
        VNOVAL as Mode
    };
    vattr_null(&mut vattr);
    vattr.va_uid = uid;
    vattr.va_gid = gid;
    vattr.va_mode = mode;
    VOP_SETATTR(vp, &mut vattr, p.p_ucred.get(), p)
}

/// `dofchownat(p, fd, path, uid, gid, flag)`.
pub fn dofchownat(
    p: &Proc,
    fd: i32,
    path: *const u8,
    uid: Uid,
    gid: Gid,
    flag: i32,
) -> Result<(), Errno> {
    if flag & !AT_SYMLINK_NOFOLLOW != 0 {
        return Err(Errno::EINVAL);
    }

    let follow = if flag & AT_SYMLINK_NOFOLLOW != 0 {
        NOFOLLOW
    } else {
        FOLLOW
    };
    let mut nd = ndinitat(LOOKUP, follow, fd, upath(path), p);
    nd.ni_pledge = PLEDGE_CHOWN | PLEDGE_RPATH;
    nd.ni_unveil = UNVEIL_WRITE;
    namei(&mut nd)?;
    let vp = ndvp(nd.ni_vp);
    let _ = vn_lock(vp, LK_EXCLUSIVE | LK_RETRY);
    let error = if vp
        .v_mount
        .get()
        .is_some_and(|mp| mp.mnt_flag.get() & MNT_RDONLY != 0)
    {
        Err(Errno::EROFS)
    } else {
        chown_vnode(p, vp, uid, gid)
    };
    vput(vp);
    error
}

/// Set ownership given a path name, without following links.
pub fn sys_lchown(p: &Proc, v: &SysArgs, _retval: &mut [Register; 2]) -> Result<(), Errno> {
    let uap: &SysLchownArgs = sysargs(v);
    let uid = uap.uid.get();
    let gid = uap.gid.get();

    let mut nd = ndinit(LOOKUP, NOFOLLOW, upath(uap.path.get()), p);
    nd.ni_pledge = PLEDGE_CHOWN | PLEDGE_RPATH;
    nd.ni_unveil = UNVEIL_WRITE;
    namei(&mut nd)?;
    let vp = ndvp(nd.ni_vp);
    let _ = vn_lock(vp, LK_EXCLUSIVE | LK_RETRY);
    let error = if vp
        .v_mount
        .get()
        .is_some_and(|mp| mp.mnt_flag.get() & MNT_RDONLY != 0)
    {
        Err(Errno::EROFS)
    } else {
        chown_vnode(p, vp, uid, gid)
    };
    vput(vp);
    error
}

/// Set ownership given a file descriptor.
pub fn sys_fchown(p: &Proc, v: &SysArgs, _retval: &mut [Register; 2]) -> Result<(), Errno> {
    let uap: &SysFchownArgs = sysargs(v);
    let uid = uap.uid.get();
    let gid = uap.gid.get();

    let fp = getvnode(p, uap.fd.get())?;
    let vp = fp.vnode();
    let _ = vn_lock(vp, LK_EXCLUSIVE | LK_RETRY);
    let error = if vp
        .v_mount
        .get()
        .is_some_and(|mp| mp.mnt_flag.get() & MNT_RDONLY != 0)
    {
        Err(Errno::EROFS)
    } else if p.fd().ofileflags(uap.fd.get() as usize) & UF_PLEDGEOPEN != 0 {
        Err(Errno::EPERM)
    } else {
        chown_vnode(p, vp, uid, gid)
    };
    let _ = VOP_UNLOCK(vp);
    let _ = frele(fp, p);
    error
}

/// `copyin` of a user `struct timeval[2]`.
fn copyin_timevals(uaddr: usize) -> Result<[Timeval; 2], Errno> {
    let mut b = [0u8; 2 * size_of::<Timeval>()];
    copyin(uaddr, &mut b)?;
    let word = |i: usize| {
        let mut w = [0u8; 8];
        w.copy_from_slice(&b[i * 8..i * 8 + 8]);
        i64::from_ne_bytes(w)
    };
    Ok([
        Timeval::new(word(0), word(1) as isize),
        Timeval::new(word(2), word(3) as isize),
    ])
}

/// `copyin` of a user `struct timespec[2]`.
fn copyin_timespecs(uaddr: usize) -> Result<[Timespec; 2], Errno> {
    Ok([
        copyin_obj::<Timespec>(uaddr)?,
        copyin_obj::<Timespec>(uaddr + size_of::<Timespec>())?,
    ])
}

/// The `utimes(2)`-style times: the user's two timevals, or `UTIME_NOW` for both.
fn utimes_times(tptr: usize) -> Result<[Timespec; 2], Errno> {
    let mut ts = [Timespec::new(0, 0); 2];
    if tptr != 0 {
        let tv = copyin_timevals(tptr)?;
        // KTRACE: not configured.
        if !tv[0].is_valid() || !tv[1].is_valid() {
            return Err(Errno::EINVAL);
        }
        ts[0] = timeval_to_timespec(&tv[0]);
        ts[1] = timeval_to_timespec(&tv[1]);
    } else {
        ts[0].tv_nsec = UTIME_NOW;
        ts[1].tv_nsec = UTIME_NOW;
    }
    Ok(ts)
}

/// The `utimensat(2)`-style times: the user's two timespecs (each possibly `UTIME_NOW` or
/// `UTIME_OMIT`), or `UTIME_NOW` for both.
fn utimens_times(tsp: usize) -> Result<[Timespec; 2], Errno> {
    let mut ts = [Timespec::new(0, 0); 2];
    if tsp != 0 {
        ts = copyin_timespecs(tsp)?;
        for t in &ts {
            if t.tv_nsec == UTIME_NOW || t.tv_nsec == UTIME_OMIT {
                continue;
            }
            // KTRACE: not configured.
            if !t.is_valid() {
                return Err(Errno::EINVAL);
            }
        }
    } else {
        ts[0].tv_nsec = UTIME_NOW;
        ts[1].tv_nsec = UTIME_NOW;
    }
    Ok(ts)
}

/// Set the access and modification times given a path name.
pub fn sys_utimes(p: &Proc, v: &SysArgs, _retval: &mut [Register; 2]) -> Result<(), Errno> {
    let uap: &SysUtimesArgs = sysargs(v);

    let mut ts = utimes_times(uap.tptr.get() as usize)?;
    doutimensat(p, AT_FDCWD, uap.path.get(), &mut ts, 0)
}

/// `utimensat(fd, path, times, flag)`.
pub fn sys_utimensat(p: &Proc, v: &SysArgs, _retval: &mut [Register; 2]) -> Result<(), Errno> {
    let uap: &SysUtimensatArgs = sysargs(v);

    let mut ts = utimens_times(uap.times.get() as usize)?;
    doutimensat(p, uap.fd.get(), uap.path.get(), &mut ts, uap.flag.get())
}

/// `doutimensat(p, fd, path, ts, flag)`.
pub fn doutimensat(
    p: &Proc,
    fd: i32,
    path: *const u8,
    ts: &mut [Timespec; 2],
    flag: i32,
) -> Result<(), Errno> {
    if flag & !AT_SYMLINK_NOFOLLOW != 0 {
        return Err(Errno::EINVAL);
    }

    let follow = if flag & AT_SYMLINK_NOFOLLOW != 0 {
        NOFOLLOW
    } else {
        FOLLOW
    };
    let mut nd = ndinitat(LOOKUP, follow, fd, upath(path), p);
    nd.ni_pledge = PLEDGE_FATTR | PLEDGE_RPATH;
    nd.ni_unveil = UNVEIL_WRITE;
    namei(&mut nd)?;
    dovutimens(p, ndvp(nd.ni_vp), ts)
}

/// `dovutimens(p, vp, ts)`: sets the times of a referenced, unlocked vnode, and releases it.
pub fn dovutimens(p: &Proc, vp: &'static Vnode, ts: &mut [Timespec; 2]) -> Result<(), Errno> {
    let mut vattr = Vattr::new();

    // KTRACE: not configured.

    vattr_null(&mut vattr);

    // make sure ctime is updated even if neither mtime nor atime is
    vattr.va_vaflags = VA_UTIMES_CHANGE;

    if ts[0].tv_nsec == UTIME_NOW || ts[1].tv_nsec == UTIME_NOW {
        if ts[0].tv_nsec == UTIME_NOW && ts[1].tv_nsec == UTIME_NOW {
            vattr.va_vaflags |= VA_UTIMES_NULL;
        }

        let now = getnanotime();
        if ts[0].tv_nsec == UTIME_NOW {
            ts[0] = now;
        }
        if ts[1].tv_nsec == UTIME_NOW {
            ts[1] = now;
        }
    }

    if ts[0].tv_nsec != UTIME_OMIT {
        vattr.va_atime = ts[0];
    }
    if ts[1].tv_nsec != UTIME_OMIT {
        vattr.va_mtime = ts[1];
    }

    let _ = vn_lock(vp, LK_EXCLUSIVE | LK_RETRY);
    let error = if vp
        .v_mount
        .get()
        .is_some_and(|mp| mp.mnt_flag.get() & MNT_RDONLY != 0)
    {
        Err(Errno::EROFS)
    } else {
        VOP_SETATTR(vp, &mut vattr, p.p_ucred.get(), p)
    };
    vput(vp);
    error
}

/// Set the access and modification times given a file descriptor.
pub fn sys_futimes(p: &Proc, v: &SysArgs, _retval: &mut [Register; 2]) -> Result<(), Errno> {
    let uap: &SysFutimesArgs = sysargs(v);

    let mut ts = utimes_times(uap.tptr.get() as usize)?;
    dofutimens(p, uap.fd.get(), &mut ts)
}

/// `futimens(fd, times)`.
pub fn sys_futimens(p: &Proc, v: &SysArgs, _retval: &mut [Register; 2]) -> Result<(), Errno> {
    let uap: &SysFutimensArgs = sysargs(v);

    let mut ts = utimens_times(uap.times.get() as usize)?;
    dofutimens(p, uap.fd.get(), &mut ts)
}

/// `dofutimens(p, fd, ts)`.
pub fn dofutimens(p: &Proc, fd: i32, ts: &mut [Timespec; 2]) -> Result<(), Errno> {
    let fp = getvnode(p, fd)?;
    let vp = fp.vnode();
    vref(vp);
    let _ = frele(fp, p);

    dovutimens(p, vp, ts)
}

/// Truncate a file given a vnode.
pub fn dotruncate(p: &Proc, vp: &'static Vnode, len: Off) -> Result<(), Errno> {
    let mut vattr = Vattr::new();

    if len < 0 {
        return Err(Errno::EINVAL);
    }
    if vp.v_type.get() == VDIR {
        return Err(Errno::EISDIR);
    }
    vn_writechk(vp)?;
    if vp.v_type.get() == crate::sys::vnode::VREG && len as u64 > lim_cur_proc(p, RLIMIT_FSIZE) {
        VOP_GETATTR(vp, &mut vattr, p.p_ucred.get(), p)?;
        if len as u64 > vattr.va_size {
            // if extending over the limit, send signal and fail
            psignal(p, SIGXFSZ);
            return Err(Errno::EFBIG);
        }
    }
    vattr_null(&mut vattr);
    vattr.va_size = len as u64;
    VOP_SETATTR(vp, &mut vattr, p.p_ucred.get(), p)
}

/// Truncate a file given its path name.
pub fn sys_truncate(p: &Proc, v: &SysArgs, _retval: &mut [Register; 2]) -> Result<(), Errno> {
    let uap: &SysTruncateArgs = sysargs(v);

    let mut nd = ndinit(LOOKUP, FOLLOW, upath(uap.path.get()), p);
    nd.ni_pledge = PLEDGE_FATTR | PLEDGE_RPATH;
    nd.ni_unveil = UNVEIL_WRITE;
    namei(&mut nd)?;
    let vp = ndvp(nd.ni_vp);
    let _ = vn_lock(vp, LK_EXCLUSIVE | LK_RETRY);
    let error = VOP_ACCESS(vp, VWRITE, p.p_ucred.get(), p)
        .and_then(|()| dotruncate(p, vp, uap.length.get()));
    vput(vp);
    error
}

/// Truncate a file given a file descriptor.
pub fn sys_ftruncate(p: &Proc, v: &SysArgs, _retval: &mut [Register; 2]) -> Result<(), Errno> {
    let uap: &SysFtruncateArgs = sysargs(v);

    let fp = getvnode(p, uap.fd.get())?;
    let error = if fp.flag() & FWRITE == 0 {
        Err(Errno::EINVAL)
    } else if p.fd().ofileflags(uap.fd.get() as usize) & UF_PLEDGEOPEN != 0 {
        Err(Errno::EPERM)
    } else {
        let vp = fp.vnode();
        let _ = vn_lock(vp, LK_EXCLUSIVE | LK_RETRY);
        let error = dotruncate(p, vp, uap.length.get());
        let _ = VOP_UNLOCK(vp);
        error
    };
    let _ = frele(fp, p);
    error
}

/// Sync an open file.
pub fn sys_fsync(p: &Proc, v: &SysArgs, _retval: &mut [Register; 2]) -> Result<(), Errno> {
    let uap: &SysFsyncArgs = sysargs(v);

    let fp = getvnode(p, uap.fd.get())?;
    let vp = fp.vnode();
    let _ = vn_lock(vp, LK_EXCLUSIVE | LK_RETRY);
    let error = VOP_FSYNC(vp, fp.f_cred.get(), MNT_WAIT, p);

    let _ = VOP_UNLOCK(vp);
    let _ = frele(fp, p);
    error
}

/// Rename files. Source and destination must either both be directories, or both not be
/// directories. If target is a directory, it must be empty.
pub fn sys_rename(p: &Proc, v: &SysArgs, _retval: &mut [Register; 2]) -> Result<(), Errno> {
    let uap: &SysRenameArgs = sysargs(v);

    dorenameat(p, AT_FDCWD, uap.from.get(), AT_FDCWD, uap.to.get())
}

/// `renameat(fromfd, from, tofd, to)`.
pub fn sys_renameat(p: &Proc, v: &SysArgs, _retval: &mut [Register; 2]) -> Result<(), Errno> {
    let uap: &SysRenameatArgs = sysargs(v);

    dorenameat(
        p,
        uap.fromfd.get(),
        uap.from.get(),
        uap.tofd.get(),
        uap.to.get(),
    )
}

/// `dorenameat(p, fromfd, from, tofd, to)`.
pub fn dorenameat(
    p: &Proc,
    fromfd: i32,
    from: *const u8,
    tofd: i32,
    to: *const u8,
) -> Result<(), Errno> {
    let mut fromnd = ndinitat(DELETE, WANTPARENT | SAVESTART, fromfd, upath(from), p);
    fromnd.ni_pledge = PLEDGE_RPATH | PLEDGE_CPATH;
    fromnd.ni_unveil = UNVEIL_READ | UNVEIL_CREATE;
    namei(&mut fromnd)?;
    let fvp = ndvp(fromnd.ni_vp);
    let fdvp = ndvp(fromnd.ni_dvp);

    let mut flags = LOCKPARENT | LOCKLEAF | NOCACHE | SAVESTART;
    // rename("foo/", "bar/");  is  OK
    if fvp.v_type.get() == VDIR {
        flags |= STRIPSLASHES;
    }

    let mut tond = ndinitat(RENAME, flags, tofd, upath(to), p);
    tond.ni_pledge = PLEDGE_CPATH;
    tond.ni_unveil = UNVEIL_CREATE;
    let error = 'out1: {
        if let Err(error) = namei(&mut tond) {
            let _ = VOP_ABORTOP(fdvp, &mut fromnd.ni_cnd);
            vrele(fdvp);
            vrele(fvp);
            break 'out1 Err(error);
        }
        let tdvp = ndvp(tond.ni_dvp);
        let tvp = tond.ni_vp;
        let mut same = false;
        let mut error = 'out: {
            if let Some(tvp) = tvp {
                if fvp.v_type.get() == VDIR && tvp.v_type.get() != VDIR {
                    break 'out Err(Errno::ENOTDIR);
                } else if fvp.v_type.get() != VDIR && tvp.v_type.get() == VDIR {
                    break 'out Err(Errno::EISDIR);
                }
            }
            let mut error = Ok(());
            if ptr::eq(fvp, tdvp) {
                error = Err(Errno::EINVAL);
            }
            // If source is the same as the destination (that is the same inode number)
            if tvp.is_some_and(|tvp| ptr::eq(fvp, tvp)) {
                same = true;
            }
            error
        };
        if error.is_ok() && !same {
            if let Some(tvp) = tvp {
                let _ = uvm_vnp_uncache(tvp);
            }
            error = VOP_RENAME(fdvp, fvp, &mut fromnd.ni_cnd, tdvp, tvp, &mut tond.ni_cnd);
        } else {
            let _ = VOP_ABORTOP(tdvp, &mut tond.ni_cnd);
            if tvp.is_some_and(|tvp| ptr::eq(tdvp, tvp)) {
                vrele(tdvp);
            } else {
                vput(tdvp);
            }
            if let Some(tvp) = tvp {
                vput(tvp);
            }
            let _ = VOP_ABORTOP(fdvp, &mut fromnd.ni_cnd);
            vrele(fdvp);
            vrele(fvp);
        }
        if let Some(sd) = tond.ni_startdir {
            vrele(sd);
        }
        if let Some(buf) = NonNull::new(tond.ni_cnd.cn_pnbuf) {
            pool_put(&NAMEI_POOL, buf);
        }
        error
    };
    // out1:
    if let Some(sd) = fromnd.ni_startdir {
        vrele(sd);
    }
    if let Some(buf) = NonNull::new(fromnd.ni_cnd.cn_pnbuf) {
        pool_put(&NAMEI_POOL, buf);
    }
    error
}

/// Make a directory file.
pub fn sys_mkdir(p: &Proc, v: &SysArgs, _retval: &mut [Register; 2]) -> Result<(), Errno> {
    let uap: &SysMkdirArgs = sysargs(v);

    domkdirat(p, AT_FDCWD, uap.path.get(), uap.mode.get())
}

/// `mkdirat(fd, path, mode)`.
pub fn sys_mkdirat(p: &Proc, v: &SysArgs, _retval: &mut [Register; 2]) -> Result<(), Errno> {
    let uap: &SysMkdiratArgs = sysargs(v);

    domkdirat(p, uap.fd.get(), uap.path.get(), uap.mode.get())
}

/// `domkdirat(p, fd, path, mode)`.
pub fn domkdirat(p: &Proc, fd: i32, path: *const u8, mode: Mode) -> Result<(), Errno> {
    let mut vattr = Vattr::new();

    let mut nd = ndinitat(CREATE, LOCKPARENT | STRIPSLASHES, fd, upath(path), p);
    nd.ni_pledge = PLEDGE_CPATH;
    nd.ni_unveil = UNVEIL_CREATE;
    namei(&mut nd)?;
    let dvp = ndvp(nd.ni_dvp);
    if let Some(vp) = nd.ni_vp {
        let _ = VOP_ABORTOP(dvp, &mut nd.ni_cnd);
        if ptr::eq(dvp, vp) {
            vrele(dvp);
        } else {
            vput(dvp);
        }
        vrele(vp);
        return Err(Errno::EEXIST);
    }
    vattr_null(&mut vattr);
    vattr.va_type = VDIR;
    vattr.va_mode = (mode & ACCESSPERMS) & !p.fd().fd_cmask.get();
    let error = VOP_MKDIR(dvp, &mut nd.ni_vp, &mut nd.ni_cnd, &mut vattr);
    if error.is_ok()
        && let Some(vp) = nd.ni_vp
    {
        vput(vp);
    }
    error
}

/// Remove a directory file.
pub fn sys_rmdir(p: &Proc, v: &SysArgs, _retval: &mut [Register; 2]) -> Result<(), Errno> {
    let uap: &SysRmdirArgs = sysargs(v);

    dounlinkat(p, AT_FDCWD, uap.path.get(), AT_REMOVEDIR)
}

/// Read a block of directory entries in a file system independent format.
pub fn sys_getdents(p: &Proc, v: &SysArgs, retval: &mut [Register; 2]) -> Result<(), Errno> {
    let uap: &SysGetdentsArgs = sysargs(v);
    let mut eofflag = 0;

    let buflen = uap.buflen.get();

    if buflen > INT_MAX as usize {
        return Err(Errno::EINVAL);
    }
    let fp = getvnode(p, uap.fd.get())?;
    let error = 'bad: {
        if fp.flag() & FREAD == 0 {
            break 'bad Err(Errno::EBADF);
        }
        let vp = fp.vnode();
        if vp.v_type.get() != VDIR {
            break 'bad Err(Errno::EINVAL);
        }

        let _ = vn_lock(vp, LK_EXCLUSIVE | LK_RETRY);

        if fp.f_offset.get() < 0 {
            let _ = VOP_UNLOCK(vp);
            break 'bad Err(Errno::EINVAL);
        }

        let mut aiov = [Iovec {
            iov_base: uap.buf.get(),
            iov_len: buflen,
        }];
        let mut auio = Uio {
            uio_iov: &mut aiov,
            uio_offset: fp.f_offset.get(),
            uio_resid: buflen,
            uio_segflg: UioSeg::UIO_USERSPACE,
            uio_rw: UioRw::UIO_READ,
            uio_procp: Some(p),
        };
        let error = VOP_READDIR(vp, &mut auio, fp.f_cred.get(), &mut eofflag);
        crate::kern::kern_lock::mtx_enter(&fp.f_mtx);
        fp.f_offset.set(auio.uio_offset);
        crate::kern::kern_lock::mtx_leave(&fp.f_mtx);
        let _ = VOP_UNLOCK(vp);
        if let Err(e) = error {
            break 'bad Err(e);
        }
        retval[0] = (buflen - auio.uio_resid) as Register;
        Ok(())
    };
    let _ = frele(fp, p);
    error
}

/// Set the mode mask for creation of filesystem nodes.
pub fn sys_umask(p: &Proc, v: &SysArgs, retval: &mut [Register; 2]) -> Result<(), Errno> {
    let uap: &SysUmaskArgs = sysargs(v);
    let fdp = p.fd();

    fdplock(fdp);
    retval[0] = fdp.fd_cmask.get() as Register;
    fdp.fd_cmask.set(uap.newmask.get() & ACCESSPERMS);
    fdpunlock(fdp);
    Ok(())
}

/// Void all references to file by ripping underlying filesystem away from vnode.
pub fn sys_revoke(p: &Proc, v: &SysArgs, _retval: &mut [Register; 2]) -> Result<(), Errno> {
    let uap: &SysRevokeArgs = sysargs(v);
    let mut vattr = Vattr::new();

    let mut nd = ndinit(LOOKUP, FOLLOW, upath(uap.path.get()), p);
    nd.ni_pledge = PLEDGE_RPATH | PLEDGE_TTY;
    nd.ni_unveil = UNVEIL_READ;
    namei(&mut nd)?;
    let vp = ndvp(nd.ni_vp);
    let error = 'out: {
        if vp.v_type.get() != VCHR
            || major(vp.v_rdev()) >= nchrdev()
            || cdevsw(major(vp.v_rdev())).d_type != D_TTY
        {
            break 'out Err(Errno::ENOTTY);
        }
        if let Err(e) = VOP_GETATTR(vp, &mut vattr, p.p_ucred.get(), p) {
            break 'out Err(e);
        }
        if p.ucred().cr_uid.get() != vattr.va_uid
            && let Err(e) = suser(p)
        {
            break 'out Err(e);
        }
        if vp.v_usecount.get() > 1 || vp.v_flag.get() & VALIASED != 0 {
            let _ = VOP_REVOKE(vp, REVOKEALL);
        }
        Ok(())
    };
    vrele(vp);
    error
}

/// Convert a user file descriptor to a kernel file entry. The file is returned with a
/// reference (`FREF`).
pub fn getvnode(p: &Proc, fd: i32) -> Result<&'static File, Errno> {
    let Some(fp) = fd_getfile(p.fd(), fd) else {
        return Err(Errno::EBADF);
    };

    if fp.f_type.get() != DTYPE_VNODE {
        let _ = frele(fp, p);
        return Err(Errno::EINVAL);
    }

    if fp.vnode().v_type.get() == VBAD {
        let _ = frele(fp, p);
        return Err(Errno::EBADF);
    }

    Ok(fp)
}

/// Positional read system call.
pub fn sys_pread(p: &Proc, v: &SysArgs, retval: &mut [Register; 2]) -> Result<(), Errno> {
    let uap: &SysPreadArgs = sysargs(v);

    let mut iov = [Iovec {
        iov_base: uap.buf.get(),
        iov_len: uap.nbyte.get(),
    }];
    if iov[0].iov_len > crate::sys::limits::SSIZE_MAX as usize {
        return Err(Errno::EINVAL);
    }
    let resid = iov[0].iov_len;

    let mut auio = Uio {
        uio_iov: &mut iov,
        uio_offset: uap.offset.get(),
        uio_resid: resid,
        uio_segflg: UioSeg::UIO_USERSPACE,
        uio_rw: UioRw::UIO_READ,
        uio_procp: Some(p),
    };

    dofilereadv(p, uap.fd.get(), &mut auio, FO_POSITION, retval)
}

/// Positional scatter read system call.
pub fn sys_preadv(p: &Proc, v: &SysArgs, retval: &mut [Register; 2]) -> Result<(), Errno> {
    let uap: &SysPreadvArgs = sysargs(v);
    let iovcnt = uap.iovcnt.get() as u32;
    let mut aiov = [Iovec::new(); UIO_SMALLIOV];

    let (iov, resid) =
        crate::kern::sys_generic::iovec_copyin(uap.iovp.get() as usize, &mut aiov, iovcnt)?;

    let mut auio = Uio {
        uio_iov: &mut *iov,
        uio_offset: uap.offset.get(),
        uio_resid: resid,
        uio_segflg: UioSeg::UIO_USERSPACE,
        uio_rw: UioRw::UIO_READ,
        uio_procp: Some(p),
    };

    let error = dofilereadv(p, uap.fd.get(), &mut auio, FO_POSITION, retval);
    // SAFETY: `iov` came from `iovec_copyin` with `iovcnt`; the uio borrowing it is gone.
    unsafe { crate::kern::sys_generic::iovec_free(iov, iovcnt) };
    error
}

/// Positional write system call.
pub fn sys_pwrite(p: &Proc, v: &SysArgs, retval: &mut [Register; 2]) -> Result<(), Errno> {
    let uap: &SysPwriteArgs = sysargs(v);

    let mut iov = [Iovec {
        iov_base: uap.buf.get().cast_mut(),
        iov_len: uap.nbyte.get(),
    }];
    if iov[0].iov_len > crate::sys::limits::SSIZE_MAX as usize {
        return Err(Errno::EINVAL);
    }
    let resid = iov[0].iov_len;

    let mut auio = Uio {
        uio_iov: &mut iov,
        uio_offset: uap.offset.get(),
        uio_resid: resid,
        uio_segflg: UioSeg::UIO_USERSPACE,
        uio_rw: UioRw::UIO_WRITE,
        uio_procp: Some(p),
    };

    dofilewritev(p, uap.fd.get(), &mut auio, FO_POSITION, retval)
}

/// Positional gather write system call.
pub fn sys_pwritev(p: &Proc, v: &SysArgs, retval: &mut [Register; 2]) -> Result<(), Errno> {
    let uap: &SysPwritevArgs = sysargs(v);
    let iovcnt = uap.iovcnt.get() as u32;
    let mut aiov = [Iovec::new(); UIO_SMALLIOV];

    let (iov, resid) =
        crate::kern::sys_generic::iovec_copyin(uap.iovp.get() as usize, &mut aiov, iovcnt)?;

    let mut auio = Uio {
        uio_iov: &mut *iov,
        uio_offset: uap.offset.get(),
        uio_resid: resid,
        uio_segflg: UioSeg::UIO_USERSPACE,
        uio_rw: UioRw::UIO_WRITE,
        uio_procp: Some(p),
    };

    let error = dofilewritev(p, uap.fd.get(), &mut auio, FO_POSITION, retval);
    // SAFETY: `iov` came from `iovec_copyin` with `iovcnt`; the uio borrowing it is gone.
    unsafe { crate::kern::sys_generic::iovec_free(iov, iovcnt) };
    error
}
