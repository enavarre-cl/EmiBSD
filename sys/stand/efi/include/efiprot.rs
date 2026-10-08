/* $FreeBSD: head/sys/boot/efi/include/efiprot.h 163898 2006-11-02 02:42:48Z marcel $ */
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

    efiprot.h

Abstract:

    EFI Protocols



Revision History

--*/
/* </LICENSES> */

/* <CODE> */
//! EFI protocol interfaces: device path, block I/O, disk I/O, simple file system, file,
//! load file, device I/O and unicode collation.
//!
//! Upstream: sys/stand/efi/include/efiprot.h @ 3ce1f3f79392
//!
//! Every protocol is a `#[repr(C)]` table of `extern "efiapi"` function pointers (and a few
//! data fields), found through `HandleProtocol`/`LocateProtocol` with the GUID constant of the
//! same name as the C macro.
//!
//! ## Deviations
//! - Type names are the C typedefs in CamelCase (`EFI_BLOCK_IO` -> `EfiBlockIo`,
//!   `EFI_FILE_HANDLE`/`EFI_FILE` -> `EfiFileHandle`/`EfiFile`); fields keep the C names.
//! - `struct _EFI_X *This` parameters are `*mut EfiX`; `VOID *` is `*mut c_void`.
//! - The GUID initialiser macros are `EfiGuid` constants of the same name.
//! - Enums (`EFI_IO_WIDTH`, `EFI_IO_OPERATION_TYPE`) are `u32` aliases plus constants.
//! - `EFI_FIELD_OFFSET` is `core::mem::offset_of!` in the `SIZE_OF_*` constants, and
//!   `EFI_PCI_ADDRESS` is the const fn [`efi_pci_address`].
//! - The variable-length arrays (`FileName[1]`, `VolumeLabel[1]`) keep one element.
//! - `UNICODE_BYTE_ORDER_MARK` is a `CHAR16` constant.
//! - The C typedef `EFI_FILE_RESERVIED` (sic) keeps its misspelling.

#![allow(non_snake_case, non_upper_case_globals, non_camel_case_types)]
// EFI names are the specification's (LBA, EFI_FILE_RESERVIED, IO_UINT8, ...), kept verbatim for grep-ability

use core::ffi::c_void;
use core::mem::offset_of;

use super::amd64::efibind::{INTN, UINT32, UINT64, UINTN};
use super::efidef::{
    BOOLEAN, CHAR8, CHAR16, EfiAllocateType, EfiGuid, EfiLba, EfiMemoryType, EfiPhysicalAddress,
    EfiStatus, EfiTime,
};
use super::efidevp::EfiDevicePath;

/// `DEVICE_PATH_PROTOCOL`.
pub const DEVICE_PATH_PROTOCOL: EfiGuid = EfiGuid::new(
    0x9576e91,
    0x6d3f,
    0x11d2,
    [0x8e, 0x39, 0x0, 0xa0, 0xc9, 0x69, 0x72, 0x3b],
);

/// `BLOCK_IO_PROTOCOL`.
pub const BLOCK_IO_PROTOCOL: EfiGuid = EfiGuid::new(
    0x964e5b21,
    0x6459,
    0x11d2,
    [0x8e, 0x39, 0x0, 0xa0, 0xc9, 0x69, 0x72, 0x3b],
);
/// `EFI_BLOCK_IO_INTERFACE_REVISION`.
pub const EFI_BLOCK_IO_INTERFACE_REVISION: UINT64 = 0x00010000;

/// `EFI_BLOCK_RESET`.
pub type EfiBlockReset =
    unsafe extern "efiapi" fn(This: *mut EfiBlockIo, ExtendedVerification: BOOLEAN) -> EfiStatus;

/// `EFI_BLOCK_READ`.
pub type EfiBlockRead = unsafe extern "efiapi" fn(
    This: *mut EfiBlockIo,
    MediaId: UINT32,
    LBA: EfiLba,
    BufferSize: UINTN,
    Buffer: *mut c_void,
) -> EfiStatus;

/// `EFI_BLOCK_WRITE`.
pub type EfiBlockWrite = unsafe extern "efiapi" fn(
    This: *mut EfiBlockIo,
    MediaId: UINT32,
    LBA: EfiLba,
    BufferSize: UINTN,
    Buffer: *mut c_void,
) -> EfiStatus;

/// `EFI_BLOCK_FLUSH`.
pub type EfiBlockFlush = unsafe extern "efiapi" fn(This: *mut EfiBlockIo) -> EfiStatus;

/// `EFI_BLOCK_IO_MEDIA`.
#[repr(C)]
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct EfiBlockIoMedia {
    /// Media id, changes when the media changes.
    pub MediaId: UINT32,
    /// The media is removable.
    pub RemovableMedia: BOOLEAN,
    /// Media is present.
    pub MediaPresent: BOOLEAN,
    /// The device is a logical partition.
    pub LogicalPartition: BOOLEAN,
    /// The media is read-only.
    pub ReadOnly: BOOLEAN,
    /// The device has a write cache.
    pub WriteCaching: BOOLEAN,
    /// Block size in bytes.
    pub BlockSize: UINT32,
    /// Required buffer alignment.
    pub IoAlign: UINT32,
    /// Last addressable block.
    pub LastBlock: EfiLba,
}

/// `EFI_BLOCK_IO`: the block I/O protocol.
#[repr(C)]
#[derive(Clone, Copy)]
pub struct EfiBlockIo {
    /// `EFI_BLOCK_IO_INTERFACE_REVISION` or later.
    pub Revision: UINT64,
    /// The media description.
    pub Media: *mut EfiBlockIoMedia,
    /// `Reset`.
    pub Reset: EfiBlockReset,
    /// `ReadBlocks`.
    pub ReadBlocks: EfiBlockRead,
    /// `WriteBlocks`.
    pub WriteBlocks: EfiBlockWrite,
    /// `FlushBlocks`.
    pub FlushBlocks: EfiBlockFlush,
}

/// `DISK_IO_PROTOCOL`.
pub const DISK_IO_PROTOCOL: EfiGuid = EfiGuid::new(
    0xce345171,
    0xba0b,
    0x11d2,
    [0x8e, 0x4f, 0x0, 0xa0, 0xc9, 0x69, 0x72, 0x3b],
);
/// `EFI_DISK_IO_INTERFACE_REVISION`.
pub const EFI_DISK_IO_INTERFACE_REVISION: UINT64 = 0x00010000;

/// `EFI_DISK_READ`.
pub type EfiDiskRead = unsafe extern "efiapi" fn(
    This: *mut EfiDiskIo,
    MediaId: UINT32,
    Offset: UINT64,
    BufferSize: UINTN,
    Buffer: *mut c_void,
) -> EfiStatus;

/// `EFI_DISK_WRITE`.
pub type EfiDiskWrite = unsafe extern "efiapi" fn(
    This: *mut EfiDiskIo,
    MediaId: UINT32,
    Offset: UINT64,
    BufferSize: UINTN,
    Buffer: *mut c_void,
) -> EfiStatus;

/// `EFI_DISK_IO`: the disk I/O protocol.
#[repr(C)]
#[derive(Clone, Copy)]
pub struct EfiDiskIo {
    /// `EFI_DISK_IO_INTERFACE_REVISION`.
    pub Revision: UINT64,
    /// `ReadDisk`.
    pub ReadDisk: EfiDiskRead,
    /// `WriteDisk`.
    pub WriteDisk: EfiDiskWrite,
}

/// `SIMPLE_FILE_SYSTEM_PROTOCOL`.
pub const SIMPLE_FILE_SYSTEM_PROTOCOL: EfiGuid = EfiGuid::new(
    0x964e5b22,
    0x6459,
    0x11d2,
    [0x8e, 0x39, 0x0, 0xa0, 0xc9, 0x69, 0x72, 0x3b],
);

/// `EFI_VOLUME_OPEN`.
pub type EfiVolumeOpen =
    unsafe extern "efiapi" fn(This: *mut EfiFileIoInterface, Root: *mut *mut EfiFile) -> EfiStatus;

/// `EFI_FILE_IO_INTERFACE_REVISION`.
pub const EFI_FILE_IO_INTERFACE_REVISION: UINT64 = 0x00010000;

/// `EFI_FILE_IO_INTERFACE`: the simple file system protocol.
#[repr(C)]
#[derive(Clone, Copy)]
pub struct EfiFileIoInterface {
    /// `EFI_FILE_IO_INTERFACE_REVISION`.
    pub Revision: UINT64,
    /// `OpenVolume`.
    pub OpenVolume: EfiVolumeOpen,
}

/// `EFI_FILE_OPEN`.
pub type EfiFileOpen = unsafe extern "efiapi" fn(
    File: *mut EfiFile,
    NewHandle: *mut *mut EfiFile,
    FileName: *mut CHAR16,
    OpenMode: UINT64,
    Attributes: UINT64,
) -> EfiStatus;

/// Open mode: read.
pub const EFI_FILE_MODE_READ: UINT64 = 0x0000000000000001;
/// Open mode: write.
pub const EFI_FILE_MODE_WRITE: UINT64 = 0x0000000000000002;
/// Open mode: create.
pub const EFI_FILE_MODE_CREATE: UINT64 = 0x8000000000000000;

/// File attribute: read only.
pub const EFI_FILE_READ_ONLY: UINT64 = 0x0000000000000001;
/// File attribute: hidden.
pub const EFI_FILE_HIDDEN: UINT64 = 0x0000000000000002;
/// File attribute: system.
pub const EFI_FILE_SYSTEM: UINT64 = 0x0000000000000004;
/// File attribute: reserved (the C spelling).
pub const EFI_FILE_RESERVIED: UINT64 = 0x0000000000000008;
/// File attribute: directory.
pub const EFI_FILE_DIRECTORY: UINT64 = 0x0000000000000010;
/// File attribute: archive.
pub const EFI_FILE_ARCHIVE: UINT64 = 0x0000000000000020;
/// File attribute: all the valid bits.
pub const EFI_FILE_VALID_ATTR: UINT64 = 0x0000000000000037;

/// `EFI_FILE_CLOSE`.
pub type EfiFileClose = unsafe extern "efiapi" fn(File: *mut EfiFile) -> EfiStatus;

/// `EFI_FILE_DELETE`.
pub type EfiFileDelete = unsafe extern "efiapi" fn(File: *mut EfiFile) -> EfiStatus;

/// `EFI_FILE_READ`.
pub type EfiFileRead = unsafe extern "efiapi" fn(
    File: *mut EfiFile,
    BufferSize: *mut UINTN,
    Buffer: *mut c_void,
) -> EfiStatus;

/// `EFI_FILE_WRITE`.
pub type EfiFileWrite = unsafe extern "efiapi" fn(
    File: *mut EfiFile,
    BufferSize: *mut UINTN,
    Buffer: *mut c_void,
) -> EfiStatus;

/// `EFI_FILE_SET_POSITION`.
pub type EfiFileSetPosition =
    unsafe extern "efiapi" fn(File: *mut EfiFile, Position: UINT64) -> EfiStatus;

/// `EFI_FILE_GET_POSITION`.
pub type EfiFileGetPosition =
    unsafe extern "efiapi" fn(File: *mut EfiFile, Position: *mut UINT64) -> EfiStatus;

/// `EFI_FILE_GET_INFO`.
pub type EfiFileGetInfo = unsafe extern "efiapi" fn(
    File: *mut EfiFile,
    InformationType: *mut EfiGuid,
    BufferSize: *mut UINTN,
    Buffer: *mut c_void,
) -> EfiStatus;

/// `EFI_FILE_SET_INFO`.
pub type EfiFileSetInfo = unsafe extern "efiapi" fn(
    File: *mut EfiFile,
    InformationType: *mut EfiGuid,
    BufferSize: UINTN,
    Buffer: *mut c_void,
) -> EfiStatus;

/// `EFI_FILE_FLUSH`.
pub type EfiFileFlush = unsafe extern "efiapi" fn(File: *mut EfiFile) -> EfiStatus;

/// `EFI_FILE_HANDLE_REVISION`.
pub const EFI_FILE_HANDLE_REVISION: UINT64 = 0x00010000;

/// `EFI_FILE` (C tag `_EFI_FILE_HANDLE`): an open file or directory.
#[repr(C)]
#[derive(Clone, Copy)]
pub struct EfiFile {
    /// `EFI_FILE_HANDLE_REVISION`.
    pub Revision: UINT64,
    /// `Open`.
    pub Open: EfiFileOpen,
    /// `Close`.
    pub Close: EfiFileClose,
    /// `Delete`.
    pub Delete: EfiFileDelete,
    /// `Read`.
    pub Read: EfiFileRead,
    /// `Write`.
    pub Write: EfiFileWrite,
    /// `GetPosition`.
    pub GetPosition: EfiFileGetPosition,
    /// `SetPosition`.
    pub SetPosition: EfiFileSetPosition,
    /// `GetInfo`.
    pub GetInfo: EfiFileGetInfo,
    /// `SetInfo`.
    pub SetInfo: EfiFileSetInfo,
    /// `Flush`.
    pub Flush: EfiFileFlush,
}

/// `EFI_FILE_HANDLE`.
pub type EfiFileHandle = *mut EfiFile;

/// `EFI_FILE_INFO_ID`.
pub const EFI_FILE_INFO_ID: EfiGuid = EfiGuid::new(
    0x9576e92,
    0x6d3f,
    0x11d2,
    [0x8e, 0x39, 0x0, 0xa0, 0xc9, 0x69, 0x72, 0x3b],
);

/// `EFI_FILE_INFO`: the `FileName` array is variable length, so the real size of an
/// instance is `SIZE_OF_EFI_FILE_INFO` plus the name, which is what `Size` holds.
#[repr(C)]
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct EfiFileInfo {
    /// Size of this structure including the name.
    pub Size: UINT64,
    /// Size of the file in bytes.
    pub FileSize: UINT64,
    /// Space the file occupies on the volume.
    pub PhysicalSize: UINT64,
    /// Creation time.
    pub CreateTime: EfiTime,
    /// Last access time.
    pub LastAccessTime: EfiTime,
    /// Modification time.
    pub ModificationTime: EfiTime,
    /// `EFI_FILE_*` attribute bits.
    pub Attribute: UINT64,
    /// NUL-terminated UCS-2 name (variable length).
    pub FileName: [CHAR16; 1],
}

/// `SIZE_OF_EFI_FILE_INFO`: the size of [`EfiFileInfo`] without its name.
pub const SIZE_OF_EFI_FILE_INFO: UINTN = offset_of!(EfiFileInfo, FileName);

/// `EFI_FILE_SYSTEM_INFO_ID`.
pub const EFI_FILE_SYSTEM_INFO_ID: EfiGuid = EfiGuid::new(
    0x9576e93,
    0x6d3f,
    0x11d2,
    [0x8e, 0x39, 0x0, 0xa0, 0xc9, 0x69, 0x72, 0x3b],
);

/// `EFI_FILE_SYSTEM_INFO`.
#[repr(C)]
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct EfiFileSystemInfo {
    /// Size of this structure including the label.
    pub Size: UINT64,
    /// The volume is read-only.
    pub ReadOnly: BOOLEAN,
    /// Volume size in bytes.
    pub VolumeSize: UINT64,
    /// Free space in bytes.
    pub FreeSpace: UINT64,
    /// Block size in bytes.
    pub BlockSize: UINT32,
    /// NUL-terminated UCS-2 label (variable length).
    pub VolumeLabel: [CHAR16; 1],
}

/// `SIZE_OF_EFI_FILE_SYSTEM_INFO`: the size of [`EfiFileSystemInfo`] without its label.
pub const SIZE_OF_EFI_FILE_SYSTEM_INFO: UINTN = offset_of!(EfiFileSystemInfo, VolumeLabel);

/// `EFI_FILE_SYSTEM_VOLUME_LABEL_INFO_ID`.
pub const EFI_FILE_SYSTEM_VOLUME_LABEL_INFO_ID: EfiGuid = EfiGuid::new(
    0xDB47D7D3,
    0xFE81,
    0x11d3,
    [0x9A, 0x35, 0x00, 0x90, 0x27, 0x3F, 0xC1, 0x4D],
);

/// `EFI_FILE_SYSTEM_VOLUME_LABEL_INFO`.
#[repr(C)]
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct EfiFileSystemVolumeLabelInfo {
    /// NUL-terminated UCS-2 label (variable length).
    pub VolumeLabel: [CHAR16; 1],
}

/// `SIZE_OF_EFI_FILE_SYSTEM_VOLUME_LABEL_INFO`.
pub const SIZE_OF_EFI_FILE_SYSTEM_VOLUME_LABEL_INFO: UINTN =
    offset_of!(EfiFileSystemVolumeLabelInfo, VolumeLabel);

/// `LOAD_FILE_PROTOCOL`.
pub const LOAD_FILE_PROTOCOL: EfiGuid = EfiGuid::new(
    0x56EC3091,
    0x954C,
    0x11d2,
    [0x8E, 0x3F, 0x00, 0xA0, 0xC9, 0x69, 0x72, 0x3B],
);

/// `EFI_LOAD_FILE`.
pub type EfiLoadFile = unsafe extern "efiapi" fn(
    This: *mut EfiLoadFileInterface,
    FilePath: *mut EfiDevicePath,
    BootPolicy: BOOLEAN,
    BufferSize: *mut UINTN,
    Buffer: *mut c_void,
) -> EfiStatus;

/// `EFI_LOAD_FILE_INTERFACE`: the load file protocol.
#[repr(C)]
#[derive(Clone, Copy)]
pub struct EfiLoadFileInterface {
    /// `LoadFile`.
    pub LoadFile: EfiLoadFile,
}

/// `DEVICE_IO_PROTOCOL`.
pub const DEVICE_IO_PROTOCOL: EfiGuid = EfiGuid::new(
    0xaf6ac311,
    0x84c3,
    0x11d2,
    [0x8e, 0x3c, 0x00, 0xa0, 0xc9, 0x69, 0x72, 0x3b],
);

/// `EFI_IO_WIDTH`.
pub type EfiIoWidth = u32;
/// `IO_UINT8`.
pub const IO_UINT8: EfiIoWidth = 0;
/// `IO_UINT16`.
pub const IO_UINT16: EfiIoWidth = 1;
/// `IO_UINT32`.
pub const IO_UINT32: EfiIoWidth = 2;
/// `IO_UINT64`.
pub const IO_UINT64: EfiIoWidth = 3;
/// `MMIO_COPY_UINT8`.
pub const MMIO_COPY_UINT8: EfiIoWidth = 4;
/// `MMIO_COPY_UINT16`.
pub const MMIO_COPY_UINT16: EfiIoWidth = 5;
/// `MMIO_COPY_UINT32`.
pub const MMIO_COPY_UINT32: EfiIoWidth = 6;
/// `MMIO_COPY_UINT64`.
pub const MMIO_COPY_UINT64: EfiIoWidth = 7;

/// `EFI_PCI_ADDRESS(bus, dev, func, reg)`.
pub const fn efi_pci_address(bus: UINTN, dev: UINTN, func: UINTN, reg: UINTN) -> UINT64 {
    ((bus << 24) + (dev << 16) + (func << 8) + reg) as UINT64
}

/// `EFI_DEVICE_IO`.
pub type EfiDeviceIo = unsafe extern "efiapi" fn(
    This: *mut EfiDeviceIoInterface,
    Width: EfiIoWidth,
    Address: UINT64,
    Count: UINTN,
    Buffer: *mut c_void,
) -> EfiStatus;

/// `EFI_IO_ACCESS`.
#[repr(C)]
#[derive(Clone, Copy)]
pub struct EfiIoAccess {
    /// `Read`.
    pub Read: EfiDeviceIo,
    /// `Write`.
    pub Write: EfiDeviceIo,
}

/// `EFI_PCI_DEVICE_PATH` (the function pointer type, not the node struct).
pub type EfiPciDevicePath = unsafe extern "efiapi" fn(
    This: *mut EfiDeviceIoInterface,
    Address: UINT64,
    PciDevicePath: *mut *mut EfiDevicePath,
) -> EfiStatus;

/// `EFI_IO_OPERATION_TYPE`.
pub type EfiIoOperationType = u32;
/// `EfiBusMasterRead`.
pub const EfiBusMasterRead: EfiIoOperationType = 0;
/// `EfiBusMasterWrite`.
pub const EfiBusMasterWrite: EfiIoOperationType = 1;
/// `EfiBusMasterCommonBuffer`.
pub const EfiBusMasterCommonBuffer: EfiIoOperationType = 2;

/// `EFI_IO_MAP`.
pub type EfiIoMap = unsafe extern "efiapi" fn(
    This: *mut EfiDeviceIoInterface,
    Operation: EfiIoOperationType,
    HostAddress: *mut EfiPhysicalAddress,
    NumberOfBytes: *mut UINTN,
    DeviceAddress: *mut EfiPhysicalAddress,
    Mapping: *mut *mut c_void,
) -> EfiStatus;

/// `EFI_IO_UNMAP`.
pub type EfiIoUnmap =
    unsafe extern "efiapi" fn(This: *mut EfiDeviceIoInterface, Mapping: *mut c_void) -> EfiStatus;

/// `EFI_IO_ALLOCATE_BUFFER`.
pub type EfiIoAllocateBuffer = unsafe extern "efiapi" fn(
    This: *mut EfiDeviceIoInterface,
    Type: EfiAllocateType,
    MemoryType: EfiMemoryType,
    Pages: UINTN,
    HostAddress: *mut EfiPhysicalAddress,
) -> EfiStatus;

/// `EFI_IO_FLUSH`.
pub type EfiIoFlush = unsafe extern "efiapi" fn(This: *mut EfiDeviceIoInterface) -> EfiStatus;

/// `EFI_IO_FREE_BUFFER`.
pub type EfiIoFreeBuffer = unsafe extern "efiapi" fn(
    This: *mut EfiDeviceIoInterface,
    Pages: UINTN,
    HostAddress: EfiPhysicalAddress,
) -> EfiStatus;

/// `EFI_DEVICE_IO_INTERFACE`: the device I/O protocol.
#[repr(C)]
#[derive(Clone, Copy)]
pub struct EfiDeviceIoInterface {
    /// `Mem`.
    pub Mem: EfiIoAccess,
    /// `Io`.
    pub Io: EfiIoAccess,
    /// `Pci`.
    pub Pci: EfiIoAccess,
    /// `Map`.
    pub Map: EfiIoMap,
    /// `PciDevicePath`.
    pub PciDevicePath: EfiPciDevicePath,
    /// `Unmap`.
    pub Unmap: EfiIoUnmap,
    /// `AllocateBuffer`.
    pub AllocateBuffer: EfiIoAllocateBuffer,
    /// `Flush`.
    pub Flush: EfiIoFlush,
    /// `FreeBuffer`.
    pub FreeBuffer: EfiIoFreeBuffer,
}

/// `UNICODE_COLLATION_PROTOCOL`.
pub const UNICODE_COLLATION_PROTOCOL: EfiGuid = EfiGuid::new(
    0x1d85cd7f,
    0xf43d,
    0x11d2,
    [0x9a, 0xc, 0x0, 0x90, 0x27, 0x3f, 0xc1, 0x4d],
);

/// `UNICODE_BYTE_ORDER_MARK`.
pub const UNICODE_BYTE_ORDER_MARK: CHAR16 = 0xfeff;

/// `EFI_UNICODE_COLLATION_STRICOLL`.
pub type EfiUnicodeCollationStricoll = unsafe extern "efiapi" fn(
    This: *mut EfiUnicodeCollationInterface,
    s1: *mut CHAR16,
    s2: *mut CHAR16,
) -> INTN;

/// `EFI_UNICODE_COLLATION_METAIMATCH`.
pub type EfiUnicodeCollationMetaimatch = unsafe extern "efiapi" fn(
    This: *mut EfiUnicodeCollationInterface,
    String: *mut CHAR16,
    Pattern: *mut CHAR16,
) -> BOOLEAN;

/// `EFI_UNICODE_COLLATION_STRLWR`.
pub type EfiUnicodeCollationStrlwr =
    unsafe extern "efiapi" fn(This: *mut EfiUnicodeCollationInterface, Str: *mut CHAR16);

/// `EFI_UNICODE_COLLATION_STRUPR`.
pub type EfiUnicodeCollationStrupr =
    unsafe extern "efiapi" fn(This: *mut EfiUnicodeCollationInterface, Str: *mut CHAR16);

/// `EFI_UNICODE_COLLATION_FATTOSTR`.
pub type EfiUnicodeCollationFattostr = unsafe extern "efiapi" fn(
    This: *mut EfiUnicodeCollationInterface,
    FatSize: UINTN,
    Fat: *mut CHAR8,
    String: *mut CHAR16,
);

/// `EFI_UNICODE_COLLATION_STRTOFAT`.
pub type EfiUnicodeCollationStrtofat = unsafe extern "efiapi" fn(
    This: *mut EfiUnicodeCollationInterface,
    String: *mut CHAR16,
    FatSize: UINTN,
    Fat: *mut CHAR8,
) -> BOOLEAN;

/// `EFI_UNICODE_COLLATION_INTERFACE`: the unicode collation protocol.
#[repr(C)]
#[derive(Clone, Copy)]
pub struct EfiUnicodeCollationInterface {
    /// `StriColl`.
    pub StriColl: EfiUnicodeCollationStricoll,
    /// `MetaiMatch`.
    pub MetaiMatch: EfiUnicodeCollationMetaimatch,
    /// `StrLwr`.
    pub StrLwr: EfiUnicodeCollationStrlwr,
    /// `StrUpr`.
    pub StrUpr: EfiUnicodeCollationStrupr,
    /// `FatToStr`.
    pub FatToStr: EfiUnicodeCollationFattostr,
    /// `StrToFat`.
    pub StrToFat: EfiUnicodeCollationStrtofat,
    /// `SupportedLanguages`.
    pub SupportedLanguages: *mut CHAR8,
}

const _: () = assert!(core::mem::size_of::<EfiBlockIoMedia>() == 32);
const _: () = assert!(core::mem::size_of::<EfiBlockIo>() == 48);
const _: () = assert!(core::mem::size_of::<EfiFile>() == 88);
const _: () = assert!(core::mem::size_of::<EfiFileInfo>() == 88);
const _: () = assert!(SIZE_OF_EFI_FILE_INFO == 80);
const _: () = assert!(SIZE_OF_EFI_FILE_SYSTEM_INFO == 36);
const _: () = assert!(core::mem::size_of::<EfiDeviceIoInterface>() == 96);
/* </CODE> */

/* <TESTS> */
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn pci_address_packs_bus_dev_func_reg() {
        assert_eq!(efi_pci_address(1, 2, 3, 4), 0x0102_0304);
    }

    #[test]
    fn file_info_header_size() {
        // 3 * 8 + 3 * 16 + 8 = 80 bytes before the name.
        assert_eq!(SIZE_OF_EFI_FILE_INFO, 80);
        assert_eq!(SIZE_OF_EFI_FILE_SYSTEM_VOLUME_LABEL_INFO, 0);
    }
}
/* </TESTS> */
