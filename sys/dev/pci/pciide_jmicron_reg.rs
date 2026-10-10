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
 * Copyright (c) 2007 Jonathan Gray <jsg@openbsd.org>
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
//! JMicron JMB36x IDE controller registers.
//!
//! Upstream: sys/dev/pci/pciide_jmicron_reg.h @ 3ce1f3f79392
//!
//! ## Deviations
//! - The function-like macros are `const fn`s named in lower case. The configuration-space
//!   offsets `pciide.c` reads and writes are `i32` (`pci_conf_read`'s `reg`), bus space
//!   offsets `BusSize`, shift counts `i32`, register values and unused offsets `u32`.
//! - The `static` timing tables are `pub const` arrays named in capitals, of `u8` where the
//!   C has `int8_t` or `u_int8_t` (every entry is written to a byte-wide register or shifted
//!   into a field, never sign-extended).
//! - `JMICRON_CHAN_EN(chan)`'s conditional is an `if`.
#![allow(non_upper_case_globals)] // the C's mixed-case names (`PDC2xx_STATE`, `ACER_0x4B`)

/// `JMICRON_MASTER_UDMA`.
pub const JMICRON_MASTER_UDMA: u32 = 1 << 2;
/// `JMICRON_MASTER_SHIFT`.
pub const JMICRON_MASTER_SHIFT: i32 = 4;
/// `JMICRON_SLAVE_SHIFT`.
pub const JMICRON_SLAVE_SHIFT: i32 = 12;

/// `JMICRON_CONF`.
pub const JMICRON_CONF: i32 = 0x40;
/// `JMICRON_CHAN_EN`.
pub const fn jmicron_chan_en(chan: i32) -> u32 {
    if chan == 1 { 4 } else { 0 }
}

/// `JMICRON_CONF_SWAP`.
pub const JMICRON_CONF_SWAP: u32 = 1 << 22;
/// `JMICRON_CONF_40PIN`.
pub const JMICRON_CONF_40PIN: u32 = 1 << 3;
/* </CODE> */

/* <TESTS> */
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    #[ignore = "needs OPENBSD_SRC (just test-ref)"]
    fn defines_match_the_header() {
        let defs = crate::reftest::defines("sys/dev/pci/pciide_jmicron_reg.h");

        crate::reftest::assert_defines!(defs; JMICRON_MASTER_UDMA, JMICRON_MASTER_SHIFT, JMICRON_SLAVE_SHIFT, JMICRON_CONF, JMICRON_CONF_SWAP, JMICRON_CONF_40PIN);
    }
}
/* </TESTS> */
