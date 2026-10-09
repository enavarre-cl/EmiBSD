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
//! ugen(4): the generic USB driver: `/dev/ugen*` gives a program the endpoints of a device no
//! other driver claimed, and `/dev/ugenN.00` the device's control pipe with the descriptor
//! and request ioctls.
//!
//! Upstream: sys/dev/usb/ugen.c @ 3ce1f3f79392
//!
//! `ugen_match` takes a device only when the generic fallback is on (`usegeneric`:
//! `usbd_probe_and_attach` offers the device last, once no driver took it or its interfaces).
//! `ugen_attach` selects configuration index 0 of a device nobody else uses, and
//! `ugen_set_config` records the endpoint descriptors of every unclaimed interface in
//! `sc_endpoints[number][direction]`. The minor number is `unit << 4 | endpoint`:
//! `ugenopen` opens the pipes the open flags ask for (interrupt-in endpoints feed a clist
//! through `ugenintr`; isochronous-in ones keep `UGEN_NISOREQS` transfers in flight that fill
//! a ring, `ugen_isoc_rintr`; bulk ones transfer synchronously in `ugen_do_read` and
//! `ugen_do_write`).
//!
//! ## Deviations
//! - `UGEN_DEBUG` is not in GENERIC: the `DPRINTF`s are absent.
//! - `sce->edesc` points into the interface in C; `usbd_interface2endpoint_descriptor`
//!   returns a copy here, so `edesc` is an `Option` of the descriptor (`None` for the
//!   C's `NULL`).
//! - The isochronous ring keeps `fill` and `cur` as offsets into `ibuf` (the C's pointers;
//!   `limit` is `ibuflen`). When the ring is full, `ugen_isoc_rintr` drops the oldest data;
//!   the C wraps `cur` with `ibuf + (limit - cur)`, an upstream bug that points before the
//!   buffer (it means `cur - limit`): here it is `cur - limit`.
//! - `ugen_set_config` clears the fields of the endpoints the C zeroes with `memset`
//!   (descriptor, interface, pipe, buffers, state, timeout), but not the clists and knote
//!   lists, which are empty when no endpoint is open. A missing endpoint descriptor in an
//!   interface is skipped (the C would dereference NULL).
//! - The interrupt `ugen_do_write` clamps the chunk to the 1024-byte buffer (`wMaxPacketSize`
//!   could be larger with high-bandwidth bits). The failure path of the isochronous open keeps
//!   the C's leak of the pipe and the ring.
//! - The device switch entries are Rust functions of the types `Cdevsw` takes. `ugen_do_ioctl`
//!   takes the argument as bytes (`ioctl_arg`, `ioctl_ret`); `USB_GET_FULL_DESC` and
//!   `USB_DO_REQUEST` move the data with `uiomove` over the user address, as `usbioctl` does.
//! - `ugenread`, `ugenwrite`, `ugenioctl` and `ugenkqfilter` answer `ENXIO` for a unit that
//!   is not attached (the C dereferences `cd_devs[unit]`).

use core::cell::Cell;
use core::ffi::c_void;
use core::ptr::{self, NonNull};
use core::slice;

use crate::dev::usb::usb::{
    UE_BULK, UE_CONTROL, UE_DIR_IN, UE_INTERRUPT, UE_ISOCHRONOUS, UF_ENDPOINT_HALT, UR_SET_ADDRESS,
    UR_SET_CONFIG, UR_SET_INTERFACE, USB_CONTROL_ENDPOINT, USB_CURRENT_ALT_INDEX,
    USB_CURRENT_CONFIG_INDEX, USB_DO_REQUEST, USB_GET_ALTINTERFACE, USB_GET_CONFIG,
    USB_GET_CONFIG_DESC, USB_GET_DEVICE_DESC, USB_GET_DEVICEINFO, USB_GET_ENDPOINT_DESC,
    USB_GET_FULL_DESC, USB_GET_INTERFACE_DESC, USB_GET_NO_ALT, USB_MAX_ENDPOINTS,
    USB_SET_ALTINTERFACE, USB_SET_CONFIG, USB_SET_SHORT_XFER, USB_SET_TIMEOUT, USBD_SHORT_XFER_OK,
    UT_READ, UT_WRITE_DEVICE, UT_WRITE_INTERFACE, UsbAltInterface, UsbConfigDesc,
    UsbConfigDescriptor, UsbCtlRequest, UsbDeviceInfo, UsbEndpointDesc, UsbEndpointDescriptor,
    UsbFullDesc, UsbInterfaceDesc, UsbWire, ue_get_addr, ue_get_dir, ue_get_xfertype, ugetw,
};
use crate::dev::usb::usb_subr::{
    usbd_fill_deviceinfo, usbd_find_edesc, usbd_find_idesc, usbd_get_cdesc, usbd_set_config_index,
    usbd_set_config_no,
};
use crate::dev::usb::usbdi::{
    UMATCH_GENERIC, UMATCH_NONE, USBD_CATCH, USBD_DEFAULT_INTERVAL, USBD_DEFAULT_TIMEOUT,
    USBD_IN_USE, USBD_INTERRUPTED, USBD_INVAL, USBD_NO_COPY, USBD_NO_TIMEOUT,
    USBD_NORMAL_COMPLETION, USBD_SYNCHRONOUS, USBD_TIMEOUT, UsbAttachArg, UsbdStatus, splusb,
    usbd_abort_pipe, usbd_alloc_buffer, usbd_alloc_xfer, usbd_clear_endpoint_stall,
    usbd_clear_endpoint_stall_async, usbd_clear_endpoint_toggle, usbd_close_pipe, usbd_deactivate,
    usbd_device2interface_handle, usbd_do_request_flags, usbd_free_xfer,
    usbd_get_config_descriptor, usbd_get_devcnt, usbd_get_device_descriptor,
    usbd_get_interface_altindex, usbd_get_interface_descriptor, usbd_get_no_alts,
    usbd_get_xfer_status, usbd_iface_claimed, usbd_interface2endpoint_descriptor, usbd_is_dying,
    usbd_open_pipe, usbd_open_pipe_intr, usbd_set_interface, usbd_setup_isoc_xfer, usbd_setup_xfer,
    usbd_transfer,
};
use crate::dev::usb::usbdi_util::{
    usb_detach_wait, usb_detach_wakeup, usbd_clear_endpoint_feature, usbd_get_config,
};
use crate::dev::usb::usbdivar::{UsbdInterface, UsbdPipe, UsbdXfer};
use crate::kern::kern_event::{
    klist_insert_locked, klist_invalidate, klist_remove_locked, seltrue_kqfilter,
};
use crate::kern::kern_malloc::{free, malloc, mallocarray};
use crate::kern::kern_subr::uiomove;
use crate::kern::kern_synch::{tsleep_nsec, wakeup};
use crate::kern::subr_prf::printf;
use crate::kern::sys_generic::selwakeup;
use crate::kern::tty_subr::{b_to_q, clalloc, clfree, ndflush, q_to_b};
use crate::kern::vfs_subr::vdevgone;
use crate::machine::conf::{cdevsw, nchrdev};
use crate::machine::intr::splx;
use crate::sys::conf::DevTypeOpen;
use crate::sys::device::{CfMatch, Cfattach, Cfdriver, DV_DULL, Device, Softc};
use crate::sys::errno::Errno;
use crate::sys::event::{EVFILT_READ, EVFILT_WRITE, FILTEROP_ISFD, Filterops, Knote};
use crate::sys::fcntl::{FREAD, FWRITE};
use crate::sys::ioctl::{ioctl_arg, ioctl_ret};
use crate::sys::malloc::{M_NOWAIT, M_TEMP, M_USBDEV, M_WAITOK};
use crate::sys::param::{PCATCH, PZERO};
use crate::sys::proc::Proc;
use crate::sys::selinfo::Selinfo;
use crate::sys::time::msec_to_nsec;
use crate::sys::tty::Clist;
use crate::sys::types::{Dev, minor};
use crate::sys::uio::{Iovec, Uio, UioRw, UioSeg};
use crate::sys::vnode::{IO_NDELAY, VCHR};

/// `UGEN_CHUNK`: chunk size for read.
pub const UGEN_CHUNK: usize = 128;
/// `UGEN_IBSIZE`: buffer size.
pub const UGEN_IBSIZE: i32 = 1020;
/// `UGEN_BBSIZE`.
pub const UGEN_BBSIZE: usize = 1024;

/// `UGEN_NISOFRAMES`: 0.5 seconds worth.
pub const UGEN_NISOFRAMES: usize = 500;
/// `UGEN_NISOREQS`: number of outstanding xfer requests.
pub const UGEN_NISOREQS: usize = 6;
/// `UGEN_NISORFRMS`: number of frames (milliseconds) per req.
pub const UGEN_NISORFRMS: usize = 4;

/// `UGEN_ASLP`: waiting for data (`state`).
pub const UGEN_ASLP: i32 = 0x02;
/// `UGEN_SHORT_OK`: short xfers are OK (`state`).
pub const UGEN_SHORT_OK: i32 = 0x04;

/// `OUT`: the index of an endpoint's output direction in `sc_endpoints`.
pub const OUT: usize = 0;
/// `IN`: the index of an endpoint's input direction in `sc_endpoints`.
pub const IN: usize = 1;

/// `struct isoreq`: one outstanding isochronous transfer.
pub struct Isoreq {
    /// `sce`: its endpoint.
    pub sce: Cell<*const UgenEndpoint>,
    /// `xfer`.
    pub xfer: Cell<Option<&'static UsbdXfer>>,
    /// `dmabuf`: the transfer's buffer.
    pub dmabuf: Cell<*mut u8>,
    /// `sizes`: the frame lengths, updated by the controller.
    pub sizes: Cell<[u16; UGEN_NISORFRMS]>,
}

/// `struct ugen_endpoint`: one direction of an endpoint. All-zero is a valid value.
pub struct UgenEndpoint {
    /// `sc`.
    pub sc: Cell<*const UgenSoftc>,
    /// `edesc`: the endpoint descriptor, `None` where the device has no such endpoint.
    pub edesc: Cell<Option<UsbEndpointDescriptor>>,
    /// `iface`.
    pub iface: Cell<Option<&'static UsbdInterface>>,
    /// `state`: `UGEN_ASLP`, `UGEN_SHORT_OK`.
    pub state: Cell<i32>,
    /// `pipeh`.
    pub pipeh: Cell<Option<&'static UsbdPipe>>,
    /// `q`: the input of an interrupt endpoint.
    pub q: Clist,
    /// `rsel`.
    pub rsel: Selinfo,
    /// `ibuf`: start of buffer (circular for isoc).
    pub ibuf: Cell<*mut u8>,
    /// `ibuflen`.
    pub ibuflen: Cell<usize>,
    /// `fill`: location for input (isoc), as an offset into `ibuf`.
    pub fill: Cell<usize>,
    /// `cur`: current read location (isoc), as an offset into `ibuf`.
    pub cur: Cell<usize>,
    /// `timeout`, in milliseconds.
    pub timeout: Cell<u32>,
    /// `isoreqs`.
    pub isoreqs: [Isoreq; UGEN_NISOREQS],
}

impl UgenEndpoint {
    /// `limit`: the end of the circular buffer, as an offset (`ibuflen`).
    fn limit(&self) -> usize {
        self.ibuflen.get()
    }
}

/// `struct ugen_softc`.
#[repr(C)]
pub struct UgenSoftc {
    /// `sc_dev`: base device.
    pub sc_dev: Device,
    /// `sc_udev`.
    pub sc_udev: Cell<Option<&'static crate::dev::usb::usbdivar::UsbdDevice>>,

    /// `sc_is_open`.
    pub sc_is_open: [Cell<u8>; USB_MAX_ENDPOINTS],
    /// `sc_endpoints`: `[number][OUT or IN]`.
    pub sc_endpoints: [[UgenEndpoint; 2]; USB_MAX_ENDPOINTS],

    /// `sc_refcnt`.
    pub sc_refcnt: Cell<i32>,
    /// `sc_secondary`.
    pub sc_secondary: Cell<u8>,
}

// SAFETY: `#[repr(C)]`, the `Device` first; every other member is a `Cell` of an integer,
// a raw pointer, an `Option` of a reference or a descriptor, or a `Clist` (null ring) or
// `Selinfo` (no knotes): all valid as zero bits.
unsafe impl Softc for UgenSoftc {}

/// `UGENUNIT(n)`.
pub const fn ugenunit(n: Dev) -> u32 {
    (minor(n) >> 4) & 0xf
}

/// `UGENENDPOINT(n)`.
pub const fn ugenendpoint(n: Dev) -> usize {
    (minor(n) & 0xf) as usize
}

/// `ugen_cd`.
pub static UGEN_CD: Cfdriver = Cfdriver::new(b"ugen", DV_DULL, 0);

/// `ugen_ca`.
pub static UGEN_CA: Cfattach = Cfattach {
    ca_devsize: size_of::<UgenSoftc>(),
    ca_match: Some(ugen_match),
    ca_attach: ugen_attach,
    ca_detach: Some(ugen_detach),
    ca_activate: None,
};

/// `NUGEN`: `ugen* at uhub?` is configured (`needs-count`; GENERIC has it).
pub const NUGEN: i32 = 1;

/// `(struct ugen_softc *)self`.
fn ugen_softc(self_: &Device) -> &'static UgenSoftc {
    // SAFETY: only called with devices `ugen_ca` made (ugen's own entry points); an attached
    // device lives until `config_detach` frees it after `ugen_detach`.
    unsafe { &*ptr::from_ref(self_.softc::<UgenSoftc>()) }
}

/// `ugen_cd.cd_devs[unit]`: the attached ugen of a unit.
pub fn ugen_unit(unit: u32) -> Option<&'static UgenSoftc> {
    let unit = unit as i32;
    if unit >= UGEN_CD.cd_ndevs.get() {
        return None;
    }
    let dv = UGEN_CD.cd_dev(unit)?;
    // SAFETY: an attached ugen, in `cd_devs` until `config_detach`.
    Some(ugen_softc(unsafe { dv.as_ref() }))
}

/// `usbd_is_dying(sc->sc_udev)`; a device whose `sc_udev` is not set yet counts as dying.
fn ugen_dying(sc: &UgenSoftc) -> bool {
    match sc.sc_udev.get() {
        Some(udev) => usbd_is_dying(udev),
        None => true,
    }
}

/// `sc->sc_udev`. Callers check [`ugen_dying`] first.
fn udev_of(sc: &UgenSoftc) -> &'static crate::dev::usb::usbdivar::UsbdDevice {
    match sc.sc_udev.get() {
        Some(d) => d,
        None => crate::kern::subr_prf::panic(format_args!("{}: no USB device", sc.sc_dev.xname())),
    }
}

/// `ugen_match`: any device, when nothing else wanted it (`usegeneric`).
pub fn ugen_match(_parent: Option<&Device>, _match: &CfMatch, aux: *mut c_void) -> i32 {
    // SAFETY: `usbd_probe_and_attach` passes a `UsbAttachArg` that lives across the
    // `config_found_sm` calling this.
    let uaa = unsafe { &*aux.cast::<UsbAttachArg>() };

    if uaa.usegeneric != 0 {
        UMATCH_GENERIC
    } else {
        UMATCH_NONE
    }
}

/// `ugen_attach`.
pub fn ugen_attach(_parent: Option<&Device>, self_: &Device, aux: *mut c_void) {
    let sc = ugen_softc(self_);
    // SAFETY: as in `ugen_match`, valid during the attach.
    let uaa = unsafe { &*aux.cast::<UsbAttachArg>() };

    let udev = uaa.device();
    sc.sc_udev.set(Some(udev));

    if usbd_get_devcnt(udev) > 0 {
        sc.sc_secondary.set(1);
    }

    if sc.sc_secondary.get() == 0 {
        // First set configuration index 0, the default one for ugen.
        let err = usbd_set_config_index(udev, 0, false);
        if err.is_err() {
            printf(format_args!(
                "{}: setting configuration index 0 failed\n",
                sc.sc_dev.xname()
            ));
            usbd_deactivate(udev);
            return;
        }
    }
    let Some(cdesc) = usbd_get_config_descriptor(udev) else {
        usbd_deactivate(udev);
        return;
    };
    let conf = cdesc.bConfigurationValue;

    // Set up all the local state for this configuration.
    let err = ugen_set_config(sc, i32::from(conf));
    if err.is_err() {
        printf(format_args!(
            "{}: setting configuration {} failed\n",
            sc.sc_dev.xname(),
            conf
        ));
        usbd_deactivate(udev);
    }
}

/// `ugen_set_config`: select configuration `configno` and record its unclaimed endpoints.
pub fn ugen_set_config(sc: &UgenSoftc, configno: i32) -> UsbdStatus {
    let dev = udev_of(sc);

    // We start at 1, not 0, because we don't care whether the control endpoint is open or
    // not. It is always present.
    for endptno in 1..USB_MAX_ENDPOINTS {
        if sc.sc_is_open[endptno].get() != 0 {
            return USBD_IN_USE;
        }
    }

    // Avoid setting the current value.
    let mut cdesc = usbd_get_config_descriptor(dev);
    if cdesc.is_none_or(|c| i32::from(c.bConfigurationValue) != configno) {
        if sc.sc_secondary.get() != 0 {
            printf(format_args!(
                "ugen_set_config: secondary, not changing config to {}\n",
                configno
            ));
            return USBD_IN_USE;
        } else {
            let err = usbd_set_config_no(dev, configno, true);
            if err.is_err() {
                return err;
            }
            cdesc = usbd_get_config_descriptor(dev);
            if cdesc.is_none_or(|c| i32::from(c.bConfigurationValue) != configno) {
                return USBD_INVAL;
            }
        }
    }
    let Some(cdesc) = cdesc else {
        return USBD_INVAL;
    };

    // memset(sc->sc_endpoints, 0, sizeof sc->sc_endpoints)
    for ends in &sc.sc_endpoints {
        for sce in ends {
            sce.sc.set(ptr::null());
            sce.edesc.set(None);
            sce.iface.set(None);
            sce.state.set(0);
            sce.pipeh.set(None);
            sce.ibuf.set(ptr::null_mut());
            sce.ibuflen.set(0);
            sce.fill.set(0);
            sce.cur.set(0);
            sce.timeout.set(0);
            for req in &sce.isoreqs {
                req.sce.set(ptr::null());
                req.xfer.set(None);
                req.dmabuf.set(ptr::null_mut());
                req.sizes.set([0; UGEN_NISORFRMS]);
            }
        }
    }
    for ifaceno in 0..cdesc.bNumInterfaces {
        if usbd_iface_claimed(dev, usize::from(ifaceno)) {
            continue;
        }
        let iface = match usbd_device2interface_handle(dev, ifaceno) {
            Ok(i) => i,
            Err(err) => return err,
        };
        let Some(id) = usbd_get_interface_descriptor(iface) else {
            continue;
        };
        for endptno in 0..id.bNumEndpoints {
            let Some(ed) = usbd_interface2endpoint_descriptor(iface, endptno) else {
                continue;
            };
            let endpt = ed.bEndpointAddress;
            let dir = if ue_get_dir(endpt) == UE_DIR_IN {
                IN
            } else {
                OUT
            };
            let sce = &sc.sc_endpoints[usize::from(ue_get_addr(endpt))][dir];
            sce.sc.set(ptr::from_ref(sc));
            sce.edesc.set(Some(ed));
            sce.iface.set(Some(iface));
        }
    }
    USBD_NORMAL_COMPLETION
}

/// `ugenopen`.
pub fn ugenopen(dev: Dev, flag: i32, _mode: i32, _p: &Proc) -> Result<(), Errno> {
    let unit = ugenunit(dev);
    let endpt = ugenendpoint(dev);

    let Some(sc) = ugen_unit(unit) else {
        return Err(Errno::ENXIO);
    };

    if ugen_dying(sc) {
        return Err(Errno::ENXIO);
    }

    if sc.sc_is_open[endpt].get() != 0 {
        return Err(Errno::EBUSY);
    }

    if endpt == usize::from(USB_CONTROL_ENDPOINT) {
        sc.sc_is_open[usize::from(USB_CONTROL_ENDPOINT)].set(1);
        return Ok(());
    }

    // Make sure there are pipes for all directions.
    for dir in OUT..=IN {
        if flag & (if dir == OUT { FWRITE } else { FREAD }) != 0 {
            let sce = &sc.sc_endpoints[endpt][dir];
            if sce.edesc.get().is_none() {
                return Err(Errno::ENXIO);
            }
        }
    }

    // Actually open the pipes.
    // XXX Should back out properly if it fails.
    for dir in OUT..=IN {
        if flag & (if dir == OUT { FWRITE } else { FREAD }) == 0 {
            continue;
        }
        let sce = &sc.sc_endpoints[endpt][dir];
        sce.state.set(0);
        sce.timeout.set(USBD_NO_TIMEOUT);
        let (Some(edesc), Some(iface)) = (sce.edesc.get(), sce.iface.get()) else {
            return Err(Errno::ENXIO);
        };
        // Clear device endpoint toggle.
        ugen_clear_iface_eps(sc, iface);
        match ue_get_xfertype(edesc.bmAttributes) {
            UE_INTERRUPT => {
                if dir == OUT {
                    match usbd_open_pipe(iface, edesc.bEndpointAddress, 0) {
                        Ok(p) => sce.pipeh.set(Some(p)),
                        Err(_) => return Err(Errno::EIO),
                    }
                    continue;
                }
                let isize = usize::from(ugetw(edesc.wMaxPacketSize));
                if isize == 0 {
                    // shouldn't happen
                    return Err(Errno::EINVAL);
                }
                sce.ibuflen.set(isize);
                let Some(ibuf) = malloc(isize, M_USBDEV, M_WAITOK) else {
                    return Err(Errno::ENOMEM);
                };
                sce.ibuf.set(ibuf.as_ptr());
                clalloc(&sce.q, UGEN_IBSIZE, false);
                // SAFETY: `ibuf` is `isize` bytes, freed only after the pipe is closed
                // (`ugen_do_close`, or just below on failure); the pipe's `priv` is the
                // endpoint, inside the softc, which outlives the pipe.
                let r = unsafe {
                    usbd_open_pipe_intr(
                        iface,
                        edesc.bEndpointAddress,
                        USBD_SHORT_XFER_OK as u8,
                        ptr::from_ref(sce).cast_mut().cast(),
                        ibuf.as_ptr(),
                        isize as u32,
                        ugenintr,
                        USBD_DEFAULT_INTERVAL,
                    )
                };
                match r {
                    Ok(p) => sce.pipeh.set(Some(p)),
                    Err(_) => {
                        free(ibuf, M_USBDEV, sce.ibuflen.get());
                        sce.ibuf.set(ptr::null_mut());
                        clfree(&sce.q);
                        return Err(Errno::EIO);
                    }
                }
                // Clear HC endpoint toggle.
                if let Some(p) = sce.pipeh.get() {
                    usbd_clear_endpoint_toggle(p);
                }
            }
            UE_BULK => {
                match usbd_open_pipe(iface, edesc.bEndpointAddress, 0) {
                    Ok(p) => sce.pipeh.set(Some(p)),
                    Err(_) => return Err(Errno::EIO),
                }
                // Clear HC endpoint toggle.
                if let Some(p) = sce.pipeh.get() {
                    usbd_clear_endpoint_toggle(p);
                }
            }
            UE_ISOCHRONOUS => {
                if dir == OUT {
                    return Err(Errno::EINVAL);
                }
                let isize = usize::from(ugetw(edesc.wMaxPacketSize));
                if isize == 0 {
                    // shouldn't happen
                    return Err(Errno::EINVAL);
                }
                sce.ibuflen.set(isize * UGEN_NISOFRAMES);
                let Some(ibuf) = mallocarray(isize, UGEN_NISOFRAMES, M_USBDEV, M_WAITOK) else {
                    return Err(Errno::ENOMEM);
                };
                sce.ibuf.set(ibuf.as_ptr());
                sce.cur.set(0);
                sce.fill.set(0);
                let pipe = match usbd_open_pipe(iface, edesc.bEndpointAddress, 0) {
                    Ok(p) => p,
                    Err(_) => {
                        free(ibuf, M_USBDEV, sce.ibuflen.get());
                        sce.ibuf.set(ptr::null_mut());
                        return Err(Errno::EIO);
                    }
                };
                sce.pipeh.set(Some(pipe));
                let mut nalloc = 0;
                let mut failed = false;
                for i in 0..UGEN_NISOREQS {
                    let req = &sce.isoreqs[i];
                    req.sce.set(ptr::from_ref(sce));
                    let Some(xfer) = usbd_alloc_xfer(udev_of(sc)) else {
                        failed = true;
                        break;
                    };
                    req.xfer.set(Some(xfer));
                    nalloc = i + 1;
                    let Some(buf) = usbd_alloc_buffer(xfer, (isize * UGEN_NISORFRMS) as u32) else {
                        failed = true;
                        break;
                    };
                    req.dmabuf.set(buf.as_ptr());
                    req.sizes.set([isize as u16; UGEN_NISORFRMS]);
                    // SAFETY: `req` is inside the endpoint, in the softc, which outlives the
                    // pipe; the frame lengths are its `sizes`, which only the controller and
                    // `ugen_isoc_rintr` touch while the transfer is in flight.
                    unsafe {
                        usbd_setup_isoc_xfer(
                            xfer,
                            pipe,
                            ptr::from_ref(req).cast_mut().cast(),
                            req.sizes.as_ptr().cast::<u16>(),
                            UGEN_NISORFRMS as u32,
                            USBD_NO_COPY | USBD_SHORT_XFER_OK,
                            Some(ugen_isoc_rintr),
                        )
                    };
                    let _ = usbd_transfer(xfer);
                }
                if failed {
                    // bad: implicit buffer free
                    for req in sce.isoreqs.iter().take(nalloc).rev() {
                        if let Some(x) = req.xfer.take() {
                            // SAFETY: allocated just above, never started or aborted with
                            // the pipe still open: ours to free, as the C does.
                            unsafe { usbd_free_xfer(NonNull::from(x)) };
                        }
                    }
                    return Err(Errno::ENOMEM);
                }
            }
            UE_CONTROL => {
                sce.timeout.set(USBD_DEFAULT_TIMEOUT);
                return Err(Errno::EINVAL);
            }
            _ => {}
        }
    }
    sc.sc_is_open[endpt].set(1);
    Ok(())
}

/// `ugenclose`.
pub fn ugenclose(dev: Dev, flag: i32, _mode: i32, _p: Option<&Proc>) -> Result<(), Errno> {
    let endpt = ugenendpoint(dev);
    let Some(sc) = ugen_unit(ugenunit(dev)) else {
        return Err(Errno::EIO);
    };

    if ugen_dying(sc) {
        return Err(Errno::EIO);
    }

    sc.sc_refcnt.set(sc.sc_refcnt.get() + 1);
    let error = ugen_do_close(sc, endpt, flag);
    sc.sc_refcnt.set(sc.sc_refcnt.get() - 1);
    if sc.sc_refcnt.get() < 0 {
        usb_detach_wakeup(&sc.sc_dev);
    }

    error
}

/// `ugen_do_close`.
pub fn ugen_do_close(sc: &UgenSoftc, endpt: usize, flag: i32) -> Result<(), Errno> {
    #[cfg(feature = "diagnostic")]
    if sc.sc_is_open[endpt].get() == 0 {
        printf(format_args!("ugenclose: not open\n"));
        return Err(Errno::EINVAL);
    }

    if endpt == usize::from(USB_CONTROL_ENDPOINT) {
        sc.sc_is_open[endpt].set(0);
        return Ok(());
    }

    for dir in OUT..=IN {
        if flag & (if dir == OUT { FWRITE } else { FREAD }) == 0 {
            continue;
        }
        let sce = &sc.sc_endpoints[endpt][dir];
        let Some(pipeh) = sce.pipeh.take() else {
            continue;
        };

        // SAFETY: the pipe `ugenopen` opened for this endpoint, taken out of `pipeh` so it
        // is not used again.
        let _ = unsafe { usbd_close_pipe(NonNull::from(pipeh)) };

        let Some(edesc) = sce.edesc.get() else {
            continue;
        };
        match ue_get_xfertype(edesc.bmAttributes) {
            UE_INTERRUPT => {
                ndflush(&sce.q, sce.q.c_cc.get());
                clfree(&sce.q);
            }
            UE_ISOCHRONOUS => {
                for req in &sce.isoreqs {
                    if let Some(x) = req.xfer.take() {
                        // SAFETY: the transfers `ugenopen` allocated, idle now that the pipe
                        // is closed.
                        unsafe { usbd_free_xfer(NonNull::from(x)) };
                    }
                }
            }
            _ => {}
        }

        if let Some(ibuf) = NonNull::new(sce.ibuf.replace(ptr::null_mut())) {
            free(ibuf, M_USBDEV, sce.ibuflen.get());
        }
    }
    sc.sc_is_open[endpt].set(0);

    Ok(())
}

/// The errno of a failed synchronous transfer, after clearing the endpoint's stall.
fn xfer_errno(pipe: &'static UsbdPipe, err: UsbdStatus) -> Errno {
    let _ = usbd_clear_endpoint_stall(pipe);
    if err == USBD_INTERRUPTED {
        Errno::EINTR
    } else if err == USBD_TIMEOUT {
        Errno::ETIMEDOUT
    } else {
        Errno::EIO
    }
}

/// `ugen_do_read`.
pub fn ugen_do_read(
    sc: &UgenSoftc,
    endpt: usize,
    uio: &mut Uio<'_>,
    flag: i32,
) -> Result<(), Errno> {
    let sce = &sc.sc_endpoints[endpt][IN];
    let mut error: Result<(), Errno> = Ok(());

    if ugen_dying(sc) {
        return Err(Errno::EIO);
    }

    if endpt == usize::from(USB_CONTROL_ENDPOINT) {
        return Err(Errno::ENODEV);
    }

    let Some(edesc) = sce.edesc.get() else {
        #[cfg(feature = "diagnostic")]
        printf(format_args!("ugenread: no edesc\n"));
        return Err(Errno::EIO);
    };
    let Some(pipeh) = sce.pipeh.get() else {
        #[cfg(feature = "diagnostic")]
        printf(format_args!("ugenread: no pipe\n"));
        return Err(Errno::EIO);
    };

    match ue_get_xfertype(edesc.bmAttributes) {
        UE_INTERRUPT => {
            let mut buffer = [0u8; UGEN_CHUNK];

            // Block until activity occurred.
            let s = splusb();
            while sce.q.c_cc.get() == 0 {
                if flag & IO_NDELAY != 0 {
                    splx(s);
                    return Err(Errno::EWOULDBLOCK);
                }
                sce.state.set(sce.state.get() | UGEN_ASLP);
                error = tsleep_nsec(
                    ptr::from_ref(sce),
                    PZERO | PCATCH,
                    "ugenrintr",
                    msec_to_nsec(u64::from(sce.timeout.get())),
                );
                sce.state.set(sce.state.get() & !UGEN_ASLP);
                if ugen_dying(sc) {
                    error = Err(Errno::EIO);
                }
                if error == Err(Errno::EWOULDBLOCK) {
                    // timeout, return 0
                    error = Ok(());
                    break;
                }
                if error.is_err() {
                    break;
                }
            }
            splx(s);

            // Transfer as many chunks as possible.
            while sce.q.c_cc.get() > 0 && uio.uio_resid > 0 && error.is_ok() {
                let n = (sce.q.c_cc.get() as usize)
                    .min(uio.uio_resid)
                    .min(buffer.len());

                // Remove a small chunk from the input queue.
                let _ = q_to_b(&sce.q, &mut buffer[..n]);

                // Copy the data to the user process.
                error = uiomove(&mut buffer[..n], uio);
                if error.is_err() {
                    break;
                }
            }
        }
        UE_BULK => {
            let mut buf = [0u8; UGEN_BBSIZE];
            let Some(xfer) = usbd_alloc_xfer(udev_of(sc)) else {
                return Err(Errno::ENOMEM);
            };
            let mut flags = USBD_SYNCHRONOUS;
            if sce.state.get() & UGEN_SHORT_OK != 0 {
                flags |= USBD_SHORT_XFER_OK;
            }
            if sce.timeout.get() == 0 {
                flags |= USBD_CATCH;
            }
            loop {
                let n = UGEN_BBSIZE.min(uio.uio_resid);
                if n == 0 {
                    break;
                }
                // SAFETY: `buf` outlives the synchronous transfer (it completes before
                // `usbd_transfer` returns).
                unsafe {
                    usbd_setup_xfer(
                        xfer,
                        pipeh,
                        ptr::null_mut(),
                        buf.as_mut_ptr(),
                        n as u32,
                        flags,
                        sce.timeout.get(),
                        None,
                    )
                };
                let err = usbd_transfer(xfer);
                if err.is_err() {
                    error = Err(xfer_errno(pipeh, err));
                    break;
                }
                let mut tn = 0u32;
                usbd_get_xfer_status(xfer, None, None, Some(&mut tn), None);
                let tn = (tn as usize).min(n);
                error = uiomove(&mut buf[..tn], uio);
                if error.is_err() || tn < n {
                    break;
                }
            }
            // SAFETY: allocated above and idle: the last transfer is done.
            unsafe { usbd_free_xfer(NonNull::from(xfer)) };
        }
        UE_ISOCHRONOUS => {
            let s = splusb();
            while sce.cur.get() == sce.fill.get() {
                if flag & IO_NDELAY != 0 {
                    splx(s);
                    return Err(Errno::EWOULDBLOCK);
                }
                sce.state.set(sce.state.get() | UGEN_ASLP);
                error = tsleep_nsec(
                    ptr::from_ref(sce),
                    PZERO | PCATCH,
                    "ugenriso",
                    msec_to_nsec(u64::from(sce.timeout.get())),
                );
                sce.state.set(sce.state.get() & !UGEN_ASLP);
                if ugen_dying(sc) {
                    error = Err(Errno::EIO);
                }
                if error == Err(Errno::EWOULDBLOCK) {
                    // timeout, return 0
                    error = Ok(());
                    break;
                }
                if error.is_err() {
                    break;
                }
            }

            while sce.cur.get() != sce.fill.get() && uio.uio_resid > 0 && error.is_ok() {
                let (cur, fill, limit) = (sce.cur.get(), sce.fill.get(), sce.limit());
                let n = if fill > cur {
                    (fill - cur).min(uio.uio_resid)
                } else {
                    (limit - cur).min(uio.uio_resid)
                };

                // Copy the data to the user process.
                let Some(ibuf) = NonNull::new(sce.ibuf.get()) else {
                    error = Err(Errno::EIO);
                    break;
                };
                // SAFETY: `ibuf` is `ibuflen` (= `limit`) bytes while the endpoint is open;
                // `cur + n <= limit` by the two cases above; `ugen_isoc_rintr` writes only
                // outside `cur..fill`... under the same spl, which is held here.
                let chunk = unsafe { slice::from_raw_parts_mut(ibuf.as_ptr().add(cur), n) };
                error = uiomove(chunk, uio);
                if error.is_err() {
                    break;
                }
                let mut cur = cur + n;
                if cur >= limit {
                    cur = 0;
                }
                sce.cur.set(cur);
            }
            splx(s);
        }
        _ => return Err(Errno::ENXIO),
    }
    error
}

/// `ugenread`.
pub fn ugenread(dev: Dev, uio: &mut Uio<'_>, flag: i32) -> Result<(), Errno> {
    let endpt = ugenendpoint(dev);
    let Some(sc) = ugen_unit(ugenunit(dev)) else {
        return Err(Errno::ENXIO);
    };

    sc.sc_refcnt.set(sc.sc_refcnt.get() + 1);
    let error = ugen_do_read(sc, endpt, uio, flag);
    sc.sc_refcnt.set(sc.sc_refcnt.get() - 1);
    if sc.sc_refcnt.get() < 0 {
        usb_detach_wakeup(&sc.sc_dev);
    }
    error
}

/// `ugen_do_write`.
pub fn ugen_do_write(
    sc: &UgenSoftc,
    endpt: usize,
    uio: &mut Uio<'_>,
    _flag: i32,
) -> Result<(), Errno> {
    let sce = &sc.sc_endpoints[endpt][OUT];
    let mut error: Result<(), Errno> = Ok(());
    let mut buf = [0u8; UGEN_BBSIZE];

    if ugen_dying(sc) {
        return Err(Errno::EIO);
    }

    if endpt == usize::from(USB_CONTROL_ENDPOINT) {
        return Err(Errno::ENODEV);
    }

    let Some(edesc) = sce.edesc.get() else {
        #[cfg(feature = "diagnostic")]
        printf(format_args!("ugenwrite: no edesc\n"));
        return Err(Errno::EIO);
    };
    let Some(pipeh) = sce.pipeh.get() else {
        #[cfg(feature = "diagnostic")]
        printf(format_args!("ugenwrite: no pipe\n"));
        return Err(Errno::EIO);
    };
    let mut flags = USBD_SYNCHRONOUS;
    if sce.timeout.get() == 0 {
        flags |= USBD_CATCH;
    }

    let xfertype = ue_get_xfertype(edesc.bmAttributes);
    if xfertype != UE_BULK && xfertype != UE_INTERRUPT {
        return Err(Errno::ENXIO);
    }
    let Some(xfer) = usbd_alloc_xfer(udev_of(sc)) else {
        return Err(Errno::EIO);
    };
    loop {
        let chunk = if xfertype == UE_BULK {
            UGEN_BBSIZE
        } else {
            usize::from(ugetw(edesc.wMaxPacketSize)).min(UGEN_BBSIZE)
        };
        let n = chunk.min(uio.uio_resid);
        if n == 0 {
            break;
        }
        error = uiomove(&mut buf[..n], uio);
        if error.is_err() {
            break;
        }
        // SAFETY: `buf` outlives the synchronous transfer (it completes before
        // `usbd_transfer` returns).
        unsafe {
            usbd_setup_xfer(
                xfer,
                pipeh,
                ptr::null_mut(),
                buf.as_mut_ptr(),
                n as u32,
                flags,
                sce.timeout.get(),
                None,
            )
        };
        let err = usbd_transfer(xfer);
        if err.is_err() {
            error = Err(xfer_errno(pipeh, err));
            break;
        }
    }
    // SAFETY: allocated above and idle: the last transfer is done.
    unsafe { usbd_free_xfer(NonNull::from(xfer)) };
    error
}

/// `ugenwrite`.
pub fn ugenwrite(dev: Dev, uio: &mut Uio<'_>, flag: i32) -> Result<(), Errno> {
    let endpt = ugenendpoint(dev);
    let Some(sc) = ugen_unit(ugenunit(dev)) else {
        return Err(Errno::ENXIO);
    };

    sc.sc_refcnt.set(sc.sc_refcnt.get() + 1);
    let error = ugen_do_write(sc, endpt, uio, flag);
    sc.sc_refcnt.set(sc.sc_refcnt.get() - 1);
    if sc.sc_refcnt.get() < 0 {
        usb_detach_wakeup(&sc.sc_dev);
    }
    error
}

/// `ugen_detach`.
pub fn ugen_detach(self_: &Device, _flags: i32) -> Result<(), Errno> {
    let sc = ugen_softc(self_);

    // Abort all pipes. Causes processes waiting for transfer to wake.
    for ends in &sc.sc_endpoints {
        for sce in ends {
            if let Some(p) = sce.pipeh.get() {
                usbd_abort_pipe(p);
            }
        }
    }

    let s = splusb();
    sc.sc_refcnt.set(sc.sc_refcnt.get() - 1);
    if sc.sc_refcnt.get() >= 0 {
        // Wake everyone
        for ends in &sc.sc_endpoints {
            wakeup(ptr::from_ref(&ends[IN]));
        }
        // Wait for processes to go away.
        usb_detach_wait(&sc.sc_dev);
    }
    splx(s);

    // locate the major number
    let n = nchrdev();
    let maj = (0..n)
        .find(|&maj| core::ptr::fn_addr_eq(cdevsw(maj).d_open, ugenopen as DevTypeOpen))
        .unwrap_or(n);

    // Nuke the vnodes for any open instances (calls close).
    let mn = self_.dv_unit.get() as u32 * USB_MAX_ENDPOINTS as u32;
    vdevgone(maj, mn, mn + USB_MAX_ENDPOINTS as u32 - 1, VCHR);

    for endptno in 0..USB_MAX_ENDPOINTS {
        if sc.sc_is_open[endptno].get() != 0 {
            let _ = ugen_do_close(sc, endptno, FREAD | FWRITE);
        }

        // ugenkqfilter() always uses IN.
        let sce = &sc.sc_endpoints[endptno][IN];
        klist_invalidate(&sce.rsel.si_note);
    }
    Ok(())
}

/// `ugenintr`: an interrupt-in transfer completed: queue its data and wake the readers.
pub fn ugenintr(xfer: &'static UsbdXfer, addr: *mut c_void, status: UsbdStatus) {
    // SAFETY: `ugenopen` passes the endpoint, inside the softc, as the pipe's `priv`; the
    // softc outlives the pipe (`ugen_detach` closes the pipes before the softc is freed).
    let sce = unsafe { &*addr.cast::<UgenEndpoint>() };
    let mut count: u32 = 0;

    if status == crate::dev::usb::usbdi::USBD_CANCELLED {
        return;
    }

    if status != USBD_NORMAL_COMPLETION {
        if status == crate::dev::usb::usbdi::USBD_STALLED
            && let Some(p) = sce.pipeh.get()
        {
            let _ = usbd_clear_endpoint_stall_async(p);
        }
        return;
    }

    usbd_get_xfer_status(xfer, None, None, Some(&mut count), None);
    let Some(ibuf) = NonNull::new(sce.ibuf.get()) else {
        return;
    };
    let count = (count as usize).min(sce.ibuflen.get());
    // SAFETY: `ibuf` is `ibuflen` bytes, filled with `count` of them by the transfer that
    // just completed; the next one starts after this callback returns.
    let data = unsafe { slice::from_raw_parts(ibuf.as_ptr(), count) };

    let _ = b_to_q(data, &sce.q);

    if sce.state.get() & UGEN_ASLP != 0 {
        sce.state.set(sce.state.get() & !UGEN_ASLP);
        wakeup(ptr::from_ref(sce));
    }
    selwakeup(&sce.rsel);
}

/// The ring part of `ugen_isoc_rintr`: drop the oldest data if `count` bytes do not fit,
/// then append `frames` (each its `sizes[i]` bytes out of `dmabuf`, `isize` apart) at `fill`.
/// Returns the new `(cur, fill)`.
pub fn isoc_ring_put(
    ring: &mut [u8],
    mut cur: usize,
    mut fill: usize,
    count: usize,
    dmabuf: &[u8],
    isize: usize,
    sizes: &[u16],
) -> (usize, usize) {
    let limit = ring.len();

    // throw away oldest input if the buffer is full
    if fill < cur && cur <= fill + count {
        cur += count;
        if cur >= limit {
            // The C has `ibuf + (limit - cur)`, which points before the buffer.
            cur -= limit;
        }
    }

    for (i, &size) in sizes.iter().enumerate() {
        let mut actlen = usize::from(size);
        let mut at = isize * i;

        // copy data to buffer
        while actlen > 0 {
            let n = actlen.min(limit - fill);
            ring[fill..fill + n].copy_from_slice(&dmabuf[at..at + n]);

            at += n;
            actlen -= n;
            fill += n;
            if fill == limit {
                fill = 0;
            }
        }
    }
    (cur, fill)
}

/// `ugen_isoc_rintr`: an isochronous transfer completed: copy its frames into the ring and
/// start the transfer again.
pub fn ugen_isoc_rintr(xfer: &'static UsbdXfer, addr: *mut c_void, status: UsbdStatus) {
    // SAFETY: `ugenopen` passes the request, inside the endpoint inside the softc, as the
    // transfer's `priv`; the softc outlives the transfers (`ugen_do_close` frees them after
    // closing the pipe).
    let req = unsafe { &*addr.cast::<Isoreq>() };
    // SAFETY: set by `ugenopen` to the endpoint the request belongs to.
    let sce = unsafe { &*req.sce.get() };
    let mut count: u32 = 0;

    // Return if we are aborting.
    if status == crate::dev::usb::usbdi::USBD_CANCELLED {
        return;
    }

    usbd_get_xfer_status(xfer, None, None, Some(&mut count), None);

    let (Some(ibuf), Some(dmabuf), Some(edesc), Some(pipeh)) = (
        NonNull::new(sce.ibuf.get()),
        NonNull::new(req.dmabuf.get()),
        sce.edesc.get(),
        sce.pipeh.get(),
    ) else {
        return;
    };
    let isize = usize::from(ugetw(edesc.wMaxPacketSize));
    let mut sizes = req.sizes.get();
    // SAFETY: `ibuf` is `ibuflen` bytes while the endpoint is open, and `dmabuf` the
    // `isize * UGEN_NISORFRMS` bytes of the request's buffer; both are ours under this
    // callback (the controller does not touch them between completion and the next submit).
    let (ring, dma) = unsafe {
        (
            slice::from_raw_parts_mut(ibuf.as_ptr(), sce.ibuflen.get()),
            slice::from_raw_parts(dmabuf.as_ptr(), isize * UGEN_NISORFRMS),
        )
    };
    let (cur, fill) = isoc_ring_put(
        ring,
        sce.cur.get(),
        sce.fill.get(),
        count as usize,
        dma,
        isize,
        &sizes,
    );
    sce.cur.set(cur);
    sce.fill.set(fill);

    // setup size for next transfer
    sizes = [isize as u16; UGEN_NISORFRMS];
    req.sizes.set(sizes);

    // SAFETY: `req.sizes` lives in the request, which outlives the transfer.
    unsafe {
        usbd_setup_isoc_xfer(
            xfer,
            pipeh,
            ptr::from_ref(req).cast_mut().cast(),
            req.sizes.as_ptr().cast::<u16>(),
            UGEN_NISORFRMS as u32,
            USBD_NO_COPY | USBD_SHORT_XFER_OK,
            Some(ugen_isoc_rintr),
        )
    };
    let _ = usbd_transfer(xfer);

    if sce.state.get() & UGEN_ASLP != 0 {
        sce.state.set(sce.state.get() & !UGEN_ASLP);
        wakeup(ptr::from_ref(sce));
    }
    selwakeup(&sce.rsel);
}

/// `ugen_set_interface`: select alternate setting `altno` of interface `ifaceno` and
/// record its endpoints.
pub fn ugen_set_interface(sc: &UgenSoftc, ifaceno: i32, altno: i32) -> UsbdStatus {
    let dev = udev_of(sc);

    let Some(cdesc) = usbd_get_config_descriptor(dev) else {
        return USBD_INVAL;
    };
    if ifaceno < 0
        || ifaceno >= i32::from(cdesc.bNumInterfaces)
        || usbd_iface_claimed(dev, ifaceno as usize)
    {
        return USBD_INVAL;
    }

    let iface = match usbd_device2interface_handle(dev, ifaceno as u8) {
        Ok(i) => i,
        Err(err) => return err,
    };
    if let Some(id) = usbd_get_interface_descriptor(iface) {
        for endptno in 0..id.bNumEndpoints {
            let Some(ed) = usbd_interface2endpoint_descriptor(iface, endptno) else {
                continue;
            };
            let endpt = ed.bEndpointAddress;
            let dir = if ue_get_dir(endpt) == UE_DIR_IN {
                IN
            } else {
                OUT
            };
            let sce = &sc.sc_endpoints[usize::from(ue_get_addr(endpt))][dir];
            sce.sc.set(ptr::null());
            sce.edesc.set(None);
            sce.iface.set(None);
        }
    }

    // Try to change setting, if this fails put back the descriptors.
    let err = usbd_set_interface(iface, altno);

    if let Some(id) = usbd_get_interface_descriptor(iface) {
        for endptno in 0..id.bNumEndpoints {
            let Some(ed) = usbd_interface2endpoint_descriptor(iface, endptno) else {
                continue;
            };
            let endpt = ed.bEndpointAddress;
            let dir = if ue_get_dir(endpt) == UE_DIR_IN {
                IN
            } else {
                OUT
            };
            let sce = &sc.sc_endpoints[usize::from(ue_get_addr(endpt))][dir];
            sce.sc.set(ptr::from_ref(sc));
            sce.edesc.set(Some(ed));
            sce.iface.set(Some(iface));
        }
    }
    err
}

/// `ugen_get_alt_index`: -1 when there is no such interface.
pub fn ugen_get_alt_index(sc: &UgenSoftc, ifaceno: i32) -> i32 {
    match usbd_device2interface_handle(udev_of(sc), ifaceno as u8) {
        Ok(iface) => usbd_get_interface_altindex(iface),
        Err(_) => -1,
    }
}

/// A `M_TEMP` configuration descriptor copy (`usbd_get_cdesc`), freed on drop.
struct Cdesc {
    ptr: NonNull<u8>,
    len: usize,
    /// The size the C frees it with (`UGETW(cdesc->wTotalLength)` on some paths).
    total: usize,
}

impl Cdesc {
    /// `usbd_get_cdesc(dev, index, &len)`.
    fn get(dev: &'static crate::dev::usb::usbdivar::UsbdDevice, index: i32) -> Option<Self> {
        let mut len = 0u32;
        let ptr = usbd_get_cdesc(dev, index, Some(&mut len))?;
        Some(Self {
            ptr,
            len: len as usize,
            total: len as usize,
        })
    }

    /// The descriptor bytes, mutable (`uiomove` takes its source mutable).
    fn bytes_mut(&mut self) -> &mut [u8] {
        // SAFETY: as in `bytes`, and `&mut self` makes this the only reference.
        unsafe { slice::from_raw_parts_mut(self.ptr.as_ptr(), self.len) }
    }

    /// The descriptor bytes.
    fn bytes(&self) -> &[u8] {
        // SAFETY: `usbd_get_cdesc` returned `len` initialised bytes, ours until the drop.
        unsafe { slice::from_raw_parts(self.ptr.as_ptr(), self.len) }
    }
}

impl Drop for Cdesc {
    fn drop(&mut self) {
        free(self.ptr, M_TEMP, self.total);
    }
}

/// `ugen_do_ioctl`.
pub fn ugen_do_ioctl(
    sc: &UgenSoftc,
    endpt: usize,
    cmd: u64,
    addr: &mut [u8],
    flag: i32,
    p: &Proc,
) -> Result<(), Errno> {
    if ugen_dying(sc) {
        return Err(Errno::EIO);
    }
    let dev = udev_of(sc);

    match cmd {
        USB_SET_SHORT_XFER => {
            if endpt == usize::from(USB_CONTROL_ENDPOINT) {
                return Err(Errno::EINVAL);
            }
            // This flag only affects read
            let sce = &sc.sc_endpoints[endpt][IN];
            if sce.pipeh.get().is_none() {
                return Err(Errno::EINVAL);
            }
            if ioctl_arg::<i32>(addr) != 0 {
                sce.state.set(sce.state.get() | UGEN_SHORT_OK);
            } else {
                sce.state.set(sce.state.get() & !UGEN_SHORT_OK);
            }
            return Ok(());
        }
        USB_SET_TIMEOUT => {
            let t = ioctl_arg::<i32>(addr) as u32;
            sc.sc_endpoints[endpt][IN].timeout.set(t);
            sc.sc_endpoints[endpt][OUT].timeout.set(t);
            return Ok(());
        }
        _ => {}
    }

    if endpt != usize::from(USB_CONTROL_ENDPOINT) {
        return Err(Errno::EINVAL);
    }

    match cmd {
        // USB_SETDEBUG: UGEN_DEBUG only.
        USB_GET_CONFIG => {
            let mut conf: u8 = 0;
            let err = usbd_get_config(dev, &mut conf);
            if err.is_err() {
                return Err(Errno::EIO);
            }
            ioctl_ret(addr, &i32::from(conf));
        }
        USB_SET_CONFIG => {
            if flag & FWRITE == 0 {
                return Err(Errno::EPERM);
            }
            let err = ugen_set_config(sc, ioctl_arg::<i32>(addr));
            if err == USBD_NORMAL_COMPLETION {
            } else if err == USBD_IN_USE {
                return Err(Errno::EBUSY);
            } else {
                return Err(Errno::EIO);
            }
        }
        USB_GET_ALTINTERFACE => {
            let mut ai: UsbAltInterface = ioctl_arg(addr);
            let Ok(iface) = usbd_device2interface_handle(dev, ai.uai_interface_index as u8) else {
                return Err(Errno::EINVAL);
            };
            let Some(idesc) = usbd_get_interface_descriptor(iface) else {
                return Err(Errno::EIO);
            };
            ai.uai_alt_no = i32::from(idesc.bAlternateSetting);
            ioctl_ret(addr, &ai);
        }
        USB_SET_ALTINTERFACE => {
            if flag & FWRITE == 0 {
                return Err(Errno::EPERM);
            }
            let ai: UsbAltInterface = ioctl_arg(addr);
            if usbd_device2interface_handle(dev, ai.uai_interface_index as u8).is_err() {
                return Err(Errno::EINVAL);
            }
            let err = ugen_set_interface(sc, ai.uai_interface_index, ai.uai_alt_no);
            if err.is_err() {
                return Err(Errno::EINVAL);
            }
        }
        USB_GET_NO_ALT => {
            let mut ai: UsbAltInterface = ioctl_arg(addr);
            let Some(cdesc) = Cdesc::get(dev, ai.uai_config_index) else {
                return Err(Errno::EINVAL);
            };
            let Some(idesc) = usbd_find_idesc(cdesc.bytes(), ai.uai_interface_index, 0) else {
                return Err(Errno::EINVAL);
            };
            ai.uai_alt_no = usbd_get_no_alts(cdesc.bytes(), i32::from(idesc.bInterfaceNumber));
            ioctl_ret(addr, &ai);
        }
        USB_GET_DEVICE_DESC => {
            ioctl_ret(addr, &usbd_get_device_descriptor(dev));
        }
        USB_GET_CONFIG_DESC => {
            let mut cd: UsbConfigDesc = ioctl_arg(addr);
            let Some(cdesc) = Cdesc::get(dev, cd.ucd_config_index) else {
                return Err(Errno::EINVAL);
            };
            cd.ucd_desc = UsbConfigDescriptor::read_from(cdesc.bytes());
            ioctl_ret(addr, &cd);
        }
        USB_GET_INTERFACE_DESC => {
            let mut id: UsbInterfaceDesc = ioctl_arg(addr);
            let Some(cdesc) = Cdesc::get(dev, id.uid_config_index) else {
                return Err(Errno::EINVAL);
            };
            let alt = if id.uid_config_index == USB_CURRENT_CONFIG_INDEX
                && id.uid_alt_index == USB_CURRENT_ALT_INDEX
            {
                i32::from(ugen_get_alt_index(sc, id.uid_interface_index) as u8)
            } else {
                id.uid_alt_index
            };
            let Some(idesc) = usbd_find_idesc(cdesc.bytes(), id.uid_interface_index, alt) else {
                return Err(Errno::EINVAL);
            };
            id.uid_desc = *idesc;
            ioctl_ret(addr, &id);
        }
        USB_GET_ENDPOINT_DESC => {
            let mut ed: UsbEndpointDesc = ioctl_arg(addr);
            let Some(cdesc) = Cdesc::get(dev, ed.ued_config_index) else {
                return Err(Errno::EINVAL);
            };
            let alt = if ed.ued_config_index == USB_CURRENT_CONFIG_INDEX
                && ed.ued_alt_index == USB_CURRENT_ALT_INDEX
            {
                i32::from(ugen_get_alt_index(sc, ed.ued_interface_index) as u8)
            } else {
                ed.ued_alt_index
            };
            let Some(edesc) = usbd_find_edesc(
                cdesc.bytes(),
                ed.ued_interface_index,
                alt,
                ed.ued_endpoint_index,
            ) else {
                return Err(Errno::EINVAL);
            };
            ed.ued_desc = *edesc;
            ioctl_ret(addr, &ed);
        }
        USB_GET_FULL_DESC => {
            let fd: UsbFullDesc = ioctl_arg(addr);
            let Some(mut cdesc) = Cdesc::get(dev, fd.ufd_config_index) else {
                return Err(Errno::EINVAL);
            };
            let len = cdesc.len.min(fd.ufd_size as usize);
            let mut iov = [Iovec {
                iov_base: fd.ufd_data as *mut c_void,
                iov_len: len,
            }];
            let mut uio = Uio {
                uio_iov: &mut iov,
                uio_offset: 0,
                uio_resid: len,
                uio_segflg: UioSeg::UIO_USERSPACE,
                uio_rw: UioRw::UIO_READ,
                uio_procp: Some(p),
            };
            return uiomove(&mut cdesc.bytes_mut()[..len], &mut uio);
        }
        USB_DO_REQUEST => {
            let mut ur: UsbCtlRequest = ioctl_arg(addr);
            let len = usize::from(ugetw(ur.ucr_request.wLength));

            if flag & FWRITE == 0 {
                return Err(Errno::EPERM);
            }
            // Avoid requests that would damage the bus integrity.
            let rq = ur.ucr_request;
            if (rq.bmRequestType == UT_WRITE_DEVICE && rq.bRequest == UR_SET_ADDRESS)
                || (rq.bmRequestType == UT_WRITE_DEVICE && rq.bRequest == UR_SET_CONFIG)
                || (rq.bmRequestType == UT_WRITE_INTERFACE && rq.bRequest == UR_SET_INTERFACE)
            {
                return Err(Errno::EINVAL);
            }

            if len > 32767 {
                return Err(Errno::EINVAL);
            }
            let rw = if rq.bmRequestType & UT_READ != 0 {
                UioRw::UIO_READ
            } else {
                UioRw::UIO_WRITE
            };
            let mut iov = [Iovec {
                iov_base: ur.ucr_data as *mut c_void,
                iov_len: len,
            }];
            let mut uio = Uio {
                uio_iov: &mut iov,
                uio_offset: 0,
                uio_resid: len,
                uio_segflg: UioSeg::UIO_USERSPACE,
                uio_rw: rw,
                uio_procp: Some(p),
            };
            let mut ptr_mem: Option<NonNull<u8>> = None;
            let r = 'ret: {
                if len != 0 {
                    let Some(m) = malloc(len, M_TEMP, M_NOWAIT) else {
                        break 'ret Err(Errno::ENOMEM);
                    };
                    ptr_mem = Some(m);
                    if rw == UioRw::UIO_WRITE {
                        // SAFETY: a fresh allocation of `len` bytes, ours until the free.
                        let buf = unsafe { slice::from_raw_parts_mut(m.as_ptr(), len) };
                        if let Err(e) = uiomove(buf, &mut uio) {
                            break 'ret Err(e);
                        }
                    }
                }
                let buf: &mut [u8] = match ptr_mem {
                    // SAFETY: as above.
                    Some(m) => unsafe { slice::from_raw_parts_mut(m.as_ptr(), len) },
                    None => &mut [],
                };
                let sce = &sc.sc_endpoints[endpt][IN];
                let err = usbd_do_request_flags(
                    dev,
                    &ur.ucr_request,
                    buf,
                    ur.ucr_flags as u16,
                    Some(&mut ur.ucr_actlen),
                    sce.timeout.get(),
                );
                if err.is_err() {
                    break 'ret Err(Errno::EIO);
                }
                // Only if USBD_SHORT_XFER_OK is set.
                let mlen = len.min(ur.ucr_actlen.max(0) as usize);
                if mlen != 0
                    && rw == UioRw::UIO_READ
                    && let Err(e) = uiomove(&mut buf[..mlen], &mut uio)
                {
                    break 'ret Err(e);
                }
                Ok(())
            };
            // ret:
            if let Some(m) = ptr_mem {
                free(m, M_TEMP, len);
            }
            ioctl_ret(addr, &ur);
            return r;
        }
        USB_GET_DEVICEINFO => {
            let mut di: UsbDeviceInfo = ioctl_arg(addr);
            usbd_fill_deviceinfo(dev, &mut di);
            ioctl_ret(addr, &di);
        }
        _ => return Err(Errno::EINVAL),
    }
    Ok(())
}

/// `ugenioctl`.
pub fn ugenioctl(dev: Dev, cmd: u64, addr: &mut [u8], flag: i32, p: &Proc) -> Result<(), Errno> {
    let endpt = ugenendpoint(dev);
    let Some(sc) = ugen_unit(ugenunit(dev)) else {
        return Err(Errno::ENXIO);
    };

    sc.sc_refcnt.set(sc.sc_refcnt.get() + 1);
    let error = ugen_do_ioctl(sc, endpt, cmd, addr, flag, p);
    sc.sc_refcnt.set(sc.sc_refcnt.get() - 1);
    if sc.sc_refcnt.get() < 0 {
        usb_detach_wakeup(&sc.sc_dev);
    }
    error
}

/// The endpoint a knote hangs on (`kn->kn_hook`).
fn kn_ugen(kn: &Knote) -> &UgenEndpoint {
    // SAFETY: `ugenkqfilter` set `kn_hook` to the endpoint, in the softc, and `ugen_detach`
    // invalidates the klists (detaching every knote) before the device goes.
    unsafe { &*kn.kn_hook.get().cast::<UgenEndpoint>() }
}

/// `filt_ugenrdetach`.
pub fn filt_ugenrdetach(kn: &Knote) {
    let sce = kn_ugen(kn);

    let s = splusb();
    klist_remove_locked(&sce.rsel.si_note, kn);
    splx(s);
}

/// `filt_ugenread_intr`: readable when the clist holds data; its size is the data.
pub fn filt_ugenread_intr(kn: &Knote, _hint: i64) -> bool {
    let sce = kn_ugen(kn);

    kn.kn_data().set(i64::from(sce.q.c_cc.get()));
    kn.kn_data().get() > 0
}

/// `filt_ugenread_isoc`: readable when the ring holds data; its size is the data.
pub fn filt_ugenread_isoc(kn: &Knote, _hint: i64) -> bool {
    let sce = kn_ugen(kn);
    let (cur, fill, limit) = (sce.cur.get(), sce.fill.get(), sce.limit());

    if cur == fill {
        return false;
    }

    if cur < fill {
        kn.kn_data().set((fill - cur) as i64);
    } else {
        kn.kn_data().set(((limit - cur) + fill) as i64);
    }

    true
}

/// `ugenread_intr_filtops`.
pub static UGENREAD_INTR_FILTOPS: Filterops = Filterops {
    f_flags: FILTEROP_ISFD,
    f_attach: None,
    f_detach: Some(filt_ugenrdetach),
    f_event: Some(filt_ugenread_intr),
    f_modify: None,
    f_process: None,
};

/// `ugenread_isoc_filtops`.
pub static UGENREAD_ISOC_FILTOPS: Filterops = Filterops {
    f_flags: FILTEROP_ISFD,
    f_attach: None,
    f_detach: Some(filt_ugenrdetach),
    f_event: Some(filt_ugenread_isoc),
    f_modify: None,
    f_process: None,
};

/// `ugenkqfilter`.
pub fn ugenkqfilter(dev: Dev, kn: &Knote) -> Result<(), Errno> {
    let Some(sc) = ugen_unit(ugenunit(dev)) else {
        return Err(Errno::ENXIO);
    };

    if ugen_dying(sc) {
        return Err(Errno::ENXIO);
    }

    // XXX always IN
    let sce = &sc.sc_endpoints[ugenendpoint(dev)][IN];
    let Some(edesc) = sce.edesc.get() else {
        return Err(Errno::EINVAL);
    };

    let klist = match kn.kn_filter().get() {
        EVFILT_READ => match ue_get_xfertype(edesc.bmAttributes) {
            UE_INTERRUPT => {
                kn.kn_fop.set(Some(&UGENREAD_INTR_FILTOPS));
                &sce.rsel.si_note
            }
            UE_ISOCHRONOUS => {
                kn.kn_fop.set(Some(&UGENREAD_ISOC_FILTOPS));
                &sce.rsel.si_note
            }
            // We have no easy way of determining if a read will yield any data or a write
            // will happen.
            UE_BULK => return seltrue_kqfilter(dev, kn),
            _ => return Err(Errno::EINVAL),
        },
        EVFILT_WRITE => match ue_get_xfertype(edesc.bmAttributes) {
            // XXX poll doesn't support this
            UE_INTERRUPT | UE_ISOCHRONOUS => return Err(Errno::EINVAL),
            // We have no easy way of determining if a read will yield any data or a write
            // will happen.
            UE_BULK => return seltrue_kqfilter(dev, kn),
            _ => return Err(Errno::EINVAL),
        },
        _ => return Err(Errno::EINVAL),
    };

    kn.kn_hook.set(ptr::from_ref(sce).cast_mut().cast());

    let s = splusb();
    klist_insert_locked(klist, kn);
    splx(s);

    Ok(())
}

/// `ugen_clear_iface_eps`: clear the halt feature of the bulk and interrupt endpoints of
/// `iface`, when no endpoint is open.
pub fn ugen_clear_iface_eps(sc: &UgenSoftc, iface: &'static UsbdInterface) {
    // Only clear interface endpoints when none are in use.
    for i in 0..USB_MAX_ENDPOINTS {
        if i == usize::from(USB_CONTROL_ENDPOINT) {
            continue;
        }
        if sc.sc_is_open[i].get() != 0 {
            return;
        }
    }

    let bad = || {
        printf(format_args!(
            "ugen_clear_iface_eps: clear endpoints failed!\n"
        ))
    };
    let Some(id) = usbd_get_interface_descriptor(iface) else {
        bad();
        return;
    };

    for i in 0..id.bNumEndpoints {
        let Some(ed) = usbd_interface2endpoint_descriptor(iface, i) else {
            bad();
            return;
        };

        let xfertype = ue_get_xfertype(ed.bmAttributes);
        if (xfertype == UE_BULK || xfertype == UE_INTERRUPT)
            && usbd_clear_endpoint_feature(
                udev_of(sc),
                i32::from(ed.bEndpointAddress),
                i32::from(UF_ENDPOINT_HALT),
            )
            .is_err()
        {
            bad();
            return;
        }
    }
}
/* </CODE> */

/* <TESTS> */
#[cfg(test)]
mod tests {
    // Host tests for `ugen`: the minor number's fields, the isochronous ring and the device
    // table of the ioctl numbers `usbdevs(8)` and `ugen` share.

    use std::assert_eq;
    use std::vec::Vec;

    use super::*;
    use crate::sys::types::makedev;

    #[test]
    fn minor_is_unit_and_endpoint() {
        let dev = makedev(63, (3 << 4) | 5);
        assert_eq!(ugenunit(dev), 3);
        assert_eq!(ugenendpoint(dev), 5);
        assert_eq!(ugenunit(makedev(63, 0x1f0)), 0xf, "the unit is 4 bits");
        assert_eq!(
            ugenendpoint(makedev(63, 0)),
            0,
            "/dev/ugen0.00 is the control pipe"
        );
    }

    #[test]
    fn the_ring_appends_frames() {
        let mut ring = std::vec![0u8; 8];
        // two frames of 3 and 2 bytes out of 4-byte slots
        let dma: Vec<u8> = std::vec![1, 2, 3, 0, 4, 5, 0, 0];
        let (cur, fill) = isoc_ring_put(&mut ring, 0, 0, 5, &dma, 4, &[3, 2, 0, 0]);
        assert_eq!((cur, fill), (0, 5));
        assert_eq!(&ring[..5], &[1, 2, 3, 4, 5]);
    }

    #[test]
    fn the_ring_wraps() {
        let mut ring = std::vec![0u8; 8];
        let dma: Vec<u8> = std::vec![1, 2, 3, 4, 5, 6, 7, 8];
        let (cur, fill) = isoc_ring_put(&mut ring, 0, 6, 8, &dma, 2, &[2, 2, 2, 2]);
        assert_eq!(fill, 6, "8 bytes from 6 around an 8-byte ring end at 6");
        assert_eq!(cur, 0);
        assert_eq!(ring, [3, 4, 5, 6, 7, 8, 1, 2]);
    }

    #[test]
    fn a_full_ring_drops_the_oldest_data() {
        let mut ring = std::vec![0u8; 8];
        let dma: Vec<u8> = std::vec![9; 8];
        // fill (2) < cur (4) <= fill + count (2 + 4): the reader is passed
        let (cur, fill) = isoc_ring_put(&mut ring, 4, 2, 4, &dma, 1, &[1, 1, 1, 1]);
        assert_eq!(fill, 6);
        assert_eq!(
            cur, 0,
            "cur + count reaches the limit and wraps to the start"
        );
        // not passed: cur (6) > fill + count (2 + 3)
        let (cur, fill) = isoc_ring_put(&mut ring, 6, 2, 3, &dma, 1, &[1, 1, 1, 0]);
        assert_eq!((cur, fill), (6, 5));
    }
}
/* </TESTS> */
