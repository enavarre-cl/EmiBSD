/*	$OpenBSD: smr.h,v 1.9 2022/07/25 08:06:44 visa Exp $	*/
/* <LICENSES> */
/*
 * Copyright (c) 2019 Visa Hankala
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
 * Copyright (c) 1991, 1993
 *	The Regents of the University of California.  All rights reserved.
 *
 * Redistribution and use in source and binary forms, with or without
 * modification, are permitted provided that the following conditions
 * are met:
 * 1. Redistributions of source code must retain the above copyright
 *    notice, this list of conditions and the following disclaimer.
 * 2. Redistributions in binary form must reproduce the above copyright
 *    notice, this list of conditions and the following disclaimer in the
 *    documentation and/or other materials provided with the distribution.
 * 3. Neither the name of the University nor the names of its contributors
 *    may be used to endorse or promote products derived from this software
 *    without specific prior written permission.
 *
 * THIS SOFTWARE IS PROVIDED BY THE REGENTS AND CONTRIBUTORS ``AS IS'' AND
 * ANY EXPRESS OR IMPLIED WARRANTIES, INCLUDING, BUT NOT LIMITED TO, THE
 * IMPLIED WARRANTIES OF MERCHANTABILITY AND FITNESS FOR A PARTICULAR PURPOSE
 * ARE DISCLAIMED.  IN NO EVENT SHALL THE REGENTS OR CONTRIBUTORS BE LIABLE
 * FOR ANY DIRECT, INDIRECT, INCIDENTAL, SPECIAL, EXEMPLARY, OR CONSEQUENTIAL
 * DAMAGES (INCLUDING, BUT NOT LIMITED TO, PROCUREMENT OF SUBSTITUTE GOODS
 * OR SERVICES; LOSS OF USE, DATA, OR PROFITS; OR BUSINESS INTERRUPTION)
 * HOWEVER CAUSED AND ON ANY THEORY OF LIABILITY, WHETHER IN CONTRACT, STRICT
 * LIABILITY, OR TORT (INCLUDING NEGLIGENCE OR OTHERWISE) ARISING IN ANY WAY
 * OUT OF THE USE OF THIS SOFTWARE, EVEN IF ADVISED OF THE POSSIBILITY OF
 * SUCH DAMAGE.
 *
 *	@(#)queue.h	8.5 (Berkeley) 8/20/94
 */
/* </LICENSES> */

//! `<sys/smr.h>`: safe memory reclamation, see `smr_call(9)`. Readers walk shared data
//! inside a read section (`smr_read_enter`/`smr_read_leave`) without locks; writers unlink an
//! object under their lock and hand its destruction to `smr_call`, which runs it once every
//! CPU has passed a quiescent state (a context switch, or the idle loop).
//!
//! Upstream: sys/sys/smr.h @ 3ce1f3f79392
//!
//! Status: `wip` (M11a): `struct smr_entry`, `struct smr_entry_list`, `smr_init`, `smr_call`,
//! `smr_barrier`, `smr_flush`, `SMR_ASSERT_CRITICAL`, `SMR_ASSERT_NONCRITICAL`,
//! `SMR_PTR_GET`, `SMR_PTR_GET_LOCKED` and `SMR_PTR_SET_LOCKED` (as [`SmrPtr`]). The
//! functions are in `kern/kern_smr.rs`.
//!
//! ## Deviations
//! - The SMR list families (`SMR_SLIST_*`, `SMR_LIST_*`, `SMR_TAILQ_*`) are not ported: no
//!   ported file uses them. The network code that does in C (`if.c`, `bpf.c`, `if_pflow.c`,
//!   `art.c`, `rtable.c`) keeps plain `queue.h` lists read under the kernel lock, as its
//!   module deviations say; they come with the M11e audit that unlocks those paths.
//! - `smr_call` (and `smr_call_impl`, `kern_smr.rs`) take a `&'static SmrEntry`: the entry
//!   must outlive the deferral (`smr_barrier_impl` lends its stack entry as `'static`
//!   while it waits for the call).
//! - `SMR_PTR_GET` is an `Acquire` load where the C's `READ_ONCE` relies on the hardware's
//!   dependency ordering, which Rust's memory model does not offer; `SMR_PTR_SET_LOCKED` is a
//!   `Release` store (the C's `membar_producer` then `WRITE_ONCE`).

use core::cell::Cell;
use core::ffi::c_void;
use core::ptr;
use core::sync::atomic::{AtomicPtr, Ordering};

use crate::queue_adapter;
use crate::sys::queue::SimpleqEntry;

/// `struct smr_entry`: a deferred call, embedded in the object it destroys.
pub struct SmrEntry {
    /// `smr_list`: the CPU's or the system-wide queue of deferred calls.
    pub smr_list: SimpleqEntry<SmrEntry>,
    /// `smr_func`: the deferred function, `None` while the entry is idle.
    pub smr_func: Cell<Option<fn(*mut c_void)>>,
    /// `smr_arg`: its argument.
    pub smr_arg: Cell<*mut c_void>,
}

impl SmrEntry {
    /// An idle entry (what `smr_init` leaves, and the zero the C's allocators give).
    pub const fn new() -> Self {
        Self {
            smr_list: SimpleqEntry::new(),
            smr_func: Cell::new(None),
            smr_arg: Cell::new(ptr::null_mut()),
        }
    }
}

impl Default for SmrEntry {
    fn default() -> Self {
        Self::new()
    }
}

// SAFETY: an entry is written by the CPU that queues it (at splhigh, on its own queue), then
// moved between queues under `smr_lock` and read by the SMR thread once it owns it.
unsafe impl Sync for SmrEntry {}

queue_adapter!(
    /// `SIMPLEQ_HEAD(smr_entry_list, smr_entry)`.
    pub SmrEntryList: SmrEntry, smr_list => SimpleqEntry<SmrEntry>
);

/// `SMR_PTR_GET`/`SMR_PTR_GET_LOCKED`/`SMR_PTR_SET_LOCKED` over one SMR-protected pointer:
/// readers load it inside a read section, the writer stores it under its lock.
pub struct SmrPtr<T>(AtomicPtr<T>);

impl<T> SmrPtr<T> {
    /// A null pointer.
    pub const fn new() -> Self {
        Self(AtomicPtr::new(ptr::null_mut()))
    }

    /// `SMR_PTR_GET(pptr)`: the pointer, for a reader inside a read section.
    pub fn get(&self) -> *mut T {
        self.0.load(Ordering::Acquire)
    }

    /// `SMR_PTR_GET_LOCKED(pptr)`: the pointer, for the writer that holds the lock.
    pub fn get_locked(&self) -> *mut T {
        self.0.load(Ordering::Relaxed)
    }

    /// `SMR_PTR_SET_LOCKED(pptr, val)`: publishes `val` once everything it points to is
    /// written (`membar_producer`).
    pub fn set_locked(&self, val: *mut T) {
        self.0.store(val, Ordering::Release);
    }
}

impl<T> Default for SmrPtr<T> {
    fn default() -> Self {
        Self::new()
    }
}

/// `smr_init(smr)`: an idle entry.
#[inline]
pub fn smr_init(smr: &SmrEntry) {
    smr.smr_func.set(None);
    smr.smr_arg.set(ptr::null_mut());
}

/// `smr_call(entry, func, arg)`: `func(arg)` once every CPU has left its read sections.
#[inline]
pub fn smr_call(entry: &'static SmrEntry, func: fn(*mut c_void), arg: *mut c_void) {
    crate::kern::kern_smr::smr_call_impl(entry, func, arg, false);
}

/// `smr_barrier()`: waits until every read section that started before it has ended.
#[inline]
pub fn smr_barrier() {
    crate::kern::kern_smr::smr_barrier_impl(false);
}

/// `smr_flush()`: `smr_barrier` without the pause between rounds.
#[inline]
pub fn smr_flush() {
    crate::kern::kern_smr::smr_barrier_impl(true);
}

/// `SMR_ASSERT_CRITICAL()` (`DIAGNOSTIC`): inside a read section.
#[inline]
pub fn smr_assert_critical() {
    #[cfg(feature = "diagnostic")]
    if !crate::kern::subr_prf::panicstr()
        && !crate::kern::init_main::DB_ACTIVE.load(Ordering::Relaxed)
    {
        crate::kassert!(smr_depth() > 0);
    }
}

/// `SMR_ASSERT_NONCRITICAL()` (`DIAGNOSTIC`): outside every read section.
#[inline]
pub fn smr_assert_noncritical() {
    #[cfg(feature = "diagnostic")]
    if !crate::kern::subr_prf::panicstr()
        && !crate::kern::init_main::DB_ACTIVE.load(Ordering::Relaxed)
    {
        crate::kassert!(smr_depth() == 0);
    }
}

/// `curcpu()->ci_schedstate.spc_smrdepth`.
#[cfg(feature = "diagnostic")]
fn smr_depth() -> u32 {
    use crate::machine::Machine;
    use crate::machine::cpu::{Cpu, curcpu};
    Machine::ci_schedstate(curcpu()).spc_smrdepth.get()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn entries_start_idle() {
        let e = SmrEntry::new();
        assert!(e.smr_func.get().is_none());
        e.smr_func.set(Some(|_| {}));
        e.smr_arg.set(ptr::from_ref(&e).cast_mut().cast());
        smr_init(&e);
        assert!(e.smr_func.get().is_none());
        assert!(e.smr_arg.get().is_null());
    }

    #[test]
    fn smr_ptr_publishes() {
        let mut x = 5;
        let p: SmrPtr<i32> = SmrPtr::new();
        assert!(p.get().is_null());
        p.set_locked(&mut x);
        assert_eq!(p.get(), ptr::from_mut(&mut x));
        assert_eq!(p.get_locked(), p.get());
    }
}
