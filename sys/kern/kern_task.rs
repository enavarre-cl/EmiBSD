/*	$OpenBSD: kern_task.c,v 1.36 2025/01/13 03:21:10 mvs Exp $ */
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

//! Task queues: `kern/kern_task.c`, see `task_add(9)` and `taskq_create(9)`. A task queue
//! owns one or more kernel threads that run the tasks queued on it, in order, in process
//! context. `systq` (under the kernel lock) and `systqmp` (`TASKQ_MPSAFE`) exist from boot.
//!
//! Upstream: sys/kern/kern_task.c @ 3ce1f3f79392
//!
//! Status: `ported` (M7b): `taskq_init`, `taskq_create`, `taskq_destroy`,
//! `taskq_create_thread`, `taskq_barrier_task`, `taskq_do_barrier`, `taskq_barrier`,
//! `taskq_del_barrier`, `task_set`, `task_add`, `task_del`, `taskq_next_work`, `taskq_thread`.
//!
//! ## Deviations
//! - `WITNESS` is not configured: `taskq_lock_type`, `TASKQ_LOCK_FLAGS`, `tq_lock_object` and
//!   the `WITNESS_INIT`/`WITNESS_CHECKORDER`/`WITNESS_LOCK`/`WITNESS_UNLOCK` calls are absent.
//! - `kcov` is not configured: `task_add` does not record `t_process` and `taskq_thread` has
//!   no `kcov_remote_enter`/`kcov_remote_leave`.
//! - `TASKQ_MPSAFE` is ignored (M11a): a worker never drops the kernel lock its kthread
//!   starts with, so every task runs under it, `systqmp`'s too, until the M11e audit; the C
//!   unlocks around an `MPSAFE` queue's loop. Everything else (several threads per queue,
//!   the barrier's generation count) is kept.
//! - `task_add_local` is `task_add` for a task that is not `'static` (an `unsafe fn`): what
//!   `sched_barrier` queues from its stack frame.
//! - `struct taskq` is `#[repr(C)]` with `tq_state` first so that the queue's own address (the
//!   "bored" and destroy wakeup channel) never equals `&tq_running`, `&tq_bthreads` or
//!   `&tq_bgen`, the other channels, as in C.
//! - `taskq_create` returns `Option<&'static Taskq>` and `taskq_destroy` takes the
//!   `NonNull<Taskq>` back and is `unsafe` (it frees); `task_add` takes a `&'static Task`,
//!   since a pending task must outlive its place on the worklist (`task_add(9)`).
//! - `task_add`, `task_del` and `taskq_next_work` return `bool` for the C's `0`/`1`;
//!   `taskq_next_work` returns the copy of the task (`*work = *next`) instead of filling an
//!   out parameter.
//! - A task without a function (a `Task::zeroed` that `task_set` never saw) panics in
//!   `taskq_thread`, where the C would call through a null pointer.

use core::cell::Cell;
use core::ffi::c_void;
use core::ptr::{self, NonNull};
use core::sync::atomic::Ordering;

use crate::kern::kern_kthread::{kthread_create, kthread_create_deferred, kthread_exit};
use crate::kern::kern_lock::{mtx_enter, mtx_leave};
use crate::kern::kern_malloc::{free, malloc};
use crate::kern::kern_synch::{msleep_nsec, wakeup, wakeup_one};
use crate::kern::sched_bsd::r#yield;
use crate::kern::subr_prf::{Str, panic, printf};
use crate::machine::cpu::curproc;
use crate::machine::intr::IPL_HIGH;
use crate::queue_adapter;
use crate::sys::malloc::{M_DEVBUF, M_WAITOK};
use crate::sys::mutex::Mutex;
use crate::sys::param::PWAIT;
use crate::sys::proc::Proc;
use crate::sys::queue::{SlistEntry, SlistHead, TailqHead};
use crate::sys::sched::sched_pause;
use crate::sys::systm::INFSLP;
use crate::sys::task::{TASK_ONQUEUE, TASKQ_MPSAFE, Task, TaskFn, TaskList};

/// `taskq_sys_name`.
const TASKQ_SYS_NAME: &[u8] = b"systq";
/// `taskq_sys_mp_name`.
const TASKQ_SYS_MP_NAME: &[u8] = b"systqmp";

/// `struct taskq_thread`: a worker of a queue, on the worker's own stack.
struct TaskqThread {
    /// `tt_entry`. Protected by: the queue's `tq_mtx`.
    tt_entry: SlistEntry<TaskqThread>,
    /// `tt_thread`: the worker.
    tt_thread: *const Proc,
}

queue_adapter!(
    /// `SLIST_HEAD(taskq_threads, taskq_thread)`.
    TaskqThreads: TaskqThread, tt_entry => SlistEntry<TaskqThread>
);

/// `tq_state`'s anonymous `enum`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[repr(u32)]
enum TqState {
    /// `TQ_S_CREATED`: no thread yet (`taskq_create_thread` has not run).
    Created,
    /// `TQ_S_RUNNING`: the threads exist.
    Running,
    /// `TQ_S_DESTROYED`: `taskq_destroy` was called.
    Destroyed,
}

/// `struct taskq`: opaque outside this file, as in C (see the module's deviations for the
/// layout).
#[repr(C)]
pub struct Taskq {
    /// `tq_state`. Protected by: `tq_mtx`.
    tq_state: Cell<TqState>,
    /// `tq_running`: threads created and not yet exited. Protected by: `tq_mtx`.
    tq_running: Cell<u32>,
    /// `tq_nthreads`: threads wanted. Immutable after creation.
    tq_nthreads: u32,
    /// `tq_flags`: `TASKQ_MPSAFE`. Immutable after creation.
    tq_flags: u32,
    /// `tq_name`: the threads' name. Immutable after creation.
    tq_name: &'static [u8],

    /// `tq_mtx`.
    tq_mtx: Mutex,
    /// `tq_worklist`. Protected by: `tq_mtx`.
    tq_worklist: TailqHead<TaskList>,

    /// `tq_threads`. Protected by: `tq_mtx`.
    tq_threads: SlistHead<TaskqThreads>,
    /// `tq_barriers`: barriers in progress. Protected by: `tq_mtx`.
    tq_barriers: Cell<u32>,
    /// `tq_bgen`: barrier generation. Protected by: `tq_mtx`.
    tq_bgen: Cell<u32>,
    /// `tq_bthreads`: threads parked in a barrier. Protected by: `tq_mtx`.
    tq_bthreads: Cell<u32>,
}

// SAFETY: every `Cell`, the worklist and the thread list are touched under `tq_mtx`; the other
// fields are immutable after creation.
unsafe impl Sync for Taskq {}

impl Taskq {
    /// The C's static initialisers and the field-by-field setup of `taskq_create`: a queue in
    /// `TQ_S_CREATED` with an empty worklist and a mutex at `ipl`.
    const fn new(name: &'static [u8], nthreads: u32, ipl: i32, flags: u32) -> Self {
        Self {
            tq_state: Cell::new(TqState::Created),
            tq_running: Cell::new(0),
            tq_nthreads: nthreads,
            tq_flags: flags,
            tq_name: name,
            // MUTEX_INITIALIZER_FLAGS(ipl, name, 0) / mtx_init_flags: the name is WITNESS's.
            tq_mtx: Mutex::new(ipl),
            tq_worklist: TailqHead::new(),
            tq_threads: SlistHead::new(),
            tq_barriers: Cell::new(0),
            tq_bgen: Cell::new(0),
            tq_bthreads: Cell::new(0),
        }
    }
}

/// `taskq_sys`.
static TASKQ_SYS: Taskq = Taskq::new(TASKQ_SYS_NAME, 1, IPL_HIGH, 0);
/// `taskq_sys_mp`.
static TASKQ_SYS_MP: Taskq = Taskq::new(TASKQ_SYS_MP_NAME, 1, IPL_HIGH, TASKQ_MPSAFE);

/// `systq`: the system task queue, one thread, under the kernel lock.
pub static SYSTQ: &Taskq = &TASKQ_SYS;
/// `systqmp`: the MP-safe system task queue, one thread.
pub static SYSTQMP: &Taskq = &TASKQ_SYS_MP;

/// A queue as the `void *` argument of its threads and deferred creation.
fn taskq_arg(tq: &Taskq) -> *mut c_void {
    ptr::from_ref(tq).cast_mut().cast()
}

/// `taskq_init`: called in `init_main.c`; defers the creation of the system queues' threads
/// until `kthread_run_deferred_queue`.
pub fn taskq_init() {
    // WITNESS_INIT(&systq->tq_lock_object, ...): not configured.
    kthread_create_deferred(taskq_create_thread, taskq_arg(SYSTQ));
    // WITNESS_INIT(&systqmp->tq_lock_object, ...): not configured.
    kthread_create_deferred(taskq_create_thread, taskq_arg(SYSTQMP));
}

/// `taskq_create`: a queue named `name` served by `nthreads` threads, its mutex at `ipl`.
/// `None` when the allocation fails (the C's `NULL`).
pub fn taskq_create(
    name: &'static [u8],
    nthreads: u32,
    ipl: i32,
    flags: u32,
) -> Option<&'static Taskq> {
    let tq = malloc(size_of::<Taskq>(), M_DEVBUF, M_WAITOK)?.cast::<Taskq>();
    // SAFETY: a fresh allocation of `size_of::<Taskq>()` bytes (malloc's chunks are aligned
    // to their power-of-two size, more than `Taskq` needs), written once before any use.
    unsafe { tq.as_ptr().write(Taskq::new(name, nthreads, ipl, flags)) };
    // SAFETY: initialised just above; it stays allocated until `taskq_destroy` or
    // `taskq_create_thread` frees it.
    let tq: &'static Taskq = unsafe { tq.as_ref() };

    // WITNESS: tq_lock_object, witness_init: not configured.

    // try to create a thread to guarantee that tasks will be serviced
    kthread_create_deferred(taskq_create_thread, taskq_arg(tq));

    Some(tq)
}

/// `taskq_destroy`: stops the queue's threads, waits for them to exit and frees the queue.
/// Tasks still on the worklist are dropped, as in C.
///
/// # Safety
///
/// `tq` came from [`taskq_create`] and is not destroyed twice; nobody uses the queue (adds a
/// task, runs a barrier) once this is called.
pub unsafe fn taskq_destroy(tq: NonNull<Taskq>) {
    {
        // SAFETY: the caller vouches that `tq` is a live queue from `taskq_create`.
        let tq = unsafe { tq.as_ref() };
        mtx_enter(&tq.tq_mtx);
        match tq.tq_state.get() {
            TqState::Created => {
                // tq is still referenced by taskq_create_thread
                tq.tq_state.set(TqState::Destroyed);
                mtx_leave(&tq.tq_mtx);
                return;
            }
            TqState::Running => tq.tq_state.set(TqState::Destroyed),
            state => panic(format_args!(
                "unexpected {} tq state {}",
                Str(tq.tq_name),
                state as u32
            )),
        }

        while tq.tq_running.get() > 0 {
            wakeup(ptr::from_ref(tq));
            let _ = msleep_nsec(
                ptr::from_ref(&tq.tq_running),
                &tq.tq_mtx,
                PWAIT,
                "tqdestroy",
                INFSLP,
            );
        }
        mtx_leave(&tq.tq_mtx);
    }

    free(tq.cast::<u8>(), M_DEVBUF, size_of::<Taskq>());
}

/// `taskq_create_thread`: creates the queue's threads (deferred from `taskq_init` and
/// `taskq_create`), or frees a queue destroyed before it got any.
pub fn taskq_create_thread(arg: *mut c_void) {
    // SAFETY: `arg` is the queue `taskq_init` or `taskq_create` deferred this call for; while
    // it is `TQ_S_CREATED` nobody else frees it (`taskq_destroy` leaves that to us), and once
    // running it lives until its threads have exited.
    let tq = unsafe { &*arg.cast::<Taskq>() };

    mtx_enter(&tq.tq_mtx);

    match tq.tq_state.get() {
        TqState::Destroyed => {
            mtx_leave(&tq.tq_mtx);
            // Only a queue from `taskq_create` can be destroyed, so it came from malloc.
            free(NonNull::from(tq).cast::<u8>(), M_DEVBUF, size_of::<Taskq>());
            return;
        }
        TqState::Created => tq.tq_state.set(TqState::Running),
        state => panic(format_args!(
            "unexpected {} tq state {}",
            Str(tq.tq_name),
            state as u32
        )),
    }

    loop {
        tq.tq_running.set(tq.tq_running.get() + 1);
        mtx_leave(&tq.tq_mtx);

        let rv = kthread_create(taskq_thread, arg, tq.tq_name);

        mtx_enter(&tq.tq_mtx);
        if rv.is_err() {
            printf(format_args!(
                "unable to create thread for \"{}\" taskq\n",
                Str(tq.tq_name)
            ));

            tq.tq_running.set(tq.tq_running.get() - 1);
            // could have been destroyed during kthread_create
            if tq.tq_state.get() == TqState::Destroyed && tq.tq_running.get() == 0 {
                wakeup_one(ptr::from_ref(&tq.tq_running));
            }
            break;
        }
        if tq.tq_running.get() >= tq.tq_nthreads {
            break;
        }
    }

    mtx_leave(&tq.tq_mtx);
}

/// `taskq_barrier_task`: the task a barrier queues; it parks the worker that runs it until the
/// barrier's generation moves on.
pub fn taskq_barrier_task(p: *mut c_void) {
    // SAFETY: `p` is the queue `taskq_do_barrier` queued this task on; the barrier's caller
    // keeps the queue alive until the barrier returns, which is after this task's wakeup.
    let tq = unsafe { &*p.cast::<Taskq>() };

    mtx_enter(&tq.tq_mtx);
    tq.tq_bthreads.set(tq.tq_bthreads.get() + 1);
    wakeup(ptr::from_ref(&tq.tq_bthreads));

    let generation = tq.tq_bgen.get();
    loop {
        let _ = msleep_nsec(
            ptr::from_ref(&tq.tq_bgen),
            &tq.tq_mtx,
            PWAIT,
            "tqbarend",
            INFSLP,
        );
        if generation != tq.tq_bgen.get() {
            break;
        }
    }
    mtx_leave(&tq.tq_mtx);
}

/// `taskq_do_barrier`: waits until every thread of the queue has finished the task it was
/// running, by parking each of them in [`taskq_barrier_task`].
fn taskq_do_barrier(tq: &Taskq) {
    let t = Task::new(taskq_barrier_task, taskq_arg(tq));
    let thread: *const Proc = curproc().map_or(ptr::null(), ptr::from_ref);

    mtx_enter(&tq.tq_mtx);
    tq.tq_barriers.set(tq.tq_barriers.get() + 1);

    // is the barrier being run from a task inside the taskq?
    if tq.tq_threads.iter().any(|tt| ptr::eq(tt.tt_thread, thread)) {
        tq.tq_bthreads.set(tq.tq_bthreads.get() + 1);
        wakeup(ptr::from_ref(&tq.tq_bthreads));
    }

    while tq.tq_bthreads.get() < tq.tq_nthreads {
        // shove the task into the queue for a worker to pick up
        t.t_flags.fetch_or(TASK_ONQUEUE, Ordering::Relaxed);
        // SAFETY: `t` is in no list: it is fresh, or a worker took it off (`taskq_next_work`),
        // or it was removed below; it is off the list again before this function returns and
        // `t` goes out of scope; under `tq_mtx`.
        unsafe { tq.tq_worklist.insert_tail(&t) };
        wakeup_one(ptr::from_ref(tq));

        let _ = msleep_nsec(
            ptr::from_ref(&tq.tq_bthreads),
            &tq.tq_mtx,
            PWAIT,
            "tqbar",
            INFSLP,
        );

        // another thread running a barrier might have done this work for us.
        if t.t_flags.load(Ordering::Relaxed) & TASK_ONQUEUE != 0 {
            // SAFETY: still flagged, so no worker took it: it is on the worklist; under
            // `tq_mtx`.
            unsafe { tq.tq_worklist.remove(&t) };
        }
    }

    tq.tq_barriers.set(tq.tq_barriers.get() - 1);
    if tq.tq_barriers.get() == 0 {
        // we're the last one out
        tq.tq_bgen.set(tq.tq_bgen.get().wrapping_add(1));
        wakeup(ptr::from_ref(&tq.tq_bgen));
        tq.tq_bthreads.set(0);
    } else {
        let generation = tq.tq_bgen.get();
        loop {
            let _ = msleep_nsec(
                ptr::from_ref(&tq.tq_bgen),
                &tq.tq_mtx,
                PWAIT,
                "tqbarwait",
                INFSLP,
            );
            if generation != tq.tq_bgen.get() {
                break;
            }
        }
    }
    mtx_leave(&tq.tq_mtx);
}

/// `taskq_barrier`: returns once every task that was running on `tq` when it was called has
/// finished.
pub fn taskq_barrier(tq: &Taskq) {
    // WITNESS_CHECKORDER(&tq->tq_lock_object, LOP_NEWORDER, NULL): not configured.

    taskq_do_barrier(tq);
}

/// `taskq_del_barrier`: removes `t` from `tq` if it is pending, then waits for a running
/// instance of it (or anything else running on `tq`) to finish.
pub fn taskq_del_barrier(tq: &Taskq, t: &Task) {
    // WITNESS_CHECKORDER(&tq->tq_lock_object, LOP_NEWORDER, NULL): not configured.

    let _ = task_del(tq, t);
    taskq_do_barrier(tq);
}

/// `task_set`: prepares `t` to call `func(arg)`. The task must not be pending.
pub fn task_set(t: &Task, func: TaskFn, arg: *mut c_void) {
    t.t_func.set(Some(func));
    t.t_arg.set(arg);
    t.t_flags.store(0, Ordering::Relaxed);
}

/// `task_add`: queues `w` on `tq` and wakes a worker. Returns `true` if it was queued, `false`
/// if it was already pending.
pub fn task_add(tq: &Taskq, w: &'static Task) -> bool {
    // SAFETY: a `'static` task outlives its place on the worklist.
    unsafe { task_add_local(tq, w) }
}

/// `task_add` for a task that is not `'static`, such as one on the caller's stack frame
/// (`sched_barrier`'s): queues `w` on `tq` and wakes a worker.
///
/// # Safety
///
/// `w` stays valid and in place until a worker has taken it off the worklist (it ran) or
/// `task_del` removed it.
pub unsafe fn task_add_local(tq: &Taskq, w: &Task) -> bool {
    let mut rv = false;

    if w.t_flags.load(Ordering::Relaxed) & TASK_ONQUEUE != 0 {
        return false;
    }

    mtx_enter(&tq.tq_mtx);
    if w.t_flags.load(Ordering::Relaxed) & TASK_ONQUEUE == 0 {
        rv = true;
        w.t_flags.fetch_or(TASK_ONQUEUE, Ordering::Relaxed);
        // SAFETY: not ONQUEUE, so in no worklist; valid while linked (the caller's contract);
        // under `tq_mtx`.
        unsafe { tq.tq_worklist.insert_tail(w) };
        // NKCOV > 0: w->t_process = curproc->p_p.
    }
    mtx_leave(&tq.tq_mtx);

    if rv {
        wakeup_one(ptr::from_ref(tq));
    }

    rv
}

/// `task_del`: takes `w` off `tq` if it is pending. Returns `true` if it was removed. A task
/// that is already running is not waited for (see [`taskq_del_barrier`]).
pub fn task_del(tq: &Taskq, w: &Task) -> bool {
    let mut rv = false;

    if w.t_flags.load(Ordering::Relaxed) & TASK_ONQUEUE == 0 {
        return false;
    }

    mtx_enter(&tq.tq_mtx);
    if w.t_flags.load(Ordering::Relaxed) & TASK_ONQUEUE != 0 {
        rv = true;
        w.t_flags.fetch_and(!TASK_ONQUEUE, Ordering::Relaxed);
        // SAFETY: ONQUEUE on this queue (the caller's contract, as in C), so linked in its
        // worklist; under `tq_mtx`.
        unsafe { tq.tq_worklist.remove(w) };
    }
    mtx_leave(&tq.tq_mtx);

    rv
}

/// `taskq_next_work`: takes the first task off the worklist, sleeping while it is empty.
/// Returns a copy of the task (the C's `*work = *next`, so the caller does not race with
/// the task's owner), or `None` once the queue is no longer running and has no work.
fn taskq_next_work(tq: &Taskq) -> Option<Task> {
    mtx_enter(&tq.tq_mtx);
    let next = loop {
        if let Some(next) = tq.tq_worklist.first() {
            break next;
        }
        if tq.tq_state.get() != TqState::Running {
            mtx_leave(&tq.tq_mtx);
            return None;
        }

        let _ = msleep_nsec(ptr::from_ref(tq), &tq.tq_mtx, PWAIT, "bored", INFSLP);
    };

    // SAFETY: `next` is the worklist's first element; under `tq_mtx`.
    unsafe { tq.tq_worklist.remove(next) };
    next.t_flags.fetch_and(!TASK_ONQUEUE, Ordering::Relaxed);

    // copy to caller to avoid races
    let work = Task::zeroed();
    work.t_func.set(next.t_func.get());
    work.t_arg.set(next.t_arg.get());
    work.t_flags
        .store(next.t_flags.load(Ordering::Relaxed), Ordering::Relaxed);
    work.t_process.set(next.t_process.get());

    let more = !tq.tq_worklist.is_empty();
    mtx_leave(&tq.tq_mtx);

    if more && tq.tq_nthreads > 1 {
        wakeup_one(ptr::from_ref(tq));
    }

    Some(work)
}

/// `taskq_thread`: a queue's worker: runs tasks until the queue is destroyed, then exits.
pub fn taskq_thread(xtq: *mut c_void) {
    let me = TaskqThread {
        tt_entry: SlistEntry::new(),
        tt_thread: curproc().map_or(ptr::null(), ptr::from_ref),
    };
    // SAFETY: `xtq` is the queue `taskq_create_thread` started this thread for; it stays
    // allocated while `tq_running` counts this thread (`taskq_destroy` waits for zero).
    let tq = unsafe { &*xtq.cast::<Taskq>() };

    // if ISSET(tq->tq_flags, TASKQ_MPSAFE) KERNEL_UNLOCK(): M11a keeps every queue under the
    // kernel lock (see the module's deviations); kthreads start holding it.

    mtx_enter(&tq.tq_mtx);
    // SAFETY: `me` is in no list and lives on this thread's stack until it is removed below
    // (`kthread_exit` does not return); under `tq_mtx`.
    unsafe { tq.tq_threads.insert_head(&me) };
    mtx_leave(&tq.tq_mtx);

    // WITNESS_CHECKORDER(&tq->tq_lock_object, LOP_NEWORDER, NULL): not configured.

    while let Some(work) = taskq_next_work(tq) {
        // WITNESS_LOCK, kcov_remote_enter: not configured.
        let Some(func) = work.t_func.get() else {
            panic(format_args!(
                "taskq_thread: {} task without a function",
                Str(tq.tq_name)
            ));
        };
        func(work.t_arg.get());
        // kcov_remote_leave, WITNESS_UNLOCK: not configured.
        sched_pause(r#yield);
    }

    mtx_enter(&tq.tq_mtx);
    // SAFETY: inserted above, still linked; under `tq_mtx`.
    unsafe { tq.tq_threads.remove(&me) };
    tq.tq_running.set(tq.tq_running.get() - 1);
    let last = tq.tq_running.get() == 0;
    // The channel's address, taken while the queue is surely alive: once `tq_running` is zero
    // the destroyer may free it.
    let running = ptr::from_ref(&tq.tq_running);
    mtx_leave(&tq.tq_mtx);

    // if ISSET(tq->tq_flags, TASKQ_MPSAFE) KERNEL_LOCK(): never released, see above.

    if last {
        wakeup_one(running);
    }

    kthread_exit(0);
}

// The queue's own address is a wakeup channel distinct from its counters' (see the module's
// deviations).
const _: () = {
    assert!(core::mem::offset_of!(Taskq, tq_state) == 0);
};

#[cfg(test)]
mod tests;
