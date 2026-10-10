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

/* <CODE> */
//! Intrusive trees: `<sys/tree.h>`, see `tree(3)` and `RBT_INIT(9)`.
//!
//! Upstream: sys/sys/tree.h @ 3ce1f3f79392
//! LZ: sys/sys/tree.rs@3c62ede68802
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
//! Every link is a [`Link`]: `None` or a pointer to a live element (splay trees) or entry
//! (red-black trees). That is the invariant the safe readers stand on. The `unsafe` mutators uphold it through their callers'
//! contract (an element stays valid and in place while it is linked), a removed element is
//! left holding no links, so no link outlives what it points at, and the links of a poisoned
//! red-black entry are never followed.
//!
//! ## Deviations
//! - `RB_*` and `RBT_*` are one algorithm, `kern/subr_tree.rs`; an `RB_ENTRY` is an `RbtEntry`
//!   whose links point at entries rather than at elements. The two APIs stay distinct.
//! - `*_FOREACH` and `*_FOREACH_SAFE` are one iterator that reads the successor before yielding.
//! - Comparators return `Ordering`; `RB_NEGINF`/`RB_INF` and `SPLAY_NEGINF`/`SPLAY_INF` are
//!   therefore not needed: `min` and `max` are methods.
//! - `name_SPLAY` on an empty tree is a no-op (the C would dereference null), and so is
//!   `name_SPLAY_MINMAX` with `Equal` (the C would loop for ever).
//!
//! ## Redesign
//! - Links are typed, `Cell<Option<NonNull<T>>>` ([`Link`]), instead of cells of `*const T`: an
//!   absent link is `None`, not a null pointer to compare, and a present one can only have come
//!   from a reference.
//! - Splay trees: one dereference in the whole family, `splay_follow`, whose soundness argument
//!   is the invariant above; the splay, its rotations, links and assembly are safe code on
//!   `&Elem` (LZ: raw pointers in `unsafe` blocks). `name_SPLAY` and `name_SPLAY_MINMAX` are
//!   one top-down splay, `splay_by`, steered by a closure (the comparator, or a fixed side), as
//!   the two C macros differ only there. The root is carried in a local and stored once by
//!   `assemble`. `SPLAY_REMOVE` leaves the unlinked element holding no links (the C leaves
//!   them stale), so `SPLAY_NEXT`, `SPLAY_LEFT` and `SPLAY_RIGHT` on it answer `None` instead
//!   of walking into the tree it left. [`SplayIter`] holds a reference, not a raw pointer.
//! - Red-black trees: [`RbtEntry`]'s links are typed too; its colour stays the C's word,
//!   read by the algorithm as an enum (`Color`), and a second word, where the C structure has
//!   padding, marks a poisoned entry, whose links the algorithm never follows. Size and offsets
//!   are asserted; no implicit padding and no invalid bit pattern is left, which pf's byte
//!   copies of `PfSrcNode` and `PfiKif` need. [`RbTree`] is typed by the [`RbType`] that walks it, so a tree
//!   cannot be read with another adapter's offset. The algorithm is safe code over typed
//!   handles (`kern/subr_tree.rs`), with its two dereferences there; this file's `unsafe` is the
//!   mutators' contracts and the two `RbType` implementations. `RB_REMOVE` and `RBT_REMOVE`
//!   leave the element as [`RbtEntry::new`] makes it. [`RbIter`] and [`RbIterReverse`] hold
//!   references, not raw pointers.

use core::cell::Cell;
use core::cmp::Ordering;
use core::marker::PhantomData;
use core::ptr::NonNull;

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

/// A link of a tree entry or head: `None`, or a pointer to the live element (splay trees) or
/// entry (red-black trees) it names. The pointers are made from references (`NonNull::from`),
/// so they carry their element's provenance; who may follow them is said where they are
/// followed (`splay_follow` here, `RbNode` in `kern/subr_tree.rs`).
pub(crate) type Link<T> = Cell<Option<NonNull<T>>>;

/*
 * Splay trees.
 */

/// `SPLAY_ENTRY(type)`: the links an element embeds to be in a splay tree. They point at the
/// left and right elements; an entry in no tree holds no links.
#[repr(C)]
pub struct SplayEntry<T> {
    spe_left: Link<T>,
    spe_right: Link<T>,
}

impl<T> SplayEntry<T> {
    /// An entry that is in no tree.
    pub const fn new() -> Self {
        Self {
            spe_left: Cell::new(None),
            spe_right: Cell::new(None),
        }
    }

    /// The left element, if any.
    fn left(&self) -> Option<&T> {
        splay_follow(&self.spe_left)
    }

    /// The right element, if any.
    fn right(&self) -> Option<&T> {
        splay_follow(&self.spe_right)
    }

    /// Leaves the entry holding no links, as [`new`](Self::new) made it.
    fn clear(&self) {
        self.spe_left.set(None);
        self.spe_right.set(None);
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
    sph_root: Link<A::Elem>,
}

impl<A: SplayAdapter> SplayHead<A> {
    /// `SPLAY_INITIALIZER`: an empty tree.
    pub const fn new() -> Self {
        Self {
            sph_root: Cell::new(None),
        }
    }

    /// `SPLAY_INIT`: empties the tree without touching the elements, which keep their links:
    /// meant for a tree that is empty or whose elements are abandoned.
    pub fn init(&self) {
        self.sph_root.set(None);
    }

    /// `SPLAY_ROOT`.
    pub fn root(&self) -> Option<&A::Elem> {
        splay_follow(&self.sph_root)
    }

    /// `SPLAY_EMPTY`.
    pub fn is_empty(&self) -> bool {
        self.sph_root.get().is_none()
    }

    /// `SPLAY_LEFT`.
    pub fn left(elem: &A::Elem) -> Option<&A::Elem> {
        A::entry(elem).left()
    }

    /// `SPLAY_RIGHT`.
    pub fn right(elem: &A::Elem) -> Option<&A::Elem> {
        A::entry(elem).right()
    }

    /// `name_SPLAY`: the main splay operation, moving the node with `elem`'s key, or the closest
    /// one, to the root. A no-op on an empty tree.
    pub fn splay(&self, elem: &A::Elem) {
        self.splay_by(|node| A::compare(elem, node));
    }

    /// `name_SPLAY_MINMAX`: splays the minimum (`Less`) or the maximum (`Greater`) to the root.
    pub fn splay_minmax(&self, comp: Ordering) {
        self.splay_by(|_| comp);
    }

    /// The top-down splay both of the above run: `dir` says, for a node, on which side of it
    /// the target lies (`Equal`: the node is the target). `name_SPLAY` asks the comparator,
    /// `name_SPLAY_MINMAX` always answers the same side, so it always rotates. Returns the new
    /// root, `None` for an empty tree.
    ///
    /// The tree is taken apart into a left piece (nodes smaller than the target, hung by their
    /// right links under `left`), a right piece (larger, hung by their left links under
    /// `right`) and the middle, rooted at `root`; `node` is the header both pieces start from.
    /// The pieces are put back around the new root at the end ([`assemble`](Self::assemble)).
    fn splay_by(&self, mut dir: impl FnMut(&A::Elem) -> Ordering) -> Option<&A::Elem> {
        let mut root = self.root()?;
        let node = SplayEntry::<A::Elem>::new();
        let mut left = &node;
        let mut right = &node;

        loop {
            match dir(root) {
                Ordering::Less => {
                    let Some(tmp) = A::entry(root).left() else {
                        break;
                    };
                    if dir(tmp) == Ordering::Less {
                        Self::rotate_right(root, tmp);
                        root = tmp;
                    }
                    let Some(next) = A::entry(root).left() else {
                        break;
                    };
                    Self::linkleft(root, &mut right);
                    root = next;
                }
                Ordering::Greater => {
                    let Some(tmp) = A::entry(root).right() else {
                        break;
                    };
                    if dir(tmp) == Ordering::Greater {
                        Self::rotate_left(root, tmp);
                        root = tmp;
                    }
                    let Some(next) = A::entry(root).right() else {
                        break;
                    };
                    Self::linkright(root, &mut left);
                    root = next;
                }
                Ordering::Equal => break,
            }
        }
        self.assemble(root, &node, left, right);
        Some(root)
    }

    /// `SPLAY_ROTATE_RIGHT`: `tmp`, the left child of `root`, takes its place.
    fn rotate_right(root: &A::Elem, tmp: &A::Elem) {
        A::entry(root).spe_left.set(A::entry(tmp).spe_right.get());
        A::entry(tmp).spe_right.set(Some(NonNull::from(root)));
    }

    /// `SPLAY_ROTATE_LEFT`: `tmp`, the right child of `root`, takes its place.
    fn rotate_left(root: &A::Elem, tmp: &A::Elem) {
        A::entry(root).spe_right.set(A::entry(tmp).spe_left.get());
        A::entry(tmp).spe_left.set(Some(NonNull::from(root)));
    }

    /// `SPLAY_LINKLEFT`: hangs `root` (with its right subtree) under `right`'s left, and makes
    /// it the new `right`; the caller descends into `root`'s left subtree.
    fn linkleft<'a>(root: &'a A::Elem, right: &mut &'a SplayEntry<A::Elem>) {
        right.spe_left.set(Some(NonNull::from(root)));
        *right = A::entry(root);
    }

    /// `SPLAY_LINKRIGHT`: hangs `root` (with its left subtree) under `left`'s right, and makes
    /// it the new `left`; the caller descends into `root`'s right subtree.
    fn linkright<'a>(root: &'a A::Elem, left: &mut &'a SplayEntry<A::Elem>) {
        left.spe_right.set(Some(NonNull::from(root)));
        *left = A::entry(root);
    }

    /// `SPLAY_ASSEMBLE`: puts the left and right pieces back around `root`, the new root. The
    /// order of the four stores matters when `left` or `right` is still `node`.
    fn assemble(
        &self,
        root: &A::Elem,
        node: &SplayEntry<A::Elem>,
        left: &SplayEntry<A::Elem>,
        right: &SplayEntry<A::Elem>,
    ) {
        let entry = A::entry(root);
        left.spe_right.set(entry.spe_left.get());
        right.spe_left.set(entry.spe_right.get());
        entry.spe_left.set(node.spe_right.get());
        entry.spe_right.set(node.spe_left.get());
        self.sph_root.set(Some(NonNull::from(root)));
    }

    /// `SPLAY_INSERT`: links `elem` and makes it the root; returns the element already there
    /// with an equal key instead, leaving the tree (and `elem`) unchanged.
    ///
    /// # Safety
    ///
    /// `elem` is in no splay tree of `A`, and stays valid and in place until it is removed (or,
    /// if the tree is abandoned by [`init`](Self::init), for as long as an element of the
    /// abandoned tree is still read through this API): the tree's links point at it.
    pub unsafe fn insert(&self, elem: &A::Elem) -> Option<&A::Elem> {
        let entry = A::entry(elem);
        match self.splay_by(|node| A::compare(elem, node)) {
            None => entry.clear(),
            Some(root) => {
                let root_entry = A::entry(root);
                match A::compare(elem, root) {
                    Ordering::Less => {
                        entry.spe_left.set(root_entry.spe_left.get());
                        entry.spe_right.set(Some(NonNull::from(root)));
                        root_entry.spe_left.set(None);
                    }
                    Ordering::Greater => {
                        entry.spe_right.set(root_entry.spe_right.get());
                        entry.spe_left.set(Some(NonNull::from(root)));
                        root_entry.spe_right.set(None);
                    }
                    Ordering::Equal => return Some(root),
                }
            }
        }
        self.sph_root.set(Some(NonNull::from(elem)));
        None
    }

    /// `SPLAY_REMOVE`: unlinks the element with `elem`'s key and returns `elem`; `None` if no
    /// element has its key. The unlinked element is left holding no links.
    ///
    /// # Safety
    ///
    /// `elem` is in this tree, or in no splay tree of `A`. Once removed, the element that had
    /// `elem`'s key is no longer pointed at by the tree, so it may be freed.
    ///
    /// The types do not enforce the rest (gaps inherited from LZ): no reference to the element
    /// obtained from this tree (`find`, `next`, an iterator, ...) may be used once it is freed,
    /// and nothing may take `&mut` of a linked element, which the tree reads and writes through
    /// shared references.
    pub unsafe fn remove<'a>(&self, elem: &'a A::Elem) -> Option<&'a A::Elem> {
        let root = self.splay_by(|node| A::compare(elem, node))?;
        if A::compare(elem, root) != Ordering::Equal {
            return None;
        }
        let root_entry = A::entry(root);
        match root_entry.left() {
            None => self.sph_root.set(root_entry.spe_right.get()),
            Some(left) => {
                // Every key on the left is smaller, so splaying the left subtree by `elem`'s key
                // brings its maximum up, with no right child to lose.
                let tmp = root_entry.spe_right.get();
                self.sph_root.set(Some(NonNull::from(left)));
                if let Some(top) = self.splay_by(|node| A::compare(elem, node)) {
                    A::entry(top).spe_right.set(tmp);
                }
            }
        }
        root_entry.clear();
        Some(elem)
    }

    /// `SPLAY_FIND`: the element whose key equals `elem`'s, splayed to the root.
    pub fn find(&self, elem: &A::Elem) -> Option<&A::Elem> {
        self.splay_by(|node| A::compare(elem, node))
            .filter(|root| A::compare(elem, root) == Ordering::Equal)
    }

    /// `SPLAY_NEXT`: the in-order successor of `elem`, which is splayed to the root first. For
    /// an element in no tree, `None`.
    pub fn next<'a>(&self, elem: &'a A::Elem) -> Option<&'a A::Elem> {
        self.splay(elem);
        let mut cur = A::entry(elem).right()?;
        while let Some(left) = A::entry(cur).left() {
            cur = left;
        }
        Some(cur)
    }

    /// `SPLAY_MIN`: the element with the smallest key, splayed to the root.
    pub fn min(&self) -> Option<&A::Elem> {
        self.splay_by(|_| Ordering::Less)
    }

    /// `SPLAY_MAX`: the element with the largest key, splayed to the root.
    pub fn max(&self) -> Option<&A::Elem> {
        self.splay_by(|_| Ordering::Greater)
    }

    /// `SPLAY_FOREACH`: in key order; every step splays, as in C.
    pub fn iter(&self) -> SplayIter<'_, A> {
        SplayIter {
            head: self,
            cur: self.min(),
        }
    }
}

impl<A: SplayAdapter> Default for SplayHead<A> {
    fn default() -> Self {
        Self::new()
    }
}

/// In-order iterator over a [`SplayHead`]; the successor is found before the current element
/// is yielded, so the current element may be unlinked while iterating; removing the next one
/// is a caller bug (the iteration then ends quietly after it).
pub struct SplayIter<'a, A: Adapter> {
    head: &'a SplayHead<A>,
    cur: Option<&'a A::Elem>,
}

impl<'a, A: SplayAdapter> Iterator for SplayIter<'a, A> {
    type Item = &'a A::Elem;

    fn next(&mut self) -> Option<&'a A::Elem> {
        let cur = self.cur?;
        self.cur = self.head.next(cur);
        Some(cur)
    }
}

/*
 * Red-black trees: the entry and tree shared by RBT_* and RB_*.
 */

/// The colour of a red-black node, as the algorithm sees the entry's colour word.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub(crate) enum Color {
    /// `RB_BLACK`.
    Black,
    /// `RB_RED`.
    Red,
}

impl Color {
    /// The colour word the C stores (`RB_BLACK`, `RB_RED`).
    pub(crate) const fn word(self) -> u32 {
        match self {
            Color::Black => RB_BLACK,
            Color::Red => RB_RED,
        }
    }
}

/// `struct rb_entry`: the links an element embeds to be in a red-black tree. They point at
/// other entries; the element is `t_offset` bytes before its entry. An entry in no tree holds
/// no links (as `new` makes it, and as `_rb_remove` leaves it), or is poisoned.
///
/// The layout is the C's, three link words and the colour word at the same offsets, with the
/// C's trailing padding made a word of its own: no implicit padding is left, and every bit
/// pattern of every field is a valid value. pf copies structures that embed an entry to and
/// from userland byte for byte (`PfAbi`, `sys/net/pfvar.rs`) and relies on both.
#[repr(C)]
pub struct RbtEntry {
    pub(crate) rbt_parent: Link<RbtEntry>,
    pub(crate) rbt_left: Link<RbtEntry>,
    pub(crate) rbt_right: Link<RbtEntry>,
    /// `rbt_color`: `RB_BLACK` or `RB_RED`; the algorithm reads it as a [`Color`] and treats
    /// any other value as a corrupt tree.
    pub(crate) rbt_color: Cell<u32>,
    /// Non-zero once `RBT_POISON` wrote a non-zero value into the links: they hold that value,
    /// not pointers, and are never followed. It fills what is padding in the C structure.
    pub(crate) rbt_poisoned: Cell<u32>,
}

impl RbtEntry {
    /// An entry that is in no tree.
    pub const fn new() -> Self {
        Self {
            rbt_parent: Cell::new(None),
            rbt_left: Cell::new(None),
            rbt_right: Cell::new(None),
            rbt_color: Cell::new(RB_BLACK),
            rbt_poisoned: Cell::new(0),
        }
    }

    /// The node's colour word, `RB_RED` or `RB_BLACK` (`RB_COLOR`).
    pub fn color(&self) -> u32 {
        self.rbt_color.get()
    }
}

impl Default for RbtEntry {
    fn default() -> Self {
        Self::new()
    }
}

/// `struct rb_tree`: the root link of a red-black tree of `T`, an [`RbType`]. The type
/// parameter ties the tree to the one `RbType` (comparator and entry offset) that walks it.
pub struct RbTree<T> {
    pub(crate) rbt_root: Link<RbtEntry>,
    _type: PhantomData<fn() -> T>,
}

impl<T> RbTree<T> {
    /// An empty tree.
    pub const fn new() -> Self {
        Self {
            rbt_root: Cell::new(None),
            _type: PhantomData,
        }
    }

    /// `_rb_init`: empties the tree without touching the elements, which keep their links:
    /// meant for a tree that is empty or whose elements are abandoned (`uvm_map_teardown`).
    pub fn init(&self) {
        self.rbt_root.set(None);
    }

    /// `_rb_empty`.
    pub fn is_empty(&self) -> bool {
        self.rbt_root.get().is_none()
    }
}

impl<T> Default for RbTree<T> {
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

// SAFETY: `Adapter`'s contract makes `A::OFFSET` the offset of the field `A::entry` projects,
// of type `A::Entry`. `Adapter` is a safe trait, so the compiler does not hold implementers to
// it: `queue_adapter!` (which `tree_adapter!` expands to) is the only impl, computing `OFFSET`
// with `offset_of!` of the same field `entry` returns, and `cargo xtask lz check` (in
// `just ci`) refuses an impl written anywhere else (`docs/ZERO_UNSAFE.md`, decision 7). An
// `RbtAdapter` has `Entry = RbtEntry`, so an `RbtEntry` lies at `OFFSET` in every `A::Elem`.
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

// SAFETY: as for `RbtInfo`, `A::OFFSET` is the offset of an `A::Entry` inside every
// `A::Elem`; for an `RbAdapter` that is an `RbEntry`, whose first field, at offset 0
// (`#[repr(C)]`, asserted at the end of this file), is an `RbtEntry`.
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
    rbh_root: RbTree<RbtInfo<A>>,
}

impl<A: RbtAdapter> RbtHead<A> {
    /// `RBT_INITIALIZER`: an empty tree.
    pub const fn new() -> Self {
        Self {
            rbh_root: RbTree::new(),
        }
    }

    /// `RBT_INIT`: empties the tree without touching the elements, which keep their links:
    /// meant for a tree that is empty or whose elements are abandoned.
    pub fn init(&self) {
        self.rbh_root.init();
    }

    /// `RBT_INSERT`: links `elem`; returns the element already there with an equal key instead,
    /// leaving the tree unchanged.
    ///
    /// # Safety
    ///
    /// `elem` is in no tree of `A` (unlinked, or poisoned), and stays valid and in place until
    /// it is removed (or, if the tree is abandoned by [`init`](Self::init), for as long as an
    /// element of the abandoned tree is still read through this API): the tree's links point at
    /// it.
    pub unsafe fn insert(&self, elem: &A::Elem) -> Option<&A::Elem> {
        // SAFETY: the caller's promise is `_rb_insert`'s precondition, word for word; the tree
        // is this head's, typed by the same `RbType`.
        unsafe { _rb_insert::<RbtInfo<A>>(&self.rbh_root, elem) }
    }

    /// `RBT_REMOVE`: unlinks `elem` and returns it; `elem` is left holding no links.
    ///
    /// # Safety
    ///
    /// `elem` is in this tree. Once removed, no link of the tree points at it any more.
    ///
    /// The types do not enforce the rest (gaps inherited from LZ): no reference to the element
    /// obtained from this tree (`find`, `next`, an iterator, ...) may be used once it is freed,
    /// and nothing may take `&mut` of a linked element, which the tree reads and writes through
    /// shared references.
    pub unsafe fn remove<'a>(&self, elem: &'a A::Elem) -> &'a A::Elem {
        // SAFETY: the caller's promise is `_rb_remove`'s precondition; the tree is this head's.
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
    /// The caller is rebuilding the tree by hand and keeps it consistent: `left` (and every
    /// element it links) stays valid and in place while linked, as for [`insert`](Self::insert),
    /// and the links it touches end up naming each other as a tree of `A`.
    pub unsafe fn set_left(elem: &A::Elem, left: Option<&A::Elem>) {
        // SAFETY: the caller's promise is `_rb_set_left`'s precondition.
        unsafe { _rb_set_left::<RbtInfo<A>>(elem, left) };
    }

    /// `RBT_SET_RIGHT`: relinks without rebalancing.
    ///
    /// # Safety
    ///
    /// As for [`set_left`](Self::set_left).
    pub unsafe fn set_right(elem: &A::Elem, right: Option<&A::Elem>) {
        // SAFETY: the caller's promise is `_rb_set_right`'s precondition.
        unsafe { _rb_set_right::<RbtInfo<A>>(elem, right) };
    }

    /// `RBT_SET_PARENT`: relinks without rebalancing.
    ///
    /// # Safety
    ///
    /// As for [`set_left`](Self::set_left).
    pub unsafe fn set_parent(elem: &A::Elem, parent: Option<&A::Elem>) {
        // SAFETY: the caller's promise is `_rb_set_parent`'s precondition.
        unsafe { _rb_set_parent::<RbtInfo<A>>(elem, parent) };
    }

    /// `RBT_POISON`: fills the links of `elem`, which has been removed, with `poison`, and
    /// marks it poisoned: following its links then panics, where the C would fault on the
    /// poison address, until it is inserted again.
    pub fn poison(elem: &A::Elem, poison: usize) {
        _rb_poison::<RbtInfo<A>>(elem, poison);
    }

    /// `RBT_CHECK`: whether every link of `elem` still holds `poison`.
    pub fn check(elem: &A::Elem, poison: usize) -> bool {
        _rb_check::<RbtInfo<A>>(elem, poison)
    }

    /// `RBT_FOREACH` and `RBT_FOREACH_SAFE`.
    pub fn iter(&self) -> RbIter<'_, RbtInfo<A>> {
        RbIter { cur: self.min() }
    }

    /// `RBT_FOREACH_REVERSE` and `RBT_FOREACH_REVERSE_SAFE`.
    pub fn iter_reverse(&self) -> RbIterReverse<'_, RbtInfo<A>> {
        RbIterReverse { cur: self.max() }
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
    rbh_root: RbTree<RbInfo<A>>,
}

impl<A: RbAdapter> RbHead<A> {
    /// `RB_INITIALIZER`: an empty tree.
    pub const fn new() -> Self {
        Self {
            rbh_root: RbTree::new(),
        }
    }

    /// `RB_INIT`: empties the tree without touching the elements, which keep their links:
    /// meant for a tree that is empty or whose elements are abandoned.
    pub fn init(&self) {
        self.rbh_root.init();
    }

    /// `RB_INSERT`: links `elem`; returns the element already there with an equal key instead,
    /// leaving the tree unchanged.
    ///
    /// # Safety
    ///
    /// `elem` is in no tree of `A` (unlinked, or poisoned), and stays valid and in place until
    /// it is removed (or, if the tree is abandoned by [`init`](Self::init), for as long as an
    /// element of the abandoned tree is still read through this API): the tree's links point at
    /// it.
    pub unsafe fn insert(&self, elem: &A::Elem) -> Option<&A::Elem> {
        // SAFETY: the caller's promise is `_rb_insert`'s precondition, word for word; the tree
        // is this head's, typed by the same `RbType`.
        unsafe { _rb_insert::<RbInfo<A>>(&self.rbh_root, elem) }
    }

    /// `RB_REMOVE`: unlinks `elem` and returns it; `elem` is left holding no links.
    ///
    /// # Safety
    ///
    /// `elem` is in this tree. Once removed, no link of the tree points at it any more.
    ///
    /// The types do not enforce the rest (gaps inherited from LZ): no reference to the element
    /// obtained from this tree (`find`, `next`, an iterator, ...) may be used once it is freed,
    /// and nothing may take `&mut` of a linked element, which the tree reads and writes through
    /// shared references.
    pub unsafe fn remove<'a>(&self, elem: &'a A::Elem) -> &'a A::Elem {
        // SAFETY: the caller's promise is `_rb_remove`'s precondition; the tree is this head's.
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
        RbIter { cur: self.min() }
    }

    /// `RB_FOREACH_REVERSE` and `RB_FOREACH_REVERSE_SAFE`.
    pub fn iter_reverse(&self) -> RbIterReverse<'_, RbInfo<A>> {
        RbIterReverse { cur: self.max() }
    }
}

impl<A: RbAdapter> Default for RbHead<A> {
    fn default() -> Self {
        Self::new()
    }
}

/// In-order iterator over a red-black tree; the successor is read before the current element
/// is yielded, so the current element may be unlinked while iterating (`*_FOREACH_SAFE`).
/// Removing the next element instead is a caller bug, as in C: its links are cleared, so the
/// iteration then ends quietly after it.
pub struct RbIter<'a, T: RbType> {
    cur: Option<&'a T::Elem>,
}

impl<'a, T: RbType> Iterator for RbIter<'a, T> {
    type Item = &'a T::Elem;

    fn next(&mut self) -> Option<&'a T::Elem> {
        let cur = self.cur?;
        self.cur = _rb_next::<T>(cur);
        Some(cur)
    }
}

/// Reverse in-order iterator over a red-black tree; see [`RbIter`].
pub struct RbIterReverse<'a, T: RbType> {
    cur: Option<&'a T::Elem>,
}

impl<'a, T: RbType> Iterator for RbIterReverse<'a, T> {
    type Item = &'a T::Elem;

    fn next(&mut self) -> Option<&'a T::Elem> {
        let cur = self.cur?;
        self.cur = _rb_prev::<T>(cur);
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

/// Follows a splay link (an element's, or the head's root) to the element it points at.
fn splay_follow<T>(link: &Link<T>) -> Option<&T> {
    // SAFETY: a splay link is `None` or points at a live element, the invariant of this module:
    // links are only written with elements the caller lent through `insert`, whose contract keeps
    // them valid and in place until `remove` unlinks them, and `remove` clears the links of the
    // element it unlinks, so no link outlives the element it points at. The pointer came from a
    // `&T` (`NonNull::from`), so it is aligned and carries the element's provenance; the
    // element is only reached through shared references, whose `Cell` links allow mutation.
    link.get().map(|elem| unsafe { elem.as_ref() })
}

// `RbInfo` relies on the `RbtEntry` being first in an `RbEntry`.
const _: () = assert!(core::mem::offset_of!(RbEntry<u8>, inner) == 0);
// An `RbtEntry` is the C's `struct rb_entry` in size and in the colour word's place; the poison
// word fills the C's trailing padding, so none is left (`PfAbi`).
const _: () = assert!(core::mem::size_of::<RbtEntry>() == 4 * core::mem::size_of::<usize>());
const _: () =
    assert!(core::mem::offset_of!(RbtEntry, rbt_color) == 3 * core::mem::size_of::<usize>());
const _: () = assert!(
    core::mem::offset_of!(RbtEntry, rbt_poisoned)
        == 3 * core::mem::size_of::<usize>() + core::mem::size_of::<u32>()
);
/* </CODE> */

/* <TESTS> */
#[cfg(test)]
mod tests {
    // Tests of `tree.rs` (splay trees and the classic `RB_*` API); see there.

    use core::cmp::Ordering;
    use core::ptr;
    use std::vec::Vec;

    use super::*;

    struct Node {
        key: i32,
        sp: SplayEntry<Node>,
        rb: RbEntry<Node>,
    }

    impl Node {
        const fn new(key: i32) -> Self {
            Self {
                key,
                sp: SplayEntry::new(),
                rb: RbEntry::new(),
            }
        }
    }

    fn by_key(a: &Node, b: &Node) -> Ordering {
        a.key.cmp(&b.key)
    }

    crate::tree_adapter!(Sp: Node, sp => SplayEntry<Node>, by_key);
    crate::tree_adapter!(Rb: Node, rb => RbEntry<Node>, by_key);

    const KEYS: [i32; 15] = [50, 20, 70, 10, 30, 60, 80, 25, 35, 65, 5, 15, 90, 1, 99];

    fn nodes() -> [Node; 15] {
        core::array::from_fn(|i| Node::new(KEYS[i]))
    }

    fn sorted() -> Vec<i32> {
        let mut v = KEYS.to_vec();
        v.sort_unstable();
        v
    }

    fn keys<'a>(it: impl Iterator<Item = &'a Node>) -> Vec<i32> {
        it.map(|n| n.key).collect()
    }

    fn key(n: Option<&Node>) -> Option<i32> {
        n.map(|n| n.key)
    }

    /// Checks the red-black invariants of a classic tree and returns its black height.
    fn check_rb(t: &RbHead<Rb>) -> usize {
        fn walk(node: Option<&Node>, parent_red: bool) -> usize {
            let Some(n) = node else { return 1 };
            let red = RbHead::<Rb>::color(n) == RB_RED;
            assert!(
                !(red && parent_red),
                "red node {} under a red parent",
                n.key
            );
            let (l, r) = (RbHead::<Rb>::left(n), RbHead::<Rb>::right(n));
            if let Some(l) = l {
                assert!(l.key < n.key);
                assert!(ptr::eq(RbHead::<Rb>::parent(l).unwrap(), n));
            }
            if let Some(r) = r {
                assert!(r.key > n.key);
                assert!(ptr::eq(RbHead::<Rb>::parent(r).unwrap(), n));
            }
            let (lh, rh) = (walk(l, red), walk(r, red));
            assert_eq!(lh, rh, "black height differs under {}", n.key);
            lh + usize::from(!red)
        }
        if let Some(root) = t.root() {
            assert_eq!(RbHead::<Rb>::color(root), RB_BLACK);
            assert!(RbHead::<Rb>::parent(root).is_none());
        }
        walk(t.root(), false)
    }

    #[test]
    fn splay_insert_find_iterate() {
        let n = nodes();
        let t = SplayHead::<Sp>::new();
        assert!(t.is_empty());
        assert!(t.min().is_none() && t.max().is_none());
        // SAFETY: the nodes outlive the tree and start unlinked.
        unsafe {
            for node in &n {
                assert!(t.insert(node).is_none());
            }
        }
        assert_eq!(keys(t.iter()), sorted());

        // find splays the hit to the root
        assert_eq!(key(t.find(&Node::new(65))), Some(65));
        assert_eq!(key(t.root()), Some(65));
        assert!(t.find(&Node::new(66)).is_none());
        assert_eq!(key(t.min()), Some(1));
        assert_eq!(key(t.root()), Some(1));
        assert_eq!(key(t.max()), Some(99));
        assert_eq!(key(t.root()), Some(99));

        let mut chain = Vec::new();
        let mut cur = t.min();
        while let Some(c) = cur {
            chain.push(c.key);
            cur = t.next(c);
        }
        assert_eq!(chain, sorted());

        // a duplicate key is refused and the existing element returned
        let dup = Node::new(30);
        // SAFETY: `dup` is unlinked; the tree refuses it, so it may go out of scope.
        let existing = unsafe { t.insert(&dup) };
        assert!(ptr::eq(existing.unwrap(), &n[4]));
        assert_eq!(keys(t.iter()), sorted());
    }

    #[test]
    fn splay_remove() {
        let n = nodes();
        let t = SplayHead::<Sp>::new();
        // SAFETY: the nodes outlive the tree; each removed node is linked or already gone.
        unsafe {
            assert!(t.remove(&n[0]).is_none()); // empty tree
            for node in &n {
                t.insert(node);
            }
            let mut expect = sorted();
            for &k in &[50, 1, 99, 30, 65, 20] {
                let node = n.iter().find(|x| x.key == k).unwrap();
                assert!(ptr::eq(t.remove(node).unwrap(), node));
                expect.retain(|&x| x != k);
                assert_eq!(keys(t.iter()), expect);
            }
            assert!(t.remove(&Node::new(1234)).is_none());
            for node in &n {
                t.remove(node);
            }
        }
        assert!(t.is_empty());
        t.init();
        assert!(t.is_empty());
    }

    /// A deterministic xorshift64 generator, so the random sequences are the same every run.
    struct Rng(u64);

    impl Rng {
        fn next(&mut self) -> u64 {
            self.0 ^= self.0 << 13;
            self.0 ^= self.0 >> 7;
            self.0 ^= self.0 << 17;
            self.0
        }

        fn below(&mut self, n: usize) -> usize {
            usize::try_from(self.next() % u64::try_from(n).unwrap()).unwrap()
        }
    }

    /// Checks that a splay tree is a binary search tree and returns its keys in order.
    fn check_splay(t: &SplayHead<Sp>) -> Vec<i32> {
        fn walk(node: Option<&Node>, lo: Option<i32>, hi: Option<i32>, out: &mut Vec<i32>) {
            let Some(n) = node else { return };
            assert!(
                lo.is_none_or(|lo| n.key > lo),
                "{} under a larger key",
                n.key
            );
            assert!(
                hi.is_none_or(|hi| n.key < hi),
                "{} under a smaller key",
                n.key
            );
            walk(SplayHead::<Sp>::left(n), lo, Some(n.key), out);
            out.push(n.key);
            walk(SplayHead::<Sp>::right(n), Some(n.key), hi, out);
        }
        let mut out = Vec::new();
        walk(t.root(), None, None, &mut out);
        out
    }

    #[test]
    fn splay_random_sequences() {
        const N: usize = 64;
        let n: Vec<Node> = (0..N)
            .map(|k| Node::new(i32::try_from(k).unwrap()))
            .collect();
        for seed in 1..=8u64 {
            let mut rng = Rng(seed.wrapping_mul(0x9e37_79b9_7f4a_7c15));
            let t = SplayHead::<Sp>::new();
            let mut linked = [false; N];
            for _ in 0..1000 {
                let i = rng.below(N);
                // SAFETY: the nodes outlive the tree; `linked` says which are in it.
                unsafe {
                    if linked[i] {
                        assert!(ptr::eq(t.remove(&n[i]).unwrap(), &n[i]));
                        assert!(SplayHead::<Sp>::left(&n[i]).is_none());
                        assert!(SplayHead::<Sp>::right(&n[i]).is_none());
                    } else {
                        assert!(t.insert(&n[i]).is_none());
                        assert!(ptr::eq(t.root().unwrap(), &n[i]));
                    }
                }
                linked[i] = !linked[i];
                let expect: Vec<i32> = (0..N)
                    .filter(|&k| linked[k])
                    .map(|k| i32::try_from(k).unwrap())
                    .collect();
                assert_eq!(check_splay(&t), expect);
                let probe = rng.below(N);
                assert_eq!(
                    t.find(&Node::new(i32::try_from(probe).unwrap())).is_some(),
                    linked[probe]
                );
                assert_eq!(check_splay(&t), expect);
            }
            assert_eq!(keys(t.iter()), check_splay(&t));
        }
    }

    #[test]
    fn splay_min_max_next_and_strangers() {
        let n = nodes();
        let t = SplayHead::<Sp>::new();
        // SAFETY: the nodes outlive the tree and start unlinked.
        unsafe {
            for node in &n {
                t.insert(node);
            }
        }
        // min and max splay their element to the root; the order survives every splay
        t.splay_minmax(Ordering::Greater);
        assert_eq!(key(t.root()), Some(99));
        t.splay_minmax(Ordering::Less);
        assert_eq!(key(t.root()), Some(1));
        t.splay_minmax(Ordering::Equal);
        assert_eq!(key(t.root()), Some(1));
        t.splay(&Node::new(31));
        assert!(matches!(key(t.root()), Some(30 | 35)));
        assert_eq!(check_splay(&t), sorted());
        assert_eq!(key(t.next(&n[12])), Some(99));
        assert!(t.next(&n[14]).is_none());
        // an element in no tree has no successor and is not found
        let stranger = Node::new(40);
        assert!(t.next(&stranger).is_none());
        assert!(t.find(&stranger).is_none());
        // removing a stranger whose key is linked unlinks the element with that key
        let twin = Node::new(65);
        // SAFETY: `twin` is in no tree; the element with its key, `n[9]`, is in this one.
        assert!(ptr::eq(unsafe { t.remove(&twin) }.unwrap(), &twin));
        assert!(SplayHead::<Sp>::left(&n[9]).is_none() && SplayHead::<Sp>::right(&n[9]).is_none());
        let expect: Vec<i32> = sorted().into_iter().filter(|&k| k != 65).collect();
        assert_eq!(check_splay(&t), expect);
        assert_eq!(keys(t.iter()), expect);
    }

    #[test]
    fn rb_insert_find_iterate() {
        let n = nodes();
        let t = RbHead::<Rb>::new();
        assert!(t.is_empty());
        // SAFETY: the nodes outlive the tree and start unlinked.
        unsafe {
            for node in &n {
                assert!(t.insert(node).is_none());
                check_rb(&t);
            }
        }
        assert_eq!(keys(t.iter()), sorted());
        let mut rev = sorted();
        rev.reverse();
        assert_eq!(keys(t.iter_reverse()), rev);
        assert_eq!(key(t.min()), Some(1));
        assert_eq!(key(t.max()), Some(99));

        assert_eq!(key(t.find(&Node::new(35))), Some(35));
        assert!(t.find(&Node::new(36)).is_none());
        assert_eq!(key(t.nfind(&Node::new(66))), Some(70));
        assert_eq!(key(t.nfind(&Node::new(99))), Some(99));
        assert!(t.nfind(&Node::new(100)).is_none());
        assert_eq!(key(t.nfind(&Node::new(0))), Some(1));

        let mut chain = Vec::new();
        let mut cur = t.min();
        while let Some(c) = cur {
            chain.push(c.key);
            cur = RbHead::<Rb>::next(c);
        }
        assert_eq!(chain, sorted());
        chain.clear();
        cur = t.max();
        while let Some(c) = cur {
            chain.push(c.key);
            cur = RbHead::<Rb>::prev(c);
        }
        assert_eq!(chain, rev);

        let dup = Node::new(70);
        // SAFETY: `dup` is unlinked; the tree refuses it.
        let existing = unsafe { t.insert(&dup) };
        assert!(ptr::eq(existing.unwrap(), &n[2]));
    }

    #[test]
    fn rb_remove_keeps_invariants() {
        let n = nodes();
        let t = RbHead::<Rb>::new();
        // SAFETY: the nodes outlive the tree; every removed node is linked.
        unsafe {
            for node in &n {
                t.insert(node);
            }
            let mut expect = sorted();
            for &k in &[50, 1, 99, 30, 65, 20, 70, 10, 80, 25, 35, 60, 5, 15, 90] {
                let node = n.iter().find(|x| x.key == k).unwrap();
                assert!(ptr::eq(t.remove(node), node));
                expect.retain(|&x| x != k);
                assert_eq!(keys(t.iter()), expect, "after removing {k}");
                check_rb(&t);
            }
        }
        assert!(t.is_empty());
    }

    #[test]
    fn rb_remove_while_iterating() {
        let n = nodes();
        let t = RbHead::<Rb>::new();
        // SAFETY: the nodes outlive the tree and start unlinked.
        unsafe {
            for node in &n {
                t.insert(node);
            }
        }
        for node in t.iter() {
            if node.key % 10 == 5 {
                // SAFETY: `node` is in the tree; the iterator already read its successor.
                unsafe { t.remove(node) };
            }
        }
        let expect: Vec<i32> = sorted().into_iter().filter(|k| k % 10 != 5).collect();
        assert_eq!(keys(t.iter()), expect);
        check_rb(&t);
    }

    #[test]
    fn rb_random_sequences() {
        const N: usize = 64;
        let n: Vec<Node> = (0..N)
            .map(|k| Node::new(i32::try_from(k).unwrap()))
            .collect();
        for seed in 1..=8u64 {
            let mut rng = Rng(seed.wrapping_mul(0x2545_f491_4f6c_dd1d));
            let t = RbHead::<Rb>::new();
            let mut linked = [false; N];
            for _ in 0..1000 {
                let i = rng.below(N);
                // SAFETY: the nodes outlive the tree; `linked` says which are in it.
                unsafe {
                    if linked[i] {
                        assert!(ptr::eq(t.remove(&n[i]), &n[i]));
                        assert!(RbHead::<Rb>::parent(&n[i]).is_none());
                        assert!(RbHead::<Rb>::left(&n[i]).is_none());
                        assert!(RbHead::<Rb>::right(&n[i]).is_none());
                    } else {
                        assert!(t.insert(&n[i]).is_none());
                        // a second element with the same key is refused
                        let twin = Node::new(n[i].key);
                        assert!(ptr::eq(t.insert(&twin).unwrap(), &n[i]));
                    }
                }
                linked[i] = !linked[i];
                check_rb(&t);
                let expect: Vec<i32> = (0..N)
                    .filter(|&k| linked[k])
                    .map(|k| i32::try_from(k).unwrap())
                    .collect();
                assert_eq!(keys(t.iter()), expect);
                let probe = i32::try_from(rng.below(N + 1)).unwrap();
                assert_eq!(
                    key(t.nfind(&Node::new(probe))),
                    expect.iter().copied().find(|&k| k >= probe)
                );
            }
        }
    }

    #[test]
    fn rb_entry_keeps_the_c_size() {
        // three links and the colour; the poison flag sits in the padding
        assert_eq!(
            core::mem::size_of::<RbtEntry>(),
            4 * core::mem::size_of::<usize>()
        );
        assert_eq!(
            core::mem::size_of::<RbTree<RbInfo<Rb>>>(),
            core::mem::size_of::<usize>()
        );
        let e = RbtEntry::new();
        assert_eq!(e.color(), RB_BLACK);
    }
}
/* </TESTS> */
