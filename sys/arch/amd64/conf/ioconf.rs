//! The amd64 kernel's autoconfiguration tables: what `config(8)` writes into `ioconf.c` from
//! `arch/amd64/conf/GENERIC`, for the devices whose drivers are ported. Not an OpenBSD file:
//! `ioconf.c` is generated, and `config(8)` is replaced here by these hand-written tables
//! (`docs/ARCHITECTURE.md`, "Deviations"); `machine::autoconf` hands them to
//! `subr_autoconf.rs`.
//!
//! GENERIC lines present: `mainbus0 at root`, `cpu0 at mainbus?`.
//! GENERIC lines left out until their drivers exist: `bios0`, `ioapic*`, `isa0`, `pci*`,
//! `vmm0`, `pvbus0`, `ipmi0` and `efifb0` at mainbus, and everything below them;
//! `mpath0 at root`; the pseudo-devices (`pdevinit[]`). Each entry keeps `config(8)`'s
//! layout: attachment, driver, unit, state, locators, flags, parents (indices into
//! `CFDATA`), the start of its locator names and the first unit a starred entry may take.

use crate::arch::amd64::amd64::cpu::{CPU_CA, CPU_CD};
use crate::arch::amd64::amd64::mainbus::{MAINBUS_CA, MAINBUS_CD};
use crate::sys::device::{Cfdata, FSTATE_NOTFOUND};

/// `pv[]` for children of `mainbus0` (`cfdata[0]`).
const PV_MAINBUS: &[i16] = &[0];

/// `cfdata[]`.
pub static CFDATA: [Cfdata; 2] = [
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
    // 1: cpu0 at mainbus?
    Cfdata::new(
        &CPU_CA,
        &CPU_CD,
        0,
        FSTATE_NOTFOUND,
        &[],
        0,
        PV_MAINBUS,
        0,
        0,
    ),
];

/// `cfroots[]`: `mainbus0`.
pub static CFROOTS: [i16; 1] = [0];
