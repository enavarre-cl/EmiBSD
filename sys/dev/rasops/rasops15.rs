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
//! 15 and 16-bit-per-pixel raster operations: `dev/rasops/rasops15.c` (`rasops15` and
//! `rasops16` in GENERIC).
//!
//! Upstream: sys/dev/rasops/rasops15.c @ 3ce1f3f79392
//!
//! The generic pixel-at-a-time `putchar`, and `putchar` for 8, 12 and 16 pixel wide fonts
//! through a (2x2)x1 stamp (the 16 patterns of four pixels, two 32-bit words each). The row
//! and column operations are rasops's generic ones.
//!
//! ## Deviations
//! - `rasops15_putchar8`, `rasops15_putchar12` and `rasops15_putchar16` are one function
//!   (`putchar_stamp`) with the number of glyph nibbles per line (2, 3, 4) as a parameter:
//!   the C's three bodies differ only in how many stamp pairs they write and in the glyph
//!   byte (`fr[0]` or `fr[1]`) each comes from. The pixels written are the same.
//!   `STAMP_SHIFT(fb, n) & STAMP_MASK` and `STAMP_READ(so)`, `STAMP_READ(so + 4)` (byte
//!   offsets into `stamp`, the nibble times 8) are `stamp[2 * nibble]` and
//!   `stamp[2 * nibble + 1]`; `STAMP_READ(28)` of the underline is `stamp[7]`.
//! - The fast paths' `uc == (u_int)-1` test for a space (which no caller passes: a space
//!   is drawn from the font's glyph for it) is kept as it is.
//! - The static `stamp[32]`, `stamp_attr` and `stamp_mutex` are atomics (`Relaxed`): the C
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

use crate::dev::rasops::rasops::{RASOPS_CLIPPING, RasopsInfo, fb_w16, fb_w32};
use crate::dev::wscons::wsdisplayvar::WSATTR_UNDERLINE;
use crate::sys::errno::Errno;

/// `RASOPS_SMALL`: not configured (`SMALL_KERNEL` is off).
const RASOPS_SMALL: bool = false;

/// `BYTE_ORDER == LITTLE_ENDIAN`.
const LITTLE_ENDIAN: bool = cfg!(target_endian = "little");

/// `stamp`: (2x2)x1 stamp for optimized character blitting. The console draws one character
/// at a time (`stamp_mutex`), under the kernel lock.
static STAMP: [AtomicI32; 32] = [const { AtomicI32::new(0) }; 32];
/// `stamp_attr`: the attribute `STAMP` was made for.
static STAMP_ATTR: AtomicU32 = AtomicU32::new(0);
/// `stamp_mutex`: XXX see note in readme.
static STAMP_MUTEX: AtomicI32 = AtomicI32::new(0);

/// `rasops15_init`: initialize a `rasops_info` descriptor for this depth.
pub fn rasops15_init(ri: &RasopsInfo) {
    let mut ops = ri.ri_ops.get();
    match ri.font().fontwidth {
        8 if !RASOPS_SMALL => ops.putchar = Some(rasops15_putchar8),
        12 if !RASOPS_SMALL => ops.putchar = Some(rasops15_putchar12),
        16 if !RASOPS_SMALL => ops.putchar = Some(rasops15_putchar16),
        _ => ops.putchar = Some(rasops15_putchar),
    }
    ri.ri_ops.set(ops);

    if ri.ri_rnum.get() == 0 {
        let is16 = u8::from(ri.ri_depth.get() == 16);
        ri.ri_rnum.set(5);
        ri.ri_rpos.set(0);
        ri.ri_gnum.set(5 + is16);
        ri.ri_gpos.set(5);
        ri.ri_bnum.set(5);
        ri.ri_bpos.set(10 + is16);
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

/// `rasops15_putchar`: paint a single character.
///
/// # Safety
///
/// `cookie` is a configured 15 or 16-bit [`RasopsInfo`] whose frame buffer is mapped; the
/// cell is on the screen (without `RASOPS_CLIPPING` the C does not check it either).
pub unsafe fn rasops15_putchar(
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

    // SAFETY: `width` two-byte pixels on each of the cell's `height` lines, `stride` bytes
    // apart, and the line the underline goes back to is the cell's (the caller's contract).
    unsafe {
        if uc == u32::from(b' ') {
            let c = clr[0] as i16;

            while height != 0 {
                height -= 1;
                let dp = rp;
                rp = rp.wrapping_offset(stride);

                for cnt in 0..width as usize {
                    fb_w16(dp.add(2 * cnt), c);
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
                    fb_w16(dp.add(2 * cnt), clr[(fb >> 31) as usize] as i16);
                    fb <<= 1;
                }
            }
        }

        // Do underline
        if attr & WSATTR_UNDERLINE as u32 != 0 {
            let c = clr[1] as i16;
            rp = rp.wrapping_offset(-(stride << 1));

            while width != 0 {
                width -= 1;
                fb_w16(rp, c);
                rp = rp.add(2);
            }
        }
    }

    Ok(())
}

/// `rasops15_makestamp`: recompute the (2x2)x1 blitting stamp.
pub fn rasops15_makestamp(ri: &RasopsInfo, attr: u32) {
    let fg = ri.ri_devcmap[((attr >> 24) & 0xf) as usize].get() & 0xffff;
    let bg = ri.ri_devcmap[((attr >> 16) & 0xf) as usize].get() & 0xffff;
    STAMP_ATTR.store(attr, Ordering::Relaxed);

    for i in (0..32).step_by(2) {
        let pick = |bit: usize| if i & bit != 0 { fg } else { bg };
        let (lo, hi) = if LITTLE_ENDIAN {
            (pick(16) | (pick(8) << 16), pick(4) | (pick(2) << 16))
        } else {
            (pick(8) | (pick(16) << 16), pick(2) | (pick(4) << 16))
        };
        STAMP[i].store(lo, Ordering::Relaxed);
        STAMP[i + 1].store(hi, Ordering::Relaxed);
    }
}

/// One entry of the stamp.
fn stamp(i: usize) -> i32 {
    STAMP.get(i).map_or(0, |s| s.load(Ordering::Relaxed))
}

/// The body of `rasops15_putchar8` (`nibbles` 2), `rasops15_putchar12` (3) and
/// `rasops15_putchar16` (4): `nibbles` glyph nibbles per line, two 32-bit words each.
///
/// # Safety
///
/// As for [`rasops15_putchar8`].
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
        return unsafe { rasops15_putchar(cookie, row, col, uc, attr) };
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
        rasops15_makestamp(ri, attr);
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
    let nwords = nibbles * 2;

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
                    fb_w32(rp.add(8 * k), stamp(2 * nibble));
                    fb_w32(rp.add(8 * k + 4), stamp(2 * nibble + 1));
                }

                y += 1;
                rp = rp.wrapping_offset(stride);
            }
        }

        // Do underline
        if attr & WSATTR_UNDERLINE as u32 != 0 {
            let c = stamp(7);

            rp = rp.wrapping_offset(-(stride << 1));
            for k in 0..nwords {
                fb_w32(rp.add(4 * k), c);
            }
        }
    }

    STAMP_MUTEX.fetch_sub(1, Ordering::Relaxed);

    Ok(())
}

/// `rasops15_putchar8`: paint a single character. This is for 8-pixel wide fonts.
///
/// # Safety
///
/// `cookie` is a configured 15 or 16-bit [`RasopsInfo`] with an 8 pixel wide font whose
/// frame buffer is mapped; the cell is on the screen (without `RASOPS_CLIPPING` the C does
/// not check it either).
pub unsafe fn rasops15_putchar8(
    cookie: *mut c_void,
    row: i32,
    col: i32,
    uc: u32,
    attr: u32,
) -> Result<(), Errno> {
    // SAFETY: the caller's contract.
    unsafe { putchar_stamp(cookie, row, col, uc, attr, 2) }
}

/// `rasops15_putchar12`: paint a single character. This is for 12-pixel wide fonts.
///
/// # Safety
///
/// As for [`rasops15_putchar8`], with a 12 pixel wide font.
pub unsafe fn rasops15_putchar12(
    cookie: *mut c_void,
    row: i32,
    col: i32,
    uc: u32,
    attr: u32,
) -> Result<(), Errno> {
    // SAFETY: the caller's contract.
    unsafe { putchar_stamp(cookie, row, col, uc, attr, 3) }
}

/// `rasops15_putchar16`: paint a single character. This is for 16-pixel wide fonts.
///
/// # Safety
///
/// As for [`rasops15_putchar8`], with a 16 pixel wide font.
pub unsafe fn rasops15_putchar16(
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
        unsafe { rasops15_putchar8(ri2.cookie(), 1, 2, u32::from(b'A'), attr(ri2)) }
            .expect("draws");
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
}
/* </TESTS> */
