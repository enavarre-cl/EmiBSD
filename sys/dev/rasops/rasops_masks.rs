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
//! Bit masks for the sub-byte depths: `dev/rasops/rasops_masks.c` and `rasops_masks.h`.
//!
//! Upstream: sys/dev/rasops/rasops_masks.c @ 3ce1f3f79392
//! Upstream: sys/dev/rasops/rasops_masks.h @ 3ce1f3f79392
//!
//! The `ragged edge` masks ([`RASOPS_LMASK`], [`RASOPS_RMASK`]), the part masks
//! ([`RASOPS_PMASK`]) and the `MBL`/`MBR`/`MBE`/`GETBITS`/`PUTBITS` macros that `rasops1`
//! and `rasops4` (through `rasops_bitops`) use to move runs of 1-bit and 4-bit pixels in
//! 32-bit words.
//!
//! ## Deviations
//! - The C writes the tables big-endian and, on a little-endian machine, flips them in
//!   place once (`rasops_masks_init`, `MBE` over every entry). The tables here are computed
//!   at compile time, already in the byte order of the target (`mbe` is applied by the
//!   `const fn`s that build them), so they are immutable `static`s and
//!   [`rasops_masks_init`] has nothing left to do (the C's own big-endian definition of it
//!   is the empty `do { } while (0)`). The entries are the C's values: the formulas were
//!   checked against every entry of the C source's tables (the part masks are the bits
//!   `x..x + w - 1`, zero where `x + w > 32`, all ones for `x == 0, w == 0`).
//! - `MBL`, `MBR`, `MBE`, `GETBITS` and `PUTBITS` are functions (`mbl`, `mbr`, `mbe`,
//!   `getbits`, `putbits`) instead of macros; `getbits` returns the value the macro stores
//!   in its `dw` argument. `MBE` is `u32::reverse_bits` on little endian (the C's five
//!   mask-and-shift steps) and the identity on big endian (`cfg!(target_endian)`).
//!   `GETBITS`/`PUTBITS` read and write the frame buffer through `fb_r32`/`fb_w32`
//!   (volatile).
//! - `rasops_pmask[x][32]` (a run of a whole word, which `rasops1`'s `putchar` and the
//!   bit operations index with for 32-bit wide runs) is one past the end of the C's row: it
//!   reads the first entry of the next row, zero, so such a run paints nothing. `pmask`
//!   keeps that result (zero past the end of the table, where the C reads whatever follows
//!   it) instead of indexing out of bounds.

use crate::dev::rasops::rasops::{fb_r32, fb_w32};

/// `BYTE_ORDER == BIG_ENDIAN`.
const BIG_ENDIAN: bool = cfg!(target_endian = "big");

/// `MBL(x, y)`: move bits left (`x >> y`).
pub(crate) const fn mbl(x: u32, y: u32) -> u32 {
    if y > 31 { 0 } else { x >> y }
}

/// `MBR(x, y)`: move bits right (`x << y`).
pub(crate) const fn mbr(x: u32, y: u32) -> u32 {
    if y > 31 { 0 } else { x << y }
}

/// `MBE(x)`: make big-endian. To get around the problem of dealing with properly ordered
/// bits on little-endian machines, everything is converted to big-endian and back again
/// when done.
pub(crate) const fn mbe(x: u32) -> u32 {
    if BIG_ENDIAN { x } else { x.reverse_bits() }
}

/// An entry of `rasops_lmask` as the C writes it (big-endian).
const fn lmask_be(i: u32) -> u32 {
    if i == 0 || i == 32 {
        0
    } else {
        0xffff_ffff >> i
    }
}

/// An entry of `rasops_rmask`, big-endian.
const fn rmask_be(i: u32) -> u32 {
    if i == 0 { 0 } else { 0xffff_ffff << (32 - i) }
}

/// An entry of `rasops_pmask[x][w]`, big-endian: the `w` bits from bit `x` on; `w == 0` is
/// the whole word for `x == 0` (the C's `w & 31` of a 32 bit run) and nothing otherwise;
/// zero where the run does not fit.
const fn pmask_be(x: u32, w: u32) -> u32 {
    if w == 0 {
        if x == 0 { 0xffff_ffff } else { 0 }
    } else if x + w <= 32 {
        (0xffff_ffff << (32 - w)) >> x
    } else {
        0
    }
}

const fn make_lmask() -> [i32; 33] {
    let mut t = [0i32; 33];
    let mut i = 0;
    while i < 33 {
        t[i] = mbe(lmask_be(i as u32)) as i32;
        i += 1;
    }
    t
}

const fn make_rmask() -> [i32; 33] {
    let mut t = [0i32; 33];
    let mut i = 0;
    while i < 33 {
        t[i] = mbe(rmask_be(i as u32)) as i32;
        i += 1;
    }
    t
}

const fn make_pmask() -> [[i32; 32]; 32] {
    let mut t = [[0i32; 32]; 32];
    let mut x = 0;
    while x < 32 {
        let mut w = 0;
        while w < 32 {
            t[x][w] = mbe(pmask_be(x as u32, w as u32)) as i32;
            w += 1;
        }
        x += 1;
    }
    t
}

/// `rasops_lmask`: `ragged edge' bitmasks.
pub static RASOPS_LMASK: [i32; 33] = make_lmask();

/// `rasops_rmask`: `ragged edge' bitmasks.
pub static RASOPS_RMASK: [i32; 33] = make_rmask();

/// `rasops_pmask`: part bitmasks, `[x][w]` is the `w` bits starting at bit `x`.
pub static RASOPS_PMASK: [[i32; 32]; 32] = make_pmask();

/// `rasops_pmask[x][w]` for the callers that index with `w` up to 32 (a run of a whole
/// word: `w == 32`). The C then reads past the row, into `rasops_pmask[x + 1][0]` (zero for
/// every `x + 1`, and out of the table for `x == 31`); this reads the same flat table and
/// gives zero past its end.
pub(crate) fn pmask(x: i32, w: i32) -> i32 {
    RASOPS_PMASK
        .as_flattened()
        .get((x * 32 + w) as usize)
        .copied()
        .unwrap_or(0)
}

/// `rasops_masks_init`: flip the masks to little-endian. They are built that way
/// (see the deviations), so there is nothing to do.
pub fn rasops_masks_init() {}

/// `GETBITS(sp, x, w, dw)`: get a number of bits (`w <= 32`) from `*sp`, starting at bit
/// `x`.
///
/// # Safety
///
/// `sp` is a 4-byte aligned address in the mapped frame buffer, and so is `sp + 4` when
/// `x + w > 32`.
pub(crate) unsafe fn getbits(sp: *const u8, x: u32, w: u32) -> u32 {
    // SAFETY: the caller's contract.
    let first = unsafe { fb_r32(sp) } as u32;
    let mut dw = mbl(first, x);
    if x + w > 32 {
        // SAFETY: the caller's contract: the next word is in the frame buffer.
        let next = unsafe { fb_r32(sp.add(4)) } as u32;
        dw |= mbr(next, 32 - x);
    }
    dw
}

/// `PUTBITS(sw, x, w, dp)`: put a number of bits (`w <= 32`) from `sw` to `*dp`, starting
/// at bit `x`.
///
/// # Safety
///
/// `dp` is a 4-byte aligned address in the mapped frame buffer, and so is `dp + 4` when
/// `x + w > 32`.
pub(crate) unsafe fn putbits(sw: u32, x: u32, w: u32, dp: *mut u8) {
    let n = (x + w) as i32 - 32;

    // SAFETY: the caller's contract.
    let old = unsafe { fb_r32(dp) } as u32;
    if n <= 0 {
        let n = RASOPS_PMASK[(x & 31) as usize][(w & 31) as usize] as u32;
        // SAFETY: the caller's contract.
        unsafe { fb_w32(dp, ((old & !n) | (mbr(sw, x) & n)) as i32) };
    } else {
        let n = n as usize;
        let rmask = RASOPS_RMASK[x as usize] as u32;
        // SAFETY: the caller's contract.
        unsafe { fb_w32(dp, ((old & rmask) | mbr(sw, x)) as i32) };
        // SAFETY: the caller's contract: the next word is in the frame buffer.
        let old = unsafe { fb_r32(dp.add(4)) } as u32;
        let rmask = RASOPS_RMASK[n] as u32;
        let lmask = RASOPS_LMASK[n] as u32;
        // SAFETY: as above.
        unsafe {
            fb_w32(
                dp.add(4),
                ((old & rmask) | (mbl(sw, 32 - x) & lmask)) as i32,
            )
        };
    }
}
/* </CODE> */

/* <TESTS> */
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn tables() {
        assert_eq!(RASOPS_LMASK[0], 0);
        assert_eq!(RASOPS_LMASK[32], 0);
        assert_eq!(RASOPS_RMASK[0], 0);
        assert_eq!(RASOPS_RMASK[32], -1);
        // The big-endian values of the C, bit-reversed on little-endian.
        assert_eq!(RASOPS_LMASK[1] as u32, mbe(0x7fff_ffff));
        assert_eq!(RASOPS_RMASK[4] as u32, mbe(0xf000_0000));
        assert_eq!(RASOPS_PMASK[0][0] as u32, mbe(0xffff_ffff));
        assert_eq!(RASOPS_PMASK[1][0], 0);
        assert_eq!(RASOPS_PMASK[2][3] as u32, mbe(0x3800_0000));
        assert_eq!(RASOPS_PMASK[30][2] as u32, mbe(0x0000_0003));
        assert_eq!(RASOPS_PMASK[30][3], 0);
        assert_eq!(RASOPS_PMASK[31][1] as u32, mbe(1));
    }

    #[test]
    fn get_and_put_bits_in_one_word() {
        let mut w = [-1i32; 2];
        let p = w.as_mut_ptr().cast::<u8>();
        // SAFETY: two words of a local array.
        unsafe {
            // Clear 8 bits at bit 4, then put a pattern over them.
            putbits(0, 4, 8, p);
            assert_eq!(w[0] as u32, !RASOPS_PMASK[4][8] as u32);
            putbits(0b1010_0101, 4, 8, p);
            assert_eq!(getbits(p, 4, 8) & 0xff, 0b1010_0101);
            assert_eq!(w[1], -1);
            // A whole word.
            assert_eq!(getbits(p.add(4), 0, 32), u32::MAX);
        }
    }
}
/* </TESTS> */
