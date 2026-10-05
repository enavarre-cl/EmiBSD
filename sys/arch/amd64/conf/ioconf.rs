//! The amd64 kernel's autoconfiguration tables: what `config(8)` writes into `ioconf.c` from
//! `arch/amd64/conf/GENERIC`, for the devices whose drivers are ported. Not an OpenBSD file:
//! `ioconf.c` is generated, and `config(8)` is replaced here by these hand-written tables
//! (`docs/ARCHITECTURE.md`, "Deviations"); `machine::autoconf` hands them to
//! `subr_autoconf.rs`.
//!
//! GENERIC lines present: `mainbus0 at root`, `cpu0 at mainbus?` (and GENERIC.MP's
//! `cpu* at mainbus?` with feature `multiprocessor`), `pci* at mainbus0`,
//! `virtio* at pci?`, `vio* at virtio?`, `vioblk* at virtio?`, `nvme* at pci?`, `scsibus* at scsi?`,
//! `sd* at scsibus?`, `softraid0 at root` and `scsibus* at softraid?` (conf/GENERIC),
//! `isa0 at mainbus0`,
//! `com0 at isa? port 0x3f8 irq 4`, `com1 at isa? port 0x2f8 irq 3`, `com2 at isa? port 0x3e8
//! irq 5`, `com3 at isa? disable port 0x2e8 irq 9`; `pseudo-device pf`, `pseudo-device pflog`,
//! `pseudo-device pty 16`, `pseudo-device vnd 4`, `pseudo-device bpfilter`, `pseudo-device
//! loop`, `pseudo-device wg`, `pseudo-device pfsync`, `pseudo-device pflow`.
//! GENERIC lines left out until their drivers exist: `bios0`, `ioapic*`, `vmm0`, `pvbus0`,
//! `ipmi0` and `efifb0` at mainbus, and everything below them; `isa0` at `pcib?`,
//! `amdpcib?` and `tcpcib?`, and every other device at `isa?` (`isadma0`, `pckbc0`, `vga0`,
//! `pcppi0`, `lpt0`, `fdc0`, `wdc*`, the sensors, ...); every other device at `pci?`
//! (`pchb*`, `ppb*`, `pcib*`, the network drivers and the storage drivers but nvme, ...), `pci*` at `ppb?` and
//! `pchb?`, and every device at `virtio?` but `vio*` and `vioblk*`;
//! `mpath0 at root`; the other pseudo-devices (`pdevinit[]`). Each entry keeps `config(8)`'s
//! layout: attachment, driver, unit, state, locators, flags, parents (indices into
//! `CFDATA`), the start of its locator names and the first unit a starred entry may take.

use crate::arch::amd64::amd64::cpu::{CPU_CA, CPU_CD};
use crate::arch::amd64::amd64::mainbus::{MAINBUS_CA, MAINBUS_CD};
use crate::dev::bio::bioattach;
use crate::dev::ic::com::COM_CD;
use crate::dev::ic::nvme::NVME_CD;
use crate::dev::isa::com_isa::COM_ISA_CA;
use crate::dev::isa::isa::{ISA_CA, ISA_CD};
use crate::dev::pci::nvme_pci::NVME_PCI_CA;
use crate::dev::pci::pci::{PCI_CA, PCI_CD};
use crate::dev::pci::virtio_pci::VIRTIO_PCI_CA;
use crate::dev::pv::if_vio::{VIO_CA, VIO_CD};
use crate::dev::pv::vioblk::{VIOBLK_CA, VIOBLK_CD};
use crate::dev::pv::virtio::VIRTIO_CD;
use crate::dev::rd::rdattach;
use crate::dev::softraid::{SOFTRAID_CA, SOFTRAID_CD};
use crate::dev::vnd::{NVND, vndattach};
use crate::kern::tty_pty::ptyattach;
#[cfg(feature = "fuse")]
use crate::miscfs::fuse::fuse_device::{NFUSE, fuseattach};
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
use crate::sys::device::{Cfdata, FSTATE_DNOTFOUND, FSTATE_NOTFOUND, FSTATE_STAR, Pdevinit};

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

/// `pv[]` for children of `virtio*` (`cfdata[3]`).
const PV_VIRTIO: &[i16] = &[3];

/// `pv[]` for children of the `scsi` attribute, carried by `vioblk*` (`cfdata[5]`),
/// `softraid0` (`cfdata[13]`) and `nvme*` (`cfdata[14]`).
const PV_VIOBLK: &[i16] = &[5, 13, 14];

/// `pv[]` for children of `scsibus*` (`cfdata[11]`).
const PV_SCSIBUS: &[i16] = &[11];

/// `loc[]` of an entry at `scsibus` with the defaults `target = -1, lun = -1`
/// (`scsi/files.scsi`: `device scsibus {[target = -1], [lun = -1]}`).
const LOC_SCSIBUS_UNK: &[i64] = &[-1, -1];

/// `pv[]` for children of `isa0` (`cfdata[6]`).
const PV_ISA: &[i16] = &[6];

/// `loc[]` of `com0 at isa? port 0x3f8 irq 4`: `port`, `size`, `iomem`, `iosiz`, `irq`,
/// `drq`, `drq2`, the unset ones at their `files.isa` defaults.
const LOC_COM0: &[i64] = &[0x3f8, 0, -1, 0, 4, -1, -1];
/// `loc[]` of `com1 at isa? port 0x2f8 irq 3`.
const LOC_COM1: &[i64] = &[0x2f8, 0, -1, 0, 3, -1, -1];
/// `loc[]` of `com2 at isa? port 0x3e8 irq 5`.
const LOC_COM2: &[i64] = &[0x3e8, 0, -1, 0, 5, -1, -1];
/// `loc[]` of `com3 at isa? disable port 0x2e8 irq 9`.
const LOC_COM3: &[i64] = &[0x2e8, 0, -1, 0, 9, -1, -1];

/// `cfdata[]`: 15 entries, 16 with `MULTIPROCESSOR` (GENERIC.MP's `cpu* at mainbus?`).
const NCFDATA: usize = if cfg!(feature = "multiprocessor") {
    16
} else {
    15
};

/// `cfdata[]`.
pub static CFDATA: [Cfdata; NCFDATA] = [
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
    // 6: isa0 at mainbus0
    Cfdata::new(
        &ISA_CA,
        &ISA_CD,
        0,
        FSTATE_NOTFOUND,
        &[],
        0,
        PV_MAINBUS,
        0,
        0,
    ),
    // 7: com0 at isa? port 0x3f8 irq 4
    Cfdata::new(
        &COM_ISA_CA,
        &COM_CD,
        0,
        FSTATE_NOTFOUND,
        LOC_COM0,
        0,
        PV_ISA,
        0,
        0,
    ),
    // 8: com1 at isa? port 0x2f8 irq 3
    Cfdata::new(
        &COM_ISA_CA,
        &COM_CD,
        1,
        FSTATE_NOTFOUND,
        LOC_COM1,
        0,
        PV_ISA,
        0,
        0,
    ),
    // 9: com2 at isa? port 0x3e8 irq 5
    Cfdata::new(
        &COM_ISA_CA,
        &COM_CD,
        2,
        FSTATE_NOTFOUND,
        LOC_COM2,
        0,
        PV_ISA,
        0,
        0,
    ),
    // 10: com3 at isa? disable port 0x2e8 irq 9
    Cfdata::new(
        &COM_ISA_CA,
        &COM_CD,
        3,
        FSTATE_DNOTFOUND,
        LOC_COM3,
        0,
        PV_ISA,
        0,
        0,
    ),
    // 11: scsibus* at scsi? (vioblk, nvme), and at softraid? (GENERIC's `scsibus* at softraid?`)
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
    // 12: sd* at scsibus?
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
    // 13: softraid0 at root
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
    // 14: nvme* at pci?
    Cfdata::new(
        &NVME_PCI_CA,
        &NVME_CD,
        0,
        FSTATE_STAR,
        LOC_PCI_UNK,
        0,
        PV_PCI,
        0,
        0,
    ),
    // 15: cpu* at mainbus? (GENERIC.MP, MULTIPROCESSOR): the application processors, unit 1
    // on (cpu0 takes unit 0).
    #[cfg(feature = "multiprocessor")]
    Cfdata::new(&CPU_CA, &CPU_CD, 1, FSTATE_STAR, &[], 0, PV_MAINBUS, 0, 1),
];

/// `cfroots[]`: `mainbus0`, `softraid0`.
pub static CFROOTS: [i16; 2] = [0, 13];

/// `pdevinit[]`: the pseudo-devices of the MI `conf/GENERIC` whose attach functions are
/// ported, in `ioconf.c`'s order (`pseudo-device pf`, `pseudo-device pflog`, `pseudo-device
/// pfsync`, `pseudo-device pflow`, `pseudo-device enc`, `pseudo-device pty 16`, `pseudo-device
/// vnd 4`, `pseudo-device bpfilter`, `pseudo-device loop`, `pseudo-device wg`, `pseudo-device
/// bio 1`, `pseudo-device fuse` under feature `fuse`; all but pty and vnd with a count of 1), then `pseudo-device rd 1`, which is not in
/// GENERIC but in the RAMDISK kernels (`arch/amd64/conf/RAMDISK*`): this kernel boots its root
/// from rd0a (M8).
pub static PDEVINIT: [Pdevinit; 12 + cfg!(feature = "fuse") as usize] = [
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
    #[cfg(feature = "fuse")]
    Pdevinit {
        pdev_attach: fuseattach,
        pdev_count: NFUSE,
    },
    Pdevinit {
        pdev_attach: rdattach,
        pdev_count: 1,
    },
];
