/*	$OpenBSD: eventvar.h,v 1.17 2022/07/09 12:48:21 visa Exp $	*/
/* <LICENSES> */
/*-
 * Copyright (c) 1999,2000 Jonathan Lemon <jlemon@FreeBSD.org>
 * All rights reserved.
 *
 * Redistribution and use in source and binary forms, with or without
 * modification, are permitted provided that the following conditions
 * are met:
 * 1. Redistributions of source code must retain the above copyright
 *    notice, this list of conditions and the following disclaimer.
 * 2. Redistributions in binary form must reproduce the above copyright
 *    notice, this list of conditions and the following disclaimer in the
 *    documentation and/or other materials provided with the distribution.
 *
 * THIS SOFTWARE IS PROVIDED BY THE AUTHOR AND CONTRIBUTORS ``AS IS'' AND
 * ANY EXPRESS OR IMPLIED WARRANTIES, INCLUDING, BUT NOT LIMITED TO, THE
 * IMPLIED WARRANTIES OF MERCHANTABILITY AND FITNESS FOR A PARTICULAR PURPOSE
 * ARE DISCLAIMED.  IN NO EVENT SHALL THE AUTHOR OR CONTRIBUTORS BE LIABLE
 * FOR ANY DIRECT, INDIRECT, INCIDENTAL, SPECIAL, EXEMPLARY, OR CONSEQUENTIAL
 * DAMAGES (INCLUDING, BUT NOT LIMITED TO, PROCUREMENT OF SUBSTITUTE GOODS
 * OR SERVICES; LOSS OF USE, DATA, OR PROFITS; OR BUSINESS INTERRUPTION)
 * HOWEVER CAUSED AND ON ANY THEORY OF LIABILITY, WHETHER IN CONTRACT, STRICT
 * LIABILITY, OR TORT (INCLUDING NEGLIGENCE OR OTHERWISE) ARISING IN ANY WAY
 * OUT OF THE USE OF THIS SOFTWARE, EVEN IF ADVISED OF THE POSSIBILITY OF
 * SUCH DAMAGE.
 *
 *	$FreeBSD: src/sys/sys/eventvar.h,v 1.3 2000/05/26 02:06:54 jake Exp $
 */
/* </LICENSES> */

/* <CODE> */
//! `<sys/eventvar.h>`: `struct kqueue`, the kernel side of a `kqueue(2)` descriptor (and of a
//! thread's `select(2)`/`poll(2)` queue, `p_kq`).
//!
//! Upstream: sys/sys/eventvar.h @ 3ce1f3f79392
//!
//! A kqueue is a reference-counted pool item (`kqueue_pool`, `kern_event.rs`): handed out
//! as `&'static Kqueue` by `kqueue_alloc`, given back by `KQRELE` on its last reference (the
//! `crget`/`crfree` idiom of `docs/C_TO_RUST.md`).
//!
//! ## Deviations
//! - The members `kq_lock` guards (`[q]`) are `Cell`s; `kq_knlist` and `kq_knhash` stay raw
//!   pointers to `malloc(M_KEVENT)` arrays of [`Knlist`] heads with their sizes, as in C,
//!   because `kqueue_expand_list` swaps the table for a bigger one; they are read through
//!   the bounds-checked [`Kqueue::knlist`] and [`Kqueue::knhash`].
//! - `kq_knhashmask` is `u64` (the C's `u_long`).

use core::cell::Cell;
use core::ptr;

use crate::kassert;
use crate::kern::subr_prf::panic;
use crate::machine::intr::IPL_HIGH;
use crate::queue_adapter;
use crate::sys::event::{Klist, KnTqe, Knlist};
use crate::sys::filedesc::Filedesc;
use crate::sys::mutex::Mutex;
use crate::sys::proc::Proc;
use crate::sys::queue::{ListEntry, TailqHead};
use crate::sys::refcnt::Refcnt;
use crate::sys::task::Task;

/// `KQ_NEVENTS`: minimize copy{in,out} calls.
pub const KQ_NEVENTS: usize = 8;
/// `KQEXTENT`: linear growth by this amount.
pub const KQEXTENT: i32 = 256;

/// `KQ_SLEEP`: a thread sleeps in `kqueue_scan` for events.
pub const KQ_SLEEP: i32 = 0x02;
/// `KQ_DYING`: the kqueue is being closed.
pub const KQ_DYING: i32 = 0x04;
/// `KQ_TASK`: `kq_task` was scheduled.
pub const KQ_TASK: i32 = 0x08;

/// `struct kqueue`.
///
/// Locking: \[I\] immutable after creation, \[L\] `kqueue_klist_lock`, \[a\] atomic
/// operations, \[q\] `kq_lock`.
pub struct Kqueue {
    /// `kq_lock`: lock for queue access.
    pub kq_lock: Mutex,
    /// \[q\] `kq_head`: list of pending event.
    pub kq_head: TailqHead<KnTqe>,
    /// \[q\] `kq_count`: # of pending events.
    pub kq_count: Cell<i32>,
    /// \[a\] `kq_refcnt`: # of references.
    pub kq_refcnt: Refcnt,
    /// \[L\] `kq_klist`: knotes of other kqs.
    pub kq_klist: Klist,
    /// \[I\] `kq_fdp`: fd table of this kq.
    pub kq_fdp: Cell<*const Filedesc>,
    /// `kq_next`: the link in the table's `fd_kqlist`.
    pub kq_next: ListEntry<Kqueue>,
    /// \[q\] `kq_nknotes`: # of registered knotes.
    pub kq_nknotes: Cell<u32>,
    /// \[q\] `kq_knlistsize`: size of `kq_knlist`.
    pub kq_knlistsize: Cell<i32>,
    /// \[q\] `kq_knlist`: list of attached knotes, indexed by descriptor.
    pub kq_knlist: Cell<*mut Knlist>,
    /// \[q\] `kq_knhashmask`: size of `kq_knhash`, minus one.
    pub kq_knhashmask: Cell<u64>,
    /// \[q\] `kq_knhash`: hash table for attached knotes.
    pub kq_knhash: Cell<*mut Knlist>,
    /// `kq_task`: deferring of activation.
    pub kq_task: Task,
    /// \[q\] `kq_state`: `KQ_*`.
    pub kq_state: Cell<i32>,
}

// SAFETY: the members are touched under `kq_lock` (or are atomics, or immutable after
// `kqueue_alloc`), as in C; the kernel runs one CPU.
unsafe impl Sync for Kqueue {}

impl Kqueue {
    /// A zeroed kqueue, as `pool_get(&kqueue_pool, PR_ZERO)` returns it.
    pub const fn new() -> Self {
        Self {
            kq_lock: Mutex::new(IPL_HIGH),
            kq_head: TailqHead::new(),
            kq_count: Cell::new(0),
            kq_refcnt: Refcnt::new(),
            kq_klist: Klist::new(),
            kq_fdp: Cell::new(ptr::null()),
            kq_next: ListEntry::new(),
            kq_nknotes: Cell::new(0),
            kq_knlistsize: Cell::new(0),
            kq_knlist: Cell::new(ptr::null_mut()),
            kq_knhashmask: Cell::new(0),
            kq_knhash: Cell::new(ptr::null_mut()),
            kq_task: Task::zeroed(),
            kq_state: Cell::new(0),
        }
    }

    /// `kq->kq_fdp`.
    pub fn fdp(&self) -> &Filedesc {
        // SAFETY: set by `kqueue_alloc` to the table the kqueue is attached to (it is on
        // that table's `fd_kqlist` until `KQRELE`), which outlives it.
        match unsafe { self.kq_fdp.get().as_ref() } {
            Some(fdp) => fdp,
            None => panic(format_args!("kqueue {:p}: no kq_fdp", self)),
        }
    }

    /// `&kq->kq_knlist[fd]`.
    pub fn knlist(&self, fd: usize) -> &Knlist {
        let size = self.kq_knlistsize.get() as usize;
        if fd >= size {
            panic(format_args!(
                "kqueue {:p}: kq_knlist[{fd}] out of {size}",
                self
            ));
        }
        // SAFETY: `kq_knlist` points at `kq_knlistsize` initialised heads (it is replaced
        // only together with the size, under `kq_lock`), and `fd` is below that.
        unsafe { &*self.kq_knlist.get().add(fd) }
    }

    /// `&kq->kq_knhash[i]`.
    pub fn knhash(&self, i: usize) -> &Knlist {
        let mask = self.kq_knhashmask.get() as usize;
        if self.kq_knhash.get().is_null() || i > mask {
            panic(format_args!(
                "kqueue {:p}: kq_knhash[{i}] past mask {mask}",
                self
            ));
        }
        // SAFETY: `kq_knhash` points at `kq_knhashmask + 1` initialised heads, set once by
        // `kqueue_expand_hash`, and `i` is at most the mask.
        unsafe { &*self.kq_knhash.get().add(i) }
    }
}

impl Default for Kqueue {
    fn default() -> Self {
        Self::new()
    }
}

impl Proc {
    /// `p->p_kq`: the thread's poll kqueue, which `kqpoll_init` made.
    pub fn kq(&self) -> &'static Kqueue {
        let kq = self.p_kq.get();
        kassert!(!kq.is_null());
        // SAFETY: non-null while the thread holds its reference (`kqpoll_init` until
        // `kqpoll_exit`); the kqueue is a pool item `KQRELE` frees only with the last one.
        unsafe { &*kq }
    }
}

queue_adapter!(
    /// `LIST_HEAD(, kqueue) fd_kqlist`: the kqueues attached to a descriptor table, through
    /// `kq_next`.
    pub KqList: Kqueue, kq_next => ListEntry<Kqueue>
);
/* </CODE> */
