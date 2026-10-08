/*	$OpenBSD: timeout.h,v 1.51 2025/05/23 23:56:14 dlg Exp $	*/
/* <LICENSES> */
/*
 * Copyright (c) 2000-2001 Artur Grabowski <art@openbsd.org>
 * All rights reserved.
 *
 * Redistribution and use in source and binary forms, with or without
 * modification, are permitted provided that the following conditions
 * are met:
 *
 * 1. Redistributions of source code must retain the above copyright
 *    notice, this list of conditions and the following disclaimer.
 * 2. The name of the author may not be used to endorse or promote products
 *    derived from this software without specific prior written permission.
 *
 * THIS SOFTWARE IS PROVIDED ``AS IS'' AND ANY EXPRESS OR IMPLIED WARRANTIES,
 * INCLUDING, BUT NOT LIMITED TO, THE IMPLIED WARRANTIES OF MERCHANTABILITY
 * AND FITNESS FOR A PARTICULAR PURPOSE ARE DISCLAIMED. IN NO EVENT SHALL
 * THE AUTHOR BE LIABLE FOR ANY DIRECT, INDIRECT, INCIDENTAL, SPECIAL,
 * EXEMPLARY, OR CONSEQUENTIAL  DAMAGES (INCLUDING, BUT NOT LIMITED TO,
 * PROCUREMENT OF SUBSTITUTE GOODS OR SERVICES; LOSS OF USE, DATA, OR PROFITS;
 * OR BUSINESS INTERRUPTION) HOWEVER CAUSED AND ON ANY THEORY OF LIABILITY,
 * WHETHER IN CONTRACT, STRICT LIABILITY, OR TORT (INCLUDING NEGLIGENCE OR
 * OTHERWISE) ARISING IN ANY WAY OUT OF THE USE OF THIS SOFTWARE, EVEN IF
 * ADVISED OF THE POSSIBILITY OF SUCH DAMAGE.
 */
/* </LICENSES> */

/* <CODE> */
//! `<sys/timeout.h>`: `timeout(9)`, the timer wheel's entries. The functions live in
//! `kern/kern_timeout.rs`.
//!
//! Upstream: sys/sys/timeout.h @ 3ce1f3f79392
//!
//! Status: `ported` (M5).
//!
//! ## Deviations
//! - `struct circq` is the C's own circular list (not `queue.h`); its operations are the
//!   `circq_*` functions of `kern_timeout.rs`. `struct timeout` is `#[repr(C)]` with `to_list`
//!   first, as the C's `timeout_from_circq` cast requires.
//! - `to_process` (the `kcov` identifier, `struct process *`) is kept as an opaque pointer
//!   until `struct process` exists (M5-b).

use core::cell::Cell;
use core::ffi::c_void;
use core::ptr;

use crate::sys::time::Timespec;

/// `struct circq`: a doubly-linked circular list node; a list is a node of its own.
#[repr(C)]
pub struct Circq {
    /// `next`: next element.
    pub next: Cell<*const Circq>,
    /// `prev`: previous element.
    pub prev: Cell<*const Circq>,
}

// SAFETY: every circq is linked and unlinked under `timeout_mutex`.
unsafe impl Sync for Circq {}

impl Circq {
    /// A node in no list (`{ NULL, NULL }`); `circq_init` makes it an empty list.
    pub const fn new() -> Self {
        Self {
            next: Cell::new(ptr::null()),
            prev: Cell::new(ptr::null()),
        }
    }
}

impl Default for Circq {
    fn default() -> Self {
        Self::new()
    }
}

/// `void (*to_func)(void *)`: the function a timeout calls.
pub type TimeoutFn = fn(*mut c_void);

/// `struct timeout`.
#[repr(C)]
pub struct Timeout {
    /// `to_list`: timeout queue, don't move.
    pub to_list: Circq,
    /// `to_abstime`: absolute time to run at.
    pub to_abstime: Cell<Timespec>,
    /// `to_func`: function to call.
    pub to_func: Cell<Option<TimeoutFn>>,
    /// `to_arg`: function argument.
    pub to_arg: Cell<*mut c_void>,
    /// `to_process`: kcov identifier (`struct process`, M5-b).
    pub to_process: Cell<*const ()>,
    /// `to_time`: ticks on event.
    pub to_time: Cell<i32>,
    /// `to_flags`: misc flags.
    pub to_flags: Cell<i32>,
    /// `to_kclock`: abstime's kernel clock.
    pub to_kclock: Cell<i32>,
}

// SAFETY: a timeout is set up by its owner before use and otherwise touched under
// `timeout_mutex`.
unsafe impl Sync for Timeout {}

impl Timeout {
    /// `TIMEOUT_INITIALIZER_FLAGS(_fn, _arg, _kclock, _flags)`.
    pub const fn new_flags(func: TimeoutFn, arg: *mut c_void, kclock: i32, flags: i32) -> Self {
        Self {
            to_list: Circq::new(),
            to_abstime: Cell::new(Timespec::new(0, 0)),
            to_func: Cell::new(Some(func)),
            to_arg: Cell::new(arg),
            to_process: Cell::new(ptr::null()),
            to_time: Cell::new(0),
            to_flags: Cell::new(flags | TIMEOUT_INITIALIZED),
            to_kclock: Cell::new(kclock),
        }
    }

    /// `TIMEOUT_INITIALIZER(_f, _a)`.
    pub const fn new(func: TimeoutFn, arg: *mut c_void) -> Self {
        Self::new_flags(func, arg, KCLOCK_NONE, 0)
    }

    /// A timeout that `timeout_set` has not seen yet (all zero, like a `static struct
    /// timeout`).
    pub const fn zeroed() -> Self {
        Self {
            to_list: Circq::new(),
            to_abstime: Cell::new(Timespec::new(0, 0)),
            to_func: Cell::new(None),
            to_arg: Cell::new(ptr::null_mut()),
            to_process: Cell::new(ptr::null()),
            to_time: Cell::new(0),
            to_flags: Cell::new(0),
            to_kclock: Cell::new(0),
        }
    }
}

/*
 * flags in the to_flags field.
 */

/// `TIMEOUT_PROC`: needs a process context.
pub const TIMEOUT_PROC: i32 = 0x01;
/// `TIMEOUT_ONQUEUE`: on any timeout queue.
pub const TIMEOUT_ONQUEUE: i32 = 0x02;
/// `TIMEOUT_INITIALIZED`: initialized.
pub const TIMEOUT_INITIALIZED: i32 = 0x04;
/// `TIMEOUT_TRIGGERED`: running or ran.
pub const TIMEOUT_TRIGGERED: i32 = 0x08;
/// `TIMEOUT_MPSAFE`: run without kernel lock.
pub const TIMEOUT_MPSAFE: i32 = 0x10;

/// `struct timeoutstat`: statistics and totals, `[T]` under `timeout_mutex`.
pub struct Timeoutstat {
    /// `tos_added`: `timeout_add*(9)` calls.
    pub tos_added: Cell<u64>,
    /// `tos_cancelled`: dequeued during `timeout_del*(9)`.
    pub tos_cancelled: Cell<u64>,
    /// `tos_deleted`: `timeout_del*(9)` calls.
    pub tos_deleted: Cell<u64>,
    /// `tos_late`: run after deadline.
    pub tos_late: Cell<u64>,
    /// `tos_pending`: number currently ONQUEUE.
    pub tos_pending: Cell<u64>,
    /// `tos_readded`: `timeout_add*(9)` + already ONQUEUE.
    pub tos_readded: Cell<u64>,
    /// `tos_rescheduled`: bucketed + already SCHEDULED.
    pub tos_rescheduled: Cell<u64>,
    /// `tos_run_softclock`: run from `softclock()`.
    pub tos_run_softclock: Cell<u64>,
    /// `tos_run_thread`: run from `softclock_thread()`.
    pub tos_run_thread: Cell<u64>,
    /// `tos_scheduled`: bucketed during `softclock()`.
    pub tos_scheduled: Cell<u64>,
    /// `tos_softclocks`: `softclock()` calls.
    pub tos_softclocks: Cell<u64>,
    /// `tos_thread_wakeups`: wakeups in `softclock_thread()`.
    pub tos_thread_wakeups: Cell<u64>,
}

// SAFETY: updated under `timeout_mutex`.
unsafe impl Sync for Timeoutstat {}

impl Timeoutstat {
    /// All zero.
    pub const fn new() -> Self {
        Self {
            tos_added: Cell::new(0),
            tos_cancelled: Cell::new(0),
            tos_deleted: Cell::new(0),
            tos_late: Cell::new(0),
            tos_pending: Cell::new(0),
            tos_readded: Cell::new(0),
            tos_rescheduled: Cell::new(0),
            tos_run_softclock: Cell::new(0),
            tos_run_thread: Cell::new(0),
            tos_scheduled: Cell::new(0),
            tos_softclocks: Cell::new(0),
            tos_thread_wakeups: Cell::new(0),
        }
    }
}

impl Default for Timeoutstat {
    fn default() -> Self {
        Self::new()
    }
}

/*
 * special macros
 */

/// `timeout_pending(to)`: is this timeout already scheduled to run?
pub fn timeout_pending(to: &Timeout) -> bool {
    to.to_flags.get() & TIMEOUT_ONQUEUE != 0
}

/// `timeout_initialized(to)`: is this timeout initialized?
pub fn timeout_initialized(to: &Timeout) -> bool {
    to.to_flags.get() & TIMEOUT_INITIALIZED != 0
}

/// `timeout_triggered(to)`: is this timeout running, or did it run?
pub fn timeout_triggered(to: &Timeout) -> bool {
    to.to_flags.get() & TIMEOUT_TRIGGERED != 0
}

/// `KCLOCK_NONE`: dummy clock for sanity checks.
pub const KCLOCK_NONE: i32 = -1;
/// `KCLOCK_UPTIME`: uptime clock; time since boot.
pub const KCLOCK_UPTIME: i32 = 0;
/// `KCLOCK_MAX`.
pub const KCLOCK_MAX: i32 = 1;

// The timeout_from_circq cast relies on to_list being first.
const _: () = {
    assert!(core::mem::offset_of!(Timeout, to_list) == 0);
};
/* </CODE> */
