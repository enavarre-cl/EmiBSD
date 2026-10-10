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
 * Copyright (c) 2005 Mark Kettenis
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
//! ServerWorks Broadcom K2/Frodo SATA controller registers.
//!
//! Upstream: sys/dev/pci/pciide_svwsata_reg.h @ 3ce1f3f79392
//!
//! ## Deviations
//! - The function-like macros are `const fn`s named in lower case. The configuration-space
//!   offsets `pciide.c` reads and writes are `i32` (`pci_conf_read`'s `reg`), bus space
//!   offsets `BusSize`, shift counts `i32`, register values and unused offsets `u32`.
//! - The `static` timing tables are `pub const` arrays named in capitals, of `u8` where the
//!   C has `int8_t` or `u_int8_t` (every entry is written to a byte-wide register or shifted
//!   into a field, never sign-extended).
//! - `struct pciide_svwsata` is [`PciideSvwsata`] (tag and handle `Option`s, allocated zeroed).
//! - `wdc_svwsata_vtbl` is a `static` [`ChannelSoftcVtbl`] over `pciide.rs`'s `svwsata_read_reg`, `svwsata_write_reg` and `svwsata_lba48_write_reg`.
#![allow(non_upper_case_globals)] // the C's mixed-case names (`PDC2xx_STATE`, `ACER_0x4B`)

use crate::dev::ic::wdc::{
    wdc_default_read_raw_multi_2, wdc_default_read_raw_multi_4, wdc_default_write_raw_multi_2,
    wdc_default_write_raw_multi_4,
};
use crate::dev::ic::wdcvar::ChannelSoftcVtbl;
use crate::dev::pci::pciide::{svwsata_lba48_write_reg, svwsata_read_reg, svwsata_write_reg};
use crate::kern::subr_prf::panic;
use crate::machine::bus::BusSize;
use crate::machine::bus::{BusSpaceHandle, BusSpaceTag};
use core::cell::Cell;

/// `SVWSATA_TF0`.
pub const SVWSATA_TF0: u32 = 0x00;
/// `SVWSATA_TF8`.
pub const SVWSATA_TF8: u32 = 0x20;

/// `SVWSATA_DMA`.
pub const SVWSATA_DMA: BusSize = 0x30;

/// `SVWSATA_SSTATUS`.
pub const SVWSATA_SSTATUS: BusSize = 0x40;
/// `SVWSATA_SERROR`.
pub const SVWSATA_SERROR: u32 = 0x44;
/// `SVWSATA_SCONTROL`.
pub const SVWSATA_SCONTROL: BusSize = 0x48;

/// `SVWSATA_SICR1`.
pub const SVWSATA_SICR1: BusSize = 0x80;
/// `SVWSATA_SICR2`.
pub const SVWSATA_SICR2: u32 = 0x84;
/// `SVWSATA_SIM`.
pub const SVWSATA_SIM: BusSize = 0x88;

/// `struct pciide_svwsata`.
pub struct PciideSvwsata {
    /// `ba5_st`.
    pub ba5_st: Cell<Option<BusSpaceTag>>,
    /// `ba5_sh`.
    pub ba5_sh: Cell<Option<BusSpaceHandle>>,
}

impl PciideSvwsata {
    /// Zeroed, as `malloc(.., M_ZERO)` leaves it.
    pub const fn new() -> Self {
        Self {
            ba5_st: Cell::new(None),
            ba5_sh: Cell::new(None),
        }
    }

    /// `ss->ba5_st`, set when BA5 is mapped.
    pub fn ba5_st(&self) -> BusSpaceTag {
        match self.ba5_st.get() {
            Some(t) => t,
            None => panic(format_args!("svwsata: BA5 not mapped")),
        }
    }

    /// `ss->ba5_sh`.
    pub fn ba5_sh(&self) -> BusSpaceHandle {
        match self.ba5_sh.get() {
            Some(h) => h,
            None => panic(format_args!("svwsata: BA5 not mapped")),
        }
    }
}

impl Default for PciideSvwsata {
    fn default() -> Self {
        Self::new()
    }
}

/// `wdc_svwsata_vtbl`.
pub static WDC_SVWSATA_VTBL: ChannelSoftcVtbl = ChannelSoftcVtbl {
    read_reg: svwsata_read_reg,
    write_reg: svwsata_write_reg,
    lba48_write_reg: svwsata_lba48_write_reg,
    read_raw_multi_2: wdc_default_read_raw_multi_2,
    write_raw_multi_2: wdc_default_write_raw_multi_2,
    read_raw_multi_4: wdc_default_read_raw_multi_4,
    write_raw_multi_4: wdc_default_write_raw_multi_4,
};
/* </CODE> */

/* <TESTS> */
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    #[ignore = "needs OPENBSD_SRC (just test-ref)"]
    fn defines_match_the_header() {
        let defs = crate::reftest::defines("sys/dev/pci/pciide_svwsata_reg.h");

        crate::reftest::assert_defines!(defs; SVWSATA_TF0, SVWSATA_TF8, SVWSATA_DMA, SVWSATA_SSTATUS, SVWSATA_SERROR, SVWSATA_SCONTROL, SVWSATA_SICR1, SVWSATA_SICR2, SVWSATA_SIM);
    }
}
/* </TESTS> */
