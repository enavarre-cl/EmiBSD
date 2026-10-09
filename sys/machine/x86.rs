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
//! The x86 machine items a machine-independent x86-only driver is written against (cfg
//! `machine_x86`, emitted by `sys/build.rs` for amd64).
//!
//! OpenBSD's `dev/acpi/acpidmar.c` (the VT-d and AMD-Vi IOMMUs) lives in a
//! machine-independent directory but is listed only by the x86 `files.<arch>` and uses x86
//! headers directly: it builds a `struct pic` of its own for its fault interrupt and hands it
//! to `intr_establish` with `ioapic_edge_stubs` (`<machine/pic.h>`, `<machine/intr.h>`,
//! `<machine/i82093var.h>`), fills a `struct bus_dma_tag` with its own functions that call
//! the common `_bus_dma*` ones (`<machine/bus.h>`), reaches page tables through
//! `PMAP_DIRECT_MAP` and writes them back with `pmap_flush_cache` (`<machine/pmap.h>`), walks
//! `bios_memmap` (`<machine/biosvar.h>`), and establishes a PCI MSI by building a
//! `pci_intr_handle_t` (`<machine/pci_machdep.h>`, `APIC_INT_VIA_MSG`). This module is those
//! items of the selected machine, so the driver never names an architecture; it exists, like
//! the driver's items, only where the cfg is set (as `machine::pci_chipset` for arm64's
//! device-tree PCI bridges).
//!
//! `dev/acpi/acpicpu_x86.c` (acpicpu(4) on x86, M16e) is the other such driver: it sets the
//! machine's idle and suspend waits (`cpu_idle_cycle_fcn`, `cpu_suspend_cycle_fcn`), links
//! itself into `struct cpu_info` (`ci_acpicpudev`, `ci_mwait`, `ci_feature_tpmflags`), idles
//! with `sti; hlt`, `inb` or `monitor`/`mwait` (with `clflush`, `cpu_mwait_size`,
//! `cpu_mwait_states`, `cpu_vendor`), and sets `cpuspeed` and `setperf_prio`.
//!
//! `dev/ipmi.c`'s x86 part (`ipmi_probe`, amd64's mainbus, M16e) reads SMBIOS's IPMI device
//! information with `bios.c`'s `smbios_find_table` and `struct smbtable`
//! (`<machine/smbiosvar.h>`).

pub use crate::arch::current::amd64::bios::smbios_find_table;
pub use crate::arch::current::amd64::bus_dma::{
    _bus_dmamap_create, _bus_dmamap_destroy, _bus_dmamap_load, _bus_dmamap_load_mbuf,
    _bus_dmamap_load_raw, _bus_dmamap_load_uio, _bus_dmamap_sync, _bus_dmamap_unload,
    _bus_dmamem_alloc, _bus_dmamem_alloc_range, _bus_dmamem_free, _bus_dmamem_map,
    _bus_dmamem_mmap, _bus_dmamem_unmap,
};
pub use crate::arch::current::amd64::bus_space::{bus_space_read_8, bus_space_write_8};
pub use crate::arch::current::amd64::cpu::{CPU_MWAIT_SIZE, CPU_MWAIT_STATES, CPU_VENDOR};
pub use crate::arch::current::amd64::identcpu::CPUSPEED;
pub use crate::arch::current::amd64::intr::intr_establish;
pub use crate::arch::current::amd64::ioapic::ioapic_edge_stubs_table;
pub use crate::arch::current::amd64::machdep::{
    CPU_IDLE_CYCLE_FCN, CPU_SUSPEND_CYCLE_FCN, SETPERF_PRIO, bios_memmap, cpu_idle_cycle_hlt,
};
pub use crate::arch::current::amd64::pmap::{pmap_direct_map, pmap_flush_cache};
pub use crate::arch::current::include::biosvar::{BIOS_MAP_END, BIOS_MAP_RES, BiosMemmap};
pub use crate::arch::current::include::bus::{
    BUS_DMA_24BIT, BusDmaSegment, BusDmaTag, BusDmaTagT, BusDmamap, BusDmamapT,
};
pub use crate::arch::current::include::cpu::{
    CpuInfo, MWAIT_IDLING, MWAIT_ONLY, cpu_info_primary, cpu_is_primary,
};
pub use crate::arch::current::include::cpufunc::{
    clflush, intr_enable, monitor, mwait, read_rflags,
};
pub use crate::arch::current::include::i82093var::APIC_INT_VIA_MSG;
pub use crate::arch::current::include::intrdefs::IST_PULSE;
pub use crate::arch::current::include::pci_machdep::PciIntrHandle;
pub use crate::arch::current::include::pic::{PIC_MSI, Pic};
pub use crate::arch::current::include::pio::inb;
pub use crate::arch::current::include::psl::PSL_I;
pub use crate::arch::current::include::smbiosvar::Smbtable;
pub use crate::arch::current::include::specialreg::TPM_ARAT;
pub use crate::arch::current::pci::acpipci::acpipci_domain_to_seg;
/* </CODE> */
