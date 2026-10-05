use super::*;

use crate::dev::wscons::wsconsio::WSDISPLAY_FONTORDER_L2R;

#[test]
fn builtin_fonts_are_found() {
    wsfont_init();
    wsfont_init(); // idempotent

    assert_eq!(wsfont_find(None, 8, 16, 0), Some(4));
    assert_eq!(wsfont_find(None, 12, 0, 0), Some(6));
    assert_eq!(wsfont_find(None, 16, 0, 0), Some(7));
    assert_eq!(wsfont_find(None, 32, 0, 0), Some(8));
    assert_eq!(
        wsfont_find(None, 0, 0, 0),
        Some(4),
        "the first font of the list"
    );
    assert_eq!(wsfont_find(Some(b"Spleen 12x24"), 0, 0, 2), Some(6));
    assert_eq!(wsfont_find(Some(b"Spleen 12x24"), 8, 0, 0), None);
    assert_eq!(wsfont_find(None, 5, 8, 0), None);

    let mut names = std::vec::Vec::new();
    wsfont_enum(|f| {
        names.push(f.name().to_vec());
        false
    });
    assert!(names.starts_with(&[b"Spleen 8x16".to_vec(), b"Spleen 12x24".to_vec()]));
}

#[test]
fn lock_and_unlock() {
    wsfont_init();

    let (lc, font) = wsfont_lock(4, WSDISPLAY_FONTORDER_L2R, WSDISPLAY_FONTORDER_L2R)
        .expect("spleen 8x16 locks");
    assert!(lc >= 1);
    // SAFETY: a locked built-in font.
    let font = unsafe { font.as_ref() };
    assert_eq!((font.fontwidth, font.fontheight, font.stride), (8, 16, 1));
    assert_eq!(font.firstchar, 32);
    assert_eq!(font.numchars, 224);
    // 'A' is glyph 33; its fourth row has its two top pixels in the middle.
    // SAFETY: the glyphs are numchars * fontheight bytes.
    let glyphs = unsafe { core::slice::from_raw_parts(font.data.cast::<u8>(), 224 * 16) };
    assert_ne!(glyphs[33 * 16..34 * 16].iter().fold(0, |a, &b| a | b), 0);
    assert_eq!(glyphs[..16], [0; 16], "the space is blank");

    assert_eq!(wsfont_unlock(4), Ok(lc - 1));
    assert_eq!(wsfont_lock(99, 0, 0).err(), Some(Errno::ENOENT));
    assert_eq!(wsfont_unlock(99), Err(Errno::ENOENT));
}

#[test]
fn unicode_maps() {
    let mut font = WsdisplayFont::zeroed();
    font.encoding = WSDISPLAY_FONTENC_ISO;
    assert_eq!(wsfont_map_unichar(&font, 0x263a), Some(0x263a));

    font.encoding = WSDISPLAY_FONTENC_IBM;
    assert_eq!(wsfont_map_unichar(&font, 'A' as i32), Some(65));
    assert_eq!(wsfont_map_unichar(&font, 0xe9), Some(130)); // e acute
    assert_eq!(wsfont_map_unichar(&font, 0x2500), Some(196)); // box drawing
    assert_eq!(wsfont_map_unichar(&font, 0x3a3), Some(228)); // capital sigma
    assert_eq!(wsfont_map_unichar(&font, 0), Some(0), "lo == 0 maps to 0");
    assert_eq!(wsfont_map_unichar(&font, 0x80), None);
    assert_eq!(wsfont_map_unichar(&font, 0x263a), None);

    font.encoding = 7;
    assert_eq!(wsfont_map_unichar(&font, 'A' as i32), None);
}

#[test]
fn rotation_turns_rows_into_columns() {
    let mut font = WsdisplayFont::zeroed();
    font.numchars = 1;
    font.fontwidth = 8;
    font.fontheight = 2;
    font.stride = 1;
    // Row 0 has the leftmost pixel set, row 1 the rightmost.
    let glyphs = [0x80u8, 0x01];
    let newstride = 1;
    let mut cw = [0u8; 8];
    wsfont_rotate_cw(&font, &glyphs, &mut cw, newstride);
    // Clockwise: column b of the original becomes row b, row r becomes column (h - 1 - r).
    assert_eq!(cw[0], 0x01);
    assert_eq!(cw[7], 0x02);
    let mut ccw = [0u8; 8];
    wsfont_rotate_ccw(&font, &glyphs, &mut ccw, newstride);
    assert_eq!(ccw[7], 0x80);
    assert_eq!(ccw[0], 0x40);
}

#[test]
fn bit_reversal_table() {
    assert_eq!(REVERSE[0x01], 0x80);
    assert_eq!(REVERSE[0xc0], 0x03);
    assert_eq!(REVERSE[0xff], 0xff);
}
