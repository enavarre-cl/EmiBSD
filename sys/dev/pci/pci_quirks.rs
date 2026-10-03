/*	$OpenBSD: pci_quirks.c,v 1.7 2015/07/20 18:05:04 miod Exp $	*/
/*	$NetBSD: pci_quirks.c,v 1.1 1998/05/31 06:03:44 cgd Exp $	*/
/* <LICENSES> */
/*
 * Copyright (c) 1998 Christopher G. Demetriou.  All rights reserved.
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

//! PCI quirk data table and lookup function: `dev/pci/pci_quirks.c`.
//!
//! Upstream: sys/dev/pci/pci_quirks.c @ 3ce1f3f79392
//!
//! Devices whose header type lies about having several functions: `pci_enumerate_bus`
//! probes eight functions of a `PCI_QUIRK_MULTIFUNCTION` device and one of a
//! `PCI_QUIRK_MONOFUNCTION` one.
//!
//! ## Deviations
//! - `pci_lookup_quirkdata` returns an `Option` (`None` for the C's NULL).

use crate::dev::pci::pcidevs::{
    PCI_PRODUCT_CIRRUS_CL_PD6729, PCI_PRODUCT_INTEL_82371FB_ISA, PCI_VENDOR_CIRRUS,
    PCI_VENDOR_INTEL,
};
use crate::dev::pci::pcivar::{PCI_QUIRK_MONOFUNCTION, PCI_QUIRK_MULTIFUNCTION, PciQuirkdata};

/// `pci_quirks[]`.
static PCI_QUIRKS: [PciQuirkdata; 2] = [
    PciQuirkdata {
        vendor: PCI_VENDOR_INTEL,
        product: PCI_PRODUCT_INTEL_82371FB_ISA,
        quirks: PCI_QUIRK_MULTIFUNCTION,
    },
    PciQuirkdata {
        vendor: PCI_VENDOR_CIRRUS,
        product: PCI_PRODUCT_CIRRUS_CL_PD6729,
        quirks: PCI_QUIRK_MONOFUNCTION,
    },
];

/// `pci_lookup_quirkdata`: the quirks of a vendor and product, if any.
pub fn pci_lookup_quirkdata(vendor: u32, product: u32) -> Option<&'static PciQuirkdata> {
    PCI_QUIRKS
        .iter()
        .find(|q| q.vendor == vendor && q.product == product)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn lookup() {
        let q = pci_lookup_quirkdata(PCI_VENDOR_INTEL, PCI_PRODUCT_INTEL_82371FB_ISA);
        assert_eq!(q.map(|q| q.quirks), Some(PCI_QUIRK_MULTIFUNCTION));
        let q = pci_lookup_quirkdata(PCI_VENDOR_CIRRUS, PCI_PRODUCT_CIRRUS_CL_PD6729);
        assert_eq!(q.map(|q| q.quirks), Some(PCI_QUIRK_MONOFUNCTION));
        assert!(pci_lookup_quirkdata(PCI_VENDOR_INTEL, 0x29c0).is_none());
    }
}
