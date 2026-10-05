use super::*;

use crate::dev::rasops::rasops::tests::test_ri;

/// Pixel `x` of line `y` of the frame buffer (pixel 0 of a word is its least significant
/// bit on little-endian, the order the masks give).
fn pixel(words: &[i32], stride_words: usize, x: usize, y: usize) -> bool {
    (words[y * stride_words + x / 32] as u32 >> (x % 32)) & 1 != 0
}

/// Checks the cell at (`row`, `col`) of an 'A': `on` is the colour of the set bits of the
/// glyph, `off` that of the others. The last two lines (the underline's) are not looked at.
fn check_cell(ri: &RasopsInfo, words: &[i32], row: usize, col: usize, on: bool, off: bool) {
    let font = ri.font();
    let (fw, fh, fs) = (
        font.fontwidth as usize,
        font.fontheight as usize,
        font.stride as usize,
    );
    let glyph = ri.glyph(u32::from(b'A') - 32).to_vec();
    let stride_words = ri.ri_stride.get() as usize / 4;
    for y in 0..fh - 2 {
        for x in 0..fw {
            let byte = glyph[y * fs + x / 8];
            let set = byte & (0x80 >> (x % 8)) != 0;
            let px = pixel(words, stride_words, col * fw + x, row * fh + y);
            assert_eq!(px, if set { on } else { off }, "pixel {x},{y}");
        }
    }
}

#[test]
fn draws_a_glyph_and_its_underline() {
    // 1 bpp, 8x16 font: 'A' with the foreground on (white), at cell (1, 2).
    let (ri, fb) = test_ri(1, 640, 480);
    assert_eq!(ri.font().fontwidth, 8);
    // SAFETY: the test descriptor and its frame buffer.
    unsafe { rasops1_putchar(ri.cookie(), 1, 2, u32::from(b'A'), 0x0100_0000 | 8) }.expect("draws");

    let w = fb.words();
    check_cell(ri, w, 1, 2, true, false);
    // The underline is the second line from the bottom, in the foreground.
    let stride_words = ri.ri_stride.get() as usize / 4;
    for x in 0..8 {
        assert!(pixel(w, stride_words, 16 + x, 16 + 14), "underline {x}");
    }
    // The pixels around the cell are as they were.
    let old = 0x5a5a_5a5au32;
    for y in 0..16 {
        let word = w[(16 + y) * stride_words] as u32;
        assert_eq!(word & !0x00ff_0000, old & !0x00ff_0000, "line {y}");
    }
}

#[test]
fn background_on_draws_the_inverse() {
    // bg on, fg off: the set bits of the glyph are the off pixels.
    let (ri, fb) = test_ri(1, 640, 480);
    // SAFETY: the test descriptor and its frame buffer.
    unsafe { rasops1_putchar(ri.cookie(), 3, 4, u32::from(b'A'), 0x0001_0000) }.expect("draws");
    check_cell(ri, fb.words(), 3, 4, false, true);
}

#[test]
fn a_space_fills_with_the_background() {
    let (ri, fb) = test_ri(1, 640, 480);
    // SAFETY: the test descriptor and its frame buffer.
    unsafe { rasops1_putchar(ri.cookie(), 0, 1, u32::from(b' '), 0x0001_0000) }.expect("draws");
    let stride_words = ri.ri_stride.get() as usize / 4;
    for y in 0..16 {
        for x in 8..16 {
            assert!(pixel(fb.words(), stride_words, x, y));
        }
    }
}

#[test]
fn a_cell_that_straddles_two_words() {
    // 1280x800 picks the 12x24 font: cell 2 is pixels 24..36, across words 0 and 1. Its
    // width is not a multiple of 8, so init installs the bit-operation column routines.
    let (ri, fb) = test_ri(1, 1280, 800);
    assert_eq!(ri.font().fontwidth, 12);
    assert!(ri.ri_ops.get().erasecols.is_some());
    // SAFETY: the test descriptor and its frame buffer.
    unsafe { rasops1_putchar(ri.cookie(), 0, 2, u32::from(b'A'), 0x0100_0000) }.expect("draws");
    check_cell(ri, fb.words(), 0, 2, true, false);
}
