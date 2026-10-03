//! Device-tree attachments of the generic drivers: OpenBSD `sys/dev/fdt/`.
//!
//! `pluart_fdt` finds the console PL011 (M4); the rest attach with autoconfiguration:
//! `virtio_mmio` is the virtio transport of the `virtio,mmio` nodes (`virtio* at fdt?`).

pub mod plrtc;
pub mod pluart_fdt;
pub mod virtio_mmio;
