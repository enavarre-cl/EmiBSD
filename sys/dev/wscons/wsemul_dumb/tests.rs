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
    let cookie: *mut c_void = (&mut *scr as *mut crate::dev::wscons::testutil::FakeScreen).cast();
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
