/* <LICENSES> */
/*
 * Copyright (c) 1990, 1993
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
 *	@(#)filedesc.h	8.1 (Berkeley) 6/2/93
 */
/* </LICENSES> */

/* <CODE> */
//! `<sys/filedesc.h>`: the per-process descriptor table, `struct filedesc`, and its initial
//! allocation `struct filedesc0`.
//!
//! This structure is used for the management of descriptors. It may be shared by multiple
//! processes.
//!
//! A process is initially started out with `NDFILE` descriptors stored within this
//! structure, selected to be enough for typical applications based on the historical limit
//! of 20 open files (and the usage of descriptors by shells). If these descriptors are
//! exhausted, a larger descriptor table may be allocated, up to a process' resource limit;
//! the internal arrays are then unused. The initial expansion is set to `NDEXTENT`; each time
//! it runs out, it is doubled until the resource limit is reached. `NDEXTENT` should be
//! selected to be the biggest multiple of `OFILESIZE` that will fit in a power-of-two sized
//! piece of memory.
//!
//! Upstream: sys/sys/filedesc.h @ 3ce1f3f79392
//!
//! ## Deviations
//! - The arrays `fd_ofiles`, `fd_ofileflags`, `fd_himap` and `fd_lomap` stay raw pointers,
//!   as in C, because `fdexpand` swaps them for bigger ones; they are read and written only
//!   through the bounds-checked accessors (`ofile`, `set_ofile`, `ofileflags`, `lomap`, ...),
//!   so no reference into an array outlives an expansion. The lengths are the C's:
//!   `fd_nfiles` entries, `NDHISLOTS(fd_nfiles)` and `NDLOSLOTS(fd_nfiles)` words.
//! - `fd_cdir`/`fd_rdir` are `Option<&'static Vnode>` (vnodes are never freed; the table
//!   holds a use count on each); they stay `None` until a root file system is mounted.
//! - The macros `NDREDUCE`, `NDHISLOTS`, `NDLOSLOTS` are `const fn`s; `fdplock`,
//!   `fdpunlock` and `fdpassertlocked` are functions (`NET_ASSERT_UNLOCKED` waits for the
//!   network stack's lock).
//! - The prototypes are their functions in `kern/kern_descrip.rs`. `Process::fd` and
//!   `Proc::fd` are this port's accessors for `ps_fd`/`p_fd`, as `vmspace()` and `ucred()`
//!   are for theirs.

use core::cell::{Cell, UnsafeCell};
use core::ptr;
use core::sync::atomic::{AtomicI32, AtomicU32, Ordering};

use crate::kassert;
use crate::kern::kern_rwlock::{rw_assert_wrlock, rw_enter_write, rw_exit_write};
use crate::kern::subr_prf::panic;
use crate::machine::intr::IPL_MPFLOOR;
use crate::sys::eventvar::KqList;
use crate::sys::file::File;
use crate::sys::mutex::Mutex;
use crate::sys::proc::{Proc, Process};
use crate::sys::queue::ListHead;
use crate::sys::rwlock::Rwlock;
use crate::sys::types::Mode;
use crate::sys::vnode::Vnode;

/// `NDFILE`: descriptors in the initial table.
pub const NDFILE: usize = 20;
/// `NDEXTENT`: the first expansion (250 bytes in 256-byte alloc).
pub const NDEXTENT: usize = 50;
/// `NDENTRIES`: 32 fds per entry.
pub const NDENTRIES: usize = 32;
/// `NDENTRYMASK`.
pub const NDENTRYMASK: usize = NDENTRIES - 1;
/// `NDENTRYSHIFT`: bits per entry.
pub const NDENTRYSHIFT: usize = 5;

/// `NDREDUCE(x)`: the number of 32-bit words `x` bits need.
pub const fn ndreduce(x: usize) -> usize {
    (x + NDENTRIES - 1) >> NDENTRYSHIFT
}

/// `NDHISLOTS(x)`: the words of `fd_himap` for `x` descriptors.
pub const fn ndhislots(x: usize) -> usize {
    ndreduce(ndreduce(x))
}

/// `NDLOSLOTS(x)`: the words of `fd_lomap` for `x` descriptors.
pub const fn ndloslots(x: usize) -> usize {
    ndhislots(x) << NDENTRYSHIFT
}

/// `UF_EXCLOSE`: auto-close on exec.
pub const UF_EXCLOSE: u8 = 0x01;
/// `UF_PLEDGED`: opened after `pledge(2)`.
pub const UF_PLEDGED: u8 = 0x02;
/// `UF_FORKCLOSE`: auto-close on fork.
pub const UF_FORKCLOSE: u8 = 0x04;
/// `UF_PLEDGEOPEN`: opened with `__pledge_open()`.
pub const UF_PLEDGEOPEN: u8 = 0x08;

/// `FD_ADVLOCK`: may hold a POSIX adv. lock (`fd_flags`).
pub const FD_ADVLOCK: i32 = 0x01;

/// `OFILESIZE`: storage required per open file descriptor.
pub const OFILESIZE: usize = size_of::<*const File>() + size_of::<u8>();

/// `struct filedesc`.
///
/// Locking: \[a\] atomic operations, \[f\] `fd_lock`, \[f/w\] `fd_lock` when writing, \[K\]
/// kernel lock, \[m\] `fd_fplock`.
pub struct Filedesc {
    /// \[f/w,m\] `fd_ofiles`: file structures for open files (`fd_nfiles` entries).
    pub fd_ofiles: Cell<*mut *const File>,
    /// \[f\] `fd_ofileflags`: per-process open file flags (`fd_nfiles` entries).
    pub fd_ofileflags: Cell<*mut u8>,
    /// \[K\] `fd_cdir`: current directory.
    pub fd_cdir: Cell<Option<&'static Vnode>>,
    /// \[K\] `fd_rdir`: root directory.
    pub fd_rdir: Cell<Option<&'static Vnode>>,
    /// \[f\] `fd_nfiles`: number of open files allocated; written under `fd_lock`, read
    /// unlocked (`fd_getfile`), so a relaxed atomic.
    pub fd_nfiles: AtomicI32,
    /// \[f\] `fd_openfd`: number of files currently open; read unlocked by
    /// `getdtablecount(2)`, so a relaxed atomic.
    pub fd_openfd: AtomicI32,
    /// \[f\] `fd_himap`: each bit points to 32 fds.
    pub fd_himap: Cell<*mut u32>,
    /// \[f\] `fd_lomap`: bitmap of free fds.
    pub fd_lomap: Cell<*mut u32>,
    /// \[f\] `fd_lastfile`: high-water mark of `fd_ofiles`.
    pub fd_lastfile: Cell<i32>,
    /// \[f\] `fd_freefile`: approx. next free file.
    pub fd_freefile: Cell<i32>,
    /// \[f/w\] `fd_cmask`: mask for file creation.
    pub fd_cmask: Cell<Mode>,
    /// \[K\] `fd_refcnt`: reference count.
    pub fd_refcnt: Cell<u32>,
    /// `fd_lock`: lock for the file descs.
    pub fd_lock: Rwlock,
    /// `fd_fplock`: lock for reading `fd_ofiles` without `fd_lock`.
    pub fd_fplock: Mutex,
    /// \[f\] `fd_kqlist`: kqueues attached to this filedesc.
    pub fd_kqlist: ListHead<KqList>,
    /// \[a\] `fd_flags`: flags on this filedesc (`FD_ADVLOCK`).
    pub fd_flags: AtomicI32,
    /// \[a\] `fd_nuserevents`: number of kqueue user events.
    pub fd_nuserevents: AtomicU32,
}

impl Filedesc {
    /// A zeroed table, as `pool_get(&fdesc_pool, PR_ZERO)` returns it.
    pub const fn new() -> Self {
        Self {
            fd_ofiles: Cell::new(ptr::null_mut()),
            fd_ofileflags: Cell::new(ptr::null_mut()),
            fd_cdir: Cell::new(None),
            fd_rdir: Cell::new(None),
            fd_nfiles: AtomicI32::new(0),
            fd_openfd: AtomicI32::new(0),
            fd_himap: Cell::new(ptr::null_mut()),
            fd_lomap: Cell::new(ptr::null_mut()),
            fd_lastfile: Cell::new(0),
            fd_freefile: Cell::new(0),
            fd_cmask: Cell::new(0),
            fd_refcnt: Cell::new(0),
            fd_lock: Rwlock::new("fdlock"),
            fd_fplock: Mutex::new(IPL_MPFLOOR),
            fd_kqlist: ListHead::new(),
            fd_flags: AtomicI32::new(0),
            fd_nuserevents: AtomicU32::new(0),
        }
    }

    /// `fd_nfiles` as an index bound.
    pub fn nfiles(&self) -> usize {
        self.fd_nfiles.load(Ordering::Relaxed) as usize
    }

    /// Checks `i` against an array of `len` entries.
    fn check(i: usize, len: usize, what: &str) {
        if i >= len {
            panic(format_args!("filedesc: {what}[{i}] out of {len}"));
        }
    }

    /// `fdp->fd_ofiles[fd]`: the file at descriptor `fd`, if any.
    pub fn ofile(&self, fd: usize) -> Option<&'static File> {
        Self::check(fd, self.nfiles(), "fd_ofiles");
        // SAFETY: `fd_ofiles` points at `fd_nfiles` entries (the internal array or the one
        // `fdexpand`/`fdcopy` allocated), and `fd` is below that. A non-null entry is a file
        // the table holds a reference to, a pool item that stays allocated while it does.
        unsafe { (*self.fd_ofiles.get().add(fd)).as_ref() }
    }

    /// `fdp->fd_ofiles[fd] = fp`.
    pub fn set_ofile(&self, fd: usize, fp: Option<&'static File>) {
        Self::check(fd, self.nfiles(), "fd_ofiles");
        let fp = fp.map_or(ptr::null(), ptr::from_ref);
        // SAFETY: as in `ofile`; the array is written only under `fd_lock` (and `fd_fplock`
        // where the C takes it), so no reader sees a torn entry.
        unsafe { *self.fd_ofiles.get().add(fd) = fp };
    }

    /// `fdp->fd_ofileflags[fd]`.
    pub fn ofileflags(&self, fd: usize) -> u8 {
        Self::check(fd, self.nfiles(), "fd_ofileflags");
        // SAFETY: `fd_ofileflags` points at `fd_nfiles` bytes and `fd` is below that.
        unsafe { *self.fd_ofileflags.get().add(fd) }
    }

    /// `fdp->fd_ofileflags[fd] = flags`.
    pub fn set_ofileflags(&self, fd: usize, flags: u8) {
        Self::check(fd, self.nfiles(), "fd_ofileflags");
        // SAFETY: as in `ofileflags`; written under `fd_lock`.
        unsafe { *self.fd_ofileflags.get().add(fd) = flags };
    }

    /// `fdp->fd_lomap[off]`.
    pub fn lomap(&self, off: usize) -> u32 {
        Self::check(off, ndloslots(self.nfiles()), "fd_lomap");
        // SAFETY: `fd_lomap` points at `NDLOSLOTS(fd_nfiles)` words and `off` is below that.
        unsafe { *self.fd_lomap.get().add(off) }
    }

    /// `fdp->fd_lomap[off] = v`.
    pub fn set_lomap(&self, off: usize, v: u32) {
        Self::check(off, ndloslots(self.nfiles()), "fd_lomap");
        // SAFETY: as in `lomap`; written under `fd_lock`.
        unsafe { *self.fd_lomap.get().add(off) = v };
    }

    /// `fdp->fd_himap[off]`.
    pub fn himap(&self, off: usize) -> u32 {
        Self::check(off, ndhislots(self.nfiles()), "fd_himap");
        // SAFETY: `fd_himap` points at `NDHISLOTS(fd_nfiles)` words and `off` is below that.
        unsafe { *self.fd_himap.get().add(off) }
    }

    /// `fdp->fd_himap[off] = v`.
    pub fn set_himap(&self, off: usize, v: u32) {
        Self::check(off, ndhislots(self.nfiles()), "fd_himap");
        // SAFETY: as in `himap`; written under `fd_lock`.
        unsafe { *self.fd_himap.get().add(off) = v };
    }
}

impl Default for Filedesc {
    fn default() -> Self {
        Self::new()
    }
}

/// `struct filedesc0`: basic allocation of descriptors: one of the above, plus arrays for
/// `NDFILE` descriptors.
///
/// `#[repr(C)]` keeps `fd_fd` at offset 0: `fdfree` gives the `struct filedesc` back to
/// `fdesc_pool` as the `struct filedesc0` it was allocated as.
#[repr(C)]
pub struct Filedesc0 {
    /// `fd_fd`.
    pub fd_fd: Filedesc,
    /// `fd_dfiles`: used when the number of open files is `<= NDFILE`, and then pointed to
    /// by `fd_ofiles`.
    pub fd_dfiles: UnsafeCell<[*const File; NDFILE]>,
    /// `fd_dfileflags`: likewise for `fd_ofileflags`.
    pub fd_dfileflags: UnsafeCell<[u8; NDFILE]>,
    /// `fd_dhimap`: used when the number of open files is `<= 1024`, and then pointed to by
    /// `fd_himap`.
    pub fd_dhimap: UnsafeCell<[u32; NDENTRIES >> NDENTRYSHIFT]>,
    /// `fd_dlomap`: likewise for `fd_lomap`.
    pub fd_dlomap: UnsafeCell<[u32; NDENTRIES]>,
}

impl Filedesc0 {
    /// A zeroed allocation.
    pub const fn new() -> Self {
        Self {
            fd_fd: Filedesc::new(),
            fd_dfiles: UnsafeCell::new([ptr::null(); NDFILE]),
            fd_dfileflags: UnsafeCell::new([0; NDFILE]),
            fd_dhimap: UnsafeCell::new([0; NDENTRIES >> NDENTRYSHIFT]),
            fd_dlomap: UnsafeCell::new([0; NDENTRIES]),
        }
    }
}

impl Default for Filedesc0 {
    fn default() -> Self {
        Self::new()
    }
}

impl Process {
    /// `pr->ps_fd`: the process's descriptor table, which it holds a reference to from
    /// `process_new` (`fdinit` for process 0) until `fdfree`.
    pub fn fd(&self) -> &'static Filedesc {
        let fdp = self.ps_fd.get();
        kassert!(!fdp.is_null());
        // SAFETY: non-null while the process holds its reference; the table is a
        // `fdesc_pool` item that `fdfree` gives back only when the last reference goes.
        unsafe { &*fdp }
    }
}

impl Proc {
    /// `p->p_fd`: the thread's copy of its process's descriptor table.
    pub fn fd(&self) -> &'static Filedesc {
        let fdp = self.p_fd.get();
        kassert!(!fdp.is_null());
        // SAFETY: as for `Process::fd`; `exit1` nulls the thread's copy after `fdfree`.
        unsafe { &*fdp }
    }
}

/// `fdplock(fdp)`: takes the table's lock for writing.
pub fn fdplock(fdp: &Filedesc) {
    // NET_ASSERT_UNLOCKED(): no network lock yet (M7b).
    rw_enter_write(&fdp.fd_lock);
}

/// `fdpunlock(fdp)`.
pub fn fdpunlock(fdp: &Filedesc) {
    rw_exit_write(&fdp.fd_lock);
}

/// `fdpassertlocked(fdp)`.
pub fn fdpassertlocked(fdp: &Filedesc) {
    rw_assert_wrlock(&fdp.fd_lock);
}

const _: () = {
    assert!(ndhislots(NDFILE) == NDENTRIES >> NDENTRYSHIFT);
    assert!(ndloslots(NDFILE) == NDENTRIES);
    assert!(core::mem::offset_of!(Filedesc0, fd_fd) == 0);
};
/* </CODE> */
