/*	$OpenBSD: pcireg.h,v 1.64 2026/04/07 08:20:40 kettenis Exp $	*/
/*	$NetBSD: pcireg.h,v 1.26 2000/05/10 16:58:42 thorpej Exp $	*/
/* <LICENSES> */
/*
 * Copyright (c) 1995, 1996 Christopher G. Demetriou.  All rights reserved.
 * Copyright (c) 1994, 1996 Charles Hannum.  All rights reserved.
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

//! `<dev/pci/pcireg.h>`: standardized PCI configuration information (XXX this is not
//! complete).
//!
//! Upstream: sys/dev/pci/pcireg.h @ 3ce1f3f79392
//!
//! The configuration space layout: the identification, command/status, class and BIST/header
//! registers, the base address registers (BARs) and how their size is decoded, the
//! capability list with the MSI, MSI-X, power management, HyperTransport and PCI Express
//! capabilities, the interrupt register and the Vital Product Data resource tags.
//!
//! ## Deviations
//! - Function-like macros are `const fn`s with the lower-case name (`PCI_VENDOR(id)` is
//!   [`pci_vendor`]), per `docs/C_TO_RUST.md`; `PCI_MAPREG_TYPE` keeps its capitals because
//!   `pci_map.c` defines a function `pci_mapreg_type`.
//! - Register offsets are `i32` (the C passes them as `int reg`), capability IDs `i32` (the
//!   C's `int capid`); register bits, masks and class codes are `u32`, like `pcireg_t`; the
//!   two 64-bit masks are `u64`. The `PCI_MSIX_*(i)` table offsets are `usize`, as
//!   `bus_space` offsets are.
//! - `PCI_COMMAND_STATUS_BITS` is a byte string with hexadecimal escapes in place of the C's
//!   octal ones (`%b` for `Bitmask`).

/// `PCI_CONFIG_SPACE_SIZE`.
pub const PCI_CONFIG_SPACE_SIZE: i32 = 0x100;

/// `PCIE_CONFIG_SPACE_SIZE`.
pub const PCIE_CONFIG_SPACE_SIZE: i32 = 0x1000;

/// `PCI_ID_REG`.
pub const PCI_ID_REG: i32 = 0x00;

/// `pci_vendor_id_t`.
pub type PciVendorId = u16;
/// `pci_product_id_t`.
pub type PciProductId = u16;

/// `PCI_VENDOR_SHIFT`.
pub const PCI_VENDOR_SHIFT: u32 = 0;

/// `PCI_VENDOR_MASK`.
pub const PCI_VENDOR_MASK: u32 = 0xffff;

/// `PCI_VENDOR(id)`: the vendor ID of a `PCI_ID_REG` value.
pub const fn pci_vendor(id: u32) -> u32 {
    (id >> PCI_VENDOR_SHIFT) & PCI_VENDOR_MASK
}

/// `PCI_PRODUCT_SHIFT`.
pub const PCI_PRODUCT_SHIFT: u32 = 16;

/// `PCI_PRODUCT_MASK`.
pub const PCI_PRODUCT_MASK: u32 = 0xffff;

/// `PCI_PRODUCT(id)`: the product ID of a `PCI_ID_REG` value.
pub const fn pci_product(id: u32) -> u32 {
    (id >> PCI_PRODUCT_SHIFT) & PCI_PRODUCT_MASK
}

/// `PCI_ID_CODE(vid, pid)`: the `PCI_ID_REG` value of a vendor and product.
pub const fn pci_id_code(vid: u32, pid: u32) -> u32 {
    ((vid & PCI_VENDOR_MASK) << PCI_VENDOR_SHIFT) | ((pid & PCI_PRODUCT_MASK) << PCI_PRODUCT_SHIFT)
}

/// `PCI_COMMAND_STATUS_REG`.
pub const PCI_COMMAND_STATUS_REG: i32 = 0x04;

/// `PCI_COMMAND_IO_ENABLE`.
pub const PCI_COMMAND_IO_ENABLE: u32 = 0x00000001;

/// `PCI_COMMAND_MEM_ENABLE`.
pub const PCI_COMMAND_MEM_ENABLE: u32 = 0x00000002;

/// `PCI_COMMAND_MASTER_ENABLE`.
pub const PCI_COMMAND_MASTER_ENABLE: u32 = 0x00000004;

/// `PCI_COMMAND_SPECIAL_ENABLE`.
pub const PCI_COMMAND_SPECIAL_ENABLE: u32 = 0x00000008;

/// `PCI_COMMAND_INVALIDATE_ENABLE`.
pub const PCI_COMMAND_INVALIDATE_ENABLE: u32 = 0x00000010;

/// `PCI_COMMAND_PALETTE_ENABLE`.
pub const PCI_COMMAND_PALETTE_ENABLE: u32 = 0x00000020;

/// `PCI_COMMAND_PARITY_ENABLE`.
pub const PCI_COMMAND_PARITY_ENABLE: u32 = 0x00000040;

/// `PCI_COMMAND_STEPPING_ENABLE`.
pub const PCI_COMMAND_STEPPING_ENABLE: u32 = 0x00000080;

/// `PCI_COMMAND_SERR_ENABLE`.
pub const PCI_COMMAND_SERR_ENABLE: u32 = 0x00000100;

/// `PCI_COMMAND_BACKTOBACK_ENABLE`.
pub const PCI_COMMAND_BACKTOBACK_ENABLE: u32 = 0x00000200;

/// `PCI_COMMAND_INTERRUPT_DISABLE`.
pub const PCI_COMMAND_INTERRUPT_DISABLE: u32 = 0x00000400;

/// `PCI_STATUS_CAPLIST_SUPPORT`.
pub const PCI_STATUS_CAPLIST_SUPPORT: u32 = 0x00100000;

/// `PCI_STATUS_66MHZ_SUPPORT`.
pub const PCI_STATUS_66MHZ_SUPPORT: u32 = 0x00200000;

/// `PCI_STATUS_UDF_SUPPORT`.
pub const PCI_STATUS_UDF_SUPPORT: u32 = 0x00400000;

/// `PCI_STATUS_BACKTOBACK_SUPPORT`.
pub const PCI_STATUS_BACKTOBACK_SUPPORT: u32 = 0x00800000;

/// `PCI_STATUS_PARITY_ERROR`.
pub const PCI_STATUS_PARITY_ERROR: u32 = 0x01000000;

/// `PCI_STATUS_DEVSEL_FAST`.
pub const PCI_STATUS_DEVSEL_FAST: u32 = 0x00000000;

/// `PCI_STATUS_DEVSEL_MEDIUM`.
pub const PCI_STATUS_DEVSEL_MEDIUM: u32 = 0x02000000;

/// `PCI_STATUS_DEVSEL_SLOW`.
pub const PCI_STATUS_DEVSEL_SLOW: u32 = 0x04000000;

/// `PCI_STATUS_DEVSEL_MASK`.
pub const PCI_STATUS_DEVSEL_MASK: u32 = 0x06000000;

/// `PCI_STATUS_TARGET_TARGET_ABORT`.
pub const PCI_STATUS_TARGET_TARGET_ABORT: u32 = 0x08000000;

/// `PCI_STATUS_MASTER_TARGET_ABORT`.
pub const PCI_STATUS_MASTER_TARGET_ABORT: u32 = 0x10000000;

/// `PCI_STATUS_MASTER_ABORT`.
pub const PCI_STATUS_MASTER_ABORT: u32 = 0x20000000;

/// `PCI_STATUS_SPECIAL_ERROR`.
pub const PCI_STATUS_SPECIAL_ERROR: u32 = 0x40000000;

/// `PCI_STATUS_PARITY_DETECT`.
pub const PCI_STATUS_PARITY_DETECT: u32 = 0x80000000;

/// `PCI_COMMAND_STATUS_BITS`: the command and status bits for `printf`'s `%b`.
pub const PCI_COMMAND_STATUS_BITS: &[u8] = b"\x10\x01IO\x02MEM\x03MASTER\x04SPECIAL\x05INVALIDATE\
\x06PALETTE\x07PARITY\x08STEPPING\x09SERR\x0aBACKTOBACK\x15CAPLIST\x16CLK66\x17UDF\
\x18BACK2BACK_STAT\x19PARITY_STAT\x1aDEVSEL_MEDIUM\x1bDEVSEL_SLOW\x1cTARGET_TARGET_ABORT\
\x1dMASTER_TARGET_ABORT\x1eMASTER_ABORT\x1fSPECIAL_ERROR\x20PARITY_DETECT";

/// `PCI_CLASS_REG`.
pub const PCI_CLASS_REG: i32 = 0x08;

/// `pci_class_t`.
pub type PciClass = u8;
/// `pci_subclass_t`.
pub type PciSubclass = u8;
/// `pci_interface_t`.
pub type PciInterface = u8;
/// `pci_revision_t`.
pub type PciRevision = u8;

/// `PCI_CLASS_SHIFT`.
pub const PCI_CLASS_SHIFT: u32 = 24;

/// `PCI_CLASS_MASK`.
pub const PCI_CLASS_MASK: u32 = 0xff;

/// `PCI_CLASS(cr)`: the base class of a `PCI_CLASS_REG` value.
pub const fn pci_class(cr: u32) -> u32 {
    (cr >> PCI_CLASS_SHIFT) & PCI_CLASS_MASK
}

/// `PCI_SUBCLASS_SHIFT`.
pub const PCI_SUBCLASS_SHIFT: u32 = 16;

/// `PCI_SUBCLASS_MASK`.
pub const PCI_SUBCLASS_MASK: u32 = 0xff;

/// `PCI_SUBCLASS(cr)`.
pub const fn pci_subclass(cr: u32) -> u32 {
    (cr >> PCI_SUBCLASS_SHIFT) & PCI_SUBCLASS_MASK
}

/// `PCI_INTERFACE_SHIFT`.
pub const PCI_INTERFACE_SHIFT: u32 = 8;

/// `PCI_INTERFACE_MASK`.
pub const PCI_INTERFACE_MASK: u32 = 0xff;

/// `PCI_INTERFACE(cr)`.
pub const fn pci_interface(cr: u32) -> u32 {
    (cr >> PCI_INTERFACE_SHIFT) & PCI_INTERFACE_MASK
}

/// `PCI_REVISION_SHIFT`.
pub const PCI_REVISION_SHIFT: u32 = 0;

/// `PCI_REVISION_MASK`.
pub const PCI_REVISION_MASK: u32 = 0xff;

/// `PCI_REVISION(cr)`.
pub const fn pci_revision(cr: u32) -> u32 {
    (cr >> PCI_REVISION_SHIFT) & PCI_REVISION_MASK
}

/// `PCI_CLASS_PREHISTORIC`.
pub const PCI_CLASS_PREHISTORIC: u32 = 0x00;

/// `PCI_CLASS_MASS_STORAGE`.
pub const PCI_CLASS_MASS_STORAGE: u32 = 0x01;

/// `PCI_CLASS_NETWORK`.
pub const PCI_CLASS_NETWORK: u32 = 0x02;

/// `PCI_CLASS_DISPLAY`.
pub const PCI_CLASS_DISPLAY: u32 = 0x03;

/// `PCI_CLASS_MULTIMEDIA`.
pub const PCI_CLASS_MULTIMEDIA: u32 = 0x04;

/// `PCI_CLASS_MEMORY`.
pub const PCI_CLASS_MEMORY: u32 = 0x05;

/// `PCI_CLASS_BRIDGE`.
pub const PCI_CLASS_BRIDGE: u32 = 0x06;

/// `PCI_CLASS_COMMUNICATIONS`.
pub const PCI_CLASS_COMMUNICATIONS: u32 = 0x07;

/// `PCI_CLASS_SYSTEM`.
pub const PCI_CLASS_SYSTEM: u32 = 0x08;

/// `PCI_CLASS_INPUT`.
pub const PCI_CLASS_INPUT: u32 = 0x09;

/// `PCI_CLASS_DOCK`.
pub const PCI_CLASS_DOCK: u32 = 0x0a;

/// `PCI_CLASS_PROCESSOR`.
pub const PCI_CLASS_PROCESSOR: u32 = 0x0b;

/// `PCI_CLASS_SERIALBUS`.
pub const PCI_CLASS_SERIALBUS: u32 = 0x0c;

/// `PCI_CLASS_WIRELESS`.
pub const PCI_CLASS_WIRELESS: u32 = 0x0d;

/// `PCI_CLASS_I2O`.
pub const PCI_CLASS_I2O: u32 = 0x0e;

/// `PCI_CLASS_SATCOM`.
pub const PCI_CLASS_SATCOM: u32 = 0x0f;

/// `PCI_CLASS_CRYPTO`.
pub const PCI_CLASS_CRYPTO: u32 = 0x10;

/// `PCI_CLASS_DASP`.
pub const PCI_CLASS_DASP: u32 = 0x11;

/// `PCI_CLASS_ACCELERATOR`.
pub const PCI_CLASS_ACCELERATOR: u32 = 0x12;

/// `PCI_CLASS_INSTRUMENTATION`.
pub const PCI_CLASS_INSTRUMENTATION: u32 = 0x13;

/// `PCI_CLASS_UNDEFINED`.
pub const PCI_CLASS_UNDEFINED: u32 = 0xff;

/// `PCI_SUBCLASS_PREHISTORIC_MISC`.
pub const PCI_SUBCLASS_PREHISTORIC_MISC: u32 = 0x00;

/// `PCI_SUBCLASS_PREHISTORIC_VGA`.
pub const PCI_SUBCLASS_PREHISTORIC_VGA: u32 = 0x01;

/// `PCI_SUBCLASS_MASS_STORAGE_SCSI`.
pub const PCI_SUBCLASS_MASS_STORAGE_SCSI: u32 = 0x00;

/// `PCI_SUBCLASS_MASS_STORAGE_IDE`.
pub const PCI_SUBCLASS_MASS_STORAGE_IDE: u32 = 0x01;

/// `PCI_SUBCLASS_MASS_STORAGE_FLOPPY`.
pub const PCI_SUBCLASS_MASS_STORAGE_FLOPPY: u32 = 0x02;

/// `PCI_SUBCLASS_MASS_STORAGE_IPI`.
pub const PCI_SUBCLASS_MASS_STORAGE_IPI: u32 = 0x03;

/// `PCI_SUBCLASS_MASS_STORAGE_RAID`.
pub const PCI_SUBCLASS_MASS_STORAGE_RAID: u32 = 0x04;

/// `PCI_SUBCLASS_MASS_STORAGE_ATA`.
pub const PCI_SUBCLASS_MASS_STORAGE_ATA: u32 = 0x05;

/// `PCI_SUBCLASS_MASS_STORAGE_SATA`.
pub const PCI_SUBCLASS_MASS_STORAGE_SATA: u32 = 0x06;

/// `PCI_INTERFACE_SATA_AHCI10`.
pub const PCI_INTERFACE_SATA_AHCI10: u32 = 0x01;

/// `PCI_SUBCLASS_MASS_STORAGE_SAS`.
pub const PCI_SUBCLASS_MASS_STORAGE_SAS: u32 = 0x07;

/// `PCI_SUBCLASS_MASS_STORAGE_NVM`.
pub const PCI_SUBCLASS_MASS_STORAGE_NVM: u32 = 0x08;

/// `PCI_SUBCLASS_MASS_STORAGE_UFS`.
pub const PCI_SUBCLASS_MASS_STORAGE_UFS: u32 = 0x09;

/// `PCI_SUBCLASS_MASS_STORAGE_MISC`.
pub const PCI_SUBCLASS_MASS_STORAGE_MISC: u32 = 0x80;

/// `PCI_SUBCLASS_NETWORK_ETHERNET`.
pub const PCI_SUBCLASS_NETWORK_ETHERNET: u32 = 0x00;

/// `PCI_SUBCLASS_NETWORK_TOKENRING`.
pub const PCI_SUBCLASS_NETWORK_TOKENRING: u32 = 0x01;

/// `PCI_SUBCLASS_NETWORK_FDDI`.
pub const PCI_SUBCLASS_NETWORK_FDDI: u32 = 0x02;

/// `PCI_SUBCLASS_NETWORK_ATM`.
pub const PCI_SUBCLASS_NETWORK_ATM: u32 = 0x03;

/// `PCI_SUBCLASS_NETWORK_ISDN`.
pub const PCI_SUBCLASS_NETWORK_ISDN: u32 = 0x04;

/// `PCI_SUBCLASS_NETWORK_WORLDFIP`.
pub const PCI_SUBCLASS_NETWORK_WORLDFIP: u32 = 0x05;

/// `PCI_SUBCLASS_NETWORK_PCIMGMULTICOMP`.
pub const PCI_SUBCLASS_NETWORK_PCIMGMULTICOMP: u32 = 0x06;

/// `PCI_SUBCLASS_NETWORK_INFINIBAND`.
pub const PCI_SUBCLASS_NETWORK_INFINIBAND: u32 = 0x07;

/// `PCI_SUBCLASS_NETWORK_MISC`.
pub const PCI_SUBCLASS_NETWORK_MISC: u32 = 0x80;

/// `PCI_SUBCLASS_DISPLAY_VGA`.
pub const PCI_SUBCLASS_DISPLAY_VGA: u32 = 0x00;

/// `PCI_SUBCLASS_DISPLAY_XGA`.
pub const PCI_SUBCLASS_DISPLAY_XGA: u32 = 0x01;

/// `PCI_SUBCLASS_DISPLAY_3D`.
pub const PCI_SUBCLASS_DISPLAY_3D: u32 = 0x02;

/// `PCI_SUBCLASS_DISPLAY_MISC`.
pub const PCI_SUBCLASS_DISPLAY_MISC: u32 = 0x80;

/// `PCI_SUBCLASS_MULTIMEDIA_VIDEO`.
pub const PCI_SUBCLASS_MULTIMEDIA_VIDEO: u32 = 0x00;

/// `PCI_SUBCLASS_MULTIMEDIA_AUDIO`.
pub const PCI_SUBCLASS_MULTIMEDIA_AUDIO: u32 = 0x01;

/// `PCI_SUBCLASS_MULTIMEDIA_TELEPHONY`.
pub const PCI_SUBCLASS_MULTIMEDIA_TELEPHONY: u32 = 0x02;

/// `PCI_SUBCLASS_MULTIMEDIA_HDAUDIO`.
pub const PCI_SUBCLASS_MULTIMEDIA_HDAUDIO: u32 = 0x03;

/// `PCI_SUBCLASS_MULTIMEDIA_MISC`.
pub const PCI_SUBCLASS_MULTIMEDIA_MISC: u32 = 0x80;

/// `PCI_SUBCLASS_MEMORY_RAM`.
pub const PCI_SUBCLASS_MEMORY_RAM: u32 = 0x00;

/// `PCI_SUBCLASS_MEMORY_FLASH`.
pub const PCI_SUBCLASS_MEMORY_FLASH: u32 = 0x01;

/// `PCI_SUBCLASS_MEMORY_MISC`.
pub const PCI_SUBCLASS_MEMORY_MISC: u32 = 0x80;

/// `PCI_SUBCLASS_BRIDGE_HOST`.
pub const PCI_SUBCLASS_BRIDGE_HOST: u32 = 0x00;

/// `PCI_SUBCLASS_BRIDGE_ISA`.
pub const PCI_SUBCLASS_BRIDGE_ISA: u32 = 0x01;

/// `PCI_SUBCLASS_BRIDGE_EISA`.
pub const PCI_SUBCLASS_BRIDGE_EISA: u32 = 0x02;

/// `PCI_SUBCLASS_BRIDGE_MC`.
pub const PCI_SUBCLASS_BRIDGE_MC: u32 = 0x03;

/// `PCI_SUBCLASS_BRIDGE_PCI`.
pub const PCI_SUBCLASS_BRIDGE_PCI: u32 = 0x04;

/// `PCI_SUBCLASS_BRIDGE_PCMCIA`.
pub const PCI_SUBCLASS_BRIDGE_PCMCIA: u32 = 0x05;

/// `PCI_SUBCLASS_BRIDGE_NUBUS`.
pub const PCI_SUBCLASS_BRIDGE_NUBUS: u32 = 0x06;

/// `PCI_SUBCLASS_BRIDGE_CARDBUS`.
pub const PCI_SUBCLASS_BRIDGE_CARDBUS: u32 = 0x07;

/// `PCI_SUBCLASS_BRIDGE_RACEWAY`.
pub const PCI_SUBCLASS_BRIDGE_RACEWAY: u32 = 0x08;

/// `PCI_SUBCLASS_BRIDGE_STPCI`.
pub const PCI_SUBCLASS_BRIDGE_STPCI: u32 = 0x09;

/// `PCI_SUBCLASS_BRIDGE_INFINIBAND`.
pub const PCI_SUBCLASS_BRIDGE_INFINIBAND: u32 = 0x0a;

/// `PCI_SUBCLASS_BRIDGE_AS`.
pub const PCI_SUBCLASS_BRIDGE_AS: u32 = 0x0b;

/// `PCI_SUBCLASS_BRIDGE_MISC`.
pub const PCI_SUBCLASS_BRIDGE_MISC: u32 = 0x80;

/// `PCI_SUBCLASS_COMMUNICATIONS_SERIAL`.
pub const PCI_SUBCLASS_COMMUNICATIONS_SERIAL: u32 = 0x00;

/// `PCI_SUBCLASS_COMMUNICATIONS_PARALLEL`.
pub const PCI_SUBCLASS_COMMUNICATIONS_PARALLEL: u32 = 0x01;

/// `PCI_SUBCLASS_COMMUNICATIONS_MPSERIAL`.
pub const PCI_SUBCLASS_COMMUNICATIONS_MPSERIAL: u32 = 0x02;

/// `PCI_SUBCLASS_COMMUNICATIONS_MODEM`.
pub const PCI_SUBCLASS_COMMUNICATIONS_MODEM: u32 = 0x03;

/// `PCI_SUBCLASS_COMMUNICATIONS_GPIB`.
pub const PCI_SUBCLASS_COMMUNICATIONS_GPIB: u32 = 0x04;

/// `PCI_SUBCLASS_COMMUNICATIONS_SMARTCARD`.
pub const PCI_SUBCLASS_COMMUNICATIONS_SMARTCARD: u32 = 0x05;

/// `PCI_SUBCLASS_COMMUNICATIONS_MISC`.
pub const PCI_SUBCLASS_COMMUNICATIONS_MISC: u32 = 0x80;

/// `PCI_SUBCLASS_SYSTEM_PIC`.
pub const PCI_SUBCLASS_SYSTEM_PIC: u32 = 0x00;

/// `PCI_SUBCLASS_SYSTEM_DMA`.
pub const PCI_SUBCLASS_SYSTEM_DMA: u32 = 0x01;

/// `PCI_SUBCLASS_SYSTEM_TIMER`.
pub const PCI_SUBCLASS_SYSTEM_TIMER: u32 = 0x02;

/// `PCI_SUBCLASS_SYSTEM_RTC`.
pub const PCI_SUBCLASS_SYSTEM_RTC: u32 = 0x03;

/// `PCI_SUBCLASS_SYSTEM_PCIHOTPLUG`.
pub const PCI_SUBCLASS_SYSTEM_PCIHOTPLUG: u32 = 0x04;

/// `PCI_SUBCLASS_SYSTEM_SDHC`.
pub const PCI_SUBCLASS_SYSTEM_SDHC: u32 = 0x05;

/// `PCI_SUBCLASS_SYSTEM_IOMMU`.
pub const PCI_SUBCLASS_SYSTEM_IOMMU: u32 = 0x06;

/// `PCI_SUBCLASS_SYSTEM_ROOTCOMPEVENT`.
pub const PCI_SUBCLASS_SYSTEM_ROOTCOMPEVENT: u32 = 0x07;

/// `PCI_SUBCLASS_SYSTEM_MISC`.
pub const PCI_SUBCLASS_SYSTEM_MISC: u32 = 0x80;

/// `PCI_SUBCLASS_INPUT_KEYBOARD`.
pub const PCI_SUBCLASS_INPUT_KEYBOARD: u32 = 0x00;

/// `PCI_SUBCLASS_INPUT_DIGITIZER`.
pub const PCI_SUBCLASS_INPUT_DIGITIZER: u32 = 0x01;

/// `PCI_SUBCLASS_INPUT_MOUSE`.
pub const PCI_SUBCLASS_INPUT_MOUSE: u32 = 0x02;

/// `PCI_SUBCLASS_INPUT_SCANNER`.
pub const PCI_SUBCLASS_INPUT_SCANNER: u32 = 0x03;

/// `PCI_SUBCLASS_INPUT_GAMEPORT`.
pub const PCI_SUBCLASS_INPUT_GAMEPORT: u32 = 0x04;

/// `PCI_SUBCLASS_INPUT_MISC`.
pub const PCI_SUBCLASS_INPUT_MISC: u32 = 0x80;

/// `PCI_SUBCLASS_DOCK_GENERIC`.
pub const PCI_SUBCLASS_DOCK_GENERIC: u32 = 0x00;

/// `PCI_SUBCLASS_DOCK_MISC`.
pub const PCI_SUBCLASS_DOCK_MISC: u32 = 0x80;

/// `PCI_SUBCLASS_PROCESSOR_386`.
pub const PCI_SUBCLASS_PROCESSOR_386: u32 = 0x00;

/// `PCI_SUBCLASS_PROCESSOR_486`.
pub const PCI_SUBCLASS_PROCESSOR_486: u32 = 0x01;

/// `PCI_SUBCLASS_PROCESSOR_PENTIUM`.
pub const PCI_SUBCLASS_PROCESSOR_PENTIUM: u32 = 0x02;

/// `PCI_SUBCLASS_PROCESSOR_ALPHA`.
pub const PCI_SUBCLASS_PROCESSOR_ALPHA: u32 = 0x10;

/// `PCI_SUBCLASS_PROCESSOR_POWERPC`.
pub const PCI_SUBCLASS_PROCESSOR_POWERPC: u32 = 0x20;

/// `PCI_SUBCLASS_PROCESSOR_MIPS`.
pub const PCI_SUBCLASS_PROCESSOR_MIPS: u32 = 0x30;

/// `PCI_SUBCLASS_PROCESSOR_COPROC`.
pub const PCI_SUBCLASS_PROCESSOR_COPROC: u32 = 0x40;

/// `PCI_SUBCLASS_SERIALBUS_FIREWIRE`.
pub const PCI_SUBCLASS_SERIALBUS_FIREWIRE: u32 = 0x00;

/// `PCI_SUBCLASS_SERIALBUS_ACCESS`.
pub const PCI_SUBCLASS_SERIALBUS_ACCESS: u32 = 0x01;

/// `PCI_SUBCLASS_SERIALBUS_SSA`.
pub const PCI_SUBCLASS_SERIALBUS_SSA: u32 = 0x02;

/// `PCI_SUBCLASS_SERIALBUS_USB`.
pub const PCI_SUBCLASS_SERIALBUS_USB: u32 = 0x03;

/// `PCI_SUBCLASS_SERIALBUS_FIBER`.
pub const PCI_SUBCLASS_SERIALBUS_FIBER: u32 = 0x04;

/// `PCI_SUBCLASS_SERIALBUS_SMBUS`.
pub const PCI_SUBCLASS_SERIALBUS_SMBUS: u32 = 0x05;

/// `PCI_SUBCLASS_SERIALBUS_INFINIBAND`.
pub const PCI_SUBCLASS_SERIALBUS_INFINIBAND: u32 = 0x06;

/// `PCI_SUBCLASS_SERIALBUS_IPMI`.
pub const PCI_SUBCLASS_SERIALBUS_IPMI: u32 = 0x07;

/// `PCI_SUBCLASS_SERIALBUS_SERCOS`.
pub const PCI_SUBCLASS_SERIALBUS_SERCOS: u32 = 0x08;

/// `PCI_SUBCLASS_SERIALBUS_CANBUS`.
pub const PCI_SUBCLASS_SERIALBUS_CANBUS: u32 = 0x09;

/// `PCI_SUBCLASS_WIRELESS_IRDA`.
pub const PCI_SUBCLASS_WIRELESS_IRDA: u32 = 0x00;

/// `PCI_SUBCLASS_WIRELESS_CONSUMERIR`.
pub const PCI_SUBCLASS_WIRELESS_CONSUMERIR: u32 = 0x01;

/// `PCI_SUBCLASS_WIRELESS_RF`.
pub const PCI_SUBCLASS_WIRELESS_RF: u32 = 0x10;

/// `PCI_SUBCLASS_WIRELESS_BLUETOOTH`.
pub const PCI_SUBCLASS_WIRELESS_BLUETOOTH: u32 = 0x11;

/// `PCI_SUBCLASS_WIRELESS_BROADBAND`.
pub const PCI_SUBCLASS_WIRELESS_BROADBAND: u32 = 0x12;

/// `PCI_SUBCLASS_WIRELESS_802_11A`.
pub const PCI_SUBCLASS_WIRELESS_802_11A: u32 = 0x20;

/// `PCI_SUBCLASS_WIRELESS_802_11B`.
pub const PCI_SUBCLASS_WIRELESS_802_11B: u32 = 0x21;

/// `PCI_SUBCLASS_WIRELESS_MISC`.
pub const PCI_SUBCLASS_WIRELESS_MISC: u32 = 0x80;

/// `PCI_SUBCLASS_I2O_STANDARD`.
pub const PCI_SUBCLASS_I2O_STANDARD: u32 = 0x00;

/// `PCI_SUBCLASS_SATCOM_TV`.
pub const PCI_SUBCLASS_SATCOM_TV: u32 = 0x01;

/// `PCI_SUBCLASS_SATCOM_AUDIO`.
pub const PCI_SUBCLASS_SATCOM_AUDIO: u32 = 0x02;

/// `PCI_SUBCLASS_SATCOM_VOICE`.
pub const PCI_SUBCLASS_SATCOM_VOICE: u32 = 0x03;

/// `PCI_SUBCLASS_SATCOM_DATA`.
pub const PCI_SUBCLASS_SATCOM_DATA: u32 = 0x04;

/// `PCI_SUBCLASS_CRYPTO_NETCOMP`.
pub const PCI_SUBCLASS_CRYPTO_NETCOMP: u32 = 0x00;

/// `PCI_SUBCLASS_CRYPTO_ENTERTAINMENT`.
pub const PCI_SUBCLASS_CRYPTO_ENTERTAINMENT: u32 = 0x10;

/// `PCI_SUBCLASS_CRYPTO_MISC`.
pub const PCI_SUBCLASS_CRYPTO_MISC: u32 = 0x80;

/// `PCI_SUBCLASS_DASP_DPIO`.
pub const PCI_SUBCLASS_DASP_DPIO: u32 = 0x00;

/// `PCI_SUBCLASS_DASP_TIMEFREQ`.
pub const PCI_SUBCLASS_DASP_TIMEFREQ: u32 = 0x01;

/// `PCI_SUBCLASS_DASP_SYNC`.
pub const PCI_SUBCLASS_DASP_SYNC: u32 = 0x10;

/// `PCI_SUBCLASS_DASP_MGMT`.
pub const PCI_SUBCLASS_DASP_MGMT: u32 = 0x20;

/// `PCI_SUBCLASS_DASP_MISC`.
pub const PCI_SUBCLASS_DASP_MISC: u32 = 0x80;

/// `PCI_BHLC_REG`.
pub const PCI_BHLC_REG: i32 = 0x0c;

/// `PCI_BIST_SHIFT`.
pub const PCI_BIST_SHIFT: u32 = 24;

/// `PCI_BIST_MASK`.
pub const PCI_BIST_MASK: u32 = 0xff;

/// `PCI_BIST(bhlcr)`.
pub const fn pci_bist(bhlcr: u32) -> u32 {
    (bhlcr >> PCI_BIST_SHIFT) & PCI_BIST_MASK
}

/// `PCI_HDRTYPE_SHIFT`.
pub const PCI_HDRTYPE_SHIFT: u32 = 16;

/// `PCI_HDRTYPE_MASK`.
pub const PCI_HDRTYPE_MASK: u32 = 0xff;

/// `PCI_HDRTYPE(bhlcr)`.
pub const fn pci_hdrtype(bhlcr: u32) -> u32 {
    (bhlcr >> PCI_HDRTYPE_SHIFT) & PCI_HDRTYPE_MASK
}

/// `PCI_HDRTYPE_TYPE(bhlcr)`: the header layout (0 device, 1 PCI-PCI bridge, 2 CardBus).
pub const fn pci_hdrtype_type(bhlcr: u32) -> u32 {
    pci_hdrtype(bhlcr) & 0x7f
}

/// `PCI_HDRTYPE_MULTIFN(bhlcr)`: the device has several functions.
pub const fn pci_hdrtype_multifn(bhlcr: u32) -> bool {
    pci_hdrtype(bhlcr) & 0x80 != 0
}

/// `PCI_LATTIMER_SHIFT`.
pub const PCI_LATTIMER_SHIFT: u32 = 8;

/// `PCI_LATTIMER_MASK`.
pub const PCI_LATTIMER_MASK: u32 = 0xff;

/// `PCI_LATTIMER(bhlcr)`.
pub const fn pci_lattimer(bhlcr: u32) -> u32 {
    (bhlcr >> PCI_LATTIMER_SHIFT) & PCI_LATTIMER_MASK
}

/// `PCI_CACHELINE_SHIFT`.
pub const PCI_CACHELINE_SHIFT: u32 = 0;

/// `PCI_CACHELINE_MASK`.
pub const PCI_CACHELINE_MASK: u32 = 0xff;

/// `PCI_CACHELINE(bhlcr)`.
pub const fn pci_cacheline(bhlcr: u32) -> u32 {
    (bhlcr >> PCI_CACHELINE_SHIFT) & PCI_CACHELINE_MASK
}

/// `PCI_MAPS`.
pub const PCI_MAPS: i32 = 0x10;

/// `PCI_CARDBUSCIS`.
pub const PCI_CARDBUSCIS: i32 = 0x28;

/// `PCI_SUBVEND_0`.
pub const PCI_SUBVEND_0: i32 = 0x2c;

/// `PCI_SUBDEV_0`.
pub const PCI_SUBDEV_0: i32 = 0x2e;

/// `PCI_EXROMADDR_0`.
pub const PCI_EXROMADDR_0: i32 = 0x30;

/// `PCI_INTLINE`.
pub const PCI_INTLINE: i32 = 0x3c;

/// `PCI_INTPIN`.
pub const PCI_INTPIN: i32 = 0x3d;

/// `PCI_MINGNT`.
pub const PCI_MINGNT: i32 = 0x3e;

/// `PCI_MAXLAT`.
pub const PCI_MAXLAT: i32 = 0x3f;

/// `PCI_SECSTAT_1`.
pub const PCI_SECSTAT_1: i32 = 0;

/// `PCI_PRIBUS_1`.
pub const PCI_PRIBUS_1: i32 = 0x18;

/// `PCI_SECBUS_1`.
pub const PCI_SECBUS_1: i32 = 0x19;

/// `PCI_SUBBUS_1`.
pub const PCI_SUBBUS_1: i32 = 0x1a;

/// `PCI_SECLAT_1`.
pub const PCI_SECLAT_1: i32 = 0x1b;

/// `PCI_IOBASEL_1`.
pub const PCI_IOBASEL_1: i32 = 0x1c;

/// `PCI_IOLIMITL_1`.
pub const PCI_IOLIMITL_1: i32 = 0x1d;

/// `PCI_IOBASEH_1`.
pub const PCI_IOBASEH_1: i32 = 0;

/// `PCI_IOLIMITH_1`.
pub const PCI_IOLIMITH_1: i32 = 0;

/// `PCI_MEMBASE_1`.
pub const PCI_MEMBASE_1: i32 = 0x20;

/// `PCI_MEMLIMIT_1`.
pub const PCI_MEMLIMIT_1: i32 = 0x22;

/// `PCI_PMBASEL_1`.
pub const PCI_PMBASEL_1: i32 = 0x24;

/// `PCI_PMLIMITL_1`.
pub const PCI_PMLIMITL_1: i32 = 0x26;

/// `PCI_PMBASEH_1`.
pub const PCI_PMBASEH_1: i32 = 0;

/// `PCI_PMLIMITH_1`.
pub const PCI_PMLIMITH_1: i32 = 0;

/// `PCI_BRIDGECTL_1`.
pub const PCI_BRIDGECTL_1: i32 = 0;

/// `PCI_SUBVEND_1`.
pub const PCI_SUBVEND_1: i32 = 0x34;

/// `PCI_SUBDEV_1`.
pub const PCI_SUBDEV_1: i32 = 0x36;

/// `PCI_EXROMADDR_1`.
pub const PCI_EXROMADDR_1: i32 = 0x38;

/// `PCI_SECSTAT_2`.
pub const PCI_SECSTAT_2: i32 = 0x16;

/// `PCI_PRIBUS_2`.
pub const PCI_PRIBUS_2: i32 = 0x18;

/// `PCI_SECBUS_2`.
pub const PCI_SECBUS_2: i32 = 0x19;

/// `PCI_SUBBUS_2`.
pub const PCI_SUBBUS_2: i32 = 0x1a;

/// `PCI_SECLAT_2`.
pub const PCI_SECLAT_2: i32 = 0x1b;

/// `PCI_MEMBASE0_2`.
pub const PCI_MEMBASE0_2: i32 = 0x1c;

/// `PCI_MEMLIMIT0_2`.
pub const PCI_MEMLIMIT0_2: i32 = 0x20;

/// `PCI_MEMBASE1_2`.
pub const PCI_MEMBASE1_2: i32 = 0x24;

/// `PCI_MEMLIMIT1_2`.
pub const PCI_MEMLIMIT1_2: i32 = 0x28;

/// `PCI_IOBASE0_2`.
pub const PCI_IOBASE0_2: i32 = 0x2c;

/// `PCI_IOLIMIT0_2`.
pub const PCI_IOLIMIT0_2: i32 = 0x30;

/// `PCI_IOBASE1_2`.
pub const PCI_IOBASE1_2: i32 = 0x34;

/// `PCI_IOLIMIT1_2`.
pub const PCI_IOLIMIT1_2: i32 = 0x38;

/// `PCI_BRIDGECTL_2`.
pub const PCI_BRIDGECTL_2: i32 = 0x3e;

/// `PCI_SUBVEND_2`.
pub const PCI_SUBVEND_2: i32 = 0x40;

/// `PCI_SUBDEV_2`.
pub const PCI_SUBDEV_2: i32 = 0x42;

/// `PCI_PCCARDIF_2`.
pub const PCI_PCCARDIF_2: i32 = 0x44;

/// `PCI_MAPREG_START`.
pub const PCI_MAPREG_START: i32 = 0x10;

/// `PCI_MAPREG_END`.
pub const PCI_MAPREG_END: i32 = 0x28;

/// `PCI_MAPREG_PPB_END`.
pub const PCI_MAPREG_PPB_END: i32 = 0x18;

/// `PCI_MAPREG_PCB_END`.
pub const PCI_MAPREG_PCB_END: i32 = 0x14;

/// `PCI_MAPREG_TYPE_MASK`.
pub const PCI_MAPREG_TYPE_MASK: u32 = 0x00000001;

/// `PCI_MAPREG_TYPE(mr)`: I/O or memory.
#[allow(non_snake_case)] // `pci_map.c`'s `pci_mapreg_type()` has the lower-case name
pub const fn PCI_MAPREG_TYPE(mr: u32) -> u32 {
    mr & PCI_MAPREG_TYPE_MASK
}

/// `PCI_MAPREG_TYPE_MEM`.
pub const PCI_MAPREG_TYPE_MEM: u32 = 0x00000000;

/// `PCI_MAPREG_TYPE_IO`.
pub const PCI_MAPREG_TYPE_IO: u32 = 0x00000001;

/// `PCI_MAPREG_MEM_TYPE_MASK`.
pub const PCI_MAPREG_MEM_TYPE_MASK: u32 = 0x00000006;

/// `PCI_MAPREG_MEM_TYPE(mr)`: the width of a memory BAR.
pub const fn pci_mapreg_mem_type(mr: u32) -> u32 {
    mr & PCI_MAPREG_MEM_TYPE_MASK
}

/// `PCI_MAPREG_MEM_TYPE_32BIT`.
pub const PCI_MAPREG_MEM_TYPE_32BIT: u32 = 0x00000000;

/// `PCI_MAPREG_MEM_TYPE_32BIT_1M`.
pub const PCI_MAPREG_MEM_TYPE_32BIT_1M: u32 = 0x00000002;

/// `PCI_MAPREG_MEM_TYPE_64BIT`.
pub const PCI_MAPREG_MEM_TYPE_64BIT: u32 = 0x00000004;

/// `_PCI_MAPREG_TYPEBITS(reg)`: the type bits of a BAR (the memory width included).
pub const fn _pci_mapreg_typebits(reg: u32) -> u32 {
    if PCI_MAPREG_TYPE(reg) == PCI_MAPREG_TYPE_IO {
        reg & PCI_MAPREG_TYPE_MASK
    } else {
        reg & (PCI_MAPREG_TYPE_MASK | PCI_MAPREG_MEM_TYPE_MASK)
    }
}

/// `PCI_MAPREG_MEM_PREFETCHABLE_MASK`.
pub const PCI_MAPREG_MEM_PREFETCHABLE_MASK: u32 = 0x00000008;

/// `PCI_MAPREG_MEM_PREFETCHABLE(mr)`.
pub const fn pci_mapreg_mem_prefetchable(mr: u32) -> bool {
    mr & PCI_MAPREG_MEM_PREFETCHABLE_MASK != 0
}

/// `PCI_MAPREG_MEM_ADDR_MASK`.
pub const PCI_MAPREG_MEM_ADDR_MASK: u32 = 0xfffffff0;

/// `PCI_MAPREG_MEM_ADDR(mr)`.
pub const fn pci_mapreg_mem_addr(mr: u32) -> u32 {
    mr & PCI_MAPREG_MEM_ADDR_MASK
}

/// `PCI_MAPREG_MEM_SIZE(mr)`: the size a memory BAR decodes, from the value read back after
/// writing all ones (the lowest address bit that stuck).
pub const fn pci_mapreg_mem_size(mr: u32) -> u32 {
    pci_mapreg_mem_addr(mr) & pci_mapreg_mem_addr(mr).wrapping_neg()
}

/// `PCI_MAPREG_MEM64_ADDR_MASK`.
pub const PCI_MAPREG_MEM64_ADDR_MASK: u64 = 0xfffffffffffffff0;

/// `PCI_MAPREG_MEM64_ADDR(mr)`.
pub const fn pci_mapreg_mem64_addr(mr: u64) -> u64 {
    mr & PCI_MAPREG_MEM64_ADDR_MASK
}

/// `PCI_MAPREG_MEM64_SIZE(mr)`.
pub const fn pci_mapreg_mem64_size(mr: u64) -> u64 {
    pci_mapreg_mem64_addr(mr) & pci_mapreg_mem64_addr(mr).wrapping_neg()
}

/// `PCI_MAPREG_IO_ADDR_MASK`.
pub const PCI_MAPREG_IO_ADDR_MASK: u32 = 0xfffffffe;

/// `PCI_MAPREG_IO_ADDR(mr)`.
pub const fn pci_mapreg_io_addr(mr: u32) -> u32 {
    mr & PCI_MAPREG_IO_ADDR_MASK
}

/// `PCI_MAPREG_IO_SIZE(mr)`.
pub const fn pci_mapreg_io_size(mr: u32) -> u32 {
    pci_mapreg_io_addr(mr) & pci_mapreg_io_addr(mr).wrapping_neg()
}

/// `PCI_CARDBUS_CIS_REG`.
pub const PCI_CARDBUS_CIS_REG: i32 = 0x28;

/// `PCI_SUBSYS_ID_REG`.
pub const PCI_SUBSYS_ID_REG: i32 = 0x2c;

/// `PCI_ROM_REG`.
pub const PCI_ROM_REG: i32 = 0x30;

/// `PCI_ROM_ENABLE`.
pub const PCI_ROM_ENABLE: u32 = 0x00000001;

/// `PCI_ROM_ADDR_MASK`.
pub const PCI_ROM_ADDR_MASK: u32 = 0xfffff800;

/// `PCI_ROM_ADDR(mr)`.
pub const fn pci_rom_addr(mr: u32) -> u32 {
    mr & PCI_ROM_ADDR_MASK
}

/// `PCI_ROM_SIZE(mr)`.
pub const fn pci_rom_size(mr: u32) -> u32 {
    pci_rom_addr(mr) & pci_rom_addr(mr).wrapping_neg()
}

/// `PCI_CAPLISTPTR_REG`: header type 0
pub const PCI_CAPLISTPTR_REG: i32 = 0x34;

/// `PCI_CARDBUS_CAPLISTPTR_REG`: header type 2
pub const PCI_CARDBUS_CAPLISTPTR_REG: i32 = 0x14;

/// `PCI_CAPLIST_PTR(cpr)`: the first capability's offset.
pub const fn pci_caplist_ptr(cpr: u32) -> u32 {
    cpr & 0xff
}

/// `PCI_CAPLIST_NEXT(cr)`: the next capability's offset.
pub const fn pci_caplist_next(cr: u32) -> u32 {
    (cr >> 8) & 0xff
}

/// `PCI_CAPLIST_CAP(cr)`: the capability's ID.
pub const fn pci_caplist_cap(cr: u32) -> u32 {
    cr & 0xff
}

/// `PCI_CAP_RESERVED`.
pub const PCI_CAP_RESERVED: i32 = 0x00;

/// `PCI_CAP_PWRMGMT`.
pub const PCI_CAP_PWRMGMT: i32 = 0x01;

/// `PCI_CAP_AGP`.
pub const PCI_CAP_AGP: i32 = 0x02;

/// `PCI_CAP_VPD`.
pub const PCI_CAP_VPD: i32 = 0x03;

/// `PCI_CAP_SLOTID`.
pub const PCI_CAP_SLOTID: i32 = 0x04;

/// `PCI_CAP_MSI`.
pub const PCI_CAP_MSI: i32 = 0x05;

/// `PCI_CAP_CPCI_HOTSWAP`.
pub const PCI_CAP_CPCI_HOTSWAP: i32 = 0x06;

/// `PCI_CAP_PCIX`.
pub const PCI_CAP_PCIX: i32 = 0x07;

/// `PCI_CAP_HT`.
pub const PCI_CAP_HT: i32 = 0x08;

/// `PCI_CAP_VENDSPEC`.
pub const PCI_CAP_VENDSPEC: i32 = 0x09;

/// `PCI_CAP_DEBUGPORT`.
pub const PCI_CAP_DEBUGPORT: i32 = 0x0a;

/// `PCI_CAP_CPCI_RSRCCTL`.
pub const PCI_CAP_CPCI_RSRCCTL: i32 = 0x0b;

/// `PCI_CAP_HOTPLUG`.
pub const PCI_CAP_HOTPLUG: i32 = 0x0c;

/// `PCI_CAP_AGP8`.
pub const PCI_CAP_AGP8: i32 = 0x0e;

/// `PCI_CAP_SECURE`.
pub const PCI_CAP_SECURE: i32 = 0x0f;

/// `PCI_CAP_PCIEXPRESS`.
pub const PCI_CAP_PCIEXPRESS: i32 = 0x10;

/// `PCI_CAP_MSIX`.
pub const PCI_CAP_MSIX: i32 = 0x11;

/// `PCI_CAP_SATA`.
pub const PCI_CAP_SATA: i32 = 0x12;

/// `PCI_VPD_ADDRESS_MASK`.
pub const PCI_VPD_ADDRESS_MASK: u32 = 0x7fff;

/// `PCI_VPD_ADDRESS_SHIFT`.
pub const PCI_VPD_ADDRESS_SHIFT: u32 = 16;

/// `PCI_VPD_ADDRESS(ofs)`.
pub const fn pci_vpd_address(ofs: u32) -> u32 {
    (ofs & PCI_VPD_ADDRESS_MASK) << PCI_VPD_ADDRESS_SHIFT
}

/// `PCI_VPD_DATAREG(ofs)`.
pub const fn pci_vpd_datareg(ofs: i32) -> i32 {
    ofs + 4
}

/// `PCI_VPD_OPFLAG`.
pub const PCI_VPD_OPFLAG: u32 = 0x80000000;

/// `PCI_MSI_MC`.
pub const PCI_MSI_MC: i32 = 0x00;

/// `PCI_MSI_MC_PVMASK`.
pub const PCI_MSI_MC_PVMASK: u32 = 0x01000000;

/// `PCI_MSI_MC_C64`.
pub const PCI_MSI_MC_C64: u32 = 0x00800000;

/// `PCI_MSI_MC_MME_MASK`.
pub const PCI_MSI_MC_MME_MASK: u32 = 0x00700000;

/// `PCI_MSI_MC_MME_SHIFT`.
pub const PCI_MSI_MC_MME_SHIFT: u32 = 20;

/// `PCI_MSI_MC_MMC_MASK`.
pub const PCI_MSI_MC_MMC_MASK: u32 = 0x000e0000;

/// `PCI_MSI_MC_MMC_SHIFT`.
pub const PCI_MSI_MC_MMC_SHIFT: u32 = 17;

/// `PCI_MSI_MC_MSIE`.
pub const PCI_MSI_MC_MSIE: u32 = 0x00010000;

/// `PCI_MSI_MA`.
pub const PCI_MSI_MA: i32 = 0x04;

/// `PCI_MSI_MAU32`.
pub const PCI_MSI_MAU32: i32 = 0x08;

/// `PCI_MSI_MD32`.
pub const PCI_MSI_MD32: i32 = 0x08;

/// `PCI_MSI_MD64`.
pub const PCI_MSI_MD64: i32 = 0x0c;

/// `PCI_MSI_MASK32`.
pub const PCI_MSI_MASK32: i32 = 0x0c;

/// `PCI_MSI_MASK64`.
pub const PCI_MSI_MASK64: i32 = 0x10;

/// `PCI_PMCSR`.
pub const PCI_PMCSR: i32 = 0x04;

/// `PCI_PMCSR_STATE_MASK`.
pub const PCI_PMCSR_STATE_MASK: u32 = 0x0003;

/// `PCI_PMCSR_STATE_D0`.
pub const PCI_PMCSR_STATE_D0: u32 = 0x0000;

/// `PCI_PMCSR_STATE_D1`.
pub const PCI_PMCSR_STATE_D1: u32 = 0x0001;

/// `PCI_PMCSR_STATE_D2`.
pub const PCI_PMCSR_STATE_D2: u32 = 0x0002;

/// `PCI_PMCSR_STATE_D3`.
pub const PCI_PMCSR_STATE_D3: u32 = 0x0003;

/// `PCI_PMCSR_PME_STATUS`.
pub const PCI_PMCSR_PME_STATUS: u32 = 0x8000;

/// `PCI_PMCSR_PME_EN`.
pub const PCI_PMCSR_PME_EN: u32 = 0x0100;

/// `PCI_HT_CAP(cr)`: the HyperTransport capability type.
pub const fn pci_ht_cap(cr: u32) -> u32 {
    if (cr >> 27) < 0x08 {
        (cr >> 27) & 0x1c
    } else {
        (cr >> 27) & 0x1f
    }
}

/// `PCI_HT_CAP_SLAVE`.
pub const PCI_HT_CAP_SLAVE: i32 = 0x00;

/// `PCI_HT_CAP_HOST`.
pub const PCI_HT_CAP_HOST: i32 = 0x04;

/// `PCI_HT_CAP_INTR`.
pub const PCI_HT_CAP_INTR: i32 = 0x10;

/// `PCI_HT_CAP_MSI`.
pub const PCI_HT_CAP_MSI: i32 = 0x15;

/// `PCI_HT_MSI_ENABLED`.
pub const PCI_HT_MSI_ENABLED: u32 = 0x00010000;

/// `PCI_HT_MSI_FIXED`.
pub const PCI_HT_MSI_FIXED: u32 = 0x00020000;

/// `PCI_HT_MSI_FIXED_ADDR`.
pub const PCI_HT_MSI_FIXED_ADDR: u64 = 0xfee00000;

/// `PCI_HT_MSI_ADDR`.
pub const PCI_HT_MSI_ADDR: i32 = 0x04;

/// `PCI_HT_MSI_ADDR_HI32`.
pub const PCI_HT_MSI_ADDR_HI32: i32 = 0x08;

/// `PCI_HT_INTR_DATA`.
pub const PCI_HT_INTR_DATA: i32 = 0x04;

/// `PCI_PCIE_XCAP`.
pub const PCI_PCIE_XCAP: i32 = 0x00;

/// `PCI_PCIE_XCAP_SI`.
pub const PCI_PCIE_XCAP_SI: u32 = 0x01000000;

/// `PCI_PCIE_XCAP_VER(x)`.
pub const fn pci_pcie_xcap_ver(x: u32) -> u32 {
    (x >> 16) & 0x0f
}

/// `PCI_PCIE_XCAP_TYPE(x)`: the device/port type.
pub const fn pci_pcie_xcap_type(x: u32) -> u32 {
    (x >> 20) & 0x0f
}

/// `PCI_PCIE_XCAP_TYPE_RP`.
pub const PCI_PCIE_XCAP_TYPE_RP: u32 = 0x4;

/// `PCI_PCIE_XCAP_TYPE_DOWN`.
pub const PCI_PCIE_XCAP_TYPE_DOWN: u32 = 0x6;

/// `PCI_PCIE_XCAP_TYPE_PCI2PCIE`.
pub const PCI_PCIE_XCAP_TYPE_PCI2PCIE: u32 = 0x8;

/// `PCI_PCIE_DCAP`.
pub const PCI_PCIE_DCAP: i32 = 0x04;

/// `PCI_PCIE_DCSR`.
pub const PCI_PCIE_DCSR: i32 = 0x08;

/// `PCI_PCIE_DCSR_ERO`.
pub const PCI_PCIE_DCSR_ERO: u32 = 0x00000010;

/// `PCI_PCIE_DCSR_ENS`.
pub const PCI_PCIE_DCSR_ENS: u32 = 0x00000800;

/// `PCI_PCIE_DCSR_MPS`.
pub const PCI_PCIE_DCSR_MPS: u32 = 0x00007000;

/// `PCI_PCIE_DCSR_CEE`.
pub const PCI_PCIE_DCSR_CEE: u32 = 0x00010000;

/// `PCI_PCIE_DCSR_NFE`.
pub const PCI_PCIE_DCSR_NFE: u32 = 0x00020000;

/// `PCI_PCIE_DCSR_FEE`.
pub const PCI_PCIE_DCSR_FEE: u32 = 0x00040000;

/// `PCI_PCIE_DCSR_URE`.
pub const PCI_PCIE_DCSR_URE: u32 = 0x00080000;

/// `PCI_PCIE_LCAP`.
pub const PCI_PCIE_LCAP: i32 = 0x0c;

/// `PCI_PCIE_LCAP_ASPM_L0S`.
pub const PCI_PCIE_LCAP_ASPM_L0S: u32 = 0x00000400;

/// `PCI_PCIE_LCAP_ASPM_L1`.
pub const PCI_PCIE_LCAP_ASPM_L1: u32 = 0x00000800;

/// `PCI_PCIE_LCSR`.
pub const PCI_PCIE_LCSR: i32 = 0x10;

/// `PCI_PCIE_LCSR_ASPM_L0S`.
pub const PCI_PCIE_LCSR_ASPM_L0S: u32 = 0x00000001;

/// `PCI_PCIE_LCSR_ASPM_L1`.
pub const PCI_PCIE_LCSR_ASPM_L1: u32 = 0x00000002;

/// `PCI_PCIE_LCSR_RL`.
pub const PCI_PCIE_LCSR_RL: u32 = 0x00000020;

/// `PCI_PCIE_LCSR_CCC`.
pub const PCI_PCIE_LCSR_CCC: u32 = 0x00000040;

/// `PCI_PCIE_LCSR_ES`.
pub const PCI_PCIE_LCSR_ES: u32 = 0x00000080;

/// `PCI_PCIE_LCSR_ECPM`.
pub const PCI_PCIE_LCSR_ECPM: u32 = 0x00000100;

/// `PCI_PCIE_LCSR_CLS`.
pub const PCI_PCIE_LCSR_CLS: u32 = 0x000f0000;

/// `PCI_PCIE_LCSR_CLS_2_5`.
pub const PCI_PCIE_LCSR_CLS_2_5: u32 = 0x00010000;

/// `PCI_PCIE_LCSR_CLS_5`.
pub const PCI_PCIE_LCSR_CLS_5: u32 = 0x00020000;

/// `PCI_PCIE_LCSR_CLS_8`.
pub const PCI_PCIE_LCSR_CLS_8: u32 = 0x00030000;

/// `PCI_PCIE_LCSR_CLS_16`.
pub const PCI_PCIE_LCSR_CLS_16: u32 = 0x00040000;

/// `PCI_PCIE_LCSR_CLS_32`.
pub const PCI_PCIE_LCSR_CLS_32: u32 = 0x00050000;

/// `PCI_PCIE_LCSR_LT`.
pub const PCI_PCIE_LCSR_LT: u32 = 0x08000000;

/// `PCI_PCIE_LCSR_SCC`.
pub const PCI_PCIE_LCSR_SCC: u32 = 0x10000000;

/// `PCI_PCIE_SLCAP`.
pub const PCI_PCIE_SLCAP: i32 = 0x14;

/// `PCI_PCIE_SLCAP_ABP`.
pub const PCI_PCIE_SLCAP_ABP: u32 = 0x00000001;

/// `PCI_PCIE_SLCAP_PCP`.
pub const PCI_PCIE_SLCAP_PCP: u32 = 0x00000002;

/// `PCI_PCIE_SLCAP_MSP`.
pub const PCI_PCIE_SLCAP_MSP: u32 = 0x00000004;

/// `PCI_PCIE_SLCAP_AIP`.
pub const PCI_PCIE_SLCAP_AIP: u32 = 0x00000008;

/// `PCI_PCIE_SLCAP_PIP`.
pub const PCI_PCIE_SLCAP_PIP: u32 = 0x00000010;

/// `PCI_PCIE_SLCAP_HPS`.
pub const PCI_PCIE_SLCAP_HPS: u32 = 0x00000020;

/// `PCI_PCIE_SLCAP_HPC`.
pub const PCI_PCIE_SLCAP_HPC: u32 = 0x00000040;

/// `PCI_PCIE_SLCSR`.
pub const PCI_PCIE_SLCSR: i32 = 0x18;

/// `PCI_PCIE_SLCSR_ABE`.
pub const PCI_PCIE_SLCSR_ABE: u32 = 0x00000001;

/// `PCI_PCIE_SLCSR_PFE`.
pub const PCI_PCIE_SLCSR_PFE: u32 = 0x00000002;

/// `PCI_PCIE_SLCSR_MSE`.
pub const PCI_PCIE_SLCSR_MSE: u32 = 0x00000004;

/// `PCI_PCIE_SLCSR_PDE`.
pub const PCI_PCIE_SLCSR_PDE: u32 = 0x00000008;

/// `PCI_PCIE_SLCSR_CCE`.
pub const PCI_PCIE_SLCSR_CCE: u32 = 0x00000010;

/// `PCI_PCIE_SLCSR_HPE`.
pub const PCI_PCIE_SLCSR_HPE: u32 = 0x00000020;

/// `PCI_PCIE_SLCSR_ABP`.
pub const PCI_PCIE_SLCSR_ABP: u32 = 0x00010000;

/// `PCI_PCIE_SLCSR_PFD`.
pub const PCI_PCIE_SLCSR_PFD: u32 = 0x00020000;

/// `PCI_PCIE_SLCSR_MSC`.
pub const PCI_PCIE_SLCSR_MSC: u32 = 0x00040000;

/// `PCI_PCIE_SLCSR_PDC`.
pub const PCI_PCIE_SLCSR_PDC: u32 = 0x00080000;

/// `PCI_PCIE_SLCSR_CC`.
pub const PCI_PCIE_SLCSR_CC: u32 = 0x00100000;

/// `PCI_PCIE_SLCSR_MS`.
pub const PCI_PCIE_SLCSR_MS: u32 = 0x00200000;

/// `PCI_PCIE_SLCSR_PDS`.
pub const PCI_PCIE_SLCSR_PDS: u32 = 0x00400000;

/// `PCI_PCIE_SLCSR_LACS`.
pub const PCI_PCIE_SLCSR_LACS: u32 = 0x01000000;

/// `PCI_PCIE_RCSR`.
pub const PCI_PCIE_RCSR: i32 = 0x1c;

/// `PCI_PCIE_DCSR2`.
pub const PCI_PCIE_DCSR2: i32 = 0x28;

/// `PCI_PCIE_DCSR2_LTREN`.
pub const PCI_PCIE_DCSR2_LTREN: u32 = 0x00000400;

/// `PCI_PCIE_LCAP2`.
pub const PCI_PCIE_LCAP2: i32 = 0x2c;

/// `PCI_PCIE_LCSR2`.
pub const PCI_PCIE_LCSR2: i32 = 0x30;

/// `PCI_PCIE_LCSR2_TLS`.
pub const PCI_PCIE_LCSR2_TLS: u32 = 0x0000000f;

/// `PCI_PCIE_LCSR2_TLS_2_5`.
pub const PCI_PCIE_LCSR2_TLS_2_5: u32 = 0x00000001;

/// `PCI_PCIE_LCSR2_TLS_5`.
pub const PCI_PCIE_LCSR2_TLS_5: u32 = 0x00000002;

/// `PCI_PCIE_LCSR2_TLS_8`.
pub const PCI_PCIE_LCSR2_TLS_8: u32 = 0x00000003;

/// `PCI_PCIE_LCSR2_TLS_16`.
pub const PCI_PCIE_LCSR2_TLS_16: u32 = 0x00000004;

/// `PCI_PCIE_LCSR2_TLS_32`.
pub const PCI_PCIE_LCSR2_TLS_32: u32 = 0x00000005;

/// `PCI_PCIE_ECAP`.
pub const PCI_PCIE_ECAP: i32 = 0x100;

/// `PCI_PCIE_ECAP_ID(x)`.
pub const fn pci_pcie_ecap_id(x: u32) -> u32 {
    x & 0x0000_ffff
}

/// `PCI_PCIE_ECAP_VER(x)`.
pub const fn pci_pcie_ecap_ver(x: u32) -> u32 {
    (x >> 16) & 0x0f
}

/// `PCI_PCIE_ECAP_NEXT(x)`.
pub const fn pci_pcie_ecap_next(x: u32) -> u32 {
    (x >> 20) & 0xffc
}

/// `PCI_PCIE_ECAP_LAST`.
pub const PCI_PCIE_ECAP_LAST: i32 = 0x0;

/// `PCI_MSIX_MC_MSIXE`.
pub const PCI_MSIX_MC_MSIXE: u32 = 0x80000000;

/// `PCI_MSIX_MC_FM`.
pub const PCI_MSIX_MC_FM: u32 = 0x40000000;

/// `PCI_MSIX_MC_TBLSZ_MASK`.
pub const PCI_MSIX_MC_TBLSZ_MASK: u32 = 0x07ff0000;

/// `PCI_MSIX_MC_TBLSZ_SHIFT`.
pub const PCI_MSIX_MC_TBLSZ_SHIFT: u32 = 16;

/// `PCI_MSIX_MC_TBLSZ(reg)`: the MSI-X table size minus one.
pub const fn pci_msix_mc_tblsz(reg: u32) -> u32 {
    (reg & PCI_MSIX_MC_TBLSZ_MASK) >> PCI_MSIX_MC_TBLSZ_SHIFT
}

/// `PCI_MSIX_TABLE`.
pub const PCI_MSIX_TABLE: i32 = 0x04;

/// `PCI_MSIX_TABLE_BIR`.
pub const PCI_MSIX_TABLE_BIR: u32 = 0x00000007;

/// `PCI_MSIX_TABLE_OFF`.
pub const PCI_MSIX_TABLE_OFF: u32 = !PCI_MSIX_TABLE_BIR;

/// `PCI_MSIX_MA(i)`: the message address of table entry `i`.
pub const fn pci_msix_ma(i: usize) -> usize {
    i * 16
}

/// `PCI_MSIX_MAU32(i)`.
pub const fn pci_msix_mau32(i: usize) -> usize {
    i * 16 + 4
}

/// `PCI_MSIX_MD(i)`: the message data.
pub const fn pci_msix_md(i: usize) -> usize {
    i * 16 + 8
}

/// `PCI_MSIX_VC(i)`: the vector control.
pub const fn pci_msix_vc(i: usize) -> usize {
    i * 16 + 12
}

/// `PCI_MSIX_VC_MASK`.
pub const PCI_MSIX_VC_MASK: u32 = 0x00000001;

/// `PCI_INTERRUPT_REG`.
pub const PCI_INTERRUPT_REG: i32 = 0x3c;

/// `pci_intr_pin_t`.
pub type PciIntrPin = u8;
/// `pci_intr_line_t`.
pub type PciIntrLine = u8;

/// `PCI_INTERRUPT_PIN_SHIFT`.
pub const PCI_INTERRUPT_PIN_SHIFT: u32 = 8;

/// `PCI_INTERRUPT_PIN_MASK`.
pub const PCI_INTERRUPT_PIN_MASK: u32 = 0xff;

/// `PCI_INTERRUPT_PIN(icr)`.
pub const fn pci_interrupt_pin(icr: u32) -> u32 {
    (icr >> PCI_INTERRUPT_PIN_SHIFT) & PCI_INTERRUPT_PIN_MASK
}

/// `PCI_INTERRUPT_LINE_SHIFT`.
pub const PCI_INTERRUPT_LINE_SHIFT: u32 = 0;

/// `PCI_INTERRUPT_LINE_MASK`.
pub const PCI_INTERRUPT_LINE_MASK: u32 = 0xff;

/// `PCI_INTERRUPT_LINE(icr)`.
pub const fn pci_interrupt_line(icr: u32) -> u32 {
    (icr >> PCI_INTERRUPT_LINE_SHIFT) & PCI_INTERRUPT_LINE_MASK
}

/// `PCI_MIN_GNT_SHIFT`.
pub const PCI_MIN_GNT_SHIFT: u32 = 16;

/// `PCI_MIN_GNT_MASK`.
pub const PCI_MIN_GNT_MASK: u32 = 0xff;

/// `PCI_MIN_GNT(icr)`.
pub const fn pci_min_gnt(icr: u32) -> u32 {
    (icr >> PCI_MIN_GNT_SHIFT) & PCI_MIN_GNT_MASK
}

/// `PCI_MAX_LAT_SHIFT`.
pub const PCI_MAX_LAT_SHIFT: u32 = 24;

/// `PCI_MAX_LAT_MASK`.
pub const PCI_MAX_LAT_MASK: u32 = 0xff;

/// `PCI_MAX_LAT(icr)`.
pub const fn pci_max_lat(icr: u32) -> u32 {
    (icr >> PCI_MAX_LAT_SHIFT) & PCI_MAX_LAT_MASK
}

/// `PCI_INTERRUPT_PIN_NONE`.
pub const PCI_INTERRUPT_PIN_NONE: u32 = 0x00;

/// `PCI_INTERRUPT_PIN_A`.
pub const PCI_INTERRUPT_PIN_A: u32 = 0x01;

/// `PCI_INTERRUPT_PIN_B`.
pub const PCI_INTERRUPT_PIN_B: u32 = 0x02;

/// `PCI_INTERRUPT_PIN_C`.
pub const PCI_INTERRUPT_PIN_C: u32 = 0x03;

/// `PCI_INTERRUPT_PIN_D`.
pub const PCI_INTERRUPT_PIN_D: u32 = 0x04;

/// `PCI_INTERRUPT_PIN_MAX`.
pub const PCI_INTERRUPT_PIN_MAX: u32 = 0x04;

/// `struct pci_vpd_smallres`: a small Vital Product Data resource tag; the data follows.
#[repr(C, packed)]
#[derive(Clone, Copy, Debug)]
pub struct PciVpdSmallres {
    /// `vpdres_byte0`: length of data + tag.
    pub vpdres_byte0: u8,
}

/// `struct pci_vpd_largeres`: a large resource tag; the data follows.
#[repr(C, packed)]
#[derive(Clone, Copy, Debug)]
pub struct PciVpdLargeres {
    /// `vpdres_byte0`.
    pub vpdres_byte0: u8,
    /// `vpdres_len_lsb`: length of data only.
    pub vpdres_len_lsb: u8,
    /// `vpdres_len_msb`.
    pub vpdres_len_msb: u8,
}

/// `PCI_VPDRES_ISLARGE(x)`.
pub const fn pci_vpdres_islarge(x: u8) -> bool {
    x & 0x80 != 0
}

/// `PCI_VPDRES_SMALL_LENGTH(x)`.
pub const fn pci_vpdres_small_length(x: u8) -> u8 {
    x & 0x7
}

/// `PCI_VPDRES_SMALL_NAME(x)`.
pub const fn pci_vpdres_small_name(x: u8) -> u8 {
    (x >> 3) & 0xf
}

/// `PCI_VPDRES_LARGE_NAME(x)`.
pub const fn pci_vpdres_large_name(x: u8) -> u8 {
    x & 0x7f
}

/// `PCI_VPDRES_TYPE_COMPATIBLE_DEVICE_ID`: small
pub const PCI_VPDRES_TYPE_COMPATIBLE_DEVICE_ID: u8 = 0x3;

/// `PCI_VPDRES_TYPE_VENDOR_DEFINED`: small
pub const PCI_VPDRES_TYPE_VENDOR_DEFINED: u8 = 0xe;

/// `PCI_VPDRES_TYPE_END_TAG`: small
pub const PCI_VPDRES_TYPE_END_TAG: u8 = 0xf;

/// `PCI_VPDRES_TYPE_IDENTIFIER_STRING`: large
pub const PCI_VPDRES_TYPE_IDENTIFIER_STRING: u8 = 0x02;

/// `PCI_VPDRES_TYPE_VPD`: large
pub const PCI_VPDRES_TYPE_VPD: u8 = 0x10;

/// `struct pci_vpd`: a VPD field: a two-letter key (`PN` part number, `SN` serial number,
/// ...) and its length; the data follows.
#[repr(C, packed)]
#[derive(Clone, Copy, Debug)]
pub struct PciVpd {
    /// `vpd_key0`.
    pub vpd_key0: u8,
    /// `vpd_key1`.
    pub vpd_key1: u8,
    /// `vpd_len`: length of data only.
    pub vpd_len: u8,
}

#[cfg(test)]
mod tests;
