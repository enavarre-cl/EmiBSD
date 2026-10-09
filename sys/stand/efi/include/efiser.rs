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

    efiser.h

Abstract:

    EFI serial protocol

Revision History

--*/
/* </LICENSES> */

/* <CODE> */
//! EFI serial I/O protocol.
//!
//! Upstream: sys/stand/efi/include/efiser.h @ 3ce1f3f79392
//!
//! ## Deviations
//! - Type names are the C typedefs in CamelCase (`SERIAL_IO_INTERFACE` ->
//!   `SerialIoInterface`, `SERIAL_IO_MODE` -> `SerialIoMode`); fields are verbatim.
//! - `EFI_PARITY_TYPE` and `EFI_STOP_BITS_TYPE` are `u32` aliases plus one constant per
//!   enumerator (firmware may return values outside the list).
//! - `struct _SERIAL_IO_INTERFACE *This` is `*mut SerialIoInterface`.

#![allow(non_snake_case, non_upper_case_globals, non_camel_case_types)]
// EFI names are the specification's (SERIAL_IO_MODE, DefaultParity, ...), kept verbatim for grep-ability

use core::ffi::c_void;

use super::amd64::efibind::{UINT8, UINT32, UINT64, UINTN};
use super::efidef::{EfiGuid, EfiStatus};

/// `SERIAL_IO_PROTOCOL`.
pub const SERIAL_IO_PROTOCOL: EfiGuid = EfiGuid::new(
    0xBB25CF6F,
    0xF1D4,
    0x11D2,
    [0x9A, 0x0C, 0x00, 0x90, 0x27, 0x3F, 0xC1, 0xFD],
);

/// `EFI_PARITY_TYPE`.
pub type EfiParityType = u32;
/// `DefaultParity`.
pub const DefaultParity: EfiParityType = 0;
/// `NoParity`.
pub const NoParity: EfiParityType = 1;
/// `EvenParity`.
pub const EvenParity: EfiParityType = 2;
/// `OddParity`.
pub const OddParity: EfiParityType = 3;
/// `MarkParity`.
pub const MarkParity: EfiParityType = 4;
/// `SpaceParity`.
pub const SpaceParity: EfiParityType = 5;

/// `EFI_STOP_BITS_TYPE`.
pub type EfiStopBitsType = u32;
/// `DefaultStopBits`.
pub const DefaultStopBits: EfiStopBitsType = 0;
/// `OneStopBit`: 1 stop bit.
pub const OneStopBit: EfiStopBitsType = 1;
/// `OneFiveStopBits`: 1.5 stop bits.
pub const OneFiveStopBits: EfiStopBitsType = 2;
/// `TwoStopBits`: 2 stop bits.
pub const TwoStopBits: EfiStopBitsType = 3;

/// Control bit, read only.
pub const EFI_SERIAL_CLEAR_TO_SEND: UINT32 = 0x0010;
/// Control bit, read only.
pub const EFI_SERIAL_DATA_SET_READY: UINT32 = 0x0020;
/// Control bit, read only.
pub const EFI_SERIAL_RING_INDICATE: UINT32 = 0x0040;
/// Control bit, read only.
pub const EFI_SERIAL_CARRIER_DETECT: UINT32 = 0x0080;
/// Control bit, write only.
pub const EFI_SERIAL_REQUEST_TO_SEND: UINT32 = 0x0002;
/// Control bit, write only.
pub const EFI_SERIAL_DATA_TERMINAL_READY: UINT32 = 0x0001;
/// Control bit, read only.
pub const EFI_SERIAL_INPUT_BUFFER_EMPTY: UINT32 = 0x0100;
/// Control bit, read only.
pub const EFI_SERIAL_OUTPUT_BUFFER_EMPTY: UINT32 = 0x0200;
/// Control bit, read/write.
pub const EFI_SERIAL_HARDWARE_LOOPBACK_ENABLE: UINT32 = 0x1000;
/// Control bit, read/write.
pub const EFI_SERIAL_SOFTWARE_LOOPBACK_ENABLE: UINT32 = 0x2000;
/// Control bit, read/write.
pub const EFI_SERIAL_HARDWARE_FLOW_CONTROL_ENABLE: UINT32 = 0x4000;

/// `SERIAL_IO_INTERFACE_REVISION`.
pub const SERIAL_IO_INTERFACE_REVISION: UINT32 = 0x00010000;

/// `EFI_SERIAL_RESET`.
pub type EfiSerialReset = unsafe extern "efiapi" fn(This: *mut SerialIoInterface) -> EfiStatus;

/// `EFI_SERIAL_SET_ATTRIBUTES`.
pub type EfiSerialSetAttributes = unsafe extern "efiapi" fn(
    This: *mut SerialIoInterface,
    BaudRate: UINT64,
    ReceiveFifoDepth: UINT32,
    Timeout: UINT32,
    Parity: EfiParityType,
    DataBits: UINT8,
    StopBits: EfiStopBitsType,
) -> EfiStatus;

/// `EFI_SERIAL_SET_CONTROL_BITS`.
pub type EfiSerialSetControlBits =
    unsafe extern "efiapi" fn(This: *mut SerialIoInterface, Control: UINT32) -> EfiStatus;

/// `EFI_SERIAL_GET_CONTROL_BITS`.
pub type EfiSerialGetControlBits =
    unsafe extern "efiapi" fn(This: *mut SerialIoInterface, Control: *mut UINT32) -> EfiStatus;

/// `EFI_SERIAL_WRITE`.
pub type EfiSerialWrite = unsafe extern "efiapi" fn(
    This: *mut SerialIoInterface,
    BufferSize: *mut UINTN,
    Buffer: *mut c_void,
) -> EfiStatus;

/// `EFI_SERIAL_READ`.
pub type EfiSerialRead = unsafe extern "efiapi" fn(
    This: *mut SerialIoInterface,
    BufferSize: *mut UINTN,
    Buffer: *mut c_void,
) -> EfiStatus;

/// `SERIAL_IO_MODE`.
#[repr(C)]
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct SerialIoMode {
    /// Mask of the control bits the device supports.
    pub ControlMask: UINT32,
    /// Current timeout.
    pub Timeout: UINT32,
    /// Current baud rate.
    pub BaudRate: UINT64,
    /// Current receive FIFO depth.
    pub ReceiveFifoDepth: UINT32,
    /// Current data bits.
    pub DataBits: UINT32,
    /// Current parity.
    pub Parity: UINT32,
    /// Current stop bits.
    pub StopBits: UINT32,
}

/// `SERIAL_IO_INTERFACE`: the serial I/O protocol.
#[repr(C)]
#[derive(Clone, Copy)]
pub struct SerialIoInterface {
    /// `SERIAL_IO_INTERFACE_REVISION`.
    pub Revision: UINT32,
    /// `Reset`.
    pub Reset: EfiSerialReset,
    /// `SetAttributes`.
    pub SetAttributes: EfiSerialSetAttributes,
    /// `SetControl`.
    pub SetControl: EfiSerialSetControlBits,
    /// `GetControl`.
    pub GetControl: EfiSerialGetControlBits,
    /// `Write`.
    pub Write: EfiSerialWrite,
    /// `Read`.
    pub Read: EfiSerialRead,
    /// The current mode.
    pub Mode: *mut SerialIoMode,
}

const _: () = assert!(core::mem::size_of::<SerialIoMode>() == 32);
const _: () = assert!(core::mem::size_of::<SerialIoInterface>() == 64);
/* </CODE> */
