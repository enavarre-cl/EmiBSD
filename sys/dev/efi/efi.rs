/* <LICENSES> */
/*
 * Copyright (c) 2026 Emilio Navarrete Lineros <enavarre@outlook.com>
 *
 * Permission to use, copy, modify, and distribute this software for any
 * purpose with or without fee is hereby granted, provided that the above
 * copyright notice and this permission notice appear in all copies.
 *
 * THE SOFTWARE IS PROVIDED "AS IS" AND THE AUTHOR DISCLAIMS ALL WARRANTIES
 * WITH REGARD TO THIS SOFTWARE INCLUDING ALL IMPLIED WARRANTIES OF
 * MERCHANTABILITY AND FITNESS. IN NO EVENT SHALL THE AUTHOR BE LIABLE FOR
 * ANY SPECIAL, DIRECT, INDIRECT, OR CONSEQUENTIAL DAMAGES OR ANY DAMAGES
 * WHATSOEVER RESULTING FROM LOSS OF USE, DATA OR PROFITS, WHETHER IN AN
 * ACTION OF CONTRACT, NEGLIGENCE OR OTHER TORTIOUS ACTION, ARISING OUT OF
 * OR IN CONNECTION WITH THE USE OR PERFORMANCE OF THIS SOFTWARE.
 */

/* Public Domain */
/* </LICENSES> */

/* <CODE> */
//! `<dev/efi/efi.h>`: the UEFI tables, memory map and runtime services the kernel uses.
//!
//! Upstream: sys/dev/efi/efi.h @ 3ce1f3f79392
//!
//! ## Deviations
//! - `EFIAPI` (`ms_abi` on amd64, the platform's C convention elsewhere) is Rust's
//!   `extern "efiapi"`, which makes the same choice.
//! - The scalar typedefs (`UINT8`, ..., `CHAR16`, `VOID`) are the Rust integer types and
//!   `c_void`; `UINTN`, `EFI_STATUS`, `EFI_HANDLE` and the address types keep a name.
//! - `EFI_MEMORY_TYPE` and `EFI_RESET_TYPE` are integer constants (the descriptor's `Type`
//!   is a `UINT32`), named as the C enumerators.
//! - The runtime services are `Option`s of function pointers: a firmware may leave one
//!   null, which the C would call anyway.
//! - `NextMemoryDescriptor` is `EfiMemoryDescriptor::read_at`, which reads the descriptor
//!   at a byte offset of the map (the map's descriptor size may exceed the structure's).
//! - `efi_guidcmp` compares the GUIDs and returns `bool` (equal), as its callers only test
//!   `== 0`.

#![allow(non_upper_case_globals)] // the C enumerators are CamelCase constants

use core::ffi::c_void;
use core::mem::size_of;
use core::ptr;

/// `UINTN`: the native unsigned integer.
pub type Uintn = usize;
/// `EFI_PHYSICAL_ADDRESS`.
pub type EfiPhysicalAddress = u64;
/// `EFI_VIRTUAL_ADDRESS`.
pub type EfiVirtualAddress = u64;
/// `EFI_STATUS`: 0 or an `EFIERR` code.
pub type EfiStatus = Uintn;
/// `EFI_HANDLE`.
pub type EfiHandle = *mut c_void;

/// `EFI_SIMPLE_TEXT_INPUT_PROTOCOL`: opaque here.
pub type EfiSimpleTextInputProtocol = *mut c_void;
/// `EFI_SIMPLE_TEXT_OUTPUT_PROTOCOL`: opaque here.
pub type EfiSimpleTextOutputProtocol = *mut c_void;
/// `EFI_BOOT_SERVICES`: opaque here (gone once the kernel runs).
pub type EfiBootServices = *mut c_void;

/// `EFIERR(x)`: an error status, the top bit set.
#[allow(non_snake_case)] // the C name
pub const fn EFIERR(x: Uintn) -> EfiStatus {
    (1 << (Uintn::BITS - 1)) | x
}

/// `EFI_GUID`.
#[repr(C)]
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[allow(non_snake_case)] // the C field names
pub struct EfiGuid {
    /// `Data1`.
    pub Data1: u32,
    /// `Data2`.
    pub Data2: u16,
    /// `Data3`.
    pub Data3: u16,
    /// `Data4`.
    pub Data4: [u8; 8],
}

/// `EFI_ACPI_20_TABLE_GUID`.
pub const EFI_ACPI_20_TABLE_GUID: EfiGuid = EfiGuid {
    Data1: 0x8868e871,
    Data2: 0xe4f1,
    Data3: 0x11d3,
    Data4: [0xbc, 0x22, 0x00, 0x80, 0xc7, 0x3c, 0x88, 0x81],
};

/// `SMBIOS_TABLE_GUID`.
pub const SMBIOS_TABLE_GUID: EfiGuid = EfiGuid {
    Data1: 0xeb9d2d31,
    Data2: 0x2d88,
    Data3: 0x11d3,
    Data4: [0x9a, 0x16, 0x00, 0x90, 0x27, 0x3f, 0xc1, 0x4d],
};

/// `SMBIOS3_TABLE_GUID`.
pub const SMBIOS3_TABLE_GUID: EfiGuid = EfiGuid {
    Data1: 0xf2fd1544,
    Data2: 0x9794,
    Data3: 0x4a2c,
    Data4: [0x99, 0x2e, 0xe5, 0xbb, 0xcf, 0x20, 0xe3, 0x94],
};

/// `EFI_SYSTEM_RESOURCE_TABLE_GUID`.
pub const EFI_SYSTEM_RESOURCE_TABLE_GUID: EfiGuid = EfiGuid {
    Data1: 0xb122a263,
    Data2: 0x3661,
    Data3: 0x4f68,
    Data4: [0x99, 0x29, 0x78, 0xf8, 0xb0, 0xd6, 0x21, 0x80],
};

/// `EFI_GLOBAL_VARIABLE`.
pub const EFI_GLOBAL_VARIABLE: EfiGuid = EfiGuid {
    Data1: 0x8be4df61,
    Data2: 0x93ca,
    Data3: 0x11d2,
    Data4: [0xaa, 0x0d, 0x00, 0xe0, 0x98, 0x03, 0x2b, 0x8c],
};

// EFI_MEMORY_TYPE

/// `EfiReservedMemoryType`.
pub const EfiReservedMemoryType: u32 = 0;
/// `EfiLoaderCode`.
pub const EfiLoaderCode: u32 = 1;
/// `EfiLoaderData`.
pub const EfiLoaderData: u32 = 2;
/// `EfiBootServicesCode`.
pub const EfiBootServicesCode: u32 = 3;
/// `EfiBootServicesData`.
pub const EfiBootServicesData: u32 = 4;
/// `EfiRuntimeServicesCode`.
pub const EfiRuntimeServicesCode: u32 = 5;
/// `EfiRuntimeServicesData`.
pub const EfiRuntimeServicesData: u32 = 6;
/// `EfiConventionalMemory`.
pub const EfiConventionalMemory: u32 = 7;
/// `EfiUnusableMemory`.
pub const EfiUnusableMemory: u32 = 8;
/// `EfiACPIReclaimMemory`.
pub const EfiACPIReclaimMemory: u32 = 9;
/// `EfiACPIMemoryNVS`.
pub const EfiACPIMemoryNVS: u32 = 10;
/// `EfiMemoryMappedIO`.
pub const EfiMemoryMappedIO: u32 = 11;
/// `EfiMemoryMappedIOPortSpace`.
pub const EfiMemoryMappedIOPortSpace: u32 = 12;
/// `EfiPalCode`.
pub const EfiPalCode: u32 = 13;
/// `EfiPersistentMemory`.
pub const EfiPersistentMemory: u32 = 14;
/// `EfiMaxMemoryType`.
pub const EfiMaxMemoryType: u32 = 15;

/// `EFI_MEMORY_UC`.
pub const EFI_MEMORY_UC: u64 = 0x0000000000000001;
/// `EFI_MEMORY_WC`.
pub const EFI_MEMORY_WC: u64 = 0x0000000000000002;
/// `EFI_MEMORY_WT`.
pub const EFI_MEMORY_WT: u64 = 0x0000000000000004;
/// `EFI_MEMORY_WB`.
pub const EFI_MEMORY_WB: u64 = 0x0000000000000008;
/// `EFI_MEMORY_UCE`.
pub const EFI_MEMORY_UCE: u64 = 0x0000000000000010;
/// `EFI_MEMORY_WP`.
pub const EFI_MEMORY_WP: u64 = 0x0000000000001000;
/// `EFI_MEMORY_RP`.
pub const EFI_MEMORY_RP: u64 = 0x0000000000002000;
/// `EFI_MEMORY_XP`.
pub const EFI_MEMORY_XP: u64 = 0x0000000000004000;
/// `EFI_MEMORY_NV`.
pub const EFI_MEMORY_NV: u64 = 0x0000000000008000;
/// `EFI_MEMORY_MORE_RELIABLE`.
pub const EFI_MEMORY_MORE_RELIABLE: u64 = 0x0000000000010000;
/// `EFI_MEMORY_RO`.
pub const EFI_MEMORY_RO: u64 = 0x0000000000020000;
/// `EFI_MEMORY_RUNTIME`.
pub const EFI_MEMORY_RUNTIME: u64 = 0x8000000000000000;

/// `EFI_MEMORY_DESCRIPTOR_VERSION`.
pub const EFI_MEMORY_DESCRIPTOR_VERSION: u32 = 1;

/// `EFI_MEMORY_DESCRIPTOR`.
#[repr(C)]
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
#[allow(non_snake_case)] // the C field names
pub struct EfiMemoryDescriptor {
    /// `Type`: an `EFI_MEMORY_TYPE`.
    pub Type: u32,
    /// `Pad`.
    pub Pad: u32,
    /// `PhysicalStart`.
    pub PhysicalStart: EfiPhysicalAddress,
    /// `VirtualStart`: 0 unless the loader called `SetVirtualAddressMap`.
    pub VirtualStart: EfiVirtualAddress,
    /// `NumberOfPages`: 4 KiB pages.
    pub NumberOfPages: u64,
    /// `Attribute`: the `EFI_MEMORY_*` bits.
    pub Attribute: u64,
}

impl EfiMemoryDescriptor {
    /// `NextMemoryDescriptor`'s walk: the descriptor at byte `off` of the memory map `map`
    /// (whose entries are the map's descriptor size apart); `None` past the end.
    pub fn read_at(map: &[u8], off: usize) -> Option<Self> {
        let end = off.checked_add(size_of::<Self>())?;
        let bytes = map.get(off..end)?;
        // SAFETY: `bytes` holds `size_of::<Self>()` bytes; every bit pattern is a valid
        // descriptor (integers only) and the read does not assume alignment.
        Some(unsafe { ptr::read_unaligned(bytes.as_ptr().cast::<Self>()) })
    }
}

// EFI_RESET_TYPE

/// `EfiResetCold`.
pub const EfiResetCold: u32 = 0;
/// `EfiResetWarm`.
pub const EfiResetWarm: u32 = 1;
/// `EfiResetShutdown`.
pub const EfiResetShutdown: u32 = 2;
/// `EfiResetPlatformSpecific`.
pub const EfiResetPlatformSpecific: u32 = 3;

/// `EFI_TABLE_HEADER`.
#[repr(C)]
#[derive(Clone, Copy, Debug, Default)]
#[allow(non_snake_case)] // the C field names
pub struct EfiTableHeader {
    /// `Signature`.
    pub Signature: u64,
    /// `Revision`: major in the high 16 bits, minor (times ten) in the low.
    pub Revision: u32,
    /// `HeaderSize`.
    pub HeaderSize: u32,
    /// `CRC32`.
    pub CRC32: u32,
    /// `Reserved`.
    pub Reserved: u32,
}

/// `EFI_TIME`.
#[repr(C)]
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
#[allow(non_snake_case)] // the C field names
pub struct EfiTime {
    /// `Year`: 1900 to 9999.
    pub Year: u16,
    /// `Month`: 1 to 12.
    pub Month: u8,
    /// `Day`: 1 to 31.
    pub Day: u8,
    /// `Hour`.
    pub Hour: u8,
    /// `Minute`.
    pub Minute: u8,
    /// `Second`.
    pub Second: u8,
    /// `Pad1`.
    pub Pad1: u8,
    /// `Nanosecond`.
    pub Nanosecond: u32,
    /// `TimeZone`: minutes from UTC.
    pub TimeZone: i16,
    /// `Daylight`.
    pub Daylight: u8,
    /// `Pad2`.
    pub Pad2: u8,
}

/// `EFI_TIME_CAPABILITIES`: opaque here.
pub type EfiTimeCapabilities = *mut c_void;

/// `EFI_GET_TIME`.
pub type EfiGetTime =
    unsafe extern "efiapi" fn(*mut EfiTime, *mut EfiTimeCapabilities) -> EfiStatus;
/// `EFI_SET_TIME`.
pub type EfiSetTime = unsafe extern "efiapi" fn(*mut EfiTime) -> EfiStatus;
/// `EFI_SET_VIRTUAL_ADDRESS_MAP`.
pub type EfiSetVirtualAddressMap =
    unsafe extern "efiapi" fn(Uintn, Uintn, u32, *mut EfiMemoryDescriptor) -> EfiStatus;
/// `EFI_GET_VARIABLE`.
pub type EfiGetVariable = unsafe extern "efiapi" fn(
    *mut u16,
    *mut EfiGuid,
    *mut u32,
    *mut Uintn,
    *mut c_void,
) -> EfiStatus;
/// `EFI_GET_NEXT_VARIABLE_NAME`.
pub type EfiGetNextVariableName =
    unsafe extern "efiapi" fn(*mut Uintn, *mut u16, *mut EfiGuid) -> EfiStatus;
/// `EFI_SET_VARIABLE`.
pub type EfiSetVariable =
    unsafe extern "efiapi" fn(*mut u16, *mut EfiGuid, u32, Uintn, *mut c_void) -> EfiStatus;
/// `EFI_RESET_SYSTEM`.
pub type EfiResetSystem = unsafe extern "efiapi" fn(u32, EfiStatus, Uintn, *mut c_void);

/// `EFI_RUNTIME_SERVICES`.
#[repr(C)]
#[allow(non_snake_case)] // the C field names
pub struct EfiRuntimeServices {
    /// `Hdr`.
    pub Hdr: EfiTableHeader,
    /// `GetTime`.
    pub GetTime: Option<EfiGetTime>,
    /// `SetTime`.
    pub SetTime: Option<EfiSetTime>,
    /// `GetWakeupTime`.
    pub GetWakeupTime: *mut c_void,
    /// `SetWakeupTime`.
    pub SetWakeupTime: *mut c_void,

    /// `SetVirtualAddressMap`.
    pub SetVirtualAddressMap: Option<EfiSetVirtualAddressMap>,
    /// `ConvertPointer`.
    pub ConvertPointer: *mut c_void,

    /// `GetVariable`.
    pub GetVariable: Option<EfiGetVariable>,
    /// `GetNextVariableName`.
    pub GetNextVariableName: Option<EfiGetNextVariableName>,
    /// `SetVariable`.
    pub SetVariable: Option<EfiSetVariable>,

    /// `GetNextHighMonotonicCount`.
    pub GetNextHighMonotonicCount: *mut c_void,
    /// `ResetSystem`.
    pub ResetSystem: Option<EfiResetSystem>,
}

/// `EFI_CONFIGURATION_TABLE`.
#[repr(C)]
#[derive(Clone, Copy, Debug)]
#[allow(non_snake_case)] // the C field names
pub struct EfiConfigurationTable {
    /// `VendorGuid`.
    pub VendorGuid: EfiGuid,
    /// `VendorTable`.
    pub VendorTable: *mut c_void,
}

/// `EFI_SYSTEM_TABLE`.
#[repr(C)]
#[allow(non_snake_case)] // the C field names
pub struct EfiSystemTable {
    /// `Hdr`.
    pub Hdr: EfiTableHeader,
    /// `FirmwareVendor`: a NUL-terminated UCS-2 string.
    pub FirmwareVendor: *mut u16,
    /// `FirmwareRevision`.
    pub FirmwareRevision: u32,
    /// `ConsoleInHandle`.
    pub ConsoleInHandle: EfiHandle,
    /// `ConIn`.
    pub ConIn: *mut EfiSimpleTextInputProtocol,
    /// `ConsoleOutHandle`.
    pub ConsoleOutHandle: EfiHandle,
    /// `ConOut`.
    pub ConOut: *mut EfiSimpleTextOutputProtocol,
    /// `StandardErrorHandle`.
    pub StandardErrorHandle: EfiHandle,
    /// `StdErr`.
    pub StdErr: *mut EfiSimpleTextOutputProtocol,
    /// `RuntimeServices`.
    pub RuntimeServices: *mut EfiRuntimeServices,
    /// `BootServices`.
    pub BootServices: *mut EfiBootServices,
    /// `NumberOfTableEntries`.
    pub NumberOfTableEntries: Uintn,
    /// `ConfigurationTable`.
    pub ConfigurationTable: *mut EfiConfigurationTable,
}

/// `EFI_SYSTEM_RESOURCE_ENTRY`.
#[repr(C)]
#[derive(Clone, Copy, Debug)]
#[allow(non_snake_case)] // the C field names
pub struct EfiSystemResourceEntry {
    /// `FwClass`.
    pub FwClass: EfiGuid,
    /// `FwType`.
    pub FwType: u32,
    /// `FwVersion`.
    pub FwVersion: u32,
    /// `LowestSupportedFwVersion`.
    pub LowestSupportedFwVersion: u32,
    /// `CapsuleFlags`.
    pub CapsuleFlags: u32,
    /// `LastAttemptVersion`.
    pub LastAttemptVersion: u32,
    /// `LastAttemptStatus`.
    pub LastAttemptStatus: u32,
}

/// `EFI_SYSTEM_RESOURCE_TABLE`: the header; `FwResourceCount` entries follow it.
#[repr(C)]
#[derive(Clone, Copy, Debug)]
#[allow(non_snake_case)] // the C field names
pub struct EfiSystemResourceTable {
    /// `FwResourceCount`.
    pub FwResourceCount: u32,
    /// `FwResourceCountMax`.
    pub FwResourceCountMax: u32,
    /// `FwResourceVersion`.
    pub FwResourceVersion: u64,
    /// `Entries[]`: the flexible array member.
    pub Entries: [EfiSystemResourceEntry; 0],
}

/// `EFI_SUCCESS`.
pub const EFI_SUCCESS: EfiStatus = 0;

/// `EFI_INVALID_PARAMETER`.
pub const EFI_INVALID_PARAMETER: EfiStatus = EFIERR(2);
/// `EFI_UNSUPPORTED`.
pub const EFI_UNSUPPORTED: EfiStatus = EFIERR(3);
/// `EFI_BUFFER_TOO_SMALL`.
pub const EFI_BUFFER_TOO_SMALL: EfiStatus = EFIERR(5);
/// `EFI_DEVICE_ERROR`.
pub const EFI_DEVICE_ERROR: EfiStatus = EFIERR(7);
/// `EFI_WRITE_PROTECTED`.
pub const EFI_WRITE_PROTECTED: EfiStatus = EFIERR(8);
/// `EFI_OUT_OF_RESOURCES`.
pub const EFI_OUT_OF_RESOURCES: EfiStatus = EFIERR(9);
/// `EFI_NOT_FOUND`.
pub const EFI_NOT_FOUND: EfiStatus = EFIERR(14);
/// `EFI_SECURITY_VIOLATION`.
pub const EFI_SECURITY_VIOLATION: EfiStatus = EFIERR(26);

/// `efi_guidcmp(a, b) == 0`: the two GUIDs are the same bytes.
pub fn efi_guidcmp(a: &EfiGuid, b: &EfiGuid) -> bool {
    a == b
}
/* </CODE> */

/* <TESTS> */
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn layouts() {
        assert_eq!(size_of::<EfiGuid>(), 16);
        assert_eq!(size_of::<EfiMemoryDescriptor>(), 40);
        assert_eq!(size_of::<EfiTableHeader>(), 24);
        assert_eq!(size_of::<EfiTime>(), 16);
        assert_eq!(size_of::<EfiRuntimeServices>(), 24 + 11 * 8);
        assert_eq!(size_of::<EfiConfigurationTable>(), 24);
        assert_eq!(size_of::<EfiSystemTable>(), 24 + 12 * 8);
        assert_eq!(core::mem::offset_of!(EfiSystemTable, RuntimeServices), 88);
        assert_eq!(size_of::<EfiSystemResourceTable>(), 16);
    }

    #[test]
    fn status_codes() {
        assert_eq!(EFI_INVALID_PARAMETER, 0x8000_0000_0000_0002);
        assert_eq!(EFI_SECURITY_VIOLATION, 0x8000_0000_0000_001a);
    }

    #[test]
    fn walk_descriptors() {
        // Two descriptors 48 bytes apart (a descriptor size larger than the structure).
        let mut map = [0u8; 96];
        map[0] = 5; // EfiRuntimeServicesCode
        map[8..16].copy_from_slice(&0x4000_0000u64.to_le_bytes());
        map[24] = 2;
        map[32..40].copy_from_slice(&EFI_MEMORY_RUNTIME.to_le_bytes());
        map[48] = 11; // EfiMemoryMappedIO
        let a = EfiMemoryDescriptor::read_at(&map, 0).unwrap();
        assert_eq!(a.Type, EfiRuntimeServicesCode);
        assert_eq!(a.PhysicalStart, 0x4000_0000);
        assert_eq!(a.NumberOfPages, 2);
        assert_eq!(a.Attribute, EFI_MEMORY_RUNTIME);
        let b = EfiMemoryDescriptor::read_at(&map, 48).unwrap();
        assert_eq!(b.Type, EfiMemoryMappedIO);
        assert!(EfiMemoryDescriptor::read_at(&map, 96).is_none());
        assert!(EfiMemoryDescriptor::read_at(&map, 60).is_none());
    }

    #[test]
    fn guids() {
        assert!(efi_guidcmp(&SMBIOS3_TABLE_GUID, &SMBIOS3_TABLE_GUID));
        assert!(!efi_guidcmp(&SMBIOS_TABLE_GUID, &SMBIOS3_TABLE_GUID));
    }
}
/* </TESTS> */
