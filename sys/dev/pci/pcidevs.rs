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
//! - Partial: only the IDs some ported file names are present (`pci.c`'s `pci_set_powerstate`
//!   and `pci_quirks.c`). The whole header, and `pcidevs_data.h` for `PCIVERBOSE`, wait for a
//!   generator in `tools/xtask` in the manner of `gen-syscalls` (`docs/ARCHITECTURE.md`).
//! - The IDs are `u32`, the type `pci_vendor`/`pci_product` return.

/// `PCI_VENDOR_CIRRUS`: Cirrus Logic.
pub const PCI_VENDOR_CIRRUS: u32 = 0x1013;
/// `PCI_VENDOR_AMD`: AMD.
pub const PCI_VENDOR_AMD: u32 = 0x1022;
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

/// `PCI_PRODUCT_CIRRUS_CL_PD6729`: CL-PD6729.
pub const PCI_PRODUCT_CIRRUS_CL_PD6729: u32 = 0x1100;

/// `PCI_PRODUCT_INTEL_82371FB_ISA`: 82371FB ISA.
pub const PCI_PRODUCT_INTEL_82371FB_ISA: u32 = 0x122e;

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    #[ignore = "needs OPENBSD_SRC (just test-ref)"]
    fn ids_match_the_generated_header() {
        let defs = crate::reftest::defines("sys/dev/pci/pcidevs.h");
        for (name, value) in [
            ("PCI_VENDOR_CIRRUS", PCI_VENDOR_CIRRUS),
            ("PCI_VENDOR_AMD", PCI_VENDOR_AMD),
            ("PCI_VENDOR_INTEL", PCI_VENDOR_INTEL),
            ("PCI_VENDOR_INVALID", PCI_VENDOR_INVALID),
            ("PCI_PRODUCT_AMD_17_1X_XHCI_1", PCI_PRODUCT_AMD_17_1X_XHCI_1),
            ("PCI_PRODUCT_AMD_17_1X_XHCI_2", PCI_PRODUCT_AMD_17_1X_XHCI_2),
            ("PCI_PRODUCT_AMD_17_6X_XHCI", PCI_PRODUCT_AMD_17_6X_XHCI),
            ("PCI_PRODUCT_CIRRUS_CL_PD6729", PCI_PRODUCT_CIRRUS_CL_PD6729),
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
