/* <LICENSES> */
/*
 * Copyright (c) 2026 Emilio Navarrete Lineros <enavarre@outlook.com>
 *
 * Permission to use, copy, modify, and distribute this software for any
 * purpose with or without fee is hereby granted, provided that the above
 * copyright notice and this permission notice appear in all copies.
 *
 * THE SOFTWARE IS PROVIDED "AS IS" AND THE AUTHOR DISCLAIMS ALL WARRANTIES
 * WITH REGARD TO THIS SOFTWARE INCLUDING ALL IMPLIED WARRANTIES OF
 * MERCHANTABILITY AND FITNESS. IN NO EVENT SHALL THE AUTHOR BE LIABLE FOR
 * ANY SPECIAL, DIRECT, INDIRECT, OR CONSEQUENTIAL DAMAGES OR ANY DAMAGES
 * WHATSOEVER RESULTING FROM LOSS OF USE, DATA OR PROFITS, WHETHER IN AN
 * ACTION OF CONTRACT, NEGLIGENCE OR OTHER TORTIOUS ACTION, ARISING OUT OF
 * OR IN CONNECTION WITH THE USE OR PERFORMANCE OF THIS SOFTWARE.
 */

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
//! 1-bit-per-pixel raster operations: `dev/rasops/rasops1.c` (`rasops1` in GENERIC).
//!
//! Upstream: sys/dev/rasops/rasops1.c @ 3ce1f3f79392
//!
//! The generic `putchar` for any font width, the 8 and 16 pixel ones the C compiles on
//! big-endian machines, and (through `rasops_bitops`) the column operations for fonts whose
//! width is not a multiple of 8.
//!
//! ## Deviations
//! - `rasops1_putchar8` and `rasops1_putchar16` are under
//!   `#if !defined(RASOPS_SMALL) && BYTE_ORDER == BIG_ENDIAN` in the C: they are ported and
//!   kept behind the `RASOPS_SMALL` and `BIG_ENDIAN` constants (neither amd64 nor arm64 is
//!   big-endian, so `rasops1_init` never selects them).
//! - `rasops1_erasecols`, `rasops1_copycols` and `rasops1_do_cursor` are the
//!   `rasops_bitops.h` template instantiated with `PIXEL_SHIFT` 0: one-line calls to the
//!   generic functions of `rasops_bitops.rs`.
//! - The glyph row is read `stride` bytes at a time (at most 4), where the C always reads 4
//!   bytes (`fr[0]`..`fr[3]`), past the row of a font narrower than 25 pixels: those bits
//!   are masked off by `rmask` in the C, and the glyph data may end there. Rows are
//!   addressed from the glyph slice, not by advancing a pointer.
//! - A shift by 32 or more (`fb << width` of a 32 pixel wide font at column 0, undefined in
//!   C) gives 0, as the `MBR` macro does.
//! - Pixels are stored with `write_volatile` (`rasops.rs`'s deviations).
//! - `RASOPS_CLIPPING` is not configured; its checks are kept behind the
//!   `RASOPS_CLIPPING` constant (false).

use core::ffi::c_void;

use crate::dev::rasops::rasops::{RASOPS_CLIPPING, RasopsInfo, fb_r32, fb_w8, fb_w16, fb_w32};
use crate::dev::rasops::rasops_bitops;
use crate::dev::rasops::rasops_masks::{
    RASOPS_LMASK, RASOPS_RMASK, mbe, mbl, mbr, pmask, rasops_masks_init,
};
use crate::dev::wscons::wsdisplayvar::WSATTR_UNDERLINE;
use crate::sys::errno::Errno;

/// `RASOPS_SMALL`: not configured (`SMALL_KERNEL` is off).
const RASOPS_SMALL: bool = false;

/// `BYTE_ORDER == BIG_ENDIAN`.
const BIG_ENDIAN: bool = cfg!(target_endian = "big");

/// `rasops1_init`: initialize a `rasops_info` descriptor for this depth.
pub fn rasops1_init(ri: &RasopsInfo) {
    rasops_masks_init();

    let mut ops = ri.ri_ops.get();
    match ri.font().fontwidth {
        8 if !RASOPS_SMALL && BIG_ENDIAN => ops.putchar = Some(rasops1_putchar8),
        16 if !RASOPS_SMALL && BIG_ENDIAN => ops.putchar = Some(rasops1_putchar16),
        _ => ops.putchar = Some(rasops1_putchar),
    }

    if ri.font().fontwidth & 7 != 0 {
        ops.erasecols = Some(rasops1_erasecols);
        ops.copycols = Some(rasops1_copycols);
        ri.ri_do_cursor.set(Some(rasops1_do_cursor));
    }
    ri.ri_ops.set(ops);
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

/// `rasops1_putchar`: paint a single character. This is the generic version, this is ugly.
///
/// # Safety
///
/// `cookie` is a configured 1-bit [`RasopsInfo`] whose frame buffer is mapped; the cell is
/// on the screen (without `RASOPS_CLIPPING` the C does not check it either).
pub unsafe fn rasops1_putchar(
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

    let font = ri.font();
    let col = col * font.fontwidth as i32;
    // SAFETY: the cell is on the screen (the caller's contract).
    let mut rp = unsafe {
        ri.ri_bits
            .get()
            .offset((row * ri.ri_yscale.get() + ((col >> 3) & !3)) as isize)
    };
    let mut height = font.fontheight;
    let mut width = font.fontwidth;
    let col = col & 31;
    let rs = ri.ri_stride.get() as isize;

    let mut bg = if attr & 0x000f_0000 != 0 {
        ri.ri_devcmap[1].get()
    } else {
        ri.ri_devcmap[0].get()
    } as u32;
    let fg = if attr & 0x0f00_0000 != 0 {
        ri.ri_devcmap[1].get()
    } else {
        ri.ri_devcmap[0].get()
    } as u32;

    // If fg and bg match this becomes a space character
    let glyph = if fg == bg || uc == u32::from(b' ') {
        None
    } else {
        Some(ri.glyph(uc.wrapping_sub(font.firstchar as u32)))
    };
    let fs = font.stride as usize;
    let mut y = 0usize;

    // SAFETY: every word written is one of the cell's: one word, or two, on each of its
    // `height` lines, `rs` bytes apart (the caller's contract).
    unsafe {
        // Single word, one mask
        if col + width as i32 <= 32 {
            let rmask = pmask(col, width as i32) as u32;
            let lmask = !rmask;

            match glyph {
                None => {
                    bg &= rmask;

                    while height != 0 {
                        height -= 1;
                        fb_w32(rp, ((fb_r32(rp) as u32 & lmask) | bg) as i32);
                        rp = rp.wrapping_offset(rs);
                    }
                }
                Some(glyph) => {
                    // NOT fontbits if bg is white
                    while height != 0 {
                        height -= 1;
                        let fb = if bg != 0 {
                            !row_bits(glyph, fs, y)
                        } else {
                            row_bits(glyph, fs, y)
                        };
                        let v = (fb_r32(rp) as u32 & lmask) | (mbe(mbl(fb, col as u32)) & rmask);
                        fb_w32(rp, v as i32);

                        y += 1;
                        rp = rp.wrapping_offset(rs);
                    }
                }
            }

            // Do underline
            if attr & WSATTR_UNDERLINE as u32 != 0 {
                rp = rp.wrapping_offset(-(rs << 1));
                fb_w32(rp, ((fb_r32(rp) as u32 & lmask) | (fg & rmask)) as i32);
            }
        } else {
            let lmask = !(RASOPS_LMASK[col as usize] as u32);
            let rmask = !(RASOPS_RMASK[((col + width as i32) & 31) as usize] as u32);

            match glyph {
                None => {
                    width = bg & !rmask;
                    bg &= !lmask;

                    while height != 0 {
                        height -= 1;
                        fb_w32(rp, ((fb_r32(rp) as u32 & lmask) | bg) as i32);
                        let rp1 = rp.add(4);
                        fb_w32(rp1, ((fb_r32(rp1) as u32 & rmask) | width) as i32);
                        rp = rp.wrapping_offset(rs);
                    }
                }
                Some(glyph) => {
                    width = 32 - col as u32;

                    // NOT fontbits if bg is white
                    while height != 0 {
                        height -= 1;
                        let fb = if bg != 0 {
                            !row_bits(glyph, fs, y)
                        } else {
                            row_bits(glyph, fs, y)
                        };

                        let v = (fb_r32(rp) as u32 & lmask) | mbe(mbl(fb, col as u32));
                        fb_w32(rp, v as i32);

                        let rp1 = rp.add(4);
                        let v = (fb_r32(rp1) as u32 & rmask) | (mbe(mbr(fb, width)) & !rmask);
                        fb_w32(rp1, v as i32);

                        y += 1;
                        rp = rp.wrapping_offset(rs);
                    }
                }
            }

            // Do underline
            if attr & WSATTR_UNDERLINE as u32 != 0 {
                rp = rp.wrapping_offset(-(rs << 1));
                fb_w32(rp, ((fb_r32(rp) as u32 & lmask) | (fg & !lmask)) as i32);
                let rp1 = rp.add(4);
                fb_w32(rp1, ((fb_r32(rp1) as u32 & rmask) | (fg & !rmask)) as i32);
            }
        }
    }

    Ok(())
}

/// `rasops1_putchar8`: paint a single character. This is for 8-pixel wide fonts
/// (`!RASOPS_SMALL` and big-endian only).
///
/// # Safety
///
/// As for [`rasops1_putchar`], with an 8 pixel wide font.
pub unsafe fn rasops1_putchar8(
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

    let font = ri.font();
    // SAFETY: the cell is on the screen (the caller's contract).
    let mut rp = unsafe {
        ri.ri_bits
            .get()
            .offset((row * ri.ri_yscale.get() + col * ri.ri_xscale.get()) as isize)
    };
    let mut height = font.fontheight;
    let rs = ri.ri_stride.get() as isize;

    let bg = if attr & 0x000f_0000 != 0 {
        ri.ri_devcmap[1].get()
    } else {
        ri.ri_devcmap[0].get()
    };
    let fg = if attr & 0x0f00_0000 != 0 {
        ri.ri_devcmap[1].get()
    } else {
        ri.ri_devcmap[0].get()
    };

    // SAFETY: one byte on each of the cell's `height` lines, `rs` bytes apart.
    unsafe {
        // If fg and bg match this becomes a space character
        if fg == bg || uc == u32::from(b' ') {
            while height != 0 {
                height -= 1;
                fb_w8(rp, bg as u8);
                rp = rp.wrapping_offset(rs);
            }
        } else {
            let glyph = ri.glyph(uc.wrapping_sub(font.firstchar as u32));
            let fs = font.stride as usize;
            let mut y = 0usize;

            // NOT fontbits if bg is white
            while height != 0 {
                height -= 1;
                let b = glyph.get(y * fs).copied().unwrap_or(0);
                fb_w8(rp, if bg != 0 { !b } else { b });
                y += 1;
                rp = rp.wrapping_offset(rs);
            }
        }

        // Do underline
        if attr & WSATTR_UNDERLINE as u32 != 0 {
            fb_w8(rp.wrapping_offset(-(rs << 1)), fg as u8);
        }
    }

    Ok(())
}

/// `rasops1_putchar16`: paint a single character. This is for 16-pixel wide fonts
/// (`!RASOPS_SMALL` and big-endian only).
///
/// # Safety
///
/// As for [`rasops1_putchar`], with a 16 pixel wide font.
pub unsafe fn rasops1_putchar16(
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

    let font = ri.font();
    // SAFETY: the cell is on the screen (the caller's contract).
    let mut rp = unsafe {
        ri.ri_bits
            .get()
            .offset((row * ri.ri_yscale.get() + col * ri.ri_xscale.get()) as isize)
    };
    let mut height = font.fontheight;
    let rs = ri.ri_stride.get() as isize;

    let bg = if attr & 0x000f_0000 != 0 {
        ri.ri_devcmap[1].get()
    } else {
        ri.ri_devcmap[0].get()
    };
    let fg = if attr & 0x0f00_0000 != 0 {
        ri.ri_devcmap[1].get()
    } else {
        ri.ri_devcmap[0].get()
    };

    // SAFETY: two bytes on each of the cell's `height` lines, `rs` bytes apart.
    unsafe {
        // If fg and bg match this becomes a space character
        if fg == bg || uc == u32::from(b' ') {
            while height != 0 {
                height -= 1;
                fb_w16(rp, bg as i16);
                rp = rp.wrapping_offset(rs);
            }
        } else {
            let glyph = ri.glyph(uc.wrapping_sub(font.firstchar as u32));
            let fs = font.stride as usize;
            let mut y = 0usize;

            // NOT fontbits if bg is white
            while height != 0 {
                height -= 1;
                let b0 = glyph.get(y * fs).copied().unwrap_or(0);
                let b1 = glyph.get(y * fs + 1).copied().unwrap_or(0);
                if bg != 0 {
                    fb_w8(rp, !b0);
                    fb_w8(rp.add(1), !b1);
                } else {
                    fb_w8(rp, b0);
                    fb_w8(rp.add(1), b1);
                }
                y += 1;
                rp = rp.wrapping_offset(rs);
            }
        }

        // Do underline
        if attr & WSATTR_UNDERLINE as u32 != 0 {
            fb_w16(rp.wrapping_offset(-(rs << 1)), fg as i16);
        }
    }

    Ok(())
}

/// `rasops1_erasecols`: erase columns (`rasops_bitops.h` with `PIXEL_SHIFT` 0).
///
/// # Safety
///
/// `cookie` is a configured 1-bit [`RasopsInfo`] whose frame buffer is mapped; the cells are
/// on the screen.
pub unsafe fn rasops1_erasecols(
    cookie: *mut c_void,
    row: i32,
    col: i32,
    num: i32,
    attr: u32,
) -> Result<(), Errno> {
    // SAFETY: the caller's contract.
    unsafe { rasops_bitops::erasecols::<0>(cookie, row, col, num, attr) }
}

/// `rasops1_do_cursor`: actually paint the cursor (`rasops_bitops.h` with `PIXEL_SHIFT` 0).
pub fn rasops1_do_cursor(ri: &RasopsInfo) -> Result<(), Errno> {
    rasops_bitops::do_cursor::<0>(ri)
}

/// `rasops1_copycols`: copy columns (`rasops_bitops.h` with `PIXEL_SHIFT` 0).
///
/// # Safety
///
/// As for [`rasops1_erasecols`].
pub unsafe fn rasops1_copycols(
    cookie: *mut c_void,
    row: i32,
    src: i32,
    dst: i32,
    num: i32,
) -> Result<(), Errno> {
    // SAFETY: the caller's contract.
    unsafe { rasops_bitops::copycols::<0>(cookie, row, src, dst, num) }
}
/* </CODE> */

/* <TESTS> */
#[cfg(test)]
mod tests {
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
        unsafe { rasops1_putchar(ri.cookie(), 1, 2, u32::from(b'A'), 0x0100_0000 | 8) }
            .expect("draws");

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
}
/* </TESTS> */
