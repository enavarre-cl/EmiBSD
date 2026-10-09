/* <LICENSES> */
/*-
 * Copyright (c) 1996 The NetBSD Foundation, Inc.
 * All rights reserved.
 *
 * This code is derived from software contributed to The NetBSD Foundation
 * by Jason R. Thorpe.
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

/* <CODE> */
//! `<sys/extent.h>`: the general purpose extent manager's types and flags (`extent(9)`).
//!
//! Upstream: sys/sys/extent.h @ 3ce1f3f79392
//!
//! An extent is a range of unsigned numbers (bus numbers, I/O ports, memory addresses) and
//! the list of regions allocated in it, kept sorted. The functions are `kern/subr_extent.rs`.
//!
//! ## Deviations
//! - `u_long` is `u64` (LP64 on both architectures).
//! - The descriptors are `#[repr(C)]` structures of `Cell`s, changed through shared
//!   references as the C changes them through pointers; the extent has no lock, its users
//!   serialize (autoconfiguration under the kernel lock), as in C.
//! - `ex_name` is a byte string (`&'static [u8]`), printed without its NUL.
//! - The `extent_alloc` and `extent_alloc_with_descr` macros are functions in
//!   `subr_extent.rs`; the prototypes are that file's.

use core::cell::Cell;

use crate::sys::param::align;
use crate::sys::queue::{ListEntry, ListHead};

/// `struct extent_region`: one allocated region `[er_start, er_end]`.
#[repr(C)]
pub struct ExtentRegion {
    /// `er_link`: link in the extent's region list (or a fixed extent's freelist).
    pub er_link: ListEntry<ExtentRegion>,
    /// `er_start`: start of region.
    pub er_start: Cell<u64>,
    /// `er_end`: end of region (inclusive).
    pub er_end: Cell<u64>,
    /// `er_flags`: `ER_*`.
    pub er_flags: Cell<i32>,
}

impl ExtentRegion {
    /// A descriptor in no list, covering nothing, with no flags.
    pub const fn new() -> Self {
        Self {
            er_link: ListEntry::new(),
            er_start: Cell::new(0),
            er_end: Cell::new(0),
            er_flags: Cell::new(0),
        }
    }
}

impl Default for ExtentRegion {
    fn default() -> Self {
        Self::new()
    }
}

crate::queue_adapter!(
    /// `LIST_HEAD(, extent_region)`: through `er_link`.
    pub ExtentRegionList: ExtentRegion, er_link => ListEntry<ExtentRegion>
);

/// `ER_ALLOC`: region descriptor dynamically allocated.
pub const ER_ALLOC: i32 = 0x01;
/// `ER_DISCARD`: discard region descriptor after use.
pub const ER_DISCARD: i32 = 0x02;

/// `struct extent`.
#[repr(C)]
pub struct Extent {
    /// `ex_name`: name of extent.
    pub ex_name: &'static [u8],
    /// `ex_regions`: allocated regions in extent, sorted by start.
    pub ex_regions: ListHead<ExtentRegionList>,
    /// `ex_start`: start of extent.
    pub ex_start: u64,
    /// `ex_end`: end of extent (inclusive).
    pub ex_end: u64,
    /// `ex_mtype`: memory type (`malloc(9)`).
    pub ex_mtype: i32,
    /// `ex_flags`: `EXF_*`.
    pub ex_flags: Cell<i32>,
    /// `ex_link`: link in the list of all extents (`DIAGNOSTIC || DDB`).
    pub ex_link: ListEntry<Extent>,
}

crate::queue_adapter!(
    /// `LIST_HEAD(listhead, extent)`: through `ex_link`.
    pub ExtentList: Extent, ex_link => ListEntry<Extent>
);

/// `struct extent_fixed`: an extent whose descriptors live in storage the caller provides.
#[repr(C)]
pub struct ExtentFixed {
    /// `fex_extent`: MUST BE FIRST.
    pub fex_extent: Extent,
    /// `fex_freelist`: freelist of region descriptors.
    pub fex_freelist: ListHead<ExtentRegionList>,
    /// `fex_storage`: storage space for descriptors.
    pub fex_storage: *mut u8,
    /// `fex_storagesize`: size of storage space.
    pub fex_storagesize: usize,
}

/// `EXF_FIXED`: extent uses fixed storage.
pub const EXF_FIXED: i32 = 0x01;
/// `EXF_NOCOALESCE`: coalescing of regions not allowed.
pub const EXF_NOCOALESCE: i32 = 0x02;
/// `EXF_WANTED`: someone asleep on extent.
pub const EXF_WANTED: i32 = 0x04;
/// `EXF_FLWANTED`: someone asleep on freelist.
pub const EXF_FLWANTED: i32 = 0x08;

/// `EXF_BITS`: the `%b` description of `ex_flags`.
pub const EXF_BITS: &[u8] = b"\x10\x04FLWANTED\x03WANTED\x02NOCOALESCE\x01FIXED";

/// `EX_NOWAIT`: not safe to sleep.
pub const EX_NOWAIT: i32 = 0x0000;
/// `EX_WAITOK`: safe to sleep.
pub const EX_WAITOK: i32 = 0x0001;
/// `EX_FAST`: take first fit in `extent_alloc()`.
pub const EX_FAST: i32 = 0x0002;
/// `EX_CATCH`: catch signals while sleeping.
pub const EX_CATCH: i32 = 0x0004;
/// `EX_NOCOALESCE`: create a non-coalescing extent.
pub const EX_NOCOALESCE: i32 = 0x0008;
/// `EX_MALLOCOK`: safe to call `malloc()`.
pub const EX_MALLOCOK: i32 = 0x0010;
/// `EX_WAITSPACE`: wait for space to become free.
pub const EX_WAITSPACE: i32 = 0x0020;
/// `EX_BOUNDZERO`: boundary lines start at 0.
pub const EX_BOUNDZERO: i32 = 0x0040;
/// `EX_CONFLICTOK`: allow conflicts.
pub const EX_CONFLICTOK: i32 = 0x0080;
/// `EX_FILLED`: create a filled extent.
pub const EX_FILLED: i32 = 0x0100;

/// `EX_NOALIGN`: don't do alignment (the "alignment" argument's place holder).
pub const EX_NOALIGN: u64 = 1;
/// `EX_NOBOUNDARY`: don't do boundary checking (the "boundary" argument's place holder).
pub const EX_NOBOUNDARY: u64 = 0;

/// `EXTENT_FIXED_STORAGE_SIZE(nregions)`: the storage a fixed extent of `nregions`
/// descriptors needs.
pub const fn extent_fixed_storage_size(nregions: usize) -> usize {
    align(size_of::<ExtentFixed>()) + align(size_of::<ExtentRegion>()) * nregions
}
/* </CODE> */
