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

/*-
 * Copyright (c) 1997, 1998 The NetBSD Foundation, Inc.
 * All rights reserved.
 *
 * This code is derived from software contributed to The NetBSD Foundation
 * by Jason R. Thorpe of the Numerical Aerospace Simulation Facility,
 * NASA Ames Research Center.
 *
 * Redistribution and use in source and binary forms, with or without
 * modification, are permitted provided that the following conditions
 * are met:
 * 1. Redistributions of source code must retain the above copyright
 *    notice, this list of conditions and the following disclaimer.
 * 2. Redistributions in binary form must reproduce the above copyright
 *    notice, this list of conditions and the following disclaimer in the
 *    documentation and/or other materials provided with the distribution.
 *
 * THIS SOFTWARE IS PROVIDED BY THE NETBSD FOUNDATION, INC. AND CONTRIBUTORS
 * ``AS IS'' AND ANY EXPRESS OR IMPLIED WARRANTIES, INCLUDING, BUT NOT LIMITED
 * TO, THE IMPLIED WARRANTIES OF MERCHANTABILITY AND FITNESS FOR A PARTICULAR
 * PURPOSE ARE DISCLAIMED.  IN NO EVENT SHALL THE FOUNDATION OR CONTRIBUTORS
 * BE LIABLE FOR ANY DIRECT, INDIRECT, INCIDENTAL, SPECIAL, EXEMPLARY, OR
 * CONSEQUENTIAL DAMAGES (INCLUDING, BUT NOT LIMITED TO, PROCUREMENT OF
 * SUBSTITUTE GOODS OR SERVICES; LOSS OF USE, DATA, OR PROFITS; OR BUSINESS
 * INTERRUPTION) HOWEVER CAUSED AND ON ANY THEORY OF LIABILITY, WHETHER IN
 * CONTRACT, STRICT LIABILITY, OR TORT (INCLUDING NEGLIGENCE OR OTHERWISE)
 * ARISING IN ANY WAY OUT OF THE USE OF THIS SOFTWARE, EVEN IF ADVISED OF THE
 * POSSIBILITY OF SUCH DAMAGE.
 */
/* </LICENSES> */

/* <CODE> */
//! ne(4) on PCI: NE2000-compatible Ethernet boards (`dev/pci/if_ne_pci.c`).
//!
//! Upstream: sys/dev/pci/if_ne_pci.c @ 3ce1f3f79392
//!
//! The PCI front end of `ne2000.c`: it maps the board's I/O space (the DP8390 registers in
//! the first 16 ports, the ASIC data port in the next 16), establishes the interrupt, and
//! lets `ne2000_attach` do the rest. The Realtek 8029, which is QEMU's `ne2k_pci`, brings its
//! own media functions (`rtl80x9.c`); the other boards have none.
//!
//! ## Deviations
//! - The softc embeds [`Ne2000Softc`] first (`#[repr(C)]`, all-zero valid), as
//!   `struct ne_pci_softc` does.
//! - The per-product media functions are `Option`s of `fn` pointers.

use core::cell::Cell;
use core::ffi::c_void;
use core::ptr;

use crate::dev::ic::dp8390::dp8390_intr;
use crate::dev::ic::dp8390var::{Dp8390Fn, Dp8390MediaChangeFn, Dp8390MediaStatusFn, Dp8390Softc};
use crate::dev::ic::ne2000::ne2000_attach;
use crate::dev::ic::ne2000reg::{NE2000_ASIC_NPORTS, NE2000_ASIC_OFFSET};
use crate::dev::ic::ne2000var::Ne2000Softc;
use crate::dev::ic::rtl80x9::{
    rtl80x9_init_card, rtl80x9_media_init, rtl80x9_mediachange, rtl80x9_mediastatus,
};
use crate::dev::pci::pci_map::pci_mapreg_map;
use crate::dev::pci::pcidevs::{
    PCI_PRODUCT_COMPEX_COMPEXE, PCI_PRODUCT_KTI_KTIE, PCI_PRODUCT_NETVIN_NV5000,
    PCI_PRODUCT_REALTEK_RT8029, PCI_PRODUCT_SURECOM_NE34, PCI_PRODUCT_VIATECH_VT86C926,
    PCI_PRODUCT_WINBOND_W89C940F, PCI_PRODUCT_WINBOND2_W89C940, PCI_VENDOR_COMPEX, PCI_VENDOR_KTI,
    PCI_VENDOR_NETVIN, PCI_VENDOR_REALTEK, PCI_VENDOR_SURECOM, PCI_VENDOR_VIATECH,
    PCI_VENDOR_WINBOND, PCI_VENDOR_WINBOND2,
};
use crate::dev::pci::pcireg::{
    PCI_MAPREG_START, PCI_MAPREG_TYPE_IO, PciProductId, PciVendorId, pci_product, pci_vendor,
};
use crate::dev::pci::pcivar::PciAttachArgs;
use crate::kern::subr_prf::{Str, printf};
use crate::machine::bus::{bus_space_subregion, bus_space_unmap};
use crate::machine::intr::IPL_NET;
use crate::machine::pci_machdep::{pci_intr_establish, pci_intr_map, pci_intr_string};
use crate::sys::device::{CfMatch, Cfattach, Device, Softc};

/// `struct ne_pci_softc`.
#[repr(C)]
pub struct NePciSoftc {
    /// `sc_ne2000`: real "ne2000" softc.
    pub sc_ne2000: Ne2000Softc,

    /// `sc_ih`: interrupt handle.
    pub sc_ih: Cell<*mut c_void>,
}

// SAFETY: `#[repr(C)]` with the `Ne2000Softc` (a `Softc`, its device first) first; the other
// member is a `Cell` of a raw pointer, all-zero valid.
unsafe impl Softc for NePciSoftc {}

/// `ne_pci_ca`.
pub static NE_PCI_CA: Cfattach = Cfattach {
    ca_devsize: size_of::<NePciSoftc>(),
    ca_match: Some(ne_pci_match),
    ca_attach: ne_pci_attach,
    ca_detach: None,
    ca_activate: None,
};

/// `struct ne_pci_product`.
pub struct NePciProduct {
    /// `npp_vendor`.
    pub npp_vendor: PciVendorId,
    /// `npp_product`.
    pub npp_product: PciProductId,
    /// `npp_mediachange`.
    pub npp_mediachange: Option<Dp8390MediaChangeFn>,
    /// `npp_mediastatus`.
    pub npp_mediastatus: Option<Dp8390MediaStatusFn>,
    /// `npp_init_card`.
    pub npp_init_card: Option<Dp8390Fn>,
    /// `npp_media_init`.
    pub npp_media_init: Option<Dp8390Fn>,
}

/// An `ne_pci_prod[]` entry without media functions.
const fn plain(vendor: u32, product: u32) -> NePciProduct {
    NePciProduct {
        npp_vendor: vendor as PciVendorId,
        npp_product: product as PciProductId,
        npp_mediachange: None,
        npp_mediastatus: None,
        npp_init_card: None,
        npp_media_init: None,
    }
}

/// `ne_pci_prod[]`.
pub static NE_PCI_PROD: [NePciProduct; 8] = [
    NePciProduct {
        npp_vendor: PCI_VENDOR_REALTEK as PciVendorId,
        npp_product: PCI_PRODUCT_REALTEK_RT8029 as PciProductId,
        npp_mediachange: Some(rtl80x9_mediachange),
        npp_mediastatus: Some(rtl80x9_mediastatus),
        npp_init_card: Some(rtl80x9_init_card),
        npp_media_init: Some(rtl80x9_media_init),
    }, // Realtek 8029
    plain(PCI_VENDOR_WINBOND, PCI_PRODUCT_WINBOND_W89C940F), // Winbond 89C940F
    plain(PCI_VENDOR_VIATECH, PCI_PRODUCT_VIATECH_VT86C926), // VIA Technologies VT86C926
    plain(PCI_VENDOR_SURECOM, PCI_PRODUCT_SURECOM_NE34),     // Surecom NE-34
    plain(PCI_VENDOR_NETVIN, PCI_PRODUCT_NETVIN_NV5000),     // NetVin 5000
    // XXX The following entries need sanity checking in pcidevs
    plain(PCI_VENDOR_COMPEX, PCI_PRODUCT_COMPEX_COMPEXE), // Compex
    plain(PCI_VENDOR_WINBOND2, PCI_PRODUCT_WINBOND2_W89C940), // ProLAN
    plain(PCI_VENDOR_KTI, PCI_PRODUCT_KTI_KTIE),          // KTI
];

/// `ne_pci_lookup`.
pub fn ne_pci_lookup(pa: &PciAttachArgs) -> Option<&'static NePciProduct> {
    NE_PCI_PROD.iter().find(|npp| {
        pci_vendor(pa.pa_id) == u32::from(npp.npp_vendor)
            && pci_product(pa.pa_id) == u32::from(npp.npp_product)
    })
}

/// `PCI_CBIO`: Configuration Base IO Address.
const PCI_CBIO: i32 = PCI_MAPREG_START;

/// `ne_pci_match`.
pub fn ne_pci_match(_parent: Option<&Device>, _match: &CfMatch, aux: *mut c_void) -> i32 {
    // SAFETY: pci attaches its children with a `pci_attach_args`.
    let pa = unsafe { &*aux.cast::<PciAttachArgs>() };

    i32::from(ne_pci_lookup(pa).is_some())
}

/// `ne_pci_attach`.
pub fn ne_pci_attach(_parent: Option<&Device>, self_: &Device, aux: *mut c_void) {
    // SAFETY: the device was made for `ne_pci_ca`, whose softc is a `NePciSoftc`; softcs are
    // never freed while the device exists, so the softc may be borrowed for 'static.
    let psc: &'static NePciSoftc = unsafe { &*ptr::from_ref(self_.softc::<NePciSoftc>()) };
    let nsc: &'static Ne2000Softc = &psc.sc_ne2000;
    let dsc: &'static Dp8390Softc = &nsc.sc_dp8390;
    // SAFETY: pci attaches its children with a `pci_attach_args` it owns for the duration of
    // the attach.
    let pa = unsafe { &*aux.cast::<PciAttachArgs>() };
    let pc = pa.pa_pc;

    let Some(npp) = ne_pci_lookup(pa) else {
        printf(format_args!("\n"));
        crate::kern::subr_prf::panic(format_args!("ne_pci_attach: impossible"));
    };

    let Ok((nict, nich, _, iosize)) = pci_mapreg_map(pa, PCI_CBIO, PCI_MAPREG_TYPE_IO, 0, 0) else {
        printf(format_args!(": can't map i/o space\n"));
        return;
    };

    let asict = nict;
    let Ok(asich) = bus_space_subregion(nict, nich, NE2000_ASIC_OFFSET, NE2000_ASIC_NPORTS) else {
        printf(format_args!(": can't subregion i/o space\n"));
        bus_space_unmap(nict, nich, iosize);
        return;
    };

    dsc.sc_regt.set(Some(nict));
    dsc.sc_regh.set(Some(nich));

    nsc.sc_asict.set(Some(asict));
    nsc.sc_asich.set(Some(asich));

    // This interface is always enabled.
    dsc.sc_enabled.set(1);

    dsc.sc_mediachange.set(npp.npp_mediachange);
    dsc.sc_mediastatus.set(npp.npp_mediastatus);
    dsc.sc_media_init.set(npp.npp_media_init);
    dsc.init_card.set(npp.npp_init_card);

    // Map and establish the interrupt.
    let Some(ih) = pci_intr_map(pa) else {
        printf(format_args!(": couldn't map interrupt\n"));
        bus_space_unmap(nict, nich, iosize);
        return;
    };
    let intrstr = pci_intr_string(pc, ih);
    let Some(cookie) = pci_intr_establish(
        pc,
        ih,
        IPL_NET,
        dp8390_intr,
        ptr::from_ref(dsc).cast_mut().cast(),
        dsc.devname(),
    ) else {
        printf(format_args!(
            ": couldn't establish interrupt at {}\n",
            Str(intrstr.as_bytes())
        ));
        bus_space_unmap(nict, nich, iosize);
        return;
    };
    psc.sc_ih.set(cookie.as_ptr());
    printf(format_args!(": {}", Str(intrstr.as_bytes())));

    // Do generic NE2000 attach. This will read the station address from the EEPROM.
    ne2000_attach(nsc, None);
}
/* </CODE> */
