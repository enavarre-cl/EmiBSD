/*	$OpenBSD: mfs_vfsops.c,v 1.63 2024/10/17 09:11:35 claudio Exp $	*/
/*	$NetBSD: mfs_vfsops.c,v 1.10 1996/02/09 22:31:28 christos Exp $	*/
/* <LICENSES> */
/*
 * Copyright (c) 1989, 1990, 1993, 1994
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
 *	@(#)mfs_vfsops.c	8.4 (Berkeley) 4/16/94
 */
/* </LICENSES> */

//! The memory based file system's file-system-type operations: `mfs_vfsops`, `mfs_mount`
//! (an FFS in the memory of the mounting process, on a made-up block device vnode), `mfs_start`
//! (the process stays in the kernel serving the file system's I/O), `mfs_checkexp` and
//! `mfs_init`.
//!
//! Upstream: sys/ufs/mfs/mfs_vfsops.c @ 3ce1f3f79392
//!
//! `mount_mfs(8)` (`newfs -DMFS`) builds the file system in its own memory, forks, and the
//! child calls `mount(2)` with `MOUNT_MFS` and `{base, size}`; `sys_mount` calls `VFS_START`
//! last, so the child sleeps in [`mfs_start`] until the file system is unmounted.
//!
//! ## Deviations
//! - `mfs_minor` is an atomic; `mfs_args` is [`MfsArgs`] (`sys/mount.rs`), read from the
//!   kernel copy of the arguments.
//! - `mfs_mount` copies the mounted-on name and the "mounted from" names with local helpers
//!   (`strlcpy` into the super-block's `fs_fsmnt` and the `MNAMELEN` arrays of `mnt_stat`),
//!   as `ffs_mount` does. A NULL `data` in a new mount answers `EINVAL` where the C
//!   dereferences it.
//! - `EXPORTMFS` (the update that only changes the export list) is not configured: an update
//!   with no device name does nothing, as in the C without it.
//! - `mfs_start`'s `p_siglist` update is `fetch_and` of the signal's mask on the thread's
//!   atomic list; a `cursig` of 0 (woken without a signal) clears nothing.
//! - `mfs_start` returns `Ok(())` for a shutdown, as the C returns 0.

use core::ptr;
use core::sync::atomic::{AtomicU32, Ordering};

use crate::kern::kern_bufq::{bufq_dequeue, bufq_init};
use crate::kern::kern_malloc::malloc;
use crate::kern::kern_sig::cursig;
use crate::kern::kern_synch::{tsleep_nsec, wakeup};
use crate::kern::subr_prf::panic;
use crate::kern::vfs_subr::{checkalias, getnewvnode, vfs_busy, vrele};
use crate::kern::vfs_syscalls::dounmount;
use crate::machine::copy::copyinstr;
use crate::sys::buf::BUFQ_FIFO;
use crate::sys::errno::Errno;
use crate::sys::malloc::{M_MFSNODE, M_WAITOK, M_ZERO};
use crate::sys::mbuf::Mbuf;
use crate::sys::mount::{
    MNAMELEN, MNT_FORCE, MNT_RDONLY, MNT_UPDATE, MNT_WANTRDWR, MfsArgs, Mount, VB_NOWAIT, VB_WRITE,
    Vfsconf, Vfsops,
};
use crate::sys::namei::Nameidata;
use crate::sys::param::{PCATCH, PWAIT};
use crate::sys::proc::Proc;
use crate::sys::signal::{SIGKILL, sigmask};
use crate::sys::signalvar::Sigctx;
use crate::sys::systm::INFSLP;
use crate::sys::types::makedev;
use crate::sys::ucred::Ucred;
use crate::sys::vnode::{FORCECLOSE, VBLK, VT_MFS, WRITECLOSE};
use crate::ufs::ffs::ffs_vfsops::{
    ffs_fhtovp, ffs_flushfiles, ffs_init, ffs_mountfs, ffs_statfs, ffs_sync, ffs_sysctl,
    ffs_unmount, ffs_vget, ffs_vptofh,
};
use crate::ufs::ffs::fs::{Fs, MAXMNTLEN};
use crate::ufs::mfs::mfs_vnops::{MFS_VOPS, mfs_doio};
use crate::ufs::mfs::mfsnode::{Mfsnode, vtomfs};
use crate::ufs::ufs::quota::ufs_quotactl;
use crate::ufs::ufs::ufs_vfsops::ufs_root;
use crate::ufs::ufs::ufsmount::vfstoufs;

/// `mfs_minor`: used for building internal dev_t.
static MFS_MINOR: AtomicU32 = AtomicU32::new(0);

/// `mfs_vfsops`: mfs vfs operations.
pub static MFS_VFSOPS: Vfsops = Vfsops {
    vfs_mount: mfs_mount,
    vfs_start: mfs_start,
    vfs_unmount: ffs_unmount,
    vfs_root: ufs_root,
    vfs_quotactl: ufs_quotactl,
    vfs_statfs: ffs_statfs,
    vfs_sync: ffs_sync,
    vfs_vget: ffs_vget,
    vfs_fhtovp: ffs_fhtovp,
    vfs_vptofh: ffs_vptofh,
    vfs_init: Some(mfs_init),
    vfs_sysctl: Some(ffs_sysctl),
    vfs_checkexp: mfs_checkexp,
};

/// `memset(fs->fs_fsmnt, 0, sizeof(fs->fs_fsmnt)); strlcpy(fs->fs_fsmnt, name, ...)`.
fn fs_strlcpy_fsmnt(fs: &Fs, name: &[u8]) {
    let mut m = [0u8; MAXMNTLEN];
    let name = name.split(|&c| c == 0).next().unwrap_or(&[]);
    let n = name.len().min(m.len() - 1);
    m[..n].copy_from_slice(&name[..n]);
    fs.fs_fsmnt.set(m);
}

/// `memset(dst, 0, MNAMELEN); strlcpy(dst, src, MNAMELEN)`.
fn mname_copy(dst: &mut [u8; MNAMELEN], src: &[u8]) {
    *dst = [0; MNAMELEN];
    let src = src.split(|&c| c == 0).next().unwrap_or(&[]);
    let n = src.len().min(MNAMELEN - 1);
    dst[..n].copy_from_slice(&src[..n]);
}

/// `mfs_mount` (`vfs_mount`): mount system call. `data` is the kernel copy of the user's
/// `struct mfs_args`.
pub fn mfs_mount(
    mp: &'static Mount,
    path: &[u8],
    data: &mut [u8],
    _ndp: &mut Nameidata<'_>,
    p: &Proc,
) -> Result<(), Errno> {
    let args = MfsArgs::from_bytes(data);

    // If updating, check whether changing from read-only to read/write; if there is no device
    // name, that's all we do.
    if mp.mnt_flag.get() & MNT_UPDATE != 0 {
        let ump = vfstoufs(mp);
        let fs = ump.fs();
        if fs.fs_ronly.get() == 0 && mp.mnt_flag.get() & MNT_RDONLY != 0 {
            let mut flags = WRITECLOSE;
            if mp.mnt_flag.get() & MNT_FORCE != 0 {
                flags |= FORCECLOSE;
            }
            ffs_flushfiles(mp, flags, p)?;
        }
        if fs.fs_ronly.get() != 0 && mp.mnt_flag.get() & MNT_WANTRDWR != 0 {
            fs.fs_ronly.set(0);
        }
        // EXPORTMFS: not configured.
        return Ok(());
    }
    let Some(args) = args else {
        return Err(Errno::EINVAL);
    };
    let mut fspec = [0u8; MNAMELEN];
    copyinstr(args.fspec, &mut fspec)?;
    let devvp = getnewvnode(VT_MFS, None, &MFS_VOPS)?;
    devvp.v_type.set(VBLK);
    let minor = MFS_MINOR.load(Ordering::Relaxed);
    if checkalias(devvp, makedev(255, minor), None).is_some() {
        panic(format_args!("mfs_mount: dup dev"));
    }
    MFS_MINOR.store(minor.wrapping_add(1), Ordering::Relaxed);
    let Some(mem) = malloc(size_of::<Mfsnode>(), M_MFSNODE, M_WAITOK | M_ZERO) else {
        panic(format_args!("mfs_mount: no memory"));
    };
    let mem = mem.cast::<Mfsnode>();
    // SAFETY: a fresh allocation of an `Mfsnode`'s size from malloc, aligned for it.
    unsafe { ptr::write(mem.as_ptr(), Mfsnode::new()) };
    // SAFETY: just initialised; freed only by `mfs_reclaim`, which clears `v_data`.
    let mfsp: &'static Mfsnode = unsafe { &*mem.as_ptr() };
    devvp.v_data.set(mem.as_ptr().cast());
    mfsp.mfs_baseoff.set(args.base);
    mfsp.mfs_size.set(args.size as i64);
    mfsp.mfs_vnode.set(Some(devvp));
    mfsp.mfs_tid.set(p.p_tid.get());
    let _ = bufq_init(&mfsp.mfs_bufq, BUFQ_FIFO);
    if let Err(e) = ffs_mountfs(devvp, mp, p) {
        mfsp.mfs_shutdown.store(1, Ordering::Relaxed);
        vrele(devvp);
        return Err(e);
    }
    let ump = vfstoufs(mp);
    let fs = ump.fs();

    fs_strlcpy_fsmnt(fs, path);
    let fsmnt = fs.fs_fsmnt.get();
    mp.update_stat(|sp| {
        sp.f_mntonname.copy_from_slice(&fsmnt[..MNAMELEN]);
        mname_copy(&mut sp.f_mntfromname, &fspec);
        mname_copy(&mut sp.f_mntfromspec, &fspec);
        sp.mount_info.__align[..MfsArgs::SIZE].copy_from_slice(&data[..MfsArgs::SIZE]);
    });

    Ok(())
}

/// `mfs_start` (`vfs_start`): used to grab the process and keep it in the kernel to service
/// memory filesystem I/O requests.
///
/// Loop servicing I/O requests. Copy the requested data into or out of the memory filesystem
/// address space.
pub fn mfs_start(mp: &'static Mount, _flags: i32, p: &Proc) -> Result<(), Errno> {
    let vp = vfstoufs(mp).devvp();
    let mfsp = vtomfs(vp);
    let mut sleepreturn: Result<(), Errno> = Ok(());

    loop {
        loop {
            if mfsp.mfs_shutdown.load(Ordering::Relaxed) == 1 {
                break;
            }
            let Some(bp) = bufq_dequeue(&mfsp.mfs_bufq) else {
                break;
            };
            mfs_doio(mfsp, bp);
            wakeup(ptr::from_ref(bp));
        }
        if mfsp.mfs_shutdown.load(Ordering::Relaxed) == 1 {
            break;
        }

        // If a non-ignored signal is received, try to unmount. If that fails, clear the signal
        // (it has been "processed"), otherwise we will loop here, as tsleep will always return
        // EINTR/ERESTART.
        if sleepreturn.is_err() {
            let mut ctx = Sigctx::default();
            let sig = cursig(p, &mut ctx, false);
            let failed = vfs_busy(mp, VB_WRITE | VB_NOWAIT).is_err()
                || dounmount(mp, if sig == SIGKILL { MNT_FORCE } else { 0 }, p).is_err();
            if failed && sig != 0 {
                p.p_siglist.fetch_and(!sigmask(sig), Ordering::Relaxed);
            }
            sleepreturn = Ok(());
            continue;
        }
        sleepreturn = tsleep_nsec(ptr::from_ref(vp), PWAIT | PCATCH, "mfsidl", INFSLP);
    }
    Ok(())
}

/// `mfs_checkexp` (`vfs_checkexp`): check export permission, not supported.
pub fn mfs_checkexp(
    _mp: &'static Mount,
    _nam: &Mbuf,
    _exflagsp: &mut i32,
    _credanonp: &mut *const Ucred,
) -> Result<(), Errno> {
    Err(Errno::EOPNOTSUPP)
}

/// `mfs_init` (`vfs_init`): memory based filesystem initialization.
pub fn mfs_init(vfsp: &'static Vfsconf) -> Result<(), Errno> {
    ffs_init(vfsp)
}

#[cfg(test)]
mod tests;
