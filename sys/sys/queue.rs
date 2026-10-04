/*	$OpenBSD: queue.h,v 1.47 2026/06/12 01:04:42 millert Exp $	*/
/*	$NetBSD: queue.h,v 1.11 1996/05/16 05:17:14 mycroft Exp $	*/
/* <LICENSES> */
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

//! Intrusive lists and queues: `<sys/queue.h>`, see `queue(3)`.
//!
//! Upstream: sys/sys/queue.h @ 3ce1f3f79392
//!
//! Six families, as in C: singly-linked lists ([`SlistHead`]), lists ([`ListHead`]), simple
//! queues ([`SimpleqHead`]), XOR simple queues ([`XsimpleqHead`]), tail queues ([`TailqHead`])
//! and singly-linked tail queues ([`StailqHead`]). An element takes part in a list by embedding
//! the matching entry type ([`SlistEntry`], ...) as a field. The lists are intrusive: they never
//! allocate, unlinking is O(1) where the C says so, and one element can sit in several lists.
//!
//! What C spells `LIST_HEAD(name, type)` plus a `field` argument on every macro is here an
//! [`Adapter`]: a zero-sized type, made by [`crate::queue_adapter!`], that names the element type and
//! the entry field. A head is generic over it (`ListHead<ProcList>`), so the field is fixed once
//! and checked by the compiler. Operation names are the C names without the family prefix:
//! `TAILQ_INSERT_TAIL(head, elm, field)` is `head.insert_tail(elm)`; operations that take no
//! head in C (`LIST_REMOVE`, `LIST_INSERT_BEFORE`, `SLIST_NEXT`) are associated functions,
//! `ListHead::<A>::remove(elm)`.
//!
//! Links are `Cell`s, so a list is modified through `&Elem` and `&Head`, like the C that mutates
//! through pointers under a lock. The lock discipline stays the caller's, as in C: readers are
//! safe functions whose references live as long as the borrow they come from; every mutator is
//! `unsafe` and states what the caller guarantees. The one invariant behind all of them: an
//! element linked into a list stays valid and in place until it is unlinked.
//!
//! ## Deviations
//! - `_FOREACH` and `_FOREACH_SAFE` are one iterator: it reads the next element before yielding
//!   the current one, so the current element may be unlinked while iterating (the `_SAFE` form).
//! - `*_HEAD_INITIALIZER` cannot take the head's own address in a `const`; an empty head stores
//!   a null "last" link that means "the head's first cell", so an empty head can be moved. Once
//!   an element is inserted the head is pinned, as in C.
//! - `TAILQ_PREV`, `TAILQ_LAST` and `STAILQ_LAST` recover the element from a link through the
//!   adapter's field offset (`container_of`) instead of C's type-punning cast of the link as a
//!   head; `prev` therefore takes the head.
//! - `XSIMPLEQ_INIT` takes the cookie as an argument until `arc4random(9)` exists (milestone M5);
//!   a head made by `new` has cookie 0 and works like a plain simple queue.
//! - `_Q_INVALIDATE` is active under feature `diagnostic` (OpenBSD's `option DIAGNOSTIC`).

use core::cell::Cell;
use core::marker::PhantomData;
use core::ptr;

/// `_Q_INVALID`: the poison written into a removed element's links under feature `diagnostic`.
#[cfg(feature = "diagnostic")]
const Q_INVALID: usize = usize::MAX;

/// Generates a zero-sized [`Adapter`] type: `queue_adapter!(pub ProcList: Proc, p_list =>
/// ListEntry<Proc>)` says that `ProcList` lists `Proc`s through their `p_list` field.
#[macro_export]
macro_rules! queue_adapter {
    ($(#[$meta:meta])* $vis:vis $name:ident: $elem:ty, $field:ident => $entry:ty) => {
        $(#[$meta])*
        $vis struct $name;

        // SAFETY: `entry` projects the named field and nothing else, and `OFFSET` is that
        // field's offset, so the two agree.
        unsafe impl $crate::sys::queue::Adapter for $name {
            type Elem = $elem;
            type Entry = $entry;
            const OFFSET: usize = ::core::mem::offset_of!($elem, $field);

            fn entry(elem: &$elem) -> &$entry {
                &elem.$field
            }
        }
    };
}

/*
 * Singly-linked List definitions.
 */

/// `SLIST_ENTRY(type)`: the link an element embeds to be in a singly-linked list.
#[repr(C)]
pub struct SlistEntry<T> {
    sle_next: Cell<*const T>,
}

impl<T> SlistEntry<T> {
    /// An entry that is in no list.
    pub const fn new() -> Self {
        Self {
            sle_next: Cell::new(ptr::null()),
        }
    }
}

impl<T> Default for SlistEntry<T> {
    fn default() -> Self {
        Self::new()
    }
}

/// `SLIST_HEAD(name, type)`: a singly-linked list of `A::Elem`, linked through the entry `A`
/// names. Elements are added at the head or after another element; removing an arbitrary
/// element costs O(n); traversal is forward only.
pub struct SlistHead<A: Adapter> {
    slh_first: Cell<*const A::Elem>,
}

impl<A: SlistAdapter> SlistHead<A> {
    /// `SLIST_HEAD_INITIALIZER`: an empty list.
    pub const fn new() -> Self {
        Self {
            slh_first: Cell::new(ptr::null()),
        }
    }

    /// `SLIST_INIT`: empties the list without touching the elements.
    pub fn init(&self) {
        self.slh_first.set(ptr::null());
    }

    /// `SLIST_FIRST`; `None` is `SLIST_END`.
    pub fn first(&self) -> Option<&A::Elem> {
        // SAFETY: a linked element is valid until unlinked (the mutators' contract).
        unsafe { self.slh_first.get().as_ref() }
    }

    /// `SLIST_EMPTY`.
    pub fn is_empty(&self) -> bool {
        self.slh_first.get().is_null()
    }

    /// `SLIST_NEXT`: the element after `elem`.
    pub fn next(elem: &A::Elem) -> Option<&A::Elem> {
        // SAFETY: as for `first`.
        unsafe { A::entry(elem).sle_next.get().as_ref() }
    }

    /// `SLIST_FOREACH` and `SLIST_FOREACH_SAFE`.
    pub fn iter(&self) -> SlistIter<'_, A> {
        SlistIter {
            cur: self.slh_first.get(),
            _head: PhantomData,
        }
    }

    /// `SLIST_INSERT_HEAD`.
    ///
    /// # Safety
    ///
    /// `elem` is in no list of `A` and stays valid and in place until it is unlinked.
    pub unsafe fn insert_head(&self, elem: &A::Elem) {
        A::entry(elem).sle_next.set(self.slh_first.get());
        self.slh_first.set(elem);
    }

    /// `SLIST_INSERT_AFTER`: links `elem` after `slistelm`.
    ///
    /// # Safety
    ///
    /// `slistelm` is linked; `elem` is in no list of `A` and stays valid and in place until it
    /// is unlinked.
    pub unsafe fn insert_after(slistelm: &A::Elem, elem: &A::Elem) {
        let after = A::entry(slistelm);
        A::entry(elem).sle_next.set(after.sle_next.get());
        after.sle_next.set(elem);
    }

    /// `SLIST_REMOVE_HEAD`: unlinks the first element.
    ///
    /// # Safety
    ///
    /// The list is not empty.
    pub unsafe fn remove_head(&self) {
        // SAFETY: the caller guarantees a first element exists; it is valid while linked.
        let first = unsafe { &*self.slh_first.get() };
        self.slh_first.set(A::entry(first).sle_next.get());
    }

    /// `SLIST_REMOVE_AFTER`: unlinks the element after `elem`.
    ///
    /// # Safety
    ///
    /// `elem` is linked and has a successor.
    pub unsafe fn remove_after(elem: &A::Elem) {
        let entry = A::entry(elem);
        // SAFETY: the caller guarantees a successor exists; it is valid while linked.
        let next = unsafe { &*entry.sle_next.get() };
        entry.sle_next.set(A::entry(next).sle_next.get());
    }

    /// `SLIST_REMOVE`: unlinks `elem`, walking the list to find its predecessor (O(n)).
    ///
    /// # Safety
    ///
    /// `elem` is in this list.
    pub unsafe fn remove(&self, elem: &A::Elem) {
        let target: *const A::Elem = elem;
        if self.slh_first.get() == target {
            // SAFETY: the list holds `elem`, so it is not empty.
            unsafe { self.remove_head() };
        } else {
            // SAFETY: `elem` is in the list and is not first, so the walk from the first element
            // reaches its predecessor through valid, linked elements.
            unsafe {
                let mut cur = &*self.slh_first.get();
                while A::entry(cur).sle_next.get() != target {
                    cur = &*A::entry(cur).sle_next.get();
                }
                let before = A::entry(cur);
                before
                    .sle_next
                    .set(A::entry(&*before.sle_next.get()).sle_next.get());
            }
        }
        invalidate(&A::entry(elem).sle_next);
    }
}

impl<A: SlistAdapter> Default for SlistHead<A> {
    fn default() -> Self {
        Self::new()
    }
}

/// Forward iterator over a [`SlistHead`]; see the module docs for its `_SAFE` behaviour.
pub struct SlistIter<'a, A: Adapter> {
    cur: *const A::Elem,
    _head: PhantomData<&'a SlistHead<A>>,
}

impl<'a, A: SlistAdapter> Iterator for SlistIter<'a, A> {
    type Item = &'a A::Elem;

    fn next(&mut self) -> Option<&'a A::Elem> {
        // SAFETY: a linked element is valid until unlinked; the following element is read now
        // so the caller may unlink the one yielded.
        let cur = unsafe { self.cur.as_ref()? };
        self.cur = A::entry(cur).sle_next.get();
        Some(cur)
    }
}

/*
 * List definitions.
 */

/// `LIST_ENTRY(type)`: the link an element embeds to be in a list. `le_prev` is the address of
/// the previous element's `le_next` (or of the head's `lh_first`), which makes unlinking O(1).
#[repr(C)]
pub struct ListEntry<T> {
    le_next: Cell<*const T>,
    le_prev: Cell<*const Cell<*const T>>,
}

impl<T> ListEntry<T> {
    /// An entry that is in no list.
    pub const fn new() -> Self {
        Self {
            le_next: Cell::new(ptr::null()),
            le_prev: Cell::new(ptr::null()),
        }
    }
}

impl<T> Default for ListEntry<T> {
    fn default() -> Self {
        Self::new()
    }
}

/// `LIST_HEAD(name, type)`: a doubly-linked list of `A::Elem` with O(1) unlinking. Elements are
/// added at the head or before or after another element; traversal is forward only.
pub struct ListHead<A: Adapter> {
    lh_first: Cell<*const A::Elem>,
}

impl<A: ListAdapter> ListHead<A> {
    /// `LIST_HEAD_INITIALIZER`: an empty list.
    pub const fn new() -> Self {
        Self {
            lh_first: Cell::new(ptr::null()),
        }
    }

    /// `LIST_INIT`: empties the list without touching the elements.
    pub fn init(&self) {
        self.lh_first.set(ptr::null());
    }

    /// `LIST_FIRST`; `None` is `LIST_END`.
    pub fn first(&self) -> Option<&A::Elem> {
        // SAFETY: a linked element is valid until unlinked (the mutators' contract).
        unsafe { self.lh_first.get().as_ref() }
    }

    /// `LIST_EMPTY`.
    pub fn is_empty(&self) -> bool {
        self.lh_first.get().is_null()
    }

    /// `LIST_NEXT`: the element after `elem`.
    pub fn next(elem: &A::Elem) -> Option<&A::Elem> {
        // SAFETY: as for `first`.
        unsafe { A::entry(elem).le_next.get().as_ref() }
    }

    /// `LIST_FOREACH` and `LIST_FOREACH_SAFE`.
    pub fn iter(&self) -> ListIter<'_, A> {
        ListIter {
            cur: self.lh_first.get(),
            _head: PhantomData,
        }
    }

    /// `LIST_INSERT_HEAD`.
    ///
    /// # Safety
    ///
    /// `elem` is in no list of `A` and stays valid and in place until it is unlinked; the head
    /// stays in place while the list is not empty.
    pub unsafe fn insert_head(&self, elem: &A::Elem) {
        let entry = A::entry(elem);
        let first = self.lh_first.get();
        entry.le_next.set(first);
        // SAFETY: a linked element is valid until unlinked.
        if let Some(first) = unsafe { first.as_ref() } {
            A::entry(first).le_prev.set(&entry.le_next);
        }
        self.lh_first.set(elem);
        entry.le_prev.set(&self.lh_first);
    }

    /// `LIST_INSERT_AFTER`: links `elem` after `listelm`.
    ///
    /// # Safety
    ///
    /// `listelm` is linked; `elem` is in no list of `A` and stays valid and in place until it is
    /// unlinked.
    pub unsafe fn insert_after(listelm: &A::Elem, elem: &A::Elem) {
        let after = A::entry(listelm);
        let entry = A::entry(elem);
        let next = after.le_next.get();
        entry.le_next.set(next);
        // SAFETY: a linked element is valid until unlinked.
        if let Some(next) = unsafe { next.as_ref() } {
            A::entry(next).le_prev.set(&entry.le_next);
        }
        after.le_next.set(elem);
        entry.le_prev.set(&after.le_next);
    }

    /// `LIST_INSERT_BEFORE`: links `elem` before `listelm`.
    ///
    /// # Safety
    ///
    /// `listelm` is linked; `elem` is in no list of `A` and stays valid and in place until it is
    /// unlinked.
    pub unsafe fn insert_before(listelm: &A::Elem, elem: &A::Elem) {
        let before = A::entry(listelm);
        let entry = A::entry(elem);
        entry.le_prev.set(before.le_prev.get());
        entry.le_next.set(listelm);
        // SAFETY: a linked element's `le_prev` points at a live cell: the previous element's
        // `le_next` or the pinned head's `lh_first`.
        unsafe { (*before.le_prev.get()).set(elem) };
        before.le_prev.set(&entry.le_next);
    }

    /// `LIST_REMOVE`: unlinks `elem` in O(1).
    ///
    /// # Safety
    ///
    /// `elem` is in a list of `A`.
    pub unsafe fn remove(elem: &A::Elem) {
        let entry = A::entry(elem);
        // SAFETY: a linked element is valid until unlinked, and its `le_prev` points at a live
        // cell (see `insert_before`).
        unsafe {
            if let Some(next) = entry.le_next.get().as_ref() {
                A::entry(next).le_prev.set(entry.le_prev.get());
            }
            (*entry.le_prev.get()).set(entry.le_next.get());
        }
        invalidate(&entry.le_prev);
        invalidate(&entry.le_next);
    }

    /// `LIST_REPLACE`: puts `elem2` where `elem` is and unlinks `elem`.
    ///
    /// # Safety
    ///
    /// `elem` is in a list of `A`; `elem2` is in no list of `A` and stays valid and in place
    /// until it is unlinked.
    pub unsafe fn replace(elem: &A::Elem, elem2: &A::Elem) {
        let old = A::entry(elem);
        let new = A::entry(elem2);
        new.le_next.set(old.le_next.get());
        // SAFETY: as for `remove`.
        unsafe {
            if let Some(next) = new.le_next.get().as_ref() {
                A::entry(next).le_prev.set(&new.le_next);
            }
            new.le_prev.set(old.le_prev.get());
            (*new.le_prev.get()).set(elem2);
        }
        invalidate(&old.le_prev);
        invalidate(&old.le_next);
    }
}

impl<A: ListAdapter> Default for ListHead<A> {
    fn default() -> Self {
        Self::new()
    }
}

/// Forward iterator over a [`ListHead`]; see the module docs for its `_SAFE` behaviour.
pub struct ListIter<'a, A: Adapter> {
    cur: *const A::Elem,
    _head: PhantomData<&'a ListHead<A>>,
}

impl<'a, A: ListAdapter> Iterator for ListIter<'a, A> {
    type Item = &'a A::Elem;

    fn next(&mut self) -> Option<&'a A::Elem> {
        // SAFETY: as for `SlistIter`.
        let cur = unsafe { self.cur.as_ref()? };
        self.cur = A::entry(cur).le_next.get();
        Some(cur)
    }
}

/*
 * Simple queue definitions.
 */

/// `SIMPLEQ_ENTRY(type)`: the link an element embeds to be in a simple queue.
#[repr(C)]
pub struct SimpleqEntry<T> {
    sqe_next: Cell<*const T>,
}

impl<T> SimpleqEntry<T> {
    /// An entry that is in no queue.
    pub const fn new() -> Self {
        Self {
            sqe_next: Cell::new(ptr::null()),
        }
    }
}

impl<T> Default for SimpleqEntry<T> {
    fn default() -> Self {
        Self::new()
    }
}

/// `SIMPLEQ_HEAD(name, type)`: a singly-linked queue of `A::Elem` with a tail pointer. Elements
/// are added at either end or after another element and removed from the head or after another
/// element; traversal is forward only.
pub struct SimpleqHead<A: Adapter> {
    sqh_first: Cell<*const A::Elem>,
    /// Address of the last element's `sqe_next`; null stands for `&sqh_first` (empty queue).
    sqh_last: Cell<*const Cell<*const A::Elem>>,
}

impl<A: SimpleqAdapter> SimpleqHead<A> {
    /// `SIMPLEQ_HEAD_INITIALIZER`: an empty queue.
    pub const fn new() -> Self {
        Self {
            sqh_first: Cell::new(ptr::null()),
            sqh_last: Cell::new(ptr::null()),
        }
    }

    /// `SIMPLEQ_INIT`: empties the queue without touching the elements.
    pub fn init(&self) {
        self.sqh_first.set(ptr::null());
        self.sqh_last.set(ptr::null());
    }

    /// `SIMPLEQ_FIRST`; `None` is `SIMPLEQ_END`.
    pub fn first(&self) -> Option<&A::Elem> {
        // SAFETY: a linked element is valid until unlinked (the mutators' contract).
        unsafe { self.sqh_first.get().as_ref() }
    }

    /// `SIMPLEQ_EMPTY`.
    pub fn is_empty(&self) -> bool {
        self.sqh_first.get().is_null()
    }

    /// `SIMPLEQ_NEXT`: the element after `elem`.
    pub fn next(elem: &A::Elem) -> Option<&A::Elem> {
        // SAFETY: as for `first`.
        unsafe { A::entry(elem).sqe_next.get().as_ref() }
    }

    /// `SIMPLEQ_FOREACH` and `SIMPLEQ_FOREACH_SAFE`.
    pub fn iter(&self) -> SimpleqIter<'_, A> {
        SimpleqIter {
            cur: self.sqh_first.get(),
            _head: PhantomData,
        }
    }

    /// The cell `sqh_last` designates: the last element's `sqe_next`, or `sqh_first`.
    fn last_link(&self) -> &Cell<*const A::Elem> {
        let last = self.sqh_last.get();
        if last.is_null() {
            &self.sqh_first
        } else {
            // SAFETY: a non-null `sqh_last` points at the `sqe_next` of a linked element, valid
            // until unlinked.
            unsafe { &*last }
        }
    }

    /// `SIMPLEQ_INSERT_HEAD`.
    ///
    /// # Safety
    ///
    /// `elem` is in no queue of `A` and stays valid and in place until it is unlinked.
    pub unsafe fn insert_head(&self, elem: &A::Elem) {
        let entry = A::entry(elem);
        let first = self.sqh_first.get();
        entry.sqe_next.set(first);
        if first.is_null() {
            self.sqh_last.set(&entry.sqe_next);
        }
        self.sqh_first.set(elem);
    }

    /// `SIMPLEQ_INSERT_TAIL`.
    ///
    /// # Safety
    ///
    /// As for [`insert_head`](Self::insert_head).
    pub unsafe fn insert_tail(&self, elem: &A::Elem) {
        let entry = A::entry(elem);
        entry.sqe_next.set(ptr::null());
        self.last_link().set(elem);
        self.sqh_last.set(&entry.sqe_next);
    }

    /// `SIMPLEQ_INSERT_AFTER`: links `elem` after `listelm`.
    ///
    /// # Safety
    ///
    /// `listelm` is in this queue; `elem` is in no queue of `A` and stays valid and in place
    /// until it is unlinked.
    pub unsafe fn insert_after(&self, listelm: &A::Elem, elem: &A::Elem) {
        let after = A::entry(listelm);
        let entry = A::entry(elem);
        let next = after.sqe_next.get();
        entry.sqe_next.set(next);
        if next.is_null() {
            self.sqh_last.set(&entry.sqe_next);
        }
        after.sqe_next.set(elem);
    }

    /// `SIMPLEQ_REMOVE_HEAD`: unlinks the first element.
    ///
    /// # Safety
    ///
    /// The queue is not empty.
    pub unsafe fn remove_head(&self) {
        // SAFETY: the caller guarantees a first element exists; it is valid while linked.
        let first = unsafe { &*self.sqh_first.get() };
        let next = A::entry(first).sqe_next.get();
        self.sqh_first.set(next);
        if next.is_null() {
            self.sqh_last.set(ptr::null());
        }
    }

    /// `SIMPLEQ_REMOVE_AFTER`: unlinks the element after `elem`.
    ///
    /// # Safety
    ///
    /// `elem` is in this queue and has a successor.
    pub unsafe fn remove_after(&self, elem: &A::Elem) {
        let entry = A::entry(elem);
        // SAFETY: the caller guarantees a successor exists; it is valid while linked.
        let next = unsafe { &*entry.sqe_next.get() };
        let after_next = A::entry(next).sqe_next.get();
        entry.sqe_next.set(after_next);
        if after_next.is_null() {
            self.sqh_last.set(&entry.sqe_next);
        }
    }

    /// `SIMPLEQ_CONCAT`: moves every element of `head2` to the end of this queue.
    ///
    /// # Safety
    ///
    /// Both heads stay in place while their queues are not empty.
    pub unsafe fn concat(&self, head2: &Self) {
        if !head2.is_empty() {
            self.last_link().set(head2.sqh_first.get());
            self.sqh_last.set(head2.sqh_last.get());
            head2.init();
        }
    }
}

impl<A: SimpleqAdapter> Default for SimpleqHead<A> {
    fn default() -> Self {
        Self::new()
    }
}

/// Forward iterator over a [`SimpleqHead`]; see the module docs for its `_SAFE` behaviour.
pub struct SimpleqIter<'a, A: Adapter> {
    cur: *const A::Elem,
    _head: PhantomData<&'a SimpleqHead<A>>,
}

impl<'a, A: SimpleqAdapter> Iterator for SimpleqIter<'a, A> {
    type Item = &'a A::Elem;

    fn next(&mut self) -> Option<&'a A::Elem> {
        // SAFETY: as for `SlistIter`.
        let cur = unsafe { self.cur.as_ref()? };
        self.cur = A::entry(cur).sqe_next.get();
        Some(cur)
    }
}

/*
 * XOR Simple queue definitions.
 */

/// `XSIMPLEQ_ENTRY(type)`: the link an element embeds to be in an XOR simple queue. The stored
/// value is the next pointer XOR'd with the head's cookie.
#[repr(C)]
pub struct XsimpleqEntry<T> {
    sqx_next: Cell<usize>,
    _elem: PhantomData<*const T>,
}

impl<T> XsimpleqEntry<T> {
    /// An entry that is in no queue.
    pub const fn new() -> Self {
        Self {
            sqx_next: Cell::new(0),
            _elem: PhantomData,
        }
    }
}

impl<T> Default for XsimpleqEntry<T> {
    fn default() -> Self {
        Self::new()
    }
}

/// `XSIMPLEQ_HEAD(name, type)`: a simple queue whose pointers are XOR'd with a per-queue random
/// cookie, so that corrupted or forged links do not dereference to anything useful. Used like a
/// [`SimpleqHead`]; `XSIMPLEQ_XOR` is [`xor`](Self::xor).
pub struct XsimpleqHead<A: Adapter> {
    sqx_first: Cell<usize>,
    /// XOR'd address of the last element's `sqx_next`; the XOR'd null stands for `&sqx_first`.
    sqx_last: Cell<usize>,
    sqx_cookie: Cell<usize>,
    _adapter: PhantomData<A>,
}

impl<A: XsimpleqAdapter> XsimpleqHead<A> {
    /// An empty queue with cookie 0, usable as a plain simple queue; [`init`](Self::init) gives it
    /// a real cookie.
    pub const fn new() -> Self {
        Self {
            sqx_first: Cell::new(0),
            sqx_last: Cell::new(0),
            sqx_cookie: Cell::new(0),
            _adapter: PhantomData,
        }
    }

    /// `XSIMPLEQ_INIT`: empties the queue and sets its cookie. The C draws the cookie from
    /// `arc4random_buf`; until that exists the caller supplies it.
    pub fn init(&self, cookie: usize) {
        self.sqx_cookie.set(cookie);
        self.sqx_first.set(self.xor(ptr::null::<A::Elem>()));
        self.sqx_last.set(self.xor(ptr::null::<Cell<usize>>()));
    }

    /// `XSIMPLEQ_XOR`: encodes or decodes a pointer with the cookie.
    pub fn xor<P>(&self, p: *const P) -> usize {
        self.sqx_cookie.get() ^ (p as usize)
    }

    fn decode<P>(&self, v: usize) -> *const P {
        (self.sqx_cookie.get() ^ v) as *const P
    }

    /// `XSIMPLEQ_FIRST`; `None` is `XSIMPLEQ_END`.
    pub fn first(&self) -> Option<&A::Elem> {
        // SAFETY: a linked element is valid until unlinked (the mutators' contract).
        unsafe { self.decode::<A::Elem>(self.sqx_first.get()).as_ref() }
    }

    /// `XSIMPLEQ_EMPTY`.
    pub fn is_empty(&self) -> bool {
        self.decode::<A::Elem>(self.sqx_first.get()).is_null()
    }

    /// `XSIMPLEQ_NEXT`: the element after `elem`.
    pub fn next<'a>(&self, elem: &'a A::Elem) -> Option<&'a A::Elem> {
        // SAFETY: as for `first`.
        unsafe {
            self.decode::<A::Elem>(A::entry(elem).sqx_next.get())
                .as_ref()
        }
    }

    /// `XSIMPLEQ_FOREACH` and `XSIMPLEQ_FOREACH_SAFE`.
    pub fn iter(&self) -> XsimpleqIter<'_, A> {
        XsimpleqIter {
            head: self,
            cur: self.decode(self.sqx_first.get()),
        }
    }

    /// The cell `sqx_last` designates: the last element's `sqx_next`, or `sqx_first`.
    fn last_link(&self) -> &Cell<usize> {
        let last = self.decode::<Cell<usize>>(self.sqx_last.get());
        if last.is_null() {
            &self.sqx_first
        } else {
            // SAFETY: a non-null decoded `sqx_last` points at the `sqx_next` of a linked
            // element, valid until unlinked.
            unsafe { &*last }
        }
    }

    /// `XSIMPLEQ_INSERT_HEAD`.
    ///
    /// # Safety
    ///
    /// `elem` is in no queue of `A` and stays valid and in place until it is unlinked.
    pub unsafe fn insert_head(&self, elem: &A::Elem) {
        let entry = A::entry(elem);
        let first = self.sqx_first.get();
        entry.sqx_next.set(first);
        if first == self.xor(ptr::null::<A::Elem>()) {
            self.sqx_last.set(self.xor(&entry.sqx_next));
        }
        self.sqx_first.set(self.xor(elem));
    }

    /// `XSIMPLEQ_INSERT_TAIL`.
    ///
    /// # Safety
    ///
    /// As for [`insert_head`](Self::insert_head).
    pub unsafe fn insert_tail(&self, elem: &A::Elem) {
        let entry = A::entry(elem);
        entry.sqx_next.set(self.xor(ptr::null::<A::Elem>()));
        self.last_link().set(self.xor(elem));
        self.sqx_last.set(self.xor(&entry.sqx_next));
    }

    /// `XSIMPLEQ_INSERT_AFTER`: links `elem` after `listelm`.
    ///
    /// # Safety
    ///
    /// `listelm` is in this queue; `elem` is in no queue of `A` and stays valid and in place
    /// until it is unlinked.
    pub unsafe fn insert_after(&self, listelm: &A::Elem, elem: &A::Elem) {
        let after = A::entry(listelm);
        let entry = A::entry(elem);
        let next = after.sqx_next.get();
        entry.sqx_next.set(next);
        if next == self.xor(ptr::null::<A::Elem>()) {
            self.sqx_last.set(self.xor(&entry.sqx_next));
        }
        after.sqx_next.set(self.xor(elem));
    }

    /// `XSIMPLEQ_REMOVE_HEAD`: unlinks the first element.
    ///
    /// # Safety
    ///
    /// The queue is not empty.
    pub unsafe fn remove_head(&self) {
        // SAFETY: the caller guarantees a first element exists; it is valid while linked.
        let first = unsafe { &*self.decode::<A::Elem>(self.sqx_first.get()) };
        let next = A::entry(first).sqx_next.get();
        self.sqx_first.set(next);
        if next == self.xor(ptr::null::<A::Elem>()) {
            self.sqx_last.set(self.xor(ptr::null::<Cell<usize>>()));
        }
    }

    /// `XSIMPLEQ_REMOVE_AFTER`: unlinks the element after `elem`.
    ///
    /// # Safety
    ///
    /// `elem` is in this queue and has a successor.
    pub unsafe fn remove_after(&self, elem: &A::Elem) {
        let entry = A::entry(elem);
        // SAFETY: the caller guarantees a successor exists; it is valid while linked.
        let next = unsafe { &*self.decode::<A::Elem>(entry.sqx_next.get()) };
        let after_next = A::entry(next).sqx_next.get();
        entry.sqx_next.set(after_next);
        if after_next == self.xor(ptr::null::<A::Elem>()) {
            self.sqx_last.set(self.xor(&entry.sqx_next));
        }
    }
}

impl<A: XsimpleqAdapter> Default for XsimpleqHead<A> {
    fn default() -> Self {
        Self::new()
    }
}

/// Forward iterator over an [`XsimpleqHead`]; see the module docs for its `_SAFE` behaviour.
pub struct XsimpleqIter<'a, A: Adapter> {
    head: &'a XsimpleqHead<A>,
    cur: *const A::Elem,
}

impl<'a, A: XsimpleqAdapter> Iterator for XsimpleqIter<'a, A> {
    type Item = &'a A::Elem;

    fn next(&mut self) -> Option<&'a A::Elem> {
        // SAFETY: as for `SlistIter`.
        let cur = unsafe { self.cur.as_ref()? };
        self.cur = self.head.decode(A::entry(cur).sqx_next.get());
        Some(cur)
    }
}

/*
 * Tail queue definitions.
 */

/// `TAILQ_ENTRY(type)`: the link an element embeds to be in a tail queue. `tqe_prev` is the
/// address of the previous element's `tqe_next` (or of the head's `tqh_first`).
#[repr(C)]
pub struct TailqEntry<T> {
    tqe_next: Cell<*const T>,
    tqe_prev: Cell<*const Cell<*const T>>,
}

impl<T> TailqEntry<T> {
    /// An entry that is in no queue.
    pub const fn new() -> Self {
        Self {
            tqe_next: Cell::new(ptr::null()),
            tqe_prev: Cell::new(ptr::null()),
        }
    }
}

impl<T> TailqEntry<T> {
    /// `elm->field.tqe_prev != NULL`: the element is in a queue (or was marked so by
    /// [`set_prev_self`](Self::set_prev_self)). Meaningful only for code that clears the link
    /// after every removal ([`clear_prev`](Self::clear_prev)), as pf does with its rules.
    pub fn is_linked(&self) -> bool {
        !self.tqe_prev.get().is_null()
    }

    /// `elm->field.tqe_prev = NULL` after a `TAILQ_REMOVE`.
    ///
    /// # Safety
    ///
    /// The element is in no queue (it was just removed from its queue).
    pub unsafe fn clear_prev(&self) {
        self.tqe_prev.set(ptr::null());
    }

    /// `elm->field.tqe_prev = &elm->field.tqe_next`: marks an element that is in no queue
    /// as linked, so that [`is_linked`](Self::is_linked) holds for it forever (pf's default
    /// rule, "never garbage collected").
    ///
    /// # Safety
    ///
    /// The element is in no queue and is never inserted into or removed from one; it stays
    /// in place (a static).
    pub unsafe fn set_prev_self(&self) {
        self.tqe_prev.set(&self.tqe_next);
    }
}

impl<T> Default for TailqEntry<T> {
    fn default() -> Self {
        Self::new()
    }
}

/// `TAILQ_HEAD(name, type)`: a doubly-linked queue of `A::Elem` with a tail pointer: O(1)
/// insertion at either end or next to any element, O(1) unlinking, traversal both ways.
pub struct TailqHead<A: Adapter> {
    tqh_first: Cell<*const A::Elem>,
    /// Address of the last element's `tqe_next`; null stands for `&tqh_first` (empty queue).
    tqh_last: Cell<*const Cell<*const A::Elem>>,
}

impl<A: TailqAdapter> TailqHead<A> {
    /// `TAILQ_HEAD_INITIALIZER`: an empty queue.
    pub const fn new() -> Self {
        Self {
            tqh_first: Cell::new(ptr::null()),
            tqh_last: Cell::new(ptr::null()),
        }
    }

    /// `TAILQ_INIT`: empties the queue without touching the elements.
    pub fn init(&self) {
        self.tqh_first.set(ptr::null());
        self.tqh_last.set(ptr::null());
    }

    /// `TAILQ_FIRST`; `None` is `TAILQ_END`.
    pub fn first(&self) -> Option<&A::Elem> {
        // SAFETY: a linked element is valid until unlinked (the mutators' contract).
        unsafe { self.tqh_first.get().as_ref() }
    }

    /// `TAILQ_LAST`.
    pub fn last(&self) -> Option<&A::Elem> {
        let last = self.tqh_last.get();
        if last.is_null() {
            return None;
        }
        // SAFETY: a non-null `tqh_last` is the `tqe_next` cell of a linked element, valid until
        // unlinked; `container_of` steps back to that element.
        unsafe { container_of::<A>(last).as_ref() }
    }

    /// `TAILQ_EMPTY`.
    pub fn is_empty(&self) -> bool {
        self.tqh_first.get().is_null()
    }

    /// `TAILQ_NEXT`: the element after `elem`.
    pub fn next(elem: &A::Elem) -> Option<&A::Elem> {
        // SAFETY: as for `first`.
        unsafe { A::entry(elem).tqe_next.get().as_ref() }
    }

    /// `TAILQ_PREV`: the element before `elem`, which must be in this queue.
    pub fn prev<'a>(&self, elem: &'a A::Elem) -> Option<&'a A::Elem> {
        let prev = A::entry(elem).tqe_prev.get();
        if ptr::eq(prev, &self.tqh_first) {
            return None;
        }
        // SAFETY: a linked element's `tqe_prev` that is not the head's cell is the `tqe_next`
        // cell of the previous linked element; `container_of` steps back to it.
        unsafe { container_of::<A>(prev).as_ref() }
    }

    /// `TAILQ_FOREACH` and `TAILQ_FOREACH_SAFE`.
    pub fn iter(&self) -> TailqIter<'_, A> {
        TailqIter {
            cur: self.tqh_first.get(),
            _head: PhantomData,
        }
    }

    /// `TAILQ_FOREACH_REVERSE` and `TAILQ_FOREACH_REVERSE_SAFE`.
    pub fn iter_reverse(&self) -> TailqIterReverse<'_, A> {
        TailqIterReverse {
            head: self,
            cur: self.last().map_or(ptr::null(), |e| e),
        }
    }

    /// The cell `tqh_last` designates: the last element's `tqe_next`, or `tqh_first`.
    fn last_link(&self) -> &Cell<*const A::Elem> {
        let last = self.tqh_last.get();
        if last.is_null() {
            &self.tqh_first
        } else {
            // SAFETY: a non-null `tqh_last` points at the `tqe_next` of a linked element, valid
            // until unlinked.
            unsafe { &*last }
        }
    }

    /// Stores `link` as `tqh_last`, folding the head's own cell back into the null sentinel.
    fn set_last(&self, link: *const Cell<*const A::Elem>) {
        if ptr::eq(link, &self.tqh_first) {
            self.tqh_last.set(ptr::null());
        } else {
            self.tqh_last.set(link);
        }
    }

    /// `TAILQ_INSERT_HEAD`.
    ///
    /// # Safety
    ///
    /// `elem` is in no queue of `A` and stays valid and in place until it is unlinked; the head
    /// stays in place while the queue is not empty.
    pub unsafe fn insert_head(&self, elem: &A::Elem) {
        let entry = A::entry(elem);
        let first = self.tqh_first.get();
        entry.tqe_next.set(first);
        // SAFETY: a linked element is valid until unlinked.
        match unsafe { first.as_ref() } {
            Some(first) => A::entry(first).tqe_prev.set(&entry.tqe_next),
            None => self.tqh_last.set(&entry.tqe_next),
        }
        self.tqh_first.set(elem);
        entry.tqe_prev.set(&self.tqh_first);
    }

    /// `TAILQ_INSERT_TAIL`.
    ///
    /// # Safety
    ///
    /// As for [`insert_head`](Self::insert_head).
    pub unsafe fn insert_tail(&self, elem: &A::Elem) {
        let entry = A::entry(elem);
        entry.tqe_next.set(ptr::null());
        let last = self.last_link();
        entry.tqe_prev.set(last);
        last.set(elem);
        self.tqh_last.set(&entry.tqe_next);
    }

    /// `TAILQ_INSERT_AFTER`: links `elem` after `listelm`.
    ///
    /// # Safety
    ///
    /// `listelm` is in this queue; `elem` is in no queue of `A` and stays valid and in place
    /// until it is unlinked.
    pub unsafe fn insert_after(&self, listelm: &A::Elem, elem: &A::Elem) {
        let after = A::entry(listelm);
        let entry = A::entry(elem);
        let next = after.tqe_next.get();
        entry.tqe_next.set(next);
        // SAFETY: a linked element is valid until unlinked.
        match unsafe { next.as_ref() } {
            Some(next) => A::entry(next).tqe_prev.set(&entry.tqe_next),
            None => self.tqh_last.set(&entry.tqe_next),
        }
        after.tqe_next.set(elem);
        entry.tqe_prev.set(&after.tqe_next);
    }

    /// `TAILQ_INSERT_BEFORE`: links `elem` before `listelm`.
    ///
    /// # Safety
    ///
    /// `listelm` is linked; `elem` is in no queue of `A` and stays valid and in place until it
    /// is unlinked.
    pub unsafe fn insert_before(listelm: &A::Elem, elem: &A::Elem) {
        let before = A::entry(listelm);
        let entry = A::entry(elem);
        entry.tqe_prev.set(before.tqe_prev.get());
        entry.tqe_next.set(listelm);
        // SAFETY: a linked element's `tqe_prev` points at a live cell: the previous element's
        // `tqe_next` or the pinned head's `tqh_first`.
        unsafe { (*before.tqe_prev.get()).set(elem) };
        before.tqe_prev.set(&entry.tqe_next);
    }

    /// `TAILQ_REMOVE`: unlinks `elem` in O(1).
    ///
    /// # Safety
    ///
    /// `elem` is in this queue.
    pub unsafe fn remove(&self, elem: &A::Elem) {
        let entry = A::entry(elem);
        // SAFETY: a linked element is valid until unlinked, and its `tqe_prev` points at a live
        // cell (see `insert_before`).
        unsafe {
            match entry.tqe_next.get().as_ref() {
                Some(next) => A::entry(next).tqe_prev.set(entry.tqe_prev.get()),
                None => self.set_last(entry.tqe_prev.get()),
            }
            (*entry.tqe_prev.get()).set(entry.tqe_next.get());
        }
        invalidate(&entry.tqe_prev);
        invalidate(&entry.tqe_next);
    }

    /// `TAILQ_REPLACE`: puts `elem2` where `elem` is and unlinks `elem`.
    ///
    /// # Safety
    ///
    /// `elem` is in this queue; `elem2` is in no queue of `A` and stays valid and in place until
    /// it is unlinked.
    pub unsafe fn replace(&self, elem: &A::Elem, elem2: &A::Elem) {
        let old = A::entry(elem);
        let new = A::entry(elem2);
        new.tqe_next.set(old.tqe_next.get());
        // SAFETY: as for `remove`.
        unsafe {
            match new.tqe_next.get().as_ref() {
                Some(next) => A::entry(next).tqe_prev.set(&new.tqe_next),
                None => self.tqh_last.set(&new.tqe_next),
            }
            new.tqe_prev.set(old.tqe_prev.get());
            (*new.tqe_prev.get()).set(elem2);
        }
        invalidate(&old.tqe_prev);
        invalidate(&old.tqe_next);
    }

    /// `TAILQ_CONCAT`: moves every element of `head2` to the end of this queue.
    ///
    /// # Safety
    ///
    /// Both heads stay in place while their queues are not empty.
    pub unsafe fn concat(&self, head2: &Self) {
        if let Some(first2) = head2.first() {
            let last = self.last_link();
            last.set(first2);
            A::entry(first2).tqe_prev.set(last);
            self.tqh_last.set(head2.tqh_last.get());
            head2.init();
        }
    }
}

impl<A: TailqAdapter> Default for TailqHead<A> {
    fn default() -> Self {
        Self::new()
    }
}

/// Forward iterator over a [`TailqHead`]; see the module docs for its `_SAFE` behaviour.
pub struct TailqIter<'a, A: Adapter> {
    cur: *const A::Elem,
    _head: PhantomData<&'a TailqHead<A>>,
}

impl<'a, A: TailqAdapter> Iterator for TailqIter<'a, A> {
    type Item = &'a A::Elem;

    fn next(&mut self) -> Option<&'a A::Elem> {
        // SAFETY: as for `SlistIter`.
        let cur = unsafe { self.cur.as_ref()? };
        self.cur = A::entry(cur).tqe_next.get();
        Some(cur)
    }
}

/// Reverse iterator over a [`TailqHead`]; see the module docs for its `_SAFE` behaviour.
pub struct TailqIterReverse<'a, A: Adapter> {
    head: &'a TailqHead<A>,
    cur: *const A::Elem,
}

impl<'a, A: TailqAdapter> Iterator for TailqIterReverse<'a, A> {
    type Item = &'a A::Elem;

    fn next(&mut self) -> Option<&'a A::Elem> {
        // SAFETY: as for `SlistIter`, walking `tqe_prev` instead.
        let cur = unsafe { self.cur.as_ref()? };
        self.cur = self.head.prev(cur).map_or(ptr::null(), |e| e);
        Some(cur)
    }
}

/*
 * Singly-linked Tail queue declarations.
 */

/// `STAILQ_ENTRY(type)`: the link an element embeds to be in a singly-linked tail queue.
#[repr(C)]
pub struct StailqEntry<T> {
    stqe_next: Cell<*const T>,
}

impl<T> StailqEntry<T> {
    /// An entry that is in no queue.
    pub const fn new() -> Self {
        Self {
            stqe_next: Cell::new(ptr::null()),
        }
    }
}

impl<T> Default for StailqEntry<T> {
    fn default() -> Self {
        Self::new()
    }
}

/// `STAILQ_HEAD(name, type)`: a singly-linked queue with a tail pointer, like a [`SimpleqHead`]
/// plus O(n) removal of an arbitrary element and O(1) access to the last one.
pub struct StailqHead<A: Adapter> {
    stqh_first: Cell<*const A::Elem>,
    /// Address of the last element's `stqe_next`; null stands for `&stqh_first` (empty queue).
    stqh_last: Cell<*const Cell<*const A::Elem>>,
}

impl<A: StailqAdapter> StailqHead<A> {
    /// `STAILQ_HEAD_INITIALIZER`: an empty queue.
    pub const fn new() -> Self {
        Self {
            stqh_first: Cell::new(ptr::null()),
            stqh_last: Cell::new(ptr::null()),
        }
    }

    /// `STAILQ_INIT`: empties the queue without touching the elements.
    pub fn init(&self) {
        self.stqh_first.set(ptr::null());
        self.stqh_last.set(ptr::null());
    }

    /// `STAILQ_FIRST`; `None` is `STAILQ_END`.
    pub fn first(&self) -> Option<&A::Elem> {
        // SAFETY: a linked element is valid until unlinked (the mutators' contract).
        unsafe { self.stqh_first.get().as_ref() }
    }

    /// `STAILQ_LAST`.
    pub fn last(&self) -> Option<&A::Elem> {
        let last = self.stqh_last.get();
        if last.is_null() {
            return None;
        }
        // SAFETY: a non-null `stqh_last` is the `stqe_next` cell of a linked element, valid
        // until unlinked; `container_of` steps back to that element.
        unsafe { container_of::<A>(last).as_ref() }
    }

    /// `STAILQ_EMPTY`.
    pub fn is_empty(&self) -> bool {
        self.stqh_first.get().is_null()
    }

    /// `STAILQ_NEXT`: the element after `elem`.
    pub fn next(elem: &A::Elem) -> Option<&A::Elem> {
        // SAFETY: as for `first`.
        unsafe { A::entry(elem).stqe_next.get().as_ref() }
    }

    /// `STAILQ_FOREACH` and `STAILQ_FOREACH_SAFE`.
    pub fn iter(&self) -> StailqIter<'_, A> {
        StailqIter {
            cur: self.stqh_first.get(),
            _head: PhantomData,
        }
    }

    /// The cell `stqh_last` designates: the last element's `stqe_next`, or `stqh_first`.
    fn last_link(&self) -> &Cell<*const A::Elem> {
        let last = self.stqh_last.get();
        if last.is_null() {
            &self.stqh_first
        } else {
            // SAFETY: a non-null `stqh_last` points at the `stqe_next` of a linked element,
            // valid until unlinked.
            unsafe { &*last }
        }
    }

    /// `STAILQ_INSERT_HEAD`.
    ///
    /// # Safety
    ///
    /// `elem` is in no queue of `A` and stays valid and in place until it is unlinked.
    pub unsafe fn insert_head(&self, elem: &A::Elem) {
        let entry = A::entry(elem);
        let first = self.stqh_first.get();
        entry.stqe_next.set(first);
        if first.is_null() {
            self.stqh_last.set(&entry.stqe_next);
        }
        self.stqh_first.set(elem);
    }

    /// `STAILQ_INSERT_TAIL`.
    ///
    /// # Safety
    ///
    /// As for [`insert_head`](Self::insert_head).
    pub unsafe fn insert_tail(&self, elem: &A::Elem) {
        let entry = A::entry(elem);
        entry.stqe_next.set(ptr::null());
        self.last_link().set(elem);
        self.stqh_last.set(&entry.stqe_next);
    }

    /// `STAILQ_INSERT_AFTER`: links `elem` after `listelm`.
    ///
    /// # Safety
    ///
    /// `listelm` is in this queue; `elem` is in no queue of `A` and stays valid and in place
    /// until it is unlinked.
    pub unsafe fn insert_after(&self, listelm: &A::Elem, elem: &A::Elem) {
        let after = A::entry(listelm);
        let entry = A::entry(elem);
        let next = after.stqe_next.get();
        entry.stqe_next.set(next);
        if next.is_null() {
            self.stqh_last.set(&entry.stqe_next);
        }
        after.stqe_next.set(elem);
    }

    /// `STAILQ_REMOVE_HEAD`: unlinks the first element.
    ///
    /// # Safety
    ///
    /// The queue is not empty.
    pub unsafe fn remove_head(&self) {
        // SAFETY: the caller guarantees a first element exists; it is valid while linked.
        let first = unsafe { &*self.stqh_first.get() };
        let next = A::entry(first).stqe_next.get();
        self.stqh_first.set(next);
        if next.is_null() {
            self.stqh_last.set(ptr::null());
        }
    }

    /// `STAILQ_REMOVE_AFTER`: unlinks the element after `elem`.
    ///
    /// # Safety
    ///
    /// `elem` is in this queue and has a successor.
    pub unsafe fn remove_after(&self, elem: &A::Elem) {
        let entry = A::entry(elem);
        // SAFETY: the caller guarantees a successor exists; it is valid while linked.
        let next = unsafe { &*entry.stqe_next.get() };
        let after_next = A::entry(next).stqe_next.get();
        entry.stqe_next.set(after_next);
        if after_next.is_null() {
            self.stqh_last.set(&entry.stqe_next);
        }
    }

    /// `STAILQ_REMOVE`: unlinks `elem`, walking the queue to find its predecessor (O(n)).
    ///
    /// # Safety
    ///
    /// `elem` is in this queue.
    pub unsafe fn remove(&self, elem: &A::Elem) {
        let target: *const A::Elem = elem;
        if self.stqh_first.get() == target {
            // SAFETY: the queue holds `elem`, so it is not empty.
            unsafe { self.remove_head() };
        } else {
            // SAFETY: `elem` is in the queue and is not first, so the walk from the first
            // element reaches its predecessor through valid, linked elements.
            unsafe {
                let mut cur = &*self.stqh_first.get();
                while A::entry(cur).stqe_next.get() != target {
                    cur = &*A::entry(cur).stqe_next.get();
                }
                self.remove_after(cur);
            }
        }
    }

    /// `STAILQ_CONCAT`: moves every element of `head2` to the end of this queue.
    ///
    /// # Safety
    ///
    /// Both heads stay in place while their queues are not empty.
    pub unsafe fn concat(&self, head2: &Self) {
        if !head2.is_empty() {
            self.last_link().set(head2.stqh_first.get());
            self.stqh_last.set(head2.stqh_last.get());
            head2.init();
        }
    }
}

impl<A: StailqAdapter> Default for StailqHead<A> {
    fn default() -> Self {
        Self::new()
    }
}

/// Forward iterator over a [`StailqHead`]; see the module docs for its `_SAFE` behaviour.
pub struct StailqIter<'a, A: Adapter> {
    cur: *const A::Elem,
    _head: PhantomData<&'a StailqHead<A>>,
}

impl<'a, A: StailqAdapter> Iterator for StailqIter<'a, A> {
    type Item = &'a A::Elem;

    fn next(&mut self) -> Option<&'a A::Elem> {
        // SAFETY: as for `SlistIter`.
        let cur = unsafe { self.cur.as_ref()? };
        self.cur = A::entry(cur).stqe_next.get();
        Some(cur)
    }
}

/// Names the entry field a list uses inside its element type: the `field` argument of the C
/// macros, fixed once per head type. Made with [`crate::queue_adapter!`].
///
/// # Safety
///
/// `entry` must return the entry embedded in `elem` at offset `OFFSET`, the same one every
/// time, and nothing else.
pub unsafe trait Adapter {
    /// The element type (`struct type` in C).
    type Elem;
    /// The embedded entry type: `SlistEntry<Elem>`, `ListEntry<Elem>`, ...
    type Entry;
    /// `offsetof(Elem, field)`.
    const OFFSET: usize;
    /// The entry embedded in `elem`.
    fn entry(elem: &Self::Elem) -> &Self::Entry;
}

/// An [`Adapter`] whose entry is an [`SlistEntry`].
pub trait SlistAdapter: Adapter<Entry = SlistEntry<<Self as Adapter>::Elem>> {}
impl<A: Adapter<Entry = SlistEntry<<A as Adapter>::Elem>>> SlistAdapter for A {}

/// An [`Adapter`] whose entry is a [`ListEntry`].
pub trait ListAdapter: Adapter<Entry = ListEntry<<Self as Adapter>::Elem>> {}
impl<A: Adapter<Entry = ListEntry<<A as Adapter>::Elem>>> ListAdapter for A {}

/// An [`Adapter`] whose entry is a [`SimpleqEntry`].
pub trait SimpleqAdapter: Adapter<Entry = SimpleqEntry<<Self as Adapter>::Elem>> {}
impl<A: Adapter<Entry = SimpleqEntry<<A as Adapter>::Elem>>> SimpleqAdapter for A {}

/// An [`Adapter`] whose entry is an [`XsimpleqEntry`].
pub trait XsimpleqAdapter: Adapter<Entry = XsimpleqEntry<<Self as Adapter>::Elem>> {}
impl<A: Adapter<Entry = XsimpleqEntry<<A as Adapter>::Elem>>> XsimpleqAdapter for A {}

/// An [`Adapter`] whose entry is a [`TailqEntry`].
pub trait TailqAdapter: Adapter<Entry = TailqEntry<<Self as Adapter>::Elem>> {}
impl<A: Adapter<Entry = TailqEntry<<A as Adapter>::Elem>>> TailqAdapter for A {}

/// An [`Adapter`] whose entry is a [`StailqEntry`].
pub trait StailqAdapter: Adapter<Entry = StailqEntry<<Self as Adapter>::Elem>> {}
impl<A: Adapter<Entry = StailqEntry<<A as Adapter>::Elem>>> StailqAdapter for A {}

/// `_Q_INVALIDATE`: poisons a link of a removed element under feature `diagnostic`, so a stale
/// use faults instead of walking a list; a no-op otherwise.
#[cfg(feature = "diagnostic")]
fn invalidate<P>(link: &Cell<*const P>) {
    link.set(Q_INVALID as *const P);
}

/// `_Q_INVALIDATE`, compiled out without feature `diagnostic`.
#[cfg(not(feature = "diagnostic"))]
fn invalidate<P>(_link: &Cell<*const P>) {}

/// The element whose entry starts at `link`.
///
/// # Safety
///
/// `link` points at the first (`*_next`) cell of an `A::Entry` embedded at `A::OFFSET` in a
/// live `A::Elem`; every entry type is `#[repr(C)]` with that cell first.
unsafe fn container_of<A: Adapter>(link: *const Cell<*const A::Elem>) -> *const A::Elem {
    // SAFETY: stepping back by the field offset stays inside the element's allocation.
    unsafe { link.cast::<u8>().sub(A::OFFSET).cast::<A::Elem>() }
}

// The `*_next` cell is the first field of every entry, which `container_of` relies on.
const _: () = {
    use core::mem::offset_of;
    assert!(offset_of!(SlistEntry<u8>, sle_next) == 0);
    assert!(offset_of!(ListEntry<u8>, le_next) == 0);
    assert!(offset_of!(SimpleqEntry<u8>, sqe_next) == 0);
    assert!(offset_of!(XsimpleqEntry<u8>, sqx_next) == 0);
    assert!(offset_of!(TailqEntry<u8>, tqe_next) == 0);
    assert!(offset_of!(StailqEntry<u8>, stqe_next) == 0);
};

#[cfg(test)]
mod tests;
