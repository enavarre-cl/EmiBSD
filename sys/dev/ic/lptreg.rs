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
 * Copyright (c) 1990 The Regents of the University of California.
 * All rights reserved.
 *
 * This code is derived from software contributed to Berkeley by
 * William Jolitz.
 *
 * Redistribution and use in source and binary forms, with or without
 * modification, are permitted provided that the following conditions
 * are met:
 * 1. Redistributions of source code must retain the above copyright
 *    notice, this list of conditions and the following disclaimer.
 * 2. Redistributions in binary form must reproduce the above copyright
 *    notice, this list of conditions and the following disclaimer in the
 *    documentation and/or other materials provided with the distribution.
 * 3. Neither the name of the University nor the names of its contributors
 *    may be used to endorse or promote products derived from this software
 *    without specific prior written permission.
 *
 * THIS SOFTWARE IS PROVIDED BY THE REGENTS AND CONTRIBUTORS ``AS IS'' AND
 * ANY EXPRESS OR IMPLIED WARRANTIES, INCLUDING, BUT NOT LIMITED TO, THE
 * IMPLIED WARRANTIES OF MERCHANTABILITY AND FITNESS FOR A PARTICULAR PURPOSE
 * ARE DISCLAIMED.  IN NO EVENT SHALL THE REGENTS OR CONTRIBUTORS BE LIABLE
 * FOR ANY DIRECT, INDIRECT, INCIDENTAL, SPECIAL, EXEMPLARY, OR CONSEQUENTIAL
 * DAMAGES (INCLUDING, BUT NOT LIMITED TO, PROCUREMENT OF SUBSTITUTE GOODS
 * OR SERVICES; LOSS OF USE, DATA, OR PROFITS; OR BUSINESS INTERRUPTION)
 * HOWEVER CAUSED AND ON ANY THEORY OF LIABILITY, WHETHER IN CONTRACT, STRICT
 * LIABILITY, OR TORT (INCLUDING NEGLIGENCE OR OTHERWISE) ARISING IN ANY WAY
 * OUT OF THE USE OF THIS SOFTWARE, EVEN IF ADVISED OF THE POSSIBILITY OF
 * SUCH DAMAGE.
 *
 *      @(#)lptreg.h	1.1 (Berkeley) 12/19/90
 */

/*
 * AT Parallel Port (for lineprinter)
 * Interface port and bit definitions
 * Written by William Jolitz 12/18/90
 * Copyright (C) William Jolitz 1990
 */
/* </LICENSES> */

/* <CODE> */
//! `<dev/ic/lptreg.h>`: the AT parallel port's registers and bits.
//!
//! Upstream: sys/dev/ic/lptreg.h @ 3ce1f3f79392
//!
//! ## Deviations
//! - The register offsets keep the C's lower-case names (`lpt_data`, `lpt_status`,
//!   `lpt_control`), as `usize` bus_space(9) offsets; the bits are `u8`, the width of the
//!   registers.

/// `lpt_data`: Data to/from printer (R/W).
#[allow(non_upper_case_globals)] // the C name
pub const lpt_data: usize = 0;

/// `lpt_status`: Status of printer (R).
#[allow(non_upper_case_globals)] // the C name
pub const lpt_status: usize = 1;
/// `LPS_NERR`: printer no error.
pub const LPS_NERR: u8 = 0x08;
/// `LPS_SELECT`: printer selected.
pub const LPS_SELECT: u8 = 0x10;
/// `LPS_NOPAPER`: printer out of paper.
pub const LPS_NOPAPER: u8 = 0x20;
/// `LPS_NACK`: printer no ack of data.
pub const LPS_NACK: u8 = 0x40;
/// `LPS_NBSY`: printer no ack of data (the C's comment; the line is BUSY, inverted).
pub const LPS_NBSY: u8 = 0x80;

/// `lpt_control`: Control printer (R/W).
#[allow(non_upper_case_globals)] // the C name
pub const lpt_control: usize = 2;
/// `LPC_STROBE`: strobe data to printer.
pub const LPC_STROBE: u8 = 0x01;
/// `LPC_AUTOLF`: automatic linefeed.
pub const LPC_AUTOLF: u8 = 0x02;
/// `LPC_NINIT`: initialize printer.
pub const LPC_NINIT: u8 = 0x04;
/// `LPC_SELECT`: printer selected.
pub const LPC_SELECT: u8 = 0x08;
/// `LPC_IENABLE`: printer out of paper (the C's comment; the bit enables the interrupt).
pub const LPC_IENABLE: u8 = 0x10;

/// `LPT_NPORTS`.
pub const LPT_NPORTS: usize = 4;
/* </CODE> */

/* <TESTS> */
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    #[ignore = "needs OPENBSD_SRC (just test-ref)"]
    fn values_match_the_c_header() {
        let defs = crate::reftest::defines("sys/dev/ic/lptreg.h");
        let ours = crate::reftest::assert_defines!(defs;
            lpt_data, lpt_status, LPS_NERR, LPS_SELECT, LPS_NOPAPER, LPS_NACK, LPS_NBSY,
            lpt_control, LPC_STROBE, LPC_AUTOLF, LPC_NINIT, LPC_SELECT, LPC_IENABLE,
            LPT_NPORTS);
        for prefix in ["LPS_", "LPC_", "LPT_", "lpt_"] {
            crate::reftest::assert_complete(&defs, prefix, &ours);
        }
    }
}
/* </TESTS> */
