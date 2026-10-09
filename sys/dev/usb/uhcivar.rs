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
//! The UHCI driver's structures: `<dev/usb/uhcivar.h>`, the softc the bus front-end
//! (`uhci_pci`) embeds and the software descriptors that wrap the controller's TDs and QHs.
//!
//! Upstream: sys/dev/usb/uhcivar.h @ 3ce1f3f79392
//!
//! To avoid having 1024 TDs for each isochronous transfer the driver keeps a virtual frame
//! list. Every `UHCI_VFRAMELIST_COUNT` entries in the real frame list point to the same
//! non-active TD; these TDs start the virtual frame list. This also simplifies linking TDs
//! and QHs in and out of the schedule. Each of those inactive TDs points at an inactive QH
//! that starts the interrupt traffic of its slot; every one of these QHs points at the QH
//! that starts the low speed control traffic, which leads to the full speed control QH, then
//! to the bulk QH and the last QH. `UHCI_VFRAMELIST_COUNT` is a power of 2 and at most
//! `UHCI_FRAMELIST_COUNT`.
//!
//! A software descriptor (`struct uhci_soft_td`, `uhci_soft_qh`) starts with the hardware
//! descriptor and keeps the driver's links after it, in the same piece of DMA memory:
//! `uhci_alloc_std` and `uhci_alloc_sqh` carve `UHCI_STD_CHUNK` (`UHCI_SQH_CHUNK`) of them
//! out of one `usb_allocmem` allocation. The chunks are never given back (as in C), so they are
//! handed around as `&'static`; the hardware part is made of `Cell<u32>`s accessed volatile
//! (`uhcireg.rs`), the software part of `Cell`s changed at `splusb()` under the kernel lock.
//!
//! The softc's members change under the kernel lock, as in C: the hard interrupt
//! (`uhci_intr`) is established at `IPL_USB` without `IPL_MPSAFE`, so it holds the kernel lock
//! as well.
//!
//! ## Deviations
//! - `uhci_soft_td_qh_t`, the union of a QH and a TD pointer that a soft TD's `link`
//!   holds, is [`UhciSoftTdQh`], an enum that remembers which arm was written, in a
//!   `Cell<Option<..>>` (`None` is the C's NULL). Reading the `std` arm of a link that holds
//!   a QH ([`UhciSoftTd::link_std`]) gives `None`: the walks that read it stop there, where
//!   the C would reinterpret the QH as a TD (only in `uhci_device_isoc_close`'s "not found"
//!   path, which prints and gives up anyway). Copying the whole union (`vstd->link =
//!   std->link`) copies the enum.
//! - `UHCI_STD_SIZE` and `UHCI_SQH_SIZE` round the Rust structures' sizes (the C's
//!   `sizeof`); a soft TD is 48 bytes here (32 in a 64-bit C kernel), so a chunk of
//!   `UHCI_STD_CHUNK` (128) TDs takes 6 KB, not one page. Each software descriptor still
//!   starts on a 16-byte boundary.
//! - `sc_suspend` holds the `DVACT_*` value the C stores in a `char`, as `i8`.
//! - `sc_pframes` is reached through a bounds-checked accessor ([`UhciSoftc::set_pframe`]).
//! - `uhci_init`, `uhci_run`, `uhci_intr`, `uhci_detach` and `uhci_activate`, declared here,
//!   are defined in `uhci.rs` with the file they come from.

use core::cell::Cell;
use core::ptr;

use crate::dev::usb::uhcireg::{
    UHCI_FRAMELIST_COUNT, UHCI_QH_ALIGN, UHCI_TD_ALIGN, UhciPhysaddrT, UhciQh, UhciTd,
};
use crate::dev::usb::usbdivar::{UsbDma, UsbdBus, UsbdHcXfer, UsbdXfer};
use crate::kern::subr_prf::panic;
use crate::machine::bus::{BusSize, BusSpaceHandle, BusSpaceTag};
use crate::queue_adapter;
use crate::sys::queue::{ListEntry, ListHead};
use crate::sys::timeout::Timeout;

/// `UHCI_VFRAMELIST_COUNT`.
pub const UHCI_VFRAMELIST_COUNT: usize = 128;

/// `uhci_soft_td_qh_t`: what a soft TD's link points at (see the module's deviations).
#[derive(Clone, Copy)]
pub enum UhciSoftTdQh {
    /// `sqh`.
    Sqh(&'static UhciSoftQh),
    /// `std`.
    Std(&'static UhciSoftTd),
}

/// `struct uhci_xfer`: an xfer of this controller. An interrupt info struct contains the
/// information needed to execute a requested routine when the controller generates an
/// interrupt. Since we cannot know which transfer generated the interrupt all structs are
/// linked together so they can be searched at interrupt time.
#[repr(C)]
pub struct UhciXfer {
    /// `xfer`: the generic xfer, first.
    pub xfer: UsbdXfer,
    /// `inext`: the list of active xfers (`le_prev` is NULL while off it).
    pub inext: ListEntry<UhciXfer>,
    /// `stdstart`.
    pub stdstart: Cell<Option<&'static UhciSoftTd>>,
    /// `stdend`.
    pub stdend: Cell<Option<&'static UhciSoftTd>>,
    /// `curframe`.
    pub curframe: Cell<i32>,
    /// `isdone` (`DIAGNOSTIC`).
    #[cfg(feature = "diagnostic")]
    pub isdone: Cell<i32>,
}

// SAFETY: `#[repr(C)]` with the `usbd_xfer` first; the other members are a list entry,
// `Cell`s of `Option`s of references and integers: all valid as zero bits
// (`pool_get(..., PR_ZERO)`).
unsafe impl UsbdHcXfer for UhciXfer {}

queue_adapter!(
    /// `LIST_ENTRY(uhci_xfer) inext`: `uhci_softc.sc_intrhead`.
    pub UhciXferIntrList: UhciXfer, inext => ListEntry<UhciXfer>
);

/// `struct uhci_soft_td`: extra information that we need for a TD.
#[repr(C)]
pub struct UhciSoftTd {
    /// `td`: the real TD, must be first.
    pub td: UhciTd,
    /// `link`: soft version of the `td_link` field.
    pub link: Cell<Option<UhciSoftTdQh>>,
    /// `physaddr`: TD's physical address.
    pub physaddr: Cell<UhciPhysaddrT>,
}

impl UhciSoftTd {
    /// A cleared soft TD, for a fresh chunk.
    pub const fn new() -> Self {
        Self {
            td: UhciTd {
                td_link: Cell::new(0),
                td_status: Cell::new(0),
                td_token: Cell::new(0),
                td_buffer: Cell::new(0),
            },
            link: Cell::new(None),
            physaddr: Cell::new(0),
        }
    }

    /// `std->link.std`: the next TD (`None` for NULL, or for a link that holds a QH).
    pub fn link_std(&self) -> Option<&'static UhciSoftTd> {
        match self.link.get() {
            Some(UhciSoftTdQh::Std(t)) => Some(t),
            _ => None,
        }
    }

    /// `std->link.std = t`.
    pub fn set_link_std(&self, t: Option<&'static UhciSoftTd>) {
        self.link.set(t.map(UhciSoftTdQh::Std));
    }
}

impl Default for UhciSoftTd {
    fn default() -> Self {
        Self::new()
    }
}

/// `UHCI_STD_SIZE`: the size of a soft TD, a multiple of `UHCI_TD_ALIGN`, so that a number
/// of soft TDs can be packed together with each real TD well aligned.
pub const UHCI_STD_SIZE: usize = size_of::<UhciSoftTd>().div_ceil(UHCI_TD_ALIGN) * UHCI_TD_ALIGN;
/// `UHCI_STD_CHUNK`.
pub const UHCI_STD_CHUNK: usize = 128;

/// `struct uhci_soft_qh`: extra information that we need for a QH.
#[repr(C)]
pub struct UhciSoftQh {
    /// `qh`: the real QH, must be first.
    pub qh: UhciQh,
    /// `hlink`: soft version of `qh_hlink`.
    pub hlink: Cell<Option<&'static UhciSoftQh>>,
    /// `elink`: soft version of `qh_elink`.
    pub elink: Cell<Option<&'static UhciSoftTd>>,
    /// `physaddr`: QH's physical address.
    pub physaddr: Cell<UhciPhysaddrT>,
    /// `pos`: timeslot position.
    pub pos: Cell<i32>,
}

impl UhciSoftQh {
    /// A cleared soft QH, for a fresh chunk.
    pub const fn new() -> Self {
        Self {
            qh: UhciQh {
                qh_hlink: Cell::new(0),
                qh_elink: Cell::new(0),
            },
            hlink: Cell::new(None),
            elink: Cell::new(None),
            physaddr: Cell::new(0),
            pos: Cell::new(0),
        }
    }
}

impl Default for UhciSoftQh {
    fn default() -> Self {
        Self::new()
    }
}

/// `UHCI_SQH_SIZE`: see [`UHCI_STD_SIZE`].
pub const UHCI_SQH_SIZE: usize = size_of::<UhciSoftQh>().div_ceil(UHCI_QH_ALIGN) * UHCI_QH_ALIGN;
/// `UHCI_SQH_CHUNK`.
pub const UHCI_SQH_CHUNK: usize = 128;

/// `struct uhci_vframe`: information about an entry in the virtual frame list.
pub struct UhciVframe {
    /// `htd`: pointer to dummy TD.
    pub htd: Cell<Option<&'static UhciSoftTd>>,
    /// `etd`: pointer to last TD.
    pub etd: Cell<Option<&'static UhciSoftTd>>,
    /// `hqh`: pointer to dummy QH.
    pub hqh: Cell<Option<&'static UhciSoftQh>>,
    /// `eqh`: pointer to last QH.
    pub eqh: Cell<Option<&'static UhciSoftQh>>,
    /// `bandwidth`: max bandwidth used by this frame.
    pub bandwidth: Cell<u32>,
}

impl UhciVframe {
    /// `vf->htd`. Panics before `uhci_init` allocated it.
    pub fn htd(&self) -> &'static UhciSoftTd {
        match self.htd.get() {
            Some(t) => t,
            None => panic(format_args!("uhci: virtual frame without a TD")),
        }
    }

    /// `vf->hqh`. Panics before `uhci_init` allocated it.
    pub fn hqh(&self) -> &'static UhciSoftQh {
        match self.hqh.get() {
            Some(q) => q,
            None => panic(format_args!("uhci: virtual frame without a QH")),
        }
    }

    /// `vf->eqh`. Panics before `uhci_init` allocated it.
    pub fn eqh(&self) -> &'static UhciSoftQh {
        match self.eqh.get() {
            Some(q) => q,
            None => panic(format_args!("uhci: virtual frame without a last QH")),
        }
    }
}

/// `struct uhci_softc`: what the bus front-end's softc starts with.
#[repr(C)]
pub struct UhciSoftc {
    /// `sc_bus`: base device, first.
    pub sc_bus: UsbdBus,
    /// `iot`.
    pub iot: Cell<Option<BusSpaceTag>>,
    /// `ioh`.
    pub ioh: Cell<Option<BusSpaceHandle>>,
    /// `sc_size`.
    pub sc_size: Cell<BusSize>,

    /// `sc_pframes`: the frame list, `UHCI_FRAMELIST_COUNT` links in `sc_dma`.
    pub sc_pframes: Cell<*mut UhciPhysaddrT>,
    /// `sc_dma`.
    pub sc_dma: UsbDma,
    /// `sc_vframes`.
    pub sc_vframes: [UhciVframe; UHCI_VFRAMELIST_COUNT],

    /// `sc_lctl_start`: dummy QH for low speed control.
    pub sc_lctl_start: Cell<Option<&'static UhciSoftQh>>,
    /// `sc_lctl_end`: last control QH.
    pub sc_lctl_end: Cell<Option<&'static UhciSoftQh>>,
    /// `sc_hctl_start`: dummy QH for high speed control.
    pub sc_hctl_start: Cell<Option<&'static UhciSoftQh>>,
    /// `sc_hctl_end`: last control QH.
    pub sc_hctl_end: Cell<Option<&'static UhciSoftQh>>,
    /// `sc_bulk_start`: dummy QH for bulk.
    pub sc_bulk_start: Cell<Option<&'static UhciSoftQh>>,
    /// `sc_bulk_end`: last bulk transfer.
    pub sc_bulk_end: Cell<Option<&'static UhciSoftQh>>,
    /// `sc_last_qh`: dummy QH at the end.
    pub sc_last_qh: Cell<Option<&'static UhciSoftQh>>,
    /// `sc_loops`: number of QHs that wants looping.
    pub sc_loops: Cell<u32>,

    /// `sc_freetds`: TD free list.
    pub sc_freetds: Cell<Option<&'static UhciSoftTd>>,
    /// `sc_freeqhs`: QH free list.
    pub sc_freeqhs: Cell<Option<&'static UhciSoftQh>>,

    /// `sc_conf`: device configuration.
    pub sc_conf: Cell<u8>,

    /// `sc_saved_sof`.
    pub sc_saved_sof: Cell<u8>,
    /// `sc_saved_frnum`.
    pub sc_saved_frnum: Cell<u16>,

    /// `sc_softwake`.
    pub sc_softwake: Cell<i8>,

    /// `sc_isreset`.
    pub sc_isreset: Cell<i8>,
    /// `sc_suspend`: `DVACT_RESUME` while operating.
    pub sc_suspend: Cell<i8>,

    /// `sc_intrhead`: the active xfers.
    pub sc_intrhead: ListHead<UhciXferIntrList>,

    /// `sc_intrxfer`: info for the root hub interrupt "pipe".
    pub sc_intrxfer: Cell<Option<&'static UsbdXfer>>,
    /// `sc_root_intr`.
    pub sc_root_intr: Timeout,

    /// `sc_vendor`: vendor string for root hub.
    pub sc_vendor: Cell<[u8; 32]>,
    /// `sc_id_vendor`: vendor ID for root hub.
    pub sc_id_vendor: Cell<i32>,
}

impl UhciSoftc {
    /// `sc->iot`, `sc->ioh`. Panics before the front-end mapped the registers.
    pub fn regs(&self) -> (BusSpaceTag, BusSpaceHandle) {
        match (self.iot.get(), self.ioh.get()) {
            (Some(t), Some(h)) => (t, h),
            _ => panic(format_args!(
                "{}: uhci registers not mapped",
                self.sc_bus.bdev.xname()
            )),
        }
    }

    /// `sc->sc_pframes[j] = htole32(v)`, checked against `UHCI_FRAMELIST_COUNT`.
    pub fn set_pframe(&self, j: usize, v: UhciPhysaddrT) {
        let base = self.sc_pframes.get();
        if base.is_null() || j >= UHCI_FRAMELIST_COUNT {
            panic(format_args!("uhci: frame {j} outside the frame list"));
        }
        // SAFETY: `uhci_init` allocated `UHCI_FRAMELIST_COUNT` links in DMA memory, never
        // freed; the index is checked above; the controller reads the list, so the store is
        // volatile.
        unsafe { ptr::write_volatile(base.add(j), v.to_le()) }
    }
}

// SAFETY: `#[repr(C)]` with the `usbd_bus` (itself headed by the device) first; the other
// members are `Cell`s of integers, a raw pointer, `Option`s of references and tags, a
// `usb_dma` and virtual frames of such `Cell`s, a list head and a zero-filled `Timeout` (set
// by `timeout_set`): all valid as zero bits.
unsafe impl crate::sys::device::Softc for UhciSoftc {}

const _: () = {
    // The hardware descriptor heads each software one, every chunk slot is aligned for the
    // controller.
    assert!(align_of::<UhciSoftTd>() <= UHCI_TD_ALIGN);
    assert!(align_of::<UhciSoftQh>() <= UHCI_QH_ALIGN);
    assert!(
        UHCI_STD_SIZE.is_multiple_of(UHCI_TD_ALIGN) && UHCI_SQH_SIZE.is_multiple_of(UHCI_QH_ALIGN)
    );
    assert!(UHCI_VFRAMELIST_COUNT.is_power_of_two());
    assert!(UHCI_VFRAMELIST_COUNT <= UHCI_FRAMELIST_COUNT);
};
/* </CODE> */

/* <TESTS> */
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn soft_descriptor_sizes() {
        assert_eq!(UHCI_STD_SIZE % 16, 0);
        assert!(UHCI_STD_SIZE >= size_of::<UhciSoftTd>());
        assert_eq!(UHCI_SQH_SIZE % 16, 0);
        assert!(UHCI_SQH_SIZE >= size_of::<UhciSoftQh>());
        assert_eq!(core::mem::offset_of!(UhciSoftTd, td), 0);
        assert_eq!(core::mem::offset_of!(UhciSoftQh, qh), 0);
    }

    #[test]
    fn td_link_union() {
        let q: &'static UhciSoftQh = std::boxed::Box::leak(std::boxed::Box::default());
        let t: &'static UhciSoftTd = std::boxed::Box::leak(std::boxed::Box::default());
        let td = UhciSoftTd::new();
        assert!(td.link_std().is_none());
        td.set_link_std(Some(t));
        assert!(td.link_std().is_some_and(|x| ptr::eq(x, t)));
        td.link.set(Some(UhciSoftTdQh::Sqh(q)));
        assert!(td.link_std().is_none());
        td.set_link_std(None);
        assert!(td.link.get().is_none());
    }

    #[test]
    #[ignore = "needs OPENBSD_SRC (just test-ref)"]
    fn constants_match_the_c_header() {
        let defs = crate::reftest::defines("sys/dev/usb/uhcivar.h");
        let ours = crate::reftest::assert_defines!(defs;
            UHCI_VFRAMELIST_COUNT, UHCI_STD_CHUNK, UHCI_SQH_CHUNK,
        );
        // The sizes are `sizeof`s of the Rust structures (see the module's deviations).
        let ours = [ours.as_slice(), &["UHCI_STD_SIZE", "UHCI_SQH_SIZE"]].concat();
        crate::reftest::assert_complete(&defs, "UHCI_", &ours);
    }
}
/* </TESTS> */
