use super::*;

use std::boxed::Box;
use std::cell::RefCell;
use std::format;
use std::vec;
use std::vec::Vec;

use crate::dev::wscons::wsdisplayvar::WSSCREEN_WSCOLORS;

/// The columns of the fake display's grid.
const COLS: i32 = 16;
/// The rows of the fake display's grid.
const ROWS: i32 = 4;

/// A display that keeps its one grid of cells in memory: the access cookie and every
/// screen's emulops cookie.
struct FakeDisplay {
    /// The cells, row by row.
    cells: RefCell<Vec<WsdisplayCharcell>>,
    /// The screen cookie `show_screen` was last called with.
    shown: Cell<*mut c_void>,
}

/// The fake display behind a cookie.
fn disp<'a>(v: *mut c_void) -> &'a FakeDisplay {
    // SAFETY: the tests hand out a leaked `FakeDisplay` as every cookie.
    unsafe { &*v.cast::<FakeDisplay>() }
}

/// `getchar`.
unsafe fn fake_getchar(v: *mut c_void, row: i32, col: i32) -> Option<WsdisplayCharcell> {
    if !(0..ROWS).contains(&row) || !(0..COLS).contains(&col) {
        return None;
    }
    Some(disp(v).cells.borrow()[(row * COLS + col) as usize])
}

/// `show_screen`.
unsafe fn fake_show_screen(
    v: *mut c_void,
    cookie: *mut c_void,
    _waitok: i32,
    _cb: Option<ShowScreenCb>,
    _cbarg: *mut c_void,
) -> Result<(), Errno> {
    disp(v).shown.set(cookie);
    Ok(())
}

/// `putchar`: the screen cookie is the display too.
unsafe fn fake_putchar(
    c: *mut c_void,
    row: i32,
    col: i32,
    uc: u32,
    attr: u32,
) -> Result<(), Errno> {
    disp(c).cells.borrow_mut()[(row * COLS + col) as usize] = WsdisplayCharcell { uc, attr };
    Ok(())
}

/// `pack_attr`: `fg << 8 | bg`, the flags above.
unsafe fn fake_pack_attr(_c: *mut c_void, fg: i32, bg: i32, flags: i32) -> Result<u32, Errno> {
    Ok(((flags as u32) << 16) | ((fg as u32) << 8) | bg as u32)
}

/// `unpack_attr`.
unsafe fn fake_unpack_attr(_c: *mut c_void, attr: u32) -> (i32, i32, i32) {
    (((attr >> 8) & 0xff) as i32, (attr & 0xff) as i32, 0)
}

static FAKE_ACCESSOPS: WsdisplayAccessops = WsdisplayAccessops {
    getchar: Some(fake_getchar),
    show_screen: Some(fake_show_screen),
    ..WsdisplayAccessops::EMPTY
};

static FAKE_EMULOPS: WsdisplayEmulops = WsdisplayEmulops {
    putchar: Some(fake_putchar),
    pack_attr: Some(fake_pack_attr),
    unpack_attr: Some(fake_unpack_attr),
    ..WsdisplayEmulops::EMPTY
};

/// A display with `n` screens (indices 0 .. n), screen 0 focused, all drawing on one fake
/// grid; nothing is ever freed.
struct Fixture {
    sc: &'static WsdisplaySoftc,
    disp: &'static FakeDisplay,
}

impl Fixture {
    fn new(n: i32) -> Self {
        let disp: &'static FakeDisplay = Box::leak(Box::new(FakeDisplay {
            cells: RefCell::new(vec![
                WsdisplayCharcell {
                    uc: u32::from(b' '),
                    attr: 0x0700,
                };
                (COLS * ROWS) as usize
            ]),
            shown: Cell::new(ptr::null_mut()),
        }));
        let cookie: *mut c_void = ptr::from_ref(disp).cast_mut().cast();
        let mut descr = WsscreenDescr::new(b"std");
        descr.ncols = COLS;
        descr.nrows = ROWS;
        descr.textops = &FAKE_EMULOPS;
        descr.capabilities = WSSCREEN_WSCOLORS;
        let descr: &'static WsscreenDescr = Box::leak(Box::new(descr));
        let screens: &'static [*const WsscreenDescr; 1] =
            Box::leak(Box::new([ptr::from_ref(descr)]));
        let list: &'static WsscreenList = Box::leak(Box::new(WsscreenList {
            nscreens: 1,
            screens: screens.as_ptr(),
        }));

        // SAFETY: a softc is valid as all-zero bits (its `Softc` contract).
        let sc: &'static WsdisplaySoftc = Box::leak(Box::new(unsafe { core::mem::zeroed() }));
        sc.sc_accessops.set(Some(&FAKE_ACCESSOPS));
        sc.sc_accesscookie.set(cookie);
        sc.sc_scrdata.set(list);
        sc.sc_resumescreen.set(WSDISPLAY_NULLSCREEN);

        for i in 0..n {
            let dconf: &'static WsscreenInternal = Box::leak(Box::new(WsscreenInternal::new()));
            dconf.emulops.set(&FAKE_EMULOPS);
            dconf.emulcookie.set(cookie);
            dconf.scrdata.set(descr);
            let scr: &'static Wsscreen = Box::leak(Box::new(Wsscreen::new()));
            scr.scr_dconf.set(dconf);
            scr.sc.set(sc);
            sc.sc_scr[i as usize].set(Some(NonNull::from(scr)));
        }
        if n > 0 {
            sc.sc_focusidx.set(0);
            sc.sc_focus.set(sc.sc_scr[0].get());
        }
        Self { sc, disp }
    }

    fn scr(&self, idx: i32) -> &'static Wsscreen {
        self.sc.scr(idx).expect("screen")
    }

    /// Writes `text` at the start of `row`.
    fn put(&self, row: i32, text: &[u8]) {
        let mut cells = self.disp.cells.borrow_mut();
        for (i, &c) in text.iter().enumerate() {
            cells[(row * COLS) as usize + i].uc = u32::from(c);
        }
    }

    /// The attribute of the cell at `pos`.
    fn attr(&self, pos: u32) -> u32 {
        self.disp.cells.borrow()[pos as usize].attr
    }

    /// A copy buffer as `allocate_copybuffer` would size it.
    fn copybuffer(&self) -> &'static mut [u8] {
        let size = ((COLS + 1) * ROWS) as usize;
        let buf: &'static mut [u8] = Vec::leak(vec![0u8; size]);
        self.sc.sc_copybuffer.set(NonNull::new(buf.as_mut_ptr()));
        self.sc.sc_copybuffer_size.set(size as u32);
        buf
    }
}

#[test]
fn minor_numbers() {
    let dev = makedev(12, wsdisplayminor(1, 3));
    assert_eq!(wsdisplayminor(1, 3), 0x103);
    assert_eq!(wsdisplayunit(dev), 1);
    assert_eq!(wsdisplayscreen(dev), 3);
    assert!(!iswsdisplayctl(dev));
    assert!(iswsdisplayctl(makedev(12, wsdisplayminor(0, 255))));
}

#[test]
fn screen_types_are_picked_by_name() {
    let f = Fixture::new(0);
    let list = f.sc.scrdata();
    let first = wsdisplay_screentype_pick(list, None).expect("default");
    assert_eq!(wsdisplay_screentype_pick(list, Some(b"")), Some(first));
    assert_eq!(
        wsdisplay_screentype_pick(list, Some(b"std\0junk")),
        Some(first)
    );
    assert_eq!(wsdisplay_screentype_pick(list, Some(b"80x25")), None);
}

#[test]
fn switching_screens_moves_the_focus() {
    let f = Fixture::new(3);
    let sc = f.sc;
    assert_eq!(wsdisplay_getactivescreen(sc), 0);

    assert_eq!(wsdisplay_switch(&sc.sc_dv, 2, 1), Ok(()));
    assert_eq!(wsdisplay_getactivescreen(sc), 2);
    assert_eq!(f.disp.shown.get(), f.scr(2).dconf().emulcookie.get());
    assert!(!sc.flag(SC_SWITCHPENDING));

    // Already there: nothing to show.
    f.disp.shown.set(ptr::null_mut());
    assert_eq!(wsdisplay_switch(&sc.sc_dv, 2, 1), Ok(()));
    assert!(f.disp.shown.get().is_null());

    assert_eq!(wsdisplay_switch(&sc.sc_dv, 5, 1), Err(Errno::ENXIO));
    assert_eq!(
        wsdisplay_switch(&sc.sc_dv, WSDISPLAY_MAXSCREEN, 1),
        Err(Errno::EINVAL)
    );
    assert_eq!(wsdisplay_switch(&sc.sc_dv, -2, 1), Err(Errno::EINVAL));

    // A switch pending elsewhere refuses another one.
    sc.set_flag(SC_SWITCHPENDING);
    assert_eq!(wsdisplay_switch(&sc.sc_dv, 0, 1), Err(Errno::EBUSY));
    sc.clr_flag(SC_SWITCHPENDING);

    // A screen in graphics mode cannot be saved (WSDISPLAY_COMPAT_USL).
    f.scr(2).scr_flags.set(SCR_GRAPHICS);
    let r = wsdisplay_switch(&sc.sc_dv, 0, 1);
    if cfg!(feature = "wsdisplay_compat_usl") {
        assert_eq!(r, Err(Errno::EBUSY));
        assert_eq!(wsdisplay_getactivescreen(sc), 2);
    } else {
        assert_eq!(r, Ok(()));
    }
}

#[test]
fn screen_state_for_the_usl_ioctls() {
    let f = Fixture::new(2);
    let sc = f.sc;
    assert_eq!(wsdisplay_maxscreenidx(sc), 11);
    assert_eq!(wsdisplay_screenstate(sc, 0), Ok(()));
    f.scr(1).scr_flags.set(SCR_OPEN);
    assert_eq!(wsdisplay_screenstate(sc, 1), Err(Errno::EBUSY));
    assert_eq!(wsdisplay_screenstate(sc, 2), Err(Errno::ENXIO));
    assert_eq!(wsdisplay_screenstate(sc, 12), Err(Errno::EINVAL));

    let mut sd = WsdisplayAddscreendata {
        idx: -1,
        ..WsdisplayAddscreendata::default()
    };
    assert_eq!(wsdisplay_getscreen(sc, &mut sd), Ok(()));
    assert_eq!(sd.idx, 0);
    assert_eq!(cstr(&sd.screentype), b"std");
    assert_eq!(cstr(&sd.emul), b"");
}

#[test]
fn display_ioctls() {
    let f = Fixture::new(1);
    let (sc, scr) = (f.sc, f.scr(0));

    let mut mode = [0u8; 4];
    assert_eq!(
        wsdisplay_internal_ioctl(sc, scr, WSDISPLAYIO_GMODE, &mut mode, 0, None),
        Ok(true)
    );
    assert_eq!(ioctl_arg::<u32>(&mode), WSDISPLAYIO_MODE_EMUL);
    scr.scr_flags.set(SCR_GRAPHICS | SCR_DUMBFB);
    let _ = wsdisplay_internal_ioctl(sc, scr, WSDISPLAYIO_GMODE, &mut mode, 0, None);
    assert_eq!(ioctl_arg::<u32>(&mode), WSDISPLAYIO_MODE_DUMBFB);

    // Setting the mode needs the descriptor open for writing.
    assert_eq!(
        wsdisplay_internal_ioctl(sc, scr, WSDISPLAYIO_SMODE, &mut mode, 0, None),
        Err(Errno::EACCES)
    );

    let mut st = [0u8; size_of::<WsdisplayScreentype>()];
    assert_eq!(
        wsdisplay_internal_ioctl(sc, scr, WSDISPLAYIO_GETSCREENTYPE, &mut st, 0, None),
        Ok(true)
    );
    let st: WsdisplayScreentype = ioctl_arg(&st);
    assert_eq!((st.nidx, st.ncols, st.nrows), (1, COLS, ROWS));
    assert_eq!(cstr(&st.name), b"std");

    let mut et = [0u8; size_of::<WsdisplayEmultype>()];
    let mut d = WsdisplayEmultype::default();
    d.idx = 100;
    ioctl_ret(&mut et, &d);
    assert_eq!(
        wsdisplay_internal_ioctl(sc, scr, WSDISPLAYIO_GETEMULTYPE, &mut et, 0, None),
        Err(Errno::EINVAL)
    );

    // Not the display's, and the fake driver has no ioctl: -1.
    assert_eq!(
        wsdisplay_internal_ioctl(sc, scr, WSDISPLAYIO_GTYPE, &mut mode, 0, None),
        Ok(false)
    );
}

#[test]
fn word_selection_is_inverted_copied_and_removed() {
    let f = Fixture::new(1);
    let scr = f.scr(0);
    let buf = f.copybuffer();
    f.put(0, b"hello world");

    scr.mouse.set(1); // on the 'e'
    mouse_copy_word(scr);
    assert_eq!((scr.cpy_start.get(), scr.cpy_end.get()), (0, 4));
    // Inverted: fg and bg swapped, with WSATTR_WSCOLORS.
    for pos in 0..5 {
        assert_eq!(
            f.attr(pos),
            ((WSATTR_WSCOLORS as u32) << 16) | 0x0007,
            "{pos}"
        );
    }
    assert_eq!(f.attr(5), 0x0700);

    mouse_copy_selection(scr);
    assert_eq!(cstr(buf), b"hello");

    mouse_copy_end(scr);
    remove_selection(scr);
    for pos in 0..5 {
        // Inverted back (the flags stay; the colours are the original ones).
        assert_eq!(f.attr(pos) & 0xffff, 0x0700, "{pos}");
    }
    assert!(!scr.mflag(SEL_EXISTS));
}

#[test]
fn line_selection_ends_with_a_carriage_return() {
    let f = Fixture::new(1);
    let scr = f.scr(0);
    let buf = f.copybuffer();
    f.put(1, b"hello world");

    scr.mouse.set(COLS as u32 + 3); // row 1
    mouse_copy_line(scr);
    assert_eq!(
        (scr.cpy_start.get(), scr.cpy_end.get()),
        (COLS as u32, 2 * COLS as u32 - 1)
    );
    mouse_copy_selection(scr);
    assert_eq!(cstr(buf), b"hello world\r");
}

#[test]
fn mouse_motion_stays_on_the_grid() {
    let f = Fixture::new(1);
    let scr = f.scr(0);
    scr.mouse.set(0);
    mouse_moverel(scr, 3, 1);
    assert_eq!(scr.mouse.get(), COLS as u32 + 3);
    assert!(scr.mflag(MOUSE_VISIBLE));
    mouse_moverel(scr, 1000, 1000);
    assert_eq!(scr.mouse.get(), (ROWS * COLS - 1) as u32);
    mouse_moverel(scr, -1000, -1000);
    assert_eq!(scr.mouse.get(), 0);
    mouse_hide(scr);
    assert!(!scr.mflag(MOUSE_VISIBLE));
}

#[test]
fn character_classes_follow_xterm() {
    assert_eq!(char_class(u32::from(b'a')), 48);
    assert_eq!(char_class(u32::from(b'7')), 48);
    assert_eq!(char_class(u32::from(b'_')), 48);
    assert_eq!(char_class(u32::from(b' ')), 32);
    assert_eq!(char_class(u32::from(b'\t')), 32);
    assert_eq!(char_class(u32::from(b'[')), 91);
    assert_eq!(char_class(0x7f), 1);
    assert_eq!(char_class(0xd7), 216);
    assert_eq!(
        char_class(0x100 | u32::from(b'a')),
        48,
        "only the low byte counts"
    );
}

#[test]
fn brightness_steps_of_five_percent() {
    let dp = |min, max, curval| WsdisplayParam {
        param: WSDISPLAYIO_PARAM_BRIGHTNESS,
        min,
        max,
        curval,
        ..WsdisplayParam::default()
    };
    assert_eq!(brightness_step(&dp(0, 100, 50), 1), 55);
    assert_eq!(brightness_step(&dp(0, 100, 98), 1), 100);
    assert_eq!(brightness_step(&dp(0, 100, 3), -1), 0);
    assert_eq!(brightness_step(&dp(0, 10, 5), 1), 6, "at least one step");
    assert_eq!(brightness_step(&dp(0, 100, 40), 0), 40);
}

#[test]
fn emulation_callbacks_ignore_the_console_before_its_attach() {
    wsdisplay_emulbell(ptr::null_mut());
    wsdisplay_emulinput(ptr::null_mut(), b"\x1b[0n");
}

#[test]
fn names_are_cut_like_strlcpy() {
    let mut dst = [0xffu8; 4];
    strlcpy_into(&mut dst, b"vt100");
    assert_eq!(&dst, b"vt1\0");
    strlcpy_into(&mut dst, b"a");
    assert_eq!(&dst, b"a\0\0\0");
}

/// The flags and constants `wsdisplay.c` and `wsmoused.h` define, and both GENERICs'
/// `WSDISPLAY_DEFAULTSCREENS`, against the C.
#[test]
#[ignore = "needs OPENBSD_SRC"]
fn constants_match_the_c_sources() {
    let defs = crate::reftest::defines("sys/dev/wscons/wsdisplay.c");
    crate::reftest::assert_defines!(defs;
        SCR_OPEN,
        SCR_WAITACTIVE,
        SCR_GRAPHICS,
        SCR_DUMBFB,
        MOUSE_VISIBLE,
        SEL_EXISTS,
        SEL_IN_PROGRESS,
        SEL_EXT_AFTER,
        BLANK_TO_EOL,
        SEL_BY_CHAR,
        SEL_BY_WORD,
        SEL_BY_LINE,
        SC_SWITCHPENDING,
        SC_PASTE_AVAIL,
    );
    let defs = crate::reftest::defines("sys/dev/wscons/wsmoused.h");
    crate::reftest::assert_defines!(defs;
        NO_BORDER,
        BORDER,
        MOUSE_COPY_BUTTON,
        MOUSE_PASTE_BUTTON,
        MOUSE_EXTEND_BUTTON,
    );
    for arch in ["amd64", "arm64"] {
        let path = crate::reftest::openbsd_src().join(format!("sys/arch/{arch}/conf/GENERIC"));
        let text = std::fs::read_to_string(path).unwrap();
        let want = format!("WSDISPLAY_DEFAULTSCREENS={WSDISPLAY_DEFAULTSCREENS}");
        assert!(
            text.lines()
                .any(|l| l.starts_with("option") && l.contains(&want)),
            "{arch}"
        );
    }
}
