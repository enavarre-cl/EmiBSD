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
//! `sys_ioctl` reach the file through `fd_getfile_mode` and call its `fileops`. `select`,
//! `pselect`, `poll`, `ppoll` and their kqueue helpers (`pselregister`, `ppollregister`,
//! `pollout`, ...) wait for `kern_event.c`.
//!
//! ## Deviations
//! - `iovec_copyin(uiov, aiov, iovcnt)` returns the iovecs (the caller's `aiov` or a
//!   `malloc(M_IOV)` array) and the residual count, and frees its own array when it fails,
//!   so `iovec_free` (`unsafe`: it frees) is only called on a successful result. The user
//!   array is copied in one iovec at a time, each read through `Iovec::from_bytes`.
//! - `dofilereadv`/`dofilewritev` take a `Uio` whose iovecs borrow the caller's array; the
//!   positioned checks (`FO_POSITION`) answer `ESPIPE` for every file that is not a vnode
//!   (and for fifos and ttys), as in C.
//! - `EPIPE` from a write would post `SIGPIPE` through `ptsignal`, which `kern_sig.c` brings;
//!   it is reported until then.
//! - `sys_ioctl`'s `pledge_ioctl` and the socket `SS_DNS` check are reported where the C
//!   makes them: only a pledged process (none can be yet) or a socket (none exist) reaches
//!   them. The argument buffer is a byte slice of `max(IOCPARM_LEN(com), sizeof(caddr_t))`
//!   bytes, from the 128-byte stack buffer or `malloc(M_IOCTLOPS)`.
//! - `KTRACE` is not configured.

use core::ptr::NonNull;
use core::sync::atomic::Ordering;

use crate::kassert;
use crate::kern::kern_descrip::fd_getfile_mode;
use crate::kern::kern_lock::{mtx_enter, mtx_leave};
use crate::kern::kern_malloc::{free, malloc, mallocarray};
use crate::machine::copy::{copyin, copyout};
use crate::sys::errno::Errno;
use crate::sys::fcntl::{FASYNC, FNONBLOCK, FREAD, FWRITE};
use crate::sys::file::{DTYPE_SOCKET, DTYPE_VNODE, FO_POSITION, File, frele};
use crate::sys::filedesc::{UF_EXCLOSE, UF_PLEDGEOPEN, fdplock, fdpunlock};
use crate::sys::filio::{FIOASYNC, FIOCLEX, FIONBIO, FIONCLEX};
use crate::sys::ioccom::{IOC_IN, IOC_OUT, IOC_VOID, IOCPARM_MAX, iocparm_len};
use crate::sys::limits::SSIZE_MAX;
use crate::sys::malloc::{M_IOCTLOPS, M_IOV, M_WAITOK};
use crate::sys::proc::{PS_PLEDGE, Proc};
use crate::sys::syscallargs::{
    SysIoctlArgs, SysReadArgs, SysReadvArgs, SysWriteArgs, SysWritevArgs,
};
use crate::sys::syslimits::IOV_MAX;
use crate::sys::systm::{SysArgs, sysargs};
use crate::sys::types::{Off, Register};
use crate::sys::uio::{Iovec, UIO_SMALLIOV, Uio, UioRw, UioSeg};
use crate::sys::vnode::{VCHR, VFIFO, VISTTY};
use crate::unported;

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
            // ptsignal(p, SIGPIPE, STHREAD): kern_sig.c.
            let _ = unported!("dofilewritev: ptsignal(SIGPIPE) (kern_sig.c)");
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
            // so->so_state & SS_DNS: sockets (M7b).
            break 'out Err(unported!("ioctl: SS_DNS (sockets)"));
        }

        // pledge_ioctl(p, com, fp): kern_pledge.c.
        if p.process().ps_flags.load(Ordering::Relaxed) & PS_PLEDGE != 0 {
            break 'out Err(unported!("pledge_ioctl (kern_pledge.c)"));
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
