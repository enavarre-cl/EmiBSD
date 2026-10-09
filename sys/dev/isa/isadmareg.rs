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
/* </LICENSES> */

/* <CODE> */
//! The PC's two 8237 DMA controllers: `<dev/isa/isadmareg.h>`.
//!
//! Upstream: sys/dev/isa/isadmareg.h @ 3ce1f3f79392
//!
//! Controller 1 (channels 0..3, byte transfers) has its registers at consecutive ports from
//! `IO_DMA1`, controller 2 (channels 4..7, word transfers) at every other port from `IO_DMA2`;
//! the offsets below are from those bases. The mode and mask bits are `<dev/ic/i8237reg.h>`'s,
//! which the C header includes.
//!
//! ## Deviations
//! - `DMA1_CHN(c)` and `DMA2_CHN(c)` are the `const fn`s `dma1_chn` and `dma2_chn`.

#![allow(clippy::identity_op)] // the C's `(1*(2*(c)))`, kept as the header writes them

pub use crate::dev::ic::i8237reg::*;

/// `DMA1_CHN(c)`: addr reg for channel c.
pub const fn dma1_chn(c: usize) -> usize {
    1 * (2 * c)
}
/// `DMA1_SR`: status register.
pub const DMA1_SR: usize = 1 * 8;
/// `DMA1_SMSK`: single mask register.
pub const DMA1_SMSK: usize = 1 * 10;
/// `DMA1_MODE`: mode register.
pub const DMA1_MODE: usize = 1 * 11;
/// `DMA1_FFC`: clear first/last FF.
pub const DMA1_FFC: usize = 1 * 12;

/// `DMA1_IOSIZE`.
pub const DMA1_IOSIZE: usize = 1 * 12;

/// `DMA2_CHN(c)`: addr reg for channel c.
pub const fn dma2_chn(c: usize) -> usize {
    2 * (2 * c)
}
/// `DMA2_SR`: status register.
pub const DMA2_SR: usize = 2 * 8;
/// `DMA2_SMSK`: single mask register.
pub const DMA2_SMSK: usize = 2 * 10;
/// `DMA2_MODE`: mode register.
pub const DMA2_MODE: usize = 2 * 11;
/// `DMA2_FFC`: clear first/last FF.
pub const DMA2_FFC: usize = 2 * 12;

/// `DMA2_IOSIZE`.
pub const DMA2_IOSIZE: usize = 2 * 12;
/* </CODE> */

/* <TESTS> */
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn channel_registers() {
        // Controller 1: address/count pairs at 0/1, 2/3, 4/5, 6/7; controller 2 at 0/2, 4/6, ...
        assert_eq!([dma1_chn(0), dma1_chn(2), dma1_chn(3)], [0, 4, 6]);
        assert_eq!([dma2_chn(0), dma2_chn(1), dma2_chn(3)], [0, 4, 12]);
    }

    #[test]
    #[ignore = "needs OPENBSD_SRC (just test-ref)"]
    fn values_match_the_c_header() {
        let defs = crate::reftest::defines("sys/dev/isa/isadmareg.h");
        for (name, v) in [
            ("DMA1_SR", DMA1_SR),
            ("DMA1_SMSK", DMA1_SMSK),
            ("DMA1_MODE", DMA1_MODE),
            ("DMA1_FFC", DMA1_FFC),
            ("DMA1_IOSIZE", DMA1_IOSIZE),
            ("DMA2_SR", DMA2_SR),
            ("DMA2_SMSK", DMA2_SMSK),
            ("DMA2_MODE", DMA2_MODE),
            ("DMA2_FFC", DMA2_FFC),
            ("DMA2_IOSIZE", DMA2_IOSIZE),
        ] {
            assert_eq!(crate::reftest::int(&defs, name), Some(v as i64), "{name}");
        }
    }
}
/* </TESTS> */
