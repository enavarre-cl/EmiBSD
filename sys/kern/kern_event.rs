/*	$OpenBSD: kern_event.c,v 1.206 2026/06/01 18:24:58 mvs Exp $	*/
/* <LICENSES> */
/*-
 * Copyright (c) 1999,2000,2001 Jonathan Lemon <jlemon@FreeBSD.org>
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
 * $FreeBSD: src/sys/kern/kern_event.c,v 1.22 2001/02/23 20:32:42 jlemon Exp $
 */
/* </LICENSES> */

//! `kern_event.c`: `kqueue(2)` and `kevent(2)`, the knotes and klists every event source
//! hangs its waiters on, the generic filters (`EVFILT_PROC`, `EVFILT_SIGNAL`,
//! `EVFILT_TIMER`, `EVFILT_USER`, the descriptor filters' dispatch, `seltrue`, the dead
//! and bad-descriptor filters), and the per-thread queue `select(2)` and `poll(2)` are
//! built on (`kqpoll_*`).
//!
//! Upstream: sys/kern/kern_event.c @ 3ce1f3f79392
//!
//! A kqueue is a pool item shared by its descriptor (`kqueueops`) or its thread (`p_kq`)
//! and the scans running on it (`KQREF`/`KQRELE`). `kqueue_register` turns a `struct kevent`
//! change into a knote: it finds or makes the knote in the kqueue's descriptor table or
//! hash, attaches it to its object through the filter (`f_attach`; for a descriptor filter
//! the file's `fo_kqfilter`, which hooks it on the object's klist), and queues it when its
//! event is already pending. Objects call `knote`/`knote_locked` on their klist; an active
//! knote is queued on `kq_head`, and `kqueue_scan` hands the queued events out, the scan's
//! markers bounding what one call collects. A knote being worked on is `KN_PROCESSING`
//! (`knote_acquire`/`knote_release`), which serializes its filter calls without holding
//! `kq_lock` across them.
//!
//! ## Deviations
//! - Pool items: knotes and kqueues are handed around as `&Knote`/`&'static Kqueue` (the
//!   `crget`/`crfree` idiom); the functions that return them to their pool (`knote_drop`,
//!   `KQRELE`) are `unsafe` and state that the caller does not use them afterwards.
//! - `PR_WAITOK`/`M_WAITOK` cannot sleep in this kernel yet (`subr_pool.rs`,
//!   `kern_malloc.rs`), so the allocations the C never sees fail are checked:
//!   `kqueue_alloc` returns `Result` (`ENOMEM`; `kqpoll_init` and `dokqueue` pass it up),
//!   `kqueue_register` fails with `ENOMEM` when it cannot get a knote or grow the
//!   descriptor table or the hash, and `filt_timerattach` when it cannot get its timer.
//! - `kqpoll_init`, `kqpoll_done` and `kqpoll_exit` take the thread (the C reads
//!   `curproc`); `kqueue_register`, `kqueue_purge`, `knote_remove` and `knote_drop` take it
//!   as `Option<&Proc>` because the C passes NULL (`filt_proc`'s `NOTE_TRACK`, a file closed
//!   without a thread). `filt_procattach` and `filt_sigattach` still read `curproc`.
//! - `knote_remove`'s `struct knlist **plist` (always `&kq->kq_knlist` or `&kq->kq_knhash`)
//!   is a `bool` saying which table; the slot is re-read after every unlock, as in C.
//! - `hashinit`/`hashfree` are typed for `LIST` heads in this port (`kern_subr.rs`); the
//!   knote hash is `SLIST` heads, so `kqueue_expand_hash` and `KQRELE` allocate and free
//!   the `hashsize(KN_HASHSIZE)` heads with `mallocarray`/`free`, with the same size and
//!   mask.
//! - `kqueue_scan`'s `struct timespec *tsp` is `Option<&mut Timespec>` and its event array
//!   a slice; `kqueue_scan_setup` is `unsafe` (the state holds the scan markers, linked into
//!   the kqueue until `kqueue_scan_finish`, so it must stay in place).
//! - `pledge_fail` (`kern_pledge.c`, not ported) is reported for a pledged process that
//!   asks for `EVFILT_PROC` without `proc`; no process can be pledged yet.
//! - `KERNEL_LOCK`/`KERNEL_ASSERT_LOCKED` are nothing without `MULTIPROCESSOR`, so the
//!   `FILTEROP_MPSAFE` and non-`MPSAFE` paths call the filter the same way (the `splhigh`
//!   around `knote_modify`/`knote_process` stays); `pool_cache_init` is nothing without
//!   `MULTIPROCESSOR`, so `kqueue_init_percpu` is empty. `KLIST_ASSERT_LOCKED` checks the
//!   list's lock under feature `diagnostic` (`option DIAGNOSTIC`). `KQUEUE_DEBUG`
//!   (`kqueue_check`) and `KTRACE` (`ktrevent`, `ktrreltimespec`) are not configured.
//!   `NET_ASSERT_UNLOCKED` waits for the network lock.
//! - `filt_timer`'s `ft_reschedule` is a `bool`.

use core::cell::Cell;
use core::ffi::c_void;
use core::ptr::{self, NonNull};
use core::slice;
use core::sync::atomic::{AtomicU32, Ordering};

use crate::kassert;
use crate::kern::kern_clock::tstohz;
use crate::kern::kern_descrip::{falloc, fd_checkclosed, fd_getfile, fdinsert};
use crate::kern::kern_lock::{mtx_enter, mtx_init, mtx_leave};
use crate::kern::kern_malloc::{free, malloc, mallocarray};
use crate::kern::kern_proc::prfind;
use crate::kern::kern_rwlock::{rw_assert_wrlock, rw_enter_write, rw_exit_write, rw_status};
use crate::kern::kern_subr::hashsize;
use crate::kern::kern_synch::{
    msleep_nsec, refcnt_init, refcnt_read, refcnt_rele, refcnt_take, tsleep_nsec, wakeup,
};
use crate::kern::kern_task::{SYSTQMP, task_add, task_set, taskq_del_barrier};
use crate::kern::kern_tc::{getnanouptime, nanotime};
use crate::kern::kern_timeout::{timeout_add, timeout_del_barrier, timeout_set};
use crate::kern::subr_pool::{pool_get, pool_init, pool_put};
use crate::kern::subr_prf::panic;
use crate::machine::copy::{copyin, copyin_obj, copyout};
use crate::machine::cpu::curproc;
use crate::machine::intr::{IPL_HIGH, IPL_MPFLOOR, IPL_SOFTCLOCK, splhigh, splx};
use crate::miscfs::deadfs::dead_vnops::DEAD_VOPS;
use crate::sys::errno::Errno;
use crate::sys::event::{
    __EV_HUP, __EV_POLL, __EV_SELECT, EV_ADD, EV_CLEAR, EV_DELETE, EV_DISABLE, EV_DISPATCH,
    EV_ENABLE, EV_EOF, EV_ERROR, EV_FLAG1, EV_ONESHOT, EV_RECEIPT, EV_SYSFLAGS, EVFILT_EXCEPT,
    EVFILT_MARKER, EVFILT_READ, EVFILT_SYSCOUNT, EVFILT_WRITE, FILTEROP_ISFD, FILTEROP_MPSAFE,
    Filterops, KN_ACTIVE, KN_DETACHED, KN_DISABLED, KN_HASHSIZE, KN_PROCESSING, KN_QUEUED,
    KN_WAITING, Kevent, Klist, Klistops, KnLink, KnTqe, Knlist, Knote, KqueueScanState,
    NOTE_ABSTIME, NOTE_CHILD, NOTE_EXIT, NOTE_FFAND, NOTE_FFCOPY, NOTE_FFCTRLMASK, NOTE_FFLAGSMASK,
    NOTE_FFNOP, NOTE_FFOR, NOTE_FORK, NOTE_MSECONDS, NOTE_NSECONDS, NOTE_PCTRLMASK, NOTE_PDATAMASK,
    NOTE_SECONDS, NOTE_SIGNAL, NOTE_TRACK, NOTE_TRACKERR, NOTE_TRIGGER, NOTE_USECONDS, klist_empty,
    knote_modify, knote_process,
};
use crate::sys::eventvar::{KQ_DYING, KQ_NEVENTS, KQ_SLEEP, KQ_TASK, KQEXTENT, KqList, Kqueue};
use crate::sys::fcntl::{FNONBLOCK, FREAD, FWRITE, O_CLOEXEC};
use crate::sys::file::{DTYPE_KQUEUE, DTYPE_VNODE, File, Fileops, frele};
use crate::sys::filedesc::{Filedesc, UF_EXCLOSE, fdpassertlocked, fdplock, fdpunlock};
use crate::sys::malloc::{M_KEVENT, M_WAITOK};
use crate::sys::mutex::{Mutex, mutex_assert_locked};
use crate::sys::param::{PCATCH, PNORELOCK, PSOCK};
use crate::sys::pledge::PLEDGE_PROC;
use crate::sys::pool::{PR_WAITOK, PR_ZERO, Pool};
use crate::sys::proc::{PID_MAX, PS_EXITING, PS_PLEDGE, Proc, Process};
use crate::sys::queue::{ListHead, SlistHead, TailqHead};
use crate::sys::rwlock::{RW_WRITE, Rwlock};
use crate::sys::signal::NSIG;
use crate::sys::stat::{S_IFIFO, Stat};
use crate::sys::syscallargs::{SysKeventArgs, SysKqueue1Args};
use crate::sys::systm::{INFSLP, MAXTSLP, SysArgs, sysargs};
use crate::sys::time::{Timespec, sec_to_nsec, timespec_to_nsec, timespecsub};
use crate::sys::timeout::{Timeout, timeout_triggered};
use crate::sys::types::{Dev, Pid, Register};
use crate::sys::uio::Uio;
use crate::sys::wait::w_exitcode;
use crate::unported;

/// `NOTE_TIMER_UNITMASK`: the unit bits of an `EVFILT_TIMER`'s `fflags`.
const NOTE_TIMER_UNITMASK: u32 = NOTE_SECONDS | NOTE_MSECONDS | NOTE_USECONDS | NOTE_NSECONDS;

/// `struct filt_timer`: an `EVFILT_TIMER` knote's timeout, behind `kn_hook`.
///
/// Locking: \[I\] immutable after creation, \[a\] atomic operations, \[t\] `ft_mtx`.
struct FiltTimer {
    /// `ft_mtx`.
    ft_mtx: Mutex,
    /// \[t\] `ft_to`.
    ft_to: Timeout,
    /// \[t\] `ft_reschedule`.
    ft_reschedule: Cell<bool>,
}

impl FiltTimer {
    /// A zeroed timer, before `filt_timerattach` sets it up.
    const fn new() -> Self {
        Self {
            ft_mtx: Mutex::new(IPL_SOFTCLOCK),
            ft_to: Timeout::zeroed(),
            ft_reschedule: Cell::new(false),
        }
    }
}

/// `kqueueops`: the file operations of a `kqueue(2)` descriptor.
pub static KQUEUEOPS: Fileops = Fileops {
    fo_read: kqueue_read,
    fo_write: kqueue_write,
    fo_ioctl: kqueue_ioctl,
    fo_kqfilter: kqueue_kqfilter,
    fo_stat: kqueue_stat,
    fo_close: kqueue_close,
    fo_seek: None,
};

/// `kqread_filtops`: `EVFILT_READ` on a kqueue.
pub static KQREAD_FILTOPS: Filterops = Filterops {
    f_flags: FILTEROP_ISFD | FILTEROP_MPSAFE,
    f_attach: None,
    f_detach: Some(filt_kqdetach),
    f_event: Some(filt_kqueue),
    f_modify: Some(filt_kqueuemodify),
    f_process: Some(filt_kqueueprocess),
};

/// `proc_filtops`: `EVFILT_PROC`.
pub static PROC_FILTOPS: Filterops = Filterops {
    f_flags: FILTEROP_MPSAFE,
    f_attach: Some(filt_procattach),
    f_detach: Some(filt_procdetach),
    f_event: Some(filt_proc),
    f_modify: Some(filt_procmodify),
    f_process: Some(filt_procprocess),
};

/// `sig_filtops`: `EVFILT_SIGNAL`.
pub static SIG_FILTOPS: Filterops = Filterops {
    f_flags: FILTEROP_MPSAFE,
    f_attach: Some(filt_sigattach),
    f_detach: Some(filt_sigdetach),
    f_event: Some(filt_signal),
    f_modify: Some(filt_procmodify),
    f_process: Some(filt_procprocess),
};

/// `file_filtops`: the descriptor filters, which the file's `fo_kqfilter` replaces.
pub static FILE_FILTOPS: Filterops = Filterops {
    f_flags: FILTEROP_ISFD | FILTEROP_MPSAFE,
    f_attach: Some(filt_fileattach),
    f_detach: None,
    f_event: None,
    f_modify: None,
    f_process: None,
};

/// `timer_filtops`: `EVFILT_TIMER`.
pub static TIMER_FILTOPS: Filterops = Filterops {
    f_flags: FILTEROP_MPSAFE,
    f_attach: Some(filt_timerattach),
    f_detach: Some(filt_timerdetach),
    f_event: None,
    f_modify: Some(filt_timermodify),
    f_process: Some(filt_timerprocess),
};

/// `user_filtops`: `EVFILT_USER`.
pub static USER_FILTOPS: Filterops = Filterops {
    f_flags: FILTEROP_MPSAFE,
    f_attach: Some(filt_userattach),
    f_detach: Some(filt_userdetach),
    f_event: None,
    f_modify: Some(filt_usermodify),
    f_process: Some(filt_userprocess),
};

/// `knote_pool`.
pub static KNOTE_POOL: Pool = Pool::new();
/// `kqueue_pool`.
pub static KQUEUE_POOL: Pool = Pool::new();
/// \[L\] `kqueue_klist_lock`: the lock of every kqueue's `kq_klist`.
pub static KQUEUE_KLIST_LOCK: Mutex = Mutex::new(IPL_MPFLOOR);
/// `kqueue_ps_list_lock`: with a process's `ps_mtx`, guards its `ps_klist` (`[Q]` in
/// `<sys/proc.h>`).
pub static KQUEUE_PS_LIST_LOCK: Rwlock = Rwlock::new("kqpsl");
/// \[a\] `kq_usereventsmax`: per process.
pub static KQ_USEREVENTSMAX: AtomicU32 = AtomicU32::new(1024);

/// Table for all system-defined filters, indexed by `~filter`.
static SYSFILT_OPS: [Option<&Filterops>; EVFILT_SYSCOUNT as usize] = [
    Some(&FILE_FILTOPS),  // EVFILT_READ
    Some(&FILE_FILTOPS),  // EVFILT_WRITE
    None,                 // EVFILT_AIO (&aio_filtops)
    Some(&FILE_FILTOPS),  // EVFILT_VNODE
    Some(&PROC_FILTOPS),  // EVFILT_PROC
    Some(&SIG_FILTOPS),   // EVFILT_SIGNAL
    Some(&TIMER_FILTOPS), // EVFILT_TIMER
    Some(&FILE_FILTOPS),  // EVFILT_DEVICE
    Some(&FILE_FILTOPS),  // EVFILT_EXCEPT
    Some(&USER_FILTOPS),  // EVFILT_USER
];

/// `seltrue_filtops`: the full kqfilter entry for device switch tables, which has the same
/// effect as a filter using `filt_seltrue()` as filter method.
pub static SELTRUE_FILTOPS: Filterops = Filterops {
    f_flags: FILTEROP_ISFD | FILTEROP_MPSAFE,
    f_attach: None,
    f_detach: Some(filt_seltruedetach),
    f_event: Some(filt_seltrue),
    f_modify: Some(filt_seltruemodify),
    f_process: Some(filt_seltrueprocess),
};

/// `dead_filtops`: the filter of a knote whose object is gone (a revoked vnode, an
/// invalidated klist).
pub static DEAD_FILTOPS: Filterops = Filterops {
    f_flags: FILTEROP_ISFD | FILTEROP_MPSAFE,
    f_attach: None,
    f_detach: Some(filt_deaddetach),
    f_event: Some(filt_dead),
    f_modify: Some(filt_seltruemodify),
    f_process: Some(filt_seltrueprocess),
};

/// `badfd_filtops`: for use with kqpoll, a closed descriptor's knote reporting `EBADF`.
pub static BADFD_FILTOPS: Filterops = Filterops {
    f_flags: FILTEROP_ISFD | FILTEROP_MPSAFE,
    f_attach: None,
    f_detach: Some(filt_deaddetach),
    f_event: Some(filt_badfd),
    f_modify: Some(filt_seltruemodify),
    f_process: Some(filt_seltrueprocess),
};

/// `mutex_klistops`.
static MUTEX_KLISTOPS: Klistops = Klistops {
    klo_assertlk: klist_mutex_assertlk,
    klo_lock: klist_mutex_lock,
    klo_unlock: klist_mutex_unlock,
};

/// `rwlock_klistops`.
static RWLOCK_KLISTOPS: Klistops = Klistops {
    klo_assertlk: klist_rwlock_assertlk,
    klo_lock: klist_rwlock_lock,
    klo_unlock: klist_rwlock_unlock,
};

/// `KLIST_ASSERT_LOCKED(kl)` (`DIAGNOSTIC`): the list's lock, or the kernel lock, is held.
fn klist_assert_locked(kl: &Klist) {
    #[cfg(feature = "diagnostic")]
    if let Some(ops) = kl.kl_ops.get() {
        // SAFETY: `kl_arg` is the lock `klist_init` was given for these ops, valid while the
        // list is (its contract).
        unsafe { (ops.klo_assertlk)(kl.kl_arg.get()) };
    }
    // else KERNEL_ASSERT_LOCKED(): nothing without MULTIPROCESSOR.
    let _ = kl;
}

/// `KN_HASH(val, mask)`.
const fn kn_hash(val: u64, mask: u64) -> u64 {
    (val ^ (val >> 8)) & mask
}

/// `kn->kn_fp->f_data` of a kqueue file: its kqueue.
fn fp_kqueue(fp: &File) -> &'static Kqueue {
    if fp.f_type.get() != DTYPE_KQUEUE {
        panic(format_args!("file {:p}: not a kqueue", fp));
    }
    // SAFETY: `dokqueue` points the `f_data` of every `DTYPE_KQUEUE` file at its kqueue,
    // which the file holds a reference to until `kqueue_close` clears `f_data`.
    match unsafe { fp.f_data.get().cast::<Kqueue>().as_ref() } {
        Some(kq) => kq,
        None => panic(format_args!("file {:p}: kqueue already closed", fp)),
    }
}

/// `kn->kn_ptr.p_process`, which an attached, not detached `EVFILT_PROC` or
/// `EVFILT_SIGNAL` knote has.
fn kn_process(kn: &Knote) -> &Process {
    // SAFETY: the attach functions point it at a live process; the knote stays on that
    // process's `ps_klist` until `filt_proc` (`NOTE_EXIT`, which clears it and sets
    // `KN_DETACHED`) or the detach functions take it off, and the process is freed only
    // after `knote_processexit`.
    match unsafe { kn.kn_ptr.p_process.get().as_ref() } {
        Some(pr) => pr,
        None => panic(format_args!("knote {:p}: no process", kn)),
    }
}

/// `kn->kn_fop->f_event(kn, hint)`, which the filters that call it directly have.
fn fop_event(kn: &Knote, hint: i64) -> bool {
    match kn.fop().f_event {
        Some(f_event) => f_event(kn, hint),
        None => panic(format_args!("knote {:p}: no f_event", kn)),
    }
}

/// `KQREF(kq)`: takes a reference to the kqueue.
#[allow(non_snake_case)] // the C name
pub fn KQREF(kq: &Kqueue) {
    refcnt_take(&kq.kq_refcnt);
}

/// `KQRELE(kq)`: drops a reference; the last one takes the kqueue off its table's list
/// and returns it to the pool.
///
/// # Safety
///
/// `kq` came from `kqueue_alloc` and the caller holds a reference; when it was the last,
/// nothing uses `kq` afterwards.
#[allow(non_snake_case)] // the C name
pub unsafe fn KQRELE(kq: &Kqueue) {
    if !refcnt_rele(&kq.kq_refcnt) {
        return;
    }

    let fdp = kq.fdp();
    if rw_status(&fdp.fd_lock) == RW_WRITE {
        // SAFETY: every kqueue is on its table's `fd_kqlist` (`dokqueue`, `kqpoll_init`),
        // whose lock is held.
        unsafe { ListHead::<KqList>::remove(kq) };
    } else {
        fdplock(fdp);
        // SAFETY: as above.
        unsafe { ListHead::<KqList>::remove(kq) };
        fdpunlock(fdp);
    }

    kassert!(kq.kq_head.is_empty());
    kassert!(kq.kq_nknotes.get() == 0);

    if let Some(list) = NonNull::new(kq.kq_knlist.get()) {
        free(
            list.cast(),
            M_KEVENT,
            kq.kq_knlistsize.get() as usize * size_of::<Knlist>(),
        );
    }
    if let Some(hash) = NonNull::new(kq.kq_knhash.get()) {
        knhash_free(hash);
    }
    klist_free(&kq.kq_klist);
    pool_put(&KQUEUE_POOL, NonNull::from(kq).cast());
}

/// `kqueue_init`: the knote and kqueue pools.
pub fn kqueue_init() {
    pool_init(
        &KQUEUE_POOL,
        size_of::<Kqueue>(),
        0,
        IPL_MPFLOOR,
        PR_WAITOK,
        "kqueuepl",
        None,
    );
    pool_init(
        &KNOTE_POOL,
        size_of::<Knote>(),
        0,
        IPL_MPFLOOR,
        PR_WAITOK,
        "knotepl",
        None,
    );
}

/// `kqueue_init_percpu`: `pool_cache_init(&knote_pool)`, nothing without `MULTIPROCESSOR`.
pub fn kqueue_init_percpu() {}

/// `filt_fileattach`: the file's `fo_kqfilter` attaches the knote and picks its filter.
pub fn filt_fileattach(kn: &Knote) -> Result<(), Errno> {
    let fp = kn.fp();

    (fp.ops().fo_kqfilter)(fp, kn)
}

/// `kqueue_kqfilter`: a kqueue is readable when it has pending events.
pub fn kqueue_kqfilter(_fp: &File, kn: &Knote) -> Result<(), Errno> {
    let kq = fp_kqueue(kn.fp());

    if kn.kn_filter().get() != EVFILT_READ {
        return Err(Errno::EINVAL);
    }

    kn.kn_fop.set(Some(&KQREAD_FILTOPS));
    klist_insert(&kq.kq_klist, kn);
    Ok(())
}

/// `filt_kqdetach`.
pub fn filt_kqdetach(kn: &Knote) {
    let kq = fp_kqueue(kn.fp());

    klist_remove(&kq.kq_klist, kn);
}

/// `filt_kqueue_common`: `kn_data` is the kqueue's pending count.
pub fn filt_kqueue_common(kn: &Knote, kq: &Kqueue) -> bool {
    mutex_assert_locked(&kq.kq_lock, "filt_kqueue_common");

    kn.kn_data().set(i64::from(kq.kq_count.get()));

    kn.kn_data().get() > 0
}

/// `filt_kqueue`.
pub fn filt_kqueue(kn: &Knote, _hint: i64) -> bool {
    let kq = fp_kqueue(kn.fp());

    mtx_enter(&kq.kq_lock);
    let active = filt_kqueue_common(kn, kq);
    mtx_leave(&kq.kq_lock);

    active
}

/// `filt_kqueuemodify`.
pub fn filt_kqueuemodify(kev: &mut Kevent, kn: &Knote) -> bool {
    let kq = fp_kqueue(kn.fp());

    mtx_enter(&kq.kq_lock);
    knote_assign(kev, kn);
    let active = filt_kqueue_common(kn, kq);
    mtx_leave(&kq.kq_lock);

    active
}

/// `filt_kqueueprocess`.
pub fn filt_kqueueprocess(kn: &Knote, kev: Option<&mut Kevent>) -> bool {
    let kq = fp_kqueue(kn.fp());

    mtx_enter(&kq.kq_lock);
    let active = if kev.is_some() && kn.has_flags(EV_ONESHOT) {
        true
    } else {
        filt_kqueue_common(kn, kq)
    };
    if active {
        knote_submit(kn, kev);
    }
    mtx_leave(&kq.kq_lock);

    active
}

/// `filt_procattach`: hooks the knote on the process `kn_id`.
pub fn filt_procattach(kn: &Knote) -> Result<(), Errno> {
    if let Some(cp) = curproc()
        && cp.process().ps_flags.load(Ordering::Relaxed) & PS_PLEDGE != 0
        && cp.p_pledge.get() & PLEDGE_PROC == 0
    {
        // pledge_fail(curproc, EPERM, PLEDGE_PROC): kern_pledge.c (see the deviations).
        return Err(unported!("filt_procattach: pledge_fail (kern_pledge.c)"));
    }

    if kn.kn_id().get() > PID_MAX as usize {
        return Err(Errno::ESRCH);
    }

    // KERNEL_LOCK(): nothing without MULTIPROCESSOR.
    let Some(pr) = prfind(kn.kn_id().get() as Pid) else {
        return Err(Errno::ESRCH);
    };

    // exiting processes can't be specified
    if pr.ps_flags.load(Ordering::Relaxed) & PS_EXITING != 0 {
        return Err(Errno::ESRCH);
    }

    kn.kn_ptr.p_process.set(pr);
    kn.set_flags(EV_CLEAR); // automatically set

    // internal flag indicating registration done by kernel
    if kn.has_flags(EV_FLAG1) {
        kn.kn_data().set(kn.kn_sdata.get()); // ppid
        kn.kn_fflags().set(NOTE_CHILD);
        kn.clear_flags(EV_FLAG1);
        rw_assert_wrlock(&KQUEUE_PS_LIST_LOCK);
    }

    // this needs both the ps_mtx and exclusive kqueue_ps_list_lock.
    let nolock = rw_status(&KQUEUE_PS_LIST_LOCK) == RW_WRITE;
    if !nolock {
        rw_enter_write(&KQUEUE_PS_LIST_LOCK);
    }
    mtx_enter(&pr.ps_mtx);
    klist_insert_locked(&pr.ps_klist, kn);
    mtx_leave(&pr.ps_mtx);
    if !nolock {
        rw_exit_write(&KQUEUE_PS_LIST_LOCK);
    }

    Ok(())
}

/// `filt_procdetach`: the knote may be attached to a different process, which may exit,
/// leaving nothing for the knote to be attached to. So when the process exits, the knote
/// is marked as `KN_DETACHED` and also flagged as `EV_ONESHOT` so it will be deleted when
/// read out. However, as part of the knote deletion, this routine is called, so a check is
/// needed to avoid actually performing a detach, because the original process does not
/// exist any more.
pub fn filt_procdetach(kn: &Knote) {
    // We set KN_DETACHED with both kqueue_ps_list_lock rwlock and &kq->kq_lock held. One of
    // them is enough here.
    rw_enter_write(&KQUEUE_PS_LIST_LOCK);
    if kn.kn_status.get() & KN_DETACHED == 0 {
        let pr = kn_process(kn);

        mtx_enter(&pr.ps_mtx);
        klist_remove_locked(&pr.ps_klist, kn);
        mtx_leave(&pr.ps_mtx);
    }
    rw_exit_write(&KQUEUE_PS_LIST_LOCK);
}

/// `filt_proc`: records the process event the user asked for; `NOTE_EXIT` detaches the
/// knote, `NOTE_FORK` with `NOTE_TRACK` registers a knote on the child.
pub fn filt_proc(kn: &Knote, hint: i64) -> bool {
    let kq = kn.kq();

    // mask off extra data
    let event = (hint as u32) & NOTE_PCTRLMASK;

    // if the user is interested in this event, record it.
    if kn.kn_sfflags.get() & event != 0 {
        kn.kn_fflags().set(kn.kn_fflags().get() | event);
    }

    // process is gone, so flag the event as finished and remove it from the process's klist
    if event == NOTE_EXIT {
        let pr = kn_process(kn);

        rw_assert_wrlock(&KQUEUE_PS_LIST_LOCK);
        mtx_enter(&kq.kq_lock);
        kn.kn_status.set(kn.kn_status.get() | KN_DETACHED);
        mtx_leave(&kq.kq_lock);

        klist_remove_locked(&pr.ps_klist, kn);
        kn.set_flags(EV_EOF | EV_ONESHOT);
        kn.kn_data().set(i64::from(w_exitcode(
            pr.ps_xexit.get() as i32,
            pr.ps_xsig.get(),
        )));
        kn.kn_ptr.p_process.set(ptr::null());
        return true;
    }

    // process forked, and user wants to track the new process, so attach a new knote to it,
    // and immediately report an event with the parent's pid.
    if event == NOTE_FORK && kn.kn_sfflags.get() & NOTE_TRACK != 0 {
        let pr = kn_process(kn);

        // register knote with new process.
        let mut kev = Kevent {
            ident: ((hint as u32) & NOTE_PDATAMASK) as usize, // pid
            filter: kn.kn_filter().get(),
            flags: kn.kn_flags().get() | EV_ADD | EV_ENABLE | EV_FLAG1,
            fflags: kn.kn_sfflags.get(),
            data: kn.kn_id().get() as i64, // parent
            udata: kn.kn_udata().get(),    // preserve udata
        };

        rw_assert_wrlock(&KQUEUE_PS_LIST_LOCK);
        mtx_leave(&pr.ps_mtx);
        let error = kqueue_register(kq, &mut kev, 0, None);
        mtx_enter(&pr.ps_mtx);

        if error.is_err() {
            kn.kn_fflags().set(kn.kn_fflags().get() | NOTE_TRACKERR);
        }
    }

    kn.kn_fflags().get() != 0
}

/// `filt_procmodify`.
pub fn filt_procmodify(kev: &mut Kevent, kn: &Knote) -> bool {
    rw_enter_write(&KQUEUE_PS_LIST_LOCK);
    let active = if kn.kn_status.get() & KN_DETACHED == 0 {
        let pr = kn_process(kn);

        mtx_enter(&pr.ps_mtx);
        let active = knote_modify(kev, kn);
        mtx_leave(&pr.ps_mtx);
        active
    } else {
        knote_assign(kev, kn);
        kn.kn_fflags().get() != 0
    };
    rw_exit_write(&KQUEUE_PS_LIST_LOCK);

    active
}

/// `filt_procprocess`.
pub fn filt_procprocess(kn: &Knote, kev: Option<&mut Kevent>) -> bool {
    let do_lock = rw_status(&KQUEUE_PS_LIST_LOCK) != RW_WRITE;

    if do_lock {
        rw_enter_write(&KQUEUE_PS_LIST_LOCK);
    }

    let active = if kn.kn_status.get() & KN_DETACHED == 0 {
        let pr = kn_process(kn);

        mtx_enter(&pr.ps_mtx);
        let active = knote_process(kn, kev);
        mtx_leave(&pr.ps_mtx);
        active
    } else {
        let active = kn.kn_fflags().get() != 0;
        if active {
            knote_submit(kn, kev);
        }
        active
    };

    if do_lock {
        rw_exit_write(&KQUEUE_PS_LIST_LOCK);
    }

    active
}

/// `filt_sigattach`: signal knotes are shared with proc knotes, so we apply a mask to the
/// hint in order to differentiate them from process hints. This could be avoided by using
/// a signal-specific knote list, but probably isn't worth the trouble.
pub fn filt_sigattach(kn: &Knote) -> Result<(), Errno> {
    let Some(cp) = curproc() else {
        panic(format_args!("filt_sigattach: no curproc"));
    };
    let pr = cp.process();

    if kn.kn_id().get() >= NSIG as usize {
        return Err(Errno::EINVAL);
    }

    kn.kn_ptr.p_process.set(pr);
    kn.set_flags(EV_CLEAR); // automatically set

    // this needs both the ps_mtx and exclusive kqueue_ps_list_lock.
    rw_enter_write(&KQUEUE_PS_LIST_LOCK);
    mtx_enter(&pr.ps_mtx);
    klist_insert_locked(&pr.ps_klist, kn);
    mtx_leave(&pr.ps_mtx);
    rw_exit_write(&KQUEUE_PS_LIST_LOCK);

    Ok(())
}

/// `filt_sigdetach`.
pub fn filt_sigdetach(kn: &Knote) {
    let pr = kn_process(kn);

    rw_enter_write(&KQUEUE_PS_LIST_LOCK);
    mtx_enter(&pr.ps_mtx);
    klist_remove_locked(&pr.ps_klist, kn);
    mtx_leave(&pr.ps_mtx);
    rw_exit_write(&KQUEUE_PS_LIST_LOCK);
}

/// `filt_signal`: counts the deliveries of signal `kn_id`.
pub fn filt_signal(kn: &Knote, hint: i64) -> bool {
    if hint & i64::from(NOTE_SIGNAL) != 0 {
        let hint = hint & !i64::from(NOTE_SIGNAL);

        if kn.kn_id().get() as i64 == hint {
            kn.kn_data().set(kn.kn_data().get() + 1);
        }
    }
    kn.kn_data().get() != 0
}

/// `filt_timervalidate(sfflags, sdata, ts)`: the timer's period, from its data and unit.
fn filt_timervalidate(sfflags: u32, sdata: i64) -> Result<Timespec, Errno> {
    if sfflags & !(NOTE_TIMER_UNITMASK | NOTE_ABSTIME) != 0 {
        return Err(Errno::EINVAL);
    }

    match sfflags & NOTE_TIMER_UNITMASK {
        NOTE_SECONDS => Ok(Timespec::new(sdata, 0)),
        NOTE_MSECONDS => Ok(Timespec::new(sdata / 1000, (sdata % 1000) * 1_000_000)),
        NOTE_USECONDS => Ok(Timespec::new(sdata / 1_000_000, (sdata % 1_000_000) * 1000)),
        NOTE_NSECONDS => Ok(Timespec::new(sdata / 1_000_000_000, sdata % 1_000_000_000)),
        _ => Err(Errno::EINVAL),
    }
}

/// `kn->kn_hook` of an `EVFILT_TIMER` knote.
fn filt_timer(kn: &Knote) -> &FiltTimer {
    // SAFETY: `filt_timerattach` points `kn_hook` at the timer it allocates, which
    // `filt_timerdetach` frees only after the timeout is gone; the knote outlives both.
    match unsafe { kn.kn_hook.get().cast::<FiltTimer>().as_ref() } {
        Some(ft) => ft,
        None => panic(format_args!("knote {:p}: no timer", kn)),
    }
}

/// `filt_timeradd(kn, ts)`: arms the timer for `ts` (relative, or absolute with
/// `NOTE_ABSTIME`).
fn filt_timeradd(kn: &Knote, ts: &Timespec) {
    let ft = filt_timer(kn);

    mutex_assert_locked(&ft.ft_mtx, "filt_timeradd");

    ft.ft_reschedule
        .set(!kn.has_flags(EV_ONESHOT) && kn.kn_sfflags.get() & NOTE_ABSTIME == 0);

    if kn.kn_sfflags.get() & NOTE_ABSTIME != 0 {
        let now = nanotime();
        if *ts > now {
            let expiry = timespecsub(ts, &now);
            // XXX timeout_abs_ts with CLOCK_REALTIME
            timeout_add(&ft.ft_to, tstohz(&expiry));
        } else {
            // Expire immediately.
            filt_dotimerexpire(kn);
        }
        return;
    }

    let mut tticks = tstohz(ts);
    // Remove extra tick from tstohz() if timeout has fired before.
    if timeout_triggered(&ft.ft_to) {
        tticks -= 1;
    }
    timeout_add(&ft.ft_to, if tticks > 0 { tticks } else { 1 });
}

/// `filt_dotimerexpire(kn)`: counts an expiry, activates the knote and re-arms a periodic
/// timer.
fn filt_dotimerexpire(kn: &Knote) {
    let ft = filt_timer(kn);
    let kq = kn.kq();

    mutex_assert_locked(&ft.ft_mtx, "filt_dotimerexpire");

    kn.kn_data().set(kn.kn_data().get() + 1);

    mtx_enter(&kq.kq_lock);
    knote_activate(kn);
    mtx_leave(&kq.kq_lock);

    if ft.ft_reschedule.get() {
        // The period was validated by filt_timerattach or filt_timermodify.
        if let Ok(ts) = filt_timervalidate(kn.kn_sfflags.get(), kn.kn_sdata.get()) {
            filt_timeradd(kn, &ts);
        }
    }
}

/// `filt_timerexpire(knx)`: the timeout's function.
pub fn filt_timerexpire(knx: *mut c_void) {
    // SAFETY: `filt_timerattach`/`filt_timermodify` set the timeout's argument to the knote,
    // and `filt_timerdetach` removes the timeout (`timeout_del_barrier`) before the knote
    // can be dropped.
    let kn = unsafe { &*knx.cast::<Knote>() };
    let ft = filt_timer(kn);

    mtx_enter(&ft.ft_mtx);
    filt_dotimerexpire(kn);
    mtx_leave(&ft.ft_mtx);
}

/// `filt_timerattach`: data contains amount of time to sleep.
pub fn filt_timerattach(kn: &Knote) -> Result<(), Errno> {
    let fdp = kn.kq().fdp();

    let ts = filt_timervalidate(kn.kn_sfflags.get(), kn.kn_sdata.get())?;

    let nuserevents = fdp.fd_nuserevents.fetch_add(1, Ordering::SeqCst) + 1;
    if nuserevents > KQ_USEREVENTSMAX.load(Ordering::Relaxed) {
        fdp.fd_nuserevents.fetch_sub(1, Ordering::SeqCst);
        return Err(Errno::ENOMEM);
    }

    if kn.kn_sfflags.get() & NOTE_ABSTIME == 0 {
        kn.set_flags(EV_CLEAR); // automatically set
    }

    let Some(mem) = malloc(size_of::<FiltTimer>(), M_KEVENT, M_WAITOK) else {
        // M_WAITOK cannot sleep yet (see the module's deviations).
        fdp.fd_nuserevents.fetch_sub(1, Ordering::SeqCst);
        return Err(Errno::ENOMEM);
    };
    let ftp = mem.cast::<FiltTimer>().as_ptr();
    // SAFETY: a fresh `malloc` block of the timer's size, aligned for any kernel object,
    // written once before anything else sees it.
    unsafe { ftp.write(FiltTimer::new()) };
    // SAFETY: as above; it stays allocated until `filt_timerdetach`.
    let ft = unsafe { &*ftp };
    mtx_init(&ft.ft_mtx, IPL_SOFTCLOCK);
    timeout_set(
        &ft.ft_to,
        filt_timerexpire,
        ptr::from_ref(kn).cast_mut().cast(),
    );
    kn.kn_hook.set(ftp.cast());

    mtx_enter(&ft.ft_mtx);
    filt_timeradd(kn, &ts);
    mtx_leave(&ft.ft_mtx);

    Ok(())
}

/// `filt_timerdetach`: stops the timer and frees it.
pub fn filt_timerdetach(kn: &Knote) {
    let fdp = kn.kq().fdp();
    let ft = filt_timer(kn);

    mtx_enter(&ft.ft_mtx);
    ft.ft_reschedule.set(false);
    mtx_leave(&ft.ft_mtx);

    timeout_del_barrier(&ft.ft_to);
    kn.kn_hook.set(ptr::null_mut());
    free(NonNull::from(ft).cast(), M_KEVENT, size_of::<FiltTimer>());
    fdp.fd_nuserevents.fetch_sub(1, Ordering::SeqCst);
}

/// `filt_timermodify`: resets the timer to the new period; any pending events are
/// discarded.
pub fn filt_timermodify(kev: &mut Kevent, kn: &Knote) -> bool {
    let kq = kn.kq();
    let ft = filt_timer(kn);

    let ts = match filt_timervalidate(kev.fflags, kev.data) {
        Ok(ts) => ts,
        Err(error) => {
            kev.flags |= EV_ERROR;
            kev.data = i64::from(error as i32);
            return false;
        }
    };

    // Reset the timer. Any pending events are discarded.
    mtx_enter(&ft.ft_mtx);
    ft.ft_reschedule.set(false);
    mtx_leave(&ft.ft_mtx);

    timeout_del_barrier(&ft.ft_to);

    mtx_enter(&ft.ft_mtx);
    mtx_enter(&kq.kq_lock);
    if kn.kn_status.get() & KN_QUEUED != 0 {
        knote_dequeue(kn);
    }
    kn.kn_status.set(kn.kn_status.get() & !KN_ACTIVE);
    mtx_leave(&kq.kq_lock);

    kn.kn_data().set(0);
    knote_assign(kev, kn);
    // Reinit timeout to invoke tick adjustment again.
    timeout_set(
        &ft.ft_to,
        filt_timerexpire,
        ptr::from_ref(kn).cast_mut().cast(),
    );
    filt_timeradd(kn, &ts);
    mtx_leave(&ft.ft_mtx);

    false
}

/// `filt_timerprocess`: active once the timer expired since the last delivery.
pub fn filt_timerprocess(kn: &Knote, kev: Option<&mut Kevent>) -> bool {
    let ft = filt_timer(kn);

    mtx_enter(&ft.ft_mtx);
    let active = kn.kn_data().get() != 0;
    if active {
        knote_submit(kn, kev);
    }
    mtx_leave(&ft.ft_mtx);

    active
}

/// `filt_userattach`.
pub fn filt_userattach(kn: &Knote) -> Result<(), Errno> {
    let fdp = kn.kq().fdp();

    let nuserevents = fdp.fd_nuserevents.fetch_add(1, Ordering::SeqCst) + 1;
    if nuserevents > KQ_USEREVENTSMAX.load(Ordering::Relaxed) {
        fdp.fd_nuserevents.fetch_sub(1, Ordering::SeqCst);
        return Err(Errno::ENOMEM);
    }

    kn.kn_ptr
        .p_useract
        .set(i32::from(kn.kn_sfflags.get() & NOTE_TRIGGER != 0));
    kn.kn_fflags().set(kn.kn_sfflags.get() & NOTE_FFLAGSMASK);
    kn.kn_data().set(kn.kn_sdata.get());

    Ok(())
}

/// `filt_userdetach`.
pub fn filt_userdetach(kn: &Knote) {
    let fdp = kn.kq().fdp();

    fdp.fd_nuserevents.fetch_sub(1, Ordering::SeqCst);
}

/// `filt_usermodify`: `NOTE_TRIGGER` activates, the `NOTE_FF*` control combines `fflags`.
pub fn filt_usermodify(kev: &mut Kevent, kn: &Knote) -> bool {
    if kev.fflags & NOTE_TRIGGER != 0 {
        kn.kn_ptr.p_useract.set(1);
    }

    let ffctrl = kev.fflags & NOTE_FFCTRLMASK;
    let fflags = kev.fflags & NOTE_FFLAGSMASK;
    match ffctrl {
        NOTE_FFNOP => {}
        NOTE_FFAND => kn.kn_fflags().set(kn.kn_fflags().get() & fflags),
        NOTE_FFOR => kn.kn_fflags().set(kn.kn_fflags().get() | fflags),
        NOTE_FFCOPY => kn.kn_fflags().set(fflags),
        _ => {
            // ignored, should not happen
        }
    }

    kn.kn_data().set(kev.data);
    kn.kn_udata().set(kev.udata);

    // Allow clearing of an activated event.
    if kev.flags & EV_CLEAR != 0 {
        kn.kn_ptr.p_useract.set(0);
    }

    kn.kn_ptr.p_useract.get() != 0
}

/// `filt_userprocess`.
pub fn filt_userprocess(kn: &Knote, kev: Option<&mut Kevent>) -> bool {
    let active = kn.kn_ptr.p_useract.get() != 0;
    if active && let Some(kev) = kev {
        *kev = kn.kn_kevent.get();
        if kn.has_flags(EV_CLEAR) {
            kn.kn_ptr.p_useract.set(0);
        }
    }

    active
}

/// `filt_seltrue`: this filter "event" routine simulates `seltrue()`.
pub fn filt_seltrue(kn: &Knote, _hint: i64) -> bool {
    // We don't know how much data can be read/written, but we know that it *can* be. This
    // is about as good as select/poll does as well.
    kn.kn_data().set(0);
    true
}

/// `filt_seltruemodify`.
pub fn filt_seltruemodify(kev: &mut Kevent, kn: &Knote) -> bool {
    knote_assign(kev, kn);
    fop_event(kn, 0)
}

/// `filt_seltrueprocess`.
pub fn filt_seltrueprocess(kn: &Knote, kev: Option<&mut Kevent>) -> bool {
    let active = fop_event(kn, 0);
    if active {
        knote_submit(kn, kev);
    }
    active
}

/// `filt_seltruedetach`: this provides full kqfilter entry for device switch tables, which
/// has same effect as filter using `filt_seltrue()` as filter method.
pub fn filt_seltruedetach(_kn: &Knote) {
    // Nothing to do
}

/// `seltrue_kqfilter(dev, kn)`: the kqfilter of a device that is always ready.
pub fn seltrue_kqfilter(_dev: Dev, kn: &Knote) -> Result<(), Errno> {
    match kn.kn_filter().get() {
        EVFILT_READ | EVFILT_WRITE => kn.kn_fop.set(Some(&SELTRUE_FILTOPS)),
        _ => return Err(Errno::EINVAL),
    }

    // Nothing more to do
    Ok(())
}

/// `filt_dead`: the object is gone.
fn filt_dead(kn: &Knote, _hint: i64) -> bool {
    if kn.kn_filter().get() == EVFILT_EXCEPT {
        // Do not deliver event because there is no out-of-band data. However, let HUP
        // condition pass for poll(2).
        if !kn.has_flags(__EV_POLL) {
            kn.set_flags(EV_DISABLE);
            return false;
        }
    }

    kn.set_flags(EV_EOF | EV_ONESHOT);
    if kn.has_flags(__EV_POLL) {
        kn.set_flags(__EV_HUP);
    }
    kn.kn_data().set(0);
    true
}

/// `filt_deaddetach`.
fn filt_deaddetach(_kn: &Knote) {
    // Nothing to do
}

/// `filt_badfd`: the descriptor was closed under a poll or select.
fn filt_badfd(kn: &Knote, _hint: i64) -> bool {
    kn.set_flags(EV_ERROR | EV_ONESHOT);
    kn.kn_data().set(i64::from(Errno::EBADF as i32));
    true
}

/// `filter_attach(kn)`.
fn filter_attach(kn: &Knote) -> Result<(), Errno> {
    let fop = kn.fop();
    let Some(f_attach) = fop.f_attach else {
        panic(format_args!(
            "filter_attach: knote {:p} has no f_attach",
            kn
        ));
    };

    // Without FILTEROP_MPSAFE the C takes KERNEL_LOCK(): nothing without MULTIPROCESSOR.
    f_attach(kn)
}

/// `filter_detach(kn)`.
fn filter_detach(kn: &Knote) {
    let fop = kn.fop();
    let Some(f_detach) = fop.f_detach else {
        panic(format_args!(
            "filter_detach: knote {:p} has no f_detach",
            kn
        ));
    };

    // Without FILTEROP_MPSAFE the C takes KERNEL_LOCK(): nothing without MULTIPROCESSOR.
    f_detach(kn);
}

/// `filter_event(kn, hint)`.
fn filter_event(kn: &Knote, hint: i64) -> bool {
    // Without FILTEROP_MPSAFE: KERNEL_ASSERT_LOCKED(), nothing without MULTIPROCESSOR.
    fop_event(kn, hint)
}

/// `filter_modify(kev, kn)`.
fn filter_modify(kev: &mut Kevent, kn: &Knote) -> bool {
    let fop = kn.fop();

    if fop.f_flags & FILTEROP_MPSAFE != 0 {
        match fop.f_modify {
            Some(f_modify) => f_modify(kev, kn),
            None => panic(format_args!(
                "filter_modify: knote {:p} has no f_modify",
                kn
            )),
        }
    } else {
        // KERNEL_LOCK(): nothing without MULTIPROCESSOR.
        match fop.f_modify {
            Some(f_modify) => f_modify(kev, kn),
            None => {
                let s = splhigh();
                let active = knote_modify(kev, kn);
                splx(s);
                active
            }
        }
    }
}

/// `filter_process(kn, kev)`.
fn filter_process(kn: &Knote, kev: Option<&mut Kevent>) -> bool {
    let fop = kn.fop();

    if fop.f_flags & FILTEROP_MPSAFE != 0 {
        match fop.f_process {
            Some(f_process) => f_process(kn, kev),
            None => panic(format_args!(
                "filter_process: knote {:p} has no f_process",
                kn
            )),
        }
    } else {
        // KERNEL_LOCK(): nothing without MULTIPROCESSOR.
        match fop.f_process {
            Some(f_process) => f_process(kn, kev),
            None => {
                let s = splhigh();
                let active = knote_process(kn, kev);
                splx(s);
                active
            }
        }
    }
}

/// `kqpoll_init(num)`: initialize the current thread for poll/select system call. `num`
/// indicates the number of serials that the system call may utilize. After this function,
/// the valid range of serials is `p_kq_serial <= x < p_kq_serial + num`.
pub fn kqpoll_init(p: &Proc, num: u32) -> Result<(), Errno> {
    if p.p_kq.get().is_null() {
        let kq = kqueue_alloc(p.fd())?;
        p.p_kq.set(kq);
        p.p_kq_serial.set(u64::from(crate::dev::rnd::arc4random()));
        let fdp = p.fd();
        fdplock(fdp);
        // SAFETY: the kqueue is new and on no list; it stays until `KQRELE` takes it off,
        // under the table's lock.
        unsafe { fdp.fd_kqlist.insert_head(kq) };
        fdpunlock(fdp);
    }

    let serial = p.p_kq_serial.get();
    if serial.wrapping_add(u64::from(num)) < serial {
        // Serial is about to wrap. Clear all attached knotes.
        kqueue_purge(Some(p), p.kq());
        p.p_kq_serial.set(0);
    }
    Ok(())
}

/// `kqpoll_done(num)`: finish poll/select system call. `num` must have the same value that
/// was used with `kqpoll_init()`.
pub fn kqpoll_done(p: &Proc, num: u32) {
    let kq = p.kq();

    let serial = p.p_kq_serial.get();
    kassert!(serial.wrapping_add(u64::from(num)) >= serial);

    p.p_kq_serial.set(serial.wrapping_add(u64::from(num)));

    // Because of kn_pollid key, a thread can in principle allocate up to O(maxfiles^2)
    // knotes by calling poll(2) repeatedly with suitably varying pollfd arrays. Prevent such
    // a large allocation by clearing knotes eagerly if there are too many of them.
    //
    // A small multiple of kq_knlistsize should give enough margin that eager clearing is
    // infrequent, or does not happen at all, with normal programs. A single pollfd entry can
    // use up to three knotes. Typically there is no significant overlap of fd and events
    // between different entries in the pollfd array.
    if u64::from(kq.kq_nknotes.get()) > 4 * kq.kq_knlistsize.get() as u64 {
        kqueue_purge(Some(p), kq);
    }
}

/// `kqpoll_exit()`: the thread's poll kqueue goes away with the thread.
pub fn kqpoll_exit(p: &Proc) {
    if p.p_kq.get().is_null() {
        return;
    }

    let kq = p.kq();
    kqueue_purge(Some(p), kq);
    kqueue_terminate(Some(p), kq);
    kassert!(refcnt_read(&kq.kq_refcnt) == 1);
    // SAFETY: the thread's reference, the last one (asserted); `p_kq` is cleared next.
    unsafe { KQRELE(kq) };
    p.p_kq.set(ptr::null());
}

/// `kqueue_alloc(fdp)`: a new kqueue on `fdp` with one reference; `ENOMEM` when the pool
/// is empty (see the module's deviations).
pub fn kqueue_alloc(fdp: &Filedesc) -> Result<&'static Kqueue, Errno> {
    let Some(mem) = pool_get(&KQUEUE_POOL, PR_WAITOK | PR_ZERO) else {
        return Err(Errno::ENOMEM);
    };
    let raw = mem.cast::<Kqueue>().as_ptr();
    // SAFETY: a fresh, suitably aligned pool item of `size_of::<Kqueue>()` bytes, written
    // once before anything else sees it.
    unsafe { raw.write(Kqueue::new()) };
    // SAFETY: as above; the item stays allocated until `KQRELE` drops the last reference.
    let kq: &'static Kqueue = unsafe { &*raw };

    refcnt_init(&kq.kq_refcnt);
    kq.kq_fdp.set(fdp);
    kq.kq_head.init();
    mtx_init(&kq.kq_lock, IPL_HIGH);
    task_set(&kq.kq_task, kqueue_task, raw.cast());
    // SAFETY: `kqueue_klist_lock` is a static.
    unsafe { klist_init_mutex(&kq.kq_klist, &KQUEUE_KLIST_LOCK) };

    Ok(kq)
}

/// `dokqueue(p, flags, retval)`: a new kqueue at the lowest free descriptor.
pub fn dokqueue(p: &Proc, flags: i32, retval: &mut [Register; 2]) -> Result<(), Errno> {
    let fdp = p.fd();

    let cloexec = if flags & O_CLOEXEC != 0 {
        UF_EXCLOSE
    } else {
        0
    };

    let kq = kqueue_alloc(fdp)?;

    fdplock(fdp);
    let (fp, fd) = match falloc(p) {
        Ok(r) => r,
        Err(error) => {
            // out:
            fdpunlock(fdp);
            pool_put(&KQUEUE_POOL, NonNull::from(kq).cast());
            return Err(error);
        }
    };
    fp.f_flag.store(
        (FREAD | FWRITE | (flags & FNONBLOCK)) as u32,
        Ordering::SeqCst,
    );
    fp.f_type.set(DTYPE_KQUEUE);
    fp.f_ops.set(Some(&KQUEUEOPS));
    fp.f_data.set(ptr::from_ref(kq).cast_mut().cast());
    retval[0] = fd as Register;
    // SAFETY: the kqueue is new and on no list; `KQRELE` takes it off under the table's
    // lock.
    unsafe { fdp.fd_kqlist.insert_head(kq) };
    fdinsert(fdp, fd, cloexec, fp);
    let _ = frele(fp, p);
    // out:
    fdpunlock(fdp);
    Ok(())
}

/// `kqueue(2)`.
pub fn sys_kqueue(p: &Proc, _v: &SysArgs, retval: &mut [Register; 2]) -> Result<(), Errno> {
    dokqueue(p, 0, retval)
}

/// `kqueue1(2)`: `kqueue(2)` with `O_CLOEXEC` and `O_NONBLOCK`.
pub fn sys_kqueue1(p: &Proc, v: &SysArgs, retval: &mut [Register; 2]) -> Result<(), Errno> {
    let uap: &SysKqueue1Args = sysargs(v);

    if uap.flags.get() & !(O_CLOEXEC | FNONBLOCK) != 0 {
        return Err(Errno::EINVAL);
    }
    dokqueue(p, uap.flags.get(), retval)
}

/// `copyin` of `kev.len()` events from the user array at `uaddr`.
fn kevents_copyin(uaddr: usize, kev: &mut [Kevent]) -> Result<(), Errno> {
    // SAFETY: `Kevent` is `AbiPod` (`#[repr(C)]` integers without padding): the slice's bytes
    // are initialised, and any bytes `copyin` writes form valid events.
    let bytes =
        unsafe { slice::from_raw_parts_mut(kev.as_mut_ptr().cast::<u8>(), size_of_val(kev)) };
    copyin(uaddr, bytes)
}

/// `copyout` of `kev` to the user array at `uaddr`.
fn kevents_copyout(kev: &[Kevent], uaddr: usize) -> Result<(), Errno> {
    // SAFETY: as in `kevents_copyin`: the events' bytes are initialised.
    let bytes = unsafe { slice::from_raw_parts(kev.as_ptr().cast::<u8>(), size_of_val(kev)) };
    copyout(bytes, uaddr)
}

/// `kevent(2)`: registers the changes, then collects the pending events.
pub fn sys_kevent(p: &Proc, v: &SysArgs, retval: &mut [Register; 2]) -> Result<(), Errno> {
    let uap: &SysKeventArgs = sysargs(v);
    let fdp = p.fd();
    let mut changelist = uap.changelist.get() as usize;
    let mut nchanges = uap.nchanges.get();
    let mut eventlist = uap.eventlist.get() as usize;
    let mut nevents = uap.nevents.get();
    let mut kev = [Kevent::default(); KQ_NEVENTS];

    let Some(fp) = fd_getfile(fdp, uap.fd.get()) else {
        return Err(Errno::EBADF);
    };

    let error: Result<(), Errno> = 'done: {
        if fp.f_type.get() != DTYPE_KQUEUE {
            break 'done Err(Errno::EBADF);
        }

        let mut tsp: Option<Timespec> = None;
        let uts = uap.timeout.get() as usize;
        if uts != 0 {
            let ts = match copyin_obj::<Timespec>(uts) {
                Ok(ts) => ts,
                Err(error) => break 'done Err(error),
            };
            // KTRACE (ktrreltimespec): not configured.
            if ts.tv_sec < 0 || !ts.is_valid() {
                break 'done Err(Errno::EINVAL);
            }
            tsp = Some(ts);
        }

        let kq = fp_kqueue(fp);
        let mut nerrors: Register = 0;

        while nchanges > 0 {
            let n = (nchanges as usize).min(KQ_NEVENTS);
            if let Err(error) = kevents_copyin(changelist, &mut kev[..n]) {
                break 'done Err(error);
            }
            // KTRACE (ktrevent): not configured.
            for kevp in &mut kev[..n] {
                kevp.flags &= !EV_SYSFLAGS;
                let error = kqueue_register(kq, kevp, 0, Some(p));
                if error.is_err() || kevp.flags & EV_RECEIPT != 0 {
                    if nevents != 0 {
                        kevp.flags = EV_ERROR;
                        kevp.data = error.map_or_else(|e| i64::from(e as i32), |()| 0);
                        let _ = kevents_copyout(slice::from_ref(kevp), eventlist);
                        eventlist += size_of::<Kevent>();
                        nevents -= 1;
                        nerrors += 1;
                    } else {
                        break 'done error;
                    }
                }
            }
            nchanges -= n as i32;
            changelist += n * size_of::<Kevent>();
        }
        if nerrors != 0 {
            retval[0] = nerrors;
            break 'done Ok(());
        }

        let scan = KqueueScanState::new();
        // SAFETY: `scan` is a local that stays in place until `kqueue_scan_finish` below.
        unsafe { kqueue_scan_setup(&scan, kq) };
        let _ = frele(fp, p);
        // Collect as many events as we can. The timeout on successive loops is disabled
        // (kqueue_scan() becomes non-blocking).
        let mut total = 0usize;
        let mut error = Ok(());
        loop {
            let n = (nevents as usize).saturating_sub(total).min(KQ_NEVENTS);
            if n == 0 {
                break;
            }
            let ready = kqueue_scan(&scan, n, &mut kev, tsp.as_mut(), p, &mut error);
            if ready == 0 {
                break;
            }
            error = kevents_copyout(&kev[..ready], eventlist + total * size_of::<Kevent>());
            // KTRACE (ktrevent): not configured.
            total += ready;
            if error.is_err() || ready < n {
                break;
            }
        }
        kqueue_scan_finish(&scan);
        retval[0] = total as Register;
        return error;
    };

    // done:
    let _ = frele(fp, p);
    error
}

/// `kqueue_register(kq, kev, pollid, p)`: applies one change (`EV_ADD`, `EV_DELETE`,
/// `EV_ENABLE`, `EV_DISABLE`, a user trigger) to the kqueue.
pub fn kqueue_register(
    kq: &Kqueue,
    kev: &mut Kevent,
    pollid: u32,
    p: Option<&Proc>,
) -> Result<(), Errno> {
    let fdp = kq.fdp();

    kassert!(pollid == 0 || p.is_some_and(|p| ptr::eq(p.p_kq.get(), kq)));

    let mut fops: Option<&'static Filterops> = None;
    if kev.filter < 0 {
        if i32::from(kev.filter) + EVFILT_SYSCOUNT < 0 {
            return Err(Errno::EINVAL);
        }
        fops = SYSFILT_OPS[(!kev.filter) as usize]; // to 0-base index
    }

    let Some(fops) = fops else {
        // XXX filter attach routine is responsible for ensuring that the identifier can be
        // attached to it.
        return Err(Errno::EINVAL);
    };
    let isfd = fops.f_flags & FILTEROP_ISFD != 0;

    if isfd {
        // validate descriptor
        if kev.ident > i32::MAX as usize {
            return Err(Errno::EBADF);
        }
    }

    let mut newkn: Option<&'static Knote> = None;
    if kev.flags & EV_ADD != 0 {
        newkn = Some(knote_alloc()?);
    }
    let mut fp: Option<&'static File> = None;

    let error: Result<(), Errno> = 'done: loop {
        // again:
        let mut list: Option<&Knlist> = None;
        if isfd {
            let Some(f) = fd_getfile(fdp, kev.ident as i32) else {
                break 'done Err(Errno::EBADF);
            };
            fp = Some(f);
            mtx_enter(&kq.kq_lock);
            if kev.flags & EV_ADD != 0
                && let Err(error) = kqueue_expand_list(kq, kev.ident as i32)
            {
                mtx_leave(&kq.kq_lock);
                break 'done Err(error);
            }
            if kev.ident < kq.kq_knlistsize.get() as usize {
                list = Some(kq.knlist(kev.ident));
            }
        } else {
            mtx_enter(&kq.kq_lock);
            if kev.flags & EV_ADD != 0
                && let Err(error) = kqueue_expand_hash(kq)
            {
                mtx_leave(&kq.kq_lock);
                break 'done Err(error);
            }
            let mask = kq.kq_knhashmask.get();
            if mask != 0 {
                list = Some(kq.knhash(kn_hash(kev.ident as u64, mask) as usize));
            }
        }
        let mut kn: Option<&Knote> = None;
        if let Some(list) = list {
            let found = list.iter().find(|kn| {
                kev.filter == kn.kn_filter().get()
                    && kev.ident == kn.kn_id().get()
                    && pollid == kn.kn_pollid.get()
            });
            if let Some(found) = found {
                if !knote_acquire(found, None, 0) {
                    // knote_acquire() has released kq_lock.
                    if let Some(f) = fp.take() {
                        let _ = frele(f, p);
                    }
                    continue;
                }
                kn = Some(found);
            }
        }
        kassert!(kn.is_none_or(|kn| kn.kn_status.get() & KN_PROCESSING != 0));

        // kn now contains the matching knote, or None if no match.
        let kn: &Knote = match kn {
            None if kev.flags & EV_ADD == 0 => {
                mtx_leave(&kq.kq_lock);
                break 'done Err(Errno::ENOENT);
            }
            None => {
                let Some(kn) = newkn.take() else {
                    panic(format_args!("kqueue_register: EV_ADD without a new knote"));
                };
                kn.kn_status.set(KN_PROCESSING);
                // apply reference count to knote structure, and do not release it at the
                // end of this routine.
                kn.kn_fp().set(fp.take());
                kn.kn_kq.set(kq);
                kn.kn_fop.set(Some(fops));

                kn.kn_sfflags.set(kev.fflags);
                kn.kn_sdata.set(kev.data);
                kev.fflags = 0;
                kev.data = 0;
                kn.kn_kevent.set(kev);
                kn.kn_pollid.set(pollid);

                knote_attach(kn);
                mtx_leave(&kq.kq_lock);

                if let Err(error) = filter_attach(kn) {
                    // SAFETY: the knote is ours (KN_PROCESSING) and is not used again.
                    unsafe { knote_drop(kn, p) };
                    break 'done Err(error);
                }

                // If this is a file descriptor filter, check if fd was closed while the knote
                // was being added. knote_fdclose() has missed kn if the function ran before
                // kn appeared in kq_knlist.
                if isfd && fd_checkclosed(fdp, kev.ident as i32, kn.fp()) {
                    // Drop the knote silently without error because another thread might
                    // already have seen it. This corresponds to the insert happening in
                    // full before the close.
                    filter_detach(kn);
                    // SAFETY: as above.
                    unsafe { knote_drop(kn, p) };
                    break 'done Ok(());
                }

                // Check if there is a pending event.
                let active = filter_process(kn, None);
                mtx_enter(&kq.kq_lock);
                if active {
                    knote_activate(kn);
                }
                kn
            }
            Some(kn) if kev.flags & EV_ADD != 0 && ptr::eq(kn.fop(), &BADFD_FILTOPS) => {
                // Nothing expects this badfd knote any longer. Drop it to make room for the
                // new knote and retry.
                kassert!(p.is_some_and(|p| ptr::eq(p.p_kq.get(), kq)));
                mtx_leave(&kq.kq_lock);
                filter_detach(kn);
                // SAFETY: the knote is ours (KN_PROCESSING) and is not used again.
                unsafe { knote_drop(kn, p) };

                kassert!(fp.is_some());
                if let Some(f) = fp.take() {
                    let _ = frele(f, p);
                }

                continue;
            }
            Some(kn) if kev.flags & EV_ADD != 0 => {
                // The user may change some filter values after the initial EV_ADD, but doing
                // so will not reset any filters which have already been triggered.
                mtx_leave(&kq.kq_lock);
                let active = filter_modify(kev, kn);
                mtx_enter(&kq.kq_lock);
                if active {
                    knote_activate(kn);
                }
                if kev.flags & EV_ERROR != 0 {
                    // release:
                    knote_release(kn);
                    mtx_leave(&kq.kq_lock);
                    break 'done Err(errno_of(kev.data));
                }
                kn
            }
            Some(kn) if kev.flags & EV_DELETE != 0 => {
                mtx_leave(&kq.kq_lock);
                filter_detach(kn);
                // SAFETY: the knote is ours (KN_PROCESSING) and is not used again.
                unsafe { knote_drop(kn, p) };
                break 'done Ok(());
            }
            Some(kn) if ptr::eq(kn.fop(), &USER_FILTOPS) => {
                // Call f_modify to allow NOTE_TRIGGER without EV_ADD.
                mtx_leave(&kq.kq_lock);
                let active = filter_modify(kev, kn);
                mtx_enter(&kq.kq_lock);
                if active {
                    knote_activate(kn);
                }
                if kev.flags & EV_ERROR != 0 {
                    // release:
                    knote_release(kn);
                    mtx_leave(&kq.kq_lock);
                    break 'done Err(errno_of(kev.data));
                }
                kn
            }
            Some(kn) => kn,
        };

        if kev.flags & EV_DISABLE != 0 && kn.kn_status.get() & KN_DISABLED == 0 {
            kn.kn_status.set(kn.kn_status.get() | KN_DISABLED);
        }

        if kev.flags & EV_ENABLE != 0 && kn.kn_status.get() & KN_DISABLED != 0 {
            kn.kn_status.set(kn.kn_status.get() & !KN_DISABLED);
            mtx_leave(&kq.kq_lock);
            // Check if there is a pending event.
            let active = filter_process(kn, None);
            mtx_enter(&kq.kq_lock);
            if active {
                knote_activate(kn);
            }
        }

        // release:
        knote_release(kn);
        mtx_leave(&kq.kq_lock);
        break 'done Ok(());
    };

    // done:
    if let Some(f) = fp {
        let _ = frele(f, p);
    }
    if let Some(kn) = newkn {
        pool_put(&KNOTE_POOL, NonNull::from(kn).cast());
    }
    error
}

/// The errno a filter left in `kev->data` with `EV_ERROR`.
fn errno_of(data: i64) -> Errno {
    Errno::from_raw(data as i32).unwrap_or(Errno::EINVAL)
}

/// `pool_get(&knote_pool, PR_WAITOK | PR_ZERO)`: a zeroed knote, or `ENOMEM` (see the
/// module's deviations).
fn knote_alloc() -> Result<&'static Knote, Errno> {
    let Some(mem) = pool_get(&KNOTE_POOL, PR_WAITOK | PR_ZERO) else {
        return Err(Errno::ENOMEM);
    };
    let raw = mem.cast::<Knote>().as_ptr();
    // SAFETY: a fresh, suitably aligned pool item of `size_of::<Knote>()` bytes, written once
    // before anything else sees it.
    unsafe { raw.write(Knote::new()) };
    // SAFETY: as above; the item stays allocated until `knote_drop` or `kqueue_register`
    // gives it back.
    Ok(unsafe { &*raw })
}

/// `kqueue_sleep(kq, tsp)`: sleeps on the kqueue (`kq_lock` held, released), at most `tsp`,
/// which is reduced by the time slept.
pub fn kqueue_sleep(kq: &Kqueue, tsp: Option<&mut Timespec>) -> Result<(), Errno> {
    mutex_assert_locked(&kq.kq_lock, "kqueue_sleep");

    let mut start = Timespec::default();
    let nsecs = match &tsp {
        Some(ts) => {
            start = getnanouptime();
            timespec_to_nsec(ts).min(MAXTSLP)
        }
        None => INFSLP,
    };
    let error = msleep_nsec(
        ptr::from_ref(kq),
        &kq.kq_lock,
        PSOCK | PCATCH | PNORELOCK,
        "kqread",
        nsecs,
    );
    if let Some(ts) = tsp {
        let stop = getnanouptime();
        let elapsed = timespecsub(&stop, &start);
        *ts = timespecsub(ts, &elapsed);
        if ts.tv_sec < 0 {
            ts.clear();
        }
    }

    error
}

/// `kqueue_scan(scan, maxevents, kevp, tsp, p, errorp)`: scan the kqueue, blocking if
/// necessary until the target time is reached. If `tsp` is `None` we block indefinitely.
/// If `tsp`'s seconds and nanoseconds are both 0 we do not block at all. Returns how many
/// events were stored at the start of `kevp`.
pub fn kqueue_scan(
    scan: &KqueueScanState,
    maxevents: usize,
    kevp: &mut [Kevent],
    mut tsp: Option<&mut Timespec>,
    p: &Proc,
    errorp: &mut Result<(), Errno>,
) -> usize {
    let kq = scan.kq();
    let mut nkev = 0usize;
    let mut error = Ok(());

    'done: {
        if maxevents == 0 {
            break 'done;
        }
        // retry:
        loop {
            kassert!(nkev == 0);

            error = Ok(());
            let mut reinserted = false;

            mtx_enter(&kq.kq_lock);

            if kq.kq_state.get() & KQ_DYING != 0 {
                mtx_leave(&kq.kq_lock);
                error = Err(Errno::EBADF);
                break 'done;
            }

            if kq.kq_count.get() == 0 {
                // Successive loops are only necessary if there are more ready events to
                // gather, so they don't need to block.
                if tsp.as_ref().is_some_and(|ts| !ts.is_set()) || scan.kqs_nevent.get() != 0 {
                    mtx_leave(&kq.kq_lock);
                    error = Ok(());
                    break 'done;
                }
                kq.kq_state.set(kq.kq_state.get() | KQ_SLEEP);
                error = kqueue_sleep(kq, tsp.as_deref_mut());
                // kqueue_sleep() has released kq_lock.
                match error {
                    Ok(()) | Err(Errno::EWOULDBLOCK) => continue,
                    // don't restart after signals...
                    Err(Errno::ERESTART) => error = Err(Errno::EINTR),
                    Err(_) => {}
                }
                break 'done;
            }

            // Put the end marker in the queue to limit the scan to the events that are
            // currently active. This prevents events from being recollected if they
            // reactivate during scan.
            //
            // If a partial scan has been performed already but no events have been
            // collected, reposition the end marker to make any new events reachable.
            if !scan.kqs_queued.get() {
                // SAFETY: the end marker is in no queue, and `kqueue_scan_setup`'s contract
                // keeps it in place until `kqueue_scan_finish` unlinks it; under kq_lock.
                unsafe { kq.kq_head.insert_tail(&scan.kqs_end) };
                scan.kqs_queued.set(true);
            } else if scan.kqs_nevent.get() == 0 {
                // SAFETY: the end marker is queued (kqs_queued); as above.
                unsafe {
                    kq.kq_head.remove(&scan.kqs_end);
                    kq.kq_head.insert_tail(&scan.kqs_end);
                }
            }

            // SAFETY: the start marker is in no queue (each scan pass unlinks it); as above.
            unsafe { kq.kq_head.insert_head(&scan.kqs_start) };
            while nkev < maxevents {
                let Some(kn) = TailqHead::<KnTqe>::next(&scan.kqs_start) else {
                    panic(format_args!("kqueue_scan: end marker missing"));
                };
                if kn.kn_filter().get() == EVFILT_MARKER {
                    if ptr::eq(kn, &scan.kqs_end) {
                        break;
                    }

                    // Move start marker past another thread's marker.
                    // SAFETY: both are in this queue, under kq_lock.
                    unsafe {
                        kq.kq_head.remove(&scan.kqs_start);
                        kq.kq_head.insert_after(kn, &scan.kqs_start);
                    }
                    continue;
                }

                if !knote_acquire(kn, None, 0) {
                    // knote_acquire() has released kq_lock.
                    mtx_enter(&kq.kq_lock);
                    continue;
                }

                // kqueue_check(kq): KQUEUE_DEBUG, not configured.
                // SAFETY: a queued knote (it follows the start marker), under kq_lock.
                unsafe { kq.kq_head.remove(kn) };
                kn.kn_status.set(kn.kn_status.get() & !KN_QUEUED);
                kq.kq_count.set(kq.kq_count.get() - 1);

                if kn.kn_status.get() & KN_DISABLED != 0 {
                    knote_release(kn);
                    continue;
                }

                mtx_leave(&kq.kq_lock);

                // Drop expired kqpoll knotes.
                if ptr::eq(p.p_kq.get(), kq) && p.p_kq_serial.get() > kn.kn_udata().get() as u64 {
                    filter_detach(kn);
                    // SAFETY: the knote is ours (KN_PROCESSING) and is not used again.
                    unsafe { knote_drop(kn, Some(p)) };
                    mtx_enter(&kq.kq_lock);
                    continue;
                }

                // Invalidate knotes whose vnodes have been revoked. This is a workaround; it
                // is tricky to clear existing knotes and prevent new ones from being
                // registered with the current revocation mechanism.
                if kn.fop().f_flags & FILTEROP_ISFD != 0
                    && let Some(fp) = kn.kn_fp().get()
                    && fp.f_type.get() == DTYPE_VNODE
                {
                    let vp = fp.vnode();

                    if vp.v_op.get().is_some_and(|ops| ptr::eq(ops, &DEAD_VOPS))
                        && !ptr::eq(kn.fop(), &DEAD_FILTOPS)
                    {
                        filter_detach(kn);
                        kn.kn_fop.set(Some(&DEAD_FILTOPS));

                        // Check if the event should be delivered. Use f_event directly
                        // because this is a special situation.
                        if !fop_event(kn, 0) {
                            filter_detach(kn);
                            // SAFETY: as above.
                            unsafe { knote_drop(kn, Some(p)) };
                            mtx_enter(&kq.kq_lock);
                            continue;
                        }
                    }
                }

                kevp[nkev] = Kevent::default();
                if !filter_process(kn, Some(&mut kevp[nkev])) {
                    mtx_enter(&kq.kq_lock);
                    if kn.kn_status.get() & KN_QUEUED == 0 {
                        kn.kn_status.set(kn.kn_status.get() & !KN_ACTIVE);
                    }
                    knote_release(kn);
                    continue;
                }

                // Post-event action on the note
                let flags = kevp[nkev].flags;
                if flags & EV_ONESHOT != 0 {
                    filter_detach(kn);
                    // SAFETY: as above.
                    unsafe { knote_drop(kn, Some(p)) };
                    mtx_enter(&kq.kq_lock);
                } else if flags & (EV_CLEAR | EV_DISPATCH) != 0 {
                    mtx_enter(&kq.kq_lock);
                    if flags & EV_DISPATCH != 0 {
                        kn.kn_status.set(kn.kn_status.get() | KN_DISABLED);
                    }
                    if kn.kn_status.get() & KN_QUEUED == 0 {
                        kn.kn_status.set(kn.kn_status.get() & !KN_ACTIVE);
                    }
                    knote_release(kn);
                } else {
                    mtx_enter(&kq.kq_lock);
                    if kn.kn_status.get() & KN_QUEUED == 0 {
                        kq.kq_count.set(kq.kq_count.get() + 1);
                        kn.kn_status.set(kn.kn_status.get() | KN_QUEUED);
                        // SAFETY: the knote is in no queue (not KN_QUEUED) and stays until
                        // dequeued; under kq_lock.
                        unsafe { kq.kq_head.insert_tail(kn) };
                        // Wakeup is done after loop.
                        reinserted = true;
                    }
                    knote_release(kn);
                }

                nkev += 1;
                scan.kqs_nevent.set(scan.kqs_nevent.get() + 1);
            }
            // SAFETY: inserted above; under kq_lock.
            unsafe { kq.kq_head.remove(&scan.kqs_start) };
            if reinserted && kq.kq_count.get() != 0 {
                kqueue_wakeup(kq);
            }
            mtx_leave(&kq.kq_lock);
            if scan.kqs_nevent.get() == 0 {
                continue;
            }
            break 'done;
        }
    }

    // done:
    *errorp = error;
    nkev
}

/// `kqueue_scan_setup(scan, kq)`: prepares a scan of `kq`, taking a reference to it.
///
/// # Safety
///
/// `scan` is a fresh state ([`KqueueScanState::new`]) that stays in place, and is not
/// dropped, until [`kqueue_scan_finish`] has run on it: its markers are linked into the
/// kqueue's queue in between.
pub unsafe fn kqueue_scan_setup(scan: &KqueueScanState, kq: &Kqueue) {
    // memset(scan, 0, sizeof(*scan)): the state is fresh (the contract).
    KQREF(kq);
    scan.kqs_kq.set(kq);
    scan.kqs_nevent.set(0);
    scan.kqs_queued.set(false);
    scan.kqs_start.kn_filter().set(EVFILT_MARKER);
    scan.kqs_start.kn_status.set(KN_PROCESSING);
    scan.kqs_end.kn_filter().set(EVFILT_MARKER);
    scan.kqs_end.kn_status.set(KN_PROCESSING);
}

/// `kqueue_scan_finish(scan)`: unlinks the end marker and drops the scan's reference.
pub fn kqueue_scan_finish(scan: &KqueueScanState) {
    let kq = scan.kq();

    kassert!(scan.kqs_start.kn_filter().get() == EVFILT_MARKER);
    kassert!(scan.kqs_start.kn_status.get() == KN_PROCESSING);
    kassert!(scan.kqs_end.kn_filter().get() == EVFILT_MARKER);
    kassert!(scan.kqs_end.kn_status.get() == KN_PROCESSING);

    if scan.kqs_queued.get() {
        scan.kqs_queued.set(false);
        mtx_enter(&kq.kq_lock);
        // SAFETY: the end marker is queued (kqs_queued); under kq_lock.
        unsafe { kq.kq_head.remove(&scan.kqs_end) };
        mtx_leave(&kq.kq_lock);
    }
    // SAFETY: the reference `kqueue_scan_setup` took; the scan does not use the kqueue
    // afterwards (`kqs_kq` is cleared).
    unsafe { KQRELE(kq) };
    scan.kqs_kq.set(ptr::null());
}

/// `kqueue_read`: XXX This could be expanded to call kqueue_scan, if desired.
pub fn kqueue_read(_fp: &File, _uio: &mut Uio<'_>, _fflags: i32) -> Result<(), Errno> {
    Err(Errno::ENXIO)
}

/// `kqueue_write`.
pub fn kqueue_write(_fp: &File, _uio: &mut Uio<'_>, _fflags: i32) -> Result<(), Errno> {
    Err(Errno::ENXIO)
}

/// `kqueue_ioctl`.
pub fn kqueue_ioctl(_fp: &File, _com: u64, _data: &mut [u8], _p: &Proc) -> Result<(), Errno> {
    Err(Errno::ENOTTY)
}

/// `kqueue_stat`: the pending count as the size, a FIFO.
pub fn kqueue_stat(fp: &File, st: &mut Stat, _p: &Proc) -> Result<(), Errno> {
    let kq = fp_kqueue(fp);

    *st = Stat::default();
    st.st_size = i64::from(kq.kq_count.get()); // unlocked read
    st.st_blksize = size_of::<Kevent>() as _;
    st.st_mode = S_IFIFO;
    Ok(())
}

/// `kqueue_purge(p, kq)`: drops every knote of the kqueue.
pub fn kqueue_purge(p: Option<&Proc>, kq: &Kqueue) {
    mtx_enter(&kq.kq_lock);
    let mut i = 0;
    while i < kq.kq_knlistsize.get() as usize {
        knote_remove(p, kq, false, i, true);
        i += 1;
    }
    if kq.kq_knhashmask.get() != 0 {
        let mut i = 0;
        while i < kq.kq_knhashmask.get() as usize + 1 {
            knote_remove(p, kq, true, i, true);
            i += 1;
        }
    }
    mtx_leave(&kq.kq_lock);
}

/// `kqueue_terminate(p, kq)`: marks the kqueue dying and wakes its sleepers.
pub fn kqueue_terminate(_p: Option<&Proc>, kq: &Kqueue) {
    mtx_enter(&kq.kq_lock);

    // Any remaining entries should be scan markers. They are removed when the ongoing scans
    // finish.
    kassert!(kq.kq_count.get() == 0);
    for kn in kq.kq_head.iter() {
        kassert!(kn.kn_filter().get() == EVFILT_MARKER);
    }

    kq.kq_state.set(kq.kq_state.get() | KQ_DYING);
    let state = kq.kq_state.get();
    kqueue_wakeup(kq);
    mtx_leave(&kq.kq_lock);

    // Any knotes that were attached to this kqueue were deleted by knote_fdclose() when this
    // kqueue's file descriptor was closed.
    kassert!(klist_empty(&kq.kq_klist));
    if state & KQ_TASK != 0 {
        taskq_del_barrier(SYSTQMP, &kq.kq_task);
    }
}

/// `kqueue_close`: the descriptor's last close.
pub fn kqueue_close(fp: &File, p: Option<&Proc>) -> Result<(), Errno> {
    let kq = fp_kqueue(fp);

    fp.f_data.set(ptr::null_mut());

    kqueue_purge(p, kq);
    kqueue_terminate(p, kq);

    // SAFETY: the file's reference (`dokqueue`); `f_data` no longer names the kqueue.
    unsafe { KQRELE(kq) };

    Ok(())
}

/// `kqueue_task(arg)`: activates the knotes of other kqueues watching this one.
fn kqueue_task(arg: *mut c_void) {
    // SAFETY: `kqueue_alloc` sets the task's argument to its kqueue, and `kqueue_terminate`
    // waits for the task (`taskq_del_barrier`) before the kqueue can be freed.
    let kq = unsafe { &*arg.cast::<Kqueue>() };

    knote(&kq.kq_klist, 0);
}

/// `kqueue_wakeup(kq)`: wakes the scan sleeping on the kqueue and schedules the
/// activation of the kqueues watching it.
pub fn kqueue_wakeup(kq: &Kqueue) {
    mutex_assert_locked(&kq.kq_lock, "kqueue_wakeup");

    if kq.kq_state.get() & KQ_SLEEP != 0 {
        kq.kq_state.set(kq.kq_state.get() & !KQ_SLEEP);
        wakeup(ptr::from_ref(kq));
    }
    if !klist_empty(&kq.kq_klist) {
        // Defer activation to avoid recursion.
        kq.kq_state.set(kq.kq_state.get() | KQ_TASK);
        // SAFETY: a kqueue is a pool item that `kqueue_terminate` keeps alive until the task
        // has run (`taskq_del_barrier` under KQ_TASK), so its task lives while queued.
        let task = unsafe { &*ptr::from_ref(&kq.kq_task) };
        task_add(SYSTQMP, task);
    }
}

/// `hashinit(KN_HASHSIZE, M_KEVENT, M_WAITOK, &hashmask)` over `SLIST` heads (see the
/// module's deviations): the table and its mask, or `None`.
fn knhash_alloc() -> Option<(NonNull<Knlist>, u64)> {
    let size = hashsize(KN_HASHSIZE);
    let hash = mallocarray(size, size_of::<Knlist>(), M_KEVENT, M_WAITOK)?.cast::<Knlist>();
    for i in 0..size {
        // SAFETY: `size` heads were just allocated at `hash`.
        unsafe { hash.as_ptr().add(i).write(Knlist::new()) };
    }
    Some((hash, size as u64 - 1))
}

/// `hashfree(hash, KN_HASHSIZE, M_KEVENT)` of a [`knhash_alloc`] table.
fn knhash_free(hash: NonNull<Knlist>) {
    free(
        hash.cast(),
        M_KEVENT,
        hashsize(KN_HASHSIZE) * size_of::<Knlist>(),
    );
}

/// `kqueue_expand_hash(kq)`: allocates the hash of non-descriptor knotes the first time
/// (`kq_lock` held, released around the allocation).
fn kqueue_expand_hash(kq: &Kqueue) -> Result<(), Errno> {
    mutex_assert_locked(&kq.kq_lock, "kqueue_expand_hash");

    if kq.kq_knhashmask.get() == 0 {
        mtx_leave(&kq.kq_lock);
        let hash = knhash_alloc();
        mtx_enter(&kq.kq_lock);
        let Some((hash, hashmask)) = hash else {
            return Err(Errno::ENOMEM);
        };
        if kq.kq_knhashmask.get() == 0 {
            kq.kq_knhash.set(hash.as_ptr());
            kq.kq_knhashmask.set(hashmask);
        } else {
            // Another thread has allocated the hash.
            mtx_leave(&kq.kq_lock);
            knhash_free(hash);
            mtx_enter(&kq.kq_lock);
        }
    }
    Ok(())
}

/// `kqueue_expand_list(kq, fd)`: grows the descriptor table past `fd` by `KQEXTENT`
/// slots at a time (`kq_lock` held, released around the allocation).
fn kqueue_expand_list(kq: &Kqueue, fd: i32) -> Result<(), Errno> {
    mutex_assert_locked(&kq.kq_lock, "kqueue_expand_list");

    if kq.kq_knlistsize.get() <= fd {
        let mut size = kq.kq_knlistsize.get();
        mtx_leave(&kq.kq_lock);
        while size <= fd {
            size += KQEXTENT;
        }
        let list = mallocarray(size as usize, size_of::<Knlist>(), M_KEVENT, M_WAITOK);
        mtx_enter(&kq.kq_lock);
        let Some(list) = list else {
            return Err(Errno::ENOMEM);
        };
        let list = list.cast::<Knlist>();
        if kq.kq_knlistsize.get() <= fd {
            let osize = kq.kq_knlistsize.get() as usize;
            let olist = kq.kq_knlist.get();
            for i in 0..size as usize {
                let head = if i < osize {
                    // SAFETY: the old table has `osize` heads; a singly-linked list's head
                    // is its first link only, so it moves by copy (the C's memcpy).
                    unsafe { ptr::read(olist.add(i)) }
                } else {
                    Knlist::new()
                };
                // SAFETY: `size` heads were just allocated at `list`.
                unsafe { list.as_ptr().add(i).write(head) };
            }
            kq.kq_knlist.set(list.as_ptr());
            kq.kq_knlistsize.set(size);
            mtx_leave(&kq.kq_lock);
            if let Some(olist) = NonNull::new(olist) {
                free(olist.cast(), M_KEVENT, osize * size_of::<Knlist>());
            }
            mtx_enter(&kq.kq_lock);
        } else {
            // Another thread has expanded the list.
            mtx_leave(&kq.kq_lock);
            free(list.cast(), M_KEVENT, size as usize * size_of::<Knlist>());
            mtx_enter(&kq.kq_lock);
        }
    }
    Ok(())
}

/// `knote_acquire(kn, klist, ls)`: acquire a knote, return `true` on success, `false` on
/// failure.
///
/// If we cannot acquire the knote we sleep and return `false`. The knote may be stale on
/// return in this case and the caller must restart whatever loop they are in.
///
/// If we are about to sleep and `klist` is given, the list is unlocked before sleep and
/// remains unlocked on return.
pub fn knote_acquire(kn: &Knote, klist: Option<&Klist>, ls: i32) -> bool {
    let kq = kn.kq();

    mutex_assert_locked(&kq.kq_lock, "knote_acquire");
    kassert!(kn.kn_filter().get() != EVFILT_MARKER);

    if kn.kn_status.get() & KN_PROCESSING != 0 {
        kn.kn_status.set(kn.kn_status.get() | KN_WAITING);
        if let Some(klist) = klist {
            mtx_leave(&kq.kq_lock);
            klist_unlock(klist, ls);
            // XXX Timeout resolves potential loss of wakeup.
            let _ = tsleep_nsec(ptr::from_ref(kn), 0, "kqepts", sec_to_nsec(1));
        } else {
            let _ = msleep_nsec(
                ptr::from_ref(kn),
                &kq.kq_lock,
                PNORELOCK,
                "kqepts",
                sec_to_nsec(1),
            );
        }
        // knote may be stale now
        return false;
    }
    kn.kn_status.set(kn.kn_status.get() | KN_PROCESSING);
    true
}

/// `knote_release(kn)`: release an acquired knote, clearing `KN_PROCESSING`.
pub fn knote_release(kn: &Knote) {
    mutex_assert_locked(&kn.kq().kq_lock, "knote_release");
    kassert!(kn.kn_filter().get() != EVFILT_MARKER);
    kassert!(kn.kn_status.get() & KN_PROCESSING != 0);

    if kn.kn_status.get() & KN_WAITING != 0 {
        kn.kn_status.set(kn.kn_status.get() & !KN_WAITING);
        wakeup(ptr::from_ref(kn));
    }
    kn.kn_status.set(kn.kn_status.get() & !KN_PROCESSING);
    // kn should not be accessed anymore
}

/// `knote_activate(kn)`: activate one knote.
pub fn knote_activate(kn: &Knote) {
    mutex_assert_locked(&kn.kq().kq_lock, "knote_activate");

    kn.kn_status.set(kn.kn_status.get() | KN_ACTIVE);
    if kn.kn_status.get() & (KN_QUEUED | KN_DISABLED) == 0 {
        knote_enqueue(kn);
    }
}

/// `knote(list, hint)`: walk down a list of knotes, activating them if their event has
/// triggered; takes the list's lock.
pub fn knote(list: &Klist, hint: i64) {
    let ls = klist_lock(list);
    knote_locked(list, hint);
    klist_unlock(list, ls);
}

/// `knote_locked(list, hint)`: `knote` with the list's lock held.
pub fn knote_locked(list: &Klist, hint: i64) {
    klist_assert_locked(list);

    for kn in list.kl_list.iter() {
        if filter_event(kn, hint) {
            let kq = kn.kq();
            mtx_enter(&kq.kq_lock);
            knote_activate(kn);
            mtx_leave(&kq.kq_lock);
        }
    }
}

/// `knote_remove(p, kq, plist, idx, purge)`: remove all knotes from a specified knlist: slot
/// `idx` of the hash (`hash`) or of the descriptor table. Without `purge`, the knotes of a
/// poll or select are rewired to report `EBADF`.
pub fn knote_remove(p: Option<&Proc>, kq: &Kqueue, hash: bool, idx: usize, purge: bool) {
    mutex_assert_locked(&kq.kq_lock, "knote_remove");

    // Always fetch array pointer as another thread can resize kq_knlist.
    let slot = || if hash { kq.knhash(idx) } else { kq.knlist(idx) };
    while let Some(mut kn) = slot().first() {
        kassert!(ptr::eq(kn.kq(), kq));

        if !purge {
            // Skip pending badfd knotes.
            while ptr::eq(kn.fop(), &BADFD_FILTOPS) {
                match SlistHead::<KnLink>::next(kn) {
                    Some(next) => kn = next,
                    None => return,
                }
                kassert!(ptr::eq(kn.kq(), kq));
            }
        }

        if !knote_acquire(kn, None, 0) {
            // knote_acquire() has released kq_lock.
            mtx_enter(&kq.kq_lock);
            continue;
        }
        mtx_leave(&kq.kq_lock);
        filter_detach(kn);

        // Notify poll(2) and select(2) when a monitored file descriptor is closed.
        //
        // This reuses the original knote for delivering the notification so as to avoid
        // allocating memory.
        let expired = p.is_some_and(|p| {
            ptr::eq(p.p_kq.get(), kq) && p.p_kq_serial.get() > kn.kn_udata().get() as u64
        });
        if !purge
            && kn.has_flags(__EV_POLL | __EV_SELECT)
            && !expired
            && !ptr::eq(kn.fop(), &BADFD_FILTOPS)
        {
            kassert!(kn.fop().f_flags & FILTEROP_ISFD != 0);
            if let Some(fp) = kn.kn_fp().take() {
                let _ = frele(fp, p);
            }

            kn.kn_fop.set(Some(&BADFD_FILTOPS));
            filter_event(kn, 0);
            mtx_enter(&kq.kq_lock);
            knote_activate(kn);
            knote_release(kn);
            continue;
        }

        // SAFETY: the knote is ours (KN_PROCESSING) and is not used again.
        unsafe { knote_drop(kn, p) };
        mtx_enter(&kq.kq_lock);
    }
}

/// `knote_fdclose(p, fd)`: remove all knotes referencing a specified fd.
pub fn knote_fdclose(p: &Proc, fd: i32) {
    let fdp = p.process().fd();

    // fdplock can be ignored if the file descriptor table is being freed because no other
    // thread can access the fdp.
    if fdp.fd_refcnt.get() != 0 {
        fdpassertlocked(fdp);
    }

    for kq in fdp.fd_kqlist.iter() {
        mtx_enter(&kq.kq_lock);
        if fd < kq.kq_knlistsize.get() {
            knote_remove(Some(p), kq, false, fd as usize, false);
        }
        mtx_leave(&kq.kq_lock);
    }
}

/// `knote_processexit(pr)`: handle a process exiting, including the triggering of
/// `NOTE_EXIT` notes. XXX this could be more efficient, doing a single pass down the klist
pub fn knote_processexit(pr: &Process) {
    // this needs both the ps_mtx and exclusive kqueue_ps_list_lock.
    rw_enter_write(&KQUEUE_PS_LIST_LOCK);
    mtx_enter(&pr.ps_mtx);
    knote_locked(&pr.ps_klist, i64::from(NOTE_EXIT));
    mtx_leave(&pr.ps_mtx);
    rw_exit_write(&KQUEUE_PS_LIST_LOCK);

    // remove other knotes hanging off the process
    klist_invalidate(&pr.ps_klist);
}

/// `knote_processfork(pr, pid)`: `NOTE_FORK` with the child's pid.
pub fn knote_processfork(pr: &Process, pid: Pid) {
    // this needs both the ps_mtx and exclusive kqueue_ps_list_lock.
    rw_enter_write(&KQUEUE_PS_LIST_LOCK);
    mtx_enter(&pr.ps_mtx);
    knote_locked(&pr.ps_klist, i64::from(NOTE_FORK) | i64::from(pid));
    mtx_leave(&pr.ps_mtx);
    rw_exit_write(&KQUEUE_PS_LIST_LOCK);
}

/// `knote_attach(kn)`: links the knote into its kqueue's descriptor table or hash.
pub fn knote_attach(kn: &Knote) {
    let kq = kn.kq();

    mutex_assert_locked(&kq.kq_lock, "knote_attach");
    kassert!(kn.kn_status.get() & KN_PROCESSING != 0);

    let list = if kn.fop().f_flags & FILTEROP_ISFD != 0 {
        kassert!(kq.kq_knlistsize.get() as usize > kn.kn_id().get());
        kq.knlist(kn.kn_id().get())
    } else {
        kassert!(kq.kq_knhashmask.get() != 0);
        kq.knhash(kn_hash(kn.kn_id().get() as u64, kq.kq_knhashmask.get()) as usize)
    };
    // SAFETY: a new knote is in no kqueue list, and stays allocated until `knote_drop`
    // unlinks it (`knote_detach`); under kq_lock.
    unsafe { list.insert_head(kn) };
    kq.kq_nknotes.set(kq.kq_nknotes.get() + 1);
}

/// `knote_detach(kn)`: unlinks the knote from its kqueue's descriptor table or hash.
pub fn knote_detach(kn: &Knote) {
    let kq = kn.kq();

    mutex_assert_locked(&kq.kq_lock, "knote_detach");
    kassert!(kn.kn_status.get() & KN_PROCESSING != 0);

    kq.kq_nknotes.set(kq.kq_nknotes.get() - 1);
    let list = if kn.fop().f_flags & FILTEROP_ISFD != 0 {
        kq.knlist(kn.kn_id().get())
    } else {
        kq.knhash(kn_hash(kn.kn_id().get() as u64, kq.kq_knhashmask.get()) as usize)
    };
    // SAFETY: `knote_attach` linked it into this list; under kq_lock.
    unsafe { list.remove(kn) };
}

/// `knote_drop(kn, p)`: unlinks the knote, drops its file reference and frees it. Should be
/// called at spl == 0, since we don't want to hold spl while calling `FRELE` and
/// `pool_put`.
///
/// # Safety
///
/// `kn` came from `knote_pool`, is attached to its kqueue (`knote_attach`) and detached
/// from its object, the caller holds it (`KN_PROCESSING`), and nothing uses it afterwards.
pub unsafe fn knote_drop(kn: &Knote, p: Option<&Proc>) {
    let kq = kn.kq();

    kassert!(kn.kn_filter().get() != EVFILT_MARKER);

    mtx_enter(&kq.kq_lock);
    knote_detach(kn);
    if kn.kn_status.get() & KN_QUEUED != 0 {
        knote_dequeue(kn);
    }
    if kn.kn_status.get() & KN_WAITING != 0 {
        kn.kn_status.set(kn.kn_status.get() & !KN_WAITING);
        wakeup(ptr::from_ref(kn));
    }
    mtx_leave(&kq.kq_lock);

    if kn.fop().f_flags & FILTEROP_ISFD != 0
        && let Some(fp) = kn.kn_fp().take()
    {
        let _ = frele(fp, p);
    }
    pool_put(&KNOTE_POOL, NonNull::from(kn).cast());
}

/// `knote_enqueue(kn)`: queues an active knote on its kqueue and wakes the kqueue.
pub fn knote_enqueue(kn: &Knote) {
    let kq = kn.kq();

    mutex_assert_locked(&kq.kq_lock, "knote_enqueue");
    kassert!(kn.kn_filter().get() != EVFILT_MARKER);
    kassert!(kn.kn_status.get() & KN_QUEUED == 0);

    // kqueue_check(kq): KQUEUE_DEBUG, not configured.
    // SAFETY: the knote is in no queue (not KN_QUEUED) and stays until dequeued; under
    // kq_lock.
    unsafe { kq.kq_head.insert_tail(kn) };
    kn.kn_status.set(kn.kn_status.get() | KN_QUEUED);
    kq.kq_count.set(kq.kq_count.get() + 1);
    kqueue_wakeup(kq);
}

/// `knote_dequeue(kn)`.
pub fn knote_dequeue(kn: &Knote) {
    let kq = kn.kq();

    mutex_assert_locked(&kq.kq_lock, "knote_dequeue");
    kassert!(kn.kn_filter().get() != EVFILT_MARKER);
    kassert!(kn.kn_status.get() & KN_QUEUED != 0);

    // SAFETY: a KN_QUEUED knote is on its kqueue's queue; under kq_lock.
    unsafe { kq.kq_head.remove(kn) };
    kn.kn_status.set(kn.kn_status.get() & !KN_QUEUED);
    kq.kq_count.set(kq.kq_count.get() - 1);
}

/// `knote_assign(kev, kn)`: assign parameters to the knote. The knote's object lock must be
/// held.
pub fn knote_assign(kev: &Kevent, kn: &Knote) {
    // Without FILTEROP_MPSAFE: KERNEL_ASSERT_LOCKED(), nothing without MULTIPROCESSOR.
    kn.kn_sfflags.set(kev.fflags);
    kn.kn_sdata.set(kev.data);
    kn.kn_udata().set(kev.udata);
}

/// `knote_submit(kn, kev)`: submit the knote's event for delivery. The knote's object lock
/// must be held.
pub fn knote_submit(kn: &Knote, kev: Option<&mut Kevent>) {
    // Without FILTEROP_MPSAFE: KERNEL_ASSERT_LOCKED(), nothing without MULTIPROCESSOR.
    if let Some(kev) = kev {
        *kev = kn.kn_kevent.get();
        if kn.has_flags(EV_CLEAR) {
            kn.kn_fflags().set(0);
            kn.kn_data().set(0);
        }
    }
}

/// `klist_init(klist, ops, arg)`: an empty klist locked by `ops` over `arg`.
///
/// # Safety
///
/// `arg` is what `ops` expects (for the ops of this file, the lock), and stays valid for as
/// long as the klist is used.
pub unsafe fn klist_init(klist: &Klist, ops: &'static Klistops, arg: *const c_void) {
    klist.kl_list.init();
    klist.kl_ops.set(Some(ops));
    klist.kl_arg.set(arg);
}

/// `klist_free(klist)`: the list must be empty.
pub fn klist_free(klist: &Klist) {
    kassert!(klist.kl_list.is_empty());
}

/// `klist_insert(klist, kn)`: hooks a knote on the list, taking the list's lock.
pub fn klist_insert(klist: &Klist, kn: &Knote) {
    let ls = klist_lock(klist);
    // SAFETY: a knote being attached is on no klist, and its filter's `f_detach` takes it
    // off before `knote_drop` frees it; under the list's lock.
    unsafe { klist.kl_list.insert_head(kn) };
    klist_unlock(klist, ls);
}

/// `klist_insert_locked(klist, kn)`: `klist_insert` with the list's lock held.
pub fn klist_insert_locked(klist: &Klist, kn: &Knote) {
    klist_assert_locked(klist);

    // SAFETY: as in `klist_insert`.
    unsafe { klist.kl_list.insert_head(kn) };
}

/// `klist_remove(klist, kn)`: unhooks a knote, taking the list's lock.
pub fn klist_remove(klist: &Klist, kn: &Knote) {
    let ls = klist_lock(klist);
    // SAFETY: the filter hooked the knote on this list (`klist_insert*`); under its lock.
    unsafe { klist.kl_list.remove(kn) };
    klist_unlock(klist, ls);
}

/// `klist_remove_locked(klist, kn)`: `klist_remove` with the list's lock held.
pub fn klist_remove_locked(klist: &Klist, kn: &Knote) {
    klist_assert_locked(klist);

    // SAFETY: as in `klist_remove`.
    unsafe { klist.kl_list.remove(kn) };
}

/// `klist_invalidate(list)`: detach all knotes from klist. The knotes are rewired to
/// indicate EOF.
///
/// The caller of this function must not hold any locks that can block filterops callbacks
/// that run with `KN_PROCESSING`. Otherwise this function might deadlock.
pub fn klist_invalidate(list: &Klist) {
    let p = curproc();

    // NET_ASSERT_UNLOCKED(): the network lock is not ported.

    let mut ls = klist_lock(list);
    while let Some(kn) = list.kl_list.first() {
        let kq = kn.kq();
        mtx_enter(&kq.kq_lock);
        if !knote_acquire(kn, Some(list), ls) {
            // knote_acquire() has released kq_lock and klist lock.
            ls = klist_lock(list);
            continue;
        }
        mtx_leave(&kq.kq_lock);
        klist_unlock(list, ls);
        filter_detach(kn);
        if kn.fop().f_flags & FILTEROP_ISFD != 0 {
            kn.kn_fop.set(Some(&DEAD_FILTOPS));
            filter_event(kn, 0);
            mtx_enter(&kq.kq_lock);
            knote_activate(kn);
            knote_release(kn);
            mtx_leave(&kq.kq_lock);
        } else {
            // SAFETY: the knote is ours (KN_PROCESSING), detached, and not used again.
            unsafe { knote_drop(kn, p) };
        }
        ls = klist_lock(list);
    }
    klist_unlock(list, ls);
}

/// `klist_lock(list)`: the list's lock, or the kernel lock at `splhigh`.
fn klist_lock(list: &Klist) -> i32 {
    match list.kl_ops.get() {
        // SAFETY: `kl_arg` is what `klist_init` was given for these ops (its contract).
        Some(ops) => unsafe { (ops.klo_lock)(list.kl_arg.get()) },
        None => {
            // KERNEL_LOCK(): nothing without MULTIPROCESSOR.
            splhigh()
        }
    }
}

/// `klist_unlock(list, ls)`.
fn klist_unlock(list: &Klist, ls: i32) {
    match list.kl_ops.get() {
        // SAFETY: as in `klist_lock`.
        Some(ops) => unsafe { (ops.klo_unlock)(list.kl_arg.get(), ls) },
        None => {
            splx(ls);
            // KERNEL_UNLOCK(): nothing without MULTIPROCESSOR.
        }
    }
}

/// `klist_mutex_assertlk(arg)`.
///
/// # Safety
///
/// `arg` points at a live `Mutex` (`klist_init_mutex`'s contract).
unsafe fn klist_mutex_assertlk(arg: *const c_void) {
    // SAFETY: the caller's contract.
    let mtx = unsafe { &*arg.cast::<Mutex>() };

    mutex_assert_locked(mtx, "klist_mutex_assertlk");
}

/// `klist_mutex_lock(arg)`.
///
/// # Safety
///
/// As for [`klist_mutex_assertlk`].
unsafe fn klist_mutex_lock(arg: *const c_void) -> i32 {
    // SAFETY: the caller's contract.
    let mtx = unsafe { &*arg.cast::<Mutex>() };

    mtx_enter(mtx);
    0
}

/// `klist_mutex_unlock(arg, s)`.
///
/// # Safety
///
/// As for [`klist_mutex_assertlk`].
unsafe fn klist_mutex_unlock(arg: *const c_void, _s: i32) {
    // SAFETY: the caller's contract.
    let mtx = unsafe { &*arg.cast::<Mutex>() };

    mtx_leave(mtx);
}

/// `klist_init_mutex(klist, mtx)`: a klist locked by a mutex.
///
/// # Safety
///
/// `mtx` stays valid and in place for as long as the klist is used (it is usually a member
/// of the same object, or a static).
pub unsafe fn klist_init_mutex(klist: &Klist, mtx: &Mutex) {
    // SAFETY: the caller's contract is `klist_init`'s for the mutex ops.
    unsafe { klist_init(klist, &MUTEX_KLISTOPS, ptr::from_ref(mtx).cast()) };
}

/// `klist_rwlock_assertlk(arg)`.
///
/// # Safety
///
/// `arg` points at a live `Rwlock` (`klist_init_rwlock`'s contract).
unsafe fn klist_rwlock_assertlk(arg: *const c_void) {
    // SAFETY: the caller's contract.
    let rwl = unsafe { &*arg.cast::<Rwlock>() };

    rw_assert_wrlock(rwl);
}

/// `klist_rwlock_lock(arg)`.
///
/// # Safety
///
/// As for [`klist_rwlock_assertlk`].
unsafe fn klist_rwlock_lock(arg: *const c_void) -> i32 {
    // SAFETY: the caller's contract.
    let rwl = unsafe { &*arg.cast::<Rwlock>() };

    rw_enter_write(rwl);
    0
}

/// `klist_rwlock_unlock(arg, s)`.
///
/// # Safety
///
/// As for [`klist_rwlock_assertlk`].
unsafe fn klist_rwlock_unlock(arg: *const c_void, _s: i32) {
    // SAFETY: the caller's contract.
    let rwl = unsafe { &*arg.cast::<Rwlock>() };

    rw_exit_write(rwl);
}

/// `klist_init_rwlock(klist, rwl)`: a klist locked by an rwlock (held for writing).
///
/// # Safety
///
/// `rwl` stays valid and in place for as long as the klist is used.
pub unsafe fn klist_init_rwlock(klist: &Klist, rwl: &Rwlock) {
    // SAFETY: the caller's contract is `klist_init`'s for the rwlock ops.
    unsafe { klist_init(klist, &RWLOCK_KLISTOPS, ptr::from_ref(rwl).cast()) };
}

#[cfg(test)]
pub(crate) mod tests;
