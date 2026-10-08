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

//! `<dev/wscons/wsconsio.h>`, the event, keyboard and display parts: the wscons event
//! structure and event types, the keyboard ioctls of `wskbd(4)` and the display ioctls of
//! `wsdisplay(4)`, with their argument structures.
//!
//! Upstream: sys/dev/wscons/wsconsio.h @ 3ce1f3f79392
//!
//! Ioctls are all in group 'W'; numbers 0 to 31 are the keyboard's (`WSKBDIO_*`), 32 to 63
//! the mouse's, 64 to 95 the display's, 96 to 127 the mux's. This file carries all four,
//! which `hidkbd(4)`, `ukbd(4)`, `wskbd(4)`, `wsmouse(4)`, `wsmux(4)` and the frame buffers
//! (`efifb(4)`, `simplefb`, rasops and wsfont, M13) use, and the events every part shares.
//!
//! ## Deviations
//! - Status `wip`: of the display section only `WSDISPLAYIO_GPCIID` is missing: its
//!   argument, `struct pcisel`, is `<dev/pci/pciio.h>`'s, not ported.
//! - `enum wsmousecfg` is the `WSMOUSECFG_*` `i32` constants (the type of
//!   `wsmouse_param.key`); `WSMOUSECFG_MAX` is computed from them as in C.
//! - The function-like macros are the lowercase `const fn`s `is_motion_event`,
//!   `is_button_event` and `is_ctrl_event`; the `_IO*` ioctl numbers are the `const fn`s of
//!   `sys/sys/ioccom.rs`, so the argument type is part of the number, as `sizeof` is in C.
//! - Pointers to user memory in the ioctl arguments (`wskbd_map_data.map`,
//!   `wskbd_encoding_data.encodings`, `wsdisplay_cmap`'s colour arrays, `wsdisplay_cursor`'s
//!   image and mask, `wsmouse_parameters.params`) are `usize`s, with the 4 bytes of padding
//!   the C compiler leaves next to them as a named `_pad0` (`vndioctl.rs`); the structures can then be read and written as
//!   plain bytes ([`AbiPod`]). `wsdisplay_font`'s `cookie` and `data` stay pointers: the
//!   kernel's own fonts are statics that point at their glyphs (`dev/wsfont`), and the ioctls
//!   that hand a font out clear both (`rasops_list_font`).
//! - `u_int` is `u32`; the LED bits and the keyboard modes, which the C passes as `int`, are
//!   `i32`.

use core::ffi::c_void;
use core::mem::size_of;
use core::ptr;

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

// SAFETY: two `int`-sized fields and a `struct timespec` (two 64-bit integers), 24 bytes
// without padding; any bytes are a valid value.
unsafe impl AbiPod for WsconsEvent {}

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

/*
 * Mouse ioctls (32 - 63)
 */

/// `WSMOUSEIO_GTYPE`: get mouse type (`WSMOUSE_TYPE_*`).
pub const WSMOUSEIO_GTYPE: u64 = _ior::<u32>(b'W', 32);
/// `WSMOUSE_TYPE_VSXXX`: DEC serial.
pub const WSMOUSE_TYPE_VSXXX: u32 = 1;
/// `WSMOUSE_TYPE_PS2`: PS/2-compatible.
pub const WSMOUSE_TYPE_PS2: u32 = 2;
/// `WSMOUSE_TYPE_USB`: USB mouse.
pub const WSMOUSE_TYPE_USB: u32 = 3;
/// `WSMOUSE_TYPE_LMS`: Logitech busmouse.
pub const WSMOUSE_TYPE_LMS: u32 = 4;
/// `WSMOUSE_TYPE_MMS`: Microsoft InPort mouse.
pub const WSMOUSE_TYPE_MMS: u32 = 5;
/// `WSMOUSE_TYPE_TPANEL`: Generic Touch Panel.
pub const WSMOUSE_TYPE_TPANEL: u32 = 6;
/// `WSMOUSE_TYPE_NEXT`: NeXT mouse.
pub const WSMOUSE_TYPE_NEXT: u32 = 7;
/// `WSMOUSE_TYPE_ARCHIMEDES`: Archimedes mouse.
pub const WSMOUSE_TYPE_ARCHIMEDES: u32 = 8;
/// `WSMOUSE_TYPE_ADB`: ADB.
pub const WSMOUSE_TYPE_ADB: u32 = 9;
/// `WSMOUSE_TYPE_HIL`: HP HIL.
pub const WSMOUSE_TYPE_HIL: u32 = 10;
/// `WSMOUSE_TYPE_LUNA`: OMRON Luna.
pub const WSMOUSE_TYPE_LUNA: u32 = 11;
/// `WSMOUSE_TYPE_DOMAIN`: Apollo Domain.
pub const WSMOUSE_TYPE_DOMAIN: u32 = 12;
/// `WSMOUSE_TYPE_BLUETOOTH`: Bluetooth mouse.
pub const WSMOUSE_TYPE_BLUETOOTH: u32 = 13;
/// `WSMOUSE_TYPE_SUN`: SUN serial mouse.
pub const WSMOUSE_TYPE_SUN: u32 = 14;
/// `WSMOUSE_TYPE_SYNAPTICS`: Synaptics touchpad.
pub const WSMOUSE_TYPE_SYNAPTICS: u32 = 15;
/// `WSMOUSE_TYPE_ALPS`: ALPS touchpad.
pub const WSMOUSE_TYPE_ALPS: u32 = 16;
/// `WSMOUSE_TYPE_SGI`: SGI serial mouse.
pub const WSMOUSE_TYPE_SGI: u32 = 17;
/// `WSMOUSE_TYPE_ELANTECH`: Elantech touchpad.
pub const WSMOUSE_TYPE_ELANTECH: u32 = 18;
/// `WSMOUSE_TYPE_SYNAP_SBTN`: Synaptics soft buttons.
pub const WSMOUSE_TYPE_SYNAP_SBTN: u32 = 19;
/// `WSMOUSE_TYPE_TOUCHPAD`: Generic touchpad.
pub const WSMOUSE_TYPE_TOUCHPAD: u32 = 20;

/// `WSMOUSEIO_SRES`: set resolution. Not applicable to all mouse types.
pub const WSMOUSEIO_SRES: u64 = _iow::<u32>(b'W', 33);
/// `WSMOUSE_RES_MIN`.
pub const WSMOUSE_RES_MIN: u32 = 0;
/// `WSMOUSE_RES_DEFAULT`.
pub const WSMOUSE_RES_DEFAULT: u32 = 75;
/// `WSMOUSE_RES_MAX`.
pub const WSMOUSE_RES_MAX: u32 = 100;

/// `WSMOUSE_CALIBCOORDS_MAX`: the most calibration samples.
pub const WSMOUSE_CALIBCOORDS_MAX: usize = 16;
/// `WSMOUSE_CALIBCOORDS_RESET`: the `samplelen` of raw mode.
pub const WSMOUSE_CALIBCOORDS_RESET: i32 = -1;

/// `struct wsmouse_calibcoord`: one calibration sample.
#[repr(C)]
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct WsmouseCalibcoord {
    /// `rawx`: raw coordinate.
    pub rawx: i32,
    /// `rawy`.
    pub rawy: i32,
    /// `x`: translated coordinate.
    pub x: i32,
    /// `y`.
    pub y: i32,
}

// SAFETY: `#[repr(C)]`, four `int`s, no padding: every pattern is a valid value.
unsafe impl AbiPod for WsmouseCalibcoord {}

/// `struct wsmouse_calibcoords`: set/get sample coordinates for calibration.
#[repr(C)]
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct WsmouseCalibcoords {
    /// `minx`: minimum value of X.
    pub minx: i32,
    /// `miny`: minimum value of Y.
    pub miny: i32,
    /// `maxx`: maximum value of X.
    pub maxx: i32,
    /// `maxy`: maximum value of Y.
    pub maxy: i32,
    /// `swapxy`: swap X/Y axis.
    pub swapxy: i32,
    /// `resx`: X resolution.
    pub resx: i32,
    /// `resy`: Y resolution.
    pub resy: i32,
    /// `samplelen`: number of samples available or `WSMOUSE_CALIBCOORDS_RESET` for raw
    /// mode.
    pub samplelen: i32,
    /// `samples`: sample coordinates.
    pub samples: [WsmouseCalibcoord; WSMOUSE_CALIBCOORDS_MAX],
}

// SAFETY: `#[repr(C)]`, `int`s only, no padding (size checked below).
unsafe impl AbiPod for WsmouseCalibcoords {}

/// `WSMOUSEIO_SCALIBCOORDS`.
pub const WSMOUSEIO_SCALIBCOORDS: u64 = _iow::<WsmouseCalibcoords>(b'W', 36);
/// `WSMOUSEIO_GCALIBCOORDS`.
pub const WSMOUSEIO_GCALIBCOORDS: u64 = _ior::<WsmouseCalibcoords>(b'W', 37);

/// `WSMOUSEIO_SETMODE`: `WSMOUSE_COMPAT` or `WSMOUSE_NATIVE`.
pub const WSMOUSEIO_SETMODE: u64 = _iow::<i32>(b'W', 38);
/// `WSMOUSE_COMPAT`.
pub const WSMOUSE_COMPAT: i32 = 0;
/// `WSMOUSE_NATIVE`.
pub const WSMOUSE_NATIVE: i32 = 1;

// Keys of the configuration parameters in WSMOUSEIO_GETPARAMS/WSMOUSEIO_SETPARAMS calls
// (`enum wsmousecfg`). Arbitrary subsets can be passed, provided that all keys are valid and
// that the number of key/value pairs doesn't exceed WSMOUSECFG_MAX.
//
// The keys are divided into various groups, which end with marker entries of the form
// WSMOUSECFG__*.

// Coordinate handling.
/// `WSMOUSECFG_DX_SCALE`: Xscale factor in [*.12] fixed-point format.
pub const WSMOUSECFG_DX_SCALE: i32 = 0;
/// `WSMOUSECFG_DY_SCALE`: Yscale factor in [*.12] fixed-point format.
pub const WSMOUSECFG_DY_SCALE: i32 = 1;
/// `WSMOUSECFG_PRESSURE_LO`: pressure limits defining start of touch.
pub const WSMOUSECFG_PRESSURE_LO: i32 = 2;
/// `WSMOUSECFG_PRESSURE_HI`: pressure limits defining end of touch.
pub const WSMOUSECFG_PRESSURE_HI: i32 = 3;
/// `WSMOUSECFG_TRKMAXDIST`: max distance to pair points for MT contact.
pub const WSMOUSECFG_TRKMAXDIST: i32 = 4;
/// `WSMOUSECFG_SWAPXY`: swap X- and Y-axis.
pub const WSMOUSECFG_SWAPXY: i32 = 5;
/// `WSMOUSECFG_X_INV`: map absolute coordinate X to (INV - X).
pub const WSMOUSECFG_X_INV: i32 = 6;
/// `WSMOUSECFG_Y_INV`: map absolute coordinate Y to (INV - Y).
pub const WSMOUSECFG_Y_INV: i32 = 7;
/// `WSMOUSECFG_REVERSE_SCROLLING`: reverse scroll directions.
pub const WSMOUSECFG_REVERSE_SCROLLING: i32 = 8;
/// `WSMOUSECFG__FILTERS`: the end of the group.
pub const WSMOUSECFG__FILTERS: i32 = 9;

// Coordinate handling, applying only in WSMOUSE_COMPAT mode.
/// `WSMOUSECFG_DX_MAX`: ignore X deltas greater than this limit.
pub const WSMOUSECFG_DX_MAX: i32 = 32;
/// `WSMOUSECFG_DY_MAX`: ignore Y deltas greater than this limit.
pub const WSMOUSECFG_DY_MAX: i32 = 33;
/// `WSMOUSECFG_X_HYSTERESIS`: retard value for X coordinates.
pub const WSMOUSECFG_X_HYSTERESIS: i32 = 34;
/// `WSMOUSECFG_Y_HYSTERESIS`: retard value for Y coordinates.
pub const WSMOUSECFG_Y_HYSTERESIS: i32 = 35;
/// `WSMOUSECFG_DECELERATION`: threshold (distance) for deceleration.
pub const WSMOUSECFG_DECELERATION: i32 = 36;
/// `WSMOUSECFG_STRONG_HYSTERESIS`: FALSE and read-only, the feature is not supported
/// anymore.
pub const WSMOUSECFG_STRONG_HYSTERESIS: i32 = 37;
/// `WSMOUSECFG_SMOOTHING`: smoothing factor (0-7).
pub const WSMOUSECFG_SMOOTHING: i32 = 38;
/// `WSMOUSECFG__TPFILTERS`: the end of the group.
pub const WSMOUSECFG__TPFILTERS: i32 = 39;

// Touchpad features.
/// `WSMOUSECFG_SOFTBUTTONS`: 2 soft-buttons at the bottom edge.
pub const WSMOUSECFG_SOFTBUTTONS: i32 = 64;
/// `WSMOUSECFG_SOFTMBTN`: add a middle-button area.
pub const WSMOUSECFG_SOFTMBTN: i32 = 65;
/// `WSMOUSECFG_TOPBUTTONS`: 3 soft-buttons at the top edge.
pub const WSMOUSECFG_TOPBUTTONS: i32 = 66;
/// `WSMOUSECFG_TWOFINGERSCROLL`: enable two-finger scrolling.
pub const WSMOUSECFG_TWOFINGERSCROLL: i32 = 67;
/// `WSMOUSECFG_EDGESCROLL`: enable edge scrolling.
pub const WSMOUSECFG_EDGESCROLL: i32 = 68;
/// `WSMOUSECFG_HORIZSCROLL`: enable horizontal edge scrolling.
pub const WSMOUSECFG_HORIZSCROLL: i32 = 69;
/// `WSMOUSECFG_SWAPSIDES`: invert soft-button/scroll areas.
pub const WSMOUSECFG_SWAPSIDES: i32 = 70;
/// `WSMOUSECFG_DISABLE`: disable all output except for clicks in the top-button area.
pub const WSMOUSECFG_DISABLE: i32 = 71;
/// `WSMOUSECFG_MTBUTTONS`: multi-touch buttons.
pub const WSMOUSECFG_MTBUTTONS: i32 = 72;
/// `WSMOUSECFG__TPFEATURES`: the end of the group.
pub const WSMOUSECFG__TPFEATURES: i32 = 73;

// Touchpad options.
/// `WSMOUSECFG_LEFT_EDGE`: ratio: left edge / total width.
pub const WSMOUSECFG_LEFT_EDGE: i32 = 128;
/// `WSMOUSECFG_RIGHT_EDGE`: ratio: right edge / total width.
pub const WSMOUSECFG_RIGHT_EDGE: i32 = 129;
/// `WSMOUSECFG_TOP_EDGE`: ratio: top edge / total height.
pub const WSMOUSECFG_TOP_EDGE: i32 = 130;
/// `WSMOUSECFG_BOTTOM_EDGE`: ratio: bottom edge / total height.
pub const WSMOUSECFG_BOTTOM_EDGE: i32 = 131;
/// `WSMOUSECFG_CENTERWIDTH`: ratio: center width / total width.
pub const WSMOUSECFG_CENTERWIDTH: i32 = 132;
/// `WSMOUSECFG_HORIZSCROLLDIST`: distance mapped to a scroll event.
pub const WSMOUSECFG_HORIZSCROLLDIST: i32 = 133;
/// `WSMOUSECFG_VERTSCROLLDIST`: distance mapped to a scroll event.
pub const WSMOUSECFG_VERTSCROLLDIST: i32 = 134;
/// `WSMOUSECFG_F2WIDTH`: width limit for single touches.
pub const WSMOUSECFG_F2WIDTH: i32 = 135;
/// `WSMOUSECFG_F2PRESSURE`: pressure limit for single touches.
pub const WSMOUSECFG_F2PRESSURE: i32 = 136;
/// `WSMOUSECFG_TAP_MAXTIME`: max. duration of tap contacts (ms).
pub const WSMOUSECFG_TAP_MAXTIME: i32 = 137;
/// `WSMOUSECFG_TAP_CLICKTIME`: time between the end of a tap and the button-up-event (ms).
pub const WSMOUSECFG_TAP_CLICKTIME: i32 = 138;
/// `WSMOUSECFG_TAP_LOCKTIME`: time between a tap-and-drag action and the button-up-event
/// (ms).
pub const WSMOUSECFG_TAP_LOCKTIME: i32 = 139;
/// `WSMOUSECFG_TAP_ONE_BTNMAP`: one-finger tap button mapping.
pub const WSMOUSECFG_TAP_ONE_BTNMAP: i32 = 140;
/// `WSMOUSECFG_TAP_TWO_BTNMAP`: two-finger tap button mapping.
pub const WSMOUSECFG_TAP_TWO_BTNMAP: i32 = 141;
/// `WSMOUSECFG_TAP_THREE_BTNMAP`: three-finger tap button mapping.
pub const WSMOUSECFG_TAP_THREE_BTNMAP: i32 = 142;
/// `WSMOUSECFG_MTBTN_MAXDIST`: MTBUTTONS: distance limit for two-finger clicks.
pub const WSMOUSECFG_MTBTN_MAXDIST: i32 = 143;
/// `WSMOUSECFG__TPSETUP`: the end of the group.
pub const WSMOUSECFG__TPSETUP: i32 = 144;

// Enable/Disable debug output.
/// `WSMOUSECFG_LOG_INPUT`.
pub const WSMOUSECFG_LOG_INPUT: i32 = 256;
/// `WSMOUSECFG_LOG_EVENTS`.
pub const WSMOUSECFG_LOG_EVENTS: i32 = 257;
/// `WSMOUSECFG__DEBUG`: the end of the group.
pub const WSMOUSECFG__DEBUG: i32 = 258;

/// `WSMOUSECFG_MAX`: the number of keys, the most pairs one call may pass.
pub const WSMOUSECFG_MAX: u32 = ((WSMOUSECFG__FILTERS - WSMOUSECFG_DX_SCALE)
    + (WSMOUSECFG__TPFILTERS - WSMOUSECFG_DX_MAX)
    + (WSMOUSECFG__TPFEATURES - WSMOUSECFG_SOFTBUTTONS)
    + (WSMOUSECFG__TPSETUP - WSMOUSECFG_LEFT_EDGE)
    + (WSMOUSECFG__DEBUG - WSMOUSECFG_LOG_INPUT)) as u32;

/// `struct wsmouse_param`: a key (`WSMOUSECFG_*`) and its value.
#[repr(C)]
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct WsmouseParam {
    /// `key`: `enum wsmousecfg`.
    pub key: i32,
    /// `value`.
    pub value: i32,
}

// SAFETY: `#[repr(C)]`, two `int`s, no padding: every pattern is a valid value.
unsafe impl AbiPod for WsmouseParam {}

/// `struct wsmouse_parameters`: the argument of `WSMOUSEIO_GETPARAMS` and
/// `WSMOUSEIO_SETPARAMS`.
#[repr(C)]
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct WsmouseParameters {
    /// `params`: a user address of `nparams` `struct wsmouse_param`s.
    pub params: usize,
    /// `nparams`.
    pub nparams: u32,
    /// The four bytes of padding the C compiler puts after `nparams`.
    pub _pad0: [u8; 4],
}

// SAFETY: `#[repr(C)]`, integers only, the padding a named field (size checked below).
unsafe impl AbiPod for WsmouseParameters {}

/// `WSMOUSEIO_GETPARAMS`.
pub const WSMOUSEIO_GETPARAMS: u64 = _iow::<WsmouseParameters>(b'W', 39);
/// `WSMOUSEIO_SETPARAMS`.
pub const WSMOUSEIO_SETPARAMS: u64 = _iow::<WsmouseParameters>(b'W', 40);

/*
 * Display ioctls (64 - 95)
 */

/// `WSDISPLAYIO_GTYPE`: get the display type (`WSDISPLAY_TYPE_*`).
pub const WSDISPLAYIO_GTYPE: u64 = _ior::<u32>(b'W', 64);
/// `WSDISPLAY_TYPE_UNKNOWN`: unknown.
pub const WSDISPLAY_TYPE_UNKNOWN: u32 = 0;
/// `WSDISPLAY_TYPE_PM_MONO`: DEC [23]100 mono.
pub const WSDISPLAY_TYPE_PM_MONO: u32 = 1;
/// `WSDISPLAY_TYPE_PM_COLOR`: DEC [23]100 color.
pub const WSDISPLAY_TYPE_PM_COLOR: u32 = 2;
/// `WSDISPLAY_TYPE_CFB`: DEC TC CFB (CX).
pub const WSDISPLAY_TYPE_CFB: u32 = 3;
/// `WSDISPLAY_TYPE_XCFB`: DEC `maxine' onboard fb.
pub const WSDISPLAY_TYPE_XCFB: u32 = 4;
/// `WSDISPLAY_TYPE_MFB`: DEC TC MFB (MX).
pub const WSDISPLAY_TYPE_MFB: u32 = 5;
/// `WSDISPLAY_TYPE_SFB`: DEC TC SFB (HX).
pub const WSDISPLAY_TYPE_SFB: u32 = 6;
/// `WSDISPLAY_TYPE_ISAVGA`: (generic) ISA VGA.
pub const WSDISPLAY_TYPE_ISAVGA: u32 = 7;
/// `WSDISPLAY_TYPE_PCIVGA`: (generic) PCI VGA.
pub const WSDISPLAY_TYPE_PCIVGA: u32 = 8;
/// `WSDISPLAY_TYPE_TGA`: DEC PCI TGA.
pub const WSDISPLAY_TYPE_TGA: u32 = 9;
/// `WSDISPLAY_TYPE_SFBP`: DEC TC SFB+ (HX+).
pub const WSDISPLAY_TYPE_SFBP: u32 = 10;
/// `WSDISPLAY_TYPE_PCIMISC`: (generic) PCI misc. disp..
pub const WSDISPLAY_TYPE_PCIMISC: u32 = 11;
/// `WSDISPLAY_TYPE_NEXTMONO`: NeXT mono display.
pub const WSDISPLAY_TYPE_NEXTMONO: u32 = 12;
/// `WSDISPLAY_TYPE_PX`: DEC TC PX.
pub const WSDISPLAY_TYPE_PX: u32 = 13;
/// `WSDISPLAY_TYPE_PXG`: DEC TC PXG.
pub const WSDISPLAY_TYPE_PXG: u32 = 14;
/// `WSDISPLAY_TYPE_TX`: DEC TC TX.
pub const WSDISPLAY_TYPE_TX: u32 = 15;
/// `WSDISPLAY_TYPE_HPCFB`: Handheld/PalmSize PC.
pub const WSDISPLAY_TYPE_HPCFB: u32 = 16;
/// `WSDISPLAY_TYPE_VIDC`: Acorn/ARM VIDC.
pub const WSDISPLAY_TYPE_VIDC: u32 = 17;
/// `WSDISPLAY_TYPE_SPX`: DEC SPX (VS3100/VS4000).
pub const WSDISPLAY_TYPE_SPX: u32 = 18;
/// `WSDISPLAY_TYPE_GPX`: DEC GPX (uVAX/VS2K/VS3100).
pub const WSDISPLAY_TYPE_GPX: u32 = 19;
/// `WSDISPLAY_TYPE_LCG`: DEC LCG (VS4000).
pub const WSDISPLAY_TYPE_LCG: u32 = 20;
/// `WSDISPLAY_TYPE_VAX_MONO`: DEC VS2K/VS3100 mono.
pub const WSDISPLAY_TYPE_VAX_MONO: u32 = 21;
/// `WSDISPLAY_TYPE_SB_P9100`: Tadpole SPARCbook P9100.
pub const WSDISPLAY_TYPE_SB_P9100: u32 = 22;
/// `WSDISPLAY_TYPE_EGA`: (generic) EGA.
pub const WSDISPLAY_TYPE_EGA: u32 = 23;
/// `WSDISPLAY_TYPE_DCPVR`: Dreamcast PowerVR.
pub const WSDISPLAY_TYPE_DCPVR: u32 = 24;
/// `WSDISPLAY_TYPE_SUN24`: Sun 24 bit framebuffers.
pub const WSDISPLAY_TYPE_SUN24: u32 = 25;
/// `WSDISPLAY_TYPE_SUNBW`: Sun black and white fb.
pub const WSDISPLAY_TYPE_SUNBW: u32 = 26;
/// `WSDISPLAY_TYPE_STI`: HP STI framebuffers.
pub const WSDISPLAY_TYPE_STI: u32 = 27;
/// `WSDISPLAY_TYPE_SUNCG3`: Sun cgthree.
pub const WSDISPLAY_TYPE_SUNCG3: u32 = 28;
/// `WSDISPLAY_TYPE_SUNCG6`: Sun cgsix.
pub const WSDISPLAY_TYPE_SUNCG6: u32 = 29;
/// `WSDISPLAY_TYPE_SUNFFB`: Sun creator FFB.
pub const WSDISPLAY_TYPE_SUNFFB: u32 = 30;
/// `WSDISPLAY_TYPE_SUNCG14`: Sun cgfourteen.
pub const WSDISPLAY_TYPE_SUNCG14: u32 = 31;
/// `WSDISPLAY_TYPE_SUNCG2`: Sun cgtwo.
pub const WSDISPLAY_TYPE_SUNCG2: u32 = 32;
/// `WSDISPLAY_TYPE_SUNCG4`: Sun cgfour.
pub const WSDISPLAY_TYPE_SUNCG4: u32 = 33;
/// `WSDISPLAY_TYPE_SUNCG8`: Sun cgeight.
pub const WSDISPLAY_TYPE_SUNCG8: u32 = 34;
/// `WSDISPLAY_TYPE_SUNTCX`: Sun TCX.
pub const WSDISPLAY_TYPE_SUNTCX: u32 = 35;
/// `WSDISPLAY_TYPE_AGTEN`: AG10E.
pub const WSDISPLAY_TYPE_AGTEN: u32 = 36;
/// `WSDISPLAY_TYPE_XVIDEO`: Xvideo.
pub const WSDISPLAY_TYPE_XVIDEO: u32 = 37;
/// `WSDISPLAY_TYPE_SUNCG12`: Sun cgtwelve.
pub const WSDISPLAY_TYPE_SUNCG12: u32 = 38;
/// `WSDISPLAY_TYPE_MGX`: SMS MGX.
pub const WSDISPLAY_TYPE_MGX: u32 = 39;
/// `WSDISPLAY_TYPE_SB_P9000`: Tadpole SPARCbook P9000.
pub const WSDISPLAY_TYPE_SB_P9000: u32 = 40;
/// `WSDISPLAY_TYPE_RFLEX`: RasterFlex series.
pub const WSDISPLAY_TYPE_RFLEX: u32 = 41;
/// `WSDISPLAY_TYPE_LUNA`: OMRON Luna.
pub const WSDISPLAY_TYPE_LUNA: u32 = 42;
/// `WSDISPLAY_TYPE_DVBOX`: HP DaVinci.
pub const WSDISPLAY_TYPE_DVBOX: u32 = 43;
/// `WSDISPLAY_TYPE_GBOX`: HP Gatorbox.
pub const WSDISPLAY_TYPE_GBOX: u32 = 44;
/// `WSDISPLAY_TYPE_RBOX`: HP Renaissance.
pub const WSDISPLAY_TYPE_RBOX: u32 = 45;
/// `WSDISPLAY_TYPE_HYPERION`: HP Hyperion.
pub const WSDISPLAY_TYPE_HYPERION: u32 = 46;
/// `WSDISPLAY_TYPE_TOPCAT`: HP Topcat.
pub const WSDISPLAY_TYPE_TOPCAT: u32 = 47;
/// `WSDISPLAY_TYPE_PXALCD`: PXALCD (Zaurus).
pub const WSDISPLAY_TYPE_PXALCD: u32 = 48;
/// `WSDISPLAY_TYPE_MAC68K`: Generic mac68k framebuffer.
pub const WSDISPLAY_TYPE_MAC68K: u32 = 49;
/// `WSDISPLAY_TYPE_SUNLEO`: Sun ZX/Leo.
pub const WSDISPLAY_TYPE_SUNLEO: u32 = 50;
/// `WSDISPLAY_TYPE_TVRX`: HP TurboVRX.
pub const WSDISPLAY_TYPE_TVRX: u32 = 51;
/// `WSDISPLAY_TYPE_CFXGA`: CF VoyagerVGA.
pub const WSDISPLAY_TYPE_CFXGA: u32 = 52;
/// `WSDISPLAY_TYPE_LCSPX`: DEC LCSPX (VS4000).
pub const WSDISPLAY_TYPE_LCSPX: u32 = 53;
/// `WSDISPLAY_TYPE_GBE`: SGI GBE frame buffer.
pub const WSDISPLAY_TYPE_GBE: u32 = 54;
/// `WSDISPLAY_TYPE_LEGSS`: DEC LEGSS (VS35x0).
pub const WSDISPLAY_TYPE_LEGSS: u32 = 55;
/// `WSDISPLAY_TYPE_IFB`: Sun Expert3D{,-Lite}.
pub const WSDISPLAY_TYPE_IFB: u32 = 56;
/// `WSDISPLAY_TYPE_RAPTOR`: Tech Source Raptor.
pub const WSDISPLAY_TYPE_RAPTOR: u32 = 57;
/// `WSDISPLAY_TYPE_DL`: DisplayLink DL-120/DL-160.
pub const WSDISPLAY_TYPE_DL: u32 = 58;
/// `WSDISPLAY_TYPE_MACHFB`: Sun PGX/PGX64.
pub const WSDISPLAY_TYPE_MACHFB: u32 = 59;
/// `WSDISPLAY_TYPE_GFXP`: Sun PGX32.
pub const WSDISPLAY_TYPE_GFXP: u32 = 60;
/// `WSDISPLAY_TYPE_RADEONFB`: Sun XVR-100.
pub const WSDISPLAY_TYPE_RADEONFB: u32 = 61;
/// `WSDISPLAY_TYPE_SMFB`: SiliconMotion SM712.
pub const WSDISPLAY_TYPE_SMFB: u32 = 62;
/// `WSDISPLAY_TYPE_SISFB`: SiS 315 Pro.
pub const WSDISPLAY_TYPE_SISFB: u32 = 63;
/// `WSDISPLAY_TYPE_ODYSSEY`: SGI Odyssey.
pub const WSDISPLAY_TYPE_ODYSSEY: u32 = 64;
/// `WSDISPLAY_TYPE_IMPACT`: SGI Impact.
pub const WSDISPLAY_TYPE_IMPACT: u32 = 65;
/// `WSDISPLAY_TYPE_GRTWO`: SGI GR2.
pub const WSDISPLAY_TYPE_GRTWO: u32 = 66;
/// `WSDISPLAY_TYPE_NEWPORT`: SGI Newport.
pub const WSDISPLAY_TYPE_NEWPORT: u32 = 67;
/// `WSDISPLAY_TYPE_LIGHT`: SGI Light.
pub const WSDISPLAY_TYPE_LIGHT: u32 = 68;
/// `WSDISPLAY_TYPE_INTELDRM`: Intel KMS framebuffer.
pub const WSDISPLAY_TYPE_INTELDRM: u32 = 69;
/// `WSDISPLAY_TYPE_RADEONDRM`: ATI Radeon KMS framebuffer.
pub const WSDISPLAY_TYPE_RADEONDRM: u32 = 70;
/// `WSDISPLAY_TYPE_EFIFB`: EFI framebuffer.
pub const WSDISPLAY_TYPE_EFIFB: u32 = 71;
/// `WSDISPLAY_TYPE_KMS`: Generic KMS framebuffer.
pub const WSDISPLAY_TYPE_KMS: u32 = 72;
/// `WSDISPLAY_TYPE_ASTFB`: AST framebuffer.
pub const WSDISPLAY_TYPE_ASTFB: u32 = 73;
/// `WSDISPLAY_TYPE_VIOGPU`: VirtIO GPU.
pub const WSDISPLAY_TYPE_VIOGPU: u32 = 74;

/// `struct wsdisplay_fbinfo`: basic display information. Not applicable to all display
/// types.
#[repr(C)]
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct WsdisplayFbinfo {
    /// `height`: height in pixels.
    pub height: u32,
    /// `width`: width in pixels.
    pub width: u32,
    /// `depth`: bits per pixel.
    pub depth: u32,
    /// `stride`: bytes per line.
    pub stride: u32,
    /// `offset`: first pixel offset (bytes).
    pub offset: u32,
    /// `cmsize`: color map size (entries).
    pub cmsize: u32,
}

// SAFETY: six `u_int`s, no padding; any bytes are a valid value.
unsafe impl AbiPod for WsdisplayFbinfo {}

/// `WSDISPLAYIO_GINFO`.
pub const WSDISPLAYIO_GINFO: u64 = _ior::<WsdisplayFbinfo>(b'W', 65);

/// `struct wsdisplay_cmap`: colormap operations. Not applicable to all display types.
#[repr(C)]
#[derive(Clone, Copy, Debug, Default)]
pub struct WsdisplayCmap {
    /// `index`: first element (0 origin).
    pub index: u32,
    /// `count`: number of elements.
    pub count: u32,
    /// `red`: red color map elements (a user address).
    pub red: usize,
    /// `green`: green color map elements (a user address).
    pub green: usize,
    /// `blue`: blue color map elements (a user address).
    pub blue: usize,
}

// SAFETY: two `u_int`s and three pointer-sized words, no padding; any bytes are valid.
unsafe impl AbiPod for WsdisplayCmap {}

/// `WSDISPLAYIO_GETCMAP`.
pub const WSDISPLAYIO_GETCMAP: u64 = _iow::<WsdisplayCmap>(b'W', 66);
/// `WSDISPLAYIO_PUTCMAP`.
pub const WSDISPLAYIO_PUTCMAP: u64 = _iow::<WsdisplayCmap>(b'W', 67);

/// `WSDISPLAYIO_GVIDEO`: video control. Not applicable to all display types.
pub const WSDISPLAYIO_GVIDEO: u64 = _ior::<u32>(b'W', 68);
/// `WSDISPLAYIO_SVIDEO`.
pub const WSDISPLAYIO_SVIDEO: u64 = _iow::<u32>(b'W', 69);
/// `WSDISPLAYIO_VIDEO_OFF`: video off.
pub const WSDISPLAYIO_VIDEO_OFF: u32 = 0;
/// `WSDISPLAYIO_VIDEO_ON`: video on.
pub const WSDISPLAYIO_VIDEO_ON: u32 = 1;

/// `struct wsdisplay_curpos`: cursor "position".
#[repr(C)]
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct WsdisplayCurpos {
    /// `x`.
    pub x: u32,
    /// `y`.
    pub y: u32,
}

// SAFETY: two `u_int`s, no padding; any bytes are a valid value.
unsafe impl AbiPod for WsdisplayCurpos {}

/// `struct wsdisplay_cursor`: cursor control. Not applicable to all display types.
#[repr(C)]
#[derive(Clone, Copy, Debug, Default)]
pub struct WsdisplayCursor {
    /// `which`: values to get/set (`WSDISPLAY_CURSOR_DO*`).
    pub which: u32,
    /// `enable`: enable/disable.
    pub enable: u32,
    /// `pos`: position.
    pub pos: WsdisplayCurpos,
    /// `hot`: hot spot.
    pub hot: WsdisplayCurpos,
    /// `cmap`: color map info.
    pub cmap: WsdisplayCmap,
    /// `size`: bit map size.
    pub size: WsdisplayCurpos,
    /// `image`: image data (a user address).
    pub image: usize,
    /// `mask`: mask data (a user address).
    pub mask: usize,
}

// SAFETY: `u_int`s, the structures above and pointer-sized words, laid out without padding
// (every member is a multiple of 8 bytes from the start); any bytes are a valid value.
unsafe impl AbiPod for WsdisplayCursor {}

/// `WSDISPLAY_CURSOR_DOCUR`: get/set enable.
pub const WSDISPLAY_CURSOR_DOCUR: u32 = 0x01;
/// `WSDISPLAY_CURSOR_DOPOS`: get/set pos.
pub const WSDISPLAY_CURSOR_DOPOS: u32 = 0x02;
/// `WSDISPLAY_CURSOR_DOHOT`: get/set hot spot.
pub const WSDISPLAY_CURSOR_DOHOT: u32 = 0x04;
/// `WSDISPLAY_CURSOR_DOCMAP`: get/set cmap.
pub const WSDISPLAY_CURSOR_DOCMAP: u32 = 0x08;
/// `WSDISPLAY_CURSOR_DOSHAPE`: get/set img/mask.
pub const WSDISPLAY_CURSOR_DOSHAPE: u32 = 0x10;
/// `WSDISPLAY_CURSOR_DOALL`: all of the above.
pub const WSDISPLAY_CURSOR_DOALL: u32 = 0x1f;

/// `WSDISPLAYIO_GCURPOS`: cursor control: get position.
pub const WSDISPLAYIO_GCURPOS: u64 = _ior::<WsdisplayCurpos>(b'W', 70);
/// `WSDISPLAYIO_SCURPOS`: cursor control: set position.
pub const WSDISPLAYIO_SCURPOS: u64 = _iow::<WsdisplayCurpos>(b'W', 71);
/// `WSDISPLAYIO_GCURMAX`: cursor control: get maximum size.
pub const WSDISPLAYIO_GCURMAX: u64 = _ior::<WsdisplayCurpos>(b'W', 72);
/// `WSDISPLAYIO_GCURSOR`: cursor control: get cursor attributes/shape.
pub const WSDISPLAYIO_GCURSOR: u64 = _iowr::<WsdisplayCursor>(b'W', 73);
/// `WSDISPLAYIO_SCURSOR`: cursor control: set cursor attributes/shape.
pub const WSDISPLAYIO_SCURSOR: u64 = _iow::<WsdisplayCursor>(b'W', 74);

/// `WSDISPLAYIO_GMODE`: display mode: emulation (text) vs. mapped (graphics) mode.
pub const WSDISPLAYIO_GMODE: u64 = _ior::<u32>(b'W', 75);
/// `WSDISPLAYIO_SMODE`.
pub const WSDISPLAYIO_SMODE: u64 = _iow::<u32>(b'W', 76);
/// `WSDISPLAYIO_MODE_EMUL`: emulation (text) mode.
pub const WSDISPLAYIO_MODE_EMUL: u32 = 0;
/// `WSDISPLAYIO_MODE_MAPPED`: mapped (graphics) mode.
pub const WSDISPLAYIO_MODE_MAPPED: u32 = 1;
/// `WSDISPLAYIO_MODE_DUMBFB`: mapped (graphics) fb mode.
pub const WSDISPLAYIO_MODE_DUMBFB: u32 = 2;

/// `struct wsdisplay_font`: a raster font, the kernel's (`wsfont(9)`'s list, a font a
/// display uses) and the argument of the font ioctls.
///
/// The glyphs are `numchars` cells of `fontheight` rows of `stride` bytes, starting at
/// `firstchar`; `cookie` and `data` are kernel pointers (the ioctls that hand a font out
/// clear them).
#[repr(C)]
#[derive(Clone, Copy, Debug)]
pub struct WsdisplayFont {
    /// `name`.
    pub name: [u8; WSFONT_NAME_SIZE],
    /// `index`.
    pub index: i32,
    /// `firstchar`.
    pub firstchar: i32,
    /// `numchars`.
    pub numchars: i32,
    /// `encoding`: `WSDISPLAY_FONTENC_*`.
    pub encoding: i32,
    /// `fontwidth`.
    pub fontwidth: u32,
    /// `fontheight`.
    pub fontheight: u32,
    /// `stride`.
    pub stride: u32,
    /// `bitorder`: `WSDISPLAY_FONTORDER_*`.
    pub bitorder: i32,
    /// `byteorder`: `WSDISPLAY_FONTORDER_*`.
    pub byteorder: i32,
    /// The four bytes the C compiler leaves before the pointers.
    pub _pad0: u32,
    /// `cookie`.
    pub cookie: *mut c_void,
    /// `data`: the glyphs.
    pub data: *mut c_void,
}

impl WsdisplayFont {
    /// A font with no glyphs and every field zero (`memset(font, 0, sizeof(*font))`).
    pub const fn zeroed() -> Self {
        Self {
            name: [0; WSFONT_NAME_SIZE],
            index: 0,
            firstchar: 0,
            numchars: 0,
            encoding: 0,
            fontwidth: 0,
            fontheight: 0,
            stride: 0,
            bitorder: 0,
            byteorder: 0,
            _pad0: 0,
            cookie: ptr::null_mut(),
            data: ptr::null_mut(),
        }
    }

    /// The font's name, up to its NUL.
    pub fn name(&self) -> &[u8] {
        let len = self
            .name
            .iter()
            .position(|&c| c == 0)
            .unwrap_or(self.name.len());
        &self.name[..len]
    }
}

// SAFETY: a font is plain data plus two pointers the kernel owns; whoever changes a font
// (wsfont_lock's bit and byte reversal) does so with no lock held on it, as in C.
unsafe impl Send for WsdisplayFont {}

// SAFETY: integers, a byte array and two pointers, the padding named (`_pad0`); any bytes
// are a valid value (the pointers are never dereferenced from an ioctl argument).
unsafe impl AbiPod for WsdisplayFont {}

/// `WSDISPLAY_MAXFONTCOUNT`.
pub const WSDISPLAY_MAXFONTCOUNT: i32 = 8;
/// `WSDISPLAY_FONTENC_ISO`.
pub const WSDISPLAY_FONTENC_ISO: i32 = 0;
/// `WSDISPLAY_FONTENC_IBM`.
pub const WSDISPLAY_FONTENC_IBM: i32 = 1;
/// `WSDISPLAY_MAXFONTSZ`.
pub const WSDISPLAY_MAXFONTSZ: u32 = 512 * 1024;
/// `WSDISPLAY_FONTORDER_KNOWN`: i.e, no need to convert.
pub const WSDISPLAY_FONTORDER_KNOWN: i32 = 0;
/// `WSDISPLAY_FONTORDER_L2R`.
pub const WSDISPLAY_FONTORDER_L2R: i32 = 1;
/// `WSDISPLAY_FONTORDER_R2L`.
pub const WSDISPLAY_FONTORDER_R2L: i32 = 2;

/// `WSDISPLAYIO_LDFONT`.
pub const WSDISPLAYIO_LDFONT: u64 = _iow::<WsdisplayFont>(b'W', 77);
/// `WSDISPLAYIO_LSFONT`.
pub const WSDISPLAYIO_LSFONT: u64 = _iowr::<WsdisplayFont>(b'W', 78);
/// `WSDISPLAYIO_DELFONT`.
pub const WSDISPLAYIO_DELFONT: u64 = _iow::<WsdisplayFont>(b'W', 79);
/// `WSDISPLAYIO_USEFONT`.
pub const WSDISPLAYIO_USEFONT: u64 = _iow::<WsdisplayFont>(b'W', 80);

/// `struct wsdisplay_burner`.
#[repr(C)]
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct WsdisplayBurner {
    /// `off`.
    pub off: u32,
    /// `on`.
    pub on: u32,
    /// `flags`: `WSDISPLAY_BURN_*`.
    pub flags: u32,
}

// SAFETY: three `u_int`s, no padding; any bytes are a valid value.
unsafe impl AbiPod for WsdisplayBurner {}

/// `WSDISPLAY_BURN_VBLANK`.
pub const WSDISPLAY_BURN_VBLANK: u32 = 0x0001;
/// `WSDISPLAY_BURN_KBD`.
pub const WSDISPLAY_BURN_KBD: u32 = 0x0002;
/// `WSDISPLAY_BURN_MOUSE`.
pub const WSDISPLAY_BURN_MOUSE: u32 = 0x0004;
/// `WSDISPLAY_BURN_OUTPUT`.
pub const WSDISPLAY_BURN_OUTPUT: u32 = 0x0008;

/// `WSDISPLAYIO_SBURNER`.
pub const WSDISPLAYIO_SBURNER: u64 = _iow::<WsdisplayBurner>(b'W', 81);
/// `WSDISPLAYIO_GBURNER`.
pub const WSDISPLAYIO_GBURNER: u64 = _ior::<WsdisplayBurner>(b'W', 82);

/// `struct wsdisplay_addscreendata` (the C marks these definitions very preliminary).
#[repr(C)]
#[derive(Clone, Copy, Debug, Default)]
pub struct WsdisplayAddscreendata {
    /// `idx`: screen index.
    pub idx: i32,
    /// `screentype`.
    pub screentype: [u8; WSSCREEN_NAME_SIZE],
    /// `emul`.
    pub emul: [u8; WSEMUL_NAME_SIZE],
}

// SAFETY: an `int` and two byte arrays, 36 bytes without padding; any bytes are valid.
unsafe impl AbiPod for WsdisplayAddscreendata {}

/// `WSDISPLAYIO_ADDSCREEN`.
pub const WSDISPLAYIO_ADDSCREEN: u64 = _iow::<WsdisplayAddscreendata>(b'W', 83);

/// `struct wsdisplay_delscreendata`.
#[repr(C)]
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct WsdisplayDelscreendata {
    /// `idx`: screen index.
    pub idx: i32,
    /// `flags`: `WSDISPLAY_DELSCR_*`.
    pub flags: i32,
}

// SAFETY: two `int`s, no padding; any bytes are a valid value.
unsafe impl AbiPod for WsdisplayDelscreendata {}

/// `WSDISPLAY_DELSCR_FORCE`.
pub const WSDISPLAY_DELSCR_FORCE: i32 = 0x01;
/// `WSDISPLAY_DELSCR_QUIET`.
pub const WSDISPLAY_DELSCR_QUIET: i32 = 0x02;

/// `WSDISPLAYIO_DELSCREEN`.
pub const WSDISPLAYIO_DELSCREEN: u64 = _iow::<WsdisplayDelscreendata>(b'W', 84);
/// `WSDISPLAYIO_GETSCREEN`.
pub const WSDISPLAYIO_GETSCREEN: u64 = _iowr::<WsdisplayAddscreendata>(b'W', 85);
/// `WSDISPLAYIO_SETSCREEN`.
pub const WSDISPLAYIO_SETSCREEN: u64 = _iow::<u32>(b'W', 86);

/// `WSDISPLAYIO_LINEBYTES`: display information: number of bytes per row, may be same as
/// pixels.
pub const WSDISPLAYIO_LINEBYTES: u64 = _ior::<u32>(b'W', 95);

/// `WSDISPLAYIO_WSMOUSED`: mouse console support.
pub const WSDISPLAYIO_WSMOUSED: u64 = _iow::<WsconsEvent>(b'W', 88);

/// `struct wsdisplay_param`: misc control. Not applicable to all display types.
#[repr(C)]
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct WsdisplayParam {
    /// `param`: `WSDISPLAYIO_PARAM_*`.
    pub param: i32,
    /// `min`.
    pub min: i32,
    /// `max`.
    pub max: i32,
    /// `curval`.
    pub curval: i32,
    /// `reserved`.
    pub reserved: [i32; 4],
}

// SAFETY: eight `int`s, no padding; any bytes are a valid value.
unsafe impl AbiPod for WsdisplayParam {}

/// `WSDISPLAYIO_PARAM_BACKLIGHT`.
pub const WSDISPLAYIO_PARAM_BACKLIGHT: i32 = 1;
/// `WSDISPLAYIO_PARAM_BRIGHTNESS`.
pub const WSDISPLAYIO_PARAM_BRIGHTNESS: i32 = 2;
/// `WSDISPLAYIO_PARAM_CONTRAST`.
pub const WSDISPLAYIO_PARAM_CONTRAST: i32 = 3;

/// `WSDISPLAYIO_GETPARAM`.
pub const WSDISPLAYIO_GETPARAM: u64 = _iowr::<WsdisplayParam>(b'W', 89);
/// `WSDISPLAYIO_SETPARAM`.
pub const WSDISPLAYIO_SETPARAM: u64 = _iowr::<WsdisplayParam>(b'W', 90);

/// `WSDISPLAYIO_DEPTH_1`: graphical mode control.
pub const WSDISPLAYIO_DEPTH_1: u32 = 0x1;
/// `WSDISPLAYIO_DEPTH_4`.
pub const WSDISPLAYIO_DEPTH_4: u32 = 0x2;
/// `WSDISPLAYIO_DEPTH_8`.
pub const WSDISPLAYIO_DEPTH_8: u32 = 0x4;
/// `WSDISPLAYIO_DEPTH_15`.
pub const WSDISPLAYIO_DEPTH_15: u32 = 0x8;
/// `WSDISPLAYIO_DEPTH_16`.
pub const WSDISPLAYIO_DEPTH_16: u32 = 0x10;
/// `WSDISPLAYIO_DEPTH_24_24`.
pub const WSDISPLAYIO_DEPTH_24_24: u32 = 0x20;
/// `WSDISPLAYIO_DEPTH_24_32`.
pub const WSDISPLAYIO_DEPTH_24_32: u32 = 0x40;
/// `WSDISPLAYIO_DEPTH_24`.
pub const WSDISPLAYIO_DEPTH_24: u32 = WSDISPLAYIO_DEPTH_24_24 | WSDISPLAYIO_DEPTH_24_32;
/// `WSDISPLAYIO_DEPTH_30`.
pub const WSDISPLAYIO_DEPTH_30: u32 = 0x80;

/// `WSDISPLAYIO_GETSUPPORTEDDEPTH`.
pub const WSDISPLAYIO_GETSUPPORTEDDEPTH: u64 = _ior::<u32>(b'W', 92);

/// `struct wsdisplay_gfx_mode`.
#[repr(C)]
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct WsdisplayGfxMode {
    /// `width`.
    pub width: i32,
    /// `height`.
    pub height: i32,
    /// `depth`.
    pub depth: i32,
}

// SAFETY: three `int`s, no padding; any bytes are a valid value.
unsafe impl AbiPod for WsdisplayGfxMode {}

/// `WSDISPLAYIO_SETGFXMODE`.
pub const WSDISPLAYIO_SETGFXMODE: u64 = _iow::<WsdisplayGfxMode>(b'W', 92);

/// `struct wsdisplay_screentype`.
#[repr(C)]
#[derive(Clone, Copy, Debug, Default)]
pub struct WsdisplayScreentype {
    /// `idx`.
    pub idx: i32,
    /// `nidx`.
    pub nidx: i32,
    /// `name`.
    pub name: [u8; WSSCREEN_NAME_SIZE],
    /// `ncols`.
    pub ncols: i32,
    /// `nrows`.
    pub nrows: i32,
    /// `fontwidth`.
    pub fontwidth: i32,
    /// `fontheight`.
    pub fontheight: i32,
}

// SAFETY: `int`s and a byte array, 40 bytes without padding; any bytes are valid.
unsafe impl AbiPod for WsdisplayScreentype {}

/// `WSDISPLAYIO_GETSCREENTYPE`.
pub const WSDISPLAYIO_GETSCREENTYPE: u64 = _iowr::<WsdisplayScreentype>(b'W', 93);

/// `struct wsdisplay_emultype`.
#[repr(C)]
#[derive(Clone, Copy, Debug, Default)]
pub struct WsdisplayEmultype {
    /// `idx`.
    pub idx: i32,
    /// `name`.
    pub name: [u8; WSSCREEN_NAME_SIZE],
}

// SAFETY: an `int` and a byte array, 20 bytes without padding; any bytes are valid.
unsafe impl AbiPod for WsdisplayEmultype {}

/// `WSDISPLAYIO_GETEMULTYPE`.
pub const WSDISPLAYIO_GETEMULTYPE: u64 = _iowr::<WsdisplayEmultype>(b'W', 94);

// XXX NOT YET DEFINED
// Mapping information retrieval.

// Mux ioctls (96 - 127)

/// `WSMUXIO_INJECTEVENT`.
pub const WSMUXIO_INJECTEVENT: u64 = _iow::<WsconsEvent>(b'W', 96);

/// `struct wsmux_device`: a device of a mux.
#[repr(C)]
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct WsmuxDevice {
    /// `type`: `WSMUX_*`.
    pub type_: i32,
    /// `idx`.
    pub idx: i32,
}

// SAFETY: two `int`s, no padding; any bytes are a valid value.
unsafe impl AbiPod for WsmuxDevice {}

/// `WSMUX_MOUSE`.
pub const WSMUX_MOUSE: i32 = 1;
/// `WSMUX_KBD`.
pub const WSMUX_KBD: i32 = 2;
/// `WSMUX_MUX`.
pub const WSMUX_MUX: i32 = 3;

/// `WSMUXIO_ADD_DEVICE`.
pub const WSMUXIO_ADD_DEVICE: u64 = _iow::<WsmuxDevice>(b'W', 97);
/// `WSMUXIO_REMOVE_DEVICE`.
pub const WSMUXIO_REMOVE_DEVICE: u64 = _iow::<WsmuxDevice>(b'W', 98);

/// `WSMUX_MAXDEV`.
pub const WSMUX_MAXDEV: usize = 32;

/// `struct wsmux_device_list`.
#[repr(C)]
#[derive(Clone, Copy, Debug)]
pub struct WsmuxDeviceList {
    /// `ndevices`.
    pub ndevices: i32,
    /// `devices`.
    pub devices: [WsmuxDevice; WSMUX_MAXDEV],
}

// SAFETY: `int`s only, 260 bytes without padding; any bytes are a valid value.
unsafe impl AbiPod for WsmuxDeviceList {}

/// `WSMUXIO_LIST_DEVICES`.
pub const WSMUXIO_LIST_DEVICES: u64 = _iowr::<WsmuxDeviceList>(b'W', 99);

const _: () = {
    assert!(size_of::<WsmuxDevice>() == 8);
    assert!(size_of::<WsmuxDeviceList>() == 260);
    assert!(size_of::<WsconsEvent>() == 24);
    assert!(size_of::<WskbdBellData>() == 16);
    assert!(size_of::<WskbdKeyrepeatData>() == 12);
    assert!(size_of::<WskbdMapData>() == 16);
    assert!(size_of::<WskbdBacklight>() == 12);
    assert!(size_of::<WskbdEncodingData>() == 16);
    assert!(size_of::<WsdisplayFbinfo>() == 24);
    assert!(size_of::<WsdisplayCmap>() == 32);
    assert!(size_of::<WsdisplayCurpos>() == 8);
    assert!(size_of::<WsdisplayCursor>() == 80);
    assert!(size_of::<WsdisplayFont>() == 88);
    assert!(size_of::<WsdisplayBurner>() == 12);
    assert!(size_of::<WsdisplayAddscreendata>() == 36);
    assert!(size_of::<WsdisplayDelscreendata>() == 8);
    assert!(size_of::<WsdisplayParam>() == 32);
    assert!(size_of::<WsdisplayGfxMode>() == 12);
    assert!(size_of::<WsdisplayScreentype>() == 40);
    assert!(size_of::<WsdisplayEmultype>() == 20);
    assert!(size_of::<WsmouseCalibcoords>() == 288);
    assert!(size_of::<WsmouseParam>() == 8);
    assert!(size_of::<WsmouseParameters>() == 16);
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
        // _IOR('W', 65, struct wsdisplay_fbinfo), _IOW('W', 77, struct wsdisplay_font),
        // _IOWR('W', 89, struct wsdisplay_param), _IOR('W', 95, u_int)
        assert_eq!(WSDISPLAYIO_GINFO, 0x4018_5741);
        assert_eq!(WSDISPLAYIO_LDFONT, 0x8058_574d);
        assert_eq!(WSDISPLAYIO_GETPARAM, 0xc020_5759);
        assert_eq!(WSDISPLAYIO_LINEBYTES, 0x4004_575f);
        // _IOW('W', 97, struct wsmux_device), _IOWR('W', 99, struct wsmux_device_list),
        // _IOW('W', 96, struct wscons_event)
        assert_eq!(WSMUXIO_ADD_DEVICE, 0x8008_5761);
        assert_eq!(WSMUXIO_LIST_DEVICES, 0xc104_5763);
        assert_eq!(WSMUXIO_INJECTEVENT, 0x8018_5760);
        // _IOR('W', 32, u_int), _IOW('W', 36, struct wsmouse_calibcoords),
        // _IOW('W', 39/40, struct wsmouse_parameters)
        assert_eq!(WSMOUSEIO_GTYPE, 0x4004_5720);
        assert_eq!(WSMOUSEIO_SCALIBCOORDS, 0x8120_5724);
        assert_eq!(WSMOUSEIO_GETPARAMS, 0x8010_5727);
        assert_eq!(WSMOUSEIO_SETPARAMS, 0x8010_5728);
        assert_eq!(WSMOUSECFG_MAX, 9 + 7 + 9 + 16 + 2);
    }

    #[test]
    fn event_classes() {
        assert!(is_motion_event(WSCONS_EVENT_MOUSE_DELTA_W));
        assert!(!is_motion_event(WSCONS_EVENT_KEY_UP));
        assert!(is_button_event(WSCONS_EVENT_MOUSE_DOWN));
        assert!(is_ctrl_event(WSCONS_EVENT_WSMOUSED_OFF));
        assert!(!is_ctrl_event(WSCONS_EVENT_SYNC));
    }

    /// Every constant against `<dev/wscons/wsconsio.h>`: the first 220 lines and the
    /// display section.
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
            WSDISPLAY_TYPE_UNKNOWN,
            WSDISPLAY_TYPE_PM_MONO,
            WSDISPLAY_TYPE_PM_COLOR,
            WSDISPLAY_TYPE_CFB,
            WSDISPLAY_TYPE_XCFB,
            WSDISPLAY_TYPE_MFB,
            WSDISPLAY_TYPE_SFB,
            WSDISPLAY_TYPE_ISAVGA,
            WSDISPLAY_TYPE_PCIVGA,
            WSDISPLAY_TYPE_TGA,
            WSDISPLAY_TYPE_SFBP,
            WSDISPLAY_TYPE_PCIMISC,
            WSDISPLAY_TYPE_NEXTMONO,
            WSDISPLAY_TYPE_PX,
            WSDISPLAY_TYPE_PXG,
            WSDISPLAY_TYPE_TX,
            WSDISPLAY_TYPE_HPCFB,
            WSDISPLAY_TYPE_VIDC,
            WSDISPLAY_TYPE_SPX,
            WSDISPLAY_TYPE_GPX,
            WSDISPLAY_TYPE_LCG,
            WSDISPLAY_TYPE_VAX_MONO,
            WSDISPLAY_TYPE_SB_P9100,
            WSDISPLAY_TYPE_EGA,
            WSDISPLAY_TYPE_DCPVR,
            WSDISPLAY_TYPE_SUN24,
            WSDISPLAY_TYPE_SUNBW,
            WSDISPLAY_TYPE_STI,
            WSDISPLAY_TYPE_SUNCG3,
            WSDISPLAY_TYPE_SUNCG6,
            WSDISPLAY_TYPE_SUNFFB,
            WSDISPLAY_TYPE_SUNCG14,
            WSDISPLAY_TYPE_SUNCG2,
            WSDISPLAY_TYPE_SUNCG4,
            WSDISPLAY_TYPE_SUNCG8,
            WSDISPLAY_TYPE_SUNTCX,
            WSDISPLAY_TYPE_AGTEN,
            WSDISPLAY_TYPE_XVIDEO,
            WSDISPLAY_TYPE_SUNCG12,
            WSDISPLAY_TYPE_MGX,
            WSDISPLAY_TYPE_SB_P9000,
            WSDISPLAY_TYPE_RFLEX,
            WSDISPLAY_TYPE_LUNA,
            WSDISPLAY_TYPE_DVBOX,
            WSDISPLAY_TYPE_GBOX,
            WSDISPLAY_TYPE_RBOX,
            WSDISPLAY_TYPE_HYPERION,
            WSDISPLAY_TYPE_TOPCAT,
            WSDISPLAY_TYPE_PXALCD,
            WSDISPLAY_TYPE_MAC68K,
            WSDISPLAY_TYPE_SUNLEO,
            WSDISPLAY_TYPE_TVRX,
            WSDISPLAY_TYPE_CFXGA,
            WSDISPLAY_TYPE_LCSPX,
            WSDISPLAY_TYPE_GBE,
            WSDISPLAY_TYPE_LEGSS,
            WSDISPLAY_TYPE_IFB,
            WSDISPLAY_TYPE_RAPTOR,
            WSDISPLAY_TYPE_DL,
            WSDISPLAY_TYPE_MACHFB,
            WSDISPLAY_TYPE_GFXP,
            WSDISPLAY_TYPE_RADEONFB,
            WSDISPLAY_TYPE_SMFB,
            WSDISPLAY_TYPE_SISFB,
            WSDISPLAY_TYPE_ODYSSEY,
            WSDISPLAY_TYPE_IMPACT,
            WSDISPLAY_TYPE_GRTWO,
            WSDISPLAY_TYPE_NEWPORT,
            WSDISPLAY_TYPE_LIGHT,
            WSDISPLAY_TYPE_INTELDRM,
            WSDISPLAY_TYPE_RADEONDRM,
            WSDISPLAY_TYPE_EFIFB,
            WSDISPLAY_TYPE_KMS,
            WSDISPLAY_TYPE_ASTFB,
            WSDISPLAY_TYPE_VIOGPU,
            WSDISPLAYIO_VIDEO_OFF,
            WSDISPLAYIO_VIDEO_ON,
            WSDISPLAY_CURSOR_DOCUR,
            WSDISPLAY_CURSOR_DOPOS,
            WSDISPLAY_CURSOR_DOHOT,
            WSDISPLAY_CURSOR_DOCMAP,
            WSDISPLAY_CURSOR_DOSHAPE,
            WSDISPLAY_CURSOR_DOALL,
            WSDISPLAYIO_MODE_EMUL,
            WSDISPLAYIO_MODE_MAPPED,
            WSDISPLAYIO_MODE_DUMBFB,
            WSDISPLAY_MAXFONTCOUNT,
            WSDISPLAY_FONTENC_ISO,
            WSDISPLAY_FONTENC_IBM,
            WSDISPLAY_FONTORDER_KNOWN,
            WSDISPLAY_FONTORDER_L2R,
            WSDISPLAY_FONTORDER_R2L,
            WSDISPLAY_BURN_VBLANK,
            WSDISPLAY_BURN_KBD,
            WSDISPLAY_BURN_MOUSE,
            WSDISPLAY_BURN_OUTPUT,
            WSDISPLAY_DELSCR_FORCE,
            WSDISPLAY_DELSCR_QUIET,
            WSDISPLAYIO_PARAM_BACKLIGHT,
            WSDISPLAYIO_PARAM_BRIGHTNESS,
            WSDISPLAYIO_PARAM_CONTRAST,
            WSDISPLAYIO_DEPTH_1,
            WSDISPLAYIO_DEPTH_4,
            WSDISPLAYIO_DEPTH_8,
            WSDISPLAYIO_DEPTH_15,
            WSDISPLAYIO_DEPTH_16,
            WSDISPLAYIO_DEPTH_24_24,
            WSDISPLAYIO_DEPTH_24_32,
            WSDISPLAYIO_DEPTH_30,
            WSMUX_MOUSE,
            WSMUX_KBD,
            WSMUX_MUX,
            WSMUX_MAXDEV,
            WSMOUSE_TYPE_VSXXX,
            WSMOUSE_TYPE_PS2,
            WSMOUSE_TYPE_USB,
            WSMOUSE_TYPE_LMS,
            WSMOUSE_TYPE_MMS,
            WSMOUSE_TYPE_TPANEL,
            WSMOUSE_TYPE_NEXT,
            WSMOUSE_TYPE_ARCHIMEDES,
            WSMOUSE_TYPE_ADB,
            WSMOUSE_TYPE_HIL,
            WSMOUSE_TYPE_LUNA,
            WSMOUSE_TYPE_DOMAIN,
            WSMOUSE_TYPE_BLUETOOTH,
            WSMOUSE_TYPE_SUN,
            WSMOUSE_TYPE_SYNAPTICS,
            WSMOUSE_TYPE_ALPS,
            WSMOUSE_TYPE_SGI,
            WSMOUSE_TYPE_ELANTECH,
            WSMOUSE_TYPE_SYNAP_SBTN,
            WSMOUSE_TYPE_TOUCHPAD,
            WSMOUSE_RES_MIN,
            WSMOUSE_RES_DEFAULT,
            WSMOUSE_RES_MAX,
            WSMOUSE_CALIBCOORDS_MAX,
            WSMOUSE_CALIBCOORDS_RESET,
            WSMOUSE_COMPAT,
            WSMOUSE_NATIVE,
        );
    }

    /// `enum wsmousecfg` against the header: every enumerator in order, its value counted
    /// as the C compiler does (an explicit `= N`, or one more than the one before).
    #[test]
    #[ignore = "needs OPENBSD_SRC"]
    fn wsmousecfg_matches_the_c_enum() {
        use std::string::ToString;
        let path = crate::reftest::openbsd_src().join("sys/dev/wscons/wsconsio.h");
        let text = std::fs::read_to_string(path).unwrap();
        let start = text.find("enum wsmousecfg {").unwrap();
        let body = &text[start..start + text[start..].find("};").unwrap()];
        let (mut c, mut next) = (std::vec::Vec::new(), 0i64);
        for line in body.lines().skip(1) {
            let line = line.split("/*").next().unwrap().trim();
            let name = line.split([',', ' ', '=']).next().unwrap();
            if name.starts_with("WSMOUSECFG_") {
                if let Some((_, v)) = line.split_once('=') {
                    next = crate::reftest::parse_int(v.trim().trim_end_matches(',')).unwrap();
                }
                c.push((name.to_string(), next));
                next += 1;
            }
        }
        macro_rules! ours {
            ($($n:ident),*) => { [$((stringify!($n).to_string(), i64::from($n))),*] };
        }
        let ours = ours!(
            WSMOUSECFG_DX_SCALE,
            WSMOUSECFG_DY_SCALE,
            WSMOUSECFG_PRESSURE_LO,
            WSMOUSECFG_PRESSURE_HI,
            WSMOUSECFG_TRKMAXDIST,
            WSMOUSECFG_SWAPXY,
            WSMOUSECFG_X_INV,
            WSMOUSECFG_Y_INV,
            WSMOUSECFG_REVERSE_SCROLLING,
            WSMOUSECFG__FILTERS,
            WSMOUSECFG_DX_MAX,
            WSMOUSECFG_DY_MAX,
            WSMOUSECFG_X_HYSTERESIS,
            WSMOUSECFG_Y_HYSTERESIS,
            WSMOUSECFG_DECELERATION,
            WSMOUSECFG_STRONG_HYSTERESIS,
            WSMOUSECFG_SMOOTHING,
            WSMOUSECFG__TPFILTERS,
            WSMOUSECFG_SOFTBUTTONS,
            WSMOUSECFG_SOFTMBTN,
            WSMOUSECFG_TOPBUTTONS,
            WSMOUSECFG_TWOFINGERSCROLL,
            WSMOUSECFG_EDGESCROLL,
            WSMOUSECFG_HORIZSCROLL,
            WSMOUSECFG_SWAPSIDES,
            WSMOUSECFG_DISABLE,
            WSMOUSECFG_MTBUTTONS,
            WSMOUSECFG__TPFEATURES,
            WSMOUSECFG_LEFT_EDGE,
            WSMOUSECFG_RIGHT_EDGE,
            WSMOUSECFG_TOP_EDGE,
            WSMOUSECFG_BOTTOM_EDGE,
            WSMOUSECFG_CENTERWIDTH,
            WSMOUSECFG_HORIZSCROLLDIST,
            WSMOUSECFG_VERTSCROLLDIST,
            WSMOUSECFG_F2WIDTH,
            WSMOUSECFG_F2PRESSURE,
            WSMOUSECFG_TAP_MAXTIME,
            WSMOUSECFG_TAP_CLICKTIME,
            WSMOUSECFG_TAP_LOCKTIME,
            WSMOUSECFG_TAP_ONE_BTNMAP,
            WSMOUSECFG_TAP_TWO_BTNMAP,
            WSMOUSECFG_TAP_THREE_BTNMAP,
            WSMOUSECFG_MTBTN_MAXDIST,
            WSMOUSECFG__TPSETUP,
            WSMOUSECFG_LOG_INPUT,
            WSMOUSECFG_LOG_EVENTS,
            WSMOUSECFG__DEBUG
        );
        assert_eq!(c, ours);
    }
}
