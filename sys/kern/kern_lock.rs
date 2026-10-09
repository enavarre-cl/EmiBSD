/* <LICENSES> */
/*
 * Copyright (c) 2017 Visa Hankala
 * Copyright (c) 2014 David Gwynne <dlg@openbsd.org>
 * Copyright (c) 2004 Artur Grabowski <art@openbsd.org>
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
//! Kernel lock and mutexes: `kern/kern_lock.c`.
//!
//! Upstream: sys/kern/kern_lock.c @ 3ce1f3f79392
//!
//! Status: `wip`. Milestone M4 ports the uniprocessor mutex: `__mtx_init`, `_mtx_init`,
//! `mtx_init`, `mtx_init_flags`, `mtx_enter`, `mtx_enter_try` and `mtx_leave`; M5 adds the
//! `pc_lock` producer/consumer generation lock (`pc_lock_init`, `pc_sprod_*`, `pc_mprod_*`,
//! `pc_cons_*`). M11a adds the `MULTIPROCESSOR` half: `kernel_lock` with
//! `_kernel_lock_init`, `_kernel_lock`, `_kernel_unlock` and `_kernel_lock_held`; the ticket
//! lock (`___mp_lock_init`, `__mp_lock_spin`, `__mp_lock`, `__mp_unlock`,
//! `__mp_release_all`, `__mp_acquire_count`, `__mp_lock_held`); the spinning mutex with its
//! parking lots (`mtx_init_parking`, `mtx_park`, `mtx_enter_park`, `mtx_leave_park`,
//! `mtx_cas`, `mtx_enter`, `mtx_enter_try`, `mtx_leave`); the interlocking `pc_mprod_enter`
//! and `pc_mprod_leave`. M11c adds the debugger's mutex, `db_mtx_enter`/`db_mtx_leave` (`DDB`;
//! uniprocessor too, as in the C). `mtx_print_parks` (a debugging printer of the parking lots
//! that nothing calls) is not here; `WITNESS` and `MP_LOCKDEBUG` are not configured.
//!
//! ## Deviations
//! - `membar_producer`/`membar_consumer`/`membar_exit` are `fence`s of the matching
//!   ordering; `membar_enter_after_atomic` is an `Acquire` fence and `membar_enter` a
//!   `SeqCst` one. Without `MULTIPROCESSOR`, `pc_mprod_*` are `pc_sprod_*` (the C aliases
//!   them).
//! - `KERNEL_LOCK()`/`KERNEL_UNLOCK()` are `sys::systm::kernel_lock()`/`kernel_unlock()`,
//!   which are nothing without `MULTIPROCESSOR`, as in the C's `<sys/systm.h>`.
//! - `__mp_lock_held` indexes `mpl_cpus` by `ci_cpuid` (`cpu_number()` on that CPU), where
//!   the C uses `CPU_INFO_UNIT(ci)`; both are the CPU's number on every machine OpenBSD runs.
//! - A parked mutex waiter (`struct mtx_waiter`) lives on the waiting CPU's stack as in the C;
//!   its `wait` flag is an `AtomicU32` (the C's `volatile`).

#[cfg(feature = "multiprocessor")]
use core::ptr;
#[cfg(feature = "multiprocessor")]
use core::sync::atomic::{AtomicPtr, AtomicU32};
use core::sync::atomic::{Ordering, fence};

use crate::kern::init_main::DB_ACTIVE;
#[cfg(feature = "multiprocessor")]
use crate::kern::sched_bsd::sched_assert_unlocked;
use crate::kern::subr_prf::panicstr;
#[cfg(any(feature = "diagnostic", feature = "multiprocessor"))]
use crate::machine::Machine;
#[cfg(any(feature = "diagnostic", feature = "multiprocessor"))]
use crate::machine::cpu::Cpu;
#[cfg(feature = "multiprocessor")]
use crate::machine::cpu::{CpuInfo, curcpu, intr_disable, intr_restore};
use crate::machine::intr::{IPL_NONE, splraise, splx};
#[cfg(feature = "multiprocessor")]
use crate::sys::mplock::MpLock;
#[cfg(all(feature = "diagnostic", not(feature = "multiprocessor")))]
use crate::sys::mutex::mtx_owner;
#[cfg(not(feature = "multiprocessor"))]
use crate::sys::mutex::mutex_assert_locked;
use crate::sys::mutex::{DbMutex, Mutex, mtx_curcpu, mutex_ipl};
use crate::sys::pclock::PcLock;
#[cfg(feature = "multiprocessor")]
use crate::sys::queue::{TailqEntry, TailqHead};

/// `MTX_PARKING_BITS`: log2 of the number of parking lots.
#[cfg(feature = "multiprocessor")]
const MTX_PARKING_BITS: u32 = 7;
/// `MTX_PARKING_LOTS`.
#[cfg(feature = "multiprocessor")]
const MTX_PARKING_LOTS: usize = 1 << MTX_PARKING_BITS;
/// `MTX_PARKING_MASK`.
#[cfg(feature = "multiprocessor")]
const MTX_PARKING_MASK: usize = MTX_PARKING_LOTS - 1;

/// `CPU_MIN_BUSY_CYCLES`: the first back-off of `db_mtx_enter`.
const CPU_MIN_BUSY_CYCLES: u32 = 1;

/// `struct mtx_waiter`: a CPU waiting for a contended mutex, linked into the mutex's
/// parking lot from the waiting CPU's stack.
#[cfg(feature = "multiprocessor")]
struct MtxWaiter {
    /// `mtx`: the mutex it waits for.
    mtx: *const Mutex,
    /// `wait` (volatile): cleared by the CPU that releases the mutex.
    wait: AtomicU32,
    /// `entry`.
    entry: TailqEntry<MtxWaiter>,
}

#[cfg(feature = "multiprocessor")]
crate::queue_adapter!(
    /// `TAILQ_ENTRY(mtx_waiter) entry`: a parking lot's waiters.
    MtxWaitlist: MtxWaiter, entry => TailqEntry<MtxWaiter>
);

/// `struct mtx_park`: a parking lot, one cache line apart from its neighbours.
#[cfg(feature = "multiprocessor")]
#[repr(align(64))]
struct MtxPark {
    /// `lock` (volatile): the CPU inside the lot, null when free.
    lock: AtomicPtr<CpuInfo>,
    /// `waiters`: guarded by `lock`.
    waiters: TailqHead<MtxWaitlist>,
}

// SAFETY: `waiters` is only touched between `mtx_enter_park` and `mtx_leave_park`, which
// make the lot's spinning `lock` exclusive; `lock` is atomic.
#[cfg(feature = "multiprocessor")]
unsafe impl Sync for MtxPark {}

/// `kernel_lock`: the big lock serialising whatever is not yet `MPSAFE`.
#[cfg(feature = "multiprocessor")]
pub static KERNEL_LOCK: MpLock = MpLock::new();

/// `mtx_parking[MTX_PARKING_LOTS]`.
#[cfg(feature = "multiprocessor")]
static MTX_PARKING: [MtxPark; MTX_PARKING_LOTS] = [const {
    MtxPark {
        lock: AtomicPtr::new(ptr::null_mut()),
        waiters: TailqHead::new(),
    }
}; MTX_PARKING_LOTS];

/// `_kernel_lock_init`: `KERNEL_LOCK_INIT()`, once, from `main`.
#[cfg(feature = "multiprocessor")]
pub fn _kernel_lock_init() {
    ___mp_lock_init(&KERNEL_LOCK);
    mtx_init_parking();
}

/// `_kernel_lock`: acquire the kernel lock. Intended for use in the scheduler and the lower
/// half of the kernel.
#[cfg(feature = "multiprocessor")]
pub fn _kernel_lock() {
    sched_assert_unlocked();
    __mp_lock(&KERNEL_LOCK);
}

/// `_kernel_unlock`: release the kernel lock.
#[cfg(feature = "multiprocessor")]
pub fn _kernel_unlock() {
    __mp_unlock(&KERNEL_LOCK);
}

/// `_kernel_lock_held`: this CPU holds the kernel lock (always true after a panic or in ddb).
#[cfg(feature = "multiprocessor")]
pub fn _kernel_lock_held() -> bool {
    if panicstr() || DB_ACTIVE.load(Ordering::Relaxed) {
        return true;
    }
    __mp_lock_held(&KERNEL_LOCK, curcpu())
}

/// `___mp_lock_init` (`__mp_lock_init`): nobody holds `mpl`; ticket 1 is served next.
#[cfg(feature = "multiprocessor")]
pub fn ___mp_lock_init(mpl: &MpLock) {
    for cpu in &mpl.mpl_cpus {
        cpu.mplc_ticket.store(0, Ordering::Relaxed);
        cpu.mplc_depth.store(0, Ordering::Relaxed);
    }
    mpl.mpl_users.store(0, Ordering::Relaxed);
    mpl.mpl_ticket.store(1, Ordering::Relaxed);
    // WITNESS: not configured.
}

/// `__mp_lock_spin`: waits until ticket `me` is served.
#[cfg(feature = "multiprocessor")]
#[inline]
fn __mp_lock_spin(mpl: &MpLock, me: u32) {
    let spc = Machine::ci_schedstate(curcpu());
    // MP_LOCKDEBUG (the spin-out into ddb): not configured.

    spc.spc_spinning.fetch_add(1, Ordering::Relaxed);
    while mpl.mpl_ticket.load(Ordering::Relaxed) != me {
        core::hint::spin_loop(); // CPU_BUSY_CYCLE()
    }
    spc.spc_spinning.fetch_sub(1, Ordering::Relaxed);
}

/// `__mp_lock`: takes `mpl`, recursively: a CPU that holds it only deepens its hold.
#[cfg(feature = "multiprocessor")]
pub fn __mp_lock(mpl: &MpLock) {
    let cpu = &mpl.mpl_cpus[Machine::cpu_number() as usize];

    // WITNESS_CHECKORDER: not configured.

    let s = intr_disable();
    if cpu.mplc_depth.fetch_add(1, Ordering::Relaxed) == 0 {
        // atomic_inc_int_nv
        let ticket = mpl
            .mpl_users
            .fetch_add(1, Ordering::Relaxed)
            .wrapping_add(1);
        cpu.mplc_ticket.store(ticket, Ordering::Relaxed);
    }
    // SAFETY: `s` is what `intr_disable` returned just above on this CPU.
    unsafe { intr_restore(s) };

    __mp_lock_spin(mpl, cpu.mplc_ticket.load(Ordering::Relaxed));
    fence(Ordering::Acquire); // membar_enter_after_atomic()

    // WITNESS_LOCK: not configured.
}

/// `__mp_unlock`: drops one level of this CPU's hold; the last one serves the next ticket.
#[cfg(feature = "multiprocessor")]
pub fn __mp_unlock(mpl: &MpLock) {
    let cpu = &mpl.mpl_cpus[Machine::cpu_number() as usize];

    // MP_LOCKDEBUG (the "not held lock" check): not configured. WITNESS_UNLOCK: neither.

    let s = intr_disable();
    if cpu.mplc_depth.fetch_sub(1, Ordering::Relaxed) == 1 {
        fence(Ordering::Release); // membar_exit()
        mpl.mpl_ticket.fetch_add(1, Ordering::Relaxed);
    }
    // SAFETY: `s` is what `intr_disable` returned just above on this CPU.
    unsafe { intr_restore(s) };
}

/// `__mp_release_all`: drops every level of this CPU's hold; returns how many there were.
#[cfg(feature = "multiprocessor")]
pub fn __mp_release_all(mpl: &MpLock) -> u32 {
    let cpu = &mpl.mpl_cpus[Machine::cpu_number() as usize];

    let s = intr_disable();
    let rv = cpu.mplc_depth.load(Ordering::Relaxed);
    // WITNESS_UNLOCK for each level: not configured.
    cpu.mplc_depth.store(0, Ordering::Relaxed);
    fence(Ordering::Release); // membar_exit()
    mpl.mpl_ticket.fetch_add(1, Ordering::Relaxed);
    // SAFETY: `s` is what `intr_disable` returned just above on this CPU.
    unsafe { intr_restore(s) };

    rv
}

/// `__mp_acquire_count`: takes `mpl` `count` times (what `__mp_release_all` returned).
#[cfg(feature = "multiprocessor")]
pub fn __mp_acquire_count(mpl: &MpLock, count: u32) {
    for _ in 0..count {
        __mp_lock(mpl);
    }
}

/// `__mp_lock_held`: `ci` holds `mpl`.
#[cfg(feature = "multiprocessor")]
pub fn __mp_lock_held(mpl: &MpLock, ci: &CpuInfo) -> bool {
    let cpu = &mpl.mpl_cpus[Machine::ci_cpuid(ci) as usize];

    cpu.mplc_ticket.load(Ordering::Relaxed) == mpl.mpl_ticket.load(Ordering::Relaxed)
        && cpu.mplc_depth.load(Ordering::Relaxed) > 0
}

/// `__mtx_init`: a free mutex raising to `wantipl`.
pub fn __mtx_init(mtx: &Mutex, wantipl: i32) {
    mtx.mtx_owner.store(0, Ordering::Relaxed);
    mtx.mtx_wantipl.set(wantipl);
    mtx.mtx_oldipl.set(IPL_NONE);
}

/// `_mtx_init(mtx, ipl)`: `__mtx_init` at `__MUTEX_IPL(ipl)`.
pub fn _mtx_init(mtx: &Mutex, ipl: i32) {
    __mtx_init(mtx, mutex_ipl(ipl));
}

/// `mtx_init_flags(m, ipl, name, flags)`: without `WITNESS`, `name` and `flags` are unused.
pub fn mtx_init_flags(mtx: &Mutex, ipl: i32, _name: Option<&'static str>, _flags: i32) {
    _mtx_init(mtx, ipl);
}

/// `mtx_init(m, ipl)`.
pub fn mtx_init(mtx: &Mutex, ipl: i32) {
    mtx_init_flags(mtx, ipl, None, 0);
}

/// `mtx_init_parking`: every lot free and empty.
#[cfg(feature = "multiprocessor")]
fn mtx_init_parking() {
    for p in &MTX_PARKING {
        p.lock.store(ptr::null_mut(), Ordering::Relaxed);
        p.waiters.init();
    }
}

/// `mtx_park`: the parking lot of `mtx`, from its address.
#[cfg(feature = "multiprocessor")]
fn mtx_park(mtx: &Mutex) -> &'static MtxPark {
    let mut addr = ptr::from_ref(mtx) as usize;
    addr >>= 6;
    addr ^= addr >> MTX_PARKING_BITS;
    addr &= MTX_PARKING_MASK;

    &MTX_PARKING[addr]
}

/// `mtx_enter_park`: takes the lot's spin lock with interrupts off; returns their state.
#[cfg(feature = "multiprocessor")]
fn mtx_enter_park(p: &MtxPark) -> u64 {
    let ci = ptr::from_ref(curcpu()).cast_mut();

    let m = intr_disable();
    while p
        .lock
        .compare_exchange(ptr::null_mut(), ci, Ordering::Relaxed, Ordering::Relaxed)
        .is_err()
    {
        core::hint::spin_loop(); // CPU_BUSY_CYCLE()
    }
    fence(Ordering::Acquire); // membar_enter_after_atomic()

    m
}

/// `mtx_leave_park`: releases the lot and puts the interrupt state `m` back.
#[cfg(feature = "multiprocessor")]
fn mtx_leave_park(p: &MtxPark, m: u64) {
    fence(Ordering::Release); // membar_exit()
    p.lock.store(ptr::null_mut(), Ordering::Relaxed);
    // SAFETY: `m` is what `mtx_enter_park` got from `intr_disable` on this CPU.
    unsafe { intr_restore(m) };
}

/// `mtx_cas`: `atomic_cas_ulong` on the owner word; returns the value found.
#[cfg(feature = "multiprocessor")]
#[inline]
fn mtx_cas(mtx: &Mutex, e: usize, v: usize) -> usize {
    match mtx
        .mtx_owner
        .compare_exchange(e, v, Ordering::Relaxed, Ordering::Relaxed)
    {
        Ok(found) | Err(found) => found,
    }
}

/// `mtx_enter_try`: takes the mutex if it is free; `false` (at the old level) otherwise.
#[cfg(feature = "multiprocessor")]
pub fn mtx_enter_try(mtx: &Mutex) -> bool {
    let this = mtx_curcpu();

    // Avoid deadlocks after panic or in DDB
    if panicstr() || DB_ACTIVE.load(Ordering::Relaxed) {
        return true;
    }

    let wantipl = mtx.mtx_wantipl.get();
    let s = if wantipl != IPL_NONE {
        splraise(wantipl)
    } else {
        IPL_NONE
    };

    let owner = mtx_cas(mtx, 0, this);
    if owner == 0 {
        fence(Ordering::Acquire); // membar_enter_after_atomic()
        if wantipl != IPL_NONE {
            mtx.mtx_oldipl.set(s);
        }
        #[cfg(feature = "diagnostic")]
        Machine::curcpu_mutex_level_add(1);
        // WITNESS_LOCK: not configured.
        return true;
    }

    if wantipl != IPL_NONE {
        splx(s);
    }

    #[cfg(feature = "diagnostic")]
    if owner & !1 == this {
        crate::kern::subr_prf::panic(format_args!("mtx {:p}: locking against myself", mtx));
    }

    false
}

/// `mtx_enter`: takes the mutex, raising to its level; spins a little, then parks in the
/// mutex's lot until the owner hands it over.
#[cfg(feature = "multiprocessor")]
pub fn mtx_enter(mtx: &Mutex) {
    let this = mtx_curcpu();
    let spc = Machine::ci_schedstate(curcpu());

    // Avoid deadlocks after panic or in DDB
    if panicstr() || DB_ACTIVE.load(Ordering::Relaxed) {
        return;
    }

    // WITNESS_CHECKORDER: not configured.

    let wantipl = mtx.mtx_wantipl.get();
    let s = if wantipl != IPL_NONE {
        splraise(wantipl)
    } else {
        IPL_NONE
    };

    let mut owner = mtx_cas(mtx, 0, this);
    if owner != 0 {
        #[cfg(feature = "diagnostic")]
        if owner & !1 == this {
            crate::kern::subr_prf::panic(format_args!("mtx {:p}: locking against myself", mtx));
        }

        // we're going to have to spin for it now
        spc.spc_spinning.fetch_add(1, Ordering::Relaxed);

        let mut spinlocked = false;
        for _ in 0..40 {
            if owner & 1 != 0 {
                // don't spin if cpus are already parked
                break;
            }
            core::hint::spin_loop(); // CPU_BUSY_CYCLE()
            owner = mtx.mtx_owner.load(Ordering::Relaxed);
            if owner == 0 {
                owner = mtx_cas(mtx, 0, this);
                if owner == 0 {
                    spinlocked = true;
                    break;
                }
            }
        }

        if !spinlocked {
            mtx_enter_slow(mtx, this, owner);
        }
        spc.spc_spinning.fetch_sub(1, Ordering::Relaxed);
    }

    // locked:
    fence(Ordering::Acquire); // membar_enter_after_atomic()
    if wantipl != IPL_NONE {
        mtx.mtx_oldipl.set(s);
    }
    #[cfg(feature = "diagnostic")]
    Machine::curcpu_mutex_level_add(1);
    // WITNESS_LOCK: not configured.
}

/// The really slow path of `mtx_enter`: publish this CPU in the mutex's parking lot, mark the
/// mutex as having waiters (bit 0 of the owner) and wait to be woken, until the mutex is
/// taken (with bit 0 kept set, since others may still be parked).
#[cfg(feature = "multiprocessor")]
fn mtx_enter_slow(mtx: &Mutex, this: usize, mut owner: usize) {
    let p = mtx_park(mtx);

    // publish our existence in the parking lot
    let w = MtxWaiter {
        mtx: ptr::from_ref(mtx),
        wait: AtomicU32::new(0),
        entry: TailqEntry::new(),
    };
    let m = mtx_enter_park(p);
    // SAFETY: the lot is locked; `w` is in no queue and stays in place on this stack until
    // the removal below, which happens before this function returns.
    unsafe { p.waiters.insert_tail(&w) };
    mtx_leave_park(p, m);

    loop {
        w.wait.store(1, Ordering::Relaxed);
        // ensure wait is visible before attempting the cas
        fence(Ordering::SeqCst); // membar_enter(): StoreStore | StoreLoad
        let o = mtx_cas(mtx, owner, owner | 1);
        if o == owner {
            while w.wait.load(Ordering::Relaxed) != 0 {
                core::hint::spin_loop(); // CPU_BUSY_CYCLE()
                // MP_LOCKDEBUG (the spin-out into ddb): not configured.
            }
            fence(Ordering::Acquire); // membar_consumer()
        } else if o != 0 {
            owner = o;
            continue;
        }

        owner = mtx_cas(mtx, 0, this | 1);
        if owner == 0 {
            break;
        }
    }

    let m = mtx_enter_park(p);
    // SAFETY: the lot is locked and `w` was linked into it above.
    unsafe { p.waiters.remove(&w) };
    mtx_leave_park(p, m);
}

/// `mtx_leave`: releases the mutex; if CPUs are parked on it, wakes the first one waiting
/// for this mutex, then restores the level it was entered at.
#[cfg(feature = "multiprocessor")]
pub fn mtx_leave(mtx: &Mutex) {
    let this = mtx_curcpu();

    // Avoid deadlocks after panic or in DDB
    if panicstr() || DB_ACTIVE.load(Ordering::Relaxed) {
        return;
    }

    // WITNESS_UNLOCK: not configured.

    #[cfg(feature = "diagnostic")]
    Machine::curcpu_mutex_level_add(-1);

    let s = mtx.mtx_oldipl.get();
    let wantipl = mtx.mtx_wantipl.get();
    fence(Ordering::Release); // membar_exit_before_atomic()
    let owner = mtx_cas(mtx, this, 0);
    if owner != this {
        #[cfg(feature = "diagnostic")]
        if owner & !1 != this {
            crate::kern::subr_prf::panic(format_args!("mtx {:p}: not held", mtx));
        }

        let p = mtx_park(mtx);
        let m = mtx_enter_park(p);
        mtx.mtx_owner.store(0, Ordering::Relaxed);
        fence(Ordering::Release); // membar_producer()
        for w in p.waiters.iter() {
            if ptr::eq(w.mtx, mtx) {
                w.wait.store(0, Ordering::Relaxed);
                break;
            }
        }
        mtx_leave_park(p, m);
    }

    if wantipl != IPL_NONE {
        splx(s);
    }
}

#[cfg(not(feature = "multiprocessor"))]
/// `mtx_enter`: takes the mutex, raising to its level.
pub fn mtx_enter(mtx: &Mutex) {
    let this = mtx_curcpu();

    // Avoid deadlocks after panic or in DDB
    if panicstr() || DB_ACTIVE.load(Ordering::Relaxed) {
        return;
    }

    // WITNESS_CHECKORDER: not configured.
    #[cfg(feature = "diagnostic")]
    if mtx_owner(mtx) == this {
        crate::kern::subr_prf::panic(format_args!("mtx {:p}: locking against myself", mtx));
    }

    if mtx.mtx_wantipl.get() != IPL_NONE {
        mtx.mtx_oldipl.set(splraise(mtx.mtx_wantipl.get()));
    }

    mtx.mtx_owner.store(this, Ordering::Relaxed);
    #[cfg(feature = "diagnostic")]
    Machine::curcpu_mutex_level_add(1);
    // WITNESS_LOCK: not configured.
}

#[cfg(not(feature = "multiprocessor"))]
/// `mtx_enter_try`: on a uniprocessor the mutex is always free (or the kernel is past caring).
pub fn mtx_enter_try(mtx: &Mutex) -> bool {
    mtx_enter(mtx);
    true
}

#[cfg(not(feature = "multiprocessor"))]
/// `mtx_leave`: releases the mutex and restores the level it was entered at.
pub fn mtx_leave(mtx: &Mutex) {
    // Avoid deadlocks after panic or in DDB
    if panicstr() || DB_ACTIVE.load(Ordering::Relaxed) {
        return;
    }

    mutex_assert_locked(mtx, "mtx_leave");
    // WITNESS_UNLOCK: not configured.

    #[cfg(feature = "diagnostic")]
    Machine::curcpu_mutex_level_add(-1);

    let s = mtx.mtx_oldipl.get();
    mtx.mtx_owner.store(0, Ordering::Relaxed);
    if mtx.mtx_wantipl.get() != IPL_NONE {
        splx(s);
    }
}

/// `CPU_MAX_BUSY_CYCLES`: `ncpusfound`, the longest back-off of `db_mtx_enter`.
fn cpu_max_busy_cycles() -> u32 {
    u32::try_from(crate::kern::init_main::NCPUSFOUND.load(Ordering::Relaxed)).unwrap_or(1)
}

/// `db_mtx_enter`: takes the debugger's mutex with interrupts off, spinning with exponential
/// back-off while another CPU holds it. Unlike `mtx_enter` it works while `db_active`.
pub fn db_mtx_enter(mtx: &DbMutex) {
    let ci = mtx_curcpu();
    let mut ncycle = CPU_MIN_BUSY_CYCLES;

    #[cfg(feature = "diagnostic")]
    if mtx.mtx_owner.load(Ordering::Relaxed) == ci {
        crate::kern::subr_prf::panic(format_args!(
            "db_mtx_enter: mtx {:p}: locking against myself",
            mtx
        ));
    }

    let s = crate::machine::cpu::intr_disable();
    loop {
        // Avoid unconditional atomic operation to prevent cache line contention.
        let owner = mtx.mtx_owner.load(Ordering::Relaxed);
        if owner == 0 {
            if mtx
                .mtx_owner
                .compare_exchange(0, ci, Ordering::Relaxed, Ordering::Relaxed)
                .is_ok()
            {
                break;
            }
            // Busy loop with exponential backoff.
            for _ in 0..ncycle {
                core::hint::spin_loop(); // CPU_BUSY_CYCLE()
            }
            if ncycle < cpu_max_busy_cycles() {
                ncycle += ncycle;
            }
        }
    }
    fence(Ordering::Acquire); // membar_enter_after_atomic()

    mtx.mtx_intr_state.set(s);

    #[cfg(feature = "diagnostic")]
    Machine::curcpu_mutex_level_add(1);
}

/// `db_mtx_leave`: releases the debugger's mutex and restores this CPU's interrupt state.
pub fn db_mtx_leave(mtx: &DbMutex) {
    #[cfg(feature = "diagnostic")]
    {
        if mtx.mtx_owner.load(Ordering::Relaxed) != mtx_curcpu() {
            crate::kern::subr_prf::panic(format_args!(
                "db_mtx_leave: mtx {:p}: not owned by this CPU",
                mtx
            ));
        }
        Machine::curcpu_mutex_level_add(-1);
    }

    let s = mtx.mtx_intr_state.get();
    #[cfg(feature = "multiprocessor")]
    fence(Ordering::Release); // membar_exit()
    mtx.mtx_owner.store(0, Ordering::Relaxed);
    // SAFETY: `s` is what `intr_disable` returned on this CPU in `db_mtx_enter`; only the
    // owner (this CPU) writes `mtx_intr_state`.
    unsafe { crate::machine::cpu::intr_restore(s) };
}

/// `pc_lock_init`.
pub fn pc_lock_init(pcl: &PcLock) {
    pcl.pcl_gen.store(0, Ordering::Relaxed);
}

/// `pc_sprod_enter`: a single (non-interlocking) producer enters; returns the generation.
pub fn pc_sprod_enter(pcl: &PcLock) -> u32 {
    let generation = pcl.pcl_gen.load(Ordering::Relaxed).wrapping_add(1);
    pcl.pcl_gen.store(generation, Ordering::Relaxed);
    fence(Ordering::Release); // membar_producer()

    generation
}

/// `pc_sprod_leave`.
pub fn pc_sprod_leave(pcl: &PcLock, generation: u32) {
    fence(Ordering::Release); // membar_producer()
    pcl.pcl_gen
        .store(generation.wrapping_add(1), Ordering::Relaxed);
}

/// `pc_mprod_enter`: multiple (interlocking) producers: waits for an even generation and
/// makes it odd with a compare-and-swap; returns the generation entered.
#[cfg(feature = "multiprocessor")]
pub fn pc_mprod_enter(pcl: &PcLock) -> u32 {
    let mut generation = pcl.pcl_gen.load(Ordering::Relaxed);
    let next = loop {
        while generation & 1 != 0 {
            core::hint::spin_loop(); // CPU_BUSY_CYCLE()
            generation = pcl.pcl_gen.load(Ordering::Relaxed);
        }

        let next = generation.wrapping_add(1);
        match pcl
            .pcl_gen
            .compare_exchange(generation, next, Ordering::Relaxed, Ordering::Relaxed)
        {
            Ok(_) => break next,
            Err(found) => {
                core::hint::spin_loop(); // CPU_BUSY_CYCLE()
                generation = found;
            }
        }
    };

    fence(Ordering::Acquire); // membar_enter_after_atomic()
    next
}

/// `pc_mprod_leave`.
#[cfg(feature = "multiprocessor")]
pub fn pc_mprod_leave(pcl: &PcLock, generation: u32) {
    fence(Ordering::Release); // membar_exit()
    pcl.pcl_gen
        .store(generation.wrapping_add(1), Ordering::Relaxed);
}

/// `pc_mprod_enter`: without `MULTIPROCESSOR` the single-producer entry (the C's alias).
#[cfg(not(feature = "multiprocessor"))]
pub fn pc_mprod_enter(pcl: &PcLock) -> u32 {
    pc_sprod_enter(pcl)
}

/// `pc_mprod_leave`: without `MULTIPROCESSOR` the single-producer exit (the C's alias).
#[cfg(not(feature = "multiprocessor"))]
pub fn pc_mprod_leave(pcl: &PcLock, generation: u32) {
    pc_sprod_leave(pcl, generation)
}

/// `pc_cons_enter`: a consumer waits for a quiescent generation and records it in `genp`.
pub fn pc_cons_enter(pcl: &PcLock, genp: &mut u32) {
    let mut generation = pcl.pcl_gen.load(Ordering::Relaxed);
    while generation & 1 != 0 {
        core::hint::spin_loop(); // CPU_BUSY_CYCLE()
        generation = pcl.pcl_gen.load(Ordering::Relaxed);
    }

    fence(Ordering::Acquire); // membar_consumer()
    *genp = generation;
}

/// `pc_cons_leave`: `false` if the read was consistent; `true` (with `genp` updated) if a
/// producer intervened and the consumer must retry.
#[must_use]
pub fn pc_cons_leave(pcl: &PcLock, genp: &mut u32) -> bool {
    fence(Ordering::Acquire); // membar_consumer()

    let mut generation = pcl.pcl_gen.load(Ordering::Relaxed);
    if generation & 1 != 0 {
        loop {
            core::hint::spin_loop(); // CPU_BUSY_CYCLE()
            generation = pcl.pcl_gen.load(Ordering::Relaxed);
            if generation & 1 == 0 {
                break;
            }
        }
    } else if generation == *genp {
        return false;
    }

    *genp = generation;
    true
}
/* </CODE> */

/* <TESTS> */
#[cfg(test)]
mod tests {
    use super::*;
    use crate::machine::intr::IPL_HIGH;
    use crate::sys::mutex::mtx_owner;

    #[test]
    fn pc_lock_generations() {
        let pcl = PcLock::new();
        let mut g = 0;
        pc_cons_enter(&pcl, &mut g);
        assert_eq!(g, 0);
        assert!(!pc_cons_leave(&pcl, &mut g), "nothing changed");
        let generation = pc_sprod_enter(&pcl);
        assert_eq!(generation, 1);
        pc_sprod_leave(&pcl, generation);
        assert_eq!(pcl.pcl_gen.load(Ordering::Relaxed), 2);
        assert!(pc_cons_leave(&pcl, &mut g), "a producer intervened");
        assert_eq!(g, 2);
        let generation = pc_mprod_enter(&pcl);
        pc_mprod_leave(&pcl, generation);
        pc_lock_init(&pcl);
        assert_eq!(pcl.pcl_gen.load(Ordering::Relaxed), 0);
    }

    #[test]
    fn enter_and_leave_track_the_owner() {
        static M: Mutex = Mutex::new(IPL_HIGH);
        assert_eq!(mtx_owner(&M), 0);
        mtx_enter(&M);
        assert_eq!(mtx_owner(&M), mtx_curcpu());
        mtx_leave(&M);
        assert_eq!(mtx_owner(&M), 0);
        assert!(mtx_enter_try(&M));
        mtx_leave(&M);
        assert_eq!(mtx_owner(&M), 0);
    }

    #[test]
    fn db_mutex_tracks_the_owner() {
        static M: DbMutex = DbMutex::new();
        db_mtx_enter(&M);
        assert_eq!(M.mtx_owner.load(Ordering::Relaxed), mtx_curcpu());
        db_mtx_leave(&M);
        assert_eq!(M.mtx_owner.load(Ordering::Relaxed), 0);
    }

    #[test]
    fn init_resets() {
        let m = Mutex::new(3);
        m.mtx_owner.store(42, Ordering::Relaxed);
        mtx_init(&m, 5);
        assert_eq!(mtx_owner(&m), 0);
        assert_eq!(m.mtx_wantipl.get(), 5);
    }
}
/* </TESTS> */
