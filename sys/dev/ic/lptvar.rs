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

/*
 * Copyright (c) 1993, 1994 Charles Hannum.
 * Copyright (c) 1990 William F. Jolitz, TeleMuse
 * All rights reserved.
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
 *	This software is a component of "386BSD" developed by
 *	William F. Jolitz, TeleMuse.
 * 4. Neither the name of the developer nor the name "386BSD"
 *    may be used to endorse or promote products derived from this software
 *    without specific prior written permission.
 *
 * THIS SOFTWARE IS A COMPONENT OF 386BSD DEVELOPED BY WILLIAM F. JOLITZ
 * AND IS INTENDED FOR RESEARCH AND EDUCATIONAL PURPOSES ONLY. THIS
 * SOFTWARE SHOULD NOT BE CONSIDERED TO BE A COMMERCIAL PRODUCT.
 * THE DEVELOPER URGES THAT USERS WHO REQUIRE A COMMERCIAL PRODUCT
 * NOT MAKE USE OF THIS WORK.
 *
 * FOR USERS WHO WISH TO UNDERSTAND THE 386BSD SYSTEM DEVELOPED
 * BY WILLIAM F. JOLITZ, WE RECOMMEND THE USER STUDY WRITTEN
 * REFERENCES SUCH AS THE  "PORTING UNIX TO THE 386" SERIES
 * (BEGINNING JANUARY 1991 "DR. DOBBS JOURNAL", USA AND BEGINNING
 * JUNE 1991 "UNIX MAGAZIN", GERMANY) BY WILLIAM F. JOLITZ AND
 * LYNNE GREER JOLITZ, AS WELL AS OTHER BOOKS ON UNIX AND THE
 * ON-LINE 386BSD USER MANUAL BEFORE USE. A BOOK DISCUSSING THE INTERNALS
 * OF 386BSD ENTITLED "386BSD FROM THE INSIDE OUT" WILL BE AVAILABLE LATE 1992.
 *
 * THIS SOFTWARE IS PROVIDED BY THE DEVELOPER ``AS IS'' AND
 * ANY EXPRESS OR IMPLIED WARRANTIES, INCLUDING, BUT NOT LIMITED TO, THE
 * IMPLIED WARRANTIES OF MERCHANTABILITY AND FITNESS FOR A PARTICULAR PURPOSE
 * ARE DISCLAIMED.  IN NO EVENT SHALL THE DEVELOPER BE LIABLE
 * FOR ANY DIRECT, INDIRECT, INCIDENTAL, SPECIAL, EXEMPLARY, OR CONSEQUENTIAL
 * DAMAGES (INCLUDING, BUT NOT LIMITED TO, PROCUREMENT OF SUBSTITUTE GOODS
 * OR SERVICES; LOSS OF USE, DATA, OR PROFITS; OR BUSINESS INTERRUPTION)
 * HOWEVER CAUSED AND ON ANY THEORY OF LIABILITY, WHETHER IN CONTRACT, STRICT
 * LIABILITY, OR TORT (INCLUDING NEGLIGENCE OR OTHERWISE) ARISING IN ANY WAY
 * OUT OF THE USE OF THIS SOFTWARE, EVEN IF ADVISED OF THE POSSIBILITY OF
 * SUCH DAMAGE.
 */
/* </LICENSES> */

/* <CODE> */
//! `<dev/ic/lptvar.h>`: lpt(4)'s softc, shared by its bus front ends (`lpt_isa.c`).
//!
//! Upstream: sys/dev/ic/lptvar.h @ 3ce1f3f79392
//!
//! The functions this header declares (`lptintr`, `lpt_port_test`, `lpt_attach_common`,
//! `lpt_activate`) live in `dev/ic/lpt.rs`, where the C defines them.
//!
//! ## Deviations
//! - The members the C changes through a shared softc are `Cell`s (the softc is reached
//!   from the interrupt, the timeout and the system calls under `spltty` and the kernel
//!   lock, as in the C); the bus tag and handle are `Option`s, set by the front end's
//!   attach.

use core::cell::Cell;
use core::ffi::c_void;

use crate::machine::bus::{BusSpaceHandle, BusSpaceTag};
use crate::sys::device::{Device, Softc};
use crate::sys::timeout::Timeout;

/// `LPT_OPEN`: device is open.
pub const LPT_OPEN: u8 = 0x01;
/// `LPT_OBUSY`: printer is busy doing output.
pub const LPT_OBUSY: u8 = 0x02;
/// `LPT_INIT`: waiting to initialize for open.
pub const LPT_INIT: u8 = 0x04;

/// `LPT_POLLED`: configured for polling only.
pub const LPT_POLLED: u8 = 0x10;
/// `LPT_AUTOLF`: automatic LF on CR.
pub const LPT_AUTOLF: u8 = 0x20;
/// `LPT_NOPRIME`: don't prime on open.
pub const LPT_NOPRIME: u8 = 0x40;
/// `LPT_NOINTR`: do not use interrupt.
pub const LPT_NOINTR: u8 = 0x80;

/// `struct lpt_softc`. Allocated zeroed by autoconf, so every member is valid as zero.
#[repr(C)]
pub struct LptSoftc {
    /// `sc_dev`.
    pub sc_dev: Device,
    /// `sc_ih`.
    pub sc_ih: Cell<*mut c_void>,
    /// `sc_wakeup_tmo`.
    pub sc_wakeup_tmo: Timeout,

    /// `sc_count`.
    pub sc_count: Cell<usize>,
    /// `sc_inbuf`: the `LPT_BSIZE` bytes `lptopen` allocates.
    pub sc_inbuf: Cell<*mut u8>,
    /// `sc_cp`: the next byte of `sc_inbuf` to send.
    pub sc_cp: Cell<*mut u8>,
    /// `sc_spinmax`.
    pub sc_spinmax: Cell<i32>,
    /// `sc_iot`: bus tag.
    pub sc_iot: Cell<Option<BusSpaceTag>>,
    /// `sc_ioh`: handle to the registers.
    pub sc_ioh: Cell<Option<BusSpaceHandle>>,
    /// `sc_state`: `LPT_OPEN`, `LPT_OBUSY`, `LPT_INIT`.
    pub sc_state: Cell<u8>,
    /// `sc_flags`: `LPT_POLLED`, `LPT_AUTOLF`, `LPT_NOPRIME`, `LPT_NOINTR`.
    pub sc_flags: Cell<u8>,
    /// `sc_control`.
    pub sc_control: Cell<u8>,
    /// `sc_laststatus`.
    pub sc_laststatus: Cell<u8>,
}

// SAFETY: `#[repr(C)]` with the device first; the other members are `Cell`s of integers,
// pointers and `Option`s of bus handles, and the timeout, all valid as zero bits.
unsafe impl Softc for LptSoftc {}
/* </CODE> */

/* <TESTS> */
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    #[ignore = "needs OPENBSD_SRC (just test-ref)"]
    fn values_match_the_c_header() {
        let defs = crate::reftest::defines("sys/dev/ic/lptvar.h");
        let ours = crate::reftest::assert_defines!(defs;
            LPT_OPEN, LPT_OBUSY, LPT_INIT, LPT_POLLED, LPT_AUTOLF, LPT_NOPRIME, LPT_NOINTR);
        crate::reftest::assert_complete(&defs, "LPT_", &ours);
    }
}
/* </TESTS> */
