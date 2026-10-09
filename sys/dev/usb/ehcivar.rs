/* <LICENSES> */
/*
 * Copyright (c) 2001 The NetBSD Foundation, Inc.
 * All rights reserved.
 *
 * This code is derived from software contributed to The NetBSD Foundation
 * by Lennart Augustsson (lennart@augustsson.net).
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
//! The EHCI driver's structures: `<dev/usb/ehcivar.h>`, the softc the bus front-ends
//! (`ehci_pci`) embed and the software descriptors that wrap the controller's queue heads,
//! qTDs and isochronous descriptors.
//!
//! Upstream: sys/dev/usb/ehcivar.h @ 3ce1f3f79392
//!
//! A software descriptor (`struct ehci_soft_qh`, `ehci_soft_qtd`, `ehci_soft_itd`) starts
//! with the hardware descriptor and keeps the driver's links and bookkeeping after it, in the
//! same piece of DMA memory: `ehci_alloc_sqh` & co. carve a page of `usb_allocmem` memory
//! into `EHCI_*_CHUNK` of them. The chunks are never given back (as in C), so they are
//! handed around as `&'static`; the hardware part is made of `Cell<u32>`s accessed volatile
//! (`ehcireg.rs`), the software part of `Cell`s changed at `splusb()` under the kernel lock.
//!
//! The softc's members change under the kernel lock at `splusb()`, as in C, except those the
//! `IPL_MPSAFE` hard interrupt (`ehci_intr`) touches without it: `sc_offs`, `sc_flags` and
//! `sc_eintrs` are atomics (`docs/C_TO_RUST.md`, a field another CPU reads without the
//! writer's lock); the tag and handle are written before the interrupt is established.
//!
//! ## Deviations
//! - The unions are split: `ehci_soft_itd`'s `itd`/`sitd` is the `itd` storage with
//!   [`EhciSoftItd::sitd`] a view of its first 36 bytes; its `u.frame_list` and
//!   `u.free_list` are separate members, as are `ehci_xfer`'s `_TD.sqtd` and `_TD.itd`
//!   (`sqtdstart`/`sqtdend` and `itdstart`/`itdend`). Nothing in the driver reads one arm of
//!   a union after writing the other.
//! - `EHCI_SQTD_SIZE`, `EHCI_SQH_SIZE` and `EHCI_ITD_SIZE` round the Rust structures' sizes
//!   (the C's `sizeof`), so the chunk counts differ from a C kernel's; each software
//!   descriptor still starts on an `EHCI_*_ALIGN` boundary inside one page.
//! - `EREAD1(sc, a)`, `EOWRITE4(sc, a, x)` and the other accessor macros are functions of the
//!   same names in lower case taking the softc ([`eread1`], [`eowrite4`], ...).
//! - `sc_flist` and `sc_softitds` are reached through bounds-checked accessors
//!   ([`EhciSoftc::flist`], [`EhciSoftc::softitd`] and their setters).
//! - `ehci_init`, `ehci_intr`, `ehci_detach`, `ehci_activate` and `ehci_reset`, declared here,
//!   are defined in `ehci.rs` with the file they come from.

use core::cell::Cell;
use core::ptr;
use core::sync::atomic::{AtomicI32, AtomicU32, AtomicUsize, Ordering};

use crate::dev::usb::ehcireg::{
    EHCI_ITD_ALIGN, EHCI_PAGE_SIZE, EHCI_QH_ALIGN, EHCI_QTD_ALIGN, EhciItd, EhciLinkT,
    EhciPhysaddrT, EhciQh, EhciQtd, EhciSitd,
};
use crate::dev::usb::usbdivar::{UsbDma, UsbdBus, UsbdHcXfer, UsbdXfer};
use crate::kern::subr_prf::panic;
use crate::machine::bus::{
    BusSize, BusSpaceHandle, BusSpaceTag, bus_space_read_1, bus_space_read_2, bus_space_read_4,
    bus_space_write_1, bus_space_write_2, bus_space_write_4,
};
use crate::queue_adapter;
use crate::sys::queue::{ListEntry, ListHead, TailqEntry, TailqHead};
use crate::sys::rwlock::Rwlock;
use crate::sys::time::Timeval;
use crate::sys::timeout::Timeout;

/// `struct ehci_soft_qtd`: a qTD and the driver's links.
#[repr(C)]
pub struct EhciSoftQtd {
    /// `qtd`: the hardware descriptor, first.
    pub qtd: EhciQtd,
    /// `nextqtd`: mirrors nextqtd in TD.
    pub nextqtd: Cell<Option<&'static EhciSoftQtd>>,
    /// `physaddr`: the bus address of `qtd`.
    pub physaddr: Cell<EhciPhysaddrT>,
    /// `dma`: qTD's DMA infos (the chunk).
    pub dma: UsbDma,
    /// `offs`: qTD's offset in struct usb_dma.
    pub offs: Cell<i32>,
    /// `hnext`.
    pub hnext: ListEntry<EhciSoftQtd>,
    /// `len`.
    pub len: Cell<u16>,
}

/// `EHCI_SQTD_SIZE`.
pub const EHCI_SQTD_SIZE: usize =
    size_of::<EhciSoftQtd>().div_ceil(EHCI_QTD_ALIGN) * EHCI_QTD_ALIGN;
/// `EHCI_SQTD_CHUNK`.
pub const EHCI_SQTD_CHUNK: usize = EHCI_PAGE_SIZE as usize / EHCI_SQTD_SIZE;

/// `struct ehci_soft_qh`: a queue head and the driver's links.
#[repr(C)]
pub struct EhciSoftQh {
    /// `qh`: the hardware descriptor, first.
    pub qh: EhciQh,
    /// `next`.
    pub next: Cell<Option<&'static EhciSoftQh>>,
    /// `prev`.
    pub prev: Cell<Option<&'static EhciSoftQh>>,
    /// `sqtd`.
    pub sqtd: Cell<Option<&'static EhciSoftQtd>>,
    /// `physaddr`: the bus address of `qh`.
    pub physaddr: Cell<EhciPhysaddrT>,
    /// `dma`: QH's DMA infos (the chunk).
    pub dma: UsbDma,
    /// `offs`: QH's offset in struct usb_dma.
    pub offs: Cell<i32>,
    /// `islot`.
    pub islot: Cell<i32>,
}

/// `EHCI_SQH_SIZE`.
pub const EHCI_SQH_SIZE: usize = size_of::<EhciSoftQh>().div_ceil(EHCI_QH_ALIGN) * EHCI_QH_ALIGN;
/// `EHCI_SQH_CHUNK`.
pub const EHCI_SQH_CHUNK: usize = EHCI_PAGE_SIZE as usize / EHCI_SQH_SIZE;

/// `struct ehci_soft_itd`: an iTD or siTD and the driver's links.
#[repr(C)]
pub struct EhciSoftItd {
    /// `itd`: the hardware descriptor, first (also the storage of the union's `sitd`).
    pub itd: EhciItd,
    /// `u.frame_list.next`: soft_itds links in a periodic frame.
    pub frame_next: Cell<Option<&'static EhciSoftItd>>,
    /// `u.frame_list.prev`.
    pub frame_prev: Cell<Option<&'static EhciSoftItd>>,
    /// `u.free_list`: circular list of free itds.
    pub free_list: ListEntry<EhciSoftItd>,
    /// `xfer_next`: Next soft_itd in xfer.
    pub xfer_next: Cell<Option<&'static EhciSoftItd>>,
    /// `physaddr`: the bus address of `itd`.
    pub physaddr: Cell<EhciPhysaddrT>,
    /// `dma`: the chunk.
    pub dma: UsbDma,
    /// `offs`: the descriptor's offset in `dma`.
    pub offs: Cell<i32>,
    /// `slot`: the frame list slot.
    pub slot: Cell<i32>,
    /// `t`: store free time.
    pub t: Cell<Timeval>,
}

impl EhciSoftItd {
    /// `itd->sitd`: the union's other arm, the split transaction descriptor.
    pub fn sitd(&self) -> &EhciSitd {
        // SAFETY: `EhciSitd` and `EhciItd` are `#[repr(C)]` structures of `Cell<u32>` only
        // (alignment 4, any bits valid), and the siTD (36 bytes) fits in the iTD (92 bytes)
        // at the same address, which is what the C's union is.
        unsafe { &*ptr::from_ref(&self.itd).cast::<EhciSitd>() }
    }
}

queue_adapter!(
    /// `LIST_ENTRY(ehci_soft_itd) free_list`: `ehci_softc.sc_freeitds`.
    pub EhciSoftItdFreeList: EhciSoftItd, free_list => ListEntry<EhciSoftItd>
);

/// `EHCI_ITD_SIZE`.
pub const EHCI_ITD_SIZE: usize = size_of::<EhciSoftItd>().div_ceil(EHCI_ITD_ALIGN) * EHCI_ITD_ALIGN;
/// `EHCI_ITD_CHUNK`.
pub const EHCI_ITD_CHUNK: usize = EHCI_PAGE_SIZE as usize / EHCI_ITD_SIZE;

/// `struct ehci_xfer`: an xfer of this controller.
#[repr(C)]
pub struct EhciXfer {
    /// `xfer`: the generic xfer, first.
    pub xfer: UsbdXfer,
    /// `inext`: List of active xfers.
    pub inext: TailqEntry<EhciXfer>,
    /// `sqtdstart` (`_TD.sqtd.start`): Ctrl/Bulk/Interrupt TD.
    pub sqtdstart: Cell<Option<&'static EhciSoftQtd>>,
    /// `sqtdend` (`_TD.sqtd.end`).
    pub sqtdend: Cell<Option<&'static EhciSoftQtd>>,
    /// `itdstart` (`_TD.itd.start`): Isochronous TD.
    pub itdstart: Cell<Option<&'static EhciSoftItd>>,
    /// `itdend` (`_TD.itd.end`).
    pub itdend: Cell<Option<&'static EhciSoftItd>>,
    /// `ehci_xfer_flags`: `EHCI_XFER_*`.
    pub ehci_xfer_flags: Cell<u32>,
    /// `isdone` (`DIAGNOSTIC`).
    #[cfg(feature = "diagnostic")]
    pub isdone: Cell<i32>,
}

// SAFETY: `#[repr(C)]` with the `usbd_xfer` first; the other members are a queue entry,
// `Cell`s of `Option`s of references and integers: all valid as zero bits
// (`pool_get(..., PR_ZERO)`).
unsafe impl UsbdHcXfer for EhciXfer {}

queue_adapter!(
    /// `TAILQ_ENTRY(ehci_xfer) inext`: `ehci_softc.sc_intrhead`.
    pub EhciXferIntrList: EhciXfer, inext => TailqEntry<EhciXfer>
);

/// `EHCI_XFER_ABORTING`: xfer is aborting.
pub const EHCI_XFER_ABORTING: u32 = 0x0001;
/// `EHCI_XFER_ABORTWAIT`: abort completion is being awaited.
pub const EHCI_XFER_ABORTWAIT: u32 = 0x0002;

/// `struct ehci_soft_islot`: information about an entry in the interrupt list.
pub struct EhciSoftIslot {
    /// `sqh`: Queue Head.
    pub sqh: Cell<Option<&'static EhciSoftQh>>,
}

impl EhciSoftIslot {
    /// `isp->sqh`. Panics before `ehci_init` allocated it.
    pub fn sqh(&self) -> &'static EhciSoftQh {
        match self.sqh.get() {
            Some(q) => q,
            None => panic(format_args!("ehci: interrupt slot without a QH")),
        }
    }
}

/// `EHCI_FRAMELIST_MAXCOUNT`.
pub const EHCI_FRAMELIST_MAXCOUNT: u32 = 1024;
/// `EHCI_IPOLLRATES`: Poll rates (1ms, 2, 4, 8 .. 128).
pub const EHCI_IPOLLRATES: u32 = 8;
/// `EHCI_INTRQHS`.
pub const EHCI_INTRQHS: usize = (1 << EHCI_IPOLLRATES) - 1;

/// `EHCI_IQHIDX(lev, pos)`: the index of the interrupt QH at level `lev`, position `pos`.
pub const fn ehci_iqhidx(lev: u32, pos: u32) -> usize {
    (((pos & ((1 << lev) - 1)) | (1 << lev)) - 1) as usize
}

/// `EHCI_ILEV_IVAL(lev)`.
pub const fn ehci_ilev_ival(lev: u32) -> i32 {
    1 << lev
}

/// `EHCI_HASH_SIZE`.
pub const EHCI_HASH_SIZE: u32 = 128;
/// `EHCI_COMPANION_MAX`.
pub const EHCI_COMPANION_MAX: u32 = 8;

/// `EHCI_FREE_LIST_INTERVAL`.
pub const EHCI_FREE_LIST_INTERVAL: u32 = 100;

/// `struct ehci_softc`: what a bus front-end's softc starts with.
#[repr(C)]
pub struct EhciSoftc {
    /// `sc_bus`: base device, first.
    pub sc_bus: UsbdBus,
    /// `iot`.
    pub iot: Cell<Option<BusSpaceTag>>,
    /// `ioh`.
    pub ioh: Cell<Option<BusSpaceHandle>>,
    /// `sc_size`.
    pub sc_size: Cell<BusSize>,
    /// `sc_offs`: offset to operational regs (read by the `IPL_MPSAFE` interrupt).
    pub sc_offs: AtomicUsize,
    /// `sc_flags`: misc flags, `EHCIF_*` (set by the `IPL_MPSAFE` interrupt).
    pub sc_flags: AtomicI32,

    /// `sc_vendor`: vendor string for root hub.
    pub sc_vendor: Cell<[u8; 16]>,
    /// `sc_id_vendor`: vendor ID for root hub.
    pub sc_id_vendor: Cell<i32>,

    /// `sc_fldma`: the periodic frame list.
    pub sc_fldma: UsbDma,
    /// `sc_flist`: `sc_flsize` links, in `sc_fldma`.
    pub sc_flist: Cell<*mut EhciLinkT>,
    /// `sc_flsize`.
    pub sc_flsize: Cell<u32>,

    /// `sc_islots`: the interrupt QH tree.
    pub sc_islots: [EhciSoftIslot; EHCI_INTRQHS],

    /// `sc_softitds`: jcmm - an array matching sc_flist, but with software pointers, not
    /// hardware address pointers (`sc_flsize` entries).
    pub sc_softitds: Cell<*mut Cell<Option<&'static EhciSoftItd>>>,

    /// `sc_intrhead`: the active xfers.
    pub sc_intrhead: TailqHead<EhciXferIntrList>,

    /// `sc_freeqhs`.
    pub sc_freeqhs: Cell<Option<&'static EhciSoftQh>>,
    /// `sc_freeqtds`.
    pub sc_freeqtds: Cell<Option<&'static EhciSoftQtd>>,
    /// `sc_freeitds`.
    pub sc_freeitds: ListHead<EhciSoftItdFreeList>,

    /// `sc_noport`.
    pub sc_noport: Cell<i32>,
    /// `sc_conf`: device configuration.
    pub sc_conf: Cell<u8>,
    /// `sc_intrxfer`: the root hub's interrupt xfer.
    pub sc_intrxfer: Cell<Option<&'static UsbdXfer>>,
    /// `sc_isreset`.
    pub sc_isreset: Cell<i8>,
    /// `sc_softwake`.
    pub sc_softwake: Cell<i8>,

    /// `sc_eintrs`: the enabled interrupts (narrowed by the `IPL_MPSAFE` interrupt).
    pub sc_eintrs: AtomicU32,
    /// `sc_async_head`: the dummy QH that heads the async list (also the doorbell's wait
    /// channel's address).
    pub sc_async_head: Cell<Option<&'static EhciSoftQh>>,

    /// `sc_doorbell_lock`.
    pub sc_doorbell_lock: Rwlock,

    /// `sc_tmo_intrlist`.
    pub sc_tmo_intrlist: Timeout,
}

/// `EHCIF_DROPPED_INTR_WORKAROUND`.
pub const EHCIF_DROPPED_INTR_WORKAROUND: i32 = 0x01;
/// `EHCIF_PCB_INTR`.
pub const EHCIF_PCB_INTR: i32 = 0x02;
/// `EHCIF_USBMODE`.
pub const EHCIF_USBMODE: i32 = 0x04;

impl EhciSoftc {
    /// `sc->iot`, `sc->ioh`. Panics before the front-end mapped the registers.
    pub fn regs(&self) -> (BusSpaceTag, BusSpaceHandle) {
        match (self.iot.get(), self.ioh.get()) {
            (Some(t), Some(h)) => (t, h),
            _ => panic(format_args!(
                "{}: ehci registers not mapped",
                self.sc_bus.bdev.xname()
            )),
        }
    }

    /// `sc->sc_async_head`. Panics before `ehci_init` allocated it.
    pub fn async_head(&self) -> &'static EhciSoftQh {
        match self.sc_async_head.get() {
            Some(q) => q,
            None => panic(format_args!(
                "{}: ehci without an async list",
                self.sc_bus.bdev.xname()
            )),
        }
    }

    /// `&sc->sc_flist[i]`, checked against `sc_flsize`.
    fn flist_ptr(&self, i: usize) -> *mut EhciLinkT {
        let base = self.sc_flist.get();
        if base.is_null() || i >= self.sc_flsize.get() as usize {
            panic(format_args!("ehci: frame {i} outside the frame list"));
        }
        base.wrapping_add(i)
    }

    /// `letoh32(sc->sc_flist[i])`.
    pub fn flist(&self, i: usize) -> EhciLinkT {
        let p = self.flist_ptr(i);
        // SAFETY: `flist_ptr` checked the index against the list `ehci_init` allocated in DMA
        // memory (freed only by `ehci_detach`); the controller reads it, so the access is
        // volatile.
        u32::from_le(unsafe { ptr::read_volatile(p) })
    }

    /// `sc->sc_flist[i] = htole32(v)`.
    pub fn set_flist(&self, i: usize, v: EhciLinkT) {
        let p = self.flist_ptr(i);
        // SAFETY: as for `flist`.
        unsafe { ptr::write_volatile(p, v.to_le()) }
    }

    /// `&sc->sc_softitds[i]`, checked against `sc_flsize`.
    fn softitd_slot(&self, i: usize) -> &Cell<Option<&'static EhciSoftItd>> {
        let base = self.sc_softitds.get();
        if base.is_null() || i >= self.sc_flsize.get() as usize {
            panic(format_args!("ehci: frame {i} outside the soft itd table"));
        }
        // SAFETY: `ehci_init` allocated `sc_flsize` zeroed entries (a valid `None` each) and
        // frees them only in `ehci_detach`; the index is checked above.
        unsafe { &*base.add(i) }
    }

    /// `sc->sc_softitds[i]`.
    pub fn softitd(&self, i: usize) -> Option<&'static EhciSoftItd> {
        self.softitd_slot(i).get()
    }

    /// `sc->sc_softitds[i] = itd`.
    pub fn set_softitd(&self, i: usize, itd: Option<&'static EhciSoftItd>) {
        self.softitd_slot(i).set(itd);
    }
}

// SAFETY: `#[repr(C)]` with the `usbd_bus` (itself headed by the device) first; the other
// members are `Cell`s of integers, raw pointers, `Option`s of references and tags, a
// `usb_dma` of `Cell`s, queue heads, atomics, a zero-filled `Rwlock` (named by `rw_init`) and
// a zero-filled `Timeout` (set by `timeout_set`): all valid as zero bits.
unsafe impl crate::sys::device::Softc for EhciSoftc {}

/// `EREAD1(sc, a)`.
pub fn eread1(sc: &EhciSoftc, a: BusSize) -> u8 {
    let (t, h) = sc.regs();
    bus_space_read_1(t, h, a)
}

/// `EREAD2(sc, a)`.
pub fn eread2(sc: &EhciSoftc, a: BusSize) -> u16 {
    let (t, h) = sc.regs();
    bus_space_read_2(t, h, a)
}

/// `EREAD4(sc, a)`.
pub fn eread4(sc: &EhciSoftc, a: BusSize) -> u32 {
    let (t, h) = sc.regs();
    bus_space_read_4(t, h, a)
}

/// `EWRITE1(sc, a, x)`.
pub fn ewrite1(sc: &EhciSoftc, a: BusSize, x: u8) {
    let (t, h) = sc.regs();
    bus_space_write_1(t, h, a, x)
}

/// `EWRITE2(sc, a, x)`.
pub fn ewrite2(sc: &EhciSoftc, a: BusSize, x: u16) {
    let (t, h) = sc.regs();
    bus_space_write_2(t, h, a, x)
}

/// `EWRITE4(sc, a, x)`.
pub fn ewrite4(sc: &EhciSoftc, a: BusSize, x: u32) {
    let (t, h) = sc.regs();
    bus_space_write_4(t, h, a, x)
}

/// `sc->sc_offs + a`: an operational register's offset.
fn eoff(sc: &EhciSoftc, a: BusSize) -> BusSize {
    sc.sc_offs.load(Ordering::Relaxed) + a
}

/// `EOREAD1(sc, a)`: an operational register.
pub fn eoread1(sc: &EhciSoftc, a: BusSize) -> u8 {
    eread1(sc, eoff(sc, a))
}

/// `EOREAD2(sc, a)`.
pub fn eoread2(sc: &EhciSoftc, a: BusSize) -> u16 {
    eread2(sc, eoff(sc, a))
}

/// `EOREAD4(sc, a)`.
pub fn eoread4(sc: &EhciSoftc, a: BusSize) -> u32 {
    eread4(sc, eoff(sc, a))
}

/// `EOWRITE1(sc, a, x)`.
pub fn eowrite1(sc: &EhciSoftc, a: BusSize, x: u8) {
    ewrite1(sc, eoff(sc, a), x)
}

/// `EOWRITE2(sc, a, x)`.
pub fn eowrite2(sc: &EhciSoftc, a: BusSize, x: u16) {
    ewrite2(sc, eoff(sc, a), x)
}

/// `EOWRITE4(sc, a, x)`.
pub fn eowrite4(sc: &EhciSoftc, a: BusSize, x: u32) {
    ewrite4(sc, eoff(sc, a), x)
}

const _: () = {
    // The hardware descriptor heads each software one, every chunk slot is aligned for the
    // controller, and a chunk holds at least one descriptor.
    assert!(align_of::<EhciSoftQtd>() <= EHCI_QTD_ALIGN);
    assert!(align_of::<EhciSoftQh>() <= EHCI_QH_ALIGN);
    assert!(align_of::<EhciSoftItd>() <= EHCI_ITD_ALIGN);
    assert!(EHCI_SQTD_CHUNK >= 1 && EHCI_SQH_CHUNK >= 1 && EHCI_ITD_CHUNK >= 1);
    assert!(EHCI_INTRQHS == 255);
};
/* </CODE> */

/* <TESTS> */
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn interrupt_tree_indexes() {
        // Level 0 is the single 1ms QH at index 0; level 7 (128ms) spans 127..254.
        assert_eq!(ehci_iqhidx(0, 0), 0);
        assert_eq!(ehci_iqhidx(0, 12345), 0);
        assert_eq!(ehci_iqhidx(1, 0), 1);
        assert_eq!(ehci_iqhidx(1, 1), 2);
        assert_eq!(ehci_iqhidx(3, 5), 12);
        assert_eq!(ehci_iqhidx(7, 0), 127);
        assert_eq!(ehci_iqhidx(7, 127), 254);
        assert_eq!(ehci_iqhidx(7, 128), 127);
        assert_eq!(ehci_ilev_ival(0), 1);
        assert_eq!(ehci_ilev_ival(7), 128);
        // Every node's parent ((i + 1) / 2 - 1, as ehci_init links them) is one level up.
        for lev in 1..EHCI_IPOLLRATES {
            for pos in 0..(1u32 << lev) {
                let i = ehci_iqhidx(lev, pos);
                let parent = (i + 1) / 2 - 1;
                assert_eq!(parent, ehci_iqhidx(lev - 1, pos >> 1));
            }
        }
    }

    #[test]
    fn chunk_sizes() {
        for (size, align) in [
            (EHCI_SQTD_SIZE, EHCI_QTD_ALIGN),
            (EHCI_SQH_SIZE, EHCI_QH_ALIGN),
            (EHCI_ITD_SIZE, EHCI_ITD_ALIGN),
        ] {
            assert_eq!(size % align, 0);
        }
        assert!(EHCI_SQTD_SIZE >= size_of::<EhciSoftQtd>());
        assert!(EHCI_SQH_SIZE >= size_of::<EhciSoftQh>());
        assert!(EHCI_ITD_SIZE >= size_of::<EhciSoftItd>());
        assert!(EHCI_SQTD_CHUNK * EHCI_SQTD_SIZE <= EHCI_PAGE_SIZE as usize);
        assert!(EHCI_SQH_CHUNK * EHCI_SQH_SIZE <= EHCI_PAGE_SIZE as usize);
        assert!(EHCI_ITD_CHUNK * EHCI_ITD_SIZE <= EHCI_PAGE_SIZE as usize);
    }

    #[test]
    #[ignore = "needs OPENBSD_SRC (just test-ref)"]
    fn constants_match_the_c_header() {
        let defs = crate::reftest::defines("sys/dev/usb/ehcivar.h");
        let mut ours = crate::reftest::assert_defines!(defs;
            EHCI_XFER_ABORTING, EHCI_XFER_ABORTWAIT, EHCI_FRAMELIST_MAXCOUNT, EHCI_IPOLLRATES,
            EHCI_HASH_SIZE, EHCI_COMPANION_MAX, EHCI_FREE_LIST_INTERVAL,
            EHCIF_DROPPED_INTR_WORKAROUND, EHCIF_PCB_INTR, EHCIF_USBMODE,
        );
        // `(1 << EHCI_IPOLLRATES) - 1`: the parser has no `-` after a parenthesis.
        assert_eq!(EHCI_INTRQHS, (1 << EHCI_IPOLLRATES) - 1);
        // Sizes of C structures, recomputed from the Rust ones (see the deviations).
        ours.extend([
            "EHCI_INTRQHS",
            "EHCI_SQTD_SIZE",
            "EHCI_SQTD_CHUNK",
            "EHCI_SQH_SIZE",
            "EHCI_SQH_CHUNK",
            "EHCI_ITD_SIZE",
            "EHCI_ITD_CHUNK",
        ]);
        crate::reftest::assert_complete(&defs, "EHCI", &ours);
    }
}
/* </TESTS> */
