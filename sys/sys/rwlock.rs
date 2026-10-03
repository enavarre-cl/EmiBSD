/*	$OpenBSD: rwlock.h,v 1.34 2025/07/21 20:36:41 bluhm Exp $	*/
/* <LICENSES> */
/*
 * Copyright (c) 2002 Artur Grabowski <art@openbsd.org>
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

//! `<sys/rwlock.h>`: multiple readers, single writer lock.
//!
//! Upstream: sys/sys/rwlock.h @ 3ce1f3f79392
//!
//! Simplistic implementation modelled after rw locks in Solaris.
//!
//! The `rwl_owner` has the following layout: `[ owner or count of readers | wrlock | wrwant
//! | wait ]`. When the WAIT bit is set (bit 0), the lock has waiters sleeping on it. When the
//! WRWANT bit is set (bit 1), at least one waiter wants a write lock. When the WRLOCK bit is
//! set (bit 2) the lock is currently write-locked. When write locked, the upper bits contain
//! the `struct proc *` pointer to the writer, otherwise they count the number of readers.
//!
//! Status: `ported` (M7a). The functions are in `kern/kern_rwlock.rs`.
//!
//! ## Deviations
//! - `WITNESS` is not configured: `struct lock_object` (`rwl_lock_obj`), `RWLOCK_LO_FLAGS`,
//!   `RRWLOCK_LO_FLAGS`, `RWLOCK_LO_INITIALIZER` and the `lock_type` arguments of the init
//!   functions do not exist; `RWL_NOWITNESS` is accepted and ignored.
//! - `RWLOCK_INITIALIZER(name)` is `Rwlock::new(name)`, `RWLOCK_INITIALIZER_TRACE` is
//!   `Rwlock::new_trace`; the fields are atomics (`volatile` in C) and `Cell`s.

use core::cell::Cell;
use core::sync::atomic::{AtomicU32, AtomicUsize, Ordering};

use crate::kern::kern_rwlock::rw_status;
use crate::sys::proc::Proc;

/// `RWL_DUPOK`: duplicate locks are fine (`WITNESS`).
pub const RWL_DUPOK: i32 = 0x01;
/// `RWL_NOWITNESS`: not checked by `WITNESS`.
pub const RWL_NOWITNESS: i32 = 0x02;
/// `RWL_IS_VNODE`: a vnode lock (`WITNESS`).
pub const RWL_IS_VNODE: i32 = 0x04;

/// `RWLOCK_WRLOCK`: the lock is write locked.
pub const RWLOCK_WRLOCK: usize = 0x04;
/// `RWLOCK_MASK`: the flag bits of `rwl_owner`.
pub const RWLOCK_MASK: usize = 0x07;

/// `RWLOCK_READER_SHIFT`: the reader count starts above the flag bits.
pub const RWLOCK_READER_SHIFT: usize = 3;
/// `RWLOCK_READ_INCR`: one reader.
pub const RWLOCK_READ_INCR: usize = 1 << RWLOCK_READER_SHIFT;

/// `RW_WRITE`: exclusive lock.
pub const RW_WRITE: i32 = 0x0001;
/// `RW_READ`: shared lock.
pub const RW_READ: i32 = 0x0002;
/// `RW_DOWNGRADE`: downgrade exclusive to shared.
pub const RW_DOWNGRADE: i32 = 0x0004;
/// `RW_UPGRADE`: upgrade shared to exclusive.
pub const RW_UPGRADE: i32 = 0x0005;
/// `RW_OPMASK`: the operation bits of the flags.
pub const RW_OPMASK: i32 = 0x0007;

/// `RW_INTR`: interruptible sleep.
pub const RW_INTR: i32 = 0x0010;
/// `RW_NOSLEEP`: don't wait for the lock.
pub const RW_NOSLEEP: i32 = 0x0040;
/// `RW_RECURSEFAIL`: fail on recursion for RRW locks.
pub const RW_RECURSEFAIL: i32 = 0x0080;
/// `RW_DUPOK`: permit duplicate lock.
pub const RW_DUPOK: i32 = 0x0100;

/// `RW_WRITE_OTHER`: for `rw_status()` and `rrw_status()` only: exclusive lock held by some
/// other thread.
pub const RW_WRITE_OTHER: i32 = 0x0100;

/// `DT_RWLOCK_IDX_NETLOCK`: the tracepoint index of `netlock` (sorted alphabetically, keep in
/// sync with `dev/dt/dt_prov_static.c`).
pub const DT_RWLOCK_IDX_NETLOCK: i32 = 1;
/// `DT_RWLOCK_IDX_SOLOCK`.
pub const DT_RWLOCK_IDX_SOLOCK: i32 = 2;

/// `struct rwlock`.
pub struct Rwlock {
    /// `rwl_owner`: the writer (a `struct proc *`) with `RWLOCK_WRLOCK`, or the reader count
    /// shifted by `RWLOCK_READER_SHIFT`, with the wait bits below.
    pub rwl_owner: AtomicUsize,
    /// `rwl_waiters`: threads waiting for the write lock.
    pub rwl_waiters: AtomicU32,
    /// `rwl_readers`: threads waiting for a read lock.
    pub rwl_readers: AtomicU32,
    /// `rwl_name`: the wait message.
    pub rwl_name: Cell<&'static str>,
    // rwl_lock_obj: WITNESS, not configured.
    /// `rwl_traceidx`: the `dt(4)` tracepoint index (`DT_RWLOCK_IDX_*`).
    pub rwl_traceidx: Cell<i32>,
}

// SAFETY: the owner word is atomic and the name/trace cells are written by the initialiser
// before the lock is shared.
unsafe impl Sync for Rwlock {}

impl Rwlock {
    /// `RWLOCK_INITIALIZER(name)`: a free lock.
    pub const fn new(name: &'static str) -> Self {
        Self::new_trace(name, 0)
    }

    /// `RWLOCK_INITIALIZER_TRACE(name, trace)`: a free lock with a tracepoint index.
    pub const fn new_trace(name: &'static str, trace: i32) -> Self {
        Self {
            rwl_owner: AtomicUsize::new(0),
            rwl_waiters: AtomicU32::new(0),
            rwl_readers: AtomicU32::new(0),
            rwl_name: Cell::new(name),
            rwl_traceidx: Cell::new(trace),
        }
    }

    /// `RWLOCK_OWNER(rwl)`: the writer, when write locked (null otherwise, or garbage from
    /// the reader count, as in C).
    pub fn owner(&self) -> *const Proc {
        (self.rwl_owner.load(Ordering::Relaxed) & !RWLOCK_MASK) as *const Proc
    }
}

/// `struct rrwlock`: recursive rwlocks.
pub struct Rrwlock {
    /// `rrwl_lock`.
    pub rrwl_lock: Rwlock,
    /// `rrwl_wcnt`: # writers.
    pub rrwl_wcnt: Cell<u32>,
}

// SAFETY: as for `Rwlock`; the count is the writer's.
unsafe impl Sync for Rrwlock {}

impl Rrwlock {
    /// A free recursive lock.
    pub const fn new(name: &'static str) -> Self {
        Self {
            rrwl_lock: Rwlock::new(name),
            rrwl_wcnt: Cell::new(0),
        }
    }
}

/// `rw_read_held`.
pub fn rw_read_held(rwl: &Rwlock) -> bool {
    rw_status(rwl) == RW_READ
}

/// `rw_write_held`.
pub fn rw_write_held(rwl: &Rwlock) -> bool {
    rw_status(rwl) == RW_WRITE
}

/// `rw_lock_held`.
pub fn rw_lock_held(rwl: &Rwlock) -> bool {
    let status = rw_status(rwl);

    status == RW_READ || status == RW_WRITE
}
