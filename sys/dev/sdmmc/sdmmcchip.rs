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
//! `<dev/sdmmc/sdmmcchip.h>`: the interface between the SD/MMC bus (`sdmmc(4)`) and the host
//! controller drivers below it (`sdhc(4)`, ...): the chip function table, the bus clock,
//! signal voltage and timing selectors, and the bus attach arguments.
//!
//! Upstream: sys/dev/sdmmc/sdmmcchip.h @ 3ce1f3f79392
//!
//! The functions declared here (`sdmmc_needs_discover`, `sdmmc_card_intr`, `sdmmc_delay`)
//! live in `sdmmc.rs` and `sdmmc_io.rs`, the modules of the C files that define them.
//!
//! ## Deviations
//! - `struct sdmmc_chip_functions` keeps the C's members; the ones a controller may leave
//!   NULL (`card_intr_mask`, `card_intr_ack`, `signal_voltage`, `execute_tuning`,
//!   `hibernate_init`; `sdhc(4)` leaves `execute_tuning` NULL) are `Option`s, the others
//!   plain function pointers. A call through a missing member panics where the C would jump
//!   to address zero.
//! - The chip functions return `Result<(), Errno>` where the C returns 0 or an errno, and
//!   `card_detect` a `bool` (the C's "non-zero if the card is inserted").
//! - The `sdmmc_chip_*` macros are functions; `saa_busname` is a byte string (`cd_name`'s
//!   type); `caps` is a `u32`, the type of the `sdmmc_softc`'s `sc_caps` it is copied to.

use core::ffi::c_void;
use core::ptr;

use crate::dev::sdmmc::sdmmcvar::SdmmcCommand;
use crate::kern::subr_prf::panic;
use crate::machine::bus::{BusDmaTag, BusDmamap, BusSize};
use crate::sys::errno::Errno;

/* clock frequencies for sdmmc_chip_bus_clock() */

/// `SDMMC_SDCLK_OFF`.
pub const SDMMC_SDCLK_OFF: i32 = 0;
/// `SDMMC_SDCLK_400KHZ`.
pub const SDMMC_SDCLK_400KHZ: i32 = 400;
/// `SDMMC_SDCLK_25MHZ`.
pub const SDMMC_SDCLK_25MHZ: i32 = 25000;
/// `SDMMC_SDCLK_50MHZ`.
pub const SDMMC_SDCLK_50MHZ: i32 = 50000;

/* voltage levels for sdmmc_chip_signal_voltage() */

/// `SDMMC_SIGNAL_VOLTAGE_330`.
pub const SDMMC_SIGNAL_VOLTAGE_330: i32 = 0;
/// `SDMMC_SIGNAL_VOLTAGE_180`.
pub const SDMMC_SIGNAL_VOLTAGE_180: i32 = 1;

/// `SDMMC_TIMING_LEGACY`.
pub const SDMMC_TIMING_LEGACY: i32 = 0;
/// `SDMMC_TIMING_HIGHSPEED`.
pub const SDMMC_TIMING_HIGHSPEED: i32 = 1;
/// `SDMMC_TIMING_UHS_SDR50`.
pub const SDMMC_TIMING_UHS_SDR50: i32 = 2;
/// `SDMMC_TIMING_UHS_SDR104`.
pub const SDMMC_TIMING_UHS_SDR104: i32 = 3;
/// `SDMMC_TIMING_MMC_DDR52`.
pub const SDMMC_TIMING_MMC_DDR52: i32 = 4;
/// `SDMMC_TIMING_MMC_HS200`.
pub const SDMMC_TIMING_MMC_HS200: i32 = 5;

/// `SDMMC_MAX_FUNCTIONS`.
pub const SDMMC_MAX_FUNCTIONS: usize = 8;

/// `sdmmc_chipset_handle_t`: the host controller's handle (`sdhc(4)`'s `struct sdhc_host`).
pub type SdmmcChipsetHandle = *mut c_void;

/// `sdmmc_chipset_tag_t`: the host controller's function table.
pub type SdmmcChipsetTag = &'static SdmmcChipFunctions;

/// `int (*)(sdmmc_chipset_handle_t, int)`: a chip function that sets one mode
/// (`signal_voltage`, `execute_tuning`).
pub type SdmmcChipSetFn = fn(SdmmcChipsetHandle, i32) -> Result<(), Errno>;

/// `struct sdmmc_chip_functions`.
pub struct SdmmcChipFunctions {
    /* host controller reset */
    /// `host_reset`.
    pub host_reset: fn(SdmmcChipsetHandle) -> Result<(), Errno>,
    /* host capabilities */
    /// `host_ocr`.
    pub host_ocr: fn(SdmmcChipsetHandle) -> u32,
    /// `host_maxblklen`.
    pub host_maxblklen: fn(SdmmcChipsetHandle) -> i32,
    /* card detection */
    /// `card_detect`.
    pub card_detect: fn(SdmmcChipsetHandle) -> bool,
    /* bus power and clock frequency */
    /// `bus_power`.
    pub bus_power: fn(SdmmcChipsetHandle, u32) -> Result<(), Errno>,
    /// `bus_clock`.
    pub bus_clock: fn(SdmmcChipsetHandle, i32, i32) -> Result<(), Errno>,
    /// `bus_width`.
    pub bus_width: fn(SdmmcChipsetHandle, i32) -> Result<(), Errno>,
    /* command execution */
    /// `exec_command`.
    pub exec_command: fn(SdmmcChipsetHandle, &mut SdmmcCommand),
    /* card interrupt */
    /// `card_intr_mask`.
    pub card_intr_mask: Option<fn(SdmmcChipsetHandle, i32)>,
    /// `card_intr_ack`.
    pub card_intr_ack: Option<fn(SdmmcChipsetHandle)>,
    /* UHS functions */
    /// `signal_voltage`.
    pub signal_voltage: Option<SdmmcChipSetFn>,
    /// `execute_tuning`.
    pub execute_tuning: Option<SdmmcChipSetFn>,
    /* hibernate */
    /// `hibernate_init`: copies the controller's state into the fake softc at the second
    /// argument.
    pub hibernate_init: Option<HibernateInitFn>,
}

/// `int (*hibernate_init)(sdmmc_chipset_handle_t, void *)`.
///
/// # Safety
///
/// The second argument points to writable memory large enough for the controller's state.
pub type HibernateInitFn = unsafe fn(SdmmcChipsetHandle, *mut c_void) -> Result<(), Errno>;

/// `struct sdmmcbus_attach_args`.
pub struct SdmmcbusAttachArgs {
    /// `saa_busname`.
    pub saa_busname: &'static [u8],
    /// `sct`.
    pub sct: Option<SdmmcChipsetTag>,
    /// `sch`.
    pub sch: SdmmcChipsetHandle,
    /// `dmat`.
    pub dmat: Option<BusDmaTag>,
    /// `dmap`.
    pub dmap: Option<&'static BusDmamap>,
    /// `flags`.
    pub flags: i32,
    /// `caps`.
    pub caps: u32,
    /// `max_seg`.
    pub max_seg: i64,
    /// `max_xfer`.
    pub max_xfer: i64,
    /// `dma_boundary`.
    pub dma_boundary: BusSize,
    /// `cookies`.
    pub cookies: [*mut c_void; SDMMC_MAX_FUNCTIONS],
}

impl SdmmcbusAttachArgs {
    /// All zero (`bzero(&saa, sizeof(saa))`).
    pub const fn new() -> Self {
        Self {
            saa_busname: b"",
            sct: None,
            sch: ptr::null_mut(),
            dmat: None,
            dmap: None,
            flags: 0,
            caps: 0,
            max_seg: 0,
            max_xfer: 0,
            dma_boundary: 0,
            cookies: [ptr::null_mut(); SDMMC_MAX_FUNCTIONS],
        }
    }
}

impl Default for SdmmcbusAttachArgs {
    fn default() -> Self {
        Self::new()
    }
}

/* host controller reset */

/// `sdmmc_chip_host_reset(tag, handle)`.
pub fn sdmmc_chip_host_reset(
    tag: SdmmcChipsetTag,
    handle: SdmmcChipsetHandle,
) -> Result<(), Errno> {
    (tag.host_reset)(handle)
}

/* host capabilities */

/// `sdmmc_chip_host_ocr(tag, handle)`.
pub fn sdmmc_chip_host_ocr(tag: SdmmcChipsetTag, handle: SdmmcChipsetHandle) -> u32 {
    (tag.host_ocr)(handle)
}

/// `sdmmc_chip_host_maxblklen(tag, handle)`.
pub fn sdmmc_chip_host_maxblklen(tag: SdmmcChipsetTag, handle: SdmmcChipsetHandle) -> i32 {
    (tag.host_maxblklen)(handle)
}

/* card detection */

/// `sdmmc_chip_card_detect(tag, handle)`.
pub fn sdmmc_chip_card_detect(tag: SdmmcChipsetTag, handle: SdmmcChipsetHandle) -> bool {
    (tag.card_detect)(handle)
}

/* bus power and clock frequency */

/// `sdmmc_chip_bus_power(tag, handle, ocr)`.
pub fn sdmmc_chip_bus_power(
    tag: SdmmcChipsetTag,
    handle: SdmmcChipsetHandle,
    ocr: u32,
) -> Result<(), Errno> {
    (tag.bus_power)(handle, ocr)
}

/// `sdmmc_chip_bus_clock(tag, handle, freq, timing)`.
pub fn sdmmc_chip_bus_clock(
    tag: SdmmcChipsetTag,
    handle: SdmmcChipsetHandle,
    freq: i32,
    timing: i32,
) -> Result<(), Errno> {
    (tag.bus_clock)(handle, freq, timing)
}

/// `sdmmc_chip_bus_width(tag, handle, width)`.
pub fn sdmmc_chip_bus_width(
    tag: SdmmcChipsetTag,
    handle: SdmmcChipsetHandle,
    width: i32,
) -> Result<(), Errno> {
    (tag.bus_width)(handle, width)
}

/* command execution */

/// `sdmmc_chip_exec_command(tag, handle, cmdp)`.
pub fn sdmmc_chip_exec_command(
    tag: SdmmcChipsetTag,
    handle: SdmmcChipsetHandle,
    cmdp: &mut SdmmcCommand,
) {
    (tag.exec_command)(handle, cmdp)
}

/* card interrupt */

/// `sdmmc_chip_card_intr_mask(tag, handle, enable)`.
pub fn sdmmc_chip_card_intr_mask(tag: SdmmcChipsetTag, handle: SdmmcChipsetHandle, enable: i32) {
    match tag.card_intr_mask {
        Some(f) => f(handle, enable),
        None => panic(format_args!("sdmmc_chip_card_intr_mask: no chip function")),
    }
}

/// `sdmmc_chip_card_intr_ack(tag, handle)`.
pub fn sdmmc_chip_card_intr_ack(tag: SdmmcChipsetTag, handle: SdmmcChipsetHandle) {
    match tag.card_intr_ack {
        Some(f) => f(handle),
        None => panic(format_args!("sdmmc_chip_card_intr_ack: no chip function")),
    }
}

/* UHS functions */

/// `sdmmc_chip_signal_voltage(tag, handle, voltage)`.
pub fn sdmmc_chip_signal_voltage(
    tag: SdmmcChipsetTag,
    handle: SdmmcChipsetHandle,
    voltage: i32,
) -> Result<(), Errno> {
    match tag.signal_voltage {
        Some(f) => f(handle, voltage),
        None => panic(format_args!("sdmmc_chip_signal_voltage: no chip function")),
    }
}

/// `sdmmc_chip_execute_tuning(tag, handle, timing)`.
pub fn sdmmc_chip_execute_tuning(
    tag: SdmmcChipsetTag,
    handle: SdmmcChipsetHandle,
    timing: i32,
) -> Result<(), Errno> {
    match tag.execute_tuning {
        Some(f) => f(handle, timing),
        None => panic(format_args!("sdmmc_chip_execute_tuning: no chip function")),
    }
}
/* </CODE> */

/* <TESTS> */
#[cfg(test)]
mod tests {
    use super::*;
    use crate::reftest;

    #[test]
    fn attach_args_start_zeroed() {
        let saa = SdmmcbusAttachArgs::new();
        assert!(saa.sct.is_none() && saa.sch.is_null() && saa.caps == 0);
        assert!(saa.cookies.iter().all(|c| c.is_null()));
    }

    #[test]
    #[ignore = "reads the C reference (just test-ref)"]
    fn defines_match_the_reference() {
        let defs = reftest::defines("sys/dev/sdmmc/sdmmcchip.h");
        let ours: &[(&str, i64)] = &[
            ("SDMMC_SDCLK_OFF", SDMMC_SDCLK_OFF.into()),
            ("SDMMC_SDCLK_400KHZ", SDMMC_SDCLK_400KHZ.into()),
            ("SDMMC_SDCLK_25MHZ", SDMMC_SDCLK_25MHZ.into()),
            ("SDMMC_SDCLK_50MHZ", SDMMC_SDCLK_50MHZ.into()),
            ("SDMMC_SIGNAL_VOLTAGE_330", SDMMC_SIGNAL_VOLTAGE_330.into()),
            ("SDMMC_SIGNAL_VOLTAGE_180", SDMMC_SIGNAL_VOLTAGE_180.into()),
            ("SDMMC_TIMING_LEGACY", SDMMC_TIMING_LEGACY.into()),
            ("SDMMC_TIMING_HIGHSPEED", SDMMC_TIMING_HIGHSPEED.into()),
            ("SDMMC_TIMING_UHS_SDR50", SDMMC_TIMING_UHS_SDR50.into()),
            ("SDMMC_TIMING_UHS_SDR104", SDMMC_TIMING_UHS_SDR104.into()),
            ("SDMMC_TIMING_MMC_DDR52", SDMMC_TIMING_MMC_DDR52.into()),
            ("SDMMC_TIMING_MMC_HS200", SDMMC_TIMING_MMC_HS200.into()),
            ("SDMMC_MAX_FUNCTIONS", SDMMC_MAX_FUNCTIONS as i64),
        ];
        for &(name, v) in ours {
            assert_eq!(reftest::int(&defs, name), Some(v), "{name}");
        }
    }
}
/* </TESTS> */
