/* <LICENSES> */
/*-
 * Copyright (c) 1999 The NetBSD Foundation, Inc.
 * All rights reserved.
 *
 * This code is derived from software contributed to The NetBSD Foundation
 * by Andrew Doran.
 *
 * Redistribution and use in source and binary forms, with or without
 * modification, are permitted provided that the following conditions
 * are met:
 * 1. Redistributions of source code must retain the above copyright
 *    notice, this list of conditions and the following disclaimer.
 * 2. Redistributions in binary form must reproduce the above copyright
 *    notice, this list of conditions and the following disclaimer in the
 *    documentation and/or other materials provided with the distribution.
 *
 * THIS SOFTWARE IS PROVIDED BY THE NETBSD FOUNDATION, INC. AND CONTRIBUTORS
 * ``AS IS'' AND ANY EXPRESS OR IMPLIED WARRANTIES, INCLUDING, BUT NOT LIMITED
 * TO, THE IMPLIED WARRANTIES OF MERCHANTABILITY AND FITNESS FOR A PARTICULAR
 * PURPOSE ARE DISCLAIMED.  IN NO EVENT SHALL THE FOUNDATION OR CONTRIBUTORS
 * BE LIABLE FOR ANY DIRECT, INDIRECT, INCIDENTAL, SPECIAL, EXEMPLARY, OR
 * CONSEQUENTIAL DAMAGES (INCLUDING, BUT NOT LIMITED TO, PROCUREMENT OF
 * SUBSTITUTE GOODS OR SERVICES; LOSS OF USE, DATA, OR PROFITS; OR BUSINESS
 * INTERRUPTION) HOWEVER CAUSED AND ON ANY THEORY OF LIABILITY, WHETHER IN
 * CONTRACT, STRICT LIABILITY, OR TORT (INCLUDING NEGLIGENCE OR OTHERWISE)
 * ARISING IN ANY WAY OUT OF THE USE OF THIS SOFTWARE, EVEN IF ADVISED OF THE
 * POSSIBILITY OF SUCH DAMAGE.
 */
/* </LICENSES> */

/* <CODE> */
//! 24-bit-per-pixel raster operations: `dev/rasops/rasops24.c` (`rasops24` in GENERIC).
//!
//! Upstream: sys/dev/rasops/rasops24.c @ 3ce1f3f79392
//!
//! The generic byte-at-a-time `putchar`, and `putchar` for 8, 12 and 16 pixel wide fonts
//! through a 4x1 stamp (the 16 patterns of four pixels, three 32-bit words each). The row
//! and column operations are rasops's generic ones.
//!
//! ## Deviations
//! - `rasops24_putchar8`, `rasops24_putchar12` and `rasops24_putchar16` are one function
//!   (`putchar_stamp`) with the number of glyph nibbles per line (2, 3, 4) as a parameter:
//!   the C's three bodies differ only in how many stamp triples they write and in the glyph
//!   byte (`fr[0]` or `fr[1]`) each comes from. The pixels written are the same.
//!   `STAMP_SHIFT(fb, n) & STAMP_MASK` and `STAMP_READ(so)`, `STAMP_READ(so + 4)`,
//!   `STAMP_READ(so + 8)` (byte offsets into `stamp`, the nibble times 16) are
//!   `stamp[4 * nibble]`, `stamp[4 * nibble + 1]` and `stamp[4 * nibble + 2]`.
//! - Upstream quirk kept: the fast paths' underline is `STAMP_READ(52)`, `stamp[13]`, written
//!   to every word of the line (a word holds one and a third pixels), where the other
//!   depths' read the stamp of four foreground pixels.
//! - The fast paths' `uc == (u_int)-1` test for a space (which no caller passes: a space
//!   is drawn from the font's glyph for it) is kept as it is.
//! - The static `stamp[64]`, `stamp_attr` and `stamp_mutex` are atomics (`Relaxed`): the C
//!   draws with them under no lock but the console's (a nested call, from an interrupt,
//!   finds `stamp_mutex` set and takes the generic path), which is what the counter still
//!   does.
//! - The glyph row is read `stride` bytes at a time (at most 4), where the C's generic
//!   `putchar` always reads 4 bytes (`fr[0]`..`fr[3]`), past the row of a font narrower
//!   than 25 pixels; those bits are shifted out only after `width` pixels, so the pixels
//!   drawn are the same.
//! - `RASOPS_SMALL` and `RASOPS_CLIPPING` are not configured; they are kept behind the
//!   constants of the same names (false). `RASOPS_SMALL` is `SMALL_KERNEL`.
//! - Pixels are stored with `write_volatile` (`rasops.rs`'s deviations).

use core::ffi::c_void;
use core::sync::atomic::{AtomicI32, AtomicU32, Ordering};

use crate::dev::rasops::rasops::{RASOPS_CLIPPING, RI_BSWAP, RasopsInfo, fb_w8, fb_w32};
use crate::dev::wscons::wsdisplayvar::WSATTR_UNDERLINE;
use crate::sys::errno::Errno;

/// `RASOPS_SMALL`: not configured (`SMALL_KERNEL` is off).
const RASOPS_SMALL: bool = false;

/// `BYTE_ORDER == LITTLE_ENDIAN`.
const LITTLE_ENDIAN: bool = cfg!(target_endian = "little");

/// `stamp`: 4x1 stamp for optimized character blitting. The console draws one character at
/// a time (`stamp_mutex`), under the kernel lock.
static STAMP: [AtomicI32; 64] = [const { AtomicI32::new(0) }; 64];
/// `stamp_attr`: the attribute `STAMP` was made for.
static STAMP_ATTR: AtomicU32 = AtomicU32::new(0);
/// `stamp_mutex`: XXX see note in readme.
static STAMP_MUTEX: AtomicI32 = AtomicI32::new(0);

/// `rasops24_init`: initialize a `rasops_info` descriptor for this depth.
pub fn rasops24_init(ri: &RasopsInfo) {
    let mut ops = ri.ri_ops.get();
    match ri.font().fontwidth {
        8 if !RASOPS_SMALL => ops.putchar = Some(rasops24_putchar8),
        12 if !RASOPS_SMALL => ops.putchar = Some(rasops24_putchar12),
        16 if !RASOPS_SMALL => ops.putchar = Some(rasops24_putchar16),
        _ => ops.putchar = Some(rasops24_putchar),
    }
    ri.ri_ops.set(ops);

    if ri.ri_rnum.get() == 0 {
        ri.ri_rnum.set(8);
        ri.ri_rpos.set(0);
        ri.ri_gnum.set(8);
        ri.ri_gpos.set(8);
        ri.ri_bnum.set(8);
        ri.ri_bpos.set(16);
    }
}

/// The four bytes of a glyph row as the C reads them (`fr[3] | fr[2] << 8 | fr[1] << 16 |
/// fr[0] << 24`): the first byte is the most significant, bytes past the font's stride are
/// zero.
fn row_bits(glyph: &[u8], fs: usize, y: usize) -> u32 {
    let mut fb = 0u32;
    for i in 0..fs.min(4) {
        let byte = glyph.get(y * fs + i).copied().unwrap_or(0);
        fb |= u32::from(byte) << (24 - 8 * i);
    }
    fb
}

/// Stores the three bytes of a pixel, `clr >> 16`, `clr >> 8`, `clr`.
///
/// # Safety
///
/// `dp` and the two bytes after it are inside the mapped frame buffer.
unsafe fn put_pixel(dp: *mut u8, clr: i32) {
    // SAFETY: the caller's contract.
    unsafe {
        fb_w8(dp, (clr >> 16) as u8);
        fb_w8(dp.add(1), (clr >> 8) as u8);
        fb_w8(dp.add(2), clr as u8);
    }
}

/// `rasops24_putchar`: put a single character. This is the generic version.
/// XXX this bites - we should use masks.
///
/// # Safety
///
/// `cookie` is a configured 24-bit [`RasopsInfo`] whose frame buffer is mapped; the cell is
/// on the screen (without `RASOPS_CLIPPING` the C does not check it either).
pub unsafe fn rasops24_putchar(
    cookie: *mut c_void,
    row: i32,
    col: i32,
    uc: u32,
    attr: u32,
) -> Result<(), Errno> {
    // SAFETY: the caller's contract.
    let ri = unsafe { &*cookie.cast::<RasopsInfo>() };

    if RASOPS_CLIPPING {
        // Catches 'row < 0' case too
        if row as u32 >= ri.ri_rows.get() as u32 {
            return Ok(());
        }

        if col as u32 >= ri.ri_cols.get() as u32 {
            return Ok(());
        }
    }

    // SAFETY: the cell is on the screen (the caller's contract).
    let mut rp = unsafe {
        ri.ri_bits
            .get()
            .offset((row * ri.ri_yscale.get() + col * ri.ri_xscale.get()) as isize)
    };
    let font = ri.font();
    let mut height = font.fontheight;
    let mut width = font.fontwidth;
    let stride = ri.ri_stride.get() as isize;

    let clr = [
        ri.ri_devcmap[((attr >> 16) & 0xf) as usize].get(),
        ri.ri_devcmap[((attr >> 24) & 0xf) as usize].get(),
    ];

    // SAFETY: `width` three-byte pixels on each of the cell's `height` lines, `stride` bytes
    // apart, and the line the underline goes back to is the cell's (the caller's contract).
    unsafe {
        if uc == u32::from(b' ') {
            while height != 0 {
                height -= 1;
                let dp = rp;
                rp = rp.wrapping_offset(stride);

                for cnt in 0..width as usize {
                    put_pixel(dp.add(3 * cnt), clr[0]);
                }
            }
        } else {
            let glyph = ri.glyph(uc.wrapping_sub(font.firstchar as u32));
            let fs = font.stride as usize;
            let mut y = 0usize;

            while height != 0 {
                height -= 1;
                let dp = rp;
                let mut fb = row_bits(glyph, fs, y);
                y += 1;
                rp = rp.wrapping_offset(stride);

                for cnt in 0..width as usize {
                    put_pixel(dp.add(3 * cnt), clr[(fb >> 31) as usize]);
                    fb <<= 1;
                }
            }
        }

        // Do underline
        if attr & WSATTR_UNDERLINE as u32 != 0 {
            rp = rp.wrapping_offset(-(stride << 1));

            while width != 0 {
                width -= 1;
                put_pixel(rp, clr[1]);
                rp = rp.add(3);
            }
        }
    }

    Ok(())
}

/// `rasops24_makestamp`: recompute the blitting stamp.
pub fn rasops24_makestamp(ri: &RasopsInfo, attr: u32) {
    let fg = ri.ri_devcmap[((attr >> 24) & 0xf) as usize].get() as u32 & 0xff_ffff;
    let bg = ri.ri_devcmap[((attr >> 16) & 0xf) as usize].get() as u32 & 0xff_ffff;
    STAMP_ATTR.store(attr, Ordering::Relaxed);

    let bswap = ri.ri_flg.get() & RI_BSWAP != 0;
    for i in (0..64).step_by(4) {
        let pick = |bit: usize| if i & bit != 0 { fg } else { bg };
        let (c1, c2, c3, c4) = if LITTLE_ENDIAN {
            (pick(32), pick(16), pick(8), pick(4))
        } else {
            (pick(8), pick(4), pick(16), pick(32))
        };
        let mut w = [
            (c1 << 8) | (c2 >> 16),
            (c2 << 16) | (c3 >> 8),
            (c3 << 24) | c4,
        ];

        if LITTLE_ENDIAN != bswap {
            for v in &mut w {
                *v = v.swap_bytes();
            }
        }

        for (k, v) in w.iter().enumerate() {
            STAMP[i + k].store(*v as i32, Ordering::Relaxed);
        }
    }
}

/// One entry of the stamp.
fn stamp(i: usize) -> i32 {
    STAMP.get(i).map_or(0, |s| s.load(Ordering::Relaxed))
}

/// The body of `rasops24_putchar8` (`nibbles` 2), `rasops24_putchar12` (3) and
/// `rasops24_putchar16` (4): `nibbles` glyph nibbles per line, three 32-bit words each.
///
/// # Safety
///
/// As for [`rasops24_putchar8`].
unsafe fn putchar_stamp(
    cookie: *mut c_void,
    row: i32,
    col: i32,
    uc: u32,
    attr: u32,
    nibbles: usize,
) -> Result<(), Errno> {
    // Can't risk remaking the stamp if it's already in use
    if STAMP_MUTEX.fetch_add(1, Ordering::Relaxed) != 0 {
        STAMP_MUTEX.fetch_sub(1, Ordering::Relaxed);
        // SAFETY: the caller's contract.
        return unsafe { rasops24_putchar(cookie, row, col, uc, attr) };
    }

    // SAFETY: the caller's contract.
    let ri = unsafe { &*cookie.cast::<RasopsInfo>() };

    if RASOPS_CLIPPING {
        if row as u32 >= ri.ri_rows.get() as u32 {
            STAMP_MUTEX.fetch_sub(1, Ordering::Relaxed);
            return Ok(());
        }

        if col as u32 >= ri.ri_cols.get() as u32 {
            STAMP_MUTEX.fetch_sub(1, Ordering::Relaxed);
            return Ok(());
        }
    }

    // Recompute stamp?
    if attr != STAMP_ATTR.load(Ordering::Relaxed) {
        rasops24_makestamp(ri, attr);
    }

    let font = ri.font();
    // SAFETY: the cell is on the screen (the caller's contract).
    let mut rp = unsafe {
        ri.ri_bits
            .get()
            .offset((row * ri.ri_yscale.get() + col * ri.ri_xscale.get()) as isize)
    };
    let mut height = font.fontheight;
    let stride = ri.ri_stride.get() as isize;
    let nwords = nibbles * 3;

    // SAFETY: `nwords` 32-bit words on each of the cell's `height` lines, `stride` bytes
    // apart (the caller's contract).
    unsafe {
        if uc == u32::MAX {
            let c = stamp(0);
            while height != 0 {
                height -= 1;
                for k in 0..nwords {
                    fb_w32(rp.add(4 * k), c);
                }
                rp = rp.wrapping_offset(stride);
            }
        } else {
            let glyph = ri.glyph(uc.wrapping_sub(font.firstchar as u32));
            let fs = font.stride as usize;
            let mut y = 0usize;

            while height != 0 {
                height -= 1;
                for k in 0..nibbles {
                    let byte = glyph.get(y * fs + k / 2).copied().unwrap_or(0);
                    let nibble = usize::from(if k % 2 == 0 { byte >> 4 } else { byte & 0xf });
                    for j in 0..3 {
                        fb_w32(rp.add(12 * k + 4 * j), stamp(4 * nibble + j));
                    }
                }

                y += 1;
                rp = rp.wrapping_offset(stride);
            }
        }

        // Do underline
        if attr & WSATTR_UNDERLINE as u32 != 0 {
            let c = stamp(13);

            rp = rp.wrapping_offset(-(stride << 1));
            for k in 0..nwords {
                fb_w32(rp.add(4 * k), c);
            }
        }
    }

    STAMP_MUTEX.fetch_sub(1, Ordering::Relaxed);

    Ok(())
}

/// `rasops24_putchar8`: put a single character. This is for 8-pixel wide fonts.
///
/// # Safety
///
/// `cookie` is a configured 24-bit [`RasopsInfo`] with an 8 pixel wide font whose frame
/// buffer is mapped; the cell is on the screen (without `RASOPS_CLIPPING` the C does not
/// check it either).
pub unsafe fn rasops24_putchar8(
    cookie: *mut c_void,
    row: i32,
    col: i32,
    uc: u32,
    attr: u32,
) -> Result<(), Errno> {
    // SAFETY: the caller's contract.
    unsafe { putchar_stamp(cookie, row, col, uc, attr, 2) }
}

/// `rasops24_putchar12`: put a single character. This is for 12-pixel wide fonts.
///
/// # Safety
///
/// As for [`rasops24_putchar8`], with a 12 pixel wide font.
pub unsafe fn rasops24_putchar12(
    cookie: *mut c_void,
    row: i32,
    col: i32,
    uc: u32,
    attr: u32,
) -> Result<(), Errno> {
    // SAFETY: the caller's contract.
    unsafe { putchar_stamp(cookie, row, col, uc, attr, 3) }
}

/// `rasops24_putchar16`: put a single character. This is for 16-pixel wide fonts.
///
/// # Safety
///
/// As for [`rasops24_putchar8`], with a 16 pixel wide font.
pub unsafe fn rasops24_putchar16(
    cookie: *mut c_void,
    row: i32,
    col: i32,
    uc: u32,
    attr: u32,
) -> Result<(), Errno> {
    // SAFETY: the caller's contract.
    unsafe { putchar_stamp(cookie, row, col, uc, attr, 4) }
}
/* </CODE> */

/* <TESTS> */
#[cfg(test)]
mod tests {
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
}
/* </TESTS> */
