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
 * Copyright (c) 2002, 2003 Artur Grabowski <art@openbsd.org>
 * Copyright (c) 2011 Thordur Bjornsson <thib@secnorth.net>
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

/*-
 * Copyright (c) 2008 The NetBSD Foundation, Inc.
 * All rights reserved.
 *
 * This code is derived from software contributed to The NetBSD Foundation
 * by Andrew Doran.
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
 * THIS SOFTWARE IS PROVIDED BY THE NETBSD FOUNDATION, INC. AND CONTRIBUTORS
 * ``AS IS'' AND ANY EXPRESS OR IMPLIED WARRANTIES, INCLUDING, BUT NOT LIMITED
 * TO, THE IMPLIED WARRANTIES OF MERCHANTABILITY AND FITNESS FOR A PARTICULAR
 * PURPOSE ARE DISCLAIMED.  IN NO EVENT SHALL THE FOUNDATION OR CONTRIBUTORS
 * BE LIABLE FOR ANY DIRECT, INDIRECT, INCIDENTAL, SPECIAL, EXEMPLARY, OR
 * CONSEQUENTIAL DAMAGES (INCLUDING, BUT NOT LIMITED TO, PROCUREMENT OF
 * SUBSTITUTE GOODS OR SERVICES; LOSS OF USE, DATA, OR PROFITS; OR BUSINESS
 * INTERRUPTION) HOWEVER CAUSED AND ON ANY THEORY OF LIABILITY, WHETHER IN
 * CONTRACT, STRICT LIABILITY, OR TORT (INCLUDING NEGLIGENCE OR OTHERWISE)
 * ARISING IN ANY WAY OUT OF THE USE OF THIS SOFTWARE, EVEN IF ADVISED OF THE
 * POSSIBILITY OF SUCH DAMAGE.
 */
/* </LICENSES> */

/* <CODE> */
//! `kern_rwlock.c`: the read/write lock, and the allocated, reference-counted rwlocks
//! (`rw_obj_*`, the NetBSD Foundation block).
//!
//! Upstream: sys/kern/kern_rwlock.c @ 3ce1f3f79392
//!
//! Status: `ported` (M7a; M11a the `MULTIPROCESSOR` spin of `rw_do_enter_write`).
//!
//! ## Deviations
//! - `rw_cas`/`rw_inc`/`rw_dec` are the atomics with and without `MULTIPROCESSOR` (on one
//!   CPU they are equivalent to the C's plain variants). `WITNESS`, `RWDIAG` (the 10 s sleep
//!   timeout and `db_enter`) and the `dt(4)` tracepoints (`TRACEINDEX`) are not configured;
//!   `rwl_traceidx` is kept.
//! - The `lock_type` arguments are gone with `WITNESS`: `_rw_init_flags(rwl, name, flags,
//!   type, trace)` is `rw_init_flags_trace(rwl, name, flags, trace)`, with `rw_init_flags`
//!   and `rw_init` as the C macros; the same for `rrw_init_flags`/`rrw_init` and
//!   `rw_obj_alloc_flags`/`rw_obj_alloc`.
//! - `rw_enter` returns `Result<(), Errno>` (`EBUSY` under `RW_NOSLEEP`, the sleep's error
//!   under `RW_INTR`); `rw_obj_free` returns `bool`.
//! - `rw_assert_*` are compiled in and empty without feature `diagnostic`, as the C macros.
//! - `rw_obj_alloc` returns the `&'static Rwlock` instead of writing through a pointer.

use core::cell::Cell;
use core::ptr::{self, NonNull};
use core::sync::atomic::{AtomicU32, Ordering, fence};

use crate::kassert;
#[cfg(feature = "multiprocessor")]
use crate::kern::kern_lock::_kernel_lock_held;
use crate::kern::kern_synch::{sleep_finish, sleep_setup, wakeup, wakeup_one};
use crate::kern::subr_pool::{pool_get, pool_init, pool_put};
use crate::kern::subr_prf::panic;
use crate::machine::cpu::curproc;
use crate::machine::intr::IPL_MPFLOOR;
#[cfg(feature = "multiprocessor")]
use crate::machine::{
    Machine,
    cpu::{Cpu, curcpu},
};
use crate::sys::errno::Errno;
use crate::sys::param::{PCATCH, PLOCK};
use crate::sys::pool::{PR_WAITOK, Pool};
use crate::sys::rwlock::{
    RW_DOWNGRADE, RW_DUPOK, RW_INTR, RW_NOSLEEP, RW_OPMASK, RW_READ, RW_RECURSEFAIL, RW_UPGRADE,
    RW_WRITE, RW_WRITE_OTHER, RWLOCK_MASK, RWLOCK_READ_INCR, RWLOCK_WRLOCK, Rrwlock, Rwlock,
};
use crate::sys::systm::INFSLP;

/// `RW_SLEEP_TMO`: how long a waiter sleeps before the `RWDIAG` complaint; forever without it.
const RW_SLEEP_TMO: u64 = INFSLP;

/// `RW_SPINS`: other OSes implement more sophisticated mechanism to determine how long the
/// process attempting to acquire the lock should be spinning. We start with the most simple
/// approach: we do `RW_SPINS` attempts at most before eventually giving up and putting the
/// process to sleep queue.
#[cfg(feature = "multiprocessor")]
const RW_SPINS: u32 = 1000;

/// `rw_cas(p, e, n)`: the compare-and-swap of the owner word; returns the old value.
fn rw_cas(p: &core::sync::atomic::AtomicUsize, e: usize, n: usize) -> usize {
    match p.compare_exchange(e, n, Ordering::SeqCst, Ordering::SeqCst) {
        Ok(o) | Err(o) => o,
    }
}

/// `rw_inc(p)`.
fn rw_inc(p: &AtomicU32) {
    p.fetch_add(1, Ordering::SeqCst);
}

/// `rw_dec(p)`.
fn rw_dec(p: &AtomicU32) {
    p.fetch_sub(1, Ordering::SeqCst);
}

/// `rw_self`: the owner word of the running thread as a writer.
fn rw_self() -> usize {
    let mut this = curproc().map_or(0, |p| ptr::from_ref(p) as usize);

    this &= !RWLOCK_MASK;
    this |= RWLOCK_WRLOCK;

    this
}

/// `rw_enter_read`.
pub fn rw_enter_read(rwl: &Rwlock) {
    let _ = rw_do_enter_read(rwl, 0);
}

/// `rw_enter_write`.
pub fn rw_enter_write(rwl: &Rwlock) {
    let _ = rw_do_enter_write(rwl, 0);
}

/// `rw_exit_read`.
pub fn rw_exit_read(rwl: &Rwlock) {
    // maybe we're the last one?
    rw_do_exit_read(rwl, RWLOCK_READ_INCR);
}

/// `rw_do_exit_read`: drops one reader, starting from the owner word `owner` the caller saw.
fn rw_do_exit_read(rwl: &Rwlock, owner: usize) {
    let mut owner = owner;
    let decr;

    // WITNESS_UNLOCK(&rwl->rwl_lock_obj, 0): not configured.

    loop {
        let d = owner.wrapping_sub(RWLOCK_READ_INCR);
        let nowner = rw_cas(&rwl.rwl_owner, owner, d);
        if owner == nowner {
            decr = d;
            break;
        }

        if nowner & RWLOCK_WRLOCK != 0 {
            panic(format_args!(
                "{} rwlock {:p}: exit read on write locked lock (owner {:#x})",
                rwl.name(),
                rwl,
                nowner
            ));
        }
        if nowner == 0 {
            panic(format_args!(
                "{} rwlock {:p}: exit read on unlocked lock",
                rwl.name(),
                rwl
            ));
        }

        owner = nowner;
    }

    // read lock didn't change anything, so no barrier needed?

    if decr == 0 {
        // last one out
        rw_exited(rwl);
    }
}

/// `rw_exit_write`.
pub fn rw_exit_write(rwl: &Rwlock) {
    let this = rw_self();

    // WITNESS_UNLOCK(&rwl->rwl_lock_obj, LOP_EXCLUSIVE): not configured.

    fence(Ordering::Release); // membar_exit_before_atomic()
    let owner = rw_cas(&rwl.rwl_owner, this, 0);
    if owner != this {
        panic(format_args!(
            "{} rwlock {:p}: exit write when lock not held (owner {:#x}, self {:#x})",
            rwl.name(),
            rwl,
            owner,
            this
        ));
    }

    rw_exited(rwl);
}

/// `_rw_init_flags_witness`: the initialisation shared by plain and recursive locks.
fn rw_init_flags_witness(rwl: &Rwlock, name: &'static str, _lo_flags: i32, trace: i32) {
    rwl.rwl_owner.store(0, Ordering::Relaxed);
    rwl.rwl_waiters.store(0, Ordering::Relaxed);
    rwl.rwl_readers.store(0, Ordering::Relaxed);
    rwl.rwl_name.set(Some(name));
    rwl.rwl_traceidx.set(trace);

    // WITNESS: rwl_lock_obj.lo_flags/lo_name/lo_type and WITNESS_INIT: not configured.
}

/// `rw_init_flags_trace(rwl, name, flags, trace)` (`_rw_init_flags` without the `lock_type`).
pub fn rw_init_flags_trace(rwl: &Rwlock, name: &'static str, flags: i32, trace: i32) {
    // RWLOCK_LO_FLAGS(flags) only feeds WITNESS.
    rw_init_flags_witness(rwl, name, flags, trace);
}

/// `rw_init_flags(rwl, name, flags)`.
pub fn rw_init_flags(rwl: &Rwlock, name: &'static str, flags: i32) {
    rw_init_flags_trace(rwl, name, flags, 0);
}

/// `rw_init(rwl, name)`.
pub fn rw_init(rwl: &Rwlock, name: &'static str) {
    rw_init_flags_trace(rwl, name, 0, 0);
}

/// `rw_enter`: takes the lock as `flags` says (`RW_WRITE`, `RW_READ`, `RW_DOWNGRADE`,
/// `RW_UPGRADE`, with `RW_NOSLEEP`/`RW_INTR`/`RW_DUPOK`).
pub fn rw_enter(rwl: &Rwlock, flags: i32) -> Result<(), Errno> {
    let op = flags & RW_OPMASK;

    match op {
        RW_WRITE => rw_do_enter_write(rwl, flags),
        RW_READ => rw_do_enter_read(rwl, flags),
        RW_DOWNGRADE => rw_downgrade(rwl, flags),
        RW_UPGRADE => rw_upgrade(rwl, flags),
        _ => panic(format_args!(
            "{} rwlock {:p}: rw_enter unexpected op {:#x}",
            rwl.name(),
            rwl,
            op
        )),
    }
}

/// `rw_do_enter_write`.
fn rw_do_enter_write(rwl: &Rwlock, flags: i32) -> Result<(), Errno> {
    let this = rw_self();
    let _ = RW_DUPOK; // WITNESS: LOP_NEWORDER | LOP_EXCLUSIVE (| LOP_DUPOK), WITNESS_CHECKORDER.

    let mut owner = rw_cas(&rwl.rwl_owner, 0, this);
    if owner == 0 {
        // wow, we won. so easy
        fence(Ordering::Acquire); // locked: membar_enter_after_atomic()
        return Ok(());
    }
    if owner == this {
        panic(format_args!(
            "{} rwlock {:p}: enter write deadlock",
            rwl.name(),
            rwl
        ));
    }

    // If process holds the kernel lock, then we want to give up on CPU as soon as possible
    // so other processes waiting for the kernel lock can progress. Hence no spinning if we
    // hold the kernel lock.
    #[cfg(feature = "multiprocessor")]
    if !_kernel_lock_held() {
        let spc = Machine::ci_schedstate(curcpu());

        // It makes sense to try to spin just in case the lock is acquired by writer.
        spc.spc_spinning.fetch_add(1, Ordering::Relaxed);
        for _ in 0..RW_SPINS {
            core::hint::spin_loop(); // CPU_BUSY_CYCLE()
            owner = rwl.rwl_owner.load(Ordering::Relaxed);
            if owner != 0 {
                continue;
            }

            owner = rw_cas(&rwl.rwl_owner, 0, this);
            if owner == 0 {
                spc.spc_spinning.fetch_sub(1, Ordering::Relaxed);
                // ok, we won now.
                fence(Ordering::Acquire); // locked: membar_enter_after_atomic()
                return Ok(());
            }
        }
        spc.spc_spinning.fetch_sub(1, Ordering::Relaxed);
    }

    if flags & RW_NOSLEEP != 0 {
        return Err(Errno::EBUSY);
    }

    let mut prio = PLOCK - 4;
    if flags & RW_INTR != 0 {
        prio |= PCATCH;
    }

    rw_inc(&rwl.rwl_waiters);
    fence(Ordering::Release); // membar_producer()
    loop {
        sleep_setup(ptr::from_ref(&rwl.rwl_waiters).cast(), prio, rwl.name());
        fence(Ordering::Acquire); // membar_consumer()
        owner = rwl.rwl_owner.load(Ordering::SeqCst);
        let error = sleep_finish(RW_SLEEP_TMO, owner != 0);
        // RWDIAG: the EWOULDBLOCK complaint and db_enter.
        if flags & RW_INTR != 0
            && let Err(e) = error
        {
            rw_dec(&rwl.rwl_waiters);
            return Err(e);
        }

        owner = rw_cas(&rwl.rwl_owner, 0, this);
        if owner == 0 {
            break;
        }
    }
    rw_dec(&rwl.rwl_waiters);

    // locked:
    fence(Ordering::Acquire); // membar_enter_after_atomic()
    // WITNESS_LOCK: not configured.

    Ok(())
}

/// `rw_read_incr`: adds a reader starting from the owner word `owner`; `false` when a writer
/// got in first.
fn rw_read_incr(rwl: &Rwlock, owner: usize) -> bool {
    let mut owner = owner;

    loop {
        let incr = owner + RWLOCK_READ_INCR;
        let nowner = rw_cas(&rwl.rwl_owner, owner, incr);
        if nowner == owner {
            return true;
        }

        owner = nowner;
        if owner & RWLOCK_WRLOCK != 0 {
            return false;
        }
    }
}

/// `rw_do_enter_read`.
fn rw_do_enter_read(rwl: &Rwlock, flags: i32) -> Result<(), Errno> {
    // WITNESS: LOP_NEWORDER (| LOP_DUPOK), WITNESS_CHECKORDER: not configured.

    let owner = rw_cas(&rwl.rwl_owner, 0, RWLOCK_READ_INCR);
    if owner == 0 {
        // ermagerd, we won!
        fence(Ordering::Acquire); // locked: membar_enter_after_atomic()
        return Ok(());
    }

    if owner & RWLOCK_WRLOCK != 0 {
        if owner == rw_self() {
            panic(format_args!(
                "{} rwlock {:p}: enter read deadlock",
                rwl.name(),
                rwl
            ));
        }
    } else if rwl.rwl_waiters.load(Ordering::SeqCst) == 0 && rw_read_incr(rwl, owner) {
        // nailed it
        fence(Ordering::Acquire);
        return Ok(());
    }

    if flags & RW_NOSLEEP != 0 {
        return Err(Errno::EBUSY);
    }

    let mut prio = PLOCK;
    if flags & RW_INTR != 0 {
        prio |= PCATCH;
    }

    rw_inc(&rwl.rwl_readers);
    fence(Ordering::Release); // membar_producer()
    loop {
        sleep_setup(ptr::from_ref(&rwl.rwl_readers).cast(), prio, rwl.name());
        fence(Ordering::Acquire); // membar_consumer()
        let error = sleep_finish(
            RW_SLEEP_TMO,
            rwl.rwl_waiters.load(Ordering::SeqCst) > 0
                || rwl.rwl_owner.load(Ordering::SeqCst) & RWLOCK_WRLOCK != 0,
        );
        // RWDIAG: the EWOULDBLOCK complaint and db_enter.
        if flags & RW_INTR != 0
            && let Err(e) = error
        {
            rw_dec(&rwl.rwl_readers);
            return Err(e);
        }
        if rw_read_incr(rwl, 0) {
            break;
        }
    }
    rw_dec(&rwl.rwl_readers);

    // locked:
    fence(Ordering::Acquire); // membar_enter_after_atomic()
    // WITNESS_LOCK: not configured.

    Ok(())
}

/// `rw_downgrade`: the writer becomes a reader.
fn rw_downgrade(rwl: &Rwlock, _flags: i32) -> Result<(), Errno> {
    let this = rw_self();

    fence(Ordering::Release); // membar_exit_before_atomic()
    let owner = rw_cas(&rwl.rwl_owner, this, RWLOCK_READ_INCR);
    if owner != this {
        panic(format_args!(
            "{} rwlock {:p}: downgrade when lock not held (owner {:#x}, self {:#x})",
            rwl.name(),
            rwl,
            owner,
            this
        ));
    }

    // WITNESS_DOWNGRADE: not configured.

    fence(Ordering::Acquire); // membar_consumer()
    if rwl.rwl_waiters.load(Ordering::SeqCst) == 0 && rwl.rwl_readers.load(Ordering::SeqCst) > 0 {
        wakeup(ptr::from_ref(&rwl.rwl_readers));
    }

    Ok(())
}

/// `rw_upgrade`: the only reader becomes the writer; `EBUSY` otherwise.
fn rw_upgrade(rwl: &Rwlock, flags: i32) -> Result<(), Errno> {
    let this = rw_self();

    kassert!(flags & RW_NOSLEEP != 0); // "RW_UPGRADE without RW_NOSLEEP"

    let owner = rw_cas(&rwl.rwl_owner, RWLOCK_READ_INCR, this);
    if owner != RWLOCK_READ_INCR {
        if owner == 0 {
            panic(format_args!(
                "{} rwlock {:p}: upgrade on unowned lock",
                rwl.name(),
                rwl
            ));
        }
        if owner & RWLOCK_WRLOCK != 0 {
            panic(format_args!(
                "{} rwlock {:p}: upgrade on write locked lock(owner {:#x}, self {:#x})",
                rwl.name(),
                rwl,
                owner,
                this
            ));
        }
        return Err(Errno::EBUSY);
    }

    // WITNESS_UPGRADE: not configured.

    Ok(())
}

/// `rw_exit`: releases the lock whichever way it is held.
pub fn rw_exit(rwl: &Rwlock) {
    let owner = rwl.rwl_owner.load(Ordering::SeqCst);
    if owner == 0 {
        panic(format_args!(
            "{} rwlock {:p}: exit on unlocked lock",
            rwl.name(),
            rwl
        ));
    }

    if owner & RWLOCK_WRLOCK != 0 {
        rw_exit_write(rwl);
    } else {
        rw_do_exit_read(rwl, owner);
    }
}

/// `rw_exited`: wakes the next writer, else the readers.
fn rw_exited(rwl: &Rwlock) {
    fence(Ordering::Acquire); // membar_consumer()
    if rwl.rwl_waiters.load(Ordering::SeqCst) > 0 {
        wakeup_one(ptr::from_ref(&rwl.rwl_waiters));
    } else if rwl.rwl_readers.load(Ordering::SeqCst) > 0 {
        wakeup(ptr::from_ref(&rwl.rwl_readers));
    }
}

/// `rw_status`: `RW_WRITE` (ours), `RW_WRITE_OTHER`, `RW_READ` or 0.
pub fn rw_status(rwl: &Rwlock) -> i32 {
    let owner = rwl.rwl_owner.load(Ordering::SeqCst);
    if owner & RWLOCK_WRLOCK != 0 {
        if rw_self() == owner {
            return RW_WRITE;
        }
        return RW_WRITE_OTHER;
    }
    if owner != 0 {
        return RW_READ;
    }
    0
}

/// Whether the `DIAGNOSTIC` assertions are to be skipped (after a panic or inside ddb).
#[cfg(feature = "diagnostic")]
fn rw_assert_skip() -> bool {
    crate::kern::subr_prf::panicstr() || crate::kern::init_main::DB_ACTIVE.load(Ordering::Relaxed)
}

/// `rw_assert_wrlock` (`DIAGNOSTIC`).
pub fn rw_assert_wrlock(rwl: &Rwlock) {
    #[cfg(feature = "diagnostic")]
    {
        if rw_assert_skip() {
            return;
        }
        if rwl.rwl_owner.load(Ordering::SeqCst) != rw_self() {
            panic(format_args!(
                "{} rwlock {:p}: lock not held by this process",
                rwl.name(),
                rwl
            ));
        }
    }
    #[cfg(not(feature = "diagnostic"))]
    let _ = rwl;
}

/// `rw_assert_rdlock` (`DIAGNOSTIC`).
pub fn rw_assert_rdlock(rwl: &Rwlock) {
    #[cfg(feature = "diagnostic")]
    {
        if rw_assert_skip() {
            return;
        }
        if rw_status(rwl) != RW_READ {
            panic(format_args!(
                "{} rwlock {:p}: lock not shared",
                rwl.name(),
                rwl
            ));
        }
    }
    #[cfg(not(feature = "diagnostic"))]
    let _ = rwl;
}

/// `rw_assert_anylock` (`DIAGNOSTIC`).
pub fn rw_assert_anylock(rwl: &Rwlock) {
    #[cfg(feature = "diagnostic")]
    {
        if rw_assert_skip() {
            return;
        }
        match rw_status(rwl) {
            RW_WRITE_OTHER => panic(format_args!(
                "{} rwlock {:p}: lock held by different process (self {:#x}, owner {:#x})",
                rwl.name(),
                rwl,
                rw_self(),
                rwl.rwl_owner.load(Ordering::SeqCst)
            )),
            0 => panic(format_args!(
                "{} rwlock {:p}: lock not held",
                rwl.name(),
                rwl
            )),
            _ => {}
        }
    }
    #[cfg(not(feature = "diagnostic"))]
    let _ = rwl;
}

/// `rw_assert_unlocked` (`DIAGNOSTIC`).
pub fn rw_assert_unlocked(rwl: &Rwlock) {
    #[cfg(feature = "diagnostic")]
    {
        if rw_assert_skip() {
            return;
        }
        if rwl.rwl_owner.load(Ordering::SeqCst) == rw_self() {
            panic(format_args!("{} rwlock {:p}: lock held", rwl.name(), rwl));
        }
    }
    #[cfg(not(feature = "diagnostic"))]
    let _ = rwl;
}

/// `rrw_init_flags(rrwl, name, flags)` (`_rrw_init_flags` without the `lock_type`).
pub fn rrw_init_flags(rrwl: &Rrwlock, name: &'static str, flags: i32) {
    rrwl.rrwl_wcnt.set(0);
    rw_init_flags_witness(&rrwl.rrwl_lock, name, flags, 0);
}

/// `rrw_init(rrwl, name)`.
pub fn rrw_init(rrwl: &Rrwlock, name: &'static str) {
    rrw_init_flags(rrwl, name, 0);
}

/// `rrw_enter`: `rw_enter` that lets the writer take the lock again.
pub fn rrw_enter(rrwl: &Rrwlock, flags: i32) -> Result<(), Errno> {
    if rrwl.rrwl_lock.rwl_owner.load(Ordering::SeqCst) == rw_self() {
        if flags & RW_RECURSEFAIL != 0 {
            return Err(Errno::EDEADLK);
        }
        rrwl.rrwl_wcnt.set(rrwl.rrwl_wcnt.get() + 1);
        // WITNESS_LOCK(LOP_EXCLUSIVE): not configured.
        return Ok(());
    }

    let rv = rw_enter(&rrwl.rrwl_lock, flags);
    if rv.is_ok() {
        rrwl.rrwl_wcnt.set(1);
    }

    rv
}

/// `rrw_exit`.
pub fn rrw_exit(rrwl: &Rrwlock) {
    if rrwl.rrwl_lock.rwl_owner.load(Ordering::SeqCst) == rw_self() {
        kassert!(rrwl.rrwl_wcnt.get() > 0);
        rrwl.rrwl_wcnt.set(rrwl.rrwl_wcnt.get() - 1);
        if rrwl.rrwl_wcnt.get() != 0 {
            // WITNESS_UNLOCK(LOP_EXCLUSIVE): not configured.
            return;
        }
    }

    rw_exit(&rrwl.rrwl_lock);
}

/// `rrw_status`.
pub fn rrw_status(rrwl: &Rrwlock) -> i32 {
    rw_status(&rrwl.rrwl_lock)
}

/// `RWLOCK_OBJ_MAGIC`.
const RWLOCK_OBJ_MAGIC: u32 = 0x5aa3_c85d;

/// `struct rwlock_obj`: an allocated, reference-counted rwlock.
pub struct RwlockObj {
    /// `ro_lock`.
    pub ro_lock: Rwlock,
    /// `ro_magic`.
    pub ro_magic: Cell<u32>,
    /// `ro_refcnt`.
    pub ro_refcnt: AtomicU32,
}

/// `rwlock_obj_pool`.
static RWLOCK_OBJ_POOL: Pool = Pool::new();

/// `rw_obj_init`: initialize the mutex object store.
pub fn rw_obj_init() {
    pool_init(
        &RWLOCK_OBJ_POOL,
        size_of::<RwlockObj>(),
        0,
        IPL_MPFLOOR,
        PR_WAITOK,
        "rwobjpl",
        None,
    );
}

/// `rw_obj_alloc_flags(lock, name, flags)`: allocate a single lock object.
pub fn rw_obj_alloc_flags(name: &'static str, flags: i32) -> &'static Rwlock {
    let Some(mem) = pool_get(&RWLOCK_OBJ_POOL, PR_WAITOK) else {
        panic(format_args!("rw_obj_alloc: rwlock_obj_pool is empty"));
    };
    let mo = mem.cast::<RwlockObj>();
    // SAFETY: a fresh, suitably aligned pool item of `size_of::<RwlockObj>()` bytes,
    // written once before anything else sees it; it lives until `rw_obj_free` returns it.
    let mo: &'static RwlockObj = unsafe {
        mo.as_ptr().write(RwlockObj {
            ro_lock: Rwlock::new(name),
            ro_magic: Cell::new(RWLOCK_OBJ_MAGIC),
            ro_refcnt: AtomicU32::new(1),
        });
        mo.as_ref()
    };
    rw_init_flags_trace(&mo.ro_lock, name, flags, 0);

    &mo.ro_lock
}

/// `rw_obj_alloc(lock, name)`.
pub fn rw_obj_alloc(name: &'static str) -> &'static Rwlock {
    rw_obj_alloc_flags(name, 0)
}

/// The object a lock from `rw_obj_alloc` lives in.
fn rw_obj_of(lock: &Rwlock) -> &RwlockObj {
    // SAFETY: `ro_lock` is the first member of `RwlockObj` (`#[repr(C)]` is not needed:
    // the cast goes through `offset_of`), and a lock given to `rw_obj_hold`/`rw_obj_free`
    // came out of `rw_obj_alloc`, which the magic number checks.
    let mo = unsafe {
        &*ptr::from_ref(lock)
            .cast::<u8>()
            .wrapping_sub(core::mem::offset_of!(RwlockObj, ro_lock))
            .cast::<RwlockObj>()
    };
    kassert!(mo.ro_magic.get() == RWLOCK_OBJ_MAGIC);
    kassert!(mo.ro_refcnt.load(Ordering::Relaxed) > 0);
    mo
}

/// `rw_obj_hold`: add a single reference to a lock object. A reference to the object must
/// already be held, and must be held across this call.
pub fn rw_obj_hold(lock: &Rwlock) {
    let mo = rw_obj_of(lock);

    mo.ro_refcnt.fetch_add(1, Ordering::SeqCst);
}

/// `rw_obj_free`: drop a reference from a lock object. If the last reference is being
/// dropped, free the object and return true. Otherwise, return false.
pub fn rw_obj_free(lock: &'static Rwlock) -> bool {
    let mo = rw_obj_of(lock);

    if mo.ro_refcnt.fetch_sub(1, Ordering::SeqCst) - 1 > 0 {
        return false;
    }
    // WITNESS_DESTROY(&mo->ro_lock): notyet in C.
    pool_put(&RWLOCK_OBJ_POOL, NonNull::from(mo).cast::<u8>());
    true
}
/* </CODE> */

/* <TESTS> */
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn write_then_read() {
        let l = Rwlock::new("test");
        assert_eq!(rw_status(&l), 0);
        rw_enter_write(&l);
        assert_eq!(rw_status(&l), RW_WRITE);
        assert!(crate::sys::rwlock::rw_write_held(&l));
        rw_exit(&l);
        rw_enter_read(&l);
        rw_enter_read(&l);
        assert_eq!(rw_status(&l), RW_READ);
        assert_eq!(rw_enter(&l, RW_WRITE | RW_NOSLEEP), Err(Errno::EBUSY));
        rw_exit_read(&l);
        assert_eq!(rw_enter(&l, RW_UPGRADE | RW_NOSLEEP), Ok(()));
        assert_eq!(rw_status(&l), RW_WRITE);
        assert_eq!(rw_enter(&l, RW_DOWNGRADE), Ok(()));
        assert_eq!(rw_status(&l), RW_READ);
        rw_exit(&l);
        assert_eq!(rw_status(&l), 0);
    }

    #[test]
    fn recursive_writer() {
        let l = Rrwlock::new("rtest");
        assert_eq!(rrw_enter(&l, RW_WRITE), Ok(()));
        assert_eq!(
            rrw_enter(&l, RW_WRITE | RW_RECURSEFAIL),
            Err(Errno::EDEADLK)
        );
        assert_eq!(rrw_enter(&l, RW_WRITE), Ok(()));
        assert_eq!(l.rrwl_wcnt.get(), 2);
        rrw_exit(&l);
        assert_eq!(rrw_status(&l), RW_WRITE);
        rrw_exit(&l);
        assert_eq!(rrw_status(&l), 0);
    }
}
/* </TESTS> */
