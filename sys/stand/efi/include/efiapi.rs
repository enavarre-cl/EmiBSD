/* $FreeBSD: head/sys/boot/efi/include/efiapi.h 278234 2015-02-05 07:19:30Z rpaulo $ */
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

    efiapi.h

Abstract:

    Global EFI runtime & boot service interfaces




Revision History

--*/
/* </LICENSES> */

/* <CODE> */
//! EFI API: the boot services, runtime services and system tables, and the loaded image.
//!
//! Upstream: sys/stand/efi/include/efiapi.h @ 3ce1f3f79392
//!
//! The firmware calls an image's entry point with its handle and an [`EfiSystemTable`]; that
//! table reaches [`EfiBootServices`], [`EfiRuntimeServices`], the console protocols and the
//! configuration tables (ACPI, SMBIOS, FDT, ...). Every service is an `extern "efiapi"`
//! function pointer whose slot keeps its place in the table.
//!
//! ## Deviations
//! - Type names are the C typedefs in CamelCase (`EFI_SYSTEM_TABLE` -> `EfiSystemTable`,
//!   `EFI_BOOT_SERVICES` -> `EfiBootServices`, `EFI_LOADED_IMAGE` -> `EfiLoadedImage`,
//!   `EFI_TABLE_HEADER` -> `EfiTableHeader`, ...); function pointer typedefs are CamelCase
//!   too (`EFI_ALLOCATE_PAGES` -> `EfiAllocatePages`). Fields and parameters are verbatim.
//! - The enums (`EFI_TIMER_DELAY`, `EFI_RESET_TYPE`, `EFI_INTERFACE_TYPE`,
//!   `EFI_LOCATE_SEARCH_TYPE`) are `u32` aliases plus one constant per enumerator.
//! - `EFI_INSTALL_MULTIPLE_PROTOCOL_INTERFACES` and `EFI_UNINSTALL_MULTIPLE_PROTOCOL_INTERFACES`
//!   are C-variadic, which stable Rust does not support for the `efiapi` ABI: their table
//!   slots are `*const c_void` (the slot keeps its place and size; calling them is not
//!   possible from Rust, and the efiboot loaders do not).
//! - `EFI_CREATE_EVENT`'s `NotifyFunction` is `Option<EfiEventNotify>`: the C passes NULL for
//!   "no notification", and `Option` of a function pointer has the same ABI as the C pointer.
//! - `EFI_LOADED_IMAGE.Unload` is `Option<EfiImageUnload>` (NULL when the image cannot unload);
//!   same ABI as the C pointer.
//! - `EFI_RESERVED_SERVICE` has an empty C parameter list; here it takes no parameters.
//! - `NextMemoryDescriptor(Ptr, Size)` is [`next_memory_descriptor`], a safe function using
//!   wrapping pointer arithmetic: it only computes an address.
//! - `EFI_RUNTIME_SERVICES_REVISION`, `EFI_BOOT_SERVICES_REVISION` and
//!   `EFI_SYSTEM_TABLE_REVISION` are computed from the specification revision constants.
//! - The commented-out `Entries[]` of `EFI_SYSTEM_RESOURCE_TABLE` stays a comment in the C and
//!   is a note here: the entries follow the table in memory.
//! - The GUID initialiser macros are `EfiGuid` constants of the same name.

#![allow(non_snake_case, non_upper_case_globals, non_camel_case_types)]
// EFI names are the specification's (RaiseTPL, EFI_GLOBAL_VARIABLE, EfiResetCold, ...), kept verbatim for grep-ability

use core::ffi::c_void;

use super::amd64::efibind::{UINT8, UINT16, UINT32, UINT64, UINTN};
use super::eficon::{SimpleInputInterface, SimpleTextOutputInterface};
use super::efidef::{
    BOOLEAN, CHAR16, EfiAllocateType, EfiEvent, EfiGuid, EfiHandle, EfiMemoryDescriptor,
    EfiMemoryType, EfiPhysicalAddress, EfiStatus, EfiTime, EfiTpl,
};
use super::efidevp::EfiDevicePath;

/// `EFI_SPECIFICATION_MAJOR_REVISION`.
pub const EFI_SPECIFICATION_MAJOR_REVISION: UINT32 = 1;
/// `EFI_SPECIFICATION_MINOR_REVISION`.
pub const EFI_SPECIFICATION_MINOR_REVISION: UINT32 = 10;

/// `EFI_OPTIONAL_PTR`.
pub const EFI_OPTIONAL_PTR: UINTN = 0x00000001;
/// `EFI_INTERNAL_FNC`: pointer to an internal runtime function.
pub const EFI_INTERNAL_FNC: UINTN = 0x00000002;
/// `EFI_INTERNAL_PTR`: pointer to internal runtime data.
pub const EFI_INTERNAL_PTR: UINTN = 0x00000004;

/// Event type: timer.
pub const EVT_TIMER: UINT32 = 0x80000000;
/// Event type: runtime.
pub const EVT_RUNTIME: UINT32 = 0x40000000;
/// Event type: runtime context.
pub const EVT_RUNTIME_CONTEXT: UINT32 = 0x20000000;
/// Event type: notify wait.
pub const EVT_NOTIFY_WAIT: UINT32 = 0x00000100;
/// Event type: notify signal.
pub const EVT_NOTIFY_SIGNAL: UINT32 = 0x00000200;
/// Event type: signalled at `ExitBootServices`.
pub const EVT_SIGNAL_EXIT_BOOT_SERVICES: UINT32 = 0x00000201;
/// Event type: signalled at a virtual address change.
pub const EVT_SIGNAL_VIRTUAL_ADDRESS_CHANGE: UINT32 = 0x60000202;
/// `EVT_EFI_SIGNAL_MASK`.
pub const EVT_EFI_SIGNAL_MASK: UINT32 = 0x000000FF;
/// `EVT_EFI_SIGNAL_MAX`.
pub const EVT_EFI_SIGNAL_MAX: UINT32 = 2;

/// `EFI_TIMER_DELAY`.
pub type EfiTimerDelay = u32;
/// `TimerCancel`.
pub const TimerCancel: EfiTimerDelay = 0;
/// `TimerPeriodic`.
pub const TimerPeriodic: EfiTimerDelay = 1;
/// `TimerRelative`.
pub const TimerRelative: EfiTimerDelay = 2;
/// `TimerTypeMax`.
pub const TimerTypeMax: EfiTimerDelay = 3;

/// `TPL_APPLICATION`.
pub const TPL_APPLICATION: EfiTpl = 4;
/// `TPL_CALLBACK`.
pub const TPL_CALLBACK: EfiTpl = 8;
/// `TPL_NOTIFY`.
pub const TPL_NOTIFY: EfiTpl = 16;
/// `TPL_HIGH_LEVEL`.
pub const TPL_HIGH_LEVEL: EfiTpl = 31;

/// `EFI_GLOBAL_VARIABLE`: the vendor GUID of the EFI platform variables.
pub const EFI_GLOBAL_VARIABLE: EfiGuid = EfiGuid::new(
    0x8BE4DF61,
    0x93CA,
    0x11d2,
    [0xAA, 0x0D, 0x00, 0xE0, 0x98, 0x03, 0x2B, 0x8C],
);

/// Variable attribute: non-volatile.
pub const EFI_VARIABLE_NON_VOLATILE: UINT32 = 0x00000001;
/// Variable attribute: boot service access.
pub const EFI_VARIABLE_BOOTSERVICE_ACCESS: UINT32 = 0x00000002;
/// Variable attribute: runtime access.
pub const EFI_VARIABLE_RUNTIME_ACCESS: UINT32 = 0x00000004;

/// `EFI_MAXIMUM_VARIABLE_SIZE`.
pub const EFI_MAXIMUM_VARIABLE_SIZE: UINTN = 1024;

/// PE32+ subsystem: EFI application.
pub const IMAGE_SUBSYSTEM_EFI_APPLICATION: UINT16 = 10;
/// PE32+ subsystem: EFI boot service driver.
pub const IMAGE_SUBSYSTEM_EFI_BOOT_SERVICE_DRIVER: UINT16 = 11;
/// PE32+ subsystem: EFI runtime driver.
pub const IMAGE_SUBSYSTEM_EFI_RUNTIME_DRIVER: UINT16 = 12;

/// PE32+ machine type: IA32.
pub const EFI_IMAGE_MACHINE_IA32: UINT16 = 0x014c;
/// PE32+ machine type: EFI byte code.
pub const EFI_IMAGE_MACHINE_EBC: UINT16 = 0x0EBC;

/// `LOADED_IMAGE_PROTOCOL`.
pub const LOADED_IMAGE_PROTOCOL: EfiGuid = EfiGuid::new(
    0x5B1B31A1,
    0x9562,
    0x11d2,
    [0x8E, 0x3F, 0x00, 0xA0, 0xC9, 0x69, 0x72, 0x3B],
);

/// `EFI_LOADED_IMAGE_INFORMATION_REVISION`.
pub const EFI_LOADED_IMAGE_INFORMATION_REVISION: UINT32 = 0x1000;

/// `EFI_RESET_TYPE`.
pub type EfiResetType = u32;
/// `EfiResetCold`.
pub const EfiResetCold: EfiResetType = 0;
/// `EfiResetWarm`.
pub const EfiResetWarm: EfiResetType = 1;
/// `EfiResetShutdown`.
pub const EfiResetShutdown: EfiResetType = 2;

/// `EFI_INTERFACE_TYPE`.
pub type EfiInterfaceType = u32;
/// `EFI_NATIVE_INTERFACE`.
pub const EFI_NATIVE_INTERFACE: EfiInterfaceType = 0;

/// `EFI_LOCATE_SEARCH_TYPE`.
pub type EfiLocateSearchType = u32;
/// `AllHandles`.
pub const AllHandles: EfiLocateSearchType = 0;
/// `ByRegisterNotify`.
pub const ByRegisterNotify: EfiLocateSearchType = 1;
/// `ByProtocol`.
pub const ByProtocol: EfiLocateSearchType = 2;

/// `EFI_OPEN_PROTOCOL_BY_HANDLE_PROTOCOL`.
pub const EFI_OPEN_PROTOCOL_BY_HANDLE_PROTOCOL: UINT32 = 0x00000001;
/// `EFI_OPEN_PROTOCOL_GET_PROTOCOL`.
pub const EFI_OPEN_PROTOCOL_GET_PROTOCOL: UINT32 = 0x00000002;
/// `EFI_OPEN_PROTOCOL_TEST_PROTOCOL`.
pub const EFI_OPEN_PROTOCOL_TEST_PROTOCOL: UINT32 = 0x00000004;
/// `EFI_OPEN_PROTOCOL_BY_CHILD_CONTROLLER`.
pub const EFI_OPEN_PROTOCOL_BY_CHILD_CONTROLLER: UINT32 = 0x00000008;
/// `EFI_OPEN_PROTOCOL_BY_DRIVER`.
pub const EFI_OPEN_PROTOCOL_BY_DRIVER: UINT32 = 0x00000010;
/// `EFI_OPEN_PROTOCOL_EXCLUSIVE`.
pub const EFI_OPEN_PROTOCOL_EXCLUSIVE: UINT32 = 0x00000020;

/// `EFI_RUNTIME_SERVICES_SIGNATURE`.
pub const EFI_RUNTIME_SERVICES_SIGNATURE: UINT64 = 0x56524553544e5552;
/// `EFI_RUNTIME_SERVICES_REVISION`.
pub const EFI_RUNTIME_SERVICES_REVISION: UINT32 =
    (EFI_SPECIFICATION_MAJOR_REVISION << 16) | EFI_SPECIFICATION_MINOR_REVISION;

/// `EFI_BOOT_SERVICES_SIGNATURE`.
pub const EFI_BOOT_SERVICES_SIGNATURE: UINT64 = 0x56524553544f4f42;
/// `EFI_BOOT_SERVICES_REVISION`.
pub const EFI_BOOT_SERVICES_REVISION: UINT32 =
    (EFI_SPECIFICATION_MAJOR_REVISION << 16) | EFI_SPECIFICATION_MINOR_REVISION;

/// `MPS_TABLE_GUID`.
pub const MPS_TABLE_GUID: EfiGuid = EfiGuid::new(
    0xeb9d2d2f,
    0x2d88,
    0x11d3,
    [0x9a, 0x16, 0x0, 0x90, 0x27, 0x3f, 0xc1, 0x4d],
);
/// `ACPI_TABLE_GUID`.
pub const ACPI_TABLE_GUID: EfiGuid = EfiGuid::new(
    0xeb9d2d30,
    0x2d88,
    0x11d3,
    [0x9a, 0x16, 0x0, 0x90, 0x27, 0x3f, 0xc1, 0x4d],
);
/// `ACPI_20_TABLE_GUID`.
pub const ACPI_20_TABLE_GUID: EfiGuid = EfiGuid::new(
    0x8868e871,
    0xe4f1,
    0x11d3,
    [0xbc, 0x22, 0x0, 0x80, 0xc7, 0x3c, 0x88, 0x81],
);
/// `SMBIOS_TABLE_GUID`.
pub const SMBIOS_TABLE_GUID: EfiGuid = EfiGuid::new(
    0xeb9d2d31,
    0x2d88,
    0x11d3,
    [0x9a, 0x16, 0x0, 0x90, 0x27, 0x3f, 0xc1, 0x4d],
);
/// `SMBIOS3_TABLE_GUID`.
pub const SMBIOS3_TABLE_GUID: EfiGuid = EfiGuid::new(
    0xf2fd1544,
    0x9794,
    0x4a2c,
    [0x99, 0x2e, 0xe5, 0xbb, 0xcf, 0x20, 0xe3, 0x94],
);
/// `SAL_SYSTEM_TABLE_GUID`.
pub const SAL_SYSTEM_TABLE_GUID: EfiGuid = EfiGuid::new(
    0xeb9d2d32,
    0x2d88,
    0x11d3,
    [0x9a, 0x16, 0x0, 0x90, 0x27, 0x3f, 0xc1, 0x4d],
);
/// `FDT_TABLE_GUID`.
pub const FDT_TABLE_GUID: EfiGuid = EfiGuid::new(
    0xb1b621d5,
    0xf19c,
    0x41a5,
    [0x83, 0x0b, 0xd9, 0x15, 0x2c, 0x69, 0xaa, 0xe0],
);
/// `DXE_SERVICES_TABLE_GUID`.
pub const DXE_SERVICES_TABLE_GUID: EfiGuid = EfiGuid::new(
    0x5ad34ba,
    0x6f02,
    0x4214,
    [0x95, 0x2e, 0x4d, 0xa0, 0x39, 0x8e, 0x2b, 0xb9],
);
/// `HOB_LIST_TABLE_GUID`.
pub const HOB_LIST_TABLE_GUID: EfiGuid = EfiGuid::new(
    0x7739f24c,
    0x93d7,
    0x11d4,
    [0x9a, 0x3a, 0x0, 0x90, 0x27, 0x3f, 0xc1, 0x4d],
);
/// `MEMORY_TYPE_INFORMATION_TABLE_GUID`.
pub const MEMORY_TYPE_INFORMATION_TABLE_GUID: EfiGuid = EfiGuid::new(
    0x4c19049f,
    0x4137,
    0x4dd3,
    [0x9c, 0x10, 0x8b, 0x97, 0xa8, 0x3f, 0xfd, 0xfa],
);
/// `DEBUG_IMAGE_INFO_TABLE_GUID`.
pub const DEBUG_IMAGE_INFO_TABLE_GUID: EfiGuid = EfiGuid::new(
    0x49152e77,
    0x1ada,
    0x4764,
    [0xb7, 0xa2, 0x7a, 0xfe, 0xfe, 0xd9, 0x5e, 0x8b],
);
/// `EFI_SYSTEM_RESOURCE_TABLE_GUID`.
pub const EFI_SYSTEM_RESOURCE_TABLE_GUID: EfiGuid = EfiGuid::new(
    0xb122a263,
    0x3661,
    0x4f68,
    [0x99, 0x29, 0x78, 0xf8, 0xb0, 0xd6, 0x21, 0x80],
);

/// `EFI_SYSTEM_TABLE_SIGNATURE`.
pub const EFI_SYSTEM_TABLE_SIGNATURE: UINT64 = 0x5453595320494249;
/// `EFI_SYSTEM_TABLE_REVISION`.
pub const EFI_SYSTEM_TABLE_REVISION: UINT32 =
    (EFI_SPECIFICATION_MAJOR_REVISION << 16) | EFI_SPECIFICATION_MINOR_REVISION;
/// `EFI_1_10_SYSTEM_TABLE_REVISION`.
pub const EFI_1_10_SYSTEM_TABLE_REVISION: UINT32 = (1 << 16) | 10;
/// `EFI_1_02_SYSTEM_TABLE_REVISION`.
pub const EFI_1_02_SYSTEM_TABLE_REVISION: UINT32 = (1 << 16) | 2;

/// `EFI_SYSTEM_RESOURCE_TABLE_FIRMWARE_RESOURCE_VERSION`.
pub const EFI_SYSTEM_RESOURCE_TABLE_FIRMWARE_RESOURCE_VERSION: UINT64 = 1;

/// `EFI_ALLOCATE_PAGES`.
pub type EfiAllocatePages = unsafe extern "efiapi" fn(
    Type: EfiAllocateType,
    MemoryType: EfiMemoryType,
    NoPages: UINTN,
    Memory: *mut EfiPhysicalAddress,
) -> EfiStatus;

/// `EFI_FREE_PAGES`.
pub type EfiFreePages =
    unsafe extern "efiapi" fn(Memory: EfiPhysicalAddress, NoPages: UINTN) -> EfiStatus;

/// `EFI_GET_MEMORY_MAP`.
pub type EfiGetMemoryMap = unsafe extern "efiapi" fn(
    MemoryMapSize: *mut UINTN,
    MemoryMap: *mut EfiMemoryDescriptor,
    MapKey: *mut UINTN,
    DescriptorSize: *mut UINTN,
    DescriptorVersion: *mut UINT32,
) -> EfiStatus;

/// `EFI_ALLOCATE_POOL`.
pub type EfiAllocatePool = unsafe extern "efiapi" fn(
    PoolType: EfiMemoryType,
    Size: UINTN,
    Buffer: *mut *mut c_void,
) -> EfiStatus;

/// `EFI_FREE_POOL`.
pub type EfiFreePool = unsafe extern "efiapi" fn(Buffer: *mut c_void) -> EfiStatus;

/// `EFI_SET_VIRTUAL_ADDRESS_MAP`.
pub type EfiSetVirtualAddressMap = unsafe extern "efiapi" fn(
    MemoryMapSize: UINTN,
    DescriptorSize: UINTN,
    DescriptorVersion: UINT32,
    VirtualMap: *mut EfiMemoryDescriptor,
) -> EfiStatus;

/// `EFI_CONVERT_POINTER`.
pub type EfiConvertPointer =
    unsafe extern "efiapi" fn(DebugDisposition: UINTN, Address: *mut *mut c_void) -> EfiStatus;

/// `EFI_EVENT_NOTIFY`.
pub type EfiEventNotify = unsafe extern "efiapi" fn(Event: EfiEvent, Context: *mut c_void);

/// `EFI_CREATE_EVENT`.
pub type EfiCreateEvent = unsafe extern "efiapi" fn(
    Type: UINT32,
    NotifyTpl: EfiTpl,
    NotifyFunction: Option<EfiEventNotify>,
    NotifyContext: *mut c_void,
    Event: *mut EfiEvent,
) -> EfiStatus;

/// `EFI_SET_TIMER`.
pub type EfiSetTimer = unsafe extern "efiapi" fn(
    Event: EfiEvent,
    Type: EfiTimerDelay,
    TriggerTime: UINT64,
) -> EfiStatus;

/// `EFI_SIGNAL_EVENT`.
pub type EfiSignalEvent = unsafe extern "efiapi" fn(Event: EfiEvent) -> EfiStatus;

/// `EFI_WAIT_FOR_EVENT`.
pub type EfiWaitForEvent = unsafe extern "efiapi" fn(
    NumberOfEvents: UINTN,
    Event: *mut EfiEvent,
    Index: *mut UINTN,
) -> EfiStatus;

/// `EFI_CLOSE_EVENT`.
pub type EfiCloseEvent = unsafe extern "efiapi" fn(Event: EfiEvent) -> EfiStatus;

/// `EFI_CHECK_EVENT`.
pub type EfiCheckEvent = unsafe extern "efiapi" fn(Event: EfiEvent) -> EfiStatus;

/// `EFI_RAISE_TPL`.
pub type EfiRaiseTpl = unsafe extern "efiapi" fn(NewTpl: EfiTpl) -> EfiTpl;

/// `EFI_RESTORE_TPL`.
pub type EfiRestoreTpl = unsafe extern "efiapi" fn(OldTpl: EfiTpl);

/// `EFI_GET_VARIABLE`.
pub type EfiGetVariable = unsafe extern "efiapi" fn(
    VariableName: *mut CHAR16,
    VendorGuid: *mut EfiGuid,
    Attributes: *mut UINT32,
    DataSize: *mut UINTN,
    Data: *mut c_void,
) -> EfiStatus;

/// `EFI_GET_NEXT_VARIABLE_NAME`.
pub type EfiGetNextVariableName = unsafe extern "efiapi" fn(
    VariableNameSize: *mut UINTN,
    VariableName: *mut CHAR16,
    VendorGuid: *mut EfiGuid,
) -> EfiStatus;

/// `EFI_SET_VARIABLE`.
pub type EfiSetVariable = unsafe extern "efiapi" fn(
    VariableName: *mut CHAR16,
    VendorGuid: *mut EfiGuid,
    Attributes: UINT32,
    DataSize: UINTN,
    Data: *mut c_void,
) -> EfiStatus;

/// `EFI_TIME_CAPABILITIES`.
#[repr(C)]
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct EfiTimeCapabilities {
    /// Resolution, in counts per second of the clock.
    pub Resolution: UINT32,
    /// Accuracy, in hertz.
    pub Accuracy: UINT32,
    /// Setting the time clears the sub-second time.
    pub SetsToZero: BOOLEAN,
}

/// `EFI_GET_TIME`.
pub type EfiGetTime = unsafe extern "efiapi" fn(
    Time: *mut EfiTime,
    Capabilities: *mut EfiTimeCapabilities,
) -> EfiStatus;

/// `EFI_SET_TIME`.
pub type EfiSetTime = unsafe extern "efiapi" fn(Time: *mut EfiTime) -> EfiStatus;

/// `EFI_GET_WAKEUP_TIME`.
pub type EfiGetWakeupTime = unsafe extern "efiapi" fn(
    Enabled: *mut BOOLEAN,
    Pending: *mut BOOLEAN,
    Time: *mut EfiTime,
) -> EfiStatus;

/// `EFI_SET_WAKEUP_TIME`.
pub type EfiSetWakeupTime =
    unsafe extern "efiapi" fn(Enable: BOOLEAN, Time: *mut EfiTime) -> EfiStatus;

/// `EFI_IMAGE_ENTRY_POINT`.
pub type EfiImageEntryPoint = unsafe extern "efiapi" fn(
    ImageHandle: EfiHandle,
    SystemTable: *mut EfiSystemTable,
) -> EfiStatus;

/// `EFI_IMAGE_LOAD`.
pub type EfiImageLoad = unsafe extern "efiapi" fn(
    BootPolicy: BOOLEAN,
    ParentImageHandle: EfiHandle,
    FilePath: *mut EfiDevicePath,
    SourceBuffer: *mut c_void,
    SourceSize: UINTN,
    ImageHandle: *mut EfiHandle,
) -> EfiStatus;

/// `EFI_IMAGE_START`.
pub type EfiImageStart = unsafe extern "efiapi" fn(
    ImageHandle: EfiHandle,
    ExitDataSize: *mut UINTN,
    ExitData: *mut *mut CHAR16,
) -> EfiStatus;

/// `EFI_EXIT`.
pub type EfiExit = unsafe extern "efiapi" fn(
    ImageHandle: EfiHandle,
    ExitStatus: EfiStatus,
    ExitDataSize: UINTN,
    ExitData: *mut CHAR16,
) -> EfiStatus;

/// `EFI_IMAGE_UNLOAD`.
pub type EfiImageUnload = unsafe extern "efiapi" fn(ImageHandle: EfiHandle) -> EfiStatus;

/// `EFI_LOADED_IMAGE`: the loaded image protocol, installed on every image's handle.
#[repr(C)]
#[derive(Clone, Copy)]
pub struct EfiLoadedImage {
    /// `EFI_LOADED_IMAGE_INFORMATION_REVISION`.
    pub Revision: UINT32,
    /// Handle of the image that loaded this one.
    pub ParentHandle: EfiHandle,
    /// The system table.
    pub SystemTable: *mut EfiSystemTable,
    /// Handle of the device the image was loaded from.
    pub DeviceHandle: EfiHandle,
    /// Device path of the image file.
    pub FilePath: *mut EfiDevicePath,
    /// Reserved.
    pub Reserved: *mut c_void,
    /// Size of `LoadOptions` in bytes.
    pub LoadOptionsSize: UINT32,
    /// Load options.
    pub LoadOptions: *mut c_void,
    /// Where the image was loaded.
    pub ImageBase: *mut c_void,
    /// Size of the image.
    pub ImageSize: UINT64,
    /// Memory type of the image's code.
    pub ImageCodeType: EfiMemoryType,
    /// Memory type of the image's data.
    pub ImageDataType: EfiMemoryType,
    /// Set when the image supports a dynamic unload request.
    pub Unload: Option<EfiImageUnload>,
}

/// `EFI_EXIT_BOOT_SERVICES`.
pub type EfiExitBootServices =
    unsafe extern "efiapi" fn(ImageHandle: EfiHandle, MapKey: UINTN) -> EfiStatus;

/// `EFI_STALL`.
pub type EfiStall = unsafe extern "efiapi" fn(Microseconds: UINTN) -> EfiStatus;

/// `EFI_SET_WATCHDOG_TIMER`.
pub type EfiSetWatchdogTimer = unsafe extern "efiapi" fn(
    Timeout: UINTN,
    WatchdogCode: UINT64,
    DataSize: UINTN,
    WatchdogData: *mut CHAR16,
) -> EfiStatus;

/// `EFI_RESET_SYSTEM`.
pub type EfiResetSystem = unsafe extern "efiapi" fn(
    ResetType: EfiResetType,
    ResetStatus: EfiStatus,
    DataSize: UINTN,
    ResetData: *mut CHAR16,
);

/// `EFI_GET_NEXT_MONOTONIC_COUNT`.
pub type EfiGetNextMonotonicCount = unsafe extern "efiapi" fn(Count: *mut UINT64) -> EfiStatus;

/// `EFI_GET_NEXT_HIGH_MONO_COUNT`.
pub type EfiGetNextHighMonoCount = unsafe extern "efiapi" fn(HighCount: *mut UINT32) -> EfiStatus;

/// `EFI_INSTALL_PROTOCOL_INTERFACE`.
pub type EfiInstallProtocolInterface = unsafe extern "efiapi" fn(
    Handle: *mut EfiHandle,
    Protocol: *mut EfiGuid,
    InterfaceType: EfiInterfaceType,
    Interface: *mut c_void,
) -> EfiStatus;

/// `EFI_REINSTALL_PROTOCOL_INTERFACE`.
pub type EfiReinstallProtocolInterface = unsafe extern "efiapi" fn(
    Handle: EfiHandle,
    Protocol: *mut EfiGuid,
    OldInterface: *mut c_void,
    NewInterface: *mut c_void,
) -> EfiStatus;

/// `EFI_UNINSTALL_PROTOCOL_INTERFACE`.
pub type EfiUninstallProtocolInterface = unsafe extern "efiapi" fn(
    Handle: EfiHandle,
    Protocol: *mut EfiGuid,
    Interface: *mut c_void,
) -> EfiStatus;

/// `EFI_HANDLE_PROTOCOL`.
pub type EfiHandleProtocol = unsafe extern "efiapi" fn(
    Handle: EfiHandle,
    Protocol: *mut EfiGuid,
    Interface: *mut *mut c_void,
) -> EfiStatus;

/// `EFI_REGISTER_PROTOCOL_NOTIFY`.
pub type EfiRegisterProtocolNotify = unsafe extern "efiapi" fn(
    Protocol: *mut EfiGuid,
    Event: EfiEvent,
    Registration: *mut *mut c_void,
) -> EfiStatus;

/// `EFI_LOCATE_HANDLE`.
pub type EfiLocateHandle = unsafe extern "efiapi" fn(
    SearchType: EfiLocateSearchType,
    Protocol: *mut EfiGuid,
    SearchKey: *mut c_void,
    BufferSize: *mut UINTN,
    Buffer: *mut EfiHandle,
) -> EfiStatus;

/// `EFI_LOCATE_DEVICE_PATH`.
pub type EfiLocateDevicePath = unsafe extern "efiapi" fn(
    Protocol: *mut EfiGuid,
    DevicePath: *mut *mut EfiDevicePath,
    Device: *mut EfiHandle,
) -> EfiStatus;

/// `EFI_INSTALL_CONFIGURATION_TABLE`.
pub type EfiInstallConfigurationTable =
    unsafe extern "efiapi" fn(Guid: *mut EfiGuid, Table: *mut c_void) -> EfiStatus;

/// `EFI_RESERVED_SERVICE`.
pub type EfiReservedService = unsafe extern "efiapi" fn() -> EfiStatus;

/// `EFI_CONNECT_CONTROLLER`.
pub type EfiConnectController = unsafe extern "efiapi" fn(
    ControllerHandle: EfiHandle,
    DriverImageHandle: *mut EfiHandle,
    RemainingDevicePath: *mut EfiDevicePath,
    Recursive: BOOLEAN,
) -> EfiStatus;

/// `EFI_DISCONNECT_CONTROLLER`.
pub type EfiDisconnectController = unsafe extern "efiapi" fn(
    ControllerHandle: EfiHandle,
    DriverImageHandle: EfiHandle,
    ChildHandle: EfiHandle,
) -> EfiStatus;

/// `EFI_OPEN_PROTOCOL`.
pub type EfiOpenProtocol = unsafe extern "efiapi" fn(
    Handle: EfiHandle,
    Protocol: *mut EfiGuid,
    Interface: *mut *mut c_void,
    ImageHandle: EfiHandle,
    ControllerHandle: EfiHandle,
    Attributes: UINT32,
) -> EfiStatus;

/// `EFI_CLOSE_PROTOCOL`.
pub type EfiCloseProtocol = unsafe extern "efiapi" fn(
    Handle: EfiHandle,
    Protocol: *mut EfiGuid,
    ImageHandle: EfiHandle,
    DeviceHandle: EfiHandle,
) -> EfiStatus;

/// `EFI_OPEN_PROTOCOL_INFORMATION_ENTRY`.
#[repr(C)]
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct EfiOpenProtocolInformationEntry {
    /// Agent handle.
    pub AgentHandle: EfiHandle,
    /// Controller handle.
    pub ControllerHandle: EfiHandle,
    /// `EFI_OPEN_PROTOCOL_*` attributes.
    pub Attributes: UINT32,
    /// Open count.
    pub OpenCount: UINT32,
}

/// `EFI_OPEN_PROTOCOL_INFORMATION`.
pub type EfiOpenProtocolInformation = unsafe extern "efiapi" fn(
    UserHandle: EfiHandle,
    Protocol: *mut EfiGuid,
    EntryBuffer: *mut *mut EfiOpenProtocolInformationEntry,
    EntryCount: *mut UINTN,
) -> EfiStatus;

/// `EFI_PROTOCOLS_PER_HANDLE`.
pub type EfiProtocolsPerHandle = unsafe extern "efiapi" fn(
    UserHandle: EfiHandle,
    ProtocolBuffer: *mut *mut *mut EfiGuid,
    ProtocolBufferCount: *mut UINTN,
) -> EfiStatus;

/// `EFI_LOCATE_HANDLE_BUFFER`.
pub type EfiLocateHandleBuffer = unsafe extern "efiapi" fn(
    SearchType: EfiLocateSearchType,
    Protocol: *mut EfiGuid,
    SearchKey: *mut c_void,
    NumberHandles: *mut UINTN,
    Buffer: *mut *mut EfiHandle,
) -> EfiStatus;

/// `EFI_LOCATE_PROTOCOL`.
pub type EfiLocateProtocol = unsafe extern "efiapi" fn(
    Protocol: *mut EfiGuid,
    Registration: *mut c_void,
    Interface: *mut *mut c_void,
) -> EfiStatus;

/// `EFI_CALCULATE_CRC32`.
pub type EfiCalculateCrc32 =
    unsafe extern "efiapi" fn(Data: *mut c_void, DataSize: UINTN, Crc32: *mut UINT32) -> EfiStatus;

/// `EFI_COPY_MEM`.
pub type EfiCopyMem =
    unsafe extern "efiapi" fn(Destination: *mut c_void, Source: *mut c_void, Length: UINTN);

/// `EFI_SET_MEM`.
pub type EfiSetMem = unsafe extern "efiapi" fn(Buffer: *mut c_void, Size: UINTN, Value: UINT8);

/// `EFI_TABLE_HEADER` (C tag `_EFI_TABLE_HEARDER`): the header of every EFI table.
#[repr(C)]
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct EfiTableHeader {
    /// Table signature.
    pub Signature: UINT64,
    /// Table revision.
    pub Revision: UINT32,
    /// Size of the table including this header.
    pub HeaderSize: UINT32,
    /// CRC-32 of the table.
    pub CRC32: UINT32,
    /// Reserved.
    pub Reserved: UINT32,
}

/// `EFI_RUNTIME_SERVICES`: the runtime services table.
#[repr(C)]
#[derive(Clone, Copy)]
pub struct EfiRuntimeServices {
    /// Table header.
    pub Hdr: EfiTableHeader,
    /// `GetTime`.
    pub GetTime: EfiGetTime,
    /// `SetTime`.
    pub SetTime: EfiSetTime,
    /// `GetWakeupTime`.
    pub GetWakeupTime: EfiGetWakeupTime,
    /// `SetWakeupTime`.
    pub SetWakeupTime: EfiSetWakeupTime,
    /// `SetVirtualAddressMap`.
    pub SetVirtualAddressMap: EfiSetVirtualAddressMap,
    /// `ConvertPointer`.
    pub ConvertPointer: EfiConvertPointer,
    /// `GetVariable`.
    pub GetVariable: EfiGetVariable,
    /// `GetNextVariableName`.
    pub GetNextVariableName: EfiGetNextVariableName,
    /// `SetVariable`.
    pub SetVariable: EfiSetVariable,
    /// `GetNextHighMonotonicCount`.
    pub GetNextHighMonotonicCount: EfiGetNextHighMonoCount,
    /// `ResetSystem`.
    pub ResetSystem: EfiResetSystem,
}

/// `EFI_BOOT_SERVICES`: the boot services table.
#[repr(C)]
#[derive(Clone, Copy)]
pub struct EfiBootServices {
    /// Table header.
    pub Hdr: EfiTableHeader,
    /// `RaiseTPL`.
    pub RaiseTPL: EfiRaiseTpl,
    /// `RestoreTPL`.
    pub RestoreTPL: EfiRestoreTpl,
    /// `AllocatePages`.
    pub AllocatePages: EfiAllocatePages,
    /// `FreePages`.
    pub FreePages: EfiFreePages,
    /// `GetMemoryMap`.
    pub GetMemoryMap: EfiGetMemoryMap,
    /// `AllocatePool`.
    pub AllocatePool: EfiAllocatePool,
    /// `FreePool`.
    pub FreePool: EfiFreePool,
    /// `CreateEvent`.
    pub CreateEvent: EfiCreateEvent,
    /// `SetTimer`.
    pub SetTimer: EfiSetTimer,
    /// `WaitForEvent`.
    pub WaitForEvent: EfiWaitForEvent,
    /// `SignalEvent`.
    pub SignalEvent: EfiSignalEvent,
    /// `CloseEvent`.
    pub CloseEvent: EfiCloseEvent,
    /// `CheckEvent`.
    pub CheckEvent: EfiCheckEvent,
    /// `InstallProtocolInterface`.
    pub InstallProtocolInterface: EfiInstallProtocolInterface,
    /// `ReinstallProtocolInterface`.
    pub ReinstallProtocolInterface: EfiReinstallProtocolInterface,
    /// `UninstallProtocolInterface`.
    pub UninstallProtocolInterface: EfiUninstallProtocolInterface,
    /// `HandleProtocol`.
    pub HandleProtocol: EfiHandleProtocol,
    /// Reserved.
    pub Reserved: *mut c_void,
    /// `RegisterProtocolNotify`.
    pub RegisterProtocolNotify: EfiRegisterProtocolNotify,
    /// `LocateHandle`.
    pub LocateHandle: EfiLocateHandle,
    /// `LocateDevicePath`.
    pub LocateDevicePath: EfiLocateDevicePath,
    /// `InstallConfigurationTable`.
    pub InstallConfigurationTable: EfiInstallConfigurationTable,
    /// `LoadImage`.
    pub LoadImage: EfiImageLoad,
    /// `StartImage`.
    pub StartImage: EfiImageStart,
    /// `Exit`.
    pub Exit: EfiExit,
    /// `UnloadImage`.
    pub UnloadImage: EfiImageUnload,
    /// `ExitBootServices`.
    pub ExitBootServices: EfiExitBootServices,
    /// `GetNextMonotonicCount`.
    pub GetNextMonotonicCount: EfiGetNextMonotonicCount,
    /// `Stall`.
    pub Stall: EfiStall,
    /// `SetWatchdogTimer`.
    pub SetWatchdogTimer: EfiSetWatchdogTimer,
    /// `ConnectController`.
    pub ConnectController: EfiConnectController,
    /// `DisconnectController`.
    pub DisconnectController: EfiDisconnectController,
    /// `OpenProtocol`.
    pub OpenProtocol: EfiOpenProtocol,
    /// `CloseProtocol`.
    pub CloseProtocol: EfiCloseProtocol,
    /// `OpenProtocolInformation`.
    pub OpenProtocolInformation: EfiOpenProtocolInformation,
    /// `ProtocolsPerHandle`.
    pub ProtocolsPerHandle: EfiProtocolsPerHandle,
    /// `LocateHandleBuffer`.
    pub LocateHandleBuffer: EfiLocateHandleBuffer,
    /// `LocateProtocol`.
    pub LocateProtocol: EfiLocateProtocol,
    /// `InstallMultipleProtocolInterfaces`: C-variadic, see the module docs.
    pub InstallMultipleProtocolInterfaces: *const c_void,
    /// `UninstallMultipleProtocolInterfaces`: C-variadic, see the module docs.
    pub UninstallMultipleProtocolInterfaces: *const c_void,
    /// `CalculateCrc32`.
    pub CalculateCrc32: EfiCalculateCrc32,
    /// `CopyMem`.
    pub CopyMem: EfiCopyMem,
    /// `SetMem`.
    pub SetMem: EfiSetMem,
}

/// `EFI_CONFIGURATION_TABLE`.
#[repr(C)]
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct EfiConfigurationTable {
    /// GUID that names the table.
    pub VendorGuid: EfiGuid,
    /// The table.
    pub VendorTable: *mut c_void,
}

/// `EFI_SYSTEM_TABLE`: the system table every image receives.
#[repr(C)]
#[derive(Clone, Copy)]
pub struct EfiSystemTable {
    /// Table header.
    pub Hdr: EfiTableHeader,
    /// Firmware vendor, a NUL-terminated UCS-2 string.
    pub FirmwareVendor: *mut CHAR16,
    /// Firmware revision.
    pub FirmwareRevision: UINT32,
    /// Console input handle.
    pub ConsoleInHandle: EfiHandle,
    /// Console input.
    pub ConIn: *mut SimpleInputInterface,
    /// Console output handle.
    pub ConsoleOutHandle: EfiHandle,
    /// Console output.
    pub ConOut: *mut SimpleTextOutputInterface,
    /// Standard error handle.
    pub StandardErrorHandle: EfiHandle,
    /// Standard error.
    pub StdErr: *mut SimpleTextOutputInterface,
    /// Runtime services.
    pub RuntimeServices: *mut EfiRuntimeServices,
    /// Boot services.
    pub BootServices: *mut EfiBootServices,
    /// Number of configuration table entries.
    pub NumberOfTableEntries: UINTN,
    /// Configuration tables.
    pub ConfigurationTable: *mut EfiConfigurationTable,
}

/// `EFI_SYSTEM_RESOURCE_TABLE`: the `EFI_SYSTEM_RESOURCE_ENTRY` entries follow it in memory.
#[repr(C)]
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct EfiSystemResourceTable {
    /// Number of entries.
    pub FwResourceCount: UINT32,
    /// Maximum number of entries.
    pub FwResourceCountMax: UINT32,
    /// Version of the table.
    pub FwResourceVersion: UINT64,
}

/// `EFI_SYSTEM_RESOURCE_ENTRY`.
#[repr(C)]
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct EfiSystemResourceEntry {
    /// Firmware class GUID.
    pub FwClass: EfiGuid,
    /// Firmware type.
    pub FwType: UINT32,
    /// Firmware version.
    pub FwVersion: UINT32,
    /// Lowest supported firmware version.
    pub LowestSupportedFwVersion: UINT32,
    /// Capsule flags.
    pub CapsuleFlags: UINT32,
    /// Last attempted version.
    pub LastAttemptVersion: UINT32,
    /// Last attempt status.
    pub LastAttemptStatus: UINT32,
}

/// `NextMemoryDescriptor(Ptr, Size)`: the descriptor `size` bytes after `ptr`. `size` is the
/// `DescriptorSize` that `GetMemoryMap` returned, which can exceed
/// `size_of::<EfiMemoryDescriptor>()`. The result is only an address: it is not read here.
pub fn next_memory_descriptor(
    ptr: *const EfiMemoryDescriptor,
    size: UINTN,
) -> *const EfiMemoryDescriptor {
    ptr.wrapping_byte_add(size)
}

const _: () = assert!(core::mem::size_of::<EfiTableHeader>() == 24);
const _: () = assert!(core::mem::size_of::<EfiTimeCapabilities>() == 12);
const _: () = assert!(core::mem::size_of::<EfiLoadedImage>() == 96);
const _: () = assert!(core::mem::size_of::<EfiOpenProtocolInformationEntry>() == 24);
const _: () = assert!(core::mem::size_of::<EfiRuntimeServices>() == 24 + 11 * 8);
const _: () = assert!(core::mem::size_of::<EfiBootServices>() == 24 + 43 * 8);
const _: () = assert!(core::mem::size_of::<EfiConfigurationTable>() == 24);
const _: () = assert!(core::mem::size_of::<EfiSystemTable>() == 120);
const _: () = assert!(core::mem::size_of::<EfiSystemResourceTable>() == 16);
const _: () = assert!(core::mem::size_of::<EfiSystemResourceEntry>() == 40);
const _: () = assert!(EFI_SYSTEM_TABLE_REVISION == 0x0001_000a);
/* </CODE> */

/* <TESTS> */
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn memory_map_stride_follows_the_reported_size() {
        let base = 0x1000usize as *const EfiMemoryDescriptor;
        assert_eq!(next_memory_descriptor(base, 48) as usize, 0x1030);
        assert_eq!(next_memory_descriptor(base, 40) as usize, 0x1028);
    }

    #[test]
    fn revisions() {
        assert_eq!(EFI_1_02_SYSTEM_TABLE_REVISION, 0x0001_0002);
        assert_eq!(EFI_BOOT_SERVICES_REVISION, EFI_RUNTIME_SERVICES_REVISION);
    }
}
/* </TESTS> */
