//! Paravirtual devices: OpenBSD `sys/dev/pv/`.
//!
//! `virtioreg` and `virtiovar` are the virtio headers, `virtio` the core the transports
//! (`dev/pci/virtio_pci.rs`, `dev/fdt/virtio_mmio.rs`) and the device drivers share;
//! `if_vio` is the network driver, `vio(4)`; `vioblk` the block driver, `vioblk(4)` (a SCSI
//! adapter), with its header `vioblkreg`, and `vioscsi` the SCSI host adapter driver,
//! `vioscsi(4)`, with `vioscsireg`.

pub mod if_vio;
pub mod vioblk;
pub mod vioblkreg;
pub mod vioscsi;
pub mod vioscsireg;
pub mod virtio;
pub mod virtioreg;
pub mod virtiovar;
