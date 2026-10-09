/* <LICENSES> */
/*
 * Copyright (c) 1995, 1996 Carnegie-Mellon University.
 * All rights reserved.
 *
 * Author: Chris G. Demetriou
 *
 * Permission to use, copy, modify and distribute this software and
 * its documentation is hereby granted, provided that both the copyright
 * notice and this permission notice appear in all copies of the
 * software, derivative works or modified versions, and any portions
 * thereof, and that both notices appear in supporting documentation.
 *
 * CARNEGIE MELLON ALLOWS FREE USE OF THIS SOFTWARE IN ITS "AS IS"
 * CONDITION.  CARNEGIE MELLON DISCLAIMS ANY LIABILITY OF ANY KIND
 * FOR ANY DAMAGES WHATSOEVER RESULTING FROM THE USE OF THIS SOFTWARE.
 *
 * Carnegie Mellon requests users of this software to return to
 *
 *  Software Distribution Coordinator  or  Software.Distribution@CS.CMU.EDU
 *  School of Computer Science
 *  Carnegie Mellon University
 *  Pittsburgh PA 15213-3890
 *
 * any improvements or extensions that they make and grant Carnegie the
 * rights to redistribute these changes.
 */
/* </LICENSES> */

/* <CODE> */
//! The emulops the PC text displays share: cursor, character cells and row/column moves on
//! a screen of 16-bit cells (character in the low byte, attribute in the high byte), in
//! video memory while the screen is displayed and in its backing store otherwise.
//!
//! Upstream: sys/dev/ic/pcdisplay_subr.c @ 3ce1f3f79392
//!
//! Each `pcdisplay_*` emulop takes the screen as its cookie: a [`Pcdisplayscreen`], or a
//! driver's screen that begins with one (`vga.c`'s `struct vgascreen`). They run at
//! `spltty()`.
//!
//! ## Deviations
//! - `PCDISPLAY_SOFTCURSOR` (an option no GENERIC sets: the hardware cursor is used) is
//!   the constant [`PCDISPLAY_SOFTCURSOR`]; both paths are compiled and the software one
//!   is dead code, as the C's `#ifdef` leaves it.
//! - The emulops return `Result<(), Errno>` (always `Ok`, the C's 0), `pcdisplay_getchar`
//!   the cell (the C fills `*cell` and returns 0).
//! - `bcopy` within the backing store is [`mem_move`], an overlapping move as `bcopy` is.

use core::cell::Cell;
use core::ffi::c_void;

use crate::dev::ic::pcdisplayvar::{Pcdisplayscreen, pcdisplay_6845_write};
use crate::dev::wscons::wsdisplayvar::WsdisplayCharcell;
use crate::machine::bus::{
    bus_space_copy_2, bus_space_read_2, bus_space_set_region_2, bus_space_write_2,
};
use crate::machine::intr::{spltty, splx};
use crate::sys::errno::Errno;

/// `PCDISPLAY_SOFTCURSOR`: draw the cursor by inverting the cell's colours instead of with
/// the 6845's hardware cursor. Not configured.
pub const PCDISPLAY_SOFTCURSOR: bool = false;

/// The screen behind an emulops cookie.
///
/// # Safety
///
/// `id` points to a live [`Pcdisplayscreen`] (or a `#[repr(C)]` screen starting with one).
unsafe fn scr_of<'a>(id: *mut c_void) -> &'a Pcdisplayscreen {
    // SAFETY: the caller's contract.
    unsafe { &*id.cast::<Pcdisplayscreen>() }
}

/// `bcopy(&mem[src], &mem[dst], n * 2)` inside one backing store: the cells move as if
/// through a temporary, whatever the overlap.
fn mem_move(mem: &[Cell<u16>], src: usize, dst: usize, n: usize) {
    if src >= dst {
        for i in 0..n {
            if let (Some(s), Some(d)) = (mem.get(src + i), mem.get(dst + i)) {
                d.set(s.get());
            }
        }
    } else {
        for i in (0..n).rev() {
            if let (Some(s), Some(d)) = (mem.get(src + i), mem.get(dst + i)) {
                d.set(s.get());
            }
        }
    }
}

/// `pcdisplay_cursor_reset`: with the software cursor, turn the hardware one off.
pub fn pcdisplay_cursor_reset(scr: &Pcdisplayscreen) {
    if PCDISPLAY_SOFTCURSOR {
        pcdisplay_6845_write!(scr.hdl(), curstart, 0x20);
        pcdisplay_6845_write!(scr.hdl(), curend, 0x00);
    }
}

/// `pcdisplay_cursor_init`: the cursor of a new screen; `existing` for the first screen,
/// which shows what the firmware left on the display.
pub fn pcdisplay_cursor_init(scr: &Pcdisplayscreen, existing: bool) {
    pcdisplay_cursor_reset(scr);

    if PCDISPLAY_SOFTCURSOR {
        if existing {
            // This is the first screen. At this point, scr->active is false and scr->mem
            // is NULL (no backing store), so we can't use pcdisplay_cursor() to do this.
            let ph = scr.hdl();
            let off = (scr.vc_crow.get() * scr.type_().ncols + scr.vc_ccol.get()) * 2
                + scr.dispoffset.get();

            let tmp = bus_space_read_2(ph.ph_memt, ph.ph_memh, off as usize);
            scr.cursortmp.set(i32::from(tmp));
            bus_space_write_2(ph.ph_memt, ph.ph_memh, off as usize, tmp ^ 0x7700);
        } else {
            scr.cursortmp.set(0);
        }
    }
    scr.cursoron.set(1);
}

/// `pcdisplay_cursor`: move the cursor to (`row`, `col`) and turn it on or off.
///
/// # Safety
///
/// `id` is a screen (see [`scr_of`]).
pub unsafe fn pcdisplay_cursor(id: *mut c_void, on: i32, row: i32, col: i32) -> Result<(), Errno> {
    // SAFETY: the caller's contract.
    let scr = unsafe { scr_of(id) };
    let ph = scr.hdl();
    let s = spltty();

    if PCDISPLAY_SOFTCURSOR {
        // Remove old cursor image
        if scr.cursoron.get() != 0 {
            let off = scr.vc_crow.get() * scr.type_().ncols + scr.vc_ccol.get();
            if scr.active.get() != 0 {
                bus_space_write_2(
                    ph.ph_memt,
                    ph.ph_memh,
                    (scr.dispoffset.get() + off * 2) as usize,
                    scr.cursortmp.get() as u16,
                );
            } else if let Some(c) = scr.mem().get(off as usize) {
                c.set(scr.cursortmp.get() as u16);
            }
        }

        scr.vc_crow.set(row);
        scr.vc_ccol.set(col);

        scr.cursoron.set(on);
        if on != 0 {
            let off = scr.vc_crow.get() * scr.type_().ncols + scr.vc_ccol.get();
            if scr.active.get() != 0 {
                let off = (off * 2 + scr.dispoffset.get()) as usize;
                let tmp = bus_space_read_2(ph.ph_memt, ph.ph_memh, off);
                scr.cursortmp.set(i32::from(tmp));
                bus_space_write_2(ph.ph_memt, ph.ph_memh, off, tmp ^ 0x7700);
            } else if let Some(c) = scr.mem().get(off as usize) {
                scr.cursortmp.set(i32::from(c.get()));
                c.set(c.get() ^ 0x7700);
            }
        }
    } else {
        scr.vc_crow.set(row);
        scr.vc_ccol.set(col);
        scr.cursoron.set(on);

        if scr.active.get() != 0 {
            let pos = if on == 0 {
                0x1010
            } else {
                scr.dispoffset.get() / 2 + row * scr.type_().ncols + col
            };

            pcdisplay_6845_write!(ph, cursorh, (pos >> 8) as u8);
            pcdisplay_6845_write!(ph, cursorl, pos as u8);
        }
    }

    splx(s);
    Ok(())
}

/// `pcdisplay_putchar`: character `c` with attribute `attr` at (`row`, `col`).
///
/// # Safety
///
/// `id` is a screen (see [`scr_of`]).
pub unsafe fn pcdisplay_putchar(
    id: *mut c_void,
    row: i32,
    col: i32,
    c: u32,
    attr: u32,
) -> Result<(), Errno> {
    // SAFETY: the caller's contract.
    let scr = unsafe { scr_of(id) };
    let ph = scr.hdl();
    let off = row * scr.type_().ncols + col;
    let cell = (c | (attr << 8)) as u16;

    let s = spltty();
    if scr.active.get() != 0 {
        bus_space_write_2(
            ph.ph_memt,
            ph.ph_memh,
            (scr.dispoffset.get() + off * 2) as usize,
            cell,
        );
    } else if let Some(m) = scr.mem().get(off as usize) {
        m.set(cell);
    }
    splx(s);

    Ok(())
}

/// `pcdisplay_getchar`: the character and attribute at (`row`, `col`).
///
/// # Safety
///
/// `id` is a screen (see [`scr_of`]).
pub unsafe fn pcdisplay_getchar(id: *mut c_void, row: i32, col: i32) -> WsdisplayCharcell {
    // SAFETY: the caller's contract.
    let scr = unsafe { scr_of(id) };
    let ph = scr.hdl();
    let off = row * scr.type_().ncols + col;
    // XXX bounds check?

    let s = spltty();
    let data = if scr.active.get() != 0 {
        bus_space_read_2(
            ph.ph_memt,
            ph.ph_memh,
            (scr.dispoffset.get() + off * 2) as usize,
        )
    } else {
        scr.mem().get(off as usize).map_or(0, Cell::get)
    };
    splx(s);

    WsdisplayCharcell {
        uc: u32::from(data & 0xff),
        attr: u32::from(data >> 8),
    }
}

/// `pcdisplay_copycols`: copy `ncols` cells of `row` from `srccol` to `dstcol`.
///
/// # Safety
///
/// `id` is a screen (see [`scr_of`]).
pub unsafe fn pcdisplay_copycols(
    id: *mut c_void,
    row: i32,
    srccol: i32,
    dstcol: i32,
    ncols: i32,
) -> Result<(), Errno> {
    // SAFETY: the caller's contract.
    let scr = unsafe { scr_of(id) };
    let ph = scr.hdl();
    let rowoff = row * scr.type_().ncols;
    let srcoff = (rowoff + srccol) as usize;
    let dstoff = (rowoff + dstcol) as usize;
    let disp = scr.dispoffset.get() as usize;

    let s = spltty();
    if scr.active.get() != 0 {
        bus_space_copy_2(
            ph.ph_memt,
            ph.ph_memh,
            disp + srcoff * 2,
            ph.ph_memh,
            disp + dstoff * 2,
            ncols as usize,
        );
    } else {
        mem_move(scr.mem(), srcoff, dstoff, ncols as usize);
    }
    splx(s);

    Ok(())
}

/// `pcdisplay_erasecols`: blank `ncols` cells of `row` from `startcol` with `fillattr`.
///
/// # Safety
///
/// `id` is a screen (see [`scr_of`]).
pub unsafe fn pcdisplay_erasecols(
    id: *mut c_void,
    row: i32,
    startcol: i32,
    ncols: i32,
    fillattr: u32,
) -> Result<(), Errno> {
    // SAFETY: the caller's contract.
    let scr = unsafe { scr_of(id) };
    let ph = scr.hdl();
    let off = (row * scr.type_().ncols + startcol) as usize;
    let val = ((fillattr << 8) | u32::from(b' ')) as u16;

    let s = spltty();
    if scr.active.get() != 0 {
        bus_space_set_region_2(
            ph.ph_memt,
            ph.ph_memh,
            scr.dispoffset.get() as usize + off * 2,
            val,
            ncols as usize,
        );
    } else {
        for c in scr.mem().iter().skip(off).take(ncols as usize) {
            c.set(val);
        }
    }
    splx(s);

    Ok(())
}

/// `pcdisplay_copyrows`: copy `nrows` rows from `srcrow` to `dstrow`.
///
/// # Safety
///
/// `id` is a screen (see [`scr_of`]).
pub unsafe fn pcdisplay_copyrows(
    id: *mut c_void,
    srcrow: i32,
    dstrow: i32,
    nrows: i32,
) -> Result<(), Errno> {
    // SAFETY: the caller's contract.
    let scr = unsafe { scr_of(id) };
    let ph = scr.hdl();
    let ncols = scr.type_().ncols;
    let srcoff = (srcrow * ncols) as usize;
    let dstoff = (dstrow * ncols) as usize;
    let disp = scr.dispoffset.get() as usize;
    let count = (nrows * ncols) as usize;

    let s = spltty();
    if scr.active.get() != 0 {
        bus_space_copy_2(
            ph.ph_memt,
            ph.ph_memh,
            disp + srcoff * 2,
            ph.ph_memh,
            disp + dstoff * 2,
            count,
        );
    } else {
        mem_move(scr.mem(), srcoff, dstoff, count);
    }
    splx(s);

    Ok(())
}

/// `pcdisplay_eraserows`: blank `nrows` rows from `startrow` with `fillattr`.
///
/// # Safety
///
/// `id` is a screen (see [`scr_of`]).
pub unsafe fn pcdisplay_eraserows(
    id: *mut c_void,
    startrow: i32,
    nrows: i32,
    fillattr: u32,
) -> Result<(), Errno> {
    // SAFETY: the caller's contract.
    let scr = unsafe { scr_of(id) };
    let ph = scr.hdl();
    let off = (startrow * scr.type_().ncols) as usize;
    let count = (nrows * scr.type_().ncols) as usize;
    let val = ((fillattr << 8) | u32::from(b' ')) as u16;

    let s = spltty();
    if scr.active.get() != 0 {
        bus_space_set_region_2(
            ph.ph_memt,
            ph.ph_memh,
            scr.dispoffset.get() as usize + off * 2,
            val,
            count,
        );
    } else {
        for c in scr.mem().iter().skip(off).take(count) {
            c.set(val);
        }
    }
    splx(s);

    Ok(())
}
/* </CODE> */

/* <TESTS> */
#[cfg(test)]
mod tests {
    // Host tests of `pcdisplay_subr.c`: the emulops over a screen that is not displayed, whose
    // cells live in its backing store (the host's bus space reads zeros and drops writes, so
    // the displayed path has nothing to observe here).

    use std::boxed::Box;
    use std::vec;
    use std::vec::Vec;

    use super::*;
    use crate::dev::ic::pcdisplayvar::PcdisplayHandle;
    use crate::dev::wscons::wsdisplayvar::WsscreenDescr;
    use crate::machine::bus::{BusSpaceTag, bus_space_map};

    const COLS: i32 = 4;
    const ROWS: i32 = 3;

    /// A 4x3 screen over a fake text buffer, not displayed.
    struct Fake {
        scr: Box<Pcdisplayscreen>,
        _type: Box<WsscreenDescr>,
        _hdl: Box<PcdisplayHandle>,
        _mem: Vec<u16>,
    }

    impl Fake {
        fn new() -> Self {
            let t = BusSpaceTag::default();
            // SAFETY: the host's bus space maps nothing; the handle is only a number.
            let h = unsafe { bus_space_map(t, 0xb8000, 0x8000, 0) }.unwrap();
            let hdl = Box::new(PcdisplayHandle {
                ph_iot: t,
                ph_memt: t,
                ph_ioh_6845: h,
                ph_memh: h,
            });
            let type_ = Box::new(WsscreenDescr {
                ncols: COLS,
                nrows: ROWS,
                fontwidth: 8,
                fontheight: 16,
                ..WsscreenDescr::new(b"4x3")
            });
            let mut mem = vec![0u16; (COLS * ROWS) as usize];
            let scr = Box::new(Pcdisplayscreen::new());
            scr.hdl.set(&*hdl);
            scr.type_.set(&*type_);
            scr.mem.set(mem.as_mut_ptr());
            Fake {
                scr,
                _type: type_,
                _hdl: hdl,
                _mem: mem,
            }
        }

        fn id(&self) -> *mut c_void {
            ptr_of(&self.scr)
        }

        fn put(&self, row: i32, col: i32, c: u8, attr: u32) {
            // SAFETY: a pcdisplay screen.
            unsafe { pcdisplay_putchar(self.id(), row, col, u32::from(c), attr) }.unwrap();
        }

        fn cell(&self, row: i32, col: i32) -> WsdisplayCharcell {
            // SAFETY: a pcdisplay screen.
            unsafe { pcdisplay_getchar(self.id(), row, col) }
        }

        fn row(&self, row: i32) -> std::string::String {
            (0..COLS)
                .map(|c| char::from(self.cell(row, c).uc as u8))
                .collect()
        }
    }

    fn ptr_of(scr: &Pcdisplayscreen) -> *mut c_void {
        core::ptr::from_ref(scr).cast_mut().cast()
    }

    #[test]
    fn putchar_and_getchar_use_the_backing_store() {
        let f = Fake::new();
        f.put(1, 2, b'x', 0x1f);
        assert_eq!(
            f.cell(1, 2),
            WsdisplayCharcell {
                uc: u32::from(b'x'),
                attr: 0x1f
            }
        );
        assert_eq!(
            f.scr.mem()[(COLS + 2) as usize].get(),
            0x1f00 | u16::from(b'x')
        );
        // outside the screen: ignored, reads as an empty cell
        f.put(ROWS, 0, b'y', 7);
        assert_eq!(f.cell(ROWS, 0), WsdisplayCharcell { uc: 0, attr: 0 });
    }

    #[test]
    fn erase_fills_with_blanks_of_the_attribute() {
        let f = Fake::new();
        // SAFETY: a pcdisplay screen.
        unsafe { pcdisplay_eraserows(f.id(), 0, ROWS, 0x07) }.unwrap();
        assert_eq!(f.row(2), "    ");
        // SAFETY: as above.
        unsafe { pcdisplay_erasecols(f.id(), 1, 1, 2, 0x70) }.unwrap();
        assert_eq!(f.cell(1, 0).attr, 0x07);
        assert_eq!(f.cell(1, 1).attr, 0x70);
        assert_eq!(f.cell(1, 2).attr, 0x70);
        assert_eq!(f.cell(1, 3).attr, 0x07);
    }

    #[test]
    fn copies_move_overlapping_cells() {
        let f = Fake::new();
        for (i, c) in b"abcd".iter().enumerate() {
            f.put(0, i as i32, *c, 7);
        }
        // right by one, overlapping: a move, not a smear
        // SAFETY: a pcdisplay screen.
        unsafe { pcdisplay_copycols(f.id(), 0, 0, 1, 3) }.unwrap();
        assert_eq!(f.row(0), "aabc");
        // and back left
        // SAFETY: as above.
        unsafe { pcdisplay_copycols(f.id(), 0, 1, 0, 3) }.unwrap();
        assert_eq!(f.row(0), "abcc");

        // rows down by one, overlapping
        // SAFETY: as above.
        unsafe { pcdisplay_copyrows(f.id(), 0, 1, 2) }.unwrap();
        assert_eq!(f.row(1), "abcc");
        assert_eq!(f.row(2), "\0\0\0\0");
    }

    #[test]
    fn cursor_moves_without_touching_cells() {
        let f = Fake::new();
        pcdisplay_cursor_init(&f.scr, false);
        assert_eq!(f.scr.cursoron.get(), 1);
        // SAFETY: a pcdisplay screen.
        unsafe { pcdisplay_cursor(f.id(), 0, 2, 3) }.unwrap();
        assert_eq!((f.scr.vc_crow.get(), f.scr.vc_ccol.get()), (2, 3));
        assert_eq!(f.scr.cursoron.get(), 0);
        assert!(f.scr.mem().iter().all(|c| c.get() == 0));
    }
}
/* </TESTS> */
