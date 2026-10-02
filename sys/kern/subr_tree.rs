/*	$OpenBSD: subr_tree.c,v 1.10 2018/10/09 08:28:43 dlg Exp $ */

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

//! The red-black tree behind `RBT_*` (and, here, `RB_*`): `kern/subr_tree.c`.
//!
//! Upstream: sys/kern/subr_tree.c @ 3ce1f3f79392
//!
//! The algorithm works on [`RbtEntry`] links (entry to entry, never element to element) and
//! reaches the element only to compare keys or to run the augment hook, through an [`RbType`]:
//! the C `struct rb_type` with its comparator, optional augment and the entry's offset inside
//! the element. `sys/sys/tree.rs` provides one `RbType` per adapter and the typed wrappers.
//!
//! ## Deviations
//! - `struct rb_type` is a trait with associated items instead of a struct of function
//!   pointers; `t_augment == NULL` is the `AUGMENTED` constant.
//! - The comparator returns `Ordering` instead of a negative, zero or positive `int`.
//! - Null results are `None`; the `_rb_*` entry points take and return references, and
//!   those that relink the tree are `unsafe` with the C precondition as their contract.
//! - The classic `RB_*` family of `tree.h`, a macro copy of this algorithm in C, is served by
//!   this one implementation too.

use core::cmp::Ordering;
use core::ptr;

use crate::sys::tree::{RB_BLACK, RB_RED, RbTree, RbtEntry};

/// `struct rb_type`: how a red-black tree reaches its elements. Implemented by
/// `sys::tree::RbtInfo<A>` and `sys::tree::RbInfo<A>` for an adapter `A`.
///
/// # Safety
///
/// `OFFSET` must be the offset of an [`RbtEntry`] inside every `Elem`; the tree reads and
/// writes that entry through it.
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

/// `rb_n2e`: the entry inside `node`.
fn rb_n2e<T: RbType>(node: &T::Elem) -> *const RbtEntry {
    let base: *const T::Elem = node;
    // SAFETY: `RbType` guarantees an `RbtEntry` lives at `OFFSET` inside every element, so the
    // offset pointer stays inside `node`.
    unsafe { base.cast::<u8>().add(T::OFFSET).cast::<RbtEntry>() }
}

/// `rb_e2n`: the element around the entry `rbe`.
///
/// # Safety
///
/// `rbe` is the entry of a live `T::Elem`.
unsafe fn rb_e2n<T: RbType>(rbe: *const RbtEntry) -> *const T::Elem {
    // SAFETY: the caller guarantees `rbe` sits at `OFFSET` inside an element, so stepping back
    // stays inside it.
    unsafe { rbe.cast::<u8>().sub(T::OFFSET).cast::<T::Elem>() }
}

/// A linked entry as a reference, for the field accesses below.
///
/// # Safety
///
/// `rbe` is non-null and points at a live entry.
#[inline]
unsafe fn e<'a>(rbe: *const RbtEntry) -> &'a RbtEntry {
    // SAFETY: forwarded.
    unsafe { &*rbe }
}

/// `rbe_set`: makes `rbe` a red leaf under `parent`.
fn rbe_set(rbe: &RbtEntry, parent: *const RbtEntry) {
    rbe.rbt_parent.set(parent);
    rbe.rbt_left.set(ptr::null());
    rbe.rbt_right.set(ptr::null());
    rbe.rbt_color.set(RB_RED);
}

/// `rbe_set_blackred`.
fn rbe_set_blackred(black: &RbtEntry, red: &RbtEntry) {
    black.rbt_color.set(RB_BLACK);
    red.rbt_color.set(RB_RED);
}

/// `rbe_augment`: runs the augment hook on the element of `rbe`.
///
/// # Safety
///
/// `rbe` is the entry of a live `T::Elem`.
unsafe fn rbe_augment<T: RbType>(rbe: *const RbtEntry) {
    // SAFETY: forwarded.
    T::augment(unsafe { &*rb_e2n::<T>(rbe) });
}

/// `rbe_if_augment`: [`rbe_augment`] when the type has an augment hook.
///
/// # Safety
///
/// As for [`rbe_augment`].
unsafe fn rbe_if_augment<T: RbType>(rbe: *const RbtEntry) {
    if T::AUGMENTED {
        // SAFETY: forwarded.
        unsafe { rbe_augment::<T>(rbe) };
    }
}

/// `rbe_rotate_left`.
///
/// # Safety
///
/// `rbe` is a linked entry of `rbt` with a right child.
unsafe fn rbe_rotate_left<T: RbType>(rbt: &RbTree, rbe: *const RbtEntry) {
    // SAFETY: every pointer followed is a link of the tree, which only holds live entries; the
    // caller guarantees the right child exists.
    unsafe {
        let tmp = e(rbe).rbt_right.get();
        let tmp_left = e(tmp).rbt_left.get();
        e(rbe).rbt_right.set(tmp_left);
        if !tmp_left.is_null() {
            e(tmp_left).rbt_parent.set(rbe);
        }

        let parent = e(rbe).rbt_parent.get();
        e(tmp).rbt_parent.set(parent);
        if !parent.is_null() {
            if rbe == e(parent).rbt_left.get() {
                e(parent).rbt_left.set(tmp);
            } else {
                e(parent).rbt_right.set(tmp);
            }
        } else {
            rbt.rbt_root.set(tmp);
        }

        e(tmp).rbt_left.set(rbe);
        e(rbe).rbt_parent.set(tmp);

        if T::AUGMENTED {
            rbe_augment::<T>(rbe);
            rbe_augment::<T>(tmp);
            let parent = e(tmp).rbt_parent.get();
            if !parent.is_null() {
                rbe_augment::<T>(parent);
            }
        }
    }
}

/// `rbe_rotate_right`.
///
/// # Safety
///
/// `rbe` is a linked entry of `rbt` with a left child.
unsafe fn rbe_rotate_right<T: RbType>(rbt: &RbTree, rbe: *const RbtEntry) {
    // SAFETY: as for `rbe_rotate_left`, mirrored.
    unsafe {
        let tmp = e(rbe).rbt_left.get();
        let tmp_right = e(tmp).rbt_right.get();
        e(rbe).rbt_left.set(tmp_right);
        if !tmp_right.is_null() {
            e(tmp_right).rbt_parent.set(rbe);
        }

        let parent = e(rbe).rbt_parent.get();
        e(tmp).rbt_parent.set(parent);
        if !parent.is_null() {
            if rbe == e(parent).rbt_left.get() {
                e(parent).rbt_left.set(tmp);
            } else {
                e(parent).rbt_right.set(tmp);
            }
        } else {
            rbt.rbt_root.set(tmp);
        }

        e(tmp).rbt_right.set(rbe);
        e(rbe).rbt_parent.set(tmp);

        if T::AUGMENTED {
            rbe_augment::<T>(rbe);
            rbe_augment::<T>(tmp);
            let parent = e(tmp).rbt_parent.get();
            if !parent.is_null() {
                rbe_augment::<T>(parent);
            }
        }
    }
}

/// `rbe_insert_color`: restores the red-black invariants after `rbe` was inserted as a red leaf.
///
/// # Safety
///
/// `rbe` is a linked entry of `rbt`.
unsafe fn rbe_insert_color<T: RbType>(rbt: &RbTree, mut rbe: *const RbtEntry) {
    // SAFETY: every pointer followed is a link of the tree, which only holds live entries; a red
    // parent always has a parent of its own (the root is black).
    unsafe {
        loop {
            let mut parent = e(rbe).rbt_parent.get();
            if parent.is_null() || e(parent).rbt_color.get() != RB_RED {
                break;
            }
            let gparent = e(parent).rbt_parent.get();

            if parent == e(gparent).rbt_left.get() {
                let tmp = e(gparent).rbt_right.get();
                if !tmp.is_null() && e(tmp).rbt_color.get() == RB_RED {
                    e(tmp).rbt_color.set(RB_BLACK);
                    rbe_set_blackred(e(parent), e(gparent));
                    rbe = gparent;
                    continue;
                }

                if e(parent).rbt_right.get() == rbe {
                    rbe_rotate_left::<T>(rbt, parent);
                    core::mem::swap(&mut parent, &mut rbe);
                }

                rbe_set_blackred(e(parent), e(gparent));
                rbe_rotate_right::<T>(rbt, gparent);
            } else {
                let tmp = e(gparent).rbt_left.get();
                if !tmp.is_null() && e(tmp).rbt_color.get() == RB_RED {
                    e(tmp).rbt_color.set(RB_BLACK);
                    rbe_set_blackred(e(parent), e(gparent));
                    rbe = gparent;
                    continue;
                }

                if e(parent).rbt_left.get() == rbe {
                    rbe_rotate_right::<T>(rbt, parent);
                    core::mem::swap(&mut parent, &mut rbe);
                }

                rbe_set_blackred(e(parent), e(gparent));
                rbe_rotate_left::<T>(rbt, gparent);
            }
        }

        e(rbt.rbt_root.get()).rbt_color.set(RB_BLACK);
    }
}

/// `rbe_remove_color`: restores the red-black invariants after a black entry was unlinked,
/// `rbe` (possibly null) having taken its place under `parent`.
///
/// # Safety
///
/// `parent` and `rbe` describe a position in `rbt` as `rbe_remove` leaves it.
unsafe fn rbe_remove_color<T: RbType>(
    rbt: &RbTree,
    mut parent: *const RbtEntry,
    mut rbe: *const RbtEntry,
) {
    // SAFETY: every pointer followed is a link of the tree, which only holds live entries; the
    // loop stops before `parent` is used once `rbe` is the root (where `parent` is null), and a
    // black non-root node always has a non-null sibling.
    unsafe {
        while (rbe.is_null() || e(rbe).rbt_color.get() == RB_BLACK) && rbe != rbt.rbt_root.get() {
            if e(parent).rbt_left.get() == rbe {
                let mut tmp = e(parent).rbt_right.get();
                if e(tmp).rbt_color.get() == RB_RED {
                    rbe_set_blackred(e(tmp), e(parent));
                    rbe_rotate_left::<T>(rbt, parent);
                    tmp = e(parent).rbt_right.get();
                }
                let tmp_left = e(tmp).rbt_left.get();
                let tmp_right = e(tmp).rbt_right.get();
                if (tmp_left.is_null() || e(tmp_left).rbt_color.get() == RB_BLACK)
                    && (tmp_right.is_null() || e(tmp_right).rbt_color.get() == RB_BLACK)
                {
                    e(tmp).rbt_color.set(RB_RED);
                    rbe = parent;
                    parent = e(rbe).rbt_parent.get();
                } else {
                    if tmp_right.is_null() || e(tmp_right).rbt_color.get() == RB_BLACK {
                        let oleft = e(tmp).rbt_left.get();
                        if !oleft.is_null() {
                            e(oleft).rbt_color.set(RB_BLACK);
                        }

                        e(tmp).rbt_color.set(RB_RED);
                        rbe_rotate_right::<T>(rbt, tmp);
                        tmp = e(parent).rbt_right.get();
                    }

                    e(tmp).rbt_color.set(e(parent).rbt_color.get());
                    e(parent).rbt_color.set(RB_BLACK);
                    let tmp_right = e(tmp).rbt_right.get();
                    if !tmp_right.is_null() {
                        e(tmp_right).rbt_color.set(RB_BLACK);
                    }

                    rbe_rotate_left::<T>(rbt, parent);
                    rbe = rbt.rbt_root.get();
                    break;
                }
            } else {
                let mut tmp = e(parent).rbt_left.get();
                if e(tmp).rbt_color.get() == RB_RED {
                    rbe_set_blackred(e(tmp), e(parent));
                    rbe_rotate_right::<T>(rbt, parent);
                    tmp = e(parent).rbt_left.get();
                }
                let tmp_left = e(tmp).rbt_left.get();
                let tmp_right = e(tmp).rbt_right.get();
                if (tmp_left.is_null() || e(tmp_left).rbt_color.get() == RB_BLACK)
                    && (tmp_right.is_null() || e(tmp_right).rbt_color.get() == RB_BLACK)
                {
                    e(tmp).rbt_color.set(RB_RED);
                    rbe = parent;
                    parent = e(rbe).rbt_parent.get();
                } else {
                    if tmp_left.is_null() || e(tmp_left).rbt_color.get() == RB_BLACK {
                        let oright = e(tmp).rbt_right.get();
                        if !oright.is_null() {
                            e(oright).rbt_color.set(RB_BLACK);
                        }

                        e(tmp).rbt_color.set(RB_RED);
                        rbe_rotate_left::<T>(rbt, tmp);
                        tmp = e(parent).rbt_left.get();
                    }

                    e(tmp).rbt_color.set(e(parent).rbt_color.get());
                    e(parent).rbt_color.set(RB_BLACK);
                    let tmp_left = e(tmp).rbt_left.get();
                    if !tmp_left.is_null() {
                        e(tmp_left).rbt_color.set(RB_BLACK);
                    }

                    rbe_rotate_right::<T>(rbt, parent);
                    rbe = rbt.rbt_root.get();
                    break;
                }
            }
        }

        if !rbe.is_null() {
            e(rbe).rbt_color.set(RB_BLACK);
        }
    }
}

/// `rbe_remove`: unlinks `rbe` from `rbt` and returns it.
///
/// # Safety
///
/// `rbe` is a linked entry of `rbt`.
unsafe fn rbe_remove<T: RbType>(rbt: &RbTree, rbe: *const RbtEntry) -> *const RbtEntry {
    let old = rbe;
    // SAFETY: every pointer followed is a link of the tree, which only holds live entries, and
    // `rbe` is one of them by the caller's guarantee.
    unsafe {
        let mut rbe = rbe;
        let child = if e(rbe).rbt_left.get().is_null() {
            e(rbe).rbt_right.get()
        } else if e(rbe).rbt_right.get().is_null() {
            e(rbe).rbt_left.get()
        } else {
            // Two children: the in-order successor takes `old`'s place in the tree.
            rbe = e(rbe).rbt_right.get();
            loop {
                let tmp = e(rbe).rbt_left.get();
                if tmp.is_null() {
                    break;
                }
                rbe = tmp;
            }

            let child = e(rbe).rbt_right.get();
            let mut parent = e(rbe).rbt_parent.get();
            let color = e(rbe).rbt_color.get();
            if !child.is_null() {
                e(child).rbt_parent.set(parent);
            }
            if !parent.is_null() {
                if e(parent).rbt_left.get() == rbe {
                    e(parent).rbt_left.set(child);
                } else {
                    e(parent).rbt_right.set(child);
                }
                rbe_if_augment::<T>(parent);
            } else {
                rbt.rbt_root.set(child);
            }
            if e(rbe).rbt_parent.get() == old {
                parent = rbe;
            }
            // *rbe = *old
            e(rbe).rbt_parent.set(e(old).rbt_parent.get());
            e(rbe).rbt_left.set(e(old).rbt_left.get());
            e(rbe).rbt_right.set(e(old).rbt_right.get());
            e(rbe).rbt_color.set(e(old).rbt_color.get());

            let tmp = e(old).rbt_parent.get();
            if !tmp.is_null() {
                if e(tmp).rbt_left.get() == old {
                    e(tmp).rbt_left.set(rbe);
                } else {
                    e(tmp).rbt_right.set(rbe);
                }
                rbe_if_augment::<T>(tmp);
            } else {
                rbt.rbt_root.set(rbe);
            }

            e(e(old).rbt_left.get()).rbt_parent.set(rbe);
            let old_right = e(old).rbt_right.get();
            if !old_right.is_null() {
                e(old_right).rbt_parent.set(rbe);
            }

            if T::AUGMENTED && !parent.is_null() {
                let mut tmp = parent;
                loop {
                    rbe_augment::<T>(tmp);
                    tmp = e(tmp).rbt_parent.get();
                    if tmp.is_null() {
                        break;
                    }
                }
            }

            if color == RB_BLACK {
                rbe_remove_color::<T>(rbt, parent, child);
            }
            return old;
        };

        let parent = e(rbe).rbt_parent.get();
        let color = e(rbe).rbt_color.get();

        if !child.is_null() {
            e(child).rbt_parent.set(parent);
        }
        if !parent.is_null() {
            if e(parent).rbt_left.get() == rbe {
                e(parent).rbt_left.set(child);
            } else {
                e(parent).rbt_right.set(child);
            }
            rbe_if_augment::<T>(parent);
        } else {
            rbt.rbt_root.set(child);
        }

        if color == RB_BLACK {
            rbe_remove_color::<T>(rbt, parent, child);
        }
    }
    old
}

/// `_rb_remove`: unlinks `elm` from `rbt` and returns it.
///
/// # Safety
///
/// `elm` is in `rbt`.
pub unsafe fn _rb_remove<'a, T: RbType>(rbt: &RbTree, elm: &'a T::Elem) -> &'a T::Elem {
    // SAFETY: forwarded; the entry returned is `elm`'s own.
    let old = unsafe { rbe_remove::<T>(rbt, rb_n2e::<T>(elm)) };
    debug_assert!(ptr::eq(old, rb_n2e::<T>(elm)));
    elm
}

/// `_rb_insert`: links `elm` into `rbt`; returns the element already there with an equal key
/// instead, leaving the tree unchanged.
///
/// # Safety
///
/// `elm` is in no tree of this type and stays valid and in place until unlinked.
pub unsafe fn _rb_insert<'a, T: RbType>(rbt: &'a RbTree, elm: &T::Elem) -> Option<&'a T::Elem> {
    let rbe = rb_n2e::<T>(elm);
    // SAFETY: every pointer followed is a link of the tree, which only holds live entries, and
    // `rbe` is the entry of the caller's live `elm`.
    unsafe {
        let mut tmp = rbt.rbt_root.get();
        let mut parent = ptr::null();
        let mut comp = Ordering::Equal;

        while !tmp.is_null() {
            parent = tmp;
            let node = &*rb_e2n::<T>(tmp);
            comp = T::compare(elm, node);
            match comp {
                Ordering::Less => tmp = e(tmp).rbt_left.get(),
                Ordering::Greater => tmp = e(tmp).rbt_right.get(),
                Ordering::Equal => return Some(node),
            }
        }

        rbe_set(e(rbe), parent);

        if !parent.is_null() {
            if comp == Ordering::Less {
                e(parent).rbt_left.set(rbe);
            } else {
                e(parent).rbt_right.set(rbe);
            }
            rbe_if_augment::<T>(parent);
        } else {
            rbt.rbt_root.set(rbe);
        }

        rbe_insert_color::<T>(rbt, rbe);
    }
    None
}

/// `_rb_find`: the element whose key equals `key`'s.
pub fn _rb_find<'a, T: RbType>(rbt: &'a RbTree, key: &T::Elem) -> Option<&'a T::Elem> {
    let mut tmp = rbt.rbt_root.get();
    // SAFETY: every pointer followed is a link of the tree, which only holds live entries.
    unsafe {
        while !tmp.is_null() {
            let node = &*rb_e2n::<T>(tmp);
            match T::compare(key, node) {
                Ordering::Less => tmp = e(tmp).rbt_left.get(),
                Ordering::Greater => tmp = e(tmp).rbt_right.get(),
                Ordering::Equal => return Some(node),
            }
        }
    }
    None
}

/// `_rb_nfind`: the first element whose key is greater than or equal to `key`'s.
pub fn _rb_nfind<'a, T: RbType>(rbt: &'a RbTree, key: &T::Elem) -> Option<&'a T::Elem> {
    let mut tmp = rbt.rbt_root.get();
    let mut res = None;
    // SAFETY: as for `_rb_find`.
    unsafe {
        while !tmp.is_null() {
            let node = &*rb_e2n::<T>(tmp);
            match T::compare(key, node) {
                Ordering::Less => {
                    res = Some(node);
                    tmp = e(tmp).rbt_left.get();
                }
                Ordering::Greater => tmp = e(tmp).rbt_right.get(),
                Ordering::Equal => return Some(node),
            }
        }
    }
    res
}

/// `_rb_next`: the in-order successor of `elm`, which must be linked.
pub fn _rb_next<T: RbType>(elm: &T::Elem) -> Option<&T::Elem> {
    let mut rbe = rb_n2e::<T>(elm);
    // SAFETY: every pointer followed is a link of the tree, which only holds live entries.
    unsafe {
        if !e(rbe).rbt_right.get().is_null() {
            rbe = e(rbe).rbt_right.get();
            while !e(rbe).rbt_left.get().is_null() {
                rbe = e(rbe).rbt_left.get();
            }
        } else {
            let parent = e(rbe).rbt_parent.get();
            if !parent.is_null() && rbe == e(parent).rbt_left.get() {
                rbe = parent;
            } else {
                loop {
                    let parent = e(rbe).rbt_parent.get();
                    if parent.is_null() || rbe != e(parent).rbt_right.get() {
                        break;
                    }
                    rbe = parent;
                }
                rbe = e(rbe).rbt_parent.get();
            }
        }
        if rbe.is_null() {
            None
        } else {
            Some(&*rb_e2n::<T>(rbe))
        }
    }
}

/// `_rb_prev`: the in-order predecessor of `elm`, which must be linked.
pub fn _rb_prev<T: RbType>(elm: &T::Elem) -> Option<&T::Elem> {
    let mut rbe = rb_n2e::<T>(elm);
    // SAFETY: as for `_rb_next`, mirrored.
    unsafe {
        if !e(rbe).rbt_left.get().is_null() {
            rbe = e(rbe).rbt_left.get();
            while !e(rbe).rbt_right.get().is_null() {
                rbe = e(rbe).rbt_right.get();
            }
        } else {
            let parent = e(rbe).rbt_parent.get();
            if !parent.is_null() && rbe == e(parent).rbt_right.get() {
                rbe = parent;
            } else {
                loop {
                    let parent = e(rbe).rbt_parent.get();
                    if parent.is_null() || rbe != e(parent).rbt_left.get() {
                        break;
                    }
                    rbe = parent;
                }
                rbe = e(rbe).rbt_parent.get();
            }
        }
        if rbe.is_null() {
            None
        } else {
            Some(&*rb_e2n::<T>(rbe))
        }
    }
}

/// `_rb_root`.
pub fn _rb_root<T: RbType>(rbt: &RbTree) -> Option<&T::Elem> {
    let rbe = rbt.rbt_root.get();
    if rbe.is_null() {
        return None;
    }
    // SAFETY: a non-null root is a live linked entry.
    Some(unsafe { &*rb_e2n::<T>(rbe) })
}

/// `_rb_min`: the element with the smallest key.
pub fn _rb_min<T: RbType>(rbt: &RbTree) -> Option<&T::Elem> {
    let mut rbe = rbt.rbt_root.get();
    let mut parent = ptr::null();
    // SAFETY: every pointer followed is a link of the tree, which only holds live entries.
    unsafe {
        while !rbe.is_null() {
            parent = rbe;
            rbe = e(rbe).rbt_left.get();
        }
        if parent.is_null() {
            None
        } else {
            Some(&*rb_e2n::<T>(parent))
        }
    }
}

/// `_rb_max`: the element with the largest key.
pub fn _rb_max<T: RbType>(rbt: &RbTree) -> Option<&T::Elem> {
    let mut rbe = rbt.rbt_root.get();
    let mut parent = ptr::null();
    // SAFETY: as for `_rb_min`.
    unsafe {
        while !rbe.is_null() {
            parent = rbe;
            rbe = e(rbe).rbt_right.get();
        }
        if parent.is_null() {
            None
        } else {
            Some(&*rb_e2n::<T>(parent))
        }
    }
}

/// `_rb_left`: the left child of `node`.
pub fn _rb_left<T: RbType>(node: &T::Elem) -> Option<&T::Elem> {
    // SAFETY: `node`'s entry is inside `node`; a non-null link is a live linked entry.
    unsafe {
        let rbe = e(rb_n2e::<T>(node)).rbt_left.get();
        if rbe.is_null() {
            None
        } else {
            Some(&*rb_e2n::<T>(rbe))
        }
    }
}

/// `_rb_right`: the right child of `node`.
pub fn _rb_right<T: RbType>(node: &T::Elem) -> Option<&T::Elem> {
    // SAFETY: as for `_rb_left`.
    unsafe {
        let rbe = e(rb_n2e::<T>(node)).rbt_right.get();
        if rbe.is_null() {
            None
        } else {
            Some(&*rb_e2n::<T>(rbe))
        }
    }
}

/// `_rb_parent`: the parent of `node`.
pub fn _rb_parent<T: RbType>(node: &T::Elem) -> Option<&T::Elem> {
    // SAFETY: as for `_rb_left`.
    unsafe {
        let rbe = e(rb_n2e::<T>(node)).rbt_parent.get();
        if rbe.is_null() {
            None
        } else {
            Some(&*rb_e2n::<T>(rbe))
        }
    }
}

fn entry_or_null<T: RbType>(node: Option<&T::Elem>) -> *const RbtEntry {
    node.map_or(ptr::null(), rb_n2e::<T>)
}

/// `_rb_set_left`: makes `left` the left child of `node` without rebalancing.
///
/// # Safety
///
/// The caller is rebuilding the tree by hand and keeps it consistent.
pub unsafe fn _rb_set_left<T: RbType>(node: &T::Elem, left: Option<&T::Elem>) {
    // SAFETY: `node`'s entry is inside `node`.
    unsafe { e(rb_n2e::<T>(node)) }
        .rbt_left
        .set(entry_or_null::<T>(left));
}

/// `_rb_set_right`: makes `right` the right child of `node` without rebalancing.
///
/// # Safety
///
/// As for [`_rb_set_left`].
pub unsafe fn _rb_set_right<T: RbType>(node: &T::Elem, right: Option<&T::Elem>) {
    // SAFETY: `node`'s entry is inside `node`.
    unsafe { e(rb_n2e::<T>(node)) }
        .rbt_right
        .set(entry_or_null::<T>(right));
}

/// `_rb_set_parent`: makes `parent` the parent of `node` without rebalancing.
///
/// # Safety
///
/// As for [`_rb_set_left`].
pub unsafe fn _rb_set_parent<T: RbType>(node: &T::Elem, parent: Option<&T::Elem>) {
    // SAFETY: `node`'s entry is inside `node`.
    unsafe { e(rb_n2e::<T>(node)) }
        .rbt_parent
        .set(entry_or_null::<T>(parent));
}

/// `_rb_poison`: fills the links of an unlinked `node` with `poison`, so a stale use faults.
pub fn _rb_poison<T: RbType>(node: &T::Elem, poison: usize) {
    // SAFETY: `node`'s entry is inside `node`.
    let rbe = unsafe { e(rb_n2e::<T>(node)) };
    let p = poison as *const RbtEntry;
    rbe.rbt_parent.set(p);
    rbe.rbt_left.set(p);
    rbe.rbt_right.set(p);
}

/// `_rb_check`: whether every link of `node` still holds `poison`.
pub fn _rb_check<T: RbType>(node: &T::Elem, poison: usize) -> bool {
    // SAFETY: `node`'s entry is inside `node`.
    let rbe = unsafe { e(rb_n2e::<T>(node)) };
    rbe.rbt_parent.get() as usize == poison
        && rbe.rbt_left.get() as usize == poison
        && rbe.rbt_right.get() as usize == poison
}

#[cfg(test)]
mod tests;
