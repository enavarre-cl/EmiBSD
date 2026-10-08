/*	$OpenBSD: task.h,v 1.18 2020/08/01 08:40:20 anton Exp $ */
/* <LICENSES> */
/*
 * Copyright (c) 2013 David Gwynne <dlg@openbsd.org>
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
//! `<sys/task.h>`: `task_add(9)`, the work items a task queue runs in a thread. The functions
//! and `struct taskq` live in `kern/kern_task.rs`.
//!
//! Upstream: sys/sys/task.h @ 3ce1f3f79392
//!
//! Status: `ported` (M7b).
//!
//! ## Deviations
//! - `struct taskq` is opaque in the header; Rust has no forward declarations, so the type is
//!   defined in `kern/kern_task.rs` (where the C defines it) with private fields and re-exported
//!   here together with `systq` and `systqmp`.
//! - `t_flags` is an `AtomicU32` (`Relaxed`): `task_add`, `task_del` and `task_pending` read
//!   it without the queue's mutex, as the C does, possibly from an interrupt handler.
//! - `t_func` is an `Option`: a task that `task_set` has not seen ([`Task::zeroed`], the C's
//!   zeroed `struct task`) has no function; `TASK_INITIALIZER` is [`Task::new`].
//! - `t_process` (the `kcov` identifier, `#if 1 /* NKCOV > 0 */`) is kept but never written:
//!   `kcov` is not configured.

use core::cell::Cell;
use core::ffi::c_void;
use core::ptr;
use core::sync::atomic::{AtomicU32, Ordering};

pub use crate::kern::kern_task::{SYSTQ, SYSTQMP, Taskq};
use crate::queue_adapter;
use crate::sys::proc::Process;
use crate::sys::queue::TailqEntry;

/// `TASK_ONQUEUE`: the task is on a queue's worklist.
pub const TASK_ONQUEUE: u32 = 1;

/// `TASKQ_MPSAFE`: the queue's threads run without the kernel lock.
pub const TASKQ_MPSAFE: u32 = 1 << 0;

/// `void (*t_func)(void *)`: the function a task calls.
pub type TaskFn = fn(*mut c_void);

/// `struct task`.
pub struct Task {
    /// `t_entry`: the link in the queue's worklist. Protected by: the queue's `tq_mtx`.
    pub t_entry: TailqEntry<Task>,
    /// `t_func`: function to call. Set by the owner (`task_set`) while the task is not pending.
    pub t_func: Cell<Option<TaskFn>>,
    /// `t_arg`: function argument. Set with `t_func`.
    pub t_arg: Cell<*mut c_void>,
    /// `t_flags`: `TASK_ONQUEUE`. Written under the queue's `tq_mtx`, read without it.
    pub t_flags: AtomicU32,
    /// `t_process`: kcov identifier (not configured, never written).
    pub t_process: Cell<*const Process>,
}

// SAFETY: the link and `t_flags` change under the queue's mutex; `t_func`, `t_arg` and
// `t_process` are written by the task's owner while it is not pending and read by the worker
// under the mutex, which is the C's contract for `task_set(9)`.
unsafe impl Sync for Task {}

impl Task {
    /// `TASK_INITIALIZER(_f, _a)`.
    pub const fn new(func: TaskFn, arg: *mut c_void) -> Self {
        Self {
            t_entry: TailqEntry::new(),
            t_func: Cell::new(Some(func)),
            t_arg: Cell::new(arg),
            t_flags: AtomicU32::new(0),
            t_process: Cell::new(ptr::null()),
        }
    }

    /// A task that `task_set` has not seen yet (all zero, like a `static struct task`).
    pub const fn zeroed() -> Self {
        Self {
            t_entry: TailqEntry::new(),
            t_func: Cell::new(None),
            t_arg: Cell::new(ptr::null_mut()),
            t_flags: AtomicU32::new(0),
            t_process: Cell::new(ptr::null()),
        }
    }
}

queue_adapter!(
    /// `TAILQ_HEAD(task_list, task)`.
    pub TaskList: Task, t_entry => TailqEntry<Task>
);

/// `task_pending(_t)`: is the task on a queue, waiting to run?
pub fn task_pending(t: &Task) -> bool {
    t.t_flags.load(Ordering::Relaxed) & TASK_ONQUEUE != 0
}
/* </CODE> */
