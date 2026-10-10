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
 * Copyright (c) 2001 Izumi Tsutsui.
 *
 * Redistribution and use in source and binary forms, with or without
 * modification, are permitted provided that the following conditions
 * are met:
 * 1. Redistributions of source code must retain the above copyright
 *    notice, this list of conditions and the following disclaimer.
 * 2. Redistributions in binary form must reproduce the above copyright
 *    notice, this list of conditions and the following disclaimer in the
 *    documentation and/or other materials provided with the distribution.
 * 3. The name of the author may not be used to endorse or promote products
 *    derived from this software without specific prior written permission.
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
 *
 */
/* </LICENSES> */

/* <CODE> */
//! Acard ATP850U/ATP860/ATP865 IDE controller registers.
//!
//! Upstream: sys/dev/pci/pciide_acard_reg.h @ 3ce1f3f79392
//!
//! ## Deviations
//! - The function-like macros are `const fn`s named in lower case. The configuration-space
//!   offsets `pciide.c` reads and writes are `i32` (`pci_conf_read`'s `reg`), bus space
//!   offsets `BusSize`, shift counts `i32`, register values and unused offsets `u32`.
//! - The `static` timing tables are `pub const` arrays named in capitals, of `u8` where the
//!   C has `int8_t` or `u_int8_t` (every entry is written to a byte-wide register or shifted
//!   into a field, never sign-extended).
#![allow(non_upper_case_globals)] // the C's mixed-case names (`PDC2xx_STATE`, `ACER_0x4B`)

/// `ATP850_IDETIME`.
pub const fn atp850_idetime(channel: i32) -> i32 {
    0x40 + channel * 4
}
/// `ATP860_IDETIME`.
pub const ATP860_IDETIME: i32 = 0x40;

/// `ATP850_SETTIME`.
pub const fn atp850_settime(drive: i32, act: u32, rec: u32) -> u32 {
    (((act & 0xf) << 8) | (rec & 0xf)) << (drive * 16)
}
/// `ATP860_SETTIME`.
pub const fn atp860_settime(channel: i32, drive: i32, act: u32, rec: u32) -> u32 {
    (((act & 0xf) << 4) | (rec & 0xf)) << (channel * 16 + drive * 8)
}
/// `ATP860_SETTIME_MASK`.
pub const fn atp860_settime_mask(channel: i32) -> u32 {
    0xffff << (channel * 16)
}

/// `acard_act_udma`.
pub const ACARD_ACT_UDMA: [u8; 7] = [0x3, 0x3, 0x3, 0x3, 0x3, 0x3, 0x3];
/// `acard_rec_udma`.
pub const ACARD_REC_UDMA: [u8; 7] = [0x1, 0x1, 0x1, 0x1, 0x1, 0x1, 0x1];
/// `acard_act_dma`.
pub const ACARD_ACT_DMA: [u8; 3] = [0x0, 0x3, 0x3];
/// `acard_rec_dma`.
pub const ACARD_REC_DMA: [u8; 3] = [0xa, 0x3, 0x1];
/// `acard_act_pio`.
pub const ACARD_ACT_PIO: [u8; 5] = [0x0, 0x0, 0x0, 0x3, 0x3];
/// `acard_rec_pio`.
pub const ACARD_REC_PIO: [u8; 5] = [0x0, 0xa, 0x8, 0x3, 0x1];

/// `ATP850_UDMA`.
pub const ATP850_UDMA: i32 = 0x54;
/// `ATP860_UDMA`.
pub const ATP860_UDMA: i32 = 0x44;

/// `ATP850_UDMA_MODE`.
pub const fn atp850_udma_mode(channel: i32, drive: i32, x: u32) -> u32 {
    (x & 0x3) << (channel * 4 + drive * 2)
}
/// `ATP860_UDMA_MODE`.
pub const fn atp860_udma_mode(channel: i32, drive: i32, x: u32) -> u32 {
    (x & 0xf) << (channel * 8 + drive * 4)
}
/// `ATP850_UDMA_MASK`.
pub const fn atp850_udma_mask(channel: i32) -> u32 {
    0xf << (channel * 4)
}
/// `ATP860_UDMA_MASK`.
pub const fn atp860_udma_mask(channel: i32) -> u32 {
    0xff << (channel * 8)
}

/// `acard_udma_conf`.
pub const ACARD_UDMA_CONF: [u8; 7] = [0x1, 0x2, 0x3, 0x4, 0x5, 0x6, 0x7];

/// `ATP8x0_CTRL`.
pub const ATP8x0_CTRL: i32 = 0x48;
/// `ATP8x0_CTRL_EN`.
pub const fn atp8x0_ctrl_en(chan: i32) -> u32 {
    0x00020000 << chan
}
/// `ATP860_CTRL_INT`.
pub const ATP860_CTRL_INT: u32 = 0x00010000;
/// `ATP860_CTRL_80P`.
pub const fn atp860_ctrl_80p(chan: i32) -> u32 {
    0x00000100 << chan
}
/* </CODE> */

/* <TESTS> */
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    #[ignore = "needs OPENBSD_SRC (just test-ref)"]
    fn defines_match_the_header() {
        let defs = crate::reftest::defines("sys/dev/pci/pciide_acard_reg.h");

        crate::reftest::assert_defines!(defs; ATP860_IDETIME, ATP850_UDMA, ATP860_UDMA, ATP8x0_CTRL, ATP860_CTRL_INT);
    }
}
/* </TESTS> */
