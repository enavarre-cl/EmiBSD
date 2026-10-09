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

    efigop.h

Abstract:
    Info about framebuffers




Revision History

--*/
/* </LICENSES> */

/* <CODE> */
//! EFI graphics output protocol (GOP).
//!
//! Upstream: sys/stand/efi/include/efigop.h @ 3ce1f3f79392
//!
//! ## Deviations
//! - Type names are the C typedefs in CamelCase (`EFI_GRAPHICS_OUTPUT` ->
//!   `EfiGraphicsOutput`, `EFI_GRAPHICS_OUTPUT_PROTOCOL_MODE` ->
//!   `EfiGraphicsOutputProtocolMode`, ...); fields are verbatim.
//! - `EFI_GRAPHICS_PIXEL_FORMAT` and `EFI_GRAPHICS_OUTPUT_BLT_OPERATION` are `u32` aliases
//!   plus one constant per enumerator (the misspelt `EfiGraphcisOutputBltOperationMax` keeps
//!   its spelling).
//! - `struct _EFI_GRAPHICS_OUTPUT *This` is `*mut EfiGraphicsOutput`.

#![allow(non_snake_case, non_upper_case_globals, non_camel_case_types)]
// EFI names are the specification's (RedMask, PixelBitMask, EfiBltVideoFill, ...), kept verbatim for grep-ability

use super::amd64::efibind::{UINT8, UINT32, UINTN};
use super::efidef::{EfiGuid, EfiPhysicalAddress, EfiStatus};

/// `EFI_GRAPHICS_OUTPUT_PROTOCOL_GUID`.
pub const EFI_GRAPHICS_OUTPUT_PROTOCOL_GUID: EfiGuid = EfiGuid::new(
    0x9042a9de,
    0x23dc,
    0x4a38,
    [0x96, 0xfb, 0x7a, 0xde, 0xd0, 0x80, 0x51, 0x6a],
);

/// `EFI_GRAPHICS_PIXEL_FORMAT`.
pub type EfiGraphicsPixelFormat = u32;
/// `PixelRedGreenBlueReserved8BitPerColor`.
pub const PixelRedGreenBlueReserved8BitPerColor: EfiGraphicsPixelFormat = 0;
/// `PixelBlueGreenRedReserved8BitPerColor`.
pub const PixelBlueGreenRedReserved8BitPerColor: EfiGraphicsPixelFormat = 1;
/// `PixelBitMask`.
pub const PixelBitMask: EfiGraphicsPixelFormat = 2;
/// `PixelBltOnly`.
pub const PixelBltOnly: EfiGraphicsPixelFormat = 3;
/// `PixelFormatMax`.
pub const PixelFormatMax: EfiGraphicsPixelFormat = 4;

/// `EFI_GRAPHICS_OUTPUT_BLT_OPERATION`.
pub type EfiGraphicsOutputBltOperation = u32;
/// `EfiBltVideoFill`.
pub const EfiBltVideoFill: EfiGraphicsOutputBltOperation = 0;
/// `EfiBltVideoToBltBuffer`.
pub const EfiBltVideoToBltBuffer: EfiGraphicsOutputBltOperation = 1;
/// `EfiBltBufferToVideo`.
pub const EfiBltBufferToVideo: EfiGraphicsOutputBltOperation = 2;
/// `EfiBltVideoToVideo`.
pub const EfiBltVideoToVideo: EfiGraphicsOutputBltOperation = 3;
/// `EfiGraphcisOutputBltOperationMax` (sic).
pub const EfiGraphcisOutputBltOperationMax: EfiGraphicsOutputBltOperation = 4;

/// `EFI_PIXEL_BITMASK`.
#[repr(C)]
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct EfiPixelBitmask {
    /// Red mask.
    pub RedMask: UINT32,
    /// Green mask.
    pub GreenMask: UINT32,
    /// Blue mask.
    pub BlueMask: UINT32,
    /// Reserved mask.
    pub ReservedMask: UINT32,
}

/// `EFI_GRAPHICS_OUTPUT_MODE_INFORMATION`.
#[repr(C)]
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct EfiGraphicsOutputModeInformation {
    /// Version, 0 for this layout.
    pub Version: UINT32,
    /// Horizontal resolution in pixels.
    pub HorizontalResolution: UINT32,
    /// Vertical resolution in pixels.
    pub VerticalResolution: UINT32,
    /// An [`EfiGraphicsPixelFormat`].
    pub PixelFormat: EfiGraphicsPixelFormat,
    /// Bit masks, valid when `PixelFormat` is `PixelBitMask`.
    pub PixelInformation: EfiPixelBitmask,
    /// Pixels per scan line (the stride).
    pub PixelsPerScanLine: UINT32,
}

/// `EFI_GRAPHICS_OUTPUT_PROTOCOL_MODE`.
#[repr(C)]
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct EfiGraphicsOutputProtocolMode {
    /// Number of modes.
    pub MaxMode: UINT32,
    /// Current mode.
    pub Mode: UINT32,
    /// Information of the current mode.
    pub Info: *mut EfiGraphicsOutputModeInformation,
    /// Size of `Info`.
    pub SizeOfInfo: UINTN,
    /// Physical address of the frame buffer.
    pub FrameBufferBase: EfiPhysicalAddress,
    /// Size of the frame buffer in bytes.
    pub FrameBufferSize: UINTN,
}

/// `EFI_GRAPHICS_OUTPUT_BLT_PIXEL`.
#[repr(C)]
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct EfiGraphicsOutputBltPixel {
    /// Blue.
    pub Blue: UINT8,
    /// Green.
    pub Green: UINT8,
    /// Red.
    pub Red: UINT8,
    /// Reserved.
    pub Reserved: UINT8,
}

/// `EFI_GRAPHICS_OUTPUT_PROTOCOL_QUERY_MODE`.
pub type EfiGraphicsOutputProtocolQueryMode = unsafe extern "efiapi" fn(
    This: *mut EfiGraphicsOutput,
    ModeNumber: UINT32,
    SizeOfInfo: *mut UINTN,
    Info: *mut *mut EfiGraphicsOutputModeInformation,
) -> EfiStatus;

/// `EFI_GRAPHICS_OUTPUT_PROTOCOL_SET_MODE`.
pub type EfiGraphicsOutputProtocolSetMode =
    unsafe extern "efiapi" fn(This: *mut EfiGraphicsOutput, ModeNumber: UINT32) -> EfiStatus;

/// `EFI_GRAPHICS_OUTPUT_PROTOCOL_BLT`.
pub type EfiGraphicsOutputProtocolBlt = unsafe extern "efiapi" fn(
    This: *mut EfiGraphicsOutput,
    BltBuffer: *mut EfiGraphicsOutputBltPixel,
    BltOperation: EfiGraphicsOutputBltOperation,
    SourceX: UINTN,
    SourceY: UINTN,
    DestinationX: UINTN,
    DestinationY: UINTN,
    Width: UINTN,
    Height: UINTN,
    Delta: UINTN,
) -> EfiStatus;

/// `EFI_GRAPHICS_OUTPUT`: the graphics output protocol.
#[repr(C)]
#[derive(Clone, Copy)]
pub struct EfiGraphicsOutput {
    /// `QueryMode`.
    pub QueryMode: EfiGraphicsOutputProtocolQueryMode,
    /// `SetMode`.
    pub SetMode: EfiGraphicsOutputProtocolSetMode,
    /// `Blt`.
    pub Blt: EfiGraphicsOutputProtocolBlt,
    /// The current mode.
    pub Mode: *mut EfiGraphicsOutputProtocolMode,
}

const _: () = assert!(core::mem::size_of::<EfiPixelBitmask>() == 16);
const _: () = assert!(core::mem::size_of::<EfiGraphicsOutputModeInformation>() == 36);
const _: () = assert!(core::mem::size_of::<EfiGraphicsOutputProtocolMode>() == 40);
const _: () = assert!(core::mem::size_of::<EfiGraphicsOutputBltPixel>() == 4);
const _: () = assert!(core::mem::size_of::<EfiGraphicsOutput>() == 32);
/* </CODE> */
