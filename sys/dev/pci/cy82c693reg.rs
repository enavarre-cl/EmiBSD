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
 * Copyright (c) 2000 The NetBSD Foundation, Inc.
 * All rights reserved.
 *
 * This code is derived from software contributed to The NetBSD Foundation
 * by Jason R. Thorpe.
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
//! Register definitions for the Cypress 82c693 hyperCache(tm) Stand-Alone PCI Peripheral
//! Controller with USB.
//!
//! Upstream: sys/dev/pci/cy82c693reg.h @ 3ce1f3f79392
//!
//! ## Deviations
//! - `CYHC_CONFIG_ADDR` is a `BusAddr` (the I/O port `bus_space_map` takes), the rest `u8`
//!   register indexes.

use crate::machine::bus::BusAddr;

/// `CYHC_CONFIG_ADDR`: Chipset Configuration Address.
pub const CYHC_CONFIG_ADDR: BusAddr = 0x22;
/// `CYHC_CONFIG_DATA`: Chipset Configuration Data.
pub const CYHC_CONFIG_DATA: BusAddr = 0x23;

/// `CONFIG_PERIPH1`: Peripheral Control #1.
pub const CONFIG_PERIPH1: u8 = 0x01;

/// `CONFIG_PERIPH2`: Peripheral Control #2.
pub const CONFIG_PERIPH2: u8 = 0x02;

/// `CONFIG_ELCR1`: Edge/Level Control #1.
pub const CONFIG_ELCR1: u8 = 0x03;

/// `CONFIG_ELCR2`: Edge/Level Control #2.
pub const CONFIG_ELCR2: u8 = 0x04;

/// `CONFIG_RTC`: RTC Configuration.
pub const CONFIG_RTC: u8 = 0x05;
/* </CODE> */

/* <TESTS> */
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    #[ignore = "needs OPENBSD_SRC (just test-ref)"]
    fn defines_match_the_header() {
        let defs = crate::reftest::defines("sys/dev/pci/cy82c693reg.h");
        crate::reftest::assert_defines!(defs; CYHC_CONFIG_ADDR, CYHC_CONFIG_DATA, CONFIG_PERIPH1,
            CONFIG_PERIPH2, CONFIG_ELCR1, CONFIG_ELCR2, CONFIG_RTC);
    }
}
/* </TESTS> */
