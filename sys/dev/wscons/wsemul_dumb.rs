/* $OpenBSD: wsemul_dumb.c,v 1.14 2020/05/25 09:55:49 jsg Exp $ */
/* $NetBSD: wsemul_dumb.c,v 1.7 2000/01/05 11:19:36 drochner Exp $ */
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
//! The `dumb` terminal emulation: printable characters, `BEL`, `BS`, `CR`, `HT`, `FF`, `VT`
//! and `LF`, wrapping at the right margin and scrolling at the bottom; no escape sequences.
//!
//! Upstream: sys/dev/wscons/wsemul_dumb.c @ 3ce1f3f79392
//!
//! A display whose emulops lack `cursor`, `copycols`, `copyrows`, `erasecols` or
//! `eraserows` is "crippled": every byte but `BEL` is then drawn at the origin.
//!
//! Compiled with option `WSEMUL_DUMB` only (`files.wscons`), which amd64 and arm64
//! `GENERIC` do not set: `wsemulconf` offers it with the cargo feature `wsemul_dumb`.
//!
//! ## Deviations
//! - The module is compiled on every build (its host tests run); only `wsemulconf`'s list
//!   depends on the feature.
//! - The console state is a `StaticCell`; another screen's state is a `Box` (the C's
//!   `malloc(M_WAITOK)`, which cannot fail; `Box::new` cannot return NULL either).
//! - `attach` computes `crippled` for a new screen as `cnattach` does; the C leaves the
//!   member of the `malloc`ed state uninitialised. A NULL screen type outside the console
//!   case (the C would dereference it) yields NULL.
//! - `output` skips the byte of an `ABORT_FAILED_CURSOR` retry only when there is one (the
//!   C would run its loop over a wrapped count for an empty write).
//! - `translate` returns no bytes, as the C's 0.

use core::ffi::c_void;
use core::ptr;

use alloc::boxed::Box;
use libkern::StaticCell;

use crate::dev::wscons::ascii::*;
#[cfg(test)]
use crate::dev::wscons::testutil::wsdisplay_emulbell;
use crate::dev::wscons::wsdisplayvar::WsscreenDescr;
#[cfg(not(test))]
use crate::dev::wscons::wsemulvar::wsdisplay_emulbell;
use crate::dev::wscons::wsemulvar::{
    ABORT_FAILED_CURSOR, ABORT_OK, BoundEmulops, WSEMUL_CLEARCURSOR, WSEMUL_CLEARSCREEN,
    WSEMUL_TRANSLATE_SIZE, WsemulAbortstate, WsemulOps, WsemulResetops, wsemul_abort_cursor,
    wsemul_abort_other, wsemul_reset_abortstate, wsemul_resume_abort, wsemulop,
};
use crate::dev::wscons::wsksymvar::{KbdT, KeysymT};
use crate::sys::errno::Errno;

/// `struct wsemul_dumb_emuldata`: the state of one dumb screen.
pub struct WsemulDumbEmuldata {
    /// `emulops` and `emulcookie`.
    pub emulops: BoundEmulops,
    /// `abortstate`.
    pub abortstate: WsemulAbortstate,
    /// `cbcookie`: wsdisplay's screen, the callbacks' argument.
    pub cbcookie: *mut c_void,
    /// `crippled`: the display lacks an emulop other than `putchar`.
    pub crippled: bool,
    /// `nrows`.
    pub nrows: u32,
    /// `ncols`.
    pub ncols: u32,
    /// `crow`.
    pub crow: u32,
    /// `ccol`.
    pub ccol: u32,
    /// `defattr`.
    pub defattr: u32,
}

impl WsemulDumbEmuldata {
    /// All zero, as the C's static console state before `cnattach`.
    pub const ZERO: Self = Self {
        emulops: BoundEmulops::NULL,
        abortstate: WsemulAbortstate::ZERO,
        cbcookie: ptr::null_mut(),
        crippled: false,
        nrows: 0,
        ncols: 0,
        crow: 0,
        ccol: 0,
        defattr: 0,
    };

    /// The state of a screen of type `type_` drawn through `cookie`.
    ///
    /// # Safety
    ///
    /// As [`BoundEmulops::new`] for `type_.textops` and `cookie`.
    unsafe fn init(
        type_: &WsscreenDescr,
        cookie: *mut c_void,
        ccol: i32,
        crow: i32,
        defattr: u32,
    ) -> Self {
        // SAFETY: the caller's contract.
        let emulops = unsafe { BoundEmulops::new(type_.textops, cookie) };
        let crippled = emulops.table().is_none_or(|t| {
            t.cursor.is_none()
                || t.copycols.is_none()
                || t.copyrows.is_none()
                || t.erasecols.is_none()
                || t.eraserows.is_none()
        });
        let mut abortstate = WsemulAbortstate::ZERO;
        wsemul_reset_abortstate(&mut abortstate);
        Self {
            emulops,
            abortstate,
            cbcookie: ptr::null_mut(),
            crippled,
            nrows: type_.nrows as u32,
            ncols: type_.ncols as u32,
            crow: crow as u32,
            ccol: ccol as u32,
            defattr,
        }
    }
}

// SAFETY: the state holds the display's emulops and cookies, valid for the screen's life;
// wsdisplay serialises every call on one screen, so one thread at a time uses it.
unsafe impl Send for WsemulDumbEmuldata {}

/// `wsemul_dumb_ops`.
pub static WSEMUL_DUMB_OPS: WsemulOps = WsemulOps {
    name: WsemulOps::name_of(b"dumb"),
    cnattach: wsemul_dumb_cnattach,
    attach: wsemul_dumb_attach,
    output: wsemul_dumb_output,
    translate: wsemul_dumb_translate,
    detach: wsemul_dumb_detach,
    reset: wsemul_dumb_resetop,
};

/// `wsemul_dumb_console_emuldata`: written by `cnattach` while cold, then reached only
/// through the cookie it returned.
static WSEMUL_DUMB_CONSOLE_EMULDATA: StaticCell<WsemulDumbEmuldata> =
    StaticCell::new(WsemulDumbEmuldata::ZERO);

/// `wsemul_dumb_cnattach`.
///
/// # Safety
///
/// `type_.textops` is NULL or a table valid for the console's life, paired with `cookie`;
/// called while cold, before any other use of the console state.
pub unsafe fn wsemul_dumb_cnattach(
    type_: &WsscreenDescr,
    cookie: *mut c_void,
    ccol: i32,
    crow: i32,
    defattr: u32,
) -> *mut c_void {
    // SAFETY: the caller's contract (one CPU, cold, nothing else holds the state).
    let edp = unsafe { WSEMUL_DUMB_CONSOLE_EMULDATA.get_mut() };
    // SAFETY: the caller's contract on `textops` and `cookie`.
    *edp = unsafe { WsemulDumbEmuldata::init(type_, cookie, ccol, crow, defattr) };
    WSEMUL_DUMB_CONSOLE_EMULDATA.as_ptr().cast()
}

/// `wsemul_dumb_attach`.
///
/// # Safety
///
/// For the console, `cnattach` ran and no other reference to its state is live; otherwise
/// `type_.textops` is NULL or a table valid for the screen's life, paired with `cookie`.
pub unsafe fn wsemul_dumb_attach(
    console: bool,
    type_: Option<&WsscreenDescr>,
    cookie: *mut c_void,
    ccol: i32,
    crow: i32,
    cbcookie: *mut c_void,
    defattr: u32,
) -> *mut c_void {
    let edp: *mut WsemulDumbEmuldata = if console {
        WSEMUL_DUMB_CONSOLE_EMULDATA.as_ptr()
    } else {
        let Some(type_) = type_ else {
            return ptr::null_mut();
        };
        // SAFETY: the caller's contract on `textops` and `cookie`.
        let e = unsafe { WsemulDumbEmuldata::init(type_, cookie, ccol, crow, defattr) };
        Box::into_raw(Box::new(e))
    };

    // SAFETY: `edp` is the console state (the caller's contract) or the new allocation.
    unsafe { (*edp).cbcookie = cbcookie };

    edp.cast()
}

/// `wsemul_dumb_output`.
///
/// # Safety
///
/// `cookie` came from this emulation's `cnattach` or `attach` and is not detached; no other
/// call on it runs at the same time.
pub unsafe fn wsemul_dumb_output(cookie: *mut c_void, data: &[u8], kernel: bool) -> u32 {
    let _ = kernel;
    // SAFETY: the caller's contract.
    let edp = unsafe { &mut *cookie.cast::<WsemulDumbEmuldata>() };
    let eo = edp.emulops;
    let mut data = data;
    let mut processed: u32 = 0;
    let mut rc: Result<(), Errno> = Ok(());

    if edp.crippled {
        for &c in data {
            wsemul_resume_abort(&mut edp.abortstate);

            if u32::from(c) == ASCII_BEL {
                wsdisplay_emulbell(edp.cbcookie);
            } else {
                rc = wsemulop(&mut edp.abortstate, || eo.putchar(0, 0, u32::from(c), 0));
                if rc.is_err() {
                    break;
                }
            }
            processed += 1;
        }
        if rc.is_err() {
            wsemul_abort_other(&mut edp.abortstate);
        }
        return processed;
    }

    match edp.abortstate.state {
        ABORT_FAILED_CURSOR => {
            // If we could not display the cursor back, we pretended not having been able to
            // display the last character. But this is a lie, so compensate here.
            if let Some((_, rest)) = data.split_first() {
                data = rest;
            }
            processed += 1;
            wsemul_reset_abortstate(&mut edp.abortstate);
        }
        ABORT_OK => {
            // remove cursor image
            let rc = eo.cursor(0, edp.crow as i32, edp.ccol as i32);
            if rc.is_err() {
                return 0;
            }
        }
        _ => {}
    }

    for &byte in data {
        wsemul_resume_abort(&mut edp.abortstate);

        let c = u32::from(byte);
        let mut newline = false;
        match c {
            ASCII_BEL => wsdisplay_emulbell(edp.cbcookie),
            ASCII_BS => {
                if edp.ccol > 0 {
                    edp.ccol -= 1;
                }
            }
            ASCII_CR => edp.ccol = 0,
            ASCII_HT => {
                let n = (8 - (edp.ccol & 7)).min(edp.ncols.wrapping_sub(edp.ccol).wrapping_sub(1));
                let (row, col, attr) = (edp.crow as i32, edp.ccol as i32, edp.defattr);
                rc = wsemulop(&mut edp.abortstate, || {
                    eo.erasecols(row, col, n as i32, attr)
                });
                if rc.is_ok() {
                    edp.ccol += n;
                }
            }
            ASCII_FF => {
                let (nrows, attr) = (edp.nrows as i32, edp.defattr);
                rc = wsemulop(&mut edp.abortstate, || eo.eraserows(0, nrows, attr));
                if rc.is_ok() {
                    edp.ccol = 0;
                    edp.crow = 0;
                }
            }
            ASCII_VT => {
                if edp.crow > 0 {
                    edp.crow -= 1;
                }
            }
            ASCII_LF => newline = true,
            _ => {
                let (row, col, attr) = (edp.crow as i32, edp.ccol as i32, edp.defattr);
                rc = wsemulop(&mut edp.abortstate, || eo.putchar(row, col, c, attr));
                if rc.is_ok() {
                    edp.ccol += 1;
                    // if cur col is still on cur line, done; otherwise wrap the column
                    // around and go on as for a line feed.
                    if edp.ccol >= edp.ncols {
                        edp.ccol = 0;
                        newline = true;
                    }
                }
            }
        }

        if newline {
            // if the cur line isn't the last, incr and leave.
            if edp.crow < edp.nrows.wrapping_sub(1) {
                edp.crow += 1;
            } else {
                let n: u32 = 1; // number of lines to scroll
                let (nrows, attr) = (edp.nrows, edp.defattr);
                rc = wsemulop(&mut edp.abortstate, || {
                    eo.copyrows(n as i32, 0, nrows.wrapping_sub(n) as i32)
                });
                if rc.is_ok() {
                    rc = wsemulop(&mut edp.abortstate, || {
                        eo.eraserows(nrows.wrapping_sub(n) as i32, n as i32, attr)
                    });
                }
                if rc.is_err() {
                    // undo wrap-at-eol processing if necessary
                    if c != ASCII_LF {
                        edp.ccol = edp.ncols.wrapping_sub(1);
                    }
                } else {
                    edp.crow -= n - 1;
                }
            }
        }

        if rc.is_err() {
            break;
        }
        processed += 1;
    }

    if rc.is_err() {
        wsemul_abort_other(&mut edp.abortstate);
    } else {
        // put cursor image back
        rc = eo.cursor(1, edp.crow as i32, edp.ccol as i32);
        if rc.is_err() {
            // Fail the last character output, remembering that only the cursor operation
            // really needs to be done.
            wsemul_abort_cursor(&mut edp.abortstate);
            processed = processed.wrapping_sub(1);
        }
    }

    if rc.is_ok() {
        wsemul_reset_abortstate(&mut edp.abortstate);
    }

    processed
}

/// `wsemul_dumb_translate`: keys send nothing.
///
/// # Safety
///
/// None beyond the table's contract; the cookie is not read.
pub unsafe fn wsemul_dumb_translate(
    cookie: *mut c_void,
    layout: KbdT,
    in_: KeysymT,
    buf: &mut [u8; WSEMUL_TRANSLATE_SIZE],
) -> &[u8] {
    let _ = (cookie, layout, in_);
    &buf[..0]
}

/// `wsemul_dumb_detach`.
///
/// # Safety
///
/// As for `output`; the cookie is not used again unless it is the console's.
pub unsafe fn wsemul_dumb_detach(cookie: *mut c_void, crowp: &mut u32, ccolp: &mut u32) {
    let edp = cookie.cast::<WsemulDumbEmuldata>();
    // SAFETY: the caller's contract.
    let e = unsafe { &*edp };
    *crowp = e.crow;
    *ccolp = e.ccol;
    if !ptr::eq(edp, WSEMUL_DUMB_CONSOLE_EMULDATA.as_ptr()) {
        // SAFETY: a state other than the console's came from `Box::into_raw` in `attach`,
        // and the caller does not use it again.
        drop(unsafe { Box::from_raw(edp) });
    }
}

/// `wsemul_dumb_resetop`.
///
/// # Safety
///
/// As for `output`.
pub unsafe fn wsemul_dumb_resetop(cookie: *mut c_void, op: WsemulResetops) {
    // SAFETY: the caller's contract.
    let edp = unsafe { &mut *cookie.cast::<WsemulDumbEmuldata>() };

    if edp.crippled {
        return;
    }

    match op {
        WSEMUL_CLEARSCREEN => {
            let _ = edp.emulops.eraserows(0, edp.nrows as i32, edp.defattr);
            edp.ccol = 0;
            edp.crow = 0;
            let _ = edp.emulops.cursor(1, 0, 0);
        }
        WSEMUL_CLEARCURSOR => {
            let _ = edp.emulops.cursor(0, edp.crow as i32, edp.ccol as i32);
        }
        _ => {}
    }
}
/* </CODE> */

/* <TESTS> */
#[cfg(test)]
mod tests {
    use super::*;
    use crate::dev::wscons::testutil::{FAKE_EMULOPS, Op, PUTCHAR_ONLY_EMULOPS, Rig, descr};
    use crate::dev::wscons::wsemulvar::ABORT_FAILED_OTHER;
    use std::boxed::Box;

    fn rig(rows: usize, cols: usize) -> Rig {
        Rig::new(&WSEMUL_DUMB_OPS, rows, cols, 0, &FAKE_EMULOPS)
    }

    fn edp(r: &Rig) -> &WsemulDumbEmuldata {
        // SAFETY: the rig's state is a dumb emulation's, not in use.
        unsafe { &*r.edp.cast::<WsemulDumbEmuldata>() }
    }

    #[test]
    fn text_wraps_and_scrolls() {
        let mut r = rig(3, 5);
        assert_eq!(r.out(b"abcdefgh"), 8);
        assert_eq!(
            (r.scr().line(0), r.scr().line(1)),
            ("abcde".into(), "fgh".into())
        );
        assert_eq!(r.scr().cursor, Some((1, 3)));
        // A line feed on the last row scrolls everything up one row.
        r.out(b"ij\r\nkl");
        assert_eq!(r.scr().line(0), "fghij");
        assert_eq!((r.scr().line(1), r.scr().line(2)), ("".into(), "kl".into()));
        assert!(r.scr().log.contains(&Op::Copyrows(1, 0, 2)));
        assert!(r.scr().log.contains(&Op::Eraserows(2, 1, 0)));
        // LF keeps the column.
        r.out(b"\nmn");
        assert_eq!((r.scr().line(0), r.scr().line(1)), ("".into(), "kl".into()));
        assert_eq!(r.scr().line(2), "  mn");
        assert_eq!((edp(&r).crow, edp(&r).ccol), (2, 4));
    }

    #[test]
    fn control_characters() {
        let mut r = rig(4, 20);
        r.out(b"ab\x08c\tX\x0bY\x07");
        assert_eq!(r.scr().line(0), "ac      XY");
        assert_eq!(r.scr().line(1), "");
        assert_eq!(r.scr().bells, 1);
        assert_eq!((edp(&r).crow, edp(&r).ccol), (0, 10));
        assert_eq!(r.scr().cell(0, 9).0, u32::from(b'Y'));
        // A tab erases up to the next stop; the last column stays (n = ncols - ccol - 1).
        r.out(b"\r\x0c");
        assert_eq!(
            (r.scr().line(0), edp(&r).crow, edp(&r).ccol),
            ("".into(), 0, 0)
        );
        r.out(b"\x0812345678901234567\t\t");
        assert_eq!(edp(&r).ccol, 19);
        // Erase-then-move: BS at column 0 stays, VT at row 0 stays.
        r.out(b"\r\x08\x0b");
        assert_eq!((edp(&r).crow, edp(&r).ccol), (0, 0));
    }

    #[test]
    fn a_failed_emulop_is_retried_without_redoing_the_rest() {
        let mut r = rig(2, 4);
        r.out(b"abcd");
        // Wrapping onto row 1 is a move; the next wrap scrolls (copyrows, eraserows).
        r.out(b"efgh");
        let calls = r.scr().calls;
        // Call order for "ij": cursor off, putchar i, putchar j, cursor on. Fail putchar j.
        r.scr_mut().fail_at = Some(calls + 2);
        assert_eq!(r.out(b"ij"), 1);
        assert_eq!(edp(&r).abortstate.state, ABORT_FAILED_OTHER);
        assert_eq!(edp(&r).abortstate.skip, 0);
        // The tty layer sends "j" again; nothing done twice.
        assert_eq!(r.out(b"j"), 1);
        assert_eq!(r.scr().line(1), "ij");
        // A failing scroll: "l" wraps at the end of row 1; fail the eraserows of the scroll.
        r.out(b"k");
        let calls = r.scr().calls;
        // cursor off, putchar l, copyrows, eraserows (fails).
        r.scr_mut().fail_at = Some(calls + 3);
        assert_eq!(r.out(b"l"), 0);
        assert_eq!(edp(&r).ccol, 3); // wrap undone
        assert_eq!(edp(&r).abortstate.skip, 2); // putchar and copyrows done
        assert_eq!(r.out(b"l"), 1);
        assert_eq!(
            (r.scr().line(0), r.scr().line(1)),
            ("ijkl".into(), "".into())
        );
        assert_eq!(
            r.scr()
                .log
                .iter()
                .filter(|o| matches!(o, Op::Copyrows(..)))
                .count(),
            2
        );
    }

    #[test]
    fn a_failed_cursor_reports_the_last_byte_unprocessed() {
        let mut r = rig(2, 8);
        let calls = r.scr().calls;
        // cursor off, putchar x, putchar y, cursor on (fails).
        r.scr_mut().fail_at = Some(calls + 3);
        assert_eq!(r.out(b"xy"), 1);
        assert_eq!(edp(&r).abortstate.state, ABORT_FAILED_CURSOR);
        // The retry of "y" only puts the cursor back.
        assert_eq!(r.out(b"y"), 1);
        assert_eq!(r.scr().line(0), "xy");
        assert_eq!(r.scr().cursor, Some((0, 2)));
    }

    #[test]
    fn a_crippled_display_draws_at_the_origin() {
        let mut r = Rig::new(&WSEMUL_DUMB_OPS, 2, 4, 0, &PUTCHAR_ONLY_EMULOPS);
        assert!(edp(&r).crippled);
        assert_eq!(r.out(b"ab\x07c"), 4);
        assert_eq!(r.scr().line(0), "c");
        assert_eq!(r.scr().bells, 1);
        // SAFETY: the rig's state; a crippled display ignores resets.
        unsafe { wsemul_dumb_resetop(r.edp, WSEMUL_CLEARSCREEN) };
        assert_eq!(r.scr().line(0), "c");
    }

    #[test]
    fn console_translate_detach_and_reset() {
        let mut scr = crate::dev::wscons::testutil::FakeScreen::new(2, 4);
        let cookie: *mut c_void =
            (&mut *scr as *mut crate::dev::wscons::testutil::FakeScreen).cast();
        let d = descr(2, 4, 0, &FAKE_EMULOPS);
        // SAFETY: only this test uses the console state; the screen outlives it.
        unsafe {
            let edp = wsemul_dumb_cnattach(&d, cookie, 1, 1, 7);
            assert_eq!(
                wsemul_dumb_attach(true, None, ptr::null_mut(), 0, 0, cookie, 0),
                edp
            );
            assert_eq!(wsemul_dumb_output(edp, b"z", true), 1);
            let mut buf = [0u8; WSEMUL_TRANSLATE_SIZE];
            assert!(wsemul_dumb_translate(edp, 0, b'a'.into(), &mut buf).is_empty());
            wsemul_dumb_resetop(edp, WSEMUL_CLEARSCREEN);
            let (mut row, mut col) = (9, 9);
            wsemul_dumb_detach(edp, &mut row, &mut col);
            assert_eq!((row, col), (0, 0));
        }
        assert!(scr.log.contains(&Op::Putchar(1, 1, u32::from(b'z'), 7)));
        assert!(
            scr.log
                .ends_with(&[Op::Eraserows(0, 2, 7), Op::Cursor(1, 0, 0)])
        );
        let _: Box<_> = scr;
    }
}
/* </TESTS> */
