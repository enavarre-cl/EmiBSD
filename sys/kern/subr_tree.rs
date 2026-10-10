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
//! The red-black tree behind `RBT_*` (and, here, `RB_*`): `kern/subr_tree.c`, see
//! `RBT_INIT(9)`.
//!
//! Upstream: sys/kern/subr_tree.c @ 3ce1f3f79392
//! LZ: sys/kern/subr_tree.rs@3c62ede68802
//!
//! The algorithm works on [`RbtEntry`] links (entry to entry, never element to element) and
//! reaches the element only to compare keys or to run the augment hook, through an [`RbType`]:
//! the C `struct rb_type` with its comparator, optional augment and the entry's offset inside
//! the element. `sys/sys/tree.rs` provides one `RbType` per adapter and the typed wrappers.
//!
//! The module is the boundary of its `unsafe`: an entry is reached only as an `RbNode`, a
//! handle whose two constructors are the element's own entry (`rb_n2e`) and a link read out of
//! a tree of the same type; `RbNode::entry` and `rb_e2n` are the only dereferences. Their
//! argument is the invariant every function here keeps: a link of an entry that is not
//! poisoned is `None` or names the live entry of a `T::Elem`. The public mutators that take an
//! element from outside (`_rb_insert`, `_rb_remove`, `_rb_set_*`) are `unsafe`, their callers
//! promising what the invariant needs (the element stays valid and in place while linked);
//! `_rb_remove` leaves the element it unlinks holding no links, and the links of a poisoned
//! entry are never followed.
//!
//! ## Deviations
//! - `struct rb_type` is a trait with associated items instead of a struct of function
//!   pointers; `t_augment == NULL` is the `AUGMENTED` constant.
//! - The comparator returns `Ordering` instead of a negative, zero or positive `int`.
//! - Null results are `None`; the `_rb_*` entry points take and return references, and
//!   those that relink the tree are `unsafe` with the C precondition as their contract.
//! - The classic `RB_*` family of `tree.h`, a macro copy of this algorithm in C, is served by
//!   this one implementation too.
//!
//! ## Redesign
//! - Entries are handled as `RbNode`s, typed by the tree's [`RbType`] (LZ: `*const RbtEntry`
//!   and an `unsafe fn e` turning one into a reference): the algorithm (rotations, the two
//!   colour fixups, removal, insertion, the walks) is safe code, and its `unsafe` is the two
//!   dereferences above (LZ: an `unsafe` block or `unsafe fn` per step). The element's entry is
//!   found by address arithmetic that keeps the element's provenance (`map_addr`), so stepping
//!   back to the element from a link is sound.
//! - [`RbTree`] is typed by its `RbType` (LZ: one untyped root that any `RbType` could read):
//!   a tree cannot be walked with another type's offset.
//! - The mirrored halves of the C (`rbe_rotate_left` and `rbe_rotate_right`, both branches of
//!   `rbe_insert_color` and of `rbe_remove_color`, `_rb_next` and `_rb_prev`, `_rb_min` and
//!   `_rb_max`) are one path each over a `Side`; the C's "relink the parent's child, or the
//!   root" step is `rbe_replace_child`.
//! - `_rb_remove` leaves the unlinked entry as `RbtEntry::new` makes it (no links, black);
//!   the C leaves its links stale, which `RBT_INIT(9)` defines nothing for, and which would
//!   let the safe readers walk from it into memory it no longer owns.
//! - `_rb_poison` marks the entry poisoned (a flag in the C structure's padding) besides
//!   writing the value into its links; following a poisoned entry's links panics, where the C
//!   would fault on the poison address (`_rb_check` still reads the raw values). A structure
//!   the algorithm relies on that is missing (a red root, a black node without a sibling) also
//!   panics, where the C would dereference null.

use core::cmp::Ordering;
use core::marker::PhantomData;
use core::ptr::{self, NonNull};

use crate::kern::subr_prf::panic;
use crate::sys::tree::{Color, Link, RbTree, RbtEntry};

/// Which child of a node: the C's mirrored halves of each step are one path over it.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
enum Side {
    Left,
    Right,
}

impl Side {
    /// The other child.
    fn opposite(self) -> Self {
        match self {
            Side::Left => Side::Right,
            Side::Right => Side::Left,
        }
    }
}

/// A handle on the entry of a `T::Elem` in, or entering, a tree of `T`: the link itself,
/// which carries the element's provenance. Made only by `rb_n2e` (from the element) and by
/// following a link of another handle or of a tree of `T` (`from_link`), so it always names
/// the live entry of a `T::Elem` (see [`entry`](Self::entry)).
struct RbNode<'a, T: RbType> {
    ptr: NonNull<RbtEntry>,
    _life: PhantomData<&'a RbtEntry>,
    _type: PhantomData<fn() -> T>,
}

impl<T: RbType> Clone for RbNode<'_, T> {
    fn clone(&self) -> Self {
        *self
    }
}

impl<T: RbType> Copy for RbNode<'_, T> {}

impl<'a, T: RbType> RbNode<'a, T> {
    /// The handle for a link read out of a tree of `T` (an entry's, or the root).
    fn from_link(ptr: NonNull<RbtEntry>) -> Self {
        Self {
            ptr,
            _life: PhantomData,
            _type: PhantomData,
        }
    }

    /// `e`: the entry the handle names.
    fn entry(self) -> &'a RbtEntry {
        // SAFETY: the handle names the live entry of a `T::Elem`. `rb_n2e` made it from a
        // reference to a live element, `T::OFFSET` bytes in, where `RbType`'s contract puts the
        // entry; or `from_link` made it from a link of a tree of `T`, which by the module's
        // invariant is the live entry of a `T::Elem`: only `rb_n2e` handles are ever stored in
        // links (`_rb_insert`, the relinking steps, `_rb_set_*`), their elements stay valid and
        // in place while linked (the `unsafe` mutators' contract), `rbe_remove` clears the links
        // of the entry it unlinks, and the links of a poisoned entry, which hold no pointers,
        // are never read into a handle (`links`). The pointer is aligned (it came from a
        // reference) and the entry is only reached shared; its fields are `Cell`s.
        unsafe { self.ptr.as_ref() }
    }

    /// The entry, to read its links: refuses a poisoned one, whose links hold the poison value,
    /// not pointers (the C would fault on it).
    fn links(self) -> &'a RbtEntry {
        let rbe = self.entry();
        if rbe.rbt_poisoned.get() {
            rb_poisoned(rbe);
        }
        rbe
    }

    /// The child on `side`, `RBE_LEFT` or `RBE_RIGHT`.
    fn child(self, side: Side) -> Option<Self> {
        let rbe = self.links();
        match side {
            Side::Left => rbe.rbt_left.get(),
            Side::Right => rbe.rbt_right.get(),
        }
        .map(Self::from_link)
    }

    /// `RBE_LEFT`.
    fn left(self) -> Option<Self> {
        self.child(Side::Left)
    }

    /// `RBE_RIGHT`.
    fn right(self) -> Option<Self> {
        self.child(Side::Right)
    }

    /// `RBE_PARENT`.
    fn parent(self) -> Option<Self> {
        self.links().rbt_parent.get().map(Self::from_link)
    }

    /// Makes `child` the child on `side`.
    fn set_child(self, side: Side, child: Option<RbNode<'_, T>>) {
        let rbe = self.entry();
        match side {
            Side::Left => rbe.rbt_left.set(link(child)),
            Side::Right => rbe.rbt_right.set(link(child)),
        }
    }

    /// Makes `parent` the parent.
    fn set_parent(self, parent: Option<RbNode<'_, T>>) {
        self.entry().rbt_parent.set(link(parent));
    }

    /// `RBE_COLOR`.
    fn color(self) -> Color {
        self.entry().rbt_color.get()
    }

    /// Sets `RBE_COLOR`.
    fn set_color(self, color: Color) {
        self.entry().rbt_color.set(color);
    }

    /// The side of `self` that `child` hangs on (the C's `child == RBE_LEFT(self)` test).
    fn side_of(self, child: RbNode<'_, T>) -> Side {
        if same(self.left(), Some(child)) {
            Side::Left
        } else {
            Side::Right
        }
    }

    /// `*rbe = *old`: takes `old`'s links and colour, and so its place among its neighbours
    /// once they are pointed here.
    fn take_place_of(self, old: Self) {
        let (rbe, old) = (self.entry(), old.links());
        rbe.rbt_parent.set(old.rbt_parent.get());
        rbe.rbt_left.set(old.rbt_left.get());
        rbe.rbt_right.set(old.rbt_right.get());
        rbe.rbt_color.set(old.rbt_color.get());
    }

    /// Leaves the entry as `RbtEntry::new` makes it: no links, black, not poisoned.
    fn clear(self) {
        let rbe = self.entry();
        rbe.rbt_parent.set(None);
        rbe.rbt_left.set(None);
        rbe.rbt_right.set(None);
        rbe.rbt_color.set(Color::Black);
        rbe.rbt_poisoned.set(false);
    }
}

/// `struct rb_type`: how a red-black tree reaches its elements. Implemented by
/// `sys::tree::RbtInfo<A>` and `sys::tree::RbInfo<A>` for an adapter `A`.
///
/// # Safety
///
/// `OFFSET` must be the offset of an [`RbtEntry`] inside every `Elem`; the tree reads and
/// writes that entry through it, and steps back from it to the element.
pub unsafe trait RbType {
    /// The element type the tree holds.
    type Elem;
    /// `t_offset`: offset of the `RbtEntry` inside `Elem`.
    const OFFSET: usize;
    /// Whether `t_augment` is set.
    const AUGMENTED: bool;
    /// `t_compare`.
    fn compare(a: &Self::Elem, b: &Self::Elem) -> Ordering;
    /// `t_augment`: recomputes an element's cached subtree data after its subtree changed.
    fn augment(elem: &Self::Elem);
}

/// The link that names `node` (`None` for none).
fn link<T: RbType>(node: Option<RbNode<'_, T>>) -> Option<NonNull<RbtEntry>> {
    node.map(|node| node.ptr)
}

/// Whether two possibly absent nodes are the same one (both absent included), the C's pointer
/// comparison.
fn same<T: RbType>(a: Option<RbNode<'_, T>>, b: Option<RbNode<'_, T>>) -> bool {
    link(a) == link(b)
}

/// Whether `node` is present and red; an absent node is a black leaf.
fn is_red<T: RbType>(node: Option<RbNode<'_, T>>) -> bool {
    node.is_some_and(|node| node.color() == Color::Red)
}

/// A poisoned entry's links were about to be followed.
fn rb_poisoned(rbe: &RbtEntry) -> ! {
    panic(format_args!(
        "rb tree: links of the poisoned entry {:p} followed",
        ptr::from_ref(rbe)
    ))
}

/// The tree lacks a structure the algorithm relies on: it was relinked by hand inconsistently,
/// or an element was used against its contract.
fn rb_corrupt(what: &str) -> ! {
    panic(format_args!("rb tree corrupt: {what}"))
}

/// `rb_n2e`: the entry inside `node`, as a handle.
fn rb_n2e<T: RbType>(node: &T::Elem) -> RbNode<'_, T> {
    // The address arithmetic keeps the element's provenance, so `rb_e2n` may step back.
    let ptr = NonNull::from(node)
        .cast::<u8>()
        .map_addr(|addr| addr.saturating_add(T::OFFSET))
        .cast::<RbtEntry>();
    RbNode::from_link(ptr)
}

/// `rb_e2n`: the element around the entry `rbe`.
fn rb_e2n<'a, T: RbType>(rbe: RbNode<'a, T>) -> &'a T::Elem {
    let elem = rbe
        .ptr
        .as_ptr()
        .cast::<u8>()
        .wrapping_sub(T::OFFSET)
        .cast::<T::Elem>();
    // SAFETY: the handle names the live entry of a `T::Elem` (see `RbNode::entry`), `T::OFFSET`
    // bytes into it by `RbType`'s contract, and its pointer carries that element's provenance
    // (`rb_n2e` derived it from a reference to the whole element), so stepping back gives the
    // live, aligned element. It is only lent shared, as the tree's callers hold it.
    unsafe { &*elem }
}

/// `RBH_ROOT`, as a handle.
fn rbh_root<T: RbType>(rbt: &RbTree<T>) -> Option<RbNode<'_, T>> {
    rbt.rbt_root.get().map(RbNode::from_link)
}

/// Makes `node` the root.
fn rbh_set_root<T: RbType>(rbt: &RbTree<T>, node: Option<RbNode<'_, T>>) {
    rbt.rbt_root.set(link(node));
}

/// Points `parent`'s link to `old` at `new` instead, or the root when `old` has no parent.
fn rbe_replace_child<T: RbType>(
    rbt: &RbTree<T>,
    parent: Option<RbNode<'_, T>>,
    old: RbNode<'_, T>,
    new: Option<RbNode<'_, T>>,
) {
    match parent {
        Some(parent) => parent.set_child(parent.side_of(old), new),
        None => rbh_set_root(rbt, new),
    }
}

/// `rbe_set`: makes `rbe` a red leaf under `parent`; this also lifts a poison.
fn rbe_set<T: RbType>(rbe: RbNode<'_, T>, parent: Option<RbNode<'_, T>>) {
    rbe.clear();
    rbe.set_parent(parent);
    rbe.set_color(Color::Red);
}

/// `rbe_set_blackred`.
fn rbe_set_blackred<T: RbType>(black: RbNode<'_, T>, red: RbNode<'_, T>) {
    black.set_color(Color::Black);
    red.set_color(Color::Red);
}

/// `rbe_augment`: runs the augment hook on the element of `rbe`.
fn rbe_augment<T: RbType>(rbe: RbNode<'_, T>) {
    T::augment(rb_e2n(rbe));
}

/// `rbe_if_augment`: `rbe_augment` when the type has an augment hook.
fn rbe_if_augment<T: RbType>(rbe: RbNode<'_, T>) {
    if T::AUGMENTED {
        rbe_augment(rbe);
    }
}

/// `rbe_rotate_left` (`side` = `Left`) and `rbe_rotate_right` (`Right`): `rbe` goes down to
/// `side`, under its child from the other side, which takes its place.
fn rbe_rotate<'a, T: RbType>(rbt: &'a RbTree<T>, rbe: RbNode<'a, T>, side: Side) {
    let up = side.opposite();
    let Some(tmp) = rbe.child(up) else {
        rb_corrupt("rotation without the child to lift");
    };
    let inner = tmp.child(side);
    rbe.set_child(up, inner);
    if let Some(inner) = inner {
        inner.set_parent(Some(rbe));
    }

    let parent = rbe.parent();
    tmp.set_parent(parent);
    rbe_replace_child(rbt, parent, rbe, Some(tmp));

    tmp.set_child(side, Some(rbe));
    rbe.set_parent(Some(tmp));

    if T::AUGMENTED {
        rbe_augment(rbe);
        rbe_augment(tmp);
        if let Some(parent) = tmp.parent() {
            rbe_augment(parent);
        }
    }
}

/// `rbe_insert_color`: restores the red-black invariants after `rbe` was inserted as a red leaf.
fn rbe_insert_color<'a, T: RbType>(rbt: &'a RbTree<T>, mut rbe: RbNode<'a, T>) {
    while let Some(mut parent) = rbe.parent().filter(|parent| parent.color() == Color::Red) {
        // The root is black, so a red parent has a parent of its own.
        let Some(gparent) = parent.parent() else {
            rb_corrupt("red root");
        };
        let side = gparent.side_of(parent);

        if let Some(uncle) = gparent
            .child(side.opposite())
            .filter(|uncle| uncle.color() == Color::Red)
        {
            uncle.set_color(Color::Black);
            rbe_set_blackred(parent, gparent);
            rbe = gparent;
            continue;
        }

        if same(parent.child(side.opposite()), Some(rbe)) {
            rbe_rotate(rbt, parent, side);
            core::mem::swap(&mut parent, &mut rbe);
        }

        rbe_set_blackred(parent, gparent);
        rbe_rotate(rbt, gparent, side.opposite());
    }

    match rbh_root(rbt) {
        Some(root) => root.set_color(Color::Black),
        None => rb_corrupt("empty after an insert"),
    }
}

/// `rbe_remove_color`: restores the red-black invariants after a black entry was unlinked,
/// `rbe` (possibly absent) having taken its place under `parent`.
fn rbe_remove_color<'a, T: RbType>(
    rbt: &'a RbTree<T>,
    mut parent: Option<RbNode<'a, T>>,
    mut rbe: Option<RbNode<'a, T>>,
) {
    while !is_red(rbe) && !same(rbe, rbh_root(rbt)) {
        // Below the root, `rbe` has a parent, and, being black (or a black leaf), a sibling.
        let Some(p) = parent else {
            rb_corrupt("non-root without a parent");
        };
        let side = if same(p.left(), rbe) {
            Side::Left
        } else {
            Side::Right
        };
        let far = side.opposite();
        let Some(mut tmp) = p.child(far) else {
            rb_corrupt("black node without a sibling");
        };

        if tmp.color() == Color::Red {
            rbe_set_blackred(tmp, p);
            rbe_rotate(rbt, p, side);
            let Some(sibling) = p.child(far) else {
                rb_corrupt("black node without a sibling");
            };
            tmp = sibling;
        }

        if !is_red(tmp.child(side)) && !is_red(tmp.child(far)) {
            tmp.set_color(Color::Red);
            rbe = Some(p);
            parent = p.parent();
        } else {
            if !is_red(tmp.child(far)) {
                if let Some(near) = tmp.child(side) {
                    near.set_color(Color::Black);
                }

                tmp.set_color(Color::Red);
                rbe_rotate(rbt, tmp, far);
                let Some(sibling) = p.child(far) else {
                    rb_corrupt("black node without a sibling");
                };
                tmp = sibling;
            }

            tmp.set_color(p.color());
            p.set_color(Color::Black);
            if let Some(far_child) = tmp.child(far) {
                far_child.set_color(Color::Black);
            }

            rbe_rotate(rbt, p, side);
            rbe = rbh_root(rbt);
            break;
        }
    }

    if let Some(rbe) = rbe {
        rbe.set_color(Color::Black);
    }
}

/// `rbe_remove`: unlinks `old` from `rbt`, leaves it holding no links and returns it.
fn rbe_remove<'a, T: RbType>(rbt: &'a RbTree<T>, old: RbNode<'a, T>) -> RbNode<'a, T> {
    let (parent, child, color) = match (old.left(), old.right()) {
        (Some(left), Some(right)) => {
            // Two children: the in-order successor, the leftmost node on the right, which has
            // no left child, leaves its place to its right child and takes `old`'s.
            let mut rbe = right;
            while let Some(tmp) = rbe.left() {
                rbe = tmp;
            }

            let child = rbe.right();
            let mut parent = rbe.parent();
            let color = rbe.color();
            if let Some(child) = child {
                child.set_parent(parent);
            }
            rbe_replace_child(rbt, parent, rbe, child);
            if let Some(parent) = parent {
                rbe_if_augment(parent);
            }
            if same(rbe.parent(), Some(old)) {
                parent = Some(rbe);
            }
            rbe.take_place_of(old);

            let tmp = old.parent();
            rbe_replace_child(rbt, tmp, old, Some(rbe));
            if let Some(tmp) = tmp {
                rbe_if_augment(tmp);
            }

            left.set_parent(Some(rbe));
            if let Some(old_right) = old.right() {
                old_right.set_parent(Some(rbe));
            }

            if T::AUGMENTED {
                let mut tmp = parent;
                while let Some(node) = tmp {
                    rbe_augment(node);
                    tmp = node.parent();
                }
            }
            (parent, child, color)
        }
        (left, right) => {
            let child = left.or(right);
            let parent = old.parent();
            let color = old.color();

            if let Some(child) = child {
                child.set_parent(parent);
            }
            rbe_replace_child(rbt, parent, old, child);
            if let Some(parent) = parent {
                rbe_if_augment(parent);
            }
            (parent, child, color)
        }
    };

    if color == Color::Black {
        rbe_remove_color(rbt, parent, child);
    }
    old.clear();
    old
}

/// `_rb_remove`: unlinks `elm` from `rbt` and returns it; `elm` is left holding no links.
///
/// # Safety
///
/// `elm` is in `rbt`. Once removed, no link of the tree points at it any more, so it may be
/// freed or moved.
pub unsafe fn _rb_remove<'a, T: RbType>(rbt: &RbTree<T>, elm: &'a T::Elem) -> &'a T::Elem {
    rbe_remove(rbt, rb_n2e::<T>(elm));
    elm
}

/// `_rb_insert`: links `elm` into `rbt`; returns the element already there with an equal key
/// instead, leaving the tree (and `elm`) unchanged.
///
/// # Safety
///
/// `elm` is in no tree of this type (its entry is unlinked, or poisoned), and stays valid and in
/// place until it is removed (or, if the tree is abandoned by `RbTree::init`, for as long as an
/// element of the abandoned tree is still read through this module): the tree's links point at
/// it.
pub unsafe fn _rb_insert<'a, T: RbType>(rbt: &'a RbTree<T>, elm: &T::Elem) -> Option<&'a T::Elem> {
    let mut tmp = rbh_root(rbt);
    let mut parent = None;
    let mut side = Side::Left;

    while let Some(node) = tmp {
        parent = Some(node);
        let elem = rb_e2n(node);
        match T::compare(elm, elem) {
            Ordering::Less => side = Side::Left,
            Ordering::Greater => side = Side::Right,
            Ordering::Equal => return Some(elem),
        }
        tmp = node.child(side);
    }

    let rbe = rb_n2e::<T>(elm);
    rbe_set(rbe, parent);

    match parent {
        Some(parent) => {
            parent.set_child(side, Some(rbe));
            rbe_if_augment(parent);
        }
        None => rbh_set_root(rbt, Some(rbe)),
    }

    rbe_insert_color(rbt, rbe);
    None
}

/// `_rb_find`: the element whose key equals `key`'s.
pub fn _rb_find<'a, T: RbType>(rbt: &'a RbTree<T>, key: &T::Elem) -> Option<&'a T::Elem> {
    let mut tmp = rbh_root(rbt);
    while let Some(node) = tmp {
        let elem = rb_e2n(node);
        tmp = match T::compare(key, elem) {
            Ordering::Less => node.left(),
            Ordering::Greater => node.right(),
            Ordering::Equal => return Some(elem),
        };
    }
    None
}

/// `_rb_nfind`: the first element whose key is greater than or equal to `key`'s.
pub fn _rb_nfind<'a, T: RbType>(rbt: &'a RbTree<T>, key: &T::Elem) -> Option<&'a T::Elem> {
    let mut tmp = rbh_root(rbt);
    let mut res = None;
    while let Some(node) = tmp {
        let elem = rb_e2n(node);
        tmp = match T::compare(key, elem) {
            Ordering::Less => {
                res = Some(elem);
                node.left()
            }
            Ordering::Greater => node.right(),
            Ordering::Equal => return Some(elem),
        };
    }
    res
}

/// The in-order neighbour of `rbe` towards `side` (`Right`: `_rb_next`, `Left`: `_rb_prev`):
/// the extreme of its subtree on that side, or else the first ancestor reached from the other
/// side.
fn rb_step<T: RbType>(rbe: RbNode<'_, T>, side: Side) -> Option<RbNode<'_, T>> {
    if let Some(mut node) = rbe.child(side) {
        while let Some(child) = node.child(side.opposite()) {
            node = child;
        }
        return Some(node);
    }

    let mut rbe = rbe;
    loop {
        let parent = rbe.parent()?;
        if !same(parent.child(side), Some(rbe)) {
            return Some(parent);
        }
        rbe = parent;
    }
}

/// `_rb_next`: the in-order successor of `elm`; `None` for an element in no tree.
pub fn _rb_next<T: RbType>(elm: &T::Elem) -> Option<&T::Elem> {
    rb_step(rb_n2e::<T>(elm), Side::Right).map(rb_e2n)
}

/// `_rb_prev`: the in-order predecessor of `elm`; `None` for an element in no tree.
pub fn _rb_prev<T: RbType>(elm: &T::Elem) -> Option<&T::Elem> {
    rb_step(rb_n2e::<T>(elm), Side::Left).map(rb_e2n)
}

/// `_rb_root`.
pub fn _rb_root<T: RbType>(rbt: &RbTree<T>) -> Option<&T::Elem> {
    rbh_root(rbt).map(rb_e2n)
}

/// The extreme node of the tree on `side`: `_rb_min` (`Left`), `_rb_max` (`Right`).
fn rb_extreme<T: RbType>(rbt: &RbTree<T>, side: Side) -> Option<&T::Elem> {
    let mut rbe = rbh_root(rbt)?;
    while let Some(child) = rbe.child(side) {
        rbe = child;
    }
    Some(rb_e2n(rbe))
}

/// `_rb_min`: the element with the smallest key.
pub fn _rb_min<T: RbType>(rbt: &RbTree<T>) -> Option<&T::Elem> {
    rb_extreme(rbt, Side::Left)
}

/// `_rb_max`: the element with the largest key.
pub fn _rb_max<T: RbType>(rbt: &RbTree<T>) -> Option<&T::Elem> {
    rb_extreme(rbt, Side::Right)
}

/// `_rb_left`: the left child of `node`.
pub fn _rb_left<T: RbType>(node: &T::Elem) -> Option<&T::Elem> {
    rb_n2e::<T>(node).left().map(rb_e2n)
}

/// `_rb_right`: the right child of `node`.
pub fn _rb_right<T: RbType>(node: &T::Elem) -> Option<&T::Elem> {
    rb_n2e::<T>(node).right().map(rb_e2n)
}

/// `_rb_parent`: the parent of `node`.
pub fn _rb_parent<T: RbType>(node: &T::Elem) -> Option<&T::Elem> {
    rb_n2e::<T>(node).parent().map(rb_e2n)
}

/// The link that names `node`'s entry, `None` for none.
fn entry_or_null<T: RbType>(node: Option<&T::Elem>) -> Option<NonNull<RbtEntry>> {
    link(node.map(rb_n2e::<T>))
}

/// `_rb_set_left`: makes `left` the left child of `node` without rebalancing.
///
/// # Safety
///
/// The caller is rebuilding the tree by hand and keeps it consistent: `left` (and every element
/// it links) stays valid and in place while linked, as for `_rb_insert`, and the links of the
/// entries it touches end up naming each other as a tree of this type.
pub unsafe fn _rb_set_left<T: RbType>(node: &T::Elem, left: Option<&T::Elem>) {
    rb_n2e::<T>(node)
        .entry()
        .rbt_left
        .set(entry_or_null::<T>(left));
}

/// `_rb_set_right`: makes `right` the right child of `node` without rebalancing.
///
/// # Safety
///
/// As for [`_rb_set_left`].
pub unsafe fn _rb_set_right<T: RbType>(node: &T::Elem, right: Option<&T::Elem>) {
    rb_n2e::<T>(node)
        .entry()
        .rbt_right
        .set(entry_or_null::<T>(right));
}

/// `_rb_set_parent`: makes `parent` the parent of `node` without rebalancing.
///
/// # Safety
///
/// As for [`_rb_set_left`].
pub unsafe fn _rb_set_parent<T: RbType>(node: &T::Elem, parent: Option<&T::Elem>) {
    rb_n2e::<T>(node)
        .entry()
        .rbt_parent
        .set(entry_or_null::<T>(parent));
}

/// `_rb_poison`: fills the links of `node`, which has been removed, with `poison`, and marks
/// it poisoned, so a stale use panics instead of walking a tree.
pub fn _rb_poison<T: RbType>(node: &T::Elem, poison: usize) {
    let rbe = rb_n2e::<T>(node).entry();
    let value = NonNull::new(ptr::without_provenance_mut::<RbtEntry>(poison));
    rbe.rbt_poisoned.set(true);
    rbe.rbt_parent.set(value);
    rbe.rbt_left.set(value);
    rbe.rbt_right.set(value);
}

/// `_rb_check`: whether every link of `node` holds `poison`.
pub fn _rb_check<T: RbType>(node: &T::Elem, poison: usize) -> bool {
    let rbe = rb_n2e::<T>(node).entry();
    let holds = |link: &Link<RbtEntry>| link.get().map_or(0, |p| p.addr().get()) == poison;
    holds(&rbe.rbt_parent) && holds(&rbe.rbt_left) && holds(&rbe.rbt_right)
}
/* </CODE> */

/* <TESTS> */
#[cfg(test)]
mod tests {
    // Tests of `subr_tree.rs` through the `RBT_*` API of `sys::tree`; see there.

    use core::cell::Cell;
    use core::cmp::Ordering;
    use core::ptr;
    use std::vec::Vec;

    use crate::sys::tree::{RB_BLACK, RB_RED, RbtAdapter, RbtEntry, RbtHead};
    use crate::tree_adapter;

    struct Node {
        key: i32,
        /// Nodes in the subtree rooted here, maintained by the augment hook of `Aug`.
        size: Cell<usize>,
        rbt: RbtEntry,
    }

    impl Node {
        const fn new(key: i32) -> Self {
            Self {
                key,
                // a node not yet in a tree is a subtree of one
                size: Cell::new(1),
                rbt: RbtEntry::new(),
            }
        }
    }

    fn by_key(a: &Node, b: &Node) -> Ordering {
        a.key.cmp(&b.key)
    }

    fn subtree_size(n: &Node) {
        let size = |c: Option<&Node>| c.map_or(0, |c| c.size.get());
        n.size
            .set(1 + size(RbtHead::<Aug>::left(n)) + size(RbtHead::<Aug>::right(n)));
    }

    tree_adapter!(Plain: Node, rbt => RbtEntry, by_key);
    tree_adapter!(Aug: Node, rbt => RbtEntry, by_key, augment = subtree_size);

    /// What a user of the augment hook does after an insert or a remove (`uvm_map_addr_augment`):
    /// the tree augments the parent and whatever it rotated, the caller walks the rest of the way
    /// up.
    fn augment_up(mut node: Option<&Node>) {
        while let Some(n) = node {
            subtree_size(n);
            node = RbtHead::<Aug>::parent(n);
        }
    }

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

    /// Checks the red-black invariants and returns the black height; with `sizes`, also that every
    /// node's `size` is its subtree's node count.
    fn check<A: RbtAdapter<Elem = Node>>(t: &RbtHead<A>, sizes: bool) -> usize {
        fn walk<A: RbtAdapter<Elem = Node>>(
            node: Option<&Node>,
            parent_red: bool,
            sizes: bool,
        ) -> (usize, usize) {
            let Some(n) = node else { return (1, 0) };
            let red = A::entry(n).color() == RB_RED;
            assert!(
                !(red && parent_red),
                "red node {} under a red parent",
                n.key
            );
            let (l, r) = (RbtHead::<A>::left(n), RbtHead::<A>::right(n));
            if let Some(l) = l {
                assert!(l.key < n.key);
                assert!(ptr::eq(RbtHead::<A>::parent(l).unwrap(), n));
            }
            if let Some(r) = r {
                assert!(r.key > n.key);
                assert!(ptr::eq(RbtHead::<A>::parent(r).unwrap(), n));
            }
            let ((lh, lc), (rh, rc)) = (walk::<A>(l, red, sizes), walk::<A>(r, red, sizes));
            assert_eq!(lh, rh, "black height differs under {}", n.key);
            let count = 1 + lc + rc;
            if sizes {
                assert_eq!(n.size.get(), count, "size of {}", n.key);
            }
            (lh + usize::from(!red), count)
        }
        if let Some(root) = t.root() {
            assert_eq!(A::entry(root).color(), RB_BLACK);
            assert!(RbtHead::<A>::parent(root).is_none());
        }
        walk::<A>(t.root(), false, sizes).0
    }

    #[test]
    fn insert_find_iterate() {
        let n = nodes();
        let t = RbtHead::<Plain>::new();
        assert!(t.is_empty() && t.root().is_none() && t.min().is_none());
        // SAFETY: the nodes outlive the tree and start unlinked.
        unsafe {
            for node in &n {
                assert!(t.insert(node).is_none());
                check(&t, false);
            }
        }
        assert_eq!(keys(t.iter()), sorted());
        let mut rev = sorted();
        rev.reverse();
        assert_eq!(keys(t.iter_reverse()), rev);
        assert_eq!(key(t.min()), Some(1));
        assert_eq!(key(t.max()), Some(99));
        assert_eq!(key(t.find(&Node::new(25))), Some(25));
        assert!(t.find(&Node::new(26)).is_none());
        assert_eq!(key(t.nfind(&Node::new(26))), Some(30));
        assert_eq!(key(t.nfind(&Node::new(1))), Some(1));
        assert!(t.nfind(&Node::new(100)).is_none());

        let mut chain = Vec::new();
        let mut cur = t.min();
        while let Some(c) = cur {
            chain.push(c.key);
            cur = RbtHead::<Plain>::next(c);
        }
        assert_eq!(chain, sorted());
        chain.clear();
        cur = t.max();
        while let Some(c) = cur {
            chain.push(c.key);
            cur = RbtHead::<Plain>::prev(c);
        }
        assert_eq!(chain, rev);

        let dup = Node::new(10);
        // SAFETY: `dup` is unlinked; the tree refuses it.
        assert!(ptr::eq(unsafe { t.insert(&dup) }.unwrap(), &n[3]));
    }

    #[test]
    fn remove_keeps_invariants() {
        let n = nodes();
        let t = RbtHead::<Plain>::new();
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
                check(&t, false);
            }
        }
        assert!(t.is_empty());
        t.init();
        assert!(t.is_empty());
    }

    #[test]
    fn remove_while_iterating_in_reverse() {
        let n = nodes();
        let t = RbtHead::<Plain>::new();
        // SAFETY: the nodes outlive the tree and start unlinked.
        unsafe {
            for node in &n {
                t.insert(node);
            }
        }
        for node in t.iter_reverse() {
            if node.key > 60 {
                // SAFETY: `node` is in the tree; the iterator already read its predecessor.
                unsafe { t.remove(node) };
            }
        }
        let expect: Vec<i32> = sorted().into_iter().filter(|&k| k <= 60).collect();
        assert_eq!(keys(t.iter()), expect);
        check(&t, false);
    }

    #[test]
    fn augment_tracks_subtree_sizes() {
        let n = nodes();
        let t = RbtHead::<Aug>::new();
        // SAFETY: the nodes outlive the tree; every removed node is linked.
        unsafe {
            for (i, node) in n.iter().enumerate() {
                t.insert(node);
                augment_up(Some(node));
                check(&t, true);
                assert_eq!(t.root().unwrap().size.get(), i + 1);
            }
            let mut left = n.len();
            for &k in &[50, 1, 99, 30, 65, 20, 70, 10] {
                let node = n.iter().find(|x| x.key == k).unwrap();
                let parent = RbtHead::<Aug>::parent(node);
                t.remove(node);
                augment_up(parent);
                left -= 1;
                check(&t, true);
                assert_eq!(t.root().unwrap().size.get(), left);
            }
        }
    }

    #[test]
    fn poison_and_check() {
        let n = Node::new(1);
        RbtHead::<Plain>::poison(&n, 0xdead_beef);
        assert!(RbtHead::<Plain>::check(&n, 0xdead_beef));
        assert!(!RbtHead::<Plain>::check(&n, 0xdead_beee));
        let t = RbtHead::<Plain>::new();
        // SAFETY: `n` outlives the tree and is unlinked (poison is not a link).
        unsafe { t.insert(&n) };
        assert!(!RbtHead::<Plain>::check(&n, 0xdead_beef));
    }

    #[test]
    fn set_links_by_hand() {
        let n = nodes();
        let t = RbtHead::<Plain>::new();
        // SAFETY: the nodes outlive the tree and start unlinked.
        unsafe {
            for node in &n[..3] {
                t.insert(node);
            }
        }
        let root = t.root().unwrap();
        let left = RbtHead::<Plain>::left(root).unwrap();
        // SAFETY: detaching and re-attaching the same child leaves the tree as it was.
        unsafe {
            RbtHead::<Plain>::set_left(root, None);
            assert!(RbtHead::<Plain>::left(root).is_none());
            RbtHead::<Plain>::set_parent(left, None);
            assert!(RbtHead::<Plain>::parent(left).is_none());
            RbtHead::<Plain>::set_left(root, Some(left));
            RbtHead::<Plain>::set_parent(left, Some(root));
            RbtHead::<Plain>::set_right(root, RbtHead::<Plain>::right(root));
        }
        assert_eq!(keys(t.iter()), [20, 50, 70]);
        check(&t, false);
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

    /// Whether `n`'s entry is as `RbtEntry::new` makes it: no links, black.
    fn unlinked(n: &Node) -> bool {
        RbtHead::<Aug>::left(n).is_none()
            && RbtHead::<Aug>::right(n).is_none()
            && RbtHead::<Aug>::parent(n).is_none()
            && n.rbt.color() == RB_BLACK
    }

    #[test]
    fn random_sequences_keep_invariants() {
        const N: usize = 96;
        let key_of = |i: usize| i32::try_from(i * 2).unwrap();
        let n: Vec<Node> = (0..N).map(|i| Node::new(key_of(i))).collect();
        for seed in 1..=8u64 {
            let mut rng = Rng(seed.wrapping_mul(0x9e37_79b9_7f4a_7c15));
            let t = RbtHead::<Aug>::new();
            let mut linked = [false; N];
            for step in 0..1500 {
                let i = rng.below(N);
                // SAFETY: the nodes outlive the tree; `linked` says which are in it.
                unsafe {
                    if linked[i] {
                        let parent = RbtHead::<Aug>::parent(&n[i]);
                        assert!(ptr::eq(t.remove(&n[i]), &n[i]));
                        assert!(unlinked(&n[i]), "seed {seed} step {step}: links left");
                        augment_up(parent);
                    } else {
                        assert!(t.insert(&n[i]).is_none());
                        augment_up(Some(&n[i]));
                    }
                }
                linked[i] = !linked[i];

                // colours, order, parent links, black height and the augmented sizes
                let model: Vec<i32> = (0..N).filter(|&k| linked[k]).map(key_of).collect();
                check(&t, true);
                assert_eq!(keys(t.iter()), model);
                let mut rev = model.clone();
                rev.reverse();
                assert_eq!(keys(t.iter_reverse()), rev);
                assert_eq!(t.root().map_or(0, |r| r.size.get()), model.len());

                // find and nfind against the model, on keys present, absent and out of range
                let probe = i32::try_from(rng.below(2 * N + 2)).unwrap() - 1;
                let want = model.iter().copied().find(|&k| k >= probe);
                assert_eq!(key(t.nfind(&Node::new(probe))), want);
                assert_eq!(key(t.find(&Node::new(probe))), want.filter(|&k| k == probe));
                assert_eq!(key(t.min()), model.first().copied());
                assert_eq!(key(t.max()), model.last().copied());
            }
        }
    }

    #[test]
    fn random_removal_while_iterating() {
        const N: usize = 80;
        let n: Vec<Node> = (0..N)
            .map(|i| Node::new(i32::try_from(i).unwrap()))
            .collect();
        for seed in 1..=8u64 {
            let mut rng = Rng(seed);
            let t = RbtHead::<Plain>::new();
            // SAFETY: the nodes outlive the tree and start unlinked.
            unsafe {
                for node in &n {
                    t.insert(node);
                }
            }
            let mut kept = Vec::new();
            let mut visit = |node: &Node| {
                if rng.below(3) == 0 {
                    kept.push(node.key);
                } else {
                    // SAFETY: `node` is in the tree; the iterator already read its neighbour.
                    unsafe { t.remove(node) };
                }
            };
            if seed % 2 == 0 {
                t.iter().for_each(&mut visit);
            } else {
                t.iter_reverse().for_each(&mut visit);
                kept.reverse();
            }
            assert_eq!(keys(t.iter()), kept);
            check(&t, false);
        }
    }

    #[test]
    fn removed_entries_read_as_new() {
        let n = nodes();
        let t = RbtHead::<Plain>::new();
        // SAFETY: the nodes outlive the tree; every removed node is linked.
        unsafe {
            for node in &n {
                t.insert(node);
            }
            // a node with two children, the root, a leaf
            let root = t.root().unwrap();
            for node in [&n[1], root, &n[13]] {
                t.remove(node);
                assert!(RbtHead::<Plain>::left(node).is_none());
                assert!(RbtHead::<Plain>::right(node).is_none());
                assert!(RbtHead::<Plain>::parent(node).is_none());
                assert!(RbtHead::<Plain>::next(node).is_none());
                assert!(RbtHead::<Plain>::prev(node).is_none());
                assert_eq!(node.rbt.color(), RB_BLACK);
                check(&t, false);
            }
            // and they can go back in
            for node in [&n[1], &n[0], &n[13]] {
                assert!(t.insert(node).is_none());
            }
        }
        assert_eq!(keys(t.iter()), sorted());
        check(&t, false);
    }

    #[test]
    fn poison_after_remove_then_reinsert() {
        // `uvm_mapent_addr_remove` poisons what it removes, `uvm_mapent_alloc` what it
        // allocates; `uvm_mapent_addr_insert` checks the poison before inserting.
        const DEADBEEF: usize = 0xdead_beef;
        let n = nodes();
        let t = RbtHead::<Plain>::new();
        // SAFETY: the nodes outlive the tree; every removed node is linked.
        unsafe {
            for node in &n {
                RbtHead::<Plain>::poison(node, DEADBEEF);
                assert!(RbtHead::<Plain>::check(node, DEADBEEF));
                t.insert(node);
                assert!(!RbtHead::<Plain>::check(node, DEADBEEF));
            }
            check(&t, false);
            for node in &n[..7] {
                t.remove(node);
                RbtHead::<Plain>::poison(node, DEADBEEF);
                assert!(RbtHead::<Plain>::check(node, DEADBEEF));
                check(&t, false);
            }
            assert_eq!(keys(t.iter()).len(), n.len() - 7);
            for node in &n[..7] {
                assert!(RbtHead::<Plain>::check(node, DEADBEEF));
                assert!(t.insert(node).is_none());
            }
        }
        assert_eq!(keys(t.iter()), sorted());
        check(&t, false);
        // a zero poison is a null link
        let lone = Node::new(7);
        RbtHead::<Plain>::poison(&lone, 0);
        assert!(RbtHead::<Plain>::check(&lone, 0));
    }
}
/* </TESTS> */
