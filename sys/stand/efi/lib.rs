/* <CODE> */
//! The UEFI types and protocols of OpenBSD's `sys/stand/efi/include`, shared by the
//! `efiboot` boot loaders (amd64's BOOTX64.EFI, arm64's BOOTAA64.EFI).
//!
//! The crate is the Rust form of the headers only (OpenBSD's `sys/stand/efi` has nothing
//! else): `include::efi` is `efi.h` and re-exports the headers it includes, so
//! `use efi::include::efi::*;` is `#include <efi.h>`. Every firmware-visible structure is
//! `#[repr(C)]` with the specification's field names, and every service is an
//! `extern "efiapi"` function pointer.

#![no_std]

pub mod include;
/* </CODE> */
