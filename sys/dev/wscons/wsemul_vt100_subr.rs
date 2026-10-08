/* $OpenBSD: wsemul_vt100_subr.c,v 1.32 2024/11/05 15:54:12 miod Exp $ */
/* $NetBSD: wsemul_vt100_subr.c,v 1.7 2000/04/28 21:56:16 mycroft Exp $ */
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
//! The vt100 emulation's commands: scrolling, erasing, the `CSI` sequences, attributes, the
//! `DCS` strings and the ANSI and DEC modes.
//!
//! Upstream: sys/dev/wscons/wsemul_vt100_subr.c @ 3ce1f3f79392
//!
//! ## Deviations
//! - `wsemul_vt100_handle_csi` takes the state and the `kernel` flag (the input state is
//!   the one `kernel` selects, as in `wsemul_vt100`).
//! - The `A3(modif1, modif2, c)` switch is a `match` on the triple.
//! - `min()` is libkern's `u_int min(u_int, u_int)` in C; the comparisons here are on
//!   `u32` with the C's conversions, and `int`/`u_int` mixes wrap as in C.
//! - `snprintf` of the `DSR` and tab stop reports is a fixed buffer [`ReportBuf`] that, as
//!   `snprintf`, keeps the first 19 bytes of a longer report.
//! - `vt100_selectattribute` returns `(attr, bkgdattr)`; `vt100_decmode` returns the result
//!   of its emulop as `Result`.
//! - `VT100_PRINTUNKNOWN`, `VT100_PRINTNOTIMPL` and `VT100_DEBUG` (debug options, off)
//!   print nothing.

use core::fmt::{self, Write};

#[cfg(test)]
use crate::dev::wscons::testutil::wsdisplay_emulinput;
use crate::dev::wscons::wscons_features::HAVE_DOUBLE_WIDTH_HEIGHT;
use crate::dev::wscons::wsdisplayvar::{
    WSATTR_BLINK, WSATTR_HILIT, WSATTR_REVERSE, WSATTR_UNDERLINE, WSATTR_WSCOLORS, WSCOL_BLACK,
    WSCOL_CYAN, WSCOL_RED, WSCOL_WHITE, WSSCREEN_BLINK, WSSCREEN_HILIT, WSSCREEN_REVERSE,
    WSSCREEN_UNDERLINE, WSSCREEN_WSCOLORS,
};
use crate::dev::wscons::wsemul_vt100::{wsemul_vt100_output_normal, wsemul_vt100_reset};
use crate::dev::wscons::wsemul_vt100var::*;
#[cfg(not(test))]
use crate::dev::wscons::wsemulvar::wsdisplay_emulinput;
use crate::dev::wscons::wsemulvar::wsemulop;
use crate::sys::errno::Errno;

/// `VTMODE_SET`.
pub const VTMODE_SET: i32 = 33;
/// `VTMODE_RESET`.
pub const VTMODE_RESET: i32 = 44;
/// `VTMODE_REPORT`.
pub const VTMODE_REPORT: i32 = 55;

/// The `char buf[20]` a report is `snprintf`ed into: keeps the first 19 bytes (the room
/// before the NUL) and counts the rest.
struct ReportBuf {
    buf: [u8; 20],
    len: usize,
}

impl ReportBuf {
    fn new() -> Self {
        Self {
            buf: [0; 20],
            len: 0,
        }
    }

    /// What the C sends: `n` clamped to `sizeof buf - 1`.
    fn bytes(&self) -> &[u8] {
        &self.buf[..self.len.min(self.buf.len() - 1)]
    }
}

impl Write for ReportBuf {
    fn write_str(&mut self, s: &str) -> fmt::Result {
        for &b in s.as_bytes() {
            if self.len < self.buf.len() - 1 {
                self.buf[self.len] = b;
            }
            self.len += 1;
        }
        Ok(())
    }
}

/// `wsemul_vt100_scrollup`: scroll up within scrolling region.
pub fn wsemul_vt100_scrollup(edp: &mut WsemulVt100Emuldata, n: i32) -> Result<(), Errno> {
    let mut n = n;
    if n as u32 > edp.scrreg_nrows {
        n = edp.scrreg_nrows as i32;
    }

    let eo = edp.emulops;
    let start = edp.scrreg_startrow;
    let help = edp.scrreg_nrows.wrapping_sub(n as u32) as i32;
    if help > 0 {
        let src = start.wrapping_add(n as u32) as i32;
        wsemulop(&mut edp.abortstate, || eo.copyrows(src, start as i32, help))?;
    }
    let (row, attr) = (start.wrapping_add(help as u32) as i32, edp.bkgdattr);
    wsemulop(&mut edp.abortstate, || eo.eraserows(row, n, attr))?;
    if HAVE_DOUBLE_WIDTH_HEIGHT {
        if let Some(d) = &mut edp.dblwid {
            let (start, n, help) = (start as usize, n as usize, help as usize);
            if help > 0 {
                d.copy_within(start + n..start + n + help, start);
            }
            d[start + help..start + help + n].fill(0);
        }
        edp.check_dw();
    }

    Ok(())
}

/// `wsemul_vt100_scrolldown`: scroll down within scrolling region.
pub fn wsemul_vt100_scrolldown(edp: &mut WsemulVt100Emuldata, n: i32) -> Result<(), Errno> {
    let mut n = n;
    if n as u32 > edp.scrreg_nrows {
        n = edp.scrreg_nrows as i32;
    }

    let eo = edp.emulops;
    let start = edp.scrreg_startrow;
    let help = edp.scrreg_nrows.wrapping_sub(n as u32) as i32;
    if help > 0 {
        let dst = start.wrapping_add(n as u32) as i32;
        wsemulop(&mut edp.abortstate, || eo.copyrows(start as i32, dst, help))?;
    }
    let attr = edp.bkgdattr;
    wsemulop(&mut edp.abortstate, || eo.eraserows(start as i32, n, attr))?;
    if HAVE_DOUBLE_WIDTH_HEIGHT {
        if let Some(d) = &mut edp.dblwid {
            let (start, n, help) = (start as usize, n as usize, help as usize);
            if help > 0 {
                d.copy_within(start..start + help, start + n);
            }
            d[start..start + n].fill(0);
        }
        edp.check_dw();
    }

    Ok(())
}

/// `wsemul_vt100_ed`: erase in display: cursor to end (0), beginning to cursor (1),
/// complete display (2).
pub fn wsemul_vt100_ed(edp: &mut WsemulVt100Emuldata, arg: i32) -> Result<(), Errno> {
    let eo = edp.emulops;

    match arg {
        0 => {
            // cursor to end
            let (row, f, n, a) = edp.erasecols_args(edp.ccol, edp.cols_left() + 1, edp.bkgdattr);
            wsemulop(&mut edp.abortstate, || eo.erasecols(row, f, n, a))?;
            let n = edp.nrows.wrapping_sub(edp.crow).wrapping_sub(1) as i32;
            if n > 0 {
                let (row, attr) = (edp.crow as i32 + 1, edp.bkgdattr);
                wsemulop(&mut edp.abortstate, || eo.eraserows(row, n, attr))?;
                if HAVE_DOUBLE_WIDTH_HEIGHT && let Some(d) = &mut edp.dblwid {
                    let from = edp.crow as usize + 1;
                    d[from..from + n as usize].fill(0);
                }
            }
        }
        1 => {
            // beginning to cursor
            if edp.crow > 0 {
                let (rows, attr) = (edp.crow as i32, edp.bkgdattr);
                wsemulop(&mut edp.abortstate, || eo.eraserows(0, rows, attr))?;
            }
            let (row, f, n, a) = edp.erasecols_args(0, edp.ccol + 1, edp.bkgdattr);
            wsemulop(&mut edp.abortstate, || eo.erasecols(row, f, n, a))?;
            if HAVE_DOUBLE_WIDTH_HEIGHT
                && let Some(d) = &mut edp.dblwid
                && edp.crow > 0
            {
                d[..edp.crow as usize].fill(0);
            }
        }
        2 => {
            // complete display
            let (rows, attr) = (edp.nrows as i32, edp.bkgdattr);
            wsemulop(&mut edp.abortstate, || eo.eraserows(0, rows, attr))?;
            if HAVE_DOUBLE_WIDTH_HEIGHT && let Some(d) = &mut edp.dblwid {
                d.fill(0);
            }
        }
        _ => {}
    }

    edp.check_dw();

    Ok(())
}

/// `wsemul_vt100_el`: erase in line: cursor to end (0), beginning to cursor (1), complete
/// line (2).
pub fn wsemul_vt100_el(edp: &mut WsemulVt100Emuldata, arg: i32) -> Result<(), Errno> {
    let eo = edp.emulops;
    let (row, f, n, a) = match arg {
        // cursor to end
        0 => edp.erasecols_args(edp.ccol, edp.cols_left() + 1, edp.bkgdattr),
        // beginning to cursor
        1 => edp.erasecols_args(0, edp.ccol + 1, edp.bkgdattr),
        // complete line
        2 => (edp.crow as i32, 0, edp.ncols as i32, edp.bkgdattr),
        _ => return Ok(()),
    };
    wsemulop(&mut edp.abortstate, || eo.erasecols(row, f, n, a))
}

/// `u_int min(u_int, u_int)` of libkern, on the C's converted operands.
fn min(a: u32, b: u32) -> u32 {
    a.min(b)
}

/// `wsemul_vt100_handle_csi`: handle commands after CSI (ESC[).
pub fn wsemul_vt100_handle_csi(edp: &mut WsemulVt100Emuldata, kernel: bool) -> Result<(), Errno> {
    let inchar = edp.instate(kernel).inchar;
    // a character past 0xff ends in the default case
    let c: u8 = if inchar >= 0x100 { 0 } else { inchar as u8 };
    let eo = edp.emulops;
    let mut rc = Ok(());

    match (edp.modif1, edp.modif2, c) {
        (b'>', 0, b'c') => {
            // DA secondary
            wsdisplay_emulinput(edp.cbcookie, WSEMUL_VT_ID2);
        }
        // ED selective erase in display, DECSED selective erase in display
        (0 | b'?', 0, b'J') => rc = wsemul_vt100_ed(edp, edp.arg(0)),
        // EL selective erase in line, DECSEL selective erase in line
        (0 | b'?', 0, b'K') => rc = wsemul_vt100_el(edp, edp.arg(0)),
        (0, 0, b'h') => {
            // SM
            for n in 0..edp.nargs as usize {
                vt100_ansimode(edp, edp.arg(n), VTMODE_SET);
            }
        }
        (b'?', 0, b'h') => {
            // DECSM
            for n in 0..edp.nargs as usize {
                rc = vt100_decmode(edp, edp.arg(n), VTMODE_SET);
                if rc.is_err() {
                    break;
                }
            }
        }
        (0, 0, b'l') => {
            // RM
            for n in 0..edp.nargs as usize {
                vt100_ansimode(edp, edp.arg(n), VTMODE_RESET);
            }
        }
        (b'?', 0, b'l') => {
            // DECRM
            for n in 0..edp.nargs as usize {
                rc = vt100_decmode(edp, edp.arg(n), VTMODE_RESET);
                if rc.is_err() {
                    break;
                }
            }
        }
        (0, b'$', b'p') => {
            // DECRQM request mode ANSI
            vt100_ansimode(edp, edp.arg(0), VTMODE_REPORT);
        }
        (b'?', b'$', b'p') => {
            // DECRQM request mode DEC
            rc = vt100_decmode(edp, edp.arg(0), VTMODE_REPORT);
        }
        (0 | b'?', 0, b'i') => {
            // MC printer controller mode: print screen (0), print cursor line (1), off
            // (4), on (5): ignored
        }
        (0, b'!', b'p') => {
            // DECSTR soft reset VT300 only
            wsemul_vt100_reset(edp);
        }
        (0, b'"', b'p') => {
            // DECSCL: VT100 mode (61, no further arguments!), VT300 mode (62, 63);
            // 7-bit (1) or 8-bit (0, 2) controls: ignored
        }
        (0, b'"', b'q') => {
            // DECSCA select character attribute VT300: erasable (0, 1), not erasable (2):
            // ignored
        }
        (0, b'$', b'u') => {
            // DECRQTSR request terminal status report: ignored
        }
        (0, b'$', b'w') => {
            // DECRQPSR request presentation status report (VT300 only)
            match edp.arg(0) {
                0 => {} // error
                1 => {} // cursor information report: ignored
                2 => {
                    // tab stop report
                    wsdisplay_emulinput(edp.cbcookie, b"\x1bP2$u");
                    let mut ps = false;
                    if let Some(tabs) = &edp.tabs {
                        let ncols = edp.ncols as usize;
                        for (i, &t) in tabs.iter().enumerate().take(ncols) {
                            if t != 0 {
                                let mut buf = ReportBuf::new();
                                let _ = write!(buf, "{}{}", if ps { "/" } else { "" }, i + 1);
                                wsdisplay_emulinput(edp.cbcookie, buf.bytes());
                                ps = true;
                            }
                        }
                    }
                    wsdisplay_emulinput(edp.cbcookie, b"\x1b\\");
                }
                _ => {}
            }
        }
        (0, b'$', b'}') => {
            // DECSASD select active status display: main display (0), status line (1):
            // ignored
        }
        (0, b'$', b'~') => {
            // DECSSDD select status line type: none (0), indicator (1), host-writable (2):
            // ignored
        }
        (0, b'&', b'u') => {
            // DECRQUPSS request user preferred supplemental set
            wsdisplay_emulinput(edp.cbcookie, b"\x1bP0!u%5\x1b\\");
        }
        (0, 0, b'@') => {
            // ICH insert character VT300 only
            let n = min(edp.def1_arg(0) as u32, edp.cols_left().wrapping_add(1));
            let help = edp.ncols_dw().wrapping_sub(edp.ccol.wrapping_add(n)) as i32;
            if help > 0 {
                let (row, f, t, k) = edp.copycols_args(edp.ccol, edp.ccol + n, help as u32);
                rc = wsemulop(&mut edp.abortstate, || eo.copycols(row, f, t, k));
            }
            if rc.is_ok() {
                let (row, f, k, a) = edp.erasecols_args(edp.ccol, n, edp.bkgdattr);
                rc = wsemulop(&mut edp.abortstate, || eo.erasecols(row, f, k, a));
            }
        }
        (0, 0, b'A') => {
            // CUU
            let n = edp.rows_above();
            if n > 0 {
                edp.crow -= min(edp.def1_arg(0) as u32, n as u32);
            }
            edp.check_dw();
        }
        (0, 0, b'B') => {
            // CUD
            let n = edp.rows_below();
            if n > 0 {
                edp.crow += min(edp.def1_arg(0) as u32, n as u32);
            }
            edp.check_dw();
        }
        (0, 0, b'C') => {
            // CUF
            edp.ccol += min(edp.def1_arg(0) as u32, edp.cols_left());
        }
        (0, 0, b'D') => {
            // CUB
            edp.ccol -= min(edp.def1_arg(0) as u32, edp.ccol);
            edp.flags &= !VTFL_LASTCHAR;
        }
        (0, 0, b'G' | b'`') => {
            // CHA, HPA
            edp.ccol = min(edp.def1_arg(0) as u32, edp.ncols).wrapping_sub(1);
        }
        (0, 0, b'H' | b'f') => {
            // CUP, HVP
            if edp.flags & VTFL_DECOM != 0 {
                edp.crow = edp
                    .scrreg_startrow
                    .wrapping_add(min(edp.def1_arg(0) as u32, edp.scrreg_nrows))
                    .wrapping_sub(1);
            } else {
                edp.crow = min(edp.def1_arg(0) as u32, edp.nrows).wrapping_sub(1);
            }
            edp.check_dw();
            edp.ccol = min(edp.def1_arg(1) as u32, edp.ncols_dw()).wrapping_sub(1);
            edp.flags &= !VTFL_LASTCHAR;
        }
        (0, 0, b'L' | b'M') => {
            // IL insert line, DL delete line
            if edp.crow >= edp.scrreg_startrow
                && edp.crow < edp.scrreg_startrow.wrapping_add(edp.scrreg_nrows)
            {
                let n = min(edp.def1_arg(0) as u32, (edp.rows_below() + 1) as u32) as i32;
                let savscrstartrow = edp.scrreg_startrow;
                let savscrnrows = edp.scrreg_nrows;
                edp.scrreg_nrows = edp.scrreg_nrows.wrapping_sub(edp.rows_above() as u32);
                edp.scrreg_startrow = edp.crow;
                rc = if c == b'L' {
                    wsemul_vt100_scrolldown(edp, n)
                } else {
                    wsemul_vt100_scrollup(edp, n)
                };
                edp.scrreg_startrow = savscrstartrow;
                edp.scrreg_nrows = savscrnrows;
            } // else not within scrolling region, ignore the sequence
        }
        (0, 0, b'P') => {
            // DCH delete character
            let n = min(edp.def1_arg(0) as u32, edp.cols_left().wrapping_add(1));
            let help = edp.ncols_dw().wrapping_sub(edp.ccol.wrapping_add(n)) as i32;
            if help > 0 {
                let (row, f, t, k) = edp.copycols_args(edp.ccol + n, edp.ccol, help as u32);
                rc = wsemulop(&mut edp.abortstate, || eo.copycols(row, f, t, k));
            }
            if rc.is_ok() {
                let (row, f, k, a) =
                    edp.erasecols_args(edp.ncols_dw().wrapping_sub(n), n, edp.bkgdattr);
                rc = wsemulop(&mut edp.abortstate, || eo.erasecols(row, f, k, a));
            }
        }
        (0, 0, b'S') => {
            // SU scroll up
            let _ = wsemul_vt100_scrollup(edp, edp.def1_arg(0));
        }
        (0, 0, b'T') => {
            // SD scroll down
            let _ = wsemul_vt100_scrolldown(edp, edp.def1_arg(0));
        }
        (0, 0, b'X') => {
            // ECH erase character
            let n = min(edp.def1_arg(0) as u32, edp.cols_left().wrapping_add(1));
            let (row, f, k, a) = edp.erasecols_args(edp.ccol, n, edp.bkgdattr);
            rc = wsemulop(&mut edp.abortstate, || eo.erasecols(row, f, k, a));
        }
        (0, 0, b'Z') => {
            // CBT
            if edp.ccol != 0 {
                for _ in 0..edp.def1_arg(0) {
                    let mut n = edp.ccol as i32 - 1;
                    if let Some(tabs) = &edp.tabs {
                        while n > 0 && tabs[n as usize] == 0 {
                            n -= 1;
                        }
                    } else {
                        n = (edp.ccol as i32 - 1) & !7;
                    }
                    edp.ccol = n as u32;
                    if n == 0 {
                        break;
                    }
                }
            }
        }
        (0, 0, b'b') => {
            // REP
            //
            // We arbitrarily limit the repeat count to 65535 to avoid uninterruptible
            // flooding of the console. This matches current xterm behaviour.
            let n = min(edp.def1_arg(0) as u32, 65535) as i32;
            let st = edp.instate(kernel);
            st.inchar = st.last_output;
            rc = wsemul_vt100_output_normal(edp, kernel, n);
        }
        (0, 0, b'c') => {
            // DA primary
            if edp.arg(0) == 0 {
                wsdisplay_emulinput(edp.cbcookie, WSEMUL_VT_ID1);
            }
        }
        (0, 0, b'd') => {
            // VPA
            edp.crow = min(edp.def1_arg(0) as u32, edp.nrows).wrapping_sub(1);
        }
        (0, 0, b'g') => {
            // TBC
            let ccol = edp.ccol as usize;
            if let Some(tabs) = &mut edp.tabs {
                match edp.args[0] {
                    0 => tabs[ccol] = 0,
                    3 => tabs.fill(0),
                    _ => {}
                }
            }
        }
        (0, 0, b'm') => {
            // SGR select graphic rendition
            let mut flags = edp.attrflags;
            let mut fgcol = edp.fgcol;
            let mut bgcol = edp.bgcol;
            for n in 0..edp.nargs {
                let a = edp.arg(n as usize);
                match a {
                    0 => {
                        // reset
                        if n == edp.nargs - 1 {
                            edp.curattr = edp.defattr;
                            edp.bkgdattr = edp.defattr;
                            edp.attrflags = 0;
                            edp.fgcol = WSCOL_WHITE;
                            edp.bgcol = WSCOL_BLACK;
                            return Ok(());
                        }
                        flags = 0;
                        fgcol = WSCOL_WHITE;
                        bgcol = WSCOL_BLACK;
                    }
                    1 => flags |= WSATTR_HILIT,       // bold
                    4 => flags |= WSATTR_UNDERLINE,   // underline
                    5 => flags |= WSATTR_BLINK,       // blink
                    7 => flags |= WSATTR_REVERSE,     // reverse
                    22 => flags &= !WSATTR_HILIT,     // ~bold VT300 only
                    24 => flags &= !WSATTR_UNDERLINE, // ~underline VT300 only
                    25 => flags &= !WSATTR_BLINK,     // ~blink VT300 only
                    27 => flags &= !WSATTR_REVERSE,   // ~reverse VT300 only
                    30..=37 => {
                        // fg color
                        flags |= WSATTR_WSCOLORS;
                        fgcol = a - 30;
                    }
                    39 => {
                        // reset fg color
                        fgcol = WSCOL_WHITE;
                        if bgcol == WSCOL_BLACK {
                            flags &= !WSATTR_WSCOLORS;
                        }
                    }
                    40..=47 => {
                        // bg color
                        flags |= WSATTR_WSCOLORS;
                        bgcol = a - 40;
                    }
                    49 => {
                        // reset bg color
                        bgcol = WSCOL_BLACK;
                        if fgcol == WSCOL_WHITE {
                            flags &= !WSATTR_WSCOLORS;
                        }
                    }
                    90..=97 => {
                        // bright foreground color
                        flags |= WSATTR_WSCOLORS;
                        fgcol = a - 82;
                    }
                    100..=107 => {
                        // bright background color
                        flags |= WSATTR_WSCOLORS;
                        bgcol = a - 92;
                    }
                    _ => {}
                }
            }
            if let Ok((attr, bkgdattr)) = vt100_selectattribute(edp, flags, fgcol, bgcol) {
                edp.curattr = attr;
                edp.bkgdattr = bkgdattr;
                edp.attrflags = flags;
                edp.fgcol = fgcol;
                edp.bgcol = bgcol;
            }
        }
        (0, 0, b'n') => {
            // reports
            match edp.arg(0) {
                5 => {
                    // DSR operating status: 0 = OK, 3 = malfunction
                    wsdisplay_emulinput(edp.cbcookie, b"\x1b[0n");
                }
                6 => {
                    // DSR cursor position report
                    let row = if edp.flags & VTFL_DECOM != 0 {
                        edp.rows_above().max(0)
                    } else {
                        edp.crow as i32
                    };
                    let mut buf = ReportBuf::new();
                    let _ = write!(buf, "\x1b[{};{}R", row + 1, edp.ccol.wrapping_add(1) as i32);
                    wsdisplay_emulinput(edp.cbcookie, buf.bytes());
                }
                15 => {
                    // DSR printer status: 13 = no printer, 10 = ready, 11 = not ready
                    wsdisplay_emulinput(edp.cbcookie, b"\x1b[?13n");
                }
                25 => {
                    // UDK status - VT300 only: 20 = locked, 21 = unlocked
                    wsdisplay_emulinput(edp.cbcookie, b"\x1b[?21n");
                }
                26 => {
                    // keyboard dialect: 1 = north american, 7 = german
                    wsdisplay_emulinput(edp.cbcookie, b"\x1b[?27;1n");
                }
                _ => {}
            }
        }
        (0, 0, b'r') => {
            // DECSTBM set top/bottom margins
            let help = min(edp.def1_arg(0) as u32, edp.nrows).wrapping_sub(1) as i32;
            let bottom = if edp.arg(1) != 0 {
                edp.arg(1) as u32
            } else {
                edp.nrows
            };
            let n = min(bottom, edp.nrows).wrapping_sub(help as u32) as i32;
            if n < 2 {
                // minimal scrolling region has 2 lines
                return Ok(());
            }
            edp.scrreg_startrow = help as u32;
            edp.scrreg_nrows = n as u32;
            edp.crow = if edp.flags & VTFL_DECOM != 0 {
                edp.scrreg_startrow
            } else {
                0
            };
            edp.ccol = 0;
        }
        (0, 0, b's') => {
            edp.flags |= VTFL_SAVEDCURS;
            edp.savedcursor_row = edp.crow;
            edp.savedcursor_col = edp.ccol;
        }
        (0, 0, b'u') => {
            if edp.flags & VTFL_SAVEDCURS != 0 {
                edp.crow = edp.savedcursor_row;
                edp.ccol = edp.savedcursor_col;
            }
        }
        (0, 0, b'y') => {
            // DECTST invoke confidence test (4): ignore
        }
        _ => {}
    }

    rc
}

/// `vt100_selectattribute`: get an attribute from the graphics driver, try to find
/// replacements if the desired appearance is not supported; `(attr, bkgdattr)`.
pub fn vt100_selectattribute(
    edp: &WsemulVt100Emuldata,
    flags: i32,
    fgcol: i32,
    bgcol: i32,
) -> Result<(u32, u32), Errno> {
    let (mut flags, mut fgcol, mut bgcol) = (flags, fgcol, bgcol);
    let caps = edp.scrcapabilities;

    if flags & WSATTR_WSCOLORS != 0 && caps & WSSCREEN_WSCOLORS == 0 {
        flags &= !WSATTR_WSCOLORS; // colors ignored (impossible)
    }
    let bkgdattr = edp
        .emulops
        .pack_attr(fgcol, bgcol, flags & WSATTR_WSCOLORS)?;

    if flags & WSATTR_HILIT != 0 && caps & WSSCREEN_HILIT == 0 {
        flags &= !WSATTR_HILIT;
        if caps & WSSCREEN_WSCOLORS != 0 {
            fgcol = WSCOL_RED;
            flags |= WSATTR_WSCOLORS;
        } // else bold ignored (impossible)
    }
    if flags & WSATTR_UNDERLINE != 0 && caps & WSSCREEN_UNDERLINE == 0 {
        flags &= !WSATTR_UNDERLINE;
        if caps & WSSCREEN_WSCOLORS != 0 {
            fgcol = WSCOL_CYAN;
            flags &= !WSATTR_UNDERLINE;
            flags |= WSATTR_WSCOLORS;
        } // else underline ignored (impossible)
    }
    if flags & WSATTR_BLINK != 0 && caps & WSSCREEN_BLINK == 0 {
        flags &= !WSATTR_BLINK; // blink ignored (impossible)
    }
    if flags & WSATTR_REVERSE != 0 && caps & WSSCREEN_REVERSE == 0 {
        flags &= !WSATTR_REVERSE;
        if caps & WSSCREEN_WSCOLORS != 0 {
            core::mem::swap(&mut fgcol, &mut bgcol);
            flags |= WSATTR_WSCOLORS;
        } // else reverse ignored (impossible)
    }
    let attr = edp.emulops.pack_attr(fgcol, bgcol, flags)?;

    Ok((attr, bkgdattr))
}

/// `wsemul_vt100_handle_dcs`: handle device control sequences if the main state machine
/// told so by setting `edp.dcstype` to a nonzero value.
pub fn wsemul_vt100_handle_dcs(edp: &mut WsemulVt100Emuldata) {
    match edp.dcstype {
        0 => return, // not handled
        DCSTYPE_TABRESTORE => {
            let ncols = edp.ncols;
            let dcspos = edp.dcspos.max(0) as usize;
            if let Some(tabs) = &mut edp.tabs {
                tabs.fill(0);
                let mut pos: i32 = 0;
                'out: {
                    let Some(dcsarg) = &edp.dcsarg else {
                        break 'out;
                    };
                    for &c in &dcsarg[..dcspos] {
                        match c {
                            b'0'..=b'9' => {
                                pos = pos * 10 + i32::from(c - b'0');
                                if pos as u32 > ncols {
                                    break 'out;
                                }
                            }
                            b'/' => {
                                if pos > 0 {
                                    tabs[pos as usize - 1] = 1;
                                }
                                pos = 0;
                            }
                            _ => {}
                        }
                    }
                    if pos > 0 {
                        tabs[pos as usize - 1] = 1;
                    }
                }
            }
        }
        _ => {}
    }
    edp.dcstype = 0;
}

/// `vt100_ansimode`: set, reset or report ANSI mode `nr`; the report code (0 unknown, 1
/// set, 2 reset, 4 permanently reset).
pub fn vt100_ansimode(edp: &mut WsemulVt100Emuldata, nr: i32, op: i32) -> i32 {
    match nr {
        2 => 0, // KAM keyboard locked/unlocked
        3 => 0, // CRM control representation
        4 => {
            // IRM insert/replace characters
            if op == VTMODE_SET {
                edp.flags |= VTFL_INSERTMODE;
            } else if op == VTMODE_RESET {
                edp.flags &= !VTFL_INSERTMODE;
            }
            if edp.flags & VTFL_INSERTMODE != 0 {
                1
            } else {
                2
            }
        }
        10 => 4, // HEM horizontal editing (permanently reset)
        12 => 4, // SRM local echo off/on: permanently reset ???
        20 => 0, // LNM newline = newline/linefeed
        _ => 0,
    }
}

/// `vt100_decmode`: set, reset or report DEC private mode `nr`.
pub fn vt100_decmode(edp: &mut WsemulVt100Emuldata, nr: i32, op: i32) -> Result<(), Errno> {
    let mut flags = edp.flags;
    let mut rc = Ok(());

    // Set or clear `bit` of `flags` as `op` says.
    let apply = |flags: &mut i32, bit: i32| {
        if op == VTMODE_SET {
            *flags |= bit;
        } else if op == VTMODE_RESET {
            *flags &= !bit;
        }
    };

    match nr {
        1 => apply(&mut flags, VTFL_APPLCURSOR), // DECCKM application/nomal cursor keys
        2 => {}                                  // DECANM ANSI vt100/vt52
        // DECCOLM 132/80 cols, DECSCLM smooth/jump scroll, DECSCNM light/dark background
        3..=5 => {}
        6 => apply(&mut flags, VTFL_DECOM), // DECOM move within/outside margins
        7 => apply(&mut flags, VTFL_DECAWM), // DECAWM autowrap
        8 => {}                             // DECARM keyboard autorepeat
        18 => {}                            // DECPFF print form feed
        19 => {}                            // DECPEX printer extent: screen/scrolling region
        25 => {
            // DECTCEM text cursor on/off
            apply(&mut flags, VTFL_CURSORON);
            if flags != edp.flags {
                let eo = edp.emulops;
                let (on, row, col) = (flags & VTFL_CURSORON, edp.crow as i32, edp.ccol as i32);
                rc = wsemulop(&mut edp.abortstate, || eo.cursor(on, row, col));
            }
        }
        // DECNRCM use 7-bit NRC / 7/8 bit from DEC multilingual or ISO-latin-1
        42 => apply(&mut flags, VTFL_NATCHARSET),
        66 => {} // DECNKM numeric keypad
        68 => {} // DECKBUM keyboard usage data processing/typewriter
        _ => {}
    }
    edp.flags = flags;

    rc
}
/* </CODE> */

/* <TESTS> */
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn reports_are_cut_as_snprintf_cuts_them() {
        let mut b = ReportBuf::new();
        let _ = write!(b, "\x1b[{};{}R", 12, 345);
        assert_eq!(b.bytes(), b"\x1b[12;345R");
        let mut b = ReportBuf::new();
        let _ = write!(b, "\x1b[{};{}R", 1234567890, 1234567890);
        assert_eq!(b.bytes(), b"\x1b[1234567890;123456");
    }
}
/* </TESTS> */
