/*	$OpenBSD: sys_generic.c,v 1.161 2026/03/09 02:44:04 deraadt Exp $	*/
/*	$NetBSD: sys_generic.c,v 1.24 1996/03/29 00:25:32 cgd Exp $	*/
/* <LICENSES> */
/*
 * Copyright (c) 1996 Theo de Raadt
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
 *	@(#)sys_generic.c	8.5 (Berkeley) 1/21/94
 */
/* </LICENSES> */

//! `sys_generic.c`: the generic file system calls: `read`, `write`, `ioctl`, `select`,
//! `poll`.
//!
//! Upstream: sys/kern/sys_generic.c @ 3ce1f3f79392
//!
//! Status: `wip`. Milestone M6 (part b) ported `sys_write` for the console; M7b
//! (`kern_descrip.c`) makes the read and write paths real: `iovec_copyin`/`iovec_free`,
//! `sys_read`, `sys_readv`, `dofilereadv`, `sys_write`, `sys_writev`, `dofilewritev` and
//! `sys_ioctl` reach the file through `fd_getfile_mode` and call its `fileops`. M8:
//! `sys_select`, `sys_pselect`, `dopselect`, `pselregister`, `pselcollect`, `selwakeup`,
//! `pollout`, `sys_poll`, `sys_ppoll`, `doppoll`, `ppollregister_evts`, `ppollregister`,
//! `ppollcollect` and `sys_utrace` (`KTRACE` is not configured: it succeeds and records
//! nothing, the C's `#else`); the file is complete.
//!
//! ## Deviations
//! - `iovec_copyin(uiov, aiov, iovcnt)` returns the iovecs (the caller's `aiov` or a
//!   `malloc(M_IOV)` array) and the residual count, and frees its own array when it fails,
//!   so `iovec_free` (`unsafe`: it frees) is only called on a successful result. The user
//!   array is copied in one iovec at a time, each read through `Iovec::from_bytes`.
//! - `dofilereadv`/`dofilewritev` take a `Uio` whose iovecs borrow the caller's array; the
//!   positioned checks (`FO_POSITION`) answer `ESPIPE` for every file that is not a vnode
//!   (and for fifos and ttys), as in C.
//! - `sys_ioctl`'s argument buffer is a byte slice of `max(IOCPARM_LEN(com), sizeof(caddr_t))`
//!   bytes, from the 128-byte stack buffer or `malloc(M_IOCTLOPS)`.
//! - `KTRACE` is not configured.
//! - `select(2)`/`poll(2)` are OpenBSD's, built on the thread's poll kqueue
//!   (`kern_event.rs`): `kqpoll_init` and `kqpoll_done` take the thread, and `kqpoll_init`
//!   can fail (`ENOMEM`, the kqueue pool cannot sleep yet), which fails the call. The fd
//!   sets and the pollfd array are one allocation each (the C keeps small ones on its
//!   stack).

use core::ptr::NonNull;
use core::sync::atomic::Ordering;

use crate::conf::param::MAXFILES;
use crate::kassert;
use crate::kern::kern_descrip::fd_getfile_mode;
use crate::kern::kern_event::{
    knote_locked, kqpoll_done, kqpoll_init, kqueue_register, kqueue_scan, kqueue_scan_finish,
    kqueue_scan_setup,
};
use crate::kern::kern_lock::{mtx_enter, mtx_leave};
use crate::kern::kern_malloc::{free, malloc, mallocarray};
use crate::kern::kern_pledge::pledge_ioctl;
use crate::kern::kern_sig::dosigsuspend;
use crate::kern::kern_sig::ptsignal;
use crate::kern::kern_synch::{nowake, tsleep_nsec};
use crate::kern::kern_time::ratecheck;
use crate::kern::subr_prf::Str;
use crate::kprintf;
use crate::machine::copy::copyin_obj;
use crate::machine::copy::{copyin, copyout};
use crate::sys::errno::Errno;
use crate::sys::event::{
    __EV_HUP, __EV_POLL, __EV_SELECT, EV_ADD, EV_ENABLE, EV_ERROR, EVFILT_EXCEPT, EVFILT_READ,
    EVFILT_WRITE, Kevent, KqueueScanState, NOTE_OOB, NOTE_SUBMIT, ev_set,
};
use crate::sys::eventvar::KQ_NEVENTS;
use crate::sys::fcntl::{FASYNC, FNONBLOCK, FREAD, FWRITE};
use crate::sys::file::{DTYPE_SOCKET, DTYPE_VNODE, FO_POSITION, File, frele};
use crate::sys::filedesc::{UF_EXCLOSE, UF_PLEDGEOPEN, fdplock, fdpunlock};
use crate::sys::filio::{FIOASYNC, FIOCLEX, FIONBIO, FIONCLEX};
use crate::sys::ioccom::{IOC_IN, IOC_OUT, IOC_VOID, IOCPARM_MAX, iocparm_len};
use crate::sys::limits::SSIZE_MAX;
use crate::sys::malloc::{M_IOCTLOPS, M_IOV, M_WAITOK};
use crate::sys::param::{PCATCH, PSOCK};
use crate::sys::poll::{
    INFTIM, POLL_NOHUP, POLLERR, POLLHUP, POLLIN, POLLNVAL, POLLOUT, POLLPRI, POLLRDBAND,
    POLLRDNORM, POLLWRNORM, Pollfd,
};
use crate::sys::proc::Proc;
use crate::sys::resource::RLIMIT_NOFILE;
use crate::sys::resourcevar::lim_cur;
use crate::sys::select::{FdMask, NFDBITS, fd_set, howmany};
use crate::sys::selinfo::Selinfo;
use crate::sys::signal::SIGPIPE;
use crate::sys::signal::Sigset;
use crate::sys::signalvar::SignalType;
use crate::sys::signalvar::sigcantmask;
use crate::sys::syscallargs::{
    SysIoctlArgs, SysReadArgs, SysReadvArgs, SysWriteArgs, SysWritevArgs,
};
use crate::sys::syscallargs::{SysPollArgs, SysPpollArgs, SysPselectArgs, SysSelectArgs};
use crate::sys::syslimits::IOV_MAX;
use crate::sys::systm::{INFSLP, MAXTSLP};
use crate::sys::systm::{SysArgs, kernel_lock, kernel_unlock, sysargs};
use crate::sys::time::{Timespec, Timeval, timespec_to_nsec, timeval_to_timespec};
use crate::sys::types::{Off, Register};
use crate::sys::uio::{Iovec, UIO_SMALLIOV, Uio, UioRw, UioSeg};
use crate::sys::vnode::{VCHR, VFIFO, VISTTY};
use alloc::vec;
use alloc::vec::Vec;
use core::mem::offset_of;
use libkern::StaticCell;

/// `STK_PARAMS`: the ioctl argument bytes `sys_ioctl` keeps on its stack.
const STK_PARAMS: usize = 128;

/// `iovec_copyin(uiov, &iov, aiov, iovcnt, &resid)`: copies in the user's `iovcnt` iovecs
/// at `uiov`, into `aiov` when they fit (`UIO_SMALLIOV`), else into a `malloc(M_IOV)` array;
/// returns the iovecs and their total length.
pub fn iovec_copyin(
    uiov: usize,
    aiov: &mut [Iovec; UIO_SMALLIOV],
    iovcnt: u32,
) -> Result<(&mut [Iovec], usize), Errno> {
    let n = iovcnt as usize;
    let iov: &mut [Iovec] = if n > UIO_SMALLIOV {
        if n > IOV_MAX {
            return Err(Errno::EINVAL);
        }
        let Some(mem) = mallocarray(n, size_of::<Iovec>(), M_IOV, M_WAITOK) else {
            return Err(Errno::ENOMEM);
        };
        let mem = mem.cast::<Iovec>().as_ptr();
        for i in 0..n {
            // SAFETY: a fresh allocation of `n` iovecs, suitably aligned.
            unsafe { mem.add(i).write(Iovec::new()) };
        }
        // SAFETY: as above, now initialised; ours until `iovec_free`.
        unsafe { core::slice::from_raw_parts_mut(mem, n) }
    } else if n > 0 {
        &mut aiov[..n]
    } else {
        return Err(Errno::EINVAL);
    };

    let mut resid: usize = 0;
    let mut error = Ok(());
    for (i, slot) in iov.iter_mut().enumerate() {
        let mut bytes = [0u8; Iovec::SIZE];
        if let Err(e) = copyin(uiov + i * Iovec::SIZE, &mut bytes) {
            error = Err(e);
            break;
        }
        *slot = Iovec::from_bytes(&bytes);
        resid += slot.iov_len;
        // Writes return ssize_t because -1 is returned on error. Therefore we must restrict
        // the length to SSIZE_MAX to avoid garbage return values. Note that the addition is
        // guaranteed to not wrap because SSIZE_MAX * 2 < SIZE_MAX.
        if slot.iov_len > SSIZE_MAX as usize || resid > SSIZE_MAX as usize {
            error = Err(Errno::EINVAL);
            break;
        }
    }

    match error {
        Ok(()) => Ok((iov, resid)),
        Err(e) => {
            // SAFETY: `iov` is what this function made for `iovcnt`, and is dropped here.
            unsafe { iovec_free(iov, iovcnt) };
            Err(e)
        }
    }
}

/// `iovec_free(iov, iovcnt)`: releases the array `iovec_copyin` allocated, if it did.
///
/// # Safety
///
/// `iov` was returned by `iovec_copyin` for the same `iovcnt` and is not used afterwards.
pub unsafe fn iovec_free(iov: &mut [Iovec], iovcnt: u32) {
    if iovcnt as usize > UIO_SMALLIOV
        && let Some(p) = NonNull::new(iov.as_mut_ptr())
    {
        free(p.cast(), M_IOV, iovcnt as usize * size_of::<Iovec>());
    }
}

/// Read system call.
pub fn sys_read(p: &Proc, v: &SysArgs, retval: &mut [Register; 2]) -> Result<(), Errno> {
    let uap: &SysReadArgs = sysargs(v);

    let mut iov = [Iovec {
        iov_base: uap.buf.get(),
        iov_len: uap.nbyte.get(),
    }];
    if iov[0].iov_len > SSIZE_MAX as usize {
        return Err(Errno::EINVAL);
    }
    let resid = iov[0].iov_len;

    let mut auio = Uio {
        uio_iov: &mut iov,
        uio_offset: 0,
        uio_resid: resid,
        uio_segflg: UioSeg::UIO_USERSPACE,
        uio_rw: UioRw::UIO_READ,
        uio_procp: Some(p),
    };

    dofilereadv(p, uap.fd.get(), &mut auio, 0, retval)
}

/// Scatter read system call.
pub fn sys_readv(p: &Proc, v: &SysArgs, retval: &mut [Register; 2]) -> Result<(), Errno> {
    let uap: &SysReadvArgs = sysargs(v);
    let iovcnt = uap.iovcnt.get() as u32;
    let mut aiov = [Iovec::new(); UIO_SMALLIOV];

    let (iov, resid) = iovec_copyin(uap.iovp.get() as usize, &mut aiov, iovcnt)?;

    let mut auio = Uio {
        uio_iov: &mut *iov,
        uio_offset: 0,
        uio_resid: resid,
        uio_segflg: UioSeg::UIO_USERSPACE,
        uio_rw: UioRw::UIO_READ,
        uio_procp: Some(p),
    };

    let error = dofilereadv(p, uap.fd.get(), &mut auio, 0, retval);
    // SAFETY: `iov` came from `iovec_copyin` with `iovcnt`; the uio borrowing it is gone.
    unsafe { iovec_free(iov, iovcnt) };
    error
}

/// The positioned-I/O checks of `dofilereadv`/`dofilewritev` (`FO_POSITION`).
fn position_check(fp: &File, offset: Off) -> Result<(), Errno> {
    if fp.f_type.get() != DTYPE_VNODE {
        return Err(Errno::ESPIPE);
    }
    let vp = fp.vnode();
    if vp.v_type.get() == VFIFO || vp.v_flag.get() & VISTTY != 0 {
        return Err(Errno::ESPIPE);
    }

    if offset < 0 && vp.v_type.get() != VCHR {
        return Err(Errno::EINVAL);
    }
    Ok(())
}

/// The errors that come after some data was moved are dropped: `ERESTART`, `EINTR` and
/// `EWOULDBLOCK` when `uio_resid` is no longer `cnt`.
fn partial_ok(error: Result<(), Errno>, resid: usize, cnt: usize) -> Result<(), Errno> {
    match error {
        Err(Errno::ERESTART | Errno::EINTR | Errno::EAGAIN) if resid != cnt => Ok(()),
        e => e,
    }
}

/// `dofilereadv(p, fd, uio, flags, retval)`: reads descriptor `fd` into the user iovecs of
/// `uio`; `retval` is the byte count.
pub fn dofilereadv<'a>(
    p: &'a Proc,
    fd: i32,
    uio: &mut Uio<'a>,
    flags: i32,
    retval: &mut [Register; 2],
) -> Result<(), Errno> {
    let fdp = p.fd();

    kassert!(uio.uio_iovcnt() > 0);

    let Some(fp) = fd_getfile_mode(fdp, fd, FREAD) else {
        return Err(Errno::EBADF);
    };

    let error = 'done: {
        // Checks for positioned read.
        if flags & FO_POSITION != 0
            && let Err(e) = position_check(fp, uio.uio_offset)
        {
            break 'done Err(e);
        }

        uio.uio_rw = UioRw::UIO_READ;
        uio.uio_segflg = UioSeg::UIO_USERSPACE;
        uio.uio_procp = Some(p);
        let cnt = uio.uio_resid;
        let error = (fp.ops().fo_read)(fp, uio, flags);
        let error = partial_ok(error, uio.uio_resid, cnt);
        let cnt = cnt - uio.uio_resid;

        mtx_enter(&fp.f_mtx);
        fp.f_rxfer.set(fp.f_rxfer.get() + 1);
        fp.f_rbytes.set(fp.f_rbytes.get() + cnt as u64);
        mtx_leave(&fp.f_mtx);
        retval[0] = cnt as Register;
        error
    };
    let _ = frele(fp, p);
    error
}

/// Write system call.
pub fn sys_write(p: &Proc, v: &SysArgs, retval: &mut [Register; 2]) -> Result<(), Errno> {
    let uap: &SysWriteArgs = sysargs(v);

    let mut iov = [Iovec {
        iov_base: uap.buf.get().cast_mut(),
        iov_len: uap.nbyte.get(),
    }];
    if iov[0].iov_len > SSIZE_MAX as usize {
        return Err(Errno::EINVAL);
    }
    let resid = iov[0].iov_len;

    let mut auio = Uio {
        uio_iov: &mut iov,
        uio_offset: 0,
        uio_resid: resid,
        uio_segflg: UioSeg::UIO_USERSPACE,
        uio_rw: UioRw::UIO_WRITE,
        uio_procp: Some(p),
    };

    dofilewritev(p, uap.fd.get(), &mut auio, 0, retval)
}

/// Gather write system call.
pub fn sys_writev(p: &Proc, v: &SysArgs, retval: &mut [Register; 2]) -> Result<(), Errno> {
    let uap: &SysWritevArgs = sysargs(v);
    let iovcnt = uap.iovcnt.get() as u32;
    let mut aiov = [Iovec::new(); UIO_SMALLIOV];

    let (iov, resid) = iovec_copyin(uap.iovp.get() as usize, &mut aiov, iovcnt)?;

    let mut auio = Uio {
        uio_iov: &mut *iov,
        uio_offset: 0,
        uio_resid: resid,
        uio_segflg: UioSeg::UIO_USERSPACE,
        uio_rw: UioRw::UIO_WRITE,
        uio_procp: Some(p),
    };

    let error = dofilewritev(p, uap.fd.get(), &mut auio, 0, retval);
    // SAFETY: `iov` came from `iovec_copyin` with `iovcnt`; the uio borrowing it is gone.
    unsafe { iovec_free(iov, iovcnt) };
    error
}

/// `dofilewritev(p, fd, uio, flags, retval)`: writes the user iovecs of `uio` to
/// descriptor `fd`; `retval` is the byte count.
pub fn dofilewritev<'a>(
    p: &'a Proc,
    fd: i32,
    uio: &mut Uio<'a>,
    flags: i32,
    retval: &mut [Register; 2],
) -> Result<(), Errno> {
    let fdp = p.fd();

    kassert!(uio.uio_iovcnt() > 0);

    let Some(fp) = fd_getfile_mode(fdp, fd, FWRITE) else {
        return Err(Errno::EBADF);
    };

    let error = 'done: {
        if fdp.ofileflags(fd as usize) & UF_PLEDGEOPEN != 0 {
            break 'done Err(Errno::EPERM);
        }

        // Checks for positioned write.
        if flags & FO_POSITION != 0
            && let Err(e) = position_check(fp, uio.uio_offset)
        {
            break 'done Err(e);
        }

        uio.uio_rw = UioRw::UIO_WRITE;
        uio.uio_segflg = UioSeg::UIO_USERSPACE;
        uio.uio_procp = Some(p);
        let cnt = uio.uio_resid;
        let error = (fp.ops().fo_write)(fp, uio, flags);
        let error = partial_ok(error, uio.uio_resid, cnt);
        if error == Err(Errno::EPIPE) {
            ptsignal(p, SIGPIPE, SignalType::STHREAD);
        }
        let cnt = cnt - uio.uio_resid;

        mtx_enter(&fp.f_mtx);
        fp.f_wxfer.set(fp.f_wxfer.get() + 1);
        fp.f_wbytes.set(fp.f_wbytes.get() + cnt as u64);
        mtx_leave(&fp.f_mtx);
        retval[0] = cnt as Register;
        error
    };
    let _ = frele(fp, p);
    error
}

/// Ioctl system call.
pub fn sys_ioctl(p: &Proc, v: &SysArgs, _retval: &mut [Register; 2]) -> Result<(), Errno> {
    let uap: &SysIoctlArgs = sysargs(v);
    let fd = uap.fd.get();
    let com = uap.com.get();
    let udata = uap.data.get() as usize;
    let fdp = p.fd();
    let mut size = 0usize;
    let mut memp: Option<NonNull<u8>> = None;
    let mut stkbuf = [0u8; STK_PARAMS];

    let Some(fp) = fd_getfile_mode(fdp, fd, FREAD | FWRITE) else {
        return Err(Errno::EBADF);
    };

    let error = 'out: {
        if fp.f_type.get() == DTYPE_SOCKET {
            let so = crate::kern::sys_socket::fp_socket(fp);

            if so.has_state(crate::sys::socketvar::SS_DNS) {
                break 'out Err(Errno::EINVAL);
            }
        }

        if let Err(error) = pledge_ioctl(p, com, fp) {
            break 'out Err(error);
        }

        if com == FIONCLEX || com == FIOCLEX {
            fdplock(fdp);
            let flags = fdp.ofileflags(fd as usize);
            fdp.set_ofileflags(
                fd as usize,
                if com == FIONCLEX {
                    flags & !UF_EXCLOSE
                } else {
                    flags | UF_EXCLOSE
                },
            );
            fdpunlock(fdp);
            break 'out Ok(());
        }

        // Interpret high order word to find amount of data to be copied to/from the user's
        // address space.
        size = iocparm_len(com) as usize;
        if size > IOCPARM_MAX {
            break 'out Err(Errno::ENOTTY);
        }
        let data: &mut [u8] = if size > STK_PARAMS {
            let Some(m) = malloc(size, M_IOCTLOPS, M_WAITOK) else {
                break 'out Err(Errno::ENOMEM);
            };
            memp = Some(m);
            // SAFETY: a fresh allocation of `size` bytes, ours until the `free` below;
            // zeroed before the slice is made.
            unsafe {
                m.as_ptr().write_bytes(0, size);
                core::slice::from_raw_parts_mut(m.as_ptr(), size)
            }
        } else {
            &mut stkbuf[..size.max(size_of::<usize>())]
        };
        let udata_bytes = udata.to_ne_bytes();
        if com & IOC_IN != 0 {
            if size != 0 {
                if let Err(e) = copyin(udata, &mut data[..size]) {
                    break 'out Err(e);
                }
            } else {
                data[..udata_bytes.len()].copy_from_slice(&udata_bytes);
            }
        } else if com & IOC_OUT != 0 && size != 0 {
            // Zero the buffer so the user always gets back something deterministic.
            data[..size].fill(0);
        } else if com & IOC_VOID != 0 {
            data[..udata_bytes.len()].copy_from_slice(&udata_bytes);
        }

        let int_arg = |data: &[u8]| i32::from_ne_bytes([data[0], data[1], data[2], data[3]]);
        let error = match com {
            FIONBIO => {
                if int_arg(data) != 0 {
                    fp.f_flag.fetch_or(FNONBLOCK as u32, Ordering::SeqCst);
                } else {
                    fp.f_flag.fetch_and(!(FNONBLOCK as u32), Ordering::SeqCst);
                }
                Ok(())
            }
            FIOASYNC => {
                let tmp = int_arg(data);
                if tmp != 0 {
                    fp.f_flag.fetch_or(FASYNC as u32, Ordering::SeqCst);
                } else {
                    fp.f_flag.fetch_and(!(FASYNC as u32), Ordering::SeqCst);
                }
                let mut tmp = tmp.to_ne_bytes();
                (fp.ops().fo_ioctl)(fp, FIOASYNC, &mut tmp, p)
            }
            _ => (fp.ops().fo_ioctl)(fp, com, data, p),
        };
        // Copy any data to user, size was already set and checked above.
        if error.is_ok() && com & IOC_OUT != 0 && size != 0 {
            copyout(&data[..size], udata)
        } else {
            error
        }
    };
    let _ = frele(fp, p);
    if let Some(m) = memp {
        free(m, M_IOCTLOPS, size);
    }
    error
}

/// Select system call.
pub fn sys_select(p: &Proc, v: &SysArgs, retval: &mut [Register; 2]) -> Result<(), Errno> {
    let uap: &SysSelectArgs = sysargs(v);

    let mut tsp = None;
    let utv = uap.tv.get() as usize;
    if utv != 0 {
        let tv: Timeval = copyin_obj(utv)?;
        // KTRACE: not configured.
        if tv.tv_sec < 0 || !tv.is_valid() {
            return Err(Errno::EINVAL);
        }
        tsp = Some(timeval_to_timespec(&tv));
    }

    dopselect(
        p,
        uap.nd.get(),
        [
            uap.r#in.get() as usize,
            uap.ou.get() as usize,
            uap.ex.get() as usize,
        ],
        tsp,
        None,
        retval,
    )
}

/// `pselect(2)`.
pub fn sys_pselect(p: &Proc, v: &SysArgs, retval: &mut [Register; 2]) -> Result<(), Errno> {
    let uap: &SysPselectArgs = sysargs(v);

    let mut tsp = None;
    let uts = uap.ts.get() as usize;
    if uts != 0 {
        let ts: Timespec = copyin_obj(uts)?;
        // KTRACE: not configured.
        if ts.tv_sec < 0 || !ts.is_valid() {
            return Err(Errno::EINVAL);
        }
        tsp = Some(ts);
    }
    let mut ssp = None;
    let umask = uap.mask.get() as usize;
    if umask != 0 {
        let mut b = [0u8; size_of::<Sigset>()];
        copyin(umask, &mut b)?;
        ssp = Some(Sigset::from_ne_bytes(b));
    }

    dopselect(
        p,
        uap.nd.get(),
        [
            uap.r#in.get() as usize,
            uap.ou.get() as usize,
            uap.ex.get() as usize,
        ],
        tsp,
        ssp,
        retval,
    )
}

/// `dopselect`: the body of `select(2)` and `pselect(2)` over the user fd sets `sets` (in,
/// out, except; 0 for NULL).
pub fn dopselect(
    p: &Proc,
    nd: i32,
    sets: [usize; 3],
    timeout: Option<Timespec>,
    sigmask: Option<Sigset>,
    retval: &mut [Register; 2],
) -> Result<(), Errno> {
    let mut timeout = timeout;
    let mut ncollected = 0usize;
    let mut nevents;

    if nd < 0 {
        return Err(Errno::EINVAL);
    }

    let nfiles = p.fd().fd_nfiles.get();
    let nd = nd.min(nfiles) as usize;

    // ni bytes per set; the six sets (three in, three out) are one zeroed allocation (the C
    // keeps up to one mask each on its stack).
    let words = howmany(nd, NFDBITS);
    let ni = words * size_of::<FdMask>();
    let mut bits: Vec<FdMask> = vec![0; 6 * words];
    let (pibits, pobits) = bits.split_at_mut(3 * words);

    kqpoll_init(p, nd as u32)?;

    let error = 'done: {
        // getbits
        for (x, &name) in sets.iter().enumerate() {
            if name != 0 && ni != 0 {
                let mut raw = vec![0u8; ni];
                if let Err(e) = copyin(name, &mut raw) {
                    break 'done Err(e);
                }
                for (w, chunk) in pibits[x * words..(x + 1) * words]
                    .iter_mut()
                    .zip(raw.as_chunks::<{ size_of::<FdMask>() }>().0)
                {
                    *w = FdMask::from_ne_bytes(*chunk);
                }
            }
        }
        // KTRACE: not configured.

        if let Some(mask) = sigmask {
            dosigsuspend(p, mask & !sigcantmask());
        }

        // Register kqueue events
        match pselregister(p, pibits, words, nd) {
            Ok(n) => nevents = n,
            Err(e) => break 'done Err(e),
        }

        // The poll/select family of syscalls has been designed to block when file
        // descriptors are not available, even if there's nothing to wait for.
        if nevents == 0 && ncollected == 0 {
            let mut nsecs = INFSLP;

            if let Some(ts) = timeout {
                if !ts.is_set() {
                    break 'done Ok(());
                }
                nsecs = timespec_to_nsec(&ts).clamp(1, MAXTSLP);
            }
            let error = tsleep_nsec(nowake(), PSOCK | PCATCH, "kqsel", nsecs);
            // select is not restarted after signals...
            break 'done match error {
                Err(Errno::ERESTART) => Err(Errno::EINTR),
                Err(Errno::EWOULDBLOCK) => Ok(()),
                other => other,
            };
        }

        // Do not block if registering found pending events.
        if ncollected > 0 {
            timeout = Some(Timespec::new(0, 0));
        }

        // Collect at most `nevents' possibly waiting in kqueue_scan()
        let scan = KqueueScanState::new();
        // SAFETY: `scan` is a local that stays in place until `kqueue_scan_finish` below.
        unsafe { kqueue_scan_setup(&scan, p.kq()) };
        let mut error = Ok(());
        while nevents > 0 {
            let mut kev: [Kevent; KQ_NEVENTS] = [Kevent::default(); KQ_NEVENTS];
            // Maximum number of events per iteration
            let count = kev.len().min(nevents);
            let ready = kqueue_scan(&scan, count, &mut kev, timeout.as_mut(), p, &mut error);

            // Convert back events that are ready.
            let mut i = 0;
            while i < ready && error.is_ok() {
                error = pselcollect(p, &kev[i], pobits, words, &mut ncollected);
                i += 1;
            }
            // Stop if there was an error or if we had enough space to collect all events
            // that were ready.
            if error.is_err() || ready < count {
                break;
            }

            nevents -= ready;
        }
        kqueue_scan_finish(&scan);
        retval[0] = ncollected as Register;
        error
    };

    // done: putbits
    let mut error = error;
    if error.is_ok() {
        for (x, &name) in sets.iter().enumerate() {
            if name != 0 && ni != 0 {
                let mut raw = vec![0u8; ni];
                for (w, chunk) in pobits[x * words..(x + 1) * words]
                    .iter()
                    .zip(raw.as_chunks_mut::<{ size_of::<FdMask>() }>().0)
                {
                    chunk.copy_from_slice(&w.to_ne_bytes());
                }
                if let Err(error2) = copyout(&raw, name) {
                    error = Err(error2);
                }
            }
        }
        // KTRACE: not configured.
    }

    kqpoll_done(p, nd as u32);

    error
}

/// Convert fd_set into kqueue events and register them on the per-thread queue; returns how
/// many were registered. `pibits` holds the three input sets, `words` masks each.
pub fn pselregister(p: &Proc, pibits: &[FdMask], words: usize, nfd: usize) -> Result<usize, Errno> {
    const EVF: [i16; 3] = [EVFILT_READ, EVFILT_WRITE, EVFILT_EXCEPT];
    const EVFF: [u32; 3] = [0, 0, NOTE_OOB];
    let mut nevents = 0;

    for msk in 0..3 {
        let mut i = 0;
        while i < nfd {
            let mut bits = pibits[msk * words + i / NFDBITS];
            while bits != 0 {
                let j = bits.trailing_zeros() as usize;
                let fd = i + j;
                if fd >= nfd {
                    break;
                }
                bits &= !(1 << j);

                let mut kev = ev_set(
                    fd,
                    EVF[msk],
                    EV_ADD | EV_ENABLE | __EV_SELECT,
                    EVFF[msk],
                    0,
                    p.p_kq_serial.get() as usize,
                );
                match kqueue_register(p.kq(), &mut kev, 0, Some(p)) {
                    Ok(()) => nevents += 1,
                    // No underlying kqfilter, unimplemented filter, specific to FIFO and
                    // __EV_SELECT
                    Err(Errno::EOPNOTSUPP | Errno::EINVAL | Errno::EPERM) => {}
                    // Device has been detached, and the rest
                    Err(e) => return Err(e),
                }
            }
            i += NFDBITS;
        }
    }

    Ok(nevents)
}

/// Convert given kqueue event into corresponding select(2) bit.
pub fn pselcollect(
    p: &Proc,
    kevp: &Kevent,
    pobits: &mut [FdMask],
    words: usize,
    ncollected: &mut usize,
) -> Result<(), Errno> {
    if kevp.udata as u64 != p.p_kq_serial.get() {
        crate::kern::subr_prf::panic(format_args!(
            "pselcollect: spurious kevp fd {} udata {:#x} serial {:#x}",
            kevp.ident,
            kevp.udata,
            p.p_kq_serial.get()
        ));
    }

    if kevp.flags & EV_ERROR != 0 {
        return Err(Errno::from_raw(kevp.data as i32).unwrap_or(Errno::EINVAL));
    }

    let set = match kevp.filter {
        EVFILT_READ => 0,
        EVFILT_WRITE => 1,
        EVFILT_EXCEPT => 2,
        _ => {
            kassert!(false);
            return Ok(());
        }
    };
    fd_set(kevp.ident, &mut pobits[set * words..(set + 1) * words]);
    *ncollected += 1;

    Ok(())
}

/// `selwakeup`: do a wakeup when a selectable event occurs.
pub fn selwakeup(sip: &Selinfo) {
    kernel_lock();
    knote_locked(&sip.si_note, i64::from(NOTE_SUBMIT));
    kernel_unlock();
}

/// Only copyout the revents field.
pub fn pollout(pl: &[Pollfd], upl: usize) -> Result<(), Errno> {
    for (i, pfd) in pl.iter().enumerate() {
        copyout(
            &pfd.revents.to_ne_bytes(),
            upl + i * size_of::<Pollfd>() + offset_of!(Pollfd, revents),
        )?;
    }
    Ok(())
}

/// We are using the same mechanism as select only we encode/decode args differently.
pub fn sys_poll(p: &Proc, v: &SysArgs, retval: &mut [Register; 2]) -> Result<(), Errno> {
    let uap: &SysPollArgs = sysargs(v);

    let mut tsp = None;
    let msec = uap.timeout.get();

    if msec != INFTIM {
        if msec < 0 {
            return Err(Errno::EINVAL);
        }
        let sec = i64::from(msec / 1000);
        tsp = Some(Timespec::new(
            sec,
            (i64::from(msec) - sec * 1000) * 1_000_000,
        ));
    }

    doppoll(p, uap.fds.get() as usize, uap.nfds.get(), tsp, None, retval)
}

/// `ppoll(2)`.
pub fn sys_ppoll(p: &Proc, v: &SysArgs, retval: &mut [Register; 2]) -> Result<(), Errno> {
    let uap: &SysPpollArgs = sysargs(v);

    let mut tsp = None;
    let uts = uap.ts.get() as usize;
    if uts != 0 {
        let ts: Timespec = copyin_obj(uts)?;
        // KTRACE: not configured.
        if ts.tv_sec < 0 || !ts.is_valid() {
            return Err(Errno::EINVAL);
        }
        tsp = Some(ts);
    }

    let mut ssp = None;
    let umask = uap.mask.get() as usize;
    if umask != 0 {
        let mut b = [0u8; size_of::<Sigset>()];
        copyin(umask, &mut b)?;
        ssp = Some(Sigset::from_ne_bytes(b));
    }

    doppoll(p, uap.fds.get() as usize, uap.nfds.get(), tsp, ssp, retval)
}

/// `doppoll`: the body of `poll(2)` and `ppoll(2)` over the user array `fds`.
pub fn doppoll(
    p: &Proc,
    fds: usize,
    nfds: u32,
    timeout: Option<Timespec>,
    sigmask: Option<Sigset>,
    retval: &mut [Register; 2],
) -> Result<(), Errno> {
    let mut timeout = timeout;
    let mut ncollected = 0usize;
    let mut nevents = 0usize;

    // Standards say no more than MAX_OPEN; this is possibly better.
    let limit = (lim_cur(RLIMIT_NOFILE) as i64).min(i64::from(MAXFILES.load(Ordering::Relaxed)));
    if i64::from(nfds) > limit {
        return Err(Errno::EINVAL);
    }

    // The array (the C keeps up to four on its stack and mallocs M_CANFAIL beyond).
    let mut pl: Vec<Pollfd> = vec![Pollfd::default(); nfds as usize];

    kqpoll_init(p, nfds)?;

    let sz = nfds as usize * size_of::<Pollfd>();
    let mut raw = vec![0u8; sz];
    if let Err(error) = copyin(fds, &mut raw) {
        // bad:
        kqpoll_done(p, nfds);
        return Err(error);
    }
    for (pfd, chunk) in pl
        .iter_mut()
        .zip(raw.as_chunks::<{ size_of::<Pollfd>() }>().0)
    {
        let word = |o: usize| {
            let mut b = [0u8; 2];
            b.copy_from_slice(&chunk[o..o + 2]);
            i16::from_ne_bytes(b)
        };
        let mut fd = [0u8; 4];
        fd.copy_from_slice(&chunk[0..4]);
        *pfd = Pollfd {
            fd: i32::from_ne_bytes(fd),
            events: word(offset_of!(Pollfd, events)),
            revents: word(offset_of!(Pollfd, revents)),
        };
    }

    if let Some(mask) = sigmask {
        dosigsuspend(p, mask & !sigcantmask());
    }

    // Register kqueue events
    ppollregister(p, &mut pl, &mut nevents, &mut ncollected);

    let error = 'done: {
        // The poll/select family of syscalls has been designed to block when file
        // descriptors are not available, even if there's nothing to wait for.
        if nevents == 0 && ncollected == 0 {
            let mut nsecs = INFSLP;

            if let Some(ts) = timeout {
                if !ts.is_set() {
                    break 'done Ok(());
                }
                nsecs = timespec_to_nsec(&ts).clamp(1, MAXTSLP);
            }

            let error = tsleep_nsec(nowake(), PSOCK | PCATCH, "kqpoll", nsecs);
            break 'done match error {
                Err(Errno::ERESTART) => Err(Errno::EINTR),
                Err(Errno::EWOULDBLOCK) => Ok(()),
                other => other,
            };
        }

        // Do not block if registering found pending events.
        if ncollected > 0 {
            timeout = Some(Timespec::new(0, 0));
        }

        // Collect at most `nevents' possibly waiting in kqueue_scan()
        let scan = KqueueScanState::new();
        // SAFETY: `scan` is a local that stays in place until `kqueue_scan_finish` below.
        unsafe { kqueue_scan_setup(&scan, p.kq()) };
        let mut error = Ok(());
        while nevents > 0 {
            let mut kev: [Kevent; KQ_NEVENTS] = [Kevent::default(); KQ_NEVENTS];
            // Maximum number of events per iteration
            let count = kev.len().min(nevents);
            let ready = kqueue_scan(&scan, count, &mut kev, timeout.as_mut(), p, &mut error);

            // Convert back events that are ready.
            for k in &kev[..ready] {
                ncollected += ppollcollect(p, k, &mut pl);
            }

            // Stop if there was an error or if we had enough place to collect all events
            // that were ready.
            if error.is_err() || ready < count {
                break;
            }

            nevents -= ready;
        }
        kqueue_scan_finish(&scan);
        retval[0] = ncollected as Register;
        error
    };

    // done: NOTE: poll(2) is not restarted after a signal and EWOULDBLOCK is ignored (since
    // the whole point is to see what would block).
    let error = match error {
        Err(Errno::EINTR) => pollout(&pl, fds).and(Err(Errno::EINTR)),
        Err(Errno::EWOULDBLOCK) | Ok(()) => pollout(&pl, fds),
        other => other,
    };
    // KTRACE: not configured.

    kqpoll_done(p, nfds);

    error
}

/// `ppollregister_evts`: registers the `kev` events of one pollfd; failures become its
/// `revents`. Returns how many were registered.
pub fn ppollregister_evts(p: &Proc, kev: &mut [Kevent], pl: &mut Pollfd, pollid: u32) -> usize {
    let mut nevents = 0;
    let nkev = kev.len();

    kassert!(pl.revents == 0);

    for kevp in kev.iter_mut() {
        loop {
            match kqueue_register(p.kq(), kevp, pollid, Some(p)) {
                Ok(()) => nevents += 1,
                // No underlying kqfilter, unimplemented filter
                Err(Errno::EOPNOTSUPP | Errno::EINVAL) => {}
                // Bad file descriptor
                Err(Errno::EBADF) => pl.revents |= POLLNVAL,
                // Specific to FIFO
                Err(Errno::EPERM) => {
                    kassert!(kevp.filter == EVFILT_WRITE);
                    if nkev == 1 {
                        // If this is the only filter make sure POLLHUP is passed to
                        // userland.
                        kevp.filter = EVFILT_EXCEPT;
                        continue;
                    }
                }
                // Device has been detached, and the rest
                Err(_) => pl.revents |= POLLERR,
            }
            break;
        }
    }

    nevents
}

/// Convert pollfd into kqueue events and register them on the per-thread queue. At most 3
/// events can correspond to a single pollfd.
pub fn ppollregister(p: &Proc, pl: &mut [Pollfd], nregistered: &mut usize, ncollected: &mut usize) {
    for (i, pfd) in pl.iter_mut().enumerate() {
        pfd.events &= !POLL_NOHUP;
        pfd.revents = 0;

        if pfd.fd < 0 {
            continue;
        }

        // POLLHUP checking is implicit in the event filters. However, the checking must be
        // even if no events are requested.
        let forcehup = pfd.events & !POLLHUP == 0;

        let mut kev = [Kevent::default(); 3];
        let mut nkev = 0;
        let udata = (p.p_kq_serial.get() + i as u64) as usize;
        if pfd.events & (POLLIN | POLLRDNORM) != 0 {
            kev[nkev] = ev_set(
                pfd.fd as usize,
                EVFILT_READ,
                EV_ADD | EV_ENABLE | __EV_POLL,
                0,
                0,
                udata,
            );
            nkev += 1;
        }
        if pfd.events & (POLLOUT | POLLWRNORM) != 0 {
            kev[nkev] = ev_set(
                pfd.fd as usize,
                EVFILT_WRITE,
                EV_ADD | EV_ENABLE | __EV_POLL,
                0,
                0,
                udata,
            );
            nkev += 1;
        }
        if pfd.events & (POLLPRI | POLLRDBAND) != 0 || forcehup {
            let evff = if forcehup { 0 } else { NOTE_OOB };

            kev[nkev] = ev_set(
                pfd.fd as usize,
                EVFILT_EXCEPT,
                EV_ADD | EV_ENABLE | __EV_POLL,
                evff,
                0,
                udata,
            );
            nkev += 1;
        }

        if nkev == 0 {
            continue;
        }

        *nregistered += ppollregister_evts(p, &mut kev[..nkev], pfd, i as u32);

        if pfd.revents != 0 {
            *ncollected += 1;
        }
    }
}

/// Convert given kqueue event into corresponding poll(2) revents bit; 1 when it is the
/// first event of its pollfd.
pub fn ppollcollect(p: &Proc, kevp: &Kevent, pl: &mut [Pollfd]) -> usize {
    static POLL_LASTERR: StaticCell<Timeval> = StaticCell::new(Timeval::new(0, 0));
    const POLL_ERRINTVL: Timeval = Timeval::new(5, 0);

    // Extract poll array index
    let serial = p.p_kq_serial.get();
    let i = (kevp.udata as u64).wrapping_sub(serial) as usize;
    let nfds = pl.len();

    if i >= nfds {
        crate::kern::subr_prf::panic(format_args!(
            "ppollcollect: spurious kevp nfds {nfds} udata {:#x} serial {serial:#x}",
            kevp.udata
        ));
    }
    if kevp.ident as i32 != pl[i].fd {
        crate::kern::subr_prf::panic(format_args!(
            "ppollcollect: kevp {}/{} mismatch fd {}!={} serial {serial:#x}",
            i + 1,
            nfds,
            kevp.ident as i32,
            pl[i].fd
        ));
    }

    // A given descriptor may already have generated an error against another filter during
    // kqueue_register().
    //
    // Make sure to set the appropriate flags but do not increment `*retval' more than once.
    let already_seen = pl[i].revents != 0;
    let pfd = &mut pl[i];

    'done: {
        // POLLNVAL preempts other events.
        if kevp.flags & EV_ERROR != 0 && kevp.data == Errno::EBADF as i64 {
            pfd.revents = POLLNVAL;
            break 'done;
        } else if pfd.revents & POLLNVAL != 0 {
            break 'done;
        }

        match kevp.filter {
            EVFILT_READ => {
                if kevp.flags & __EV_HUP != 0 {
                    pfd.revents |= POLLHUP;
                }
                if pfd.events & (POLLIN | POLLRDNORM) != 0 {
                    pfd.revents |= pfd.events & (POLLIN | POLLRDNORM);
                }
            }
            EVFILT_WRITE => {
                // POLLHUP and POLLOUT/POLLWRNORM are mutually exclusive
                if kevp.flags & __EV_HUP != 0 {
                    pfd.revents |= POLLHUP;
                } else if pfd.events & (POLLOUT | POLLWRNORM) != 0 {
                    pfd.revents |= pfd.events & (POLLOUT | POLLWRNORM);
                }
            }
            EVFILT_EXCEPT => {
                if kevp.flags & __EV_HUP != 0 {
                    pfd.revents |= POLLHUP;
                    break 'done;
                }
                if pfd.events & (POLLPRI | POLLRDBAND) != 0 {
                    pfd.revents |= pfd.events & (POLLPRI | POLLRDBAND);
                }
            }
            _ => {
                kassert!(false);
            }
        }
    }

    // Make noise about unclaimed events as they might indicate a bug and can result in
    // spurious-looking wakeups of poll(2).
    //
    // Live-locking within the system call should not happen because the scan loop in
    // doppoll() has an upper limit for the number of events to process.
    if pfd.revents == 0 && {
        // The C reads and writes the static without a lock (a benign race); here the kernel
        // lock serialises the rate limiter, which `SY_NOLOCK` callers reach concurrently.
        kernel_lock();
        // SAFETY: the rate limiter is touched only here, under the kernel lock.
        let noisy = ratecheck(unsafe { POLL_LASTERR.get_mut() }, &POLL_ERRINTVL);
        kernel_unlock();
        noisy
    } {
        kprintf!(
            "{}[{}]: poll index {} fd {} events 0x{:x} filter {}/0x{:x} unclaimed\n",
            Str(p.process().comm()),
            p.p_tid.get(),
            i,
            pfd.fd,
            pfd.events,
            kevp.filter,
            kevp.flags
        );
    }

    usize::from(!already_seen && pfd.revents != 0)
}

/// `utrace(2)`: a user record for the process's `ktrace(1)` trace. `KTRACE` is not
/// configured, so there is no trace to add to and the call succeeds, as the C's `#else`.
pub fn sys_utrace(_curp: &Proc, _v: &SysArgs, _retval: &mut [Register; 2]) -> Result<(), Errno> {
    // KTRACE: ktruser(curp, label, addr, len).
    Ok(())
}

#[cfg(test)]
mod tests;
