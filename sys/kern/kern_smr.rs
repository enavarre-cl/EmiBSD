/*	$OpenBSD: kern_smr.c,v 1.18 2025/07/28 05:25:44 dlg Exp $	*/
/* <LICENSES> */
/*
 * Copyright (c) 2019-2020 Visa Hankala
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
//! Safe memory reclamation: `kern/kern_smr.c`, see `smr_call(9)`. A CPU passes a quiescent
//! state each time it switches threads or idles (`smr_idle`); the SMR thread runs the
//! deferred calls once a grace period has made every CPU pass one (`smr_grace_wait`).
//!
//! Upstream: sys/kern/kern_smr.c @ 3ce1f3f79392
//!
//! Status: `ported` (M11a): `smr_cpu_is_idle`, `smr_startup`, `smr_startup_thread`,
//! `smr_thread`, `smr_grace_wait`, `smr_wakeup`, `smr_read_enter`, `smr_read_leave`,
//! `smr_dispatch`, `smr_idle`, `smr_call_impl` and `smr_barrier_impl`. The types and the
//! `smr_call`/`smr_barrier`/`smr_flush` wrappers are in `sys/smr.rs`.
//!
//! ## Deviations
//! - `WITNESS` is not configured: `smr_lock_obj`, `smr_lock_type` and the
//!   `WITNESS_INIT`/`WITNESS_CHECKORDER`/`WITNESS_LOCK`/`WITNESS_UNLOCK` calls are absent;
//!   dt(4) is not configured either (the `TRACEPOINT`s).
//! - `smr_expedite`, `smr_ndeferred` and `smr_grace_period` are atomics (`smr_lock`
//!   protects the first two as in C; the third is the C's `READ_ONCE`/`WRITE_ONCE`);
//!   `smr_deferred` is a simple queue made `Sync`, changed under `smr_lock`.
//! - `smr_call_impl` takes a `&'static SmrEntry`: the entry must stay in place until its
//!   call ran. `smr_barrier_impl`'s entry lives on its stack frame, which waits for the
//!   call, and is lent as `'static` for that long.

use core::ffi::c_void;
use core::ptr;
use core::sync::atomic::{AtomicU8, AtomicU32, Ordering, fence};

use crate::kern::init_main::DB_ACTIVE;
use crate::kern::kern_kthread::kthread_create;
use crate::kern::kern_lock::{mtx_enter, mtx_leave};
#[cfg(feature = "multiprocessor")]
use crate::kern::kern_sched::{sched_peg_curproc, sched_unpeg_curproc};
use crate::kern::kern_synch::{cond_init, cond_signal_handler, cond_wait, msleep_nsec, wakeup};
use crate::kern::kern_tc::getmicrouptime;
use crate::kern::kern_time::ratecheck;
use crate::kern::kern_timeout::{timeout_add_msec, timeout_set};
use crate::kern::subr_prf::{panic, panicstr, printf};
use crate::machine::Machine;
use crate::machine::cpu::{Cpu, CpuInfo, curcpu};
#[cfg(feature = "multiprocessor")]
use crate::machine::cpu::{cpu_info_foreach, cpu_is_running};
use crate::machine::intr::{IPL_HIGH, splhigh, splx};
use crate::sys::mutex::Mutex;
use crate::sys::param::PVM;
use crate::sys::proc::Cond;
use crate::sys::queue::SimpleqHead;
use crate::sys::sched::SchedstatePercpu;
use crate::sys::smr::{SmrEntry, SmrEntryList, smr_assert_noncritical, smr_init};
use crate::sys::systm::{INFSLP, kernel_assert_locked, kernel_unlock};
use crate::sys::time::{Timeval, msec_to_nsec, timersub};
use crate::sys::timeout::Timeout;

/// `SMR_PAUSE`: pause between rounds in msec.
const SMR_PAUSE: u64 = 100;

/// `struct smr_entry_list smr_deferred`'s head, made `Sync`: changed under `smr_lock`.
struct SmrDeferred(SimpleqHead<SmrEntryList>);
// SAFETY: see the type's doc.
unsafe impl Sync for SmrDeferred {}

/// `smr_lock`.
pub static SMR_LOCK: Mutex = Mutex::new(IPL_HIGH);
/// `smr_deferred`: the calls every CPU dispatched, waiting for a grace period.
static SMR_DEFERRED: SmrDeferred = SmrDeferred(SimpleqHead::new());
/// `smr_wakeup_tmo`.
static SMR_WAKEUP_TMO: Timeout = Timeout::new(smr_wakeup, ptr::null_mut());
/// `smr_expedite`. Protected by: `smr_lock`.
static SMR_EXPEDITE: AtomicU32 = AtomicU32::new(0);
/// `smr_ndeferred`: the length of `smr_deferred`, also the SMR thread's wakeup channel.
/// Protected by: `smr_lock`.
static SMR_NDEFERRED: AtomicU32 = AtomicU32::new(0);
/// `smr_grace_period`: the current grace period, bumped by `smr_grace_wait`.
static SMR_GRACE_PERIOD: AtomicU8 = AtomicU8::new(0);

/// `smr_cpu_is_idle`: `ci` runs its idle thread.
#[inline]
fn smr_cpu_is_idle(ci: &CpuInfo) -> bool {
    ptr::eq(
        Machine::ci_curproc(ci),
        Machine::ci_schedstate(ci).spc_idleproc.get(),
    )
}

/// `smr_startup`: `main` sets the subsystem up, before the scheduler runs.
pub fn smr_startup() {
    SMR_DEFERRED.0.init();
    // WITNESS_INIT: not configured.
    timeout_set(&SMR_WAKEUP_TMO, smr_wakeup, ptr::null_mut());
}

/// `smr_startup_thread`: `main` starts the SMR thread once every CPU partakes in scheduling.
pub fn smr_startup_thread() {
    if kthread_create(smr_thread, ptr::null_mut(), b"smr").is_err() {
        panic(format_args!("could not create smr thread"));
    }
}

/// `smr_logintvl`: how often a slow dispatch is reported.
const SMR_LOGINTVL: Timeval = Timeval::new(300, 0);

/// `smr_thread`: collects the dispatched calls, waits for a grace period and runs them. It
/// runs without the kernel lock.
pub fn smr_thread(_arg: *mut c_void) {
    let mut loglast = Timeval::new(0, 0);
    let deferred: SimpleqHead<SmrEntryList> = SimpleqHead::new();

    kernel_assert_locked();
    kernel_unlock();

    deferred.init();

    loop {
        mtx_enter(&SMR_LOCK);
        if SMR_NDEFERRED.load(Ordering::Relaxed) == 0 {
            while SMR_NDEFERRED.load(Ordering::Relaxed) == 0 {
                let _ = msleep_nsec(
                    ptr::from_ref(&SMR_NDEFERRED),
                    &SMR_LOCK,
                    PVM,
                    "bored",
                    INFSLP,
                );
            }
        } else if SMR_EXPEDITE.load(Ordering::Relaxed) == 0 {
            let _ = msleep_nsec(
                ptr::from_ref(&SMR_NDEFERRED),
                &SMR_LOCK,
                PVM,
                "pause",
                msec_to_nsec(SMR_PAUSE),
            );
        }

        // SAFETY: `deferred` stays in place on this stack frame for the thread's life;
        // `smr_deferred` is changed under `smr_lock`, which is held.
        unsafe { deferred.concat(&SMR_DEFERRED.0) };
        SMR_NDEFERRED.store(0, Ordering::Relaxed);
        SMR_EXPEDITE.store(0, Ordering::Relaxed);
        mtx_leave(&SMR_LOCK);

        let start = getmicrouptime();

        smr_grace_wait();

        // WITNESS_CHECKORDER, WITNESS_LOCK: not configured.

        let mut count: u64 = 0;
        while let Some(smr) = deferred.first() {
            // SAFETY: `smr` is the first entry of this thread's private queue.
            unsafe { deferred.remove_head() };
            // TRACEPOINT(smr, called, ...): dt(4), not configured.
            // The call may free the entry: read it first.
            let (func, arg) = (smr.smr_func.get(), smr.smr_arg.get());
            let Some(func) = func else {
                panic(format_args!("smr_thread: entry without a function"));
            };
            func(arg);
            count += 1;
        }

        // WITNESS_UNLOCK: not configured.

        let end = getmicrouptime();
        let elapsed = timersub(&end, &start);
        if elapsed.tv_sec >= 2 && ratecheck(&mut loglast, &SMR_LOGINTVL) {
            printf(format_args!(
                "smr: dispatch took {}.{:06}s\n",
                elapsed.tv_sec, elapsed.tv_usec
            ));
        }
        // TRACEPOINT(smr, thread, TIMEVAL_TO_NSEC(&elapsed), count): dt(4).
        let _ = count;
    }
}

/// `smr_grace_wait`: announce next grace period and wait until all CPUs have entered it by
/// crossing quiescent state. Without `MULTIPROCESSOR` the caller's own switch is enough.
pub fn smr_grace_wait() {
    #[cfg(feature = "multiprocessor")]
    {
        let smrgp = SMR_GRACE_PERIOD.load(Ordering::Relaxed).wrapping_add(1);
        SMR_GRACE_PERIOD.store(smrgp, Ordering::Relaxed);

        Machine::ci_schedstate(curcpu())
            .spc_smrgp
            .store(smrgp, Ordering::Relaxed);

        cpu_info_foreach(&mut |ci| {
            if !cpu_is_running(ci) {
                return;
            }
            let spc = Machine::ci_schedstate(ci);
            if spc.spc_smrgp.load(Ordering::Relaxed) == smrgp {
                return;
            }
            sched_peg_curproc(ci);
            crate::kassert!(spc.spc_smrgp.load(Ordering::Relaxed) == smrgp);
        });
        sched_unpeg_curproc();
    }
}

/// `smr_wakeup`: the timeout that ends the SMR thread's pause.
pub fn smr_wakeup(_arg: *mut c_void) {
    // TRACEPOINT(smr, wakeup, NULL): dt(4), not configured.
    wakeup(ptr::from_ref(&SMR_NDEFERRED));
}

/// `smr_read_enter`: starts a read section (counted under `DIAGNOSTIC`).
pub fn smr_read_enter() {
    #[cfg(feature = "diagnostic")]
    {
        let spc = Machine::ci_schedstate(curcpu());
        spc.spc_smrdepth.set(spc.spc_smrdepth.get() + 1);
    }
}

/// `smr_read_leave`: ends a read section.
pub fn smr_read_leave() {
    #[cfg(feature = "diagnostic")]
    {
        let spc = Machine::ci_schedstate(curcpu());
        crate::kassert!(spc.spc_smrdepth.get() > 0);
        spc.spc_smrdepth.set(spc.spc_smrdepth.get() - 1);
    }
}

/// `smr_dispatch`: move SMR entries from the local queue to the system-wide queue.
pub fn smr_dispatch(spc: &SchedstatePercpu) {
    let mut wake = false;

    mtx_enter(&SMR_LOCK);
    if SMR_NDEFERRED.load(Ordering::Relaxed) == 0 {
        wake = true;
    }
    // SAFETY: both heads are statics (`spc` is a CPU's), changed under `smr_lock` (held)
    // and, for the CPU's own queue, at splhigh, which `smr_lock` raises to.
    unsafe { SMR_DEFERRED.0.concat(&spc.spc_deferred) };
    SMR_NDEFERRED.fetch_add(spc.spc_ndeferred.get(), Ordering::Relaxed);
    spc.spc_ndeferred.set(0);
    SMR_EXPEDITE.fetch_or(u32::from(spc.spc_smrexpedite.get()), Ordering::Relaxed);
    spc.spc_smrexpedite.set(0);
    let expedite = SMR_EXPEDITE.load(Ordering::Relaxed);
    mtx_leave(&SMR_LOCK);

    if expedite != 0 {
        smr_wakeup(ptr::null_mut());
    } else if wake {
        let _ = timeout_add_msec(&SMR_WAKEUP_TMO, SMR_PAUSE);
    }
}

/// `smr_idle`: signal that the current CPU is in quiescent state.
pub fn smr_idle() {
    let spc = Machine::ci_schedstate(curcpu());

    smr_assert_noncritical();

    if spc.spc_ndeferred.get() > 0 {
        smr_dispatch(spc);
    }

    // Update this CPU's view of the system's grace period. The update must become visible
    // after any preceding reads of SMR-protected data.
    let smrgp = SMR_GRACE_PERIOD.load(Ordering::Relaxed);
    if spc.spc_smrgp.load(Ordering::Relaxed) != smrgp {
        fence(Ordering::Release); // membar_exit()
        spc.spc_smrgp.store(smrgp, Ordering::Relaxed);
    }
}

/// `smr_call_impl`: queues `func(arg)` on this CPU, to run after the next grace period
/// (sooner with `expedite`). `smr` must be idle (`smr_init`, or its previous call ran).
pub fn smr_call_impl(
    smr: &'static SmrEntry,
    func: fn(*mut c_void),
    arg: *mut c_void,
    expedite: bool,
) {
    let ci = curcpu();
    let spc = Machine::ci_schedstate(ci);

    crate::kassert!(smr.smr_func.get().is_none());

    smr.smr_func.set(Some(func));
    smr.smr_arg.set(arg);

    let s = splhigh();
    // SAFETY: `smr` is idle, so on no queue, and `'static`; the CPU's queue is changed
    // at splhigh by this CPU only.
    unsafe { spc.spc_deferred.insert_tail(smr) };
    spc.spc_ndeferred.set(spc.spc_ndeferred.get() + 1);
    spc.spc_smrexpedite
        .set(spc.spc_smrexpedite.get() | u8::from(expedite));
    splx(s);
    // TRACEPOINT(smr, call, ...): dt(4), not configured.

    // If this call was made from an interrupt context that preempted idle state, dispatch
    // the local queue to the shared queue immediately. The entries would linger in the
    // local queue long if the CPU went to sleep without calling smr_idle().
    if smr_cpu_is_idle(ci) {
        smr_dispatch(spc);
    }
}

/// `smr_barrier_impl`: waits for a grace period (expedited with `expedite`).
pub fn smr_barrier_impl(expedite: bool) {
    let c = Cond::new();
    let smr = SmrEntry::new();

    if panicstr() || DB_ACTIVE.load(Ordering::Relaxed) {
        return;
    }

    // WITNESS_CHECKORDER: not configured. TRACEPOINT(smr, barrier_enter, ...): dt(4).
    cond_init(&c);
    smr_init(&smr);
    // SAFETY: `smr` and `c` stay on this frame until `cond_wait` returns, which is after
    // the SMR thread took `smr` off its queue and called `cond_signal_handler`, the last use
    // of either: for that long the entry may be treated as `'static`.
    let smr_static: &'static SmrEntry = unsafe { &*ptr::from_ref(&smr) };
    smr_call_impl(
        smr_static,
        cond_signal_handler,
        ptr::from_ref(&c).cast_mut().cast(),
        expedite,
    );
    cond_wait(&c, "smrbar");
    // TRACEPOINT(smr, barrier_exit, ...): dt(4), not configured.
}
/* </CODE> */

/* <TESTS> */
#[cfg(test)]
mod tests {
    use super::*;

    /// The steps touch the global queue and the host CPU's SMR members, so they run in order
    /// in one test.
    #[test]
    fn dispatch_moves_the_cpu_queue_and_idle_tracks_the_grace_period() {
        static ENTRY: SmrEntry = SmrEntry::new();
        let spc = Machine::ci_schedstate(curcpu());

        // A CPU queue of one call, dispatched while the thread is not bored (no wakeup or
        // timeout to arm).
        SMR_NDEFERRED.store(1, Ordering::Relaxed);
        smr_init(&ENTRY);
        ENTRY.smr_func.set(Some(|_| {}));
        // SAFETY: `ENTRY` is static and in no queue.
        unsafe { spc.spc_deferred.insert_tail(&ENTRY) };
        spc.spc_ndeferred.set(1);
        smr_dispatch(spc);
        assert!(spc.spc_deferred.is_empty());
        assert_eq!(spc.spc_ndeferred.get(), 0);
        assert_eq!(SMR_NDEFERRED.load(Ordering::Relaxed), 2);
        assert!(SMR_DEFERRED.0.first().is_some_and(|e| ptr::eq(e, &ENTRY)));
        SMR_DEFERRED.0.init();
        SMR_NDEFERRED.store(0, Ordering::Relaxed);

        // smr_idle publishes the system's grace period as this CPU's view.
        SMR_GRACE_PERIOD.store(5, Ordering::Relaxed);
        smr_idle();
        assert_eq!(spc.spc_smrgp.load(Ordering::Relaxed), 5);
    }
}
/* </TESTS> */
