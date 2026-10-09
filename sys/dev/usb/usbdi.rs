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
//! The USB driver interface: `<dev/usb/usbdi.h>` (what a USB device driver sees: status
//! codes, request flags, `struct usb_attach_arg`, `usb_task`, match levels) and
//! `dev/usb/usbdi.c` (pipes, transfers, synchronous requests).
//!
//! Upstream: sys/dev/usb/usbdi.h @ 3ce1f3f79392, sys/dev/usb/usbdi.c @ 3ce1f3f79392
//!
//! A driver opens a pipe on an endpoint of its interface ([`usbd_open_pipe`]), allocates an
//! xfer from the device's bus ([`usbd_alloc_xfer`]), fills it ([`usbd_setup_xfer`]) and
//! hands it to [`usbd_transfer`]; the host controller completes it through
//! [`usb_transfer_complete`], which copies the data back, runs the driver's callback and
//! starts the pipe's next xfer. [`usbd_do_request`] is the synchronous control request on
//! pipe 0.
//!
//! ## Deviations
//! - The header and the file share this module.
//! - `usbd_status` is the enum [`UsbdStatus`]; a function that returns a status and an object
//!   through an out-parameter (`usbd_open_pipe`, `usbd_device2interface_handle`) returns
//!   `Result<&'static T, UsbdStatus>`, and `usbd_open_pipe_intr` reports a failed first
//!   transfer as `Err` even when `usbd_transfer` said `USBD_NORMAL_COMPLETION` (the C then
//!   returns success with the pipe already closed).
//! - The callers' buffers (`usbd_setup_xfer`'s `buffer`, `usbd_setup_isoc_xfer`'s
//!   `frlengths`, `usbd_open_pipe_intr`'s `buffer`) are raw pointers kept in the xfer, so
//!   those functions are `unsafe` with the C's contract: the memory stays valid until the
//!   transfer completes (`docs/C_TO_RUST.md`, a buffer a transfer carries). The synchronous
//!   requests take a slice and refuse (`USBD_INVAL`) one shorter than `wLength`, where the C
//!   would overrun it.
//! - The functions that free (`usbd_close_pipe`, `usbd_free_xfer`) are `unsafe` and take a
//!   `NonNull`, as `*_destroy` does (`docs/C_TO_RUST.md`).
//! - The descriptor getters return copies (`usbd_get_device_descriptor`,
//!   `usbd_interface2endpoint_descriptor`, `usbd_get_endpoint_descriptor`) or references into
//!   the configuration descriptor (`usbd_get_config_descriptor`,
//!   `usbd_get_interface_descriptor`); see `usbdivar.rs`.
//! - `usbd_get_no_alts` and `usbd_desc_iter_*` walk a byte slice; `usbd_get_no_alts` stops at
//!   a zero `bLength`, where the C loops forever. `usbd_desc_iter_init` returns the iterator
//!   (over nothing for an unconfigured device, where the C dereferences NULL).
//! - `usbd_get_xfer_status` takes `Option<&mut T>` for its nullable out-parameters.
//! - `usbd_match_device` takes a slice of entries that are or begin with a [`UsbDevno`]
//!   (`AsRef`), as `scsi_inqmatch` does; `usb_lookup` is the same call.
//! - `usbd_iface_claimed` returns `bool`.
//! - The `DIAGNOSTIC` NULL checks of `usbd_close_pipe`, `usbd_abort_pipe`,
//!   `usbd_get_config_descriptor` & co. have no NULL to test; the `busy_free` checks are
//!   under feature `diagnostic`.
//! - `USB_DEBUG` is not in GENERIC: `usbd_dump_iface`, `usbd_dump_device`,
//!   `usbd_dump_endpoint`, `usbd_dump_queue`, `usbd_dump_pipe` and the `DPRINTF`s are not
//!   ported, as in a kernel built without it.
//! - `usbd_xfer.timeout_handle` is set to the C's `timeout_set(&to, NULL, NULL)` member by
//!   member (`timeout_set` here takes a function, never NULL).

use core::cell::Cell;
use core::ffi::c_void;
use core::ptr::{self, NonNull};
use core::sync::atomic::Ordering;

use crate::dev::usb::usb::{
    UByte, UDESC_INTERFACE, UDESC_STRING, UE_XFERTYPE, UES_HALT, UF_ENDPOINT_HALT,
    UR_CLEAR_FEATURE, UR_GET_STATUS, UR_SET_INTERFACE, USBD_SHORT_XFER_OK, UT_READ_ENDPOINT,
    UT_WRITE_ENDPOINT, UT_WRITE_INTERFACE, UsbConfigDescriptor, UsbDescriptor, UsbDeviceDescriptor,
    UsbDeviceRequest, UsbEndpointDescriptor, UsbInterfaceDescriptor, UsbStatus,
    UsbStringDescriptor, UsbWire, ugetw, usb_tap, usb_wire_at, usetw, usetw2,
};
use crate::dev::usb::usb_mem::{kernaddr, usb_allocmem, usb_freemem};
use crate::dev::usb::usb_quirks::UsbdQuirks;
use crate::dev::usb::usb_subr::{usb_delay_ms, usbd_fill_iface_data, usbd_setup_pipe};
use crate::dev::usb::usbdivar::{
    URQ_AUTO_DMABUF, URQ_DEV_DMABUF, URQ_REQUEST, USBTAP_DIR_IN, USBTAP_DIR_OUT, UsbdBus,
    UsbdDevice, UsbdInterface, UsbdPipe, UsbdPipeList, UsbdXfer, usbd_xfer_isread,
};
#[cfg(feature = "diagnostic")]
use crate::dev::usb::usbdivar::{XFER_FREE, XFER_ONQU};
use crate::kassert;
use crate::kern::kern_malloc::free;
use crate::kern::kern_synch::{tsleep_nsec, wakeup};
use crate::kern::kern_time::ratecheck;
use crate::kern::subr_prf::printf;
use crate::machine::intr::{IPL_BIO, IPL_SOFTNET, splraise, splsoftassert, splx};
use crate::queue_adapter;
use crate::sys::errno::Errno;
use crate::sys::malloc::M_USB;
use crate::sys::param::{PCATCH, PRIBIO, PWAIT};
use crate::sys::queue::{ListHead, TailqEntry};
use crate::sys::systm::INFSLP;
use crate::sys::time::{Timeval, sec_to_nsec};
use crate::sys::timeout::{KCLOCK_NONE, TIMEOUT_INITIALIZED};

/// `usbd_status`.
#[repr(i32)]
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
#[allow(non_camel_case_types)] // OpenBSD names, verbatim
pub enum UsbdStatus {
    /// `USBD_NORMAL_COMPLETION`: must be 0.
    USBD_NORMAL_COMPLETION = 0,
    /// `USBD_IN_PROGRESS`: 1.
    USBD_IN_PROGRESS,
    /* errors */
    /// `USBD_PENDING_REQUESTS`: 2.
    USBD_PENDING_REQUESTS,
    /// `USBD_NOT_STARTED`: 3.
    USBD_NOT_STARTED,
    /// `USBD_INVAL`: 4.
    USBD_INVAL,
    /// `USBD_NOMEM`: 5.
    USBD_NOMEM,
    /// `USBD_CANCELLED`: 6.
    USBD_CANCELLED,
    /// `USBD_BAD_ADDRESS`: 7.
    USBD_BAD_ADDRESS,
    /// `USBD_IN_USE`: 8.
    USBD_IN_USE,
    /// `USBD_NO_ADDR`: 9.
    USBD_NO_ADDR,
    /// `USBD_SET_ADDR_FAILED`: 10.
    USBD_SET_ADDR_FAILED,
    /// `USBD_NO_POWER`: 11.
    USBD_NO_POWER,
    /// `USBD_TOO_DEEP`: 12.
    USBD_TOO_DEEP,
    /// `USBD_IOERROR`: 13.
    USBD_IOERROR,
    /// `USBD_NOT_CONFIGURED`: 14.
    USBD_NOT_CONFIGURED,
    /// `USBD_TIMEOUT`: 15.
    USBD_TIMEOUT,
    /// `USBD_SHORT_XFER`: 16.
    USBD_SHORT_XFER,
    /// `USBD_STALLED`: 17.
    USBD_STALLED,
    /// `USBD_INTERRUPTED`: 18.
    USBD_INTERRUPTED,

    /// `USBD_ERROR_MAX`: must be last.
    USBD_ERROR_MAX,
}

pub use UsbdStatus::*;

impl UsbdStatus {
    /// The C's `if (err)`: anything but `USBD_NORMAL_COMPLETION` (`USBD_IN_PROGRESS`
    /// included).
    pub fn is_err(self) -> bool {
        self != USBD_NORMAL_COMPLETION
    }
}

/// `usbd_callback`: a transfer's completion function. It may free the xfer (as
/// `usbd_request_async_cb` does); nothing touches the xfer after it returns.
pub type UsbdCallback = fn(xfer: &'static UsbdXfer, priv_: *mut c_void, status: UsbdStatus);

/* Open flags */
/// `USBD_EXCLUSIVE_USE`.
pub const USBD_EXCLUSIVE_USE: u8 = 0x01;

/// `USBD_DEFAULT_INTERVAL`: use default (specified by ep. desc.) interval on interrupt pipe.
pub const USBD_DEFAULT_INTERVAL: i32 = -1;

/* Request flags */
/// `USBD_NO_COPY`: do not copy data to DMA buffer.
pub const USBD_NO_COPY: u16 = 0x01;
/// `USBD_SYNCHRONOUS`: wait for completion.
pub const USBD_SYNCHRONOUS: u16 = 0x02;
// in usb.h: USBD_SHORT_XFER_OK 0x04, allow short reads.
/// `USBD_FORCE_SHORT_XFER`: force last short packet on write.
pub const USBD_FORCE_SHORT_XFER: u16 = 0x08;
/// `USBD_CATCH`: catch signals while sleeping.
pub const USBD_CATCH: u16 = 0x10;

/// `USBD_NO_TIMEOUT`.
pub const USBD_NO_TIMEOUT: u32 = 0;
/// `USBD_DEFAULT_TIMEOUT`: ms = 5 s.
pub const USBD_DEFAULT_TIMEOUT: u32 = 5000;

/// `DEVINFOSIZE`.
pub const DEVINFOSIZE: usize = 1024;

/// `struct usbd_desc_iter`: an iterator for descriptors.
pub struct UsbdDescIter<'a> {
    /// The descriptors, `cur` to `end` in C.
    buf: &'a [UByte],
    /// `cur`, as an offset into `buf`.
    cur: usize,
}

/// `struct usb_task`: the `usb_task` structs form a queue of things to run in the USB task
/// threads. Normally this is just device discovery when a connect/disconnect has been
/// detected. But it may also be used by drivers that need to perform (short) tasks that must
/// have a process context.
///
/// All-zero is a valid, idle task.
pub struct UsbTask {
    /// `next`: on one of `usb.c`'s queues.
    pub next: TailqEntry<UsbTask>,
    /// `dev`.
    pub dev: Cell<Option<&'static UsbdDevice>>,
    /// `fun`.
    pub fun: Cell<Option<fn(*mut c_void)>>,
    /// `arg`.
    pub arg: Cell<*mut c_void>,
    /// `type`: `USB_TASK_TYPE_*`.
    pub r#type: Cell<i8>,
    /// `state`: `USB_TASK_STATE_*`.
    pub state: Cell<u32>,
}

/// `USB_TASK_TYPE_GENERIC`.
pub const USB_TASK_TYPE_GENERIC: i8 = 0;
/// `USB_TASK_TYPE_EXPLORE`.
pub const USB_TASK_TYPE_EXPLORE: i8 = 1;
/// `USB_TASK_TYPE_ABORT`.
pub const USB_TASK_TYPE_ABORT: i8 = 2;

/// `USB_TASK_STATE_NONE`.
pub const USB_TASK_STATE_NONE: u32 = 0x0;
/// `USB_TASK_STATE_ONQ`.
pub const USB_TASK_STATE_ONQ: u32 = 0x1;
/// `USB_TASK_STATE_RUN`.
pub const USB_TASK_STATE_RUN: u32 = 0x2;

impl UsbTask {
    /// An idle task (all zero, as a task in a zeroed softc).
    pub const fn new() -> Self {
        Self {
            next: TailqEntry::new(),
            dev: Cell::new(None),
            fun: Cell::new(None),
            arg: Cell::new(ptr::null_mut()),
            r#type: Cell::new(USB_TASK_TYPE_GENERIC),
            state: Cell::new(USB_TASK_STATE_NONE),
        }
    }
}

impl Default for UsbTask {
    fn default() -> Self {
        Self::new()
    }
}

queue_adapter!(
    /// `TAILQ_ENTRY(usb_task) next`: `usb.c`'s task queues.
    pub UsbTaskList: UsbTask, next => TailqEntry<UsbTask>
);

/// `usb_init_task(t, f, a, y)`.
pub fn usb_init_task(t: &UsbTask, f: fn(*mut c_void), a: *mut c_void, y: i8) {
    t.fun.set(Some(f));
    t.arg.set(a);
    t.r#type.set(y);
    t.state.set(USB_TASK_STATE_NONE);
}

/// `struct usb_devno`: a vendor/product pair in a driver's match table.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct UsbDevno {
    /// `ud_vendor`.
    pub ud_vendor: u16,
    /// `ud_product`: `USB_PRODUCT_ANY` matches all.
    pub ud_product: u16,
}

impl AsRef<UsbDevno> for UsbDevno {
    fn as_ref(&self) -> &UsbDevno {
        self
    }
}

/// `USB_PRODUCT_ANY`.
pub const USB_PRODUCT_ANY: u16 = 0xffff;

/// `struct usb_attach_arg`: what a USB device or interface driver's match and attach get as
/// `aux`, from `usbd_probe_and_attach`. Valid only during the `config_found` that hands it
/// over.
pub struct UsbAttachArg {
    /// `port`.
    pub port: i32,
    /// `configno`.
    pub configno: i32,
    /// `ifaceno`.
    pub ifaceno: i32,
    /// `vendor`.
    pub vendor: i32,
    /// `product`.
    pub product: i32,
    /// `release`.
    pub release: i32,
    /// `device`: current device.
    pub device: Option<&'static UsbdDevice>,
    /// `iface`: current interface.
    pub iface: Option<&'static UsbdInterface>,
    /// `usegeneric`.
    pub usegeneric: i32,
    /// `ifaces`: all interfaces (`nifaces` of them), NULL when matching the whole device.
    pub ifaces: *const &'static UsbdInterface,
    /// `nifaces`: number of interfaces.
    pub nifaces: i32,
    /// `cookie`: correlates the drivers attached from one device (audio and wskbd).
    pub cookie: *mut c_void,
}

impl UsbAttachArg {
    /// `uaa->ifaces[0..nifaces]`.
    pub fn ifaces(&self) -> &[&'static UsbdInterface] {
        if self.ifaces.is_null() || self.nifaces <= 0 {
            return &[];
        }
        // SAFETY: `usbd_probe_and_attach` points `ifaces` at `nifaces` interfaces it owns
        // until `config_found` returns, and the argument is only valid that long.
        unsafe { core::slice::from_raw_parts(self.ifaces, self.nifaces as usize) }
    }

    /// `uaa->device`. Panics on an argument without one (`usbd_probe_and_attach` always
    /// sets it).
    pub fn device(&self) -> &'static UsbdDevice {
        match self.device {
            Some(d) => d,
            None => crate::kern::subr_prf::panic(format_args!("usb_attach_arg without device")),
        }
    }
}

/* Match codes. */
/* First five codes is for a whole device. */
/// `UMATCH_VENDOR_PRODUCT_REV`.
pub const UMATCH_VENDOR_PRODUCT_REV: i32 = 14;
/// `UMATCH_VENDOR_PRODUCT`.
pub const UMATCH_VENDOR_PRODUCT: i32 = 13;
/// `UMATCH_VENDOR_DEVCLASS_DEVPROTO`.
pub const UMATCH_VENDOR_DEVCLASS_DEVPROTO: i32 = 12;
/// `UMATCH_DEVCLASS_DEVSUBCLASS_DEVPROTO`.
pub const UMATCH_DEVCLASS_DEVSUBCLASS_DEVPROTO: i32 = 11;
/// `UMATCH_DEVCLASS_DEVSUBCLASS`.
pub const UMATCH_DEVCLASS_DEVSUBCLASS: i32 = 10;
/* Next six codes are for interfaces. */
/// `UMATCH_VENDOR_PRODUCT_REV_CONF_IFACE`.
pub const UMATCH_VENDOR_PRODUCT_REV_CONF_IFACE: i32 = 9;
/// `UMATCH_VENDOR_PRODUCT_CONF_IFACE`.
pub const UMATCH_VENDOR_PRODUCT_CONF_IFACE: i32 = 8;
/// `UMATCH_VENDOR_IFACESUBCLASS_IFACEPROTO`.
pub const UMATCH_VENDOR_IFACESUBCLASS_IFACEPROTO: i32 = 7;
/// `UMATCH_VENDOR_IFACESUBCLASS`.
pub const UMATCH_VENDOR_IFACESUBCLASS: i32 = 6;
/// `UMATCH_IFACECLASS_IFACESUBCLASS_IFACEPROTO`.
pub const UMATCH_IFACECLASS_IFACESUBCLASS_IFACEPROTO: i32 = 5;
/// `UMATCH_IFACECLASS_IFACESUBCLASS`.
pub const UMATCH_IFACECLASS_IFACESUBCLASS: i32 = 4;
/// `UMATCH_IFACECLASS`.
pub const UMATCH_IFACECLASS: i32 = 3;
/// `UMATCH_IFACECLASS_GENERIC`.
pub const UMATCH_IFACECLASS_GENERIC: i32 = 2;
/* Generic driver */
/// `UMATCH_GENERIC`.
pub const UMATCH_GENERIC: i32 = 1;
/* No match */
/// `UMATCH_NONE`.
pub const UMATCH_NONE: i32 = 0;

/// `IPL_USB`: the host controllers' interrupt level.
pub const IPL_USB: i32 = IPL_BIO;
/// `IPL_SOFTUSB`: the USB soft interrupt's level.
pub const IPL_SOFTUSB: i32 = IPL_SOFTNET;

/// `splusb()`.
pub fn splusb() -> i32 {
    splraise(IPL_SOFTUSB)
}

/// `splhardusb()`.
pub fn splhardusb() -> i32 {
    splraise(IPL_USB)
}

/// `usbd_is_dying`.
pub fn usbd_is_dying(dev: &UsbdDevice) -> bool {
    dev.dying.get() != 0 || dev.bus().dying.get() != 0
}

/// `usbd_deactivate`.
pub fn usbd_deactivate(dev: &UsbdDevice) {
    dev.dying.set(1);
}

/// `usbd_ref_incr`.
pub fn usbd_ref_incr(dev: &UsbdDevice) {
    dev.ref_cnt.set(dev.ref_cnt.get().wrapping_add(1));
}

/// `usbd_ref_decr`.
pub fn usbd_ref_decr(dev: &UsbdDevice) {
    dev.ref_cnt.set(dev.ref_cnt.get().wrapping_sub(1));
    if dev.ref_cnt.get() == 0 {
        wakeup(ptr::from_ref(&dev.ref_cnt));
    }
}

/// `usbd_ref_wait`.
pub fn usbd_ref_wait(dev: &UsbdDevice) {
    while dev.ref_cnt.get() > 0 {
        let _ = tsleep_nsec(
            ptr::from_ref(&dev.ref_cnt),
            PWAIT,
            "usbref",
            sec_to_nsec(60),
        );
    }
}

/// `usbd_get_devcnt`.
pub fn usbd_get_devcnt(dev: &UsbdDevice) -> i32 {
    dev.ndevs.get()
}

/// `usbd_claim_iface`.
pub fn usbd_claim_iface(dev: &UsbdDevice, ifaceno: usize) {
    dev.ifaces()[ifaceno].claimed.set(1);
}

/// `usbd_iface_claimed`: `true` when a driver claimed the interface.
pub fn usbd_iface_claimed(dev: &UsbdDevice, ifaceno: usize) -> bool {
    dev.ifaces()[ifaceno].claimed.get() != 0
}

/// `usbd_open_pipe`.
pub fn usbd_open_pipe(
    iface: &'static UsbdInterface,
    address: u8,
    flags: u8,
) -> Result<&'static UsbdPipe, UsbdStatus> {
    usbd_open_pipe_ival(iface, address, flags, USBD_DEFAULT_INTERVAL)
}

/// `usbd_open_pipe_ival`.
pub fn usbd_open_pipe_ival(
    iface: &'static UsbdInterface,
    address: u8,
    flags: u8,
    ival: i32,
) -> Result<&'static UsbdPipe, UsbdStatus> {
    let endpoints = iface.endpoints();
    let mut found = None;
    for i in 0..usize::from(iface.idesc().bNumEndpoints) {
        let Some(ep) = endpoints.get(i) else {
            return Err(USBD_IOERROR);
        };
        if !ep.has_edesc() {
            return Err(USBD_IOERROR);
        }
        if ep.edesc().bEndpointAddress == address {
            found = Some(ep);
            break;
        }
    }
    let Some(ep) = found else {
        return Err(USBD_BAD_ADDRESS);
    };
    if flags & USBD_EXCLUSIVE_USE != 0 && ep.refcnt.get() != 0 {
        return Err(USBD_IN_USE);
    }
    let p = usbd_setup_pipe(iface.device(), Some(iface), ep, ival)?;
    // SAFETY: a new pipe, in no list; it stays allocated until `usbd_close_pipe` takes it
    // off. Under the kernel lock, as every USB path.
    unsafe { iface.pipes.insert_head(p) };
    Ok(p)
}

/// `usbd_open_pipe_intr`: opens an interrupt pipe and starts its repeating transfer into
/// `buffer`.
///
/// # Safety
///
/// `buffer` is valid for `len` bytes until the pipe is closed (the transfer repeats).
#[allow(clippy::too_many_arguments)] // the C's signature
pub unsafe fn usbd_open_pipe_intr(
    iface: &'static UsbdInterface,
    address: u8,
    flags: u8,
    priv_: *mut c_void,
    buffer: *mut u8,
    len: u32,
    cb: UsbdCallback,
    ival: i32,
) -> Result<&'static UsbdPipe, UsbdStatus> {
    let ipipe = usbd_open_pipe_ival(iface, address, USBD_EXCLUSIVE_USE, ival)?;
    let err = match usbd_alloc_xfer(iface.device()) {
        None => USBD_NOMEM,
        Some(xfer) => {
            // SAFETY: the caller's contract on `buffer`.
            unsafe {
                usbd_setup_xfer(
                    xfer,
                    ipipe,
                    priv_,
                    buffer,
                    len,
                    u16::from(flags),
                    USBD_NO_TIMEOUT,
                    Some(cb),
                )
            };
            ipipe.intrxfer.set(Some(xfer));
            ipipe.repeat.set(1);
            let e = usbd_transfer(xfer);
            if e == USBD_IN_PROGRESS {
                return Ok(ipipe);
            }
            // bad2:
            ipipe.intrxfer.set(None);
            ipipe.repeat.set(0);
            // SAFETY: the xfer is ours and no longer referenced by the pipe.
            unsafe { usbd_free_xfer(NonNull::from(xfer)) };
            e
        }
    };
    // bad1:
    // SAFETY: the pipe was opened above and nobody else knows it.
    unsafe { usbd_close_pipe(NonNull::from(ipipe)) };
    Err(err)
}

/// `usbd_close_pipe`: aborts what is queued, closes and frees the pipe (and its repeating
/// xfer).
///
/// # Safety
///
/// `pipe` is open (`usbd_open_pipe*`, `usbd_setup_pipe`) and nobody uses it afterwards.
pub unsafe fn usbd_close_pipe(pipe: NonNull<UsbdPipe>) -> UsbdStatus {
    // SAFETY: the caller's contract: an open pipe, allocated until the `free` below.
    let p: &'static UsbdPipe = unsafe { &*pipe.as_ptr() };

    if !p.queue.is_empty() {
        usbd_abort_pipe(p);
    }

    // Default pipes are never linked
    if p.iface.get().is_some() {
        // SAFETY: a pipe of an interface is on its `pipes` list (`usbd_open_pipe_ival`).
        unsafe { ListHead::<UsbdPipeList>::remove(p) };
    }
    let ep = p.endpoint();
    ep.refcnt.set(ep.refcnt.get() - 1);
    (p.methods().close)(p);
    if let Some(x) = p.intrxfer.get() {
        // SAFETY: the pipe's repeating xfer dies with it.
        unsafe { usbd_free_xfer(NonNull::from(x)) };
    }
    free(pipe.cast::<u8>(), M_USB, p.pipe_size.get());
    USBD_NORMAL_COMPLETION
}

/// `usbd_transfer`: starts `xfer`; for a `USBD_SYNCHRONOUS` one, waits for it (polling the
/// controller when the bus polls).
pub fn usbd_transfer(xfer: &'static UsbdXfer) -> UsbdStatus {
    let pipe = xfer.pipe();
    let bus = pipe.device().bus();
    let polling = bus.use_polling.get() != 0;

    if usbd_is_dying(pipe.device()) {
        return USBD_IOERROR;
    }

    xfer.done.store(false, Ordering::Relaxed);
    xfer.status.set(USBD_NOT_STARTED);

    if pipe.aborting.get() != 0 {
        return USBD_CANCELLED;
    }

    // If there is no buffer, allocate one.
    if xfer.rqflags.get() & URQ_DEV_DMABUF == 0 {
        #[cfg(feature = "diagnostic")]
        if xfer.rqflags.get() & URQ_AUTO_DMABUF != 0 {
            printf(format_args!("usbd_transfer: has old buffer!\n"));
        }
        let err = usb_allocmem(bus, xfer.length.get() as usize, 0, 0, &xfer.dmabuf);
        if err.is_err() {
            return err;
        }
        xfer.rqflags.set(xfer.rqflags.get() | URQ_AUTO_DMABUF);
    }

    if !usbd_xfer_isread(xfer) && xfer.flags.get() & USBD_NO_COPY == 0 && xfer.length.get() != 0 {
        // SAFETY: `usbd_setup_xfer`'s contract makes `buffer` valid for `length` bytes, and
        // the DMA buffer has at least `length` bytes (`usb_allocmem` above, or the driver's
        // `usbd_alloc_buffer`); the two do not overlap.
        unsafe {
            ptr::copy_nonoverlapping(
                xfer.buffer.get().cast_const(),
                kernaddr(&xfer.dmabuf, 0),
                xfer.length.get() as usize,
            )
        };
    }

    usb_tap(bus, xfer, USBTAP_DIR_OUT);

    let err = (pipe.methods().transfer)(xfer);

    if err != USBD_IN_PROGRESS && err != USBD_NORMAL_COMPLETION {
        // The transfer has not been queued, so free buffer.
        if xfer.rqflags.get() & URQ_AUTO_DMABUF != 0 {
            usb_freemem(bus, &xfer.dmabuf);
            xfer.rqflags.set(xfer.rqflags.get() & !URQ_AUTO_DMABUF);
        }
    }

    if xfer.flags.get() & USBD_SYNCHRONOUS == 0 {
        return err;
    }

    // Sync transfer, wait for completion.
    if err != USBD_IN_PROGRESS {
        return err;
    }

    let s = splusb();
    if polling {
        let mut timo = i64::from(xfer.timeout.get());
        while timo >= 0 {
            usb_delay_ms(bus, 1);
            if bus.dying.get() != 0 {
                xfer.status.set(USBD_IOERROR);
                usb_transfer_complete(xfer);
                break;
            }

            usbd_dopoll(pipe.device());
            if xfer.done.load(Ordering::Relaxed) {
                break;
            }
            timo -= 1;
        }

        if timo < 0 {
            xfer.status.set(USBD_TIMEOUT);
            usb_transfer_complete(xfer);
        }
    } else {
        while !xfer.done.load(Ordering::Relaxed) {
            let flags = PRIBIO
                | if xfer.flags.get() & USBD_CATCH != 0 {
                    PCATCH
                } else {
                    0
                };

            let r = tsleep_nsec(ptr::from_ref(xfer), flags, "usbsyn", INFSLP);
            if let Err(e) = r
                && !xfer.done.load(Ordering::Relaxed)
            {
                usbd_abort_pipe(pipe);
                if e == Errno::EINTR {
                    xfer.status.set(USBD_INTERRUPTED);
                } else {
                    xfer.status.set(USBD_TIMEOUT);
                }
            }
        }
    }
    splx(s);
    xfer.status.get()
}

/// `usbd_alloc_buffer`: gives the xfer a DMA buffer of `size` bytes the driver fills itself
/// (with `USBD_NO_COPY`); returns its kernel address.
pub fn usbd_alloc_buffer(xfer: &UsbdXfer, size: u32) -> Option<NonNull<u8>> {
    let bus = xfer.device().bus();

    #[cfg(feature = "diagnostic")]
    if xfer.rqflags.get() & (URQ_DEV_DMABUF | URQ_AUTO_DMABUF) != 0 {
        printf(format_args!(
            "usbd_alloc_buffer: xfer already has a buffer\n"
        ));
    }
    let err = usb_allocmem(bus, size as usize, 0, 0, &xfer.dmabuf);
    if err.is_err() {
        return None;
    }
    xfer.rqflags.set(xfer.rqflags.get() | URQ_DEV_DMABUF);
    NonNull::new(kernaddr(&xfer.dmabuf, 0))
}

/// `usbd_free_buffer`.
pub fn usbd_free_buffer(xfer: &UsbdXfer) {
    #[cfg(feature = "diagnostic")]
    if xfer.rqflags.get() & (URQ_DEV_DMABUF | URQ_AUTO_DMABUF) == 0 {
        printf(format_args!("usbd_free_buffer: no buffer\n"));
        return;
    }
    xfer.rqflags
        .set(xfer.rqflags.get() & !(URQ_DEV_DMABUF | URQ_AUTO_DMABUF));
    usb_freemem(xfer.device().bus(), &xfer.dmabuf);
}

/// `usbd_alloc_xfer`: an xfer of `dev`'s bus (`allocx`), zeroed.
pub fn usbd_alloc_xfer(dev: &'static UsbdDevice) -> Option<&'static UsbdXfer> {
    let bus = dev.bus();
    let xfer = (bus.methods().allocx)(bus)?;
    #[cfg(feature = "diagnostic")]
    xfer.busy_free.set(XFER_FREE);
    xfer.device.set(Some(dev));
    // timeout_set(&xfer->timeout_handle, NULL, NULL)
    let to = &xfer.timeout_handle;
    to.to_func.set(None);
    to.to_arg.set(ptr::null_mut());
    to.to_kclock.set(KCLOCK_NONE);
    to.to_flags.set(TIMEOUT_INITIALIZED);
    Some(xfer)
}

/// `usbd_free_xfer`: frees the xfer's DMA buffer and gives the xfer back to its bus.
///
/// # Safety
///
/// `xfer` came from `usbd_alloc_xfer`, is not queued, and nobody uses it afterwards.
pub unsafe fn usbd_free_xfer(xfer: NonNull<UsbdXfer>) {
    // SAFETY: the caller's contract: a live xfer until `freex` below.
    let x = unsafe { xfer.as_ref() };
    if x.rqflags.get() & (URQ_DEV_DMABUF | URQ_AUTO_DMABUF) != 0 {
        usbd_free_buffer(x);
    }
    #[cfg(feature = "diagnostic")]
    if x.busy_free.get() != XFER_FREE {
        printf(format_args!(
            "usbd_free_xfer: xfer={:p} not free\n",
            xfer.as_ptr()
        ));
        return;
    }
    let bus = x.device().bus();
    // SAFETY: the xfer came from this bus's `allocx` and is not used again (the caller's
    // contract).
    unsafe { (bus.methods().freex)(bus, xfer) };
}

/// `usbd_setup_xfer`.
///
/// # Safety
///
/// `buffer` is valid for `length` bytes (reads for a write to the device, writes for a read)
/// until the transfer completes; it may be null when `length` is 0 or with `USBD_NO_COPY`.
#[allow(clippy::too_many_arguments)] // the C's signature
pub unsafe fn usbd_setup_xfer(
    xfer: &UsbdXfer,
    pipe: &'static UsbdPipe,
    priv_: *mut c_void,
    buffer: *mut u8,
    length: u32,
    flags: u16,
    timeout: u32,
    callback: Option<UsbdCallback>,
) {
    xfer.pipe.set(Some(pipe));
    xfer.priv_.set(priv_);
    xfer.buffer.set(buffer);
    xfer.length.set(length);
    xfer.actlen.set(0);
    xfer.flags.set(flags);
    xfer.timeout.set(timeout);
    xfer.status.set(USBD_NOT_STARTED);
    xfer.callback.set(callback);
    xfer.rqflags.set(xfer.rqflags.get() & !URQ_REQUEST);
    xfer.nframes.set(0);
}

/// `usbd_setup_default_xfer`: a control request on the device's pipe 0.
///
/// # Safety
///
/// As for [`usbd_setup_xfer`].
#[allow(clippy::too_many_arguments)] // the C's signature
pub unsafe fn usbd_setup_default_xfer(
    xfer: &UsbdXfer,
    dev: &UsbdDevice,
    priv_: *mut c_void,
    timeout: u32,
    req: &UsbDeviceRequest,
    buffer: *mut u8,
    length: u32,
    flags: u16,
    callback: Option<UsbdCallback>,
) {
    xfer.pipe.set(dev.default_pipe.get());
    xfer.priv_.set(priv_);
    xfer.buffer.set(buffer);
    xfer.length.set(length);
    xfer.actlen.set(0);
    xfer.flags.set(flags);
    xfer.timeout.set(timeout);
    xfer.status.set(USBD_NOT_STARTED);
    xfer.callback.set(callback);
    xfer.request.set(*req);
    xfer.rqflags.set(xfer.rqflags.get() | URQ_REQUEST);
    xfer.nframes.set(0);
}

/// `usbd_setup_isoc_xfer`.
///
/// # Safety
///
/// `frlengths` points at `nframes` frame lengths that stay valid (and are updated by the
/// controller) until the transfer completes.
pub unsafe fn usbd_setup_isoc_xfer(
    xfer: &UsbdXfer,
    pipe: &'static UsbdPipe,
    priv_: *mut c_void,
    frlengths: *mut u16,
    nframes: u32,
    flags: u16,
    callback: Option<UsbdCallback>,
) {
    xfer.pipe.set(Some(pipe));
    xfer.priv_.set(priv_);
    xfer.buffer.set(ptr::null_mut());
    let mut length = 0u32;
    for i in 0..nframes as usize {
        // SAFETY: the caller's contract: `nframes` readable lengths.
        length = length.wrapping_add(u32::from(unsafe { *frlengths.add(i) }));
    }
    xfer.length.set(length);
    xfer.actlen.set(0);
    xfer.flags.set(flags);
    xfer.timeout.set(USBD_NO_TIMEOUT);
    xfer.status.set(USBD_NOT_STARTED);
    xfer.callback.set(callback);
    xfer.rqflags.set(xfer.rqflags.get() & !URQ_REQUEST);
    xfer.frlengths.set(frlengths);
    xfer.nframes.set(nframes as i32);
}

/// `usbd_get_xfer_status`: fills the out-parameters that are given.
pub fn usbd_get_xfer_status(
    xfer: &UsbdXfer,
    priv_: Option<&mut *mut c_void>,
    buffer: Option<&mut *mut u8>,
    count: Option<&mut u32>,
    status: Option<&mut UsbdStatus>,
) {
    if let Some(p) = priv_ {
        *p = xfer.priv_.get();
    }
    if let Some(b) = buffer {
        *b = xfer.buffer.get();
    }
    if let Some(c) = count {
        *c = xfer.actlen.get();
    }
    if let Some(s) = status {
        *s = xfer.status.get();
    }
}

/// `usbd_get_config_descriptor`: the current configuration's descriptor (its header; the
/// whole of it is `dev.cdesc`), `None` when unconfigured.
pub fn usbd_get_config_descriptor(dev: &UsbdDevice) -> Option<&'static UsbConfigDescriptor> {
    dev.cdesc()
}

/// `usbd_get_interface_descriptor`.
pub fn usbd_get_interface_descriptor(
    iface: &UsbdInterface,
) -> Option<&'static UsbInterfaceDescriptor> {
    iface.idesc.get()
}

/// `usbd_get_device_descriptor`: a copy of the device descriptor.
pub fn usbd_get_device_descriptor(dev: &UsbdDevice) -> UsbDeviceDescriptor {
    dev.ddesc.get()
}

/// `usbd_interface2endpoint_descriptor`: a copy of the interface's `index`th endpoint
/// descriptor.
pub fn usbd_interface2endpoint_descriptor(
    iface: &UsbdInterface,
    index: u8,
) -> Option<UsbEndpointDescriptor> {
    if index >= iface.idesc().bNumEndpoints {
        return None;
    }
    let ep = iface.endpoints().get(usize::from(index))?;
    if !ep.has_edesc() {
        return None;
    }
    Some(ep.edesc())
}

/// `usbd_abort_pipe`: aborts every xfer queued on the pipe (the controller completes each).
pub fn usbd_abort_pipe(pipe: &'static UsbdPipe) {
    let s = splusb();
    pipe.repeat.set(0);
    pipe.aborting.set(1);
    while let Some(xfer) = pipe.queue.first() {
        // SAFETY: a queued xfer is allocated until it is completed and freed by its owner;
        // the controller's `abort` completes it, which takes it off the queue.
        let xfer: &'static UsbdXfer = unsafe { &*ptr::from_ref(xfer) };
        // Make the HC abort it (and invoke the callback).
        (pipe.methods().abort)(xfer);
        // XXX only for non-0 usbd_clear_endpoint_stall(pipe);
    }
    pipe.aborting.set(0);
    splx(s);
}

/// The `CLEAR_FEATURE(ENDPOINT_HALT)` request for `pipe`'s endpoint.
fn clear_halt_request(pipe: &UsbdPipe) -> UsbDeviceRequest {
    let mut req = UsbDeviceRequest {
        bmRequestType: UT_WRITE_ENDPOINT,
        bRequest: UR_CLEAR_FEATURE,
        ..UsbDeviceRequest::default()
    };
    usetw(&mut req.wValue, UF_ENDPOINT_HALT);
    usetw(
        &mut req.wIndex,
        u16::from(pipe.endpoint().edesc().bEndpointAddress),
    );
    usetw(&mut req.wLength, 0);
    req
}

/// `usbd_clear_endpoint_stall`.
pub fn usbd_clear_endpoint_stall(pipe: &'static UsbdPipe) -> UsbdStatus {
    let dev = pipe.device();

    // Clearing en endpoint stall resets the endpoint toggle, so do the same to the HC
    // toggle.
    usbd_clear_endpoint_toggle(pipe);

    let req = clear_halt_request(pipe);
    usbd_do_request(dev, &req, &mut [])
}

/// `usbd_clear_endpoint_stall_async`.
pub fn usbd_clear_endpoint_stall_async(pipe: &'static UsbdPipe) -> UsbdStatus {
    let dev = pipe.device();

    usbd_clear_endpoint_toggle(pipe);

    let req = clear_halt_request(pipe);

    let Some(xfer) = usbd_alloc_xfer(dev) else {
        return USBD_NOMEM;
    };

    usbd_request_async(xfer, &req, ptr::null_mut(), None)
}

/// `usbd_clear_endpoint_toggle`.
pub fn usbd_clear_endpoint_toggle(pipe: &'static UsbdPipe) {
    if let Some(f) = pipe.methods().cleartoggle {
        f(pipe);
    }
}

/// `usbd_device2interface_handle`.
pub fn usbd_device2interface_handle(
    dev: &UsbdDevice,
    ifaceno: u8,
) -> Result<&'static UsbdInterface, UsbdStatus> {
    let Some(cd) = dev.cdesc() else {
        return Err(USBD_NOT_CONFIGURED);
    };
    let ifaces = dev.ifaces();
    if ifaceno < cd.bNumInterfaces {
        return Ok(&ifaces[usize::from(ifaceno)]);
    }
    // The correct interface should be at dev->ifaces[ifaceno], but we've seen
    // non-compliant devices in the wild which present non-contiguous interface numbers and
    // this skews the indices. For this reason we linearly search the interface array.
    for iface in ifaces.iter().take(usize::from(cd.bNumInterfaces)) {
        if iface.idesc().bInterfaceNumber == ifaceno {
            return Ok(iface);
        }
    }
    Err(USBD_INVAL)
}

/// `usbd_set_interface`: selects alternate setting `altno` (XXXX use altno).
pub fn usbd_set_interface(iface: &'static UsbdInterface, altno: i32) -> UsbdStatus {
    if iface.pipes.first().is_some() {
        return USBD_IN_USE;
    }

    let endpoints = iface.endpoints.get();
    let nendpt = usize::from(iface.nendpt.get());
    let err = usbd_fill_iface_data(iface.device(), iface.index.get() as usize, altno);
    if err.is_err() {
        return err;
    }

    // new setting works, we can free old endpoints
    if let Some(e) = NonNull::new(endpoints) {
        free(
            e.cast::<u8>(),
            M_USB,
            nendpt * size_of::<crate::dev::usb::usbdivar::UsbdEndpoint>(),
        );
    }

    let idesc = iface.idesc();
    let mut req = UsbDeviceRequest {
        bmRequestType: UT_WRITE_INTERFACE,
        bRequest: UR_SET_INTERFACE,
        ..UsbDeviceRequest::default()
    };
    usetw(&mut req.wValue, u16::from(idesc.bAlternateSetting));
    usetw(&mut req.wIndex, u16::from(idesc.bInterfaceNumber));
    usetw(&mut req.wLength, 0);
    usbd_do_request(iface.device(), &req, &mut [])
}

/// `usbd_get_no_alts`: the number of alternate settings of interface `ifaceno` in a
/// configuration descriptor (`cdesc`, all `wTotalLength` bytes of it).
pub fn usbd_get_no_alts(cdesc: &[u8], ifaceno: i32) -> i32 {
    let Some(cd) = usb_wire_at::<UsbConfigDescriptor>(cdesc, 0) else {
        return 0;
    };
    let end = usize::from(ugetw(cd.wTotalLength)).min(cdesc.len());
    let mut p = 0usize;
    let mut n = 0;
    while p < end {
        let d = UsbInterfaceDescriptor::read_from(&cdesc[p..end]);
        if d.bLength == 0 {
            break;
        }
        if p + usize::from(d.bLength) <= end
            && d.bDescriptorType == UDESC_INTERFACE
            && i32::from(d.bInterfaceNumber) == ifaceno
        {
            n += 1;
        }
        p += usize::from(d.bLength);
    }
    n
}

/// `usbd_get_interface_altindex`.
pub fn usbd_get_interface_altindex(iface: &UsbdInterface) -> i32 {
    iface.altindex.get()
}

/*** Internal routines ***/

/// `usb_transfer_complete`: called by the host controller at `splusb()` when `xfer` is
/// done (`status`, `actlen` set): copies the data in, frees the automatic DMA buffer, takes
/// the xfer off the pipe's queue (unless the pipe repeats), runs the controller's `done` and
/// the callback, wakes a synchronous waiter and starts the pipe's next xfer.
pub fn usb_transfer_complete(xfer: &'static UsbdXfer) {
    let pipe = xfer.pipe();
    let bus = pipe.device().bus();
    let polling = bus.use_polling.get() != 0;

    // #if 0: XXX ohci_intr1() calls usb_transfer_complete() for RHSC.
    // splsoftassert(IPL_SOFTUSB);

    #[cfg(feature = "diagnostic")]
    if xfer.busy_free.get() != XFER_ONQU {
        printf(format_args!(
            "usb_transfer_complete: xfer={:p} not on queue\n",
            ptr::from_ref(xfer)
        ));
        return;
    }

    // XXXX
    if polling {
        pipe.running.set(0);
    }

    if xfer.actlen.get() > xfer.length.get() {
        #[cfg(feature = "diagnostic")]
        printf(format_args!(
            "usb_transfer_complete: actlen > len {} > {}\n",
            xfer.actlen.get(),
            xfer.length.get()
        ));
        xfer.actlen.set(xfer.length.get());
    }

    if usbd_xfer_isread(xfer) && xfer.actlen.get() != 0 && xfer.flags.get() & USBD_NO_COPY == 0 {
        // SAFETY: `usbd_setup_xfer`'s contract makes `buffer` writable for `length` >=
        // `actlen` bytes; the DMA buffer holds at least `length` bytes; distinct memory.
        unsafe {
            ptr::copy_nonoverlapping(
                kernaddr(&xfer.dmabuf, 0).cast_const(),
                xfer.buffer.get(),
                xfer.actlen.get() as usize,
            )
        };
    }

    // if we allocated the buffer in usbd_transfer() we free it here.
    if xfer.rqflags.get() & URQ_AUTO_DMABUF != 0 && pipe.repeat.get() == 0 {
        usb_freemem(bus, &xfer.dmabuf);
        xfer.rqflags.set(xfer.rqflags.get() & !URQ_AUTO_DMABUF);
    }

    if pipe.repeat.get() == 0 {
        // Remove request from queue.
        kassert!(pipe.queue.first().is_some_and(|f| ptr::eq(f, xfer)));
        // SAFETY: `xfer` is the queue's head (the assertion above, the C's `KASSERT`).
        unsafe { pipe.queue.remove_head() };
        #[cfg(feature = "diagnostic")]
        xfer.busy_free.set(XFER_FREE);
    }

    // Count completed transfers.
    let mut stats = bus.stats.get();
    let t = usize::from(pipe.endpoint().edesc().bmAttributes & UE_XFERTYPE);
    stats.uds_requests[t] = stats.uds_requests[t].wrapping_add(1);
    bus.stats.set(stats);

    xfer.done.store(true, Ordering::Relaxed);
    if !xfer.status.get().is_err()
        && xfer.actlen.get() < xfer.length.get()
        && xfer.flags.get() & USBD_SHORT_XFER_OK == 0
    {
        xfer.status.set(USBD_SHORT_XFER);
    }

    usb_tap(bus, xfer, USBTAP_DIR_IN);

    // We cannot dereference ``xfer'' after calling the callback as it might free it.
    let status = xfer.status.get();
    let flags = xfer.flags.get();
    let repeat = pipe.repeat.get() != 0;

    if repeat {
        if let Some(cb) = xfer.callback.get() {
            cb(xfer, xfer.priv_.get(), xfer.status.get());
        }
        (pipe.methods().done)(xfer);
    } else {
        (pipe.methods().done)(xfer);
        if let Some(cb) = xfer.callback.get() {
            cb(xfer, xfer.priv_.get(), xfer.status.get());
        }
    }

    if flags & USBD_SYNCHRONOUS != 0 && !polling {
        wakeup(ptr::from_ref(xfer));
    }

    if pipe.repeat.get() == 0 {
        // XXX should we stop the queue on all errors?
        if (status == USBD_CANCELLED || status == USBD_IOERROR || status == USBD_TIMEOUT)
            && pipe.iface.get().is_some()
        {
            // not control pipe
            pipe.running.set(0);
        } else {
            usbd_start_next(pipe);
        }
    }
}

/// `usb_insert_transfer`: queues `xfer` on its pipe; `USBD_NORMAL_COMPLETION` when the pipe
/// was idle (the caller starts it), `USBD_IN_PROGRESS` otherwise.
pub fn usb_insert_transfer(xfer: &'static UsbdXfer) -> UsbdStatus {
    let pipe = xfer.pipe();

    #[cfg(feature = "diagnostic")]
    {
        if xfer.busy_free.get() != XFER_FREE {
            printf(format_args!(
                "usb_insert_transfer: xfer={:p} not free\n",
                ptr::from_ref(xfer)
            ));
            return USBD_INVAL;
        }
        xfer.busy_free.set(XFER_ONQU);
    }
    let s = splusb();
    // SAFETY: an xfer is on at most one queue (busy_free in C); it stays allocated while
    // queued; under splusb() and the kernel lock.
    unsafe { pipe.queue.insert_tail(xfer) };
    let err = if pipe.running.get() != 0 {
        USBD_IN_PROGRESS
    } else {
        pipe.running.set(1);
        USBD_NORMAL_COMPLETION
    };
    splx(s);
    err
}

/// `usbd_start_next`: starts the next xfer queued on `pipe` (called at `splusb()`).
pub fn usbd_start_next(pipe: &'static UsbdPipe) {
    splsoftassert(IPL_SOFTUSB, "usbd_start_next");

    // Get next request in queue.
    match pipe.queue.first() {
        None => pipe.running.set(0),
        Some(xfer) => {
            // SAFETY: a queued xfer is allocated until it is completed.
            let xfer: &'static UsbdXfer = unsafe { &*ptr::from_ref(xfer) };
            let err = (pipe.methods().start)(xfer);
            if err != USBD_IN_PROGRESS {
                printf(format_args!("usbd_start_next: error={}\n", err as i32));
                pipe.running.set(0);
                // XXX do what?
            }
        }
    }
}

/// `usbd_do_request`: a synchronous control request on pipe 0; `data` is the data stage
/// (at least `wLength` bytes).
pub fn usbd_do_request(
    dev: &'static UsbdDevice,
    req: &UsbDeviceRequest,
    data: &mut [u8],
) -> UsbdStatus {
    usbd_do_request_flags(dev, req, data, 0, None, USBD_DEFAULT_TIMEOUT)
}

/// `usbd_do_request_flags`.
pub fn usbd_do_request_flags(
    dev: &'static UsbdDevice,
    req: &UsbDeviceRequest,
    data: &mut [u8],
    flags: u16,
    actlen: Option<&mut i32>,
    timeout: u32,
) -> UsbdStatus {
    #[cfg(feature = "diagnostic")]
    if dev.bus().intr_context.get() != 0 {
        printf(format_args!("usbd_do_request: not in process context\n"));
        return USBD_INVAL;
    }

    // If the bus is gone, don't go any further.
    if usbd_is_dying(dev) {
        return USBD_IOERROR;
    }

    let len = ugetw(req.wLength);
    if data.len() < usize::from(len) {
        return USBD_INVAL;
    }

    let Some(xfer) = usbd_alloc_xfer(dev) else {
        return USBD_NOMEM;
    };
    // SAFETY: `data` is borrowed for this whole synchronous call and has `wLength` bytes
    // (checked above); the transfer completes before `usbd_transfer` returns.
    unsafe {
        usbd_setup_default_xfer(
            xfer,
            dev,
            ptr::null_mut(),
            timeout,
            req,
            data.as_mut_ptr(),
            u32::from(len),
            flags | USBD_SYNCHRONOUS,
            None,
        )
    };
    let err = usbd_transfer(xfer);
    if let Some(a) = actlen {
        *a = xfer.actlen.get() as i32;
    }
    if err == USBD_STALLED {
        // The control endpoint has stalled. Control endpoints should not halt, but some
        // may do so anyway so clear any halt condition.
        let mut status = UsbStatus::zeroed();
        let mut treq = UsbDeviceRequest {
            bmRequestType: UT_READ_ENDPOINT,
            bRequest: UR_GET_STATUS,
            ..UsbDeviceRequest::default()
        };
        usetw(&mut treq.wValue, 0);
        usetw(&mut treq.wIndex, 0);
        usetw(&mut treq.wLength, size_of::<UsbStatus>() as u16);
        // SAFETY: `status` lives across the synchronous transfer.
        unsafe {
            usbd_setup_default_xfer(
                xfer,
                dev,
                ptr::null_mut(),
                USBD_DEFAULT_TIMEOUT,
                &treq,
                status.as_bytes_mut().as_mut_ptr(),
                size_of::<UsbStatus>() as u32,
                USBD_SYNCHRONOUS,
                None,
            )
        };
        'bad: {
            let nerr = usbd_transfer(xfer);
            if nerr.is_err() {
                break 'bad;
            }
            let s = ugetw(status.wStatus);
            if s & UES_HALT == 0 {
                break 'bad;
            }
            treq.bmRequestType = UT_WRITE_ENDPOINT;
            treq.bRequest = UR_CLEAR_FEATURE;
            usetw(&mut treq.wValue, UF_ENDPOINT_HALT);
            usetw(&mut treq.wIndex, 0);
            usetw(&mut treq.wLength, 0);
            // SAFETY: as above; no data stage.
            unsafe {
                usbd_setup_default_xfer(
                    xfer,
                    dev,
                    ptr::null_mut(),
                    USBD_DEFAULT_TIMEOUT,
                    &treq,
                    status.as_bytes_mut().as_mut_ptr(),
                    0,
                    USBD_SYNCHRONOUS,
                    None,
                )
            };
            let _ = usbd_transfer(xfer);
        }
    }

    // bad:
    // SAFETY: our own xfer, completed (synchronous), not used again.
    unsafe { usbd_free_xfer(NonNull::from(xfer)) };
    err
}

/// `usbd_request_async_cb`: frees the xfer of a fire-and-forget request.
pub fn usbd_request_async_cb(xfer: &'static UsbdXfer, _priv: *mut c_void, _status: UsbdStatus) {
    // SAFETY: the xfer of `usbd_request_async` belongs to this callback, which is its last
    // user (`usb_transfer_complete` does not touch it after the callback).
    unsafe { usbd_free_xfer(NonNull::from(xfer)) };
}

/// `usbd_request_async`: execute a request without waiting for completion. Can be used from
/// interrupt context. The xfer is freed when it completes (by `callback`, or by
/// `usbd_request_async_cb` when it is `None`).
pub fn usbd_request_async(
    xfer: &'static UsbdXfer,
    req: &UsbDeviceRequest,
    priv_: *mut c_void,
    callback: Option<UsbdCallback>,
) -> UsbdStatus {
    let callback = callback.unwrap_or(usbd_request_async_cb);

    // SAFETY: no buffer (`USBD_NO_COPY`): the data stage stays in the DMA buffer.
    unsafe {
        usbd_setup_default_xfer(
            xfer,
            xfer.device(),
            ptr::null_mut(),
            USBD_DEFAULT_TIMEOUT,
            req,
            ptr::null_mut(),
            u32::from(ugetw(req.wLength)),
            USBD_NO_COPY,
            Some(callback),
        )
    };
    // `priv` is only handed back to the callback.
    xfer.priv_.set(priv_);
    let err = usbd_transfer(xfer);
    if err != USBD_IN_PROGRESS {
        // SAFETY: the transfer did not start, so the xfer is still ours alone.
        unsafe { usbd_free_xfer(NonNull::from(xfer)) };
        return err;
    }
    USBD_NORMAL_COMPLETION
}

/// `usbd_get_quirks`.
pub fn usbd_get_quirks(dev: &UsbdDevice) -> &'static UsbdQuirks {
    dev.quirks()
}

/* XXX do periodic free() of free list */

/// `usbd_dopoll`: called from keyboard driver when in polling mode.
pub fn usbd_dopoll(udev: &UsbdDevice) {
    let bus = udev.bus();
    (bus.methods().do_poll)(bus);
}

/// `usbd_set_polling`.
pub fn usbd_set_polling(dev: &UsbdDevice, on: bool) {
    let bus: &'static UsbdBus = dev.bus();
    if on {
        bus.use_polling.set(bus.use_polling.get().wrapping_add(1));
    } else {
        bus.use_polling.set(bus.use_polling.get().wrapping_sub(1));
    }
    // When polling we need to make sure there is nothing pending to do.
    if bus.use_polling.get() != 0 {
        (bus.methods().soft_intr)(ptr::from_ref(bus).cast_mut().cast());
    }
}

/// `usbd_get_endpoint_descriptor`: a copy of the interface's endpoint descriptor for
/// `address`.
pub fn usbd_get_endpoint_descriptor(
    iface: &UsbdInterface,
    address: u8,
) -> Option<UsbEndpointDescriptor> {
    let endpoints = iface.endpoints();
    for i in 0..usize::from(iface.idesc().bNumEndpoints) {
        let ep = endpoints.get(i)?;
        let ed = ep.edesc();
        if ed.bEndpointAddress == address {
            return Some(ed);
        }
    }
    None
}

/// `usbd_ratecheck()` can limit the number of error messages that occurs. When a device is
/// unplugged it may take up to 0.25s for the hub driver to notice it. If the driver
/// continuously tries to do I/O operations this can generate a large number of messages.
pub fn usbd_ratecheck(last: &mut Timeval) -> bool {
    // 0.25 s
    let errinterval = Timeval {
        tv_sec: 0,
        tv_usec: 250000,
    };
    ratecheck(last, &errinterval)
}

/// `usbd_match_device`: search for a vendor/product pair in an array whose entries are, or
/// begin with, a [`UsbDevno`].
pub fn usbd_match_device<T: AsRef<UsbDevno>>(tbl: &[T], vendor: u16, product: u16) -> Option<&T> {
    tbl.iter().find(|e| {
        let d = e.as_ref();
        d.ud_vendor == vendor && (d.ud_product == product || d.ud_product == USB_PRODUCT_ANY)
    })
}

/// `usb_lookup(tbl, vendor, product)`.
pub fn usb_lookup<T: AsRef<UsbDevno>>(tbl: &[T], vendor: u16, product: u16) -> Option<&T> {
    usbd_match_device(tbl, vendor, product)
}

/// `usbd_desc_iter_init`: an iterator over the device's configuration descriptor.
pub fn usbd_desc_iter_init(dev: &UsbdDevice) -> UsbdDescIter<'static> {
    let buf = match (dev.cdesc.get(), dev.cdesc()) {
        (Some(b), Some(cd)) => &b[..usize::from(ugetw(cd.wTotalLength)).min(b.len())],
        _ => &[],
    };
    UsbdDescIter { buf, cur: 0 }
}

impl<'a> UsbdDescIter<'a> {
    /// An iterator over the descriptors in `buf` (the C's `cur`/`end` pair).
    pub fn new(buf: &'a [u8]) -> Self {
        Self { buf, cur: 0 }
    }
}

/// `usbd_desc_iter_next`.
pub fn usbd_desc_iter_next<'a>(iter: &mut UsbdDescIter<'a>) -> Option<&'a UsbDescriptor> {
    let end = iter.buf.len();
    if iter.cur + size_of::<UsbDescriptor>() >= end {
        if iter.cur != end {
            printf(format_args!("usbd_desc_iter_next: bad descriptor\n"));
        }
        return None;
    }
    let desc = usb_wire_at::<UsbDescriptor>(iter.buf, iter.cur)?;
    if desc.bLength == 0 {
        printf(format_args!("usbd_desc_iter_next: descriptor length = 0\n"));
        return None;
    }
    iter.cur += usize::from(desc.bLength);
    if iter.cur > end {
        printf(format_args!(
            "usbd_desc_iter_next: descriptor length too large\n"
        ));
        return None;
    }
    Some(desc)
}

/// `usbd_str`: builds a string descriptor of `l` bytes from the ASCII string `s`; returns
/// its length.
pub fn usbd_str(p: &mut UsbStringDescriptor, l: i32, s: &[u8]) -> i32 {
    let mut l = l;
    if l == 0 {
        return 0;
    }
    let slen = s.iter().position(|&b| b == 0).unwrap_or(s.len());
    p.bLength = (2 * slen + 2) as u8;
    if l == 1 {
        return 1;
    }
    p.bDescriptorType = UDESC_STRING;
    l -= 2;
    let mut i = 0;
    while i < slen && l > 1 {
        let Some(w) = p.bString.get_mut(i) else {
            break;
        };
        usetw2(w, 0, s[i]);
        i += 1;
        l -= 2;
    }
    (2 * i + 2) as i32
}
/* </CODE> */
