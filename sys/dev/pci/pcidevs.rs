/*
 * THIS FILE AUTOMATICALLY GENERATED.  DO NOT EDIT.
 *
 * generated from:
 *	OpenBSD: pcidevs,v 1.2147 2026/08/14 03:32:01 jsg Exp
 */
/*	$NetBSD: pcidevs,v 1.30 1997/06/24 06:20:24 thorpej Exp $	*/
/* <LICENSES> */
/*
 * Copyright (c) 1995, 1996 Christopher G. Demetriou
 * All rights reserved.
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

//! `<dev/pci/pcidevs.h>`: PCI vendor and product IDs, the subset the ported code names.
//!
//! Upstream: sys/dev/pci/pcidevs.h @ 3ce1f3f79392
//!
//! OpenBSD generates this header (and `pcidevs_data.h`, the names `PCIVERBOSE` prints) from
//! `sys/dev/pci/pcidevs` with `devlist2h.awk`: about 350 vendors and 9600 products, 640 KB
//! of defines and 800 KB of name tables. Here the IDs are added by hand as the code that uses
//! them is ported, with the generated names and values.
//!
//! ## Deviations
//! - Partial: only the IDs some ported file names are present (`pci.c`'s `pci_set_powerstate`,
//!   `pci_quirks.c`, `virtio_pci.c`, `nvme_pci.c`, `xhci_pci.c`, `auich.c`, `azalia.c` and
//!   `azalia_codec.c`). The whole header, and `pcidevs_data.h` for
//!   `PCIVERBOSE`, wait for a generator in `tools/xtask` in the manner of `gen-syscalls`
//!   (`docs/ARCHITECTURE.md`).
//! - The IDs are `u32`, the type `pci_vendor`/`pci_product` return.

/// `PCI_VENDOR_OPENBSD`: OpenBSD.
pub const PCI_VENDOR_OPENBSD: u32 = 0x0b5d;
/// `PCI_VENDOR_CIRRUS`: Cirrus Logic.
pub const PCI_VENDOR_CIRRUS: u32 = 0x1013;
/// `PCI_VENDOR_AMD`: AMD.
pub const PCI_VENDOR_AMD: u32 = 0x1022;
/// `PCI_VENDOR_APPLE`: Apple.
pub const PCI_VENDOR_APPLE: u32 = 0x106b;
/// `PCI_VENDOR_SIS`: SiS.
pub const PCI_VENDOR_SIS: u32 = 0x1039;
/// `PCI_VENDOR_NVIDIA`: NVIDIA.
pub const PCI_VENDOR_NVIDIA: u32 = 0x10de;
/// `PCI_VENDOR_QUMRANET`: Qumranet.
pub const PCI_VENDOR_QUMRANET: u32 = 0x1af4;
/// `PCI_VENDOR_FRESCO`: Fresco Logic.
pub const PCI_VENDOR_FRESCO: u32 = 0x1b73;
/// `PCI_VENDOR_INTEL`: Intel.
pub const PCI_VENDOR_INTEL: u32 = 0x8086;
/// `PCI_VENDOR_INVALID`: INVALID VENDOR ID.
pub const PCI_VENDOR_INVALID: u32 = 0xffff;

/// `PCI_PRODUCT_AMD_17_1X_XHCI_1`: 17h/1xh xHCI.
pub const PCI_PRODUCT_AMD_17_1X_XHCI_1: u32 = 0x15e0;
/// `PCI_PRODUCT_AMD_17_1X_XHCI_2`: 17h/1xh xHCI.
pub const PCI_PRODUCT_AMD_17_1X_XHCI_2: u32 = 0x15e1;
/// `PCI_PRODUCT_AMD_17_6X_XHCI`: 17h/6xh xHCI.
pub const PCI_PRODUCT_AMD_17_6X_XHCI: u32 = 0x1639;

/// `PCI_PRODUCT_APPLE_NVME1`: NVMe.
pub const PCI_PRODUCT_APPLE_NVME1: u32 = 0x2001;
/// `PCI_PRODUCT_APPLE_NVME2`: NVMe.
pub const PCI_PRODUCT_APPLE_NVME2: u32 = 0x2003;
/// `PCI_PRODUCT_APPLE_NVME3`: NVMe.
pub const PCI_PRODUCT_APPLE_NVME3: u32 = 0x2005;

/// `PCI_PRODUCT_CIRRUS_CL_PD6729`: CL-PD6729.
pub const PCI_PRODUCT_CIRRUS_CL_PD6729: u32 = 0x1100;

/// `PCI_PRODUCT_FRESCO_FL1000`: FL1000 xHCI.
pub const PCI_PRODUCT_FRESCO_FL1000: u32 = 0x1000;
/// `PCI_PRODUCT_FRESCO_FL1400`: FL1400 xHCI.
pub const PCI_PRODUCT_FRESCO_FL1400: u32 = 0x1400;

/// `PCI_PRODUCT_INTEL_82371FB_ISA`: 82371FB ISA.
pub const PCI_PRODUCT_INTEL_82371FB_ISA: u32 = 0x122e;

/// `PCI_PRODUCT_OPENBSD_CONTROL`: VMM Control.
pub const PCI_PRODUCT_OPENBSD_CONTROL: u32 = 0x0777;

// auich(4)'s controllers.
/// `PCI_PRODUCT_AMD_PBC768_ACA`: 768 AC97.
pub const PCI_PRODUCT_AMD_PBC768_ACA: u32 = 0x7445;
/// `PCI_PRODUCT_AMD_8111_ACA`: 8111 AC97.
pub const PCI_PRODUCT_AMD_8111_ACA: u32 = 0x746d;
/// `PCI_PRODUCT_INTEL_82801AA_ACA`: 82801AA AC97.
pub const PCI_PRODUCT_INTEL_82801AA_ACA: u32 = 0x2415;
/// `PCI_PRODUCT_INTEL_82801AB_ACA`: 82801AB AC97.
pub const PCI_PRODUCT_INTEL_82801AB_ACA: u32 = 0x2425;
/// `PCI_PRODUCT_INTEL_82801BA_ACA`: 82801BA AC97.
pub const PCI_PRODUCT_INTEL_82801BA_ACA: u32 = 0x2445;
/// `PCI_PRODUCT_INTEL_82801CA_ACA`: 82801CA/CAM AC97.
pub const PCI_PRODUCT_INTEL_82801CA_ACA: u32 = 0x2485;
/// `PCI_PRODUCT_INTEL_82801DB_ACA`: 82801DB AC97.
pub const PCI_PRODUCT_INTEL_82801DB_ACA: u32 = 0x24c5;
/// `PCI_PRODUCT_INTEL_82801EB_ACA`: 82801EB/ER AC97.
pub const PCI_PRODUCT_INTEL_82801EB_ACA: u32 = 0x24d5;
/// `PCI_PRODUCT_INTEL_6300ESB_ACA`: 6300ESB AC97.
pub const PCI_PRODUCT_INTEL_6300ESB_ACA: u32 = 0x25a6;
/// `PCI_PRODUCT_INTEL_82801FB_ACA`: 82801FB AC97.
pub const PCI_PRODUCT_INTEL_82801FB_ACA: u32 = 0x266e;
/// `PCI_PRODUCT_INTEL_6321ESB_ACA`: 6321ESB AC97.
pub const PCI_PRODUCT_INTEL_6321ESB_ACA: u32 = 0x2698;
/// `PCI_PRODUCT_INTEL_82801GB_ACA`: 82801GB AC97.
pub const PCI_PRODUCT_INTEL_82801GB_ACA: u32 = 0x27de;
/// `PCI_PRODUCT_INTEL_82440MX_ACA`: 82440MX AC97.
pub const PCI_PRODUCT_INTEL_82440MX_ACA: u32 = 0x7195;
/// `PCI_PRODUCT_NVIDIA_MCP04_AC97`: MCP04 AC97.
pub const PCI_PRODUCT_NVIDIA_MCP04_AC97: u32 = 0x003a;
/// `PCI_PRODUCT_NVIDIA_NFORCE4_AC`: nForce4 AC97.
pub const PCI_PRODUCT_NVIDIA_NFORCE4_AC: u32 = 0x0059;
/// `PCI_PRODUCT_NVIDIA_NFORCE2_ACA`: nForce2 AC97.
pub const PCI_PRODUCT_NVIDIA_NFORCE2_ACA: u32 = 0x006a;
/// `PCI_PRODUCT_NVIDIA_NFORCE2_400_ACA`: nForce2 400 AC97.
pub const PCI_PRODUCT_NVIDIA_NFORCE2_400_ACA: u32 = 0x008a;
/// `PCI_PRODUCT_NVIDIA_NFORCE3_ACA`: nForce3 AC97.
pub const PCI_PRODUCT_NVIDIA_NFORCE3_ACA: u32 = 0x00da;
/// `PCI_PRODUCT_NVIDIA_NFORCE3_250_ACA`: nForce3 250 AC97.
pub const PCI_PRODUCT_NVIDIA_NFORCE3_250_ACA: u32 = 0x00ea;
/// `PCI_PRODUCT_NVIDIA_NFORCE_ACA`: nForce AC97.
pub const PCI_PRODUCT_NVIDIA_NFORCE_ACA: u32 = 0x01b1;
/// `PCI_PRODUCT_NVIDIA_MCP51_ACA`: MCP51 AC97.
pub const PCI_PRODUCT_NVIDIA_MCP51_ACA: u32 = 0x026b;
/// `PCI_PRODUCT_SIS_7012_ACA`: 7012 AC97.
pub const PCI_PRODUCT_SIS_7012_ACA: u32 = 0x7012;

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    #[ignore = "needs OPENBSD_SRC (just test-ref)"]
    fn ids_match_the_generated_header() {
        let defs = crate::reftest::defines("sys/dev/pci/pcidevs.h");
        for (name, value) in [
            ("PCI_VENDOR_OPENBSD", PCI_VENDOR_OPENBSD),
            ("PCI_VENDOR_CIRRUS", PCI_VENDOR_CIRRUS),
            ("PCI_VENDOR_QUMRANET", PCI_VENDOR_QUMRANET),
            ("PCI_PRODUCT_OPENBSD_CONTROL", PCI_PRODUCT_OPENBSD_CONTROL),
            ("PCI_VENDOR_APPLE", PCI_VENDOR_APPLE),
            ("PCI_PRODUCT_APPLE_NVME1", PCI_PRODUCT_APPLE_NVME1),
            ("PCI_PRODUCT_APPLE_NVME2", PCI_PRODUCT_APPLE_NVME2),
            ("PCI_PRODUCT_APPLE_NVME3", PCI_PRODUCT_APPLE_NVME3),
            ("PCI_VENDOR_AMD", PCI_VENDOR_AMD),
            ("PCI_VENDOR_INTEL", PCI_VENDOR_INTEL),
            ("PCI_VENDOR_INVALID", PCI_VENDOR_INVALID),
            ("PCI_VENDOR_SIS", PCI_VENDOR_SIS),
            ("PCI_VENDOR_NVIDIA", PCI_VENDOR_NVIDIA),
            ("PCI_PRODUCT_AMD_PBC768_ACA", PCI_PRODUCT_AMD_PBC768_ACA),
            ("PCI_PRODUCT_AMD_8111_ACA", PCI_PRODUCT_AMD_8111_ACA),
            (
                "PCI_PRODUCT_INTEL_82801AA_ACA",
                PCI_PRODUCT_INTEL_82801AA_ACA,
            ),
            (
                "PCI_PRODUCT_INTEL_82801AB_ACA",
                PCI_PRODUCT_INTEL_82801AB_ACA,
            ),
            (
                "PCI_PRODUCT_INTEL_82801BA_ACA",
                PCI_PRODUCT_INTEL_82801BA_ACA,
            ),
            (
                "PCI_PRODUCT_INTEL_82801CA_ACA",
                PCI_PRODUCT_INTEL_82801CA_ACA,
            ),
            (
                "PCI_PRODUCT_INTEL_82801DB_ACA",
                PCI_PRODUCT_INTEL_82801DB_ACA,
            ),
            (
                "PCI_PRODUCT_INTEL_82801EB_ACA",
                PCI_PRODUCT_INTEL_82801EB_ACA,
            ),
            (
                "PCI_PRODUCT_INTEL_6300ESB_ACA",
                PCI_PRODUCT_INTEL_6300ESB_ACA,
            ),
            (
                "PCI_PRODUCT_INTEL_82801FB_ACA",
                PCI_PRODUCT_INTEL_82801FB_ACA,
            ),
            (
                "PCI_PRODUCT_INTEL_6321ESB_ACA",
                PCI_PRODUCT_INTEL_6321ESB_ACA,
            ),
            (
                "PCI_PRODUCT_INTEL_82801GB_ACA",
                PCI_PRODUCT_INTEL_82801GB_ACA,
            ),
            (
                "PCI_PRODUCT_INTEL_82440MX_ACA",
                PCI_PRODUCT_INTEL_82440MX_ACA,
            ),
            (
                "PCI_PRODUCT_NVIDIA_MCP04_AC97",
                PCI_PRODUCT_NVIDIA_MCP04_AC97,
            ),
            (
                "PCI_PRODUCT_NVIDIA_NFORCE4_AC",
                PCI_PRODUCT_NVIDIA_NFORCE4_AC,
            ),
            (
                "PCI_PRODUCT_NVIDIA_NFORCE2_ACA",
                PCI_PRODUCT_NVIDIA_NFORCE2_ACA,
            ),
            (
                "PCI_PRODUCT_NVIDIA_NFORCE2_400_ACA",
                PCI_PRODUCT_NVIDIA_NFORCE2_400_ACA,
            ),
            (
                "PCI_PRODUCT_NVIDIA_NFORCE3_ACA",
                PCI_PRODUCT_NVIDIA_NFORCE3_ACA,
            ),
            (
                "PCI_PRODUCT_NVIDIA_NFORCE3_250_ACA",
                PCI_PRODUCT_NVIDIA_NFORCE3_250_ACA,
            ),
            (
                "PCI_PRODUCT_NVIDIA_NFORCE_ACA",
                PCI_PRODUCT_NVIDIA_NFORCE_ACA,
            ),
            ("PCI_PRODUCT_NVIDIA_MCP51_ACA", PCI_PRODUCT_NVIDIA_MCP51_ACA),
            ("PCI_PRODUCT_SIS_7012_ACA", PCI_PRODUCT_SIS_7012_ACA),
            ("PCI_PRODUCT_AMD_17_1X_XHCI_1", PCI_PRODUCT_AMD_17_1X_XHCI_1),
            ("PCI_PRODUCT_AMD_17_1X_XHCI_2", PCI_PRODUCT_AMD_17_1X_XHCI_2),
            ("PCI_PRODUCT_AMD_17_6X_XHCI", PCI_PRODUCT_AMD_17_6X_XHCI),
            ("PCI_PRODUCT_CIRRUS_CL_PD6729", PCI_PRODUCT_CIRRUS_CL_PD6729),
            ("PCI_VENDOR_FRESCO", PCI_VENDOR_FRESCO),
            ("PCI_PRODUCT_FRESCO_FL1000", PCI_PRODUCT_FRESCO_FL1000),
            ("PCI_PRODUCT_FRESCO_FL1400", PCI_PRODUCT_FRESCO_FL1400),
            (
                "PCI_PRODUCT_INTEL_82371FB_ISA",
                PCI_PRODUCT_INTEL_82371FB_ISA,
            ),
        ] {
            assert_eq!(
                crate::reftest::int(&defs, name),
                Some(i64::from(value)),
                "{name}"
            );
        }
    }
}
