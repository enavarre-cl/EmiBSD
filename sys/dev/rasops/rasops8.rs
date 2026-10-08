/*	$OpenBSD: rasops8.c,v 1.12 2023/01/18 11:08:49 nicm Exp $	*/
/*	$NetBSD: rasops8.c,v 1.8 2000/04/12 14:22:29 pk Exp $	*/
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
//! 8-bit-per-pixel raster operations: `dev/rasops/rasops8.c` (`rasops8` in GENERIC).
//!
//! Upstream: sys/dev/rasops/rasops8.c @ 3ce1f3f79392
//!
//! The generic pixel-at-a-time `putchar`, and `putchar` for 8, 12 and 16 pixel wide fonts
//! through a 4x1 stamp (the 16 patterns of four pixels, a 32-bit word each). The row and
//! column operations are rasops's generic ones.
//!
//! ## Deviations
//! - `rasops8_putchar8`, `rasops8_putchar12` and `rasops8_putchar16` are one function
//!   (`putchar_stamp`) with the number of 32-bit words per line (2, 3, 4) as a parameter:
//!   the C's three bodies differ only in how many words they write and in the glyph byte
//!   (`fr[0]` or `fr[1]`) each pair of words comes from. The pixels written are the same.
//!   `STAMP_SHIFT(fb, n) & STAMP_MASK` and `STAMP_READ` (a byte offset into `stamp`, the
//!   nibble times 4) are `stamp[nibble]`.
//! - Upstream bug kept: in `rasops8_putchar16` the loop that paints a space (`while
//!   (height--) rp[0] = rp[1] = rp[2] = rp[3] = stamp[0];`) does not advance `rp` (the 8 and
//!   12 pixel versions do): only the cell's first line is cleared, and an underline then
//!   goes two lines above it. `putchar_stamp`'s `space_advances` is false for the 16 pixel
//!   font to do the same.
//! - The static `stamp[16]`, `stamp_attr` and `stamp_mutex` are atomics (`Relaxed`): the C
//!   draws with them under no lock but the console's (a nested call, from an interrupt,
//!   finds `stamp_mutex` set and takes the generic path), which is what the counter still
//!   does.
//! - The glyph row is read `stride` bytes at a time (at most 4), where the C's generic
//!   `putchar` always reads 4 bytes (`fr[0]`..`fr[3]`), past the row of a font narrower
//!   than 25 pixels; those bits are shifted out only after `width` pixels, so the pixels
//!   drawn are the same.
//! - `RASOPS_SMALL`, `RASOPS_CLIPPING` and `NRASOPS_BSWAP` are not configured; they are
//!   kept behind the constants of the same names (false). `RASOPS_SMALL` is `SMALL_KERNEL`.
//! - Pixels are stored with `write_volatile` (`rasops.rs`'s deviations).

use core::ffi::c_void;
use core::sync::atomic::{AtomicI32, AtomicU32, Ordering};

use crate::dev::rasops::rasops::{
    NRASOPS_BSWAP, RASOPS_CLIPPING, RI_BSWAP, RasopsInfo, fb_w8, fb_w32,
};
use crate::dev::wscons::wsdisplayvar::WSATTR_UNDERLINE;
use crate::sys::errno::Errno;

/// `RASOPS_SMALL`: not configured (`SMALL_KERNEL` is off).
const RASOPS_SMALL: bool = false;

/// `BYTE_ORDER == LITTLE_ENDIAN`.
const LITTLE_ENDIAN: bool = cfg!(target_endian = "little");

/// `stamp`: 4x1 stamp for optimized character blitting. The console draws one character at
/// a time (`stamp_mutex`), under the kernel lock.
static STAMP: [AtomicI32; 16] = [const { AtomicI32::new(0) }; 16];
/// `stamp_attr`: the attribute `STAMP` was made for.
static STAMP_ATTR: AtomicU32 = AtomicU32::new(0);
/// `stamp_mutex`: XXX see note in README.
static STAMP_MUTEX: AtomicI32 = AtomicI32::new(0);

/// `rasops8_init`: initialize a `rasops_info` descriptor for this depth.
pub fn rasops8_init(ri: &RasopsInfo) {
    let mut ops = ri.ri_ops.get();
    match ri.font().fontwidth {
        8 if !RASOPS_SMALL => ops.putchar = Some(rasops8_putchar8),
        12 if !RASOPS_SMALL => ops.putchar = Some(rasops8_putchar12),
        16 if !RASOPS_SMALL => ops.putchar = Some(rasops8_putchar16),
        _ => ops.putchar = Some(rasops8_putchar),
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

/// `rasops8_putchar`: put a single character.
///
/// # Safety
///
/// `cookie` is a configured 8-bit [`RasopsInfo`] whose frame buffer is mapped; the cell is
/// on the screen (without `RASOPS_CLIPPING` the C does not check it either).
pub unsafe fn rasops8_putchar(
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
        ri.ri_devcmap[((attr >> 16) & 0xf) as usize].get() as u8,
        ri.ri_devcmap[((attr >> 24) & 0xf) as usize].get() as u8,
    ];

    // SAFETY: `width` one-byte pixels on each of the cell's `height` lines, `stride` bytes
    // apart, and the line the underline goes back to is the cell's (the caller's contract).
    unsafe {
        if uc == u32::from(b' ') {
            let c = clr[0];

            while height != 0 {
                height -= 1;
                let dp = rp;
                rp = rp.wrapping_offset(stride);

                for cnt in 0..width as usize {
                    fb_w8(dp.add(cnt), c);
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
                    fb_w8(dp.add(cnt), clr[(fb >> 31) as usize]);
                    fb <<= 1;
                }
            }
        }

        // Do underline
        if attr & WSATTR_UNDERLINE as u32 != 0 {
            let c = clr[1];

            rp = rp.wrapping_offset(-(stride << 1));

            while width != 0 {
                width -= 1;
                fb_w8(rp, c);
                rp = rp.add(1);
            }
        }
    }

    Ok(())
}

/// `rasops8_makestamp`: recompute the 4x1 blitting stamp.
pub fn rasops8_makestamp(ri: &RasopsInfo, attr: u32) {
    let fg = ri.ri_devcmap[((attr >> 24) & 0xf) as usize].get() & 0xff;
    let bg = ri.ri_devcmap[((attr >> 16) & 0xf) as usize].get() & 0xff;
    STAMP_ATTR.store(attr, Ordering::Relaxed);

    for (i, s) in STAMP.iter().enumerate() {
        let pick = |bit: usize| if i & bit != 0 { fg } else { bg };
        let mut v = if LITTLE_ENDIAN {
            pick(8) | (pick(4) << 8) | (pick(2) << 16) | (pick(1) << 24)
        } else {
            pick(1) | (pick(2) << 8) | (pick(4) << 16) | (pick(8) << 24)
        };
        if NRASOPS_BSWAP && ri.ri_flg.get() & RI_BSWAP != 0 {
            v = v.swap_bytes();
        }
        s.store(v, Ordering::Relaxed);
    }
}

/// One entry of the stamp.
fn stamp(i: usize) -> i32 {
    STAMP.get(i).map_or(0, |s| s.load(Ordering::Relaxed))
}

/// The body of `rasops8_putchar8` (`nwords` 2), `rasops8_putchar12` (3) and
/// `rasops8_putchar16` (4): `nwords` 32-bit words per line, four pixels each.
/// `space_advances` is whether painting a space goes on to the next line (it does not in
/// the 16 pixel version, see the deviations).
///
/// # Safety
///
/// As for [`rasops8_putchar8`].
unsafe fn putchar_stamp(
    cookie: *mut c_void,
    row: i32,
    col: i32,
    uc: u32,
    attr: u32,
    nwords: usize,
    space_advances: bool,
) -> Result<(), Errno> {
    // Can't risk remaking the stamp if it's already in use
    if STAMP_MUTEX.fetch_add(1, Ordering::Relaxed) != 0 {
        STAMP_MUTEX.fetch_sub(1, Ordering::Relaxed);
        // SAFETY: the caller's contract.
        return unsafe { rasops8_putchar(cookie, row, col, uc, attr) };
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
        rasops8_makestamp(ri, attr);
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

    // SAFETY: `nwords` 32-bit words on each of the cell's `height` lines, `stride` bytes
    // apart (the caller's contract).
    unsafe {
        if uc == u32::from(b' ') {
            while height != 0 {
                height -= 1;
                let c = stamp(0);
                for k in 0..nwords {
                    fb_w32(rp.add(4 * k), c);
                }
                if space_advances {
                    rp = rp.wrapping_offset(stride);
                }
            }
        } else {
            let glyph = ri.glyph(uc.wrapping_sub(font.firstchar as u32));
            let fs = font.stride as usize;
            let mut y = 0usize;

            while height != 0 {
                height -= 1;
                for k in 0..nwords {
                    let byte = glyph.get(y * fs + k / 2).copied().unwrap_or(0);
                    let nibble = if k % 2 == 0 { byte >> 4 } else { byte & 0xf };
                    fb_w32(rp.add(4 * k), stamp(usize::from(nibble)));
                }

                y += 1;
                rp = rp.wrapping_offset(stride);
            }
        }

        // Do underline
        if attr & WSATTR_UNDERLINE as u32 != 0 {
            rp = rp.wrapping_offset(-(stride << 1));
            for k in 0..nwords {
                fb_w32(rp.add(4 * k), stamp(15));
            }
        }
    }

    STAMP_MUTEX.fetch_sub(1, Ordering::Relaxed);

    Ok(())
}

/// `rasops8_putchar8`: put a single character. This is for 8-pixel wide fonts.
///
/// # Safety
///
/// `cookie` is a configured 8-bit [`RasopsInfo`] with an 8 pixel wide font whose frame
/// buffer is mapped; the cell is on the screen (without `RASOPS_CLIPPING` the C does not
/// check it either).
pub unsafe fn rasops8_putchar8(
    cookie: *mut c_void,
    row: i32,
    col: i32,
    uc: u32,
    attr: u32,
) -> Result<(), Errno> {
    // SAFETY: the caller's contract.
    unsafe { putchar_stamp(cookie, row, col, uc, attr, 2, true) }
}

/// `rasops8_putchar12`: put a single character. This is for 12-pixel wide fonts.
///
/// # Safety
///
/// As for [`rasops8_putchar8`], with a 12 pixel wide font.
pub unsafe fn rasops8_putchar12(
    cookie: *mut c_void,
    row: i32,
    col: i32,
    uc: u32,
    attr: u32,
) -> Result<(), Errno> {
    // SAFETY: the caller's contract.
    unsafe { putchar_stamp(cookie, row, col, uc, attr, 3, true) }
}

/// `rasops8_putchar16`: put a single character. This is for 16-pixel wide fonts.
///
/// # Safety
///
/// As for [`rasops8_putchar8`], with a 16 pixel wide font.
pub unsafe fn rasops8_putchar16(
    cookie: *mut c_void,
    row: i32,
    col: i32,
    uc: u32,
    attr: u32,
) -> Result<(), Errno> {
    // SAFETY: the caller's contract.
    unsafe { putchar_stamp(cookie, row, col, uc, attr, 4, false) }
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

    /// Checks the cell at (`row`, `col`) of an 'A' painted with `attr(ri)`, then the underline.
    fn check_cell(ri: &RasopsInfo, b: &[u8], row: usize, col: usize) {
        let font = ri.font();
        let (fw, fh, fs) = (
            font.fontwidth as usize,
            font.fontheight as usize,
            font.stride as usize,
        );
        let stride = ri.ri_stride.get() as usize;
        let (white, blue) = (ri.ri_devcmap[7].get() as u8, ri.ri_devcmap[4].get() as u8);
        let glyph = ri.glyph(u32::from(b'A') - 32).to_vec();
        for y in 0..fh - 2 {
            for x in 0..fw {
                let set = glyph[y * fs + x / 8] & (0x80 >> (x % 8)) != 0;
                let px = b[(row * fh + y) * stride + col * fw + x];
                assert_eq!(px, if set { white } else { blue }, "pixel {x},{y}");
            }
        }
        for x in 0..fw {
            assert_eq!(
                b[(row * fh + fh - 2) * stride + col * fw + x],
                white,
                "underline {x}"
            );
        }
    }

    #[test]
    fn the_stamp_putchar_draws_a_glyph_and_its_underline() {
        let (ri, fb) = test_ri(8, 640, 480);
        assert_eq!(ri.font().fontwidth, 8);
        // SAFETY: the test descriptor and its frame buffer.
        unsafe { rasops8_putchar8(ri.cookie(), 1, 2, u32::from(b'A'), attr(ri)) }.expect("draws");
        check_cell(ri, &bytes(fb.words()), 1, 2);
    }

    #[test]
    fn the_generic_putchar_draws_the_same() {
        let (ri, fb) = test_ri(8, 640, 480);
        // SAFETY: the test descriptor and its frame buffer.
        unsafe { rasops8_putchar(ri.cookie(), 1, 2, u32::from(b'A'), attr(ri)) }.expect("draws");
        check_cell(ri, &bytes(fb.words()), 1, 2);

        let (ri2, fb2) = test_ri(8, 640, 480);
        // SAFETY: the test descriptor and its frame buffer.
        unsafe { rasops8_putchar8(ri2.cookie(), 1, 2, u32::from(b'A'), attr(ri2)) }.expect("draws");
        assert_eq!(fb.words(), fb2.words());
    }

    #[test]
    fn a_twelve_pixel_font() {
        let (ri, fb) = test_ri(8, 1280, 800);
        assert_eq!(ri.font().fontwidth, 12);
        // SAFETY: the test descriptor and its frame buffer.
        unsafe { rasops8_putchar12(ri.cookie(), 2, 3, u32::from(b'A'), attr(ri)) }.expect("draws");
        check_cell(ri, &bytes(fb.words()), 2, 3);
    }

    #[test]
    fn a_space_is_the_background() {
        let (ri, fb) = test_ri(8, 640, 480);
        // SAFETY: the test descriptor and its frame buffer.
        unsafe { rasops8_putchar8(ri.cookie(), 0, 0, u32::from(b' '), attr(ri)) }.expect("draws");
        let b = bytes(fb.words());
        let stride = ri.ri_stride.get() as usize;
        let blue = ri.ri_devcmap[4].get() as u8;
        for y in 0..14 {
            for x in 0..8 {
                assert_eq!(b[y * stride + x], blue);
            }
        }
    }

    #[test]
    fn a_sixteen_pixel_font() {
        // The 16-pixel font, when the geometry picks it; its space paints the first line only
        // (the upstream bug in the deviations), a glyph is painted whole.
        let (ri, fb) = test_ri(8, 2560, 1440);
        if ri.font().fontwidth != 16 {
            return;
        }
        // SAFETY: the test descriptor and its frame buffer.
        unsafe { rasops8_putchar16(ri.cookie(), 1, 1, u32::from(b'A'), attr(ri)) }.expect("draws");
        check_cell(ri, &bytes(fb.words()), 1, 1);
    }
}
/* </TESTS> */
