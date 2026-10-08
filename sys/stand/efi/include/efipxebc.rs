/* $FreeBSD: head/sys/boot/efi/include/efipxebc.h 163898 2006-11-02 02:42:48Z marcel $ */
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

    efipxebc.h

Abstract:

    EFI PXE Base Code Protocol



Revision History

--*/
/* </LICENSES> */

/* <CODE> */
//! EFI PXE base code protocol.
//!
//! Upstream: sys/stand/efi/include/efipxebc.h @ 3ce1f3f79392
//!
//! ## Deviations
//! - Type names are the C typedefs in CamelCase (`EFI_PXE_BASE_CODE` -> `EfiPxeBaseCode`,
//!   `EFI_IP_ADDRESS` -> `EfiIpAddress`, ...); fields are verbatim.
//! - `EFI_IP_ADDRESS` and `EFI_PXE_BASE_CODE_PACKET` are `#[repr(C)]` unions; reading a field
//!   is unsafe.
//! - The anonymous union `u` inside `EFI_PXE_BASE_CODE_ICMP_ERROR` and its anonymous `Echo`
//!   struct get names: `EfiPxeBaseCodeIcmpErrorU` and `EfiPxeBaseCodeIcmpErrorEcho`.
//! - `EFI_PXE_BASE_CODE_TFTP_OPCODE`, `EFI_PXE_BASE_CODE_FUNCTION` and
//!   `EFI_PXE_BASE_CODE_CALLBACK_STATUS` are `u32` aliases plus one constant per enumerator.
//! - The `Dhcpv6` packet member is commented out in the C ("TBD in EFI v1.1") and so is here
//!   (not ported: there is nothing to port).
//! - `SrvList[1]` keeps its one declared element.

#![allow(non_snake_case, non_upper_case_globals, non_camel_case_types)]
// EFI names are the specification's (BootpOpcode, EFI_PXE_BASE_CODE_TFTP_FIRST, ...), kept verbatim for grep-ability

use core::ffi::c_void;

use super::amd64::efibind::{UINT8, UINT16, UINT32, UINT64, UINTN};
use super::efidef::{
    BOOLEAN, CHAR8, EfiGuid, EfiIpv4Address, EfiIpv6Address, EfiMacAddress, EfiStatus,
};

/// `EFI_PXE_BASE_CODE_PROTOCOL`.
pub const EFI_PXE_BASE_CODE_PROTOCOL: EfiGuid = EfiGuid::new(
    0x03c4e603,
    0xac28,
    0x11d3,
    [0x9a, 0x2d, 0x00, 0x90, 0x27, 0x3f, 0xc1, 0x4d],
);

/// `DEFAULT_TTL`.
pub const DEFAULT_TTL: UINT8 = 8;
/// `DEFAULT_ToS`.
pub const DEFAULT_ToS: UINT8 = 0;

/// `EFI_PXE_BASE_CODE_MAX_IPCNT`.
pub const EFI_PXE_BASE_CODE_MAX_IPCNT: UINTN = 8;
/// `EFI_PXE_BASE_CODE_MAX_ARP_ENTRIES`.
pub const EFI_PXE_BASE_CODE_MAX_ARP_ENTRIES: UINTN = 8;
/// `EFI_PXE_BASE_CODE_MAX_ROUTE_ENTRIES`.
pub const EFI_PXE_BASE_CODE_MAX_ROUTE_ENTRIES: UINTN = 8;

/// IP filter: station IP.
pub const EFI_PXE_BASE_CODE_IP_FILTER_STATION_IP: UINT8 = 0x0001;
/// IP filter: broadcast.
pub const EFI_PXE_BASE_CODE_IP_FILTER_BROADCAST: UINT8 = 0x0002;
/// IP filter: promiscuous.
pub const EFI_PXE_BASE_CODE_IP_FILTER_PROMISCUOUS: UINT8 = 0x0004;
/// IP filter: promiscuous multicast.
pub const EFI_PXE_BASE_CODE_IP_FILTER_PROMISCUOUS_MULTICAST: UINT8 = 0x0008;

/// UDP operation flag: any source IP.
pub const EFI_PXE_BASE_CODE_UDP_OPFLAGS_ANY_SRC_IP: UINT16 = 0x0001;
/// UDP operation flag: any source port.
pub const EFI_PXE_BASE_CODE_UDP_OPFLAGS_ANY_SRC_PORT: UINT16 = 0x0002;
/// UDP operation flag: any destination IP.
pub const EFI_PXE_BASE_CODE_UDP_OPFLAGS_ANY_DEST_IP: UINT16 = 0x0004;
/// UDP operation flag: any destination port.
pub const EFI_PXE_BASE_CODE_UDP_OPFLAGS_ANY_DEST_PORT: UINT16 = 0x0008;
/// UDP operation flag: use the IP filter.
pub const EFI_PXE_BASE_CODE_UDP_OPFLAGS_USE_FILTER: UINT16 = 0x0010;
/// UDP operation flag: the packet may be fragmented.
pub const EFI_PXE_BASE_CODE_UDP_OPFLAGS_MAY_FRAGMENT: UINT16 = 0x0020;

/// `Discover()` boot type: bootstrap.
pub const EFI_PXE_BASE_CODE_BOOT_TYPE_BOOTSTRAP: UINT16 = 0;
/// `Discover()` boot type: Microsoft Windows NT RIS.
pub const EFI_PXE_BASE_CODE_BOOT_TYPE_MS_WINNT_RIS: UINT16 = 1;
/// `Discover()` boot type: Intel LCM.
pub const EFI_PXE_BASE_CODE_BOOT_TYPE_INTEL_LCM: UINT16 = 2;
/// `Discover()` boot type: DOS UNDI.
pub const EFI_PXE_BASE_CODE_BOOT_TYPE_DOSUNDI: UINT16 = 3;
/// `Discover()` boot type: NEC ESMPRO.
pub const EFI_PXE_BASE_CODE_BOOT_TYPE_NEC_ESMPRO: UINT16 = 4;
/// `Discover()` boot type: IBM WSoD.
pub const EFI_PXE_BASE_CODE_BOOT_TYPE_IBM_WSoD: UINT16 = 5;
/// `Discover()` boot type: IBM LCCM.
pub const EFI_PXE_BASE_CODE_BOOT_TYPE_IBM_LCCM: UINT16 = 6;
/// `Discover()` boot type: CA Unicenter TNG.
pub const EFI_PXE_BASE_CODE_BOOT_TYPE_CA_UNICENTER_TNG: UINT16 = 7;
/// `Discover()` boot type: HP OpenView.
pub const EFI_PXE_BASE_CODE_BOOT_TYPE_HP_OPENVIEW: UINT16 = 8;
/// `Discover()` boot type: Altiris 9.
pub const EFI_PXE_BASE_CODE_BOOT_TYPE_ALTIRIS_9: UINT16 = 9;
/// `Discover()` boot type: Altiris 10.
pub const EFI_PXE_BASE_CODE_BOOT_TYPE_ALTIRIS_10: UINT16 = 10;
/// `Discover()` boot type: Altiris 11.
pub const EFI_PXE_BASE_CODE_BOOT_TYPE_ALTIRIS_11: UINT16 = 11;
/// `Discover()` boot type: not used (12).
pub const EFI_PXE_BASE_CODE_BOOT_TYPE_NOT_USED_12: UINT16 = 12;
/// `Discover()` boot type: Red Hat install.
pub const EFI_PXE_BASE_CODE_BOOT_TYPE_REDHAT_INSTALL: UINT16 = 13;
/// `Discover()` boot type: Red Hat boot.
pub const EFI_PXE_BASE_CODE_BOOT_TYPE_REDHAT_BOOT: UINT16 = 14;
/// `Discover()` boot type: Rembo.
pub const EFI_PXE_BASE_CODE_BOOT_TYPE_REMBO: UINT16 = 15;
/// `Discover()` boot type: BEOboot.
pub const EFI_PXE_BASE_CODE_BOOT_TYPE_BEOBOOT: UINT16 = 16;
/// `Discover()` boot type: PXE test (17 through 65534 are reserved or for vendor use).
pub const EFI_PXE_BASE_CODE_BOOT_TYPE_PXETEST: UINT16 = 65535;

/// `EFI_PXE_BASE_CODE_BOOT_LAYER_MASK`.
pub const EFI_PXE_BASE_CODE_BOOT_LAYER_MASK: UINT16 = 0x7FFF;
/// `EFI_PXE_BASE_CODE_BOOT_LAYER_INITIAL`.
pub const EFI_PXE_BASE_CODE_BOOT_LAYER_INITIAL: UINT16 = 0x0000;
/// `EFI_PXE_BASE_CODE_BOOT_LAYER_CREDENTIALS`.
pub const EFI_PXE_BASE_CODE_BOOT_LAYER_CREDENTIALS: UINT16 = 0x8000;

/// `EFI_PXE_BASE_CODE_TFTP_OPCODE`.
pub type EfiPxeBaseCodeTftpOpcode = u32;
/// `EFI_PXE_BASE_CODE_TFTP_FIRST`.
pub const EFI_PXE_BASE_CODE_TFTP_FIRST: EfiPxeBaseCodeTftpOpcode = 0;
/// `EFI_PXE_BASE_CODE_TFTP_GET_FILE_SIZE`.
pub const EFI_PXE_BASE_CODE_TFTP_GET_FILE_SIZE: EfiPxeBaseCodeTftpOpcode = 1;
/// `EFI_PXE_BASE_CODE_TFTP_READ_FILE`.
pub const EFI_PXE_BASE_CODE_TFTP_READ_FILE: EfiPxeBaseCodeTftpOpcode = 2;
/// `EFI_PXE_BASE_CODE_TFTP_WRITE_FILE`.
pub const EFI_PXE_BASE_CODE_TFTP_WRITE_FILE: EfiPxeBaseCodeTftpOpcode = 3;
/// `EFI_PXE_BASE_CODE_TFTP_READ_DIRECTORY`.
pub const EFI_PXE_BASE_CODE_TFTP_READ_DIRECTORY: EfiPxeBaseCodeTftpOpcode = 4;
/// `EFI_PXE_BASE_CODE_MTFTP_GET_FILE_SIZE`.
pub const EFI_PXE_BASE_CODE_MTFTP_GET_FILE_SIZE: EfiPxeBaseCodeTftpOpcode = 5;
/// `EFI_PXE_BASE_CODE_MTFTP_READ_FILE`.
pub const EFI_PXE_BASE_CODE_MTFTP_READ_FILE: EfiPxeBaseCodeTftpOpcode = 6;
/// `EFI_PXE_BASE_CODE_MTFTP_READ_DIRECTORY`.
pub const EFI_PXE_BASE_CODE_MTFTP_READ_DIRECTORY: EfiPxeBaseCodeTftpOpcode = 7;
/// `EFI_PXE_BASE_CODE_MTFTP_LAST`.
pub const EFI_PXE_BASE_CODE_MTFTP_LAST: EfiPxeBaseCodeTftpOpcode = 8;

/// `EFI_PXE_BASE_CODE_INTERFACE_REVISION`.
pub const EFI_PXE_BASE_CODE_INTERFACE_REVISION: UINT64 = 0x00010000;

/// `EFI_PXE_BASE_CODE_CALLBACK_PROTOCOL`.
pub const EFI_PXE_BASE_CODE_CALLBACK_PROTOCOL: EfiGuid = EfiGuid::new(
    0x245dca21,
    0xfb7b,
    0x11d3,
    [0x8f, 0x01, 0x00, 0xa0, 0xc9, 0x69, 0x72, 0x3b],
);

/// `EFI_PXE_BASE_CODE_CALLBACK_INTERFACE_REVISION`.
pub const EFI_PXE_BASE_CODE_CALLBACK_INTERFACE_REVISION: UINT64 = 0x00010000;

/// `EFI_PXE_BASE_CODE_FUNCTION`.
pub type EfiPxeBaseCodeFunction = u32;
/// `EFI_PXE_BASE_CODE_FUNCTION_FIRST`.
pub const EFI_PXE_BASE_CODE_FUNCTION_FIRST: EfiPxeBaseCodeFunction = 0;
/// `EFI_PXE_BASE_CODE_FUNCTION_DHCP`.
pub const EFI_PXE_BASE_CODE_FUNCTION_DHCP: EfiPxeBaseCodeFunction = 1;
/// `EFI_PXE_BASE_CODE_FUNCTION_DISCOVER`.
pub const EFI_PXE_BASE_CODE_FUNCTION_DISCOVER: EfiPxeBaseCodeFunction = 2;
/// `EFI_PXE_BASE_CODE_FUNCTION_MTFTP`.
pub const EFI_PXE_BASE_CODE_FUNCTION_MTFTP: EfiPxeBaseCodeFunction = 3;
/// `EFI_PXE_BASE_CODE_FUNCTION_UDP_WRITE`.
pub const EFI_PXE_BASE_CODE_FUNCTION_UDP_WRITE: EfiPxeBaseCodeFunction = 4;
/// `EFI_PXE_BASE_CODE_FUNCTION_UDP_READ`.
pub const EFI_PXE_BASE_CODE_FUNCTION_UDP_READ: EfiPxeBaseCodeFunction = 5;
/// `EFI_PXE_BASE_CODE_FUNCTION_ARP`.
pub const EFI_PXE_BASE_CODE_FUNCTION_ARP: EfiPxeBaseCodeFunction = 6;
/// `EFI_PXE_BASE_CODE_FUNCTION_IGMP`.
pub const EFI_PXE_BASE_CODE_FUNCTION_IGMP: EfiPxeBaseCodeFunction = 7;
/// `EFI_PXE_BASE_CODE_PXE_FUNCTION_LAST`.
pub const EFI_PXE_BASE_CODE_PXE_FUNCTION_LAST: EfiPxeBaseCodeFunction = 8;

/// `EFI_PXE_BASE_CODE_CALLBACK_STATUS`.
pub type EfiPxeBaseCodeCallbackStatus = u32;
/// `EFI_PXE_BASE_CODE_CALLBACK_STATUS_FIRST`.
pub const EFI_PXE_BASE_CODE_CALLBACK_STATUS_FIRST: EfiPxeBaseCodeCallbackStatus = 0;
/// `EFI_PXE_BASE_CODE_CALLBACK_STATUS_CONTINUE`.
pub const EFI_PXE_BASE_CODE_CALLBACK_STATUS_CONTINUE: EfiPxeBaseCodeCallbackStatus = 1;
/// `EFI_PXE_BASE_CODE_CALLBACK_STATUS_ABORT`.
pub const EFI_PXE_BASE_CODE_CALLBACK_STATUS_ABORT: EfiPxeBaseCodeCallbackStatus = 2;
/// `EFI_PXE_BASE_CODE_CALLBACK_STATUS_LAST`.
pub const EFI_PXE_BASE_CODE_CALLBACK_STATUS_LAST: EfiPxeBaseCodeCallbackStatus = 3;

/// `EFI_IP_ADDRESS`: an IPv4 or IPv6 address, as the four-word view, `v4` or `v6`.
#[repr(C)]
#[derive(Clone, Copy)]
pub union EfiIpAddress {
    /// `Addr`.
    pub Addr: [UINT32; 4],
    /// `v4`.
    pub v4: EfiIpv4Address,
    /// `v6`.
    pub v6: EfiIpv6Address,
}

/// `EFI_PXE_BASE_CODE_UDP_PORT`.
pub type EfiPxeBaseCodeUdpPort = UINT16;

/// `EFI_PXE_BASE_CODE_DHCPV4_PACKET`.
#[repr(C)]
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct EfiPxeBaseCodeDhcpv4Packet {
    /// BOOTP opcode.
    pub BootpOpcode: UINT8,
    /// BOOTP hardware type.
    pub BootpHwType: UINT8,
    /// BOOTP hardware address length.
    pub BootpHwAddrLen: UINT8,
    /// BOOTP gateway hops.
    pub BootpGateHops: UINT8,
    /// BOOTP transaction id.
    pub BootpIdent: UINT32,
    /// BOOTP seconds.
    pub BootpSeconds: UINT16,
    /// BOOTP flags.
    pub BootpFlags: UINT16,
    /// Client IP address.
    pub BootpCiAddr: [UINT8; 4],
    /// Your IP address.
    pub BootpYiAddr: [UINT8; 4],
    /// Server IP address.
    pub BootpSiAddr: [UINT8; 4],
    /// Gateway IP address.
    pub BootpGiAddr: [UINT8; 4],
    /// Client hardware address.
    pub BootpHwAddr: [UINT8; 16],
    /// Server host name.
    pub BootpSrvName: [UINT8; 64],
    /// Boot file name.
    pub BootpBootFile: [UINT8; 128],
    /// DHCP magic cookie.
    pub DhcpMagik: UINT32,
    /// DHCP options.
    pub DhcpOptions: [UINT8; 56],
}

/// `EFI_PXE_BASE_CODE_PACKET`.
#[repr(C)]
#[derive(Clone, Copy)]
pub union EfiPxeBaseCodePacket {
    /// `Raw`.
    pub Raw: [UINT8; 1472],
    /// `Dhcpv4`.
    pub Dhcpv4: EfiPxeBaseCodeDhcpv4Packet,
}

/// The anonymous `Echo` struct of the ICMP error union.
#[repr(C)]
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct EfiPxeBaseCodeIcmpErrorEcho {
    /// `Identifier`.
    pub Identifier: UINT16,
    /// `Sequence`.
    pub Sequence: UINT16,
}

/// The anonymous union `u` of `EFI_PXE_BASE_CODE_ICMP_ERROR`.
#[repr(C)]
#[derive(Clone, Copy)]
pub union EfiPxeBaseCodeIcmpErrorU {
    /// `reserved`.
    pub reserved: UINT32,
    /// `Mtu`.
    pub Mtu: UINT32,
    /// `Pointer`.
    pub Pointer: UINT32,
    /// `Echo`.
    pub Echo: EfiPxeBaseCodeIcmpErrorEcho,
}

/// `EFI_PXE_BASE_CODE_ICMP_ERROR`.
#[repr(C)]
#[derive(Clone, Copy)]
pub struct EfiPxeBaseCodeIcmpError {
    /// ICMP type.
    pub Type: UINT8,
    /// ICMP code.
    pub Code: UINT8,
    /// ICMP checksum.
    pub Checksum: UINT16,
    /// Type-dependent word.
    pub u: EfiPxeBaseCodeIcmpErrorU,
    /// Data.
    pub Data: [UINT8; 494],
}

/// `EFI_PXE_BASE_CODE_TFTP_ERROR`.
#[repr(C)]
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct EfiPxeBaseCodeTftpError {
    /// Error code.
    pub ErrorCode: UINT8,
    /// Error string.
    pub ErrorString: [CHAR8; 127],
}

/// `EFI_PXE_BASE_CODE_IP_FILTER`.
#[repr(C)]
#[derive(Clone, Copy)]
pub struct EfiPxeBaseCodeIpFilter {
    /// `EFI_PXE_BASE_CODE_IP_FILTER_*` bits.
    pub Filters: UINT8,
    /// Number of entries of `IpList` in use.
    pub IpCnt: UINT8,
    /// Reserved.
    pub reserved: UINT16,
    /// IP list.
    pub IpList: [EfiIpAddress; EFI_PXE_BASE_CODE_MAX_IPCNT],
}

/// `EFI_PXE_BASE_CODE_ARP_ENTRY`.
#[repr(C)]
#[derive(Clone, Copy)]
pub struct EfiPxeBaseCodeArpEntry {
    /// IP address.
    pub IpAddr: EfiIpAddress,
    /// MAC address.
    pub MacAddr: EfiMacAddress,
}

/// `EFI_PXE_BASE_CODE_ROUTE_ENTRY`.
#[repr(C)]
#[derive(Clone, Copy)]
pub struct EfiPxeBaseCodeRouteEntry {
    /// IP address.
    pub IpAddr: EfiIpAddress,
    /// Subnet mask.
    pub SubnetMask: EfiIpAddress,
    /// Gateway address.
    pub GwAddr: EfiIpAddress,
}

/// `EFI_PXE_BASE_CODE_SRVLIST`.
#[repr(C)]
#[derive(Clone, Copy)]
pub struct EfiPxeBaseCodeSrvlist {
    /// Boot server type.
    pub Type: UINT16,
    /// Accept any response.
    pub AcceptAnyResponse: BOOLEAN,
    /// Reserved.
    pub Reserved: UINT8,
    /// Server address.
    pub IpAddr: EfiIpAddress,
}

/// `EFI_PXE_BASE_CODE_DISCOVER_INFO`.
#[repr(C)]
#[derive(Clone, Copy)]
pub struct EfiPxeBaseCodeDiscoverInfo {
    /// Use multicast.
    pub UseMCast: BOOLEAN,
    /// Use broadcast.
    pub UseBCast: BOOLEAN,
    /// Use unicast.
    pub UseUCast: BOOLEAN,
    /// Only use the server list.
    pub MustUseList: BOOLEAN,
    /// Server multicast address.
    pub ServerMCastIp: EfiIpAddress,
    /// Number of entries of `SrvList`.
    pub IpCnt: UINT16,
    /// Server list (variable length).
    pub SrvList: [EfiPxeBaseCodeSrvlist; 1],
}

/// `EFI_PXE_BASE_CODE_MTFTP_INFO`.
#[repr(C)]
#[derive(Clone, Copy)]
pub struct EfiPxeBaseCodeMtftpInfo {
    /// Multicast IP address.
    pub MCastIp: EfiIpAddress,
    /// Client port.
    pub CPort: EfiPxeBaseCodeUdpPort,
    /// Server port.
    pub SPort: EfiPxeBaseCodeUdpPort,
    /// Listen timeout.
    pub ListenTimeout: UINT16,
    /// Transmit timeout.
    pub TransmitTimeout: UINT16,
}

/// `EFI_PXE_BASE_CODE_MODE`.
#[repr(C)]
#[derive(Clone, Copy)]
pub struct EfiPxeBaseCodeMode {
    /// The protocol has been started.
    pub Started: BOOLEAN,
    /// IPv6 is available.
    pub Ipv6Available: BOOLEAN,
    /// IPv6 is supported.
    pub Ipv6Supported: BOOLEAN,
    /// IPv6 is in use.
    pub UsingIpv6: BOOLEAN,
    /// BIS is supported.
    pub BisSupported: BOOLEAN,
    /// BIS was detected.
    pub BisDetected: BOOLEAN,
    /// Automatic ARP.
    pub AutoArp: BOOLEAN,
    /// Send the GUID in discover packets.
    pub SendGUID: BOOLEAN,
    /// `DhcpDiscover` is valid.
    pub DhcpDiscoverValid: BOOLEAN,
    /// `DhcpAck` was received.
    pub DhcpAckReceived: BOOLEAN,
    /// `ProxyOffer` was received.
    pub ProxyOfferReceived: BOOLEAN,
    /// `PxeDiscover` is valid.
    pub PxeDiscoverValid: BOOLEAN,
    /// `PxeReply` was received.
    pub PxeReplyReceived: BOOLEAN,
    /// `PxeBisReply` was received.
    pub PxeBisReplyReceived: BOOLEAN,
    /// An ICMP error was received.
    pub IcmpErrorReceived: BOOLEAN,
    /// A TFTP error was received.
    pub TftpErrorReceived: BOOLEAN,
    /// Make callbacks.
    pub MakeCallbacks: BOOLEAN,
    /// Time to live.
    pub TTL: UINT8,
    /// Type of service.
    pub ToS: UINT8,
    /// Station IP address.
    pub StationIp: EfiIpAddress,
    /// Subnet mask.
    pub SubnetMask: EfiIpAddress,
    /// DHCP discover packet.
    pub DhcpDiscover: EfiPxeBaseCodePacket,
    /// DHCP ack packet.
    pub DhcpAck: EfiPxeBaseCodePacket,
    /// Proxy offer packet.
    pub ProxyOffer: EfiPxeBaseCodePacket,
    /// PXE discover packet.
    pub PxeDiscover: EfiPxeBaseCodePacket,
    /// PXE reply packet.
    pub PxeReply: EfiPxeBaseCodePacket,
    /// PXE BIS reply packet.
    pub PxeBisReply: EfiPxeBaseCodePacket,
    /// IP filter.
    pub IpFilter: EfiPxeBaseCodeIpFilter,
    /// Entries of `ArpCache` in use.
    pub ArpCacheEntries: UINT32,
    /// ARP cache.
    pub ArpCache: [EfiPxeBaseCodeArpEntry; EFI_PXE_BASE_CODE_MAX_ARP_ENTRIES],
    /// Entries of `RouteTable` in use.
    pub RouteTableEntries: UINT32,
    /// Route table.
    pub RouteTable: [EfiPxeBaseCodeRouteEntry; EFI_PXE_BASE_CODE_MAX_ROUTE_ENTRIES],
    /// Last ICMP error.
    pub IcmpError: EfiPxeBaseCodeIcmpError,
    /// Last TFTP error.
    pub TftpError: EfiPxeBaseCodeTftpError,
}

/// `EFI_PXE_BASE_CODE_START`.
pub type EfiPxeBaseCodeStart =
    unsafe extern "efiapi" fn(This: *mut EfiPxeBaseCode, UseIpv6: BOOLEAN) -> EfiStatus;

/// `EFI_PXE_BASE_CODE_STOP`.
pub type EfiPxeBaseCodeStop = unsafe extern "efiapi" fn(This: *mut EfiPxeBaseCode) -> EfiStatus;

/// `EFI_PXE_BASE_CODE_DHCP`.
pub type EfiPxeBaseCodeDhcp =
    unsafe extern "efiapi" fn(This: *mut EfiPxeBaseCode, SortOffers: BOOLEAN) -> EfiStatus;

/// `EFI_PXE_BASE_CODE_DISCOVER`.
pub type EfiPxeBaseCodeDiscover = unsafe extern "efiapi" fn(
    This: *mut EfiPxeBaseCode,
    Type: UINT16,
    Layer: *mut UINT16,
    UseBis: BOOLEAN,
    Info: *mut EfiPxeBaseCodeDiscoverInfo,
) -> EfiStatus;

/// `EFI_PXE_BASE_CODE_MTFTP`.
pub type EfiPxeBaseCodeMtftp = unsafe extern "efiapi" fn(
    This: *mut EfiPxeBaseCode,
    Operation: EfiPxeBaseCodeTftpOpcode,
    BufferPtr: *mut c_void,
    Overwrite: BOOLEAN,
    BufferSize: *mut UINT64,
    BlockSize: *mut UINTN,
    ServerIp: *mut EfiIpAddress,
    Filename: *mut UINT8,
    Info: *mut EfiPxeBaseCodeMtftpInfo,
    DontUseBuffer: BOOLEAN,
) -> EfiStatus;

/// `EFI_PXE_BASE_CODE_UDP_WRITE`.
pub type EfiPxeBaseCodeUdpWrite = unsafe extern "efiapi" fn(
    This: *mut EfiPxeBaseCode,
    OpFlags: UINT16,
    DestIp: *mut EfiIpAddress,
    DestPort: *mut EfiPxeBaseCodeUdpPort,
    GatewayIp: *mut EfiIpAddress,
    SrcIp: *mut EfiIpAddress,
    SrcPort: *mut EfiPxeBaseCodeUdpPort,
    HeaderSize: *mut UINTN,
    HeaderPtr: *mut c_void,
    BufferSize: *mut UINTN,
    BufferPtr: *mut c_void,
) -> EfiStatus;

/// `EFI_PXE_BASE_CODE_UDP_READ`.
pub type EfiPxeBaseCodeUdpRead = unsafe extern "efiapi" fn(
    This: *mut EfiPxeBaseCode,
    OpFlags: UINT16,
    DestIp: *mut EfiIpAddress,
    DestPort: *mut EfiPxeBaseCodeUdpPort,
    SrcIp: *mut EfiIpAddress,
    SrcPort: *mut EfiPxeBaseCodeUdpPort,
    HeaderSize: *mut UINTN,
    HeaderPtr: *mut c_void,
    BufferSize: *mut UINTN,
    BufferPtr: *mut c_void,
) -> EfiStatus;

/// `EFI_PXE_BASE_CODE_SET_IP_FILTER`.
pub type EfiPxeBaseCodeSetIpFilter = unsafe extern "efiapi" fn(
    This: *mut EfiPxeBaseCode,
    NewFilter: *mut EfiPxeBaseCodeIpFilter,
) -> EfiStatus;

/// `EFI_PXE_BASE_CODE_ARP`.
pub type EfiPxeBaseCodeArp = unsafe extern "efiapi" fn(
    This: *mut EfiPxeBaseCode,
    IpAddr: *mut EfiIpAddress,
    MacAddr: *mut EfiMacAddress,
) -> EfiStatus;

/// `EFI_PXE_BASE_CODE_SET_PARAMETERS`.
pub type EfiPxeBaseCodeSetParameters = unsafe extern "efiapi" fn(
    This: *mut EfiPxeBaseCode,
    NewAutoArp: *mut BOOLEAN,
    NewSendGUID: *mut BOOLEAN,
    NewTTL: *mut UINT8,
    NewToS: *mut UINT8,
    NewMakeCallback: *mut BOOLEAN,
) -> EfiStatus;

/// `EFI_PXE_BASE_CODE_SET_STATION_IP`.
pub type EfiPxeBaseCodeSetStationIp = unsafe extern "efiapi" fn(
    This: *mut EfiPxeBaseCode,
    NewStationIp: *mut EfiIpAddress,
    NewSubnetMask: *mut EfiIpAddress,
) -> EfiStatus;

/// `EFI_PXE_BASE_CODE_SET_PACKETS`.
pub type EfiPxeBaseCodeSetPackets = unsafe extern "efiapi" fn(
    This: *mut EfiPxeBaseCode,
    NewDhcpDiscoverValid: *mut BOOLEAN,
    NewDhcpAckReceived: *mut BOOLEAN,
    NewProxyOfferReceived: *mut BOOLEAN,
    NewPxeDiscoverValid: *mut BOOLEAN,
    NewPxeReplyReceived: *mut BOOLEAN,
    NewPxeBisReplyReceived: *mut BOOLEAN,
    NewDhcpDiscover: *mut EfiPxeBaseCodePacket,
    NewDhcpAck: *mut EfiPxeBaseCodePacket,
    NewProxyOffer: *mut EfiPxeBaseCodePacket,
    NewPxeDiscover: *mut EfiPxeBaseCodePacket,
    NewPxeReply: *mut EfiPxeBaseCodePacket,
    NewPxeBisReply: *mut EfiPxeBaseCodePacket,
) -> EfiStatus;

/// `EFI_PXE_BASE_CODE`: the PXE base code protocol.
#[repr(C)]
#[derive(Clone, Copy)]
pub struct EfiPxeBaseCode {
    /// `EFI_PXE_BASE_CODE_INTERFACE_REVISION`.
    pub Revision: UINT64,
    /// `Start`.
    pub Start: EfiPxeBaseCodeStart,
    /// `Stop`.
    pub Stop: EfiPxeBaseCodeStop,
    /// `Dhcp`.
    pub Dhcp: EfiPxeBaseCodeDhcp,
    /// `Discover`.
    pub Discover: EfiPxeBaseCodeDiscover,
    /// `Mtftp`.
    pub Mtftp: EfiPxeBaseCodeMtftp,
    /// `UdpWrite`.
    pub UdpWrite: EfiPxeBaseCodeUdpWrite,
    /// `UdpRead`.
    pub UdpRead: EfiPxeBaseCodeUdpRead,
    /// `SetIpFilter`.
    pub SetIpFilter: EfiPxeBaseCodeSetIpFilter,
    /// `Arp`.
    pub Arp: EfiPxeBaseCodeArp,
    /// `SetParameters`.
    pub SetParameters: EfiPxeBaseCodeSetParameters,
    /// `SetStationIp`.
    pub SetStationIp: EfiPxeBaseCodeSetStationIp,
    /// `SetPackets`.
    pub SetPackets: EfiPxeBaseCodeSetPackets,
    /// The mode.
    pub Mode: *mut EfiPxeBaseCodeMode,
}

/// `EFI_PXE_CALLBACK`.
pub type EfiPxeCallback = unsafe extern "efiapi" fn(
    This: *mut EfiPxeBaseCodeCallback,
    Function: EfiPxeBaseCodeFunction,
    Received: BOOLEAN,
    PacketLen: UINT32,
    Packet: *mut EfiPxeBaseCodePacket,
) -> EfiPxeBaseCodeCallbackStatus;

/// `EFI_PXE_BASE_CODE_CALLBACK`: the PXE base code callback protocol.
#[repr(C)]
#[derive(Clone, Copy)]
pub struct EfiPxeBaseCodeCallback {
    /// `EFI_PXE_BASE_CODE_CALLBACK_INTERFACE_REVISION`.
    pub Revision: UINT64,
    /// `Callback`.
    pub Callback: EfiPxeCallback,
}

const _: () = assert!(core::mem::size_of::<EfiIpAddress>() == 16);
const _: () = assert!(core::mem::size_of::<EfiPxeBaseCodeDhcpv4Packet>() == 296);
const _: () = assert!(core::mem::size_of::<EfiPxeBaseCodePacket>() == 1472);
const _: () = assert!(core::mem::size_of::<EfiPxeBaseCodeIcmpError>() == 504);
const _: () = assert!(core::mem::size_of::<EfiPxeBaseCodeTftpError>() == 128);
const _: () = assert!(core::mem::size_of::<EfiPxeBaseCodeIpFilter>() == 132);
const _: () = assert!(core::mem::size_of::<EfiPxeBaseCodeArpEntry>() == 48);
const _: () = assert!(core::mem::size_of::<EfiPxeBaseCodeRouteEntry>() == 48);
const _: () = assert!(core::mem::size_of::<EfiPxeBaseCodeSrvlist>() == 20);
const _: () = assert!(core::mem::size_of::<EfiPxeBaseCodeDiscoverInfo>() == 44);
const _: () = assert!(core::mem::size_of::<EfiPxeBaseCodeMtftpInfo>() == 24);
const _: () = assert!(core::mem::size_of::<EfiPxeBaseCodeMode>() == 10424);
const _: () = assert!(core::mem::size_of::<EfiPxeBaseCode>() == 112);
/* </CODE> */
