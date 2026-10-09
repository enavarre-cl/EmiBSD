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
//! The VGA's character generator and screen geometry: loading a font into one of the eight
//! font slots of plane 2 (`vga_loadchars`), choosing the slots the two halves of a
//! 512-character set come from (`vga_setfontset`), and programming the 6845 and the
//! attribute controller for a screen type (`vga_setscreentype`).
//!
//! Upstream: sys/dev/ic/vga_subr.c @ 3ce1f3f79392
//!
//! The font memory is only reachable after the sequencer and the graphics controller are
//! switched from odd/even text addressing to plain access of plane 2 at 0xa0000
//! (`fontram`); `textram` switches them back.
//!
//! ## Deviations
//! - `vga_loadchars` takes the font as a byte slice of `num * lpc` glyph rows (the C's
//!   `char *`); a shorter slice loads only the glyphs it holds.
//! - `PCDISPLAY_SOFTCURSOR` is `pcdisplay_subr.rs`'s constant: not set, so
//!   `vga_setscreentype` programs the hardware cursor's scan lines, as the C without the
//!   option does.

use crate::dev::ic::pcdisplay_subr::PCDISPLAY_SOFTCURSOR;
use crate::dev::ic::vgavar::{
    VgaHandle, vga_6845_write, vga_attr_write, vga_gdc_write, vga_ts_write,
};
use crate::dev::wscons::wsdisplayvar::{WSSCREEN_HILIT, WsscreenDescr};
use crate::machine::bus::bus_space_write_1;
use crate::machine::intr::{splhigh, splx};

/// `fontram`: program the sequencer and the graphics controller to access the character
/// generator.
fn fontram(vh: &VgaHandle) {
    // program sequencer to access character generator

    vga_ts_write!(vh, syncreset, 0x01); // synchronous reset
    vga_ts_write!(vh, wrplmask, 0x04); // write to map 2
    vga_ts_write!(vh, memmode, 0x07); // sequential addressing
    vga_ts_write!(vh, syncreset, 0x03); // clear synchronous reset

    // program graphics controller to access character generator

    vga_gdc_write!(vh, rdplanesel, 0x02); // select map 2 for cpu reads
    vga_gdc_write!(vh, mode, 0x00); // disable odd-even addressing
    vga_gdc_write!(vh, misc, 0x04); // map starts at 0xA000
}

/// `textram`: program the sequencer and the graphics controller back to video RAM.
fn textram(vh: &VgaHandle) {
    // program sequencer to access video ram

    vga_ts_write!(vh, syncreset, 0x01); // synchronous reset
    vga_ts_write!(vh, wrplmask, 0x03); // write to map 0 & 1
    vga_ts_write!(vh, memmode, 0x03); // odd-even addressing
    vga_ts_write!(vh, syncreset, 0x03); // clear synchronous reset

    // program graphics controller for text mode

    vga_gdc_write!(vh, rdplanesel, 0x00); // select map 0 for cpu reads
    vga_gdc_write!(vh, mode, 0x10); // enable odd-even addressing
    // map starts at 0xb800 or 0xb000 (mono)
    vga_gdc_write!(vh, misc, if vh.vh_mono != 0 { 0x0a } else { 0x0e });
}

/// `vga_loadchars`: glyphs `first` to `first + num - 1` of font slot `fontset`, `lpc`
/// lines per character, from `data` (`lpc` bytes per glyph).
pub fn vga_loadchars(vh: &VgaHandle, fontset: i32, first: i32, num: i32, lpc: i32, data: &[u8]) {
    // fontset number swizzle done in vga_setfontset()
    let offset = ((fontset << 13) | (first << 5)) as usize;
    let (num, lpc) = (num.max(0) as usize, lpc.max(0) as usize);

    let s = splhigh();
    fontram(vh);

    for i in 0..num {
        for j in 0..lpc {
            let Some(&b) = data.get(i * lpc + j) else {
                break;
            };
            bus_space_write_1(vh.vh_memt(), vh.vh_allmemh, offset + (i << 5) + j, b);
        }
    }

    textram(vh);
    splx(s);
}

/// `vga_setfontset`: the font slots of the low (`fontset1`) and high (`fontset2`) 256
/// characters; an extended font if they differ.
pub fn vga_setfontset(vh: &VgaHandle, fontset1: i32, fontset2: i32) {
    const CMAPTABA: [u8; 8] = [0x00, 0x10, 0x01, 0x11, 0x02, 0x12, 0x03, 0x13];
    const CMAPTABB: [u8; 8] = [0x00, 0x20, 0x04, 0x24, 0x08, 0x28, 0x0c, 0x2c];

    // extended font if fontset1 != fontset2
    let cmap = CMAPTABA[(fontset1 & 7) as usize] | CMAPTABB[(fontset2 & 7) as usize];

    vga_ts_write!(vh, fontsel, cmap);
}

/// `vga_setscreentype`: the character height and number of rows of `type_`.
pub fn vga_setscreentype(vh: &VgaHandle, type_: &WsscreenDescr) {
    vga_6845_write!(vh, maxrow, (type_.fontheight - 1) as u8);

    // lo byte
    vga_6845_write!(vh, vde, (type_.fontheight * type_.nrows - 1) as u8);

    if !PCDISPLAY_SOFTCURSOR {
        // set cursor to last 2 lines
        vga_6845_write!(vh, curstart, (type_.fontheight - 2) as u8);
        vga_6845_write!(vh, curend, (type_.fontheight - 1) as u8);
    }
    // disable colour plane 3 if needed for font selection
    if type_.capabilities & WSSCREEN_HILIT != 0 {
        // these are the screens which don't support 512-character fonts
        vga_attr_write!(vh, colplen, 0x0f);
    } else {
        vga_attr_write!(vh, colplen, 0x07);
    }
}
/* </CODE> */
