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
//! `<dev/isa/pcppireg.h>`: PPI speaker control values (port B of the PC's 8255 at
//! `IO_PPI`).
//!
//! Upstream: sys/dev/isa/pcppireg.h @ 3ce1f3f79392

/// `PIT_ENABLETMR2`: Enable timer/counter 2.
pub const PIT_ENABLETMR2: u8 = 0x01;
/// `PIT_SPKRDATA`: Direct to speaker.
pub const PIT_SPKRDATA: u8 = 0x02;

/// `PIT_SPKR`: the timer's gate and the speaker together.
pub const PIT_SPKR: u8 = PIT_ENABLETMR2 | PIT_SPKRDATA;
/* </CODE> */

/* <TESTS> */
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    #[ignore = "needs OPENBSD_SRC (just test-ref)"]
    fn values_match_the_c_header() {
        let defs = crate::reftest::defines("sys/dev/isa/pcppireg.h");
        let ours = crate::reftest::assert_defines!(defs; PIT_ENABLETMR2, PIT_SPKRDATA, PIT_SPKR);
        crate::reftest::assert_complete(&defs, "PIT_", &ours);
    }
}
/* </TESTS> */
