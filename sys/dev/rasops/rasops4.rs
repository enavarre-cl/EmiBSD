/*	$OpenBSD: rasops4.c,v 1.13 2023/01/18 11:08:49 nicm Exp $	*/
/*	$NetBSD: rasops4.c,v 1.4 2001/11/15 09:48:15 lukem Exp $	*/
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
//! 4-bit-per-pixel raster operations: `dev/rasops/rasops4.c` (no amd64 or arm64 driver).
//!
//! Upstream: sys/dev/rasops/rasops4.c @ 3ce1f3f79392
//!
//! `putchar` for 8, 12 and 16 pixel wide fonts through a 4x1 stamp (the 16 patterns of four
//! pixels), and, through `rasops_bitops`, the column operations for fonts whose width is
//! odd.
//!
//! ## Deviations
//! - `rasops4_putchar8`, `rasops4_putchar12` and `rasops4_putchar16` are one function
//!   (`putchar_stamp`) with the number of 16-bit words per line (2, 3, 4) as a parameter:
//!   the C's three bodies differ only in how many words they write (`rp[0]`..`rp[3]`, the
//!   glyph byte `fr[0]` or `fr[1]` each pair of words comes from). The pixels written are
//!   the same.
//! - The static `stamp[16]`, `stamp_attr` and `stamp_mutex` are atomics (`Relaxed`): the C
//!   draws with them under no lock but the console's (a nested call, from an interrupt,
//!   finds `stamp_mutex` set and takes the generic path), which is what the counter still
//!   does.
//! - `rasops4_init` panics on a font width other than 8, 12 or 16 (no generic `putchar`
//!   exists yet); the C's statement after the `panic()` (`ri->ri_ops.putchar =
//!   rasops4_putchar`) never runs and is left out. `rasops4_putchar`, the generic
//!   version, is the C's `return (EAGAIN)` stub ("XXX punt").
//! - The `#ifdef notyet` generic `rasops4_putchar` of the C (an unfinished body with
//!   `/* get bits, mask */` placeholders) is not compiled in the C and is not ported.
//! - `rasops4_erasecols`, `rasops4_copycols` and `rasops4_do_cursor` are the
//!   `rasops_bitops.h` template instantiated with `PIXEL_SHIFT` 2: one-line calls to the
//!   generic functions of `rasops_bitops.rs`.
//! - `RASOPS_SMALL` and `RASOPS_CLIPPING` are not configured; they are kept behind the
//!   constants of the same names (false). `RASOPS_SMALL` is `SMALL_KERNEL`, off.
//! - Pixels are stored with `write_volatile` (`rasops.rs`'s deviations).

use core::ffi::c_void;
use core::sync::atomic::{AtomicI32, AtomicU16, AtomicU32, Ordering};

use crate::dev::rasops::rasops::{RASOPS_CLIPPING, RasopsInfo, fb_w16};
use crate::dev::rasops::rasops_bitops;
use crate::dev::rasops::rasops_masks::rasops_masks_init;
use crate::dev::wscons::wsdisplayvar::WSATTR_UNDERLINE;
use crate::kern::subr_prf::panic;
use crate::sys::errno::Errno;

/// `RASOPS_SMALL`: not configured (`SMALL_KERNEL` is off).
const RASOPS_SMALL: bool = false;

/// `BYTE_ORDER == LITTLE_ENDIAN`.
const LITTLE_ENDIAN: bool = cfg!(target_endian = "little");

/// `stamp`: 4x1 stamp for optimized character blitting. The console draws one character at
/// a time (`stamp_mutex`), under the kernel lock.
static STAMP: [AtomicU16; 16] = [const { AtomicU16::new(0) }; 16];
/// `stamp_attr`: the attribute `STAMP` was made for.
static STAMP_ATTR: AtomicU32 = AtomicU32::new(0);
/// `stamp_mutex`: XXX see note in README.
static STAMP_MUTEX: AtomicI32 = AtomicI32::new(0);

/// `rasops4_init`: initialize a `rasops_info` descriptor for this depth.
pub fn rasops4_init(ri: &RasopsInfo) {
    rasops_masks_init();

    let mut ops = ri.ri_ops.get();
    match ri.font().fontwidth {
        8 if !RASOPS_SMALL => ops.putchar = Some(rasops4_putchar8),
        12 if !RASOPS_SMALL => ops.putchar = Some(rasops4_putchar12),
        16 if !RASOPS_SMALL => ops.putchar = Some(rasops4_putchar16),
        _ => panic(format_args!(
            "fontwidth not 8/12/16 or RASOPS_SMALL - fixme!"
        )),
    }

    if ri.font().fontwidth & 1 != 0 {
        ops.erasecols = Some(rasops4_erasecols);
        ops.copycols = Some(rasops4_copycols);
        ri.ri_do_cursor.set(Some(rasops4_do_cursor));
    }
    ri.ri_ops.set(ops);
}

/// `rasops4_putchar`: put a single character. This is the generic version.
///
/// # Safety
///
/// None: the arguments are not used. It is an `unsafe fn` to be an emulop.
pub unsafe fn rasops4_putchar(
    _cookie: *mut c_void,
    _row: i32,
    _col: i32,
    _uc: u32,
    _attr: u32,
) -> Result<(), Errno> {
    // XXX punt
    Err(Errno::EAGAIN)
}

/// `rasops4_makestamp`: recompute the blitting stamp.
pub fn rasops4_makestamp(ri: &RasopsInfo, attr: u32) {
    let fg = (ri.ri_devcmap[((attr >> 24) & 0xf) as usize].get() & 0xf) as u16;
    let bg = (ri.ri_devcmap[((attr >> 16) & 0xf) as usize].get() & 0xf) as u16;
    STAMP_ATTR.store(attr, Ordering::Relaxed);

    for (i, s) in STAMP.iter().enumerate() {
        let pick = |bit: usize| if i & bit != 0 { fg } else { bg };
        let v = if LITTLE_ENDIAN {
            (pick(1) << 8) | (pick(2) << 12) | pick(4) | (pick(8) << 4)
        } else {
            pick(1) | (pick(2) << 4) | (pick(4) << 8) | (pick(8) << 12)
        };
        s.store(v, Ordering::Relaxed);
    }
}

/// One entry of the stamp.
fn stamp(i: usize) -> i16 {
    STAMP.get(i).map_or(0, |s| s.load(Ordering::Relaxed)) as i16
}

/// The body of `rasops4_putchar8` (`nwords` 2), `rasops4_putchar12` (3) and
/// `rasops4_putchar16` (4): `nwords` 16-bit words per line, four pixels each.
///
/// # Safety
///
/// As for [`rasops4_putchar8`].
unsafe fn putchar_stamp(
    cookie: *mut c_void,
    row: i32,
    col: i32,
    uc: u32,
    attr: u32,
    nwords: usize,
) -> Result<(), Errno> {
    // Can't risk remaking the stamp if it's already in use
    if STAMP_MUTEX.fetch_add(1, Ordering::Relaxed) != 0 {
        STAMP_MUTEX.fetch_sub(1, Ordering::Relaxed);
        // SAFETY: the caller's contract.
        return unsafe { rasops4_putchar(cookie, row, col, uc, attr) };
    }

    // SAFETY: the caller's contract.
    let ri = unsafe { &*cookie.cast::<RasopsInfo>() };

    if RASOPS_CLIPPING {
        // Catches 'row < 0' case too
        if row as u32 >= ri.ri_rows.get() as u32 {
            STAMP_MUTEX.fetch_sub(1, Ordering::Relaxed);
            return Ok(());
        }

        if col as u32 >= ri.ri_cols.get() as u32 {
            STAMP_MUTEX.fetch_sub(1, Ordering::Relaxed);
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

    // Recompute stamp?
    if attr != STAMP_ATTR.load(Ordering::Relaxed) {
        rasops4_makestamp(ri, attr);
    }

    // SAFETY: `nwords` 16-bit words on each of the cell's `height` lines, `rs` bytes apart
    // (the caller's contract), and the two lines the underline goes back over are the
    // cell's.
    unsafe {
        if uc == u32::from(b' ') {
            let c = stamp(0);
            while height != 0 {
                height -= 1;
                for k in 0..nwords {
                    fb_w16(rp.add(2 * k), c);
                }
                rp = rp.wrapping_offset(rs);
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
                    fb_w16(rp.add(2 * k), stamp(usize::from(nibble)));
                }
                y += 1;
                rp = rp.wrapping_offset(rs);
            }
        }

        // Do underline
        if attr & WSATTR_UNDERLINE as u32 != 0 {
            rp = rp.wrapping_offset(-(rs << 1));
            for k in 0..nwords {
                fb_w16(rp.add(2 * k), stamp(15));
            }
        }
    }

    STAMP_MUTEX.fetch_sub(1, Ordering::Relaxed);

    Ok(())
}

/// `rasops4_putchar8`: put a single character. This is for 8-pixel wide fonts.
///
/// # Safety
///
/// `cookie` is a configured 4-bit [`RasopsInfo`] with an 8 pixel wide font whose frame
/// buffer is mapped; the cell is on the screen (without `RASOPS_CLIPPING` the C does not
/// check it either).
pub unsafe fn rasops4_putchar8(
    cookie: *mut c_void,
    row: i32,
    col: i32,
    uc: u32,
    attr: u32,
) -> Result<(), Errno> {
    // SAFETY: the caller's contract.
    unsafe { putchar_stamp(cookie, row, col, uc, attr, 2) }
}

/// `rasops4_putchar12`: put a single character. This is for 12-pixel wide fonts.
///
/// # Safety
///
/// As for [`rasops4_putchar8`], with a 12 pixel wide font.
pub unsafe fn rasops4_putchar12(
    cookie: *mut c_void,
    row: i32,
    col: i32,
    uc: u32,
    attr: u32,
) -> Result<(), Errno> {
    // SAFETY: the caller's contract.
    unsafe { putchar_stamp(cookie, row, col, uc, attr, 3) }
}

/// `rasops4_putchar16`: put a single character. This is for 16-pixel wide fonts.
///
/// # Safety
///
/// As for [`rasops4_putchar8`], with a 16 pixel wide font.
pub unsafe fn rasops4_putchar16(
    cookie: *mut c_void,
    row: i32,
    col: i32,
    uc: u32,
    attr: u32,
) -> Result<(), Errno> {
    // SAFETY: the caller's contract.
    unsafe { putchar_stamp(cookie, row, col, uc, attr, 4) }
}

/// `rasops4_erasecols`: erase columns (`rasops_bitops.h` with `PIXEL_SHIFT` 2).
///
/// # Safety
///
/// `cookie` is a configured 4-bit [`RasopsInfo`] whose frame buffer is mapped; the cells are
/// on the screen.
pub unsafe fn rasops4_erasecols(
    cookie: *mut c_void,
    row: i32,
    col: i32,
    num: i32,
    attr: u32,
) -> Result<(), Errno> {
    // SAFETY: the caller's contract.
    unsafe { rasops_bitops::erasecols::<2>(cookie, row, col, num, attr) }
}

/// `rasops4_do_cursor`: actually paint the cursor (`rasops_bitops.h` with `PIXEL_SHIFT` 2).
pub fn rasops4_do_cursor(ri: &RasopsInfo) -> Result<(), Errno> {
    rasops_bitops::do_cursor::<2>(ri)
}

/// `rasops4_copycols`: copy columns (`rasops_bitops.h` with `PIXEL_SHIFT` 2).
///
/// # Safety
///
/// As for [`rasops4_erasecols`].
pub unsafe fn rasops4_copycols(
    cookie: *mut c_void,
    row: i32,
    src: i32,
    dst: i32,
    num: i32,
) -> Result<(), Errno> {
    // SAFETY: the caller's contract.
    unsafe { rasops_bitops::copycols::<2>(cookie, row, src, dst, num) }
}
/* </CODE> */

/* <TESTS> */
#[cfg(test)]
mod tests {
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
}
/* </TESTS> */
