//! The amd64 kernel's autoconfiguration tables: what `config(8)` writes into `ioconf.c` from
//! `arch/amd64/conf/GENERIC`, for the devices whose drivers are ported. Not an OpenBSD file:
//! `ioconf.c` is generated, and `config(8)` is replaced here by these hand-written tables
//! (`docs/ARCHITECTURE.md`, "Deviations"); `machine::autoconf` hands them to
//! `subr_autoconf.rs`.
//!
//! GENERIC lines present: `mainbus0 at root`, `cpu0 at mainbus?` (and GENERIC.MP's
//! `cpu* at mainbus?` with feature `multiprocessor`), `pci* at mainbus0`,
//! `virtio* at pci?`, `vio* at virtio?`, `vioblk* at virtio?`, `auich* at pci?`,
//! `audio* at auich?`, `azalia* at pci?`, `audio* at azalia?`, `scsibus* at scsi?`,
//! `sd* at scsibus?`, `softraid0 at root` and `scsibus* at softraid?` (conf/GENERIC),
//! `xhci* at pci?`, `usb* at xhci?`, `uhub* at usb?`, `uhub* at uhub?`, `umass* at uhub?`
//! and `scsibus* at scsi?` below it, `uhidev* at uhub?`, `ukbd* at uhidev?` (M12),
//! `nvme* at pci?`, `vioscsi* at virtio?`, `cd* at scsibus?`, `ahci* at pci?`, `siop* at pci?`,
//! `bios0 at mainbus0`, `acpi0 at bios0`, `acpitimer* at acpi?`, `acpihpet* at acpi?`,
//! `ioapic* at mainbus?`, `acpimadt0 at acpi?`, `acpiprt* at acpi?` and `acpipci* at
//! acpi?` (M13), `em* at pci?` (M13), `puc* at pci?` and `com* at puc?` (M13; `com*` takes the units from 4),
//! `isa0 at mainbus0`,
//! `com0 at isa? port 0x3f8 irq 4`, `com1 at isa? port 0x2f8 irq 3`, `com2 at isa? port 0x3e8
//! irq 5`, `com3 at isa? disable port 0x2e8 irq 9`; `pseudo-device pf`, `pseudo-device pflog`,
//! `pseudo-device pty 16`, `pseudo-device vnd 4`, `pseudo-device bpfilter`, `pseudo-device
//! loop`, `pseudo-device wg`, `pseudo-device pfsync`, `pseudo-device pflow`.
//! GENERIC lines left out until their drivers exist: `vmm0`, `pvbus0`, `ipmi0`
//! and `efifb0` at mainbus, and everything below them; `efi0` and `mpbios0` at bios0, and
//! every other device at `acpi?` (`acpimcfg*`, `acpicpu*`, ...); `isa0` at `pcib?`,
//! `amdpcib?` and `tcpcib?`, and every other device at `isa?` (`isadma0`, `pckbc0`, `vga0`,
//! `pcppi0`, `lpt0`, `fdc0`, `wdc*`, the sensors, ...); every other device at `pci?`
//! (`pchb*`, `ppb*`, `pcib*`, the network drivers but em and the storage drivers but nvme, ahci and siop, ...), every
//! other
//! `audio*` (at `uaudio?`, ...), `pci*` at `ppb?` and
//! `pchb?`, and every device at `virtio?` but `vio*`, `vioblk*` and `vioscsi*`; `usb*` at `ehci?`, `uhci?`
//! and `ohci?`, every device at `uhub?` but `uhub*`, `umass*` and `uhidev*`, every device
//! at `uhidev?` but `ukbd*` (`wskbd* at ukbd?` waits for wskbd, M13);
//! `mpath0 at root`; the other pseudo-devices (`pdevinit[]`). Each entry keeps `config(8)`'s
//! layout: attachment, driver, unit, state, locators, flags, parents (indices into
//! `CFDATA`), the start of its locator names and the first unit a starred entry may take.

use crate::arch::amd64::amd64::acpi_machdep::ACPI_CA;
use crate::arch::amd64::amd64::bios::{BIOS_CA, BIOS_CD};
use crate::arch::amd64::amd64::cpu::{CPU_CA, CPU_CD};
use crate::arch::amd64::amd64::ioapic::{IOAPIC_CA, IOAPIC_CD};
use crate::arch::amd64::amd64::mainbus::{MAINBUS_CA, MAINBUS_CD};
use crate::arch::amd64::pci::acpipci::{ACPIPCI_CA, ACPIPCI_CD};
use crate::dev::acpi::acpi::ACPI_CD;
use crate::dev::acpi::acpihpet::{ACPIHPET_CA, ACPIHPET_CD};
use crate::dev::acpi::acpimadt::{ACPIMADT_CA, ACPIMADT_CD};
use crate::dev::acpi::acpiprt::{ACPIPRT_CA, ACPIPRT_CD};
use crate::dev::acpi::acpitimer::{ACPITIMER_CA, ACPITIMER_CD};
use crate::dev::audio::{AUDIO_CA, AUDIO_CD};
use crate::dev::bio::bioattach;
use crate::dev::ic::ahci::AHCI_CD;
use crate::dev::ic::com::COM_CD;
use crate::dev::ic::nvme::NVME_CD;
use crate::dev::ic::siop::SIOP_CD;
use crate::dev::isa::com_isa::COM_ISA_CA;
use crate::dev::isa::isa::{ISA_CA, ISA_CD};
use crate::dev::pci::ahci_pci::AHCI_PCI_CA;
use crate::dev::pci::auich::{AUICH_CA, AUICH_CD};
use crate::dev::pci::azalia::{AZALIA_CA, AZALIA_CD};
use crate::dev::pci::if_em::{EM_CA, EM_CD};
use crate::dev::pci::nvme_pci::NVME_PCI_CA;
use crate::dev::pci::pci::{PCI_CA, PCI_CD};
use crate::dev::pci::puc::{PUC_CD, PUC_PCI_CA};
use crate::dev::pci::siop_pci::SIOP_PCI_CA;
use crate::dev::pci::virtio_pci::VIRTIO_PCI_CA;
use crate::dev::pci::xhci_pci::XHCI_PCI_CA;
use crate::dev::puc::com_puc::COM_PUC_CA;
use crate::dev::pv::if_vio::{VIO_CA, VIO_CD};
use crate::dev::pv::vioblk::{VIOBLK_CA, VIOBLK_CD};
use crate::dev::pv::vioscsi::{VIOSCSI_CA, VIOSCSI_CD};
use crate::dev::pv::virtio::VIRTIO_CD;
use crate::dev::rd::rdattach;
use crate::dev::softraid::{SOFTRAID_CA, SOFTRAID_CD};
use crate::dev::usb::uhidev::{UHIDEV_CA, UHIDEV_CD};
use crate::dev::usb::uhub::{UHUB_CA, UHUB_CD, UHUB_UHUB_CA};
use crate::dev::usb::ukbd::{UKBD_CA, UKBD_CD};
use crate::dev::usb::umass::{UMASS_CA, UMASS_CD};
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
/// `softraid0` (`cfdata[13]`), `umass*` (`cfdata[22]`), `nvme*` (`cfdata[25]`), `vioscsi*`
/// (`cfdata[26]`), `ahci*` (`cfdata[28]`, through atascsi) and `siop*` (`cfdata[29]`).
const PV_VIOBLK: &[i16] = &[5, 13, 22, 25, 26, 28, 29];

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

/// `pv[]` for children of the `usbus` attribute, carried by `xhci*` (`cfdata[14]`).
const PV_XHCI: &[i16] = &[14];

/// `pv[]` for children of `usb*` (`cfdata[15]`).
const PV_USB: &[i16] = &[15];

/// `pv[]` for children of the `uhub` attribute, carried by both `uhub*` entries
/// (`cfdata[16]`, `cfdata[17]`).
const PV_UHUB: &[i16] = &[16, 17];

/// `loc[]` of an entry at `uhub` with the defaults `port = -1, configuration = -1,
/// interface = -1, vendor = -1, product = -1, release = -1` (`dev/usb/files.usb`: `device
/// uhub {[port = -1], ...}`).
const LOC_UHUB_UNK: &[i64] = &[-1, -1, -1, -1, -1, -1];

/// `pv[]` for children of `uhidev*` (`cfdata[23]`): the `uhidbus` attribute (`files.usb`:
/// `define uhidbus {[reportid = -1]}`) is carried by `uhidev` alone here.
const PV_UHIDEV: &[i16] = &[23];

/// `loc[]` of an entry at `uhidbus` with the default `reportid = -1`.
const LOC_UHIDBUS_UNK: &[i64] = &[-1];

/// `pv[]` for children of `auich*` (`cfdata[18]`).
const PV_AUICH: &[i16] = &[18];

/// `pv[]` for children of the `audio` attribute, carried by `azalia*` (`cfdata[20]`).
const PV_AZALIA: &[i16] = &[20];

/// `pv[]` for children of `bios0` (`cfdata[30]`).
const PV_BIOS: &[i16] = &[30];

/// `pv[]` for children of `acpi0` (`cfdata[31]`).
const PV_ACPI: &[i16] = &[31];

/// `pv[]` for children of `puc*` (`cfdata[38]`).
const PV_PUC: &[i16] = &[38];

/// `loc[]` of an entry at `puc` with the default `port = -1` (`files.pci`: `device puc {[port =
/// -1]}`).
const LOC_PUC_UNK: &[i64] = &[-1];

/// `cfdata[]`: 41 entries, 42 with `MULTIPROCESSOR` (GENERIC.MP's `cpu* at mainbus?`).
const NCFDATA: usize = if cfg!(feature = "multiprocessor") {
    42
} else {
    41
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
    // 11: scsibus* at scsi? (vioblk, umass, nvme, vioscsi, ahci, siop), and at softraid? (GENERIC's `scsibus* at
    // softraid?`)
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
    // 14: xhci* at pci?
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
    // 15: usb* at xhci?
    Cfdata::new(&USB_CA, &USB_CD, 0, FSTATE_STAR, &[], 0, PV_XHCI, 0, 0),
    // 16: uhub* at usb?
    Cfdata::new(&UHUB_CA, &UHUB_CD, 0, FSTATE_STAR, &[], 0, PV_USB, 0, 0),
    // 17: uhub* at uhub?
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
    // 18: auich* at pci?
    Cfdata::new(
        &AUICH_CA,
        &AUICH_CD,
        0,
        FSTATE_STAR,
        LOC_PCI_UNK,
        0,
        PV_PCI,
        0,
        0,
    ),
    // 19: audio* at auich?
    Cfdata::new(&AUDIO_CA, &AUDIO_CD, 0, FSTATE_STAR, &[], 0, PV_AUICH, 0, 0),
    // 20: azalia* at pci?
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
    // 21: audio* at azalia?
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
    // 22: umass* at uhub?
    Cfdata::new(
        &UMASS_CA,
        &UMASS_CD,
        0,
        FSTATE_STAR,
        LOC_UHUB_UNK,
        0,
        PV_UHUB,
        0,
        0,
    ),
    // 23: uhidev* at uhub?
    Cfdata::new(
        &UHIDEV_CA,
        &UHIDEV_CD,
        0,
        FSTATE_STAR,
        LOC_UHUB_UNK,
        0,
        PV_UHUB,
        0,
        0,
    ),
    // 24: ukbd* at uhidev?
    Cfdata::new(
        &UKBD_CA,
        &UKBD_CD,
        0,
        FSTATE_STAR,
        LOC_UHIDBUS_UNK,
        0,
        PV_UHIDEV,
        0,
        0,
    ),
    // 25: nvme* at pci?
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
    // 26: vioscsi* at virtio?
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
    // 27: cd* at scsibus?
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
    // 28: ahci* at pci?
    Cfdata::new(
        &AHCI_PCI_CA,
        &AHCI_CD,
        0,
        FSTATE_STAR,
        LOC_PCI_UNK,
        0,
        PV_PCI,
        0,
        0,
    ),
    // 29: siop* at pci?
    Cfdata::new(
        &SIOP_PCI_CA,
        &SIOP_CD,
        0,
        FSTATE_STAR,
        LOC_PCI_UNK,
        0,
        PV_PCI,
        0,
        0,
    ),
    // 30: bios0 at mainbus0
    Cfdata::new(
        &BIOS_CA,
        &BIOS_CD,
        0,
        FSTATE_NOTFOUND,
        &[],
        0,
        PV_MAINBUS,
        0,
        0,
    ),
    // 31: acpi0 at bios0
    Cfdata::new(
        &ACPI_CA,
        &ACPI_CD,
        0,
        FSTATE_NOTFOUND,
        &[],
        0,
        PV_BIOS,
        0,
        0,
    ),
    // 32: acpitimer* at acpi?
    Cfdata::new(
        &ACPITIMER_CA,
        &ACPITIMER_CD,
        0,
        FSTATE_STAR,
        &[],
        0,
        PV_ACPI,
        0,
        0,
    ),
    // 33: acpihpet* at acpi?
    Cfdata::new(
        &ACPIHPET_CA,
        &ACPIHPET_CD,
        0,
        FSTATE_STAR,
        &[],
        0,
        PV_ACPI,
        0,
        0,
    ),
    // 34: ioapic* at mainbus?
    Cfdata::new(
        &IOAPIC_CA,
        &IOAPIC_CD,
        0,
        FSTATE_STAR,
        &[],
        0,
        PV_MAINBUS,
        0,
        0,
    ),
    // 35: acpimadt0 at acpi?
    Cfdata::new(
        &ACPIMADT_CA,
        &ACPIMADT_CD,
        0,
        FSTATE_NOTFOUND,
        &[],
        0,
        PV_ACPI,
        0,
        0,
    ),
    // 36: acpiprt* at acpi?
    Cfdata::new(
        &ACPIPRT_CA,
        &ACPIPRT_CD,
        0,
        FSTATE_STAR,
        &[],
        0,
        PV_ACPI,
        0,
        0,
    ),
    // 37: acpipci* at acpi?
    Cfdata::new(
        &ACPIPCI_CA,
        &ACPIPCI_CD,
        0,
        FSTATE_STAR,
        &[],
        0,
        PV_ACPI,
        0,
        0,
    ),
    // 38: puc* at pci?
    Cfdata::new(
        &PUC_PCI_CA,
        &PUC_CD,
        0,
        FSTATE_STAR,
        LOC_PCI_UNK,
        0,
        PV_PCI,
        0,
        0,
    ),
    // 39: com* at puc?: the units from 4 on (com0 to com3 are the ISA lines above)
    Cfdata::new(
        &COM_PUC_CA,
        &COM_CD,
        0,
        FSTATE_STAR,
        LOC_PUC_UNK,
        0,
        PV_PUC,
        0,
        4,
    ),
    // 40: em* at pci?
    Cfdata::new(&EM_CA, &EM_CD, 0, FSTATE_STAR, LOC_PCI_UNK, 0, PV_PCI, 0, 0),
    // 41: cpu* at mainbus? (GENERIC.MP, MULTIPROCESSOR): the application processors, unit 1
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
