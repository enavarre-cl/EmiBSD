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
//! ITE IT8211F/IT8212F IDE controller registers.
//!
//! Upstream: sys/dev/pci/pciide_ite_reg.h @ 3ce1f3f79392
//!
//! ## Deviations
//! - The function-like macros are `const fn`s named in lower case. The configuration-space
//!   offsets `pciide.c` reads and writes are `i32` (`pci_conf_read`'s `reg`), bus space
//!   offsets `BusSize`, shift counts `i32`, register values and unused offsets `u32`.
//! - The `static` timing tables are `pub const` arrays named in capitals, of `u8` where the
//!   C has `int8_t` or `u_int8_t` (every entry is written to a byte-wide register or shifted
//!   into a field, never sign-extended).
//! - `IT_TIM(chan)`'s conditional is an `if`.
#![allow(non_upper_case_globals)] // the C's mixed-case names (`PDC2xx_STATE`, `ACER_0x4B`)

// Registers definition for IT8212F
/// `IT_CFG`: I/O configuration.
pub const IT_CFG: i32 = 0x40;
/// `IT_CFG_MASK`.
pub const IT_CFG_MASK: u32 = 0x0000ffff;
/// `IT_CFG_IORDY`.
pub const fn it_cfg_iordy(chan: i32) -> u32 {
    0x0001 << chan
}
/// `IT_CFG_BLID`.
pub const fn it_cfg_blid(chan: i32) -> u32 {
    0x0004 << chan
}
/// `IT_CFG_CABLE`.
pub const fn it_cfg_cable(chan: i32, drive: i32) -> u32 {
    0x0010 << (chan * 2 + drive)
}
/// `IT_CFG_DECODE`.
pub const fn it_cfg_decode(chan: i32) -> u32 {
    0x8000 >> (chan * 2)
}

/// `IT_MODE`: mode control / RAID function.
pub const IT_MODE: i32 = 0x50;
/// `IT_MODE_MASK`.
pub const IT_MODE_MASK: u32 = 0x0000ffff;
/// `IT_MODE_CPU`.
pub const IT_MODE_CPU: u32 = 0x0001;
/// `IT_MODE_50MHZ`.
pub const fn it_mode_50mhz(chan: i32) -> u32 {
    0x0002 << chan
}
/// `IT_MODE_DMA`.
pub const fn it_mode_dma(chan: i32, drive: i32) -> u32 {
    0x0008 << (chan * 2 + drive)
}
/// `IT_MODE_RESET`.
pub const IT_MODE_RESET: u32 = 0x0080;
/// `IT_MODE_RAID1`.
pub const IT_MODE_RAID1: u32 = 0x0100;

/// `IT_TIM`: timings.
pub const fn it_tim(chan: i32) -> i32 {
    if chan != 0 { 0x58 } else { 0x54 }
}
/// `IT_TIM_UDMA5`.
pub const fn it_tim_udma5(drive: i32) -> u32 {
    0x00800000 << (drive * 8)
}
/* </CODE> */

/* <TESTS> */
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    #[ignore = "needs OPENBSD_SRC (just test-ref)"]
    fn defines_match_the_header() {
        let defs = crate::reftest::defines("sys/dev/pci/pciide_ite_reg.h");

        crate::reftest::assert_defines!(defs; IT_CFG, IT_CFG_MASK, IT_MODE, IT_MODE_MASK, IT_MODE_CPU, IT_MODE_RESET, IT_MODE_RAID1);
    }
}
/* </TESTS> */
