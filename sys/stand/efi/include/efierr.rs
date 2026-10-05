/* $FreeBSD: head/sys/boot/efi/include/efierr.h 163898 2006-11-02 02:42:48Z marcel $ */

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

    efierr.h

Abstract:

    EFI error codes




Revision History

--*/
/* </LICENSES> */

//! EFI status codes.
//!
//! Upstream: sys/stand/efi/include/efierr.h @ 3ce1f3f79392
//!
//! Errors are `EFIERR(n)`: the top bit of the `UINTN` status set. Warnings are small positive
//! values and are not errors.
//!
//! ## Deviations
//! - `EFI_ERROR(a)` is the const fn [`efi_error`] and `EFIWARN(a)` is [`efiwarn`].

use super::amd64::efibind::{INTN, efierr};
use super::efidef::EfiStatus;

/// `EFIWARN(a)`: a warning code is the value itself.
pub const fn efiwarn(a: EfiStatus) -> EfiStatus {
    a
}

/// `EFI_ERROR(a)`: true when the status, taken as signed, is negative (the error bit).
pub const fn efi_error(a: EfiStatus) -> bool {
    (a as INTN) < 0
}

/// `EFI_SUCCESS`.
pub const EFI_SUCCESS: EfiStatus = 0;
/// `EFI_LOAD_ERROR`.
pub const EFI_LOAD_ERROR: EfiStatus = efierr(1);
/// `EFI_INVALID_PARAMETER`.
pub const EFI_INVALID_PARAMETER: EfiStatus = efierr(2);
/// `EFI_UNSUPPORTED`.
pub const EFI_UNSUPPORTED: EfiStatus = efierr(3);
/// `EFI_BAD_BUFFER_SIZE`.
pub const EFI_BAD_BUFFER_SIZE: EfiStatus = efierr(4);
/// `EFI_BUFFER_TOO_SMALL`.
pub const EFI_BUFFER_TOO_SMALL: EfiStatus = efierr(5);
/// `EFI_NOT_READY`.
pub const EFI_NOT_READY: EfiStatus = efierr(6);
/// `EFI_DEVICE_ERROR`.
pub const EFI_DEVICE_ERROR: EfiStatus = efierr(7);
/// `EFI_WRITE_PROTECTED`.
pub const EFI_WRITE_PROTECTED: EfiStatus = efierr(8);
/// `EFI_OUT_OF_RESOURCES`.
pub const EFI_OUT_OF_RESOURCES: EfiStatus = efierr(9);
/// `EFI_VOLUME_CORRUPTED`.
pub const EFI_VOLUME_CORRUPTED: EfiStatus = efierr(10);
/// `EFI_VOLUME_FULL`.
pub const EFI_VOLUME_FULL: EfiStatus = efierr(11);
/// `EFI_NO_MEDIA`.
pub const EFI_NO_MEDIA: EfiStatus = efierr(12);
/// `EFI_MEDIA_CHANGED`.
pub const EFI_MEDIA_CHANGED: EfiStatus = efierr(13);
/// `EFI_NOT_FOUND`.
pub const EFI_NOT_FOUND: EfiStatus = efierr(14);
/// `EFI_ACCESS_DENIED`.
pub const EFI_ACCESS_DENIED: EfiStatus = efierr(15);
/// `EFI_NO_RESPONSE`.
pub const EFI_NO_RESPONSE: EfiStatus = efierr(16);
/// `EFI_NO_MAPPING`.
pub const EFI_NO_MAPPING: EfiStatus = efierr(17);
/// `EFI_TIMEOUT`.
pub const EFI_TIMEOUT: EfiStatus = efierr(18);
/// `EFI_NOT_STARTED`.
pub const EFI_NOT_STARTED: EfiStatus = efierr(19);
/// `EFI_ALREADY_STARTED`.
pub const EFI_ALREADY_STARTED: EfiStatus = efierr(20);
/// `EFI_ABORTED`.
pub const EFI_ABORTED: EfiStatus = efierr(21);
/// `EFI_ICMP_ERROR`.
pub const EFI_ICMP_ERROR: EfiStatus = efierr(22);
/// `EFI_TFTP_ERROR`.
pub const EFI_TFTP_ERROR: EfiStatus = efierr(23);
/// `EFI_PROTOCOL_ERROR`.
pub const EFI_PROTOCOL_ERROR: EfiStatus = efierr(24);

/// `EFI_WARN_UNKNOWN_GLYPH`.
pub const EFI_WARN_UNKNOWN_GLYPH: EfiStatus = efiwarn(1);
/// `EFI_WARN_DELETE_FAILURE`.
pub const EFI_WARN_DELETE_FAILURE: EfiStatus = efiwarn(2);
/// `EFI_WARN_WRITE_FAILURE`.
pub const EFI_WARN_WRITE_FAILURE: EfiStatus = efiwarn(3);
/// `EFI_WARN_BUFFER_TOO_SMALL`.
pub const EFI_WARN_BUFFER_TOO_SMALL: EfiStatus = efiwarn(4);

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn errors_have_the_top_bit_and_warnings_do_not() {
        assert!(!efi_error(EFI_SUCCESS));
        assert!(efi_error(EFI_LOAD_ERROR));
        assert!(efi_error(EFI_PROTOCOL_ERROR));
        assert!(!efi_error(EFI_WARN_UNKNOWN_GLYPH));
        assert_eq!(EFI_NOT_FOUND, 0x8000_0000_0000_000e);
    }
}
