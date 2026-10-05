/*	$OpenBSD: usbdivar.h,v 1.85 2025/03/01 14:43:03 kirill Exp $ */
/*	$NetBSD: usbdivar.h,v 1.70 2002/07/11 21:14:36 augustss Exp $	*/
/*	$FreeBSD: src/sys/dev/usb/usbdivar.h,v 1.11 1999/11/17 22:33:51 n_hibma Exp $	*/
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

//! The USB stack's internal structures: `<dev/usb/usbdivar.h>`, what the host controller
//! drivers and the stack share (`struct usbd_bus`, `usbd_device`, `usbd_interface`,
//! `usbd_pipe`, `usbd_xfer` and the method tables).
//!
//! Upstream: sys/dev/usb/usbdivar.h @ 3ce1f3f79392
//!
//! Every structure here is allocated zeroed (`malloc(..., M_ZERO)`, `pool_get(..., PR_ZERO)`)
//! or lives in a zeroed softc, so every member is valid as all-zero bits: pointers are
//! `Cell<Option<&'static T>>` (`None` is NULL) or raw pointers, counters are `Cell`s of
//! integers. The members change under the kernel lock and `splusb()`, as in C (no USB path is
//! `IPL_MPSAFE`), which is what makes the `Cell`s sound.
//!
//! The objects are long-lived kernel allocations handed around as `&'static T`, as the C
//! passes pointers: a `usbd_device` lives from `usbd_new_device` to `usb_free_device`, a pipe
//! from `usbd_setup_pipe` to `usbd_close_pipe`, an xfer from `usbd_alloc_xfer` to
//! `usbd_free_xfer`, an interface and its endpoints until the configuration changes. As in C,
//! nobody may use one after it is freed; the functions that free are `unsafe` and take a
//! `NonNull` (`docs/C_TO_RUST.md`, a typed object returned by `*_create`).
//!
//! A host controller embeds the [`UsbdBus`] first in its softc, as the C's `struct
//! xhci_softc { struct usbd_bus sc_bus; ... }`, and its pipes and xfers embed [`UsbdPipe`] and
//! [`UsbdXfer`] first ([`UsbdHcPipe`], [`UsbdHcXfer`]); [`usbd_bus_set_hc_types`] records
//! their types (the C's `pipe_size`) so that [`UsbdPipe::hc`] and [`UsbdXfer::hc`] can check
//! the casts back.
//!
//! ## Deviations
//! - Descriptors inside the configuration descriptor are `&'static` references into it (it
//!   is not changed once a device is configured, see `usbd_set_config_index`); an endpoint's
//!   `edesc` is a raw pointer read by copy ([`UsbdEndpoint::edesc`]), because pipe 0's
//!   descriptor (`usbd_device.def_ep_desc`) is changed in place after the pointer to it is
//!   taken (`usbd_new_device` fixes `wMaxPacketSize`).
//! - The arrays the C indexes through pointers (`ifaces`, `endpoints`, `ports`, `subdevs`)
//!   are raw pointers with slice accessors whose length is the C's count (`bNumInterfaces`,
//!   `nendpt`, `nports`, `nsubdev`).
//! - `usbd_bus.pipe_size` stays; [`UsbdBus`] also records the host controller's pipe and xfer
//!   types (`pipe_type`, `xfer_type`) so the casts to them are checked (`docs/C_TO_RUST.md`, a
//!   structure whose allocation size a member records).
//! - `usbd_xfer.done` (`volatile char`) is an atomic; `busy_free` exists only under feature
//!   `diagnostic`, as under `DIAGNOSTIC`.
//! - `usbd_bus.bpfif` is the tap `bpfsattach` returned; `bpf` is the `Cell` the bpf code
//!   points at its listeners (`NBPFILTER` is always on here).
//! - `usbd_dump_iface` & co. are `USB_DEBUG`-only in `usbdi.c`; `USB_DEBUG` is not in GENERIC,
//!   so they are not ported (see `usbdi.rs`).

use core::any::TypeId;
use core::cell::Cell;
use core::ffi::c_void;
use core::ptr::{self, NonNull};
use core::slice;
use core::sync::atomic::AtomicBool;

use crate::dev::usb::usb::{
    UE_DIR_IN, USB_MAX_DEVICES, USB_MAX_STRING_LEN, UT_READ, UsbConfigDescriptor,
    UsbDeviceDescriptor, UsbDeviceRequest, UsbDeviceStats, UsbEndpointDescriptor,
    UsbEndpointSsCompDescriptor, UsbInterfaceDescriptor, UsbPortStatus,
};
use crate::dev::usb::usb_mem::UsbDmaBlock;
use crate::dev::usb::usb_quirks::UsbdQuirks;
use crate::dev::usb::usbdi::{UsbTask, UsbdCallback, UsbdStatus};
use crate::kern::kern_softintr::SoftintrHand;
use crate::kern::subr_prf::panic;
use crate::machine::bus::BusDmaTag;
use crate::net::bpfdesc::BpfIf;
use crate::queue_adapter;
use crate::sys::device::Device;
use crate::sys::errno::Errno;
use crate::sys::queue::{ListEntry, ListHead, SimpleqEntry, SimpleqHead};
use crate::sys::timeout::Timeout;

/// `USB_DMA_COHERENT`: `usb_allocmem` flag for coherent memory.
pub const USB_DMA_COHERENT: i32 = 1 << 0;

/// `struct usb_dma` ("from usb_mem.h"): a piece of a DMA block.
pub struct UsbDma {
    /// `block`: the block (blocks are never freed, `usb_mem.c`).
    pub block: Cell<Option<&'static UsbDmaBlock>>,
    /// `offs`: the offset of this piece in the block.
    pub offs: Cell<u32>,
}

/// `struct usbd_endpoint`.
pub struct UsbdEndpoint {
    /// `edesc`: the endpoint descriptor, inside the configuration descriptor or the device's
    /// `def_ep_desc`.
    edesc: Cell<*const UsbEndpointDescriptor>,
    /// `esscd`: the super speed companion descriptor, if any.
    esscd: Cell<*const UsbEndpointSsCompDescriptor>,
    /// `refcnt`: pipes open on the endpoint.
    pub refcnt: Cell<i32>,
    /// `savedtoggle`.
    pub savedtoggle: Cell<i32>,
}

impl UsbdEndpoint {
    /// `ep->edesc != NULL`.
    pub fn has_edesc(&self) -> bool {
        !self.edesc.get().is_null()
    }

    /// `*ep->edesc`: a copy of the endpoint descriptor. Panics without one (the C's NULL
    /// dereference).
    pub fn edesc(&self) -> UsbEndpointDescriptor {
        let p = self.edesc.get();
        if p.is_null() {
            panic(format_args!("usbd_endpoint without a descriptor"));
        }
        // SAFETY: `set_edesc`'s contract: the descriptor outlives the endpoint; the type has
        // alignment 1 and any bytes are a value; the read is a copy, so a later change of the
        // descriptor in place (pipe 0's) is not undercut by a live reference.
        unsafe { ptr::read(p) }
    }

    /// `ep->edesc`, the pointer, for host controllers that compare it.
    pub fn edesc_ptr(&self) -> *const UsbEndpointDescriptor {
        self.edesc.get()
    }

    /// `ep->esscd`: a copy of the super speed companion descriptor, `None` for NULL.
    pub fn esscd(&self) -> Option<UsbEndpointSsCompDescriptor> {
        let p = self.esscd.get();
        if p.is_null() {
            return None;
        }
        // SAFETY: as for `edesc`.
        Some(unsafe { ptr::read(p) })
    }

    /// `ep->edesc = ed; ep->esscd = essd;`.
    ///
    /// # Safety
    ///
    /// Each non-null pointer points at a descriptor that stays allocated as long as the
    /// endpoint is used (the configuration descriptor until the configuration changes, or the
    /// device's `def_ep_desc`).
    pub unsafe fn set_edesc(
        &self,
        ed: *const UsbEndpointDescriptor,
        essd: *const UsbEndpointSsCompDescriptor,
    ) {
        self.edesc.set(ed);
        self.esscd.set(essd);
    }
}

/// The type of `usbd_bus_methods.dev_setaddr`.
pub type UsbdSetaddrFn = fn(dev: &'static UsbdDevice, addr: i32) -> Result<(), Errno>;

/// `struct usbd_bus_methods`: a host controller's operations on its bus.
pub struct UsbdBusMethods {
    /// `open_pipe`: sets up the controller's part of a new pipe (`methods`, rings, ...).
    pub open_pipe: fn(pipe: &'static UsbdPipe) -> UsbdStatus,
    /// `dev_setaddr`: sets the device's address (xHCI assigns its own; others send
    /// `SET_ADDRESS` with `usbd_set_address`). `None` skips the step, as a NULL method does.
    pub dev_setaddr: Option<UsbdSetaddrFn>,
    /// `soft_intr`: the soft interrupt handler, with the bus as argument.
    pub soft_intr: fn(bus: *mut c_void),
    /// `do_poll`: polls the controller (`usbd_dopoll`).
    pub do_poll: fn(bus: &'static UsbdBus),
    /// `allocx`: a zeroed xfer of the controller's type ([`UsbdHcXfer`]).
    pub allocx: fn(bus: &'static UsbdBus) -> Option<&'static UsbdXfer>,
    /// `freex`: frees an xfer `allocx` returned.
    pub freex: unsafe fn(bus: &'static UsbdBus, xfer: NonNull<UsbdXfer>),
}

/// `struct usbd_pipe_methods`: a host controller's operations on a pipe.
pub struct UsbdPipeMethods {
    /// `transfer`: queues an xfer (usually `usb_insert_transfer`, then `start` if the pipe
    /// was idle).
    pub transfer: fn(xfer: &'static UsbdXfer) -> UsbdStatus,
    /// `start`: starts the xfer at the head of the queue.
    pub start: fn(xfer: &'static UsbdXfer) -> UsbdStatus,
    /// `abort`: aborts an xfer and completes it (`usb_transfer_complete`).
    pub abort: fn(xfer: &'static UsbdXfer),
    /// `close`: tears down the controller's part of the pipe.
    pub close: fn(pipe: &'static UsbdPipe),
    /// `cleartoggle`: resets the data toggle, if the controller keeps one.
    pub cleartoggle: Option<fn(pipe: &'static UsbdPipe)>,
    /// `done`: the controller's part of completing an xfer, from `usb_transfer_complete`.
    pub done: fn(xfer: &'static UsbdXfer),
}

/// `struct usbd_tt`: a transaction translator.
pub struct UsbdTt {
    /// `hub`.
    pub hub: Cell<Option<&'static UsbdHub>>,
    /// `hcpriv`: the host controller's.
    pub hcpriv: Cell<*mut c_void>,
}

/// `struct usbd_port`: a hub's port.
pub struct UsbdPort {
    /// `status`.
    pub status: Cell<UsbPortStatus>,
    /// `power`: mA of current on port.
    pub power: Cell<u16>,
    /// `portno`.
    pub portno: Cell<u8>,
    /// `restartcnt`.
    pub restartcnt: Cell<u8>,
    /// `reattach`.
    pub reattach: Cell<u8>,
    /// `device`: connected device.
    pub device: Cell<Option<&'static UsbdDevice>>,
    /// `parent`: the port's hub.
    pub parent: Cell<Option<&'static UsbdDevice>>,
    /// `tt`: transaction translator (if any).
    pub tt: Cell<Option<&'static UsbdTt>>,
}

/// `USBD_RESTART_MAX`.
pub const USBD_RESTART_MAX: u8 = 5;

/// The type of `usbd_hub.explore`.
pub type UsbdExploreFn = fn(hub: &'static UsbdDevice) -> i32;

/// `struct usbd_hub`: what `uhub(4)` hangs on a hub's device.
pub struct UsbdHub {
    /// `explore`.
    pub explore: Cell<Option<UsbdExploreFn>>,
    /// `hubsoftc`: the `uhub` softc.
    pub hubsoftc: Cell<*mut c_void>,
    /// `ports`: `nports` entries (`mallocarray`ed by the hub driver).
    pub ports: Cell<*mut UsbdPort>,
    /// `nports`.
    pub nports: Cell<i32>,
    /// `powerdelay`.
    pub powerdelay: Cell<u8>,
    /// `ttthink`.
    pub ttthink: Cell<u8>,
    /// `multi`.
    pub multi: Cell<u8>,
}

impl UsbdHub {
    /// `hub->ports[0..nports]`.
    pub fn ports(&self) -> &'static [UsbdPort] {
        let p = self.ports.get();
        let n = self.nports.get();
        if p.is_null() || n <= 0 {
            return &[];
        }
        // SAFETY: the hub driver allocated `nports` initialised ports at `ports` and frees
        // them only after clearing `usbd_device.hub` (uhub_detach); the ports are `Cell`s.
        unsafe { slice::from_raw_parts(p, n as usize) }
    }

    /// `hub->explore(dev)`. Panics when the hub driver set none (the C's NULL call).
    pub fn explore(&self, dev: &'static UsbdDevice) -> i32 {
        match self.explore.get() {
            Some(f) => f(dev),
            None => panic(format_args!("usbd_hub without explore")),
        }
    }
}

/// `struct usbd_bus`: what a host controller's softc starts with.
#[repr(C)]
pub struct UsbdBus {
    /* Filled by HC driver */
    /// `bdev`: base device, host adapter.
    pub bdev: Device,
    /// `methods`.
    pub methods: Cell<Option<&'static UsbdBusMethods>>,
    /// `bpfif`: the bpf tap (`usb_attach`).
    pub bpfif: Cell<Option<&'static BpfIf>>,
    /// `bpf`: the listeners' pointer the bpf code keeps up to date (`caddr_t bpf`).
    pub bpf: Cell<*mut u8>,
    /// `pipe_size`: size of a pipe struct.
    pub pipe_size: Cell<u32>,
    /// The host controller's pipe type ([`usbd_bus_set_hc_types`]).
    pub pipe_type: Cell<Option<fn() -> TypeId>>,
    /// The host controller's xfer type ([`usbd_bus_set_hc_types`]).
    pub xfer_type: Cell<Option<fn() -> TypeId>>,
    /* Filled by usb driver */
    /// `root_hub`.
    pub root_hub: Cell<Option<&'static UsbdDevice>>,
    /// `devices`: by address.
    pub devices: [Cell<Option<&'static UsbdDevice>>; USB_MAX_DEVICES],
    /// `use_polling`: a count, nonzero while polling.
    pub use_polling: Cell<i8>,
    /// `dying`.
    pub dying: Cell<i8>,
    /// `flags`: `USB_BUS_*`.
    pub flags: Cell<i32>,
    /// `usbctl`: the `usb(4)` device.
    pub usbctl: Cell<Option<NonNull<Device>>>,
    /// `stats`.
    pub stats: Cell<UsbDeviceStats>,
    /// `intr_context`.
    pub intr_context: Cell<i32>,
    /// `no_intrs`.
    pub no_intrs: Cell<u32>,
    /// `usbrev`: USB revision, `USBREV_*`.
    pub usbrev: Cell<i32>,
    /// `soft`: soft interrupt cookie.
    pub soft: Cell<Option<NonNull<SoftintrHand>>>,
    /// `dmatag`: DMA tag.
    pub dmatag: Cell<Option<BusDmaTag>>,
    /// `dmaflags`.
    pub dmaflags: Cell<i32>,
}

impl UsbdBus {
    /// `bus->methods`. Panics before the host controller set them.
    pub fn methods(&self) -> &'static UsbdBusMethods {
        match self.methods.get() {
            Some(m) => m,
            None => panic(format_args!(
                "{}: usbd_bus without methods",
                self.bdev.xname()
            )),
        }
    }

    /// `bus->dmatag`. Panics before the host controller set it.
    pub fn dmatag(&self) -> BusDmaTag {
        match self.dmatag.get() {
            Some(t) => t,
            None => panic(format_args!(
                "{}: usbd_bus without a DMA tag",
                self.bdev.xname()
            )),
        }
    }

    /// `bus->devices[addr]`, `None` outside the table.
    pub fn device(&self, addr: usize) -> Option<&'static UsbdDevice> {
        self.devices.get(addr).and_then(|d| d.get())
    }
}

// SAFETY: `#[repr(C)]` with the device first; every other member is a `Cell` of an
// `Option` of a reference, `NonNull` or `fn` (None is zero), a raw pointer, an integer, a
// structure of integers or a `bus_dma_tag_t` option: all valid as zero bits.
unsafe impl crate::sys::device::Softc for UsbdBus {}

/// `USB_BUS_CONFIG_PENDING`.
pub const USB_BUS_CONFIG_PENDING: i32 = 0x01;
/// `USB_BUS_DISCONNECTING`.
pub const USB_BUS_DISCONNECTING: i32 = 0x02;

/// `USBREV_UNKNOWN`.
pub const USBREV_UNKNOWN: i32 = 0;
/// `USBREV_PRE_1_0`.
pub const USBREV_PRE_1_0: i32 = 1;
/// `USBREV_1_0`.
pub const USBREV_1_0: i32 = 2;
/// `USBREV_1_1`.
pub const USBREV_1_1: i32 = 3;
/// `USBREV_2_0`.
pub const USBREV_2_0: i32 = 4;
/// `USBREV_3_0`.
pub const USBREV_3_0: i32 = 5;
/// `USBREV_STR`.
pub const USBREV_STR: [&str; 6] = ["unknown", "pre 1.0", "1.0", "1.1", "2.0", "3.0"];

/// `struct usbd_device`: a device on a bus.
pub struct UsbdDevice {
    /// `bus`: our controller.
    pub bus: Cell<Option<&'static UsbdBus>>,
    /// `default_pipe`: pipe 0.
    pub default_pipe: Cell<Option<&'static UsbdPipe>>,
    /// `dying`: hardware removed.
    pub dying: Cell<u8>,
    /// `ref_cnt`: # of procs using device.
    pub ref_cnt: Cell<u8>,
    /// `address`: device address.
    pub address: Cell<u8>,
    /// `config`: current configuration #.
    pub config: Cell<u8>,
    /// `depth`: distance from root hub.
    pub depth: Cell<u8>,
    /// `speed`: low/full/high speed.
    pub speed: Cell<u8>,
    /// `self_powered`: flag for self powered.
    pub self_powered: Cell<u8>,
    /// `power`: mA the device uses.
    pub power: Cell<u16>,
    /// `langid`: language for strings.
    pub langid: Cell<i16>,
    /// `powersrc`: upstream hub port, or 0.
    pub powersrc: Cell<Option<&'static UsbdPort>>,
    /// `myhub`: upstream hub.
    pub myhub: Cell<Option<&'static UsbdDevice>>,
    /// `myhsport`: closest high speed port.
    pub myhsport: Cell<Option<&'static UsbdPort>>,
    /// `def_ep`: for pipe 0.
    pub def_ep: UsbdEndpoint,
    /// `def_ep_desc`: for pipe 0.
    pub def_ep_desc: Cell<UsbEndpointDescriptor>,
    /// `ifaces`: array of all interfaces (`cdesc->bNumInterfaces` of them).
    pub ifaces: Cell<*mut UsbdInterface>,
    /// `ddesc`: device descriptor.
    pub ddesc: Cell<UsbDeviceDescriptor>,
    /// `cdesc`: full config descr (`wTotalLength` bytes).
    pub cdesc: Cell<Option<&'static [u8]>>,
    /// `quirks`: device quirks, always set.
    pub quirks: Cell<Option<&'static UsbdQuirks>>,
    /// `hub`: only if this is a hub.
    pub hub: Cell<Option<&'static UsbdHub>>,
    /// `subdevs`: sub-devices, 0 terminated (`nsubdev` slots).
    pub subdevs: Cell<*mut Option<NonNull<Device>>>,
    /// `nsubdev`: size of the `subdevs` array.
    pub nsubdev: Cell<i32>,
    /// `ndevs`: # of subdevs.
    pub ndevs: Cell<i32>,
    /// `serial`: serial number, can be NULL (`USB_MAX_STRING_LEN` bytes, NUL-terminated).
    pub serial: Cell<*mut u8>,
    /// `vendor`: vendor string, can be NULL.
    pub vendor: Cell<*mut u8>,
    /// `product`: product string, can be NULL.
    pub product: Cell<*mut u8>,
}

/// `USBD_NOLANG`.
pub const USBD_NOLANG: i16 = -1;

impl UsbdDevice {
    /// `dev->bus`. Panics on a device without one (they are made with it).
    pub fn bus(&self) -> &'static UsbdBus {
        match self.bus.get() {
            Some(b) => b,
            None => panic(format_args!("usbd_device without a bus")),
        }
    }

    /// `dev->cdesc` as its header, `None` when unconfigured.
    pub fn cdesc(&self) -> Option<&'static UsbConfigDescriptor> {
        self.cdesc
            .get()
            .and_then(|b| crate::dev::usb::usb::usb_wire_at::<UsbConfigDescriptor>(b, 0))
    }

    /// `dev->ifaces[0..cdesc->bNumInterfaces]`.
    pub fn ifaces(&self) -> &'static [UsbdInterface] {
        let p = self.ifaces.get();
        let Some(cd) = self.cdesc() else {
            return &[];
        };
        if p.is_null() {
            return &[];
        }
        // SAFETY: `usbd_set_config_index` allocated `bNumInterfaces` zeroed interfaces (valid
        // all-zero) together with `cdesc`, and frees both together.
        unsafe { slice::from_raw_parts(p, usize::from(cd.bNumInterfaces)) }
    }

    /// `dev->quirks`. Panics before `usbd_new_device` set it ("always set").
    pub fn quirks(&self) -> &'static UsbdQuirks {
        match self.quirks.get() {
            Some(q) => q,
            None => panic(format_args!("usbd_device without quirks")),
        }
    }

    /// `dev->subdevs[0..nsubdev]`.
    pub fn subdevs(&self) -> &'static [Cell<Option<NonNull<Device>>>] {
        let p = self.subdevs.get();
        let n = self.nsubdev.get();
        if p.is_null() || n <= 0 {
            return &[];
        }
        // SAFETY: `usbd_probe_and_attach` allocated `nsubdev` slots at `subdevs` and wrote
        // each before publishing it; `Cell<T>` has `T`'s layout; freed by `usb_free_device`.
        unsafe { slice::from_raw_parts(p.cast::<Cell<Option<NonNull<Device>>>>(), n as usize) }
    }

    /// A NUL-terminated string member (`serial`, `vendor`, `product`) as bytes without the
    /// NUL, `None` for NULL.
    pub fn string(p: *mut u8) -> Option<&'static [u8]> {
        if p.is_null() {
            return None;
        }
        // SAFETY: the strings are `USB_MAX_STRING_LEN`-byte allocations (usbd_cache_devinfo)
        // freed only by `usb_free_device`.
        let all = unsafe { slice::from_raw_parts(p.cast_const(), USB_MAX_STRING_LEN) };
        let len = all.iter().position(|&b| b == 0).unwrap_or(all.len());
        Some(&all[..len])
    }
}

/// `struct usbd_interface`.
pub struct UsbdInterface {
    /// `device`.
    pub device: Cell<Option<&'static UsbdDevice>>,
    /// `idesc`: inside the device's configuration descriptor.
    pub idesc: Cell<Option<&'static UsbInterfaceDescriptor>>,
    /// `index`.
    pub index: Cell<i32>,
    /// `altindex`.
    pub altindex: Cell<i32>,
    /// `endpoints`: `nendpt` of them.
    pub endpoints: Cell<*mut UsbdEndpoint>,
    /// `priv`: the attached driver's.
    pub priv_: Cell<*mut c_void>,
    /// `pipes`: the pipes open on this interface.
    pub pipes: ListHead<UsbdPipeList>,
    /// `claimed`: a driver attached to it (`usbd_claim_iface`).
    pub claimed: Cell<u8>,
    /// `nendpt`.
    pub nendpt: Cell<u8>,
}

impl UsbdInterface {
    /// `iface->device`. Panics on an interface `usbd_fill_iface_data` did not fill.
    pub fn device(&self) -> &'static UsbdDevice {
        match self.device.get() {
            Some(d) => d,
            None => panic(format_args!("usbd_interface without a device")),
        }
    }

    /// `iface->idesc`. Panics on an interface `usbd_fill_iface_data` did not fill.
    pub fn idesc(&self) -> &'static UsbInterfaceDescriptor {
        match self.idesc.get() {
            Some(d) => d,
            None => panic(format_args!("usbd_interface without a descriptor")),
        }
    }

    /// `iface->endpoints[0..nendpt]`.
    pub fn endpoints(&self) -> &'static [UsbdEndpoint] {
        let p = self.endpoints.get();
        let n = usize::from(self.nendpt.get());
        if p.is_null() || n == 0 {
            return &[];
        }
        // SAFETY: `usbd_fill_iface_data` allocated `nendpt` zeroed endpoints (valid
        // all-zero) at `endpoints`; they are freed only with the interface data.
        unsafe { slice::from_raw_parts(p, n) }
    }
}

/// `struct usbd_pipe`: what a host controller's pipe starts with.
#[repr(C)]
pub struct UsbdPipe {
    /// `iface`: `None` for a device's default pipe.
    pub iface: Cell<Option<&'static UsbdInterface>>,
    /// `device`.
    pub device: Cell<Option<&'static UsbdDevice>>,
    /// `endpoint`.
    pub endpoint: Cell<Option<&'static UsbdEndpoint>>,
    /// `pipe_size`: the allocation's size (`usbd_bus.pipe_size`).
    pub pipe_size: Cell<usize>,
    /// `running`.
    pub running: Cell<i8>,
    /// `aborting`.
    pub aborting: Cell<i8>,
    /// `queue`: the xfers queued on the pipe.
    pub queue: SimpleqHead<UsbdXferQueue>,
    /// `next`: on `usbd_interface.pipes`.
    pub next: ListEntry<UsbdPipe>,
    /// `intrxfer`: used for repeating requests.
    pub intrxfer: Cell<Option<&'static UsbdXfer>>,
    /// `repeat`.
    pub repeat: Cell<i8>,
    /// `interval`.
    pub interval: Cell<i32>,
    /* Filled by HC driver. */
    /// `methods`.
    pub methods: Cell<Option<&'static UsbdPipeMethods>>,
}

impl UsbdPipe {
    /// `pipe->device`. Panics on a pipe `usbd_setup_pipe` did not make.
    pub fn device(&self) -> &'static UsbdDevice {
        match self.device.get() {
            Some(d) => d,
            None => panic(format_args!("usbd_pipe without a device")),
        }
    }

    /// `pipe->endpoint`. Panics on a pipe `usbd_setup_pipe` did not make.
    pub fn endpoint(&self) -> &'static UsbdEndpoint {
        match self.endpoint.get() {
            Some(e) => e,
            None => panic(format_args!("usbd_pipe without an endpoint")),
        }
    }

    /// `pipe->methods`. Panics before the host controller's `open_pipe` set them.
    pub fn methods(&self) -> &'static UsbdPipeMethods {
        match self.methods.get() {
            Some(m) => m,
            None => panic(format_args!("usbd_pipe without methods")),
        }
    }

    /// `(struct xhci_pipe *)pipe`: the host controller's pipe this one heads.
    ///
    /// Panics if the bus's host controller did not register `T` as its pipe type.
    pub fn hc<T: UsbdHcPipe>(&self) -> &T {
        let bus = self.device().bus();
        let ok = bus
            .pipe_type
            .get()
            .is_some_and(|f| f() == TypeId::of::<T>())
            && self.pipe_size.get() >= size_of::<T>();
        if !ok {
            panic(format_args!(
                "usbd_pipe: not a pipe of this host controller"
            ));
        }
        // SAFETY: the bus allocates its pipes `pipe_size` bytes, zeroed, for its registered
        // pipe type (`usbd_setup_pipe`), which is `T`; `T: UsbdHcPipe` is `#[repr(C)]` with
        // the pipe first and valid all-zero.
        unsafe { &*ptr::from_ref(self).cast::<T>() }
    }
}

queue_adapter!(
    /// `LIST_ENTRY(usbd_pipe) next`: `usbd_interface.pipes`.
    pub UsbdPipeList: UsbdPipe, next => ListEntry<UsbdPipe>
);

/// `struct usbd_xfer`: what a host controller's xfer starts with.
#[repr(C)]
pub struct UsbdXfer {
    /// `pipe`.
    pub pipe: Cell<Option<&'static UsbdPipe>>,
    /// `priv`: the caller's argument to the callback.
    pub priv_: Cell<*mut c_void>,
    /// `buffer`: the caller's buffer (`usbd_setup_xfer`'s contract keeps it valid).
    pub buffer: Cell<*mut u8>,
    /// `length`.
    pub length: Cell<u32>,
    /// `actlen`.
    pub actlen: Cell<u32>,
    /// `flags`: `USBD_*` request flags.
    pub flags: Cell<u16>,
    /// `timeout`, in ms.
    pub timeout: Cell<u32>,
    /// `status`.
    pub status: Cell<UsbdStatus>,
    /// `callback`.
    pub callback: Cell<Option<UsbdCallback>>,
    /// `done` (`volatile char`): set by `usb_transfer_complete`, polled by `usbd_transfer`.
    pub done: AtomicBool,
    /// `busy_free`: `XFER_FREE` or `XFER_ONQU` (`DIAGNOSTIC`).
    #[cfg(feature = "diagnostic")]
    pub busy_free: Cell<u32>,

    /* For control pipe */
    /// `request`.
    pub request: Cell<UsbDeviceRequest>,

    /* For isoc */
    /// `frlengths`: `nframes` frame lengths, the caller's.
    pub frlengths: Cell<*mut u16>,
    /// `nframes`.
    pub nframes: Cell<i32>,

    /* For memory allocation */
    /// `device`.
    pub device: Cell<Option<&'static UsbdDevice>>,
    /// `dmabuf`.
    pub dmabuf: UsbDma,

    /// `rqflags`: `URQ_*`.
    pub rqflags: Cell<i32>,

    /// `next`: on `usbd_pipe.queue`.
    pub next: SimpleqEntry<UsbdXfer>,

    /// `hcpriv`: private use by the HC driver.
    pub hcpriv: Cell<*mut c_void>,

    /// `abort_task`.
    pub abort_task: UsbTask,
    /// `timeout_handle`.
    pub timeout_handle: Timeout,
}

/// `XFER_FREE`.
#[cfg(feature = "diagnostic")]
pub const XFER_FREE: u32 = 0x42555359;
/// `XFER_ONQU`.
#[cfg(feature = "diagnostic")]
pub const XFER_ONQU: u32 = 0x4f4e5155;

/// `URQ_REQUEST`.
pub const URQ_REQUEST: i32 = 0x01;
/// `URQ_AUTO_DMABUF`.
pub const URQ_AUTO_DMABUF: i32 = 0x10;
/// `URQ_DEV_DMABUF`.
pub const URQ_DEV_DMABUF: i32 = 0x20;

impl UsbdXfer {
    /// `xfer->pipe`. Panics before `usbd_setup_xfer`.
    pub fn pipe(&self) -> &'static UsbdPipe {
        match self.pipe.get() {
            Some(p) => p,
            None => panic(format_args!("usbd_xfer without a pipe")),
        }
    }

    /// `xfer->device`. Panics on an xfer `usbd_alloc_xfer` did not make.
    pub fn device(&self) -> &'static UsbdDevice {
        match self.device.get() {
            Some(d) => d,
            None => panic(format_args!("usbd_xfer without a device")),
        }
    }

    /// `(struct xhci_xfer *)xfer`: the host controller's xfer this one heads.
    ///
    /// Panics if the bus's host controller did not register `T` as its xfer type.
    pub fn hc<T: UsbdHcXfer>(&self) -> &T {
        let bus = self.device().bus();
        if !bus
            .xfer_type
            .get()
            .is_some_and(|f| f() == TypeId::of::<T>())
        {
            panic(format_args!(
                "usbd_xfer: not an xfer of this host controller"
            ));
        }
        // SAFETY: every xfer of this bus comes from its `allocx`, which allocates the
        // registered type `T` (`usbd_bus_set_hc_types`' contract); `T: UsbdHcXfer` is
        // `#[repr(C)]` with the xfer first.
        unsafe { &*ptr::from_ref(self).cast::<T>() }
    }
}

queue_adapter!(
    /// `SIMPLEQ_ENTRY(usbd_xfer) next`: `usbd_pipe.queue`.
    pub UsbdXferQueue: UsbdXfer, next => SimpleqEntry<UsbdXfer>
);

/// A host controller's pipe: `struct xhci_pipe { struct usbd_pipe pipe; ... }`.
///
/// # Safety
///
/// The type is `#[repr(C)]`, its first member is a [`UsbdPipe`], and the all-zero bit
/// pattern is a valid value of it (`usbd_setup_pipe` allocates with `M_ZERO`).
pub unsafe trait UsbdHcPipe: Sized + 'static {}

/// A host controller's xfer: `struct xhci_xfer { struct usbd_xfer xfer; ... }`.
///
/// # Safety
///
/// The type is `#[repr(C)]` and its first member is a [`UsbdXfer`].
pub unsafe trait UsbdHcXfer: Sized + 'static {}

/// `bus->pipe_size = sizeof(struct xhci_pipe)`: registers the host controller's pipe and xfer
/// types, so `usbd_setup_pipe` allocates pipes of `P`'s size and [`UsbdPipe::hc`] and
/// [`UsbdXfer::hc`] accept the casts.
///
/// # Safety
///
/// The bus's `allocx` returns only xfers that head an `X`.
pub unsafe fn usbd_bus_set_hc_types<P: UsbdHcPipe, X: UsbdHcXfer>(bus: &UsbdBus) {
    bus.pipe_size.set(size_of::<P>() as u32);
    bus.pipe_type.set(Some(TypeId::of::<P>));
    bus.xfer_type.set(Some(TypeId::of::<X>));
}

/// `usbd_xfer_isread`: whether the transfer moves data from the device.
#[inline]
pub fn usbd_xfer_isread(xfer: &UsbdXfer) -> bool {
    if xfer.rqflags.get() & URQ_REQUEST != 0 {
        return xfer.request.get().bmRequestType & UT_READ != 0;
    }
    xfer.pipe().endpoint().edesc().bEndpointAddress & UE_DIR_IN != 0
}

/// `USBTAP_DIR_OUT`.
pub const USBTAP_DIR_OUT: u8 = 0;
/// `USBTAP_DIR_IN`.
pub const USBTAP_DIR_IN: u8 = 1;

/// `UHUB_UNK_CONFIGURATION`.
pub const UHUB_UNK_CONFIGURATION: i32 = -1;
/// `UHUB_UNK_INTERFACE`.
pub const UHUB_UNK_INTERFACE: i32 = -1;
