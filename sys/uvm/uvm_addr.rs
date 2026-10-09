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
 * Copyright (c) 2011 Ariane van der Steldt <ariane@stack.nl>
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
//! `uvm_addr.h` / `uvm_addr.c`: address selection logic.
//!
//! Upstream: sys/uvm/uvm_addr.h @ 3ce1f3f79392
//! Upstream: sys/uvm/uvm_addr.c @ 3ce1f3f79392
//!
//! Address selection is just that: selection. These functions may make no changes to the
//! map, except for their own state (which is passed as a `uaddr_state` pointer).
//!
//! Each selector is described by a [`UvmAddrFunctions`] table (the C's function pointers)
//! and a [`UvmAddrState`] that carries the range it manages; the bestfit and pivot
//! selectors keep more state after it ([`UaddrBestfitState`], [`UaddrPivotState`]).
//!
//! ## Deviations
//! - The states with extra fields are `#[repr(C)]` with the base [`UvmAddrState`] first,
//!   as in C; the selectors cast back to their own state after checking that the function
//!   table is theirs.
//! - The C's output parameters become return values: a selector yields the entry and the
//!   address, [`uvm_addr_fitspace`] the lowest and highest fitting address,
//!   [`uvm_addr_invoke`] the first and last entry and the address.
//! - `uaddr_lin_*` and `uaddr_rnd_print` are `#if 0` in the C and not ported;
//!   `uvm_addr_print` and `uaddr_pivot_print` (`DEBUG`/`DDB`) wait for the ddb printers.
//! - `SMALL_KERNEL` is not configured: the bestfit, pivot and stack/brk selectors exist.
//! - `MACHINE_STACK_GROWS_UP` is not defined on amd64 or arm64.
//! - A pivot remembers its entry as a raw pointer: [`uaddr_pivot_remove`] clears it whenever
//!   the entry leaves the free tree, before the map can free the entry.

use core::cell::Cell;
use core::ptr::{self, NonNull};

use crate::dev::rnd::{arc4random, arc4random_uniform};
use crate::kern::subr_pool::{pool_get, pool_init, pool_put};
use crate::kern::subr_prf::panic;
use crate::machine::intr::IPL_VM;
use crate::sys::errno::Errno;
use crate::sys::param::{PAGE_MASK, PAGE_SHIFT, PAGE_SIZE};
use crate::sys::pool::{PR_WAITOK, Pool};
use crate::sys::tree::{RbtEntry, RbtHead};
use crate::uvm::uvm_extern::VmProt;
use crate::uvm::uvm_map::{
    UVM_MAXKADDR, UvmMapAddr, VM_MAP_GUARDPAGES, VM_MAP_ISVMSPACE, VmMap, VmMapEntry,
    uvm_map_entrybyaddr, uvm_map_hint, uvm_map_isavail, uvm_map_uaddr_e, vm_map_assert_anylock,
    vmmap_free_end, vmmap_free_start,
};
use crate::{kassert, tree_adapter};

/// `NUM_PIVOTS`: number of pivots in pivot allocator.
const NUM_PIVOTS: usize = 16;
/// `PIVOT_RND`: max number (inclusive) of pages the pivot allocator will place between
/// allocations.
///
/// The `uaddr_pivot_random()` function attempts to bias towards small space between
/// allocations, so putting a large number here is fine.
const PIVOT_RND: u32 = 8;
/// `PIVOT_EXPIRE`: number of allocations that a pivot can supply before expiring. When a
/// pivot expires, a new pivot has to be found.
///
/// Must be at least 1.
const PIVOT_EXPIRE: i32 = 1024;

/// `struct uvm_addr_state`: UVM address selection base state.
///
/// Each uvm address algorithm requires these parameters: lower bound address (page
/// aligned), upper bound address (page aligned), function address pointers.
///
/// The map that owns the state guards it with its lock.
pub struct UvmAddrState {
    /// `uaddr_minaddr`.
    pub uaddr_minaddr: Cell<usize>,
    /// `uaddr_maxaddr`.
    pub uaddr_maxaddr: Cell<usize>,
    /// `uaddr_functions`.
    pub uaddr_functions: &'static UvmAddrFunctions,
}

// SAFETY: the owning map's lock guards the state (see the struct doc); the function table
// is immutable.
unsafe impl Sync for UvmAddrState {}

/// `uaddr_select`'s signature: `(map, uaddr, sz, align, offset, prot, hint)`, yielding the
/// entry that contains the selected address and the address.
pub type UaddrSelect = for<'m> fn(
    &'m VmMap,
    &'m UvmAddrState,
    usize,
    usize,
    usize,
    VmProt,
    usize,
) -> Result<(&'m VmMapEntry, usize), Errno>;

/// `uaddr_free_insert`'s and `uaddr_free_remove`'s signature: `(map, uaddr_state, entry)`.
pub type UaddrFreeOp = fn(&VmMap, &UvmAddrState, &VmMapEntry);

/// `struct uvm_addr_functions`: describes one algorithm implementation.
///
/// Each algorithm is described in terms of: `uaddr_select`, an address selection algorithm;
/// `uaddr_free_insert`, a freelist insertion function (optional); `uaddr_free_remove`, a
/// freelist deletion function (optional); `uaddr_destroy`, a destructor for the algorithm
/// state.
pub struct UvmAddrFunctions {
    /// `uaddr_select`.
    pub uaddr_select: UaddrSelect,
    /// `uaddr_free_insert`.
    pub uaddr_free_insert: Option<UaddrFreeOp>,
    /// `uaddr_free_remove`.
    pub uaddr_free_remove: Option<UaddrFreeOp>,
    /// `uaddr_destroy`.
    pub uaddr_destroy: fn(&'static UvmAddrState),
    // uaddr_print: DEBUG/DDB, see the module's deviations.
    /// `uaddr_name`: name of the allocator.
    pub uaddr_name: &'static str,
}

/// `uaddr_pool`: pool with `uvm_addr_state` structures.
static UADDR_POOL: Pool = Pool::new();
/// `uaddr_bestfit_pool`.
static UADDR_BESTFIT_POOL: Pool = Pool::new();
/// `uaddr_pivot_pool`.
static UADDR_PIVOT_POOL: Pool = Pool::new();
/// `uaddr_rnd_pool`.
static UADDR_RND_POOL: Pool = Pool::new();

/// `struct uaddr_bestfit_state`: uvm_addr state for bestfit selector.
#[repr(C)]
pub struct UaddrBestfitState {
    /// `ubf_uaddr`.
    pub ubf_uaddr: UvmAddrState,
    /// `ubf_free`.
    pub ubf_free: RbtHead<UaddrFreeRbtree>,
}

/// `struct uaddr_rnd_state`: uvm_addr state for rnd selector.
#[repr(C)]
pub struct UaddrRndState {
    /// `ur_uaddr`.
    pub ur_uaddr: UvmAddrState,
    // ur_free: #if 0 in the C.
}

/// `struct uaddr_pivot`: definition of a pivot in pivot selector.
pub struct UaddrPivot {
    /// `addr`: end of prev. allocation.
    pub addr: Cell<usize>,
    /// `expire`: best before date.
    pub expire: Cell<i32>,
    /// `dir`: direction.
    pub dir: Cell<i32>,
    /// `entry`: will contain next alloc (see the module's deviations).
    pub entry: Cell<*const VmMapEntry>,
}

impl UaddrPivot {
    /// A zeroed pivot: expired.
    pub const fn new() -> Self {
        Self {
            addr: Cell::new(0),
            expire: Cell::new(0),
            dir: Cell::new(0),
            entry: Cell::new(ptr::null()),
        }
    }

    /// `pivot->entry`, when the pivot points into a free entry.
    fn entry(&self) -> Option<&VmMapEntry> {
        // SAFETY: `uaddr_pivot_remove` nulls this pointer before the entry leaves the free
        // tree, so a non-null pointer is an entry still linked in the map, which the map's
        // lock keeps alive.
        unsafe { self.entry.get().as_ref() }
    }
}

impl Default for UaddrPivot {
    fn default() -> Self {
        Self::new()
    }
}

/// `struct uaddr_pivot_state`: uvm_addr state for pivot selector.
#[repr(C)]
pub struct UaddrPivotState {
    /// `up_uaddr`.
    pub up_uaddr: UvmAddrState,
    /// `up_free`: free space tree, for fast pivot selection.
    pub up_free: RbtHead<UaddrFreeRbtree>,
    /// `up_pivots`: list of pivots. The pointers point to after the last allocation.
    pub up_pivots: [UaddrPivot; NUM_PIVOTS],
}

/// `uaddr_kbootstrap`: the kernel bootstrap allocator's state (`uvm_addr_init` sets its
/// range).
pub static UADDR_KBOOTSTRAP: UvmAddrState = UvmAddrState {
    uaddr_minaddr: Cell::new(0),
    uaddr_maxaddr: Cell::new(0),
    uaddr_functions: &UADDR_KERNEL_FUNCTIONS,
};

/*
 * Support functions.
 */

/// The bestfit state behind `uaddr`.
fn bestfit_state(uaddr: &UvmAddrState) -> &UaddrBestfitState {
    kassert!(ptr::eq(uaddr.uaddr_functions, &UADDR_BESTFIT_FUNCTIONS));
    // SAFETY: a state whose functions are the bestfit ones was made by
    // `uaddr_bestfit_create`, as the first field of a `#[repr(C)]` `UaddrBestfitState`.
    unsafe { &*ptr::from_ref(uaddr).cast::<UaddrBestfitState>() }
}

/// The pivot state behind `uaddr`.
fn pivot_state(uaddr: &UvmAddrState) -> &UaddrPivotState {
    kassert!(ptr::eq(uaddr.uaddr_functions, &UADDR_PIVOT_FUNCTIONS));
    // SAFETY: a state whose functions are the pivot ones was made by `uaddr_pivot_create`,
    // as the first field of a `#[repr(C)]` `UaddrPivotState`.
    unsafe { &*ptr::from_ref(uaddr).cast::<UaddrPivotState>() }
}

/// `uvm_addr_entrybyspace`: find smallest entry in tree that will fit `sz` bytes.
pub fn uvm_addr_entrybyspace(free: &RbtHead<UaddrFreeRbtree>, sz: usize) -> Option<&VmMapEntry> {
    let mut tmp = free.root();
    let mut res = None;
    while let Some(e) = tmp {
        if e.fspace.get() >= sz {
            res = Some(e);
            tmp = RbtHead::<UaddrFreeRbtree>::left(e);
        } else {
            tmp = RbtHead::<UaddrFreeRbtree>::right(e);
        }
    }
    res
}

/// `uvm_addr_align_forward`: the first address at or after `addr` that is `offset` past a
/// multiple of `align` (wrapping, which the callers check for).
#[inline]
fn uvm_addr_align_forward(addr: usize, align: usize, offset: usize) -> usize {
    kassert!(offset < align || (align == 0 && offset == 0));
    kassert!(align & align.wrapping_sub(1) == 0);
    kassert!(offset & PAGE_MASK == 0);

    let align = align.max(PAGE_SIZE);
    let adjusted = (addr & !(align - 1)).wrapping_add(offset);
    if adjusted < addr {
        adjusted.wrapping_add(align)
    } else {
        adjusted
    }
}

/// `uvm_addr_align_backward`: the last address at or before `addr` that is `offset` past a
/// multiple of `align` (wrapping, which the callers check for).
#[inline]
fn uvm_addr_align_backward(addr: usize, align: usize, offset: usize) -> usize {
    kassert!(offset < align || (align == 0 && offset == 0));
    kassert!(align & align.wrapping_sub(1) == 0);
    kassert!(offset & PAGE_MASK == 0);

    let align = align.max(PAGE_SIZE);
    let adjusted = (addr & !(align - 1)).wrapping_add(offset);
    if adjusted > addr {
        adjusted.wrapping_sub(align)
    } else {
        adjusted
    }
}

/// `uvm_addr_fitspace`: try to fit the requested space into the entry. Yields the lowest
/// and the highest address at which `sz` bytes fit in `[low_addr, high_addr)` with the
/// alignment and the gaps.
#[allow(clippy::too_many_arguments)] // the C's signature
pub fn uvm_addr_fitspace(
    low_addr: usize,
    high_addr: usize,
    sz: usize,
    align: usize,
    offset: usize,
    before_gap: usize,
    after_gap: usize,
) -> Result<(usize, usize), Errno> {
    if low_addr > high_addr {
        return Err(Errno::ENOMEM);
    }
    let fspace = high_addr - low_addr;
    if fspace < before_gap + after_gap {
        return Err(Errno::ENOMEM);
    }
    if fspace - before_gap - after_gap < sz {
        return Err(Errno::ENOMEM);
    }

    // Calculate lowest address.
    let tmp = low_addr + before_gap;
    let low_addr = uvm_addr_align_forward(tmp, align, offset);
    if low_addr < tmp {
        // Overflow during alignment.
        return Err(Errno::ENOMEM);
    }
    if high_addr - after_gap - sz < low_addr {
        return Err(Errno::ENOMEM);
    }

    // Calculate highest address.
    let tmp = high_addr - (after_gap + sz);
    let high_addr = uvm_addr_align_backward(tmp, align, offset);
    if high_addr > tmp {
        // Overflow during alignment.
        return Err(Errno::ENOMEM);
    }
    if low_addr > high_addr {
        return Err(Errno::ENOMEM);
    }

    Ok((low_addr, high_addr))
}

/// `uvm_addr_init`: initialize uvm_addr.
pub fn uvm_addr_init() {
    pool_init(
        &UADDR_POOL,
        size_of::<UvmAddrState>(),
        0,
        IPL_VM,
        PR_WAITOK,
        "uaddr",
        None,
    );
    pool_init(
        &UADDR_BESTFIT_POOL,
        size_of::<UaddrBestfitState>(),
        0,
        IPL_VM,
        PR_WAITOK,
        "uaddrbest",
        None,
    );
    pool_init(
        &UADDR_PIVOT_POOL,
        size_of::<UaddrPivotState>(),
        0,
        IPL_VM,
        PR_WAITOK,
        "uaddrpivot",
        None,
    );
    pool_init(
        &UADDR_RND_POOL,
        size_of::<UaddrRndState>(),
        0,
        IPL_VM,
        PR_WAITOK,
        "uaddrrnd",
        None,
    );

    UADDR_KBOOTSTRAP.uaddr_minaddr.set(PAGE_SIZE);
    UADDR_KBOOTSTRAP
        .uaddr_maxaddr
        .set(0usize.wrapping_sub(PAGE_SIZE));
    // uaddr_kbootstrap.uaddr_functions = &uaddr_kernel_functions: statically.
}

/// `uvm_addr_destroy`: invoke destructor function of uaddr.
pub fn uvm_addr_destroy(uaddr: Option<&'static UvmAddrState>) {
    if let Some(uaddr) = uaddr {
        (uaddr.uaddr_functions.uaddr_destroy)(uaddr);
    }
}

/// `uvm_addr_linsearch`: directional first fit.
///
/// Do a linear search for free space, starting at addr in entry. `direction == 1`: search
/// forward; `direction == -1`: search backward.
///
/// Output: `low <= addr <= high` and entry will contain addr. `ENOMEM` if no space is
/// available.
///
/// gap describes the space that must appear between the preceding entry.
#[allow(clippy::too_many_arguments)] // the C's signature
pub fn uvm_addr_linsearch<'m>(
    map: &'m VmMap,
    _uaddr: &UvmAddrState,
    hint: usize,
    sz: usize,
    align: usize,
    offset: usize,
    direction: i32,
    low: usize,
    high: usize,
    before_gap: usize,
    after_gap: usize,
) -> Result<(&'m VmMapEntry, usize), Errno> {
    kassert!(direction == -1 || direction == 1);
    kassert!(
        hint & PAGE_MASK == 0
            && high & PAGE_MASK == 0
            && low & PAGE_MASK == 0
            && before_gap & PAGE_MASK == 0
            && after_gap & PAGE_MASK == 0
    );
    kassert!(high.wrapping_add(sz) > high); // Check for overflow.

    // Hint magic.
    let mut hint = hint;
    if hint == 0 {
        hint = if direction == 1 { low } else { high };
    } else if hint > high {
        if direction != -1 {
            return Err(Errno::ENOMEM);
        }
        hint = high;
    } else if hint < low {
        if direction != 1 {
            return Err(Errno::ENOMEM);
        }
        hint = low;
    }

    let mut entry = uvm_map_entrybyaddr(
        &map.addr,
        hint.wrapping_sub(if direction == -1 { 1 } else { 0 }),
    );
    while let Some(e) = entry {
        if (direction == 1 && vmmap_free_start(e) > high)
            || (direction == -1 && vmmap_free_end(e) < low)
        {
            break;
        }

        if let Ok((low_addr, high_addr)) = uvm_addr_fitspace(
            low.max(vmmap_free_start(e)),
            high.min(vmmap_free_end(e)),
            sz,
            align,
            offset,
            before_gap,
            after_gap,
        ) {
            let addr = if hint >= low_addr && hint <= high_addr {
                hint
            } else if direction == 1 {
                low_addr
            } else {
                high_addr
            };
            return Ok((e, addr));
        }

        entry = if direction == 1 {
            RbtHead::<UvmMapAddr>::next(e)
        } else {
            RbtHead::<UvmMapAddr>::prev(e)
        };
    }

    Err(Errno::ENOMEM)
}

/// `uvm_addr_invoke`: invoke address selector of uaddr. `uaddr` may be `None`, in which
/// case the algorithm will fail with `ENOMEM`.
///
/// Will invoke `uvm_map_isavail` to fill in the last entry: yields the first entry, the
/// last entry and the address.
#[allow(clippy::too_many_arguments)] // the C's signature
pub fn uvm_addr_invoke<'m>(
    map: &'m VmMap,
    uaddr: Option<&'m UvmAddrState>,
    sz: usize,
    align: usize,
    offset: usize,
    prot: VmProt,
    hint: usize,
) -> Result<(&'m VmMapEntry, &'m VmMapEntry, usize), Errno> {
    let Some(uaddr) = uaddr else {
        return Err(Errno::ENOMEM);
    };

    let hint = hint & !PAGE_MASK;
    if hint != 0 && !(hint >= uaddr.uaddr_minaddr.get() && hint < uaddr.uaddr_maxaddr.get()) {
        return Err(Errno::ENOMEM);
    }

    vm_map_assert_anylock(map);

    let (entry, addr) =
        (uaddr.uaddr_functions.uaddr_select)(map, uaddr, sz, align, offset, prot, hint)?;

    let mut first = Some(entry);
    let mut last = None;
    if !uvm_map_isavail(map, Some(uaddr), &mut first, &mut last, addr, sz) {
        panic(format_args!(
            "uvm_addr_invoke: address selector {:p} ({} {:#x}-{:#x}) returned unavailable address {:#x} sz {:#x}",
            ptr::from_ref(uaddr),
            uaddr.uaddr_functions.uaddr_name,
            uaddr.uaddr_minaddr.get(),
            uaddr.uaddr_maxaddr.get(),
            addr,
            sz
        ));
    }
    match (first, last) {
        (Some(first), Some(last)) => Ok((first, last, addr)),
        _ => panic(format_args!(
            "uvm_addr_invoke: uvm_map_isavail left the range {:#x}+{:#x} unbounded",
            addr, sz
        )),
    }
}

/// `uaddr_destroy`: destroy a `uvm_addr_state` structure. The uaddr must have been
/// previously allocated from `uaddr_pool`.
pub fn uaddr_destroy(uaddr: &'static UvmAddrState) {
    pool_put(&UADDR_POOL, NonNull::from(uaddr).cast::<u8>());
}

/*
 * Randomized allocator.
 * This allocator use uvm_map_hint to acquire a random address and searches
 * from there.
 */

/// `uaddr_rnd_functions`.
pub static UADDR_RND_FUNCTIONS: UvmAddrFunctions = UvmAddrFunctions {
    uaddr_select: uaddr_rnd_select,
    uaddr_free_insert: Some(uaddr_rnd_insert),
    uaddr_free_remove: Some(uaddr_rnd_remove),
    uaddr_destroy: uaddr_rnd_destroy,
    uaddr_name: "uaddr_rnd",
};

/// `uaddr_rnd_create`.
pub fn uaddr_rnd_create(minaddr: usize, maxaddr: usize) -> &'static UvmAddrState {
    let Some(mem) = pool_get(&UADDR_RND_POOL, PR_WAITOK) else {
        panic(format_args!("uaddr_rnd_create: uaddr_rnd_pool is empty"));
    };
    let p = mem.cast::<UaddrRndState>();
    // SAFETY: a fresh, suitably aligned pool item of `size_of::<UaddrRndState>()` bytes,
    // written once before anything else sees it; it lives until `uaddr_rnd_destroy`.
    let uaddr: &'static UaddrRndState = unsafe {
        p.as_ptr().write(UaddrRndState {
            ur_uaddr: UvmAddrState {
                uaddr_minaddr: Cell::new(minaddr),
                uaddr_maxaddr: Cell::new(maxaddr),
                uaddr_functions: &UADDR_RND_FUNCTIONS,
            },
        });
        p.as_ref()
    };
    &uaddr.ur_uaddr
}

/// `uaddr_rnd_select`.
pub fn uaddr_rnd_select<'m>(
    map: &'m VmMap,
    uaddr: &'m UvmAddrState,
    sz: usize,
    align: usize,
    offset: usize,
    prot: VmProt,
    hint: usize,
) -> Result<(&'m VmMapEntry, usize), Errno> {
    kassert!(map.flags.get() & VM_MAP_ISVMSPACE != 0);
    let vm = map.vmspace();

    // Deal with guardpages: search for space with one extra page.
    let guard_sz = if map.flags.get() & VM_MAP_GUARDPAGES == 0 {
        0
    } else {
        PAGE_SIZE
    };

    if uaddr.uaddr_maxaddr.get().wrapping_sub(guard_sz) < sz {
        return Err(Errno::ENOMEM);
    }
    let minaddr = uvm_addr_align_forward(uaddr.uaddr_minaddr.get(), align, offset);
    let maxaddr = uvm_addr_align_backward(uaddr.uaddr_maxaddr.get() - sz - guard_sz, align, offset);

    // Quick fail if the allocation won't fit.
    if minaddr >= maxaddr {
        return Err(Errno::ENOMEM);
    }

    // Select a hint.
    let mut hint = hint;
    if hint == 0 {
        hint = uvm_map_hint(vm, prot, minaddr, maxaddr);
    }
    // Clamp hint to uaddr range.
    hint = hint.max(minaddr).min(maxaddr);

    // Align hint to align,offset parameters.
    let tmp = hint;
    hint = uvm_addr_align_forward(tmp, align, offset);
    // Check for overflow during alignment.
    if hint < tmp || hint > maxaddr {
        return Err(Errno::ENOMEM); // Compatibility mode: never look backwards.
    }

    let before_gap = 0;
    let after_gap = guard_sz;
    hint -= hint.min(before_gap);
    let need = before_gap + after_gap + sz;

    // Use the augmented address tree to look up the first entry at or after hint with
    // sufficient space.
    //
    // This code is the original optimized code, but will fail if the subtree it looks at
    // does have sufficient space, but fails to meet the align constraint.
    //
    // Guard: subtree is not exhausted and max(fspace) >= required.
    let mut entry = uvm_map_entrybyaddr(&map.addr, hint);

    // Walk up the tree, until there is at least sufficient space.
    while let Some(e) = entry {
        if e.fspace_augment.get() >= need {
            break;
        }
        entry = RbtHead::<UvmMapAddr>::parent(e);
    }

    while let Some(e) = entry {
        // Test if this fits.
        if vmmap_free_end(e) > hint
            && uvm_map_uaddr_e(map, e).is_some_and(|u| ptr::eq(u, uaddr))
            && let Ok((low_addr, high_addr)) = uvm_addr_fitspace(
                uaddr.uaddr_minaddr.get().max(vmmap_free_start(e)),
                uaddr.uaddr_maxaddr.get().min(vmmap_free_end(e)),
                sz,
                align,
                offset,
                before_gap,
                after_gap,
            )
        {
            let addr = if hint >= low_addr && hint <= high_addr {
                hint
            } else {
                low_addr
            };
            return Ok((e, addr));
        }

        // RBT_NEXT, but skip subtrees that cannot possible fit.
        match RbtHead::<UvmMapAddr>::right(e) {
            Some(right) if right.fspace_augment.get() >= need => {
                let mut leftmost = right;
                while let Some(next) = RbtHead::<UvmMapAddr>::left(leftmost) {
                    leftmost = next;
                }
                entry = Some(leftmost);
            }
            _ => {
                // do_parent: climb while we are a right child; stop at the first ancestor
                // we are a left child of.
                let mut cur = e;
                entry = loop {
                    match RbtHead::<UvmMapAddr>::parent(cur) {
                        None => break None,
                        Some(parent) => {
                            if RbtHead::<UvmMapAddr>::left(parent).is_some_and(|l| ptr::eq(l, cur))
                            {
                                break Some(parent);
                            }
                            cur = parent;
                        }
                    }
                };
            }
        }
    }

    // Lookup failed.
    Err(Errno::ENOMEM)
}

/// `uaddr_rnd_destroy`: destroy a `uaddr_rnd_state` structure.
pub fn uaddr_rnd_destroy(uaddr: &'static UvmAddrState) {
    pool_put(&UADDR_RND_POOL, NonNull::from(uaddr).cast::<u8>());
}

/// `uaddr_rnd_insert`: add entry to tailq (nothing: the tailq is `#if 0`).
pub fn uaddr_rnd_insert(_map: &VmMap, _uaddr_p: &UvmAddrState, _entry: &VmMapEntry) {}

/// `uaddr_rnd_remove`: remove entry from tailq (nothing: the tailq is `#if 0`).
pub fn uaddr_rnd_remove(_map: &VmMap, _uaddr_p: &UvmAddrState, _entry: &VmMapEntry) {}

/*
 * Kernel allocation bootstrap logic.
 */

/// `uaddr_kernel_functions`.
pub static UADDR_KERNEL_FUNCTIONS: UvmAddrFunctions = UvmAddrFunctions {
    uaddr_select: uaddr_kbootstrap_select,
    uaddr_free_insert: None,
    uaddr_free_remove: None,
    uaddr_destroy: uaddr_kbootstrap_destroy,
    uaddr_name: "uaddr_kbootstrap",
};

/// `uaddr_kbootstrap_select`: select an address from the map.
///
/// This function ignores the uaddr spec and instead uses the map directly. Because of that
/// property, the uaddr algorithm can be shared across all kernel maps.
pub fn uaddr_kbootstrap_select<'m>(
    map: &'m VmMap,
    _uaddr: &'m UvmAddrState,
    sz: usize,
    align: usize,
    offset: usize,
    _prot: VmProt,
    _hint: usize,
) -> Result<(&'m VmMapEntry, usize), Errno> {
    let maxkaddr = UVM_MAXKADDR.load(core::sync::atomic::Ordering::Relaxed);
    for entry in map.addr.iter() {
        if vmmap_free_end(entry) <= maxkaddr
            && let Ok((addr, _)) = uvm_addr_fitspace(
                vmmap_free_start(entry),
                vmmap_free_end(entry),
                sz,
                align,
                offset,
                0,
                0,
            )
        {
            return Ok((entry, addr));
        }
    }

    Err(Errno::ENOMEM)
}

/// `uaddr_kbootstrap_destroy`: don't destroy the kernel bootstrap allocator.
pub fn uaddr_kbootstrap_destroy(uaddr: &'static UvmAddrState) {
    kassert!(ptr::eq(uaddr, &UADDR_KBOOTSTRAP));
}

/*
 * Best fit algorithm.
 */

/// `uaddr_bestfit_functions`.
pub static UADDR_BESTFIT_FUNCTIONS: UvmAddrFunctions = UvmAddrFunctions {
    uaddr_select: uaddr_bestfit_select,
    uaddr_free_insert: Some(uaddr_bestfit_insert),
    uaddr_free_remove: Some(uaddr_bestfit_remove),
    uaddr_destroy: uaddr_bestfit_destroy,
    uaddr_name: "uaddr_bestfit",
};

/// `uaddr_bestfit_create`.
pub fn uaddr_bestfit_create(minaddr: usize, maxaddr: usize) -> &'static UvmAddrState {
    let Some(mem) = pool_get(&UADDR_BESTFIT_POOL, PR_WAITOK) else {
        panic(format_args!(
            "uaddr_bestfit_create: uaddr_bestfit_pool is empty"
        ));
    };
    let p = mem.cast::<UaddrBestfitState>();
    // SAFETY: a fresh, suitably aligned pool item of `size_of::<UaddrBestfitState>()`
    // bytes, written once before anything else sees it; it lives until
    // `uaddr_bestfit_destroy`.
    let uaddr: &'static UaddrBestfitState = unsafe {
        p.as_ptr().write(UaddrBestfitState {
            ubf_uaddr: UvmAddrState {
                uaddr_minaddr: Cell::new(minaddr),
                uaddr_maxaddr: Cell::new(maxaddr),
                uaddr_functions: &UADDR_BESTFIT_FUNCTIONS,
            },
            ubf_free: RbtHead::new(),
        });
        p.as_ref()
    };
    uaddr.ubf_free.init();
    &uaddr.ubf_uaddr
}

/// `uaddr_bestfit_destroy`.
pub fn uaddr_bestfit_destroy(uaddr: &'static UvmAddrState) {
    pool_put(&UADDR_BESTFIT_POOL, NonNull::from(uaddr).cast::<u8>());
}

/// `uaddr_bestfit_insert`.
pub fn uaddr_bestfit_insert(_map: &VmMap, uaddr_p: &UvmAddrState, entry: &VmMapEntry) {
    let uaddr = bestfit_state(uaddr_p);
    // SAFETY: the map's lock is held and the entry is on no free tree (`UVM_ET_FREEMAPPED`
    // is clear, asserted by `uvm_mapent_free_insert`).
    if let Some(rb_rv) = unsafe { uaddr.ubf_free.insert(entry) } {
        panic(format_args!(
            "uaddr_bestfit_insert: duplicate insertion: state {:p} inserting {:p}, colliding with {:p}",
            ptr::from_ref(uaddr),
            ptr::from_ref(entry),
            ptr::from_ref(rb_rv)
        ));
    }
}

/// `uaddr_bestfit_remove`.
pub fn uaddr_bestfit_remove(_map: &VmMap, uaddr_p: &UvmAddrState, entry: &VmMapEntry) {
    let uaddr = bestfit_state(uaddr_p);
    // SAFETY: the map's lock is held and the entry is on this free tree
    // (`UVM_ET_FREEMAPPED` is set, asserted by `uvm_mapent_free_remove`).
    let removed = unsafe { uaddr.ubf_free.remove(entry) };
    if !ptr::eq(removed, entry) {
        panic(format_args!("uaddr_bestfit_remove: entry was not in tree"));
    }
}

/// `uaddr_bestfit_select`.
pub fn uaddr_bestfit_select<'m>(
    map: &'m VmMap,
    uaddr_p: &'m UvmAddrState,
    sz: usize,
    align: usize,
    offset: usize,
    _prot: VmProt,
    _hint: usize,
) -> Result<(&'m VmMapEntry, usize), Errno> {
    let uaddr = bestfit_state(uaddr_p);
    let guardsz = if map.flags.get() & VM_MAP_GUARDPAGES != 0 {
        PAGE_SIZE
    } else {
        0
    };
    if sz.wrapping_add(guardsz) < sz {
        return Err(Errno::ENOMEM);
    }

    // Find smallest item on freelist capable of holding item. Deal with guardpages:
    // search for space with one extra page.
    let mut entry = uvm_addr_entrybyspace(&uaddr.ubf_free, sz + guardsz).ok_or(Errno::ENOMEM)?;

    // Walk the tree until we find an entry that fits.
    let (min, max) = loop {
        match uvm_addr_fitspace(
            vmmap_free_start(entry),
            vmmap_free_end(entry),
            sz,
            align,
            offset,
            0,
            guardsz,
        ) {
            Ok(fit) => break fit,
            Err(_) => {
                entry = RbtHead::<UaddrFreeRbtree>::next(entry).ok_or(Errno::ENOMEM)?;
            }
        }
    };

    // Return the address that generates the least fragmentation.
    let addr = if min - vmmap_free_start(entry) <= vmmap_free_end(entry) - guardsz - sz - max {
        min
    } else {
        max
    };
    Ok((entry, addr))
}

/*
 * A userspace allocator based on pivots.
 */

/// `uaddr_pivot_functions`.
pub static UADDR_PIVOT_FUNCTIONS: UvmAddrFunctions = UvmAddrFunctions {
    uaddr_select: uaddr_pivot_select,
    uaddr_free_insert: Some(uaddr_pivot_insert),
    uaddr_free_remove: Some(uaddr_pivot_remove),
    uaddr_destroy: uaddr_pivot_destroy,
    uaddr_name: "uaddr_pivot",
};

/// `uaddr_pivot_random`: a special random function for pivots.
///
/// This function will return: a random number, a multiple of `PAGE_SIZE`, at least
/// `PAGE_SIZE`.
///
/// The random function has a slightly higher change to return a small number.
pub fn uaddr_pivot_random() -> usize {
    // The sum of two six-sided dice will have a normal distribution. We map the highest
    // probable number to 1, by folding the curve (think of a graph on a piece of paper,
    // that you fold).
    //
    // Because the fold happens at PIVOT_RND - 1, the numbers 0 and 1 have the same and
    // highest probability of happening.
    let r = (arc4random_uniform(PIVOT_RND) + arc4random_uniform(PIVOT_RND)) as i32
        - (PIVOT_RND as i32 - 1);
    let r = r.unsigned_abs() as usize;

    // Make the returned value at least PAGE_SIZE and a multiple of PAGE_SIZE.
    (1 + r) << PAGE_SHIFT
}

/// `uaddr_pivot_newpivot`: select a new pivot.
///
/// A pivot must: be chosen random; have a randomly chosen gap before it, where the
/// uaddr_state starts; have a randomly chosen gap after it, before the uaddr_state ends.
///
/// Furthermore, the pivot must provide sufficient space for the allocation. The addr will
/// be set to the selected address.
///
/// Returns `ENOMEM` on failure.
#[allow(clippy::too_many_arguments)] // the C's signature
pub fn uaddr_pivot_newpivot<'m>(
    _map: &'m VmMap,
    uaddr: &'m UaddrPivotState,
    pivot: &UaddrPivot,
    sz: usize,
    align: usize,
    offset: usize,
    before_gap: usize,
    after_gap: usize,
) -> Result<(&'m VmMapEntry, usize), Errno> {
    let mut minaddr = uaddr.up_uaddr.uaddr_minaddr.get();
    let mut maxaddr = uaddr.up_uaddr.uaddr_maxaddr.get();
    kassert!(minaddr < maxaddr);
    #[cfg(feature = "diagnostic")]
    if minaddr + 2 * PAGE_SIZE > maxaddr {
        panic(format_args!(
            "uaddr_pivot_newpivot: cannot grant random pivot in area less than 2 pages (size = {:#x})",
            maxaddr - minaddr
        ));
    }

    // Gap calculation: 1/32 of the size of the managed area.
    //
    // At most: sufficient to not get truncated at arc4random. At least: 2 PAGE_SIZE.
    //
    // minaddr and maxaddr will be changed according to arc4random.
    let dist = ((maxaddr - minaddr) / 32).max(2 * PAGE_SIZE);
    if dist >> PAGE_SHIFT > 0xffff_ffff {
        minaddr += (arc4random() as usize) << PAGE_SHIFT;
        maxaddr -= (arc4random() as usize) << PAGE_SHIFT;
    } else {
        minaddr += (arc4random_uniform((dist >> PAGE_SHIFT) as u32) as usize) << PAGE_SHIFT;
        maxaddr -= (arc4random_uniform((dist >> PAGE_SHIFT) as u32) as usize) << PAGE_SHIFT;
    }

    // A very fast way to find an entry that will be large enough to hold the allocation,
    // but still is found more or less randomly: the tree path selector has a 50% chance to
    // go for a bigger or smaller entry.
    //
    // Note that the memory may actually be available, but the fragmentation may be so bad
    // and the gaps chosen so unfortunately, that the allocation will not succeed. Or the
    // alignment can only be satisfied by an entry that is not visited in the randomly
    // selected path.
    //
    // This code finds an entry with sufficient space in O(log n) time.
    let mut path = arc4random();
    let mut found: Option<(&'m VmMapEntry, usize, usize)> = None;
    let mut entry = uaddr.up_free.root();
    while let Some(e) = entry {
        let fit = uvm_addr_fitspace(
            vmmap_free_start(e).max(minaddr),
            vmmap_free_end(e).min(maxaddr),
            sz,
            align,
            offset,
            before_gap,
            after_gap,
        );

        // It fits, save this entry.
        if let Ok((min, max)) = fit {
            found = Some((e, min, max));
        }

        // Next.
        entry = if fit.is_err() {
            RbtHead::<UaddrFreeRbtree>::right(e)
        } else if path & 0x1 == 0 {
            path >>= 1;
            RbtHead::<UaddrFreeRbtree>::right(e)
        } else {
            path >>= 1;
            RbtHead::<UaddrFreeRbtree>::left(e)
        };
    }
    let Some((found, found_minaddr, found_maxaddr)) = found else {
        return Err(Errno::ENOMEM); // Not found a large enough region.
    };

    // Calculate a random address within found.
    //
    // found_minaddr and found_maxaddr are already aligned, so be sure to select a multiple
    // of align as the offset in the entry. Preferably, arc4random_uniform is used to
    // provide no bias within the entry. However if the size of the entry exceeds
    // arc4random_uniforms argument limit, we simply use arc4random (thus limiting
    // ourselves to 4G * PAGE_SIZE bytes offset).
    let addr = if found_maxaddr == found_minaddr {
        found_minaddr
    } else {
        kassert!(align >= PAGE_SIZE && align & (align - 1) == 0);
        let arc4_arg = found_maxaddr - found_minaddr;
        if arc4_arg > 0xffff_ffff {
            found_minaddr + (arc4random() as usize & !(align - 1))
        } else {
            found_minaddr + (arc4random_uniform(arc4_arg as u32) as usize & !(align - 1))
        }
    };

    // Set up new pivot and return selected address.
    //
    // Depending on the direction of the pivot, the pivot must be placed at the bottom or
    // the top of the allocation: if the pivot moves upwards, place the pivot at the top of
    // the allocation; if the pivot moves downwards, place the pivot at the bottom of the
    // allocation.
    pivot.entry.set(found);
    pivot.dir.set(if arc4random() & 0x1 != 0 { 1 } else { -1 });
    if pivot.dir.get() > 0 {
        pivot.addr.set(addr + sz);
    } else {
        pivot.addr.set(addr);
    }
    pivot.expire.set(PIVOT_EXPIRE - 1); // First use is right now.
    Ok((found, addr))
}

/// `uaddr_pivot_select`: pivot selector.
///
/// Each time the selector is invoked, it will select a random pivot, which it will use to
/// select memory with. The memory will be placed at the pivot, with a randomly sized gap
/// between the allocation and the pivot. The pivot will then move so it will never revisit
/// this address.
///
/// Each allocation, the pivot expiry timer ticks. Once the pivot becomes expired, it will
/// be replaced with a newly created pivot. Pivots also automatically expire if they fail to
/// provide memory for an allocation.
///
/// Expired pivots are replaced using the `uaddr_pivot_newpivot()` function, which will
/// ensure the pivot points at memory in such a way that the allocation will succeed. As an
/// added bonus, the `uaddr_pivot_newpivot()` function will perform the allocation
/// immediately and move the pivot as appropriate.
///
/// If `uaddr_pivot_newpivot()` fails to find a new pivot that will allow the allocation to
/// succeed, it will not create a new pivot and the allocation will fail.
///
/// A pivot running into used memory will automatically expire (because it will fail to
/// allocate).
///
/// Characteristics of the allocator: best case, an allocation is O(log N) (it would be
/// O(1), if it weren't for the need to check if the memory is free; although that can be
/// avoided...); worst case, an allocation is O(log N) (the `uaddr_pivot_newpivot()`
/// function has that complexity); failed allocations always take O(log N) (the
/// `uaddr_pivot_newpivot()` function will walk that deep into the tree).
pub fn uaddr_pivot_select<'m>(
    map: &'m VmMap,
    uaddr_p: &'m UvmAddrState,
    sz: usize,
    align: usize,
    offset: usize,
    prot: VmProt,
    hint: usize,
) -> Result<(&'m VmMapEntry, usize), Errno> {
    // When we have a hint, use the rnd allocator that finds the area that is closest to
    // the hint, if there is such an area.
    if hint != 0 {
        return uaddr_rnd_select(map, uaddr_p, sz, align, offset, prot, hint)
            .map_err(|_| Errno::ENOMEM);
    }

    // Select a random pivot and a random gap sizes around the allocation.
    let uaddr = pivot_state(uaddr_p);
    let pivot = &uaddr.up_pivots[arc4random_uniform(NUM_PIVOTS as u32) as usize];
    let before_gap = uaddr_pivot_random();
    let after_gap = uaddr_pivot_random();
    if pivot.addr.get() != 0
        && pivot.expire.get() != 0
        && let Some(entry) = pivot.entry()
    {
        // Attempt to use the pivot to map the entry.
        if pivot.dir.get() > 0 {
            if let Ok((min, _)) = uvm_addr_fitspace(
                vmmap_free_start(entry).max(pivot.addr.get()),
                vmmap_free_end(entry),
                sz,
                align,
                offset,
                before_gap,
                after_gap,
            ) {
                pivot.addr.set(min + sz);
                pivot.expire.set(pivot.expire.get() - 1);
                return Ok((entry, min));
            }
        } else if let Ok((_, max)) = uvm_addr_fitspace(
            vmmap_free_start(entry),
            vmmap_free_end(entry).min(pivot.addr.get()),
            sz,
            align,
            offset,
            before_gap,
            after_gap,
        ) {
            pivot.addr.set(max);
            pivot.expire.set(pivot.expire.get() - 1);
            return Ok((entry, max));
        }
    }

    // Pivot expired or allocation failed. Use pivot selector to do the allocation and
    // find a new pivot.
    uaddr_pivot_newpivot(map, uaddr, pivot, sz, align, offset, before_gap, after_gap)
}

/// `uaddr_pivot_destroy`: free the pivot.
pub fn uaddr_pivot_destroy(uaddr: &'static UvmAddrState) {
    pool_put(&UADDR_PIVOT_POOL, NonNull::from(uaddr).cast::<u8>());
}

/// `uaddr_pivot_insert`: insert an entry with free space in the space tree.
pub fn uaddr_pivot_insert(_map: &VmMap, uaddr_p: &UvmAddrState, entry: &VmMapEntry) {
    let uaddr = pivot_state(uaddr_p);
    // SAFETY: the map's lock is held and the entry is on no free tree (`UVM_ET_FREEMAPPED`
    // is clear, asserted by `uvm_mapent_free_insert`).
    if let Some(rb_rv) = unsafe { uaddr.up_free.insert(entry) } {
        panic(format_args!(
            "uaddr_pivot_insert: duplicate insertion: state {:p} inserting entry {:p} which collides with {:p}",
            ptr::from_ref(uaddr),
            ptr::from_ref(entry),
            ptr::from_ref(rb_rv)
        ));
    }

    let start = vmmap_free_start(entry);
    let end = vmmap_free_end(entry);

    // Update all pivots that are contained in this entry.
    for p in &uaddr.up_pivots {
        let mut check_addr = p.addr.get();
        if check_addr == 0 {
            continue;
        }
        if p.dir.get() < 0 {
            check_addr -= 1;
        }

        if start <= check_addr && check_addr < end {
            kassert!(p.entry.get().is_null());
            p.entry.set(entry);
        }
    }
}

/// `uaddr_pivot_remove`: remove an entry with free space from the space tree.
pub fn uaddr_pivot_remove(_map: &VmMap, uaddr_p: &UvmAddrState, entry: &VmMapEntry) {
    let uaddr = pivot_state(uaddr_p);
    // SAFETY: the map's lock is held and the entry is on this free tree
    // (`UVM_ET_FREEMAPPED` is set, asserted by `uvm_mapent_free_remove`).
    let removed = unsafe { uaddr.up_free.remove(entry) };
    if !ptr::eq(removed, entry) {
        panic(format_args!("uaddr_pivot_remove: entry was not in tree"));
    }

    // Inform any pivot with this entry that the entry is gone. Note that this does not
    // automatically invalidate the pivot.
    for p in &uaddr.up_pivots {
        if ptr::eq(p.entry.get(), entry) {
            p.entry.set(ptr::null());
        }
    }
}

/// `uaddr_pivot_create`: create a new pivot selector.
///
/// Initially, all pivots are in the expired state. Two reasons for this: it means this
/// allocator will not take a huge amount of time; pivots select better on demand, because
/// the pivot selection will be affected by preceding allocations: the next pivots will
/// likely end up in different segments of free memory, that was segmented by an earlier
/// allocation; better spread.
pub fn uaddr_pivot_create(minaddr: usize, maxaddr: usize) -> &'static UvmAddrState {
    let Some(mem) = pool_get(&UADDR_PIVOT_POOL, PR_WAITOK) else {
        panic(format_args!(
            "uaddr_pivot_create: uaddr_pivot_pool is empty"
        ));
    };
    let p = mem.cast::<UaddrPivotState>();
    // SAFETY: a fresh, suitably aligned pool item of `size_of::<UaddrPivotState>()` bytes,
    // written once before anything else sees it; it lives until `uaddr_pivot_destroy`.
    let uaddr: &'static UaddrPivotState = unsafe {
        p.as_ptr().write(UaddrPivotState {
            up_uaddr: UvmAddrState {
                uaddr_minaddr: Cell::new(minaddr),
                uaddr_maxaddr: Cell::new(maxaddr),
                uaddr_functions: &UADDR_PIVOT_FUNCTIONS,
            },
            up_free: RbtHead::new(),
            up_pivots: [const { UaddrPivot::new() }; NUM_PIVOTS],
        });
        p.as_ref()
    };
    uaddr.up_free.init();
    &uaddr.up_uaddr
}

/*
 * Stack/break allocator.
 *
 * Stack area is grown into in the opposite direction of the stack growth,
 * brk area is grown downward (because sbrk() grows upward).
 *
 * Both areas are grown into proportially: a weighted chance is used to
 * select which one (stack or brk area) to try. If the allocation fails,
 * the other one is tested.
 */

/// `uaddr_stack_brk_functions`.
pub static UADDR_STACK_BRK_FUNCTIONS: UvmAddrFunctions = UvmAddrFunctions {
    uaddr_select: uaddr_stack_brk_select,
    uaddr_free_insert: None,
    uaddr_free_remove: None,
    uaddr_destroy,
    uaddr_name: "uaddr_stckbrk",
};

/// `uaddr_stack_brk_select`: stack/brk address selector.
pub fn uaddr_stack_brk_select<'m>(
    map: &'m VmMap,
    uaddr: &'m UvmAddrState,
    sz: usize,
    align: usize,
    offset: usize,
    _prot: VmProt,
    _hint: usize,
) -> Result<(&'m VmMapEntry, usize), Errno> {
    // Set up brk search strategy.
    let start = map.b_start.get().max(uaddr.uaddr_minaddr.get());
    let end = map.b_end.get().min(uaddr.uaddr_maxaddr.get());
    let before_gap = 0;
    let after_gap = 0;
    let dir = -1; // Opposite of brk() growth.

    if end.wrapping_sub(start) >= sz
        && let Ok(found) = uvm_addr_linsearch(
            map,
            uaddr,
            0,
            sz,
            align,
            offset,
            dir,
            start,
            end.wrapping_sub(sz),
            before_gap,
            after_gap,
        )
    {
        return Ok(found);
    }

    // Set up stack search strategy.
    let start = map.s_start.get().max(uaddr.uaddr_minaddr.get());
    let end = map.s_end.get().min(uaddr.uaddr_maxaddr.get());
    let before_gap = ((arc4random() & 0x3) as usize + 1) << PAGE_SHIFT;
    let after_gap = ((arc4random() & 0x3) as usize + 1) << PAGE_SHIFT;
    let dir = 1; // MACHINE_STACK_GROWS_UP is not defined.
    let span = end.wrapping_sub(start);
    if span >= before_gap + after_gap
        && span - before_gap - after_gap >= sz
        && let Ok(found) = uvm_addr_linsearch(
            map,
            uaddr,
            0,
            sz,
            align,
            offset,
            dir,
            start,
            end.wrapping_sub(sz),
            before_gap,
            after_gap,
        )
    {
        return Ok(found);
    }

    Err(Errno::ENOMEM)
}

/// `uaddr_stack_brk_create`.
pub fn uaddr_stack_brk_create(minaddr: usize, maxaddr: usize) -> &'static UvmAddrState {
    let Some(mem) = pool_get(&UADDR_POOL, PR_WAITOK) else {
        panic(format_args!("uaddr_stack_brk_create: uaddr_pool is empty"));
    };
    let p = mem.cast::<UvmAddrState>();
    // SAFETY: a fresh, suitably aligned pool item of `size_of::<UvmAddrState>()` bytes,
    // written once before anything else sees it; it lives until `uaddr_destroy`.
    unsafe {
        p.as_ptr().write(UvmAddrState {
            uaddr_minaddr: Cell::new(minaddr),
            uaddr_maxaddr: Cell::new(maxaddr),
            uaddr_functions: &UADDR_STACK_BRK_FUNCTIONS,
        });
        p.as_ref()
    }
}

/// `uvm_mapent_fspace_cmp`: free space comparison. Compares smaller free-space before
/// larger free-space.
pub fn uvm_mapent_fspace_cmp(e1: &VmMapEntry, e2: &VmMapEntry) -> core::cmp::Ordering {
    e1.fspace
        .get()
        .cmp(&e2.fspace.get())
        .then(e1.start.get().cmp(&e2.start.get()))
}

tree_adapter!(
    /// `uaddr_free_rbtree`: the free-space tree of the bestfit and pivot selectors
    /// (`dfree.rbtree`), ordered by `uvm_mapent_fspace_cmp`.
    pub UaddrFreeRbtree: VmMapEntry, rbtree => RbtEntry, uvm_mapent_fspace_cmp
);
/* </CODE> */

/* <TESTS> */
#[cfg(test)]
mod tests {
    use std::{assert, assert_eq};

    use super::*;

    #[test]
    fn fitspace_finds_the_bounds() {
        let p = PAGE_SIZE;
        // Four pages, two wanted: lowest and highest start.
        assert_eq!(
            uvm_addr_fitspace(10 * p, 14 * p, 2 * p, 0, 0, 0, 0),
            Ok((10 * p, 12 * p))
        );
        // Gaps eat into both ends.
        assert_eq!(
            uvm_addr_fitspace(10 * p, 14 * p, 2 * p, 0, 0, p, p),
            Ok((11 * p, 11 * p))
        );
        // Too small, with and without gaps.
        assert_eq!(
            uvm_addr_fitspace(10 * p, 11 * p, 2 * p, 0, 0, 0, 0),
            Err(Errno::ENOMEM)
        );
        assert_eq!(
            uvm_addr_fitspace(10 * p, 14 * p, 2 * p, 0, 0, 2 * p, p),
            Err(Errno::ENOMEM)
        );
        // Alignment moves the lowest start up and the highest down.
        assert_eq!(
            uvm_addr_fitspace(9 * p, 20 * p, p, 4 * p, 0, 0, 0),
            Ok((12 * p, 16 * p))
        );
        assert_eq!(
            uvm_addr_fitspace(9 * p, 20 * p, p, 4 * p, p, 0, 0),
            Ok((9 * p, 17 * p))
        );
        // Inverted range.
        assert_eq!(
            uvm_addr_fitspace(14 * p, 10 * p, p, 0, 0, 0, 0),
            Err(Errno::ENOMEM)
        );
    }

    #[test]
    fn pivot_random_is_pages() {
        for _ in 0..256 {
            let r = uaddr_pivot_random();
            assert!(r >= PAGE_SIZE && r & PAGE_MASK == 0);
            assert!(r <= (PIVOT_RND as usize) << PAGE_SHIFT);
        }
    }

    #[test]
    fn fspace_cmp_orders_by_size_then_address() {
        let a = VmMapEntry::new();
        let b = VmMapEntry::new();
        a.fspace.set(PAGE_SIZE);
        b.fspace.set(2 * PAGE_SIZE);
        assert_eq!(uvm_mapent_fspace_cmp(&a, &b), core::cmp::Ordering::Less);
        b.fspace.set(PAGE_SIZE);
        a.start.set(0x1000);
        b.start.set(0x2000);
        assert_eq!(uvm_mapent_fspace_cmp(&a, &b), core::cmp::Ordering::Less);
        assert_eq!(uvm_mapent_fspace_cmp(&b, &a), core::cmp::Ordering::Greater);
        assert_eq!(uvm_mapent_fspace_cmp(&a, &a), core::cmp::Ordering::Equal);
    }
}
/* </TESTS> */
