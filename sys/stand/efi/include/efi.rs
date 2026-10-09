/* <LICENSES> */
/*++

Copyright (c)  1999 - 2002 Intel Corporation. All rights reserved
This software and associated documentation (if any) is furnished
under a license and may only be used or copied in accordance
with the terms of the license. Except as permitted by such
license, no part of this software or documentation may be
reproduced, stored in a retrieval system, or transmitted in any
form or by any means without the express written consent of
Intel Corporation.

Module Name:

    efi.h

Abstract:

    Public EFI header files



Revision History

--*/
/* </LICENSES> */

/* <CODE> */
//! The public EFI header: includes every other header of the set.
//!
//! Upstream: sys/stand/efi/include/efi.h @ 3ce1f3f79392
//!
//! `efi.h` is the one header the C code includes (`#include <efi.h>`); it pulls in `efibind.h`,
//! `efidef.h`, `efidevp.h`, `efiprot.h`, `eficon.h`, `efiser.h`, `efi_nii.h`, `efipxebc.h`,
//! `efinet.h`, `efiapi.h`, `efifs.h`, `efierr.h` and `efigop.h`. This module re-exports all
//! of them, so `use efi::include::efi::*;` brings the same names into scope.
//!
//! ## Deviations
//! - `eficonsctl.h` is not included by `efi.h` and is not re-exported; use
//!   `efi::include::eficonsctl`.
//! - `EFI_FIRMWARE_VENDOR` (`L"INTEL"`) is a `[u16; 6]` constant including the NUL.
//! - `EFI_STRINGIZE`, `EFI_PROTOCOL_DEFINITION`, `EFI_GUID_DEFINITION` and `EFI_GUID_STRING`
//!   are preprocessor-only helpers (stringising an include path, an empty macro) with no
//!   meaning in Rust and are not ported.
//! - The build-flag comment (`EFI32`, `EFI_DEBUG`, `EFI_NT_EMULATOR`) has no counterpart:
//!   there is one 64-bit configuration.

#![allow(non_upper_case_globals)]
// EFI_FIRMWARE_* keep their C names

pub use super::amd64::efibind::*;
pub use super::efi_nii::*;
pub use super::efiapi::*;
pub use super::eficon::*;
pub use super::efidef::*;
pub use super::efidevp::*;
pub use super::efierr::*;
pub use super::efifs::*;
pub use super::efigop::*;
pub use super::efinet::*;
pub use super::efiprot::*;
pub use super::efipxebc::*;
pub use super::efiser::*;

/// `EFI_FIRMWARE_VENDOR`: `L"INTEL"`, NUL-terminated.
pub const EFI_FIRMWARE_VENDOR: [u16; 6] = [
    b'I' as u16,
    b'N' as u16,
    b'T' as u16,
    b'E' as u16,
    b'L' as u16,
    0,
];
/// `EFI_FIRMWARE_MAJOR_REVISION`.
pub const EFI_FIRMWARE_MAJOR_REVISION: u32 = 14;
/// `EFI_FIRMWARE_MINOR_REVISION`.
pub const EFI_FIRMWARE_MINOR_REVISION: u32 = 62;
/// `EFI_FIRMWARE_REVISION`.
pub const EFI_FIRMWARE_REVISION: u32 =
    (EFI_FIRMWARE_MAJOR_REVISION << 16) | EFI_FIRMWARE_MINOR_REVISION;
/* </CODE> */

/* <TESTS> */
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn one_import_brings_the_whole_set() {
        let g: EfiGuid = LOADED_IMAGE_PROTOCOL;
        assert_eq!(g.Data1, 0x5B1B31A1);
        assert!(efi_error(EFI_LOAD_ERROR));
        assert_eq!(efi_size_to_pages(1), 1);
        assert_eq!(EFI_FIRMWARE_REVISION, 0x000e_003e);
        assert_eq!(core::mem::size_of::<UINTN>(), 8);
        assert_eq!(EfiLoaderData, 2);
        assert_eq!(MEDIA_HARDDRIVE_DP, 1);
        assert_eq!(SCAN_ESC, 0x17);
    }
}
/* </TESTS> */
