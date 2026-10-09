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

/* <CODE> */
//! Simple generic bus access primitives: `arch/arm64/arm64/bus_space.c`.
//!
//! Upstream: sys/arch/arm64/arm64/bus_space.c @ 3ce1f3f79392
//!
//! Status: `wip`. Milestone M2 ports `arm64_bs_tag`, `fdt_cons_bs_tag`, the single-register
//! accessors, `generic_space_map`/`unmap`/`region`/`vaddr`. M12 adds `generic_space_mmap`
//! and the `bus_private`/`_space_mmap` members of `arm64_bs_tag`, for the bridges that copy
//! it (`simplebus`, `pciecam`). The raw-multi accessors arrive with their drivers.
//!
//! ## Deviations
//! - Until `pmap_init`, `generic_space_map` is the identity inside the bootstrap device map
//!   the early init installs (the first GiB of physical space as device memory in `TTBR0`,
//!   `machdep.rs`, `BOOTSTRAP_DEVICE_MAP_SIZE`); afterwards it maps through `pmap_kenter_cache`
//!   in the kernel half from the `vmmap` window below `virtual_avail` (`VMMAP_SIZE`; the
//!   C's `km_alloc(kv_any)` takes the range from `kernel_map`), and the console
//!   is remapped so the lower half can go to user address spaces. A map outside the bootstrap
//!   map before `pmap_init`
//!   it is reported as unported. The C swaps `_space_map` for `pmap_bootstrap_bs_map` during
//!   `consinit` for the same reason.
//! - The map flags are accepted and ignored: the bootstrap mapping is Device-nGnRnE.
//! - After `pmap_init`, a mapping that no longer fits in the `vmmap` window (a PCIe ECAM
//!   region is 256 MiB) takes its virtual range from `km_alloc(kv_any, kp_none, kd_nowait)`,
//!   as the C does for every mapping, and `generic_space_unmap` gives such a range back with
//!   `km_free`.

use core::ptr;

use core::sync::atomic::Ordering;

use crate::arch::arm64::arm64::machdep::BOOTSTRAP_DEVICE_MAP_SIZE;
use crate::arch::arm64::arm64::pmap::{
    VMMAP, VMMAP_SIZE, pmap_growkernel, pmap_initialized, pmap_kenter_cache, pmap_kernel,
    pmap_kremove,
};
use crate::arch::arm64::include::bus::{
    BUS_SPACE_MAP_CACHEABLE, BUS_SPACE_MAP_PREFETCHABLE, BusSpace, BusSpaceHandle,
};
use crate::arch::arm64::include::param::PAGE_SIZE;
use crate::arch::arm64::include::pmap::{
    PMAP_CACHE_DEV_NGNRE, PMAP_CACHE_DEV_NGNRNE, PMAP_CACHE_WB,
};
use crate::arch::arm64::include::vmparam::VM_MIN_KERNEL_ADDRESS;
use crate::machine::bus::{BusAddr, BusSize};
use crate::machine::pmap::pmap_update;
use crate::sys::errno::Errno;
use crate::sys::mman::{PROT_READ, PROT_WRITE};
use crate::sys::types::{Off, Paddr, Vaddr, Vsize};
use crate::unported;
use crate::uvm::uvm_km::{KD_NOWAIT, KP_NONE, KV_ANY, km_alloc, km_free};
use crate::uvm::uvm_param::{round_page, trunc_page};

/// `arm64_bs_tag`: the one bus space of the machine.
pub static ARM64_BS_TAG: BusSpace = BusSpace {
    bus_base: 0, // XXX
    bus_private: ptr::null_mut(),
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
    _space_mmap: generic_space_mmap,
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
    flags: u32,
) -> Result<BusSpaceHandle, Errno> {
    if !pmap_initialized() {
        // Before the pmap is up: the bootstrap identity map (see the module's deviations).
        return if offs
            .checked_add(size)
            .is_some_and(|end| end <= BOOTSTRAP_DEVICE_MAP_SIZE)
        {
            Ok(BusSpaceHandle(offs))
        } else {
            Err(unported!(
                "generic_space_map outside the bootstrap device map before pmap_init"
            ))
        };
    }

    // The C: startpa = trunc_page(bpa); endpa = round_page(bpa + size); va = km_alloc(endpa
    // - startpa, &kv_any, &kp_none, &kd_nowait); pmap_kenter_cache each page. Here the
    // virtual range comes from `vmmap` (see `pmap.rs`).
    let startpa = trunc_page(offs);
    let endpa = round_page(offs.checked_add(size).ok_or(Errno::EINVAL)?);
    let len = endpa - startpa;
    let mut va = VMMAP.load(Ordering::Relaxed);
    if va + len > VM_MIN_KERNEL_ADDRESS + VMMAP_SIZE {
        // Beyond the window: kernel_map space, as the C takes for every mapping.
        let Some(kva) = km_alloc(len, &KV_ANY, &KP_NONE, &KD_NOWAIT) else {
            return Err(Errno::ENOMEM);
        };
        va = kva.as_ptr() as usize;
    } else {
        VMMAP.store(va + len, Ordering::Relaxed);
        let _ = pmap_growkernel(Vaddr::new(va + len));
    }
    let cache = if flags & BUS_SPACE_MAP_CACHEABLE != 0 {
        PMAP_CACHE_WB
    } else if flags & BUS_SPACE_MAP_PREFETCHABLE != 0 {
        PMAP_CACHE_DEV_NGNRE
    } else {
        PMAP_CACHE_DEV_NGNRNE
    };
    let mut pa = startpa;
    let mut cur = va;
    while pa < endpa {
        // SAFETY: a fresh kernel virtual range (`vmmap` or `km_alloc`) the tables cover,
        // mapping device registers the caller owns.
        unsafe {
            pmap_kenter_cache(
                Vaddr::new(cur),
                Paddr::new(pa),
                PROT_READ | PROT_WRITE,
                cache,
            )
        };
        pa += PAGE_SIZE;
        cur += PAGE_SIZE;
    }
    pmap_update(pmap_kernel());
    Ok(BusSpaceHandle(va + (offs - startpa)))
}

/// `generic_space_unmap`: releases a mapping; the bootstrap identity map is never released.
pub fn generic_space_unmap(_t: &'static BusSpace, bsh: BusSpaceHandle, size: BusSize) {
    if bsh.0 >= VM_MIN_KERNEL_ADDRESS {
        // pmap_kremove(va, endva - va), km_free: the vmmap range is not reused.
        let va = trunc_page(bsh.0);
        let endva = round_page(bsh.0 + size);
        // SAFETY: a range `generic_space_map` entered.
        unsafe { pmap_kremove(Vaddr::new(va), Vsize::new(endva - va)) };
        pmap_update(pmap_kernel());
        // A range beyond the vmmap window came from km_alloc.
        if va >= VM_MIN_KERNEL_ADDRESS + VMMAP_SIZE
            && let Some(kva) = core::ptr::NonNull::new(va as *mut u8)
        {
            km_free(kva, endva - va, &KV_ANY, &KP_NONE);
        }
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

/// `generic_space_mmap`: the bus address is the physical address.
pub fn generic_space_mmap(
    _t: &'static BusSpace,
    addr: BusAddr,
    off: Off,
    _prot: i32,
    _flags: i32,
) -> Option<Paddr> {
    Some(Paddr::new(addr.wrapping_add(off as usize)))
}
/* </CODE> */
