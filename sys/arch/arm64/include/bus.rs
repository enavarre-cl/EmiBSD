/* $OpenBSD: bus.h,v 1.13 2026/06/22 07:54:19 deraadt Exp $ */
/*
 * Copyright (c) 2003-2004 Opsycon AB Sweden.  All rights reserved.
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

//! arm64 `<machine/bus.h>`: the bus access methods as a table of functions.
//!
//! Upstream: sys/arch/arm64/include/bus.h @ 3ce1f3f79392
//!
//! Status: `wip`. Milestone M2 ports `struct bus_space` with the single-register accessors,
//! map, unmap, subregion and vaddr, the `BUS_SPACE_MAP_*` flags and `bus_space_barrier`. The
//! raw-multi accessors, `_space_mmap`, `bus_private` and all of `bus_dma` arrive with M6.
//!
//! ## Deviations
//! - A tag is a `&'static BusSpace` (C: `bus_space_tag_t`, a pointer to the table).
//! - `_space_map` is an `unsafe fn`, as in the machine contract.

use core::arch::asm;

use crate::machine::bus::{BusAddr, BusSize};
use crate::sys::errno::Errno;

/// `BUS_SPACE_MAP_CACHEABLE`.
pub const BUS_SPACE_MAP_CACHEABLE: u32 = 0x01;
/// `BUS_SPACE_MAP_POSTED`: device memory with posted writes (nGnRE).
pub const BUS_SPACE_MAP_POSTED: u32 = 0x02;
/// `BUS_SPACE_MAP_LINEAR`.
pub const BUS_SPACE_MAP_LINEAR: u32 = 0x04;
/// `BUS_SPACE_MAP_PREFETCHABLE`.
pub const BUS_SPACE_MAP_PREFETCHABLE: u32 = 0x08;

/// `bus_space_handle_t`: the kernel virtual address of a mapped region, only ever produced by
/// `_space_map` or `_space_subregion`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct BusSpaceHandle(pub(in crate::arch::arm64) usize);

/// `struct bus_space` (`bus_space_t`): one bus's access methods.
pub struct BusSpace {
    /// `bus_base`: the bus's base address.
    pub bus_base: BusAddr,
    /// `_space_read_1`.
    pub _space_read_1: fn(&'static BusSpace, BusSpaceHandle, BusSize) -> u8,
    /// `_space_write_1`.
    pub _space_write_1: fn(&'static BusSpace, BusSpaceHandle, BusSize, u8),
    /// `_space_read_2`.
    pub _space_read_2: fn(&'static BusSpace, BusSpaceHandle, BusSize) -> u16,
    /// `_space_write_2`.
    pub _space_write_2: fn(&'static BusSpace, BusSpaceHandle, BusSize, u16),
    /// `_space_read_4`.
    pub _space_read_4: fn(&'static BusSpace, BusSpaceHandle, BusSize) -> u32,
    /// `_space_write_4`.
    pub _space_write_4: fn(&'static BusSpace, BusSpaceHandle, BusSize, u32),
    /// `_space_read_8`.
    pub _space_read_8: fn(&'static BusSpace, BusSpaceHandle, BusSize) -> u64,
    /// `_space_write_8`.
    pub _space_write_8: fn(&'static BusSpace, BusSpaceHandle, BusSize, u64),
    /// `_space_map`.
    pub _space_map:
        unsafe fn(&'static BusSpace, BusAddr, BusSize, u32) -> Result<BusSpaceHandle, Errno>,
    /// `_space_unmap`.
    pub _space_unmap: fn(&'static BusSpace, BusSpaceHandle, BusSize),
    /// `_space_subregion`.
    pub _space_subregion:
        fn(&'static BusSpace, BusSpaceHandle, BusSize, BusSize) -> Result<BusSpaceHandle, Errno>,
    /// `_space_vaddr`.
    pub _space_vaddr: fn(&'static BusSpace, BusSpaceHandle) -> *mut u8,
}

/// `bus_space_barrier`: a full system barrier (`dsb sy`), whatever the flags.
pub fn bus_space_barrier(
    _t: &'static BusSpace,
    _h: BusSpaceHandle,
    _offset: BusSize,
    _length: BusSize,
    _flags: u32,
) {
    // SAFETY: a barrier orders memory accesses and touches nothing else.
    unsafe { asm!("dsb sy", options(nostack, preserves_flags)) };
}
