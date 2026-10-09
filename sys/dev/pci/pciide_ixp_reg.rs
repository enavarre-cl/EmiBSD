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
 *  Copyright (c) 2004 The NetBSD Foundation.
 *  All rights reserved.
 *
 *  This code is derived from software contributed to the NetBSD Foundation
 *  by Quentin Garnier.
 *
 *  Redistribution and use in source and binary forms, with or without
 *  modification, are permitted provided that the following conditions
 *  are met:
 *  1. Redistributions of source code must retain the above copyright
 *     notice, this list of conditions and the following disclaimer.
 *  2. Redistributions in binary form must reproduce the above copyright
 *     notice, this list of conditions and the following disclaimer in the
 *     documentation and/or other materials provided with the distribution.
 *
 *  THIS SOFTWARE IS PROVIDED BY THE NETBSD FOUNDATION, INC. AND CONTRIBUTORS
 *  ``AS IS'' AND ANY EXPRESS OR IMPLIED WARRANTIES, INCLUDING, BUT NOT LIMITED
 *  TO, THE IMPLIED WARRANTIES OF MERCHANTABILITY AND FITNESS FOR A PARTICULAR
 *  PURPOSE ARE DISCLAIMED.  IN NO EVENT SHALL THE FOUNDATION OR CONTRIBUTORS
 *  BE LIABLE FOR ANY DIRECT, INDIRECT, INCIDENTAL, SPECIAL, EXEMPLARY, OR
 *  CONSEQUENTIAL DAMAGES (INCLUDING, BUT NOT LIMITED TO, PROCUREMENT OF
 *  SUBSTITUTE GOODS OR SERVICES; LOSS OF USE, DATA, OR PROFITS; OR BUSINESS
 *  INTERRUPTION) HOWEVER CAUSED AND ON ANY THEORY OF LIABILITY, WHETHER IN
 *  CONTRACT, STRICT LIABILITY, OR TORT (INCLUDING NEGLIGENCE OR OTHERWISE)
 *  ARISING IN ANY WAY OUT OF THE USE OF THIS SOFTWARE, EVEN IF ADVISED OF THE
 *  POSSIBILITY OF SUCH DAMAGE.
 */
/* </LICENSES> */

/* <CODE> */
//! ATI IXP IDE controller registers (values from the Linux driver).
//!
//! Upstream: sys/dev/pci/pciide_ixp_reg.h @ 3ce1f3f79392
//!
//! ## Deviations
//! - The function-like macros are `const fn`s named in lower case. The configuration-space
//!   offsets `pciide.c` reads and writes are `i32` (`pci_conf_read`'s `reg`), bus space
//!   offsets `BusSize`, shift counts `i32`, register values and unused offsets `u32`.
//! - The `static` timing tables are `pub const` arrays named in capitals, of `u8` where the
//!   C has `int8_t` or `u_int8_t` (every entry is written to a byte-wide register or shifted
//!   into a field, never sign-extended).
//! - The statement macros `IXP_UDMA_ENABLE`, `IXP_UDMA_DISABLE`, `IXP_SET_MODE` and `IXP_SET_TIMING` are functions taking the register value by `&mut`.

// All values gathered from the linux driver.

/// `IXP_PIO_TIMING`.
pub const IXP_PIO_TIMING: i32 = 0x40;
/// `IXP_MDMA_TIMING`.
pub const IXP_MDMA_TIMING: i32 = 0x44;
/// `IXP_PIO_CTL`.
pub const IXP_PIO_CTL: i32 = 0x48;
/// `IXP_PIO_MODE`.
pub const IXP_PIO_MODE: u32 = 0x4a;
/// `IXP_UDMA_CTL`.
pub const IXP_UDMA_CTL: i32 = 0x54;
/// `IXP_UDMA_MODE`.
pub const IXP_UDMA_MODE: u32 = 0x56;

/// `ixp_pio_timings`.
pub const IXP_PIO_TIMINGS: [u8; 5] = [0x5d, 0x47, 0x34, 0x22, 0x20];

/// `ixp_mdma_timings`.
pub const IXP_MDMA_TIMINGS: [u8; 3] = [0x77, 0x21, 0x20];

// First 4 bits of UDMA_CTL enable or disable UDMA for the drive
/// `IXP_UDMA_ENABLE`.
pub fn ixp_udma_enable(u: &mut u32, c: i32, d: i32) {
    *u |= 1 << (2 * c + d);
}
/// `IXP_UDMA_DISABLE`.
pub fn ixp_udma_disable(u: &mut u32, c: i32, d: i32) {
    *u &= !(1 << (2 * c + d));
}

// UDMA_MODE has 4 bits per drive, though only 3 are actually used
// Note that in this macro u is the whole
// UDMA_CTL+UDMA_MODE register 32bits.
// PIO_MODE works just the same.
/// `IXP_SET_MODE`.
pub fn ixp_set_mode(u: &mut u32, c: i32, d: i32, m: u32) {
    let ixpshift = 16 + 8 * c + 4 * d;
    *u &= !(0x7 << ixpshift);
    *u |= (m & 0x7) << ixpshift;
}

// MDMA_TIMING has one byte per drive.
// PIO_TIMING works just the same.
/// `IXP_SET_TIMING`.
pub fn ixp_set_timing(m: &mut u32, c: i32, d: i32, t: u32) {
    let ixpshift = 16 * c + 8 * d;
    *m &= !(0xff << ixpshift);
    *m |= (t & 0xff) << ixpshift;
}
/* </CODE> */

/* <TESTS> */
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    #[ignore = "needs OPENBSD_SRC (just test-ref)"]
    fn defines_match_the_header() {
        let defs = crate::reftest::defines("sys/dev/pci/pciide_ixp_reg.h");

        crate::reftest::assert_defines!(defs; IXP_PIO_TIMING, IXP_MDMA_TIMING, IXP_PIO_CTL, IXP_PIO_MODE, IXP_UDMA_CTL, IXP_UDMA_MODE);
    }
}
/* </TESTS> */
