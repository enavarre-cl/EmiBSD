//! Device-tree attachments of the generic drivers: OpenBSD `sys/dev/fdt/`.
//!
//! `pluart_fdt` finds the console PL011 (M4); the rest attach with autoconfiguration:
//! `virtio_mmio` is the virtio transport of the `virtio,mmio` nodes (`virtio* at fdt?`);
//! `pciecam` is the generic ECAM PCIe host bridge (`pciecam* at fdt?`, M12), which
//! `files.arm64` lists: it is compiled where cfg `machine_pci_chipset` is set (`sys/build.rs`).

#[cfg(machine_pci_chipset)]
pub mod pciecam;
pub mod plrtc;
pub mod pluart_fdt;
pub mod virtio_mmio;
