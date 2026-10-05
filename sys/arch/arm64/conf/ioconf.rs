//! The arm64 kernel's autoconfiguration tables: what `config(8)` writes into `ioconf.c` from
//! `arch/arm64/conf/GENERIC`, for the devices whose drivers are ported. Not an OpenBSD file:
//! `ioconf.c` is generated, and `config(8)` is replaced here by these hand-written tables
//! (`docs/ARCHITECTURE.md`, "Deviations"); `machine::autoconf` hands them to
//! `subr_autoconf.rs`.
//!
//! GENERIC lines present: `vioscsi* at virtio?`, `cd* at scsibus?` (M13), `mainbus0 at root`, `ampintc* at fdt? early 1`, `agtimer* at fdt?`,
//! `virtio* at fdt?`, `vio* at virtio?`, `vioblk* at virtio?`, `scsibus* at scsi?`,
//! `sd* at scsibus?`, `softraid0 at root` and `scsibus* at softraid?` (conf/GENERIC),
//! `pluart* at fdt?`,
//! `plrtc* at fdt?`, `efi0 at mainbus?`, `simplebus* at fdt?`, `ampintcmsi* at fdt? early
//! 1`, `pciecam* at fdt?`, `pci* at pciecam?`, `virtio* at pci?`, `xhci* at pci?`, `usb* at
//! xhci?`, `uhub* at usb?`, `uhub* at uhub?` (M12), `cpu0 at mainbus?`
//! and, with `MULTIPROCESSOR`, `GENERIC.MP`'s `cpu* at mainbus?`;
//! `azalia* at pci?` and `audio* at azalia?` (M12);
//! `pseudo-device pf`, `pseudo-device pflog`, `pseudo-device pty 16`, `pseudo-device vnd 4`,
//! `pseudo-device bpfilter`, `pseudo-device loop`, `pseudo-device wg`, `pseudo-device pfsync`,
//! `pseudo-device pflow`.
//! The `fdt` attribute (`files.arm64`: `define fdt {[early = 0]}`) is carried by `mainbus`,
//! `simplebus` and `ampintc` (`device ampintc: fdt`, whose GICv2m frames `ampintcmsi`
//! attach below it); `agintc`, which also carries it, is not ported. Every other GENERIC
//! line waits for its driver (`smbios0 at efi?`, the devices at `virtio?` but `vio*` and
//! `vioblk*`, the devices at `pci?` but `virtio*`, `xhci*` and `azalia*`, the other host bridges, `usb*` at
//! the other host controllers, the devices at `uhub?` but `uhub*`, ...),
//! as do the other pseudo-devices (`pdevinit[]`). Each entry keeps `config(8)`'s layout:
//! attachment, driver, unit, state, locators, flags, parents (indices into `CFDATA`), the
//! start of its locator names and the first unit a starred entry may take.

use crate::arch::arm64::arm64::cpu::{CPU_CA, CPU_CD};
use crate::arch::arm64::dev::agtimer::{AGTIMER_CA, AGTIMER_CD};
use crate::arch::arm64::dev::ampintc::{AMPINTC_CA, AMPINTC_CD, AMPINTCMSI_CA, AMPINTCMSI_CD};
use crate::arch::arm64::dev::efi_machdep::{EFI_CA, EFI_CD};
use crate::arch::arm64::dev::mainbus::{MAINBUS_CA, MAINBUS_CD};
use crate::arch::arm64::dev::simplebus::{SIMPLEBUS_CA, SIMPLEBUS_CD};
use crate::dev::audio::{AUDIO_CA, AUDIO_CD};
use crate::dev::bio::bioattach;
use crate::dev::fdt::pciecam::{PCIECAM_CA, PCIECAM_CD};
use crate::dev::fdt::plrtc::{PLRTC_CA, PLRTC_CD};
use crate::dev::fdt::pluart_fdt::PLUART_FDT_CA;
use crate::dev::fdt::virtio_mmio::VIRTIO_MMIO_CA;
use crate::dev::ic::pluart::PLUART_CD;
use crate::dev::pci::azalia::{AZALIA_CA, AZALIA_CD};
use crate::dev::pci::pci::{PCI_CA, PCI_CD};
use crate::dev::pci::virtio_pci::VIRTIO_PCI_CA;
use crate::dev::pci::xhci_pci::XHCI_PCI_CA;
use crate::dev::pv::if_vio::{VIO_CA, VIO_CD};
use crate::dev::pv::vioblk::{VIOBLK_CA, VIOBLK_CD};
use crate::dev::pv::vioscsi::{VIOSCSI_CA, VIOSCSI_CD};
use crate::dev::pv::virtio::VIRTIO_CD;
use crate::dev::rd::rdattach;
use crate::dev::softraid::{SOFTRAID_CA, SOFTRAID_CD};
use crate::dev::usb::uhub::{UHUB_CA, UHUB_CD, UHUB_UHUB_CA};
use crate::dev::usb::usb::{USB_CA, USB_CD};
use crate::dev::usb::xhci::XHCI_CD;
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
use crate::scsi::cd::{CD_CA, CD_CD};
use crate::scsi::scsiconf::{SCSIBUS_CA, SCSIBUS_CD};
use crate::scsi::sd::{SD_CA, SD_CD};
use crate::sys::device::{Cfdata, FSTATE_NOTFOUND, FSTATE_STAR, Pdevinit};

/// `pv[]` for children of the `fdt` attribute: `mainbus0` (`cfdata[0]`), `ampintc*`
/// (`cfdata[1]`) and `simplebus*` (`cfdata[12]`).
const PV_FDT: &[i16] = &[0, 1, 12];

/// `pv[]` for children of `mainbus0` (`cfdata[0]`) through `mainbus` itself.
const PV_MAINBUS: &[i16] = &[0];

/// `loc[]` of `early 1`.
const LOC_EARLY_1: &[i64] = &[1];

/// `loc[]` of `early 0`, the default.
const LOC_EARLY_0: &[i64] = &[0];

/// `pv[]` for children of `virtio*` (`cfdata[3]` at fdt, `cfdata[16]` at pci).
const PV_VIRTIO: &[i16] = &[3, 16];

/// `pv[]` for children of `pciecam*` (`cfdata[14]`) through the `pcibus` attribute.
const PV_PCIECAM: &[i16] = &[14];

/// `loc[]` of an entry at `pcibus` with the default `bus = -1` (`conf/files`: `define pcibus
/// {[bus = -1]}`).
const LOC_PCIBUS_UNK: &[i64] = &[-1];

/// `pv[]` for children of `pci*` (`cfdata[15]`).
const PV_PCI: &[i16] = &[15];

/// `loc[]` of an entry at `pci` with the defaults `dev = -1, function = -1` (`conf/files`:
/// `device pci {[dev = -1], [function = -1]}`).
const LOC_PCI_UNK: &[i64] = &[-1, -1];

/// `pv[]` for children of the `usbus` attribute, carried by `xhci*` (`cfdata[20]`).
const PV_XHCI: &[i16] = &[20];

/// `pv[]` for children of `usb*` (`cfdata[21]`).
const PV_USB: &[i16] = &[21];

/// `pv[]` for children of the `uhub` attribute, carried by both `uhub*` entries
/// (`cfdata[22]`, `cfdata[23]`).
const PV_UHUB: &[i16] = &[22, 23];

/// `loc[]` of an entry at `uhub` with the defaults `port = -1, configuration = -1,
/// interface = -1, vendor = -1, product = -1, release = -1` (`dev/usb/files.usb`: `device
/// uhub {[port = -1], ...}`).
const LOC_UHUB_UNK: &[i64] = &[-1, -1, -1, -1, -1, -1];

/// `pv[]` for children of the `audio` attribute, carried by `azalia*` (`cfdata[17]`).
const PV_AZALIA: &[i16] = &[17];

/// `pv[]` for children of the `scsi` attribute, carried by `vioblk*` (`cfdata[5]`) and
/// `softraid0` (`cfdata[11]`).
/// M13: also `vioscsi*` (`cfdata[24]`).
const PV_VIOBLK: &[i16] = &[5, 11, 24];

/// `pv[]` for children of `scsibus*` (`cfdata[9]`).
const PV_SCSIBUS: &[i16] = &[9];

/// `loc[]` of an entry at `scsibus` with the defaults `target = -1, lun = -1`
/// (`scsi/files.scsi`: `device scsibus {[target = -1], [lun = -1]}`).
const LOC_SCSIBUS_UNK: &[i64] = &[-1, -1];

/// How many `cfdata[]` entries: `cpu*` comes with `MULTIPROCESSOR` (`GENERIC.MP`).
const NCFDATA: usize = if cfg!(feature = "multiprocessor") {
    27
} else {
    26
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
    // 12: simplebus* at fdt?
    Cfdata::new(
        &SIMPLEBUS_CA,
        &SIMPLEBUS_CD,
        0,
        FSTATE_STAR,
        LOC_EARLY_0,
        0,
        PV_FDT,
        0,
        0,
    ),
    // 13: ampintcmsi* at fdt? early 1
    Cfdata::new(
        &AMPINTCMSI_CA,
        &AMPINTCMSI_CD,
        0,
        FSTATE_STAR,
        LOC_EARLY_1,
        0,
        PV_FDT,
        0,
        0,
    ),
    // 14: pciecam* at fdt?
    Cfdata::new(
        &PCIECAM_CA,
        &PCIECAM_CD,
        0,
        FSTATE_STAR,
        LOC_EARLY_0,
        0,
        PV_FDT,
        0,
        0,
    ),
    // 15: pci* at pciecam?
    Cfdata::new(
        &PCI_CA,
        &PCI_CD,
        0,
        FSTATE_STAR,
        LOC_PCIBUS_UNK,
        0,
        PV_PCIECAM,
        0,
        0,
    ),
    // 16: virtio* at pci?
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
    // 17: azalia* at pci?
    Cfdata::new(
        &AZALIA_CA,
        &AZALIA_CD,
        0,
        FSTATE_STAR,
        LOC_PCI_UNK,
        0,
        PV_PCI,
        0,
        0,
    ),
    // 18: audio* at azalia?
    Cfdata::new(
        &AUDIO_CA,
        &AUDIO_CD,
        0,
        FSTATE_STAR,
        &[],
        0,
        PV_AZALIA,
        0,
        0,
    ),
    // 19: cpu0 at mainbus?
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
    // 20: xhci* at pci?
    Cfdata::new(
        &XHCI_PCI_CA,
        &XHCI_CD,
        0,
        FSTATE_STAR,
        LOC_PCI_UNK,
        0,
        PV_PCI,
        0,
        0,
    ),
    // 21: usb* at xhci?
    Cfdata::new(&USB_CA, &USB_CD, 0, FSTATE_STAR, &[], 0, PV_XHCI, 0, 0),
    // 22: uhub* at usb?
    Cfdata::new(&UHUB_CA, &UHUB_CD, 0, FSTATE_STAR, &[], 0, PV_USB, 0, 0),
    // 23: uhub* at uhub?
    Cfdata::new(
        &UHUB_UHUB_CA,
        &UHUB_CD,
        0,
        FSTATE_STAR,
        LOC_UHUB_UNK,
        0,
        PV_UHUB,
        0,
        0,
    ),
    // 24: vioscsi* at virtio?
    Cfdata::new(
        &VIOSCSI_CA,
        &VIOSCSI_CD,
        0,
        FSTATE_STAR,
        &[],
        0,
        PV_VIRTIO,
        0,
        0,
    ),
    // 25: cd* at scsibus?
    Cfdata::new(
        &CD_CA,
        &CD_CD,
        0,
        FSTATE_STAR,
        LOC_SCSIBUS_UNK,
        0,
        PV_SCSIBUS,
        0,
        0,
    ),
    // 26: cpu* at mainbus? (GENERIC.MP)
    #[cfg(feature = "multiprocessor")]
    Cfdata::new(&CPU_CA, &CPU_CD, 1, FSTATE_STAR, &[], 0, PV_MAINBUS, 0, 1),
];

/// `cfroots[]`: `mainbus0`, `softraid0`.
pub static CFROOTS: [i16; 2] = [0, 11];

/// `pdevinit[]`: the pseudo-devices of the MI `conf/GENERIC` whose attach functions are
/// ported, in `ioconf.c`'s order (`pseudo-device pf`, `pseudo-device pflog`, `pseudo-device
/// pfsync`, `pseudo-device pflow`, `pseudo-device enc`, `pseudo-device pty 16`, `pseudo-device
/// vnd 4`, `pseudo-device bpfilter`, `pseudo-device loop`, `pseudo-device wg`, `pseudo-device
/// bio 1`, `pseudo-device fuse` under feature `fuse`; all but pty and vnd with a count of 1), then `pseudo-device rd 1`, which is not in
/// GENERIC but in the RAMDISK kernels (`arch/arm64/conf/RAMDISK*`): this kernel boots its root
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
