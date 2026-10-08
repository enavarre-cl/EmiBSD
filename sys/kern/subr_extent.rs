/*	$OpenBSD: subr_extent.c,v 1.65 2024/01/19 22:12:24 kettenis Exp $	*/
/*	$NetBSD: subr_extent.c,v 1.7 1996/11/21 18:46:34 cgd Exp $	*/
/* <LICENSES> */
/*-
 * Copyright (c) 1996, 1998 The NetBSD Foundation, Inc.
 * All rights reserved.
 *
 * This code is derived from software contributed to The NetBSD Foundation
 * by Jason R. Thorpe and Matthias Drochner.
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
 * THIS SOFTWARE IS PROVIDED BY THE NETBSD FOUNDATION, INC. AND CONTRIBUTORS
 * ``AS IS'' AND ANY EXPRESS OR IMPLIED WARRANTIES, INCLUDING, BUT NOT LIMITED
 * TO, THE IMPLIED WARRANTIES OF MERCHANTABILITY AND FITNESS FOR A PARTICULAR
 * PURPOSE ARE DISCLAIMED.  IN NO EVENT SHALL THE FOUNDATION OR CONTRIBUTORS
 * BE LIABLE FOR ANY DIRECT, INDIRECT, INCIDENTAL, SPECIAL, EXEMPLARY, OR
 * CONSEQUENTIAL DAMAGES (INCLUDING, BUT NOT LIMITED TO, PROCUREMENT OF
 * SUBSTITUTE GOODS OR SERVICES; LOSS OF USE, DATA, OR PROFITS; OR BUSINESS
 * INTERRUPTION) HOWEVER CAUSED AND ON ANY THEORY OF LIABILITY, WHETHER IN
 * CONTRACT, STRICT LIABILITY, OR TORT (INCLUDING NEGLIGENCE OR OTHERWISE)
 * ARISING IN ANY WAY OUT OF THE USE OF THIS SOFTWARE, EVEN IF ADVISED OF THE
 * POSSIBILITY OF SUCH DAMAGE.
 */
/* </LICENSES> */

//! The general purpose extent manager (`extent(9)`): `kern/subr_extent.c`.
//!
//! Upstream: sys/kern/subr_extent.c @ 3ce1f3f79392
//!
//! An extent keeps the regions allocated in a range of numbers as a sorted list of
//! descriptors, coalescing neighbours unless it was created `EX_NOCOALESCE`. A region is
//! allocated at a given place (`extent_alloc_region`) or wherever it fits in a subrange, with
//! an alignment, a skew and a boundary it must not cross (`extent_alloc_subregion`, best fit
//! or `EX_FAST` first fit); `extent_free` gives a range back, splitting a region if needed. A
//! filled extent (`EX_FILLED`) starts with everything allocated, so freeing describes what is
//! available (the PCI host bridges' windows). Descriptors come from a pool, or from the
//! caller's storage for a fixed extent.
//!
//! The list of all extents and `extent_print`/`extent_print_all` are compiled
//! (`DIAGNOSTIC || DDB`: GENERIC has `option DDB`); the argument checks that panic are under
//! feature `diagnostic`, and without it a region outside the extent fails with `EINVAL`, as
//! in C. The `_EXTENT_TESTING` userland harness is replaced by host tests.
//!
//! ## Deviations
//! - Errors are `Err(Errno)` and the allocated start is the `Ok` value (the C's `int` and
//!   `u_long *result`).
//! - A fixed extent's storage is a `&'static mut [u8]`; the `struct extent_fixed` is placed
//!   at its first address aligned for it (the C assumes the caller aligned it), and storage
//!   too small for it panics with or without `DIAGNOSTIC` (the C writes past it without).
//! - The C's NULL checks of `name`, `ex`, `myrp` and `result`, and the "storage provided for
//!   non-fixed" check, have no counterpart: the types rule them out.
//! - `extent_register`'s `initialized`/`LIST_INIT` is the list head's constant initializer.
//! - The functions that take a caller's descriptor (`*_with_descr`) are `unsafe`: the
//!   descriptor must be in no list and stay valid while the extent links it.
//! - `extent_destroy` is `unsafe`: the extent must not be used afterwards.

use core::cell::Cell;
use core::fmt;
use core::ptr::{self, NonNull};
use core::sync::atomic::{AtomicBool, Ordering};

use crate::kern::kern_malloc::{free, malloc};
use crate::kern::kern_synch::{tsleep_nsec, wakeup};
use crate::kern::subr_pool::{pool_get, pool_init, pool_put};
use crate::kern::subr_prf::{Bitmask, Str, db_printf, panic, printf};
use crate::machine::intr::IPL_VM;
use crate::sys::errno::Errno;
use crate::sys::extent::{
    ER_ALLOC, ER_DISCARD, EX_BOUNDZERO, EX_CATCH, EX_CONFLICTOK, EX_FAST, EX_FILLED, EX_MALLOCOK,
    EX_NOCOALESCE, EX_WAITOK, EX_WAITSPACE, EXF_BITS, EXF_FIXED, EXF_FLWANTED, EXF_NOCOALESCE,
    EXF_WANTED, Extent, ExtentFixed, ExtentList, ExtentRegion, ExtentRegionList,
};
use crate::sys::malloc::{M_NOWAIT, M_WAITOK};
use crate::sys::param::{PCATCH, PRIBIO, align};
use crate::sys::pool::{PR_NOWAIT, PR_WAITOK, Pool};
use crate::sys::queue::{ListEntry, ListHead};
use crate::sys::systm::INFSLP;

/// `LIST_HEAD(listhead, extent) ext_list`: every extent, for `extent_print_all`.
struct ExtListHead(ListHead<ExtentList>);

// SAFETY: the list is changed by `extent_create` and `extent_destroy`, which their callers
// serialize (autoconfiguration under the kernel lock), as the C's unlocked list.
unsafe impl Sync for ExtListHead {}

/// `ext_list`.
static EXT_LIST: ExtListHead = ExtListHead(ListHead::new());

/// `ex_region_pl`: the pool of dynamically allocated region descriptors.
pub static EX_REGION_PL: Pool = Pool::new();

/// `extent_pool_init`'s `inited`.
static EX_REGION_PL_INITED: AtomicBool = AtomicBool::new(false);

/// `extent_align(start, align, skew)`: shortcut to align to an arbitrary power-of-two
/// boundary.
const fn extent_align(start: u64, align: u64, skew: u64) -> u64 {
    (start.wrapping_sub(skew).wrapping_add(align.wrapping_sub(1)) & align.wrapping_neg())
        .wrapping_add(skew)
}

/// `LE_OV(x, y, z)`: whether `x + y <= z`, failing if the sum overflows.
const fn le_ov(x: u64, y: u64, z: u64) -> bool {
    let sum = x.wrapping_add(y);
    sum >= x && sum <= z
}

/// The `struct extent_fixed` around a fixed extent.
fn extent_fixed(ex: &Extent) -> &ExtentFixed {
    debug_assert!(ex.ex_flags.get() & EXF_FIXED != 0);
    // SAFETY: `EXF_FIXED` is set only by `extent_create` on the `fex_extent` of an
    // `ExtentFixed` (`#[repr(C)]`, the extent first) in the caller's storage, which lives
    // as long as the extent.
    unsafe { &*ptr::from_ref(ex).cast::<ExtentFixed>() }
}

/// `extent_register(ex)`: puts the extent on the list of all extents.
fn extent_register(ex: &'static Extent) {
    #[cfg(feature = "diagnostic")]
    for ep in EXT_LIST.0.iter() {
        if ptr::eq(ep, ex) {
            panic(format_args!("extent_register: already registered"));
        }
    }

    // Insert into list
    // SAFETY: a new extent is in no list, and lives until `extent_destroy` unlinks it.
    unsafe { EXT_LIST.0.insert_head(ex) };
}

/// `extent_pool_init()`: sets up the descriptor pool once.
fn extent_pool_init() {
    if !EX_REGION_PL_INITED.load(Ordering::Acquire) {
        pool_init(
            &EX_REGION_PL,
            size_of::<ExtentRegion>(),
            0,
            IPL_VM,
            0,
            "extentpl",
            None,
        );
        EX_REGION_PL_INITED.store(true, Ordering::Release);
    }
}

/// `extent_print_all()`: prints out all extents registered (DDB `show extents`).
pub fn extent_print_all() {
    for ep in EXT_LIST.0.iter() {
        extent_print1(ep, db_printf);
    }
}

/// A new extent descriptor over `[start, end]`, with no regions.
fn extent_new(name: &'static [u8], start: u64, end: u64, mtype: i32, flags: i32) -> Extent {
    Extent {
        ex_name: name,
        ex_regions: ListHead::new(),
        ex_start: start,
        ex_end: end,
        ex_mtype: mtype,
        ex_flags: Cell::new(flags),
        ex_link: ListEntry::new(),
    }
}

/// `extent_create(name, start, end, mtype, storage, storagesize, flags)`: allocates and
/// initializes an extent map over `[start, end]`; with `storage` it is a fixed extent whose
/// descriptors live there. `None` when memory is short (`EX_WAITOK` not given).
pub fn extent_create(
    name: &'static [u8],
    start: u64,
    end: u64,
    mtype: i32,
    storage: Option<&'static mut [u8]>,
    flags: i32,
) -> Option<&'static Extent> {
    let fixed_extent = storage.is_some();

    #[cfg(feature = "diagnostic")]
    {
        // Check arguments.
        if end < start {
            printf(format_args!(
                "extent_create: extent `{}', start 0x{:x}, end 0x{:x}\n",
                Str(name),
                start,
                end
            ));
            panic(format_args!("extent_create: end < start"));
        }
    }

    extent_pool_init();

    // Allocate extent descriptor.
    let ex: &'static Extent = if let Some(storage) = storage {
        storage.fill(0);
        let storagesize = storage.len();
        let base = storage.as_mut_ptr();

        // Align all descriptors on "long" boundaries (and the fixed extent itself: see the
        // deviations).
        let pad = base.align_offset(align_of::<ExtentFixed>());
        if pad > storagesize || storagesize - pad < size_of::<ExtentFixed>() {
            panic(format_args!(
                "extent_create: fixed extent, bad storagesize 0x{:x}",
                storagesize
            ));
        }
        let mut cp = pad;
        let mut sz = storagesize - pad;

        // SAFETY: `pad` aligns the address for `ExtentFixed`, and `sz` covers its size.
        let fex = unsafe { base.add(cp) }.cast::<ExtentFixed>();
        // SAFETY: as above; the storage is the caller's, for good (`'static`). Fill in the
        // extent descriptor (below for the flags).
        let fex: &'static ExtentFixed = unsafe {
            fex.write(ExtentFixed {
                fex_extent: extent_new(name, start, end, mtype, EXF_FIXED),
                fex_freelist: ListHead::new(),
                fex_storage: base,
                fex_storagesize: storagesize,
            });
            &*fex
        };
        cp += align(size_of::<ExtentFixed>());
        sz = sz.saturating_sub(align(size_of::<ExtentFixed>()));

        // In a fixed extent, we have to pre-allocate region descriptors and place them in
        // the extent's freelist.
        while sz >= align(size_of::<ExtentRegion>()) {
            // SAFETY: `cp` stays aligned (multiples of `ALIGN` past an aligned start) and
            // `sz` covers the descriptor; the storage lives for good.
            let rp: &'static ExtentRegion = unsafe {
                let rp = base.add(cp).cast::<ExtentRegion>();
                rp.write(ExtentRegion::new());
                &*rp
            };
            cp += align(size_of::<ExtentRegion>());
            sz -= align(size_of::<ExtentRegion>());
            // SAFETY: a fresh descriptor, in no list, in storage that lives for good.
            unsafe { fex.fex_freelist.insert_head(rp) };
        }
        &fex.fex_extent
    } else {
        let p = malloc(
            size_of::<Extent>(),
            mtype,
            if flags & EX_WAITOK != 0 {
                M_WAITOK
            } else {
                M_NOWAIT
            },
        )?
        .cast::<Extent>();
        // SAFETY: a fresh allocation of an extent's size, written once before use and
        // freed only by `extent_destroy`.
        unsafe {
            p.write(extent_new(name, start, end, mtype, 0));
            &*p.as_ptr()
        }
    };

    // Fill in the extent descriptor and return it to the caller.
    if flags & EX_NOCOALESCE != 0 {
        ex.ex_flags.set(ex.ex_flags.get() | EXF_NOCOALESCE);
    }

    if flags & EX_FILLED != 0 {
        let Some(rp) = extent_alloc_region_descriptor(ex, flags) else {
            if !fixed_extent {
                free(NonNull::from(ex).cast(), mtype, size_of::<Extent>());
            }
            return None;
        };
        rp.er_start.set(start);
        rp.er_end.set(end);
        // SAFETY: a descriptor just taken, in no list; the extent is new and in place.
        unsafe { ex.ex_regions.insert_head(rp) };
    }

    // DIAGNOSTIC || DDB
    extent_register(ex);
    Some(ex)
}

/// `extent_destroy(ex)`: destroys an extent map.
///
/// # Safety
///
/// `ex` came from `extent_create` and is not used afterwards (its memory is freed when it
/// is not a fixed extent).
pub unsafe fn extent_destroy(ex: &'static Extent) {
    // Free all region descriptors in extent.
    let mut rp = ex.ex_regions.first();
    while let Some(orp) = rp {
        rp = ListHead::<ExtentRegionList>::next(orp);
        // SAFETY: `orp` is linked in the extent's region list.
        unsafe { ListHead::<ExtentRegionList>::remove(orp) };
        extent_free_region_descriptor(ex, orp);
    }

    // DIAGNOSTIC || DDB: remove from the list of all extents.
    // SAFETY: `extent_create` registered it.
    unsafe { ListHead::<ExtentList>::remove(ex) };

    // If we're not a fixed extent, free the extent descriptor itself.
    if ex.ex_flags.get() & EXF_FIXED == 0 {
        free(NonNull::from(ex).cast(), ex.ex_mtype, size_of::<Extent>());
    }
}

/// `extent_insert_and_optimize(ex, start, size, after, rp)`: inserts a region into the sorted
/// region list after `after`, or at the head when `None`, coalescing with its neighbours when
/// allowed; `rp` is freed when it is not needed.
fn extent_insert_and_optimize(
    ex: &Extent,
    start: u64,
    size: u64,
    after: Option<&ExtentRegion>,
    rp: &ExtentRegion,
) {
    let Some(after) = after else {
        // We're the first in the region list. If there's a region after us, attempt to
        // coalesce to save descriptor overhead.
        if ex.ex_flags.get() & EXF_NOCOALESCE == 0
            && let Some(first) = ex.ex_regions.first()
            && start.wrapping_add(size) == first.er_start.get()
        {
            // We can coalesce. Prepend us to the first region.
            first.er_start.set(start);
            extent_free_region_descriptor(ex, rp);
            return;
        }

        // Can't coalesce. Fill in the region descriptor in, and insert us at the head of
        // the region list.
        rp.er_start.set(start);
        rp.er_end.set(start.wrapping_add(size.wrapping_sub(1)));
        // SAFETY: `rp` is a descriptor in no list (the allocators' contract) that lives as
        // long as the extent links it; the extent stays in place.
        unsafe { ex.ex_regions.insert_head(rp) };
        return;
    };

    // If EXF_NOCOALESCE is set, coalescing is disallowed.
    if ex.ex_flags.get() & EXF_NOCOALESCE == 0 {
        let mut appended = false;

        // Attempt to coalesce with the region before us.
        if after.er_end.get().wrapping_add(1) == start {
            // We can coalesce. Append ourselves and make note of it.
            after.er_end.set(start.wrapping_add(size.wrapping_sub(1)));
            appended = true;
        }

        // Attempt to coalesce with the region after us.
        if let Some(nextr) = ListHead::<ExtentRegionList>::next(after)
            && start.wrapping_add(size) == nextr.er_start.get()
        {
            // We can coalesce. Note that if we appended ourselves to the previous region, we
            // exactly fit the gap, and can free the "next" region descriptor.
            if appended {
                // Yup, we can free it up.
                after.er_end.set(nextr.er_end.get());
                // SAFETY: `nextr` is linked after `after`.
                unsafe { ListHead::<ExtentRegionList>::remove(nextr) };
                extent_free_region_descriptor(ex, nextr);
            } else {
                // Nope, just prepend us to the next region.
                nextr.er_start.set(start);
            }

            extent_free_region_descriptor(ex, rp);
            return;
        }

        // We weren't able to coalesce with the next region, but we don't need to allocate a
        // region descriptor if we appended ourselves to the previous region.
        if appended {
            extent_free_region_descriptor(ex, rp);
            return;
        }
    }

    // cant_coalesce: fill in the region descriptor and insert ourselves into the region
    // list.
    rp.er_start.set(start);
    rp.er_end.set(start.wrapping_add(size.wrapping_sub(1)));
    // SAFETY: `after` is linked; `rp` is in no list (as above).
    unsafe { ListHead::<ExtentRegionList>::insert_after(after, rp) };
}

/// The sleep `EX_WAITSPACE` (and a fixed extent's empty freelist) takes:
/// `tsleep_nsec(chan, PRIBIO | (EX_CATCH ? PCATCH : 0), "extnt", INFSLP)`.
fn extent_sleep<T>(chan: *const T, flags: i32) -> Result<(), Errno> {
    let pri = PRIBIO | if flags & EX_CATCH != 0 { PCATCH } else { 0 };
    tsleep_nsec(chan, pri, "extnt", INFSLP)
}

/// `extent_do_alloc_region(ex, start, size, flags, myrp)`: allocates a specific region in an
/// extent map, with the descriptor `myrp`.
pub fn extent_do_alloc_region(
    ex: &Extent,
    start: u64,
    size: u64,
    flags: i32,
    myrp: &ExtentRegion,
) -> Result<(), Errno> {
    let mut start = start;
    let mut size = size;
    let mut end = start.wrapping_add(size.wrapping_sub(1));

    #[cfg(feature = "diagnostic")]
    {
        // Check arguments.
        if size < 1 {
            printf(format_args!(
                "extent_do_alloc_region: extent `{}', size 0x{:x}\n",
                Str(ex.ex_name),
                size
            ));
            panic(format_args!("extent_do_alloc_region: bad size"));
        }
        if end < start {
            printf(format_args!(
                "extent_do_alloc_region: extent `{}', start 0x{:x}, size 0x{:x}\n",
                Str(ex.ex_name),
                start,
                size
            ));
            panic(format_args!("extent_do_alloc_region: overflow"));
        }
        if flags & EX_CONFLICTOK != 0 && flags & EX_WAITSPACE != 0 {
            panic(format_args!(
                "extent_do_alloc_region: EX_CONFLICTOK and EX_WAITSPACE are mutually exclusive"
            ));
        }
    }

    // Make sure the requested region lies within the extent.
    if start < ex.ex_start || end > ex.ex_end {
        #[cfg(feature = "diagnostic")]
        {
            printf(format_args!(
                "extent_do_alloc_region: extent `{}' (0x{:x} - 0x{:x})\n",
                Str(ex.ex_name),
                ex.ex_start,
                ex.ex_end
            ));
            printf(format_args!(
                "extent_do_alloc_region: start 0x{:x}, end 0x{:x}\n",
                start, end
            ));
            panic(format_args!(
                "extent_do_alloc_region: region lies outside extent"
            ));
        }
        #[cfg(not(feature = "diagnostic"))]
        {
            extent_free_region_descriptor(ex, myrp);
            return Err(Errno::EINVAL);
        }
    }

    'alloc_start: loop {
        // Attempt to place ourselves in the desired area of the extent. We save ourselves
        // some work by keeping the list sorted. In other words, if the start of the current
        // region is greater than the end of our region, we don't have to search any further.
        //
        // Keep a pointer to the last region we looked at so that we don't have to traverse
        // the list again when we insert ourselves. If "last" is NULL when we finally insert
        // ourselves, we go at the head of the list. See extent_insert_and_optimize() for
        // details.
        let mut last: Option<&ExtentRegion> = None;

        for rp in ex.ex_regions.iter() {
            if rp.er_start.get() > end {
                // We lie before this region and don't conflict.
                break;
            }

            // The current region begins before we end. Check for a conflict.
            if rp.er_end.get() >= start {
                // We conflict. If we can (and want to) wait, do so.
                if flags & EX_WAITSPACE != 0 {
                    ex.ex_flags.set(ex.ex_flags.get() | EXF_WANTED);
                    extent_sleep(ptr::from_ref(ex), flags)?;
                    continue 'alloc_start;
                }

                // If we tolerate conflicts adjust things such that all space in the
                // requested region is allocated.
                if flags & EX_CONFLICTOK != 0 {
                    // There are four possibilities:
                    //
                    // 1. The current region overlaps with the start of the requested
                    //    region. Adjust the requested region to start at the end of the
                    //    current region and try again.
                    //
                    // 2. The current region falls completely within the requested region.
                    //    Free the current region and try again.
                    //
                    // 3. The current region overlaps with the end of the requested region.
                    //    Adjust the requested region to end at the start of the current
                    //    region and try again.
                    //
                    // 4. The requested region falls completely within the current region.
                    //    We're done.
                    if rp.er_start.get() <= start {
                        if rp.er_end.get() < ex.ex_end {
                            start = rp.er_end.get() + 1;
                            size = end.wrapping_sub(start).wrapping_add(1);
                            continue 'alloc_start;
                        }
                    } else if rp.er_end.get() < end {
                        // SAFETY: `rp` is linked in the extent's region list.
                        unsafe { ListHead::<ExtentRegionList>::remove(rp) };
                        extent_free_region_descriptor(ex, rp);
                        continue 'alloc_start;
                    } else if rp.er_start.get() < end && rp.er_start.get() > ex.ex_start {
                        end = rp.er_start.get() - 1;
                        size = end.wrapping_sub(start).wrapping_add(1);
                        continue 'alloc_start;
                    }
                    return Ok(());
                }

                extent_free_region_descriptor(ex, myrp);
                return Err(Errno::EAGAIN);
            }
            // We don't conflict, but this region lies before us. Keep a pointer to this
            // region, and keep trying.
            last = Some(rp);
        }

        // We don't conflict with any regions. "last" points to the region we fall after, or
        // is NULL if we belong at the beginning of the region list. Insert ourselves.
        extent_insert_and_optimize(ex, start, size, last, myrp);
        return Ok(());
    }
}

/// `extent_alloc_region(ex, start, size, flags)`: allocates `[start, start + size - 1]`.
pub fn extent_alloc_region(ex: &Extent, start: u64, size: u64, flags: i32) -> Result<(), Errno> {
    // Allocate the region descriptor. It will be freed later if we can coalesce with
    // another region.
    let Some(rp) = extent_alloc_region_descriptor(ex, flags) else {
        #[cfg(feature = "diagnostic")]
        printf(format_args!(
            "extent_alloc_region: can't allocate region descriptor\n"
        ));
        return Err(Errno::ENOMEM);
    };

    extent_do_alloc_region(ex, start, size, flags, rp)
}

/// `extent_alloc_region_with_descr(ex, start, size, flags, rp)`: as `extent_alloc_region`,
/// with the caller's descriptor (`ER_DISCARD`: never freed to a pool), in an `EX_NOCOALESCE`
/// extent.
///
/// # Safety
///
/// `rp` is in no list and stays valid and in place while the extent links it.
pub unsafe fn extent_alloc_region_with_descr(
    ex: &Extent,
    start: u64,
    size: u64,
    flags: i32,
    rp: &ExtentRegion,
) -> Result<(), Errno> {
    #[cfg(feature = "diagnostic")]
    if ex.ex_flags.get() & EXF_NOCOALESCE == 0 {
        panic(format_args!(
            "extent_alloc_region_with_descr: EX_NOCOALESCE not set"
        ));
    }

    rp.er_flags.set(ER_DISCARD);
    extent_do_alloc_region(ex, start, size, flags, rp)
}

/// The end of `extent_do_alloc`'s search.
enum Fit<'a> {
    /// `found`: insert at this start, after this region.
    Found(u64, Option<&'a ExtentRegion>),
    /// `fail`: no exact (or, with `EX_FAST`, first) fit.
    Fail,
}

/// `extent_do_alloc`'s boundary check of a candidate at `newstart` (written twice in the C):
/// `Ok` with the (possibly moved) start, `Err(true)` when the moved candidate no longer fits
/// (`fits` says; the C's `goto skip` in the loop, `goto fail` after it), `Err(false)` when
/// the request can't fit (`goto fail`).
fn extent_boundary_check(
    ex: &Extent,
    newstart: u64,
    size: u64,
    boundary: u64,
    flags: i32,
    fits: impl Fn(u64) -> bool,
) -> Result<u64, bool> {
    let mut newstart = newstart;
    let newend = newstart.wrapping_add(size.wrapping_sub(1));

    // Calculate the next boundary after the start of this region.
    let skew = if flags & EX_BOUNDZERO != 0 {
        0
    } else {
        ex.ex_start
    };
    let mut dontcross = extent_align(newstart.wrapping_add(1), boundary, skew).wrapping_sub(1);

    // Check for overflow
    if dontcross < ex.ex_start {
        dontcross = ex.ex_end;
    } else if newend > dontcross {
        // Candidate region crosses boundary. Throw away the leading part and see if we
        // still fit.
        newstart = dontcross.wrapping_add(1);
        dontcross = dontcross.wrapping_add(boundary);
        if !fits(newstart) {
            return Err(true);
        }
    }

    // If we run past the end of the extent or the boundary overflows, then the request
    // can't fit.
    if newstart.wrapping_add(size).wrapping_sub(1) > ex.ex_end || dontcross < newstart {
        return Err(false);
    }

    Ok(newstart)
}

/// `extent_do_alloc(ex, substart, subend, size, alignment, skew, boundary, flags, myrp,
/// result)`: allocates `size` in `[substart, subend]`, aligned to `alignment` (a power of 2)
/// after `skew`, not crossing a multiple of `boundary` (0: no boundary); the smallest gap
/// that holds it, or the first with `EX_FAST`. Returns the start.
#[allow(clippy::too_many_arguments)] // the C's signature
pub fn extent_do_alloc(
    ex: &Extent,
    substart: u64,
    subend: u64,
    size: u64,
    alignment: u64,
    skew: u64,
    boundary: u64,
    flags: i32,
    myrp: &ExtentRegion,
) -> Result<u64, Errno> {
    #[cfg(feature = "diagnostic")]
    {
        // Check arguments.
        if substart < ex.ex_start
            || substart > ex.ex_end
            || subend > ex.ex_end
            || subend < ex.ex_start
        {
            printf(format_args!(
                "extent_do_alloc: extent `{}', ex_start 0x{:x}, ex_end 0x{:x}\n",
                Str(ex.ex_name),
                ex.ex_start,
                ex.ex_end
            ));
            printf(format_args!(
                "extent_do_alloc: substart 0x{:x}, subend 0x{:x}\n",
                substart, subend
            ));
            panic(format_args!("extent_do_alloc: bad subregion"));
        }
        if size < 1 || size - 1 > subend.wrapping_sub(substart) {
            printf(format_args!(
                "extent_do_alloc: extent `{}', size 0x{:x}\n",
                Str(ex.ex_name),
                size
            ));
            panic(format_args!("extent_do_alloc: bad size"));
        }
        if alignment == 0 {
            panic(format_args!("extent_do_alloc: bad alignment"));
        }
        if boundary != 0 && boundary < size {
            printf(format_args!(
                "extent_do_alloc: extent `{}', size 0x{:x}, boundary 0x{:x}\n",
                Str(ex.ex_name),
                size,
                boundary
            ));
            panic(format_args!("extent_do_alloc: bad boundary"));
        }
    }

    'alloc_start: loop {
        // Keep a pointer to the last region we looked at so that we don't have to traverse
        // the list again when we insert ourselves. If "last" is NULL when we finally insert
        // ourselves, we go at the head of the list. See extent_insert_and_optimize() for
        // details.
        let mut last: Option<&ExtentRegion> = None;

        // Keep track of size and location of the smallest chunk we fit in.
        //
        // Since the extent can be as large as the numeric range of the CPU (0 - 0xffffffff
        // for 32-bit systems), the best overhead value can be the maximum unsigned integer.
        // Thus, we initialize "bestovh" to 0, since we insert ourselves into the region list
        // immediately on an exact match (which is the only case where "bestovh" would be set
        // to 0).
        let mut bestovh: u64 = 0;
        let mut beststart: u64 = 0;
        let mut bestlast: Option<&ExtentRegion> = None;

        // Keep track of end of free region. This is either the end of extent or the start of
        // a region past the subend.
        let mut exend = ex.ex_end;

        // For N allocated regions, we must make (N + 1) checks for unallocated space. The
        // first chunk we check is the area from the beginning of the subregion to the first
        // allocated region after that point.
        let mut newstart = extent_align(substart, alignment, skew);
        if newstart < ex.ex_start {
            #[cfg(feature = "diagnostic")]
            {
                printf(format_args!(
                    "extent_do_alloc: extent `{}' (0x{:x} - 0x{:x}), alignment 0x{:x}\n",
                    Str(ex.ex_name),
                    ex.ex_start,
                    ex.ex_end,
                    alignment
                ));
                panic(format_args!("extent_do_alloc: overflow after alignment"));
            }
            #[cfg(not(feature = "diagnostic"))]
            {
                extent_free_region_descriptor(ex, myrp);
                return Err(Errno::EINVAL);
            }
        }

        // Find the first allocated region that begins on or after the subregion start,
        // advancing the "last" pointer along the way.
        let mut rp = ex.ex_regions.first();
        while let Some(r) = rp {
            if r.er_start.get() >= newstart {
                break;
            }
            last = Some(r);
            rp = ListHead::<ExtentRegionList>::next(r);
        }

        // Relocate the start of our candidate region to the end of the last allocated region
        // (if there was one overlapping our subrange).
        if let Some(l) = last
            && l.er_end.get() >= newstart
        {
            newstart = extent_align(l.er_end.get().wrapping_add(1), alignment, skew);
        }

        let fit = 'scan: {
            while let Some(r) = rp {
                // If the region pasts the subend, bail out and see if we fit against the
                // subend.
                if r.er_start.get() > subend {
                    exend = r.er_start.get();
                    break;
                }

                // Check the chunk before "rp". Note that our comparison is safe from
                // overflow conditions.
                if le_ov(newstart, size, r.er_start.get()) {
                    // Do a boundary check, if necessary. Note that a region may *begin* on
                    // the boundary, but it must end before the boundary.
                    let checked = if boundary != 0 {
                        extent_boundary_check(ex, newstart, size, boundary, flags, |ns| {
                            le_ov(ns, size, r.er_start.get())
                        })
                    } else {
                        Ok(newstart)
                    };
                    match checked {
                        Err(false) => break 'scan Fit::Fail,
                        // skip
                        Err(true) => {}
                        Ok(ns) => {
                            newstart = ns;
                            // We would fit into this space. Calculate the overhead (wasted
                            // space). If we exactly fit, or we're taking the first fit,
                            // insert ourselves into the region list.
                            let ovh = r.er_start.get() - newstart - size;
                            if flags & EX_FAST != 0 || ovh == 0 {
                                break 'scan Fit::Found(newstart, last);
                            }

                            // Don't exactly fit, but check to see if we're better than any
                            // current choice.
                            if bestovh == 0 || ovh < bestovh {
                                bestovh = ovh;
                                beststart = newstart;
                                bestlast = last;
                            }
                        }
                    }
                }

                // skip: skip past the current region and check again.
                newstart = extent_align(r.er_end.get().wrapping_add(1), alignment, skew);
                if newstart < r.er_end.get() {
                    // Overflow condition. Don't error out, since we might have a chunk of
                    // space that we can use.
                    break 'scan Fit::Fail;
                }

                last = Some(r);
                rp = ListHead::<ExtentRegionList>::next(r);
            }

            // The final check is from the current starting point to the end of the
            // subregion. If there were no allocated regions, "newstart" is set to the
            // beginning of the subregion, or just past the end of the last allocated region,
            // adjusted for alignment in either case.
            if le_ov(newstart, size.wrapping_sub(1), subend) {
                // Do a boundary check, if necessary. Note that a region may *begin* on the
                // boundary, but it must end before the boundary.
                if boundary != 0 {
                    match extent_boundary_check(ex, newstart, size, boundary, flags, |ns| {
                        le_ov(ns, size.wrapping_sub(1), subend)
                    }) {
                        Ok(ns) => newstart = ns,
                        Err(_) => break 'scan Fit::Fail,
                    }
                }

                // We would fit into this space. Calculate the overhead (wasted space). If we
                // exactly fit, or we're taking the first fit, insert ourselves into the region
                // list.
                let ovh = exend
                    .wrapping_sub(newstart)
                    .wrapping_sub(size.wrapping_sub(1));
                if flags & EX_FAST != 0 || ovh == 0 {
                    break 'scan Fit::Found(newstart, last);
                }

                // Don't exactly fit, but check to see if we're better than any current
                // choice.
                if bestovh == 0 || ovh < bestovh {
                    bestovh = ovh;
                    beststart = newstart;
                    bestlast = last;
                }
            }

            Fit::Fail
        };

        let (start, after) = match fit {
            Fit::Found(start, after) => (start, after),
            // fail: one of the following two conditions have occurred:
            //
            //	There is no chunk large enough to hold the request.
            //
            //	If EX_FAST was not specified, there is not an exact match for the request.
            //
            // Note that if we reach this point and EX_FAST is set, then we know there is no
            // space in the extent for the request.
            Fit::Fail if flags & EX_FAST == 0 && bestovh != 0 => {
                // We have a match that's "good enough".
                (beststart, bestlast)
            }
            Fit::Fail => {
                // No space currently available. Wait for it to free up, if possible.
                if flags & EX_WAITSPACE != 0 {
                    ex.ex_flags.set(ex.ex_flags.get() | EXF_WANTED);
                    extent_sleep(ptr::from_ref(ex), flags)?;
                    continue 'alloc_start;
                }

                extent_free_region_descriptor(ex, myrp);
                return Err(Errno::EAGAIN);
            }
        };

        // found: insert ourselves into the region list.
        extent_insert_and_optimize(ex, start, size, after, myrp);
        return Ok(start);
    }
}

/// `extent_alloc_subregion(ex, substart, subend, size, alignment, skew, boundary, flags,
/// result)`: see `extent_do_alloc`; returns the start.
#[allow(clippy::too_many_arguments)] // the C's signature
pub fn extent_alloc_subregion(
    ex: &Extent,
    substart: u64,
    subend: u64,
    size: u64,
    alignment: u64,
    skew: u64,
    boundary: u64,
    flags: i32,
) -> Result<u64, Errno> {
    // Allocate the region descriptor. It will be freed later if we can coalesce with
    // another region.
    let Some(rp) = extent_alloc_region_descriptor(ex, flags) else {
        #[cfg(feature = "diagnostic")]
        printf(format_args!(
            "extent_alloc_subregion: can't allocate region descriptor\n"
        ));
        return Err(Errno::ENOMEM);
    };

    extent_do_alloc(
        ex, substart, subend, size, alignment, skew, boundary, flags, rp,
    )
}

/// `extent_alloc_subregion_with_descr(...)`: as `extent_alloc_subregion`, with the caller's
/// descriptor, in an `EX_NOCOALESCE` extent.
///
/// # Safety
///
/// `rp` is in no list and stays valid and in place while the extent links it.
#[allow(clippy::too_many_arguments)] // the C's signature
pub unsafe fn extent_alloc_subregion_with_descr(
    ex: &Extent,
    substart: u64,
    subend: u64,
    size: u64,
    alignment: u64,
    skew: u64,
    boundary: u64,
    flags: i32,
    rp: &ExtentRegion,
) -> Result<u64, Errno> {
    #[cfg(feature = "diagnostic")]
    if ex.ex_flags.get() & EXF_NOCOALESCE == 0 {
        panic(format_args!(
            "extent_alloc_subregion_with_descr: EX_NOCOALESCE not set"
        ));
    }

    rp.er_flags.set(ER_DISCARD);
    extent_do_alloc(
        ex, substart, subend, size, alignment, skew, boundary, flags, rp,
    )
}

/// `extent_alloc(ex, size, alignment, skew, boundary, flags, result)`: the simple case of
/// `extent_alloc_subregion`, over the whole extent.
pub fn extent_alloc(
    ex: &Extent,
    size: u64,
    alignment: u64,
    skew: u64,
    boundary: u64,
    flags: i32,
) -> Result<u64, Errno> {
    extent_alloc_subregion(
        ex,
        ex.ex_start,
        ex.ex_end,
        size,
        alignment,
        skew,
        boundary,
        flags,
    )
}

/// `extent_alloc_with_descr(ex, size, alignment, skew, boundary, flags, region, result)`:
/// the simple case of `extent_alloc_subregion_with_descr`.
///
/// # Safety
///
/// As for `extent_alloc_subregion_with_descr`.
#[allow(clippy::too_many_arguments)] // the C's signature
pub unsafe fn extent_alloc_with_descr(
    ex: &Extent,
    size: u64,
    alignment: u64,
    skew: u64,
    boundary: u64,
    flags: i32,
    rp: &ExtentRegion,
) -> Result<u64, Errno> {
    // SAFETY: the caller's guarantee, forwarded.
    unsafe {
        extent_alloc_subregion_with_descr(
            ex,
            ex.ex_start,
            ex.ex_end,
            size,
            alignment,
            skew,
            boundary,
            flags,
            rp,
        )
    }
}

/// `extent_free(ex, start, size, flags)`: gives `[start, start + size - 1]` back. Panics when
/// the range is not allocated, unless `EX_CONFLICTOK` allows partial overlaps.
pub fn extent_free(ex: &Extent, start: u64, size: u64, flags: i32) -> Result<(), Errno> {
    let end = start.wrapping_add(size.wrapping_sub(1));

    #[cfg(feature = "diagnostic")]
    {
        // Check arguments.
        if start < ex.ex_start || end > ex.ex_end {
            extent_print(ex);
            printf(format_args!(
                "extent_free: extent `{}', start 0x{:x}, size 0x{:x}\n",
                Str(ex.ex_name),
                start,
                size
            ));
            panic(format_args!(
                "extent_free: extent `{}', region not within extent",
                Str(ex.ex_name)
            ));
        }
        // Check for an overflow.
        if end < start {
            extent_print(ex);
            printf(format_args!(
                "extent_free: extent `{}', start 0x{:x}, size 0x{:x}\n",
                Str(ex.ex_name),
                start,
                size
            ));
            panic(format_args!("extent_free: overflow"));
        }
    }

    // If we're allowing coalescing, we must allocate a region descriptor now, since it
    // might block.
    //
    // XXX Make a static, create-time flags word, so we don't
    // XXX have to lock to read it!
    let exflags = ex.ex_flags.get();

    let mut nrp: Option<&ExtentRegion> = None;
    if exflags & EXF_NOCOALESCE == 0 {
        // Allocate a region descriptor.
        nrp = Some(extent_alloc_region_descriptor(ex, flags).ok_or(Errno::ENOMEM)?);
    }

    // Find region and deallocate. Several possibilities:
    //
    //	1. (start == er_start) && (end == er_end):
    //	   Free descriptor.
    //
    //	2. (start == er_start) && (end < er_end):
    //	   Adjust er_start.
    //
    //	3. (start > er_start) && (end == er_end):
    //	   Adjust er_end.
    //
    //	4. (start > er_start) && (end < er_end):
    //	   Fragment region. Requires descriptor alloc.
    //
    // Cases 2, 3, and 4 require that the EXF_NOCOALESCE flag is not set.
    //
    // If the EX_CONFLICTOK flag is set, partially overlapping regions are allowed. This is
    // handled in cases 1a, 2a and 3a below.
    let done = 'search: {
        for rp in ex.ex_regions.iter() {
            // Save ourselves some comparisons; does the current region end before chunk to be
            // freed begins? If so, then we haven't found the appropriate region descriptor.
            if rp.er_end.get() < start {
                continue;
            }

            // Save ourselves some traversal; does the current region begin after the chunk
            // to be freed ends? If so, then we've already passed any possible region
            // descriptors that might have contained the chunk to be freed.
            if rp.er_start.get() > end {
                break;
            }

            // Case 1.
            if start == rp.er_start.get() && end == rp.er_end.get() {
                // SAFETY: `rp` is linked in the extent's region list.
                unsafe { ListHead::<ExtentRegionList>::remove(rp) };
                extent_free_region_descriptor(ex, rp);
                break 'search true;
            }

            // The following cases all require that EXF_NOCOALESCE is not set.
            if ex.ex_flags.get() & EXF_NOCOALESCE != 0 {
                continue;
            }

            // Case 2.
            if start == rp.er_start.get() && end < rp.er_end.get() {
                rp.er_start.set(end + 1);
                break 'search true;
            }

            // Case 3.
            if start > rp.er_start.get() && end == rp.er_end.get() {
                rp.er_end.set(start - 1);
                break 'search true;
            }

            // Case 4 (`nrp` was allocated: the extent coalesces).
            if start > rp.er_start.get()
                && end < rp.er_end.get()
                && let Some(n) = nrp.take()
            {
                // Fill in new descriptor.
                n.er_start.set(end + 1);
                n.er_end.set(rp.er_end.get());

                // Adjust current descriptor.
                rp.er_end.set(start - 1);

                // Insert new descriptor after current.
                // SAFETY: `rp` is linked; `n` is a descriptor just taken, in no list.
                unsafe { ListHead::<ExtentRegionList>::insert_after(rp, n) };

                // We used the new descriptor, so don't free it below
                break 'search true;
            }

            if flags & EX_CONFLICTOK == 0 {
                continue;
            }

            // Case 1a.
            if start <= rp.er_start.get() && end >= rp.er_end.get() {
                // SAFETY: as in case 1; the iterator already read the next region.
                unsafe { ListHead::<ExtentRegionList>::remove(rp) };
                extent_free_region_descriptor(ex, rp);
                continue;
            }

            // Case 2a.
            if start <= rp.er_start.get() && end >= rp.er_start.get() {
                rp.er_start.set(end.wrapping_add(1));
            }

            // Case 3a.
            if start <= rp.er_end.get() && end >= rp.er_end.get() {
                rp.er_end.set(start.wrapping_sub(1));
            }
        }
        false
    };

    if !done && flags & EX_CONFLICTOK == 0 {
        // Region not found, or request otherwise invalid.
        // DIAGNOSTIC || DDB
        extent_print(ex);
        printf(format_args!(
            "extent_free: start 0x{:x}, end 0x{:x}\n",
            start, end
        ));
        panic(format_args!("extent_free: region not found"));
    }

    // done:
    if let Some(n) = nrp {
        extent_free_region_descriptor(ex, n);
    }
    if ex.ex_flags.get() & EXF_WANTED != 0 {
        ex.ex_flags.set(ex.ex_flags.get() & !EXF_WANTED);
        wakeup(ptr::from_ref(ex));
    }
    Ok(())
}

/// `extent_alloc_region_descriptor(ex, flags)`: a descriptor from a fixed extent's freelist
/// (waiting for one with `EX_WAITOK`, or from the pool with `EX_MALLOCOK`), or from the pool.
fn extent_alloc_region_descriptor(ex: &Extent, flags: i32) -> Option<&'static ExtentRegion> {
    if ex.ex_flags.get() & EXF_FIXED != 0 {
        let fex = extent_fixed(ex);

        let mut from_pool = false;
        while fex.fex_freelist.is_empty() {
            if flags & EX_MALLOCOK != 0 {
                from_pool = true;
                break;
            }

            if flags & EX_WAITOK == 0 {
                return None;
            }
            ex.ex_flags.set(ex.ex_flags.get() | EXF_FLWANTED);
            if extent_sleep(ptr::from_ref(&fex.fex_freelist), flags).is_err() {
                return None;
            }
        }
        if !from_pool {
            let rp = fex.fex_freelist.first()?;
            // SAFETY: `rp` is linked in the freelist.
            unsafe { ListHead::<ExtentRegionList>::remove(rp) };

            // Don't muck with flags after pulling it off the freelist; it may be a
            // dynamically allocated region pointer that was kindly given to us, and we need
            // to preserve that information.

            // SAFETY: the freelist's descriptors live in the extent's `'static` storage or
            // came from the pool, and are never freed while the extent exists.
            return Some(unsafe { &*ptr::from_ref(rp) });
        }
    }

    // alloc:
    let p = pool_get(
        &EX_REGION_PL,
        if flags & EX_WAITOK != 0 {
            PR_WAITOK
        } else {
            PR_NOWAIT
        },
    )?
    .cast::<ExtentRegion>();
    // SAFETY: a fresh pool item of a descriptor's size (`extent_pool_init`), written once
    // before use and given back only by `extent_free_region_descriptor`.
    let rp: &'static ExtentRegion = unsafe {
        p.write(ExtentRegion::new());
        &*p.as_ptr()
    };
    rp.er_flags.set(ER_ALLOC);
    Some(rp)
}

/// `extent_free_region_descriptor(ex, rp)`: gives a descriptor back to the fixed extent's
/// freelist (waking a waiter) or to the pool; a caller's (`ER_DISCARD`) is left alone.
fn extent_free_region_descriptor(ex: &Extent, rp: &ExtentRegion) {
    if rp.er_flags.get() & ER_DISCARD != 0 {
        return;
    }

    if ex.ex_flags.get() & EXF_FIXED != 0 {
        let fex = extent_fixed(ex);

        // If someone's waiting for a region descriptor, be nice and give them this one,
        // rather than just free'ing it back to the system.
        if rp.er_flags.get() & ER_ALLOC != 0 {
            if ex.ex_flags.get() & EXF_FLWANTED != 0 {
                // Clear all but ER_ALLOC flag.
                rp.er_flags.set(ER_ALLOC);
                // SAFETY: a descriptor being freed is in no list; a pool item stays valid
                // until it is put back.
                unsafe { fex.fex_freelist.insert_head(rp) };
                // wake_em_up:
                ex.ex_flags.set(ex.ex_flags.get() & !EXF_FLWANTED);
                wakeup(ptr::from_ref(&fex.fex_freelist));
                return;
            }
            pool_put(&EX_REGION_PL, NonNull::from(rp).cast());
        } else {
            // Clear all flags.
            rp.er_flags.set(0);
            // SAFETY: a descriptor of the fixed storage, in no list, living as long as it.
            unsafe { fex.fex_freelist.insert_head(rp) };
        }

        if ex.ex_flags.get() & EXF_FLWANTED != 0 {
            // wake_em_up:
            ex.ex_flags.set(ex.ex_flags.get() & !EXF_FLWANTED);
            wakeup(ptr::from_ref(&fex.fex_freelist));
        }
        return;
    }

    // We know it's dynamically allocated if we get here.
    pool_put(&EX_REGION_PL, NonNull::from(rp).cast());
}

/// `extent_print(ex)`.
pub fn extent_print(ex: &Extent) {
    extent_print1(ex, printf);
}

/// `extent_print1(ex, pr)`: the extent's name, range and flags, then a line per region.
pub fn extent_print1(ex: &Extent, pr: fn(fmt::Arguments<'_>) -> usize) {
    pr(format_args!(
        "extent `{}' (0x{:x} - 0x{:x}), flags={}\n",
        Str(ex.ex_name),
        ex.ex_start,
        ex.ex_end,
        Bitmask(ex.ex_flags.get() as u64, EXF_BITS)
    ));

    for rp in ex.ex_regions.iter() {
        pr(format_args!(
            "     0x{:x} - 0x{:x}\n",
            rp.er_start.get(),
            rp.er_end.get()
        ));
    }
}

#[cfg(test)]
mod tests;
