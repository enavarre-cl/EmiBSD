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
/* </LICENSES> */

/* <CODE> */
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
//! `plrtc* at fdt?`, `efi0 at mainbus?`, `simplebus* at fdt?`, `ampintcmsi* at fdt? early
//! 1`, `pciecam* at fdt?`, `pci* at pciecam?`, `virtio* at pci?`, `xhci* at pci?`, `usb* at
//! xhci?`, `uhub* at usb?`, `uhub* at uhub?`, `umass* at uhub?` and `scsibus* at scsi?` below
//! it, `uhidev* at uhub?`, `ukbd* at uhidev?` (M12), `ums* at uhidev?`,
//! `wsmouse* at ums? mux 0`, `uwacom* at uhidev?` and `wsmouse* at uwacom? mux 0` (M16b), `uhid* at uhidev?`, `uaudio* at uhub?`, `audio* at uaudio?`, `ugen* at uhub?` (M16b), `cpu0 at mainbus?`
//! and, with `MULTIPROCESSOR`, `GENERIC.MP`'s `cpu* at mainbus?`;
//! `azalia* at pci?` and `audio* at azalia?` (M12); `vioscsi* at virtio?` and `cd* at
//! scsibus?`, `psci* at fdt? early 1`, `ahci* at pci?`, `nvme* at pci?`, `em* at pci?`, `simplefb* at
//! fdt?` and `wsdisplay* at simplefb?` (M13);
//! `re* at pci?`, `rgephy* at mii?`, `rlphy* at mii?` and `ukphy* at mii?` (M13);
//! `vmx* at pci?` (M13); `wskbd* at ukbd? mux 1` and `pseudo-device wsmux 2` (M13);
//! arm64 ACPI (M14): `acpi0 at mainbus?` (the `acpi_fdt` attachment), `acpimcfg* at acpi?`,
//! `acpiiort* at acpi?`, `acpipci* at acpi?`, `pci* at acpipci?` and `pluart* at acpi?`;
//! M16f: `smmu* at acpiiort?`, `smmu* at fdt?`, `plgpio* at fdt? early 1`, `gpiokeys* at
//! fdt?`, `agintc* at fdt? early 1` and `agintcmsi* at fdt? early 1`;
//! `ppb* at pci?` and `pci* at ppb?` (M16e); `ipmi* at acpi?` and `ipmi* at fdt?` (M16e;
//! QEMU's `virt` has no IPMI device; `ipmi* at iic?` waits for `ipmi_i2c.c`, SSIF, and the
//! iic(4) stack);
//! M16b: `ehci* at pci?` and `usb* at ehci?`, `uhci* at pci?` and `usb* at uhci?`, `ohci* at pci?` and `usb* at ohci?`,
//! `cdce* at uhub?`, `uftdi* at uhub?` and `ucom* at uftdi?`;
//! `pseudo-device pf`, `pseudo-device pflog`, `pseudo-device pty 16`, `pseudo-device vnd 4`,
//! `pseudo-device bpfilter`, `pseudo-device loop`, `pseudo-device wg`, `pseudo-device pfsync`,
//! `pseudo-device pflow`.
//! The `fdt` attribute (`files.arm64`: `define fdt {[early = 0]}`) is carried by `mainbus`,
//! `simplebus`, `ampintc` (`device ampintc: fdt`, whose GICv2m frames `ampintcmsi`
//! attach below it) and `agintc` (`device agintc: fdt`, whose ITS `agintcmsi` attaches
//! below it). Every other GENERIC
//! line waits for its driver (`smbios0 at efi?`, the devices at `virtio?` but `vio*`,
//! `vioblk*` and `vioscsi*`, the devices at `pci?` but `virtio*`, `xhci*`, `ehci*`, `uhci*`, `ohci*`, `azalia*`, `ahci*`, `nvme*`, `ppb*`,
//! `em*`, `re*` and `vmx*`, the PHYs at `mii?` but `rgephy*`, `rlphy*` and `ukphy*`, the other devices at `acpi?` (`acpiac*`, `acpibtn*`, `acpicpu*`, `ahci*`, `com*`, `xhci*`,
//! ...), `ahci*` at `fdt?`, `ehci*` at `acpi?` and `fdt?`, the other host
//! bridges, `usb*` at the other host controllers, the devices at `uhub?` but `uhub*`,
//! `umass*`, `uhidev*`, `uaudio*`, `cdce*`, `uftdi*` and `ugen*`, the devices at `uhidev?` but `ukbd*`, `ums*`, `uwacom*` and `uhid*`, every `wskbd*`
//! but the one at `ukbd?`, every `wsmouse*` but the ones at `ums?` and `uwacom?`, every `ucom*` but the one at `uftdi?`, ...),
//! as do the other pseudo-devices (`pdevinit[]`). Each entry keeps `config(8)`'s layout:
//! attachment, driver, unit, state, locators, flags, parents (indices into `CFDATA`), the
//! start of its locator names and the first unit a starred entry may take.

use libkern::StaticCell;

use crate::arch::arm64::arm64::acpi_machdep::ACPI_FDT_CA;
use crate::arch::arm64::arm64::cpu::{CPU_CA, CPU_CD};
use crate::arch::arm64::dev::acpiiort::{ACPIIORT_CA, ACPIIORT_CD};
use crate::arch::arm64::dev::acpipci::{ACPIPCI_CA, ACPIPCI_CD};
use crate::arch::arm64::dev::agintc::{AGINTC_CA, AGINTC_CD, AGINTCMSI_CA, AGINTCMSI_CD};
use crate::arch::arm64::dev::agtimer::{AGTIMER_CA, AGTIMER_CD};
use crate::arch::arm64::dev::ampintc::{AMPINTC_CA, AMPINTC_CD, AMPINTCMSI_CA, AMPINTCMSI_CD};
use crate::arch::arm64::dev::efi_machdep::{EFI_CA, EFI_CD};
use crate::arch::arm64::dev::mainbus::{MAINBUS_CA, MAINBUS_CD};
use crate::arch::arm64::dev::simplebus::{SIMPLEBUS_CA, SIMPLEBUS_CD};
use crate::arch::arm64::dev::smmu::SMMU_CD;
use crate::arch::arm64::dev::smmu_acpi::SMMU_ACPI_CA;
use crate::arch::arm64::dev::smmu_fdt::SMMU_FDT_CA;
use crate::dev::acpi::acpi::ACPI_CD;
use crate::dev::acpi::acpimcfg::{ACPIMCFG_CA, ACPIMCFG_CD};
use crate::dev::acpi::ipmi_acpi::IPMI_ACPI_CA;
use crate::dev::acpi::pluart_acpi::PLUART_ACPI_CA;
use crate::dev::audio::{AUDIO_CA, AUDIO_CD};
use crate::dev::bio::bioattach;
use crate::dev::fdt::gpiokeys::{GPIOKEYS_CA, GPIOKEYS_CD};
use crate::dev::fdt::ipmi_fdt::IPMI_FDT_CA;
use crate::dev::fdt::pciecam::{PCIECAM_CA, PCIECAM_CD};
use crate::dev::fdt::plgpio::{PLGPIO_CA, PLGPIO_CD};
use crate::dev::fdt::plrtc::{PLRTC_CA, PLRTC_CD};
use crate::dev::fdt::pluart_fdt::PLUART_FDT_CA;
use crate::dev::fdt::psci::{PSCI_CA, PSCI_CD};
use crate::dev::fdt::simplefb::{SIMPLEFB_CA, SIMPLEFB_CD};
use crate::dev::fdt::virtio_mmio::VIRTIO_MMIO_CA;
use crate::dev::ic::ahci::AHCI_CD;
use crate::dev::ic::nvme::NVME_CD;
use crate::dev::ic::pluart::PLUART_CD;
use crate::dev::ic::re::RE_CD;
use crate::dev::ipmi::IPMI_CD;
use crate::dev::mii::rgephy::{RGEPHY_CA, RGEPHY_CD};
use crate::dev::mii::rlphy::{RLPHY_CA, RLPHY_CD};
use crate::dev::mii::ukphy::{UKPHY_CA, UKPHY_CD};
use crate::dev::pci::ahci_pci::AHCI_PCI_CA;
use crate::dev::pci::azalia::{AZALIA_CA, AZALIA_CD};
use crate::dev::pci::ehci_pci::EHCI_PCI_CA;
use crate::dev::pci::if_em::{EM_CA, EM_CD};
use crate::dev::pci::if_re_pci::RE_PCI_CA;
use crate::dev::pci::if_vmx::{VMX_CA, VMX_CD};
use crate::dev::pci::nvme_pci::NVME_PCI_CA;
use crate::dev::pci::ohci_pci::OHCI_PCI_CA;
use crate::dev::pci::pci::{PCI_CA, PCI_CD};
use crate::dev::pci::ppb::{PPB_CA, PPB_CD};
use crate::dev::pci::uhci_pci::UHCI_PCI_CA;
use crate::dev::pci::virtio_pci::VIRTIO_PCI_CA;
use crate::dev::pci::xhci_pci::XHCI_PCI_CA;
use crate::dev::pv::if_vio::{VIO_CA, VIO_CD};
use crate::dev::pv::vioblk::{VIOBLK_CA, VIOBLK_CD};
use crate::dev::pv::vioscsi::{VIOSCSI_CA, VIOSCSI_CD};
use crate::dev::pv::virtio::VIRTIO_CD;
use crate::dev::rd::rdattach;
use crate::dev::softraid::{SOFTRAID_CA, SOFTRAID_CD};
use crate::dev::usb::ehci::EHCI_CD;
use crate::dev::usb::if_cdce::{CDCE_CA, CDCE_CD};
use crate::dev::usb::ohci::OHCI_CD;
use crate::dev::usb::uaudio::{UAUDIO_CA, UAUDIO_CD};
use crate::dev::usb::ucom::{UCOM_CA, UCOM_CD};
use crate::dev::usb::uftdi::{UFTDI_CA, UFTDI_CD};
use crate::dev::usb::ugen::{UGEN_CA, UGEN_CD};
use crate::dev::usb::uhci::UHCI_CD;
use crate::dev::usb::uhid::{UHID_CA, UHID_CD};
use crate::dev::usb::uhidev::{UHIDEV_CA, UHIDEV_CD};
use crate::dev::usb::uhub::{UHUB_CA, UHUB_CD, UHUB_UHUB_CA};
use crate::dev::usb::ukbd::{UKBD_CA, UKBD_CD};
use crate::dev::usb::umass::{UMASS_CA, UMASS_CD};
use crate::dev::usb::ums::{UMS_CA, UMS_CD};
use crate::dev::usb::usb::{USB_CA, USB_CD};
use crate::dev::usb::uwacom::{UWACOM_CA, UWACOM_CD};
use crate::dev::usb::xhci::XHCI_CD;
use crate::dev::vnd::{NVND, vndattach};
use crate::dev::wscons::wsdisplay::{WSDISPLAY_CA, WSDISPLAY_CD};
use crate::dev::wscons::wskbd::{WSKBD_CA, WSKBD_CD};
use crate::dev::wscons::wsmouse::{WSMOUSE_CA, WSMOUSE_CD};
use crate::dev::wscons::wsmux::wsmuxattach;
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
/// (`cfdata[1]`), `simplebus*` (`cfdata[12]`) and `agintc*` (`cfdata[51]`).
const PV_FDT: &[i16] = &[0, 1, 12, 51];

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

/// `pv[]` for children of `pci*` (`cfdata[15]` at pciecam, `cfdata[46]` at acpipci,
/// `cfdata[54]` at ppb).
const PV_PCI: &[i16] = &[15, 46, 54];

/// `pv[]` for children of `ppb*` (`cfdata[53]`) through the `pcibus` attribute.
const PV_PPB: &[i16] = &[53];

/// `pv[]` for children of `acpi0` (`cfdata[41]`).
const PV_ACPI: &[i16] = &[41];

/// `pv[]` for children of `acpipci*` (`cfdata[44]`) through the `pcibus` attribute.
const PV_ACPIPCI: &[i16] = &[44];

/// `pv[]` for children of `acpiiort*` (`cfdata[43]`).
const PV_ACPIIORT: &[i16] = &[43];

/// `loc[]` of an entry at `pci` with the defaults `dev = -1, function = -1` (`conf/files`:
/// `device pci {[dev = -1], [function = -1]}`).
const LOC_PCI_UNK: &[i64] = &[-1, -1];

/// `pv[]` for children of the `usbus` attribute, carried by `xhci*` (`cfdata[20]`), `ehci*`
/// (`cfdata[64]`), `uhci*` (`cfdata[65]`) and `ohci*` (`cfdata[66]`): `usb* at xhci?`, `usb* at ehci?`, `usb* at
/// uhci?` and `usb* at ohci?` are one entry, as config(8) merges them.
const PV_USBUS: &[i16] = &[20, 64, 65, 66];

/// `pv[]` for children of `usb*` (`cfdata[21]`).
const PV_USB: &[i16] = &[21];

/// `pv[]` for children of the `uhub` attribute, carried by both `uhub*` entries
/// (`cfdata[22]`, `cfdata[23]`).
const PV_UHUB: &[i16] = &[22, 23];

/// `loc[]` of an entry at `uhub` with the defaults `port = -1, configuration = -1,
/// interface = -1, vendor = -1, product = -1, release = -1` (`dev/usb/files.usb`: `device
/// uhub {[port = -1], ...}`).
const LOC_UHUB_UNK: &[i64] = &[-1, -1, -1, -1, -1, -1];

/// `pv[]` for children of `uhidev*` (`cfdata[25]`): the `uhidbus` attribute (`files.usb`:
/// `define uhidbus {[reportid = -1]}`) is carried by `uhidev` alone here.
const PV_UHIDEV: &[i16] = &[25];

/// `loc[]` of an entry at `uhidbus` with the default `reportid = -1`.
const LOC_UHIDBUS_UNK: &[i64] = &[-1];

/// `pv[]` for children of the `audio` attribute, carried by `azalia*` (`cfdata[17]`) and
/// `uaudio*` (`cfdata[62]`): `config(8)` merges `audio* at azalia?` and `audio* at
/// uaudio?` into one entry.
const PV_AZALIA: &[i16] = &[17, 62];

/// `pv[]` for children of the `scsi` attribute, carried by `vioblk*` (`cfdata[5]`),
/// `softraid0` (`cfdata[11]`), `umass*` (`cfdata[24]`), `vioscsi*` (`cfdata[27]`), `ahci*`
/// (`cfdata[30]`, through atascsi) and `nvme*` (`cfdata[31]`).
const PV_VIOBLK: &[i16] = &[5, 11, 24, 27, 30, 31];

/// `pv[]` for children of `scsibus*` (`cfdata[9]`).
const PV_SCSIBUS: &[i16] = &[9];

/// `loc[]` of an entry at `scsibus` with the defaults `target = -1, lun = -1`
/// (`scsi/files.scsi`: `device scsibus {[target = -1], [lun = -1]}`).
const LOC_SCSIBUS_UNK: &[i64] = &[-1, -1];

/// `pv[]` for children of the `mii` attribute, carried by `re*` (`cfdata[34]`).
const PV_MII: &[i16] = &[34];

/// `loc[]` of an entry at `mii` with the default `phy = -1` (`conf/files`: `define mii {[phy =
/// -1]}`).
const LOC_MII_UNK: &[i64] = &[-1];

/// `pv[]` for children of `simplefb*` (`cfdata[33]`).
const PV_SIMPLEFB: &[i16] = &[33];

/// `loc[]` of an entry at `wsemuldisplaydev` with the defaults `console = -1, primary = -1,
/// mux = 1` (`conf/files`: `define wsemuldisplaydev {[console = -1], [primary = -1], [mux =
/// 1]}`).
const LOC_WSEMULDISPLAYDEV_UNK: &[i64] = &[-1, -1, 1];

/// `pv[]` for children of `ukbd*` (`cfdata[26]`): the `wskbddev` attribute.
const PV_UKBD: &[i16] = &[26];

/// `loc[]` of `wskbd* at ukbd? mux 1`: `console = -1` (`conf/files`: `define wskbddev
/// {[console = -1], [mux = 1]}`), `mux 1`.
const LOC_WSKBDDEV_MUX1: &[i64] = &[-1, 1];

/// `pv[]` for children of `ums*` (`cfdata[57]`): the `wsmousedev` attribute.
const PV_UMS: &[i16] = &[57];

/// `pv[]` for children of `uwacom*` (`cfdata[59]`): the `wsmousedev` attribute.
const PV_UWACOM: &[i16] = &[59];

/// `loc[]` of `wsmouse* at ums? mux 0` and `wsmouse* at uwacom? mux 0`: `mux = 0` (`conf/files`:
/// `define wsmousedev {[mux = 0]}`).
const LOC_WSMOUSEDEV_MUX0: &[i64] = &[0];

// `cf_locnames`: where an entry's run of locator names starts in `LOCNAMP`; an entry without
// locators has 0, the empty run at `LOCNAMP[0]`.
/// `cf_locnames` of an entry at `fdt`: `early`.
const LN_FDT: i32 = 1;
/// `cf_locnames` of an entry at `scsibus`: `target`, `lun`.
const LN_SCSIBUS: i32 = 3;
/// `cf_locnames` of an entry at `pcibus`: `bus`.
const LN_PCIBUS: i32 = 6;
/// `cf_locnames` of an entry at `pci`: `dev`, `function`.
const LN_PCI: i32 = 8;
/// `cf_locnames` of an entry at `uhub`: `port`, `configuration`, `interface`, `vendor`, `product`, `release`.
const LN_UHUB: i32 = 11;
/// `cf_locnames` of an entry at `uhidbus`: `reportid`.
const LN_UHIDBUS: i32 = 18;
/// `cf_locnames` of an entry at `mii`: `phy`.
const LN_MII: i32 = 20;
/// `cf_locnames` of an entry at `wsemuldisplaydev`: `console`, `primary`, `mux`.
const LN_WSEMULDISPLAYDEV: i32 = 22;
/// `cf_locnames` of an entry at `wskbddev`: `console`, `mux`.
const LN_WSKBDDEV: i32 = 26;
/// `cf_locnames` of an entry at `wsmousedev`: `mux`.
const LN_WSMOUSEDEV: i32 = 29;
/// `cf_locnames` of an entry at `ucombus`: `portno`.
const LN_UCOMBUS: i32 = 31;

/// `pv[]` for children of `uftdi*` (`cfdata[68]`): the `ucombus` attribute (`files.usb`:
/// `define ucombus {[portno = -1]}`) is carried by `uftdi` alone here.
const PV_UFTDI: &[i16] = &[68];

/// `loc[]` of an entry at `ucombus` with the default `portno = -1`.
const LOC_UCOMBUS_UNK: &[i64] = &[-1];

/// `{0}`: the free slots `config(8)` leaves at the end of `cfdata[]` for UKC's `add`.
const NFREE: usize = 8;

/// How many `cfdata[]` entries: `cpu*` comes with `MULTIPROCESSOR` (`GENERIC.MP`).
const NCFDATA: usize = if cfg!(feature = "multiprocessor") {
    71
} else {
    70
};

/// `cfdata[]`, edited by UKC (`boot -c`) before autoconfiguration reads it
/// (`machine::autoconf::ioconf_mut`).
pub static CFDATA: StaticCell<[Cfdata; NCFDATA + NFREE]> = StaticCell::new([
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
        LN_FDT,
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
        LN_FDT,
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
        LN_FDT,
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
        LN_FDT,
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
        LN_FDT,
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
    // 9: scsibus* at scsi? (vioblk, umass, vioscsi, ahci, nvme), and at softraid? (GENERIC's `scsibus* at
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
    // 10: sd* at scsibus?
    Cfdata::new(
        &SD_CA,
        &SD_CD,
        0,
        FSTATE_STAR,
        LOC_SCSIBUS_UNK,
        0,
        PV_SCSIBUS,
        LN_SCSIBUS,
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
        LN_FDT,
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
        LN_FDT,
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
        LN_FDT,
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
        LN_PCIBUS,
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
        LN_PCI,
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
        LN_PCI,
        0,
    ),
    // 18: audio* at azalia?, audio* at uaudio?
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
        LN_PCI,
        0,
    ),
    // 21: usb* at xhci?, usb* at ehci?, usb* at uhci?, usb* at ohci?
    Cfdata::new(&USB_CA, &USB_CD, 0, FSTATE_STAR, &[], 0, PV_USBUS, 0, 0),
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
        LN_UHUB,
        0,
    ),
    // 24: umass* at uhub?
    Cfdata::new(
        &UMASS_CA,
        &UMASS_CD,
        0,
        FSTATE_STAR,
        LOC_UHUB_UNK,
        0,
        PV_UHUB,
        LN_UHUB,
        0,
    ),
    // 25: uhidev* at uhub?
    Cfdata::new(
        &UHIDEV_CA,
        &UHIDEV_CD,
        0,
        FSTATE_STAR,
        LOC_UHUB_UNK,
        0,
        PV_UHUB,
        LN_UHUB,
        0,
    ),
    // 26: ukbd* at uhidev?
    Cfdata::new(
        &UKBD_CA,
        &UKBD_CD,
        0,
        FSTATE_STAR,
        LOC_UHIDBUS_UNK,
        0,
        PV_UHIDEV,
        LN_UHIDBUS,
        0,
    ),
    // 27: vioscsi* at virtio?
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
    // 28: cd* at scsibus?
    Cfdata::new(
        &CD_CA,
        &CD_CD,
        0,
        FSTATE_STAR,
        LOC_SCSIBUS_UNK,
        0,
        PV_SCSIBUS,
        LN_SCSIBUS,
        0,
    ),
    // 29: psci* at fdt? early 1
    Cfdata::new(
        &PSCI_CA,
        &PSCI_CD,
        0,
        FSTATE_STAR,
        LOC_EARLY_1,
        0,
        PV_FDT,
        LN_FDT,
        0,
    ),
    // 30: ahci* at pci? flags 0x0000
    Cfdata::new(
        &AHCI_PCI_CA,
        &AHCI_CD,
        0,
        FSTATE_STAR,
        LOC_PCI_UNK,
        0,
        PV_PCI,
        LN_PCI,
        0,
    ),
    // 31: nvme* at pci?
    Cfdata::new(
        &NVME_PCI_CA,
        &NVME_CD,
        0,
        FSTATE_STAR,
        LOC_PCI_UNK,
        0,
        PV_PCI,
        LN_PCI,
        0,
    ),
    // 32: em* at pci?
    Cfdata::new(
        &EM_CA,
        &EM_CD,
        0,
        FSTATE_STAR,
        LOC_PCI_UNK,
        0,
        PV_PCI,
        LN_PCI,
        0,
    ),
    // 33: simplefb* at fdt?
    Cfdata::new(
        &SIMPLEFB_CA,
        &SIMPLEFB_CD,
        0,
        FSTATE_STAR,
        LOC_EARLY_0,
        0,
        PV_FDT,
        LN_FDT,
        0,
    ),
    // 34: re* at pci?
    Cfdata::new(
        &RE_PCI_CA,
        &RE_CD,
        0,
        FSTATE_STAR,
        LOC_PCI_UNK,
        0,
        PV_PCI,
        LN_PCI,
        0,
    ),
    // 35: rgephy* at mii?
    Cfdata::new(
        &RGEPHY_CA,
        &RGEPHY_CD,
        0,
        FSTATE_STAR,
        LOC_MII_UNK,
        0,
        PV_MII,
        LN_MII,
        0,
    ),
    // 36: rlphy* at mii?
    Cfdata::new(
        &RLPHY_CA,
        &RLPHY_CD,
        0,
        FSTATE_STAR,
        LOC_MII_UNK,
        0,
        PV_MII,
        LN_MII,
        0,
    ),
    // 37: ukphy* at mii?
    Cfdata::new(
        &UKPHY_CA,
        &UKPHY_CD,
        0,
        FSTATE_STAR,
        LOC_MII_UNK,
        0,
        PV_MII,
        LN_MII,
        0,
    ),
    // 38: wsdisplay* at simplefb?
    Cfdata::new(
        &WSDISPLAY_CA,
        &WSDISPLAY_CD,
        0,
        FSTATE_STAR,
        LOC_WSEMULDISPLAYDEV_UNK,
        0,
        PV_SIMPLEFB,
        LN_WSEMULDISPLAYDEV,
        0,
    ),
    // 39: vmx* at pci?
    Cfdata::new(
        &VMX_CA,
        &VMX_CD,
        0,
        FSTATE_STAR,
        LOC_PCI_UNK,
        0,
        PV_PCI,
        LN_PCI,
        0,
    ),
    // 40: wskbd* at ukbd? mux 1
    Cfdata::new(
        &WSKBD_CA,
        &WSKBD_CD,
        0,
        FSTATE_STAR,
        LOC_WSKBDDEV_MUX1,
        0,
        PV_UKBD,
        LN_WSKBDDEV,
        0,
    ),
    // 41: acpi0 at mainbus? (attach acpi at fdt with acpi_fdt)
    Cfdata::new(
        &ACPI_FDT_CA,
        &ACPI_CD,
        0,
        FSTATE_NOTFOUND,
        LOC_EARLY_0,
        0,
        PV_MAINBUS,
        LN_FDT,
        0,
    ),
    // 42: acpimcfg* at acpi?
    Cfdata::new(
        &ACPIMCFG_CA,
        &ACPIMCFG_CD,
        0,
        FSTATE_STAR,
        &[],
        0,
        PV_ACPI,
        0,
        0,
    ),
    // 43: acpiiort* at acpi?
    Cfdata::new(
        &ACPIIORT_CA,
        &ACPIIORT_CD,
        0,
        FSTATE_STAR,
        &[],
        0,
        PV_ACPI,
        0,
        0,
    ),
    // 44: acpipci* at acpi?
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
    // 45: pluart* at acpi?
    Cfdata::new(
        &PLUART_ACPI_CA,
        &PLUART_CD,
        0,
        FSTATE_STAR,
        &[],
        0,
        PV_ACPI,
        0,
        0,
    ),
    // 46: pci* at acpipci?
    Cfdata::new(
        &PCI_CA,
        &PCI_CD,
        0,
        FSTATE_STAR,
        LOC_PCIBUS_UNK,
        0,
        PV_ACPIPCI,
        LN_PCIBUS,
        0,
    ),
    // 47: smmu* at acpiiort?
    Cfdata::new(
        &SMMU_ACPI_CA,
        &SMMU_CD,
        0,
        FSTATE_STAR,
        &[],
        0,
        PV_ACPIIORT,
        0,
        0,
    ),
    // 48: smmu* at fdt?
    Cfdata::new(
        &SMMU_FDT_CA,
        &SMMU_CD,
        0,
        FSTATE_STAR,
        LOC_EARLY_0,
        0,
        PV_FDT,
        LN_FDT,
        0,
    ),
    // 49: plgpio* at fdt? early 1
    Cfdata::new(
        &PLGPIO_CA,
        &PLGPIO_CD,
        0,
        FSTATE_STAR,
        LOC_EARLY_1,
        0,
        PV_FDT,
        LN_FDT,
        0,
    ),
    // 50: gpiokeys* at fdt?
    Cfdata::new(
        &GPIOKEYS_CA,
        &GPIOKEYS_CD,
        0,
        FSTATE_STAR,
        LOC_EARLY_0,
        0,
        PV_FDT,
        LN_FDT,
        0,
    ),
    // 51: agintc* at fdt? early 1
    Cfdata::new(
        &AGINTC_CA,
        &AGINTC_CD,
        0,
        FSTATE_STAR,
        LOC_EARLY_1,
        0,
        PV_FDT,
        LN_FDT,
        0,
    ),
    // 52: agintcmsi* at fdt? early 1
    Cfdata::new(
        &AGINTCMSI_CA,
        &AGINTCMSI_CD,
        0,
        FSTATE_STAR,
        LOC_EARLY_1,
        0,
        PV_FDT,
        LN_FDT,
        0,
    ),
    // 53: ppb* at pci?
    Cfdata::new(
        &PPB_CA,
        &PPB_CD,
        0,
        FSTATE_STAR,
        LOC_PCI_UNK,
        0,
        PV_PCI,
        LN_PCI,
        0,
    ),
    // 54: pci* at ppb?
    Cfdata::new(
        &PCI_CA,
        &PCI_CD,
        0,
        FSTATE_STAR,
        LOC_PCIBUS_UNK,
        0,
        PV_PPB,
        LN_PCIBUS,
        0,
    ),
    // 55: ipmi* at acpi?
    Cfdata::new(
        &IPMI_ACPI_CA,
        &IPMI_CD,
        0,
        FSTATE_STAR,
        &[],
        0,
        PV_ACPI,
        0,
        0,
    ),
    // 56: ipmi* at fdt?
    Cfdata::new(
        &IPMI_FDT_CA,
        &IPMI_CD,
        0,
        FSTATE_STAR,
        LOC_EARLY_0,
        0,
        PV_FDT,
        LN_FDT,
        0,
    ),
    // 57: ums* at uhidev?
    Cfdata::new(
        &UMS_CA,
        &UMS_CD,
        0,
        FSTATE_STAR,
        LOC_UHIDBUS_UNK,
        0,
        PV_UHIDEV,
        LN_UHIDBUS,
        0,
    ),
    // 58: wsmouse* at ums? mux 0
    Cfdata::new(
        &WSMOUSE_CA,
        &WSMOUSE_CD,
        0,
        FSTATE_STAR,
        LOC_WSMOUSEDEV_MUX0,
        0,
        PV_UMS,
        LN_WSMOUSEDEV,
        0,
    ),
    // 59: uwacom* at uhidev?
    Cfdata::new(
        &UWACOM_CA,
        &UWACOM_CD,
        0,
        FSTATE_STAR,
        LOC_UHIDBUS_UNK,
        0,
        PV_UHIDEV,
        LN_UHIDBUS,
        0,
    ),
    // 60: wsmouse* at uwacom? mux 0
    Cfdata::new(
        &WSMOUSE_CA,
        &WSMOUSE_CD,
        0,
        FSTATE_STAR,
        LOC_WSMOUSEDEV_MUX0,
        0,
        PV_UWACOM,
        LN_WSMOUSEDEV,
        0,
    ),
    // 61: uhid* at uhidev?
    Cfdata::new(
        &UHID_CA,
        &UHID_CD,
        0,
        FSTATE_STAR,
        LOC_UHIDBUS_UNK,
        0,
        PV_UHIDEV,
        LN_UHIDBUS,
        0,
    ),
    // 62: uaudio* at uhub?
    Cfdata::new(
        &UAUDIO_CA,
        &UAUDIO_CD,
        0,
        FSTATE_STAR,
        LOC_UHUB_UNK,
        0,
        PV_UHUB,
        LN_UHUB,
        0,
    ),
    // 63: ugen* at uhub?
    Cfdata::new(
        &UGEN_CA,
        &UGEN_CD,
        0,
        FSTATE_STAR,
        LOC_UHUB_UNK,
        0,
        PV_UHUB,
        LN_UHUB,
        0,
    ),
    // 64: ehci* at pci?
    Cfdata::new(
        &EHCI_PCI_CA,
        &EHCI_CD,
        0,
        FSTATE_STAR,
        LOC_PCI_UNK,
        0,
        PV_PCI,
        LN_PCI,
        0,
    ),
    // 65: uhci* at pci?
    Cfdata::new(
        &UHCI_PCI_CA,
        &UHCI_CD,
        0,
        FSTATE_STAR,
        LOC_PCI_UNK,
        0,
        PV_PCI,
        LN_PCI,
        0,
    ),
    // 66: ohci* at pci?
    Cfdata::new(
        &OHCI_PCI_CA,
        &OHCI_CD,
        0,
        FSTATE_STAR,
        LOC_PCI_UNK,
        0,
        PV_PCI,
        LN_PCI,
        0,
    ),
    // 67: cdce* at uhub?
    Cfdata::new(
        &CDCE_CA,
        &CDCE_CD,
        0,
        FSTATE_STAR,
        LOC_UHUB_UNK,
        0,
        PV_UHUB,
        LN_UHUB,
        0,
    ),
    // 68: uftdi* at uhub?
    Cfdata::new(
        &UFTDI_CA,
        &UFTDI_CD,
        0,
        FSTATE_STAR,
        LOC_UHUB_UNK,
        0,
        PV_UHUB,
        LN_UHUB,
        0,
    ),
    // 69: ucom* at uftdi?
    Cfdata::new(
        &UCOM_CA,
        &UCOM_CD,
        0,
        FSTATE_STAR,
        LOC_UCOMBUS_UNK,
        0,
        PV_UFTDI,
        LN_UCOMBUS,
        0,
    ),
    // 70: cpu* at mainbus? (GENERIC.MP)
    #[cfg(feature = "multiprocessor")]
    Cfdata::new(&CPU_CA, &CPU_CD, 1, FSTATE_STAR, &[], 0, PV_MAINBUS, 0, 1),
    // The free slots.
    Cfdata::free(),
    Cfdata::free(),
    Cfdata::free(),
    Cfdata::free(),
    Cfdata::free(),
    Cfdata::free(),
    Cfdata::free(),
    Cfdata::free(),
]);

/// `cfroots[]`: `mainbus0`, `softraid0`.
pub static CFROOTS: StaticCell<[i16; 2]> = StaticCell::new([0, 11]);

/// The number of `pdevinit[]` entries.
const NPDEVINIT: usize = 13 + cfg!(feature = "fuse") as usize;

/// `pdevinit[]`: the pseudo-devices of the MI `conf/GENERIC` whose attach functions are
/// ported, in `ioconf.c`'s order (`pseudo-device pf`, `pseudo-device pflog`, `pseudo-device
/// pfsync`, `pseudo-device pflow`, `pseudo-device enc`, `pseudo-device pty 16`, `pseudo-device
/// vnd 4`, `pseudo-device bpfilter`, `pseudo-device loop`, `pseudo-device wg`, `pseudo-device
/// bio 1`, `pseudo-device fuse` under feature `fuse`; all but pty and vnd with a count of 1), then
/// the machine GENERIC's `pseudo-device wsmux 2` (M13), then `pseudo-device rd 1`, which is not in
/// GENERIC but in the RAMDISK kernels (`arch/arm64/conf/RAMDISK*`): this kernel boots its root
/// from rd0a (M8).
pub static PDEVINIT: StaticCell<[Pdevinit; NPDEVINIT]> = StaticCell::new([
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
        pdev_attach: wsmuxattach,
        pdev_count: 2,
    },
    Pdevinit {
        pdev_attach: rdattach,
        pdev_count: 1,
    },
]);

/// `pdevnames[]`: the pseudo-devices' names, in `PDEVINIT`'s order.
pub static PDEVNAMES: [&[u8]; NPDEVINIT] = [
    b"pf",
    b"pflog",
    b"pfsync",
    b"pflow",
    b"enc",
    b"pty",
    b"vnd",
    b"bpfilter",
    b"loop",
    b"wg",
    b"bio",
    #[cfg(feature = "fuse")]
    b"fuse",
    b"wsmux",
    b"rd",
];

/// `locnames[]`: every locator name of the entries above, once.
pub static LOCNAMES: [&[u8]; 18] = [
    b"early",
    b"target",
    b"lun",
    b"bus",
    b"dev",
    b"function",
    b"port",
    b"configuration",
    b"interface",
    b"vendor",
    b"product",
    b"release",
    b"reportid",
    b"phy",
    b"console",
    b"primary",
    b"mux",
    b"portno",
];

/// `locnamp[]`: one run of indices into `LOCNAMES` per locator attribute, each ended by `-1`
/// (`config(8)` writes one per parent device; `mkioconf.c`'s XXX asks for this compression).
pub static LOCNAMP: [i16; 33] = [
    -1, 0, -1, 1, 2, -1, 3, -1, 4, 5, -1, 6, 7, 8, 9, 10, 11, -1, 12, -1, 13, -1, 14, 15, 16, -1,
    14, 16, -1, 16, -1, 17, -1,
];
/* </CODE> */
