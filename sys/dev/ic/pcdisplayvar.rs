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
//! `<dev/ic/pcdisplayvar.h>`: the state the PC text displays (`vga(4)`, `pcdisplay`) share:
//! a screen ([`Pcdisplayscreen`]: its type, cursor, display offset and backing store) and
//! the bus handles of the adapter ([`PcdisplayHandle`]), with the 6845 accessors.
//!
//! Upstream: sys/dev/ic/pcdisplayvar.h @ 3ce1f3f79392
//!
//! A screen is the emulops' cookie: `vga.c` embeds it at the start of its `struct
//! vgascreen` and hands that pointer to the `pcdisplay_*` emulops of `pcdisplay_subr.c`,
//! which see only this first member. Both are `#[repr(C)]` for that reason.
//!
//! ## Deviations
//! - The screen's members are `Cell`s: the emulops, wsdisplay and the switch timeout reach
//!   a screen through shared pointers (the C's `void *` cookie), under `spltty()` and the
//!   kernel lock as in C.
//! - `cursortmp` exists whatever `PCDISPLAY_SOFTCURSOR` says (`pcdisplay_subr.rs`'s
//!   constant; the option is not in GENERIC), so both cursor paths compile.
//! - `pcdisplay_6845_read`/`pcdisplay_6845_write` are macros taking the register's field
//!   name, as the C's do, over [`_pcdisplay_6845_read`]/[`_pcdisplay_6845_write`] and
//!   `offset_of!` on [`RegMc6845`](crate::dev::ic::mc6845reg::RegMc6845).
//! - The backing store (`mem`, `u_int16_t *`) is read and written through
//!   [`Pcdisplayscreen::mem`], a slice of `ncols * nrows` cells; a cell outside it is
//!   ignored where the C would write past the allocation.

use core::cell::Cell;
use core::ptr;

use crate::dev::ic::mc6845reg::{MC6845_DATA, MC6845_INDEX};
use crate::dev::wscons::wsdisplayvar::WsscreenDescr;
use crate::machine::bus::{BusSpaceHandle, BusSpaceTag, bus_space_read_1, bus_space_write_1};

/// `struct pcdisplayscreen`.
#[repr(C)]
pub struct Pcdisplayscreen {
    /// `hdl`: the adapter's handles (inside its `struct vga_config`).
    pub hdl: Cell<*const PcdisplayHandle>,
    /// `type`: the screen's type.
    pub type_: Cell<*const WsscreenDescr>,
    /// `active`: currently displayed.
    pub active: Cell<i32>,
    /// `mem`: backing store for contents (`ncols * nrows` cells), NULL for the one screen
    /// of an adapter that has no other.
    pub mem: Cell<*mut u16>,
    /// `cursoron`: cursor displayed?
    pub cursoron: Cell<i32>,
    /// `cursortmp`: glyph & attribute behind software cursor (`PCDISPLAY_SOFTCURSOR`).
    pub cursortmp: Cell<i32>,
    /// `vc_ccol`: current cursor column.
    pub vc_ccol: Cell<i32>,
    /// `vc_crow`: current cursor row.
    pub vc_crow: Cell<i32>,
    /// `dispoffset`: offset of displayed area in video mem.
    pub dispoffset: Cell<i32>,
    /// `visibleoffset`.
    pub visibleoffset: Cell<i32>,
}

impl Pcdisplayscreen {
    /// A screen with every member zero (the C's static or freshly allocated one).
    pub const fn new() -> Self {
        Self {
            hdl: Cell::new(ptr::null()),
            type_: Cell::new(ptr::null()),
            active: Cell::new(0),
            mem: Cell::new(ptr::null_mut()),
            cursoron: Cell::new(0),
            cursortmp: Cell::new(0),
            vc_ccol: Cell::new(0),
            vc_crow: Cell::new(0),
            dispoffset: Cell::new(0),
            visibleoffset: Cell::new(0),
        }
    }

    /// `scr->hdl`.
    pub fn hdl(&self) -> &PcdisplayHandle {
        // SAFETY: the driver points `hdl` at its configuration's handle before the screen is
        // used (`vga_init_screen`), and the configuration outlives its screens.
        unsafe { &*self.hdl.get() }
    }

    /// `scr->type`.
    pub fn type_(&self) -> &WsscreenDescr {
        // SAFETY: the driver sets the type before the screen is used; types are the
        // driver's static descriptors.
        unsafe { &*self.type_.get() }
    }

    /// The backing store `mem` as `ncols * nrows` cells; empty when there is none.
    pub fn mem(&self) -> &[Cell<u16>] {
        let mem = self.mem.get();
        if mem.is_null() || self.type_.get().is_null() {
            return &[];
        }
        let t = self.type_();
        let n = (t.ncols.max(0) * t.nrows.max(0)) as usize;
        // SAFETY: `mem` is the `mallocarray(ncols, nrows * 2)` of `vga_alloc_screen` for
        // this screen's type, alive until `vga_free_screen`; `Cell<u16>` has `u16`'s layout,
        // and the store is only touched through these cells.
        unsafe { core::slice::from_raw_parts(mem.cast::<Cell<u16>>(), n) }
    }
}

impl Default for Pcdisplayscreen {
    fn default() -> Self {
        Self::new()
    }
}

/// `struct pcdisplay_handle`.
#[derive(Clone, Copy)]
pub struct PcdisplayHandle {
    /// `ph_iot`.
    pub ph_iot: BusSpaceTag,
    /// `ph_memt`.
    pub ph_memt: BusSpaceTag,
    /// `ph_ioh_6845`.
    pub ph_ioh_6845: BusSpaceHandle,
    /// `ph_memh`.
    pub ph_memh: BusSpaceHandle,
}

/// `_pcdisplay_6845_read`: register `reg` of the 6845.
#[inline]
pub fn _pcdisplay_6845_read(ph: &PcdisplayHandle, reg: usize) -> u8 {
    bus_space_write_1(ph.ph_iot, ph.ph_ioh_6845, MC6845_INDEX, reg as u8);
    bus_space_read_1(ph.ph_iot, ph.ph_ioh_6845, MC6845_DATA)
}

/// `_pcdisplay_6845_write`.
#[inline]
pub fn _pcdisplay_6845_write(ph: &PcdisplayHandle, reg: usize, val: u8) {
    bus_space_write_1(ph.ph_iot, ph.ph_ioh_6845, MC6845_INDEX, reg as u8);
    bus_space_write_1(ph.ph_iot, ph.ph_ioh_6845, MC6845_DATA, val);
}

/// `pcdisplay_6845_read(ph, reg)`: the 6845 register named by `reg`, a field of `struct
/// reg_mc6845`.
macro_rules! pcdisplay_6845_read {
    ($ph:expr, $reg:ident) => {
        $crate::dev::ic::pcdisplayvar::_pcdisplay_6845_read(
            $ph,
            ::core::mem::offset_of!($crate::dev::ic::mc6845reg::RegMc6845, $reg),
        )
    };
}
pub(crate) use pcdisplay_6845_read;

/// `pcdisplay_6845_write(ph, reg, val)`.
macro_rules! pcdisplay_6845_write {
    ($ph:expr, $reg:ident, $val:expr) => {
        $crate::dev::ic::pcdisplayvar::_pcdisplay_6845_write(
            $ph,
            ::core::mem::offset_of!($crate::dev::ic::mc6845reg::RegMc6845, $reg),
            $val,
        )
    };
}
pub(crate) use pcdisplay_6845_write;
/* </CODE> */

/* <TESTS> */
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_screen_without_backing_store_has_no_cells() {
        let scr = Pcdisplayscreen::new();
        assert!(scr.mem().is_empty());
        assert_eq!(scr.active.get(), 0);
    }
}
/* </TESTS> */
