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

pub use crate::arch::current::amd64::bus_dma::{
    _bus_dmamap_create, _bus_dmamap_destroy, _bus_dmamap_load, _bus_dmamap_load_mbuf,
    _bus_dmamap_load_raw, _bus_dmamap_load_uio, _bus_dmamap_sync, _bus_dmamap_unload,
    _bus_dmamem_alloc, _bus_dmamem_alloc_range, _bus_dmamem_free, _bus_dmamem_map,
    _bus_dmamem_mmap, _bus_dmamem_unmap,
};
pub use crate::arch::current::amd64::bus_space::{bus_space_read_8, bus_space_write_8};
pub use crate::arch::current::amd64::intr::intr_establish;
pub use crate::arch::current::amd64::ioapic::ioapic_edge_stubs_table;
pub use crate::arch::current::amd64::machdep::bios_memmap;
pub use crate::arch::current::amd64::pmap::{pmap_direct_map, pmap_flush_cache};
pub use crate::arch::current::include::biosvar::{BIOS_MAP_END, BIOS_MAP_RES, BiosMemmap};
pub use crate::arch::current::include::bus::{
    BUS_DMA_24BIT, BusDmaSegment, BusDmaTag, BusDmaTagT, BusDmamap, BusDmamapT,
};
pub use crate::arch::current::include::cpu::CpuInfo;
pub use crate::arch::current::include::i82093var::APIC_INT_VIA_MSG;
pub use crate::arch::current::include::intrdefs::IST_PULSE;
pub use crate::arch::current::include::pci_machdep::PciIntrHandle;
pub use crate::arch::current::include::pic::{PIC_MSI, Pic};
pub use crate::arch::current::pci::acpipci::acpipci_domain_to_seg;
/* </CODE> */
