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
//! `<dev/wscons/wsemul_vt100var.h>`: the state of the vt100 terminal emulation, and the
//! helper macros its files share.
//!
//! Upstream: sys/dev/wscons/wsemul_vt100var.h @ 3ce1f3f79392
//!
//! ## Deviations
//! - `emulops` and `emulcookie` are one [`BoundEmulops`] (as in `wsemulvar`).
//! - `chartab_G[4]` and `savedchartab_G[4]` point at one of the four character tables or
//!   are NULL; here each slot is an `Option<`[`Vt100Chartab`]`>` naming the table, looked up
//!   in the state by [`WsemulVt100Emuldata::chartab`]. A slot naming a table that could not
//!   be allocated (the C's NULL table pointer) reads as no table, as in C.
//! - The `malloc`ed arrays (`tabs`, `dblwid`, `dcsarg` and the four character tables) are
//!   `Option<Vec<_>>`, `None` where the C pointer is NULL.
//! - `console` exists in every kernel (the C has it only with `DIAGNOSTIC`).
//! - `translatebuf` is the caller's buffer (`wsemulvar`'s `translate`), not a member.
//! - The macros are methods: `ARG`, `DEF1_ARG`, `DEFx_ARG` ([`arg`](WsemulVt100Emuldata::arg),
//!   [`def1_arg`](WsemulVt100Emuldata::def1_arg), [`defx_arg`](WsemulVt100Emuldata::defx_arg)),
//!   `ROWS_ABOVE`, `ROWS_BELOW`, `CHECK_DW`, `NCOLS`, `COLS_LEFT`, and `COPYCOLS`/`ERASECOLS`,
//!   which give the emulop's arguments (`copycols_args`, `erasecols_args`). Their arithmetic
//!   keeps the C's: `u_int` members, wrapping, converted to `int` with `as` where the C
//!   converts.
//! - `char modif1`/`modif2` are `u8`; the `HAVE_DOUBLE_WIDTH_HEIGHT` members exist in every
//!   kernel, and `CHECK_DW`/`NCOLS` test the constant.
//! - The prototypes belong to `wsemul_vt100.c`, `wsemul_vt100_subr.c`,
//!   `wsemul_vt100_chars.c` and `wsemul_vt100_keys.c`, whose modules define them.

use core::ffi::c_void;
use core::ptr;

use alloc::vec::Vec;

use crate::dev::wscons::wscons_features::HAVE_DOUBLE_WIDTH_HEIGHT;
use crate::dev::wscons::wsemulvar::{BoundEmulops, WsemulAbortstate, WsemulInputstate};

/// `VT100_EMUL_NARGS`: max # of args to a command.
pub const VT100_EMUL_NARGS: usize = 10;

/// `VT100_EMUL_ARG_CLAMP`: max value of arg to a command.
pub const VT100_EMUL_ARG_CLAMP: i32 = 100000;

/// `VTFL_LASTCHAR`: printed last char on line (below cursor).
pub const VTFL_LASTCHAR: i32 = 0x001;
/// `VTFL_INSERTMODE`.
pub const VTFL_INSERTMODE: i32 = 0x002;
/// `VTFL_APPLKEYPAD`.
pub const VTFL_APPLKEYPAD: i32 = 0x004;
/// `VTFL_APPLCURSOR`.
pub const VTFL_APPLCURSOR: i32 = 0x008;
/// `VTFL_DECOM`: origin mode.
pub const VTFL_DECOM: i32 = 0x010;
/// `VTFL_DECAWM`: auto wrap.
pub const VTFL_DECAWM: i32 = 0x020;
/// `VTFL_CURSORON`.
pub const VTFL_CURSORON: i32 = 0x040;
/// `VTFL_NATCHARSET`: national replacement charset mode.
pub const VTFL_NATCHARSET: i32 = 0x080;
/// `VTFL_SAVEDCURS`: we have a saved cursor state.
pub const VTFL_SAVEDCURS: i32 = 0x100;
/// `VTFL_UTF8`: utf-8 character set.
pub const VTFL_UTF8: i32 = 0x200;

/// `DCS_MAXLEN`: the size of `dcsarg`.
pub const DCS_MAXLEN: usize = 256;
/// `DCSTYPE_TABRESTORE`: DCS2$t.
pub const DCSTYPE_TABRESTORE: i32 = 1;

/// `WSEMUL_VT_ID1`: response to primary DA request. Operating level: 61 = VT100,
/// 62 = VT200, 63 = VT300; extensions: 1 = 132 cols, 2 = printer port, 6 = selective erase,
/// 7 = soft charset, 8 = UDKs, 9 = NRC sets. VT100 = "033[?1;2c".
pub const WSEMUL_VT_ID1: &[u8] = b"\x1b[?62;6c";
/// `WSEMUL_VT_ID2`: response to secondary DA request: ident code 24 = VT320, firmware
/// version, hardware options 0 = no options.
pub const WSEMUL_VT_ID2: &[u8] = b"\x1b[>24;20;0c";

/// The character table a `chartab_G` slot points at.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Vt100Chartab {
    /// `isolatin1tab`.
    Isolatin1,
    /// `decgraphtab`.
    Decgraph,
    /// `dectechtab`.
    Dectech,
    /// `nrctab`.
    Nrc,
}

/// `struct wsemul_vt100_emuldata`: the state of one vt100 screen.
pub struct WsemulVt100Emuldata {
    /// `emulops` and `emulcookie`.
    pub emulops: BoundEmulops,
    /// `abortstate`.
    pub abortstate: WsemulAbortstate,
    /// `scrcapabilities`.
    pub scrcapabilities: i32,
    /// `nrows`.
    pub nrows: u32,
    /// `ncols`.
    pub ncols: u32,
    /// `crow`.
    pub crow: u32,
    /// `ccol`.
    pub ccol: u32,
    /// `defattr`: default attribute.
    pub defattr: u32,
    /// `kernattr`: attribute for kernel output.
    pub kernattr: u32,
    /// `cbcookie`: wsdisplay's screen, the callbacks' argument.
    pub cbcookie: *mut c_void,
    /// `console` (`DIAGNOSTIC` in C).
    pub console: bool,
    /// `state`: processing state (`VT100_EMUL_STATE_*`).
    pub state: u32,
    /// `flags`: `VTFL_*`.
    pub flags: i32,
    /// `curattr`: currently used attribute.
    pub curattr: u32,
    /// `bkgdattr`.
    pub bkgdattr: u32,
    /// `attrflags`: properties of curattr.
    pub attrflags: i32,
    /// `fgcol`.
    pub fgcol: i32,
    /// `bgcol`.
    pub bgcol: i32,
    /// `scrreg_startrow`.
    pub scrreg_startrow: u32,
    /// `scrreg_nrows`.
    pub scrreg_nrows: u32,
    /// `tabs`: `ncols` tab stops.
    pub tabs: Option<Vec<u8>>,
    /// `dblwid`: `nrows` double width flags.
    pub dblwid: Option<Vec<u8>>,
    /// `dw`: the cursor row is double width.
    pub dw: i32,
    /// `chartab0`: the G set invoked into GL.
    pub chartab0: i32,
    /// `chartab1`: the G set invoked into GR.
    pub chartab1: i32,
    /// `chartab_G`.
    pub chartab_g: [Option<Vt100Chartab>; 4],
    /// `isolatin1tab`.
    pub isolatin1tab: Option<Vec<u32>>,
    /// `decgraphtab`.
    pub decgraphtab: Option<Vec<u32>>,
    /// `dectechtab`.
    pub dectechtab: Option<Vec<u32>>,
    /// `nrctab`.
    pub nrctab: Option<Vec<u32>>,
    /// `sschartab`: single shift.
    pub sschartab: i32,
    /// `nargs`.
    pub nargs: i32,
    /// `args`: numeric command args (CSI/DCS).
    pub args: [i32; VT100_EMUL_NARGS],
    /// `modif1`: {>?} in VT100_EMUL_STATE_CSI.
    pub modif1: u8,
    /// `modif2`: {!"$&} in VT100_EMUL_STATE_CSI.
    pub modif2: u8,
    /// `designating`: substate in VT100_EMUL_STATE_SCS*.
    pub designating: i32,
    /// `dcstype`: substate in VT100_EMUL_STATE_STRING.
    pub dcstype: i32,
    /// `dcsarg`.
    pub dcsarg: Option<Vec<u8>>,
    /// `dcspos`.
    pub dcspos: i32,
    /// `savedcursor_row`.
    pub savedcursor_row: u32,
    /// `savedcursor_col`.
    pub savedcursor_col: u32,
    /// `savedattr`.
    pub savedattr: u32,
    /// `savedbkgdattr`.
    pub savedbkgdattr: u32,
    /// `savedattrflags`.
    pub savedattrflags: i32,
    /// `savedfgcol`.
    pub savedfgcol: i32,
    /// `savedbgcol`.
    pub savedbgcol: i32,
    /// `savedchartab0`.
    pub savedchartab0: i32,
    /// `savedchartab1`.
    pub savedchartab1: i32,
    /// `savedchartab_G`.
    pub savedchartab_g: [Option<Vt100Chartab>; 4],
    /// `instate`: userland input state.
    pub instate: WsemulInputstate,
    /// `kstate`: kernel input state.
    pub kstate: WsemulInputstate,
}

impl WsemulVt100Emuldata {
    /// All zero and NULL, as the C's static console state before `cnattach`.
    pub const ZERO: Self = Self {
        emulops: BoundEmulops::NULL,
        abortstate: WsemulAbortstate::ZERO,
        scrcapabilities: 0,
        nrows: 0,
        ncols: 0,
        crow: 0,
        ccol: 0,
        defattr: 0,
        kernattr: 0,
        cbcookie: ptr::null_mut(),
        console: false,
        state: 0,
        flags: 0,
        curattr: 0,
        bkgdattr: 0,
        attrflags: 0,
        fgcol: 0,
        bgcol: 0,
        scrreg_startrow: 0,
        scrreg_nrows: 0,
        tabs: None,
        dblwid: None,
        dw: 0,
        chartab0: 0,
        chartab1: 0,
        chartab_g: [None; 4],
        isolatin1tab: None,
        decgraphtab: None,
        dectechtab: None,
        nrctab: None,
        sschartab: 0,
        nargs: 0,
        args: [0; VT100_EMUL_NARGS],
        modif1: 0,
        modif2: 0,
        designating: 0,
        dcstype: 0,
        dcsarg: None,
        dcspos: 0,
        savedcursor_row: 0,
        savedcursor_col: 0,
        savedattr: 0,
        savedbkgdattr: 0,
        savedattrflags: 0,
        savedfgcol: 0,
        savedbgcol: 0,
        savedchartab0: 0,
        savedchartab1: 0,
        savedchartab_g: [None; 4],
        instate: WsemulInputstate::ZERO,
        kstate: WsemulInputstate::ZERO,
    };

    /// The input state of the stream being processed: the kernel's or userland's (the
    /// `instate` argument of the C's handlers).
    pub fn instate(&mut self, kernel: bool) -> &mut WsemulInputstate {
        if kernel {
            &mut self.kstate
        } else {
            &mut self.instate
        }
    }

    /// The table a `chartab_G` slot points at, `None` for NULL.
    pub fn chartab(&self, slot: Option<Vt100Chartab>) -> Option<&[u32]> {
        let t = match slot? {
            Vt100Chartab::Isolatin1 => &self.isolatin1tab,
            Vt100Chartab::Decgraph => &self.decgraphtab,
            Vt100Chartab::Dectech => &self.dectechtab,
            Vt100Chartab::Nrc => &self.nrctab,
        };
        t.as_deref()
    }

    /// `ARG(n)`.
    pub fn arg(&self, n: usize) -> i32 {
        self.args[n]
    }

    /// `DEF1_ARG(n)`: the argument, 1 when absent (0).
    pub fn def1_arg(&self, n: usize) -> i32 {
        self.defx_arg(n, 1)
    }

    /// `DEFx_ARG(n, x)`: the argument, `x` when absent (0).
    pub fn defx_arg(&self, n: usize, x: i32) -> i32 {
        if self.args[n] != 0 { self.args[n] } else { x }
    }

    /// `ROWS_ABOVE`: rows between the scrolling region's top and the cursor; negative when
    /// the cursor is outside the scrolling region.
    pub fn rows_above(&self) -> i32 {
        self.crow as i32 - self.scrreg_startrow as i32
    }

    /// `ROWS_BELOW`: rows between the cursor and the scrolling region's bottom; negative
    /// when the cursor is outside the scrolling region.
    pub fn rows_below(&self) -> i32 {
        (self.scrreg_startrow.wrapping_add(self.scrreg_nrows)) as i32 - self.crow as i32 - 1
    }

    /// `CHECK_DW`: note whether the cursor row is double width, and keep the cursor on its
    /// visible half.
    pub fn check_dw(&mut self) {
        if !HAVE_DOUBLE_WIDTH_HEIGHT {
            return;
        }
        let dbl = self
            .dblwid
            .as_ref()
            .is_some_and(|d| d[self.crow as usize] != 0);
        if dbl {
            self.dw = 1;
            let last = (self.ncols >> 1).wrapping_sub(1);
            if self.ccol > last {
                self.ccol = last;
            }
        } else {
            self.dw = 0;
        }
    }

    /// `NCOLS`: the columns of the cursor row (half of them on a double width row).
    pub fn ncols_dw(&self) -> u32 {
        if HAVE_DOUBLE_WIDTH_HEIGHT {
            self.ncols >> self.dw
        } else {
            self.ncols
        }
    }

    /// `COLS_LEFT`: columns right of the cursor.
    pub fn cols_left(&self) -> u32 {
        self.ncols_dw().wrapping_sub(self.ccol).wrapping_sub(1)
    }

    /// The doubling shift of the cursor row, 0 without `HAVE_DOUBLE_WIDTH_HEIGHT`.
    fn dw_shift(&self) -> i32 {
        if HAVE_DOUBLE_WIDTH_HEIGHT { self.dw } else { 0 }
    }

    /// `COPYCOLS(f, t, n)`: the `copycols` arguments (row, source, destination, count) for
    /// columns of the cursor row, doubled on a double width row.
    pub fn copycols_args(&self, f: u32, t: u32, n: u32) -> (i32, i32, i32, i32) {
        let dw = self.dw_shift();
        (
            self.crow as i32,
            (f << dw) as i32,
            (t << dw) as i32,
            (n << dw) as i32,
        )
    }

    /// `ERASECOLS(f, n, a)`: the `erasecols` arguments (row, start, count, attribute).
    pub fn erasecols_args(&self, f: u32, n: u32, a: u32) -> (i32, i32, i32, u32) {
        let dw = self.dw_shift();
        (self.crow as i32, (f << dw) as i32, (n << dw) as i32, a)
    }
}

// SAFETY: the state holds the display's emulops and cookies, which the display driver and
// wsdisplay keep valid for the screen's life; wsdisplay serialises every call on one screen
// (under its tty, the console's output at spltty), so the state is used by one thread at a
// time wherever it lives.
unsafe impl Send for WsemulVt100Emuldata {}
/* </CODE> */

/* <TESTS> */
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn region_and_column_helpers() {
        let mut e = WsemulVt100Emuldata::ZERO;
        (e.nrows, e.ncols, e.scrreg_startrow, e.scrreg_nrows) = (25, 80, 5, 10);
        e.crow = 2;
        assert_eq!((e.rows_above(), e.rows_below()), (-3, 12));
        e.crow = 14;
        assert_eq!((e.rows_above(), e.rows_below()), (9, 0));
        e.ccol = 79;
        assert_eq!(e.cols_left(), 0);
        e.dblwid = Some(std::vec![0; 25]);
        e.dblwid.as_mut().unwrap()[14] = 1;
        e.check_dw();
        assert_eq!((e.dw, e.ccol, e.ncols_dw(), e.cols_left()), (1, 39, 40, 0));
        assert_eq!(e.copycols_args(1, 2, 3), (14, 2, 4, 6));
        e.args[0] = 0;
        e.args[1] = 7;
        assert_eq!((e.def1_arg(0), e.defx_arg(0, 9), e.def1_arg(1)), (1, 9, 7));
    }

    /// Every constant against `<dev/wscons/wsemul_vt100var.h>`.
    #[test]
    #[ignore = "needs OPENBSD_SRC"]
    fn constants_match_the_c_header() {
        let defs = crate::reftest::defines("sys/dev/wscons/wsemul_vt100var.h");
        crate::reftest::assert_defines!(defs;
            VT100_EMUL_NARGS, VT100_EMUL_ARG_CLAMP, VTFL_LASTCHAR, VTFL_INSERTMODE,
            VTFL_APPLKEYPAD, VTFL_APPLCURSOR, VTFL_DECOM, VTFL_DECAWM, VTFL_CURSORON,
            VTFL_NATCHARSET, VTFL_SAVEDCURS, VTFL_UTF8, DCS_MAXLEN, DCSTYPE_TABRESTORE,
        );
        assert_eq!(defs["WSEMUL_VT_ID1"], "\"\\033[?62;6c\"");
        assert_eq!(defs["WSEMUL_VT_ID2"], "\"\\033[>24;20;0c\"");
    }
}
/* </TESTS> */
