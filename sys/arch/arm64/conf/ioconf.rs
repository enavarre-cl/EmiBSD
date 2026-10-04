//! The arm64 kernel's autoconfiguration tables: what `config(8)` writes into `ioconf.c` from
//! `arch/arm64/conf/GENERIC`, for the devices whose drivers are ported. Not an OpenBSD file:
//! `ioconf.c` is generated, and `config(8)` is replaced here by these hand-written tables
//! (`docs/ARCHITECTURE.md`, "Deviations"); `machine::autoconf` hands them to
//! `subr_autoconf.rs`.
//!
//! GENERIC lines present: `mainbus0 at root`, `ampintc* at fdt? early 1`, `agtimer* at fdt?`,
//! `virtio* at fdt?`, `vio* at virtio?`, `vioblk* at virtio?`, `scsibus* at scsi?`,
//! `sd* at scsibus?`, `softraid0 at root` and `scsibus* at softraid?` (conf/GENERIC),
//! `pluart* at fdt?`,
//! `plrtc* at fdt?`, `efi0 at mainbus?`;
//! `pseudo-device pf`, `pseudo-device pflog`, `pseudo-device pty 16`, `pseudo-device vnd 4`,
//! `pseudo-device bpfilter`, `pseudo-device loop`, `pseudo-device wg`, `pseudo-device pfsync`,
//! `pseudo-device pflow`.
//! The `fdt` attribute (`files.arm64`: `define fdt {[early = 0]}`) is carried by `mainbus`
//! and `simplebus`; `simplebus` is not ported, so mainbus is the only parent here. Every
//! other GENERIC line waits for its driver (`cpu0 at mainbus?`, `smbios0 at efi?`,
//! `simplebus* at fdt?`, the devices at `virtio?` but `vio*` and `vioblk*`, `virtio* at pci?`
//! with a host bridge driver, ...),
//! as do the other pseudo-devices (`pdevinit[]`). Each entry keeps `config(8)`'s layout:
//! attachment, driver, unit, state, locators, flags, parents (indices into `CFDATA`), the
//! start of its locator names and the first unit a starred entry may take.

use crate::arch::arm64::dev::agtimer::{AGTIMER_CA, AGTIMER_CD};
use crate::arch::arm64::dev::ampintc::{AMPINTC_CA, AMPINTC_CD};
use crate::arch::arm64::dev::efi_machdep::{EFI_CA, EFI_CD};
use crate::arch::arm64::dev::mainbus::{MAINBUS_CA, MAINBUS_CD};
use crate::dev::bio::bioattach;
use crate::dev::fdt::plrtc::{PLRTC_CA, PLRTC_CD};
use crate::dev::fdt::pluart_fdt::PLUART_FDT_CA;
use crate::dev::fdt::virtio_mmio::VIRTIO_MMIO_CA;
use crate::dev::ic::pluart::PLUART_CD;
use crate::dev::pv::if_vio::{VIO_CA, VIO_CD};
use crate::dev::pv::vioblk::{VIOBLK_CA, VIOBLK_CD};
use crate::dev::pv::virtio::VIRTIO_CD;
use crate::dev::rd::rdattach;
use crate::dev::softraid::{SOFTRAID_CA, SOFTRAID_CD};
use crate::dev::vnd::{NVND, vndattach};
use crate::kern::tty_pty::ptyattach;
use crate::net::bpf::bpfilterattach;
use crate::net::if_enc::encattach;
use crate::net::if_loop::loopattach;
use crate::net::if_pflog::pflogattach;
use crate::net::if_pflow::pflowattach;
use crate::net::if_pfsync::pfsyncattach;
use crate::net::if_wg::wgattach;
use crate::net::pf_ioctl::pfattach;
use crate::scsi::scsiconf::{SCSIBUS_CA, SCSIBUS_CD};
use crate::scsi::sd::{SD_CA, SD_CD};
use crate::sys::device::{Cfdata, FSTATE_NOTFOUND, FSTATE_STAR, Pdevinit};

/// `pv[]` for children of `mainbus0` (`cfdata[0]`) through the `fdt` attribute.
const PV_FDT: &[i16] = &[0];

/// `pv[]` for children of `mainbus0` (`cfdata[0]`) through `mainbus` itself.
const PV_MAINBUS: &[i16] = &[0];

/// `loc[]` of `early 1`.
const LOC_EARLY_1: &[i64] = &[1];

/// `loc[]` of `early 0`, the default.
const LOC_EARLY_0: &[i64] = &[0];

/// `pv[]` for children of `virtio*` (`cfdata[3]`).
const PV_VIRTIO: &[i16] = &[3];

/// `pv[]` for children of the `scsi` attribute, carried by `vioblk*` (`cfdata[5]`) and
/// `softraid0` (`cfdata[11]`).
const PV_VIOBLK: &[i16] = &[5, 11];

/// `pv[]` for children of `scsibus*` (`cfdata[9]`).
const PV_SCSIBUS: &[i16] = &[9];

/// `loc[]` of an entry at `scsibus` with the defaults `target = -1, lun = -1`
/// (`scsi/files.scsi`: `device scsibus {[target = -1], [lun = -1]}`).
const LOC_SCSIBUS_UNK: &[i64] = &[-1, -1];

/// `cfdata[]`.
pub static CFDATA: [Cfdata; 12] = [
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
    // 5: vioblk* at virtio?
    Cfdata::new(
        &VIOBLK_CA,
        &VIOBLK_CD,
        0,
        FSTATE_STAR,
        &[],
        0,
        PV_VIRTIO,
        0,
        0,
    ),
    // 6: pluart* at fdt?
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
    // 7: plrtc* at fdt?
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
    // 8: efi0 at mainbus?
    Cfdata::new(
        &EFI_CA,
        &EFI_CD,
        0,
        FSTATE_NOTFOUND,
        &[],
        0,
        PV_MAINBUS,
        0,
        0,
    ),
    // 9: scsibus* at scsi? (vioblk), and at softraid? (GENERIC's `scsibus* at softraid?`)
    Cfdata::new(
        &SCSIBUS_CA,
        &SCSIBUS_CD,
        0,
        FSTATE_STAR,
        &[],
        0,
        PV_VIOBLK,
        0,
        0,
    ),
    // 10: sd* at scsibus?
    Cfdata::new(
        &SD_CA,
        &SD_CD,
        0,
        FSTATE_STAR,
        LOC_SCSIBUS_UNK,
        0,
        PV_SCSIBUS,
        0,
        0,
    ),
    // 11: softraid0 at root
    Cfdata::new(
        &SOFTRAID_CA,
        &SOFTRAID_CD,
        0,
        FSTATE_NOTFOUND,
        &[],
        0,
        &[],
        0,
        0,
    ),
];

/// `cfroots[]`: `mainbus0`, `softraid0`.
pub static CFROOTS: [i16; 2] = [0, 11];

/// `pdevinit[]`: the pseudo-devices of the MI `conf/GENERIC` whose attach functions are
/// ported, in `ioconf.c`'s order (`pseudo-device pf`, `pseudo-device pflog`, `pseudo-device
/// pfsync`, `pseudo-device pflow`, `pseudo-device enc`, `pseudo-device pty 16`, `pseudo-device
/// vnd 4`, `pseudo-device bpfilter`, `pseudo-device loop`, `pseudo-device wg`, `pseudo-device
/// bio 1`; all but pty and vnd with a count of 1), then `pseudo-device rd 1`, which is not in
/// GENERIC but in the RAMDISK kernels (`arch/arm64/conf/RAMDISK*`): this kernel boots its root
/// from rd0a (M8).
pub static PDEVINIT: [Pdevinit; 12] = [
    Pdevinit {
        pdev_attach: pfattach,
        pdev_count: 1,
    },
    Pdevinit {
        pdev_attach: pflogattach,
        pdev_count: 1,
    },
    Pdevinit {
        pdev_attach: pfsyncattach,
        pdev_count: 1,
    },
    Pdevinit {
        pdev_attach: pflowattach,
        pdev_count: 1,
    },
    Pdevinit {
        pdev_attach: encattach,
        pdev_count: 1,
    },
    Pdevinit {
        pdev_attach: ptyattach,
        pdev_count: 16,
    },
    Pdevinit {
        pdev_attach: vndattach,
        pdev_count: NVND,
    },
    Pdevinit {
        pdev_attach: bpfilterattach,
        pdev_count: 1,
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
        pdev_attach: bioattach,
        pdev_count: 1,
    },
    Pdevinit {
        pdev_attach: rdattach,
        pdev_count: 1,
    },
];
