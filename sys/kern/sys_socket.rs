/*	$OpenBSD: sys_socket.c,v 1.68 2025/02/13 12:39:15 bluhm Exp $	*/
/*	$NetBSD: sys_socket.c,v 1.13 1995/08/12 23:59:09 mycroft Exp $	*/
/* <LICENSES> */
/*
 * Copyright (c) 1982, 1986, 1990, 1993
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
 *	@(#)sys_socket.c	8.1 (Berkeley) 6/10/93
 */
/* </LICENSES> */

//! The file operations of sockets: `kern/sys_socket.c`.
//!
//! Upstream: sys/kern/sys_socket.c @ 3ce1f3f79392
//!
//! A `DTYPE_SOCKET` file points its `f_data` at its socket; `socketops` turns `read(2)`,
//! `write(2)`, `ioctl(2)`, `fstat(2)` and the last `close(2)` into `soreceive`, `sosend`,
//! the socket ioctls (or the interface's and the protocol's), a socket `stat` and
//! `soclose`.
//!
//! ## Deviations
//! - `fo_ioctl`'s `data` is the kernel copy of the argument (`sys_ioctl`); the `int`
//!   arguments are read and written in native byte order. `ifioctl` wants its request
//!   aligned for the structure the command names: a request in `sys_ioctl`'s byte buffer on
//!   the stack (at most `STK_PARAMS`, 128 bytes) is copied through an aligned buffer here.
//! - [`fp_socket`] is `fp->f_data` cast to the socket, checking the type as the C assumes.

use core::ffi::c_void;
use core::ptr;
use core::sync::atomic::Ordering;

use crate::kern::kern_lock::{mtx_enter, mtx_leave};
use crate::kern::kern_sig::{sigio_getown, sigio_setown};
use crate::kern::subr_prf::panic;
use crate::kern::uipc_socket::{soclose, soo_kqfilter, soreceive, sosend};
use crate::kern::uipc_socket2::{solock_shared, sounlock_shared};
use crate::net::if_::ifioctl;
use crate::sys::errno::Errno;
use crate::sys::fcntl::FNONBLOCK;
use crate::sys::file::{DTYPE_SOCKET, File, Fileops};
use crate::sys::filio::{FIOASYNC, FIOGETOWN, FIONREAD, FIOSETOWN};
use crate::sys::ioccom::iocgroup;
use crate::sys::proc::Proc;
use crate::sys::protosw::{pru_control, pru_sense};
use crate::sys::socket::MSG_DONTWAIT;
use crate::sys::socketvar::{SB_ASYNC, SS_CANTRCVMORE, SS_CANTSENDMORE, SS_RCVATMARK, Socket};
use crate::sys::sockio::{SIOCATMARK, SIOCGPGRP, SIOCSPGRP};
use crate::sys::stat::{S_IFSOCK, S_IRGRP, S_IROTH, S_IRUSR, S_IWGRP, S_IWOTH, S_IWUSR, Stat};
use crate::sys::ttycom::{TIOCGPGRP, TIOCSPGRP};
use crate::sys::uio::Uio;

/// `socketops`.
pub static SOCKETOPS: Fileops = Fileops {
    fo_read: soo_read,
    fo_write: soo_write,
    fo_ioctl: soo_ioctl,
    fo_kqfilter: soo_kqfilter,
    fo_stat: soo_stat,
    fo_close: soo_close,
    fo_seek: None,
};

/// `(struct socket *)fp->f_data` of a socket file.
pub fn fp_socket(fp: &File) -> &'static Socket {
    if fp.f_type.get() != DTYPE_SOCKET {
        panic(format_args!("file {:p}: not a socket", fp));
    }
    // SAFETY: every `DTYPE_SOCKET` file's `f_data` is set to a live socket before the file
    // is inserted in a table; the file holds the socket's file reference until `soo_close`
    // (which clears `f_data`) runs on its last reference, after every user of the file.
    match unsafe { fp.f_data.get().cast::<Socket>().cast_const().as_ref() } {
        Some(so) => so,
        None => panic(format_args!("file {:p}: socket already closed", fp)),
    }
}

/// The `int` an `ioctl` argument holds.
fn int_arg(data: &[u8]) -> i32 {
    data.first_chunk::<4>()
        .map_or(0, |b| i32::from_ne_bytes(*b))
}

/// Stores an `int` into an `ioctl` argument.
fn set_int_arg(data: &mut [u8], v: i32) {
    if let Some(b) = data.first_chunk_mut::<4>() {
        *b = v.to_ne_bytes();
    }
}

/// `soo_read(fp, uio, fflags)`: `read(2)` on a socket.
pub fn soo_read(fp: &File, uio: &mut Uio<'_>, _fflags: i32) -> Result<(), Errno> {
    let so = fp_socket(fp);
    let mut flags = 0;

    if fp.flag() & FNONBLOCK != 0 {
        flags |= MSG_DONTWAIT;
    }

    soreceive(so, None, uio, None, None, Some(&mut flags), 0)
}

/// `soo_write(fp, uio, fflags)`: `write(2)` on a socket.
pub fn soo_write(fp: &File, uio: &mut Uio<'_>, _fflags: i32) -> Result<(), Errno> {
    let so = fp_socket(fp);
    let mut flags = 0;

    if fp.flag() & FNONBLOCK != 0 {
        flags |= MSG_DONTWAIT;
    }

    sosend(so, None, Some(uio), None, None, flags)
}

/// `ifioctl` over `data`, through an aligned copy when `data` is not aligned for the
/// interface request (see the module's deviations).
fn soo_ifioctl(so: &Socket, cmd: u64, data: &mut [u8], p: &Proc) -> Result<(), Errno> {
    let so = ptr::from_ref(so).cast::<c_void>();
    if data.as_ptr().align_offset(align_of::<u64>()) == 0 {
        // SAFETY: `data` is the kernel copy of the request, as long as the command says
        // (`sys_ioctl`) and aligned (checked).
        return unsafe { ifioctl(so, cmd, data.as_mut_ptr(), p) };
    }
    // sys_ioctl's on-stack buffer: at most STK_PARAMS bytes.
    let mut aligned = [0u64; 16];
    let len = data.len().min(size_of_val(&aligned));
    let bytes = aligned.as_mut_ptr().cast::<u8>();
    // SAFETY: both buffers hold `len` bytes and do not overlap.
    unsafe { ptr::copy_nonoverlapping(data.as_ptr(), bytes, len) };
    // SAFETY: an aligned copy of the request, as long as `data` (at most 128 bytes).
    let error = unsafe { ifioctl(so, cmd, bytes, p) };
    // SAFETY: as above, back into the caller's buffer.
    unsafe { ptr::copy_nonoverlapping(bytes, data.as_mut_ptr(), len) };
    error
}

/// `soo_ioctl(fp, cmd, data, p)`: the socket ioctls; the interface ones go to `ifioctl`,
/// the protocol ones to `pru_control`.
pub fn soo_ioctl(fp: &File, cmd: u64, data: &mut [u8], p: &Proc) -> Result<(), Errno> {
    let so = fp_socket(fp);

    match cmd {
        FIOASYNC => {
            mtx_enter(&so.so_rcv.sb_mtx);
            mtx_enter(&so.so_snd.sb_mtx);
            if int_arg(data) != 0 {
                so.so_rcv.set_flags(SB_ASYNC);
                so.so_snd.set_flags(SB_ASYNC);
            } else {
                so.so_rcv.clear_flags(SB_ASYNC);
                so.so_snd.clear_flags(SB_ASYNC);
            }
            mtx_leave(&so.so_snd.sb_mtx);
            mtx_leave(&so.so_rcv.sb_mtx);
        }

        FIONREAD => set_int_arg(data, so.so_rcv.sb_datacc.get() as i32),

        FIOSETOWN | SIOCSPGRP | TIOCSPGRP => {
            return sigio_setown(&so.so_sigio, cmd, &int_arg(data));
        }

        FIOGETOWN | SIOCGPGRP | TIOCGPGRP => {
            let mut pgid = 0;
            sigio_getown(&so.so_sigio, cmd, &mut pgid);
            set_int_arg(data, pgid);
        }

        SIOCATMARK => set_int_arg(data, i32::from(so.so_rcv.has_state(SS_RCVATMARK))),

        _ => {
            // Interface/routing/protocol specific ioctls: interface and routing ioctls
            // should have a different entry since a socket's unnecessary
            if iocgroup(cmd) == u64::from(b'i') {
                return soo_ifioctl(so, cmd, data, p);
            }
            if iocgroup(cmd) == u64::from(b'r') {
                return Err(Errno::EOPNOTSUPP);
            }
            return pru_control(so, cmd, data, None);
        }
    }

    Ok(())
}

/// `soo_stat(fp, ub, p)`: `fstat(2)` on a socket.
pub fn soo_stat(fp: &File, ub: &mut Stat, _p: &Proc) -> Result<(), Errno> {
    let so = fp_socket(fp);

    *ub = Stat::default();
    ub.st_mode = S_IFSOCK;
    solock_shared(so);
    mtx_enter(&so.so_rcv.sb_mtx);
    if !so.so_rcv.has_state(SS_CANTRCVMORE) || so.so_rcv.sb_cc.get() != 0 {
        ub.st_mode |= S_IRUSR | S_IRGRP | S_IROTH;
    }
    mtx_leave(&so.so_rcv.sb_mtx);
    mtx_enter(&so.so_snd.sb_mtx);
    if !so.so_snd.has_state(SS_CANTSENDMORE) {
        ub.st_mode |= S_IWUSR | S_IWGRP | S_IWOTH;
    }
    mtx_leave(&so.so_snd.sb_mtx);
    ub.st_uid = so.so_euid.get();
    ub.st_gid = so.so_egid.get();
    let _ = pru_sense(so, ub);
    sounlock_shared(so);
    Ok(())
}

/// `soo_close(fp, p)`: the last reference to a socket file is gone: close the socket.
pub fn soo_close(fp: &File, _p: Option<&Proc>) -> Result<(), Errno> {
    let mut error = Ok(());

    if !fp.f_data.get().is_null() {
        let flags = if fp.f_flag.load(Ordering::Relaxed) & FNONBLOCK as u32 != 0 {
            MSG_DONTWAIT
        } else {
            0
        };
        error = soclose(fp_socket(fp), flags);
    }
    fp.f_data.set(ptr::null_mut());
    error
}
