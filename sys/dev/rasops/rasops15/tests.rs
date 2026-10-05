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

/// Pixel `x` of line `y`.
fn pixel(b: &[u8], stride: usize, x: usize, y: usize) -> u16 {
    u16::from_le_bytes([b[y * stride + 2 * x], b[y * stride + 2 * x + 1]])
}

/// Checks the cell at (`row`, `col`) of an 'A' painted with `attr(ri)`, then the underline.
fn check_cell(ri: &RasopsInfo, b: &[u8], row: usize, col: usize) {
    let font = ri.font();
    let (fw, fh, fs) = (
        font.fontwidth as usize,
        font.fontheight as usize,
        font.stride as usize,
    );
    let stride = ri.ri_stride.get() as usize;
    let (white, blue) = (ri.ri_devcmap[7].get() as u16, ri.ri_devcmap[4].get() as u16);
    assert_ne!(white, blue);
    let glyph = ri.glyph(u32::from(b'A') - 32).to_vec();
    for y in 0..fh - 2 {
        for x in 0..fw {
            let set = glyph[y * fs + x / 8] & (0x80 >> (x % 8)) != 0;
            let px = pixel(b, stride, col * fw + x, row * fh + y);
            assert_eq!(px, if set { white } else { blue }, "pixel {x},{y}");
        }
    }
    for x in 0..fw {
        assert_eq!(
            pixel(b, stride, col * fw + x, row * fh + fh - 2),
            white,
            "underline {x}"
        );
    }
}

#[test]
fn init_defaults_the_channel_layout() {
    let (ri, _fb) = test_ri(16, 640, 480);
    assert_eq!(
        (ri.ri_rnum.get(), ri.ri_gnum.get(), ri.ri_bnum.get()),
        (5, 6, 5)
    );
    assert_eq!(
        (ri.ri_rpos.get(), ri.ri_gpos.get(), ri.ri_bpos.get()),
        (0, 5, 11)
    );
    let (ri, _fb) = test_ri(15, 640, 480);
    assert_eq!(
        (ri.ri_rnum.get(), ri.ri_gnum.get(), ri.ri_bnum.get()),
        (5, 5, 5)
    );
    assert_eq!(
        (ri.ri_rpos.get(), ri.ri_gpos.get(), ri.ri_bpos.get()),
        (0, 5, 10)
    );
}

#[test]
fn the_stamp_putchar_draws_a_glyph_and_its_underline() {
    let (ri, fb) = test_ri(16, 640, 480);
    assert_eq!(ri.font().fontwidth, 8);
    // SAFETY: the test descriptor and its frame buffer.
    unsafe { rasops15_putchar8(ri.cookie(), 1, 2, u32::from(b'A'), attr(ri)) }.expect("draws");
    check_cell(ri, &bytes(fb.words()), 1, 2);
}

#[test]
fn the_generic_putchar_draws_the_same() {
    let (ri, fb) = test_ri(16, 640, 480);
    // SAFETY: the test descriptor and its frame buffer.
    unsafe { rasops15_putchar(ri.cookie(), 1, 2, u32::from(b'A'), attr(ri)) }.expect("draws");
    check_cell(ri, &bytes(fb.words()), 1, 2);

    let (ri2, fb2) = test_ri(16, 640, 480);
    // SAFETY: the test descriptor and its frame buffer.
    unsafe { rasops15_putchar8(ri2.cookie(), 1, 2, u32::from(b'A'), attr(ri2)) }.expect("draws");
    assert_eq!(fb.words(), fb2.words());
}

#[test]
fn a_space_is_the_background() {
    // 15 bits, with attributes of its own: the stamp is remade only when the attribute
    // changes (the C's), and the 16-bit tests use the colours of `attr`.
    let (ri, fb) = test_ri(15, 640, 480);
    let attr = (3 << 24) | (1 << 16);
    // SAFETY: the test descriptor and its frame buffer.
    unsafe { rasops15_putchar8(ri.cookie(), 0, 0, u32::from(b' '), attr) }.expect("draws");
    let b = bytes(fb.words());
    let stride = ri.ri_stride.get() as usize;
    let blue = ri.ri_devcmap[1].get() as u16;
    for y in 0..14 {
        for x in 0..8 {
            assert_eq!(pixel(&b, stride, x, y), blue);
        }
    }
}

#[test]
fn a_twelve_pixel_font() {
    let (ri, fb) = test_ri(16, 1280, 800);
    assert_eq!(ri.font().fontwidth, 12);
    // SAFETY: the test descriptor and its frame buffer.
    unsafe { rasops15_putchar12(ri.cookie(), 2, 3, u32::from(b'A'), attr(ri)) }.expect("draws");
    check_cell(ri, &bytes(fb.words()), 2, 3);
}

#[test]
fn a_sixteen_pixel_font() {
    // The 16-pixel font, when the geometry picks it.
    let (ri, fb) = test_ri(16, 2560, 1440);
    if ri.font().fontwidth != 16 {
        return;
    }
    // SAFETY: the test descriptor and its frame buffer.
    unsafe { rasops15_putchar16(ri.cookie(), 1, 1, u32::from(b'A'), attr(ri)) }.expect("draws");
    check_cell(ri, &bytes(fb.words()), 1, 1);
}
