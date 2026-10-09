/*	$OpenBSD: ohcivar.h,v 1.39 2017/06/01 09:47:55 mpi Exp $ */
/*	$NetBSD: ohcivar.h,v 1.32 2003/02/22 05:24:17 tsutsui Exp $	*/
/*	$FreeBSD: src/sys/dev/usb/ohcivar.h,v 1.13 1999/11/17 22:33:41 n_hibma Exp $	*/
/* <LICENSES> */
/*
 * Copyright (c) 1998 The NetBSD Foundation, Inc.
 * All rights reserved.
 *
 * This code is derived from software contributed to The NetBSD Foundation
 * by Lennart Augustsson (lennart@augustsson.net) at
 * Carlstedt Research & Technology.
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
//! The OHCI driver's structures: `<dev/usb/ohcivar.h>`, the softc the bus front-ends
//! (`ohci_pci`) embed and the software descriptors that wrap the controller's endpoint
//! descriptors, general and isochronous transfer descriptors.
//!
//! Upstream: sys/dev/usb/ohcivar.h @ 3ce1f3f79392
//!
//! A software descriptor (`struct ohci_soft_ed`, `ohci_soft_td`, `ohci_soft_itd`) starts with
//! the hardware descriptor and keeps the driver's links and bookkeeping after it, in the same
//! piece of DMA memory: `ohci_alloc_sed` & co. carve a `usb_allocmem` chunk into
//! `OHCI_*_CHUNK` of them. The chunks are never given back (as in C), so they are handed
//! around as `&'static`; the hardware part is made of `Cell`s accessed volatile
//! (`ohcireg.rs`), the software part of `Cell`s.
//!
//! The softc's members change under the kernel lock, as in C: the front-end establishes the
//! interrupt without `IPL_MPSAFE`, so `ohci_intr` runs with the kernel lock too, at
//! `splhardusb()` against the soft interrupt's `splusb()` sections; the tag and handle are
//! written before the interrupt is established.
//!
//! ## Deviations
//! - `OHCI_SED_SIZE`, `OHCI_STD_SIZE` and `OHCI_SITD_SIZE` round the Rust structures' sizes
//!   (the C's `sizeof`), so a chunk's byte size differs from a C kernel's; each software
//!   descriptor still starts on an `OHCI_*_ALIGN` boundary of one physically contiguous
//!   chunk.
//! - `struct ohci_soft_itd`'s `isdone` exists under feature `diagnostic`, as under
//!   `DIAGNOSTIC`.
//! - `OREAD4(sc, r)`, `OWRITE4(sc, r, x)` and the 1- and 2-byte variants (with their
//!   `OBARR(sc)` barrier), which `ohci.c` defines, are functions here, beside the softc they
//!   take ([`oread4`], [`owrite4`], ...).
//! - `ohci_checkrev`, `ohci_handover`, `ohci_init`, `ohci_intr`, `ohci_detach` and
//!   `ohci_activate`, declared here, are defined in `ohci.rs` with the file they come from.

use core::cell::Cell;

use crate::dev::usb::ohcireg::{
    OHCI_ED_ALIGN, OHCI_ITD_ALIGN, OHCI_NO_INTRS, OHCI_TD_ALIGN, OhciEd, OhciHcca, OhciItd,
    OhciPhysaddrT, OhciTd,
};
use crate::dev::usb::usbdivar::{UsbDma, UsbdBus, UsbdHcXfer, UsbdXfer};
use crate::kern::subr_prf::panic;
use crate::machine::bus::{
    BUS_SPACE_BARRIER_READ, BUS_SPACE_BARRIER_WRITE, BusSize, BusSpaceHandle, BusSpaceTag,
    bus_space_barrier, bus_space_read_1, bus_space_read_2, bus_space_read_4, bus_space_write_1,
    bus_space_write_2, bus_space_write_4,
};
use crate::queue_adapter;
use crate::sys::queue::{ListEntry, ListHead};
use crate::sys::time::Timeval;
use crate::sys::timeout::Timeout;

/// `struct ohci_soft_ed`: an endpoint descriptor and the driver's link.
#[repr(C)]
pub struct OhciSoftEd {
    /// `ed`: the hardware descriptor, first.
    pub ed: OhciEd,
    /// `next`.
    pub next: Cell<Option<&'static OhciSoftEd>>,
    /// `physaddr`: the bus address of `ed`.
    pub physaddr: Cell<OhciPhysaddrT>,
}

/// `OHCI_SED_SIZE`.
pub const OHCI_SED_SIZE: usize = size_of::<OhciSoftEd>().div_ceil(OHCI_ED_ALIGN) * OHCI_ED_ALIGN;
/// `OHCI_SED_CHUNK`.
pub const OHCI_SED_CHUNK: usize = 128;

/// `struct ohci_soft_td`: a general transfer descriptor and the driver's links.
#[repr(C)]
pub struct OhciSoftTd {
    /// `td`: the hardware descriptor, first.
    pub td: OhciTd,
    /// `nexttd`: mirrors nexttd in TD.
    pub nexttd: Cell<Option<&'static OhciSoftTd>>,
    /// `dnext`: next in done list.
    pub dnext: Cell<Option<&'static OhciSoftTd>>,
    /// `physaddr`: the bus address of `td`.
    pub physaddr: Cell<OhciPhysaddrT>,
    /// `hnext`: on `ohci_softc.sc_hash_tds`.
    pub hnext: ListEntry<OhciSoftTd>,
    /// `xfer`.
    pub xfer: Cell<Option<&'static UsbdXfer>>,
    /// `len`.
    pub len: Cell<u16>,
    /// `flags`: `OHCI_CALL_DONE`, `OHCI_ADD_LEN`.
    pub flags: Cell<u16>,
}

/// `OHCI_CALL_DONE`.
pub const OHCI_CALL_DONE: u16 = 0x0001;
/// `OHCI_ADD_LEN`.
pub const OHCI_ADD_LEN: u16 = 0x0002;

queue_adapter!(
    /// `LIST_ENTRY(ohci_soft_td) hnext`: `ohci_softc.sc_hash_tds`.
    pub OhciSoftTdHash: OhciSoftTd, hnext => ListEntry<OhciSoftTd>
);

/// `OHCI_STD_SIZE`.
pub const OHCI_STD_SIZE: usize = size_of::<OhciSoftTd>().div_ceil(OHCI_TD_ALIGN) * OHCI_TD_ALIGN;
/// `OHCI_STD_CHUNK`.
pub const OHCI_STD_CHUNK: usize = 128;

/// `struct ohci_soft_itd`: an isochronous transfer descriptor and the driver's links.
#[repr(C)]
pub struct OhciSoftItd {
    /// `itd`: the hardware descriptor, first.
    pub itd: OhciItd,
    /// `nextitd`: mirrors nexttd in ITD.
    pub nextitd: Cell<Option<&'static OhciSoftItd>>,
    /// `dnext`: next in done list.
    pub dnext: Cell<Option<&'static OhciSoftItd>>,
    /// `physaddr`: the bus address of `itd`.
    pub physaddr: Cell<OhciPhysaddrT>,
    /// `hnext`: on `ohci_softc.sc_hash_itds`.
    pub hnext: ListEntry<OhciSoftItd>,
    /// `xfer`.
    pub xfer: Cell<Option<&'static UsbdXfer>>,
    /// `flags`: `OHCI_CALL_DONE`.
    pub flags: Cell<u16>,
    /// `isdone` (`DIAGNOSTIC`).
    #[cfg(feature = "diagnostic")]
    pub isdone: Cell<i8>,
}

queue_adapter!(
    /// `LIST_ENTRY(ohci_soft_itd) hnext`: `ohci_softc.sc_hash_itds`.
    pub OhciSoftItdHash: OhciSoftItd, hnext => ListEntry<OhciSoftItd>
);

/// `OHCI_SITD_SIZE`.
pub const OHCI_SITD_SIZE: usize =
    size_of::<OhciSoftItd>().div_ceil(OHCI_ITD_ALIGN) * OHCI_ITD_ALIGN;
/// `OHCI_SITD_CHUNK`.
pub const OHCI_SITD_CHUNK: usize = 64;

/// `OHCI_NO_EDS`.
pub const OHCI_NO_EDS: usize = 2 * OHCI_NO_INTRS - 1;

/// `OHCI_HASH_SIZE`.
pub const OHCI_HASH_SIZE: usize = 128;

/// `struct ohci_softc`: what a bus front-end's softc starts with.
#[repr(C)]
pub struct OhciSoftc {
    /// `sc_bus`: base device, first.
    pub sc_bus: UsbdBus,
    /// `iot`.
    pub iot: Cell<Option<BusSpaceTag>>,
    /// `ioh`.
    pub ioh: Cell<Option<BusSpaceHandle>>,
    /// `sc_size`.
    pub sc_size: Cell<BusSize>,

    /// `sc_hccadma`: the HCCA's DMA memory.
    pub sc_hccadma: UsbDma,
    /// `sc_hcca`: the HCCA, in `sc_hccadma` (NULL until `ohci_init` allocated it).
    pub sc_hcca: Cell<Option<&'static OhciHcca>>,
    /// `sc_eds`: the dummy EDs of the interrupt tree.
    pub sc_eds: [Cell<Option<&'static OhciSoftEd>>; OHCI_NO_EDS],
    /// `sc_bws`: the interrupt EDs hung in each of the 32 slots.
    pub sc_bws: [Cell<u32>; OHCI_NO_INTRS],

    /// `sc_eintrs`: the enabled interrupts.
    pub sc_eintrs: Cell<u32>,
    /// `sc_isoc_head`: the dummy ED that heads the isochronous list.
    pub sc_isoc_head: Cell<Option<&'static OhciSoftEd>>,
    /// `sc_ctrl_head`: the dummy ED that heads the control list.
    pub sc_ctrl_head: Cell<Option<&'static OhciSoftEd>>,
    /// `sc_bulk_head`: the dummy ED that heads the bulk list.
    pub sc_bulk_head: Cell<Option<&'static OhciSoftEd>>,

    /// `sc_hash_tds`: the TDs by bus address.
    pub sc_hash_tds: [ListHead<OhciSoftTdHash>; OHCI_HASH_SIZE],
    /// `sc_hash_itds`: the isochronous TDs by bus address.
    pub sc_hash_itds: [ListHead<OhciSoftItdHash>; OHCI_HASH_SIZE],

    /// `sc_noport`.
    pub sc_noport: Cell<i32>,
    /// `sc_conf`: device configuration.
    pub sc_conf: Cell<u8>,

    /// `sc_softwake`.
    pub sc_softwake: Cell<i8>,

    /// `sc_freeeds`.
    pub sc_freeeds: Cell<Option<&'static OhciSoftEd>>,
    /// `sc_freetds`.
    pub sc_freetds: Cell<Option<&'static OhciSoftTd>>,
    /// `sc_freeitds`.
    pub sc_freeitds: Cell<Option<&'static OhciSoftItd>>,

    /// `sc_intrxfer`: the root hub's interrupt xfer.
    pub sc_intrxfer: Cell<Option<&'static UsbdXfer>>,

    /// `sc_sidone`: isochronous TDs the controller retired, for the soft interrupt.
    pub sc_sidone: Cell<Option<&'static OhciSoftItd>>,
    /// `sc_sdone`: TDs the controller retired, for the soft interrupt.
    pub sc_sdone: Cell<Option<&'static OhciSoftTd>>,

    /// `sc_vendor`: vendor string for root hub.
    pub sc_vendor: Cell<[u8; 16]>,
    /// `sc_id_vendor`: vendor ID for root hub.
    pub sc_id_vendor: Cell<i32>,

    /// `sc_control`: Preserved during suspend/standby.
    pub sc_control: Cell<u32>,
    /// `sc_intre`.
    pub sc_intre: Cell<u32>,
    /// `sc_ival`.
    pub sc_ival: Cell<u32>,

    /// `sc_overrun_cnt`.
    pub sc_overrun_cnt: Cell<u32>,
    /// `sc_overrun_ntc`.
    pub sc_overrun_ntc: Cell<Timeval>,

    /// `sc_tmo_rhsc`.
    pub sc_tmo_rhsc: Timeout,
}

impl OhciSoftc {
    /// `sc->iot`, `sc->ioh`. Panics before the front-end mapped the registers.
    pub fn regs(&self) -> (BusSpaceTag, BusSpaceHandle) {
        match (self.iot.get(), self.ioh.get()) {
            (Some(t), Some(h)) => (t, h),
            _ => panic(format_args!(
                "{}: ohci registers not mapped",
                self.sc_bus.bdev.xname()
            )),
        }
    }

    /// `sc->sc_hcca`. Panics before `ohci_init` allocated it.
    pub fn hcca(&self) -> &'static OhciHcca {
        match self.sc_hcca.get() {
            Some(h) => h,
            None => panic(format_args!(
                "{}: ohci without an HCCA",
                self.sc_bus.bdev.xname()
            )),
        }
    }

    /// `sc->sc_ctrl_head`. Panics before `ohci_init` allocated it.
    pub fn ctrl_head(&self) -> &'static OhciSoftEd {
        head(self, self.sc_ctrl_head.get())
    }

    /// `sc->sc_bulk_head`. Panics before `ohci_init` allocated it.
    pub fn bulk_head(&self) -> &'static OhciSoftEd {
        head(self, self.sc_bulk_head.get())
    }

    /// `sc->sc_isoc_head`. Panics before `ohci_init` allocated it.
    pub fn isoc_head(&self) -> &'static OhciSoftEd {
        head(self, self.sc_isoc_head.get())
    }

    /// `sc->sc_eds[i]`. Panics before `ohci_init` allocated it.
    pub fn ed(&self, i: usize) -> &'static OhciSoftEd {
        head(self, self.sc_eds[i].get())
    }
}

/// A list head ED of `ohci_init`'s, or a panic (the C's NULL dereference).
fn head(sc: &OhciSoftc, sed: Option<&'static OhciSoftEd>) -> &'static OhciSoftEd {
    match sed {
        Some(s) => s,
        None => panic(format_args!(
            "{}: ohci schedule not set up",
            sc.sc_bus.bdev.xname()
        )),
    }
}

// SAFETY: `#[repr(C)]` with the `usbd_bus` (itself headed by the device) first; the other
// members are `Cell`s of integers, arrays of bytes, `Option`s of references and tags, a
// `Timeval`, a `usb_dma` of `Cell`s, list heads (NULL is empty) and a zero-filled `Timeout`
// (set by `timeout_set`): all valid as zero bits.
unsafe impl crate::sys::device::Softc for OhciSoftc {}

/// `struct ohci_xfer`: an xfer of this controller.
#[repr(C)]
pub struct OhciXfer {
    /// `xfer`: the generic xfer, first.
    pub xfer: UsbdXfer,
}

// SAFETY: `#[repr(C)]` with the `usbd_xfer` first and nothing else.
unsafe impl UsbdHcXfer for OhciXfer {}

/// `OBARR(sc)`: a read and write barrier over the whole register window.
fn obarr(sc: &OhciSoftc) {
    let (t, h) = sc.regs();
    bus_space_barrier(
        t,
        h,
        0,
        sc.sc_size.get(),
        BUS_SPACE_BARRIER_READ | BUS_SPACE_BARRIER_WRITE,
    );
}

/// `OWRITE1(sc, r, x)`.
pub fn owrite1(sc: &OhciSoftc, r: BusSize, x: u8) {
    obarr(sc);
    let (t, h) = sc.regs();
    bus_space_write_1(t, h, r, x)
}

/// `OWRITE2(sc, r, x)`.
pub fn owrite2(sc: &OhciSoftc, r: BusSize, x: u16) {
    obarr(sc);
    let (t, h) = sc.regs();
    bus_space_write_2(t, h, r, x)
}

/// `OWRITE4(sc, r, x)`.
pub fn owrite4(sc: &OhciSoftc, r: BusSize, x: u32) {
    obarr(sc);
    let (t, h) = sc.regs();
    bus_space_write_4(t, h, r, x)
}

/// `OREAD1(sc, r)`.
pub fn oread1(sc: &OhciSoftc, r: BusSize) -> u8 {
    obarr(sc);
    let (t, h) = sc.regs();
    bus_space_read_1(t, h, r)
}

/// `OREAD2(sc, r)`.
pub fn oread2(sc: &OhciSoftc, r: BusSize) -> u16 {
    obarr(sc);
    let (t, h) = sc.regs();
    bus_space_read_2(t, h, r)
}

/// `OREAD4(sc, r)`.
pub fn oread4(sc: &OhciSoftc, r: BusSize) -> u32 {
    obarr(sc);
    let (t, h) = sc.regs();
    bus_space_read_4(t, h, r)
}

const _: () = {
    // The hardware descriptor heads each software one, every chunk slot is aligned for the
    // controller, and the C's chunk counts are kept.
    assert!(align_of::<OhciSoftEd>() <= OHCI_ED_ALIGN);
    assert!(align_of::<OhciSoftTd>() <= OHCI_TD_ALIGN);
    assert!(align_of::<OhciSoftItd>() <= OHCI_ITD_ALIGN);
    assert!(OHCI_SED_SIZE.is_multiple_of(OHCI_ED_ALIGN));
    assert!(OHCI_STD_SIZE.is_multiple_of(OHCI_TD_ALIGN));
    assert!(OHCI_SITD_SIZE.is_multiple_of(OHCI_ITD_ALIGN));
    assert!(OHCI_NO_EDS == 63);
};
/* </CODE> */

/* <TESTS> */
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn soft_descriptor_sizes() {
        assert_eq!(OHCI_SED_SIZE, 32);
        assert_eq!(OHCI_STD_SIZE % 16, 0);
        assert!(OHCI_STD_SIZE >= size_of::<OhciSoftTd>());
        assert_eq!(OHCI_SITD_SIZE % 32, 0);
        assert!(OHCI_SITD_SIZE >= size_of::<OhciSoftItd>());
        assert_eq!(core::mem::offset_of!(OhciSoftEd, ed), 0);
        assert_eq!(core::mem::offset_of!(OhciSoftTd, td), 0);
        assert_eq!(core::mem::offset_of!(OhciSoftItd, itd), 0);
    }
}
/* </TESTS> */
