/* $OpenBSD: wskbdvar.h,v 1.4 2022/02/16 06:23:42 anton Exp $ */
/* $NetBSD: wskbdvar.h,v 1.8 1999/12/01 23:22:59 augustss Exp $ */
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

//! `<dev/wscons/wskbdvar.h>`: the interface between keyboard drivers and `wskbd(4)`.
//!
//! Upstream: sys/dev/wscons/wskbdvar.h @ 3ce1f3f79392
//!
//! A keyboard driver (`ukbd(4)`, `pckbd(4)`, ...) attaches a `wskbd` child with a
//! [`WskbddevAttachArgs`]: its [`WskbdAccessops`] (enable, set the LEDs, driver-specific
//! ioctls), the cookie they are called with, the layouts it offers and whether it is the
//! console keyboard (whose [`WskbdConsops`] it hands to `wskbd_cnattach`). It feeds key
//! events to the child with `wskbd_input` and `wskbd_rawinput`. The functions are in
//! `wskbd.rs`.
//!
//! ## Deviations
//! - The operation tables are structures of `fn` pointers; the `void *` cookie stays an
//!   untyped pointer (`*mut c_void`), as in `sys/sys/device.rs`. `ioctl` takes the kernel
//!   copy of the argument as a byte slice, and returns `Ok(true)` where the C returns 0,
//!   `Ok(false)` where it returns -1 (not the driver's ioctl, `wskbd` goes on), or the
//!   errno (the convention of `ttioctl` in `kern/tty.rs`). `getc`'s two out-parameters are
//!   `&mut`s.
//! - `wskbd_consops`' `debugger` member is an `Option`, as a console keyboard may have none.
//! - The `wskbddevcf_console` and `wskbddevcf_mux` locator macros are functions of the
//!   `cfdata`; a missing locator reads as the default of `define wskbddev {[console = -1],
//!   [mux = 1]}` in `sys/conf/files`.
//! - The prototypes of `wskbd_cnattach`, `wskbd_cndetach`, `wskbddevprint`, `wskbd_input`,
//!   `wskbd_rawinput`, `wskbd_cngetc`, `wskbd_cnpollc` and `wskbd_cnbell` belong to
//!   `wskbd.c`; they are in `wskbd.rs`.

use core::ffi::c_void;

use crate::dev::wscons::wsksymvar::WskbdMapdata;
use crate::sys::device::Cfdata;
use crate::sys::errno::Errno;
use crate::sys::proc::Proc;

/// The type of `wskbd_accessops`' `ioctl`: `Ok(true)` handled, `Ok(false)` not the driver's.
pub type WskbdIoctlFn =
    fn(v: *mut c_void, cmd: u64, data: &mut [u8], flag: i32, p: &Proc) -> Result<bool, Errno>;

/// `struct wskbd_accessops`: keyboard access functions (must be provided by all keyboards).
///
/// There is a cookie provided by the keyboard driver associated with these functions, which
/// is passed to them when they are invoked.
#[derive(Clone, Copy)]
pub struct WskbdAccessops {
    /// `enable`: turn the keyboard on or off.
    pub enable: fn(v: *mut c_void, on: i32) -> i32,
    /// `set_leds`: set the keyboard's LEDs (`WSKBD_LED_*`).
    pub set_leds: fn(v: *mut c_void, leds: i32),
    /// `ioctl`: driver-specific ioctls.
    pub ioctl: WskbdIoctlFn,
}

/// `struct wskbd_consops`: keyboard console functions (must be provided by console input
/// keyboards).
///
/// There is a cookie provided by the keyboard driver associated with these functions, which
/// is passed to them when they are invoked.
#[derive(Clone, Copy)]
pub struct WskbdConsops {
    /// `getc`: the next key event (type and key code) when polling.
    pub getc: fn(v: *mut c_void, type_: &mut u32, data: &mut i32),
    /// `pollc`: switch polling on or off.
    pub pollc: fn(v: *mut c_void, on: i32),
    /// `bell`: ring the bell (pitch, period, volume).
    pub bell: fn(v: *mut c_void, pitch: u32, period: u32, volume: u32),
    /// `debugger`: enter the debugger.
    pub debugger: Option<fn(v: *mut c_void)>,
}

/// `struct wskbddev_attach_args`: attachment information provided by wskbddev devices when
/// attaching wskbd units.
pub struct WskbddevAttachArgs {
    /// `console`: is it console?
    pub console: i32,
    /// `keymap`: the layouts the keyboard offers.
    pub keymap: &'static WskbdMapdata,
    /// `accessops`: access ops.
    pub accessops: &'static WskbdAccessops,
    /// `accesscookie`: access cookie.
    pub accesscookie: *mut c_void,
    /// `audiocookie`.
    pub audiocookie: *mut c_void,
}

/// `WSKBDDEVCF_CONSOLE`: the index of the `console` locator.
pub const WSKBDDEVCF_CONSOLE: usize = 0;
/// `WSKBDDEVCF_CONSOLE_UNK`: the `console` locator left open.
pub const WSKBDDEVCF_CONSOLE_UNK: i64 = -1;
/// `WSKBDDEVCF_MUX`: the index of the `mux` locator.
pub const WSKBDDEVCF_MUX: usize = 1;

/// `wskbddevcf_console`: spec'd as console?
pub fn wskbddevcf_console(cf: &Cfdata) -> i64 {
    cf.cf_loc
        .get(WSKBDDEVCF_CONSOLE)
        .copied()
        .unwrap_or(WSKBDDEVCF_CONSOLE_UNK)
}

/// `wskbddevcf_mux`: the mux the keyboard attaches to (`mux = 1` by default).
pub fn wskbddevcf_mux(cf: &Cfdata) -> i64 {
    cf.cf_loc.get(WSKBDDEVCF_MUX).copied().unwrap_or(1)
}
