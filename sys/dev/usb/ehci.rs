/* <LICENSES> */
/*
 * Copyright (c) 2014-2015 Martin Pieuchot
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
 * Copyright (c) 2004-2008 The NetBSD Foundation, Inc.
 * All rights reserved.
 *
 * This code is derived from software contributed to The NetBSD Foundation
 * by Lennart Augustsson (lennart@augustsson.net), Charles M. Hannum and
 * Jeremy Morse (jeremy.morse@gmail.com).
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
//! ehci(4): the Enhanced Host Controller Interface (USB 2.0) driver, machine independent
//! (`dev/usb/ehci.c`). Bus front-ends (`dev/pci/ehci_pci.rs`) map the registers, establish
//! the interrupt, call [`ehci_init`] and attach `usb(4)`.
//!
//! Upstream: sys/dev/usb/ehci.c @ 3ce1f3f79392
//!
//! The controller runs two schedules from DMA memory. The asynchronous one is a circular
//! list of queue heads (one per control or bulk pipe, headed by a dummy QH) the controller
//! walks round robin; the periodic one is the frame list, one link per 1 ms frame, pointing
//! at a tree of `EHCI_INTRQHS` dummy interrupt QHs (poll intervals 1 to 128 ms) that the
//! interrupt pipes' QHs hang from, preceded in each frame by the isochronous descriptors
//! (iTDs for high speed devices, siTDs for full speed ones behind a transaction translator).
//! A transfer is a chain of qTDs hung on its pipe's QH (`ehci_set_qh_qtd`). The interrupt
//! only says that something finished: the soft interrupt scans every active transfer
//! (`sc_intrhead`). A QH leaves the async list only after the "interrupt on async advance"
//! doorbell says the controller no longer holds it (`ehci_sync_hc`). The root hub is
//! emulated (`ehci_root_ctrl_start`); a port change completes its interrupt transfer.
//! Low and full speed devices on a root port are handed to the companion controller
//! (`ehci_disown`, `EHCI_PS_PO`).
//!
//! Locking is the C's: everything but the hard interrupt runs under the kernel lock at
//! `splusb()`; the doorbell is serialised by `sc_doorbell_lock`. The hard interrupt
//! (`ehci_intr`, established `IPL_MPSAFE` by the front-end) reads and acknowledges the status
//! register, wakes the doorbell's sleeper and schedules the soft interrupt.
//!
//! ## Deviations
//! - `EHCI_DEBUG` is not in GENERIC: the `DPRINTF`s and the `ehci_dump_*` functions are
//!   absent, as in a kernel built without it. The `DIAGNOSTIC` blocks (`isdone`, the
//!   messages of `ehci_freex`, `ehci_device_clear_toggle`, `ehci_sync_hc` and the start
//!   functions, `ehci_activate`'s non-empty list check) are under feature `diagnostic`.
//! - `struct pool *ehcixfer`, `malloc`ed by the first `ehci_init`, is a static [`Pool`] with
//!   a flag for the C's `ehcixfer == NULL`; its "unable to allocate pool descriptor" path
//!   cannot happen.
//! - `ehci_init` returns a `UsbdStatus`; where the C returns the errno `ENOMEM` through it
//!   (no pool descriptor, no `sc_softitds`) it returns `USBD_NOMEM`. The front-end only prints
//!   the value.
//! - `ehci_setaddr` returns `Err(EINVAL)` where the C returns `EINVAL` and `Err(EPERM)` where
//!   it returns the literal 1 (`EPERM`'s value); `ehci_activate`'s `DIAGNOSTIC` `-1` is
//!   `Err(ERESTART)` (`-1`). The callers only test for nonzero.
//! - The casts `(struct ehci_softc *)bus`, `(struct ehci_pipe *)pipe` and `(struct ehci_xfer
//!   *)xfer` are checked: [`ehci_softc`] checks the bus's methods are this driver's, the pipe
//!   and xfer casts go through `UsbdPipe::hc`/`UsbdXfer::hc`.
//! - `struct ehci_pipe`'s union `u` is split into its members (`reqdma`, `next_frame`,
//!   `cur_xfers`).
//! - `ehci_alloc_sqtd_chain` returns the chain's first and last qTD instead of filling
//!   `sp`/`ep`; its `DIAGNOSTIC` check of the buffer index is always made (the index cannot
//!   pass 4, see the function), the message only under `diagnostic`. `ehci_alloc_itd_chain`
//!   and `ehci_alloc_sitd_chain` return `Err(())` for the C's 1.
//! - `TAILQ_REMOVE(&sc->sc_intrhead, ex, inext)` is [`ehci_intrhead_remove`], which unlinks
//!   an xfer only if it is on the list (the C would corrupt the list otherwise; every path
//!   that removes reaches a linked xfer).
//! - Walking a qTD chain that ends before the C's stop condition (a NULL `nextqtd` the C would
//!   dereference) stops the walk.
//! - The root hub's `UDESC_DEVICE_QUALIFIER` copies the 10 bytes of `ehci_odevd` and zeroes
//!   the rest of the `min(len, USB_DEVICE_DESCRIPTOR_SIZE)` it reports, where the C copies
//!   whatever follows the structure in memory; the hub descriptor copies at most the
//!   structure's own size. String descriptors are built in a local `usb_string_descriptor_t`
//!   and copied, as in `xhci.rs`.
//! - Arming an xfer's timeout (`timeout_del`, `timeout_set`, `timeout_add_msec`) is
//!   [`ehci_arm_timeout`], shared by the start functions.

use core::cell::Cell;
use core::ffi::c_void;
use core::mem::offset_of;
use core::ptr::{self, NonNull};
use core::sync::atomic::{AtomicBool, Ordering};

use crate::dev::rnd::arc4random;
use crate::dev::usb::ehcireg::*;
use crate::dev::usb::ehcivar::{
    EHCI_INTRQHS, EHCI_IPOLLRATES, EHCI_ITD_CHUNK, EHCI_ITD_SIZE, EHCI_SQH_CHUNK, EHCI_SQH_SIZE,
    EHCI_SQTD_CHUNK, EHCI_SQTD_SIZE, EHCI_XFER_ABORTING, EHCI_XFER_ABORTWAIT,
    EHCIF_DROPPED_INTR_WORKAROUND, EHCIF_PCB_INTR, EHCIF_USBMODE, EhciSoftItd, EhciSoftItdFreeList,
    EhciSoftQh, EhciSoftQtd, EhciSoftc, EhciXfer, EhciXferIntrList, ehci_ilev_ival, ehci_iqhidx,
    eoread4, eowrite4, eread1, eread4, ewrite4,
};
use crate::dev::usb::usb::{
    UC_BUS_POWERED, UC_SELF_POWERED, UDCLASS_HUB, UDESC_CONFIG, UDESC_DEVICE,
    UDESC_DEVICE_QUALIFIER, UDESC_ENDPOINT, UDESC_HUB, UDESC_INTERFACE,
    UDESC_OTHER_SPEED_CONFIGURATION, UDESC_STRING, UDPROTO_FSHUB, UDPROTO_HSHUBSTT,
    UDS_SELF_POWERED, UDSUBCLASS_HUB, UE_BULK, UE_CONTROL, UE_DIR_IN, UE_INTERRUPT, UE_ISOCHRONOUS,
    UHD_PORT_IND, UHD_PWR_INDIVIDUAL, UHD_PWR_NO_SWITCH, UHF_C_PORT_CONNECTION, UHF_C_PORT_ENABLE,
    UHF_C_PORT_OVER_CURRENT, UHF_C_PORT_RESET, UHF_C_PORT_SUSPEND, UHF_PORT_DISOWN_TO_1_1,
    UHF_PORT_ENABLE, UHF_PORT_INDICATOR, UHF_PORT_POWER, UHF_PORT_RESET, UHF_PORT_SUSPEND,
    UHF_PORT_TEST, UICLASS_HUB, UIPROTO_HSHUBSTT, UISUBCLASS_HUB, UPS_C_CONNECT_STATUS,
    UPS_C_OVERCURRENT_INDICATOR, UPS_C_PORT_ENABLED, UPS_C_PORT_RESET, UPS_CURRENT_CONNECT_STATUS,
    UPS_HIGH_SPEED, UPS_OVERCURRENT_INDICATOR, UPS_PORT_ENABLED, UPS_PORT_POWER, UPS_RESET,
    UPS_SUSPEND, UR_CLEAR_FEATURE, UR_CLEAR_TT_BUFFER, UR_GET_CONFIG, UR_GET_DESCRIPTOR,
    UR_GET_INTERFACE, UR_GET_STATUS, UR_GET_TT_STATE, UR_RESET_TT, UR_SET_ADDRESS, UR_SET_CONFIG,
    UR_SET_DESCRIPTOR, UR_SET_FEATURE, UR_SET_INTERFACE, UR_STOP_TT, UR_SYNCH_FRAME,
    USB_CONFIG_DESCRIPTOR_SIZE, USB_CONTROL_ENDPOINT, USB_DEVICE_DESCRIPTOR_SIZE,
    USB_ENDPOINT_DESCRIPTOR_SIZE, USB_HUB_DESCRIPTOR_SIZE, USB_INTERFACE_DESCRIPTOR_SIZE,
    USB_MAX_DEVICES, USB_PORT_ROOT_RESET_DELAY, USB_RESUME_WAIT, USB_SPEED_FULL, USB_SPEED_HIGH,
    USB_SPEED_LOW, UT_READ_CLASS_DEVICE, UT_READ_CLASS_OTHER, UT_READ_DEVICE, UT_READ_ENDPOINT,
    UT_READ_INTERFACE, UT_WRITE_CLASS_DEVICE, UT_WRITE_CLASS_OTHER, UT_WRITE_DEVICE,
    UT_WRITE_ENDPOINT, UT_WRITE_INTERFACE, UsbConfigDescriptor, UsbDeviceDescriptor,
    UsbDeviceQualifier, UsbDeviceRequest, UsbEndpointDescriptor, UsbHubDescriptor,
    UsbInterfaceDescriptor, UsbPortStatus, UsbStringDescriptor, UsbWire, ue_get_addr, ue_get_size,
    ue_get_trans, ue_get_xfertype, ugetw, usetw,
};
use crate::dev::usb::usb::{usb_add_task, usb_rem_task, usb_schedsoftintr};
use crate::dev::usb::usb_mem::{dmaaddr, kernaddr, usb_allocmem, usb_freemem, usb_syncmem};
use crate::dev::usb::usb_subr::{usb_delay_ms, usbd_set_address};
use crate::dev::usb::usbdi::{
    IPL_SOFTUSB, USB_TASK_TYPE_ABORT, USBD_CANCELLED, USBD_DEFAULT_INTERVAL, USBD_FORCE_SHORT_XFER,
    USBD_IN_PROGRESS, USBD_INVAL, USBD_IOERROR, USBD_NOMEM, USBD_NORMAL_COMPLETION,
    USBD_NOT_STARTED, USBD_STALLED, USBD_TIMEOUT, UsbdStatus, splhardusb, splusb, usb_init_task,
    usb_insert_transfer, usb_transfer_complete, usbd_str,
};
use crate::dev::usb::usbdivar::{
    URQ_REQUEST, USB_DMA_COHERENT, USBREV_2_0, UsbDma, UsbdBus, UsbdBusMethods, UsbdDevice,
    UsbdHcPipe, UsbdPipe, UsbdPipeMethods, UsbdXfer, usbd_bus_set_hc_types, usbd_xfer_isread,
};
use crate::kassert;
use crate::kern::kern_malloc::{free, mallocarray};
use crate::kern::kern_rwlock::{rw_enter_write, rw_exit_write, rw_init};
use crate::kern::kern_synch::{tsleep_nsec, wakeup};
use crate::kern::kern_timeout::{timeout_add_msec, timeout_add_sec, timeout_del, timeout_set};
use crate::kern::subr_autoconf::{config_activate_children, config_detach_children};
use crate::kern::subr_pool::{pool_get, pool_init, pool_put};
use crate::kern::subr_prf::{panic, printf};
use crate::machine::bus::{
    BUS_DMASYNC_POSTREAD, BUS_DMASYNC_POSTWRITE, BUS_DMASYNC_PREREAD, BUS_DMASYNC_PREWRITE,
};
use crate::machine::intr::{splsoftassert, splx};
use crate::sys::device::{
    CD_SKIPHIBERNATE, Cfdriver, DV_DULL, DVACT_POWERDOWN, DVACT_RESUME, DVACT_SUSPEND, Device,
};
use crate::sys::errno::Errno;
use crate::sys::malloc::{M_NOWAIT, M_USBHC, M_ZERO};
use crate::sys::param::PZERO;
use crate::sys::pool::{PR_NOWAIT, PR_ZERO, Pool};
use crate::sys::queue::{ListHead, TailqHead};
use crate::sys::systm::INFSLP;
use crate::sys::time::msec_to_nsec;

/// `EHCI_INTR_ENDPT`: the root hub's interrupt endpoint.
const EHCI_INTR_ENDPT: u8 = 1;

/// `offsetof(struct ehci_qh, qh_link)`.
const QH_LINK: usize = offset_of!(EhciQh, qh_link);
/// `offsetof(struct ehci_qh, qh_qtd.qtd_status)`.
const QH_QTD_STATUS: usize = offset_of!(EhciQh, qh_qtd) + offset_of!(EhciQtd, qtd_status);
/// `offsetof(struct ehci_qtd, qtd_status)`.
const QTD_STATUS: usize = offset_of!(EhciQtd, qtd_status);
/// `offsetof(struct ehci_itd, itd_next)`.
const ITD_NEXT: usize = offset_of!(EhciItd, itd_next);
/// `offsetof(struct ehci_itd, itd_ctl)`.
const ITD_CTL: usize = offset_of!(EhciItd, itd_ctl);
/// `offsetof(struct ehci_sitd, sitd_trans)`.
const SITD_TRANS: usize = offset_of!(EhciSitd, sitd_trans);

/// `struct ehci_pipe`: a pipe of this controller.
#[repr(C)]
pub struct EhciPipe {
    /// `pipe`: the generic pipe, first.
    pub pipe: UsbdPipe,
    /// `sqh`: the pipe's queue head (none for an isochronous pipe).
    pub sqh: Cell<Option<&'static EhciSoftQh>>,
    /// `u.ctl.reqdma`: a control pipe's SETUP packet.
    pub reqdma: UsbDma,
    /// `u.isoc.next_frame`.
    pub next_frame: Cell<u32>,
    /// `u.isoc.cur_xfers`.
    pub cur_xfers: Cell<u32>,
}

impl EhciPipe {
    /// `epipe->sqh`. Panics on a pipe without one (the C's NULL dereference).
    fn sqh(&self) -> &'static EhciSoftQh {
        match self.sqh.get() {
            Some(q) => q,
            None => panic(format_args!("ehci: pipe without a queue head")),
        }
    }
}

// SAFETY: `#[repr(C)]` with the `usbd_pipe` first; the other members are `Cell`s of an
// `Option` of a reference and of integers, and a `usb_dma` of such `Cell`s: all valid as zero
// bits (`usbd_setup_pipe`'s `M_ZERO`).
unsafe impl UsbdHcPipe for EhciPipe {}

/// `ehci_cd`.
pub static EHCI_CD: Cfdriver = Cfdriver::new(b"ehci", DV_DULL, CD_SKIPHIBERNATE);

/// `ehcixfer`: the pool of [`EhciXfer`]s.
static EHCIXFER: Pool = Pool::new();
/// `ehcixfer != NULL`: the pool is initialised.
static EHCIXFER_INIT: AtomicBool = AtomicBool::new(false);

/// `ehci_bus_methods`.
pub static EHCI_BUS_METHODS: UsbdBusMethods = UsbdBusMethods {
    open_pipe: ehci_open,
    dev_setaddr: Some(ehci_setaddr),
    soft_intr: ehci_softintr,
    do_poll: ehci_poll,
    allocx: ehci_allocx,
    freex: ehci_freex,
};

/// `ehci_root_ctrl_methods`.
static EHCI_ROOT_CTRL_METHODS: UsbdPipeMethods = UsbdPipeMethods {
    transfer: ehci_root_ctrl_transfer,
    start: ehci_root_ctrl_start,
    abort: ehci_root_ctrl_abort,
    close: ehci_root_ctrl_close,
    cleartoggle: None,
    done: ehci_root_ctrl_done,
};

/// `ehci_root_intr_methods`.
static EHCI_ROOT_INTR_METHODS: UsbdPipeMethods = UsbdPipeMethods {
    transfer: ehci_root_intr_transfer,
    start: ehci_root_intr_start,
    abort: ehci_root_intr_abort,
    close: ehci_root_intr_close,
    cleartoggle: None,
    done: ehci_root_intr_done,
};

/// `ehci_device_ctrl_methods`.
static EHCI_DEVICE_CTRL_METHODS: UsbdPipeMethods = UsbdPipeMethods {
    transfer: ehci_device_ctrl_transfer,
    start: ehci_device_ctrl_start,
    abort: ehci_device_ctrl_abort,
    close: ehci_device_ctrl_close,
    cleartoggle: None,
    done: ehci_device_ctrl_done,
};

/// `ehci_device_intr_methods`.
static EHCI_DEVICE_INTR_METHODS: UsbdPipeMethods = UsbdPipeMethods {
    transfer: ehci_device_intr_transfer,
    start: ehci_device_intr_start,
    abort: ehci_device_intr_abort,
    close: ehci_device_intr_close,
    cleartoggle: Some(ehci_device_clear_toggle),
    done: ehci_device_intr_done,
};

/// `ehci_device_bulk_methods`.
static EHCI_DEVICE_BULK_METHODS: UsbdPipeMethods = UsbdPipeMethods {
    transfer: ehci_device_bulk_transfer,
    start: ehci_device_bulk_start,
    abort: ehci_device_bulk_abort,
    close: ehci_device_bulk_close,
    cleartoggle: Some(ehci_device_clear_toggle),
    done: ehci_device_bulk_done,
};

/// `ehci_device_isoc_methods`.
static EHCI_DEVICE_ISOC_METHODS: UsbdPipeMethods = UsbdPipeMethods {
    transfer: ehci_device_isoc_transfer,
    start: ehci_device_isoc_start,
    abort: ehci_device_isoc_abort,
    close: ehci_device_isoc_close,
    cleartoggle: None,
    done: ehci_device_isoc_done,
};

/*
 * Data structures and routines to emulate the root hub.
 */
/// `ehci_devd`.
static EHCI_DEVD: UsbDeviceDescriptor = UsbDeviceDescriptor {
    bLength: USB_DEVICE_DESCRIPTOR_SIZE as u8,
    bDescriptorType: UDESC_DEVICE,
    bcdUSB: [0x00, 0x02], // USB version
    bDeviceClass: UDCLASS_HUB,
    bDeviceSubClass: UDSUBCLASS_HUB,
    bDeviceProtocol: UDPROTO_HSHUBSTT,
    bMaxPacketSize: 64,
    idVendor: [0, 0],
    idProduct: [0, 0],
    bcdDevice: [0x00, 0x01],
    iManufacturer: 1,
    iProduct: 2,
    iSerialNumber: 0,
    bNumConfigurations: 1,
};

/// `ehci_odevd`.
static EHCI_ODEVD: UsbDeviceQualifier = UsbDeviceQualifier {
    bLength: USB_DEVICE_DESCRIPTOR_SIZE as u8,
    bDescriptorType: UDESC_DEVICE_QUALIFIER,
    bcdUSB: [0x00, 0x02], // USB version
    bDeviceClass: UDCLASS_HUB,
    bDeviceSubClass: UDSUBCLASS_HUB,
    bDeviceProtocol: UDPROTO_FSHUB,
    bMaxPacketSize0: 64,
    bNumConfigurations: 1,
    bReserved: 0,
};

/// `ehci_confd`.
static EHCI_CONFD: UsbConfigDescriptor = UsbConfigDescriptor {
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

/// `ehci_ifcd`.
static EHCI_IFCD: UsbInterfaceDescriptor = UsbInterfaceDescriptor {
    bLength: USB_INTERFACE_DESCRIPTOR_SIZE as u8,
    bDescriptorType: UDESC_INTERFACE,
    bInterfaceNumber: 0,
    bAlternateSetting: 0,
    bNumEndpoints: 1,
    bInterfaceClass: UICLASS_HUB,
    bInterfaceSubClass: UISUBCLASS_HUB,
    bInterfaceProtocol: UIPROTO_HSHUBSTT,
    iInterface: 0,
};

/// `ehci_endpd`.
static EHCI_ENDPD: UsbEndpointDescriptor = UsbEndpointDescriptor {
    bLength: USB_ENDPOINT_DESCRIPTOR_SIZE as u8,
    bDescriptorType: UDESC_ENDPOINT,
    bEndpointAddress: UE_DIR_IN | EHCI_INTR_ENDPT,
    bmAttributes: UE_INTERRUPT,
    wMaxPacketSize: [8, 0], // max packet
    bInterval: 12,
};

/// `ehci_hubd`.
static EHCI_HUBD: UsbHubDescriptor = UsbHubDescriptor {
    bDescLength: USB_HUB_DESCRIPTOR_SIZE as u8,
    bDescriptorType: UDESC_HUB,
    bNbrPorts: 0,
    wHubCharacteristics: [0, 0],
    bPwrOn2PwrGood: 0,
    bHubContrCurrent: 0,
    DeviceRemovable: [0; 32],
};

/// `(struct ehci_softc *)bus`: the softc whose `sc_bus` this is. Panics on a bus of another
/// controller.
pub fn ehci_softc(bus: &'static UsbdBus) -> &'static EhciSoftc {
    if !bus
        .methods
        .get()
        .is_some_and(|m| ptr::eq(m, &EHCI_BUS_METHODS))
    {
        panic(format_args!("{}: not an ehci bus", bus.bdev.xname()));
    }
    // SAFETY: only `ehci_init` installs `EHCI_BUS_METHODS`, on the `usbd_bus` that heads an
    // `EhciSoftc` (`#[repr(C)]`, bus first), which lives as long as the bus.
    unsafe { &*ptr::from_ref(bus).cast::<EhciSoftc>() }
}

/// `(struct ehci_pipe *)pipe`.
fn ehci_pipe(pipe: &'static UsbdPipe) -> &'static EhciPipe {
    pipe.hc::<EhciPipe>()
}

/// `(struct ehci_xfer *)xfer`.
fn ehci_xfer(xfer: &'static UsbdXfer) -> &'static EhciXfer {
    xfer.hc::<EhciXfer>()
}

/// `sc->sc_bus.bdev.dv_xname`.
fn devname(sc: &EhciSoftc) -> &str {
    sc.sc_bus.bdev.xname()
}

/// `SIMPLEQ_FIRST(&pipe->queue)` of a pipe the caller just queued on.
fn queue_first(pipe: &'static UsbdPipe) -> &'static UsbdXfer {
    match pipe.queue.first() {
        Some(x) => x,
        None => panic(format_args!("ehci: empty pipe queue")),
    }
}

/// `TAILQ_REMOVE(&sc->sc_intrhead, ex, inext)`, for an xfer on the list (see the module's
/// deviations).
fn ehci_intrhead_remove(sc: &'static EhciSoftc, ex: &'static EhciXfer) {
    if ex.inext.is_linked() {
        // SAFETY: `inext` is cleared after every removal below, so a linked xfer is on
        // `sc_intrhead`, the only list it joins; at splusb() under the kernel lock.
        unsafe {
            sc.sc_intrhead.remove(ex);
            ex.inext.clear_prev();
        }
    }
}

/// `TAILQ_INSERT_TAIL(&sc->sc_intrhead, ex, inext)`.
fn ehci_intrhead_insert(sc: &'static EhciSoftc, ex: &'static EhciXfer) {
    // SAFETY: the xfer is in no list (it is started once and removed when it completes or is
    // aborted); xfers are pool items that stay in place until freed, and a freed xfer is not
    // active; at splusb() under the kernel lock.
    unsafe { sc.sc_intrhead.insert_tail(ex) };
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
        panic(format_args!("ehci: frame {i} outside the xfer"));
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

/// `usb_syncmem(&sqh->dma, sqh->offs + off, len, ops)`.
fn sqh_sync(sqh: &EhciSoftQh, off: usize, len: usize, ops: i32) {
    usb_syncmem(&sqh.dma, sqh.offs.get() as usize + off, len, ops);
}

/// `usb_syncmem(&sqtd->dma, sqtd->offs + off, len, ops)`.
fn sqtd_sync(sqtd: &EhciSoftQtd, off: usize, len: usize, ops: i32) {
    usb_syncmem(&sqtd.dma, sqtd.offs.get() as usize + off, len, ops);
}

/// `usb_syncmem(&itd->dma, itd->offs + off, len, ops)`.
fn itd_sync(itd: &EhciSoftItd, off: usize, len: usize, ops: i32) {
    usb_syncmem(&itd.dma, itd.offs.get() as usize + off, len, ops);
}

/// `memset(&qtd, 0, sizeof(struct ehci_qtd))`.
fn qtd_clear(qtd: &EhciQtd) {
    ehci_set(&qtd.qtd_next, 0);
    ehci_set(&qtd.qtd_altnext, 0);
    ehci_set(&qtd.qtd_status, 0);
    for c in qtd.qtd_buffer.iter().chain(qtd.qtd_buffer_hi.iter()) {
        ehci_set(c, 0);
    }
}

/// `memset(&qh, 0, sizeof(struct ehci_qh))`.
fn qh_clear(qh: &EhciQh) {
    ehci_set(&qh.qh_link, 0);
    ehci_set(&qh.qh_endp, 0);
    ehci_set(&qh.qh_endphub, 0);
    ehci_set(&qh.qh_curqtd, 0);
    qtd_clear(&qh.qh_qtd);
}

/// `memset(&itd, 0, sizeof(struct ehci_itd))`.
fn itd_clear(itd: &EhciItd) {
    ehci_set(&itd.itd_next, 0);
    for c in itd
        .itd_ctl
        .iter()
        .chain(itd.itd_bufr.iter())
        .chain(itd.itd_bufr_hi.iter())
    {
        ehci_set(c, 0);
    }
}

/// `sqh->next`'s or `sqh->prev`'s target. Panics on NULL (the C's dereference).
fn qh_link_target(q: Option<&'static EhciSoftQh>) -> &'static EhciSoftQh {
    match q {
        Some(q) => q,
        None => panic(format_args!("ehci: unlinked queue head")),
    }
}

/// The software descriptor at byte `offs` of a fresh chunk, zeroed: `KERNADDR(&dma, offs)`.
///
/// # Safety
///
/// `dma` is a chunk `usb_allocmem` just returned, `EHCI_PAGE_SIZE`-aligned and at least
/// `offs + size_of::<T>()` bytes; `offs` is a multiple of `T`'s alignment; nobody else uses
/// the bytes; and every member of `T` is valid as zero bits. Chunks are never freed, so the
/// descriptor lives forever.
unsafe fn ehci_carve<T>(dma: &UsbDma, offs: usize) -> &'static T {
    let p = kernaddr(dma, offs).cast::<T>();
    // SAFETY: the caller's contract: the bytes are this chunk's, unused, aligned, and zero is
    // a valid `T`.
    unsafe {
        ptr::write_bytes(p, 0, 1);
        &*p
    }
}

/// `ehci_reverse_bits`: reverse a number with `nbits` bits. Used to evenly distribute
/// lower-level interrupt heads in the periodic schedule. Suitable for use with
/// `EHCI_IPOLLRATES <= 9`.
pub fn ehci_reverse_bits(c: u8, nbits: u32) -> u8 {
    let c = ((c >> 1) & 0x55) | ((c << 1) & 0xaa);
    let c = ((c >> 2) & 0x33) | ((c << 2) & 0xcc);
    let c = ((c >> 4) & 0x0f) | ((c << 4) & 0xf0);

    c >> (8 - nbits)
}

/// `ehci_init`: reads the capabilities, resets the controller, builds the periodic frame
/// list with its interrupt QH tree and the async list, and starts the controller.
pub fn ehci_init(sc: &'static EhciSoftc) -> UsbdStatus {
    sc.sc_offs
        .store(usize::from(eread1(sc, EHCI_CAPLENGTH)), Ordering::Relaxed);

    let sparams = eread4(sc, EHCI_HCSPARAMS);
    sc.sc_noport.set(ehci_hcs_n_ports(sparams) as i32);
    let cparams = eread4(sc, EHCI_HCCPARAMS);

    // MUST clear segment register if 64 bit capable.
    if ehci_hcc_64bit(cparams) != 0 {
        ewrite4(sc, EHCI_CTRLDSSEGMENT, 0);
    }

    sc.sc_bus.usbrev.set(USBREV_2_0);

    let err = ehci_reset(sc);
    if err.is_err() {
        return err;
    }

    if !EHCIXFER_INIT.load(Ordering::Relaxed) {
        pool_init(
            &EHCIXFER,
            size_of::<EhciXfer>(),
            0,
            IPL_SOFTUSB,
            0,
            "ehcixfer",
            None,
        );
        EHCIXFER_INIT.store(true, Ordering::Relaxed);
    }

    // frame list size at default, read back what we got and use that
    let flsize = match ehci_cmd_fls(eoread4(sc, EHCI_USBCMD)) {
        0 => 1024,
        1 => 512,
        2 => 256,
        _ => return USBD_IOERROR,
    };
    sc.sc_flsize.set(flsize);
    let flbytes = flsize as usize * size_of::<EhciLinkT>();
    let err = usb_allocmem(
        &sc.sc_bus,
        flbytes,
        EHCI_FLALIGN_ALIGN,
        USB_DMA_COHERENT,
        &sc.sc_fldma,
    );
    if err.is_err() {
        return err;
    }
    sc.sc_flist
        .set(kernaddr(&sc.sc_fldma, 0).cast::<EhciLinkT>());

    for i in 0..flsize as usize {
        sc.set_flist(i, EHCI_LINK_TERMINATE);
    }

    eowrite4(sc, EHCI_PERIODICLISTBASE, dmaaddr(&sc.sc_fldma, 0) as u32);

    let softitds_bytes = flsize as usize * size_of::<Cell<Option<&'static EhciSoftItd>>>();
    let Some(softitds) = mallocarray(
        flsize as usize,
        size_of::<Cell<Option<&'static EhciSoftItd>>>(),
        M_USBHC,
        M_NOWAIT | M_ZERO,
    ) else {
        usb_freemem(&sc.sc_bus, &sc.sc_fldma);
        return USBD_NOMEM;
    };
    sc.sc_softitds.set(softitds.as_ptr().cast());
    sc.sc_freeitds.init();
    sc.sc_intrhead.init();

    // Set up the bus struct.
    sc.sc_bus.methods.set(Some(&EHCI_BUS_METHODS));
    // SAFETY: `ehci_allocx`, this bus's `allocx`, returns only xfers that head an `EhciXfer`.
    unsafe { usbd_bus_set_hc_types::<EhciPipe, EhciXfer>(&sc.sc_bus) };

    sc.sc_eintrs.store(EHCI_NORMAL_INTRS, Ordering::Relaxed);

    let bad1 = |err: UsbdStatus| -> UsbdStatus {
        if let Some(p) = NonNull::new(sc.sc_softitds.get()) {
            free(p.cast(), M_USBHC, softitds_bytes);
        }
        sc.sc_softitds.set(ptr::null_mut());
        usb_freemem(&sc.sc_bus, &sc.sc_fldma);
        err
    };

    // Allocate the interrupt dummy QHs. These are arranged to give poll intervals that are
    // powers of 2 times 1ms.
    for i in 0..EHCI_INTRQHS {
        let Some(sqh) = ehci_alloc_sqh(sc) else {
            return bad1(USBD_NOMEM);
        };
        sc.sc_islots[i].sqh.set(Some(sqh));
    }
    for i in 0..EHCI_INTRQHS {
        let sqh = sc.sc_islots[i].sqh();
        if i == 0 {
            // The last (1ms) QH terminates.
            ehci_set(&sqh.qh.qh_link, EHCI_LINK_TERMINATE);
            sqh.next.set(None);
        } else {
            // Otherwise the next QH has half the poll interval
            let next = sc.sc_islots[i.div_ceil(2) - 1].sqh();
            sqh.next.set(Some(next));
            ehci_set(&sqh.qh.qh_link, next.physaddr.get() | EHCI_LINK_QH);
        }
        ehci_set(&sqh.qh.qh_endp, ehci_qh_set_eps(EHCI_QH_SPEED_HIGH));
        ehci_set(&sqh.qh.qh_endphub, ehci_qh_set_mult(1));
        ehci_set(&sqh.qh.qh_curqtd, EHCI_LINK_TERMINATE);
        ehci_set(&sqh.qh.qh_qtd.qtd_next, EHCI_LINK_TERMINATE);
        ehci_set(&sqh.qh.qh_qtd.qtd_altnext, EHCI_LINK_TERMINATE);
        ehci_set(&sqh.qh.qh_qtd.qtd_status, EHCI_QTD_HALTED);
        sqh.sqtd.set(None);
        sqh_sync(
            sqh,
            0,
            size_of::<EhciQh>(),
            BUS_DMASYNC_PREWRITE | BUS_DMASYNC_PREREAD,
        );
    }
    // Point the frame list at the last level (128ms).
    let last = EHCI_IPOLLRATES - 1;
    for i in 0..(1usize << last) {
        let mut j = i;
        while j < flsize as usize {
            let slot = ehci_iqhidx(last, u32::from(ehci_reverse_bits(i as u8, last)));
            sc.set_flist(j, EHCI_LINK_QH | sc.sc_islots[slot].sqh().physaddr.get());
            j += 1 << last;
        }
    }
    usb_syncmem(&sc.sc_fldma, 0, flbytes, BUS_DMASYNC_PREWRITE);

    // Allocate dummy QH that starts the async list.
    let Some(sqh) = ehci_alloc_sqh(sc) else {
        return bad1(USBD_NOMEM);
    };
    // Fill the QH
    ehci_set(
        &sqh.qh.qh_endp,
        ehci_qh_set_eps(EHCI_QH_SPEED_HIGH) | EHCI_QH_HRECL,
    );
    ehci_set(&sqh.qh.qh_link, sqh.physaddr.get() | EHCI_LINK_QH);
    ehci_set(&sqh.qh.qh_curqtd, EHCI_LINK_TERMINATE);
    sqh.prev.set(Some(sqh)); // It's a circular list..
    sqh.next.set(Some(sqh));
    // Fill the overlay qTD
    ehci_set(&sqh.qh.qh_qtd.qtd_next, EHCI_LINK_TERMINATE);
    ehci_set(&sqh.qh.qh_qtd.qtd_altnext, EHCI_LINK_TERMINATE);
    ehci_set(&sqh.qh.qh_qtd.qtd_status, EHCI_QTD_HALTED);
    sqh.sqtd.set(None);
    sqh_sync(
        sqh,
        0,
        size_of::<EhciQh>(),
        BUS_DMASYNC_PREWRITE | BUS_DMASYNC_PREREAD,
    );

    // Point to async list
    sc.sc_async_head.set(Some(sqh));
    eowrite4(sc, EHCI_ASYNCLISTADDR, sqh.physaddr.get() | EHCI_LINK_QH);

    timeout_set(
        &sc.sc_tmo_intrlist,
        ehci_intrlist_timeout,
        ptr::from_ref(sc).cast_mut().cast(),
    );

    rw_init(&sc.sc_doorbell_lock, "ehcidb");

    // Turn on controller
    eowrite4(
        sc,
        EHCI_USBCMD,
        EHCI_CMD_ITC_2 // 2 microframes interrupt delay
            | (eoread4(sc, EHCI_USBCMD) & EHCI_CMD_FLS_M)
            | EHCI_CMD_ASE
            | EHCI_CMD_PSE
            | EHCI_CMD_RS,
    );

    // Take over port ownership
    eowrite4(sc, EHCI_CONFIGFLAG, EHCI_CONF_CF);

    let mut hcr = 0;
    for _ in 0..100 {
        usb_delay_ms(&sc.sc_bus, 1);
        hcr = eoread4(sc, EHCI_USBSTS) & EHCI_STS_HCH;
        if hcr == 0 {
            break;
        }
    }
    if hcr != 0 {
        printf(format_args!("{}: run timeout\n", devname(sc)));
        return USBD_IOERROR;
    }

    // Enable interrupts
    eowrite4(sc, EHCI_USBINTR, sc.sc_eintrs.load(Ordering::Relaxed));

    USBD_NORMAL_COMPLETION
}

/// `ehci_intr`: the hard interrupt handler; `v` is the `EhciSoftc`. Established
/// `IPL_MPSAFE`: it runs without the kernel lock and touches only atomics and registers
/// before waking the doorbell's sleeper or scheduling the soft interrupt.
pub fn ehci_intr(v: *mut c_void) -> i32 {
    if v.is_null() {
        return 0;
    }
    // SAFETY: the front-end establishes the interrupt with its softc's `EhciSoftc` as the
    // argument and disestablishes it before the softc goes away.
    let sc: &'static EhciSoftc = unsafe { &*v.cast::<EhciSoftc>() };

    if sc.sc_bus.dying.get() != 0 {
        return 0;
    }

    // If we get an interrupt while polling, then just ignore it.
    if sc.sc_bus.use_polling.get() != 0 {
        let intrs = ehci_sts_intrs(eoread4(sc, EHCI_USBSTS));

        if intrs != 0 {
            eowrite4(sc, EHCI_USBSTS, intrs); // Acknowledge
        }
        return 0;
    }

    ehci_intr1(sc)
}

/// `ehci_intr1`: acknowledges the controller's interrupts and dispatches them.
pub fn ehci_intr1(sc: &'static EhciSoftc) -> i32 {
    let intrs = eoread4(sc, EHCI_USBSTS);
    if intrs == 0xffffffff {
        sc.sc_bus.dying.set(1);
        return 0;
    }
    let intrs = ehci_sts_intrs(intrs);
    if intrs == 0 {
        return 0;
    }

    let mut eintrs = intrs & sc.sc_eintrs.load(Ordering::Relaxed);
    if eintrs == 0 {
        return 0;
    }

    eowrite4(sc, EHCI_USBSTS, intrs); // Acknowledge
    sc.sc_bus
        .no_intrs
        .set(sc.sc_bus.no_intrs.get().wrapping_add(1));

    if eintrs & EHCI_STS_HSE != 0 {
        printf(format_args!(
            "{}: unrecoverable error, controller halted\n",
            devname(sc)
        ));
        sc.sc_bus.dying.set(1);
        return 1;
    }
    if eintrs & EHCI_STS_IAA != 0 {
        wakeup(ptr::from_ref(&sc.sc_async_head));
        eintrs &= !EHCI_STS_IAA;
    }
    if eintrs & (EHCI_STS_INT | EHCI_STS_ERRINT) != 0 {
        usb_schedsoftintr(&sc.sc_bus);
        eintrs &= !(EHCI_STS_INT | EHCI_STS_ERRINT);
    }
    if eintrs & EHCI_STS_PCD != 0 {
        sc.sc_flags.fetch_or(EHCIF_PCB_INTR, Ordering::Relaxed);
        usb_schedsoftintr(&sc.sc_bus);
        eintrs &= !EHCI_STS_PCD;
    }

    if eintrs != 0 {
        // Block unprocessed interrupts.
        let left = sc.sc_eintrs.fetch_and(!eintrs, Ordering::Relaxed) & !eintrs;
        eowrite4(sc, EHCI_USBINTR, left);
        printf(format_args!(
            "{}: blocking intrs 0x{:x}\n",
            devname(sc),
            eintrs
        ));
    }

    1
}

/// `ehci_pcd`: completes the root hub's interrupt transfer with the ports whose status
/// changed.
pub fn ehci_pcd(sc: &'static EhciSoftc, xfer: Option<&'static UsbdXfer>) {
    let Some(xfer) = xfer else {
        // Just ignore the change.
        return;
    };

    let length = xfer.length.get() as usize;
    // SAFETY: the root hub's interrupt xfer is in the driver's hands until completed here;
    // its DMA buffer has `length` bytes.
    let p = unsafe { xfer_buf(xfer, length) };
    let m = i64::from(sc.sc_noport.get()).min(length as i64 * 8 - 1);
    p.fill(0);
    let mut i = 1;
    while i <= m {
        // Pick out CHANGE bits from the status reg.
        if eoread4(sc, ehci_portsc(i as usize)) & EHCI_PS_CLEAR != 0
            && let Some(b) = p.get_mut(i as usize / 8)
        {
            *b |= 1 << (i % 8);
        }
        i += 1;
    }
    xfer.actlen.set(xfer.length.get());
    xfer.status.set(USBD_NORMAL_COMPLETION);

    usb_transfer_complete(xfer);
}

/// `ehci_setaddr`: the bus's `dev_setaddr`. Works around the half configured control
/// (default) pipe when setting the address of a device.
///
/// Because a single QH is setup per endpoint in `ehci_open()`, and the control pipe is
/// configured before we could have set the address of the device or read the
/// `wMaxPacketSize` of the endpoint, we have to re-open the pipe twice here.
pub fn ehci_setaddr(dev: &'static UsbdDevice, addr: i32) -> Result<(), Errno> {
    // Root Hub
    if dev.depth.get() == 0 {
        return Ok(());
    }

    let pipe = default_pipe(dev);

    // Re-establish the default pipe with the new max packet size.
    ehci_close_pipe(pipe);
    if ehci_open(pipe).is_err() {
        return Err(Errno::EINVAL);
    }

    if usbd_set_address(dev, addr).is_err() {
        return Err(Errno::EPERM);
    }

    dev.address.set(addr as u8);

    // Re-establish the default pipe with the new address.
    ehci_close_pipe(pipe);
    if ehci_open(pipe).is_err() {
        return Err(Errno::EINVAL);
    }

    Ok(())
}

/// `dev->default_pipe`. Panics on a device without one (the C's NULL dereference).
fn default_pipe(dev: &'static UsbdDevice) -> &'static UsbdPipe {
    match dev.default_pipe.get() {
        Some(p) => p,
        None => panic(format_args!("ehci: device without a default pipe")),
    }
}

/// `ehci_softintr`: the soft interrupt; `v` is the bus.
pub fn ehci_softintr(v: *mut c_void) {
    // SAFETY: `usb_attach` establishes the soft interrupt (and `usb_schedsoftintr` calls it)
    // with the bus as argument; the bus lives in the softc for the controller's life.
    let bus: &'static UsbdBus = unsafe { &*v.cast::<UsbdBus>() };
    let sc = ehci_softc(bus);

    if sc.sc_bus.dying.get() != 0 {
        return;
    }

    sc.sc_bus.intr_context.set(sc.sc_bus.intr_context.get() + 1);

    if sc.sc_flags.load(Ordering::Relaxed) & EHCIF_PCB_INTR != 0 {
        sc.sc_flags.fetch_and(!EHCIF_PCB_INTR, Ordering::Relaxed);
        ehci_pcd(sc, sc.sc_intrxfer.get());
    }

    // The only explanation I can think of for why EHCI is as brain dead as UHCI
    // interrupt-wise is that Intel was involved in both. An interrupt just tells us that
    // something is done, we have no clue what, so we need to scan through all active
    // transfers. :-(
    let mut ex = sc.sc_intrhead.first();
    while let Some(e) = ex {
        let next = TailqHead::<EhciXferIntrList>::next(e);
        ehci_check_intr(sc, &e.xfer);
        ex = next;
    }

    // Schedule a callout to catch any dropped transactions.
    if sc.sc_flags.load(Ordering::Relaxed) & EHCIF_DROPPED_INTR_WORKAROUND != 0
        && !sc.sc_intrhead.is_empty()
    {
        timeout_add_sec(&sc.sc_tmo_intrlist, 1);
    }

    if sc.sc_softwake.get() != 0 {
        sc.sc_softwake.set(0);
        wakeup(ptr::from_ref(&sc.sc_softwake));
    }

    sc.sc_bus.intr_context.set(sc.sc_bus.intr_context.get() - 1);
}

/// `ehci_check_intr`: checks whether an active xfer is done.
pub fn ehci_check_intr(sc: &'static EhciSoftc, xfer: &'static UsbdXfer) {
    let attr = xfer.pipe().endpoint().edesc().bmAttributes;

    if ue_get_xfertype(attr) == UE_ISOCHRONOUS {
        ehci_check_itd_intr(sc, xfer);
    } else {
        ehci_check_qh_intr(sc, xfer);
    }
}

/// `ex->sqtdstart`, `ex->sqtdend` of a started xfer.
fn sqtd_bounds(ex: &EhciXfer) -> (&'static EhciSoftQtd, &'static EhciSoftQtd) {
    match (ex.sqtdstart.get(), ex.sqtdend.get()) {
        (Some(s), Some(e)) => (s, e),
        _ => panic(format_args!(
            "kernel diagnostic assertion \"ex->sqtdstart != NULL && ex->sqtdend != NULL\" failed"
        )),
    }
}

/// `ex->itdstart`, `ex->itdend` of a started isochronous xfer.
fn itd_bounds(ex: &EhciXfer) -> (&'static EhciSoftItd, &'static EhciSoftItd) {
    match (ex.itdstart.get(), ex.itdend.get()) {
        (Some(s), Some(e)) => (s, e),
        _ => panic(format_args!(
            "kernel diagnostic assertion \"ex->itdstart != NULL && ex->itdend != NULL\" failed"
        )),
    }
}

/// `ehci_check_qh_intr`: a control, bulk or interrupt xfer is done when its last qTD is
/// inactive, or an earlier one halted or came back short.
pub fn ehci_check_qh_intr(sc: &'static EhciSoftc, xfer: &'static UsbdXfer) {
    let ex = ehci_xfer(xfer);
    let (start, lsqtd) = sqtd_bounds(ex);
    let st = size_of::<u32>();

    sqtd_sync(
        lsqtd,
        QTD_STATUS,
        st,
        BUS_DMASYNC_POSTWRITE | BUS_DMASYNC_POSTREAD,
    );

    // If the last TD is still active we need to check whether there is a an error somewhere
    // in the middle, or whether there was a short packet (SPD and not ACTIVE).
    if ehci_get(&lsqtd.qtd.qtd_status) & EHCI_QTD_ACTIVE != 0 {
        let mut done = false;
        let mut sqtd = start;
        while !ptr::eq(sqtd, lsqtd) {
            sqtd_sync(
                sqtd,
                QTD_STATUS,
                st,
                BUS_DMASYNC_POSTWRITE | BUS_DMASYNC_POSTREAD,
            );
            let status = ehci_get(&sqtd.qtd.qtd_status);
            sqtd_sync(sqtd, QTD_STATUS, st, BUS_DMASYNC_PREREAD);
            // If there's an active QTD the xfer isn't done.
            if status & EHCI_QTD_ACTIVE != 0 {
                break;
            }
            // Any kind of error makes the xfer done.
            if status & EHCI_QTD_HALTED != 0 {
                done = true;
                break;
            }
            // We want short packets, and it is short: it's done
            if ehci_qtd_get_bytes(status) != 0 {
                done = true;
                break;
            }
            let Some(next) = sqtd.nextqtd.get() else {
                break;
            };
            sqtd = next;
        }
        if !done {
            sqtd_sync(lsqtd, QTD_STATUS, st, BUS_DMASYNC_PREREAD);
            return;
        }
    }
    // done:
    ehci_intrhead_remove(sc, ex);
    timeout_del(&xfer.timeout_handle);
    usb_rem_task(xfer.pipe().device(), &xfer.abort_task);
    ehci_idone(xfer);
}

/// `ehci_check_itd_intr`: an isochronous xfer at the head of its pipe is done when its last
/// descriptor is inactive.
pub fn ehci_check_itd_intr(sc: &'static EhciSoftc, xfer: &'static UsbdXfer) {
    let ex = ehci_xfer(xfer);

    if !xfer.pipe().queue.first().is_some_and(|f| ptr::eq(f, xfer)) {
        return;
    }

    let (_, itd) = itd_bounds(ex);

    // Check no active transfers in last itd, meaning we're finished
    if xfer.device().speed.get() == USB_SPEED_HIGH {
        itd_sync(
            itd,
            ITD_CTL,
            size_of::<[u32; 8]>(),
            BUS_DMASYNC_POSTWRITE | BUS_DMASYNC_POSTREAD,
        );

        for c in &itd.itd.itd_ctl {
            if ehci_get(c) & EHCI_ITD_ACTIVE != 0 {
                return;
            }
        }
    } else {
        itd_sync(
            itd,
            SITD_TRANS,
            size_of::<u32>(),
            BUS_DMASYNC_POSTWRITE | BUS_DMASYNC_POSTREAD,
        );

        if ehci_get(&itd.sitd().sitd_trans) & EHCI_SITD_ACTIVE != 0 {
            return;
        }
    }

    // All descriptor(s) inactive, it's done
    ehci_intrhead_remove(sc, ex);
    timeout_del(&xfer.timeout_handle);
    usb_rem_task(xfer.pipe().device(), &xfer.abort_task);
    ehci_isoc_idone(xfer);
}

/// `ehci_isoc_idone`: collects the frame lengths of a finished isochronous xfer and
/// completes it.
pub fn ehci_isoc_idone(xfer: &'static UsbdXfer) {
    let ex = ehci_xfer(xfer);
    let mut nframes: i32 = 0;
    let mut actlen: u32 = 0;

    if xfer.status.get() == USBD_CANCELLED || xfer.status.get() == USBD_TIMEOUT {
        return;
    }

    if xfer.device().speed.get() == USB_SPEED_HIGH {
        let uframes = match xfer.pipe().endpoint().edesc().bInterval {
            0 => panic(format_args!("isoc xfer suddenly has 0 bInterval, invalid")),
            1 => 1,
            2 => 2,
            3 => 4,
            _ => 8,
        };

        let mut itd = ex.itdstart.get();
        while let Some(t) = itd {
            itd_sync(
                t,
                ITD_CTL,
                size_of::<[u32; 8]>(),
                BUS_DMASYNC_POSTWRITE | BUS_DMASYNC_POSTREAD,
            );

            let mut i = 0;
            while i < 8 {
                // XXX - driver didn't fill in the frame full of uframes. This leads to
                // scheduling inefficiencies, but working around this doubles complexity of
                // tracking an xfer.
                if nframes >= xfer.nframes.get() {
                    break;
                }

                let status = ehci_get(&t.itd.itd_ctl[i]);
                let mut len = ehci_itd_get_len(status);
                if ehci_itd_get_status(status) != 0 {
                    len = 0; // No valid data on error
                }

                set_frlength(xfer, nframes, len as u16);
                nframes += 1;
                actlen += len;
                i += uframes;
            }
            itd = t.xfer_next.get();
        }
    } else {
        let mut itd = ex.itdstart.get();
        while let Some(t) = itd {
            itd_sync(
                t,
                SITD_TRANS,
                size_of::<u32>(),
                BUS_DMASYNC_POSTWRITE | BUS_DMASYNC_POSTREAD,
            );

            let status = ehci_get(&t.sitd().sitd_trans);
            let mut len = ehci_sitd_get_len(status);
            let fl = u32::from(frlength(xfer, nframes));
            if fl >= len {
                len = fl - len;
            } else {
                len = 0;
            }

            set_frlength(xfer, nframes, len as u16);
            nframes += 1;
            actlen += len;
            itd = t.xfer_next.get();
        }
    }

    #[cfg(feature = "diagnostic")]
    ex.isdone.set(1);
    xfer.actlen.set(actlen);
    xfer.status.set(USBD_NORMAL_COMPLETION);

    usb_syncmem(
        &xfer.dmabuf,
        0,
        xfer.length.get() as usize,
        if usbd_xfer_isread(xfer) {
            BUS_DMASYNC_POSTREAD
        } else {
            BUS_DMASYNC_POSTWRITE
        },
    );
    usb_transfer_complete(xfer);
}

/// `ehci_idone`: sums the lengths of a finished control, bulk or interrupt xfer's qTDs,
/// derives its status and completes it.
pub fn ehci_idone(xfer: &'static UsbdXfer) {
    let ex = ehci_xfer(xfer);
    let mut status: u32 = 0;

    #[cfg(feature = "diagnostic")]
    {
        let s = crate::machine::intr::splhigh();
        if ex.isdone.get() != 0 {
            splx(s);
            printf(format_args!("ehci_idone: ex={:p} is done!\n", ex));
            return;
        }
        ex.isdone.set(1);
        splx(s);
    }
    if xfer.status.get() == USBD_CANCELLED || xfer.status.get() == USBD_TIMEOUT {
        return;
    }

    let mut actlen: u32 = 0;
    let mut sqtd = ex.sqtdstart.get();
    while let Some(q) = sqtd {
        sqtd_sync(
            q,
            0,
            size_of::<EhciQtd>(),
            BUS_DMASYNC_POSTWRITE | BUS_DMASYNC_POSTREAD,
        );
        let nstatus = ehci_get(&q.qtd.qtd_status);
        if nstatus & EHCI_QTD_ACTIVE != 0 {
            break;
        }

        status = nstatus;
        // halt is ok if descriptor is last, and complete
        if ehci_get(&q.qtd.qtd_next) == EHCI_LINK_TERMINATE && ehci_qtd_get_bytes(status) == 0 {
            status &= !EHCI_QTD_HALTED;
        }
        if ehci_qtd_get_pid(status) != EHCI_QTD_PID_SETUP {
            actlen = actlen
                .wrapping_add(u32::from(q.len.get()))
                .wrapping_sub(ehci_qtd_get_bytes(status));
        }
        sqtd = q.nextqtd.get();
    }

    let cerr = ehci_qtd_get_cerr(status);
    xfer.actlen.set(actlen);
    if status & EHCI_QTD_HALTED != 0 {
        if status & EHCI_QTD_BABBLE == 0 && cerr > 0 {
            xfer.status.set(USBD_STALLED);
        } else {
            xfer.status.set(USBD_IOERROR); // more info XXX
        }
    } else {
        xfer.status.set(USBD_NORMAL_COMPLETION);
    }

    if xfer.actlen.get() != 0 {
        usb_syncmem(
            &xfer.dmabuf,
            0,
            xfer.actlen.get() as usize,
            if usbd_xfer_isread(xfer) {
                BUS_DMASYNC_POSTREAD
            } else {
                BUS_DMASYNC_POSTWRITE
            },
        );
    }
    usb_transfer_complete(xfer);
}

/// `ehci_poll`: the bus's `do_poll`.
pub fn ehci_poll(bus: &'static UsbdBus) {
    let sc = ehci_softc(bus);

    if eoread4(sc, EHCI_USBSTS) & sc.sc_eintrs.load(Ordering::Relaxed) != 0 {
        ehci_intr1(sc);
    }
}

/// `ehci_detach`: detaches `usb(4)`, resets the controller and frees the frame list.
pub fn ehci_detach(self_: &Device, flags: i32) -> Result<(), Errno> {
    // SAFETY: the front-ends' softcs start with the `EhciSoftc` (`#[repr(C)]`), made for
    // their attachment.
    let sc = unsafe { self_.softc::<EhciSoftc>() };

    config_detach_children(self_, flags)?;

    timeout_del(&sc.sc_tmo_intrlist);

    let _ = ehci_reset(sc);

    usb_delay_ms(&sc.sc_bus, 300); // XXX let stray task complete

    if let Some(p) = NonNull::new(sc.sc_softitds.get()) {
        free(
            p.cast(),
            M_USBHC,
            sc.sc_flsize.get() as usize * size_of::<Cell<Option<&'static EhciSoftItd>>>(),
        );
    }
    sc.sc_softitds.set(ptr::null_mut());
    usb_freemem(&sc.sc_bus, &sc.sc_fldma);
    // XXX free other data structures XXX

    Ok(())
}

/// `ehci_activate`: suspend stops the schedules and resets the controller; resume brings the
/// lists and the ports back; power-down resets the controller after the children.
pub fn ehci_activate(self_: &Device, act: i32) -> Result<(), Errno> {
    // SAFETY: as in `ehci_detach`.
    let sc = unsafe { self_.softc::<EhciSoftc>() };
    let noport = sc.sc_noport.get().max(0) as usize;

    match act {
        DVACT_SUSPEND => {
            let rv = config_activate_children(self_, act);

            #[cfg(feature = "diagnostic")]
            if !sc.sc_intrhead.is_empty() {
                printf(format_args!("{}: interrupt list not empty\n", devname(sc)));
                return Err(Errno::ERESTART);
            }

            sc.sc_bus.use_polling.set(sc.sc_bus.use_polling.get() + 1);

            for i in 1..=noport {
                let cmd = eoread4(sc, ehci_portsc(i));
                if cmd & (EHCI_PS_PO | EHCI_PS_PE) == EHCI_PS_PE {
                    eowrite4(sc, ehci_portsc(i), cmd | EHCI_PS_SUSP);
                }
            }

            // First tell the host to stop processing Asynchronous and Periodic schedules.
            let cmd = eoread4(sc, EHCI_USBCMD) & !(EHCI_CMD_ASE | EHCI_CMD_PSE);
            eowrite4(sc, EHCI_USBCMD, cmd);
            let mut hcr = 0;
            for _ in 0..100 {
                usb_delay_ms(&sc.sc_bus, 1);
                hcr = eoread4(sc, EHCI_USBSTS) & (EHCI_STS_ASS | EHCI_STS_PSS);
                if hcr == 0 {
                    break;
                }
            }
            if hcr != 0 {
                printf(format_args!("{}: disable schedules timeout\n", devname(sc)));
            }

            // Then reset the host as if it was a shutdown.
            //
            // All USB devices are disconnected/reconnected during a suspend/resume cycle so
            // keep it simple.
            let _ = ehci_reset(sc);

            sc.sc_bus.use_polling.set(sc.sc_bus.use_polling.get() - 1);
            rv
        }
        DVACT_RESUME => {
            sc.sc_bus.use_polling.set(sc.sc_bus.use_polling.get() + 1);

            let _ = ehci_reset(sc);

            let cparams = eread4(sc, EHCI_HCCPARAMS);
            // MUST clear segment register if 64 bit capable.
            if ehci_hcc_64bit(cparams) != 0 {
                ewrite4(sc, EHCI_CTRLDSSEGMENT, 0);
            }

            eowrite4(sc, EHCI_PERIODICLISTBASE, dmaaddr(&sc.sc_fldma, 0) as u32);
            eowrite4(
                sc,
                EHCI_ASYNCLISTADDR,
                sc.async_head().physaddr.get() | EHCI_LINK_QH,
            );

            let mut hcr = 0;
            for i in 1..=noport {
                let cmd = eoread4(sc, ehci_portsc(i));
                if cmd & (EHCI_PS_PO | EHCI_PS_SUSP) == EHCI_PS_SUSP {
                    eowrite4(sc, ehci_portsc(i), cmd | EHCI_PS_FPR);
                    hcr = 1;
                }
            }

            if hcr != 0 {
                usb_delay_ms(&sc.sc_bus, USB_RESUME_WAIT);
                for i in 1..=noport {
                    let cmd = eoread4(sc, ehci_portsc(i));
                    if cmd & (EHCI_PS_PO | EHCI_PS_SUSP) == EHCI_PS_SUSP {
                        eowrite4(sc, ehci_portsc(i), cmd & !EHCI_PS_FPR);
                    }
                }
            }

            // Turn on controller
            eowrite4(
                sc,
                EHCI_USBCMD,
                EHCI_CMD_ITC_2 // 2 microframes interrupt delay
                    | (eoread4(sc, EHCI_USBCMD) & EHCI_CMD_FLS_M)
                    | EHCI_CMD_ASE
                    | EHCI_CMD_PSE
                    | EHCI_CMD_RS,
            );

            // Take over port ownership
            eowrite4(sc, EHCI_CONFIGFLAG, EHCI_CONF_CF);
            for _ in 0..100 {
                usb_delay_ms(&sc.sc_bus, 1);
                hcr = eoread4(sc, EHCI_USBSTS) & EHCI_STS_HCH;
                if hcr == 0 {
                    break;
                }
            }

            if hcr != 0 {
                printf(format_args!("{}: run timeout\n", devname(sc)));
                // XXX should we bail here?
            }

            eowrite4(sc, EHCI_USBINTR, sc.sc_eintrs.load(Ordering::Relaxed));

            usb_delay_ms(&sc.sc_bus, USB_RESUME_WAIT);

            sc.sc_bus.use_polling.set(sc.sc_bus.use_polling.get() - 1);
            config_activate_children(self_, act)
        }
        DVACT_POWERDOWN => {
            let rv = config_activate_children(self_, act);
            let _ = ehci_reset(sc);
            rv
        }
        _ => config_activate_children(self_, act),
    }
}

/// `ehci_reset`: halts the controller and resets it, keeping `EHCI_USBMODE` on the
/// controllers that need it.
pub fn ehci_reset(sc: &EhciSoftc) -> UsbdStatus {
    eowrite4(sc, EHCI_USBCMD, 0); // Halt controller
    let mut hcr = 0;
    for _ in 0..100 {
        usb_delay_ms(&sc.sc_bus, 1);
        hcr = eoread4(sc, EHCI_USBSTS) & EHCI_STS_HCH;
        if hcr != 0 {
            break;
        }
    }

    if hcr == 0 {
        printf(format_args!("{}: halt timeout\n", devname(sc)));
    }

    let usbmode = if sc.sc_flags.load(Ordering::Relaxed) & EHCIF_USBMODE != 0 {
        eoread4(sc, EHCI_USBMODE)
    } else {
        0
    };

    eowrite4(sc, EHCI_USBCMD, EHCI_CMD_HCRESET);
    for _ in 0..100 {
        usb_delay_ms(&sc.sc_bus, 1);
        hcr = eoread4(sc, EHCI_USBCMD) & EHCI_CMD_HCRESET;
        if hcr == 0 {
            break;
        }
    }

    if hcr != 0 {
        printf(format_args!("{}: reset timeout\n", devname(sc)));
        return USBD_IOERROR;
    }

    if sc.sc_flags.load(Ordering::Relaxed) & EHCIF_USBMODE != 0 {
        eowrite4(sc, EHCI_USBMODE, usbmode);
    }

    USBD_NORMAL_COMPLETION
}

/// `ehci_allocx`: the bus's `allocx`, a zeroed [`EhciXfer`].
pub fn ehci_allocx(_bus: &'static UsbdBus) -> Option<&'static UsbdXfer> {
    let p = pool_get(&EHCIXFER, PR_NOWAIT | PR_ZERO)?;
    // SAFETY: the pool's items are `size_of::<EhciXfer>()` bytes at the pool's alignment (at
    // least the type's, see the check at the end of the file), zeroed (`PR_ZERO`), which is a
    // valid `EhciXfer`; it lives until `ehci_freex`.
    let ex: &'static EhciXfer = unsafe { &*p.as_ptr().cast::<EhciXfer>() };
    #[cfg(feature = "diagnostic")]
    ex.isdone.set(1);
    Some(&ex.xfer)
}

/// `ehci_freex`: the bus's `freex`.
///
/// # Safety
///
/// `xfer` came from [`ehci_allocx`] and nothing uses it any more.
pub unsafe fn ehci_freex(_bus: &'static UsbdBus, xfer: NonNull<UsbdXfer>) {
    #[cfg(feature = "diagnostic")]
    {
        // SAFETY: the caller's contract: an `EhciXfer` from `ehci_allocx`, still allocated.
        let ex = unsafe { xfer.cast::<EhciXfer>().as_ref() };
        if ex.isdone.get() == 0 {
            printf(format_args!("ehci_freex: !isdone\n"));
            return;
        }
    }
    // The xfer heads its `EhciXfer` (`#[repr(C)]`), the pool item.
    pool_put(&EHCIXFER, xfer.cast());
}

/// `ehci_device_clear_toggle`: the pipe's `cleartoggle`.
pub fn ehci_device_clear_toggle(pipe: &'static UsbdPipe) {
    let epipe = ehci_pipe(pipe);
    let sqh = epipe.sqh();

    #[cfg(feature = "diagnostic")]
    if ehci_get(&sqh.qh.qh_qtd.qtd_status) & EHCI_QTD_ACTIVE != 0 {
        printf(format_args!("ehci_device_clear_toggle: queue active\n"));
    }
    let st = &sqh.qh.qh_qtd.qtd_status;
    ehci_set(st, ehci_get(st) & !EHCI_QTD_TOGGLE_MASK);
}

/// `ehci_open`: the bus's `open_pipe`. A control, bulk or interrupt pipe gets a QH in the
/// async list or the interrupt tree; an isochronous pipe gets none.
pub fn ehci_open(pipe: &'static UsbdPipe) -> UsbdStatus {
    let dev = pipe.device();
    let sc = ehci_softc(dev.bus());
    let ed = pipe.endpoint().edesc();
    let addr = dev.address.get();
    let xfertype = ue_get_xfertype(ed.bmAttributes);
    let epipe = ehci_pipe(pipe);

    if sc.sc_bus.dying.get() != 0 {
        return USBD_IOERROR;
    }

    let (hshubaddr, hshubport) = match dev.myhsport.get() {
        Some(port) => {
            let parent = match port.parent.get() {
                Some(p) => p,
                None => panic(format_args!("ehci_open: high speed port without a hub")),
            };
            (
                u32::from(parent.address.get()),
                u32::from(port.portno.get()),
            )
        }
        None => (0, 0),
    };

    // Root Hub
    if dev.depth.get() == 0 {
        match ed.bEndpointAddress {
            USB_CONTROL_ENDPOINT => pipe.methods.set(Some(&EHCI_ROOT_CTRL_METHODS)),
            a if a == UE_DIR_IN | EHCI_INTR_ENDPT => {
                pipe.methods.set(Some(&EHCI_ROOT_INTR_METHODS))
            }
            _ => return USBD_INVAL,
        }
        return USBD_NORMAL_COMPLETION;
    }

    // XXX All this stuff is only valid for async.
    let speed = match dev.speed.get() {
        USB_SPEED_LOW => EHCI_QH_SPEED_LOW,
        USB_SPEED_FULL => EHCI_QH_SPEED_FULL,
        USB_SPEED_HIGH => EHCI_QH_SPEED_HIGH,
        s => panic(format_args!("ehci_open: bad device speed {s}")),
    };

    // NAK reload count: must be zero with using periodic transfer. Linux 4.20's driver
    // (ehci-q.c) sets 4, we use same value.
    let naks = if xfertype == UE_CONTROL || xfertype == UE_BULK {
        4
    } else {
        0
    };

    // Allocate sqh for everything, save isoc xfers
    let mut sqh_opt = None;
    if xfertype != UE_ISOCHRONOUS {
        let Some(sqh) = ehci_alloc_sqh(sc) else {
            return USBD_NOMEM;
        };
        // qh_link filled when the QH is added
        ehci_set(
            &sqh.qh.qh_endp,
            ehci_qh_set_addr(u32::from(addr))
                | ehci_qh_set_endpt(u32::from(ue_get_addr(ed.bEndpointAddress)))
                | ehci_qh_set_eps(speed)
                | if xfertype == UE_CONTROL {
                    EHCI_QH_DTC
                } else {
                    0
                }
                | ehci_qh_set_mpl(u32::from(ugetw(ed.wMaxPacketSize)))
                | if speed != EHCI_QH_SPEED_HIGH && xfertype == UE_CONTROL {
                    EHCI_QH_CTL
                } else {
                    0
                }
                | ehci_qh_set_nrl(naks),
        );
        // To reduce conflict with split isochronous transfer, schedule (split) interrupt
        // transfer at latter half of 1ms frame:
        //
        //         |<-------------- H-Frame -------------->|
        //         .H0  :H1   H2   H3   H4   H5   H6   H7  .H0" :H1"
        //         .    :                                  .    :
        // [HS]    .    :          SS        CS   CS'  CS" .    :
        // [FS/LS] .    :               |<== >>>> >>>|     .    :
        //         .    :                                  .    :
        //         .B7' :B0   B1   B2   B3   B4   B5   B6  .B7  :B0"
        //              |<-------------- B-Frame -------------->|
        ehci_set(
            &sqh.qh.qh_endphub,
            ehci_qh_set_mult(1)
                | ehci_qh_set_smask(if xfertype == UE_INTERRUPT { 0x08 } else { 0 }),
        );
        if speed != EHCI_QH_SPEED_HIGH {
            let eh = &sqh.qh.qh_endphub;
            ehci_set(
                eh,
                ehci_get(eh)
                    | ehci_qh_set_huba(hshubaddr)
                    | ehci_qh_set_port(hshubport)
                    | ehci_qh_set_cmask(0xe0),
            );
        }
        ehci_set(&sqh.qh.qh_curqtd, EHCI_LINK_TERMINATE);
        // Fill the overlay qTD
        ehci_set(&sqh.qh.qh_qtd.qtd_next, EHCI_LINK_TERMINATE);
        ehci_set(&sqh.qh.qh_qtd.qtd_altnext, EHCI_LINK_TERMINATE);
        ehci_set(
            &sqh.qh.qh_qtd.qtd_status,
            ehci_qtd_set_toggle(pipe.endpoint().savedtoggle.get() as u32),
        );

        sqh_sync(
            sqh,
            0,
            size_of::<EhciQh>(),
            BUS_DMASYNC_PREWRITE | BUS_DMASYNC_PREREAD,
        );
        epipe.sqh.set(Some(sqh));
        sqh_opt = Some(sqh);
    } // xfertype == UE_ISOC

    match (xfertype, sqh_opt) {
        (UE_CONTROL, Some(sqh)) => {
            let err = usb_allocmem(
                &sc.sc_bus,
                size_of::<UsbDeviceRequest>(),
                0,
                USB_DMA_COHERENT,
                &epipe.reqdma,
            );
            if err.is_err() {
                ehci_free_sqh(sc, sqh);
                return err;
            }
            pipe.methods.set(Some(&EHCI_DEVICE_CTRL_METHODS));
            let s = splusb();
            ehci_add_qh(sqh, sc.async_head());
            splx(s);
        }
        (UE_BULK, Some(sqh)) => {
            pipe.methods.set(Some(&EHCI_DEVICE_BULK_METHODS));
            let s = splusb();
            ehci_add_qh(sqh, sc.async_head());
            splx(s);
        }
        (UE_INTERRUPT, Some(sqh)) => {
            pipe.methods.set(Some(&EHCI_DEVICE_INTR_METHODS));
            let mut ival = pipe.interval.get();
            if ival == USBD_DEFAULT_INTERVAL {
                ival = i32::from(ed.bInterval);
            }
            let s = splusb();
            let err = ehci_device_setintr(sc, sqh, ival);
            splx(s);
            return err;
        }
        (UE_ISOCHRONOUS, _) => {
            match speed {
                EHCI_QH_SPEED_HIGH | EHCI_QH_SPEED_FULL => {
                    pipe.methods.set(Some(&EHCI_DEVICE_ISOC_METHODS))
                }
                _ => return USBD_INVAL, // EHCI_QH_SPEED_LOW, default
            }
            // Spec page 271 says intervals > 16 are invalid
            if ed.bInterval == 0 || ed.bInterval > 16 {
                printf(format_args!("ehci: opening pipe with invalid bInterval\n"));
                return USBD_INVAL;
            }
            if ugetw(ed.wMaxPacketSize) == 0 {
                printf(format_args!("ehci: zero length endpoint open request\n"));
                return USBD_INVAL;
            }
            epipe.next_frame.set(0);
            epipe.cur_xfers.set(0);
        }
        _ => return USBD_INVAL,
    }
    USBD_NORMAL_COMPLETION
}

/// `ehci_add_qh`: adds a QH to the schedule after `head`. Called at `splusb()`. If in the
/// async schedule, it will always have a next. If in the intr schedule it may not.
pub fn ehci_add_qh(sqh: &'static EhciSoftQh, head: &'static EhciSoftQh) {
    splsoftassert(IPL_SOFTUSB, "ehci_add_qh");

    let l = size_of::<EhciLinkT>();
    sqh_sync(head, QH_LINK, l, BUS_DMASYNC_POSTWRITE);
    sqh.next.set(head.next.get());
    sqh.prev.set(Some(head));
    ehci_set(&sqh.qh.qh_link, ehci_get(&head.qh.qh_link));
    sqh_sync(sqh, QH_LINK, l, BUS_DMASYNC_PREWRITE);
    head.next.set(Some(sqh));
    if let Some(next) = sqh.next.get() {
        next.prev.set(Some(sqh));
    }
    ehci_set(&head.qh.qh_link, sqh.physaddr.get() | EHCI_LINK_QH);
    sqh_sync(head, QH_LINK, l, BUS_DMASYNC_PREWRITE);
}

/// `ehci_rem_qh`: removes a QH from the schedule and waits until the controller lets go of
/// it. Called at `splusb()`. Will always have a 'next' if it's in the async list as it's
/// circular.
pub fn ehci_rem_qh(sc: &'static EhciSoftc, sqh: &'static EhciSoftQh) {
    splsoftassert(IPL_SOFTUSB, "ehci_rem_qh");
    let l = size_of::<EhciLinkT>();
    // XXX
    sqh_sync(sqh, QH_LINK, l, BUS_DMASYNC_POSTWRITE);
    let prev = qh_link_target(sqh.prev.get());
    ehci_set(&prev.qh.qh_link, ehci_get(&sqh.qh.qh_link));
    prev.next.set(sqh.next.get());
    if let Some(next) = sqh.next.get() {
        next.prev.set(sqh.prev.get());
    }
    sqh_sync(prev, QH_LINK, l, BUS_DMASYNC_PREWRITE);

    ehci_sync_hc(sc);
}

/// `ehci_set_qh_qtd`: hangs a qTD chain on a QH, keeping the toggle and the ping state.
pub fn ehci_set_qh_qtd(sqh: &'static EhciSoftQh, sqtd: &'static EhciSoftQtd) {
    let st = size_of::<u32>();
    let all = size_of::<EhciQh>();
    let ov = &sqh.qh.qh_qtd;

    // Save toggle bit and ping status.
    sqh_sync(sqh, 0, all, BUS_DMASYNC_POSTWRITE | BUS_DMASYNC_POSTREAD);
    let status =
        ehci_get(&ov.qtd_status) & (EHCI_QTD_TOGGLE_MASK | ehci_qtd_set_status(EHCI_QTD_PINGSTATE));
    // Set HALTED to make hw leave it alone.
    ehci_set(&ov.qtd_status, ehci_qtd_set_status(EHCI_QTD_HALTED));
    sqh_sync(
        sqh,
        QH_QTD_STATUS,
        st,
        BUS_DMASYNC_PREWRITE | BUS_DMASYNC_PREREAD,
    );
    ehci_set(&sqh.qh.qh_curqtd, 0);
    ehci_set(&ov.qtd_next, sqtd.physaddr.get());
    ehci_set(&ov.qtd_altnext, EHCI_LINK_TERMINATE);
    for b in &ov.qtd_buffer {
        ehci_set(b, 0);
    }
    sqh.sqtd.set(Some(sqtd));
    sqh_sync(sqh, 0, all, BUS_DMASYNC_PREWRITE | BUS_DMASYNC_PREREAD);
    // Set !HALTED && !ACTIVE to start execution, preserve some fields
    ehci_set(&ov.qtd_status, status);
    sqh_sync(
        sqh,
        QH_QTD_STATUS,
        st,
        BUS_DMASYNC_PREWRITE | BUS_DMASYNC_PREREAD,
    );
}

/// `ehci_sync_hc`: ensures that the HC has released all references to the QH. We do this
/// by asking for a Async Advance Doorbell interrupt and then we wait for the interrupt. To
/// make this easier we first obtain exclusive use of the doorbell.
pub fn ehci_sync_hc(sc: &'static EhciSoftc) {
    if sc.sc_bus.dying.get() != 0 {
        return;
    }

    // get doorbell
    rw_enter_write(&sc.sc_doorbell_lock);
    let s = splhardusb();
    let mut tries = 0;
    let mut error;
    loop {
        eowrite4(sc, EHCI_USBCMD, eoread4(sc, EHCI_USBCMD) | EHCI_CMD_IAAD);
        error = tsleep_nsec(
            ptr::from_ref(&sc.sc_async_head),
            PZERO,
            "ehcidi",
            msec_to_nsec(500),
        );
        tries += 1;
        if error.is_ok() || tries >= 10 {
            break;
        }
    }
    splx(s);
    // release doorbell
    rw_exit_write(&sc.sc_doorbell_lock);
    #[cfg(feature = "diagnostic")]
    if let Err(e) = error {
        printf(format_args!("ehci_sync_hc: tsleep() = {}\n", e as i32));
    }
    #[cfg(not(feature = "diagnostic"))]
    let _ = error;
}

/// `ehci_rem_itd_chain`: unlinks an xfer's iTDs or siTDs from the frame list.
pub fn ehci_rem_itd_chain(sc: &'static EhciSoftc, ex: &'static EhciXfer) {
    splsoftassert(IPL_SOFTUSB, "ehci_rem_itd_chain");

    let _ = itd_bounds(ex);

    let mut itd = ex.itdstart.get();
    while let Some(t) = itd {
        let prev = t.frame_prev.get();
        // Unlink itd from hardware chain, or frame array
        match prev {
            None => {
                // We're at the table head
                let slot = t.slot.get() as usize;
                sc.set_softitd(slot, t.frame_next.get());
                sc.set_flist(slot, ehci_get(&t.itd.itd_next));
                usb_syncmem(
                    &sc.sc_fldma,
                    size_of::<u32>() * slot,
                    size_of::<u32>(),
                    BUS_DMASYNC_PREWRITE | BUS_DMASYNC_PREREAD,
                );

                if let Some(next) = t.frame_next.get() {
                    next.frame_prev.set(None);
                }
            }
            Some(prev) => {
                // XXX this part is untested...
                ehci_set(&prev.itd.itd_next, ehci_get(&t.itd.itd_next));
                itd_sync(t, ITD_NEXT, size_of::<u32>(), BUS_DMASYNC_PREWRITE);

                prev.frame_next.set(t.frame_next.get());
                if let Some(next) = t.frame_next.get() {
                    next.frame_prev.set(Some(prev));
                }
            }
        }
        itd = t.xfer_next.get();
    }
}

/// `ehci_free_itd_chain`: gives an xfer's iTDs or siTDs back to the free list.
pub fn ehci_free_itd_chain(sc: &'static EhciSoftc, ex: &'static EhciXfer) {
    splsoftassert(IPL_SOFTUSB, "ehci_free_itd_chain");

    let _ = itd_bounds(ex);

    let mut prev: Option<&'static EhciSoftItd> = None;
    let mut itd = ex.itdstart.get();
    while let Some(t) = itd {
        if let Some(p) = prev {
            ehci_free_itd(sc, p);
        }
        prev = Some(t);
        itd = t.xfer_next.get();
    }
    if let Some(p) = prev {
        ehci_free_itd(sc, p);
    }
    ex.itdstart.set(None);
    ex.itdend.set(None);
}

/// `ehci_root_ctrl_transfer`: simulates a hardware hub by handling all the necessary
/// requests.
pub fn ehci_root_ctrl_transfer(xfer: &'static UsbdXfer) -> UsbdStatus {
    // Insert last in queue.
    let err = usb_insert_transfer(xfer);
    if err.is_err() {
        return err;
    }

    // Pipe isn't running, start first
    ehci_root_ctrl_start(queue_first(xfer.pipe()))
}

/// `ehci_root_ctrl_start`: answers a request to the emulated root hub from the port
/// registers and completes it at once.
pub fn ehci_root_ctrl_start(xfer: &'static UsbdXfer) -> UsbdStatus {
    let sc = ehci_softc(xfer.device().bus());

    if sc.sc_bus.dying.get() != 0 {
        return USBD_IOERROR;
    }

    #[cfg(feature = "diagnostic")]
    if xfer.rqflags.get() & URQ_REQUEST == 0 {
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
                    let mut devd = EHCI_DEVD;
                    usetw(&mut devd.idVendor, sc.sc_id_vendor.get() as u16);
                    let l = len.min(USB_DEVICE_DESCRIPTOR_SIZE);
                    totlen = l;
                    buf[..l].copy_from_slice(&devd.as_bytes()[..l]);
                }
                UDESC_DEVICE_QUALIFIER => {
                    if value & 0xff != 0 {
                        break 'ret Err(USBD_IOERROR);
                    }
                    let l = len.min(USB_DEVICE_DESCRIPTOR_SIZE);
                    totlen = l;
                    let src = EHCI_ODEVD.as_bytes();
                    let n = l.min(src.len());
                    buf[..n].copy_from_slice(&src[..n]);
                    buf[n..l].fill(0);
                }
                // We can't really operate at another speed, but the spec says we need this
                // descriptor.
                UDESC_OTHER_SPEED_CONFIGURATION | UDESC_CONFIG => {
                    if value & 0xff != 0 {
                        break 'ret Err(USBD_IOERROR);
                    }
                    let mut confd = EHCI_CONFD;
                    confd.bDescriptorType = (value >> 8) as u8;
                    let parts: [&[u8]; 3] = [
                        &confd.as_bytes()[..USB_CONFIG_DESCRIPTOR_SIZE],
                        &EHCI_IFCD.as_bytes()[..USB_INTERFACE_DESCRIPTOR_SIZE],
                        &EHCI_ENDPD.as_bytes()[..USB_ENDPOINT_DESCRIPTOR_SIZE],
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
                        2 => Some(b"EHCI root hub"), // Product
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
                let port = ehci_portsc(usize::from(index));
                let v = eoread4(sc, port) & !EHCI_PS_CLEAR;
                match i32::from(value) {
                    UHF_PORT_ENABLE => eowrite4(sc, port, v & !EHCI_PS_PE),
                    UHF_PORT_SUSPEND => eowrite4(sc, port, v & !EHCI_PS_SUSP),
                    UHF_PORT_POWER => eowrite4(sc, port, v & !EHCI_PS_PP),
                    UHF_PORT_TEST => {}
                    UHF_PORT_INDICATOR => eowrite4(sc, port, v & !EHCI_PS_PIC),
                    UHF_C_PORT_CONNECTION => eowrite4(sc, port, v | EHCI_PS_CSC),
                    UHF_C_PORT_ENABLE => eowrite4(sc, port, v | EHCI_PS_PEC),
                    UHF_C_PORT_SUSPEND => {
                        // how?
                    }
                    UHF_C_PORT_OVER_CURRENT => eowrite4(sc, port, v | EHCI_PS_OCC),
                    UHF_C_PORT_RESET => sc.sc_isreset.set(0),
                    _ => break 'ret Err(USBD_IOERROR),
                }
            }
            (UR_GET_DESCRIPTOR, UT_READ_CLASS_DEVICE) => {
                if value & 0xff != 0 {
                    break 'ret Err(USBD_IOERROR);
                }
                let mut hubd = EHCI_HUBD;
                hubd.bNbrPorts = sc.sc_noport.get() as u8;
                let v = eread4(sc, EHCI_HCSPARAMS);
                usetw(
                    &mut hubd.wHubCharacteristics,
                    (if ehci_hcs_ppc(v) != 0 {
                        UHD_PWR_INDIVIDUAL
                    } else {
                        UHD_PWR_NO_SWITCH
                    }) | (if ehci_hcs_p_indicator(v) != 0 {
                        UHD_PORT_IND
                    } else {
                        0
                    }),
                );
                hubd.bPwrOn2PwrGood = 200; // XXX can't find out?
                // The C's loop advances `i` twice per eight ports (its body has an `i++` of
                // its own).
                let mut i = 0usize;
                let mut l = sc.sc_noport.get();
                while l > 0 {
                    if let Some(b) = hubd.DeviceRemovable.get_mut(i) {
                        *b = 0; // XXX can't find out?
                    }
                    i += 2;
                    l -= 8;
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
                let v = eoread4(sc, ehci_portsc(usize::from(index)));
                let mut i = UPS_HIGH_SPEED;
                if v & EHCI_PS_CS != 0 {
                    i |= UPS_CURRENT_CONNECT_STATUS;
                }
                if v & EHCI_PS_PE != 0 {
                    i |= UPS_PORT_ENABLED;
                }
                if v & EHCI_PS_SUSP != 0 {
                    i |= UPS_SUSPEND;
                }
                if v & EHCI_PS_OCA != 0 {
                    i |= UPS_OVERCURRENT_INDICATOR;
                }
                if v & EHCI_PS_PR != 0 {
                    i |= UPS_RESET;
                }
                if v & EHCI_PS_PP != 0 {
                    i |= UPS_PORT_POWER;
                }
                let mut ps = UsbPortStatus::zeroed();
                usetw(&mut ps.wPortStatus, i);
                let mut i = 0;
                if v & EHCI_PS_CSC != 0 {
                    i |= UPS_C_CONNECT_STATUS;
                }
                if v & EHCI_PS_PEC != 0 {
                    i |= UPS_C_PORT_ENABLED;
                }
                if v & EHCI_PS_OCC != 0 {
                    i |= UPS_C_OVERCURRENT_INDICATOR;
                }
                if sc.sc_isreset.get() != 0 {
                    i |= UPS_C_PORT_RESET;
                }
                usetw(&mut ps.wPortChange, i);
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
                let port = ehci_portsc(usize::from(index));
                let mut v = eoread4(sc, port) & !EHCI_PS_CLEAR;
                match i32::from(value) {
                    UHF_PORT_ENABLE => eowrite4(sc, port, v | EHCI_PS_PE),
                    UHF_PORT_SUSPEND => eowrite4(sc, port, v | EHCI_PS_SUSP),
                    UHF_PORT_DISOWN_TO_1_1 => {
                        // enter to Port Reset State
                        v &= !EHCI_PS_PE;
                        eowrite4(sc, port, v | EHCI_PS_PR);
                        ehci_disown(sc, i32::from(index), 0);
                    }
                    UHF_PORT_RESET => {
                        if ehci_ps_is_lowspeed(v) {
                            // Low speed device, give up ownership.
                            ehci_disown(sc, i32::from(index), 1);
                        } else {
                            // Start reset sequence.
                            v &= !(EHCI_PS_PE | EHCI_PS_PR);
                            eowrite4(sc, port, v | EHCI_PS_PR);
                            // Wait for reset to complete.
                            usb_delay_ms(&sc.sc_bus, USB_PORT_ROOT_RESET_DELAY);
                            if sc.sc_bus.dying.get() != 0 {
                                break 'ret Err(USBD_IOERROR);
                            }
                            // Terminate reset sequence.
                            let v = eoread4(sc, port);
                            eowrite4(sc, port, v & !EHCI_PS_PR);
                            // Wait for HC to complete reset.
                            usb_delay_ms(&sc.sc_bus, EHCI_PORT_RESET_COMPLETE);
                            if sc.sc_bus.dying.get() != 0 {
                                break 'ret Err(USBD_IOERROR);
                            }
                            let v = eoread4(sc, port);
                            if v & EHCI_PS_PR != 0 {
                                printf(format_args!("{}: port reset timeout\n", devname(sc)));
                                break 'ret Err(USBD_IOERROR);
                            }
                            if v & EHCI_PS_PE == 0 {
                                // Not a high speed device, give up ownership.
                                ehci_disown(sc, i32::from(index), 0);
                            } else {
                                sc.sc_isreset.set(1);
                            }
                        }
                    }
                    UHF_PORT_POWER => eowrite4(sc, port, v | EHCI_PS_PP),
                    UHF_PORT_TEST => {}
                    UHF_PORT_INDICATOR => eowrite4(sc, port, v | EHCI_PS_PIC),
                    _ => break 'ret Err(USBD_IOERROR),
                }
            }
            (UR_CLEAR_TT_BUFFER, UT_WRITE_CLASS_OTHER)
            | (UR_RESET_TT, UT_WRITE_CLASS_OTHER)
            | (UR_GET_TT_STATE, UT_READ_CLASS_OTHER)
            | (UR_STOP_TT, UT_WRITE_CLASS_OTHER) => {}
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

/// `ehci_disown`: gives a root port to the companion controller.
pub fn ehci_disown(sc: &'static EhciSoftc, index: i32, _lowspeed: i32) {
    let port = ehci_portsc(index as usize);
    let v = eoread4(sc, port) & !EHCI_PS_CLEAR;
    eowrite4(sc, port, v | EHCI_PS_PO);
}

/// `ehci_root_ctrl_abort`: abort a root control request. Nothing to do, all transfers are
/// synchronous.
pub fn ehci_root_ctrl_abort(_xfer: &'static UsbdXfer) {}

/// `ehci_root_ctrl_close`: close the root pipe. Nothing to do.
pub fn ehci_root_ctrl_close(_pipe: &'static UsbdPipe) {}

/// `ehci_root_intr_done`.
pub fn ehci_root_intr_done(_xfer: &'static UsbdXfer) {}

/// `ehci_root_intr_transfer`.
pub fn ehci_root_intr_transfer(xfer: &'static UsbdXfer) -> UsbdStatus {
    // Insert last in queue.
    let err = usb_insert_transfer(xfer);
    if err.is_err() {
        return err;
    }

    // Pipe isn't running, start first
    ehci_root_intr_start(queue_first(xfer.pipe()))
}

/// `ehci_root_intr_start`: the root hub's interrupt transfer waits for a port change.
pub fn ehci_root_intr_start(xfer: &'static UsbdXfer) -> UsbdStatus {
    let sc = ehci_softc(xfer.device().bus());

    if sc.sc_bus.dying.get() != 0 {
        return USBD_IOERROR;
    }

    sc.sc_intrxfer.set(Some(xfer));

    USBD_IN_PROGRESS
}

/// `ehci_root_intr_abort`.
pub fn ehci_root_intr_abort(xfer: &'static UsbdXfer) {
    let sc = ehci_softc(xfer.device().bus());

    sc.sc_intrxfer.set(None);

    xfer.status.set(USBD_CANCELLED);
    let s = splusb();
    usb_transfer_complete(xfer);
    splx(s);
}

/// `ehci_root_intr_close`.
pub fn ehci_root_intr_close(_pipe: &'static UsbdPipe) {}

/// `ehci_root_ctrl_done`.
pub fn ehci_root_ctrl_done(_xfer: &'static UsbdXfer) {}

/// `ehci_alloc_sqh`: a cleared QH from the free list, carving a new chunk when it is empty.
pub fn ehci_alloc_sqh(sc: &'static EhciSoftc) -> Option<&'static EhciSoftQh> {
    let s = splusb();
    if sc.sc_freeqhs.get().is_none() {
        let dma = UsbDma {
            block: Cell::new(None),
            offs: Cell::new(0),
        };
        let err = usb_allocmem(
            &sc.sc_bus,
            EHCI_SQH_SIZE * EHCI_SQH_CHUNK,
            EHCI_PAGE_SIZE as usize,
            USB_DMA_COHERENT,
            &dma,
        );
        if err.is_err() {
            splx(s);
            return None;
        }
        for i in 0..EHCI_SQH_CHUNK {
            let offs = i * EHCI_SQH_SIZE;
            // SAFETY: the chunk is `EHCI_SQH_SIZE * EHCI_SQH_CHUNK` fresh bytes, page aligned
            // (`usb_allocmem`'s alignment), `offs` a multiple of `EHCI_SQH_SIZE` (itself a
            // multiple of the structure's alignment); a soft QH is `Cell`s only, valid as
            // zero bits.
            let sqh: &'static EhciSoftQh = unsafe { ehci_carve(&dma, offs) };
            sqh.physaddr.set(dmaaddr(&dma, offs) as u32);
            sqh.dma.block.set(dma.block.get());
            sqh.dma.offs.set(dma.offs.get());
            sqh.offs.set(offs as i32);
            sqh.next.set(sc.sc_freeqhs.get());
            sc.sc_freeqhs.set(Some(sqh));
        }
    }
    let sqh = sc.sc_freeqhs.get()?;
    sc.sc_freeqhs.set(sqh.next.get());
    qh_clear(&sqh.qh);
    sqh.next.set(None);
    sqh.prev.set(None);

    splx(s);
    Some(sqh)
}

/// `ehci_free_sqh`.
pub fn ehci_free_sqh(sc: &'static EhciSoftc, sqh: &'static EhciSoftQh) {
    let s = splusb();
    sqh.next.set(sc.sc_freeqhs.get());
    sc.sc_freeqhs.set(Some(sqh));
    splx(s);
}

/// `ehci_alloc_sqtd`: a cleared qTD from the free list, carving a new chunk when it is
/// empty.
pub fn ehci_alloc_sqtd(sc: &'static EhciSoftc) -> Option<&'static EhciSoftQtd> {
    let s = splusb();
    if sc.sc_freeqtds.get().is_none() {
        let dma = UsbDma {
            block: Cell::new(None),
            offs: Cell::new(0),
        };
        let err = usb_allocmem(
            &sc.sc_bus,
            EHCI_SQTD_SIZE * EHCI_SQTD_CHUNK,
            EHCI_PAGE_SIZE as usize,
            USB_DMA_COHERENT,
            &dma,
        );
        if err.is_err() {
            splx(s);
            return None;
        }
        for i in 0..EHCI_SQTD_CHUNK {
            let offs = i * EHCI_SQTD_SIZE;
            // SAFETY: as in `ehci_alloc_sqh`, for soft qTDs.
            let sqtd: &'static EhciSoftQtd = unsafe { ehci_carve(&dma, offs) };
            sqtd.physaddr.set(dmaaddr(&dma, offs) as u32);
            sqtd.dma.block.set(dma.block.get());
            sqtd.dma.offs.set(dma.offs.get());
            sqtd.offs.set(offs as i32);
            sqtd.nextqtd.set(sc.sc_freeqtds.get());
            sc.sc_freeqtds.set(Some(sqtd));
        }
    }

    let sqtd = sc.sc_freeqtds.get()?;
    sc.sc_freeqtds.set(sqtd.nextqtd.get());
    qtd_clear(&sqtd.qtd);
    sqtd.nextqtd.set(None);

    splx(s);
    Some(sqtd)
}

/// `ehci_free_sqtd`.
pub fn ehci_free_sqtd(sc: &'static EhciSoftc, sqtd: &'static EhciSoftQtd) {
    let s = splusb();
    sqtd.nextqtd.set(sc.sc_freeqtds.get());
    sc.sc_freeqtds.set(Some(sqtd));
    splx(s);
}

/// `ehci_alloc_sqtd_chain`: the qTDs that move `alen` bytes of the xfer's buffer, each
/// covering at most five pages; returns the first and the last.
pub fn ehci_alloc_sqtd_chain(
    sc: &'static EhciSoftc,
    alen: u32,
    xfer: &'static UsbdXfer,
) -> Result<(&'static EhciSoftQtd, &'static EhciSoftQtd), UsbdStatus> {
    let rd = usbd_xfer_isread(xfer);
    let dma = &xfer.dmabuf;
    let ed = xfer.pipe().endpoint().edesc();
    let page = EHCI_PAGE_SIZE;
    let nbuf = EHCI_QTD_NBUFFERS as u32;

    let mut len = alen;
    let iscontrol = ue_get_xfertype(ed.bmAttributes) == UE_CONTROL;

    let mut dataphys = dmaaddr(dma, 0) as u32;
    let dataphyslastpage = ehci_page(dataphys.wrapping_add(len).wrapping_sub(1));
    let mut qtdstatus = EHCI_QTD_ACTIVE
        | ehci_qtd_set_pid(if rd {
            EHCI_QTD_PID_IN
        } else {
            EHCI_QTD_PID_OUT
        })
        | ehci_qtd_set_cerr(3); // IOC and BYTES set below
    let mps = u32::from(ugetw(ed.wMaxPacketSize));
    let mut forceshort =
        (xfer.flags.get() & USBD_FORCE_SHORT_XFER != 0 || len == 0) && len.is_multiple_of(mps);
    // The control transfer data stage always starts with a toggle of 1. For other transfers
    // we let the hardware track the toggle state.
    if iscontrol {
        qtdstatus |= ehci_qtd_set_toggle(1);
    }

    let Some(start) = ehci_alloc_sqtd(sc) else {
        return Err(USBD_NOMEM); // XXX free chain
    };
    let mut cur = start;

    usb_syncmem(
        dma,
        0,
        alen as usize,
        if rd {
            BUS_DMASYNC_PREREAD
        } else {
            BUS_DMASYNC_PREWRITE
        },
    );
    loop {
        let dataphyspage = ehci_page(dataphys);
        // The EHCI hardware can handle at most 5 pages.
        let mut curlen;
        if dataphyslastpage.wrapping_sub(dataphyspage) < nbuf * page {
            // we can handle it in this QTD
            curlen = len;
        } else {
            // must use multiple TDs, fill as much as possible.
            curlen = nbuf * page - ehci_page_offset(dataphys);

            if curlen > len {
                curlen = len;
            }

            // the length must be a multiple of the max size
            curlen -= curlen % mps;
        }

        len -= curlen;

        // Allocate another transfer if there's more data left, or if force last short
        // transfer flag is set and we're allocating a multiple of the max packet size.
        let (next, nextphys) = if len != 0 || forceshort {
            let Some(next) = ehci_alloc_sqtd(sc) else {
                return Err(USBD_NOMEM); // XXX free chain
            };
            (Some(next), next.physaddr.get())
        } else {
            (None, EHCI_LINK_TERMINATE)
        };

        // `curlen + EHCI_PAGE_OFFSET(dataphys)` is at most five pages (both branches above),
        // so `i` stays below EHCI_QTD_NBUFFERS; the C checks it under DIAGNOSTIC.
        let mut i: u32 = 0;
        while i * page < curlen + ehci_page_offset(dataphys) {
            let mut a = dataphys.wrapping_add(i * page);
            if i != 0 {
                // use offset only in first buffer
                a = ehci_page(a);
            }
            let (Some(b), Some(bhi)) = (
                cur.qtd.qtd_buffer.get(i as usize),
                cur.qtd.qtd_buffer_hi.get(i as usize),
            ) else {
                #[cfg(feature = "diagnostic")]
                printf(format_args!("ehci_alloc_sqtd_chain: i={i}\n"));
                return Err(USBD_NOMEM);
            };
            ehci_set(b, a);
            ehci_set(bhi, 0);
            i += 1;
        }
        cur.nextqtd.set(next);
        ehci_set(&cur.qtd.qtd_next, nextphys);
        ehci_set(&cur.qtd.qtd_altnext, nextphys);
        ehci_set(&cur.qtd.qtd_status, qtdstatus | ehci_qtd_set_bytes(curlen));
        cur.len.set(curlen as u16);
        if iscontrol {
            // adjust the toggle based on the number of packets in this qtd
            if (curlen.div_ceil(mps) & 1) != 0 || curlen == 0 {
                qtdstatus ^= EHCI_QTD_TOGGLE_MASK;
            }
        }
        if len == 0 {
            if !forceshort {
                break;
            }
            forceshort = false;
        }
        sqtd_sync(
            cur,
            0,
            size_of::<EhciQtd>(),
            BUS_DMASYNC_PREWRITE | BUS_DMASYNC_PREREAD,
        );
        dataphys = dataphys.wrapping_add(curlen);
        cur = match next {
            Some(n) => n,
            None => break, // unreachable: a next qTD was allocated above
        };
    }
    let st = &cur.qtd.qtd_status;
    ehci_set(st, ehci_get(st) | EHCI_QTD_IOC);
    sqtd_sync(
        cur,
        0,
        size_of::<EhciQtd>(),
        BUS_DMASYNC_PREWRITE | BUS_DMASYNC_PREREAD,
    );

    Ok((start, cur))
}

/// `ehci_free_sqtd_chain`: gives an xfer's qTDs back and unhooks them from its pipe's QH.
pub fn ehci_free_sqtd_chain(sc: &'static EhciSoftc, ex: &'static EhciXfer) {
    let epipe = ehci_pipe(ex.xfer.pipe());

    let mut sqtd = ex.sqtdstart.get();
    while let Some(q) = sqtd {
        let next = q.nextqtd.get();
        ehci_free_sqtd(sc, q);
        sqtd = next;
    }
    ex.sqtdstart.set(None);
    ex.sqtdend.set(None);
    epipe.sqh().sqtd.set(None);
}

/// `ehci_alloc_itd`: an iTD/siTD from the free list that was not freed in this frame or the
/// last one (the controller may still be reading it), carving a new chunk when none is.
pub fn ehci_alloc_itd(sc: &'static EhciSoftc) -> Option<&'static EhciSoftItd> {
    let s = splusb();

    // Find an itd that wasn't freed this frame or last frame. This can discard itds that
    // were freed before frindex wrapped around
    // XXX - can this lead to thrashing? Could fix by enabling wrap-around interrupt and
    //       fiddling with list when that happens
    let frindex = (eoread4(sc, EHCI_FRINDEX).wrapping_add(1) >> 3) as i32;
    let previndex = if frindex != 0 {
        frindex - 1
    } else {
        sc.sc_flsize.get() as i32
    };

    let mut freeitd = sc
        .sc_freeitds
        .iter()
        .find(|itd| itd.slot.get() != frindex && itd.slot.get() != previndex);

    if freeitd.is_none() {
        let dma = UsbDma {
            block: Cell::new(None),
            offs: Cell::new(0),
        };
        let err = usb_allocmem(
            &sc.sc_bus,
            EHCI_ITD_SIZE * EHCI_ITD_CHUNK,
            EHCI_PAGE_SIZE as usize,
            USB_DMA_COHERENT,
            &dma,
        );
        if err.is_err() {
            splx(s);
            return None;
        }

        for i in 0..EHCI_ITD_CHUNK {
            let offs = i * EHCI_ITD_SIZE;
            // SAFETY: as in `ehci_alloc_sqh`, for soft iTDs.
            let itd: &'static EhciSoftItd = unsafe { ehci_carve(&dma, offs) };
            itd.physaddr.set(dmaaddr(&dma, offs) as u32);
            itd.dma.block.set(dma.block.get());
            itd.dma.offs.set(dma.offs.get());
            itd.offs.set(offs as i32);
            // SAFETY: a fresh itd, on no list, that never moves; at splusb().
            unsafe { sc.sc_freeitds.insert_head(itd) };
        }
        freeitd = sc.sc_freeitds.first();
    }

    let Some(itd) = freeitd else {
        splx(s);
        return None;
    };
    // SAFETY: `itd` was found on `sc_freeitds` above; at splusb().
    unsafe { ListHead::<EhciSoftItdFreeList>::remove(itd) };
    itd_clear(&itd.itd);
    itd_sync(
        itd,
        ITD_NEXT,
        size_of::<u32>(),
        BUS_DMASYNC_PREWRITE | BUS_DMASYNC_PREREAD,
    );

    itd.frame_next.set(None);
    itd.frame_prev.set(None);
    itd.xfer_next.set(None);
    itd.slot.set(0);
    splx(s);

    Some(itd)
}

/// `ehci_free_itd`.
pub fn ehci_free_itd(sc: &'static EhciSoftc, itd: &'static EhciSoftItd) {
    let s = splusb();
    // SAFETY: an itd in use is on no list (`ehci_alloc_itd` removed it); itds never move; at
    // splusb().
    unsafe { sc.sc_freeitds.insert_head(itd) };
    splx(s);
}

/// `ehci_close_pipe`: close a regular pipe. Assumes that there are no pending transactions.
pub fn ehci_close_pipe(pipe: &'static UsbdPipe) {
    let epipe = ehci_pipe(pipe);
    let sc = ehci_softc(pipe.device().bus());
    let sqh = epipe.sqh();

    let s = splusb();
    ehci_rem_qh(sc, sqh);
    splx(s);
    pipe.endpoint()
        .savedtoggle
        .set(ehci_qtd_get_toggle(ehci_get(&sqh.qh.qh_qtd.qtd_status)) as i32);
    ehci_free_sqh(sc, sqh);
}

/// `ehci_abort_xfer`: abort a device request.
///
/// If this routine is called at `splusb()` it guarantees that the request will be removed
/// from the hardware scheduling and that the callback for it will be called with
/// `USBD_CANCELLED` status. It's impossible to guarantee that the requested transfer will not
/// have happened since the hardware runs concurrently. If the transaction has already
/// happened we rely on the ordinary interrupt processing to process it.
pub fn ehci_abort_xfer(xfer: &'static UsbdXfer, status: UsbdStatus) {
    let sc = ehci_softc(xfer.device().bus());
    let epipe = ehci_pipe(xfer.pipe());
    let ex = ehci_xfer(xfer);

    if sc.sc_bus.dying.get() != 0 || xfer.status.get() == USBD_NOT_STARTED {
        let s = splusb();
        if xfer.status.get() != USBD_NOT_STARTED {
            ehci_intrhead_remove(sc, ex);
        }
        xfer.status.set(status); // make software ignore it
        timeout_del(&xfer.timeout_handle);
        usb_rem_task(xfer.device(), &xfer.abort_task);
        #[cfg(feature = "diagnostic")]
        ex.isdone.set(1);
        usb_transfer_complete(xfer);
        splx(s);
        return;
    }

    if xfer.device().bus().intr_context.get() != 0 {
        panic(format_args!("ehci_abort_xfer: not in process context"));
    }

    // If an abort is already in progress then just wait for it to complete and return.
    if ex.ehci_xfer_flags.get() & EHCI_XFER_ABORTING != 0 {
        // No need to wait if we're aborting from a timeout.
        if status == USBD_TIMEOUT {
            return;
        }
        // Override the status which might be USBD_TIMEOUT.
        xfer.status.set(status);
        ex.ehci_xfer_flags
            .set(ex.ehci_xfer_flags.get() | EHCI_XFER_ABORTWAIT);
        while ex.ehci_xfer_flags.get() & EHCI_XFER_ABORTING != 0 {
            let _ = tsleep_nsec(ptr::from_ref(&ex.ehci_xfer_flags), PZERO, "ehciaw", INFSLP);
        }
        return;
    }

    let sqh = epipe.sqh();
    let st = size_of::<u32>();

    // Step 1: Make interrupt routine and timeouts ignore xfer.
    let s = splusb();
    ex.ehci_xfer_flags
        .set(ex.ehci_xfer_flags.get() | EHCI_XFER_ABORTING);
    xfer.status.set(status); // make software ignore it
    ehci_intrhead_remove(sc, ex);
    timeout_del(&xfer.timeout_handle);
    usb_rem_task(xfer.device(), &xfer.abort_task);
    splx(s);

    // Step 2: Deactivate all of the qTDs that we will be removing, otherwise the queue head
    // may go active again.
    sqh_sync(
        sqh,
        QH_QTD_STATUS,
        st,
        BUS_DMASYNC_POSTWRITE | BUS_DMASYNC_POSTREAD,
    );
    ehci_set(&sqh.qh.qh_qtd.qtd_status, EHCI_QTD_HALTED);
    sqh_sync(
        sqh,
        QH_QTD_STATUS,
        st,
        BUS_DMASYNC_PREWRITE | BUS_DMASYNC_PREREAD,
    );

    let mut sqtd = ex.sqtdstart.get();
    while let Some(q) = sqtd {
        sqtd_sync(
            q,
            QTD_STATUS,
            st,
            BUS_DMASYNC_POSTWRITE | BUS_DMASYNC_POSTREAD,
        );
        ehci_set(&q.qtd.qtd_status, EHCI_QTD_HALTED);
        sqtd_sync(
            q,
            QTD_STATUS,
            st,
            BUS_DMASYNC_PREWRITE | BUS_DMASYNC_PREREAD,
        );
        sqtd = q.nextqtd.get();
    }
    ehci_sync_hc(sc);

    // Step 3: Make sure the soft interrupt routine has run. This should remove any completed
    // items off the queue. The hardware has no reference to completed items (TDs). It's
    // safe to remove them at any time.
    let s = splusb();
    sc.sc_softwake.set(1);
    usb_schedsoftintr(&sc.sc_bus);
    let _ = tsleep_nsec(ptr::from_ref(&sc.sc_softwake), PZERO, "ehciab", INFSLP);

    #[cfg(feature = "diagnostic")]
    ex.isdone.set(1);
    // Do the wakeup first to avoid touching the xfer after the callback.
    ex.ehci_xfer_flags
        .set(ex.ehci_xfer_flags.get() & !EHCI_XFER_ABORTING);
    if ex.ehci_xfer_flags.get() & EHCI_XFER_ABORTWAIT != 0 {
        ex.ehci_xfer_flags
            .set(ex.ehci_xfer_flags.get() & !EHCI_XFER_ABORTWAIT);
        wakeup(ptr::from_ref(&ex.ehci_xfer_flags));
    }
    usb_transfer_complete(xfer);

    splx(s);
}

/// `ehci_abort_isoc_xfer`: deactivates an isochronous xfer's descriptors and completes it.
pub fn ehci_abort_isoc_xfer(xfer: &'static UsbdXfer, status: UsbdStatus) {
    let sc = ehci_softc(xfer.device().bus());
    let ex = ehci_xfer(xfer);

    splsoftassert(IPL_SOFTUSB, "ehci_abort_isoc_xfer");

    if sc.sc_bus.dying.get() != 0 || xfer.status.get() == USBD_NOT_STARTED {
        if xfer.status.get() != USBD_NOT_STARTED {
            ehci_intrhead_remove(sc, ex);
        }
        xfer.status.set(status);
        timeout_del(&xfer.timeout_handle);
        usb_rem_task(xfer.device(), &xfer.abort_task);
        usb_transfer_complete(xfer);
        return;
    }

    // Transfer is already done.
    if xfer.status.get() != USBD_IN_PROGRESS {
        return;
    }

    #[cfg(feature = "diagnostic")]
    ex.isdone.set(1);
    xfer.status.set(status);
    ehci_intrhead_remove(sc, ex);
    timeout_del(&xfer.timeout_handle);
    usb_rem_task(xfer.device(), &xfer.abort_task);

    if xfer.device().speed.get() == USB_SPEED_HIGH {
        let mut itd = ex.itdstart.get();
        while let Some(t) = itd {
            let ctl = size_of::<[u32; 8]>();
            itd_sync(
                t,
                ITD_CTL,
                ctl,
                BUS_DMASYNC_POSTWRITE | BUS_DMASYNC_POSTREAD,
            );

            for c in &t.itd.itd_ctl {
                ehci_set(c, ehci_get(c) & !EHCI_ITD_ACTIVE);
            }

            itd_sync(t, ITD_CTL, ctl, BUS_DMASYNC_PREWRITE | BUS_DMASYNC_PREREAD);
            itd = t.xfer_next.get();
        }
    } else {
        let mut itd = ex.itdstart.get();
        while let Some(t) = itd {
            let st = size_of::<u32>();
            itd_sync(
                t,
                SITD_TRANS,
                st,
                BUS_DMASYNC_POSTWRITE | BUS_DMASYNC_POSTREAD,
            );

            let tr = &t.sitd().sitd_trans;
            ehci_set(tr, ehci_get(tr) & !EHCI_SITD_ACTIVE);

            itd_sync(
                t,
                SITD_TRANS,
                st,
                BUS_DMASYNC_PREWRITE | BUS_DMASYNC_PREREAD,
            );
            itd = t.xfer_next.get();
        }
    }

    sc.sc_softwake.set(1);
    usb_schedsoftintr(&sc.sc_bus);
    let _ = tsleep_nsec(ptr::from_ref(&sc.sc_softwake), PZERO, "ehciab", INFSLP);

    usb_transfer_complete(xfer);
}

/// `ehci_timeout`: an xfer's timeout; the abort runs in the USB abort task thread.
pub fn ehci_timeout(addr: *mut c_void) {
    // SAFETY: the timeout's argument is its xfer (`ehci_arm_timeout`), which the stack keeps
    // until the xfer completes, and completion deletes the timeout.
    let xfer: &'static UsbdXfer = unsafe { &*addr.cast::<UsbdXfer>() };
    let sc = ehci_softc(xfer.device().bus());

    if sc.sc_bus.dying.get() != 0 {
        ehci_timeout_task(addr);
        return;
    }

    usb_init_task(
        &xfer.abort_task,
        ehci_timeout_task,
        addr,
        USB_TASK_TYPE_ABORT,
    );
    usb_add_task(xfer.device(), &xfer.abort_task);
}

/// `ehci_timeout_task`.
pub fn ehci_timeout_task(addr: *mut c_void) {
    // SAFETY: as in `ehci_timeout`; completion removes the task.
    let xfer: &'static UsbdXfer = unsafe { &*addr.cast::<UsbdXfer>() };

    let s = splusb();
    ehci_abort_xfer(xfer, USBD_TIMEOUT);
    splx(s);
}

/// `ehci_intrlist_timeout`: some EHCI chips from VIA / ATI seem to trigger interrupts before
/// writing back the qTD status, or miss signalling occasionally under heavy load. If the
/// host machine is too fast, we can miss transaction completion - when we scan the active
/// list the transaction still seems to be active. This generally exhibits itself as a umass
/// stall that never recovers.
///
/// We work around this behaviour by setting up this callback after any softintr that
/// completes with transactions still pending, giving us another chance to check for
/// completion after the writeback has taken place.
pub fn ehci_intrlist_timeout(arg: *mut c_void) {
    // SAFETY: `ehci_init` sets the timeout with the softc as argument; `ehci_detach` deletes
    // it before the softc goes away.
    let sc: &'static EhciSoftc = unsafe { &*arg.cast::<EhciSoftc>() };

    if sc.sc_bus.dying.get() != 0 {
        return;
    }

    let s = splusb();
    usb_schedsoftintr(&sc.sc_bus);
    splx(s);
}

/// Arms the xfer's timeout (`timeout_del`, `timeout_set`, `timeout_add_msec`), as the start
/// functions do.
fn ehci_arm_timeout(xfer: &'static UsbdXfer) {
    timeout_del(&xfer.timeout_handle);
    timeout_set(
        &xfer.timeout_handle,
        ehci_timeout,
        ptr::from_ref(xfer).cast_mut().cast(),
    );
    timeout_add_msec(&xfer.timeout_handle, u64::from(xfer.timeout.get()));
}

/// `ehci_device_ctrl_transfer`.
pub fn ehci_device_ctrl_transfer(xfer: &'static UsbdXfer) -> UsbdStatus {
    // Insert last in queue.
    let err = usb_insert_transfer(xfer);
    if err.is_err() {
        return err;
    }

    // Pipe isn't running, start first
    ehci_device_ctrl_start(queue_first(xfer.pipe()))
}

/// `ehci_device_ctrl_start`: a control transfer as a SETUP qTD, the data qTDs and a STATUS
/// qTD hung on the pipe's QH.
pub fn ehci_device_ctrl_start(xfer: &'static UsbdXfer) -> UsbdStatus {
    let sc = ehci_softc(xfer.device().bus());
    let epipe = ehci_pipe(xfer.pipe());
    let ex = ehci_xfer(xfer);
    let req = xfer.request.get();
    let len = u32::from(ugetw(req.wLength));

    kassert!(xfer.rqflags.get() & URQ_REQUEST != 0);

    if sc.sc_bus.dying.get() != 0 {
        return USBD_IOERROR;
    }

    let fail = |err: UsbdStatus| -> UsbdStatus {
        // bad1:
        xfer.status.set(err);
        usb_transfer_complete(xfer);
        err
    };

    let Some(setup) = ehci_alloc_sqtd(sc) else {
        return fail(USBD_NOMEM);
    };
    let Some(stat) = ehci_alloc_sqtd(sc) else {
        // bad2:
        ehci_free_sqtd(sc, setup);
        return fail(USBD_NOMEM);
    };

    let sqh = epipe.sqh();
    let all = size_of::<EhciQtd>();

    // Set up data transaction
    let next = if len != 0 {
        match ehci_alloc_sqtd_chain(sc, len, xfer) {
            Ok((next, end)) => {
                let st = &end.qtd.qtd_status;
                ehci_set(st, ehci_get(st) & !EHCI_QTD_IOC);
                end.nextqtd.set(Some(stat));
                ehci_set(&end.qtd.qtd_next, stat.physaddr.get());
                ehci_set(&end.qtd.qtd_altnext, stat.physaddr.get());
                sqtd_sync(end, 0, all, BUS_DMASYNC_PREWRITE | BUS_DMASYNC_PREREAD);
                next
            }
            Err(err) => {
                // bad3: bad2:
                ehci_free_sqtd(sc, stat);
                ehci_free_sqtd(sc, setup);
                return fail(err);
            }
        }
    } else {
        stat
    };

    let reqlen = size_of::<UsbDeviceRequest>();
    // SAFETY: `ehci_open` allocated `sizeof(usb_device_request_t)` bytes of DMA memory for
    // the pipe's SETUP packet, which only this pipe's started transfer writes.
    unsafe {
        ptr::copy_nonoverlapping(req.as_bytes().as_ptr(), kernaddr(&epipe.reqdma, 0), reqlen)
    };
    usb_syncmem(&epipe.reqdma, 0, reqlen, BUS_DMASYNC_PREWRITE);

    // Clear toggle
    ehci_set(
        &setup.qtd.qtd_status,
        EHCI_QTD_ACTIVE
            | ehci_qtd_set_pid(EHCI_QTD_PID_SETUP)
            | ehci_qtd_set_cerr(3)
            | ehci_qtd_set_toggle(0)
            | ehci_qtd_set_bytes(reqlen as u32),
    );
    ehci_set(&setup.qtd.qtd_buffer[0], dmaaddr(&epipe.reqdma, 0) as u32);
    ehci_set(&setup.qtd.qtd_buffer_hi[0], 0);
    setup.nextqtd.set(Some(next));
    ehci_set(&setup.qtd.qtd_next, next.physaddr.get());
    ehci_set(&setup.qtd.qtd_altnext, next.physaddr.get());
    setup.len.set(reqlen as u16);
    sqtd_sync(setup, 0, all, BUS_DMASYNC_PREWRITE | BUS_DMASYNC_PREREAD);

    ehci_set(
        &stat.qtd.qtd_status,
        EHCI_QTD_ACTIVE
            | ehci_qtd_set_pid(if usbd_xfer_isread(xfer) {
                EHCI_QTD_PID_OUT
            } else {
                EHCI_QTD_PID_IN
            })
            | ehci_qtd_set_cerr(3)
            | ehci_qtd_set_toggle(1)
            | EHCI_QTD_IOC,
    );
    ehci_set(&stat.qtd.qtd_buffer[0], 0); // XXX not needed?
    ehci_set(&stat.qtd.qtd_buffer_hi[0], 0); // XXX not needed?
    stat.nextqtd.set(None);
    ehci_set(&stat.qtd.qtd_next, EHCI_LINK_TERMINATE);
    ehci_set(&stat.qtd.qtd_altnext, EHCI_LINK_TERMINATE);
    stat.len.set(0);
    sqtd_sync(stat, 0, all, BUS_DMASYNC_PREWRITE | BUS_DMASYNC_PREREAD);

    ex.sqtdstart.set(Some(setup));
    ex.sqtdend.set(Some(stat));
    #[cfg(feature = "diagnostic")]
    {
        if ex.isdone.get() == 0 {
            printf(format_args!(
                "ehci_device_ctrl_start: not done, ex={:p}\n",
                ex
            ));
        }
        ex.isdone.set(0);
    }

    // Insert qTD in QH list.
    let s = splusb();
    ehci_set_qh_qtd(sqh, setup);
    if xfer.timeout.get() != 0 && sc.sc_bus.use_polling.get() == 0 {
        ehci_arm_timeout(xfer);
    }
    ehci_intrhead_insert(sc, ex);
    xfer.status.set(USBD_IN_PROGRESS);
    splx(s);

    USBD_IN_PROGRESS
}

/// `ehci_device_ctrl_done`.
pub fn ehci_device_ctrl_done(xfer: &'static UsbdXfer) {
    let sc = ehci_softc(xfer.device().bus());
    let ex = ehci_xfer(xfer);

    kassert!(xfer.rqflags.get() & URQ_REQUEST != 0);

    if xfer.status.get() != USBD_NOMEM {
        ehci_free_sqtd_chain(sc, ex);
    }
}

/// `ehci_device_ctrl_abort`.
pub fn ehci_device_ctrl_abort(xfer: &'static UsbdXfer) {
    ehci_abort_xfer(xfer, USBD_CANCELLED);
}

/// `ehci_device_ctrl_close`.
pub fn ehci_device_ctrl_close(pipe: &'static UsbdPipe) {
    ehci_close_pipe(pipe);
}

/// `ehci_device_bulk_transfer`.
pub fn ehci_device_bulk_transfer(xfer: &'static UsbdXfer) -> UsbdStatus {
    // Insert last in queue.
    let err = usb_insert_transfer(xfer);
    if err.is_err() {
        return err;
    }

    // Pipe isn't running, start first
    ehci_device_bulk_start(queue_first(xfer.pipe()))
}

/// Hangs a started bulk or interrupt xfer's qTDs on its pipe's QH and makes it active: the
/// common tail of `ehci_device_bulk_start`, `ehci_device_intr_start` and the repeat path of
/// `ehci_device_intr_done`.
fn ehci_device_data_start(
    sc: &'static EhciSoftc,
    xfer: &'static UsbdXfer,
    ex: &'static EhciXfer,
    sqh: &'static EhciSoftQh,
    data: &'static EhciSoftQtd,
    dataend: &'static EhciSoftQtd,
    _func: &str,
) {
    // Set up interrupt info.
    ex.sqtdstart.set(Some(data));
    ex.sqtdend.set(Some(dataend));
    #[cfg(feature = "diagnostic")]
    {
        if ex.isdone.get() == 0 {
            printf(format_args!("{_func}: not done, ex={:p}\n", ex));
        }
        ex.isdone.set(0);
    }

    let s = splusb();
    ehci_set_qh_qtd(sqh, data);
    if xfer.timeout.get() != 0 && sc.sc_bus.use_polling.get() == 0 {
        ehci_arm_timeout(xfer);
    }
    ehci_intrhead_insert(sc, ex);
    xfer.status.set(USBD_IN_PROGRESS);
    splx(s);
}

/// `ehci_device_bulk_start`.
pub fn ehci_device_bulk_start(xfer: &'static UsbdXfer) -> UsbdStatus {
    let sc = ehci_softc(xfer.device().bus());
    let epipe = ehci_pipe(xfer.pipe());
    let ex = ehci_xfer(xfer);

    kassert!(xfer.rqflags.get() & URQ_REQUEST == 0);

    if sc.sc_bus.dying.get() != 0 {
        return USBD_IOERROR;
    }

    let sqh = epipe.sqh();

    let (data, dataend) = match ehci_alloc_sqtd_chain(sc, xfer.length.get(), xfer) {
        Ok(c) => c,
        Err(err) => {
            xfer.status.set(err);
            usb_transfer_complete(xfer);
            return err;
        }
    };

    ehci_device_data_start(sc, xfer, ex, sqh, data, dataend, "ehci_device_bulk_start");

    USBD_IN_PROGRESS
}

/// `ehci_device_bulk_abort`.
pub fn ehci_device_bulk_abort(xfer: &'static UsbdXfer) {
    ehci_abort_xfer(xfer, USBD_CANCELLED);
}

/// `ehci_device_bulk_close`: close a device bulk pipe.
pub fn ehci_device_bulk_close(pipe: &'static UsbdPipe) {
    ehci_close_pipe(pipe);
}

/// `ehci_device_bulk_done`.
pub fn ehci_device_bulk_done(xfer: &'static UsbdXfer) {
    let sc = ehci_softc(xfer.device().bus());
    let ex = ehci_xfer(xfer);

    if xfer.status.get() != USBD_NOMEM {
        ehci_free_sqtd_chain(sc, ex);
    }
}

/// `ehci_device_setintr`: hangs an interrupt pipe's QH in the interrupt tree at the level of
/// its poll interval.
pub fn ehci_device_setintr(
    sc: &'static EhciSoftc,
    sqh: &'static EhciSoftQh,
    ival: i32,
) -> UsbdStatus {
    // Find a poll rate that is large enough.
    let mut lev = EHCI_IPOLLRATES - 1;
    while lev > 0 {
        if ehci_ilev_ival(lev) <= ival {
            break;
        }
        lev -= 1;
    }

    // Pick an interrupt slot at the right level.
    // XXX could do better than picking at random
    let islot = ehci_iqhidx(lev, arc4random());

    sqh.islot.set(islot as i32);
    let isp = &sc.sc_islots[islot];
    ehci_add_qh(sqh, isp.sqh());

    USBD_NORMAL_COMPLETION
}

/// `ehci_device_intr_transfer`.
pub fn ehci_device_intr_transfer(xfer: &'static UsbdXfer) -> UsbdStatus {
    // Insert last in queue.
    let err = usb_insert_transfer(xfer);
    if err.is_err() {
        return err;
    }

    // Pipe isn't running (otherwise err would be USBD_INPROG), so start it first.
    ehci_device_intr_start(queue_first(xfer.pipe()))
}

/// `ehci_device_intr_start`.
pub fn ehci_device_intr_start(xfer: &'static UsbdXfer) -> UsbdStatus {
    let sc = ehci_softc(xfer.device().bus());
    let ex = ehci_xfer(xfer);
    let epipe = ehci_pipe(xfer.pipe());

    kassert!(xfer.rqflags.get() & URQ_REQUEST == 0);

    if sc.sc_bus.dying.get() != 0 {
        return USBD_IOERROR;
    }

    let sqh = epipe.sqh();

    let (data, dataend) = match ehci_alloc_sqtd_chain(sc, xfer.length.get(), xfer) {
        Ok(c) => c,
        Err(err) => {
            xfer.status.set(err);
            usb_transfer_complete(xfer);
            return err;
        }
    };

    ehci_device_data_start(sc, xfer, ex, sqh, data, dataend, "ehci_device_intr_start");

    USBD_IN_PROGRESS
}

/// `ehci_device_intr_abort`.
pub fn ehci_device_intr_abort(xfer: &'static UsbdXfer) {
    let pipe = xfer.pipe();
    kassert!(pipe.repeat.get() == 0 || pipe.intrxfer.get().is_some_and(|x| ptr::eq(x, xfer)));

    // XXX - abort_xfer uses ehci_sync_hc, which syncs via the advance async doorbell. That's
    //       dependant on the async list, whereas intr xfers are periodic, should not use
    //       this?
    ehci_abort_xfer(xfer, USBD_CANCELLED);
}

/// `ehci_device_intr_close`.
pub fn ehci_device_intr_close(pipe: &'static UsbdPipe) {
    ehci_close_pipe(pipe);
}

/// `ehci_device_intr_done`: a repeating interrupt xfer is started again at once.
pub fn ehci_device_intr_done(xfer: &'static UsbdXfer) {
    let sc = ehci_softc(xfer.device().bus());
    let epipe = ehci_pipe(xfer.pipe());
    let ex = ehci_xfer(xfer);

    if xfer.pipe().repeat.get() != 0 {
        ehci_free_sqtd_chain(sc, ex);

        let sqh = epipe.sqh();

        let (data, dataend) = match ehci_alloc_sqtd_chain(sc, xfer.length.get(), xfer) {
            Ok(c) => c,
            Err(err) => {
                xfer.status.set(err);
                return;
            }
        };

        ehci_device_data_start(sc, xfer, ex, sqh, data, dataend, "ehci_device_intr_done");
    } else if xfer.status.get() != USBD_NOMEM {
        ehci_free_sqtd_chain(sc, ex);
    }
}

/// `ehci_device_isoc_transfer`: starts every isochronous xfer at once (they are scheduled
/// into future frames), even while another is in progress.
pub fn ehci_device_isoc_transfer(xfer: &'static UsbdXfer) -> UsbdStatus {
    let err = usb_insert_transfer(xfer);
    if err.is_err() && err != USBD_IN_PROGRESS {
        return err;
    }

    ehci_device_isoc_start(xfer)
}

/// `ehci_device_isoc_start`: builds the xfer's iTDs (high speed) or siTDs (full speed) and
/// links them into the frame list from a few frames ahead of the controller (or after the
/// pipe's previous xfer).
pub fn ehci_device_isoc_start(xfer: &'static UsbdXfer) -> UsbdStatus {
    let sc = ehci_softc(xfer.device().bus());
    let epipe = ehci_pipe(xfer.pipe());
    let ex = ehci_xfer(xfer);
    let ed = xfer.pipe().endpoint().edesc();
    let ival = u32::from(ed.bInterval);

    kassert!(xfer.rqflags.get() & URQ_REQUEST == 0);
    kassert!(ival > 0 && ival <= 16);

    // To allow continuous transfers, above we start all transfers immediately. However,
    // we're still going to get usbd_start_next call this when another xfer completes. So,
    // check if this is already in progress or not
    if ex.itdstart.get().is_some() {
        return USBD_IN_PROGRESS;
    }

    if sc.sc_bus.dying.get() != 0 {
        return USBD_IOERROR;
    }

    // Why would you do that anyway?
    if sc.sc_bus.use_polling.get() != 0 {
        return USBD_INVAL;
    }

    // To avoid complication, don't allow a request right now that'll span the entire frame
    // table. To within 4 frames, to allow some leeway on either side of where the hc
    // currently is.
    let flsize = sc.sc_flsize.get() as i64;
    if (1i64 << (ival - 1)) * i64::from(xfer.nframes.get()) >= (flsize - 4) * 8 {
        return USBD_INVAL;
    }

    // Step 1: Allocate and initialize itds.
    let link = if xfer.device().speed.get() == USB_SPEED_HIGH {
        if ehci_alloc_itd_chain(sc, xfer).is_err() {
            return USBD_INVAL;
        }

        EHCI_LINK_ITD
    } else {
        if ehci_alloc_sitd_chain(sc, xfer).is_err() {
            return USBD_INVAL;
        }

        EHCI_LINK_SITD
    };

    #[cfg(feature = "diagnostic")]
    {
        if ex.isdone.get() == 0 {
            printf(format_args!(
                "ehci_device_isoc_start: not done, ex={:p}\n",
                ex
            ));
        }
        ex.isdone.set(0);
    }

    // Part 2: Transfer descriptors have now been set up, now they must be scheduled into
    // the period frame list. Erk. Not wanting to complicate matters, transfer is denied if
    // the transfer spans more than the period frame list.
    let s = splusb();

    // Start inserting frames
    let mut frindex = if epipe.cur_xfers.get() > 0 {
        epipe.next_frame.get()
    } else {
        let f = eoread4(sc, EHCI_FRINDEX);
        let f = f >> 3; // Erase microframe index
        f + 2
    };

    let flsize = sc.sc_flsize.get();
    if frindex >= flsize {
        frindex &= flsize - 1;
    }

    // What's the frame interval?
    let mut ival = 1u32 << (ival - 1);
    if ival / 8 == 0 {
        ival = 1;
    } else {
        ival /= 8;
    }

    // Abuse the fact that itd_next == sitd_next.
    let mut itd = ex.itdstart.get();
    while let Some(t) = itd {
        let slot = frindex as usize;
        ehci_set(&t.itd.itd_next, sc.flist(slot));
        if ehci_get(&t.itd.itd_next) == 0 {
            ehci_set(&t.itd.itd_next, EHCI_LINK_TERMINATE);
        }

        sc.set_flist(slot, link | t.physaddr.get());
        t.frame_next.set(sc.softitd(slot));
        sc.set_softitd(slot, Some(t));
        if let Some(next) = t.frame_next.get() {
            next.frame_prev.set(Some(t));
        }
        t.slot.set(frindex as i32);
        t.frame_prev.set(None);

        frindex += ival;
        if frindex >= flsize {
            frindex -= flsize;
        }
        itd = t.xfer_next.get();
    }

    epipe.cur_xfers.set(epipe.cur_xfers.get() + 1);
    epipe.next_frame.set(frindex);

    ehci_intrhead_insert(sc, ex);
    xfer.status.set(USBD_IN_PROGRESS);
    xfer.done.store(false, Ordering::Relaxed);
    splx(s);

    USBD_IN_PROGRESS
}

/// `ehci_alloc_itd_chain`: the iTDs of a high speed isochronous xfer, one per frame (or per
/// several transactions when the interval is below 8 microframes).
fn ehci_alloc_itd_chain(sc: &'static EhciSoftc, xfer: &'static UsbdXfer) -> Result<(), ()> {
    let ex = ehci_xfer(xfer);
    let ed = xfer.pipe().endpoint().edesc();
    let mps = ugetw(ed.wMaxPacketSize);
    let mut itd: Option<&'static EhciSoftItd> = None;
    let mut pitd: Option<&'static EhciSoftItd> = None;
    let mut offs: u32 = 0;
    let mut trans_count: i32 = 0;

    // How many itds do we need? One per transfer if interval >= 8 microframes, fewer if we
    // use multiple microframes per frame.
    let ufrperframe = match ed.bInterval {
        1 => 8,
        2 => 4,
        3 => 2,
        _ => 1,
    };
    let nframes = (xfer.nframes.get() + (ufrperframe - 1)) / ufrperframe;
    let uframes = (8 / ufrperframe) as usize;
    if nframes == 0 {
        return Err(());
    }

    let isread = usbd_xfer_isread(xfer);
    usb_syncmem(
        &xfer.dmabuf,
        0,
        xfer.length.get() as usize,
        if isread {
            BUS_DMASYNC_PREREAD
        } else {
            BUS_DMASYNC_PREWRITE
        },
    );
    let block_size = xfer.dmabuf.block().size;
    for _ in 0..nframes {
        let froffs = offs;

        let Some(t) = ehci_alloc_itd(sc) else {
            ehci_free_itd_chain(sc, ex);
            return Err(());
        };
        itd = Some(t);

        match pitd {
            Some(p) => p.xfer_next.set(Some(t)),
            None => ex.itdstart.set(Some(t)),
        }

        // Step 1.5, initialize uframes
        let mut j = 0;
        while j < 8 {
            // Calculate which page in the list this starts in
            let addr = dmaaddr(&xfer.dmabuf, froffs as usize) as u32;
            let addr = ehci_page_offset(addr).wrapping_add(offs - froffs);
            let addr = ehci_page(addr) / EHCI_PAGE_SIZE;

            // This gets the initial offset into the first page, looks how far further along
            // the current uframe offset is. Works out how many pages that is.
            let fl = u32::from(frlength(xfer, trans_count));
            ehci_set(
                &t.itd.itd_ctl[j],
                EHCI_ITD_ACTIVE
                    | ehci_itd_set_len(fl)
                    | ehci_itd_set_pg(addr)
                    | ehci_itd_set_offs(dmaaddr(&xfer.dmabuf, offs as usize) as u32),
            );

            offs += fl;
            trans_count += 1;

            if trans_count >= xfer.nframes.get() {
                // Set IOC
                let c = &t.itd.itd_ctl[j];
                ehci_set(c, ehci_get(c) | EHCI_ITD_IOC);
                break;
            }
            j += uframes;
        }

        // Step 1.75, set buffer pointers. To simplify matters, all pointers are filled out
        // for the next 7 hardware pages in the dma block, so no need to worry what pages to
        // cover and what to not.
        for j in 0..EHCI_ITD_NBUFFERS {
            // Don't try to lookup a page that's past the end of buffer
            let page_offs = ehci_page(froffs + EHCI_PAGE_SIZE * j as u32) as usize;

            if page_offs >= block_size {
                break;
            }

            let page = dmaaddr(&xfer.dmabuf, page_offs) as u64;
            let page = page & !0xfff;
            ehci_set(&t.itd.itd_bufr[j], page as u32);
            ehci_set(&t.itd.itd_bufr_hi[j], (page >> 32) as u32);
        }

        // Other special values
        let b = &t.itd.itd_bufr[0];
        ehci_set(
            b,
            ehci_get(b)
                | ehci_itd_set_endpt(u32::from(ue_get_addr(ed.bEndpointAddress)))
                | ehci_itd_set_daddr(u32::from(xfer.pipe().device().address.get())),
        );

        let b = &t.itd.itd_bufr[1];
        ehci_set(
            b,
            ehci_get(b)
                | if isread { ehci_itd_set_dir(1) } else { 0 }
                | ehci_itd_set_maxpkt(u32::from(ue_get_size(mps))),
        );
        // FIXME: handle invalid trans
        let b = &t.itd.itd_bufr[2];
        ehci_set(
            b,
            ehci_get(b) | ehci_itd_set_multi(u32::from(ue_get_trans(mps)) + 1),
        );

        pitd = Some(t);
    }

    ex.itdend.set(itd);

    Ok(())
}

/// `ehci_alloc_sitd_chain`: the siTDs of a full speed isochronous xfer behind a transaction
/// translator, one per frame.
fn ehci_alloc_sitd_chain(sc: &'static EhciSoftc, xfer: &'static UsbdXfer) -> Result<(), ()> {
    let ex = ehci_xfer(xfer);
    let dev = xfer.device();
    let hsport = match dev.myhsport.get() {
        Some(p) => p,
        None => panic(format_args!("ehci_alloc_sitd_chain: no high speed hub")),
    };
    let hshub = match hsport.parent.get() {
        Some(h) => h,
        None => panic(format_args!("ehci_alloc_sitd_chain: port without a hub")),
    };
    let ed = xfer.pipe().endpoint().edesc();
    let mut itd: Option<&'static EhciSoftItd> = None;
    let mut pitd: Option<&'static EhciSoftItd> = None;
    let mut offs: usize = 0;

    let nframes = xfer.nframes.get();
    if nframes == 0 {
        return Err(());
    }

    let isread = usbd_xfer_isread(xfer);
    let mut endp = ehci_sitd_set_endpt(u32::from(ue_get_addr(ed.bEndpointAddress)))
        | ehci_sitd_set_addr(u32::from(dev.address.get()))
        | ehci_sitd_set_port(u32::from(hsport.portno.get()))
        | ehci_sitd_set_huba(u32::from(hshub.address.get()));

    if isread {
        endp |= ehci_sitd_set_dir(1);
    }

    usb_syncmem(
        &xfer.dmabuf,
        0,
        xfer.length.get() as usize,
        if isread {
            BUS_DMASYNC_PREREAD
        } else {
            BUS_DMASYNC_PREWRITE
        },
    );
    for i in 0..nframes {
        let fl = u32::from(frlength(xfer, i));
        let addr = dmaaddr(&xfer.dmabuf, offs) as u32;
        let mut page = ehci_page(addr.wrapping_add(fl).wrapping_sub(1));

        let Some(t) = ehci_alloc_itd(sc) else {
            ehci_free_itd_chain(sc, ex);
            return Err(());
        };
        itd = Some(t);
        match pitd {
            Some(p) => p.xfer_next.set(Some(t)),
            None => ex.itdstart.set(Some(t)),
        }

        let sitd = t.sitd();
        ehci_set(&sitd.sitd_endp, endp);
        ehci_set(&sitd.sitd_back, EHCI_LINK_TERMINATE);
        ehci_set(
            &sitd.sitd_trans,
            EHCI_SITD_ACTIVE
                | ehci_sitd_set_len(fl)
                | if i == nframes - 1 { EHCI_SITD_IOC } else { 0 },
        );

        let uf = 1u32.max(fl.div_ceil(188));

        // Since we do not yet budget and schedule micro-frames we assume there is no other
        // transfer using the same TT.
        let (smask, cmask): (u8, u8) = if isread {
            (0x01, ((((1u32 << (uf + 2)) - 1) << 2) & 0xff) as u8)
        } else {
            // Is the payload is greater than 188 bytes?
            let tp = if uf == 1 {
                EHCI_SITD_TP_ALL
            } else {
                EHCI_SITD_TP_BEGIN
            };

            page |= ehci_sitd_set_tcount(uf) | ehci_sitd_set_tp(tp);
            ((((1u32 << uf) - 1) & 0xff) as u8, 0x00)
        };

        ehci_set(
            &sitd.sitd_sched,
            ehci_sitd_set_smask(u32::from(smask)) | ehci_sitd_set_cmask(u32::from(cmask)),
        );
        ehci_set(&sitd.sitd_bufr[0], addr);
        ehci_set(&sitd.sitd_bufr[1], page);

        offs += fl as usize;
        pitd = Some(t);
    }

    ex.itdend.set(itd);

    Ok(())
}

/// `ehci_device_isoc_abort`.
pub fn ehci_device_isoc_abort(xfer: &'static UsbdXfer) {
    let s = splusb();
    ehci_abort_isoc_xfer(xfer, USBD_CANCELLED);
    splx(s);
}

/// `ehci_device_isoc_close`.
pub fn ehci_device_isoc_close(_pipe: &'static UsbdPipe) {}

/// `ehci_device_isoc_done`: unlinks and frees the xfer's isochronous descriptors.
pub fn ehci_device_isoc_done(xfer: &'static UsbdXfer) {
    let sc = ehci_softc(xfer.device().bus());
    let epipe = ehci_pipe(xfer.pipe());
    let ex = ehci_xfer(xfer);

    let s = splusb();
    epipe.cur_xfers.set(epipe.cur_xfers.get().wrapping_sub(1));
    if xfer.status.get() != USBD_NOMEM {
        ehci_rem_itd_chain(sc, ex);
        ehci_free_itd_chain(sc, ex);
    }
    splx(s);
}

const _: () = {
    // `pool_init(ehcixfer, sizeof(struct ehci_xfer), 0, ...)`: the pool's default alignment
    // (`ALIGN(1)`, at least 8) must suit the type `ehci_allocx` casts its items to.
    assert!(align_of::<EhciXfer>() <= 8);
    // The chunks are larger than `USB_MEM_SMALL`, so each gets a page-aligned block of its
    // own from `usb_allocmem`, as `ehci_carve` needs.
    assert!(EHCI_SQH_SIZE * EHCI_SQH_CHUNK > crate::dev::usb::usb_mem::USB_MEM_SMALL);
    assert!(EHCI_SQTD_SIZE * EHCI_SQTD_CHUNK > crate::dev::usb::usb_mem::USB_MEM_SMALL);
    assert!(EHCI_ITD_SIZE * EHCI_ITD_CHUNK > crate::dev::usb::usb_mem::USB_MEM_SMALL);
};
/* </CODE> */

/* <TESTS> */
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn reverse_bits() {
        assert_eq!(ehci_reverse_bits(0, 7), 0);
        assert_eq!(ehci_reverse_bits(1, 7), 64);
        assert_eq!(ehci_reverse_bits(2, 7), 32);
        assert_eq!(ehci_reverse_bits(3, 7), 96);
        assert_eq!(ehci_reverse_bits(127, 7), 127);
        assert_eq!(ehci_reverse_bits(0x01, 8), 0x80);
        assert_eq!(ehci_reverse_bits(0x0f, 8), 0xf0);
        // A permutation of 0..128.
        let mut seen = [false; 128];
        for i in 0..128u8 {
            let r = ehci_reverse_bits(i, 7) as usize;
            assert!(!seen[r]);
            seen[r] = true;
        }
    }

    #[test]
    fn frame_list_spreads_the_128ms_slots() {
        // Frame j of a 1024-entry list points at the 128ms QH of position reverse(j % 128):
        // consecutive frames hit QHs whose 1ms..64ms ancestors alternate.
        let last = EHCI_IPOLLRATES - 1;
        let slots: [usize; 4] = core::array::from_fn(|i| {
            ehci_iqhidx(last, u32::from(ehci_reverse_bits(i as u8, last)))
        });
        assert_eq!(slots, [127, 191, 159, 223]);
        // Their parents at the 2ms level differ for frames 0 and 1.
        assert_ne!((slots[0] + 1) / 2 - 1, (slots[1] + 1) / 2 - 1);
    }

    #[test]
    fn descriptor_offsets() {
        assert_eq!(QH_LINK, 0);
        assert_eq!(QH_QTD_STATUS, 24);
        assert_eq!(QTD_STATUS, 8);
        assert_eq!(ITD_NEXT, 0);
        assert_eq!(ITD_CTL, 4);
        assert_eq!(SITD_TRANS, 12);
    }

    #[test]
    fn root_hub_descriptors() {
        assert_eq!(EHCI_DEVD.as_bytes().len(), USB_DEVICE_DESCRIPTOR_SIZE);
        assert_eq!(EHCI_DEVD.bcdUSB, [0x00, 0x02]);
        assert_eq!(EHCI_ODEVD.bLength as usize, USB_DEVICE_DESCRIPTOR_SIZE);
        assert_eq!(
            usize::from(ugetw(EHCI_CONFD.wTotalLength)),
            USB_CONFIG_DESCRIPTOR_SIZE
                + USB_INTERFACE_DESCRIPTOR_SIZE
                + USB_ENDPOINT_DESCRIPTOR_SIZE
        );
        assert_eq!(EHCI_ENDPD.bEndpointAddress, 0x81);
        assert_eq!(EHCI_ENDPD.bInterval, 12);
        assert_eq!(EHCI_HUBD.bDescriptorType, UDESC_HUB);
    }
}
/* </TESTS> */
