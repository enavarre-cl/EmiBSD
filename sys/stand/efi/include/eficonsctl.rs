/* $FreeBSD: head/sys/boot/efi/include/eficonsctl.h 272105 2014-09-25 13:31:08Z emaste $ */

/* <LICENSES> */
/*-
 * Copyright (c) 2004 - 2010, Intel Corporation. All rights reserved.
 *
 * Redistribution and use in source and binary forms, with or without
 * modification, are permitted provided that the following conditions
 * are met:
 *
 * 1. Redistributions of source code must retain the above copyright
 *    notice, this list of conditions and the following disclaimer.
 * 2. Redistributions in binary form must reproduce the above copyright
 *    notice, this list of conditions and the following disclaimer in the
 *    documentation and/or other materials provided with the distribution.
 *
 * THIS SOFTWARE IS PROVIDED BY THE COPYRIGHT HOLDERS AND CONTRIBUTORS "AS IS"
 * AND ANY EXPRESS OR IMPLIED WARRANTIES, INCLUDING, BUT NOT LIMITED TO, THE
 * IMPLIED WARRANTIES OF MERCHANTABILITY AND FITNESS FOR A PARTICULAR PURPOSE
 * ARE DISCLAIMED.  IN NO EVENT SHALL THE COPYRIGHT HOLDER OR CONTRIBUTORS
 * BE LIABLE FOR ANY DIRECT, INDIRECT, INCIDENTAL, SPECIAL, EXEMPLARY, OR
 * CONSEQUENTIAL DAMAGES (INCLUDING, BUT NOT LIMITED TO, PROCUREMENT OF
 * SUBSTITUTE GOODS OR SERVICES; LOSS OF USE, DATA, OR PROFITS; OR BUSINESS
 * INTERRUPTION) HOWEVER CAUSED AND ON ANY THEORY OF LIABILITY, WHETHER IN
 * CONTRACT, STRICT LIABILITY, OR TORT (INCLUDING NEGLIGENCE OR OTHERWISE)
 * ARISING IN ANY WAY OUT OF THE USE OF THIS SOFTWARE, EVEN IF ADVISED OF
 * THE POSSIBILITY OF SUCH DAMAGE.
 */

/*
 * Original Module Name: ConsoleControl.h
 * Abstract: Abstraction of a Text mode or GOP/UGA screen
 */
/* </LICENSES> */

//! EFI console control protocol (the Apple and Intel text/graphics screen switch).
//!
//! Upstream: sys/stand/efi/include/eficonsctl.h @ 3ce1f3f79392
//!
//! The header carries the FreeBSD tag on its 33rd line, after its licence; the tag is the
//! first line of this file, as the other headers have it. The routine descriptions
//! (`/*++ ... --*/`) after each prototype are in the doc comments below.
//!
//! ## Deviations
//! - `EFI_CONSOLE_CONTROL_PROTOCOL` is `EfiConsoleControlProtocol`;
//!   `EFI_CONSOLE_CONTROL_SCREEN_MODE` is a `u32` alias plus one constant per enumerator.
//! - `extern EFI_GUID gEfiConsoleControlProtocolGuid` is not ported: nothing in the tree
//!   defines it (the GUID is the macro), and a Rust `extern` static would be an unresolved
//!   symbol. The GUID macro is the `EfiGuid` constant `EFI_CONSOLE_CONTROL_PROTOCOL_GUID`.
//! - `This` is `*mut EfiConsoleControlProtocol`; the `OPTIONAL` arguments of `GetMode` are raw
//!   pointers that may be null.

#![allow(non_snake_case, non_upper_case_globals, non_camel_case_types)]
// EFI names are the specification's (GopUgaExists, EfiConsoleControlScreenText, ...), kept verbatim for grep-ability

use super::efidef::{BOOLEAN, CHAR16, EfiGuid, EfiStatus};

/// `EFI_CONSOLE_CONTROL_PROTOCOL_GUID`.
pub const EFI_CONSOLE_CONTROL_PROTOCOL_GUID: EfiGuid = EfiGuid::new(
    0xf42f7782,
    0x12e,
    0x4c12,
    [0x99, 0x56, 0x49, 0xf9, 0x43, 0x4, 0xf7, 0x21],
);

/// `EFI_CONSOLE_CONTROL_SCREEN_MODE`.
pub type EfiConsoleControlScreenMode = u32;
/// `EfiConsoleControlScreenText`.
pub const EfiConsoleControlScreenText: EfiConsoleControlScreenMode = 0;
/// `EfiConsoleControlScreenGraphics`.
pub const EfiConsoleControlScreenGraphics: EfiConsoleControlScreenMode = 1;
/// `EfiConsoleControlScreenMaxValue`.
pub const EfiConsoleControlScreenMaxValue: EfiConsoleControlScreenMode = 2;

/// `EFI_CONSOLE_CONTROL_PROTOCOL_GET_MODE`: return the current video mode, whether Graphics
/// Output or UGA Draw devices exist, and whether Std In is locked. The last two arguments
/// are optional and only returned when non-null.
pub type EfiConsoleControlProtocolGetMode = unsafe extern "efiapi" fn(
    This: *mut EfiConsoleControlProtocol,
    Mode: *mut EfiConsoleControlScreenMode,
    GopUgaExists: *mut BOOLEAN,
    StdInLocked: *mut BOOLEAN,
) -> EfiStatus;

/// `EFI_CONSOLE_CONTROL_PROTOCOL_SET_MODE`: set the current mode to text or graphics
/// (graphics is for Quiet Boot).
pub type EfiConsoleControlProtocolSetMode = unsafe extern "efiapi" fn(
    This: *mut EfiConsoleControlProtocol,
    Mode: EfiConsoleControlScreenMode,
) -> EfiStatus;

/// `EFI_CONSOLE_CONTROL_PROTOCOL_LOCK_STD_IN`: lock the Std In devices until `Password` is
/// typed; a null password unlocks the keyboard. Returns `EFI_DEVICE_ERROR` when Std In is
/// not locked.
pub type EfiConsoleControlProtocolLockStdIn = unsafe extern "efiapi" fn(
    This: *mut EfiConsoleControlProtocol,
    Password: *mut CHAR16,
) -> EfiStatus;

/// `EFI_CONSOLE_CONTROL_PROTOCOL` (C tag `_EFI_CONSOLE_CONTROL_PROTOCOL`).
#[repr(C)]
#[derive(Clone, Copy)]
pub struct EfiConsoleControlProtocol {
    /// `GetMode`.
    pub GetMode: EfiConsoleControlProtocolGetMode,
    /// `SetMode`.
    pub SetMode: EfiConsoleControlProtocolSetMode,
    /// `LockStdIn`.
    pub LockStdIn: EfiConsoleControlProtocolLockStdIn,
}

const _: () = assert!(core::mem::size_of::<EfiConsoleControlProtocol>() == 24);
