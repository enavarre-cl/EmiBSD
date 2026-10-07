/* $OpenBSD: wsemul_vt100.c,v 1.49 2026/08/30 06:44:10 miod Exp $ */
/* $NetBSD: wsemul_vt100.c,v 1.13 2000/04/28 21:56:16 mycroft Exp $ */
/* <LICENSES> */
/*
 * Copyright (c) 2007, 2013 Miodrag Vallat.
 *
 * Permission to use, copy, modify, and distribute this software for any
 * purpose with or without fee is hereby granted, provided that the above
 * copyright notice, this permission notice, and the disclaimer below
 * appear in all copies.
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

//! The `vt100` terminal emulation (a VT220/VT320 subset with ANSI colours and UTF-8): the
//! output state machine, attach and detach.
//!
//! Upstream: sys/dev/wscons/wsemul_vt100.c @ 3ce1f3f79392
//!
//! `output` decodes the bytes into characters (`wsemul_getchar`, UTF-8 in `ESC % G` mode)
//! and runs each through the state machine: C0/C1 controls first, then the printable
//! characters in the normal state, then one handler per escape state (`ESC`, `CSI`, the
//! `SCS` designations, `ESC #`, `ESC SP`, strings, `DCS`). Every handler is abortable: when
//! an emulop fails it returns the error without changing the state, so the tty layer's
//! retry of the same byte resumes it (`wsemulvar`'s abort state skips the emulops already
//! done). At the bottom of the scrolling region, a run of newlines is scrolled at once
//! (jump scroll).
//!
//! ## Deviations
//! - The handlers take the state and the `kernel` flag; the input state they work on is
//!   `edp.kstate` for kernel output and `edp.instate` otherwise (the C passes a pointer to
//!   one of them beside `edp`, which Rust cannot alias).
//! - The console state is a `StaticCell`; another screen's state is a `Box` and its tables
//!   `Vec`s, allocated fallibly as the C's `malloc(M_NOWAIT)` (a table that cannot be
//!   allocated is `None`, as the C's NULL); `free` is the drop in `detach`.
//! - `WS_KERNEL_FG`/`BG`/`COLATTR`/`MONOATTR` (kernel options, unset in `GENERIC`) are their
//!   defaults: white on blue, no attribute.
//! - The `CSI` and `DCS` argument terminators read `args[nargs]` when ten arguments were
//!   already collected (`nargs == VT100_EMUL_NARGS`), past the array; here that clamp is
//!   skipped, which is what the C's in-bounds case does for a non-negative argument.
//! - `output` skips the byte of an `ABORT_FAILED_CURSOR` retry only when there is one.
//! - The `DIAGNOSTIC` checks are behind the `diagnostic` feature; `VT100_PRINTUNKNOWN` and
//!   `VT100_PRINTNOTIMPL` (debug options, off) print nothing.
//! - The `HAVE_*` blocks test the constants of `wscons_features`.

use core::ffi::c_void;
use core::ptr;

use alloc::alloc::{Layout, alloc};
use alloc::boxed::Box;
use alloc::vec::Vec;
use libkern::StaticCell;

use crate::dev::wscons::ascii::*;
#[cfg(test)]
use crate::dev::wscons::testutil::wsdisplay_emulbell;
use crate::dev::wscons::wscons_features::{
    HAVE_DOUBLE_WIDTH_HEIGHT, HAVE_JUMP_SCROLL, HAVE_UTF8_SUPPORT,
};
use crate::dev::wscons::wsdisplayvar::{
    WSATTR_WSCOLORS, WSCOL_BLACK, WSCOL_BLUE, WSCOL_WHITE, WSSCREEN_WSCOLORS, WsscreenDescr,
};
use crate::dev::wscons::wsemul_subr::wsemul_getchar;
use crate::dev::wscons::wsemul_vt100_chars::{vt100_initchartables, vt100_setnrc};
use crate::dev::wscons::wsemul_vt100_keys::wsemul_vt100_translate;
use crate::dev::wscons::wsemul_vt100_subr::{
    wsemul_vt100_ed, wsemul_vt100_handle_csi, wsemul_vt100_handle_dcs, wsemul_vt100_scrolldown,
    wsemul_vt100_scrollup,
};
use crate::dev::wscons::wsemul_vt100var::*;
#[cfg(not(test))]
use crate::dev::wscons::wsemulvar::wsdisplay_emulbell;
use crate::dev::wscons::wsemulvar::{
    ABORT_FAILED_CURSOR, ABORT_FAILED_JUMP_SCROLL, ABORT_OK, BoundEmulops, WSEMUL_CLEARCURSOR,
    WSEMUL_CLEARSCREEN, WSEMUL_RESET, WSEMUL_SYNCFONT, WsemulInputstate, WsemulOps, WsemulResetops,
    wsemul_abort_cursor, wsemul_abort_jump_scroll, wsemul_abort_other, wsemul_reset_abortstate,
    wsemul_resume_abort, wsemulop,
};
use crate::kprintf;
use crate::sys::errno::Errno;

/// `VT100_EMUL_STATE_NORMAL`: normal processing.
pub const VT100_EMUL_STATE_NORMAL: u32 = 0;
/// `VT100_EMUL_STATE_ESC`: got ESC.
pub const VT100_EMUL_STATE_ESC: u32 = 1;
/// `VT100_EMUL_STATE_CSI`: got CSI (ESC[).
pub const VT100_EMUL_STATE_CSI: u32 = 2;
/// `VT100_EMUL_STATE_SCS94`: got ESC{()*+}.
pub const VT100_EMUL_STATE_SCS94: u32 = 3;
/// `VT100_EMUL_STATE_SCS94_PERCENT`: got ESC{()*+}%.
pub const VT100_EMUL_STATE_SCS94_PERCENT: u32 = 4;
/// `VT100_EMUL_STATE_SCS96`: got ESC{-./}.
pub const VT100_EMUL_STATE_SCS96: u32 = 5;
/// `VT100_EMUL_STATE_SCS96_PERCENT`: got ESC{-./}%.
pub const VT100_EMUL_STATE_SCS96_PERCENT: u32 = 6;
/// `VT100_EMUL_STATE_ESC_HASH`: got ESC#.
pub const VT100_EMUL_STATE_ESC_HASH: u32 = 7;
/// `VT100_EMUL_STATE_ESC_SPC`: got ESC<SPC>.
pub const VT100_EMUL_STATE_ESC_SPC: u32 = 8;
/// `VT100_EMUL_STATE_STRING`: waiting for ST (ESC\).
pub const VT100_EMUL_STATE_STRING: u32 = 9;
/// `VT100_EMUL_STATE_STRING_ESC`: waiting for ST, got ESC.
pub const VT100_EMUL_STATE_STRING_ESC: u32 = 10;
/// `VT100_EMUL_STATE_DCS`: got DCS (ESC P).
pub const VT100_EMUL_STATE_DCS: u32 = 11;
/// `VT100_EMUL_STATE_DCS_DOLLAR`: got DCS<p>$.
pub const VT100_EMUL_STATE_DCS_DOLLAR: u32 = 12;
/// `VT100_EMUL_STATE_ESC_PERCENT`: got ESC%.
pub const VT100_EMUL_STATE_ESC_PERCENT: u32 = 13;

/// `WS_KERNEL_FG`: the colour of kernel output.
const WS_KERNEL_FG: i32 = WSCOL_WHITE;
/// `WS_KERNEL_BG`.
const WS_KERNEL_BG: i32 = WSCOL_BLUE;
/// `WS_KERNEL_COLATTR`.
const WS_KERNEL_COLATTR: i32 = 0;
/// `WS_KERNEL_MONOATTR`.
const WS_KERNEL_MONOATTR: i32 = 0;

/// `vt100_handler`: the handler of one escape state.
type Vt100Handler = fn(&mut WsemulVt100Emuldata, bool) -> Result<(), Errno>;

/// `wsemul_vt100_ops`.
pub static WSEMUL_VT100_OPS: WsemulOps = WsemulOps {
    name: WsemulOps::name_of(b"vt100"),
    cnattach: wsemul_vt100_cnattach,
    attach: wsemul_vt100_attach,
    output: wsemul_vt100_output,
    translate: wsemul_vt100_translate,
    detach: wsemul_vt100_detach,
    reset: wsemul_vt100_resetop,
};

/// `wsemul_vt100_console_emuldata`: written by `cnattach` while cold, then reached only
/// through the cookie it returned.
static WSEMUL_VT100_CONSOLE_EMULDATA: StaticCell<WsemulVt100Emuldata> =
    StaticCell::new(WsemulVt100Emuldata::ZERO);

/// `vt100_output`: the handlers of the states after `VT100_EMUL_STATE_NORMAL`, in order.
static VT100_OUTPUT: [Vt100Handler; 13] = [
    wsemul_vt100_output_esc,
    wsemul_vt100_output_csi,
    wsemul_vt100_output_scs94,
    wsemul_vt100_output_scs94_percent,
    wsemul_vt100_output_scs96,
    wsemul_vt100_output_scs96_percent,
    wsemul_vt100_output_esc_hash,
    wsemul_vt100_output_esc_spc,
    wsemul_vt100_output_string,
    wsemul_vt100_output_string_esc,
    wsemul_vt100_output_dcs,
    wsemul_vt100_output_dcs_dollar,
    wsemul_vt100_output_esc_percent,
];

/// `malloc(sizeof *edp, M_DEVBUF, M_NOWAIT)` of a state: `None` when there is no memory.
fn malloc_emuldata(value: WsemulVt100Emuldata) -> Option<Box<WsemulVt100Emuldata>> {
    let layout = Layout::new::<WsemulVt100Emuldata>();
    // SAFETY: the state is not zero-sized, as `alloc` requires.
    let p = unsafe { alloc(layout) }.cast::<WsemulVt100Emuldata>();
    if p.is_null() {
        return None;
    }
    // SAFETY: `p` is non-null and was allocated by the global allocator for this layout, so
    // it is aligned and valid for a write; nothing else refers to it. The written value
    // initialises it, which is what `Box::from_raw` needs to own and later free it.
    unsafe {
        p.write(value);
        Some(Box::from_raw(p))
    }
}

/// `malloc(n * sizeof(T), M_DEVBUF, M_NOWAIT)` of a table, filled with `fill`; `None` when
/// there is no memory.
fn malloc_table<T: Copy>(n: usize, fill: T) -> Option<Vec<T>> {
    let mut v = Vec::new();
    v.try_reserve_exact(n).ok()?;
    v.resize(n, fill);
    Some(v)
}

/// `wsemul_vt100_init`: the members `cnattach` and `attach` share.
///
/// # Safety
///
/// As [`BoundEmulops::new`] for `type_.textops` and `cookie`.
unsafe fn wsemul_vt100_init(
    edp: &mut WsemulVt100Emuldata,
    type_: &WsscreenDescr,
    cookie: *mut c_void,
    ccol: i32,
    crow: i32,
    defattr: u32,
) {
    // SAFETY: the caller's contract.
    edp.emulops = unsafe { BoundEmulops::new(type_.textops, cookie) };
    edp.scrcapabilities = type_.capabilities;
    edp.nrows = type_.nrows as u32;
    edp.ncols = type_.ncols as u32;
    edp.crow = crow as u32;
    edp.ccol = ccol as u32;
    edp.defattr = defattr;
    wsemul_reset_abortstate(&mut edp.abortstate);
}

/// `wsemul_vt100_cnattach`.
///
/// # Safety
///
/// `type_.textops` is NULL or a table valid for the console's life, paired with `cookie`;
/// called while cold, before any other use of the console state.
pub unsafe fn wsemul_vt100_cnattach(
    type_: &WsscreenDescr,
    cookie: *mut c_void,
    ccol: i32,
    crow: i32,
    defattr: u32,
) -> *mut c_void {
    // SAFETY: the caller's contract (one CPU, cold, nothing else holds the state).
    let edp = unsafe { WSEMUL_VT100_CONSOLE_EMULDATA.get_mut() };
    // SAFETY: the caller's contract on `textops` and `cookie`.
    unsafe { wsemul_vt100_init(edp, type_, cookie, ccol, crow, defattr) };
    edp.console = true;
    edp.cbcookie = ptr::null_mut();

    let res = if type_.capabilities & WSSCREEN_WSCOLORS != 0 {
        edp.emulops.pack_attr(
            WS_KERNEL_FG,
            WS_KERNEL_BG,
            WS_KERNEL_COLATTR | WSATTR_WSCOLORS,
        )
    } else {
        edp.emulops.pack_attr(0, 0, WS_KERNEL_MONOATTR)
    };
    edp.kernattr = res.unwrap_or(defattr);

    edp.tabs = None;
    if HAVE_DOUBLE_WIDTH_HEIGHT {
        edp.dblwid = None;
        edp.dw = 0;
    }
    edp.dcsarg = None;
    edp.isolatin1tab = None;
    edp.decgraphtab = None;
    edp.dectechtab = None;
    edp.nrctab = None;
    wsemul_vt100_reset(edp);
    WSEMUL_VT100_CONSOLE_EMULDATA.as_ptr().cast()
}

/// `wsemul_vt100_attach`: NULL when the state cannot be allocated.
///
/// # Safety
///
/// For the console, `cnattach` ran and no other reference to its state is live; otherwise
/// `type_.textops` is NULL or a table valid for the screen's life, paired with `cookie`.
pub unsafe fn wsemul_vt100_attach(
    console: bool,
    type_: Option<&WsscreenDescr>,
    cookie: *mut c_void,
    ccol: i32,
    crow: i32,
    cbcookie: *mut c_void,
    defattr: u32,
) -> *mut c_void {
    let edp: *mut WsemulVt100Emuldata = if console {
        let edp = WSEMUL_VT100_CONSOLE_EMULDATA.as_ptr();
        // SAFETY: the caller's contract: no other reference to the console state.
        crate::kassert!(unsafe { (*edp).console });
        edp
    } else {
        let Some(type_) = type_ else {
            return ptr::null_mut();
        };
        let Some(mut e) = malloc_emuldata(WsemulVt100Emuldata::ZERO) else {
            return ptr::null_mut();
        };
        // SAFETY: the caller's contract on `textops` and `cookie`.
        unsafe { wsemul_vt100_init(&mut e, type_, cookie, ccol, crow, defattr) };
        e.console = false;
        Box::into_raw(e)
    };
    // SAFETY: the console state (the caller's contract) or the new allocation, which
    // nothing else references.
    let e = unsafe { &mut *edp };
    e.cbcookie = cbcookie;

    e.tabs = malloc_table(e.ncols as usize, 0u8);
    if HAVE_DOUBLE_WIDTH_HEIGHT {
        e.dblwid = malloc_table(e.nrows as usize, 0u8);
        e.dw = 0;
    }
    e.dcsarg = malloc_table(DCS_MAXLEN, 0u8);
    e.isolatin1tab = malloc_table(128, 0u32);
    e.decgraphtab = malloc_table(128, 0u32);
    e.dectechtab = malloc_table(128, 0u32);
    e.nrctab = malloc_table(128, 0u32);
    vt100_initchartables(e);
    wsemul_vt100_reset(e);
    edp.cast()
}

/// `wsemul_vt100_detach`.
///
/// # Safety
///
/// `cookie` came from this emulation's `cnattach` or `attach`, nothing else uses it during
/// the call, and it is not used again unless it is the console's.
pub unsafe fn wsemul_vt100_detach(cookie: *mut c_void, crowp: &mut u32, ccolp: &mut u32) {
    let edp = cookie.cast::<WsemulVt100Emuldata>();
    // SAFETY: the caller's contract.
    let e = unsafe { &mut *edp };

    *crowp = e.crow;
    *ccolp = e.ccol;
    e.tabs = None;
    if HAVE_DOUBLE_WIDTH_HEIGHT {
        e.dblwid = None;
    }
    e.dcsarg = None;
    e.isolatin1tab = None;
    e.decgraphtab = None;
    e.dectechtab = None;
    e.nrctab = None;
    if !ptr::eq(edp, WSEMUL_VT100_CONSOLE_EMULDATA.as_ptr()) {
        // SAFETY: a state other than the console's came from `Box::into_raw` in `attach`,
        // and the caller does not use it again.
        drop(unsafe { Box::from_raw(edp) });
    }
}

/// `wsemul_vt100_resetop`.
///
/// # Safety
///
/// `cookie` came from this emulation's `cnattach` or `attach` and is not detached; no other
/// call on it runs at the same time.
pub unsafe fn wsemul_vt100_resetop(cookie: *mut c_void, op: WsemulResetops) {
    // SAFETY: the caller's contract.
    let edp = unsafe { &mut *cookie.cast::<WsemulVt100Emuldata>() };

    match op {
        WSEMUL_RESET => wsemul_vt100_reset(edp),
        WSEMUL_SYNCFONT => vt100_initchartables(edp),
        WSEMUL_CLEARSCREEN => {
            let _ = wsemul_vt100_ed(edp, 2);
            edp.ccol = 0;
            edp.crow = 0;
            let _ = edp.emulops.cursor(edp.flags & VTFL_CURSORON, 0, 0);
        }
        WSEMUL_CLEARCURSOR => {
            let _ = edp.emulops.cursor(0, edp.crow as i32, edp.ccol as i32);
        }
    }
}

/// `wsemul_vt100_reset`: the terminal state of a hard reset.
pub fn wsemul_vt100_reset(edp: &mut WsemulVt100Emuldata) {
    edp.state = VT100_EMUL_STATE_NORMAL;
    edp.flags = VTFL_DECAWM | VTFL_CURSORON;
    edp.curattr = edp.defattr;
    edp.bkgdattr = edp.defattr;
    edp.attrflags = 0;
    edp.fgcol = WSCOL_WHITE;
    edp.bgcol = WSCOL_BLACK;
    edp.scrreg_startrow = 0;
    edp.scrreg_nrows = edp.nrows;
    if let Some(tabs) = &mut edp.tabs {
        tabs.fill(0);
        for i in (8..edp.ncols as usize).step_by(8) {
            tabs[i] = 1;
        }
    }
    edp.dcspos = 0;
    edp.dcstype = 0;
    edp.chartab_g[0] = None;
    edp.chartab_g[1] = Some(Vt100Chartab::Nrc); // ???
    edp.chartab_g[2] = Some(Vt100Chartab::Isolatin1);
    edp.chartab_g[3] = Some(Vt100Chartab::Isolatin1);
    edp.chartab0 = 0;
    edp.chartab1 = 2;
    edp.sschartab = 0;
    edp.instate = WsemulInputstate::ZERO;
    edp.kstate = WsemulInputstate::ZERO;
}

/// `wsemul_vt100_nextline`: move the cursor to the next line if possible. If the cursor is
/// at the bottom of the scroll area, then scroll it up. If the cursor is at the bottom of
/// the screen then don't move it down.
pub fn wsemul_vt100_nextline(edp: &mut WsemulVt100Emuldata) -> Result<(), Errno> {
    if edp.rows_below() == 0 {
        // Bottom of the scroll region.
        wsemul_vt100_scrollup(edp, 1)
    } else {
        if edp.crow + 1 < edp.nrows {
            // Cursor not at the bottom of the screen.
            edp.crow += 1;
        }
        edp.check_dw();
        Ok(())
    }
}

/// `wsemul_vt100_output_normal`: draw the current character `count` times (more than once
/// for `REP`), through the character set it is invoked from.
pub fn wsemul_vt100_output_normal(
    edp: &mut WsemulVt100Emuldata,
    kernel: bool,
    count: i32,
) -> Result<(), Errno> {
    let oldsschartab = edp.sschartab;
    let inchar = edp.instate(kernel).inchar;
    let eo = edp.emulops;
    let mut dc: u32 = 0;

    if HAVE_UTF8_SUPPORT && edp.flags & VTFL_UTF8 != 0 {
        eo.mapchar(inchar as i32, &mut dc);
    } else {
        let mut c = (inchar & 0xff) as u8;
        let ct = if c & 0x80 != 0 {
            c &= 0x7f;
            edp.chartab_g[edp.chartab1 as usize]
        } else if edp.sschartab != 0 {
            let ct = edp.chartab_g[edp.sschartab as usize];
            edp.sschartab = 0;
            ct
        } else {
            edp.chartab_g[edp.chartab0 as usize]
        };
        dc = match edp.chartab(ct) {
            Some(t) => t[usize::from(c)],
            None => u32::from(c),
        };
    }

    let mut rc = Ok(());
    for _ in 0..count {
        if edp.flags & (VTFL_LASTCHAR | VTFL_DECAWM) == (VTFL_LASTCHAR | VTFL_DECAWM) {
            wsemul_vt100_nextline(edp)?;
            edp.ccol = 0;
            edp.flags &= !VTFL_LASTCHAR;
        }

        if edp.flags & VTFL_INSERTMODE != 0 && edp.cols_left() != 0 {
            let (row, f, t, n) = edp.copycols_args(edp.ccol, edp.ccol + 1, edp.cols_left());
            rc = wsemulop(&mut edp.abortstate, || eo.copycols(row, f, t, n));
            if rc.is_err() {
                break;
            }
        }

        let col = if HAVE_DOUBLE_WIDTH_HEIGHT {
            edp.ccol << edp.dw
        } else {
            edp.ccol
        };
        let (row, attr) = (
            edp.crow as i32,
            if kernel { edp.kernattr } else { edp.curattr },
        );
        rc = wsemulop(&mut edp.abortstate, || {
            eo.putchar(row, col as i32, dc, attr)
        });
        if rc.is_err() {
            break;
        }

        if edp.cols_left() != 0 {
            edp.ccol += 1;
        } else {
            edp.flags |= VTFL_LASTCHAR;
        }
    }

    if rc.is_err() {
        // undo potential sschartab update
        edp.sschartab = oldsschartab;
    }
    rc
}

/// `wsemul_vt100_output_c0c1`: a C0 or C1 control character.
fn wsemul_vt100_output_c0c1(edp: &mut WsemulVt100Emuldata, kernel: bool) -> Result<(), Errno> {
    let mut rc = Ok(());

    match edp.instate(kernel).inchar {
        ASCII_BEL => {
            if edp.state == VT100_EMUL_STATE_STRING {
                // acts as an equivalent to the ``ESC \'' string end
                wsemul_vt100_handle_dcs(edp);
                edp.state = VT100_EMUL_STATE_NORMAL;
            } else {
                wsdisplay_emulbell(edp.cbcookie);
            }
        }
        ASCII_BS => {
            if edp.ccol > 0 {
                edp.ccol -= 1;
                edp.flags &= !VTFL_LASTCHAR;
            }
        }
        ASCII_CR => edp.ccol = 0,
        ASCII_HT => {
            let ncols = edp.ncols_dw();
            if let Some(tabs) = &edp.tabs {
                if edp.cols_left() != 0 {
                    let mut n = edp.ccol + 1;
                    while n < ncols.wrapping_sub(1) && tabs[n as usize] == 0 {
                        n += 1;
                    }
                    edp.ccol = n;
                }
            } else {
                edp.ccol += (8 - (edp.ccol & 7)).min(edp.cols_left());
            }
        }
        ASCII_SO => {
            // LS1
            edp.flags &= !VTFL_UTF8;
            edp.chartab0 = 1;
        }
        ASCII_SI => {
            // LS0
            edp.flags &= !VTFL_UTF8;
            edp.chartab0 = 0;
        }
        ASCII_ESC => {
            if kernel {
                kprintf!("wsemul_vt100_output_c0c1: ESC in kernel output ignored\n");
            } else if edp.state == VT100_EMUL_STATE_STRING {
                // might be a string end
                edp.state = VT100_EMUL_STATE_STRING_ESC;
            } else {
                // XXX cancel current escape sequence
                edp.state = VT100_EMUL_STATE_ESC;
            }
        }
        ASCII_CAN | ASCII_SUB => {
            // cancel current escape sequence
            edp.state = VT100_EMUL_STATE_NORMAL;
        }
        ASCII_LF | ASCII_VT | ASCII_FF => rc = wsemul_vt100_nextline(edp),
        _ => {
            // ASCII_NUL and the rest: ignore
        }
    }

    if edp.cols_left() != 0 {
        edp.flags &= !VTFL_LASTCHAR;
    }

    rc
}

/// `wsemul_vt100_output_esc`: the character after `ESC`.
fn wsemul_vt100_output_esc(edp: &mut WsemulVt100Emuldata, kernel: bool) -> Result<(), Errno> {
    let mut newstate = VT100_EMUL_STATE_NORMAL;
    let mut rc = Ok(());
    let inchar = edp.instate(kernel).inchar;

    match char::from_u32(inchar).unwrap_or('\0') {
        '[' => {
            // CSI
            edp.nargs = 0;
            edp.args = [0; VT100_EMUL_NARGS];
            edp.modif1 = 0;
            edp.modif2 = 0;
            newstate = VT100_EMUL_STATE_CSI;
        }
        '7' => {
            // DECSC
            edp.flags |= VTFL_SAVEDCURS;
            edp.savedcursor_row = edp.crow;
            edp.savedcursor_col = edp.ccol;
            edp.savedattr = edp.curattr;
            edp.savedbkgdattr = edp.bkgdattr;
            edp.savedattrflags = edp.attrflags;
            edp.savedfgcol = edp.fgcol;
            edp.savedbgcol = edp.bgcol;
            edp.savedchartab_g = edp.chartab_g;
            edp.savedchartab0 = edp.chartab0;
            edp.savedchartab1 = edp.chartab1;
        }
        '8' => {
            // DECRC
            if edp.flags & VTFL_SAVEDCURS != 0 {
                edp.crow = edp.savedcursor_row;
                edp.ccol = edp.savedcursor_col;
                edp.curattr = edp.savedattr;
                edp.bkgdattr = edp.savedbkgdattr;
                edp.attrflags = edp.savedattrflags;
                edp.fgcol = edp.savedfgcol;
                edp.bgcol = edp.savedbgcol;
                edp.chartab_g = edp.savedchartab_g;
                edp.chartab0 = edp.savedchartab0;
                edp.chartab1 = edp.savedchartab1;
            }
        }
        '=' => edp.flags |= VTFL_APPLKEYPAD, // DECKPAM application mode
        '>' => edp.flags &= !VTFL_APPLKEYPAD, // DECKPNM numeric mode
        'E' => {
            // NEL
            edp.ccol = 0;
            rc = wsemul_vt100_nextline(edp);
        }
        'D' => rc = wsemul_vt100_nextline(edp), // IND
        'H' => {
            // HTS
            let ccol = edp.ccol as usize;
            if let Some(tabs) = &mut edp.tabs {
                tabs[ccol] = 1;
            }
        }
        '~' => {
            // LS1R
            edp.flags &= !VTFL_UTF8;
            edp.chartab1 = 1;
        }
        'n' => {
            // LS2
            edp.flags &= !VTFL_UTF8;
            edp.chartab0 = 2;
        }
        '}' => {
            // LS2R
            edp.flags &= !VTFL_UTF8;
            edp.chartab1 = 2;
        }
        'o' => {
            // LS3
            edp.flags &= !VTFL_UTF8;
            edp.chartab0 = 3;
        }
        '|' => {
            // LS3R
            edp.flags &= !VTFL_UTF8;
            edp.chartab1 = 3;
        }
        'N' => {
            // SS2
            edp.flags &= !VTFL_UTF8;
            edp.sschartab = 2;
        }
        'O' => {
            // SS3
            edp.flags &= !VTFL_UTF8;
            edp.sschartab = 3;
        }
        'M' => {
            // RI
            let i = edp.rows_above();
            if i > 0 {
                if edp.crow > 0 {
                    edp.crow -= 1;
                }
                edp.check_dw();
            } else if i == 0 {
                // Top of scroll region.
                rc = wsemul_vt100_scrolldown(edp, 1);
            }
        }
        'P' => {
            // DCS
            edp.nargs = 0;
            edp.args = [0; VT100_EMUL_NARGS];
            newstate = VT100_EMUL_STATE_DCS;
        }
        'c' => {
            // RIS
            wsemul_vt100_reset(edp);
            rc = wsemul_vt100_ed(edp, 2);
            if rc.is_ok() {
                edp.ccol = 0;
                edp.crow = 0;
            }
        }
        '(' | ')' | '*' | '+' => {
            // SCS
            edp.designating = (inchar - u32::from(b'(')) as i32;
            newstate = VT100_EMUL_STATE_SCS94;
        }
        '-' | '.' | '/' => {
            // SCS
            edp.designating = (inchar - u32::from(b'-') + 1) as i32;
            newstate = VT100_EMUL_STATE_SCS96;
        }
        '#' => newstate = VT100_EMUL_STATE_ESC_HASH,
        ' ' => newstate = VT100_EMUL_STATE_ESC_SPC, // 7/8 bit
        ']' | '^' | '_' => {
            // OSC operating system command, PM privacy message, APC application program
            // command: ignored
            newstate = VT100_EMUL_STATE_STRING;
        }
        '<' => {}                                       // exit VT52 mode - ignored
        '%' => newstate = VT100_EMUL_STATE_ESC_PERCENT, // UTF-8 encoding sequences
        _ => {}
    }

    if edp.cols_left() != 0 {
        edp.flags &= !VTFL_LASTCHAR;
    }

    rc?;

    edp.state = newstate;
    Ok(())
}

/// `wsemul_vt100_output_scs94`: designate a 94-character set.
fn wsemul_vt100_output_scs94(edp: &mut WsemulVt100Emuldata, kernel: bool) -> Result<(), Errno> {
    let mut newstate = VT100_EMUL_STATE_NORMAL;
    let g = edp.designating as usize;

    match char::from_u32(edp.instate(kernel).inchar).unwrap_or('\0') {
        '%' => newstate = VT100_EMUL_STATE_SCS94_PERCENT, // probably DEC supplemental graphic
        'A' => {
            // british / national
            edp.flags &= !VTFL_UTF8;
            edp.chartab_g[g] = Some(Vt100Chartab::Nrc);
        }
        'B' => {
            // ASCII
            edp.flags &= !VTFL_UTF8;
            edp.chartab_g[g] = None;
        }
        '<' => {
            // user preferred supplemental
            // XXX not really "user" preferred
            edp.flags &= !VTFL_UTF8;
            edp.chartab_g[g] = Some(Vt100Chartab::Isolatin1);
        }
        '0' => {
            // DEC special graphic
            edp.flags &= !VTFL_UTF8;
            edp.chartab_g[g] = Some(Vt100Chartab::Decgraph);
        }
        '>' => {
            // DEC tech
            edp.flags &= !VTFL_UTF8;
            edp.chartab_g[g] = Some(Vt100Chartab::Dectech);
        }
        _ => {}
    }

    edp.state = newstate;
    Ok(())
}

/// `wsemul_vt100_output_scs94_percent`.
fn wsemul_vt100_output_scs94_percent(
    edp: &mut WsemulVt100Emuldata,
    kernel: bool,
) -> Result<(), Errno> {
    if edp.instate(kernel).inchar == u32::from(b'5') {
        // DEC supplemental graphic
        // XXX there are differences
        edp.flags &= !VTFL_UTF8;
        edp.chartab_g[edp.designating as usize] = Some(Vt100Chartab::Isolatin1);
    }

    edp.state = VT100_EMUL_STATE_NORMAL;
    Ok(())
}

/// `wsemul_vt100_output_scs96`: designate a 96-character set, or a national replacement
/// set.
fn wsemul_vt100_output_scs96(edp: &mut WsemulVt100Emuldata, kernel: bool) -> Result<(), Errno> {
    let mut newstate = VT100_EMUL_STATE_NORMAL;

    let nrc = match char::from_u32(edp.instate(kernel).inchar).unwrap_or('\0') {
        '%' => {
            // probably portuguese
            newstate = VT100_EMUL_STATE_SCS96_PERCENT;
            None
        }
        'A' => {
            // ISO-latin-1 supplemental
            edp.flags &= !VTFL_UTF8;
            edp.chartab_g[edp.designating as usize] = Some(Vt100Chartab::Isolatin1);
            None
        }
        '4' => Some(1),        // dutch
        '5' | 'C' => Some(2),  // finnish
        'R' => Some(3),        // french
        'Q' => Some(4),        // french canadian
        'K' => Some(5),        // german
        'Y' => Some(6),        // italian
        'E' | '6' => Some(7),  // norwegian / danish
        'Z' => Some(9),        // spanish
        '7' | 'H' => Some(10), // swedish
        '=' => Some(11),       // swiss
        _ => None,
    };
    if let Some(nrc) = nrc {
        // what table ??? (an unknown one is ignored, as the default case does)
        let _ = vt100_setnrc(edp, nrc);
    }

    edp.state = newstate;
    Ok(())
}

/// `wsemul_vt100_output_scs96_percent`.
fn wsemul_vt100_output_scs96_percent(
    edp: &mut WsemulVt100Emuldata,
    kernel: bool,
) -> Result<(), Errno> {
    if edp.instate(kernel).inchar == u32::from(b'6') {
        // portuguese
        let _ = vt100_setnrc(edp, 8);
    }

    edp.state = VT100_EMUL_STATE_NORMAL;
    Ok(())
}

/// `wsemul_vt100_output_esc_spc`: 7-bit (`F`) or 8-bit (`G`) controls, ignored.
fn wsemul_vt100_output_esc_spc(edp: &mut WsemulVt100Emuldata, kernel: bool) -> Result<(), Errno> {
    let _ = kernel;
    edp.state = VT100_EMUL_STATE_NORMAL;
    Ok(())
}

/// `wsemul_vt100_output_string`: a character of a string (OSC, PM, APC, DCS); kept in
/// `dcsarg` when a DCS is to be handled.
fn wsemul_vt100_output_string(edp: &mut WsemulVt100Emuldata, kernel: bool) -> Result<(), Errno> {
    let inchar = edp.instate(kernel).inchar;
    if edp.dcstype != 0
        && (edp.dcspos as usize) < DCS_MAXLEN
        && let Some(dcsarg) = &mut edp.dcsarg
        && inchar & !0xff == 0
    {
        dcsarg[edp.dcspos as usize] = inchar as u8;
        edp.dcspos += 1;
    }

    edp.state = VT100_EMUL_STATE_STRING;
    Ok(())
}

/// `wsemul_vt100_output_string_esc`: `ESC` in a string; `ESC \` (ST) ends it.
fn wsemul_vt100_output_string_esc(
    edp: &mut WsemulVt100Emuldata,
    kernel: bool,
) -> Result<(), Errno> {
    if edp.instate(kernel).inchar == u32::from(b'\\') {
        // ST complete
        wsemul_vt100_handle_dcs(edp);
        edp.state = VT100_EMUL_STATE_NORMAL;
    } else {
        edp.state = VT100_EMUL_STATE_STRING;
    }

    Ok(())
}

/// Collects one more digit `d` of the current argument, clamped (`-1`) once it would reach
/// `VT100_EMUL_ARG_CLAMP` (the CSI and DCS argument digit case).
fn vt100_arg_digit(edp: &mut WsemulVt100Emuldata, d: u32) {
    let n = edp.nargs as usize;
    if n >= VT100_EMUL_NARGS {
        return;
    }
    // Do not allow values to grow too large
    if edp.args[n] >= 0 && edp.args[n] < VT100_EMUL_ARG_CLAMP / 10 {
        edp.args[n] = edp.args[n] * 10 + d as i32;
    } else {
        edp.args[n] = -1; // clamped
    }
}

/// Ends the current argument: applies the clamp and counts it (the CSI and DCS argument
/// terminator case).
fn vt100_arg_end(edp: &mut WsemulVt100Emuldata) {
    let n = edp.nargs as usize;
    // apply clamp
    if n < VT100_EMUL_NARGS && edp.args[n] < 0 {
        edp.args[n] = VT100_EMUL_ARG_CLAMP;
    }
    if n < VT100_EMUL_NARGS {
        edp.nargs += 1;
    }
}

/// `wsemul_vt100_output_dcs`: the parameters and final character of a DCS.
fn wsemul_vt100_output_dcs(edp: &mut WsemulVt100Emuldata, kernel: bool) -> Result<(), Errno> {
    let mut newstate = VT100_EMUL_STATE_DCS;
    let inchar = edp.instate(kernel).inchar;

    match char::from_u32(inchar).unwrap_or('\0') {
        '0'..='9' => vt100_arg_digit(edp, inchar - u32::from(b'0')), // argument digit
        ';' => vt100_arg_end(edp),                                   // argument terminator
        c => {
            vt100_arg_end(edp);
            newstate = VT100_EMUL_STATE_STRING;
            match c {
                '$' => newstate = VT100_EMUL_STATE_DCS_DOLLAR,
                // DECDLD soft charset; DECRQUPSS user preferred supplemental set ('u'
                // must follow - need another state); DECUDK program F6..F20: ignored
                '{' | '!' | '|' => {}
                _ => {}
            }
        }
    }

    edp.state = newstate;
    Ok(())
}

/// `wsemul_vt100_output_dcs_dollar`: `DCS ... $`.
fn wsemul_vt100_output_dcs_dollar(
    edp: &mut WsemulVt100Emuldata,
    kernel: bool,
) -> Result<(), Errno> {
    match char::from_u32(edp.instate(kernel).inchar).unwrap_or('\0') {
        // DECRSTS terminal state restore, DECRQSS control function request: ignored
        'p' | 'q' => {}
        't' => {
            // DECRSPS restore presentation state
            match edp.arg(0) {
                0 => {} // error
                1 => {} // cursor information restore: ignored
                2 => {
                    // tab stop restore
                    edp.dcspos = 0;
                    edp.dcstype = DCSTYPE_TABRESTORE;
                }
                _ => {}
            }
        }
        _ => {}
    }

    edp.state = VT100_EMUL_STATE_STRING;
    Ok(())
}

/// `wsemul_vt100_output_esc_percent`: `ESC % G` enters UTF-8 mode, `ESC % @` leaves it.
fn wsemul_vt100_output_esc_percent(
    edp: &mut WsemulVt100Emuldata,
    kernel: bool,
) -> Result<(), Errno> {
    if HAVE_UTF8_SUPPORT {
        match char::from_u32(edp.instate(kernel).inchar).unwrap_or('\0') {
            'G' => {
                edp.flags |= VTFL_UTF8;
                edp.instate.mbleft = 0;
                edp.kstate.mbleft = 0;
            }
            '@' => edp.flags &= !VTFL_UTF8,
            _ => {}
        }
    }
    edp.state = VT100_EMUL_STATE_NORMAL;
    Ok(())
}

/// `wsemul_vt100_output_esc_hash`: `ESC #`: line width and height, and the alignment
/// pattern.
fn wsemul_vt100_output_esc_hash(edp: &mut WsemulVt100Emuldata, kernel: bool) -> Result<(), Errno> {
    let eo = edp.emulops;

    match char::from_u32(edp.instate(kernel).inchar).unwrap_or('\0') {
        '5' => {
            // DECSWL single width, single height
            if HAVE_DOUBLE_WIDTH_HEIGHT && edp.dblwid.is_some() && edp.dw != 0 {
                let (row, half) = (edp.crow as i32, edp.ncols / 2);
                for i in 0..half as i32 {
                    wsemulop(&mut edp.abortstate, || eo.copycols(row, 2 * i, i, 1))?;
                }
                let (n, attr) = (edp.ncols.wrapping_sub(half) as i32, edp.bkgdattr);
                wsemulop(&mut edp.abortstate, || {
                    eo.erasecols(row, half as i32, n, attr)
                })?;
                let crow = edp.crow as usize;
                if let Some(d) = &mut edp.dblwid {
                    d[crow] = 0;
                }
                edp.dw = 0;
            }
        }
        '6' | '3' | '4' => {
            // DECDWL double width, single height; DECDHL double width, double height, top
            // half; DECDHL double width, double height, bottom half
            if HAVE_DOUBLE_WIDTH_HEIGHT && edp.dblwid.is_some() && edp.dw == 0 {
                let (row, half, attr) = (edp.crow as i32, edp.ncols / 2, edp.bkgdattr);
                for i in (0..half as i32).rev() {
                    wsemulop(&mut edp.abortstate, || eo.copycols(row, i, 2 * i, 1))?;
                }
                for i in 0..half as i32 {
                    wsemulop(&mut edp.abortstate, || {
                        eo.erasecols(row, 2 * i + 1, 1, attr)
                    })?;
                }
                let crow = edp.crow as usize;
                if let Some(d) = &mut edp.dblwid {
                    d[crow] = 1;
                }
                edp.dw = 1;
                let last = (edp.ncols >> 1).wrapping_sub(1);
                if edp.ccol > last {
                    edp.ccol = last;
                }
            }
        }
        '8' => {
            // DECALN
            let attr = edp.curattr;
            for i in 0..edp.nrows as i32 {
                for j in 0..edp.ncols as i32 {
                    wsemulop(&mut edp.abortstate, || {
                        eo.putchar(i, j, u32::from(b'E'), attr)
                    })?;
                }
            }
            edp.ccol = 0;
            edp.crow = 0;
        }
        _ => {}
    }

    if edp.cols_left() != 0 {
        edp.flags &= !VTFL_LASTCHAR;
    }

    edp.state = VT100_EMUL_STATE_NORMAL;
    Ok(())
}

/// `wsemul_vt100_output_csi`: the parameters, modifiers and final character of a CSI.
fn wsemul_vt100_output_csi(edp: &mut WsemulVt100Emuldata, kernel: bool) -> Result<(), Errno> {
    let mut newstate = VT100_EMUL_STATE_CSI;
    let inchar = edp.instate(kernel).inchar;

    match char::from_u32(inchar).unwrap_or('\0') {
        '0'..='9' => vt100_arg_digit(edp, inchar - u32::from(b'0')), // argument digit
        ';' => vt100_arg_end(edp),                                   // argument terminator
        '?' | '>' => edp.modif1 = inchar as u8,                      // DEC specific, DA query
        '!' | '"' | '$' | '&' => edp.modif2 = inchar as u8,
        _ => {
            // end of escape sequence
            let oargs = edp.nargs;
            vt100_arg_end(edp);
            if let Err(e) = wsemul_vt100_handle_csi(edp, kernel) {
                // undo nargs progress
                edp.nargs = oargs;
                return Err(e);
            }
            newstate = VT100_EMUL_STATE_NORMAL;
        }
    }

    if edp.cols_left() != 0 {
        edp.flags &= !VTFL_LASTCHAR;
    }

    edp.state = newstate;
    Ok(())
}

/// `wsemul_vt100_output`: interpret `data`; the number of bytes processed.
///
/// # Safety
///
/// `cookie` came from this emulation's `cnattach` or `attach` and is not detached; no other
/// call on it runs at the same time.
pub unsafe fn wsemul_vt100_output(cookie: *mut c_void, data: &[u8], kernel: bool) -> u32 {
    // SAFETY: the caller's contract.
    let edp = unsafe { &mut *cookie.cast::<WsemulVt100Emuldata>() };
    let mut data = data;
    let mut processed: u32 = 0;
    let mut rc: Result<(), Errno> = Ok(());

    #[cfg(feature = "diagnostic")]
    if kernel && !edp.console {
        #[allow(clippy::panic)] // the C's DIAGNOSTIC panic
        {
            panic!("wsemul_vt100_output: kernel output, not console");
        }
    }

    match edp.abortstate.state {
        ABORT_FAILED_CURSOR => {
            // If we could not display the cursor back, we pretended not having been able to
            // process the last byte. But this is a lie, so compensate here.
            if let Some((_, rest)) = data.split_first() {
                data = rest;
            }
            processed += 1;
            wsemul_reset_abortstate(&mut edp.abortstate);
        }
        ABORT_OK if edp.flags & VTFL_CURSORON != 0 => {
            // remove cursor image if visible
            let col = if HAVE_DOUBLE_WIDTH_HEIGHT {
                edp.ccol << edp.dw
            } else {
                edp.ccol
            };
            if edp.emulops.cursor(0, edp.crow as i32, col as i32).is_err() {
                return 0;
            }
        }
        _ => {}
    }

    loop {
        if HAVE_JUMP_SCROLL {
            let lines: i32 = match edp.abortstate.state {
                // If we failed a previous jump scroll attempt, we need to try to resume it
                // with the same distance. We can not recompute it since there might be more
                // bytes in the tty ring, causing a different result.
                ABORT_FAILED_JUMP_SCROLL => edp.abortstate.lines,
                // If we are at the bottom of the scrolling area, count newlines until an
                // escape sequence appears.
                ABORT_OK
                    if (edp.state == VT100_EMUL_STATE_NORMAL || kernel)
                        && edp.rows_below() == 0 =>
                {
                    wsemul_vt100_jump_scroll(edp, data, kernel) as i32
                }
                // Elsewhere in the scrolling area, or if we are recovering a non-scrolling
                // failure, do not try to scroll (yet).
                _ => 0,
            };

            if lines > 1 {
                wsemul_resume_abort(&mut edp.abortstate);
                if wsemul_vt100_scrollup(edp, lines).is_err() {
                    wsemul_abort_jump_scroll(&mut edp.abortstate, lines);
                    return processed;
                }
                wsemul_reset_abortstate(&mut edp.abortstate);
                edp.crow = edp.crow.wrapping_sub(lines as u32);
            }
        }

        wsemul_resume_abort(&mut edp.abortstate);

        let prev_count = data.len();
        let allow_utf8 = HAVE_UTF8_SUPPORT
            && edp.state == VT100_EMUL_STATE_NORMAL
            && !kernel
            && edp.flags & VTFL_UTF8 != 0;
        if wsemul_getchar(&mut data, edp.instate(kernel), allow_utf8).is_err() {
            break;
        }
        let consumed = (prev_count - data.len()) as u32;

        let inchar = edp.instate(kernel).inchar;
        if inchar & !0xff == 0 && (inchar & 0x7f) < 0x20 {
            rc = wsemul_vt100_output_c0c1(edp, kernel);
            if rc.is_err() {
                break;
            }
            processed += consumed;
            continue;
        }

        if edp.state == VT100_EMUL_STATE_NORMAL || kernel {
            rc = wsemul_vt100_output_normal(edp, kernel, 1);
            if rc.is_err() {
                break;
            }
            let st = edp.instate(kernel);
            st.last_output = st.inchar;
            processed += consumed;
            continue;
        }
        #[cfg(feature = "diagnostic")]
        if edp.state as usize > VT100_OUTPUT.len() {
            #[allow(clippy::panic)] // the C's DIAGNOSTIC panic
            {
                panic!("wsemul_vt100: invalid state {}", edp.state);
            }
        }
        rc = VT100_OUTPUT[edp.state as usize - 1](edp, kernel);
        if rc.is_err() {
            break;
        }
        processed += consumed;
    }

    if rc.is_err() {
        wsemul_abort_other(&mut edp.abortstate);
    } else if edp.flags & VTFL_CURSORON != 0 {
        // put cursor image back if visible
        let col = if HAVE_DOUBLE_WIDTH_HEIGHT {
            edp.ccol << edp.dw
        } else {
            edp.ccol
        };
        rc = edp.emulops.cursor(1, edp.crow as i32, col as i32);
        if rc.is_err() {
            // Pretend the last byte hasn't been processed, while remembering that only the
            // cursor operation really needs to be done.
            wsemul_abort_cursor(&mut edp.abortstate);
            processed = processed.wrapping_sub(1);
        }
    }

    if rc.is_ok() {
        wsemul_reset_abortstate(&mut edp.abortstate);
    }

    processed
}

/// `wsemul_vt100_jump_scroll`: the number of lines the bytes in `data` would scroll before
/// the next escape sequence (at most one less than the scrolling region), counted on a
/// copy of the input state.
fn wsemul_vt100_jump_scroll(edp: &WsemulVt100Emuldata, data: &[u8], kernel: bool) -> u32 {
    let mut data = data;
    let mut lines: u32 = 0;
    let mut pos = edp.ccol;
    let mut tmpstate = if kernel { edp.kstate } else { edp.instate }; // structure copy
    let ncols = edp.ncols_dw();
    let allow_utf8 = HAVE_UTF8_SUPPORT && !kernel && edp.flags & VTFL_UTF8 != 0;

    while wsemul_getchar(&mut data, &mut tmpstate, allow_utf8).is_ok() {
        // Only char causing a transition from VT100_EMUL_STATE_NORMAL to another state,
        // for now. Revisit this if this changes...
        if tmpstate.inchar == ASCII_ESC {
            break;
        }

        if edp.flags & VTFL_DECAWM != 0 {
            match tmpstate.inchar {
                ASCII_BS => pos = pos.saturating_sub(1),
                ASCII_CR => pos = 0,
                ASCII_HT => {
                    if let Some(tabs) = &edp.tabs {
                        pos += 1;
                        while pos < ncols.wrapping_sub(1) && tabs[pos as usize] == 0 {
                            pos += 1;
                        }
                    } else {
                        pos = (pos + 7) & !7;
                        if pos >= ncols {
                            pos = ncols.wrapping_sub(1);
                        }
                    }
                }
                c => {
                    if !(c & !0xff == 0 && (c & 0x7f) < 0x20) {
                        let wrapped = pos >= ncols;
                        pos += 1;
                        if wrapped {
                            pos = 0;
                            tmpstate.inchar = ASCII_LF;
                        }
                    }
                }
            }
        }

        if matches!(tmpstate.inchar, ASCII_LF | ASCII_VT | ASCII_FF) {
            lines += 1;
            if lines >= edp.scrreg_nrows.wrapping_sub(1) {
                break;
            }
        }
    }

    lines
}

#[cfg(test)]
mod tests;
