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
 * Copyright (c) 1982, 1986, 1989, 1991, 1993
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
 *	@(#)kern_descrip.c	8.6 (Berkeley) 4/19/94
 */
/* </LICENSES> */

/* <CODE> */
//! Descriptor management: `kern/kern_descrip.c`. The per-process descriptor tables
//! (`fdinit`, `fdcopy`, `fdshare`, `fdfree`), descriptor allocation over the two-level free
//! bitmap (`fdalloc`, `fdexpand`), the open file structures (`falloc`, `fnew`, `closef`,
//! `fdrop`, the global `filehead` list) and the descriptor system calls (`dup`, `dup2`,
//! `dup3`, `fcntl`, `close`, `fstat`, `fpathconf`, `flock`, `closefrom`,
//! `getdtablecount`).
//!
//! Upstream: sys/kern/kern_descrip.c @ 3ce1f3f79392
//!
//! The functions keep the C's locking: `fd_lock` (`fdplock`) serialises the table's
//! writers, `fd_fplock` lets `fd_getfile` read a slot without it, and `fhdlk` guards
//! `filehead`. Several functions return with the table unlocked as in C (`fdrelease`,
//! `finishdup`); their comments say so.
//!
//! ## Deviations
//! - Out-parameters are return values: `fdalloc(p, want)` returns the descriptor,
//!   `falloc(p)` the file and the descriptor, `fd_getfile` an `Option<&'static File>` (the
//!   C's NULL is `None`). `closef` takes a file (the C's NULL early return has no caller
//!   here).
//! - `fdexpand` returns `ENOMEM` when `malloc(9)` cannot serve it: `M_WAITOK` cannot sleep
//!   in this kernel yet (`kern_malloc.rs`), and failing the call that needed the descriptor
//!   is better than a panic. Its callers pass the error up. `fnew` likewise fails (`ENFILE`)
//!   when `file_pool` is empty; `fdinit` and `fdcopy`, whose callers cannot fail, panic.
//! - `fdalloc` reads the limit (`lim_cur(RLIMIT_NOFILE)`, `maxfiles`) once and hands the
//!   search to `fdalloc_search`, which the host tests drive without a `curproc`; the C
//!   re-reads the limit when it restarts the search, with nothing in between that could
//!   change it.
//! - `find_next_zero` reads the words past the end of a bitmap as full, where the C reads
//!   past the array; the C rejects whatever it finds there (`i < last`), so the result is
//!   the same.
//! - The vnode paths (`VOP_ADVLOCK` of the record locks, `flock` and `closef`,
//!   `VOP_PATHCONF`, `VISTTY` for `F_ISATTY`, `vref`/`vrele` of `fd_cdir`/`fd_rdir`) are
//!   the vfs core's (`vfs_vops.rs`, `vfs_subr.rs`). `F_ISATTY` answers 0/`ENOTTY` for every
//!   file that is not a vnode, as in C, so the console stand-in is not a tty to `isatty(3)`.
//! - `KTRACE` is not configured. `KASSERTMSG` is `kassert!`.

use core::ffi::c_void;
use core::ptr::{self, NonNull};
use core::sync::atomic::{AtomicI32, Ordering};

use crate::conf::param::MAXFILES;
use crate::kassert;
use crate::kern::kern_event::knote_fdclose;
use crate::kern::kern_lock::{mtx_enter, mtx_init, mtx_leave};
use crate::kern::kern_malloc::{free, mallocarray};
use crate::kern::kern_pledge::{pledge_fcntl, pledge_flock};
use crate::kern::kern_prot::{crfree, crhold, suser};
use crate::kern::kern_rwlock::rw_init;
use crate::kern::subr_pool::{pool_get, pool_init, pool_put};
use crate::kern::subr_prf::{panic, tablefull};
use crate::kern::vfs_subr::{vfs_stall_barrier, vref, vrele};
use crate::kern::vfs_vnops::vn_lock;
use crate::kern::vfs_vops::{VOP_ADVLOCK, VOP_PATHCONF, VOP_UNLOCK};
use crate::machine::copy::{copyin, copyout};
use crate::machine::cpu::curproc;
use crate::machine::intr::{IPL_MPFLOOR, IPL_NONE};
use crate::sys::errno::Errno;
use crate::sys::fcntl::{
    F_DUPFD, F_DUPFD_CLOEXEC, F_DUPFD_CLOFORK, F_FLOCK, F_GETFD, F_GETFL, F_GETLK, F_GETOWN,
    F_ISATTY, F_POSIX, F_RDLCK, F_SETFD, F_SETFL, F_SETLK, F_SETLKW, F_SETOWN, F_UNLCK, F_WAIT,
    F_WRLCK, FASYNC, FCNTLFLAGS, FD_CLOEXEC, FD_CLOFORK, FREAD, FWRITE, Flock, LOCK_EX, LOCK_NB,
    LOCK_SH, LOCK_UN, O_CLOEXEC, O_CLOFORK, fflags, oflags,
};
use crate::sys::file::{
    DTYPE_KQUEUE, DTYPE_PIPE, DTYPE_SOCKET, DTYPE_VNODE, FDUP_MAX_COUNT, FIF_HASLOCK, FIF_INSERTED,
    File, FileList, foffset, fref, frele,
};
use crate::sys::filedesc::{
    FD_ADVLOCK, Filedesc, Filedesc0, NDENTRIES, NDENTRYMASK, NDENTRYSHIFT, NDEXTENT, NDFILE,
    OFILESIZE, UF_EXCLOSE, UF_FORKCLOSE, UF_PLEDGED, UF_PLEDGEOPEN, fdpassertlocked, fdplock,
    fdpunlock, ndhislots, ndloslots,
};
use crate::sys::filio::{FIOASYNC, FIOGETOWN, FIOSETOWN};
use crate::sys::lock::{LK_EXCLUSIVE, LK_RETRY};
use crate::sys::malloc::{M_FILEDESC, M_WAITOK, M_ZERO};
use crate::sys::pool::{PR_WAITOK, PR_ZERO, Pool};
use crate::sys::proc::{PS_PLEDGE, PS_SUGID, PS_SUGIDEXEC, Proc, Process};
use crate::sys::queue::ListHead;
use crate::sys::resource::RLIMIT_NOFILE;
use crate::sys::resourcevar::lim_cur;
use crate::sys::stat::{S_IWGRP, S_IWOTH, Stat};
use crate::sys::syscallargs::{
    SysCloseArgs, SysClosefromArgs, SysDup2Args, SysDup3Args, SysDupArgs, SysFcntlArgs,
    SysFlockArgs, SysFpathconfArgs, SysFstatArgs,
};
use crate::sys::syslimits::PIPE_BUF;
use crate::sys::systm::{SysArgs, sysargs};
use crate::sys::types::{Dev, Register, minor};
use crate::sys::unistd::{_PC_PIPE_BUF, SEEK_CUR, SEEK_SET};
use crate::sys::vnode::VISTTY;

/// `DUPF_CLOEXEC`.
const DUPF_CLOEXEC: i32 = 0x01;
/// `DUPF_DUP2`.
const DUPF_DUP2: i32 = 0x02;
/// `DUPF_CLOFORK`.
const DUPF_CLOFORK: i32 = 0x04;

/// `struct filelist filehead`'s type: the list made `Sync`, its lock being `fhdlk`.
pub struct FileListHead(pub ListHead<FileList>);
// SAFETY: the list is only read or changed with `FHDLK` held.
unsafe impl Sync for FileListHead {}

/// `fhdlk`: protects `filehead`. Interrupts are blocked as long as it is held, with and
/// without the kernel lock.
pub static FHDLK: crate::sys::mutex::Mutex = crate::sys::mutex::Mutex::new(IPL_MPFLOOR);
/// `filehead`: head of list of open files.
pub static FILEHEAD: FileListHead = FileListHead(ListHead::new());
/// `numfiles`: actual number of open files.
pub static NUMFILES: AtomicI32 = AtomicI32::new(0);

/// `file_pool`.
pub static FILE_POOL: Pool = Pool::new();
/// `fdesc_pool`.
pub static FDESC_POOL: Pool = Pool::new();

/// `filedesc_init`: the pools of open files and descriptor tables.
pub fn filedesc_init() {
    pool_init(
        &FILE_POOL,
        size_of::<File>(),
        0,
        IPL_MPFLOOR,
        PR_WAITOK,
        "filepl",
        None,
    );
    pool_init(
        &FDESC_POOL,
        size_of::<Filedesc0>(),
        0,
        IPL_NONE,
        PR_WAITOK,
        "fdescpl",
        None,
    );
    FILEHEAD.0.init();
}

/// `find_next_zero(bitmap, want, bits)`: the first clear bit at or after bit `want` in the
/// first `NDLOSLOTS(bits)` words of `bitmap`, or -1.
fn find_next_zero(bitmap: &dyn Fn(usize) -> u32, want: i32, bits: u32) -> i32 {
    if want as u32 > bits {
        return -1;
    }

    let mut off = (want as u32 >> NDENTRYSHIFT) as usize;
    let i = want as u32 & NDENTRYMASK as u32;
    let sub = 'found: {
        if i != 0 {
            let sub = bitmap(off) | (!0u32 >> (NDENTRIES as u32 - i));
            if sub != !0 {
                break 'found sub;
            }
            off += 1;
        }

        let maxoff = ndloslots(bits as usize);
        while off < maxoff {
            let sub = bitmap(off);
            if sub != !0 {
                break 'found sub;
            }
            off += 1;
        }

        return -1;
    };

    ((off << NDENTRYSHIFT) as u32 + (!sub).trailing_zeros()) as i32
}

/// `fdp->fd_himap[off]`, a word past the end reading as full (see the deviations).
fn himap_or_full(fdp: &Filedesc, off: usize) -> u32 {
    if off < ndhislots(fdp.nfiles()) {
        fdp.himap(off)
    } else {
        !0
    }
}

/// `fdp->fd_lomap[off]`, a word past the end reading as full (see the deviations).
fn lomap_or_full(fdp: &Filedesc, off: usize) -> u32 {
    if off < ndloslots(fdp.nfiles()) {
        fdp.lomap(off)
    } else {
        !0
    }
}

/// `find_last_set(fd, last)`: the highest descriptor below `last` that is in use, or 0.
pub fn find_last_set(fd: &Filedesc, last: i32) -> i32 {
    let mut off = (last - 1) >> NDENTRYSHIFT;

    while off >= 0 && fd.lomap(off as usize) == 0 {
        off -= 1;
    }
    if off < 0 {
        return 0;
    }

    let mut i = ((off + 1) << NDENTRYSHIFT) - 1;
    if i >= last {
        i = last - 1;
    }

    while i > 0 && !fd_inuse(fd, i) {
        i -= 1;
    }
    i
}

/// `fd_inuse(fdp, fd)`: whether descriptor `fd` is allocated.
fn fd_inuse(fdp: &Filedesc, fd: i32) -> bool {
    let off = (fd as u32 >> NDENTRYSHIFT) as usize;

    fdp.lomap(off) & (1u32 << (fd as u32 & NDENTRYMASK as u32)) != 0
}

/// `fd_used(fdp, fd)`: marks descriptor `fd` allocated.
pub(crate) fn fd_used(fdp: &Filedesc, fd: i32) {
    let off = (fd as u32 >> NDENTRYSHIFT) as usize;

    fdp.set_lomap(
        off,
        fdp.lomap(off) | 1u32 << (fd as u32 & NDENTRYMASK as u32),
    );
    if fdp.lomap(off) == !0 {
        let hi = off >> NDENTRYSHIFT;
        fdp.set_himap(hi, fdp.himap(hi) | 1u32 << (off & NDENTRYMASK));
    }

    if fd > fdp.fd_lastfile.get() {
        fdp.fd_lastfile.set(fd);
    }
    fdp.fd_openfd
        .store(fdp.fd_openfd.load(Ordering::Relaxed) + 1, Ordering::Relaxed);
}

/// `fd_unused(fdp, fd)`: marks descriptor `fd` free.
fn fd_unused(fdp: &Filedesc, fd: i32) {
    let off = (fd as u32 >> NDENTRYSHIFT) as usize;

    if fd < fdp.fd_freefile.get() {
        fdp.fd_freefile.set(fd);
    }

    if fdp.lomap(off) == !0 {
        let hi = off >> NDENTRYSHIFT;
        fdp.set_himap(hi, fdp.himap(hi) & !(1u32 << (off & NDENTRYMASK)));
    }
    fdp.set_lomap(
        off,
        fdp.lomap(off) & !(1u32 << (fd as u32 & NDENTRYMASK as u32)),
    );

    #[cfg(feature = "diagnostic")]
    if fd > fdp.fd_lastfile.get() {
        panic(format_args!("fd_unused: fd_lastfile inconsistent"));
    }
    if fd == fdp.fd_lastfile.get() {
        fdp.fd_lastfile.set(find_last_set(fdp, fd));
    }
    fdp.fd_openfd
        .store(fdp.fd_openfd.load(Ordering::Relaxed) - 1, Ordering::Relaxed);
}

/// `fd_iterfile(fp, p)`: the open file after `fp` (the first one when `None`) in `filehead`,
/// with a reference taken; the reference on `fp` is dropped.
pub fn fd_iterfile(fp: Option<&'static File>, p: &Proc) -> Option<&'static File> {
    mtx_enter(&FHDLK);
    let mut nfp = match fp {
        None => FILEHEAD.0.first(),
        Some(fp) => ListHead::<FileList>::next(fp),
    };

    // don't refcount when f_count == 0 to avoid race in fdrop()
    while let Some(n) = nfp {
        let count = n.f_count.load(Ordering::SeqCst);
        if count == 0 {
            nfp = ListHead::<FileList>::next(n);
            continue;
        }
        if n.f_count
            .compare_exchange(count, count + 1, Ordering::SeqCst, Ordering::SeqCst)
            .is_ok()
        {
            break;
        }
    }
    mtx_leave(&FHDLK);

    if let Some(fp) = fp {
        let _ = frele(fp, p);
    }

    nfp
}

/// `fd_getfile(fdp, fd)`: the file at descriptor `fd`, with a reference taken, or `None`.
pub fn fd_getfile(fdp: &Filedesc, fd: i32) -> Option<&'static File> {
    vfs_stall_barrier();

    if fd as u32 >= fdp.fd_nfiles.load(Ordering::Relaxed) as u32 {
        return None;
    }

    mtx_enter(&fdp.fd_fplock);
    let fp = fdp.ofile(fd as usize);
    if let Some(fp) = fp {
        fp.f_count.fetch_add(1, Ordering::SeqCst);
    }
    mtx_leave(&fdp.fd_fplock);

    fp
}

/// `fd_getfile_mode(fdp, fd, mode)`: as `fd_getfile`, `None` also when the file is not open
/// for `mode` (`FREAD`, `FWRITE`).
pub fn fd_getfile_mode(fdp: &Filedesc, fd: i32, mode: i32) -> Option<&'static File> {
    kassert!(mode != 0);

    let fp = fd_getfile(fdp, fd)?;

    if fp.flag() & mode == 0 {
        let _ = frele(fp, curproc());
        return None;
    }

    Some(fp)
}

/// `fd_checkclosed(fdp, fd, fp)`: whether descriptor `fd` no longer refers to `fp`.
pub fn fd_checkclosed(fdp: &Filedesc, fd: i32, fp: &File) -> bool {
    mtx_enter(&fdp.fd_fplock);
    kassert!(fd < fdp.fd_nfiles.load(Ordering::Relaxed));
    let closed = !fdp.ofile(fd as usize).is_some_and(|f| ptr::eq(f, fp));
    mtx_leave(&fdp.fd_fplock);
    closed
}

/*
 * System calls on descriptors.
 */

/// Duplicate a file descriptor.
pub fn sys_dup(p: &Proc, v: &SysArgs, retval: &mut [Register; 2]) -> Result<(), Errno> {
    let uap: &SysDupArgs = sysargs(v);
    let fdp = p.fd();
    let old = uap.fd.get();

    loop {
        let Some(fp) = fd_getfile(fdp, old) else {
            return Err(Errno::EBADF);
        };
        fdplock(fdp);
        match fdalloc(p, 0) {
            Err(Errno::ENOSPC) => {
                let expanded = fdexpand(p);
                fdpunlock(fdp);
                let _ = frele(fp, p);
                expanded?;
                // restart
            }
            Err(error) => {
                fdpunlock(fdp);
                let _ = frele(fp, p);
                return Err(error);
            }
            // No need for FRELE(), finishdup() uses current ref.
            Ok(new) => return finishdup(p, fp, old, new, retval, 0),
        }
    }
}

/// Duplicate a file descriptor to a particular value.
pub fn sys_dup2(p: &Proc, v: &SysArgs, retval: &mut [Register; 2]) -> Result<(), Errno> {
    let uap: &SysDup2Args = sysargs(v);

    dodup3(p, uap.from.get(), uap.to.get(), 0, retval)
}

/// `dup3(2)`: `dup2` with `O_CLOEXEC`/`O_CLOFORK`, refusing `from == to`.
pub fn sys_dup3(p: &Proc, v: &SysArgs, retval: &mut [Register; 2]) -> Result<(), Errno> {
    let uap: &SysDup3Args = sysargs(v);

    if uap.from.get() == uap.to.get() {
        return Err(Errno::EINVAL);
    }
    if uap.flags.get() & !(O_CLOEXEC | O_CLOFORK) != 0 {
        return Err(Errno::EINVAL);
    }
    dodup3(p, uap.from.get(), uap.to.get(), uap.flags.get(), retval)
}

/// `dodup3(p, old, new, flags, retval)`: the common part of `dup2` and `dup3`.
pub fn dodup3(
    p: &Proc,
    old: i32,
    new: i32,
    flags: i32,
    retval: &mut [Register; 2],
) -> Result<(), Errno> {
    let fdp = p.fd();

    loop {
        let Some(fp) = fd_getfile(fdp, old) else {
            return Err(Errno::EBADF);
        };
        if old == new {
            // NOTE! This doesn't clear the close-on-exec flag. This might or might not be
            // the intended behavior from the start, but this is what everyone else does.
            retval[0] = new as Register;
            let _ = frele(fp, p);
            return Ok(());
        }
        if u64::from(new as u32) >= lim_cur(RLIMIT_NOFILE)
            || new as u32 >= MAXFILES.load(Ordering::Relaxed) as u32
        {
            let _ = frele(fp, p);
            return Err(Errno::EBADF);
        }
        fdplock(fdp);
        if new >= fdp.fd_nfiles.load(Ordering::Relaxed) {
            match fdalloc(p, new) {
                Err(Errno::ENOSPC) => {
                    let expanded = fdexpand(p);
                    fdpunlock(fdp);
                    let _ = frele(fp, p);
                    expanded?;
                    continue; // restart
                }
                Err(error) => {
                    fdpunlock(fdp);
                    let _ = frele(fp, p);
                    return Err(error);
                }
                Ok(i) => {
                    if new != i {
                        panic(format_args!("dup2: fdalloc"));
                    }
                    fd_unused(fdp, new);
                }
            }
        }

        let mut dupflags = DUPF_DUP2;
        if flags & O_CLOEXEC != 0 {
            dupflags |= DUPF_CLOEXEC;
        }
        if flags & O_CLOFORK != 0 {
            dupflags |= DUPF_CLOFORK;
        }

        // No need for FRELE(), finishdup() uses current ref.
        return finishdup(p, fp, old, new, retval, dupflags);
    }
}

/// The `l_whence == SEEK_CUR` adjustment `fcntl`'s record locks make against the file
/// offset.
fn flock_seek_cur(fp: &File, fl: &mut Flock) {
    if i32::from(fl.l_whence) == SEEK_CUR {
        let offset = foffset(fp);

        if fl.l_start == 0 && fl.l_len < 0 {
            // lockf(3) compliance hack
            fl.l_len = -fl.l_len;
            fl.l_start = offset - fl.l_len;
        } else {
            fl.l_start += offset;
        }
    }
}

/// `fcntl` with an `int` argument handed to the file's ioctl: `(*fp->f_ops->fo_ioctl)(fp,
/// com, (caddr_t)&tmp, p)`. Returns the error and `tmp` as the ioctl left it.
fn fo_ioctl_int(fp: &File, com: u64, tmp: i32, p: &Proc) -> (Result<(), Errno>, i32) {
    let mut data = tmp.to_ne_bytes();
    let error = (fp.ops().fo_ioctl)(fp, com, &mut data, p);
    (error, i32::from_ne_bytes(data))
}

/// The file control system call.
pub fn sys_fcntl(p: &Proc, v: &SysArgs, retval: &mut [Register; 2]) -> Result<(), Errno> {
    let uap: &SysFcntlArgs = sysargs(v);
    let fd = uap.fd.get();
    let cmd = uap.cmd.get();
    let arg = uap.arg.get() as usize;
    let fdp = p.fd();
    let mut flg = F_POSIX;

    pledge_fcntl(p, cmd)?;

    'restart: loop {
        let Some(fp) = fd_getfile(fdp, fd) else {
            return Err(Errno::EBADF);
        };
        let error: Result<(), Errno> = 'out: {
            match cmd {
                F_DUPFD | F_DUPFD_CLOEXEC | F_DUPFD_CLOFORK => {
                    let newmin = arg as isize as i32;
                    if u64::from(newmin as u32) >= lim_cur(RLIMIT_NOFILE)
                        || newmin as u32 >= MAXFILES.load(Ordering::Relaxed) as u32
                    {
                        break 'out Err(Errno::EINVAL);
                    }
                    fdplock(fdp);
                    match fdalloc(p, newmin) {
                        Err(Errno::ENOSPC) => {
                            let expanded = fdexpand(p);
                            fdpunlock(fdp);
                            let _ = frele(fp, p);
                            expanded?;
                            continue 'restart;
                        }
                        Err(error) => {
                            fdpunlock(fdp);
                            let _ = frele(fp, p);
                            return Err(error);
                        }
                        Ok(i) => {
                            let mut dupflags = 0;
                            if cmd == F_DUPFD_CLOEXEC {
                                dupflags |= DUPF_CLOEXEC;
                            }
                            if cmd == F_DUPFD_CLOFORK {
                                dupflags |= DUPF_CLOFORK;
                            }

                            // No need for FRELE(), finishdup() uses current ref.
                            return finishdup(p, fp, fd, i, retval, dupflags);
                        }
                    }
                }

                F_GETFD => {
                    fdplock(fdp);
                    let flags = fdp.ofileflags(fd as usize);
                    retval[0] = (if flags & UF_EXCLOSE != 0 {
                        FD_CLOEXEC
                    } else {
                        0
                    } | if flags & UF_FORKCLOSE != 0 {
                        FD_CLOFORK
                    } else {
                        0
                    }) as Register;
                    fdpunlock(fdp);
                    Ok(())
                }

                F_SETFD => {
                    fdplock(fdp);
                    let arg = arg as isize;
                    let i = (if arg & FD_CLOEXEC as isize != 0 {
                        UF_EXCLOSE
                    } else {
                        0
                    }) | (if arg & FD_CLOFORK as isize != 0 {
                        UF_FORKCLOSE
                    } else {
                        0
                    });
                    fdp.set_ofileflags(
                        fd as usize,
                        (fdp.ofileflags(fd as usize) & !(UF_EXCLOSE | UF_FORKCLOSE)) | i,
                    );
                    fdpunlock(fdp);
                    Ok(())
                }

                F_GETFL => {
                    retval[0] = oflags(fp.flag()) as Register;
                    Ok(())
                }

                F_ISATTY => {
                    if fp.f_type.get() == DTYPE_VNODE && fp.vnode().v_flag.get() & VISTTY != 0 {
                        retval[0] = 1;
                        Ok(())
                    } else {
                        retval[0] = 0;
                        Err(Errno::ENOTTY)
                    }
                }

                F_SETFL => {
                    let mut prev = fp.f_flag.load(Ordering::SeqCst);
                    loop {
                        let tmp = (prev & !(FCNTLFLAGS as u32))
                            | (fflags(arg as isize as i32) & FCNTLFLAGS) as u32;
                        match fp.f_flag.compare_exchange(
                            prev,
                            tmp,
                            Ordering::SeqCst,
                            Ordering::SeqCst,
                        ) {
                            Ok(_) => break,
                            Err(cur) => prev = cur,
                        }
                    }
                    let tmp = fp.flag() & FASYNC;
                    fo_ioctl_int(fp, FIOASYNC, tmp, p).0
                }

                F_GETOWN => {
                    let (error, tmp) = fo_ioctl_int(fp, FIOGETOWN, 0, p);
                    retval[0] = tmp as Register;
                    error
                }

                F_SETOWN => fo_ioctl_int(fp, FIOSETOWN, arg as isize as i32, p).0,

                F_SETLKW | F_SETLK => {
                    if cmd == F_SETLKW {
                        flg |= F_WAIT;
                    }

                    if let Err(error) = pledge_flock(p) {
                        break 'out Err(error);
                    }

                    if fp.f_type.get() != DTYPE_VNODE {
                        break 'out Err(Errno::EINVAL);
                    }
                    let vp = fp.vnode();
                    // Copy in the lock structure
                    let mut bytes = [0u8; Flock::SIZE];
                    if let Err(e) = copyin(arg, &mut bytes) {
                        break 'out Err(e);
                    }
                    let mut fl = Flock::from_bytes(&bytes);
                    flock_seek_cur(fp, &mut fl);
                    let id = ptr::from_ref(fdp).cast::<c_void>();
                    let error = match fl.l_type {
                        F_RDLCK => {
                            if fp.flag() & FREAD == 0 {
                                break 'out Err(Errno::EBADF);
                            }
                            fdp.fd_flags.fetch_or(FD_ADVLOCK, Ordering::SeqCst);
                            VOP_ADVLOCK(vp, id, F_SETLK, &mut fl, flg)
                        }
                        F_WRLCK => {
                            if fp.flag() & FWRITE == 0 {
                                break 'out Err(Errno::EBADF);
                            }
                            fdp.fd_flags.fetch_or(FD_ADVLOCK, Ordering::SeqCst);
                            VOP_ADVLOCK(vp, id, F_SETLK, &mut fl, flg)
                        }
                        F_UNLCK => {
                            break 'out VOP_ADVLOCK(vp, id, i32::from(F_UNLCK), &mut fl, F_POSIX);
                        }
                        _ => break 'out Err(Errno::EINVAL),
                    };

                    if fd_checkclosed(fdp, fd, fp) {
                        // We have lost the race with close() or dup2(); unlock, pretend that
                        // we've won the race and that lock had been removed by close()
                        fl.l_whence = SEEK_SET as i16;
                        fl.l_start = 0;
                        fl.l_len = 0;
                        let _ = VOP_ADVLOCK(vp, id, i32::from(F_UNLCK), &mut fl, F_POSIX);
                        // fl.l_type = F_UNLCK: F_SETLK copies nothing out, so the C's
                        // assignment is never read.
                    }
                    error
                }

                F_GETLK => {
                    if let Err(error) = pledge_flock(p) {
                        break 'out Err(error);
                    }

                    if fp.f_type.get() != DTYPE_VNODE {
                        break 'out Err(Errno::EINVAL);
                    }
                    let vp = fp.vnode();
                    // Copy in the lock structure
                    let mut bytes = [0u8; Flock::SIZE];
                    if let Err(e) = copyin(arg, &mut bytes) {
                        break 'out Err(e);
                    }
                    let mut fl = Flock::from_bytes(&bytes);
                    flock_seek_cur(fp, &mut fl);
                    if fl.l_type != F_RDLCK
                        && fl.l_type != F_WRLCK
                        && fl.l_type != F_UNLCK
                        && fl.l_type != 0
                    {
                        break 'out Err(Errno::EINVAL);
                    }
                    let id = ptr::from_ref(fdp).cast::<c_void>();
                    if let Err(e) = VOP_ADVLOCK(vp, id, F_GETLK, &mut fl, F_POSIX) {
                        break 'out Err(e);
                    }
                    copyout(&fl.to_bytes(), arg)
                }

                _ => Err(Errno::EINVAL),
            }
        };
        let _ = frele(fp, p);
        return error;
    }
}

/// Common code for `dup`, `dup2`, and `fcntl(F_DUPFD)`. Called with the table locked; returns
/// with it unlocked, and consumes the caller's reference to `fp`.
pub fn finishdup(
    p: &Proc,
    fp: &'static File,
    old: i32,
    new: i32,
    retval: &mut [Register; 2],
    dupflags: i32,
) -> Result<(), Errno> {
    let fdp = p.fd();

    fdpassertlocked(fdp);
    kassert!(fp.f_iflags.load(Ordering::Relaxed) & FIF_INSERTED != 0);

    let fail = |error: Errno| {
        fdpunlock(fdp);
        let _ = frele(fp, p);
        Err(error)
    };

    if fp.f_count.load(Ordering::SeqCst) >= FDUP_MAX_COUNT {
        return fail(Errno::EDEADLK);
    }

    let oldfp = fd_getfile(fdp, new);
    if dupflags & DUPF_DUP2 != 0 && oldfp.is_none() {
        if fd_inuse(fdp, new) {
            return fail(Errno::EBUSY);
        }
        fd_used(fdp, new);
    }

    // Use `fd_fplock' to synchronize with fd_getfile() so that the function no longer
    // creates a new reference to the old file.
    mtx_enter(&fdp.fd_fplock);
    fdp.set_ofile(new as usize, Some(fp));
    mtx_leave(&fdp.fd_fplock);

    let mut flags = fdp.ofileflags(old as usize) & !(UF_EXCLOSE | UF_FORKCLOSE);
    if dupflags & DUPF_CLOEXEC != 0 {
        flags |= UF_EXCLOSE;
    }
    if dupflags & DUPF_CLOFORK != 0 {
        flags |= UF_FORKCLOSE;
    }
    fdp.set_ofileflags(new as usize, flags);
    retval[0] = new as Register;

    if let Some(oldfp) = oldfp {
        knote_fdclose(p, new);
        fdpunlock(fdp);
        let _ = closef(oldfp, p);
    } else {
        fdpunlock(fdp);
    }

    Ok(())
}

/// `fdinsert(fdp, fd, flags, fp)`: installs `fp` at descriptor `fd` (allocated by
/// `fdalloc`), linking it in `filehead` the first time.
pub fn fdinsert(fdp: &Filedesc, fd: i32, flags: u8, fp: &'static File) {
    fdpassertlocked(fdp);

    mtx_enter(&FHDLK);
    if fp.f_iflags.load(Ordering::SeqCst) & FIF_INSERTED == 0 {
        fp.f_iflags.fetch_or(FIF_INSERTED, Ordering::SeqCst);
        match fdp.ofile(0) {
            // SAFETY: the file at descriptor 0 was inserted (FIF_INSERTED) and stays linked
            // until its `fdrop`; `fp` is a new pool item in no list, which stays in place
            // until its own `fdrop` unlinks it. `FHDLK` is held.
            Some(fq) => unsafe { ListHead::<FileList>::insert_after(fq, fp) },
            // SAFETY: as above; the head is a static.
            None => unsafe { FILEHEAD.0.insert_head(fp) },
        }
    }
    mtx_leave(&FHDLK);

    mtx_enter(&fdp.fd_fplock);
    kassert!(fdp.ofile(fd as usize).is_none());
    fdp.set_ofile(fd as usize, Some(fp));
    mtx_leave(&fdp.fd_fplock);

    fdp.set_ofileflags(
        fd as usize,
        fdp.ofileflags(fd as usize) | (flags & (UF_EXCLOSE | UF_FORKCLOSE | UF_PLEDGEOPEN)),
    );
}

/// `fdremove(fdp, fd)`: empties descriptor `fd` and frees its number.
pub fn fdremove(fdp: &Filedesc, fd: i32) {
    fdpassertlocked(fdp);

    // Use `fd_fplock' to synchronize with fd_getfile() so that the function no longer
    // creates a new reference to the file.
    mtx_enter(&fdp.fd_fplock);
    fdp.set_ofile(fd as usize, None);
    mtx_leave(&fdp.fd_fplock);

    fdp.set_ofileflags(fd as usize, 0);

    fd_unused(fdp, fd);
}

/// `fdrelease(p, fd)`: closes descriptor `fd`. Called with the table locked; returns with it
/// unlocked.
pub fn fdrelease(p: &Proc, fd: i32) -> Result<(), Errno> {
    let fdp = p.fd();

    fdpassertlocked(fdp);

    let Some(fp) = fd_getfile(fdp, fd) else {
        fdpunlock(fdp);
        return Err(Errno::EBADF);
    };
    fdremove(fdp, fd);
    knote_fdclose(p, fd);
    fdpunlock(fdp);
    closef(fp, p)
}

/// Close a file descriptor.
pub fn sys_close(p: &Proc, v: &SysArgs, _retval: &mut [Register; 2]) -> Result<(), Errno> {
    let uap: &SysCloseArgs = sysargs(v);
    let fdp = p.fd();

    fdplock(fdp);
    // fdrelease unlocks fdp.
    fdrelease(p, uap.fd.get())
}

/// Return status information about a file descriptor.
pub fn sys_fstat(p: &Proc, v: &SysArgs, _retval: &mut [Register; 2]) -> Result<(), Errno> {
    let uap: &SysFstatArgs = sysargs(v);
    let fdp = p.fd();

    let Some(fp) = fd_getfile(fdp, uap.fd.get()) else {
        return Err(Errno::EBADF);
    };
    let mut ub = Stat::default();
    let error = (fp.ops().fo_stat)(fp, &mut ub, p);
    let _ = frele(fp, p);
    error?;
    // Don't let non-root see generation numbers (for NFS security)
    if suser(p).is_err() {
        ub.st_gen = 0;
    }
    copyout(&ub.to_bytes(), uap.sb.get() as usize)
}

/// Return pathconf information about a file descriptor.
pub fn sys_fpathconf(p: &Proc, v: &SysArgs, retval: &mut [Register; 2]) -> Result<(), Errno> {
    let uap: &SysFpathconfArgs = sysargs(v);
    let fdp = p.fd();

    let Some(fp) = fd_getfile(fdp, uap.fd.get()) else {
        return Err(Errno::EBADF);
    };
    let error = match fp.f_type.get() {
        DTYPE_PIPE | DTYPE_SOCKET => {
            if uap.name.get() != _PC_PIPE_BUF {
                Err(Errno::EINVAL)
            } else {
                retval[0] = PIPE_BUF as Register;
                Ok(())
            }
        }
        DTYPE_VNODE => {
            let vp = fp.vnode();
            let _ = vn_lock(vp, LK_EXCLUSIVE | LK_RETRY);
            let error = VOP_PATHCONF(vp, uap.name.get(), &mut retval[0]);
            let _ = VOP_UNLOCK(vp);
            error
        }
        _ => Err(Errno::EOPNOTSUPP),
    };
    let _ = frele(fp, p);
    error
}

/// Allocate a file descriptor for the process: the lowest free one at or above `want`.
/// `ENOSPC` asks the caller to `fdexpand` and retry; `EMFILE` is the limit. Called with the
/// table locked.
pub fn fdalloc(p: &Proc, want: i32) -> Result<i32, Errno> {
    let fdp = p.fd();

    fdpassertlocked(fdp);

    let lim = (lim_cur(RLIMIT_NOFILE) as i32).min(MAXFILES.load(Ordering::Relaxed));
    let pledged = p.process().ps_flags.load(Ordering::Relaxed) & PS_PLEDGE != 0;
    fdalloc_search(fdp, want, lim, pledged)
}

/// The search of `fdalloc` once the limit is known: the lowest free descriptor of `fdp` at
/// or above `want` and below `lim`, marked `UF_PLEDGED` for a pledged process.
fn fdalloc_search(fdp: &Filedesc, want: i32, lim: i32, pledged: bool) -> Result<i32, Errno> {
    let mut want = want;

    // Search for a free descriptor starting at the higher of want or fd_freefile. If that
    // fails, consider expanding the ofile array.
    loop {
        let last = fdp.fd_nfiles.load(Ordering::Relaxed).min(lim);
        let mut i = want;
        if i < fdp.fd_freefile.get() {
            i = fdp.fd_freefile.get();
        }
        let off = i >> NDENTRYSHIFT;
        let new = find_next_zero(
            &|o| himap_or_full(fdp, o),
            off,
            ((last + NDENTRIES as i32 - 1) >> NDENTRYSHIFT) as u32,
        );
        if new != -1 {
            let j = find_next_zero(
                &|o| lomap_or_full(fdp, new as usize + o),
                if new > off { 0 } else { i & NDENTRYMASK as i32 },
                NDENTRIES as u32,
            );
            if j == -1 {
                // Free file descriptor in this block was below want, try again with higher
                // want.
                want = (new + 1) << NDENTRYSHIFT;
                continue;
            }
            let i = j + (new << NDENTRYSHIFT);
            if i < last {
                fd_used(fdp, i);
                if want <= fdp.fd_freefile.get() {
                    fdp.fd_freefile.set(i);
                }
                fdp.set_ofileflags(i as usize, if pledged { UF_PLEDGED } else { 0 });
                return Ok(i);
            }
        }
        if fdp.fd_nfiles.load(Ordering::Relaxed) >= lim {
            return Err(Errno::EMFILE);
        }

        return Err(Errno::ENOSPC);
    }
}

/// `fdexpand(p)`: grows the table to `NDEXTENT` descriptors, then doubles it. Called with the
/// table locked. `ENOMEM` when `malloc(9)` fails (see the deviations).
pub fn fdexpand(p: &Proc) -> Result<(), Errno> {
    let fdp = p.fd();

    fdpassertlocked(fdp);

    let oldnfiles = fdp.nfiles();
    let oldofile = fdp.fd_ofiles.get();

    // No space in current array.
    let nfiles = if oldnfiles < NDEXTENT {
        NDEXTENT
    } else {
        2 * oldnfiles
    };

    let Some(newofile) = mallocarray(nfiles, OFILESIZE, M_FILEDESC, M_WAITOK) else {
        return Err(Errno::ENOMEM);
    };
    // Allocate all required chunks before calling free(9) to make sure that ``fd_ofiles''
    // stays valid if we go to sleep.
    let grow_maps = ndhislots(nfiles) > ndhislots(oldnfiles);
    let maps = if grow_maps {
        let himap = mallocarray(ndhislots(nfiles), size_of::<u32>(), M_FILEDESC, M_WAITOK);
        let lomap = mallocarray(ndloslots(nfiles), size_of::<u32>(), M_FILEDESC, M_WAITOK);
        match (himap, lomap) {
            (Some(h), Some(l)) => Some((h.cast::<u32>(), l.cast::<u32>())),
            (h, l) => {
                if let Some(h) = h {
                    free(h, M_FILEDESC, ndhislots(nfiles) * size_of::<u32>());
                }
                if let Some(l) = l {
                    free(l, M_FILEDESC, ndloslots(nfiles) * size_of::<u32>());
                }
                free(newofile, M_FILEDESC, nfiles * OFILESIZE);
                return Err(Errno::ENOMEM);
            }
        }
    } else {
        None
    };
    let newofile = newofile.cast::<*const File>().as_ptr();
    // SAFETY: the allocation holds `nfiles` pointers followed by `nfiles` flag bytes.
    let newofileflags = unsafe { newofile.add(nfiles) }.cast::<u8>();

    // Copy the existing ofile and ofileflags arrays and zero the new portion of each array.
    // SAFETY: the old arrays hold `oldnfiles` entries and the new ones `nfiles >
    // oldnfiles`; they are different allocations.
    unsafe {
        ptr::copy_nonoverlapping(oldofile, newofile, oldnfiles);
        for i in oldnfiles..nfiles {
            newofile.add(i).write(ptr::null());
        }
        ptr::copy_nonoverlapping(fdp.fd_ofileflags.get(), newofileflags, oldnfiles);
        ptr::write_bytes(newofileflags.add(oldnfiles), 0, nfiles - oldnfiles);
    }

    if let Some((newhimap, newlomap)) = maps {
        let (newhimap, newlomap) = (newhimap.as_ptr(), newlomap.as_ptr());
        // SAFETY: as above, for maps of NDHISLOTS/NDLOSLOTS words.
        unsafe {
            let old = ndhislots(oldnfiles);
            ptr::copy_nonoverlapping(fdp.fd_himap.get(), newhimap, old);
            ptr::write_bytes(newhimap.add(old), 0, ndhislots(nfiles) - old);

            let old = ndloslots(oldnfiles);
            ptr::copy_nonoverlapping(fdp.fd_lomap.get(), newlomap, old);
            ptr::write_bytes(newlomap.add(old), 0, ndloslots(nfiles) - old);
        }

        if ndhislots(oldnfiles) > ndhislots(NDFILE) {
            if let Some(h) = NonNull::new(fdp.fd_himap.get()) {
                free(
                    h.cast(),
                    M_FILEDESC,
                    ndhislots(oldnfiles) * size_of::<u32>(),
                );
            }
            if let Some(l) = NonNull::new(fdp.fd_lomap.get()) {
                free(
                    l.cast(),
                    M_FILEDESC,
                    ndloslots(oldnfiles) * size_of::<u32>(),
                );
            }
        }
        fdp.fd_himap.set(newhimap);
        fdp.fd_lomap.set(newlomap);
    }

    mtx_enter(&fdp.fd_fplock);
    fdp.fd_ofiles.set(newofile);
    mtx_leave(&fdp.fd_fplock);

    fdp.fd_ofileflags.set(newofileflags);
    fdp.fd_nfiles.store(nfiles as i32, Ordering::Relaxed);

    if oldnfiles > NDFILE
        && let Some(old) = NonNull::new(oldofile)
    {
        free(old.cast(), M_FILEDESC, oldnfiles * OFILESIZE);
    }
    Ok(())
}

/// Create a new open file structure and allocate a file descriptor for the process that
/// refers to it. Returns the file, with a reference for the caller besides the one the
/// table will hold, and the descriptor, which the caller fills with `fdinsert`. Called with
/// the table locked.
pub fn falloc(p: &Proc) -> Result<(&'static File, i32), Errno> {
    fdpassertlocked(p.fd());

    let i = loop {
        match fdalloc(p, 0) {
            Ok(i) => break i,
            Err(Errno::ENOSPC) => fdexpand(p)?,
            Err(error) => return Err(error),
        }
    };

    let Some(fp) = fnew(p) else {
        fd_unused(p.fd(), i);
        return Err(Errno::ENFILE);
    };

    fref(fp);
    Ok((fp, i))
}

/// `fnew(p)`: a new open file with one reference and the thread's credentials, or `None`
/// when `maxfiles` is reached.
pub fn fnew(p: &Proc) -> Option<&'static File> {
    let nfiles = NUMFILES.fetch_add(1, Ordering::SeqCst) + 1;
    if nfiles > MAXFILES.load(Ordering::Relaxed) {
        NUMFILES.fetch_sub(1, Ordering::SeqCst);
        tablefull("file");
        return None;
    }

    let Some(mem) = pool_get(&FILE_POOL, PR_WAITOK | PR_ZERO) else {
        // PR_WAITOK cannot sleep yet (see the module's deviations).
        NUMFILES.fetch_sub(1, Ordering::SeqCst);
        return None;
    };
    let fp = mem.cast::<File>();
    // SAFETY: a fresh, suitably aligned pool item of `size_of::<File>()` bytes, written once
    // before anything else sees it.
    unsafe { fp.as_ptr().write(File::new()) };
    // SAFETY: as above; the item stays allocated until `fdrop`.
    let fp: &'static File = unsafe { fp.as_ref() };
    // We need to block interrupts as long as `f_mtx' is being taken with and without the
    // KERNEL_LOCK().
    mtx_init(&fp.f_mtx, IPL_MPFLOOR);
    fp.f_count.store(1, Ordering::SeqCst);
    fp.f_cred.set(crhold(p.ucred()));

    Some(fp)
}

/// Build a new filedesc structure.
pub fn fdinit() -> &'static Filedesc {
    let Some(mem) = pool_get(&FDESC_POOL, PR_WAITOK | PR_ZERO) else {
        panic(format_args!("fdinit: fdesc_pool is empty"));
    };
    let newfdp = mem.cast::<Filedesc0>();
    // SAFETY: a fresh, suitably aligned pool item of `size_of::<Filedesc0>()` bytes, written
    // once before anything else sees it.
    unsafe { newfdp.as_ptr().write(Filedesc0::new()) };
    // SAFETY: as above; the item stays allocated until `fdfree`, and never moves, so the
    // pointers into its own arrays stay valid.
    let newfdp: &'static Filedesc0 = unsafe { newfdp.as_ref() };
    let fd = &newfdp.fd_fd;
    rw_init(&fd.fd_lock, "fdlock");
    mtx_init(&fd.fd_fplock, IPL_MPFLOOR);
    fd.fd_kqlist.init();

    // Create the file descriptor table.
    fd.fd_refcnt.set(1);
    fd.fd_cmask.set(S_IWGRP | S_IWOTH);
    fd.fd_ofiles.set(newfdp.fd_dfiles.get().cast());
    fd.fd_ofileflags.set(newfdp.fd_dfileflags.get().cast());
    fd.fd_nfiles.store(NDFILE as i32, Ordering::Relaxed);
    fd.fd_himap.set(newfdp.fd_dhimap.get().cast());
    fd.fd_lomap.set(newfdp.fd_dlomap.get().cast());

    fd.fd_freefile.set(0);
    fd.fd_lastfile.set(0);

    fd
}

/// Share a filedesc structure.
pub fn fdshare(pr: &Process) -> &'static Filedesc {
    let fdp = pr.fd();
    fdp.fd_refcnt.set(fdp.fd_refcnt.get() + 1);
    fdp
}

/// Copy a filedesc structure.
pub fn fdcopy(pr: &Process) -> &'static Filedesc {
    let fdp = pr.fd();

    let newfdp = fdinit();

    fdplock(fdp);
    newfdp.fd_cdir.set(fdp.fd_cdir.get());
    if let Some(cdir) = newfdp.fd_cdir.get() {
        vref(cdir);
    }
    newfdp.fd_rdir.set(fdp.fd_rdir.get());
    if let Some(rdir) = newfdp.fd_rdir.get() {
        vref(rdir);
    }

    // If the number of open files fits in the internal arrays of the open file structure,
    // use them, otherwise allocate additional memory for the number of descriptors
    // currently in use.
    let lastfile = fdp.fd_lastfile.get();
    if lastfile >= NDFILE as i32 {
        // Compute the smallest multiple of NDEXTENT needed for the file descriptors
        // currently in use, allowing the table to shrink.
        let mut i = fdp.nfiles();
        while i >= 2 * NDEXTENT && i > lastfile as usize * 2 {
            i /= 2;
        }
        let Some(mem) = mallocarray(i, OFILESIZE, M_FILEDESC, M_WAITOK | M_ZERO) else {
            panic(format_args!("fdcopy: no memory for {i} descriptors"));
        };
        let ofiles = mem.cast::<*const File>().as_ptr();
        newfdp.fd_ofiles.set(ofiles);
        // SAFETY: the allocation holds `i` pointers followed by `i` flag bytes.
        newfdp.fd_ofileflags.set(unsafe { ofiles.add(i) }.cast());
        newfdp.fd_nfiles.store(i as i32, Ordering::Relaxed);
    }
    if ndhislots(newfdp.nfiles()) > ndhislots(NDFILE) {
        let n = newfdp.nfiles();
        let himap = mallocarray(
            ndhislots(n),
            size_of::<u32>(),
            M_FILEDESC,
            M_WAITOK | M_ZERO,
        );
        let lomap = mallocarray(
            ndloslots(n),
            size_of::<u32>(),
            M_FILEDESC,
            M_WAITOK | M_ZERO,
        );
        let (Some(himap), Some(lomap)) = (himap, lomap) else {
            panic(format_args!(
                "fdcopy: no memory for the maps of {n} descriptors"
            ));
        };
        newfdp.fd_himap.set(himap.cast().as_ptr());
        newfdp.fd_lomap.set(lomap.cast().as_ptr());
    }
    newfdp.fd_freefile.set(fdp.fd_freefile.get());
    newfdp
        .fd_flags
        .store(fdp.fd_flags.load(Ordering::SeqCst), Ordering::SeqCst);
    newfdp.fd_cmask.set(fdp.fd_cmask.get());

    for i in 0..=lastfile {
        let Some(fp) = fdp.ofile(i as usize) else {
            continue;
        };
        let fileflags = fdp.ofileflags(i as usize);
        // If the UF_FORKCLOSE flag is set, skip the fd. XXX Gruesome hack. If count gets
        // too high, fail to copy an fd, since fdcopy()'s callers do not permit it to
        // indicate failure yet. Meanwhile, kqueue files have to be tied to the process
        // that opened them to enforce their internal consistency, so close them here.
        if fp.f_count.load(Ordering::SeqCst) >= FDUP_MAX_COUNT
            || fileflags & UF_FORKCLOSE != 0
            || fp.f_type.get() == DTYPE_KQUEUE
        {
            if i < newfdp.fd_freefile.get() {
                newfdp.fd_freefile.set(i);
            }
            continue;
        }

        fref(fp);
        newfdp.set_ofile(i as usize, Some(fp));
        newfdp.set_ofileflags(i as usize, fileflags);
        fd_used(newfdp, i);
    }
    fdpunlock(fdp);

    newfdp
}

/// Release a filedesc structure.
pub fn fdfree(p: &Proc) {
    let fdp = p.fd();

    fdp.fd_refcnt.set(fdp.fd_refcnt.get() - 1);
    if fdp.fd_refcnt.get() > 0 {
        return;
    }
    for fd in 0..=fdp.fd_lastfile.get() {
        if let Some(fp) = fdp.ofile(fd as usize) {
            fdp.set_ofile(fd as usize, None);
            knote_fdclose(p, fd);
            // closef() expects a refcount of 2
            fref(fp);
            let _ = closef(fp, p);
        }
    }
    p.p_fd.set(ptr::null());
    let nfiles = fdp.nfiles();
    if nfiles > NDFILE
        && let Some(ofiles) = NonNull::new(fdp.fd_ofiles.get())
    {
        free(ofiles.cast(), M_FILEDESC, nfiles * OFILESIZE);
    }
    if ndhislots(nfiles) > ndhislots(NDFILE) {
        if let Some(h) = NonNull::new(fdp.fd_himap.get()) {
            free(h.cast(), M_FILEDESC, ndhislots(nfiles) * size_of::<u32>());
        }
        if let Some(l) = NonNull::new(fdp.fd_lomap.get()) {
            free(l.cast(), M_FILEDESC, ndloslots(nfiles) * size_of::<u32>());
        }
    }
    if let Some(cdir) = fdp.fd_cdir.get() {
        vrele(cdir);
    }
    if let Some(rdir) = fdp.fd_rdir.get() {
        vrele(rdir);
    }
    kassert!(fdp.fd_nuserevents.load(Ordering::SeqCst) == 0);
    // The table is the `fd_fd` member, at offset 0, of the `struct filedesc0` `fdinit` took
    // from the pool.
    pool_put(&FDESC_POOL, NonNull::from(fdp).cast());
}

/// Internal form of close. Decrement reference count on file structure. `p` may be `None`
/// when closing a file that was being passed in a message.
///
/// The fp must have its usecount bumped and will be FRELEd here.
pub fn closef<'a>(fp: &'static File, p: impl Into<Option<&'a Proc>>) -> Result<(), Errno> {
    let p = p.into();

    kassert!(fp.f_count.load(Ordering::SeqCst) >= 2);

    fp.f_count.fetch_sub(1, Ordering::SeqCst);

    // POSIX record locking dictates that any close releases ALL locks owned by this
    // process. This is handled by setting a flag in the unlock to free ONLY locks obeying
    // POSIX semantics, and not to free BSD-style file locks. If the descriptor was in a
    // message, POSIX-style locks aren't passed with the descriptor.
    if let Some(p) = p {
        // SAFETY: a thread's table pointer is null or the live table it holds a reference
        // to (`fdfree` runs this loop before dropping it).
        if let Some(fdp) = unsafe { p.p_fd.get().as_ref() }
            && fdp.fd_flags.load(Ordering::SeqCst) & FD_ADVLOCK != 0
            && fp.f_type.get() == DTYPE_VNODE
        {
            let vp = fp.vnode();
            let mut lf = Flock {
                l_whence: SEEK_SET as i16,
                l_start: 0,
                l_len: 0,
                l_type: F_UNLCK,
                ..Flock::default()
            };
            let id = ptr::from_ref(fdp).cast::<c_void>();
            let _ = VOP_ADVLOCK(vp, id, i32::from(F_UNLCK), &mut lf, F_POSIX);
        }
    }

    frele(fp, p)
}

/// `fdrop(fp, p)`: the last reference to `fp` is gone: unlink it, close it, free it.
pub fn fdrop(fp: &'static File, p: Option<&Proc>) -> Result<(), Errno> {
    kassert!(fp.f_count.load(Ordering::SeqCst) == 0);

    mtx_enter(&FHDLK);
    if fp.f_iflags.load(Ordering::SeqCst) & FIF_INSERTED != 0 {
        // SAFETY: an inserted file is in `filehead` until here; `FHDLK` is held.
        unsafe { ListHead::<FileList>::remove(fp) };
    }
    mtx_leave(&FHDLK);

    let error = match fp.f_ops.get() {
        Some(ops) => (ops.fo_close)(fp, p),
        None => Ok(()),
    };

    // SAFETY: `fnew` gave every file a credential reference, which this drops.
    crfree(unsafe { &*fp.f_cred.get() });
    NUMFILES.fetch_sub(1, Ordering::SeqCst);
    pool_put(&FILE_POOL, NonNull::from(fp).cast());

    error
}

/// Apply an advisory lock on a file descriptor.
///
/// Just attempt to get a record lock of the requested type on the entire file (`l_whence =
/// SEEK_SET`, `l_start = 0`, `l_len = 0`).
pub fn sys_flock(p: &Proc, v: &SysArgs, _retval: &mut [Register; 2]) -> Result<(), Errno> {
    let uap: &SysFlockArgs = sysargs(v);
    let how = uap.how.get();
    let fdp = p.fd();

    let Some(fp) = fd_getfile(fdp, uap.fd.get()) else {
        return Err(Errno::EBADF);
    };
    let error = 'out: {
        if fp.f_type.get() != DTYPE_VNODE {
            break 'out Err(Errno::EOPNOTSUPP);
        }
        let vp = fp.vnode();
        let id = ptr::from_ref(fp).cast::<c_void>();
        let mut lf = Flock {
            l_whence: SEEK_SET as i16,
            l_start: 0,
            l_len: 0,
            ..Flock::default()
        };
        if how & LOCK_UN != 0 {
            lf.l_type = F_UNLCK;
            fp.f_iflags.fetch_and(!FIF_HASLOCK, Ordering::SeqCst);
            break 'out VOP_ADVLOCK(vp, id, i32::from(F_UNLCK), &mut lf, F_FLOCK);
        }
        if how & LOCK_EX != 0 {
            lf.l_type = F_WRLCK;
        } else if how & LOCK_SH != 0 {
            lf.l_type = F_RDLCK;
        } else {
            break 'out Err(Errno::EINVAL);
        }
        fp.f_iflags.fetch_or(FIF_HASLOCK, Ordering::SeqCst);
        if how & LOCK_NB != 0 {
            VOP_ADVLOCK(vp, id, F_SETLK, &mut lf, F_FLOCK)
        } else {
            VOP_ADVLOCK(vp, id, F_SETLK, &mut lf, F_FLOCK | F_WAIT)
        }
    };
    let _ = frele(fp, p);
    error
}

/// File Descriptor pseudo-device driver (`/dev/fd/`).
///
/// Opening minor device N dup()s the file (if any) connected to file descriptor N belonging
/// to the calling process. Note that this driver consists of only the ``open()'' routine,
/// because all subsequent references to this file will be direct to the other driver.
pub fn filedescopen(dev: Dev, _mode: i32, _type: i32, p: &Proc) -> Result<(), Errno> {
    // XXX Kludge: set curproc->p_dupfd to contain the value of the the file descriptor being
    // sought for duplication. The error return ensures that the vnode for this device will
    // be released by vn_open. Open will detect this special error and take the actions in
    // dupfdopen below. Other callers of vn_open or VOP_OPEN will simply report the error.
    p.p_dupfd.set(minor(dev) as i32);
    Err(Errno::ENODEV)
}

/// Duplicate the specified descriptor to a free descriptor.
pub fn dupfdopen(p: &Proc, indx: i32, mode: i32) -> Result<(), Errno> {
    let fdp = p.fd();
    let dupfd = p.p_dupfd.get();

    fdpassertlocked(fdp);

    // Assume that the filename was user-specified; applications do not tend to open
    // /dev/fd/# when they can just call dup()
    if p.process().ps_flags.load(Ordering::Relaxed) & (PS_SUGIDEXEC | PS_SUGID) != 0 {
        if p.p_descfd.get() == 255 {
            return Err(Errno::EPERM);
        }
        if i32::from(p.p_descfd.get()) != dupfd {
            return Err(Errno::EPERM);
        }
    }

    // If the to-be-dup'd fd number is greater than the allowed number of file descriptors,
    // or the fd to be dup'd has already been closed, reject. Note, there is no need to
    // check for new == old because fd_getfile will return NULL if the file at indx is newly
    // created by falloc.
    let Some(wfp) = fd_getfile(fdp, dupfd) else {
        return Err(Errno::EBADF);
    };

    // Check that the mode the file is being opened for is a subset of the mode of the
    // existing descriptor.
    if ((mode & (FREAD | FWRITE)) | wfp.flag()) != wfp.flag() {
        let _ = frele(wfp, p);
        return Err(Errno::EACCES);
    }
    if wfp.f_count.load(Ordering::SeqCst) >= FDUP_MAX_COUNT {
        let _ = frele(wfp, p);
        return Err(Errno::EDEADLK);
    }

    kassert!(wfp.f_iflags.load(Ordering::SeqCst) & FIF_INSERTED != 0);

    mtx_enter(&fdp.fd_fplock);
    kassert!(fdp.ofile(indx as usize).is_none());
    fdp.set_ofile(indx as usize, Some(wfp));
    mtx_leave(&fdp.fd_fplock);

    fdp.set_ofileflags(
        indx as usize,
        (fdp.ofileflags(indx as usize) & (UF_EXCLOSE | UF_FORKCLOSE))
            | (fdp.ofileflags(dupfd as usize) & !(UF_EXCLOSE | UF_FORKCLOSE)),
    );

    Ok(())
}

/// Doing an exec, so handle fd flags: do close-on-exec and clear pledged and close-on-fork.
pub fn fdprepforexec(p: &Proc) {
    let fdp = p.fd();

    fdplock(fdp);
    let mut fd = 0;
    while fd <= fdp.fd_lastfile.get() {
        let flags = fdp.ofileflags(fd as usize) & !(UF_PLEDGED | UF_FORKCLOSE);
        fdp.set_ofileflags(fd as usize, flags);
        if flags & UF_EXCLOSE != 0 {
            // fdrelease() unlocks fdp.
            let _ = fdrelease(p, fd);
            fdplock(fdp);
        }
        fd += 1;
    }
    fdpunlock(fdp);
}

/// `closefrom(2)`: closes every descriptor from `fd` on.
pub fn sys_closefrom(p: &Proc, v: &SysArgs, _retval: &mut [Register; 2]) -> Result<(), Errno> {
    let uap: &SysClosefromArgs = sysargs(v);
    let fdp = p.fd();

    let startfd = uap.fd.get() as u32;
    fdplock(fdp);

    if startfd > fdp.fd_lastfile.get() as u32 {
        fdpunlock(fdp);
        return Err(Errno::EBADF);
    }

    let mut i = startfd;
    while i <= fdp.fd_lastfile.get() as u32 {
        // fdrelease() unlocks fdp.
        let _ = fdrelease(p, i as i32);
        fdplock(fdp);
        i += 1;
    }

    fdpunlock(fdp);
    Ok(())
}

/// `getdtablecount(2)`: the number of descriptors open.
pub fn sys_getdtablecount(p: &Proc, _v: &SysArgs, retval: &mut [Register; 2]) -> Result<(), Errno> {
    retval[0] = p.fd().fd_openfd.load(Ordering::Relaxed) as Register;
    Ok(())
}
/* </CODE> */

/* <TESTS> */
#[cfg(test)]
mod tests {
    // Host tests for the descriptor tables: the bitmap search (`find_next_zero`, `fd_used`,
    // `fd_unused`, `find_last_set`), descriptor allocation and expansion (`fdalloc`'s search,
    // `fdexpand`, past the 1024 descriptors of the internal maps), the open file life cycle
    // (`fnew`, `fdinsert`, `fd_getfile`, `fdrelease`, `closef`, `fdrop`), `finishdup`, `fdcopy`,
    // `fdprepforexec` and `fdfree`.
    //
    // The limit `fdalloc` reads needs a `curproc`, which the host's one CPU cannot give a test
    // without disturbing the others, so the tests drive `fdalloc_search` with the limit as an
    // argument.

    use std::boxed::Box;
    use std::sync::MutexGuard;
    use std::sync::atomic::AtomicUsize;
    use std::{assert, assert_eq, vec::Vec};

    use super::*;
    use crate::kern::kern_proc::procinit;
    use crate::kern::kern_prot::crget;
    use crate::kern::subr_pool::tests::setup_real_memory;
    use crate::sys::event::Knote;
    use crate::sys::file::Fileops;
    use crate::sys::filedesc::NDENTRIES;
    use crate::sys::uio::Uio;

    /// Real memory, the process pools (for the credentials) and the file pools.
    fn setup() -> MutexGuard<'static, ()> {
        let guard = setup_real_memory();
        crate::machine::cons::consinit();
        procinit();
        filedesc_init();
        guard
    }

    /// A thread of a fresh process with credentials and a new descriptor table.
    fn thread() -> &'static Proc {
        let pr: &'static Process = Box::leak(Box::new(Process::new()));
        let p: &'static Proc = Box::leak(Box::new(Proc::new()));
        p.p_p.set(pr);
        pr.ps_mainproc.set(p);
        let cr = crget();
        p.p_ucred.set(cr);
        pr.ps_ucred.set(crhold(cr));
        let fdp = fdinit();
        pr.ps_fd.set(fdp);
        p.p_fd.set(fdp);
        p
    }

    /// How many times `test_close` ran.
    static CLOSES: AtomicUsize = AtomicUsize::new(0);

    fn test_rw(_fp: &File, _uio: &mut Uio<'_>, _flags: i32) -> Result<(), Errno> {
        Ok(())
    }

    fn test_ioctl(_fp: &File, _com: u64, _data: &mut [u8], _p: &Proc) -> Result<(), Errno> {
        Err(Errno::ENOTTY)
    }

    fn test_kqfilter(_fp: &File, _kn: &Knote) -> Result<(), Errno> {
        Err(Errno::EINVAL)
    }

    fn test_stat(_fp: &File, _ub: &mut Stat, _p: &Proc) -> Result<(), Errno> {
        Ok(())
    }

    fn test_close(_fp: &File, _p: Option<&Proc>) -> Result<(), Errno> {
        CLOSES.fetch_add(1, Ordering::SeqCst);
        Ok(())
    }

    static TESTOPS: Fileops = Fileops {
        fo_read: test_rw,
        fo_write: test_rw,
        fo_ioctl: test_ioctl,
        fo_kqfilter: test_kqfilter,
        fo_stat: test_stat,
        fo_close: test_close,
        fo_seek: None,
    };

    /// `falloc` + `fdinsert` with the limit given: a new file at the lowest free descriptor,
    /// the caller's extra reference dropped.
    fn open_file(p: &Proc, lim: i32) -> (i32, &'static File) {
        let fdp = p.fd();
        fdplock(fdp);
        let fd = loop {
            match fdalloc_search(fdp, 0, lim, false) {
                Ok(fd) => break fd,
                Err(Errno::ENOSPC) => fdexpand(p).unwrap(),
                Err(e) => panic!("fdalloc: {e:?}"),
            }
        };
        let fp = fnew(p).unwrap();
        fref(fp);
        fp.f_flag.store((FREAD | FWRITE) as u32, Ordering::SeqCst);
        fp.f_type.set(DTYPE_PIPE);
        fp.f_ops.set(Some(&TESTOPS));
        fdinsert(fdp, fd, 0, fp);
        fdpunlock(fdp);
        frele(fp, p).unwrap();
        (fd, fp)
    }

    #[test]
    fn find_next_zero_scans_words_from_want() {
        let map = [!0u32, 0xffff_00ff, 0];
        let at = |o: usize| map.get(o).copied().unwrap_or(!0);
        assert_eq!(find_next_zero(&at, 0, 96), 40);
        assert_eq!(find_next_zero(&at, 41, 96), 41);
        assert_eq!(find_next_zero(&at, 48, 96), 64);
        assert_eq!(find_next_zero(&at, 97, 96), -1);
        let full = |_| !0u32;
        assert_eq!(find_next_zero(&full, 0, 32), -1);
        // bits rounds up to whole high-map words: NDLOSLOTS(1) is 32 words.
        let one = |o: usize| if o == 31 { 0xfffe_ffff } else { !0 };
        assert_eq!(find_next_zero(&one, 0, 1), 31 * 32 + 16);
    }

    #[test]
    fn fdinit_builds_an_empty_table() {
        let _g = setup();
        let p = thread();
        let fdp = p.fd();
        assert_eq!(fdp.fd_nfiles.load(Ordering::Relaxed), NDFILE as i32);
        assert_eq!(fdp.fd_refcnt.get(), 1);
        assert_eq!(fdp.fd_cmask.get(), S_IWGRP | S_IWOTH);
        assert_eq!(fdp.fd_openfd.load(Ordering::Relaxed), 0);
        assert!((0..NDFILE).all(|fd| fdp.ofile(fd).is_none()));
        fdfree(p);
        assert!(p.p_fd.get().is_null());
    }

    #[test]
    fn fdalloc_takes_the_lowest_free_descriptor() {
        let _g = setup();
        let p = thread();
        let fdp = p.fd();
        fdplock(fdp);
        for want in 0..3 {
            assert_eq!(fdalloc_search(fdp, 0, 1000, false), Ok(want));
        }
        assert_eq!(fdalloc_search(fdp, 7, 1000, true), Ok(7));
        assert_eq!(fdp.ofileflags(7), UF_PLEDGED);
        assert_eq!(fdp.fd_lastfile.get(), 7);
        assert_eq!(fdp.fd_openfd.load(Ordering::Relaxed), 4);

        fd_unused(fdp, 1);
        assert_eq!(fdp.fd_freefile.get(), 1);
        assert_eq!(fdalloc_search(fdp, 0, 1000, false), Ok(1));
        fd_unused(fdp, 7);
        assert_eq!(fdp.fd_lastfile.get(), 2);
        assert_eq!(find_last_set(fdp, 3), 2);

        // The table is full at NDFILE: ENOSPC asks for an expansion, EMFILE is the limit.
        while fdalloc_search(fdp, 0, 1000, false).is_ok() {}
        assert_eq!(fdp.fd_openfd.load(Ordering::Relaxed), NDFILE as i32);
        assert_eq!(fdalloc_search(fdp, 0, 1000, false), Err(Errno::ENOSPC));
        assert_eq!(
            fdalloc_search(fdp, 0, NDFILE as i32, false),
            Err(Errno::EMFILE)
        );
        fdpunlock(fdp);
    }

    #[test]
    fn fdexpand_grows_the_table_and_the_maps() {
        let _g = setup();
        let p = thread();
        let fdp = p.fd();
        fdplock(fdp);
        let mut sizes = Vec::new();
        let mut fd = 0;
        while fd < 1100 {
            match fdalloc_search(fdp, 0, 4096, false) {
                Ok(got) => {
                    assert_eq!(got, fd);
                    fd += 1;
                }
                Err(Errno::ENOSPC) => {
                    fdexpand(p).unwrap();
                    sizes.push(fdp.fd_nfiles.load(Ordering::Relaxed));
                }
                Err(e) => panic!("fdalloc: {e:?}"),
            }
        }
        assert_eq!(sizes, [50, 100, 200, 400, 800, 1600]);
        // Past 1024 descriptors the maps left the internal arrays: 2 high words, 64 low.
        assert_eq!(ndhislots(fdp.nfiles()), 2);
        assert_eq!(fdp.himap(0), !0);
        assert_eq!(fdp.lomap(31), !0);
        assert_eq!(fdp.lomap(34), (1 << (1100 - 34 * NDENTRIES)) - 1);
        assert_eq!(fdp.fd_lastfile.get(), 1099);

        // A hole is found again through the high map.
        fd_unused(fdp, 40);
        assert_eq!(fdp.himap(0) & 2, 0);
        assert_eq!(fdalloc_search(fdp, 0, 4096, false), Ok(40));
        assert_eq!(fdalloc_search(fdp, 2000, 4096, false), Err(Errno::ENOSPC));
        fdpunlock(fdp);
        fdfree(p);
    }

    #[test]
    fn files_are_counted_shared_and_closed() {
        let _g = setup();
        let p = thread();
        let fdp = p.fd();
        let closes = CLOSES.load(Ordering::SeqCst);
        let numfiles = NUMFILES.load(Ordering::SeqCst);

        let (fd, fp) = open_file(p, 1000);
        assert_eq!(fd, 0);
        assert_eq!(fp.f_count.load(Ordering::SeqCst), 1);
        assert!(fp.f_iflags.load(Ordering::SeqCst) & FIF_INSERTED != 0);
        assert!(ptr::eq(FILEHEAD.0.first().unwrap(), fp));
        assert_eq!(NUMFILES.load(Ordering::SeqCst), numfiles + 1);

        // fd_getfile takes a reference; fd_getfile_mode refuses a mode the file lacks.
        let got = fd_getfile(fdp, fd).unwrap();
        assert_eq!(fp.f_count.load(Ordering::SeqCst), 2);
        frele(got, p).unwrap();
        fp.f_flag.store(FREAD as u32, Ordering::SeqCst);
        assert!(fd_getfile_mode(fdp, fd, FWRITE).is_none());
        assert_eq!(fp.f_count.load(Ordering::SeqCst), 1);
        assert!(fd_getfile(fdp, 5).is_none());
        assert!(fd_getfile(fdp, -1).is_none());

        // dup: finishdup puts the same file at a second descriptor.
        let mut retval = [0; 2];
        fdplock(fdp);
        let new = fdalloc_search(fdp, 3, 1000, false).unwrap();
        fdp.set_ofileflags(fd as usize, UF_EXCLOSE);
        let dup = fd_getfile(fdp, fd).unwrap();
        assert_eq!(
            finishdup(p, dup, fd, new, &mut retval, DUPF_CLOFORK),
            Ok(())
        );
        assert_eq!(retval[0], 3);
        assert_eq!(fp.f_count.load(Ordering::SeqCst), 2);
        assert_eq!(fdp.ofileflags(3), UF_FORKCLOSE);
        assert!(!fd_checkclosed(fdp, 3, fp));

        // fdcopy skips the close-on-fork descriptor and shares the file.
        let child = fdcopy(p.process());
        assert!(ptr::eq(child.ofile(0).unwrap(), fp));
        assert!(child.ofile(3).is_none());
        assert_eq!(fp.f_count.load(Ordering::SeqCst), 3);

        // exec closes the close-on-exec descriptor.
        fdprepforexec(p);
        assert!(fdp.ofile(0).is_none());
        assert_eq!(fdp.ofileflags(3), 0);
        assert_eq!(fp.f_count.load(Ordering::SeqCst), 2);
        assert_eq!(CLOSES.load(Ordering::SeqCst), closes);

        // close(3), then the child's table goes: the last reference closes the file.
        fdplock(fdp);
        assert_eq!(fdrelease(p, 3), Ok(()));
        fdplock(fdp);
        assert_eq!(fdrelease(p, 3), Err(Errno::EBADF));
        assert_eq!(fdp.fd_openfd.load(Ordering::Relaxed), 0);
        let q = thread();
        let qfd = q.fd();
        q.p_fd.set(child);
        fdfree(q);
        assert_eq!(CLOSES.load(Ordering::SeqCst), closes + 1);
        assert_eq!(NUMFILES.load(Ordering::SeqCst), numfiles);
        assert!(FILEHEAD.0.first().is_none_or(|f| !ptr::eq(f, fp)));
        q.p_fd.set(qfd);
        fdfree(q);
        fdfree(p);
    }
}
/* </TESTS> */
