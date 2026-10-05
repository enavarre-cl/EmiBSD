/* $OpenBSD: wsconsio.h,v 1.102 2024/09/30 01:41:49 jsg Exp $ */
/* $NetBSD: wsconsio.h,v 1.74 2005/04/28 07:15:44 martin Exp $ */
/* <LICENSES> */
/*
 * Copyright (c) 1996, 1997 Christopher G. Demetriou.  All rights reserved.
 *
 * Redistribution and use in source and binary forms, with or without
 * modification, are permitted provided that the following conditions
 * are met:
 * 1. Redistributions of source code must retain the above copyright
 *    notice, this list of conditions and the following disclaimer.
 * 2. Redistributions in binary form must reproduce the above copyright
 *    notice, this list of conditions and the following disclaimer in the
 *    documentation and/or other materials provided with the distribution.
 * 3. All advertising materials mentioning features or use of this software
 *    must display the following acknowledgement:
 *      This product includes software developed by Christopher G. Demetriou
 *	for the NetBSD Project.
 * 4. The name of the author may not be used to endorse or promote products
 *    derived from this software without specific prior written permission
 *
 * THIS SOFTWARE IS PROVIDED BY THE AUTHOR ``AS IS'' AND ANY EXPRESS OR
 * IMPLIED WARRANTIES, INCLUDING, BUT NOT LIMITED TO, THE IMPLIED WARRANTIES
 * OF MERCHANTABILITY AND FITNESS FOR A PARTICULAR PURPOSE ARE DISCLAIMED.
 * IN NO EVENT SHALL THE AUTHOR BE LIABLE FOR ANY DIRECT, INDIRECT,
 * INCIDENTAL, SPECIAL, EXEMPLARY, OR CONSEQUENTIAL DAMAGES (INCLUDING, BUT
 * NOT LIMITED TO, PROCUREMENT OF SUBSTITUTE GOODS OR SERVICES; LOSS OF USE,
 * DATA, OR PROFITS; OR BUSINESS INTERRUPTION) HOWEVER CAUSED AND ON ANY
 * THEORY OF LIABILITY, WHETHER IN CONTRACT, STRICT LIABILITY, OR TORT
 * (INCLUDING NEGLIGENCE OR OTHERWISE) ARISING IN ANY WAY OUT OF THE USE OF
 * THIS SOFTWARE, EVEN IF ADVISED OF THE POSSIBILITY OF SUCH DAMAGE.
 */
/* </LICENSES> */

//! `<dev/wscons/wsconsio.h>`, the event and keyboard half: the wscons event structure and
//! event types, and the keyboard ioctls of `wskbd(4)` with their argument structures.
//!
//! Upstream: sys/dev/wscons/wsconsio.h @ 3ce1f3f79392
//!
//! Ioctls are all in group 'W'; numbers 0 to 31 are the keyboard's (`WSKBDIO_*`), 32 to 63
//! the mouse's, 64 to 95 the display's, 96 to 127 the mux's. This file carries the first
//! group, which `hidkbd(4)`, `ukbd(4)` and `wskbd(4)` use, and the events every part shares.
//!
//! ## Deviations
//! - Status `wip`: the mouse (`WSMOUSEIO_*`, `WSMOUSE_TYPE_*`, `wsmouse_calibcoords`, ...),
//!   display (`WSDISPLAYIO_*`, `wsdisplay_*`, fonts, cursors, colour maps) and mux
//!   (`WSMUXIO_*`) sections of the header are not here: no ported code uses them yet (M13).
//! - The function-like macros are the lowercase `const fn`s `is_motion_event`,
//!   `is_button_event` and `is_ctrl_event`; the `_IO*` ioctl numbers are the `const fn`s of
//!   `sys/sys/ioccom.rs`, so the argument type is part of the number, as `sizeof` is in C.
//! - Pointers to user memory in the ioctl arguments (`wskbd_map_data.map`,
//!   `wskbd_encoding_data.encodings`) are `usize`s, with the 4 bytes of padding the C compiler
//!   leaves before them as a named `_pad0` (`vndioctl.rs`); the structures can then be read
//!   and written as plain bytes ([`AbiPod`]).
//! - `u_int` is `u32`; the LED bits and the keyboard modes, which the C passes as `int`, are
//!   `i32`.

use core::mem::size_of;

use crate::dev::wscons::wsksymvar::KbdT;
use crate::machine::copy::AbiPod;
use crate::sys::ioccom::{_io, _ior, _iow, _iowr};
use crate::sys::time::Timespec;

/// `WSSCREEN_NAME_SIZE`.
pub const WSSCREEN_NAME_SIZE: usize = 16;
/// `WSEMUL_NAME_SIZE`.
pub const WSEMUL_NAME_SIZE: usize = 16;
/// `WSFONT_NAME_SIZE`.
pub const WSFONT_NAME_SIZE: usize = 32;
/// `WSCONS_EVENT_KEY_UP`: key code.
pub const WSCONS_EVENT_KEY_UP: u32 = 1;
/// `WSCONS_EVENT_KEY_DOWN`: key code.
pub const WSCONS_EVENT_KEY_DOWN: u32 = 2;
/// `WSCONS_EVENT_ALL_KEYS_UP`: void.
pub const WSCONS_EVENT_ALL_KEYS_UP: u32 = 3;
/// `WSCONS_EVENT_MOUSE_UP`: button # (leftmost = 0).
pub const WSCONS_EVENT_MOUSE_UP: u32 = 4;
/// `WSCONS_EVENT_MOUSE_DOWN`: button # (leftmost = 0).
pub const WSCONS_EVENT_MOUSE_DOWN: u32 = 5;
/// `WSCONS_EVENT_MOUSE_DELTA_X`: X delta amount.
pub const WSCONS_EVENT_MOUSE_DELTA_X: u32 = 6;
/// `WSCONS_EVENT_MOUSE_DELTA_Y`: Y delta amount.
pub const WSCONS_EVENT_MOUSE_DELTA_Y: u32 = 7;
/// `WSCONS_EVENT_MOUSE_ABSOLUTE_X`: X location.
pub const WSCONS_EVENT_MOUSE_ABSOLUTE_X: u32 = 8;
/// `WSCONS_EVENT_MOUSE_ABSOLUTE_Y`: Y location.
pub const WSCONS_EVENT_MOUSE_ABSOLUTE_Y: u32 = 9;
/// `WSCONS_EVENT_MOUSE_DELTA_Z`: Z delta amount.
pub const WSCONS_EVENT_MOUSE_DELTA_Z: u32 = 10;
/// `WSCONS_EVENT_MOUSE_ABSOLUTE_Z`: (legacy, see below).
pub const WSCONS_EVENT_MOUSE_ABSOLUTE_Z: u32 = 11;
/// `WSCONS_EVENT_MOUSE_DELTA_W`: W delta amount.
pub const WSCONS_EVENT_MOUSE_DELTA_W: u32 = 16;
/// `WSCONS_EVENT_MOUSE_ABSOLUTE_W`: (legacy, see below).
pub const WSCONS_EVENT_MOUSE_ABSOLUTE_W: u32 = 17;
/// `WSCONS_EVENT_SYNC`.
pub const WSCONS_EVENT_SYNC: u32 = 18;
/// `WSCONS_EVENT_WSMOUSED_ON`: wsmoused(8) active.
pub const WSCONS_EVENT_WSMOUSED_ON: u32 = 12;
/// `WSCONS_EVENT_WSMOUSED_OFF`: wsmoused(8) inactive.
pub const WSCONS_EVENT_WSMOUSED_OFF: u32 = 13;
/// `WSCONS_EVENT_TOUCH_WIDTH`: contact width.
pub const WSCONS_EVENT_TOUCH_WIDTH: u32 = 24;
/// `WSCONS_EVENT_TOUCH_RESET`: (no value).
pub const WSCONS_EVENT_TOUCH_RESET: u32 = 25;
/// `WSCONS_EVENT_HSCROLL`: dx * 4096 / scroll_unit.
pub const WSCONS_EVENT_HSCROLL: u32 = 26;
/// `WSCONS_EVENT_VSCROLL`: dy * 4096 / scroll_unit.
pub const WSCONS_EVENT_VSCROLL: u32 = 27;
/// `WSKBD_TYPE_LK201`: lk-201.
pub const WSKBD_TYPE_LK201: u32 = 1;
/// `WSKBD_TYPE_LK401`: lk-401.
pub const WSKBD_TYPE_LK401: u32 = 2;
/// `WSKBD_TYPE_PC_XT`: PC-ish, XT scancode.
pub const WSKBD_TYPE_PC_XT: u32 = 3;
/// `WSKBD_TYPE_PC_AT`: PC-ish, AT scancode.
pub const WSKBD_TYPE_PC_AT: u32 = 4;
/// `WSKBD_TYPE_USB`: USB, XT scancode.
pub const WSKBD_TYPE_USB: u32 = 5;
/// `WSKBD_TYPE_NEXT`: NeXT keyboard.
pub const WSKBD_TYPE_NEXT: u32 = 6;
/// `WSKBD_TYPE_HPC_KBD`: HPC builtin keyboard.
pub const WSKBD_TYPE_HPC_KBD: u32 = 7;
/// `WSKBD_TYPE_HPC_BTN`: HPC/PsPC buttons.
pub const WSKBD_TYPE_HPC_BTN: u32 = 8;
/// `WSKBD_TYPE_ARCHIMEDES`: Archimedes keyboard.
pub const WSKBD_TYPE_ARCHIMEDES: u32 = 9;
/// `WSKBD_TYPE_ADB`: Apple ADB keyboard.
pub const WSKBD_TYPE_ADB: u32 = 10;
/// `WSKBD_TYPE_SUN`: Sun Type3/4.
pub const WSKBD_TYPE_SUN: u32 = 11;
/// `WSKBD_TYPE_SUN5`: Sun Type5.
pub const WSKBD_TYPE_SUN5: u32 = 12;
/// `WSKBD_TYPE_HIL`: HP HIL.
pub const WSKBD_TYPE_HIL: u32 = 13;
/// `WSKBD_TYPE_GSC`: HP PS/2.
pub const WSKBD_TYPE_GSC: u32 = 14;
/// `WSKBD_TYPE_LUNA`: OMRON Luna.
pub const WSKBD_TYPE_LUNA: u32 = 15;
/// `WSKBD_TYPE_ZAURUS`: Sharp Zaurus.
pub const WSKBD_TYPE_ZAURUS: u32 = 16;
/// `WSKBD_TYPE_DOMAIN`: Apollo Domain.
pub const WSKBD_TYPE_DOMAIN: u32 = 17;
/// `WSKBD_TYPE_BLUETOOTH`: Bluetooth keyboard.
pub const WSKBD_TYPE_BLUETOOTH: u32 = 18;
/// `WSKBD_TYPE_KPC`: Palm keypad.
pub const WSKBD_TYPE_KPC: u32 = 19;
/// `WSKBD_TYPE_SGI`: SGI serial keyboard.
pub const WSKBD_TYPE_SGI: u32 = 20;
/// `WSKBD_BELL_DOPITCH`: get/set pitch.
pub const WSKBD_BELL_DOPITCH: u32 = 0x1;
/// `WSKBD_BELL_DOPERIOD`: get/set period.
pub const WSKBD_BELL_DOPERIOD: u32 = 0x2;
/// `WSKBD_BELL_DOVOLUME`: get/set volume.
pub const WSKBD_BELL_DOVOLUME: u32 = 0x4;
/// `WSKBD_BELL_DOALL`: all of the above.
pub const WSKBD_BELL_DOALL: u32 = 0x7;
/// `WSKBD_KEYREPEAT_DODEL1`: get/set del1.
pub const WSKBD_KEYREPEAT_DODEL1: u32 = 0x1;
/// `WSKBD_KEYREPEAT_DODELN`: get/set delN.
pub const WSKBD_KEYREPEAT_DODELN: u32 = 0x2;
/// `WSKBD_KEYREPEAT_DOALL`: all of the above.
pub const WSKBD_KEYREPEAT_DOALL: u32 = 0x3;
/// `WSKBD_LED_CAPS`.
pub const WSKBD_LED_CAPS: i32 = 0x01;
/// `WSKBD_LED_NUM`.
pub const WSKBD_LED_NUM: i32 = 0x02;
/// `WSKBD_LED_SCROLL`.
pub const WSKBD_LED_SCROLL: i32 = 0x04;
/// `WSKBD_LED_COMPOSE`.
pub const WSKBD_LED_COMPOSE: i32 = 0x08;
/// `WSKBDIO_MAXMAPLEN`.
pub const WSKBDIO_MAXMAPLEN: u32 = 65536;
/// `WSKBD_TRANSLATED`.
pub const WSKBD_TRANSLATED: i32 = 0;
/// `WSKBD_RAW`.
pub const WSKBD_RAW: i32 = 1;

/// `WSCONS_EVENT_TOUCH_PRESSURE`: (single-)touch pressure, the legacy Z of the mouse.
pub const WSCONS_EVENT_TOUCH_PRESSURE: u32 = WSCONS_EVENT_MOUSE_ABSOLUTE_Z;
/// `WSCONS_EVENT_TOUCH_CONTACTS`: the number of contacts, the legacy W of the mouse.
pub const WSCONS_EVENT_TOUCH_CONTACTS: u32 = WSCONS_EVENT_MOUSE_ABSOLUTE_W;

/// `IS_MOTION_EVENT(type)`: whether `type` is a mouse motion event.
pub const fn is_motion_event(type_: u32) -> bool {
    type_ == WSCONS_EVENT_MOUSE_DELTA_X
        || type_ == WSCONS_EVENT_MOUSE_DELTA_Y
        || type_ == WSCONS_EVENT_MOUSE_DELTA_Z
        || type_ == WSCONS_EVENT_MOUSE_DELTA_W
}

/// `IS_BUTTON_EVENT(type)`: whether `type` is a mouse button event.
pub const fn is_button_event(type_: u32) -> bool {
    type_ == WSCONS_EVENT_MOUSE_UP || type_ == WSCONS_EVENT_MOUSE_DOWN
}

/// `IS_CTRL_EVENT(type)`: whether `type` is a `wsmoused(8)` control event.
pub const fn is_ctrl_event(type_: u32) -> bool {
    type_ == WSCONS_EVENT_WSMOUSED_ON || type_ == WSCONS_EVENT_WSMOUSED_OFF
}

/// `struct wscons_event`: the common event structure (used by keyboard and mouse).
#[repr(C)]
#[derive(Clone, Copy, Debug)]
pub struct WsconsEvent {
    /// `type`: `WSCONS_EVENT_*`.
    pub type_: u32,
    /// `value`: the event's information (see the event types).
    pub value: i32,
    /// `time`: when it happened.
    pub time: Timespec,
}

/// `struct wskbd_bell_data`: manipulate the keyboard bell.
#[repr(C)]
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct WskbdBellData {
    /// `which`: values to get/set (`WSKBD_BELL_DO*`).
    pub which: u32,
    /// `pitch`: pitch, in Hz.
    pub pitch: u32,
    /// `period`: period, in milliseconds.
    pub period: u32,
    /// `volume`: percentage of max volume.
    pub volume: u32,
}

// SAFETY: `#[repr(C)]`, four `u32`s, no padding: every pattern is a valid value.
unsafe impl AbiPod for WskbdBellData {}

/// `struct wskbd_keyrepeat_data`: manipulate the emulation key repeat settings.
#[repr(C)]
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
#[allow(non_snake_case)] // `delN`, as OpenBSD names it
pub struct WskbdKeyrepeatData {
    /// `which`: values to get/set (`WSKBD_KEYREPEAT_DO*`).
    pub which: u32,
    /// `del1`: delay before first, ms.
    pub del1: u32,
    /// `delN`: delay before rest, ms.
    pub delN: u32,
}

// SAFETY: `#[repr(C)]`, three `u32`s, no padding: every pattern is a valid value.
unsafe impl AbiPod for WskbdKeyrepeatData {}

/// `struct wskbd_map_data`: manipulate keysym groups.
#[repr(C)]
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct WskbdMapData {
    /// `maplen`: number of entries in map.
    pub maplen: u32,
    /// The four bytes of padding the C compiler puts before the pointer.
    pub _pad0: [u8; 4],
    /// `map`: map to get or set (a user address of `struct wscons_keymap`s).
    pub map: usize,
}

// SAFETY: `#[repr(C)]`, integers only, the padding a named field (size checked below).
unsafe impl AbiPod for WskbdMapData {}

/// `struct wskbd_backlight`: get/set keyboard backlight. Not applicable to all keyboard
/// types.
#[repr(C)]
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct WskbdBacklight {
    /// `min`.
    pub min: u32,
    /// `max`.
    pub max: u32,
    /// `curval`.
    pub curval: u32,
}

// SAFETY: `#[repr(C)]`, three `u32`s, no padding: every pattern is a valid value.
unsafe impl AbiPod for WskbdBacklight {}

/// `struct wskbd_encoding_data`: the layouts a keyboard offers.
#[repr(C)]
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct WskbdEncodingData {
    /// `nencodings`.
    pub nencodings: i32,
    /// The four bytes of padding the C compiler puts before the pointer.
    pub _pad0: [u8; 4],
    /// `encodings`: a user address of `kbd_t`s.
    pub encodings: usize,
}

// SAFETY: `#[repr(C)]`, integers only, the padding a named field (size checked below).
unsafe impl AbiPod for WskbdEncodingData {}

/// `WSKBDIO_GTYPE`: get keyboard type (`WSKBD_TYPE_*`).
pub const WSKBDIO_GTYPE: u64 = _ior::<u32>(b'W', 0);
/// `WSKBDIO_BELL`: ring the bell.
pub const WSKBDIO_BELL: u64 = _io(b'W', 1);
/// `WSKBDIO_COMPLEXBELL`: ring the bell with the given parameters.
pub const WSKBDIO_COMPLEXBELL: u64 = _iow::<WskbdBellData>(b'W', 2);
/// `WSKBDIO_SETBELL`.
pub const WSKBDIO_SETBELL: u64 = _iow::<WskbdBellData>(b'W', 3);
/// `WSKBDIO_GETBELL`.
pub const WSKBDIO_GETBELL: u64 = _ior::<WskbdBellData>(b'W', 4);
/// `WSKBDIO_SETDEFAULTBELL`.
pub const WSKBDIO_SETDEFAULTBELL: u64 = _iow::<WskbdBellData>(b'W', 5);
/// `WSKBDIO_GETDEFAULTBELL`.
pub const WSKBDIO_GETDEFAULTBELL: u64 = _ior::<WskbdBellData>(b'W', 6);
/// `WSKBDIO_SETKEYREPEAT`.
pub const WSKBDIO_SETKEYREPEAT: u64 = _iow::<WskbdKeyrepeatData>(b'W', 7);
/// `WSKBDIO_GETKEYREPEAT`.
pub const WSKBDIO_GETKEYREPEAT: u64 = _ior::<WskbdKeyrepeatData>(b'W', 8);
/// `WSKBDIO_SETDEFAULTKEYREPEAT`.
pub const WSKBDIO_SETDEFAULTKEYREPEAT: u64 = _iow::<WskbdKeyrepeatData>(b'W', 9);
/// `WSKBDIO_GETDEFAULTKEYREPEAT`.
pub const WSKBDIO_GETDEFAULTKEYREPEAT: u64 = _ior::<WskbdKeyrepeatData>(b'W', 10);
/// `WSKBDIO_SETLEDS`: set the keyboard LEDs (`WSKBD_LED_*`).
pub const WSKBDIO_SETLEDS: u64 = _iow::<i32>(b'W', 11);
/// `WSKBDIO_GETLEDS`: get the keyboard LEDs.
pub const WSKBDIO_GETLEDS: u64 = _ior::<i32>(b'W', 12);
/// `WSKBDIO_GETMAP`: get the keysym map.
pub const WSKBDIO_GETMAP: u64 = _iowr::<WskbdMapData>(b'W', 13);
/// `WSKBDIO_SETMAP`: set the keysym map.
pub const WSKBDIO_SETMAP: u64 = _iow::<WskbdMapData>(b'W', 14);
/// `WSKBDIO_GETENCODING`: get the layout.
pub const WSKBDIO_GETENCODING: u64 = _ior::<KbdT>(b'W', 15);
/// `WSKBDIO_SETENCODING`: set the layout.
pub const WSKBDIO_SETENCODING: u64 = _iow::<KbdT>(b'W', 16);
/// `WSKBDIO_GETBACKLIGHT`.
pub const WSKBDIO_GETBACKLIGHT: u64 = _ior::<WskbdBacklight>(b'W', 17);
/// `WSKBDIO_SETBACKLIGHT`.
pub const WSKBDIO_SETBACKLIGHT: u64 = _iow::<WskbdBacklight>(b'W', 18);
/// `WSKBDIO_SETMODE`: internal use only: translated or raw.
pub const WSKBDIO_SETMODE: u64 = _iow::<i32>(b'W', 19);
/// `WSKBDIO_GETMODE`: internal use only.
pub const WSKBDIO_GETMODE: u64 = _ior::<i32>(b'W', 20);
/// `WSKBDIO_GETENCODINGS`: the layouts the keyboard offers.
pub const WSKBDIO_GETENCODINGS: u64 = _iowr::<WskbdEncodingData>(b'W', 21);

const _: () = {
    assert!(size_of::<WsconsEvent>() == 24);
    assert!(size_of::<WskbdBellData>() == 16);
    assert!(size_of::<WskbdKeyrepeatData>() == 12);
    assert!(size_of::<WskbdMapData>() == 16);
    assert!(size_of::<WskbdBacklight>() == 12);
    assert!(size_of::<WskbdEncodingData>() == 16);
};

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn keyboard_ioctls_encode_direction_size_group_and_number() {
        // _IOR('W', 12, int): out, 4 bytes, group 'W', number 12
        assert_eq!(WSKBDIO_GETLEDS, 0x4004_570c);
        // _IOW('W', 2, struct wskbd_bell_data): in, 16 bytes
        assert_eq!(WSKBDIO_COMPLEXBELL, 0x8010_5702);
        // _IO('W', 1)
        assert_eq!(WSKBDIO_BELL, 0x2000_5701);
        // _IOWR('W', 13, struct wskbd_map_data)
        assert_eq!(WSKBDIO_GETMAP, 0xc010_570d);
        assert_eq!(WSKBDIO_SETMODE, 0x8004_5713);
    }

    #[test]
    fn event_classes() {
        assert!(is_motion_event(WSCONS_EVENT_MOUSE_DELTA_W));
        assert!(!is_motion_event(WSCONS_EVENT_KEY_UP));
        assert!(is_button_event(WSCONS_EVENT_MOUSE_DOWN));
        assert!(is_ctrl_event(WSCONS_EVENT_WSMOUSED_OFF));
        assert!(!is_ctrl_event(WSCONS_EVENT_SYNC));
    }

    /// Every constant against `<dev/wscons/wsconsio.h>`, the first 220 lines.
    #[test]
    #[ignore = "needs OPENBSD_SRC"]
    fn constants_match_the_c_header() {
        let defs = crate::reftest::defines("sys/dev/wscons/wsconsio.h");
        crate::reftest::assert_defines!(defs;
            WSSCREEN_NAME_SIZE,
            WSEMUL_NAME_SIZE,
            WSFONT_NAME_SIZE,
            WSCONS_EVENT_KEY_UP,
            WSCONS_EVENT_KEY_DOWN,
            WSCONS_EVENT_ALL_KEYS_UP,
            WSCONS_EVENT_MOUSE_UP,
            WSCONS_EVENT_MOUSE_DOWN,
            WSCONS_EVENT_MOUSE_DELTA_X,
            WSCONS_EVENT_MOUSE_DELTA_Y,
            WSCONS_EVENT_MOUSE_ABSOLUTE_X,
            WSCONS_EVENT_MOUSE_ABSOLUTE_Y,
            WSCONS_EVENT_MOUSE_DELTA_Z,
            WSCONS_EVENT_MOUSE_ABSOLUTE_Z,
            WSCONS_EVENT_MOUSE_DELTA_W,
            WSCONS_EVENT_MOUSE_ABSOLUTE_W,
            WSCONS_EVENT_SYNC,
            WSCONS_EVENT_WSMOUSED_ON,
            WSCONS_EVENT_WSMOUSED_OFF,
            WSCONS_EVENT_TOUCH_WIDTH,
            WSCONS_EVENT_TOUCH_RESET,
            WSCONS_EVENT_HSCROLL,
            WSCONS_EVENT_VSCROLL,
            WSKBD_TYPE_LK201,
            WSKBD_TYPE_LK401,
            WSKBD_TYPE_PC_XT,
            WSKBD_TYPE_PC_AT,
            WSKBD_TYPE_USB,
            WSKBD_TYPE_NEXT,
            WSKBD_TYPE_HPC_KBD,
            WSKBD_TYPE_HPC_BTN,
            WSKBD_TYPE_ARCHIMEDES,
            WSKBD_TYPE_ADB,
            WSKBD_TYPE_SUN,
            WSKBD_TYPE_SUN5,
            WSKBD_TYPE_HIL,
            WSKBD_TYPE_GSC,
            WSKBD_TYPE_LUNA,
            WSKBD_TYPE_ZAURUS,
            WSKBD_TYPE_DOMAIN,
            WSKBD_TYPE_BLUETOOTH,
            WSKBD_TYPE_KPC,
            WSKBD_TYPE_SGI,
            WSKBD_BELL_DOPITCH,
            WSKBD_BELL_DOPERIOD,
            WSKBD_BELL_DOVOLUME,
            WSKBD_BELL_DOALL,
            WSKBD_KEYREPEAT_DODEL1,
            WSKBD_KEYREPEAT_DODELN,
            WSKBD_KEYREPEAT_DOALL,
            WSKBD_LED_CAPS,
            WSKBD_LED_NUM,
            WSKBD_LED_SCROLL,
            WSKBD_LED_COMPOSE,
            WSKBDIO_MAXMAPLEN,
            WSKBD_TRANSLATED,
            WSKBD_RAW,
        );
    }
}
