/* <LICENSES> */
/*
 * Copyright (c) 2020-2024 Scott Cheloha <cheloha@openbsd.org>
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

/* <CODE> */
//! `<sys/clockintr.h>`: schedulable clock interrupts and the per-CPU queue that dispatches
//! them. The functions live in `kern/kern_clockintr.rs`.
//!
//! Upstream: sys/sys/clockintr.h @ 3ce1f3f79392
//!
//! Status: `ported` (M5).
//!
//! ## Deviations
//! - `cq_intrclock` is `Cell<Option<Intrclock>>`: the C copies the platform's `struct
//!   intrclock` by value once (`CQ_INTRCLOCK` says whether it did); `None` is "not installed".
//! - `cq_stat` is a `Cell` of a `Copy` struct updated as a whole; `cq_gen`/`cq_dispatch` are
//!   atomics (the C's `volatile`).

use core::cell::Cell;
use core::ffi::c_void;
use core::ptr;
use core::sync::atomic::AtomicU32;

use crate::machine::intr::IPL_CLOCK;
use crate::queue_adapter;
use crate::sys::mutex::Mutex;
use crate::sys::queue::{TailqEntry, TailqHead};

/// `struct clockintr_stat`.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct ClockintrStat {
    /// `cs_dispatched`: total time in dispatch (ns).
    pub cs_dispatched: u64,
    /// `cs_early`: number of early dispatch calls.
    pub cs_early: u64,
    /// `cs_earliness`: total earliness (ns).
    pub cs_earliness: u64,
    /// `cs_lateness`: total lateness (ns).
    pub cs_lateness: u64,
    /// `cs_prompt`: number of prompt dispatch calls.
    pub cs_prompt: u64,
    /// `cs_run`: number of events dispatched.
    pub cs_run: u64,
    /// `cs_spurious`: number of spurious dispatch calls.
    pub cs_spurious: u64,
}

/*
 * Platform API
 */

/// `struct intrclock`: the local interrupt clock a platform installs.
#[derive(Clone, Copy, Debug)]
pub struct Intrclock {
    /// `ic_cookie`.
    pub ic_cookie: *mut c_void,
    /// `ic_rearm`: program the next interrupt `nsecs` from now.
    pub ic_rearm: fn(*mut c_void, u64),
    /// `ic_trigger`: raise an interrupt as soon as possible.
    pub ic_trigger: fn(*mut c_void),
}

/// The callback of a `clockintr`: `void (*cl_func)(struct clockrequest *, void *, void *)`,
/// with the request, the clock frame and `cl_arg`.
pub type ClockintrFn = fn(&Clockrequest, *mut c_void, *mut c_void);

/// `struct clockintr`: schedulable clock interrupt callback.
///
/// Struct member protections:
/// - I: immutable after initialization.
/// - m: parent queue mutex (`cl_queue->cq_mtx`).
pub struct Clockintr {
    /// \[m\] `cl_expiration`: dispatch time.
    pub cl_expiration: Cell<u64>,
    /// \[m\] `cl_alink`: `cq_all` glue.
    pub cl_alink: TailqEntry<Clockintr>,
    /// \[m\] `cl_plink`: `cq_pend` glue.
    pub cl_plink: TailqEntry<Clockintr>,
    /// \[I\] `cl_arg`: argument.
    pub cl_arg: Cell<*mut c_void>,
    /// \[I\] `cl_func`: callback.
    pub cl_func: Cell<Option<ClockintrFn>>,
    /// \[I\] `cl_queue`: parent queue.
    pub cl_queue: Cell<*const Clockqueue>,
    /// \[m\] `cl_flags`: `CLST_*` flags.
    pub cl_flags: Cell<u32>,
}

// SAFETY: touched under the parent queue's mutex (or before binding, by one CPU).
unsafe impl Sync for Clockintr {}

impl Clockintr {
    /// An unbound, unscheduled clock interrupt (all zero, as the C's).
    pub const fn new() -> Self {
        Self {
            cl_expiration: Cell::new(0),
            cl_alink: TailqEntry::new(),
            cl_plink: TailqEntry::new(),
            cl_arg: Cell::new(ptr::null_mut()),
            cl_func: Cell::new(None),
            cl_queue: Cell::new(ptr::null()),
            cl_flags: Cell::new(0),
        }
    }
}

impl Default for Clockintr {
    fn default() -> Self {
        Self::new()
    }
}

queue_adapter!(
    /// `TAILQ_HEAD(, clockintr) cq_all`: every established clockintr, through `cl_alink`.
    pub ClockintrAll: Clockintr, cl_alink => TailqEntry<Clockintr>
);

queue_adapter!(
    /// `TAILQ_HEAD(, clockintr) cq_pend`: the pending clockintrs, through `cl_plink`.
    pub ClockintrPend: Clockintr, cl_plink => TailqEntry<Clockintr>
);

/// `CLST_PENDING`: scheduled to run.
pub const CLST_PENDING: u32 = 0x0000_0001;

/// `struct clockrequest`: interface for callback rescheduling requests.
///
/// Struct member protections:
/// - I: immutable after initialization.
/// - o: owned by a single CPU.
pub struct Clockrequest {
    /// \[o\] `cr_expiration`: copy of dispatch time.
    pub cr_expiration: Cell<u64>,
    /// \[I\] `cr_queue`: enclosing queue.
    pub cr_queue: Cell<*const Clockqueue>,
    /// \[o\] `cr_flags`: `CR_*` flags.
    pub cr_flags: Cell<u32>,
}

impl Clockrequest {
    /// A request of no queue.
    pub const fn new() -> Self {
        Self {
            cr_expiration: Cell::new(0),
            cr_queue: Cell::new(ptr::null()),
            cr_flags: Cell::new(0),
        }
    }
}

impl Default for Clockrequest {
    fn default() -> Self {
        Self::new()
    }
}

/// `CR_RESCHEDULE`: reschedule upon return.
pub const CR_RESCHEDULE: u32 = 0x0000_0001;

/// `struct clockqueue`: per-CPU clock interrupt state.
///
/// Struct member protections:
/// - a: modified atomically.
/// - I: immutable after initialization.
/// - m: per-queue mutex (`cq_mtx`).
/// - o: owned by a single CPU.
pub struct Clockqueue {
    /// \[o\] `cq_request`: callback request object.
    pub cq_request: Clockrequest,
    /// \[a\] `cq_mtx`: per-queue mutex.
    pub cq_mtx: Mutex,
    /// \[o\] `cq_uptime`: cached uptime.
    pub cq_uptime: Cell<u64>,
    /// \[m\] `cq_all`: established clockintr list.
    pub cq_all: TailqHead<ClockintrAll>,
    /// \[m\] `cq_pend`: pending clockintr list.
    pub cq_pend: TailqHead<ClockintrPend>,
    /// \[m\] `cq_running`: running clockintr.
    pub cq_running: Cell<*const Clockintr>,
    /// \[o\] `cq_hardclock`: hardclock handle.
    pub cq_hardclock: Clockintr,
    /// \[I\] `cq_intrclock`: local interrupt clock.
    pub cq_intrclock: Cell<Option<Intrclock>>,
    /// \[o\] `cq_stat`: dispatch statistics.
    pub cq_stat: Cell<ClockintrStat>,
    /// \[o\] `cq_gen` (volatile): `cq_stat` update generation.
    pub cq_gen: AtomicU32,
    /// \[o\] `cq_dispatch` (volatile): dispatch is running.
    pub cq_dispatch: AtomicU32,
    /// \[m\] `cq_flags`: `CQ_*` flags.
    pub cq_flags: Cell<u32>,
}

// SAFETY: one CPU's queue, touched by that CPU under `cq_mtx` (the lists and flags) or alone
// (the `[o]` members); the generation and dispatch words are atomics.
unsafe impl Sync for Clockqueue {}

impl Clockqueue {
    /// A queue before `clockqueue_init`: all zero, as the C's `cpu_info` static.
    pub const fn new() -> Self {
        Self {
            cq_request: Clockrequest::new(),
            cq_mtx: Mutex::new(IPL_CLOCK),
            cq_uptime: Cell::new(0),
            cq_all: TailqHead::new(),
            cq_pend: TailqHead::new(),
            cq_running: Cell::new(ptr::null()),
            cq_hardclock: Clockintr::new(),
            cq_intrclock: Cell::new(None),
            cq_stat: Cell::new(ClockintrStat {
                cs_dispatched: 0,
                cs_early: 0,
                cs_earliness: 0,
                cs_lateness: 0,
                cs_prompt: 0,
                cs_run: 0,
                cs_spurious: 0,
            }),
            cq_gen: AtomicU32::new(0),
            cq_dispatch: AtomicU32::new(0),
            cq_flags: Cell::new(0),
        }
    }
}

impl Default for Clockqueue {
    fn default() -> Self {
        Self::new()
    }
}

/// `CQ_INIT`: `clockintr_cpu_init()` done.
pub const CQ_INIT: u32 = 0x0000_0001;
/// `CQ_INTRCLOCK`: intrclock installed.
pub const CQ_INTRCLOCK: u32 = 0x0000_0002;
/// `CQ_IGNORE_REQUEST`: ignore callback requests.
pub const CQ_IGNORE_REQUEST: u32 = 0x0000_0004;
/// `CQ_NEED_WAKEUP`: caller at barrier.
pub const CQ_NEED_WAKEUP: u32 = 0x0000_0008;
/// `CQ_STATE_MASK`.
pub const CQ_STATE_MASK: u32 = 0x0000_000f;

/*
 * Kernel API
 */

/// `CL_BARRIER`: block if callback is running.
pub const CL_BARRIER: u32 = 0x0000_0001;
/// `CL_FLAG_MASK`.
pub const CL_FLAG_MASK: u32 = 0x0000_0001;
/* </CODE> */
