/*	$OpenBSD: rasops32.c,v 1.14 2024/07/21 13:18:15 fcambus Exp $	*/
/*	$NetBSD: rasops32.c,v 1.7 2000/04/12 14:22:29 pk Exp $	*/
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

//! 32-bit-per-pixel raster operations: `dev/rasops/rasops32.c` (`rasops32` in GENERIC).
//!
//! Upstream: sys/dev/rasops/rasops32.c @ 3ce1f3f79392
//!
//! Only `putchar` is depth-specific; the row and column operations are rasops's generic
//! word-wide ones.
//!
//! ## Deviations
//! - `rasops32_putchar`'s double-pixel special cases for the common widths (6, 8, 12, 16 and
//!   32 pixels: a table of 64-bit words, two pixels per lookup) are the C's per-pixel default
//!   loop for every width: the pixels written are the same. The glyph row is read `stride`
//!   bytes at a time (at most 4), where the C's default case reads 4 bytes whatever the
//!   stride.
//! - Pixels are stored with `write_volatile` (`rasops.rs`'s deviations).

use core::ffi::c_void;

use crate::dev::rasops::rasops::{RASOPS_CLIPPING, RasopsInfo, fb_w32};
use crate::dev::wscons::wsdisplayvar::WSATTR_UNDERLINE;
use crate::sys::errno::Errno;

/// `rasops32_init`: initialize a `rasops_info` descriptor for this depth.
pub fn rasops32_init(ri: &RasopsInfo) {
    if ri.ri_rnum.get() == 0 {
        ri.ri_rnum.set(8);
        ri.ri_rpos.set(0);
        ri.ri_gnum.set(8);
        ri.ri_gpos.set(8);
        ri.ri_bnum.set(8);
        ri.ri_bpos.set(16);
    }

    let mut ops = ri.ri_ops.get();
    ops.putchar = Some(rasops32_putchar);
    ri.ri_ops.set(ops);
}

/// `rasops32_putchar`: paint a single character.
///
/// # Safety
///
/// `cookie` is a configured 32-bit [`RasopsInfo`] whose frame buffer is mapped; the cell is
/// on the screen (without `RASOPS_CLIPPING` the C does not check it either).
pub unsafe fn rasops32_putchar(
    cookie: *mut c_void,
    row: i32,
    col: i32,
    mut uc: u32,
    attr: u32,
) -> Result<(), Errno> {
    // SAFETY: the caller's contract.
    let ri = unsafe { &*cookie.cast::<RasopsInfo>() };

    if RASOPS_CLIPPING
        && (row as u32 >= ri.ri_rows.get() as u32 || col as u32 >= ri.ri_cols.get() as u32)
    {
        // Catches 'row < 0' case too
        return Ok(());
    }

    // SAFETY: the cell is on the screen (the caller's contract).
    let mut rp = unsafe {
        ri.ri_bits
            .get()
            .add((row * ri.ri_yscale.get() + col * ri.ri_xscale.get()) as usize)
    };

    let font = ri.font();
    let height = font.fontheight as usize;
    let width = font.fontwidth as usize;
    let stride = ri.ri_stride.get() as usize;

    let b = ri.ri_devcmap[((attr >> 16) & 0xf) as usize].get();
    let f = ri.ri_devcmap[((attr >> 24) & 0xf) as usize].get();

    // SAFETY: every pixel written is in the cell: `width` 4-byte pixels on each of its
    // `height` lines, `stride` bytes apart.
    unsafe {
        if uc == u32::from(b' ') {
            for _ in 0..height {
                // the general, pixel-at-a-time case is fast enough
                for cnt in 0..width {
                    fb_w32(rp.add(cnt * 4), b);
                }
                rp = rp.add(stride);
            }
        } else {
            uc -= font.firstchar as u32;
            let glyph = ri.glyph(uc);
            let fs = font.stride as usize;
            let clr = [b, f];

            for line in glyph.chunks_exact(fs).take(height) {
                // The row's bits, leftmost pixel in the most significant bit.
                let fb = line
                    .iter()
                    .take(4)
                    .enumerate()
                    .fold(0u32, |acc, (i, &byte)| {
                        acc | u32::from(byte) << (24 - 8 * i)
                    });

                for cnt in 0..width {
                    fb_w32(rp.add(cnt * 4), clr[((fb << cnt) >> 31) as usize]);
                }
                rp = rp.add(stride);
            }
        }

        // Do underline a pixel at a time
        if attr & WSATTR_UNDERLINE as u32 != 0 {
            rp = rp.sub(stride);
            for cnt in 0..width {
                fb_w32(rp.add(cnt * 4), f);
            }
        }
    }

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    use crate::dev::rasops::rasops::tests::test_ri;

    #[test]
    fn draws_a_glyph_and_its_underline() {
        // 32 bpp, 8x16 font: an 'A' in white on blue at cell (1, 2).
        let (ri, fb) = test_ri(32, 640, 480);
        let attr = ri.ri_pack_attr(7, 4, 0x10 | 8).expect("colours pack"); // WSCOLORS|UNDERLINE
        let white = ri.ri_devcmap[7].get();
        let blue = ri.ri_devcmap[4].get();
        // SAFETY: the test descriptor and its frame buffer.
        unsafe { rasops32_putchar(ri.cookie(), 1, 2, u32::from(b'A'), attr) }.expect("draws");

        let stride = ri.ri_stride.get() as usize / 4;
        let w = fb.words();
        let px = |x: usize, y: usize| w[(16 + y) * stride + 16 + x];
        let glyph = ri.glyph(u32::from(b'A') - 32).to_vec();
        for y in 0..15 {
            for x in 0..8 {
                let on = glyph[y] & (0x80 >> x) != 0;
                assert_eq!(px(x, y), if on { white } else { blue }, "pixel {x},{y}");
            }
        }
        for x in 0..8 {
            assert_eq!(px(x, 15), white, "underline");
        }
    }
}
