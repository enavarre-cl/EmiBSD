/* $OpenBSD: vgareg.h,v 1.5 2009/02/01 14:37:22 miod Exp $ */
/* $NetBSD: vgareg.h,v 1.2 1998/05/28 16:48:41 drochner Exp $ */
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
//! `<dev/ic/vgareg.h>`: the VGA's indexed register files beyond the 6845: the attribute
//! controller, the timing sequencer and the graphics data controller, and the DAC ports.
//!
//! Upstream: sys/dev/ic/vgareg.h @ 3ce1f3f79392
//!
//! As in `mc6845reg.rs`, each `__packed` structure only names the registers of one index
//! port: a field's offset ([`core::mem::offset_of!`]) is the register's index, which
//! `vgavar.h`'s accessor macros take with `offsetof`, and the size is what `vga_pci.c`
//! saves and restores. The `VGA_*` constants are offsets from the VGA's I/O base 0x3c0.
//!
//! ## Deviations
//! - `__packed` is `#[repr(C, packed)]`; every field is a byte, so the layout is the same.

/// `struct reg_vgaattr`: indexed via port 0x3c0.
#[repr(C, packed)]
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct RegVgaattr {
    /// `palette[16]`.
    pub palette: [u8; 16],
    /// `mode`.
    pub mode: u8,
    /// `overscan`.
    pub overscan: u8,
    /// `colplen`.
    pub colplen: u8,
    /// `horpixpan`.
    pub horpixpan: u8,
    /// `colreset`.
    pub colreset: u8,
    /// `misc`.
    pub misc: u8,
}

/// `VGA_ATC_INDEX`.
pub const VGA_ATC_INDEX: usize = 0;
/// `VGA_ATC_DATAW`.
pub const VGA_ATC_DATAW: usize = 0;
/// `VGA_ATC_DATAR`.
pub const VGA_ATC_DATAR: usize = 1;

/// `struct reg_vgats`: indexed via port 0x3c4.
#[repr(C, packed)]
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct RegVgats {
    /// `syncreset`.
    pub syncreset: u8,
    /// `mode`.
    pub mode: u8,
    /// `wrplmask`.
    pub wrplmask: u8,
    /// `fontsel`.
    pub fontsel: u8,
    /// `memmode`.
    pub memmode: u8,
}

/// `VGA_TS_INDEX`.
pub const VGA_TS_INDEX: usize = 4;
/// `VGA_TS_DATA`.
pub const VGA_TS_DATA: usize = 5;

/// `struct reg_vgagdc`: indexed via port 0x3ce.
#[repr(C, packed)]
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct RegVgagdc {
    /// `setres`.
    pub setres: u8,
    /// `ensetres`.
    pub ensetres: u8,
    /// `colorcomp`.
    pub colorcomp: u8,
    /// `rotfunc`.
    pub rotfunc: u8,
    /// `rdplanesel`.
    pub rdplanesel: u8,
    /// `mode`.
    pub mode: u8,
    /// `misc`.
    pub misc: u8,
    /// `colorcare`.
    pub colorcare: u8,
    /// `bitmask`.
    pub bitmask: u8,
}

/// `VGA_GDC_INDEX`.
pub const VGA_GDC_INDEX: usize = 0xe;
/// `VGA_GDC_DATA`.
pub const VGA_GDC_DATA: usize = 0xf;

/// `VGA_DAC_MASK`: pixel write mask.
pub const VGA_DAC_MASK: usize = 0x06;
/// `VGA_DAC_READ`: palette read address.
pub const VGA_DAC_READ: usize = 0x07;
/// `VGA_DAC_WRITE`: palette write address.
pub const VGA_DAC_WRITE: usize = 0x08;
/// `VGA_DAC_DATA`: palette data register.
pub const VGA_DAC_DATA: usize = 0x09;
/* </CODE> */

/* <TESTS> */
#[cfg(test)]
mod tests {
    use core::mem::{offset_of, size_of};

    use super::*;

    #[test]
    fn register_indices_are_the_field_offsets() {
        assert_eq!(size_of::<RegVgaattr>(), 22);
        assert_eq!(offset_of!(RegVgaattr, colplen), 0x12);
        assert_eq!(size_of::<RegVgats>(), 5);
        assert_eq!(offset_of!(RegVgats, memmode), 4);
        assert_eq!(size_of::<RegVgagdc>(), 9);
        assert_eq!(offset_of!(RegVgagdc, misc), 6);
    }

    /// Every define against `<dev/ic/vgareg.h>`.
    #[test]
    #[ignore = "needs OPENBSD_SRC"]
    fn constants_match_the_c_header() {
        let defs = crate::reftest::defines("sys/dev/ic/vgareg.h");
        let names = crate::reftest::assert_defines!(defs;
            VGA_ATC_INDEX, VGA_ATC_DATAW, VGA_ATC_DATAR, VGA_TS_INDEX, VGA_TS_DATA,
            VGA_GDC_INDEX, VGA_GDC_DATA, VGA_DAC_MASK, VGA_DAC_READ, VGA_DAC_WRITE,
            VGA_DAC_DATA,
        );
        crate::reftest::assert_complete(&defs, "VGA_", &names);
    }
}
/* </TESTS> */
