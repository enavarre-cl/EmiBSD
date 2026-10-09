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
//! `ums* at uhidev?`, `wsmouse* at ums? mux 0`, `uwacom* at uhidev?` and `wsmouse* at uwacom?
//! mux 0` (M16b), `uhid* at uhidev?`, `uaudio* at uhub?` and `audio* at uaudio?` (M16b) and `ugen* at uhub?`
//! (M16b, the last of `uhub?`'s devices:
//! `ugen` is the generic fallback), `nvme* at pci?`, `vioscsi* at virtio?`, `cd* at scsibus?`, `ahci* at pci?`, `siop* at pci?`,
//! `bios0 at mainbus0`, `acpi0 at bios0`, `acpitimer* at acpi?`, `acpihpet* at acpi?`,
//! `ioapic* at mainbus?`, `acpimadt0 at acpi?`, `acpiprt* at acpi?` and `acpipci* at
//! acpi?` (M13), `puc* at pci?` and `com* at puc?` (M13; `com*` takes the units from 4),
//! `em* at pci?` (M13), `efifb0 at mainbus?` and `wsdisplay0 at efifb?` (M13),
//! `re* at pci?`, `rlphy* at mii?`, `rgephy* at mii?` and `ukphy* at mii?` (M13),
//! `vmx* at pci?` (M13), `vga0 at isa?`, `vga* at pci?` and `wsdisplay0 at vga? console 1` (M13),
//! `wskbd* at ukbd? mux 1` and `pseudo-device wsmux 2` (M13), `acpimcfg* at acpi?` (M14),
//! `ppb* at pci?` and `pci* at ppb?` (M16e),
//! `acpidmar0 at acpi? disable` (M16e),
//! `piixpm* at pci?`, `iic* at piixpm?`, `ichiic* at pci?` and `iic* at ichiic?` (M16e),
//! `ipmi0 at acpi? disable` and `ipmi0 at mainbus? disable` (M16e; `boot -c`'s `enable ipmi`
//! turns them on),
//! `tpm* at acpi?` (M16e), `acpicpu* at acpi?` (M16e),
//! `ehci* at pci?` and `usb* at ehci?` (M16b), `uhci* at pci?` and `usb* at uhci?` (M16b),
//! `ohci* at pci?` and `usb* at ohci?` (M16b),
//! `cdce* at uhub?`, `uftdi* at uhub?` and `ucom* at uftdi?` (M16b),
//! `pcn* at pci?`, `ne* at pci?`, `fxp* at pci?`, `inphy* at mii?`, `dc* at pci?`,
//! `lxtphy* at mii?` and `dcphy* at mii?` (M16c),
//! `eap* at pci?`, `audio* at eap?` and `midi* at eap?` (M16d), `lpt0 at isa? port 0x378
//! irq 7` (M16d; `#lpt1` and `#lpt2` are commented out in GENERIC),
//! `isa0 at mainbus0`,
//! `com0 at isa? port 0x3f8 irq 4`, `com1 at isa? port 0x2f8 irq 3`, `com2 at isa? port 0x3e8
//! irq 5`, `com3 at isa? disable port 0x2e8 irq 9`; `pseudo-device pf`, `pseudo-device pflog`,
//! `pseudo-device pty 16`, `pseudo-device vnd 4`, `pseudo-device bpfilter`, `pseudo-device
//! loop`, `pseudo-device wg`, `pseudo-device pfsync`, `pseudo-device pflow`.
//! GENERIC lines left out until their drivers exist: `vmm0` and `pvbus0`
//! at mainbus, and everything below them; `efi0` and `mpbios0` at bios0, and
//! every other device at `acpi?` (`acpiec*`, `acpitz*`, ...); every device at `iic?` (`spdmem*`,
//! `lm*`, ... are not ported: the scan prints what it finds as not configured), the other
//! `iic*` parents (`viapm?`, `amdiic?`, ...); `isa0` at `pcib?`,
//! `amdpcib?` and `tcpcib?`, and every other device at `isa?` (`isadma0`, `pckbc0`,
//! `pcppi0`, `fdc0`, `wdc*`, the sensors, ...); every other device at `pci?`
//! (`pchb*`, `pcib*`, the network drivers but em, re, vmx, pcn, ne, fxp and dc (`rl* at pci?` among them: QEMU's rtl8139 is
//! an 8139C+, which re(4) takes) and the storage drivers but nvme, ahci and siop, ...), every other
//! device at `mii?` (the other PHY drivers), every
//! other
//! `audio*` (at `envy?`, ...), every other `midi*` (at `umidi?`, `envy?`, `mpu?`), `pci*` at
//! `pchb?`, and every device at `virtio?` but `vio*`, `vioblk*` and `vioscsi*`; every device at `uhub?` but `uhub*`, `umass*`, `uhidev*`, `uaudio*`, `cdce*`, `uftdi*` and `ugen*`, every device
//! at `uhidev?` but `ukbd*`, `ums*`, `uwacom*` and `uhid*`, every `wskbd*` but the one at `ukbd?`, every
//! `wsmouse*` but the ones at `ums?` and `uwacom?`, every `ucom*` but the one at `uftdi?`;
//! `mpath0 at root`; the other pseudo-devices (`pdevinit[]`). Each entry keeps `config(8)`'s
//! layout: attachment, driver, unit, state, locators, flags, parents (indices into
//! `CFDATA`), the start of its locator names and the first unit a starred entry may take.

use libkern::StaticCell;

use crate::arch::amd64::amd64::acpi_machdep::ACPI_CA;
use crate::arch::amd64::amd64::bios::{BIOS_CA, BIOS_CD};
use crate::arch::amd64::amd64::cpu::{CPU_CA, CPU_CD};
use crate::arch::amd64::amd64::efifb::{EFIFB_CA, EFIFB_CD};
use crate::arch::amd64::amd64::ioapic::{IOAPIC_CA, IOAPIC_CD};
use crate::arch::amd64::amd64::mainbus::{MAINBUS_CA, MAINBUS_CD};
use crate::arch::amd64::pci::acpipci::{ACPIPCI_CA, ACPIPCI_CD};
use crate::dev::acpi::acpi::ACPI_CD;
use crate::dev::acpi::acpicpu_x86::{ACPICPU_CA, ACPICPU_CD};
use crate::dev::acpi::acpidmar::{ACPIDMAR_CA, ACPIDMAR_CD};
use crate::dev::acpi::acpihpet::{ACPIHPET_CA, ACPIHPET_CD};
use crate::dev::acpi::acpimadt::{ACPIMADT_CA, ACPIMADT_CD};
use crate::dev::acpi::acpimcfg::{ACPIMCFG_CA, ACPIMCFG_CD};
use crate::dev::acpi::acpiprt::{ACPIPRT_CA, ACPIPRT_CD};
use crate::dev::acpi::acpitimer::{ACPITIMER_CA, ACPITIMER_CD};
use crate::dev::acpi::ipmi_acpi::IPMI_ACPI_CA;
use crate::dev::acpi::tpm::{TPM_CA, TPM_CD};
use crate::dev::audio::{AUDIO_CA, AUDIO_CD};
use crate::dev::bio::bioattach;
use crate::dev::i2c::i2c::{IIC_CA, IIC_CD};
use crate::dev::ic::ahci::AHCI_CD;
use crate::dev::ic::com::COM_CD;
use crate::dev::ic::dc::DC_CD;
use crate::dev::ic::fxp::FXP_CD;
use crate::dev::ic::lpt::LPT_CD;
use crate::dev::ic::ne2000::NE_CD;
use crate::dev::ic::nvme::NVME_CD;
use crate::dev::ic::re::RE_CD;
use crate::dev::ic::siop::SIOP_CD;
use crate::dev::ic::vga::VGA_CD;
use crate::dev::ipmi::{IPMI_CA, IPMI_CD};
use crate::dev::isa::com_isa::COM_ISA_CA;
use crate::dev::isa::isa::{ISA_CA, ISA_CD};
use crate::dev::isa::lpt_isa::LPT_ISA_CA;
use crate::dev::isa::vga_isa::VGA_ISA_CA;
use crate::dev::midi::{MIDI_CA, MIDI_CD};
use crate::dev::mii::dcphy::{DCPHY_CA, DCPHY_CD};
use crate::dev::mii::inphy::{INPHY_CA, INPHY_CD};
use crate::dev::mii::lxtphy::{LXTPHY_CA, LXTPHY_CD};
use crate::dev::mii::rgephy::{RGEPHY_CA, RGEPHY_CD};
use crate::dev::mii::rlphy::{RLPHY_CA, RLPHY_CD};
use crate::dev::mii::ukphy::{UKPHY_CA, UKPHY_CD};
use crate::dev::pci::ahci_pci::AHCI_PCI_CA;
use crate::dev::pci::auich::{AUICH_CA, AUICH_CD};
use crate::dev::pci::azalia::{AZALIA_CA, AZALIA_CD};
use crate::dev::pci::eap::{EAP_CA, EAP_CD};
use crate::dev::pci::ehci_pci::EHCI_PCI_CA;
use crate::dev::pci::ichiic::{ICHIIC_CA, ICHIIC_CD};
use crate::dev::pci::if_dc_pci::DC_PCI_CA;
use crate::dev::pci::if_em::{EM_CA, EM_CD};
use crate::dev::pci::if_fxp_pci::FXP_PCI_CA;
use crate::dev::pci::if_ne_pci::NE_PCI_CA;
use crate::dev::pci::if_pcn::{PCN_CA, PCN_CD};
use crate::dev::pci::if_re_pci::RE_PCI_CA;
use crate::dev::pci::if_vmx::{VMX_CA, VMX_CD};
use crate::dev::pci::nvme_pci::NVME_PCI_CA;
use crate::dev::pci::ohci_pci::OHCI_PCI_CA;
use crate::dev::pci::pci::{PCI_CA, PCI_CD};
use crate::dev::pci::piixpm::{PIIXPM_CA, PIIXPM_CD};
use crate::dev::pci::ppb::{PPB_CA, PPB_CD};
use crate::dev::pci::puc::{PUC_CD, PUC_PCI_CA};
use crate::dev::pci::siop_pci::SIOP_PCI_CA;
use crate::dev::pci::uhci_pci::UHCI_PCI_CA;
use crate::dev::pci::vga_pci::VGA_PCI_CA;
use crate::dev::pci::virtio_pci::VIRTIO_PCI_CA;
use crate::dev::pci::xhci_pci::XHCI_PCI_CA;
use crate::dev::puc::com_puc::COM_PUC_CA;
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
use crate::sys::device::{Cfdata, FSTATE_DNOTFOUND, FSTATE_NOTFOUND, FSTATE_STAR, Pdevinit};

/// `pv[]` for children of `mainbus0` (`cfdata[0]`).
const PV_MAINBUS: &[i16] = &[0];

/// `loc[]` of an entry at `pcibus` with the default `bus = -1` (`conf/files`: `define pcibus
/// {[bus = -1]}`).
const LOC_PCIBUS_UNK: &[i64] = &[-1];

/// `pv[]` for children of `pci*`: at mainbus0 (`cfdata[2]`) and at ppb (`cfdata[54]`).
const PV_PCI: &[i16] = &[2, 54];

/// `pv[]` for children of `ppb*` (`cfdata[53]`) through the `pcibus` attribute.
const PV_PPB: &[i16] = &[53];

/// `pv[]` for the `iic*` at `piixpm?` (`cfdata[56]`) through the `i2cbus` attribute.
const PV_PIIXPM: &[i16] = &[56];

/// `pv[]` for the `iic*` at `ichiic?` (`cfdata[58]`) through the `i2cbus` attribute.
const PV_ICHIIC: &[i16] = &[58];

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
/// `loc[]` of `lpt0 at isa? port 0x378 irq 7` (M16d).
const LOC_LPT0: &[i64] = &[0x378, 0, -1, 0, 7, -1, -1];

/// `pv[]` for children of the `usbus` attribute, carried by `xhci*` (`cfdata[14]`), `ehci*`
/// (`cfdata[71]`), `uhci*` (`cfdata[72]`) and `ohci*` (`cfdata[73]`): `usb* at xhci?`, `usb* at ehci?`, `usb* at
/// uhci?` and `usb* at ohci?` are one entry, as config(8) merges them.
const PV_USBUS: &[i16] = &[14, 71, 72, 73];

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

/// `pv[]` for children of the `audio` attribute, carried by `azalia*` (`cfdata[20]`),
/// `uaudio*` (`cfdata[69]`) and `eap*` (`cfdata[84]`): `config(8)` merges `audio* at
/// azalia?`, `audio* at uaudio?` and `audio* at eap?` into one entry.
const PV_AZALIA: &[i16] = &[20, 69, 84];

/// `pv[]` for children of `eap*` (`cfdata[84]`) through the `midibus` attribute: `midi*
/// at eap?` (M16d).
const PV_EAP: &[i16] = &[84];

/// `pv[]` for children of `bios0` (`cfdata[30]`).
const PV_BIOS: &[i16] = &[30];

/// `pv[]` for children of `acpi0` (`cfdata[31]`).
const PV_ACPI: &[i16] = &[31];

/// `pv[]` for children of `puc*` (`cfdata[38]`).
const PV_PUC: &[i16] = &[38];

/// `loc[]` of an entry at `puc` with the default `port = -1` (`files.pci`: `device puc {[port =
/// -1]}`).
const LOC_PUC_UNK: &[i64] = &[-1];

/// `pv[]` for children of the `mii` attribute, carried by every device with that attribute:
/// `re*` (`cfdata[42]`), `pcn*` (`cfdata[77]`), `ne*` (`cfdata[78]`), `fxp*` (`cfdata[79]`) and
/// `dc*` (`cfdata[81]`): `config(8)` merges the `at mii?` lines of one device.
const PV_MII: &[i16] = &[42, 77, 78, 79, 81];

/// `loc[]` of an entry at `mii` with the default `phy = -1` (`conf/files`: `define mii {[phy =
/// -1]}`).
const LOC_MII_UNK: &[i64] = &[-1];

/// `pv[]` for children of `efifb0` (`cfdata[41]`).
const PV_EFIFB: &[i16] = &[41];

/// `loc[]` of an entry at `wsemuldisplaydev` with the defaults `console = -1, primary = -1,
/// mux = 1` (`conf/files`: `define wsemuldisplaydev {[console = -1], [primary = -1], [mux =
/// 1]}`).
const LOC_WSEMULDISPLAYDEV_UNK: &[i64] = &[-1, -1, 1];

/// `loc[]` of `vga0 at isa?`: port, size, iomem, iomsiz, irq, drq, drq2 at their
/// defaults (`dev/isa/files.isa`: `device isa {[port = -1], [size = 0], [iomem = -1],
/// [iomsiz = 0], [irq = -1], [drq = -1], [drq2 = -1]}`).
const LOC_VGA_ISA: &[i64] = &[-1, 0, -1, 0, -1, -1, -1];

/// `pv[]` for children of the `vga` device, `vga0 at isa?` (`cfdata[48]`) and `vga* at
/// pci?` (`cfdata[49]`).
const PV_VGA: &[i16] = &[48, 49];

/// `loc[]` of `wsdisplay0 at vga? console 1`: console 1, primary -1, mux 1.
const LOC_WSEMULDISPLAYDEV_CONSOLE: &[i64] = &[1, -1, 1];

/// `pv[]` for children of `ukbd*` (`cfdata[24]`): the `wskbddev` attribute.
const PV_UKBD: &[i16] = &[24];

/// `loc[]` of `wskbd* at ukbd? mux 1`: `console = -1` (`conf/files`: `define wskbddev
/// {[console = -1], [mux = 1]}`), `mux 1`.
const LOC_WSKBDDEV_MUX1: &[i64] = &[-1, 1];

/// `pv[]` for children of `ums*` (`cfdata[64]`): the `wsmousedev` attribute.
const PV_UMS: &[i16] = &[64];

/// `pv[]` for children of `uwacom*` (`cfdata[66]`): the `wsmousedev` attribute.
const PV_UWACOM: &[i16] = &[66];

/// `loc[]` of `wsmouse* at ums? mux 0` and `wsmouse* at uwacom? mux 0`: `mux = 0` (`conf/files`:
/// `define wsmousedev {[mux = 0]}`).
const LOC_WSMOUSEDEV_MUX0: &[i64] = &[0];

// `cf_locnames`: where an entry's run of locator names starts in `LOCNAMP`; an entry without
// locators has 0, the empty run at `LOCNAMP[0]`.
/// `cf_locnames` of an entry at `pcibus`: `bus`.
const LN_PCIBUS: i32 = 1;
/// `cf_locnames` of an entry at `pci`: `dev`, `function`.
const LN_PCI: i32 = 3;
/// `cf_locnames` of an entry at `isa`: `port`, `size`, `iomem`, `iosiz`, `irq`, `drq`, `drq2`.
const LN_ISA: i32 = 6;
/// `cf_locnames` of an entry at `scsibus`: `target`, `lun`.
const LN_SCSIBUS: i32 = 14;
/// `cf_locnames` of an entry at `uhub`: `port`, `configuration`, `interface`, `vendor`, `product`, `release`.
const LN_UHUB: i32 = 17;
/// `cf_locnames` of an entry at `uhidbus`: `reportid`.
const LN_UHIDBUS: i32 = 24;
/// `cf_locnames` of an entry at `puc`: `port`.
const LN_PUC: i32 = 26;
/// `cf_locnames` of an entry at `mii`: `phy`.
const LN_MII: i32 = 28;
/// `cf_locnames` of an entry at `wsemuldisplaydev`: `console`, `primary`, `mux`.
const LN_WSEMULDISPLAYDEV: i32 = 30;
/// `cf_locnames` of an entry at `wskbddev`: `console`, `mux`.
const LN_WSKBDDEV: i32 = 34;
/// `cf_locnames` of an entry at `wsmousedev`: `mux`.
const LN_WSMOUSEDEV: i32 = 37;
/// `cf_locnames` of an entry at `ucombus`: `portno`.
const LN_UCOMBUS: i32 = 39;

/// `pv[]` for children of `uftdi*` (`cfdata[75]`): the `ucombus` attribute (`files.usb`:
/// `define ucombus {[portno = -1]}`) is carried by `uftdi` alone here.
const PV_UFTDI: &[i16] = &[75];

/// `loc[]` of an entry at `ucombus` with the default `portno = -1`.
const LOC_UCOMBUS_UNK: &[i64] = &[-1];

/// `{0}`: the free slots `config(8)` leaves at the end of `cfdata[]` for UKC's `add`.
const NFREE: usize = 8;

/// `cfdata[]`: 87 entries, 88 with `MULTIPROCESSOR` (GENERIC.MP's `cpu* at mainbus?`).
const NCFDATA: usize = if cfg!(feature = "multiprocessor") {
    88
} else {
    87
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
        LN_PCIBUS,
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
        LN_PCI,
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
        LN_ISA,
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
        LN_ISA,
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
        LN_ISA,
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
        LN_ISA,
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
        LN_SCSIBUS,
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
        LN_PCI,
        0,
    ),
    // 15: usb* at xhci?, usb* at ehci?, usb* at uhci?, usb* at ohci?
    Cfdata::new(&USB_CA, &USB_CD, 0, FSTATE_STAR, &[], 0, PV_USBUS, 0, 0),
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
        LN_UHUB,
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
        LN_PCI,
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
        LN_PCI,
        0,
    ),
    // 21: audio* at azalia?, audio* at uaudio?, audio* at eap?
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
        LN_UHUB,
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
        LN_UHUB,
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
        LN_UHIDBUS,
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
        LN_PCI,
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
        LN_SCSIBUS,
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
        LN_PCI,
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
        LN_PCI,
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
        LN_PCI,
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
        LN_PUC,
        4,
    ),
    // 40: em* at pci?
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
    // 41: efifb0 at mainbus?
    Cfdata::new(
        &EFIFB_CA,
        &EFIFB_CD,
        0,
        FSTATE_NOTFOUND,
        &[],
        0,
        PV_MAINBUS,
        0,
        0,
    ),
    // 42: re* at pci?
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
    // 43: rlphy* at mii?
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
    // 44: rgephy* at mii?
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
    // 45: ukphy* at mii?
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
    // 46: wsdisplay0 at efifb?
    Cfdata::new(
        &WSDISPLAY_CA,
        &WSDISPLAY_CD,
        0,
        FSTATE_NOTFOUND,
        LOC_WSEMULDISPLAYDEV_UNK,
        0,
        PV_EFIFB,
        LN_WSEMULDISPLAYDEV,
        0,
    ),
    // 47: vmx* at pci?
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
    // 48: vga0 at isa?
    Cfdata::new(
        &VGA_ISA_CA,
        &VGA_CD,
        0,
        FSTATE_NOTFOUND,
        LOC_VGA_ISA,
        0,
        PV_ISA,
        LN_ISA,
        0,
    ),
    // 49: vga* at pci?: the units from 1 on (vga0 is the ISA line above)
    Cfdata::new(
        &VGA_PCI_CA,
        &VGA_CD,
        1,
        FSTATE_STAR,
        LOC_PCI_UNK,
        0,
        PV_PCI,
        LN_PCI,
        1,
    ),
    // 50: wsdisplay0 at vga? console 1
    Cfdata::new(
        &WSDISPLAY_CA,
        &WSDISPLAY_CD,
        0,
        FSTATE_NOTFOUND,
        LOC_WSEMULDISPLAYDEV_CONSOLE,
        0,
        PV_VGA,
        LN_WSEMULDISPLAYDEV,
        0,
    ),
    // 51: wskbd* at ukbd? mux 1
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
    // 52: acpimcfg* at acpi?
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
    // 55: acpidmar0 at acpi? disable (M16e): FSTATE_DNOTFOUND, as config(8) writes a
    // disabled unit; `boot -c` (UKC `enable acpidmar`) turns it on.
    Cfdata::new(
        &ACPIDMAR_CA,
        &ACPIDMAR_CD,
        0,
        FSTATE_DNOTFOUND,
        &[],
        0,
        PV_ACPI,
        0,
        0,
    ),
    // 56: piixpm* at pci?
    Cfdata::new(
        &PIIXPM_CA,
        &PIIXPM_CD,
        0,
        FSTATE_STAR,
        LOC_PCI_UNK,
        0,
        PV_PCI,
        LN_PCI,
        0,
    ),
    // 57: iic* at piixpm?
    Cfdata::new(&IIC_CA, &IIC_CD, 0, FSTATE_STAR, &[], 0, PV_PIIXPM, 0, 0),
    // 58: ichiic* at pci?
    Cfdata::new(
        &ICHIIC_CA,
        &ICHIIC_CD,
        0,
        FSTATE_STAR,
        LOC_PCI_UNK,
        0,
        PV_PCI,
        LN_PCI,
        0,
    ),
    // 59: iic* at ichiic?
    Cfdata::new(&IIC_CA, &IIC_CD, 0, FSTATE_STAR, &[], 0, PV_ICHIIC, 0, 0),
    // 60: ipmi0 at acpi? disable
    Cfdata::new(
        &IPMI_ACPI_CA,
        &IPMI_CD,
        0,
        FSTATE_DNOTFOUND,
        &[],
        0,
        PV_ACPI,
        0,
        0,
    ),
    // 61: ipmi0 at mainbus? disable
    Cfdata::new(
        &IPMI_CA,
        &IPMI_CD,
        0,
        FSTATE_DNOTFOUND,
        &[],
        0,
        PV_MAINBUS,
        0,
        0,
    ),
    // 62: tpm* at acpi? (M16e)
    Cfdata::new(&TPM_CA, &TPM_CD, 0, FSTATE_STAR, &[], 0, PV_ACPI, 0, 0),
    // 63: acpicpu* at acpi? (M16e)
    Cfdata::new(
        &ACPICPU_CA,
        &ACPICPU_CD,
        0,
        FSTATE_STAR,
        &[],
        0,
        PV_ACPI,
        0,
        0,
    ),
    // 64: ums* at uhidev?
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
    // 65: wsmouse* at ums? mux 0
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
    // 66: uwacom* at uhidev?
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
    // 67: wsmouse* at uwacom? mux 0
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
    // 68: uhid* at uhidev?
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
    // 69: uaudio* at uhub?
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
    // 70: ugen* at uhub?
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
    // 71: ehci* at pci?
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
    // 72: uhci* at pci?
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
    // 73: ohci* at pci?
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
    // 74: cdce* at uhub?
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
    // 75: uftdi* at uhub?
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
    // 76: ucom* at uftdi?
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
    // 77: pcn* at pci? (M16c)
    Cfdata::new(
        &PCN_CA,
        &PCN_CD,
        0,
        FSTATE_STAR,
        LOC_PCI_UNK,
        0,
        PV_PCI,
        LN_PCI,
        0,
    ),
    // 78: ne* at pci? (M16c)
    Cfdata::new(
        &NE_PCI_CA,
        &NE_CD,
        0,
        FSTATE_STAR,
        LOC_PCI_UNK,
        0,
        PV_PCI,
        LN_PCI,
        0,
    ),
    // 79: fxp* at pci? (M16c)
    Cfdata::new(
        &FXP_PCI_CA,
        &FXP_CD,
        0,
        FSTATE_STAR,
        LOC_PCI_UNK,
        0,
        PV_PCI,
        LN_PCI,
        0,
    ),
    // 80: inphy* at mii? (M16c)
    Cfdata::new(
        &INPHY_CA,
        &INPHY_CD,
        0,
        FSTATE_STAR,
        LOC_MII_UNK,
        0,
        PV_MII,
        LN_MII,
        0,
    ),
    // 81: dc* at pci? (M16c)
    Cfdata::new(
        &DC_PCI_CA,
        &DC_CD,
        0,
        FSTATE_STAR,
        LOC_PCI_UNK,
        0,
        PV_PCI,
        LN_PCI,
        0,
    ),
    // 82: lxtphy* at mii? (M16c)
    Cfdata::new(
        &LXTPHY_CA,
        &LXTPHY_CD,
        0,
        FSTATE_STAR,
        LOC_MII_UNK,
        0,
        PV_MII,
        LN_MII,
        0,
    ),
    // 83: dcphy* at mii? (M16c)
    Cfdata::new(
        &DCPHY_CA,
        &DCPHY_CD,
        0,
        FSTATE_STAR,
        LOC_MII_UNK,
        0,
        PV_MII,
        LN_MII,
        0,
    ),
    // 84: eap* at pci? (M16d)
    Cfdata::new(
        &EAP_CA,
        &EAP_CD,
        0,
        FSTATE_STAR,
        LOC_PCI_UNK,
        0,
        PV_PCI,
        LN_PCI,
        0,
    ),
    // 85: midi* at eap? (M16d)
    Cfdata::new(&MIDI_CA, &MIDI_CD, 0, FSTATE_STAR, &[], 0, PV_EAP, 0, 0),
    // 86: lpt0 at isa? port 0x378 irq 7 (M16d)
    Cfdata::new(
        &LPT_ISA_CA,
        &LPT_CD,
        0,
        FSTATE_NOTFOUND,
        LOC_LPT0,
        0,
        PV_ISA,
        LN_ISA,
        0,
    ),
    // 87: cpu* at mainbus? (GENERIC.MP, MULTIPROCESSOR): the application processors, unit 1
    // on (cpu0 takes unit 0).
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
pub static CFROOTS: StaticCell<[i16; 2]> = StaticCell::new([0, 13]);

/// The number of `pdevinit[]` entries.
const NPDEVINIT: usize = 13 + cfg!(feature = "fuse") as usize;

/// `pdevinit[]`: the pseudo-devices of the MI `conf/GENERIC` whose attach functions are
/// ported, in `ioconf.c`'s order (`pseudo-device pf`, `pseudo-device pflog`, `pseudo-device
/// pfsync`, `pseudo-device pflow`, `pseudo-device enc`, `pseudo-device pty 16`, `pseudo-device
/// vnd 4`, `pseudo-device bpfilter`, `pseudo-device loop`, `pseudo-device wg`, `pseudo-device
/// bio 1`, `pseudo-device fuse` under feature `fuse`; all but pty and vnd with a count of 1), then
/// the machine GENERIC's `pseudo-device wsmux 2` (M13), then `pseudo-device rd 1`, which is not in
/// GENERIC but in the RAMDISK kernels (`arch/amd64/conf/RAMDISK*`): this kernel boots its root
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
pub static LOCNAMES: [&[u8]; 23] = [
    b"bus",
    b"dev",
    b"function",
    b"port",
    b"size",
    b"iomem",
    b"iosiz",
    b"irq",
    b"drq",
    b"drq2",
    b"target",
    b"lun",
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
pub static LOCNAMP: [i16; 41] = [
    -1, 0, -1, 1, 2, -1, 3, 4, 5, 6, 7, 8, 9, -1, 10, 11, -1, 3, 12, 13, 14, 15, 16, -1, 17, -1, 3,
    -1, 18, -1, 19, 20, 21, -1, 19, 21, -1, 21, -1, 22, -1,
];
/* </CODE> */
