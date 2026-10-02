/*	$OpenBSD: tree.h,v 1.31 2023/03/08 04:43:09 guenther Exp $	*/
/* <LICENSES> */
/*
 * Copyright 2002 Niels Provos <provos@citi.umich.edu>
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
 * THIS SOFTWARE IS PROVIDED BY THE AUTHOR ``AS IS'' AND ANY EXPRESS OR
 * IMPLIED WARRANTIES, INCLUDING, BUT NOT LIMITED TO, THE IMPLIED WARRANTIES
 * OF MERCHANTABILITY AND FITNESS FOR A PARTICULAR PURPOSE ARE DISCLAIMED.
 * IN NO EVENT SHALL THE AUTHOR BE LIABLE FOR ANY DIRECT, INDIRECT,
 * INCIDENTAL, SPECIAL, EXEMPLARY, OR CONSEQUENTIAL DAMAGES (INCLUDING, BUT
 * NOT LIMITED TO, PROCUREMENT OF SUBSTITUTE GOODS OR SERVICES; LOSS OF USE,
 * DATA, OR PROFITS; OR BUSINESS INTERRUPTION) HOWEVER CAUSED AND ON ANY
 * THEORY OF LIABILITY, WHETHER IN CONTRACT, STRICT LIABILITY, OR TORT
 * (INCLUDING NEGLIGENCE OR OTHERWISE) ARISING IN ANY WAY OUT OF THE USE OF
 * THIS SOFTWARE, EVEN IF ADVISED OF THE POSSIBILITY OF SUCH DAMAGE.
 */

/*
 * Copyright (c) 2016 David Gwynne <dlg@openbsd.org>
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

//! Intrusive trees: `<sys/tree.h>`, see `tree(3)`.
//!
//! Upstream: sys/sys/tree.h @ 3ce1f3f79392
//!
//! Three families: splay trees ([`SplayHead`]), the classic macro-generated red-black trees
//! ([`RbHead`], `RB_*`) and the red-black trees whose algorithm is a library function
//! ([`RbtHead`], `RBT_*`, implemented in `kern/subr_tree.rs`). An element takes part in a tree
//! by embedding the matching entry ([`SplayEntry`], [`RbEntry`], [`RbtEntry`]).
//!
//! The adapter pattern is `queue.rs`'s with one addition: the `cmp` argument of `*_GENERATE`
//! and the optional augment hook (`RB_AUGMENT`, `t_augment`) belong to the adapter too, as
//! [`TreeAdapter`], made by [`crate::tree_adapter!`]. Readers are safe, mutators `unsafe` with
//! the C precondition as their contract, as in `queue.rs`.
//!
//! ## Deviations
//! - `RB_*` and `RBT_*` are one algorithm, `kern/subr_tree.rs`; an `RB_ENTRY` is an `RbtEntry`
//!   whose links point at entries rather than at elements. The two APIs stay distinct.
//! - `*_FOREACH` and `*_FOREACH_SAFE` are one iterator that reads the successor before yielding.
//! - Comparators return `Ordering`; `RB_NEGINF`/`RB_INF` and `SPLAY_NEGINF`/`SPLAY_INF` are
//!   therefore not needed: `min` and `max` are methods.
//! - `name_SPLAY` on an empty tree is a no-op (the C would dereference null).

use core::cell::Cell;
use core::cmp::Ordering;
use core::marker::PhantomData;
use core::ptr;

use crate::kern::subr_tree::{
    _rb_check, _rb_find, _rb_insert, _rb_left, _rb_max, _rb_min, _rb_next, _rb_nfind, _rb_parent,
    _rb_poison, _rb_prev, _rb_remove, _rb_right, _rb_root, _rb_set_left, _rb_set_parent,
    _rb_set_right, RbType,
};
use crate::sys::queue::Adapter;

/// `RB_BLACK`.
pub const RB_BLACK: u32 = 0;
/// `RB_RED`.
pub const RB_RED: u32 = 1;

/// Generates a zero-sized [`TreeAdapter`]: `tree_adapter!(pub VmMapTree: VmMapEntry, rb_entry =>
/// RbtEntry, cmp)` says that `VmMapTree` keys `VmMapEntry`s through their `rb_entry` field,
/// ordered by `cmp`; add `, augment = f` for an augment hook.
#[macro_export]
macro_rules! tree_adapter {
    ($(#[$meta:meta])* $vis:vis $name:ident: $elem:ty, $field:ident => $entry:ty, $cmp:expr) => {
        $crate::queue_adapter!($(#[$meta])* $vis $name: $elem, $field => $entry);

        impl $crate::sys::tree::TreeAdapter for $name {
            fn compare(a: &$elem, b: &$elem) -> ::core::cmp::Ordering {
                $cmp(a, b)
            }
        }
    };
    ($(#[$meta:meta])* $vis:vis $name:ident: $elem:ty, $field:ident => $entry:ty, $cmp:expr,
     augment = $aug:expr) => {
        $crate::queue_adapter!($(#[$meta])* $vis $name: $elem, $field => $entry);

        impl $crate::sys::tree::TreeAdapter for $name {
            const AUGMENTED: bool = true;

            fn compare(a: &$elem, b: &$elem) -> ::core::cmp::Ordering {
                $cmp(a, b)
            }

            fn augment(elem: &$elem) {
                $aug(elem)
            }
        }
    };
}

/*
 * Splay trees.
 */

/// `SPLAY_ENTRY(type)`: the links an element embeds to be in a splay tree.
#[repr(C)]
pub struct SplayEntry<T> {
    spe_left: Cell<*const T>,
    spe_right: Cell<*const T>,
}

impl<T> SplayEntry<T> {
    /// An entry that is in no tree.
    pub const fn new() -> Self {
        Self {
            spe_left: Cell::new(ptr::null()),
            spe_right: Cell::new(ptr::null()),
        }
    }
}

impl<T> Default for SplayEntry<T> {
    fn default() -> Self {
        Self::new()
    }
}

/// `SPLAY_HEAD(name, type)`: a splay tree of `A::Elem`. Every operation splays the node it
/// touched (or the closest one) to the root, so access locality makes later lookups faster at
/// the cost of writes on every lookup.
pub struct SplayHead<A: Adapter> {
    sph_root: Cell<*const A::Elem>,
}

impl<A: SplayAdapter> SplayHead<A> {
    /// `SPLAY_INITIALIZER`: an empty tree.
    pub const fn new() -> Self {
        Self {
            sph_root: Cell::new(ptr::null()),
        }
    }

    /// `SPLAY_INIT`: empties the tree without touching the elements.
    pub fn init(&self) {
        self.sph_root.set(ptr::null());
    }

    /// `SPLAY_ROOT`.
    pub fn root(&self) -> Option<&A::Elem> {
        // SAFETY: a linked element is valid until unlinked (the mutators' contract).
        unsafe { self.sph_root.get().as_ref() }
    }

    /// `SPLAY_EMPTY`.
    pub fn is_empty(&self) -> bool {
        self.sph_root.get().is_null()
    }

    /// `SPLAY_LEFT`.
    pub fn left(elem: &A::Elem) -> Option<&A::Elem> {
        // SAFETY: as for `root`.
        unsafe { A::entry(elem).spe_left.get().as_ref() }
    }

    /// `SPLAY_RIGHT`.
    pub fn right(elem: &A::Elem) -> Option<&A::Elem> {
        // SAFETY: as for `root`.
        unsafe { A::entry(elem).spe_right.get().as_ref() }
    }

    /// `name_SPLAY`: the main splay operation, moving the node with `elem`'s key, or the closest
    /// one, to the root. A no-op on an empty tree.
    pub fn splay(&self, elem: &A::Elem) {
        let node = SplayEntry::<A::Elem>::new();
        let mut left: *const SplayEntry<A::Elem> = &node;
        let mut right: *const SplayEntry<A::Elem> = &node;

        // SAFETY: every pointer followed is a link of the tree, which only holds live elements;
        // `left` and `right` point at `node` (live for this call) or at linked entries.
        unsafe {
            loop {
                let Some(root) = self.sph_root.get().as_ref() else {
                    return;
                };
                match A::compare(elem, root) {
                    Ordering::Less => {
                        let tmp = A::entry(root).spe_left.get();
                        if tmp.is_null() {
                            break;
                        }
                        if A::compare(elem, &*tmp) == Ordering::Less {
                            self.rotate_right(tmp);
                            if A::entry(&*self.sph_root.get()).spe_left.get().is_null() {
                                break;
                            }
                        }
                        self.linkleft(&mut right);
                    }
                    Ordering::Greater => {
                        let tmp = A::entry(root).spe_right.get();
                        if tmp.is_null() {
                            break;
                        }
                        if A::compare(elem, &*tmp) == Ordering::Greater {
                            self.rotate_left(tmp);
                            if A::entry(&*self.sph_root.get()).spe_right.get().is_null() {
                                break;
                            }
                        }
                        self.linkright(&mut left);
                    }
                    Ordering::Equal => break,
                }
            }
            self.assemble(&node, left, right);
        }
    }

    /// `name_SPLAY_MINMAX`: splays the minimum (`Less`) or the maximum (`Greater`) to the root.
    pub fn splay_minmax(&self, comp: Ordering) {
        let node = SplayEntry::<A::Elem>::new();
        let mut left: *const SplayEntry<A::Elem> = &node;
        let mut right: *const SplayEntry<A::Elem> = &node;

        // SAFETY: as for `splay`.
        unsafe {
            loop {
                let Some(root) = self.sph_root.get().as_ref() else {
                    return;
                };
                match comp {
                    Ordering::Less => {
                        let tmp = A::entry(root).spe_left.get();
                        if tmp.is_null() {
                            break;
                        }
                        self.rotate_right(tmp);
                        if A::entry(&*self.sph_root.get()).spe_left.get().is_null() {
                            break;
                        }
                        self.linkleft(&mut right);
                    }
                    Ordering::Greater => {
                        let tmp = A::entry(root).spe_right.get();
                        if tmp.is_null() {
                            break;
                        }
                        self.rotate_left(tmp);
                        if A::entry(&*self.sph_root.get()).spe_right.get().is_null() {
                            break;
                        }
                        self.linkright(&mut left);
                    }
                    Ordering::Equal => break,
                }
            }
            self.assemble(&node, left, right);
        }
    }

    /// `SPLAY_ROTATE_RIGHT`: `tmp` is the root's left child and becomes the root.
    unsafe fn rotate_right(&self, tmp: *const A::Elem) {
        // SAFETY: the caller passes the root's live left child.
        unsafe {
            let root = self.sph_root.get();
            A::entry(&*root)
                .spe_left
                .set(A::entry(&*tmp).spe_right.get());
            A::entry(&*tmp).spe_right.set(root);
            self.sph_root.set(tmp);
        }
    }

    /// `SPLAY_ROTATE_LEFT`: `tmp` is the root's right child and becomes the root.
    unsafe fn rotate_left(&self, tmp: *const A::Elem) {
        // SAFETY: the caller passes the root's live right child.
        unsafe {
            let root = self.sph_root.get();
            A::entry(&*root)
                .spe_right
                .set(A::entry(&*tmp).spe_left.get());
            A::entry(&*tmp).spe_left.set(root);
            self.sph_root.set(tmp);
        }
    }

    /// `SPLAY_LINKLEFT`: hangs the root under `right`'s left and descends left.
    unsafe fn linkleft(&self, right: &mut *const SplayEntry<A::Elem>) {
        // SAFETY: the tree is non-empty and `right` points at a live entry.
        unsafe {
            let root = self.sph_root.get();
            (**right).spe_left.set(root);
            *right = A::entry(&*root);
            self.sph_root.set(A::entry(&*root).spe_left.get());
        }
    }

    /// `SPLAY_LINKRIGHT`: hangs the root under `left`'s right and descends right.
    unsafe fn linkright(&self, left: &mut *const SplayEntry<A::Elem>) {
        // SAFETY: the tree is non-empty and `left` points at a live entry.
        unsafe {
            let root = self.sph_root.get();
            (**left).spe_right.set(root);
            *left = A::entry(&*root);
            self.sph_root.set(A::entry(&*root).spe_right.get());
        }
    }

    /// `SPLAY_ASSEMBLE`: reattaches the left and right pieces around the new root.
    unsafe fn assemble(
        &self,
        node: &SplayEntry<A::Elem>,
        left: *const SplayEntry<A::Elem>,
        right: *const SplayEntry<A::Elem>,
    ) {
        // SAFETY: the tree is non-empty; `left` and `right` point at `node` or at live entries.
        unsafe {
            let root = A::entry(&*self.sph_root.get());
            (*left).spe_right.set(root.spe_left.get());
            (*right).spe_left.set(root.spe_right.get());
            root.spe_left.set(node.spe_right.get());
            root.spe_right.set(node.spe_left.get());
        }
    }

    /// `SPLAY_INSERT`: links `elem` and makes it the root; returns the element already there
    /// with an equal key instead, leaving the tree unchanged.
    ///
    /// # Safety
    ///
    /// `elem` is in no tree of `A` and stays valid and in place until unlinked.
    pub unsafe fn insert(&self, elem: &A::Elem) -> Option<&A::Elem> {
        let entry = A::entry(elem);
        match self.root() {
            None => {
                entry.spe_left.set(ptr::null());
                entry.spe_right.set(ptr::null());
            }
            Some(_) => {
                self.splay(elem);
                // SAFETY: the tree is non-empty, so the root is a live element.
                let root = unsafe { &*self.sph_root.get() };
                let root_entry = A::entry(root);
                match A::compare(elem, root) {
                    Ordering::Less => {
                        entry.spe_left.set(root_entry.spe_left.get());
                        entry.spe_right.set(root);
                        root_entry.spe_left.set(ptr::null());
                    }
                    Ordering::Greater => {
                        entry.spe_right.set(root_entry.spe_right.get());
                        entry.spe_left.set(root);
                        root_entry.spe_right.set(ptr::null());
                    }
                    Ordering::Equal => return Some(root),
                }
            }
        }
        self.sph_root.set(elem);
        None
    }

    /// `SPLAY_REMOVE`: unlinks `elem`; `None` if no element has its key.
    ///
    /// # Safety
    ///
    /// `elem` is in this tree, or in no tree of `A`.
    pub unsafe fn remove<'a>(&self, elem: &'a A::Elem) -> Option<&'a A::Elem> {
        if self.is_empty() {
            return None;
        }
        self.splay(elem);
        // SAFETY: the tree is non-empty, so the root is a live element; after the splay its
        // children are live or null.
        unsafe {
            let root = &*self.sph_root.get();
            if A::compare(elem, root) != Ordering::Equal {
                return None;
            }
            let root_entry = A::entry(root);
            if root_entry.spe_left.get().is_null() {
                self.sph_root.set(root_entry.spe_right.get());
            } else {
                let tmp = root_entry.spe_right.get();
                self.sph_root.set(root_entry.spe_left.get());
                self.splay(elem);
                A::entry(&*self.sph_root.get()).spe_right.set(tmp);
            }
        }
        Some(elem)
    }

    /// `SPLAY_FIND`: the element whose key equals `elem`'s, splayed to the root.
    pub fn find(&self, elem: &A::Elem) -> Option<&A::Elem> {
        if self.is_empty() {
            return None;
        }
        self.splay(elem);
        self.root()
            .filter(|root| A::compare(elem, root) == Ordering::Equal)
    }

    /// `SPLAY_NEXT`: the in-order successor of `elem`, which must be linked; `elem` is splayed
    /// to the root first.
    pub fn next<'a>(&self, elem: &'a A::Elem) -> Option<&'a A::Elem> {
        self.splay(elem);
        let mut cur = A::entry(elem).spe_right.get();
        if cur.is_null() {
            return None;
        }
        // SAFETY: every pointer followed is a link of the tree, which only holds live elements.
        unsafe {
            while !A::entry(&*cur).spe_left.get().is_null() {
                cur = A::entry(&*cur).spe_left.get();
            }
            Some(&*cur)
        }
    }

    /// `SPLAY_MIN`: the element with the smallest key, splayed to the root.
    pub fn min(&self) -> Option<&A::Elem> {
        if self.is_empty() {
            return None;
        }
        self.splay_minmax(Ordering::Less);
        self.root()
    }

    /// `SPLAY_MAX`: the element with the largest key, splayed to the root.
    pub fn max(&self) -> Option<&A::Elem> {
        if self.is_empty() {
            return None;
        }
        self.splay_minmax(Ordering::Greater);
        self.root()
    }

    /// `SPLAY_FOREACH`: in key order; every step splays, as in C.
    pub fn iter(&self) -> SplayIter<'_, A> {
        SplayIter {
            head: self,
            cur: self.min().map_or(ptr::null(), |e| e),
        }
    }
}

impl<A: SplayAdapter> Default for SplayHead<A> {
    fn default() -> Self {
        Self::new()
    }
}

/// In-order iterator over a [`SplayHead`].
pub struct SplayIter<'a, A: Adapter> {
    head: &'a SplayHead<A>,
    cur: *const A::Elem,
}

impl<'a, A: SplayAdapter> Iterator for SplayIter<'a, A> {
    type Item = &'a A::Elem;

    fn next(&mut self) -> Option<&'a A::Elem> {
        // SAFETY: a linked element is valid until unlinked; the successor is found before the
        // current element is yielded.
        let cur = unsafe { self.cur.as_ref()? };
        self.cur = self.head.next(cur).map_or(ptr::null(), |e| e);
        Some(cur)
    }
}

/*
 * Red-black trees: the entry and tree shared by RBT_* and RB_*.
 */

/// `struct rb_entry`: the links an element embeds to be in a red-black tree. They point at
/// other entries; the element is `t_offset` bytes before its entry.
#[repr(C)]
pub struct RbtEntry {
    pub(crate) rbt_parent: Cell<*const RbtEntry>,
    pub(crate) rbt_left: Cell<*const RbtEntry>,
    pub(crate) rbt_right: Cell<*const RbtEntry>,
    pub(crate) rbt_color: Cell<u32>,
}

impl RbtEntry {
    /// An entry that is in no tree.
    pub const fn new() -> Self {
        Self {
            rbt_parent: Cell::new(ptr::null()),
            rbt_left: Cell::new(ptr::null()),
            rbt_right: Cell::new(ptr::null()),
            rbt_color: Cell::new(RB_BLACK),
        }
    }

    /// The node's colour, `RB_RED` or `RB_BLACK`.
    pub fn color(&self) -> u32 {
        self.rbt_color.get()
    }
}

impl Default for RbtEntry {
    fn default() -> Self {
        Self::new()
    }
}

/// `struct rb_tree`: the root link.
pub struct RbTree {
    pub(crate) rbt_root: Cell<*const RbtEntry>,
}

impl RbTree {
    /// An empty tree.
    pub const fn new() -> Self {
        Self {
            rbt_root: Cell::new(ptr::null()),
        }
    }

    /// `_rb_init`.
    pub fn init(&self) {
        self.rbt_root.set(ptr::null());
    }

    /// `_rb_empty`.
    pub fn is_empty(&self) -> bool {
        self.rbt_root.get().is_null()
    }
}

impl Default for RbTree {
    fn default() -> Self {
        Self::new()
    }
}

/// `RB_ENTRY(type)`: the links an element embeds to be in a classic red-black tree. Here it is
/// an [`RbtEntry`] (see the module docs); `T` records the element type the C macro names.
#[repr(C)]
pub struct RbEntry<T> {
    inner: RbtEntry,
    _elem: PhantomData<*const T>,
}

impl<T> RbEntry<T> {
    /// An entry that is in no tree.
    pub const fn new() -> Self {
        Self {
            inner: RbtEntry::new(),
            _elem: PhantomData,
        }
    }
}

impl<T> Default for RbEntry<T> {
    fn default() -> Self {
        Self::new()
    }
}

/// `_name##_RBT_INFO`: the `struct rb_type` of an `RBT_*` adapter.
pub struct RbtInfo<A>(PhantomData<A>);

// SAFETY: `A::OFFSET` is the offset of `A::Entry`, an `RbtEntry`, inside `A::Elem`.
unsafe impl<A: RbtAdapter> RbType for RbtInfo<A> {
    type Elem = A::Elem;
    const OFFSET: usize = A::OFFSET;
    const AUGMENTED: bool = A::AUGMENTED;

    fn compare(a: &A::Elem, b: &A::Elem) -> Ordering {
        A::compare(a, b)
    }

    fn augment(elem: &A::Elem) {
        A::augment(elem);
    }
}

/// The `struct rb_type` of an `RB_*` adapter.
pub struct RbInfo<A>(PhantomData<A>);

// SAFETY: `A::OFFSET` is the offset of `A::Entry`, an `RbEntry` whose first (`#[repr(C)]`)
// field is an `RbtEntry`, inside `A::Elem`.
unsafe impl<A: RbAdapter> RbType for RbInfo<A> {
    type Elem = A::Elem;
    const OFFSET: usize = A::OFFSET;
    const AUGMENTED: bool = A::AUGMENTED;

    fn compare(a: &A::Elem, b: &A::Elem) -> Ordering {
        A::compare(a, b)
    }

    fn augment(elem: &A::Elem) {
        A::augment(elem);
    }
}

/// `RBT_HEAD(name, type)`: a red-black tree of `A::Elem` run by `kern/subr_tree.rs`.
pub struct RbtHead<A: Adapter> {
    rbh_root: RbTree,
    _adapter: PhantomData<A>,
}

impl<A: RbtAdapter> RbtHead<A> {
    /// `RBT_INITIALIZER`: an empty tree.
    pub const fn new() -> Self {
        Self {
            rbh_root: RbTree::new(),
            _adapter: PhantomData,
        }
    }

    /// `RBT_INIT`: empties the tree without touching the elements.
    pub fn init(&self) {
        self.rbh_root.init();
    }

    /// `RBT_INSERT`: links `elem`; returns the element already there with an equal key instead,
    /// leaving the tree unchanged.
    ///
    /// # Safety
    ///
    /// `elem` is in no tree of `A` and stays valid and in place until unlinked.
    pub unsafe fn insert(&self, elem: &A::Elem) -> Option<&A::Elem> {
        // SAFETY: forwarded.
        unsafe { _rb_insert::<RbtInfo<A>>(&self.rbh_root, elem) }
    }

    /// `RBT_REMOVE`: unlinks `elem` and returns it.
    ///
    /// # Safety
    ///
    /// `elem` is in this tree.
    pub unsafe fn remove<'a>(&self, elem: &'a A::Elem) -> &'a A::Elem {
        // SAFETY: forwarded.
        unsafe { _rb_remove::<RbtInfo<A>>(&self.rbh_root, elem) }
    }

    /// `RBT_FIND`: the element whose key equals `key`'s.
    pub fn find(&self, key: &A::Elem) -> Option<&A::Elem> {
        _rb_find::<RbtInfo<A>>(&self.rbh_root, key)
    }

    /// `RBT_NFIND`: the first element whose key is greater than or equal to `key`'s.
    pub fn nfind(&self, key: &A::Elem) -> Option<&A::Elem> {
        _rb_nfind::<RbtInfo<A>>(&self.rbh_root, key)
    }

    /// `RBT_ROOT`.
    pub fn root(&self) -> Option<&A::Elem> {
        _rb_root::<RbtInfo<A>>(&self.rbh_root)
    }

    /// `RBT_EMPTY`.
    pub fn is_empty(&self) -> bool {
        self.rbh_root.is_empty()
    }

    /// `RBT_MIN`.
    pub fn min(&self) -> Option<&A::Elem> {
        _rb_min::<RbtInfo<A>>(&self.rbh_root)
    }

    /// `RBT_MAX`.
    pub fn max(&self) -> Option<&A::Elem> {
        _rb_max::<RbtInfo<A>>(&self.rbh_root)
    }

    /// `RBT_NEXT`: the in-order successor of a linked `elem`.
    pub fn next(elem: &A::Elem) -> Option<&A::Elem> {
        _rb_next::<RbtInfo<A>>(elem)
    }

    /// `RBT_PREV`: the in-order predecessor of a linked `elem`.
    pub fn prev(elem: &A::Elem) -> Option<&A::Elem> {
        _rb_prev::<RbtInfo<A>>(elem)
    }

    /// `RBT_LEFT`.
    pub fn left(elem: &A::Elem) -> Option<&A::Elem> {
        _rb_left::<RbtInfo<A>>(elem)
    }

    /// `RBT_RIGHT`.
    pub fn right(elem: &A::Elem) -> Option<&A::Elem> {
        _rb_right::<RbtInfo<A>>(elem)
    }

    /// `RBT_PARENT`.
    pub fn parent(elem: &A::Elem) -> Option<&A::Elem> {
        _rb_parent::<RbtInfo<A>>(elem)
    }

    /// `RBT_SET_LEFT`: relinks without rebalancing.
    ///
    /// # Safety
    ///
    /// The caller is rebuilding the tree by hand and keeps it consistent.
    pub unsafe fn set_left(elem: &A::Elem, left: Option<&A::Elem>) {
        // SAFETY: forwarded.
        unsafe { _rb_set_left::<RbtInfo<A>>(elem, left) };
    }

    /// `RBT_SET_RIGHT`: relinks without rebalancing.
    ///
    /// # Safety
    ///
    /// As for [`set_left`](Self::set_left).
    pub unsafe fn set_right(elem: &A::Elem, right: Option<&A::Elem>) {
        // SAFETY: forwarded.
        unsafe { _rb_set_right::<RbtInfo<A>>(elem, right) };
    }

    /// `RBT_SET_PARENT`: relinks without rebalancing.
    ///
    /// # Safety
    ///
    /// As for [`set_left`](Self::set_left).
    pub unsafe fn set_parent(elem: &A::Elem, parent: Option<&A::Elem>) {
        // SAFETY: forwarded.
        unsafe { _rb_set_parent::<RbtInfo<A>>(elem, parent) };
    }

    /// `RBT_POISON`: fills the links of an unlinked `elem` with `poison`.
    pub fn poison(elem: &A::Elem, poison: usize) {
        _rb_poison::<RbtInfo<A>>(elem, poison);
    }

    /// `RBT_CHECK`: whether every link of `elem` still holds `poison`.
    pub fn check(elem: &A::Elem, poison: usize) -> bool {
        _rb_check::<RbtInfo<A>>(elem, poison)
    }

    /// `RBT_FOREACH` and `RBT_FOREACH_SAFE`.
    pub fn iter(&self) -> RbIter<'_, RbtInfo<A>> {
        RbIter {
            cur: self.min().map_or(ptr::null(), |e| e),
            _tree: PhantomData,
        }
    }

    /// `RBT_FOREACH_REVERSE` and `RBT_FOREACH_REVERSE_SAFE`.
    pub fn iter_reverse(&self) -> RbIterReverse<'_, RbtInfo<A>> {
        RbIterReverse {
            cur: self.max().map_or(ptr::null(), |e| e),
            _tree: PhantomData,
        }
    }
}

impl<A: RbtAdapter> Default for RbtHead<A> {
    fn default() -> Self {
        Self::new()
    }
}

/// `RB_HEAD(name, type)`: a classic red-black tree of `A::Elem`, served by the same algorithm
/// as [`RbtHead`].
pub struct RbHead<A: Adapter> {
    rbh_root: RbTree,
    _adapter: PhantomData<A>,
}

impl<A: RbAdapter> RbHead<A> {
    /// `RB_INITIALIZER`: an empty tree.
    pub const fn new() -> Self {
        Self {
            rbh_root: RbTree::new(),
            _adapter: PhantomData,
        }
    }

    /// `RB_INIT`: empties the tree without touching the elements.
    pub fn init(&self) {
        self.rbh_root.init();
    }

    /// `RB_INSERT`: links `elem`; returns the element already there with an equal key instead,
    /// leaving the tree unchanged.
    ///
    /// # Safety
    ///
    /// `elem` is in no tree of `A` and stays valid and in place until unlinked.
    pub unsafe fn insert(&self, elem: &A::Elem) -> Option<&A::Elem> {
        // SAFETY: forwarded.
        unsafe { _rb_insert::<RbInfo<A>>(&self.rbh_root, elem) }
    }

    /// `RB_REMOVE`: unlinks `elem` and returns it.
    ///
    /// # Safety
    ///
    /// `elem` is in this tree.
    pub unsafe fn remove<'a>(&self, elem: &'a A::Elem) -> &'a A::Elem {
        // SAFETY: forwarded.
        unsafe { _rb_remove::<RbInfo<A>>(&self.rbh_root, elem) }
    }

    /// `RB_FIND`: the element whose key equals `key`'s.
    pub fn find(&self, key: &A::Elem) -> Option<&A::Elem> {
        _rb_find::<RbInfo<A>>(&self.rbh_root, key)
    }

    /// `RB_NFIND`: the first element whose key is greater than or equal to `key`'s.
    pub fn nfind(&self, key: &A::Elem) -> Option<&A::Elem> {
        _rb_nfind::<RbInfo<A>>(&self.rbh_root, key)
    }

    /// `RB_ROOT`.
    pub fn root(&self) -> Option<&A::Elem> {
        _rb_root::<RbInfo<A>>(&self.rbh_root)
    }

    /// `RB_EMPTY`.
    pub fn is_empty(&self) -> bool {
        self.rbh_root.is_empty()
    }

    /// `RB_MIN`.
    pub fn min(&self) -> Option<&A::Elem> {
        _rb_min::<RbInfo<A>>(&self.rbh_root)
    }

    /// `RB_MAX`.
    pub fn max(&self) -> Option<&A::Elem> {
        _rb_max::<RbInfo<A>>(&self.rbh_root)
    }

    /// `RB_NEXT`: the in-order successor of a linked `elem`.
    pub fn next(elem: &A::Elem) -> Option<&A::Elem> {
        _rb_next::<RbInfo<A>>(elem)
    }

    /// `RB_PREV`: the in-order predecessor of a linked `elem`.
    pub fn prev(elem: &A::Elem) -> Option<&A::Elem> {
        _rb_prev::<RbInfo<A>>(elem)
    }

    /// `RB_LEFT`.
    pub fn left(elem: &A::Elem) -> Option<&A::Elem> {
        _rb_left::<RbInfo<A>>(elem)
    }

    /// `RB_RIGHT`.
    pub fn right(elem: &A::Elem) -> Option<&A::Elem> {
        _rb_right::<RbInfo<A>>(elem)
    }

    /// `RB_PARENT`.
    pub fn parent(elem: &A::Elem) -> Option<&A::Elem> {
        _rb_parent::<RbInfo<A>>(elem)
    }

    /// `RB_COLOR`.
    pub fn color(elem: &A::Elem) -> u32 {
        A::entry(elem).inner.color()
    }

    /// `RB_FOREACH` and `RB_FOREACH_SAFE`.
    pub fn iter(&self) -> RbIter<'_, RbInfo<A>> {
        RbIter {
            cur: self.min().map_or(ptr::null(), |e| e),
            _tree: PhantomData,
        }
    }

    /// `RB_FOREACH_REVERSE` and `RB_FOREACH_REVERSE_SAFE`.
    pub fn iter_reverse(&self) -> RbIterReverse<'_, RbInfo<A>> {
        RbIterReverse {
            cur: self.max().map_or(ptr::null(), |e| e),
            _tree: PhantomData,
        }
    }
}

impl<A: RbAdapter> Default for RbHead<A> {
    fn default() -> Self {
        Self::new()
    }
}

/// In-order iterator over a red-black tree; the successor is read before the current element
/// is yielded, so the current element may be unlinked while iterating.
pub struct RbIter<'a, T: RbType> {
    cur: *const T::Elem,
    _tree: PhantomData<&'a T::Elem>,
}

impl<'a, T: RbType> Iterator for RbIter<'a, T> {
    type Item = &'a T::Elem;

    fn next(&mut self) -> Option<&'a T::Elem> {
        // SAFETY: a linked element is valid until unlinked.
        let cur = unsafe { self.cur.as_ref()? };
        self.cur = _rb_next::<T>(cur).map_or(ptr::null(), |e| e);
        Some(cur)
    }
}

/// Reverse in-order iterator over a red-black tree; see [`RbIter`].
pub struct RbIterReverse<'a, T: RbType> {
    cur: *const T::Elem,
    _tree: PhantomData<&'a T::Elem>,
}

impl<'a, T: RbType> Iterator for RbIterReverse<'a, T> {
    type Item = &'a T::Elem;

    fn next(&mut self) -> Option<&'a T::Elem> {
        // SAFETY: a linked element is valid until unlinked.
        let cur = unsafe { self.cur.as_ref()? };
        self.cur = _rb_prev::<T>(cur).map_or(ptr::null(), |e| e);
        Some(cur)
    }
}

/// An [`Adapter`] with the `cmp` argument of `*_GENERATE` and the augment hook (`RB_AUGMENT`,
/// `t_augment`). Made with [`crate::tree_adapter!`].
pub trait TreeAdapter: Adapter {
    /// Whether [`augment`](Self::augment) does anything (`t_augment != NULL`).
    const AUGMENTED: bool = false;
    /// Orders two elements by key.
    fn compare(a: &Self::Elem, b: &Self::Elem) -> Ordering;
    /// Recomputes an element's cached subtree data after its subtree changed.
    fn augment(_elem: &Self::Elem) {}
}

/// A [`TreeAdapter`] whose entry is a [`SplayEntry`].
pub trait SplayAdapter: TreeAdapter<Entry = SplayEntry<<Self as Adapter>::Elem>> {}
impl<A: TreeAdapter<Entry = SplayEntry<<A as Adapter>::Elem>>> SplayAdapter for A {}

/// A [`TreeAdapter`] whose entry is an [`RbEntry`] (`RB_*`).
pub trait RbAdapter: TreeAdapter<Entry = RbEntry<<Self as Adapter>::Elem>> {}
impl<A: TreeAdapter<Entry = RbEntry<<A as Adapter>::Elem>>> RbAdapter for A {}

/// A [`TreeAdapter`] whose entry is an [`RbtEntry`] (`RBT_*`).
pub trait RbtAdapter: TreeAdapter<Entry = RbtEntry> {}
impl<A: TreeAdapter<Entry = RbtEntry>> RbtAdapter for A {}

// `RbInfo` relies on the `RbtEntry` being first in an `RbEntry`.
const _: () = assert!(core::mem::offset_of!(RbEntry<u8>, inner) == 0);

#[cfg(test)]
mod tests;
