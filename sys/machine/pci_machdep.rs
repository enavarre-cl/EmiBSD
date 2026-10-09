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
/* </LICENSES> */

/* <CODE> */
//! `<machine/pci_machdep.h>` as a trait: what machine-independent PCI code (`dev/pci/pci.c`,
//! `pci_map.c`, the drivers) asks of the machine: the chipset, tag and interrupt handle
//! types, configuration space access, and interrupt mapping and establishment.
//!
//! In C each architecture's header declares `pci_chipset_tag_t`, `pcitag_t` and
//! `pci_intr_handle_t` and the functions (or macros) below; here they are the associated
//! types and methods of [`PciMachdep`], with free functions carrying the C names so a driver
//! reads like its original. amd64 implements them with configuration mechanism #1 (or ECAM)
//! and its interrupt code (`arch/amd64/pci/pci_machdep.rs`); arm64 dispatches through the
//! host bridge's `struct machine_pci_chipset`, as its C header does.
//!
//! The functions that return `0` or `1` with an out parameter (`pci_intr_map*`) return
//! `Option`, `None` where the C returns 1; `pci_intr_enable_msivec` returns `true` where the
//! C returns nonzero (it failed). `pci_decompose_tag`'s three out pointers, any of them
//! NULL, are one returned tuple.

use core::ffi::c_void;
use core::fmt;
use core::ptr::NonNull;

use crate::dev::pci::pcivar::{PciAttachArgs, PcibusAttachArgs, Pcireg};
use crate::kern::subr_prf::{Str, snprintf};
use crate::machine::Machine;
use crate::machine::bus::{BusAddr, BusSpaceHandle, BusSpaceTag};
use crate::machine::cpu::CpuInfo;
use crate::sys::device::Device;
use crate::sys::errno::Errno;

/// `pci_chipset_tag_t`: the selected machine's chipset tag.
pub type PciChipsetTag = <Machine as PciMachdep>::PciChipsetTag;
/// `pcitag_t`: names one function's configuration space.
pub type Pcitag = <Machine as PciMachdep>::Pcitag;
/// `pci_intr_handle_t`: an interrupt `pci_intr_map*` found, for `pci_intr_establish`.
pub type PciIntrHandle = <Machine as PciMachdep>::PciIntrHandle;

/// [`PciMachdep::PCI_MSI_PER_BRIDGE`] of the selected machine.
pub const PCI_MSI_PER_BRIDGE: bool = <Machine as PciMachdep>::PCI_MSI_PER_BRIDGE;

/// The interrupt handler `pci_intr_establish` takes: `int (*)(void *)`.
pub type PciIntrFn = fn(*mut c_void) -> i32;

/// What `pci_intr_string` returns: the C's static `char irqstr[64]`, by value.
#[derive(Clone, Copy)]
pub struct PciIntrStr {
    buf: [u8; 64],
}

impl PciIntrStr {
    /// The formatted string, truncated to 63 bytes as `snprintf` into the C's buffer would.
    pub fn new(args: fmt::Arguments<'_>) -> Self {
        let mut buf = [0u8; 64];
        snprintf(&mut buf, args);
        Self { buf }
    }

    /// The bytes, up to the NUL.
    pub fn as_bytes(&self) -> &[u8] {
        let n = self
            .buf
            .iter()
            .position(|&c| c == 0)
            .unwrap_or(self.buf.len());
        &self.buf[..n]
    }
}

impl fmt::Display for PciIntrStr {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}", Str(self.as_bytes()))
    }
}

/// The machine-dependent PCI interface.
pub trait PciMachdep {
    /// `pci_chipset_tag_t`.
    type PciChipsetTag: Copy + 'static;
    /// `pcitag_t`.
    type Pcitag: Copy + PartialEq + Default + 'static;
    /// `pci_intr_handle_t`.
    type PciIntrHandle: Copy + 'static;

    /// Whether the machine decides MSI per host bridge (the x86 black and white lists behind
    /// `PCI_FLAGS_MSI_ENABLED`), which `virtio_pci` overrides: the C's `#if defined(__i386__)
    /// || defined(__amd64__)` there.
    const PCI_MSI_PER_BRIDGE: bool;

    /// `PCI_IO_START`: where a bridge's I/O window may begin (`ppb.c` defaults it to 0).
    const PCI_IO_START: u64;
    /// `PCI_IO_END`: where it must end (`ppb.c`'s default: `0xffffffff`).
    const PCI_IO_END: u64;
    /// `PCI_MEM_START`: where a bridge's memory window may begin (`ppb.c`'s default: 0).
    const PCI_MEM_START: u64;
    /// `PCI_MEM_END`: where it must end (`ppb.c`'s default: `0xffffffff`).
    const PCI_MEM_END: u64;

    /// `pci_attach_hook(parent, self, pba)`: the machine's look at a PCI bus being attached.
    fn pci_attach_hook(parent: &Device, self_: &Device, pba: &PcibusAttachArgs);

    /// `pci_bus_maxdevs(pc, busno)`: how many device numbers bus `busno` has.
    fn pci_bus_maxdevs(pc: Self::PciChipsetTag, busno: i32) -> i32;

    /// `pci_lookup_segment(segment, bus)`: the chipset of PCI segment `segment` (whose
    /// bus `bus` the caller is about to address). `None` where the C's answer is a NULL no
    /// configuration access can go through (arm64 without that segment); amd64's NULL is
    /// its one chipset, so amd64 answers `Some`.
    fn pci_lookup_segment(segment: i32, bus: i32) -> Option<Self::PciChipsetTag>;

    /// `pci_mcfg_init(iot, addr, segment, min_bus, max_bus)`: the ECAM window at `addr` of
    /// buses `min_bus..=max_bus` of PCI segment `segment`, from the ACPI MCFG table
    /// (`acpimcfg`). amd64 records segment 0's for extended configuration space; arm64 maps
    /// it and makes the segment's chipset.
    fn pci_mcfg_init(iot: BusSpaceTag, addr: BusAddr, segment: i32, min_bus: i32, max_bus: i32);

    /// `pci_make_tag(pc, bus, device, function)`.
    fn pci_make_tag(pc: Self::PciChipsetTag, bus: i32, device: i32, function: i32) -> Self::Pcitag;

    /// `pci_decompose_tag(pc, tag, &bus, &device, &function)`.
    fn pci_decompose_tag(pc: Self::PciChipsetTag, tag: Self::Pcitag) -> (i32, i32, i32);

    /// `PCITAG_NODE(tag)` (`__HAVE_FDT`): the device-tree node of the function, 0 when it has
    /// none or the machine has no device tree.
    fn pcitag_node(tag: Self::Pcitag) -> i32;

    /// `pci_conf_size(pc, tag)`: the size of the function's configuration space.
    fn pci_conf_size(pc: Self::PciChipsetTag, tag: Self::Pcitag) -> i32;

    /// `pci_conf_read(pc, tag, reg)`: reads the 32-bit register at `reg` (4-byte aligned).
    fn pci_conf_read(pc: Self::PciChipsetTag, tag: Self::Pcitag, reg: i32) -> Pcireg;

    /// `pci_conf_write(pc, tag, reg, data)`.
    fn pci_conf_write(pc: Self::PciChipsetTag, tag: Self::Pcitag, reg: i32, data: Pcireg);

    /// `pci_probe_device_hook(pc, pa)`: lets the machine alter the attach arguments or skip
    /// the device (nonzero).
    fn pci_probe_device_hook(pc: Self::PciChipsetTag, pa: &mut PciAttachArgs) -> i32;

    /// `pci_dev_postattach(dev, pa)`: the machine's look at a device after it attached.
    fn pci_dev_postattach(dev: &Device, pa: &PciAttachArgs);

    /// `pci_min_powerstate(pc, tag)`: the lowest power state the device may be put in.
    fn pci_min_powerstate(pc: Self::PciChipsetTag, tag: Self::Pcitag) -> Pcireg;

    /// `pci_set_powerstate_md(pc, tag, state, pre)`: tells the firmware about a power state
    /// change, before (`pre` 1) and after (`pre` 0).
    fn pci_set_powerstate_md(pc: Self::PciChipsetTag, tag: Self::Pcitag, state: i32, pre: i32);

    /// `pci_msix_table_map(pc, tag, memt, &memh)`: maps the device's MSI-X table.
    fn pci_msix_table_map(
        pc: Self::PciChipsetTag,
        tag: Self::Pcitag,
        memt: BusSpaceTag,
    ) -> Result<BusSpaceHandle, Errno>;

    /// `pci_msix_table_unmap(pc, tag, memt, memh)`.
    fn pci_msix_table_unmap(
        pc: Self::PciChipsetTag,
        tag: Self::Pcitag,
        memt: BusSpaceTag,
        memh: BusSpaceHandle,
    );

    /// `pci_intr_enable_msivec(pa, num_vec)`: enables `num_vec` MSI vectors; `true` when it
    /// cannot.
    fn pci_intr_enable_msivec(pa: &PciAttachArgs, num_vec: i32) -> bool;

    /// `pci_intr_map_msi(pa, &ih)`: the device's MSI, if it can use one.
    fn pci_intr_map_msi(pa: &PciAttachArgs) -> Option<Self::PciIntrHandle>;

    /// `pci_intr_map_msivec(pa, vec, &ih)`: MSI vector `vec`.
    fn pci_intr_map_msivec(pa: &PciAttachArgs, vec: i32) -> Option<Self::PciIntrHandle>;

    /// `pci_intr_map_msix(pa, vec, &ih)`: MSI-X table entry `vec`.
    fn pci_intr_map_msix(pa: &PciAttachArgs, vec: i32) -> Option<Self::PciIntrHandle>;

    /// `pci_intr_map(pa, &ih)`: the device's INTx interrupt.
    fn pci_intr_map(pa: &PciAttachArgs) -> Option<Self::PciIntrHandle>;

    /// `pci_intr_string(pc, ih)`: describes the interrupt for the attach line.
    fn pci_intr_string(pc: Self::PciChipsetTag, ih: Self::PciIntrHandle) -> PciIntrStr;

    /// `pci_intr_establish_cpu(pc, ih, level, ci, func, arg, what)`: registers `func(arg)`
    /// at `level` on `ci` (any CPU when `None`); returns the cookie
    /// `pci_intr_disestablish` takes.
    #[allow(clippy::too_many_arguments)] // the C's signature
    fn pci_intr_establish_cpu(
        pc: Self::PciChipsetTag,
        ih: Self::PciIntrHandle,
        level: i32,
        ci: Option<&'static CpuInfo>,
        func: PciIntrFn,
        arg: *mut c_void,
        what: &'static str,
    ) -> Option<NonNull<c_void>>;

    /// `pci_intr_disestablish(pc, cookie)`.
    ///
    /// # Safety
    ///
    /// `cookie` came from `pci_intr_establish*` and is not used afterwards.
    unsafe fn pci_intr_disestablish(pc: Self::PciChipsetTag, cookie: NonNull<c_void>);
}

/// `PCI_IO_START` on the selected machine.
pub const PCI_IO_START: u64 = <Machine as PciMachdep>::PCI_IO_START;
/// `PCI_IO_END` on the selected machine.
pub const PCI_IO_END: u64 = <Machine as PciMachdep>::PCI_IO_END;
/// `PCI_MEM_START` on the selected machine.
pub const PCI_MEM_START: u64 = <Machine as PciMachdep>::PCI_MEM_START;
/// `PCI_MEM_END` on the selected machine.
pub const PCI_MEM_END: u64 = <Machine as PciMachdep>::PCI_MEM_END;

/// `pci_attach_hook` on the selected machine.
pub fn pci_attach_hook(parent: &Device, self_: &Device, pba: &PcibusAttachArgs) {
    Machine::pci_attach_hook(parent, self_, pba)
}

/// `pci_bus_maxdevs` on the selected machine.
pub fn pci_bus_maxdevs(pc: PciChipsetTag, busno: i32) -> i32 {
    Machine::pci_bus_maxdevs(pc, busno)
}

/// `pci_lookup_segment` on the selected machine.
pub fn pci_lookup_segment(segment: i32, bus: i32) -> Option<PciChipsetTag> {
    Machine::pci_lookup_segment(segment, bus)
}

/// `pci_mcfg_init` on the selected machine.
pub fn pci_mcfg_init(iot: BusSpaceTag, addr: BusAddr, segment: i32, min_bus: i32, max_bus: i32) {
    Machine::pci_mcfg_init(iot, addr, segment, min_bus, max_bus)
}

/// `pci_make_tag` on the selected machine.
pub fn pci_make_tag(pc: PciChipsetTag, bus: i32, device: i32, function: i32) -> Pcitag {
    Machine::pci_make_tag(pc, bus, device, function)
}

/// `pci_decompose_tag` on the selected machine: `(bus, device, function)`.
pub fn pci_decompose_tag(pc: PciChipsetTag, tag: Pcitag) -> (i32, i32, i32) {
    Machine::pci_decompose_tag(pc, tag)
}

/// `PCITAG_NODE` on the selected machine.
pub fn pcitag_node(tag: Pcitag) -> i32 {
    Machine::pcitag_node(tag)
}

/// `pci_conf_size` on the selected machine.
pub fn pci_conf_size(pc: PciChipsetTag, tag: Pcitag) -> i32 {
    Machine::pci_conf_size(pc, tag)
}

/// `pci_conf_read` on the selected machine.
pub fn pci_conf_read(pc: PciChipsetTag, tag: Pcitag, reg: i32) -> Pcireg {
    Machine::pci_conf_read(pc, tag, reg)
}

/// `pci_conf_write` on the selected machine.
pub fn pci_conf_write(pc: PciChipsetTag, tag: Pcitag, reg: i32, data: Pcireg) {
    Machine::pci_conf_write(pc, tag, reg, data)
}

/// `pci_probe_device_hook` on the selected machine.
pub fn pci_probe_device_hook(pc: PciChipsetTag, pa: &mut PciAttachArgs) -> i32 {
    Machine::pci_probe_device_hook(pc, pa)
}

/// `pci_dev_postattach` on the selected machine.
pub fn pci_dev_postattach(dev: &Device, pa: &PciAttachArgs) {
    Machine::pci_dev_postattach(dev, pa)
}

/// `pci_min_powerstate` on the selected machine.
pub fn pci_min_powerstate(pc: PciChipsetTag, tag: Pcitag) -> Pcireg {
    Machine::pci_min_powerstate(pc, tag)
}

/// `pci_set_powerstate_md` on the selected machine.
pub fn pci_set_powerstate_md(pc: PciChipsetTag, tag: Pcitag, state: i32, pre: i32) {
    Machine::pci_set_powerstate_md(pc, tag, state, pre)
}

/// `pci_msix_table_map` on the selected machine.
pub fn pci_msix_table_map(
    pc: PciChipsetTag,
    tag: Pcitag,
    memt: BusSpaceTag,
) -> Result<BusSpaceHandle, Errno> {
    Machine::pci_msix_table_map(pc, tag, memt)
}

/// `pci_msix_table_unmap` on the selected machine.
pub fn pci_msix_table_unmap(
    pc: PciChipsetTag,
    tag: Pcitag,
    memt: BusSpaceTag,
    memh: BusSpaceHandle,
) {
    Machine::pci_msix_table_unmap(pc, tag, memt, memh)
}

/// `pci_intr_enable_msivec` on the selected machine.
pub fn pci_intr_enable_msivec(pa: &PciAttachArgs, num_vec: i32) -> bool {
    Machine::pci_intr_enable_msivec(pa, num_vec)
}

/// `pci_intr_map_msi` on the selected machine.
pub fn pci_intr_map_msi(pa: &PciAttachArgs) -> Option<PciIntrHandle> {
    Machine::pci_intr_map_msi(pa)
}

/// `pci_intr_map_msivec` on the selected machine.
pub fn pci_intr_map_msivec(pa: &PciAttachArgs, vec: i32) -> Option<PciIntrHandle> {
    Machine::pci_intr_map_msivec(pa, vec)
}

/// `pci_intr_map_msix` on the selected machine.
pub fn pci_intr_map_msix(pa: &PciAttachArgs, vec: i32) -> Option<PciIntrHandle> {
    Machine::pci_intr_map_msix(pa, vec)
}

/// `pci_intr_map` on the selected machine.
pub fn pci_intr_map(pa: &PciAttachArgs) -> Option<PciIntrHandle> {
    Machine::pci_intr_map(pa)
}

/// `pci_intr_string` on the selected machine.
pub fn pci_intr_string(pc: PciChipsetTag, ih: PciIntrHandle) -> PciIntrStr {
    Machine::pci_intr_string(pc, ih)
}

/// `pci_intr_establish(pc, ih, level, func, arg, what)`: as `pci_intr_establish_cpu` on
/// any CPU.
pub fn pci_intr_establish(
    pc: PciChipsetTag,
    ih: PciIntrHandle,
    level: i32,
    func: PciIntrFn,
    arg: *mut c_void,
    what: &'static str,
) -> Option<NonNull<c_void>> {
    Machine::pci_intr_establish_cpu(pc, ih, level, None, func, arg, what)
}

/// `pci_intr_establish_cpu` on the selected machine.
#[allow(clippy::too_many_arguments)] // the C's signature
pub fn pci_intr_establish_cpu(
    pc: PciChipsetTag,
    ih: PciIntrHandle,
    level: i32,
    ci: Option<&'static CpuInfo>,
    func: PciIntrFn,
    arg: *mut c_void,
    what: &'static str,
) -> Option<NonNull<c_void>> {
    Machine::pci_intr_establish_cpu(pc, ih, level, ci, func, arg, what)
}

/// `pci_intr_disestablish` on the selected machine.
///
/// # Safety
///
/// As for [`PciMachdep::pci_intr_disestablish`].
pub unsafe fn pci_intr_disestablish(pc: PciChipsetTag, cookie: NonNull<c_void>) {
    // SAFETY: forwarded.
    unsafe { Machine::pci_intr_disestablish(pc, cookie) }
}
/* </CODE> */
