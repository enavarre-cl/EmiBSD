/* $OpenBSD: wskbd.c,v 1.124 2025/07/18 17:34:29 mvs Exp $ */
/* $NetBSD: wskbd.c,v 1.80 2005/05/04 01:52:16 augustss Exp $ */
/* <LICENSES> */
/*
 * Copyright (c) 1996, 1997 Christopher G. Demetriou.  All rights reserved.
 *
 * Keysym translator:
 * Contributed to The NetBSD Foundation by Juergen Hannken-Illjes.
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

/*
 * Copyright (c) 1992, 1993
 *	The Regents of the University of California.  All rights reserved.
 *
 * This software was developed by the Computer Systems Engineering group
 * at Lawrence Berkeley Laboratory under DARPA contract BG 91-66 and
 * contributed to Berkeley.
 *
 * All advertising materials mentioning features or use of this software
 * must display the following acknowledgement:
 *	This product includes software developed by the University of
 *	California, Lawrence Berkeley Laboratory.
 *
 * Redistribution and use in source and binary forms, with or without
 * modification, are permitted provided that the following conditions
 * are met:
 * 1. Redistributions of source code must retain the above copyright
 *    notice, this list of conditions and the following disclaimer.
 * 2. Redistributions in binary form must reproduce the above copyright
 *    notice, this list of conditions and the following disclaimer in the
 *    documentation and/or other materials provided with the distribution.
 * 3. Neither the name of the University nor the names of its contributors
 *    may be used to endorse or promote products derived from this software
 *    without specific prior written permission.
 *
 * THIS SOFTWARE IS PROVIDED BY THE REGENTS AND CONTRIBUTORS ``AS IS'' AND
 * ANY EXPRESS OR IMPLIED WARRANTIES, INCLUDING, BUT NOT LIMITED TO, THE
 * IMPLIED WARRANTIES OF MERCHANTABILITY AND FITNESS FOR A PARTICULAR PURPOSE
 * ARE DISCLAIMED.  IN NO EVENT SHALL THE REGENTS OR CONTRIBUTORS BE LIABLE
 * FOR ANY DIRECT, INDIRECT, INCIDENTAL, SPECIAL, EXEMPLARY, OR CONSEQUENTIAL
 * DAMAGES (INCLUDING, BUT NOT LIMITED TO, PROCUREMENT OF SUBSTITUTE GOODS
 * OR SERVICES; LOSS OF USE, DATA, OR PROFITS; OR BUSINESS INTERRUPTION)
 * HOWEVER CAUSED AND ON ANY THEORY OF LIABILITY, WHETHER IN CONTRACT, STRICT
 * LIABILITY, OR TORT (INCLUDING NEGLIGENCE OR OTHERWISE) ARISING IN ANY WAY
 * OUT OF THE USE OF THIS SOFTWARE, EVEN IF ADVISED OF THE POSSIBILITY OF
 * SUCH DAMAGE.
 *
 *	@(#)kbd.c	8.2 (Berkeley) 10/30/93
 */
/* </LICENSES> */

//! `wskbd(4)`: the wscons keyboard driver (`wskbd* at wskbddev?`), the half the keyboard
//! drivers call into: the print function of their `config_found`, and the callbacks that
//! deliver key events and attach the console keyboard.
//!
//! Upstream: sys/dev/wscons/wskbd.c @ 3ce1f3f79392
//!
//! Status `wip`, milestone M13 brings the driver. Today only `wskbddevprint` is ported (the
//! function a keyboard driver passes to `config_found` when it offers a `wskbd` child, which
//! prints `wskbd at <parent>` when nothing claims it). No `wskbd* at ukbd?` line exists in the
//! machines' `ioconf.rs` yet, so that `config_found` finds nothing and the keyboard drivers'
//! `sc_wskbddev` stays `None`, as on an OpenBSD kernel configured without `wskbd`: the
//! callbacks below are then never reached.
//!
//! ## Deviations
//! - Not ported (M13): the driver itself (`wskbd_match`, `wskbd_attach`, `wskbd_detach`,
//!   `wskbd_activate`, the `wskbdopen`/`wskbdread`/`wskbdioctl` device entry points, the
//!   keysym translation `wskbd_translate`, the key repeat, the mux and `wskbd_cngetc`,
//!   `wskbd_cnpollc` and `wskbd_cnbell`).
//! - Visible stubs, each printing `unported: <name>` once: [`wskbd_input`],
//!   [`wskbd_rawinput`], [`wskbd_cnattach`] and [`wskbd_cndetach`]. The keyboard events are
//!   dropped, no console keyboard is registered.

use core::ffi::c_void;

use crate::dev::wscons::wskbdvar::WskbdConsops;
use crate::dev::wscons::wsksymvar::WskbdMapdata;
use crate::kern::subr_prf::Str;
use crate::kprintf;
use crate::sys::device::{Device, UNCONF};
use crate::unported;

/// `wskbddevprint`: print function (for parent devices).
pub fn wskbddevprint(aux: *mut c_void, pnp: Option<&[u8]>) -> i32 {
    let _ = aux;
    if let Some(pnp) = pnp {
        kprintf!("wskbd at {}", Str(pnp));
    }
    UNCONF
}

/// `wskbd_cnattach`: attach the console keyboard with its console operations, cookie and
/// layouts. Not ported: M13.
pub fn wskbd_cnattach(
    consops: &'static WskbdConsops,
    cookie: *mut c_void,
    mapdata: &'static WskbdMapdata,
) {
    let _ = (consops, cookie, mapdata);
    let _ = unported!("wskbd_cnattach (wskbd.c, M13)");
}

/// `wskbd_cndetach`: detach the console keyboard. Not ported: M13.
pub fn wskbd_cndetach() {
    let _ = unported!("wskbd_cndetach (wskbd.c, M13)");
}

/// `wskbd_input`: callback from the keyboard driver to the wskbd interface driver: a key
/// event (`WSCONS_EVENT_KEY_UP` or `WSCONS_EVENT_KEY_DOWN`, and the key code). Not ported:
/// M13.
pub fn wskbd_input(kbddev: &Device, type_: u32, value: i32) {
    let _ = (kbddev, type_, value);
    let _ = unported!("wskbd_input (wskbd.c, M13)");
}

/// `wskbd_rawinput`: as [`wskbd_input`], for `WSDISPLAY_COMPAT_RAWKBD`: raw XT scancodes.
/// Not ported: M13.
pub fn wskbd_rawinput(kbddev: &Device, buf: &[u8]) {
    let _ = (kbddev, buf);
    let _ = unported!("wskbd_rawinput (wskbd.c, M13)");
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::sys::device::QUIET;

    #[test]
    fn devprint_reports_the_child_as_unconfigured() {
        assert_eq!(wskbddevprint(core::ptr::null_mut(), Some(b"ukbd0")), UNCONF);
        assert_eq!(wskbddevprint(core::ptr::null_mut(), None), UNCONF);
        assert_ne!(UNCONF, QUIET);
    }
}
