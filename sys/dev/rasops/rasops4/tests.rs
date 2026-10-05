use super::*;

use std::vec::Vec;

use crate::dev::rasops::rasops::tests::test_ri;

/// The colour index (0..16) of pixel `x` on line `y`: two pixels a byte, the left one in
/// the high nibble.
fn pixel(bytes: &[u8], stride: usize, x: usize, y: usize) -> u8 {
    (bytes[y * stride + x / 2] >> (4 * (1 - x % 2))) & 0xf
}

fn bytes(words: &[i32]) -> Vec<u8> {
    words.iter().flat_map(|w| w.to_le_bytes()).collect()
}

/// Foreground colour 7, background 4, underlined: a 4-bit descriptor packs mono attributes,
/// so the colour indices are put in by hand.
fn attr(_ri: &RasopsInfo) -> u32 {
    (7 << 24) | (4 << 16) | 8
}

/// `rasops4_putchar8`, again while another test thread holds the stamp (the generic
/// version, which does not exist for 4 bits, answers `EAGAIN`).
fn put8(ri: &RasopsInfo, row: i32, col: i32, uc: u32, attr: u32) {
    loop {
        // SAFETY: the test descriptor and its frame buffer.
        match unsafe { rasops4_putchar8(ri.cookie(), row, col, uc, attr) } {
            Ok(()) => return,
            Err(Errno::EAGAIN) => std::thread::yield_now(),
            Err(e) => panic!("putchar8: {e:?}"),
        }
    }
}

#[test]
fn draws_a_glyph_and_its_underline() {
    let (ri, fb) = test_ri(4, 640, 480);
    assert_eq!(ri.font().fontwidth, 8);
    put8(ri, 1, 2, u32::from(b'A'), attr(ri));

    let b = bytes(fb.words());
    let stride = ri.ri_stride.get() as usize;
    let glyph = ri.glyph(u32::from(b'A') - 32).to_vec();
    for (y, &bits) in glyph.iter().enumerate().take(14) {
        for x in 0..8 {
            let on = bits & (0x80 >> x) != 0;
            let want = if on { 7 } else { 4 };
            assert_eq!(pixel(&b, stride, 16 + x, 16 + y), want, "pixel {x},{y}");
        }
    }
    // The underline is the second line from the bottom, in the foreground.
    for x in 0..8 {
        assert_eq!(pixel(&b, stride, 16 + x, 16 + 14), 7, "underline {x}");
    }
    // The neighbouring cell is untouched.
    assert_eq!(pixel(&b, stride, 24, 16), 0x5);
}

#[test]
fn a_space_is_the_background() {
    let (ri, fb) = test_ri(4, 640, 480);
    put8(ri, 0, 0, u32::from(b' '), attr(ri));
    let b = bytes(fb.words());
    let stride = ri.ri_stride.get() as usize;
    for y in 0..14 {
        for x in 0..8 {
            assert_eq!(pixel(&b, stride, x, y), 4);
        }
    }
}

#[test]
fn the_stamp_follows_the_attribute() {
    let (ri, _fb) = test_ri(4, 640, 480);
    rasops4_makestamp(ri, attr(ri));
    // Nibble 0 is all background, 15 all foreground, 8 is the first pixel only (the
    // high nibble of the low byte on little-endian).
    assert_eq!(stamp(0) as u16, 0x4444);
    assert_eq!(stamp(15) as u16, 0x7777);
    assert_eq!(stamp(8) as u16, 0x4474);
}

#[test]
fn erase_and_copy_columns() {
    let (ri, fb) = test_ri(4, 640, 480);
    put8(ri, 1, 3, u32::from(b'A'), attr(ri));
    // SAFETY: the test descriptor and its frame buffer.
    unsafe {
        // Copy cell 3 to cell 0 and erase cells 5 and 6 (the bit operations: 8 pixels of 4
        // bits are whole words).
        rasops4_copycols(ri.cookie(), 1, 3, 0, 1).expect("copies");
        rasops4_erasecols(ri.cookie(), 1, 5, 2, attr(ri)).expect("erases");
    }
    let b = bytes(fb.words());
    let stride = ri.ri_stride.get() as usize;
    for y in 0..16 {
        for x in 0..8 {
            assert_eq!(
                pixel(&b, stride, x, 16 + y),
                pixel(&b, stride, 24 + x, 16 + y),
                "copied pixel {x},{y}"
            );
            assert_eq!(pixel(&b, stride, 40 + x, 16 + y), 4, "erased pixel {x},{y}");
            assert_eq!(pixel(&b, stride, 48 + x, 16 + y), 4, "erased pixel {x},{y}");
        }
    }
    // The cell after them is untouched.
    assert_eq!(pixel(&b, stride, 56, 16), 0x5);
}
