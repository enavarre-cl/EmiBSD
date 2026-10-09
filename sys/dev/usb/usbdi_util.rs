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
//! Helpers for USB drivers: `<dev/usb/usbdi_util.h>` and `dev/usb/usbdi_util.c`, the
//! standard and hub/HID class requests as synchronous control transfers.
//!
//! Upstream: sys/dev/usb/usbdi_util.h @ 3ce1f3f79392, sys/dev/usb/usbdi_util.c @ 3ce1f3f79392
//!
//! ## Deviations
//! - The header and the file share this module. The header also declares
//!   `usbd_get_string_desc`, `usbd_delay_ms`, `usbd_set_config_no` and
//!   `usbd_set_config_index`, which `usb_subr.c` defines; they are re-exported from here.
//! - The `void *` buffers are byte slices (a structure's `as_bytes_mut()`, see
//!   [`UsbWire`]), and the typed requests take the structure they fill by `&mut`.
//! - `usbd_get_hid_descriptor` finds `id` in the device's configuration descriptor by
//!   address (`None` if it is not in it) and stops at a zero `bLength`, where the C would loop
//!   forever.
//! - `USB_DEBUG` is not in GENERIC: the `DPRINTF`s are absent.

use core::ptr;

use crate::dev::usb::usb::{
    UDESC_HUB, UDESC_INTERFACE, UDESC_SS_HUB, UR_CLEAR_FEATURE, UR_GET_CONFIG, UR_GET_DESCRIPTOR,
    UR_GET_STATUS, UR_SET_DEPTH, UR_SET_FEATURE, USB_HUB_DESCRIPTOR_SIZE,
    USB_HUB_SS_DESCRIPTOR_SIZE, UT_READ_CLASS_DEVICE, UT_READ_CLASS_OTHER, UT_READ_DEVICE,
    UT_READ_INTERFACE, UT_WRITE_CLASS_DEVICE, UT_WRITE_CLASS_INTERFACE, UT_WRITE_CLASS_OTHER,
    UT_WRITE_ENDPOINT, UsbDeviceRequest, UsbHubDescriptor, UsbHubSsDescriptor,
    UsbInterfaceDescriptor, UsbPortStatus, UsbStatus, UsbWire, ugetw, usb_wire_at, usetw, usetw2,
};
pub use crate::dev::usb::usb_subr::{
    usbd_delay_ms, usbd_get_string_desc, usbd_set_config_index, usbd_set_config_no,
};
use crate::dev::usb::usbdi::{UsbdStatus, usbd_do_request};
use crate::dev::usb::usbdivar::UsbdDevice;
use crate::dev::usb::usbhid::{UDESC_HID, UDESC_REPORT, UR_SET_IDLE, UsbHidDescriptor};
use crate::kern::kern_synch::{tsleep_nsec, wakeup};
use crate::kern::subr_prf::printf;
use crate::sys::device::Device;
use crate::sys::param::PZERO;
use crate::sys::time::sec_to_nsec;

/// A request with `bmRequestType`, `bRequest`, `wValue`, `wIndex` and `wLength` filled.
fn request(rtype: u8, req: u8, value: u16, index: u16, len: u16) -> UsbDeviceRequest {
    let mut r = UsbDeviceRequest {
        bmRequestType: rtype,
        bRequest: req,
        ..UsbDeviceRequest::default()
    };
    usetw(&mut r.wValue, value);
    usetw(&mut r.wIndex, index);
    usetw(&mut r.wLength, len);
    r
}

/// `usbd_get_desc`: `GET_DESCRIPTOR(type, index)` of `len` bytes into `desc`.
pub fn usbd_get_desc(
    dev: &'static UsbdDevice,
    type_: i32,
    index: i32,
    len: i32,
    desc: &mut [u8],
) -> UsbdStatus {
    let mut req = request(UT_READ_DEVICE, UR_GET_DESCRIPTOR, 0, 0, len as u16);
    usetw2(&mut req.wValue, type_ as u8, index as u8);
    usbd_do_request(dev, &req, desc)
}

/// `usbd_get_device_status`.
pub fn usbd_get_device_status(dev: &'static UsbdDevice, st: &mut UsbStatus) -> UsbdStatus {
    let req = request(
        UT_READ_DEVICE,
        UR_GET_STATUS,
        0,
        0,
        size_of::<UsbStatus>() as u16,
    );
    usbd_do_request(dev, &req, st.as_bytes_mut())
}

/// `usbd_get_hub_descriptor`: the hub descriptor for a hub of `nports` ports.
pub fn usbd_get_hub_descriptor(
    dev: &'static UsbdDevice,
    hd: &mut UsbHubDescriptor,
    nports: u8,
) -> UsbdStatus {
    let len = (USB_HUB_DESCRIPTOR_SIZE + (usize::from(nports) + 1) / 8) as u16;
    let mut req = request(UT_READ_CLASS_DEVICE, UR_GET_DESCRIPTOR, 0, 0, len);
    usetw2(&mut req.wValue, UDESC_HUB, 0);
    usbd_do_request(dev, &req, hd.as_bytes_mut())
}

/// `usbd_get_hub_ss_descriptor`: the super speed hub descriptor for `nports` ports.
pub fn usbd_get_hub_ss_descriptor(
    dev: &'static UsbdDevice,
    hd: &mut UsbHubSsDescriptor,
    nports: u8,
) -> UsbdStatus {
    let len = (USB_HUB_SS_DESCRIPTOR_SIZE + (usize::from(nports) + 1) / 8) as u16;
    let mut req = request(UT_READ_CLASS_DEVICE, UR_GET_DESCRIPTOR, 0, 0, len);
    usetw2(&mut req.wValue, UDESC_SS_HUB, 0);
    usbd_do_request(dev, &req, hd.as_bytes_mut())
}

/// `usbd_get_port_status`.
pub fn usbd_get_port_status(
    dev: &'static UsbdDevice,
    port: i32,
    ps: &mut UsbPortStatus,
) -> UsbdStatus {
    let req = request(
        UT_READ_CLASS_OTHER,
        UR_GET_STATUS,
        0,
        port as u16,
        size_of::<UsbPortStatus>() as u16,
    );
    usbd_do_request(dev, &req, ps.as_bytes_mut())
}

/// `usbd_set_hub_depth`.
pub fn usbd_set_hub_depth(dev: &'static UsbdDevice, depth: i32) -> UsbdStatus {
    let req = request(UT_WRITE_CLASS_DEVICE, UR_SET_DEPTH, depth as u16, 0, 0);
    usbd_do_request(dev, &req, &mut [])
}

/// `usbd_clear_port_feature`.
pub fn usbd_clear_port_feature(dev: &'static UsbdDevice, port: i32, sel: i32) -> UsbdStatus {
    let req = request(
        UT_WRITE_CLASS_OTHER,
        UR_CLEAR_FEATURE,
        sel as u16,
        port as u16,
        0,
    );
    usbd_do_request(dev, &req, &mut [])
}

/// `usbd_clear_endpoint_feature`.
pub fn usbd_clear_endpoint_feature(dev: &'static UsbdDevice, epaddr: i32, sel: i32) -> UsbdStatus {
    let req = request(
        UT_WRITE_ENDPOINT,
        UR_CLEAR_FEATURE,
        sel as u16,
        epaddr as u16,
        0,
    );
    usbd_do_request(dev, &req, &mut [])
}

/// `usbd_set_port_feature`.
pub fn usbd_set_port_feature(dev: &'static UsbdDevice, port: i32, sel: i32) -> UsbdStatus {
    let req = request(
        UT_WRITE_CLASS_OTHER,
        UR_SET_FEATURE,
        sel as u16,
        port as u16,
        0,
    );
    usbd_do_request(dev, &req, &mut [])
}

/// `usbd_set_idle`.
pub fn usbd_set_idle(dev: &'static UsbdDevice, ifaceno: i32, duration: i32, id: i32) -> UsbdStatus {
    let mut req = request(UT_WRITE_CLASS_INTERFACE, UR_SET_IDLE, 0, ifaceno as u16, 0);
    usetw2(&mut req.wValue, duration as u8, id as u8);
    usbd_do_request(dev, &req, &mut [])
}

/// `usbd_get_report_descriptor`: `len` bytes of the HID report descriptor into `data`.
pub fn usbd_get_report_descriptor(
    dev: &'static UsbdDevice,
    ifaceno: i32,
    data: &mut [u8],
    len: i32,
) -> UsbdStatus {
    let mut req = request(
        UT_READ_INTERFACE,
        UR_GET_DESCRIPTOR,
        0,
        ifaceno as u16,
        len as u16,
    );
    usetw2(&mut req.wValue, UDESC_REPORT, 0); // report id should be 0
    usbd_do_request(dev, &req, data)
}

/// `usbd_get_hid_descriptor`: the HID descriptor that follows the interface descriptor `id`
/// in `dev`'s configuration descriptor.
pub fn usbd_get_hid_descriptor(
    dev: &UsbdDevice,
    id: &UsbInterfaceDescriptor,
) -> Option<&'static UsbHidDescriptor> {
    let cdesc = dev.cdesc.get()?;
    let cd = dev.cdesc()?;
    let end = usize::from(ugetw(cd.wTotalLength)).min(cdesc.len());
    // `(char *)id - (char *)cdesc`: `id` must lie inside the descriptor.
    let base = cdesc.as_ptr() as usize;
    let at = ptr::from_ref(id) as usize;
    if at < base || at >= base + end {
        return None;
    }
    let mut p = at - base + usize::from(id.bLength);

    while p < end {
        let blen = usize::from(cdesc[p]);
        let btype = cdesc.get(p + 1).copied();
        if p + blen <= end && btype == Some(UDESC_HID) {
            return usb_wire_at::<UsbHidDescriptor>(&cdesc[..end], p);
        }
        if btype == Some(UDESC_INTERFACE) {
            break;
        }
        if blen == 0 {
            break;
        }
        p += blen;
    }
    None
}

/// `usbd_get_config`: the device's current configuration value.
pub fn usbd_get_config(dev: &'static UsbdDevice, conf: &mut u8) -> UsbdStatus {
    let req = request(UT_READ_DEVICE, UR_GET_CONFIG, 0, 0, 1);
    usbd_do_request(dev, &req, core::slice::from_mut(conf))
}

/// `usb_detach_wait`: waits (up to a minute) for `usb_detach_wakeup` on `dv`.
pub fn usb_detach_wait(dv: &Device) {
    if tsleep_nsec(ptr::from_ref(dv), PZERO, "usbdet", sec_to_nsec(60)).is_err() {
        printf(format_args!(
            "usb_detach_wait: {} didn't detach\n",
            dv.xname()
        ));
    }
}

/// `usb_detach_wakeup`.
pub fn usb_detach_wakeup(dv: &Device) {
    wakeup(ptr::from_ref(dv));
}
/* </CODE> */
