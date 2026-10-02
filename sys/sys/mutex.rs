/*	$OpenBSD: mutex.h,v 1.26 2025/12/11 23:34:44 dlg Exp $	*/
/*
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

//! `<sys/mutex.h>`: the mutex. A mutex is owned by a cpu, non-recursive, spinning, and not
//! providing mutual exclusion between processes, only cpus.
//!
//! Upstream: sys/sys/mutex.h @ 3ce1f3f79392
//!
//! Status: `wip`. Milestone M4 ports `struct mutex` (the machine-independent one: both
//! architectures' `<machine/mutex.h>` say `__USE_MI_MUTEX`), `MUTEX_INITIALIZER`,
//! `mtx_curcpu`, `mtx_owner`, `mtx_owned`, `MUTEX_ASSERT_LOCKED`/`UNLOCKED` and the `MTX_*`
//! flags. `WITNESS` (`lock_object`, `MTX_LO_INITIALIZER`) and `struct db_mutex` come with M5.
//! The functions live in `kern/kern_lock.rs`.
//!
//! ## Deviations
//! - `__MUTEX_IPL(ipl)` is `ipl`: there is no `MULTIPROCESSOR`, so no `IPL_MPFLOOR` raise.
//! - The fields are an atomic and `Cell`s, so a `static` mutex is entered through `&`.

use core::cell::Cell;
use core::sync::atomic::{AtomicUsize, Ordering};

use crate::kern::init_main::DB_ACTIVE;
use crate::kern::subr_prf::panicstr;
use crate::machine::Machine;
use crate::machine::cpu::Cpu;
use crate::machine::intr::IPL_NONE;
#[cfg(not(feature = "diagnostic"))]
#[allow(unused_imports)] // the DIAGNOSTIC asserts are the readers
use core::hint as _;

/// `__MUTEX_IPL(ipl)`: the level a mutex of `ipl` raises to (see the module's deviations).
pub const fn mutex_ipl(ipl: i32) -> i32 {
    ipl
}

/// `MTX_NOWITNESS`.
pub const MTX_NOWITNESS: i32 = 0x01;
/// `MTX_DUPOK`.
pub const MTX_DUPOK: i32 = 0x02;

/// `struct mutex`.
pub struct Mutex {
    /// `mtx_owner`: the owning CPU (`mtx_curcpu()`), 0 when free; bit 0 is the MP waiter flag.
    pub mtx_owner: AtomicUsize,
    /// `mtx_wantipl`: the level the mutex raises to.
    pub mtx_wantipl: Cell<i32>,
    /// `mtx_oldipl`: the level to restore on leave.
    pub mtx_oldipl: Cell<i32>,
}

// SAFETY: the mutex is its own lock: the owner is set and the cells written by the one CPU
// that entered it, on entry and exit.
unsafe impl Sync for Mutex {}

impl Mutex {
    /// `MUTEX_INITIALIZER(ipl)`: a free mutex that raises to `ipl`.
    pub const fn new(ipl: i32) -> Self {
        Self {
            mtx_owner: AtomicUsize::new(0),
            mtx_wantipl: Cell::new(mutex_ipl(ipl)),
            mtx_oldipl: Cell::new(IPL_NONE),
        }
    }
}

/// `mtx_curcpu()`: this CPU, as the number `mtx_owner` holds.
pub fn mtx_curcpu() -> usize {
    Machine::curcpu_ptr() as usize
}

/// `mtx_owner(mtx)`: the owning CPU, without the waiter bit.
pub fn mtx_owner(mtx: &Mutex) -> usize {
    mtx.mtx_owner.load(Ordering::Relaxed) & !1
}

/// `MUTEX_ASSERT_LOCKED(mtx)` (`DIAGNOSTIC`).
pub fn mutex_assert_locked(mtx: &Mutex, func: &str) {
    #[cfg(feature = "diagnostic")]
    if mtx_owner(mtx) != mtx_curcpu() && !(panicstr() || DB_ACTIVE.load(Ordering::Relaxed)) {
        crate::kern::subr_prf::panic(format_args!("mutex {:p} not held in {func}", mtx));
    }
    #[cfg(not(feature = "diagnostic"))]
    let _ = (mtx, func);
}

/// `MUTEX_ASSERT_UNLOCKED(mtx)` (`DIAGNOSTIC`).
pub fn mutex_assert_unlocked(mtx: &Mutex, func: &str) {
    #[cfg(feature = "diagnostic")]
    if mtx_owner(mtx) == mtx_curcpu() && !(panicstr() || DB_ACTIVE.load(Ordering::Relaxed)) {
        crate::kern::subr_prf::panic(format_args!("mutex {:p} held in {func}", mtx));
    }
    #[cfg(not(feature = "diagnostic"))]
    let _ = (mtx, func);
}

/// `mtx_owned(mtx)`: whether this CPU holds `mtx` (or the kernel is past caring).
pub fn mtx_owned(mtx: &Mutex) -> bool {
    mtx_owner(mtx) == mtx_curcpu() || panicstr() || DB_ACTIVE.load(Ordering::Relaxed)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn initializer_is_free() {
        let m = Mutex::new(7);
        assert_eq!(mtx_owner(&m), 0);
        assert_eq!(m.mtx_wantipl.get(), 7);
        assert_eq!(m.mtx_oldipl.get(), IPL_NONE);
        assert!(!mtx_owned(&m));
    }
}
