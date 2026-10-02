/*	$OpenBSD: kern_lock.c,v 1.87 2026/08/30 23:36:26 gnezdo Exp $	*/
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

//! Kernel lock and mutexes: `kern/kern_lock.c`.
//!
//! Upstream: sys/kern/kern_lock.c @ 3ce1f3f79392
//!
//! Status: `wip`. Milestone M4 ports the uniprocessor mutex: `__mtx_init`, `_mtx_init`,
//! `mtx_init`, `mtx_init_flags`, `mtx_enter`, `mtx_enter_try` and `mtx_leave`; M5 adds the
//! `pc_lock` producer/consumer generation lock (`pc_lock_init`, `pc_sprod_*`, `pc_mprod_*`,
//! `pc_cons_*`). The kernel lock (`__mp_lock`, `_kernel_lock*`), the `MULTIPROCESSOR` mutex
//! with its parking lots and `db_mtx_enter`/`db_mtx_leave` come later; `WITNESS` is not
//! configured.
//!
//! ## Deviations
//! - `membar_producer`/`membar_consumer`/`membar_exit` are `fence`s of the matching
//!   ordering; `pc_mprod_*` are `pc_sprod_*` (the C aliases them without `MULTIPROCESSOR`).
//!
//! ## Deviations
//! - `KERNEL_LOCK()`/`KERNEL_UNLOCK()` are nothing without `MULTIPROCESSOR`, as in the C's
//!   `<sys/systm.h>`, so no function stands in for them.

use core::sync::atomic::{Ordering, fence};

use crate::kern::init_main::DB_ACTIVE;
use crate::kern::subr_prf::panicstr;
#[cfg(feature = "diagnostic")]
use crate::machine::Machine;
#[cfg(feature = "diagnostic")]
use crate::machine::cpu::Cpu;
use crate::machine::intr::{IPL_NONE, splraise, splx};
#[cfg(feature = "diagnostic")]
use crate::sys::mutex::mtx_owner;
use crate::sys::mutex::{Mutex, mtx_curcpu, mutex_assert_locked, mutex_ipl};
use crate::sys::pclock::PcLock;

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

/// `mtx_enter_try`: on a uniprocessor the mutex is always free (or the kernel is past caring).
pub fn mtx_enter_try(mtx: &Mutex) -> bool {
    mtx_enter(mtx);
    true
}

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

/// `pc_mprod_enter`: multiple (interlocking) producers; without `MULTIPROCESSOR` the
/// single-producer entry.
pub fn pc_mprod_enter(pcl: &PcLock) -> u32 {
    pc_sprod_enter(pcl)
}

/// `pc_mprod_leave`.
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
    fn init_resets() {
        let m = Mutex::new(3);
        m.mtx_owner.store(42, Ordering::Relaxed);
        mtx_init(&m, 5);
        assert_eq!(mtx_owner(&m), 0);
        assert_eq!(m.mtx_wantipl.get(), 5);
    }
}
