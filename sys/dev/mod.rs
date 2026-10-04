//! Device drivers: OpenBSD `sys/dev/`.
//!
//! Only drivers for hardware QEMU exposes are ported; others are `skipped: deferred-driver`.
//! `cons` is the console framework, `ic/` the chip drivers (`com(4)`, `pluart(4)`), `isa/` the
//! ISA bus definitions amd64 still needs.
//! `consfile` is the console-as-a-file stand-in until `/dev/console` exists (not OpenBSD
//! code, `ports.toml` `[[extra]]`).
//! ISA bus definitions amd64 still needs, `pci/` the PCI bus, `pv/` the paravirtual devices
//! (`virtio(4)`).

pub mod bio;
pub mod biovar;
pub mod clock_subr;
pub mod cons;
pub mod consfile;
pub mod efi;
pub mod fdt;
pub mod ic;
pub mod isa;
pub mod ofw;
pub mod pci;
pub mod pv;
pub mod rd;
pub mod rnd;
pub mod softraid;
pub mod softraid_concat;
pub mod softraid_crypto;
pub mod softraid_raid0;
pub mod softraid_raid1;
pub mod softraid_raid1c;
pub mod softraid_raid5;
pub mod softraid_raid6;
pub mod softraidvar;
pub mod vnd;
pub mod vndioctl;
