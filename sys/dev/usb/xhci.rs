/* $OpenBSD: xhci.c,v 1.136 2025/03/01 14:43:03 kirill Exp $ */
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
/* </LICENSES> */

/* <CODE> */
//! xhci(4): the eXtensible Host Controller Interface driver, machine independent
//! (`dev/usb/xhci.c`). Bus front-ends (`dev/pci/xhci_pci.rs`) map the registers, establish
//! the interrupt and call [`xhci_init`], attach `usb(4)` and then [`xhci_config`].
//!
//! Upstream: sys/dev/usb/xhci.c @ 3ce1f3f79392
//!
//! The controller works from rings of TRBs in DMA memory: one command ring the driver
//! produces (`xhci_command_submit`, answered by command completion events), one event ring
//! the controller produces (`xhci_event_dequeue`, run by the soft interrupt), and one
//! transfer ring per open endpoint. A device gets a slot (`XHCI_CMD_ENABLE_SLOT`) when its
//! default pipe opens; its input context describes the slot and the endpoints, its output
//! context (pointed to by the DCBAA) is the controller's copy. The root hub is emulated:
//! `xhci_root_ctrl_start` answers the standard and hub requests from the port registers, and
//! a port status change event completes the root hub's interrupt transfer.
//!
//! Locking is the C's: everything but the hard interrupt runs under the kernel lock at
//! `splusb()`; commands are serialised by `sc_cmd_lock` and sleep for their completion. The
//! hard interrupt (`xhci_intr`, established `IPL_MPSAFE` by the front-end) only reads the
//! status registers and schedules the soft interrupt.
//!
//! ## Deviations
//! - `XHCI_DEBUG` is not in GENERIC: the `DPRINTF`s, `xhci_dump_trb`, `xhci_cmd_noop` and the
//!   debug blocks of `xhci_init`, `xhci_config` and `xhci_setaddr` are absent, as in a kernel
//!   built without it.
//! - `struct pool *xhcixfer`, `malloc`ed by the first `xhci_init`, is a static [`Pool`] with
//!   a flag for the C's `xhcixfer == NULL`; the C's "unable to allocate pool descriptor" path
//!   cannot happen.
//! - The casts `(struct xhci_softc *)bus`, `(struct xhci_pipe *)pipe` and `(struct xhci_xfer
//!   *)xfer` are checked: [`xhci_softc`] checks the bus's methods are this driver's, the pipe
//!   and xfer casts go through `UsbdPipe::hc`/`UsbdXfer::hc`.
//! - TRBs, contexts and tables in DMA memory are reached through [`XhciTrbRef`] and the
//!   bounds-checked accessors of `xhcivar.rs`; the C's out-of-range indexes (an event naming
//!   slot `USB_MAX_DEVICES` or DCI 0, a port bit beyond the root hub's status buffer) are
//!   ignored where the C would index past its arrays.
//! - `xhci_xfer_get_trb` returns the TRB and the ring's toggle as a pair instead of through
//!   `uint8_t *togglep`; `xhci_ring_consume` returns `Option` (NULL); `xhci_ring_produce`
//!   cannot fail, so `xhci_command_submit`'s `EAGAIN` for a NULL TRB is gone.
//! - `xhci_pipe.halted` holds a `UsbdStatus`, `USBD_NORMAL_COMPLETION` for the C's 0 (not
//!   halted) and `USBD_IN_PROGRESS` for the C's literal 1.
//! - `xhci_context_setup` returns `EINVAL` for an unknown speed where the C returns
//!   `USBD_INVAL` through its `int`; the callers only test for nonzero. `xhci_command_abort`
//!   returns `false` where the C returns 1 (timeout).
//! - `usbd_dma_contig_alloc` records the map only once everything succeeded (the C leaves a
//!   destroyed map in `dma->map` on failure); it returns the kernel address instead of
//!   filling `void **kvap`.
//! - `xhci_root_ctrl_start` copies at most the xfer's buffer (`min(wLength, length)`) and at
//!   most the descriptor's own size (`hubd.bDescLength` grows with the port count beyond
//!   `sizeof(hubd)` past 31 ports in C); string descriptors are built in a local
//!   `usb_string_descriptor_t` and copied.
//! - `fls(ival)` (libkern, not ported) is `32 - ival.leading_zeros()`.

use core::cell::Cell;
use core::ffi::c_void;
use core::ptr::{self, NonNull};
use core::sync::atomic::{AtomicBool, Ordering};

use crate::dev::usb::usb::usb_schedsoftintr;
use crate::dev::usb::usb::{
    UC_BUS_POWERED, UC_SELF_POWERED, UDCLASS_HUB, UDESC_CONFIG, UDESC_DEVICE, UDESC_ENDPOINT,
    UDESC_ENDPOINT_SS_COMP, UDESC_INTERFACE, UDESC_OTHER_SPEED_CONFIGURATION, UDESC_SS_HUB,
    UDESC_STRING, UDPROTO_HSHUBMTT, UDPROTO_HSHUBSTT, UDS_SELF_POWERED, UDSUBCLASS_HUB, UE_BULK,
    UE_CONTROL, UE_DIR_IN, UE_INTERRUPT, UE_ISOCHRONOUS, UHD_PORT_IND, UHD_PWR_GANGED,
    UHD_PWR_INDIVIDUAL, UHF_C_BH_PORT_RESET, UHF_C_PORT_CONNECTION, UHF_C_PORT_ENABLE,
    UHF_C_PORT_LINK_STATE, UHF_C_PORT_OVER_CURRENT, UHF_C_PORT_RESET, UHF_C_PORT_SUSPEND,
    UHF_PORT_ENABLE, UHF_PORT_INDICATOR, UHF_PORT_POWER, UHF_PORT_RESET, UHF_PORT_SUSPEND,
    UICLASS_HUB, UIPROTO_HSHUBSTT, UISUBCLASS_HUB, UPS_C_BH_PORT_RESET, UPS_C_CONNECT_STATUS,
    UPS_C_OVERCURRENT_INDICATOR, UPS_C_PORT_CONFIG_ERROR, UPS_C_PORT_ENABLED,
    UPS_C_PORT_LINK_STATE, UPS_C_PORT_RESET, UPS_CURRENT_CONNECT_STATUS, UPS_FULL_SPEED,
    UPS_HIGH_SPEED, UPS_LOW_SPEED, UPS_OVERCURRENT_INDICATOR, UPS_PORT_ENABLED, UPS_PORT_POWER,
    UPS_PORT_POWER_SS, UPS_RESET, UR_CLEAR_FEATURE, UR_CLEAR_TT_BUFFER, UR_GET_CONFIG,
    UR_GET_DESCRIPTOR, UR_GET_INTERFACE, UR_GET_STATUS, UR_GET_TT_STATE, UR_RESET_TT,
    UR_SET_ADDRESS, UR_SET_CONFIG, UR_SET_DESCRIPTOR, UR_SET_FEATURE, UR_SET_INTERFACE, UR_STOP_TT,
    UR_SYNCH_FRAME, USB_CONFIG_DESCRIPTOR_SIZE, USB_CONTROL_ENDPOINT, USB_DEVICE_DESCRIPTOR_SIZE,
    USB_ENDPOINT_DESCRIPTOR_SIZE, USB_ENDPOINT_SS_COMP_DESCRIPTOR_SIZE, USB_HUB_DESCRIPTOR_SIZE,
    USB_INTERFACE_DESCRIPTOR_SIZE, USB_MAX_DEVICES, USB_RESUME_WAIT, USB_SPEED_FULL,
    USB_SPEED_HIGH, USB_SPEED_LOW, USB_SPEED_SUPER, UT_READ_CLASS_DEVICE, UT_READ_CLASS_OTHER,
    UT_READ_DEVICE, UT_READ_ENDPOINT, UT_READ_INTERFACE, UT_WRITE_CLASS_DEVICE,
    UT_WRITE_CLASS_OTHER, UT_WRITE_DEVICE, UT_WRITE_ENDPOINT, UT_WRITE_INTERFACE,
    UsbConfigDescriptor, UsbDeviceDescriptor, UsbEndpointDescriptor, UsbEndpointSsCompDescriptor,
    UsbHubDescriptor, UsbInterfaceDescriptor, UsbPortStatus, UsbStringDescriptor, UsbWire,
    ue_get_addr, ue_get_dir, ue_get_size, ue_get_trans, ue_get_xfertype, ugetw, ups_port_ls_set,
    usetw,
};
use crate::dev::usb::usb::{usb_add_task, usb_rem_task};
use crate::dev::usb::usb_mem::{dmaaddr, kernaddr, usb_syncmem};
use crate::dev::usb::usb_subr::usb_delay_ms;
use crate::dev::usb::usbdi::{
    IPL_SOFTUSB, USB_TASK_TYPE_ABORT, USBD_CANCELLED, USBD_FORCE_SHORT_XFER, USBD_IN_PROGRESS,
    USBD_INVAL, USBD_IOERROR, USBD_NOMEM, USBD_NORMAL_COMPLETION, USBD_NOT_STARTED, USBD_STALLED,
    USBD_TIMEOUT, UsbdStatus, splusb, usb_init_task, usb_insert_transfer, usb_transfer_complete,
    usbd_str,
};
use crate::dev::usb::usbdivar::{
    URQ_REQUEST, USBREV_3_0, UsbdBus, UsbdBusMethods, UsbdDevice, UsbdHcPipe, UsbdPipe,
    UsbdPipeMethods, UsbdXfer, usbd_bus_set_hc_types, usbd_xfer_isread,
};
use crate::dev::usb::xhcireg::*;
use crate::dev::usb::xhcivar::{
    UsbdDmaInfo, XHCI_CMD_TIMEOUT, XHCI_MAX_CMDS, XHCI_MAX_EVTS, XHCI_MAX_XFER, XHCI_NOCSS,
    XhciRing, XhciSoftc, XhciTrbRef, XhciXfer, xdwrite4, xoread4, xowrite4, xread1, xread2, xread4,
    xrread4, xrwrite4,
};
use crate::kassert;
use crate::kern::kern_rwlock::{rw_assert_wrlock, rw_enter_write, rw_exit_write, rw_init};
use crate::kern::kern_synch::{tsleep_nsec, wakeup};
use crate::kern::kern_timeout::{timeout_add_msec, timeout_del, timeout_set};
use crate::kern::subr_autoconf::{config_activate_children, config_detach_children};
use crate::kern::subr_pool::{pool_get, pool_init, pool_put};
use crate::kern::subr_prf::{panic, printf};
use crate::machine::bus::{
    BUS_DMA_64BIT, BUS_DMA_COHERENT, BUS_DMA_NOWAIT, BUS_DMA_ZERO, BUS_DMASYNC_POSTREAD,
    BUS_DMASYNC_POSTWRITE, BUS_DMASYNC_PREREAD, BUS_DMASYNC_PREWRITE, BusDmaSegment, BusSize,
    bus_dmamap_create, bus_dmamap_destroy, bus_dmamap_load_raw, bus_dmamap_sync, bus_dmamap_unload,
    bus_dmamem_alloc, bus_dmamem_free, bus_dmamem_map, bus_dmamem_unmap,
};
use crate::machine::cpu::delay;
use crate::machine::intr::{splsoftassert, splx};
use crate::sys::device::{
    CD_SKIPHIBERNATE, Cfdriver, DV_DULL, DVACT_POWERDOWN, DVACT_RESUME, Device,
};
use crate::sys::errno::Errno;
use crate::sys::param::{PZERO, howmany};
use crate::sys::pool::{PR_NOWAIT, PR_ZERO, Pool};

/// `XHCI_INTR_ENDPT`: the root hub's interrupt endpoint.
const XHCI_INTR_ENDPT: u8 = 1;

/// `TRB_PROCESSED_NO`.
const TRB_PROCESSED_NO: u8 = 0;
/// `TRB_PROCESSED_YES`.
const TRB_PROCESSED_YES: u8 = 1;
/// `TRB_PROCESSED_SHORT`.
const TRB_PROCESSED_SHORT: u8 = 2;

/// `struct xhci_pipe`: a pipe of this controller and its transfer ring.
#[repr(C)]
pub struct XhciPipe {
    /// `pipe`: the generic pipe, first.
    pub pipe: UsbdPipe,

    /// `dci`: the endpoint's device context index.
    pub dci: Cell<u8>,
    /// `slot`: Device slot ID.
    pub slot: Cell<u8>,
    /// `ring`: the transfer ring.
    pub ring: XhciRing,

    /// `pending_xfers`: the xfer each TRB belongs to (XXX used to pass the xfer pointer back
    /// to the interrupt routine, better way?).
    pub pending_xfers: [Cell<Option<&'static UsbdXfer>>; XHCI_MAX_XFER],
    /// `aborted_xfer`.
    pub aborted_xfer: Cell<Option<&'static UsbdXfer>>,
    /// `halted`: `USBD_NORMAL_COMPLETION` when running, else the status to report.
    pub halted: Cell<UsbdStatus>,
    /// `free_trbs`.
    pub free_trbs: Cell<usize>,
    /// `skip`.
    pub skip: Cell<i32>,
    /// `trb_processed`: `TRB_PROCESSED_*`, per TRB (isochronous transfers).
    pub trb_processed: [Cell<u8>; XHCI_MAX_XFER],
}

// SAFETY: `#[repr(C)]` with the `usbd_pipe` first; the other members are `Cell`s of
// integers, raw pointers, `Option`s of references, a DMA tag option and a `UsbdStatus` whose
// zero is `USBD_NORMAL_COMPLETION`: all valid as zero bits (`usbd_setup_pipe`'s `M_ZERO`).
unsafe impl UsbdHcPipe for XhciPipe {}

/// `xhci_cd`.
pub static XHCI_CD: Cfdriver = Cfdriver::new(b"xhci", DV_DULL, CD_SKIPHIBERNATE);

/// `xhcixfer`: the pool of [`XhciXfer`]s.
static XHCIXFER: Pool = Pool::new();
/// `xhcixfer != NULL`: the pool is initialised.
static XHCIXFER_INIT: AtomicBool = AtomicBool::new(false);

/// `xhci_bus_methods`.
pub static XHCI_BUS_METHODS: UsbdBusMethods = UsbdBusMethods {
    open_pipe: xhci_pipe_open,
    dev_setaddr: Some(xhci_setaddr),
    soft_intr: xhci_softintr,
    do_poll: xhci_poll,
    allocx: xhci_allocx,
    freex: xhci_freex,
};

/// `xhci_root_ctrl_methods`.
static XHCI_ROOT_CTRL_METHODS: UsbdPipeMethods = UsbdPipeMethods {
    transfer: xhci_root_ctrl_transfer,
    start: xhci_root_ctrl_start,
    abort: xhci_noop,
    close: xhci_pipe_close,
    cleartoggle: None,
    done: xhci_noop,
};

/// `xhci_root_intr_methods`.
static XHCI_ROOT_INTR_METHODS: UsbdPipeMethods = UsbdPipeMethods {
    transfer: xhci_root_intr_transfer,
    start: xhci_root_intr_start,
    abort: xhci_root_intr_abort,
    close: xhci_pipe_close,
    cleartoggle: None,
    done: xhci_root_intr_done,
};

/// `xhci_device_ctrl_methods`.
static XHCI_DEVICE_CTRL_METHODS: UsbdPipeMethods = UsbdPipeMethods {
    transfer: xhci_device_ctrl_transfer,
    start: xhci_device_ctrl_start,
    abort: xhci_device_ctrl_abort,
    close: xhci_pipe_close,
    cleartoggle: None,
    done: xhci_noop,
};

/// `xhci_device_intr_methods`.
static XHCI_DEVICE_INTR_METHODS: UsbdPipeMethods = UsbdPipeMethods {
    transfer: xhci_device_generic_transfer,
    start: xhci_device_generic_start,
    abort: xhci_device_generic_abort,
    close: xhci_pipe_close,
    cleartoggle: None,
    done: xhci_device_generic_done,
};

/// `xhci_device_bulk_methods`.
static XHCI_DEVICE_BULK_METHODS: UsbdPipeMethods = UsbdPipeMethods {
    transfer: xhci_device_generic_transfer,
    start: xhci_device_generic_start,
    abort: xhci_device_generic_abort,
    close: xhci_pipe_close,
    cleartoggle: None,
    done: xhci_device_generic_done,
};

/// `xhci_device_isoc_methods`.
static XHCI_DEVICE_ISOC_METHODS: UsbdPipeMethods = UsbdPipeMethods {
    transfer: xhci_device_isoc_transfer,
    start: xhci_device_isoc_start,
    abort: xhci_device_generic_abort,
    close: xhci_pipe_close,
    cleartoggle: None,
    done: xhci_noop,
};

/* Root hub descriptors. */
/// `xhci_devd`.
static XHCI_DEVD: UsbDeviceDescriptor = UsbDeviceDescriptor {
    bLength: USB_DEVICE_DESCRIPTOR_SIZE as u8,
    bDescriptorType: UDESC_DEVICE,
    bcdUSB: [0x00, 0x03], // USB version
    bDeviceClass: UDCLASS_HUB,
    bDeviceSubClass: UDSUBCLASS_HUB,
    bDeviceProtocol: UDPROTO_HSHUBSTT,
    bMaxPacketSize: 9,
    idVendor: [0, 0],
    idProduct: [0, 0],
    bcdDevice: [0x00, 0x01],
    iManufacturer: 1,
    iProduct: 2,
    iSerialNumber: 0,
    bNumConfigurations: 1,
};

/// `xhci_confd`.
static XHCI_CONFD: UsbConfigDescriptor = UsbConfigDescriptor {
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

/// `xhci_ifcd`.
static XHCI_IFCD: UsbInterfaceDescriptor = UsbInterfaceDescriptor {
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

/// `xhci_endpd`.
static XHCI_ENDPD: UsbEndpointDescriptor = UsbEndpointDescriptor {
    bLength: USB_ENDPOINT_DESCRIPTOR_SIZE as u8,
    bDescriptorType: UDESC_ENDPOINT,
    bEndpointAddress: UE_DIR_IN | XHCI_INTR_ENDPT,
    bmAttributes: UE_INTERRUPT,
    wMaxPacketSize: [2, 0], // max 15 ports
    bInterval: 255,
};

/// `xhci_endpcd`.
#[allow(dead_code)] // the C defines it for the super speed root hub; nothing sends it yet
static XHCI_ENDPCD: UsbEndpointSsCompDescriptor = UsbEndpointSsCompDescriptor {
    bLength: USB_ENDPOINT_SS_COMP_DESCRIPTOR_SIZE as u8,
    bDescriptorType: UDESC_ENDPOINT_SS_COMP,
    bMaxBurst: 0,
    bmAttributes: 0,
    wBytesPerInterval: [0, 0],
};

/// `xhci_hubd`.
static XHCI_HUBD: UsbHubDescriptor = UsbHubDescriptor {
    bDescLength: USB_HUB_DESCRIPTOR_SIZE as u8,
    bDescriptorType: UDESC_SS_HUB,
    bNbrPorts: 0,
    wHubCharacteristics: [0, 0],
    bPwrOn2PwrGood: 0,
    bHubContrCurrent: 0,
    DeviceRemovable: [0; 32],
};

/// `(struct xhci_softc *)bus`: the softc whose `sc_bus` this is. Panics on a bus of another
/// controller.
pub fn xhci_softc(bus: &'static UsbdBus) -> &'static XhciSoftc {
    if !bus
        .methods
        .get()
        .is_some_and(|m| ptr::eq(m, &XHCI_BUS_METHODS))
    {
        panic(format_args!("{}: not an xhci bus", bus.bdev.xname()));
    }
    // SAFETY: only `xhci_init` installs `XHCI_BUS_METHODS`, on the `usbd_bus` that heads an
    // `XhciSoftc` (`#[repr(C)]`, bus first), which lives as long as the bus.
    unsafe { &*ptr::from_ref(bus).cast::<XhciSoftc>() }
}

/// `(struct xhci_pipe *)pipe`.
fn xhci_pipe(pipe: &'static UsbdPipe) -> &'static XhciPipe {
    pipe.hc::<XhciPipe>()
}

/// `(struct xhci_xfer *)xfer`.
fn xhci_xfer(xfer: &'static UsbdXfer) -> &'static XhciXfer {
    xfer.hc::<XhciXfer>()
}

/// `DEVNAME(sc)`.
fn devname(sc: &XhciSoftc) -> &str {
    sc.sc_bus.bdev.xname()
}

/// `SIMPLEQ_FIRST(&pipe->queue)` of a pipe the caller just queued on.
fn queue_first(pipe: &'static UsbdPipe) -> &'static UsbdXfer {
    match pipe.queue.first() {
        Some(x) => x,
        None => panic(format_args!("xhci: empty pipe queue")),
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

/// `&xfer->frlengths[i]`, checked against `nframes`.
fn frlength_ptr(xfer: &UsbdXfer, i: i32) -> *mut u16 {
    let base = xfer.frlengths.get();
    if base.is_null() || i < 0 || i >= xfer.nframes.get() {
        panic(format_args!("xhci: frame {i} outside the xfer"));
    }
    base.wrapping_add(i as usize)
}

/// `usbd_dma_contig_alloc`: `size` bytes of zeroed, mapped and loaded DMA memory in one
/// segment; returns its kernel address.
pub fn usbd_dma_contig_alloc(
    bus: &UsbdBus,
    dma: &UsbdDmaInfo,
    size: BusSize,
    alignment: BusSize,
    boundary: BusSize,
) -> Result<NonNull<u8>, Errno> {
    let tag = bus.dmatag();
    dma.tag.set(Some(tag));
    dma.size.set(size);

    let map = bus_dmamap_create(
        tag,
        size,
        1,
        size,
        boundary,
        BUS_DMA_NOWAIT | bus.dmaflags.get(),
    )?;

    let mut segs = [BusDmaSegment::default(); 1];
    let nsegs = match bus_dmamem_alloc(
        tag,
        size,
        alignment,
        boundary,
        &mut segs,
        BUS_DMA_NOWAIT | BUS_DMA_ZERO | bus.dmaflags.get(),
    ) {
        Ok(n) => n,
        Err(e) => {
            // destroy:
            // SAFETY: the map was created above and is not used again.
            unsafe { bus_dmamap_destroy(tag, NonNull::from(map)) };
            return Err(e);
        }
    };

    let vaddr = match bus_dmamem_map(tag, &mut segs, size, BUS_DMA_NOWAIT | BUS_DMA_COHERENT) {
        Ok(v) => v,
        Err(e) => {
            // free: destroy:
            // SAFETY: the segment allocated above, not mapped and not used again; the map
            // was created above.
            unsafe {
                bus_dmamem_free(tag, &segs);
                bus_dmamap_destroy(tag, NonNull::from(map));
            }
            return Err(e);
        }
    };

    // SAFETY: the segment is this allocation's own, mapped at `vaddr` until
    // `usbd_dma_contig_free` unloads it.
    if let Err(e) = unsafe { bus_dmamap_load_raw(tag, map, &segs, size, BUS_DMA_NOWAIT) } {
        // unmap: free: destroy:
        // SAFETY: the mapping, segment and map made above, none used again.
        unsafe {
            bus_dmamem_unmap(tag, vaddr, size);
            bus_dmamem_free(tag, &segs);
            bus_dmamap_destroy(tag, NonNull::from(map));
        }
        return Err(e);
    }

    bus_dmamap_sync(
        tag,
        map,
        0,
        size,
        BUS_DMASYNC_PREREAD | BUS_DMASYNC_PREWRITE,
    );

    dma.map.set(Some(map));
    dma.seg.set(segs[0]);
    dma.nsegs.set(nsegs as i32);
    dma.vaddr.set(vaddr.as_ptr());
    let seg0 = map.dm_segs().first().map(|s| s.get()).unwrap_or_default();
    dma.paddr.set(seg0.ds_addr);

    Ok(vaddr)
}

/// `usbd_dma_contig_free`.
pub fn usbd_dma_contig_free(bus: &UsbdBus, dma: &UsbdDmaInfo) {
    if let Some(map) = dma.map.get() {
        let tag = bus.dmatag();
        bus_dmamap_sync(
            tag,
            map,
            0,
            dma.size.get(),
            BUS_DMASYNC_POSTREAD | BUS_DMASYNC_POSTWRITE,
        );
        bus_dmamap_unload(tag, map);
        if let Some(vaddr) = NonNull::new(dma.vaddr.get()) {
            // SAFETY: the mapping `usbd_dma_contig_alloc` made, unloaded above; nothing
            // keeps a pointer into it (the ring, table or context pointers are dropped with
            // the structure that owns them).
            unsafe { bus_dmamem_unmap(tag, vaddr, dma.size.get()) };
        }
        // SAFETY: the segment and map `usbd_dma_contig_alloc` made, unmapped and unloaded.
        unsafe {
            bus_dmamem_free(tag, &[dma.seg.get()]);
            bus_dmamap_destroy(tag, NonNull::from(map));
        }
        dma.map.set(None);
    }
}

/// `xhci_init`: reads the capabilities, resets the controller and allocates the DCBAA, the
/// command and event rings, the segment table and the scratchpad. Prints the end of the
/// front-end's attach line (`", xHCI 1.0"`).
pub fn xhci_init(sc: &'static XhciSoftc) -> Result<(), Errno> {
    sc.sc_bus.usbrev.set(USBREV_3_0);
    sc.sc_bus.methods.set(Some(&XHCI_BUS_METHODS));
    // SAFETY: `xhci_allocx`, this bus's `allocx`, returns only xfers that head an `XhciXfer`.
    unsafe { usbd_bus_set_hc_types::<XhciPipe, XhciXfer>(&sc.sc_bus) };

    sc.sc_oper_off
        .store(usize::from(xread1(sc, XHCI_CAPLENGTH)), Ordering::Relaxed);
    sc.sc_door_off
        .store(xread4(sc, XHCI_DBOFF) as usize, Ordering::Relaxed);
    sc.sc_runt_off
        .store(xread4(sc, XHCI_RTSOFF) as usize, Ordering::Relaxed);

    sc.sc_version.set(xread2(sc, XHCI_HCIVERSION));
    let v = sc.sc_version.get();
    printf(format_args!(", xHCI {:x}.{:x}\n", v >> 8, v & 0xff));

    xhci_reset(sc)?;

    if !XHCIXFER_INIT.load(Ordering::Relaxed) {
        pool_init(
            &XHCIXFER,
            size_of::<XhciXfer>(),
            0,
            IPL_SOFTUSB,
            0,
            "xhcixfer",
            None,
        );
        XHCIXFER_INIT.store(true, Ordering::Relaxed);
    }

    let hcr = xread4(sc, XHCI_HCCPARAMS);
    sc.sc_ctxsize
        .set(if xhci_hcc_csz(hcr) != 0 { 64 } else { 32 });
    if xhci_hcc_ac64(hcr) != 0 {
        sc.sc_bus
            .dmaflags
            .set(sc.sc_bus.dmaflags.get() | BUS_DMA_64BIT);
    }

    // Use 4K for the moment since it's easier.
    sc.sc_pagesize.set(4096);

    // Get port and device slot numbers.
    let hcr = xread4(sc, XHCI_HCSPARAMS1);
    sc.sc_noport.set(xhci_hcs1_n_ports(hcr) as i32);
    sc.sc_noslot.set(xhci_hcs1_devslot_max(hcr) as i32);

    let bus = &sc.sc_bus;
    let pagesize = sc.sc_pagesize.get() as BusSize;

    // Setup Device Context Base Address Array.
    let Ok(segs) = usbd_dma_contig_alloc(
        bus,
        &sc.sc_dcbaa.dma,
        (sc.sc_noslot.get() as usize + 1) * size_of::<u64>(),
        XHCI_DCBAA_ALIGN,
        pagesize,
    ) else {
        return Err(Errno::ENOMEM);
    };
    sc.sc_dcbaa.segs.set(segs.as_ptr().cast());

    // Setup command ring.
    rw_init(&sc.sc_cmd_lock, "xhcicmd");
    if let Err(e) = xhci_ring_alloc(sc, &sc.sc_cmd_ring, XHCI_MAX_CMDS, XHCI_CMDS_RING_ALIGN) {
        printf(format_args!(
            "{}: could not allocate command ring.\n",
            devname(sc)
        ));
        usbd_dma_contig_free(bus, &sc.sc_dcbaa.dma);
        return Err(e);
    }

    // Setup one event ring and its segment table (ERST).
    if let Err(e) = xhci_ring_alloc(sc, &sc.sc_evt_ring, XHCI_MAX_EVTS, XHCI_EVTS_RING_ALIGN) {
        printf(format_args!(
            "{}: could not allocate event ring.\n",
            devname(sc)
        ));
        xhci_ring_free(sc, &sc.sc_cmd_ring);
        usbd_dma_contig_free(bus, &sc.sc_dcbaa.dma);
        return Err(e);
    }

    // Allocate the required entry for the segment table.
    let Ok(segs) = usbd_dma_contig_alloc(
        bus,
        &sc.sc_erst.dma,
        size_of::<XhciErseg>(),
        XHCI_ERST_ALIGN,
        XHCI_ERST_BOUNDARY,
    ) else {
        printf(format_args!(
            "{}: could not allocate segment table.\n",
            devname(sc)
        ));
        xhci_ring_free(sc, &sc.sc_evt_ring);
        xhci_ring_free(sc, &sc.sc_cmd_ring);
        usbd_dma_contig_free(bus, &sc.sc_dcbaa.dma);
        return Err(Errno::ENOMEM);
    };
    sc.sc_erst.segs.set(segs.as_ptr().cast());

    // Set our ring address and size in its corresponding segment.
    sc.sc_erst.set_seg(
        0,
        XhciErseg {
            er_addr: (sc.sc_evt_ring.dma.paddr.get() as u64).to_le(),
            er_size: (XHCI_MAX_EVTS as u32).to_le(),
            er_rsvd: 0,
        },
    );
    bus_dmamap_sync(
        sc.sc_erst.dma.tag(),
        sc.sc_erst.dma.map(),
        0,
        sc.sc_erst.dma.size.get(),
        BUS_DMASYNC_PREREAD | BUS_DMASYNC_PREWRITE,
    );

    // Get the number of scratch pages and configure them if necessary.
    let hcr = xread4(sc, XHCI_HCSPARAMS2);
    let npage = xhci_hcs2_spb_max(hcr) as i32;

    if npage > 0 && xhci_scratchpad_alloc(sc, npage).is_err() {
        printf(format_args!(
            "{}: could not allocate scratchpad.\n",
            devname(sc)
        ));
        usbd_dma_contig_free(bus, &sc.sc_erst.dma);
        xhci_ring_free(sc, &sc.sc_evt_ring);
        xhci_ring_free(sc, &sc.sc_cmd_ring);
        usbd_dma_contig_free(bus, &sc.sc_dcbaa.dma);
        return Err(Errno::ENOMEM);
    }

    Ok(())
}

/// `xhci_config`: programs the slots, the DCBAA, the command ring and the event ring,
/// enables the interrupter and starts the controller.
pub fn xhci_config(sc: &XhciSoftc) {
    // Make sure to program a number of device slots we can handle.
    if sc.sc_noslot.get() > USB_MAX_DEVICES as i32 {
        sc.sc_noslot.set(USB_MAX_DEVICES as i32);
    }
    let hcr = xoread4(sc, XHCI_CONFIG) & !XHCI_CONFIG_SLOTS_MASK;
    xowrite4(sc, XHCI_CONFIG, hcr | sc.sc_noslot.get() as u32);

    // Set the device context base array address.
    let paddr = sc.sc_dcbaa.dma.paddr.get() as u64;
    xowrite4(sc, XHCI_DCBAAP_LO, paddr as u32);
    xowrite4(sc, XHCI_DCBAAP_HI, (paddr >> 32) as u32);

    // Set the command ring address.
    let paddr = sc.sc_cmd_ring.dma.paddr.get() as u64;
    xowrite4(sc, XHCI_CRCR_LO, (paddr as u32) | XHCI_CRCR_LO_RCS);
    xowrite4(sc, XHCI_CRCR_HI, (paddr >> 32) as u32);

    // Set the ERST count number to 1, since we use only one event ring.
    xrwrite4(sc, xhci_erstsz(0), xhci_ersts_set(1));

    // Set the segment table address.
    let paddr = sc.sc_erst.dma.paddr.get() as u64;
    xrwrite4(sc, xhci_erstba_lo(0), paddr as u32);
    xrwrite4(sc, xhci_erstba_hi(0), (paddr >> 32) as u32);

    // Set the ring dequeue address.
    let paddr = sc.sc_evt_ring.dma.paddr.get() as u64;
    xrwrite4(sc, xhci_erdp_lo(0), paddr as u32);
    xrwrite4(sc, xhci_erdp_hi(0), (paddr >> 32) as u32);

    // If we successfully saved the state during suspend, restore it here. Otherwise some
    // Intel controllers don't function correctly after resume.
    if sc.sc_saved_state.get() != 0 {
        xowrite4(sc, XHCI_USBCMD, XHCI_CMD_CRS); // Restore state
        let mut hcr = xoread4(sc, XHCI_USBSTS);
        for _ in 0..100 {
            usb_delay_ms(&sc.sc_bus, 1);
            hcr = xoread4(sc, XHCI_USBSTS) & XHCI_STS_RSS;
            if hcr == 0 {
                break;
            }
        }

        if hcr != 0 {
            printf(format_args!("{}: restore state timeout\n", devname(sc)));
        }

        sc.sc_saved_state.set(0);
    }

    // Enable interrupts.
    let hcr = xrread4(sc, xhci_iman(0));
    xrwrite4(sc, xhci_iman(0), hcr | XHCI_IMAN_INTR_ENA);

    // Set default interrupt moderation.
    xrwrite4(sc, xhci_imod(0), XHCI_IMOD_DEFAULT);

    // Allow event interrupt and start the controller.
    xowrite4(sc, XHCI_USBCMD, XHCI_CMD_INTE | XHCI_CMD_RS);
}

/// `xhci_detach`: detaches `usb(4)`, stops the controller and frees its memory.
pub fn xhci_detach(self_: &Device, flags: i32) -> Result<(), Errno> {
    // SAFETY: the front-ends' softcs start with the `XhciSoftc` (`#[repr(C)]`), made for
    // their attachment.
    let sc = unsafe { self_.softc::<XhciSoftc>() };

    if let Err(rv) = config_detach_children(self_, flags) {
        printf(format_args!(
            "{}: error while detaching {}\n",
            devname(sc),
            rv as i32
        ));
        return Err(rv);
    }

    // Since the hardware might already be gone, ignore the errors.
    let _ = xhci_command_abort(sc);

    let _ = xhci_reset(sc);

    // Disable interrupts.
    xrwrite4(sc, xhci_imod(0), 0);
    xrwrite4(sc, xhci_iman(0), 0);

    // Clear the event ring address.
    xrwrite4(sc, xhci_erdp_lo(0), 0);
    xrwrite4(sc, xhci_erdp_hi(0), 0);

    xrwrite4(sc, xhci_erstba_lo(0), 0);
    xrwrite4(sc, xhci_erstba_hi(0), 0);

    xrwrite4(sc, xhci_erstsz(0), 0);

    // Clear the command ring address.
    xowrite4(sc, XHCI_CRCR_LO, 0);
    xowrite4(sc, XHCI_CRCR_HI, 0);

    xowrite4(sc, XHCI_DCBAAP_LO, 0);
    xowrite4(sc, XHCI_DCBAAP_HI, 0);

    if sc.sc_spad.npage.get() > 0 {
        xhci_scratchpad_free(sc);
    }

    usbd_dma_contig_free(&sc.sc_bus, &sc.sc_erst.dma);
    xhci_ring_free(sc, &sc.sc_evt_ring);
    xhci_ring_free(sc, &sc.sc_cmd_ring);
    usbd_dma_contig_free(&sc.sc_bus, &sc.sc_dcbaa.dma);

    Ok(())
}

/// `xhci_activate`: resume reinitialises the controller (polling), power-down suspends it
/// after the children.
pub fn xhci_activate(self_: &Device, act: i32) -> Result<(), Errno> {
    // SAFETY: as in `xhci_detach`.
    let sc = unsafe { self_.softc::<XhciSoftc>() };

    match act {
        DVACT_RESUME => {
            sc.sc_bus.use_polling.set(sc.sc_bus.use_polling.get() + 1);
            xhci_reinit(sc);
            sc.sc_bus.use_polling.set(sc.sc_bus.use_polling.get() - 1);
            config_activate_children(self_, act)
        }
        DVACT_POWERDOWN => {
            let rv = config_activate_children(self_, act);
            xhci_suspend(sc);
            rv
        }
        _ => config_activate_children(self_, act),
    }
}

/// `xhci_reset`: halts the controller and resets it.
pub fn xhci_reset(sc: &XhciSoftc) -> Result<(), Errno> {
    xowrite4(sc, XHCI_USBCMD, 0); // Halt controller
    let mut hcr = 0;
    for _ in 0..100 {
        usb_delay_ms(&sc.sc_bus, 1);
        hcr = xoread4(sc, XHCI_USBSTS) & XHCI_STS_HCH;
        if hcr != 0 {
            break;
        }
    }

    if hcr == 0 {
        printf(format_args!("{}: halt timeout\n", devname(sc)));
    }

    xowrite4(sc, XHCI_USBCMD, XHCI_CMD_HCRST);
    for _ in 0..100 {
        usb_delay_ms(&sc.sc_bus, 1);
        hcr =
            (xoread4(sc, XHCI_USBCMD) & XHCI_CMD_HCRST) | (xoread4(sc, XHCI_USBSTS) & XHCI_STS_CNR);
        if hcr == 0 {
            break;
        }
    }

    if hcr != 0 {
        printf(format_args!("{}: reset timeout\n", devname(sc)));
        return Err(Errno::EIO);
    }

    Ok(())
}

/// `xhci_suspend`: halts the controller, saves its state (unless `XHCI_NOCSS`) and clears
/// the ring and table addresses.
pub fn xhci_suspend(sc: &XhciSoftc) {
    xowrite4(sc, XHCI_USBCMD, 0); // Halt controller
    let mut hcr = 0;
    for _ in 0..100 {
        usb_delay_ms(&sc.sc_bus, 1);
        hcr = xoread4(sc, XHCI_USBSTS) & XHCI_STS_HCH;
        if hcr != 0 {
            break;
        }
    }

    if hcr == 0 {
        printf(format_args!("{}: halt timeout\n", devname(sc)));
        let _ = xhci_reset(sc);
        return;
    }

    // Some Intel controllers will not power down completely unless they have seen a save
    // state command. This in turn will prevent the SoC from reaching its lowest idle state.
    // So save the state here.
    if sc.sc_flags.get() & XHCI_NOCSS == 0 {
        xowrite4(sc, XHCI_USBCMD, XHCI_CMD_CSS); // Save state
        let mut hcr = xoread4(sc, XHCI_USBSTS);
        for _ in 0..100 {
            usb_delay_ms(&sc.sc_bus, 1);
            hcr = xoread4(sc, XHCI_USBSTS) & XHCI_STS_SSS;
            if hcr == 0 {
                break;
            }
        }

        if hcr != 0 {
            printf(format_args!("{}: save state timeout\n", devname(sc)));
            let _ = xhci_reset(sc);
            return;
        }

        sc.sc_saved_state.set(1);
    }

    // Disable interrupts.
    xrwrite4(sc, xhci_imod(0), 0);
    xrwrite4(sc, xhci_iman(0), 0);

    // Clear the event ring address.
    xrwrite4(sc, xhci_erdp_lo(0), 0);
    xrwrite4(sc, xhci_erdp_hi(0), 0);

    xrwrite4(sc, xhci_erstba_lo(0), 0);
    xrwrite4(sc, xhci_erstba_hi(0), 0);

    xrwrite4(sc, xhci_erstsz(0), 0);

    // Clear the command ring address.
    xowrite4(sc, XHCI_CRCR_LO, 0);
    xowrite4(sc, XHCI_CRCR_HI, 0);

    xowrite4(sc, XHCI_DCBAAP_LO, 0);
    xowrite4(sc, XHCI_DCBAAP_HI, 0);
}

/// `xhci_reinit`: resets the controller and its rings and configures it again (resume).
pub fn xhci_reinit(sc: &XhciSoftc) {
    let _ = xhci_reset(sc);
    xhci_ring_reset(sc, &sc.sc_cmd_ring);
    xhci_ring_reset(sc, &sc.sc_evt_ring);

    // Renesas controllers, at least, need more time to resume.
    usb_delay_ms(&sc.sc_bus, USB_RESUME_WAIT);

    xhci_config(sc);
}

/// `xhci_intr`: the hard interrupt handler; `v` is the `XhciSoftc`. Established
/// `IPL_MPSAFE`: it runs without the kernel lock and touches only atomics and registers
/// before scheduling the soft interrupt.
pub fn xhci_intr(v: *mut c_void) -> i32 {
    // SAFETY: the front-end establishes the interrupt with its softc's `XhciSoftc` as the
    // argument and disestablishes it before the softc goes away.
    let sc: &XhciSoftc = unsafe { &*v.cast::<XhciSoftc>() };

    if sc.sc_dead.load(Ordering::Relaxed) != 0 {
        return 0;
    }

    // If we get an interrupt while polling, then just ignore it.
    if sc.sc_bus.use_polling.get() != 0 {
        return 0;
    }

    xhci_intr1(sc)
}

/// `xhci_intr1`: acknowledges the controller's interrupt and schedules the soft interrupt.
pub fn xhci_intr1(sc: &XhciSoftc) -> i32 {
    let intrs = xoread4(sc, XHCI_USBSTS);
    if intrs == 0xffffffff {
        sc.sc_bus.dying.set(1);
        sc.sc_dead.store(1, Ordering::Relaxed);
        return 0;
    }

    if intrs & XHCI_STS_EINT == 0 {
        return 0;
    }

    sc.sc_bus
        .no_intrs
        .set(sc.sc_bus.no_intrs.get().wrapping_add(1));

    if intrs & XHCI_STS_HSE != 0 {
        printf(format_args!("{}: host system error\n", devname(sc)));
        sc.sc_bus.dying.set(1);
        xowrite4(sc, XHCI_USBSTS, intrs);
        return 1;
    }

    // Acknowledge interrupts
    xowrite4(sc, XHCI_USBSTS, intrs);
    let intrs = xrread4(sc, xhci_iman(0));
    xrwrite4(sc, xhci_iman(0), intrs | XHCI_IMAN_INTR_PEND);

    usb_schedsoftintr(&sc.sc_bus);

    1
}

/// `xhci_poll`: the bus's `do_poll`.
pub fn xhci_poll(bus: &'static UsbdBus) {
    let sc = xhci_softc(bus);

    if xoread4(sc, XHCI_USBSTS) != 0 {
        xhci_intr1(sc);
    }
}

/// `xhci_softintr`: the soft interrupt; `v` is the bus.
pub fn xhci_softintr(v: *mut c_void) {
    // SAFETY: `usb_attach` establishes the soft interrupt (and `usb_schedsoftintr` calls it)
    // with the bus as argument; the bus lives in the softc for the controller's life.
    let bus: &'static UsbdBus = unsafe { &*v.cast::<UsbdBus>() };
    let sc = xhci_softc(bus);

    if sc.sc_bus.dying.get() != 0 {
        return;
    }

    sc.sc_bus.intr_context.set(sc.sc_bus.intr_context.get() + 1);
    xhci_event_dequeue(sc);
    sc.sc_bus.intr_context.set(sc.sc_bus.intr_context.get() - 1);
}

/// `xhci_event_dequeue`: handles every event the controller produced, then moves the
/// event ring's dequeue pointer.
pub fn xhci_event_dequeue(sc: &'static XhciSoftc) {
    while let Some(trb) = xhci_ring_consume(sc, &sc.sc_evt_ring) {
        let paddr = trb.paddr();
        let status = trb.status();
        let flags = trb.flags();

        match flags & XHCI_TRB_TYPE_MASK {
            XHCI_EVT_XFER => xhci_event_xfer(sc, paddr, status, flags),
            XHCI_EVT_CMD_COMPLETE => {
                sc.sc_result_trb.set(trb.get());
                xhci_event_command(sc, paddr);
            }
            XHCI_EVT_PORT_CHANGE => xhci_event_port_change(sc, paddr, status),
            XHCI_EVT_HOST_CTRL => {
                // TODO
            }
            _ => {}
        }
    }

    let paddr = sc.sc_evt_ring.deqptr();
    xrwrite4(sc, xhci_erdp_lo(0), (paddr as u32) | XHCI_ERDP_LO_BUSY);
    xrwrite4(sc, xhci_erdp_hi(0), (paddr >> 32) as u32);
}

/// `xhci_skip_all`: completes the xfers queued before a missed service interval.
pub fn xhci_skip_all(xp: &'static XhciPipe) {
    if xp.skip.get() != 0 {
        // Find the last transfer to skip, this is necessary as xhci_xfer_done() posts new
        // transfers which we don't want to skip.
        'done: {
            let Some(mut last) = xp.pipe.queue.first() else {
                break 'done;
            };
            while let Some(xfer) = crate::sys::queue::SimpleqHead::<
                crate::dev::usb::usbdivar::UsbdXferQueue,
            >::next(last)
            {
                last = xfer;
            }

            loop {
                let Some(xfer) = xp.pipe.queue.first() else {
                    break 'done;
                };
                xfer.status.set(USBD_NORMAL_COMPLETION);
                xhci_xfer_done(xfer);
                if ptr::eq(xfer, last) {
                    break;
                }
            }
        }
        xp.skip.set(0);
    }
}

/// `xhci_event_xfer`: a transfer event for the TRB at `paddr` of slot and DCI in `flags`.
pub fn xhci_event_xfer(sc: &'static XhciSoftc, paddr: u64, status: u32, flags: u32) {
    let slot = xhci_trb_get_slot(flags) as u8;
    let dci = xhci_trb_get_ep(flags) as u8;
    if i32::from(slot) > sc.sc_noslot.get() {
        return;
    }

    let Some(xp) = sc
        .sc_sdevs
        .get(usize::from(slot))
        .and_then(|sdev| sdev.pipes.get(usize::from(dci).wrapping_sub(1)))
        .and_then(Cell::get)
    else {
        return;
    };

    let code = xhci_trb_get_code(status);
    let mut remain = xhci_trb_remain(status);

    match code {
        XHCI_CODE_RING_UNDERRUN | XHCI_CODE_RING_OVERRUN => {
            xhci_skip_all(xp);
            return;
        }
        XHCI_CODE_MISSED_SRV => {
            xp.skip.set(1);
            return;
        }
        _ => {}
    }

    let ntrb = xp.ring.ntrb.get();
    let trb_idx = (paddr as i64 - xp.ring.dma.paddr.get() as i64) / size_of::<XhciTrb>() as i64;
    if trb_idx < 0 || trb_idx >= ntrb as i64 {
        printf(format_args!(
            "{}: wrong trb index ({}) max is {}\n",
            devname(sc),
            trb_idx as u32,
            ntrb.wrapping_sub(1)
        ));
        return;
    }
    let trb_idx = trb_idx as usize;

    let Some(xfer) = xp.pending_xfers[trb_idx].get() else {
        return;
    };

    if remain > xfer.length.get() {
        remain = xfer.length.get();
    }

    let xfertype = ue_get_xfertype(xfer.pipe().endpoint().edesc().bmAttributes);

    match xfertype {
        UE_BULK | UE_INTERRUPT | UE_CONTROL => {
            if xhci_event_xfer_generic(sc, xfer, xp, remain, trb_idx, code, slot, dci) {
                return;
            }
        }
        UE_ISOCHRONOUS => {
            if xhci_event_xfer_isoc(xfer, xp, remain, trb_idx, code) {
                return;
            }
        }
        _ => panic(format_args!(
            "xhci_event_xfer: unknown xfer type {xfertype}"
        )),
    }

    xhci_xfer_done(xfer);
}

/// `xhci_xfer_length_generic`: the bytes the xfer's TRBs up to `trb_idx` asked for.
pub fn xhci_xfer_length_generic(xx: &XhciXfer, xp: &XhciPipe, trb_idx: usize) -> u32 {
    let ntrb = xp.ring.ntrb.get() as i64;
    let mut trb0_idx = ((i64::from(xx.index.get()) + ntrb) - xx.ntrb.get() as i64) % (ntrb - 1);
    let mut len: u32 = 0;

    loop {
        let trb = xp.ring.trb(trb0_idx as usize);
        let type_ = trb.flags() & XHCI_TRB_TYPE_MASK;
        if type_ == XHCI_TRB_TYPE_NORMAL || type_ == XHCI_TRB_TYPE_DATA {
            len = len.wrapping_add(xhci_trb_len(trb.status()));
        }
        if trb0_idx as usize == trb_idx {
            break;
        }
        trb0_idx += 1;
        if trb0_idx == ntrb {
            trb0_idx = 0;
        }
    }
    len
}

/// `xhci_event_xfer_generic`: a bulk, interrupt or control transfer event; `true` (the C's
/// 1) when the xfer is not done yet.
#[allow(clippy::too_many_arguments)] // the C's signature
pub fn xhci_event_xfer_generic(
    sc: &'static XhciSoftc,
    xfer: &'static UsbdXfer,
    xp: &'static XhciPipe,
    remain: u32,
    trb_idx: usize,
    code: u32,
    slot: u8,
    dci: u8,
) -> bool {
    let xx = xhci_xfer(xfer);

    let sync_ops = |xfer: &UsbdXfer| {
        if usbd_xfer_isread(xfer) {
            BUS_DMASYNC_POSTREAD
        } else {
            BUS_DMASYNC_POSTWRITE
        }
    };

    match code {
        XHCI_CODE_SUCCESS => {
            if xfer.actlen.get() == 0 {
                if remain != 0 {
                    xfer.actlen
                        .set(xhci_xfer_length_generic(xx, xp, trb_idx).wrapping_sub(remain));
                } else {
                    xfer.actlen.set(xfer.length.get());
                }
            }
            if xfer.actlen.get() != 0 {
                usb_syncmem(
                    &xfer.dmabuf,
                    0,
                    xfer.actlen.get() as BusSize,
                    sync_ops(xfer),
                );
            }
            xfer.status.set(USBD_NORMAL_COMPLETION);
        }
        XHCI_CODE_SHORT_XFER => {
            // Use values from the transfer TRB instead of the status TRB.
            if xfer.actlen.get() == 0 {
                xfer.actlen
                    .set(xhci_xfer_length_generic(xx, xp, trb_idx).wrapping_sub(remain));
            }
            // If this is not the last TRB of a transfer, we should theoretically clear the
            // IOC at the end of the chain but the HC might have already processed it before
            // we had a chance to schedule the softinterrupt.
            if xx.index.get() != trb_idx as i32 {
                return true;
            }
            if xfer.actlen.get() != 0 {
                usb_syncmem(
                    &xfer.dmabuf,
                    0,
                    xfer.actlen.get() as BusSize,
                    sync_ops(xfer),
                );
            }
            xfer.status.set(USBD_NORMAL_COMPLETION);
        }
        XHCI_CODE_TXERR | XHCI_CODE_SPLITERR => {
            xfer.status.set(USBD_IOERROR);
        }
        XHCI_CODE_STALL | XHCI_CODE_BABBLE => {
            // Prevent any timeout to kick in.
            timeout_del(&xfer.timeout_handle);
            usb_rem_task(xfer.device(), &xfer.abort_task);

            // We need to report this condition for umass(4).
            if code == XHCI_CODE_STALL {
                xp.halted.set(USBD_STALLED);
            } else {
                xp.halted.set(USBD_IOERROR);
            }
            // Since the stack might try to start a new transfer as soon as a pending one
            // finishes, make sure the endpoint is fully reset before calling
            // usb_transfer_complete().
            xp.aborted_xfer.set(Some(xfer));
            xhci_cmd_reset_ep_async(sc, slot, dci);
            return true;
        }
        XHCI_CODE_XFER_STOPPED | XHCI_CODE_XFER_STOPINV
            if xp.aborted_xfer.get().is_some_and(|a| ptr::eq(a, xfer)) =>
        {
            // Endpoint stopped while processing a TD.
            return true;
        }
        _ => {
            // XHCI_CODE_XFER_STOPPED, XHCI_CODE_XFER_STOPINV of another xfer fall through.
            xfer.status.set(USBD_IOERROR);
            xp.halted.set(USBD_IN_PROGRESS);
        }
    }

    false
}

/// `xhci_event_xfer_isoc`: an isochronous transfer event; `true` (the C's 1) when the xfer
/// is not done yet.
pub fn xhci_event_xfer_isoc(
    xfer: &'static UsbdXfer,
    xp: &'static XhciPipe,
    remain: u32,
    trb_idx: usize,
    code: u32,
) -> bool {
    let xx = xhci_xfer(xfer);
    let mut frame_idx: i32 = 0;
    let mut skip_trb = false;

    kassert!(xx.index.get() >= 0);

    match code {
        XHCI_CODE_SHORT_XFER => xp.trb_processed[trb_idx].set(TRB_PROCESSED_SHORT),
        _ => xp.trb_processed[trb_idx].set(TRB_PROCESSED_YES),
    }

    let ntrb = xp.ring.ntrb.get() as i64;
    let mut trb0_idx = ((i64::from(xx.index.get()) + ntrb) - xx.ntrb.get() as i64) % (ntrb - 1);

    // Find the according frame index for this TRB.
    while trb0_idx != trb_idx as i64 {
        if xp.ring.trb(trb0_idx as usize).flags() & XHCI_TRB_TYPE_MASK == XHCI_TRB_TYPE_ISOCH {
            frame_idx += 1;
        }
        let was = trb0_idx;
        trb0_idx += 1;
        if was == ntrb - 1 {
            trb0_idx = 0;
        }
    }

    // If we queued two TRBs for a frame and this is the second TRB, check if the first TRB
    // needs accounting since it might not have raised an interrupt in case of full data
    // received.
    if xp.ring.trb(trb_idx).flags() & XHCI_TRB_TYPE_MASK == XHCI_TRB_TYPE_NORMAL {
        frame_idx -= 1;
        let first = if trb_idx == 0 {
            xp.ring.ntrb.get() - 2
        } else {
            trb_idx - 1
        };
        if xp.trb_processed[first].get() == TRB_PROCESSED_NO {
            set_frlength(
                xfer,
                frame_idx,
                xhci_trb_len(xp.ring.trb(first).status()) as u16,
            );
        } else if xp.trb_processed[first].get() == TRB_PROCESSED_SHORT {
            skip_trb = true;
        }
    }

    if !skip_trb {
        let add = xhci_trb_len(xp.ring.trb(trb_idx).status()).wrapping_sub(remain);
        let fl = frlength(xfer, frame_idx).wrapping_add(add as u16);
        set_frlength(xfer, frame_idx, fl);
        xfer.actlen
            .set(xfer.actlen.get().wrapping_add(u32::from(fl)));
    }

    if xx.index.get() != trb_idx as i32 {
        return true;
    }

    if xp.skip.get() != 0 {
        while let Some(skipxfer) = xp.pipe.queue.first() {
            if ptr::eq(skipxfer, xfer) {
                break;
            }
            skipxfer.status.set(USBD_NORMAL_COMPLETION);
            xhci_xfer_done(skipxfer);
        }
        xp.skip.set(0);
    }

    usb_syncmem(
        &xfer.dmabuf,
        0,
        xfer.length.get() as BusSize,
        if usbd_xfer_isread(xfer) {
            BUS_DMASYNC_POSTREAD
        } else {
            BUS_DMASYNC_POSTWRITE
        },
    );
    xfer.status.set(USBD_NORMAL_COMPLETION);

    false
}

/// `xhci_event_command`: a command completion event for the command TRB at `paddr`.
pub fn xhci_event_command(sc: &'static XhciSoftc, paddr: u64) {
    let ring = &sc.sc_cmd_ring;
    let trb_idx = (paddr as i64 - ring.dma.paddr.get() as i64) / size_of::<XhciTrb>() as i64;
    if trb_idx < 0 || trb_idx >= ring.ntrb.get() as i64 {
        printf(format_args!(
            "{}: wrong trb index ({}) max is {}\n",
            devname(sc),
            trb_idx as u32,
            ring.ntrb.get().wrapping_sub(1)
        ));
        return;
    }

    let trb = ring.trb(trb_idx as usize);

    bus_dmamap_sync(
        ring.dma.tag(),
        ring.dma.map(),
        trb.off(),
        size_of::<XhciTrb>(),
        BUS_DMASYNC_POSTREAD | BUS_DMASYNC_POSTWRITE,
    );

    let flags = trb.flags();

    let slot = xhci_trb_get_slot(flags) as usize;
    let dci = xhci_trb_get_ep(flags) as usize;
    let pipe_of = || {
        sc.sc_sdevs
            .get(slot)
            .and_then(|sdev| sdev.pipes.get(dci.wrapping_sub(1)))
            .and_then(Cell::get)
    };

    match flags & XHCI_TRB_TYPE_MASK {
        XHCI_CMD_RESET_EP => {
            let Some(xp) = pipe_of() else {
                return;
            };

            // Update the dequeue pointer past the last TRB.
            xhci_cmd_set_tr_deq_async(
                sc,
                xp.slot.get(),
                xp.dci.get(),
                xp.ring.deqptr() | u64::from(xp.ring.toggle.get()),
            );
        }
        XHCI_CMD_SET_TR_DEQ => {
            let Some(xp) = pipe_of() else {
                return;
            };

            let status = xp.halted.get();
            xp.halted.set(USBD_NORMAL_COMPLETION);
            if let Some(aborted) = xp.aborted_xfer.get() {
                aborted.status.set(status);
                xhci_xfer_done(aborted);
                wakeup(ptr::from_ref(xp));
            }
        }
        XHCI_CMD_CONFIG_EP
        | XHCI_CMD_STOP_EP
        | XHCI_CMD_DISABLE_SLOT
        | XHCI_CMD_ENABLE_SLOT
        | XHCI_CMD_ADDRESS_DEVICE
        | XHCI_CMD_EVAL_CTX
        | XHCI_CMD_NOOP
            if ptr::eq(sc.sc_cmd_trb.get(), trb.as_ptr()) =>
        {
            // All these commands are synchronous.
            //
            // If TRBs differ, this could be a delayed result after we gave up waiting for
            // the expected TRB due to timeout (and nothing is done).
            sc.sc_cmd_trb.set(ptr::null_mut());
            wakeup(ptr::from_ref(&sc.sc_cmd_trb));
        }
        _ => {}
    }
}

/// `xhci_event_port_change`: completes the root hub's interrupt transfer with the bit of
/// the port that changed.
pub fn xhci_event_port_change(sc: &'static XhciSoftc, paddr: u64, status: u32) {
    let port = xhci_trb_portid(paddr) as usize;

    if xhci_trb_get_code(status) != XHCI_CODE_SUCCESS {
        return;
    }

    let Some(xfer) = sc.sc_intrxfer.get() else {
        return;
    };

    // SAFETY: the root hub's interrupt xfer is the controller's while `sc_intrxfer` holds it
    // (`xhci_root_intr_start`), and its buffer is touched only here until it completes.
    let p = unsafe { xfer_buf(xfer, xfer.length.get() as usize) };
    p.fill(0);

    if let Some(b) = p.get_mut(port / 8) {
        *b |= 1 << (port % 8);
    }

    xfer.actlen.set(xfer.length.get());
    xfer.status.set(USBD_NORMAL_COMPLETION);

    usb_transfer_complete(xfer);
}

/// `xhci_xfer_done`: gives the xfer's TRBs back to its ring and completes it.
pub fn xhci_xfer_done(xfer: &'static UsbdXfer) {
    let xp = xhci_pipe(xfer.pipe());
    let xx = xhci_xfer(xfer);

    splsoftassert(IPL_SOFTUSB, "xhci_xfer_done");

    if xp.aborted_xfer.get().is_some_and(|a| ptr::eq(a, xfer)) {
        xp.aborted_xfer.set(None);
    }

    let mut i = xx.index.get() as i64;
    for _ in 0..xx.ntrb.get() {
        if let Some(slot) = usize::try_from(i)
            .ok()
            .and_then(|i| xp.pending_xfers.get(i))
        {
            slot.set(None);
        }
        if i == 0 {
            i = xp.ring.ntrb.get() as i64 - 1;
        }
        i -= 1;
    }
    xp.free_trbs
        .set(xp.free_trbs.get() + xx.ntrb.get() + xx.zerotd.get());
    xx.index.set(-1);
    xx.ntrb.set(0);
    xx.zerotd.set(0);

    timeout_del(&xfer.timeout_handle);
    usb_rem_task(xfer.device(), &xfer.abort_task);
    usb_transfer_complete(xfer);
}

/// `xhci_ed2dci`: the Device Context Index (DCI) of an endpoint, as stated in section 4.5.1
/// of the xHCI specification r1.1.
#[inline]
fn xhci_ed2dci(ed: &UsbEndpointDescriptor) -> u8 {
    if ue_get_xfertype(ed.bmAttributes) == UE_CONTROL {
        return ue_get_addr(ed.bEndpointAddress) * 2 + 1;
    }

    let dir = if ue_get_dir(ed.bEndpointAddress) == UE_DIR_IN {
        1
    } else {
        0
    };

    ue_get_addr(ed.bEndpointAddress) * 2 + dir
}

/// `xhci_pipe_open`: the bus's `open_pipe`; a device's default pipe gets a slot.
pub fn xhci_pipe_open(pipe: &'static UsbdPipe) -> UsbdStatus {
    let sc = xhci_softc(pipe.device().bus());
    let xp = xhci_pipe(pipe);
    let ed = pipe.endpoint().edesc();
    let mut slot: u8 = 0;
    let xfertype = ue_get_xfertype(ed.bmAttributes);

    kassert!(xp.slot.get() == 0);

    if sc.sc_bus.dying.get() != 0 {
        return USBD_IOERROR;
    }

    // Root Hub
    if pipe.device().depth.get() == 0 {
        match ed.bEndpointAddress {
            USB_CONTROL_ENDPOINT => pipe.methods.set(Some(&XHCI_ROOT_CTRL_METHODS)),
            a if a == UE_DIR_IN | XHCI_INTR_ENDPT => {
                pipe.methods.set(Some(&XHCI_ROOT_INTR_METHODS))
            }
            _ => {
                pipe.methods.set(None);
                return USBD_INVAL;
            }
        }
        return USBD_NORMAL_COMPLETION;
    }

    match xfertype {
        UE_CONTROL => {
            pipe.methods.set(Some(&XHCI_DEVICE_CTRL_METHODS));

            // Get a slot and init the device's contexts.
            //
            // Since the control endpoint, represented as the default pipe, is always opened
            // first we are dealing with a new device. Put a new slot in the ENABLED state.
            let error = xhci_cmd_slot_control(sc, &mut slot, true);
            if error.is_err() || slot == 0 || i32::from(slot) > sc.sc_noslot.get() {
                return USBD_INVAL;
            }

            if xhci_softdev_alloc(sc, slot).is_err() {
                let _ = xhci_cmd_slot_control(sc, &mut slot, false);
                return USBD_NOMEM;
            }
        }
        UE_ISOCHRONOUS => pipe.methods.set(Some(&XHCI_DEVICE_ISOC_METHODS)),
        UE_BULK => pipe.methods.set(Some(&XHCI_DEVICE_BULK_METHODS)),
        UE_INTERRUPT => pipe.methods.set(Some(&XHCI_DEVICE_INTR_METHODS)),
        _ => return USBD_INVAL,
    }

    // Our USBD Bus Interface is pipe-oriented but for most of the operations we need to
    // access a device context, so keep track of the slot ID in every pipe.
    if slot == 0 {
        slot = xhci_pipe(default_pipe(pipe.device())).slot.get();
    }

    xp.slot.set(slot);
    xp.dci.set(xhci_ed2dci(&ed));

    if xhci_pipe_init(sc, pipe).is_err() {
        let _ = xhci_cmd_slot_control(sc, &mut slot, false);
        return USBD_IOERROR;
    }

    USBD_NORMAL_COMPLETION
}

/// `dev->default_pipe`. Panics on a device without one (the C's NULL dereference).
fn default_pipe(dev: &'static UsbdDevice) -> &'static UsbdPipe {
    match dev.default_pipe.get() {
        Some(p) => p,
        None => panic(format_args!("xhci: device without a default pipe")),
    }
}

/// `xhci_get_txinfo`: the maximum Endpoint Service Interface Time (ESIT) payload and the
/// average TRB buffer length for an endpoint.
#[inline]
fn xhci_get_txinfo(_sc: &XhciSoftc, pipe: &'static UsbdPipe) -> u32 {
    let ed = pipe.endpoint().edesc();
    let esscd = pipe.endpoint().esscd();
    let mps = ugetw(ed.wMaxPacketSize);

    let (mep, atl): (u32, u32) = match ue_get_xfertype(ed.bmAttributes) {
        UE_CONTROL => (0, 8),
        UE_INTERRUPT | UE_ISOCHRONOUS => match esscd {
            Some(esscd) if pipe.device().speed.get() >= USB_SPEED_SUPER => {
                let mep = u32::from(ugetw(esscd.wBytesPerInterval));
                (mep, mep)
            }
            _ => {
                let mep = (u32::from(ue_get_trans(mps)) + 1) * u32::from(ue_get_size(mps));
                (mep, mep)
            }
        },
        _ => (0, 0), // UE_BULK and default
    };

    xhci_epctx_max_esit_payload(mep) | xhci_epctx_avg_trb_len(atl)
}

/// `xhci_linear_interval`.
#[inline]
fn xhci_linear_interval(ed: &UsbEndpointDescriptor) -> u32 {
    let ival = u32::from(ed.bInterval).clamp(1, 255);

    // fls(ival) - 1
    (u32::BITS - ival.leading_zeros()) - 1
}

/// `xhci_exponential_interval`.
#[inline]
fn xhci_exponential_interval(ed: &UsbEndpointDescriptor) -> u32 {
    let ival = u32::from(ed.bInterval).clamp(1, 16);

    ival - 1
}

/// `xhci_pipe_interval`: the endpoint's interval expressed in 2^(ival) * 125us, see section
/// 6.2.3.6 of xHCI r1.1 Specification.
pub fn xhci_pipe_interval(pipe: &'static UsbdPipe) -> u32 {
    let ed = pipe.endpoint().edesc();
    let speed = pipe.device().speed.get();
    let xfertype = ue_get_xfertype(ed.bmAttributes);

    let ival = if xfertype == UE_CONTROL || xfertype == UE_BULK {
        // Control and Bulk endpoints never NAKs.
        0
    } else {
        match speed {
            // Convert 1-2^(15)ms into 3-18
            USB_SPEED_FULL if xfertype == UE_ISOCHRONOUS => xhci_exponential_interval(&ed) + 3,
            // Convert 1-255ms into 3-10
            USB_SPEED_FULL | USB_SPEED_LOW => xhci_linear_interval(&ed) + 3,
            // USB_SPEED_HIGH, USB_SPEED_SUPER, default: convert 1-2^(15) * 125us into 0-15
            _ => xhci_exponential_interval(&ed),
        }
    };

    kassert!(ival <= 15);
    xhci_epctx_set_ival(ival)
}

/// `xhci_pipe_maxburst`.
pub fn xhci_pipe_maxburst(pipe: &'static UsbdPipe) -> u32 {
    let ed = pipe.endpoint().edesc();
    let esscd = pipe.endpoint().esscd();
    let mps = ugetw(ed.wMaxPacketSize);
    let xfertype = ue_get_xfertype(ed.bmAttributes);
    let mut maxb = 0;

    match pipe.device().speed.get() {
        USB_SPEED_HIGH => {
            if xfertype == UE_ISOCHRONOUS || xfertype == UE_INTERRUPT {
                maxb = u32::from(ue_get_trans(mps));
            }
        }
        USB_SPEED_SUPER => {
            if let Some(esscd) = esscd
                && (xfertype == UE_ISOCHRONOUS || xfertype == UE_INTERRUPT)
            {
                maxb = u32::from(esscd.bMaxBurst);
            }
        }
        _ => {}
    }

    maxb
}

/// `xhci_last_valid_dci`: the last valid Endpoint Context, ignoring `ignore`.
#[inline]
fn xhci_last_valid_dci(
    pipes: &[Cell<Option<&'static XhciPipe>>; 31],
    ignore: Option<&XhciPipe>,
) -> u32 {
    for p in pipes.iter().rev() {
        if let Some(lxp) = p.get()
            && !ignore.is_some_and(|ig| ptr::eq(ig, lxp))
        {
            return xhci_sctx_dci(u32::from(lxp.dci.get()));
        }
    }

    0
}

/// `xhci_context_setup`: fills the input context for the pipe's endpoint and its slot
/// (route string, speed, root hub port, hub and TT fields).
pub fn xhci_context_setup(sc: &XhciSoftc, pipe: &'static UsbdPipe) -> Result<(), Errno> {
    let xp = xhci_pipe(pipe);
    let sdev = sc.sdev(usize::from(xp.slot.get()));
    let ed = pipe.endpoint().edesc();
    let mps = ugetw(ed.wMaxPacketSize);
    let mut xfertype = ue_get_xfertype(ed.bmAttributes);
    let mut cerr = 0;
    let mut route: u32 = 0;
    let dev = pipe.device();

    let myhub = |d: &'static UsbdDevice| match d.myhub.get() {
        Some(h) => h,
        None => panic(format_args!("xhci: device without a hub")),
    };
    let portno = |d: &'static UsbdDevice| match d.powersrc.get() {
        Some(p) => u32::from(p.portno.get()),
        None => panic(format_args!("xhci: device without a port")),
    };

    // Calculate the Route String. Assume that there is no hub with more than 15 ports and
    // that they all have a detph < 6. See section 8.9 of USB 3.1 Specification for more
    // details.
    let mut hub = dev;
    while myhub(hub).depth.get() != 0 {
        let port = portno(hub);
        let depth = u32::from(myhub(hub).depth.get());

        route |= port << (4 * (depth - 1));
        hub = myhub(hub);
    }

    // Get Root Hub port
    let rhport = portno(hub);

    let speed = match dev.speed.get() {
        USB_SPEED_LOW => XHCI_SPEED_LOW,
        USB_SPEED_FULL => XHCI_SPEED_FULL,
        USB_SPEED_HIGH => XHCI_SPEED_HIGH,
        USB_SPEED_SUPER => XHCI_SPEED_SUPER,
        _ => return Err(Errno::EINVAL),
    };

    // Setup the endpoint context
    if xfertype != UE_ISOCHRONOUS {
        cerr = 3;
    }

    if (ed.bEndpointAddress & UE_DIR_IN) != 0 || xfertype == UE_CONTROL {
        xfertype |= 0x4;
    }

    let dci = usize::from(xp.dci.get());
    let info_lo = xhci_pipe_interval(pipe);
    let info_hi = xhci_epctx_set_mps(u32::from(ue_get_size(mps)))
        | xhci_epctx_set_maxb(xhci_pipe_maxburst(pipe))
        | xhci_epctx_set_eptype(u32::from(xfertype))
        | xhci_epctx_set_cerr(cerr);
    let txinfo = xhci_get_txinfo(sc, pipe);
    let deqp = xp.ring.deqptr() | u64::from(xp.ring.toggle.get());
    sdev.ep_ctx_update(dci - 1, |c| {
        c.info_lo = info_lo.to_le();
        c.info_hi = info_hi.to_le();
        c.txinfo = txinfo.to_le();
        c.deqp = deqp.to_le();
    });

    // Unmask the new endpoint
    sdev.input_ctx_update(|c| {
        c.drop_flags = 0;
        c.add_flags = xhci_inctx_mask_dci(u32::from(xp.dci.get())).to_le();
    });

    // Setup the slot context
    let last = xhci_last_valid_dci(&sdev.pipes, None);
    sdev.slot_ctx_update(|c| {
        c.info_lo = (last | xhci_sctx_speed(speed) | xhci_sctx_route(route)).to_le();
        c.info_hi = xhci_sctx_rhport(rhport).to_le();
        c.tt = 0;
        c.state = 0;
    });

    // XXX
    let uhub_is_mtt = |d: &UsbdDevice| d.ddesc.get().bDeviceProtocol == UDPROTO_HSHUBMTT;

    // If we are opening the interrupt pipe of a hub, update its context before putting it in
    // the CONFIGURED state.
    if let Some(h) = dev.hub.get() {
        let nports = h.nports.get() as u32;
        let mtt = uhub_is_mtt(dev);
        let ttthink = u32::from(h.ttthink.get());

        sdev.slot_ctx_update(|c| {
            c.info_lo |= xhci_sctx_hub(1).to_le();
            c.info_hi |= xhci_sctx_nports(nports).to_le();

            if mtt {
                c.info_lo |= xhci_sctx_mtt(1).to_le();
            }

            c.tt |= xhci_sctx_tt_think_time(ttthink).to_le();
        });
    }

    // If this is a Low or Full Speed device below an external High Speed hub, it needs some
    // TT love.
    if speed < XHCI_SPEED_HIGH
        && let Some(hsport) = dev.myhsport.get()
    {
        let hshub = match hsport.parent.get() {
            Some(h) => h,
            None => panic(format_args!("xhci: high speed port without a hub")),
        };
        let slot = u32::from(xhci_pipe(default_pipe(hshub)).slot.get());
        let mtt = uhub_is_mtt(hshub);
        let portno = u32::from(hsport.portno.get());

        sdev.slot_ctx_update(|c| {
            if mtt {
                c.info_lo |= xhci_sctx_mtt(1).to_le();
            }

            c.tt |= (xhci_sctx_tt_hub_sid(slot) | xhci_sctx_tt_port_num(portno)).to_le();
        });
    }

    // Unmask the slot context
    sdev.input_ctx_update(|c| c.add_flags |= xhci_inctx_mask_dci(0).to_le());

    bus_dmamap_sync(
        sdev.ictx_dma.tag(),
        sdev.ictx_dma.map(),
        0,
        sc.sc_pagesize.get() as BusSize,
        BUS_DMASYNC_PREREAD | BUS_DMASYNC_PREWRITE,
    );

    Ok(())
}

/// `xhci_pipe_init`: allocates the pipe's transfer ring and tells the controller about the
/// endpoint (Address Device with BSR for the default pipe, Configure Endpoint otherwise).
pub fn xhci_pipe_init(sc: &XhciSoftc, pipe: &'static UsbdPipe) -> Result<(), Errno> {
    let xp = xhci_pipe(pipe);
    let sdev = sc.sdev(usize::from(xp.slot.get()));

    if xhci_ring_alloc(sc, &xp.ring, XHCI_MAX_XFER, XHCI_XFER_RING_ALIGN).is_err() {
        return Err(Errno::ENOMEM);
    }

    xp.free_trbs.set(xp.ring.ntrb.get());
    xp.halted.set(USBD_NORMAL_COMPLETION);

    sdev.pipes[usize::from(xp.dci.get()) - 1].set(Some(xp));

    xhci_context_setup(sc, pipe)?;

    let error = if xp.dci.get() == 1 {
        // If we are opening the default pipe, the Slot should be in the ENABLED state. Issue
        // an "Address Device" with BSR=1 to put the device in the DEFAULT state. We cannot
        // jump directly to the ADDRESSED state with BSR=0 because some Low/Full speed devices
        // won't accept a SET_ADDRESS command before we've read their device descriptor.
        xhci_cmd_set_address(
            sc,
            xp.slot.get(),
            sdev.ictx_dma.paddr.get() as u64,
            XHCI_TRB_BSR,
        )
    } else {
        xhci_cmd_configure_ep(sc, xp.slot.get(), sdev.ictx_dma.paddr.get() as u64)
    };

    if error.is_err() {
        xhci_ring_free(sc, &xp.ring);
        return Err(Errno::EIO);
    }

    Ok(())
}

/// `xhci_pipe_close`: drops the endpoint from the device's context; closing the default
/// pipe disables the slot.
pub fn xhci_pipe_close(pipe: &'static UsbdPipe) {
    let sc = xhci_softc(pipe.device().bus());
    let xp = xhci_pipe(pipe);

    // Root Hub
    if pipe.device().depth.get() == 0 {
        return;
    }

    let sdev = sc.sdev(usize::from(xp.slot.get()));
    let dci = u32::from(xp.dci.get());

    // Mask the endpoint
    sdev.input_ctx_update(|c| {
        c.drop_flags = xhci_inctx_mask_dci(dci).to_le();
        c.add_flags = 0;
    });

    // Update last valid Endpoint Context
    let last = xhci_last_valid_dci(&sdev.pipes, Some(xp));
    sdev.slot_ctx_update(|c| {
        c.info_lo &= (!xhci_sctx_dci(31)).to_le();
        c.info_lo |= last.to_le();
    });

    // Clear the Endpoint Context
    sdev.ep_ctx_update(dci as usize - 1, |c| *c = XhciEpctx::default());

    bus_dmamap_sync(
        sdev.ictx_dma.tag(),
        sdev.ictx_dma.map(),
        0,
        sc.sc_pagesize.get() as BusSize,
        BUS_DMASYNC_PREREAD | BUS_DMASYNC_PREWRITE,
    );

    // On error the C only prints under XHCI_DEBUG ("error clearing ep").
    let _ = xhci_cmd_configure_ep(sc, xp.slot.get(), sdev.ictx_dma.paddr.get() as u64);

    xhci_ring_free(sc, &xp.ring);
    sdev.pipes[dci as usize - 1].set(None);

    // If we are closing the default pipe, the device is probably gone, so put its slot in
    // the DISABLED state.
    if dci == 1 {
        let mut slot = xp.slot.get();
        let _ = xhci_cmd_slot_control(sc, &mut slot, false);
        xp.slot.set(slot);
        xhci_softdev_free(sc, slot);
    }
}

/// `xhci_setaddr`: the bus's `dev_setaddr`; transitions a device from DEFAULT to ADDRESSED
/// Slot state, a hook needed for Low/Full speed devices (section 4.5.3 of USB 3.1
/// Specification).
pub fn xhci_setaddr(dev: &'static UsbdDevice, _addr: i32) -> Result<(), Errno> {
    let sc = xhci_softc(dev.bus());

    // Root Hub
    if dev.depth.get() == 0 {
        return Ok(());
    }

    let xp = xhci_pipe(default_pipe(dev));
    let sdev = sc.sdev(usize::from(xp.slot.get()));

    kassert!(xp.dci.get() == 1);

    xhci_context_setup(sc, default_pipe(dev))?;

    xhci_cmd_set_address(sc, xp.slot.get(), sdev.ictx_dma.paddr.get() as u64, 0)
}

/// `xhci_allocx`: the bus's `allocx`, a zeroed [`XhciXfer`].
pub fn xhci_allocx(_bus: &'static UsbdBus) -> Option<&'static UsbdXfer> {
    let p = pool_get(&XHCIXFER, PR_NOWAIT | PR_ZERO)?;
    // SAFETY: the pool's items are `size_of::<XhciXfer>()` bytes at the pool's alignment
    // (at least the type's, see the check at the end of the file), zeroed (`PR_ZERO`),
    // which is a valid `XhciXfer`; it lives until `xhci_freex`.
    let xx: &'static XhciXfer = unsafe { &*p.as_ptr().cast::<XhciXfer>() };
    Some(&xx.xfer)
}

/// `xhci_freex`: the bus's `freex`.
///
/// # Safety
///
/// `xfer` came from [`xhci_allocx`] and nothing uses it any more.
pub unsafe fn xhci_freex(_bus: &'static UsbdBus, xfer: NonNull<UsbdXfer>) {
    // The xfer heads its `XhciXfer` (`#[repr(C)]`), the pool item.
    pool_put(&XHCIXFER, xfer.cast());
}

/// `xhci_scratchpad_alloc`: the scratchpad table and its `npage` pages, entry 0 of the
/// DCBAA.
pub fn xhci_scratchpad_alloc(sc: &XhciSoftc, npage: i32) -> Result<(), Errno> {
    let bus = &sc.sc_bus;
    let pagesize = sc.sc_pagesize.get() as BusSize;
    let n = npage as usize;

    // Allocate the required entry for the table.
    if usbd_dma_contig_alloc(
        bus,
        &sc.sc_spad.table_dma,
        n * size_of::<u64>(),
        XHCI_SPAD_TABLE_ALIGN,
        pagesize,
    )
    .is_err()
    {
        return Err(Errno::ENOMEM);
    }

    // Allocate pages. XXX does not need to be contiguous.
    if usbd_dma_contig_alloc(bus, &sc.sc_spad.pages_dma, n * pagesize, pagesize, 0).is_err() {
        usbd_dma_contig_free(bus, &sc.sc_spad.table_dma);
        return Err(Errno::ENOMEM);
    }

    for i in 0..n {
        sc.sc_spad
            .table_dma
            .write_le64(i, (sc.sc_spad.pages_dma.paddr.get() + i * pagesize) as u64);
    }

    bus_dmamap_sync(
        sc.sc_spad.table_dma.tag(),
        sc.sc_spad.table_dma.map(),
        0,
        n * size_of::<u64>(),
        BUS_DMASYNC_PREREAD | BUS_DMASYNC_PREWRITE,
    );

    // Entry 0 points to the table of scratchpad pointers.
    sc.sc_dcbaa
        .set_seg(0, sc.sc_spad.table_dma.paddr.get() as u64);
    bus_dmamap_sync(
        sc.sc_dcbaa.dma.tag(),
        sc.sc_dcbaa.dma.map(),
        0,
        size_of::<u64>(),
        BUS_DMASYNC_PREREAD | BUS_DMASYNC_PREWRITE,
    );

    sc.sc_spad.npage.set(npage);

    Ok(())
}

/// `xhci_scratchpad_free`.
pub fn xhci_scratchpad_free(sc: &XhciSoftc) {
    sc.sc_dcbaa.set_seg(0, 0);
    bus_dmamap_sync(
        sc.sc_dcbaa.dma.tag(),
        sc.sc_dcbaa.dma.map(),
        0,
        size_of::<u64>(),
        BUS_DMASYNC_PREREAD | BUS_DMASYNC_PREWRITE,
    );

    usbd_dma_contig_free(&sc.sc_bus, &sc.sc_spad.pages_dma);
    usbd_dma_contig_free(&sc.sc_bus, &sc.sc_spad.table_dma);
}

/// `xhci_ring_alloc`: a ring of `ntrb` TRBs.
pub fn xhci_ring_alloc(
    sc: &XhciSoftc,
    ring: &XhciRing,
    ntrb: usize,
    alignment: BusSize,
) -> Result<(), Errno> {
    let size = ntrb * size_of::<XhciTrb>();

    let trbs = usbd_dma_contig_alloc(&sc.sc_bus, &ring.dma, size, alignment, XHCI_RING_BOUNDARY)?;
    ring.trbs.set(trbs.as_ptr().cast());

    ring.ntrb.set(ntrb);

    xhci_ring_reset(sc, ring);

    Ok(())
}

/// `xhci_ring_free`.
pub fn xhci_ring_free(sc: &XhciSoftc, ring: &XhciRing) {
    usbd_dma_contig_free(&sc.sc_bus, &ring.dma);
}

/// `xhci_ring_reset`: empties the ring; every ring but the event ring links its last TRB to
/// its first.
pub fn xhci_ring_reset(sc: &XhciSoftc, ring: &XhciRing) {
    let ntrb = ring.ntrb.get();
    let size = ntrb * size_of::<XhciTrb>();

    for i in 0..ntrb {
        let trb = ring.trb(i);
        trb.set_paddr(0);
        trb.set_status(0);
        trb.set_flags(0);
    }

    ring.index.set(0);
    ring.toggle.set(XHCI_TRB_CYCLE);

    // Since all our rings use only one segment, at least for the moment, link their tail to
    // their head.
    if !ptr::eq(ring, &sc.sc_evt_ring) {
        let trb = ring.trb(ntrb - 1);

        trb.set_paddr(ring.dma.paddr.get() as u64);
        trb.set_flags(XHCI_TRB_TYPE_LINK | XHCI_TRB_LINKSEG | XHCI_TRB_CYCLE);
        bus_dmamap_sync(
            ring.dma.tag(),
            ring.dma.map(),
            0,
            size,
            BUS_DMASYNC_PREWRITE,
        );
    } else {
        bus_dmamap_sync(
            ring.dma.tag(),
            ring.dma.map(),
            0,
            size,
            BUS_DMASYNC_PREREAD | BUS_DMASYNC_PREWRITE,
        );
    }
}

/// `xhci_ring_consume`: the next TRB the controller produced, `None` when it has not.
pub fn xhci_ring_consume(_sc: &XhciSoftc, ring: &XhciRing) -> Option<XhciTrbRef> {
    let trb = ring.trb(ring.index.get() as usize);

    kassert!((ring.index.get() as usize) < ring.ntrb.get());

    bus_dmamap_sync(
        ring.dma.tag(),
        ring.dma.map(),
        trb.off(),
        size_of::<XhciTrb>(),
        BUS_DMASYNC_POSTREAD,
    );

    // Make sure this TRB can be consumed.
    if ring.toggle.get() != (trb.flags() & XHCI_TRB_CYCLE) {
        return None;
    }

    ring.index.set(ring.index.get() + 1);

    if ring.index.get() as usize == ring.ntrb.get() {
        ring.index.set(0);
        ring.toggle.set(ring.toggle.get() ^ 1);
    }

    Some(trb)
}

/// `xhci_ring_produce`: the next free TRB of a ring the driver produces; wraps through the
/// link TRB, carrying the chain bit and flipping its cycle bit.
pub fn xhci_ring_produce(_sc: &XhciSoftc, ring: &XhciRing) -> XhciTrbRef {
    kassert!((ring.index.get() as usize) < ring.ntrb.get());

    let ntrb = ring.ntrb.get();
    let sync = |trb: XhciTrbRef, ops: i32| {
        bus_dmamap_sync(
            ring.dma.tag(),
            ring.dma.map(),
            trb.off(),
            size_of::<XhciTrb>(),
            ops,
        );
    };

    // Setup the link TRB after the previous TRB is done.
    if ring.index.get() == 0 {
        let lnk = ring.trb(ntrb - 1);
        let trb = ring.trb(ntrb - 2);

        sync(lnk, BUS_DMASYNC_POSTREAD | BUS_DMASYNC_POSTWRITE);

        lnk.set_flags(lnk.flags() & !XHCI_TRB_CHAIN);
        if trb.flags() & XHCI_TRB_CHAIN != 0 {
            lnk.set_flags(lnk.flags() | XHCI_TRB_CHAIN);
        }

        sync(lnk, BUS_DMASYNC_PREWRITE);

        lnk.set_flags(lnk.flags() ^ XHCI_TRB_CYCLE);

        sync(lnk, BUS_DMASYNC_PREWRITE);
    }

    let trb = ring.trb(ring.index.get() as usize);
    ring.index.set(ring.index.get() + 1);
    sync(trb, BUS_DMASYNC_POSTREAD | BUS_DMASYNC_POSTWRITE);

    // Toggle cycle state of the link TRB and skip it.
    if ring.index.get() as usize == ntrb - 1 {
        ring.index.set(0);
        ring.toggle.set(ring.toggle.get() ^ 1);
    }

    trb
}

/// `xhci_xfer_get_trb`: the xfer's next TRB on its pipe's ring and the ring's toggle before
/// it; `last` is -1 for a zero-length TD, 0 inside a chain and 1 at its end.
pub fn xhci_xfer_get_trb(sc: &XhciSoftc, xfer: &'static UsbdXfer, last: i32) -> (XhciTrbRef, u32) {
    let xp = xhci_pipe(xfer.pipe());
    let xx = xhci_xfer(xfer);

    kassert!(xp.free_trbs.get() >= 1);
    xp.free_trbs.set(xp.free_trbs.get().wrapping_sub(1));
    let toggle = xp.ring.toggle.get();

    let idx = xp.ring.index.get() as usize;
    match last {
        -1 => {
            // This will be a zero-length TD.
            xp.pending_xfers[idx].set(None);
            xx.zerotd.set(xx.zerotd.get() + 1);
        }
        0 => {
            // This will be in a chain.
            xp.pending_xfers[idx].set(Some(xfer));
            xx.index.set(-2);
            xx.ntrb.set(xx.ntrb.get() + 1);
        }
        1 => {
            // This will terminate a chain.
            xp.pending_xfers[idx].set(Some(xfer));
            xx.index.set(idx as i32);
            xx.ntrb.set(xx.ntrb.get() + 1);
        }
        _ => {}
    }

    xp.trb_processed[idx].set(TRB_PROCESSED_NO);

    (xhci_ring_produce(sc, &xp.ring), toggle)
}

/// `xhci_command_submit`: puts `trb0` on the command ring and rings the doorbell; with a
/// `timeout` (ns), waits for its completion event and copies it into `trb0`.
pub fn xhci_command_submit(sc: &XhciSoftc, trb0: &mut XhciTrb, timeout: u64) -> Result<(), Errno> {
    kassert!(timeout == 0 || sc.sc_cmd_trb.get().is_null());

    trb0.trb_flags |= sc.sc_cmd_ring.toggle.get().to_le();

    let ring = &sc.sc_cmd_ring;
    let trb = xhci_ring_produce(sc, ring);
    trb.set_paddr_raw(trb0.trb_paddr);
    trb.set_status_raw(trb0.trb_status);
    bus_dmamap_sync(
        ring.dma.tag(),
        ring.dma.map(),
        trb.off(),
        size_of::<XhciTrb>(),
        BUS_DMASYNC_PREWRITE,
    );

    trb.set_flags_raw(trb0.trb_flags);
    bus_dmamap_sync(
        ring.dma.tag(),
        ring.dma.map(),
        trb.off(),
        size_of::<XhciTrb>(),
        BUS_DMASYNC_PREWRITE,
    );

    if timeout == 0 {
        xdwrite4(sc, xhci_doorbell(0), 0);
        return Ok(());
    }

    rw_assert_wrlock(&sc.sc_cmd_lock);

    let s = splusb();
    sc.sc_cmd_trb.set(trb.as_ptr());
    xdwrite4(sc, xhci_doorbell(0), 0);
    if let Err(error) = tsleep_nsec(ptr::from_ref(&sc.sc_cmd_trb), PZERO, "xhcicmd", timeout) {
        kassert!(ptr::eq(sc.sc_cmd_trb.get(), trb.as_ptr()) || sc.sc_cmd_trb.get().is_null());
        // Just because the timeout expired this does not mean that the TRB isn't active
        // anymore! We could get an interrupt from this TRB later on and then wonder what to
        // do with it. We'd rather abort it.
        let _ = xhci_command_abort(sc);
        sc.sc_cmd_trb.set(ptr::null_mut());
        splx(s);
        return Err(error);
    }
    splx(s);

    *trb0 = sc.sc_result_trb.get();

    if xhci_trb_get_code(u32::from_le(trb0.trb_status)) == XHCI_CODE_SUCCESS {
        return Ok(());
    }

    Err(Errno::EIO)
}

/// `xhci_command_abort`: aborts the running command; `false` (the C's 1) when the ring did
/// not stop in time.
pub fn xhci_command_abort(sc: &XhciSoftc) -> bool {
    let reg = xoread4(sc, XHCI_CRCR_LO);
    if reg & XHCI_CRCR_LO_CRR == 0 {
        return true;
    }

    xowrite4(sc, XHCI_CRCR_LO, reg | XHCI_CRCR_LO_CA);
    xowrite4(sc, XHCI_CRCR_HI, 0);

    let mut reg = reg;
    for _ in 0..2500 {
        delay(100);
        reg = xoread4(sc, XHCI_CRCR_LO) & XHCI_CRCR_LO_CRR;
        if reg == 0 {
            break;
        }
    }

    if reg != 0 {
        printf(format_args!(
            "{}: command ring abort timeout\n",
            devname(sc)
        ));
        return false;
    }

    true
}

/// A command TRB (`struct xhci_trb trb` on the stack), members in the controller's order.
fn cmd_trb(paddr: u64, flags: u32) -> XhciTrb {
    XhciTrb {
        trb_paddr: paddr.to_le(),
        trb_status: 0,
        trb_flags: flags.to_le(),
    }
}

/// A synchronous command under `sc_cmd_lock`.
fn xhci_command_sync(sc: &XhciSoftc, trb: &mut XhciTrb) -> Result<(), Errno> {
    rw_enter_write(&sc.sc_cmd_lock);
    let error = xhci_command_submit(sc, trb, XHCI_CMD_TIMEOUT);
    rw_exit_write(&sc.sc_cmd_lock);
    error
}

/// `xhci_cmd_configure_ep`: Configure Endpoint with the input context at `addr`.
pub fn xhci_cmd_configure_ep(sc: &XhciSoftc, slot: u8, addr: u64) -> Result<(), Errno> {
    let mut trb = cmd_trb(
        addr,
        xhci_trb_set_slot(u32::from(slot)) | XHCI_CMD_CONFIG_EP,
    );

    xhci_command_sync(sc, &mut trb)
}

/// `xhci_cmd_stop_ep`: Stop Endpoint.
pub fn xhci_cmd_stop_ep(sc: &XhciSoftc, slot: u8, dci: u8) -> Result<(), Errno> {
    let mut trb = cmd_trb(
        0,
        xhci_trb_set_slot(u32::from(slot)) | xhci_trb_set_ep(u32::from(dci)) | XHCI_CMD_STOP_EP,
    );

    xhci_command_sync(sc, &mut trb)
}

/// `xhci_cmd_reset_ep_async`: Reset Endpoint, without waiting (completed by
/// `xhci_event_command`).
pub fn xhci_cmd_reset_ep_async(sc: &XhciSoftc, slot: u8, dci: u8) {
    let mut trb = cmd_trb(
        0,
        xhci_trb_set_slot(u32::from(slot)) | xhci_trb_set_ep(u32::from(dci)) | XHCI_CMD_RESET_EP,
    );

    let _ = xhci_command_submit(sc, &mut trb, 0);
}

/// `xhci_cmd_set_tr_deq_async`: Set TR Dequeue Pointer, without waiting.
pub fn xhci_cmd_set_tr_deq_async(sc: &XhciSoftc, slot: u8, dci: u8, addr: u64) {
    let mut trb = cmd_trb(
        addr,
        xhci_trb_set_slot(u32::from(slot)) | xhci_trb_set_ep(u32::from(dci)) | XHCI_CMD_SET_TR_DEQ,
    );

    let _ = xhci_command_submit(sc, &mut trb, 0);
}

/// `xhci_cmd_slot_control`: Enable Slot (`*slotp` gets the slot) or Disable Slot
/// `*slotp`.
pub fn xhci_cmd_slot_control(sc: &XhciSoftc, slotp: &mut u8, enable: bool) -> Result<(), Errno> {
    let mut trb = if enable {
        cmd_trb(0, XHCI_CMD_ENABLE_SLOT)
    } else {
        cmd_trb(
            0,
            xhci_trb_set_slot(u32::from(*slotp)) | XHCI_CMD_DISABLE_SLOT,
        )
    };

    if xhci_command_sync(sc, &mut trb).is_err() {
        return Err(Errno::EIO);
    }

    if enable {
        *slotp = xhci_trb_get_slot(u32::from_le(trb.trb_flags)) as u8;
    }

    Ok(())
}

/// `xhci_cmd_set_address`: Address Device with the input context at `addr`; `bsr` is
/// `XHCI_TRB_BSR` or 0.
pub fn xhci_cmd_set_address(sc: &XhciSoftc, slot: u8, addr: u64, bsr: u32) -> Result<(), Errno> {
    let mut trb = cmd_trb(
        addr,
        xhci_trb_set_slot(u32::from(slot)) | XHCI_CMD_ADDRESS_DEVICE | bsr,
    );

    xhci_command_sync(sc, &mut trb)
}

/// `xhci_softdev_alloc`: the slot's input and output contexts; the DCBAA points at the
/// output context.
pub fn xhci_softdev_alloc(sc: &XhciSoftc, slot: u8) -> Result<(), Errno> {
    let sdev = sc.sdev(usize::from(slot));
    let pagesize = sc.sc_pagesize.get() as BusSize;
    let ctxsize = sc.sc_ctxsize.get() as usize;

    // Setup input context. Even with 64 byte context size, it fits into the smallest
    // supported page size, so use that.
    let Ok(kva) = usbd_dma_contig_alloc(
        &sc.sc_bus,
        &sdev.ictx_dma,
        pagesize,
        XHCI_ICTX_ALIGN,
        pagesize,
    ) else {
        return Err(Errno::ENOMEM);
    };
    let kva = kva.as_ptr();

    sdev.input_ctx.set(kva.cast());
    sdev.slot_ctx.set(kva.wrapping_add(ctxsize).cast());
    for (i, ep) in sdev.ep_ctx.iter().enumerate() {
        ep.set(kva.wrapping_add((i + 2) * ctxsize).cast());
    }

    // Setup output context
    if usbd_dma_contig_alloc(
        &sc.sc_bus,
        &sdev.octx_dma,
        pagesize,
        XHCI_OCTX_ALIGN,
        pagesize,
    )
    .is_err()
    {
        usbd_dma_contig_free(&sc.sc_bus, &sdev.ictx_dma);
        return Err(Errno::ENOMEM);
    }

    for p in &sdev.pipes {
        p.set(None);
    }

    sc.sc_dcbaa
        .set_seg(usize::from(slot), sdev.octx_dma.paddr.get() as u64);
    bus_dmamap_sync(
        sc.sc_dcbaa.dma.tag(),
        sc.sc_dcbaa.dma.map(),
        usize::from(slot) * size_of::<u64>(),
        size_of::<u64>(),
        BUS_DMASYNC_PREREAD | BUS_DMASYNC_PREWRITE,
    );

    Ok(())
}

/// `xhci_softdev_free`.
pub fn xhci_softdev_free(sc: &XhciSoftc, slot: u8) {
    let sdev = sc.sdev(usize::from(slot));

    sc.sc_dcbaa.set_seg(usize::from(slot), 0);
    bus_dmamap_sync(
        sc.sc_dcbaa.dma.tag(),
        sc.sc_dcbaa.dma.map(),
        usize::from(slot) * size_of::<u64>(),
        size_of::<u64>(),
        BUS_DMASYNC_PREREAD | BUS_DMASYNC_PREWRITE,
    );

    usbd_dma_contig_free(&sc.sc_bus, &sdev.octx_dma);
    usbd_dma_contig_free(&sc.sc_bus, &sdev.ictx_dma);

    sdev.clear();
}

/// `xhci_abort_xfer`: stops the endpoint and moves its dequeue pointer past the xfer, which
/// `xhci_event_command` then completes with `status`.
pub fn xhci_abort_xfer(xfer: &'static UsbdXfer, status: UsbdStatus) {
    let sc = xhci_softc(xfer.device().bus());
    let xp = xhci_pipe(xfer.pipe());

    splsoftassert(IPL_SOFTUSB, "xhci_abort_xfer");

    // XXX The stack should not call abort() in this case.
    if sc.sc_bus.dying.get() != 0 || xfer.status.get() == USBD_NOT_STARTED {
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

    // Prevent any timeout to kick in.
    timeout_del(&xfer.timeout_handle);
    usb_rem_task(xfer.device(), &xfer.abort_task);

    // Indicate that we are aborting this transfer.
    xp.halted.set(status);
    xp.aborted_xfer.set(Some(xfer));

    // Stop the endpoint and wait until the hardware says so.
    if xhci_cmd_stop_ep(sc, xp.slot.get(), xp.dci.get()).is_err() {
        // Assume the device is gone.
        xp.halted.set(USBD_NORMAL_COMPLETION);
        xp.aborted_xfer.set(None);
        xfer.status.set(status);
        usb_transfer_complete(xfer);
        return;
    }

    // The transfer was already completed when we stopped the endpoint, no need to move the
    // dequeue pointer past its TRBs.
    if xp.aborted_xfer.get().is_none() {
        xp.halted.set(USBD_NORMAL_COMPLETION);
        return;
    }

    // At this stage the endpoint has been stopped, so update its dequeue pointer past the
    // last TRB of the transfer.
    //
    // Note: This assumes that only one transfer per endpoint has pending TRBs on the ring.
    xhci_cmd_set_tr_deq_async(
        sc,
        xp.slot.get(),
        xp.dci.get(),
        xp.ring.deqptr() | u64::from(xp.ring.toggle.get()),
    );
    if tsleep_nsec(ptr::from_ref(xp), PZERO, "xhciab", XHCI_CMD_TIMEOUT).is_err() {
        printf(format_args!("{}: timeout aborting transfer\n", devname(sc)));
    }
}

/// `xhci_timeout`: an xfer's timeout; the abort runs in the USB abort task thread.
pub fn xhci_timeout(addr: *mut c_void) {
    // SAFETY: the timeout's argument is its xfer (`xhci_device_*_start`), which the stack
    // keeps until the xfer completes, and completion deletes the timeout.
    let xfer: &'static UsbdXfer = unsafe { &*addr.cast::<UsbdXfer>() };
    let sc = xhci_softc(xfer.device().bus());

    if sc.sc_bus.dying.get() != 0 {
        xhci_timeout_task(addr);
        return;
    }

    usb_init_task(
        &xfer.abort_task,
        xhci_timeout_task,
        addr,
        USB_TASK_TYPE_ABORT,
    );
    usb_add_task(xfer.device(), &xfer.abort_task);
}

/// `xhci_timeout_task`.
pub fn xhci_timeout_task(addr: *mut c_void) {
    // SAFETY: as in `xhci_timeout`; completion removes the task.
    let xfer: &'static UsbdXfer = unsafe { &*addr.cast::<UsbdXfer>() };

    let s = splusb();
    xhci_abort_xfer(xfer, USBD_TIMEOUT);
    splx(s);
}

/// `xhci_root_ctrl_transfer`.
pub fn xhci_root_ctrl_transfer(xfer: &'static UsbdXfer) -> UsbdStatus {
    let err = usb_insert_transfer(xfer);
    if err.is_err() {
        return err;
    }

    xhci_root_ctrl_start(queue_first(xfer.pipe()))
}

/// `xhci_root_ctrl_start`: answers a request to the emulated root hub from the port
/// registers and completes it at once.
pub fn xhci_root_ctrl_start(xfer: &'static UsbdXfer) -> UsbdStatus {
    let sc = xhci_softc(xfer.device().bus());

    kassert!(xfer.rqflags.get() & URQ_REQUEST != 0);

    if sc.sc_bus.dying.get() != 0 {
        return USBD_IOERROR;
    }

    let req = xfer.request.get();

    let len = usize::from(ugetw(req.wLength));
    let value = ugetw(req.wValue);
    let mut index = ugetw(req.wIndex);

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
                    let mut devd = XHCI_DEVD;
                    usetw(&mut devd.idVendor, sc.sc_id_vendor.get() as u16);
                    let l = len.min(USB_DEVICE_DESCRIPTOR_SIZE);
                    totlen = l;
                    buf[..l].copy_from_slice(&devd.as_bytes()[..l]);
                }
                // We can't really operate at another speed, but the spec says we need this
                // descriptor.
                UDESC_OTHER_SPEED_CONFIGURATION | UDESC_CONFIG => {
                    if value & 0xff != 0 {
                        break 'ret Err(USBD_IOERROR);
                    }
                    let mut confd = XHCI_CONFD;
                    confd.bDescriptorType = (value >> 8) as u8;
                    let parts: [&[u8]; 3] = [
                        &confd.as_bytes()[..USB_CONFIG_DESCRIPTOR_SIZE],
                        &XHCI_IFCD.as_bytes()[..USB_INTERFACE_DESCRIPTOR_SIZE],
                        &XHCI_ENDPD.as_bytes()[..USB_ENDPOINT_DESCRIPTOR_SIZE],
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
                        2 => Some(b"xHCI root hub"), // Product
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
                let port = xhci_portsc(usize::from(index));
                let v = xoread4(sc, port) & !XHCI_PS_CLEAR;
                match i32::from(value) {
                    UHF_PORT_ENABLE => xowrite4(sc, port, v | XHCI_PS_PED),
                    UHF_PORT_SUSPEND => {
                        // TODO
                    }
                    UHF_PORT_POWER => xowrite4(sc, port, v & !XHCI_PS_PP),
                    UHF_PORT_INDICATOR => xowrite4(sc, port, v & !xhci_ps_set_pic(3)),
                    UHF_C_PORT_CONNECTION => xowrite4(sc, port, v | XHCI_PS_CSC),
                    UHF_C_PORT_ENABLE => xowrite4(sc, port, v | XHCI_PS_PEC),
                    UHF_C_PORT_SUSPEND | UHF_C_PORT_LINK_STATE => {
                        xowrite4(sc, port, v | XHCI_PS_PLC)
                    }
                    UHF_C_PORT_OVER_CURRENT => xowrite4(sc, port, v | XHCI_PS_OCC),
                    UHF_C_PORT_RESET => xowrite4(sc, port, v | XHCI_PS_PRC),
                    UHF_C_BH_PORT_RESET => xowrite4(sc, port, v | XHCI_PS_WRC),
                    _ => break 'ret Err(USBD_IOERROR),
                }
            }
            (UR_GET_DESCRIPTOR, UT_READ_CLASS_DEVICE) => {
                if len == 0 {
                    break 'ret Ok(0);
                }
                if value & 0xff != 0 {
                    break 'ret Err(USBD_IOERROR);
                }
                let v = xread4(sc, XHCI_HCCPARAMS);
                let mut hubd = XHCI_HUBD;
                hubd.bNbrPorts = sc.sc_noport.get() as u8;
                usetw(
                    &mut hubd.wHubCharacteristics,
                    (if xhci_hcc_ppc(v) != 0 {
                        UHD_PWR_INDIVIDUAL
                    } else {
                        UHD_PWR_GANGED
                    }) | (if xhci_hcc_pind(v) != 0 {
                        UHD_PORT_IND
                    } else {
                        0
                    }),
                );
                hubd.bPwrOn2PwrGood = 10; // xHCI section 5.4.9
                let mut i = 1;
                while i <= sc.sc_noport.get() {
                    let v = xoread4(sc, xhci_portsc(i as usize));
                    if v & XHCI_PS_DR != 0
                        && let Some(b) = hubd.DeviceRemovable.get_mut(i as usize / 8)
                    {
                        *b |= 1 << (i % 8);
                    }
                    i += 1;
                }
                hubd.bDescLength = (USB_HUB_DESCRIPTOR_SIZE as i32 + i) as u8;
                let l = len
                    .min(usize::from(hubd.bDescLength))
                    .min(size_of::<UsbHubDescriptor>());
                totlen = l;
                buf[..l].copy_from_slice(&hubd.as_bytes()[..l]);
            }
            (UR_GET_STATUS, UT_READ_CLASS_DEVICE) => {
                if len != 16 {
                    break 'ret Err(USBD_IOERROR);
                }
                buf.fill(0);
                totlen = len;
            }
            (UR_GET_STATUS, UT_READ_CLASS_OTHER) => {
                if index < 1 || i32::from(index) > sc.sc_noport.get() {
                    break 'ret Err(USBD_IOERROR);
                }
                if len != 4 {
                    break 'ret Err(USBD_IOERROR);
                }
                let v = xoread4(sc, xhci_portsc(usize::from(index)));
                let mut i = ups_port_ls_set(xhci_ps_get_pls(v) as u16);
                match xhci_ps_speed(v) {
                    XHCI_SPEED_FULL => i |= UPS_FULL_SPEED,
                    XHCI_SPEED_LOW => i |= UPS_LOW_SPEED,
                    XHCI_SPEED_HIGH => i |= UPS_HIGH_SPEED,
                    _ => {} // XHCI_SPEED_SUPER, default
                }
                if v & XHCI_PS_CCS != 0 {
                    i |= UPS_CURRENT_CONNECT_STATUS;
                }
                if v & XHCI_PS_PED != 0 {
                    i |= UPS_PORT_ENABLED;
                }
                if v & XHCI_PS_OCA != 0 {
                    i |= UPS_OVERCURRENT_INDICATOR;
                }
                if v & XHCI_PS_PR != 0 {
                    i |= UPS_RESET;
                }
                if v & XHCI_PS_PP != 0 {
                    if xhci_ps_speed(v) >= XHCI_SPEED_FULL && xhci_ps_speed(v) <= XHCI_SPEED_HIGH {
                        i |= UPS_PORT_POWER;
                    } else {
                        i |= UPS_PORT_POWER_SS;
                    }
                }
                let mut ps = UsbPortStatus::zeroed();
                usetw(&mut ps.wPortStatus, i);
                let mut i = 0;
                if v & XHCI_PS_CSC != 0 {
                    i |= UPS_C_CONNECT_STATUS;
                }
                if v & XHCI_PS_PEC != 0 {
                    i |= UPS_C_PORT_ENABLED;
                }
                if v & XHCI_PS_OCC != 0 {
                    i |= UPS_C_OVERCURRENT_INDICATOR;
                }
                if v & XHCI_PS_PRC != 0 {
                    i |= UPS_C_PORT_RESET;
                }
                if v & XHCI_PS_WRC != 0 {
                    i |= UPS_C_BH_PORT_RESET;
                }
                if v & XHCI_PS_PLC != 0 {
                    i |= UPS_C_PORT_LINK_STATE;
                }
                if v & XHCI_PS_CEC != 0 {
                    i |= UPS_C_PORT_CONFIG_ERROR;
                }
                usetw(&mut ps.wPortChange, i);
                let l = len.min(size_of::<UsbPortStatus>());
                buf[..l].copy_from_slice(&ps.as_bytes()[..l]);
                totlen = l;
            }
            (UR_SET_DESCRIPTOR, UT_WRITE_CLASS_DEVICE) => break 'ret Err(USBD_IOERROR),
            (UR_SET_FEATURE, UT_WRITE_CLASS_DEVICE) => {}
            (UR_SET_FEATURE, UT_WRITE_CLASS_OTHER) => {
                let i = index >> 8;
                index &= 0x00ff;

                if index < 1 || i32::from(index) > sc.sc_noport.get() {
                    break 'ret Err(USBD_IOERROR);
                }
                let port = xhci_portsc(usize::from(index));
                let mut v = xoread4(sc, port) & !XHCI_PS_CLEAR;

                match i32::from(value) {
                    UHF_PORT_ENABLE => xowrite4(sc, port, v | XHCI_PS_PED),
                    UHF_PORT_SUSPEND => {
                        if xhci_ps_speed(v) == XHCI_SPEED_SUPER {
                            break 'ret Err(USBD_IOERROR);
                        }
                        xowrite4(
                            sc,
                            port,
                            v | xhci_ps_set_pls(if i != 0 {
                                2 /* LPM */
                            } else {
                                3
                            }) | XHCI_PS_LWS,
                        );
                    }
                    UHF_PORT_RESET => xowrite4(sc, port, v | XHCI_PS_PR),
                    UHF_PORT_POWER => xowrite4(sc, port, v | XHCI_PS_PP),
                    UHF_PORT_INDICATOR => {
                        v &= !xhci_ps_set_pic(3);
                        v |= xhci_ps_set_pic(1);

                        xowrite4(sc, port, v);
                    }
                    UHF_C_PORT_RESET => xowrite4(sc, port, v | XHCI_PS_PRC),
                    UHF_C_BH_PORT_RESET => xowrite4(sc, port, v | XHCI_PS_WRC),
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

/// `xhci_noop`: the methods the root hub's control pipe does not need.
pub fn xhci_noop(_xfer: &'static UsbdXfer) {}

/// `xhci_root_intr_transfer`.
pub fn xhci_root_intr_transfer(xfer: &'static UsbdXfer) -> UsbdStatus {
    let err = usb_insert_transfer(xfer);
    if err.is_err() {
        return err;
    }

    xhci_root_intr_start(queue_first(xfer.pipe()))
}

/// `xhci_root_intr_start`: the root hub's interrupt transfer waits for a port change event.
pub fn xhci_root_intr_start(xfer: &'static UsbdXfer) -> UsbdStatus {
    let sc = xhci_softc(xfer.device().bus());

    if sc.sc_bus.dying.get() != 0 {
        return USBD_IOERROR;
    }

    sc.sc_intrxfer.set(Some(xfer));

    USBD_IN_PROGRESS
}

/// `xhci_root_intr_abort`.
pub fn xhci_root_intr_abort(xfer: &'static UsbdXfer) {
    let sc = xhci_softc(xfer.device().bus());

    sc.sc_intrxfer.set(None);

    xfer.status.set(USBD_CANCELLED);
    let s = splusb();
    usb_transfer_complete(xfer);
    splx(s);
}

/// `xhci_root_intr_done`.
pub fn xhci_root_intr_done(_xfer: &'static UsbdXfer) {}

/// `xhci_xfer_tdsize`: the number of packets remaining in the TD after the corresponding
/// TRB (section 4.11.2.4 of xHCI specification r1.1).
#[inline]
fn xhci_xfer_tdsize(xfer: &'static UsbdXfer, remain: u32, len: u32) -> u32 {
    let mps = ugetw(xfer.pipe().endpoint().edesc().wMaxPacketSize);

    if len == 0 {
        return xhci_trb_tdrem(0);
    }

    let mut npkt = howmany((remain - len) as usize, usize::from(ue_get_size(mps))) as u32;
    if npkt > 31 {
        npkt = 31;
    }

    xhci_trb_tdrem(npkt)
}

/// `xhci_xfer_tbc`: the Transfer Burst Count (TBC) and the Transfer Last Burst Packet Count
/// (TLBPC) (section 4.11.2.3 of xHCI specification r1.1).
#[inline]
fn xhci_xfer_tbc(xfer: &'static UsbdXfer, len: u32) -> (u32, u32) {
    let mps = ugetw(xfer.pipe().endpoint().edesc().wMaxPacketSize);

    // Transfer Descriptor Packet Count, section 4.14.1.
    let mut tdpc = howmany(len as usize, usize::from(ue_get_size(mps))) as u32;
    if tdpc == 0 {
        tdpc = 1;
    }

    // Transfer Burst Count
    let maxb = xhci_pipe_maxburst(xfer.pipe());
    let tbc = howmany(tdpc as usize, maxb as usize + 1) as u32 - 1;

    // Transfer Last Burst Packet Count
    let tlbpc = if xfer.device().speed.get() == USB_SPEED_SUPER {
        let residue = tdpc % (maxb + 1);
        if residue == 0 { maxb } else { residue - 1 }
    } else {
        tdpc - 1
    };

    (tbc, tlbpc)
}

/// `xhci_device_ctrl_transfer`.
pub fn xhci_device_ctrl_transfer(xfer: &'static UsbdXfer) -> UsbdStatus {
    let err = usb_insert_transfer(xfer);
    if err.is_err() {
        return err;
    }

    xhci_device_ctrl_start(queue_first(xfer.pipe()))
}

/// Arms the xfer's timeout (`timeout_set` + `timeout_add_msec`), as the start functions do.
fn xhci_arm_timeout(xfer: &'static UsbdXfer) {
    timeout_del(&xfer.timeout_handle);
    timeout_set(
        &xfer.timeout_handle,
        xhci_timeout,
        ptr::from_ref(xfer).cast_mut().cast(),
    );
    timeout_add_msec(&xfer.timeout_handle, u64::from(xfer.timeout.get()));
}

/// `xhci_device_ctrl_start`: a control transfer as a SETUP, an optional DATA and a STATUS
/// TRB; the SETUP TRB's cycle bit is flipped last to hand the TD over.
pub fn xhci_device_ctrl_start(xfer: &'static UsbdXfer) -> UsbdStatus {
    let sc = xhci_softc(xfer.device().bus());
    let xp = xhci_pipe(xfer.pipe());
    let req = xfer.request.get();
    let len = u32::from(ugetw(req.wLength));

    kassert!(xfer.rqflags.get() & URQ_REQUEST != 0);

    if sc.sc_bus.dying.get() != 0 || xp.halted.get() != USBD_NORMAL_COMPLETION {
        return USBD_IOERROR;
    }

    if xp.free_trbs.get() < 3 {
        return USBD_NOMEM;
    }

    let isread = usbd_xfer_isread(xfer);
    let ring = &xp.ring;
    let sync = |trb: XhciTrbRef| {
        bus_dmamap_sync(
            ring.dma.tag(),
            ring.dma.map(),
            trb.off(),
            size_of::<XhciTrb>(),
            BUS_DMASYNC_PREWRITE,
        );
    };

    if len != 0 {
        usb_syncmem(
            &xfer.dmabuf,
            0,
            len as BusSize,
            if isread {
                BUS_DMASYNC_PREREAD
            } else {
                BUS_DMASYNC_PREWRITE
            },
        );
    }

    // We'll toggle the setup TRB once we're finished with the stages.
    let (trb0, toggle) = xhci_xfer_get_trb(sc, xfer, 0);

    let mut flags = XHCI_TRB_TYPE_SETUP | XHCI_TRB_IDT | (toggle ^ 1);
    if len != 0 {
        if isread {
            flags |= XHCI_TRB_TRT_IN;
        } else {
            flags |= XHCI_TRB_TRT_OUT;
        }
    }

    // memcpy(&trb0->trb_paddr, &xfer->request, sizeof(trb0->trb_paddr)): the SETUP packet's
    // eight bytes, in wire order.
    let mut setup = [0u8; 8];
    setup.copy_from_slice(req.as_bytes());
    trb0.set_paddr_raw(u64::from_ne_bytes(setup));
    trb0.set_status(xhci_trb_intr(0) | xhci_trb_len(8));
    trb0.set_flags(flags);
    sync(trb0);

    // Data TRB
    if len != 0 {
        let (trb, toggle) = xhci_xfer_get_trb(sc, xfer, 0);

        let mut flags = XHCI_TRB_TYPE_DATA | toggle;
        if isread {
            flags |= XHCI_TRB_DIR_IN | XHCI_TRB_ISP;
        }

        trb.set_paddr(dmaaddr(&xfer.dmabuf, 0) as u64);
        trb.set_status(xhci_trb_intr(0) | xhci_trb_len(len) | xhci_xfer_tdsize(xfer, len, len));
        trb.set_flags(flags);

        sync(trb);
    }

    // Status TRB
    let (trb, toggle) = xhci_xfer_get_trb(sc, xfer, 1);

    let mut flags = XHCI_TRB_TYPE_STATUS | XHCI_TRB_IOC | toggle;
    if len == 0 || !isread {
        flags |= XHCI_TRB_DIR_IN;
    }

    trb.set_paddr(0);
    trb.set_status(xhci_trb_intr(0));
    trb.set_flags(flags);

    sync(trb);

    // Setup TRB
    trb0.set_flags(trb0.flags() ^ XHCI_TRB_CYCLE);
    sync(trb0);

    let s = splusb();
    xdwrite4(
        sc,
        xhci_doorbell(usize::from(xp.slot.get())),
        u32::from(xp.dci.get()),
    );

    xfer.status.set(USBD_IN_PROGRESS);
    if xfer.timeout.get() != 0 && sc.sc_bus.use_polling.get() == 0 {
        xhci_arm_timeout(xfer);
    }
    splx(s);

    USBD_IN_PROGRESS
}

/// `xhci_device_ctrl_abort`.
pub fn xhci_device_ctrl_abort(xfer: &'static UsbdXfer) {
    xhci_abort_xfer(xfer, USBD_CANCELLED);
}

/// `xhci_device_generic_transfer`: bulk and interrupt transfers.
pub fn xhci_device_generic_transfer(xfer: &'static UsbdXfer) -> UsbdStatus {
    let err = usb_insert_transfer(xfer);
    if err.is_err() {
        return err;
    }

    xhci_device_generic_start(queue_first(xfer.pipe()))
}

/// `xhci_device_generic_start`: a bulk or interrupt transfer as a chain of NORMAL TRBs of at
/// most 64 KiB, none crossing a 64 KiB boundary, plus a zero-length TD when asked for.
pub fn xhci_device_generic_start(xfer: &'static UsbdXfer) -> UsbdStatus {
    let sc = xhci_softc(xfer.device().bus());
    let xp = xhci_pipe(xfer.pipe());
    let mps = u32::from(ugetw(xfer.pipe().endpoint().edesc().wMaxPacketSize));
    let mut paddr = dmaaddr(&xfer.dmabuf, 0) as u64;
    let length = xfer.length.get();
    let mut zerotd: i32 = 0;

    kassert!(xfer.rqflags.get() & URQ_REQUEST == 0);

    if sc.sc_bus.dying.get() != 0 || xp.halted.get() != USBD_NORMAL_COMPLETION {
        return USBD_IOERROR;
    }

    // How many TRBs do we need for this transfer?
    let mut ntrb = howmany(length as usize, XHCI_TRB_MAXSIZE as usize) as i32;

    // If the buffer crosses a 64k boundary, we need one more.
    let mut len = XHCI_TRB_MAXSIZE - (paddr & (u64::from(XHCI_TRB_MAXSIZE) - 1)) as u32;
    if len < length {
        ntrb = howmany((length - len) as usize, XHCI_TRB_MAXSIZE as usize) as i32 + 1;
    } else {
        len = length;
    }

    // If we need to append a zero length packet, we need one more.
    if (xfer.flags.get() & USBD_FORCE_SHORT_XFER != 0 || length == 0)
        && length.is_multiple_of(u32::from(ue_get_size(mps as u16)))
    {
        zerotd = 1;
    }

    if (xp.free_trbs.get() as i64) < i64::from(ntrb + zerotd) {
        return USBD_NOMEM;
    }

    let isread = usbd_xfer_isread(xfer);
    let ring = &xp.ring;
    let sync = |trb: XhciTrbRef| {
        bus_dmamap_sync(
            ring.dma.tag(),
            ring.dma.map(),
            trb.off(),
            size_of::<XhciTrb>(),
            BUS_DMASYNC_PREWRITE,
        );
    };

    usb_syncmem(
        &xfer.dmabuf,
        0,
        length as BusSize,
        if isread {
            BUS_DMASYNC_PREREAD
        } else {
            BUS_DMASYNC_PREWRITE
        },
    );

    // We'll toggle the first TRB once we're finished with the chain.
    let (trb0, toggle) = xhci_xfer_get_trb(sc, xfer, i32::from(ntrb == 1));
    let mut flags = XHCI_TRB_TYPE_NORMAL | (toggle ^ 1);
    if isread {
        flags |= XHCI_TRB_ISP;
    }
    flags |= if ntrb == 1 {
        XHCI_TRB_IOC
    } else {
        XHCI_TRB_CHAIN
    };

    trb0.set_paddr(dmaaddr(&xfer.dmabuf, 0) as u64);
    trb0.set_status(xhci_trb_intr(0) | xhci_trb_len(len) | xhci_xfer_tdsize(xfer, length, len));
    trb0.set_flags(flags);
    sync(trb0);

    let mut remain = length - len;
    paddr += u64::from(len);

    // Chain more TRBs if needed.
    let mut i = ntrb - 1;
    while i > 0 {
        len = remain.min(XHCI_TRB_MAXSIZE);

        // Next (or Last) TRB.
        let (trb, toggle) = xhci_xfer_get_trb(sc, xfer, i32::from(i == 1));
        let mut flags = XHCI_TRB_TYPE_NORMAL | toggle;
        if isread {
            flags |= XHCI_TRB_ISP;
        }
        flags |= if i == 1 { XHCI_TRB_IOC } else { XHCI_TRB_CHAIN };

        trb.set_paddr(paddr);
        trb.set_status(xhci_trb_intr(0) | xhci_trb_len(len) | xhci_xfer_tdsize(xfer, remain, len));
        trb.set_flags(flags);

        sync(trb);

        remain -= len;
        paddr += u64::from(len);
        i -= 1;
    }

    // Do we need to issue a zero length transfer?
    if zerotd == 1 {
        let (trb, toggle) = xhci_xfer_get_trb(sc, xfer, -1);
        trb.set_paddr(0);
        trb.set_status(0);
        trb.set_flags(XHCI_TRB_TYPE_NORMAL | XHCI_TRB_IOC | toggle);
        sync(trb);
    }

    // First TRB.
    trb0.set_flags(trb0.flags() ^ XHCI_TRB_CYCLE);
    sync(trb0);

    let s = splusb();
    xdwrite4(
        sc,
        xhci_doorbell(usize::from(xp.slot.get())),
        u32::from(xp.dci.get()),
    );

    xfer.status.set(USBD_IN_PROGRESS);
    if xfer.timeout.get() != 0 && sc.sc_bus.use_polling.get() == 0 {
        xhci_arm_timeout(xfer);
    }
    splx(s);

    USBD_IN_PROGRESS
}

/// `xhci_device_generic_done`: restarts a repeating (interrupt) xfer.
pub fn xhci_device_generic_done(xfer: &'static UsbdXfer) {
    // Only happens with interrupt transfers.
    if xfer.pipe().repeat.get() != 0 {
        xfer.actlen.set(0);
        let _ = xhci_device_generic_start(xfer);
    }
}

/// `xhci_device_generic_abort`.
pub fn xhci_device_generic_abort(xfer: &'static UsbdXfer) {
    kassert!(
        xfer.pipe().repeat.get() == 0
            || xfer.pipe().intrxfer.get().is_some_and(|x| ptr::eq(x, xfer))
    );

    xhci_abort_xfer(xfer, USBD_CANCELLED);
}

/// `xhci_device_isoc_transfer`: every isochronous xfer is started at once.
pub fn xhci_device_isoc_transfer(xfer: &'static UsbdXfer) -> UsbdStatus {
    let err = usb_insert_transfer(xfer);
    if err.is_err() && err != USBD_IN_PROGRESS {
        return err;
    }

    xhci_device_isoc_start(xfer)
}

/// `xhci_device_isoc_start`: one ISOCH TRB per frame (plus a NORMAL one when the frame
/// crosses a 64 KiB boundary).
pub fn xhci_device_isoc_start(xfer: &'static UsbdXfer) -> UsbdStatus {
    let sc = xhci_softc(xfer.device().bus());
    let xp = xhci_pipe(xfer.pipe());
    let xx = xhci_xfer(xfer);

    kassert!(xfer.rqflags.get() & URQ_REQUEST == 0);

    // To allow continuous transfers, above we start all transfers immediately. However,
    // we're still going to get usbd_start_next call this when another xfer completes. So,
    // check if this is already in progress or not
    if xx.ntrb.get() > 0 {
        return USBD_IN_PROGRESS;
    }

    if sc.sc_bus.dying.get() != 0 || xp.halted.get() != USBD_NORMAL_COMPLETION {
        return USBD_IOERROR;
    }

    // Why would you do that anyway?
    if sc.sc_bus.use_polling.get() != 0 {
        return USBD_INVAL;
    }

    let nframes = xfer.nframes.get();
    let mut paddr = dmaaddr(&xfer.dmabuf, 0) as u64;

    // How many TRBs do for all Transfers?
    let mut ntrb: i64 = 0;
    for i in 0..nframes {
        let frlen = u32::from(frlength(xfer, i));
        // How many TRBs do we need for this transfer?
        ntrb += howmany(frlen as usize, XHCI_TRB_MAXSIZE as usize) as i64;

        // If the buffer crosses a 64k boundary, we need one more.
        let len = XHCI_TRB_MAXSIZE - (paddr & (u64::from(XHCI_TRB_MAXSIZE) - 1)) as u32;
        if len < frlen {
            ntrb += 1;
        }

        paddr += u64::from(frlen);
    }

    if (xp.free_trbs.get() as i64) < ntrb {
        return USBD_NOMEM;
    }

    let isread = usbd_xfer_isread(xfer);
    let ring = &xp.ring;
    let sync = |trb: XhciTrbRef| {
        bus_dmamap_sync(
            ring.dma.tag(),
            ring.dma.map(),
            trb.off(),
            size_of::<XhciTrb>(),
            BUS_DMASYNC_PREWRITE,
        );
    };

    usb_syncmem(
        &xfer.dmabuf,
        0,
        xfer.length.get() as BusSize,
        if isread {
            BUS_DMASYNC_PREREAD
        } else {
            BUS_DMASYNC_PREWRITE
        },
    );

    paddr = dmaaddr(&xfer.dmabuf, 0) as u64;

    let mut trb0: Option<XhciTrbRef> = None;
    for i in 0..nframes {
        let frlen = u32::from(frlength(xfer, i));
        // How many TRBs do we need for this transfer?
        let mut ntrb = howmany(frlen as usize, XHCI_TRB_MAXSIZE as usize) as i32;

        // If the buffer crosses a 64k boundary, we need one more.
        let mut len = XHCI_TRB_MAXSIZE - (paddr & (u64::from(XHCI_TRB_MAXSIZE) - 1)) as u32;
        if len < frlen {
            ntrb += 1;
        } else {
            len = frlen;
        }

        kassert!(ntrb < 3);

        // We'll commit the first TRB once we're finished with the chain.
        let (trb, mut toggle) = xhci_xfer_get_trb(sc, xfer, i32::from(ntrb == 1));

        // Record the first TRB so we can toggle later.
        if trb0.is_none() {
            trb0 = Some(trb);
            toggle ^= 1;
        }

        let mut flags = XHCI_TRB_TYPE_ISOCH | XHCI_TRB_SIA | toggle;
        if isread {
            flags |= XHCI_TRB_ISP;
        }
        flags |= if ntrb == 1 {
            XHCI_TRB_IOC
        } else {
            XHCI_TRB_CHAIN
        };

        let (tbc, tlbpc) = xhci_xfer_tbc(xfer, frlen);
        flags |= xhci_trb_isoc_tbc(tbc) | xhci_trb_isoc_tlbpc(tlbpc);

        trb.set_paddr(paddr);
        trb.set_status(xhci_trb_intr(0) | xhci_trb_len(len) | xhci_xfer_tdsize(xfer, frlen, len));
        trb.set_flags(flags);

        sync(trb);

        let mut remain = frlen - len;
        paddr += u64::from(len);

        // Chain more TRBs if needed.
        let mut j = ntrb - 1;
        while j > 0 {
            len = remain.min(XHCI_TRB_MAXSIZE);

            // Next (or Last) TRB.
            let (trb, toggle) = xhci_xfer_get_trb(sc, xfer, i32::from(j == 1));
            let mut flags = XHCI_TRB_TYPE_NORMAL | toggle;
            if isread {
                flags |= XHCI_TRB_ISP;
            }
            flags |= if j == 1 { XHCI_TRB_IOC } else { XHCI_TRB_CHAIN };

            trb.set_paddr(paddr);
            trb.set_status(
                xhci_trb_intr(0) | xhci_trb_len(len) | xhci_xfer_tdsize(xfer, remain, len),
            );
            trb.set_flags(flags);

            sync(trb);

            remain -= len;
            paddr += u64::from(len);
            j -= 1;
        }

        set_frlength(xfer, i, 0);
    }

    // First TRB.
    if let Some(trb0) = trb0 {
        trb0.set_flags(trb0.flags() ^ XHCI_TRB_CYCLE);
        sync(trb0);
    }

    let s = splusb();
    xdwrite4(
        sc,
        xhci_doorbell(usize::from(xp.slot.get())),
        u32::from(xp.dci.get()),
    );

    xfer.status.set(USBD_IN_PROGRESS);

    if xfer.timeout.get() != 0 {
        xhci_arm_timeout(xfer);
    }
    splx(s);

    USBD_IN_PROGRESS
}

const _: () = {
    // `pool_init(xhcixfer, sizeof(struct xhci_xfer), 0, ...)`: the pool's default alignment
    // (`ALIGN(1)`, at least 8) must suit the type `xhci_allocx` casts its items to.
    assert!(align_of::<XhciXfer>() <= 8);
    assert!(XHCI_MAX_XFER >= 3);
};
/* </CODE> */

/* <TESTS> */
#[cfg(test)]
mod tests {
    use super::*;

    fn ed(addr: u8, attr: u8, mps: u16, ival: u8) -> UsbEndpointDescriptor {
        let mut e = UsbEndpointDescriptor::zeroed();
        e.bLength = USB_ENDPOINT_DESCRIPTOR_SIZE as u8;
        e.bDescriptorType = UDESC_ENDPOINT;
        e.bEndpointAddress = addr;
        e.bmAttributes = attr;
        usetw(&mut e.wMaxPacketSize, mps);
        e.bInterval = ival;
        e
    }

    #[test]
    fn device_context_index() {
        // Section 4.5.1: control endpoints 2n+1, others 2n + (IN ? 1 : 0).
        assert_eq!(xhci_ed2dci(&ed(0x00, UE_CONTROL, 64, 0)), 1);
        assert_eq!(xhci_ed2dci(&ed(0x81, UE_BULK, 512, 0)), 3);
        assert_eq!(xhci_ed2dci(&ed(0x02, UE_BULK, 512, 0)), 4);
        assert_eq!(xhci_ed2dci(&ed(0x81, UE_INTERRUPT, 8, 10)), 3);
        assert_eq!(xhci_ed2dci(&ed(0x8f, UE_INTERRUPT, 8, 10)), 31);
    }

    #[test]
    fn intervals() {
        // Linear (low/full speed interrupt): fls(ival) - 1.
        assert_eq!(xhci_linear_interval(&ed(0x81, UE_INTERRUPT, 8, 0)), 0);
        assert_eq!(xhci_linear_interval(&ed(0x81, UE_INTERRUPT, 8, 1)), 0);
        assert_eq!(xhci_linear_interval(&ed(0x81, UE_INTERRUPT, 8, 10)), 3);
        assert_eq!(xhci_linear_interval(&ed(0x81, UE_INTERRUPT, 8, 255)), 7);
        // Exponential: bInterval - 1, clamped to 1..16.
        assert_eq!(xhci_exponential_interval(&ed(0x81, UE_INTERRUPT, 8, 0)), 0);
        assert_eq!(xhci_exponential_interval(&ed(0x81, UE_INTERRUPT, 8, 4)), 3);
        assert_eq!(
            xhci_exponential_interval(&ed(0x81, UE_INTERRUPT, 8, 200)),
            15
        );
    }

    #[test]
    fn root_hub_descriptors() {
        assert_eq!(XHCI_DEVD.as_bytes().len(), USB_DEVICE_DESCRIPTOR_SIZE);
        assert_eq!(XHCI_DEVD.bcdUSB, [0x00, 0x03]);
        assert_eq!(
            ugetw(XHCI_CONFD.wTotalLength) as usize,
            USB_CONFIG_DESCRIPTOR_SIZE
                + USB_INTERFACE_DESCRIPTOR_SIZE
                + USB_ENDPOINT_DESCRIPTOR_SIZE
        );
        assert_eq!(XHCI_ENDPD.bEndpointAddress, 0x81);
        assert_eq!(XHCI_HUBD.bDescriptorType, UDESC_SS_HUB);
    }

    #[test]
    fn command_trb_layout() {
        let t = cmd_trb(0x1234_5000, xhci_trb_set_slot(3) | XHCI_CMD_CONFIG_EP);
        assert_eq!(u64::from_le(t.trb_paddr), 0x1234_5000);
        assert_eq!(t.trb_status, 0);
        assert_eq!(xhci_trb_get_slot(u32::from_le(t.trb_flags)), 3);
        assert_eq!(
            u32::from_le(t.trb_flags) & XHCI_TRB_TYPE_MASK,
            XHCI_CMD_CONFIG_EP
        );
    }
}
/* </TESTS> */
