//! Host tests of `vga.c`: attributes, character mapping and font selection, screens and
//! their backing stores, scrollback arithmetic, fonts and ioctls. The host's bus space
//! reads zeros (so `vga_init` sees a monochrome adapter) and drops writes.

use std::boxed::Box;
use std::vec;

use super::*;
use crate::machine::bus::BusSpaceTag;

/// A configuration as `vga_init` makes it, monochrome or (with its handle reused) colour,
/// in a box that stays in place.
fn config(mono: bool) -> Box<VgaConfig> {
    let t = BusSpaceTag::default();
    let mut hdl = vga_init(t, t).hdl;
    assert_eq!(hdl.vh_mono, 1); // the host reads 0 from the misc output register
    hdl.vh_mono = i32::from(mono);
    let vc = Box::new(VgaConfig::new(hdl));
    vc.currenttype.set(if mono {
        &VGA_STDSCREEN_MONO.0
    } else {
        &VGA_STDSCREEN.0
    });
    vc.vc_fonts[0].set(&VGA_BUILTINFONT);
    vc
}

fn cookie<T>(x: &T) -> *mut c_void {
    ptr::from_ref(x).cast_mut().cast()
}

/// A screen of `type_` on `vc` (not on the configuration's list: a test's own).
fn screen(vc: &VgaConfig, type_: &WsscreenDescr) -> Box<Vgascreen> {
    let scr = Box::new(Vgascreen::new());
    scr.cfg.set(vc);
    scr.pcs.hdl.set(&vc.hdl.vh_ph);
    scr.pcs.type_.set(type_);
    scr
}

#[test]
fn colour_attributes_pack_and_unpack() {
    let vc = config(false);
    let scr = screen(&vc, &VGA_STDSCREEN.0);
    let id = cookie(&*scr);
    // SAFETY: `id` is a vga screen in all the calls below.
    unsafe {
        assert_eq!(vga_pack_attr(id, 0, 0, 0), Ok(7));
        let a = vga_pack_attr(id, WSCOL_RED, WSCOL_BLUE, WSATTR_WSCOLORS).unwrap();
        assert_eq!(a, FG_RED | BG_BLUE);
        assert_eq!(vga_unpack_attr(id, a), (WSCOL_RED, WSCOL_BLUE, 0));
        let a = vga_pack_attr(id, WSCOL_GREEN, 0, WSATTR_WSCOLORS | WSATTR_HILIT).unwrap();
        assert_eq!(vga_unpack_attr(id, a), (WSCOL_GREEN + 8, WSCOL_BLACK, 0));
        assert_eq!(vga_pack_attr(id, 0, 0, WSATTR_BLINK), Ok(7 | FG_BLINK));
        assert_eq!(vga_pack_attr(id, 0, 0, WSATTR_REVERSE), Err(Errno::EINVAL));
    }
}

#[test]
fn mono_attributes_pack_and_unpack() {
    let vc = config(true);
    let scr = screen(&vc, &VGA_STDSCREEN_MONO.0);
    let id = cookie(&*scr);
    // SAFETY: `id` is a vga screen in all the calls below.
    unsafe {
        assert_eq!(vga_pack_attr(id, 0, 0, WSATTR_WSCOLORS), Err(Errno::EINVAL));
        assert_eq!(vga_pack_attr(id, 0, 0, WSATTR_REVERSE), Ok(0x70));
        let a = vga_pack_attr(id, 0, 0, WSATTR_UNDERLINE | WSATTR_HILIT).unwrap();
        assert_eq!(a, 0x07 | FG_UNDERLINE | FG_INTENSE);
        assert_eq!(vga_unpack_attr(id, 0x70), (WSCOL_BLACK, WSCOL_WHITE, 0));
        assert_eq!(vga_unpack_attr(id, 0x01), (WSCOL_BLACK, WSCOL_BLACK, 1));
    }
}

#[test]
fn fonts_are_selected_by_height_and_name_and_map_characters() {
    let vc = config(false);
    let scr = screen(&vc, &VGA_STDSCREEN.0);
    assert_eq!(vga_selectfont(&vc, &scr, None, None), Ok(()));
    assert!(ptr::eq(scr.fontset1.get(), &VGA_BUILTINFONT));
    assert!(scr.fontset2().is_none());
    // no 10-line font for the 80x40 screens
    let scr40 = screen(&vc, &VGA_40LSCREEN.0);
    assert_eq!(vga_selectfont(&vc, &scr40, None, None), Err(Errno::ENXIO));
    // a second font only on a screen without highlighting
    let iso = Box::new(Vgafont {
        name: font_name(b"iso"),
        height: 16,
        encoding: WSDISPLAY_FONTENC_ISO,
        slot: 1,
        fontdata: ptr::null(),
    });
    vc.vc_fonts[1].set(&*iso);
    assert_eq!(
        vga_selectfont(&vc, &scr, None, Some(b"iso\0")),
        Err(Errno::ENXIO)
    );
    let bf = screen(&vc, &VGA_STDSCREEN_BF.0);
    assert_eq!(
        vga_selectfont(&vc, &bf, Some(b"builtin"), Some(b"iso")),
        Ok(())
    );
    assert!(ptr::eq(bf.fontset2.get(), &*iso));

    let mut i = 0;
    // SAFETY: vga screens.
    unsafe {
        // the builtin (IBM) font maps 0xe9 to its CP437 glyph; the ISO one as is, and as
        // well, so the second font wins with attribute bit 3
        assert_eq!(vga_mapchar(cookie(&*scr), 0xe9, &mut i), 5);
        assert_eq!(i, 0x82);
        assert_eq!(vga_mapchar(cookie(&*bf), 0xe9, &mut i), 5);
        assert_eq!(i, 0xe9 | 0x0800);
        // only the IBM font has a box drawing glyph
        assert_eq!(vga_mapchar(cookie(&*bf), 0x2500, &mut i), 5);
        assert_eq!(i, 0xc4);
    }
}

#[test]
fn second_screen_gets_backing_store_and_free_returns_it() {
    let _g = crate::kern::subr_pool::tests::setup_real_memory();
    let vc = config(false);
    let v = cookie(&*vc);
    let (mut c1, mut c2) = (ptr::null_mut(), ptr::null_mut());
    let (mut x, mut y, mut attr) = (0, 0, 0);
    // SAFETY: `v` is a configuration and the types are its descriptors.
    unsafe {
        vga_alloc_screen(v, &VGA_STDSCREEN.0, &mut c1, &mut x, &mut y, &mut attr).unwrap();
        let s1 = &*c1.cast::<Vgascreen>();
        assert_eq!(s1.pcs.active.get(), 1);
        assert!(s1.pcs.mem.get().is_null());
        assert!(ptr::eq(vc.active.get(), s1));
        assert_eq!(attr, 7);

        vga_alloc_screen(v, &VGA_STDSCREEN.0, &mut c2, &mut x, &mut y, &mut attr).unwrap();
        let s2 = &*c2.cast::<Vgascreen>();
        assert_eq!(vc.nscreens.get(), 2);
        assert_eq!(s2.pcs.active.get(), 0);
        assert_eq!(s1.pcs.mem().len(), 80 * 25);
        assert!(s2.pcs.mem().iter().all(|c| c.get() == 0x0720));

        // the inactive screen scrolls in its backing store
        s2.pcs.mem()[80].set(0x0741);
        vga_copyrows(c2, 1, 0, 24).unwrap();
        assert_eq!(s2.pcs.mem()[0].get(), 0x0741);

        vga_free_screen(v, c2);
        assert_eq!(vc.nscreens.get(), 1);
        assert!(s1.pcs.mem.get().is_null());
        assert!(ptr::eq(vc.screens.first().unwrap(), s1));
        assert!(vga_getchar(v, 0, 0).is_some());
    }
}

#[test]
fn scrollback_moves_the_visible_offset_within_the_text_memory() {
    let vc = config(false);
    let scr = screen(&vc, &VGA_STDSCREEN.0);
    scr.maxdispoffset.set(0x8000 - 80 * 25 * 2);
    scr.pcs.dispoffset.set(160 * 30);
    scr.pcs.visibleoffset.set(160 * 30);
    // SAFETY: a configuration and its screen.
    unsafe {
        vga_scrollback(cookie(&*vc), cookie(&*scr), -10);
        assert_eq!(scr.pcs.visibleoffset.get(), 160 * 20);
        // never past the start of the memory
        vga_scrollback(cookie(&*vc), cookie(&*scr), -100);
        assert_eq!(scr.pcs.visibleoffset.get(), 0);
        // nor past the bottom
        vga_scrollback(cookie(&*vc), cookie(&*scr), 100);
        assert_eq!(scr.pcs.visibleoffset.get(), 160 * 30);
        vga_scrollback(cookie(&*vc), cookie(&*scr), -1);
        vga_scrollback(cookie(&*vc), cookie(&*scr), 0);
        assert_eq!(scr.pcs.visibleoffset.get(), scr.pcs.dispoffset.get());
    }
}

#[test]
fn fonts_load_into_free_slots_and_list() {
    let _g = crate::kern::subr_pool::tests::setup_real_memory();
    let vc = config(false);
    let v = cookie(&*vc);
    let glyphs = vec![0u8; 256 * 8].leak();
    let mut f = WsdisplayFont::zeroed();
    f.name[..4].copy_from_slice(b"tiny");
    f.index = -1;
    f.numchars = 256;
    f.fontwidth = 8;
    f.fontheight = 8;
    f.stride = 1;
    f.encoding = WSDISPLAY_FONTENC_IBM;
    f.data = glyphs.as_mut_ptr().cast();
    // SAFETY: `v` is a configuration; the glyphs are leaked, kept for good.
    unsafe {
        assert_eq!(vga_load_font(v, ptr::null_mut(), &mut f), Ok(()));
        assert_eq!(f.index, 1);
        f.index = 1;
        assert_eq!(
            vga_load_font(v, ptr::null_mut(), &mut f),
            Err(Errno::EEXIST)
        );
        f.stride = 2;
        assert_eq!(
            vga_load_font(v, ptr::null_mut(), &mut f),
            Err(Errno::EINVAL)
        );

        let mut l = WsdisplayFont::zeroed();
        l.index = 1;
        assert_eq!(vga_list_font(v, &mut l), Ok(()));
        assert_eq!(cstr(&l.name), b"tiny");
        assert_eq!((l.fontheight, l.numchars, l.stride), (8, 256, 1));
        l.index = 2;
        assert_eq!(vga_list_font(v, &mut l), Err(Errno::EINVAL));

        // select it by name for an 80x50 screen (8 lines)
        let scr = screen(&vc, &VGA_50LSCREEN_BF.0);
        let mut u = WsdisplayFont::zeroed();
        u.name[..7].copy_from_slice(b"tiny,xx");
        assert_eq!(vga_load_font(v, cookie(&*scr), &mut u), Err(Errno::ENXIO));
        u.name = [0; WSFONT_NAME_SIZE];
        u.name[..4].copy_from_slice(b"tiny");
        assert_eq!(vga_load_font(v, cookie(&*scr), &mut u), Ok(()));
        assert_eq!(scr.fontset1().map(|f| f.slot), Some(1));
    }
}

#[test]
fn ioctls_of_the_generic_vga() {
    let vc = config(false);
    vc.vc_type.set(WSDISPLAY_TYPE_PCIVGA as i32);
    let v = cookie(&*vc);
    let mut data = [0u8; 4];
    // SAFETY: `v` is a configuration; `data` holds an int.
    unsafe {
        assert_eq!(
            vga_ioctl(v, WSDISPLAYIO_GTYPE, &mut data, 0, None),
            Ok(true)
        );
        assert_eq!(u32::from_ne_bytes(data), WSDISPLAY_TYPE_PCIVGA);
        assert_eq!(
            vga_ioctl(
                v,
                crate::dev::wscons::wsconsio::WSDISPLAYIO_GINFO,
                &mut data,
                0,
                None
            ),
            Err(Errno::ENOTTY)
        );
        assert_eq!(vga_mmap(v, 0, 0), None);
        assert!(vga_getchar(v, 0, 0).is_none());
    }
    assert!(!vga_is_console(
        BusSpaceTag::default(),
        WSDISPLAY_TYPE_PCIVGA as i32
    ));
}
