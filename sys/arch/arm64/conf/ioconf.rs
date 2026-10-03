//! The arm64 kernel's autoconfiguration tables: what `config(8)` writes into `ioconf.c` from
//! `arch/arm64/conf/GENERIC`, for the devices whose drivers are ported. Not an OpenBSD file:
//! `ioconf.c` is generated, and `config(8)` is replaced here by these hand-written tables
//! (`docs/ARCHITECTURE.md`, "Deviations"); `machine::autoconf` hands them to
//! `subr_autoconf.rs`.
//!
//! GENERIC lines present: `mainbus0 at root`, `ampintc* at fdt? early 1`, `agtimer* at fdt?`.
//! The `fdt` attribute (`files.arm64`: `define fdt {[early = 0]}`) is carried by `mainbus`
//! and `simplebus`; `simplebus` is not ported, so mainbus is the only parent here. Every
//! other GENERIC line waits for its driver (`cpu0 at mainbus?`, `simplebus* at fdt?`, ...),
//! as do the pseudo-devices (`pdevinit[]`). Each entry keeps `config(8)`'s layout:
//! attachment, driver, unit, state, locators, flags, parents (indices into `CFDATA`), the
//! start of its locator names and the first unit a starred entry may take.

use crate::arch::arm64::dev::agtimer::{AGTIMER_CA, AGTIMER_CD};
use crate::arch::arm64::dev::ampintc::{AMPINTC_CA, AMPINTC_CD};
use crate::arch::arm64::dev::mainbus::{MAINBUS_CA, MAINBUS_CD};
use crate::sys::device::{Cfdata, FSTATE_NOTFOUND, FSTATE_STAR};

/// `pv[]` for children of `mainbus0` (`cfdata[0]`) through the `fdt` attribute.
const PV_FDT: &[i16] = &[0];

/// `loc[]` of `early 1`.
const LOC_EARLY_1: &[i64] = &[1];

/// `loc[]` of `early 0`, the default.
const LOC_EARLY_0: &[i64] = &[0];

/// `cfdata[]`.
pub static CFDATA: [Cfdata; 3] = [
    // 0: mainbus0 at root
    Cfdata::new(
        &MAINBUS_CA,
        &MAINBUS_CD,
        0,
        FSTATE_NOTFOUND,
        &[],
        0,
        &[],
        0,
        0,
    ),
    // 1: ampintc* at fdt? early 1
    Cfdata::new(
        &AMPINTC_CA,
        &AMPINTC_CD,
        0,
        FSTATE_STAR,
        LOC_EARLY_1,
        0,
        PV_FDT,
        0,
        0,
    ),
    // 2: agtimer* at fdt?
    Cfdata::new(
        &AGTIMER_CA,
        &AGTIMER_CD,
        0,
        FSTATE_STAR,
        LOC_EARLY_0,
        0,
        PV_FDT,
        0,
        0,
    ),
];

/// `cfroots[]`: `mainbus0`.
pub static CFROOTS: [i16; 1] = [0];
