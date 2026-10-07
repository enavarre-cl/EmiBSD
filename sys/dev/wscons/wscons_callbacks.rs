/* $OpenBSD: wscons_callbacks.h,v 1.9 2013/10/18 22:06:40 miod Exp $ */
/* $NetBSD: wscons_callbacks.h,v 1.16 2001/11/10 17:14:51 augustss Exp $ */
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

//! `<dev/wscons/wscons_callbacks.h>`: the calls between the wscons glue, the display
//! interface (`wsdisplay.c`) and the keyboard interface (`wskbd.c`).
//!
//! Upstream: sys/dev/wscons/wscons_callbacks.h @ 3ce1f3f79392
//!
//! The header only declares functions; `wsdisplay.c` defines the `wsdisplay_*` ones and
//! `wskbd.c` the `wskbd_*` ones. Neither driver is ported yet, so this module is the seam:
//! each `wsdisplay_*` function here is a visible stub until the `wsdisplay.c` port defines
//! it in `wsdisplay.rs` and replaces the stub with a `pub use`; the `wskbd_*` functions are
//! re-exported from `wskbd.rs`, where `wskbd.c`'s other stubs are.
//!
//! ## Deviations
//! - `struct wsevsrc` (defined by `<dev/wscons/wsmuxvar.h>`, not ported) is the
//!   uninhabited [`Wsevsrc`], as `docs/C_TO_RUST.md` has it for a structure only declared
//!   so far; the pointers to it are `Option<&Wsevsrc>`, `None` until `wsmux.c` exists.
//! - Visible stubs, each printing `unported: <name>` once (wsdisplay.c, M13):
//!   [`wsdisplay_set_console_kbd`], [`wsdisplay_kbdinput`], [`wsdisplay_rawkbdinput`],
//!   [`wsdisplay_switch`] (`ENOSYS`), [`wsdisplay_reset`], [`wsdisplay_kbdholdscreen`],
//!   [`wsdisplay_set_cons_kbd`], [`wsdisplay_unset_cons_kbd`], [`wsdisplay_set_kbd`]
//!   (`ENOSYS`) and [`wsdisplay_param`] (`ENOSYS`).
//! - The `int` results that are 0 or an errno are `Result<(), Errno>`; `wskbd_pickfree`
//!   keeps its index or -1.

use crate::dev::wscons::wsconsio::WsdisplayParam;
use crate::dev::wscons::wsksymvar::{KbdT, KeysymT};
use crate::sys::device::Device;
use crate::sys::errno::Errno;
use crate::sys::types::Dev;
use crate::unported;

pub use crate::dev::wscons::wskbd::{wskbd_pickfree, wskbd_set_console_display, wskbd_set_display};

/// `struct wsevsrc`: an event source of the wscons mux (`<dev/wscons/wsmuxvar.h>`), only
/// declared here. Uninhabited until `wsmux.c` is ported: no value of it exists, so every
/// `Option<&Wsevsrc>` is `None`.
pub enum Wsevsrc {}

/// `enum wsdisplay_resetops`: what `wsdisplay_reset` resets.
#[repr(i32)]
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[allow(non_camel_case_types)] // OpenBSD names, verbatim, for grep-ability
pub enum WsdisplayResetops {
    /// `WSDISPLAY_RESETEMUL`: reset the terminal emulation.
    WSDISPLAY_RESETEMUL = 0,
    /// `WSDISPLAY_RESETCLOSE`: reset the screen as on last close.
    WSDISPLAY_RESETCLOSE = 1,
}

pub use WsdisplayResetops::*;

/// `wsdisplay_set_console_kbd`: calls to the display interface from the glue code: the
/// console keyboard's event source. Not ported: wsdisplay.c (M13).
pub fn wsdisplay_set_console_kbd(src: Option<&Wsevsrc>) {
    let _ = src;
    let _ = unported!("wsdisplay_set_console_kbd (wsdisplay.c, M13)");
}

/// `wsdisplay_kbdinput`: calls to the display interface from the keyboard interface: `ks`,
/// keysyms in layout `layout`, for the focused screen's tty. Not ported: wsdisplay.c (M13).
pub fn wsdisplay_kbdinput(v: &Device, layout: KbdT, ks: &[KeysymT]) {
    let _ = (v, layout, ks);
    let _ = unported!("wsdisplay_kbdinput (wsdisplay.c, M13)");
}

/// `wsdisplay_rawkbdinput`: raw scancodes for the focused screen's tty
/// (`WSDISPLAY_COMPAT_RAWKBD`). Not ported: wsdisplay.c (M13).
pub fn wsdisplay_rawkbdinput(v: &Device, buf: &[u8]) {
    let _ = (v, buf);
    let _ = unported!("wsdisplay_rawkbdinput (wsdisplay.c, M13)");
}

/// `wsdisplay_switch`: switch to screen `no`. Not ported: wsdisplay.c (M13).
pub fn wsdisplay_switch(dev: &Device, no: i32, waitok: i32) -> Result<(), Errno> {
    let _ = (dev, no, waitok);
    Err(unported!("wsdisplay_switch (wsdisplay.c, M13)"))
}

/// `wsdisplay_reset`. Not ported: wsdisplay.c (M13).
pub fn wsdisplay_reset(dev: &Device, op: WsdisplayResetops) {
    let _ = (dev, op);
    let _ = unported!("wsdisplay_reset (wsdisplay.c, M13)");
}

/// `wsdisplay_kbdholdscreen`: the keyboard's Hold Screen key. Not ported: wsdisplay.c (M13).
pub fn wsdisplay_kbdholdscreen(v: &Device, hold: i32) {
    let _ = (v, hold);
    let _ = unported!("wsdisplay_kbdholdscreen (wsdisplay.c, M13)");
}

/// `wsdisplay_set_cons_kbd`: the console keyboard's polled `getc`, `pollc` and `bell`.
/// Not ported: wsdisplay.c (M13).
pub fn wsdisplay_set_cons_kbd(
    get: fn(Dev) -> i32,
    poll: fn(Dev, i32),
    bell: Option<fn(Dev, u32, u32, u32)>,
) {
    let _ = (get, poll, bell);
    let _ = unported!("wsdisplay_set_cons_kbd (wsdisplay.c, M13)");
}

/// `wsdisplay_unset_cons_kbd`. Not ported: wsdisplay.c (M13).
pub fn wsdisplay_unset_cons_kbd() {
    let _ = unported!("wsdisplay_unset_cons_kbd (wsdisplay.c, M13)");
}

/// `wsdisplay_set_kbd`: attach the keyboard event source `src` to display `dev`. Not
/// ported: wsdisplay.c (M13).
pub fn wsdisplay_set_kbd(dev: &Device, src: Option<&Wsevsrc>) -> Result<(), Errno> {
    let _ = (dev, src);
    Err(unported!("wsdisplay_set_kbd (wsdisplay.c, M13)"))
}

/// `wsdisplay_param`: the `WSDISPLAYIO_GETPARAM`/`SETPARAM` of display `dev`, from the
/// keyboard's brightness keys. Not ported: wsdisplay.c (M13).
pub fn wsdisplay_param(dev: &Device, cmd: u64, dp: &mut WsdisplayParam) -> Result<(), Errno> {
    let _ = (dev, cmd, dp);
    Err(unported!("wsdisplay_param (wsdisplay.c, M13)"))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn resetops_values() {
        assert_eq!(WSDISPLAY_RESETEMUL as i32, 0);
        assert_eq!(WSDISPLAY_RESETCLOSE as i32, 1);
    }
}
