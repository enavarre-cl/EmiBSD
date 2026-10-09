/* <LICENSES> */
/*
 * Copyright (c) 2026 Emilio Navarrete Lineros <enavarre@outlook.com>
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

/*
 * Copyright (c) 2015 David Gwynne <dlg@openbsd.org>
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
//! Interface send and receive queues: `<net/ifq.h>` and `net/ifq.c`, see `ifq_enq(9)`.
//!
//! Upstream: sys/net/ifq.h @ 3ce1f3f79392
//! Upstream: sys/net/ifq.c @ 3ce1f3f79392
//!
//! `struct ifqueue` sits between the network stack and a driver's transmission of packets:
//! when the stack has finished generating a packet it queues it on an ifqueue
//! (`ifq_enqueue`) and notifies the driver to start transmission (`ifq_start`). A network
//! device may have several transmit rings, each with its own ifqueue (`if_attach_queues`).
//! The ifqueue is also where traffic conditioning happens: the queue keeps its packets in a
//! conditioner described by a `struct ifq_ops` (`priq`, the default, keeps one list per
//! priority and drops from a lower priority when full). The ifqueue API takes the queue lock
//! on behalf of the conditioners, so they only have to keep or reject mbufs; a conditioner
//! that drops a packet while the lock is held hands it to `ifq_mfreem`, which frees it after
//! the lock is released.
//!
//! An ifqueue also serialises work: `ifq_start`, `ifq_restart` and `ifq_serialize` run their
//! tasks one at a time, on the first CPU that dispatches work; a second CPU only queues its
//! task and returns. `if_qstart` therefore never runs twice at once for one ring.
//!
//! `struct ifiqueue` is the receive side: a driver hands a list of packets to `ifiq_input`
//! (`if_input`), which counts them and queues them for the interface's softnet task, where
//! `ifiq_process` passes them to `if_input_process`.
//!
//! Drivers follow the patterns of `ifq.h`: an MP-safe driver sets `IFXF_MPSAFE` and
//! `if_qstart` before `if_attach`; its start routine dequeues with `ifq_dequeue` until the
//! ring is full (then `ifq_set_oactive`); its transmit completion calls `ifq_restart` when
//! `ifq_is_oactive`; bringing the interface down clears `IFF_RUNNING` and waits with
//! `ifq_barrier` before freeing what the start routine uses.
//!
//! Status: `ported` (M7b).
//!
//! ## Deviations
//! - `struct ifqueue` and `struct ifiqueue` are shared as `&'static`: they are embedded in a
//!   `struct ifnet` (`if_snd`, `if_rcv`) or `malloc`ed by `if_attach_queues`, and their tasks
//!   go on task queues that keep a reference. Their members are `Cell`s and atomics
//!   (`ifq_len` and `ifq_oactive` are read without the mutex, the C's `READ_ONCE`), so the
//!   all-zero queue is valid, as inside an `M_ZERO` softc.
//! - The `_ifq_ptr`/`_ifiq_ptr` unions are side-by-side fields (`ifq_softc` and the
//!   one-element map `ifq_ifqs`), `docs/C_TO_RUST.md`: the map is what `if_ifqs` points at
//!   until `if_attach_queues` allocates a real one.
//! - `struct ifq_ops` is a table of `unsafe fn`s: each operation trusts that `ifq_q` was made
//!   by the same table's `ifqop_alloc`, which `ifq_init` and `ifq_attach` keep true.
//!   `ifqop_deq_begin` returns the packet with its cookie instead of filling `void **`.
//! - `kstat(4)` is not ported: the `NKSTAT > 0` code (`ifq_kstat_tpl`, `ifq_kstat_copy`,
//!   `ifiq_kstat_*`, the `kstat_create`/`kstat_install` in `ifq_init`/`ifiq_init` and the
//!   `kstat_destroy` calls) is not compiled, as with `NKSTAT` 0; `ifq_kstat`/`ifiq_kstat`
//!   stay NULL.
//! - `bpf(4)` is configured: `ifiq_input` and `ifiq_enqueue_qlim` hand each packet to the
//!   queue's and the interface's taps (`ifiq_bpfp`, `if_bpf`) through `if_bpf_mtap`, which
//!   `if_attach_common` always sets; a hook left unset would tap nothing.
//! - `ifq_serialize` takes a `&'static Task` (the task may run on another CPU after the call
//!   returns); `ifq_barrier`'s task on its own stack goes through the private
//!   `ifq_serialize_task`, whose caller waits for it.
//! - `priq_alloc` panics when `malloc(M_WAITOK)` fails, where the C would sleep (`malloc(9)`
//!   does not sleep yet, `kern/kern_malloc.rs`).
//! - `ifq_deq_sleep` returns the packet instead of filling `struct mbuf **`; the `volatile
//!   unsigned int *` counters are `&AtomicU32`. `ifiq_input` returns `bool` (the C's "the
//!   queue is under pressure"), `ifiq_enqueue_qlim` `Result<(), Errno>` (always `Ok`, as the
//!   C always returns 0).
//! - `ifq_len`, `ifq_empty`, `ifiq_len` and `ifiq_empty` are functions; `ifq_q_enter` returns
//!   `Option<NonNull<c_void>>` (NULL is `None`).
//! - `net_ifiq_sysctl`'s `#if 0` pressure code is not translated (it is compiled out in C);
//!   the function returns `EOPNOTSUPP` as the C does.

use core::cell::Cell;
use core::cmp::min;
use core::ffi::c_void;
use core::ptr::{self, NonNull};
use core::sync::atomic::{AtomicU32, Ordering};

use crate::kassert;
use crate::kern::kern_lock::{mtx_enter, mtx_init, mtx_leave};
use crate::kern::kern_malloc::{free, malloc};
use crate::kern::kern_synch::{cond_signal_handler, cond_wait, msleep_nsec};
use crate::kern::kern_task::{task_add, task_del, task_set, taskq_del_barrier};
use crate::kern::subr_prf::panic;
use crate::kern::uipc_mbuf::{m_freem, ml_dequeue, ml_enlist, ml_enqueue, ml_init, ml_purge};
use crate::machine::Machine;
use crate::machine::cpu::Cpu;
use crate::machine::intr::IPL_NET;
use crate::net::bpf::BPF_DIRECTION_IN;
use crate::net::if_::{IFF_RUNNING, IFQ_MAXPRIO, IFQ_NQUEUES, IFXF_MONITOR, IfData};
use crate::net::if_::{if_input_process, net_tq};
use crate::net::if_var::Ifnet;
use crate::sys::errno::Errno;
use crate::sys::malloc::{M_DEVBUF, M_WAITOK};
use crate::sys::mbuf::{M_FLOWID, M_MCAST, Mbuf, MbufList, mbuf_list_first, ml_empty, ml_len};
use crate::sys::mutex::{Mutex, mutex_assert_locked};
use crate::sys::proc::Cond;
use crate::sys::queue::TailqHead;
use crate::sys::systm::INFSLP;
use crate::sys::task::{TASK_ONQUEUE, Task, TaskList, Taskq};

/// `IFQ_MAXLEN`: the default length of a send queue.
pub const IFQ_MAXLEN: u32 = 256;

/// `struct ifqueue`: an interface send queue (see the module's documentation).
pub struct Ifqueue {
    /// `ifq_if`: the interface.
    pub ifq_if: Cell<Option<&'static Ifnet>>,
    /// `ifq_softnet`: the softnet task queue the queue's work goes to.
    pub ifq_softnet: Cell<Option<&'static Taskq>>,
    /// `ifq_softc` (`_ifq_ptr._ifq_softc`): the driver's ring.
    pub ifq_softc: Cell<*mut c_void>,
    /// `ifq_ifqs` (`_ifq_ptr._ifq_ifqs`): a one-element map of send queues, borrowed for the
    /// interface's default map.
    pub ifq_ifqs: [Cell<*const Ifqueue>; 1],

    // mbuf handling
    /// `ifq_mtx`.
    pub ifq_mtx: Mutex,
    /// `ifq_ops`: the traffic conditioner. Protected by: `ifq_mtx`.
    pub ifq_ops: Cell<Option<&'static IfqOps>>,
    /// `ifq_q`: the conditioner's state. Protected by: `ifq_mtx`.
    pub ifq_q: Cell<*mut c_void>,
    /// `ifq_free`: mbufs to free once `ifq_mtx` is released. Protected by: `ifq_mtx`.
    pub ifq_free: MbufList,
    /// `ifq_len`: packets queued. Written under `ifq_mtx`, read without it.
    pub ifq_len: AtomicU32,
    /// `ifq_oactive`: the driver's ring is full.
    pub ifq_oactive: AtomicU32,

    // statistics
    /// `ifq_packets`. Protected by: `ifq_mtx`.
    pub ifq_packets: Cell<u64>,
    /// `ifq_bytes`. Protected by: `ifq_mtx`.
    pub ifq_bytes: Cell<u64>,
    /// `ifq_qdrops`. Protected by: `ifq_mtx`.
    pub ifq_qdrops: Cell<u64>,
    /// `ifq_errors`. Protected by: `ifq_mtx`.
    pub ifq_errors: Cell<u64>,
    /// `ifq_mcasts`. Protected by: `ifq_mtx`.
    pub ifq_mcasts: Cell<u64>,
    /// `ifq_oactives`. Protected by: `ifq_mtx`.
    pub ifq_oactives: Cell<u32>,

    /// `ifq_kstat` (`struct kstat *`, not configured).
    pub ifq_kstat: Cell<*mut c_void>,

    // work serialisation
    /// `ifq_task_mtx`.
    pub ifq_task_mtx: Mutex,
    /// `ifq_task_list`: work waiting for the serialiser. Protected by: `ifq_task_mtx`.
    pub ifq_task_list: TailqHead<TaskList>,
    /// `ifq_serializer`: the CPU running the work, NULL when none. Protected by:
    /// `ifq_task_mtx`.
    pub ifq_serializer: Cell<*const ()>,
    /// `ifq_bundle`: the deferred `ifq_start` (transmit mitigation).
    pub ifq_bundle: Task,

    // work to be serialised
    /// `ifq_start`.
    pub ifq_start: Task,
    /// `ifq_restart`.
    pub ifq_restart: Task,

    // properties
    /// `ifq_maxlen`.
    pub ifq_maxlen: Cell<u32>,
    /// `ifq_idx`.
    pub ifq_idx: Cell<u32>,
}

// SAFETY: the cells change under `ifq_mtx` or `ifq_task_mtx` as their docs say, or while the
// queue is set up and not yet shared (`ifq_init`); the counters read unlocked are atomics.
unsafe impl Sync for Ifqueue {}

/// `struct ifiqueue`: an interface receive queue.
pub struct Ifiqueue {
    /// `ifiq_if`: the interface.
    pub ifiq_if: Cell<Option<&'static Ifnet>>,
    /// `ifiq_bpfp`: a per-queue `bpf(4)` tap (`caddr_t *`).
    pub ifiq_bpfp: Cell<*mut *mut u8>,
    /// `ifiq_softnet`: the softnet task queue that processes the packets.
    pub ifiq_softnet: Cell<Option<&'static Taskq>>,
    /// `ifiq_softc` (`_ifiq_ptr._ifiq_softc`).
    pub ifiq_softc: Cell<*mut c_void>,
    /// `ifiq_ifiqs` (`_ifiq_ptr._ifiq_ifiqs`): a one-element map, as `ifq_ifqs`.
    pub ifiq_ifiqs: [Cell<*const Ifiqueue>; 1],

    /// `ifiq_mtx`.
    pub ifiq_mtx: Mutex,
    /// `ifiq_ml`: packets waiting for `ifiq_process`. Protected by: `ifiq_mtx`.
    pub ifiq_ml: MbufList,
    /// `ifiq_task`: `ifiq_process`.
    pub ifiq_task: Task,
    /// `ifiq_pressure`.
    pub ifiq_pressure: Cell<u32>,

    // counters
    /// `ifiq_packets`. Protected by: `ifiq_mtx`.
    pub ifiq_packets: Cell<u64>,
    /// `ifiq_bytes`. Protected by: `ifiq_mtx`.
    pub ifiq_bytes: Cell<u64>,
    /// `ifiq_fdrops`: dropped by a filter. Protected by: `ifiq_mtx`.
    pub ifiq_fdrops: Cell<u64>,
    /// `ifiq_qdrops`: dropped because the queue was full. Protected by: `ifiq_mtx`.
    pub ifiq_qdrops: Cell<u64>,
    /// `ifiq_errors`. Protected by: `ifiq_mtx`.
    pub ifiq_errors: Cell<u64>,
    /// `ifiq_mcasts`. Protected by: `ifiq_mtx`.
    pub ifiq_mcasts: Cell<u64>,
    /// `ifiq_noproto`. Protected by: `ifiq_mtx`.
    pub ifiq_noproto: Cell<u64>,

    /// `ifiq_enqueues`: number of times a list of packets were put on `ifiq_ml`.
    pub ifiq_enqueues: Cell<u64>,
    /// `ifiq_dequeues`: number of times a list of packets were pulled off `ifiq_ml`.
    pub ifiq_dequeues: Cell<u64>,

    /// `ifiq_kstat` (`struct kstat *`, not configured).
    pub ifiq_kstat: Cell<*mut c_void>,

    // properties
    /// `ifiq_idx`.
    pub ifiq_idx: Cell<u32>,
}

// SAFETY: as for `Ifqueue`, with `ifiq_mtx`.
unsafe impl Sync for Ifiqueue {}

/// `unsigned int (*ifqop_idx)(unsigned int, const struct mbuf *)`.
pub type IfqopIdxFn = fn(u32, &Mbuf) -> u32;
/// `struct mbuf *(*ifqop_enq)(struct ifqueue *, struct mbuf *)`: keeps `m` and returns NULL,
/// or returns the packet to drop (`m` itself, or one it evicts).
///
/// # Safety
///
/// `ifq_mtx` is held and `ifq_q` was made by the same table's `ifqop_alloc`.
pub type IfqopEnqFn = unsafe fn(&Ifqueue, &'static Mbuf) -> Option<&'static Mbuf>;
/// `struct mbuf *(*ifqop_deq_begin)(struct ifqueue *, void **)`: the next packet and its
/// cookie for `ifqop_deq_commit`, without removing it.
///
/// # Safety
///
/// As for [`IfqopEnqFn`].
pub type IfqopDeqBeginFn = unsafe fn(&Ifqueue) -> Option<(&'static Mbuf, *mut c_void)>;
/// `void (*ifqop_deq_commit)(struct ifqueue *, struct mbuf *, void *)`: removes the packet
/// `ifqop_deq_begin` returned.
///
/// # Safety
///
/// As for [`IfqopEnqFn`]; `m` and `cookie` come from `ifqop_deq_begin` under the same hold of
/// `ifq_mtx`.
pub type IfqopDeqCommitFn = unsafe fn(&Ifqueue, &'static Mbuf, *mut c_void);
/// `void (*ifqop_purge)(struct ifqueue *, struct mbuf_list *)`: moves every packet to `ml`.
///
/// # Safety
///
/// As for [`IfqopEnqFn`].
pub type IfqopPurgeFn = unsafe fn(&Ifqueue, &MbufList);
/// `void *(*ifqop_alloc)(unsigned int, void *)`: the conditioner's state for queue `idx`.
pub type IfqopAllocFn = fn(u32, *mut c_void) -> *mut c_void;
/// `void (*ifqop_free)(unsigned int, void *)`.
///
/// # Safety
///
/// The state came from the same table's `ifqop_alloc` and is no longer used.
pub type IfqopFreeFn = unsafe fn(u32, *mut c_void);

/// `struct ifq_ops`: a traffic conditioner.
pub struct IfqOps {
    /// `ifqop_idx`: picks the send queue of a packet.
    pub ifqop_idx: IfqopIdxFn,
    /// `ifqop_enq`.
    pub ifqop_enq: IfqopEnqFn,
    /// `ifqop_deq_begin`.
    pub ifqop_deq_begin: IfqopDeqBeginFn,
    /// `ifqop_deq_commit`.
    pub ifqop_deq_commit: IfqopDeqCommitFn,
    /// `ifqop_purge`.
    pub ifqop_purge: IfqopPurgeFn,
    /// `ifqop_alloc`.
    pub ifqop_alloc: IfqopAllocFn,
    /// `ifqop_free`.
    pub ifqop_free: IfqopFreeFn,
}

/// `struct priq`: priq internal structures, one packet list per priority.
struct Priq {
    /// `pq_lists`.
    pq_lists: [MbufList; IFQ_NQUEUES as usize],
}

/// `priq_ops`.
static PRIQ_OPS: IfqOps = IfqOps {
    ifqop_idx: priq_idx,
    ifqop_enq: priq_enq,
    ifqop_deq_begin: priq_deq_begin,
    ifqop_deq_commit: priq_deq_commit,
    ifqop_purge: priq_purge,
    ifqop_alloc: priq_alloc,
    ifqop_free: priq_free,
};

/// `ifq_priq_ops`: the default conditioner.
pub static IFQ_PRIQ_OPS: &IfqOps = &PRIQ_OPS;

/// `ifiq_maxlen_drop`: `ifiq_input` drops a list when this many packets wait.
pub static IFIQ_MAXLEN_DROP: AtomicU32 = AtomicU32::new(2048 * 5);
/// `ifiq_maxlen_return`: `ifiq_input` reports pressure above this many.
pub static IFIQ_MAXLEN_RETURN: AtomicU32 = AtomicU32::new(2048 * 3);

/// The queue's interface (`ifq->ifq_if`), set by `ifq_init`.
fn ifq_ifp(ifq: &Ifqueue) -> &'static Ifnet {
    match ifq.ifq_if.get() {
        Some(ifp) => ifp,
        None => panic(format_args!("ifqueue {:p} used before ifq_init", ifq)),
    }
}

/// The queue's conditioner (`ifq->ifq_ops`), set by `ifq_init`.
fn ifq_ops(ifq: &Ifqueue) -> &'static IfqOps {
    match ifq.ifq_ops.get() {
        Some(ops) => ops,
        None => panic(format_args!("ifqueue {:p} used before ifq_init", ifq)),
    }
}

/// The input queue's interface (`ifiq->ifiq_if`), set by `ifiq_init`.
fn ifiq_ifp(ifiq: &Ifiqueue) -> &'static Ifnet {
    match ifiq.ifiq_if.get() {
        Some(ifp) => ifp,
        None => panic(format_args!("ifiqueue {:p} used before ifiq_init", ifiq)),
    }
}

/// A queue's softnet task queue, set by `ifq_init`/`ifiq_init`.
fn softnet(tq: Option<&'static Taskq>) -> &'static Taskq {
    match tq {
        Some(tq) => tq,
        None => panic(format_args!("interface queue used before softnet_init")),
    }
}

/// `ifq_run_start`.
fn ifq_run_start(ifq: &'static Ifqueue) {
    ifq_serialize(ifq, &ifq.ifq_start);
}

/// `ifq_serialize`: runs `t` in the queue's serialisation context: at once if no CPU is
/// running the queue's work, otherwise after the work in progress, on the CPU doing it.
pub fn ifq_serialize(ifq: &Ifqueue, t: &'static Task) {
    // SAFETY: a `'static` task stays valid until it has run.
    unsafe { ifq_serialize_task(ifq, t) };
}

/// The body of `ifq_serialize`.
///
/// # Safety
///
/// `t` stays valid and in place until it has run (it may be queued for the CPU that is
/// running the queue's work when this returns).
unsafe fn ifq_serialize_task(ifq: &Ifqueue, t: &Task) {
    if t.t_flags.load(Ordering::Relaxed) & TASK_ONQUEUE != 0 {
        return;
    }

    mtx_enter(&ifq.ifq_task_mtx);
    if t.t_flags.load(Ordering::Relaxed) & TASK_ONQUEUE == 0 {
        t.t_flags.fetch_or(TASK_ONQUEUE, Ordering::Relaxed);
        // SAFETY: without `TASK_ONQUEUE` (checked under the mutex) the task is on no list, and
        // the caller keeps it alive until it has run, which takes it off again.
        unsafe { ifq.ifq_task_list.insert_tail(t) };
    }

    if ifq.ifq_serializer.get().is_null() {
        ifq.ifq_serializer.set(Machine::curcpu_ptr());

        while let Some(t) = ifq.ifq_task_list.first() {
            // SAFETY: `t` is the list's first element, under `ifq_task_mtx`.
            unsafe { ifq.ifq_task_list.remove(t) };
            t.t_flags.fetch_and(!TASK_ONQUEUE, Ordering::Relaxed);
            // work = *t: copy to caller to avoid races.
            let func = t.t_func.get();
            let arg = t.t_arg.get();

            mtx_leave(&ifq.ifq_task_mtx);

            match func {
                Some(func) => func(arg),
                None => panic(format_args!("ifq_serialize: task without a function")),
            }

            mtx_enter(&ifq.ifq_task_mtx);
        }

        ifq.ifq_serializer.set(ptr::null());
    }
    mtx_leave(&ifq.ifq_task_mtx);
}

/// `ifq_start`: notifies the driver that packets wait on `ifq`. With fewer than `if_txmit`
/// packets queued the start is deferred to the softnet task (transmit mitigation).
pub fn ifq_start(ifq: &'static Ifqueue) {
    if ifq_len(ifq) >= min(ifq_ifp(ifq).if_txmit.get(), ifq.ifq_maxlen.get()) {
        let _ = task_del(softnet(ifq.ifq_softnet.get()), &ifq.ifq_bundle);
        ifq_run_start(ifq);
    } else {
        let _ = task_add(softnet(ifq.ifq_softnet.get()), &ifq.ifq_bundle);
    }
}

/// The queue a task of `ifq_init` was set with.
///
/// # Safety
///
/// `p` is the argument `ifq_init` gave to one of the queue's tasks.
unsafe fn task_ifq(p: *mut c_void) -> &'static Ifqueue {
    // SAFETY: the caller's contract; the queue outlives its tasks (`ifq_destroy` takes them
    // off the task queues before the queue goes away).
    unsafe { &*p.cast::<Ifqueue>() }
}

/// `ifq_start_task`: calls the driver's `if_qstart` if the interface is running, the queue
/// is not empty and the ring is not full.
fn ifq_start_task(p: *mut c_void) {
    // SAFETY: `ifq_init` set this task with the queue as its argument.
    let ifq = unsafe { task_ifq(p) };
    let ifp = ifq_ifp(ifq);

    if ifp.if_flags.get() & IFF_RUNNING == 0 || ifq_empty(ifq) || ifq_is_oactive(ifq) {
        return;
    }

    if let Some(qstart) = ifp.if_qstart.get() {
        qstart(ifq);
    }
}

/// `ifq_set_oactive`: the driver's ring is full.
pub fn ifq_set_oactive(ifq: &Ifqueue) {
    if ifq.ifq_oactive.load(Ordering::Relaxed) != 0 {
        return;
    }

    mtx_enter(&ifq.ifq_mtx);
    if ifq.ifq_oactive.load(Ordering::Relaxed) == 0 {
        ifq.ifq_oactive.store(1, Ordering::Relaxed);
        ifq.ifq_oactives.set(ifq.ifq_oactives.get() + 1);
    }
    mtx_leave(&ifq.ifq_mtx);
}

/// `ifq_deq_set_oactive`: `ifq_set_oactive` between `ifq_deq_begin` and its commit or
/// rollback (`ifq_mtx` held).
pub fn ifq_deq_set_oactive(ifq: &Ifqueue) {
    mutex_assert_locked(&ifq.ifq_mtx, "ifq_deq_set_oactive");

    if ifq.ifq_oactive.load(Ordering::Relaxed) == 0 {
        ifq.ifq_oactive.store(1, Ordering::Relaxed);
        ifq.ifq_oactives.set(ifq.ifq_oactives.get() + 1);
    }
}

/// `ifq_restart_task`: clears `oactive` and calls the driver's `if_qstart`.
fn ifq_restart_task(p: *mut c_void) {
    // SAFETY: `ifq_init` set this task with the queue as its argument.
    let ifq = unsafe { task_ifq(p) };
    let ifp = ifq_ifp(ifq);

    ifq_clr_oactive(ifq);
    if let Some(qstart) = ifp.if_qstart.get() {
        qstart(ifq);
    }
}

/// `ifq_bundle_task`: the deferred `ifq_start`.
fn ifq_bundle_task(p: *mut c_void) {
    // SAFETY: `ifq_init` set this task with the queue as its argument.
    let ifq = unsafe { task_ifq(p) };

    ifq_run_start(ifq);
}

/// `ifq_barrier`: waits until the work running in the queue's serialisation context (the
/// driver's start routine) has finished.
pub fn ifq_barrier(ifq: &Ifqueue) {
    let c = Cond::new();
    let t = Task::new(cond_signal_handler, ptr::from_ref(&c).cast_mut().cast());

    let _ = task_del(softnet(ifq.ifq_softnet.get()), &ifq.ifq_bundle);

    if ifq.ifq_serializer.get().is_null() {
        return;
    }

    // SAFETY: `t` and `c` live on this stack until `cond_wait` returns, which is after the
    // serialiser has run `t` (its handler signals `c`).
    unsafe { ifq_serialize_task(ifq, &t) };

    cond_wait(&c, "ifqbar");
}

/// `ifq_init`: initialises send queue `idx` of `ifp`, with the `priq` conditioner.
pub fn ifq_init(ifq: &'static Ifqueue, ifp: &'static Ifnet, idx: u32) {
    let arg = ptr::from_ref(ifq).cast_mut().cast::<c_void>();

    ifq.ifq_if.set(Some(ifp));
    ifq.ifq_softnet.set(net_tq(idx));
    ifq.ifq_softc.set(ptr::null_mut());

    mtx_init(&ifq.ifq_mtx, IPL_NET);

    // default to priq
    ifq.ifq_ops.set(Some(&PRIQ_OPS));
    ifq.ifq_q.set((PRIQ_OPS.ifqop_alloc)(idx, ptr::null_mut()));

    ml_init(&ifq.ifq_free);
    ifq.ifq_len.store(0, Ordering::Relaxed);

    ifq.ifq_packets.set(0);
    ifq.ifq_bytes.set(0);
    ifq.ifq_qdrops.set(0);
    ifq.ifq_errors.set(0);
    ifq.ifq_mcasts.set(0);

    mtx_init(&ifq.ifq_task_mtx, IPL_NET);
    ifq.ifq_task_list.init();
    ifq.ifq_serializer.set(ptr::null());
    task_set(&ifq.ifq_bundle, ifq_bundle_task, arg);

    task_set(&ifq.ifq_start, ifq_start_task, arg);
    task_set(&ifq.ifq_restart, ifq_restart_task, arg);

    if ifq.ifq_maxlen.get() == 0 {
        ifq_init_maxlen(ifq, IFQ_MAXLEN);
    }

    ifq.ifq_idx.set(idx);

    // NKSTAT > 0: kstat_create(ifp->if_xname, 0, "txq", ...): kstat(4) is not configured.
}

/// `ifq_attach`: replaces the queue's conditioner; the pending packets move to the new one
/// (and are dropped if it refuses them).
pub fn ifq_attach(ifq: &Ifqueue, newops: &'static IfqOps, opsarg: *mut c_void) {
    let ml = MbufList::new();
    let free_ml = MbufList::new();

    let newq = (newops.ifqop_alloc)(ifq.ifq_idx.get(), opsarg);

    mtx_enter(&ifq.ifq_mtx);
    let oldops = ifq_ops(ifq);
    // SAFETY: `ifq_mtx` is held and `ifq_q` belongs to the current conditioner.
    unsafe { (oldops.ifqop_purge)(ifq, &ml) };
    ifq.ifq_len.store(0, Ordering::Relaxed);

    let oldq = ifq.ifq_q.get();

    ifq.ifq_ops.set(Some(newops));
    ifq.ifq_q.set(newq);

    while let Some(m) = ml_dequeue(&ml) {
        // SAFETY: `ifq_mtx` is held and `ifq_q` was just made by `newops`.
        match unsafe { (newops.ifqop_enq)(ifq, m) } {
            Some(m) => {
                ifq.ifq_qdrops.set(ifq.ifq_qdrops.get() + 1);
                ml_enqueue(&free_ml, m);
            }
            None => {
                ifq.ifq_len.fetch_add(1, Ordering::Relaxed);
            }
        }
    }
    mtx_leave(&ifq.ifq_mtx);

    // SAFETY: `oldq` came from `oldops` and is no longer reachable from the queue.
    unsafe { (oldops.ifqop_free)(ifq.ifq_idx.get(), oldq) };

    let _ = ml_purge(&free_ml);
}

/// `ifq_destroy`: tears the queue down in `if_detach`, freeing what is still queued.
pub fn ifq_destroy(ifq: &Ifqueue) {
    let ml = MbufList::new();

    // NKSTAT > 0: kstat_destroy(ifq->ifq_kstat): not configured.

    crate::sys::systm::net_assert_unlocked("ifq_destroy");
    taskq_del_barrier(softnet(ifq.ifq_softnet.get()), &ifq.ifq_bundle);

    // don't need to lock because this is the last use of the ifq
    let ops = ifq_ops(ifq);
    // SAFETY: the last use of the queue: nobody else touches it, and `ifq_q` is `ops`'s.
    unsafe {
        (ops.ifqop_purge)(ifq, &ml);
        (ops.ifqop_free)(ifq.ifq_idx.get(), ifq.ifq_q.get());
    }

    let _ = ml_purge(&ml);
}

/// `ifq_add_data`: adds the queue's counters to `data` (`SIOCGIFDATA`).
pub fn ifq_add_data(ifq: &Ifqueue, data: &mut IfData) {
    mtx_enter(&ifq.ifq_mtx);
    data.ifi_opackets += ifq.ifq_packets.get();
    data.ifi_obytes += ifq.ifq_bytes.get();
    data.ifi_oqdrops += ifq.ifq_qdrops.get();
    data.ifi_omcasts += ifq.ifq_mcasts.get();
    // ifp->if_data.ifi_oerrors
    mtx_leave(&ifq.ifq_mtx);
}

/// `ifq_enqueue`: tries to fit `m` on the queue. The conditioner may drop another packet to
/// make room; `ENOBUFS` when `m` itself was dropped (and freed).
pub fn ifq_enqueue(ifq: &Ifqueue, m: &'static Mbuf) -> Result<(), Errno> {
    mtx_enter(&ifq.ifq_mtx);
    // SAFETY: `ifq_mtx` is held and `ifq_q` belongs to the current conditioner.
    let dm = unsafe { (ifq_ops(ifq).ifqop_enq)(ifq, m) };
    let dropped_m = dm.is_some_and(|dm| ptr::eq(dm, m));
    if !dropped_m {
        ifq.ifq_packets.set(ifq.ifq_packets.get() + 1);
        ifq.ifq_bytes
            .set(ifq.ifq_bytes.get() + m.m_pkthdr().len.get() as u64);
        if m.m_flags().get() & M_MCAST != 0 {
            ifq.ifq_mcasts.set(ifq.ifq_mcasts.get() + 1);
        }
    }

    if dm.is_none() {
        ifq.ifq_len.fetch_add(1, Ordering::Relaxed);
    } else {
        ifq.ifq_qdrops.set(ifq.ifq_qdrops.get() + 1);
    }
    mtx_leave(&ifq.ifq_mtx);

    if let Some(dm) = dm {
        m_freem(dm);
    }

    if dropped_m {
        Err(Errno::ENOBUFS)
    } else {
        Ok(())
    }
}

/// `ifq_deq_enter`.
fn ifq_deq_enter(ifq: &Ifqueue) {
    mtx_enter(&ifq.ifq_mtx);
}

/// `ifq_deq_leave`: releases `ifq_mtx` and frees what the conditioner dropped meanwhile.
fn ifq_deq_leave(ifq: &Ifqueue) {
    let ml = MbufList::new();
    ml_enlist(&ml, &ifq.ifq_free);

    mtx_leave(&ifq.ifq_mtx);

    if !ml_empty(&ml) {
        let _ = ml_purge(&ml);
    }
}

/// `ifq_deq_begin`: the next packet, left on the queue with `ifq_mtx` held until
/// `ifq_deq_commit` or `ifq_deq_rollback`. `None` (and the mutex released) when empty.
pub fn ifq_deq_begin(ifq: &Ifqueue) -> Option<&'static Mbuf> {
    ifq_deq_enter(ifq);
    let next = if ifq.ifq_len.load(Ordering::Relaxed) == 0 {
        None
    } else {
        // SAFETY: `ifq_mtx` is held and `ifq_q` belongs to the current conditioner.
        unsafe { (ifq_ops(ifq).ifqop_deq_begin)(ifq) }
    };
    let Some((m, cookie)) = next else {
        ifq_deq_leave(ifq);
        return None;
    };

    m.m_pkthdr().ph_cookie.set(cookie.cast());

    Some(m)
}

/// `ifq_deq_commit`: takes the packet `ifq_deq_begin` returned off the queue and releases
/// `ifq_mtx`.
pub fn ifq_deq_commit(ifq: &Ifqueue, m: &'static Mbuf) {
    let cookie = m.m_pkthdr().ph_cookie.get().cast::<c_void>();

    // SAFETY: `ifq_deq_begin` returned `m` and `cookie` and left `ifq_mtx` held.
    unsafe { (ifq_ops(ifq).ifqop_deq_commit)(ifq, m, cookie) };
    ifq.ifq_len.fetch_sub(1, Ordering::Relaxed);
    ifq_deq_leave(ifq);
}

/// `ifq_deq_rollback`: leaves the packet `ifq_deq_begin` returned on the queue and releases
/// `ifq_mtx`.
pub fn ifq_deq_rollback(ifq: &Ifqueue, _m: &Mbuf) {
    ifq_deq_leave(ifq);
}

/// `ifq_dequeue`: the next packet, taken off the queue.
pub fn ifq_dequeue(ifq: &Ifqueue) -> Option<&'static Mbuf> {
    let m = ifq_deq_begin(ifq)?;

    ifq_deq_commit(ifq, m);

    Some(m)
}

/// `ifq_deq_sleep`: dequeues a packet, sleeping on the queue until one arrives (`tun(4)`).
/// `EWOULDBLOCK` when empty and `nbio`; `EIO` when `alive` drops to zero while sleeping.
pub fn ifq_deq_sleep(
    ifq: &Ifqueue,
    nbio: bool,
    priority: i32,
    wmesg: &'static str,
    sleeping: &AtomicU32,
    alive: &AtomicU32,
) -> Result<&'static Mbuf, Errno> {
    let result;

    ifq_deq_enter(ifq);
    if ifq.ifq_len.load(Ordering::Relaxed) == 0 && nbio {
        result = Err(Errno::EWOULDBLOCK);
    } else {
        loop {
            let ops = ifq_ops(ifq);
            // SAFETY: `ifq_mtx` is held (`msleep_nsec` returns with it held again) and
            // `ifq_q` belongs to the current conditioner.
            if let Some((m, cookie)) = unsafe { (ops.ifqop_deq_begin)(ifq) } {
                // SAFETY: as above, with the packet and cookie just returned.
                unsafe { (ops.ifqop_deq_commit)(ifq, m, cookie) };
                ifq.ifq_len.fetch_sub(1, Ordering::Relaxed);
                result = Ok(m);
                break;
            }

            sleeping.fetch_add(1, Ordering::Relaxed);
            let error = msleep_nsec(ptr::from_ref(ifq), &ifq.ifq_mtx, priority, wmesg, INFSLP);
            sleeping.fetch_sub(1, Ordering::Relaxed);
            if let Err(e) = error {
                result = Err(e);
                break;
            }
            if alive.load(Ordering::Relaxed) == 0 {
                result = Err(Errno::EIO);
                break;
            }
        }
    }
    ifq_deq_leave(ifq);

    result
}

/// `ifq_hdatalen`: the length of the packet at the head of the queue, 0 when empty.
pub fn ifq_hdatalen(ifq: &Ifqueue) -> i32 {
    let mut len = 0;

    if ifq_empty(ifq) {
        return 0;
    }

    if let Some(m) = ifq_deq_begin(ifq) {
        len = m.m_pkthdr().len.get();
        ifq_deq_rollback(ifq, m);
    }

    len
}

/// `ifq_init_maxlen`: sets the queue's length limit. This is not MP safe, use only during
/// attach.
pub fn ifq_init_maxlen(ifq: &Ifqueue, maxlen: u32) {
    ifq.ifq_maxlen.set(maxlen);
}

/// `ifq_purge`: drops every queued packet (counted as `qdrops`); returns how many.
pub fn ifq_purge(ifq: &Ifqueue) -> u32 {
    let ml = MbufList::new();

    mtx_enter(&ifq.ifq_mtx);
    // SAFETY: `ifq_mtx` is held and `ifq_q` belongs to the current conditioner.
    unsafe { (ifq_ops(ifq).ifqop_purge)(ifq, &ml) };
    let rv = ifq.ifq_len.swap(0, Ordering::Relaxed);
    ifq.ifq_qdrops.set(ifq.ifq_qdrops.get() + u64::from(rv));
    mtx_leave(&ifq.ifq_mtx);

    kassert!(rv == ml_len(&ml));

    let _ = ml_purge(&ml);

    rv
}

/// `ifq_q_enter`: takes `ifq_mtx` and returns the conditioner's state if `ops` is the queue's
/// conditioner; `None` (and the mutex released) otherwise.
pub fn ifq_q_enter(ifq: &Ifqueue, ops: &IfqOps) -> Option<NonNull<c_void>> {
    mtx_enter(&ifq.ifq_mtx);
    if ifq.ifq_ops.get().is_some_and(|cur| ptr::eq(cur, ops)) {
        return NonNull::new(ifq.ifq_q.get());
    }

    mtx_leave(&ifq.ifq_mtx);

    None
}

/// `ifq_q_leave`: releases the mutex `ifq_q_enter` took.
pub fn ifq_q_leave(ifq: &Ifqueue, q: NonNull<c_void>) {
    kassert!(ptr::eq(q.as_ptr(), ifq.ifq_q.get()));
    mtx_leave(&ifq.ifq_mtx);
}

/// `ifq_mfreem`: a conditioner drops `m` while `ifq_mtx` is held; it is freed once the mutex
/// is released.
pub fn ifq_mfreem(ifq: &Ifqueue, m: &'static Mbuf) {
    mutex_assert_locked(&ifq.ifq_mtx, "ifq_mfreem");

    ifq.ifq_len.fetch_sub(1, Ordering::Relaxed);
    ifq.ifq_qdrops.set(ifq.ifq_qdrops.get() + 1);
    ml_enqueue(&ifq.ifq_free, m);
}

/// `ifq_mfreeml`: as `ifq_mfreem`, for a list.
pub fn ifq_mfreeml(ifq: &Ifqueue, ml: &MbufList) {
    mutex_assert_locked(&ifq.ifq_mtx, "ifq_mfreeml");

    ifq.ifq_len.fetch_sub(ml_len(ml), Ordering::Relaxed);
    ifq.ifq_qdrops
        .set(ifq.ifq_qdrops.get() + u64::from(ml_len(ml)));
    ml_enlist(&ifq.ifq_free, ml);
}

/// `ifq_len(ifq)`: packets queued, read without the mutex.
pub fn ifq_len(ifq: &Ifqueue) -> u32 {
    ifq.ifq_len.load(Ordering::Relaxed)
}

/// `ifq_empty(ifq)`.
pub fn ifq_empty(ifq: &Ifqueue) -> bool {
    ifq_len(ifq) == 0
}

/// `ifq_is_priq`: whether the queue uses the default conditioner.
pub fn ifq_is_priq(ifq: &Ifqueue) -> bool {
    ifq.ifq_ops
        .get()
        .is_some_and(|ops| ptr::eq(ops, IFQ_PRIQ_OPS))
}

/// `ifq_clr_oactive`: the driver's ring has room again.
pub fn ifq_clr_oactive(ifq: &Ifqueue) {
    ifq.ifq_oactive.store(0, Ordering::Relaxed);
}

/// `ifq_is_oactive`.
pub fn ifq_is_oactive(ifq: &Ifqueue) -> bool {
    ifq.ifq_oactive.load(Ordering::Relaxed) != 0
}

/// `ifq_restart`: clears `oactive` and runs the driver's start routine in the serialisation
/// context (from the transmit completion path).
pub fn ifq_restart(ifq: &'static Ifqueue) {
    ifq_serialize(ifq, &ifq.ifq_restart);
}

/// `ifq_idx`: which of `nifqs` send queues gets `m`, as the queue's conditioner decides.
pub fn ifq_idx(ifq: &Ifqueue, nifqs: u32, m: &Mbuf) -> u32 {
    (ifq_ops(ifq).ifqop_idx)(nifqs, m)
}

/// `ifiq_init`: initialises input queue `idx` of `ifp`.
pub fn ifiq_init(ifiq: &'static Ifiqueue, ifp: &'static Ifnet, idx: u32) {
    ifiq.ifiq_if.set(Some(ifp));
    ifiq.ifiq_bpfp.set(ptr::null_mut());
    ifiq.ifiq_softnet.set(net_tq(idx));
    ifiq.ifiq_softc.set(ptr::null_mut());

    mtx_init(&ifiq.ifiq_mtx, IPL_NET);
    ml_init(&ifiq.ifiq_ml);
    task_set(
        &ifiq.ifiq_task,
        ifiq_process,
        ptr::from_ref(ifiq).cast_mut().cast(),
    );
    ifiq.ifiq_pressure.set(0);

    ifiq.ifiq_packets.set(0);
    ifiq.ifiq_bytes.set(0);
    ifiq.ifiq_fdrops.set(0);
    ifiq.ifiq_qdrops.set(0);
    ifiq.ifiq_errors.set(0);

    ifiq.ifiq_idx.set(idx);

    // NKSTAT > 0: kstat_create(ifp->if_xname, 0, "rxq", ...): kstat(4) is not configured.
}

/// `ifiq_destroy`: tears the queue down in `if_detach`.
pub fn ifiq_destroy(ifiq: &Ifiqueue) {
    // NKSTAT > 0: kstat_destroy(ifiq->ifiq_kstat): not configured.

    crate::sys::systm::net_assert_unlocked("ifiq_destroy");
    taskq_del_barrier(softnet(ifiq.ifiq_softnet.get()), &ifiq.ifiq_task);

    // don't need to lock because this is the last use of the ifiq
    let _ = ml_purge(&ifiq.ifiq_ml);
}

/// `ifiq_input`: a driver's received packets (`if_input`): stamps them with the interface,
/// counts them and queues them for the softnet task. Returns `true` when the queue is under
/// pressure (the driver may slow its ring refill, `if_rxr_livelocked`).
pub fn ifiq_input(ifiq: &'static Ifiqueue, ml: &MbufList) -> bool {
    let ifp = ifiq_ifp(ifiq);
    let mut bytes: u64 = 0;
    let mut fdrops: u64 = 0;

    if ml_empty(ml) {
        return false;
    }

    for m in ml.iter() {
        m.m_pkthdr().ph_ifidx.set(ifp.if_index.get());
        m.m_pkthdr().ph_rtableid.set(ifp.if_rdomain.get());
        bytes += m.m_pkthdr().len.get() as u64;
    }
    let packets = u64::from(ml_len(ml));

    let ifiq_bpfp = ifiq.ifiq_bpfp.get();
    let ifiq_bpf = if ifiq_bpfp.is_null() {
        ptr::null_mut()
    } else {
        // SAFETY: a queue's `ifiq_bpfp` is NULL or points at its driver's per-queue tap
        // cookie, which lives as long as the queue.
        unsafe { ifiq_bpfp.read() }
    };
    let if_bpf = ifp.if_bpf.get();
    if !ifiq_bpf.is_null() || !if_bpf.is_null() {
        let ml0 = MbufList::new();
        ml_enlist(&ml0, ml);

        let mtap = |arg: *mut u8, m: &Mbuf| {
            !arg.is_null()
                && ifp
                    .if_bpf_mtap
                    .get()
                    .is_some_and(|f| f(arg, m, BPF_DIRECTION_IN))
        };
        while let Some(m) = ml_dequeue(&ml0) {
            let mut drop = false;
            if mtap(ifiq_bpf, m) {
                drop = true;
            }
            if mtap(if_bpf, m) {
                drop = true;
            }
            if drop {
                m_freem(m);
                fdrops += 1;
            } else {
                ml_enqueue(ml, m);
            }
        }

        if ml_empty(ml) {
            mtx_enter(&ifiq.ifiq_mtx);
            ifiq.ifiq_packets.set(ifiq.ifiq_packets.get() + packets);
            ifiq.ifiq_bytes.set(ifiq.ifiq_bytes.get() + bytes);
            ifiq.ifiq_fdrops.set(ifiq.ifiq_fdrops.get() + fdrops);
            mtx_leave(&ifiq.ifiq_mtx);

            return false;
        }
    }

    mtx_enter(&ifiq.ifiq_mtx);
    ifiq.ifiq_packets.set(ifiq.ifiq_packets.get() + packets);
    ifiq.ifiq_bytes.set(ifiq.ifiq_bytes.get() + bytes);
    ifiq.ifiq_fdrops.set(ifiq.ifiq_fdrops.get() + fdrops);

    let len = ml_len(&ifiq.ifiq_ml);
    if ifp.if_xflags.get() & IFXF_MONITOR == 0 {
        if len > IFIQ_MAXLEN_DROP.load(Ordering::Relaxed) {
            ifiq.ifiq_qdrops
                .set(ifiq.ifiq_qdrops.get() + u64::from(ml_len(ml)));
        } else {
            ifiq.ifiq_enqueues.set(ifiq.ifiq_enqueues.get() + 1);
            ml_enlist(&ifiq.ifiq_ml, ml);
        }
    }
    mtx_leave(&ifiq.ifiq_mtx);

    if ml_empty(ml) {
        let _ = task_add(softnet(ifiq.ifiq_softnet.get()), &ifiq.ifiq_task);
    } else {
        let _ = ml_purge(ml);
    }

    len > IFIQ_MAXLEN_RETURN.load(Ordering::Relaxed)
}

/// `ifiq_add_data`: adds the queue's counters to `data` (`SIOCGIFDATA`).
pub fn ifiq_add_data(ifiq: &Ifiqueue, data: &mut IfData) {
    mtx_enter(&ifiq.ifiq_mtx);
    data.ifi_ipackets += ifiq.ifiq_packets.get();
    data.ifi_ibytes += ifiq.ifiq_bytes.get();
    data.ifi_iqdrops += ifiq.ifiq_qdrops.get();
    mtx_leave(&ifiq.ifiq_mtx);
}

/// `ifiq_enqueue_qlim`: queues one packet for the softnet task; with `qlim` nonzero the
/// packet is dropped when that many already wait.
pub fn ifiq_enqueue_qlim(
    ifiq: &'static Ifiqueue,
    m: &'static Mbuf,
    qlim: u32,
) -> Result<(), Errno> {
    let ifp = ifiq_ifp(ifiq);

    m.m_pkthdr().ph_ifidx.set(ifp.if_index.get());
    m.m_pkthdr().ph_rtableid.set(ifp.if_rdomain.get());

    let if_bpf = ifp.if_bpf.get();
    if !if_bpf.is_null()
        && let Some(mtap) = ifp.if_bpf_mtap.get()
        && mtap(if_bpf, m, BPF_DIRECTION_IN)
    {
        mtx_enter(&ifiq.ifiq_mtx);
        ifiq.ifiq_packets.set(ifiq.ifiq_packets.get() + 1);
        ifiq.ifiq_bytes
            .set(ifiq.ifiq_bytes.get() + m.m_pkthdr().len.get() as u64);
        ifiq.ifiq_fdrops.set(ifiq.ifiq_fdrops.get() + 1);
        mtx_leave(&ifiq.ifiq_mtx);

        m_freem(m);
        return Ok(());
    }

    mtx_enter(&ifiq.ifiq_mtx);
    ifiq.ifiq_packets.set(ifiq.ifiq_packets.get() + 1);
    ifiq.ifiq_bytes
        .set(ifiq.ifiq_bytes.get() + m.m_pkthdr().len.get() as u64);

    let dropped = if qlim != 0 && ml_len(&ifiq.ifiq_ml) >= qlim {
        ifiq.ifiq_qdrops.set(ifiq.ifiq_qdrops.get() + 1);
        true
    } else {
        ifiq.ifiq_enqueues.set(ifiq.ifiq_enqueues.get() + 1);
        ml_enqueue(&ifiq.ifiq_ml, m);
        false
    };

    mtx_leave(&ifiq.ifiq_mtx);

    if dropped {
        m_freem(m);
        return Ok(());
    }

    let _ = task_add(softnet(ifiq.ifiq_softnet.get()), &ifiq.ifiq_task);

    Ok(())
}

/// `ifiq_enqueue`: `ifiq_enqueue_qlim` without a limit.
pub fn ifiq_enqueue(ifiq: &'static Ifiqueue, m: &'static Mbuf) -> Result<(), Errno> {
    ifiq_enqueue_qlim(ifiq, m, 0)
}

/// `ifiq_len(ifiq)`: packets waiting, read without the mutex.
pub fn ifiq_len(ifiq: &Ifiqueue) -> u32 {
    ml_len(&ifiq.ifiq_ml)
}

/// `ifiq_empty(ifiq)`.
pub fn ifiq_empty(ifiq: &Ifiqueue) -> bool {
    ifiq_len(ifiq) == 0
}

/// `ifiq_process`: the softnet task: takes the waiting packets and hands them to
/// `if_input_process`.
fn ifiq_process(arg: *mut c_void) {
    // SAFETY: `ifiq_init` set this task with the queue as its argument; the queue outlives
    // its task (`ifiq_destroy` takes it off the task queue first).
    let ifiq: &'static Ifiqueue = unsafe { &*arg.cast::<Ifiqueue>() };
    let ml = MbufList::new();

    if ifiq_empty(ifiq) {
        return;
    }

    mtx_enter(&ifiq.ifiq_mtx);
    ifiq.ifiq_dequeues.set(ifiq.ifiq_dequeues.get() + 1);
    ml_enlist(&ml, &ifiq.ifiq_ml);
    mtx_leave(&ifiq.ifiq_mtx);

    if_input_process(ifiq_ifp(ifiq), &ml, ifiq.ifiq_idx.get());
}

/// `net_ifiq_sysctl`: `net.link.ifrxq`; the pressure knobs are disabled (`#if 0` in C since
/// 6.6), so every request is `EOPNOTSUPP`.
pub fn net_ifiq_sysctl(
    _name: &[i32],
    _oldp: usize,
    _oldlenp: Option<&mut usize>,
    _newp: usize,
    _newlen: usize,
) -> Result<(), Errno> {
    // pressure is disabled for 6.6-release
    Err(Errno::EOPNOTSUPP)
}

/// `priq_idx`: the flow id picks the queue.
fn priq_idx(nqueues: u32, m: &Mbuf) -> u32 {
    let mut flow = 0;

    if m.m_pkthdr().csum_flags.get() & M_FLOWID != 0 {
        flow = u32::from(m.m_pkthdr().ph_flowid.get());
    }

    flow % nqueues
}

/// `priq_alloc`.
fn priq_alloc(_idx: u32, _null: *mut c_void) -> *mut c_void {
    let Some(pq) = malloc(size_of::<Priq>(), M_DEVBUF, M_WAITOK) else {
        panic(format_args!("priq_alloc: out of memory"));
    };
    let pq = pq.cast::<Priq>();
    // SAFETY: a fresh allocation of `size_of::<Priq>()` bytes, aligned (malloc's chunks are
    // aligned to their size), written once before use: ml_init on every list.
    unsafe {
        pq.as_ptr().write(Priq {
            pq_lists: [const { MbufList::new() }; IFQ_NQUEUES as usize],
        })
    };
    pq.as_ptr().cast()
}

/// `priq_free`.
///
/// # Safety
///
/// `pq` came from `priq_alloc` and is no longer used.
unsafe fn priq_free(_idx: u32, pq: *mut c_void) {
    if let Some(pq) = NonNull::new(pq.cast::<u8>()) {
        free(pq, M_DEVBUF, size_of::<Priq>());
    }
}

/// The queue's `struct priq`.
///
/// # Safety
///
/// `ifq_q` was made by `priq_alloc` (the queue's conditioner is priq) and `ifq_mtx` is held,
/// or the queue is not shared yet.
unsafe fn priq_of(ifq: &Ifqueue) -> &Priq {
    // SAFETY: the caller's contract.
    unsafe { &*ifq.ifq_q.get().cast::<Priq>() }
}

/// `priq_enq`: queues `m` on the list of its priority; when the queue is full, drops a packet
/// of a lower priority instead, or refuses `m` if there is none.
///
/// # Safety
///
/// As for [`IfqopEnqFn`].
unsafe fn priq_enq(ifq: &Ifqueue, m: &'static Mbuf) -> Option<&'static Mbuf> {
    // SAFETY: the caller's contract.
    let pq = unsafe { priq_of(ifq) };
    let mprio = m.m_pkthdr().pf.prio.get();
    kassert!(u32::from(mprio) <= IFQ_MAXPRIO);

    let mut n = None;
    // Find a lower priority queue to drop from
    if ifq_len(ifq) >= ifq.ifq_maxlen.get() {
        let lower = pq.pq_lists[..usize::from(mprio)]
            .iter()
            .find(|pl| ml_len(pl) > 0);
        match lower {
            Some(pl) => n = ml_dequeue(pl),
            // There's no lower priority queue that we can drop from so don't enqueue this
            // one.
            None => return Some(m),
        }
    }

    ml_enqueue(&pq.pq_lists[usize::from(mprio)], m);

    n
}

/// `priq_deq_begin`: the first packet of the highest priority, with its list as the cookie.
///
/// # Safety
///
/// As for [`IfqopDeqBeginFn`].
unsafe fn priq_deq_begin(ifq: &Ifqueue) -> Option<(&'static Mbuf, *mut c_void)> {
    // SAFETY: the caller's contract.
    let pq = unsafe { priq_of(ifq) };

    pq.pq_lists.iter().rev().find_map(|pl| {
        mbuf_list_first(pl).map(|m| (m, ptr::from_ref(pl).cast_mut().cast::<c_void>()))
    })
}

/// `priq_deq_commit`.
///
/// # Safety
///
/// As for [`IfqopDeqCommitFn`]: `cookie` is the list `priq_deq_begin` found `m` on.
unsafe fn priq_deq_commit(_ifq: &Ifqueue, m: &'static Mbuf, cookie: *mut c_void) {
    // SAFETY: the cookie is one of the queue's lists (`priq_deq_begin`), alive under the
    // mutex the caller holds.
    let pl = unsafe { &*cookie.cast::<MbufList>() };

    kassert!(mbuf_list_first(pl).is_some_and(|first| ptr::eq(first, m)));
    let _ = m;

    let _ = ml_dequeue(pl);
}

/// `priq_purge`: moves every packet to `ml`, highest priority first.
///
/// # Safety
///
/// As for [`IfqopPurgeFn`].
unsafe fn priq_purge(ifq: &Ifqueue, ml: &MbufList) {
    // SAFETY: the caller's contract.
    let pq = unsafe { priq_of(ifq) };

    for pl in pq.pq_lists.iter().rev() {
        ml_enlist(ml, pl);
    }
}
/* </CODE> */

/* <TESTS> */
#[cfg(test)]
mod tests {
    // Host tests for the interface queues: the priq conditioner, the counters and the dequeue
    // protocol, over a zero-filled interface (no softnet task queue: nothing here starts the
    // queue).

    use std::vec::Vec;

    use super::*;
    use crate::kern::uipc_mbuf::MBPOOL;
    use crate::net::if_::tests::{setup_net, test_ifnet, test_packet};
    use crate::sys::pool::Pool;

    /// A packet of `len` bytes at priority `prio`, tagged with `id` in its flow id.
    fn packet(len: usize, prio: u8, id: u16) -> &'static Mbuf {
        let bytes: Vec<u8> = (0..len).map(|i| i as u8).collect();
        let m = test_packet(&bytes);
        m.m_pkthdr().pf.prio.set(prio);
        m.m_pkthdr().ph_flowid.set(id);
        m
    }

    /// A send queue of `maxlen` packets on a fresh interface.
    fn queue(maxlen: u32) -> &'static Ifqueue {
        let ifp = test_ifnet(b"tifq0");
        ifq_init_maxlen(&ifp.if_snd, maxlen);
        ifq_init(&ifp.if_snd, ifp, 0);
        &ifp.if_snd
    }

    fn outstanding(pp: &Pool) -> u32 {
        pp.pr_nout.get()
    }

    #[test]
    fn priq_dequeues_the_highest_priority_first_and_counts() {
        let _g = setup_net();
        let ifq = queue(IFQ_MAXLEN);
        assert!(ifq_is_priq(ifq));
        assert_eq!(ifq.ifq_maxlen.get(), IFQ_MAXLEN);

        for (prio, id) in [(1, 10), (6, 60), (3, 30), (6, 61)] {
            assert_eq!(ifq_enqueue(ifq, packet(20, prio, id)), Ok(()));
        }
        let mcast = packet(20, 0, 0);
        mcast.m_flags().set(mcast.m_flags().get() | M_MCAST);
        assert_eq!(ifq_enqueue(ifq, mcast), Ok(()));

        assert_eq!(ifq_len(ifq), 5);
        assert_eq!(ifq.ifq_packets.get(), 5);
        assert_eq!(ifq.ifq_bytes.get(), 100);
        assert_eq!(ifq.ifq_mcasts.get(), 1);
        assert_eq!(ifq_hdatalen(ifq), 20);

        let order: Vec<u16> = core::iter::from_fn(|| {
            let m = ifq_dequeue(ifq)?;
            let id = m.m_pkthdr().ph_flowid.get();
            m_freem(m);
            Some(id)
        })
        .collect();
        // Highest priority first, FIFO within a priority.
        assert_eq!(order, [60, 61, 30, 10, 0]);
        assert!(ifq_empty(ifq));
        assert_eq!(ifq_hdatalen(ifq), 0);
    }

    #[test]
    fn a_full_priq_drops_lower_priorities_or_refuses() {
        let _g = setup_net();
        let ifq = queue(2);
        let before = outstanding(&MBPOOL);

        assert_eq!(ifq_enqueue(ifq, packet(8, 0, 1)), Ok(()));
        assert_eq!(ifq_enqueue(ifq, packet(8, 0, 2)), Ok(()));
        // Full: a priority 3 packet evicts the oldest priority 0 one.
        assert_eq!(ifq_enqueue(ifq, packet(8, 3, 3)), Ok(()));
        assert_eq!(ifq_len(ifq), 2);
        assert_eq!(ifq.ifq_qdrops.get(), 1);
        // Full and nothing lower than priority 0: the new packet is refused (and freed).
        assert_eq!(ifq_enqueue(ifq, packet(8, 0, 4)), Err(Errno::ENOBUFS));
        assert_eq!(ifq.ifq_qdrops.get(), 2);
        assert_eq!(
            ifq.ifq_packets.get(),
            3,
            "a refused packet is not counted as sent"
        );

        let first = ifq_dequeue(ifq).expect("priority 3");
        assert_eq!(first.m_pkthdr().ph_flowid.get(), 3);
        m_freem(first);
        let second = ifq_dequeue(ifq).expect("the survivor");
        assert_eq!(second.m_pkthdr().ph_flowid.get(), 2);
        m_freem(second);
        assert_eq!(
            outstanding(&MBPOOL),
            before,
            "every dropped packet was freed"
        );
    }

    #[test]
    fn deq_begin_and_rollback_leave_the_packet_queued() {
        let _g = setup_net();
        let ifq = queue(IFQ_MAXLEN);
        assert_eq!(ifq_enqueue(ifq, packet(12, 2, 7)), Ok(()));

        let m = ifq_deq_begin(ifq).expect("head");
        assert_eq!(m.m_pkthdr().ph_flowid.get(), 7);
        ifq_deq_set_oactive(ifq);
        ifq_deq_rollback(ifq, m);
        assert_eq!(ifq_len(ifq), 1);
        assert!(ifq_is_oactive(ifq));
        assert_eq!(ifq.ifq_oactives.get(), 1);
        ifq_clr_oactive(ifq);
        ifq_set_oactive(ifq);
        ifq_set_oactive(ifq);
        assert_eq!(ifq.ifq_oactives.get(), 2, "counted once per transition");

        let m = ifq_deq_begin(ifq).expect("still there");
        ifq_deq_commit(ifq, m);
        assert!(ifq_empty(ifq));
        m_freem(m);
    }

    #[test]
    fn purge_and_attach_keep_the_books() {
        let _g = setup_net();
        let ifq = queue(IFQ_MAXLEN);
        let before = outstanding(&MBPOOL);
        for id in 0..4 {
            assert_eq!(ifq_enqueue(ifq, packet(4, (id % 8) as u8, id)), Ok(()));
        }

        // Reattaching the conditioner moves the packets to the new state.
        ifq_attach(ifq, IFQ_PRIQ_OPS, ptr::null_mut());
        assert_eq!(ifq_len(ifq), 4);
        assert_eq!(ifq.ifq_qdrops.get(), 0);

        // ifq_q_enter takes the mutex only for the queue's own conditioner.
        let q = ifq_q_enter(ifq, IFQ_PRIQ_OPS).expect("priq");
        ifq_q_leave(ifq, q);

        let mut data = IfData::default();
        ifq_add_data(ifq, &mut data);
        assert_eq!(data.ifi_opackets, 4);
        assert_eq!(data.ifi_obytes, 16);

        assert_eq!(ifq_purge(ifq), 4);
        assert!(ifq_empty(ifq));
        assert_eq!(ifq.ifq_qdrops.get(), 4);
        assert_eq!(outstanding(&MBPOOL), before);
    }

    #[test]
    fn priq_idx_spreads_by_flow_id() {
        let _g = setup_net();
        let ifq = queue(IFQ_MAXLEN);
        let m = packet(4, 0, 13);
        assert_eq!(ifq_idx(ifq, 4, m), 0, "no M_FLOWID: queue 0");
        m.m_pkthdr()
            .csum_flags
            .set(m.m_pkthdr().csum_flags.get() | M_FLOWID);
        assert_eq!(ifq_idx(ifq, 4, m), 13 % 4);
        m_freem(m);
    }

    #[test]
    #[ignore = "needs OPENBSD_SRC (just test-ref)"]
    fn values_match_the_c_header() {
        let defs = crate::reftest::defines("sys/net/ifq.h");
        crate::reftest::assert_defines!(defs; IFQ_MAXLEN);
    }
}
/* </TESTS> */
