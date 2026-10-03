//! Paravirtual devices: OpenBSD `sys/dev/pv/`.
//!
//! `virtioreg` and `virtiovar` are the virtio headers, `virtio` the core the transports
//! (`dev/pci/virtio_pci.rs`, `dev/fdt/virtio_mmio.rs`) and the device drivers share.

pub mod virtio;
pub mod virtioreg;
pub mod virtiovar;
