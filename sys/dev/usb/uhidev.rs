/*	$OpenBSD: uhidev.c,v 1.112 2025/11/13 23:04:48 jmatthew Exp $	*/
/*	$NetBSD: uhidev.c,v 1.14 2003/03/11 16:44:00 augustss Exp $	*/
/*	$OpenBSD: uhidev.h,v 1.41 2022/03/21 12:18:52 thfr Exp $	*/
/*	$NetBSD: uhidev.h,v 1.3 2002/10/08 09:56:17 dan Exp $	*/
/* <LICENSES> */
/*
 * Copyright (c) 2001 The NetBSD Foundation, Inc.
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
//! uhidev(4): the USB HID root device. It owns the interrupt pipes of one HID interface and
//! hands each input report to the child (`ukbd(4)`, `ums(4)`, ...) that owns its report ID.
//!
//! Upstream: sys/dev/usb/uhidev.c @ 3ce1f3f79392, sys/dev/usb/uhidev.h @ 3ce1f3f79392
//!
//! `uhidev_match` takes any interface of class HID (and the Xbox 360 and Xbox One gamepads,
//! which are vendor class and have no report descriptor). `uhidev_attach` reads the
//! interface's endpoints and its report descriptor (or one of `uhid_rdesc.rs`'s replacements),
//! counts the report IDs, offers the interface to a driver that claims several of them at
//! once, then looks for one child per report ID with something in it (`config_found_sm` with
//! a [`UhidevAttachArg`]). The children open the pipes through [`uhidev_open`] (reference
//! counted: the first opens the input pipe, and the output pipe when the interface has one)
//! and [`uhidev_close`]; `uhidev_intr` splits the report ID off a report and calls the
//! child's `sc_intr`. The reports a child sends or reads are `uhidev_set_report`,
//! `uhidev_get_report` and their `_async` forms, `uhidev_write` and `uhidev_ioctl`.
//!
//! Everything runs under the kernel lock at `splusb()`, like the rest of the USB stack: no
//! USB path but xhci's hard interrupt is `IPL_MPSAFE`.
//!
//! ## Deviations
//! - `UHIDEV_DEBUG` is not in GENERIC: the `DPRINTF`s and the report dump of `uhidev_intr`
//!   are absent. `SMALL_KERNEL` is not defined: the replacement report descriptors and the
//!   Xbox controllers are compiled in.
//! - The softc members are `Cell`s over `&'static` USB objects and raw pointers (a zeroed
//!   softc is all `None` and null); the child driver's `struct uhidev` is [`Uhidev`], the
//!   first member of its own softc, so `sc_subdevs` is an array of [`Uhidev`] pointers
//!   (`Option<NonNull<Uhidev>>`, `None` for the C's `NULL`).
//! - `uhidev_get_report_desc` returns the descriptor as a slice instead of two out
//!   parameters; the descriptor is freed in `uhidev_detach`, so the slice is valid for as
//!   long as the attached children are.
//! - A child's interrupt function is `fn(&Uhidev, &mut [u8])`: the report without its
//!   report ID, the C's `(ibuf, len)` pair. `uhidev_intr` clamps the length to the input
//!   buffer, and a report of zero bytes with a report ID byte to the empty slice (the C's
//!   `cc--` would wrap).
//! - `uhidev_set_report`, `uhidev_set_report_async` and `uhidev_get_report` return
//!   `Result<usize, Errno>`: the C's `actlen`, or `Err(EIO)` for its `-1` (`ENOMEM` when a
//!   temporary buffer cannot be had). The data is a slice, and `uhidev_set_report` and
//!   `uhidev_write` take it mutable because `usbd_do_request` and `usbd_setup_xfer` do.
//!   `uhidev_get_report_async` is `unsafe` (the destination buffer must outlive the
//!   transfer) and its callback keeps the C's `len == -1` for failure.
//! - `uhidev_ioctl` and `uhidev_report_type_conv` return `Result<bool, Errno>` (`Ok(false)`
//!   is the C's `-1`, as in `hidkbd_ioctl`) and `Option<u8>`.
//! - `uhidev_use_rdesc` returns the replacement descriptor as `Ok(Some((allocation, size)))`
//!   instead of through out parameters.
//! - A report descriptor of length zero is refused (`no report descriptor`): the C would
//!   `malloc(0)`.
//! - The report descriptor read from the device is allocated zeroed (the C leaves it
//!   uninitialised) because Rust may not form a slice over uninitialised bytes.
//! - `uhidev_detach` returns the first error of its children (the C ORs the errnos) and
//!   clears `sc_subdevs` after freeing it; a failed attach leaves a softc `uhidev_detach`
//!   can handle (no descriptor, no children).
//! - The Wacom battery collection (`#ifdef notyet` in the C) is not carried.
//! - `KASSERTMSG(nclaimed > 0, ...)` is `kassert!(nclaimed > 0)` (no message).
//! - The failure paths of `uhidev_set_report_async` and `uhidev_get_report_async` keep the
//!   C's leaks (the xfer when `usbd_transfer` or `usbd_request_async` fails).

use alloc::vec::Vec;
use core::cell::Cell;
use core::ffi::c_void;
use core::mem::size_of;
use core::ptr::{self, NonNull};
use core::slice;

use crate::dev::hid::hid::{
    HCOLL_PHYSICAL, HUD_STYLUS, HUD_TABLET_FKEYS, HUP_DIGITIZERS, HUP_WACOM, HidItem, HidKind,
    hid_all, hid_end_parse, hid_feature, hid_get_id_of_collection, hid_get_item, hid_input,
    hid_output, hid_report_size, hid_start_parse, hid_usage2,
};
use crate::dev::usb::uhid_rdesc::{
    UHID_GRAPHIRE_REPORT_DESCR, UHID_GRAPHIRE3_4X5_REPORT_DESCR, UHID_XB360GP_REPORT_DESCR,
    UHID_XBONEGP_REPORT_DESCR,
};
use crate::dev::usb::usb::{
    UE_DIR_IN, UE_DIR_OUT, UE_INTERRUPT, UICLASS_HID, UICLASS_VENDOR, USB_GET_REPORT,
    USB_GET_REPORT_DESC, USB_GET_REPORT_ID, USB_SET_REPORT, USBD_SHORT_XFER_OK,
    UT_READ_CLASS_INTERFACE, UT_WRITE_CLASS_INTERFACE, UsbCtlReport, UsbCtlReportDesc,
    UsbDeviceRequest, UsbInterfaceDescriptor, ue_get_dir, ue_get_xfertype, ugetw, usetw, usetw2,
};
use crate::dev::usb::usb_mem::kernaddr;
use crate::dev::usb::usb_quirks::UQ_BAD_HID;
use crate::dev::usb::usbdevs::{
    USB_PRODUCT_WACOM_GRAPHIRE, USB_PRODUCT_WACOM_GRAPHIRE3_4X5, USB_PRODUCT_WACOM_GRAPHIRE4_4X5,
    USB_VENDOR_WACOM,
};
use crate::dev::usb::usbdi::{
    UMATCH_IFACECLASS_GENERIC, UMATCH_IFACECLASS_IFACESUBCLASS_IFACEPROTO, UMATCH_NONE,
    USBD_CANCELLED, USBD_CATCH, USBD_DEFAULT_INTERVAL, USBD_DEFAULT_TIMEOUT, USBD_INVAL,
    USBD_IOERROR, USBD_NO_COPY, USBD_NO_TIMEOUT, USBD_NORMAL_COMPLETION, USBD_SHORT_XFER,
    USBD_STALLED, USBD_SYNCHRONOUS, UsbAttachArg, UsbdStatus, usbd_alloc_buffer, usbd_alloc_xfer,
    usbd_clear_endpoint_stall, usbd_clear_endpoint_stall_async, usbd_close_pipe, usbd_deactivate,
    usbd_do_request, usbd_do_request_flags, usbd_free_xfer, usbd_get_interface_descriptor,
    usbd_get_quirks, usbd_get_xfer_status, usbd_interface2endpoint_descriptor, usbd_is_dying,
    usbd_open_pipe, usbd_open_pipe_intr, usbd_request_async, usbd_setup_xfer, usbd_transfer,
};
use crate::dev::usb::usbdi_util::{usbd_get_hid_descriptor, usbd_get_report_descriptor};
use crate::dev::usb::usbdivar::{UsbdDevice, UsbdInterface, UsbdPipe, UsbdXfer};
use crate::dev::usb::usbhid::{
    UHID_FEATURE_REPORT, UHID_INPUT_REPORT, UHID_OUTPUT_REPORT, UR_GET_REPORT, UR_SET_REPORT,
};
use crate::kassert;
use crate::kern::kern_malloc::{free, malloc, mallocarray};
use crate::kern::subr_autoconf::{config_deactivate, config_detach, config_found_sm};
use crate::kern::subr_prf::{Str, panic, printf};
use crate::sys::device::{
    CfMatch, Cfattach, Cfdriver, DV_DULL, DVACT_DEACTIVATE, Device, Softc, UNCONF,
};
use crate::sys::errno::Errno;
use crate::sys::ioctl::{ioctl_arg, ioctl_ret};
use crate::sys::malloc::{M_NOWAIT, M_TEMP, M_USBDEV, M_WAITOK, M_ZERO};
use crate::sys::proc::Proc;

/// `UISUBCLASS_XBOX360_CONTROLLER`.
const UISUBCLASS_XBOX360_CONTROLLER: u8 = 0x5d;
/// `UIPROTO_XBOX360_GAMEPAD`.
const UIPROTO_XBOX360_GAMEPAD: u8 = 0x01;
/// `UISUBCLASS_XBOXONE_CONTROLLER`.
const UISUBCLASS_XBOXONE_CONTROLLER: u8 = 0x47;
/// `UIPROTO_XBOXONE_GAMEPAD`.
const UIPROTO_XBOXONE_GAMEPAD: u8 = 0xd0;

/// `UHIDEV_F_XB1`: Xbox One controller.
pub const UHIDEV_F_XB1: u32 = 0x0001;

/// `UHIDEV_OPEN`: device is open (`uhidev.sc_state`).
pub const UHIDEV_OPEN: u8 = 0x01;

/// The type of a child's `sc_intr`: the report (without its report ID) the device sent.
pub type UhidevIntrFn = fn(&Uhidev, &mut [u8]);

/// `struct uhidev_async_info`: what `uhidev_get_report_async_cb` needs to deliver the report.
struct UhidevAsyncInfo {
    /// `callback`.
    callback: UhidevGetReportCb,
    /// `priv`.
    priv_: *mut c_void,
    /// `data`.
    data: *mut u8,
    /// `id`.
    id: i32,
}

/// The type of `uhidev_get_report_async`'s callback: `(priv, id, data, len)`, `len` being
/// `-1` when the request failed.
pub type UhidevGetReportCb = fn(priv_: *mut c_void, id: i32, data: *mut u8, len: i32);

/// `struct uhidev_softc`: the HID interface.
#[repr(C)]
pub struct UhidevSoftc {
    /// `sc_dev`: base device.
    pub sc_dev: Device,
    /// `sc_udev`.
    pub sc_udev: Cell<Option<&'static UsbdDevice>>,
    /// `sc_iface`: interface.
    pub sc_iface: Cell<Option<&'static UsbdInterface>>,
    /// `sc_ifaceno`: interface number.
    pub sc_ifaceno: Cell<i32>,
    /// `sc_ipipe`: input interrupt pipe.
    pub sc_ipipe: Cell<Option<&'static UsbdPipe>>,
    /// `sc_ixfer`: read request.
    pub sc_ixfer: Cell<Option<&'static UsbdXfer>>,
    /// `sc_iep_addr`.
    pub sc_iep_addr: Cell<i32>,

    /// `sc_ibuf`: the input buffer the interrupt pipe fills.
    pub sc_ibuf: Cell<*mut u8>,
    /// `sc_isize`.
    pub sc_isize: Cell<u32>,

    /// `sc_opipe`: output interrupt pipe.
    pub sc_opipe: Cell<Option<&'static UsbdPipe>>,
    /// `sc_oxfer`: write request.
    pub sc_oxfer: Cell<Option<&'static UsbdXfer>>,
    /// `sc_owxfer`: internal write request.
    pub sc_owxfer: Cell<Option<&'static UsbdXfer>>,
    /// `sc_oep_addr`.
    pub sc_oep_addr: Cell<i32>,

    /// `sc_repdesc`: the report descriptor (`M_USBDEV`).
    pub sc_repdesc: Cell<*mut u8>,
    /// `sc_repdesc_size`.
    pub sc_repdesc_size: Cell<i32>,

    /// `sc_nrepid`: report IDs, one more than the largest.
    pub sc_nrepid: Cell<u32>,
    /// `sc_subdevs`: `sc_nrepid` children, by report ID (`M_USBDEV`).
    pub sc_subdevs: Cell<*mut Cell<Option<NonNull<Uhidev>>>>,

    /// `sc_refcnt`: how many children have the pipes open.
    pub sc_refcnt: Cell<i32>,

    /// `sc_flags`: `UHIDEV_F_*`.
    pub sc_flags: Cell<u32>,
}

// SAFETY: `#[repr(C)]` with the device first; every other member is a `Cell` of an
// `Option` of a reference, an integer or a raw pointer: all valid as zero bits.
unsafe impl Softc for UhidevSoftc {}

impl UhidevSoftc {
    /// `sc->sc_udev`. Panics before the attach set it.
    pub fn udev(&self) -> &'static UsbdDevice {
        match self.sc_udev.get() {
            Some(d) => d,
            None => panic(format_args!("{}: no USB device", self.sc_dev.xname())),
        }
    }

    /// `sc->sc_subdevs[0..sc_nrepid]`.
    pub fn subdevs(&self) -> &[Cell<Option<NonNull<Uhidev>>>] {
        match NonNull::new(self.sc_subdevs.get()) {
            // SAFETY: `uhidev_attach` allocates `sc_nrepid` zeroed (`None`) entries and
            // frees them, clearing the pointer, in `uhidev_detach`.
            Some(p) => unsafe { slice::from_raw_parts(p.as_ptr(), self.sc_nrepid.get() as usize) },
            None => &[],
        }
    }

    /// The report descriptor (`sc_repdesc`, `sc_repdesc_size`); empty before the attach got
    /// it.
    fn repdesc(&self) -> &[u8] {
        match NonNull::new(self.sc_repdesc.get()) {
            // SAFETY: `sc_repdesc` is an `M_USBDEV` allocation of `sc_repdesc_size` bytes,
            // initialised (zeroed, then filled by the request, or copied from a table), freed
            // only by `uhidev_detach`.
            Some(p) => unsafe {
                slice::from_raw_parts(p.as_ptr(), self.sc_repdesc_size.get() as usize)
            },
            None => &[],
        }
    }
}

/// `struct uhidev`: the head of the softc of a driver attached to one report ID of the
/// interface (`ukbd_softc` starts with it).
#[repr(C)]
pub struct Uhidev {
    /// `sc_dev`: base device.
    pub sc_dev: Device,
    /// `sc_udev`: USB device.
    pub sc_udev: Cell<Option<&'static UsbdDevice>>,
    /// `sc_parent`.
    pub sc_parent: Cell<Option<&'static UhidevSoftc>>,
    /// `sc_report_id`.
    pub sc_report_id: Cell<u8>,
    /// `sc_state`: `UHIDEV_OPEN`.
    pub sc_state: Cell<u8>,
    /// `sc_intr`: the report handler.
    pub sc_intr: Cell<Option<UhidevIntrFn>>,

    /// `sc_isize`: input report size, in bytes.
    pub sc_isize: Cell<i32>,
    /// `sc_osize`: output report size.
    pub sc_osize: Cell<i32>,
    /// `sc_fsize`: feature report size.
    pub sc_fsize: Cell<i32>,
}

// SAFETY: `#[repr(C)]` with the device first; every other member is a `Cell` of an
// `Option` of a reference, a function pointer or an integer: all valid as zero bits.
unsafe impl Softc for Uhidev {}

impl Uhidev {
    /// `scd->sc_parent`. Panics before the child's attach set it.
    pub fn parent(&self) -> &'static UhidevSoftc {
        match self.sc_parent.get() {
            Some(p) => p,
            None => panic(format_args!("{}: no uhidev parent", self.sc_dev.xname())),
        }
    }
}

/// `struct uhidev_attach_arg`: what a child of `uhidev` gets as `aux`.
pub struct UhidevAttachArg<'a> {
    /// `uaa`: the interface's attach argument.
    pub uaa: &'a UsbAttachArg,
    /// `parent`.
    pub parent: &'static UhidevSoftc,
    /// `reportid`: the report ID the child is offered.
    pub reportid: u8,
    /// `nreports`: how many report IDs the interface has.
    pub nreports: u32,
    /// `claimed`: `nreports` flags a driver claiming several report IDs sets; null when
    /// the child is offered one report ID.
    pub claimed: *mut u8,
}

impl UhidevAttachArg<'_> {
    /// `UHIDEV_CLAIM_MULTIPLE_REPORTID(u)`: the child is offered all the report IDs.
    pub fn claim_multiple_reportid(&self) -> bool {
        !self.claimed.is_null()
    }
}

/// `uhidev_cd`.
pub static UHIDEV_CD: Cfdriver = Cfdriver::new(b"uhidev", DV_DULL, 0);

/// `uhidev_ca`.
pub static UHIDEV_CA: Cfattach = Cfattach {
    ca_devsize: size_of::<UhidevSoftc>(),
    ca_match: Some(uhidev_match),
    ca_attach: uhidev_attach,
    ca_detach: Some(uhidev_detach),
    ca_activate: Some(uhidev_activate),
};

/// `(struct uhidev_softc *)self`.
fn uhidev_softc(self_: &Device) -> &'static UhidevSoftc {
    // SAFETY: only called with devices `uhidev_ca` made (uhidev's own entry points); an
    // attached device lives until `config_detach` frees it after `uhidev_detach`.
    unsafe { &*ptr::from_ref(self_.softc::<UhidevSoftc>()) }
}

/// `uhidev_match`: an interface of class HID, or an Xbox 360 or Xbox One gamepad.
pub fn uhidev_match(_parent: Option<&Device>, _match: &CfMatch, aux: *mut c_void) -> i32 {
    // SAFETY: `usbd_probe_and_attach` hands its `usb_attach_arg`, valid during the match.
    let uaa = unsafe { &*aux.cast::<UsbAttachArg>() };

    let Some(iface) = uaa.iface else {
        return UMATCH_NONE;
    };
    let Some(id) = usbd_get_interface_descriptor(iface) else {
        return UMATCH_NONE;
    };
    if id.bInterfaceClass == UICLASS_VENDOR
        && id.bInterfaceSubClass == UISUBCLASS_XBOX360_CONTROLLER
        && id.bInterfaceProtocol == UIPROTO_XBOX360_GAMEPAD
    {
        return UMATCH_IFACECLASS_IFACESUBCLASS_IFACEPROTO;
    }
    if id.bInterfaceClass == UICLASS_VENDOR
        && id.bInterfaceSubClass == UISUBCLASS_XBOXONE_CONTROLLER
        && id.bInterfaceProtocol == UIPROTO_XBOXONE_GAMEPAD
    {
        return UMATCH_IFACECLASS_IFACESUBCLASS_IFACEPROTO;
    }
    if id.bInterfaceClass != UICLASS_HID {
        return UMATCH_NONE;
    }
    if usbd_get_quirks(uaa.device()).uq_flags & UQ_BAD_HID != 0 {
        return UMATCH_NONE;
    }

    UMATCH_IFACECLASS_GENERIC
}

/// `uhidev_attach_repid`: offer report ID `repid` to the drivers; `false` when it was
/// assigned already (by `uhidev_set_report_dev`).
pub fn uhidev_attach_repid(sc: &UhidevSoftc, uha: &mut UhidevAttachArg<'_>, repid: usize) -> bool {
    // Could already be assigned by uhidev_set_report_dev().
    if sc.subdevs()[repid].get().is_some() {
        return false;
    }

    uha.reportid = repid as u8;
    let dev = config_found_sm(
        &sc.sc_dev,
        ptr::from_mut(uha).cast::<c_void>(),
        Some(uhidevprint),
        None,
    );
    sc.subdevs()[repid].set(dev.map(NonNull::cast::<Uhidev>));
    true
}

/// `uhidev_attach`.
pub fn uhidev_attach(_parent: Option<&Device>, self_: &Device, aux: *mut c_void) {
    let sc = uhidev_softc(self_);
    // SAFETY: as in `uhidev_match`, valid during the attach.
    let uaa = unsafe { &*aux.cast::<UsbAttachArg>() };
    let xname = sc.sc_dev.xname();

    sc.sc_udev.set(Some(uaa.device()));
    sc.sc_iface.set(uaa.iface);
    sc.sc_ifaceno.set(uaa.ifaceno);
    let Some(iface) = uaa.iface else {
        return;
    };
    let Some(id) = usbd_get_interface_descriptor(iface) else {
        return;
    };

    sc.sc_iep_addr.set(-1);
    sc.sc_oep_addr.set(-1);
    for i in 0..id.bNumEndpoints {
        let Some(ed) = usbd_interface2endpoint_descriptor(iface, i) else {
            printf(format_args!(
                "{}: could not read endpoint descriptor\n",
                xname
            ));
            return;
        };

        if ue_get_dir(ed.bEndpointAddress) == UE_DIR_IN
            && ue_get_xfertype(ed.bmAttributes) == UE_INTERRUPT
        {
            sc.sc_iep_addr.set(i32::from(ed.bEndpointAddress));
        } else if ue_get_dir(ed.bEndpointAddress) == UE_DIR_OUT
            && ue_get_xfertype(ed.bmAttributes) == UE_INTERRUPT
        {
            sc.sc_oep_addr.set(i32::from(ed.bEndpointAddress));
        } else {
            printf(format_args!("{}: unexpected endpoint\n", xname));
            return;
        }
    }

    let (desc_mem, size) = match uhidev_use_rdesc(sc, id, uaa.vendor, uaa.product) {
        Err(_) => return,
        Ok(Some(d)) => d,
        Ok(None) => {
            let Some(hid) = usbd_get_hid_descriptor(sc.udev(), id) else {
                printf(format_args!("{}: no HID descriptor\n", xname));
                return;
            };
            let size = usize::from(ugetw(hid.descrs[0].wDescriptorLength));
            if size == 0 {
                printf(format_args!("{}: no report descriptor\n", xname));
                return;
            }
            let Some(mem) = malloc(size, M_USBDEV, M_NOWAIT | M_ZERO) else {
                printf(format_args!("{}: no memory\n", xname));
                return;
            };
            // SAFETY: a fresh zeroed allocation of `size` bytes, ours until it is stored
            // in `sc_repdesc` below or freed here.
            let buf = unsafe { slice::from_raw_parts_mut(mem.as_ptr(), size) };
            if usbd_get_report_descriptor(sc.udev(), sc.sc_ifaceno.get(), buf, size as i32).is_err()
            {
                printf(format_args!("{}: no report descriptor\n", xname));
                free(mem, M_USBDEV, size);
                return;
            }
            (mem, size)
        }
    };

    sc.sc_repdesc.set(desc_mem.as_ptr());
    sc.sc_repdesc_size.set(size as i32);
    let desc = sc.repdesc();

    let nrepid = uhidev_maxrepid(desc);
    if nrepid < 0 {
        return;
    }
    printf(format_args!(
        "{}: iclass {}/{}",
        xname, id.bInterfaceClass, id.bInterfaceSubClass
    ));
    if nrepid > 0 {
        printf(format_args!(
            ", {} report id{}",
            nrepid,
            if nrepid > 1 { "s" } else { "" }
        ));
    }
    printf(format_args!("\n"));
    let nrepid = nrepid as usize + 1;
    let Some(subdevs) = mallocarray(
        nrepid,
        size_of::<Cell<Option<NonNull<Uhidev>>>>(),
        M_USBDEV,
        M_NOWAIT | M_ZERO,
    ) else {
        printf(format_args!("{}: no memory\n", xname));
        return;
    };
    sc.sc_subdevs.set(subdevs.as_ptr().cast());
    sc.sc_nrepid.set(nrepid as u32);
    sc.sc_isize.set(0);

    for repid in 0..nrepid {
        let repsz = hid_report_size(desc, hid_input, repid as u8);
        if repsz > sc.sc_isize.get() as i32 {
            sc.sc_isize.set(repsz as u32);
        }
    }
    // one byte for the report ID
    sc.sc_isize.set(sc.sc_isize.get() + u32::from(nrepid != 1));

    let Some(claimed) = malloc(nrepid, M_TEMP, M_WAITOK | M_ZERO) else {
        printf(format_args!("{}: no memory\n", xname));
        return;
    };
    let mut uha = UhidevAttachArg {
        uaa,
        parent: sc,
        reportid: 0,
        nreports: nrepid as u32,
        claimed: claimed.as_ptr(),
    };

    // Look for a driver claiming multiple report IDs first.
    let dev = config_found_sm(self_, ptr::from_mut(&mut uha).cast::<c_void>(), None, None);
    if let Some(dev) = dev {
        let mut nclaimed = 0;

        // SAFETY: `claimed` is `nrepid` zeroed bytes the claiming driver wrote through
        // `uha.claimed`; `config_found_sm` has returned, so nobody writes them now.
        let flags = unsafe { slice::from_raw_parts(claimed.as_ptr(), nrepid) };
        for (repid, &c) in flags.iter().enumerate() {
            if c == 0 {
                continue;
            }

            nclaimed += 1;
            // Could already be assigned by uhidev_set_report_dev().
            if sc.subdevs()[repid].get().is_none() {
                sc.subdevs()[repid].set(Some(dev.cast::<Uhidev>()));
            }
        }
        kassert!(nclaimed > 0);
        let _ = nclaimed;
    }

    free(claimed, M_TEMP, nrepid);
    uha.claimed = ptr::null_mut();

    // Special case for Wacom tablets
    if uaa.vendor == i32::from(USB_VENDOR_WACOM) {
        let mut ndigitizers = 0;
        // Get all the needed collections (only 3 seem to be of interest currently).
        if let Some(repid) = hid_get_id_of_collection(
            desc,
            hid_usage2(HUP_WACOM | HUP_DIGITIZERS, HUD_STYLUS),
            HCOLL_PHYSICAL,
        ) && (repid as usize) < nrepid
        {
            ndigitizers += u32::from(uhidev_attach_repid(sc, &mut uha, repid as usize));
        }
        if let Some(repid) = hid_get_id_of_collection(
            desc,
            hid_usage2(HUP_WACOM | HUP_DIGITIZERS, HUD_TABLET_FKEYS),
            HCOLL_PHYSICAL,
        ) && (repid as usize) < nrepid
        {
            ndigitizers += u32::from(uhidev_attach_repid(sc, &mut uha, repid as usize));
        }
        // The Wacom battery collection is `#ifdef notyet` in the C ("not handled in
        // hidms_wacom_setup() yet"): not carried.

        if ndigitizers != 0 {
            return;
        }
    }

    for repid in 0..nrepid {
        let r = repid as u8;
        if hid_report_size(desc, hid_input, r) == 0
            && hid_report_size(desc, hid_output, r) == 0
            && hid_report_size(desc, hid_feature, r) == 0
        {
            continue;
        }

        uhidev_attach_repid(sc, &mut uha, repid);
    }
}

/// `uhidev_use_rdesc`: the replacement report descriptor of a device that ships a broken one
/// or none (Wacom Graphire, Xbox 360 and Xbox One gamepads) as a fresh `M_USBDEV` allocation
/// and its size, `Ok(None)` for every other device.
pub fn uhidev_use_rdesc(
    sc: &UhidevSoftc,
    id: &UsbInterfaceDescriptor,
    vendor: i32,
    product: i32,
) -> Result<Option<(NonNull<u8>, usize)>, Errno> {
    let mut reportbuf: [u8; 2] = [2, 2];
    let mut descptr: Option<&'static [u8]> = None;

    if vendor == i32::from(USB_VENDOR_WACOM) {
        // The report descriptor for the Wacom Graphire is broken.
        if product == i32::from(USB_PRODUCT_WACOM_GRAPHIRE) {
            descptr = Some(&UHID_GRAPHIRE_REPORT_DESCR);
        } else if product == i32::from(USB_PRODUCT_WACOM_GRAPHIRE3_4X5)
            || product == i32::from(USB_PRODUCT_WACOM_GRAPHIRE4_4X5)
        {
            let _ = uhidev_set_report(sc, i32::from(UHID_FEATURE_REPORT), 2, &mut reportbuf);
            descptr = Some(&UHID_GRAPHIRE3_4X5_REPORT_DESCR);
        }
    } else if id.bInterfaceClass == UICLASS_VENDOR
        && id.bInterfaceSubClass == UISUBCLASS_XBOX360_CONTROLLER
        && id.bInterfaceProtocol == UIPROTO_XBOX360_GAMEPAD
    {
        // The Xbox 360 gamepad has no report descriptor.
        descptr = Some(&UHID_XB360GP_REPORT_DESCR);
    } else if id.bInterfaceClass == UICLASS_VENDOR
        && id.bInterfaceSubClass == UISUBCLASS_XBOXONE_CONTROLLER
        && id.bInterfaceProtocol == UIPROTO_XBOXONE_GAMEPAD
    {
        sc.sc_flags.set(sc.sc_flags.get() | UHIDEV_F_XB1);
        // The Xbox One gamepad has no report descriptor.
        descptr = Some(&UHID_XBONEGP_REPORT_DESCR);
    }

    let Some(descptr) = descptr else {
        return Ok(None);
    };
    let size = descptr.len();
    let Some(desc) = malloc(size, M_USBDEV, M_NOWAIT) else {
        return Err(Errno::ENOMEM);
    };
    // SAFETY: a fresh allocation of `size` bytes, not overlapping the static table.
    unsafe { ptr::copy_nonoverlapping(descptr.as_ptr(), desc.as_ptr(), size) };

    Ok(Some((desc, size)))
}

/// `uhidev_maxrepid`: the largest report ID in the descriptor, `-1` when it has no items.
pub fn uhidev_maxrepid(buf: &[u8]) -> i32 {
    let mut maxid: i32 = -1;
    let mut h = HidItem {
        report_ID: 0,
        ..HidItem::default()
    };
    let mut d = hid_start_parse(buf, hid_all);
    while hid_get_item(&mut d, &mut h) {
        if h.report_ID as i32 > maxid {
            maxid = h.report_ID as i32;
        }
    }
    hid_end_parse(d);
    maxid
}

/// `uhidevprint`: the `config_found` print function of the children.
pub fn uhidevprint(aux: *mut c_void, pnp: Option<&[u8]>) -> i32 {
    // SAFETY: `uhidev_attach` and `uhidev_attach_repid` pass a `UhidevAttachArg` that lives
    // across the `config_found_sm` calling this.
    let uha = unsafe { &*aux.cast::<UhidevAttachArg<'_>>() };

    if let Some(pnp) = pnp {
        printf(format_args!("uhid at {}", Str(pnp)));
    }
    if uha.reportid != 0 {
        printf(format_args!(" reportid {}", uha.reportid));
    }
    UNCONF
}

/// `uhidev_activate`: the device is going away: tell each child (once, a driver claiming
/// several report IDs is in several slots) and mark the USB device dying.
pub fn uhidev_activate(self_: &Device, act: i32) -> Result<(), Errno> {
    let sc = uhidev_softc(self_);
    let mut rv = Ok(());

    if act == DVACT_DEACTIVATE {
        let subs = sc.subdevs();
        for (i, slot) in subs.iter().enumerate() {
            let Some(dev) = slot.get() else {
                continue;
            };

            // Only notify devices attached to multiple report ids once.
            if subs[..i].iter().any(|s| s.get() == Some(dev)) {
                continue;
            }

            // SAFETY: an attached child, in `sc_subdevs` until `uhidev_detach`.
            let r = config_deactivate(unsafe { dev.cast::<Device>().as_ref() });
            if let Err(e) = r
                && e != Errno::EOPNOTSUPP
            {
                rv = Err(e);
            }
        }
        usbd_deactivate(sc.udev());
    }
    rv
}

/// `uhidev_detach`: close the pipes, detach the children and free the descriptor.
pub fn uhidev_detach(self_: &Device, flags: i32) -> Result<(), Errno> {
    let sc = uhidev_softc(self_);
    let mut rv = Ok(());

    if let Some(p) = sc.sc_opipe.take() {
        // SAFETY: the open output pipe, forgotten by the `take`.
        unsafe { usbd_close_pipe(NonNull::from(p)) };
    }

    if let Some(p) = sc.sc_ipipe.take() {
        // SAFETY: the open input pipe, forgotten by the `take`.
        unsafe { usbd_close_pipe(NonNull::from(p)) };
    }

    if let Some(d) = NonNull::new(sc.sc_repdesc.get()) {
        free(d, M_USBDEV, sc.sc_repdesc_size.get() as usize);
        sc.sc_repdesc.set(ptr::null_mut());
    }

    let subs = sc.subdevs();
    for (i, slot) in subs.iter().enumerate() {
        let Some(dev) = slot.get() else {
            continue;
        };

        // SAFETY: an attached child, never used again through this table (the slot is
        // cleared below).
        let r = unsafe { config_detach(dev.cast::<Device>(), flags) };
        if rv.is_ok() {
            rv = r;
        }

        // Nullify without detaching any other instances of this device found on other
        // report ids.
        for other in &subs[i + 1..] {
            if other.get() == Some(dev) {
                other.set(None);
            }
        }

        slot.set(None);
    }
    if let Some(s) = NonNull::new(sc.sc_subdevs.get()) {
        free(
            s.cast(),
            M_USBDEV,
            sc.sc_nrepid.get() as usize * size_of::<Cell<Option<NonNull<Uhidev>>>>(),
        );
        sc.sc_subdevs.set(ptr::null_mut());
        sc.sc_nrepid.set(0);
    }

    rv
}

/// Where the report is in the bytes a transfer brought in: the report ID and the offset of
/// the report after it. With one report ID (`nrepid == 1`) the device sends no ID byte and the
/// ID is 0; otherwise the first byte is the ID, and `None` is a transfer too short to have it
/// (the C's `cc--` would wrap).
fn uhidev_split_report(nrepid: usize, buf: &[u8]) -> Option<(usize, usize)> {
    if nrepid != 1 {
        buf.first().map(|&rep| (usize::from(rep), 1))
    } else {
        Some((0, 0))
    }
}

/// `uhidev_intr`: the input pipe's callback: split the report ID off the report and hand it
/// to the child that owns it (if it has the device open).
pub fn uhidev_intr(xfer: &'static UsbdXfer, addr: *mut c_void, status: UsbdStatus) {
    // SAFETY: `uhidev_open` passes the softc as the pipe's `priv`; it outlives the pipe
    // (`uhidev_detach` closes the pipes before the softc is freed).
    let sc = unsafe { &*addr.cast::<UhidevSoftc>() };
    let mut cc: u32 = 0;

    if usbd_is_dying(sc.udev()) {
        return;
    }

    usbd_get_xfer_status(xfer, None, None, Some(&mut cc), None);

    if status == USBD_CANCELLED || status == USBD_IOERROR {
        return;
    }

    if status != USBD_NORMAL_COMPLETION {
        if let Some(p) = sc.sc_ipipe.get() {
            let _ = usbd_clear_endpoint_stall_async(p);
        }
        return;
    }

    let Some(ibuf) = NonNull::new(sc.sc_ibuf.get()) else {
        return;
    };
    let cc = (cc as usize).min(sc.sc_isize.get() as usize);
    // SAFETY: `sc_ibuf` is `sc_isize` bytes; the transfer copied `cc` of them in before this
    // callback and the next one starts after it returns.
    let buf = unsafe { slice::from_raw_parts_mut(ibuf.as_ptr(), cc) };
    let Some((rep, off)) = uhidev_split_report(sc.sc_nrepid.get() as usize, buf) else {
        return;
    };
    if rep >= sc.sc_nrepid.get() as usize {
        printf(format_args!("uhidev_intr: bad repid {}\n", rep));
        return;
    }
    let Some(scd) = sc.subdevs()[rep].get() else {
        return;
    };
    // SAFETY: an attached child, in `sc_subdevs` until `uhidev_detach`.
    let scd = unsafe { scd.as_ref() };
    if scd.sc_state.get() & UHIDEV_OPEN == 0 {
        return;
    }
    let Some(intr) = scd.sc_intr.get() else {
        return;
    };

    intr(scd, &mut buf[off..]);
}

/// `uhidev_get_report_desc`: the report descriptor of the interface.
pub fn uhidev_get_report_desc(sc: &UhidevSoftc) -> &[u8] {
    sc.repdesc()
}

/// `uhidev_open`: a child opens the device: the first one opens the pipes. `EBUSY` if this
/// child has it open already.
pub fn uhidev_open(scd: &Uhidev) -> Result<(), Errno> {
    let sc = scd.parent();
    let error: Errno;

    if scd.sc_state.get() & UHIDEV_OPEN != 0 {
        return Err(Errno::EBUSY);
    }
    scd.sc_state.set(scd.sc_state.get() | UHIDEV_OPEN);
    let refcnt = sc.sc_refcnt.get();
    sc.sc_refcnt.set(refcnt + 1);
    if refcnt != 0 {
        return Ok(());
    }

    'out1: {
        'out2: {
            'out3: {
                // Set up input interrupt pipe.
                if sc.sc_isize.get() != 0 {
                    let Some(ibuf) = malloc(sc.sc_isize.get() as usize, M_USBDEV, M_WAITOK) else {
                        error = Errno::ENOMEM;
                        break 'out1;
                    };
                    sc.sc_ibuf.set(ibuf.as_ptr());

                    let Some(iface) = sc.sc_iface.get() else {
                        error = Errno::EIO;
                        break 'out1;
                    };
                    // SAFETY: `sc_ibuf` is `sc_isize` bytes, freed only after the pipe is
                    // closed (`uhidev_close`, the `out1` path); the softc outlives the pipe.
                    let r = unsafe {
                        usbd_open_pipe_intr(
                            iface,
                            sc.sc_iep_addr.get() as u8,
                            USBD_SHORT_XFER_OK as u8,
                            ptr::from_ref(sc).cast_mut().cast(),
                            ibuf.as_ptr(),
                            sc.sc_isize.get(),
                            uhidev_intr,
                            USBD_DEFAULT_INTERVAL,
                        )
                    };
                    match r {
                        Ok(p) => sc.sc_ipipe.set(Some(p)),
                        Err(_) => {
                            error = Errno::EIO;
                            break 'out1;
                        }
                    }

                    sc.sc_ixfer.set(usbd_alloc_xfer(sc.udev()));
                    if sc.sc_ixfer.get().is_none() {
                        error = Errno::ENOMEM;
                        break 'out1; // xxxx
                    }
                }

                // Set up output interrupt pipe if an output interrupt endpoint exists.
                if sc.sc_oep_addr.get() != -1 {
                    let Some(iface) = sc.sc_iface.get() else {
                        error = Errno::EIO;
                        break 'out2;
                    };
                    match usbd_open_pipe(iface, sc.sc_oep_addr.get() as u8, 0) {
                        Ok(p) => sc.sc_opipe.set(Some(p)),
                        Err(_) => {
                            error = Errno::EIO;
                            break 'out2;
                        }
                    }

                    sc.sc_oxfer.set(usbd_alloc_xfer(sc.udev()));
                    if sc.sc_oxfer.get().is_none() {
                        error = Errno::ENOMEM;
                        break 'out3;
                    }

                    sc.sc_owxfer.set(usbd_alloc_xfer(sc.udev()));
                    if sc.sc_owxfer.get().is_none() {
                        error = Errno::ENOMEM;
                        break 'out3;
                    }

                    // XBox One controller initialization
                    if sc.sc_flags.get() & UHIDEV_F_XB1 != 0
                        && let (Some(oxfer), Some(opipe)) = (sc.sc_oxfer.get(), sc.sc_opipe.get())
                    {
                        let mut init_data: [u8; 5] = [0x05, 0x20, 0x00, 0x01, 0x00];
                        // SAFETY: a synchronous transfer: `init_data` outlives
                        // `usbd_transfer`.
                        unsafe {
                            usbd_setup_xfer(
                                oxfer,
                                opipe,
                                ptr::null_mut(),
                                init_data.as_mut_ptr(),
                                init_data.len() as u32,
                                USBD_SYNCHRONOUS | USBD_CATCH,
                                USBD_NO_TIMEOUT,
                                None,
                            )
                        };
                        if usbd_transfer(oxfer).is_err() {
                            error = Errno::EIO;
                            break 'out3;
                        }
                    }
                }

                return Ok(());
            }
            // out3: Abort output pipe
            if let Some(p) = sc.sc_opipe.get() {
                // SAFETY: the open output pipe; `out1` forgets it.
                unsafe { usbd_close_pipe(NonNull::from(p)) };
            }
        }
        // out2: Abort input pipe
        if let Some(p) = sc.sc_ipipe.get() {
            // SAFETY: the open input pipe; `out1` forgets it.
            unsafe { usbd_close_pipe(NonNull::from(p)) };
        }
    }
    // out1: failed in some way
    if let Some(b) = NonNull::new(sc.sc_ibuf.get()) {
        free(b, M_USBDEV, sc.sc_isize.get() as usize);
    }
    sc.sc_ibuf.set(ptr::null_mut());
    scd.sc_state.set(scd.sc_state.get() & !UHIDEV_OPEN);
    sc.sc_refcnt.set(0);
    sc.sc_ipipe.set(None);
    sc.sc_opipe.set(None);
    for x in [&sc.sc_oxfer, &sc.sc_owxfer, &sc.sc_ixfer] {
        if let Some(xfer) = x.take() {
            // SAFETY: an xfer `uhidev_open` allocated; nothing refers to it any more.
            unsafe { usbd_free_xfer(NonNull::from(xfer)) };
        }
    }
    Err(error)
}

/// `uhidev_close`: a child closes the device: the last one closes the pipes.
pub fn uhidev_close(scd: &Uhidev) {
    let sc = scd.parent();

    if scd.sc_state.get() & UHIDEV_OPEN == 0 {
        return;
    }
    scd.sc_state.set(scd.sc_state.get() & !UHIDEV_OPEN);
    sc.sc_refcnt.set(sc.sc_refcnt.get() - 1);
    if sc.sc_refcnt.get() != 0 {
        return;
    }

    // Disable interrupts.
    if let Some(p) = sc.sc_opipe.take() {
        // SAFETY: the open output pipe, forgotten by the `take`.
        unsafe { usbd_close_pipe(NonNull::from(p)) };
    }

    if let Some(p) = sc.sc_ipipe.take() {
        // SAFETY: the open input pipe, forgotten by the `take`.
        unsafe { usbd_close_pipe(NonNull::from(p)) };
    }

    for x in [&sc.sc_oxfer, &sc.sc_owxfer, &sc.sc_ixfer] {
        if let Some(xfer) = x.take() {
            // SAFETY: an xfer `uhidev_open` allocated; nothing refers to it any more.
            unsafe { usbd_free_xfer(NonNull::from(xfer)) };
        }
    }

    if let Some(b) = NonNull::new(sc.sc_ibuf.get()) {
        free(b, M_USBDEV, sc.sc_isize.get() as usize);
        sc.sc_ibuf.set(ptr::null_mut());
    }
}

/// `uhidev_report_type_conv`: the `UHID_*_REPORT` number of a parse's item kind, `None` for
/// a kind that is not a report type.
pub fn uhidev_report_type_conv(hid_type_id: HidKind) -> Option<u8> {
    match hid_type_id {
        hid_input => Some(UHID_INPUT_REPORT),
        hid_output => Some(UHID_OUTPUT_REPORT),
        hid_feature => Some(UHID_FEATURE_REPORT),
        _ => None,
    }
}

/// A class request on the interface for report `id` of type `type_`: `wValue` is the type
/// and the ID, `wIndex` the interface, `wLength` `len`.
fn report_request(
    sc: &UhidevSoftc,
    bm: u8,
    b: u8,
    type_: i32,
    id: i32,
    len: usize,
) -> UsbDeviceRequest {
    let mut req = UsbDeviceRequest {
        bmRequestType: bm,
        bRequest: b,
        ..UsbDeviceRequest::default()
    };
    usetw2(&mut req.wValue, type_ as u8, id as u8);
    usetw(&mut req.wIndex, sc.sc_ifaceno.get() as u16);
    usetw(&mut req.wLength, len as u16);
    req
}

/// `uhidev_set_report`: send a report of type `type_` and ID `id` (prepended to the data
/// when not 0) on the output pipe, or as a SET_REPORT request when there is none. The
/// number of bytes of `data` sent.
pub fn uhidev_set_report(
    sc: &UhidevSoftc,
    type_: i32,
    id: i32,
    data: &mut [u8],
) -> Result<usize, Errno> {
    let actlen = data.len();
    let mut tmp: Vec<u8> = Vec::new();

    // Prepend the reportID.
    let buf: &mut [u8] = if id > 0 {
        tmp.try_reserve_exact(data.len() + 1)
            .map_err(|_| Errno::ENOMEM)?;
        tmp.push(id as u8);
        tmp.extend_from_slice(data);
        &mut tmp
    } else {
        data
    };

    let mut failed = false;
    if let Some(opipe) = sc.sc_opipe.get() {
        let Some(owxfer) = sc.sc_owxfer.get() else {
            return Err(Errno::EIO);
        };
        // SAFETY: a synchronous transfer: `buf` outlives `usbd_transfer`.
        unsafe {
            usbd_setup_xfer(
                owxfer,
                opipe,
                ptr::null_mut(),
                buf.as_mut_ptr(),
                buf.len() as u32,
                USBD_SYNCHRONOUS | USBD_CATCH,
                0,
                None,
            )
        };
        if usbd_transfer(owxfer).is_err() {
            let _ = usbd_clear_endpoint_stall(opipe);
            failed = true;
        }
    } else {
        let req = report_request(
            sc,
            UT_WRITE_CLASS_INTERFACE,
            UR_SET_REPORT,
            type_,
            id,
            buf.len(),
        );

        if usbd_do_request(sc.udev(), &req, buf).is_err() {
            failed = true;
        }
    }

    if failed { Err(Errno::EIO) } else { Ok(actlen) }
}

/// `uhidev_set_report_async_cb`: the end of an asynchronous report on the output pipe.
pub fn uhidev_set_report_async_cb(xfer: &'static UsbdXfer, priv_: *mut c_void, err: UsbdStatus) {
    // SAFETY: `uhidev_set_report_async` passes the softc as `priv`; it outlives the xfer.
    let sc = unsafe { &*priv_.cast::<UhidevSoftc>() };

    if err == USBD_STALLED
        && let Some(p) = sc.sc_opipe.get()
    {
        let _ = usbd_clear_endpoint_stall_async(p);
    }
    // SAFETY: the xfer belongs to this callback, which is its last user.
    unsafe { usbd_free_xfer(NonNull::from(xfer)) };
}

/// `uhidev_set_report_async`: as [`uhidev_set_report`], without waiting; the data is copied.
pub fn uhidev_set_report_async(
    sc: &UhidevSoftc,
    type_: i32,
    id: i32,
    data: &[u8],
) -> Result<usize, Errno> {
    let actlen = data.len();
    let mut len = data.len();

    let Some(xfer) = usbd_alloc_xfer(sc.udev()) else {
        return Err(Errno::EIO);
    };

    if id > 0 {
        len += 1;
    }

    let Some(buf) = usbd_alloc_buffer(xfer, len as u32) else {
        // SAFETY: our xfer, never started.
        unsafe { usbd_free_xfer(NonNull::from(xfer)) };
        return Err(Errno::EIO);
    };

    // Prepend the reportID.
    // SAFETY: `buf` is `len` bytes (`data.len()` plus the ID byte when there is one) of the
    // xfer's DMA buffer, not overlapping `data`.
    unsafe {
        if id > 0 {
            *buf.as_ptr() = id as u8;
            ptr::copy_nonoverlapping(data.as_ptr(), buf.as_ptr().add(1), len - 1);
        } else {
            ptr::copy_nonoverlapping(data.as_ptr(), buf.as_ptr(), len);
        }
    }

    let mut failed = false;
    if let Some(opipe) = sc.sc_opipe.get() {
        // SAFETY: `USBD_NO_COPY`: `buf` is the xfer's own DMA buffer.
        unsafe {
            usbd_setup_xfer(
                xfer,
                opipe,
                ptr::from_ref(sc).cast_mut().cast(),
                buf.as_ptr(),
                len as u32,
                USBD_NO_COPY,
                USBD_DEFAULT_TIMEOUT,
                Some(uhidev_set_report_async_cb),
            )
        };
        if usbd_transfer(xfer).is_err() {
            let _ = usbd_clear_endpoint_stall_async(opipe);
            failed = true;
        }
    } else {
        let req = report_request(sc, UT_WRITE_CLASS_INTERFACE, UR_SET_REPORT, type_, id, len);
        if usbd_request_async(xfer, &req, ptr::null_mut(), None).is_err() {
            failed = true;
        }
    }

    if failed { Err(Errno::EIO) } else { Ok(actlen) }
}

/// `uhidev_get_report`: read a report of type `type_` and ID `id` with a GET_REPORT request
/// into `data`. The number of bytes the device returned (including the report ID when
/// `id` is not 0).
pub fn uhidev_get_report(
    sc: &UhidevSoftc,
    type_: i32,
    id: i32,
    data: &mut [u8],
) -> Result<usize, Errno> {
    let mut tmp: Vec<u8> = Vec::new();
    let mut actlen: i32 = 0;

    let buf: &mut [u8] = if id > 0 {
        tmp.try_reserve_exact(data.len() + 1)
            .map_err(|_| Errno::ENOMEM)?;
        tmp.resize(data.len() + 1, 0);
        &mut tmp
    } else {
        &mut *data
    };

    let req = report_request(
        sc,
        UT_READ_CLASS_INTERFACE,
        UR_GET_REPORT,
        type_,
        id,
        buf.len(),
    );

    let err = usbd_do_request_flags(
        sc.udev(),
        &req,
        buf,
        0,
        Some(&mut actlen),
        USBD_DEFAULT_TIMEOUT,
    );
    let failed = err != USBD_NORMAL_COMPLETION && err != USBD_SHORT_XFER;

    // Skip the reportID.
    if id > 0 {
        let n = data.len();
        data.copy_from_slice(&tmp[1..=n]);
    }

    if failed {
        Err(Errno::EIO)
    } else {
        Ok(actlen as usize)
    }
}

/// `uhidev_get_report_async_cb`: the end of `uhidev_get_report_async`: copy the report to the
/// caller's buffer (without the report ID) and call it back.
pub fn uhidev_get_report_async_cb(xfer: &'static UsbdXfer, priv_: *mut c_void, err: UsbdStatus) {
    let info = priv_.cast::<UhidevAsyncInfo>();
    // SAFETY: `uhidev_get_report_async` hands the `malloc`ed info, which this callback owns
    // and frees below.
    let i = unsafe { &*info };
    let mut len: i32 = -1;

    if !usbd_is_dying(xfer.pipe().device()) {
        if err == USBD_NORMAL_COMPLETION || err == USBD_SHORT_XFER {
            len = xfer.actlen.get() as i32;
            let buf = kernaddr(&xfer.dmabuf, 0);
            // SAFETY: the DMA buffer holds `actlen` bytes the device wrote; `i.data` is the
            // caller's buffer, which `uhidev_get_report_async`'s contract keeps valid for
            // the length it asked for.
            unsafe {
                if i.id > 0 {
                    len -= 1;
                    ptr::copy_nonoverlapping(buf.add(1), i.data, len.max(0) as usize);
                } else {
                    ptr::copy_nonoverlapping(buf, i.data, len.max(0) as usize);
                }
            }
        }
        (i.callback)(i.priv_, i.id, i.data, len);
    }
    if let Some(n) = NonNull::new(info) {
        free(n.cast(), M_TEMP, size_of::<UhidevAsyncInfo>());
    }
    // SAFETY: the xfer belongs to this callback, which is its last user.
    unsafe { usbd_free_xfer(NonNull::from(xfer)) };
}

/// `uhidev_get_report_async`: as [`uhidev_get_report`], without waiting: the report is
/// copied to `data` and `callback(priv_, id, data, len)` is called when it arrives.
///
/// # Safety
///
/// `data` is valid for `len` bytes (the C's contract) until the callback has run.
pub unsafe fn uhidev_get_report_async(
    sc: &UhidevSoftc,
    type_: i32,
    id: i32,
    data: *mut u8,
    len: usize,
    priv_: *mut c_void,
    callback: UhidevGetReportCb,
) -> Result<usize, Errno> {
    let actlen = len;
    let mut len = len;

    let Some(xfer) = usbd_alloc_xfer(sc.udev()) else {
        return Err(Errno::EIO);
    };

    if id > 0 {
        len += 1;
    }

    if usbd_alloc_buffer(xfer, len as u32).is_none() {
        // SAFETY: our xfer, never started.
        unsafe { usbd_free_xfer(NonNull::from(xfer)) };
        return Err(Errno::EIO);
    }

    let Some(mem) = malloc(size_of::<UhidevAsyncInfo>(), M_TEMP, M_NOWAIT) else {
        // SAFETY: our xfer, never started.
        unsafe { usbd_free_xfer(NonNull::from(xfer)) };
        return Err(Errno::EIO);
    };
    let info = mem.cast::<UhidevAsyncInfo>();
    // SAFETY: a fresh allocation of a `UhidevAsyncInfo`'s size (malloc aligns for any
    // type), written whole; `uhidev_get_report_async_cb` frees it.
    unsafe {
        info.write(UhidevAsyncInfo {
            callback,
            priv_,
            data,
            id,
        });
    }

    let req = report_request(sc, UT_READ_CLASS_INTERFACE, UR_GET_REPORT, type_, id, len);

    if usbd_request_async(
        xfer,
        &req,
        info.as_ptr().cast(),
        Some(uhidev_get_report_async_cb),
    )
    .is_err()
    {
        free(mem, M_TEMP, size_of::<UhidevAsyncInfo>());
        return Err(Errno::EIO);
    }

    Ok(actlen)
}

/// `uhidev_write`: write `data` on the output pipe, synchronously.
pub fn uhidev_write(sc: &UhidevSoftc, data: &mut [u8]) -> UsbdStatus {
    let (Some(opipe), Some(owxfer)) = (sc.sc_opipe.get(), sc.sc_owxfer.get()) else {
        return USBD_INVAL;
    };

    // SAFETY: a synchronous transfer: `data` outlives `usbd_transfer`.
    unsafe {
        usbd_setup_xfer(
            owxfer,
            opipe,
            ptr::null_mut(),
            data.as_mut_ptr(),
            data.len() as u32,
            USBD_SYNCHRONOUS | USBD_CATCH,
            0,
            None,
        )
    };
    let error = usbd_transfer(owxfer);
    if error.is_err() {
        let _ = usbd_clear_endpoint_stall(opipe);
    }

    error
}

/// The size of the report of type `ucr_report` for `uhidev_ioctl`'s `USB_GET_REPORT` and
/// `USB_SET_REPORT`.
fn report_size(sc: &Uhidev, ucr_report: i32) -> Result<usize, Errno> {
    let size = if ucr_report == i32::from(UHID_INPUT_REPORT) {
        sc.sc_isize.get()
    } else if ucr_report == i32::from(UHID_OUTPUT_REPORT) {
        sc.sc_osize.get()
    } else if ucr_report == i32::from(UHID_FEATURE_REPORT) {
        sc.sc_fsize.get()
    } else {
        return Err(Errno::EINVAL);
    };
    Ok(size as usize)
}

/// `uhidev_ioctl`: the generic HID ioctls (`USB_GET_REPORT_DESC`, `USB_GET_REPORT`,
/// `USB_SET_REPORT`, `USB_GET_REPORT_ID`) for a child. `Ok(false)` where the C returns `-1`:
/// not one of them.
pub fn uhidev_ioctl(
    sc: &Uhidev,
    cmd: u64,
    data: &mut [u8],
    flag: i32,
    p: Option<&Proc>,
) -> Result<bool, Errno> {
    let _ = (flag, p);
    match cmd {
        USB_GET_REPORT_DESC => {
            let mut rd = ioctl_arg::<UsbCtlReportDesc>(data);
            let desc = uhidev_get_report_desc(sc.parent());
            let size = desc.len().min(rd.ucrd_data.len());
            rd.ucrd_size = size as i32;
            rd.ucrd_data[..size].copy_from_slice(&desc[..size]);
            ioctl_ret(data, &rd);
        }
        USB_GET_REPORT => {
            let mut re = ioctl_arg::<UsbCtlReport>(data);
            let size = report_size(sc, re.ucr_report)?;
            let buf = re.ucr_data.get_mut(..size).ok_or(Errno::EINVAL)?;
            let r = uhidev_get_report(
                sc.parent(),
                re.ucr_report,
                i32::from(sc.sc_report_id.get()),
                buf,
            );
            if r != Ok(size) {
                return Err(Errno::EIO);
            }
            ioctl_ret(data, &re);
        }
        USB_SET_REPORT => {
            let mut re = ioctl_arg::<UsbCtlReport>(data);
            let size = report_size(sc, re.ucr_report)?;
            let buf = re.ucr_data.get_mut(..size).ok_or(Errno::EINVAL)?;
            let r = uhidev_set_report(
                sc.parent(),
                re.ucr_report,
                i32::from(sc.sc_report_id.get()),
                buf,
            );
            if r != Ok(size) {
                return Err(Errno::EIO);
            }
        }
        USB_GET_REPORT_ID => {
            ioctl_ret(data, &i32::from(sc.sc_report_id.get()));
        }
        _ => return Ok(false),
    }
    Ok(true)
}

/// `uhidev_set_report_dev`: give report ID `repid` to the child `dev` (which has the device
/// open), as a driver that claimed several report IDs does for those it did not.
pub fn uhidev_set_report_dev(sc: &UhidevSoftc, dev: &Uhidev, repid: usize) -> Result<(), Errno> {
    if dev.sc_state.get() & UHIDEV_OPEN == 0 {
        return Err(Errno::ENODEV);
    }
    if repid >= sc.sc_nrepid.get() as usize {
        return Err(Errno::EINVAL);
    }

    sc.subdevs()[repid].set(Some(NonNull::from(dev)));
    Ok(())
}
/* </CODE> */

/* <TESTS> */
#[cfg(test)]
mod tests {
    // Host tests for `uhidev`: the report ID split of `uhidev_intr`, the descriptor's largest
    // report ID, the replacement descriptors `uhidev_use_rdesc` hands out, the generic HID
    // ioctls, and the child table (`uhidev_set_report_dev`).

    use std::boxed::Box;
    use std::mem::MaybeUninit;
    use std::vec::Vec;
    use std::{assert, assert_eq};

    use super::*;
    use crate::dev::hid::hid::hid_collection;
    use crate::dev::usb::usb::UsbInterfaceDescriptor;
    use crate::kern::subr_pool::tests::setup_real_memory;

    /// QEMU's `usb-kbd` report descriptor: one report, no report ID.
    const QEMU_KBD: &[u8] = &[
        0x05, 0x01, 0x09, 0x06, 0xa1, 0x01, 0x75, 0x01, 0x95, 0x08, 0x05, 0x07, 0x19, 0xe0, 0x29,
        0xe7, 0x15, 0x00, 0x25, 0x01, 0x81, 0x02, 0x95, 0x01, 0x75, 0x08, 0x81, 0x01, 0x95, 0x05,
        0x75, 0x01, 0x05, 0x08, 0x19, 0x01, 0x29, 0x05, 0x91, 0x02, 0x95, 0x01, 0x75, 0x03, 0x91,
        0x01, 0x95, 0x06, 0x75, 0x08, 0x15, 0x00, 0x25, 0xff, 0x05, 0x07, 0x19, 0x00, 0x29, 0xff,
        0x81, 0x00, 0xc0,
    ];

    /// A zeroed softc, as `config_make_softc` makes one, that lives as long as the test needs.
    fn zeroed<T: Softc>() -> &'static T {
        // SAFETY: the `Softc` contract: all-zero is a valid `T`.
        Box::leak(Box::new(unsafe {
            MaybeUninit::<T>::zeroed().assume_init()
        }))
    }

    #[test]
    fn the_report_id_is_the_first_byte_unless_there_is_only_one() {
        // One report ID: the device sends no ID byte.
        assert_eq!(uhidev_split_report(1, &[1, 2, 3]), Some((0, 0)));
        assert_eq!(uhidev_split_report(1, &[]), Some((0, 0)));
        // Several: the first byte is the ID, and the report starts after it.
        assert_eq!(uhidev_split_report(3, &[2, 0xaa, 0xbb]), Some((2, 1)));
        assert_eq!(uhidev_split_report(3, &[0, 0xaa]), Some((0, 1)));
        // A transfer without the ID byte (the C's `cc--` would wrap).
        assert_eq!(uhidev_split_report(3, &[]), None);
    }

    #[test]
    fn maxrepid_of_a_descriptor() {
        assert_eq!(uhidev_maxrepid(QEMU_KBD), 0);
        assert_eq!(uhidev_maxrepid(&[]), -1);
        // REPORT_ID (5) in an otherwise minimal collection.
        assert_eq!(
            uhidev_maxrepid(&[
                0x05, 0x01, 0x09, 0x06, 0xa1, 0x01, 0x85, 0x05, 0x75, 0x08, 0x95, 0x01, 0x81, 0x02,
                0xc0
            ]),
            5
        );
    }

    #[test]
    fn report_types_map_to_the_uhid_numbers() {
        assert_eq!(uhidev_report_type_conv(hid_input), Some(UHID_INPUT_REPORT));
        assert_eq!(
            uhidev_report_type_conv(hid_output),
            Some(UHID_OUTPUT_REPORT)
        );
        assert_eq!(
            uhidev_report_type_conv(hid_feature),
            Some(UHID_FEATURE_REPORT)
        );
        assert_eq!(uhidev_report_type_conv(hid_collection), None);
        assert_eq!(uhidev_report_type_conv(hid_all), None);
    }

    #[test]
    fn a_child_offered_one_report_id_does_not_claim_several() {
        let sc: &'static UhidevSoftc = zeroed();
        let uaa: &'static UsbAttachArg = {
            // SAFETY: only the `Option`s, integers and raw pointers of the argument are read;
            // all-zero is `None`, 0 and null.
            Box::leak(Box::new(unsafe {
                MaybeUninit::<UsbAttachArg>::zeroed().assume_init()
            }))
        };
        let mut claimed = [0u8; 2];
        let mut uha = UhidevAttachArg {
            uaa,
            parent: sc,
            reportid: 0,
            nreports: 2,
            claimed: core::ptr::null_mut(),
        };
        assert!(!uha.claim_multiple_reportid());
        uha.claimed = claimed.as_mut_ptr();
        assert!(uha.claim_multiple_reportid());
    }

    fn iface(class: u8, sub: u8, proto: u8) -> UsbInterfaceDescriptor {
        UsbInterfaceDescriptor {
            bLength: 9,
            bInterfaceClass: class,
            bInterfaceSubClass: sub,
            bInterfaceProtocol: proto,
            ..UsbInterfaceDescriptor::default()
        }
    }

    /// The bytes `uhidev_use_rdesc` allocated, freed.
    fn take(r: Result<Option<(NonNull<u8>, usize)>, Errno>) -> Option<Vec<u8>> {
        let (p, n) = r.unwrap()?;
        // SAFETY: a fresh `n`-byte `M_USBDEV` allocation, initialised by the copy.
        let v = unsafe { slice::from_raw_parts(p.as_ptr(), n) }.to_vec();
        free(p, M_USBDEV, n);
        Some(v)
    }

    #[test]
    fn broken_or_missing_descriptors_are_replaced() {
        let _g = setup_real_memory();
        let sc: &UhidevSoftc = zeroed();
        let hid = iface(UICLASS_HID, 1, 1);

        // The Wacom Graphire has one, the Graphire3 sends a SET_REPORT first (needs a device:
        // not here).
        let d = take(uhidev_use_rdesc(
            sc,
            &hid,
            i32::from(USB_VENDOR_WACOM),
            i32::from(USB_PRODUCT_WACOM_GRAPHIRE),
        ))
        .unwrap();
        assert_eq!(d, UHID_GRAPHIRE_REPORT_DESCR);

        // A Wacom product with no replacement, and any other HID device: none.
        assert_eq!(
            take(uhidev_use_rdesc(
                sc,
                &hid,
                i32::from(USB_VENDOR_WACOM),
                0x1234
            )),
            None
        );
        assert_eq!(take(uhidev_use_rdesc(sc, &hid, 0x0627, 1)), None);

        // The Xbox 360 gamepad has no descriptor.
        let x360 = iface(
            UICLASS_VENDOR,
            UISUBCLASS_XBOX360_CONTROLLER,
            UIPROTO_XBOX360_GAMEPAD,
        );
        let d = take(uhidev_use_rdesc(sc, &x360, 0x045e, 0x028e)).unwrap();
        assert_eq!(d, UHID_XB360GP_REPORT_DESCR);
        assert_eq!(sc.sc_flags.get() & UHIDEV_F_XB1, 0);

        // The Xbox One's too, and the softc remembers which one it is.
        let xone = iface(
            UICLASS_VENDOR,
            UISUBCLASS_XBOXONE_CONTROLLER,
            UIPROTO_XBOXONE_GAMEPAD,
        );
        let d = take(uhidev_use_rdesc(sc, &xone, 0x045e, 0x02ea)).unwrap();
        assert_eq!(d, UHID_XBONEGP_REPORT_DESCR);
        assert_eq!(sc.sc_flags.get() & UHIDEV_F_XB1, UHIDEV_F_XB1);
    }

    /// A child of a parent whose report descriptor is `desc`, for the ioctls.
    fn child(desc: &'static [u8], report_id: u8) -> &'static Uhidev {
        let sc: &'static UhidevSoftc = zeroed();
        sc.sc_repdesc.set(desc.as_ptr().cast_mut());
        sc.sc_repdesc_size.set(desc.len() as i32);
        let scd: &'static Uhidev = zeroed();
        scd.sc_parent.set(Some(sc));
        scd.sc_report_id.set(report_id);
        scd.sc_isize.set(8);
        scd
    }

    #[test]
    fn the_generic_hid_ioctls() {
        let p = &crate::kern::init_main::PROC0;
        let scd = child(QEMU_KBD, 3);

        // USB_GET_REPORT_ID
        let mut data = [0u8; 4];
        assert_eq!(
            uhidev_ioctl(scd, USB_GET_REPORT_ID, &mut data, 0, Some(p)),
            Ok(true)
        );
        assert_eq!(ioctl_arg::<i32>(&data), 3);

        // USB_GET_REPORT_DESC: the size and the bytes.
        let mut data = std::vec![0u8; size_of::<UsbCtlReportDesc>()];
        assert_eq!(
            uhidev_ioctl(scd, USB_GET_REPORT_DESC, &mut data, 0, Some(p)),
            Ok(true)
        );
        let rd = ioctl_arg::<UsbCtlReportDesc>(&data);
        assert_eq!(rd.ucrd_size as usize, QEMU_KBD.len());
        assert_eq!(&rd.ucrd_data[..QEMU_KBD.len()], QEMU_KBD);

        // A report type that does not exist.
        let mut data = std::vec![0u8; size_of::<UsbCtlReport>()];
        ioctl_ret(&mut data, &{
            let mut re = ioctl_arg::<UsbCtlReport>(&[]);
            re.ucr_report = 9;
            re
        });
        assert_eq!(
            uhidev_ioctl(scd, USB_GET_REPORT, &mut data, 0, Some(p)),
            Err(Errno::EINVAL)
        );
        assert_eq!(
            uhidev_ioctl(scd, USB_SET_REPORT, &mut data, 0, Some(p)),
            Err(Errno::EINVAL)
        );

        // Not a HID ioctl: the caller goes on (the C's -1).
        assert_eq!(uhidev_ioctl(scd, 0x1234, &mut [], 0, Some(p)), Ok(false));
    }

    #[test]
    fn a_child_gets_a_report_id_only_while_open() {
        let sc: &'static UhidevSoftc = zeroed();
        let subs: &'static mut [Cell<Option<NonNull<Uhidev>>>] = Box::leak(
            (0..3)
                .map(|_| Cell::new(None))
                .collect::<Vec<_>>()
                .into_boxed_slice(),
        );
        sc.sc_subdevs.set(subs.as_mut_ptr());
        sc.sc_nrepid.set(3);
        let dev: &'static Uhidev = zeroed();

        assert_eq!(uhidev_set_report_dev(sc, dev, 1), Err(Errno::ENODEV));
        dev.sc_state.set(UHIDEV_OPEN);
        assert_eq!(uhidev_set_report_dev(sc, dev, 3), Err(Errno::EINVAL));
        assert_eq!(uhidev_set_report_dev(sc, dev, 2), Ok(()));
        assert_eq!(sc.subdevs()[2].get(), Some(NonNull::from(dev)));
        assert_eq!(sc.subdevs()[1].get(), None);

        // `uhidev_attach_repid` leaves a slot assigned this way alone.
        let uaa: &'static UsbAttachArg =
        // SAFETY: as in the test above.
        Box::leak(Box::new(unsafe { MaybeUninit::<UsbAttachArg>::zeroed().assume_init() }));
        let mut uha = UhidevAttachArg {
            uaa,
            parent: sc,
            reportid: 0,
            nreports: 3,
            claimed: core::ptr::null_mut(),
        };
        assert!(!uhidev_attach_repid(sc, &mut uha, 2));
    }
}
/* </TESTS> */
