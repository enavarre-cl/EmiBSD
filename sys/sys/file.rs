/*	$OpenBSD: file.h,v 1.67 2026/09/19 17:21:52 dv Exp $	*/
/*	$NetBSD: file.h,v 1.11 1995/03/26 20:24:13 jtc Exp $	*/
/* <LICENSES> */
/*
 * Copyright (c) 1982, 1986, 1989, 1993
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
 *	@(#)file.h	8.2 (Berkeley) 8/20/94
 */
/* </LICENSES> */

//! `<sys/file.h>`: the kernel descriptor table entry, `struct file`, with its operations
//! vector `struct fileops`, the `DTYPE_*` descriptor types, the `FIF_*` internal flags and
//! the reference macros `FREF`/`FRELE`.
//!
//! Upstream: sys/sys/file.h @ 3ce1f3f79392
//!
//! A `struct file` is a pool item (`file_pool`, `kern_descrip.rs`) that every descriptor
//! table pointing at it, and every thread using it, holds a reference to; it is handed around
//! as `&'static File` (the `crget`/`crfree` idiom of `docs/C_TO_RUST.md`) and goes back to the
//! pool in `fdrop` when the last reference goes.
//!
//! ## Deviations
//! - `struct fileops` is a struct of `fn` pointers returning `Result<(), Errno>`. `fo_ioctl`'s
//!   `caddr_t data` is the kernel copy of the argument as a byte slice; `fo_kqfilter`'s
//!   `struct knote *` is a raw pointer until `kern_event.c` exists; `fo_close` takes
//!   `Option<&Proc>` (a file passed in a message is closed with no process); `fo_seek` is an
//!   `Option` because the C leaves it NULL for most types.
//! - The `[a]`/`[f]`/`[I]` members are atomics (`f_flag`, `f_iflags`, `f_count`) or `Cell`s;
//!   `f_ops`, `f_type`, `f_data` and `f_cred` are written once, right after `falloc`, before
//!   the file is inserted in a table, as in C.
//! - `FREF`/`FRELE` are the functions [`fref`]/[`frele`]; `frele` takes the thread as
//!   `impl Into<Option<&Proc>>`, as the C passes NULL from the socket garbage collector.
//!   `FREF` calls `vfs_stall_barrier()` (`vfs_subr.rs`) as in C.
//! - `maxfiles` is `conf/param.rs`'s `MAXFILES`; `numfiles` is `kern_descrip.rs`'s
//!   `NUMFILES`; `vnops` is `kern/vfs_vnops.rs`'s `VNOPS`; `socketops` arrives with
//!   sockets.

use core::cell::Cell;
use core::ffi::c_void;
use core::ptr;
use core::sync::atomic::{AtomicU32, Ordering};

use crate::kern::kern_descrip::fdrop;
use crate::kern::kern_lock::{mtx_enter, mtx_leave};
use crate::machine::cpu::MAXCPUS;
use crate::machine::intr::IPL_MPFLOOR;
use crate::queue_adapter;
use crate::sys::errno::Errno;
use crate::sys::mutex::Mutex;
use crate::sys::proc::Proc;
use crate::sys::queue::ListEntry;
use crate::sys::stat::Stat;
use crate::sys::types::Off;
use crate::sys::ucred::Ucred;
use crate::sys::uio::Uio;

/// `DTYPE_VNODE`: file.
pub const DTYPE_VNODE: i32 = 1;
/// `DTYPE_SOCKET`: communications endpoint.
pub const DTYPE_SOCKET: i32 = 2;
/// `DTYPE_PIPE`: pipe.
pub const DTYPE_PIPE: i32 = 3;
/// `DTYPE_KQUEUE`: event queue.
pub const DTYPE_KQUEUE: i32 = 4;
/// `DTYPE_DMABUF`: DMA buffer (for DRM).
pub const DTYPE_DMABUF: i32 = 5;
/// `DTYPE_SYNC`: sync file (for DRM).
pub const DTYPE_SYNC: i32 = 6;
/// `DTYPE_VMM`: `vmm(4)` virtual machine.
pub const DTYPE_VMM: i32 = 7;

/// `FO_POSITION`: positioned read/write.
pub const FO_POSITION: i32 = 0x0000_0001;

/// `FIF_HASLOCK`: descriptor holds advisory lock.
pub const FIF_HASLOCK: u32 = 0x01;
/// `FIF_INSERTED`: present in `filehead`.
pub const FIF_INSERTED: u32 = 0x80;

/// `FDUP_MAX_COUNT`: the reference count past which a file is no longer duplicated.
pub const FDUP_MAX_COUNT: u32 = u32::MAX - 2 * MAXCPUS;

/// `struct fileops`: file operations. `fo_read`, `fo_write` and `fo_close` could be called
/// without the kernel lock held.
pub struct Fileops {
    /// `fo_read(fp, uio, fflags)`.
    pub fo_read: fn(&File, &mut Uio<'_>, i32) -> Result<(), Errno>,
    /// `fo_write(fp, uio, fflags)`.
    pub fo_write: fn(&File, &mut Uio<'_>, i32) -> Result<(), Errno>,
    /// `fo_ioctl(fp, com, data, p)`: `data` is the kernel copy of the argument.
    pub fo_ioctl: fn(&File, u64, &mut [u8], &Proc) -> Result<(), Errno>,
    /// `fo_kqfilter(fp, kn)`: `kn` is a `struct knote *` (`kern_event.c`, not ported).
    pub fo_kqfilter: fn(&File, *mut c_void) -> Result<(), Errno>,
    /// `fo_stat(fp, ub, p)`.
    pub fo_stat: fn(&File, &mut Stat, &Proc) -> Result<(), Errno>,
    /// `fo_close(fp, p)`: `p` is `None` when closing a file that was in a message.
    pub fo_close: fn(&File, Option<&Proc>) -> Result<(), Errno>,
    /// `fo_seek(fp, offset, whence, p)`, NULL for the types that cannot seek.
    pub fo_seek: Option<FoSeek>,
}

/// The type of `fo_seek(fp, offset, whence, p)`.
pub type FoSeek = fn(&File, &mut Off, i32, &Proc) -> Result<(), Errno>;

/// `struct file`: kernel descriptor table. One entry for each open kernel vnode and socket.
///
/// Locks: \[I\] immutable after creation, \[F\] global `fhdlk` mutex, \[a\] atomic
/// operations, \[f\] per file `f_mtx`, \[v\] vnode lock.
pub struct File {
    /// \[F\] `f_list`: list of active files.
    pub f_list: ListEntry<File>,
    /// `f_mtx`.
    pub f_mtx: Mutex,
    /// \[a\] `f_flag`: see `fcntl.h` (`FREAD`, `FWRITE`, `FNONBLOCK`, ...).
    pub f_flag: AtomicU32,
    /// \[a\] `f_iflags`: internal flags (`FIF_*`).
    pub f_iflags: AtomicU32,
    /// \[I\] `f_type`: descriptor type (`DTYPE_*`).
    pub f_type: Cell<i32>,
    /// \[a\] `f_count`: reference count.
    pub f_count: AtomicU32,
    /// \[I\] `f_cred`: credentials associated with descriptor; the file holds a reference.
    pub f_cred: Cell<*const Ucred>,
    /// \[I\] `f_ops`: file operation pointers.
    pub f_ops: Cell<Option<&'static Fileops>>,
    /// \[f,v\] `f_offset`: offset.
    pub f_offset: Cell<Off>,
    /// \[I\] `f_data`: private data, whose type `f_type` names.
    pub f_data: Cell<*mut c_void>,
    /// \[f\] `f_rxfer`: total number of read transfers.
    pub f_rxfer: Cell<u64>,
    /// \[f\] `f_wxfer`: total number of write transfers.
    pub f_wxfer: Cell<u64>,
    /// \[f\] `f_seek`: total independent seek operations.
    pub f_seek: Cell<u64>,
    /// \[f\] `f_rbytes`: total bytes read.
    pub f_rbytes: Cell<u64>,
    /// \[f\] `f_wbytes`: total bytes written.
    pub f_wbytes: Cell<u64>,
}

impl File {
    /// A zeroed file, as `pool_get(&file_pool, PR_ZERO)` returns it.
    pub const fn new() -> Self {
        Self {
            f_list: ListEntry::new(),
            f_mtx: Mutex::new(IPL_MPFLOOR),
            f_flag: AtomicU32::new(0),
            f_iflags: AtomicU32::new(0),
            f_type: Cell::new(0),
            f_count: AtomicU32::new(0),
            f_cred: Cell::new(ptr::null()),
            f_ops: Cell::new(None),
            f_offset: Cell::new(0),
            f_data: Cell::new(ptr::null_mut()),
            f_rxfer: Cell::new(0),
            f_wxfer: Cell::new(0),
            f_seek: Cell::new(0),
            f_rbytes: Cell::new(0),
            f_wbytes: Cell::new(0),
        }
    }

    /// `fp->f_flag` as the `int` the `fcntl.h` constants are.
    pub fn flag(&self) -> i32 {
        self.f_flag.load(Ordering::Relaxed) as i32
    }

    /// `fp->f_ops`, which every inserted file has.
    pub fn ops(&self) -> &'static Fileops {
        match self.f_ops.get() {
            Some(ops) => ops,
            None => crate::kern::subr_prf::panic(format_args!("file {:p}: no f_ops", self)),
        }
    }
}

impl Default for File {
    fn default() -> Self {
        Self::new()
    }
}

queue_adapter!(
    /// `LIST_HEAD(filelist, file)`: the list of open files, through `f_list`.
    pub FileList: File, f_list => ListEntry<File>
);

/// `FREF(fp)`: takes a reference to `fp`.
pub fn fref(fp: &File) {
    crate::kern::vfs_subr::vfs_stall_barrier();
    fp.f_count.fetch_add(1, Ordering::SeqCst);
}

/// `FRELE(fp, p)`: drops a reference to `fp`; the last one closes it (`fdrop`).
pub fn frele<'a>(fp: &'static File, p: impl Into<Option<&'a Proc>>) -> Result<(), Errno> {
    if fp.f_count.fetch_sub(1, Ordering::SeqCst) == 1 {
        fdrop(fp, p.into())
    } else {
        Ok(())
    }
}

/// `foffset(fp)`: the file's offset, read under `f_mtx`.
pub fn foffset(fp: &File) -> Off {
    mtx_enter(&fp.f_mtx);
    let offset = fp.f_offset.get();
    mtx_leave(&fp.f_mtx);
    offset
}
