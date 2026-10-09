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
/* </LICENSES> */

/* <CODE> */
//! Host test double for the terminal emulations (`wsemul_dumb`, `wsemul_vt100`). Not an
//! OpenBSD file.
//!
//! [`FakeScreen`] is a display: its emulops ([`FAKE_EMULOPS`]) draw into a character grid,
//! log every call, and can be told to fail one call (to exercise the emulations' abort and
//! resume). [`wsdisplay_emulbell`] and [`wsdisplay_emulinput`] stand in for `wsdisplay.c`'s
//! callbacks in the test build: they record into the screen the emulation's `cbcookie`
//! points at.

use core::ffi::c_void;
use std::boxed::Box;
use std::string::String;
use std::vec;
use std::vec::Vec;

use crate::dev::wscons::wsdisplayvar::{WsdisplayEmulops, WsscreenDescr};
use crate::dev::wscons::wsemulvar::WsemulOps;
use crate::sys::errno::Errno;

/// One emulop call.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Op {
    Cursor(i32, i32, i32),
    Putchar(i32, i32, u32, u32),
    Copycols(i32, i32, i32, i32),
    Erasecols(i32, i32, i32, u32),
    Copyrows(i32, i32, i32),
    Eraserows(i32, i32, u32),
}

/// A fake display screen.
pub(crate) struct FakeScreen {
    pub rows: usize,
    pub cols: usize,
    /// `(character, attribute)` per cell, row-major; an erased cell holds a space.
    pub cells: Vec<(u32, u32)>,
    /// Where the cursor image is shown.
    pub cursor: Option<(i32, i32)>,
    /// The emulops that succeeded, in order.
    pub log: Vec<Op>,
    /// Emulop calls attempted so far (cursor, putchar, copy and erase).
    pub calls: usize,
    /// Fail the call whose number (counted by `calls`, from 0) this is, once.
    pub fail_at: Option<usize>,
    /// `wsdisplay_emulbell` calls.
    pub bells: usize,
    /// What `wsdisplay_emulinput` sent to the tty.
    pub input: Vec<u8>,
}

impl FakeScreen {
    pub fn new(rows: usize, cols: usize) -> Box<Self> {
        Box::new(Self {
            rows,
            cols,
            cells: vec![(u32::from(b' '), 0); rows * cols],
            cursor: None,
            log: Vec::new(),
            calls: 0,
            fail_at: None,
            bells: 0,
            input: Vec::new(),
        })
    }

    /// The characters of row `r`, `'?'` for those that are not `char`s.
    pub fn row(&self, r: usize) -> String {
        self.cells[r * self.cols..(r + 1) * self.cols]
            .iter()
            .map(|&(c, _)| char::from_u32(c).unwrap_or('?'))
            .collect()
    }

    /// Row `r` without its trailing spaces.
    pub fn line(&self, r: usize) -> String {
        String::from(self.row(r).trim_end())
    }

    pub fn cell(&self, r: usize, c: usize) -> (u32, u32) {
        self.cells[r * self.cols + c]
    }

    /// Counts a call and fails it if it is the one `fail_at` names.
    fn call(&mut self) -> Result<(), Errno> {
        let n = self.calls;
        self.calls += 1;
        if self.fail_at == Some(n) {
            self.fail_at = None;
            return Err(Errno::EAGAIN);
        }
        Ok(())
    }

    fn check(&self, row: i32, col: i32, ncols: i32) {
        assert!(row >= 0 && (row as usize) < self.rows, "row {row}");
        assert!(
            col >= 0 && ncols >= 0 && (col + ncols) as usize <= self.cols,
            "cols {col}+{ncols}"
        );
    }
}

/// The screen behind a cookie.
///
/// # Safety
///
/// `c` is a live `FakeScreen` no other reference points to during the call.
unsafe fn scr<'a>(c: *mut c_void) -> &'a mut FakeScreen {
    // SAFETY: the caller's contract.
    unsafe { &mut *c.cast::<FakeScreen>() }
}

unsafe fn cursor(c: *mut c_void, on: i32, row: i32, col: i32) -> Result<(), Errno> {
    // SAFETY: the emulation passes the cookie the test bound to this table.
    let s = unsafe { scr(c) };
    s.call()?;
    s.check(row, col, 1);
    s.cursor = if on != 0 { Some((row, col)) } else { None };
    s.log.push(Op::Cursor(on, row, col));
    Ok(())
}

unsafe fn mapchar(_c: *mut c_void, ch: i32, cp: &mut u32) -> i32 {
    *cp = ch as u32;
    5
}

unsafe fn putchar(c: *mut c_void, row: i32, col: i32, uc: u32, attr: u32) -> Result<(), Errno> {
    // SAFETY: as in `cursor`.
    let s = unsafe { scr(c) };
    s.call()?;
    s.check(row, col, 1);
    let cols = s.cols;
    s.cells[row as usize * cols + col as usize] = (uc, attr);
    s.log.push(Op::Putchar(row, col, uc, attr));
    Ok(())
}

unsafe fn copycols(c: *mut c_void, row: i32, src: i32, dst: i32, n: i32) -> Result<(), Errno> {
    // SAFETY: as in `cursor`.
    let s = unsafe { scr(c) };
    s.call()?;
    s.check(row, src, n);
    s.check(row, dst, n);
    let base = row as usize * s.cols;
    let (src, dst, n) = (src as usize, dst as usize, n as usize);
    s.cells.copy_within(base + src..base + src + n, base + dst);
    s.log
        .push(Op::Copycols(row, src as i32, dst as i32, n as i32));
    Ok(())
}

unsafe fn erasecols(c: *mut c_void, row: i32, col: i32, n: i32, attr: u32) -> Result<(), Errno> {
    // SAFETY: as in `cursor`.
    let s = unsafe { scr(c) };
    s.call()?;
    s.check(row, col, n);
    let base = row as usize * s.cols + col as usize;
    s.cells[base..base + n as usize].fill((u32::from(b' '), attr));
    s.log.push(Op::Erasecols(row, col, n, attr));
    Ok(())
}

unsafe fn copyrows(c: *mut c_void, src: i32, dst: i32, n: i32) -> Result<(), Errno> {
    // SAFETY: as in `cursor`.
    let s = unsafe { scr(c) };
    s.call()?;
    assert!(src >= 0 && dst >= 0 && n >= 0);
    assert!((src + n) as usize <= s.rows && (dst + n) as usize <= s.rows);
    let w = s.cols;
    let (src, dst, n) = (src as usize, dst as usize, n as usize);
    s.cells.copy_within(src * w..(src + n) * w, dst * w);
    s.log.push(Op::Copyrows(src as i32, dst as i32, n as i32));
    Ok(())
}

unsafe fn eraserows(c: *mut c_void, row: i32, n: i32, attr: u32) -> Result<(), Errno> {
    // SAFETY: as in `cursor`.
    let s = unsafe { scr(c) };
    s.call()?;
    assert!(
        row >= 0 && n >= 0 && (row + n) as usize <= s.rows,
        "rows {row}+{n}"
    );
    let w = s.cols;
    s.cells[row as usize * w..(row + n) as usize * w].fill((u32::from(b' '), attr));
    s.log.push(Op::Eraserows(row, n, attr));
    Ok(())
}

/// The attribute of `fg`/`bg`/`flags`: `fg << 24 | bg << 16 | flags`.
pub(crate) fn attr(fg: i32, bg: i32, flags: i32) -> u32 {
    ((fg as u32 & 0xff) << 24) | ((bg as u32 & 0xff) << 16) | (flags as u32 & 0xffff)
}

unsafe fn pack_attr(_c: *mut c_void, fg: i32, bg: i32, flags: i32) -> Result<u32, Errno> {
    Ok(attr(fg, bg, flags))
}

unsafe fn unpack_attr(_c: *mut c_void, a: u32) -> (i32, i32, i32) {
    ((a >> 24) as i32, ((a >> 16) & 0xff) as i32, 0)
}

/// The fake display's emulops, every one present.
pub(crate) static FAKE_EMULOPS: WsdisplayEmulops = WsdisplayEmulops {
    cursor: Some(cursor),
    mapchar: Some(mapchar),
    putchar: Some(putchar),
    copycols: Some(copycols),
    erasecols: Some(erasecols),
    copyrows: Some(copyrows),
    eraserows: Some(eraserows),
    pack_attr: Some(pack_attr),
    unpack_attr: Some(unpack_attr),
};

/// A display that can only `putchar` (a "crippled" one, for the dumb emulation).
pub(crate) static PUTCHAR_ONLY_EMULOPS: WsdisplayEmulops = WsdisplayEmulops {
    putchar: Some(putchar),
    ..WsdisplayEmulops::EMPTY
};

/// A screen type of `rows` x `cols` with capabilities `caps` over `textops`.
pub(crate) fn descr(
    rows: usize,
    cols: usize,
    caps: i32,
    textops: &'static WsdisplayEmulops,
) -> WsscreenDescr {
    let mut d = WsscreenDescr::new(b"fake");
    d.nrows = rows as i32;
    d.ncols = cols as i32;
    d.capabilities = caps;
    d.textops = textops;
    d.fontwidth = 8;
    d.fontheight = 16;
    d
}

/// An emulation attached (not as console) to a fake screen; detached on drop.
pub(crate) struct Rig {
    pub ops: &'static WsemulOps,
    scr: *mut FakeScreen,
    pub edp: *mut c_void,
}

impl Rig {
    /// `ops` attached to a new `rows` x `cols` screen with capabilities `caps`, the cursor
    /// at the origin, default attribute 0; the callbacks record into the screen.
    pub fn new(
        ops: &'static WsemulOps,
        rows: usize,
        cols: usize,
        caps: i32,
        textops: &'static WsdisplayEmulops,
    ) -> Self {
        let scr = Box::into_raw(FakeScreen::new(rows, cols));
        let d = descr(rows, cols, caps, textops);
        // SAFETY: the table is static and the cookie a live screen the rig frees last.
        let edp = unsafe { (ops.attach)(false, Some(&d), scr.cast(), 0, 0, scr.cast(), 0) };
        assert!(!edp.is_null());
        Self { ops, scr, edp }
    }

    /// Feeds `data` as userland output; the bytes processed.
    pub fn out(&mut self, data: &[u8]) -> u32 {
        // SAFETY: `edp` is the state `attach` made for this table.
        unsafe { (self.ops.output)(self.edp, data, false) }
    }

    /// Feeds `data` until all of it is processed, as the tty layer retries.
    pub fn out_all(&mut self, data: &[u8]) {
        let mut rest = data;
        for _ in 0..1000 {
            if rest.is_empty() {
                return;
            }
            let n = self.out(rest) as usize;
            rest = &rest[n..];
        }
        panic!("output does not progress");
    }

    pub fn scr(&self) -> &FakeScreen {
        // SAFETY: the screen lives as long as the rig; no emulop runs during the borrow.
        unsafe { &*self.scr }
    }

    pub fn scr_mut(&mut self) -> &mut FakeScreen {
        // SAFETY: as in `scr`.
        unsafe { &mut *self.scr }
    }
}

impl Drop for Rig {
    fn drop(&mut self) {
        let (mut r, mut c) = (0, 0);
        // SAFETY: `edp` came from `attach` and is not used again.
        unsafe { (self.ops.detach)(self.edp, &mut r, &mut c) };
        // SAFETY: `scr` came from `Box::into_raw` in `new`.
        drop(unsafe { Box::from_raw(self.scr) });
    }
}

/// `wsdisplay_emulbell` of the test build: counts a bell on the screen `v` points at.
pub(crate) fn wsdisplay_emulbell(v: *mut c_void) {
    if v.is_null() {
        return;
    }
    // SAFETY: the rigs pass their live screen as the callback cookie.
    unsafe { scr(v) }.bells += 1;
}

/// `wsdisplay_emulinput` of the test build: records `data` on the screen `v` points at.
pub(crate) fn wsdisplay_emulinput(v: *mut c_void, data: &[u8]) {
    if v.is_null() {
        return;
    }
    // SAFETY: as in `wsdisplay_emulbell`.
    unsafe { scr(v) }.input.extend_from_slice(data);
}
/* </CODE> */
