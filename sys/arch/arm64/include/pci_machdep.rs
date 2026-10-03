/*	$OpenBSD: pci_machdep.h,v 1.13 2025/01/23 11:24:34 kettenis Exp $ */
/* <LICENSES> */
/*
 * Copyright (c) 2003-2004 Opsycon AB  (www.opsycon.se / www.opsycon.com)
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

//! arm64 `<machine/pci_machdep.h>`: machine-specific PCI structure and type definitions.
//!
//! Upstream: sys/arch/arm64/include/pci_machdep.h @ 3ce1f3f79392
//!
//! Status: `wip`. On arm64 every PCI host bridge driver (`pciecam`, `dwpcie`, `aplpcie`, ...)
//! fills a `struct machine_pci_chipset` with its configuration and interrupt functions, and
//! the macros this header defines call through it; M7b ports the types and the macros (the
//! machine contract's `PciMachdep` impl in `arch/arm64/mod.rs`). No host bridge driver is
//! ported yet, so no PCI bus attaches on arm64.
//!
//! ## Deviations
//! - The chipset's members are Rust `fn` pointers; the interrupt mapping functions return
//!   `Option` (the C's 1 is `None`) and `pc_intr_string` returns its text by value.
//! - The functions `arm64/pci_machdep.c` defines (`pci_mcfg_init`, `pci_lookup_segment`,
//!   `pci_intr_enable_msivec`, `pci_msi_enable`, `pci_msix_enable`, `_pci_intr_map_msi*`,
//!   `pci_msix_table_map`/`unmap`) are not ported; the contract reports the ones it reaches.

use core::ffi::c_void;
use core::ptr::NonNull;

use crate::arch::arm64::include::bus::BusDmaTag;
use crate::arch::arm64::include::cpu::CpuInfo;
use crate::dev::pci::pcivar::{PciAttachArgs, PcibusAttachArgs, Pcireg};
use crate::machine::pci_machdep::{PciIntrFn, PciIntrStr};
use crate::sys::device::Device;

/// `pci_chipset_tag_t`.
pub type PciChipsetTag = &'static MachinePciChipset;

/// `pcitag_t`: the host bridge's node in the upper half, the configuration space offset in
/// the lower.
pub type Pcitag = u64;

/// `PCITAG_NODE(x)`.
pub const fn pcitag_node(x: Pcitag) -> u64 {
    x >> 32
}

/// `PCITAG_OFFSET(x)`.
pub const fn pcitag_offset(x: Pcitag) -> u64 {
    x & 0xffff_ffff
}

/// `PCI_NONE`: supported interrupt types.
pub const PCI_NONE: i32 = 0;
/// `PCI_INTX`.
pub const PCI_INTX: i32 = 1;
/// `PCI_MSI`.
pub const PCI_MSI: i32 = 2;
/// `PCI_MSIX`.
pub const PCI_MSIX: i32 = 3;

/// `pci_intr_handle_t`.
#[derive(Clone, Copy)]
pub struct PciIntrHandle {
    /// `ih_pc`.
    pub ih_pc: PciChipsetTag,
    /// `ih_tag`.
    pub ih_tag: Pcitag,
    /// `ih_intrpin`.
    pub ih_intrpin: i32,
    /// `ih_type`: `PCI_INTX`, `PCI_MSI` or `PCI_MSIX`.
    pub ih_type: i32,
    /// `ih_dmat`.
    pub ih_dmat: &'static BusDmaTag,
}

/// `struct machine_pci_chipset`: a host bridge's PCI functions (machine-specific; not to be
/// used directly by machine-independent code).
pub struct MachinePciChipset {
    /// `pc_conf_v`: the configuration functions' cookie.
    pub pc_conf_v: *mut c_void,
    /// `pc_attach_hook`.
    pub pc_attach_hook: fn(&Device, &Device, &PcibusAttachArgs),
    /// `pc_bus_maxdevs`.
    pub pc_bus_maxdevs: fn(*mut c_void, i32) -> i32,
    /// `pc_make_tag`.
    pub pc_make_tag: fn(*mut c_void, i32, i32, i32) -> Pcitag,
    /// `pc_decompose_tag`.
    pub pc_decompose_tag: fn(*mut c_void, Pcitag) -> (i32, i32, i32),
    /// `pc_conf_size`.
    pub pc_conf_size: fn(*mut c_void, Pcitag) -> i32,
    /// `pc_conf_read`.
    pub pc_conf_read: fn(*mut c_void, Pcitag, i32) -> Pcireg,
    /// `pc_conf_write`.
    pub pc_conf_write: fn(*mut c_void, Pcitag, i32, Pcireg),
    /// `pc_probe_device_hook`.
    pub pc_probe_device_hook: fn(*mut c_void, &mut PciAttachArgs) -> i32,

    /// `pc_intr_v`: the interrupt functions' cookie.
    pub pc_intr_v: *mut c_void,
    /// `pc_intr_map`.
    pub pc_intr_map: fn(&PciAttachArgs) -> Option<PciIntrHandle>,
    /// `pc_intr_map_msi`.
    pub pc_intr_map_msi: fn(&PciAttachArgs) -> Option<PciIntrHandle>,
    /// `pc_intr_map_msivec`.
    pub pc_intr_map_msivec: fn(&PciAttachArgs, i32) -> Option<PciIntrHandle>,
    /// `pc_intr_map_msix`.
    pub pc_intr_map_msix: fn(&PciAttachArgs, i32) -> Option<PciIntrHandle>,
    /// `pc_intr_string`.
    pub pc_intr_string: fn(*mut c_void, PciIntrHandle) -> PciIntrStr,
    /// `pc_intr_establish`.
    #[allow(clippy::type_complexity)] // the C's member
    pub pc_intr_establish: fn(
        *mut c_void,
        PciIntrHandle,
        i32,
        Option<&'static CpuInfo>,
        PciIntrFn,
        *mut c_void,
        &'static str,
    ) -> Option<NonNull<c_void>>,
    /// `pc_intr_disestablish`.
    pub pc_intr_disestablish: unsafe fn(*mut c_void, NonNull<c_void>),
}

// SAFETY: a chipset is filled once by its host bridge's attach and only read afterwards;
// the cookies are only used by the bridge's own functions.
unsafe impl Sync for MachinePciChipset {}
