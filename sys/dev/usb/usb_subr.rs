/*	$OpenBSD: usb_subr.c,v 1.168 2026/09/08 00:24:30 deraadt Exp $ */
/*	$NetBSD: usb_subr.c,v 1.103 2003/01/10 11:19:13 augustss Exp $	*/
/*	$FreeBSD: src/sys/dev/usb/usb_subr.c,v 1.18 1999/11/17 22:33:47 n_hibma Exp $	*/
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

//! The USB stack's device management: `dev/usb/usb_subr.c` (enumeration of a new device,
//! configuration, interface and endpoint setup, driver attachment and detachment, string
//! descriptors, the error strings).
//!
//! Upstream: sys/dev/usb/usb_subr.c @ 3ce1f3f79392
//!
//! A hub driver that sees a new device on a port calls [`usbd_new_device`]: it makes the
//! `usbd_device`, opens pipe 0, reads the device descriptor, sets the address, caches the
//! strings and calls [`usbd_probe_and_attach`], which offers the device to the drivers
//! (`config_found` with a [`UsbAttachArg`]) as a whole, then interface by interface in each
//! configuration, then to a generic driver. [`usbd_detach`] undoes it when the device goes.
//!
//! ## Deviations
//! - `option USBVERBOSE` is in GENERIC, but `usbdevs_data.h` (the vendor and product name
//!   tables) is not ported: `usbd_cache_devinfo` names a device without strings as a kernel
//!   without the option does (`vendor 0x%04x`, `product 0x%04x`).
//! - `USB_DEBUG` is not in GENERIC: the `DPRINTF`s and the `USB_DEBUG`-only power-source
//!   check of `usbd_set_config_index` are absent, and `usbd_probe_and_attach` prints its
//!   non-debug message, as without the option.
//! - `usbd_errstr` returns the C's numeric text only for `USBD_ERROR_MAX` (the one value of
//!   the enum past the table, `"19"`), without the C's `static char buffer[5]`.
//! - `usbd_get_string` returns `bool` (the C returns `buf` or NULL) and never writes past
//!   `buf`; `usbd_cache_devinfo` returns `Result`.
//! - The configuration descriptor is not changed once published: `usbd_set_config_index`
//!   corrects the `wMaxPacketSize` of a high speed device's control and bulk endpoints for
//!   every interface and alternate setting before it stores the descriptor, where the C's
//!   `usbd_parse_idesc` corrects those of an alternate setting when it is selected. Drivers
//!   then hold plain references into it (`usbdivar.rs`); the values the stack uses are the
//!   C's.
//! - `usbd_find_idesc`, `usbd_find_edesc` take the configuration descriptor's bytes and
//!   return references into them; a descriptor shorter than its structure is skipped where
//!   the C would read past it.
//! - `usbd_parse_idesc` returns `bool` (`true` for the C's 0); `usbd_get_routestring` returns
//!   `Option<u32>` (`None` for the C's -1), `usbd_get_location` `Option<(bus, route,
//!   ifaceno)>` and `usbd_getnewaddr` `Option<usize>`; `usbd_reset_port` returns `Result`
//!   (`EIO`, `ETIMEDOUT`), and `usbd_set_address` (a `dev_setaddr`) `Err(EPERM)` for the C's
//!   1, as the method type is `Result<(), Errno>` (xHCI's returns an errno).
//! - Where the C dereferences a NULL that cannot occur on a working bus (the root hub's
//!   missing parent hub in `usbd_new_device`'s retry resets, a device without a power source
//!   in the power check), the step is skipped or the budget is 0 mA.
//! - The configuration descriptor is freed with the size it was allocated with (the C passes
//!   the header's `wTotalLength`, which the full descriptor may have changed).
//! - `usbd_detach` returns the first `config_detach` error (the C ORs the errnos together);
//!   the device is freed only when every child detached, as in C.
//! - `usbd_print`'s `devinfop` buffer and `usbd_devinfo` write into byte slices; a
//!   `M_WAITOK` allocation that fails (it cannot) prints nothing.
//! - The static `cookie` of `usbd_probe_and_attach` is an atomic counter turned into the
//!   pointer the C hands out.

use core::ffi::c_void;
use core::ptr::{self, NonNull};
use core::sync::atomic::{AtomicI32, AtomicUsize, Ordering};

use crate::dev::usb::usb::{
    UC_BUS_POWERED, UC_SELF_POWERED, UDESC_CONFIG, UDESC_DEVICE, UDESC_ENDPOINT,
    UDESC_ENDPOINT_SS_COMP, UDESC_INTERFACE, UDESC_STRING, UDS_SELF_POWERED, UE_BULK, UE_CONTROL,
    UHD_PWR_INDIVIDUAL, UHF_C_PORT_RESET, UHF_PORT_DISOWN_TO_1_1, UHF_PORT_RESET, UPS_C_PORT_RESET,
    UPS_CURRENT_CONNECT_STATUS, UR_GET_DESCRIPTOR, UR_SET_ADDRESS, UR_SET_CONFIG,
    USB_2_MAX_BULK_PACKET, USB_2_MAX_CTRL_PACKET, USB_CONFIG_DESCRIPTOR_SIZE, USB_CONTROL_ENDPOINT,
    USB_CURRENT_CONFIG_INDEX, USB_DEVICE_DESCRIPTOR_SIZE, USB_ENDPOINT_DESCRIPTOR_SIZE,
    USB_HUB_DESCRIPTOR_SIZE, USB_LANGUAGE_TABLE, USB_MAX_DEVICES, USB_MAX_DEVNAMES,
    USB_MAX_IPACKET, USB_MAX_STRING_LEN, USB_MIN_POWER, USB_PORT_RESET_DELAY,
    USB_PORT_RESET_RECOVERY, USB_SET_ADDRESS_SETTLE, USB_SPEED_FULL, USB_SPEED_HIGH, USB_SPEED_LOW,
    USB_SPEED_SUPER, USB_START_ADDR, USB_UNCONFIG_INDEX, USB_UNCONFIG_NO, USBD_SHORT_XFER_OK,
    USBPALOCK, UT_READ_CLASS_DEVICE, UT_READ_DEVICE, UT_WRITE_DEVICE, UsbConfigDescriptor,
    UsbDeviceInfo, UsbDeviceRequest, UsbEndpointDescriptor, UsbEndpointSsCompDescriptor,
    UsbHubDescriptor, UsbInterfaceDescriptor, UsbPortStatus, UsbStatus, UsbStringDescriptor,
    UsbWire, ue_get_xfertype, ugetw, usb_wire_at, usetw, usetw2,
};
use crate::dev::usb::usb_quirks::{
    UQ_BUS_POWERED, UQ_EHCI_NEEDTO_DISOWN, UQ_NO_STRINGS, UQ_POWER_CLAIM, UQ_SWAP_UNICODE,
    USBD_NO_QUIRK, usbd_find_quirk,
};
use crate::dev::usb::usbdi::{
    DEVINFOSIZE, USBD_DEFAULT_INTERVAL, USBD_DEFAULT_TIMEOUT, USBD_ERROR_MAX, USBD_INVAL,
    USBD_NO_ADDR, USBD_NO_POWER, USBD_NOMEM, USBD_NORMAL_COMPLETION, USBD_SET_ADDR_FAILED,
    USBD_SHORT_XFER, USBD_TIMEOUT, UsbAttachArg, UsbdStatus, usbd_claim_iface, usbd_close_pipe,
    usbd_deactivate, usbd_do_request, usbd_do_request_flags, usbd_get_config_descriptor,
    usbd_iface_claimed, usbd_is_dying,
};
use crate::dev::usb::usbdi_util::{
    usbd_clear_port_feature, usbd_get_desc, usbd_get_device_status, usbd_get_port_status,
    usbd_set_port_feature,
};
use crate::dev::usb::usbdivar::{
    UHUB_UNK_CONFIGURATION, UHUB_UNK_INTERFACE, USBD_NOLANG, USBREV_2_0, UsbdBus, UsbdDevice,
    UsbdEndpoint, UsbdInterface, UsbdPipe, UsbdPort,
};
use crate::kern::kern_malloc::{free, malloc, mallocarray};
use crate::kern::kern_rwlock::{rw_enter_write, rw_exit_write};
use crate::kern::kern_synch::tsleep_nsec;
use crate::kern::subr_autoconf::{config_detach, config_found};
use crate::kern::subr_prf::{Str, panic, printf, snprintf};
use crate::machine::cpu::delay;
use crate::sys::device::{Device, QUIET, UNCONF};
use crate::sys::errno::Errno;
use crate::sys::malloc::{M_NOWAIT, M_TEMP, M_USB, M_WAITOK, M_ZERO};
use crate::sys::param::PRIBIO;
use crate::sys::systm::COLD;
use crate::sys::time::msec_to_nsec;
use libkern::strlcpy;

/// `usbd_error_strs[]`.
pub const USBD_ERROR_STRS: [&str; 20] = [
    "NORMAL_COMPLETION",
    "IN_PROGRESS",
    "PENDING_REQUESTS",
    "NOT_STARTED",
    "INVAL",
    "NOMEM",
    "CANCELLED",
    "BAD_ADDRESS",
    "IN_USE",
    "NO_ADDR",
    "SET_ADDR_FAILED",
    "NO_POWER",
    "TOO_DEEP",
    "IOERROR",
    "NOT_CONFIGURED",
    "TIMEOUT",
    "SHORT_XFER",
    "STALLED",
    "INTERRUPTED",
    "XXX",
];

/// `usbd_errstr`: the name of a status.
pub fn usbd_errstr(err: UsbdStatus) -> &'static str {
    if err < USBD_ERROR_MAX {
        USBD_ERROR_STRS[err as usize]
    } else {
        // snprintf(buffer, sizeof(buffer), "%d", err): only USBD_ERROR_MAX gets here.
        "19"
    }
}

/// `usbd_get_string_desc`: string descriptor `sindex` in language `langid`; `*sizep` is the
/// length the device sent.
pub fn usbd_get_string_desc(
    dev: &'static UsbdDevice,
    sindex: i32,
    langid: i32,
    sdesc: &mut UsbStringDescriptor,
    sizep: &mut i32,
) -> UsbdStatus {
    let mut req = UsbDeviceRequest {
        bmRequestType: UT_READ_DEVICE,
        bRequest: UR_GET_DESCRIPTOR,
        ..UsbDeviceRequest::default()
    };
    usetw2(&mut req.wValue, UDESC_STRING, sindex as u8);
    usetw(&mut req.wIndex, langid as u16);
    usetw(&mut req.wLength, 2); // size and descriptor type first
    let mut actlen = 0;
    let err = usbd_do_request_flags(
        dev,
        &req,
        sdesc.as_bytes_mut(),
        USBD_SHORT_XFER_OK,
        Some(&mut actlen),
        USBD_DEFAULT_TIMEOUT,
    );
    if err.is_err() {
        return err;
    }

    if actlen < 2 {
        return USBD_SHORT_XFER;
    }

    let len = usize::from(sdesc.bLength).min(size_of::<UsbStringDescriptor>());
    usetw(&mut req.wLength, len as u16); // the whole string
    let err = usbd_do_request_flags(
        dev,
        &req,
        sdesc.as_bytes_mut(),
        USBD_SHORT_XFER_OK,
        Some(&mut actlen),
        USBD_DEFAULT_TIMEOUT,
    );
    if err.is_err() {
        return err;
    }

    // actlen != sdesc->bLength: a DPRINTFN(-1) under USB_DEBUG.

    *sizep = actlen;
    USBD_NORMAL_COMPLETION
}

/// `usbd_get_string`: string `si` of the device as NUL-terminated ASCII in `buf` (non-ASCII
/// characters become `?`); `false` where the C returns NULL.
pub fn usbd_get_string(dev: &'static UsbdDevice, si: i32, buf: &mut [u8]) -> bool {
    let quirks = dev.quirks().uq_flags;
    let swap = quirks & UQ_SWAP_UNICODE != 0;
    let mut us = UsbStringDescriptor::zeroed();
    let mut size = 0;

    if si == 0 {
        return false;
    }
    if quirks & UQ_NO_STRINGS != 0 {
        return false;
    }
    if dev.langid.get() == USBD_NOLANG {
        // Set up default language
        let err = usbd_get_string_desc(dev, USB_LANGUAGE_TABLE, 0, &mut us, &mut size);
        if err.is_err() || size < 4 {
            dev.langid.set(0); // Well, just pick English then
        } else {
            // Pick the first language as the default.
            dev.langid.set(ugetw(us.bString[0]) as i16);
        }
    }
    let err = usbd_get_string_desc(dev, si, i32::from(dev.langid.get()), &mut us, &mut size);
    if err.is_err() {
        return false;
    }
    let n = (size / 2 - 1).max(0) as usize;
    let mut i = 0;
    while i < n && i < buf.len() && i < us.bString.len() {
        let c = ugetw(us.bString[i]);
        // Convert from Unicode, handle buggy strings.
        buf[i] = if c & 0xff00 == 0 {
            c as u8
        } else if c & 0x00ff == 0 && swap {
            (c >> 8) as u8
        } else {
            b'?'
        };
        i += 1;
    }
    if !buf.is_empty() {
        let at = i.min(buf.len() - 1);
        buf[at] = 0;
    }
    true
}

/// `usbd_trim_spaces`: drops the leading and trailing spaces (and trailing newlines) of the
/// NUL-terminated string in `p`.
fn usbd_trim_spaces(p: &mut [u8]) {
    let len = p.iter().position(|&b| b == 0).unwrap_or(p.len());
    let mut q = 0;
    while q < len && p[q] == b' ' {
        // skip leading spaces
        q += 1;
    }
    let mut w = 0;
    let mut e = 0;
    while q < len {
        // copy string
        let c = p[q];
        p[w] = c;
        q += 1;
        w += 1;
        if c != b' ' && c != b'\n' {
            // remember last non-space
            e = w;
        }
    }
    if e < p.len() {
        p[e] = 0; // kill trailing spaces
    }
}

/// One `USB_MAX_STRING_LEN`-byte string buffer (`malloc(USB_MAX_STRING_LEN, M_USB,
/// M_NOWAIT)`).
fn usb_string_alloc() -> Option<&'static mut [u8]> {
    let p = malloc(USB_MAX_STRING_LEN, M_USB, M_NOWAIT)?;
    // SAFETY: a fresh allocation of `USB_MAX_STRING_LEN` bytes, initialised here, owned by
    // the device until `usb_free_device`.
    unsafe {
        p.as_ptr().write_bytes(0, USB_MAX_STRING_LEN);
        Some(core::slice::from_raw_parts_mut(
            p.as_ptr(),
            USB_MAX_STRING_LEN,
        ))
    }
}

/// `strlen` of a NUL-terminated buffer.
fn cstrlen(b: &[u8]) -> usize {
    b.iter().position(|&c| c == 0).unwrap_or(b.len())
}

/// `usbd_cache_devinfo`: reads and keeps the device's serial number, vendor and product
/// strings (`ENOMEM` if a buffer cannot be had).
pub fn usbd_cache_devinfo(dev: &'static UsbdDevice) -> Result<(), Errno> {
    let udd = dev.ddesc.get();

    let serial = usb_string_alloc().ok_or(Errno::ENOMEM)?;
    if usbd_get_string(dev, i32::from(udd.iSerialNumber), serial) {
        usbd_trim_spaces(serial);
        dev.serial.set(serial.as_mut_ptr());
    } else if let Some(p) = NonNull::new(serial.as_mut_ptr()) {
        free(p, M_USB, USB_MAX_STRING_LEN);
        dev.serial.set(ptr::null_mut());
    }

    let vendor = usb_string_alloc().ok_or(Errno::ENOMEM)?;
    dev.vendor.set(vendor.as_mut_ptr());
    if usbd_get_string(dev, i32::from(udd.iManufacturer), vendor) {
        usbd_trim_spaces(vendor);
    } else {
        vendor[0] = 0;
    }
    if cstrlen(vendor) == 0 {
        // USBVERBOSE: usb_known_vendors[] is not ported (usbdevs_data.h).
        snprintf(vendor, format_args!("vendor 0x{:04x}", ugetw(udd.idVendor)));
    }

    let product = usb_string_alloc().ok_or(Errno::ENOMEM)?;
    dev.product.set(product.as_mut_ptr());
    if usbd_get_string(dev, i32::from(udd.iProduct), product) {
        usbd_trim_spaces(product);
    } else {
        product[0] = 0;
    }
    if cstrlen(product) == 0 {
        // USBVERBOSE: usb_known_products[] is not ported (usbdevs_data.h).
        snprintf(
            product,
            format_args!("product 0x{:04x}", ugetw(udd.idProduct)),
        );
    }

    Ok(())
}

/// `usbd_printBCD`: `bcd` as `"%x.%02x"` into `cp`; returns the length written.
#[allow(non_snake_case)] // the C's name
pub fn usbd_printBCD(cp: &mut [u8], bcd: i32) -> usize {
    let len = cp.len();
    let l = snprintf(cp, format_args!("{:x}.{:02x}", bcd >> 8, bcd & 0xff));
    if len == 0 {
        return 0;
    }
    if l >= len {
        return len - 1;
    }
    l
}

/// Appends formatted text at the end of the NUL-terminated string in `base` (`snprintf(cp,
/// base + len - cp, ...); cp += strlen(cp)`).
fn append(base: &mut [u8], args: core::fmt::Arguments<'_>) {
    let at = cstrlen(base);
    if at < base.len() {
        snprintf(&mut base[at..], args);
    }
}

/// `usbd_devinfo`: the device's description for attach lines (`"vendor product", class
/// c/s rev u/d addr a`).
pub fn usbd_devinfo(dev: &UsbdDevice, showclass: bool, base: &mut [u8]) {
    let udd = dev.ddesc.get();
    let vendor = UsbdDevice::string(dev.vendor.get()).unwrap_or(b"");
    let product = UsbdDevice::string(dev.product.get()).unwrap_or(b"");

    if base.is_empty() {
        return;
    }
    base[0] = 0;
    append(base, format_args!("\"{} {}\"", Str(vendor), Str(product)));
    if showclass {
        append(
            base,
            format_args!(", class {}/{}", udd.bDeviceClass, udd.bDeviceSubClass),
        );
    }
    let bcd_usb = i32::from(ugetw(udd.bcdUSB));
    let bcd_device = i32::from(ugetw(udd.bcdDevice));
    append(base, format_args!(" rev "));
    let at = cstrlen(base);
    usbd_printBCD(&mut base[at..], bcd_usb);
    append(base, format_args!("/"));
    let at = cstrlen(base);
    usbd_printBCD(&mut base[at..], bcd_device);
    append(base, format_args!(" addr {}", dev.address.get()));
}

/// `usb_delay_wchan`: the channel `usb_delay_ms` sleeps on.
static USB_DELAY_WCHAN: AtomicI32 = AtomicI32::new(0);

/// `usb_delay_ms`: delay for a certain number of ms (spinning while polling or cold).
pub fn usb_delay_ms(bus: &UsbdBus, ms: u32) {
    if bus.use_polling.get() != 0 || COLD.load(Ordering::Relaxed) {
        delay((ms + 1) * 1000);
    } else {
        let _ = tsleep_nsec(
            ptr::from_ref(&USB_DELAY_WCHAN),
            PRIBIO,
            "usbdly",
            msec_to_nsec(u64::from(ms)),
        );
    }
}

/// `usbd_delay_ms`: delay given a device handle.
pub fn usbd_delay_ms(dev: &UsbdDevice, ms: u32) {
    if usbd_is_dying(dev) {
        return;
    }

    usb_delay_ms(dev.bus(), ms);
}

/// `usbd_port_disown_to_1_1`: hands the device on `port` of an EHCI root hub over to its
/// companion controller.
pub fn usbd_port_disown_to_1_1(dev: &'static UsbdDevice, port: i32) -> UsbdStatus {
    let mut ps = UsbPortStatus::default();

    let err = usbd_set_port_feature(dev, port, UHF_PORT_DISOWN_TO_1_1);
    if err.is_err() {
        return err;
    }
    let mut n = 10;
    loop {
        // Wait for device to recover from reset.
        usbd_delay_ms(dev, USB_PORT_RESET_DELAY);
        let err = usbd_get_port_status(dev, port, &mut ps);
        if err.is_err() {
            return err;
        }
        // If the device disappeared, just give up.
        if ugetw(ps.wPortStatus) & UPS_CURRENT_CONNECT_STATUS == 0 {
            return USBD_NORMAL_COMPLETION;
        }
        n -= 1;
        if ugetw(ps.wPortChange) & UPS_C_PORT_RESET != 0 || n <= 0 {
            break;
        }
    }
    if n == 0 {
        return USBD_TIMEOUT;
    }

    USBD_NORMAL_COMPLETION
}

/// `usbd_reset_port`: resets `port` of the hub `dev` and waits for the device to recover.
pub fn usbd_reset_port(dev: &'static UsbdDevice, port: i32) -> Result<(), Errno> {
    let mut ps = UsbPortStatus::default();

    if usbd_set_port_feature(dev, port, UHF_PORT_RESET).is_err() {
        return Err(Errno::EIO);
    }
    let mut n = 10;
    loop {
        // Wait for device to recover from reset.
        usbd_delay_ms(dev, USB_PORT_RESET_DELAY);
        if usbd_get_port_status(dev, port, &mut ps).is_err() {
            return Err(Errno::EIO);
        }
        // If the device disappeared, just give up.
        if ugetw(ps.wPortStatus) & UPS_CURRENT_CONNECT_STATUS == 0 {
            return Ok(());
        }
        n -= 1;
        if ugetw(ps.wPortChange) & UPS_C_PORT_RESET != 0 || n <= 0 {
            break;
        }
    }

    // Clear port reset even if a timeout occurred.
    if usbd_clear_port_feature(dev, port, UHF_C_PORT_RESET).is_err() {
        return Err(Errno::EIO);
    }

    if n == 0 {
        return Err(Errno::ETIMEDOUT);
    }

    // Wait for the device to recover from reset.
    usbd_delay_ms(dev, USB_PORT_RESET_RECOVERY);
    Ok(())
}

/// The bytes of a configuration descriptor, `wTotalLength` of them (or fewer when the
/// buffer is shorter).
fn cdesc_bytes(cd: &[u8]) -> &[u8] {
    match usb_wire_at::<UsbConfigDescriptor>(cd, 0) {
        Some(h) => &cd[..usize::from(ugetw(h.wTotalLength)).min(cd.len())],
        None => &[],
    }
}

/// `usbd_find_idesc`: the descriptor of alternate setting `altno` of the `ifaceno`th
/// interface in the configuration descriptor `cd`.
pub fn usbd_find_idesc(cd: &[u8], ifaceno: i32, altno: i32) -> Option<&UsbInterfaceDescriptor> {
    let buf = cdesc_bytes(cd);
    let end = buf.len();
    let mut p = 0usize;
    let mut curidx = -1;
    let mut lastidx = -1;
    let mut curaidx = 0;

    while p < end {
        let blen = usize::from(buf[p]);
        if blen == 0 {
            // bad descriptor
            break;
        }
        let at = p;
        p += blen;
        if p <= end
            && buf.get(at + 1) == Some(&UDESC_INTERFACE)
            && let Some(d) = usb_wire_at::<UsbInterfaceDescriptor>(buf, at)
        {
            if i32::from(d.bInterfaceNumber) != lastidx {
                lastidx = i32::from(d.bInterfaceNumber);
                curidx += 1;
                curaidx = 0;
            } else {
                curaidx += 1;
            }
            if ifaceno == curidx && altno == curaidx {
                return Some(d);
            }
        }
    }
    None
}

/// `usbd_find_edesc`: the `endptidx`th endpoint descriptor of alternate setting `altno` of
/// the `ifaceno`th interface in `cd`.
pub fn usbd_find_edesc(
    cd: &[u8],
    ifaceno: i32,
    altno: i32,
    endptidx: i32,
) -> Option<&UsbEndpointDescriptor> {
    let buf = cdesc_bytes(cd);
    let end = buf.len();

    let d = usbd_find_idesc(cd, ifaceno, altno)?;
    if endptidx >= i32::from(d.bNumEndpoints) {
        // quick exit
        return None;
    }

    let mut curidx = -1;
    let mut p = (ptr::from_ref(d) as usize - buf.as_ptr() as usize) + usize::from(d.bLength);
    while p < end {
        let blen = usize::from(buf[p]);
        if blen == 0 {
            // bad descriptor
            break;
        }
        let at = p;
        p += blen;
        let btype = buf.get(at + 1).copied();
        if p <= end && btype == Some(UDESC_INTERFACE) {
            return None;
        }
        if p <= end && btype == Some(UDESC_ENDPOINT) {
            curidx += 1;
            if curidx == endptidx {
                return usb_wire_at::<UsbEndpointDescriptor>(buf, at);
            }
        }
    }
    None
}

/// `usbd_fill_iface_data`: sets up interface `ifaceno` of `dev` with alternate setting
/// `altno`: its descriptor and endpoints.
pub fn usbd_fill_iface_data(dev: &'static UsbdDevice, ifaceno: usize, altno: i32) -> UsbdStatus {
    let Some(ifc) = dev.ifaces().get(ifaceno) else {
        return USBD_INVAL;
    };
    let Some(cdesc) = dev.cdesc.get() else {
        return USBD_INVAL;
    };

    let Some(idesc) = usbd_find_idesc(cdesc, ifaceno as i32, altno) else {
        return USBD_INVAL;
    };

    let nendpt = usize::from(idesc.bNumEndpoints);

    ifc.device.set(Some(dev));
    ifc.idesc.set(Some(idesc));
    ifc.index.set(ifaceno as i32);
    ifc.altindex.set(altno);
    ifc.endpoints.set(ptr::null_mut());
    ifc.priv_.set(ptr::null_mut());
    ifc.pipes.init();
    ifc.nendpt.set(nendpt as u8);

    if nendpt != 0 {
        let Some(e) = mallocarray(nendpt, size_of::<UsbdEndpoint>(), M_USB, M_NOWAIT | M_ZERO)
        else {
            return USBD_NOMEM;
        };
        ifc.endpoints.set(e.as_ptr().cast::<UsbdEndpoint>());
    }

    if !usbd_parse_idesc(dev, ifc) {
        if let Some(e) = NonNull::new(ifc.endpoints.get()) {
            free(e.cast::<u8>(), M_USB, nendpt * size_of::<UsbdEndpoint>());
        }
        ifc.endpoints.set(ptr::null_mut());
        return USBD_INVAL;
    }

    USBD_NORMAL_COMPLETION
}

/// The endpoint descriptors (and their super speed companions) of the interface whose
/// descriptor is at `at` in the configuration descriptor `buf`: calls `f(i, ed, essd)` for
/// each of its `bNumEndpoints`; `false` when they are not all there (the C's -1).
fn usbd_walk_endpoints(
    buf: &[u8],
    at: usize,
    mut f: impl FnMut(usize, usize, Option<usize>),
) -> bool {
    let end = buf.len();
    let Some(idesc) = usb_wire_at::<UsbInterfaceDescriptor>(buf, at) else {
        return false;
    };
    let mut p = at + usize::from(idesc.bLength);

    for i in 0..usize::from(idesc.bNumEndpoints) {
        while p < end {
            let blen = usize::from(buf[p]);
            let btype = buf.get(p + 1).copied();
            if p + blen <= end && blen != 0 && btype == Some(UDESC_ENDPOINT) {
                break;
            }
            if blen == 0 || btype == Some(UDESC_INTERFACE) {
                return false;
            }
            p += blen;
        }

        if p >= end || p + USB_ENDPOINT_DESCRIPTOR_SIZE > end {
            return false;
        }

        let blen = usize::from(buf[p]);
        let mut pp = Some(p + blen);
        if let Some(q) = pp
            && (q >= end || buf[q] == 0 || buf.get(q + 1) != Some(&UDESC_ENDPOINT_SS_COMP))
        {
            pp = None;
        }
        if let Some(q) = pp
            && q + size_of::<UsbEndpointSsCompDescriptor>() > end
        {
            pp = None;
        }

        f(i, p, pp);
        p += blen;
    }
    true
}

/// `usbd_parse_idesc`: points the interface's endpoints at their descriptors; `true` for the
/// C's 0.
pub fn usbd_parse_idesc(dev: &UsbdDevice, ifc: &UsbdInterface) -> bool {
    let Some(buf) = dev.cdesc.get() else {
        return false;
    };
    let buf = cdesc_bytes(buf);
    let idesc = ifc.idesc();
    let at = ptr::from_ref(idesc) as usize - buf.as_ptr() as usize;
    let endpoints = ifc.endpoints();

    usbd_walk_endpoints(buf, at, |i, ed, essd| {
        // The high speed packet size fix-up (`wMaxPacketSize` of control and bulk endpoints)
        // was applied by `usbd_set_config_index` before the descriptor was published.
        let ep = &endpoints[i];
        let edp = buf[ed..].as_ptr().cast::<UsbEndpointDescriptor>();
        let essdp = match essd {
            Some(q) => buf[q..].as_ptr().cast::<UsbEndpointSsCompDescriptor>(),
            None => ptr::null(),
        };
        // SAFETY: both point inside the configuration descriptor (bounds checked by the
        // walk), which stays allocated, unchanged, while the configuration is set.
        unsafe { ep.set_edesc(edp, essdp) };
        ep.refcnt.set(0);
        ep.savedtoggle.set(0);
    })
}

/// The high speed `wMaxPacketSize` correction of `usbd_parse_idesc`, applied to every
/// interface descriptor's endpoints in a configuration descriptor `buf` before it is
/// published: control endpoints must say `USB_2_MAX_CTRL_PACKET`, bulk ones
/// `USB_2_MAX_BULK_PACKET`.
fn usbd_fix_max_packet(buf: &mut [u8]) {
    let end = buf.len();
    let mut fixes = [0usize; 64];
    let mut p = 0;
    while p + 1 < end {
        let blen = usize::from(buf[p]);
        if blen == 0 {
            break;
        }
        if buf[p + 1] == UDESC_INTERFACE {
            let mut n = 0;
            usbd_walk_endpoints(buf, p, |_, ed, _| {
                if n < fixes.len() {
                    fixes[n] = ed;
                    n += 1;
                }
            });
            for &ed in &fixes[..n] {
                let Some(e) = usb_wire_at::<UsbEndpointDescriptor>(buf, ed) else {
                    continue;
                };
                let mps = match ue_get_xfertype(e.bmAttributes) {
                    UE_CONTROL => USB_2_MAX_CTRL_PACKET,
                    UE_BULK => USB_2_MAX_BULK_PACKET,
                    _ => continue,
                };
                if ugetw(e.wMaxPacketSize) != mps {
                    // `wMaxPacketSize` is at offset 4 of the endpoint descriptor.
                    let off = ed + core::mem::offset_of!(UsbEndpointDescriptor, wMaxPacketSize);
                    let mut w = [0u8; 2];
                    usetw(&mut w, mps);
                    buf[off..off + 2].copy_from_slice(&w);
                }
            }
        }
        p += blen;
    }
}

/// `usbd_free_iface_data`.
pub fn usbd_free_iface_data(dev: &UsbdDevice, ifcno: usize) {
    let Some(ifc) = dev.ifaces().get(ifcno) else {
        return;
    };

    if let Some(e) = NonNull::new(ifc.endpoints.get()) {
        free(
            e.cast::<u8>(),
            M_USB,
            usize::from(ifc.nendpt.get()) * size_of::<UsbdEndpoint>(),
        );
    }
    ifc.endpoints.set(ptr::null_mut());
}

/// `usbd_set_config`: `SET_CONFIGURATION(conf)`.
pub fn usbd_set_config(dev: &'static UsbdDevice, conf: i32) -> UsbdStatus {
    let mut req = UsbDeviceRequest {
        bmRequestType: UT_WRITE_DEVICE,
        bRequest: UR_SET_CONFIG,
        ..UsbDeviceRequest::default()
    };
    usetw(&mut req.wValue, conf as u16);
    usetw(&mut req.wIndex, 0);
    usetw(&mut req.wLength, 0);
    usbd_do_request(dev, &req, &mut [])
}

/// `usbd_set_config_no`: selects the configuration whose value is `no`.
pub fn usbd_set_config_no(dev: &'static UsbdDevice, no: i32, msg: bool) -> UsbdStatus {
    let mut cd = UsbConfigDescriptor::zeroed();

    // Figure out what config index to use.
    for index in 0..i32::from(dev.ddesc.get().bNumConfigurations) {
        let err = usbd_get_desc(
            dev,
            i32::from(UDESC_CONFIG),
            index,
            USB_CONFIG_DESCRIPTOR_SIZE as i32,
            cd.as_bytes_mut(),
        );
        if err.is_err() || cd.bDescriptorType != UDESC_CONFIG {
            return err;
        }
        if i32::from(cd.bConfigurationValue) == no {
            return usbd_set_config_index(dev, index, msg);
        }
    }
    USBD_INVAL
}

/// Frees the device's configuration data: interfaces, endpoints and descriptor.
fn usbd_free_config(dev: &UsbdDevice) {
    let nifc = dev.cdesc().map_or(0, |cd| usize::from(cd.bNumInterfaces));
    for ifcidx in 0..nifc {
        usbd_free_iface_data(dev, ifcidx);
    }
    if let Some(p) = NonNull::new(dev.ifaces.get()) {
        free(p.cast::<u8>(), M_USB, nifc * size_of::<UsbdInterface>());
    }
    if let Some(b) = dev.cdesc.get() {
        // The size `usbd_set_config_index` allocated (the C frees the header's
        // `wTotalLength`, which the full descriptor may have changed).
        free(NonNull::from(b).cast::<u8>(), M_USB, b.len().max(1));
    }
    dev.ifaces.set(ptr::null_mut());
    dev.cdesc.set(None);
}

/// `usbd_set_config_index`: selects the `index`th configuration (`USB_UNCONFIG_INDEX`
/// unconfigures): reads its descriptor, checks the power budget, sends `SET_CONFIGURATION`
/// and sets up its interfaces at alternate setting 0.
pub fn usbd_set_config_index(dev: &'static UsbdDevice, index: i32, msg: bool) -> UsbdStatus {
    let mut cd = UsbConfigDescriptor::zeroed();

    // XXX check that all interfaces are idle
    if i32::from(dev.config.get()) != USB_UNCONFIG_NO {
        // Free all configuration data structures.
        usbd_free_config(dev);
        dev.config.set(USB_UNCONFIG_NO as u8);
    }

    if index == USB_UNCONFIG_INDEX {
        // We are unconfiguring the device, so leave unallocated.
        return usbd_set_config(dev, USB_UNCONFIG_NO);
    }

    // Get the short descriptor.
    let err = usbd_get_desc(
        dev,
        i32::from(UDESC_CONFIG),
        index,
        USB_CONFIG_DESCRIPTOR_SIZE as i32,
        cd.as_bytes_mut(),
    );
    if err.is_err() {
        return err;
    }
    if cd.bDescriptorType != UDESC_CONFIG {
        return USBD_INVAL;
    }
    let cdplen = usize::from(ugetw(cd.wTotalLength));
    let Some(cdpmem) = malloc(cdplen.max(1), M_USB, M_NOWAIT) else {
        return USBD_NOMEM;
    };
    // SAFETY: a fresh allocation of `cdplen` bytes, zeroed here, owned by this function until
    // it is published in `dev.cdesc` (or freed at `bad`).
    let cdp: &'static mut [u8] = unsafe {
        cdpmem.as_ptr().write_bytes(0, cdplen.max(1));
        core::slice::from_raw_parts_mut(cdpmem.as_ptr(), cdplen)
    };

    let err = 'bad: {
        // Get the full descriptor.
        let mut err = USBD_NORMAL_COMPLETION;
        for _ in 0..3 {
            err = usbd_get_desc(dev, i32::from(UDESC_CONFIG), index, cdplen as i32, cdp);
            if !err.is_err() {
                break;
            }
            usbd_delay_ms(dev, 200);
        }
        if err.is_err() {
            break 'bad err;
        }

        let hdr = UsbConfigDescriptor::read_from(cdp);
        if hdr.bDescriptorType != UDESC_CONFIG {
            break 'bad USBD_INVAL;
        }

        // Figure out if the device is self or bus powered.
        let quirks = dev.quirks().uq_flags;
        let mut selfpowered = 0u8;
        if quirks & UQ_BUS_POWERED == 0 && hdr.bmAttributes & UC_SELF_POWERED != 0 {
            // May be self powered.
            if hdr.bmAttributes & UC_BUS_POWERED != 0 {
                // Must ask device.
                if quirks & UQ_POWER_CLAIM != 0 {
                    // Hub claims to be self powered, but isn't. It seems that the power
                    // status can be determined by the hub characteristics.
                    let mut hd = UsbHubDescriptor::zeroed();
                    let mut req = UsbDeviceRequest {
                        bmRequestType: UT_READ_CLASS_DEVICE,
                        bRequest: UR_GET_DESCRIPTOR,
                        ..UsbDeviceRequest::default()
                    };
                    usetw(&mut req.wValue, 0);
                    usetw(&mut req.wIndex, 0);
                    usetw(&mut req.wLength, USB_HUB_DESCRIPTOR_SIZE as u16);
                    let e = usbd_do_request(dev, &req, hd.as_bytes_mut());
                    if !e.is_err() && ugetw(hd.wHubCharacteristics) & UHD_PWR_INDIVIDUAL != 0 {
                        selfpowered = 1;
                    }
                } else {
                    let mut ds = UsbStatus::zeroed();
                    let e = usbd_get_device_status(dev, &mut ds);
                    if !e.is_err() && ugetw(ds.wStatus) & UDS_SELF_POWERED != 0 {
                        selfpowered = 1;
                    }
                }
            } else {
                selfpowered = 1;
            }
        }

        // Check if we have enough power.
        // (USB_DEBUG: a missing power source is USBD_IOERROR.)
        let power = i32::from(hdr.bMaxPower) * 2;
        let srcpower = dev.powersrc.get().map_or(0, |p| i32::from(p.power.get()));
        if power > srcpower {
            // XXX print nicer message.
            if msg {
                printf(format_args!(
                    "{}: device addr {} (config {}) exceeds power budget, {} mA > {} mA\n",
                    dev.bus().bdev.xname(),
                    dev.address.get(),
                    hdr.bConfigurationValue,
                    power,
                    srcpower
                ));
            }
            break 'bad USBD_NO_POWER;
        }
        dev.power.set(power as u16);
        dev.self_powered.set(selfpowered);

        // Set the actual configuration value.
        let err = usbd_set_config(dev, i32::from(hdr.bConfigurationValue));
        if err.is_err() {
            break 'bad err;
        }

        // Allocate and fill interface data.
        let nifc = usize::from(hdr.bNumInterfaces);
        let Some(ifaces) = mallocarray(nifc, size_of::<UsbdInterface>(), M_USB, M_NOWAIT | M_ZERO)
        else {
            break 'bad USBD_NOMEM;
        };
        dev.ifaces.set(ifaces.as_ptr().cast::<UsbdInterface>());

        // The C's in-place fix of high speed endpoints (usbd_parse_idesc), done once here.
        if dev.speed.get() == USB_SPEED_HIGH {
            usbd_fix_max_packet(cdp);
        }
        let cdp: &'static [u8] = cdp;
        dev.cdesc.set(Some(cdp));
        dev.config.set(hdr.bConfigurationValue);
        for ifcidx in 0..nifc {
            let err = usbd_fill_iface_data(dev, ifcidx, 0);
            if err.is_err() {
                return err;
            }
        }

        return USBD_NORMAL_COMPLETION;
    };

    // bad:
    free(cdpmem, M_USB, cdplen.max(1));
    err
}

/* XXX add function for alternate settings */

/// `usbd_setup_pipe`: a pipe of the bus's size on endpoint `ep`, opened by the host
/// controller.
pub fn usbd_setup_pipe(
    dev: &'static UsbdDevice,
    iface: Option<&'static UsbdInterface>,
    ep: &'static UsbdEndpoint,
    ival: i32,
) -> Result<&'static UsbdPipe, UsbdStatus> {
    let bus = dev.bus();
    let size = bus.pipe_size.get() as usize;
    if size < size_of::<UsbdPipe>() {
        panic(format_args!(
            "{}: pipe_size {} too small",
            bus.bdev.xname(),
            size
        ));
    }
    let Some(mem) = malloc(size, M_USB, M_NOWAIT | M_ZERO) else {
        return Err(USBD_NOMEM);
    };
    // SAFETY: a zeroed allocation of at least `size_of::<UsbdPipe>()` bytes, aligned by
    // malloc; all-zero is a valid pipe (and a valid host controller pipe, `UsbdHcPipe`).
    let p: &'static UsbdPipe = unsafe { &*mem.as_ptr().cast::<UsbdPipe>() };
    p.pipe_size.set(size);
    p.device.set(Some(dev));
    p.iface.set(iface);
    p.endpoint.set(Some(ep));
    ep.refcnt.set(ep.refcnt.get() + 1);
    p.interval.set(ival);
    p.queue.init();
    let err = (bus.methods().open_pipe)(p);
    if err.is_err() {
        ep.refcnt.set(ep.refcnt.get() - 1);
        free(mem, M_USB, size);
        return Err(err);
    }
    Ok(p)
}

/// `usbd_set_address`: `SET_ADDRESS(addr)` (the `dev_setaddr` of controllers that let the
/// stack do it). The C returns 1 on failure, which is `EPERM`'s number; callers only test
/// for nonzero.
pub fn usbd_set_address(dev: &'static UsbdDevice, addr: i32) -> Result<(), Errno> {
    let mut req = UsbDeviceRequest {
        bmRequestType: UT_WRITE_DEVICE,
        bRequest: UR_SET_ADDRESS,
        ..UsbDeviceRequest::default()
    };
    usetw(&mut req.wValue, addr as u16);
    usetw(&mut req.wIndex, 0);
    usetw(&mut req.wLength, 0);
    if usbd_do_request(dev, &req, &mut []).is_err() {
        return Err(Errno::EPERM);
    }

    // Allow device time to set new address
    usbd_delay_ms(dev, USB_SET_ADDRESS_SETTLE);

    Ok(())
}

/// `usbd_getnewaddr`: the first free address of the bus; `None` for the C's -1 (all
/// taken).
pub fn usbd_getnewaddr(bus: &UsbdBus) -> Option<usize> {
    (1..USB_MAX_DEVICES).find(|&addr| bus.devices[addr].get().is_none())
}

/// `cookie` of `usbd_probe_and_attach`: used to correlate audio and wskbd devices as this
/// is the common point of attachment between the two.
static PROBE_COOKIE: AtomicUsize = AtomicUsize::new(0);

/// Appends `dv` to the device's `subdevs` (`dev->subdevs[dev->ndevs++] = dv`).
fn subdevs_push(dev: &UsbdDevice, dv: NonNull<Device>) {
    let i = dev.ndevs.get();
    if let Some(slot) = dev.subdevs().get(i as usize) {
        slot.set(Some(dv));
    }
    dev.ndevs.set(i + 1);
}

/// `mallocarray(n, sizeof(dv), M_USB, flags)` for `subdevs`, every slot NULL.
fn subdevs_alloc(dev: &UsbdDevice, n: usize, flags: i32) -> bool {
    let Some(p) = mallocarray(n, size_of::<Option<NonNull<Device>>>(), M_USB, flags) else {
        return false;
    };
    let p = p.cast::<Option<NonNull<Device>>>();
    for i in 0..n {
        // SAFETY: `n` slots were allocated; each is written before the array is published.
        unsafe { p.as_ptr().add(i).write(None) };
    }
    dev.subdevs.set(p.as_ptr());
    dev.nsubdev.set(n as i32);
    true
}

/// Frees `subdevs`.
fn subdevs_free(dev: &UsbdDevice) {
    if let Some(p) = NonNull::new(dev.subdevs.get()) {
        free(
            p.cast::<u8>(),
            M_USB,
            dev.nsubdev.get() as usize * size_of::<Option<NonNull<Device>>>(),
        );
    }
    dev.subdevs.set(ptr::null_mut());
    dev.nsubdev.set(0);
}

/// `usbd_probe_and_attach`: offers the device to the drivers: as a whole, then each
/// interface of each configuration, then to the generic driver.
pub fn usbd_probe_and_attach(
    parent: &Device,
    dev: &'static UsbdDevice,
    port: i32,
    addr: i32,
) -> UsbdStatus {
    let dd = dev.ddesc.get();

    rw_enter_write(&USBPALOCK);

    let cookie = PROBE_COOKIE.fetch_add(1, Ordering::Relaxed) + 1;
    let mut uaa = UsbAttachArg {
        device: Some(dev),
        iface: None,
        ifaces: ptr::null(),
        nifaces: 0,
        usegeneric: 0,
        port,
        configno: UHUB_UNK_CONFIGURATION,
        ifaceno: UHUB_UNK_INTERFACE,
        vendor: i32::from(ugetw(dd.idVendor)),
        product: i32::from(ugetw(dd.idProduct)),
        release: i32::from(ugetw(dd.bcdDevice)),
        cookie: ptr::without_provenance_mut(cookie),
    };
    let aux = |uaa: &mut UsbAttachArg| ptr::from_mut(uaa).cast::<c_void>();

    let err = 'fail: {
        // First try with device specific drivers.
        if let Some(dv) = config_found(parent, aux(&mut uaa), Some(usbd_print)) {
            if !subdevs_alloc(dev, 2, M_NOWAIT) {
                break 'fail USBD_NOMEM;
            }
            subdevs_push(dev, dv);
            break 'fail USBD_NORMAL_COMPLETION;
        }

        // Next try with interface drivers.
        let mut generic = false;
        for confi in 0..i32::from(dd.bNumConfigurations) {
            let err = usbd_set_config_index(dev, confi, true);
            if err.is_err() {
                printf(format_args!(
                    "{}: port {}, set config {} at addr {} failed\n",
                    parent.xname(),
                    port,
                    confi,
                    addr
                ));
                break 'fail err;
            }
            let Some(cd) = dev.cdesc() else {
                break 'fail USBD_INVAL;
            };
            let nifaces = usize::from(cd.bNumInterfaces);
            uaa.configno = i32::from(cd.bConfigurationValue);
            let Some(ifm) = mallocarray(
                nifaces.max(1),
                size_of::<&'static UsbdInterface>(),
                M_USB,
                M_NOWAIT,
            ) else {
                break 'fail USBD_NOMEM;
            };
            let ifaces = ifm.cast::<&'static UsbdInterface>();
            for (i, ifc) in dev.ifaces().iter().enumerate().take(nifaces) {
                // SAFETY: `nifaces` slots were allocated; each is written before use.
                unsafe { ifaces.as_ptr().add(i).write(ifc) };
            }
            uaa.ifaces = ifaces.as_ptr();
            uaa.nifaces = nifaces as i32;

            // add 1 for possible ugen and 1 for NULL terminator
            if !subdevs_alloc(dev, nifaces + 2, M_NOWAIT | M_ZERO) {
                free(
                    ifm,
                    M_USB,
                    nifaces.max(1) * size_of::<&'static UsbdInterface>(),
                );
                break 'fail USBD_NOMEM;
            }

            for i in 0..nifaces {
                if usbd_iface_claimed(dev, i) {
                    continue;
                }
                // SAFETY: slot `i` was written above.
                let ifc: &'static UsbdInterface = unsafe { *ifaces.as_ptr().add(i) };
                uaa.iface = Some(ifc);
                uaa.ifaceno = i32::from(ifc.idesc().bInterfaceNumber);
                if let Some(dv) = config_found(parent, aux(&mut uaa), Some(usbd_print)) {
                    subdevs_push(dev, dv);
                    usbd_claim_iface(dev, i);
                }
            }
            free(
                ifm,
                M_USB,
                nifaces.max(1) * size_of::<&'static UsbdInterface>(),
            );
            uaa.ifaces = ptr::null();
            uaa.nifaces = 0;

            if dev.ndevs.get() > 0 {
                if (0..nifaces).any(|i| !usbd_iface_claimed(dev, i)) {
                    generic = true;
                    break;
                }
                break 'fail USBD_NORMAL_COMPLETION;
            }

            subdevs_free(dev);
        }

        if !generic {
            // No interfaces were attached in any of the configurations.
            if dd.bNumConfigurations > 1 {
                // don't change if only 1 config
                let _ = usbd_set_config_index(dev, 0, false);
            }
        }

        // generic: Finally try the generic driver.
        uaa.iface = None;
        uaa.usegeneric = 1;
        uaa.configno = if dev.ndevs.get() == 0 {
            UHUB_UNK_CONFIGURATION
        } else {
            dev.cdesc().map_or(UHUB_UNK_CONFIGURATION, |cd| {
                i32::from(cd.bConfigurationValue)
            })
        };
        uaa.ifaceno = UHUB_UNK_INTERFACE;
        if let Some(dv) = config_found(parent, aux(&mut uaa), Some(usbd_print)) {
            if dev.ndevs.get() == 0 && !subdevs_alloc(dev, 2, M_NOWAIT) {
                break 'fail USBD_NOMEM;
            }
            subdevs_push(dev, dv);
            break 'fail USBD_NORMAL_COMPLETION;
        }

        // The generic attach failed, but leave the device as it is. We just did not find any
        // drivers, that's all. The device is fully operational and not harming anyone.
        USBD_NORMAL_COMPLETION
    };

    rw_exit_write(&USBPALOCK);
    err
}

/// `usbd_new_device`: called when a new device has been put in the powered state, but not
/// yet in the addressed state. Get initial descriptor, set the address, get full
/// descriptor, and attach a driver.
pub fn usbd_new_device(
    parent: &Device,
    bus: &'static UsbdBus,
    depth: i32,
    speed: i32,
    port: i32,
    up: &'static UsbdPort,
) -> UsbdStatus {
    // Fixed size for ep0 max packet, FULL device variable size is handled below.
    let speed = speed as u8;
    let mps0: u32 = match speed {
        USB_SPEED_LOW => 8,
        USB_SPEED_HIGH | USB_SPEED_FULL => 64,
        USB_SPEED_SUPER => 512,
        _ => return USBD_INVAL,
    };

    let Some(addr) = usbd_getnewaddr(bus) else {
        printf(format_args!(
            "{}: No free USB addresses, new device ignored.\n",
            bus.bdev.xname()
        ));
        return USBD_NO_ADDR;
    };

    let Some(mem) = malloc(size_of::<UsbdDevice>(), M_USB, M_NOWAIT | M_ZERO) else {
        return USBD_NOMEM;
    };
    // SAFETY: a zeroed allocation of a device (all-zero is a valid `UsbdDevice`, see
    // `usbdivar.rs`), freed only by `usb_free_device`.
    let dev: &'static UsbdDevice = unsafe { &*mem.as_ptr().cast::<UsbdDevice>() };

    dev.bus.set(Some(bus));

    // Set up default endpoint handle.
    // SAFETY: `def_ep_desc` is a member of the device, which outlives its endpoint.
    unsafe { dev.def_ep.set_edesc(dev.def_ep_desc.as_ptr(), ptr::null()) };

    // Set up default endpoint descriptor.
    let mut ed = UsbEndpointDescriptor {
        bLength: USB_ENDPOINT_DESCRIPTOR_SIZE as u8,
        bDescriptorType: UDESC_ENDPOINT,
        bEndpointAddress: USB_CONTROL_ENDPOINT,
        bmAttributes: UE_CONTROL,
        bInterval: 0,
        ..UsbEndpointDescriptor::default()
    };
    usetw(&mut ed.wMaxPacketSize, mps0 as u16);
    dev.def_ep_desc.set(ed);

    dev.quirks.set(Some(&USBD_NO_QUIRK));
    dev.address.set(USB_START_ADDR);
    let mut dd = dev.ddesc.get();
    dd.bMaxPacketSize = 0;
    dev.ddesc.set(dd);
    dev.depth.set(depth as u8);
    dev.powersrc.set(Some(up));
    dev.myhub.set(up.parent.get());
    dev.speed.set(speed);
    dev.langid.set(USBD_NOLANG);

    up.device.set(Some(dev));

    // Locate port on upstream high speed hub
    let mut adev = dev;
    let mut hub = up.parent.get();
    while let Some(h) = hub {
        if h.speed.get() == USB_SPEED_HIGH {
            break;
        }
        adev = h;
        hub = h.myhub.get();
    }
    match hub {
        Some(h) => {
            let ports = h.hub.get().map_or(&[][..], |hh| hh.ports());
            match ports
                .iter()
                .find(|p| p.device.get().is_some_and(|d| ptr::eq(d, adev)))
            {
                Some(p) => dev.myhsport.set(Some(p)),
                None => panic(format_args!("usbd_new_device: cannot find HS port")),
            }
        }
        None => dev.myhsport.set(None),
    }

    let err = 'fail: {
        // Establish the default pipe.
        match usbd_setup_pipe(dev, None, &dev.def_ep, USBD_DEFAULT_INTERVAL) {
            Ok(p) => dev.default_pipe.set(Some(p)),
            Err(e) => break 'fail e,
        }

        // Try to get device descriptor. Some device will need small size query at first
        // (XXX: out of spec); we will get full size descriptor later, just determine the
        // maximum packet size of the control pipe at this moment.
        let mut dd = dev.ddesc.get();
        let mut err = USBD_NORMAL_COMPLETION;
        for i in 0..3 {
            // Get the first 8 bytes of the device descriptor. 8 byte is magic size, some
            // device only return 8 byte for 1st query (XXX: out of spec)
            err = usbd_get_desc(
                dev,
                i32::from(UDESC_DEVICE),
                0,
                USB_MAX_IPACKET as i32,
                dd.as_bytes_mut(),
            );
            dev.ddesc.set(dd);
            if !err.is_err() {
                break;
            }
            if err == USBD_TIMEOUT {
                break 'fail err;
            }
            usbd_delay_ms(dev, 100 + 50 * i);
        }

        // some device need actual size request for the query. try again
        if err.is_err() {
            let mut ed = dev.def_ep_desc.get();
            usetw(&mut ed.wMaxPacketSize, USB_DEVICE_DESCRIPTOR_SIZE as u16);
            dev.def_ep_desc.set(ed);
            if let Some(h) = up.parent.get() {
                let _ = usbd_reset_port(h, port);
            }
            for i in 0..3 {
                err = usbd_get_desc(
                    dev,
                    i32::from(UDESC_DEVICE),
                    0,
                    USB_DEVICE_DESCRIPTOR_SIZE as i32,
                    dd.as_bytes_mut(),
                );
                dev.ddesc.set(dd);
                if !err.is_err() {
                    break;
                }
                if err == USBD_TIMEOUT {
                    break 'fail err;
                }
                usbd_delay_ms(dev, 100 + 50 * i);
            }
        }

        // XXX some devices need more time to wake up
        if err.is_err() {
            let mut ed = dev.def_ep_desc.get();
            usetw(&mut ed.wMaxPacketSize, USB_MAX_IPACKET as u16);
            dev.def_ep_desc.set(ed);
            if let Some(h) = up.parent.get() {
                let _ = usbd_reset_port(h, port);
            }
            usbd_delay_ms(dev, 500);
            err = usbd_get_desc(
                dev,
                i32::from(UDESC_DEVICE),
                0,
                USB_MAX_IPACKET as i32,
                dd.as_bytes_mut(),
            );
            dev.ddesc.set(dd);
        }

        if err.is_err() {
            break 'fail err;
        }

        if dd.bDescriptorType != UDESC_DEVICE
            || usize::from(dd.bLength) < USB_DEVICE_DESCRIPTOR_SIZE
        {
            break 'fail USBD_INVAL;
        }

        let mut mps = u32::from(dd.bMaxPacketSize);
        if speed == USB_SPEED_SUPER {
            if mps == 0xff {
                mps = 9;
            }
            // xHCI Section 4.8.2.1
            mps = 1u32.checked_shl(mps).unwrap_or(0);
        }

        if mps != mps0 {
            if speed == USB_SPEED_LOW || (mps != 8 && mps != 16 && mps != 32 && mps != 64) {
                break 'fail USBD_INVAL;
            }
            let mut ed = dev.def_ep_desc.get();
            usetw(&mut ed.wMaxPacketSize, mps as u16);
            dev.def_ep_desc.set(ed);
        }

        // Set the address if the HC didn't do it already.
        if let Some(setaddr) = bus.methods().dev_setaddr
            && setaddr(dev, addr as i32).is_err()
        {
            break 'fail USBD_SET_ADDR_FAILED;
        }

        // Wait for device to settle before reloading the descriptor.
        usbd_delay_ms(dev, 10);

        // If this device is attached to an xHCI controller, this address does not
        // correspond to the hardware one.
        dev.address.set(addr as u8);

        let err = usbd_reload_device_desc(dev);
        if err.is_err() {
            break 'fail err;
        }

        // send disown request to handover 2.0 to 1.1.
        if dev.quirks().uq_flags & UQ_EHCI_NEEDTO_DISOWN != 0 {
            // only effective when the target device is on ehci
            if dev.bus().usbrev.get() == USBREV_2_0 {
                let Some(myhub) = dev.myhub.get() else {
                    panic(format_args!("usbd_new_device: disown without a hub"));
                };
                let _ = usbd_port_disown_to_1_1(myhub, port);
                // reset_port required to finish disown request
                let _ = usbd_reset_port(myhub, port);
                return USBD_NORMAL_COMPLETION;
            }
        }

        // Assume 100mA bus powered for now. Changed when configured.
        dev.power.set(USB_MIN_POWER);
        dev.self_powered.set(0);

        // Get device info and cache it
        if usbd_cache_devinfo(dev).is_err() {
            break 'fail USBD_NOMEM;
        }

        bus.devices[addr].set(Some(dev));

        let err = usbd_probe_and_attach(parent, dev, port, addr as i32);
        if err.is_err() {
            break 'fail err;
        }

        return USBD_NORMAL_COMPLETION;
    };

    // fail:
    // SAFETY: the device made above; nothing kept it (a failed attach leaves no child).
    unsafe { usb_free_device(NonNull::from(dev)) };
    up.device.set(None);
    err
}

/// `usbd_reload_device_desc`: reads the full device descriptor and looks its quirks up.
pub fn usbd_reload_device_desc(dev: &'static UsbdDevice) -> UsbdStatus {
    // Get the full device descriptor.
    let mut dd = dev.ddesc.get();
    let err = usbd_get_desc(
        dev,
        i32::from(UDESC_DEVICE),
        0,
        USB_DEVICE_DESCRIPTOR_SIZE as i32,
        dd.as_bytes_mut(),
    );
    dev.ddesc.set(dd);
    if err.is_err() {
        return err;
    }

    // Figure out what's wrong with this device.
    dev.quirks.set(Some(usbd_find_quirk(&dd)));

    USBD_NORMAL_COMPLETION
}

/// `usbd_print`: the `cfprint_t` of `usbd_probe_and_attach`.
pub fn usbd_print(aux: *mut c_void, pnp: Option<&[u8]>) -> i32 {
    // SAFETY: `usbd_probe_and_attach` passes a `UsbAttachArg` that lives across the
    // `config_found` calling this.
    let uaa = unsafe { &*aux.cast::<UsbAttachArg>() };

    let Some(mem) = malloc(DEVINFOSIZE, M_TEMP, M_WAITOK) else {
        return UNCONF;
    };
    // SAFETY: a fresh `DEVINFOSIZE`-byte allocation, ours until the `free` below.
    let devinfop = unsafe {
        mem.as_ptr().write_bytes(0, DEVINFOSIZE);
        core::slice::from_raw_parts_mut(mem.as_ptr(), DEVINFOSIZE)
    };
    usbd_devinfo(uaa.device(), false, devinfop);

    if let Some(pnp) = pnp {
        if uaa.usegeneric == 0 {
            free(mem, M_TEMP, DEVINFOSIZE);
            return QUIET;
        }
        printf(format_args!("{} at {}", Str(devinfop), Str(pnp)));
    }
    if uaa.port != 0 {
        printf(format_args!(" port {}", uaa.port));
    }
    if uaa.configno != UHUB_UNK_CONFIGURATION {
        printf(format_args!(" configuration {}", uaa.configno));
    }
    if uaa.ifaceno != UHUB_UNK_INTERFACE {
        printf(format_args!(" interface {}", uaa.ifaceno));
    }

    if pnp.is_none() {
        printf(format_args!(" {}\n", Str(devinfop)));
    }
    free(mem, M_TEMP, DEVINFOSIZE);
    UNCONF
}

/// `usbd_fill_deviceinfo`: the `USB_DEVICEINFO` answer for `dev`.
pub fn usbd_fill_deviceinfo(dev: &UsbdDevice, di: &mut UsbDeviceInfo) {
    let dd = dev.ddesc.get();

    // memset(di, 0, sizeof(*di))
    // SAFETY: `UsbDeviceInfo` is made of integers and arrays of them: all-zero is a value.
    *di = unsafe { core::mem::zeroed() };
    di.udi_bus = dev
        .bus()
        .usbctl
        .get()
        // SAFETY: `usbctl` is the attached `usb(4)` device of the bus, alive while the bus is.
        .map_or(0, |d| unsafe { d.as_ref() }.dv_unit.get() as u8);
    di.udi_addr = dev.address.get();
    strlcpy(
        &mut di.udi_vendor,
        UsbdDevice::string(dev.vendor.get()).unwrap_or(b""),
    );
    strlcpy(
        &mut di.udi_product,
        UsbdDevice::string(dev.product.get()).unwrap_or(b""),
    );
    usbd_printBCD(&mut di.udi_release, i32::from(ugetw(dd.bcdDevice)));
    di.udi_vendorNo = ugetw(dd.idVendor);
    di.udi_productNo = ugetw(dd.idProduct);
    di.udi_releaseNo = ugetw(dd.bcdDevice);
    di.udi_class = dd.bDeviceClass;
    di.udi_subclass = dd.bDeviceSubClass;
    di.udi_protocol = dd.bDeviceProtocol;
    di.udi_config = dev.config.get();
    di.udi_power = if dev.self_powered.get() != 0 {
        0
    } else {
        i32::from(dev.power.get())
    };
    di.udi_speed = dev.speed.get();
    di.udi_port = dev.powersrc.get().map_or(0, |p| p.portno.get());

    for (i, slot) in dev.subdevs().iter().enumerate().take(USB_MAX_DEVNAMES) {
        let Some(sd) = slot.get() else {
            break;
        };
        // SAFETY: an attached child of the device, alive until `usbd_detach`.
        let name = unsafe { sd.as_ref() }.xname();
        strlcpy(&mut di.udi_devnames[i], name.as_bytes());
    }

    if let Some(hub) = dev.hub.get() {
        let ports = hub.ports();
        for (i, p) in ports.iter().enumerate().take(di.udi_ports.len()) {
            let st = p.status.get();
            di.udi_ports[i] =
                u32::from(ugetw(st.wPortChange)) << 16 | u32::from(ugetw(st.wPortStatus));
        }
        di.udi_nports = hub.nports.get();
    }

    if let Some(serial) = UsbdDevice::string(dev.serial.get()) {
        strlcpy(&mut di.udi_serial, serial);
    }
}

/// `usbd_get_routestring`: the device's route string (USB 3.1, 8.9), `None` for the C's -1
/// (a hub with more than 15 ports on the way).
pub fn usbd_get_routestring(dev: &UsbdDevice) -> Option<u32> {
    // Calculate the Route String. Assume that there is no hub with more than 15 ports and
    // that they all have a depth < 6. See section 8.9 of USB 3.1 Specification for more
    // details.
    let mut r: u32 = dev.powersrc.get().map_or(0, |p| u32::from(p.portno.get()));
    let mut hub = dev.myhub.get();
    while let Some(h) = hub {
        if h.depth.get() <= 1 {
            break;
        }
        let port = h.powersrc.get().map_or(0, |p| p.portno.get());
        if port > 15 {
            return None;
        }
        r <<= 4;
        r |= u32::from(port);
        hub = h.myhub.get();
    }

    // Add in the host root port, of which there may be 255.
    let port = hub
        .and_then(|h| h.powersrc.get())
        .map_or(0, |p| p.portno.get());
    r <<= 8;
    r |= u32::from(port);

    Some(r)
}

/// `usbd_get_location`: `(bus, route, ifaceno)` of an interface of the device, `None` for
/// the C's -1.
pub fn usbd_get_location(dev: Option<&UsbdDevice>, iface: &UsbdInterface) -> Option<(u8, u32, u8)> {
    let dev = dev?;
    if usbd_is_dying(dev) {
        return None;
    }
    let cd = dev.cdesc()?;
    if cd.bNumInterfaces == 0 {
        return None;
    }
    let bus = dev.bus.get()?;
    let usbctl = bus.usbctl.get()?;
    dev.myhub.get()?;
    dev.powersrc.get()?;

    for (i, ifc) in dev.ifaces().iter().enumerate() {
        if ptr::eq(iface, ifc) {
            // SAFETY: the bus's attached `usb(4)` device, alive while the bus is.
            let unit = unsafe { usbctl.as_ref() }.dv_unit.get() as u8;
            let route = usbd_get_routestring(dev).unwrap_or(0);
            return Some((unit, route, i as u8));
        }
    }

    None
}

/// `usbd_get_cdesc`: retrieve a complete descriptor for a certain device and index (a
/// `M_TEMP` copy the caller frees, `*lenp` bytes).
pub fn usbd_get_cdesc(
    dev: &'static UsbdDevice,
    index: i32,
    lenp: Option<&mut u32>,
) -> Option<NonNull<u8>> {
    if index == USB_CURRENT_CONFIG_INDEX {
        let tdesc = usbd_get_config_descriptor(dev)?;
        let bytes = dev.cdesc.get()?;
        let len = usize::from(ugetw(tdesc.wTotalLength)).min(bytes.len());
        if let Some(l) = lenp {
            *l = len as u32;
        }
        let cdesc = malloc(len.max(1), M_TEMP, M_WAITOK)?;
        // SAFETY: `len` bytes of the current descriptor into a fresh allocation of as many.
        unsafe { ptr::copy_nonoverlapping(bytes.as_ptr(), cdesc.as_ptr(), len) };
        Some(cdesc)
    } else {
        let mut cdescr = UsbConfigDescriptor::zeroed();
        let err = usbd_get_desc(
            dev,
            i32::from(UDESC_CONFIG),
            index,
            USB_CONFIG_DESCRIPTOR_SIZE as i32,
            cdescr.as_bytes_mut(),
        );
        if err.is_err() || cdescr.bDescriptorType != UDESC_CONFIG {
            return None;
        }
        let len = usize::from(ugetw(cdescr.wTotalLength));
        if let Some(l) = lenp {
            *l = len as u32;
        }
        let cdesc = malloc(len.max(1), M_TEMP, M_WAITOK)?;
        // SAFETY: a fresh allocation of `len` bytes, ours.
        let buf = unsafe {
            cdesc.as_ptr().write_bytes(0, len);
            core::slice::from_raw_parts_mut(cdesc.as_ptr(), len)
        };
        let err = usbd_get_desc(dev, i32::from(UDESC_CONFIG), index, len as i32, buf);
        if err.is_err() {
            free(cdesc, M_TEMP, len.max(1));
            return None;
        }
        Some(cdesc)
    }
}

/// `usb_free_device`: closes pipe 0 and frees the device with its configuration and strings.
///
/// # Safety
///
/// `dev` came from `usbd_new_device`, has no attached children, and nobody uses it
/// afterwards.
pub unsafe fn usb_free_device(dev: NonNull<UsbdDevice>) {
    // SAFETY: the caller's contract: a live device until the `free` below.
    let d = unsafe { dev.as_ref() };

    if let Some(p) = d.default_pipe.get() {
        // SAFETY: pipe 0 belongs to the device, which is going away.
        unsafe { usbd_close_pipe(NonNull::from(p)) };
    }
    if !d.ifaces.get().is_null() {
        let nifc = d.cdesc().map_or(0, |cd| usize::from(cd.bNumInterfaces));
        for ifcidx in 0..nifc {
            usbd_free_iface_data(d, ifcidx);
        }
        if let Some(p) = NonNull::new(d.ifaces.get()) {
            free(p.cast::<u8>(), M_USB, nifc * size_of::<UsbdInterface>());
        }
        d.ifaces.set(ptr::null_mut());
    }
    if let Some(b) = d.cdesc.get() {
        // The size `usbd_set_config_index` allocated.
        free(NonNull::from(b).cast::<u8>(), M_USB, b.len().max(1));
        d.cdesc.set(None);
    }
    subdevs_free(d);
    let bus = d.bus();
    if let Some(slot) = bus.devices.get(usize::from(d.address.get())) {
        slot.set(None);
    }

    for s in [&d.vendor, &d.product, &d.serial] {
        if let Some(p) = NonNull::new(s.get()) {
            free(p, M_USB, USB_MAX_STRING_LEN);
        }
    }

    free(dev.cast::<u8>(), M_USB, size_of::<UsbdDevice>());
}

/// `usbd_detach`: should only be called by the USB thread doing bus exploration to avoid
/// connect/disconnect races. Detaches the device's drivers and frees it if they all went.
pub fn usbd_detach(dev: &'static UsbdDevice, _parent: &Device) -> Result<(), Errno> {
    let mut rv: Result<(), Errno> = Ok(());

    usbd_deactivate(dev);

    if dev.ndevs.get() > 0 {
        for slot in dev.subdevs() {
            let Some(sd) = slot.get() else {
                break;
            };
            // SAFETY: an attached child of the device; `config_detach` frees it on success
            // and the slot is not used again (the device goes too, or stays dying).
            let r = unsafe { config_detach(sd, crate::sys::device::DETACH_FORCE) };
            if rv.is_ok() {
                rv = r;
            }
        }
    }

    if rv.is_ok() {
        // SAFETY: every child detached; the caller (the hub) forgets the device.
        unsafe { usb_free_device(NonNull::from(dev)) };
    }

    rv
}

#[cfg(test)]
mod tests;
