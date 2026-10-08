/* $FreeBSD: head/sys/boot/efi/include/efi_nii.h 163898 2006-11-02 02:42:48Z marcel $ */
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

Module name:
    efi_nii.h

Abstract:

Revision history:
    2000-Feb-18 M(f)J   GUID updated.
                Structure order changed for machine word alignment.
                Added StringId[4] to structure.

    2000-Feb-14 M(f)J   Genesis.
--*/
/* </LICENSES> */

/* <CODE> */
//! EFI network interface identifier protocol (NII).
//!
//! Upstream: sys/stand/efi/include/efi_nii.h @ 3ce1f3f79392
//!
//! ## Deviations
//! - `EFI_NETWORK_INTERFACE_IDENTIFIER_INTERFACE` is `EfiNetworkInterfaceIdentifierInterface`;
//!   `EFI_NETWORK_INTERFACE_TYPE` is a `u32` alias plus its one enumerator.
//! - The two `extern EFI_GUID NetworkInterfaceIdentifierProtocol[_31]` declarations are not
//!   ported: no C file of the tree defines them (the GUIDs are the macros), and a Rust
//!   `extern` static would be an unresolved symbol. The GUID macros are the `EfiGuid`
//!   constants `EFI_NETWORK_INTERFACE_IDENTIFIER_PROTOCOL[_31]`.

#![allow(non_snake_case, non_upper_case_globals, non_camel_case_types)]
// EFI names are the specification's (EfiNetworkInterfaceUndi, StringId, ...), kept verbatim for grep-ability

use super::amd64::efibind::{UINT8, UINT32, UINT64};
use super::efidef::{BOOLEAN, CHAR8, EfiGuid};

/// `EFI_NETWORK_INTERFACE_IDENTIFIER_PROTOCOL`.
pub const EFI_NETWORK_INTERFACE_IDENTIFIER_PROTOCOL: EfiGuid = EfiGuid::new(
    0xE18541CD,
    0xF755,
    0x4f73,
    [0x92, 0x8D, 0x64, 0x3C, 0x8A, 0x79, 0xB2, 0x29],
);
/// `EFI_NETWORK_INTERFACE_IDENTIFIER_PROTOCOL_31`.
pub const EFI_NETWORK_INTERFACE_IDENTIFIER_PROTOCOL_31: EfiGuid = EfiGuid::new(
    0x1ACED566,
    0x76ED,
    0x4218,
    [0xBC, 0x81, 0x76, 0x7F, 0x1F, 0x97, 0x7A, 0x89],
);

/// `EFI_NETWORK_INTERFACE_IDENTIFIER_INTERFACE_REVISION`.
pub const EFI_NETWORK_INTERFACE_IDENTIFIER_INTERFACE_REVISION: UINT64 = 0x00010000;
/// `EFI_NETWORK_INTERFACE_IDENTIFIER_INTERFACE_REVISION_31`.
pub const EFI_NETWORK_INTERFACE_IDENTIFIER_INTERFACE_REVISION_31: UINT64 = 0x00010001;

/// `EFI_NETWORK_INTERFACE_TYPE`.
pub type EfiNetworkInterfaceType = u32;
/// `EfiNetworkInterfaceUndi`.
pub const EfiNetworkInterfaceUndi: EfiNetworkInterfaceType = 1;

/// `EFI_NETWORK_INTERFACE_IDENTIFIER_INTERFACE`.
#[repr(C)]
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct EfiNetworkInterfaceIdentifierInterface {
    /// Revision of the network interface identifier protocol interface.
    pub Revision: UINT64,
    /// Address of the first byte of the identifying structure for this network interface,
    /// zero when there is none. For PXE/UNDI this is the first byte of the `!PXE` structure.
    pub ID: UINT64,
    /// Address of the unrelocated driver/ROM image, zero when there is none.
    pub ImageAddr: UINT64,
    /// Size of the unrelocated driver/ROM image, zero when there is none.
    pub ImageSize: UINT32,
    /// Four ASCII characters for the DHCP class identifier (option 60): "UNDI" or "SNPN".
    pub StringId: [CHAR8; 4],
    /// Network interface type, for DHCP option 94.
    pub Type: UINT8,
    /// Major version, for DHCP option 94.
    pub MajorVer: UINT8,
    /// Minor version, for DHCP option 94.
    pub MinorVer: UINT8,
    /// The interface supports IPv6.
    pub Ipv6Supported: BOOLEAN,
    /// Interface number to be used with the `pxeid` structure.
    pub IfNum: UINT8,
}

const _: () = assert!(core::mem::size_of::<EfiNetworkInterfaceIdentifierInterface>() == 40);
/* </CODE> */
