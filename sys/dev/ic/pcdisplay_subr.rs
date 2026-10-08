/* $OpenBSD: pcdisplay_subr.c,v 1.14 2020/05/25 09:55:48 jsg Exp $ */
/* $NetBSD: pcdisplay_subr.c,v 1.16 2000/06/08 07:01:19 cgd Exp $ */
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

#[cfg(test)]
mod tests;
