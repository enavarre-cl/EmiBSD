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

/*
 * Copyright (c) 1998, 1999 Christopher G. Demetriou.  All rights reserved.
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
 *      This product includes software developed by Christopher G. Demetriou
 *	for the NetBSD Project.
 * 4. The name of the author may not be used to endorse or promote products
 *    derived from this software without specific prior written permission
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
//! PCI "universal" communications card driver configuration data (used to match/attach the
//! cards): `dev/pci/pucdata.c`.
//!
//! Upstream: sys/dev/pci/pucdata.c @ 3ce1f3f79392
//!
//! The table of cards `puc(4)` knows, [`PUC_DEVS`], whole and in the C's order (the first card
//! that matches wins): 218 cards and their 566 ports, from Intel's KT serial ports to QEMU's
//! `pci-serial`. Each entry keeps the C's comment about the card.
//!
//! ## Deviations
//! - `puc_devs[]` is [`PUC_DEVS`] and `int puc_ndevs = nitems(puc_devs)` is `PUC_DEVS.len()`.
//! - An entry is a [`PucDeviceDescription::new`] call (IDs, masks, ports) rather than nested
//!   braces; the unused ports of the 16 are filled with zeros there.
//! - The vendor and product IDs the table names are in `pcidevs.rs` (the part of `pcidevs.h`
//!   `puc.c` and `pucdata.c` use).

use crate::dev::pci::pcidevs::*;
use crate::dev::pci::pucvar::{
    PUC_PORT_COM, PUC_PORT_COM_MUL4, PUC_PORT_COM_MUL8, PUC_PORT_COM_MUL10, PUC_PORT_COM_MUL128,
    PUC_PORT_COM_XR17V35X, PUC_PORT_LPT, PucDeviceDescription, PucPort,
};

/// `puc_devs[]`.
pub static PUC_DEVS: [PucDeviceDescription; 218] = [
    // 6 Series KT
    PucDeviceDescription::new(
        [
            PCI_VENDOR_INTEL,
            PCI_PRODUCT_INTEL_6SERIES_KT,
            0x0000,
            0x0000,
        ],
        [0xffff, 0xffff, 0x0000, 0x0000],
        &[PucPort::new(PUC_PORT_COM, 0x10, 0x0000)],
    ),
    // 7 Series KT
    PucDeviceDescription::new(
        [
            PCI_VENDOR_INTEL,
            PCI_PRODUCT_INTEL_7SERIES_KT,
            0x0000,
            0x0000,
        ],
        [0xffff, 0xffff, 0x0000, 0x0000],
        &[PucPort::new(PUC_PORT_COM, 0x10, 0x0000)],
    ),
    // 8 Series KT
    PucDeviceDescription::new(
        [
            PCI_VENDOR_INTEL,
            PCI_PRODUCT_INTEL_8SERIES_KT,
            0x0000,
            0x0000,
        ],
        [0xffff, 0xffff, 0x0000, 0x0000],
        &[PucPort::new(PUC_PORT_COM, 0x10, 0x0000)],
    ),
    // 8 Series LP KT
    PucDeviceDescription::new(
        [
            PCI_VENDOR_INTEL,
            PCI_PRODUCT_INTEL_8SERIES_LP_KT,
            0x0000,
            0x0000,
        ],
        [0xffff, 0xffff, 0x0000, 0x0000],
        &[PucPort::new(PUC_PORT_COM, 0x10, 0x0000)],
    ),
    // 9 Series KT
    PucDeviceDescription::new(
        [
            PCI_VENDOR_INTEL,
            PCI_PRODUCT_INTEL_9SERIES_KT,
            0x0000,
            0x0000,
        ],
        [0xffff, 0xffff, 0x0000, 0x0000],
        &[PucPort::new(PUC_PORT_COM, 0x10, 0x0000)],
    ),
    // 9 Series LP KT
    PucDeviceDescription::new(
        [
            PCI_VENDOR_INTEL,
            PCI_PRODUCT_INTEL_9SERIES_LP_KT,
            0x0000,
            0x0000,
        ],
        [0xffff, 0xffff, 0x0000, 0x0000],
        &[PucPort::new(PUC_PORT_COM, 0x10, 0x0000)],
    ),
    // 100 Series KT
    PucDeviceDescription::new(
        [
            PCI_VENDOR_INTEL,
            PCI_PRODUCT_INTEL_100SERIES_KT,
            0x0000,
            0x0000,
        ],
        [0xffff, 0xffff, 0x0000, 0x0000],
        &[PucPort::new(PUC_PORT_COM, 0x10, 0x0000)],
    ),
    // 100 Series LP KT
    PucDeviceDescription::new(
        [
            PCI_VENDOR_INTEL,
            PCI_PRODUCT_INTEL_100SERIES_LP_KT,
            0x0000,
            0x0000,
        ],
        [0xffff, 0xffff, 0x0000, 0x0000],
        &[PucPort::new(PUC_PORT_COM, 0x10, 0x0000)],
    ),
    // 200 Series KT
    PucDeviceDescription::new(
        [
            PCI_VENDOR_INTEL,
            PCI_PRODUCT_INTEL_200SERIES_KT,
            0x0000,
            0x0000,
        ],
        [0xffff, 0xffff, 0x0000, 0x0000],
        &[PucPort::new(PUC_PORT_COM, 0x10, 0x0000)],
    ),
    // 300 Series KT
    PucDeviceDescription::new(
        [
            PCI_VENDOR_INTEL,
            PCI_PRODUCT_INTEL_300SERIES_KT,
            0x0000,
            0x0000,
        ],
        [0xffff, 0xffff, 0x0000, 0x0000],
        &[PucPort::new(PUC_PORT_COM, 0x10, 0x0000)],
    ),
    // 300 Series U KT
    PucDeviceDescription::new(
        [
            PCI_VENDOR_INTEL,
            PCI_PRODUCT_INTEL_300SERIES_U_KT,
            0x0000,
            0x0000,
        ],
        [0xffff, 0xffff, 0x0000, 0x0000],
        &[PucPort::new(PUC_PORT_COM, 0x10, 0x0000)],
    ),
    // 400 Series KT
    PucDeviceDescription::new(
        [
            PCI_VENDOR_INTEL,
            PCI_PRODUCT_INTEL_400SERIES_KT,
            0x0000,
            0x0000,
        ],
        [0xffff, 0xffff, 0x0000, 0x0000],
        &[PucPort::new(PUC_PORT_COM, 0x10, 0x0000)],
    ),
    // 400 Series LP KT
    PucDeviceDescription::new(
        [
            PCI_VENDOR_INTEL,
            PCI_PRODUCT_INTEL_400SERIES_LP_KT,
            0x0000,
            0x0000,
        ],
        [0xffff, 0xffff, 0x0000, 0x0000],
        &[PucPort::new(PUC_PORT_COM, 0x10, 0x0000)],
    ),
    // 400 Series V KT
    PucDeviceDescription::new(
        [
            PCI_VENDOR_INTEL,
            PCI_PRODUCT_INTEL_400SERIES_V_KT,
            0x0000,
            0x0000,
        ],
        [0xffff, 0xffff, 0x0000, 0x0000],
        &[PucPort::new(PUC_PORT_COM, 0x10, 0x0000)],
    ),
    // 495 Series LP KT
    PucDeviceDescription::new(
        [
            PCI_VENDOR_INTEL,
            PCI_PRODUCT_INTEL_495SERIES_LP_KT,
            0x0000,
            0x0000,
        ],
        [0xffff, 0xffff, 0x0000, 0x0000],
        &[PucPort::new(PUC_PORT_COM, 0x10, 0x0000)],
    ),
    // 500 Series KT
    PucDeviceDescription::new(
        [
            PCI_VENDOR_INTEL,
            PCI_PRODUCT_INTEL_500SERIES_KT,
            0x0000,
            0x0000,
        ],
        [0xffff, 0xffff, 0x0000, 0x0000],
        &[PucPort::new(PUC_PORT_COM, 0x10, 0x0000)],
    ),
    // 500 Series LP KT
    PucDeviceDescription::new(
        [
            PCI_VENDOR_INTEL,
            PCI_PRODUCT_INTEL_500SERIES_LP_KT,
            0x0000,
            0x0000,
        ],
        [0xffff, 0xffff, 0x0000, 0x0000],
        &[PucPort::new(PUC_PORT_COM, 0x10, 0x0000)],
    ),
    // 600 Series KT
    PucDeviceDescription::new(
        [
            PCI_VENDOR_INTEL,
            PCI_PRODUCT_INTEL_600SERIES_KT,
            0x0000,
            0x0000,
        ],
        [0xffff, 0xffff, 0x0000, 0x0000],
        &[PucPort::new(PUC_PORT_COM, 0x10, 0x0000)],
    ),
    // 600 Series LP KT
    PucDeviceDescription::new(
        [
            PCI_VENDOR_INTEL,
            PCI_PRODUCT_INTEL_600SERIES_LP_KT,
            0x0000,
            0x0000,
        ],
        [0xffff, 0xffff, 0x0000, 0x0000],
        &[PucPort::new(PUC_PORT_COM, 0x10, 0x0000)],
    ),
    // 700 Series KT
    PucDeviceDescription::new(
        [
            PCI_VENDOR_INTEL,
            PCI_PRODUCT_INTEL_700SERIES_KT,
            0x0000,
            0x0000,
        ],
        [0xffff, 0xffff, 0x0000, 0x0000],
        &[PucPort::new(PUC_PORT_COM, 0x10, 0x0000)],
    ),
    // MTL KT
    PucDeviceDescription::new(
        [PCI_VENDOR_INTEL, PCI_PRODUCT_INTEL_MTL_KT, 0x0000, 0x0000],
        [0xffff, 0xffff, 0x0000, 0x0000],
        &[PucPort::new(PUC_PORT_COM, 0x10, 0x0000)],
    ),
    // 82946GZ KT
    PucDeviceDescription::new(
        [
            PCI_VENDOR_INTEL,
            PCI_PRODUCT_INTEL_82946GZ_KT,
            0x0000,
            0x0000,
        ],
        [0xffff, 0xffff, 0x0000, 0x0000],
        &[PucPort::new(PUC_PORT_COM, 0x10, 0x0000)],
    ),
    // 82Q965 KT
    PucDeviceDescription::new(
        [
            PCI_VENDOR_INTEL,
            PCI_PRODUCT_INTEL_82Q965_KT,
            0x0000,
            0x0000,
        ],
        [0xffff, 0xffff, 0x0000, 0x0000],
        &[PucPort::new(PUC_PORT_COM, 0x10, 0x0000)],
    ),
    // 82G965 KT
    PucDeviceDescription::new(
        [
            PCI_VENDOR_INTEL,
            PCI_PRODUCT_INTEL_82G965_KT,
            0x0000,
            0x0000,
        ],
        [0xffff, 0xffff, 0x0000, 0x0000],
        &[PucPort::new(PUC_PORT_COM, 0x10, 0x0000)],
    ),
    // 82Q35 KT
    PucDeviceDescription::new(
        [PCI_VENDOR_INTEL, PCI_PRODUCT_INTEL_82Q35_KT, 0x0000, 0x0000],
        [0xffff, 0xffff, 0x0000, 0x0000],
        &[PucPort::new(PUC_PORT_COM, 0x10, 0x0000)],
    ),
    // 82G33 KT
    PucDeviceDescription::new(
        [PCI_VENDOR_INTEL, PCI_PRODUCT_INTEL_82G33_KT, 0x0000, 0x0000],
        [0xffff, 0xffff, 0x0000, 0x0000],
        &[PucPort::new(PUC_PORT_COM, 0x10, 0x0000)],
    ),
    // 82Q33 KT
    PucDeviceDescription::new(
        [PCI_VENDOR_INTEL, PCI_PRODUCT_INTEL_82Q33_KT, 0x0000, 0x0000],
        [0xffff, 0xffff, 0x0000, 0x0000],
        &[PucPort::new(PUC_PORT_COM, 0x10, 0x0000)],
    ),
    // 82X38 KT
    PucDeviceDescription::new(
        [PCI_VENDOR_INTEL, PCI_PRODUCT_INTEL_82X38_KT, 0x0000, 0x0000],
        [0xffff, 0xffff, 0x0000, 0x0000],
        &[PucPort::new(PUC_PORT_COM, 0x10, 0x0000)],
    ),
    // GM965 KT
    PucDeviceDescription::new(
        [
            PCI_VENDOR_INTEL,
            PCI_PRODUCT_INTEL_82GM965_KT,
            0x0000,
            0x0000,
        ],
        [0xffff, 0xffff, 0x0000, 0x0000],
        &[PucPort::new(PUC_PORT_COM, 0x10, 0x0000)],
    ),
    // GME965 KT
    PucDeviceDescription::new(
        [
            PCI_VENDOR_INTEL,
            PCI_PRODUCT_INTEL_82GME965_KT,
            0x0000,
            0x0000,
        ],
        [0xffff, 0xffff, 0x0000, 0x0000],
        &[PucPort::new(PUC_PORT_COM, 0x10, 0x0000)],
    ),
    // GM45 KT
    PucDeviceDescription::new(
        [
            PCI_VENDOR_INTEL,
            PCI_PRODUCT_INTEL_82GM45_KT,
            0x0000,
            0x0000,
        ],
        [0xffff, 0xffff, 0x0000, 0x0000],
        &[PucPort::new(PUC_PORT_COM, 0x10, 0x0000)],
    ),
    // Q45 KT
    PucDeviceDescription::new(
        [PCI_VENDOR_INTEL, PCI_PRODUCT_INTEL_82Q45_KT, 0x0000, 0x0000],
        [0xffff, 0xffff, 0x0000, 0x0000],
        &[PucPort::new(PUC_PORT_COM, 0x10, 0x0000)],
    ),
    // 3400 KT
    PucDeviceDescription::new(
        [PCI_VENDOR_INTEL, PCI_PRODUCT_INTEL_3400_KT, 0x0000, 0x0000],
        [0xffff, 0xffff, 0x0000, 0x0000],
        &[PucPort::new(PUC_PORT_COM, 0x10, 0x0000)],
    ),
    // Intel EG20T
    PucDeviceDescription::new(
        [
            PCI_VENDOR_INTEL,
            PCI_PRODUCT_INTEL_EG20T_SERIAL_1,
            0x0000,
            0x0000,
        ],
        [0xffff, 0xffff, 0x0000, 0x0000],
        &[PucPort::new(PUC_PORT_COM, 0x10, 0x0000)],
    ),
    // Intel EG20T
    PucDeviceDescription::new(
        [
            PCI_VENDOR_INTEL,
            PCI_PRODUCT_INTEL_EG20T_SERIAL_2,
            0x0000,
            0x0000,
        ],
        [0xffff, 0xffff, 0x0000, 0x0000],
        &[PucPort::new(PUC_PORT_COM, 0x10, 0x0000)],
    ),
    // Intel EG20T
    PucDeviceDescription::new(
        [
            PCI_VENDOR_INTEL,
            PCI_PRODUCT_INTEL_EG20T_SERIAL_3,
            0x0000,
            0x0000,
        ],
        [0xffff, 0xffff, 0x0000, 0x0000],
        &[PucPort::new(PUC_PORT_COM, 0x10, 0x0000)],
    ),
    // Intel EG20T
    PucDeviceDescription::new(
        [
            PCI_VENDOR_INTEL,
            PCI_PRODUCT_INTEL_EG20T_SERIAL_4,
            0x0000,
            0x0000,
        ],
        [0xffff, 0xffff, 0x0000, 0x0000],
        &[PucPort::new(PUC_PORT_COM, 0x10, 0x0000)],
    ),
    // Atom S1200 UART
    PucDeviceDescription::new(
        [
            PCI_VENDOR_INTEL,
            PCI_PRODUCT_INTEL_ATOM_S1200_UART,
            0x0000,
            0x0000,
        ],
        [0xffff, 0xffff, 0x0000, 0x0000],
        &[PucPort::new(PUC_PORT_COM, 0x10, 0x0000)],
    ),
    // Intel C3000 UART
    PucDeviceDescription::new(
        [
            PCI_VENDOR_INTEL,
            PCI_PRODUCT_INTEL_C3000_HSUART,
            0x0000,
            0x0000,
        ],
        [0xffff, 0xffff, 0x0000, 0x0000],
        &[PucPort::new(PUC_PORT_COM, 0x10, 0x0000)],
    ),
    // XXX no entry because I have no data:
    // XXX Dolphin Peripherals 4006 (single parallel)
    // Dolphin Peripherals 4014 (dual parallel port) card.  PLX 9050, with
    // a seemingly-lame EEPROM setup that puts the Dolphin IDs
    // into the subsystem fields, and claims that it's a
    // network/misc (0x02/0x80) device.
    // "Dolphin Peripherals 4014"
    PucDeviceDescription::new(
        [PCI_VENDOR_PLX, PCI_PRODUCT_PLX_9050, 0xd84d, 0x6810],
        [0xffff, 0xffff, 0xffff, 0xffff],
        &[
            PucPort::new(PUC_PORT_LPT, 0x20, 0x0000),
            PucPort::new(PUC_PORT_LPT, 0x24, 0x0000),
        ],
    ),
    // XXX no entry because I have no data:
    // XXX Dolphin Peripherals 4025 (single serial)
    // Dolphin Peripherals 4035 (dual serial port) card.  PLX 9050, with
    // a seemingly-lame EEPROM setup that puts the Dolphin IDs
    // into the subsystem fields, and claims that it's a
    // network/misc (0x02/0x80) device.
    // "Dolphin Peripherals 4035"
    PucDeviceDescription::new(
        [PCI_VENDOR_PLX, PCI_PRODUCT_PLX_9050, 0xd84d, 0x6808],
        [0xffff, 0xffff, 0xffff, 0xffff],
        &[
            PucPort::new(PUC_PORT_COM, 0x18, 0x0000),
            PucPort::new(PUC_PORT_COM, 0x1c, 0x0000),
        ],
    ),
    // XXX no entry because I have no data:
    // XXX Dolphin Peripherals 4078 (dual serial and single parallel)
    // Decision PCCOM PCI series. PLX 9052 with 1 or 2 16554 UARTS
    // Decision Computer Inc PCCOM 2 Port RS232/422/485: 2S
    // "Decision Computer Inc PCCOM 2 Port RS232/422/485",
    PucDeviceDescription::new(
        [PCI_VENDOR_DCI, PCI_PRODUCT_DCI_APCI2, 0, 0],
        [0xffff, 0xffff, 0, 0],
        &[
            PucPort::new(PUC_PORT_COM, 0x1c, 0x0000),
            PucPort::new(PUC_PORT_COM, 0x1c, 0x0008),
        ],
    ),
    // Decision Computer Inc PCCOM 4 Port RS232/422/485: 4S
    // "Decision Computer Inc PCCOM 4 Port RS232/422/485",
    PucDeviceDescription::new(
        [PCI_VENDOR_DCI, PCI_PRODUCT_DCI_APCI4, 0, 0],
        [0xffff, 0xffff, 0, 0],
        &[
            PucPort::new(PUC_PORT_COM, 0x1c, 0x0000),
            PucPort::new(PUC_PORT_COM, 0x1c, 0x0008),
            PucPort::new(PUC_PORT_COM, 0x1c, 0x0010),
            PucPort::new(PUC_PORT_COM, 0x1c, 0x0018),
        ],
    ),
    // Decision Computer Inc PCCOM 8 Port RS232/422/485: 8S
    // "Decision Computer Inc PCCOM 8 Port RS232/422/485",
    PucDeviceDescription::new(
        [PCI_VENDOR_DCI, PCI_PRODUCT_DCI_APCI8, 0, 0],
        [0xffff, 0xffff, 0, 0],
        &[
            PucPort::new(PUC_PORT_COM, 0x1c, 0x0000),
            PucPort::new(PUC_PORT_COM, 0x1c, 0x0008),
            PucPort::new(PUC_PORT_COM, 0x1c, 0x0010),
            PucPort::new(PUC_PORT_COM, 0x1c, 0x0018),
            PucPort::new(PUC_PORT_COM, 0x1c, 0x0020),
            PucPort::new(PUC_PORT_COM, 0x1c, 0x0028),
            PucPort::new(PUC_PORT_COM, 0x1c, 0x0030),
            PucPort::new(PUC_PORT_COM, 0x1c, 0x0038),
        ],
    ),
    // IBM SurePOS 300 Series (481033H) serial ports
    // "IBM SurePOS 300 Series (481033H) serial ports",
    PucDeviceDescription::new(
        [PCI_VENDOR_IBM, PCI_PRODUCT_IBM_4810_SCC, 0, 0],
        [0xffff, 0xffff, 0, 0],
        &[
            PucPort::new(PUC_PORT_COM, 0x10, 0x0000), // Port C
            PucPort::new(PUC_PORT_COM, 0x18, 0x0000), // Port D
            PucPort::new(PUC_PORT_COM, 0x14, 0x0000), // Port E
            PucPort::new(PUC_PORT_COM, 0x1c, 0x0000), // Port F
        ],
    ),
    // SIIG Boards.
    // SIIG provides documentation for their boards at:
    // <URL:http://www.siig.com/driver.htm>
    // Please excuse the weird ordering, it's the order they
    // use in their documentation.
    // SIIG "10x" family boards.
    // SIIG Cyber Serial PCI 16C550 (10x family): 1S
    // "SIIG Cyber Serial PCI 16C550 (10x family)",
    PucDeviceDescription::new(
        [PCI_VENDOR_SIIG, PCI_PRODUCT_SIIG_1000, 0, 0],
        [0xffff, 0xffff, 0, 0],
        &[PucPort::new(PUC_PORT_COM, 0x18, 0x0000)],
    ),
    // SIIG Cyber Serial PCI 16C650 (10x family): 1S
    // "SIIG Cyber Serial PCI 16C650 (10x family)",
    PucDeviceDescription::new(
        [PCI_VENDOR_SIIG, PCI_PRODUCT_SIIG_1001, 0, 0],
        [0xffff, 0xffff, 0, 0],
        &[PucPort::new(PUC_PORT_COM, 0x18, 0x0000)],
    ),
    // SIIG Cyber Serial PCI 16C850 (10x family): 1S
    // "SIIG Cyber Serial PCI 16C850 (10x family)",
    PucDeviceDescription::new(
        [PCI_VENDOR_SIIG, PCI_PRODUCT_SIIG_1002, 0, 0],
        [0xffff, 0xffff, 0, 0],
        &[PucPort::new(PUC_PORT_COM, 0x18, 0x0000)],
    ),
    // SIIG Cyber I/O PCI 16C550 (10x family): 1S, 1P
    // "SIIG Cyber I/O PCI 16C550 (10x family)",
    PucDeviceDescription::new(
        [PCI_VENDOR_SIIG, PCI_PRODUCT_SIIG_1010, 0, 0],
        [0xffff, 0xffff, 0, 0],
        &[
            PucPort::new(PUC_PORT_COM, 0x18, 0x0000),
            PucPort::new(PUC_PORT_LPT, 0x1c, 0x0000),
        ],
    ),
    // SIIG Cyber I/O PCI 16C650 (10x family): 1S, 1P
    // "SIIG Cyber I/O PCI 16C650 (10x family)",
    PucDeviceDescription::new(
        [PCI_VENDOR_SIIG, PCI_PRODUCT_SIIG_1011, 0, 0],
        [0xffff, 0xffff, 0, 0],
        &[
            PucPort::new(PUC_PORT_COM, 0x18, 0x0000),
            PucPort::new(PUC_PORT_LPT, 0x1c, 0x0000),
        ],
    ),
    // SIIG Cyber I/O PCI 16C850 (10x family): 1S, 1P
    // "SIIG Cyber I/O PCI 16C850 (10x family)",
    PucDeviceDescription::new(
        [PCI_VENDOR_SIIG, PCI_PRODUCT_SIIG_1012, 0, 0],
        [0xffff, 0xffff, 0, 0],
        &[
            PucPort::new(PUC_PORT_COM, 0x18, 0x0000),
            PucPort::new(PUC_PORT_LPT, 0x1c, 0x0000),
        ],
    ),
    // SIIG Cyber Parallel PCI (10x family): 1P
    // "SIIG Cyber Parallel PCI (10x family)",
    PucDeviceDescription::new(
        [PCI_VENDOR_SIIG, PCI_PRODUCT_SIIG_1020, 0, 0],
        [0xffff, 0xffff, 0, 0],
        &[PucPort::new(PUC_PORT_LPT, 0x18, 0x0000)],
    ),
    // SIIG Cyber Parallel Dual PCI (10x family): 2P
    // "SIIG Cyber Parallel Dual PCI (10x family)",
    PucDeviceDescription::new(
        [PCI_VENDOR_SIIG, PCI_PRODUCT_SIIG_1021, 0, 0],
        [0xffff, 0xffff, 0, 0],
        &[
            PucPort::new(PUC_PORT_LPT, 0x18, 0x0000),
            PucPort::new(PUC_PORT_LPT, 0x20, 0x0000),
        ],
    ),
    // SIIG Cyber Serial Dual PCI 16C550 (10x family): 2S
    // "SIIG Cyber Serial Dual PCI 16C550 (10x family)",
    PucDeviceDescription::new(
        [PCI_VENDOR_SIIG, PCI_PRODUCT_SIIG_1030, 0, 0],
        [0xffff, 0xffff, 0, 0],
        &[
            PucPort::new(PUC_PORT_COM, 0x18, 0x0000),
            PucPort::new(PUC_PORT_COM, 0x1c, 0x0000),
        ],
    ),
    // SIIG Cyber Serial Dual PCI 16C650 (10x family): 2S
    // "SIIG Cyber Serial Dual PCI 16C650 (10x family)",
    PucDeviceDescription::new(
        [PCI_VENDOR_SIIG, PCI_PRODUCT_SIIG_1031, 0, 0],
        [0xffff, 0xffff, 0, 0],
        &[
            PucPort::new(PUC_PORT_COM, 0x18, 0x0000),
            PucPort::new(PUC_PORT_COM, 0x1c, 0x0000),
        ],
    ),
    // SIIG Cyber Serial Dual PCI 16C850 (10x family): 2S
    // "SIIG Cyber Serial Dual PCI 16C850 (10x family)",
    PucDeviceDescription::new(
        [PCI_VENDOR_SIIG, PCI_PRODUCT_SIIG_1032, 0, 0],
        [0xffff, 0xffff, 0, 0],
        &[
            PucPort::new(PUC_PORT_COM, 0x18, 0x0000),
            PucPort::new(PUC_PORT_COM, 0x1c, 0x0000),
        ],
    ),
    // SIIG Cyber 2S1P PCI 16C550 (10x family): 2S, 1P
    // "SIIG Cyber 2S1P PCI 16C550 (10x family)",
    PucDeviceDescription::new(
        [PCI_VENDOR_SIIG, PCI_PRODUCT_SIIG_1034, 0, 0],
        [0xffff, 0xffff, 0, 0],
        &[
            PucPort::new(PUC_PORT_COM, 0x18, 0x0000),
            PucPort::new(PUC_PORT_COM, 0x1c, 0x0000),
            PucPort::new(PUC_PORT_LPT, 0x20, 0x0000),
        ],
    ),
    // SIIG Cyber 2S1P PCI 16C650 (10x family): 2S, 1P
    // "SIIG Cyber 2S1P PCI 16C650 (10x family)",
    PucDeviceDescription::new(
        [PCI_VENDOR_SIIG, PCI_PRODUCT_SIIG_1035, 0, 0],
        [0xffff, 0xffff, 0, 0],
        &[
            PucPort::new(PUC_PORT_COM, 0x18, 0x0000),
            PucPort::new(PUC_PORT_COM, 0x1c, 0x0000),
            PucPort::new(PUC_PORT_LPT, 0x20, 0x0000),
        ],
    ),
    // SIIG Cyber 2S1P PCI 16C850 (10x family): 2S, 1P
    // "SIIG Cyber 2S1P PCI 16C850 (10x family)",
    PucDeviceDescription::new(
        [PCI_VENDOR_SIIG, PCI_PRODUCT_SIIG_1036, 0, 0],
        [0xffff, 0xffff, 0, 0],
        &[
            PucPort::new(PUC_PORT_COM, 0x18, 0x0000),
            PucPort::new(PUC_PORT_COM, 0x1c, 0x0000),
            PucPort::new(PUC_PORT_LPT, 0x20, 0x0000),
        ],
    ),
    // SIIG Cyber 4S PCI 16C550 (10x family): 4S
    // "SIIG Cyber 4S PCI 16C550 (10x family)",
    PucDeviceDescription::new(
        [PCI_VENDOR_SIIG, PCI_PRODUCT_SIIG_1050, 0, 0],
        [0xffff, 0xffff, 0, 0],
        &[
            PucPort::new(PUC_PORT_COM, 0x18, 0x0000),
            PucPort::new(PUC_PORT_COM, 0x1c, 0x0000),
            PucPort::new(PUC_PORT_COM, 0x20, 0x0000),
            PucPort::new(PUC_PORT_COM, 0x24, 0x0000),
        ],
    ),
    // SIIG Cyber 4S PCI 16C650 (10x family): 4S
    // "SIIG Cyber 4S PCI 16C650 (10x family)",
    PucDeviceDescription::new(
        [PCI_VENDOR_SIIG, PCI_PRODUCT_SIIG_1051, 0, 0],
        [0xffff, 0xffff, 0, 0],
        &[
            PucPort::new(PUC_PORT_COM, 0x18, 0x0000),
            PucPort::new(PUC_PORT_COM, 0x1c, 0x0000),
            PucPort::new(PUC_PORT_COM, 0x20, 0x0000),
            PucPort::new(PUC_PORT_COM, 0x24, 0x0000),
        ],
    ),
    // SIIG Cyber 4S PCI 16C850 (10x family): 4S
    // "SIIG Cyber 4S PCI 16C850 (10x family)",
    PucDeviceDescription::new(
        [PCI_VENDOR_SIIG, PCI_PRODUCT_SIIG_1052, 0, 0],
        [0xffff, 0xffff, 0, 0],
        &[
            PucPort::new(PUC_PORT_COM, 0x18, 0x0000),
            PucPort::new(PUC_PORT_COM, 0x1c, 0x0000),
            PucPort::new(PUC_PORT_COM, 0x20, 0x0000),
            PucPort::new(PUC_PORT_COM, 0x24, 0x0000),
        ],
    ),
    // SIIG "20x" family boards.
    // SIIG Cyber Parallel PCI (20x family): 1P
    // "SIIG Cyber Parallel PCI (20x family)",
    PucDeviceDescription::new(
        [PCI_VENDOR_SIIG, PCI_PRODUCT_SIIG_2020, 0, 0],
        [0xffff, 0xffff, 0, 0],
        &[PucPort::new(PUC_PORT_LPT, 0x10, 0x0000)],
    ),
    // SIIG Cyber Parallel Dual PCI (20x family): 2P
    // "SIIG Cyber Parallel Dual PCI (20x family)",
    PucDeviceDescription::new(
        [PCI_VENDOR_SIIG, PCI_PRODUCT_SIIG_2021, 0, 0],
        [0xffff, 0xffff, 0, 0],
        &[
            PucPort::new(PUC_PORT_LPT, 0x10, 0x0000),
            PucPort::new(PUC_PORT_LPT, 0x18, 0x0000),
        ],
    ),
    // SIIG Cyber 2P1S PCI 16C550 (20x family): 1S, 2P
    // "SIIG Cyber 2P1S PCI 16C550 (20x family)",
    PucDeviceDescription::new(
        [PCI_VENDOR_SIIG, PCI_PRODUCT_SIIG_2040, 0, 0],
        [0xffff, 0xffff, 0, 0],
        &[
            PucPort::new(PUC_PORT_COM, 0x10, 0x0000),
            PucPort::new(PUC_PORT_LPT, 0x14, 0x0000),
            PucPort::new(PUC_PORT_LPT, 0x1c, 0x0000),
        ],
    ),
    // SIIG Cyber 2P1S PCI 16C650 (20x family): 1S, 2P
    // "SIIG Cyber 2P1S PCI 16C650 (20x family)",
    PucDeviceDescription::new(
        [PCI_VENDOR_SIIG, PCI_PRODUCT_SIIG_2041, 0, 0],
        [0xffff, 0xffff, 0, 0],
        &[
            PucPort::new(PUC_PORT_COM, 0x10, 0x0000),
            PucPort::new(PUC_PORT_LPT, 0x14, 0x0000),
            PucPort::new(PUC_PORT_LPT, 0x1c, 0x0000),
        ],
    ),
    // SIIG Cyber 2P1S PCI 16C850 (20x family): 1S, 2P
    // "SIIG Cyber 2P1S PCI 16C850 (20x family)",
    PucDeviceDescription::new(
        [PCI_VENDOR_SIIG, PCI_PRODUCT_SIIG_2042, 0, 0],
        [0xffff, 0xffff, 0, 0],
        &[
            PucPort::new(PUC_PORT_COM, 0x10, 0x0000),
            PucPort::new(PUC_PORT_LPT, 0x14, 0x0000),
            PucPort::new(PUC_PORT_LPT, 0x1c, 0x0000),
        ],
    ),
    // SIIG Cyber Serial PCI 16C550 (20x family): 1S
    // "SIIG Cyber Serial PCI 16C550 (20x family)",
    PucDeviceDescription::new(
        [PCI_VENDOR_SIIG, PCI_PRODUCT_SIIG_2000, 0, 0],
        [0xffff, 0xffff, 0, 0],
        &[PucPort::new(PUC_PORT_COM, 0x10, 0x0000)],
    ),
    // SIIG Cyber Serial PCI 16C650 (20x family): 1S
    // "SIIG Cyber Serial PCI 16C650 (20x family)",
    PucDeviceDescription::new(
        [PCI_VENDOR_SIIG, PCI_PRODUCT_SIIG_2001, 0, 0],
        [0xffff, 0xffff, 0, 0],
        &[PucPort::new(PUC_PORT_COM, 0x10, 0x0000)],
    ),
    // SIIG Cyber Serial PCI 16C850 (20x family): 1S
    // "SIIG Cyber Serial PCI 16C850 (20x family)",
    PucDeviceDescription::new(
        [PCI_VENDOR_SIIG, PCI_PRODUCT_SIIG_2002, 0, 0],
        [0xffff, 0xffff, 0, 0],
        &[PucPort::new(PUC_PORT_COM, 0x10, 0x0000)],
    ),
    // SIIG Cyber I/O PCI 16C550 (20x family): 1S, 1P
    // "SIIG Cyber I/O PCI 16C550 (20x family)",
    PucDeviceDescription::new(
        [PCI_VENDOR_SIIG, PCI_PRODUCT_SIIG_2010, 0, 0],
        [0xffff, 0xffff, 0, 0],
        &[
            PucPort::new(PUC_PORT_COM, 0x10, 0x0000),
            PucPort::new(PUC_PORT_LPT, 0x14, 0x0000),
        ],
    ),
    // SIIG Cyber I/O PCI 16C650 (20x family): 1S, 1P
    // "SIIG Cyber I/O PCI 16C650 (20x family)",
    PucDeviceDescription::new(
        [PCI_VENDOR_SIIG, PCI_PRODUCT_SIIG_2011, 0, 0],
        [0xffff, 0xffff, 0, 0],
        &[
            PucPort::new(PUC_PORT_COM, 0x10, 0x0000),
            PucPort::new(PUC_PORT_LPT, 0x14, 0x0000),
        ],
    ),
    // SIIG Cyber I/O PCI 16C850 (20x family): 1S, 1P
    // "SIIG Cyber I/O PCI 16C850 (20x family)",
    PucDeviceDescription::new(
        [PCI_VENDOR_SIIG, PCI_PRODUCT_SIIG_2012, 0, 0],
        [0xffff, 0xffff, 0, 0],
        &[
            PucPort::new(PUC_PORT_COM, 0x10, 0x0000),
            PucPort::new(PUC_PORT_LPT, 0x14, 0x0000),
        ],
    ),
    // SIIG Cyber Serial Dual PCI 16C550 (20x family): 2S
    // "SIIG Cyber Serial Dual PCI 16C550 (20x family)",
    PucDeviceDescription::new(
        [PCI_VENDOR_SIIG, PCI_PRODUCT_SIIG_2030, 0, 0],
        [0xffff, 0xffff, 0, 0],
        &[
            PucPort::new(PUC_PORT_COM, 0x10, 0x0000),
            PucPort::new(PUC_PORT_COM, 0x14, 0x0000),
        ],
    ),
    // SIIG Cyber Serial Dual PCI 16C650 (20x family): 2S
    // "SIIG Cyber Serial Dual PCI 16C650 (20x family)",
    PucDeviceDescription::new(
        [PCI_VENDOR_SIIG, PCI_PRODUCT_SIIG_2031, 0, 0],
        [0xffff, 0xffff, 0, 0],
        &[
            PucPort::new(PUC_PORT_COM, 0x10, 0x0000),
            PucPort::new(PUC_PORT_COM, 0x14, 0x0000),
        ],
    ),
    // SIIG Cyber Serial Dual PCI 16C850 (20x family): 2S
    // "SIIG Cyber Serial Dual PCI 16C850 (20x family)",
    PucDeviceDescription::new(
        [PCI_VENDOR_SIIG, PCI_PRODUCT_SIIG_2032, 0, 0],
        [0xffff, 0xffff, 0, 0],
        &[
            PucPort::new(PUC_PORT_COM, 0x10, 0x0000),
            PucPort::new(PUC_PORT_COM, 0x14, 0x0000),
        ],
    ),
    // SIIG Cyber 2S1P PCI 16C550 (20x family): 2S, 1P
    // "SIIG Cyber 2S1P PCI 16C550 (20x family)",
    PucDeviceDescription::new(
        [PCI_VENDOR_SIIG, PCI_PRODUCT_SIIG_2060, 0, 0],
        [0xffff, 0xffff, 0, 0],
        &[
            PucPort::new(PUC_PORT_COM, 0x10, 0x0000),
            PucPort::new(PUC_PORT_COM, 0x14, 0x0000),
            PucPort::new(PUC_PORT_LPT, 0x18, 0x0000),
        ],
    ),
    // SIIG Cyber 2S1P PCI 16C650 (20x family): 2S, 1P
    // "SIIG Cyber 2S1P PCI 16C650 (20x family)",
    PucDeviceDescription::new(
        [PCI_VENDOR_SIIG, PCI_PRODUCT_SIIG_2061, 0, 0],
        [0xffff, 0xffff, 0, 0],
        &[
            PucPort::new(PUC_PORT_COM, 0x10, 0x0000),
            PucPort::new(PUC_PORT_COM, 0x14, 0x0000),
            PucPort::new(PUC_PORT_LPT, 0x18, 0x0000),
        ],
    ),
    // SIIG Cyber 2S1P PCI 16C850 (20x family): 2S, 1P
    // "SIIG Cyber 2S1P PCI 16C850 (20x family)",
    PucDeviceDescription::new(
        [PCI_VENDOR_SIIG, PCI_PRODUCT_SIIG_2062, 0, 0],
        [0xffff, 0xffff, 0, 0],
        &[
            PucPort::new(PUC_PORT_COM, 0x10, 0x0000),
            PucPort::new(PUC_PORT_COM, 0x14, 0x0000),
            PucPort::new(PUC_PORT_LPT, 0x18, 0x0000),
        ],
    ),
    // SIIG Cyber 4S PCI 16C550 (20x family): 4S
    // "SIIG Cyber 4S PCI 16C550 (20x family)",
    PucDeviceDescription::new(
        [PCI_VENDOR_SIIG, PCI_PRODUCT_SIIG_2050, 0, 0],
        [0xffff, 0xffff, 0, 0],
        &[
            PucPort::new(PUC_PORT_COM, 0x10, 0x0000),
            PucPort::new(PUC_PORT_COM, 0x14, 0x0000),
            PucPort::new(PUC_PORT_COM, 0x18, 0x0000),
            PucPort::new(PUC_PORT_COM, 0x1c, 0x0000),
        ],
    ),
    // SIIG Cyber 4S PCI 16C650 (20x family): 4S
    // "SIIG Cyber 4S PCI 16C650 (20x family)",
    PucDeviceDescription::new(
        [PCI_VENDOR_SIIG, PCI_PRODUCT_SIIG_2051, 0, 0],
        [0xffff, 0xffff, 0, 0],
        &[
            PucPort::new(PUC_PORT_COM, 0x10, 0x0000),
            PucPort::new(PUC_PORT_COM, 0x14, 0x0000),
            PucPort::new(PUC_PORT_COM, 0x18, 0x0000),
            PucPort::new(PUC_PORT_COM, 0x1c, 0x0000),
        ],
    ),
    // SIIG Cyber 4S PCI 16C850 (20x family): 4S
    // "SIIG Cyber 4S PCI 16C850 (20x family)",
    PucDeviceDescription::new(
        [PCI_VENDOR_SIIG, PCI_PRODUCT_SIIG_2052, 0, 0],
        [0xffff, 0xffff, 0, 0],
        &[
            PucPort::new(PUC_PORT_COM, 0x10, 0x0000),
            PucPort::new(PUC_PORT_COM, 0x14, 0x0000),
            PucPort::new(PUC_PORT_COM, 0x18, 0x0000),
            PucPort::new(PUC_PORT_COM, 0x1c, 0x0000),
        ],
    ),
    // SIIG Cyber 8S PCI 16C850 (20x family): 8S
    // "SIIG Cyber 8S PCI 16C850 (20x family)",
    PucDeviceDescription::new(
        [PCI_VENDOR_SIIG, PCI_PRODUCT_SIIG_2081, 0, 0],
        [0xffff, 0xffff, 0, 0],
        &[
            PucPort::new(PUC_PORT_COM, 0x10, 0x0000),
            PucPort::new(PUC_PORT_COM, 0x14, 0x0000),
            PucPort::new(PUC_PORT_COM, 0x18, 0x0000),
            PucPort::new(PUC_PORT_COM, 0x1c, 0x0000),
            PucPort::new(PUC_PORT_COM, 0x20, 0x0000),
            PucPort::new(PUC_PORT_COM, 0x20, 0x0008),
            PucPort::new(PUC_PORT_COM, 0x20, 0x0010),
            PucPort::new(PUC_PORT_COM, 0x20, 0x0018),
        ],
    ),
    // SIIG Cyber 8S PCI 16C850 (20x family): 8S
    // "SIIG Cyber 8S PCI 16C850 (20x family)",
    PucDeviceDescription::new(
        [
            PCI_VENDOR_OXFORD2,
            PCI_PRODUCT_OXFORD2_OX16PCI954,
            PCI_VENDOR_SIIG,
            PCI_PRODUCT_SIIG_2082,
        ],
        [0xffff, 0xffff, 0xffff, 0xffff],
        &[
            PucPort::new(PUC_PORT_COM_MUL10, 0x10, 0x0000),
            PucPort::new(PUC_PORT_COM_MUL10, 0x10, 0x0008),
            PucPort::new(PUC_PORT_COM_MUL10, 0x10, 0x0010),
            PucPort::new(PUC_PORT_COM_MUL10, 0x10, 0x0018),
        ],
    ),
    // OX16PCI954, first part of Serial Technologies Expander PCI-232-108
    // "OX16PCI954"
    PucDeviceDescription::new(
        [
            PCI_VENDOR_OXFORD2,
            PCI_PRODUCT_OXFORD2_OX16PCI954,
            PCI_VENDOR_OXFORD2,
            0,
        ],
        [0xffff, 0xffff, 0xffff, 0xffff],
        &[
            PucPort::new(PUC_PORT_COM_MUL8, 0x10, 0x0000),
            PucPort::new(PUC_PORT_COM_MUL8, 0x10, 0x0008),
            PucPort::new(PUC_PORT_COM_MUL8, 0x10, 0x0010),
            PucPort::new(PUC_PORT_COM_MUL8, 0x10, 0x0018),
        ],
    ),
    // Exsys EX-41092 (sold as SIIG JJ-E10011-S3)
    // "Exsys EX-41092",
    PucDeviceDescription::new(
        [
            PCI_VENDOR_OXFORD2,
            PCI_PRODUCT_OXFORD2_EXSYS_EX41092,
            0x0000,
            0x0000,
        ],
        [0xffff, 0xffff, 0x0000, 0x0000],
        &[PucPort::new(PUC_PORT_COM_MUL10, 0x10, 0x0000)],
    ),
    // Exsys EX-41098, second part of Serial Technologies Expander PCI-232-108
    // "Exsys EX-41098",
    PucDeviceDescription::new(
        [
            PCI_VENDOR_OXFORD2,
            PCI_PRODUCT_OXFORD2_EXSYS_EX41098,
            PCI_VENDOR_OXFORD2,
            0,
        ],
        [0xffff, 0xffff, 0xffff, 0xffff],
        &[
            PucPort::new(PUC_PORT_COM_MUL8, 0x10, 0x0000),
            PucPort::new(PUC_PORT_COM_MUL8, 0x10, 0x0008),
            PucPort::new(PUC_PORT_COM_MUL8, 0x10, 0x0010),
            PucPort::new(PUC_PORT_COM_MUL8, 0x10, 0x0018),
        ],
    ),
    // Exsys EX-41098, second part of SIIG Cyber 8S PCI Card
    // "Exsys EX-41098",
    PucDeviceDescription::new(
        [
            PCI_VENDOR_OXFORD2,
            PCI_PRODUCT_OXFORD2_EXSYS_EX41098,
            PCI_VENDOR_SIIG,
            PCI_PRODUCT_SIIG_2082,
        ],
        [0xffff, 0xffff, 0xffff, 0xffff],
        &[
            PucPort::new(PUC_PORT_COM_MUL10, 0x10, 0x0000),
            PucPort::new(PUC_PORT_COM_MUL10, 0x10, 0x0008),
            PucPort::new(PUC_PORT_COM_MUL10, 0x10, 0x0010),
            PucPort::new(PUC_PORT_COM_MUL10, 0x10, 0x0018),
        ],
    ),
    // VScom PCI-400S, based on PLX 9050 Chip, 16k buffer
    // "VScom PCI-400S",
    PucDeviceDescription::new(
        [PCI_VENDOR_PLX, PCI_PRODUCT_PLX_1077, 0x10b5, 0x1077],
        [0xffff, 0xffff, 0xffff, 0xffff],
        &[
            PucPort::new(PUC_PORT_COM_MUL8, 0x18, 0x0000),
            PucPort::new(PUC_PORT_COM_MUL8, 0x18, 0x0008),
            PucPort::new(PUC_PORT_COM_MUL8, 0x18, 0x0010),
            PucPort::new(PUC_PORT_COM_MUL8, 0x18, 0x0018),
        ],
    ),
    // VScom PCI-800, as sold on http://www.swann.com.au/isp/titan.html.
    // Some PLX chip.  Note: This board has a software selectable(?)
    // clock multiplier which this driver doesn't support, so you'll
    // have to use an appropriately scaled baud rate when talking to
    // the card.
    // "VScom PCI-800",
    PucDeviceDescription::new(
        [PCI_VENDOR_PLX, PCI_PRODUCT_PLX_1076, 0x10b5, 0x1076],
        [0xffff, 0xffff, 0xffff, 0xffff],
        &[
            PucPort::new(PUC_PORT_COM, 0x18, 0x0000),
            PucPort::new(PUC_PORT_COM, 0x18, 0x0008),
            PucPort::new(PUC_PORT_COM, 0x18, 0x0010),
            PucPort::new(PUC_PORT_COM, 0x18, 0x0018),
            PucPort::new(PUC_PORT_COM, 0x18, 0x0020),
            PucPort::new(PUC_PORT_COM, 0x18, 0x0028),
            PucPort::new(PUC_PORT_COM, 0x18, 0x0030),
            PucPort::new(PUC_PORT_COM, 0x18, 0x0038),
        ],
    ),
    // VScom PCI 011H, 1 lpt.
    // "VScom PCI-011H",
    PucDeviceDescription::new(
        [PCI_VENDOR_OXFORD2, PCI_PRODUCT_OXFORD2_VSCOM_PCI011H, 0, 0],
        [0xffff, 0xffff, 0, 0],
        &[PucPort::new(PUC_PORT_LPT, 0x10, 0x0000)],
    ),
    // VScom PCI x10H, 1 lpt.
    // is the lpt part of VScom 110H, 210H, 410H
    // "VScom PCI-x10H",
    PucDeviceDescription::new(
        [PCI_VENDOR_OXFORD, PCI_PRODUCT_OXFORD_VSCOM_PCIX10H, 0, 0],
        [0xffff, 0xffff, 0, 0],
        &[PucPort::new(PUC_PORT_LPT, 0x10, 0x0000)],
    ),
    // VScom PCI 100H, little sister of 800H, 1 com.
    // also com part of VScom 110H
    // The one I have defaults to a frequency of 14.7456 MHz which is
    // jumper J1 set to 2-3.
    // "VScom PCI-100H",
    PucDeviceDescription::new(
        [PCI_VENDOR_OXFORD, PCI_PRODUCT_OXFORD_VSCOM_PCI100H, 0, 0],
        [0xffff, 0xffff, 0, 0],
        &[PucPort::new(PUC_PORT_COM_MUL8, 0x10, 0x0000)],
    ),
    // VScom PCI 200H, little sister of 800H, 2 com.
    // also com part of VScom 210H
    // The one I have defaults to a frequency of 14.7456 MHz which is
    // jumper J1 set to 2-3.
    // "VScom PCI-200H",
    PucDeviceDescription::new(
        [PCI_VENDOR_OXFORD, PCI_PRODUCT_OXFORD_VSCOM_PCI200H, 0, 0],
        [0xffff, 0xffff, 0, 0],
        &[
            PucPort::new(PUC_PORT_COM_MUL8, 0x10, 0x0000),
            PucPort::new(PUC_PORT_COM_MUL8, 0x10, 0x0008),
        ],
    ),
    // VScom PCI 400H and 800H. Uses 4/8 16950 UART, behind a PCI chips
    // that offers 4 com port on PCI device 0 (both 400H and 800H)
    // and 4 on PCI device 1 (800H only). PCI device 0 has
    // device ID 3 and PCI device 1 device ID 4. Uses a 14.7456 MHz crystal
    // instead of the standard 1.8432MHz.
    // There's a version with a jumper for selecting the crystal frequency,
    // defaults to 8x as used here. The jumperless version uses 8x, too.
    // "VScom PCI-400H/800H",
    PucDeviceDescription::new(
        [PCI_VENDOR_OXFORD, PCI_PRODUCT_OXFORD_VSCOM_PCI800H_0, 0, 0],
        [0xffff, 0xffff, 0, 0],
        &[
            PucPort::new(PUC_PORT_COM_MUL8, 0x10, 0x0000),
            PucPort::new(PUC_PORT_COM_MUL8, 0x10, 0x0008),
            PucPort::new(PUC_PORT_COM_MUL8, 0x10, 0x0010),
            PucPort::new(PUC_PORT_COM_MUL8, 0x10, 0x0018),
        ],
    ),
    // "VScom PCI-400H/800H",
    PucDeviceDescription::new(
        [PCI_VENDOR_OXFORD, PCI_PRODUCT_OXFORD_VSCOM_PCI800H_1, 0, 0],
        [0xffff, 0xffff, 0, 0],
        &[
            PucPort::new(PUC_PORT_COM_MUL8, 0x10, 0x0000),
            PucPort::new(PUC_PORT_COM_MUL8, 0x10, 0x0008),
            PucPort::new(PUC_PORT_COM_MUL8, 0x10, 0x0010),
            PucPort::new(PUC_PORT_COM_MUL8, 0x10, 0x0018),
        ],
    ),
    // VScom PCI 200HV2, is 200H Version 2.
    // Sells as 200H
    // "VScom PCI-200HV2",
    PucDeviceDescription::new(
        [PCI_VENDOR_OXFORD, PCI_PRODUCT_OXFORD_VSCOM_PCI200HV2, 0, 0],
        [0xffff, 0xffff, 0, 0],
        &[
            PucPort::new(PUC_PORT_COM_MUL8, 0x10, 0x0000),
            PucPort::new(PUC_PORT_COM_MUL8, 0x14, 0x0000),
        ],
    ),
    // VScom PCI 010L
    // one lpt
    // untested
    // "VScom PCI-010L",
    PucDeviceDescription::new(
        [PCI_VENDOR_OXFORD, PCI_PRODUCT_OXFORD_VSCOM_PCI010L, 0, 0],
        [0xffff, 0xffff, 0, 0],
        &[PucPort::new(PUC_PORT_LPT, 0x1c, 0x0000)],
    ),
    // VScom PCI 100L
    // one com
    // The one I have defaults to a frequency of 14.7456 MHz which is
    // jumper J1 set to 2-3.
    // "VScom PCI-100L",
    PucDeviceDescription::new(
        [PCI_VENDOR_OXFORD, PCI_PRODUCT_OXFORD_VSCOM_PCI100L, 0, 0],
        [0xffff, 0xffff, 0, 0],
        &[PucPort::new(PUC_PORT_COM_MUL8, 0x14, 0x0000)],
    ),
    // VScom PCI 110L
    // one com, one lpt
    // untested
    // "VScom PCI-110L",
    PucDeviceDescription::new(
        [PCI_VENDOR_OXFORD, PCI_PRODUCT_OXFORD_VSCOM_PCI110L, 0, 0],
        [0xffff, 0xffff, 0, 0],
        &[
            PucPort::new(PUC_PORT_COM_MUL8, 0x14, 0x0000),
            PucPort::new(PUC_PORT_LPT, 0x1c, 0x0000),
        ],
    ),
    // VScom PCI-200L has 2 x 16550 UARTS.
    // The board has a jumper which allows you to select a clock speed
    // of either 14.7456MHz or 1.8432MHz. By default it runs at
    // the fast speed.
    // "VScom PCI-200L with 2 x 16550 UARTS"
    PucDeviceDescription::new(
        [PCI_VENDOR_OXFORD, PCI_PRODUCT_OXFORD_VSCOM_PCI200L, 0, 0],
        [0xffff, 0xffff, 0, 0],
        &[
            PucPort::new(PUC_PORT_COM_MUL8, 0x14, 0x0000),
            PucPort::new(PUC_PORT_COM_MUL8, 0x18, 0x0000),
        ],
    ),
    // VScom PCI-210L
    // Has a jumper for frequency selection, defaults to 8x as used here
    // two com, one lpt
    // "VScom PCI-210L"
    PucDeviceDescription::new(
        [PCI_VENDOR_OXFORD, PCI_PRODUCT_OXFORD_VSCOM_PCI210L, 0, 0],
        [0xffff, 0xffff, 0, 0],
        &[
            PucPort::new(PUC_PORT_COM_MUL8, 0x14, 0x0000),
            PucPort::new(PUC_PORT_COM_MUL8, 0x18, 0x0000),
            PucPort::new(PUC_PORT_LPT, 0x1c, 0x0000),
        ],
    ),
    // VScom PCI 400L
    // Has a jumper for frequency selection, defaults to 8x as used here
    // This is equal to J1 in pos 2-3
    // VendorID mismatch with docs, should be 14d2 (oxford), is 10d2 (molex)
    // "VScom PCI-400L",
    PucDeviceDescription::new(
        [PCI_VENDOR_MOLEX, PCI_PRODUCT_MOLEX_VSCOM_PCI400L, 0, 0],
        [0xffff, 0xffff, 0, 0],
        &[
            PucPort::new(PUC_PORT_COM_MUL8, 0x14, 0x0000),
            PucPort::new(PUC_PORT_COM_MUL8, 0x18, 0x0000),
            PucPort::new(PUC_PORT_COM_MUL8, 0x20, 0x0000),
            PucPort::new(PUC_PORT_COM_MUL8, 0x20, 0x0008),
        ],
    ),
    // VScom PCI 800L
    // Has a jumper for frequency selection, defaults to 8x as used here
    // "VScom PCI-800L",
    PucDeviceDescription::new(
        [PCI_VENDOR_OXFORD, PCI_PRODUCT_OXFORD_VSCOM_PCI800L, 0, 0],
        [0xffff, 0xffff, 0, 0],
        &[
            PucPort::new(PUC_PORT_COM_MUL8, 0x14, 0x0000),
            PucPort::new(PUC_PORT_COM_MUL8, 0x18, 0x0000),
            PucPort::new(PUC_PORT_COM_MUL8, 0x20, 0x0000),
            PucPort::new(PUC_PORT_COM_MUL8, 0x20, 0x0008),
            PucPort::new(PUC_PORT_COM_MUL8, 0x20, 0x0010),
            PucPort::new(PUC_PORT_COM_MUL8, 0x20, 0x0018),
            PucPort::new(PUC_PORT_COM_MUL8, 0x20, 0x0020),
            PucPort::new(PUC_PORT_COM_MUL8, 0x20, 0x0028),
        ],
    ),
    // Exsys EX-41098
    // "Exsys EX-41098",
    PucDeviceDescription::new(
        [PCI_VENDOR_OXFORD2, PCI_PRODUCT_OXFORD2_EXSYS_EX41098, 0, 0],
        [0xffff, 0xffff, 0, 0],
        &[
            PucPort::new(PUC_PORT_COM, 0x10, 0x0000),
            PucPort::new(PUC_PORT_COM, 0x10, 0x0008),
            PucPort::new(PUC_PORT_COM, 0x10, 0x0010),
            PucPort::new(PUC_PORT_COM, 0x10, 0x0018),
        ],
    ),
    // Boards with an Oxford Semiconductor chip.
    // Oxford Semiconductor provides documentation for their chip at:
    // <URL:http://www.plxtech.com/products/uart/>
    // As sold by Kouwell <URL:http://www.kouwell.com/>.
    // I/O Flex PCI I/O Card Model-223 with 4 serial and 1 parallel ports.
    // Exsys EX-1372 (uses Oxford OX16PCI952 and a 8x clock)
    // "Oxford Semiconductor OX16PCI952 UARTs",
    PucDeviceDescription::new(
        [
            PCI_VENDOR_OXFORD2,
            PCI_PRODUCT_OXFORD2_OX16PCI952,
            PCI_VENDOR_OXFORD2,
            0x0001,
        ],
        [0xffff, 0xffff, 0xffff, 0xffff],
        &[
            PucPort::new(PUC_PORT_COM_MUL8, 0x10, 0x0000),
            PucPort::new(PUC_PORT_COM_MUL8, 0x14, 0x0000),
        ],
    ),
    // Oxford Semiconductor OX16PCI952 PCI `950 UARTs - 128 byte FIFOs
    // "Oxford Semiconductor OX16PCI952 UARTs",
    PucDeviceDescription::new(
        [PCI_VENDOR_OXFORD2, PCI_PRODUCT_OXFORD2_OX16PCI952, 0, 0],
        [0xffff, 0xffff, 0, 0],
        &[
            PucPort::new(PUC_PORT_COM, 0x10, 0x0000),
            PucPort::new(PUC_PORT_COM, 0x14, 0x0000),
        ],
    ),
    // Oxford Semiconductor OX16PCI952 PCI Parallel port
    // "Oxford Semiconductor OX16PCI952 Parallel port",
    PucDeviceDescription::new(
        [PCI_VENDOR_OXFORD2, PCI_PRODUCT_OXFORD2_OX16PCI952P, 0, 0],
        [0xffff, 0xffff, 0, 0],
        &[PucPort::new(PUC_PORT_LPT, 0x10, 0x0000)],
    ),
    // Oxford Semiconductor OXPCIE952 PCIE Parallel port
    // "Oxford Semiconductor OXPCIE952 PCIE Parallel port",
    PucDeviceDescription::new(
        [PCI_VENDOR_OXFORD2, PCI_PRODUCT_OXFORD2_OXPCIE952, 0, 0],
        [0xffff, 0xffff, 0, 0],
        &[PucPort::new(PUC_PORT_LPT, 0x10, 0x0000)],
    ),
    // SIIG 2050 (uses Oxford 16PCI954 and a 10x clock)
    // "Oxford Semiconductor OX16PCI954 UARTs",
    PucDeviceDescription::new(
        [
            PCI_VENDOR_OXFORD2,
            PCI_PRODUCT_OXFORD2_OX16PCI954,
            PCI_VENDOR_SIIG,
            PCI_PRODUCT_SIIG_2050,
        ],
        [0xffff, 0xffff, 0xffff, 0xffff],
        &[
            PucPort::new(PUC_PORT_COM_MUL10, 0x10, 0x0000),
            PucPort::new(PUC_PORT_COM_MUL10, 0x10, 0x0008),
            PucPort::new(PUC_PORT_COM_MUL10, 0x10, 0x0010),
            PucPort::new(PUC_PORT_COM_MUL10, 0x10, 0x0018),
        ],
    ),
    // I-O DATA RSA-PCI2 (uses Oxford 16PCI954 and a 8x clock)
    // "Oxford Semiconductor OX16PCI954 UARTs",
    PucDeviceDescription::new(
        [
            PCI_VENDOR_OXFORD2,
            PCI_PRODUCT_OXFORD2_OX16PCI954,
            PCI_VENDOR_IODATA,
            0xc070,
        ],
        [0xffff, 0xffff, 0xffff, 0xffff],
        &[
            PucPort::new(PUC_PORT_COM_MUL8, 0x10, 0x0000),
            PucPort::new(PUC_PORT_COM_MUL8, 0x10, 0x0008),
        ],
    ),
    // Oxford Semiconductor OX16PCI954 PCI UARTs
    // "Oxford Semiconductor OX16PCI954 UARTs",
    PucDeviceDescription::new(
        [PCI_VENDOR_OXFORD2, PCI_PRODUCT_OXFORD2_OX16PCI954, 0, 0],
        [0xffff, 0xffff, 0, 0],
        &[
            PucPort::new(PUC_PORT_COM, 0x10, 0x0000),
            PucPort::new(PUC_PORT_COM, 0x10, 0x0008),
            PucPort::new(PUC_PORT_COM, 0x10, 0x0010),
            PucPort::new(PUC_PORT_COM, 0x10, 0x0018),
        ],
    ),
    // Commell MP-954GPS, GPS and 2 COM
    // "Oxford Semiconductor OX16mPCI954 UARTs",
    PucDeviceDescription::new(
        [PCI_VENDOR_OXFORD2, PCI_PRODUCT_OXFORD2_OXMPCI954, 0, 0],
        [0xffff, 0xffff, 0, 0],
        &[
            PucPort::new(PUC_PORT_COM_MUL4, 0x10, 0x0000),
            PucPort::new(PUC_PORT_COM_MUL4, 0x10, 0x0008),
            PucPort::new(PUC_PORT_COM_MUL4, 0x10, 0x0010),
        ],
    ),
    // Oxford Semiconductor OX16PCI954K PCI UARTs
    // "Oxford Semiconductor OX16PCI954K UARTs",
    PucDeviceDescription::new(
        [PCI_VENDOR_OXFORD2, PCI_PRODUCT_OXFORD2_OX16PCI954K, 0, 0],
        [0xffff, 0xffff, 0, 0],
        &[
            PucPort::new(PUC_PORT_COM, 0x10, 0x0000),
            PucPort::new(PUC_PORT_COM, 0x14, 0x0000),
            PucPort::new(PUC_PORT_COM, 0x18, 0x0000),
            PucPort::new(PUC_PORT_COM, 0x1c, 0x0000),
        ],
    ),
    // Oxford Semiconductor OX16PCI954 PCI Parallel port
    // "Oxford Semiconductor OX16PCI954 Parallel port",
    PucDeviceDescription::new(
        [PCI_VENDOR_OXFORD2, PCI_PRODUCT_OXFORD2_OX16PCI954P, 0, 0],
        [0xffff, 0xffff, 0, 0],
        &[PucPort::new(PUC_PORT_LPT, 0x10, 0x0000)],
    ),
    // Oxford Semiconductor PCIE `950 UARTs - 128 byte FIFOs
    // "Oxford Semiconductor PCIE UARTs",
    PucDeviceDescription::new(
        [PCI_VENDOR_OXFORD2, PCI_PRODUCT_OXFORD2_OXPCIE952S, 0, 0],
        [0xffff, 0xffff, 0, 0],
        &[PucPort::new(PUC_PORT_COM, 0x10, 0x0000)],
    ),
    // Brainboxes BB16PCI958.
    // Apparently based on an Oxford Semiconductor OX16PCI958 chip.
    // "Brainboxes BB16PCI958 UARTs",
    PucDeviceDescription::new(
        [
            PCI_VENDOR_BRAINBOXES,
            PCI_PRODUCT_BRAINBOXES_IS200_BB16PCI958,
            0,
            0,
        ],
        [0xffff, 0xffff, 0, 0],
        &[
            PucPort::new(PUC_PORT_COM, 0x18, 0x0000),
            PucPort::new(PUC_PORT_COM, 0x18, 0x0008),
        ],
    ),
    // NEC PK-UG-X001 K56flex PCI Modem card.
    // NEC MARTH bridge chip and Rockwell RCVDL56ACF/SP using.
    // "NEC PK-UG-X001 K56flex PCI Modem",
    PucDeviceDescription::new(
        [PCI_VENDOR_NEC, PCI_PRODUCT_NEC_MARTH, 0x1033, 0x8014],
        [0xffff, 0xffff, 0xffff, 0xffff],
        &[PucPort::new(PUC_PORT_COM, 0x10, 0x0000)],
    ),
    // NEC PK-UG-X008
    // "NEC PK-UG-X008",
    PucDeviceDescription::new(
        [PCI_VENDOR_NEC, PCI_PRODUCT_NEC_PKUG, 0x1033, 0x8012],
        [0xffff, 0xffff, 0xffff, 0xffff],
        &[PucPort::new(PUC_PORT_COM, 0x10, 0x0000)],
    ),
    // Lava Computers 2SP-PCI (0x8000-0x8003)
    // "Lava Computers 2SP-PCI parallel port",
    PucDeviceDescription::new(
        [PCI_VENDOR_LAVA, PCI_PRODUCT_LAVA_TWOSP_1P, 0, 0],
        [0xffff, 0xfffc, 0, 0],
        &[PucPort::new(PUC_PORT_LPT, 0x10, 0x0000)],
    ),
    // Lava Computers 2SP-PCI and Quattro-PCI serial ports
    // "Lava Computers dual serial port",
    PucDeviceDescription::new(
        [PCI_VENDOR_LAVA, PCI_PRODUCT_LAVA_TWOSP_2S, 0, 0],
        [0xffff, 0xfffc, 0, 0],
        &[
            PucPort::new(PUC_PORT_COM, 0x10, 0x0000),
            PucPort::new(PUC_PORT_COM, 0x14, 0x0000),
        ],
    ),
    // Lava Computers Quattro-PCI serial ports.
    // A second version of the Quattro-PCI with different PCI ids.
    // "Lava Computers Quattro-PCI 4-port serial",
    PucDeviceDescription::new(
        [PCI_VENDOR_LAVA, PCI_PRODUCT_LAVA_QUATTRO_AB2, 0, 0],
        [0xffff, 0xfffe, 0, 0],
        &[
            PucPort::new(PUC_PORT_COM, 0x10, 0x0000),
            PucPort::new(PUC_PORT_COM, 0x14, 0x0000),
        ],
    ),
    // Lava Computers LavaPort-Dual and LavaPort-Quad 4*clock PCI
    // serial ports.
    // "Lava Computers high-speed port",
    PucDeviceDescription::new(
        [PCI_VENDOR_LAVA, PCI_PRODUCT_LAVA_LAVAPORT_0, 0, 0],
        [0xffff, 0xfffc, 0, 0],
        &[
            PucPort::new(PUC_PORT_COM_MUL4, 0x10, 0x0000),
            PucPort::new(PUC_PORT_COM_MUL4, 0x14, 0x0000),
        ],
    ),
    // Lava Computers LavaPort-single serial port.
    // "Lava Computers high-speed port",
    PucDeviceDescription::new(
        [PCI_VENDOR_LAVA, PCI_PRODUCT_LAVA_LAVAPORT_2, 0, 0],
        [0xffff, 0xfffc, 0, 0],
        &[PucPort::new(PUC_PORT_COM_MUL4, 0x10, 0x0000)],
    ),
    // Lava Computers LavaPort-650
    // "Lava Computers high-speed port",
    PucDeviceDescription::new(
        [PCI_VENDOR_LAVA, PCI_PRODUCT_LAVA_650, 0, 0],
        [0xffff, 0xfffc, 0, 0],
        &[PucPort::new(PUC_PORT_COM_MUL4, 0x10, 0x0000)],
    ),
    // Koutech IOFLEX-2S PCI Dual Port Serial, port 1
    // "Koutech IOFLEX-2S PCI Dual Port Serial, port 1",
    PucDeviceDescription::new(
        [PCI_VENDOR_LAVA, PCI_PRODUCT_LAVA_IOFLEX_2S_0, 0, 0],
        [0xffff, 0xfffc, 0, 0],
        &[PucPort::new(PUC_PORT_COM, 0x10, 0x0000)],
    ),
    // Koutech IOFLEX-2S PCI Dual Port Serial, port 2
    // "Koutech IOFLEX-2S PCI Dual Port Serial, port 2",
    PucDeviceDescription::new(
        [PCI_VENDOR_LAVA, PCI_PRODUCT_LAVA_IOFLEX_2S_1, 0, 0],
        [0xffff, 0xfffc, 0, 0],
        &[PucPort::new(PUC_PORT_COM, 0x10, 0x0000)],
    ),
    // Lava Computers Octopus-550 serial ports
    // "Lava Computers Octopus-550 8-port serial",
    PucDeviceDescription::new(
        [PCI_VENDOR_LAVA, PCI_PRODUCT_LAVA_OCTOPUS550_0, 0, 0],
        [0xffff, 0xfffc, 0, 0],
        &[
            PucPort::new(PUC_PORT_COM, 0x10, 0x0000),
            PucPort::new(PUC_PORT_COM, 0x14, 0x0000),
            PucPort::new(PUC_PORT_COM, 0x18, 0x0000),
            PucPort::new(PUC_PORT_COM, 0x1c, 0x0000),
        ],
    ),
    // Lava Computers Octopus-550 serial ports
    // "Lava Computers Octopus-550 8-port serial",
    PucDeviceDescription::new(
        [PCI_VENDOR_LAVA, PCI_PRODUCT_LAVA_OCTOPUS550_1, 0, 0],
        [0xffff, 0xfffc, 0, 0],
        &[
            PucPort::new(PUC_PORT_COM, 0x10, 0x0000),
            PucPort::new(PUC_PORT_COM, 0x14, 0x0000),
            PucPort::new(PUC_PORT_COM, 0x18, 0x0000),
            PucPort::new(PUC_PORT_COM, 0x1c, 0x0000),
        ],
    ),
    // US Robotics (3Com) PCI Modems
    // "US Robotics (3Com) 3CP5610 PCI 16550 Modem",
    PucDeviceDescription::new(
        [PCI_VENDOR_USR, PCI_PRODUCT_USR_3CP5610, 0, 0],
        [0xffff, 0xffff, 0, 0],
        &[PucPort::new(PUC_PORT_COM, 0x10, 0x0000)],
    ),
    // IBM 33L4618: AT&T/Lucent Venus Modem
    // "IBM 33L4618: AT&T/Lucent Venus Modem",
    // "Actiontec 56K PCI Master"
    PucDeviceDescription::new(
        [PCI_VENDOR_LUCENT, PCI_PRODUCT_LUCENT_VENUSMODEM, 0, 0],
        [0xffff, 0xffff, 0, 0],
        &[PucPort::new(PUC_PORT_COM, 0x18, 0x0008)],
    ),
    // Topic/SmartLink 5634PCV SurfRider
    // "Topic/SmartLink 5634PCV SurfRider"
    PucDeviceDescription::new(
        [PCI_VENDOR_TOPIC, PCI_PRODUCT_TOPIC_5634PCV, 0, 0],
        [0xffff, 0xffff, 0, 0],
        &[PucPort::new(PUC_PORT_COM, 0x10, 0x0000)],
    ),
    // SD-LAB PCI I/O Card 4S
    // "Syba Tech Ltd. PCI-4S"
    PucDeviceDescription::new(
        [PCI_VENDOR_SYBA, PCI_PRODUCT_SYBA_4S, 0, 0],
        [0xffff, 0xffff, 0, 0],
        &[
            PucPort::new(PUC_PORT_COM, 0x10, 0x03e8),
            PucPort::new(PUC_PORT_COM, 0x10, 0x02e8),
            PucPort::new(PUC_PORT_COM, 0x10, 0x03f8),
            PucPort::new(PUC_PORT_COM, 0x10, 0x02f8),
        ],
    ),
    // SD-LAB PCI I/O Card 4S2P
    // "Syba Tech Ltd. PCI-4S2P-550-ECP"
    PucDeviceDescription::new(
        [PCI_VENDOR_SYBA, PCI_PRODUCT_SYBA_4S2P, 0, 0],
        [0xffff, 0xffff, 0, 0],
        &[
            PucPort::new(PUC_PORT_COM, 0x10, 0x02e8),
            PucPort::new(PUC_PORT_COM, 0x10, 0x02f8),
            PucPort::new(PUC_PORT_LPT, 0x10, 0x0000),
            PucPort::new(PUC_PORT_COM, 0x10, 0x03e8),
            PucPort::new(PUC_PORT_COM, 0x10, 0x03f8),
            PucPort::new(PUC_PORT_LPT, 0x10, 0x0000),
        ],
    ),
    // Moxa Technologies Co., Ltd. PCI I/O Card 4S RS232/422/485
    // "Moxa Technologies, Industio CP-114"
    PucDeviceDescription::new(
        [PCI_VENDOR_MOXA, PCI_PRODUCT_MOXA_CP114, 0, 0],
        [0xffff, 0xffff, 0, 0],
        &[
            PucPort::new(PUC_PORT_COM_MUL8, 0x18, 0x0000),
            PucPort::new(PUC_PORT_COM_MUL8, 0x18, 0x0008),
            PucPort::new(PUC_PORT_COM_MUL8, 0x18, 0x0010),
            PucPort::new(PUC_PORT_COM_MUL8, 0x18, 0x0018),
        ],
    ),
    // Moxa Technologies Co., Ltd. PCI I/O Card 4S RS232/422/485
    // "Moxa Technologies, SmartIO C104H/PCI"
    PucDeviceDescription::new(
        [PCI_VENDOR_MOXA, PCI_PRODUCT_MOXA_C104H, 0, 0],
        [0xffff, 0xffff, 0, 0],
        &[
            PucPort::new(PUC_PORT_COM_MUL8, 0x18, 0x0000),
            PucPort::new(PUC_PORT_COM_MUL8, 0x18, 0x0008),
            PucPort::new(PUC_PORT_COM_MUL8, 0x18, 0x0010),
            PucPort::new(PUC_PORT_COM_MUL8, 0x18, 0x0018),
        ],
    ),
    // Moxa Technologies Co., Ltd. PCI I/O Card 4S RS232
    // "Moxa Technologies, SmartIO CP104UL/PCI"
    PucDeviceDescription::new(
        [PCI_VENDOR_MOXA, PCI_PRODUCT_MOXA_CP104UL, 0, 0],
        [0xffff, 0xffff, 0, 0],
        &[
            PucPort::new(PUC_PORT_COM_MUL8, 0x18, 0x0000),
            PucPort::new(PUC_PORT_COM_MUL8, 0x18, 0x0008),
            PucPort::new(PUC_PORT_COM_MUL8, 0x18, 0x0010),
            PucPort::new(PUC_PORT_COM_MUL8, 0x18, 0x0018),
        ],
    ),
    // Moxa Technologies Co., Ltd. PCI I/O Card 4S RS232
    // "Moxa Technologies, SmartIO CP104JU/PCI"
    PucDeviceDescription::new(
        [PCI_VENDOR_MOXA, PCI_PRODUCT_MOXA_CP104JU, 0, 0],
        [0xffff, 0xffff, 0, 0],
        &[
            PucPort::new(PUC_PORT_COM_MUL8, 0x18, 0x0000),
            PucPort::new(PUC_PORT_COM_MUL8, 0x18, 0x0008),
            PucPort::new(PUC_PORT_COM_MUL8, 0x18, 0x0010),
            PucPort::new(PUC_PORT_COM_MUL8, 0x18, 0x0018),
        ],
    ),
    // Moxa Technologies Co., Ltd. PCI I/O Card 4S RS232
    // "Moxa Technologies, SmartIO CP104EL/PCI"
    PucDeviceDescription::new(
        [PCI_VENDOR_MOXA, PCI_PRODUCT_MOXA_CP104EL, 0, 0],
        [0xffff, 0xffff, 0, 0],
        &[
            PucPort::new(PUC_PORT_COM_MUL8, 0x18, 0x0000),
            PucPort::new(PUC_PORT_COM_MUL8, 0x18, 0x0008),
            PucPort::new(PUC_PORT_COM_MUL8, 0x18, 0x0010),
            PucPort::new(PUC_PORT_COM_MUL8, 0x18, 0x0018),
        ],
    ),
    // Moxa Technologies Co., Ltd. PCI I/O Card 8S RS232
    // "Moxa Technologies, Industio C168H"
    PucDeviceDescription::new(
        [PCI_VENDOR_MOXA, PCI_PRODUCT_MOXA_C168H, 0, 0],
        [0xffff, 0xffff, 0, 0],
        &[
            PucPort::new(PUC_PORT_COM_MUL8, 0x18, 0x0000),
            PucPort::new(PUC_PORT_COM_MUL8, 0x18, 0x0008),
            PucPort::new(PUC_PORT_COM_MUL8, 0x18, 0x0010),
            PucPort::new(PUC_PORT_COM_MUL8, 0x18, 0x0018),
            PucPort::new(PUC_PORT_COM_MUL8, 0x18, 0x0020),
            PucPort::new(PUC_PORT_COM_MUL8, 0x18, 0x0028),
            PucPort::new(PUC_PORT_COM_MUL8, 0x18, 0x0030),
            PucPort::new(PUC_PORT_COM_MUL8, 0x18, 0x0038),
        ],
    ),
    // Moxa Technologies Co., Ltd. PCI I/O Card 8S RS232
    // "Moxa Technologies, CP-168U"
    PucDeviceDescription::new(
        [PCI_VENDOR_MOXA, PCI_PRODUCT_MOXA_CP168U, 0, 0],
        [0xffff, 0xffff, 0, 0],
        &[
            PucPort::new(PUC_PORT_COM_MUL8, 0x18, 0x0000),
            PucPort::new(PUC_PORT_COM_MUL8, 0x18, 0x0008),
            PucPort::new(PUC_PORT_COM_MUL8, 0x18, 0x0010),
            PucPort::new(PUC_PORT_COM_MUL8, 0x18, 0x0018),
            PucPort::new(PUC_PORT_COM_MUL8, 0x18, 0x0020),
            PucPort::new(PUC_PORT_COM_MUL8, 0x18, 0x0028),
            PucPort::new(PUC_PORT_COM_MUL8, 0x18, 0x0030),
            PucPort::new(PUC_PORT_COM_MUL8, 0x18, 0x0038),
        ],
    ),
    // NetMos 1P PCI: 1P
    // "NetMos NM9805 1284 Printer Port"
    PucDeviceDescription::new(
        [PCI_VENDOR_NETMOS, PCI_PRODUCT_NETMOS_NM9805, 0, 0],
        [0xffff, 0xffff, 0, 0],
        &[PucPort::new(PUC_PORT_LPT, 0x10, 0x0000)],
    ),
    // NetMos 1S PCI 16C650 : 1S
    // "NetMos NM9835 UART"
    PucDeviceDescription::new(
        [PCI_VENDOR_NETMOS, PCI_PRODUCT_NETMOS_NM9835, 0x1000, 0x0001],
        [0xffff, 0xffff, 0xffff, 0xffff],
        &[PucPort::new(PUC_PORT_COM, 0x10, 0x0000)],
    ),
    // NetMos 2S1P PCI 16C650 : 2S, 1P
    // "NetMos NM9835 Dual UART and 1284 Printer port"
    PucDeviceDescription::new(
        [PCI_VENDOR_NETMOS, PCI_PRODUCT_NETMOS_NM9835, 0, 0],
        [0xffff, 0xffff, 0, 0],
        &[
            PucPort::new(PUC_PORT_COM, 0x10, 0x0000),
            PucPort::new(PUC_PORT_COM, 0x14, 0x0000),
            PucPort::new(PUC_PORT_LPT, 0x18, 0x0000),
        ],
    ),
    // NetMos 4S PCI 16C650 : 4S, 0P
    // "NetMos NM9845 Quad UART"
    PucDeviceDescription::new(
        [PCI_VENDOR_NETMOS, PCI_PRODUCT_NETMOS_NM9845, 0x1000, 0x0004],
        [0xffff, 0xffff, 0xffff, 0xffff],
        &[
            PucPort::new(PUC_PORT_COM, 0x10, 0x0000),
            PucPort::new(PUC_PORT_COM, 0x14, 0x0000),
            PucPort::new(PUC_PORT_COM, 0x18, 0x0000),
            PucPort::new(PUC_PORT_COM, 0x1c, 0x0000),
        ],
    ),
    // NetMos 4S1P PCI 16C650 : 4S, 1P
    // "NetMos NM9845 Quad UART and 1284 Printer port"
    PucDeviceDescription::new(
        [PCI_VENDOR_NETMOS, PCI_PRODUCT_NETMOS_NM9845, 0x1000, 0x0014],
        [0xffff, 0xffff, 0xffff, 0xffff],
        &[
            PucPort::new(PUC_PORT_COM, 0x10, 0x0000),
            PucPort::new(PUC_PORT_COM, 0x14, 0x0000),
            PucPort::new(PUC_PORT_COM, 0x18, 0x0000),
            PucPort::new(PUC_PORT_COM, 0x1c, 0x0000),
            PucPort::new(PUC_PORT_LPT, 0x20, 0x0000),
        ],
    ),
    // NetMos 6S PCI 16C650 : 6S, 0P
    // "NetMos NM9845 6 UART"
    PucDeviceDescription::new(
        [PCI_VENDOR_NETMOS, PCI_PRODUCT_NETMOS_NM9845, 0x1000, 0x0006],
        [0xffff, 0xffff, 0xffff, 0xffff],
        &[
            PucPort::new(PUC_PORT_COM, 0x10, 0x0000),
            PucPort::new(PUC_PORT_COM, 0x14, 0x0000),
            PucPort::new(PUC_PORT_COM, 0x18, 0x0000),
            PucPort::new(PUC_PORT_COM, 0x1c, 0x0000),
            PucPort::new(PUC_PORT_COM, 0x20, 0x0000),
            PucPort::new(PUC_PORT_COM, 0x24, 0x0000),
        ],
    ),
    // NetMos 2S PCI 16C650 : 2S
    // "NetMos NM9845 Dual UART"
    PucDeviceDescription::new(
        [PCI_VENDOR_NETMOS, PCI_PRODUCT_NETMOS_NM9845, 0, 0],
        [0xffff, 0xffff, 0, 0],
        &[
            PucPort::new(PUC_PORT_COM, 0x10, 0x0000),
            PucPort::new(PUC_PORT_COM, 0x14, 0x0000),
        ],
    ),
    // NetMos 6S PCI 16C650 : 6S
    // Shows up as three PCI devices, two with a single serial
    // port and one with four serial ports (on a special ISA
    // extender chip).
    // "NetMos NM9865 6 UART: 1 UART"
    PucDeviceDescription::new(
        [PCI_VENDOR_NETMOS, PCI_PRODUCT_NETMOS_NM9865, 0xa000, 0x1000],
        [0xffff, 0xffff, 0xffff, 0xffff],
        &[PucPort::new(PUC_PORT_COM, 0x10, 0x0000)],
    ),
    // "NetMos NM9865 6 UART: 4 UART ISA"
    PucDeviceDescription::new(
        [PCI_VENDOR_NETMOS, PCI_PRODUCT_NETMOS_NM9865, 0xa000, 0x3004],
        [0xffff, 0xffff, 0xffff, 0xffff],
        &[
            PucPort::new(PUC_PORT_COM, 0x10, 0x0000),
            PucPort::new(PUC_PORT_COM, 0x14, 0x0000),
            PucPort::new(PUC_PORT_COM, 0x18, 0x0000),
            PucPort::new(PUC_PORT_COM, 0x1c, 0x0000),
        ],
    ),
    // NetMos PCIe Peripheral Controller :UART part
    // "NetMos NM9900 UART"
    PucDeviceDescription::new(
        [PCI_VENDOR_NETMOS, PCI_PRODUCT_NETMOS_NM9900, 0xa000, 0x1000],
        [0xffff, 0xffff, 0xffff, 0xffff],
        &[PucPort::new(PUC_PORT_COM, 0x10, 0x0000)],
    ),
    // NetMos PCIe Peripheral Controller :UART part
    // "NetMos NM9901 UART"
    PucDeviceDescription::new(
        [PCI_VENDOR_NETMOS, PCI_PRODUCT_NETMOS_NM9901, 0xa000, 0x1000],
        [0xffff, 0xffff, 0xffff, 0xffff],
        &[PucPort::new(PUC_PORT_COM, 0x10, 0x0000)],
    ),
    // NetMos PCIe Peripheral Controller :parallel part
    // "NetMos NM9901 UART"
    PucDeviceDescription::new(
        [PCI_VENDOR_NETMOS, PCI_PRODUCT_NETMOS_NM9901, 0xa000, 0x2000],
        [0xffff, 0xffff, 0xffff, 0xffff],
        &[PucPort::new(PUC_PORT_LPT, 0x10, 0x0000)],
    ),
    // NetMos NM9922: 2S
    PucDeviceDescription::new(
        [PCI_VENDOR_NETMOS, PCI_PRODUCT_NETMOS_NM9922, 0xa000, 0x1000],
        [0xffff, 0xffff, 0xffff, 0xffff],
        &[PucPort::new(PUC_PORT_COM, 0x10, 0x0000)],
    ),
    // Perle Speed8 LE.
    // Based on an Oxford Semiconductor OX16PCI954 chip, using the
    // 8-bit pass-through Local Bus function to hook up 4
    // additional UARTs.
    // OX16PCI954 internal UARTs
    PucDeviceDescription::new(
        [
            PCI_VENDOR_PERLE,
            PCI_PRODUCT_PERLE_SPEED8_LE,
            PCI_VENDOR_OXFORD2,
            PCI_PRODUCT_OXFORD2_OX16PCI954,
        ],
        [0xffff, 0xffff, 0xffff, 0xffff],
        &[
            PucPort::new(PUC_PORT_COM_MUL8, 0x10, 0x0000),
            PucPort::new(PUC_PORT_COM_MUL8, 0x10, 0x0008),
            PucPort::new(PUC_PORT_COM_MUL8, 0x10, 0x0010),
            PucPort::new(PUC_PORT_COM_MUL8, 0x10, 0x0018),
        ],
    ),
    // OX16PCI954 8-bit pass-through Local Bus
    PucDeviceDescription::new(
        [
            PCI_VENDOR_PERLE,
            PCI_PRODUCT_PERLE_SPEED8_LE,
            PCI_VENDOR_OXFORD2,
            0x9511,
        ],
        [0xffff, 0xffff, 0xffff, 0xffff],
        &[
            PucPort::new(PUC_PORT_COM_MUL8, 0x10, 0x0000),
            PucPort::new(PUC_PORT_COM_MUL8, 0x10, 0x0008),
            PucPort::new(PUC_PORT_COM_MUL8, 0x10, 0x0010),
            PucPort::new(PUC_PORT_COM_MUL8, 0x10, 0x0018),
        ],
    ),
    // Sunix 4018A : 2-port parallel
    PucDeviceDescription::new(
        [PCI_VENDOR_SUNIX, PCI_PRODUCT_SUNIX_4018A, 0, 0],
        [0xffff, 0xffff, 0, 0],
        &[
            PucPort::new(PUC_PORT_LPT, 0x10, 0x0000),
            PucPort::new(PUC_PORT_LPT, 0x18, 0x0000),
        ],
    ),
    // SUNIX 40XX series of serial/parallel combo cards.
    // Tested with 4055A and 4065A.
    // SUNIX 400X 1P
    PucDeviceDescription::new(
        [PCI_VENDOR_SUNIX, PCI_PRODUCT_SUNIX_40XX, 0x1409, 0x4000],
        [0xffff, 0xffff, 0xffff, 0xeff0],
        &[PucPort::new(PUC_PORT_LPT, 0x10, 0x0000)],
    ),
    // SUNIX 401X 2P
    PucDeviceDescription::new(
        [PCI_VENDOR_SUNIX, PCI_PRODUCT_SUNIX_40XX, 0x1409, 0x4010],
        [0xffff, 0xffff, 0xffff, 0xeff0],
        &[
            PucPort::new(PUC_PORT_LPT, 0x10, 0x0000),
            PucPort::new(PUC_PORT_LPT, 0x18, 0x0000),
        ],
    ),
    // SUNIX 402X 1S
    PucDeviceDescription::new(
        [PCI_VENDOR_SUNIX, PCI_PRODUCT_SUNIX_40XX, 0x1409, 0x4020],
        [0xffff, 0xffff, 0xffff, 0xeff0],
        &[PucPort::new(PUC_PORT_COM_MUL8, 0x10, 0x0000)],
    ),
    // SUNIX 403X 2S
    PucDeviceDescription::new(
        [PCI_VENDOR_SUNIX, PCI_PRODUCT_SUNIX_40XX, 0x1409, 0x4030],
        [0xffff, 0xffff, 0xffff, 0xeff0],
        &[
            PucPort::new(PUC_PORT_COM_MUL8, 0x10, 0x0000),
            PucPort::new(PUC_PORT_COM_MUL8, 0x10, 0x0008),
        ],
    ),
    // SUNIX 4036 2S
    PucDeviceDescription::new(
        [PCI_VENDOR_SUNIX, PCI_PRODUCT_SUNIX_40XX, 0x1409, 0x0002],
        [0xffff, 0xffff, 0xffff, 0xffff],
        &[
            PucPort::new(PUC_PORT_COM_MUL8, 0x10, 0x0000),
            PucPort::new(PUC_PORT_COM_MUL8, 0x10, 0x0008),
        ],
    ),
    // SUNIX 405X 4S
    PucDeviceDescription::new(
        [PCI_VENDOR_SUNIX, PCI_PRODUCT_SUNIX_40XX, 0x1409, 0x4050],
        [0xffff, 0xffff, 0xffff, 0xe0f0],
        &[
            PucPort::new(PUC_PORT_COM_MUL8, 0x10, 0x0000),
            PucPort::new(PUC_PORT_COM_MUL8, 0x10, 0x0008),
            PucPort::new(PUC_PORT_COM, 0x14, 0x0000),
            PucPort::new(PUC_PORT_COM, 0x14, 0x0008),
        ],
    ),
    // SUNIX 406X 8S
    PucDeviceDescription::new(
        [PCI_VENDOR_SUNIX, PCI_PRODUCT_SUNIX_40XX, 0x1409, 0x5066],
        [0xffff, 0xffff, 0xffff, 0xffff],
        &[
            PucPort::new(PUC_PORT_COM_MUL8, 0x10, 0x0000),
            PucPort::new(PUC_PORT_COM_MUL8, 0x10, 0x0008),
            PucPort::new(PUC_PORT_COM, 0x14, 0x0000),
            PucPort::new(PUC_PORT_COM, 0x14, 0x0008),
            PucPort::new(PUC_PORT_COM, 0x18, 0x0000),
            PucPort::new(PUC_PORT_COM, 0x1c, 0x0000),
            PucPort::new(PUC_PORT_COM, 0x20, 0x0000),
            PucPort::new(PUC_PORT_COM, 0x24, 0x0000),
        ],
    ),
    // SUNIX 406X 8S
    PucDeviceDescription::new(
        [PCI_VENDOR_SUNIX, PCI_PRODUCT_SUNIX_40XX, 0x1409, 0x4060],
        [0xffff, 0xffff, 0xffff, 0xe0f0],
        &[
            PucPort::new(PUC_PORT_COM_MUL8, 0x10, 0x0000),
            PucPort::new(PUC_PORT_COM_MUL8, 0x10, 0x0008),
            PucPort::new(PUC_PORT_COM_MUL8, 0x14, 0x0000),
            PucPort::new(PUC_PORT_COM_MUL8, 0x14, 0x0008),
            PucPort::new(PUC_PORT_COM_MUL8, 0x18, 0x0000),
            PucPort::new(PUC_PORT_COM_MUL8, 0x1c, 0x0000),
            PucPort::new(PUC_PORT_COM_MUL8, 0x20, 0x0000),
            PucPort::new(PUC_PORT_COM_MUL8, 0x24, 0x0000),
        ],
    ),
    // SUNIX 407X 2S/1P
    PucDeviceDescription::new(
        [PCI_VENDOR_SUNIX, PCI_PRODUCT_SUNIX_40XX, 0x1409, 0x4070],
        [0xffff, 0xffff, 0xffff, 0xeff0],
        &[
            PucPort::new(PUC_PORT_COM_MUL8, 0x10, 0x0000),
            PucPort::new(PUC_PORT_COM_MUL8, 0x10, 0x0008),
            PucPort::new(PUC_PORT_LPT, 0x18, 0x0000),
        ],
    ),
    // SUNIX 408X 2S/2P
    PucDeviceDescription::new(
        [PCI_VENDOR_SUNIX, PCI_PRODUCT_SUNIX_40XX, 0x1409, 0x4080],
        [0xffff, 0xffff, 0xffff, 0xeff0],
        &[
            PucPort::new(PUC_PORT_COM_MUL8, 0x10, 0x0000),
            PucPort::new(PUC_PORT_COM_MUL8, 0x10, 0x0008),
            PucPort::new(PUC_PORT_LPT, 0x18, 0x0000),
            PucPort::new(PUC_PORT_LPT, 0x20, 0x0000),
        ],
    ),
    // SUNIX 409X 4S/2P
    PucDeviceDescription::new(
        [PCI_VENDOR_SUNIX, PCI_PRODUCT_SUNIX_40XX, 0x1409, 0x4090],
        [0xffff, 0xffff, 0xffff, 0xeff0],
        &[
            PucPort::new(PUC_PORT_COM_MUL8, 0x10, 0x0000),
            PucPort::new(PUC_PORT_COM_MUL8, 0x10, 0x0008),
            PucPort::new(PUC_PORT_COM, 0x14, 0x0000),
            PucPort::new(PUC_PORT_COM, 0x14, 0x0008),
            PucPort::new(PUC_PORT_LPT, 0x18, 0x0000),
            PucPort::new(PUC_PORT_LPT, 0x20, 0x0000),
        ],
    ),
    // SUNIX 50XX series of serial/parallel combo cards.
    // Tested with 5066A.
    // SUNIX 5008 1P
    PucDeviceDescription::new(
        [PCI_VENDOR_SUNIX2, PCI_PRODUCT_SUNIX2_50XX, 0x1fd4, 0x0100],
        [0xffff, 0xffff, 0xffff, 0xeff0],
        &[PucPort::new(PUC_PORT_LPT, 0x14, 0x0000)],
    ),
    // SUNIX 5016 16S
    PucDeviceDescription::new(
        [PCI_VENDOR_SUNIX2, PCI_PRODUCT_SUNIX2_50XX, 0x1fd4, 0x0010],
        [0xffff, 0xffff, 0xffff, 0xffff],
        &[
            PucPort::new(PUC_PORT_COM_MUL8, 0x10, 0x0000),
            PucPort::new(PUC_PORT_COM_MUL8, 0x10, 0x0008),
            PucPort::new(PUC_PORT_COM_MUL8, 0x10, 0x0010),
            PucPort::new(PUC_PORT_COM_MUL8, 0x10, 0x0018),
            PucPort::new(PUC_PORT_COM_MUL8, 0x14, 0x0000),
            PucPort::new(PUC_PORT_COM_MUL8, 0x14, 0x0008),
            PucPort::new(PUC_PORT_COM_MUL8, 0x14, 0x0010),
            PucPort::new(PUC_PORT_COM_MUL8, 0x14, 0x0018),
            PucPort::new(PUC_PORT_COM_MUL8, 0x14, 0x0020),
            PucPort::new(PUC_PORT_COM_MUL8, 0x14, 0x0028),
            PucPort::new(PUC_PORT_COM_MUL8, 0x14, 0x0030),
            PucPort::new(PUC_PORT_COM_MUL8, 0x14, 0x0038),
            PucPort::new(PUC_PORT_COM_MUL8, 0x14, 0x0040),
            PucPort::new(PUC_PORT_COM_MUL8, 0x14, 0x0048),
            PucPort::new(PUC_PORT_COM_MUL8, 0x14, 0x0050),
            PucPort::new(PUC_PORT_COM_MUL8, 0x14, 0x0058),
        ],
    ),
    // SUNIX 5027 1S
    PucDeviceDescription::new(
        [PCI_VENDOR_SUNIX2, PCI_PRODUCT_SUNIX2_50XX, 0x1fd4, 0x0001],
        [0xffff, 0xffff, 0xffff, 0xffff],
        &[PucPort::new(PUC_PORT_COM_MUL8, 0x10, 0x0000)],
    ),
    // SUNIX 5037 2S
    PucDeviceDescription::new(
        [PCI_VENDOR_SUNIX2, PCI_PRODUCT_SUNIX2_50XX, 0x1fd4, 0x0002],
        [0xffff, 0xffff, 0xffff, 0xffff],
        &[
            PucPort::new(PUC_PORT_COM_MUL8, 0x10, 0x0000),
            PucPort::new(PUC_PORT_COM_MUL8, 0x10, 0x0008),
        ],
    ),
    // SUNIX 5056 4S
    PucDeviceDescription::new(
        [PCI_VENDOR_SUNIX2, PCI_PRODUCT_SUNIX2_50XX, 0x1fd4, 0x0004],
        [0xffff, 0xffff, 0xffff, 0xffff],
        &[
            PucPort::new(PUC_PORT_COM_MUL8, 0x10, 0x0000),
            PucPort::new(PUC_PORT_COM_MUL8, 0x10, 0x0008),
            PucPort::new(PUC_PORT_COM_MUL8, 0x10, 0x0010),
            PucPort::new(PUC_PORT_COM_MUL8, 0x10, 0x0018),
        ],
    ),
    // SUNIX 5066 8S
    PucDeviceDescription::new(
        [PCI_VENDOR_SUNIX2, PCI_PRODUCT_SUNIX2_50XX, 0x1fd4, 0x0008],
        [0xffff, 0xffff, 0xffff, 0xffff],
        &[
            PucPort::new(PUC_PORT_COM_MUL8, 0x10, 0x0000),
            PucPort::new(PUC_PORT_COM_MUL8, 0x10, 0x0008),
            PucPort::new(PUC_PORT_COM_MUL8, 0x10, 0x0010),
            PucPort::new(PUC_PORT_COM_MUL8, 0x10, 0x0018),
            PucPort::new(PUC_PORT_COM_MUL8, 0x14, 0x0000),
            PucPort::new(PUC_PORT_COM_MUL8, 0x14, 0x0008),
            PucPort::new(PUC_PORT_COM_MUL8, 0x14, 0x0010),
            PucPort::new(PUC_PORT_COM_MUL8, 0x14, 0x0018),
        ],
    ),
    // SUNIX 5069 1S / 1P
    PucDeviceDescription::new(
        [PCI_VENDOR_SUNIX2, PCI_PRODUCT_SUNIX2_50XX, 0x1fd4, 0x0101],
        [0xffff, 0xffff, 0xffff, 0xeff0],
        &[
            PucPort::new(PUC_PORT_COM_MUL8, 0x10, 0x0000),
            PucPort::new(PUC_PORT_LPT, 0x14, 0x0000),
        ],
    ),
    // SUNIX 5079 2S / 1P
    PucDeviceDescription::new(
        [PCI_VENDOR_SUNIX2, PCI_PRODUCT_SUNIX2_50XX, 0x1fd4, 0x0102],
        [0xffff, 0xffff, 0xffff, 0xffff],
        &[
            PucPort::new(PUC_PORT_COM_MUL8, 0x10, 0x0000),
            PucPort::new(PUC_PORT_COM_MUL8, 0x10, 0x0008),
            PucPort::new(PUC_PORT_LPT, 0x14, 0x0000),
        ],
    ),
    // SUNIX 5099 4S / 1P
    PucDeviceDescription::new(
        [PCI_VENDOR_SUNIX2, PCI_PRODUCT_SUNIX2_50XX, 0x1fd4, 0x0104],
        [0xffff, 0xffff, 0xffff, 0xffff],
        &[
            PucPort::new(PUC_PORT_COM_MUL8, 0x10, 0x0000),
            PucPort::new(PUC_PORT_COM_MUL8, 0x10, 0x0008),
            PucPort::new(PUC_PORT_COM_MUL8, 0x10, 0x0010),
            PucPort::new(PUC_PORT_COM_MUL8, 0x10, 0x0018),
            PucPort::new(PUC_PORT_LPT, 0x14, 0x0000),
        ],
    ),
    // Boca Research Turbo Serial 654 (4 serial port) card.
    // Appears to be the same as Chase Research PLC PCI-FAST4 card,
    // same as Perle PCI-FAST4 Multi-Port serial card
    // "Boca Turbo Serial 654 - IOP654"
    PucDeviceDescription::new(
        [PCI_VENDOR_PLX, PCI_PRODUCT_PLX_9050, 0x12e0, 0x0031],
        [0xffff, 0xffff, 0xffff, 0xffff],
        &[
            PucPort::new(PUC_PORT_COM_MUL4, 0x18, 0x0000),
            PucPort::new(PUC_PORT_COM_MUL4, 0x18, 0x0008),
            PucPort::new(PUC_PORT_COM_MUL4, 0x18, 0x0010),
            PucPort::new(PUC_PORT_COM_MUL4, 0x18, 0x0018),
        ],
    ),
    // Boca Research Turbo Serial 658 (8 serial port) card.
    // Appears to be the same as Chase Research PLC PCI-FAST8 card
    // same as Perle PCI-FAST8 Multi-Port serial card
    // "Boca Turbo Serial 658 - IOP658"
    PucDeviceDescription::new(
        [PCI_VENDOR_PLX, PCI_PRODUCT_PLX_9050, 0x12e0, 0x0021],
        [0xffff, 0xffff, 0xffff, 0xffff],
        &[
            PucPort::new(PUC_PORT_COM_MUL4, 0x18, 0x0000),
            PucPort::new(PUC_PORT_COM_MUL4, 0x18, 0x0008),
            PucPort::new(PUC_PORT_COM_MUL4, 0x18, 0x0010),
            PucPort::new(PUC_PORT_COM_MUL4, 0x18, 0x0018),
            PucPort::new(PUC_PORT_COM_MUL4, 0x18, 0x0020),
            PucPort::new(PUC_PORT_COM_MUL4, 0x18, 0x0028),
            PucPort::new(PUC_PORT_COM_MUL4, 0x18, 0x0030),
            PucPort::new(PUC_PORT_COM_MUL4, 0x18, 0x0038),
        ],
    ),
    // Cronyx Engineering Ltd. Omega-PCI (8 serial port) card.
    // "Cronyx Omega-PCI"
    PucDeviceDescription::new(
        [PCI_VENDOR_PLX, PCI_PRODUCT_PLX_CRONYX_OMEGA, 0, 0],
        [0xffff, 0xffff, 0, 0],
        &[
            PucPort::new(PUC_PORT_COM, 0x18, 0x0000),
            PucPort::new(PUC_PORT_COM, 0x18, 0x0008),
            PucPort::new(PUC_PORT_COM, 0x18, 0x0010),
            PucPort::new(PUC_PORT_COM, 0x18, 0x0018),
            PucPort::new(PUC_PORT_COM, 0x18, 0x0020),
            PucPort::new(PUC_PORT_COM, 0x18, 0x0028),
            PucPort::new(PUC_PORT_COM, 0x18, 0x0030),
            PucPort::new(PUC_PORT_COM, 0x18, 0x0038),
        ],
    ),
    // PLX 9016 8 port serial card. (i.e. Syba)
    // "PLX 9016 - Syba"
    PucDeviceDescription::new(
        [PCI_VENDOR_PLX, PCI_PRODUCT_PLX_9016, 0, 0],
        [0xffff, 0xffff, 0, 0],
        &[
            PucPort::new(PUC_PORT_COM_MUL4, 0x10, 0x0000),
            PucPort::new(PUC_PORT_COM_MUL4, 0x10, 0x0008),
            PucPort::new(PUC_PORT_COM_MUL4, 0x10, 0x0010),
            PucPort::new(PUC_PORT_COM_MUL4, 0x10, 0x0018),
            PucPort::new(PUC_PORT_COM_MUL4, 0x10, 0x0020),
            PucPort::new(PUC_PORT_COM_MUL4, 0x10, 0x0028),
            PucPort::new(PUC_PORT_COM_MUL4, 0x10, 0x0030),
            PucPort::new(PUC_PORT_COM_MUL4, 0x10, 0x0038),
        ],
    ),
    // Avlab Technology, Inc. Low Profile PCI 4 Serial: 4S
    // "Avlab Low Profile PCI 4 Serial"
    PucDeviceDescription::new(
        [PCI_VENDOR_AVLAB, PCI_PRODUCT_AVLAB_LPPCI4S_2, 0, 0],
        [0xffff, 0xffff, 0, 0],
        &[
            PucPort::new(PUC_PORT_COM, 0x10, 0x0000),
            PucPort::new(PUC_PORT_COM, 0x14, 0x0000),
            PucPort::new(PUC_PORT_COM, 0x18, 0x0000),
            PucPort::new(PUC_PORT_COM, 0x1c, 0x0000),
        ],
    ),
    // Avlab Technology, Inc. Low Profile PCI 4 Serial: 4S
    // "Avlab Low Profile PCI 4 Serial"
    PucDeviceDescription::new(
        [PCI_VENDOR_AVLAB, PCI_PRODUCT_AVLAB_LPPCI4S, 0, 0],
        [0xffff, 0xffff, 0, 0],
        &[
            PucPort::new(PUC_PORT_COM, 0x10, 0x0000),
            PucPort::new(PUC_PORT_COM, 0x14, 0x0000),
            PucPort::new(PUC_PORT_COM, 0x18, 0x0000),
            PucPort::new(PUC_PORT_COM, 0x1c, 0x0000),
        ],
    ),
    // Avlab Technology, Inc. PCI 2 Serial: 2S
    // "Avlab PCI 2 Serial"
    PucDeviceDescription::new(
        [PCI_VENDOR_AVLAB, PCI_PRODUCT_AVLAB_PCI2S, 0, 0],
        [0xffff, 0xffff, 0, 0],
        &[
            PucPort::new(PUC_PORT_COM, 0x10, 0x0000),
            PucPort::new(PUC_PORT_COM, 0x14, 0x0000),
        ],
    ),
    // Digi International Digi Neo 4 Serial
    PucDeviceDescription::new(
        [PCI_VENDOR_DIGI, PCI_PRODUCT_DIGI_NEO4, 0, 0],
        [0xffff, 0xffff, 0, 0],
        &[
            PucPort::new(PUC_PORT_COM_MUL8, 0x10, 0x0000),
            PucPort::new(PUC_PORT_COM_MUL8, 0x10, 0x0200),
            PucPort::new(PUC_PORT_COM_MUL8, 0x10, 0x0400),
            PucPort::new(PUC_PORT_COM_MUL8, 0x10, 0x0600),
        ],
    ),
    // Digi International Digi Neo 8 Serial
    PucDeviceDescription::new(
        [PCI_VENDOR_DIGI, PCI_PRODUCT_DIGI_NEO8, 0, 0],
        [0xffff, 0xffff, 0, 0],
        &[
            PucPort::new(PUC_PORT_COM_MUL8, 0x10, 0x0000),
            PucPort::new(PUC_PORT_COM_MUL8, 0x10, 0x0200),
            PucPort::new(PUC_PORT_COM_MUL8, 0x10, 0x0400),
            PucPort::new(PUC_PORT_COM_MUL8, 0x10, 0x0600),
            PucPort::new(PUC_PORT_COM_MUL8, 0x10, 0x0800),
            PucPort::new(PUC_PORT_COM_MUL8, 0x10, 0x0a00),
            PucPort::new(PUC_PORT_COM_MUL8, 0x10, 0x0c00),
            PucPort::new(PUC_PORT_COM_MUL8, 0x10, 0x0e00),
        ],
    ),
    // Digi International Digi Neo 8 PCIe Serial
    PucDeviceDescription::new(
        [PCI_VENDOR_DIGI, PCI_PRODUCT_DIGI_NEO8_PCIE, 0, 0],
        [0xffff, 0xffff, 0, 0],
        &[
            PucPort::new(PUC_PORT_COM_MUL8, 0x10, 0x0000),
            PucPort::new(PUC_PORT_COM_MUL8, 0x10, 0x0200),
            PucPort::new(PUC_PORT_COM_MUL8, 0x10, 0x0400),
            PucPort::new(PUC_PORT_COM_MUL8, 0x10, 0x0600),
            PucPort::new(PUC_PORT_COM_MUL8, 0x10, 0x0800),
            PucPort::new(PUC_PORT_COM_MUL8, 0x10, 0x0a00),
            PucPort::new(PUC_PORT_COM_MUL8, 0x10, 0x0c00),
            PucPort::new(PUC_PORT_COM_MUL8, 0x10, 0x0e00),
        ],
    ),
    // Multi-Tech ISI5634PCI/4 4-port modem board.
    // Has a 4-channel Exar XR17C154 UART, but with bogus product ID in its
    // config EEPROM.
    PucDeviceDescription::new(
        [PCI_VENDOR_EXAR, PCI_PRODUCT_EXAR_XR17C158, 0x2205, 0x2003],
        [0xffff, 0xffff, 0xffff, 0xffff],
        &[
            PucPort::new(PUC_PORT_COM_MUL8, 0x10, 0x0000),
            PucPort::new(PUC_PORT_COM_MUL8, 0x10, 0x0200),
            PucPort::new(PUC_PORT_COM_MUL8, 0x10, 0x0400),
            PucPort::new(PUC_PORT_COM_MUL8, 0x10, 0x0600),
        ],
    ),
    // EXAR XR17C152 Dual UART
    PucDeviceDescription::new(
        [PCI_VENDOR_EXAR, PCI_PRODUCT_EXAR_XR17C152, 0, 0],
        [0xffff, 0xffff, 0, 0],
        &[
            PucPort::new(PUC_PORT_COM_MUL8, 0x10, 0x0000),
            PucPort::new(PUC_PORT_COM_MUL8, 0x10, 0x0200),
        ],
    ),
    // Exar XR17C154 Quad UART
    PucDeviceDescription::new(
        [PCI_VENDOR_EXAR, PCI_PRODUCT_EXAR_XR17C154, 0, 0],
        [0xffff, 0xffff, 0, 0],
        &[
            PucPort::new(PUC_PORT_COM_MUL8, 0x10, 0x0000),
            PucPort::new(PUC_PORT_COM_MUL8, 0x10, 0x0200),
            PucPort::new(PUC_PORT_COM_MUL8, 0x10, 0x0400),
            PucPort::new(PUC_PORT_COM_MUL8, 0x10, 0x0600),
        ],
    ),
    // Exar XR17C158 Eight Channel UART
    PucDeviceDescription::new(
        [PCI_VENDOR_EXAR, PCI_PRODUCT_EXAR_XR17C158, 0, 0],
        [0xffff, 0xffff, 0, 0],
        &[
            PucPort::new(PUC_PORT_COM_MUL8, 0x10, 0x0000),
            PucPort::new(PUC_PORT_COM_MUL8, 0x10, 0x0200),
            PucPort::new(PUC_PORT_COM_MUL8, 0x10, 0x0400),
            PucPort::new(PUC_PORT_COM_MUL8, 0x10, 0x0600),
            PucPort::new(PUC_PORT_COM_MUL8, 0x10, 0x0800),
            PucPort::new(PUC_PORT_COM_MUL8, 0x10, 0x0a00),
            PucPort::new(PUC_PORT_COM_MUL8, 0x10, 0x0c00),
            PucPort::new(PUC_PORT_COM_MUL8, 0x10, 0x0e00),
        ],
    ),
    // Exar XR17V352 Dual UART
    PucDeviceDescription::new(
        [PCI_VENDOR_EXAR, PCI_PRODUCT_EXAR_XR17V352, 0, 0],
        [0xffff, 0xffff, 0, 0],
        &[
            PucPort::new(PUC_PORT_COM_XR17V35X, 0x10, 0x0000),
            PucPort::new(PUC_PORT_COM_XR17V35X, 0x10, 0x0400),
        ],
    ),
    // Exar XR17V354 Quad UART
    PucDeviceDescription::new(
        [PCI_VENDOR_EXAR, PCI_PRODUCT_EXAR_XR17V354, 0, 0],
        [0xffff, 0xffff, 0, 0],
        &[
            PucPort::new(PUC_PORT_COM_XR17V35X, 0x10, 0x0000),
            PucPort::new(PUC_PORT_COM_XR17V35X, 0x10, 0x0400),
            PucPort::new(PUC_PORT_COM_XR17V35X, 0x10, 0x0800),
            PucPort::new(PUC_PORT_COM_XR17V35X, 0x10, 0x0C00),
        ],
    ),
    // Dell DRAC 3 Virtual UART
    PucDeviceDescription::new(
        [PCI_VENDOR_DELL, PCI_PRODUCT_DELL_DRAC_3_VUART, 0, 0],
        [0xffff, 0xffff, 0, 0],
        &[PucPort::new(PUC_PORT_COM_MUL128, 0x14, 0x0000)],
    ),
    // Dell DRAC 4 Virtual UART
    PucDeviceDescription::new(
        [PCI_VENDOR_DELL, PCI_PRODUCT_DELL_DRAC_4_VUART, 0, 0],
        [0xffff, 0xffff, 0, 0],
        &[PucPort::new(PUC_PORT_COM_MUL128, 0x14, 0x0000)],
    ),
    // Cardbus devices which can potentially show up because of
    // Expresscard adapters
    // XXX Keep this list synchronized with cardbus/com_cardbus.c
    // "",
    PucDeviceDescription::new(
        [PCI_VENDOR_3COM, PCI_PRODUCT_3COM_GLOBALMODEM56, 0, 0],
        [0xffff, 0xffff, 0, 0],
        &[PucPort::new(PUC_PORT_COM, 0x10, 0x0000)],
    ),
    // "",
    PucDeviceDescription::new(
        [PCI_VENDOR_3COM, PCI_PRODUCT_3COM_MODEM56, 0, 0],
        [0xffff, 0xffff, 0, 0],
        &[PucPort::new(PUC_PORT_COM, 0x10, 0x0000)],
    ),
    // "",
    PucDeviceDescription::new(
        [PCI_VENDOR_BROADCOM, PCI_PRODUCT_BROADCOM_SERIAL, 0, 0],
        [0xffff, 0xffff, 0, 0],
        &[PucPort::new(PUC_PORT_COM, 0x10, 0x0000)],
    ),
    // "",
    PucDeviceDescription::new(
        [PCI_VENDOR_BROADCOM, PCI_PRODUCT_BROADCOM_SERIAL_2, 0, 0],
        [0xffff, 0xffff, 0, 0],
        &[PucPort::new(PUC_PORT_COM, 0x10, 0x0000)],
    ),
    // "",
    PucDeviceDescription::new(
        [PCI_VENDOR_BROADCOM, PCI_PRODUCT_BROADCOM_SERIAL_GC, 0, 0],
        [0xffff, 0xffff, 0, 0],
        &[PucPort::new(PUC_PORT_COM, 0x10, 0x0000)],
    ),
    // "",
    PucDeviceDescription::new(
        [PCI_VENDOR_INTEL, PCI_PRODUCT_INTEL_MODEM56, 0, 0],
        [0xffff, 0xffff, 0, 0],
        &[PucPort::new(PUC_PORT_COM, 0x10, 0x0000)],
    ),
    // "",
    PucDeviceDescription::new(
        [PCI_VENDOR_OXFORD2, PCI_PRODUCT_OXFORD2_OXCB950, 0, 0],
        [0xffff, 0xffff, 0, 0],
        &[PucPort::new(PUC_PORT_COM, 0x10, 0x0000)],
    ),
    // "Xircom Cardbus 56K Modem",
    PucDeviceDescription::new(
        [PCI_VENDOR_XIRCOM, PCI_PRODUCT_XIRCOM_MODEM_56K, 0, 0],
        [0xffff, 0xffff, 0, 0],
        &[PucPort::new(PUC_PORT_COM, 0x10, 0x0000)],
    ),
    // "Xircom CBEM56G Modem",
    PucDeviceDescription::new(
        [PCI_VENDOR_XIRCOM, PCI_PRODUCT_XIRCOM_CBEM56G, 0, 0],
        [0xffff, 0xffff, 0, 0],
        &[PucPort::new(PUC_PORT_COM, 0x10, 0x0000)],
    ),
    // "Xircom 56k Modem",
    PucDeviceDescription::new(
        [PCI_VENDOR_XIRCOM, PCI_PRODUCT_XIRCOM_MODEM56, 0, 0],
        [0xffff, 0xffff, 0, 0],
        &[PucPort::new(PUC_PORT_COM, 0x10, 0x0000)],
    ),
    // "WinChipHead CH351 (2S)",
    PucDeviceDescription::new(
        [PCI_VENDOR_WCH2, PCI_PRODUCT_WCH2_CH351, 0, 0],
        [0xffff, 0xffff, 0, 0],
        &[
            PucPort::new(PUC_PORT_COM, 0x10, 0x0000),
            PucPort::new(PUC_PORT_COM, 0x14, 0x0000),
        ],
    ),
    // "WinChipHead CH352",
    PucDeviceDescription::new(
        [PCI_VENDOR_WCH, PCI_PRODUCT_WCH_CH352, 0, 0],
        [0xffff, 0xffff, 0, 0],
        &[
            PucPort::new(PUC_PORT_COM, 0x10, 0x0000),
            PucPort::new(PUC_PORT_COM, 0x14, 0x0000),
        ],
    ),
    // "WinChipHead CH382 (2S)",
    PucDeviceDescription::new(
        [PCI_VENDOR_WCH2, PCI_PRODUCT_WCH2_CH382_1, 0, 0],
        [0xffff, 0xffff, 0, 0],
        &[
            PucPort::new(PUC_PORT_COM, 0x10, 0x00c0),
            PucPort::new(PUC_PORT_COM, 0x10, 0x00c8),
        ],
    ),
    // "WinChipHead CH382 (2S1P)",
    PucDeviceDescription::new(
        [PCI_VENDOR_WCH2, PCI_PRODUCT_WCH2_CH382_2, 0, 0],
        [0xffff, 0xffff, 0, 0],
        &[
            PucPort::new(PUC_PORT_COM, 0x10, 0x00c0),
            PucPort::new(PUC_PORT_COM, 0x10, 0x00c8),
        ],
    ),
    // "TXIC TX382B (2S)",
    PucDeviceDescription::new(
        [PCI_VENDOR_TXIC, PCI_PRODUCT_TXIC_TX382B, 0, 0],
        [0xffff, 0xffff, 0, 0],
        &[
            PucPort::new(PUC_PORT_COM, 0x10, 0x0000),
            PucPort::new(PUC_PORT_COM, 0x14, 0x0000),
        ],
    ),
    // "ASIX AX99100",
    PucDeviceDescription::new(
        [PCI_VENDOR_ASIX, PCI_PRODUCT_ASIX_AX99100, 0, 0],
        [0xffff, 0xffff, 0, 0],
        &[PucPort::new(PUC_PORT_COM, 0x10, 0x0000)],
    ),
    // "NetMos NM9820 UART"
    PucDeviceDescription::new(
        [PCI_VENDOR_NETMOS, PCI_PRODUCT_NETMOS_NM9820, 0, 0],
        [0xffff, 0xffff, 0, 0],
        &[PucPort::new(PUC_PORT_COM, 0x10, 0x0000)],
    ),
    // "MosChip MCS9865 Quad Serial Port"
    PucDeviceDescription::new(
        [PCI_VENDOR_MOSCHIP, PCI_PRODUCT_MOSCHIP_MCS9865, 0x1000, 0x4],
        [0xffff, 0xffff, 0xffff, 0xffff],
        &[
            PucPort::new(PUC_PORT_COM, 0x10, 0x0000),
            PucPort::new(PUC_PORT_COM, 0x14, 0x0000),
            PucPort::new(PUC_PORT_COM, 0x18, 0x0000),
            PucPort::new(PUC_PORT_COM, 0x1c, 0x0000),
        ],
    ),
    // "MosChip MCS9865 Dual Serial Port"
    PucDeviceDescription::new(
        [PCI_VENDOR_MOSCHIP, PCI_PRODUCT_MOSCHIP_MCS9865, 0x1000, 0x2],
        [0xffff, 0xffff, 0xffff, 0xffff],
        &[
            PucPort::new(PUC_PORT_COM, 0x10, 0x0000),
            PucPort::new(PUC_PORT_COM, 0x14, 0x0000),
        ],
    ),
    // "MosChip MCS9865 Single Serial Port"
    PucDeviceDescription::new(
        [PCI_VENDOR_MOSCHIP, PCI_PRODUCT_MOSCHIP_MCS9865, 0x1000, 0x1],
        [0xffff, 0xffff, 0xffff, 0xffff],
        &[PucPort::new(PUC_PORT_COM, 0x10, 0x0000)],
    ),
    // "Redhat QEMU PCI Serial"
    PucDeviceDescription::new(
        [PCI_VENDOR_REDHAT, PCI_PRODUCT_REDHAT_SERIAL, 0x0000, 0x0000],
        [0xffff, 0xffff, 0x0000, 0x0000],
        &[PucPort::new(PUC_PORT_COM, 0x10, 0x0000)],
    ),
    // "Redhat QEMU PCI Serial 2x"
    PucDeviceDescription::new(
        [
            PCI_VENDOR_REDHAT,
            PCI_PRODUCT_REDHAT_SERIAL2,
            0x0000,
            0x0000,
        ],
        [0xffff, 0xffff, 0x0000, 0x0000],
        &[
            PucPort::new(PUC_PORT_COM, 0x10, 0x0000),
            PucPort::new(PUC_PORT_COM, 0x10, 0x0008),
        ],
    ),
    // "Redhat QEMU PCI Serial 4x"
    PucDeviceDescription::new(
        [
            PCI_VENDOR_REDHAT,
            PCI_PRODUCT_REDHAT_SERIAL4,
            0x0000,
            0x0000,
        ],
        [0xffff, 0xffff, 0x0000, 0x0000],
        &[
            PucPort::new(PUC_PORT_COM, 0x10, 0x0000),
            PucPort::new(PUC_PORT_COM, 0x10, 0x0008),
            PucPort::new(PUC_PORT_COM, 0x10, 0x0010),
            PucPort::new(PUC_PORT_COM, 0x10, 0x0018),
        ],
    ),
];
/* </CODE> */

/* <TESTS> */
#[cfg(test)]
mod tests {
    use super::*;
    use crate::dev::pci::pucvar::PUC_MAX_PORTS;

    #[test]
    fn the_table_is_whole() {
        assert_eq!(PUC_DEVS.len(), 218);
        let ports: usize = PUC_DEVS
            .iter()
            .map(|d| d.ports.iter().filter(|p| p.type_ != 0).count())
            .sum();
        assert_eq!(ports, 566);
        // The ports of an entry come first, without holes.
        for d in &PUC_DEVS {
            let n = d.ports.iter().filter(|p| p.type_ != 0).count();
            assert!(n >= 1 && n <= PUC_MAX_PORTS);
            assert!(d.ports[..n].iter().all(|p| p.type_ != 0));
            assert!(d.ports[..n].iter().all(|p| p.bar >= 0x10 && p.bar <= 0x24));
        }
    }

    #[test]
    fn the_last_three_entries_are_qemus_serial_cards() {
        let tail = &PUC_DEVS[PUC_DEVS.len() - 3..];

        for (d, (prod, n)) in tail.iter().zip([(2u16, 1), (3, 2), (4, 4)]) {
            assert_eq!(d.rval, [0x1b36, prod, 0, 0]);
            assert_eq!(d.rmask, [0xffff, 0xffff, 0, 0]);
            for i in 0..n {
                let p = &d.ports[i];
                assert_eq!((p.bar, p.offset), (0x10, 8 * i as u16));
            }
            assert_eq!(d.ports[n].type_, 0);
        }
    }

    #[test]
    #[ignore = "needs OPENBSD_SRC (just test-ref)"]
    fn the_table_matches_the_c() {
        let dir = crate::reftest::openbsd_src();
        let text = std::fs::read_to_string(dir.join("sys/dev/pci/pucdata.c")).unwrap();
        let defs = crate::reftest::defines("sys/dev/pci/pcidevs.h");
        let resolve = |tok: &str| -> i64 {
            let tok = tok.trim();
            if let Some(hex) = tok.strip_prefix("0x") {
                i64::from_str_radix(hex, 16).unwrap()
            } else if let Ok(n) = tok.parse() {
                n
            } else {
                crate::reftest::int(&defs, tok).unwrap_or_else(|| panic!("{tok}"))
            }
        };

        // Without the comments and line breaks: every `{ vendor, product, subvendor, subproduct }`
        // group (the one naming a PCI_VENDOR_) in order, and every `{ PUC_PORT_..., bar, offset }`.
        let mut flat = std::string::String::new();
        let mut rest = text.as_str();
        while let Some(i) = rest.find("/*") {
            flat.push_str(&rest[..i]);
            rest = &rest[i + 2..];
            rest = &rest[rest.find("*/").unwrap() + 2..];
        }
        flat.push_str(rest);
        let flat = flat.replace('\n', " ");

        let mut ids = std::vec::Vec::new();
        let mut ports = std::vec::Vec::new();
        for group in flat.split('{').skip(1) {
            let Some(inner) = group.split('}').next() else {
                continue;
            };
            let f: std::vec::Vec<&str> = inner.split(',').collect();
            if f.len() >= 4 && f[0].contains("PCI_VENDOR_") {
                ids.push([resolve(f[0]), resolve(f[1]), resolve(f[2]), resolve(f[3])]);
            } else if f.len() >= 3 && f[0].trim().starts_with("PUC_PORT_") {
                let t = match f[0].trim() {
                    "PUC_PORT_LPT" => PUC_PORT_LPT,
                    "PUC_PORT_COM" => PUC_PORT_COM,
                    "PUC_PORT_COM_MUL4" => PUC_PORT_COM_MUL4,
                    "PUC_PORT_COM_MUL8" => PUC_PORT_COM_MUL8,
                    "PUC_PORT_COM_MUL10" => PUC_PORT_COM_MUL10,
                    "PUC_PORT_COM_MUL128" => PUC_PORT_COM_MUL128,
                    "PUC_PORT_COM_XR17V35X" => PUC_PORT_COM_XR17V35X,
                    other => panic!("{other}"),
                };
                ports.push((i64::from(t), resolve(f[1]), resolve(f[2])));
            }
        }

        assert_eq!(ids.len(), PUC_DEVS.len());
        for (d, id) in PUC_DEVS.iter().zip(&ids) {
            let got = [
                i64::from(d.rval[0]),
                i64::from(d.rval[1]),
                i64::from(d.rval[2]),
                i64::from(d.rval[3]),
            ];
            assert_eq!(&got, id);
        }

        let table: std::vec::Vec<(i64, i64, i64)> = PUC_DEVS
            .iter()
            .flat_map(|d| d.ports.iter().filter(|p| p.type_ != 0))
            .map(|p| (i64::from(p.type_), i64::from(p.bar), i64::from(p.offset)))
            .collect();
        assert_eq!(table, ports);
    }
}
/* </TESTS> */
