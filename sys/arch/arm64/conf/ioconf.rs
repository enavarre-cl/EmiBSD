//! The arm64 kernel's autoconfiguration tables: what `config(8)` writes into `ioconf.c` from
//! `arch/arm64/conf/GENERIC`, for the devices whose drivers are ported. Not an OpenBSD file:
//! `ioconf.c` is generated, and `config(8)` is replaced here by these hand-written tables
//! (`docs/ARCHITECTURE.md`, "Deviations"); `machine::autoconf` hands them to
//! `subr_autoconf.rs`.
//!
//! GENERIC lines present: `mainbus0 at root`, `ampintc* at fdt? early 1`, `agtimer* at fdt?`,
//! `virtio* at fdt?`, `vio* at virtio?`, `pluart* at fdt?`, `plrtc* at fdt?`;
//! `pseudo-device pty 16`,
//! `pseudo-device loop`, `pseudo-device wg`.
//! The `fdt` attribute (`files.arm64`: `define fdt {[early = 0]}`) is carried by `mainbus`
//! and `simplebus`; `simplebus` is not ported, so mainbus is the only parent here. Every
//! other GENERIC line waits for its driver (`cpu0 at mainbus?`, `simplebus* at fdt?`, the
//! devices at `virtio?` but `vio*`, `virtio* at pci?` with a host bridge driver, ...),
//! as do the other pseudo-devices (`pdevinit[]`). Each entry keeps `config(8)`'s layout:
//! attachment, driver, unit, state, locators, flags, parents (indices into `CFDATA`), the
//! start of its locator names and the first unit a starred entry may take.

use crate::arch::arm64::dev::agtimer::{AGTIMER_CA, AGTIMER_CD};
use crate::arch::arm64::dev::ampintc::{AMPINTC_CA, AMPINTC_CD};
use crate::arch::arm64::dev::mainbus::{MAINBUS_CA, MAINBUS_CD};
use crate::dev::fdt::plrtc::{PLRTC_CA, PLRTC_CD};
use crate::dev::fdt::pluart_fdt::PLUART_FDT_CA;
use crate::dev::fdt::virtio_mmio::VIRTIO_MMIO_CA;
use crate::dev::ic::pluart::PLUART_CD;
use crate::dev::pv::if_vio::{VIO_CA, VIO_CD};
use crate::dev::pv::virtio::VIRTIO_CD;
use crate::dev::rd::rdattach;
use crate::kern::tty_pty::ptyattach;
use crate::net::if_loop::loopattach;
use crate::net::if_wg::wgattach;
use crate::sys::device::{Cfdata, FSTATE_NOTFOUND, FSTATE_STAR, Pdevinit};

/// `pv[]` for children of `mainbus0` (`cfdata[0]`) through the `fdt` attribute.
const PV_FDT: &[i16] = &[0];

/// `loc[]` of `early 1`.
const LOC_EARLY_1: &[i64] = &[1];

/// `loc[]` of `early 0`, the default.
const LOC_EARLY_0: &[i64] = &[0];

/// `pv[]` for children of `virtio*` (`cfdata[3]`).
const PV_VIRTIO: &[i16] = &[3];

/// `cfdata[]`.
pub static CFDATA: [Cfdata; 7] = [
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
    // 3: virtio* at fdt?
    Cfdata::new(
        &VIRTIO_MMIO_CA,
        &VIRTIO_CD,
        0,
        FSTATE_STAR,
        LOC_EARLY_0,
        0,
        PV_FDT,
        0,
        0,
    ),
    // 4: vio* at virtio?
    Cfdata::new(&VIO_CA, &VIO_CD, 0, FSTATE_STAR, &[], 0, PV_VIRTIO, 0, 0),
    // 5: pluart* at fdt?
    Cfdata::new(
        &PLUART_FDT_CA,
        &PLUART_CD,
        0,
        FSTATE_STAR,
        LOC_EARLY_0,
        0,
        PV_FDT,
        0,
        0,
    ),
    // 6: plrtc* at fdt?
    Cfdata::new(
        &PLRTC_CA,
        &PLRTC_CD,
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

/// `pdevinit[]`: the pseudo-devices of the MI `conf/GENERIC` whose attach functions are
/// ported, in `ioconf.c`'s order (`pseudo-device pty 16`, `pseudo-device loop` and
/// `pseudo-device wg` with a count of 1), then `pseudo-device rd 1`, which is not in GENERIC
/// but in the RAMDISK kernels (`arch/arm64/conf/RAMDISK*`): this kernel boots its root from
/// rd0a (M8).
pub static PDEVINIT: [Pdevinit; 4] = [
    Pdevinit {
        pdev_attach: ptyattach,
        pdev_count: 16,
    },
    Pdevinit {
        pdev_attach: loopattach,
        pdev_count: 1,
    },
    Pdevinit {
        pdev_attach: wgattach,
        pdev_count: 1,
    },
    Pdevinit {
        pdev_attach: rdattach,
        pdev_count: 1,
    },
];
