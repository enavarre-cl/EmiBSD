/*	$OpenBSD: pci_subr.c,v 1.22 2017/03/22 07:21:39 jsg Exp $	*/
/*	$NetBSD: pci_subr.c,v 1.19 1996/10/13 01:38:29 christos Exp $	*/
/* <LICENSES> */
/*
 * Copyright (c) 1995, 1996 Christopher G. Demetriou.  All rights reserved.
 * Copyright (c) 1994 Charles Hannum.  All rights reserved.
 *
 * Redistribution and use in source and binary forms, with or without
 * modification, are permitted provided that the following conditions
 * are met:
 * 1. Redistributions of source code must retain the above copyright
 *    notice, this list of conditions and the following disclaimer.
 * 2. Redistributions in binary form must reproduce the above copyright
 *    notice, this list of conditions and the following disclaimer in the
 *    documentation and/or other materials provided with the distribution.
 * 3. All advertising materials mentioning features or use of this software
 *    must display the following acknowledgement:
 *	This product includes software developed by Charles Hannum.
 * 4. The name of the author may not be used to endorse or promote products
 *    derived from this software without specific prior written permission.
 *
 * THIS SOFTWARE IS PROVIDED BY THE AUTHOR ``AS IS'' AND ANY EXPRESS OR
 * IMPLIED WARRANTIES, INCLUDING, BUT NOT LIMITED TO, THE IMPLIED WARRANTIES
 * OF MERCHANTABILITY AND FITNESS FOR A PARTICULAR PURPOSE ARE DISCLAIMED.
 * IN NO EVENT SHALL THE AUTHOR BE LIABLE FOR ANY DIRECT, INDIRECT,
 * INCIDENTAL, SPECIAL, EXEMPLARY, OR CONSEQUENTIAL DAMAGES (INCLUDING, BUT
 * NOT LIMITED TO, PROCUREMENT OF SUBSTITUTE GOODS OR SERVICES; LOSS OF USE,
 * DATA, OR PROFITS; OR BUSINESS INTERRUPTION) HOWEVER CAUSED AND ON ANY
 * THEORY OF LIABILITY, WHETHER IN CONTRACT, STRICT LIABILITY, OR TORT
 * (INCLUDING NEGLIGENCE OR OTHERWISE) ARISING IN ANY WAY OUT OF THE USE OF
 * THIS SOFTWARE, EVEN IF ADVISED OF THE POSSIBILITY OF SUCH DAMAGE.
 */
/* </LICENSES> */

/* <CODE> */
//! PCI autoconfiguration support functions: `dev/pci/pci_subr.c`.
//!
//! Upstream: sys/dev/pci/pci_subr.c @ 3ce1f3f79392
//!
//! The descriptions of the known PCI classes and subclasses, and `pci_devinfo`, which turns
//! a device's ID and class registers into the text of its attach line ("vendor 0x8086
//! product 0x29c0 (class bridge subclass host, rev 0x02)").
//!
//! ## Deviations
//! - `option PCIVERBOSE` (in amd64 GENERIC) is not configured: the vendor and product name
//!   tables (`pcidevs_data.h`, 800 KB generated from `pcidevs`) are not ported, so
//!   `pci_findvendor` and `pci_findproduct` find nothing and `pci_devinfo` prints the IDs,
//!   as a kernel built without the option does (`docs/ARCHITECTURE.md`). The verbose branches
//!   are kept behind the [`PCIVERBOSE`] constant.
//! - The class tables are slices without the C's terminating `{ 0 }` entry; a subclass table
//!   is an `Option` (the C's NULL `subclasses`).
//! - `pci_devinfo` writes a NUL-terminated string into a byte slice (the C's `cp`/`cp_max`).

use crate::dev::pci::pcireg::*;
use crate::dev::pci::pcivar::Pcireg;
use crate::kern::subr_prf::{Str, snprintf};

/// `PCIVERBOSE`: print vendor and product names (see the module's deviations).
pub const PCIVERBOSE: bool = false;

/// `struct pci_class`: the description of a known PCI class or subclass. Subclasses are
/// described in the same way as classes, but have no subclass table.
pub struct PciClass {
    /// `name`.
    pub name: &'static [u8],
    /// `val`: as wide as `pci_{,sub}class_t`.
    pub val: u32,
    /// `subclasses`.
    pub subclasses: Option<&'static [PciClass]>,
}

/// A subclass entry.
const fn sub(name: &'static [u8], val: u32) -> PciClass {
    PciClass {
        name,
        val,
        subclasses: None,
    }
}

/// `pci_subclass_prehistoric[]`.
pub static PCI_SUBCLASS_PREHISTORIC: [PciClass; 2] = [
    sub(b"miscellaneous", PCI_SUBCLASS_PREHISTORIC_MISC),
    sub(b"VGA", PCI_SUBCLASS_PREHISTORIC_VGA),
];

/// `pci_subclass_mass_storage[]`.
pub static PCI_SUBCLASS_MASS_STORAGE: [PciClass; 10] = [
    sub(b"SCSI", PCI_SUBCLASS_MASS_STORAGE_SCSI),
    sub(b"IDE", PCI_SUBCLASS_MASS_STORAGE_IDE),
    sub(b"floppy", PCI_SUBCLASS_MASS_STORAGE_FLOPPY),
    sub(b"IPI", PCI_SUBCLASS_MASS_STORAGE_IPI),
    sub(b"RAID", PCI_SUBCLASS_MASS_STORAGE_RAID),
    sub(b"ATA", PCI_SUBCLASS_MASS_STORAGE_ATA),
    sub(b"SATA", PCI_SUBCLASS_MASS_STORAGE_SATA),
    sub(b"SAS", PCI_SUBCLASS_MASS_STORAGE_SAS),
    sub(b"UFS", PCI_SUBCLASS_MASS_STORAGE_UFS),
    sub(b"miscellaneous", PCI_SUBCLASS_MASS_STORAGE_MISC),
];

/// `pci_subclass_network[]`.
pub static PCI_SUBCLASS_NETWORK: [PciClass; 9] = [
    sub(b"ethernet", PCI_SUBCLASS_NETWORK_ETHERNET),
    sub(b"token ring", PCI_SUBCLASS_NETWORK_TOKENRING),
    sub(b"FDDI", PCI_SUBCLASS_NETWORK_FDDI),
    sub(b"ATM", PCI_SUBCLASS_NETWORK_ATM),
    sub(b"ISDN", PCI_SUBCLASS_NETWORK_ISDN),
    sub(b"WorldFip", PCI_SUBCLASS_NETWORK_WORLDFIP),
    sub(
        b"PCMIG Multi Computing",
        PCI_SUBCLASS_NETWORK_PCIMGMULTICOMP,
    ),
    sub(b"InfiniBand", PCI_SUBCLASS_NETWORK_INFINIBAND),
    sub(b"miscellaneous", PCI_SUBCLASS_NETWORK_MISC),
];

/// `pci_subclass_display[]`.
pub static PCI_SUBCLASS_DISPLAY: [PciClass; 4] = [
    sub(b"VGA", PCI_SUBCLASS_DISPLAY_VGA),
    sub(b"XGA", PCI_SUBCLASS_DISPLAY_XGA),
    sub(b"3D", PCI_SUBCLASS_DISPLAY_3D),
    sub(b"miscellaneous", PCI_SUBCLASS_DISPLAY_MISC),
];

/// `pci_subclass_multimedia[]`.
pub static PCI_SUBCLASS_MULTIMEDIA: [PciClass; 5] = [
    sub(b"video", PCI_SUBCLASS_MULTIMEDIA_VIDEO),
    sub(b"audio", PCI_SUBCLASS_MULTIMEDIA_AUDIO),
    sub(b"telephony", PCI_SUBCLASS_MULTIMEDIA_TELEPHONY),
    sub(b"hdaudio", PCI_SUBCLASS_MULTIMEDIA_HDAUDIO),
    sub(b"miscellaneous", PCI_SUBCLASS_MULTIMEDIA_MISC),
];

/// `pci_subclass_memory[]`.
pub static PCI_SUBCLASS_MEMORY: [PciClass; 3] = [
    sub(b"RAM", PCI_SUBCLASS_MEMORY_RAM),
    sub(b"flash", PCI_SUBCLASS_MEMORY_FLASH),
    sub(b"miscellaneous", PCI_SUBCLASS_MEMORY_MISC),
];

/// `pci_subclass_bridge[]`.
pub static PCI_SUBCLASS_BRIDGE: [PciClass; 13] = [
    sub(b"host", PCI_SUBCLASS_BRIDGE_HOST),
    sub(b"ISA", PCI_SUBCLASS_BRIDGE_ISA),
    sub(b"EISA", PCI_SUBCLASS_BRIDGE_EISA),
    sub(b"MicroChannel", PCI_SUBCLASS_BRIDGE_MC),
    sub(b"PCI", PCI_SUBCLASS_BRIDGE_PCI),
    sub(b"PCMCIA", PCI_SUBCLASS_BRIDGE_PCMCIA),
    sub(b"NuBus", PCI_SUBCLASS_BRIDGE_NUBUS),
    sub(b"CardBus", PCI_SUBCLASS_BRIDGE_CARDBUS),
    sub(b"RACEway", PCI_SUBCLASS_BRIDGE_RACEWAY),
    sub(b"Semi-transparent PCI", PCI_SUBCLASS_BRIDGE_STPCI),
    sub(b"InfiniBand", PCI_SUBCLASS_BRIDGE_INFINIBAND),
    sub(b"miscellaneous", PCI_SUBCLASS_BRIDGE_MISC),
    sub(b"advanced switching", PCI_SUBCLASS_BRIDGE_AS),
];

/// `pci_subclass_communications[]`.
pub static PCI_SUBCLASS_COMMUNICATIONS: [PciClass; 7] = [
    sub(b"serial", PCI_SUBCLASS_COMMUNICATIONS_SERIAL),
    sub(b"parallel", PCI_SUBCLASS_COMMUNICATIONS_PARALLEL),
    sub(b"multi-port serial", PCI_SUBCLASS_COMMUNICATIONS_MPSERIAL),
    sub(b"modem", PCI_SUBCLASS_COMMUNICATIONS_MODEM),
    sub(b"GPIB", PCI_SUBCLASS_COMMUNICATIONS_GPIB),
    sub(b"smartcard", PCI_SUBCLASS_COMMUNICATIONS_SMARTCARD),
    sub(b"miscellaneous", PCI_SUBCLASS_COMMUNICATIONS_MISC),
];

/// `pci_subclass_system[]`.
pub static PCI_SUBCLASS_SYSTEM: [PciClass; 9] = [
    sub(b"interrupt", PCI_SUBCLASS_SYSTEM_PIC),
    sub(b"8237 DMA", PCI_SUBCLASS_SYSTEM_DMA),
    sub(b"8254 timer", PCI_SUBCLASS_SYSTEM_TIMER),
    sub(b"RTC", PCI_SUBCLASS_SYSTEM_RTC),
    sub(b"PCI Hot-Plug", PCI_SUBCLASS_SYSTEM_PCIHOTPLUG),
    sub(b"SD Host Controller", PCI_SUBCLASS_SYSTEM_SDHC),
    sub(b"IOMMU", PCI_SUBCLASS_SYSTEM_IOMMU),
    sub(b"root complex event", PCI_SUBCLASS_SYSTEM_ROOTCOMPEVENT),
    sub(b"miscellaneous", PCI_SUBCLASS_SYSTEM_MISC),
];

/// `pci_subclass_input[]`.
pub static PCI_SUBCLASS_INPUT: [PciClass; 6] = [
    sub(b"keyboard", PCI_SUBCLASS_INPUT_KEYBOARD),
    sub(b"digitizer", PCI_SUBCLASS_INPUT_DIGITIZER),
    sub(b"mouse", PCI_SUBCLASS_INPUT_MOUSE),
    sub(b"scanner", PCI_SUBCLASS_INPUT_SCANNER),
    sub(b"game port", PCI_SUBCLASS_INPUT_GAMEPORT),
    sub(b"miscellaneous", PCI_SUBCLASS_INPUT_MISC),
];

/// `pci_subclass_dock[]`.
pub static PCI_SUBCLASS_DOCK: [PciClass; 2] = [
    sub(b"generic", PCI_SUBCLASS_DOCK_GENERIC),
    sub(b"miscellaneous", PCI_SUBCLASS_DOCK_MISC),
];

/// `pci_subclass_processor[]`.
pub static PCI_SUBCLASS_PROCESSOR: [PciClass; 7] = [
    sub(b"386", PCI_SUBCLASS_PROCESSOR_386),
    sub(b"486", PCI_SUBCLASS_PROCESSOR_486),
    sub(b"Pentium", PCI_SUBCLASS_PROCESSOR_PENTIUM),
    sub(b"Alpha", PCI_SUBCLASS_PROCESSOR_ALPHA),
    sub(b"PowerPC", PCI_SUBCLASS_PROCESSOR_POWERPC),
    sub(b"MIPS", PCI_SUBCLASS_PROCESSOR_MIPS),
    sub(b"Co-processor", PCI_SUBCLASS_PROCESSOR_COPROC),
];

/// `pci_subclass_serialbus[]`.
pub static PCI_SUBCLASS_SERIALBUS: [PciClass; 10] = [
    sub(b"Firewire", PCI_SUBCLASS_SERIALBUS_FIREWIRE),
    sub(b"ACCESS.bus", PCI_SUBCLASS_SERIALBUS_ACCESS),
    sub(b"SSA", PCI_SUBCLASS_SERIALBUS_SSA),
    sub(b"USB", PCI_SUBCLASS_SERIALBUS_USB),
    // XXX Fiber Channel/_FIBRECHANNEL
    sub(b"Fiber Channel", PCI_SUBCLASS_SERIALBUS_FIBER),
    sub(b"SMBus", PCI_SUBCLASS_SERIALBUS_SMBUS),
    sub(b"InfiniBand", PCI_SUBCLASS_SERIALBUS_INFINIBAND),
    sub(b"IPMI", PCI_SUBCLASS_SERIALBUS_IPMI),
    sub(b"SERCOS", PCI_SUBCLASS_SERIALBUS_SERCOS),
    sub(b"CANbus", PCI_SUBCLASS_SERIALBUS_CANBUS),
];

/// `pci_subclass_wireless[]`.
pub static PCI_SUBCLASS_WIRELESS: [PciClass; 8] = [
    sub(b"IrDA", PCI_SUBCLASS_WIRELESS_IRDA),
    sub(b"Consumer IR", PCI_SUBCLASS_WIRELESS_CONSUMERIR),
    sub(b"RF", PCI_SUBCLASS_WIRELESS_RF),
    sub(b"bluetooth", PCI_SUBCLASS_WIRELESS_BLUETOOTH),
    sub(b"broadband", PCI_SUBCLASS_WIRELESS_BROADBAND),
    sub(b"802.11a (5 GHz)", PCI_SUBCLASS_WIRELESS_802_11A),
    sub(b"802.11b (2.4 GHz)", PCI_SUBCLASS_WIRELESS_802_11B),
    sub(b"miscellaneous", PCI_SUBCLASS_WIRELESS_MISC),
];

/// `pci_subclass_i2o[]`.
pub static PCI_SUBCLASS_I2O: [PciClass; 1] = [sub(b"standard", PCI_SUBCLASS_I2O_STANDARD)];

/// `pci_subclass_satcom[]`.
pub static PCI_SUBCLASS_SATCOM: [PciClass; 4] = [
    sub(b"TV", PCI_SUBCLASS_SATCOM_TV),
    sub(b"audio", PCI_SUBCLASS_SATCOM_AUDIO),
    sub(b"voice", PCI_SUBCLASS_SATCOM_VOICE),
    sub(b"data", PCI_SUBCLASS_SATCOM_DATA),
];

/// `pci_subclass_crypto[]`.
pub static PCI_SUBCLASS_CRYPTO: [PciClass; 3] = [
    sub(b"network/computing", PCI_SUBCLASS_CRYPTO_NETCOMP),
    sub(b"entertainment", PCI_SUBCLASS_CRYPTO_ENTERTAINMENT),
    sub(b"miscellaneous", PCI_SUBCLASS_CRYPTO_MISC),
];

/// `pci_subclass_dasp[]`.
pub static PCI_SUBCLASS_DASP: [PciClass; 5] = [
    sub(b"DPIO", PCI_SUBCLASS_DASP_DPIO),
    sub(b"Time and Frequency", PCI_SUBCLASS_DASP_TIMEFREQ),
    sub(b"synchronization", PCI_SUBCLASS_DASP_SYNC),
    sub(b"management", PCI_SUBCLASS_DASP_MGMT),
    sub(b"miscellaneous", PCI_SUBCLASS_DASP_MISC),
];

/// A class entry.
const fn class(name: &'static [u8], val: u32, subclasses: &'static [PciClass]) -> PciClass {
    PciClass {
        name,
        val,
        subclasses: Some(subclasses),
    }
}

/// `pci_class[]`: the known classes.
pub static PCI_CLASS: [PciClass; 21] = [
    class(
        b"prehistoric",
        PCI_CLASS_PREHISTORIC,
        &PCI_SUBCLASS_PREHISTORIC,
    ),
    class(
        b"mass storage",
        PCI_CLASS_MASS_STORAGE,
        &PCI_SUBCLASS_MASS_STORAGE,
    ),
    class(b"network", PCI_CLASS_NETWORK, &PCI_SUBCLASS_NETWORK),
    class(b"display", PCI_CLASS_DISPLAY, &PCI_SUBCLASS_DISPLAY),
    class(
        b"multimedia",
        PCI_CLASS_MULTIMEDIA,
        &PCI_SUBCLASS_MULTIMEDIA,
    ),
    class(b"memory", PCI_CLASS_MEMORY, &PCI_SUBCLASS_MEMORY),
    class(b"bridge", PCI_CLASS_BRIDGE, &PCI_SUBCLASS_BRIDGE),
    class(
        b"communications",
        PCI_CLASS_COMMUNICATIONS,
        &PCI_SUBCLASS_COMMUNICATIONS,
    ),
    class(b"system", PCI_CLASS_SYSTEM, &PCI_SUBCLASS_SYSTEM),
    class(b"input", PCI_CLASS_INPUT, &PCI_SUBCLASS_INPUT),
    class(b"dock", PCI_CLASS_DOCK, &PCI_SUBCLASS_DOCK),
    class(b"processor", PCI_CLASS_PROCESSOR, &PCI_SUBCLASS_PROCESSOR),
    class(b"serial bus", PCI_CLASS_SERIALBUS, &PCI_SUBCLASS_SERIALBUS),
    class(b"wireless", PCI_CLASS_WIRELESS, &PCI_SUBCLASS_WIRELESS),
    class(b"I2O", PCI_CLASS_I2O, &PCI_SUBCLASS_I2O),
    class(b"satellite comm", PCI_CLASS_SATCOM, &PCI_SUBCLASS_SATCOM),
    class(b"crypto", PCI_CLASS_CRYPTO, &PCI_SUBCLASS_CRYPTO),
    class(b"DASP", PCI_CLASS_DASP, &PCI_SUBCLASS_DASP),
    sub(b"accelerator", PCI_CLASS_ACCELERATOR),
    sub(b"instrumentation", PCI_CLASS_INSTRUMENTATION),
    sub(b"undefined", PCI_CLASS_UNDEFINED),
];

/// `pci_findvendor`: the vendor's name, from `pcidevs_data.h` under `PCIVERBOSE`.
pub fn pci_findvendor(_id_reg: Pcireg) -> Option<&'static [u8]> {
    // PCIVERBOSE: walk pci_known_vendors[] for PCI_VENDOR(id_reg) (not ported).
    None
}

/// `pci_findproduct`: the product's name, from `pcidevs_data.h` under `PCIVERBOSE`.
pub fn pci_findproduct(_id_reg: Pcireg) -> Option<&'static [u8]> {
    // PCIVERBOSE: walk pci_known_products[] for the vendor and PCI_PRODUCT(id_reg).
    None
}

/// `strlen(cp)` within the buffer.
fn cstrlen(cp: &[u8]) -> usize {
    cp.iter().position(|&c| c == 0).unwrap_or(cp.len())
}

/// `snprintf(cp + strlen(cp), ...)`: appends to the NUL-terminated string in `cp`.
fn append(cp: &mut [u8], args: core::fmt::Arguments<'_>) {
    let len = cstrlen(cp);
    if len < cp.len() {
        snprintf(&mut cp[len..], args);
    }
}

/// `pci_devinfo`: describes a device for its attach line, from its ID and class registers,
/// with the class when `showclass` (and no product name) is set.
pub fn pci_devinfo(id_reg: Pcireg, class_reg: Pcireg, showclass: bool, cp: &mut [u8]) {
    let unmatched: &[u8] = if PCIVERBOSE { b"unknown " } else { b"" };

    let vendor = pci_vendor(id_reg);
    let product = pci_product(id_reg);

    let class = pci_class(class_reg);
    let subclass = pci_subclass(class_reg);
    let _interface = pci_interface(class_reg);
    let revision = pci_revision(class_reg);

    let mut vendor_namep = None;
    let mut product_namep = None;
    if PCIVERBOSE {
        vendor_namep = pci_findvendor(id_reg);
        if vendor_namep.is_some() {
            product_namep = pci_findproduct(id_reg);
        }
    }

    let classp = PCI_CLASS.iter().find(|c| c.val == class);
    let subclassp = classp
        .and_then(|c| c.subclasses)
        .and_then(|s| s.iter().find(|sc| sc.val == subclass));

    match (vendor_namep, product_namep) {
        (None, _) => {
            snprintf(
                cp,
                format_args!(
                    "{}vendor 0x{:04x} product 0x{:04x}",
                    Str(unmatched),
                    vendor,
                    product
                ),
            );
        }
        (Some(v), Some(p)) => {
            snprintf(cp, format_args!("\"{} {}\"", Str(v), Str(p)));
        }
        (Some(v), None) => {
            snprintf(
                cp,
                format_args!("vendor \"{}\", unknown product 0x{:04x}", Str(v), product),
            );
        }
    }
    if showclass && product_namep.is_none() {
        append(cp, format_args!(" ("));
        match (classp, subclassp) {
            (None, _) => append(
                cp,
                format_args!("unknown class 0x{:02x}, subclass 0x{:02x}", class, subclass),
            ),
            (Some(c), None) => append(
                cp,
                format_args!("class {} unknown subclass 0x{:02x}", Str(c.name), subclass),
            ),
            (Some(c), Some(s)) => append(
                cp,
                format_args!("class {} subclass {}", Str(c.name), Str(s.name)),
            ),
        }
        // not very useful: ", interface 0x%02x"
        append(cp, format_args!(", rev 0x{:02x})", revision));
    } else {
        append(cp, format_args!(" rev 0x{:02x}", revision));
    }
}
/* </CODE> */

/* <TESTS> */
#[cfg(test)]
mod tests {
    use super::*;

    fn devinfo(id: Pcireg, class: Pcireg, showclass: bool) -> std::string::String {
        let mut buf = [0u8; 256];
        pci_devinfo(id, class, showclass, &mut buf);
        let n = cstrlen(&buf);
        std::string::String::from_utf8_lossy(&buf[..n]).into_owned()
    }

    #[test]
    fn devinfo_without_pciverbose() {
        // QEMU q35's host bridge: Intel 0x29c0, class bridge/host, revision 0.
        let id = pci_id_code(0x8086, 0x29c0);
        let class = (PCI_CLASS_BRIDGE << PCI_CLASS_SHIFT) | (PCI_SUBCLASS_BRIDGE_HOST << 16);
        assert_eq!(
            devinfo(id, class, true),
            "vendor 0x8086 product 0x29c0 (class bridge subclass host, rev 0x00)"
        );
        assert_eq!(
            devinfo(id, class | 2, false),
            "vendor 0x8086 product 0x29c0 rev 0x02"
        );
        // An unknown subclass and an unknown class.
        let class = (PCI_CLASS_BRIDGE << PCI_CLASS_SHIFT) | (0x42 << 16) | 1;
        assert_eq!(
            devinfo(id, class, true),
            "vendor 0x8086 product 0x29c0 (class bridge unknown subclass 0x42, rev 0x01)"
        );
        let class = (0x77 << PCI_CLASS_SHIFT) | (0x05 << 16);
        assert_eq!(
            devinfo(id, class, true),
            "vendor 0x8086 product 0x29c0 (unknown class 0x77, subclass 0x05, rev 0x00)"
        );
        // A class without a subclass table.
        let class = PCI_CLASS_ACCELERATOR << PCI_CLASS_SHIFT;
        assert_eq!(
            devinfo(id, class, true),
            "vendor 0x8086 product 0x29c0 (class accelerator unknown subclass 0x00, rev 0x00)"
        );
    }
}
/* </TESTS> */
