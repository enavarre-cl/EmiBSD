use super::*;
use crate::dev::wscons::testutil::{FAKE_EMULOPS, FakeScreen, Op, Rig, attr, descr};
use crate::dev::wscons::wsdisplayvar::{
    WSATTR_HILIT, WSATTR_REVERSE, WSATTR_UNDERLINE, WSCOL_CYAN, WSCOL_GREEN, WSCOL_RED,
    WSSCREEN_BLINK, WSSCREEN_HILIT, WSSCREEN_REVERSE, WSSCREEN_UNDERLINE,
};
use std::string::String;
use std::vec::Vec;

const ALLCAPS: i32 =
    WSSCREEN_WSCOLORS | WSSCREEN_REVERSE | WSSCREEN_HILIT | WSSCREEN_BLINK | WSSCREEN_UNDERLINE;

fn rig(rows: usize, cols: usize) -> Rig {
    Rig::new(&WSEMUL_VT100_OPS, rows, cols, ALLCAPS, &FAKE_EMULOPS)
}

fn edp(r: &Rig) -> &WsemulVt100Emuldata {
    // SAFETY: the rig's state is a vt100 emulation's, not in use.
    unsafe { &*r.edp.cast::<WsemulVt100Emuldata>() }
}

fn pos(r: &Rig) -> (u32, u32) {
    (edp(r).crow, edp(r).ccol)
}

fn lines(r: &Rig) -> Vec<String> {
    (0..r.scr().rows).map(|i| r.scr().line(i)).collect()
}

#[test]
fn text_wraps_at_the_margin() {
    let mut r = rig(3, 5);
    assert_eq!(r.out(b"abcde"), 5);
    // The last column holds the cursor until the next character (VTFL_LASTCHAR).
    assert_eq!(pos(&r), (0, 4));
    assert_ne!(edp(&r).flags & VTFL_LASTCHAR, 0);
    r.out(b"fg");
    assert_eq!(lines(&r), ["abcde", "fg", ""]);
    assert_eq!(pos(&r), (1, 2));
    assert_eq!(r.scr().cursor, Some((1, 2)));
    // Without autowrap the last column is overwritten.
    r.out(b"\x1b[?7lxyzuvw");
    assert_eq!(r.scr().line(1), "fgxyw");
}

#[test]
fn newlines_scroll_at_the_bottom_with_jump_scroll() {
    let mut r = rig(3, 8);
    r.out(b"one\r\ntwo\r\nthree");
    assert_eq!(lines(&r), ["one", "two", "three"]);
    // Two line feeds at the bottom: one scroll of two lines.
    r.out(b"\r\nfour\r\nfive");
    assert_eq!(lines(&r), ["three", "four", "five"]);
    assert!(r.scr().log.contains(&Op::Copyrows(2, 0, 1)));
    assert!(r.scr().log.contains(&Op::Eraserows(1, 2, 0)));
    assert_eq!(pos(&r), (2, 4));
}

#[test]
fn control_characters() {
    let mut r = rig(4, 20);
    r.out(b"ab\x08c\tX\x07");
    assert_eq!(r.scr().line(0), "ac      X");
    assert_eq!(r.scr().bells, 1);
    r.out(b"\x0b1\x0c2\n3\r4");
    assert_eq!(
        lines(&r),
        ["ac      X", "         1", "          2", "4          3"]
    );
    // A tab stops at the last column.
    r.out(b"\t\t\t\t");
    assert_eq!(pos(&r), (3, 19));
    // CAN cancels an escape sequence.
    r.out(b"\r\x1b[\x18A");
    assert_eq!(r.scr().line(3), "A          3");
}

#[test]
fn cursor_movement() {
    let mut r = rig(10, 40);
    r.out(b"\x1b[3;5H");
    assert_eq!(pos(&r), (2, 4));
    r.out(b"\x1b[2A\x1b[10C");
    assert_eq!(pos(&r), (0, 14));
    r.out(b"\x1b[B\x1b[100D");
    assert_eq!(pos(&r), (1, 0));
    r.out(b"\x1b[99;99f");
    assert_eq!(pos(&r), (9, 39));
    r.out(b"\x1b[7G\x1b[2d");
    assert_eq!(pos(&r), (1, 6));
    r.out(b"\x1b[H");
    assert_eq!(pos(&r), (0, 0));
    // Saved cursor: DECSC/DECRC and CSI s/u.
    r.out(b"\x1b[5;6H\x1b7\x1b[H\x1b8");
    assert_eq!(pos(&r), (4, 5));
    r.out(b"\x1b[s\x1b[H\x1b[u");
    assert_eq!(pos(&r), (4, 5));
    // RI at the top scrolls down; IND and NEL go down.
    r.out(b"\x1b[Htop\x1bM");
    assert_eq!(
        (r.scr().line(0), r.scr().line(1)),
        ("".into(), "top".into())
    );
    r.out(b"\x1bD\x1bE");
    assert_eq!(pos(&r), (2, 0));
}

#[test]
fn erase_in_display_and_line() {
    let mut r = rig(4, 6);
    let fill = b"\x1b[Haaaaaa\x1b[2;1Hbbbbbb\x1b[3;1Hcccccc\x1b[4;1Hdddddd";
    r.out(fill);
    r.out(b"\x1b[2;3H\x1b[K");
    assert_eq!(lines(&r), ["aaaaaa", "bb", "cccccc", "dddddd"]);
    r.out(b"\x1b[3;3H\x1b[1K");
    assert_eq!(r.scr().line(2), "   ccc");
    r.out(b"\x1b[4;1H\x1b[2K");
    assert_eq!(r.scr().line(3), "");
    r.out(fill);
    r.out(b"\x1b[2;3H\x1b[J");
    assert_eq!(lines(&r), ["aaaaaa", "bb", "", ""]);
    r.out(fill);
    r.out(b"\x1b[2;3H\x1b[1J");
    assert_eq!(lines(&r), ["", "   bbb", "cccccc", "dddddd"]);
    r.out(b"\x1b[2J");
    assert_eq!(lines(&r), ["", "", "", ""]);
    // ECH erases in place.
    r.out(b"\x1b[Hxxxxxx\x1b[1;2H\x1b[3X");
    assert_eq!(r.scr().line(0), "x   xx");
}

#[test]
fn sgr_attributes() {
    let mut r = rig(2, 20);
    r.out(b"\x1b[1;31;44mA\x1b[0mB\x1b[4;92mC\x1b[39;7mD\x1b[mE");
    let a = |c| r.scr().cell(0, c).1;
    assert_eq!(
        a(0),
        attr(WSCOL_RED, WSCOL_BLUE, WSATTR_HILIT | WSATTR_WSCOLORS)
    );
    assert_eq!(a(1), 0);
    assert_eq!(
        a(2),
        attr(
            WSCOL_GREEN + 8,
            WSCOL_BLACK,
            WSATTR_UNDERLINE | WSATTR_WSCOLORS
        )
    );
    assert_eq!(
        a(3),
        attr(WSCOL_WHITE, WSCOL_BLACK, WSATTR_UNDERLINE | WSATTR_REVERSE)
    );
    assert_eq!(a(4), 0);
    // The background attribute erases in the background colour, without the flags.
    r.out(b"\x1b[1;45m\x1b[2K");
    assert_eq!(r.scr().cell(0, 0).1, attr(WSCOL_WHITE, 5, WSATTR_WSCOLORS));
}

#[test]
fn sgr_replacements_on_a_poor_display() {
    // Colours but no reverse, highlight nor underline: reverse swaps the colours, bold is
    // red, underline cyan.
    let mut r = Rig::new(&WSEMUL_VT100_OPS, 2, 10, WSSCREEN_WSCOLORS, &FAKE_EMULOPS);
    r.out(b"\x1b[7mR\x1b[0;1mB\x1b[0;4mU");
    assert_eq!(
        r.scr().cell(0, 0).1,
        attr(WSCOL_BLACK, WSCOL_WHITE, WSATTR_WSCOLORS)
    );
    assert_eq!(
        r.scr().cell(0, 1).1,
        attr(WSCOL_RED, WSCOL_BLACK, WSATTR_WSCOLORS)
    );
    assert_eq!(
        r.scr().cell(0, 2).1,
        attr(WSCOL_CYAN, WSCOL_BLACK, WSATTR_WSCOLORS)
    );
}

#[test]
fn scrolling_region() {
    let mut r = rig(5, 4);
    r.out(b"\x1b[H0\x1b[2;1H1\x1b[3;1H2\x1b[4;1H3\x1b[5;1H4");
    // Rows 2 to 4 (1-based); the cursor homes.
    r.out(b"\x1b[2;4r");
    assert_eq!(pos(&r), (0, 0));
    assert_eq!((edp(&r).scrreg_startrow, edp(&r).scrreg_nrows), (1, 3));
    r.out(b"\x1b[4;1H\nx");
    assert_eq!(lines(&r), ["0", "2", "3", "x", "4"]);
    // Reverse index at the region's top scrolls the region down.
    r.out(b"\x1b[2;1H\x1bM");
    assert_eq!(lines(&r), ["0", "", "2", "3", "4"]);
    // Origin mode: CUP is relative to the region.
    r.out(b"\x1b[?6h\x1b[1;1Hy\x1b[6n");
    assert_eq!(r.scr().line(1), "y");
    assert_eq!(r.scr().input, b"\x1b[1;2R");
    // A region smaller than two lines is refused.
    r.out(b"\x1b[3;3r");
    assert_eq!((edp(&r).scrreg_startrow, edp(&r).scrreg_nrows), (1, 3));
    r.out(b"\x1b[r");
    assert_eq!((edp(&r).scrreg_startrow, edp(&r).scrreg_nrows), (0, 5));
}

#[test]
fn insert_and_delete_lines_and_characters() {
    let mut r = rig(4, 6);
    r.out(b"\x1b[H0\x1b[2;1H1\x1b[3;1H2\x1b[4;1H3");
    r.out(b"\x1b[2;1H\x1b[2L");
    assert_eq!(lines(&r), ["0", "", "", "1"]);
    r.out(b"\x1b[M");
    assert_eq!(lines(&r), ["0", "", "1", ""]);
    r.out(b"\x1b[Habcdef\x1b[1;2H\x1b[2@");
    assert_eq!(r.scr().line(0), "a  bcd");
    r.out(b"\x1b[3P");
    assert_eq!(r.scr().line(0), "acd");
    // Insert mode shifts the rest of the line right.
    r.out(b"\x1b[4hXY\x1b[4l");
    assert_eq!(r.scr().line(0), "aXYcd");
    // Scroll up and down (SU, SD) of the whole region.
    r.out(b"\x1b[S");
    assert_eq!(lines(&r), ["", "1", "", ""]);
    r.out(b"\x1b[2T");
    assert_eq!(lines(&r), ["", "", "", "1"]);
}

#[test]
fn character_sets() {
    let mut r = rig(2, 20);
    // DEC special graphics in G0, back to ASCII.
    r.out(b"\x1b(0qx`\x1b(Bq");
    let row: Vec<u32> = (0..4).map(|c| r.scr().cell(0, c).0).collect();
    assert_eq!(row, [0x2500, 0x2502, 0x25c6, u32::from(b'q')]);
    // DEC technical in G1, invoked with SO; SI returns to G0.
    r.out(b"\r\x1b)>\x0e\x7b\x0fa");
    assert_eq!(
        (r.scr().cell(0, 0).0, r.scr().cell(0, 1).0),
        (0x2190, u32::from(b'a'))
    );
    // Single shift 2 of G2 (ISO Latin-1 by default) for one character.
    r.out(b"\r\x1bNIJ");
    assert_eq!(
        (r.scr().cell(0, 0).0, r.scr().cell(0, 1).0),
        (0xc9, u32::from(b'J'))
    );
    // A national replacement set (german) in G1 replaces `[`.
    r.out(b"\r\x1b-K\x1b)A\x0e[\x0f[");
    assert_eq!(
        (r.scr().cell(0, 0).0, r.scr().cell(0, 1).0),
        (0xc4, u32::from(b'['))
    );
    // 8-bit bytes go through GR (G2): é in ISO Latin-1.
    r.out(b"\r\xe9");
    assert_eq!(r.scr().cell(0, 0).0, 0xe9);
}

#[test]
fn utf8_mode() {
    let mut r = rig(2, 20);
    r.out("\x1b%Gé€😀".as_bytes());
    let row: Vec<u32> = (0..3).map(|c| r.scr().cell(0, c).0).collect();
    assert_eq!(row, [0xe9, 0x20ac, 0x1f600]);
    // As in C, the bytes of a sequence a write leaves incomplete are not counted as
    // processed, although the input state took them: the tty layer sends them again, and
    // that first resend is ill-formed (dropped) before the next one draws the character.
    let euro = "€".as_bytes();
    assert_eq!(r.out(&euro[..1]), 0);
    assert_eq!(edp(&r).instate.mbleft, 2);
    assert_eq!(r.out(euro), 0);
    assert_eq!(r.out(euro), 3);
    assert_eq!(r.scr().cell(0, 3).0, 0x20ac);
    // ESC % @ leaves UTF-8 mode: the bytes are Latin-1 again.
    r.out(b"\x1b%@\xc3\xa9");
    assert_eq!((r.scr().cell(0, 4).0, r.scr().cell(0, 5).0), (0xc3, 0xa9));
}

#[test]
fn replies_to_the_host() {
    let mut r = rig(5, 20);
    r.out(b"\x1b[c");
    assert_eq!(r.scr().input, WSEMUL_VT_ID1);
    r.scr_mut().input.clear();
    r.out(b"\x1b[>c");
    assert_eq!(r.scr().input, WSEMUL_VT_ID2);
    r.scr_mut().input.clear();
    // CSI ? 15 n is not one the C answers.
    r.out(b"\x1b[3;7H\x1b[6n\x1b[5n\x1b[15n\x1b[?15n");
    assert_eq!(r.scr().input, b"\x1b[3;7R\x1b[0n\x1b[?13n");
}

#[test]
fn tab_stops() {
    let mut r = rig(2, 30);
    r.out(b"\t");
    assert_eq!(pos(&r), (0, 8));
    // Clear all, set one at column 4, clear the one under the cursor.
    r.out(b"\x1b[3g\r\x1b[4C\x1bH\r\t");
    assert_eq!(pos(&r), (0, 4));
    r.out(b"\x1b[g\r\t");
    assert_eq!(pos(&r), (0, 29));
    // Restore stops at columns 3 and 10 (1-based) through DCS, then report them.
    r.out(b"\x1bP2$t3/10\x1b\\\r\t");
    assert_eq!(pos(&r), (0, 2));
    r.out(b"\x1b[2$w");
    assert_eq!(r.scr().input, b"\x1bP2$u3/10\x1b\\");
    // Backward tab.
    r.out(b"\x1b[20G\x1b[Z");
    assert_eq!(pos(&r), (0, 9));
}

#[test]
fn double_width_alignment_and_repeat() {
    let mut r = rig(3, 8);
    r.out(b"abcd\x1b#6");
    assert_eq!(r.scr().row(0), "a b c d ");
    assert_eq!(pos(&r), (0, 3));
    r.out(b"\x1b#5");
    assert_eq!(r.scr().line(0), "abcd");
    r.out(b"\x1b#8");
    assert_eq!(lines(&r), ["EEEEEEEE", "EEEEEEEE", "EEEEEEEE"]);
    assert_eq!(pos(&r), (0, 0));
    r.out(b"\x1b[2Jx\x1b[3b");
    assert_eq!(r.scr().line(0), "xxxx");
}

#[test]
fn osc_strings_are_swallowed() {
    let mut r = rig(2, 20);
    r.out(b"a\x1b]0;title\x07b\x1b]2;x\x1b\\c");
    assert_eq!(r.scr().line(0), "abc");
    assert_eq!(r.scr().bells, 0);
}

/// Every emulop of a run may fail once: the tty layer's retries (`out_all`) must end on
/// the screen a run without failures draws.
#[test]
fn any_failed_emulop_is_resumed() {
    let data: &[u8] = b"hello\r\n\x1b[1;31mworld\x1b[0m\r\nthird\n\nx\x1b[2;3H\x1b[K\x1b[L\
        \x1b[2@ins\x1b[P\x1b[1;2r\x1b[2;1H\n\n\x1b[r\x1b[5;1H\r\n\r\n\r\nend\x1b#6!";
    let reference = {
        let mut r = rig(5, 9);
        r.out_all(data);
        (
            r.scr().cells.clone(),
            pos(&r),
            r.scr().cursor,
            r.scr().calls,
        )
    };
    for k in 0..reference.3 {
        let mut r = rig(5, 9);
        r.scr_mut().fail_at = Some(r.scr().calls + k);
        r.out_all(data);
        assert_eq!(r.scr().fail_at, None, "call {k} not reached");
        assert!(
            r.scr().cells == reference.0,
            "screen after failing call {k}"
        );
        assert_eq!(
            (pos(&r), r.scr().cursor),
            (reference.1, reference.2),
            "call {k}"
        );
    }
}

#[test]
fn console_kernel_output_and_resets() {
    let mut scr = FakeScreen::new(3, 10);
    let cookie: *mut c_void = (&mut *scr as *mut FakeScreen).cast();
    let d = descr(3, 10, ALLCAPS, &FAKE_EMULOPS);
    let kattr = attr(WSCOL_WHITE, WSCOL_BLUE, WSATTR_WSCOLORS);
    // SAFETY: only this test uses the console state; the screen outlives it.
    unsafe {
        let edp = wsemul_vt100_cnattach(&d, cookie, 0, 1, 0);
        // Before the real attach: no tables, no callbacks.
        assert_eq!(wsemul_vt100_output(edp, b"k\x1b[c", true), 4);
        assert_eq!(
            wsemul_vt100_attach(true, None, ptr::null_mut(), 0, 0, cookie, 0),
            edp
        );
        assert_eq!(wsemul_vt100_output(edp, b"\x1b(0q", false), 4);
        wsemul_vt100_resetop(edp, WSEMUL_CLEARCURSOR);
        let (mut row, mut col) = (0, 0);
        wsemul_vt100_detach(edp, &mut row, &mut col);
        assert_eq!((row, col), (1, 4));
    }
    // Kernel output is drawn with the kernel attribute and ignores escapes.
    assert!(scr.log.contains(&Op::Putchar(1, 0, u32::from(b'k'), kattr)));
    assert!(scr.log.contains(&Op::Putchar(1, 1, u32::from(b'['), kattr)));
    assert!(scr.input.is_empty());
    // After attach the tables exist: DEC graphics work.
    assert!(scr.log.contains(&Op::Putchar(1, 3, 0x2500, 0)));
    assert_eq!(scr.cursor, None);
    let mut r = rig(2, 4);
    r.out(b"ab");
    // SAFETY: the rig's state, not in use.
    unsafe { wsemul_vt100_resetop(r.edp, WSEMUL_CLEARSCREEN) };
    assert_eq!(
        (lines(&r), pos(&r), r.scr().cursor),
        (std::vec!["".into(), "".into()], (0, 0), Some((0, 0)))
    );
}
