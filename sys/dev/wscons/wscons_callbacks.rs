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

/* <CODE> */
//! `<dev/wscons/wscons_callbacks.h>`: the calls between the wscons glue, the display
//! interface (`wsdisplay.c`) and the keyboard interface (`wskbd.c`).
//!
//! Upstream: sys/dev/wscons/wscons_callbacks.h @ 3ce1f3f79392
//!
//! The header only declares functions; `wsdisplay.c` defines the `wsdisplay_*` ones
//! (`wsdisplay.rs`) and `wskbd.c` the `wskbd_*` ones (`wskbd.rs`), all re-exported here.
//!
//! ## Deviations
//! - `struct wsevsrc`, which the header only declares, is `<dev/wscons/wsmuxvar.h>`'s
//!   [`Wsevsrc`] (`wsmuxvar.rs`), re-exported here; the pointers to it are
//!   `Option<&Wsevsrc>`.
//! - The `int` results that are 0 or an errno are `Result<(), Errno>`; `wskbd_pickfree`
//!   keeps its index or -1.

pub use crate::dev::wscons::wsdisplay::{
    wsdisplay_kbdholdscreen, wsdisplay_kbdinput, wsdisplay_param, wsdisplay_rawkbdinput,
    wsdisplay_reset, wsdisplay_set_cons_kbd, wsdisplay_set_console_kbd, wsdisplay_set_kbd,
    wsdisplay_switch, wsdisplay_unset_cons_kbd,
};
pub use crate::dev::wscons::wskbd::{wskbd_pickfree, wskbd_set_console_display, wskbd_set_display};
pub use crate::dev::wscons::wsmuxvar::Wsevsrc;

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
/* </CODE> */

/* <TESTS> */
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn resetops_values() {
        assert_eq!(WSDISPLAY_RESETEMUL as i32, 0);
        assert_eq!(WSDISPLAY_RESETCLOSE as i32, 1);
    }
}
/* </TESTS> */
