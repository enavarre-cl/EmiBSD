//! The UEFI headers of `sys/stand/efi/include`, one module per header.
//!
//! Upstream: sys/stand/efi/include/
//!
//! `efi` is `efi.h`: it re-exports every header `efi.h` includes, so a user writes
//! `use efi::include::efi::*;` as C writes `#include <efi.h>`. `eficonsctl` is not included
//! by `efi.h` (the C code includes it on its own) and is used as `include::eficonsctl`.

pub mod amd64;
pub mod efi;
pub mod efi_nii;
pub mod efiapi;
pub mod eficon;
pub mod eficonsctl;
pub mod efidef;
pub mod efidevp;
pub mod efierr;
pub mod efifs;
pub mod efigop;
pub mod efinet;
pub mod efiprot;
pub mod efipxebc;
pub mod efiser;
