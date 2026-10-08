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

/* <CODE> */
//! `<sys/smr.h>`: safe memory reclamation, see `smr_call(9)`. Readers walk shared data
//! inside a read section (`smr_read_enter`/`smr_read_leave`) without locks; writers unlink an
//! object under their lock and hand its destruction to `smr_call`, which runs it once every
//! CPU has passed a quiescent state (a context switch, or the idle loop).
//!
//! Upstream: sys/sys/smr.h @ 3ce1f3f79392
//!
//! Status: `wip` (M11a): `struct smr_entry`, `struct smr_entry_list`, `smr_init`, `smr_call`,
//! `smr_barrier`, `smr_flush`, `SMR_ASSERT_CRITICAL`, `SMR_ASSERT_NONCRITICAL`,
//! `SMR_PTR_GET`, `SMR_PTR_GET_LOCKED` and `SMR_PTR_SET_LOCKED` (as [`SmrPtr`]); M11e the
//! singly-linked lists (`SMR_SLIST_*`, [`SmrSlistHead`]), for `bpf.c` and `if_pflow.c`. The
//! functions are in `kern/kern_smr.rs`.
//!
//! ## Deviations
//! - `SMR_LIST_*` and `SMR_TAILQ_*` are not ported: their C users (`if_pppoe.c`,
//!   `if_etherbridge.c`, carp) are not configured.
//! - `SMR_SLIST_*` follow `<sys/queue.h>`'s port: an [`SmrSlistHead`] over an adapter
//!   (`queue_adapter!`) whose entry is an [`SmrSlistEntry`]; the links are atomics, read
//!   with `Acquire` by the readers (`first`, `next`, `iter`) and by the writer
//!   (`first_locked`, ...), and published with `Release` (the C's `membar_producer`).
//!   A removed element keeps its link, as in C, so a reader standing on it goes on.
//! - `smr_call` (and `smr_call_impl`, `kern_smr.rs`) take a `&'static SmrEntry`: the entry
//!   must outlive the deferral (`smr_barrier_impl` lends its stack entry as `'static`
//!   while it waits for the call).
//! - `SMR_PTR_GET` is an `Acquire` load where the C's `READ_ONCE` relies on the hardware's
//!   dependency ordering, which Rust's memory model does not offer; `SMR_PTR_SET_LOCKED` is a
//!   `Release` store (the C's `membar_producer` then `WRITE_ONCE`).

use core::cell::Cell;
use core::ffi::c_void;
use core::marker::PhantomData;
use core::ptr;
use core::sync::atomic::{AtomicPtr, Ordering};

use crate::queue_adapter;
use crate::sys::queue::{Adapter, SimpleqEntry};

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

/// `SMR_SLIST_ENTRY(type)`: the link an element embeds to be in an SMR singly-linked list.
pub struct SmrSlistEntry<T> {
    /// `smr_sle_next`: next element, SMR-protected.
    smr_sle_next: AtomicPtr<T>,
}

impl<T> SmrSlistEntry<T> {
    /// An entry that is in no list (all-zero, as the C's `M_ZERO` elements).
    pub const fn new() -> Self {
        Self {
            smr_sle_next: AtomicPtr::new(ptr::null_mut()),
        }
    }
}

impl<T> Default for SmrSlistEntry<T> {
    fn default() -> Self {
        Self::new()
    }
}

/// `SMR_SLIST_HEAD(name, type)`: a singly-linked list that readers walk inside SMR read
/// sections while one writer, holding its lock, changes it. A removed element must not be
/// freed before a grace period (`smr_call`, `smr_barrier`).
pub struct SmrSlistHead<A: Adapter> {
    /// `smr_slh_first`: first element, SMR-protected.
    smr_slh_first: AtomicPtr<A::Elem>,
}

impl<A: SmrSlistAdapter> SmrSlistHead<A> {
    /// `SMR_SLIST_HEAD_INITIALIZER`: an empty list.
    pub const fn new() -> Self {
        Self {
            smr_slh_first: AtomicPtr::new(ptr::null_mut()),
        }
    }

    /// `SMR_SLIST_INIT`.
    pub fn init(&self) {
        self.smr_slh_first.store(ptr::null_mut(), Ordering::Release);
    }

    /// `SMR_SLIST_FIRST`: for a reader inside a read section.
    pub fn first(&self) -> Option<&A::Elem> {
        // SAFETY: a linked element, or one unlinked less than a grace period ago, is valid
        // while the reader stays in its read section (the writers' contract).
        unsafe { self.smr_slh_first.load(Ordering::Acquire).as_ref() }
    }

    /// `SMR_SLIST_NEXT`: for a reader inside a read section.
    pub fn next(elem: &A::Elem) -> Option<&A::Elem> {
        // SAFETY: as for `first`.
        unsafe { A::entry(elem).smr_sle_next.load(Ordering::Acquire).as_ref() }
    }

    /// `SMR_SLIST_FOREACH`: for a reader inside a read section.
    pub fn iter(&self) -> SmrSlistIter<'_, A> {
        SmrSlistIter {
            cur: self.smr_slh_first.load(Ordering::Acquire),
            _head: PhantomData,
        }
    }

    /// `SMR_SLIST_FIRST_LOCKED`: for the writer that holds the list's lock.
    pub fn first_locked(&self) -> Option<&A::Elem> {
        // SAFETY: a linked element is valid until unlinked (the mutators' contract).
        unsafe { self.smr_slh_first.load(Ordering::Acquire).as_ref() }
    }

    /// `SMR_SLIST_EMPTY_LOCKED`.
    pub fn is_empty_locked(&self) -> bool {
        self.smr_slh_first.load(Ordering::Acquire).is_null()
    }

    /// `SMR_SLIST_FOREACH_LOCKED` and `SMR_SLIST_FOREACH_SAFE_LOCKED`: for the writer.
    pub fn iter_locked(&self) -> SmrSlistIter<'_, A> {
        self.iter()
    }

    /// `SMR_SLIST_INSERT_HEAD_LOCKED`: links `elem` first, once its link is written.
    ///
    /// # Safety
    ///
    /// The caller holds the list's lock; `elem` is in no list of `A` and stays valid and in
    /// place until it is unlinked and a grace period has passed.
    pub unsafe fn insert_head_locked(&self, elem: &A::Elem) {
        A::entry(elem).smr_sle_next.store(
            self.smr_slh_first.load(Ordering::Relaxed),
            Ordering::Relaxed,
        );
        self.smr_slh_first
            .store(ptr::from_ref(elem).cast_mut(), Ordering::Release);
    }

    /// `SMR_SLIST_REMOVE_LOCKED`: unlinks `elem`, whose own link is left intact so that
    /// concurrent readers standing on it go on.
    ///
    /// # Safety
    ///
    /// The caller holds the list's lock and `elem` is in this list.
    pub unsafe fn remove_locked(&self, elem: &A::Elem) {
        let target = ptr::from_ref(elem).cast_mut();
        let next = A::entry(elem).smr_sle_next.load(Ordering::Relaxed);
        if self.smr_slh_first.load(Ordering::Relaxed) == target {
            self.smr_slh_first.store(next, Ordering::Release);
            return;
        }
        // SAFETY: `elem` is in the list and is not first, so the walk from the first element
        // reaches its predecessor through valid, linked elements.
        unsafe {
            let mut cur = &*self.smr_slh_first.load(Ordering::Relaxed);
            while A::entry(cur).smr_sle_next.load(Ordering::Relaxed) != target {
                cur = &*A::entry(cur).smr_sle_next.load(Ordering::Relaxed);
            }
            A::entry(cur).smr_sle_next.store(next, Ordering::Release);
        }
    }
}

impl<A: SmrSlistAdapter> Default for SmrSlistHead<A> {
    fn default() -> Self {
        Self::new()
    }
}

/// Forward iterator over an [`SmrSlistHead`]; the next element is read before the current
/// one is yielded (the `_SAFE` variant).
pub struct SmrSlistIter<'a, A: Adapter> {
    cur: *mut A::Elem,
    _head: PhantomData<&'a SmrSlistHead<A>>,
}

impl<'a, A: SmrSlistAdapter> Iterator for SmrSlistIter<'a, A> {
    type Item = &'a A::Elem;

    fn next(&mut self) -> Option<&'a A::Elem> {
        // SAFETY: as for `SmrSlistHead::first`.
        let cur = unsafe { self.cur.as_ref()? };
        self.cur = A::entry(cur).smr_sle_next.load(Ordering::Acquire);
        Some(cur)
    }
}

/// An [`Adapter`] whose entry is an [`SmrSlistEntry`].
pub trait SmrSlistAdapter: Adapter<Entry = SmrSlistEntry<<Self as Adapter>::Elem>> {}
impl<A: Adapter<Entry = SmrSlistEntry<<A as Adapter>::Elem>>> SmrSlistAdapter for A {}

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
/* </CODE> */

/* <TESTS> */
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

    #[test]
    fn smr_slist_links_and_unlinks() {
        struct E {
            link: SmrSlistEntry<E>,
            v: i32,
        }
        queue_adapter!(EList: E, link => SmrSlistEntry<E>);
        let (a, b, c) = (
            E {
                link: SmrSlistEntry::new(),
                v: 1,
            },
            E {
                link: SmrSlistEntry::new(),
                v: 2,
            },
            E {
                link: SmrSlistEntry::new(),
                v: 3,
            },
        );
        let head: SmrSlistHead<EList> = SmrSlistHead::new();
        assert!(head.is_empty_locked());
        // SAFETY: the elements outlive the list and are linked once.
        unsafe {
            head.insert_head_locked(&a);
            head.insert_head_locked(&b);
            head.insert_head_locked(&c);
        }
        let v: std::vec::Vec<i32> = head.iter().map(|e| e.v).collect();
        assert_eq!(v, [3, 2, 1]);
        // SAFETY: `b` is in the list.
        unsafe { head.remove_locked(&b) };
        // A reader standing on the removed element still reaches the rest.
        assert!(SmrSlistHead::<EList>::next(&b).is_some_and(|e| e.v == 1));
        let v: std::vec::Vec<i32> = head.iter_locked().map(|e| e.v).collect();
        assert_eq!(v, [3, 1]);
        // SAFETY: `c` is first.
        unsafe { head.remove_locked(&c) };
        assert!(head.first().is_some_and(|e| e.v == 1));
        assert!(head.first_locked().is_some_and(|e| e.v == 1));
    }
}
/* </TESTS> */
