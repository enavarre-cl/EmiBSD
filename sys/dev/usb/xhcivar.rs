/* <LICENSES> */
/*
 * Copyright (c) 2014 Martin Pieuchot
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
/* </LICENSES> */

/* <CODE> */
//! The xHCI driver's structures: `<dev/usb/xhcivar.h>`, the softc the bus front-ends
//! (`xhci_pci`) embed, the rings and the device contexts.
//!
//! Upstream: sys/dev/usb/xhcivar.h @ 3ce1f3f79392
//!
//! Every member is valid as all-zero bits (the softc is `M_ZERO`, an xfer `PR_ZERO`). The
//! members change under the kernel lock at `splusb()`, as in C, except those the
//! `IPL_MPSAFE` hard interrupt (`xhci_intr`) touches without it: `sc_dead` and the register
//! offsets are atomics (`docs/C_TO_RUST.md`, a field another CPU reads without the writer's
//! lock); the tag and handle are written before the interrupt is established and never
//! again.
//!
//! The rings, the device context base address array, the event ring segment table, the
//! scratchpad table and the contexts are DMA memory the controller reads and writes: they are
//! reached through raw pointers with volatile accesses ([`XhciTrbRef`], the `*_update`
//! methods), never through references (`docs/C_TO_RUST.md`, DMA memory a device reads and
//! writes concurrently).
//!
//! ## Deviations
//! - `XREAD1(sc, a)`, `XOWRITE4(sc, a, x)` and the other accessor macros are functions of the
//!   same names in lower case taking the softc ([`xread1`], [`xowrite4`], ...).
//! - `sc_oper_off`, `sc_runt_off`, `sc_door_off` and `sc_dead` are relaxed atomics: the
//!   `IPL_MPSAFE` interrupt handler reads them on any CPU, possibly while `xhci_init` sets
//!   them (a shared INTx line can fire before the controller is configured).
//! - `struct xhci_soft_dev`'s `memset(sdev, 0, ...)` is [`XhciSoftDev::clear`].
//! - The pointers into DMA memory (`trbs`, `segs`, `input_ctx`, `slot_ctx`, `ep_ctx`) are
//!   kept as in C, and every access through them goes through a bounds-checked accessor:
//!   [`XhciRing::trb`], [`XhciDevctx::set_seg`], [`XhciErst::set_seg`] and the context
//!   `*_update` methods.
//! - `xhci_init`, `xhci_config`, `xhci_reinit`, `xhci_intr`, `xhci_detach` and
//!   `xhci_activate`, declared here, are defined in `xhci.rs` with the file they come from.

use core::cell::Cell;
use core::ptr::{self, NonNull};
use core::sync::atomic::{AtomicI32, AtomicUsize, Ordering};

use crate::dev::usb::usb::USB_MAX_DEVICES;
use crate::dev::usb::usbdivar::{UsbdBus, UsbdHcXfer, UsbdXfer};
use crate::dev::usb::xhci::XhciPipe;
use crate::dev::usb::xhcireg::{XhciEpctx, XhciErseg, XhciInctx, XhciSctx, XhciTrb};
use crate::kern::subr_prf::panic;
use crate::machine::bus::{
    BusAddr, BusDmaSegment, BusDmaTag, BusDmamap, BusSize, BusSpaceHandle, BusSpaceTag,
    bus_space_read_4, bus_space_write_1, bus_space_write_2, bus_space_write_4,
};
use crate::sys::rwlock::Rwlock;
use crate::sys::time::msec_to_nsec;

/// `XHCI_CMD_TIMEOUT`: default command execution time (implementation defined), in ns.
pub const XHCI_CMD_TIMEOUT: u64 = msec_to_nsec(500);

/// `XHCI_MAX_CMDS`.
pub const XHCI_MAX_CMDS: usize = 16;
/// `XHCI_MAX_EVTS`.
pub const XHCI_MAX_EVTS: usize = 16 * 13;
/// `XHCI_MAX_XFER`.
pub const XHCI_MAX_XFER: usize = 16 * 16;

/// `struct usbd_dma_info`: one contiguous DMA allocation, mapped and loaded
/// (`usbd_dma_contig_alloc`).
pub struct UsbdDmaInfo {
    /// `tag`.
    pub tag: Cell<Option<BusDmaTag>>,
    /// `map`: `None` while nothing is allocated.
    pub map: Cell<Option<&'static BusDmamap>>,
    /// `seg`.
    pub seg: Cell<BusDmaSegment>,
    /// `nsegs`.
    pub nsegs: Cell<i32>,
    /// `paddr`: the bus address of the first byte.
    pub paddr: Cell<BusAddr>,
    /// `vaddr`: the kernel mapping.
    pub vaddr: Cell<*mut u8>,
    /// `size`.
    pub size: Cell<BusSize>,
}

impl UsbdDmaInfo {
    /// `dma->tag`. Panics before `usbd_dma_contig_alloc` set it.
    pub fn tag(&self) -> BusDmaTag {
        match self.tag.get() {
            Some(t) => t,
            None => panic(format_args!("usbd_dma_info without a tag")),
        }
    }

    /// `dma->map`. Panics while nothing is allocated.
    pub fn map(&self) -> &'static BusDmamap {
        match self.map.get() {
            Some(m) => m,
            None => panic(format_args!("usbd_dma_info without a map")),
        }
    }

    /// A pointer to the `T` at byte `off` of the allocation, checked against its size and
    /// `T`'s alignment. Panics outside the allocation (the C's overrun).
    pub fn at<T>(&self, off: usize) -> NonNull<T> {
        let base = self.vaddr.get();
        let end = off.checked_add(size_of::<T>());
        let ok = !base.is_null()
            && end.is_some_and(|e| e <= self.size.get())
            && (base as usize + off).is_multiple_of(align_of::<T>());
        if !ok {
            panic(format_args!(
                "usbd_dma_info: offset {off} outside the allocation"
            ));
        }
        // SAFETY: `base` is non-null and `off + size_of::<T>()` is inside the mapping.
        unsafe { NonNull::new_unchecked(base.add(off).cast::<T>()) }
    }

    /// `((uint64_t *)dma->vaddr)[i] = htole64(v)`: a 64-bit entry of a table the controller
    /// reads (DCBAA, scratchpad table).
    pub fn write_le64(&self, i: usize, v: u64) {
        let p = self.at::<u64>(i * size_of::<u64>());
        // SAFETY: `at` checked the entry is inside the live allocation; the controller reads
        // the memory, so the write is volatile.
        unsafe { ptr::write_volatile(p.as_ptr(), v.to_le()) };
    }

    /// Zeroes the bookkeeping (`memset` of the structure) after a free.
    pub fn clear(&self) {
        self.tag.set(None);
        self.map.set(None);
        self.seg.set(BusDmaSegment::default());
        self.nsegs.set(0);
        self.paddr.set(0);
        self.vaddr.set(ptr::null_mut());
        self.size.set(0);
    }
}

/// `struct xhci_xfer`: an xfer of this controller.
#[repr(C)]
pub struct XhciXfer {
    /// `xfer`: the generic xfer, first.
    pub xfer: UsbdXfer,
    /// `index`: index of the last TRB (`-1` none, `-2` while a chain is being built).
    pub index: Cell<i32>,
    /// `ntrb`: number of associated TRBs.
    pub ntrb: Cell<usize>,
    /// `zerotd`: is a zero length TD required?
    pub zerotd: Cell<usize>,
}

// SAFETY: `#[repr(C)]` with the `usbd_xfer` first; the other members are integer `Cell`s,
// valid as zero bits (`pool_get(..., PR_ZERO)`).
unsafe impl UsbdHcXfer for XhciXfer {}

/// A TRB inside a ring: its address and index, made only by [`XhciRing::trb`]. Accesses are
/// volatile and convert from and to the controller's little-endian layout.
///
/// Valid while its ring is allocated: the driver uses one only inside the function that got
/// it, at `splusb()` under the kernel lock, and frees a ring only when the pipe or the
/// controller goes away.
#[derive(Clone, Copy, Debug)]
pub struct XhciTrbRef {
    p: NonNull<XhciTrb>,
    idx: usize,
}

impl XhciTrbRef {
    /// The TRB's address (`struct xhci_trb *`).
    pub fn as_ptr(self) -> *mut XhciTrb {
        self.p.as_ptr()
    }

    /// The TRB's index in its ring.
    pub fn index(self) -> usize {
        self.idx
    }

    /// `TRBOFF(r, trb)`: the TRB's byte offset in its ring.
    pub fn off(self) -> usize {
        self.idx * size_of::<XhciTrb>()
    }

    /// `*trb`, as stored (little-endian members).
    pub fn get(self) -> XhciTrb {
        // SAFETY: the type's invariant (inside a live ring); the controller writes the
        // memory, so the read is volatile.
        unsafe { ptr::read_volatile(self.p.as_ptr()) }
    }

    /// `letoh64(trb->trb_paddr)`.
    pub fn paddr(self) -> u64 {
        // SAFETY: as for `get`.
        u64::from_le(unsafe { ptr::read_volatile(&raw const (*self.p.as_ptr()).trb_paddr) })
    }

    /// `trb->trb_paddr = htole64(v)`.
    pub fn set_paddr(self, v: u64) {
        // SAFETY: as for `get`; the controller reads the memory.
        unsafe { ptr::write_volatile(&raw mut (*self.p.as_ptr()).trb_paddr, v.to_le()) }
    }

    /// `trb->trb_paddr = v`, `v` already in the controller's byte order (a SETUP packet).
    pub fn set_paddr_raw(self, v: u64) {
        // SAFETY: as for `set_paddr`.
        unsafe { ptr::write_volatile(&raw mut (*self.p.as_ptr()).trb_paddr, v) }
    }

    /// `trb->trb_status = v`, `v` already in the controller's byte order.
    pub fn set_status_raw(self, v: u32) {
        // SAFETY: as for `set_paddr`.
        unsafe { ptr::write_volatile(&raw mut (*self.p.as_ptr()).trb_status, v) }
    }

    /// `trb->trb_flags = v`, `v` already in the controller's byte order.
    pub fn set_flags_raw(self, v: u32) {
        // SAFETY: as for `set_paddr`.
        unsafe { ptr::write_volatile(&raw mut (*self.p.as_ptr()).trb_flags, v) }
    }

    /// `letoh32(trb->trb_status)`.
    pub fn status(self) -> u32 {
        // SAFETY: as for `get`.
        u32::from_le(unsafe { ptr::read_volatile(&raw const (*self.p.as_ptr()).trb_status) })
    }

    /// `trb->trb_status = htole32(v)`.
    pub fn set_status(self, v: u32) {
        // SAFETY: as for `set_paddr`.
        unsafe { ptr::write_volatile(&raw mut (*self.p.as_ptr()).trb_status, v.to_le()) }
    }

    /// `letoh32(trb->trb_flags)`.
    pub fn flags(self) -> u32 {
        // SAFETY: as for `get`.
        u32::from_le(unsafe { ptr::read_volatile(&raw const (*self.p.as_ptr()).trb_flags) })
    }

    /// `trb->trb_flags = htole32(v)`.
    pub fn set_flags(self, v: u32) {
        // SAFETY: as for `set_paddr`.
        unsafe { ptr::write_volatile(&raw mut (*self.p.as_ptr()).trb_flags, v.to_le()) }
    }
}

/// `struct xhci_ring`: a TRB ring of one segment whose last TRB links back to the first
/// (except the event ring, which the segment table bounds).
pub struct XhciRing {
    /// `trbs`: `ntrb` TRBs in `dma`.
    pub trbs: Cell<*mut XhciTrb>,
    /// `ntrb`.
    pub ntrb: Cell<usize>,
    /// `dma`.
    pub dma: UsbdDmaInfo,
    /// `index`: the next TRB to produce or consume.
    pub index: Cell<u32>,
    /// `toggle`: Producer/Consumer bit.
    pub toggle: Cell<u32>,
}

impl XhciRing {
    /// `&ring->trbs[i]`. Panics outside the ring (the C's overrun).
    pub fn trb(&self, i: usize) -> XhciTrbRef {
        let base = self.trbs.get();
        if base.is_null() || i >= self.ntrb.get() {
            panic(format_args!("xhci_ring: TRB {i} outside the ring"));
        }
        XhciTrbRef {
            // SAFETY: `xhci_ring_alloc` set `trbs` to the mapping of `ntrb` TRBs, and `i` is
            // below `ntrb`.
            p: unsafe { NonNull::new_unchecked(base.add(i)) },
            idx: i,
        }
    }

    /// `DEQPTR(r)`: the bus address of the TRB at `index`.
    pub fn deqptr(&self) -> u64 {
        (self.dma.paddr.get() + size_of::<XhciTrb>() * self.index.get() as usize) as u64
    }
}

/// `struct xhci_soft_dev`: a device slot's contexts and open pipes.
pub struct XhciSoftDev {
    /// `input_ctx`: Input context.
    pub input_ctx: Cell<*mut XhciInctx>,
    /// `slot_ctx`: in the input context.
    pub slot_ctx: Cell<*mut XhciSctx>,
    /// `ep_ctx`: in the input context.
    pub ep_ctx: [Cell<*mut XhciEpctx>; 31],
    /// `ictx_dma`.
    pub ictx_dma: UsbdDmaInfo,
    /// `octx_dma`: Output context.
    pub octx_dma: UsbdDmaInfo,
    /// `pipes`: by DCI - 1.
    pub pipes: [Cell<Option<&'static XhciPipe>>; 31],
}

/// `*ctx = f(*ctx)` on a context in DMA memory, as one volatile read and one volatile write.
fn ctx_update<T: Copy>(p: *mut T, what: &str, f: impl FnOnce(&mut T)) {
    if p.is_null() {
        panic(format_args!("xhci_soft_dev: no {what} context"));
    }
    // SAFETY: `xhci_softdev_alloc` points the context pointers into the slot's input
    // context page, live until `xhci_softdev_free` clears them (null is refused above); the
    // controller reads the page during commands, so the accesses are volatile.
    unsafe {
        let mut v = ptr::read_volatile(p);
        f(&mut v);
        ptr::write_volatile(p, v);
    }
}

impl XhciSoftDev {
    /// Changes `*sdev->input_ctx` (members in the controller's byte order).
    pub fn input_ctx_update(&self, f: impl FnOnce(&mut XhciInctx)) {
        ctx_update(self.input_ctx.get(), "input", f);
    }

    /// Changes `*sdev->slot_ctx` (members in the controller's byte order).
    pub fn slot_ctx_update(&self, f: impl FnOnce(&mut XhciSctx)) {
        ctx_update(self.slot_ctx.get(), "slot", f);
    }

    /// Changes `*sdev->ep_ctx[i]` (members in the controller's byte order).
    pub fn ep_ctx_update(&self, i: usize, f: impl FnOnce(&mut XhciEpctx)) {
        let p = self.ep_ctx.get(i).map_or(ptr::null_mut(), Cell::get);
        ctx_update(p, "endpoint", f);
    }

    /// `memset(sdev, 0, sizeof(struct xhci_soft_dev))`.
    pub fn clear(&self) {
        self.input_ctx.set(ptr::null_mut());
        self.slot_ctx.set(ptr::null_mut());
        for c in &self.ep_ctx {
            c.set(ptr::null_mut());
        }
        self.ictx_dma.clear();
        self.octx_dma.clear();
        for p in &self.pipes {
            p.set(None);
        }
    }
}

/// `struct xhci_devctx`: device context segment table.
pub struct XhciDevctx {
    /// `segs`: at most `USB_MAX_DEVICES + 1`.
    pub segs: Cell<*mut u64>,
    /// `dma`.
    pub dma: UsbdDmaInfo,
}

impl XhciDevctx {
    /// `sc->sc_dcbaa.segs[i] = htole64(v)`.
    pub fn set_seg(&self, i: usize, v: u64) {
        self.dma.write_le64(i, v);
    }
}

/// `struct xhci_erst`: event ring segment table.
pub struct XhciErst {
    /// `segs`: one segment per event ring.
    pub segs: Cell<*mut XhciErseg>,
    /// `dma`.
    pub dma: UsbdDmaInfo,
}

impl XhciErst {
    /// `sc->sc_erst.segs[i] = *seg` (members already in the controller's byte order).
    pub fn set_seg(&self, i: usize, seg: XhciErseg) {
        let p = self.dma.at::<XhciErseg>(i * size_of::<XhciErseg>());
        // SAFETY: `at` checked the entry is inside the live table; the controller reads it.
        unsafe { ptr::write_volatile(p.as_ptr(), seg) };
    }
}

/// `struct xhci_scratchpad`.
pub struct XhciScratchpad {
    /// `table_dma`.
    pub table_dma: UsbdDmaInfo,
    /// `pages_dma`.
    pub pages_dma: UsbdDmaInfo,
    /// `npage`.
    pub npage: Cell<i32>,
}

/// `struct xhci_softc`: what a bus front-end's softc starts with.
#[repr(C)]
pub struct XhciSoftc {
    /// `sc_bus`: the generic bus, first.
    pub sc_bus: UsbdBus,

    /// `iot`.
    pub iot: Cell<Option<BusSpaceTag>>,
    /// `ioh`.
    pub ioh: Cell<Option<BusSpaceHandle>>,
    /// `sc_size`.
    pub sc_size: Cell<BusSize>,

    /// `sc_dead`: read by the `IPL_MPSAFE` interrupt.
    pub sc_dead: AtomicI32,
    /// `sc_saved_state`.
    pub sc_saved_state: Cell<i32>,

    /// `sc_oper_off`: Operational Register space.
    pub sc_oper_off: AtomicUsize,
    /// `sc_runt_off`: Runtime.
    pub sc_runt_off: AtomicUsize,
    /// `sc_door_off`: Doorbell.
    pub sc_door_off: AtomicUsize,

    /// `sc_version`: xHCI version.
    pub sc_version: Cell<u16>,
    /// `sc_pagesize`: xHCI page size, minimum 4k.
    pub sc_pagesize: Cell<u32>,
    /// `sc_ctxsize`: 32/64 byte context structs.
    pub sc_ctxsize: Cell<u32>,

    /// `sc_noport`: Maximum number of ports.
    pub sc_noport: Cell<i32>,

    /// `sc_conf`: Device configuration.
    pub sc_conf: Cell<u8>,
    /// `sc_intrxfer`: Root HUB interrupt xfer.
    pub sc_intrxfer: Cell<Option<&'static UsbdXfer>>,

    /// `sc_dcbaa`: Device context base addr.
    pub sc_dcbaa: XhciDevctx,
    /// `sc_cmd_ring`: Command ring.
    pub sc_cmd_ring: XhciRing,
    /// `sc_cmd_lock`: Serialize commands.
    pub sc_cmd_lock: Rwlock,

    /// `sc_erst`: Event ring segment table.
    pub sc_erst: XhciErst,
    /// `sc_evt_ring`: Event ring.
    pub sc_evt_ring: XhciRing,

    /// `sc_spad`: Optional scratchpad.
    pub sc_spad: XhciScratchpad,

    /// `sc_noslot`: Maximum number of slots.
    pub sc_noslot: Cell<i32>,
    /// `sc_sdevs`.
    pub sc_sdevs: [XhciSoftDev; USB_MAX_DEVICES],

    /// `sc_cmd_trb`: the command TRB a synchronous command waits for (also its wait
    /// channel's address).
    pub sc_cmd_trb: Cell<*mut XhciTrb>,
    /// `sc_result_trb`: the completion event of the last command.
    pub sc_result_trb: Cell<XhciTrb>,

    /// `sc_vendor`: Vendor string for root hub.
    pub sc_vendor: Cell<[u8; 16]>,
    /// `sc_id_vendor`: Vendor ID for root hub.
    pub sc_id_vendor: Cell<i32>,

    /// `sc_flags`: `XHCI_NOCSS`.
    pub sc_flags: Cell<i32>,
}

/// `XHCI_NOCSS`: do not save the controller's state on suspend.
pub const XHCI_NOCSS: i32 = 0x01;

impl XhciSoftc {
    /// `sc->iot`, `sc->ioh`. Panics before the front-end mapped the registers.
    pub fn regs(&self) -> (BusSpaceTag, BusSpaceHandle) {
        match (self.iot.get(), self.ioh.get()) {
            (Some(t), Some(h)) => (t, h),
            _ => panic(format_args!(
                "{}: xhci registers not mapped",
                self.sc_bus.bdev.xname()
            )),
        }
    }

    /// `&sc->sc_sdevs[slot]`. Panics beyond the table.
    pub fn sdev(&self, slot: usize) -> &XhciSoftDev {
        match self.sc_sdevs.get(slot) {
            Some(s) => s,
            None => panic(format_args!("xhci: slot {slot} beyond the table")),
        }
    }
}

// SAFETY: `#[repr(C)]` with the `usbd_bus` (itself headed by the device) first; the other
// members are `Cell`s of integers, raw pointers, `Option`s of references and tags, arrays of
// them, atomics and a zero-filled `Rwlock` (named by `rw_init`): all valid as zero bits.
unsafe impl crate::sys::device::Softc for XhciSoftc {}

/// `xhci_read_1`: a byte register read as part of its 32-bit word (some controllers only
/// decode 32-bit accesses).
pub fn xhci_read_1(iot: BusSpaceTag, ioh: BusSpaceHandle, offset: BusSize) -> u8 {
    let reg = bus_space_read_4(iot, ioh, offset & !3);
    ((reg >> ((offset & 3) * 8)) & 0xff) as u8
}

/// `xhci_read_2`: a 16-bit register read as part of its 32-bit word.
pub fn xhci_read_2(iot: BusSpaceTag, ioh: BusSpaceHandle, offset: BusSize) -> u16 {
    let reg = bus_space_read_4(iot, ioh, offset & !2);
    ((reg >> ((offset & 2) * 8)) & 0xffff) as u16
}

/// `XREAD1(sc, a)`.
pub fn xread1(sc: &XhciSoftc, a: BusSize) -> u8 {
    let (t, h) = sc.regs();
    xhci_read_1(t, h, a)
}

/// `XREAD2(sc, a)`.
pub fn xread2(sc: &XhciSoftc, a: BusSize) -> u16 {
    let (t, h) = sc.regs();
    xhci_read_2(t, h, a)
}

/// `XREAD4(sc, a)`.
pub fn xread4(sc: &XhciSoftc, a: BusSize) -> u32 {
    let (t, h) = sc.regs();
    bus_space_read_4(t, h, a)
}

/// `XWRITE1(sc, a, x)`.
pub fn xwrite1(sc: &XhciSoftc, a: BusSize, x: u8) {
    let (t, h) = sc.regs();
    bus_space_write_1(t, h, a, x)
}

/// `XWRITE2(sc, a, x)`.
pub fn xwrite2(sc: &XhciSoftc, a: BusSize, x: u16) {
    let (t, h) = sc.regs();
    bus_space_write_2(t, h, a, x)
}

/// `XWRITE4(sc, a, x)`.
pub fn xwrite4(sc: &XhciSoftc, a: BusSize, x: u32) {
    let (t, h) = sc.regs();
    bus_space_write_4(t, h, a, x)
}

/// `XOREAD4(sc, a)`: an operational register.
pub fn xoread4(sc: &XhciSoftc, a: BusSize) -> u32 {
    xread4(sc, sc.sc_oper_off.load(Ordering::Relaxed) + a)
}

/// `XOWRITE4(sc, a, x)`.
pub fn xowrite4(sc: &XhciSoftc, a: BusSize, x: u32) {
    xwrite4(sc, sc.sc_oper_off.load(Ordering::Relaxed) + a, x)
}

/// `XRREAD4(sc, a)`: a runtime register.
pub fn xrread4(sc: &XhciSoftc, a: BusSize) -> u32 {
    xread4(sc, sc.sc_runt_off.load(Ordering::Relaxed) + a)
}

/// `XRWRITE4(sc, a, x)`.
pub fn xrwrite4(sc: &XhciSoftc, a: BusSize, x: u32) {
    xwrite4(sc, sc.sc_runt_off.load(Ordering::Relaxed) + a, x)
}

/// `XDREAD4(sc, a)`: a doorbell register.
pub fn xdread4(sc: &XhciSoftc, a: BusSize) -> u32 {
    xread4(sc, sc.sc_door_off.load(Ordering::Relaxed) + a)
}

/// `XDWRITE4(sc, a, x)`.
pub fn xdwrite4(sc: &XhciSoftc, a: BusSize, x: u32) {
    xwrite4(sc, sc.sc_door_off.load(Ordering::Relaxed) + a, x)
}
/* </CODE> */

/* <TESTS> */
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn limits() {
        assert_eq!(XHCI_MAX_CMDS, 16);
        assert_eq!(XHCI_MAX_EVTS, 208);
        assert_eq!(XHCI_MAX_XFER, 256);
        assert_eq!(XHCI_CMD_TIMEOUT, 500_000_000);
    }
}
/* </TESTS> */
