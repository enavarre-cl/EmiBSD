//! The machine-independent half of efi(4): OpenBSD `sys/dev/efi/`. `efi` is `efi.h`, the UEFI
//! types the arch drivers (`arch/arm64/dev/efi_machdep.rs`) use; `efi.c`, the `/dev/efi`
//! device, is not ported yet.

#[allow(clippy::module_inception)] // OpenBSD's layout: sys/dev/efi/efi.h
pub mod efi;
