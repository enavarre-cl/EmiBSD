/*	$OpenBSD: hidkbd.c,v 1.15 2024/10/21 19:05:31 miod Exp $	*/
/*      $NetBSD: ukbd.c,v 1.85 2003/03/11 16:44:00 augustss Exp $        */
/*	$OpenBSD: hidkbdsc.h,v 1.3 2022/11/09 10:05:18 robert Exp $	*/
/*	$OpenBSD: hidkbdvar.h,v 1.1 2016/01/08 15:54:13 jcs Exp $	*/
/* <LICENSES> */
/*
 * Copyright (c) 1998 The NetBSD Foundation, Inc.
 * All rights reserved.
 *
 * This code is derived from software contributed to The NetBSD Foundation
 * by Lennart Augustsson (lennart@augustsson.net) at
 * Carlstedt Research & Technology.
 *
 * Redistribution and use in source and binary forms, with or without
 * modification, are permitted provided that the following conditions
 * are met:
 * 1. Redistributions of source code must retain the above copyright
 *    notice, this list of conditions and the following disclaimer.
 * 2. Redistributions in binary form must reproduce the above copyright
 *    notice, this list of conditions and the following disclaimer in the
 *    documentation and/or other materials provided with the distribution.
 *
 * THIS SOFTWARE IS PROVIDED BY THE NETBSD FOUNDATION, INC. AND CONTRIBUTORS
 * ``AS IS'' AND ANY EXPRESS OR IMPLIED WARRANTIES, INCLUDING, BUT NOT LIMITED
 * TO, THE IMPLIED WARRANTIES OF MERCHANTABILITY AND FITNESS FOR A PARTICULAR
 * PURPOSE ARE DISCLAIMED.  IN NO EVENT SHALL THE FOUNDATION OR CONTRIBUTORS
 * BE LIABLE FOR ANY DIRECT, INDIRECT, INCIDENTAL, SPECIAL, EXEMPLARY, OR
 * CONSEQUENTIAL DAMAGES (INCLUDING, BUT NOT LIMITED TO, PROCUREMENT OF
 * SUBSTITUTE GOODS OR SERVICES; LOSS OF USE, DATA, OR PROFITS; OR BUSINESS
 * INTERRUPTION) HOWEVER CAUSED AND ON ANY THEORY OF LIABILITY, WHETHER IN
 * CONTRACT, STRICT LIABILITY, OR TORT (INCLUDING NEGLIGENCE OR OTHERWISE)
 * ARISING IN ANY WAY OUT OF THE USE OF THIS SOFTWARE, EVEN IF ADVISED OF THE
 * POSSIBILITY OF SUCH DAMAGE.
 */
/* </LICENSES> */

/* <CODE> */
//! `hidkbd`: the keyboard logic `ukbd(4)` and `ikbd(4)` wrap: parsing a keyboard's report
//! descriptor, decoding its input reports into key presses and releases, and handing them to
//! `wskbd(4)`; with `hidkbdsc.h` (the keyboard state) and `hidkbdvar.h` (the bell hook).
//!
//! Upstream: sys/dev/hid/hidkbd.c @ 3ce1f3f79392, sys/dev/hid/hidkbdsc.h @ 3ce1f3f79392,
//! sys/dev/hid/hidkbdvar.h @ 3ce1f3f79392
//!
//! A bus driver embeds a [`Hidkbd`] in its softc and calls [`hidkbd_attach`] with the report
//! descriptor of its keyboard interface. The parse finds the modifier keys (variable one-bit
//! fields on the keyboard usage page, `sc_var`), the array of key codes (`sc_keycodeloc`,
//! `sc_nkeycode`) and the LED outputs. [`hidkbd_attach_wskbd`] then offers a `wskbd` child.
//! Every input report goes to [`hidkbd_input`], which extracts the modifiers and the key
//! codes and [`hidkbd_decode`]s them against the previous report: a key code that left the
//! array is a release, one that entered it a press, a modifier bit that changed a press or a
//! release of that modifier's key. Keyboards with the "spurious button up" quirk
//! (`HIDKBD_SPUR_BUT_UP`) hold decoding for 20 ms, so that a key up followed by a key down
//! for the same key vanishes. Apple keyboards come with a `sc_munge` function
//! (`hidkbd_apple_munge` and friends) that rewrites key codes in the report first, for the
//! Fn key layers and the ISO layout.
//!
//! ## Deviations
//! - `WSDISPLAY_COMPAT_RAWKBD` is on in the GENERIC of both architectures, so its code is
//!   compiled in unconditionally: `hidkbd_trtab`, the raw mode (`sc_rawkbd`, `WSKBDIO_SETMODE`)
//!   and the raw path of `hidkbd_decode`. The translation of that path into XT scancodes is
//!   [`hidkbd_raw_translate`], a function of its own (the C does it inline), so that it is
//!   host-tested.
//! - `apple_fn_trans` is the table of the `#else` branch of `#ifdef __macppc__` (the one
//!   every architecture but macppc uses); the macppc branch is not carried, nor are the
//!   `#ifdef notyet` entries of the Apple tables, which the C never compiles.
//! - `ukbd_keydesctab` (`extern`) is `dev/usb/ukbdmap.rs`'s [`UKBD_KEYDESCTAB`], the
//!   layouts of [`UKBD_KEYMAPDATA`] (`ukbd_keymapdata`).
//! - `struct hidkbd` is `#[repr(C)]`-free but all-zero valid (every member a `Cell`, or an
//!   array of them), as a member of a `config_make_softc` softc must be ([`Hidkbd::new`] makes
//!   the same zeroes for a standalone one). `sc_var` is a `Cell<*mut HidkbdVariable>` beside
//!   `sc_nvar`, read through the slice accessor `vars()` (`docs/C_TO_RUST.md`, the `malloc`ed
//!   array idiom); `sc_device` and `sc_wskbddev` are `Cell<Option<NonNull<Device>>>`;
//!   `sc_pollchars` is a `Cell` of the whole array.
//! - `hidkbd_parse_desc` allocates `sc_var` with `M_ZERO` too (the C does not), so that the
//!   entries a descriptor with an invalid variable never fills are defined: they read as
//!   mask 0, which can never produce a key event. When that `M_NOWAIT` allocation fails the C
//!   returns success with `sc_var == NULL` and `sc_nvar > 0`, and `hidkbd_input` then
//!   dereferences NULL; here `vars()` is empty and the modifiers are ignored.
//! - Functions take byte slices where the C takes a pointer and a length (`hidkbd_input`'s
//!   report is `&mut [u8]` because `sc_munge` rewrites it in place; the Apple translation is
//!   bounds-checked against the report, which the C is not).
//! - `hidkbd_attach`'s `ENXIO` is `Err(Errno::ENXIO)`; `hidkbd_ioctl`'s 0, `-1` and errno are
//!   `Ok(true)`, `Ok(false)` and `Err` (`ttioctl`'s convention). `hidkbd_parse_desc`'s
//!   `const char *` error is `Err(&'static str)`.
//! - `hidkbd_bell_fn` and its argument are one `Option` in a `StaticCell`, written by
//!   `hidkbd_hookup_bell` and read by `hidkbd_bell`, both under the kernel lock as in C.
//! - `hidkbd_detach` also clears `sc_var`, `sc_nvar` and `sc_wskbddev` once it has freed or
//!   detached them, so that a stale pointer cannot be used.
//! - The `HIDKBD_DEBUG` tracing and `DPRINTF`s are not carried; `DIAGNOSTIC`'s attach message
//!   is behind the `diagnostic` feature.

use core::cell::Cell;
use core::ffi::c_void;
use core::mem::size_of;
use core::ptr::{self, NonNull};
use core::slice;
use core::sync::atomic::{AtomicI32, Ordering};

use libkern::StaticCell;

use crate::dev::hid::hid::{
    HIO_CONST, HIO_VARIABLE, HUL_CAPS_LOCK, HUL_COMPOSE, HUL_NUM_LOCK, HUL_SCROLL_LOCK,
    HUP_KEYBOARD, HUP_LED, HidItem, HidLocation, hid_end_parse, hid_get_data, hid_get_item,
    hid_get_usage, hid_get_usage_page, hid_input, hid_locate, hid_output, hid_start_parse,
    hid_usage2,
};
use crate::dev::usb::ukbdmap::UKBD_KEYDESCTAB;
use crate::dev::wscons::wsconsio::{
    WSCONS_EVENT_KEY_DOWN, WSCONS_EVENT_KEY_UP, WSKBD_LED_CAPS, WSKBD_LED_COMPOSE, WSKBD_LED_NUM,
    WSKBD_LED_SCROLL, WSKBD_RAW, WSKBDIO_COMPLEXBELL, WSKBDIO_GETLEDS, WSKBDIO_SETMODE,
    WskbdBellData,
};
use crate::dev::wscons::wskbd::{wskbd_input, wskbd_rawinput, wskbddevprint};
use crate::dev::wscons::wskbdvar::{WskbdAccessops, WskbddevAttachArgs};
use crate::dev::wscons::wsksymvar::{KbdT, WskbdMapdata};
use crate::kern::kern_malloc::{free, mallocarray};
use crate::kern::kern_timeout::{timeout_add_msec, timeout_set};
use crate::kern::subr_autoconf::{config_detach, config_found};
use crate::kprintf;
use crate::machine::intr::{spltty, splx};
use crate::sys::device::Device;
use crate::sys::errno::Errno;
use crate::sys::ioctl::{ioctl_arg, ioctl_ret};
use crate::sys::malloc::{M_DEVBUF, M_NOWAIT, M_ZERO};
use crate::sys::proc::Proc;
use crate::sys::timeout::Timeout;

/// `MAXKEYCODE`: the most key codes of the array one report carries.
pub const MAXKEYCODE: usize = 6;
/// `MAXVARS`: the most variable keys (modifiers) one keyboard has.
pub const MAXVARS: usize = 128;
/// `MAXKEYS`: the most events one report can decode to (`MAXVARS+2*MAXKEYCODE`).
pub const MAXKEYS: usize = MAXVARS + 2 * MAXKEYCODE;

/// `HIDKBD_SPUR_BUT_UP`: quirk: spurious button up events.
pub const HIDKBD_SPUR_BUT_UP: u32 = 0x001;

/// `PRESS`: an event word for a key press (the key code in the low byte).
const PRESS: u16 = 0x000;
/// `RELEASE`: an event word for a key release.
const RELEASE: u16 = 0x100;
/// `CODEMASK`: the key code of an event word.
const CODEMASK: u16 = 0x0ff;

/// `NN`: no translation (an entry of `hidkbd_trtab`).
const NN: u8 = 0;

/// `hidkbd_trtab`: translate USB key codes to US keyboard XT scancodes. Scancodes >= 0x80
/// represent EXTENDED keycodes (see <http://www.microsoft.com/whdc/archive/Scancode.mspx>).
/// Used by the raw mode, `WSDISPLAY_COMPAT_RAWKBD`.
#[rustfmt::skip]
pub static HIDKBD_TRTAB: [u8; 256] = [
    NN, NN, NN, NN, 0x1e, 0x30, 0x2e, 0x20, // 00
    0x12, 0x21, 0x22, 0x23, 0x17, 0x24, 0x25, 0x26, // 08
    0x32, 0x31, 0x18, 0x19, 0x10, 0x13, 0x1f, 0x14, // 10
    0x16, 0x2f, 0x11, 0x2d, 0x15, 0x2c, 0x02, 0x03, // 18
    0x04, 0x05, 0x06, 0x07, 0x08, 0x09, 0x0a, 0x0b, // 20
    0x1c, 0x01, 0x0e, 0x0f, 0x39, 0x0c, 0x0d, 0x1a, // 28
    0x1b, 0x2b, 0x2b, 0x27, 0x28, 0x29, 0x33, 0x34, // 30
    0x35, 0x3a, 0x3b, 0x3c, 0x3d, 0x3e, 0x3f, 0x40, // 38
    0x41, 0x42, 0x43, 0x44, 0x57, 0x58, 0xb7, 0x46, // 40
    0x7f, 0xd2, 0xc7, 0xc9, 0xd3, 0xcf, 0xd1, 0xcd, // 48
    0xcb, 0xd0, 0xc8, 0x45, 0xb5, 0x37, 0x4a, 0x4e, // 50
    0x9c, 0x4f, 0x50, 0x51, 0x4b, 0x4c, 0x4d, 0x47, // 58
    0x48, 0x49, 0x52, 0x53, 0x56, 0xdd, 0xde, 0x59, // 60
    0x64, 0x65, 0x66, 0x67, 0x68, 0x69, 0x6a, 0x6b, // 68
    0x6c, 0x6d, 0x6e, 0x76, 0x97, NN, 0x93, 0x95, // 70
    0x91, 0x92, 0x94, 0x9a, 0x96, 0x98, 0x99, 0xa0, // 78
    0xb0, 0xae, NN, NN, NN, 0x7e, NN, 0x73, // 80
    0x70, 0x7d, 0x79, 0x7b, 0x5c, NN, NN, NN, // 88
    NN, NN, 0x78, 0x77, 0x76, NN, NN, NN, // 90
    NN, NN, NN, NN, NN, NN, NN, NN, // 98
    NN, NN, NN, NN, NN, NN, NN, NN, // a0
    NN, NN, NN, NN, NN, NN, NN, NN, // a8
    NN, NN, NN, NN, NN, NN, NN, NN, // b0
    NN, NN, NN, NN, NN, NN, NN, NN, // b8
    NN, NN, NN, NN, NN, NN, NN, NN, // c0
    NN, NN, NN, NN, NN, NN, NN, NN, // c8
    NN, NN, NN, NN, NN, NN, NN, NN, // d0
    NN, NN, NN, NN, NN, NN, NN, NN, // d8
    0x1d, 0x2a, 0x38, 0xdb, 0x9d, 0x36, 0xb8, 0xdc, // e0
    NN, NN, NN, NN, NN, NN, NN, NN, // e8
    NN, NN, NN, NN, NN, NN, NN, NN, // f0
    NN, NN, NN, NN, NN, NN, NN, NN, // f8
];

/// `struct hidkbd_translation`: a key code and what it is rewritten to.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct HidkbdTranslation {
    /// `original`: the key code in the report.
    pub original: u8,
    /// `translation`: the key code it becomes.
    pub translation: u8,
}

/// A table entry: `{ original, translation }`.
const fn tr(original: u8, translation: u8) -> HidkbdTranslation {
    HidkbdTranslation {
        original,
        translation,
    }
}

/// `apple_tb_trans`: the number row to the function keys (with Fn pressed on a Tb keyboard).
static APPLE_TB_TRANS: [HidkbdTranslation; 12] = [
    tr(30, 58), // 1 -> F1
    tr(31, 59), // 2 -> F2
    tr(32, 60), // 3 -> F3
    tr(33, 61), // 4 -> F4
    tr(34, 62), // 5 -> F5
    tr(35, 63), // 6 -> F6
    tr(36, 64), // 7 -> F7
    tr(37, 65), // 8 -> F8
    tr(38, 66), // 9 -> F9
    tr(39, 67), // 0 -> F10
    tr(45, 68), // - -> F11
    tr(46, 69), // = -> F12
];

/// `apple_fn_trans`: the Fn layer of an Apple keyboard (the non-macppc branch).
static APPLE_FN_TRANS: [HidkbdTranslation; 12] = [
    tr(40, 73),  // return -> insert
    tr(42, 76),  // backspace -> delete
    tr(58, 233), // F1 -> screen brightness down
    tr(59, 232), // F2 -> screen brightness up
    tr(63, 102), // F6 -> sleep
    tr(67, 127), // F10 -> audio mute
    tr(68, 129), // F11 -> audio lower
    tr(69, 128), // F12 -> audio raise
    tr(79, 77),  // right -> end
    tr(80, 74),  // left -> home
    tr(81, 78),  // down -> page down
    tr(82, 75),  // up -> page up
];

/// `apple_mba_trans`: the Fn layer of a MacBook Air keyboard.
static APPLE_MBA_TRANS: [HidkbdTranslation; 9] = [
    tr(40, 73),  // return -> insert
    tr(42, 76),  // backspace -> delete
    tr(66, 127), // F9 -> audio mute
    tr(67, 129), // F10 -> audio lower
    tr(68, 128), // F11 -> audio raise
    tr(79, 77),  // right -> end
    tr(80, 74),  // left -> home
    tr(81, 78),  // down -> page down
    tr(82, 75),  // up -> page up
];

/// `apple_iso_trans`: the ISO layout swaps `less` and `grave`.
static APPLE_ISO_TRANS: [HidkbdTranslation; 2] = [
    tr(53, 100), // less -> grave
    tr(100, 53),
];

/// `KEY_ERROR`: a report whose first key code says the keyboard has too many keys down.
const KEY_ERROR: u8 = 0x01;

/// `struct hidkbd_variable`: one modifier: where its bit is, the bit's mask in `var[]` and
/// the key code it stands for.
#[derive(Clone, Copy, Debug, Default)]
pub struct HidkbdVariable {
    /// `loc`: where in the report.
    pub loc: HidLocation,
    /// `mask`: the bit of its `var` byte (`1 << (index % 8)`).
    pub mask: u8,
    /// `key`: the key code (the usage on the keyboard page).
    pub key: u8,
}

/// `struct hidkbd_data`: the keys down in one report: the key code array and the modifiers.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct HidkbdData {
    /// `keycode`: the array of key codes.
    pub keycode: [u8; MAXKEYCODE],
    /// `var`: the modifier values.
    pub var: [u8; MAXVARS],
}

impl HidkbdData {
    /// No key down.
    pub const fn new() -> Self {
        Self {
            keycode: [0; MAXKEYCODE],
            var: [0; MAXVARS],
        }
    }
}

impl Default for HidkbdData {
    fn default() -> Self {
        Self::new()
    }
}

/// The type of `sc_munge`: rewrites an input report before it is decoded.
pub type HidkbdMungeFn = fn(&Hidkbd, &mut [u8]);

/// `struct hidkbd`: the state of one HID keyboard, a member of its bus driver's softc.
///
/// The members change under the kernel lock (the interrupt that delivers reports, the
/// `wskbd` entry points and the debounce timeout), as in C; hence the `Cell`s. All-zero is a
/// valid value.
pub struct Hidkbd {
    /// `sc_ndata`: stored data: the report being decoded.
    pub sc_ndata: Cell<HidkbdData>,
    /// `sc_odata`: the previous report.
    pub sc_odata: Cell<HidkbdData>,

    /// `sc_nvar`: the number of input variables (modifiers).
    pub sc_nvar: Cell<u32>,
    /// `sc_var`: the input variables, `sc_nvar` entries `malloc`ed by `hidkbd_parse_desc`.
    pub sc_var: Cell<*mut HidkbdVariable>,

    /// `sc_keycodeloc`: where the key code array is.
    pub sc_keycodeloc: Cell<HidLocation>,
    /// `sc_nkeycode`: the number of key codes in it.
    pub sc_nkeycode: Cell<u32>,

    /// `sc_numloc`: the Num Lock LED output.
    pub sc_numloc: Cell<HidLocation>,
    /// `sc_capsloc`: the Caps Lock LED output.
    pub sc_capsloc: Cell<HidLocation>,
    /// `sc_scroloc`: the Scroll Lock LED output.
    pub sc_scroloc: Cell<HidLocation>,
    /// `sc_compose`: the Compose LED output.
    pub sc_compose: Cell<HidLocation>,
    /// `sc_leds`: the LEDs lit (`WSKBD_LED_*`).
    pub sc_leds: Cell<i32>,

    /// `sc_fn`: optional extra input source used by `sc_munge`.
    pub sc_fn: Cell<HidLocation>,

    /// `sc_device`: the bus driver's device.
    pub sc_device: Cell<Option<NonNull<Device>>>,
    /// `sc_wskbddev`: the `wskbd` child, if one attached.
    pub sc_wskbddev: Cell<Option<NonNull<Device>>>,
    /// `sc_enabled`.
    pub sc_enabled: Cell<i8>,

    /// `sc_console_keyboard`: we are the console keyboard.
    pub sc_console_keyboard: Cell<i8>,

    /// `sc_debounce`: for quirk handling.
    pub sc_debounce: Cell<i8>,
    /// `sc_delay`: for quirk handling.
    pub sc_delay: Timeout,
    /// `sc_data`: for quirk handling.
    pub sc_data: Cell<HidkbdData>,

    /// `sc_rawkbd`: raw mode (`WSDISPLAY_COMPAT_RAWKBD`): events are XT scancodes.
    pub sc_rawkbd: Cell<i32>,

    /// `sc_polling`: console polling is on.
    pub sc_polling: Cell<i32>,
    /// `sc_npollchar`: the number of events waiting in `sc_pollchars`.
    pub sc_npollchar: Cell<i32>,
    /// `sc_pollchars`: the events a poll decoded, for `hidkbd_cngetc`.
    pub sc_pollchars: Cell<[u16; MAXKEYS]>,

    /// `sc_munge`: rewrites a report before decoding.
    pub sc_munge: Cell<Option<HidkbdMungeFn>>,
}

impl Hidkbd {
    /// A keyboard in the all-zero state a zeroed softc has.
    pub const fn new() -> Self {
        Self {
            sc_ndata: Cell::new(HidkbdData::new()),
            sc_odata: Cell::new(HidkbdData::new()),
            sc_nvar: Cell::new(0),
            sc_var: Cell::new(ptr::null_mut()),
            sc_keycodeloc: Cell::new(HidLocation {
                size: 0,
                count: 0,
                pos: 0,
            }),
            sc_nkeycode: Cell::new(0),
            sc_numloc: Cell::new(HidLocation {
                size: 0,
                count: 0,
                pos: 0,
            }),
            sc_capsloc: Cell::new(HidLocation {
                size: 0,
                count: 0,
                pos: 0,
            }),
            sc_scroloc: Cell::new(HidLocation {
                size: 0,
                count: 0,
                pos: 0,
            }),
            sc_compose: Cell::new(HidLocation {
                size: 0,
                count: 0,
                pos: 0,
            }),
            sc_leds: Cell::new(0),
            sc_fn: Cell::new(HidLocation {
                size: 0,
                count: 0,
                pos: 0,
            }),
            sc_device: Cell::new(None),
            sc_wskbddev: Cell::new(None),
            sc_enabled: Cell::new(0),
            sc_console_keyboard: Cell::new(0),
            sc_debounce: Cell::new(0),
            sc_delay: Timeout::zeroed(),
            sc_data: Cell::new(HidkbdData::new()),
            sc_rawkbd: Cell::new(0),
            sc_polling: Cell::new(0),
            sc_npollchar: Cell::new(0),
            sc_pollchars: Cell::new([0; MAXKEYS]),
            sc_munge: Cell::new(None),
        }
    }

    /// The input variables `hidkbd_parse_desc` found: `sc_var[0..sc_nvar]`.
    fn vars(&self) -> &[HidkbdVariable] {
        let p = self.sc_var.get();
        if p.is_null() {
            return &[];
        }
        // SAFETY: `hidkbd_parse_desc` made `sc_var` a zeroed `malloc` of `sc_nvar` entries
        // (`HidkbdVariable` is plain integers, so zeroed is valid); `hidkbd_detach` is the
        // only thing that frees it, after which nothing reads the keyboard.
        unsafe { slice::from_raw_parts(p, self.sc_nvar.get() as usize) }
    }

    /// The `wskbd` child, if one attached.
    fn wskbddev(&self) -> Option<&Device> {
        // SAFETY: `sc_wskbddev` is what `config_found` returned in `hidkbd_attach_wskbd`;
        // the child lives until `hidkbd_detach`'s `config_detach`, and nothing delivers
        // reports to a keyboard that is detaching.
        self.sc_wskbddev.get().map(|d| unsafe { d.as_ref() })
    }
}

impl Default for Hidkbd {
    fn default() -> Self {
        Self::new()
    }
}

/// `hidkbd_is_console`: whether the next keyboard to attach is the console keyboard.
#[allow(non_upper_case_globals)] // the C global's name, as `hw_vendor` and its kin
pub static hidkbd_is_console: AtomicI32 = AtomicI32::new(0);

/// `hidkbd_bell_fn` and `hidkbd_bell_fn_arg`: the bell a sound driver hooked up.
#[derive(Clone, Copy)]
struct BellHook {
    /// `hidkbd_bell_fn`.
    func: fn(arg: *mut c_void, pitch: u32, period: u32, volume: u32, poll: i32),
    /// `hidkbd_bell_fn_arg`, as the address it is.
    arg: usize,
}

/// The bell hook; written by `hidkbd_hookup_bell` (autoconfiguration) under the kernel lock.
static HIDKBD_BELL: StaticCell<Option<BellHook>> = StaticCell::new(None);

/// `ukbd_keymapdata`: the keyboard's table of layouts and the one in force.
pub static UKBD_KEYMAPDATA: WskbdMapdata = WskbdMapdata::new(&UKBD_KEYDESCTAB, 0);

/// `hidkbd_attach`: parse the report descriptor `desc` of report `id` and set the keyboard
/// up; `Err(ENXIO)` when the descriptor has no usable keys (after printing why).
pub fn hidkbd_attach(
    self_: &Device,
    kbd: &Hidkbd,
    console: i32,
    qflags: u32,
    id: i32,
    desc: &[u8],
) -> Result<(), Errno> {
    kbd.sc_var.set(ptr::null_mut());

    if let Err(parserr) = hidkbd_parse_desc(kbd, id, desc) {
        kprintf!(": {}\n", parserr);
        return Err(Errno::ENXIO);
    }

    #[cfg(feature = "diagnostic")]
    kprintf!(
        ": {} variable keys, {} key codes",
        kbd.sc_nvar.get(),
        kbd.sc_nkeycode.get()
    );

    kbd.sc_device.set(Some(NonNull::from(self_)));
    kbd.sc_debounce
        .set(i8::from(qflags & HIDKBD_SPUR_BUT_UP != 0));

    // Remember if we're the console keyboard.
    //
    // XXX This always picks the first (USB) keyboard to attach, but what else can we really
    // do?
    if console != 0 {
        kbd.sc_console_keyboard
            .set(hidkbd_is_console.load(Ordering::Relaxed) as i8);
        // Don't let any other keyboard have it.
        hidkbd_is_console.store(0, Ordering::Relaxed);
    }

    timeout_set(
        &kbd.sc_delay,
        hidkbd_delayed_decode,
        ptr::from_ref(kbd).cast_mut().cast(),
    );

    Ok(())
}

/// `hidkbd_attach_wskbd`: offer a `wskbd` child with layout `layout` and the bus driver's
/// access operations.
pub fn hidkbd_attach_wskbd(kbd: &Hidkbd, layout: KbdT, accessops: &'static WskbdAccessops) {
    UKBD_KEYMAPDATA.set_layout(layout);

    let Some(dev) = kbd.sc_device.get() else {
        return;
    };
    // SAFETY: `sc_device` is the bus driver's own device, set by `hidkbd_attach`; it is
    // attached for as long as the keyboard is.
    let dev = unsafe { dev.as_ref() };
    let mut a = WskbddevAttachArgs {
        console: i32::from(kbd.sc_console_keyboard.get()),
        keymap: &UKBD_KEYMAPDATA,
        accessops,
        accesscookie: ptr::from_ref(dev).cast_mut().cast(),
        audiocookie: ptr::null_mut(),
    };
    kbd.sc_wskbddev.set(config_found(
        dev,
        ptr::from_mut(&mut a).cast::<c_void>(),
        Some(wskbddevprint),
    ));
}

/// `hidkbd_detach`: detach the keyboard: give the console back, detach the `wskbd` child and
/// free the variables.
pub fn hidkbd_detach(kbd: &Hidkbd, flags: i32) -> Result<(), Errno> {
    let mut rv = Ok(());

    if kbd.sc_console_keyboard.get() != 0 {
        // Disconnect our consops and set hidkbd_is_console back to 1 so that the next USB
        // keyboard attached to the system will get it.
        // XXX Should notify some other keyboard that it can be console, if there are any
        // other keyboards.
        if let Some(dev) = kbd.sc_device.get() {
            // SAFETY: as in `hidkbd_attach_wskbd`.
            let dev = unsafe { dev.as_ref() };
            kprintf!("{}: was console keyboard\n", dev.xname());
        }
        hidkbd_is_console.store(1, Ordering::Relaxed);
    }
    // No need to do reference counting of hidkbd, wskbd has all the goo
    if let Some(wsk) = kbd.sc_wskbddev.get() {
        // SAFETY: `wsk` is the attached child `hidkbd_attach_wskbd` stored; it is not used
        // again (`sc_wskbddev` is cleared on success).
        rv = unsafe { config_detach(wsk, flags) };
        if rv.is_ok() {
            kbd.sc_wskbddev.set(None);
        }
    }

    if let Some(var) = NonNull::new(kbd.sc_var.get()) {
        free(var.cast(), M_DEVBUF, 0);
        kbd.sc_var.set(ptr::null_mut());
        kbd.sc_nvar.set(0);
    }

    rv
}

/// `hidkbd_translate`: what `table` rewrites `keycode` to; 0 if it does not.
pub fn hidkbd_translate(table: &[HidkbdTranslation], keycode: u8) -> u8 {
    for t in table {
        if t.original == keycode {
            return t.translation;
        }
    }
    0
}

/// `hidkbd_apple_translate`: rewrite the key code array of the report `ibuf` through `trans`.
pub fn hidkbd_apple_translate(kbd: &Hidkbd, ibuf: &mut [u8], trans: &[HidkbdTranslation]) {
    let spos = (kbd.sc_keycodeloc.get().pos / 8) as usize;
    let epos = spos.saturating_add(kbd.sc_nkeycode.get() as usize);

    let Some(codes) = ibuf.get_mut(spos..epos.min(ibuf.len())) else {
        return;
    };
    for pos in codes {
        let xlat = hidkbd_translate(trans, *pos);
        if xlat != 0 {
            *pos = xlat;
        }
    }
}

/// `hidkbd_apple_munge`: the Fn layer of an Apple keyboard.
pub fn hidkbd_apple_munge(kbd: &Hidkbd, ibuf: &mut [u8]) {
    if hid_get_data(ibuf, &kbd.sc_fn.get()) == 0 {
        return;
    }

    hidkbd_apple_translate(kbd, ibuf, &APPLE_FN_TRANS);
}

/// `hidkbd_apple_tb_munge`: the Fn layer of an Apple keyboard with the number row as function
/// keys.
pub fn hidkbd_apple_tb_munge(kbd: &Hidkbd, ibuf: &mut [u8]) {
    if hid_get_data(ibuf, &kbd.sc_fn.get()) == 0 {
        return;
    }

    hidkbd_apple_munge(kbd, ibuf);

    hidkbd_apple_translate(kbd, ibuf, &APPLE_TB_TRANS);
}

/// `hidkbd_apple_iso_munge`: an Apple keyboard with the ISO layout.
pub fn hidkbd_apple_iso_munge(kbd: &Hidkbd, ibuf: &mut [u8]) {
    hidkbd_apple_translate(kbd, ibuf, &APPLE_ISO_TRANS);
    hidkbd_apple_munge(kbd, ibuf);
}

/// `hidkbd_apple_mba_munge`: the Fn layer of a MacBook Air keyboard.
pub fn hidkbd_apple_mba_munge(kbd: &Hidkbd, ibuf: &mut [u8]) {
    if hid_get_data(ibuf, &kbd.sc_fn.get()) == 0 {
        return;
    }

    hidkbd_apple_translate(kbd, ibuf, &APPLE_MBA_TRANS);
}

/// `hidkbd_apple_iso_mba_munge`: a MacBook Air keyboard with the ISO layout.
pub fn hidkbd_apple_iso_mba_munge(kbd: &Hidkbd, ibuf: &mut [u8]) {
    hidkbd_apple_translate(kbd, ibuf, &APPLE_ISO_TRANS);
    hidkbd_apple_mba_munge(kbd, ibuf);
}

/// `hidkbd_input`: an input report arrived: let `sc_munge` rewrite it, extract the modifiers
/// and the key codes, and decode them (at once, or after 20 ms for the spurious-up quirk).
pub fn hidkbd_input(kbd: &Hidkbd, data: &mut [u8]) {
    let mut ud = kbd.sc_ndata.get();

    if let Some(munge) = kbd.sc_munge.get() {
        munge(kbd, data);
    }

    // extract variable keys
    for (i, var) in kbd.vars().iter().enumerate() {
        ud.var[i] = hid_get_data(data, &var.loc) as u8;
    }

    // extract keycodes
    let nkeycode = (kbd.sc_nkeycode.get() as usize).min(MAXKEYCODE);
    let pos = (kbd.sc_keycodeloc.get().pos / 8) as usize;
    match data.get(pos..pos + nkeycode) {
        Some(codes) => ud.keycode[..nkeycode].copy_from_slice(codes),
        None => ud.keycode[..nkeycode].fill(0),
    }
    kbd.sc_ndata.set(ud);

    if kbd.sc_debounce.get() != 0 && kbd.sc_polling.get() == 0 {
        // Some keyboards have a peculiar quirk. They sometimes generate a key up followed by
        // a key down for the same key after about 10 ms. We avoid this bug by holding off
        // decoding for 20 ms.
        kbd.sc_data.set(ud);
        timeout_add_msec(&kbd.sc_delay, 20);
    } else {
        hidkbd_decode(kbd, &ud);
    }
}

/// `hidkbd_delayed_decode`: the debounce timeout: decode the report held back.
fn hidkbd_delayed_decode(addr: *mut c_void) {
    // SAFETY: `hidkbd_attach` set the timeout's argument to this keyboard, which outlives it
    // (`hidkbd_detach` runs after the bus driver stops the timeout).
    let kbd = unsafe { &*addr.cast::<Hidkbd>() };

    let ud = kbd.sc_data.get();
    hidkbd_decode(kbd, &ud);
}

/// The `RELEASE` or `PRESS` of an event word as the wscons event type.
fn key_event(key: u16) -> u32 {
    if key & RELEASE != 0 {
        WSCONS_EVENT_KEY_UP
    } else {
        WSCONS_EVENT_KEY_DOWN
    }
}

/// `hidkbd_raw_translate`: the XT scancodes of the events `ibuf` (`WSDISPLAY_COMPAT_RAWKBD`),
/// into `cbuf` (at most two bytes per event); returns how many bytes were written. Keys
/// without a scancode (`NN`) are skipped, extended ones (>= 0x80 in `hidkbd_trtab`) are
/// prefixed with 0xe0, and a release sets the top bit.
pub fn hidkbd_raw_translate(ibuf: &[u16], cbuf: &mut [u8]) -> usize {
    let mut j = 0;
    for &key in ibuf {
        let c = HIDKBD_TRTAB[usize::from(key & CODEMASK)];
        if c == NN {
            continue;
        }
        if c & 0x80 != 0 {
            cbuf[j] = 0xe0;
            j += 1;
        }
        cbuf[j] = c & 0x7f;
        if key & RELEASE != 0 {
            cbuf[j] |= 0x80;
        }
        j += 1;
    }
    j
}

/// `hidkbd_decode`: compare the report `ud` with the previous one and turn the differences
/// into key events: for `wskbd` (`wskbd_input`), in raw mode as XT scancodes
/// (`wskbd_rawinput`), or, while polling, into `sc_pollchars`.
pub fn hidkbd_decode(kbd: &Hidkbd, ud: &HidkbdData) {
    let mut ibuf = [0u16; MAXKEYS]; // chars events
    let mut nkeys = 0;
    let mut addkey = |c: u16| {
        ibuf[nkeys] = c;
        nkeys += 1;
    };

    if ud.keycode[0] == KEY_ERROR {
        return; // ignore
    }

    let odata = kbd.sc_odata.get();
    for (i, var) in kbd.vars().iter().enumerate() {
        if (odata.var[i] & var.mask) != (ud.var[i] & var.mask) {
            addkey(
                u16::from(var.key)
                    | if ud.var[i] & var.mask != 0 {
                        PRESS
                    } else {
                        RELEASE
                    },
            );
        }
    }

    let nkeycode = (kbd.sc_nkeycode.get() as usize).min(MAXKEYCODE);
    let (ocodes, ncodes) = (&odata.keycode[..nkeycode], &ud.keycode[..nkeycode]);
    if ncodes != ocodes {
        // Check for released keys.
        for &key in ocodes {
            if key == 0 || ncodes.contains(&key) {
                continue;
            }
            addkey(u16::from(key) | RELEASE);
        }

        // Check for pressed keys.
        for &key in ncodes {
            if key == 0 || ocodes.contains(&key) {
                continue;
            }
            addkey(u16::from(key) | PRESS);
        }
    }
    kbd.sc_odata.set(*ud);

    if nkeys == 0 {
        return;
    }

    if kbd.sc_polling.get() != 0 {
        let mut pollchars = kbd.sc_pollchars.get();
        pollchars[..nkeys].copy_from_slice(&ibuf[..nkeys]);
        kbd.sc_pollchars.set(pollchars);
        kbd.sc_npollchar.set(nkeys as i32);
        return;
    }

    let Some(wskbddev) = kbd.wskbddev() else {
        return;
    };

    if kbd.sc_rawkbd.get() != 0 {
        let mut cbuf = [0u8; MAXKEYS * 2];
        let j = hidkbd_raw_translate(&ibuf[..nkeys], &mut cbuf);
        let s = spltty();
        wskbd_rawinput(wskbddev, &cbuf[..j]);

        // Pass audio, brightness and sleep keys to wskbd_input anyway.
        for &key in &ibuf[..nkeys] {
            match key & CODEMASK {
                102 | 127 | 128 | 129 | 232 | 233 | 234 | 235 | 236 => {
                    wskbd_input(wskbddev, key_event(key), i32::from(key & CODEMASK));
                }
                _ => {}
            }
        }
        splx(s);

        return;
    }

    let s = spltty();
    for &key in &ibuf[..nkeys] {
        wskbd_input(wskbddev, key_event(key), i32::from(key & CODEMASK));
    }
    splx(s);
}

/// `hidkbd_enable`: switch the keyboard on or off; `EBUSY` if it is in that state already.
pub fn hidkbd_enable(kbd: &Hidkbd, on: i32) -> Result<(), Errno> {
    if i32::from(kbd.sc_enabled.get()) == on {
        return Err(Errno::EBUSY);
    }

    kbd.sc_enabled.set(on as i8);
    Ok(())
}

/// `hidkbd_set_leds`: record the LEDs `leds` and build the output report that lights them
/// into `report`. `false` where the C returns 0 (nothing changed, no report to send), `true`
/// where it returns 1.
pub fn hidkbd_set_leds(kbd: &Hidkbd, leds: i32, report: &mut u8) -> bool {
    if kbd.sc_leds.get() == leds {
        return false;
    }

    kbd.sc_leds.set(leds);

    // This is not totally correct, since we did not check the report size from the
    // descriptor but for keyboards it should just be a single byte with the relevant bits
    // set.
    *report = 0;
    let (scro, num, caps, compose) = (
        kbd.sc_scroloc.get(),
        kbd.sc_numloc.get(),
        kbd.sc_capsloc.get(),
        kbd.sc_compose.get(),
    );
    if leds & WSKBD_LED_SCROLL != 0 && scro.size == 1 {
        *report |= 1u8.wrapping_shl(scro.pos);
    }
    if leds & WSKBD_LED_NUM != 0 && num.size == 1 {
        *report |= 1u8.wrapping_shl(num.pos);
    }
    if leds & WSKBD_LED_CAPS != 0 && caps.size == 1 {
        *report |= 1u8.wrapping_shl(caps.pos);
    }
    if leds & WSKBD_LED_COMPOSE != 0 && compose.size == 1 {
        *report |= 1u8.wrapping_shl(compose.pos);
    }

    true
}

/// `hidkbd_ioctl`: the ioctls `hidkbd` answers itself: `WSKBDIO_GETLEDS`,
/// `WSKBDIO_COMPLEXBELL` and `WSKBDIO_SETMODE`. `Ok(false)` where the C returns `-1`: not
/// one of them.
pub fn hidkbd_ioctl(
    kbd: &Hidkbd,
    cmd: u64,
    data: &mut [u8],
    flag: i32,
    p: Option<&Proc>,
) -> Result<bool, Errno> {
    let _ = (flag, p);
    match cmd {
        WSKBDIO_GETLEDS => {
            ioctl_ret(data, &kbd.sc_leds.get());
            Ok(true)
        }
        WSKBDIO_COMPLEXBELL => {
            let d: WskbdBellData = ioctl_arg(data);
            hidkbd_bell(d.pitch, d.period, d.volume, 0);
            Ok(true)
        }
        WSKBDIO_SETMODE => {
            kbd.sc_rawkbd
                .set(i32::from(ioctl_arg::<i32>(data) == WSKBD_RAW));
            Ok(true)
        }
        _ => Ok(false),
    }
}

/// `hidkbd_cngetc`: the next event decoded while polling: its type (`WSCONS_EVENT_KEY_UP` or
/// `_DOWN`) and key code. Only valid when `sc_npollchar > 0`, as in C.
pub fn hidkbd_cngetc(kbd: &Hidkbd, type_: &mut u32, data: &mut i32) {
    let mut pollchars = kbd.sc_pollchars.get();
    let c = pollchars[0];
    let n = (kbd.sc_npollchar.get() - 1).max(0);
    kbd.sc_npollchar.set(n);
    pollchars.copy_within(1..=(n as usize).min(MAXKEYS - 1), 0);
    kbd.sc_pollchars.set(pollchars);
    *type_ = key_event(c);
    *data = i32::from(c & CODEMASK);
}

/// `hidkbd_bell`: ring the bell through the hook a sound driver registered, if any.
pub fn hidkbd_bell(pitch: u32, period: u32, volume: u32, poll: i32) {
    // SAFETY: the hook is written once, by `hidkbd_hookup_bell` during autoconfiguration under
    // the kernel lock, which every caller of the bell also holds.
    if let Some(hook) = unsafe { HIDKBD_BELL.read() } {
        (hook.func)(hook.arg as *mut c_void, pitch, period, volume, poll);
    }
}

/// `hidkbd_hookup_bell`: register the bell (the first driver to do so keeps it).
pub fn hidkbd_hookup_bell(
    func: fn(arg: *mut c_void, pitch: u32, period: u32, volume: u32, poll: i32),
    arg: *mut c_void,
) {
    // SAFETY: see `hidkbd_bell`: autoconfiguration under the kernel lock, no reader running.
    let hook = unsafe { HIDKBD_BELL.get_mut() };
    if hook.is_none() {
        *hook = Some(BellHook {
            func,
            arg: arg as usize,
        });
    }
}

/// `hidkbd_parse_desc`: find the keys of report `id` in the descriptor `desc`: the modifier
/// variables, the key code array and the LEDs. `Err` has the reason there is no keyboard.
pub fn hidkbd_parse_desc(kbd: &Hidkbd, id: i32, desc: &[u8]) -> Result<(), &'static str> {
    let mut h = HidItem::default();
    let mut ivar: usize = 0;

    kbd.sc_nkeycode.set(0);

    let mut d = hid_start_parse(desc, hid_input);
    while hid_get_item(&mut d, &mut h) {
        if h.kind != hid_input
            || h.flags & HIO_CONST != 0
            || hid_get_usage_page(h.usage) != HUP_KEYBOARD
            || h.report_ID != id as u32
        {
            continue;
        }
        if h.flags & HIO_VARIABLE != 0 {
            ivar += 1;
        }
    }
    hid_end_parse(d);

    if ivar > MAXVARS {
        // too many variable keys
        ivar = MAXVARS;
    }

    kbd.sc_nvar.set(ivar as u32);
    let var = mallocarray(
        ivar,
        size_of::<HidkbdVariable>(),
        M_DEVBUF,
        M_NOWAIT | M_ZERO,
    );
    kbd.sc_var
        .set(var.map_or(ptr::null_mut(), |p| p.as_ptr().cast::<HidkbdVariable>()));

    if var.is_none() {
        return Ok(());
    }
    let vars = kbd.sc_var.get();

    let mut i: usize = 0;

    let mut d = hid_start_parse(desc, hid_input);
    while hid_get_item(&mut d, &mut h) {
        if h.kind != hid_input
            || h.flags & HIO_CONST != 0
            || hid_get_usage_page(h.usage) != HUP_KEYBOARD
            || h.report_ID != id as u32
        {
            continue;
        }

        if h.flags & HIO_VARIABLE != 0 {
            // variable reports should be one bit each
            if h.loc.size != 1 {
                // bad variable size
                continue;
            }

            // variable report
            if i < MAXVARS && i < ivar {
                // SAFETY: `vars` holds `ivar` zeroed entries and `i < ivar`; nothing else
                // reads the array while the descriptor is being parsed.
                unsafe {
                    vars.add(i).write(HidkbdVariable {
                        loc: h.loc,
                        mask: 1 << (i % 8),
                        key: hid_get_usage(h.usage) as u8,
                    });
                }
                i += 1;
            }
        } else {
            // keys array should be in bytes, on a byte boundary
            if h.loc.size != 8 {
                // key code size != 8
                continue;
            }
            if h.loc.pos % 8 != 0 {
                // array not on byte boundary
                continue;
            }
            if kbd.sc_nkeycode.get() != 0 {
                // ignoring multiple arrays
                continue;
            }
            kbd.sc_keycodeloc.set(h.loc);
            if h.loc.count as usize > MAXKEYCODE {
                // ignoring extra key codes
                kbd.sc_nkeycode.set(MAXKEYCODE as u32);
            } else {
                kbd.sc_nkeycode.set(h.loc.count);
            }
        }
    }
    hid_end_parse(d);

    // don't attach if no keys...
    if kbd.sc_nkeycode.get() == 0 && ivar == 0 {
        return Err("no usable key codes array");
    }

    // each LED output is its own member of the keyboard: a miss only zeroes its `size`
    let desc_id = id as u8;
    let locate_led = |led: u32, cell: &Cell<HidLocation>| {
        let mut loc = cell.get();
        let _ = hid_locate(
            desc,
            hid_usage2(HUP_LED, led),
            desc_id,
            hid_output,
            Some(&mut loc),
            None,
        );
        cell.set(loc);
    };
    locate_led(HUL_NUM_LOCK, &kbd.sc_numloc);
    locate_led(HUL_CAPS_LOCK, &kbd.sc_capsloc);
    locate_led(HUL_SCROLL_LOCK, &kbd.sc_scroloc);
    locate_led(HUL_COMPOSE, &kbd.sc_compose);

    Ok(())
}
/* </CODE> */

/* <TESTS> */
#[cfg(test)]
mod tests {
    // Host tests for `hidkbd`: descriptor parsing over QEMU's `usb-kbd`, the decoding of reports
    // into key events (read back through the polling path, which is what the console keyboard
    // uses), the LED report, the Apple key code rewriting, the raw scancodes, the ioctls and the
    // debounce quirk.

    use std::boxed::Box;
    use std::sync::atomic::AtomicU32;
    use std::vec::Vec;
    use std::{assert, assert_eq};

    use super::*;
    use crate::kern::subr_pool::tests::setup_real_memory;
    use crate::sys::ioccom::_io;

    /// QEMU's `usb-kbd` report descriptor (`hw/usb/dev-hid.c`): 8 modifier bits, a constant byte,
    /// five LEDs plus padding out, and an array of 6 key codes.
    const QEMU_KBD: &[u8] = &[
        0x05, 0x01, 0x09, 0x06, 0xa1, 0x01, 0x75, 0x01, 0x95, 0x08, 0x05, 0x07, 0x19, 0xe0, 0x29,
        0xe7, 0x15, 0x00, 0x25, 0x01, 0x81, 0x02, 0x95, 0x01, 0x75, 0x08, 0x81, 0x01, 0x95, 0x05,
        0x75, 0x01, 0x05, 0x08, 0x19, 0x01, 0x29, 0x05, 0x91, 0x02, 0x95, 0x01, 0x75, 0x03, 0x91,
        0x01, 0x95, 0x06, 0x75, 0x08, 0x15, 0x00, 0x25, 0xff, 0x05, 0x07, 0x19, 0x00, 0x29, 0xff,
        0x81, 0x00, 0xc0,
    ];

    /// A boot protocol mouse: no keys.
    const BOOT_MOUSE: &[u8] = &[
        0x05, 0x01, 0x09, 0x02, 0xa1, 0x01, 0x09, 0x01, 0xa1, 0x00, 0x05, 0x09, 0x19, 0x01, 0x29,
        0x03, 0x15, 0x00, 0x25, 0x01, 0x95, 0x03, 0x75, 0x01, 0x81, 0x02, 0x95, 0x01, 0x75, 0x05,
        0x81, 0x01, 0x05, 0x01, 0x09, 0x30, 0x09, 0x31, 0x15, 0x81, 0x25, 0x7f, 0x75, 0x08, 0x95,
        0x02, 0x81, 0x06, 0xc0, 0xc0,
    ];

    fn kbd_for(desc: &[u8]) -> Box<Hidkbd> {
        let kbd = Box::new(Hidkbd::new());
        assert_eq!(hidkbd_parse_desc(&kbd, 0, desc), Ok(()));
        kbd
    }

    /// Decodes `report` while polling and returns the events as (type, key code).
    fn poll(kbd: &Hidkbd, report: &[u8]) -> Vec<(u32, i32)> {
        kbd.sc_polling.set(1);
        kbd.sc_npollchar.set(0);
        hidkbd_input(kbd, &mut report.to_vec());
        let mut v = Vec::new();
        while kbd.sc_npollchar.get() > 0 {
            let (mut t, mut d) = (0, 0);
            hidkbd_cngetc(kbd, &mut t, &mut d);
            v.push((t, d));
        }
        v
    }

    const UP: u32 = WSCONS_EVENT_KEY_UP;
    const DOWN: u32 = WSCONS_EVENT_KEY_DOWN;

    fn fake_device() -> &'static Device {
        // SAFETY: a `Device` is all-zero valid (`config_make_softc` allocates it zeroed); the box
        // is leaked, so the reference lives forever.
        Box::leak(Box::new(unsafe { core::mem::zeroed::<Device>() }))
    }

    #[test]
    fn parse_qemu_usb_kbd() {
        let _g = setup_real_memory();
        let kbd = kbd_for(QEMU_KBD);
        assert_eq!(kbd.sc_nvar.get(), 8);
        assert_eq!(kbd.sc_nkeycode.get(), 6);
        assert_eq!(
            kbd.sc_keycodeloc.get(),
            HidLocation {
                size: 8,
                count: 6,
                pos: 16
            }
        );
        for (i, v) in kbd.vars().iter().enumerate() {
            assert_eq!(v.key, 0xe0 + i as u8, "the modifiers are usages 0xe0..0xe7");
            assert_eq!(v.mask, 1 << i);
            assert_eq!(
                v.loc,
                HidLocation {
                    size: 1,
                    count: 1,
                    pos: i as u32
                }
            );
        }
        // the LEDs are the bits of the output report, in the order of the usages 1 to 5
        assert_eq!(
            kbd.sc_numloc.get(),
            HidLocation {
                size: 1,
                count: 1,
                pos: 0
            }
        );
        assert_eq!(
            kbd.sc_capsloc.get(),
            HidLocation {
                size: 1,
                count: 1,
                pos: 1
            }
        );
        assert_eq!(
            kbd.sc_scroloc.get(),
            HidLocation {
                size: 1,
                count: 1,
                pos: 2
            }
        );
        assert_eq!(
            kbd.sc_compose.get(),
            HidLocation {
                size: 1,
                count: 1,
                pos: 3
            }
        );
        assert_eq!(hidkbd_detach(&kbd, 0), Ok(()));
        assert!(kbd.sc_var.get().is_null());
    }

    #[test]
    fn a_mouse_is_not_a_keyboard() {
        let _g = setup_real_memory();
        let kbd = Hidkbd::new();
        assert_eq!(
            hidkbd_parse_desc(&kbd, 0, BOOT_MOUSE),
            Err("no usable key codes array")
        );
        assert_eq!(kbd.sc_nkeycode.get(), 0);
        // another report ID than the keyboard's finds nothing either
        assert_eq!(
            hidkbd_parse_desc(&kbd, 1, QEMU_KBD),
            Err("no usable key codes array")
        );
    }

    #[test]
    fn attach_failure_is_enxio_and_success_records_the_device() {
        let _g = setup_real_memory();
        let dev = fake_device();
        let kbd = Hidkbd::new();
        assert_eq!(
            hidkbd_attach(dev, &kbd, 0, 0, 0, BOOT_MOUSE),
            Err(Errno::ENXIO)
        );
        assert_eq!(hidkbd_attach(dev, &kbd, 0, 0, 0, QEMU_KBD), Ok(()));
        assert_eq!(kbd.sc_device.get(), Some(NonNull::from(dev)));
        assert_eq!(kbd.sc_debounce.get(), 0);
        assert_eq!(kbd.sc_console_keyboard.get(), 0);
        assert_eq!(hidkbd_detach(&kbd, 0), Ok(()));
    }

    #[test]
    fn the_first_console_keyboard_takes_the_console() {
        let _g = setup_real_memory();
        let dev = fake_device();
        hidkbd_is_console.store(1, Ordering::Relaxed);
        let first = Hidkbd::new();
        let second = Hidkbd::new();
        assert_eq!(hidkbd_attach(dev, &first, 1, 0, 0, QEMU_KBD), Ok(()));
        assert_eq!(hidkbd_attach(dev, &second, 1, 0, 0, QEMU_KBD), Ok(()));
        assert_eq!(first.sc_console_keyboard.get(), 1);
        assert_eq!(second.sc_console_keyboard.get(), 0);
        assert_eq!(hidkbd_is_console.load(Ordering::Relaxed), 0);
        // detaching the console keyboard hands the console back
        assert_eq!(hidkbd_detach(&first, 0), Ok(()));
        assert_eq!(hidkbd_is_console.load(Ordering::Relaxed), 1);
        assert_eq!(hidkbd_detach(&second, 0), Ok(()));
        hidkbd_is_console.store(0, Ordering::Relaxed);
    }

    #[test]
    fn reports_decode_to_presses_and_releases() {
        let _g = setup_real_memory();
        let kbd = kbd_for(QEMU_KBD);
        // 'a' goes down
        assert_eq!(poll(&kbd, &[0, 0, 4, 0, 0, 0, 0, 0]), [(DOWN, 4)]);
        // the same report again: nothing
        assert_eq!(poll(&kbd, &[0, 0, 4, 0, 0, 0, 0, 0]), []);
        // left shift (modifier bit 1, key 0xe1) and 'b' go down: the modifiers come first
        assert_eq!(
            poll(&kbd, &[0x02, 0, 4, 5, 0, 0, 0, 0]),
            [(DOWN, 0xe1), (DOWN, 5)]
        );
        // 'a' goes up while 'b' stays, even though the array shifted
        assert_eq!(poll(&kbd, &[0x02, 0, 5, 0, 0, 0, 0, 0]), [(UP, 4)]);
        // two modifiers at once, then everything up: releases, modifiers first
        assert_eq!(poll(&kbd, &[0x03, 0, 5, 0, 0, 0, 0, 0]), [(DOWN, 0xe0)]);
        assert_eq!(
            poll(&kbd, &[0, 0, 0, 0, 0, 0, 0, 0]),
            [(UP, 0xe0), (UP, 0xe1), (UP, 5)]
        );
        assert_eq!(hidkbd_detach(&kbd, 0), Ok(()));
    }

    #[test]
    fn the_error_report_is_ignored_and_does_not_replace_the_state() {
        let _g = setup_real_memory();
        let kbd = kbd_for(QEMU_KBD);
        assert_eq!(poll(&kbd, &[0, 0, 4, 0, 0, 0, 0, 0]), [(DOWN, 4)]);
        // rollover error: all six slots 0x01
        assert_eq!(poll(&kbd, &[0, 0, 1, 1, 1, 1, 1, 1]), []);
        // so the next real report compares against 'a' still down
        assert_eq!(poll(&kbd, &[0, 0, 0, 0, 0, 0, 0, 0]), [(UP, 4)]);
        assert_eq!(hidkbd_detach(&kbd, 0), Ok(()));
    }

    #[test]
    fn a_short_report_reads_as_no_keys() {
        let _g = setup_real_memory();
        let kbd = kbd_for(QEMU_KBD);
        assert_eq!(poll(&kbd, &[0, 0, 4, 0, 0, 0, 0, 0]), [(DOWN, 4)]);
        // 5 bytes cannot hold the array: its codes read as zero, the key is released
        assert_eq!(poll(&kbd, &[0, 0, 4, 0, 0]), [(UP, 4)]);
        assert_eq!(hidkbd_detach(&kbd, 0), Ok(()));
    }

    #[test]
    fn polling_events_are_consumed_in_order() {
        let _g = setup_real_memory();
        let kbd = kbd_for(QEMU_KBD);
        kbd.sc_polling.set(1);
        hidkbd_input(&kbd, &mut [0x02, 0, 4, 5, 0, 0, 0, 0]);
        assert_eq!(kbd.sc_npollchar.get(), 3);
        let (mut t, mut d) = (0, 0);
        hidkbd_cngetc(&kbd, &mut t, &mut d);
        assert_eq!((t, d), (DOWN, 0xe1));
        assert_eq!(kbd.sc_npollchar.get(), 2);
        hidkbd_cngetc(&kbd, &mut t, &mut d);
        assert_eq!((t, d), (DOWN, 4));
        hidkbd_cngetc(&kbd, &mut t, &mut d);
        assert_eq!((t, d), (DOWN, 5));
        assert_eq!(kbd.sc_npollchar.get(), 0);
        assert_eq!(hidkbd_detach(&kbd, 0), Ok(()));
    }

    #[test]
    fn events_reach_wskbd_when_a_child_is_attached() {
        let _g = setup_real_memory();
        let kbd = kbd_for(QEMU_KBD);
        // no child: decoded, then dropped (the C's `sc_wskbddev == NULL`)
        hidkbd_input(&kbd, &mut [0, 0, 4, 0, 0, 0, 0, 0]);
        assert_eq!(kbd.sc_odata.get().keycode[0], 4);
        // with a child (a keyboard open in event mode) the events reach its queue: 4 up, 5 down
        let wsk = crate::dev::wscons::wskbd::tests::keyboard(crate::dev::wscons::wsksymdef::KB_US);
        let evar = &wsk.sc_base.me_evar;
        crate::dev::wscons::wsevent::wsevent_init(evar).unwrap();
        crate::dev::wscons::wskbd::wskbd_do_open(wsk, evar).unwrap();
        kbd.sc_wskbddev.set(Some(NonNull::from(&wsk.sc_base.me_dv)));
        hidkbd_input(&kbd, &mut [0, 0, 5, 0, 0, 0, 0, 0]);
        assert_eq!(kbd.sc_odata.get().keycode[0], 5);
        assert_eq!(evar.ws_put.get(), 2);
        kbd.sc_rawkbd.set(1);
        hidkbd_input(&kbd, &mut [0, 0, 0, 0, 0, 0, 0, 0]);
        assert_eq!(kbd.sc_odata.get().keycode[0], 0);
        kbd.sc_wskbddev.set(None);
        assert_eq!(hidkbd_detach(&kbd, 0), Ok(()));
    }

    #[test]
    fn raw_scancodes_are_xt() {
        let mut cbuf = [0u8; 8];
        // a (usb 0x04) is XT 0x1e; released, with the top bit
        let n = hidkbd_raw_translate(&[0x04 | PRESS, 0x04 | RELEASE], &mut cbuf);
        assert_eq!(&cbuf[..n], [0x1e, 0x9e]);
        // insert (usb 0x49) is the extended XT e0 52
        let n = hidkbd_raw_translate(&[0x49 | PRESS, 0x49 | RELEASE], &mut cbuf);
        assert_eq!(&cbuf[..n], [0xe0, 0x52, 0xe0, 0xd2]);
        // keys without a scancode are skipped
        let n = hidkbd_raw_translate(&[0x00 | PRESS, 0xa0 | PRESS, 0x05 | PRESS], &mut cbuf);
        assert_eq!(&cbuf[..n], [0x30]);
        // the modifiers: left control, left shift, right alt (extended)
        let n = hidkbd_raw_translate(&[0xe0 | PRESS, 0xe1 | PRESS, 0xe6 | PRESS], &mut cbuf);
        assert_eq!(&cbuf[..n], [0x1d, 0x2a, 0xe0, 0x38]);
        assert_eq!(HIDKBD_TRTAB.len(), 256);
        assert_eq!(HIDKBD_TRTAB[0x29], 0x01, "escape");
        assert_eq!(HIDKBD_TRTAB[0x2c], 0x39, "space");
    }

    #[test]
    fn leds_build_the_output_report() {
        let _g = setup_real_memory();
        let kbd = kbd_for(QEMU_KBD);
        let mut report = 0xff;
        assert!(
            !hidkbd_set_leds(&kbd, 0, &mut report),
            "unchanged: nothing to send"
        );
        assert_eq!(report, 0xff);
        assert!(hidkbd_set_leds(
            &kbd,
            WSKBD_LED_CAPS | WSKBD_LED_SCROLL,
            &mut report
        ));
        assert_eq!(
            report, 0b0000_0110,
            "caps is the second LED, scroll the third"
        );
        assert_eq!(kbd.sc_leds.get(), WSKBD_LED_CAPS | WSKBD_LED_SCROLL);
        assert!(!hidkbd_set_leds(
            &kbd,
            WSKBD_LED_CAPS | WSKBD_LED_SCROLL,
            &mut report
        ));
        assert!(hidkbd_set_leds(
            &kbd,
            WSKBD_LED_NUM | WSKBD_LED_COMPOSE,
            &mut report
        ));
        assert_eq!(report, 0b0000_1001);
        assert!(hidkbd_set_leds(&kbd, 0, &mut report));
        assert_eq!(report, 0);
        // an LED the descriptor lacks stays dark
        kbd.sc_capsloc.set(HidLocation::default());
        assert!(hidkbd_set_leds(&kbd, WSKBD_LED_CAPS, &mut report));
        assert_eq!(report, 0);
        assert_eq!(hidkbd_detach(&kbd, 0), Ok(()));
    }

    #[test]
    fn enable_toggles_and_refuses_a_repeat() {
        let kbd = Hidkbd::new();
        assert_eq!(hidkbd_enable(&kbd, 1), Ok(()));
        assert_eq!(hidkbd_enable(&kbd, 1), Err(Errno::EBUSY));
        assert_eq!(hidkbd_enable(&kbd, 0), Ok(()));
        assert_eq!(hidkbd_enable(&kbd, 0), Err(Errno::EBUSY));
    }

    static BELLS: AtomicU32 = AtomicU32::new(0);

    fn test_bell(arg: *mut c_void, pitch: u32, period: u32, volume: u32, poll: i32) {
        assert_eq!(arg as usize, 0x1234);
        assert_eq!((pitch, period, volume, poll), (1500, 100, 50, 0));
        BELLS.fetch_add(1, Ordering::Relaxed);
    }

    fn other_bell(_: *mut c_void, _: u32, _: u32, _: u32, _: i32) {
        panic!("the first hook keeps the bell");
    }

    #[test]
    fn ioctls_leds_mode_and_bell() {
        let _g = setup_real_memory();
        let kbd = Hidkbd::new();
        let p = &crate::kern::init_main::PROC0;
        let mut data = [0u8; 16];

        kbd.sc_leds.set(WSKBD_LED_NUM);
        assert_eq!(
            hidkbd_ioctl(&kbd, WSKBDIO_GETLEDS, &mut data, 0, Some(p)),
            Ok(true)
        );
        assert_eq!(ioctl_arg::<i32>(&data), WSKBD_LED_NUM);

        ioctl_ret(&mut data, &WSKBD_RAW);
        assert_eq!(
            hidkbd_ioctl(&kbd, WSKBDIO_SETMODE, &mut data, 0, Some(p)),
            Ok(true)
        );
        assert_eq!(kbd.sc_rawkbd.get(), 1);
        ioctl_ret(&mut data, &0i32);
        assert_eq!(
            hidkbd_ioctl(&kbd, WSKBDIO_SETMODE, &mut data, 0, Some(p)),
            Ok(true)
        );
        assert_eq!(kbd.sc_rawkbd.get(), 0);

        // the bell: nothing hooked up yet is not an error
        ioctl_ret(
            &mut data,
            &WskbdBellData {
                which: 7,
                pitch: 1500,
                period: 100,
                volume: 50,
            },
        );
        assert_eq!(
            hidkbd_ioctl(&kbd, WSKBDIO_COMPLEXBELL, &mut data, 0, Some(p)),
            Ok(true)
        );
        let before = BELLS.load(Ordering::Relaxed);
        hidkbd_hookup_bell(test_bell, 0x1234 as *mut c_void);
        hidkbd_hookup_bell(other_bell, ptr::null_mut());
        assert_eq!(
            hidkbd_ioctl(&kbd, WSKBDIO_COMPLEXBELL, &mut data, 0, Some(p)),
            Ok(true)
        );
        assert_eq!(BELLS.load(Ordering::Relaxed), before + 1);

        // not ours: wskbd goes on
        assert_eq!(
            hidkbd_ioctl(&kbd, _io(b'W', 99), &mut data, 0, Some(p)),
            Ok(false)
        );
    }

    #[test]
    fn translation_tables() {
        assert_eq!(hidkbd_translate(&APPLE_FN_TRANS, 82), 75, "up -> page up");
        assert_eq!(hidkbd_translate(&APPLE_FN_TRANS, 4), 0, "not in the table");
        assert_eq!(hidkbd_translate(&APPLE_ISO_TRANS, 53), 100);
        assert_eq!(hidkbd_translate(&APPLE_ISO_TRANS, 100), 53);
        assert_eq!(hidkbd_translate(&[], 53), 0);
        assert_eq!(APPLE_FN_TRANS.len(), 12);
        assert_eq!(APPLE_TB_TRANS[0], tr(30, 58));
    }

    /// A keyboard that has the key code array at byte 2 of 8 and the Fn key as bit 1 of byte 1.
    fn apple_kbd() -> Hidkbd {
        let kbd = Hidkbd::new();
        kbd.sc_keycodeloc.set(HidLocation {
            size: 8,
            count: 6,
            pos: 16,
        });
        kbd.sc_nkeycode.set(6);
        kbd.sc_fn.set(HidLocation {
            size: 1,
            count: 1,
            pos: 9,
        });
        kbd
    }

    #[test]
    fn apple_munging_rewrites_the_key_codes_with_fn_down() {
        let kbd = apple_kbd();

        // Fn up: nothing changes
        let mut r = [0, 0x00, 82, 40, 4, 0, 0, 0];
        hidkbd_apple_munge(&kbd, &mut r);
        assert_eq!(r, [0, 0x00, 82, 40, 4, 0, 0, 0]);

        // Fn down: up -> page up, return -> insert, 'a' stays
        let mut r = [0, 0x02, 82, 40, 4, 0, 0, 0];
        hidkbd_apple_munge(&kbd, &mut r);
        assert_eq!(r, [0, 0x02, 75, 73, 4, 0, 0, 0]);

        // the number row is the function keys on a Tb keyboard, after the Fn layer
        let mut r = [0, 0x02, 30, 31, 58, 0, 0, 0];
        hidkbd_apple_tb_munge(&kbd, &mut r);
        assert_eq!(r, [0, 0x02, 58, 59, 233, 0, 0, 0]);
        let mut r = [0, 0x00, 30, 0, 0, 0, 0, 0];
        hidkbd_apple_tb_munge(&kbd, &mut r);
        assert_eq!(r[2], 30, "Fn up");

        // MacBook Air: F9 is mute with Fn
        let mut r = [0, 0x02, 66, 0, 0, 0, 0, 0];
        hidkbd_apple_mba_munge(&kbd, &mut r);
        assert_eq!(r[2], 127);

        // the ISO layout swaps less and grave whatever Fn says
        let mut r = [0, 0x00, 53, 100, 0, 0, 0, 0];
        hidkbd_apple_iso_munge(&kbd, &mut r);
        assert_eq!(r, [0, 0x00, 100, 53, 0, 0, 0, 0]);
        let mut r = [0, 0x02, 53, 82, 0, 0, 0, 0];
        hidkbd_apple_iso_mba_munge(&kbd, &mut r);
        assert_eq!(r, [0, 0x02, 100, 75, 0, 0, 0, 0]);
    }

    #[test]
    fn apple_munging_stays_inside_a_short_report() {
        let kbd = apple_kbd();
        let mut r = [0u8, 0x02, 82, 82];
        hidkbd_apple_munge(&kbd, &mut r);
        assert_eq!(r, [0, 0x02, 75, 75]);
        let mut r = [0u8, 0x02];
        hidkbd_apple_munge(&kbd, &mut r);
        assert_eq!(r, [0, 0x02]);
        hidkbd_apple_translate(&kbd, &mut [], &APPLE_FN_TRANS);
    }

    fn rewrite_up_to_down(kbd: &Hidkbd, r: &mut [u8]) {
        hidkbd_apple_translate(kbd, r, &[tr(82, 81)]);
    }

    #[test]
    fn munge_runs_before_decoding() {
        let _g = setup_real_memory();
        let kbd = kbd_for(QEMU_KBD);
        kbd.sc_munge.set(Some(rewrite_up_to_down));
        assert_eq!(poll(&kbd, &[0, 0, 82, 0, 0, 0, 0, 0]), [(DOWN, 81)]);
        assert_eq!(hidkbd_detach(&kbd, 0), Ok(()));
    }

    #[test]
    fn the_debounce_quirk_holds_decoding_back() {
        let _g = setup_real_memory();
        // the debounce arms a timeout: the wheel is global, under its test lock
        let _t = crate::kern::kern_timeout::tests::LOCK
            .lock()
            .unwrap_or_else(|e| e.into_inner());
        crate::kern::kern_timeout::timeout_startup();
        let dev = fake_device();
        let kbd = Hidkbd::new();
        assert_eq!(
            hidkbd_attach(dev, &kbd, 0, HIDKBD_SPUR_BUT_UP, 0, QEMU_KBD),
            Ok(())
        );
        assert_eq!(kbd.sc_debounce.get(), 1);
        // not polling: the report waits in sc_data, nothing is decoded yet
        hidkbd_input(&kbd, &mut [0, 0, 4, 0, 0, 0, 0, 0]);
        assert_eq!(kbd.sc_data.get().keycode[0], 4);
        assert_eq!(kbd.sc_odata.get().keycode[0], 0);
        // the timeout decodes it (run by hand, the wheel is not ticking: take it off the wheel)
        assert!(crate::sys::timeout::timeout_pending(&kbd.sc_delay));
        assert!(crate::kern::kern_timeout::timeout_del(&kbd.sc_delay));
        hidkbd_delayed_decode(ptr::from_ref(&kbd).cast_mut().cast());
        assert_eq!(kbd.sc_odata.get().keycode[0], 4);
        // while polling there is no holding back
        kbd.sc_polling.set(1);
        hidkbd_input(&kbd, &mut [0, 0, 5, 0, 0, 0, 0, 0]);
        assert_eq!(kbd.sc_npollchar.get(), 2);
        assert_eq!(hidkbd_detach(&kbd, 0), Ok(()));
    }

    #[test]
    fn odd_descriptors_are_parsed_like_the_c_does() {
        let _g = setup_real_memory();
        // 200 one-bit variable keys: capped at MAXVARS
        let d: &[u8] = &[
            0x05, 0x01, 0x09, 0x06, 0xa1, 0x01, 0x05, 0x07, 0x19, 0x00, 0x29, 0xc7, 0x75, 0x01,
            0x95, 0xc8, 0x81, 0x02, 0xc0,
        ];
        let kbd = Hidkbd::new();
        assert_eq!(hidkbd_parse_desc(&kbd, 0, d), Ok(()));
        assert_eq!(kbd.sc_nvar.get(), MAXVARS as u32);
        assert_eq!(kbd.sc_nkeycode.get(), 0);
        assert_eq!(kbd.vars()[127].mask, 1 << 7);
        assert_eq!(kbd.vars()[9].key, 9);
        assert_eq!(hidkbd_detach(&kbd, 0), Ok(()));

        // a variable key that is not one bit wide is skipped but still counted: its slot stays
        // zero (mask 0), which never produces an event
        let d: &[u8] = &[
            0x05, 0x01, 0x09, 0x06, 0xa1, 0x01, 0x05, 0x07, 0x19, 0xe0, 0x29, 0xe0, 0x75, 0x01,
            0x95, 0x01, 0x81, 0x02, 0x19, 0xe1, 0x29, 0xe1, 0x75, 0x02, 0x81, 0x02, 0xc0,
        ];
        let kbd = Hidkbd::new();
        assert_eq!(hidkbd_parse_desc(&kbd, 0, d), Ok(()));
        assert_eq!(kbd.sc_nvar.get(), 2);
        assert_eq!(kbd.vars()[0].key, 0xe0);
        assert_eq!(kbd.vars()[1].mask, 0);
        assert_eq!(hidkbd_detach(&kbd, 0), Ok(()));

        // an array that is not 8 bits wide, one off a byte boundary, a second array and more than
        // MAXKEYCODE codes: only the first byte-aligned 8-bit array counts, capped at 6
        let d: &[u8] = &[
            0x05, 0x01, 0x09, 0x06, 0xa1, 0x01, 0x05, 0x07, 0x19, 0x00, 0x29, 0xff, 0x75, 0x04,
            0x95, 0x02, 0x81, 0x00, // 4-bit array: skipped
            0x75, 0x01, 0x95, 0x01, 0x81,
            0x01, // constant bit: shifts the next array off the byte
            0x75, 0x08, 0x95, 0x02, 0x81, 0x00, // array at bit 9: skipped
            0x95, 0x08, 0x81, 0x00, // 8 key codes at bit 25: also off a byte
            0xc0,
        ];
        let kbd = Hidkbd::new();
        assert_eq!(
            hidkbd_parse_desc(&kbd, 0, d),
            Err("no usable key codes array")
        );
        let d: &[u8] = &[
            0x05, 0x01, 0x09, 0x06, 0xa1, 0x01, 0x05, 0x07, 0x19, 0x00, 0x29, 0xff, 0x75, 0x08,
            0x95, 0x08, 0x81, 0x00, 0x95, 0x02, 0x81, 0x00, 0xc0,
        ];
        let kbd = Hidkbd::new();
        assert_eq!(hidkbd_parse_desc(&kbd, 0, d), Ok(()));
        assert_eq!(kbd.sc_nkeycode.get(), 6);
        assert_eq!(
            kbd.sc_keycodeloc.get(),
            HidLocation {
                size: 8,
                count: 8,
                pos: 0
            }
        );
        assert_eq!(hidkbd_detach(&kbd, 0), Ok(()));
    }

    #[test]
    fn the_keymap_data_has_the_usb_layouts_and_takes_the_layout() {
        use crate::dev::wscons::wsksymdef::KB_US;
        assert_eq!(UKBD_KEYMAPDATA.keydesc.len(), UKBD_KEYDESCTAB.len());
        assert_eq!(UKBD_KEYMAPDATA.keydesc[0].name, KB_US);
        UKBD_KEYMAPDATA.set_layout(KB_US);
        assert_eq!(UKBD_KEYMAPDATA.layout(), KB_US);
    }
}
/* </TESTS> */
