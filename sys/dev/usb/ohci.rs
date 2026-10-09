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
//! ohci(4): the Open Host Controller Interface (USB 1.1) driver, machine independent
//! (`dev/usb/ohci.c`). Bus front-ends (`dev/pci/ohci_pci.rs`) map the registers, establish
//! the interrupt, call [`ohci_checkrev`], [`ohci_handover`] and [`ohci_init`] and attach
//! `usb(4)`.
//!
//! Upstream: sys/dev/usb/ohci.c @ 3ce1f3f79392
//!
//! The controller walks lists of endpoint descriptors (EDs) in DMA memory: a control list and
//! a bulk list, each headed by a skipped dummy ED, and a periodic tree of 63 dummy EDs whose
//! 32 leaves the HCCA's interrupt table points at (spread by `revbits`), ending in the
//! isochronous list's head. Every pipe has an ED; a transfer is a chain of TDs (general) or
//! ITDs (isochronous) queued between the ED's head and its tail, which is always an unused
//! sentinel descriptor (`opipe->tail`). The controller retires finished descriptors onto the
//! done queue, whose head it writes into the HCCA: the interrupt reverses that list, finding
//! each descriptor by bus address in a hash (`ohci_add_done`), and the soft interrupt
//! completes the transfers (`ohci_softintr`). The root hub is emulated
//! (`ohci_root_ctrl_start`); a port change completes its interrupt transfer from the hard
//! interrupt (`ohci_rhsc`), with the change interrupt then held off for a second.
//!
//! Locking is the C's: everything runs under the kernel lock (the front-end establishes the
//! interrupt without `IPL_MPSAFE`), at `splusb()`, and the hard interrupt's `splhardusb()`
//! sections keep the done lists consistent.
//!
//! ## Deviations
//! - `OHCI_DEBUG` is not in GENERIC: the `DPRINTF`s, `ohci_cc_strs` and the `ohci_dump*`
//!   functions are absent, as in a kernel built without it. The `DIAGNOSTIC` blocks are under
//!   feature `diagnostic`, but for `ohci_intr`'s rate-checked `DPRINTFN` while polling, whose
//!   only effect is the empty `DPRINTFN`. `ohci_free_std_chain` is under `#if 0` and is not
//!   ported.
//! - `struct pool *ohcixfer`, `malloc`ed by the first `ohci_init`, is a static [`Pool`] with a
//!   flag for the C's `ohcixfer == NULL`; its "unable to allocate pool descriptor" path cannot
//!   happen.
//! - `ohci_setaddr` returns `Err(EINVAL)` where the C returns `EINVAL` and `Err(EPERM)` where
//!   it returns the literal 1 (`EPERM`'s value). The callers only test for nonzero.
//! - The casts `(struct ohci_softc *)bus`, `(struct ohci_pipe *)pipe` are checked:
//!   [`ohci_softc`] checks the bus's methods are this driver's, the pipe cast goes through
//!   `UsbdPipe::hc`. `xfer->hcpriv` (a `void *`) holds the xfer's first TD, or first ITD on an
//!   isochronous pipe; [`hcpriv_std`] and [`hcpriv_sitd`] check the pipe's type before
//!   reading it back.
//! - `struct ohci_pipe`'s unions are split into their members (`tail_td`/`tail_itd`,
//!   `reqdma`, `nslots`/`pos`, `iso_next`/`iso_inuse`).
//! - `ohci_alloc_std_chain` returns the chain's last TD (`*ep`, possibly none) instead of
//!   filling `ep`. Where the C goes on with a pointer it never set or that is NULL, the port
//!   stops: `ohci_device_bulk_start` returns the chain's error before touching the tail (the
//!   C dereferences an uninitialised pointer first), `ohci_device_request` frees the second
//!   TD of a failed chain only if there is one (the C passes NULL to `ohci_free_std`), and a
//!   NULL the C would dereference elsewhere (a chain or ED list that ends early) is a panic.
//!   A zero `wMaxPacketSize` panics where the C divides by it.
//! - The root hub's string descriptors are built in a local `usb_string_descriptor_t` and
//!   copied, as in `ehci.rs`; the hub descriptor copies at most the structure's own size.
//! - Arming an xfer's timeout (`timeout_del`, `timeout_set`, `timeout_add_msec`) is
//!   [`ohci_arm_timeout`], shared by the start functions.

use core::cell::Cell;
use core::ffi::c_void;
use core::ptr::{self, NonNull};
use core::sync::atomic::{AtomicBool, Ordering};

use crate::dev::usb::ohcireg::*;
use crate::dev::usb::ohcivar::{
    OHCI_ADD_LEN, OHCI_CALL_DONE, OHCI_HASH_SIZE, OHCI_NO_EDS, OHCI_SED_CHUNK, OHCI_SED_SIZE,
    OHCI_SITD_CHUNK, OHCI_SITD_SIZE, OHCI_STD_CHUNK, OHCI_STD_SIZE, OhciSoftEd, OhciSoftItd,
    OhciSoftItdHash, OhciSoftTd, OhciSoftTdHash, OhciSoftc, OhciXfer, oread4, owrite4,
};
use crate::dev::usb::usb::{
    UC_BUS_POWERED, UC_SELF_POWERED, UDCLASS_HUB, UDESC_CONFIG, UDESC_DEVICE, UDESC_ENDPOINT,
    UDESC_HUB, UDESC_INTERFACE, UDESC_STRING, UDPROTO_FSHUB, UDS_SELF_POWERED, UDSUBCLASS_HUB,
    UE_BULK, UE_CONTROL, UE_DIR_IN, UE_INTERRUPT, UE_ISOCHRONOUS, UHD_PWR_GANGED,
    UHD_PWR_INDIVIDUAL, UHD_PWR_NO_SWITCH, UHD_PWRON_FACTOR, UHF_C_PORT_CONNECTION,
    UHF_C_PORT_ENABLE, UHF_C_PORT_OVER_CURRENT, UHF_C_PORT_RESET, UHF_C_PORT_SUSPEND,
    UHF_PORT_DISOWN_TO_1_1, UHF_PORT_ENABLE, UHF_PORT_POWER, UHF_PORT_RESET, UHF_PORT_SUSPEND,
    UICLASS_HUB, UIPROTO_FSHUB, UISUBCLASS_HUB, UPS_C_CONNECT_STATUS, UPS_C_OVERCURRENT_INDICATOR,
    UPS_C_PORT_ENABLED, UPS_C_PORT_RESET, UPS_C_SUSPEND, UPS_CURRENT_CONNECT_STATUS, UPS_LOW_SPEED,
    UPS_OVERCURRENT_INDICATOR, UPS_PORT_ENABLED, UPS_PORT_POWER, UPS_RESET, UPS_SUSPEND,
    UR_CLEAR_FEATURE, UR_GET_CONFIG, UR_GET_DESCRIPTOR, UR_GET_INTERFACE, UR_GET_STATUS,
    UR_SET_ADDRESS, UR_SET_CONFIG, UR_SET_DESCRIPTOR, UR_SET_FEATURE, UR_SET_INTERFACE,
    UR_SYNCH_FRAME, USB_BUS_RESET_DELAY, USB_CONFIG_DESCRIPTOR_SIZE, USB_CONTROL_ENDPOINT,
    USB_DEVICE_DESCRIPTOR_SIZE, USB_ENDPOINT_DESCRIPTOR_SIZE, USB_HUB_DESCRIPTOR_SIZE,
    USB_INTERFACE_DESCRIPTOR_SIZE, USB_MAX_DEVICES, USB_PORT_ROOT_RESET_DELAY, USB_RESUME_DELAY,
    USB_RESUME_RECOVERY, USB_RESUME_WAIT, USB_SPEED_LOW, USBD_SHORT_XFER_OK, UT_READ_CLASS_DEVICE,
    UT_READ_CLASS_OTHER, UT_READ_DEVICE, UT_READ_ENDPOINT, UT_READ_INTERFACE,
    UT_WRITE_CLASS_DEVICE, UT_WRITE_CLASS_OTHER, UT_WRITE_DEVICE, UT_WRITE_ENDPOINT,
    UT_WRITE_INTERFACE, UsbConfigDescriptor, UsbDeviceDescriptor, UsbDeviceRequest,
    UsbEndpointDescriptor, UsbHubDescriptor, UsbInterfaceDescriptor, UsbPortStatus,
    UsbStringDescriptor, UsbWire, ue_get_addr, ue_get_dir, ue_get_xfertype, ugetw, usetw,
};
use crate::dev::usb::usb::{usb_add_task, usb_rem_task, usb_schedsoftintr};
use crate::dev::usb::usb_mem::{dmaaddr, kernaddr, usb_allocmem, usb_freemem, usb_syncmem};
use crate::dev::usb::usb_subr::{usb_delay_ms, usbd_set_address};
use crate::dev::usb::usbdi::{
    IPL_SOFTUSB, USB_TASK_TYPE_ABORT, USBD_CANCELLED, USBD_DEFAULT_INTERVAL, USBD_FORCE_SHORT_XFER,
    USBD_IN_PROGRESS, USBD_INVAL, USBD_IOERROR, USBD_NOMEM, USBD_NORMAL_COMPLETION,
    USBD_NOT_STARTED, USBD_STALLED, USBD_TIMEOUT, UsbdStatus, splhardusb, splusb, usb_init_task,
    usb_insert_transfer, usb_transfer_complete, usbd_ratecheck, usbd_str,
};
use crate::dev::usb::usbdivar::{
    USB_DMA_COHERENT, USBREV_1_0, USBREV_UNKNOWN, UsbDma, UsbdBus, UsbdBusMethods, UsbdDevice,
    UsbdHcPipe, UsbdPipe, UsbdPipeMethods, UsbdXfer, usbd_bus_set_hc_types, usbd_xfer_isread,
};
use crate::kassert;
use crate::kern::kern_synch::{tsleep_nsec, wakeup};
use crate::kern::kern_timeout::{timeout_add_msec, timeout_add_sec, timeout_del, timeout_set};
use crate::kern::subr_autoconf::{config_activate_children, config_detach_children};
use crate::kern::subr_pool::{pool_get, pool_init, pool_put};
use crate::kern::subr_prf::{panic, printf};
use crate::machine::bus::{
    BUS_DMASYNC_POSTREAD, BUS_DMASYNC_POSTWRITE, BUS_DMASYNC_PREREAD, BUS_DMASYNC_PREWRITE,
};
use crate::machine::cpu::{curproc, delay};
use crate::machine::intr::{splsoftassert, splx};
use crate::sys::device::{
    CD_SKIPHIBERNATE, Cfdriver, DV_DULL, DVACT_POWERDOWN, DVACT_RESUME, DVACT_SUSPEND, Device,
};
use crate::sys::errno::Errno;
use crate::sys::param::PZERO;
use crate::sys::pool::{PR_NOWAIT, PR_ZERO, Pool};
use crate::sys::queue::ListHead;
use crate::sys::systm::INFSLP;

/// `revbits`: reverse the bits in a value 0 .. 31.
const REVBITS: [u8; OHCI_NO_INTRS] = [
    0x00, 0x10, 0x08, 0x18, 0x04, 0x14, 0x0c, 0x1c, 0x02, 0x12, 0x0a, 0x1a, 0x06, 0x16, 0x0e, 0x1e,
    0x01, 0x11, 0x09, 0x19, 0x05, 0x15, 0x0d, 0x1d, 0x03, 0x13, 0x0b, 0x1b, 0x07, 0x17, 0x0f, 0x1f,
];

/// `OHCI_INTR_ENDPT`: the root hub's interrupt endpoint.
const OHCI_INTR_ENDPT: u8 = 1;

/// `struct ohci_pipe`: a pipe of this controller.
#[repr(C)]
pub struct OhciPipe {
    /// `pipe`: the generic pipe, first.
    pub pipe: UsbdPipe,
    /// `sed`: the pipe's endpoint descriptor (none for the root hub's pipes).
    pub sed: Cell<Option<&'static OhciSoftEd>>,
    /// `tail.td`: the sentinel TD at the end of the ED's queue.
    pub tail_td: Cell<Option<&'static OhciSoftTd>>,
    /// `tail.itd`: the sentinel ITD, on an isochronous pipe.
    pub tail_itd: Cell<Option<&'static OhciSoftItd>>,
    /// `u.ctl.reqdma`: a control pipe's SETUP packet.
    pub reqdma: UsbDma,
    /// `u.intr.nslots`: the interrupt slots the ED serves.
    pub nslots: Cell<i32>,
    /// `u.intr.pos`: the interrupt tree ED it hangs from.
    pub pos: Cell<i32>,
    /// `u.iso.next`: the frame of the next isochronous transfer, -1 before the first.
    pub iso_next: Cell<i32>,
    /// `u.iso.inuse`: the frames scheduled.
    pub iso_inuse: Cell<i32>,
}

impl OhciPipe {
    /// `opipe->sed`. Panics on a pipe without one (the C's NULL dereference).
    fn sed(&self) -> &'static OhciSoftEd {
        match self.sed.get() {
            Some(s) => s,
            None => panic(format_args!("ohci: pipe without an endpoint descriptor")),
        }
    }

    /// `opipe->tail.td`. Panics on a pipe without one.
    fn tail_td(&self) -> &'static OhciSoftTd {
        match self.tail_td.get() {
            Some(t) => t,
            None => panic(format_args!("ohci: pipe without a tail TD")),
        }
    }

    /// `opipe->tail.itd`. Panics on a pipe without one.
    fn tail_itd(&self) -> &'static OhciSoftItd {
        match self.tail_itd.get() {
            Some(t) => t,
            None => panic(format_args!("ohci: pipe without a tail ITD")),
        }
    }
}

// SAFETY: `#[repr(C)]` with the `usbd_pipe` first; the other members are `Cell`s of
// `Option`s of references and of integers, and a `usb_dma` of such `Cell`s: all valid as zero
// bits (`usbd_setup_pipe`'s `M_ZERO`).
unsafe impl UsbdHcPipe for OhciPipe {}

/// `ohci_cd`.
pub static OHCI_CD: Cfdriver = Cfdriver::new(b"ohci", DV_DULL, CD_SKIPHIBERNATE);

/// `ohcixfer`: the pool of [`OhciXfer`]s.
static OHCIXFER: Pool = Pool::new();
/// `ohcixfer != NULL`: the pool is initialised.
static OHCIXFER_INIT: AtomicBool = AtomicBool::new(false);

/// `ohci_bus_methods`.
pub static OHCI_BUS_METHODS: UsbdBusMethods = UsbdBusMethods {
    open_pipe: ohci_open,
    dev_setaddr: Some(ohci_setaddr),
    soft_intr: ohci_softintr,
    do_poll: ohci_poll,
    allocx: ohci_allocx,
    freex: ohci_freex,
};

/// `ohci_root_ctrl_methods`.
static OHCI_ROOT_CTRL_METHODS: UsbdPipeMethods = UsbdPipeMethods {
    transfer: ohci_root_ctrl_transfer,
    start: ohci_root_ctrl_start,
    abort: ohci_root_ctrl_abort,
    close: ohci_root_ctrl_close,
    cleartoggle: None,
    done: ohci_root_ctrl_done,
};

/// `ohci_root_intr_methods`.
static OHCI_ROOT_INTR_METHODS: UsbdPipeMethods = UsbdPipeMethods {
    transfer: ohci_root_intr_transfer,
    start: ohci_root_intr_start,
    abort: ohci_root_intr_abort,
    close: ohci_root_intr_close,
    cleartoggle: None,
    done: ohci_root_intr_done,
};

/// `ohci_device_ctrl_methods`.
static OHCI_DEVICE_CTRL_METHODS: UsbdPipeMethods = UsbdPipeMethods {
    transfer: ohci_device_ctrl_transfer,
    start: ohci_device_ctrl_start,
    abort: ohci_device_ctrl_abort,
    close: ohci_device_ctrl_close,
    cleartoggle: None,
    done: ohci_device_ctrl_done,
};

/// `ohci_device_intr_methods`.
static OHCI_DEVICE_INTR_METHODS: UsbdPipeMethods = UsbdPipeMethods {
    transfer: ohci_device_intr_transfer,
    start: ohci_device_intr_start,
    abort: ohci_device_intr_abort,
    close: ohci_device_intr_close,
    cleartoggle: Some(ohci_device_clear_toggle),
    done: ohci_device_intr_done,
};

/// `ohci_device_bulk_methods`.
static OHCI_DEVICE_BULK_METHODS: UsbdPipeMethods = UsbdPipeMethods {
    transfer: ohci_device_bulk_transfer,
    start: ohci_device_bulk_start,
    abort: ohci_device_bulk_abort,
    close: ohci_device_bulk_close,
    cleartoggle: Some(ohci_device_clear_toggle),
    done: ohci_device_bulk_done,
};

/// `ohci_device_isoc_methods`.
static OHCI_DEVICE_ISOC_METHODS: UsbdPipeMethods = UsbdPipeMethods {
    transfer: ohci_device_isoc_transfer,
    start: ohci_device_isoc_start,
    abort: ohci_device_isoc_abort,
    close: ohci_device_isoc_close,
    cleartoggle: None,
    done: ohci_device_isoc_done,
};

/*
 * Data structures and routines to emulate the root hub.
 */
/// `ohci_devd`.
static OHCI_DEVD: UsbDeviceDescriptor = UsbDeviceDescriptor {
    bLength: USB_DEVICE_DESCRIPTOR_SIZE as u8,
    bDescriptorType: UDESC_DEVICE,   // type
    bcdUSB: [0x00, 0x01],            // USB version
    bDeviceClass: UDCLASS_HUB,       // class
    bDeviceSubClass: UDSUBCLASS_HUB, // subclass
    bDeviceProtocol: UDPROTO_FSHUB,
    bMaxPacketSize: 64, // max packet
    idVendor: [0, 0],
    idProduct: [0, 0],
    bcdDevice: [0x00, 0x01], // device id
    iManufacturer: 1,
    iProduct: 2,
    iSerialNumber: 0,      // string indices
    bNumConfigurations: 1, // # of configurations
};

/// `ohci_confd`.
static OHCI_CONFD: UsbConfigDescriptor = UsbConfigDescriptor {
    bLength: USB_CONFIG_DESCRIPTOR_SIZE as u8,
    bDescriptorType: UDESC_CONFIG,
    wTotalLength: [
        (USB_CONFIG_DESCRIPTOR_SIZE + USB_INTERFACE_DESCRIPTOR_SIZE + USB_ENDPOINT_DESCRIPTOR_SIZE)
            as u8,
        0,
    ],
    bNumInterfaces: 1,
    bConfigurationValue: 1,
    iConfiguration: 0,
    bmAttributes: UC_BUS_POWERED | UC_SELF_POWERED,
    bMaxPower: 0, // max power
};

/// `ohci_ifcd`.
static OHCI_IFCD: UsbInterfaceDescriptor = UsbInterfaceDescriptor {
    bLength: USB_INTERFACE_DESCRIPTOR_SIZE as u8,
    bDescriptorType: UDESC_INTERFACE,
    bInterfaceNumber: 0,
    bAlternateSetting: 0,
    bNumEndpoints: 1,
    bInterfaceClass: UICLASS_HUB,
    bInterfaceSubClass: UISUBCLASS_HUB,
    bInterfaceProtocol: UIPROTO_FSHUB,
    iInterface: 0,
};

/// `ohci_endpd`.
static OHCI_ENDPD: UsbEndpointDescriptor = UsbEndpointDescriptor {
    bLength: USB_ENDPOINT_DESCRIPTOR_SIZE as u8,
    bDescriptorType: UDESC_ENDPOINT,
    bEndpointAddress: UE_DIR_IN | OHCI_INTR_ENDPT,
    bmAttributes: UE_INTERRUPT,
    wMaxPacketSize: [8, 0], // max packet
    bInterval: 255,
};

/// `ohci_hubd`.
static OHCI_HUBD: UsbHubDescriptor = UsbHubDescriptor {
    bDescLength: USB_HUB_DESCRIPTOR_SIZE as u8,
    bDescriptorType: UDESC_HUB,
    bNbrPorts: 0,
    wHubCharacteristics: [0, 0],
    bPwrOn2PwrGood: 0,
    bHubContrCurrent: 0,
    DeviceRemovable: [0; 32],
};

/// `(struct ohci_softc *)bus`: the softc whose `sc_bus` this is. Panics on a bus of another
/// controller.
pub fn ohci_softc(bus: &'static UsbdBus) -> &'static OhciSoftc {
    if !bus
        .methods
        .get()
        .is_some_and(|m| ptr::eq(m, &OHCI_BUS_METHODS))
    {
        panic(format_args!("{}: not an ohci bus", bus.bdev.xname()));
    }
    // SAFETY: only `ohci_init` installs `OHCI_BUS_METHODS`, on the `usbd_bus` that heads an
    // `OhciSoftc` (`#[repr(C)]`, bus first), which lives as long as the bus.
    unsafe { &*ptr::from_ref(bus).cast::<OhciSoftc>() }
}

/// `(struct ohci_pipe *)pipe`.
fn ohci_pipe(pipe: &'static UsbdPipe) -> &'static OhciPipe {
    pipe.hc::<OhciPipe>()
}

/// `sc->sc_bus.bdev.dv_xname`.
fn devname(sc: &OhciSoftc) -> &str {
    sc.sc_bus.bdev.xname()
}

/// `SIMPLEQ_FIRST(&pipe->queue)` of a pipe the caller just queued on.
fn queue_first(pipe: &'static UsbdPipe) -> &'static UsbdXfer {
    match pipe.queue.first() {
        Some(x) => x,
        None => panic(format_args!("ohci: empty pipe queue")),
    }
}

/// Whether the xfer's pipe is isochronous (its `hcpriv` is then an ITD, otherwise a TD).
fn xfer_is_isoc(xfer: &UsbdXfer) -> bool {
    ue_get_xfertype(xfer.pipe().endpoint().edesc().bmAttributes) == UE_ISOCHRONOUS
}

/// `(struct ohci_soft_td *)xfer->hcpriv` of a control, bulk or interrupt xfer.
fn hcpriv_std(xfer: &UsbdXfer) -> Option<&'static OhciSoftTd> {
    if xfer_is_isoc(xfer) {
        panic(format_args!(
            "ohci: hcpriv of an isochronous xfer read as a TD"
        ));
    }
    // SAFETY: on a pipe that is not isochronous only this driver sets `hcpriv`, to NULL or to
    // a TD of its chunks, which are never freed; a TD is `Cell`s that stay valid on the free
    // list (`PR_ZERO` makes a fresh xfer's NULL).
    unsafe { xfer.hcpriv.get().cast::<OhciSoftTd>().as_ref() }
}

/// `(struct ohci_soft_itd *)xfer->hcpriv` of an isochronous xfer.
fn hcpriv_sitd(xfer: &UsbdXfer) -> Option<&'static OhciSoftItd> {
    if !xfer_is_isoc(xfer) {
        panic(format_args!("ohci: hcpriv of a TD xfer read as an ITD"));
    }
    // SAFETY: as in `hcpriv_std`, for the ITDs this driver hangs on isochronous xfers.
    unsafe { xfer.hcpriv.get().cast::<OhciSoftItd>().as_ref() }
}

/// `xfer->hcpriv = std`.
fn set_hcpriv_std(xfer: &UsbdXfer, std: &'static OhciSoftTd) {
    xfer.hcpriv.set(ptr::from_ref(std).cast_mut().cast());
}

/// `xfer->hcpriv = sitd`.
fn set_hcpriv_sitd(xfer: &UsbdXfer, sitd: &'static OhciSoftItd) {
    xfer.hcpriv.set(ptr::from_ref(sitd).cast_mut().cast());
}

/// `p->xfer == xfer`.
fn same_xfer(p: Option<&'static UsbdXfer>, xfer: &UsbdXfer) -> bool {
    p.is_some_and(|x| ptr::eq(x, xfer))
}

/// The xfer's DMA buffer, `len` bytes of it at most (`KERNADDR(&xfer->dmabuf, 0)`).
///
/// # Safety
///
/// The xfer is in the controller's hands (between its `start` and `usb_transfer_complete`),
/// so `usbd_transfer` gave it a DMA buffer of at least `length` bytes that nothing else
/// reads or writes while the returned slice lives.
unsafe fn xfer_buf<'a>(xfer: &UsbdXfer, len: usize) -> &'a mut [u8] {
    let len = len.min(xfer.length.get() as usize);
    if len == 0 {
        return &mut [];
    }
    // SAFETY: the caller's contract: `len` bytes of the xfer's own DMA buffer, unaliased.
    unsafe { core::slice::from_raw_parts_mut(kernaddr(&xfer.dmabuf, 0), len) }
}

/// `&xfer->frlengths[i]`, checked against `nframes`.
fn frlength_ptr(xfer: &UsbdXfer, i: i32) -> *mut u16 {
    let base = xfer.frlengths.get();
    if base.is_null() || i < 0 || i >= xfer.nframes.get() {
        panic(format_args!("ohci: frame {i} outside the xfer"));
    }
    base.wrapping_add(i as usize)
}

/// `xfer->frlengths[i]`.
fn frlength(xfer: &UsbdXfer, i: i32) -> u16 {
    let p = frlength_ptr(xfer, i);
    // SAFETY: `frlength_ptr` checked the index against `nframes`; `usbd_setup_isoc_xfer`'s
    // contract keeps the array valid until the transfer completes.
    unsafe { p.read() }
}

/// `xfer->frlengths[i] = v`.
fn set_frlength(xfer: &UsbdXfer, i: i32, v: u16) {
    let p = frlength_ptr(xfer, i);
    // SAFETY: as for `frlength`.
    unsafe { p.write(v) }
}

/// `usb_syncmem(&xfer->dmabuf, 0, len, read ? POSTREAD : POSTWRITE)` (or the `PRE`
/// operations): the data buffer before or after the controller moved it.
fn xfer_sync(xfer: &UsbdXfer, len: u32, post: bool) {
    let rd = usbd_xfer_isread(xfer);
    let ops = match (post, rd) {
        (false, true) => BUS_DMASYNC_PREREAD,
        (false, false) => BUS_DMASYNC_PREWRITE,
        (true, true) => BUS_DMASYNC_POSTREAD,
        (true, false) => BUS_DMASYNC_POSTWRITE,
    };
    usb_syncmem(&xfer.dmabuf, 0, len as usize, ops);
}

/// `memset(&ed, 0, sizeof(struct ohci_ed))`.
fn ed_clear(ed: &OhciEd) {
    ohci_set(&ed.ed_flags, 0);
    ohci_set(&ed.ed_tailp, 0);
    ohci_set(&ed.ed_headp, 0);
    ohci_set(&ed.ed_nexted, 0);
}

/// `memset(&td, 0, sizeof(struct ohci_td))`.
fn td_clear(td: &OhciTd) {
    ohci_set(&td.td_flags, 0);
    ohci_set(&td.td_cbp, 0);
    ohci_set(&td.td_nexttd, 0);
    ohci_set(&td.td_be, 0);
}

/// `memset(&itd, 0, sizeof(struct ohci_itd))`.
fn itd_clear(itd: &OhciItd) {
    ohci_set(&itd.itd_flags, 0);
    ohci_set(&itd.itd_bp0, 0);
    ohci_set(&itd.itd_nextitd, 0);
    ohci_set(&itd.itd_be, 0);
    for o in &itd.itd_offset {
        ohci_set16(o, 0);
    }
}

/// `x |= htole32(bits)` on a descriptor member.
fn ohci_or(c: &Cell<u32>, bits: u32) {
    ohci_set(c, ohci_get(c) | bits);
}

/// `x &= htole32(~bits)` on a descriptor member.
fn ohci_clr(c: &Cell<u32>, bits: u32) {
    ohci_set(c, ohci_get(c) & !bits);
}

/// The software descriptor at byte `offs` of a fresh chunk, zeroed: `KERNADDR(&dma, offs)`.
///
/// # Safety
///
/// `dma` is a chunk `usb_allocmem` just returned, aligned to at least `T`'s alignment and at
/// least `offs + size_of::<T>()` bytes; `offs` is a multiple of `T`'s alignment; nobody else
/// uses the bytes; and every member of `T` is valid as zero bits. Chunks are never freed, so
/// the descriptor lives forever.
unsafe fn ohci_carve<T>(dma: &UsbDma, offs: usize) -> &'static T {
    let p = kernaddr(dma, offs).cast::<T>();
    // SAFETY: the caller's contract: the bytes are this chunk's, unused, aligned, and zero is
    // a valid `T`.
    unsafe {
        ptr::write_bytes(p, 0, 1);
        &*p
    }
}

/// `a % mps`, `wMaxPacketSize` being the divisor; panics on a zero size, where the C's
/// division traps.
fn mps_rem(a: u32, mps: u32) -> u32 {
    match a.checked_rem(mps) {
        Some(r) => r,
        None => panic(format_args!("ohci: endpoint with a zero max packet size")),
    }
}

/// `ohci_activate`: suspend puts the controller in its suspend state (saving the registers
/// a BIOS may lose); resume reprograms them and brings it back; power-down resets it after
/// the children.
pub fn ohci_activate(self_: &Device, act: i32) -> Result<(), Errno> {
    // SAFETY: the front-ends' softcs start with the `OhciSoftc` (`#[repr(C)]`), made for
    // their attachment.
    let sc = unsafe { self_.softc::<OhciSoftc>() };

    match act {
        DVACT_SUSPEND => {
            let rv = config_activate_children(self_, act);
            sc.sc_bus.use_polling.set(sc.sc_bus.use_polling.get() + 1);
            let mut reg = oread4(sc, OHCI_CONTROL) & !OHCI_HCFS_MASK;
            if sc.sc_control.get() == 0 {
                // Preserve register values, in case that APM BIOS does not recover them.
                sc.sc_control.set(reg);
                sc.sc_intre.set(oread4(sc, OHCI_INTERRUPT_ENABLE));
                sc.sc_ival.set(ohci_get_ival(oread4(sc, OHCI_FM_INTERVAL)));
            }
            reg |= OHCI_HCFS_SUSPEND;
            owrite4(sc, OHCI_CONTROL, reg);
            usb_delay_ms(&sc.sc_bus, USB_RESUME_WAIT);
            sc.sc_bus.use_polling.set(sc.sc_bus.use_polling.get() - 1);
            rv
        }
        DVACT_RESUME => {
            sc.sc_bus.use_polling.set(sc.sc_bus.use_polling.get() + 1);

            // Some broken BIOSes do not recover these values
            owrite4(sc, OHCI_HCCA, dmaaddr(&sc.sc_hccadma, 0) as u32);
            owrite4(sc, OHCI_CONTROL_HEAD_ED, sc.ctrl_head().physaddr.get());
            owrite4(sc, OHCI_BULK_HEAD_ED, sc.bulk_head().physaddr.get());
            if sc.sc_intre.get() != 0 {
                owrite4(
                    sc,
                    OHCI_INTERRUPT_ENABLE,
                    sc.sc_intre.get() & (OHCI_ALL_INTRS | OHCI_MIE),
                );
            }
            let mut reg = if sc.sc_control.get() != 0 {
                sc.sc_control.get()
            } else {
                oread4(sc, OHCI_CONTROL)
            };
            reg |= OHCI_HCFS_RESUME;
            owrite4(sc, OHCI_CONTROL, reg);
            usb_delay_ms(&sc.sc_bus, USB_RESUME_DELAY);
            reg = (reg & !OHCI_HCFS_MASK) | OHCI_HCFS_OPERATIONAL;
            owrite4(sc, OHCI_CONTROL, reg);

            let ival = sc.sc_ival.get();
            let mut reg = (oread4(sc, OHCI_FM_REMAINING) & OHCI_FIT) ^ OHCI_FIT;
            reg |= ohci_fsmps(ival) | ival;
            owrite4(sc, OHCI_FM_INTERVAL, reg);
            owrite4(sc, OHCI_PERIODIC_START, ohci_periodic(ival));

            // Fiddle the No OverCurrent Protection to avoid a chip bug
            let reg = oread4(sc, OHCI_RH_DESCRIPTOR_A);
            owrite4(sc, OHCI_RH_DESCRIPTOR_A, reg | OHCI_NOCP);
            owrite4(sc, OHCI_RH_STATUS, OHCI_LPSC); // Enable port power
            usb_delay_ms(&sc.sc_bus, OHCI_ENABLE_POWER_DELAY);
            owrite4(sc, OHCI_RH_DESCRIPTOR_A, reg);

            usb_delay_ms(&sc.sc_bus, USB_RESUME_RECOVERY);
            sc.sc_control.set(0);
            sc.sc_intre.set(0);
            sc.sc_ival.set(0);
            sc.sc_bus.use_polling.set(sc.sc_bus.use_polling.get() - 1);
            config_activate_children(self_, act)
        }
        DVACT_POWERDOWN => {
            let rv = config_activate_children(self_, act);
            owrite4(sc, OHCI_CONTROL, OHCI_HCFS_RESET);
            rv
        }
        _ => config_activate_children(self_, act),
    }
}

/// `ohci_detach`: detaches `usb(4)` and stops the root hub's change timeout.
pub fn ohci_detach(self_: &Device, flags: i32) -> Result<(), Errno> {
    // SAFETY: as in `ohci_activate`.
    let sc = unsafe { self_.softc::<OhciSoftc>() };

    config_detach_children(self_, flags)?;

    timeout_del(&sc.sc_tmo_rhsc);

    usb_delay_ms(&sc.sc_bus, 300); // XXX let stray task complete

    // free data structures XXX

    Ok(())
}

/// `ohci_alloc_sed`: a cleared ED from the free list, carving a new chunk when it is empty.
pub fn ohci_alloc_sed(sc: &'static OhciSoftc) -> Option<&'static OhciSoftEd> {
    let s = splusb();
    if sc.sc_freeeds.get().is_none() {
        let dma = UsbDma {
            block: Cell::new(None),
            offs: Cell::new(0),
        };
        let err = usb_allocmem(
            &sc.sc_bus,
            OHCI_SED_SIZE * OHCI_SED_CHUNK,
            OHCI_ED_ALIGN,
            USB_DMA_COHERENT,
            &dma,
        );
        if err.is_err() {
            splx(s);
            return None;
        }
        for i in 0..OHCI_SED_CHUNK {
            let offs = i * OHCI_SED_SIZE;
            // SAFETY: the chunk is `OHCI_SED_SIZE * OHCI_SED_CHUNK` fresh bytes of a block of
            // its own (page aligned, see the check at the end of the file), `offs` a multiple
            // of `OHCI_SED_SIZE` (itself a multiple of the structure's alignment); a soft ED
            // is `Cell`s only, valid as zero bits.
            let sed: &'static OhciSoftEd = unsafe { ohci_carve(&dma, offs) };
            sed.physaddr.set(dmaaddr(&dma, offs) as u32);
            sed.next.set(sc.sc_freeeds.get());
            sc.sc_freeeds.set(Some(sed));
        }
    }
    let sed = sc.sc_freeeds.get()?;
    sc.sc_freeeds.set(sed.next.get());
    ed_clear(&sed.ed);
    sed.next.set(None);

    splx(s);
    Some(sed)
}

/// `ohci_free_sed`.
pub fn ohci_free_sed(sc: &'static OhciSoftc, sed: &'static OhciSoftEd) {
    let s = splusb();
    sed.next.set(sc.sc_freeeds.get());
    sc.sc_freeeds.set(Some(sed));
    splx(s);
}

/// `ohci_alloc_std`: a cleared TD from the free list, carving a new chunk when it is empty;
/// it goes into the hash, where the done queue's bus addresses are looked up.
pub fn ohci_alloc_std(sc: &'static OhciSoftc) -> Option<&'static OhciSoftTd> {
    let s = splusb();
    if sc.sc_freetds.get().is_none() {
        let dma = UsbDma {
            block: Cell::new(None),
            offs: Cell::new(0),
        };
        let err = usb_allocmem(
            &sc.sc_bus,
            OHCI_STD_SIZE * OHCI_STD_CHUNK,
            OHCI_TD_ALIGN,
            USB_DMA_COHERENT,
            &dma,
        );
        if err.is_err() {
            splx(s);
            return None;
        }
        for i in 0..OHCI_STD_CHUNK {
            let offs = i * OHCI_STD_SIZE;
            // SAFETY: as in `ohci_alloc_sed`, for soft TDs (`Cell`s and an unlinked list
            // entry, valid as zero bits).
            let std: &'static OhciSoftTd = unsafe { ohci_carve(&dma, offs) };
            std.physaddr.set(dmaaddr(&dma, offs) as u32);
            std.nexttd.set(sc.sc_freetds.get());
            sc.sc_freetds.set(Some(std));
        }
    }

    let std = sc.sc_freetds.get()?;
    sc.sc_freetds.set(std.nexttd.get());
    td_clear(&std.td);
    std.nexttd.set(None);
    std.xfer.set(None);
    ohci_hash_add_td(sc, std);

    splx(s);
    Some(std)
}

/// `ohci_free_std`: takes the TD out of the hash and puts it on the free list.
pub fn ohci_free_std(sc: &'static OhciSoftc, std: &'static OhciSoftTd) {
    let s = splusb();
    // SAFETY: a TD handed out by `ohci_alloc_std` is on its hash list until it is freed,
    // which happens once; TDs live forever; at splusb() under the kernel lock.
    unsafe { ListHead::<OhciSoftTdHash>::remove(std) };
    std.nexttd.set(sc.sc_freetds.get());
    sc.sc_freetds.set(Some(std));
    splx(s);
}

/// `ohci_alloc_std_chain`: fills `sp` and as many new TDs as needed to move `alen` bytes of
/// the xfer's buffer (each TD crossing at most one page), plus a zero length TD when a write
/// must end with a short packet; returns the last TD filled (none for an empty read). The TD
/// after it is a fresh one, the pipe's next tail.
pub fn ohci_alloc_std_chain(
    sc: &'static OhciSoftc,
    alen: u32,
    xfer: &'static UsbdXfer,
    sp: &'static OhciSoftTd,
) -> Result<Option<&'static OhciSoftTd>, UsbdStatus> {
    let rd = usbd_xfer_isread(xfer);
    let dma = &xfer.dmabuf;
    let flags = xfer.flags.get();

    xfer_sync(xfer, xfer.length.get(), false);

    let mut len = alen;
    let mut cur = sp;
    let mut end = None;

    let mut dataphys = dmaaddr(dma, 0) as u32;
    let dataphysend = ohci_page(dataphys.wrapping_add(len).wrapping_sub(1));
    let tdflags = (if rd { OHCI_TD_IN } else { OHCI_TD_OUT })
        | (if flags & USBD_SHORT_XFER_OK != 0 {
            OHCI_TD_R
        } else {
            0
        })
        | OHCI_TD_NOCC
        | OHCI_TD_TOGGLE_CARRY
        | OHCI_TD_NOINTR;
    let mps = u32::from(ugetw(xfer.pipe().endpoint().edesc().wMaxPacketSize));

    while len > 0 {
        let Some(next) = ohci_alloc_std(sc) else {
            // XXX free chain
            return Err(USBD_NOMEM);
        };

        // The OHCI hardware can handle at most one page crossing.
        let curlen = if ohci_page(dataphys) == dataphysend
            || ohci_page(dataphys).wrapping_add(OHCI_PAGE_SIZE) == dataphysend
        {
            // we can handle it in this TD
            len
        } else {
            // must use multiple TDs, fill as much as possible.
            let mut curlen = 2 * OHCI_PAGE_SIZE - (dataphys & (OHCI_PAGE_SIZE - 1));
            // the length must be a multiple of the max size
            curlen -= mps_rem(curlen, mps);
            #[cfg(feature = "diagnostic")]
            if curlen == 0 {
                panic(format_args!("ohci_alloc_std: curlen == 0"));
            }
            curlen
        };
        len = len.wrapping_sub(curlen);

        ohci_set(&cur.td.td_flags, tdflags);
        ohci_set(&cur.td.td_cbp, dataphys);
        cur.nexttd.set(Some(next));
        ohci_set(&cur.td.td_nexttd, next.physaddr.get());
        ohci_set(&cur.td.td_be, dataphys.wrapping_add(curlen).wrapping_sub(1));
        cur.len.set(curlen as u16);
        cur.flags.set(OHCI_ADD_LEN);
        cur.xfer.set(Some(xfer));
        dataphys = dataphys.wrapping_add(curlen);
        end = Some(cur);
        cur = next;
    }
    if !rd && (flags & USBD_FORCE_SHORT_XFER != 0 || alen == 0) && mps_rem(alen, mps) == 0 {
        // Force a 0 length transfer at the end.

        let Some(next) = ohci_alloc_std(sc) else {
            return Err(USBD_NOMEM);
        };

        ohci_set(&cur.td.td_flags, tdflags);
        ohci_set(&cur.td.td_cbp, 0); // indicate 0 length packet
        cur.nexttd.set(Some(next));
        ohci_set(&cur.td.td_nexttd, next.physaddr.get());
        ohci_set(&cur.td.td_be, !0);
        cur.len.set(0);
        cur.flags.set(0);
        cur.xfer.set(Some(xfer));
        end = Some(cur);
    }

    Ok(end)
}

/// `ohci_alloc_sitd`: a cleared ITD from the free list, carving a new chunk when it is empty;
/// it goes into the hash.
pub fn ohci_alloc_sitd(sc: &'static OhciSoftc) -> Option<&'static OhciSoftItd> {
    if sc.sc_freeitds.get().is_none() {
        let dma = UsbDma {
            block: Cell::new(None),
            offs: Cell::new(0),
        };
        let err = usb_allocmem(
            &sc.sc_bus,
            OHCI_SITD_SIZE * OHCI_SITD_CHUNK,
            OHCI_ITD_ALIGN,
            USB_DMA_COHERENT,
            &dma,
        );
        if err.is_err() {
            return None;
        }
        let s = splusb();
        for i in 0..OHCI_SITD_CHUNK {
            let offs = i * OHCI_SITD_SIZE;
            // SAFETY: as in `ohci_alloc_sed`, for soft ITDs.
            let sitd: &'static OhciSoftItd = unsafe { ohci_carve(&dma, offs) };
            sitd.physaddr.set(dmaaddr(&dma, offs) as u32);
            sitd.nextitd.set(sc.sc_freeitds.get());
            sc.sc_freeitds.set(Some(sitd));
        }
        splx(s);
    }

    let s = splusb();
    let Some(sitd) = sc.sc_freeitds.get() else {
        splx(s);
        return None;
    };
    sc.sc_freeitds.set(sitd.nextitd.get());
    itd_clear(&sitd.itd);
    sitd.nextitd.set(None);
    sitd.xfer.set(None);
    ohci_hash_add_itd(sc, sitd);
    splx(s);

    #[cfg(feature = "diagnostic")]
    sitd.isdone.set(0);

    Some(sitd)
}

/// `ohci_free_sitd`: takes the ITD out of the hash and puts it on the free list.
pub fn ohci_free_sitd(sc: &'static OhciSoftc, sitd: &'static OhciSoftItd) {
    #[cfg(feature = "diagnostic")]
    {
        if sitd.isdone.get() == 0 {
            panic(format_args!(
                "ohci_free_sitd: sitd={:p} not done",
                ptr::from_ref(sitd)
            ));
        }
        // Warn double free
        sitd.isdone.set(0);
    }

    let s = splusb();
    ohci_hash_rem_itd(sc, sitd);
    sitd.nextitd.set(sc.sc_freeitds.get());
    sc.sc_freeitds.set(Some(sitd));
    splx(s);
}

/// `ohci_checkrev`: prints the controller's OHCI revision (ending the attach line) and
/// accepts only 1.0.
pub fn ohci_checkrev(sc: &OhciSoftc) -> UsbdStatus {
    let rev = oread4(sc, OHCI_REVISION);
    printf(format_args!(
        "version {}.{}{}\n",
        ohci_rev_hi(rev),
        ohci_rev_lo(rev),
        if ohci_rev_legacy(rev) != 0 {
            ", legacy support"
        } else {
            ""
        }
    ));

    if ohci_rev_hi(rev) != 1 || ohci_rev_lo(rev) != 0 {
        printf(format_args!("{}: unsupported OHCI revision\n", devname(sc)));
        sc.sc_bus.usbrev.set(USBREV_UNKNOWN);
        return USBD_INVAL;
    }
    sc.sc_bus.usbrev.set(USBREV_1_0);

    USBD_NORMAL_COMPLETION
}

/// `ohci_handover`: asks the SMM (BIOS) to give up the controller if it routes its
/// interrupts.
pub fn ohci_handover(sc: &OhciSoftc) -> UsbdStatus {
    let mut ctl = oread4(sc, OHCI_CONTROL);
    if ctl & OHCI_IR != 0 {
        // SMM active, request change
        if sc.sc_intre.get() & (OHCI_OC | OHCI_MIE) == (OHCI_OC | OHCI_MIE) {
            owrite4(sc, OHCI_INTERRUPT_ENABLE, OHCI_MIE);
        }
        let s = oread4(sc, OHCI_COMMAND_STATUS);
        owrite4(sc, OHCI_COMMAND_STATUS, s | OHCI_OCR);
        let mut i = 0;
        while i < 100 && ctl & OHCI_IR != 0 {
            usb_delay_ms(&sc.sc_bus, 1);
            ctl = oread4(sc, OHCI_CONTROL);
            i += 1;
        }
        owrite4(sc, OHCI_INTERRUPT_DISABLE, OHCI_MIE);
        if ctl & OHCI_IR != 0 {
            printf(format_args!(
                "{}: SMM does not respond, will reset\n",
                devname(sc)
            ));
        }
    }

    USBD_NORMAL_COMPLETION
}

/// `ohci_init`: builds the HCCA, the list heads and the interrupt ED tree, resets the
/// controller, programs it, makes it operational, powers the ports and enables interrupts.
pub fn ohci_init(sc: &'static OhciSoftc) -> UsbdStatus {
    for h in &sc.sc_hash_tds {
        h.init();
    }
    for h in &sc.sc_hash_itds {
        h.init();
    }

    if !OHCIXFER_INIT.load(Ordering::Relaxed) {
        pool_init(
            &OHCIXFER,
            size_of::<OhciXfer>(),
            0,
            IPL_SOFTUSB,
            0,
            "ohcixfer",
            None,
        );
        OHCIXFER_INIT.store(true, Ordering::Relaxed);
    }

    // XXX determine alignment by R/W
    // Allocate the HCCA area.
    let err = usb_allocmem(
        &sc.sc_bus,
        OHCI_HCCA_SIZE,
        OHCI_HCCA_ALIGN,
        USB_DMA_COHERENT,
        &sc.sc_hccadma,
    );
    if err.is_err() {
        return err;
    }
    // SAFETY: `usb_allocmem` returned `OHCI_HCCA_SIZE` bytes aligned to `OHCI_HCCA_ALIGN`
    // (more than the structure's size and alignment, see `ohcireg.rs`), cleared here, which
    // is a valid `OhciHcca` of `Cell`s; it stays allocated for the controller's life (only
    // the failure paths below free it, after clearing `sc_hcca`).
    let hcca: &'static OhciHcca = unsafe {
        let p = kernaddr(&sc.sc_hccadma, 0);
        ptr::write_bytes(p, 0, OHCI_HCCA_SIZE);
        &*p.cast::<OhciHcca>()
    };
    sc.sc_hcca.set(Some(hcca));

    sc.sc_eintrs.set(OHCI_NORMAL_INTRS);

    // The C's `goto bad1` .. `bad5`: what was allocated is given back in reverse order.
    let bad1 = |err: UsbdStatus| -> UsbdStatus {
        sc.sc_hcca.set(None);
        usb_freemem(&sc.sc_bus, &sc.sc_hccadma);
        err
    };

    // Allocate dummy ED that starts the control list.
    let Some(ctrl) = ohci_alloc_sed(sc) else {
        return bad1(USBD_NOMEM);
    };
    sc.sc_ctrl_head.set(Some(ctrl));
    ohci_or(&ctrl.ed.ed_flags, OHCI_ED_SKIP);

    let bad2 = |err: UsbdStatus| -> UsbdStatus {
        ohci_free_sed(sc, ctrl);
        bad1(err)
    };

    // Allocate dummy ED that starts the bulk list.
    let Some(bulk) = ohci_alloc_sed(sc) else {
        return bad2(USBD_NOMEM);
    };
    sc.sc_bulk_head.set(Some(bulk));
    ohci_or(&bulk.ed.ed_flags, OHCI_ED_SKIP);

    let bad3 = |err: UsbdStatus| -> UsbdStatus {
        ohci_free_sed(sc, bulk);
        bad2(err)
    };

    // Allocate dummy ED that starts the isochronous list.
    let Some(isoc) = ohci_alloc_sed(sc) else {
        return bad3(USBD_NOMEM);
    };
    sc.sc_isoc_head.set(Some(isoc));
    ohci_or(&isoc.ed.ed_flags, OHCI_ED_SKIP);

    let bad4 = |err: UsbdStatus| -> UsbdStatus {
        ohci_free_sed(sc, isoc);
        bad3(err)
    };

    // Allocate all the dummy EDs that make up the interrupt tree.
    for i in 0..OHCI_NO_EDS {
        let Some(sed) = ohci_alloc_sed(sc) else {
            for j in (0..i).rev() {
                ohci_free_sed(sc, sc.ed(j));
            }
            return bad4(USBD_NOMEM);
        };
        // All ED fields are set to 0.
        sc.sc_eds[i].set(Some(sed));
        ohci_or(&sed.ed.ed_flags, OHCI_ED_SKIP);
        let psed = if i != 0 { sc.ed((i - 1) / 2) } else { isoc };
        sed.next.set(Some(psed));
        ohci_set(&sed.ed.ed_nexted, psed.physaddr.get());
    }
    // Fill HCCA interrupt table. The bit reversal is to get the tree set up properly to
    // spread the interrupts.
    for (i, &r) in REVBITS.iter().enumerate() {
        ohci_set(
            &hcca.hcca_interrupt_table[usize::from(r)],
            sc.ed(OHCI_NO_EDS - OHCI_NO_INTRS + i).physaddr.get(),
        );
    }

    let bad5 = |err: UsbdStatus| -> UsbdStatus {
        for i in 0..OHCI_NO_EDS {
            ohci_free_sed(sc, sc.ed(i));
        }
        bad4(err)
    };

    // Preserve values programmed by SMM/BIOS but lost over reset.
    let ctl = oread4(sc, OHCI_CONTROL);
    let rwc = ctl & OHCI_RWC;
    let fm = oread4(sc, OHCI_FM_INTERVAL);
    let desca = oread4(sc, OHCI_RH_DESCRIPTOR_A);
    let descb = oread4(sc, OHCI_RH_DESCRIPTOR_B);

    // Determine in what context we are running.
    if ctl & OHCI_IR != 0 {
        owrite4(sc, OHCI_CONTROL, OHCI_HCFS_RESET | rwc);
    }
    // The C's `#if 0` reuse of a BIOS-started controller is not built: either way the
    // controller was (or now is) cold started.
    usb_delay_ms(&sc.sc_bus, USB_BUS_RESET_DELAY);

    // This reset should not be necessary according to the OHCI spec, but without it some
    // controllers do not start.
    owrite4(sc, OHCI_CONTROL, OHCI_HCFS_RESET | rwc);
    usb_delay_ms(&sc.sc_bus, USB_BUS_RESET_DELAY);

    // We now own the host controller and the bus has been reset.

    owrite4(sc, OHCI_COMMAND_STATUS, OHCI_HCR); // Reset HC
    // Nominal time for a reset is 10 us.
    let mut hcr = 0;
    for _ in 0..10 {
        delay(10);
        hcr = oread4(sc, OHCI_COMMAND_STATUS) & OHCI_HCR;
        if hcr == 0 {
            break;
        }
    }
    if hcr != 0 {
        printf(format_args!("{}: reset timeout\n", devname(sc)));
        return bad5(USBD_IOERROR);
    }

    // The controller is now in SUSPEND state, we have 2ms to finish.

    // Set up HC registers.
    owrite4(sc, OHCI_HCCA, dmaaddr(&sc.sc_hccadma, 0) as u32);
    owrite4(sc, OHCI_CONTROL_HEAD_ED, ctrl.physaddr.get());
    owrite4(sc, OHCI_BULK_HEAD_ED, bulk.physaddr.get());
    // disable all interrupts and then switch on all desired interrupts
    owrite4(sc, OHCI_INTERRUPT_DISABLE, OHCI_ALL_INTRS);
    // switch on desired functional features
    let mut ctl = oread4(sc, OHCI_CONTROL);
    ctl &= !(OHCI_CBSR_MASK | OHCI_LES | OHCI_HCFS_MASK | OHCI_IR);
    ctl |= OHCI_PLE | OHCI_IE | OHCI_CLE | OHCI_BLE | OHCI_RATIO_1_4 | OHCI_HCFS_OPERATIONAL | rwc;
    // And finally start it!
    owrite4(sc, OHCI_CONTROL, ctl);

    // The controller is now OPERATIONAL. Set a some final registers that should be set
    // earlier, but that the controller ignores when in the SUSPEND state.
    let ival = ohci_get_ival(fm);
    let mut fm = (oread4(sc, OHCI_FM_REMAINING) & OHCI_FIT) ^ OHCI_FIT;
    fm |= ohci_fsmps(ival) | ival;
    owrite4(sc, OHCI_FM_INTERVAL, fm);
    let per = ohci_periodic(ival); // 90% periodic
    owrite4(sc, OHCI_PERIODIC_START, per);

    // Fiddle the No OverCurrent Protection bit to avoid chip bug.
    owrite4(sc, OHCI_RH_DESCRIPTOR_A, desca | OHCI_NOCP);
    owrite4(sc, OHCI_RH_STATUS, OHCI_LPSC); // Enable port power
    usb_delay_ms(&sc.sc_bus, OHCI_ENABLE_POWER_DELAY);
    owrite4(sc, OHCI_RH_DESCRIPTOR_A, desca);
    owrite4(sc, OHCI_RH_DESCRIPTOR_B, descb);
    usb_delay_ms(&sc.sc_bus, ohci_get_potpgt(desca) * UHD_PWRON_FACTOR as u32);

    // The AMD756 requires a delay before re-reading the register, otherwise it will
    // occasionally report 0 ports.
    sc.sc_noport.set(0);
    let mut i = 0;
    while i < 10 && sc.sc_noport.get() == 0 {
        usb_delay_ms(&sc.sc_bus, OHCI_READ_DESC_DELAY);
        sc.sc_noport
            .set(ohci_get_ndp(oread4(sc, OHCI_RH_DESCRIPTOR_A)) as i32);
        i += 1;
    }

    // Set up the bus struct.
    sc.sc_bus.methods.set(Some(&OHCI_BUS_METHODS));
    // SAFETY: `ohci_allocx`, this bus's `allocx`, returns only xfers that head an `OhciXfer`.
    unsafe { usbd_bus_set_hc_types::<OhciPipe, OhciXfer>(&sc.sc_bus) };

    sc.sc_control.set(0);
    sc.sc_intre.set(0);

    timeout_set(
        &sc.sc_tmo_rhsc,
        ohci_rhsc_enable,
        ptr::from_ref(sc).cast_mut().cast(),
    );

    // Finally, turn on interrupts.
    owrite4(sc, OHCI_INTERRUPT_ENABLE, sc.sc_eintrs.get() | OHCI_MIE);

    USBD_NORMAL_COMPLETION
}

/// `ohci_allocx`: the bus's `allocx`, a zeroed [`OhciXfer`].
pub fn ohci_allocx(_bus: &'static UsbdBus) -> Option<&'static UsbdXfer> {
    let p = pool_get(&OHCIXFER, PR_NOWAIT | PR_ZERO)?;
    // SAFETY: the pool's items are `size_of::<OhciXfer>()` bytes at the pool's alignment (at
    // least the type's, see the check at the end of the file), zeroed (`PR_ZERO`), which is a
    // valid `OhciXfer`; it lives until `ohci_freex`.
    let ox: &'static OhciXfer = unsafe { &*p.as_ptr().cast::<OhciXfer>() };
    Some(&ox.xfer)
}

/// `ohci_freex`: the bus's `freex`.
///
/// # Safety
///
/// `xfer` came from [`ohci_allocx`] and nothing uses it any more.
pub unsafe fn ohci_freex(_bus: &'static UsbdBus, xfer: NonNull<UsbdXfer>) {
    // The xfer heads its `OhciXfer` (`#[repr(C)]`), the pool item.
    pool_put(&OHCIXFER, xfer.cast());
}

/// `ohci_intr`: the hard interrupt handler; `p` is the `OhciSoftc`. Established without
/// `IPL_MPSAFE`, so it runs with the kernel lock.
pub fn ohci_intr(p: *mut c_void) -> i32 {
    if p.is_null() {
        return 0;
    }
    // SAFETY: the front-end establishes the interrupt with its softc's `OhciSoftc` as the
    // argument and disestablishes it before the softc goes away.
    let sc: &'static OhciSoftc = unsafe { &*p.cast::<OhciSoftc>() };

    if sc.sc_bus.dying.get() != 0 {
        return 0;
    }

    // If we get an interrupt while polling, then just ignore it.
    if sc.sc_bus.use_polling.get() != 0 {
        return 0;
    }

    ohci_intr1(sc)
}

/// `ohci_intr1`: takes the done queue's head from the HCCA, acknowledges the controller's
/// interrupts and dispatches them.
pub fn ohci_intr1(sc: &'static OhciSoftc) -> i32 {
    // In case the interrupt occurs before initialization has completed.
    let Some(hcca) = sc.sc_hcca.get() else {
        #[cfg(feature = "diagnostic")]
        printf(format_args!("ohci_intr: sc->sc_hcca == NULL\n"));
        return 0;
    };

    let mut intrs = 0;
    let mut done = ohci_get(&hcca.hcca_done_head);
    if done != 0 {
        if done & !OHCI_DONE_INTRS != 0 {
            intrs = OHCI_WDH;
        }
        if done & OHCI_DONE_INTRS != 0 {
            intrs |= oread4(sc, OHCI_INTERRUPT_STATUS);
        }
        ohci_set(&hcca.hcca_done_head, 0);
    } else {
        intrs = oread4(sc, OHCI_INTERRUPT_STATUS);
        // If we've flushed out a WDH then reread
        if intrs & OHCI_WDH != 0 {
            done = ohci_get(&hcca.hcca_done_head);
            ohci_set(&hcca.hcca_done_head, 0);
        }
    }

    if intrs == 0xffffffff {
        sc.sc_bus.dying.set(1);
        return 0;
    }

    if intrs == 0 {
        return 0;
    }

    intrs &= !OHCI_MIE;
    owrite4(sc, OHCI_INTERRUPT_STATUS, intrs); // Acknowledge
    let mut eintrs = intrs & sc.sc_eintrs.get();
    if eintrs == 0 {
        return 0;
    }

    sc.sc_bus.intr_context.set(sc.sc_bus.intr_context.get() + 1);
    sc.sc_bus
        .no_intrs
        .set(sc.sc_bus.no_intrs.get().wrapping_add(1));

    if eintrs & OHCI_SO != 0 {
        sc.sc_overrun_cnt
            .set(sc.sc_overrun_cnt.get().wrapping_add(1));
        let mut ntc = sc.sc_overrun_ntc.get();
        let print = usbd_ratecheck(&mut ntc);
        sc.sc_overrun_ntc.set(ntc);
        if print {
            printf(format_args!(
                "{}: {} scheduling overruns\n",
                devname(sc),
                sc.sc_overrun_cnt.get()
            ));
            sc.sc_overrun_cnt.set(0);
        }
        // XXX do what
        eintrs &= !OHCI_SO;
    }
    if eintrs & OHCI_WDH != 0 {
        ohci_add_done(sc, done & !OHCI_DONE_INTRS);
        usb_schedsoftintr(&sc.sc_bus);
        eintrs &= !OHCI_WDH;
    }
    if eintrs & OHCI_RD != 0 {
        printf(format_args!("{}: resume detect\n", devname(sc)));
        // XXX process resume detect
    }
    if eintrs & OHCI_UE != 0 {
        printf(format_args!(
            "{}: unrecoverable error, controller halted\n",
            devname(sc)
        ));
        owrite4(sc, OHCI_CONTROL, OHCI_HCFS_RESET);
        // XXX what else
    }
    if eintrs & OHCI_RHSC != 0 {
        ohci_rhsc(sc, sc.sc_intrxfer.get());
        // Disable RHSC interrupt for now, because it will be on until the port has been
        // reset.
        ohci_rhsc_able(sc, false);

        // Do not allow RHSC interrupts > 1 per second
        timeout_add_sec(&sc.sc_tmo_rhsc, 1);
        eintrs &= !OHCI_RHSC;
    }

    sc.sc_bus.intr_context.set(sc.sc_bus.intr_context.get() - 1);

    if eintrs != 0 {
        // Block unprocessed interrupts. XXX
        owrite4(sc, OHCI_INTERRUPT_DISABLE, eintrs);
        sc.sc_eintrs.set(sc.sc_eintrs.get() & !eintrs);
        printf(format_args!(
            "{}: blocking intrs 0x{:x}\n",
            devname(sc),
            eintrs
        ));
    }

    1
}

/// `ohci_rhsc_able`: turns the root hub status change interrupt on or off.
pub fn ohci_rhsc_able(sc: &OhciSoftc, on: bool) {
    if on {
        sc.sc_eintrs.set(sc.sc_eintrs.get() | OHCI_RHSC);
        owrite4(sc, OHCI_INTERRUPT_ENABLE, OHCI_RHSC);
    } else {
        sc.sc_eintrs.set(sc.sc_eintrs.get() & !OHCI_RHSC);
        owrite4(sc, OHCI_INTERRUPT_DISABLE, OHCI_RHSC);
    }
}

/// `ohci_rhsc_enable`: `sc_tmo_rhsc`, a second after a change: reports the ports' state and
/// turns the change interrupt back on.
pub fn ohci_rhsc_enable(v_sc: *mut c_void) {
    // SAFETY: `ohci_init` sets the timeout with the softc as argument; `ohci_detach` deletes
    // it before the softc goes away.
    let sc: &'static OhciSoftc = unsafe { &*v_sc.cast::<OhciSoftc>() };

    if sc.sc_bus.dying.get() != 0 {
        return;
    }

    let s = splhardusb();
    ohci_rhsc(sc, sc.sc_intrxfer.get());

    ohci_rhsc_able(sc, true);
    splx(s);
}

/// `ohci_add_done`: reverses the controller's done queue (newest first) into completion
/// order, finding each descriptor by bus address, and appends it to the lists the soft
/// interrupt processes.
pub fn ohci_add_done(sc: &'static OhciSoftc, mut done: OhciPhysaddrT) {
    // Reverse the done list.
    let mut sdone: Option<&'static OhciSoftTd> = None;
    let mut sidone: Option<&'static OhciSoftItd> = None;
    while done != 0 {
        if let Some(std) = ohci_hash_find_td(sc, done) {
            std.dnext.set(sdone);
            done = ohci_get(&std.td.td_nexttd);
            sdone = Some(std);
            continue;
        }
        if let Some(sitd) = ohci_hash_find_itd(sc, done) {
            sitd.dnext.set(sidone);
            done = ohci_get(&sitd.itd.itd_nextitd);
            sidone = Some(sitd);
            continue;
        }
        panic(format_args!("ohci_add_done: addr 0x{done:08x} not found"));
    }

    // sdone & sidone now hold the done lists.
    // Put them on the already processed lists.
    match sc.sc_sdone.get() {
        None => sc.sc_sdone.set(sdone),
        Some(mut p) => {
            while let Some(n) = p.dnext.get() {
                p = n;
            }
            p.dnext.set(sdone);
        }
    }
    match sc.sc_sidone.get() {
        None => sc.sc_sidone.set(sidone),
        Some(mut p) => {
            while let Some(n) = p.dnext.get() {
                p = n;
            }
            p.dnext.set(sidone);
        }
    }
}

/// `ohci_softintr`: the soft interrupt; `v` is the bus. Completes the transfers whose
/// descriptors the hard interrupt moved to the done lists.
pub fn ohci_softintr(v: *mut c_void) {
    // SAFETY: `usb_attach` establishes the soft interrupt (and `usb_schedsoftintr` calls it)
    // with the bus as argument; the bus lives in the softc for the controller's life.
    let bus: &'static UsbdBus = unsafe { &*v.cast::<UsbdBus>() };
    let sc = ohci_softc(bus);

    if sc.sc_bus.dying.get() != 0 {
        return;
    }

    sc.sc_bus.intr_context.set(sc.sc_bus.intr_context.get() + 1);

    let s = splhardusb();
    let sdone = sc.sc_sdone.take();
    let sidone = sc.sc_sidone.take();
    splx(s);

    let mut std = sdone;
    while let Some(t) = std {
        let stdnext = t.dnext.get();
        std = stdnext;
        let Some(xfer) = t.xfer.get() else {
            // xfer == NULL: There seems to be no xfer associated with this TD. It is tailp
            // that happened to end up on the done queue. Shouldn't happen, but some chips
            // are broken(?).
            continue;
        };
        if xfer.status.get() == USBD_CANCELLED || xfer.status.get() == USBD_TIMEOUT {
            // Handled by abort routine.
            continue;
        }
        timeout_del(&xfer.timeout_handle);
        usb_rem_task(xfer.device(), &xfer.abort_task);

        let mut len = i32::from(t.len.get());
        let cbp = ohci_get(&t.td.td_cbp);
        if cbp != 0 {
            len -= ohci_get(&t.td.td_be).wrapping_sub(cbp).wrapping_add(1) as i32;
        }
        if t.flags.get() & OHCI_ADD_LEN != 0 {
            xfer.actlen.set(xfer.actlen.get().wrapping_add_signed(len));
        }

        let cc = ohci_td_get_cc(ohci_get(&t.td.td_flags));
        if cc == OHCI_CC_NO_ERROR {
            let done = t.flags.get() & OHCI_CALL_DONE != 0;

            ohci_free_std(sc, t);
            if done {
                if xfer.actlen.get() != 0 {
                    xfer_sync(xfer, xfer.actlen.get(), true);
                }
                xfer.status.set(USBD_NORMAL_COMPLETION);
                let s = splusb();
                usb_transfer_complete(xfer);
                splx(s);
            }
        } else {
            // Endpoint is halted. First unlink all the TDs belonging to the failed transfer,
            // and then restart the endpoint.
            let opipe = ohci_pipe(xfer.pipe());

            // remove TDs
            let mut p = t;
            while same_xfer(p.xfer.get(), xfer) {
                let n = match p.nexttd.get() {
                    Some(n) => n,
                    None => panic(format_args!("ohci_softintr: TD chain ends early")),
                };
                ohci_free_std(sc, p);
                p = n;
            }

            // clear halt
            ohci_set(&opipe.sed().ed.ed_headp, p.physaddr.get());
            owrite4(sc, OHCI_COMMAND_STATUS, OHCI_CLF);

            if cc == OHCI_CC_STALL {
                xfer.status.set(USBD_STALLED);
            } else if cc == OHCI_CC_DATA_UNDERRUN {
                if xfer.actlen.get() != 0 {
                    xfer_sync(xfer, xfer.actlen.get(), true);
                }
                xfer.status.set(USBD_NORMAL_COMPLETION);
            } else {
                xfer.status.set(USBD_IOERROR);
            }
            let s = splusb();
            usb_transfer_complete(xfer);
            splx(s);
        }
    }

    let mut sitd = sidone;
    while let Some(si) = sitd {
        let sitdnext = si.dnext.get();
        sitd = sitdnext;
        let Some(xfer) = si.xfer.get() else {
            continue;
        };
        if xfer.status.get() == USBD_CANCELLED || xfer.status.get() == USBD_TIMEOUT {
            // Handled by abort routine.
            continue;
        }
        #[cfg(feature = "diagnostic")]
        {
            if si.isdone.get() != 0 {
                printf(format_args!(
                    "ohci_softintr: sitd={:p} is done\n",
                    ptr::from_ref(si)
                ));
            }
            si.isdone.set(1);
        }
        if si.flags.get() & OHCI_CALL_DONE != 0 {
            let opipe = ohci_pipe(xfer.pipe());
            opipe
                .iso_inuse
                .set(opipe.iso_inuse.get() - xfer.nframes.get());
            let uedir = ue_get_dir(xfer.pipe().endpoint().edesc().bEndpointAddress);
            xfer.status.set(USBD_NORMAL_COMPLETION);
            let mut actlen: u32 = 0;
            let mut i = 0;
            let mut cur = match hcpriv_sitd(xfer) {
                Some(c) => c,
                None => panic(format_args!("ohci_softintr: isochronous xfer without ITDs")),
            };
            loop {
                let next = cur.nextitd.get();
                if ohci_itd_get_cc(ohci_get(&cur.itd.itd_flags)) != OHCI_CC_NO_ERROR {
                    xfer.status.set(USBD_IOERROR);
                }
                // For input, update frlengths with actual
                // XXX anything necessary for output?
                if uedir == UE_DIR_IN && xfer.status.get() == USBD_NORMAL_COMPLETION {
                    let iframes = ohci_itd_get_fc(ohci_get(&cur.itd.itd_flags)) as usize;
                    for j in 0..iframes {
                        let psw = cur.itd.itd_offset.get(j).map_or(0, ohci_get16);
                        let len = if u32::from(ohci_itd_psw_get_cc(psw)) & OHCI_CC_NOT_ACCESSED_MASK
                            == OHCI_CC_NOT_ACCESSED
                        {
                            0
                        } else {
                            ohci_itd_psw_length(psw)
                        };
                        set_frlength(xfer, i, len);
                        actlen += u32::from(len);
                        i += 1;
                    }
                }
                if cur.flags.get() & OHCI_CALL_DONE != 0 {
                    break;
                }
                ohci_free_sitd(sc, cur);
                cur = match next {
                    Some(n) => n,
                    None => panic(format_args!("ohci_softintr: ITD chain ends early")),
                };
            }
            ohci_free_sitd(sc, cur);
            if uedir == UE_DIR_IN && xfer.status.get() == USBD_NORMAL_COMPLETION {
                xfer.actlen.set(actlen);
            }
            xfer.hcpriv.set(ptr::null_mut());

            if xfer.status.get() == USBD_NORMAL_COMPLETION {
                xfer_sync(xfer, xfer.length.get(), true);
            }

            let s = splusb();
            usb_transfer_complete(xfer);
            splx(s);
        }
    }

    if sc.sc_softwake.get() != 0 {
        sc.sc_softwake.set(0);
        wakeup(ptr::from_ref(&sc.sc_softwake));
    }

    sc.sc_bus.intr_context.set(sc.sc_bus.intr_context.get() - 1);
}

/// `ohci_device_ctrl_done`.
pub fn ohci_device_ctrl_done(xfer: &'static UsbdXfer) {
    #[cfg(feature = "diagnostic")]
    if xfer.rqflags.get() & crate::dev::usb::usbdivar::URQ_REQUEST == 0 {
        panic(format_args!("ohci_device_ctrl_done: not a request"));
    }
    #[cfg(not(feature = "diagnostic"))]
    let _ = xfer;
}

/// `ohci_device_intr_done`: a repeating interrupt xfer is queued again at once, its data TD
/// being the old tail and a new TD the new tail.
pub fn ohci_device_intr_done(xfer: &'static UsbdXfer) {
    let sc = ohci_softc(xfer.device().bus());
    let opipe = ohci_pipe(xfer.pipe());

    if xfer.pipe().repeat.get() != 0 {
        let sed = opipe.sed();
        let data = opipe.tail_td();
        let Some(tail) = ohci_alloc_std(sc) else {
            xfer.status.set(USBD_NOMEM);
            return;
        };
        tail.xfer.set(None);

        ohci_set(
            &data.td.td_flags,
            OHCI_TD_IN | OHCI_TD_NOCC | ohci_td_set_di(1) | OHCI_TD_TOGGLE_CARRY,
        );
        if xfer.flags.get() & USBD_SHORT_XFER_OK != 0 {
            ohci_or(&data.td.td_flags, OHCI_TD_R);
        }
        ohci_set(&data.td.td_cbp, dmaaddr(&xfer.dmabuf, 0) as u32);
        data.nexttd.set(Some(tail));
        ohci_set(&data.td.td_nexttd, tail.physaddr.get());
        ohci_set(
            &data.td.td_be,
            ohci_get(&data.td.td_cbp)
                .wrapping_add(xfer.length.get())
                .wrapping_sub(1),
        );
        data.len.set(xfer.length.get() as u16);
        data.xfer.set(Some(xfer));
        data.flags.set(OHCI_CALL_DONE | OHCI_ADD_LEN);
        set_hcpriv_std(xfer, data);
        xfer.actlen.set(0);

        ohci_set(&sed.ed.ed_tailp, tail.physaddr.get());
        opipe.tail_td.set(Some(tail));
    }
}

/// `ohci_device_bulk_done`.
pub fn ohci_device_bulk_done(_xfer: &'static UsbdXfer) {}

/// `ohci_rhsc`: completes the root hub's interrupt transfer with the ports whose status
/// changed.
pub fn ohci_rhsc(sc: &'static OhciSoftc, xfer: Option<&'static UsbdXfer>) {
    let _hstatus = oread4(sc, OHCI_RH_STATUS);

    let Some(xfer) = xfer else {
        // Just ignore the change.
        return;
    };

    let length = xfer.length.get();
    // SAFETY: the root hub's interrupt xfer is in the driver's hands until completed here;
    // its DMA buffer has `length` bytes.
    let p = unsafe { xfer_buf(xfer, length as usize) };
    let m = (sc.sc_noport.get() as u32).min(length.wrapping_mul(8).wrapping_sub(1));
    p.fill(0);
    for i in 1..=m {
        // Pick out CHANGE bits from the status reg.
        if oread4(sc, ohci_rh_port_status(i as usize)) >> 16 != 0
            && let Some(b) = p.get_mut(i as usize / 8)
        {
            *b |= 1 << (i % 8);
        }
    }
    xfer.actlen.set(xfer.length.get());
    xfer.status.set(USBD_NORMAL_COMPLETION);

    usb_transfer_complete(xfer);
}

/// `ohci_root_intr_done`.
pub fn ohci_root_intr_done(_xfer: &'static UsbdXfer) {}

/// `ohci_root_ctrl_done`.
pub fn ohci_root_ctrl_done(_xfer: &'static UsbdXfer) {}

/// `ohci_poll`: the bus's `do_poll`.
pub fn ohci_poll(bus: &'static UsbdBus) {
    let sc = ohci_softc(bus);

    if oread4(sc, OHCI_INTERRUPT_STATUS) & sc.sc_eintrs.get() != 0 {
        ohci_intr1(sc);
    }
}

/// `ohci_device_request`: a control transfer as a SETUP TD (the pipe's old tail), the data
/// TDs and a STATUS TD, ahead of a new tail.
pub fn ohci_device_request(xfer: &'static UsbdXfer) -> UsbdStatus {
    let sc = ohci_softc(xfer.device().bus());
    let opipe = ohci_pipe(xfer.pipe());
    let req = xfer.request.get();

    let len = u32::from(ugetw(req.wLength));

    let setup = opipe.tail_td();
    let Some(mut stat) = ohci_alloc_std(sc) else {
        // bad1:
        return USBD_NOMEM;
    };
    let Some(tail) = ohci_alloc_std(sc) else {
        // bad2:
        ohci_free_std(sc, stat);
        return USBD_NOMEM;
    };
    tail.xfer.set(None);

    let sed = opipe.sed();

    let next = stat;

    // Set up data transaction
    if len != 0 {
        let std = stat;

        match ohci_alloc_std_chain(sc, len, xfer, std) {
            Ok(end) => {
                // point at free TD
                stat = match end.and_then(|e| e.nexttd.get()) {
                    Some(s) => s,
                    None => panic(format_args!("ohci_device_request: empty data chain")),
                };
            }
            Err(err) => {
                // `stat = stat->nexttd`, then bad3: bad2:
                let second = std.nexttd.get();
                ohci_free_std(sc, tail);
                if let Some(s) = second {
                    ohci_free_std(sc, s);
                }
                return err;
            }
        }
        // Start toggle at 1 and then use the carried toggle.
        ohci_clr(&std.td.td_flags, OHCI_TD_TOGGLE_MASK);
        ohci_or(&std.td.td_flags, OHCI_TD_TOGGLE_1);
    }

    let reqlen = size_of::<UsbDeviceRequest>();
    // SAFETY: `ohci_open` allocated `sizeof(usb_device_request_t)` bytes of DMA memory for
    // the pipe's SETUP packet, which only this pipe's started transfer writes.
    unsafe {
        ptr::copy_nonoverlapping(req.as_bytes().as_ptr(), kernaddr(&opipe.reqdma, 0), reqlen)
    };

    ohci_set(
        &setup.td.td_flags,
        OHCI_TD_SETUP | OHCI_TD_NOCC | OHCI_TD_TOGGLE_0 | OHCI_TD_NOINTR,
    );
    ohci_set(&setup.td.td_cbp, dmaaddr(&opipe.reqdma, 0) as u32);
    setup.nexttd.set(Some(next));
    ohci_set(&setup.td.td_nexttd, next.physaddr.get());
    ohci_set(
        &setup.td.td_be,
        ohci_get(&setup.td.td_cbp)
            .wrapping_add(reqlen as u32)
            .wrapping_sub(1),
    );
    setup.len.set(0);
    setup.xfer.set(Some(xfer));
    setup.flags.set(0);
    set_hcpriv_std(xfer, setup);

    ohci_set(
        &stat.td.td_flags,
        (if usbd_xfer_isread(xfer) {
            OHCI_TD_OUT
        } else {
            OHCI_TD_IN
        }) | OHCI_TD_NOCC
            | OHCI_TD_TOGGLE_1
            | ohci_td_set_di(1),
    );
    ohci_set(&stat.td.td_cbp, 0);
    stat.nexttd.set(Some(tail));
    ohci_set(&stat.td.td_nexttd, tail.physaddr.get());
    ohci_set(&stat.td.td_be, 0);
    stat.flags.set(OHCI_CALL_DONE);
    stat.len.set(0);
    stat.xfer.set(Some(xfer));

    // Insert ED in schedule
    let s = splusb();
    ohci_set(&sed.ed.ed_tailp, tail.physaddr.get());
    opipe.tail_td.set(Some(tail));
    owrite4(sc, OHCI_COMMAND_STATUS, OHCI_CLF);
    if xfer.timeout.get() != 0 && sc.sc_bus.use_polling.get() == 0 {
        ohci_arm_timeout(xfer);
    }
    splx(s);

    USBD_NORMAL_COMPLETION
}

/// `ohci_add_ed`: adds an ED to the schedule, after `head`. Called at `splusb()`.
pub fn ohci_add_ed(sed: &'static OhciSoftEd, head: &'static OhciSoftEd) {
    splsoftassert(IPL_SOFTUSB, "ohci_add_ed");
    sed.next.set(head.next.get());
    ohci_set(&sed.ed.ed_nexted, ohci_get(&head.ed.ed_nexted));
    head.next.set(Some(sed));
    ohci_set(&head.ed.ed_nexted, sed.physaddr.get());
}

/// `ohci_rem_ed`: removes an ED from the schedule. Called at `splusb()`.
pub fn ohci_rem_ed(sed: &'static OhciSoftEd, head: &'static OhciSoftEd) {
    splsoftassert(IPL_SOFTUSB, "ohci_rem_ed");

    // XXX
    let mut p = Some(head);
    while let Some(q) = p {
        if q.next.get().is_some_and(|n| ptr::eq(n, sed)) {
            break;
        }
        p = q.next.get();
    }
    let Some(p) = p else {
        panic(format_args!("ohci_rem_ed: ED not found"));
    };
    p.next.set(sed.next.get());
    ohci_set(&p.ed.ed_nexted, ohci_get(&sed.ed.ed_nexted));
}

/// `HASH(a)`: the hash bucket of a descriptor's bus address.
///
/// When a transfer is completed the TD is added to the done queue by the host controller.
/// This queue is the processed by software. Unfortunately the queue contains the physical
/// address of the TD and we have no simple way to translate this back to a kernel address.
/// To make the translation possible (and fast) we use a hash table of TDs currently in the
/// schedule. The physical address is used as the hash value.
const fn ohci_hash(a: OhciPhysaddrT) -> usize {
    (a >> 4) as usize % OHCI_HASH_SIZE
}

/// `ohci_hash_add_td`. Called at `splusb()`.
pub fn ohci_hash_add_td(sc: &'static OhciSoftc, std: &'static OhciSoftTd) {
    let h = ohci_hash(std.physaddr.get());

    splsoftassert(IPL_SOFTUSB, "ohci_hash_add_td");

    // SAFETY: a TD taken off the free list is on no hash list; TDs live forever; at splusb()
    // under the kernel lock.
    unsafe { sc.sc_hash_tds[h].insert_head(std) };
}

/// `ohci_hash_find_td`.
pub fn ohci_hash_find_td(sc: &'static OhciSoftc, a: OhciPhysaddrT) -> Option<&'static OhciSoftTd> {
    let h = ohci_hash(a);

    sc.sc_hash_tds[h].iter().find(|std| std.physaddr.get() == a)
}

/// `ohci_hash_add_itd`. Called at `splusb()`.
pub fn ohci_hash_add_itd(sc: &'static OhciSoftc, sitd: &'static OhciSoftItd) {
    let h = ohci_hash(sitd.physaddr.get());

    splsoftassert(IPL_SOFTUSB, "ohci_hash_add_itd");

    // SAFETY: an ITD taken off the free list is on no hash list; ITDs live forever; at
    // splusb() under the kernel lock.
    unsafe { sc.sc_hash_itds[h].insert_head(sitd) };
}

/// `ohci_hash_rem_itd`. Called at `splusb()`.
pub fn ohci_hash_rem_itd(_sc: &'static OhciSoftc, sitd: &'static OhciSoftItd) {
    splsoftassert(IPL_SOFTUSB, "ohci_hash_rem_itd");

    // SAFETY: an ITD handed out by `ohci_alloc_sitd` is on its hash list until it is freed,
    // which happens once; at splusb() under the kernel lock.
    unsafe { ListHead::<OhciSoftItdHash>::remove(sitd) };
}

/// `ohci_hash_find_itd`.
pub fn ohci_hash_find_itd(
    sc: &'static OhciSoftc,
    a: OhciPhysaddrT,
) -> Option<&'static OhciSoftItd> {
    let h = ohci_hash(a);

    sc.sc_hash_itds[h]
        .iter()
        .find(|sitd| sitd.physaddr.get() == a)
}

/// `ohci_timeout`: an xfer's timeout; the abort runs in the USB abort task thread.
pub fn ohci_timeout(addr: *mut c_void) {
    // SAFETY: the timeout's argument is its xfer (`ohci_arm_timeout`), which the stack keeps
    // until the xfer completes, and completion deletes the timeout.
    let xfer: &'static UsbdXfer = unsafe { &*addr.cast::<UsbdXfer>() };
    let sc = ohci_softc(xfer.device().bus());

    if sc.sc_bus.dying.get() != 0 {
        ohci_timeout_task(addr);
        return;
    }

    usb_init_task(
        &xfer.abort_task,
        ohci_timeout_task,
        addr,
        USB_TASK_TYPE_ABORT,
    );
    usb_add_task(xfer.device(), &xfer.abort_task);
}

/// `ohci_timeout_task`.
pub fn ohci_timeout_task(addr: *mut c_void) {
    // SAFETY: as in `ohci_timeout`; completion removes the task.
    let xfer: &'static UsbdXfer = unsafe { &*addr.cast::<UsbdXfer>() };

    let s = splusb();
    ohci_abort_xfer(xfer, USBD_TIMEOUT);
    splx(s);
}

/// Arms the xfer's timeout (`timeout_del`, `timeout_set`, `timeout_add_msec`), as the start
/// functions do.
fn ohci_arm_timeout(xfer: &'static UsbdXfer) {
    timeout_del(&xfer.timeout_handle);
    timeout_set(
        &xfer.timeout_handle,
        ohci_timeout,
        ptr::from_ref(xfer).cast_mut().cast(),
    );
    timeout_add_msec(&xfer.timeout_handle, u64::from(xfer.timeout.get()));
}

/// `ohci_open`: the bus's `open_pipe`. The root hub's pipes are emulated; any other pipe gets
/// an ED with a sentinel TD (or ITD), hung on the list or tree of its transfer type.
pub fn ohci_open(pipe: &'static UsbdPipe) -> UsbdStatus {
    let dev = pipe.device();
    let sc = ohci_softc(dev.bus());
    let ed = pipe.endpoint().edesc();
    let opipe = ohci_pipe(pipe);
    let xfertype = ue_get_xfertype(ed.bmAttributes);

    if sc.sc_bus.dying.get() != 0 {
        return USBD_IOERROR;
    }

    // Root Hub
    if dev.depth.get() == 0 {
        match ed.bEndpointAddress {
            USB_CONTROL_ENDPOINT => pipe.methods.set(Some(&OHCI_ROOT_CTRL_METHODS)),
            a if a == UE_DIR_IN | OHCI_INTR_ENDPT => {
                pipe.methods.set(Some(&OHCI_ROOT_INTR_METHODS))
            }
            _ => return USBD_INVAL,
        }
        return USBD_NORMAL_COMPLETION;
    }

    let Some(sed) = ohci_alloc_sed(sc) else {
        // bad0:
        return USBD_NOMEM;
    };
    opipe.sed.set(Some(sed));
    let mut std = None;
    let (tdphys, fmt) = if xfertype == UE_ISOCHRONOUS {
        let Some(sitd) = ohci_alloc_sitd(sc) else {
            // bad1:
            ohci_free_sed(sc, sed);
            return USBD_NOMEM;
        };
        opipe.tail_itd.set(Some(sitd));
        let dir = if ue_get_dir(ed.bEndpointAddress) == UE_DIR_IN {
            OHCI_ED_DIR_IN
        } else {
            OHCI_ED_DIR_OUT
        };
        (sitd.physaddr.get(), OHCI_ED_FORMAT_ISO | dir)
    } else {
        let Some(t) = ohci_alloc_std(sc) else {
            // bad1:
            ohci_free_sed(sc, sed);
            return USBD_NOMEM;
        };
        opipe.tail_td.set(Some(t));
        std = Some(t);
        (t.physaddr.get(), OHCI_ED_FORMAT_GEN | OHCI_ED_DIR_TD)
    };
    ohci_set(
        &sed.ed.ed_flags,
        ohci_ed_set_fa(u32::from(dev.address.get()))
            | ohci_ed_set_en(u32::from(ue_get_addr(ed.bEndpointAddress)))
            | (if dev.speed.get() == USB_SPEED_LOW {
                OHCI_ED_SPEED
            } else {
                0
            })
            | fmt
            | ohci_ed_set_maxp(u32::from(ugetw(ed.wMaxPacketSize))),
    );
    ohci_set(
        &sed.ed.ed_headp,
        tdphys
            | (if pipe.endpoint().savedtoggle.get() != 0 {
                OHCI_TOGGLECARRY
            } else {
                0
            }),
    );
    ohci_set(&sed.ed.ed_tailp, tdphys);

    match xfertype {
        UE_CONTROL => {
            pipe.methods.set(Some(&OHCI_DEVICE_CTRL_METHODS));
            let err = usb_allocmem(
                &sc.sc_bus,
                size_of::<UsbDeviceRequest>(),
                0,
                USB_DMA_COHERENT,
                &opipe.reqdma,
            );
            if err.is_err() {
                // bad: bad1: bad0:
                if let Some(t) = std {
                    ohci_free_std(sc, t);
                }
                ohci_free_sed(sc, sed);
                return USBD_NOMEM;
            }
            let s = splusb();
            ohci_add_ed(sed, sc.ctrl_head());
            splx(s);
        }
        UE_INTERRUPT => {
            pipe.methods.set(Some(&OHCI_DEVICE_INTR_METHODS));
            let mut ival = pipe.interval.get();
            if ival == USBD_DEFAULT_INTERVAL {
                ival = i32::from(ed.bInterval);
            }
            return ohci_device_setintr(sc, opipe, ival);
        }
        UE_ISOCHRONOUS => {
            pipe.methods.set(Some(&OHCI_DEVICE_ISOC_METHODS));
            return ohci_setup_isoc(pipe);
        }
        UE_BULK => {
            pipe.methods.set(Some(&OHCI_DEVICE_BULK_METHODS));
            let s = splusb();
            ohci_add_ed(sed, sc.bulk_head());
            splx(s);
        }
        _ => {}
    }
    USBD_NORMAL_COMPLETION
}

/// `ohci_setaddr`: the bus's `dev_setaddr`. Works around the half configured control
/// (default) pipe when setting the address of a device.
///
/// Because a single ED is setup per endpoint in `ohci_open()`, and the control pipe is
/// configured before we could have set the address of the device or read the
/// `wMaxPacketSize` of the endpoint, we have to re-open the pipe twice here.
pub fn ohci_setaddr(dev: &'static UsbdDevice, addr: i32) -> Result<(), Errno> {
    // Root Hub
    if dev.depth.get() == 0 {
        return Ok(());
    }

    let pipe = default_pipe(dev);

    // Re-establish the default pipe with the new max packet size.
    ohci_device_ctrl_close(pipe);
    if ohci_open(pipe).is_err() {
        return Err(Errno::EINVAL);
    }

    if usbd_set_address(dev, addr).is_err() {
        return Err(Errno::EPERM);
    }

    dev.address.set(addr as u8);

    // Re-establish the default pipe with the new address.
    ohci_device_ctrl_close(pipe);
    if ohci_open(pipe).is_err() {
        return Err(Errno::EINVAL);
    }

    Ok(())
}

/// `dev->default_pipe`. Panics on a device without one (the C's NULL dereference).
fn default_pipe(dev: &'static UsbdDevice) -> &'static UsbdPipe {
    match dev.default_pipe.get() {
        Some(p) => p,
        None => panic(format_args!("ohci: device without a default pipe")),
    }
}

/// `ohci_close_pipe`: closes a regular pipe: takes its ED off the list `head` starts, keeps
/// its data toggle and frees it. Assumes that there are no pending transactions.
pub fn ohci_close_pipe(pipe: &'static UsbdPipe, head: &'static OhciSoftEd) {
    let opipe = ohci_pipe(pipe);
    let sc = ohci_softc(pipe.device().bus());
    let sed = opipe.sed();

    let s = splusb();
    #[cfg(feature = "diagnostic")]
    {
        ohci_or(&sed.ed.ed_flags, OHCI_ED_SKIP);
        if ohci_get(&sed.ed.ed_tailp) & OHCI_HEADMASK != ohci_get(&sed.ed.ed_headp) & OHCI_HEADMASK
        {
            let std = ohci_hash_find_td(sc, ohci_get(&sed.ed.ed_headp));
            printf(format_args!(
                "ohci_close_pipe: pipe not empty sed={:p} hd=0x{:x} tl=0x{:x} pipe={:p}, std={:p}\n",
                ptr::from_ref(sed),
                ohci_get(&sed.ed.ed_headp) as i32,
                ohci_get(&sed.ed.ed_tailp) as i32,
                ptr::from_ref(pipe),
                std.map_or(ptr::null(), ptr::from_ref),
            ));
            usb_delay_ms(&sc.sc_bus, 2);
            if ohci_get(&sed.ed.ed_tailp) & OHCI_HEADMASK
                != ohci_get(&sed.ed.ed_headp) & OHCI_HEADMASK
            {
                printf(format_args!("ohci_close_pipe: pipe still not empty\n"));
            }
        }
    }
    ohci_rem_ed(sed, head);
    // Make sure the host controller is not touching this ED
    usb_delay_ms(&sc.sc_bus, 1);
    splx(s);
    pipe.endpoint()
        .savedtoggle
        .set(if ohci_get(&sed.ed.ed_headp) & OHCI_TOGGLECARRY != 0 {
            1
        } else {
            0
        });
    ohci_free_sed(sc, sed);
}

/// `ohci_abort_xfer`: abort a device request.
///
/// If this routine is called at `splusb()` it guarantees that the request will be removed
/// from the hardware scheduling and that the callback for it will be called with
/// `USBD_CANCELLED` status. It's impossible to guarantee that the requested transfer will not
/// have happened since the hardware runs concurrently. If the transaction has already
/// happened we rely on the ordinary interrupt processing to process it.
pub fn ohci_abort_xfer(xfer: &'static UsbdXfer, status: UsbdStatus) {
    let opipe = ohci_pipe(xfer.pipe());
    let sc = ohci_softc(xfer.device().bus());
    let sed = opipe.sed();

    if sc.sc_bus.dying.get() != 0 {
        // If we're dying, just do the software part.
        let s = splusb();
        xfer.status.set(status); // make software ignore it
        timeout_del(&xfer.timeout_handle);
        usb_rem_task(xfer.device(), &xfer.abort_task);
        usb_transfer_complete(xfer);
        splx(s);
        return;
    }

    if xfer.device().bus().intr_context.get() != 0 || curproc().is_none() {
        panic(format_args!("ohci_abort_xfer: not in process context"));
    }

    // Step 1: Make interrupt routine and hardware ignore xfer.
    let s = splusb();
    xfer.status.set(status); // make software ignore it
    timeout_del(&xfer.timeout_handle);
    usb_rem_task(xfer.device(), &xfer.abort_task);
    splx(s);
    ohci_or(&sed.ed.ed_flags, OHCI_ED_SKIP); // force hardware skip

    // Step 2: Wait until we know hardware has finished any possible use of the xfer. Also
    // make sure the soft interrupt routine has run.
    usb_delay_ms(xfer.device().bus(), 20); // Hardware finishes in 1ms
    let s = splusb();
    sc.sc_softwake.set(1);
    usb_schedsoftintr(&sc.sc_bus);
    let _ = tsleep_nsec(ptr::from_ref(&sc.sc_softwake), PZERO, "ohciab", INFSLP);
    splx(s);

    // Step 3: Remove any vestiges of the xfer from the hardware. The complication here is
    // that the hardware may have executed beyond the xfer we're trying to abort. So as we're
    // scanning the TDs of this xfer we check if the hardware points to any of them.
    let s = splusb(); // XXX why?
    let Some(mut p) = hcpriv_std(xfer) else {
        #[cfg(feature = "diagnostic")]
        {
            splx(s);
            printf(format_args!("ohci_abort_xfer: hcpriv is NULL\n"));
            return;
        }
        #[cfg(not(feature = "diagnostic"))]
        panic(format_args!("ohci_abort_xfer: hcpriv is NULL"));
    };
    let headp = ohci_get(&sed.ed.ed_headp) & OHCI_HEADMASK;
    let mut hit = false;
    while same_xfer(p.xfer.get(), xfer) {
        hit |= headp == p.physaddr.get();
        let n = match p.nexttd.get() {
            Some(n) => n,
            None => panic(format_args!("ohci_abort_xfer: TD chain ends early")),
        };
        if ohci_td_get_cc(ohci_get(&p.td.td_flags)) == OHCI_CC_NOT_ACCESSED {
            ohci_free_std(sc, p);
        }
        p = n;
    }
    // Zap headp register if hardware pointed inside the xfer.
    if hit {
        ohci_set(&sed.ed.ed_headp, p.physaddr.get()); // unlink TDs
    }

    // Step 4: Turn on hardware again.
    ohci_clr(&sed.ed.ed_flags, OHCI_ED_SKIP); // remove hardware skip

    // Step 5: Execute callback.
    usb_transfer_complete(xfer);

    splx(s);
}

/// `ohci_root_ctrl_transfer`: simulates a hardware hub by handling all the necessary
/// requests.
pub fn ohci_root_ctrl_transfer(xfer: &'static UsbdXfer) -> UsbdStatus {
    // Insert last in queue.
    let err = usb_insert_transfer(xfer);
    if err.is_err() {
        return err;
    }

    // Pipe isn't running, start first
    ohci_root_ctrl_start(queue_first(xfer.pipe()))
}

/// `ohci_root_ctrl_start`: answers a request to the emulated root hub from the root hub
/// registers and completes it at once.
pub fn ohci_root_ctrl_start(xfer: &'static UsbdXfer) -> UsbdStatus {
    let sc = ohci_softc(xfer.device().bus());

    if sc.sc_bus.dying.get() != 0 {
        return USBD_IOERROR;
    }

    #[cfg(feature = "diagnostic")]
    if xfer.rqflags.get() & crate::dev::usb::usbdivar::URQ_REQUEST == 0 {
        // XXX panic
        return USBD_INVAL;
    }
    let req = xfer.request.get();

    let len = usize::from(ugetw(req.wLength));
    let value = ugetw(req.wValue);
    let index = ugetw(req.wIndex);

    let buf: &mut [u8] = if len != 0 {
        // SAFETY: `usbd_transfer` started the xfer with its DMA buffer; the request is
        // answered and completed here before anybody else looks at the buffer.
        unsafe { xfer_buf(xfer, len) }
    } else {
        &mut []
    };
    let len = buf.len();

    // The C's `goto ret` with `err` set is `Err(err)`; the length moved is `totlen`.
    let res: Result<usize, UsbdStatus> = 'ret: {
        let mut totlen = 0;
        match (req.bRequest, req.bmRequestType) {
            (UR_CLEAR_FEATURE, UT_WRITE_DEVICE)
            | (UR_CLEAR_FEATURE, UT_WRITE_INTERFACE)
            | (UR_CLEAR_FEATURE, UT_WRITE_ENDPOINT) => {
                // DEVICE_REMOTE_WAKEUP and ENDPOINT_HALT are no-ops for the integrated root
                // hub.
            }
            (UR_GET_CONFIG, UT_READ_DEVICE) => {
                if len > 0 {
                    buf[0] = sc.sc_conf.get();
                    totlen = 1;
                }
            }
            (UR_GET_DESCRIPTOR, UT_READ_DEVICE) => match (value >> 8) as u8 {
                UDESC_DEVICE => {
                    if value & 0xff != 0 {
                        break 'ret Err(USBD_IOERROR);
                    }
                    let mut devd = OHCI_DEVD;
                    usetw(&mut devd.idVendor, sc.sc_id_vendor.get() as u16);
                    let l = len.min(USB_DEVICE_DESCRIPTOR_SIZE);
                    totlen = l;
                    buf[..l].copy_from_slice(&devd.as_bytes()[..l]);
                }
                UDESC_CONFIG => {
                    if value & 0xff != 0 {
                        break 'ret Err(USBD_IOERROR);
                    }
                    let parts: [&[u8]; 3] = [
                        &OHCI_CONFD.as_bytes()[..USB_CONFIG_DESCRIPTOR_SIZE],
                        &OHCI_IFCD.as_bytes()[..USB_INTERFACE_DESCRIPTOR_SIZE],
                        &OHCI_ENDPD.as_bytes()[..USB_ENDPOINT_DESCRIPTOR_SIZE],
                    ];
                    for part in parts {
                        let l = (len - totlen).min(part.len());
                        buf[totlen..totlen + l].copy_from_slice(&part[..l]);
                        totlen += l;
                    }
                }
                UDESC_STRING => {
                    if len == 0 {
                        break 'ret Ok(0);
                    }
                    buf[0] = 0;
                    totlen = 1;
                    let vendor = sc.sc_vendor.get();
                    let s: Option<&[u8]> = match value & 0xff {
                        0 => Some(b"\x01"),          // Language table
                        1 => Some(&vendor),          // Vendor
                        2 => Some(b"OHCI root hub"), // Product
                        _ => None,
                    };
                    if let Some(s) = s {
                        let mut sd = UsbStringDescriptor::zeroed();
                        let n = usbd_str(&mut sd, len as i32, s);
                        let l = len.min(size_of::<UsbStringDescriptor>());
                        buf[..l].copy_from_slice(&sd.as_bytes()[..l]);
                        totlen = n as usize;
                    }
                }
                _ => break 'ret Err(USBD_IOERROR),
            },
            (UR_GET_INTERFACE, UT_READ_INTERFACE) => {
                if len > 0 {
                    buf[0] = 0;
                    totlen = 1;
                }
            }
            (UR_GET_STATUS, UT_READ_DEVICE) => {
                if len > 1 {
                    let mut w = [0u8; 2];
                    usetw(&mut w, UDS_SELF_POWERED);
                    buf[..2].copy_from_slice(&w);
                    totlen = 2;
                }
            }
            (UR_GET_STATUS, UT_READ_INTERFACE) | (UR_GET_STATUS, UT_READ_ENDPOINT) => {
                if len > 1 {
                    buf[..2].fill(0);
                    totlen = 2;
                }
            }
            (UR_SET_ADDRESS, UT_WRITE_DEVICE) => {
                if usize::from(value) >= USB_MAX_DEVICES {
                    break 'ret Err(USBD_IOERROR);
                }
            }
            (UR_SET_CONFIG, UT_WRITE_DEVICE) => {
                if value != 0 && value != 1 {
                    break 'ret Err(USBD_IOERROR);
                }
                sc.sc_conf.set(value as u8);
            }
            (UR_SET_DESCRIPTOR, UT_WRITE_DEVICE) => {}
            (UR_SET_FEATURE, UT_WRITE_DEVICE)
            | (UR_SET_FEATURE, UT_WRITE_INTERFACE)
            | (UR_SET_FEATURE, UT_WRITE_ENDPOINT) => break 'ret Err(USBD_IOERROR),
            (UR_SET_INTERFACE, UT_WRITE_INTERFACE) => {}
            (UR_SYNCH_FRAME, UT_WRITE_ENDPOINT) => {}
            /* Hub requests */
            (UR_CLEAR_FEATURE, UT_WRITE_CLASS_DEVICE) => {}
            (UR_CLEAR_FEATURE, UT_WRITE_CLASS_OTHER) => {
                if index < 1 || i32::from(index) > sc.sc_noport.get() {
                    break 'ret Err(USBD_IOERROR);
                }
                let port = ohci_rh_port_status(usize::from(index));
                let value = i32::from(value);
                match value {
                    UHF_PORT_ENABLE => owrite4(sc, port, u32::from(UPS_CURRENT_CONNECT_STATUS)),
                    UHF_PORT_SUSPEND => owrite4(sc, port, u32::from(UPS_OVERCURRENT_INDICATOR)),
                    UHF_PORT_POWER => {
                        // Yes, writing to the LOW_SPEED bit clears power.
                        owrite4(sc, port, u32::from(UPS_LOW_SPEED))
                    }
                    UHF_C_PORT_CONNECTION => {
                        owrite4(sc, port, u32::from(UPS_C_CONNECT_STATUS) << 16)
                    }
                    UHF_C_PORT_ENABLE => owrite4(sc, port, u32::from(UPS_C_PORT_ENABLED) << 16),
                    UHF_C_PORT_SUSPEND => owrite4(sc, port, u32::from(UPS_C_SUSPEND) << 16),
                    UHF_C_PORT_OVER_CURRENT => {
                        owrite4(sc, port, u32::from(UPS_C_OVERCURRENT_INDICATOR) << 16)
                    }
                    UHF_C_PORT_RESET => owrite4(sc, port, u32::from(UPS_C_PORT_RESET) << 16),
                    _ => break 'ret Err(USBD_IOERROR),
                }
                let change = matches!(
                    value,
                    UHF_C_PORT_CONNECTION
                        | UHF_C_PORT_ENABLE
                        | UHF_C_PORT_SUSPEND
                        | UHF_C_PORT_OVER_CURRENT
                        | UHF_C_PORT_RESET
                );
                // Enable RHSC interrupt if condition is cleared.
                if change && oread4(sc, port) >> 16 == 0 {
                    ohci_rhsc_able(sc, true);
                }
            }
            (UR_GET_DESCRIPTOR, UT_READ_CLASS_DEVICE) => {
                if value & 0xff != 0 {
                    break 'ret Err(USBD_IOERROR);
                }
                let mut v = oread4(sc, OHCI_RH_DESCRIPTOR_A);
                let mut hubd = OHCI_HUBD;
                hubd.bNbrPorts = sc.sc_noport.get() as u8;
                usetw(
                    &mut hubd.wHubCharacteristics,
                    if v & OHCI_NPS != 0 {
                        UHD_PWR_NO_SWITCH
                    } else if v & OHCI_PSM != 0 {
                        UHD_PWR_GANGED
                    } else {
                        UHD_PWR_INDIVIDUAL
                    }, // XXX overcurrent
                );
                hubd.bPwrOn2PwrGood = ohci_get_potpgt(v) as u8;
                v = oread4(sc, OHCI_RH_DESCRIPTOR_B);
                // The C's loop advances `i` twice per eight ports (its body has an `i++` of
                // its own).
                let mut i = 0usize;
                let mut l = sc.sc_noport.get();
                while l > 0 {
                    if let Some(b) = hubd.DeviceRemovable.get_mut(i) {
                        *b = v as u8;
                    }
                    i += 2;
                    l -= 8;
                    v >>= 8;
                }
                hubd.bDescLength = (USB_HUB_DESCRIPTOR_SIZE + i) as u8;
                let l = len
                    .min(usize::from(hubd.bDescLength))
                    .min(size_of::<UsbHubDescriptor>());
                totlen = l;
                buf[..l].copy_from_slice(&hubd.as_bytes()[..l]);
            }
            (UR_GET_STATUS, UT_READ_CLASS_DEVICE) => {
                if len != 4 {
                    break 'ret Err(USBD_IOERROR);
                }
                buf.fill(0); // ? XXX
                totlen = len;
            }
            (UR_GET_STATUS, UT_READ_CLASS_OTHER) => {
                if index < 1 || i32::from(index) > sc.sc_noport.get() {
                    break 'ret Err(USBD_IOERROR);
                }
                if len != 4 {
                    break 'ret Err(USBD_IOERROR);
                }
                let v = oread4(sc, ohci_rh_port_status(usize::from(index)));
                let mut ps = UsbPortStatus::zeroed();
                usetw(&mut ps.wPortStatus, v as u16);
                usetw(&mut ps.wPortChange, (v >> 16) as u16);
                let l = len.min(size_of::<UsbPortStatus>());
                buf[..l].copy_from_slice(&ps.as_bytes()[..l]);
                totlen = l;
            }
            (UR_SET_DESCRIPTOR, UT_WRITE_CLASS_DEVICE) => break 'ret Err(USBD_IOERROR),
            (UR_SET_FEATURE, UT_WRITE_CLASS_DEVICE) => {}
            (UR_SET_FEATURE, UT_WRITE_CLASS_OTHER) => {
                if index < 1 || i32::from(index) > sc.sc_noport.get() {
                    break 'ret Err(USBD_IOERROR);
                }
                let port = ohci_rh_port_status(usize::from(index));
                match i32::from(value) {
                    UHF_PORT_ENABLE => owrite4(sc, port, u32::from(UPS_PORT_ENABLED)),
                    UHF_PORT_SUSPEND => owrite4(sc, port, u32::from(UPS_SUSPEND)),
                    UHF_PORT_RESET => {
                        owrite4(sc, port, u32::from(UPS_RESET));
                        for _ in 0..5 {
                            usb_delay_ms(&sc.sc_bus, USB_PORT_ROOT_RESET_DELAY);
                            if sc.sc_bus.dying.get() != 0 {
                                break 'ret Err(USBD_IOERROR);
                            }
                            if oread4(sc, port) & u32::from(UPS_RESET) == 0 {
                                break;
                            }
                        }
                    }
                    UHF_PORT_POWER => owrite4(sc, port, u32::from(UPS_PORT_POWER)),
                    UHF_PORT_DISOWN_TO_1_1 => {
                        // accept, but do nothing
                    }
                    _ => break 'ret Err(USBD_IOERROR),
                }
            }
            _ => break 'ret Err(USBD_IOERROR),
        }
        Ok(totlen)
    };

    let err = match res {
        Ok(totlen) => {
            xfer.actlen.set(totlen as u32);
            USBD_NORMAL_COMPLETION
        }
        Err(e) => e,
    };
    // ret:
    xfer.status.set(err);
    let s = splusb();
    usb_transfer_complete(xfer);
    splx(s);
    err
}

/// `ohci_root_ctrl_abort`: abort a root control request. Nothing to do, all transfers are
/// synchronous.
pub fn ohci_root_ctrl_abort(_xfer: &'static UsbdXfer) {}

/// `ohci_root_ctrl_close`: close the root pipe. Nothing to do.
pub fn ohci_root_ctrl_close(_pipe: &'static UsbdPipe) {}

/// `ohci_root_intr_transfer`.
pub fn ohci_root_intr_transfer(xfer: &'static UsbdXfer) -> UsbdStatus {
    // Insert last in queue.
    let err = usb_insert_transfer(xfer);
    if err.is_err() {
        return err;
    }

    // Pipe isn't running, start first
    ohci_root_intr_start(queue_first(xfer.pipe()))
}

/// `ohci_root_intr_start`: the root hub's interrupt transfer waits for a port change.
pub fn ohci_root_intr_start(xfer: &'static UsbdXfer) -> UsbdStatus {
    let sc = ohci_softc(xfer.device().bus());

    if sc.sc_bus.dying.get() != 0 {
        return USBD_IOERROR;
    }

    sc.sc_intrxfer.set(Some(xfer));

    USBD_IN_PROGRESS
}

/// `ohci_root_intr_abort`.
pub fn ohci_root_intr_abort(xfer: &'static UsbdXfer) {
    let sc = ohci_softc(xfer.device().bus());

    sc.sc_intrxfer.set(None);

    xfer.status.set(USBD_CANCELLED);
    let s = splusb();
    usb_transfer_complete(xfer);
    splx(s);
}

/// `ohci_root_intr_close`.
pub fn ohci_root_intr_close(_pipe: &'static UsbdPipe) {}

/// `ohci_device_ctrl_transfer`.
pub fn ohci_device_ctrl_transfer(xfer: &'static UsbdXfer) -> UsbdStatus {
    // Insert last in queue.
    let err = usb_insert_transfer(xfer);
    if err.is_err() {
        return err;
    }

    // Pipe isn't running, start first
    ohci_device_ctrl_start(queue_first(xfer.pipe()))
}

/// `ohci_device_ctrl_start`.
pub fn ohci_device_ctrl_start(xfer: &'static UsbdXfer) -> UsbdStatus {
    let sc = ohci_softc(xfer.device().bus());

    if sc.sc_bus.dying.get() != 0 {
        return USBD_IOERROR;
    }

    #[cfg(feature = "diagnostic")]
    if xfer.rqflags.get() & crate::dev::usb::usbdivar::URQ_REQUEST == 0 {
        // XXX panic
        printf(format_args!("ohci_device_ctrl_transfer: not a request\n"));
        return USBD_INVAL;
    }

    let err = ohci_device_request(xfer);
    if err.is_err() {
        return err;
    }

    USBD_IN_PROGRESS
}

/// `ohci_device_ctrl_abort`: abort a device control request.
pub fn ohci_device_ctrl_abort(xfer: &'static UsbdXfer) {
    ohci_abort_xfer(xfer, USBD_CANCELLED);
}

/// `ohci_device_ctrl_close`: close a device control pipe.
pub fn ohci_device_ctrl_close(pipe: &'static UsbdPipe) {
    let opipe = ohci_pipe(pipe);
    let sc = ohci_softc(pipe.device().bus());

    ohci_close_pipe(pipe, sc.ctrl_head());
    ohci_free_std(sc, opipe.tail_td());
}

/// `ohci_device_clear_toggle`: the pipe's `cleartoggle`.
pub fn ohci_device_clear_toggle(pipe: &'static UsbdPipe) {
    let opipe = ohci_pipe(pipe);

    ohci_clr(&opipe.sed().ed.ed_headp, OHCI_TOGGLECARRY);
}

/// `ohci_device_bulk_transfer`.
pub fn ohci_device_bulk_transfer(xfer: &'static UsbdXfer) -> UsbdStatus {
    // Insert last in queue.
    let err = usb_insert_transfer(xfer);
    if err.is_err() {
        return err;
    }

    // Pipe isn't running, start first
    ohci_device_bulk_start(queue_first(xfer.pipe()))
}

/// `ohci_device_bulk_start`: the xfer's TDs, from the pipe's old tail, ahead of a new tail.
pub fn ohci_device_bulk_start(xfer: &'static UsbdXfer) -> UsbdStatus {
    let sc = ohci_softc(xfer.device().bus());
    let opipe = ohci_pipe(xfer.pipe());

    if sc.sc_bus.dying.get() != 0 {
        return USBD_IOERROR;
    }

    #[cfg(feature = "diagnostic")]
    if xfer.rqflags.get() & crate::dev::usb::usbdivar::URQ_REQUEST != 0 {
        // XXX panic
        printf(format_args!("ohci_device_bulk_start: a request\n"));
        return USBD_INVAL;
    }

    let len = xfer.length.get();
    let sed = opipe.sed();

    // Update device address
    ohci_set(
        &sed.ed.ed_flags,
        (ohci_get(&sed.ed.ed_flags) & !OHCI_ED_ADDRMASK)
            | ohci_ed_set_fa(u32::from(xfer.device().address.get())),
    );

    // Allocate a chain of new TDs (including a new tail).
    let data = opipe.tail_td();
    let last = match ohci_alloc_std_chain(sc, len, xfer, data) {
        Ok(Some(last)) => last,
        Ok(None) => panic(format_args!("ohci_device_bulk_start: empty TD chain")),
        Err(err) => return err,
    };
    // We want interrupt at the end of the transfer.
    ohci_clr(&last.td.td_flags, OHCI_TD_INTR_MASK);
    ohci_or(&last.td.td_flags, ohci_td_set_di(1));
    last.flags.set(last.flags.get() | OHCI_CALL_DONE);
    // point at sentinel
    let tail = match last.nexttd.get() {
        Some(t) => t,
        None => panic(format_args!("ohci_device_bulk_start: chain without a tail")),
    };

    tail.xfer.set(None);
    set_hcpriv_std(xfer, data);

    // Insert ED in schedule
    let s = splusb();
    let mut tdp = data;
    while !ptr::eq(tdp, tail) {
        tdp.xfer.set(Some(xfer));
        tdp = match tdp.nexttd.get() {
            Some(n) => n,
            None => panic(format_args!("ohci_device_bulk_start: TD chain ends early")),
        };
    }
    ohci_set(&sed.ed.ed_tailp, tail.physaddr.get());
    opipe.tail_td.set(Some(tail));
    ohci_clr(&sed.ed.ed_flags, OHCI_ED_SKIP);
    owrite4(sc, OHCI_COMMAND_STATUS, OHCI_BLF);
    if xfer.timeout.get() != 0 && sc.sc_bus.use_polling.get() == 0 {
        ohci_arm_timeout(xfer);
    }

    splx(s);

    USBD_IN_PROGRESS
}

/// `ohci_device_bulk_abort`.
pub fn ohci_device_bulk_abort(xfer: &'static UsbdXfer) {
    ohci_abort_xfer(xfer, USBD_CANCELLED);
}

/// `ohci_device_bulk_close`: close a device bulk pipe.
pub fn ohci_device_bulk_close(pipe: &'static UsbdPipe) {
    let opipe = ohci_pipe(pipe);
    let sc = ohci_softc(pipe.device().bus());

    ohci_close_pipe(pipe, sc.bulk_head());
    ohci_free_std(sc, opipe.tail_td());
}

/// `ohci_device_intr_transfer`.
pub fn ohci_device_intr_transfer(xfer: &'static UsbdXfer) -> UsbdStatus {
    // Insert last in queue.
    let err = usb_insert_transfer(xfer);
    if err.is_err() {
        return err;
    }

    // Pipe isn't running, start first
    ohci_device_intr_start(queue_first(xfer.pipe()))
}

/// `ohci_device_intr_start`: one TD (the pipe's old tail) for the whole buffer, ahead of a
/// new tail.
pub fn ohci_device_intr_start(xfer: &'static UsbdXfer) -> UsbdStatus {
    let sc = ohci_softc(xfer.device().bus());
    let opipe = ohci_pipe(xfer.pipe());
    let sed = opipe.sed();

    if sc.sc_bus.dying.get() != 0 {
        return USBD_IOERROR;
    }

    #[cfg(feature = "diagnostic")]
    if xfer.rqflags.get() & crate::dev::usb::usbdivar::URQ_REQUEST != 0 {
        panic(format_args!("ohci_device_intr_transfer: a request"));
    }

    xfer_sync(xfer, xfer.length.get(), false);

    let len = xfer.length.get();

    let data = opipe.tail_td();
    let Some(tail) = ohci_alloc_std(sc) else {
        return USBD_NOMEM;
    };
    tail.xfer.set(None);

    ohci_set(
        &data.td.td_flags,
        (if usbd_xfer_isread(xfer) {
            OHCI_TD_IN
        } else {
            OHCI_TD_OUT
        }) | OHCI_TD_NOCC
            | ohci_td_set_di(1)
            | OHCI_TD_TOGGLE_CARRY,
    );
    if xfer.flags.get() & USBD_SHORT_XFER_OK != 0 {
        ohci_or(&data.td.td_flags, OHCI_TD_R);
    }
    ohci_set(&data.td.td_cbp, dmaaddr(&xfer.dmabuf, 0) as u32);
    data.nexttd.set(Some(tail));
    ohci_set(&data.td.td_nexttd, tail.physaddr.get());
    ohci_set(
        &data.td.td_be,
        ohci_get(&data.td.td_cbp).wrapping_add(len).wrapping_sub(1),
    );
    data.len.set(len as u16);
    data.xfer.set(Some(xfer));
    data.flags.set(OHCI_CALL_DONE | OHCI_ADD_LEN);
    set_hcpriv_std(xfer, data);

    // Insert ED in schedule
    let s = splusb();
    ohci_set(&sed.ed.ed_tailp, tail.physaddr.get());
    opipe.tail_td.set(Some(tail));
    ohci_clr(&sed.ed.ed_flags, OHCI_ED_SKIP);
    splx(s);

    USBD_IN_PROGRESS
}

/// `ohci_device_intr_abort`.
pub fn ohci_device_intr_abort(xfer: &'static UsbdXfer) {
    let pipe = xfer.pipe();
    kassert!(pipe.repeat.get() == 0 || pipe.intrxfer.get().is_some_and(|x| ptr::eq(x, xfer)));

    ohci_abort_xfer(xfer, USBD_CANCELLED);
}

/// `ohci_device_intr_close`: close a device interrupt pipe: its ED leaves the interrupt
/// tree and its slots' bandwidth count drops.
pub fn ohci_device_intr_close(pipe: &'static UsbdPipe) {
    let opipe = ohci_pipe(pipe);
    let sc = ohci_softc(pipe.device().bus());
    let nslots = opipe.nslots.get();
    let pos = opipe.pos.get();
    let sed = opipe.sed();

    let s = splusb();
    ohci_or(&sed.ed.ed_flags, OHCI_ED_SKIP);
    if ohci_get(&sed.ed.ed_tailp) & OHCI_HEADMASK != ohci_get(&sed.ed.ed_headp) & OHCI_HEADMASK {
        usb_delay_ms(&sc.sc_bus, 2);
    }

    let mut p = Some(sc.ed(pos as usize));
    while let Some(q) = p {
        if q.next.get().is_some_and(|n| ptr::eq(n, sed)) {
            break;
        }
        p = q.next.get();
    }
    // DIAGNOSTIC's check; without it the C dereferences the NULL.
    let Some(p) = p else {
        panic(format_args!("ohci_device_intr_close: ED not found"));
    };
    p.next.set(sed.next.get());
    ohci_set(&p.ed.ed_nexted, ohci_get(&sed.ed.ed_nexted));
    splx(s);

    for j in 0..nslots {
        let b = &sc.sc_bws[((pos * nslots + j) as usize) % OHCI_NO_INTRS];
        b.set(b.get().wrapping_sub(1));
    }

    ohci_free_std(sc, opipe.tail_td());
    ohci_free_sed(sc, sed);
}

/// `ohci_device_setintr`: hangs an interrupt pipe's ED in the interrupt tree, at the level
/// of its poll interval, below the dummy ED whose slots carry the least load.
pub fn ohci_device_setintr(
    sc: &'static OhciSoftc,
    opipe: &'static OhciPipe,
    ival: i32,
) -> UsbdStatus {
    let sed = opipe.sed();

    if ival == 0 {
        printf(format_args!("ohci_setintr: 0 interval\n"));
        return USBD_INVAL;
    }

    let (best, nslots) = ohci_setintr_slot(&sc.sc_bws, ival);

    let s = splusb();
    let hsed = sc.ed(best);
    sed.next.set(hsed.next.get());
    ohci_set(&sed.ed.ed_nexted, ohci_get(&hsed.ed.ed_nexted));
    hsed.next.set(Some(sed));
    ohci_set(&hsed.ed.ed_nexted, sed.physaddr.get());
    splx(s);

    for j in 0..nslots {
        let b = &sc.sc_bws[(best * nslots + j) % OHCI_NO_INTRS];
        b.set(b.get().wrapping_add(1));
    }
    opipe.nslots.set(nslots as i32);
    opipe.pos.set(best as i32);

    USBD_NORMAL_COMPLETION
}

/// The tree ED `ohci_device_setintr` picks for a poll interval of `ival` ms, and the number
/// of the 32 slots each ED at that level serves.
///
/// We now know which level in the tree the ED must go into. Figure out which slot has most
/// bandwidth left over. Slots to examine:
///
/// ```text
/// npoll
/// 1    0
/// 2    1 2
/// 4    3 4 5 6
/// 8    7 8 9 10 11 12 13 14
/// N    (N-1) .. (N-1+N-1)
/// ```
fn ohci_setintr_slot(bws: &[Cell<u32>; OHCI_NO_INTRS], ival: i32) -> (usize, usize) {
    let mut npoll = OHCI_NO_INTRS;
    // The C compares the unsigned `npoll` with the int `ival` as unsigned.
    while npoll as u32 > ival as u32 {
        npoll /= 2;
    }

    let slow = npoll - 1;
    let shigh = slow + npoll;
    let nslots = OHCI_NO_INTRS / npoll;
    let mut best = slow;
    let mut bestbw = !0u32;
    for i in slow..shigh {
        let mut bw = 0u32;
        for j in 0..nslots {
            bw = bw.wrapping_add(bws[(i * nslots + j) % OHCI_NO_INTRS].get());
        }
        if bw < bestbw {
            best = i;
            bestbw = bw;
        }
    }
    (best, nslots)
}

/// `ohci_device_isoc_transfer`: queues the xfer and puts it in the schedule at once (they
/// are scheduled into future frames), even while another is in progress.
pub fn ohci_device_isoc_transfer(xfer: &'static UsbdXfer) -> UsbdStatus {
    // Put it on our queue,
    let err = usb_insert_transfer(xfer);

    // bail out on error,
    if err.is_err() && err != USBD_IN_PROGRESS {
        return err;
    }

    // XXX should check inuse here

    // insert into schedule,
    ohci_device_isoc_enter(xfer);

    // and start if the pipe wasn't running
    if !err.is_err() {
        ohci_device_isoc_start(queue_first(xfer.pipe()));
    }

    err
}

/// `ohci_device_isoc_enter`: the xfer's frames in ITDs (eight frames and one page crossing
/// each at most), from the pipe's old tail ITD, scheduled from a few frames ahead of the
/// controller or after the pipe's previous xfer.
pub fn ohci_device_isoc_enter(xfer: &'static UsbdXfer) {
    let sc = ohci_softc(xfer.device().bus());
    let opipe = ohci_pipe(xfer.pipe());
    let sed = opipe.sed();

    if sc.sc_bus.dying.get() != 0 {
        return;
    }

    xfer_sync(xfer, xfer.length.get(), false);

    if opipe.iso_next.get() == -1 {
        // Not in use yet, schedule it a few frames ahead.
        opipe
            .iso_next
            .set(ohci_get(&sc.hcca().hcca_frame_number).wrapping_add(5) as i32);
    }

    let mut sitd = opipe.tail_itd();
    let buf = dmaaddr(&xfer.dmabuf, 0) as u32;
    let mut bp0 = ohci_page(buf);
    let mut offs = ohci_page_offset(buf);
    let nframes = xfer.nframes.get();
    set_hcpriv_sitd(xfer, sitd);
    let mut ncur: u32 = 0;
    for i in 0..nframes {
        let noffs = offs.wrapping_add(u32::from(frlength(xfer, i)));
        if ncur as usize == OHCI_ITD_NOFFSET // all offsets used
            || ohci_page(buf.wrapping_add(noffs)) > bp0.wrapping_add(OHCI_PAGE_SIZE)
        // too many page crossings
        {
            // Allocate next ITD
            let Some(nsitd) = ohci_alloc_sitd(sc) else {
                // XXX what now?
                printf(format_args!("{}: isoc TD alloc failed\n", devname(sc)));
                return;
            };

            // Fill current ITD
            ohci_set(
                &sitd.itd.itd_flags,
                OHCI_ITD_NOCC
                    | ohci_itd_set_sf(opipe.iso_next.get() as u32)
                    | ohci_itd_set_di(6) // delay intr a little
                    | ohci_itd_set_fc(ncur),
            );
            ohci_set(&sitd.itd.itd_bp0, bp0);
            sitd.nextitd.set(Some(nsitd));
            ohci_set(&sitd.itd.itd_nextitd, nsitd.physaddr.get());
            ohci_set(&sitd.itd.itd_be, bp0.wrapping_add(offs).wrapping_sub(1));
            sitd.xfer.set(Some(xfer));
            sitd.flags.set(0);

            sitd = nsitd;
            opipe
                .iso_next
                .set(opipe.iso_next.get().wrapping_add(ncur as i32));
            bp0 = ohci_page(buf.wrapping_add(offs));
            ncur = 0;
        }
        if let Some(o) = sitd.itd.itd_offset.get(ncur as usize) {
            ohci_set16(o, ohci_itd_mk_offs(offs));
        }
        offs = noffs;
        ncur += 1;
    }
    let Some(nsitd) = ohci_alloc_sitd(sc) else {
        // XXX what now?
        printf(format_args!("{}: isoc TD alloc failed\n", devname(sc)));
        return;
    };
    // Fixup last used ITD
    ohci_set(
        &sitd.itd.itd_flags,
        OHCI_ITD_NOCC
            | ohci_itd_set_sf(opipe.iso_next.get() as u32)
            | ohci_itd_set_di(0)
            | ohci_itd_set_fc(ncur),
    );
    ohci_set(&sitd.itd.itd_bp0, bp0);
    sitd.nextitd.set(Some(nsitd));
    ohci_set(&sitd.itd.itd_nextitd, nsitd.physaddr.get());
    ohci_set(&sitd.itd.itd_be, bp0.wrapping_add(offs).wrapping_sub(1));
    sitd.xfer.set(Some(xfer));
    sitd.flags.set(OHCI_CALL_DONE);

    opipe
        .iso_next
        .set(opipe.iso_next.get().wrapping_add(ncur as i32));
    opipe.iso_inuse.set(opipe.iso_inuse.get() + nframes);

    xfer.actlen.set(offs); // XXX pretend we did it all

    xfer.status.set(USBD_IN_PROGRESS);

    let s = splusb();
    ohci_set(&sed.ed.ed_tailp, nsitd.physaddr.get());
    opipe.tail_itd.set(Some(nsitd));
    ohci_clr(&sed.ed.ed_flags, OHCI_ED_SKIP);
    splx(s);
}

/// `ohci_device_isoc_start`: the xfer is already in the schedule (`ohci_device_isoc_enter`).
pub fn ohci_device_isoc_start(xfer: &'static UsbdXfer) -> UsbdStatus {
    let sc = ohci_softc(xfer.device().bus());

    if sc.sc_bus.dying.get() != 0 {
        return USBD_IOERROR;
    }

    #[cfg(feature = "diagnostic")]
    if xfer.status.get() != USBD_IN_PROGRESS {
        printf(format_args!(
            "ohci_device_isoc_start: not in progress {:p}\n",
            ptr::from_ref(xfer)
        ));
    }

    USBD_IN_PROGRESS
}

/// `ohci_device_isoc_abort`: skips the pipe's ED, waits for the frames in flight, completes
/// the xfer and unlinks its ITDs.
pub fn ohci_device_isoc_abort(xfer: &'static UsbdXfer) {
    let sc = ohci_softc(xfer.device().bus());
    let opipe = ohci_pipe(xfer.pipe());

    let s = splusb();

    // Transfer is already done.
    if xfer.status.get() != USBD_NOT_STARTED && xfer.status.get() != USBD_IN_PROGRESS {
        splx(s);
        printf(format_args!("ohci_device_isoc_abort: early return\n"));
        return;
    }

    // Give xfer the requested abort code.
    xfer.status.set(USBD_CANCELLED);

    let sed = opipe.sed();
    ohci_or(&sed.ed.ed_flags, OHCI_ED_SKIP); // force hardware skip

    let Some(mut sitd) = hcpriv_sitd(xfer) else {
        #[cfg(feature = "diagnostic")]
        {
            splx(s);
            printf(format_args!("ohci_device_isoc_abort: hcpriv==0\n"));
            return;
        }
        #[cfg(not(feature = "diagnostic"))]
        panic(format_args!("ohci_device_isoc_abort: hcpriv==0"));
    };
    while same_xfer(sitd.xfer.get(), xfer) {
        #[cfg(feature = "diagnostic")]
        sitd.isdone.set(1);
        sitd = match sitd.nextitd.get() {
            Some(n) => n,
            None => panic(format_args!("ohci_device_isoc_abort: ITD chain ends early")),
        };
    }

    splx(s);

    usb_delay_ms(&sc.sc_bus, OHCI_ITD_NOFFSET as u32);

    let s = splusb();

    // Run callback.
    usb_transfer_complete(xfer);

    ohci_set(&sed.ed.ed_headp, sitd.physaddr.get()); // unlink TDs
    ohci_clr(&sed.ed.ed_flags, OHCI_ED_SKIP); // remove hardware skip

    splx(s);
}

/// `ohci_device_isoc_done`.
pub fn ohci_device_isoc_done(_xfer: &'static UsbdXfer) {}

/// `ohci_setup_isoc`: an isochronous pipe's ED goes on the isochronous list.
pub fn ohci_setup_isoc(pipe: &'static UsbdPipe) -> UsbdStatus {
    let opipe = ohci_pipe(pipe);
    let sc = ohci_softc(pipe.device().bus());

    opipe.iso_next.set(-1);
    opipe.iso_inuse.set(0);

    let s = splusb();
    ohci_add_ed(opipe.sed(), sc.isoc_head());
    splx(s);

    USBD_NORMAL_COMPLETION
}

/// `ohci_device_isoc_close`.
pub fn ohci_device_isoc_close(pipe: &'static UsbdPipe) {
    let opipe = ohci_pipe(pipe);
    let sc = ohci_softc(pipe.device().bus());

    ohci_close_pipe(pipe, sc.isoc_head());
    #[cfg(feature = "diagnostic")]
    opipe.tail_itd().isdone.set(1);
    ohci_free_sitd(sc, opipe.tail_itd());
}

const _: () = {
    // `pool_init(ohcixfer, sizeof(struct ohci_xfer), 0, ...)`: the pool's default alignment
    // (`ALIGN(1)`, at least 8) must suit the type `ohci_allocx` casts its items to.
    assert!(align_of::<OhciXfer>() <= 8);
    // The chunks are larger than `USB_MEM_SMALL`, so each gets a page-aligned, physically
    // contiguous block of its own from `usb_allocmem`, as `ohci_carve` needs.
    assert!(OHCI_SED_SIZE * OHCI_SED_CHUNK > crate::dev::usb::usb_mem::USB_MEM_SMALL);
    assert!(OHCI_STD_SIZE * OHCI_STD_CHUNK > crate::dev::usb::usb_mem::USB_MEM_SMALL);
    assert!(OHCI_SITD_SIZE * OHCI_SITD_CHUNK > crate::dev::usb::usb_mem::USB_MEM_SMALL);
    assert!(OHCI_HCCA_SIZE > crate::dev::usb::usb_mem::USB_MEM_SMALL);
};
/* </CODE> */

/* <TESTS> */
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn revbits_reverses_five_bits() {
        for (i, &r) in REVBITS.iter().enumerate() {
            let mut want = 0u8;
            for b in 0..5 {
                if i & (1 << b) != 0 {
                    want |= 1 << (4 - b);
                }
            }
            assert_eq!(r, want, "revbits[{i}]");
        }
    }

    #[test]
    fn interrupt_tree_levels() {
        // ED i hangs below ED (i-1)/2; the last 32 are the leaves the HCCA points at.
        assert_eq!(OHCI_NO_EDS - OHCI_NO_INTRS, 31);
        let parent = |i: usize| (i - 1) / 2;
        assert_eq!(parent(31), 15);
        assert_eq!(parent(62), 30);
        assert_eq!(parent(2), 0);
    }

    #[test]
    fn setintr_picks_the_least_loaded_slot() {
        let bws: [Cell<u32>; OHCI_NO_INTRS] = core::array::from_fn(|_| Cell::new(0));
        // A 10 ms interval polls every 8 ms: level 8 (EDs 7..14), 4 slots each.
        assert_eq!(ohci_setintr_slot(&bws, 10), (7, 4));
        // 1 ms: the root ED, all 32 slots; 255 ms: a leaf, one slot.
        assert_eq!(ohci_setintr_slot(&bws, 1), (0, 32));
        assert_eq!(ohci_setintr_slot(&bws, 255), (31, 1));
        // Load ED 7's slots (28..31): ED 8 is chosen.
        for j in 0..4 {
            bws[(7 * 4 + j) % OHCI_NO_INTRS].set(1);
        }
        assert_eq!(ohci_setintr_slot(&bws, 10), (8, 4));
    }

    #[test]
    fn hash_buckets() {
        assert_eq!(ohci_hash(0x0010), 1);
        assert_eq!(ohci_hash(0x0800), 0);
        assert_eq!(ohci_hash(0x1234_5670), (0x0123_4567 % 128) as usize);
    }

    #[test]
    fn root_hub_descriptors() {
        assert_eq!(OHCI_DEVD.as_bytes().len(), USB_DEVICE_DESCRIPTOR_SIZE);
        assert_eq!(OHCI_DEVD.bcdUSB, [0x00, 0x01]);
        assert_eq!(OHCI_DEVD.bDeviceProtocol, UDPROTO_FSHUB);
        assert_eq!(
            usize::from(ugetw(OHCI_CONFD.wTotalLength)),
            USB_CONFIG_DESCRIPTOR_SIZE
                + USB_INTERFACE_DESCRIPTOR_SIZE
                + USB_ENDPOINT_DESCRIPTOR_SIZE
        );
        assert_eq!(OHCI_ENDPD.bEndpointAddress, 0x81);
        assert_eq!(OHCI_ENDPD.bInterval, 255);
        assert_eq!(OHCI_HUBD.bDescriptorType, UDESC_HUB);
    }
}
/* </TESTS> */
