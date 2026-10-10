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
//! Intrusive lists and queues: `<sys/queue.h>`, see `queue(3)`.
//!
//! Upstream: sys/sys/queue.h @ 3ce1f3f79392
//! LZ: sys/sys/queue.rs@3c62ede68802
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
//! Links are interior-mutable, so a list is modified through `&Elem` and `&Head`, like the C
//! that mutates through pointers under a lock. The lock discipline stays the caller's, as in C:
//! readers are safe functions whose references live as long as the borrow they come from; every
//! mutator is `unsafe` and states what the caller guarantees. The contract behind all of them:
//! an element linked into a list stays valid and in place until it is unlinked, a non-empty
//! list or tail queue head stays in place (its first element's back link names it; the other
//! heads are named by nothing), and an element unlinked and then freed is not the pending
//! position of a live iterator (the `_FOREACH_SAFE` rule: only the element just yielded may go).
//!
//! ## Deviations
//! - `_FOREACH` and `_FOREACH_SAFE` are one iterator: it reads the next element before yielding
//!   the current one, so the current element may be unlinked while iterating (the `_SAFE` form).
//! - `*_HEAD_INITIALIZER` cannot take the head's own address in a `const`; an empty head stores
//!   a null "last" link that means "the head's first link", so an empty head can be moved. Once
//!   an element is inserted the head is pinned, as in C.
//! - `TAILQ_PREV`, `TAILQ_LAST` and `STAILQ_LAST` recover the element from a link through the
//!   adapter's field offset (`container_of`) instead of C's type-punning cast of the link as a
//!   head; `prev` therefore takes the head.
//! - `XSIMPLEQ_INIT` takes the cookie as an argument until `arc4random(9)` exists (milestone M5);
//!   a head made by `new` has cookie 0 and works like a plain simple queue.
//! - `_Q_INVALIDATE` poisons the links of a removed element under feature `diagnostic`
//!   (OpenBSD's `option DIAGNOSTIC`), and a poisoned link that is dereferenced, or read
//!   through a reader, fails a `kassert!` where the C would fault on `(void *)-1`. Without
//!   `diagnostic` the same removals clear the links instead of leaving them stale (see
//!   "Redesign").
//! - The readers are safe functions, as in LZ, so they cannot ask that the element they are
//!   given be in the list, and their soundness rests on the callers, as the C's macros do:
//!   `next` of an element removed by an operation that does not invalidate (`*_REMOVE_HEAD`,
//!   `*_REMOVE_AFTER`, `STAILQ_REMOVE`) still reads its old successor, valid only while that
//!   successor stays linked; [`XsimpleqHead::next`] of an element of another XOR queue decodes
//!   its link with the wrong cookie; [`TailqHead::prev`] of the first element of another tail
//!   queue steps back from that queue's head as if it were an element. Each is a reader given
//!   an element outside the list, which the mutators' `# Safety` sections and the readers'
//!   docs forbid. Making it a type needs a signature change (the user's decision; N2 kept the
//!   source API).
//!
//! ## Redesign
//! - The links are typed: LZ's `Cell<*const T>` (`struct type *`) and
//!   `Cell<*const Cell<*const T>>` (`struct type **`) are one private type, `Link<P>`, a
//!   `Cell<Option<NonNull<P>>>` whose `None` is the C's `NULL` (`docs/IDIOMS.md`, "a typed
//!   intrusive link"). The fields stay private and only this module writes them, so the
//!   type's invariant (a `Some` link of a head or of a linked element names a live object)
//!   holds by construction from the mutators' contracts, and the dereference lives in one
//!   method, `Link::target`, instead of an `unsafe` block at every read.
//! - A "previous" or "last" link that names a next link inside an element (`le_prev`,
//!   `tqe_prev`, `sqh_last`, `tqh_last`, `stqh_last`) is made by `link_of` with the provenance
//!   of the whole element, not of the link's field, so `container_of` (`TAILQ_LAST`,
//!   `TAILQ_PREV`, `STAILQ_LAST`) steps back to the element through a pointer allowed to reach
//!   it. `container_of` itself is safe address arithmetic (`wrapping_byte_sub`); only the
//!   dereference of its result is `unsafe`, at the three readers.
//! - The XOR queue encodes with `expose_provenance` and decodes with
//!   `with_exposed_provenance`, Rust's spelling of the C's pointer/integer round trip.
//! - The `_FOREACH_SAFE` iterators share one cursor (`Cursor`) and a step (`Step`): the next
//!   link, the XOR'd next link, or `tqe_prev`; the cursor holds the pending position as a raw
//!   `NonNull` and dereferences it in one place.
//! - What the C turns into a `NULL` dereference (removing from an empty queue, removing after
//!   the last element, `SLIST_REMOVE` or `STAILQ_REMOVE` of an element that is not there,
//!   inserting before, removing or replacing an element that is in no list, a second
//!   `LIST_REMOVE` or `TAILQ_REMOVE` included) is an explicit kernel panic (`panic(9)`, through
//!   `queue_panic`) in every configuration, where the C faults: an `Option` cannot be
//!   dereferenced, so the fault becomes a message naming the operation.
//! - `LIST_REMOVE`, `LIST_REPLACE`, `SLIST_REMOVE`, `TAILQ_REMOVE` and `TAILQ_REPLACE` clear the
//!   removed element's links (`None`) when `diagnostic` is off, where LZ and the C leave them
//!   stale: a stale link would name an element that may be freed, and the readers are safe.
//!   OpenBSD's GENERIC kernels run with `DIAGNOSTIC`, which poisons them, so no correct caller
//!   reads them.
//! - The entries no longer need their next link first: `container_of` uses the adapter's
//!   offset plus the link's offset in its entry (`NextEntry::NEXT`). They stay `#[repr(C)]`,
//!   the C's layout, for the structures that embed them.
//! - [`Adapter`] is a safe trait and `queue_adapter!` expands a plain `impl`
//!   (`docs/ZERO_UNSAFE.md`, decision 7), so a module that declares a list can be compiled
//!   under `#[forbid(unsafe_code)]`. The contract `container_of` trusts (`OFFSET` is the offset
//!   of the field `entry` projects) is kept by the macro, which computes both from one field
//!   name, and `cargo xtask lz check` refuses any `impl Adapter` written by hand: the one place
//!   where a CI check, not the compiler, upholds a premise of a `SAFETY:` argument.

use core::cell::Cell;
use core::marker::PhantomData;
use core::mem::offset_of;
use core::num::NonZero;
use core::ptr::{self, NonNull};

/// `_Q_INVALID`: the address written into a removed element's links under feature
/// `diagnostic` (the C's `(void *)-1`).
const Q_INVALID: usize = usize::MAX;

/// Generates a zero-sized [`Adapter`] type: `queue_adapter!(pub ProcList: Proc, p_list =>
/// ListEntry<Proc>)` says that `ProcList` lists `Proc`s through their `p_list` field.
#[macro_export]
macro_rules! queue_adapter {
    ($(#[$meta:meta])* $vis:vis $name:ident: $elem:ty, $field:ident => $entry:ty) => {
        $(#[$meta])*
        $vis struct $name;

        // `Adapter`'s contract: `entry` projects the named field and nothing else, and
        // `OFFSET` is that field's offset, so the two agree. This macro and `tree_adapter!`
        // (which expands to it) are the only impls; `cargo xtask lz check` refuses any other.
        impl $crate::sys::queue::Adapter for $name {
            type Elem = $elem;
            type Entry = $entry;
            const OFFSET: usize = ::core::mem::offset_of!($elem, $field);

            fn entry(elem: &$elem) -> &$entry {
                &elem.$field
            }
        }
    };
}

/// A typed link: the C's `struct type *` (a first or next link, `P = T`) or `struct type **`
/// (a previous or last link, `P = Link<T>`). `None` is `NULL`.
///
/// Invariant: a link of a head, or of an element linked into a list, is `None` or names a live
/// `P` (an element linked into the same list, or a next link inside one or inside a head that
/// stays in place while its list is not empty), or, under `diagnostic`, the `_Q_INVALID`
/// poison. Only this module writes links, and only through the mutators, whose callers
/// guarantee that a linked element stays valid and in place until it is unlinked.
#[repr(transparent)]
struct Link<P>(Cell<Option<NonNull<P>>>);

impl<P> Link<P> {
    /// A `NULL` link.
    const fn new() -> Self {
        Self(Cell::new(None))
    }

    /// The link's value. Under `diagnostic`, reading the poison of a removed element is the
    /// C's fault on `(void *)-1`, a failed assertion here.
    fn get(&self) -> Option<NonNull<P>> {
        let v = self.0.get();
        #[cfg(feature = "diagnostic")]
        crate::kassert!(v.is_none_or(|p| p.addr().get() != Q_INVALID));
        v
    }

    /// The raw value, poison included (`le_prev != NULL`, `tqe_prev != NULL`).
    fn peek(&self) -> Option<NonNull<P>> {
        self.0.get()
    }

    fn set(&self, v: Option<NonNull<P>>) {
        self.0.set(v);
    }

    /// Points the link at `target`, with the provenance of the reference.
    fn set_ref(&self, target: &P) {
        self.0.set(Some(NonNull::from(target)));
    }

    fn is_null(&self) -> bool {
        self.0.get().is_none()
    }

    /// Whether the link names `target` (`==` on addresses).
    fn names(&self, target: &P) -> bool {
        self.0.get() == Some(NonNull::from(target))
    }

    /// The object the link names. The lifetime is the caller's to bound: the public readers
    /// tie it to the borrow of the head or element they were given.
    fn target<'a>(&self) -> Option<&'a P> {
        let p = self.0.get()?;
        #[cfg(feature = "diagnostic")]
        crate::kassert!(p.addr().get() != Q_INVALID);
        // SAFETY: by the type's invariant a `Some` link of a head or of a linked element names
        // a live `P`: the mutators' callers keep linked elements valid and in place until they
        // are unlinked, and the list and tail queue heads the back links name in place while
        // not empty. The mutators read the head's links and the links of the elements their
        // `# Safety` sections require to be linked. The safe readers (`next`, the iterators'
        // steps) read the link of the element their caller passes, and that element must be in
        // the list, or never linked (its links are `None`): an obligation on the callers, as
        // with the C's macros, which the safe signatures cannot carry (module docs,
        // "Deviations"; the mutators that leave a removed element's link stale say so in their
        // `# Safety`). The reference lives as long as the borrow the reader was given. The
        // poison is never dereferenced: the assertion above stops it under `diagnostic`, the
        // only configuration that writes it. `P` is only read through `&` (its links are
        // `Cell`s).
        Some(unsafe { p.as_ref() })
    }
}

/// The position of a `_FOREACH_SAFE` loop: the element the next call yields, read from the
/// links before the previous one was handed out, so that one may be unlinked.
struct Cursor<'a, T, S> {
    cur: Option<NonNull<T>>,
    step: S,
    _list: PhantomData<&'a T>,
}

impl<'a, T, S: Step<T>> Cursor<'a, T, S> {
    fn new(first: Option<NonNull<T>>, step: S) -> Self {
        Self {
            cur: first,
            step,
            _list: PhantomData,
        }
    }

    fn advance(&mut self) -> Option<&'a T> {
        let cur = self.cur?;
        // SAFETY: `cur` was read from a link of the head or of the element yielded last, while
        // that element was still linked (the `Link` invariant), so it named a linked element.
        // The callers' contract (module docs) lets them unlink only the element just yielded,
        // never the pending one, so it is still linked, valid and in place; the reference lives
        // as long as the borrow of the head the iterator came from.
        let elem = unsafe { cur.as_ref() };
        self.cur = self.step.step(elem);
        Some(elem)
    }
}

/// The step of a cursor through an entry's next link.
struct NextStep<A>(PhantomData<A>);

impl<A> NextStep<A> {
    const fn new() -> Self {
        Self(PhantomData)
    }
}

/*
 * Singly-linked List definitions.
 */

/// `SLIST_ENTRY(type)`: the link an element embeds to be in a singly-linked list.
#[repr(C)]
pub struct SlistEntry<T> {
    sle_next: Link<T>,
}

impl<T> SlistEntry<T> {
    /// An entry that is in no list.
    pub const fn new() -> Self {
        Self {
            sle_next: Link::new(),
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
    slh_first: Link<A::Elem>,
}

impl<A: SlistAdapter> SlistHead<A> {
    /// `SLIST_HEAD_INITIALIZER`: an empty list.
    pub const fn new() -> Self {
        Self {
            slh_first: Link::new(),
        }
    }

    /// `SLIST_INIT`: empties the list without touching the elements.
    pub fn init(&self) {
        self.slh_first.set(None);
    }

    /// `SLIST_FIRST`; `None` is `SLIST_END`.
    pub fn first(&self) -> Option<&A::Elem> {
        self.slh_first.target()
    }

    /// `SLIST_EMPTY`.
    pub fn is_empty(&self) -> bool {
        self.slh_first.is_null()
    }

    /// `SLIST_NEXT`: the element after `elem`, which must be in the list or never linked
    /// (module docs, "Deviations").
    pub fn next(elem: &A::Elem) -> Option<&A::Elem> {
        A::entry(elem).sle_next.target()
    }

    /// `SLIST_FOREACH` and `SLIST_FOREACH_SAFE`.
    pub fn iter(&self) -> SlistIter<'_, A> {
        SlistIter(Cursor::new(self.slh_first.get(), NextStep::new()))
    }

    /// `SLIST_INSERT_HEAD`.
    ///
    /// # Safety
    ///
    /// `elem` is in no list of `A` and stays valid and in place until it is unlinked.
    pub unsafe fn insert_head(&self, elem: &A::Elem) {
        A::entry(elem).sle_next.set(self.slh_first.get());
        self.slh_first.set_ref(elem);
    }

    /// `SLIST_INSERT_AFTER`: links `elem` after `slistelm`.
    ///
    /// # Safety
    ///
    /// `slistelm` is linked; `elem` is in no list of `A` and stays valid and in place until it
    /// is unlinked.
    pub unsafe fn insert_after(slistelm: &A::Elem, elem: &A::Elem) {
        let after = &A::entry(slistelm).sle_next;
        A::entry(elem).sle_next.set(after.get());
        after.set_ref(elem);
    }

    /// `SLIST_REMOVE_HEAD`: unlinks the first element. Its link still names its successor.
    ///
    /// # Safety
    ///
    /// The list is not empty (otherwise the kernel panics, where the C dereferences `NULL`).
    /// The removed element's link still names its old successor, as in C: the caller does not
    /// pass the removed element to [`next`](Self::next) once that successor is unlinked and
    /// freed, until the element is inserted again (the safe reader cannot check it).
    pub unsafe fn remove_head(&self) {
        let Some(first) = self.slh_first.target() else {
            queue_panic("SLIST_REMOVE_HEAD: empty");
        };
        self.slh_first.set(A::entry(first).sle_next.get());
    }

    /// `SLIST_REMOVE_AFTER`: unlinks the element after `elem`. Its link still names its
    /// successor.
    ///
    /// # Safety
    ///
    /// `elem` is linked and has a successor (otherwise the kernel panics, where the C
    /// dereferences `NULL`).
    /// The removed element's link still names its old successor, as in C: the caller does not
    /// pass the removed element to [`next`](Self::next) once that successor is unlinked and
    /// freed, until the element is inserted again (the safe reader cannot check it).
    pub unsafe fn remove_after(elem: &A::Elem) {
        let link = &A::entry(elem).sle_next;
        let Some(next) = link.target() else {
            queue_panic("SLIST_REMOVE_AFTER: no successor");
        };
        link.set(A::entry(next).sle_next.get());
    }

    /// `SLIST_REMOVE`: unlinks `elem`, walking the list to find its predecessor (O(n)), and
    /// invalidates its link.
    ///
    /// # Safety
    ///
    /// `elem` is in this list (otherwise the walk ends in a kernel panic, where the C
    /// dereferences `NULL`).
    pub unsafe fn remove(&self, elem: &A::Elem) {
        let next = A::entry(elem).sle_next.get();
        let mut link = &self.slh_first;
        loop {
            if link.names(elem) {
                link.set(next);
                break;
            }
            match link.target() {
                Some(cur) => link = &A::entry(cur).sle_next,
                None => queue_panic("SLIST_REMOVE: element not in the list"),
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
pub struct SlistIter<'a, A: Adapter>(Cursor<'a, A::Elem, NextStep<A>>);

impl<'a, A: SlistAdapter> Iterator for SlistIter<'a, A> {
    type Item = &'a A::Elem;

    fn next(&mut self) -> Option<&'a A::Elem> {
        self.0.advance()
    }
}

/*
 * List definitions.
 */

/// `LIST_ENTRY(type)`: the link an element embeds to be in a list. `le_prev` names the previous
/// element's `le_next` (or the head's `lh_first`), which makes unlinking O(1).
#[repr(C)]
pub struct ListEntry<T> {
    le_next: Link<T>,
    le_prev: Link<Link<T>>,
}

impl<T> ListEntry<T> {
    /// An entry that is in no list.
    pub const fn new() -> Self {
        Self {
            le_next: Link::new(),
            le_prev: Link::new(),
        }
    }

    /// `elm->field.le_prev != NULL`: the element is in a list. Meaningful only for code that
    /// clears the link after every removal ([`clear_prev`](Self::clear_prev)), as uhci(4)
    /// does with its active xfers (`uhci_active_intr_list`).
    pub fn is_linked(&self) -> bool {
        self.le_prev.peek().is_some()
    }

    /// `elm->field.le_prev = NULL` after a `LIST_REMOVE`.
    ///
    /// # Safety
    ///
    /// The element is in no list (it was just removed from its list).
    pub unsafe fn clear_prev(&self) {
        self.le_prev.set(None);
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
    lh_first: Link<A::Elem>,
}

impl<A: ListAdapter> ListHead<A> {
    /// `LIST_HEAD_INITIALIZER`: an empty list.
    pub const fn new() -> Self {
        Self {
            lh_first: Link::new(),
        }
    }

    /// `LIST_INIT`: empties the list without touching the elements.
    pub fn init(&self) {
        self.lh_first.set(None);
    }

    /// `LIST_FIRST`; `None` is `LIST_END`.
    pub fn first(&self) -> Option<&A::Elem> {
        self.lh_first.target()
    }

    /// `LIST_EMPTY`.
    pub fn is_empty(&self) -> bool {
        self.lh_first.is_null()
    }

    /// `LIST_NEXT`: the element after `elem`, which must be in the list or never linked
    /// (module docs, "Deviations").
    pub fn next(elem: &A::Elem) -> Option<&A::Elem> {
        A::entry(elem).le_next.target()
    }

    /// `LIST_FOREACH` and `LIST_FOREACH_SAFE`.
    pub fn iter(&self) -> ListIter<'_, A> {
        ListIter(Cursor::new(self.lh_first.get(), NextStep::new()))
    }

    /// `LIST_INSERT_HEAD`.
    ///
    /// # Safety
    ///
    /// `elem` is in no list of `A` and stays valid and in place until it is unlinked; the head
    /// stays in place while the list is not empty.
    pub unsafe fn insert_head(&self, elem: &A::Elem) {
        let entry = A::entry(elem);
        entry.le_next.set(self.lh_first.get());
        if let Some(first) = self.lh_first.target() {
            A::entry(first).le_prev.set(Some(link_of::<A>(elem)));
        }
        self.lh_first.set_ref(elem);
        entry.le_prev.set_ref(&self.lh_first);
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
        entry.le_next.set(after.le_next.get());
        if let Some(next) = after.le_next.target() {
            A::entry(next).le_prev.set(Some(link_of::<A>(elem)));
        }
        after.le_next.set_ref(elem);
        entry.le_prev.set(Some(link_of::<A>(listelm)));
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
        let Some(prev) = before.le_prev.target() else {
            queue_panic("LIST_INSERT_BEFORE: listelm not linked");
        };
        entry.le_prev.set(before.le_prev.get());
        entry.le_next.set_ref(listelm);
        prev.set_ref(elem);
        before.le_prev.set(Some(link_of::<A>(elem)));
    }

    /// `LIST_REMOVE`: unlinks `elem` in O(1) and invalidates its links.
    ///
    /// # Safety
    ///
    /// `elem` is in a list of `A`. An element in no list, one already removed included,
    /// panics the kernel, where the C dereferences `NULL` (or `_Q_INVALID`).
    pub unsafe fn remove(elem: &A::Elem) {
        let entry = A::entry(elem);
        let Some(prev) = entry.le_prev.target() else {
            queue_panic("LIST_REMOVE: element not linked");
        };
        if let Some(next) = entry.le_next.target() {
            A::entry(next).le_prev.set(entry.le_prev.get());
        }
        prev.set(entry.le_next.get());
        invalidate(&entry.le_prev);
        invalidate(&entry.le_next);
    }

    /// `LIST_REPLACE`: puts `elem2` where `elem` is and unlinks `elem`, invalidating its links.
    ///
    /// # Safety
    ///
    /// `elem` is in a list of `A`; `elem2` is in no list of `A` and stays valid and in place
    /// until it is unlinked.
    pub unsafe fn replace(elem: &A::Elem, elem2: &A::Elem) {
        let old = A::entry(elem);
        let new = A::entry(elem2);
        let Some(prev) = old.le_prev.target() else {
            queue_panic("LIST_REPLACE: element not linked");
        };
        new.le_next.set(old.le_next.get());
        if let Some(next) = new.le_next.target() {
            A::entry(next).le_prev.set(Some(link_of::<A>(elem2)));
        }
        new.le_prev.set(old.le_prev.get());
        prev.set_ref(elem2);
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
pub struct ListIter<'a, A: Adapter>(Cursor<'a, A::Elem, NextStep<A>>);

impl<'a, A: ListAdapter> Iterator for ListIter<'a, A> {
    type Item = &'a A::Elem;

    fn next(&mut self) -> Option<&'a A::Elem> {
        self.0.advance()
    }
}

/*
 * Simple queue definitions.
 */

/// `SIMPLEQ_ENTRY(type)`: the link an element embeds to be in a simple queue.
#[repr(C)]
pub struct SimpleqEntry<T> {
    sqe_next: Link<T>,
}

impl<T> SimpleqEntry<T> {
    /// An entry that is in no queue.
    pub const fn new() -> Self {
        Self {
            sqe_next: Link::new(),
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
    sqh_first: Link<A::Elem>,
    /// The last element's `sqe_next`; `None` stands for `&sqh_first` (empty queue).
    sqh_last: Link<Link<A::Elem>>,
}

impl<A: SimpleqAdapter> SimpleqHead<A> {
    /// `SIMPLEQ_HEAD_INITIALIZER`: an empty queue.
    pub const fn new() -> Self {
        Self {
            sqh_first: Link::new(),
            sqh_last: Link::new(),
        }
    }

    /// `SIMPLEQ_INIT`: empties the queue without touching the elements.
    pub fn init(&self) {
        self.sqh_first.set(None);
        self.sqh_last.set(None);
    }

    /// `SIMPLEQ_FIRST`; `None` is `SIMPLEQ_END`.
    pub fn first(&self) -> Option<&A::Elem> {
        self.sqh_first.target()
    }

    /// `SIMPLEQ_EMPTY`.
    pub fn is_empty(&self) -> bool {
        self.sqh_first.is_null()
    }

    /// `SIMPLEQ_NEXT`: the element after `elem`, which must be in the queue or never linked
    /// (module docs, "Deviations").
    pub fn next(elem: &A::Elem) -> Option<&A::Elem> {
        A::entry(elem).sqe_next.target()
    }

    /// `SIMPLEQ_FOREACH` and `SIMPLEQ_FOREACH_SAFE`.
    pub fn iter(&self) -> SimpleqIter<'_, A> {
        SimpleqIter(Cursor::new(self.sqh_first.get(), NextStep::new()))
    }

    /// The link `sqh_last` designates: the last element's `sqe_next`, or `sqh_first`.
    fn last_link(&self) -> &Link<A::Elem> {
        self.sqh_last.target().unwrap_or(&self.sqh_first)
    }

    /// `SIMPLEQ_INSERT_HEAD`.
    ///
    /// # Safety
    ///
    /// `elem` is in no queue of `A` and stays valid and in place until it is unlinked.
    pub unsafe fn insert_head(&self, elem: &A::Elem) {
        let entry = A::entry(elem);
        entry.sqe_next.set(self.sqh_first.get());
        if entry.sqe_next.is_null() {
            self.sqh_last.set(Some(link_of::<A>(elem)));
        }
        self.sqh_first.set_ref(elem);
    }

    /// `SIMPLEQ_INSERT_TAIL`.
    ///
    /// # Safety
    ///
    /// As for [`insert_head`](Self::insert_head).
    pub unsafe fn insert_tail(&self, elem: &A::Elem) {
        A::entry(elem).sqe_next.set(None);
        self.last_link().set_ref(elem);
        self.sqh_last.set(Some(link_of::<A>(elem)));
    }

    /// `SIMPLEQ_INSERT_AFTER`: links `elem` after `listelm`.
    ///
    /// # Safety
    ///
    /// `listelm` is in this queue; `elem` is in no queue of `A` and stays valid and in place
    /// until it is unlinked.
    pub unsafe fn insert_after(&self, listelm: &A::Elem, elem: &A::Elem) {
        let after = &A::entry(listelm).sqe_next;
        let entry = A::entry(elem);
        entry.sqe_next.set(after.get());
        if entry.sqe_next.is_null() {
            self.sqh_last.set(Some(link_of::<A>(elem)));
        }
        after.set_ref(elem);
    }

    /// `SIMPLEQ_REMOVE_HEAD`: unlinks the first element. Its link still names its successor.
    ///
    /// # Safety
    ///
    /// The queue is not empty (otherwise the kernel panics, where the C dereferences `NULL`).
    /// The removed element's link still names its old successor, as in C: the caller does not
    /// pass the removed element to [`next`](Self::next) once that successor is unlinked and
    /// freed, until the element is inserted again (the safe reader cannot check it).
    pub unsafe fn remove_head(&self) {
        let Some(first) = self.sqh_first.target() else {
            queue_panic("SIMPLEQ_REMOVE_HEAD: empty");
        };
        self.sqh_first.set(A::entry(first).sqe_next.get());
        if self.sqh_first.is_null() {
            self.sqh_last.set(None);
        }
    }

    /// `SIMPLEQ_REMOVE_AFTER`: unlinks the element after `elem`. Its link still names its
    /// successor.
    ///
    /// # Safety
    ///
    /// `elem` is in this queue and has a successor (otherwise the kernel panics, where the C
    /// dereferences `NULL`).
    /// The removed element's link still names its old successor, as in C: the caller does not
    /// pass the removed element to [`next`](Self::next) once that successor is unlinked and
    /// freed, until the element is inserted again (the safe reader cannot check it).
    pub unsafe fn remove_after(&self, elem: &A::Elem) {
        let link = &A::entry(elem).sqe_next;
        let Some(next) = link.target() else {
            queue_panic("SIMPLEQ_REMOVE_AFTER: no successor");
        };
        link.set(A::entry(next).sqe_next.get());
        if link.is_null() {
            self.sqh_last.set(Some(link_of::<A>(elem)));
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
pub struct SimpleqIter<'a, A: Adapter>(Cursor<'a, A::Elem, NextStep<A>>);

impl<'a, A: SimpleqAdapter> Iterator for SimpleqIter<'a, A> {
    type Item = &'a A::Elem;

    fn next(&mut self) -> Option<&'a A::Elem> {
        self.0.advance()
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

    /// `XSIMPLEQ_XOR`: encodes or decodes a pointer with the cookie. Encoding exposes the
    /// pointer's provenance, so that [`decode`](Self::decode) may rebuild a usable pointer.
    pub fn xor<P>(&self, p: *const P) -> usize {
        self.sqx_cookie.get() ^ p.expose_provenance()
    }

    /// The encoded null: the end of the queue.
    fn end(&self) -> usize {
        self.xor(ptr::null::<A::Elem>())
    }

    /// The object an encoded link of this queue names, `None` for the encoded null.
    fn decode<'a, P>(&self, v: usize) -> Option<&'a P> {
        let p = NonNull::new(ptr::with_exposed_provenance_mut::<P>(
            self.sqx_cookie.get() ^ v,
        ))?;
        // SAFETY: every encoded value this module stores in the head or in an element linked
        // into this queue is `xor`, with this head's cookie, of a reference to a live object
        // (an element linked into this queue, or the `sqx_next` inside one), whose provenance
        // `xor` exposed; the callers keep linked elements valid and in place until they are
        // unlinked. The mutators decode the head's links and the links of the elements their
        // `# Safety` sections require to be in this queue. `next` and the iterator's step
        // decode the link of the element their caller passes, which must be in this queue: an
        // element of another XOR queue would decode with the wrong cookie. That obligation is
        // the caller's, as with the C's macro, and the safe signature cannot carry it (module
        // docs, "Deviations"). `with_exposed_provenance_mut` picks up the exposed provenance,
        // and the reference only reads (the links are `Cell`s).
        Some(unsafe { p.as_ref() })
    }

    /// `XSIMPLEQ_FIRST`; `None` is `XSIMPLEQ_END`.
    pub fn first(&self) -> Option<&A::Elem> {
        self.decode(self.sqx_first.get())
    }

    /// `XSIMPLEQ_EMPTY`.
    pub fn is_empty(&self) -> bool {
        self.sqx_first.get() == self.end()
    }

    /// `XSIMPLEQ_NEXT`: the element after `elem`, which must be in this queue
    /// (module docs, "Deviations").
    pub fn next<'a>(&self, elem: &'a A::Elem) -> Option<&'a A::Elem> {
        self.decode(A::entry(elem).sqx_next.get())
    }

    /// `XSIMPLEQ_FOREACH` and `XSIMPLEQ_FOREACH_SAFE`.
    pub fn iter(&self) -> XsimpleqIter<'_, A> {
        XsimpleqIter(Cursor::new(self.first().map(NonNull::from), self))
    }

    /// The link `sqx_last` designates: the last element's `sqx_next`, or `sqx_first`.
    fn last_link(&self) -> &Cell<usize> {
        self.decode(self.sqx_last.get()).unwrap_or(&self.sqx_first)
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
        if first == self.end() {
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
        entry.sqx_next.set(self.end());
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
        if next == self.end() {
            self.sqx_last.set(self.xor(&entry.sqx_next));
        }
        after.sqx_next.set(self.xor(elem));
    }

    /// `XSIMPLEQ_REMOVE_HEAD`: unlinks the first element.
    ///
    /// # Safety
    ///
    /// The queue is not empty (otherwise the kernel panics, where the C dereferences `NULL`).
    /// The removed element's link still names its old successor, as in C: the caller does not
    /// pass the removed element to [`next`](Self::next) once that successor is unlinked and
    /// freed, until the element is inserted again (the safe reader cannot check it).
    pub unsafe fn remove_head(&self) {
        let Some(first) = self.first() else {
            queue_panic("XSIMPLEQ_REMOVE_HEAD: empty");
        };
        let next = A::entry(first).sqx_next.get();
        self.sqx_first.set(next);
        if next == self.end() {
            self.sqx_last.set(self.xor(ptr::null::<Cell<usize>>()));
        }
    }

    /// `XSIMPLEQ_REMOVE_AFTER`: unlinks the element after `elem`.
    ///
    /// # Safety
    ///
    /// `elem` is in this queue and has a successor (otherwise the kernel panics, where the C
    /// dereferences `NULL`).
    /// The removed element's link still names its old successor, as in C: the caller does not
    /// pass the removed element to [`next`](Self::next) once that successor is unlinked and
    /// freed, until the element is inserted again (the safe reader cannot check it).
    pub unsafe fn remove_after(&self, elem: &A::Elem) {
        let entry = A::entry(elem);
        let Some(next) = self.decode::<A::Elem>(entry.sqx_next.get()) else {
            queue_panic("XSIMPLEQ_REMOVE_AFTER: no successor");
        };
        let after_next = A::entry(next).sqx_next.get();
        entry.sqx_next.set(after_next);
        if after_next == self.end() {
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
pub struct XsimpleqIter<'a, A: Adapter>(Cursor<'a, A::Elem, &'a XsimpleqHead<A>>);

impl<'a, A: XsimpleqAdapter> Iterator for XsimpleqIter<'a, A> {
    type Item = &'a A::Elem;

    fn next(&mut self) -> Option<&'a A::Elem> {
        self.0.advance()
    }
}

/*
 * Tail queue definitions.
 */

/// `TAILQ_ENTRY(type)`: the link an element embeds to be in a tail queue. `tqe_prev` names the
/// previous element's `tqe_next` (or the head's `tqh_first`).
#[repr(C)]
pub struct TailqEntry<T> {
    tqe_next: Link<T>,
    tqe_prev: Link<Link<T>>,
}

impl<T> TailqEntry<T> {
    /// An entry that is in no queue.
    pub const fn new() -> Self {
        Self {
            tqe_next: Link::new(),
            tqe_prev: Link::new(),
        }
    }
}

impl<T> TailqEntry<T> {
    /// `elm->field.tqe_prev != NULL`: the element is in a queue (or was marked so by
    /// [`set_prev_self`](Self::set_prev_self)). Meaningful only for code that clears the link
    /// after every removal ([`clear_prev`](Self::clear_prev)), as pf does with its rules.
    pub fn is_linked(&self) -> bool {
        self.tqe_prev.peek().is_some()
    }

    /// `elm->field.tqe_prev = NULL` after a `TAILQ_REMOVE`.
    ///
    /// # Safety
    ///
    /// The element is in no queue (it was just removed from its queue).
    pub unsafe fn clear_prev(&self) {
        self.tqe_prev.set(None);
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
        self.tqe_prev.set_ref(&self.tqe_next);
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
    tqh_first: Link<A::Elem>,
    /// The last element's `tqe_next`; `None` stands for `&tqh_first` (empty queue).
    tqh_last: Link<Link<A::Elem>>,
}

impl<A: TailqAdapter> TailqHead<A> {
    /// `TAILQ_HEAD_INITIALIZER`: an empty queue.
    pub const fn new() -> Self {
        Self {
            tqh_first: Link::new(),
            tqh_last: Link::new(),
        }
    }

    /// `TAILQ_INIT`: empties the queue without touching the elements.
    pub fn init(&self) {
        self.tqh_first.set(None);
        self.tqh_last.set(None);
    }

    /// `TAILQ_FIRST`; `None` is `TAILQ_END`.
    pub fn first(&self) -> Option<&A::Elem> {
        self.tqh_first.target()
    }

    /// `TAILQ_LAST`.
    pub fn last(&self) -> Option<&A::Elem> {
        let elem = container_of::<A>(self.tqh_last.get()?)?;
        // SAFETY: a `Some` `tqh_last` is made by `link_of` from the last element of this queue
        // (`insert_*`, `remove`, `replace`, `concat` keep it so), so it is that element's
        // `tqe_next` with the provenance of the whole element; `container_of` steps back by the
        // entry's offset to the element, which is linked and therefore live. The reference
        // lives as long as the borrow of the head.
        Some(unsafe { elem.as_ref() })
    }

    /// `TAILQ_EMPTY`.
    pub fn is_empty(&self) -> bool {
        self.tqh_first.is_null()
    }

    /// `TAILQ_NEXT`: the element after `elem`, which must be in the queue or never linked
    /// (module docs, "Deviations").
    pub fn next(elem: &A::Elem) -> Option<&A::Elem> {
        A::entry(elem).tqe_next.target()
    }

    /// `TAILQ_PREV`: the element before `elem`, which must be in this queue (module docs,
    /// "Deviations").
    pub fn prev<'a>(&self, elem: &'a A::Elem) -> Option<&'a A::Elem> {
        let prev = A::entry(elem).tqe_prev.get()?;
        if prev == NonNull::from(&self.tqh_first) {
            return None;
        }
        let elem = container_of::<A>(prev)?;
        // SAFETY: `elem` must be in this queue (the caller's obligation, which the safe
        // signature cannot carry: module docs, "Deviations"; the first element of another
        // tail queue would name that head's `tqh_first`, which is not excluded here). Then its
        // `tqe_prev` names this head's `tqh_first` (excluded above) or, made by `link_of`, the
        // `tqe_next` of the previous element with that element's provenance; `container_of`
        // steps back by the entry's offset to it, and it is linked, so live. An element in no
        // queue has `None` (never linked, or cleared by `remove`).
        Some(unsafe { elem.as_ref() })
    }

    /// `TAILQ_FOREACH` and `TAILQ_FOREACH_SAFE`.
    pub fn iter(&self) -> TailqIter<'_, A> {
        TailqIter(Cursor::new(self.tqh_first.get(), NextStep::new()))
    }

    /// `TAILQ_FOREACH_REVERSE` and `TAILQ_FOREACH_REVERSE_SAFE`.
    pub fn iter_reverse(&self) -> TailqIterReverse<'_, A> {
        TailqIterReverse(Cursor::new(self.last().map(NonNull::from), self))
    }

    /// The link `tqh_last` designates: the last element's `tqe_next`, or `tqh_first`.
    fn last_link(&self) -> &Link<A::Elem> {
        self.tqh_last.target().unwrap_or(&self.tqh_first)
    }

    /// `tqh_last` as a value to store in an element's `tqe_prev`: the last element's
    /// `tqe_next` with that element's provenance, or the head's `tqh_first`.
    fn last_ptr(&self) -> NonNull<Link<A::Elem>> {
        self.tqh_last
            .get()
            .unwrap_or_else(|| NonNull::from(&self.tqh_first))
    }

    /// Stores `link` as `tqh_last`, folding the head's own link back into the `None` sentinel.
    fn set_last(&self, link: Option<NonNull<Link<A::Elem>>>) {
        if link == Some(NonNull::from(&self.tqh_first)) {
            self.tqh_last.set(None);
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
        entry.tqe_next.set(self.tqh_first.get());
        match self.tqh_first.target() {
            Some(first) => A::entry(first).tqe_prev.set(Some(link_of::<A>(elem))),
            None => self.tqh_last.set(Some(link_of::<A>(elem))),
        }
        self.tqh_first.set_ref(elem);
        entry.tqe_prev.set_ref(&self.tqh_first);
    }

    /// `TAILQ_INSERT_TAIL`.
    ///
    /// # Safety
    ///
    /// As for [`insert_head`](Self::insert_head).
    pub unsafe fn insert_tail(&self, elem: &A::Elem) {
        let entry = A::entry(elem);
        entry.tqe_next.set(None);
        entry.tqe_prev.set(Some(self.last_ptr()));
        self.last_link().set_ref(elem);
        self.tqh_last.set(Some(link_of::<A>(elem)));
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
        entry.tqe_next.set(after.tqe_next.get());
        match after.tqe_next.target() {
            Some(next) => A::entry(next).tqe_prev.set(Some(link_of::<A>(elem))),
            None => self.tqh_last.set(Some(link_of::<A>(elem))),
        }
        after.tqe_next.set_ref(elem);
        entry.tqe_prev.set(Some(link_of::<A>(listelm)));
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
        let Some(prev) = before.tqe_prev.target() else {
            queue_panic("TAILQ_INSERT_BEFORE: listelm not linked");
        };
        entry.tqe_prev.set(before.tqe_prev.get());
        entry.tqe_next.set_ref(listelm);
        prev.set_ref(elem);
        before.tqe_prev.set(Some(link_of::<A>(elem)));
    }

    /// `TAILQ_REMOVE`: unlinks `elem` in O(1) and invalidates its links.
    ///
    /// # Safety
    ///
    /// `elem` is in this queue. An element in no queue, one already removed included, panics
    /// the kernel, where the C dereferences `NULL` (or `_Q_INVALID`).
    pub unsafe fn remove(&self, elem: &A::Elem) {
        let entry = A::entry(elem);
        let Some(prev) = entry.tqe_prev.target() else {
            queue_panic("TAILQ_REMOVE: element not linked");
        };
        match entry.tqe_next.target() {
            Some(next) => A::entry(next).tqe_prev.set(entry.tqe_prev.get()),
            None => self.set_last(entry.tqe_prev.get()),
        }
        prev.set(entry.tqe_next.get());
        invalidate(&entry.tqe_prev);
        invalidate(&entry.tqe_next);
    }

    /// `TAILQ_REPLACE`: puts `elem2` where `elem` is and unlinks `elem`, invalidating its links.
    ///
    /// # Safety
    ///
    /// `elem` is in this queue; `elem2` is in no queue of `A` and stays valid and in place until
    /// it is unlinked.
    pub unsafe fn replace(&self, elem: &A::Elem, elem2: &A::Elem) {
        let old = A::entry(elem);
        let new = A::entry(elem2);
        let Some(prev) = old.tqe_prev.target() else {
            queue_panic("TAILQ_REPLACE: element not linked");
        };
        new.tqe_next.set(old.tqe_next.get());
        match new.tqe_next.target() {
            Some(next) => A::entry(next).tqe_prev.set(Some(link_of::<A>(elem2))),
            None => self.tqh_last.set(Some(link_of::<A>(elem2))),
        }
        new.tqe_prev.set(old.tqe_prev.get());
        prev.set_ref(elem2);
        invalidate(&old.tqe_prev);
        invalidate(&old.tqe_next);
    }

    /// `TAILQ_CONCAT`: moves every element of `head2` to the end of this queue.
    ///
    /// # Safety
    ///
    /// Both heads stay in place while their queues are not empty.
    pub unsafe fn concat(&self, head2: &Self) {
        if let Some(first2) = head2.tqh_first.target() {
            A::entry(first2).tqe_prev.set(Some(self.last_ptr()));
            self.last_link().set_ref(first2);
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
pub struct TailqIter<'a, A: Adapter>(Cursor<'a, A::Elem, NextStep<A>>);

impl<'a, A: TailqAdapter> Iterator for TailqIter<'a, A> {
    type Item = &'a A::Elem;

    fn next(&mut self) -> Option<&'a A::Elem> {
        self.0.advance()
    }
}

/// Reverse iterator over a [`TailqHead`]; see the module docs for its `_SAFE` behaviour.
pub struct TailqIterReverse<'a, A: Adapter>(Cursor<'a, A::Elem, &'a TailqHead<A>>);

impl<'a, A: TailqAdapter> Iterator for TailqIterReverse<'a, A> {
    type Item = &'a A::Elem;

    fn next(&mut self) -> Option<&'a A::Elem> {
        self.0.advance()
    }
}

/*
 * Singly-linked Tail queue declarations.
 */

/// `STAILQ_ENTRY(type)`: the link an element embeds to be in a singly-linked tail queue.
#[repr(C)]
pub struct StailqEntry<T> {
    stqe_next: Link<T>,
}

impl<T> StailqEntry<T> {
    /// An entry that is in no queue.
    pub const fn new() -> Self {
        Self {
            stqe_next: Link::new(),
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
    stqh_first: Link<A::Elem>,
    /// The last element's `stqe_next`; `None` stands for `&stqh_first` (empty queue).
    stqh_last: Link<Link<A::Elem>>,
}

impl<A: StailqAdapter> StailqHead<A> {
    /// `STAILQ_HEAD_INITIALIZER`: an empty queue.
    pub const fn new() -> Self {
        Self {
            stqh_first: Link::new(),
            stqh_last: Link::new(),
        }
    }

    /// `STAILQ_INIT`: empties the queue without touching the elements.
    pub fn init(&self) {
        self.stqh_first.set(None);
        self.stqh_last.set(None);
    }

    /// `STAILQ_FIRST`; `None` is `STAILQ_END`.
    pub fn first(&self) -> Option<&A::Elem> {
        self.stqh_first.target()
    }

    /// `STAILQ_LAST`.
    pub fn last(&self) -> Option<&A::Elem> {
        let elem = container_of::<A>(self.stqh_last.get()?)?;
        // SAFETY: a `Some` `stqh_last` is made by `link_of` from the last element of this
        // queue, so it is that element's `stqe_next` with the provenance of the whole element;
        // `container_of` steps back by the entry's offset to the element, which is linked and
        // therefore live. The reference lives as long as the borrow of the head.
        Some(unsafe { elem.as_ref() })
    }

    /// `STAILQ_EMPTY`.
    pub fn is_empty(&self) -> bool {
        self.stqh_first.is_null()
    }

    /// `STAILQ_NEXT`: the element after `elem`, which must be in the queue or never linked
    /// (module docs, "Deviations").
    pub fn next(elem: &A::Elem) -> Option<&A::Elem> {
        A::entry(elem).stqe_next.target()
    }

    /// `STAILQ_FOREACH` and `STAILQ_FOREACH_SAFE`.
    pub fn iter(&self) -> StailqIter<'_, A> {
        StailqIter(Cursor::new(self.stqh_first.get(), NextStep::new()))
    }

    /// The link `stqh_last` designates: the last element's `stqe_next`, or `stqh_first`.
    fn last_link(&self) -> &Link<A::Elem> {
        self.stqh_last.target().unwrap_or(&self.stqh_first)
    }

    /// `STAILQ_INSERT_HEAD`.
    ///
    /// # Safety
    ///
    /// `elem` is in no queue of `A` and stays valid and in place until it is unlinked.
    pub unsafe fn insert_head(&self, elem: &A::Elem) {
        let entry = A::entry(elem);
        entry.stqe_next.set(self.stqh_first.get());
        if entry.stqe_next.is_null() {
            self.stqh_last.set(Some(link_of::<A>(elem)));
        }
        self.stqh_first.set_ref(elem);
    }

    /// `STAILQ_INSERT_TAIL`.
    ///
    /// # Safety
    ///
    /// As for [`insert_head`](Self::insert_head).
    pub unsafe fn insert_tail(&self, elem: &A::Elem) {
        A::entry(elem).stqe_next.set(None);
        self.last_link().set_ref(elem);
        self.stqh_last.set(Some(link_of::<A>(elem)));
    }

    /// `STAILQ_INSERT_AFTER`: links `elem` after `listelm`.
    ///
    /// # Safety
    ///
    /// `listelm` is in this queue; `elem` is in no queue of `A` and stays valid and in place
    /// until it is unlinked.
    pub unsafe fn insert_after(&self, listelm: &A::Elem, elem: &A::Elem) {
        let after = &A::entry(listelm).stqe_next;
        let entry = A::entry(elem);
        entry.stqe_next.set(after.get());
        if entry.stqe_next.is_null() {
            self.stqh_last.set(Some(link_of::<A>(elem)));
        }
        after.set_ref(elem);
    }

    /// `STAILQ_REMOVE_HEAD`: unlinks the first element. Its link still names its successor.
    ///
    /// # Safety
    ///
    /// The queue is not empty (otherwise the kernel panics, where the C dereferences `NULL`).
    /// The removed element's link still names its old successor, as in C: the caller does not
    /// pass the removed element to [`next`](Self::next) once that successor is unlinked and
    /// freed, until the element is inserted again (the safe reader cannot check it).
    pub unsafe fn remove_head(&self) {
        let Some(first) = self.stqh_first.target() else {
            queue_panic("STAILQ_REMOVE_HEAD: empty");
        };
        self.stqh_first.set(A::entry(first).stqe_next.get());
        if self.stqh_first.is_null() {
            self.stqh_last.set(None);
        }
    }

    /// `STAILQ_REMOVE_AFTER`: unlinks the element after `elem`. Its link still names its
    /// successor.
    ///
    /// # Safety
    ///
    /// `elem` is in this queue and has a successor (otherwise the kernel panics, where the C
    /// dereferences `NULL`).
    /// The removed element's link still names its old successor, as in C: the caller does not
    /// pass the removed element to [`next`](Self::next) once that successor is unlinked and
    /// freed, until the element is inserted again (the safe reader cannot check it).
    pub unsafe fn remove_after(&self, elem: &A::Elem) {
        let link = &A::entry(elem).stqe_next;
        let Some(next) = link.target() else {
            queue_panic("STAILQ_REMOVE_AFTER: no successor");
        };
        link.set(A::entry(next).stqe_next.get());
        if link.is_null() {
            self.stqh_last.set(Some(link_of::<A>(elem)));
        }
    }

    /// `STAILQ_REMOVE`: unlinks `elem`, walking the queue to find its predecessor (O(n)). As in
    /// C, its link is left as it is.
    ///
    /// # Safety
    ///
    /// `elem` is in this queue (otherwise the walk ends in a kernel panic, where the C
    /// dereferences `NULL`).
    /// The removed element's link still names its old successor, as in C: the caller does not
    /// pass the removed element to [`next`](Self::next) once that successor is unlinked and
    /// freed, until the element is inserted again (the safe reader cannot check it).
    pub unsafe fn remove(&self, elem: &A::Elem) {
        let next = A::entry(elem).stqe_next.get();
        // The element whose `stqe_next` names `elem`, `None` for the head's `stqh_first`.
        let mut before: Option<&A::Elem> = None;
        let mut link = &self.stqh_first;
        loop {
            if link.names(elem) {
                link.set(next);
                if next.is_none() {
                    self.stqh_last.set(before.map(link_of::<A>));
                }
                return;
            }
            match link.target() {
                Some(cur) => {
                    before = Some(cur);
                    link = &A::entry(cur).stqe_next;
                }
                None => queue_panic("STAILQ_REMOVE: element not in the queue"),
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
pub struct StailqIter<'a, A: Adapter>(Cursor<'a, A::Elem, NextStep<A>>);

impl<'a, A: StailqAdapter> Iterator for StailqIter<'a, A> {
    type Item = &'a A::Elem;

    fn next(&mut self) -> Option<&'a A::Elem> {
        self.0.advance()
    }
}

/// Names the entry field a list uses inside its element type: the `field` argument of the C
/// macros, fixed once per head type. Made with [`crate::queue_adapter!`].
///
/// # Contract
///
/// `entry` returns the entry embedded in `elem` at offset `OFFSET`, the same one every time,
/// and nothing else; [`container_of`] trusts it to step back from a link to its element. The
/// trait is safe (`docs/ZERO_UNSAFE.md`, decision 7), so the compiler does not hold an
/// implementer to this: [`crate::queue_adapter!`] alone implements it, with `OFFSET` the
/// `offset_of!` of the very field `entry` projects (and `tree_adapter!` expands to it), and
/// `cargo xtask lz check`, in `just ci`, refuses an impl written anywhere else.
pub trait Adapter {
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

/// An entry with a next link (every family but the XOR queue, whose links are encoded).
trait NextEntry<T> {
    /// `offsetof(entry, xx_next)`.
    const NEXT: usize;
    /// The entry's next link.
    fn next_link(&self) -> &Link<T>;
}

impl<T> NextEntry<T> for SlistEntry<T> {
    const NEXT: usize = offset_of!(Self, sle_next);
    fn next_link(&self) -> &Link<T> {
        &self.sle_next
    }
}

impl<T> NextEntry<T> for ListEntry<T> {
    const NEXT: usize = offset_of!(Self, le_next);
    fn next_link(&self) -> &Link<T> {
        &self.le_next
    }
}

impl<T> NextEntry<T> for SimpleqEntry<T> {
    const NEXT: usize = offset_of!(Self, sqe_next);
    fn next_link(&self) -> &Link<T> {
        &self.sqe_next
    }
}

impl<T> NextEntry<T> for TailqEntry<T> {
    const NEXT: usize = offset_of!(Self, tqe_next);
    fn next_link(&self) -> &Link<T> {
        &self.tqe_next
    }
}

impl<T> NextEntry<T> for StailqEntry<T> {
    const NEXT: usize = offset_of!(Self, stqe_next);
    fn next_link(&self) -> &Link<T> {
        &self.stqe_next
    }
}

/// How a [`Cursor`] moves one element on.
trait Step<T> {
    /// The element after `elem` in the walk's direction.
    fn step(&self, elem: &T) -> Option<NonNull<T>>;
}

impl<A: Adapter> Step<A::Elem> for NextStep<A>
where
    A::Entry: NextEntry<A::Elem>,
{
    fn step(&self, elem: &A::Elem) -> Option<NonNull<A::Elem>> {
        A::entry(elem).next_link().get()
    }
}

impl<A: XsimpleqAdapter> Step<A::Elem> for &XsimpleqHead<A> {
    fn step(&self, elem: &A::Elem) -> Option<NonNull<A::Elem>> {
        self.next(elem).map(NonNull::from)
    }
}

impl<A: TailqAdapter> Step<A::Elem> for &TailqHead<A> {
    fn step(&self, elem: &A::Elem) -> Option<NonNull<A::Elem>> {
        self.prev(elem).map(NonNull::from)
    }
}

/// What the C does by dereferencing `NULL` when a caller breaks an operation's precondition
/// (`what` names the operation and the broken condition): a kernel panic, in every
/// configuration.
#[cold]
#[cfg(not(test))]
fn queue_panic(what: &str) -> ! {
    crate::kern::subr_prf::panic(format_args!("{what}"))
}

/// The host tests' `queue_panic`: an unwinding panic, so that `#[should_panic]` sees it (the
/// host's `panic(9)` ends the test process through `boot`).
#[cold]
#[cfg(test)]
fn queue_panic(what: &str) -> ! {
    std::panic!("{what}")
}

/// `_Q_INVALIDATE`: under feature `diagnostic`, poisons a link of a removed element so that a
/// stale use fails an assertion instead of walking a list; otherwise clears it.
fn invalidate<P>(link: &Link<P>) {
    #[cfg(feature = "diagnostic")]
    link.set(Some(NonNull::without_provenance(NonZero::<usize>::MAX)));
    #[cfg(not(feature = "diagnostic"))]
    link.set(None);
}

/// The next link inside `elem`'s entry, carrying the provenance of the whole element rather than
/// of the link's field, so that [`container_of`] may step back from it to the element.
fn link_of<A: Adapter>(elem: &A::Elem) -> NonNull<Link<A::Elem>>
where
    A::Entry: NextEntry<A::Elem>,
{
    let field = NonNull::from(A::entry(elem).next_link());
    NonNull::from(elem).with_addr(field.addr()).cast()
}

/// The element whose next link is `link`: the inverse of [`link_of`]. Address arithmetic only;
/// the caller dereferences the result where the link is known to be inside a live element.
/// The step back is `A::OFFSET` plus the link's offset in its entry, so the result is the
/// element only because `A::OFFSET` is the offset of the entry [`link_of`] took the link from:
/// [`Adapter`]'s contract, which only `queue_adapter!` implements (`cargo xtask lz check`
/// refuses any other impl); the three readers' `SAFETY:` arguments rest on it.
fn container_of<A: Adapter>(link: NonNull<Link<A::Elem>>) -> Option<NonNull<A::Elem>>
where
    A::Entry: NextEntry<A::Elem>,
{
    let back = A::OFFSET + <A::Entry as NextEntry<A::Elem>>::NEXT;
    NonNull::new(link.as_ptr().wrapping_byte_sub(back).cast::<A::Elem>())
}

// `_Q_INVALID` is the C's `(void *)-1`, the value `invalidate` writes.
const _: () = assert!(NonZero::<usize>::MAX.get() == Q_INVALID);
/* </CODE> */

/* <TESTS> */
#[cfg(test)]
mod tests {
    // Tests of `queue.rs`; see there.

    use std::vec::Vec;

    use super::*;

    /// An element that can sit in one list of every family at once.
    struct Node {
        id: u32,
        sl: SlistEntry<Node>,
        li: ListEntry<Node>,
        sq: SimpleqEntry<Node>,
        xq: XsimpleqEntry<Node>,
        tq: TailqEntry<Node>,
        st: StailqEntry<Node>,
    }

    impl Node {
        const fn new(id: u32) -> Self {
            Self {
                id,
                sl: SlistEntry::new(),
                li: ListEntry::new(),
                sq: SimpleqEntry::new(),
                xq: XsimpleqEntry::new(),
                tq: TailqEntry::new(),
                st: StailqEntry::new(),
            }
        }
    }

    queue_adapter!(Sl: Node, sl => SlistEntry<Node>);
    queue_adapter!(Li: Node, li => ListEntry<Node>);
    queue_adapter!(Sq: Node, sq => SimpleqEntry<Node>);
    queue_adapter!(Xq: Node, xq => XsimpleqEntry<Node>);
    queue_adapter!(Tq: Node, tq => TailqEntry<Node>);
    queue_adapter!(St: Node, st => StailqEntry<Node>);

    fn nodes<const N: usize>() -> [Node; N] {
        core::array::from_fn(|i| Node::new(i as u32 + 1))
    }

    fn ids<'a>(it: impl Iterator<Item = &'a Node>) -> Vec<u32> {
        it.map(|n| n.id).collect()
    }

    fn id(n: Option<&Node>) -> Option<u32> {
        n.map(|n| n.id)
    }

    /// A small deterministic generator (xorshift64) for the model tests.
    struct Rng(u64);

    impl Rng {
        fn below(&mut self, n: usize) -> usize {
            self.0 ^= self.0 << 13;
            self.0 ^= self.0 >> 7;
            self.0 ^= self.0 << 17;
            (self.0 % n as u64) as usize
        }
    }

    #[test]
    fn link_of_and_container_of_are_inverse() {
        let n = Node::new(7);
        assert_eq!(Sl::OFFSET, core::mem::offset_of!(Node, sl));
        assert_eq!(Tq::OFFSET, core::mem::offset_of!(Node, tq));
        assert!(ptr::eq(Tq::entry(&n), &n.tq));
        let link = link_of::<Tq>(&n);
        assert!(ptr::eq(link.as_ptr(), &n.tq.tqe_next));
        assert_eq!(container_of::<Tq>(link), Some(NonNull::from(&n)));
        let link = link_of::<St>(&n);
        assert!(ptr::eq(link.as_ptr(), &n.st.stqe_next));
        assert_eq!(container_of::<St>(link), Some(NonNull::from(&n)));
        // the next link need not be first in its entry: the offsets are added
        assert_eq!(<ListEntry<Node> as NextEntry<Node>>::NEXT, 0);
        assert_eq!(
            <TailqEntry<Node> as NextEntry<Node>>::NEXT,
            core::mem::offset_of!(TailqEntry<Node>, tqe_next)
        );
    }

    #[test]
    fn links_are_pointer_sized() {
        assert_eq!(size_of::<Link<Node>>(), size_of::<*const Node>());
        assert_eq!(size_of::<TailqEntry<Node>>(), 2 * size_of::<*const Node>());
        assert_eq!(size_of::<TailqHead<Tq>>(), 2 * size_of::<*const Node>());
        assert_eq!(size_of::<SlistEntry<Node>>(), size_of::<*const Node>());
    }

    #[test]
    fn empty_heads() {
        let sl = SlistHead::<Sl>::new();
        let li = ListHead::<Li>::new();
        let sq = SimpleqHead::<Sq>::new();
        let xq = XsimpleqHead::<Xq>::new();
        let tq = TailqHead::<Tq>::new();
        let st = StailqHead::<St>::new();
        assert!(sl.is_empty() && li.is_empty() && sq.is_empty());
        assert!(xq.is_empty() && tq.is_empty() && st.is_empty());
        assert!(sl.first().is_none() && li.first().is_none() && sq.first().is_none());
        assert!(xq.first().is_none() && tq.first().is_none() && st.first().is_none());
        assert!(tq.last().is_none() && st.last().is_none());
        assert_eq!(sl.iter().count() + li.iter().count() + sq.iter().count(), 0);
        assert_eq!(xq.iter().count() + tq.iter().count() + st.iter().count(), 0);
        assert_eq!(tq.iter_reverse().count(), 0);
    }

    #[test]
    fn slist() {
        let n = nodes::<4>();
        let h = SlistHead::<Sl>::new();
        // SAFETY: the nodes outlive the head and are linked into one slist each.
        unsafe {
            h.insert_head(&n[2]);
            h.insert_head(&n[0]);
            SlistHead::<Sl>::insert_after(&n[0], &n[1]);
            SlistHead::<Sl>::insert_after(&n[2], &n[3]); // after the last
        }
        assert_eq!(ids(h.iter()), [1, 2, 3, 4]);
        assert_eq!(id(h.first()), Some(1));
        assert_eq!(id(SlistHead::<Sl>::next(&n[1])), Some(3));
        assert_eq!(id(SlistHead::<Sl>::next(&n[3])), None);
        // SAFETY: each element is in the list as the operation requires.
        unsafe {
            h.remove(&n[2]); // middle, O(n) path
            assert_eq!(ids(h.iter()), [1, 2, 4]);
            h.remove(&n[3]); // last
            assert_eq!(ids(h.iter()), [1, 2]);
            h.remove(&n[0]); // first, the head's link
            assert_eq!(ids(h.iter()), [2]);
            h.insert_head(&n[0]);
            SlistHead::<Sl>::remove_after(&n[0]);
            assert_eq!(ids(h.iter()), [1]);
            h.remove_head();
        }
        assert!(h.is_empty());
        h.init();
        assert!(h.is_empty());
    }

    #[test]
    fn slist_single_and_remove_while_iterating() {
        let n = nodes::<5>();
        let h = SlistHead::<Sl>::new();
        // SAFETY: the nodes outlive the head and start unlinked.
        unsafe {
            h.insert_head(&n[0]);
            h.remove(&n[0]);
        }
        assert!(h.is_empty());
        // SAFETY: as above.
        unsafe {
            for node in n.iter().rev() {
                h.insert_head(node);
            }
        }
        for node in h.iter() {
            if node.id != 3 {
                // SAFETY: `node` is in the list; the iterator already read its successor.
                unsafe { h.remove(node) };
            }
        }
        assert_eq!(ids(h.iter()), [3]);
    }

    #[test]
    fn list() {
        let n = nodes::<5>();
        let h = ListHead::<Li>::new();
        // SAFETY: the nodes outlive the head; each operation's precondition holds.
        unsafe {
            h.insert_head(&n[2]);
            h.insert_head(&n[0]);
            ListHead::<Li>::insert_after(&n[0], &n[1]);
            ListHead::<Li>::insert_before(&n[2], &n[3]); // before an element in the middle
            ListHead::<Li>::insert_before(&n[0], &n[4]); // before the first: touches lh_first
        }
        assert_eq!(ids(h.iter()), [5, 1, 2, 4, 3]);
        assert_eq!(id(ListHead::<Li>::next(&n[3])), Some(3));
        // SAFETY: as above.
        unsafe {
            ListHead::<Li>::remove(&n[4]); // first
            ListHead::<Li>::remove(&n[2]); // last
            ListHead::<Li>::remove(&n[1]); // middle
        }
        assert_eq!(ids(h.iter()), [1, 4]);
        let fresh = Node::new(9);
        // SAFETY: `n[3]` is linked, `fresh` is not and outlives the head.
        unsafe { ListHead::<Li>::replace(&n[3], &fresh) }; // the last
        assert_eq!(ids(h.iter()), [1, 9]);
        // SAFETY: `n[0]` is linked and first; `n[3]` was replaced, so it is in no list.
        unsafe { ListHead::<Li>::replace(&n[0], &n[3]) }; // the first: lh_first follows
        assert_eq!(ids(h.iter()), [4, 9]);
        // SAFETY: as above; `n[0]` is in no list again.
        unsafe { ListHead::<Li>::insert_after(&n[3], &n[0]) };
        assert_eq!(ids(h.iter()), [4, 1, 9]);
        // SAFETY: as above.
        unsafe {
            ListHead::<Li>::remove(&fresh);
            ListHead::<Li>::remove(&n[3]);
            ListHead::<Li>::remove(&n[0]);
        }
        assert!(h.is_empty());
        // SAFETY: empty again; the single element's prev is the head's link.
        unsafe {
            h.insert_head(&n[1]);
            ListHead::<Li>::remove(&n[1]);
        }
        assert!(h.is_empty());
    }

    #[test]
    fn list_is_linked() {
        let n = nodes::<2>();
        let h = ListHead::<Li>::new();
        assert!(!n[0].li.is_linked());
        // SAFETY: the nodes outlive the head; each operation's precondition holds.
        unsafe {
            h.insert_head(&n[0]);
            h.insert_head(&n[1]);
        }
        assert!(n[0].li.is_linked() && n[1].li.is_linked());
        // SAFETY: `n[0]` is linked; after the removal it is in no list.
        unsafe {
            ListHead::<Li>::remove(&n[0]);
            n[0].li.clear_prev();
        }
        assert!(!n[0].li.is_linked());
        assert_eq!(ids(h.iter()), [2]);
    }

    #[test]
    fn list_remove_while_iterating() {
        let n = nodes::<6>();
        let h = ListHead::<Li>::new();
        // SAFETY: the nodes outlive the head and start unlinked.
        unsafe {
            for node in n.iter().rev() {
                h.insert_head(node);
            }
        }
        for node in h.iter() {
            if node.id % 2 == 0 {
                // SAFETY: `node` is in the list; the iterator already read its successor.
                unsafe { ListHead::<Li>::remove(node) };
            }
        }
        assert_eq!(ids(h.iter()), [1, 3, 5]);
        for node in h.iter() {
            // SAFETY: as above, every element in turn.
            unsafe { ListHead::<Li>::remove(node) };
        }
        assert!(h.is_empty());
    }

    #[test]
    fn simpleq() {
        let n = nodes::<4>();
        let extra = Node::new(5);
        let h = SimpleqHead::<Sq>::new();
        // SAFETY: the nodes outlive the head; each operation's precondition holds.
        unsafe {
            h.insert_tail(&n[1]); // tail into an empty queue uses the sentinel
            h.insert_head(&n[0]);
            h.insert_tail(&n[3]);
            h.insert_after(&n[1], &n[2]);
        }
        assert_eq!(ids(h.iter()), [1, 2, 3, 4]);
        assert_eq!(id(SimpleqHead::<Sq>::next(&n[0])), Some(2));
        // SAFETY: as above.
        unsafe {
            h.remove_head();
            assert_eq!(ids(h.iter()), [2, 3, 4]);
            h.remove_after(&n[2]); // removes the last: sqh_last must follow
            assert_eq!(ids(h.iter()), [2, 3]);
            h.insert_tail(&n[3]);
            assert_eq!(ids(h.iter()), [2, 3, 4]);
            h.insert_after(&n[3], &n[0]); // after the last: sqh_last must follow
            h.insert_tail(&extra);
            assert_eq!(ids(h.iter()), [2, 3, 4, 1, 5]);
            for _ in 0..5 {
                h.remove_head();
            }
        }
        assert!(h.is_empty());
        // SAFETY: the queue is empty again, so the tail insert goes through the sentinel.
        unsafe { h.insert_tail(&n[0]) };
        assert_eq!(ids(h.iter()), [1]);
        // SAFETY: the only element; removing it brings the sentinel back.
        unsafe {
            h.remove_head();
            h.insert_head(&n[1]); // head into an empty queue sets sqh_last
            h.insert_tail(&n[2]);
        }
        assert_eq!(ids(h.iter()), [2, 3]);
    }

    #[test]
    fn simpleq_concat() {
        let a = nodes::<2>();
        let b = nodes::<2>();
        let ha = SimpleqHead::<Sq>::new();
        let hb = SimpleqHead::<Sq>::new();
        // SAFETY: both empty: nothing moves.
        unsafe { ha.concat(&hb) };
        assert!(ha.is_empty() && hb.is_empty());
        // SAFETY: the nodes outlive the heads and start unlinked.
        unsafe {
            hb.insert_tail(&b[0]);
            ha.concat(&hb); // into an empty queue
        }
        assert_eq!(ids(ha.iter()), [1]);
        assert!(hb.is_empty());
        // SAFETY: as above.
        unsafe {
            ha.insert_tail(&b[1]); // the tail moved with the elements
            hb.insert_tail(&a[0]);
            hb.insert_tail(&a[1]);
            ha.concat(&hb);
        }
        assert_eq!(ids(ha.iter()), [1, 2, 1, 2]);
        assert!(hb.is_empty());
        // SAFETY: `hb` is empty, concat of an empty queue changes nothing; then tail insert works.
        unsafe {
            ha.concat(&hb);
            let c = Node::new(3);
            ha.insert_tail(&c);
            assert_eq!(ids(ha.iter()), [1, 2, 1, 2, 3]);
            ha.init();
        }
    }

    #[test]
    fn simpleq_remove_head_keeps_the_successor_link() {
        // C: `SIMPLEQ_REMOVE_HEAD` does not touch the removed element.
        let n = nodes::<2>();
        let h = SimpleqHead::<Sq>::new();
        // SAFETY: the nodes outlive the head and start unlinked.
        unsafe {
            h.insert_tail(&n[0]);
            h.insert_tail(&n[1]);
            h.remove_head();
        }
        assert_eq!(id(SimpleqHead::<Sq>::next(&n[0])), Some(2));
        assert_eq!(ids(h.iter()), [2]);
    }

    #[test]
    fn xsimpleq_with_and_without_cookie() {
        for cookie in [0usize, 0xdead_beef_cafe_f00d, usize::MAX] {
            let n = nodes::<4>();
            let extra = Node::new(5);
            let h = XsimpleqHead::<Xq>::new();
            h.init(cookie);
            assert!(h.is_empty());
            assert_eq!(id(h.first()), None);
            // SAFETY: the nodes outlive the head; each operation's precondition holds.
            unsafe {
                h.insert_tail(&n[1]);
                h.insert_head(&n[0]);
                h.insert_tail(&n[3]);
                h.insert_after(&n[1], &n[2]);
            }
            assert_eq!(ids(h.iter()), [1, 2, 3, 4], "cookie {cookie:#x}");
            assert_eq!(id(h.next(&n[0])), Some(2));
            assert_eq!(id(h.next(&n[3])), None);
            if cookie != 0 {
                // the stored words are not the raw pointers
                assert_ne!(n[0].xq.sqx_next.get(), &n[1] as *const Node as usize);
            }
            // SAFETY: as above.
            unsafe {
                h.remove_head();
                h.remove_after(&n[2]);
                assert_eq!(ids(h.iter()), [2, 3]);
                h.insert_tail(&n[3]);
                assert_eq!(ids(h.iter()), [2, 3, 4]);
                h.insert_after(&n[3], &n[0]); // after the last: sqx_last must follow
                h.insert_tail(&extra);
                assert_eq!(ids(h.iter()), [2, 3, 4, 1, 5]);
            }
            for _ in h.iter() {
                // SAFETY: the queue is not empty while the loop runs; the iterator already
                // read the next element, and only the first (the one yielded) goes.
                unsafe { h.remove_head() };
            }
            assert!(h.is_empty());
            // SAFETY: empty again; the tail insert goes through the sentinel.
            unsafe { h.insert_tail(&n[0]) };
            assert_eq!(ids(h.iter()), [1]);
            // SAFETY: the only element; then a head insert into the empty queue.
            unsafe {
                h.remove_head();
                h.insert_head(&n[1]);
                h.insert_tail(&n[2]);
            }
            assert_eq!(ids(h.iter()), [2, 3]);
        }
    }

    #[test]
    fn tailq() {
        let n = nodes::<5>();
        let h = TailqHead::<Tq>::new();
        assert_eq!(id(h.last()), None);
        // SAFETY: the nodes outlive the head; each operation's precondition holds.
        unsafe {
            h.insert_tail(&n[2]); // into an empty queue
            h.insert_head(&n[0]);
            h.insert_after(&n[0], &n[1]);
            h.insert_tail(&n[4]);
            TailqHead::<Tq>::insert_before(&n[4], &n[3]);
        }
        assert_eq!(ids(h.iter()), [1, 2, 3, 4, 5]);
        assert_eq!(ids(h.iter_reverse()), [5, 4, 3, 2, 1]);
        assert_eq!(id(h.first()), Some(1));
        assert_eq!(id(h.last()), Some(5));
        assert_eq!(id(TailqHead::<Tq>::next(&n[1])), Some(3));
        assert_eq!(id(h.prev(&n[1])), Some(1));
        assert_eq!(id(h.prev(&n[0])), None);
        assert_eq!(id(TailqHead::<Tq>::next(&n[4])), None);
        // SAFETY: as above.
        unsafe {
            h.remove(&n[4]); // last: tqh_last moves back
            assert_eq!(id(h.last()), Some(4));
            h.remove(&n[0]); // first
            assert_eq!(id(h.first()), Some(2));
            assert_eq!(id(h.prev(&n[1])), None);
            h.remove(&n[2]); // middle
        }
        assert_eq!(ids(h.iter()), [2, 4]);
        assert_eq!(ids(h.iter_reverse()), [4, 2]);
        let fresh = Node::new(9);
        // SAFETY: `n[3]` is the last element, `fresh` is unlinked and outlives the head.
        unsafe { h.replace(&n[3], &fresh) };
        assert_eq!(ids(h.iter()), [2, 9]);
        assert_eq!(id(h.last()), Some(9));
        // SAFETY: `n[1]` is the first element, `n[3]` was replaced so it is in no queue.
        unsafe { h.replace(&n[1], &n[3]) };
        assert_eq!(ids(h.iter()), [4, 9]);
        assert_eq!(ids(h.iter_reverse()), [9, 4]);
        assert_eq!(id(h.prev(&fresh)), Some(4));
        // SAFETY: `n[3]` is first; `n[0]` is in no queue. Before the first touches tqh_first.
        unsafe { TailqHead::<Tq>::insert_before(&n[3], &n[0]) };
        assert_eq!(ids(h.iter()), [1, 4, 9]);
        assert_eq!(id(h.prev(&n[3])), Some(1));
        // SAFETY: after the last moves tqh_last.
        unsafe { h.insert_after(&fresh, &n[1]) };
        assert_eq!(id(h.last()), Some(2));
        assert_eq!(ids(h.iter_reverse()), [2, 9, 4, 1]);
        // SAFETY: as above.
        unsafe {
            h.remove(&fresh);
            h.remove(&n[1]);
            h.remove(&n[0]);
            h.remove(&n[3]);
        }
        assert!(h.is_empty());
        assert_eq!(id(h.last()), None);
        // SAFETY: empty again; both inserts go through the sentinel.
        unsafe {
            h.insert_tail(&n[0]);
            h.insert_tail(&n[1]);
        }
        assert_eq!(ids(h.iter_reverse()), [2, 1]);
        // SAFETY: a single element is first and last; removing it restores the sentinel.
        unsafe {
            h.remove(&n[0]);
            h.remove(&n[1]);
            h.insert_head(&n[2]);
        }
        assert_eq!(id(h.last()), Some(3));
        assert_eq!(id(h.first()), Some(3));
    }

    #[test]
    fn tailq_concat_and_reverse_removal() {
        let a = nodes::<3>();
        let b = nodes::<2>();
        let ha = TailqHead::<Tq>::new();
        let hb = TailqHead::<Tq>::new();
        // SAFETY: the nodes outlive the heads and start unlinked; both empty first.
        unsafe {
            ha.concat(&hb);
            assert!(ha.is_empty() && hb.is_empty());
            for node in &a {
                ha.insert_tail(node);
            }
            for node in &b {
                hb.insert_tail(node);
            }
            ha.concat(&hb);
        }
        assert_eq!(ids(ha.iter()), [1, 2, 3, 1, 2]);
        assert_eq!(ids(ha.iter_reverse()), [2, 1, 3, 2, 1]);
        assert!(hb.is_empty() && hb.last().is_none());
        assert!(ptr::eq(ha.last().map_or(ptr::null(), |e| e), &b[1]));
        assert!(ptr::eq(ha.prev(&b[0]).map_or(ptr::null(), |e| e), &a[2]));
        for node in ha.iter_reverse() {
            if node.id == 1 {
                // SAFETY: `node` is in `ha`; the iterator already read its predecessor.
                unsafe { ha.remove(node) };
            }
        }
        assert_eq!(ids(ha.iter()), [2, 3, 2]);
        // Into an empty queue: the moved first element's prev becomes the new head's link.
        // SAFETY: as above.
        unsafe { hb.concat(&ha) };
        assert!(ha.is_empty());
        assert_eq!(ids(hb.iter_reverse()), [2, 3, 2]);
        assert_eq!(id(hb.prev(&a[1])), None);
        // SAFETY: the tail moved with the elements.
        unsafe { hb.insert_tail(&a[0]) };
        assert_eq!(ids(hb.iter()), [2, 3, 2, 1]);
    }

    #[test]
    fn tailq_set_prev_self() {
        let n = Node::new(1);
        assert!(!n.tq.is_linked());
        // SAFETY: `n` is in no queue and is never inserted; it outlives the test.
        unsafe { n.tq.set_prev_self() };
        assert!(n.tq.is_linked());
        // SAFETY: as above.
        unsafe { n.tq.clear_prev() };
        assert!(!n.tq.is_linked());
    }

    #[test]
    fn stailq() {
        let n = nodes::<4>();
        let h = StailqHead::<St>::new();
        assert_eq!(id(h.last()), None);
        // SAFETY: the nodes outlive the head; each operation's precondition holds.
        unsafe {
            h.insert_tail(&n[1]);
            h.insert_head(&n[0]);
            h.insert_tail(&n[3]);
            h.insert_after(&n[1], &n[2]);
        }
        assert_eq!(ids(h.iter()), [1, 2, 3, 4]);
        assert_eq!(id(h.last()), Some(4));
        assert_eq!(id(StailqHead::<St>::next(&n[2])), Some(4));
        // SAFETY: as above.
        unsafe {
            h.remove(&n[2]); // middle, O(n) path
            assert_eq!(ids(h.iter()), [1, 2, 4]);
            h.remove(&n[3]); // last: stqh_last moves back
            assert_eq!(id(h.last()), Some(2));
            h.remove(&n[0]); // first
            assert_eq!(ids(h.iter()), [2]);
            assert_eq!(id(h.last()), Some(2));
            h.remove(&n[1]); // the only one: the sentinel comes back
        }
        assert!(h.is_empty());
        assert_eq!(id(h.last()), None);
        // SAFETY: as above; all four are unlinked again.
        unsafe {
            h.insert_tail(&n[0]);
            h.insert_after(&n[0], &n[1]); // after the last
            assert_eq!(id(h.last()), Some(2));
            h.remove_after(&n[0]); // the last
            assert_eq!(id(h.last()), Some(1));
            h.remove_head();
        }
        assert!(h.is_empty() && h.last().is_none());
        let hb = StailqHead::<St>::new();
        // SAFETY: as above.
        unsafe {
            h.concat(&hb); // both empty
            hb.insert_tail(&n[2]);
            hb.insert_tail(&n[3]);
            h.concat(&hb); // into an empty queue
            h.insert_tail(&n[0]);
        }
        assert_eq!(ids(h.iter()), [3, 4, 1]);
        assert_eq!(id(h.last()), Some(1));
        assert!(hb.is_empty() && hb.last().is_none());
        for node in h.iter() {
            // SAFETY: `node` is in the queue; the iterator already read its successor.
            unsafe { h.remove(node) };
        }
        assert!(h.is_empty() && h.last().is_none());
    }

    #[test]
    fn one_element_in_several_lists() {
        let n = nodes::<3>();
        let li = ListHead::<Li>::new();
        let tq = TailqHead::<Tq>::new();
        let sq = SimpleqHead::<Sq>::new();
        // SAFETY: the nodes outlive the heads; each node is in at most one list per family.
        unsafe {
            for node in &n {
                li.insert_head(node);
                tq.insert_tail(node);
                sq.insert_tail(node);
            }
            ListHead::<Li>::remove(&n[1]);
        }
        assert_eq!(ids(li.iter()), [3, 1]);
        assert_eq!(ids(tq.iter()), [1, 2, 3]);
        assert_eq!(ids(sq.iter()), [1, 2, 3]);
    }

    #[test]
    fn empty_heads_can_be_moved_and_const_initialised() {
        // A `static` head needs a lock around it (`Cell` is not `Sync`); a const context is fine.
        let list: ListHead<Li> = const { ListHead::new() };
        let entry: TailqEntry<Node> = const { TailqEntry::new() };
        let moved = [SimpleqHead::<Sq>::new(), SimpleqHead::<Sq>::default()];
        assert!(list.is_empty());
        assert!(moved.iter().all(SimpleqHead::is_empty));
        assert!(entry.tqe_next.peek().is_none());
        // An emptied head is movable again: the sentinel, not its own address, marks the end.
        let n = Node::new(1);
        let h = TailqHead::<Tq>::new();
        // SAFETY: `n` outlives the head; removed before the head moves.
        unsafe {
            h.insert_tail(&n);
            h.remove(&n);
        }
        let moved = h;
        // SAFETY: as above.
        unsafe { moved.insert_tail(&n) };
        assert_eq!(ids(moved.iter_reverse()), [1]);
    }

    #[cfg(not(feature = "diagnostic"))]
    #[test]
    fn removed_links_are_cleared() {
        let n = nodes::<3>();
        let tq = TailqHead::<Tq>::new();
        let li = ListHead::<Li>::new();
        let sl = SlistHead::<Sl>::new();
        // SAFETY: the nodes outlive the heads and start unlinked.
        unsafe {
            for node in &n {
                tq.insert_tail(node);
                li.insert_head(node);
                sl.insert_head(node);
            }
            tq.remove(&n[1]);
            ListHead::<Li>::remove(&n[1]);
            sl.remove(&n[1]);
        }
        assert_eq!(id(TailqHead::<Tq>::next(&n[1])), None);
        assert_eq!(id(tq.prev(&n[1])), None);
        assert!(!n[1].tq.is_linked() && !n[1].li.is_linked());
        assert_eq!(id(ListHead::<Li>::next(&n[1])), None);
        assert_eq!(id(SlistHead::<Sl>::next(&n[1])), None);
        let fresh = Node::new(9);
        // SAFETY: `n[0]` is linked in both, `fresh` is in neither and outlives the heads.
        unsafe {
            tq.replace(&n[0], &fresh);
            ListHead::<Li>::replace(&n[0], &n[1]);
        }
        assert!(!n[0].tq.is_linked() && !n[0].li.is_linked());
        assert_eq!(ids(tq.iter()), [9, 3]);
        assert_eq!(ids(li.iter()), [3, 2]);
    }

    // What the C turns into a NULL dereference is a kernel panic in every configuration
    // (`queue_panic`; the host tests' version unwinds, so `should_panic` sees it).

    #[test]
    #[should_panic(expected = "SLIST_REMOVE_HEAD: empty")]
    fn slist_remove_head_of_an_empty_list_panics() {
        let h = SlistHead::<Sl>::new();
        // SAFETY: the precondition is broken on purpose; the panic comes before any change.
        unsafe { h.remove_head() };
    }

    #[test]
    #[should_panic(expected = "SIMPLEQ_REMOVE_HEAD: empty")]
    fn simpleq_remove_head_of_an_empty_queue_panics() {
        let h = SimpleqHead::<Sq>::new();
        // SAFETY: as above.
        unsafe { h.remove_head() };
    }

    #[test]
    #[should_panic(expected = "XSIMPLEQ_REMOVE_HEAD: empty")]
    fn xsimpleq_remove_head_of_an_empty_queue_panics() {
        let h = XsimpleqHead::<Xq>::new();
        h.init(0x1234_5678);
        // SAFETY: as above.
        unsafe { h.remove_head() };
    }

    #[test]
    #[should_panic(expected = "STAILQ_REMOVE_HEAD: empty")]
    fn stailq_remove_head_of_an_empty_queue_panics() {
        let h = StailqHead::<St>::new();
        // SAFETY: as above.
        unsafe { h.remove_head() };
    }

    #[test]
    #[should_panic(expected = "SLIST_REMOVE_AFTER: no successor")]
    fn slist_remove_after_the_last_panics() {
        let n = nodes::<1>();
        let h = SlistHead::<Sl>::new();
        // SAFETY: `n` outlives the head; the second call breaks its precondition on purpose.
        unsafe {
            h.insert_head(&n[0]);
            SlistHead::<Sl>::remove_after(&n[0]);
        }
    }

    #[test]
    #[should_panic(expected = "SIMPLEQ_REMOVE_AFTER: no successor")]
    fn simpleq_remove_after_the_last_panics() {
        let n = nodes::<1>();
        let h = SimpleqHead::<Sq>::new();
        // SAFETY: as above.
        unsafe {
            h.insert_tail(&n[0]);
            h.remove_after(&n[0]);
        }
    }

    #[test]
    #[should_panic(expected = "XSIMPLEQ_REMOVE_AFTER: no successor")]
    fn xsimpleq_remove_after_the_last_panics() {
        let n = nodes::<1>();
        let h = XsimpleqHead::<Xq>::new();
        h.init(0x1234_5678);
        // SAFETY: as above.
        unsafe {
            h.insert_tail(&n[0]);
            h.remove_after(&n[0]);
        }
    }

    #[test]
    #[should_panic(expected = "STAILQ_REMOVE_AFTER: no successor")]
    fn stailq_remove_after_the_last_panics() {
        let n = nodes::<1>();
        let h = StailqHead::<St>::new();
        // SAFETY: as above.
        unsafe {
            h.insert_tail(&n[0]);
            h.remove_after(&n[0]);
        }
    }

    #[test]
    #[should_panic(expected = "SLIST_REMOVE: element not in the list")]
    fn slist_remove_of_an_absent_element_panics() {
        let n = nodes::<2>();
        let h = SlistHead::<Sl>::new();
        // SAFETY: as above; `n[1]` was never inserted.
        unsafe {
            h.insert_head(&n[0]);
            h.remove(&n[1]);
        }
    }

    #[test]
    #[should_panic(expected = "STAILQ_REMOVE: element not in the queue")]
    fn stailq_remove_of_an_absent_element_panics() {
        let n = nodes::<2>();
        let h = StailqHead::<St>::new();
        // SAFETY: as above.
        unsafe {
            h.insert_tail(&n[0]);
            h.remove(&n[1]);
        }
    }

    #[test]
    #[should_panic(expected = "LIST_REMOVE: element not linked")]
    fn list_remove_of_an_unlinked_element_panics() {
        let n = Node::new(1);
        // SAFETY: `n` was never inserted: the precondition is broken on purpose.
        unsafe { ListHead::<Li>::remove(&n) };
    }

    #[test]
    #[should_panic(expected = "TAILQ_REMOVE: element not linked")]
    fn tailq_remove_of_an_unlinked_element_panics() {
        let n = Node::new(1);
        let h = TailqHead::<Tq>::new();
        // SAFETY: as above.
        unsafe { h.remove(&n) };
    }

    // A second removal reads the links the first one cleared; under `diagnostic` they are
    // poisoned instead, and the kernel's `kassert!` (which ends the host test process) fires.
    #[cfg(not(feature = "diagnostic"))]
    #[test]
    #[should_panic(expected = "LIST_REMOVE: element not linked")]
    fn list_double_remove_panics() {
        let n = nodes::<2>();
        let h = ListHead::<Li>::new();
        // SAFETY: the nodes outlive the head; the second removal is the broken precondition.
        unsafe {
            h.insert_head(&n[0]);
            h.insert_head(&n[1]);
            ListHead::<Li>::remove(&n[0]);
            ListHead::<Li>::remove(&n[0]);
        }
    }

    #[cfg(not(feature = "diagnostic"))]
    #[test]
    #[should_panic(expected = "TAILQ_REMOVE: element not linked")]
    fn tailq_double_remove_panics() {
        let n = nodes::<2>();
        let h = TailqHead::<Tq>::new();
        // SAFETY: as above.
        unsafe {
            h.insert_tail(&n[0]);
            h.insert_tail(&n[1]);
            h.remove(&n[1]);
            h.remove(&n[1]);
        }
    }

    #[test]
    #[should_panic(expected = "LIST_INSERT_BEFORE: listelm not linked")]
    fn list_insert_before_an_unlinked_element_panics() {
        let n = nodes::<2>();
        // SAFETY: `n[0]` was never inserted: the precondition is broken on purpose.
        unsafe { ListHead::<Li>::insert_before(&n[0], &n[1]) };
    }

    #[test]
    #[should_panic(expected = "TAILQ_INSERT_BEFORE: listelm not linked")]
    fn tailq_insert_before_an_unlinked_element_panics() {
        let n = nodes::<2>();
        // SAFETY: as above.
        unsafe { TailqHead::<Tq>::insert_before(&n[0], &n[1]) };
    }

    #[test]
    #[should_panic(expected = "LIST_REPLACE: element not linked")]
    fn list_replace_of_an_unlinked_element_panics() {
        let n = nodes::<2>();
        // SAFETY: as above.
        unsafe { ListHead::<Li>::replace(&n[0], &n[1]) };
    }

    #[test]
    #[should_panic(expected = "TAILQ_REPLACE: element not linked")]
    fn tailq_replace_of_an_unlinked_element_panics() {
        let n = nodes::<2>();
        let h = TailqHead::<Tq>::new();
        // SAFETY: as above.
        unsafe { h.replace(&n[0], &n[1]) };
    }

    #[test]
    fn prev_of_a_never_linked_element_is_none() {
        let n = Node::new(1);
        let h = TailqHead::<Tq>::new();
        assert_eq!(id(h.prev(&n)), None);
        assert_eq!(id(TailqHead::<Tq>::next(&n)), None);
        assert_eq!(id(ListHead::<Li>::next(&n)), None);
    }

    /// A model of a doubly-linked queue: the ids in order.
    fn check_tailq(h: &TailqHead<Tq>, model: &[usize], n: &[Node]) {
        let want: Vec<u32> = model.iter().map(|&i| n[i].id).collect();
        assert_eq!(ids(h.iter()), want);
        let mut rev = want.clone();
        rev.reverse();
        assert_eq!(ids(h.iter_reverse()), rev);
        assert_eq!(id(h.first()), want.first().copied());
        assert_eq!(id(h.last()), want.last().copied());
        assert_eq!(h.is_empty(), want.is_empty());
        for (k, &i) in model.iter().enumerate() {
            let prev = k.checked_sub(1).map(|p| n[model[p]].id);
            let next = model.get(k + 1).map(|&x| n[x].id);
            assert_eq!(id(h.prev(&n[i])), prev);
            assert_eq!(id(TailqHead::<Tq>::next(&n[i])), next);
        }
    }

    #[test]
    fn tailq_and_list_against_a_model() {
        const N: usize = 12;
        let n = nodes::<N>();
        let tq = TailqHead::<Tq>::new();
        let li = ListHead::<Li>::new();
        let mut mt: Vec<usize> = Vec::new();
        let mut ml: Vec<usize> = Vec::new();
        let mut rng = Rng(0x9e37_79b9_7f4a_7c15);
        for _ in 0..4000 {
            let i = rng.below(N);
            // tail queue
            let at = mt.iter().position(|&x| x == i);
            match (at, rng.below(6)) {
                (None, 0) => {
                    // SAFETY: `n[i]` is in no tail queue; the nodes outlive the heads.
                    unsafe { tq.insert_head(&n[i]) };
                    mt.insert(0, i);
                }
                (None, 1 | 2) if !mt.is_empty() => {
                    let k = rng.below(mt.len());
                    // SAFETY: `n[mt[k]]` is in `tq`, `n[i]` is in no tail queue.
                    unsafe { tq.insert_after(&n[mt[k]], &n[i]) };
                    mt.insert(k + 1, i);
                }
                (None, 3) if !mt.is_empty() => {
                    let k = rng.below(mt.len());
                    // SAFETY: as above.
                    unsafe { TailqHead::<Tq>::insert_before(&n[mt[k]], &n[i]) };
                    mt.insert(k, i);
                }
                (None, _) => {
                    // SAFETY: as above.
                    unsafe { tq.insert_tail(&n[i]) };
                    mt.push(i);
                }
                (Some(k), 0) => {
                    let j = (0..N).find(|j| !mt.contains(j));
                    if let Some(j) = j {
                        // SAFETY: `n[i]` is in `tq`, `n[j]` is in no tail queue.
                        unsafe { tq.replace(&n[i], &n[j]) };
                        mt[k] = j;
                    }
                }
                (Some(k), _) => {
                    // SAFETY: `n[i]` is in `tq`.
                    unsafe { tq.remove(&n[i]) };
                    mt.remove(k);
                }
            }
            check_tailq(&tq, &mt, &n);
            // list
            let at = ml.iter().position(|&x| x == i);
            match (at, rng.below(5)) {
                (None, 1 | 2) if !ml.is_empty() => {
                    let k = rng.below(ml.len());
                    // SAFETY: `n[ml[k]]` is in `li`, `n[i]` is in no list.
                    unsafe { ListHead::<Li>::insert_after(&n[ml[k]], &n[i]) };
                    ml.insert(k + 1, i);
                }
                (None, 3) if !ml.is_empty() => {
                    let k = rng.below(ml.len());
                    // SAFETY: as above.
                    unsafe { ListHead::<Li>::insert_before(&n[ml[k]], &n[i]) };
                    ml.insert(k, i);
                }
                (None, _) => {
                    // SAFETY: as above.
                    unsafe { li.insert_head(&n[i]) };
                    ml.insert(0, i);
                }
                (Some(k), 0) => {
                    if let Some(j) = (0..N).find(|j| !ml.contains(j)) {
                        // SAFETY: `n[i]` is in `li`, `n[j]` is in no list.
                        unsafe { ListHead::<Li>::replace(&n[i], &n[j]) };
                        ml[k] = j;
                    }
                }
                (Some(k), _) => {
                    // SAFETY: `n[i]` is in `li`.
                    unsafe { ListHead::<Li>::remove(&n[i]) };
                    ml.remove(k);
                }
            }
            let want: Vec<u32> = ml.iter().map(|&x| n[x].id).collect();
            assert_eq!(ids(li.iter()), want);
        }
    }

    #[test]
    fn singly_linked_queues_against_a_model() {
        const N: usize = 10;
        let n = nodes::<N>();
        let sq = SimpleqHead::<Sq>::new();
        let st = StailqHead::<St>::new();
        let sl = SlistHead::<Sl>::new();
        let xq = XsimpleqHead::<Xq>::new();
        xq.init(0x5a5a_a5a5_0ff0_f00f);
        // the same model drives the four: they support the same operations here
        let mut m: Vec<usize> = Vec::new();
        let mut rng = Rng(0x2545_f491_4f6c_dd1d);
        for _ in 0..4000 {
            let i = rng.below(N);
            let at = m.iter().position(|&x| x == i);
            match (at, rng.below(5)) {
                (None, 0) => {
                    // SAFETY: `n[i]` is in no queue of any of the four; the nodes outlive them.
                    unsafe {
                        sq.insert_head(&n[i]);
                        st.insert_head(&n[i]);
                        sl.insert_head(&n[i]);
                        xq.insert_head(&n[i]);
                    }
                    m.insert(0, i);
                }
                (None, 1 | 2) if !m.is_empty() => {
                    let k = rng.below(m.len());
                    let after = &n[m[k]];
                    // SAFETY: `after` is in each queue, `n[i]` in none.
                    unsafe {
                        sq.insert_after(after, &n[i]);
                        st.insert_after(after, &n[i]);
                        SlistHead::<Sl>::insert_after(after, &n[i]);
                        xq.insert_after(after, &n[i]);
                    }
                    m.insert(k + 1, i);
                }
                (None, _) => {
                    // SAFETY: as above; the slist has no tail, so it inserts after its last.
                    unsafe {
                        sq.insert_tail(&n[i]);
                        st.insert_tail(&n[i]);
                        match m.last() {
                            Some(&l) => SlistHead::<Sl>::insert_after(&n[l], &n[i]),
                            None => sl.insert_head(&n[i]),
                        }
                        xq.insert_tail(&n[i]);
                    }
                    m.push(i);
                }
                (Some(0), 0 | 1) => {
                    // SAFETY: the queues are not empty.
                    unsafe {
                        sq.remove_head();
                        st.remove_head();
                        sl.remove_head();
                        xq.remove_head();
                    }
                    m.remove(0);
                }
                (Some(k), 2) if k > 0 => {
                    let before = &n[m[k - 1]];
                    // SAFETY: `before` is in each queue and `n[i]` follows it.
                    unsafe {
                        sq.remove_after(before);
                        st.remove_after(before);
                        SlistHead::<Sl>::remove_after(before);
                        xq.remove_after(before);
                    }
                    m.remove(k);
                }
                (Some(k), _) => {
                    // SAFETY: `n[i]` is in each queue. The simple queues have no arbitrary
                    // remove: they remove after the predecessor, or the head.
                    unsafe {
                        st.remove(&n[i]);
                        sl.remove(&n[i]);
                        if k == 0 {
                            sq.remove_head();
                            xq.remove_head();
                        } else {
                            sq.remove_after(&n[m[k - 1]]);
                            xq.remove_after(&n[m[k - 1]]);
                        }
                    }
                    m.remove(k);
                }
            }
            let want: Vec<u32> = m.iter().map(|&x| n[x].id).collect();
            assert_eq!(ids(sq.iter()), want);
            assert_eq!(ids(st.iter()), want);
            assert_eq!(ids(sl.iter()), want);
            assert_eq!(ids(xq.iter()), want);
            assert_eq!(id(st.last()), want.last().copied());
            assert_eq!(id(sq.first()), want.first().copied());
        }
    }

    #[cfg(feature = "diagnostic")]
    #[test]
    fn removed_links_are_poisoned_under_diagnostic() {
        let n = nodes::<2>();
        let h = TailqHead::<Tq>::new();
        // SAFETY: the nodes outlive the head and start unlinked.
        unsafe {
            h.insert_tail(&n[0]);
            h.insert_tail(&n[1]);
            h.remove(&n[0]);
        }
        let addr = |l: &Link<_>| l.peek().map(|p: NonNull<_>| p.addr().get());
        assert_eq!(addr(&n[0].tq.tqe_next), Some(Q_INVALID));
        assert_eq!(
            n[0].tq.tqe_prev.peek().map(|p| p.addr().get()),
            Some(Q_INVALID)
        );
        // `le_prev != NULL` still holds for a poisoned link, as in C
        assert!(n[0].tq.is_linked());
        assert_eq!(ids(h.iter()), [2]);
    }
}
/* </TESTS> */
