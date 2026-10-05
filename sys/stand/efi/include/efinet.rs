/* $FreeBSD: head/sys/boot/efi/include/efinet.h 163898 2006-11-02 02:42:48Z marcel $ */

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
    efinet.h

Abstract:
    EFI Simple Network protocol

Revision History
--*/
/* </LICENSES> */

//! EFI simple network protocol (SNP).
//!
//! Upstream: sys/stand/efi/include/efinet.h @ 3ce1f3f79392
//!
//! ## Deviations
//! - Type names are the C typedefs in CamelCase (`EFI_SIMPLE_NETWORK` -> `EfiSimpleNetwork`,
//!   `EFI_SIMPLE_NETWORK_MODE` -> `EfiSimpleNetworkMode`, ...); fields are verbatim.
//! - `EFI_SIMPLE_NETWORK_STATE` is a `u32` alias plus one constant per enumerator.
//! - `struct _EFI_SIMPLE_NETWORK *This` is `*mut EfiSimpleNetwork`; `OPTIONAL` pointers are
//!   plain raw pointers that may be null.
//! - `EFI_IP_ADDRESS` comes from `efipxebc`, as in the C include order.

#![allow(non_snake_case, non_upper_case_globals, non_camel_case_types)]
// EFI names are the specification's (RxTotalFrames, MCastFilter, ...), kept verbatim for grep-ability

use core::ffi::c_void;

use super::amd64::efibind::{UINT8, UINT16, UINT32, UINT64, UINTN};
use super::efidef::{BOOLEAN, EfiEvent, EfiGuid, EfiMacAddress, EfiStatus};
use super::efipxebc::EfiIpAddress;

/// `EFI_SIMPLE_NETWORK_PROTOCOL`.
pub const EFI_SIMPLE_NETWORK_PROTOCOL: EfiGuid = EfiGuid::new(
    0xA19832B9,
    0xAC25,
    0x11D3,
    [0x9A, 0x2D, 0x00, 0x90, 0x27, 0x3F, 0xC1, 0x4D],
);

/// `EFI_SIMPLE_NETWORK_STATE`.
pub type EfiSimpleNetworkState = u32;
/// `EfiSimpleNetworkStopped`.
pub const EfiSimpleNetworkStopped: EfiSimpleNetworkState = 0;
/// `EfiSimpleNetworkStarted`.
pub const EfiSimpleNetworkStarted: EfiSimpleNetworkState = 1;
/// `EfiSimpleNetworkInitialized`.
pub const EfiSimpleNetworkInitialized: EfiSimpleNetworkState = 2;
/// `EfiSimpleNetworkMaxState`.
pub const EfiSimpleNetworkMaxState: EfiSimpleNetworkState = 3;

/// Receive filter: unicast.
pub const EFI_SIMPLE_NETWORK_RECEIVE_UNICAST: UINT32 = 0x01;
/// Receive filter: multicast.
pub const EFI_SIMPLE_NETWORK_RECEIVE_MULTICAST: UINT32 = 0x02;
/// Receive filter: broadcast.
pub const EFI_SIMPLE_NETWORK_RECEIVE_BROADCAST: UINT32 = 0x04;
/// Receive filter: promiscuous.
pub const EFI_SIMPLE_NETWORK_RECEIVE_PROMISCUOUS: UINT32 = 0x08;
/// Receive filter: promiscuous multicast.
pub const EFI_SIMPLE_NETWORK_RECEIVE_PROMISCUOUS_MULTICAST: UINT32 = 0x10;

/// Interrupt status: receive.
pub const EFI_SIMPLE_NETWORK_RECEIVE_INTERRUPT: UINT32 = 0x01;
/// Interrupt status: transmit.
pub const EFI_SIMPLE_NETWORK_TRANSMIT_INTERRUPT: UINT32 = 0x02;
/// Interrupt status: command.
pub const EFI_SIMPLE_NETWORK_COMMAND_INTERRUPT: UINT32 = 0x04;
/// Interrupt status: software.
pub const EFI_SIMPLE_NETWORK_SOFTWARE_INTERRUPT: UINT32 = 0x08;

/// `MAX_MCAST_FILTER_CNT`.
pub const MAX_MCAST_FILTER_CNT: UINTN = 16;

/// `EFI_SIMPLE_NETWORK_INTERFACE_REVISION`.
pub const EFI_SIMPLE_NETWORK_INTERFACE_REVISION: UINT64 = 0x00010000;

/// `EFI_NETWORK_STATISTICS`.
#[repr(C)]
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct EfiNetworkStatistics {
    /// Total frames received, including frames with errors and dropped frames.
    pub RxTotalFrames: UINT64,
    /// Valid frames received and copied into receive buffers.
    pub RxGoodFrames: UINT64,
    /// Frames below the minimum length for the media.
    pub RxUndersizeFrames: UINT64,
    /// Frames longer than the maximum length for the media.
    pub RxOversizeFrames: UINT64,
    /// Valid frames dropped because receive buffers were full.
    pub RxDroppedFrames: UINT64,
    /// Valid unicast frames received and not dropped.
    pub RxUnicastFrames: UINT64,
    /// Valid broadcast frames received and not dropped.
    pub RxBroadcastFrames: UINT64,
    /// Valid multicast frames received and not dropped.
    pub RxMulticastFrames: UINT64,
    /// Frames with CRC or alignment errors.
    pub RxCrcErrorFrames: UINT64,
    /// Total bytes received.
    pub RxTotalBytes: UINT64,
    /// Total frames transmitted.
    pub TxTotalFrames: UINT64,
    /// Good frames transmitted.
    pub TxGoodFrames: UINT64,
    /// Undersize frames transmitted.
    pub TxUndersizeFrames: UINT64,
    /// Oversize frames transmitted.
    pub TxOversizeFrames: UINT64,
    /// Frames dropped on transmit.
    pub TxDroppedFrames: UINT64,
    /// Unicast frames transmitted.
    pub TxUnicastFrames: UINT64,
    /// Broadcast frames transmitted.
    pub TxBroadcastFrames: UINT64,
    /// Multicast frames transmitted.
    pub TxMulticastFrames: UINT64,
    /// Frames transmitted with CRC errors.
    pub TxCrcErrorFrames: UINT64,
    /// Total bytes transmitted.
    pub TxTotalBytes: UINT64,
    /// Collisions detected on this subnet.
    pub Collisions: UINT64,
    /// Frames destined for an unsupported protocol.
    pub UnsupportedProtocol: UINT64,
}

/// `EFI_SIMPLE_NETWORK_MODE`.
#[repr(C)]
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct EfiSimpleNetworkMode {
    /// An [`EfiSimpleNetworkState`].
    pub State: UINT32,
    /// Size of a hardware address.
    pub HwAddressSize: UINT32,
    /// Size of the media header.
    pub MediaHeaderSize: UINT32,
    /// Maximum packet size.
    pub MaxPacketSize: UINT32,
    /// Size of the non-volatile RAM.
    pub NvRamSize: UINT32,
    /// Access granularity of the non-volatile RAM.
    pub NvRamAccessSize: UINT32,
    /// Receive filters the device supports.
    pub ReceiveFilterMask: UINT32,
    /// Receive filters currently set.
    pub ReceiveFilterSetting: UINT32,
    /// Maximum multicast filters.
    pub MaxMCastFilterCount: UINT32,
    /// Multicast filters in use.
    pub MCastFilterCount: UINT32,
    /// Multicast filter list.
    pub MCastFilter: [EfiMacAddress; MAX_MCAST_FILTER_CNT],
    /// Current station address.
    pub CurrentAddress: EfiMacAddress,
    /// Broadcast address.
    pub BroadcastAddress: EfiMacAddress,
    /// Permanent station address.
    pub PermanentAddress: EfiMacAddress,
    /// Interface type (RFC 3232).
    pub IfType: UINT8,
    /// The station address can be changed.
    pub MacAddressChangeable: BOOLEAN,
    /// Multiple transmits can be outstanding.
    pub MultipleTxSupported: BOOLEAN,
    /// Media presence is detectable.
    pub MediaPresentSupported: BOOLEAN,
    /// Media is present.
    pub MediaPresent: BOOLEAN,
}

/// `EFI_SIMPLE_NETWORK_START`.
pub type EfiSimpleNetworkStart =
    unsafe extern "efiapi" fn(This: *mut EfiSimpleNetwork) -> EfiStatus;

/// `EFI_SIMPLE_NETWORK_STOP`.
pub type EfiSimpleNetworkStop = unsafe extern "efiapi" fn(This: *mut EfiSimpleNetwork) -> EfiStatus;

/// `EFI_SIMPLE_NETWORK_INITIALIZE`.
pub type EfiSimpleNetworkInitialize = unsafe extern "efiapi" fn(
    This: *mut EfiSimpleNetwork,
    ExtraRxBufferSize: UINTN,
    ExtraTxBufferSize: UINTN,
) -> EfiStatus;

/// `EFI_SIMPLE_NETWORK_RESET`.
pub type EfiSimpleNetworkReset = unsafe extern "efiapi" fn(
    This: *mut EfiSimpleNetwork,
    ExtendedVerification: BOOLEAN,
) -> EfiStatus;

/// `EFI_SIMPLE_NETWORK_SHUTDOWN`.
pub type EfiSimpleNetworkShutdown =
    unsafe extern "efiapi" fn(This: *mut EfiSimpleNetwork) -> EfiStatus;

/// `EFI_SIMPLE_NETWORK_RECEIVE_FILTERS`.
pub type EfiSimpleNetworkReceiveFilters = unsafe extern "efiapi" fn(
    This: *mut EfiSimpleNetwork,
    Enable: UINT32,
    Disable: UINT32,
    ResetMCastFilter: BOOLEAN,
    MCastFilterCnt: UINTN,
    MCastFilter: *mut EfiMacAddress,
) -> EfiStatus;

/// `EFI_SIMPLE_NETWORK_STATION_ADDRESS`.
pub type EfiSimpleNetworkStationAddress = unsafe extern "efiapi" fn(
    This: *mut EfiSimpleNetwork,
    Reset: BOOLEAN,
    New: *mut EfiMacAddress,
) -> EfiStatus;

/// `EFI_SIMPLE_NETWORK_STATISTICS`.
pub type EfiSimpleNetworkStatistics = unsafe extern "efiapi" fn(
    This: *mut EfiSimpleNetwork,
    Reset: BOOLEAN,
    StatisticsSize: *mut UINTN,
    StatisticsTable: *mut EfiNetworkStatistics,
) -> EfiStatus;

/// `EFI_SIMPLE_NETWORK_MCAST_IP_TO_MAC`.
pub type EfiSimpleNetworkMcastIpToMac = unsafe extern "efiapi" fn(
    This: *mut EfiSimpleNetwork,
    IPv6: BOOLEAN,
    IP: *mut EfiIpAddress,
    MAC: *mut EfiMacAddress,
) -> EfiStatus;

/// `EFI_SIMPLE_NETWORK_NVDATA`.
pub type EfiSimpleNetworkNvdata = unsafe extern "efiapi" fn(
    This: *mut EfiSimpleNetwork,
    ReadWrite: BOOLEAN,
    Offset: UINTN,
    BufferSize: UINTN,
    Buffer: *mut c_void,
) -> EfiStatus;

/// `EFI_SIMPLE_NETWORK_GET_STATUS`.
pub type EfiSimpleNetworkGetStatus = unsafe extern "efiapi" fn(
    This: *mut EfiSimpleNetwork,
    InterruptStatus: *mut UINT32,
    TxBuf: *mut *mut c_void,
) -> EfiStatus;

/// `EFI_SIMPLE_NETWORK_TRANSMIT`.
pub type EfiSimpleNetworkTransmit = unsafe extern "efiapi" fn(
    This: *mut EfiSimpleNetwork,
    HeaderSize: UINTN,
    BufferSize: UINTN,
    Buffer: *mut c_void,
    SrcAddr: *mut EfiMacAddress,
    DestAddr: *mut EfiMacAddress,
    Protocol: *mut UINT16,
) -> EfiStatus;

/// `EFI_SIMPLE_NETWORK_RECEIVE`.
pub type EfiSimpleNetworkReceive = unsafe extern "efiapi" fn(
    This: *mut EfiSimpleNetwork,
    HeaderSize: *mut UINTN,
    BufferSize: *mut UINTN,
    Buffer: *mut c_void,
    SrcAddr: *mut EfiMacAddress,
    DestAddr: *mut EfiMacAddress,
    Protocol: *mut UINT16,
) -> EfiStatus;

/// `EFI_SIMPLE_NETWORK`: the simple network protocol.
#[repr(C)]
#[derive(Clone, Copy)]
pub struct EfiSimpleNetwork {
    /// `EFI_SIMPLE_NETWORK_INTERFACE_REVISION`.
    pub Revision: UINT64,
    /// `Start`.
    pub Start: EfiSimpleNetworkStart,
    /// `Stop`.
    pub Stop: EfiSimpleNetworkStop,
    /// `Initialize`.
    pub Initialize: EfiSimpleNetworkInitialize,
    /// `Reset`.
    pub Reset: EfiSimpleNetworkReset,
    /// `Shutdown`.
    pub Shutdown: EfiSimpleNetworkShutdown,
    /// `ReceiveFilters`.
    pub ReceiveFilters: EfiSimpleNetworkReceiveFilters,
    /// `StationAddress`.
    pub StationAddress: EfiSimpleNetworkStationAddress,
    /// `Statistics`.
    pub Statistics: EfiSimpleNetworkStatistics,
    /// `MCastIpToMac`.
    pub MCastIpToMac: EfiSimpleNetworkMcastIpToMac,
    /// `NvData`.
    pub NvData: EfiSimpleNetworkNvdata,
    /// `GetStatus`.
    pub GetStatus: EfiSimpleNetworkGetStatus,
    /// `Transmit`.
    pub Transmit: EfiSimpleNetworkTransmit,
    /// `Receive`.
    pub Receive: EfiSimpleNetworkReceive,
    /// `WaitForPacket`: signalled when a packet can be received.
    pub WaitForPacket: EfiEvent,
    /// The mode.
    pub Mode: *mut EfiSimpleNetworkMode,
}

const _: () = assert!(core::mem::size_of::<EfiNetworkStatistics>() == 176);
const _: () = assert!(core::mem::size_of::<EfiSimpleNetworkMode>() == 656);
const _: () = assert!(core::mem::size_of::<EfiSimpleNetwork>() == 128);
