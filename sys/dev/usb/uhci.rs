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
//! uhci(4): the Universal Host Controller Interface (USB 1.x) driver, machine independent
//! (`dev/usb/uhci.c`). The bus front-end (`dev/pci/uhci_pci.rs`) maps the I/O registers,
//! establishes the interrupt, calls [`uhci_init`] and attaches `usb(4)`.
//!
//! Upstream: sys/dev/usb/uhci.c @ 3ce1f3f79392
//!
//! The controller walks a frame list of 1024 links in DMA memory, one per 1 ms frame. The
//! driver points every entry at one of `UHCI_VFRAMELIST_COUNT` (128) inactive TDs, the
//! virtual frame list (`uhcivar.rs`): an isochronous pipe hangs one TD per virtual frame after
//! it, an interrupt pipe hangs a QH in every `ival`-th slot's interrupt list, and every slot
//! then leads to the shared control queues (low speed, then full speed), the bulk queue and
//! a last QH with an inactive TD after it. A transfer is a chain of TDs hung on its pipe's QH
//! (`qh_elink`). While a bulk QH is queued the last QH loops back to the full speed control
//! QH ("bandwidth reclamation", `uhci_add_loop`). The interrupt only says that something
//! finished: the soft interrupt scans every active transfer (`sc_intrhead`). The root hub
//! is emulated (`uhci_root_ctrl_start`) and its port changes are polled every 255 ms
//! (`uhci_poll_hub`), since UHCI has no port change interrupt.
//!
//! Locking is the C's: everything runs under the kernel lock at `splusb()`, the hard
//! interrupt (`uhci_intr`, established at `IPL_USB` without `IPL_MPSAFE`) included.
//!
//! ## Deviations
//! - `UHCI_DEBUG` is not in GENERIC: the `DPRINTF`s, the `uhci_dump_*` functions,
//!   `uhcinoloop` and `thesc` are absent, as in a kernel built without it; so are `UREAD4`,
//!   `UHCISTS` and `UHCI_CURFRAME`, which only that code uses. `UHCI_CTL_LOOP` is not
//!   defined in C either: control QHs do not loop. The `DIAGNOSTIC` blocks (`isdone`, the
//!   `TD_IS_FREE` check of `uhci_free_std`, the start functions' request checks, the QH
//!   search's end check, `uhci_detach`'s assertion, ...) are under feature `diagnostic`.
//! - `struct pool *uhcixfer`, `malloc`ed by the first `uhci_init`, is a static [`Pool`] with
//!   a flag for the C's `uhcixfer == NULL`; its "unable to allocate pool descriptor" path
//!   (which returns the errno `ENOMEM` as a `usbd_status`) cannot happen.
//! - `UREAD1(sc, r)`, `UWRITE2(sc, r, x)` and the other accessor macros are functions of the
//!   same names in lower case taking the softc ([`uread1`], [`uwrite2`], ...); `UHCICMD` is
//!   [`uhcicmd`].
//! - The casts `(struct uhci_softc *)bus`, `(struct uhci_pipe *)pipe` and `(struct uhci_xfer
//!   *)xfer` are checked: [`uhci_softc`] checks the bus's methods are this driver's, the pipe
//!   and xfer casts go through `UsbdPipe::hc`/`UsbdXfer::hc`.
//! - `struct uhci_pipe`'s union `u` is split into its members, prefixed by the arm where two
//!   arms share a name (`ctl_sqh`, `bulk_sqh`, ...). The interrupt pipe's `qhs` and the
//!   isochronous pipe's `stds` stay `malloc`ed arrays, reached through bounds-checked
//!   accessors.
//! - `uhci_alloc_std_chain` returns the chain's first and last TD instead of filling
//!   `sp`/`ep`. `uhci_find_prev_qh` returns `None` where the C returns NULL (its
//!   `DIAGNOSTIC` end check) or would follow a NULL `hlink`; the callers panic then, where the
//!   C dereferences the NULL.
//! - Walking a TD chain that ends (a NULL `link.std`, or one that holds a QH) before the C's
//!   stop condition stops the walk, where the C would dereference the NULL: in
//!   `uhci_check_intr` the xfer counts as still active, `uhci_free_std_chain` stops freeing.
//! - `uhci_device_intr_done` requeues with `uhci_alloc_std_chain`, which "cannot fail since
//!   we freed the chain above"; should it fail anyway the Rust panics, where the C would use
//!   the uninitialised `data`.
//! - The root hub's string descriptors are built in a local `usb_string_descriptor_t` and
//!   copied, as in `xhci.rs` and `ehci.rs`; the hub descriptor copies at most the structure's
//!   own size.
//! - Arming an xfer's timeout (`timeout_del`, `timeout_set`, `timeout_add_msec`) is
//!   [`uhci_arm_timeout`], shared by the start functions.

use core::cell::Cell;
use core::ffi::c_void;
use core::ptr::{self, NonNull};
use core::sync::atomic::{AtomicBool, Ordering};

use crate::dev::usb::uhcireg::*;
use crate::dev::usb::uhcivar::{
    UHCI_SQH_CHUNK, UHCI_SQH_SIZE, UHCI_STD_CHUNK, UHCI_STD_SIZE, UHCI_VFRAMELIST_COUNT,
    UhciSoftQh, UhciSoftTd, UhciSoftTdQh, UhciSoftc, UhciXfer, UhciXferIntrList,
};
use crate::dev::usb::usb::{
    UC_BUS_POWERED, UC_SELF_POWERED, UDCLASS_HUB, UDESC_CONFIG, UDESC_DEVICE, UDESC_ENDPOINT,
    UDESC_HUB, UDESC_INTERFACE, UDESC_STRING, UDPROTO_FSHUB, UDS_SELF_POWERED, UDSUBCLASS_HUB,
    UE_BULK, UE_CONTROL, UE_DIR_IN, UE_INTERRUPT, UE_ISOCHRONOUS, UHD_OC_INDIVIDUAL,
    UHD_PWR_NO_SWITCH, UHF_C_PORT_CONNECTION, UHF_C_PORT_ENABLE, UHF_C_PORT_OVER_CURRENT,
    UHF_C_PORT_RESET, UHF_PORT_DISOWN_TO_1_1, UHF_PORT_ENABLE, UHF_PORT_POWER, UHF_PORT_RESET,
    UHF_PORT_SUSPEND, UICLASS_HUB, UIPROTO_FSHUB, UISUBCLASS_HUB, UPS_C_CONNECT_STATUS,
    UPS_C_OVERCURRENT_INDICATOR, UPS_C_PORT_ENABLED, UPS_C_PORT_RESET, UPS_CURRENT_CONNECT_STATUS,
    UPS_LOW_SPEED, UPS_OVERCURRENT_INDICATOR, UPS_PORT_ENABLED, UPS_PORT_POWER, UPS_SUSPEND,
    UR_CLEAR_FEATURE, UR_GET_BUS_STATE, UR_GET_CONFIG, UR_GET_DESCRIPTOR, UR_GET_INTERFACE,
    UR_GET_STATUS, UR_SET_ADDRESS, UR_SET_CONFIG, UR_SET_DESCRIPTOR, UR_SET_FEATURE,
    UR_SET_INTERFACE, UR_SYNCH_FRAME, USB_BUS_RESET_DELAY, USB_CONFIG_DESCRIPTOR_SIZE,
    USB_CONTROL_ENDPOINT, USB_DEVICE_DESCRIPTOR_SIZE, USB_ENDPOINT_DESCRIPTOR_SIZE,
    USB_HUB_DESCRIPTOR_SIZE, USB_INTERFACE_DESCRIPTOR_SIZE, USB_MAX_DEVICES, USB_PORT_RESET_DELAY,
    USB_PORT_ROOT_RESET_DELAY, USB_RESUME_DELAY, USB_RESUME_RECOVERY, USB_RESUME_WAIT,
    USB_SPEED_LOW, USBD_SHORT_XFER_OK, UT_READ_CLASS_DEVICE, UT_READ_CLASS_OTHER, UT_READ_DEVICE,
    UT_READ_ENDPOINT, UT_READ_INTERFACE, UT_WRITE_CLASS_DEVICE, UT_WRITE_CLASS_OTHER,
    UT_WRITE_DEVICE, UT_WRITE_ENDPOINT, UT_WRITE_INTERFACE, UsbConfigDescriptor,
    UsbDeviceDescriptor, UsbDeviceRequest, UsbEndpointDescriptor, UsbHubDescriptor,
    UsbInterfaceDescriptor, UsbPortStatus, UsbStringDescriptor, UsbWire, ue_get_dir,
    ue_get_xfertype, ugetw, usetw,
};
use crate::dev::usb::usb::{usb_add_task, usb_rem_task, usb_schedsoftintr};
use crate::dev::usb::usb_mem::{dmaaddr, kernaddr, usb_allocmem, usb_syncmem};
use crate::dev::usb::usb_subr::{usb_delay_ms, usbd_set_address};
use crate::dev::usb::usbdi::{
    IPL_SOFTUSB, USB_TASK_TYPE_ABORT, USBD_CANCELLED, USBD_DEFAULT_INTERVAL, USBD_FORCE_SHORT_XFER,
    USBD_IN_PROGRESS, USBD_INVAL, USBD_IOERROR, USBD_NOMEM, USBD_NORMAL_COMPLETION,
    USBD_NOT_STARTED, USBD_STALLED, USBD_TIMEOUT, UsbdStatus, splhardusb, splusb, usb_init_task,
    usb_insert_transfer, usb_transfer_complete, usbd_str,
};
use crate::dev::usb::usbdivar::{
    USB_DMA_COHERENT, UsbDma, UsbdBus, UsbdBusMethods, UsbdHcPipe, UsbdPipe, UsbdPipeMethods,
    UsbdXfer, usbd_bus_set_hc_types, usbd_xfer_isread,
};
use crate::kassert;
use crate::kern::kern_malloc::{free, mallocarray};
use crate::kern::kern_synch::{tsleep_nsec, wakeup};
use crate::kern::kern_timeout::{timeout_add_msec, timeout_del, timeout_set};
use crate::kern::subr_autoconf::{config_activate_children, config_detach_children};
use crate::kern::subr_pool::{pool_get, pool_init, pool_put};
use crate::kern::subr_prf::{panic, printf};
use crate::machine::bus::{
    BUS_DMASYNC_POSTREAD, BUS_DMASYNC_POSTWRITE, BUS_DMASYNC_PREREAD, BUS_DMASYNC_PREWRITE,
    BUS_SPACE_BARRIER_READ, BUS_SPACE_BARRIER_WRITE, BusSize, bus_space_barrier, bus_space_read_1,
    bus_space_read_2, bus_space_write_1, bus_space_write_2, bus_space_write_4,
};
use crate::machine::cpu::{curproc, delay};
use crate::machine::intr::{splsoftassert, splx};
use crate::sys::device::{
    CD_SKIPHIBERNATE, Cfdriver, DV_DULL, DVACT_POWERDOWN, DVACT_RESUME, DVACT_SUSPEND, Device,
};
use crate::sys::errno::Errno;
use crate::sys::malloc::{M_NOWAIT, M_USBHC, M_WAITOK};
use crate::sys::param::PZERO;
use crate::sys::pool::{PR_NOWAIT, PR_ZERO, Pool};
use crate::sys::queue::ListHead;
use crate::sys::systm::INFSLP;

/// `UHCI_RESET_TIMEOUT`: ms, reset timeout.
const UHCI_RESET_TIMEOUT: u32 = 100;

/// `UHCI_INTR_ENDPT`: the root hub's interrupt endpoint.
const UHCI_INTR_ENDPT: u8 = 1;

/// `TD_IS_FREE` (`DIAGNOSTIC`): the token a freed TD carries.
#[cfg(feature = "diagnostic")]
const TD_IS_FREE: u32 = 0x12345678;

/// `struct uhci_pipe`: a pipe of this controller (`u` split into its members).
#[repr(C)]
pub struct UhciPipe {
    /// `pipe`: the generic pipe, first.
    pub pipe: UsbdPipe,
    /// `nexttoggle`.
    pub nexttoggle: Cell<i32>,

    /* Control pipe */
    /// `u.ctl.sqh`.
    pub ctl_sqh: Cell<Option<&'static UhciSoftQh>>,
    /// `u.ctl.reqdma`: the SETUP packet.
    pub reqdma: UsbDma,
    /// `u.ctl.setup`.
    pub setup: Cell<Option<&'static UhciSoftTd>>,
    /// `u.ctl.stat`.
    pub stat: Cell<Option<&'static UhciSoftTd>>,
    /// `u.ctl.length`.
    pub ctl_length: Cell<u32>,

    /* Interrupt pipe */
    /// `u.intr.npoll`.
    pub npoll: Cell<i32>,
    /// `u.intr.isread`.
    pub intr_isread: Cell<i32>,
    /// `u.intr.qhs`: `npoll` QHs, `malloc`ed.
    pub qhs: Cell<*mut Option<&'static UhciSoftQh>>,

    /* Bulk pipe */
    /// `u.bulk.sqh`.
    pub bulk_sqh: Cell<Option<&'static UhciSoftQh>>,
    /// `u.bulk.length`.
    pub bulk_length: Cell<u32>,
    /// `u.bulk.isread`.
    pub bulk_isread: Cell<i32>,

    /* Iso pipe */
    /// `u.iso.stds`: `UHCI_VFRAMELIST_COUNT` TDs, `malloc`ed.
    pub stds: Cell<*mut Option<&'static UhciSoftTd>>,
    /// `u.iso.next`.
    pub iso_next: Cell<i32>,
    /// `u.iso.inuse`.
    pub iso_inuse: Cell<i32>,
}

impl UhciPipe {
    /// `upipe->u.intr.qhs[i]`, checked against `npoll`.
    fn qh(&self, i: i32) -> &'static UhciSoftQh {
        let base = self.qhs.get();
        if base.is_null() || i < 0 || i >= self.npoll.get() {
            panic(format_args!("uhci: interrupt QH {i} outside the pipe's"));
        }
        // SAFETY: `uhci_device_setintr` wrote `npoll` entries in the array it allocated, which
        // `uhci_device_intr_close` frees last; the index is checked above.
        match unsafe { *base.add(i as usize) } {
            Some(q) => q,
            None => panic(format_args!("uhci: interrupt pipe without QH {i}")),
        }
    }

    /// `upipe->u.iso.stds[i]`, checked against `UHCI_VFRAMELIST_COUNT`.
    fn std(&self, i: i32) -> Option<&'static UhciSoftTd> {
        let base = self.stds.get();
        if base.is_null() || i < 0 || i as usize >= UHCI_VFRAMELIST_COUNT {
            panic(format_args!("uhci: isochronous TD {i} outside the pipe's"));
        }
        // SAFETY: `uhci_setup_isoc` wrote `UHCI_VFRAMELIST_COUNT` entries in the array it
        // allocated, which `uhci_device_isoc_close` frees last; the index is checked above.
        unsafe { *base.add(i as usize) }
    }

    /// `upipe->u.ctl.sqh`, `upipe->u.bulk.sqh`. Panics on a pipe without one (the C's NULL
    /// dereference).
    fn sqh_of(q: &Cell<Option<&'static UhciSoftQh>>) -> &'static UhciSoftQh {
        match q.get() {
            Some(q) => q,
            None => panic(format_args!("uhci: pipe without a queue head")),
        }
    }

    /// `upipe->u.ctl.setup`, `upipe->u.ctl.stat`.
    fn td_of(t: &Cell<Option<&'static UhciSoftTd>>) -> &'static UhciSoftTd {
        match t.get() {
            Some(t) => t,
            None => panic(format_args!("uhci: control pipe without its TDs")),
        }
    }
}

// SAFETY: `#[repr(C)]` with the `usbd_pipe` first; the other members are `Cell`s of
// integers, of `Option`s of references and of raw pointers, and a `usb_dma` of such `Cell`s:
// all valid as zero bits (`usbd_setup_pipe`'s `M_ZERO`).
unsafe impl UsbdHcPipe for UhciPipe {}

/// `uhci_cd`.
pub static UHCI_CD: Cfdriver = Cfdriver::new(b"uhci", DV_DULL, CD_SKIPHIBERNATE);

/// `uhcixfer`: the pool of [`UhciXfer`]s.
static UHCIXFER: Pool = Pool::new();
/// `uhcixfer != NULL`: the pool is initialised.
static UHCIXFER_INIT: AtomicBool = AtomicBool::new(false);

/// `uhci_bus_methods`.
pub static UHCI_BUS_METHODS: UsbdBusMethods = UsbdBusMethods {
    open_pipe: uhci_open,
    dev_setaddr: Some(usbd_set_address),
    soft_intr: uhci_softintr,
    do_poll: uhci_poll,
    allocx: uhci_allocx,
    freex: uhci_freex,
};

/// `uhci_root_ctrl_methods`.
static UHCI_ROOT_CTRL_METHODS: UsbdPipeMethods = UsbdPipeMethods {
    transfer: uhci_root_ctrl_transfer,
    start: uhci_root_ctrl_start,
    abort: uhci_root_ctrl_abort,
    close: uhci_root_ctrl_close,
    cleartoggle: None,
    done: uhci_root_ctrl_done,
};

/// `uhci_root_intr_methods`.
static UHCI_ROOT_INTR_METHODS: UsbdPipeMethods = UsbdPipeMethods {
    transfer: uhci_root_intr_transfer,
    start: uhci_root_intr_start,
    abort: uhci_root_intr_abort,
    close: uhci_root_intr_close,
    cleartoggle: None,
    done: uhci_root_intr_done,
};

/// `uhci_device_ctrl_methods`.
static UHCI_DEVICE_CTRL_METHODS: UsbdPipeMethods = UsbdPipeMethods {
    transfer: uhci_device_ctrl_transfer,
    start: uhci_device_ctrl_start,
    abort: uhci_device_ctrl_abort,
    close: uhci_device_ctrl_close,
    cleartoggle: None,
    done: uhci_device_ctrl_done,
};

/// `uhci_device_intr_methods`.
static UHCI_DEVICE_INTR_METHODS: UsbdPipeMethods = UsbdPipeMethods {
    transfer: uhci_device_intr_transfer,
    start: uhci_device_intr_start,
    abort: uhci_device_intr_abort,
    close: uhci_device_intr_close,
    cleartoggle: Some(uhci_device_clear_toggle),
    done: uhci_device_intr_done,
};

/// `uhci_device_bulk_methods`.
static UHCI_DEVICE_BULK_METHODS: UsbdPipeMethods = UsbdPipeMethods {
    transfer: uhci_device_bulk_transfer,
    start: uhci_device_bulk_start,
    abort: uhci_device_bulk_abort,
    close: uhci_device_bulk_close,
    cleartoggle: Some(uhci_device_clear_toggle),
    done: uhci_device_bulk_done,
};

/// `uhci_device_isoc_methods`.
static UHCI_DEVICE_ISOC_METHODS: UsbdPipeMethods = UsbdPipeMethods {
    transfer: uhci_device_isoc_transfer,
    start: uhci_device_isoc_start,
    abort: uhci_device_isoc_abort,
    close: uhci_device_isoc_close,
    cleartoggle: None,
    done: uhci_device_isoc_done,
};

/*
 * Data structures and routines to emulate the root hub.
 */
/// `uhci_devd`.
static UHCI_DEVD: UsbDeviceDescriptor = UsbDeviceDescriptor {
    bLength: USB_DEVICE_DESCRIPTOR_SIZE as u8,
    bDescriptorType: UDESC_DEVICE,   // type
    bcdUSB: [0x00, 0x01],            // USB version
    bDeviceClass: UDCLASS_HUB,       // class
    bDeviceSubClass: UDSUBCLASS_HUB, // subclass
    bDeviceProtocol: UDPROTO_FSHUB,  // protocol
    bMaxPacketSize: 64,              // max packet
    idVendor: [0, 0],                // device id
    idProduct: [0, 0],
    bcdDevice: [0x00, 0x01],
    iManufacturer: 1, // string indices
    iProduct: 2,
    iSerialNumber: 0,
    bNumConfigurations: 1, // # of configurations
};

/// `uhci_confd`.
static UHCI_CONFD: UsbConfigDescriptor = UsbConfigDescriptor {
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

/// `uhci_ifcd`.
static UHCI_IFCD: UsbInterfaceDescriptor = UsbInterfaceDescriptor {
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

/// `uhci_endpd`.
static UHCI_ENDPD: UsbEndpointDescriptor = UsbEndpointDescriptor {
    bLength: USB_ENDPOINT_DESCRIPTOR_SIZE as u8,
    bDescriptorType: UDESC_ENDPOINT,
    bEndpointAddress: UE_DIR_IN | UHCI_INTR_ENDPT,
    bmAttributes: UE_INTERRUPT,
    wMaxPacketSize: [8, 0],
    bInterval: 255,
};

/// `uhci_hubd_piix`.
static UHCI_HUBD_PIIX: UsbHubDescriptor = UsbHubDescriptor {
    bDescLength: USB_HUB_DESCRIPTOR_SIZE as u8,
    bDescriptorType: UDESC_HUB,
    bNbrPorts: 2,
    wHubCharacteristics: [(UHD_PWR_NO_SWITCH | UHD_OC_INDIVIDUAL) as u8, 0],
    bPwrOn2PwrGood: 50, // power on to power good
    bHubContrCurrent: 0,
    DeviceRemovable: [0; 32], // both ports are removable
};

/// `(struct uhci_softc *)bus`: the softc whose `sc_bus` this is. Panics on a bus of another
/// controller.
pub fn uhci_softc(bus: &'static UsbdBus) -> &'static UhciSoftc {
    if !bus
        .methods
        .get()
        .is_some_and(|m| ptr::eq(m, &UHCI_BUS_METHODS))
    {
        panic(format_args!("{}: not a uhci bus", bus.bdev.xname()));
    }
    // SAFETY: only `uhci_init` installs `UHCI_BUS_METHODS`, on the `usbd_bus` that heads a
    // `UhciSoftc` (`#[repr(C)]`, bus first), which lives as long as the bus.
    unsafe { &*ptr::from_ref(bus).cast::<UhciSoftc>() }
}

/// `(struct uhci_pipe *)pipe`.
fn uhci_pipe(pipe: &'static UsbdPipe) -> &'static UhciPipe {
    pipe.hc::<UhciPipe>()
}

/// `(struct uhci_xfer *)xfer`.
fn uhci_xfer(xfer: &'static UsbdXfer) -> &'static UhciXfer {
    xfer.hc::<UhciXfer>()
}

/// `sc->sc_bus.bdev.dv_xname`.
fn devname(sc: &UhciSoftc) -> &str {
    sc.sc_bus.bdev.xname()
}

/// `SIMPLEQ_FIRST(&pipe->queue)` of a pipe the caller just queued on.
fn queue_first(pipe: &'static UsbdPipe) -> &'static UsbdXfer {
    match pipe.queue.first() {
        Some(x) => x,
        None => panic(format_args!("uhci: empty pipe queue")),
    }
}

/// `UBARR(sc)`: a read/write barrier over the registers.
fn ubarr(sc: &UhciSoftc) {
    let (t, h) = sc.regs();
    bus_space_barrier(
        t,
        h,
        0,
        sc.sc_size.get(),
        BUS_SPACE_BARRIER_READ | BUS_SPACE_BARRIER_WRITE,
    );
}

/// `UWRITE1(sc, r, x)`.
pub fn uwrite1(sc: &UhciSoftc, r: BusSize, x: u8) {
    ubarr(sc);
    let (t, h) = sc.regs();
    bus_space_write_1(t, h, r, x);
}

/// `UWRITE2(sc, r, x)`.
pub fn uwrite2(sc: &UhciSoftc, r: BusSize, x: u16) {
    ubarr(sc);
    let (t, h) = sc.regs();
    bus_space_write_2(t, h, r, x);
}

/// `UWRITE4(sc, r, x)`.
pub fn uwrite4(sc: &UhciSoftc, r: BusSize, x: u32) {
    ubarr(sc);
    let (t, h) = sc.regs();
    bus_space_write_4(t, h, r, x);
}

/// `UREAD1(sc, r)`.
pub fn uread1(sc: &UhciSoftc, r: BusSize) -> u8 {
    ubarr(sc);
    let (t, h) = sc.regs();
    bus_space_read_1(t, h, r)
}

/// `UREAD2(sc, r)`.
pub fn uread2(sc: &UhciSoftc, r: BusSize) -> u16 {
    ubarr(sc);
    let (t, h) = sc.regs();
    bus_space_read_2(t, h, r)
}

/// `UHCICMD(sc, cmd)`.
pub fn uhcicmd(sc: &UhciSoftc, cmd: u16) {
    uwrite2(sc, UHCI_CMD, cmd);
}

/// `uhci_add_intr_list(sc, ex)`: `LIST_INSERT_HEAD(&sc->sc_intrhead, ex, inext)`.
fn uhci_add_intr_list(sc: &'static UhciSoftc, ux: &'static UhciXfer) {
    // SAFETY: the xfer is on no list (it is started once and leaves the list when it is
    // done); xfers are pool items that stay in place until freed, and a freed xfer is not
    // active; at splusb() under the kernel lock.
    unsafe { sc.sc_intrhead.insert_head(ux) };
}

/// `uhci_del_intr_list(ex)`: `LIST_REMOVE` and `le_prev = NULL`.
fn uhci_del_intr_list(ux: &'static UhciXfer) {
    // SAFETY: the callers test `uhci_active_intr_list` first, and `le_prev` is cleared after
    // every removal, so the xfer is on `sc_intrhead`, the only list it joins; at splusb()
    // under the kernel lock.
    unsafe {
        ListHead::<UhciXferIntrList>::remove(ux);
        ux.inext.clear_prev();
    }
}

/// `uhci_active_intr_list(ex)`: `le_prev != NULL`.
fn uhci_active_intr_list(ux: &UhciXfer) -> bool {
    ux.inext.is_linked()
}

/// `uhci_find_prev_qh`: the QH before `sqh` in the list that starts at `pqh` (see the
/// module's deviations for `None`).
fn uhci_find_prev_qh(
    pqh: &'static UhciSoftQh,
    sqh: &'static UhciSoftQh,
) -> Option<&'static UhciSoftQh> {
    let mut pqh = pqh;
    loop {
        let next = pqh.hlink.get();
        if next.is_some_and(|h| ptr::eq(h, sqh)) {
            return Some(pqh);
        }
        #[cfg(feature = "diagnostic")]
        if uhci_get(&pqh.qh.qh_hlink) & UHCI_PTR_T != 0 {
            printf(format_args!("uhci_find_prev_qh: QH not found\n"));
            return None;
        }
        pqh = next?;
    }
}

/// [`uhci_find_prev_qh`] for the callers that dereference its result.
fn uhci_prev_qh(pqh: &'static UhciSoftQh, sqh: &'static UhciSoftQh) -> &'static UhciSoftQh {
    match uhci_find_prev_qh(pqh, sqh) {
        Some(p) => p,
        None => panic(format_args!("uhci: QH not found in its list")),
    }
}

/// A softc's dummy or last QH, set by `uhci_init`. Panics before.
fn sc_qh(q: &Cell<Option<&'static UhciSoftQh>>) -> &'static UhciSoftQh {
    match q.get() {
        Some(q) => q,
        None => panic(format_args!("uhci: schedule not initialised")),
    }
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
        panic(format_args!("uhci: frame {i} outside the xfer"));
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

/// `memset(&std->td, 0, sizeof(struct uhci_td))`.
fn td_clear(std: &UhciSoftTd) {
    uhci_set(&std.td.td_link, 0);
    uhci_set(&std.td.td_status, 0);
    uhci_set(&std.td.td_token, 0);
    uhci_set(&std.td.td_buffer, 0);
}

/// `td->td_status &= htole32(~bits)`.
fn td_status_clear(std: &UhciSoftTd, bits: u32) {
    let st = &std.td.td_status;
    uhci_set(st, uhci_get(st) & !bits);
}

/// `td->td_status |= htole32(bits)`.
fn td_status_set(std: &UhciSoftTd, bits: u32) {
    let st = &std.td.td_status;
    uhci_set(st, uhci_get(st) | bits);
}

/// The DMA buffer's direction for `usb_syncmem`.
fn sync_ops(xfer: &UsbdXfer, read: i32, write: i32) -> i32 {
    if usbd_xfer_isread(xfer) { read } else { write }
}

/// `uhci_globalreset`.
pub fn uhci_globalreset(sc: &UhciSoftc) {
    uhcicmd(sc, UHCI_CMD_GRESET); // global reset
    usb_delay_ms(&sc.sc_bus, USB_BUS_RESET_DELAY); // wait a little
    uhcicmd(sc, 0); // do nothing
}

/// `uhci_init`: resets the controller, builds the frame list, the virtual frame list with
/// its interrupt QHs and the control and bulk queues, and starts the controller.
pub fn uhci_init(sc: &'static UhciSoftc) -> UsbdStatus {
    // Save SOF over HC reset.
    sc.sc_saved_sof.set(uread1(sc, UHCI_SOF));

    uwrite2(sc, UHCI_INTR, 0); // disable interrupts
    uhci_globalreset(sc); // reset the controller
    uhci_reset(sc);

    if !UHCIXFER_INIT.load(Ordering::Relaxed) {
        pool_init(
            &UHCIXFER,
            size_of::<UhciXfer>(),
            0,
            IPL_SOFTUSB,
            0,
            "uhcixfer",
            None,
        );
        UHCIXFER_INIT.store(true, Ordering::Relaxed);
    }

    // Restore saved SOF.
    uwrite1(sc, UHCI_SOF, sc.sc_saved_sof.get());

    // Allocate and initialize real frame array.
    let err = usb_allocmem(
        &sc.sc_bus,
        UHCI_FRAMELIST_COUNT * size_of::<UhciPhysaddrT>(),
        UHCI_FRAMELIST_ALIGN,
        USB_DMA_COHERENT,
        &sc.sc_dma,
    );
    if err.is_err() {
        return err;
    }
    sc.sc_pframes
        .set(kernaddr(&sc.sc_dma, 0).cast::<UhciPhysaddrT>());
    uwrite2(sc, UHCI_FRNUM, 0); // set frame number to 0
    uwrite4(sc, UHCI_FLBASEADDR, dmaaddr(&sc.sc_dma, 0) as u32); // set frame list

    // Allocate a TD, inactive, that hangs from the last QH. This is to avoid a bug in the
    // PIIX that makes it run berserk otherwise.
    let Some(std) = uhci_alloc_std(sc) else {
        return USBD_NOMEM;
    };
    std.set_link_std(None);
    uhci_set(&std.td.td_link, UHCI_PTR_T);
    uhci_set(&std.td.td_status, 0); // inactive
    uhci_set(&std.td.td_token, 0);
    uhci_set(&std.td.td_buffer, 0);

    // Allocate the dummy QH marking the end and used for looping the QHs.
    let Some(lsqh) = uhci_alloc_sqh(sc) else {
        return USBD_NOMEM;
    };
    lsqh.hlink.set(None);
    uhci_set(&lsqh.qh.qh_hlink, UHCI_PTR_T); // end of QH chain
    lsqh.elink.set(Some(std));
    uhci_set(&lsqh.qh.qh_elink, std.physaddr.get() | UHCI_PTR_TD);
    sc.sc_last_qh.set(Some(lsqh));

    // Allocate the dummy QH where bulk traffic will be queued.
    let Some(bsqh) = uhci_alloc_sqh(sc) else {
        return USBD_NOMEM;
    };
    bsqh.hlink.set(Some(lsqh));
    uhci_set(&bsqh.qh.qh_hlink, lsqh.physaddr.get() | UHCI_PTR_QH);
    bsqh.elink.set(None);
    uhci_set(&bsqh.qh.qh_elink, UHCI_PTR_T);
    sc.sc_bulk_start.set(Some(bsqh));
    sc.sc_bulk_end.set(Some(bsqh));

    // Allocate dummy QH where high speed control traffic will be queued.
    let Some(chsqh) = uhci_alloc_sqh(sc) else {
        return USBD_NOMEM;
    };
    chsqh.hlink.set(Some(bsqh));
    uhci_set(&chsqh.qh.qh_hlink, bsqh.physaddr.get() | UHCI_PTR_QH);
    chsqh.elink.set(None);
    uhci_set(&chsqh.qh.qh_elink, UHCI_PTR_T);
    sc.sc_hctl_start.set(Some(chsqh));
    sc.sc_hctl_end.set(Some(chsqh));

    // Allocate dummy QH where control traffic will be queued.
    let Some(clsqh) = uhci_alloc_sqh(sc) else {
        return USBD_NOMEM;
    };
    clsqh.hlink.set(Some(chsqh));
    uhci_set(&clsqh.qh.qh_hlink, chsqh.physaddr.get() | UHCI_PTR_QH);
    clsqh.elink.set(None);
    uhci_set(&clsqh.qh.qh_elink, UHCI_PTR_T);
    sc.sc_lctl_start.set(Some(clsqh));
    sc.sc_lctl_end.set(Some(clsqh));

    // Make all (virtual) frame list pointers point to the interrupt queue heads and the
    // interrupt queue heads at the control queue head and point the physical frame list to
    // the virtual.
    for i in 0..UHCI_VFRAMELIST_COUNT {
        let std = uhci_alloc_std(sc);
        let sqh = uhci_alloc_sqh(sc);
        let (Some(std), Some(sqh)) = (std, sqh) else {
            return USBD_NOMEM;
        };
        std.link.set(Some(UhciSoftTdQh::Sqh(sqh)));
        uhci_set(&std.td.td_link, sqh.physaddr.get() | UHCI_PTR_QH);
        uhci_set(&std.td.td_status, UHCI_TD_IOS); // iso, inactive
        uhci_set(&std.td.td_token, 0);
        uhci_set(&std.td.td_buffer, 0);
        sqh.hlink.set(Some(clsqh));
        uhci_set(&sqh.qh.qh_hlink, clsqh.physaddr.get() | UHCI_PTR_QH);
        sqh.elink.set(None);
        uhci_set(&sqh.qh.qh_elink, UHCI_PTR_T);
        let vf = &sc.sc_vframes[i];
        vf.htd.set(Some(std));
        vf.etd.set(Some(std));
        vf.hqh.set(Some(sqh));
        vf.eqh.set(Some(sqh));
        let mut j = i;
        while j < UHCI_FRAMELIST_COUNT {
            sc.set_pframe(j, std.physaddr.get());
            j += UHCI_VFRAMELIST_COUNT;
        }
    }

    sc.sc_intrhead.init();

    timeout_set(
        &sc.sc_root_intr,
        uhci_poll_hub,
        ptr::from_ref(sc).cast_mut().cast(),
    );

    // Set up the bus struct.
    sc.sc_bus.methods.set(Some(&UHCI_BUS_METHODS));
    // SAFETY: `uhci_allocx`, this bus's `allocx`, returns only xfers that head a `UhciXfer`.
    unsafe { usbd_bus_set_hc_types::<UhciPipe, UhciXfer>(&sc.sc_bus) };

    sc.sc_suspend.set(DVACT_RESUME as i8);

    uhcicmd(sc, UHCI_CMD_MAXP); // Assume 64 byte packets at frame end

    uwrite2(
        sc,
        UHCI_INTR,
        UHCI_INTR_TOCRCIE | UHCI_INTR_RIE | UHCI_INTR_IOCE | UHCI_INTR_SPIE,
    ); // enable interrupts

    uhci_run(sc, 1) // and here we go...
}

/// `uhci_activate`: suspend stops the controller and enters global suspend; resume
/// restores the frame list and restarts it; power-down stops it after the children.
pub fn uhci_activate(self_: &Device, act: i32) -> Result<(), Errno> {
    // SAFETY: the front-end's softc starts with the `UhciSoftc` (`#[repr(C)]`), made for its
    // attachment.
    let sc = unsafe { self_.softc::<UhciSoftc>() };

    match act {
        DVACT_SUSPEND => {
            let rv = config_activate_children(self_, act);
            sc.sc_bus.use_polling.set(sc.sc_bus.use_polling.get() + 1);
            let _ = uhci_run(sc, 0); // stop the controller

            // save some state if BIOS doesn't
            sc.sc_saved_frnum.set(uread2(sc, UHCI_FRNUM));

            uwrite2(sc, UHCI_INTR, 0); // disable intrs

            let cmd = uread2(sc, UHCI_CMD);
            uhcicmd(sc, cmd | UHCI_CMD_EGSM); // enter global suspend
            usb_delay_ms(&sc.sc_bus, USB_RESUME_WAIT);
            sc.sc_suspend.set(act as i8);
            sc.sc_bus.use_polling.set(sc.sc_bus.use_polling.get() - 1);
            rv
        }
        DVACT_POWERDOWN => {
            let rv = config_activate_children(self_, act);
            let _ = uhci_run(sc, 0); // stop the controller
            rv
        }
        DVACT_RESUME => {
            #[cfg(feature = "diagnostic")]
            if sc.sc_suspend.get() == DVACT_RESUME as i8 {
                printf(format_args!(
                    "uhci_powerhook: weird, resume without suspend.\n"
                ));
            }
            sc.sc_bus.use_polling.set(sc.sc_bus.use_polling.get() + 1);
            sc.sc_suspend.set(act as i8);
            let cmd = uread2(sc, UHCI_CMD);
            if cmd & UHCI_CMD_RS != 0 {
                let _ = uhci_run(sc, 0); // in case BIOS has started it
            }

            // restore saved state
            uwrite4(sc, UHCI_FLBASEADDR, dmaaddr(&sc.sc_dma, 0) as u32);
            uwrite2(sc, UHCI_FRNUM, sc.sc_saved_frnum.get());
            uwrite1(sc, UHCI_SOF, sc.sc_saved_sof.get());

            uhcicmd(sc, cmd | UHCI_CMD_FGR); // force global resume
            usb_delay_ms(&sc.sc_bus, USB_RESUME_DELAY);
            uhcicmd(sc, cmd & !UHCI_CMD_EGSM); // back to normal
            uhcicmd(sc, UHCI_CMD_MAXP);
            uwrite2(
                sc,
                UHCI_INTR,
                UHCI_INTR_TOCRCIE | UHCI_INTR_RIE | UHCI_INTR_IOCE | UHCI_INTR_SPIE,
            ); // re-enable intrs
            let _ = uhci_run(sc, 1); // and start traffic again
            usb_delay_ms(&sc.sc_bus, USB_RESUME_RECOVERY);
            sc.sc_bus.use_polling.set(sc.sc_bus.use_polling.get() - 1);
            config_activate_children(self_, act)
        }
        _ => config_activate_children(self_, act),
    }
}

/// `uhci_detach`: detaches `usb(4)`. The other data structures are not freed (XXX in C).
pub fn uhci_detach(self_: &Device, flags: i32) -> Result<(), Errno> {
    config_detach_children(self_, flags)?;

    #[cfg(feature = "diagnostic")]
    {
        // SAFETY: as in `uhci_activate`.
        let sc = unsafe { self_.softc::<UhciSoftc>() };
        kassert!(sc.sc_intrxfer.get().is_none());
    }

    // XXX free other data structures XXX

    Ok(())
}

/// `uhci_allocx`: the bus's `allocx`, a zeroed [`UhciXfer`].
pub fn uhci_allocx(_bus: &'static UsbdBus) -> Option<&'static UsbdXfer> {
    let p = pool_get(&UHCIXFER, PR_NOWAIT | PR_ZERO)?;
    // SAFETY: the pool's items are `size_of::<UhciXfer>()` bytes at the pool's alignment (at
    // least the type's), zeroed (`PR_ZERO`), which is a valid `UhciXfer`; it lives until
    // `uhci_freex`.
    let ux: &'static UhciXfer = unsafe { &*p.as_ptr().cast::<UhciXfer>() };
    #[cfg(feature = "diagnostic")]
    ux.isdone.set(1);
    Some(&ux.xfer)
}

/// `uhci_freex`: the bus's `freex`.
///
/// # Safety
///
/// `xfer` came from [`uhci_allocx`] and nothing uses it any more.
pub unsafe fn uhci_freex(_bus: &'static UsbdBus, xfer: NonNull<UsbdXfer>) {
    #[cfg(feature = "diagnostic")]
    {
        // SAFETY: the caller's contract: a `UhciXfer` from `uhci_allocx`, still allocated.
        let ux = unsafe { xfer.cast::<UhciXfer>().as_ref() };
        if ux.isdone.get() == 0 {
            printf(format_args!("uhci_freex: !isdone\n"));
            return;
        }
    }
    // The xfer heads its `UhciXfer` (`#[repr(C)]`), the pool item.
    pool_put(&UHCIXFER, xfer.cast());
}

/// `uhci_poll_hub`: executed periodically, simulates interrupts from the root controller
/// interrupt pipe for port status change.
pub fn uhci_poll_hub(addr: *mut c_void) {
    // SAFETY: `uhci_init` sets the timeout with the softc as argument; the softc lives as
    // long as the controller.
    let sc: &'static UhciSoftc = unsafe { &*addr.cast::<UhciSoftc>() };

    if sc.sc_bus.dying.get() != 0 {
        return;
    }

    let Some(xfer) = sc.sc_intrxfer.get() else {
        return;
    };

    let p = kernaddr(&xfer.dmabuf, 0);
    let mut v: u8 = 0;
    if uread2(sc, UHCI_PORTSC1) & (UHCI_PORTSC_CSC | UHCI_PORTSC_OCIC) != 0 {
        v |= 1 << 1;
    }
    if uread2(sc, UHCI_PORTSC2) & (UHCI_PORTSC_CSC | UHCI_PORTSC_OCIC) != 0 {
        v |= 1 << 2;
    }
    // SAFETY: the root hub's interrupt xfer is started (`sc_intrxfer`), so `usbd_transfer`
    // gave it a DMA buffer (at least one byte: `usb_allocmem` never returns less), which only
    // this driver writes until the xfer completes.
    unsafe { p.write(v) };
    if v == 0 {
        // No change, try again in a while
        timeout_add_msec(&sc.sc_root_intr, 255);
        return;
    }

    xfer.actlen.set(xfer.length.get());
    xfer.status.set(USBD_NORMAL_COMPLETION);

    let s = splusb();
    let bus = xfer.device().bus();
    bus.intr_context.set(bus.intr_context.get() + 1);
    usb_transfer_complete(xfer);
    bus.intr_context.set(bus.intr_context.get() - 1);
    splx(s);
}

/// `uhci_root_ctrl_done`.
pub fn uhci_root_ctrl_done(_xfer: &'static UsbdXfer) {}

/// `uhci_add_loop`: let the last QH loop back to the high speed control transfer QH. This is
/// what intel calls "bandwidth reclamation" and improves USB performance a lot for some
/// devices. If we are already looping, just count it.
pub fn uhci_add_loop(sc: &'static UhciSoftc) {
    sc.sc_loops.set(sc.sc_loops.get().wrapping_add(1));
    if sc.sc_loops.get() == 1 {
        // Note, we don't loop back the soft pointer.
        uhci_set(
            &sc_qh(&sc.sc_last_qh).qh.qh_hlink,
            sc_qh(&sc.sc_hctl_start).physaddr.get() | UHCI_PTR_QH,
        );
    }
}

/// `uhci_rem_loop`.
pub fn uhci_rem_loop(sc: &'static UhciSoftc) {
    sc.sc_loops.set(sc.sc_loops.get().wrapping_sub(1));
    if sc.sc_loops.get() == 0 {
        uhci_set(&sc_qh(&sc.sc_last_qh).qh.qh_hlink, UHCI_PTR_T);
    }
}

/// Links `sqh` after `end`, the last QH of a queue (the body of `uhci_add_*`).
fn uhci_append_qh(end: &'static UhciSoftQh, sqh: &'static UhciSoftQh) {
    sqh.hlink.set(end.hlink.get());
    uhci_set(&sqh.qh.qh_hlink, uhci_get(&end.qh.qh_hlink));
    end.hlink.set(Some(sqh));
    uhci_set(&end.qh.qh_hlink, sqh.physaddr.get() | UHCI_PTR_QH);
}

/// Unlinks `sqh` from the queue that starts at `start` and returns the QH before it (the
/// body of `uhci_remove_*`): the T bit should be set in the elink of the QH so that the HC
/// doesn't follow the pointer. This condition may fail if the transferred packet was short
/// so that the QH still points at the last used TD. In this case we set the T bit and wait a
/// little for the HC to stop looking at the TD.
fn uhci_unlink_qh(start: &'static UhciSoftQh, sqh: &'static UhciSoftQh) -> &'static UhciSoftQh {
    if uhci_get(&sqh.qh.qh_elink) & UHCI_PTR_T == 0 {
        uhci_set(&sqh.qh.qh_elink, UHCI_PTR_T);
        delay(UHCI_QH_REMOVE_DELAY);
    }

    let pqh = uhci_prev_qh(start, sqh);
    pqh.hlink.set(sqh.hlink.get());
    uhci_set(&pqh.qh.qh_hlink, uhci_get(&sqh.qh.qh_hlink));
    delay(UHCI_QH_REMOVE_DELAY);
    pqh
}

/// `uhci_add_hs_ctrl`: add high speed control QH, called at `splusb()`.
pub fn uhci_add_hs_ctrl(sc: &'static UhciSoftc, sqh: &'static UhciSoftQh) {
    splsoftassert(IPL_SOFTUSB, "uhci_add_hs_ctrl");

    uhci_append_qh(sc_qh(&sc.sc_hctl_end), sqh);
    sc.sc_hctl_end.set(Some(sqh));
}

/// `uhci_remove_hs_ctrl`: remove high speed control QH, called at `splusb()`.
pub fn uhci_remove_hs_ctrl(sc: &'static UhciSoftc, sqh: &'static UhciSoftQh) {
    splsoftassert(IPL_SOFTUSB, "uhci_remove_hs_ctrl");

    let pqh = uhci_unlink_qh(sc_qh(&sc.sc_hctl_start), sqh);
    if sc.sc_hctl_end.get().is_some_and(|e| ptr::eq(e, sqh)) {
        sc.sc_hctl_end.set(Some(pqh));
    }
}

/// `uhci_add_ls_ctrl`: add low speed control QH, called at `splusb()`.
pub fn uhci_add_ls_ctrl(sc: &'static UhciSoftc, sqh: &'static UhciSoftQh) {
    splsoftassert(IPL_SOFTUSB, "uhci_add_ls_ctrl");

    uhci_append_qh(sc_qh(&sc.sc_lctl_end), sqh);
    sc.sc_lctl_end.set(Some(sqh));
}

/// `uhci_remove_ls_ctrl`: remove low speed control QH, called at `splusb()`.
pub fn uhci_remove_ls_ctrl(sc: &'static UhciSoftc, sqh: &'static UhciSoftQh) {
    splsoftassert(IPL_SOFTUSB, "uhci_remove_ls_ctrl");

    let pqh = uhci_unlink_qh(sc_qh(&sc.sc_lctl_start), sqh);
    if sc.sc_lctl_end.get().is_some_and(|e| ptr::eq(e, sqh)) {
        sc.sc_lctl_end.set(Some(pqh));
    }
}

/// `uhci_add_bulk`: add bulk QH, called at `splusb()`.
pub fn uhci_add_bulk(sc: &'static UhciSoftc, sqh: &'static UhciSoftQh) {
    splsoftassert(IPL_SOFTUSB, "uhci_add_bulk");

    uhci_append_qh(sc_qh(&sc.sc_bulk_end), sqh);
    sc.sc_bulk_end.set(Some(sqh));
    uhci_add_loop(sc);
}

/// `uhci_remove_bulk`: remove bulk QH, called at `splusb()`.
pub fn uhci_remove_bulk(sc: &'static UhciSoftc, sqh: &'static UhciSoftQh) {
    splsoftassert(IPL_SOFTUSB, "uhci_remove_bulk");

    uhci_rem_loop(sc);
    let pqh = uhci_unlink_qh(sc_qh(&sc.sc_bulk_start), sqh);
    if sc.sc_bulk_end.get().is_some_and(|e| ptr::eq(e, sqh)) {
        sc.sc_bulk_end.set(Some(pqh));
    }
}

/// `uhci_intr`: the hard interrupt handler; `arg` is the `UhciSoftc`.
pub fn uhci_intr(arg: *mut c_void) -> i32 {
    if arg.is_null() {
        return 0;
    }
    // SAFETY: the front-end establishes the interrupt with its softc's `UhciSoftc` as the
    // argument and disestablishes it before the softc goes away.
    let sc: &'static UhciSoftc = unsafe { &*arg.cast::<UhciSoftc>() };

    if sc.sc_bus.dying.get() != 0 {
        return 0;
    }
    if sc.sc_bus.use_polling.get() != 0 {
        return 0;
    }
    uhci_intr1(sc)
}

/// `uhci_intr1`: acknowledges the controller's interrupts and schedules the soft interrupt.
pub fn uhci_intr1(sc: &'static UhciSoftc) -> i32 {
    let status = uread2(sc, UHCI_STS);
    if status == 0xffff {
        sc.sc_bus.dying.set(1);
        return 0;
    }
    let status = status & UHCI_STS_ALLINTRS;
    if status == 0 {
        // The interrupt was not for us.
        return 0;
    }

    if sc.sc_suspend.get() != DVACT_RESUME as i8 {
        printf(format_args!(
            "{}: interrupt while not operating ignored\n",
            devname(sc)
        ));
        return 0;
    }

    let mut ack: u16 = 0;
    if status & UHCI_STS_USBINT != 0 {
        ack |= UHCI_STS_USBINT;
    }
    if status & UHCI_STS_USBEI != 0 {
        ack |= UHCI_STS_USBEI;
    }
    if status & UHCI_STS_RD != 0 {
        ack |= UHCI_STS_RD;
    }
    if status & UHCI_STS_HSE != 0 {
        ack |= UHCI_STS_HSE;
        printf(format_args!("{}: host system error\n", devname(sc)));
    }
    if status & UHCI_STS_HCPE != 0 {
        ack |= UHCI_STS_HCPE;
        printf(format_args!(
            "{}: host controller process error\n",
            devname(sc)
        ));
    }
    if status & UHCI_STS_HCH != 0 {
        // no acknowledge needed
        if sc.sc_bus.dying.get() == 0 {
            printf(format_args!("{}: host controller halted\n", devname(sc)));
        }
        sc.sc_bus.dying.set(1);
    }

    if ack == 0 {
        return 0; // nothing to acknowledge
    }
    uwrite2(sc, UHCI_STS, ack); // acknowledge the ints

    sc.sc_bus
        .no_intrs
        .set(sc.sc_bus.no_intrs.get().wrapping_add(1));
    usb_schedsoftintr(&sc.sc_bus);

    1
}

/// `uhci_softintr`: the soft interrupt; `v` is the bus.
pub fn uhci_softintr(v: *mut c_void) {
    // SAFETY: `usb_attach` establishes the soft interrupt (and `usb_schedsoftintr` calls it)
    // with the bus as argument; the bus lives in the softc for the controller's life.
    let bus: &'static UsbdBus = unsafe { &*v.cast::<UsbdBus>() };
    let sc = uhci_softc(bus);

    if sc.sc_bus.dying.get() != 0 {
        return;
    }

    sc.sc_bus.intr_context.set(sc.sc_bus.intr_context.get() + 1);

    // Interrupts on UHCI really suck. When the host controller interrupts because a transfer
    // is completed there is no way of knowing which transfer it was. You can scan down the
    // TDs and QHs of the previous frame to limit the search, but that assumes that the
    // interrupt was not delayed by more than 1 ms, which may not always be true (e.g. after
    // debug output on a slow console). We scan all interrupt descriptors to see if any have
    // completed.
    let mut ux = sc.sc_intrhead.first();
    while let Some(u) = ux {
        let nextex = ListHead::<UhciXferIntrList>::next(u);
        uhci_check_intr(sc, &u.xfer);
        ux = nextex;
    }

    if sc.sc_softwake.get() != 0 {
        sc.sc_softwake.set(0);
        wakeup(ptr::from_ref(&sc.sc_softwake));
    }

    sc.sc_bus.intr_context.set(sc.sc_bus.intr_context.get() - 1);
}

/// `uhci_check_intr`: an xfer is done when its last TD is inactive, or an earlier one
/// stalled or came back short.
pub fn uhci_check_intr(_sc: &'static UhciSoftc, xfer: &'static UsbdXfer) {
    let ux = uhci_xfer(xfer);

    if xfer.status.get() == USBD_CANCELLED || xfer.status.get() == USBD_TIMEOUT {
        return;
    }

    let Some(start) = ux.stdstart.get() else {
        return;
    };
    let Some(lstd) = ux.stdend.get() else {
        #[cfg(feature = "diagnostic")]
        printf(format_args!("uhci_check_intr: std==0\n"));
        return;
    };
    // If the last TD is still active we need to check whether there is an error somewhere
    // in the middle, or whether there was a short packet (SPD and not ACTIVE).
    if uhci_get(&lstd.td.td_status) & UHCI_TD_ACTIVE != 0 {
        let mut done = false;
        let mut std = start;
        while !ptr::eq(std, lstd) {
            let status = uhci_get(&std.td.td_status);
            // If there's an active TD the xfer isn't done.
            if status & UHCI_TD_ACTIVE != 0 {
                break;
            }
            // Any kind of error makes the xfer done.
            if status & UHCI_TD_STALLED != 0 {
                done = true;
                break;
            }
            // We want short packets, and it is short: it's done
            if status & UHCI_TD_SPD != 0
                && uhci_td_get_actlen(status) < uhci_td_get_maxlen(uhci_get(&std.td.td_token))
            {
                done = true;
                break;
            }
            let Some(next) = std.link_std() else {
                break;
            };
            std = next;
        }
        if !done {
            return;
        }
    }
    // done:
    timeout_del(&xfer.timeout_handle);
    usb_rem_task(xfer.pipe().device(), &xfer.abort_task);
    uhci_idone(xfer);
}

/// `uhci_idone`: computes a finished xfer's length and status and completes it. Called at
/// `splusb()`.
pub fn uhci_idone(xfer: &'static UsbdXfer) {
    let ux = uhci_xfer(xfer);
    let upipe = uhci_pipe(xfer.pipe());

    #[cfg(feature = "diagnostic")]
    {
        let s = crate::machine::intr::splhigh();
        if ux.isdone.get() != 0 {
            splx(s);
            printf(format_args!("uhci_idone: ux={:p} is done!\n", ux));
            return;
        }
        ux.isdone.set(1);
        splx(s);
    }

    if xfer.nframes.get() != 0 {
        // Isoc transfer, do things differently.
        let nframes = xfer.nframes.get();
        let mut actlen: u32 = 0;
        let mut n = ux.curframe.get();
        for i in 0..nframes {
            let std = upipe.std(n);
            n += 1;
            if n as usize >= UHCI_VFRAMELIST_COUNT {
                n = 0;
            }
            let status = std.map_or(0, |t| uhci_get(&t.td.td_status));
            let len = uhci_td_get_actlen(status);
            set_frlength(xfer, i, len as u16);
            actlen += len;
        }
        upipe.iso_inuse.set(upipe.iso_inuse.get() - nframes);
        usb_syncmem(
            &xfer.dmabuf,
            0,
            xfer.length.get() as usize,
            sync_ops(xfer, BUS_DMASYNC_POSTREAD, BUS_DMASYNC_POSTWRITE),
        );
        xfer.actlen.set(actlen);
        xfer.status.set(USBD_NORMAL_COMPLETION);
        // end:
        usb_transfer_complete(xfer);
        return;
    }

    // The transfer is done, compute actual length and status.
    let mut status: u32 = 0;
    let mut actlen: u32 = 0;
    let mut std = ux.stdstart.get();
    while let Some(t) = std {
        let nstatus = uhci_get(&t.td.td_status);
        if nstatus & UHCI_TD_ACTIVE != 0 {
            break;
        }

        status = nstatus;
        if uhci_td_get_pid(uhci_get(&t.td.td_token)) != UHCI_TD_PID_SETUP {
            actlen += uhci_td_get_actlen(status);
        } else {
            // UHCI will report CRCTO in addition to a STALL or NAK for a SETUP transaction.
            // See section 3.2.2, "TD CONTROL AND STATUS".
            if status & (UHCI_TD_STALLED | UHCI_TD_NAK) != 0 {
                status &= !UHCI_TD_CRCTO;
            }
        }
        std = t.link_std();
    }
    // If there are left over TDs we need to update the toggle.
    if let Some(t) = std {
        upipe
            .nexttoggle
            .set(uhci_td_get_dt(uhci_get(&t.td.td_token)) as i32);
    }

    status &= UHCI_TD_ERROR;
    xfer.actlen.set(actlen);
    if status != 0 {
        if status == UHCI_TD_STALLED {
            xfer.status.set(USBD_STALLED);
        } else {
            xfer.status.set(USBD_IOERROR); // more info XXX
        }
    } else {
        if xfer.actlen.get() != 0 {
            usb_syncmem(
                &xfer.dmabuf,
                0,
                xfer.actlen.get() as usize,
                sync_ops(xfer, BUS_DMASYNC_POSTREAD, BUS_DMASYNC_POSTWRITE),
            );
        }
        xfer.status.set(USBD_NORMAL_COMPLETION);
    }

    // end:
    usb_transfer_complete(xfer);
}

/// `uhci_timeout`: an xfer's timeout; the abort runs in the USB abort task thread.
pub fn uhci_timeout(addr: *mut c_void) {
    // SAFETY: the timeout's argument is its xfer (`uhci_arm_timeout`), which the stack keeps
    // until the xfer completes, and completion deletes the timeout.
    let xfer: &'static UsbdXfer = unsafe { &*addr.cast::<UsbdXfer>() };
    let sc = uhci_softc(xfer.device().bus());

    if sc.sc_bus.dying.get() != 0 {
        uhci_timeout_task(addr);
        return;
    }

    usb_init_task(
        &xfer.abort_task,
        uhci_timeout_task,
        addr,
        USB_TASK_TYPE_ABORT,
    );
    usb_add_task(xfer.device(), &xfer.abort_task);
}

/// `uhci_timeout_task`.
pub fn uhci_timeout_task(addr: *mut c_void) {
    // SAFETY: as in `uhci_timeout`; completion removes the task.
    let xfer: &'static UsbdXfer = unsafe { &*addr.cast::<UsbdXfer>() };

    let s = splusb();
    uhci_abort_xfer(xfer, USBD_TIMEOUT);
    splx(s);
}

/// `uhci_poll`: the bus's `do_poll`.
pub fn uhci_poll(bus: &'static UsbdBus) {
    let sc = uhci_softc(bus);

    if uread2(sc, UHCI_STS) & UHCI_STS_ALLINTRS != 0 {
        uhci_intr1(sc);
    }
}

/// `uhci_reset`: resets the host controller; the reset bit goes low when the controller is
/// done.
pub fn uhci_reset(sc: &UhciSoftc) {
    uhcicmd(sc, UHCI_CMD_HCRESET);
    // The reset bit goes low when the controller is done.
    let mut n = 0;
    while n < UHCI_RESET_TIMEOUT && uread2(sc, UHCI_CMD) & UHCI_CMD_HCRESET != 0 {
        usb_delay_ms(&sc.sc_bus, 1);
        n += 1;
    }
    if n >= UHCI_RESET_TIMEOUT {
        printf(format_args!("{}: controller did not reset\n", devname(sc)));
    }
}

/// `uhci_run`: starts (`run` nonzero) or stops the controller and waits until it is in that
/// state.
pub fn uhci_run(sc: &UhciSoftc, run: i32) -> UsbdStatus {
    let run = run != 0;
    let s = splhardusb();
    let mut cmd = uread2(sc, UHCI_CMD);
    if run {
        cmd |= UHCI_CMD_RS;
    } else {
        cmd &= !UHCI_CMD_RS;
    }
    uhcicmd(sc, cmd);
    for _ in 0..10 {
        let running = uread2(sc, UHCI_STS) & UHCI_STS_HCH == 0;
        // return when we've entered the state we want
        if run == running {
            splx(s);
            return USBD_NORMAL_COMPLETION;
        }
        usb_delay_ms(&sc.sc_bus, 1);
    }
    splx(s);
    printf(format_args!(
        "{}: cannot {}\n",
        devname(sc),
        if run { "start" } else { "stop" }
    ));
    USBD_IOERROR
}

/*
 * Memory management routines.
 *  uhci_alloc_std allocates TDs
 *  uhci_alloc_sqh allocates QHs
 * These two routines do their own free list management, partly for speed, partly because
 * allocating DMAable memory has page size granularity so much memory would be wasted if only
 * one TD/QH (32 bytes) was placed in each allocated chunk.
 */

/// `uhci_alloc_std`: a cleared TD from the free list, carving a new chunk when it is empty.
pub fn uhci_alloc_std(sc: &'static UhciSoftc) -> Option<&'static UhciSoftTd> {
    let s = splusb();
    if sc.sc_freetds.get().is_none() {
        let dma = UsbDma {
            block: Cell::new(None),
            offs: Cell::new(0),
        };
        let err = usb_allocmem(
            &sc.sc_bus,
            UHCI_STD_SIZE * UHCI_STD_CHUNK,
            UHCI_TD_ALIGN,
            USB_DMA_COHERENT,
            &dma,
        );
        if err.is_err() {
            splx(s);
            return None;
        }
        for i in 0..UHCI_STD_CHUNK {
            let offs = i * UHCI_STD_SIZE;
            let p = kernaddr(&dma, offs).cast::<UhciSoftTd>();
            // SAFETY: the chunk is `UHCI_STD_SIZE * UHCI_STD_CHUNK` fresh bytes, aligned to
            // `UHCI_TD_ALIGN` (at least the structure's alignment, checked in uhcivar.rs), and
            // `offs` is a multiple of `UHCI_STD_SIZE`, itself a multiple of it; nobody else
            // uses the bytes, and chunks are never freed, so the TD lives forever.
            let std: &'static UhciSoftTd = unsafe {
                p.write(UhciSoftTd::new());
                &*p
            };
            std.physaddr.set(dmaaddr(&dma, offs) as u32);
            std.set_link_std(sc.sc_freetds.get());
            sc.sc_freetds.set(Some(std));
        }
    }

    let std = sc.sc_freetds.get()?;
    sc.sc_freetds.set(std.link_std());
    td_clear(std);

    splx(s);
    Some(std)
}

/// `uhci_free_std`.
pub fn uhci_free_std(sc: &'static UhciSoftc, std: &'static UhciSoftTd) {
    #[cfg(feature = "diagnostic")]
    {
        if uhci_get(&std.td.td_token) == TD_IS_FREE {
            printf(format_args!("uhci_free_std: freeing free TD {:p}\n", std));
            return;
        }
        uhci_set(&std.td.td_token, TD_IS_FREE);
    }

    let s = splusb();
    std.set_link_std(sc.sc_freetds.get());
    sc.sc_freetds.set(Some(std));
    splx(s);
}

/// `uhci_alloc_sqh`: a cleared QH from the free list, carving a new chunk when it is empty.
pub fn uhci_alloc_sqh(sc: &'static UhciSoftc) -> Option<&'static UhciSoftQh> {
    let s = splusb();
    if sc.sc_freeqhs.get().is_none() {
        let dma = UsbDma {
            block: Cell::new(None),
            offs: Cell::new(0),
        };
        let err = usb_allocmem(
            &sc.sc_bus,
            UHCI_SQH_SIZE * UHCI_SQH_CHUNK,
            UHCI_QH_ALIGN,
            USB_DMA_COHERENT,
            &dma,
        );
        if err.is_err() {
            splx(s);
            return None;
        }
        for i in 0..UHCI_SQH_CHUNK {
            let offs = i * UHCI_SQH_SIZE;
            let p = kernaddr(&dma, offs).cast::<UhciSoftQh>();
            // SAFETY: as in `uhci_alloc_std`, for soft QHs.
            let sqh: &'static UhciSoftQh = unsafe {
                p.write(UhciSoftQh::new());
                &*p
            };
            sqh.physaddr.set(dmaaddr(&dma, offs) as u32);
            sqh.hlink.set(sc.sc_freeqhs.get());
            sc.sc_freeqhs.set(Some(sqh));
        }
    }
    let sqh = sc.sc_freeqhs.get()?;
    sc.sc_freeqhs.set(sqh.hlink.get());
    uhci_set(&sqh.qh.qh_hlink, 0);
    uhci_set(&sqh.qh.qh_elink, 0);

    splx(s);
    Some(sqh)
}

/// `uhci_free_sqh`.
pub fn uhci_free_sqh(sc: &'static UhciSoftc, sqh: &'static UhciSoftQh) {
    sqh.hlink.set(sc.sc_freeqhs.get());
    sc.sc_freeqhs.set(Some(sqh));
}

/// `uhci_free_std_chain`: frees the TDs from `std` up to, not including, `stdend`.
pub fn uhci_free_std_chain(
    sc: &'static UhciSoftc,
    std: Option<&'static UhciSoftTd>,
    stdend: Option<&'static UhciSoftTd>,
) {
    let mut std = std;
    while let Some(t) = std {
        if stdend.is_some_and(|e| ptr::eq(e, t)) {
            break;
        }
        let p = t.link_std();
        uhci_free_std(sc, t);
        std = p;
    }
}

/// `uhci_alloc_std_chain`: the TDs that move `len` bytes of the xfer's buffer, one per
/// packet, with the data toggles set; returns the first and the last.
pub fn uhci_alloc_std_chain(
    sc: &'static UhciSoftc,
    len: u32,
    xfer: &'static UsbdXfer,
) -> Result<(&'static UhciSoftTd, &'static UhciSoftTd), UsbdStatus> {
    let upipe = uhci_pipe(xfer.pipe());
    let mut flags = xfer.flags.get();
    let rd = usbd_xfer_isread(xfer);
    let dma = &xfer.dmabuf;
    let addr = u32::from(xfer.device().address.get());
    let endpt = u32::from(xfer.pipe().endpoint().edesc().bEndpointAddress);

    usb_syncmem(
        &xfer.dmabuf,
        0,
        xfer.length.get() as usize,
        sync_ops(xfer, BUS_DMASYNC_PREREAD, BUS_DMASYNC_PREWRITE),
    );

    let mps = u32::from(ugetw(xfer.pipe().endpoint().edesc().wMaxPacketSize));
    if mps == 0 {
        printf(format_args!("uhci_alloc_std_chain: mps=0\n"));
        return Err(USBD_INVAL);
    }
    let mut ntd = len.div_ceil(mps) as i32;
    if len == 0 {
        flags |= USBD_FORCE_SHORT_XFER;
    }
    if flags & USBD_FORCE_SHORT_XFER != 0 && len.is_multiple_of(mps) {
        ntd += 1;
    }
    let mut tog = upipe.nexttoggle.get() as u32;
    if ntd % 2 == 0 {
        tog ^= 1;
    }
    upipe.nexttoggle.set((tog ^ 1) as i32);
    let mut lastp: Option<&'static UhciSoftTd> = None;
    let mut ep: Option<&'static UhciSoftTd> = None;
    let mut lastlink = UHCI_PTR_T;
    ntd -= 1;
    let mut status = uhci_td_zero_actlen(uhci_td_set_errcnt(3) | UHCI_TD_ACTIVE);
    if xfer.pipe().device().speed.get() == USB_SPEED_LOW {
        status |= UHCI_TD_LS;
    }
    if flags & USBD_SHORT_XFER_OK != 0 {
        status |= UHCI_TD_SPD;
    }
    let mut i = ntd;
    while i >= 0 {
        let Some(p) = uhci_alloc_std(sc) else {
            uhci_free_std_chain(sc, lastp, None);
            return Err(USBD_NOMEM);
        };
        p.set_link_std(lastp);
        uhci_set(&p.td.td_link, lastlink | UHCI_PTR_VF | UHCI_PTR_TD);
        lastp = Some(p);
        lastlink = p.physaddr.get();
        uhci_set(&p.td.td_status, status);
        let l = if i == ntd {
            // last TD
            ep = Some(p);
            let l = len % mps;
            if l == 0 && flags & USBD_FORCE_SHORT_XFER == 0 {
                mps
            } else {
                l
            }
        } else {
            mps
        };
        uhci_set(
            &p.td.td_token,
            if rd {
                uhci_td_in(l, endpt, addr, tog)
            } else {
                uhci_td_out(l, endpt, addr, tog)
            },
        );
        uhci_set(
            &p.td.td_buffer,
            dmaaddr(dma, i as usize * mps as usize) as u32,
        );
        tog ^= 1;
        i -= 1;
    }
    match (lastp, ep) {
        (Some(sp), Some(ep)) => Ok((sp, ep)),
        _ => panic(format_args!("uhci_alloc_std_chain: empty chain")),
    }
}

/// `uhci_device_clear_toggle`: the pipe's `cleartoggle`.
pub fn uhci_device_clear_toggle(pipe: &'static UsbdPipe) {
    let upipe = uhci_pipe(pipe);
    upipe.nexttoggle.set(0);
}

/// `uhci_device_bulk_transfer`.
pub fn uhci_device_bulk_transfer(xfer: &'static UsbdXfer) -> UsbdStatus {
    // Insert last in queue.
    let err = usb_insert_transfer(xfer);
    if err.is_err() {
        return err;
    }

    // Pipe isn't running (otherwise err would be USBD_INPROG), so start it first.
    uhci_device_bulk_start(queue_first(xfer.pipe()))
}

/// Arms the xfer's timeout (`timeout_del`, `timeout_set`, `timeout_add_msec`), as the start
/// functions do.
fn uhci_arm_timeout(xfer: &'static UsbdXfer) {
    timeout_del(&xfer.timeout_handle);
    timeout_set(
        &xfer.timeout_handle,
        uhci_timeout,
        ptr::from_ref(xfer).cast_mut().cast(),
    );
    timeout_add_msec(&xfer.timeout_handle, u64::from(xfer.timeout.get()));
}

/// `ux->isdone` bookkeeping of a start function (`DIAGNOSTIC`).
#[cfg(feature = "diagnostic")]
fn uhci_mark_started(ux: &UhciXfer, func: &str) {
    if ux.isdone.get() == 0 {
        printf(format_args!("{func}: not done, ux={:p}\n", ux));
    }
    ux.isdone.set(0);
}

/// `uhci_device_bulk_start`: hangs the data TDs on the pipe's QH and queues it for bulk.
pub fn uhci_device_bulk_start(xfer: &'static UsbdXfer) -> UsbdStatus {
    let sc = uhci_softc(xfer.device().bus());
    let upipe = uhci_pipe(xfer.pipe());
    let ux = uhci_xfer(xfer);

    if sc.sc_bus.dying.get() != 0 {
        return USBD_IOERROR;
    }

    #[cfg(feature = "diagnostic")]
    if xfer.rqflags.get() & crate::dev::usb::usbdivar::URQ_REQUEST != 0 {
        panic(format_args!("uhci_device_bulk_start: a request"));
    }

    let len = xfer.length.get();
    let sqh = UhciPipe::sqh_of(&upipe.bulk_sqh);

    let (data, dataend) = match uhci_alloc_std_chain(sc, len, xfer) {
        Ok(c) => c,
        Err(err) => return err,
    };
    td_status_set(dataend, UHCI_TD_IOC);

    // Set up interrupt info.
    ux.stdstart.set(Some(data));
    ux.stdend.set(Some(dataend));
    #[cfg(feature = "diagnostic")]
    uhci_mark_started(ux, "uhci_device_bulk_start");

    sqh.elink.set(Some(data));
    uhci_set(&sqh.qh.qh_elink, data.physaddr.get() | UHCI_PTR_TD);

    let s = splusb();
    uhci_add_bulk(sc, sqh);
    uhci_add_intr_list(sc, ux);

    if xfer.timeout.get() != 0 && sc.sc_bus.use_polling.get() == 0 {
        uhci_arm_timeout(xfer);
    }
    xfer.status.set(USBD_IN_PROGRESS);
    splx(s);

    USBD_IN_PROGRESS
}

/// `uhci_device_bulk_abort`: abort a device bulk request.
pub fn uhci_device_bulk_abort(xfer: &'static UsbdXfer) {
    uhci_abort_xfer(xfer, USBD_CANCELLED);
}

/// `uhci_abort_xfer`: abort a device request.
///
/// If this routine is called at `splusb()` it guarantees that the request will be removed
/// from the hardware scheduling and that the callback for it will be called with
/// `USBD_CANCELLED` status. It's impossible to guarantee that the requested transfer will not
/// have happened since the hardware runs concurrently. If the transaction has already
/// happened we rely on the ordinary interrupt processing to process it.
pub fn uhci_abort_xfer(xfer: &'static UsbdXfer, status: UsbdStatus) {
    let sc = uhci_softc(xfer.device().bus());
    let ux = uhci_xfer(xfer);

    if sc.sc_bus.dying.get() != 0 {
        // If we're dying, just do the software part.
        let s = splusb();
        xfer.status.set(status); // make software ignore it
        timeout_del(&xfer.timeout_handle);
        usb_rem_task(xfer.device(), &xfer.abort_task);
        #[cfg(feature = "diagnostic")]
        ux.isdone.set(1);
        usb_transfer_complete(xfer);
        splx(s);
        return;
    }

    if xfer.device().bus().intr_context.get() != 0 || curproc().is_none() {
        panic(format_args!("uhci_abort_xfer: not in process context"));
    }

    // Step 1: Make interrupt routine and hardware ignore xfer.
    let s = splusb();
    xfer.status.set(status); // make software ignore it
    timeout_del(&xfer.timeout_handle);
    usb_rem_task(xfer.device(), &xfer.abort_task);
    let mut std = ux.stdstart.get();
    while let Some(t) = std {
        td_status_clear(t, UHCI_TD_ACTIVE | UHCI_TD_IOC);
        std = t.link_std();
    }
    splx(s);

    // Step 2: Wait until we know hardware has finished any possible use of the xfer. Also
    // make sure the soft interrupt routine has run.
    usb_delay_ms(&sc.sc_bus, 2); // Hardware finishes in 1ms
    let s = splusb();
    sc.sc_softwake.set(1);
    usb_schedsoftintr(&sc.sc_bus);
    let _ = tsleep_nsec(ptr::from_ref(&sc.sc_softwake), PZERO, "uhciab", INFSLP);
    splx(s);

    // Step 3: Execute callback.
    let s = splusb();
    #[cfg(feature = "diagnostic")]
    ux.isdone.set(1);
    usb_transfer_complete(xfer);
    splx(s);
}

/// `uhci_device_bulk_close`: close a device bulk pipe.
pub fn uhci_device_bulk_close(pipe: &'static UsbdPipe) {
    let sc = uhci_softc(pipe.device().bus());
    let upipe = uhci_pipe(pipe);

    uhci_free_sqh(sc, UhciPipe::sqh_of(&upipe.bulk_sqh));
    pipe.endpoint().savedtoggle.set(upipe.nexttoggle.get());
}

/// `uhci_device_ctrl_transfer`.
pub fn uhci_device_ctrl_transfer(xfer: &'static UsbdXfer) -> UsbdStatus {
    // Insert last in queue.
    let err = usb_insert_transfer(xfer);
    if err.is_err() {
        return err;
    }

    // Pipe isn't running (otherwise err would be USBD_INPROG), so start it first.
    uhci_device_ctrl_start(queue_first(xfer.pipe()))
}

/// `uhci_device_ctrl_start`.
pub fn uhci_device_ctrl_start(xfer: &'static UsbdXfer) -> UsbdStatus {
    let sc = uhci_softc(xfer.device().bus());

    if sc.sc_bus.dying.get() != 0 {
        return USBD_IOERROR;
    }

    #[cfg(feature = "diagnostic")]
    if xfer.rqflags.get() & crate::dev::usb::usbdivar::URQ_REQUEST == 0 {
        panic(format_args!("uhci_device_ctrl_transfer: not a request"));
    }

    let err = uhci_device_request(xfer);
    if err.is_err() {
        return err;
    }

    USBD_IN_PROGRESS
}

/// `uhci_device_intr_transfer`.
pub fn uhci_device_intr_transfer(xfer: &'static UsbdXfer) -> UsbdStatus {
    // Insert last in queue.
    let err = usb_insert_transfer(xfer);
    if err.is_err() {
        return err;
    }

    // Pipe isn't running (otherwise err would be USBD_INPROG), so start it first.
    uhci_device_intr_start(queue_first(xfer.pipe()))
}

/// Points every QH of an interrupt pipe at `data` (or terminates them with `None`).
fn uhci_intr_set_elink(upipe: &UhciPipe, data: Option<&'static UhciSoftTd>) {
    for i in 0..upipe.npoll.get() {
        let sqh = upipe.qh(i);
        sqh.elink.set(data);
        uhci_set(
            &sqh.qh.qh_elink,
            match data {
                Some(d) => d.physaddr.get() | UHCI_PTR_TD,
                None => UHCI_PTR_T,
            },
        );
    }
}

/// `uhci_device_intr_start`: hangs the data TDs on every QH of the pipe.
pub fn uhci_device_intr_start(xfer: &'static UsbdXfer) -> UsbdStatus {
    let sc = uhci_softc(xfer.device().bus());
    let upipe = uhci_pipe(xfer.pipe());
    let ux = uhci_xfer(xfer);

    if sc.sc_bus.dying.get() != 0 {
        return USBD_IOERROR;
    }

    #[cfg(feature = "diagnostic")]
    if xfer.rqflags.get() & crate::dev::usb::usbdivar::URQ_REQUEST != 0 {
        panic(format_args!("uhci_device_intr_start: a request"));
    }

    upipe.intr_isread.set(i32::from(usbd_xfer_isread(xfer)));

    let (data, dataend) = match uhci_alloc_std_chain(sc, xfer.length.get(), xfer) {
        Ok(c) => c,
        Err(err) => return err,
    };
    td_status_set(dataend, UHCI_TD_IOC);

    let s = splusb();
    // Set up interrupt info.
    ux.stdstart.set(Some(data));
    ux.stdend.set(Some(dataend));
    #[cfg(feature = "diagnostic")]
    uhci_mark_started(ux, "uhci_device_intr_transfer");

    uhci_intr_set_elink(upipe, Some(data));
    uhci_add_intr_list(sc, ux);
    xfer.status.set(USBD_IN_PROGRESS);
    splx(s);

    USBD_IN_PROGRESS
}

/// `uhci_device_ctrl_abort`: abort a device control request.
pub fn uhci_device_ctrl_abort(xfer: &'static UsbdXfer) {
    uhci_abort_xfer(xfer, USBD_CANCELLED);
}

/// `uhci_device_ctrl_close`: close a device control pipe. Nothing is freed (as in C).
pub fn uhci_device_ctrl_close(_pipe: &'static UsbdPipe) {}

/// `uhci_device_intr_abort`.
pub fn uhci_device_intr_abort(xfer: &'static UsbdXfer) {
    let pipe = xfer.pipe();
    kassert!(pipe.repeat.get() == 0 || pipe.intrxfer.get().is_some_and(|x| ptr::eq(x, xfer)));

    uhci_abort_xfer(xfer, USBD_CANCELLED);
}

/// `uhci_device_intr_close`: close a device interrupt pipe.
pub fn uhci_device_intr_close(pipe: &'static UsbdPipe) {
    let upipe = uhci_pipe(pipe);
    let sc = uhci_softc(pipe.device().bus());

    // Unlink descriptors from controller data structures.
    let qhs = upipe.qhs.get();
    let npoll = upipe.npoll.get();
    let s = splusb();
    for i in 0..npoll {
        uhci_remove_intr(sc, upipe.qh(i));
    }
    splx(s);

    // We now have to wait for any activity on the physical descriptors to stop.
    usb_delay_ms(&sc.sc_bus, 2);

    for i in 0..npoll {
        uhci_free_sqh(sc, upipe.qh(i));
    }
    if let Some(p) = NonNull::new(qhs) {
        free(
            p.cast(),
            M_USBHC,
            npoll as usize * size_of::<Option<&'static UhciSoftQh>>(),
        );
    }

    // XXX free other resources
}

/// `uhci_device_request`: a control transfer as a SETUP TD, the data TDs and a STATUS TD
/// hung on the pipe's QH, queued for low or full speed control.
pub fn uhci_device_request(xfer: &'static UsbdXfer) -> UsbdStatus {
    let sc = uhci_softc(xfer.device().bus());
    let upipe = uhci_pipe(xfer.pipe());
    let ux = uhci_xfer(xfer);
    let req = xfer.request.get();
    let addr = u32::from(xfer.device().address.get());
    let endpt = u32::from(xfer.pipe().endpoint().edesc().bEndpointAddress);

    let ls = if xfer.device().speed.get() == USB_SPEED_LOW {
        UHCI_TD_LS
    } else {
        0
    };
    let len = u32::from(ugetw(req.wLength));

    let setup = UhciPipe::td_of(&upipe.setup);
    let stat = UhciPipe::td_of(&upipe.stat);
    let sqh = UhciPipe::sqh_of(&upipe.ctl_sqh);

    // Set up data transaction
    let next = if len != 0 {
        upipe.nexttoggle.set(1);
        let (data, dataend) = match uhci_alloc_std_chain(sc, len, xfer) {
            Ok(c) => c,
            Err(err) => return err,
        };
        dataend.set_link_std(Some(stat));
        uhci_set(
            &dataend.td.td_link,
            stat.physaddr.get() | UHCI_PTR_VF | UHCI_PTR_TD,
        );
        data
    } else {
        stat
    };
    upipe.ctl_length.set(len);

    let reqlen = size_of::<UsbDeviceRequest>();
    // SAFETY: `uhci_open` allocated `sizeof(usb_device_request_t)` bytes of DMA memory for
    // the pipe's SETUP packet, which only this pipe's started transfer writes.
    unsafe {
        ptr::copy_nonoverlapping(req.as_bytes().as_ptr(), kernaddr(&upipe.reqdma, 0), reqlen)
    };

    setup.set_link_std(Some(next));
    uhci_set(
        &setup.td.td_link,
        next.physaddr.get() | UHCI_PTR_VF | UHCI_PTR_TD,
    );
    uhci_set(
        &setup.td.td_status,
        uhci_td_set_errcnt(3) | ls | UHCI_TD_ACTIVE,
    );
    uhci_set(
        &setup.td.td_token,
        uhci_td_setup(reqlen as u32, endpt, addr),
    );
    uhci_set(&setup.td.td_buffer, dmaaddr(&upipe.reqdma, 0) as u32);

    stat.set_link_std(None);
    uhci_set(&stat.td.td_link, UHCI_PTR_T);
    uhci_set(
        &stat.td.td_status,
        uhci_td_set_errcnt(3) | ls | UHCI_TD_ACTIVE | UHCI_TD_IOC,
    );
    uhci_set(
        &stat.td.td_token,
        if usbd_xfer_isread(xfer) {
            uhci_td_out(0, endpt, addr, 1)
        } else {
            uhci_td_in(0, endpt, addr, 1)
        },
    );
    uhci_set(&stat.td.td_buffer, 0);

    // Set up interrupt info.
    ux.stdstart.set(Some(setup));
    ux.stdend.set(Some(stat));
    #[cfg(feature = "diagnostic")]
    uhci_mark_started(ux, "uhci_device_request");

    sqh.elink.set(Some(setup));
    uhci_set(&sqh.qh.qh_elink, setup.physaddr.get() | UHCI_PTR_TD);

    let s = splusb();
    if xfer.device().speed.get() == USB_SPEED_LOW {
        uhci_add_ls_ctrl(sc, sqh);
    } else {
        uhci_add_hs_ctrl(sc, sqh);
    }
    uhci_add_intr_list(sc, ux);
    if xfer.timeout.get() != 0 && sc.sc_bus.use_polling.get() == 0 {
        uhci_arm_timeout(xfer);
    }
    xfer.status.set(USBD_IN_PROGRESS);
    splx(s);

    USBD_NORMAL_COMPLETION
}

/// `uhci_device_isoc_transfer`: enters every isochronous xfer in the schedule at once (they
/// fill the frames after the pipe's previous one), and starts it when the pipe was idle.
pub fn uhci_device_isoc_transfer(xfer: &'static UsbdXfer) -> UsbdStatus {
    // Put it on our queue,
    let err = usb_insert_transfer(xfer);

    // bail out on error,
    if err.is_err() && err != USBD_IN_PROGRESS {
        return err;
    }

    // XXX should check inuse here

    // insert into schedule,
    uhci_device_isoc_enter(xfer);

    // and start if the pipe wasn't running
    if !err.is_err() {
        let _ = uhci_device_isoc_start(queue_first(xfer.pipe()));
    }

    err
}

/// `uhci_device_isoc_enter`: fills the pipe's TDs for the xfer's frames, from the frame
/// after the previous xfer's (or a few frames ahead of the controller).
pub fn uhci_device_isoc_enter(xfer: &'static UsbdXfer) {
    let sc = uhci_softc(xfer.device().bus());
    let upipe = uhci_pipe(xfer.pipe());

    if sc.sc_bus.dying.get() != 0 {
        return;
    }

    if xfer.status.get() == USBD_IN_PROGRESS {
        // This request has already been entered into the frame list
        printf(format_args!(
            "uhci_device_isoc_enter: xfer={:p} in frame list\n",
            xfer
        ));
        // XXX
    }

    #[cfg(feature = "diagnostic")]
    if upipe.iso_inuse.get() as usize >= UHCI_VFRAMELIST_COUNT {
        printf(format_args!("uhci_device_isoc_enter: overflow!\n"));
    }

    usb_syncmem(
        &xfer.dmabuf,
        0,
        xfer.length.get() as usize,
        sync_ops(xfer, BUS_DMASYNC_PREREAD, BUS_DMASYNC_PREWRITE),
    );

    let mut next = upipe.iso_next.get();
    if next == -1 {
        // Not in use yet, schedule it a few frames ahead.
        next = ((u32::from(uread2(sc, UHCI_FRNUM)) + 3) % UHCI_VFRAMELIST_COUNT as u32) as i32;
    }

    xfer.status.set(USBD_IN_PROGRESS);
    uhci_xfer(xfer).curframe.set(next);

    let mut buf = dmaaddr(&xfer.dmabuf, 0) as u32;
    let mut status = uhci_td_zero_actlen(uhci_td_set_errcnt(0) | UHCI_TD_ACTIVE | UHCI_TD_IOS);
    let nframes = xfer.nframes.get();
    let s = splusb();
    for i in 0..nframes {
        let std = upipe.std(next);
        next += 1;
        if next as usize >= UHCI_VFRAMELIST_COUNT {
            next = 0;
        }
        let len = u32::from(frlength(xfer, i));
        if let Some(std) = std {
            uhci_set(&std.td.td_buffer, buf);
            if i == nframes - 1 {
                status |= UHCI_TD_IOC;
            }
            uhci_set(&std.td.td_status, status);
            let tok = &std.td.td_token;
            uhci_set(tok, uhci_get(tok) & !UHCI_TD_MAXLEN_MASK);
            uhci_set(tok, uhci_get(tok) | uhci_td_set_maxlen(len));
        }
        buf = buf.wrapping_add(len);
    }
    upipe.iso_next.set(next);
    upipe.iso_inuse.set(upipe.iso_inuse.get() + nframes);

    splx(s);
}

/// `uhci_device_isoc_start`: the xfer's last TD marks its completion.
pub fn uhci_device_isoc_start(xfer: &'static UsbdXfer) -> UsbdStatus {
    let sc = uhci_softc(xfer.device().bus());
    let upipe = uhci_pipe(xfer.pipe());
    let ux = uhci_xfer(xfer);

    if sc.sc_bus.dying.get() != 0 {
        return USBD_IOERROR;
    }

    #[cfg(feature = "diagnostic")]
    if xfer.status.get() != USBD_IN_PROGRESS {
        printf(format_args!(
            "uhci_device_isoc_start: not in progress {:p}\n",
            xfer
        ));
    }

    // Find the last TD
    let mut i = ux.curframe.get() + (xfer.nframes.get() - 1);
    if i as usize >= UHCI_VFRAMELIST_COUNT {
        i -= UHCI_VFRAMELIST_COUNT as i32;
    }
    let end = upipe.std(i);

    #[cfg(feature = "diagnostic")]
    if end.is_none() {
        printf(format_args!("uhci_device_isoc_start: end == NULL\n"));
        return USBD_INVAL;
    }

    let s = splusb();

    // Set up interrupt info.
    ux.stdstart.set(end);
    ux.stdend.set(end);
    #[cfg(feature = "diagnostic")]
    uhci_mark_started(ux, "uhci_device_isoc_start");
    uhci_add_intr_list(sc, ux);

    splx(s);

    USBD_IN_PROGRESS
}

/// `uhci_device_isoc_abort`: deactivates the xfer's TDs and completes it.
pub fn uhci_device_isoc_abort(xfer: &'static UsbdXfer) {
    let ux = uhci_xfer(xfer);
    let upipe = uhci_pipe(xfer.pipe());

    let s = splusb();

    // Transfer is already done.
    if xfer.status.get() != USBD_NOT_STARTED && xfer.status.get() != USBD_IN_PROGRESS {
        splx(s);
        return;
    }

    // Give xfer the requested abort code.
    xfer.status.set(USBD_CANCELLED);

    // make hardware ignore it,
    let nframes = xfer.nframes.get();
    let mut n = ux.curframe.get();
    let mut maxlen = 0;
    for _ in 0..nframes {
        if let Some(std) = upipe.std(n) {
            td_status_clear(std, UHCI_TD_ACTIVE | UHCI_TD_IOC);
            let len = uhci_td_get_maxlen(uhci_get(&std.td.td_token));
            if len > maxlen {
                maxlen = len;
            }
        }
        n += 1;
        if n as usize >= UHCI_VFRAMELIST_COUNT {
            n = 0;
        }
    }

    // and wait until we are sure the hardware has finished.
    delay(maxlen);

    #[cfg(feature = "diagnostic")]
    ux.isdone.set(1);
    // Run callback and remove from interrupt list.
    usb_transfer_complete(xfer);

    splx(s);
}

/// `uhci_device_isoc_close`: deactivates the pipe's TDs, unlinks them from the virtual
/// frame list and frees them.
pub fn uhci_device_isoc_close(pipe: &'static UsbdPipe) {
    let sc = uhci_softc(pipe.device().bus());
    let upipe = uhci_pipe(pipe);

    // Make sure all TDs are marked as inactive. Wait for completion. Unschedule.
    // Deallocate.
    for i in 0..UHCI_VFRAMELIST_COUNT as i32 {
        if let Some(std) = upipe.std(i) {
            td_status_clear(std, UHCI_TD_ACTIVE);
        }
    }
    usb_delay_ms(&sc.sc_bus, 2); // wait for completion

    let s = splusb();
    for i in 0..UHCI_VFRAMELIST_COUNT {
        let std = upipe.std(i as i32);
        let mut vstd = sc.sc_vframes[i].htd.get();
        while let Some(v) = vstd {
            let l = v.link_std();
            if l.is_some_and(|l| std.is_some_and(|t| ptr::eq(l, t))) {
                break;
            }
            vstd = l;
        }
        let (Some(v), Some(std)) = (vstd, std) else {
            // panic
            printf(format_args!(
                "uhci_device_isoc_close: {:p} not found\n",
                std.map_or(ptr::null(), ptr::from_ref)
            ));
            splx(s);
            return;
        };
        v.link.set(std.link.get());
        uhci_set(&v.td.td_link, uhci_get(&std.td.td_link));
        uhci_free_std(sc, std);
    }
    splx(s);

    if let Some(p) = NonNull::new(upipe.stds.get()) {
        free(
            p.cast(),
            M_USBHC,
            UHCI_VFRAMELIST_COUNT * size_of::<Option<&'static UhciSoftTd>>(),
        );
    }
}

/// `uhci_setup_isoc`: an isochronous pipe gets one inactive TD in every virtual frame.
pub fn uhci_setup_isoc(pipe: &'static UsbdPipe) -> UsbdStatus {
    let sc = uhci_softc(pipe.device().bus());
    let upipe = uhci_pipe(pipe);
    let addr = u32::from(pipe.device().address.get());
    let endpt = pipe.endpoint().edesc().bEndpointAddress;
    let rd = ue_get_dir(endpt) == UE_DIR_IN;
    let endpt = u32::from(endpt);
    let bytes = UHCI_VFRAMELIST_COUNT * size_of::<Option<&'static UhciSoftTd>>();

    let Some(mem) = mallocarray(
        UHCI_VFRAMELIST_COUNT,
        size_of::<Option<&'static UhciSoftTd>>(),
        M_USBHC,
        M_WAITOK,
    ) else {
        panic(format_args!(
            "uhci_setup_isoc: mallocarray(M_WAITOK) failed"
        ));
    };
    let stds = mem.as_ptr().cast::<Option<&'static UhciSoftTd>>();
    upipe.stds.set(stds);

    let token = if rd {
        uhci_td_in(0, endpt, addr, 0)
    } else {
        uhci_td_out(0, endpt, addr, 0)
    };

    // Allocate the TDs and mark as inactive;
    for i in 0..UHCI_VFRAMELIST_COUNT {
        let Some(std) = uhci_alloc_std(sc) else {
            // bad:
            for j in (0..i).rev() {
                if let Some(t) = upipe.std(j as i32) {
                    uhci_free_std(sc, t);
                }
            }
            free(mem, M_USBHC, bytes);
            return USBD_NOMEM;
        };
        uhci_set(&std.td.td_status, UHCI_TD_IOS); // iso, inactive
        uhci_set(&std.td.td_token, token);
        // SAFETY: `stds` has room for `UHCI_VFRAMELIST_COUNT` entries (`mallocarray` above);
        // writing initialises entry `i` before anything reads it.
        unsafe { stds.add(i).write(Some(std)) };
    }

    // Insert TDs into schedule.
    let s = splusb();
    for i in 0..UHCI_VFRAMELIST_COUNT {
        let Some(std) = upipe.std(i as i32) else {
            continue;
        };
        let vstd = sc.sc_vframes[i].htd();
        std.link.set(vstd.link.get());
        uhci_set(&std.td.td_link, uhci_get(&vstd.td.td_link));
        vstd.set_link_std(Some(std));
        uhci_set(&vstd.td.td_link, std.physaddr.get() | UHCI_PTR_TD);
    }
    splx(s);

    upipe.iso_next.set(-1);
    upipe.iso_inuse.set(0);

    USBD_NORMAL_COMPLETION
}

/// `uhci_device_isoc_done`: takes the xfer off the active list.
pub fn uhci_device_isoc_done(xfer: &'static UsbdXfer) {
    let ux = uhci_xfer(xfer);

    if !uhci_active_intr_list(ux) {
        return;
    }

    let Some(end) = ux.stdend.get() else {
        #[cfg(feature = "diagnostic")]
        printf(format_args!(
            "uhci_device_isoc_done: xfer={:p} stdend==NULL\n",
            xfer
        ));
        return;
    };

    // Turn off the interrupt since it is active even if the TD is not.
    td_status_clear(end, UHCI_TD_IOC);

    uhci_del_intr_list(ux); // remove from active list
}

/// `uhci_device_intr_done`: a repeating interrupt xfer is started again at once.
pub fn uhci_device_intr_done(xfer: &'static UsbdXfer) {
    let sc = uhci_softc(xfer.device().bus());
    let upipe = uhci_pipe(xfer.pipe());
    let ux = uhci_xfer(xfer);

    uhci_intr_set_elink(upipe, None);
    uhci_free_std_chain(sc, ux.stdstart.get(), None);

    // XXX Wasteful.
    if xfer.pipe().repeat.get() != 0 {
        // This alloc cannot fail since we freed the chain above.
        let (data, dataend) = match uhci_alloc_std_chain(sc, xfer.length.get(), xfer) {
            Ok(c) => c,
            Err(_) => panic(format_args!("uhci_device_intr_done: requeue failed")),
        };
        td_status_set(dataend, UHCI_TD_IOC);

        ux.stdstart.set(Some(data));
        ux.stdend.set(Some(dataend));
        #[cfg(feature = "diagnostic")]
        uhci_mark_started(ux, "uhci_device_intr_done");
        uhci_intr_set_elink(upipe, Some(data));
        xfer.status.set(USBD_IN_PROGRESS);
        // The ux is already on the examined list, just leave it.
    } else if uhci_active_intr_list(ux) {
        uhci_del_intr_list(ux);
    }
}

/// `uhci_device_ctrl_done`: deallocate request data structures.
pub fn uhci_device_ctrl_done(xfer: &'static UsbdXfer) {
    let sc = uhci_softc(xfer.device().bus());
    let upipe = uhci_pipe(xfer.pipe());
    let ux = uhci_xfer(xfer);

    #[cfg(feature = "diagnostic")]
    if xfer.rqflags.get() & crate::dev::usb::usbdivar::URQ_REQUEST == 0 {
        panic(format_args!("uhci_device_ctrl_done: not a request"));
    }

    if !uhci_active_intr_list(ux) {
        return;
    }

    uhci_del_intr_list(ux); // remove from active list

    let sqh = UhciPipe::sqh_of(&upipe.ctl_sqh);
    if xfer.device().speed.get() == USB_SPEED_LOW {
        uhci_remove_ls_ctrl(sc, sqh);
    } else {
        uhci_remove_hs_ctrl(sc, sqh);
    }

    if upipe.ctl_length.get() != 0 {
        let data = ux.stdstart.get().and_then(|s| s.link_std());
        uhci_free_std_chain(sc, data, ux.stdend.get());
    }
}

/// `uhci_device_bulk_done`: deallocate request data structures.
pub fn uhci_device_bulk_done(xfer: &'static UsbdXfer) {
    let sc = uhci_softc(xfer.device().bus());
    let upipe = uhci_pipe(xfer.pipe());
    let ux = uhci_xfer(xfer);

    if !uhci_active_intr_list(ux) {
        return;
    }

    uhci_del_intr_list(ux); // remove from active list

    uhci_remove_bulk(sc, UhciPipe::sqh_of(&upipe.bulk_sqh));

    uhci_free_std_chain(sc, ux.stdstart.get(), None);
}

/// `uhci_add_intr`: add interrupt QH to its virtual frame's list.
pub fn uhci_add_intr(sc: &'static UhciSoftc, sqh: &'static UhciSoftQh) {
    let vf = &sc.sc_vframes[sqh.pos.get() as usize];

    let eqh = vf.eqh();
    uhci_append_qh(eqh, sqh);
    vf.eqh.set(Some(sqh));
    vf.bandwidth.set(vf.bandwidth.get().wrapping_add(1));
}

/// `uhci_remove_intr`: remove interrupt QH.
pub fn uhci_remove_intr(sc: &'static UhciSoftc, sqh: &'static UhciSoftQh) {
    let vf = &sc.sc_vframes[sqh.pos.get() as usize];

    // See comment in uhci_remove_ctrl()
    let pqh = uhci_unlink_qh(vf.hqh(), sqh);
    if vf.eqh.get().is_some_and(|e| ptr::eq(e, sqh)) {
        vf.eqh.set(Some(pqh));
    }
    vf.bandwidth.set(vf.bandwidth.get().wrapping_sub(1));
}

/// `MOD(i)`: a virtual frame index.
const fn vframe_mod(i: u32) -> u32 {
    i & (UHCI_VFRAMELIST_COUNT as u32 - 1)
}

/// The offset in the schedule (below `ival`) whose `npoll` virtual frames have the most
/// bandwidth left over, given each frame's bandwidth: `uhci_device_setintr`'s search.
pub fn uhci_best_offset(bandwidth: impl Fn(usize) -> u32, ival: u32, npoll: u32) -> u32 {
    let mut bestoffs = 0;
    let mut bestbw = !0u32;
    for offs in 0..ival {
        let mut bw: u32 = 0;
        for i in 0..npoll {
            bw = bw.wrapping_add(bandwidth(vframe_mod(i * ival + offs) as usize));
        }
        if bw < bestbw {
            bestbw = bw;
            bestoffs = offs;
        }
    }
    bestoffs
}

/// `uhci_device_setintr`: one QH per `ival` virtual frames, at the offset with the most
/// bandwidth left over.
pub fn uhci_device_setintr(sc: &'static UhciSoftc, upipe: &UhciPipe, ival: i32) -> UsbdStatus {
    if ival == 0 {
        printf(format_args!("uhci_device_setintr: 0 interval\n"));
        return USBD_INVAL;
    }

    let ival = (ival as u32).min(UHCI_VFRAMELIST_COUNT as u32);
    let npoll = (UHCI_VFRAMELIST_COUNT as u32).div_ceil(ival);

    let qbytes = npoll as usize * size_of::<Option<&'static UhciSoftQh>>();
    let Some(mem) = mallocarray(
        npoll as usize,
        size_of::<Option<&'static UhciSoftQh>>(),
        M_USBHC,
        M_NOWAIT,
    ) else {
        return USBD_NOMEM;
    };
    let qhs = mem.as_ptr().cast::<Option<&'static UhciSoftQh>>();

    // Figure out which offset in the schedule that has most bandwidth left over.
    let bestoffs = uhci_best_offset(|f| sc.sc_vframes[f].bandwidth.get(), ival, npoll);

    for i in 0..npoll as usize {
        let Some(sqh) = uhci_alloc_sqh(sc) else {
            for j in (0..i).rev() {
                // SAFETY: entries `0..i` were written by the earlier iterations.
                if let Some(q) = unsafe { *qhs.add(j) } {
                    uhci_free_sqh(sc, q);
                }
            }
            free(mem, M_USBHC, qbytes);
            return USBD_NOMEM;
        };
        sqh.elink.set(None);
        uhci_set(&sqh.qh.qh_elink, UHCI_PTR_T);
        sqh.pos.set(vframe_mod(i as u32 * ival + bestoffs) as i32);
        // SAFETY: `qhs` has room for `npoll` entries (`mallocarray` above); writing
        // initialises entry `i` before anything reads it.
        unsafe { qhs.add(i).write(Some(sqh)) };
    }

    upipe.npoll.set(npoll as i32);
    upipe.qhs.set(qhs);

    let s = splusb();
    // Enter QHs into the controller data structures.
    for i in 0..npoll as i32 {
        uhci_add_intr(sc, upipe.qh(i));
    }
    splx(s);

    USBD_NORMAL_COMPLETION
}

/// `uhci_open`: the bus's `open_pipe`. Open a new pipe.
pub fn uhci_open(pipe: &'static UsbdPipe) -> UsbdStatus {
    let sc = uhci_softc(pipe.device().bus());
    let upipe = uhci_pipe(pipe);
    let ed = pipe.endpoint().edesc();

    upipe.nexttoggle.set(pipe.endpoint().savedtoggle.get());

    // Root Hub
    if pipe.device().depth.get() == 0 {
        match ed.bEndpointAddress {
            USB_CONTROL_ENDPOINT => pipe.methods.set(Some(&UHCI_ROOT_CTRL_METHODS)),
            a if a == UE_DIR_IN | UHCI_INTR_ENDPT => {
                pipe.methods.set(Some(&UHCI_ROOT_INTR_METHODS))
            }
            _ => return USBD_INVAL,
        }
    } else {
        match ue_get_xfertype(ed.bmAttributes) {
            UE_CONTROL => {
                pipe.methods.set(Some(&UHCI_DEVICE_CTRL_METHODS));
                let Some(sqh) = uhci_alloc_sqh(sc) else {
                    return USBD_NOMEM;
                };
                upipe.ctl_sqh.set(Some(sqh));
                let Some(setup) = uhci_alloc_std(sc) else {
                    uhci_free_sqh(sc, sqh);
                    return USBD_NOMEM;
                };
                upipe.setup.set(Some(setup));
                let Some(stat) = uhci_alloc_std(sc) else {
                    uhci_free_sqh(sc, sqh);
                    uhci_free_std(sc, setup);
                    return USBD_NOMEM;
                };
                upipe.stat.set(Some(stat));
                let err = usb_allocmem(
                    &sc.sc_bus,
                    size_of::<UsbDeviceRequest>(),
                    0,
                    USB_DMA_COHERENT,
                    &upipe.reqdma,
                );
                if err.is_err() {
                    uhci_free_sqh(sc, sqh);
                    uhci_free_std(sc, setup);
                    uhci_free_std(sc, stat);
                    return USBD_NOMEM;
                }
            }
            UE_INTERRUPT => {
                pipe.methods.set(Some(&UHCI_DEVICE_INTR_METHODS));
                let mut ival = pipe.interval.get();
                if ival == USBD_DEFAULT_INTERVAL {
                    ival = i32::from(ed.bInterval);
                }
                return uhci_device_setintr(sc, upipe, ival);
            }
            UE_ISOCHRONOUS => {
                pipe.methods.set(Some(&UHCI_DEVICE_ISOC_METHODS));
                return uhci_setup_isoc(pipe);
            }
            UE_BULK => {
                pipe.methods.set(Some(&UHCI_DEVICE_BULK_METHODS));
                let Some(sqh) = uhci_alloc_sqh(sc) else {
                    return USBD_NOMEM;
                };
                upipe.bulk_sqh.set(Some(sqh));
            }
            _ => {}
        }
    }
    USBD_NORMAL_COMPLETION
}

/// `uhci_portreset`: resets a root port and enables it.
///
/// The USB hub protocol requires that SET_FEATURE(PORT_RESET) also enables the port, and
/// also states that SET_FEATURE(PORT_ENABLE) should not be used by the USB subsystem. As we
/// cannot issue a SET_FEATURE(PORT_ENABLE) externally, we must ensure that the port will be
/// enabled as part of the reset.
///
/// On the VT83C572, the port cannot be successfully enabled until the outstanding "port
/// enable change" and "connection status change" events have been reset.
pub fn uhci_portreset(sc: &UhciSoftc, index: i32) -> UsbdStatus {
    let port = match index {
        1 => UHCI_PORTSC1,
        2 => UHCI_PORTSC2,
        _ => return USBD_IOERROR,
    };

    let x = urwmask(uread2(sc, port));
    uwrite2(sc, port, x | UHCI_PORTSC_PR);

    usb_delay_ms(&sc.sc_bus, USB_PORT_ROOT_RESET_DELAY);

    let x = urwmask(uread2(sc, port));
    uwrite2(sc, port, x & !UHCI_PORTSC_PR);

    delay(100);

    let x = urwmask(uread2(sc, port));
    uwrite2(sc, port, x | UHCI_PORTSC_PE);

    let mut lim = 10;
    loop {
        lim -= 1;
        if lim <= 0 {
            break;
        }
        usb_delay_ms(&sc.sc_bus, USB_PORT_RESET_DELAY);

        let x = uread2(sc, port);

        if x & UHCI_PORTSC_CCS == 0 {
            // No device is connected (or was disconnected during reset). Consider the port
            // reset. The delay must be long enough to ensure on the initial iteration that
            // the device connection will have been registered. 50ms appears to be
            // sufficient, but 20ms is not.
            break;
        }

        if x & (UHCI_PORTSC_POEDC | UHCI_PORTSC_CSC) != 0 {
            // Port enabled changed and/or connection status changed were set. Reset either
            // or both raised flags (by writing a 1 to that bit), and wait again for state to
            // settle.
            uwrite2(
                sc,
                port,
                urwmask(x) | (x & (UHCI_PORTSC_POEDC | UHCI_PORTSC_CSC)),
            );
            continue;
        }

        if x & UHCI_PORTSC_PE != 0 {
            // Port is enabled
            break;
        }

        uwrite2(sc, port, urwmask(x) | UHCI_PORTSC_PE);
    }

    if lim <= 0 {
        return USBD_TIMEOUT;
    }

    sc.sc_isreset.set(1);
    USBD_NORMAL_COMPLETION
}

/// `uhci_root_ctrl_transfer`: simulate a hardware hub by handling all the necessary
/// requests.
pub fn uhci_root_ctrl_transfer(xfer: &'static UsbdXfer) -> UsbdStatus {
    // Insert last in queue.
    let err = usb_insert_transfer(xfer);
    if err.is_err() {
        return err;
    }

    // Pipe isn't running (otherwise err would be USBD_INPROG), so start it first.
    uhci_root_ctrl_start(queue_first(xfer.pipe()))
}

/// The port status register of root port `index` (1 or 2).
fn uhci_portsc(index: u16) -> Option<BusSize> {
    match index {
        1 => Some(UHCI_PORTSC1),
        2 => Some(UHCI_PORTSC2),
        _ => None,
    }
}

/// `uhci_root_ctrl_start`: answers a request to the emulated root hub from the port
/// registers and completes it at once.
pub fn uhci_root_ctrl_start(xfer: &'static UsbdXfer) -> UsbdStatus {
    let sc = uhci_softc(xfer.device().bus());

    if sc.sc_bus.dying.get() != 0 {
        return USBD_IOERROR;
    }

    #[cfg(feature = "diagnostic")]
    if xfer.rqflags.get() & crate::dev::usb::usbdivar::URQ_REQUEST == 0 {
        panic(format_args!("uhci_root_ctrl_start: not a request"));
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

    // `Err(err)` is the C's `goto ret`, which leaves `actlen` alone; `Ok(totlen)` the length
    // moved.
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
                    let mut devd = UHCI_DEVD;
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
                        &UHCI_CONFD.as_bytes()[..USB_CONFIG_DESCRIPTOR_SIZE],
                        &UHCI_IFCD.as_bytes()[..USB_INTERFACE_DESCRIPTOR_SIZE],
                        &UHCI_ENDPD.as_bytes()[..USB_ENDPOINT_DESCRIPTOR_SIZE],
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
                        2 => Some(b"UHCI root hub"), // Product
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
                let Some(port) = uhci_portsc(index) else {
                    break 'ret Err(USBD_IOERROR);
                };
                match i32::from(value) {
                    UHF_PORT_ENABLE => {
                        let x = urwmask(uread2(sc, port));
                        uwrite2(sc, port, x & !UHCI_PORTSC_PE);
                    }
                    UHF_PORT_SUSPEND => {
                        let x = urwmask(uread2(sc, port));
                        uwrite2(sc, port, x & !UHCI_PORTSC_SUSP);
                    }
                    UHF_PORT_RESET => {
                        let x = urwmask(uread2(sc, port));
                        uwrite2(sc, port, x & !UHCI_PORTSC_PR);
                    }
                    UHF_C_PORT_CONNECTION => {
                        let x = urwmask(uread2(sc, port));
                        uwrite2(sc, port, x | UHCI_PORTSC_CSC);
                    }
                    UHF_C_PORT_ENABLE => {
                        let x = urwmask(uread2(sc, port));
                        uwrite2(sc, port, x | UHCI_PORTSC_POEDC);
                    }
                    UHF_C_PORT_OVER_CURRENT => {
                        let x = urwmask(uread2(sc, port));
                        uwrite2(sc, port, x | UHCI_PORTSC_OCIC);
                    }
                    UHF_C_PORT_RESET => {
                        sc.sc_isreset.set(0);
                        break 'ret Err(USBD_NORMAL_COMPLETION);
                    }
                    // UHF_PORT_CONNECTION, UHF_PORT_OVER_CURRENT, UHF_PORT_POWER,
                    // UHF_PORT_LOW_SPEED, UHF_C_PORT_SUSPEND and the rest
                    _ => break 'ret Err(USBD_IOERROR),
                }
            }
            (UR_GET_BUS_STATE, UT_READ_CLASS_OTHER) => {
                let Some(port) = uhci_portsc(index) else {
                    break 'ret Err(USBD_IOERROR);
                };
                if len > 0 {
                    buf[0] = ((uread2(sc, port) & UHCI_PORTSC_LS) >> UHCI_PORTSC_LS_SHIFT) as u8;
                    totlen = 1;
                }
            }
            (UR_GET_DESCRIPTOR, UT_READ_CLASS_DEVICE) => {
                if value & 0xff != 0 {
                    break 'ret Err(USBD_IOERROR);
                }
                let l = len
                    .min(USB_HUB_DESCRIPTOR_SIZE)
                    .min(size_of::<UsbHubDescriptor>());
                totlen = l;
                buf[..l].copy_from_slice(&UHCI_HUBD_PIIX.as_bytes()[..l]);
            }
            (UR_GET_STATUS, UT_READ_CLASS_DEVICE) => {
                if len != 4 {
                    break 'ret Err(USBD_IOERROR);
                }
                buf.fill(0);
                totlen = len;
            }
            (UR_GET_STATUS, UT_READ_CLASS_OTHER) => {
                let Some(port) = uhci_portsc(index) else {
                    break 'ret Err(USBD_IOERROR);
                };
                if len != 4 {
                    break 'ret Err(USBD_IOERROR);
                }
                let x = uread2(sc, port);
                let (status, change) = uhci_port_status(x, sc.sc_isreset.get() != 0);
                let mut ps = UsbPortStatus::zeroed();
                usetw(&mut ps.wPortStatus, status);
                usetw(&mut ps.wPortChange, change);
                let l = len.min(size_of::<UsbPortStatus>());
                buf[..l].copy_from_slice(&ps.as_bytes()[..l]);
                totlen = l;
            }
            (UR_SET_DESCRIPTOR, UT_WRITE_CLASS_DEVICE) => break 'ret Err(USBD_IOERROR),
            (UR_SET_FEATURE, UT_WRITE_CLASS_DEVICE) => {}
            (UR_SET_FEATURE, UT_WRITE_CLASS_OTHER) => {
                let Some(port) = uhci_portsc(index) else {
                    break 'ret Err(USBD_IOERROR);
                };
                match i32::from(value) {
                    UHF_PORT_ENABLE => {
                        let x = urwmask(uread2(sc, port));
                        uwrite2(sc, port, x | UHCI_PORTSC_PE);
                    }
                    UHF_PORT_SUSPEND => {
                        let x = urwmask(uread2(sc, port));
                        uwrite2(sc, port, x | UHCI_PORTSC_SUSP);
                    }
                    UHF_PORT_RESET => {
                        break 'ret Err(uhci_portreset(sc, i32::from(index)));
                    }
                    UHF_PORT_POWER => {
                        // Pretend we turned on power
                        break 'ret Err(USBD_NORMAL_COMPLETION);
                    }
                    UHF_PORT_DISOWN_TO_1_1 => {
                        // accept, but do nothing
                        break 'ret Err(USBD_NORMAL_COMPLETION);
                    }
                    // UHF_C_PORT_CONNECTION, UHF_C_PORT_ENABLE, UHF_C_PORT_OVER_CURRENT,
                    // UHF_PORT_CONNECTION, UHF_PORT_OVER_CURRENT, UHF_PORT_LOW_SPEED,
                    // UHF_C_PORT_SUSPEND, UHF_C_PORT_RESET and the rest
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

/// The hub port status and change words of a port status register value (`UR_GET_STATUS`
/// of a root port); `isreset` is `sc_isreset`.
pub fn uhci_port_status(x: u16, isreset: bool) -> (u16, u16) {
    let mut status = 0;
    let mut change = 0;
    if x & UHCI_PORTSC_CCS != 0 {
        status |= UPS_CURRENT_CONNECT_STATUS;
    }
    if x & UHCI_PORTSC_CSC != 0 {
        change |= UPS_C_CONNECT_STATUS;
    }
    if x & UHCI_PORTSC_PE != 0 {
        status |= UPS_PORT_ENABLED;
    }
    if x & UHCI_PORTSC_POEDC != 0 {
        change |= UPS_C_PORT_ENABLED;
    }
    if x & UHCI_PORTSC_OCI != 0 {
        status |= UPS_OVERCURRENT_INDICATOR;
    }
    if x & UHCI_PORTSC_OCIC != 0 {
        change |= UPS_C_OVERCURRENT_INDICATOR;
    }
    if x & UHCI_PORTSC_SUSP != 0 {
        status |= UPS_SUSPEND;
    }
    if x & UHCI_PORTSC_LSDA != 0 {
        status |= UPS_LOW_SPEED;
    }
    status |= UPS_PORT_POWER;
    if isreset {
        change |= UPS_C_PORT_RESET;
    }
    (status, change)
}

/// `uhci_root_ctrl_abort`: abort a root control request. Nothing to do, all transfers are
/// synchronous.
pub fn uhci_root_ctrl_abort(_xfer: &'static UsbdXfer) {}

/// `uhci_root_ctrl_close`: close the root pipe.
pub fn uhci_root_ctrl_close(_pipe: &'static UsbdPipe) {}

/// `uhci_root_intr_abort`: stops polling the ports and completes the xfer.
pub fn uhci_root_intr_abort(xfer: &'static UsbdXfer) {
    let sc = uhci_softc(xfer.device().bus());

    timeout_del(&sc.sc_root_intr);
    sc.sc_intrxfer.set(None);

    xfer.status.set(USBD_CANCELLED);
    let s = splusb();
    usb_transfer_complete(xfer);
    splx(s);
}

/// `uhci_root_intr_transfer`.
pub fn uhci_root_intr_transfer(xfer: &'static UsbdXfer) -> UsbdStatus {
    // Insert last in queue.
    let err = usb_insert_transfer(xfer);
    if err.is_err() {
        return err;
    }

    // Pipe isn't running (otherwise err would be USBD_INPROG), start first
    uhci_root_intr_start(queue_first(xfer.pipe()))
}

/// `uhci_root_intr_start`: start a transfer on the root interrupt pipe; the ports are
/// polled every 255 ms.
pub fn uhci_root_intr_start(xfer: &'static UsbdXfer) -> UsbdStatus {
    let sc = uhci_softc(xfer.device().bus());

    if sc.sc_bus.dying.get() != 0 {
        return USBD_IOERROR;
    }

    sc.sc_intrxfer.set(Some(xfer));
    timeout_add_msec(&sc.sc_root_intr, 255);

    USBD_IN_PROGRESS
}

/// `uhci_root_intr_close`.
pub fn uhci_root_intr_close(_pipe: &'static UsbdPipe) {}

/// `uhci_root_intr_done`: a repeating root hub xfer polls again.
pub fn uhci_root_intr_done(xfer: &'static UsbdXfer) {
    let sc = uhci_softc(xfer.device().bus());

    if xfer.pipe().repeat.get() != 0 {
        timeout_add_msec(&sc.sc_root_intr, 255);
    }
}

const _: () = {
    // `pool_init(uhcixfer, sizeof(struct uhci_xfer), 0, ...)`: the pool's default alignment
    // (`ALIGN(1)`, at least 8) must suit the type `uhci_allocx` casts its items to.
    assert!(align_of::<UhciXfer>() <= 8);
    // The chunks are larger than `USB_MEM_SMALL`, so each gets a block of its own from
    // `usb_allocmem`, at least `UHCI_TD_ALIGN`-aligned, as the carving needs.
    assert!(UHCI_STD_SIZE * UHCI_STD_CHUNK > crate::dev::usb::usb_mem::USB_MEM_SMALL);
    assert!(UHCI_SQH_SIZE * UHCI_SQH_CHUNK > crate::dev::usb::usb_mem::USB_MEM_SMALL);
};
/* </CODE> */

/* <TESTS> */
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn root_hub_descriptors() {
        assert_eq!(UHCI_DEVD.as_bytes().len(), USB_DEVICE_DESCRIPTOR_SIZE);
        assert_eq!(&UHCI_DEVD.as_bytes()[..4], &[18, UDESC_DEVICE, 0x00, 0x01]);
        assert_eq!(UHCI_CONFD.wTotalLength, [9 + 9 + 7, 0]);
        assert_eq!(UHCI_ENDPD.bEndpointAddress, 0x81);
        assert_eq!(UHCI_ENDPD.bInterval, 255);
        let hubd = &UHCI_HUBD_PIIX.as_bytes()[..USB_HUB_DESCRIPTOR_SIZE];
        // Two ports, no power switching, individual over-current, 50 * 2 ms to power good.
        assert_eq!(
            &hubd[..6],
            &[USB_HUB_DESCRIPTOR_SIZE as u8, UDESC_HUB, 2, 0x0a, 0, 50]
        );
    }

    #[test]
    fn port_status_words() {
        // QEMU's port with a full speed device after the reset: connected, enabled, with the
        // connect change still set.
        let (st, ch) = uhci_port_status(UHCI_PORTSC_CCS | UHCI_PORTSC_CSC | UHCI_PORTSC_PE, true);
        assert_eq!(
            st,
            UPS_CURRENT_CONNECT_STATUS | UPS_PORT_ENABLED | UPS_PORT_POWER
        );
        assert_eq!(ch, UPS_C_CONNECT_STATUS | UPS_C_PORT_RESET);
        // A low speed device, over-current, suspended; power is always reported.
        let (st, ch) = uhci_port_status(
            UHCI_PORTSC_LSDA | UHCI_PORTSC_OCI | UHCI_PORTSC_OCIC | UHCI_PORTSC_SUSP,
            false,
        );
        assert_eq!(
            st,
            UPS_LOW_SPEED | UPS_OVERCURRENT_INDICATOR | UPS_SUSPEND | UPS_PORT_POWER
        );
        assert_eq!(ch, UPS_C_OVERCURRENT_INDICATOR);
        assert_eq!(uhci_port_status(0, false), (UPS_PORT_POWER, 0));
    }

    #[test]
    fn interrupt_schedule_offset() {
        // An empty schedule: offset 0.
        assert_eq!(uhci_best_offset(|_| 0, 8, 16), 0);
        // Frames 0, 8, 16, ... busy: the next offset wins.
        assert_eq!(uhci_best_offset(|f| u32::from(f % 8 == 0), 8, 16), 1);
        // Every offset of a 4 ms pipe but 3 has a pipe: offset 3.
        assert_eq!(uhci_best_offset(|f| u32::from(f % 4 != 3), 4, 32), 3);
        // ival 128: one QH, the first frame with the least bandwidth.
        assert_eq!(uhci_best_offset(|f| if f == 5 { 0 } else { 2 }, 128, 1), 5);
        assert_eq!(vframe_mod(130), 2);
    }
}
/* </TESTS> */
