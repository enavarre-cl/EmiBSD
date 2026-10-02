/*	$OpenBSD: bus_space.c,v 1.1 2024/11/12 04:56:27 jsg Exp $ */
/* <LICENSES> */
/*
 * Copyright (c) 2001-2003 Opsycon AB  (www.opsycon.se / www.opsycon.com)
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
 * THIS SOFTWARE IS PROVIDED BY THE AUTHOR ``AS IS'' AND ANY EXPRESS
 * OR IMPLIED WARRANTIES, INCLUDING, BUT NOT LIMITED TO, THE IMPLIED
 * WARRANTIES OF MERCHANTABILITY AND FITNESS FOR A PARTICULAR PURPOSE
 * ARE DISCLAIMED.  IN NO EVENT SHALL THE AUTHOR BE LIABLE FOR ANY
 * DIRECT, INDIRECT, INCIDENTAL, SPECIAL, EXEMPLARY, OR CONSEQUENTIAL
 * DAMAGES (INCLUDING, BUT NOT LIMITED TO, PROCUREMENT OF SUBSTITUTE GOODS
 * OR SERVICES; LOSS OF USE, DATA, OR PROFITS; OR BUSINESS INTERRUPTION)
 * HOWEVER CAUSED AND ON ANY THEORY OF LIABILITY, WHETHER IN CONTRACT, STRICT
 * LIABILITY, OR TORT (INCLUDING NEGLIGENCE OR OTHERWISE) ARISING IN ANY WAY
 * OUT OF THE USE OF THIS SOFTWARE, EVEN IF ADVISED OF THE POSSIBILITY OF
 * SUCH DAMAGE.
 *
 */
/* </LICENSES> */

//! Simple generic bus access primitives: `arch/arm64/arm64/bus_space.c`.
//!
//! Upstream: sys/arch/arm64/arm64/bus_space.c @ 3ce1f3f79392
//!
//! Status: `wip`. Milestone M2 ports `arm64_bs_tag`, `fdt_cons_bs_tag`, the single-register
//! accessors, `generic_space_map`/`unmap`/`region`/`vaddr`. The raw-multi accessors and
//! `generic_space_mmap` arrive with M6.
//!
//! ## Deviations
//! - `generic_space_map` needs `km_alloc` and `pmap_kenter_cache` (M3). Until then the early
//!   init installs one identity mapping of the first GiB of physical space as device memory
//!   (`machdep.rs`, `BOOTSTRAP_DEVICE_MAP_SIZE`); a map inside it is the identity, one outside
//!   it is reported as unported. The C swaps `_space_map` for `pmap_bootstrap_bs_map` during
//!   `consinit` for the same reason.
//! - The map flags are accepted and ignored: the bootstrap mapping is Device-nGnRnE.

use core::ptr;

use crate::arch::arm64::arm64::machdep::BOOTSTRAP_DEVICE_MAP_SIZE;
use crate::arch::arm64::include::bus::{BusSpace, BusSpaceHandle};
use crate::machine::bus::{BusAddr, BusSize};
use crate::sys::errno::Errno;
use crate::unported;

/// `arm64_bs_tag`: the one bus space of the machine.
pub static ARM64_BS_TAG: BusSpace = BusSpace {
    bus_base: 0, // XXX
    _space_read_1: generic_space_read_1,
    _space_write_1: generic_space_write_1,
    _space_read_2: generic_space_read_2,
    _space_write_2: generic_space_write_2,
    _space_read_4: generic_space_read_4,
    _space_write_4: generic_space_write_4,
    _space_read_8: generic_space_read_8,
    _space_write_8: generic_space_write_8,
    _space_map: generic_space_map,
    _space_unmap: generic_space_unmap,
    _space_subregion: generic_space_region,
    _space_vaddr: generic_space_vaddr,
};

/// `fdt_cons_bs_tag`: the tag the device-tree console attach uses.
pub static FDT_CONS_BS_TAG: &BusSpace = &ARM64_BS_TAG;

/// `generic_space_read_1`.
pub fn generic_space_read_1(_t: &'static BusSpace, h: BusSpaceHandle, o: BusSize) -> u8 {
    // SAFETY: `h` came from `generic_space_map`, which mapped the device at it.
    unsafe { ptr::read_volatile((h.0 + o) as *const u8) }
}

/// `generic_space_read_2`.
pub fn generic_space_read_2(_t: &'static BusSpace, h: BusSpaceHandle, o: BusSize) -> u16 {
    // SAFETY: as for `generic_space_read_1`.
    unsafe { ptr::read_volatile((h.0 + o) as *const u16) }
}

/// `generic_space_read_4`.
pub fn generic_space_read_4(_t: &'static BusSpace, h: BusSpaceHandle, o: BusSize) -> u32 {
    // SAFETY: as for `generic_space_read_1`.
    unsafe { ptr::read_volatile((h.0 + o) as *const u32) }
}

/// `generic_space_read_8`.
pub fn generic_space_read_8(_t: &'static BusSpace, h: BusSpaceHandle, o: BusSize) -> u64 {
    // SAFETY: as for `generic_space_read_1`.
    unsafe { ptr::read_volatile((h.0 + o) as *const u64) }
}

/// `generic_space_write_1`.
pub fn generic_space_write_1(_t: &'static BusSpace, h: BusSpaceHandle, o: BusSize, v: u8) {
    // SAFETY: as for `generic_space_read_1`.
    unsafe { ptr::write_volatile((h.0 + o) as *mut u8, v) }
}

/// `generic_space_write_2`.
pub fn generic_space_write_2(_t: &'static BusSpace, h: BusSpaceHandle, o: BusSize, v: u16) {
    // SAFETY: as for `generic_space_read_1`.
    unsafe { ptr::write_volatile((h.0 + o) as *mut u16, v) }
}

/// `generic_space_write_4`.
pub fn generic_space_write_4(_t: &'static BusSpace, h: BusSpaceHandle, o: BusSize, v: u32) {
    // SAFETY: as for `generic_space_read_1`.
    unsafe { ptr::write_volatile((h.0 + o) as *mut u32, v) }
}

/// `generic_space_write_8`.
pub fn generic_space_write_8(_t: &'static BusSpace, h: BusSpaceHandle, o: BusSize, v: u64) {
    // SAFETY: as for `generic_space_read_1`.
    unsafe { ptr::write_volatile((h.0 + o) as *mut u64, v) }
}

/// `generic_space_map`: maps `[offs, offs + size)` as device memory (see the module's
/// deviations for what "maps" means before M3).
///
/// # Safety
///
/// As for `machine::bus::BusSpace::bus_space_map`.
pub unsafe fn generic_space_map(
    _t: &'static BusSpace,
    offs: BusAddr,
    size: BusSize,
    _flags: u32,
) -> Result<BusSpaceHandle, Errno> {
    if offs
        .checked_add(size)
        .is_some_and(|end| end <= BOOTSTRAP_DEVICE_MAP_SIZE)
    {
        Ok(BusSpaceHandle(offs))
    } else {
        Err(unported!(
            "generic_space_map outside the bootstrap device map: km_alloc, pmap_kenter_cache"
        ))
    }
}

/// `generic_space_unmap`: releases a mapping; the bootstrap identity map is never released.
pub fn generic_space_unmap(_t: &'static BusSpace, bsh: BusSpaceHandle, size: BusSize) {
    if !bsh
        .0
        .checked_add(size)
        .is_some_and(|end| end <= BOOTSTRAP_DEVICE_MAP_SIZE)
    {
        let _ = unported!("generic_space_unmap: pmap_kremove, km_free");
    }
}

/// `generic_space_region`: a handle for `[offset, offset + size)` of an existing mapping.
pub fn generic_space_region(
    _t: &'static BusSpace,
    bsh: BusSpaceHandle,
    offset: BusSize,
    _size: BusSize,
) -> Result<BusSpaceHandle, Errno> {
    Ok(BusSpaceHandle(bsh.0 + offset))
}

/// `generic_space_vaddr`: the kernel virtual address behind a handle.
pub fn generic_space_vaddr(_t: &'static BusSpace, h: BusSpaceHandle) -> *mut u8 {
    h.0 as *mut u8
}
