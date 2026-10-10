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

/*
 * Copyright (c) 2003 Alexander Yurchenko <grange@openbsd.org>
 * All rights reserved.
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
 * THIS SOFTWARE IS PROVIDED BY THE AUTHOR ``AS IS'' AND ANY EXPRESS OR
 * IMPLIED WARRANTIES, INCLUDING, BUT NOT LIMITED TO, THE IMPLIED WARRANTIES
 * OF MERCHANTABILITY AND FITNESS FOR A PARTICULAR PURPOSE ARE DISCLAIMED.
 * IN NO EVENT SHALL THE AUTHOR BE LIABLE FOR ANY DIRECT, INDIRECT,
 * INCIDENTAL, SPECIAL, EXEMPLARY, OR CONSEQUENTIAL DAMAGES (INCLUDING, BUT
 * NOT LIMITED TO, PROCUREMENT OF SUBSTITUTE GOODS OR SERVICES; LOSS OF USE,
 * DATA, OR PROFITS; OR BUSINESS INTERRUPTION) HOWEVER CAUSED AND ON ANY
 * THEORY OF LIABILITY, WHETHER IN CONTRACT, STRICT LIABILITY, OR TORT
 * (INCLUDING NEGLIGENCE OR OTHERWISE) ARISING IN ANY WAY OUT OF THE USE OF
 * THIS SOFTWARE, EVEN IF ADVISED OF THE POSSIBILITY OF SUCH DAMAGE.
 */
/* </LICENSES> */

/* <CODE> */
//! NVIDIA nForce IDE controller registers.
//!
//! Upstream: sys/dev/pci/pciide_nforce_reg.h @ 3ce1f3f79392
//!
//! ## Deviations
//! - The function-like macros are `const fn`s named in lower case. The configuration-space
//!   offsets `pciide.c` reads and writes are `i32` (`pci_conf_read`'s `reg`), bus space
//!   offsets `BusSize`, shift counts `i32`, register values and unused offsets `u32`.
//! - The `static` timing tables are `pub const` arrays named in capitals, of `u8` where the
//!   C has `int8_t` or `u_int8_t` (every entry is written to a byte-wide register or shifted
//!   into a field, never sign-extended).
#![allow(non_upper_case_globals)] // the C's mixed-case names (`PDC2xx_STATE`, `ACER_0x4B`)

// Configuration register
/// `NFORCE_CONF`.
pub const NFORCE_CONF: i32 = 0x50;
/// `NFORCE_CHAN_EN`.
pub const fn nforce_chan_en(chan: i32) -> u32 {
    0x00000001 << (1 - chan)
}

// PIO and multiword DMA timing register
/// `NFORCE_PIODMATIM`.
pub const NFORCE_PIODMATIM: i32 = 0x58;
/// `NFORCE_PIODMATIM_MASK`.
pub const fn nforce_piodmatim_mask(chan: i32) -> u32 {
    0xffff << ((1 - chan) * 16)
}
/// `NFORCE_PIODMATIM_SET`.
pub const fn nforce_piodmatim_set(chan: i32, drive: i32, x: u32) -> u32 {
    x << ((3 - (chan * 2 + drive)) * 8)
}

// PIO timing register
/// `NFORCE_PIOTIM`.
pub const NFORCE_PIOTIM: i32 = 0x5c;

// UDMA timing register
/// `NFORCE_UDMATIM`.
pub const NFORCE_UDMATIM: i32 = 0x60;
/// `NFORCE_UDMATIM_MASK`.
pub const fn nforce_udmatim_mask(chan: i32) -> u32 {
    0xffff << ((1 - chan) * 16)
}
/// `NFORCE_UDMATIM_SET`.
pub const fn nforce_udmatim_set(chan: i32, drive: i32, x: u32) -> u32 {
    x << ((3 - (chan * 2 + drive)) * 8)
}
/// `NFORCE_UDMA_EN`.
pub const fn nforce_udma_en(chan: i32, drive: i32) -> u32 {
    0x40 << ((3 - (chan * 2 + drive)) * 8)
}
/// `NFORCE_UDMA_ENM`.
pub const fn nforce_udma_enm(chan: i32, drive: i32) -> u32 {
    0x80 << ((3 - (chan * 2 + drive)) * 8)
}

// Timing values
/// `nforce_pio`.
pub const NFORCE_PIO: [u8; 5] = [0xa8, 0x65, 0x42, 0x22, 0x20];
/// `nforce_udma`.
pub const NFORCE_UDMA: [u8; 7] = [0x02, 0x01, 0x00, 0x04, 0x05, 0x06, 0x07];
/* </CODE> */

/* <TESTS> */
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    #[ignore = "needs OPENBSD_SRC (just test-ref)"]
    fn defines_match_the_header() {
        let defs = crate::reftest::defines("sys/dev/pci/pciide_nforce_reg.h");

        crate::reftest::assert_defines!(defs; NFORCE_CONF, NFORCE_PIODMATIM, NFORCE_PIOTIM, NFORCE_UDMATIM);
    }
}
/* </TESTS> */
