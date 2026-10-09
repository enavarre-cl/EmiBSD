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

    eficon.h

Abstract:

    EFI console protocols



Revision History

--*/
/* </LICENSES> */

/* <CODE> */
//! EFI console protocols: simple text output and simple text input.
//!
//! Upstream: sys/stand/efi/include/eficon.h @ 3ce1f3f79392
//!
//! `ConOut`, `StdErr` and `ConIn` of the system table point at these interfaces.
//!
//! ## Deviations
//! - Type names are the C typedefs in CamelCase (`SIMPLE_TEXT_OUTPUT_INTERFACE` ->
//!   `SimpleTextOutputInterface`, `EFI_INPUT_KEY` -> `EfiInputKey`, ...); fields are verbatim.
//! - `struct _X *This` parameters are `*mut X`.
//! - `EFI_TEXT_ATTR(f, b)` is the const fn [`efi_text_attr`]. The colour constants are
//!   `UINTN`, the type of `SetAttribute`'s argument.
//! - Box drawing, block element, shape and arrow constants are `CHAR16`; the scan codes are
//!   `UINT16`, the type of `EFI_INPUT_KEY.ScanCode`.

#![allow(non_snake_case, non_upper_case_globals, non_camel_case_types)]
// EFI names are the specification's (SIMPLE_TEXT_OUTPUT_MODE, ScanCode, ...), kept verbatim for grep-ability

use super::amd64::efibind::{INT32, UINT16, UINTN};
use super::efidef::{BOOLEAN, CHAR16, EfiEvent, EfiGuid, EfiStatus};

/// `SIMPLE_TEXT_OUTPUT_PROTOCOL`.
pub const SIMPLE_TEXT_OUTPUT_PROTOCOL: EfiGuid = EfiGuid::new(
    0x387477c2,
    0x69c7,
    0x11d2,
    [0x8e, 0x39, 0x0, 0xa0, 0xc9, 0x69, 0x72, 0x3b],
);

/// `SIMPLE_TEXT_INPUT_PROTOCOL`.
pub const SIMPLE_TEXT_INPUT_PROTOCOL: EfiGuid = EfiGuid::new(
    0x387477c1,
    0x69c7,
    0x11d2,
    [0x8e, 0x39, 0x0, 0xa0, 0xc9, 0x69, 0x72, 0x3b],
);

/// `EFI_BLACK`.
pub const EFI_BLACK: UINTN = 0x00;
/// `EFI_BLUE`.
pub const EFI_BLUE: UINTN = 0x01;
/// `EFI_GREEN`.
pub const EFI_GREEN: UINTN = 0x02;
/// `EFI_CYAN`.
pub const EFI_CYAN: UINTN = EFI_BLUE | EFI_GREEN;
/// `EFI_RED`.
pub const EFI_RED: UINTN = 0x04;
/// `EFI_MAGENTA`.
pub const EFI_MAGENTA: UINTN = EFI_BLUE | EFI_RED;
/// `EFI_BROWN`.
pub const EFI_BROWN: UINTN = EFI_GREEN | EFI_RED;
/// `EFI_LIGHTGRAY`.
pub const EFI_LIGHTGRAY: UINTN = EFI_BLUE | EFI_GREEN | EFI_RED;
/// `EFI_BRIGHT`.
pub const EFI_BRIGHT: UINTN = 0x08;
/// `EFI_DARKGRAY`.
pub const EFI_DARKGRAY: UINTN = EFI_BRIGHT;
/// `EFI_LIGHTBLUE`.
pub const EFI_LIGHTBLUE: UINTN = EFI_BLUE | EFI_BRIGHT;
/// `EFI_LIGHTGREEN`.
pub const EFI_LIGHTGREEN: UINTN = EFI_GREEN | EFI_BRIGHT;
/// `EFI_LIGHTCYAN`.
pub const EFI_LIGHTCYAN: UINTN = EFI_CYAN | EFI_BRIGHT;
/// `EFI_LIGHTRED`.
pub const EFI_LIGHTRED: UINTN = EFI_RED | EFI_BRIGHT;
/// `EFI_LIGHTMAGENTA`.
pub const EFI_LIGHTMAGENTA: UINTN = EFI_MAGENTA | EFI_BRIGHT;
/// `EFI_YELLOW`.
pub const EFI_YELLOW: UINTN = EFI_BROWN | EFI_BRIGHT;
/// `EFI_WHITE`.
pub const EFI_WHITE: UINTN = EFI_BLUE | EFI_GREEN | EFI_RED | EFI_BRIGHT;

/// `EFI_BACKGROUND_BLACK`.
pub const EFI_BACKGROUND_BLACK: UINTN = 0x00;
/// `EFI_BACKGROUND_BLUE`.
pub const EFI_BACKGROUND_BLUE: UINTN = 0x10;
/// `EFI_BACKGROUND_GREEN`.
pub const EFI_BACKGROUND_GREEN: UINTN = 0x20;
/// `EFI_BACKGROUND_CYAN`.
pub const EFI_BACKGROUND_CYAN: UINTN = EFI_BACKGROUND_BLUE | EFI_BACKGROUND_GREEN;
/// `EFI_BACKGROUND_RED`.
pub const EFI_BACKGROUND_RED: UINTN = 0x40;
/// `EFI_BACKGROUND_MAGENTA`.
pub const EFI_BACKGROUND_MAGENTA: UINTN = EFI_BACKGROUND_BLUE | EFI_BACKGROUND_RED;
/// `EFI_BACKGROUND_BROWN`.
pub const EFI_BACKGROUND_BROWN: UINTN = EFI_BACKGROUND_GREEN | EFI_BACKGROUND_RED;
/// `EFI_BACKGROUND_LIGHTGRAY`.
pub const EFI_BACKGROUND_LIGHTGRAY: UINTN =
    EFI_BACKGROUND_BLUE | EFI_BACKGROUND_GREEN | EFI_BACKGROUND_RED;

/// `BOXDRAW_HORIZONTAL`.
pub const BOXDRAW_HORIZONTAL: CHAR16 = 0x2500;
/// `BOXDRAW_VERTICAL`.
pub const BOXDRAW_VERTICAL: CHAR16 = 0x2502;
/// `BOXDRAW_DOWN_RIGHT`.
pub const BOXDRAW_DOWN_RIGHT: CHAR16 = 0x250c;
/// `BOXDRAW_DOWN_LEFT`.
pub const BOXDRAW_DOWN_LEFT: CHAR16 = 0x2510;
/// `BOXDRAW_UP_RIGHT`.
pub const BOXDRAW_UP_RIGHT: CHAR16 = 0x2514;
/// `BOXDRAW_UP_LEFT`.
pub const BOXDRAW_UP_LEFT: CHAR16 = 0x2518;
/// `BOXDRAW_VERTICAL_RIGHT`.
pub const BOXDRAW_VERTICAL_RIGHT: CHAR16 = 0x251c;
/// `BOXDRAW_VERTICAL_LEFT`.
pub const BOXDRAW_VERTICAL_LEFT: CHAR16 = 0x2524;
/// `BOXDRAW_DOWN_HORIZONTAL`.
pub const BOXDRAW_DOWN_HORIZONTAL: CHAR16 = 0x252c;
/// `BOXDRAW_UP_HORIZONTAL`.
pub const BOXDRAW_UP_HORIZONTAL: CHAR16 = 0x2534;
/// `BOXDRAW_VERTICAL_HORIZONTAL`.
pub const BOXDRAW_VERTICAL_HORIZONTAL: CHAR16 = 0x253c;
/// `BOXDRAW_DOUBLE_HORIZONTAL`.
pub const BOXDRAW_DOUBLE_HORIZONTAL: CHAR16 = 0x2550;
/// `BOXDRAW_DOUBLE_VERTICAL`.
pub const BOXDRAW_DOUBLE_VERTICAL: CHAR16 = 0x2551;
/// `BOXDRAW_DOWN_RIGHT_DOUBLE`.
pub const BOXDRAW_DOWN_RIGHT_DOUBLE: CHAR16 = 0x2552;
/// `BOXDRAW_DOWN_DOUBLE_RIGHT`.
pub const BOXDRAW_DOWN_DOUBLE_RIGHT: CHAR16 = 0x2553;
/// `BOXDRAW_DOUBLE_DOWN_RIGHT`.
pub const BOXDRAW_DOUBLE_DOWN_RIGHT: CHAR16 = 0x2554;
/// `BOXDRAW_DOWN_LEFT_DOUBLE`.
pub const BOXDRAW_DOWN_LEFT_DOUBLE: CHAR16 = 0x2555;
/// `BOXDRAW_DOWN_DOUBLE_LEFT`.
pub const BOXDRAW_DOWN_DOUBLE_LEFT: CHAR16 = 0x2556;
/// `BOXDRAW_DOUBLE_DOWN_LEFT`.
pub const BOXDRAW_DOUBLE_DOWN_LEFT: CHAR16 = 0x2557;
/// `BOXDRAW_UP_RIGHT_DOUBLE`.
pub const BOXDRAW_UP_RIGHT_DOUBLE: CHAR16 = 0x2558;
/// `BOXDRAW_UP_DOUBLE_RIGHT`.
pub const BOXDRAW_UP_DOUBLE_RIGHT: CHAR16 = 0x2559;
/// `BOXDRAW_DOUBLE_UP_RIGHT`.
pub const BOXDRAW_DOUBLE_UP_RIGHT: CHAR16 = 0x255a;
/// `BOXDRAW_UP_LEFT_DOUBLE`.
pub const BOXDRAW_UP_LEFT_DOUBLE: CHAR16 = 0x255b;
/// `BOXDRAW_UP_DOUBLE_LEFT`.
pub const BOXDRAW_UP_DOUBLE_LEFT: CHAR16 = 0x255c;
/// `BOXDRAW_DOUBLE_UP_LEFT`.
pub const BOXDRAW_DOUBLE_UP_LEFT: CHAR16 = 0x255d;
/// `BOXDRAW_VERTICAL_RIGHT_DOUBLE`.
pub const BOXDRAW_VERTICAL_RIGHT_DOUBLE: CHAR16 = 0x255e;
/// `BOXDRAW_VERTICAL_DOUBLE_RIGHT`.
pub const BOXDRAW_VERTICAL_DOUBLE_RIGHT: CHAR16 = 0x255f;
/// `BOXDRAW_DOUBLE_VERTICAL_RIGHT`.
pub const BOXDRAW_DOUBLE_VERTICAL_RIGHT: CHAR16 = 0x2560;
/// `BOXDRAW_VERTICAL_LEFT_DOUBLE`.
pub const BOXDRAW_VERTICAL_LEFT_DOUBLE: CHAR16 = 0x2561;
/// `BOXDRAW_VERTICAL_DOUBLE_LEFT`.
pub const BOXDRAW_VERTICAL_DOUBLE_LEFT: CHAR16 = 0x2562;
/// `BOXDRAW_DOUBLE_VERTICAL_LEFT`.
pub const BOXDRAW_DOUBLE_VERTICAL_LEFT: CHAR16 = 0x2563;
/// `BOXDRAW_DOWN_HORIZONTAL_DOUBLE`.
pub const BOXDRAW_DOWN_HORIZONTAL_DOUBLE: CHAR16 = 0x2564;
/// `BOXDRAW_DOWN_DOUBLE_HORIZONTAL`.
pub const BOXDRAW_DOWN_DOUBLE_HORIZONTAL: CHAR16 = 0x2565;
/// `BOXDRAW_DOUBLE_DOWN_HORIZONTAL`.
pub const BOXDRAW_DOUBLE_DOWN_HORIZONTAL: CHAR16 = 0x2566;
/// `BOXDRAW_UP_HORIZONTAL_DOUBLE`.
pub const BOXDRAW_UP_HORIZONTAL_DOUBLE: CHAR16 = 0x2567;
/// `BOXDRAW_UP_DOUBLE_HORIZONTAL`.
pub const BOXDRAW_UP_DOUBLE_HORIZONTAL: CHAR16 = 0x2568;
/// `BOXDRAW_DOUBLE_UP_HORIZONTAL`.
pub const BOXDRAW_DOUBLE_UP_HORIZONTAL: CHAR16 = 0x2569;
/// `BOXDRAW_VERTICAL_HORIZONTAL_DOUBLE`.
pub const BOXDRAW_VERTICAL_HORIZONTAL_DOUBLE: CHAR16 = 0x256a;
/// `BOXDRAW_VERTICAL_DOUBLE_HORIZONTAL`.
pub const BOXDRAW_VERTICAL_DOUBLE_HORIZONTAL: CHAR16 = 0x256b;
/// `BOXDRAW_DOUBLE_VERTICAL_HORIZONTAL`.
pub const BOXDRAW_DOUBLE_VERTICAL_HORIZONTAL: CHAR16 = 0x256c;
/// `BLOCKELEMENT_FULL_BLOCK`.
pub const BLOCKELEMENT_FULL_BLOCK: CHAR16 = 0x2588;
/// `BLOCKELEMENT_LIGHT_SHADE`.
pub const BLOCKELEMENT_LIGHT_SHADE: CHAR16 = 0x2591;
/// `GEOMETRICSHAPE_UP_TRIANGLE`.
pub const GEOMETRICSHAPE_UP_TRIANGLE: CHAR16 = 0x25b2;
/// `GEOMETRICSHAPE_RIGHT_TRIANGLE`.
pub const GEOMETRICSHAPE_RIGHT_TRIANGLE: CHAR16 = 0x25ba;
/// `GEOMETRICSHAPE_DOWN_TRIANGLE`.
pub const GEOMETRICSHAPE_DOWN_TRIANGLE: CHAR16 = 0x25bc;
/// `GEOMETRICSHAPE_LEFT_TRIANGLE`.
pub const GEOMETRICSHAPE_LEFT_TRIANGLE: CHAR16 = 0x25c4;
/// `ARROW_UP`.
pub const ARROW_UP: CHAR16 = 0x2191;
/// `ARROW_DOWN`.
pub const ARROW_DOWN: CHAR16 = 0x2193;
/// `CHAR_NULL`.
pub const CHAR_NULL: CHAR16 = 0x0000;
/// `CHAR_BACKSPACE`.
pub const CHAR_BACKSPACE: CHAR16 = 0x0008;
/// `CHAR_TAB`.
pub const CHAR_TAB: CHAR16 = 0x0009;
/// `CHAR_LINEFEED`.
pub const CHAR_LINEFEED: CHAR16 = 0x000A;
/// `CHAR_CARRIAGE_RETURN`.
pub const CHAR_CARRIAGE_RETURN: CHAR16 = 0x000D;
/// `SCAN_NULL`.
pub const SCAN_NULL: UINT16 = 0x0000;
/// `SCAN_UP`.
pub const SCAN_UP: UINT16 = 0x0001;
/// `SCAN_DOWN`.
pub const SCAN_DOWN: UINT16 = 0x0002;
/// `SCAN_RIGHT`.
pub const SCAN_RIGHT: UINT16 = 0x0003;
/// `SCAN_LEFT`.
pub const SCAN_LEFT: UINT16 = 0x0004;
/// `SCAN_HOME`.
pub const SCAN_HOME: UINT16 = 0x0005;
/// `SCAN_END`.
pub const SCAN_END: UINT16 = 0x0006;
/// `SCAN_INSERT`.
pub const SCAN_INSERT: UINT16 = 0x0007;
/// `SCAN_DELETE`.
pub const SCAN_DELETE: UINT16 = 0x0008;
/// `SCAN_PAGE_UP`.
pub const SCAN_PAGE_UP: UINT16 = 0x0009;
/// `SCAN_PAGE_DOWN`.
pub const SCAN_PAGE_DOWN: UINT16 = 0x000A;
/// `SCAN_F1`.
pub const SCAN_F1: UINT16 = 0x000B;
/// `SCAN_F2`.
pub const SCAN_F2: UINT16 = 0x000C;
/// `SCAN_F3`.
pub const SCAN_F3: UINT16 = 0x000D;
/// `SCAN_F4`.
pub const SCAN_F4: UINT16 = 0x000E;
/// `SCAN_F5`.
pub const SCAN_F5: UINT16 = 0x000F;
/// `SCAN_F6`.
pub const SCAN_F6: UINT16 = 0x0010;
/// `SCAN_F7`.
pub const SCAN_F7: UINT16 = 0x0011;
/// `SCAN_F8`.
pub const SCAN_F8: UINT16 = 0x0012;
/// `SCAN_F9`.
pub const SCAN_F9: UINT16 = 0x0013;
/// `SCAN_F10`.
pub const SCAN_F10: UINT16 = 0x0014;
/// `SCAN_ESC`.
pub const SCAN_ESC: UINT16 = 0x0017;

/// `EFI_TEXT_RESET`.
pub type EfiTextReset = unsafe extern "efiapi" fn(
    This: *mut SimpleTextOutputInterface,
    ExtendedVerification: BOOLEAN,
) -> EfiStatus;

/// `EFI_TEXT_OUTPUT_STRING`.
pub type EfiTextOutputString = unsafe extern "efiapi" fn(
    This: *mut SimpleTextOutputInterface,
    String: *mut CHAR16,
) -> EfiStatus;

/// `EFI_TEXT_TEST_STRING`.
pub type EfiTextTestString = unsafe extern "efiapi" fn(
    This: *mut SimpleTextOutputInterface,
    String: *mut CHAR16,
) -> EfiStatus;

/// `EFI_TEXT_QUERY_MODE`.
pub type EfiTextQueryMode = unsafe extern "efiapi" fn(
    This: *mut SimpleTextOutputInterface,
    ModeNumber: UINTN,
    Columns: *mut UINTN,
    Rows: *mut UINTN,
) -> EfiStatus;

/// `EFI_TEXT_SET_MODE`.
pub type EfiTextSetMode =
    unsafe extern "efiapi" fn(This: *mut SimpleTextOutputInterface, ModeNumber: UINTN) -> EfiStatus;

/// `EFI_TEXT_SET_ATTRIBUTE`.
pub type EfiTextSetAttribute =
    unsafe extern "efiapi" fn(This: *mut SimpleTextOutputInterface, Attribute: UINTN) -> EfiStatus;

/// `EFI_TEXT_CLEAR_SCREEN`.
pub type EfiTextClearScreen =
    unsafe extern "efiapi" fn(This: *mut SimpleTextOutputInterface) -> EfiStatus;

/// `EFI_TEXT_SET_CURSOR_POSITION`.
pub type EfiTextSetCursorPosition = unsafe extern "efiapi" fn(
    This: *mut SimpleTextOutputInterface,
    Column: UINTN,
    Row: UINTN,
) -> EfiStatus;

/// `EFI_TEXT_ENABLE_CURSOR`.
pub type EfiTextEnableCursor =
    unsafe extern "efiapi" fn(This: *mut SimpleTextOutputInterface, Enable: BOOLEAN) -> EfiStatus;

/// `SIMPLE_TEXT_OUTPUT_MODE`.
#[repr(C)]
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct SimpleTextOutputMode {
    /// Number of modes.
    pub MaxMode: INT32,
    /// Current mode.
    pub Mode: INT32,
    /// Current attribute.
    pub Attribute: INT32,
    /// Cursor column.
    pub CursorColumn: INT32,
    /// Cursor row.
    pub CursorRow: INT32,
    /// The cursor is visible.
    pub CursorVisible: BOOLEAN,
}

/// `SIMPLE_TEXT_OUTPUT_INTERFACE`: the simple text output protocol.
#[repr(C)]
#[derive(Clone, Copy)]
pub struct SimpleTextOutputInterface {
    /// `Reset`.
    pub Reset: EfiTextReset,
    /// `OutputString`.
    pub OutputString: EfiTextOutputString,
    /// `TestString`.
    pub TestString: EfiTextTestString,
    /// `QueryMode`.
    pub QueryMode: EfiTextQueryMode,
    /// `SetMode`.
    pub SetMode: EfiTextSetMode,
    /// `SetAttribute`.
    pub SetAttribute: EfiTextSetAttribute,
    /// `ClearScreen`.
    pub ClearScreen: EfiTextClearScreen,
    /// `SetCursorPosition`.
    pub SetCursorPosition: EfiTextSetCursorPosition,
    /// `EnableCursor`.
    pub EnableCursor: EfiTextEnableCursor,
    /// The current mode.
    pub Mode: *mut SimpleTextOutputMode,
}

/// `EFI_INPUT_KEY`.
#[repr(C)]
#[derive(Clone, Copy, PartialEq, Eq, Debug, Default)]
pub struct EfiInputKey {
    /// `SCAN_*` code, 0 for a character key.
    pub ScanCode: UINT16,
    /// The character, 0 for a scan-code key.
    pub UnicodeChar: CHAR16,
}

/// `EFI_INPUT_RESET`.
pub type EfiInputReset = unsafe extern "efiapi" fn(
    This: *mut SimpleInputInterface,
    ExtendedVerification: BOOLEAN,
) -> EfiStatus;

/// `EFI_INPUT_READ_KEY`.
pub type EfiInputReadKey =
    unsafe extern "efiapi" fn(This: *mut SimpleInputInterface, Key: *mut EfiInputKey) -> EfiStatus;

/// `SIMPLE_INPUT_INTERFACE`: the simple text input protocol.
#[repr(C)]
#[derive(Clone, Copy)]
pub struct SimpleInputInterface {
    /// `Reset`.
    pub Reset: EfiInputReset,
    /// `ReadKeyStroke`.
    pub ReadKeyStroke: EfiInputReadKey,
    /// `WaitForKey`: signalled when a key is available.
    pub WaitForKey: EfiEvent,
}

/// `EFI_TEXT_ATTR(f, b)`: foreground `f` and background `b` (0..7) as an attribute.
pub const fn efi_text_attr(f: UINTN, b: UINTN) -> UINTN {
    f | (b << 4)
}

const _: () = assert!(core::mem::size_of::<SimpleTextOutputMode>() == 24);
const _: () = assert!(core::mem::size_of::<SimpleTextOutputInterface>() == 80);
const _: () = assert!(core::mem::size_of::<EfiInputKey>() == 4);
const _: () = assert!(core::mem::size_of::<SimpleInputInterface>() == 24);
/* </CODE> */

/* <TESTS> */
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn text_attr_combines_colours() {
        assert_eq!(efi_text_attr(EFI_LIGHTGRAY, 0), 0x07);
        assert_eq!(efi_text_attr(EFI_WHITE, EFI_BLUE), 0x1f);
        assert_eq!(EFI_BACKGROUND_LIGHTGRAY, 0x70);
        assert_eq!(efi_text_attr(EFI_YELLOW, EFI_BACKGROUND_BLACK), 0x0e);
    }

    #[test]
    fn key_constants() {
        assert_eq!(SCAN_ESC, 0x17);
        assert_eq!(CHAR_CARRIAGE_RETURN, 0x0d);
        assert_eq!(BOXDRAW_VERTICAL_HORIZONTAL_DOUBLE, 0x256a);
    }
}
/* </TESTS> */
