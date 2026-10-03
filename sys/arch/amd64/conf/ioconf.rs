//! The amd64 kernel's autoconfiguration tables: what `config(8)` writes into `ioconf.c` from
//! `arch/amd64/conf/GENERIC`, for the devices whose drivers are ported. Not an OpenBSD file:
//! `ioconf.c` is generated, and `config(8)` is replaced here by these hand-written tables
//! (`docs/ARCHITECTURE.md`, "Deviations"); `machine::autoconf` hands them to
//! `subr_autoconf.rs`.
//!
//! GENERIC lines present: `mainbus0 at root`, `cpu0 at mainbus?`, `pci* at mainbus0`,
//! `virtio* at pci?`; `pseudo-device loop`.
//! GENERIC lines left out until their drivers exist: `bios0`, `ioapic*`, `isa0`, `vmm0`,
//! `pvbus0`, `ipmi0` and `efifb0` at mainbus, and everything below them; every other device
//! at `pci?` (`pchb*`, `ppb*`, `pcib*`, the network and storage drivers, ...), `pci*` at
//! `ppb?` and `pchb?`, and every device at `virtio?` but `vio*`;
//! `mpath0 at root`; the other pseudo-devices (`pdevinit[]`). Each entry keeps `config(8)`'s
//! layout: attachment, driver, unit, state, locators, flags, parents (indices into
//! `CFDATA`), the start of its locator names and the first unit a starred entry may take.

use crate::arch::amd64::amd64::cpu::{CPU_CA, CPU_CD};
use crate::arch::amd64::amd64::mainbus::{MAINBUS_CA, MAINBUS_CD};
use crate::dev::pci::pci::{PCI_CA, PCI_CD};
use crate::dev::pci::virtio_pci::VIRTIO_PCI_CA;
use crate::dev::pv::virtio::VIRTIO_CD;
use crate::net::if_loop::loopattach;
use crate::sys::device::{Cfdata, FSTATE_NOTFOUND, FSTATE_STAR, Pdevinit};

/// `pv[]` for children of `mainbus0` (`cfdata[0]`).
const PV_MAINBUS: &[i16] = &[0];

/// `loc[]` of an entry at `pcibus` with the default `bus = -1` (`conf/files`: `define pcibus
/// {[bus = -1]}`).
const LOC_PCIBUS_UNK: &[i64] = &[-1];

/// `pv[]` for children of `pci*` (`cfdata[2]`).
const PV_PCI: &[i16] = &[2];

/// `loc[]` of an entry at `pci` with the defaults `dev = -1, function = -1` (`conf/files`:
/// `device pci {[dev = -1], [function = -1]}`).
const LOC_PCI_UNK: &[i64] = &[-1, -1];

/// `cfdata[]`.
pub static CFDATA: [Cfdata; 4] = [
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
    // 2: pci* at mainbus0
    Cfdata::new(
        &PCI_CA,
        &PCI_CD,
        0,
        FSTATE_STAR,
        LOC_PCIBUS_UNK,
        0,
        PV_MAINBUS,
        0,
        0,
    ),
    // 3: virtio* at pci?
    Cfdata::new(
        &VIRTIO_PCI_CA,
        &VIRTIO_CD,
        0,
        FSTATE_STAR,
        LOC_PCI_UNK,
        0,
        PV_PCI,
        0,
        0,
    ),
];

/// `cfroots[]`: `mainbus0`.
pub static CFROOTS: [i16; 1] = [0];

/// `pdevinit[]`: the pseudo-devices of the MI `conf/GENERIC` whose attach functions are
/// ported, in `ioconf.c`'s order (`pseudo-device loop` gets a count of 1).
pub static PDEVINIT: [Pdevinit; 1] = [Pdevinit {
    pdev_attach: loopattach,
    pdev_count: 1,
}];
