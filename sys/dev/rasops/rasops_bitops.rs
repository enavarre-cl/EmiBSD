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
//! Column operations for the depths below 8 bits: `dev/rasops/rasops_bitops.h`.
//!
//! Upstream: sys/dev/rasops/rasops_bitops.h @ 3ce1f3f79392
//!
//! `rasops_bitops.h` is a template the C includes into `rasops1.c` and `rasops4.c`, with
//! `NAME(ident)` naming the functions and `PIXEL_SHIFT` the log2 of the bits per pixel.
//! It defines `erasecols`, `do_cursor` and `copycols` for runs of pixels that do not fall on
//! a 32-bit boundary.
//!
//! ## Deviations
//! - The template is generic functions over `const PIXEL_SHIFT: u32` ([`erasecols`],
//!   [`do_cursor`], [`copycols`]); `rasops1.rs` and `rasops4.rs` define `rasops1_erasecols`,
//!   `rasops4_copycols` and the rest as one-line calls to them (`NAME(erasecols)`).
//! - `GETBITS`/`PUTBITS` are the functions of `rasops_masks.rs`. The pixels are read and
//!   written with `write_volatile` (`rasops.rs`'s deviations). Row pointers are advanced
//!   with wrapping arithmetic: the C's `DELTA` after the last row leaves them one row past
//!   the cell, which is not dereferenced.
//! - `RASOPS_CLIPPING` is not configured; its checks are kept behind the
//!   `RASOPS_CLIPPING` constant (false).

use core::ffi::c_void;

use crate::dev::rasops::rasops::{RASOPS_CLIPPING, RasopsInfo, fb_r32, fb_w32};
use crate::dev::rasops::rasops_masks::{RASOPS_LMASK, RASOPS_RMASK, getbits, pmask, putbits};
use crate::sys::errno::Errno;

/// A pointer `n` bytes on (`DELTA`).
fn delta(p: *mut u8, n: isize) -> *mut u8 {
    p.wrapping_offset(n)
}

/// `NAME(erasecols)`: erase columns.
///
/// # Safety
///
/// `cookie` is a configured [`RasopsInfo`] of a depth below 8 bits whose frame buffer is
/// mapped, `PIXEL_SHIFT` is the log2 of its bits per pixel, and the cells are on the
/// screen (without `RASOPS_CLIPPING` the C does not check them either).
pub(crate) unsafe fn erasecols<const PIXEL_SHIFT: u32>(
    cookie: *mut c_void,
    row: i32,
    mut col: i32,
    mut num: i32,
    attr: u32,
) -> Result<(), Errno> {
    // SAFETY: the caller's contract.
    let ri = unsafe { &*cookie.cast::<RasopsInfo>() };

    if RASOPS_CLIPPING {
        if row as u32 >= ri.ri_rows.get() as u32 {
            return Ok(());
        }

        if col < 0 {
            num += col;
            col = 0;
        }

        if (col + num) > ri.ri_cols.get() {
            num = ri.ri_cols.get() - col;
        }

        if num <= 0 {
            return Ok(());
        }
    }

    let font = ri.font();
    col *= (font.fontwidth << PIXEL_SHIFT) as i32;
    num *= (font.fontwidth << PIXEL_SHIFT) as i32;
    let mut height = font.fontheight;
    let clr = ri.ri_devcmap[((attr >> 16) & 0xf) as usize].get();
    let stride = ri.ri_stride.get() as isize;
    let mut rp = delta(
        ri.ri_bits.get(),
        (row * ri.ri_yscale.get() + ((col >> 3) & !3)) as isize,
    );

    // SAFETY: every word touched holds pixels of the cell (the caller's contract): one
    // word, or the run from the left edge's word to the right edge's, on each line.
    unsafe {
        if (col & 31) + num <= 32 {
            let lmask = !pmask(col & 31, num);
            let lclr = clr & !lmask;

            while height != 0 {
                height -= 1;
                let dp = rp;
                rp = delta(rp, stride);

                fb_w32(dp, (fb_r32(dp) & lmask) | lclr);
            }
        } else {
            let lmask = RASOPS_RMASK[(col & 31) as usize];
            let rmask = RASOPS_LMASK[((col + num) & 31) as usize];

            if lmask != 0 {
                num = (num - (32 - (col & 31))) >> 5;
            } else {
                num >>= 5;
            }

            let lclr = clr & !lmask;
            let rclr = clr & !rmask;

            while height != 0 {
                height -= 1;
                let mut dp = rp;
                rp = delta(rp, stride);

                if lmask != 0 {
                    fb_w32(dp, (fb_r32(dp) & lmask) | lclr);
                    dp = dp.add(4);
                }

                for _ in 0..num {
                    fb_w32(dp, clr);
                    dp = dp.add(4);
                }

                if rmask != 0 {
                    fb_w32(dp, (fb_r32(dp) & rmask) | rclr);
                }
            }
        }
    }

    Ok(())
}

/// `NAME(do_cursor)`: actually paint the cursor.
pub(crate) fn do_cursor<const PIXEL_SHIFT: u32>(ri: &RasopsInfo) -> Result<(), Errno> {
    let font = ri.font();
    let row = ri.ri_crow.get();
    let col = (ri.ri_ccol.get() * font.fontwidth as i32) << PIXEL_SHIFT;
    let mut height = font.fontheight;
    let num = (font.fontwidth << PIXEL_SHIFT) as i32;
    let stride = ri.ri_stride.get() as isize;
    let mut rp = delta(
        ri.ri_bits.get(),
        (row * ri.ri_yscale.get() + ((col >> 3) & !3)) as isize,
    );

    // SAFETY: the cursor's cell is on the screen: the words touched are those of its
    // pixels on each line.
    unsafe {
        if (col & 31) + num <= 32 {
            let lmask = pmask(col & 31, num);

            while height != 0 {
                height -= 1;
                let dp = rp;
                rp = delta(rp, stride);
                fb_w32(dp, fb_r32(dp) ^ lmask);
            }
        } else {
            let lmask = !RASOPS_RMASK[(col & 31) as usize];
            let rmask = !RASOPS_LMASK[((col + num) & 31) as usize];

            while height != 0 {
                height -= 1;
                let mut dp = rp;
                rp = delta(rp, stride);

                if lmask != -1 {
                    fb_w32(dp, fb_r32(dp) ^ lmask);
                    dp = dp.add(4);
                }

                if rmask != -1 {
                    fb_w32(dp, fb_r32(dp) ^ rmask);
                }
            }
        }
    }

    Ok(())
}

/// `NAME(copycols)`: copy columns. Ick!
///
/// # Safety
///
/// As for [`erasecols`].
pub(crate) unsafe fn copycols<const PIXEL_SHIFT: u32>(
    cookie: *mut c_void,
    mut row: i32,
    mut src: i32,
    mut dst: i32,
    mut num: i32,
) -> Result<(), Errno> {
    // SAFETY: the caller's contract.
    let ri = unsafe { &*cookie.cast::<RasopsInfo>() };

    if RASOPS_CLIPPING {
        if dst == src {
            return Ok(());
        }

        // Catches < 0 case too
        if row as u32 >= ri.ri_rows.get() as u32 {
            return Ok(());
        }

        if src < 0 {
            num += src;
            src = 0;
        }

        if (src + num) > ri.ri_cols.get() {
            num = ri.ri_cols.get() - src;
        }

        if dst < 0 {
            num += dst;
            dst = 0;
        }

        if (dst + num) > ri.ri_cols.get() {
            num = ri.ri_cols.get() - dst;
        }

        if num <= 0 {
            return Ok(());
        }
    }

    let font = ri.font();
    let cnt = (font.fontwidth << PIXEL_SHIFT) as i32;
    src *= cnt;
    dst *= cnt;
    num *= cnt;
    row *= ri.ri_yscale.get();
    let mut height = font.fontheight;
    let mut db = dst & 31;
    let stride = ri.ri_stride.get() as isize;
    let bits = ri.ri_bits.get();

    // SAFETY: every word read or written holds pixels of the source or destination cells
    // (the caller's contract), as in the C.
    unsafe {
        if db + num <= 32 {
            // Destination is contained within a single word
            let mut srp = delta(bits, (row + ((src >> 3) & !3)) as isize);
            let mut drp = delta(bits, (row + ((dst >> 3) & !3)) as isize);
            let sb = src & 31;

            while height != 0 {
                height -= 1;
                let tmp = getbits(srp, sb as u32, num as u32);
                putbits(tmp, db as u32, num as u32, drp);
                srp = delta(srp, stride);
                drp = delta(drp, stride);
            }

            return Ok(());
        }

        let lmask = RASOPS_RMASK[db as usize];
        let rmask = RASOPS_LMASK[((dst + num) & 31) as usize];
        let lnum = (32 - db) & 31;
        let mut rnum = (dst + num) & 31;

        let full = if lmask != 0 {
            (num - (32 - (dst & 31))) >> 5
        } else {
            num >> 5
        };

        if src < dst && src + num > dst {
            // Copy right-to-left
            let sb = src & 31;
            src += num;
            dst += num;
            let mut srp = delta(bits, (row + ((src >> 3) & !3)) as isize);
            let mut drp = delta(bits, (row + ((dst >> 3) & !3)) as isize);

            src &= 31;
            rnum = 32 - lnum;
            db = dst & 31;

            src -= db;
            if src < 0 {
                src += 32;
            }

            while height != 0 {
                height -= 1;
                let mut sp = srp;
                let mut dp = drp;
                srp = delta(srp, stride);
                drp = delta(drp, stride);

                if db != 0 {
                    let tmp = getbits(sp, src as u32, db as u32);
                    putbits(tmp, 0, db as u32, dp);
                    dp = delta(dp, -4);
                    sp = delta(sp, -4);
                }

                // Now aligned to 32-bits wrt dp
                for _ in 0..full {
                    let tmp = getbits(sp, src as u32, 32);
                    fb_w32(dp, tmp as i32);
                    dp = delta(dp, -4);
                    sp = delta(sp, -4);
                }

                if lmask != 0 {
                    // The C has `if (src > sb) sp++;` here under `#if 0`.
                    let tmp = getbits(sp, sb as u32, lnum as u32);
                    putbits(tmp, rnum as u32, lnum as u32, dp);
                }
            }
        } else {
            // Copy left-to-right
            let mut srp = delta(bits, (row + ((src >> 3) & !3)) as isize);
            let mut drp = delta(bits, (row + ((dst >> 3) & !3)) as isize);
            db = dst & 31;

            while height != 0 {
                height -= 1;
                let mut sb = src & 31;
                let mut sp = srp;
                let mut dp = drp;
                srp = delta(srp, stride);
                drp = delta(drp, stride);

                if lmask != 0 {
                    let tmp = getbits(sp, sb as u32, lnum as u32);
                    putbits(tmp, db as u32, lnum as u32, dp);
                    dp = delta(dp, 4);

                    sb += lnum;
                    if sb > 31 {
                        sp = delta(sp, 4);
                        sb -= 32;
                    }
                }

                // Now aligned to 32-bits wrt dp
                for _ in 0..full {
                    let tmp = getbits(sp, sb as u32, 32);
                    fb_w32(dp, tmp as i32);
                    dp = delta(dp, 4);
                    sp = delta(sp, 4);
                }

                if rmask != 0 {
                    let tmp = getbits(sp, sb as u32, rnum as u32);
                    putbits(tmp, 0, rnum as u32, dp);
                }
            }
        }
    }

    Ok(())
}
/* </CODE> */

/* <TESTS> */
#[cfg(test)]
mod tests {
    use super::*;

    use crate::dev::rasops::rasops::tests::test_ri;

    #[test]
    fn erase_and_cursor_on_one_bit_cells() {
        // 1 bpp, 8x16 font; columns 3 and 4 are bits 24..40 of the line: the two-word path.
        let (ri, fb) = test_ri(1, 640, 480);
        let attr = ri.ri_pack_attr(0, 0, 0).expect("attr packs");
        // SAFETY: the test descriptor and its frame buffer.
        unsafe { erasecols::<0>(ri.cookie(), 2, 3, 2, attr) }.expect("erases");
        let stride = ri.ri_stride.get() as usize / 4;
        let w = fb.words();
        let word = |y: usize, i: usize| w[(2 * 16 + y) * stride + i] as u32;
        let old = 0x5a5a_5a5au32;
        let in0 = !(RASOPS_RMASK[24] as u32); // the bits of word 0 that are in the run
        let in1 = !(RASOPS_LMASK[8] as u32); // and of word 1
        assert_eq!(in0.count_ones(), 8);
        assert_eq!(in1.count_ones(), 8);
        // The background of black is 0: the run is cleared, the rest is as it was.
        assert_eq!(word(0, 0), old & !in0);
        assert_eq!(word(0, 1), old & !in1);
        assert_eq!(word(15, 0), old & !in0);
        // The line below the cell, and the next word, are untouched.
        assert_eq!(word(16, 0), old);
        assert_eq!(word(0, 2), old);

        // The cursor of column 3 inverts the bits of its cell, those of word 0.
        ri.ri_crow.set(2);
        ri.ri_ccol.set(3);
        do_cursor::<0>(ri).expect("cursor");
        let w = fb.words();
        let word = |y: usize, i: usize| w[(2 * 16 + y) * stride + i] as u32;
        assert_eq!(word(0, 0), (old & !in0) | in0);
        assert_eq!(word(0, 1), old & !in1);
    }
}
/* </TESTS> */
