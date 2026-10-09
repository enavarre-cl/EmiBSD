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
 * Copyright (c) 1998 The NetBSD Foundation, Inc.
 * All rights reserved.
 *
 * This code is derived from software contributed to The NetBSD Foundation
 * by Lennart Augustsson (augustss@netbsd.org).
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
//! `<dev/midi_if.h>`: the interface between midi(4) and the hardware drivers.
//!
//! Upstream: sys/dev/midi_if.h @ 3ce1f3f79392
//!
//! A hardware driver (eap(4)'s UART) fills a `static` [`MidiHwIf`] and hands it, with its
//! own softc as the opaque handle, to `midi_attach_mi` (`dev/midi.rs`, where the C defines
//! it); midi(4) then calls the driver through the table with that handle first.
//!
//! ## Deviations
//! - The operations are `unsafe fn`s over the `void *` handle, as `audio_if.h`'s are
//!   (`docs/C_TO_RUST.md`, a table of function pointers over an opaque `void *`); every
//!   member is an `Option`, NULL in C.
//! - `struct midi_info`'s `name` is a `&'static str`: the drivers hand out string
//!   literals.

use core::ffi::c_void;

use crate::sys::errno::Errno;
use crate::sys::proc::Proc;

/// `MIDI_PROP_OUT_INTR`: the hardware interrupts when it can take the next byte.
pub const MIDI_PROP_OUT_INTR: i32 = 1;
/// `MIDI_PROP_CAN_INPUT`.
pub const MIDI_PROP_CAN_INPUT: i32 = 2;

/// `struct midi_info`.
#[derive(Clone, Copy, Debug, Default)]
pub struct MidiInfo {
    /// `name`: name of MIDI hardware.
    pub name: &'static str,
    /// `props`: `MIDI_PROP_*`.
    pub props: i32,
}

/// `void (*)(void *, int)`: the input call-back midi(4) hands to `open` (`midi_iintr`),
/// called with each byte received.
///
/// # Safety
///
/// The argument is the `void *` midi(4) passed together with the function, and the caller
/// holds `AUDIO_LOCK` (`audio_lock`).
pub type MidiIintr = unsafe fn(*mut c_void, i32);

/// `void (*)(void *)`: the output call-back midi(4) hands to `open` (`midi_ointr`), called
/// when the hardware can take more.
///
/// # Safety
///
/// As [`MidiIintr`].
pub type MidiOintr = unsafe fn(*mut c_void);

/// `int (*open)(void *, int, void (*)(void *, int), void (*)(void *), void *)`.
pub type MidiOpenFn =
    unsafe fn(*mut c_void, i32, MidiIintr, MidiOintr, *mut c_void) -> Result<(), Errno>;
/// `void (*close)(void *)`, and `flush`.
pub type MidiVoidFn = unsafe fn(*mut c_void);
/// `int (*output)(void *, int)`: whether the byte was taken.
pub type MidiOutputFn = unsafe fn(*mut c_void, i32) -> bool;
/// `void (*getinfo)(void *, struct midi_info *)`.
pub type MidiGetinfoFn = unsafe fn(*mut c_void, &mut MidiInfo);
/// `int (*ioctl)(void *, u_long, caddr_t, int, struct proc *)`.
pub type MidiIoctlFn = unsafe fn(*mut c_void, u64, &mut [u8], i32, &Proc) -> Result<(), Errno>;

/// `struct midi_hw_if`. Every operation takes the driver's handle first; the contract of
/// each `unsafe fn` is that it is the `hdl` the driver gave to `midi_attach_mi` together
/// with this table.
#[derive(Clone, Copy)]
pub struct MidiHwIf {
    /// `open(hdl, flags, iintr, ointr, arg)`: open hardware.
    pub open: Option<MidiOpenFn>,
    /// `close(hdl)`: close hardware.
    pub close: Option<MidiVoidFn>,
    /// `output(hdl, byte)`: output a byte.
    pub output: Option<MidiOutputFn>,
    /// `flush(hdl)`: flush the output.
    pub flush: Option<MidiVoidFn>,
    /// `getinfo(hdl, mi)`.
    pub getinfo: Option<MidiGetinfoFn>,
    /// `ioctl(hdl, cmd, addr, flag, p)`.
    pub ioctl: Option<MidiIoctlFn>,
}
/* </CODE> */

/* <TESTS> */
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    #[ignore = "needs OPENBSD_SRC (just test-ref)"]
    fn values_match_the_c_header() {
        let defs = crate::reftest::defines("sys/dev/midi_if.h");
        let ours = crate::reftest::assert_defines!(defs; MIDI_PROP_OUT_INTR, MIDI_PROP_CAN_INPUT);
        crate::reftest::assert_complete(&defs, "MIDI_PROP", &ours);
    }
}
/* </TESTS> */
