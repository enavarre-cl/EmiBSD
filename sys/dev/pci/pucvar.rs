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
//! PCI "universal" communications card software structures: `dev/pci/pucvar.h`.
//!
//! Upstream: sys/dev/pci/pucvar.h @ 3ce1f3f79392
//!
//! The description of one card ([`PucDeviceDescription`]: the PCI IDs it matches and where each
//! of its ports lives), the kinds of port ([`PUC_PORT_TYPES`]), what `puc(4)` hands to the
//! `com(4)` attachment ([`PucAttachArgs`]) and the softc of a `puc*` device ([`PucSoftc`]).
//!
//! ## Deviations
//! - `extern const struct puc_device_description puc_devs[]` and `puc_ndevs` are
//!   [`PUC_DEVS`](crate::dev::pci::pucdata::PUC_DEVS) and its length (`pucdata.rs`).
//! - The structures' `type` field is `type_`, the keyword's usual spelling.
//! - [`PucDeviceDescription::new`] takes the four IDs as `u32` (what `pcidevs.rs` has) and keeps
//!   the low 16 bits, as the C's `u_int16_t` initialisers do, and fills the 16-port array with
//!   `{ 0, 0, 0 }` (no port) after the ports it is given.
//! - `puc_attach_args` carries the tag and handle as `Option`s (empty until `puc_common_attach`
//!   fills them, the C's uninitialised fields); `intr_string` returns the `PciIntrStr` by
//!   value, as `pci_intr_string` does, and `intr_establish` takes the handler's name as a
//!   `&'static str` and returns the cookie `pci_intr_establish` returns.

use core::cell::Cell;
use core::ffi::c_void;
use core::ptr::NonNull;

use crate::dev::ic::comreg::COM_FREQ;
use crate::dev::pci::pcireg::PCI_MAPREG_START;
use crate::machine::bus::{BusAddr, BusSize, BusSpaceHandle, BusSpaceTag};
use crate::machine::pci_machdep::{PciIntrFn, PciIntrStr};
use crate::sys::device::{Device, Softc};

/// `PUC_MAX_PORTS`.
pub const PUC_MAX_PORTS: usize = 16;

/// One entry of `puc_device_description.ports[]`: a port's kind, the BAR it lives in (its
/// configuration-space offset) and its offset in that BAR.
#[derive(Clone, Copy)]
pub struct PucPort {
    /// `type`: a `PUC_PORT_*`, 0 for no port.
    pub type_: u8,
    /// `bar`: the BAR's register, `PCI_MAPREG_START + 4 * n`.
    pub bar: u8,
    /// `offset`: from the start of the BAR.
    pub offset: u16,
}

impl PucPort {
    /// A port of kind `type_` at `offset` in the BAR `bar`.
    pub const fn new(type_: i32, bar: u8, offset: u16) -> Self {
        Self {
            type_: type_ as u8,
            bar,
            offset,
        }
    }
}

/// `struct puc_device_description`: a card `puc(4)` knows. A PCI function matches when each of
/// its vendor, product, subsystem vendor and subsystem product IDs, masked by `rmask`, equals
/// `rval`.
pub struct PucDeviceDescription {
    /// `rval`: vendor, product, subsystem vendor, subsystem product.
    pub rval: [u16; 4],
    /// `rmask`: which bits of each ID count.
    pub rmask: [u16; 4],
    /// `ports`: the card's ports, the unused ones of type 0.
    pub ports: [PucPort; PUC_MAX_PORTS],
}

impl PucDeviceDescription {
    /// A description from its IDs, masks and ports (at most [`PUC_MAX_PORTS`]; a table built
    /// at compile time fails to compile with more).
    pub const fn new(rval: [u32; 4], rmask: [u32; 4], ports: &[PucPort]) -> Self {
        let mut d = Self {
            rval: [0; 4],
            rmask: [0; 4],
            ports: [PucPort::new(0, 0, 0); PUC_MAX_PORTS],
        };
        let mut i = 0;
        while i < 4 {
            d.rval[i] = rval[i] as u16;
            d.rmask[i] = rmask[i] as u16;
            i += 1;
        }
        let mut i = 0;
        while i < ports.len() {
            d.ports[i] = ports[i];
            i += 1;
        }
        d
    }
}

/// `PUC_PORT_LPT`.
pub const PUC_PORT_LPT: i32 = 1;
/// `PUC_PORT_COM`.
pub const PUC_PORT_COM: i32 = 2;
/// `PUC_PORT_COM_MUL4`: a UART clocked at 4 times the standard rate.
pub const PUC_PORT_COM_MUL4: i32 = 3;
/// `PUC_PORT_COM_MUL8`.
pub const PUC_PORT_COM_MUL8: i32 = 4;
/// `PUC_PORT_COM_MUL10`.
pub const PUC_PORT_COM_MUL10: i32 = 5;
/// `PUC_PORT_COM_MUL128`.
pub const PUC_PORT_COM_MUL128: i32 = 6;
/// `PUC_PORT_COM_XR17V35X`: an Exar XR17V35x UART.
pub const PUC_PORT_COM_XR17V35X: i32 = 7;

/// `struct puc_port_type`: a kind of port and the clock of its UART.
pub struct PucPortType {
    /// `type`.
    pub type_: i32,
    /// `freq`: in Hz, 0 for a parallel port.
    pub freq: u32,
}

/// `puc_port_types[]`.
pub static PUC_PORT_TYPES: [PucPortType; 7] = [
    PucPortType {
        type_: PUC_PORT_LPT,
        freq: 0,
    },
    PucPortType {
        type_: PUC_PORT_COM,
        freq: COM_FREQ as u32,
    },
    PucPortType {
        type_: PUC_PORT_COM_MUL4,
        freq: COM_FREQ as u32 * 4,
    },
    PucPortType {
        type_: PUC_PORT_COM_MUL8,
        freq: COM_FREQ as u32 * 8,
    },
    PucPortType {
        type_: PUC_PORT_COM_MUL10,
        freq: COM_FREQ as u32 * 10,
    },
    PucPortType {
        type_: PUC_PORT_COM_MUL128,
        freq: COM_FREQ as u32 * 128,
    },
    PucPortType {
        type_: PUC_PORT_COM_XR17V35X,
        freq: 125_000_000,
    },
];

/// `PUC_IS_LPT(type)`.
pub const fn puc_is_lpt(type_: i32) -> bool {
    type_ == PUC_PORT_LPT
}

/// `PUC_IS_COM(type)`: anything that is not a parallel port is a serial one.
pub const fn puc_is_com(type_: i32) -> bool {
    type_ != PUC_PORT_LPT
}

/// `PUC_PORT_BAR_INDEX(bar)`: the BAR number of a configuration-space offset.
pub const fn puc_port_bar_index(bar: i32) -> usize {
    ((bar - PCI_MAPREG_START) / 4) as usize
}

/// The `intr_string` member of `struct puc_attach_args`.
pub type PucIntrStringFn = fn(&PucAttachArgs) -> PciIntrStr;

/// The `intr_establish` member of `struct puc_attach_args`: `(paa, ipl, handler, arg, name)`.
pub type PucIntrEstablishFn =
    fn(&PucAttachArgs, i32, PciIntrFn, *mut c_void, &'static str) -> Option<NonNull<c_void>>;

/// `struct puc_attach_args`: what `puc(4)` hands to the driver of a port.
pub struct PucAttachArgs {
    /// `port`: the port's index in the card's description.
    pub port: i32,
    /// `type`: its `PUC_PORT_*`.
    pub type_: i32,
    /// `puc`: the card's softc (`struct puc_pci_softc` for a PCI card).
    pub puc: *const c_void,
    /// `a`: the BAR's bus address.
    pub a: BusAddr,
    /// `t`: the port's bus space tag.
    pub t: Option<BusSpaceTag>,
    /// `h`: the port's registers.
    pub h: Option<BusSpaceHandle>,
    /// `intr_string`.
    pub intr_string: PucIntrStringFn,
    /// `intr_establish`.
    pub intr_establish: PucIntrEstablishFn,
}

/// `PUC_NBARS`.
pub const PUC_NBARS: usize = 6;

/// One entry of `puc_softc.sc_bar_mappings[]`: the card's BAR `i`.
pub struct PucBarMapping {
    /// `mapped`.
    pub mapped: Cell<bool>,
    /// `type`.
    pub type_: Cell<usize>,
    /// `a`: the BAR's bus address.
    pub a: Cell<BusAddr>,
    /// `s`: its size.
    pub s: Cell<BusSize>,
    /// `t`.
    pub t: Cell<Option<BusSpaceTag>>,
    /// `h`.
    pub h: Cell<Option<BusSpaceHandle>>,
}

/// One entry of `puc_softc.sc_ports[]`: the card's port `i`.
pub struct PucPortState {
    /// `dev`: the port's device, once attached.
    pub dev: Cell<Option<NonNull<Device>>>,
    /// `intrhand`: filled in by the port attachment.
    pub intrhand: Cell<*mut c_void>,
    /// `real_intrhand`: the port driver's handler, behind a shared interrupt.
    pub real_intrhand: Cell<Option<PciIntrFn>>,
    /// `real_intrhand_arg`.
    pub real_intrhand_arg: Cell<*mut c_void>,
}

/// `struct puc_softc`.
#[repr(C)]
pub struct PucSoftc {
    /// `sc_dev`.
    pub sc_dev: Device,
    /// `sc_desc`: static configuration data.
    pub sc_desc: Cell<Option<&'static PucDeviceDescription>>,
    /// `sc_bar_mappings`: card-global dynamic data.
    pub sc_bar_mappings: [PucBarMapping; PUC_NBARS],
    /// `sc_ports`: per-port dynamic data.
    pub sc_ports: [PucPortState; PUC_MAX_PORTS],
    /// `sc_xr17v35x`.
    pub sc_xr17v35x: Cell<bool>,
}

// SAFETY: `#[repr(C)]` with the device first; every other member is a `Cell` of an integer, a
// bool, a raw pointer, or an `Option` of a reference, handle, tag, non-null pointer or function
// pointer, all valid as zero bits (the `Option`s are `None`).
unsafe impl Softc for PucSoftc {}
/* </CODE> */

/* <TESTS> */
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_description_keeps_the_low_16_bits_and_pads_the_ports() {
        let d = PucDeviceDescription::new(
            [0x1b36, 0x0002, 0, 0],
            [0xffff, 0xffff, 0, 0],
            &[PucPort::new(PUC_PORT_COM, 0x10, 0x0008)],
        );
        assert_eq!(d.rval, [0x1b36, 0x0002, 0, 0]);
        assert_eq!(d.ports[0].type_, PUC_PORT_COM as u8);
        assert_eq!((d.ports[0].bar, d.ports[0].offset), (0x10, 8));
        assert!(d.ports[1..].iter().all(|p| p.type_ == 0));
    }

    #[test]
    fn port_kinds_and_clocks() {
        assert!(puc_is_com(PUC_PORT_COM_MUL8) && !puc_is_lpt(PUC_PORT_COM_MUL8));
        assert!(puc_is_lpt(PUC_PORT_LPT) && !puc_is_com(PUC_PORT_LPT));
        assert_eq!(puc_port_bar_index(0x10), 0);
        assert_eq!(puc_port_bar_index(0x24), 5);
        assert_eq!(PUC_PORT_TYPES[4].freq, 18_432_000);
        assert_eq!(PUC_PORT_TYPES[6].freq, 125_000_000);
        for (i, t) in PUC_PORT_TYPES.iter().enumerate() {
            assert_eq!(t.type_, i as i32 + 1);
        }
    }
}
/* </TESTS> */
