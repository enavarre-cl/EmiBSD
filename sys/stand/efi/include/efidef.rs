/* $FreeBSD: head/sys/boot/efi/include/efidef.h 279038 2015-02-20 01:40:55Z imp $ */

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

    efidef.h

Abstract:

    EFI definitions




Revision History

--*/
/* </LICENSES> */

//! EFI base definitions: GUID, time, network addresses, memory types and the memory descriptor.
//!
//! Upstream: sys/stand/efi/include/efidef.h @ 3ce1f3f79392
//!
//! The firmware-visible structures are `#[repr(C)]` with the specification's field names.
//! C enums are `int`-sized and firmware may hand back a value outside the enumerator list, so
//! every enum is a `u32` alias plus one constant per enumerator, not a Rust `enum`.
//!
//! ## Deviations
//! - `EFI_STATUS` is `EfiStatus` and the other typedefs are CamelCase (`EfiHandle`,
//!   `EfiGuid`, `EfiTime`, ...); the scalar `CHAR16`/`CHAR8`/`BOOLEAN` keep the specification
//!   names (`non_camel_case_types` is allowed for them).
//! - `EFI_GUID` gets a `const fn new`, so the GUID macros of the other headers are `const`s.
//! - `EFI_ALLOCATE_TYPE` and `EFI_MEMORY_TYPE` are `u32` aliases with constants (see above).
//! - `EFI_SIZE_TO_PAGES(a)` is the const fn [`efi_size_to_pages`].
//! - `TRUE`/`FALSE` are `BOOLEAN` constants; `NULL` is `core::ptr::null_mut()` and is not
//!   redefined; `IN`/`OUT`/`OPTIONAL` are documentation only and vanish.

#![allow(non_snake_case, non_upper_case_globals, non_camel_case_types)]
// EFI names are the specification's (CHAR16, EfiLoaderData, Data1, ...), kept verbatim for grep-ability

use core::ffi::c_void;

use super::amd64::efibind::{INT16, UINT8, UINT16, UINT32, UINT64, UINTN};

/// `CHAR16`: a UCS-2 code unit.
pub type CHAR16 = UINT16;
/// `CHAR8`: an 8-bit character.
pub type CHAR8 = UINT8;
/// `BOOLEAN`: one byte, 0 or 1.
pub type BOOLEAN = UINT8;

/// `TRUE`.
pub const TRUE: BOOLEAN = 1;
/// `FALSE`.
pub const FALSE: BOOLEAN = 0;

/// `EFI_STATUS`: a status code; the top bit marks an error.
pub type EfiStatus = UINTN;
/// `EFI_LBA`: a logical block address.
pub type EfiLba = UINT64;
/// `EFI_TPL`: a task priority level.
pub type EfiTpl = UINTN;
/// `EFI_HANDLE`: an opaque firmware handle.
pub type EfiHandle = *mut c_void;
/// `EFI_EVENT`: an opaque firmware event.
pub type EfiEvent = *mut c_void;

/// `EFI_TIME.Daylight` bit: the time is adjusted for daylight saving.
pub const EFI_TIME_ADJUST_DAYLIGHT: UINT8 = 0x01;
/// `EFI_TIME.Daylight` bit: the time is in daylight saving time.
pub const EFI_TIME_IN_DAYLIGHT: UINT8 = 0x02;
/// `EFI_TIME.TimeZone` value: no time zone.
pub const EFI_UNSPECIFIED_TIMEZONE: INT16 = 0x07FF;

/// `EFI_ALLOCATE_TYPE`.
pub type EfiAllocateType = u32;
/// `AllocateAnyPages`.
pub const AllocateAnyPages: EfiAllocateType = 0;
/// `AllocateMaxAddress`.
pub const AllocateMaxAddress: EfiAllocateType = 1;
/// `AllocateAddress`.
pub const AllocateAddress: EfiAllocateType = 2;
/// `MaxAllocateType`.
pub const MaxAllocateType: EfiAllocateType = 3;

/// `EFI_MEMORY_TYPE`.
pub type EfiMemoryType = u32;
/// `EfiReservedMemoryType`.
pub const EfiReservedMemoryType: EfiMemoryType = 0;
/// `EfiLoaderCode`.
pub const EfiLoaderCode: EfiMemoryType = 1;
/// `EfiLoaderData`.
pub const EfiLoaderData: EfiMemoryType = 2;
/// `EfiBootServicesCode`.
pub const EfiBootServicesCode: EfiMemoryType = 3;
/// `EfiBootServicesData`.
pub const EfiBootServicesData: EfiMemoryType = 4;
/// `EfiRuntimeServicesCode`.
pub const EfiRuntimeServicesCode: EfiMemoryType = 5;
/// `EfiRuntimeServicesData`.
pub const EfiRuntimeServicesData: EfiMemoryType = 6;
/// `EfiConventionalMemory`.
pub const EfiConventionalMemory: EfiMemoryType = 7;
/// `EfiUnusableMemory`.
pub const EfiUnusableMemory: EfiMemoryType = 8;
/// `EfiACPIReclaimMemory`.
pub const EfiACPIReclaimMemory: EfiMemoryType = 9;
/// `EfiACPIMemoryNVS`.
pub const EfiACPIMemoryNVS: EfiMemoryType = 10;
/// `EfiMemoryMappedIO`.
pub const EfiMemoryMappedIO: EfiMemoryType = 11;
/// `EfiMemoryMappedIOPortSpace`.
pub const EfiMemoryMappedIOPortSpace: EfiMemoryType = 12;
/// `EfiPalCode`.
pub const EfiPalCode: EfiMemoryType = 13;
/// `EfiMaxMemoryType`.
pub const EfiMaxMemoryType: EfiMemoryType = 14;

/// Memory attribute: uncacheable.
pub const EFI_MEMORY_UC: UINT64 = 0x0000_0000_0000_0001;
/// Memory attribute: write-combining.
pub const EFI_MEMORY_WC: UINT64 = 0x0000_0000_0000_0002;
/// Memory attribute: write-through.
pub const EFI_MEMORY_WT: UINT64 = 0x0000_0000_0000_0004;
/// Memory attribute: write-back.
pub const EFI_MEMORY_WB: UINT64 = 0x0000_0000_0000_0008;
/// Memory attribute: uncacheable, exported.
pub const EFI_MEMORY_UCE: UINT64 = 0x0000_0000_0000_0010;
/// Memory attribute: write-protected.
pub const EFI_MEMORY_WP: UINT64 = 0x0000_0000_0000_1000;
/// Memory attribute: read-protected.
pub const EFI_MEMORY_RP: UINT64 = 0x0000_0000_0000_2000;
/// Memory attribute: execute-protected.
pub const EFI_MEMORY_XP: UINT64 = 0x0000_0000_0000_4000;
/// Memory attribute: the range requires a runtime mapping.
pub const EFI_MEMORY_RUNTIME: UINT64 = 0x8000_0000_0000_0000;

/// `EFI_MEMORY_DESCRIPTOR_VERSION`.
pub const EFI_MEMORY_DESCRIPTOR_VERSION: UINT32 = 1;

/// `ISO_639_2`: one byte of an ISO 639-2 language code.
pub type ISO_639_2 = UINT8;
/// `ISO_639_2_ENTRY_SIZE`: the size of one language code.
pub const ISO_639_2_ENTRY_SIZE: UINTN = 3;

/// `EFI_PAGE_SIZE`.
pub const EFI_PAGE_SIZE: UINTN = 4096;
/// `EFI_PAGE_MASK`.
pub const EFI_PAGE_MASK: UINTN = 0xFFF;
/// `EFI_PAGE_SHIFT`.
pub const EFI_PAGE_SHIFT: UINTN = 12;

/// `EFI_PHYSICAL_ADDRESS`.
pub type EfiPhysicalAddress = UINT64;
/// `EFI_VIRTUAL_ADDRESS`.
pub type EfiVirtualAddress = UINT64;

/// `EFI_GUID`: a globally unique identifier.
#[repr(C)]
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct EfiGuid {
    /// `Data1`.
    pub Data1: UINT32,
    /// `Data2`.
    pub Data2: UINT16,
    /// `Data3`.
    pub Data3: UINT16,
    /// `Data4`.
    pub Data4: [UINT8; 8],
}

impl EfiGuid {
    /// Build a GUID from the four fields of the C initialiser `{ d1, d2, d3, { d4... } }`.
    pub const fn new(Data1: UINT32, Data2: UINT16, Data3: UINT16, Data4: [UINT8; 8]) -> Self {
        Self {
            Data1,
            Data2,
            Data3,
            Data4,
        }
    }
}

/// `EFI_TIME`.
#[repr(C)]
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct EfiTime {
    /// 1998 - 20XX.
    pub Year: UINT16,
    /// 1 - 12.
    pub Month: UINT8,
    /// 1 - 31.
    pub Day: UINT8,
    /// 0 - 23.
    pub Hour: UINT8,
    /// 0 - 59.
    pub Minute: UINT8,
    /// 0 - 59.
    pub Second: UINT8,
    /// Padding.
    pub Pad1: UINT8,
    /// 0 - 999,999,999.
    pub Nanosecond: UINT32,
    /// -1440 to 1440 or [`EFI_UNSPECIFIED_TIMEZONE`].
    pub TimeZone: INT16,
    /// `EFI_TIME_ADJUST_DAYLIGHT` and `EFI_TIME_IN_DAYLIGHT` bits.
    pub Daylight: UINT8,
    /// Padding.
    pub Pad2: UINT8,
}

/// `EFI_IPv4_ADDRESS`.
#[repr(C)]
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct EfiIpv4Address {
    /// `Addr`.
    pub Addr: [UINT8; 4],
}

/// `EFI_IPv6_ADDRESS`.
#[repr(C)]
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct EfiIpv6Address {
    /// `Addr`.
    pub Addr: [UINT8; 16],
}

/// `EFI_MAC_ADDRESS`.
#[repr(C)]
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct EfiMacAddress {
    /// `Addr`.
    pub Addr: [UINT8; 32],
}

/// `EFI_MEMORY_DESCRIPTOR`: one entry of the memory map. Firmware strides over entries by
/// the `DescriptorSize` that `GetMemoryMap` returns, not by this struct's size; see
/// `next_memory_descriptor` in `efiapi`.
#[repr(C)]
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct EfiMemoryDescriptor {
    /// An [`EfiMemoryType`] (32 bits, then 32 bits of padding).
    pub Type: UINT32,
    /// Padding.
    pub Pad: UINT32,
    /// Physical address of the first byte.
    pub PhysicalStart: EfiPhysicalAddress,
    /// Virtual address of the first byte.
    pub VirtualStart: EfiVirtualAddress,
    /// Size in 4 KiB pages.
    pub NumberOfPages: UINT64,
    /// `EFI_MEMORY_*` attribute bits.
    pub Attribute: UINT64,
}

/// `EFI_SIZE_TO_PAGES(a)`: the number of 4 KiB pages that hold `a` bytes.
pub const fn efi_size_to_pages(a: UINTN) -> UINTN {
    (a >> EFI_PAGE_SHIFT) + if (a & EFI_PAGE_MASK) != 0 { 1 } else { 0 }
}

const _: () = assert!(core::mem::size_of::<EfiGuid>() == 16);
const _: () = assert!(core::mem::size_of::<EfiTime>() == 16);
const _: () = assert!(core::mem::size_of::<EfiIpv4Address>() == 4);
const _: () = assert!(core::mem::size_of::<EfiIpv6Address>() == 16);
const _: () = assert!(core::mem::size_of::<EfiMacAddress>() == 32);
const _: () = assert!(core::mem::size_of::<EfiMemoryDescriptor>() == 40);

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn size_to_pages_rounds_up() {
        assert_eq!(efi_size_to_pages(0), 0);
        assert_eq!(efi_size_to_pages(1), 1);
        assert_eq!(efi_size_to_pages(4096), 1);
        assert_eq!(efi_size_to_pages(4097), 2);
        assert_eq!(efi_size_to_pages(3 * 4096 + 5), 4);
    }

    #[test]
    fn guid_new_keeps_fields() {
        let g = EfiGuid::new(1, 2, 3, [4, 5, 6, 7, 8, 9, 10, 11]);
        assert_eq!((g.Data1, g.Data2, g.Data3, g.Data4[7]), (1, 2, 3, 11));
    }
}
