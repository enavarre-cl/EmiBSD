/* <LICENSES> */
/*
 * Copyright (c) 2009 Miodrag Vallat.
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
//! `<dev/wscons/wsemulvar.h>`: the interface between `wsdisplay(4)` and its terminal
//! emulations (`vt100`, `dumb`, `sun`).
//!
//! Upstream: sys/dev/wscons/wsemulvar.h @ 3ce1f3f79392
//!
//! An emulation is a [`WsemulOps`] table: `cnattach` and `attach` make its state for a
//! screen (the console's, or a new one) over the screen's emulops and cookie, `output`
//! interprets the bytes written to the screen's tty, `translate` turns a keysym into the
//! bytes the tty reads, `detach` frees the state and `reset` acts on wsdisplay's resets.
//!
//! The tty layer needs a character output to be atomic. Since this may expand to multiple
//! emulops operations, which may fail, each emulation keeps state of its current processing
//! ([`WsemulAbortstate`]), so that if an operation fails, the whole character from the tty
//! layer is reported as not having been output, while it has in fact been partly processed.
//! When the tty layer retransmits the character, this state is used to not retrigger the
//! emulops which have been issued successfully already ([`wsemulop`]). In the particular
//! case where every character was processed but displaying the cursor image back fails, the
//! emulation pretends it could not output the last character; when the tty layer tries
//! again (really to get the cursor image back), that character is skipped
//! (`ABORT_FAILED_CURSOR`).
//!
//! ## Deviations
//! - The `void *` cookies stay `*mut c_void` and the operations are `unsafe fn`s whose
//!   contract is that the cookie came from the same table's `cnattach`/`attach`
//!   (`docs/C_TO_RUST.md`, a table of function pointers over an opaque `void *` the table's
//!   own constructor made).
//! - `output` takes the bytes as a slice; `attach` takes the screen type as an `Option`
//!   (wsdisplay attaches the console screen with a NULL type) and `console` as a `bool`.
//! - `translate` writes into a buffer the caller passes and returns the bytes, borrowed from
//!   that buffer or from a static string, where the C returns a count and a pointer into the
//!   emulation's state (`translatebuf`): a returned slice cannot borrow from behind the raw
//!   cookie.
//! - `struct wsemul_abortstate`'s anonymous `state` enum is [`WsemulAbort`]; the `static
//!   inline` helpers are free functions; the `WSEMULOP(rc, edp, was, rutin, args)` macro is
//!   [`wsemulop`], which takes the operation as a closure.
//! - An emulation's `emulops` pointer and `emulcookie` are one [`BoundEmulops`], whose
//!   methods call the driver. An operation the driver left NULL (the C calls through it, and
//!   every driver that offers a terminal emulation fills them all) does nothing and succeeds;
//!   a NULL `pack_attr` fails with `ENODEV` (the emulations then keep their default
//!   attribute) and a NULL `mapchar` maps every character to itself with quality 0.
//! - `wsemul_pick`, `wsemul_getname`, `wsemul_getchar` and `wsemul_utf8_translate` are
//!   defined by their `.c` files' modules (`wsemulconf`, `wsemul_subr`) and not repeated
//!   here.
//! - The callbacks into the display interface, `wsdisplay_emulbell` and
//!   `wsdisplay_emulinput`, are defined by `wsdisplay.rs` and re-exported here, where the C
//!   declares them.

use core::ffi::c_void;
use core::ptr;

use crate::dev::wscons::wscons_features::HAVE_RESTARTABLE_EMULOPS;
use crate::dev::wscons::wsconsio::WSEMUL_NAME_SIZE;
use crate::dev::wscons::wsdisplayvar::{WsdisplayEmulops, WsscreenDescr};
use crate::dev::wscons::wsksymvar::{KbdT, KeysymT};
use crate::sys::errno::Errno;

pub use crate::dev::wscons::wsdisplay::{wsdisplay_emulbell, wsdisplay_emulinput};

/// The size of the buffer `translate` may fill: the vt100 emulation's `translatebuf`, four
/// bytes with `HAVE_UTF8_SUPPORT` (a UTF-8 sequence), one without.
pub const WSEMUL_TRANSLATE_SIZE: usize = 4;

/// `enum wsemul_resetops`: what `reset` does.
#[repr(i32)]
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[allow(non_camel_case_types)] // OpenBSD names, verbatim, for grep-ability
pub enum WsemulResetops {
    /// `WSEMUL_RESET`: reset the terminal state.
    WSEMUL_RESET = 0,
    /// `WSEMUL_SYNCFONT`: the font changed; recompute the character tables.
    WSEMUL_SYNCFONT = 1,
    /// `WSEMUL_CLEARSCREEN`: clear the screen and home the cursor.
    WSEMUL_CLEARSCREEN = 2,
    /// `WSEMUL_CLEARCURSOR`: remove the cursor image.
    WSEMUL_CLEARCURSOR = 3,
}

pub use WsemulResetops::*;

/// `cnattach`: the emulation state of the console screen of type `type_`, drawn through
/// `cookie` (the screen cookie of the display's `alloc_screen`), with the cursor at
/// (`ccol`, `crow`) and default attribute `defattr`.
pub type WsemulCnattachFn = unsafe fn(
    type_: &WsscreenDescr,
    cookie: *mut c_void,
    ccol: i32,
    crow: i32,
    defattr: u32,
) -> *mut c_void;
/// `attach`: as `cnattach`, for a new screen, or (`console` true) to give the console's
/// state its callback cookie `cbcookie` (then `type_` is `None` and only `cbcookie` is
/// read). NULL when the state cannot be allocated.
pub type WsemulAttachFn = unsafe fn(
    console: bool,
    type_: Option<&WsscreenDescr>,
    cookie: *mut c_void,
    ccol: i32,
    crow: i32,
    cbcookie: *mut c_void,
    defattr: u32,
) -> *mut c_void;
/// `output`: interpret `data` (from the kernel when `kernel`); the number of bytes
/// processed, fewer than `data.len()` when an emulop failed.
pub type WsemulOutputFn = unsafe fn(cookie: *mut c_void, data: &[u8], kernel: bool) -> u32;
/// `translate`: the bytes keysym `in_` (from a keyboard with layout `layout`) sends to the
/// tty, in `buf` or a static string; empty when the keysym sends nothing.
pub type WsemulTranslateFn = for<'a> unsafe fn(
    cookie: *mut c_void,
    layout: KbdT,
    in_: KeysymT,
    buf: &'a mut [u8; WSEMUL_TRANSLATE_SIZE],
) -> &'a [u8];
/// `detach`: free the state; the cursor position in `crowp`/`ccolp`.
pub type WsemulDetachFn = unsafe fn(cookie: *mut c_void, crowp: &mut u32, ccolp: &mut u32);
/// `reset`.
pub type WsemulResetFn = unsafe fn(cookie: *mut c_void, op: WsemulResetops);

/// `struct wsemul_ops`: a terminal emulation.
///
/// Every operation but `cnattach` and `attach` takes the cookie one of those returned:
/// calling it is `unsafe`, with that pairing as the contract, and with the caller
/// serialising the calls on one cookie (wsdisplay calls them under its screen's tty).
#[derive(Clone, Copy)]
pub struct WsemulOps {
    /// `name`.
    pub name: [u8; WSEMUL_NAME_SIZE],
    /// `cnattach`.
    pub cnattach: WsemulCnattachFn,
    /// `attach`.
    pub attach: WsemulAttachFn,
    /// `output`.
    pub output: WsemulOutputFn,
    /// `translate`.
    pub translate: WsemulTranslateFn,
    /// `detach`.
    pub detach: WsemulDetachFn,
    /// `reset`.
    pub reset: WsemulResetFn,
}

impl WsemulOps {
    /// The `name` member's initialiser: `name` NUL-padded (cut at `WSEMUL_NAME_SIZE - 1`).
    pub const fn name_of(name: &[u8]) -> [u8; WSEMUL_NAME_SIZE] {
        let mut n = [0u8; WSEMUL_NAME_SIZE];
        let mut i = 0;
        while i < name.len() && i < WSEMUL_NAME_SIZE - 1 {
            n[i] = name[i];
            i += 1;
        }
        n
    }

    /// The name, up to its NUL.
    pub fn name(&self) -> &[u8] {
        let len = self
            .name
            .iter()
            .position(|&c| c == 0)
            .unwrap_or(self.name.len());
        &self.name[..len]
    }
}

/// `struct wsemul_inputstate`: the state of multi-byte character sequences decoding.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct WsemulInputstate {
    /// `inchar`: character being reconstructed.
    pub inchar: u32,
    /// `lbound`: lower bound of above.
    pub lbound: u32,
    /// `mbleft`: multibyte bytes left until char complete.
    pub mbleft: u32,
    /// `last_output`: last printable character (used by vt100 emul only).
    pub last_output: u32,
}

impl WsemulInputstate {
    /// All zero, the state after a reset.
    pub const ZERO: Self = Self {
        inchar: 0,
        lbound: 0,
        mbleft: 0,
        last_output: 0,
    };
}

/// The anonymous enum of `struct wsemul_abortstate`'s `state`: where the last output
/// stopped.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[allow(non_camel_case_types)] // OpenBSD names, verbatim, for grep-ability
pub enum WsemulAbort {
    /// `ABORT_OK`.
    ABORT_OK,
    /// `ABORT_FAILED_CURSOR`: everything was output but the cursor image.
    ABORT_FAILED_CURSOR,
    /// `ABORT_FAILED_JUMP_SCROLL`: a jump scroll failed.
    ABORT_FAILED_JUMP_SCROLL,
    /// `ABORT_FAILED_OTHER`: another emulop failed.
    ABORT_FAILED_OTHER,
}

pub use WsemulAbort::*;

/// `struct wsemul_abortstate`: emulops failure abort/recovery state.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct WsemulAbortstate {
    /// `state`.
    pub state: WsemulAbort,
    /// `skip`: emulops to skip before reaching resume point.
    pub skip: i32,
    /// `done`: emulops completed.
    pub done: i32,
    /// `lines`: jump scroll lines.
    pub lines: i32,
}

impl WsemulAbortstate {
    /// All clear (what the C's static and `malloc`ed states hold before
    /// [`wsemul_reset_abortstate`]).
    pub const ZERO: Self = Self {
        state: ABORT_OK,
        skip: 0,
        done: 0,
        lines: 0,
    };
}

/// A display's emulops table bound to the screen cookie it is called with: the
/// `emulops`/`emulcookie` pair of an emulation's state.
#[derive(Clone, Copy)]
pub struct BoundEmulops {
    ops: *const WsdisplayEmulops,
    cookie: *mut c_void,
}

impl BoundEmulops {
    /// No table (the state of an emulation before it is attached).
    pub const NULL: Self = Self {
        ops: ptr::null(),
        cookie: ptr::null_mut(),
    };

    /// Binds `ops` (a screen type's `textops`) to `cookie`.
    ///
    /// # Safety
    ///
    /// `ops` is NULL or points to a table that stays valid as long as this value or a copy
    /// of it is used, and `cookie` is the screen cookie the driver pairs with that table
    /// (what its `alloc_screen` returned for this screen type).
    pub unsafe fn new(ops: *const WsdisplayEmulops, cookie: *mut c_void) -> Self {
        Self { ops, cookie }
    }

    /// The table, `None` when NULL.
    pub fn table(&self) -> Option<&WsdisplayEmulops> {
        // SAFETY: `new`'s contract: NULL or valid for as long as `self` is used.
        unsafe { self.ops.as_ref() }
    }

    /// The screen cookie.
    pub fn cookie(&self) -> *mut c_void {
        self.cookie
    }

    /// `cursor`: turn the cursor off or on at (`row`, `col`).
    pub fn cursor(&self, on: i32, row: i32, col: i32) -> Result<(), Errno> {
        match self.table().and_then(|t| t.cursor) {
            // SAFETY: `new`'s contract pairs the cookie with the table.
            Some(f) => unsafe { f(self.cookie, on, row, col) },
            None => Ok(()),
        }
    }

    /// `mapchar`: the glyph for `ch` in `*cp`; the quality of the mapping.
    pub fn mapchar(&self, ch: i32, cp: &mut u32) -> i32 {
        match self.table().and_then(|t| t.mapchar) {
            // SAFETY: `new`'s contract pairs the cookie with the table.
            Some(f) => unsafe { f(self.cookie, ch, cp) },
            None => {
                *cp = ch as u32;
                0
            }
        }
    }

    /// `putchar`: draw `uc` with attribute `attr` at (`row`, `col`).
    pub fn putchar(&self, row: i32, col: i32, uc: u32, attr: u32) -> Result<(), Errno> {
        match self.table().and_then(|t| t.putchar) {
            // SAFETY: `new`'s contract pairs the cookie with the table.
            Some(f) => unsafe { f(self.cookie, row, col, uc, attr) },
            None => Ok(()),
        }
    }

    /// `copycols`.
    pub fn copycols(&self, row: i32, srccol: i32, dstcol: i32, ncols: i32) -> Result<(), Errno> {
        match self.table().and_then(|t| t.copycols) {
            // SAFETY: `new`'s contract pairs the cookie with the table.
            Some(f) => unsafe { f(self.cookie, row, srccol, dstcol, ncols) },
            None => Ok(()),
        }
    }

    /// `erasecols`.
    pub fn erasecols(&self, row: i32, startcol: i32, ncols: i32, attr: u32) -> Result<(), Errno> {
        match self.table().and_then(|t| t.erasecols) {
            // SAFETY: `new`'s contract pairs the cookie with the table.
            Some(f) => unsafe { f(self.cookie, row, startcol, ncols, attr) },
            None => Ok(()),
        }
    }

    /// `copyrows`.
    pub fn copyrows(&self, srcrow: i32, dstrow: i32, nrows: i32) -> Result<(), Errno> {
        match self.table().and_then(|t| t.copyrows) {
            // SAFETY: `new`'s contract pairs the cookie with the table.
            Some(f) => unsafe { f(self.cookie, srcrow, dstrow, nrows) },
            None => Ok(()),
        }
    }

    /// `eraserows`.
    pub fn eraserows(&self, row: i32, nrows: i32, attr: u32) -> Result<(), Errno> {
        match self.table().and_then(|t| t.eraserows) {
            // SAFETY: `new`'s contract pairs the cookie with the table.
            Some(f) => unsafe { f(self.cookie, row, nrows, attr) },
            None => Ok(()),
        }
    }

    /// `pack_attr`: the attribute for `fg`/`bg` and `WSATTR_*` `flags`.
    pub fn pack_attr(&self, fg: i32, bg: i32, flags: i32) -> Result<u32, Errno> {
        match self.table().and_then(|t| t.pack_attr) {
            // SAFETY: `new`'s contract pairs the cookie with the table.
            Some(f) => unsafe { f(self.cookie, fg, bg, flags) },
            None => Err(Errno::ENODEV),
        }
    }
}

// SAFETY: the pair is a table pointer and a cookie that the display driver keeps valid for
// the screen's life; the emulation that holds it is used by one thread at a time (wsdisplay
// serialises its calls under the screen's tty, the console's at spltty).
unsafe impl Send for BoundEmulops {}

/// `wsemul_resume_abort`: start character processing, assuming cursor or jump scroll
/// failure condition has been taken care of.
pub fn wsemul_resume_abort(was: &mut WsemulAbortstate) {
    was.state = ABORT_OK;
    was.done = 0;
}

/// `wsemul_abort_cursor`: register a processing failure point: the cursor image.
pub fn wsemul_abort_cursor(was: &mut WsemulAbortstate) {
    was.state = ABORT_FAILED_CURSOR;
}

/// `wsemul_abort_jump_scroll`: a jump scroll of `lines` failed.
pub fn wsemul_abort_jump_scroll(was: &mut WsemulAbortstate, lines: i32) {
    was.state = ABORT_FAILED_JUMP_SCROLL;
    was.skip = was.done;
    was.lines = lines;
}

/// `wsemul_abort_other`: another emulop failed.
pub fn wsemul_abort_other(was: &mut WsemulAbortstate) {
    was.state = ABORT_FAILED_OTHER;
    was.skip = was.done;
}

/// `wsemul_reset_abortstate`: initialize abortstate structure.
pub fn wsemul_reset_abortstate(was: &mut WsemulAbortstate) {
    was.state = ABORT_OK;
    was.skip = 0;
    // The C leaves `done` alone here too.
}

/// `WSEMULOP(rc, edp, was, rutin, args)`: handle failing emulops calls consistently. With
/// `HAVE_RESTARTABLE_EMULOPS`, an emulop the aborted output already issued is skipped (its
/// `skip` count), and each one that succeeds counts as `done`; without it the result of
/// `op` is ignored.
pub fn wsemulop(
    was: &mut WsemulAbortstate,
    op: impl FnOnce() -> Result<(), Errno>,
) -> Result<(), Errno> {
    if HAVE_RESTARTABLE_EMULOPS {
        let rc = if was.skip != 0 {
            was.skip -= 1;
            Ok(())
        } else {
            op()
        };
        if rc.is_ok() {
            was.done += 1;
        }
        rc
    } else {
        let _ = op();
        Ok(())
    }
}
/* </CODE> */

/* <TESTS> */
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn wsemulop_skips_what_was_done_and_counts_successes() {
        let mut was = WsemulAbortstate::ZERO;
        wsemul_reset_abortstate(&mut was);
        wsemul_resume_abort(&mut was);
        assert_eq!(wsemulop(&mut was, || Ok(())), Ok(()));
        assert_eq!(
            wsemulop(&mut was, || Err(Errno::EAGAIN)),
            Err(Errno::EAGAIN)
        );
        wsemul_abort_other(&mut was);
        assert_eq!((was.state, was.skip), (ABORT_FAILED_OTHER, 1));
        // The retry skips the first emulop without calling it, then calls the second.
        wsemul_resume_abort(&mut was);
        let mut called = 0;
        assert_eq!(
            wsemulop(&mut was, || {
                called += 1;
                Ok(())
            }),
            Ok(())
        );
        assert_eq!(
            wsemulop(&mut was, || {
                called += 10;
                Ok(())
            }),
            Ok(())
        );
        assert_eq!((called, was.skip, was.done), (10, 0, 2));
        wsemul_abort_jump_scroll(&mut was, 3);
        assert_eq!(
            (was.state, was.skip, was.lines),
            (ABORT_FAILED_JUMP_SCROLL, 2, 3)
        );
        assert_eq!(WsemulOps::name_of(b"vt100")[..6], *b"vt100\0");
    }
}
/* </TESTS> */
