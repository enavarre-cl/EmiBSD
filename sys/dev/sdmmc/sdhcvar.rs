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
 * Copyright (c) 2006 Uwe Stuehler <uwe@openbsd.org>
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
//! `<dev/sdmmc/sdhcvar.h>`: the state an SD Host Controller attachment (`sdhc_pci`, ...)
//! shares with `sdhc.c`: the softc with its hosts (one per slot) and the attachment's hooks.
//!
//! Upstream: sys/dev/sdmmc/sdhcvar.h @ 3ce1f3f79392
//!
//! The functions declared here (`sdhc_host_found`, `sdhc_activate`, `sdhc_shutdown`,
//! `sdhc_intr`, `sdhc_needs_discover`, `sdhc_write_2`) live in `sdhc.rs`.
//!
//! ## Deviations
//! - The softc is `#[repr(C)]` with the device first; its members are `Cell`s, all-zero
//!   valid. `sc_host` is the C's `struct sdhc_host **` (an array the attachment allocates),
//!   read through [`SdhcSoftc::host`]; `sc_dmat` is an `Option`.
//! - The attachment hooks take the softc by reference; `sc_card_detect` returns a `bool`
//!   and `sc_signal_voltage` a `Result`, as the chip functions they stand in for.

use core::cell::Cell;

use crate::dev::sdmmc::sdhc::SdhcHost;
use crate::machine::bus::{BusDmaTag, BusSize};
use crate::sys::device::{Device, Softc};
use crate::sys::errno::Errno;

/* flag values */

/// `SDHC_F_NOPWR0`.
pub const SDHC_F_NOPWR0: u32 = 1 << 0;
/// `SDHC_F_NONREMOVABLE`.
pub const SDHC_F_NONREMOVABLE: u32 = 1 << 1;
/// `SDHC_F_32BIT_ACCESS`.
pub const SDHC_F_32BIT_ACCESS: u32 = 1 << 2;
/// `SDHC_F_NO_HS_BIT`.
pub const SDHC_F_NO_HS_BIT: u32 = 1 << 3;

/// `void (*sc_bus_clock_pre)(struct sdhc_softc *, int freq, int timing)` and
/// `sc_bus_clock_post`.
pub type SdhcBusClockFn = fn(&SdhcSoftc, i32, i32);

/// `int (*sc_card_detect)(struct sdhc_softc *)`: true if the card is inserted.
pub type SdhcCardDetectFn = fn(&SdhcSoftc) -> bool;

/// `int (*sc_signal_voltage)(struct sdhc_softc *, int)`.
pub type SdhcSignalVoltageFn = fn(&SdhcSoftc, i32) -> Result<(), Errno>;

/// `struct sdhc_softc`.
#[repr(C)]
pub struct SdhcSoftc {
    /// `sc_dev`.
    pub sc_dev: Device,
    /// `sc_host`: the attachment's array of host pointers.
    pub sc_host: Cell<*mut *mut SdhcHost>,
    /// `sc_nhosts`.
    pub sc_nhosts: Cell<i32>,
    /// `sc_flags`: `SDHC_F_*`.
    pub sc_flags: Cell<u32>,
    /// `sc_clkbase`: the base clock (kHz) when the capabilities do not give it.
    pub sc_clkbase: Cell<u32>,

    /// `sc_dmat`.
    pub sc_dmat: Cell<Option<BusDmaTag>>,
    /// `sc_dma_boundary`.
    pub sc_dma_boundary: Cell<BusSize>,

    /// `sc_bus_clock_pre`.
    pub sc_bus_clock_pre: Cell<Option<SdhcBusClockFn>>,
    /// `sc_bus_clock_post`.
    pub sc_bus_clock_post: Cell<Option<SdhcBusClockFn>>,
    /// `sc_card_detect`.
    pub sc_card_detect: Cell<Option<SdhcCardDetectFn>>,
    /// `sc_signal_voltage`.
    pub sc_signal_voltage: Cell<Option<SdhcSignalVoltageFn>>,
}

impl SdhcSoftc {
    /// `sc->sc_host[n]`, `None` for a NULL slot.
    pub fn host(&self, n: i32) -> Option<&'static SdhcHost> {
        let hosts = self.sc_host.get();
        if hosts.is_null() || n < 0 {
            return None;
        }
        // SAFETY: the attachment allocated `sc_host` with room for every slot it maps, and
        // `sdhc_host_found` keeps `n` below the hosts it stored; a stored host is never freed
        // while its bus is attached (the C's invariant).
        unsafe { (*hosts.add(n as usize)).as_ref() }
    }
}

// SAFETY: `#[repr(C)]` with the device first; every other member is a `Cell` of an integer,
// a raw pointer, or an `Option` of a tag or a function pointer, all valid as zero bits.
unsafe impl Softc for SdhcSoftc {}
/* </CODE> */

/* <TESTS> */
#[cfg(test)]
mod tests {
    use super::*;
    use crate::reftest;

    #[test]
    #[ignore = "reads the C reference (just test-ref)"]
    fn defines_match_the_reference() {
        let defs = reftest::defines("sys/dev/sdmmc/sdhcvar.h");
        for (name, v) in [
            ("SDHC_F_NOPWR0", SDHC_F_NOPWR0),
            ("SDHC_F_NONREMOVABLE", SDHC_F_NONREMOVABLE),
            ("SDHC_F_32BIT_ACCESS", SDHC_F_32BIT_ACCESS),
            ("SDHC_F_NO_HS_BIT", SDHC_F_NO_HS_BIT),
        ] {
            assert_eq!(reftest::int(&defs, name), Some(i64::from(v)), "{name}");
        }
    }
}
/* </TESTS> */
