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
 * Copyright (c) 2000 Manuel Bouyer.
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
//! `siop* at pci?`: the PCI front-end of siop(4) ("SYM53c8xx PCI-SCSI I/O Processors
//! driver: PCI front-end"): matches the Symbios chips `siop_pci_common` knows and attaches
//! the adapter.
//!
//! Upstream: sys/dev/pci/siop_pci.c @ 3ce1f3f79392
//!
//! ## Deviations
//! - `struct siop_pci_softc` is `#[repr(C)]`, the siop softc first (itself headed by the
//!   device), as in the C; `siop.rs` relies on that order.
//! - The softc is reached as `&'static` (softcs are never freed while the device exists).

use core::ffi::c_void;
use core::ptr;

use crate::dev::ic::siop::{siop_attach, siop_intr};
use crate::dev::ic::siopvar::SiopSoftc;
use crate::dev::pci::pcireg::pci_revision;
use crate::dev::pci::pcivar::PciAttachArgs;
use crate::dev::pci::siop_pci_common::{
    SiopPciCommonSoftc, siop_lookup_product, siop_pci_attach_common,
};
use crate::sys::device::{CfMatch, Cfattach, Device, Softc};

/// `struct siop_pci_softc`.
#[repr(C)]
pub struct SiopPciSoftc {
    /// `siop`.
    pub siop: SiopSoftc,
    /// `siop_pci`.
    pub siop_pci: SiopPciCommonSoftc,
}

// SAFETY: `#[repr(C)]` with the siop softc first, itself headed by the device (through the
// common softc); every member is a `Cell` of an integer, a raw pointer, an `Option` of a
// reference, a bus tag or a chipset tag, a queue head or the iopool, all valid as zero
// bytes (`sys/queue.rs`, `scsi/scsiconf.rs`).
unsafe impl Softc for SiopPciSoftc {}

/// `siop_pci_ca`.
pub static SIOP_PCI_CA: Cfattach = Cfattach {
    ca_devsize: size_of::<SiopPciSoftc>(),
    ca_match: Some(siop_pci_match),
    ca_attach: siop_pci_attach,
    ca_detach: None,
    ca_activate: None,
};

/// `siop_pci_match`: look if it's a known product.
pub fn siop_pci_match(_parent: Option<&Device>, _match: &CfMatch, aux: *mut c_void) -> i32 {
    // SAFETY: pci attaches its children with a `pci_attach_args`.
    let pa = unsafe { &*aux.cast::<PciAttachArgs>() };

    i32::from(siop_lookup_product(pa.pa_id, pci_revision(pa.pa_class) as i32).is_some())
}

/// `siop_pci_attach`.
pub fn siop_pci_attach(_parent: Option<&Device>, self_: &Device, aux: *mut c_void) {
    // SAFETY: pci attaches its children with a `pci_attach_args` it owns for the duration of
    // the attach.
    let pa = unsafe { &*aux.cast::<PciAttachArgs>() };
    // SAFETY: `self_` was made for `siop_pci_ca`, whose softc is a `SiopPciSoftc`; softcs
    // are never freed while the device exists, so the softc may be borrowed for 'static.
    let sc: &'static SiopPciSoftc = unsafe { &*ptr::from_ref(self_.softc::<SiopPciSoftc>()) };

    if !siop_pci_attach_common(&sc.siop_pci, &sc.siop.sc_c, pa, siop_intr) {
        return;
    }

    siop_attach(&sc.siop);
}
/* </CODE> */
