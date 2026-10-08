/* <CODE> */
//! Device-tree attachments of the generic drivers: OpenBSD `sys/dev/fdt/`.
//!
//! `pluart_fdt` finds the console PL011 (M4); the rest attach with autoconfiguration:
//! `virtio_mmio` is the virtio transport of the `virtio,mmio` nodes (`virtio* at fdt?`);
//! `psci` (with `pscivar`) is PSCI, `psci* at fdt? early 1` (M13); `simplefb` the
//! firmware's frame buffer, `simplefb* at fdt?` (M13); `pciecam` is the generic ECAM PCIe host bridge (`pciecam* at fdt?`, M12), which
//! `files.arm64` lists: it is compiled where cfg `machine_pci_chipset` is set (`sys/build.rs`);
//! `ipmi_fdt` is ipmi(4) on an `ipmi-kcs` node (`ipmi* at fdt?`, M16e).

pub mod ipmi_fdt;
#[cfg(machine_pci_chipset)]
pub mod pciecam;
pub mod plrtc;
pub mod pluart_fdt;
pub mod psci;
pub mod pscivar;
pub mod simplefb;
pub mod virtio_mmio;
/* </CODE> */
