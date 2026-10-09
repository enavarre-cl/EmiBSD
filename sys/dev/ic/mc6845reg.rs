/* <LICENSES> */
/*
 * Copyright (c) 1998
 *	Matthias Drochner.  All rights reserved.
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
 *
 */
/* </LICENSES> */

/* <CODE> */
//! `<dev/ic/mc6845reg.h>`: the registers of the Motorola 6845 CRT controller, as the PC
//! display adapters (MDA, CGA, EGA, VGA) carry it.
//!
//! Upstream: sys/dev/ic/mc6845reg.h @ 3ce1f3f79392
//!
//! The registers are reached through an index and a data port; `struct reg_mc6845` only
//! gives each register its index, as the offset of a byte field, which the
//! `pcdisplay_6845_read`/`pcdisplay_6845_write` macros take with `offsetof`. Here it is the
//! `#[repr(C)]` [`RegMc6845`], whose field offsets ([`core::mem::offset_of!`]) are the
//! indices; its size is what `vga_pci.c` saves and restores.
//!
//! ## Deviations
//! - The C's fields are `char`; here they are `u8`, the register width.

/// `struct reg_mc6845`: indexed via port 0x3d4 (mono 0x3b4).
#[repr(C)]
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct RegMc6845 {
    /// `htotal`.
    pub htotal: u8,
    /// `hdisple`.
    pub hdisple: u8,
    /// `hblanks`.
    pub hblanks: u8,
    /// `hblanke`.
    pub hblanke: u8,
    /// `hsyncs`.
    pub hsyncs: u8,
    /// `hsynce`.
    pub hsynce: u8,
    /// `vtotal`.
    pub vtotal: u8,
    /// `overfll`.
    pub overfll: u8,
    /// `irowaddr`.
    pub irowaddr: u8,
    /// `maxrow`.
    pub maxrow: u8,
    /// `curstart`.
    pub curstart: u8,
    /// `curend`.
    pub curend: u8,
    /// `startadrh`.
    pub startadrh: u8,
    /// `startadrl`.
    pub startadrl: u8,
    /// `cursorh`.
    pub cursorh: u8,
    /// `cursorl`.
    pub cursorl: u8,
    /// `vsyncs`.
    pub vsyncs: u8,
    /// `vsynce`.
    pub vsynce: u8,
    /// `vde`.
    pub vde: u8,
    /// `offset`.
    pub offset: u8,
    /// `uloc`.
    pub uloc: u8,
    /// `vbstart`.
    pub vbstart: u8,
    /// `vbend`.
    pub vbend: u8,
    /// `mode`.
    pub mode: u8,
    /// `splitl`.
    pub splitl: u8,
}

/// `MC6845_INDEX`: the index port, relative to the 6845's I/O base.
pub const MC6845_INDEX: usize = 4;
/// `MC6845_DATA`: the data port.
pub const MC6845_DATA: usize = 5;
/* </CODE> */

/* <TESTS> */
#[cfg(test)]
mod tests {
    use core::mem::{offset_of, size_of};

    use super::*;

    #[test]
    fn register_indices_are_the_field_offsets() {
        assert_eq!(size_of::<RegMc6845>(), 25);
        assert_eq!(offset_of!(RegMc6845, maxrow), 9);
        assert_eq!(offset_of!(RegMc6845, startadrh), 12);
        assert_eq!(offset_of!(RegMc6845, cursorl), 15);
        assert_eq!(offset_of!(RegMc6845, vde), 18);
        assert_eq!(offset_of!(RegMc6845, mode), 23);
    }

    /// The two port offsets against `<dev/ic/mc6845reg.h>`.
    #[test]
    #[ignore = "needs OPENBSD_SRC"]
    fn constants_match_the_c_header() {
        let defs = crate::reftest::defines("sys/dev/ic/mc6845reg.h");
        crate::reftest::assert_defines!(defs; MC6845_INDEX, MC6845_DATA);
    }
}
/* </TESTS> */
