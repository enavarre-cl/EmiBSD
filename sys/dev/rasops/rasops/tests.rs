use super::*;

use std::boxed::Box;
use std::vec;

use crate::dev::wscons::wsdisplayvar::{WSCOL_BLUE, WSCOL_WHITE};
use crate::kern::subr_pool::tests::setup_real_memory;

/// A frame buffer for the tests: leaked memory the descriptor points into.
pub(crate) struct TestFb {
    ptr: *mut i32,
    len: usize,
}

impl TestFb {
    /// The frame buffer's words as they are now.
    pub(crate) fn words(&self) -> &[i32] {
        // SAFETY: the leaked allocation of `len` words; read after the writes are done.
        unsafe { core::slice::from_raw_parts(self.ptr, self.len) }
    }
}

/// A descriptor for a `width` x `height` frame buffer of `depth` bits (stride = width *
/// bytes per pixel), filled with a pattern so clears are visible, configured by
/// `rasops_init` with `flags`.
pub(crate) fn test_ri_flags(
    depth: i32,
    width: i32,
    height: i32,
    flags: i32,
) -> (&'static RasopsInfo, TestFb) {
    let bpp = if depth == 15 { 16 } else { depth };
    let stride = width * bpp / 8;
    let len = (stride * height / 4) as usize;
    let fb = Box::leak(vec![0x5a5a_5a5ai32; len].into_boxed_slice());
    let ptr = fb.as_mut_ptr();
    let ri: &'static RasopsInfo = Box::leak(Box::new(RasopsInfo::new()));
    ri.ri_depth.set(depth);
    ri.ri_bits.set(ptr.cast());
    ri.ri_width.set(width);
    ri.ri_height.set(height);
    ri.ri_stride.set(stride);
    ri.ri_flg.set(flags);
    rasops_init(ri, 50, 160).expect("rasops_init");
    (ri, TestFb { ptr, len })
}

/// [`test_ri_flags`] with no flags.
pub(crate) fn test_ri(depth: i32, width: i32, height: i32) -> (&'static RasopsInfo, TestFb) {
    test_ri_flags(depth, width, height, 0)
}

#[test]
fn colormap() {
    assert_eq!(&RASOPS_CMAP[0..3], &[0, 0, 0]);
    assert_eq!(&RASOPS_CMAP[3..6], &[0x7f, 0, 0]);
    assert_eq!(&RASOPS_CMAP[7 * 3..8 * 3], &[0xc7, 0xc7, 0xc7]);
    assert_eq!(&RASOPS_CMAP[15 * 3..16 * 3], &[0xff, 0xff, 0xff]);
    assert_eq!(&RASOPS_CMAP[100 * 3..101 * 3], &[0xff, 0xff, 0xff]);
    // The last 16 are the first 16 inverted, in reverse order.
    assert_eq!(&RASOPS_CMAP[240 * 3..241 * 3], &[0, 0, 0]); // ~HILITE_WHITE
    assert_eq!(&RASOPS_CMAP[255 * 3..256 * 3], &[0xff, 0xff, 0xff]); // ~NORMAL_BLACK
    assert_eq!(&RASOPS_CMAP[247 * 3..248 * 3], &[0x80, 0x80, 0x80]); // ~HILITE_BLACK
}

#[test]
fn attributes() {
    let n = ptr::null_mut();
    // SAFETY: the attribute packers do not use the cookie.
    unsafe {
        assert_eq!(rasops_pack_cattr(n, 1, 2, 0), Ok(0x0700_0000));
        assert_eq!(rasops_pack_cattr(n, 1, 2, WSATTR_WSCOLORS), Ok(0x0102_0000));
        assert_eq!(
            rasops_pack_cattr(n, 1, 2, WSATTR_WSCOLORS | WSATTR_REVERSE | WSATTR_HILIT),
            Ok(0x0a01_0000)
        );
        assert_eq!(rasops_pack_cattr(n, 1, 2, WSATTR_BLINK), Err(Errno::EINVAL));
        assert_eq!(
            rasops_pack_mattr(n, 5, 5, WSATTR_REVERSE | WSATTR_UNDERLINE),
            Ok(0x0001_0008)
        );
        assert_eq!(rasops_pack_mattr(n, 5, 5, WSATTR_HILIT), Err(Errno::EINVAL));
        assert_eq!(rasops_unpack_attr(n, 0x0a01_0008), (10, 1, 8));
    }
}

#[test]
fn geometry_and_font_choice() {
    // 640 pixels wide: the 8-pixel font, an 80x30 grid.
    let (ri, _fb) = test_ri(32, 640, 480);
    assert_eq!((ri.font().fontwidth, ri.font().fontheight), (8, 16));
    assert_eq!((ri.ri_cols.get(), ri.ri_rows.get()), (80, 30));
    assert_eq!(ri.ri_xscale.get(), 32);
    assert_eq!(ri.ri_yscale.get(), 16 * 640 * 4);
    assert_eq!(ri.ri_flg.get() & RI_CFGDONE, RI_CFGDONE);

    // 1280x800, centred: the 12x24 font, 106 columns (wantcols 160 is cut to the width),
    // 33 rows. The emulated height is the whole screen (wantrows 50 is cut to it), so the
    // grid is not moved down: the 8 leftover lines are at the bottom.
    let (ri, _fb) = test_ri_flags(32, 1280, 800, RI_CENTER);
    assert_eq!((ri.font().fontwidth, ri.font().fontheight), (12, 24));
    assert_eq!((ri.ri_cols.get(), ri.ri_rows.get()), (106, 33));
    assert_eq!(ri.ri_emuwidth.get(), 1280);
    assert_eq!(ri.ri_xorigin.get(), 0);
    assert_eq!(ri.ri_emuheight.get(), 800);
    assert_eq!(ri.ri_yorigin.get(), 0);

    // 1000x700 centred with the 12x24 font and an 80x25 wish: 960x600, centred.
    let ri: &'static RasopsInfo = Box::leak(Box::new(RasopsInfo::new()));
    let fb = Box::leak(vec![0i32; 1000 * 700].into_boxed_slice());
    ri.ri_depth.set(32);
    ri.ri_bits.set(fb.as_mut_ptr().cast());
    ri.ri_width.set(1000);
    ri.ri_height.set(700);
    ri.ri_stride.set(4000);
    ri.ri_flg.set(RI_CENTER);
    rasops_init(ri, 25, 80).expect("rasops_init");
    assert_eq!((ri.ri_cols.get(), ri.ri_rows.get()), (80, 25));
    assert_eq!((ri.ri_xorigin.get(), ri.ri_yorigin.get()), (20, 50));

    // 1920 wide: the 16-pixel font.
    let (ri, _fb) = test_ri(32, 1920, 64 * 10);
    assert_eq!(ri.font().fontwidth, 16);

    // An unsupported depth.
    let ri: &'static RasopsInfo = Box::leak(Box::new(RasopsInfo::new()));
    let fb = Box::leak(vec![0u8; 64 * 64 * 2].into_boxed_slice());
    ri.ri_depth.set(2);
    ri.ri_bits.set(fb.as_mut_ptr());
    ri.ri_width.set(64);
    ri.ri_height.set(64);
    ri.ri_stride.set(16);
    assert_eq!(rasops_init(ri, 10, 20), Err(Errno::EINVAL));
}

#[test]
fn devcmap_follows_the_channel_layout() {
    // Red in the high byte, blue in the low one (the GOP's BGRX order).
    let ri: &'static RasopsInfo = Box::leak(Box::new(RasopsInfo::new()));
    ri.ri_depth.set(32);
    ri.ri_rnum.set(8);
    ri.ri_rpos.set(16);
    ri.ri_gnum.set(8);
    ri.ri_gpos.set(8);
    ri.ri_bnum.set(8);
    ri.ri_bpos.set(0);
    rasops_init_devcmap(ri);
    assert_eq!(ri.ri_devcmap[0].get(), 0);
    assert_eq!(ri.ri_devcmap[1].get(), 0x7f_0000);
    assert_eq!(ri.ri_devcmap[4].get(), 0x7f);
    assert_eq!(ri.ri_devcmap[15].get(), 0xff_ffff);

    // 16 bits, r5g6b5: the value is doubled into both halves of the word.
    ri.ri_depth.set(16);
    ri.ri_rnum.set(5);
    ri.ri_rpos.set(11);
    ri.ri_gnum.set(6);
    ri.ri_gpos.set(5);
    ri.ri_bnum.set(5);
    ri.ri_bpos.set(0);
    rasops_init_devcmap(ri);
    assert_eq!(ri.ri_devcmap[9].get() as u32, 0xf800_f800);

    // Monochrome.
    ri.ri_depth.set(1);
    rasops_init_devcmap(ri);
    assert_eq!(ri.ri_devcmap[0].get(), 0);
    assert_eq!(ri.ri_devcmap[3].get(), -1);
}

#[test]
fn erase_and_copy_on_the_frame_buffer() {
    let (ri, fb) = test_ri(32, 640, 480);
    let blue = ri.ri_devcmap[WSCOL_BLUE as usize].get();
    let attr = ri
        .ri_pack_attr(WSCOL_WHITE, WSCOL_BLUE, WSATTR_WSCOLORS)
        .expect("packs");
    let stride = 640;
    let cell = |w: &[i32], row: usize, col: usize| w[row * 16 * stride + col * 8];

    // Rows 2 and 3 cleared to blue; row 1 and 4 untouched.
    ri.ri_eraserows(2, 2, attr).expect("eraserows");
    let w = fb.words();
    assert_eq!(cell(w, 2, 0), blue);
    assert_eq!(w[(4 * 16 - 1) * stride + 639], blue);
    assert_eq!(cell(w, 1, 0), 0x5a5a_5a5a);
    assert_eq!(cell(w, 4, 0), 0x5a5a_5a5a);

    // Columns 5..8 of row 6.
    ri.ri_erasecols(6, 5, 3, attr).expect("erasecols");
    let w = fb.words();
    assert_eq!(cell(w, 6, 5), blue);
    assert_eq!(w[(6 * 16 + 15) * stride + 8 * 8 - 1], blue);
    assert_eq!(cell(w, 6, 4), 0x5a5a_5a5a);
    assert_eq!(cell(w, 6, 8), 0x5a5a_5a5a);

    // Row 2 (blue) copied down onto row 10, then column 5 of row 6 onto column 20.
    ri.ri_copyrows(2, 10, 1).expect("copyrows");
    ri.ri_copycols(6, 5, 20, 1).expect("copycols");
    let w = fb.words();
    assert_eq!(cell(w, 10, 0), blue);
    assert_eq!(cell(w, 6, 20), blue);
    assert_eq!(cell(w, 6, 21), 0x5a5a_5a5a);

    // The cursor inverts its cell, twice restores it.
    // SAFETY: the test descriptor.
    unsafe { rasops_cursor(ri.cookie(), 1, 10, 0) }.expect("cursor on");
    assert_eq!(cell(fb.words(), 10, 0), !blue);
    // SAFETY: as above.
    unsafe { rasops_cursor(ri.cookie(), 0, 10, 0) }.expect("cursor off");
    assert_eq!(cell(fb.words(), 10, 0), blue);
}

#[test]
fn virtual_screens_keep_a_backing_store() {
    let _g = setup_real_memory();
    let (ri, fb) = test_ri_flags(32, 640, 480, RI_VCONS | RI_WRONLY | RI_CLEAR);
    assert_eq!(ri.ri_nscreens.get(), 1);
    let scr = ri.ri_active.get().cast::<c_void>();
    assert!(!scr.is_null());
    let ops = ri.ri_ops.get();
    let putchar = ops.putchar.expect("vcons putchar");
    let copyrows = ops.copyrows.expect("vcons copyrows");
    let white = ri.ri_devcmap[7].get();

    // SAFETY: the active screen and the descriptor rasops_init made.
    unsafe {
        let attr = (ops.pack_attr.expect("pack"))(scr, 0, 0, 0).expect("packs");
        putchar(scr, 3, 4, u32::from(b'X'), attr).expect("putchar");
        let c = rasops_getchar(ri.cookie(), 3, 4).expect("a cell");
        assert_eq!(c.uc, u32::from(b'X'));
        // The glyph is on the frame buffer: some pixel of the cell is white.
        let w = fb.words();
        let lit = (0..16).any(|y| (0..8).any(|x| w[(3 * 16 + y) * 640 + 4 * 8 + x] == white));
        assert!(lit);

        // Scrolling the whole screen up a row moves the cell and fills the scrollback.
        copyrows(scr, 1, 0, 29).expect("copyrows");
        assert_eq!(
            rasops_getchar(ri.cookie(), 2, 4).map(|c| c.uc),
            Some(u32::from(b'X'))
        );
        assert_eq!(
            rasops_getchar(ri.cookie(), 3, 4).map(|c| c.uc),
            Some(u32::from(b' '))
        );

        // A second screen is not visible and draws nothing.
        let (mut cookie, mut x, mut y, mut a) = (ptr::null_mut(), 0, 0, 0);
        rasops_alloc_screen(
            ri.cookie(),
            ptr::null(),
            &mut cookie,
            &mut x,
            &mut y,
            &mut a,
        )
        .expect("second screen");
        assert_eq!(ri.ri_nscreens.get(), 2);
        let before = fb.words().to_vec();
        putchar(cookie, 0, 0, u32::from(b'Y'), a).expect("putchar");
        assert_eq!(fb.words(), &before[..]);

        // Showing it redraws from its store.
        rasops_show_screen(ri.cookie(), cookie, 0, None, ptr::null_mut()).expect("switch");
        assert_eq!(
            rasops_getchar(ri.cookie(), 0, 0).map(|c| c.uc),
            Some(u32::from(b'Y'))
        );
        rasops_show_screen(ri.cookie(), scr, 0, None, ptr::null_mut()).expect("switch back");
        rasops_free_screen(ri.cookie(), cookie);
        assert_eq!(ri.ri_nscreens.get(), 1);
    }
}

#[test]
fn mapchar_and_font_listing() {
    let (ri, _fb) = test_ri(32, 640, 480);
    let mut c = 0;
    // SAFETY: the test descriptor.
    unsafe {
        assert_eq!(rasops_mapchar(ri.cookie(), 'A' as i32, &mut c), 5);
        assert_eq!(c, 'A' as u32);
        assert_eq!(rasops_mapchar(ri.cookie(), 0x10, &mut c), 0);
        assert_eq!(c, '?' as u32);
        assert_eq!(rasops_mapchar(ri.cookie(), 0x263a, &mut c), 0);

        let mut f = WsdisplayFont::zeroed();
        f.index = 1;
        rasops_list_font(ri.cookie(), &mut f).expect("second font");
        assert_eq!(f.name(), b"Spleen 12x24");
        assert!(f.data.is_null() && f.cookie.is_null());
        f.index = 50;
        assert_eq!(rasops_list_font(ri.cookie(), &mut f), Err(Errno::EINVAL));
    }
}

#[test]
fn framebuffer_claims() {
    let _g = setup_real_memory();
    assert!(!rasops_check_framebuffer(Paddr::new(0x7000_1000)));
    rasops_claim_framebuffer(Paddr::new(0x7000_0000), Psize::new(0x10_0000), None);
    assert!(rasops_check_framebuffer(Paddr::new(0x7000_1000)));
    assert!(!rasops_check_framebuffer(Paddr::new(0x7010_0000)));
}
