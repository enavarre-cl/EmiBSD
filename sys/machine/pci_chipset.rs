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
//! The machine items a device-tree PCI host bridge driver is written against, on the machines
//! whose `<machine/pci_machdep.h>` is a `struct machine_pci_chipset` table (cfg
//! `machine_pci_chipset`, emitted by `sys/build.rs` for arm64).
//!
//! OpenBSD's `dev/fdt/pciecam.c` (and `dwpcie.c`, `rkpcie.c`, ...) are machine-independent
//! files that use machine headers directly: they fill a `struct machine_pci_chipset` with
//! their configuration and interrupt functions and build `pci_intr_handle_t`s
//! (`<machine/pci_machdep.h>`), copy their parent's `struct bus_space` to override
//! `_space_map` and `_space_mmap` (`<machine/bus.h>`), wrap the interrupt cookie in a
//! `struct machine_intr_handle` under their own `struct interrupt_controller`
//! (`<machine/intr.h>`), and call the MSI helpers of the machine's `pci_machdep.c`. Only the
//! machines with these headers (arm64 here; armv7, riscv64 and powerpc64 in OpenBSD) list
//! such a driver in their `files.<arch>`, and only they compile it. This module is those
//! items of the selected machine, so the drivers never name an architecture; it exists, like
//! the drivers, only where the cfg is set. The `fdt_intr_establish_imap*` and
//! `fdt_intr_establish_msi*` functions such a driver also calls are the `machine::fdt`
//! contract.

pub use crate::arch::current::dev::pci_machdep::{
    _pci_intr_map_msi, _pci_intr_map_msivec, _pci_intr_map_msix, pci_msi_enable, pci_msix_enable,
};
pub use crate::arch::current::include::bus::{BusSpace, bus_space_mmap};
pub use crate::arch::current::include::intr::{InterruptController, MachineIntrHandle};
pub use crate::arch::current::include::pci_machdep::{
    MachinePciChipset, PCI_INTX, PCI_MSI, PCI_MSIX, PCI_NONE, PciIntrHandle,
};
/* </CODE> */
