use super::*;

use std::vec::Vec;

use crate::dev::rasops::rasops::tests::test_ri;

fn bytes(words: &[i32]) -> Vec<u8> {
    words.iter().flat_map(|w| w.to_le_bytes()).collect()
}

/// White on blue, underlined.
fn attr(ri: &RasopsInfo) -> u32 {
    ri.ri_pack_attr(7, 4, 0x10 | 8).expect("colours pack") // WSCOLORS|UNDERLINE
}

/// The three bytes of pixel `x` of line `y`, as the generic `putchar` stores a colour:
/// `clr >> 16`, `clr >> 8`, `clr`.
fn pixel(b: &[u8], stride: usize, x: usize, y: usize) -> [u8; 3] {
    let o = y * stride + 3 * x;
    [b[o], b[o + 1], b[o + 2]]
}

fn bgr(clr: i32) -> [u8; 3] {
    [(clr >> 16) as u8, (clr >> 8) as u8, clr as u8]
}

/// Checks the cell at (`row`, `col`) of an 'A' painted with `attr(ri)`, and, when
/// `underline`, the underline.
fn check_cell(ri: &RasopsInfo, b: &[u8], row: usize, col: usize, underline: bool) {
    let font = ri.font();
    let (fw, fh, fs) = (
        font.fontwidth as usize,
        font.fontheight as usize,
        font.stride as usize,
    );
    let stride = ri.ri_stride.get() as usize;
    let (white, blue) = (bgr(ri.ri_devcmap[7].get()), bgr(ri.ri_devcmap[4].get()));
    assert_ne!(white, blue);
    let glyph = ri.glyph(u32::from(b'A') - 32).to_vec();
    for y in 0..fh - 2 {
        for x in 0..fw {
            let set = glyph[y * fs + x / 8] & (0x80 >> (x % 8)) != 0;
            let px = pixel(b, stride, col * fw + x, row * fh + y);
            assert_eq!(px, if set { white } else { blue }, "pixel {x},{y}");
        }
    }
    if underline {
        for x in 0..fw {
            assert_eq!(
                pixel(b, stride, col * fw + x, row * fh + fh - 2),
                white,
                "underline {x}"
            );
        }
    }
}

#[test]
fn init_defaults_the_channel_layout() {
    let (ri, _fb) = test_ri(24, 640, 480);
    assert_eq!(
        (ri.ri_rnum.get(), ri.ri_gnum.get(), ri.ri_bnum.get()),
        (8, 8, 8)
    );
    assert_eq!(
        (ri.ri_rpos.get(), ri.ri_gpos.get(), ri.ri_bpos.get()),
        (0, 8, 16)
    );
}

#[test]
fn the_generic_putchar_draws_a_glyph_and_its_underline() {
    let (ri, fb) = test_ri(24, 640, 480);
    assert_eq!(ri.font().fontwidth, 8);
    // SAFETY: the test descriptor and its frame buffer.
    unsafe { rasops24_putchar(ri.cookie(), 1, 2, u32::from(b'A'), attr(ri)) }.expect("draws");
    check_cell(ri, &bytes(fb.words()), 1, 2, true);
}

#[test]
fn the_stamp_putchar_draws_the_same_glyph() {
    let (ri, fb) = test_ri(24, 640, 480);
    // SAFETY: the test descriptor and its frame buffer.
    unsafe { rasops24_putchar8(ri.cookie(), 1, 2, u32::from(b'A'), attr(ri)) }.expect("draws");
    // The underline of the fast paths is the C's (see the deviations): not looked at.
    check_cell(ri, &bytes(fb.words()), 1, 2, false);

    // Without the underline both draw the same cell.
    let plain = attr(ri) & !8;
    let (ri1, fb1) = test_ri(24, 640, 480);
    // SAFETY: the test descriptor and its frame buffer.
    unsafe { rasops24_putchar8(ri1.cookie(), 1, 2, u32::from(b'A'), plain) }.expect("draws");
    let (ri2, fb2) = test_ri(24, 640, 480);
    // SAFETY: the test descriptor and its frame buffer.
    unsafe { rasops24_putchar(ri2.cookie(), 1, 2, u32::from(b'A'), plain) }.expect("draws");
    assert_eq!(fb1.words(), fb2.words());
}

#[test]
fn a_twelve_pixel_font() {
    let (ri, fb) = test_ri(24, 1280, 800);
    assert_eq!(ri.font().fontwidth, 12);
    // SAFETY: the test descriptor and its frame buffer.
    unsafe { rasops24_putchar12(ri.cookie(), 2, 3, u32::from(b'A'), attr(ri)) }.expect("draws");
    check_cell(ri, &bytes(fb.words()), 2, 3, false);
}

#[test]
fn a_space_is_the_background() {
    let (ri, fb) = test_ri(24, 640, 480);
    // SAFETY: the test descriptor and its frame buffer.
    unsafe { rasops24_putchar8(ri.cookie(), 0, 0, u32::from(b' '), attr(ri)) }.expect("draws");
    let b = bytes(fb.words());
    let stride = ri.ri_stride.get() as usize;
    let blue = bgr(ri.ri_devcmap[4].get());
    for y in 0..14 {
        for x in 0..8 {
            assert_eq!(pixel(&b, stride, x, y), blue);
        }
    }
}

#[test]
fn a_sixteen_pixel_font() {
    // The 16-pixel font, when the geometry picks it.
    let (ri, fb) = test_ri(24, 2560, 1440);
    if ri.font().fontwidth != 16 {
        return;
    }
    // SAFETY: the test descriptor and its frame buffer.
    unsafe { rasops24_putchar16(ri.cookie(), 1, 1, u32::from(b'A'), attr(ri)) }.expect("draws");
    check_cell(ri, &bytes(fb.words()), 1, 1, false);
}
