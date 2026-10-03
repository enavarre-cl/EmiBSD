/* $OpenBSD: art.h,v 1.29 2026/04/23 01:28:03 jsg Exp $ */
/*	$OpenBSD: art.c,v 1.36 2026/04/23 01:28:03 jsg Exp $ */
/* <LICENSES> */
/*
 * Copyright (c) 2015 Martin Pieuchot
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
 * Copyright (c) 2015 Martin Pieuchot
 * Copyright (c) 2001 Yoichi Hariguchi
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

//! The Allotment Routing Table (ART): `<net/art.h>` and `net/art.c`.
//!
//! Upstream: sys/net/art.h @ 3ce1f3f79392
//! Upstream: sys/net/art.c @ 3ce1f3f79392
//!
//! Yoichi Hariguchi's paper can be found at <http://www.hariguchi.org/art/art.pdf>. This
//! implementation is tweaked to minimise pointer traversal during lookups by only accessing
//! the heaps; it avoids following pointers to or through the [`ArtTable`] structures.
//!
//! A heap is an array of [`ArtHeapEntry`] words whose meaning depends on where they are. Most
//! entries point at nodes ([`ArtNode`], leaf entries) or at the next heap to traverse; the two
//! kinds are tagged by the low bit (a heap pointer has it set) and masked before use. The first
//! two entries are not used by the search: `heap[0]` points at the [`ArtTable`] of this part of
//! the tree, and `heap[1]` holds the node that would be in the parent heap if this table were
//! not using its slot, the default entry (route) of the table. Nodes hold the exact prefix they
//! represent (address and prefix length) and a pointer to the data associated with it
//! (`an_value`):
//!
//! ```text
//!   struct art --art_root--> heap --heap[0]--> struct art_table --at_parent--> NULL
//!                             |   <--at_heap--
//!                             | heap entry
//!                             v
//!                            heap --heap[0]--> struct art_table --at_parent--> (above)
//!                             | node entry
//!                             v
//!                     struct art_node --an_value--> "user" data
//! ```
//!
//! Locking: modification (`art_insert`, `art_delete`) and iteration (`art_iter_next`, ...)
//! over the ART must be serialised by the caller. Lookups (`art_match`, `art_lookup`) run
//! within an SMR critical section in C. Iteration manipulates the reference counts of the
//! tables it traverses and keeps them until it runs out of entries, so a caller may release its
//! locks between `art_iter_open` and `art_iter_next`; `art_iter_close` drops them early. The
//! iterator holds no reference on the nodes or on the data hanging off `an_value`.
//!
//! Status: `ported` (M7b).
//!
//! ## Deviations
//! - SMR (`kern/kern_smr.c`) is not ported. `SMR_PTR_GET`/`SMR_PTR_SET_LOCKED` are plain
//!   loads and stores of `Cell`s, `SMR_ASSERT_CRITICAL` is nothing and `smr_barrier` in the
//!   garbage collectors is empty: on one CPU with a kernel that is not preempted, no reader is
//!   inside a lookup when the collector task runs, as `net/if_.rs` argues for its index map.
//! - A heap is reached through a raw pointer to its first word (`*const Cell<usize>`): its
//!   size is `1 << (bits + 1)` words, which only its table knows, and the lookups do not read
//!   the table (the C's point). The slot accessor is `unsafe` with that bound as its contract;
//!   every index comes from [`art_bindex`], which stays below the size by construction.
//! - The `an__u` union (`an_gc` for the garbage collector, `an_addr` for the prefix) is two
//!   fields side by side (`docs/C_TO_RUST.md`): the collector's link is written after the
//!   node left the tree, when nothing reads the address any more.
//! - `struct art`'s `art_levels` is a slice (`art_nlevels` is kept beside it, as in C);
//!   `art_alloc` writes an initialised `Art` into the `malloc`ed block instead of relying on
//!   `M_ZERO`, because an empty slice is not all-zero bits.
//! - Addresses are byte slices (`&[u8]`) holding at least the address length's bytes; the C
//!   takes `const void *` into a socket address.
//! - `art_allot`, which the C writes as Knuth's `goto` expansion of a double recursion,
//!   is the recursion: depth is at most a table's stride (8). It descends into a non-fringe
//!   slot only when the slot holds the replaced entry, as the expansion does.
//! - `art_table_get`'s `j == -1` for the root is `u32::MAX` (the C's `unsigned int` -1).

use core::cell::Cell;
use core::ffi::c_void;
use core::mem::size_of;
use core::ptr::{self, NonNull};
use core::sync::atomic::{AtomicPtr, Ordering};

use crate::kassert;
use crate::kern::kern_lock::{mtx_enter, mtx_leave};
use crate::kern::kern_malloc::malloc;
use crate::kern::kern_task::{SYSTQMP, task_add};
use crate::kern::subr_pool::{POOL_ALLOCATOR_SINGLE, pool_get, pool_init, pool_put};
use crate::kern::subr_prf::panic;
use crate::machine::intr::IPL_SOFTNET;
use crate::sys::malloc::{M_NOWAIT, M_RTABLE, M_ZERO};
use crate::sys::mutex::Mutex;
use crate::sys::pool::{PR_NOWAIT, PR_ZERO, Pool};
use crate::sys::task::Task;

/// `art_heap_entry`: a word of a heap, a node pointer or a tagged heap pointer.
pub type ArtHeapEntry = usize;

/// A heap: the address of its first word (see the module's deviations).
pub type ArtHeap = *const Cell<ArtHeapEntry>;

/// `ART_HEAP_IDX_TABLE`: `heap[0]`, the table of the heap.
pub const ART_HEAP_IDX_TABLE: usize = 0;
/// `ART_HEAP_IDX_DEFAULT`: `heap[1]`, the default entry of the table.
pub const ART_HEAP_IDX_DEFAULT: usize = 1;

/// `AT_HEAPSIZE(bits)`: the size in bytes of the heap of a table of stride `bits`.
pub const fn at_heapsize(bits: u32) -> usize {
    (1 << (bits + 1)) * size_of::<ArtHeapEntry>()
}

/// The table of an `alen`-bit tree is in levels of these strides: IPv4.
const ART_PLEN32_LEVELS: [u32; 7] = [8, 4, 4, 4, 4, 4, 4];

/// IPv6.
const ART_PLEN128_LEVELS: [u32; 32] = [4; 32];

/// MPLS labels.
const ART_PLEN20_LEVELS: [u32; 5] = [4, 4, 4, 4, 4];

/// `struct art`: the root of the ART, equivalent to the radix head.
pub struct Art {
    /// `art_root`: the root heap, NULL while the tree is empty.
    pub art_root: Cell<ArtHeap>,
    /// `art_levels`: the stride of each level.
    pub art_levels: Cell<&'static [u32]>,
    /// `art_nlevels`: the number of levels.
    pub art_nlevels: Cell<u32>,
    /// `art_alen`: the address length in bits.
    pub art_alen: Cell<u32>,
}

impl Art {
    /// An empty tree with no levels, for `art_init` to configure.
    pub const fn new() -> Self {
        Self {
            art_root: Cell::new(ptr::null()),
            art_levels: Cell::new(&[]),
            art_nlevels: Cell::new(0),
            art_alen: Cell::new(0),
        }
    }
}

impl Default for Art {
    fn default() -> Self {
        Self::new()
    }
}

// SAFETY: the tree is changed by callers that serialise themselves (the rtable lock), as the
// C's locking comment requires; lookups only load words.
unsafe impl Sync for Art {}

/// `struct art_table`: an allotment table. All-zero is a valid value (`PR_ZERO`).
pub struct ArtTable {
    /// `at_heap`.
    pub at_heap: Cell<ArtHeap>,
    /// `at_parent`: parent table.
    pub at_parent: Cell<*const ArtTable>,
    /// `at_index`: index in the parent table.
    pub at_index: Cell<u32>,
    /// `at_minfringe`: index that fringe begins.
    pub at_minfringe: Cell<u32>,
    /// `at_level`: level of the table.
    pub at_level: Cell<u32>,
    /// `at_bits`: stride length of the table.
    pub at_bits: Cell<u32>,
    /// `at_offset`: sum of parents' stride len.
    pub at_offset: Cell<u32>,
    /// `at_refcnt`.
    pub at_refcnt: Cell<u32>,
}

/// `struct art_node`: the internal representation of a route entry. All-zero is a valid
/// value (`PR_ZERO`).
pub struct ArtNode {
    /// `an_value`: the data of the prefix (the routing table's list of `rtentry`s).
    pub an_value: Cell<*mut c_void>,
    /// `an_gc` (`an__u.an__gc`): the garbage collector's link, after `art_put`.
    pub an_gc: Cell<*const ArtNode>,
    /// `an_addr` (`an__u.an__addr`): the prefix.
    pub an_addr: Cell<[u8; 16]>,
    /// `an_plen`: the prefix length in bits.
    pub an_plen: Cell<u32>,
}

/// `struct art_iter`: an iteration over the nodes of a tree.
pub struct ArtIter {
    /// `ai_art`.
    pub ai_art: *const Art,
    /// `ai_table`: the table being walked, NULL when done.
    pub ai_table: *const ArtTable,
    /// `ai_j`.
    pub ai_j: u32,
    /// `ai_i`.
    pub ai_i: u32,
}

impl ArtIter {
    /// An iterator before `art_iter_open`.
    pub const fn new() -> Self {
        Self {
            ai_art: ptr::null(),
            ai_table: ptr::null(),
            ai_j: 0,
            ai_i: 0,
        }
    }
}

impl Default for ArtIter {
    fn default() -> Self {
        Self::new()
    }
}

/// A word of a static's garbage list, protected by its mutex.
type GcList<T> = AtomicPtr<T>;

static AN_POOL: Pool = Pool::new();
static AT_POOL: Pool = Pool::new();
static AT_HEAP_4_POOL: Pool = Pool::new();
static AT_HEAP_8_POOL: Pool = Pool::new();

/// `art_table_gc_list`. Protected by: `ART_TABLE_GC_MTX`.
static ART_TABLE_GC_LIST: GcList<ArtTable> = AtomicPtr::new(ptr::null_mut());
static ART_TABLE_GC_MTX: Mutex = Mutex::new(IPL_SOFTNET);
static ART_TABLE_GC_TASK: Task = Task::new(art_table_gc, ptr::null_mut());

/// `art_node_gc_list`. Protected by: `ART_NODE_GC_MTX`.
static ART_NODE_GC_LIST: GcList<ArtNode> = AtomicPtr::new(ptr::null_mut());
static ART_NODE_GC_MTX: Mutex = Mutex::new(IPL_SOFTNET);
static ART_NODE_GC_TASK: Task = Task::new(art_gc, ptr::null_mut());

/// `heap[i]`.
///
/// # Safety
///
/// `heap` is a live heap and `i` is below its size, `1 << (bits + 1)` for its table's stride.
unsafe fn heap_slot<'a>(heap: ArtHeap, i: usize) -> &'a Cell<ArtHeapEntry> {
    // SAFETY: the caller's contract.
    unsafe { &*heap.add(i) }
}

/// `art_heap_to_table(heap)`: the table a heap belongs to.
///
/// # Safety
///
/// `heap` is a live heap (its first word was set by `art_table_get`).
pub unsafe fn art_heap_to_table<'a>(heap: ArtHeap) -> &'a ArtTable {
    // SAFETY: the caller's contract; `heap[0]` holds the table, which lives as long as its
    // heap.
    unsafe { &*(heap_slot(heap, ART_HEAP_IDX_TABLE).get() as *const ArtTable) }
}

/// `art_heap_entry_is_node(ahe)`: the entry is a node pointer (NULL is a node).
pub const fn art_heap_entry_is_node(ahe: ArtHeapEntry) -> bool {
    ahe & 1 == 0
}

/// `art_heap_entry_to_node(ahe)`: the node of a node entry, `None` for NULL.
///
/// # Safety
///
/// `ahe` is a node entry of a live tree: zero, or the address of a node that `art_put` has
/// not given to the collector yet.
pub unsafe fn art_heap_entry_to_node(ahe: ArtHeapEntry) -> Option<&'static ArtNode> {
    // SAFETY: the caller's contract; nodes are pool items that stay until `art_gc`.
    unsafe { (ahe as *const ArtNode).as_ref() }
}

/// `art_heap_entry_to_heap(ahe)`: the heap of a heap entry.
pub const fn art_heap_entry_to_heap(ahe: ArtHeapEntry) -> ArtHeap {
    (ahe & !1) as ArtHeap
}

/// `art_node_to_heap_entry(an)`.
pub fn art_node_to_heap_entry(an: &ArtNode) -> ArtHeapEntry {
    ptr::from_ref(an) as ArtHeapEntry
}

/// `art_heap_to_heap_entry(heap)`.
pub fn art_heap_to_heap_entry(heap: ArtHeap) -> ArtHeapEntry {
    heap as ArtHeapEntry | 1
}

/// `ART_FOREACH(an, art, ai)`: calls `f` on every node of the tree, in the iterator's order.
/// `f` returns `false` to stop early, which closes the iterator as the C's callers do.
pub fn art_foreach(art: &Art, ai: &mut ArtIter, mut f: impl FnMut(&'static ArtNode) -> bool) {
    let mut an = art_iter_open(art, ai);
    while let Some(node) = an {
        if !f(node) {
            art_iter_close(ai);
            return;
        }
        an = art_iter_next(ai);
    }
}

/// `art_boot`: initialises the pools.
pub fn art_boot() {
    pool_init(
        &AN_POOL,
        size_of::<ArtNode>(),
        0,
        IPL_SOFTNET,
        0,
        "art_node",
        None,
    );
    pool_init(
        &AT_POOL,
        size_of::<ArtTable>(),
        0,
        IPL_SOFTNET,
        0,
        "art_table",
        None,
    );
    pool_init(
        &AT_HEAP_4_POOL,
        at_heapsize(4),
        0,
        IPL_SOFTNET,
        0,
        "art_heap4",
        None,
    );
    pool_init(
        &AT_HEAP_8_POOL,
        at_heapsize(8),
        0,
        IPL_SOFTNET,
        0,
        "art_heap8",
        Some(&POOL_ALLOCATOR_SINGLE),
    );
}

/// `art_init`: per routing table initialization, for an address of `alen` bits.
pub fn art_init(art: &Art, alen: u32) {
    let levels: &'static [u32] = match alen {
        32 => &ART_PLEN32_LEVELS,
        128 => &ART_PLEN128_LEVELS,
        20 => &ART_PLEN20_LEVELS,
        _ => panic(format_args!("no configuration for alen {alen}")),
    };

    #[cfg(feature = "diagnostic")]
    {
        let bits: u32 = levels.iter().sum();
        if alen != bits {
            panic(format_args!("sum of levels {bits} != address len {alen}"));
        }
    }

    art.art_root.set(ptr::null());
    art.art_levels.set(levels);
    art.art_nlevels.set(levels.len() as u32);
    art.art_alen.set(alen);
}

/// `art_alloc`: a new tree for addresses of `alen` bits, `None` without memory.
pub fn art_alloc(alen: u32) -> Option<&'static Art> {
    let p = malloc(size_of::<Art>(), M_RTABLE, M_NOWAIT | M_ZERO)?.cast::<Art>();
    // SAFETY: `malloc` returned a block of `size_of::<Art>()` bytes, aligned for any object of
    // that size; writing an initialised value makes it a valid `Art` that lives until the
    // `free` the C never does (routing tables are never freed).
    let art = unsafe {
        p.as_ptr().write(Art::new());
        &*p.as_ptr()
    };
    art_init(art, alen);
    Some(art)
}

/// `art_bindex`: the base index of the part of `addr` and `plen` covered by the table at
/// `offset` with stride `bits`.
///
/// In other words, this takes the multi-level (complete) address `addr` and prefix length
/// `plen` and returns the single level base index for the table. With an address of 32 bits
/// divided into four 8-bit tables, there are at most 4 base indexes if the prefix length is
/// over 24.
fn art_bindex(offset: u32, bits: u32, addr: &[u8], plen: u32) -> u32 {
    kassert!(plen >= offset);
    kassert!(plen <= offset + bits);

    // We are only interested in the part of the prefix length corresponding to the range of
    // this table.
    let plen = plen - offset;

    // Jump to the first byte of the address containing bits covered by this table.
    let a = &addr[(offset / 8) as usize..];

    // The table covers the bit range between `boff` and `bend`.
    let boff = offset % 8;
    let bend = bits + boff;

    kassert!(bend <= 32);

    let first = |b: u8| u32::from(b) & ((1 << (8 - boff)) - 1);
    let k = if bend > 24 {
        (first(a[0]) << (bend - 8))
            | (u32::from(a[1]) << (bend - 16))
            | (u32::from(a[2]) << (bend - 24))
            | (u32::from(a[3]) >> (32 - bend))
    } else if bend > 16 {
        (first(a[0]) << (bend - 8))
            | (u32::from(a[1]) << (bend - 16))
            | (u32::from(a[2]) >> (24 - bend))
    } else if bend > 8 {
        (first(a[0]) << (bend - 8)) | (u32::from(a[1]) >> (16 - bend))
    } else {
        (u32::from(a[0]) >> (8 - bend)) & ((1 << bits) - 1)
    };

    // Single level base index formula:
    (k >> (bits - plen)) + (1 << plen)
}

/// `art_match`: (non-perfect) lookup; the best existing match for a destination.
pub fn art_match(art: &Art, addr: &[u8]) -> Option<&'static ArtNode> {
    let mut offset = 0;
    let mut level = 0usize;
    let mut dahe: ArtHeapEntry = 0;

    // SMR_ASSERT_CRITICAL(): no SMR (the module's deviations).

    let mut heap = art.art_root.get();
    if heap.is_null() {
        return None;
    }

    let levels = art.art_levels.get();
    // Iterate until we find a leaf.
    let ahe = loop {
        let bits = levels[level];
        let p = offset + bits;

        // Remember the default route of each table we visit in case we do not find a better
        // matching route.
        // SAFETY: `heap` is a live heap of this tree with stride `bits`; slot 1 exists in
        // every heap.
        let ahe = unsafe { heap_slot(heap, ART_HEAP_IDX_DEFAULT) }.get();
        if ahe != 0 {
            dahe = ahe;
        }

        // Do a single level route lookup.
        let j = art_bindex(offset, bits, addr, p) as usize;
        // SAFETY: as above; `art_bindex`'s result is below `1 << (bits + 1)`.
        let ahe = unsafe { heap_slot(heap, j) }.get();

        // If this is a leaf (NULL is a leaf) we're done.
        if art_heap_entry_is_node(ahe) {
            break ahe;
        }

        heap = art_heap_entry_to_heap(ahe);
        offset = p;
        level += 1;

        kassert!(level < art.art_nlevels.get() as usize);
    };

    // SAFETY: both are node entries of this live tree.
    unsafe { art_heap_entry_to_node(ahe).or_else(|| art_heap_entry_to_node(dahe)) }
}

/// `art_node_check`: whether node `an` is exactly the prefix `addr`/`plen`.
fn art_node_check(an: &ArtNode, addr: &[u8], plen: u32) -> bool {
    if an.an_plen.get() != plen {
        return false;
    }

    let an_addr = an.an_addr.get();
    let whole = (plen / 8) as usize;
    if an_addr[..whole] != addr[..whole] {
        return false;
    }
    let rest = plen % 8;
    if rest == 0 {
        return true;
    }

    let shift = 8 - rest;
    (an_addr[whole] >> shift) == (addr[whole] >> shift)
}

/// `art_lookup`: perfect lookup; the node of exactly `addr`/`plen`, if it exists.
pub fn art_lookup(art: &Art, addr: &[u8], plen: u32) -> Option<&'static ArtNode> {
    let mut offset = 0;
    let mut level = 0usize;

    kassert!(plen <= art.art_alen.get());

    // SMR_ASSERT_CRITICAL(): no SMR.

    let mut heap = art.art_root.get();
    if heap.is_null() {
        return None;
    }

    // Default route
    if plen == 0 {
        // SAFETY: the root heap is live; slot 1 exists in every heap.
        let ahe = unsafe { heap_slot(heap, ART_HEAP_IDX_DEFAULT) }.get();
        // SAFETY: a node entry of this tree.
        return unsafe { art_heap_entry_to_node(ahe) };
    }

    let levels = art.art_levels.get();
    // If the prefix length is smaller than the sum of the stride length at this level the
    // entry must be in the current table.
    let bits = loop {
        let bits = levels[level];
        let p = offset + bits;
        if plen <= p {
            break bits;
        }

        // Do a single level route lookup.
        let j = art_bindex(offset, bits, addr, p) as usize;
        // SAFETY: `heap` has stride `bits` and `j` is a base index below its size.
        let ahe = unsafe { heap_slot(heap, j) }.get();

        // A leaf is a match, but not a perfect one, or NULL
        if art_heap_entry_is_node(ahe) {
            return None;
        }

        heap = art_heap_entry_to_heap(ahe);
        offset = p;
        level += 1;

        kassert!(level < art.art_nlevels.get() as usize);
    };

    let i = art_bindex(offset, bits, addr, plen) as usize;
    // SAFETY: as in the loop.
    let mut ahe = unsafe { heap_slot(heap, i) }.get();
    if !art_heap_entry_is_node(ahe) {
        heap = art_heap_entry_to_heap(ahe);
        // SAFETY: a child heap of this tree; slot 1 exists in every heap.
        ahe = unsafe { heap_slot(heap, ART_HEAP_IDX_DEFAULT) }.get();
    }

    // Make sure we've got a perfect match
    // SAFETY: a node entry of this tree.
    let an = unsafe { art_heap_entry_to_node(ahe) }?;
    if art_node_check(an, addr, plen) {
        return Some(an);
    }

    None
}

/// `art_is_empty`.
pub fn art_is_empty(art: &Art) -> bool {
    art.art_root.get().is_null()
}

/// `art_insert`: insertion. Inserts node `an`, or returns an existing node with the same
/// destination and prefix length; `None` without memory.
pub fn art_insert(art: &Art, an: &'static ArtNode) -> Option<&'static ArtNode> {
    let plen = an.an_plen.get();
    let addr = an.an_addr.get();

    kassert!(plen <= art.art_alen.get());

    let mut heap = art.art_root.get();
    let mut at: &ArtTable;
    if heap.is_null() {
        at = art_table_get(art, None, u32::MAX)?;
        heap = at.at_heap.get();
        art.art_root.set(heap);
    } else {
        // SAFETY: the root heap is live.
        at = unsafe { art_heap_to_table(heap) };
    }

    // Default route
    if plen == 0 {
        // SAFETY: slot 1 exists in every heap.
        let ahep = unsafe { heap_slot(heap, ART_HEAP_IDX_DEFAULT) };
        // SAFETY: a node entry of this tree.
        if let Some(oan) = unsafe { art_heap_entry_to_node(ahep.get()) } {
            return Some(oan);
        }

        art_table_ref(art, at);
        ahep.set(art_node_to_heap_entry(an));
        return Some(an);
    }

    // If the prefix length is smaller than the sum of the stride length at this level the
    // entry must be in the current table.
    loop {
        let p = at.at_offset.get() + at.at_bits.get();
        if p >= plen {
            break;
        }
        // Do a single level route lookup.
        let j = art_bindex(at.at_offset.get(), at.at_bits.get(), &addr, p);
        // SAFETY: `heap` is `at`'s heap and `j` a base index of its stride.
        let ahep = unsafe { heap_slot(heap, j as usize) };
        let ahe = ahep.get();

        // If the node corresponding to the fringe index is a leaf we need to allocate a
        // subtable. The route entry of this node will then become the default route of the
        // subtable.
        if art_heap_entry_is_node(ahe) {
            let child = art_table_get(art, Some(at), j)?;

            art_table_ref(art, at);

            at = child;
            heap = at.at_heap.get();
            // SAFETY: the new heap is live; slot 1 exists in every heap.
            unsafe { heap_slot(heap, ART_HEAP_IDX_DEFAULT) }.set(ahe);
            ahep.set(art_heap_to_heap_entry(heap));
        } else {
            heap = art_heap_entry_to_heap(ahe);
            // SAFETY: a child heap of this tree.
            at = unsafe { art_heap_to_table(heap) };
        }
    }

    let i = art_bindex(at.at_offset.get(), at.at_bits.get(), &addr, plen);
    // SAFETY: `heap` is `at`'s heap and `i` a base index of its stride.
    let mut ahep = unsafe { heap_slot(heap, i as usize) };
    let mut oahe = ahep.get();
    if !art_heap_entry_is_node(oahe) {
        heap = art_heap_entry_to_heap(oahe);
        // SAFETY: a child heap of this tree; slot 1 exists in every heap.
        ahep = unsafe { heap_slot(heap, ART_HEAP_IDX_DEFAULT) };
        oahe = ahep.get();
    }

    // Check if there's an existing node
    // SAFETY: a node entry of this tree.
    if let Some(oan) = unsafe { art_heap_entry_to_node(oahe) }
        && art_node_check(oan, &addr, plen)
    {
        return Some(oan);
    }

    // If the index `i' of the route that we are inserting is not a fringe index, we need to
    // allot this new route pointer to all the corresponding fringe indices.
    art_table_ref(art, at);
    let ahe = art_node_to_heap_entry(an);
    if i < at.at_minfringe.get() {
        art_allot(at, i, oahe, ahe);
    } else {
        ahep.set(ahe);
    }

    Some(an)
}

/// `art_delete`: deletion. Removes and returns the node of `addr`/`plen`, if any.
pub fn art_delete(art: &Art, addr: &[u8], plen: u32) -> Option<&'static ArtNode> {
    kassert!(plen <= art.art_alen.get());

    let mut heap = art.art_root.get();
    if heap.is_null() {
        return None;
    }

    // SAFETY: the root heap is live.
    let mut at = unsafe { art_heap_to_table(heap) };

    // Default route
    if plen == 0 {
        // SAFETY: slot 1 exists in every heap.
        let ahep = unsafe { heap_slot(heap, ART_HEAP_IDX_DEFAULT) };

        // SAFETY: a node entry of this tree.
        let an = unsafe { art_heap_entry_to_node(ahep.get()) }?;

        ahep.set(0);
        art_table_free(art, Some(at));

        return Some(an);
    }

    // If the prefix length is smaller than the sum of the stride length at this level the
    // entry must be in the current table.
    loop {
        let p = at.at_offset.get() + at.at_bits.get();
        if p >= plen {
            break;
        }
        // Do a single level route lookup.
        let j = art_bindex(at.at_offset.get(), at.at_bits.get(), addr, p);
        // SAFETY: `heap` is `at`'s heap and `j` a base index of its stride.
        let ahe = unsafe { heap_slot(heap, j as usize) }.get();

        // If this is a leaf, there is no route to delete.
        if art_heap_entry_is_node(ahe) {
            return None;
        }

        heap = art_heap_entry_to_heap(ahe);
        // SAFETY: a child heap of this tree.
        at = unsafe { art_heap_to_table(heap) };
    }

    let i = art_bindex(at.at_offset.get(), at.at_bits.get(), addr, plen);
    // SAFETY: `heap` is `at`'s heap and `i` a base index of its stride.
    let mut ahep = unsafe { heap_slot(heap, i as usize) };
    let mut ahe = ahep.get();
    if !art_heap_entry_is_node(ahe) {
        let nheap = art_heap_entry_to_heap(ahe);
        // SAFETY: a child heap of this tree; slot 1 exists in every heap.
        ahep = unsafe { heap_slot(nheap, ART_HEAP_IDX_DEFAULT) };
        ahe = ahep.get();
    }

    // SAFETY: a node entry of this tree.
    let an = unsafe { art_heap_entry_to_node(ahe) }?; // No route to delete

    // Get the next most specific route for the index `i'.
    let j = i >> 1;
    let pahe = if j > 1 {
        // SAFETY: `j` is a smaller index of the same heap.
        unsafe { heap_slot(heap, j as usize) }.get()
    } else {
        0
    };

    // If the index `i' of the route that we are removing is not a fringe index, we need to
    // allot the next most specific route pointer to all the corresponding fringe indices.
    if i < at.at_minfringe.get() {
        art_allot(at, i, ahe, pahe);
    } else {
        ahep.set(pahe);
    }

    // We have removed an entry from this table.
    art_table_free(art, Some(at));

    Some(an)
}

/// `art_table_ref`.
pub fn art_table_ref<'a>(_art: &Art, at: &'a ArtTable) -> &'a ArtTable {
    at.at_refcnt.set(at.at_refcnt.get() + 1);
    at
}

/// `art_table_rele`: drops a reference; `true` when it was the last.
fn art_table_rele(at: Option<&ArtTable>) -> bool {
    let Some(at) = at else {
        return false;
    };

    let refs = at.at_refcnt.get() - 1;
    at.at_refcnt.set(refs);
    refs == 0
}

/// `art_table_free`: drops a reference on `at`; with the last one, garbage collects the table
/// and all its parents that are empty. `true` when `at` went away.
pub fn art_table_free(art: &Art, at: Option<&ArtTable>) -> bool {
    if art_table_rele(at) {
        let mut at = at;
        // Garbage collect this table and all its parents that are empty.
        while let Some(t) = at {
            at = art_table_put(art, t);
            if !art_table_rele(at) {
                break;
            }
        }

        return true;
    }

    false
}

/// `art_iter_descend`: enters the child heap `heap` whose slot in the parent held `pahe`;
/// returns the table's default route if it is not the parent's.
fn art_iter_descend(
    ai: &mut ArtIter,
    heap: ArtHeap,
    pahe: ArtHeapEntry,
) -> Option<&'static ArtNode> {
    // SAFETY: `heap` is a live heap of the iterated tree.
    let at = unsafe { art_heap_to_table(heap) };
    // SAFETY: `ai_art` was set by `art_iter_open` to a tree that outlives the iteration.
    let art = unsafe { &*ai.ai_art };
    ai.ai_table = art_table_ref(art, at);

    // Start looking at non-fringe nodes.
    ai.ai_j = 1;

    // The default route (index 1) is processed by the parent table (where it belongs)
    // otherwise it could be processed more than once.
    ai.ai_i = 2;

    // Process the default route now.
    // SAFETY: slot 1 exists in every heap.
    let ahe = unsafe { heap_slot(heap, ART_HEAP_IDX_DEFAULT) }.get();
    if ahe != 0 && ahe != pahe {
        // SAFETY: a node entry of this tree.
        return unsafe { art_heap_entry_to_node(ahe) };
    }

    // Tell the caller to proceed with art_iter_next.
    None
}

/// `art_iter_open`: starts iterating over `art`, returning its first node.
pub fn art_iter_open(art: &Art, ai: &mut ArtIter) -> Option<&'static ArtNode> {
    let heap = art.art_root.get();

    ai.ai_art = art;

    if heap.is_null() {
        // empty, we're already done
        ai.ai_table = ptr::null();
        return None;
    }

    if let Some(an) = art_iter_descend(ai, heap, 0) {
        return Some(an); // default route
    }

    art_iter_next(ai)
}

/// `art_iter_next`: the next node, `None` (and every reference dropped) at the end.
pub fn art_iter_next(ai: &mut ArtIter) -> Option<&'static ArtNode> {
    // SAFETY: an open iterator holds a reference on `ai_table`, which keeps it alive.
    let mut at: &ArtTable = unsafe { &*ai.ai_table };
    let mut heap = at.at_heap.get();
    // SAFETY: set by `art_iter_open`; the tree outlives the iteration.
    let art = unsafe { &*ai.ai_art };

    // The C's `descend:` label: entered at the start and after moving into a child table.
    'descend: loop {
        let minfringe = at.at_minfringe.get();
        // Iterate non-fringe nodes in ``natural'' order.
        if ai.ai_j < minfringe {
            loop {
                loop {
                    let i = ai.ai_i;
                    if i >= minfringe {
                        break;
                    }
                    ai.ai_i = i << 1;

                    // SAFETY: `i` and `i >> 1` are below `minfringe`, inside `at`'s heap.
                    let (pahe, ahe) = unsafe {
                        (
                            heap_slot(heap, (i >> 1) as usize).get(),
                            heap_slot(heap, i as usize).get(),
                        )
                    };
                    if ahe != 0 && ahe != pahe {
                        // SAFETY: non-fringe slots hold node entries of this tree.
                        return unsafe { art_heap_entry_to_node(ahe) };
                    }
                }

                ai.ai_j += 2;
                if ai.ai_j < minfringe {
                    ai.ai_i = ai.ai_j;
                } else {
                    // Set up the fringe loop
                    ai.ai_i = minfringe;
                    break;
                }
            }
        }

        // Descendent tables are only found on fringe nodes, and they record where they were
        // placed in their parent. This allows the iterator to know where to resume traversal
        // when it ascends back to the parent table. By keeping the table refs when going down
        // the tree, the topology is preserved even if all the nodes are removed.
        loop {
            let maxfringe = at.at_minfringe.get() << 1;

            // Iterate fringe nodes.
            loop {
                let i = ai.ai_i;
                if i >= maxfringe {
                    break;
                }
                ai.ai_i = i + 1;

                // SAFETY: `i` is below `2 * minfringe`, the heap's size.
                let (pahe, ahe) = unsafe {
                    (
                        heap_slot(heap, (i >> 1) as usize).get(),
                        heap_slot(heap, i as usize).get(),
                    )
                };
                if art_heap_entry_is_node(ahe) {
                    if ahe != 0 && ahe != pahe {
                        // SAFETY: a node entry of this tree.
                        return unsafe { art_heap_entry_to_node(ahe) };
                    }
                } else {
                    heap = art_heap_entry_to_heap(ahe);

                    if let Some(an) = art_iter_descend(ai, heap, pahe) {
                        return Some(an); // default route?
                    }

                    // Start looping over the child table
                    // SAFETY: the child heap is live.
                    at = unsafe { art_heap_to_table(heap) };
                    continue 'descend;
                }
            }

            // Ascend back up to the parent
            let parent = at.at_parent.get();
            ai.ai_i = at.at_index.get().wrapping_add(1); // the root's -1 wraps, as in C
            art_table_free(art, Some(at));

            ai.ai_table = parent;
            if parent.is_null() {
                // The root table has no parent
                break 'descend;
            }

            // SAFETY: a parent table is referenced by its child's walk, so it is alive.
            at = unsafe { &*parent };
            ai.ai_j = at.at_minfringe.get();
            heap = at.at_heap.get();
        }
    }

    None
}

/// `art_iter_close`: drops the references an unfinished iteration holds.
pub fn art_iter_close(ai: &mut ArtIter) {
    let mut at = ai.ai_table;
    while !at.is_null() {
        // SAFETY: the iteration holds a reference on every table from `ai_table` up.
        let t = unsafe { &*at };
        let parent = t.at_parent.get();
        // SAFETY: set by `art_iter_open`.
        art_table_free(unsafe { &*ai.ai_art }, Some(t));
        at = parent;
    }

    ai.ai_table = ptr::null();
}

/// `art_table_get`: creates a table and uses index `j` of `parent` to set its default route.
/// This function does not modify the root or the parent.
pub fn art_table_get(art: &Art, parent: Option<&ArtTable>, j: u32) -> Option<&'static ArtTable> {
    kassert!(j != ART_HEAP_IDX_TABLE as u32 && j != ART_HEAP_IDX_DEFAULT as u32);
    kassert!(parent.is_some() || j == u32::MAX);

    let level = parent.map_or(0, |p| p.at_level.get() + 1);
    kassert!(level < art.art_nlevels.get());

    let at_mem = pool_get(&AT_POOL, PR_NOWAIT | PR_ZERO)?;
    // SAFETY: a zeroed pool item of `size_of::<ArtTable>()` bytes; all-zero is a valid
    // `ArtTable` (cells of integers and raw pointers). It lives until `art_table_gc`.
    let at: &'static ArtTable = unsafe { &*at_mem.as_ptr().cast::<ArtTable>() };

    let bits = art.art_levels.get()[level as usize];
    let heap_mem = match bits {
        4 => pool_get(&AT_HEAP_4_POOL, PR_NOWAIT | PR_ZERO),
        8 => pool_get(&AT_HEAP_8_POOL, PR_NOWAIT | PR_ZERO),
        _ => panic(format_args!("incorrect stride length {bits}")),
    };

    let Some(heap_mem) = heap_mem else {
        pool_put(&AT_POOL, at_mem);
        return None;
    };
    // A zeroed block of `at_heapsize(bits)` bytes is `1 << (bits + 1)` zero words.
    let heap: ArtHeap = heap_mem.as_ptr().cast::<Cell<ArtHeapEntry>>();

    // SAFETY: slot 0 of the new heap.
    unsafe { heap_slot(heap, ART_HEAP_IDX_TABLE) }.set(ptr::from_ref(at) as ArtHeapEntry);

    at.at_parent.set(parent.map_or(ptr::null(), ptr::from_ref));
    at.at_index.set(j);
    at.at_minfringe.set(1 << bits);
    at.at_level.set(level);
    at.at_bits.set(bits);
    at.at_heap.set(heap);
    at.at_refcnt.set(0);

    if let Some(parent) = parent {
        at.at_offset
            .set(parent.at_offset.get() + parent.at_bits.get());

        // SAFETY: `j` is a fringe index of the parent's heap.
        let ahe = unsafe { heap_slot(parent.at_heap.get(), j as usize) }.get();
        // SAFETY: slot 1 of the new heap.
        unsafe { heap_slot(heap, ART_HEAP_IDX_DEFAULT) }.set(ahe);
    }

    Some(at)
}

/// `art_table_put`: deletes a table and uses its index to restore its parent's default
/// route, unlinking the table from its parent. Returns the parent.
pub fn art_table_put<'a>(art: &Art, at: &ArtTable) -> Option<&'a ArtTable> {
    let parent = at.at_parent.get();
    let j = at.at_index.get();

    kassert!(at.at_refcnt.get() == 0);
    kassert!(j != 0 && j != 1);

    // SAFETY: a table's parent outlives it (the child holds a reference on it).
    let parent_ref: Option<&'a ArtTable> = unsafe { parent.as_ref() };
    if let Some(p) = parent_ref {
        kassert!(j != u32::MAX);
        kassert!(at.at_level.get() == p.at_level.get() + 1);
        kassert!(p.at_refcnt.get() >= 1);

        // Give the route back to its parent.
        // SAFETY: slot 1 of the table's heap; `j` is a fringe index of the parent's heap.
        unsafe {
            let ahe = heap_slot(at.at_heap.get(), ART_HEAP_IDX_DEFAULT).get();
            heap_slot(p.at_heap.get(), j as usize).set(ahe);
        }
    } else {
        kassert!(j == u32::MAX);
        kassert!(at.at_level.get() == 0);
        art.art_root.set(ptr::null());
    }

    mtx_enter(&ART_TABLE_GC_MTX);
    at.at_parent.set(ART_TABLE_GC_LIST.load(Ordering::Relaxed));
    ART_TABLE_GC_LIST.store(ptr::from_ref(at).cast_mut(), Ordering::Relaxed);
    mtx_leave(&ART_TABLE_GC_MTX);

    task_add(SYSTQMP, &ART_TABLE_GC_TASK);

    parent_ref
}

/// `art_table_gc`: frees the tables `art_table_put` collected.
pub fn art_table_gc(_null: *mut c_void) {
    mtx_enter(&ART_TABLE_GC_MTX);
    let mut at = ART_TABLE_GC_LIST.swap(ptr::null_mut(), Ordering::Relaxed);
    mtx_leave(&ART_TABLE_GC_MTX);

    // smr_barrier(): no SMR (the module's deviations).

    while let Some(t) = NonNull::new(at) {
        // SAFETY: a table on the collector's list is out of every tree; this task is its only
        // user until it frees it here.
        let tr = unsafe { t.as_ref() };
        let next = tr.at_parent.get().cast_mut();

        let heap = NonNull::new(tr.at_heap.get().cast_mut()).map(NonNull::cast::<u8>);
        let bits = tr.at_bits.get();
        if let Some(heap) = heap {
            match bits {
                4 => pool_put(&AT_HEAP_4_POOL, heap),
                8 => pool_put(&AT_HEAP_8_POOL, heap),
                _ => panic(format_args!("incorrect stride length {bits}")),
            }
        }

        pool_put(&AT_POOL, t.cast());

        at = next;
    }
}

/// `art_allot`: substitutes node entry `nahe` for `oahe` in the subtree whose root index is
/// `i`: every fringe slot (or the default of the child table it points to) and every
/// non-fringe slot that held `oahe`, and slot `i` itself.
fn art_allot(at: &ArtTable, i: u32, oahe: ArtHeapEntry, nahe: ArtHeapEntry) {
    kassert!(i < at.at_minfringe.get());

    let heap = at.at_heap.get();
    let minfringe = at.at_minfringe.get();

    for k in [i << 1, (i << 1) + 1] {
        if k >= minfringe {
            // Change fringe nodes.
            // SAFETY: `k` is below `2 * minfringe`, the heap's size.
            let mut ahep = unsafe { heap_slot(heap, k as usize) };
            if !art_heap_entry_is_node(ahep.get()) {
                let child = art_heap_entry_to_heap(ahep.get());
                // SAFETY: a child heap of this tree; slot 1 exists in every heap.
                ahep = unsafe { heap_slot(child, ART_HEAP_IDX_DEFAULT) };
            }
            if ahep.get() == oahe {
                ahep.set(nahe);
            }
        } else {
            // SAFETY: `k` is a non-fringe index of the heap.
            if unsafe { heap_slot(heap, k as usize) }.get() == oahe {
                art_allot(at, k, oahe, nahe);
            }
        }
    }

    // Change non-fringe node.
    // SAFETY: `i` is below `minfringe`.
    unsafe { heap_slot(heap, i as usize) }.set(nahe);
}

/// `art_get`: a new node for the prefix `addr`/`plen`, `None` without memory.
pub fn art_get(addr: &[u8], plen: u32) -> Option<&'static ArtNode> {
    let p = pool_get(&AN_POOL, PR_NOWAIT | PR_ZERO)?;
    // SAFETY: a zeroed pool item of `size_of::<ArtNode>()` bytes; all-zero is a valid
    // `ArtNode`. It lives until `art_gc`.
    let an: &'static ArtNode = unsafe { &*p.as_ptr().cast::<ArtNode>() };

    art_node_init(an, addr, plen);

    Some(an)
}

/// `art_node_init`: sets the node's prefix.
pub fn art_node_init(an: &ArtNode, addr: &[u8], plen: u32) {
    let len = plen.div_ceil(8) as usize;
    kassert!(len <= 16);
    let mut a = [0u8; 16];
    a[..len].copy_from_slice(&addr[..len]);
    an.an_addr.set(a);
    an.an_plen.set(plen);
}

/// `art_put`: gives a node that left the tree to the garbage collector.
pub fn art_put(an: &'static ArtNode) {
    mtx_enter(&ART_NODE_GC_MTX);
    an.an_gc.set(ART_NODE_GC_LIST.load(Ordering::Relaxed));
    ART_NODE_GC_LIST.store(ptr::from_ref(an).cast_mut(), Ordering::Relaxed);
    mtx_leave(&ART_NODE_GC_MTX);

    task_add(SYSTQMP, &ART_NODE_GC_TASK);
}

/// `art_gc`: frees the nodes `art_put` collected.
pub fn art_gc(_null: *mut c_void) {
    mtx_enter(&ART_NODE_GC_MTX);
    let mut an = ART_NODE_GC_LIST.swap(ptr::null_mut(), Ordering::Relaxed);
    mtx_leave(&ART_NODE_GC_MTX);

    // smr_barrier(): no SMR (the module's deviations).

    while let Some(n) = NonNull::new(an) {
        // SAFETY: a node on the collector's list is out of every tree; this task is its only
        // user until it frees it here.
        let next = unsafe { n.as_ref() }.an_gc.get().cast_mut();

        pool_put(&AN_POOL, n.cast());

        an = next;
    }
}

#[cfg(test)]
mod tests;
