/* $FreeBSD: head/sys/boot/efi/include/efidevp.h 312314 2017-01-16 20:57:01Z tsoome $ */
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

    devpath.h

Abstract:

    Defines for parsing the EFI Device Path structures



Revision History

--*/
/* </LICENSES> */

/* <CODE> */
//! EFI device path structures (UEFI specification, section C).
//!
//! Upstream: sys/stand/efi/include/efidevp.h @ 3ce1f3f79392
//!
//! A device path is a sequence of variable-length nodes, each starting with an
//! [`EfiDevicePath`] header (type, sub-type, 16-bit little-endian length), ended by an
//! "end entire device path" node. The typed node structs mirror the C layout (`#[repr(C)]`,
//! natural alignment, not packed), but a node inside a path is only guaranteed byte-aligned:
//! read its fields with `read_unaligned` unless the alignment is known.
//!
//! ## Deviations
//! - Struct names are the C typedefs in CamelCase (`PCI_DEVICE_PATH` -> `PciDevicePath`,
//!   `IPv4_DEVICE_PATH` -> `Ipv4DevicePath`, `UKNOWN_DEVICE_VENDOR_DEVICE_PATH` ->
//!   `UnknownDeviceVendorDevicePath`, ...); field names are verbatim.
//! - The path macros are functions in snake_case: `DevicePathType` -> [`device_path_type`],
//!   `DevicePathSubType`, `DevicePathNodeLength`, `NextDevicePathNode`, `IsDevicePathType`,
//!   `IsDevicePathEndType`, `IsDevicePathEndSubType`, `IsDevicePathEnd`,
//!   `IsDevicePathUnpacked`, `SetDevicePathNodeLength`, `SetDevicePathEndNode`. The ones that
//!   read or write through a node pointer are `unsafe fn`.
//! - `DP_IS_END_TYPE(a)` and `DP_IS_END_SUBTYPE(a)` are unusable in C (the first is empty,
//!   the second has unbalanced parentheses) and are left out.
//! - `EISA_ID`, `EISA_PNP_ID`, `EFI_PNP_ID` and `EISA_ID_TO_NUM` are const fns
//!   ([`eisa_id`], [`eisa_pnp_id`], [`efi_pnp_id`], [`eisa_id_to_num`]).
//! - `EFI_FIELD_OFFSET` is `core::mem::offset_of!` in `SIZE_OF_FILEPATH_DEVICE_PATH`.
//! - The GUID initialiser macros are `EfiGuid` constants of the same name.
//! - `EFI_DEV_PATH` and `EFI_DEV_PATH_PTR` are `#[repr(C)]` unions; reading a field is unsafe.
//! - The flexible arrays (`PathName[1]`, `String[1]`) keep their one-element declaration.
//! - `EFI_DEVICE_PATH_TO_TEXT_NODE`/`_PATH` function pointers are `extern "efiapi"`.

#![allow(non_snake_case, non_upper_case_globals, non_camel_case_types)]
// EFI names are the specification's (Header, HID, EISA_ID, ...), kept verbatim for grep-ability

use core::mem::{offset_of, size_of};

use super::amd64::efibind::{UINT8, UINT16, UINT32, UINT64, UINTN};
use super::efidef::{
    BOOLEAN, CHAR8, CHAR16, EfiGuid, EfiIpv4Address, EfiIpv6Address, EfiMacAddress,
    EfiPhysicalAddress,
};

/// `EFI_DP_TYPE_MASK`.
pub const EFI_DP_TYPE_MASK: UINT8 = 0x7F;
/// `EFI_DP_TYPE_UNPACKED`.
pub const EFI_DP_TYPE_UNPACKED: UINT8 = 0x80;

/// `END_DEVICE_PATH_TYPE`.
pub const END_DEVICE_PATH_TYPE: UINT8 = 0x7f;

/// `END_ENTIRE_DEVICE_PATH_SUBTYPE`.
pub const END_ENTIRE_DEVICE_PATH_SUBTYPE: UINT8 = 0xff;
/// `END_INSTANCE_DEVICE_PATH_SUBTYPE`.
pub const END_INSTANCE_DEVICE_PATH_SUBTYPE: UINT8 = 0x01;
/// `END_DEVICE_PATH_LENGTH`: the size of an end node (`sizeof(EFI_DEVICE_PATH)`).
pub const END_DEVICE_PATH_LENGTH: UINTN = size_of::<EfiDevicePath>();

/// `HARDWARE_DEVICE_PATH`.
pub const HARDWARE_DEVICE_PATH: UINT8 = 0x01;
/// `HW_PCI_DP`.
pub const HW_PCI_DP: UINT8 = 0x01;
/// `HW_PCCARD_DP`.
pub const HW_PCCARD_DP: UINT8 = 0x02;
/// `HW_MEMMAP_DP`.
pub const HW_MEMMAP_DP: UINT8 = 0x03;
/// `HW_VENDOR_DP`.
pub const HW_VENDOR_DP: UINT8 = 0x04;
/// `HW_CONTROLLER_DP`.
pub const HW_CONTROLLER_DP: UINT8 = 0x05;

/// `UNKNOWN_DEVICE_GUID`.
pub const UNKNOWN_DEVICE_GUID: EfiGuid = EfiGuid::new(
    0xcf31fac5,
    0xc24e,
    0x11d2,
    [0x85, 0xf3, 0x0, 0xa0, 0xc9, 0x3e, 0xc9, 0x3b],
);

/// `ACPI_DEVICE_PATH`.
pub const ACPI_DEVICE_PATH: UINT8 = 0x02;
/// `ACPI_DP`.
pub const ACPI_DP: UINT8 = 0x01;
/// `ACPI_EXTENDED_DP`.
pub const ACPI_EXTENDED_DP: UINT8 = 0x02;

/// `PNP_EISA_ID_CONST`.
pub const PNP_EISA_ID_CONST: UINT32 = 0x41d0;
/// `PNP_EISA_ID_MASK`.
pub const PNP_EISA_ID_MASK: UINT32 = 0xffff;

/// `MESSAGING_DEVICE_PATH`.
pub const MESSAGING_DEVICE_PATH: UINT8 = 0x03;
/// `MSG_ATAPI_DP`.
pub const MSG_ATAPI_DP: UINT8 = 0x01;
/// `MSG_SCSI_DP`.
pub const MSG_SCSI_DP: UINT8 = 0x02;
/// `MSG_FIBRECHANNEL_DP`.
pub const MSG_FIBRECHANNEL_DP: UINT8 = 0x03;
/// `MSG_1394_DP`.
pub const MSG_1394_DP: UINT8 = 0x04;
/// `MSG_USB_DP`.
pub const MSG_USB_DP: UINT8 = 0x05;
/// `MSG_USB_CLASS_DP`.
pub const MSG_USB_CLASS_DP: UINT8 = 0x0F;
/// `MSG_I2O_DP`.
pub const MSG_I2O_DP: UINT8 = 0x06;
/// `MSG_MAC_ADDR_DP`.
pub const MSG_MAC_ADDR_DP: UINT8 = 0x0b;
/// `MSG_IPv4_DP`.
pub const MSG_IPv4_DP: UINT8 = 0x0c;
/// `MSG_IPv6_DP`.
pub const MSG_IPv6_DP: UINT8 = 0x0d;
/// `MSG_INFINIBAND_DP`.
pub const MSG_INFINIBAND_DP: UINT8 = 0x09;

/// `INFINIBAND_RESOURCE_FLAG_IOC_SERVICE`.
pub const INFINIBAND_RESOURCE_FLAG_IOC_SERVICE: UINT32 = 0x01;
/// `INFINIBAND_RESOURCE_FLAG_EXTENDED_BOOT_ENVIRONMENT`.
pub const INFINIBAND_RESOURCE_FLAG_EXTENDED_BOOT_ENVIRONMENT: UINT32 = 0x02;
/// `INFINIBAND_RESOURCE_FLAG_CONSOLE_PROTOCOL`.
pub const INFINIBAND_RESOURCE_FLAG_CONSOLE_PROTOCOL: UINT32 = 0x04;
/// `INFINIBAND_RESOURCE_FLAG_STORAGE_PROTOCOL`.
pub const INFINIBAND_RESOURCE_FLAG_STORAGE_PROTOCOL: UINT32 = 0x08;
/// `INFINIBAND_RESOURCE_FLAG_NETWORK_PROTOCOL`.
pub const INFINIBAND_RESOURCE_FLAG_NETWORK_PROTOCOL: UINT32 = 0x10;

/// `MSG_UART_DP`.
pub const MSG_UART_DP: UINT8 = 0x0e;
/// `MSG_VENDOR_DP` (the node is a [`VendorDevicePath`]).
pub const MSG_VENDOR_DP: UINT8 = 0x0A;

/// `DEVICE_PATH_MESSAGING_PC_ANSI`.
pub const DEVICE_PATH_MESSAGING_PC_ANSI: EfiGuid = EfiGuid::new(
    0xe0c14753,
    0xf9be,
    0x11d2,
    [0x9a, 0x0c, 0x00, 0x90, 0x27, 0x3f, 0xc1, 0x4d],
);
/// `DEVICE_PATH_MESSAGING_VT_100`.
pub const DEVICE_PATH_MESSAGING_VT_100: EfiGuid = EfiGuid::new(
    0xdfa66065,
    0xb419,
    0x11d3,
    [0x9a, 0x2d, 0x00, 0x90, 0x27, 0x3f, 0xc1, 0x4d],
);
/// `DEVICE_PATH_MESSAGING_VT_100_PLUS`.
pub const DEVICE_PATH_MESSAGING_VT_100_PLUS: EfiGuid = EfiGuid::new(
    0x7baec70b,
    0x57e0,
    0x4c76,
    [0x8e, 0x87, 0x2f, 0x9e, 0x28, 0x08, 0x83, 0x43],
);
/// `DEVICE_PATH_MESSAGING_VT_UTF8`.
pub const DEVICE_PATH_MESSAGING_VT_UTF8: EfiGuid = EfiGuid::new(
    0xad15a0d6,
    0x8bec,
    0x4acf,
    [0xa0, 0x73, 0xd0, 0x1d, 0xe7, 0x7e, 0x2d, 0x88],
);

/// `MSG_SATA_DP`.
pub const MSG_SATA_DP: UINT8 = 0x12;

/// `MEDIA_DEVICE_PATH`.
pub const MEDIA_DEVICE_PATH: UINT8 = 0x04;
/// `MEDIA_HARDDRIVE_DP`.
pub const MEDIA_HARDDRIVE_DP: UINT8 = 0x01;

/// `MBR_TYPE_PCAT`.
pub const MBR_TYPE_PCAT: UINT8 = 0x01;
/// `MBR_TYPE_EFI_PARTITION_TABLE_HEADER`.
pub const MBR_TYPE_EFI_PARTITION_TABLE_HEADER: UINT8 = 0x02;

/// `SIGNATURE_TYPE_MBR`.
pub const SIGNATURE_TYPE_MBR: UINT8 = 0x01;
/// `SIGNATURE_TYPE_GUID`.
pub const SIGNATURE_TYPE_GUID: UINT8 = 0x02;

/// `MEDIA_CDROM_DP`.
pub const MEDIA_CDROM_DP: UINT8 = 0x02;
/// `MEDIA_VENDOR_DP` (the node is a [`VendorDevicePath`]).
pub const MEDIA_VENDOR_DP: UINT8 = 0x03;
/// `MEDIA_FILEPATH_DP`.
pub const MEDIA_FILEPATH_DP: UINT8 = 0x04;
/// `SIZE_OF_FILEPATH_DEVICE_PATH`: the size of a file path node without its name.
pub const SIZE_OF_FILEPATH_DEVICE_PATH: UINTN = offset_of!(FilepathDevicePath, PathName);
/// `MEDIA_PROTOCOL_DP`.
pub const MEDIA_PROTOCOL_DP: UINT8 = 0x05;

/// `BBS_DEVICE_PATH`.
pub const BBS_DEVICE_PATH: UINT8 = 0x05;
/// `BBS_BBS_DP`.
pub const BBS_BBS_DP: UINT8 = 0x01;

/// `BBS_TYPE_FLOPPY`.
pub const BBS_TYPE_FLOPPY: UINT16 = 0x01;
/// `BBS_TYPE_HARDDRIVE`.
pub const BBS_TYPE_HARDDRIVE: UINT16 = 0x02;
/// `BBS_TYPE_CDROM`.
pub const BBS_TYPE_CDROM: UINT16 = 0x03;
/// `BBS_TYPE_PCMCIA`.
pub const BBS_TYPE_PCMCIA: UINT16 = 0x04;
/// `BBS_TYPE_USB`.
pub const BBS_TYPE_USB: UINT16 = 0x05;
/// `BBS_TYPE_EMBEDDED_NETWORK`.
pub const BBS_TYPE_EMBEDDED_NETWORK: UINT16 = 0x06;
/// `BBS_TYPE_DEV`.
pub const BBS_TYPE_DEV: UINT16 = 0x80;
/// `BBS_TYPE_UNKNOWN`.
pub const BBS_TYPE_UNKNOWN: UINT16 = 0xFF;

/// `EFI_LOADED_IMAGE_DEVICE_PATH_PROTOCOL_GUID`.
pub const EFI_LOADED_IMAGE_DEVICE_PATH_PROTOCOL_GUID: EfiGuid = EfiGuid::new(
    0xbc62157e,
    0x3e33,
    0x4fec,
    [0x99, 0x20, 0x2d, 0x3b, 0x36, 0xd7, 0x50, 0xdf],
);

/// `EFI_DEVICE_PATH_TO_TEXT_PROTOCOL_GUID`.
pub const EFI_DEVICE_PATH_TO_TEXT_PROTOCOL_GUID: EfiGuid = EfiGuid::new(
    0x8b843e20,
    0x8132,
    0x4852,
    [0x90, 0xcc, 0x55, 0x1a, 0x4e, 0x4a, 0x7f, 0x1c],
);

/// `EFI_DEVICE_PATH`: the header of every node.
#[repr(C)]
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct EfiDevicePath {
    /// Node type (`HARDWARE_DEVICE_PATH`, ...), with `EFI_DP_TYPE_UNPACKED` in the top bit.
    pub Type: UINT8,
    /// Node sub-type.
    pub SubType: UINT8,
    /// Node length in bytes, little-endian, header included.
    pub Length: [UINT8; 2],
}

/// `PCI_DEVICE_PATH`.
#[repr(C)]
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct PciDevicePath {
    /// Header.
    pub Header: EfiDevicePath,
    /// PCI function.
    pub Function: UINT8,
    /// PCI device.
    pub Device: UINT8,
}

/// `PCCARD_DEVICE_PATH`.
#[repr(C)]
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct PccardDevicePath {
    /// Header.
    pub Header: EfiDevicePath,
    /// Function number.
    pub FunctionNumber: UINT8,
}

/// `MEMMAP_DEVICE_PATH`.
#[repr(C)]
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct MemmapDevicePath {
    /// Header.
    pub Header: EfiDevicePath,
    /// Memory type.
    pub MemoryType: UINT32,
    /// First address.
    pub StartingAddress: EfiPhysicalAddress,
    /// Last address.
    pub EndingAddress: EfiPhysicalAddress,
}

/// `VENDOR_DEVICE_PATH`.
#[repr(C)]
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct VendorDevicePath {
    /// Header.
    pub Header: EfiDevicePath,
    /// Vendor GUID (vendor data follows the node).
    pub Guid: EfiGuid,
}

/// `UNKNOWN_DEVICE_VENDOR_DEVICE_PATH` (C tag `_UKNOWN_DEVICE_VENDOR_DP`).
#[repr(C)]
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct UnknownDeviceVendorDevicePath {
    /// The vendor node.
    pub DevicePath: VendorDevicePath,
    /// Legacy drive letter.
    pub LegacyDriveLetter: UINT8,
}

/// `CONTROLLER_DEVICE_PATH`.
#[repr(C)]
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct ControllerDevicePath {
    /// Header.
    pub Header: EfiDevicePath,
    /// Controller number.
    pub Controller: UINT32,
}

/// `ACPI_HID_DEVICE_PATH`.
#[repr(C)]
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct AcpiHidDevicePath {
    /// Header.
    pub Header: EfiDevicePath,
    /// `_HID`, see [`eisa_pnp_id`].
    pub HID: UINT32,
    /// `_UID`.
    pub UID: UINT32,
}

/// `ACPI_EXTENDED_HID_DEVICE_PATH`.
#[repr(C)]
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct AcpiExtendedHidDevicePath {
    /// Header.
    pub Header: EfiDevicePath,
    /// `_HID`.
    pub HID: UINT32,
    /// `_UID`.
    pub UID: UINT32,
    /// `_CID`.
    pub CID: UINT32,
}

/// `ATAPI_DEVICE_PATH`.
#[repr(C)]
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct AtapiDevicePath {
    /// Header.
    pub Header: EfiDevicePath,
    /// Primary (0) or secondary (1) channel.
    pub PrimarySecondary: UINT8,
    /// Master (0) or slave (1).
    pub SlaveMaster: UINT8,
    /// Logical unit number.
    pub Lun: UINT16,
}

/// `SCSI_DEVICE_PATH`.
#[repr(C)]
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct ScsiDevicePath {
    /// Header.
    pub Header: EfiDevicePath,
    /// Target.
    pub Pun: UINT16,
    /// Logical unit.
    pub Lun: UINT16,
}

/// `FIBRECHANNEL_DEVICE_PATH`.
#[repr(C)]
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct FibrechannelDevicePath {
    /// Header.
    pub Header: EfiDevicePath,
    /// Reserved.
    pub Reserved: UINT32,
    /// World wide name.
    pub WWN: UINT64,
    /// Logical unit.
    pub Lun: UINT64,
}

/// `F1394_DEVICE_PATH`.
#[repr(C)]
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct F1394DevicePath {
    /// Header.
    pub Header: EfiDevicePath,
    /// Reserved.
    pub Reserved: UINT32,
    /// 1394 GUID.
    pub Guid: UINT64,
}

/// `USB_DEVICE_PATH`.
#[repr(C)]
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct UsbDevicePath {
    /// Header.
    pub Header: EfiDevicePath,
    /// Parent port number.
    pub ParentPortNumber: UINT8,
    /// Interface number.
    pub InterfaceNumber: UINT8,
}

/// `USB_CLASS_DEVICE_PATH`.
#[repr(C)]
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct UsbClassDevicePath {
    /// Header.
    pub Header: EfiDevicePath,
    /// Vendor id.
    pub VendorId: UINT16,
    /// Product id.
    pub ProductId: UINT16,
    /// Device class.
    pub DeviceClass: UINT8,
    /// Device sub-class.
    pub DeviceSubClass: UINT8,
    /// Device protocol.
    pub DeviceProtocol: UINT8,
}

/// `I2O_DEVICE_PATH`.
#[repr(C)]
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct I2oDevicePath {
    /// Header.
    pub Header: EfiDevicePath,
    /// Target id.
    pub Tid: UINT32,
}

/// `MAC_ADDR_DEVICE_PATH`.
#[repr(C)]
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct MacAddrDevicePath {
    /// Header.
    pub Header: EfiDevicePath,
    /// MAC address, zero-padded to 32 bytes.
    pub MacAddress: EfiMacAddress,
    /// Interface type (RFC 3232).
    pub IfType: UINT8,
}

/// `IPv4_DEVICE_PATH`.
#[repr(C)]
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct Ipv4DevicePath {
    /// Header.
    pub Header: EfiDevicePath,
    /// Local address.
    pub LocalIpAddress: EfiIpv4Address,
    /// Remote address.
    pub RemoteIpAddress: EfiIpv4Address,
    /// Local port.
    pub LocalPort: UINT16,
    /// Remote port.
    pub RemotePort: UINT16,
    /// IP protocol number.
    pub Protocol: UINT16,
    /// Static (1) or DHCP (0) address.
    pub StaticIpAddress: BOOLEAN,
}

/// `IPv6_DEVICE_PATH`.
#[repr(C)]
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct Ipv6DevicePath {
    /// Header.
    pub Header: EfiDevicePath,
    /// Local address.
    pub LocalIpAddress: EfiIpv6Address,
    /// Remote address.
    pub RemoteIpAddress: EfiIpv6Address,
    /// Local port.
    pub LocalPort: UINT16,
    /// Remote port.
    pub RemotePort: UINT16,
    /// IP protocol number.
    pub Protocol: UINT16,
    /// Static (1) or DHCP (0) address.
    pub StaticIpAddress: BOOLEAN,
}

/// `INFINIBAND_DEVICE_PATH`.
#[repr(C)]
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct InfinibandDevicePath {
    /// Header.
    pub Header: EfiDevicePath,
    /// `INFINIBAND_RESOURCE_FLAG_*` bits.
    pub ResourceFlags: UINT32,
    /// Port GID.
    pub PortGid: [UINT8; 16],
    /// Service id.
    pub ServiceId: UINT64,
    /// Target port id.
    pub TargetPortId: UINT64,
    /// Device id.
    pub DeviceId: UINT64,
}

/// `UART_DEVICE_PATH`.
#[repr(C)]
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct UartDevicePath {
    /// Header.
    pub Header: EfiDevicePath,
    /// Reserved.
    pub Reserved: UINT32,
    /// Baud rate (0 for the default).
    pub BaudRate: UINT64,
    /// Data bits (0 for the default).
    pub DataBits: UINT8,
    /// Parity (0 for the default).
    pub Parity: UINT8,
    /// Stop bits (0 for the default).
    pub StopBits: UINT8,
}

/// `SATA_DEVICE_PATH`.
#[repr(C)]
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct SataDevicePath {
    /// Header.
    pub Header: EfiDevicePath,
    /// HBA port number.
    pub HBAPortNumber: UINT16,
    /// Port multiplier port number.
    pub PortMultiplierPortNumber: UINT16,
    /// Logical unit.
    pub Lun: UINT16,
}

/// `HARDDRIVE_DEVICE_PATH`.
#[repr(C)]
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct HarddriveDevicePath {
    /// Header.
    pub Header: EfiDevicePath,
    /// Partition number (1-based, 0 for the whole disk).
    pub PartitionNumber: UINT32,
    /// First LBA of the partition.
    pub PartitionStart: UINT64,
    /// Size of the partition in blocks.
    pub PartitionSize: UINT64,
    /// MBR disk signature or partition GUID.
    pub Signature: [UINT8; 16],
    /// `MBR_TYPE_*`.
    pub MBRType: UINT8,
    /// `SIGNATURE_TYPE_*`.
    pub SignatureType: UINT8,
}

/// `CDROM_DEVICE_PATH`.
#[repr(C)]
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct CdromDevicePath {
    /// Header.
    pub Header: EfiDevicePath,
    /// Boot catalog entry.
    pub BootEntry: UINT32,
    /// First LBA of the partition.
    pub PartitionStart: UINT64,
    /// Size of the partition in blocks.
    pub PartitionSize: UINT64,
}

/// `FILEPATH_DEVICE_PATH`: the name is `CHAR16` data running past the declared element.
#[repr(C)]
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct FilepathDevicePath {
    /// Header.
    pub Header: EfiDevicePath,
    /// NUL-terminated UCS-2 path name (variable length).
    pub PathName: [CHAR16; 1],
}

/// `MEDIA_PROTOCOL_DEVICE_PATH`.
#[repr(C)]
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct MediaProtocolDevicePath {
    /// Header.
    pub Header: EfiDevicePath,
    /// Protocol GUID.
    pub Protocol: EfiGuid,
}

/// `BBS_BBS_DEVICE_PATH`: the string is `CHAR8` data running past the declared element.
#[repr(C)]
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct BbsBbsDevicePath {
    /// Header.
    pub Header: EfiDevicePath,
    /// `BBS_TYPE_*`.
    pub DeviceType: UINT16,
    /// Status flags.
    pub StatusFlag: UINT16,
    /// NUL-terminated description (variable length).
    pub String: [CHAR8; 1],
}

/// `EFI_DEV_PATH`: any node, by value.
#[repr(C)]
#[derive(Clone, Copy)]
pub union EfiDevPath {
    /// `DevPath`.
    pub DevPath: EfiDevicePath,
    /// `Pci`.
    pub Pci: PciDevicePath,
    /// `PcCard`.
    pub PcCard: PccardDevicePath,
    /// `MemMap`.
    pub MemMap: MemmapDevicePath,
    /// `Vendor`.
    pub Vendor: VendorDevicePath,
    /// `UnknownVendor`.
    pub UnknownVendor: UnknownDeviceVendorDevicePath,
    /// `Controller`.
    pub Controller: ControllerDevicePath,
    /// `Acpi`.
    pub Acpi: AcpiHidDevicePath,
    /// `Atapi`.
    pub Atapi: AtapiDevicePath,
    /// `Scsi`.
    pub Scsi: ScsiDevicePath,
    /// `FibreChannel`.
    pub FibreChannel: FibrechannelDevicePath,
    /// `F1394`.
    pub F1394: F1394DevicePath,
    /// `Usb`.
    pub Usb: UsbDevicePath,
    /// `UsbClass`.
    pub UsbClass: UsbClassDevicePath,
    /// `I2O`.
    pub I2O: I2oDevicePath,
    /// `MacAddr`.
    pub MacAddr: MacAddrDevicePath,
    /// `Ipv4`.
    pub Ipv4: Ipv4DevicePath,
    /// `Ipv6`.
    pub Ipv6: Ipv6DevicePath,
    /// `InfiniBand`.
    pub InfiniBand: InfinibandDevicePath,
    /// `Uart`.
    pub Uart: UartDevicePath,
    /// `HardDrive`.
    pub HardDrive: HarddriveDevicePath,
    /// `CD`.
    pub CD: CdromDevicePath,
    /// `FilePath`.
    pub FilePath: FilepathDevicePath,
    /// `MediaProtocol`.
    pub MediaProtocol: MediaProtocolDevicePath,
    /// `Bbs`.
    pub Bbs: BbsBbsDevicePath,
}

/// `EFI_DEV_PATH_PTR`: any node, by pointer.
#[repr(C)]
#[derive(Clone, Copy)]
pub union EfiDevPathPtr {
    /// `DevPath`.
    pub DevPath: *mut EfiDevicePath,
    /// `Pci`.
    pub Pci: *mut PciDevicePath,
    /// `PcCard`.
    pub PcCard: *mut PccardDevicePath,
    /// `MemMap`.
    pub MemMap: *mut MemmapDevicePath,
    /// `Vendor`.
    pub Vendor: *mut VendorDevicePath,
    /// `UnknownVendor`.
    pub UnknownVendor: *mut UnknownDeviceVendorDevicePath,
    /// `Controller`.
    pub Controller: *mut ControllerDevicePath,
    /// `Acpi`.
    pub Acpi: *mut AcpiHidDevicePath,
    /// `ExtendedAcpi`.
    pub ExtendedAcpi: *mut AcpiExtendedHidDevicePath,
    /// `Atapi`.
    pub Atapi: *mut AtapiDevicePath,
    /// `Scsi`.
    pub Scsi: *mut ScsiDevicePath,
    /// `FibreChannel`.
    pub FibreChannel: *mut FibrechannelDevicePath,
    /// `F1394`.
    pub F1394: *mut F1394DevicePath,
    /// `Usb`.
    pub Usb: *mut UsbDevicePath,
    /// `UsbClass`.
    pub UsbClass: *mut UsbClassDevicePath,
    /// `I2O`.
    pub I2O: *mut I2oDevicePath,
    /// `MacAddr`.
    pub MacAddr: *mut MacAddrDevicePath,
    /// `Ipv4`.
    pub Ipv4: *mut Ipv4DevicePath,
    /// `Ipv6`.
    pub Ipv6: *mut Ipv6DevicePath,
    /// `InfiniBand`.
    pub InfiniBand: *mut InfinibandDevicePath,
    /// `Uart`.
    pub Uart: *mut UartDevicePath,
    /// `HardDrive`.
    pub HardDrive: *mut HarddriveDevicePath,
    /// `FilePath`.
    pub FilePath: *mut FilepathDevicePath,
    /// `MediaProtocol`.
    pub MediaProtocol: *mut MediaProtocolDevicePath,
    /// `CD`.
    pub CD: *mut CdromDevicePath,
    /// `Bbs`.
    pub Bbs: *mut BbsBbsDevicePath,
}

/// `EFI_DEVICE_PATH_TO_TEXT_NODE`.
pub type EfiDevicePathToTextNode = unsafe extern "efiapi" fn(
    This: *mut EfiDevicePath,
    DisplayOnly: BOOLEAN,
    AllowShortCuts: BOOLEAN,
) -> *mut CHAR16;

/// `EFI_DEVICE_PATH_TO_TEXT_PATH`.
pub type EfiDevicePathToTextPath = unsafe extern "efiapi" fn(
    This: *mut EfiDevicePath,
    DisplayOnly: BOOLEAN,
    AllowShortCuts: BOOLEAN,
) -> *mut CHAR16;

/// `EFI_DEVICE_PATH_TO_TEXT_PROTOCOL`.
#[repr(C)]
#[derive(Clone, Copy)]
pub struct EfiDevicePathToTextProtocol {
    /// `ConvertDeviceNodeToText`.
    pub ConvertDeviceNodeToText: EfiDevicePathToTextNode,
    /// `ConvertDevicePathToText`.
    pub ConvertDevicePathToText: EfiDevicePathToTextPath,
}

/// `EISA_ID(_Name, _Num)`: the compressed EISA id `_Name` with the number `_Num` in the
/// upper 16 bits.
pub const fn eisa_id(name: UINT32, num: UINT32) -> UINT32 {
    name | (num << 16)
}

/// `EISA_PNP_ID(_PNPId)`.
pub const fn eisa_pnp_id(pnp_id: UINT32) -> UINT32 {
    eisa_id(PNP_EISA_ID_CONST, pnp_id)
}

/// `EFI_PNP_ID(_PNPId)`.
pub const fn efi_pnp_id(pnp_id: UINT32) -> UINT32 {
    eisa_id(PNP_EISA_ID_CONST, pnp_id)
}

/// `EISA_ID_TO_NUM(_Id)`.
pub const fn eisa_id_to_num(id: UINT32) -> UINT32 {
    id >> 16
}

/// `DevicePathType(a)`: the node type with the "unpacked" bit masked off.
///
/// # Safety
/// `a` must point at a valid, readable device path node header.
pub unsafe fn device_path_type(a: *const EfiDevicePath) -> UINT8 {
    // SAFETY: the caller guarantees `a` points at a readable node header.
    unsafe { (*a).Type & EFI_DP_TYPE_MASK }
}

/// `DevicePathSubType(a)`.
///
/// # Safety
/// `a` must point at a valid, readable device path node header.
pub unsafe fn device_path_sub_type(a: *const EfiDevicePath) -> UINT8 {
    // SAFETY: the caller guarantees `a` points at a readable node header.
    unsafe { (*a).SubType }
}

/// `DevicePathNodeLength(a)`: the 16-bit little-endian node length.
///
/// # Safety
/// `a` must point at a valid, readable device path node header.
pub unsafe fn device_path_node_length(a: *const EfiDevicePath) -> UINTN {
    // SAFETY: the caller guarantees `a` points at a readable node header.
    let len = unsafe { (*a).Length };
    (len[0] as UINTN) | ((len[1] as UINTN) << 8)
}

/// `NextDevicePathNode(a)`: the node that follows `a` in the path.
///
/// # Safety
/// `a` must point at a valid, readable device path node header, and the node it describes
/// must be followed by another node (or the end of the buffer that holds the path), so the
/// returned pointer stays within, or one past, the same allocation.
pub unsafe fn next_device_path_node(a: *const EfiDevicePath) -> *mut EfiDevicePath {
    // SAFETY: the caller guarantees `a` points at a readable node header.
    let len = unsafe { device_path_node_length(a) };
    // SAFETY: the caller guarantees the node of `len` bytes lies within one allocation.
    unsafe { a.cast::<UINT8>().add(len) as *mut EfiDevicePath }
}

/// `IsDevicePathType(a, t)`.
///
/// # Safety
/// `a` must point at a valid, readable device path node header.
pub unsafe fn is_device_path_type(a: *const EfiDevicePath, t: UINT8) -> bool {
    // SAFETY: the caller guarantees `a` points at a readable node header.
    unsafe { device_path_type(a) == t }
}

/// `IsDevicePathEndType(a)`.
///
/// # Safety
/// `a` must point at a valid, readable device path node header.
pub unsafe fn is_device_path_end_type(a: *const EfiDevicePath) -> bool {
    // SAFETY: the caller guarantees `a` points at a readable node header.
    unsafe { is_device_path_type(a, END_DEVICE_PATH_TYPE) }
}

/// `IsDevicePathEndSubType(a)`.
///
/// # Safety
/// `a` must point at a valid, readable device path node header.
pub unsafe fn is_device_path_end_sub_type(a: *const EfiDevicePath) -> bool {
    // SAFETY: the caller guarantees `a` points at a readable node header.
    unsafe { (*a).SubType == END_ENTIRE_DEVICE_PATH_SUBTYPE }
}

/// `IsDevicePathEnd(a)`: the end of the entire device path.
///
/// # Safety
/// `a` must point at a valid, readable device path node header.
pub unsafe fn is_device_path_end(a: *const EfiDevicePath) -> bool {
    // SAFETY: the caller guarantees `a` points at a readable node header.
    unsafe { is_device_path_end_type(a) && is_device_path_end_sub_type(a) }
}

/// `IsDevicePathUnpacked(a)`: the node has the "unpacked" bit set.
///
/// # Safety
/// `a` must point at a valid, readable device path node header.
pub unsafe fn is_device_path_unpacked(a: *const EfiDevicePath) -> bool {
    // SAFETY: the caller guarantees `a` points at a readable node header.
    unsafe { ((*a).Type & EFI_DP_TYPE_UNPACKED) != 0 }
}

/// `SetDevicePathNodeLength(a, l)`: store `l` as the 16-bit little-endian node length.
///
/// # Safety
/// `a` must point at a valid, writable device path node header.
pub unsafe fn set_device_path_node_length(a: *mut EfiDevicePath, l: UINTN) {
    // SAFETY: the caller guarantees `a` points at a writable node header.
    unsafe { (*a).Length = [l as UINT8, (l >> 8) as UINT8] };
}

/// `SetDevicePathEndNode(a)`: make `a` an "end entire device path" node.
///
/// # Safety
/// `a` must point at a valid, writable device path node header.
pub unsafe fn set_device_path_end_node(a: *mut EfiDevicePath) {
    // SAFETY: the caller guarantees `a` points at a writable node header.
    unsafe {
        (*a).Type = END_DEVICE_PATH_TYPE;
        (*a).SubType = END_ENTIRE_DEVICE_PATH_SUBTYPE;
        (*a).Length = [size_of::<EfiDevicePath>() as UINT8, 0];
    }
}

const _: () = assert!(size_of::<EfiDevicePath>() == 4);
const _: () = assert!(size_of::<PciDevicePath>() == 6);
const _: () = assert!(size_of::<VendorDevicePath>() == 20);
const _: () = assert!(size_of::<AcpiHidDevicePath>() == 12);
const _: () = assert!(size_of::<MacAddrDevicePath>() == 37);
const _: () = assert!(size_of::<UartDevicePath>() == 24);
const _: () = assert!(size_of::<HarddriveDevicePath>() == 42 + 6);
const _: () = assert!(SIZE_OF_FILEPATH_DEVICE_PATH == 4);
/* </CODE> */

/* <TESTS> */
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn walk_a_small_path() {
        // ACPI node (12 bytes), file path node (4 + 4 bytes), end node (4 bytes).
        let buf: [u8; 24] = [
            ACPI_DEVICE_PATH,
            ACPI_DP,
            12,
            0,
            0,
            0,
            0,
            0,
            0,
            0,
            0,
            0, //
            MEDIA_DEVICE_PATH,
            MEDIA_FILEPATH_DP,
            8,
            0,
            b'a',
            0,
            0,
            0, //
            END_DEVICE_PATH_TYPE,
            END_ENTIRE_DEVICE_PATH_SUBTYPE,
            4,
            0,
        ];
        let mut n = buf.as_ptr().cast::<EfiDevicePath>();
        let mut types = [0u8; 3];
        let mut i = 0;
        // SAFETY: `buf` holds three well-formed nodes ending in an end node; `n` stays inside it.
        unsafe {
            while !is_device_path_end(n) {
                types[i] = device_path_type(n);
                i += 1;
                n = next_device_path_node(n);
            }
            assert_eq!(n as usize - buf.as_ptr() as usize, 20);
            assert_eq!(device_path_node_length(n), 4);
        }
        assert_eq!(&types[..2], &[ACPI_DEVICE_PATH, MEDIA_DEVICE_PATH]);
    }

    #[test]
    fn node_length_is_little_endian() {
        let mut node = EfiDevicePath {
            Type: 0x84,
            SubType: 1,
            Length: [0, 0],
        };
        // SAFETY: `node` is a live, writable, readable header.
        unsafe {
            set_device_path_node_length(&mut node, 0x0102);
            assert_eq!(node.Length, [0x02, 0x01]);
            assert_eq!(device_path_node_length(&node), 0x0102);
            assert!(is_device_path_unpacked(&node));
            assert_eq!(device_path_type(&node), 4);
            assert_eq!(device_path_sub_type(&node), 1);
            set_device_path_end_node(&mut node);
            assert!(is_device_path_end(&node));
            assert_eq!(device_path_node_length(&node), 4);
        }
    }

    #[test]
    fn pnp_ids() {
        assert_eq!(efi_pnp_id(0x0a03), 0x0a03_41d0);
        assert_eq!(eisa_pnp_id(0x0a03), efi_pnp_id(0x0a03));
        assert_eq!(eisa_id_to_num(0x0a03_41d0), 0x0a03);
    }
}
/* </TESTS> */
