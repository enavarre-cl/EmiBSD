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
 * Copyright (c) 1988, 1989, 1993
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
 *	@(#)radix.h	8.2 (Berkeley) 10/31/94
 */
/*
 * Copyright (c) 1988, 1989, 1993
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
 *	@(#)radix.c	8.6 (Berkeley) 10/17/95
 */
/* </LICENSES> */

/* <CODE> */
//! Radix search trees for routing lookups: `<net/radix.h>` and `net/radix.c`.
//!
//! Upstream: sys/net/radix.h @ 3ce1f3f79392
//! Upstream: sys/net/radix.c @ 3ce1f3f79392
//!
//! The data structure for the keys is a radix tree with one way branching removed. The index
//! `rn_b` at an internal node n represents a bit position to be tested. The tree is arranged so
//! that all descendants of a node n have keys whose bits all agree up to position `rn_b - 1`.
//! (We say the index of n is `rn_b`.) There is at least one descendant which has a one bit at
//! position `rn_b`, and at least one with a zero there.
//!
//! A route is determined by a pair of key and mask. We require that the bit-wise logical and
//! of the key and mask be the key. We define the index of a route associated with the mask to
//! be the first bit number in the mask where 0 occurs (with bit number 0 representing the
//! highest order bit).
//!
//! We say a mask is normal if every bit is 0 past the index of the mask. If a node n has a
//! descendant (k, m) with index(m) == index(n) == `rn_b`, and m is a normal mask, then the
//! route applies to every descendant of n. If index(m) < `rn_b`, this implies the trailing
//! last few bits of k before bit b are all 0 (and hence consequently true of every descendant
//! of n), so the route applies to all descendants of the node as well.
//!
//! Similar logic shows that a non-normal mask m such that index(m) <= index(n) could
//! potentially apply to many children of n. Thus, for each non-host route, we attach its mask
//! to a list at an internal node as high in the tree as we can go.
//!
//! The present version of the code makes use of normal routes in short-circuiting an explicit
//! mask and compare operation when testing whether a key satisfies a normal route, and also in
//! remembering the unique leaf that governs a subtree.
//!
//! Keys and masks are byte strings whose first byte is their length (a `struct sockaddr`'s
//! `sa_len`, `SALEN`). A leaf points at its caller's key, which must stay where it is while the
//! leaf is in a tree; masks are copied once into the shared mask tree (`mask_rnhead`), and a
//! leaf points at that unique copy, so masks compare by address.
//!
//! The nodes a route occupies are the caller's: `rn_addroute` takes a pair of [`RadixNode`]s
//! (`struct radix_node treenodes[2]`), usually embedded in the caller's own structure (pf's
//! `pfrke_node[2]`), and makes the first one the leaf and, unless the key is already in the
//! tree, the second one the internal node above it. `rn_delete` hands the pair back. Every
//! leaf is therefore the first element of a pair: the head's own leaves, the mask tree's
//! (`malloc`ed with their key), or a caller's.
//!
//! The tree invariant the `unsafe fn`s rely on: every node reachable from a head is live (the
//! head's, the mask tree's, which are never freed, and callers' pairs, which stay until
//! `rn_delete` returns them); an internal node (`rn_b >= 0`) has both children; every node has
//! a parent (the top's is itself); and every key a tree holds, like every key passed in, is
//! readable for its length byte's count and for the `keylen` given to [`rn_init`]. Callers
//! serialise all access to a tree (the C relies on the net lock or pf's lock).
//!
//! Status: `ported` (M9).
//!
//! ## Deviations
//! - The `rn_u` union (leaf: `rn_key`, `rn_mask`, `rn_dupedkey`; internal node: `rn_off`,
//!   `rn_l`, `rn_r`) and `rm_rmu` (`rm_mask`, `rm_leaf`) are side-by-side fields
//!   (`docs/C_TO_RUST.md`): every path reads the member its node kind (`rn_b`) or the mask's
//!   `RNF_NORMAL` selects, and `*to = *from` copies all of them. The one exception is
//!   `rn_link_dupedkey`'s branch for a parent at bit 0, which in C writes the parent's
//!   `rn_dupedkey` and so, through the union, its `rn_r`; here it writes `rn_dupedkey` only.
//!   An internal node at bit 0 needs a tree at offset 0 (only the mask tree, which never has
//!   duplicated keys) holding keys whose length bytes differ in their top bit.
//! - Links are `Cell<*const RadixNode>` / `Cell<*const RadixMask>` (NULL kept): the nodes are
//!   the callers', reached through each other and through the pair arithmetic (`&tt[1]`), so
//!   the functions that follow them are `unsafe fn`s whose contract is the tree invariant
//!   above. The public functions return `Option<&'static RadixNode>` for `struct radix_node *`
//!   (NULL is `None`); `'static` stands for "until `rn_delete` returns it", as for pool items.
//! - Keys and masks are `*const u8` (`caddr_t`/`void *`), NULL for "no mask": the tree keeps
//!   them, so they cannot be borrowed slices.
//! - `rn_walktree`'s `int (*f)(struct radix_node *, void *, u_int)` and its `void *w` are a
//!   closure `FnMut(&'static RadixNode, u32) -> Result<(), Errno>`, as `rtable_walk`'s are;
//!   the first error stops the walk and is returned.
//! - `rn_inithead(void **head, int off)` takes `&mut Option<&'static RadixNodeHead>` and
//!   returns `bool` (the C's 1 or 0); `off` is in bytes, as in C (`offsetof(struct
//!   sockaddr_in, sin_addr)`). `rn_inithead0` always returned 1 and returns nothing;
//!   `rn_initmask`'s 1 is `Err(ENOMEM)`.
//! - `rn_insert`'s `int *dupentry` is the second element of the returned tuple;
//!   `rn_add_dupedkey` and `rn_del_radix_mask` return `bool` (`false` where the C returns -1).
//!   `rn_add_dupedkey` keeps its unused `head` and `prio` arguments, `rn_delete` its unused
//!   `rn`.
//! - `rn_delete`'s `dupedkey_tt` (the start of the multipath chain) is assigned in C but never
//!   read; it is not kept. Its key comparison `memcmp(v + off, tt->rn_key + off, vlen - off)`
//!   compares nothing when `vlen <= off` (the C would pass a negative length).
//! - Only the functions `radix.h` declares are public. The other non-`static` ones of
//!   `radix.c` (`rn_refines`, `rn_addmask`, `rn_insert`, ...) and the globals `mask_rnhead`
//!   and `rtmask_pool` are private to the module; nothing else in the kernel uses them.
//! - `rn_addmask`'s on-stack `addmask_key` starts zeroed, so its trailing-zero trim never
//!   reads an uninitialised length byte.
//! - The globals are atomics (`rn_zeros`, `rn_ones`, `max_keylen`, `mask_rnhead`): they are
//!   written once by `rn_init`/`rn_initmask` before any tree exists.

use core::cell::Cell;
use core::mem::size_of;
use core::ptr::{self, NonNull};
use core::sync::atomic::{AtomicPtr, AtomicU32, Ordering};

use crate::kern::kern_malloc::{free, malloc, mallocarray};
use crate::kern::subr_pool::{pool_get, pool_init, pool_put};
use crate::kern::subr_prf::panic;
use crate::machine::intr::IPL_SOFTNET;
use crate::sys::errno::Errno;
use crate::sys::malloc::{M_NOWAIT, M_RTABLE, M_ZERO};
use crate::sys::pool::{PR_NOWAIT, PR_ZERO, Pool};
use crate::sys::select::NBBY;
use crate::sys::syslog::LOG_ERR;
use crate::{kassert, log};

/// `RNF_NORMAL`: leaf contains normal route.
pub const RNF_NORMAL: u8 = 1;
/// `RNF_ROOT`: leaf is root leaf for tree.
pub const RNF_ROOT: u8 = 2;
/// `RNF_ACTIVE`: this node is alive (for rtfree).
pub const RNF_ACTIVE: u8 = 4;

/// `KEYLEN_LIMIT`: maximum allowed keylen.
const KEYLEN_LIMIT: usize = 64;

/// `struct radix_node`: a radix search tree node, a leaf (`rn_b < 0`) or an internal node.
/// All-zero (`RadixNode::new()`, `bzero`) is a valid value, the state `rn_addroute` expects.
pub struct RadixNode {
    /// `rn_mklist`: list of masks contained in subtree.
    pub rn_mklist: Cell<*const RadixMask>,
    /// `rn_p`: parent.
    pub rn_p: Cell<*const RadixNode>,
    /// `rn_b`: bit offset; -1-index(netmask).
    pub rn_b: Cell<i16>,
    /// `rn_bmask`: node: mask for bit test.
    pub rn_bmask: Cell<u8>,
    /// `rn_flags`: `RNF_*`.
    pub rn_flags: Cell<u8>,
    /// `rn_key` (leaf only): object of search.
    pub rn_key: Cell<*const u8>,
    /// `rn_mask` (leaf only): netmask, if present (the mask tree's copy).
    pub rn_mask: Cell<*const u8>,
    /// `rn_dupedkey` (leaf only): the next leaf with the same key.
    pub rn_dupedkey: Cell<*const RadixNode>,
    /// `rn_off` (node only): where to start compare.
    pub rn_off: Cell<i32>,
    /// `rn_l` (node only): progeny.
    pub rn_l: Cell<*const RadixNode>,
    /// `rn_r` (node only): progeny.
    pub rn_r: Cell<*const RadixNode>,
}

impl RadixNode {
    /// A zeroed node.
    pub const fn new() -> Self {
        Self {
            rn_mklist: Cell::new(ptr::null()),
            rn_p: Cell::new(ptr::null()),
            rn_b: Cell::new(0),
            rn_bmask: Cell::new(0),
            rn_flags: Cell::new(0),
            rn_key: Cell::new(ptr::null()),
            rn_mask: Cell::new(ptr::null()),
            rn_dupedkey: Cell::new(ptr::null()),
            rn_off: Cell::new(0),
            rn_l: Cell::new(ptr::null()),
            rn_r: Cell::new(ptr::null()),
        }
    }

    /// `*self = *from`: copies every member.
    fn assign(&self, from: &RadixNode) {
        self.rn_mklist.set(from.rn_mklist.get());
        self.rn_p.set(from.rn_p.get());
        self.rn_b.set(from.rn_b.get());
        self.rn_bmask.set(from.rn_bmask.get());
        self.rn_flags.set(from.rn_flags.get());
        self.rn_key.set(from.rn_key.get());
        self.rn_mask.set(from.rn_mask.get());
        self.rn_dupedkey.set(from.rn_dupedkey.get());
        self.rn_off.set(from.rn_off.get());
        self.rn_l.set(from.rn_l.get());
        self.rn_r.set(from.rn_r.get());
    }
}

impl Default for RadixNode {
    fn default() -> Self {
        Self::new()
    }
}

/// `struct radix_mask`: annotations to tree concerning potential routes applying to subtrees.
/// A pool item (`rtmask_pool`); all-zero is a valid value (`PR_ZERO`).
pub struct RadixMask {
    /// `rm_b`: bit offset; -1-index(netmask).
    pub rm_b: Cell<i16>,
    /// `rm_unused`: cf. `rn_bmask`.
    pub rm_unused: Cell<u8>,
    /// `rm_flags`: cf. `rn_flags`.
    pub rm_flags: Cell<u8>,
    /// `rm_mklist`: more masks to try.
    pub rm_mklist: Cell<*const RadixMask>,
    /// `rm_mask`: the mask (without `RNF_NORMAL`).
    pub rm_mask: Cell<*const u8>,
    /// `rm_leaf`: for normal routes (`RNF_NORMAL`).
    pub rm_leaf: Cell<*const RadixNode>,
    /// `rm_refs`: number of references to this struct.
    pub rm_refs: Cell<i32>,
}

/// `struct radix_node_head`: a tree. Its three nodes are the empty tree's: the left root leaf
/// (all-zero key), the top, and the right root leaf (all-ones key).
pub struct RadixNodeHead {
    /// `rnh_treetop`.
    pub rnh_treetop: Cell<*const RadixNode>,
    /// `rnh_addrsize`: permit, but not require fixed keys.
    pub rnh_addrsize: Cell<i32>,
    /// `rnh_pktsize`: permit, but not require fixed keys.
    pub rnh_pktsize: Cell<i32>,
    /// `rnh_nodes`: empty tree for common case.
    pub rnh_nodes: [RadixNode; 3],
    /// `rnh_rtableid`: passed to `rn_walktree`'s function.
    pub rnh_rtableid: Cell<u32>,
}

impl RadixNodeHead {
    /// A zeroed head, before `rn_inithead0`.
    pub const fn new() -> Self {
        Self {
            rnh_treetop: Cell::new(ptr::null()),
            rnh_addrsize: Cell::new(0),
            rnh_pktsize: Cell::new(0),
            rnh_nodes: [RadixNode::new(), RadixNode::new(), RadixNode::new()],
            rnh_rtableid: Cell::new(0),
        }
    }
}

impl Default for RadixNodeHead {
    fn default() -> Self {
        Self::new()
    }
}

/// `rn_zeros`: array of 0s, `max_keylen` bytes, allocated and filled during `rn_init`.
static RN_ZEROS: AtomicPtr<u8> = AtomicPtr::new(ptr::null_mut());
/// `rn_ones`: array of 1s, the second half of `rn_zeros`'s allocation.
static RN_ONES: AtomicPtr<u8> = AtomicPtr::new(ptr::null_mut());
/// `max_keylen`: size of the above arrays.
static MAX_KEYLEN: AtomicU32 = AtomicU32::new(0);

/// `mask_rnhead`: head of shared mask tree.
static MASK_RNHEAD: AtomicPtr<RadixNodeHead> = AtomicPtr::new(ptr::null_mut());
/// `rtmask_pool`: pool for `radix_mask` structures.
static RTMASK_POOL: Pool = Pool::new();

/// `SALEN(sa)`: the length byte of a key or mask.
///
/// # Safety
///
/// `sa` points at a readable byte.
unsafe fn salen(sa: *const u8) -> usize {
    // SAFETY: the caller's contract.
    usize::from(unsafe { *sa })
}

/// `p[i]`.
///
/// # Safety
///
/// `p + i` is readable.
unsafe fn at(p: *const u8, i: usize) -> u8 {
    // SAFETY: the caller's contract.
    unsafe { *p.add(i) }
}

/// `*p` for a node pointer the tree holds, for one access.
///
/// # Safety
///
/// `p` is non-NULL and points at a live node.
unsafe fn nd(p: *const RadixNode) -> &'static RadixNode {
    // SAFETY: the caller's contract; "static" is the tree invariant's "while in the tree".
    unsafe { &*p }
}

/// `memcmp(a + from, b + from, to - from) == 0`; true when `to <= from`.
///
/// # Safety
///
/// Both are readable up to `to`.
unsafe fn bytes_eq(a: *const u8, b: *const u8, from: usize, to: usize) -> bool {
    // SAFETY: the caller's contract.
    (from..to).all(|i| unsafe { at(a, i) == at(b, i) })
}

/// `rn_search`: descends from `head` along the bits of `v` to a leaf.
///
/// # Safety
///
/// The tree invariant (module docs) holds for the tree below `head`, and `v` is readable at
/// every internal node's offset.
unsafe fn rn_search(v: *const u8, head: &'static RadixNode) -> &'static RadixNode {
    let mut x = head;
    // SAFETY: internal nodes have both children, and `v` covers their offsets (the contract).
    unsafe {
        while x.rn_b.get() >= 0 {
            x = if x.rn_bmask.get() & at(v, x.rn_off.get() as usize) != 0 {
                nd(x.rn_r.get())
            } else {
                nd(x.rn_l.get())
            };
        }
    }
    x
}

/// `rn_search_m`: as `rn_search`, following a 1 bit only where the mask `m` has one too.
///
/// # Safety
///
/// As for `rn_search`, for `v` and `m` both.
unsafe fn rn_search_m(v: *const u8, head: &'static RadixNode, m: *const u8) -> &'static RadixNode {
    let mut x = head;
    // SAFETY: as in `rn_search`.
    unsafe {
        while x.rn_b.get() >= 0 {
            let off = x.rn_off.get() as usize;
            let bmask = x.rn_bmask.get();
            x = if bmask & at(m, off) != 0 && bmask & at(v, off) != 0 {
                nd(x.rn_r.get())
            } else {
                nd(x.rn_l.get())
            };
        }
    }
    x
}

/// `rn_refines`: the mask `m` is strictly more specific than the mask `n` (every bit of `n` is
/// set in `m`, and the masks differ).
///
/// # Safety
///
/// Both masks are readable for their length bytes.
unsafe fn rn_refines(m: *const u8, n: *const u8) -> bool {
    // SAFETY: the loops read `n` below its length and `m` below its own (`lim` is the shorter
    // of the two lengths, and the last loop stops at `m`'s).
    unsafe {
        let nlen = isize::from(*n);
        let longer = nlen - isize::from(*m);
        let lim2 = nlen;
        let lim = if longer > 0 { nlen - longer } else { nlen };
        let (mut ni, mut mi) = (1isize, 1isize);
        let mut masks_are_equal = true;
        while ni < lim {
            let (nb, mb) = (*n.offset(ni), *m.offset(mi));
            if nb & !mb != 0 {
                return false;
            }
            if nb != mb {
                masks_are_equal = false;
            }
            ni += 1;
            mi += 1;
        }
        while ni < lim2 {
            if *n.offset(ni) != 0 {
                return false;
            }
            ni += 1;
        }
        if masks_are_equal && longer < 0 {
            let lim2 = mi - longer;
            while mi < lim2 {
                if *m.offset(mi) != 0 {
                    return true;
                }
                mi += 1;
            }
        }
        !masks_are_equal
    }
}

/// `rn_lookup`: with a mask `m`, the route of exactly key `v` and mask `m`; without
/// (`m` NULL), a regular `rn_match`. Never returns the tree's root leaves.
///
/// # Safety
///
/// The tree invariant (module docs) holds for `head`, `rn_init` has run, and `v` (and `m`
/// unless NULL) are keys as the module docs describe.
pub unsafe fn rn_lookup(
    v: *const u8,
    m: *const u8,
    head: &RadixNodeHead,
) -> Option<&'static RadixNode> {
    let mut netmask: *const u8 = ptr::null();
    // SAFETY: the caller's contract; `rnh_treetop` is set by `rn_inithead0`.
    unsafe {
        if !m.is_null() {
            let tm = rn_addmask(m, true, nd(head.rnh_treetop.get()).rn_off.get())?;
            netmask = tm.rn_key.get();
        }
        let mut x = rn_match(v, head);
        if !netmask.is_null() {
            while let Some(n) = x
                && n.rn_mask.get() != netmask
            {
                x = n.rn_dupedkey.get().as_ref();
            }
        }
        // Never return internal nodes to the upper layer.
        if x.is_some_and(|n| n.rn_flags.get() & RNF_ROOT != 0) {
            return None;
        }
        x
    }
}

/// `rn_satisfies_leaf`: `trial` matches `leaf`'s key under its mask (all ones without one),
/// from byte `skip` on.
///
/// # Safety
///
/// `trial` and the leaf's key and mask are readable for the shorter of their lengths, and
/// `rn_init` has run.
unsafe fn rn_satisfies_leaf(trial: *const u8, leaf: &RadixNode, skip: usize) -> bool {
    let cp2 = leaf.rn_key.get();
    let mut cp3 = leaf.rn_mask.get();
    // SAFETY: the caller's contract; `rn_ones` is `max_keylen` bytes, the longest key.
    unsafe {
        let mut length = salen(trial).min(salen(cp2));
        if cp3.is_null() {
            cp3 = RN_ONES.load(Ordering::Relaxed);
        } else {
            length = length.min(salen(cp3));
        }
        (skip..length).all(|i| (at(trial, i) ^ at(cp2, i)) & at(cp3, i) == 0)
    }
}

/// `rn_match`: the most specific route whose key and mask match `v`.
///
/// # Safety
///
/// The tree invariant (module docs) holds for `head`, `rn_init` has run, and `v` is a key as
/// the module docs describe.
pub unsafe fn rn_match(v: *const u8, head: &RadixNodeHead) -> Option<&'static RadixNode> {
    // SAFETY: the caller's contract: every node followed is live, internal nodes have both
    // children and a parent, and the keys compared are readable for the lengths used.
    unsafe {
        let top = nd(head.rnh_treetop.get());
        let off = top.rn_off.get() as usize;
        let t = rn_search(v, top);
        // See if we match exactly as a host destination or at least learn how many bits
        // match, for normal mask finesse.
        //
        // It doesn't hurt us to limit how many bytes to check to the length of the mask,
        // since if it matches we had a genuine match and the leaf we have is the most specific
        // one anyway; if it didn't match with a shorter length it would fail with a long one.
        // This wins big for class B&C netmasks which are probably the most common case...
        let vlen = if t.rn_mask.get().is_null() {
            salen(v)
        } else {
            salen(t.rn_mask.get())
        };
        let key = t.rn_key.get();
        let mut cp = off;
        while cp < vlen && at(v, cp) == at(key, cp) {
            cp += 1;
        }
        if cp >= vlen {
            // This extra grot is in case we are explicitly asked to look up the default.
            // Ugh!
            let t = if t.rn_flags.get() & RNF_ROOT != 0 {
                t.rn_dupedkey.get().as_ref()
            } else {
                Some(t)
            };
            kassert!(t.is_none_or(|t| t.rn_flags.get() & RNF_ROOT == 0));
            return t;
        }

        // on1: find first bit that differs.
        let mut test = u32::from(at(v, cp) ^ at(key, cp));
        let mut b: i32 = 7;
        loop {
            test >>= 1;
            if test == 0 {
                break;
            }
            b -= 1;
        }
        let matched_off = cp;
        b += (matched_off as i32) << 3;
        let rn_b = -1 - b;

        // If there is a host route in a duped-key chain, it will be first.
        let saved_t = t;
        let mut x = if t.rn_mask.get().is_null() {
            t.rn_dupedkey.get().as_ref()
        } else {
            Some(t)
        };
        while let Some(t) = x {
            // Even if we don't match exactly as a host, we may match if the leaf we wound up
            // at is a route to a net.
            if t.rn_flags.get() & RNF_NORMAL != 0 {
                if rn_b <= i32::from(t.rn_b.get()) {
                    kassert!(t.rn_flags.get() & RNF_ROOT == 0);
                    return Some(t);
                }
            } else if rn_satisfies_leaf(v, t, matched_off) {
                kassert!(t.rn_flags.get() & RNF_ROOT == 0);
                return Some(t);
            }
            x = t.rn_dupedkey.get().as_ref();
        }

        // Start searching up the tree.
        let mut t = saved_t;
        loop {
            t = nd(t.rn_p.get());
            let mut m = t.rn_mklist.get();
            while let Some(mm) = m.as_ref() {
                // If non-contiguous masks ever become important we can restore the masking
                // and open coding of the search and satisfaction test and put the calculation
                // of "off" back before the "do".
                if mm.rm_flags.get() & RNF_NORMAL != 0 {
                    if rn_b <= i32::from(mm.rm_b.get()) {
                        let leaf = nd(mm.rm_leaf.get());
                        kassert!(leaf.rn_flags.get() & RNF_ROOT == 0);
                        return Some(leaf);
                    }
                } else {
                    let off = (t.rn_off.get() as usize).min(matched_off);
                    let mut x = Some(rn_search_m(v, t, mm.rm_mask.get()));
                    while let Some(xx) = x
                        && xx.rn_mask.get() != mm.rm_mask.get()
                    {
                        x = xx.rn_dupedkey.get().as_ref();
                    }
                    if let Some(x) = x
                        && rn_satisfies_leaf(v, x, off)
                    {
                        kassert!(x.rn_flags.get() & RNF_ROOT == 0);
                        return Some(x);
                    }
                }
                m = mm.rm_mklist.get();
            }
            if ptr::eq(t, top) {
                break;
            }
        }
        None
    }
}

/// `rn_newpair`: makes `nodes[0]` a leaf for key `v` and `nodes[1]` the internal node testing
/// bit `b` above it; returns the internal node.
fn rn_newpair(v: *const u8, b: i32, nodes: &'static [RadixNode]) -> &'static RadixNode {
    let tt = &nodes[0];
    let t = &nodes[1];
    t.rn_b.set(b as i16);
    t.rn_bmask.set(0x80u8 >> (b & 7));
    t.rn_l.set(tt);
    t.rn_off.set(b >> 3);
    tt.rn_b.set(-1);
    tt.rn_key.set(v);
    tt.rn_p.set(t);
    tt.rn_flags.set(RNF_ACTIVE);
    t.rn_flags.set(RNF_ACTIVE);
    t
}

/// `rn_insert`: inserts key `v` into the tree with the pair `nodes`. Returns the new leaf
/// (`nodes[0]`) and `false`, or the leaf already holding the key and `true` (`*dupentry`).
///
/// # Safety
///
/// The tree invariant (module docs) holds for `head`, `v` is a key as described there and
/// stays in place while in the tree, and `nodes` holds two nodes.
unsafe fn rn_insert(
    v: *const u8,
    head: &RadixNodeHead,
    nodes: &'static [RadixNode],
) -> (&'static RadixNode, bool) {
    // SAFETY: the caller's contract, as in `rn_search`; `cp - 1` is a byte already compared.
    unsafe {
        let top = nd(head.rnh_treetop.get());
        let off = top.rn_off.get() as usize;
        let t = rn_search(v, top);

        // Find first bit at which v and t->rn_key differ.
        let vlen = salen(v);
        let key = t.rn_key.get();
        let mut cp = off;
        loop {
            if cp >= vlen {
                return (t, true);
            }
            let differ = at(key, cp) != at(v, cp);
            cp += 1;
            if differ {
                break;
            }
        }
        // on1:
        let mut cmp_res = u32::from(at(v, cp - 1) ^ at(key, cp - 1));
        let mut b = (cp as i32) << 3;
        while cmp_res != 0 {
            cmp_res >>= 1;
            b -= 1;
        }

        let mut x = top;
        let mut p;
        loop {
            p = x;
            x = if at(v, x.rn_off.get() as usize) & x.rn_bmask.get() != 0 {
                nd(x.rn_r.get())
            } else {
                nd(x.rn_l.get())
            };
            // x->rn_b < b && x->rn_b >= 0
            if !(x.rn_b.get() >= 0 && b > i32::from(x.rn_b.get())) {
                break;
            }
        }
        let t = rn_newpair(v, b, nodes);
        let tt = nd(t.rn_l.get());
        if at(v, p.rn_off.get() as usize) & p.rn_bmask.get() == 0 {
            p.rn_l.set(t);
        } else {
            p.rn_r.set(t);
        }
        x.rn_p.set(t);
        t.rn_p.set(p); // frees x, p as temp vars below
        if at(v, t.rn_off.get() as usize) & t.rn_bmask.get() == 0 {
            t.rn_r.set(x);
        } else {
            t.rn_r.set(tt);
            t.rn_l.set(x);
        }
        (tt, false)
    }
}

/// `rn_addmask`: the mask tree's leaf for the mask `n` (its unique copy is the leaf's key),
/// entering it unless `search`; bytes below `skip` are taken as all ones. A mask with no bit
/// set past `skip` is the mask tree's left root leaf. `None` when `search` finds nothing or
/// the allocation fails.
///
/// # Safety
///
/// `rn_inithead` has run once (the mask tree exists) and `n` is readable for its length byte's
/// count.
unsafe fn rn_addmask(n: *const u8, search: bool, skip: i32) -> Option<&'static RadixNode> {
    let max_keylen = MAX_KEYLEN.load(Ordering::Relaxed) as usize;
    // SAFETY: the caller's contract; the mask tree is never freed.
    let mask_rnhead: &'static RadixNodeHead = unsafe { &*MASK_RNHEAD.load(Ordering::Relaxed) };
    let mut addmask_key = [0u8; KEYLEN_LIMIT];

    // SAFETY: the caller's contract.
    let mut mlen = unsafe { salen(n) }.min(max_keylen);
    let skip = if skip == 0 { 1 } else { skip as usize };
    if mlen <= skip {
        return Some(&mask_rnhead.rnh_nodes[0]); // rn_zero root node
    }
    let ones = RN_ONES.load(Ordering::Relaxed);
    for (i, k) in addmask_key.iter_mut().enumerate().take(skip).skip(1) {
        // SAFETY: `i < skip < mlen <= max_keylen`, the size of `rn_ones`.
        *k = unsafe { at(ones, i) };
    }
    let m0 = mlen;
    for (i, k) in addmask_key.iter_mut().enumerate().take(m0).skip(skip) {
        // SAFETY: below `n`'s length.
        *k = unsafe { at(n, i) };
    }
    // Trim trailing zeroes.
    let mut cp = mlen;
    while cp > 0 && addmask_key[cp - 1] == 0 {
        cp -= 1;
    }
    mlen = cp;
    if mlen <= skip {
        return Some(&mask_rnhead.rnh_nodes[0]);
    }
    addmask_key[m0..max_keylen].fill(0);
    addmask_key[0] = mlen as u8;
    // SAFETY: the mask tree satisfies the tree invariant; its offsets are below `max_keylen`,
    // inside `addmask_key`, and its keys are `max_keylen` bytes long.
    let tm = unsafe {
        let tm = rn_search(addmask_key.as_ptr(), nd(mask_rnhead.rnh_treetop.get()));
        bytes_eq(addmask_key.as_ptr(), tm.rn_key.get(), 0, mlen).then_some(tm)
    };
    if tm.is_some() || search {
        return tm;
    }

    let size = max_keylen + 2 * size_of::<RadixNode>();
    let block = malloc(size, M_RTABLE, M_NOWAIT | M_ZERO)?;
    let pair = block.as_ptr().cast::<RadixNode>();
    // SAFETY: the block is `size` bytes, aligned for a node (malloc's chunks are aligned to
    // their power-of-two size), so it holds the two nodes and then `max_keylen` key bytes; it
    // stays in the mask tree for ever (freed only below, before anything links to it).
    let (pair, netmask): (&'static [RadixNode], *mut u8) = unsafe {
        pair.write(RadixNode::new());
        pair.add(1).write(RadixNode::new());
        let netmask = block.as_ptr().add(2 * size_of::<RadixNode>());
        ptr::copy_nonoverlapping(addmask_key.as_ptr(), netmask, mlen);
        (core::slice::from_raw_parts(pair, 2), netmask)
    };
    // SAFETY: the mask tree's invariant; the new key is `max_keylen` bytes and never moves.
    let (tm, maskduplicated) = unsafe { rn_insert(netmask, mask_rnhead, pair) };
    if maskduplicated {
        log!(LOG_ERR, "rn_addmask: mask impossibly already in tree\n");
        free(block, M_RTABLE, size);
        return Some(tm);
    }

    // Calculate index of mask, and check for normalcy.
    const NORMAL_CHARS: [u8; 9] = [0, 0x80, 0xc0, 0xe0, 0xf0, 0xf8, 0xfc, 0xfe, 0xff];
    // SAFETY: `netmask` holds the `mlen` bytes copied above; `i` stays below `mlen`.
    let byte = |i: usize| unsafe { at(netmask, i) };
    let cplim = mlen;
    let mut isnormal = true;
    let mut b = 0usize;
    let mut cp = skip;
    while cp < cplim && byte(cp) == 0xff {
        cp += 1;
    }
    if cp != cplim {
        let mut j = 0x80u8;
        while j & byte(cp) != 0 {
            b += 1;
            j >>= 1;
        }
        if byte(cp) != NORMAL_CHARS[b] || cp != cplim - 1 {
            isnormal = false;
        }
    }
    b += cp << 3;
    tm.rn_b.set((-1 - b as i32) as i16);
    if isnormal {
        tm.rn_flags.set(tm.rn_flags.get() | RNF_NORMAL);
    }
    Some(tm)
}

/// `rn_lexobetter`: an arbitrary ordering for non-contiguous masks.
///
/// # Safety
///
/// Both masks are readable for their length bytes.
unsafe fn rn_lexobetter(mp: *const u8, np: *const u8) -> bool {
    // SAFETY: the caller's contract; the comparison reads `mp`'s length, which is `np`'s too.
    unsafe {
        // Longer masks might not really be lexicographically better, but longer masks always
        // have precedence since they must be checked first. The netmasks were normalized
        // before calling this function and don't have unneeded trailing zeros.
        if salen(mp) > salen(np) {
            return true;
        }
        if salen(mp) < salen(np) {
            return false;
        }
        // Must return the first difference between the masks to ensure deterministic sorting.
        for i in 0..salen(mp) {
            let (a, b) = (at(mp, i), at(np, i));
            if a != b {
                return a > b;
            }
        }
        false
    }
}

/// `rn_new_radix_mask`: a mask annotation for the leaf `tt`, in front of `next`; NULL when the
/// pool is empty.
fn rn_new_radix_mask(tt: &'static RadixNode, next: *const RadixMask) -> *const RadixMask {
    let Some(item) = pool_get(&RTMASK_POOL, PR_NOWAIT | PR_ZERO) else {
        log!(LOG_ERR, "Mask for route not entered\n");
        return ptr::null();
    };
    // SAFETY: a zeroed item of `rtmask_pool`, sized for a `RadixMask`; all-zero is valid.
    let m = unsafe { item.cast::<RadixMask>().as_ref() };
    m.rm_b.set(tt.rn_b.get());
    m.rm_flags.set(tt.rn_flags.get());
    if tt.rn_flags.get() & RNF_NORMAL != 0 {
        m.rm_leaf.set(tt);
    } else {
        m.rm_mask.set(tt.rn_mask.get());
    }
    m.rm_mklist.set(next);
    tt.rn_mklist.set(m);
    m
}

/// `rn_lift_node`: finds the point where the `rn_mklist` needs to be changed: the highest
/// ancestor of the leaf `t` whose index its mask still covers.
///
/// # Safety
///
/// The tree invariant (module docs) holds for `t`'s tree.
unsafe fn rn_lift_node(t: &'static RadixNode) -> Option<&'static RadixNode> {
    let b = -1 - i32::from(t.rn_b.get());
    let mut t = t;
    // SAFETY: every node has a parent (the top's is itself).
    unsafe {
        // rewind possible dupedkey list to head
        while t.rn_b.get() < 0 {
            t = nd(t.rn_p.get());
        }
        // can't lift node above head of dupedkey list, give up
        if b > i32::from(t.rn_b.get()) {
            return None;
        }
        let mut x;
        loop {
            x = t;
            t = nd(t.rn_p.get());
            if !(b <= i32::from(t.rn_b.get()) && !ptr::eq(x, t)) {
                break;
            }
        }
        Some(x)
    }
}

/// `rn_add_radix_mask`: adds the new route `tt` to the highest possible ancestor's list.
///
/// # Safety
///
/// The tree invariant (module docs) holds for `tt`'s tree.
unsafe fn rn_add_radix_mask(tt: &'static RadixNode, keyduplicated: bool) {
    let b_leaf = tt.rn_b.get();

    if tt.rn_mask.get().is_null() {
        return; // can't lift at all
    }
    // SAFETY: the caller's contract.
    let Some(x) = (unsafe { rn_lift_node(tt) }) else {
        return; // didn't lift either
    };

    // Search through routes associated with node to insert new route according to index.
    // Need same criteria as when sorting dupedkeys to avoid double loop on deletion.
    let netmask = tt.rn_mask.get();
    let mut mp: &Cell<*const RadixMask> = &x.rn_mklist;
    // SAFETY: the masks on a list and the leaves they name are live (the tree invariant), and
    // masks are the mask tree's copies, readable for their lengths.
    unsafe {
        while let Some(m) = mp.get().as_ref() {
            if m.rm_b.get() < b_leaf {
                mp = &m.rm_mklist;
                continue;
            }
            if m.rm_b.get() > b_leaf {
                break;
            }
            let mmask = if m.rm_flags.get() & RNF_NORMAL != 0 {
                if keyduplicated {
                    if ptr::eq(nd(m.rm_leaf.get()).rn_p.get(), tt) {
                        // new route is better
                        m.rm_leaf.set(tt);
                    } else if cfg!(feature = "diagnostic") {
                        let mut t = m.rm_leaf.get();
                        while let Some(tn) = t.as_ref()
                            && ptr::eq(tn.rn_mklist.get(), m)
                        {
                            if ptr::eq(tn, tt) {
                                break;
                            }
                            t = tn.rn_dupedkey.get();
                        }
                        if t.is_null() {
                            log!(
                                LOG_ERR,
                                "Non-unique normal route on dupedkey, mask not entered\n"
                            );
                            return;
                        }
                    }
                    m.rm_refs.set(m.rm_refs.get() + 1);
                    tt.rn_mklist.set(m);
                    return;
                } else if tt.rn_flags.get() & RNF_NORMAL != 0 {
                    log!(LOG_ERR, "Non-unique normal route, mask not entered\n");
                    return;
                }
                nd(m.rm_leaf.get()).rn_mask.get()
            } else {
                m.rm_mask.get()
            };
            if mmask == netmask {
                m.rm_refs.set(m.rm_refs.get() + 1);
                tt.rn_mklist.set(m);
                return;
            }
            if rn_refines(netmask, mmask) || rn_lexobetter(netmask, mmask) {
                break;
            }
            mp = &m.rm_mklist;
        }
    }
    mp.set(rn_new_radix_mask(tt, mp.get()));
}

/// `rn_add_dupedkey`: links the leaf `tt` into the duplicated-key list of `saved_tt`, sorted
/// most specific mask first. `false` (the C's -1) when the list already has `tt`'s mask.
///
/// # Safety
///
/// The tree invariant (module docs) holds for `saved_tt`'s tree.
unsafe fn rn_add_dupedkey(
    saved_tt: &'static RadixNode,
    _head: &RadixNodeHead,
    tt: &'static RadixNode,
    _prio: u8,
) -> bool {
    let netmask = tt.rn_mask.get();
    let b_leaf = if netmask.is_null() { 0 } else { tt.rn_b.get() };

    let mut x: Option<&'static RadixNode> = Some(saved_tt);
    let mut xp = saved_tt;
    // SAFETY: the list's leaves are live, their masks the mask tree's copies.
    unsafe {
        while let Some(xn) = x {
            let xmask = xn.rn_mask.get();
            if xmask == netmask {
                return false;
            }
            if netmask.is_null()
                || (!xmask.is_null()
                    && (b_leaf < xn.rn_b.get() // index(netmask) > node
                        || rn_refines(netmask, xmask)
                        || rn_lexobetter(netmask, xmask)))
            {
                break;
            }
            xp = xn;
            x = xn.rn_dupedkey.get().as_ref();
        }
    }
    // If the mask is not duplicated, we wouldn't find it among possible duplicate key entries
    // anyway, so the above test doesn't hurt.
    //
    // We sort the masks for a duplicated key the same way as in a masklist -- most specific to
    // least specific. This may require the unfortunate nuisance of relocating the head of the
    // list.
    //
    // We also reverse, or doubly link the list through the parent pointer.
    //
    // (The C's `before` starts at -1 and is only tested, so it reduces to this comparison.)
    let before = x.is_some_and(|x| ptr::eq(x, saved_tt));
    // SAFETY: the caller's contract.
    unsafe { rn_link_dupedkey(tt, xp, before) };
    true
}

/// `rn_link_dupedkey`: inserts `tt` after `x`, or in place of `x` if `before`.
///
/// # Safety
///
/// The tree invariant (module docs) holds for `x`'s tree.
unsafe fn rn_link_dupedkey(tt: &'static RadixNode, x: &'static RadixNode, before: bool) {
    // SAFETY: `x` has a parent; a list's leaves are live.
    unsafe {
        if before {
            let xp = nd(x.rn_p.get());
            if xp.rn_b.get() > 0 {
                // link in at head of list
                tt.rn_dupedkey.set(x);
                tt.rn_flags.set(x.rn_flags.get());
                tt.rn_p.set(xp);
                x.rn_p.set(tt);
                if ptr::eq(xp.rn_l.get(), x) {
                    xp.rn_l.set(tt);
                } else {
                    xp.rn_r.set(tt);
                }
            } else {
                tt.rn_dupedkey.set(x);
                xp.rn_dupedkey.set(tt);
                tt.rn_p.set(xp);
                x.rn_p.set(tt);
            }
        } else {
            tt.rn_dupedkey.set(x.rn_dupedkey.get());
            x.rn_dupedkey.set(tt);
            tt.rn_p.set(x);
            if let Some(d) = tt.rn_dupedkey.get().as_ref() {
                d.rn_p.set(tt);
            }
        }
    }
}

/// `rn_fixup_nodes`: ensures that routes are properly promoted upwards after the leaf `tt`
/// was inserted. It adjusts the `rn_mklist` of the parent node to make sure overlapping routes
/// can be found.
///
/// There are two cases for `tt`'s sibling: a leaf with a possible `rn_dupedkey` list, or an
/// internal node with maybe its own mklist. If the mask of the route is bigger than the
/// current branch bit then a `rn_mklist` entry needs to be made.
///
/// # Safety
///
/// The tree invariant (module docs) holds for `tt`'s tree.
unsafe fn rn_fixup_nodes(tt: &'static RadixNode) {
    // SAFETY: `tt`'s parent is internal, so it has both children; lists hold live nodes and
    // masks.
    unsafe {
        let tp = nd(tt.rn_p.get());
        let mut x = if ptr::eq(tp.rn_r.get(), tt) {
            tp.rn_l.get()
        } else {
            tp.rn_r.get()
        };

        let b_leaf = -1 - tp.rn_b.get();
        if nd(x).rn_b.get() < 0 {
            // x is a leaf node
            let mut xx: Option<&RadixNode> = None;
            let mut mp: &Cell<*const RadixMask> = &tp.rn_mklist;
            while let Some(xn) = x.as_ref() {
                if let Some(xxn) = xx
                    && !xxn.rn_mklist.get().is_null()
                    && xxn.rn_mask.get() == xn.rn_mask.get()
                    && xn.rn_mklist.get().is_null()
                {
                    // multipath route
                    xn.rn_mklist.set(xxn.rn_mklist.get());
                    let m = &*xn.rn_mklist.get();
                    m.rm_refs.set(m.rm_refs.get() + 1);
                }
                if !xn.rn_mask.get().is_null()
                    && xn.rn_b.get() >= b_leaf
                    && xn.rn_mklist.get().is_null()
                {
                    let m = rn_new_radix_mask(xn, ptr::null());
                    mp.set(m);
                    if let Some(m) = m.as_ref() {
                        mp = &m.rm_mklist;
                    }
                }
                xx = Some(xn);
                x = xn.rn_dupedkey.get();
            }
        } else if !nd(x).rn_mklist.get().is_null() {
            // x is an internal node
            let x = nd(x);
            // Skip over masks whose index is > that of new node.
            let mut mp: &Cell<*const RadixMask> = &x.rn_mklist;
            while let Some(m) = mp.get().as_ref() {
                if m.rm_b.get() >= b_leaf {
                    break;
                }
                mp = &m.rm_mklist;
            }
            tp.rn_mklist.set(mp.get());
            mp.set(ptr::null());
        }
    }
}

/// `rn_addroute`: enters the route of key `v` and mask `n` (NULL for a host route) into the
/// tree, using the caller's pair `treenodes` (zeroed: `RadixNode::new()` or `bzero`). Returns
/// the route's leaf (`treenodes[0]`), or `None` when the route already exists or memory ran
/// out.
///
/// # Safety
///
/// The tree invariant (module docs) holds for `head` and `rn_init` has run; `v` (and `n`
/// unless NULL) are keys as the module docs describe; `v` and `treenodes` stay in place, and
/// `treenodes` is used for nothing else, until `rn_delete` returns the leaf.
pub unsafe fn rn_addroute(
    v: *const u8,
    n: *const u8,
    head: &RadixNodeHead,
    treenodes: &'static [RadixNode; 2],
    prio: u8,
) -> Option<&'static RadixNode> {
    // SAFETY: the caller's contract.
    unsafe {
        let top = nd(head.rnh_treetop.get());
        let mut tm = None;

        // In dealing with non-contiguous masks, there may be many different routes which have
        // the same mask. We will find it useful to have a unique pointer to the mask to speed
        // avoiding duplicate references at nodes and possibly save time in calculating indices.
        if !n.is_null() {
            tm = Some(rn_addmask(n, false, top.rn_off.get())?);
        }

        let (mut tt, keyduplicated) = rn_insert(v, head, treenodes);
        let saved_tt = tt;

        if keyduplicated {
            tt = &treenodes[0];
            tt.rn_key.set(v);
            tt.rn_b.set(-1);
            tt.rn_flags.set(RNF_ACTIVE);
        }

        // Put mask into the node.
        if let Some(tm) = tm {
            tt.rn_mask.set(tm.rn_key.get());
            tt.rn_b.set(tm.rn_b.get());
            tt.rn_flags
                .set(tt.rn_flags.get() | (tm.rn_flags.get() & RNF_NORMAL));
        }

        // Either insert into dupedkey list or as a leaf node.
        if keyduplicated {
            if !rn_add_dupedkey(saved_tt, head, tt, prio) {
                return None;
            }
        } else {
            rn_fixup_nodes(tt);
        }

        // finally insert a radix_mask element if needed
        rn_add_radix_mask(tt, keyduplicated);
        Some(tt)
    }
}

/// `rn_del_radix_mask`: cleans up the mask list, `tt` points to the route that needs to be
/// cleaned. `false` (the C's -1) when the annotations are inconsistent and the route must
/// stay.
///
/// # Safety
///
/// The tree invariant (module docs) holds for `tt`'s tree.
unsafe fn rn_del_radix_mask(tt: &'static RadixNode) -> bool {
    // Cleanup mask list from possible references to this route.
    let saved_m = tt.rn_mklist.get();
    if tt.rn_mask.get().is_null() || saved_m.is_null() {
        return true;
    }
    // SAFETY: a leaf's annotation, its list and the leaves they name are live.
    unsafe {
        let m = &*saved_m;
        if tt.rn_flags.get() & RNF_NORMAL != 0 {
            let leaf_is_tt = ptr::eq(m.rm_leaf.get(), tt);
            if !leaf_is_tt && m.rm_refs.get() == 0 {
                log!(LOG_ERR, "rn_delete: inconsistent normal annotation\n");
                return false;
            }
            if !leaf_is_tt {
                m.rm_refs.set(m.rm_refs.get() - 1);
                if m.rm_refs.get() >= 0 {
                    return true;
                }
                log!(LOG_ERR, "rn_delete: inconsistent mklist refcount\n");
            }
            // If we end up here tt should be m->rm_leaf and therefore tt should be the head of
            // a multipath chain. If this is not the case the table is no longer consistent.
            if m.rm_refs.get() > 0 {
                let d = tt.rn_dupedkey.get();
                if d.is_null() || !ptr::eq(nd(d).rn_mklist.get(), m) {
                    log!(LOG_ERR, "rn_delete: inconsistent dupedkey list\n");
                    return false;
                }
                m.rm_leaf.set(d);
                m.rm_refs.set(m.rm_refs.get() - 1);
                return true;
            }
            // else tt is last and only route
        } else {
            if m.rm_mask.get() != tt.rn_mask.get() {
                log!(LOG_ERR, "rn_delete: inconsistent annotation\n");
                return true;
            }
            m.rm_refs.set(m.rm_refs.get() - 1);
            if m.rm_refs.get() >= 0 {
                return true;
            }
        }

        // No other references hold to the radix_mask remove it from the tree.
        let Some(x) = rn_lift_node(tt) else {
            return true; // Wasn't lifted at all
        };

        // Finally eliminate the radix_mask from the tree.
        let mut found = false;
        let mut mp: &Cell<*const RadixMask> = &x.rn_mklist;
        while let Some(m) = mp.get().as_ref() {
            if ptr::eq(m, saved_m) {
                mp.set(m.rm_mklist.get());
                pool_put(&RTMASK_POOL, NonNull::from(m).cast());
                found = true;
                break;
            }
            mp = &m.rm_mklist;
        }

        if !found {
            log!(LOG_ERR, "rn_delete: couldn't find our annotation\n");
            if tt.rn_flags.get() & RNF_NORMAL != 0 {
                return false; // Dangling ref to us
            }
        }
    }
    true
}

/// `rn_swap_nodes`: moves the internal node `from` into `to` and fixes up the parent and
/// child pointers.
///
/// # Safety
///
/// The tree invariant (module docs) holds for `from`'s tree, `from` is an internal node in it
/// and `to` is not in use.
unsafe fn rn_swap_nodes(from: &'static RadixNode, to: &'static RadixNode) {
    to.assign(from);
    // SAFETY: an internal node has a parent and both children.
    unsafe {
        let p = nd(from.rn_p.get());
        if ptr::eq(p.rn_l.get(), from) {
            p.rn_l.set(to);
        } else {
            p.rn_r.set(to);
        }
        nd(to.rn_l.get()).rn_p.set(to);
        nd(to.rn_r.get()).rn_p.set(to);
    }
}

/// `&tt[1]`: the second node of the pair whose first node is the leaf `tt`.
///
/// # Safety
///
/// `tt` is the first node of a pair (every leaf but the head's right root leaf is).
unsafe fn pair_node(tt: &'static RadixNode) -> &'static RadixNode {
    // SAFETY: the caller's contract.
    unsafe { &*ptr::from_ref(tt).add(1) }
}

/// `rn_delete`: removes the route of key `v` and mask `n` (NULL for a host route) and returns
/// its leaf, the first node of the pair `rn_addroute` was given; `None` when there is no such
/// route or the tree's annotations are inconsistent. `rn` is unused, as in C.
///
/// # Safety
///
/// The tree invariant (module docs) holds for `head` and `rn_init` has run; `v` (and `n`
/// unless NULL) are keys as the module docs describe. Inside an `rn_walktree` function, only
/// the leaf being visited may be deleted.
pub unsafe fn rn_delete(
    v: *const u8,
    n: *const u8,
    head: &RadixNodeHead,
    _rn: Option<&RadixNode>,
) -> Option<&'static RadixNode> {
    // SAFETY: the caller's contract: the nodes followed are live and linked as the tree
    // invariant says; every leaf removed here is the first node of a pair.
    unsafe {
        let top = nd(head.rnh_treetop.get());
        let off = top.rn_off.get() as usize;
        let vlen = salen(v);

        // Implement a lookup similar to rn_lookup but we need to save the radix leaf node
        // (where the rn_dupedkey list starts) so it is not possible to use rn_lookup.
        let mut tt = rn_search(v, top);
        // make sure the key is a perfect match
        if !bytes_eq(v, tt.rn_key.get(), off, vlen) {
            return None;
        }

        // Here, tt is the deletion target, and saved_tt is the head of the dupedkey chain.
        let saved_tt = tt;

        // make tt point to the start of the rn_dupedkey list of multipath routes.
        if !n.is_null() {
            let tm = rn_addmask(n, true, off as i32)?;
            let netmask = tm.rn_key.get();
            while tt.rn_mask.get() != netmask {
                tt = tt.rn_dupedkey.get().as_ref()?;
            }
        }

        kassert!(tt.rn_flags.get() & RNF_ROOT == 0);

        // remove possible radix_mask
        if !rn_del_radix_mask(tt) {
            return None;
        }

        // Finally eliminate us from tree.
        let tp = nd(tt.rn_p.get());
        let tt1 = pair_node(tt);
        if !saved_tt.rn_dupedkey.get().is_null() {
            let x;
            if ptr::eq(tt, saved_tt) {
                x = nd(saved_tt.rn_dupedkey.get());
                x.rn_p.set(tp);
                if ptr::eq(tp.rn_l.get(), tt) {
                    tp.rn_l.set(x);
                } else {
                    tp.rn_r.set(x);
                }
            } else {
                x = saved_tt;
                tp.rn_dupedkey.set(tt.rn_dupedkey.get());
                if let Some(d) = tt.rn_dupedkey.get().as_ref() {
                    d.rn_p.set(tp);
                }
            }

            // We may be holding an active internal node in the tree.
            if tt1.rn_flags.get() & RNF_ACTIVE != 0 {
                rn_swap_nodes(tt1, pair_node(x));
            }
            // over and out
        } else {
            // non-rn_dupedkey case, remove tt and tp node from the tree
            let x = if ptr::eq(tp.rn_l.get(), tt) {
                nd(tp.rn_r.get())
            } else {
                nd(tp.rn_l.get())
            };
            let pp = nd(tp.rn_p.get());
            if ptr::eq(pp.rn_r.get(), tp) {
                pp.rn_r.set(x);
            } else {
                pp.rn_l.set(x);
            }
            x.rn_p.set(pp);

            // Demote routes attached to us (actually on the internal parent node).
            if !tp.rn_mklist.get().is_null() {
                if x.rn_b.get() >= 0 {
                    let mut mp: &Cell<*const RadixMask> = &x.rn_mklist;
                    while let Some(m) = mp.get().as_ref() {
                        mp = &m.rm_mklist;
                    }
                    mp.set(tp.rn_mklist.get());
                } else {
                    // If there are any key,mask pairs in a sibling duped-key chain, some
                    // subset will appear sorted in the same order attached to our mklist.
                    let mut m = tp.rn_mklist.get();
                    let mut xp: *const RadixNode = x;
                    while !m.is_null()
                        && let Some(xn) = xp.as_ref()
                    {
                        if ptr::eq(m, xn.rn_mklist.get()) {
                            let mr = &*m;
                            let mut mm = mr.rm_mklist.get();
                            xn.rn_mklist.set(ptr::null());
                            mr.rm_refs.set(mr.rm_refs.get() - 1);
                            if mr.rm_refs.get() < 0 {
                                pool_put(&RTMASK_POOL, NonNull::from(mr).cast());
                            } else if mr.rm_flags.get() & RNF_NORMAL != 0 {
                                // don't progress because this a multipath route. Next route
                                // will use the same m.
                                mm = m;
                            }
                            m = mm;
                        }
                        xp = xn.rn_dupedkey.get();
                    }
                    if !m.is_null() {
                        log!(LOG_ERR, "rn_delete: Orphaned Mask {:p} at {:p}\n", m, xp);
                    }
                }
            }

            // We may be holding an active internal node in the tree. If so swap our internal
            // node (t) with the parent node (tp) since that one was just removed from the
            // tree.
            if !ptr::eq(tp, tt1) {
                rn_swap_nodes(tt1, tp);
            }

            // no rn_dupedkey list so no need to fixup multipath chains
        }

        // out:
        tt.rn_flags.set(tt.rn_flags.get() & !RNF_ACTIVE);
        tt1.rn_flags.set(tt1.rn_flags.get() & !RNF_ACTIVE);
        Some(tt)
    }
}

/// `rn_walktree`: calls `f` on every route of the tree (every leaf but the root leaves,
/// duplicated keys included), in key order, with the head's `rnh_rtableid`. The first error
/// `f` returns stops the walk and is returned. `f` may delete the leaf it is given.
pub fn rn_walktree(
    h: &RadixNodeHead,
    mut f: impl FnMut(&'static RadixNode, u32) -> Result<(), Errno>,
) -> Result<(), Errno> {
    // SAFETY: the tree invariant, which `rn_addroute`'s and `rn_delete`'s contracts maintain:
    // internal nodes have both children, every node a parent, and the right root leaf ends
    // the walk. The successor is found before `f` runs, so deleting the visited leaf is fine.
    unsafe {
        let mut rn = nd(h.rnh_treetop.get());

        // This gets complicated because we may delete the node while applying the function f
        // to it, so we need to calculate the successor node in advance.

        // First time through node, go left.
        while rn.rn_b.get() >= 0 {
            rn = nd(rn.rn_l.get());
        }
        loop {
            let mut base: *const RadixNode = rn;
            // If at right child go back up, otherwise, go right.
            while ptr::eq(nd(rn.rn_p.get()).rn_r.get(), rn) && rn.rn_flags.get() & RNF_ROOT == 0 {
                rn = nd(rn.rn_p.get());
            }
            // Find the next *leaf* since next node might vanish, too.
            rn = nd(nd(rn.rn_p.get()).rn_r.get());
            while rn.rn_b.get() >= 0 {
                rn = nd(rn.rn_l.get());
            }
            let next = rn;
            // Process leaves.
            while let Some(r) = base.as_ref() {
                base = r.rn_dupedkey.get();
                if r.rn_flags.get() & RNF_ROOT == 0 {
                    f(nd(r), h.rnh_rtableid.get())?;
                }
            }
            rn = next;
            if rn.rn_flags.get() & RNF_ROOT != 0 {
                return Ok(());
            }
        }
    }
}

/// `rn_initmask`: creates the shared mask tree, once.
fn rn_initmask() -> Result<(), Errno> {
    if !MASK_RNHEAD.load(Ordering::Relaxed).is_null() {
        return Ok(());
    }

    kassert!(MAX_KEYLEN.load(Ordering::Relaxed) > 0);

    let Some(p) = malloc(size_of::<RadixNodeHead>(), M_RTABLE, M_NOWAIT) else {
        return Err(Errno::ENOMEM);
    };
    let p = p.as_ptr().cast::<RadixNodeHead>();
    // SAFETY: a fresh block of a head's size, aligned (malloc's chunks are aligned to their
    // power-of-two size); it is the mask tree's for ever.
    let rnh: &'static RadixNodeHead = unsafe {
        p.write(RadixNodeHead::new());
        &*p
    };
    MASK_RNHEAD.store(p, Ordering::Relaxed);
    rn_inithead0(rnh, 0);
    Ok(())
}

/// `rn_inithead`: allocates and initialises a tree into `*head` unless it already has one;
/// `off` is the byte offset of the key inside the socket addresses (`offsetof(struct
/// sockaddr_in, sin_addr)`). `false` when memory ran out. The head is `free`d with
/// `M_RTABLE` and `size_of::<RadixNodeHead>()`, once its tree is empty.
pub fn rn_inithead(head: &mut Option<&'static RadixNodeHead>, off: i32) -> bool {
    if head.is_some() {
        return true;
    }

    if rn_initmask().is_err() {
        panic(format_args!("failed to initialize the mask tree"));
    }

    let Some(p) = malloc(size_of::<RadixNodeHead>(), M_RTABLE, M_NOWAIT) else {
        return false;
    };
    let p = p.as_ptr().cast::<RadixNodeHead>();
    // SAFETY: a fresh block of a head's size, aligned; the caller owns it from now on and
    // must not free it while it holds routes.
    let rnh: &'static RadixNodeHead = unsafe {
        p.write(RadixNodeHead::new());
        &*p
    };
    *head = Some(rnh);
    rn_inithead0(rnh, off);
    true
}

/// `rn_inithead0`: initialises `rnh` as an empty tree whose keys start `offset` bytes in.
fn rn_inithead0(rnh: &'static RadixNodeHead, offset: i32) {
    let off = offset * NBBY as i32;

    // memset(rnh, 0, sizeof(*rnh))
    rnh.rnh_treetop.set(ptr::null());
    rnh.rnh_addrsize.set(0);
    rnh.rnh_pktsize.set(0);
    for n in &rnh.rnh_nodes {
        n.assign(&RadixNode::new());
    }
    rnh.rnh_rtableid.set(0);

    let t = rn_newpair(RN_ZEROS.load(Ordering::Relaxed), off, &rnh.rnh_nodes[..2]);
    let ttt = &rnh.rnh_nodes[2];
    t.rn_r.set(ttt);
    t.rn_p.set(t);
    let tt = &rnh.rnh_nodes[0]; // t->rn_l
    tt.rn_flags.set(RNF_ROOT | RNF_ACTIVE);
    t.rn_flags.set(RNF_ROOT | RNF_ACTIVE);
    tt.rn_b.set((-1 - off) as i16);
    ttt.assign(tt);
    ttt.rn_key.set(RN_ONES.load(Ordering::Relaxed));
    rnh.rnh_treetop.set(t);
}

/// `rn_init`: sizes the all-zeros and all-ones keys for keys of up to `keylen` bytes, and
/// initialises the mask pool the first time. Can be called multiple times with a different
/// key length as long as no radix tree head has been allocated.
pub fn rn_init(keylen: u32) {
    kassert!(keylen as usize <= KEYLEN_LIMIT);

    let max_keylen = MAX_KEYLEN.load(Ordering::Relaxed);
    if max_keylen == 0 {
        pool_init(
            &RTMASK_POOL,
            size_of::<RadixMask>(),
            0,
            IPL_SOFTNET,
            0,
            "rtmask",
            None,
        );
    }

    if keylen <= max_keylen {
        return;
    }

    kassert!(MASK_RNHEAD.load(Ordering::Relaxed).is_null());

    if let Some(old) = NonNull::new(RN_ZEROS.load(Ordering::Relaxed)) {
        free(old, M_RTABLE, 2 * max_keylen as usize);
    }
    let Some(zeros) = mallocarray(2, keylen as usize, M_RTABLE, M_NOWAIT | M_ZERO) else {
        panic(format_args!(
            "cannot initialize a radix tree without memory"
        ));
    };
    let zeros = zeros.as_ptr();
    MAX_KEYLEN.store(keylen, Ordering::Relaxed);
    // SAFETY: the block is `2 * keylen` zeroed bytes; the second half becomes `rn_ones`.
    let ones = unsafe {
        let ones = zeros.add(keylen as usize);
        ptr::write_bytes(ones, 0xff, keylen as usize);
        ones
    };
    RN_ZEROS.store(zeros, Ordering::Relaxed);
    RN_ONES.store(ones, Ordering::Relaxed);
}

/// Host tests of other modules (the IPsec SPD, PF_KEY): the radix globals back to "never
/// initialised" after `setup_real_memory`, as the old ones point into the previous test's
/// memory. The next `rn_init` starts over.
#[cfg(test)]
pub(crate) fn rn_test_reset() {
    MAX_KEYLEN.store(0, Ordering::Relaxed);
    MASK_RNHEAD.store(ptr::null_mut(), Ordering::Relaxed);
    RN_ZEROS.store(ptr::null_mut(), Ordering::Relaxed);
    RN_ONES.store(ptr::null_mut(), Ordering::Relaxed);
}
/* </CODE> */

/* <TESTS> */
#[cfg(test)]
mod tests {
    // Host tests for the radix trees, as pf's tables use them: IPv4 keys laid out as
    // `sockaddr_in`s (length, family, port, address) in a tree at the address's offset, routes
    // added with and without masks, longest-prefix matches, exact lookups, duplicated keys,
    // deletion and walks.

    use std::boxed::Box;
    use std::sync::MutexGuard;
    use std::vec::Vec;

    use super::*;
    use crate::kern::subr_pool::tests::setup_real_memory;

    /// `sizeof(struct sockaddr_in)`.
    const SIN_LEN: u8 = 16;
    /// `offsetof(struct sockaddr_in, sin_addr)`.
    const SIN_ADDR_OFF: i32 = 4;
    /// `AF_INET`.
    const AF_INET: u8 = 2;

    /// Fresh memory, and the radix globals back to "never initialised" (the old ones point into
    /// the previous test's memory), then `rn_init` as `pfr_initialize` calls it
    /// (`sizeof(struct sockaddr_in6)`).
    fn setup() -> MutexGuard<'static, ()> {
        let guard = setup_real_memory();
        crate::machine::cons::consinit();
        MAX_KEYLEN.store(0, Ordering::Relaxed);
        MASK_RNHEAD.store(ptr::null_mut(), Ordering::Relaxed);
        RN_ZEROS.store(ptr::null_mut(), Ordering::Relaxed);
        RN_ONES.store(ptr::null_mut(), Ordering::Relaxed);
        rn_init(28);
        guard
    }

    /// A `sockaddr_in` for `a`, in a buffer longer than `max_keylen` (pf's `sockaddr_union`).
    fn sin(a: [u8; 4]) -> [u8; 32] {
        let mut k = [0u8; 32];
        k[0] = SIN_LEN;
        k[1] = AF_INET;
        k[4..8].copy_from_slice(&a);
        k
    }

    /// The mask of a `plen`-bit prefix, as `pfr_prepare_network` builds it.
    fn mask(plen: u32) -> [u8; 32] {
        let m = if plen == 0 {
            0
        } else {
            u32::MAX << (32 - plen)
        };
        sin(m.to_be_bytes())
    }

    /// What pf's `struct pfr_kentry` holds for the tree: the node pair and the key.
    struct Entry {
        nodes: [RadixNode; 2],
        key: [u8; 32],
    }

    fn entry(a: [u8; 4]) -> &'static Entry {
        Box::leak(Box::new(Entry {
            nodes: [RadixNode::new(), RadixNode::new()],
            key: sin(a),
        }))
    }

    fn head() -> &'static RadixNodeHead {
        let mut h = None;
        assert!(rn_inithead(&mut h, SIN_ADDR_OFF));
        let h = h.expect("a head");
        let mut again = Some(h);
        assert!(
            rn_inithead(&mut again, SIN_ADDR_OFF),
            "an existing head is kept"
        );
        assert!(again.is_some_and(|a| ptr::eq(a, h)));
        h
    }

    fn add(h: &RadixNodeHead, e: &'static Entry, plen: Option<u32>) -> Option<&'static RadixNode> {
        let m = plen.map(mask);
        let mp = m.as_ref().map_or(ptr::null(), |m| m.as_ptr());
        // SAFETY: the key and the nodes are leaked, the mask is read during the call only.
        unsafe { rn_addroute(e.key.as_ptr(), mp, h, &e.nodes, 0) }
    }

    fn del(h: &RadixNodeHead, e: &'static Entry, plen: Option<u32>) -> Option<&'static RadixNode> {
        let m = plen.map(mask);
        let mp = m.as_ref().map_or(ptr::null(), |m| m.as_ptr());
        // SAFETY: as in `add`.
        unsafe { rn_delete(e.key.as_ptr(), mp, h, None) }
    }

    fn matched(h: &RadixNodeHead, a: [u8; 4]) -> Option<&'static RadixNode> {
        let k = sin(a);
        // SAFETY: a local key, read during the call.
        unsafe { rn_match(k.as_ptr(), h) }
    }

    fn lookup(h: &RadixNodeHead, a: [u8; 4], plen: Option<u32>) -> Option<&'static RadixNode> {
        let k = sin(a);
        let m = plen.map(mask);
        let mp = m.as_ref().map_or(ptr::null(), |m| m.as_ptr());
        // SAFETY: local key and mask, read during the call.
        unsafe { rn_lookup(k.as_ptr(), mp, h) }
    }

    fn is(rn: Option<&RadixNode>, e: &Entry) -> bool {
        rn.is_some_and(|rn| ptr::eq(rn, &e.nodes[0]))
    }

    /// The address of a leaf's key.
    fn addr_of(rn: &RadixNode) -> [u8; 4] {
        let mut a = [0u8; 4];
        // SAFETY: leaves hold the tests' leaked 32-byte keys.
        a.copy_from_slice(unsafe { core::slice::from_raw_parts(rn.rn_key.get().add(4), 4) });
        a
    }

    #[test]
    fn masks_refine_and_order() {
        let (m8, m16, m24) = (mask(8), mask(16), mask(24));
        // SAFETY: local masks.
        unsafe {
            assert!(rn_refines(m16.as_ptr(), m8.as_ptr()), "/16 is inside /8");
            assert!(!rn_refines(m8.as_ptr(), m16.as_ptr()));
            assert!(!rn_refines(m16.as_ptr(), m16.as_ptr()), "equal masks");
            assert!(rn_refines(m24.as_ptr(), m16.as_ptr()));
            let odd = sin([255, 0, 255, 0]);
            assert!(!rn_refines(odd.as_ptr(), m16.as_ptr()));
            assert!(rn_lexobetter(m24.as_ptr(), m16.as_ptr()));
            assert!(!rn_lexobetter(m16.as_ptr(), m24.as_ptr()));
            assert!(!rn_lexobetter(m16.as_ptr(), m16.as_ptr()));
        }
    }

    #[test]
    fn longest_prefix_wins() {
        let _g = setup();
        let h = head();
        let net8 = entry([10, 0, 0, 0]);
        let net16 = entry([10, 1, 0, 0]);
        let net24 = entry([10, 1, 2, 0]);
        let host = entry([10, 1, 2, 3]);
        let other = entry([192, 168, 0, 0]);

        assert!(matched(h, [10, 1, 2, 3]).is_none(), "empty tree");
        assert!(is(add(h, net16, Some(16)), net16));
        assert!(is(add(h, net8, Some(8)), net8));
        assert!(is(add(h, host, None), host));
        assert!(is(add(h, net24, Some(24)), net24));
        assert!(is(add(h, other, Some(16)), other));

        assert!(is(matched(h, [10, 1, 2, 3]), host));
        assert!(is(matched(h, [10, 1, 2, 4]), net24));
        assert!(is(matched(h, [10, 1, 2, 255]), net24));
        assert!(is(matched(h, [10, 1, 3, 1]), net16));
        assert!(is(matched(h, [10, 1, 255, 255]), net16));
        assert!(is(matched(h, [10, 2, 0, 0]), net8));
        assert!(is(matched(h, [10, 255, 255, 255]), net8));
        assert!(is(matched(h, [192, 168, 77, 1]), other));
        assert!(matched(h, [192, 169, 0, 1]).is_none());
        assert!(matched(h, [11, 0, 0, 0]).is_none());
        assert!(
            matched(h, [0, 0, 0, 0]).is_none(),
            "the root leaf is not a route"
        );

        // A default route catches the rest; it is a duplicated key of the left root leaf.
        let dflt = entry([0, 0, 0, 0]);
        assert!(is(add(h, dflt, Some(0)), dflt));
        assert!(is(matched(h, [11, 0, 0, 0]), dflt));
        assert!(
            is(matched(h, [0, 0, 0, 0]), dflt),
            "explicitly asked for the default"
        );
        assert!(is(matched(h, [10, 1, 2, 3]), host));
    }

    #[test]
    fn lookup_wants_the_exact_mask() {
        let _g = setup();
        let h = head();
        let net8 = entry([10, 0, 0, 0]);
        let net16 = entry([10, 1, 0, 0]);
        let net24 = entry([10, 1, 2, 0]);
        add(h, net8, Some(8)).expect("added");
        add(h, net16, Some(16)).expect("added");
        add(h, net24, Some(24)).expect("added");

        assert!(is(lookup(h, [10, 1, 0, 0], Some(16)), net16));
        assert!(is(lookup(h, [10, 0, 0, 0], Some(8)), net8));
        assert!(is(lookup(h, [10, 1, 2, 0], Some(24)), net24));
        assert!(
            lookup(h, [10, 1, 0, 0], Some(24)).is_none(),
            "a known mask, but not on that key"
        );
        assert!(
            lookup(h, [10, 1, 0, 0], Some(20)).is_none(),
            "a mask the mask tree does not have"
        );
        assert!(
            is(lookup(h, [10, 1, 9, 9], None), net16),
            "no mask is rn_match"
        );
        assert!(lookup(h, [12, 0, 0, 0], None).is_none());
    }

    #[test]
    fn duplicated_keys_chain_most_specific_first() {
        let _g = setup();
        let h = head();
        let net16 = entry([10, 1, 0, 0]);
        assert!(is(add(h, net16, Some(16)), net16));

        let again = entry([10, 1, 0, 0]);
        assert!(add(h, again, Some(16)).is_none(), "same key and mask");

        let net24 = entry([10, 1, 0, 0]);
        let host = entry([10, 1, 0, 0]);
        let net8 = entry([10, 0, 0, 0]);
        let net8b = entry([10, 0, 0, 0]);
        assert!(is(add(h, net24, Some(24)), net24), "same key, longer mask");
        assert!(is(add(h, host, None), host), "same key, no mask");
        assert!(
            add(h, entry([10, 1, 0, 0]), None).is_none(),
            "a second host route"
        );
        assert!(is(add(h, net8, Some(8)), net8));
        assert!(add(h, net8b, Some(8)).is_none());

        // The chain starts with the host route, then the masks from the most specific.
        let mut chain = Vec::new();
        let mut rn = matched(h, [10, 1, 0, 0]);
        while let Some(n) = rn {
            chain.push(ptr::from_ref(n));
            // SAFETY: the chain's leaves are the tests' leaked nodes.
            rn = unsafe { n.rn_dupedkey.get().as_ref() };
        }
        assert_eq!(
            chain,
            [&host.nodes[0], &net24.nodes[0], &net16.nodes[0]].map(ptr::from_ref)
        );

        assert!(is(matched(h, [10, 1, 0, 0]), host));
        assert!(is(matched(h, [10, 1, 0, 7]), net24));
        assert!(is(matched(h, [10, 1, 7, 7]), net16));
        assert!(is(matched(h, [10, 7, 7, 7]), net8));
        assert!(is(lookup(h, [10, 1, 0, 0], Some(16)), net16));
        assert!(is(lookup(h, [10, 1, 0, 0], Some(24)), net24));

        // Deleting from the chain: the head (the leaf in the tree), then the rest.
        assert!(is(del(h, host, None), host));
        assert_eq!(host.nodes[0].rn_flags.get() & RNF_ACTIVE, 0);
        assert!(is(matched(h, [10, 1, 0, 0]), net24));
        assert!(is(del(h, net16, Some(16)), net16));
        assert!(is(matched(h, [10, 1, 7, 7]), net8));
        assert!(is(matched(h, [10, 1, 0, 9]), net24));
        assert!(is(del(h, net24, Some(24)), net24));
        assert!(is(matched(h, [10, 1, 0, 0]), net8));
        assert!(is(del(h, net8, Some(8)), net8));
        assert!(matched(h, [10, 1, 0, 0]).is_none());
        assert_eq!(walk(h).len(), 0);
    }

    #[test]
    fn delete_unlinks_and_returns_the_pair() {
        let _g = setup();
        let h = head();
        let net8 = entry([10, 0, 0, 0]);
        let net16 = entry([10, 1, 0, 0]);
        let net24 = entry([10, 1, 2, 0]);
        let host = entry([10, 1, 2, 3]);
        let far = entry([172, 16, 0, 0]);
        add(h, net8, Some(8)).expect("added");
        add(h, net16, Some(16)).expect("added");
        add(h, net24, Some(24)).expect("added");
        add(h, host, None).expect("added");
        add(h, far, Some(12)).expect("added");

        assert!(
            del(h, entry([10, 9, 9, 9]), None).is_none(),
            "not in the tree"
        );
        assert!(
            del(h, net16, Some(24)).is_none(),
            "the key with another mask"
        );
        assert!(del(h, net16, Some(20)).is_none(), "an unknown mask");

        assert!(is(del(h, net24, Some(24)), net24));
        assert_eq!(net24.nodes[0].rn_flags.get() & RNF_ACTIVE, 0);
        assert_eq!(net24.nodes[1].rn_flags.get() & RNF_ACTIVE, 0);
        assert!(is(matched(h, [10, 1, 2, 4]), net16));
        assert!(is(matched(h, [10, 1, 2, 3]), host));
        assert!(del(h, net24, Some(24)).is_none(), "already gone");

        assert!(is(del(h, net8, Some(8)), net8));
        assert!(matched(h, [10, 2, 0, 0]).is_none());
        assert!(is(matched(h, [10, 1, 9, 9]), net16));
        assert!(is(matched(h, [172, 31, 1, 1]), far));

        // The pair can go back in once zeroed, as pf's `bzero(ke->pfrke_node)` does.
        net24.nodes[0].assign(&RadixNode::new());
        net24.nodes[1].assign(&RadixNode::new());
        assert!(is(add(h, net24, Some(24)), net24));
        assert!(is(matched(h, [10, 1, 2, 4]), net24));

        for (e, plen) in [
            (host, None),
            (net16, Some(16)),
            (far, Some(12)),
            (net24, Some(24)),
        ] {
            assert!(is(del(h, e, plen), e));
        }
        assert!(walk(h).is_empty());
        assert!(matched(h, [10, 1, 2, 3]).is_none());
    }

    #[test]
    fn non_contiguous_masks_match_through_the_annotations() {
        let _g = setup();
        let h = head();
        let odd = entry([10, 0, 5, 0]);
        let near = entry([10, 0, 4, 0]);
        let m = sin([255, 0, 255, 0]);
        // SAFETY: leaked key and nodes; the mask is read during the calls only.
        unsafe {
            assert!(is(
                rn_addroute(odd.key.as_ptr(), m.as_ptr(), h, &odd.nodes, 0),
                odd
            ));
        }
        add(h, near, Some(24)).expect("added");
        assert!(is(matched(h, [10, 7, 5, 9]), odd));
        assert!(is(matched(h, [10, 0, 5, 1]), odd));
        assert!(is(matched(h, [10, 0, 4, 1]), near));
        assert!(matched(h, [10, 7, 6, 9]).is_none());
        assert!(matched(h, [11, 0, 5, 0]).is_none());
        // SAFETY: as above.
        unsafe {
            assert!(is(rn_lookup(odd.key.as_ptr(), m.as_ptr(), h), odd));
            assert!(is(rn_delete(odd.key.as_ptr(), m.as_ptr(), h, None), odd));
        }
        assert!(matched(h, [10, 7, 5, 9]).is_none());
    }

    /// The addresses `rn_walktree` visits, in order.
    fn walk(h: &RadixNodeHead) -> Vec<[u8; 4]> {
        let mut seen = Vec::new();
        let r = rn_walktree(h, |rn, id| {
            assert_eq!(id, h.rnh_rtableid.get());
            seen.push(addr_of(rn));
            Ok(())
        });
        assert_eq!(r, Ok(()));
        seen
    }

    #[test]
    fn walk_visits_every_leaf_in_key_order() {
        let _g = setup();
        let h = head();
        assert!(walk(h).is_empty(), "the root leaves are not routes");
        h.rnh_rtableid.set(3);

        let addrs = [
            ([192, 168, 1, 0], Some(24)),
            ([10, 0, 0, 0], Some(8)),
            ([10, 1, 0, 0], Some(16)),
            ([10, 1, 0, 0], Some(24)),
            ([10, 1, 2, 3], None),
            ([0, 0, 0, 0], Some(0)),
            ([255, 255, 255, 255], None),
            ([172, 16, 0, 0], Some(12)),
        ];
        let entries: Vec<&'static Entry> = addrs
            .iter()
            .map(|&(a, plen)| {
                let e = entry(a);
                assert!(is(add(h, e, plen), e));
                e
            })
            .collect();

        let seen = walk(h);
        assert_eq!(seen.len(), addrs.len(), "duplicated keys included");
        let mut sorted = seen.clone();
        sorted.sort_unstable();
        assert_eq!(seen, sorted, "in key order");
        for (a, _) in addrs {
            assert!(seen.contains(&a));
        }

        // The first error stops the walk and comes back.
        let mut calls = 0;
        let r = rn_walktree(h, |_, _| {
            calls += 1;
            if calls == 3 {
                Err(Errno::EINTR)
            } else {
                Ok(())
            }
        });
        assert_eq!(r, Err(Errno::EINTR));
        assert_eq!(calls, 3);

        // The function may delete the leaf it is given.
        let r = rn_walktree(h, |rn, _| {
            let (&e, &(_, plen)) = entries
                .iter()
                .zip(addrs.iter())
                .find(|(e, _)| ptr::eq(rn, &e.nodes[0]))
                .expect("one of ours");
            assert!(is(del(h, e, plen), e));
            Ok(())
        });
        assert_eq!(r, Ok(()));
        assert!(walk(h).is_empty());
    }
}
/* </TESTS> */
