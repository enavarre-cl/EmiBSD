//! Host tests of `pcdisplay_subr.c`: the emulops over a screen that is not displayed, whose
//! cells live in its backing store (the host's bus space reads zeros and drops writes, so
//! the displayed path has nothing to observe here).

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
