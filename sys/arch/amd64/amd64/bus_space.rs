/*	$OpenBSD: bus_space.c,v 1.32 2026/08/19 08:56:28 hshoexer Exp $	*/
/*	$NetBSD: bus_space.c,v 1.2 2003/03/14 18:47:53 christos Exp $	*/
/* <LICENSES> */
/*-
 * Copyright (c) 1996, 1997, 1998 The NetBSD Foundation, Inc.
 * All rights reserved.
 *
 * This code is derived from software contributed to The NetBSD Foundation
 * by Charles M. Hannum and by Jason R. Thorpe of the Numerical Aerospace
 * Simulation Facility, NASA Ames Research Center.
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

//! amd64 `bus_space(9)`: port I/O and memory space, `arch/amd64/amd64/bus_space.c`.
//!
//! Upstream: sys/arch/amd64/amd64/bus_space.c @ 3ce1f3f79392
//!
//! Status: `wip`. Milestone M2 ports the single-register accessors, `bus_space_map`/`unmap`
//! for I/O space and `bus_space_subregion`. The multi/region/copy accessors, memory-space
//! mapping (`x86_mem_add_mapping`, the ISA hole, the direct map), `bus_space_alloc`/`free`,
//! `bus_space_vaddr`/`mmap`, the extent maps (`x86_bus_space_init`, `x86_bus_space_mallocok`)
//! and the SEV-ES variants arrive with M3 and the buses that need them.
//!
//! ## Deviations
//! - The tag is the enum [`X86BusSpace`] instead of a pointer to an ops table; the accessors
//!   dispatch on it. `X86_BUS_SPACE_IO`/`X86_BUS_SPACE_MEM` are its two values.
//! - `<machine/bus.h>` is not ported (one of its licence blocks has an advertising clause, see
//!   `ports.toml`): the `BUS_SPACE_MAP_*` flags and `bus_space_barrier`, which live there in C,
//!   are defined here from `bus_space(9)`.
//! - Without the `ioport_ex`/`iomem_ex` extents (M3) a map does not check for overlapping
//!   claims, and memory-space maps are reported as unported.

use core::arch::asm;
use core::ptr;

use crate::arch::amd64::include::pio::{inb, inl, inw, outb, outl, outw};
use crate::machine::bus::{BUS_SPACE_BARRIER_READ, BUS_SPACE_BARRIER_WRITE, BusAddr, BusSize};
use crate::sys::errno::Errno;
use crate::unported;

/// `BUS_SPACE_MAP_CACHEABLE`.
pub const BUS_SPACE_MAP_CACHEABLE: u32 = 0x0001;
/// `BUS_SPACE_MAP_LINEAR`.
pub const BUS_SPACE_MAP_LINEAR: u32 = 0x0002;
/// `BUS_SPACE_MAP_PREFETCHABLE`.
pub const BUS_SPACE_MAP_PREFETCHABLE: u32 = 0x0008;

/// `bus_space_tag_t`: which of the two x86 spaces a handle lives in.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum X86BusSpace {
    /// `X86_BUS_SPACE_IO`: port I/O, reached with `in`/`out`.
    Io,
    /// `X86_BUS_SPACE_MEM`: memory-mapped, reached through a kernel virtual address.
    Mem,
}

/// `X86_BUS_SPACE_IO`: space is i/o space.
pub const X86_BUS_SPACE_IO: X86BusSpace = X86BusSpace::Io;
/// `X86_BUS_SPACE_MEM`: space is mem space.
pub const X86_BUS_SPACE_MEM: X86BusSpace = X86BusSpace::Mem;

/// `bus_space_handle_t`: a port base (I/O space) or a kernel virtual address (memory space),
/// only ever produced by [`bus_space_map`] or [`bus_space_subregion`].
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct BusSpaceHandle(usize);

/// `bus_space_map`: claims `[bpa, bpa + size)` in space `t`. I/O space needs no mapping, so
/// the handle is the port base; memory space needs `pmap` (M3).
///
/// # Safety
///
/// As for `machine::bus::BusSpace::bus_space_map`.
pub unsafe fn bus_space_map(
    t: X86BusSpace,
    bpa: BusAddr,
    size: BusSize,
    flags: u32,
) -> Result<BusSpaceHandle, Errno> {
    match t {
        X86BusSpace::Io => {
            if flags & BUS_SPACE_MAP_LINEAR != 0 {
                return Err(Errno::EINVAL);
            }
            // extent_alloc_region(ioport_ex, bpa, size, ...): M3.
            // For I/O space, that's all she wrote.
            Ok(BusSpaceHandle(bpa))
        }
        X86BusSpace::Mem => {
            // The ISA hole, the direct map and x86_mem_add_mapping need pmap.
            let _ = size;
            Err(unported!("bus_space_map (memory space): pmap"))
        }
    }
}

/// `bus_space_unmap`: releases a mapping.
pub fn bus_space_unmap(t: X86BusSpace, _bsh: BusSpaceHandle, _size: BusSize) {
    match t {
        X86BusSpace::Io => {
            // extent_free(ioport_ex, ...): M3.
        }
        X86BusSpace::Mem => {
            let _ = unported!("bus_space_unmap (memory space): pmap");
        }
    }
}

/// `bus_space_subregion`: a handle for `[offset, offset + size)` of an existing mapping.
pub fn bus_space_subregion(
    _t: X86BusSpace,
    bsh: BusSpaceHandle,
    offset: BusSize,
    _size: BusSize,
) -> Result<BusSpaceHandle, Errno> {
    Ok(BusSpaceHandle(bsh.0 + offset))
}

/// `x86_bus_space_io_read_1`.
pub fn x86_bus_space_io_read_1(h: BusSpaceHandle, o: BusSize) -> u8 {
    // SAFETY: `h` came from `bus_space_map` in I/O space, so `h + o` is a port of a device
    // this driver owns.
    unsafe { inb((h.0 + o) as u16) }
}

/// `x86_bus_space_io_read_2`.
pub fn x86_bus_space_io_read_2(h: BusSpaceHandle, o: BusSize) -> u16 {
    // SAFETY: as for `x86_bus_space_io_read_1`.
    unsafe { inw((h.0 + o) as u16) }
}

/// `x86_bus_space_io_read_4`.
pub fn x86_bus_space_io_read_4(h: BusSpaceHandle, o: BusSize) -> u32 {
    // SAFETY: as for `x86_bus_space_io_read_1`.
    unsafe { inl((h.0 + o) as u16) }
}

/// `x86_bus_space_io_write_1`.
pub fn x86_bus_space_io_write_1(h: BusSpaceHandle, o: BusSize, v: u8) {
    // SAFETY: as for `x86_bus_space_io_read_1`.
    unsafe { outb((h.0 + o) as u16, v) }
}

/// `x86_bus_space_io_write_2`.
pub fn x86_bus_space_io_write_2(h: BusSpaceHandle, o: BusSize, v: u16) {
    // SAFETY: as for `x86_bus_space_io_read_1`.
    unsafe { outw((h.0 + o) as u16, v) }
}

/// `x86_bus_space_io_write_4`.
pub fn x86_bus_space_io_write_4(h: BusSpaceHandle, o: BusSize, v: u32) {
    // SAFETY: as for `x86_bus_space_io_read_1`.
    unsafe { outl((h.0 + o) as u16, v) }
}

/// `x86_bus_space_mem_read_1`.
pub fn x86_bus_space_mem_read_1(h: BusSpaceHandle, o: BusSize) -> u8 {
    // SAFETY: `h` came from `bus_space_map` in memory space, which mapped the device at it.
    unsafe { ptr::read_volatile((h.0 + o) as *const u8) }
}

/// `x86_bus_space_mem_read_2`.
pub fn x86_bus_space_mem_read_2(h: BusSpaceHandle, o: BusSize) -> u16 {
    // SAFETY: as for `x86_bus_space_mem_read_1`.
    unsafe { ptr::read_volatile((h.0 + o) as *const u16) }
}

/// `x86_bus_space_mem_read_4`.
pub fn x86_bus_space_mem_read_4(h: BusSpaceHandle, o: BusSize) -> u32 {
    // SAFETY: as for `x86_bus_space_mem_read_1`.
    unsafe { ptr::read_volatile((h.0 + o) as *const u32) }
}

/// `x86_bus_space_mem_write_1`.
pub fn x86_bus_space_mem_write_1(h: BusSpaceHandle, o: BusSize, v: u8) {
    // SAFETY: as for `x86_bus_space_mem_read_1`.
    unsafe { ptr::write_volatile((h.0 + o) as *mut u8, v) }
}

/// `x86_bus_space_mem_write_2`.
pub fn x86_bus_space_mem_write_2(h: BusSpaceHandle, o: BusSize, v: u16) {
    // SAFETY: as for `x86_bus_space_mem_read_1`.
    unsafe { ptr::write_volatile((h.0 + o) as *mut u16, v) }
}

/// `x86_bus_space_mem_write_4`.
pub fn x86_bus_space_mem_write_4(h: BusSpaceHandle, o: BusSize, v: u32) {
    // SAFETY: as for `x86_bus_space_mem_read_1`.
    unsafe { ptr::write_volatile((h.0 + o) as *mut u32, v) }
}

/// `bus_space_read_1`: dispatches on the space.
pub fn bus_space_read_1(t: X86BusSpace, h: BusSpaceHandle, o: BusSize) -> u8 {
    match t {
        X86BusSpace::Io => x86_bus_space_io_read_1(h, o),
        X86BusSpace::Mem => x86_bus_space_mem_read_1(h, o),
    }
}

/// `bus_space_read_2`.
pub fn bus_space_read_2(t: X86BusSpace, h: BusSpaceHandle, o: BusSize) -> u16 {
    match t {
        X86BusSpace::Io => x86_bus_space_io_read_2(h, o),
        X86BusSpace::Mem => x86_bus_space_mem_read_2(h, o),
    }
}

/// `bus_space_read_4`.
pub fn bus_space_read_4(t: X86BusSpace, h: BusSpaceHandle, o: BusSize) -> u32 {
    match t {
        X86BusSpace::Io => x86_bus_space_io_read_4(h, o),
        X86BusSpace::Mem => x86_bus_space_mem_read_4(h, o),
    }
}

/// `bus_space_write_1`.
pub fn bus_space_write_1(t: X86BusSpace, h: BusSpaceHandle, o: BusSize, v: u8) {
    match t {
        X86BusSpace::Io => x86_bus_space_io_write_1(h, o, v),
        X86BusSpace::Mem => x86_bus_space_mem_write_1(h, o, v),
    }
}

/// `bus_space_write_2`.
pub fn bus_space_write_2(t: X86BusSpace, h: BusSpaceHandle, o: BusSize, v: u16) {
    match t {
        X86BusSpace::Io => x86_bus_space_io_write_2(h, o, v),
        X86BusSpace::Mem => x86_bus_space_mem_write_2(h, o, v),
    }
}

/// `bus_space_write_4`.
pub fn bus_space_write_4(t: X86BusSpace, h: BusSpaceHandle, o: BusSize, v: u32) {
    match t {
        X86BusSpace::Io => x86_bus_space_io_write_4(h, o, v),
        X86BusSpace::Mem => x86_bus_space_mem_write_4(h, o, v),
    }
}

/// `bus_space_barrier`: `mfence` for read and write, `sfence` for write only, `lfence`
/// otherwise.
pub fn bus_space_barrier(
    _t: X86BusSpace,
    _h: BusSpaceHandle,
    _offset: BusSize,
    _length: BusSize,
    flags: u32,
) {
    // SAFETY: fences order memory accesses and touch nothing else.
    unsafe {
        if flags == BUS_SPACE_BARRIER_READ | BUS_SPACE_BARRIER_WRITE {
            asm!("mfence", options(nostack, preserves_flags));
        } else if flags == BUS_SPACE_BARRIER_WRITE {
            asm!("sfence", options(nostack, preserves_flags));
        } else {
            asm!("lfence", options(nostack, preserves_flags));
        }
    }
}
