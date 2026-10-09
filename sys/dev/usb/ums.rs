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
/* </LICENSES> */

/* <CODE> */
//! ums(4): the USB mouse, tablet and touch screen driver, a child of `uhidev(4)`: the glue
//! between a pointer's interrupt pipe (`uhidev.rs`) and the bus-independent HID mouse code
//! (`hid/hidms.rs`) that decodes the reports and feeds `wsmouse(4)`.
//!
//! Upstream: sys/dev/usb/ums.c @ 3ce1f3f79392
//!
//! `ums_match` takes a report ID whose collection is a pointer, a mouse, a touch screen or a
//! pen (and, for Apple's virtual digitizer, all the report IDs). `ums_attach` sizes the
//! reports, applies the device's quirks (the Elecom button count fix, Microsoft's mice with
//! a bad class), lets `hidms` parse the descriptor and offers a `wsmouse` child. The
//! interrupt pipe is opened by `ums_enable` (the `wsmouse` child's `enable`) through
//! `uhidev_open`; a device with `UQ_ALWAYS_OPEN` keeps it open from the attach.
//!
//! ## Deviations
//! - The `wsmouse` access operations are `fn` pointers over the device's address as the
//!   untyped cookie (`*mut c_void`), as `wsmousevar.rs` declares them; `ums_enable` returns
//!   the errno as an int, as the C; `ums_ioctl` returns `Ok(false)` where the C returns -1.
//! - `ums_fix_elecom_descriptor` patches the report descriptor `uhidev` owns in place,
//!   through its `sc_repdesc` pointer (the C is handed a `void *`); the check and the patch
//!   are the pure function `elecom_fix_descriptor`, which the host tests exercise.
//! - `ums_intr` gets the report as a slice (see `uhidev.rs`).

use core::cell::Cell;
use core::ffi::c_void;
use core::ptr;

use crate::dev::hid::hid::{
    HUD_PEN, HUD_TOUCHSCREEN, HUG_MOUSE, HUG_POINTER, HUP_DIGITIZERS, HUP_GENERIC_DESKTOP,
    hid_feature, hid_input, hid_is_collection, hid_output, hid_report_size, hid_usage2,
};
use crate::dev::hid::hidms::{
    hidms_attach, hidms_detach, hidms_disable, hidms_enable, hidms_input, hidms_ioctl, hidms_setup,
};
use crate::dev::hid::hidmsvar::{
    HIDMS_LEADINGBYTE, HIDMS_MS_BAD_CLASS, HIDMS_REVZ, HIDMS_SPUR_BUT_UP, HIDMS_VENDOR_BUTTONS,
    HIDMS_Z, Hidms,
};
use crate::dev::usb::uhidev::{
    UHIDEV_OPEN, Uhidev, UhidevAttachArg, uhidev_close, uhidev_get_report_desc, uhidev_ioctl,
    uhidev_open,
};
use crate::dev::usb::usb_quirks::{
    UQ_ALWAYS_OPEN, UQ_MS_BAD_CLASS, UQ_MS_LEADING_BYTE, UQ_MS_REVZ, UQ_MS_VENDOR_BUTTONS,
    UQ_SPUR_BUT_UP,
};
use crate::dev::usb::usbdevs::{
    USB_PRODUCT_APPLE_VIRTUAL_DIGITIZER, USB_PRODUCT_ELECOM_MDT1DRBK, USB_PRODUCT_ELECOM_MDT1URBK,
    USB_PRODUCT_ELECOM_MHT1DRBK, USB_PRODUCT_ELECOM_MHT1URBK, USB_PRODUCT_ELECOM_MXT3DRBK,
    USB_PRODUCT_ELECOM_MXT3URBK, USB_PRODUCT_ELECOM_MXT4DRBK, USB_PRODUCT_MICROSOFT_WLNOTEBOOK3,
    USB_VENDOR_APPLE, USB_VENDOR_ELECOM, USB_VENDOR_MICROSOFT,
};
use crate::dev::usb::usbdi::{
    UMATCH_IFACECLASS, UMATCH_NONE, UMATCH_VENDOR_PRODUCT, usbd_get_quirks, usbd_is_dying,
};
use crate::dev::usb::usbdi_util::usbd_set_idle;
use crate::dev::wscons::wsconsio::{WSMOUSE_TYPE_USB, WSMOUSEIO_GTYPE};
use crate::dev::wscons::wsmousevar::WsmouseAccessops;
use crate::kprintf;
use crate::sys::device::{CfMatch, Cfattach, Cfdriver, DV_DULL, Device, Softc};
use crate::sys::errno::Errno;
use crate::sys::ioctl::ioctl_ret;
use crate::sys::proc::Proc;

/// `struct ums_softc`.
#[repr(C)]
pub struct UmsSoftc {
    /// `sc_hdev`: the `uhidev` child head.
    pub sc_hdev: Uhidev,
    /// `sc_ms`: the bus-independent mouse.
    pub sc_ms: Hidms,
    /// `sc_quirks`.
    pub sc_quirks: Cell<u32>,
}

// SAFETY: `#[repr(C)]`, the `Uhidev` (whose first member is the `Device`) first; `Hidms` is
// valid as all-zero (its `new()`) and the rest is an integer `Cell`.
unsafe impl Softc for UmsSoftc {}

/// `ums_accessops`: the mouse access functions.
pub static UMS_ACCESSOPS: WsmouseAccessops = WsmouseAccessops {
    enable: ums_enable,
    ioctl: ums_ioctl,
    disable: ums_disable,
};

/// `ums_cd`.
pub static UMS_CD: Cfdriver = Cfdriver::new(b"ums", DV_DULL, 0);

/// `ums_ca`.
pub static UMS_CA: Cfattach = Cfattach {
    ca_devsize: size_of::<UmsSoftc>(),
    ca_match: Some(ums_match),
    ca_attach: ums_attach,
    ca_detach: Some(ums_detach),
    ca_activate: None,
};

/// `(struct ums_softc *)self`.
fn ums_softc(self_: &Device) -> &'static UmsSoftc {
    // SAFETY: only called with devices `ums_ca` made (ums's own entry points); an attached
    // device lives until `config_detach` frees it after `ums_detach`.
    unsafe { &*ptr::from_ref(self_.softc::<UmsSoftc>()) }
}

/// The softc behind a `wsmouse` access cookie: the address of the ums device.
fn ums_cookie(v: *mut c_void) -> &'static UmsSoftc {
    // SAFETY: `hidms_attach` hands the address of the ums device (the head of its softc) as
    // the cookie; the device outlives the `wsmouse` child (`ums_detach` detaches the child
    // first).
    unsafe { &*v.cast::<UmsSoftc>() }
}

/// `ums_match`: a report ID that is a pointer, mouse, touch screen or pen collection.
pub fn ums_match(_parent: Option<&Device>, _match: &CfMatch, aux: *mut c_void) -> i32 {
    // SAFETY: `uhidev_attach` passes a `UhidevAttachArg` that lives across the
    // `config_found_sm` calling this.
    let uha = unsafe { &*aux.cast::<UhidevAttachArg<'_>>() };

    if uha.claim_multiple_reportid() {
        // The virtual USB digitizer exposed under Apple Virtualization uses several report
        // IDs, but only the pointer report is relevant here. We still claim them all to
        // prevent other drivers from binding.
        if uha.uaa.vendor == i32::from(USB_VENDOR_APPLE)
            && uha.uaa.product == i32::from(USB_PRODUCT_APPLE_VIRTUAL_DIGITIZER)
        {
            for i in 0..uha.nreports as usize {
                // SAFETY: `claimed` has `nreports` entries (`UhidevAttachArg`).
                unsafe { uha.claimed.add(i).write(1) };
            }

            return UMATCH_VENDOR_PRODUCT;
        }
        return UMATCH_NONE;
    }

    let desc = uhidev_get_report_desc(uha.parent);

    for usage in [
        hid_usage2(HUP_GENERIC_DESKTOP, HUG_POINTER),
        hid_usage2(HUP_GENERIC_DESKTOP, HUG_MOUSE),
        hid_usage2(HUP_DIGITIZERS, HUD_TOUCHSCREEN),
        hid_usage2(HUP_DIGITIZERS, HUD_PEN),
    ] {
        if hid_is_collection(desc, uha.reportid, usage as i32) {
            return UMATCH_IFACECLASS;
        }
    }

    UMATCH_NONE
}

/// `ums_attach`.
pub fn ums_attach(_parent: Option<&Device>, self_: &Device, aux: *mut c_void) {
    let sc = ums_softc(self_);
    let ms = &sc.sc_ms;
    // SAFETY: as in `ums_match`, valid during the attach; nothing else refers to it while
    // this driver attaches.
    let uha = unsafe { &mut *aux.cast::<UhidevAttachArg<'_>>() };
    let uaa = uha.uaa;
    let mut qflags: u32 = 0;

    sc.sc_hdev.sc_intr.set(Some(ums_intr));
    sc.sc_hdev.sc_parent.set(Some(uha.parent));
    sc.sc_hdev.sc_udev.set(Some(uaa.device()));

    // Use only the pointer report ID for the Apple Virtual USB Digitizer. The remaining
    // report IDs are claimed but ignored.
    if uaa.vendor == i32::from(USB_VENDOR_APPLE)
        && uaa.product == i32::from(USB_PRODUCT_APPLE_VIRTUAL_DIGITIZER)
    {
        uha.reportid = 1;
        sc.sc_hdev.sc_report_id.set(1);
    } else {
        sc.sc_hdev.sc_report_id.set(uha.reportid);
    }

    let _ = usbd_set_idle(uha.parent.udev(), uha.parent.sc_ifaceno.get(), 0, 0);

    sc.sc_quirks.set(usbd_get_quirks(uaa.device()).uq_flags);

    if uaa.vendor == i32::from(USB_VENDOR_ELECOM) {
        ums_fix_elecom_descriptor(sc, uha, uaa.product);
    }
    let desc = uhidev_get_report_desc(uha.parent);

    let repid = uha.reportid;
    sc.sc_hdev
        .sc_isize
        .set(hid_report_size(desc, hid_input, repid));
    sc.sc_hdev
        .sc_osize
        .set(hid_report_size(desc, hid_output, repid));
    sc.sc_hdev
        .sc_fsize
        .set(hid_report_size(desc, hid_feature, repid));

    let quirks = sc.sc_quirks.get();
    if quirks & UQ_MS_REVZ != 0 {
        qflags |= HIDMS_REVZ;
    }
    if quirks & UQ_SPUR_BUT_UP != 0 {
        qflags |= HIDMS_SPUR_BUT_UP;
    }
    if quirks & UQ_MS_BAD_CLASS != 0 {
        qflags |= HIDMS_MS_BAD_CLASS;
    }
    if quirks & UQ_MS_LEADING_BYTE != 0 {
        qflags |= HIDMS_LEADINGBYTE;
    }
    if quirks & UQ_MS_VENDOR_BUTTONS != 0 {
        qflags |= HIDMS_VENDOR_BUTTONS;
    }

    if hidms_setup(self_, ms, qflags, i32::from(uha.reportid), desc).is_err() {
        return;
    }

    // The Microsoft Wireless Notebook Optical Mouse 3000 Model 1049 has five Report IDs:
    // 19, 23, 24, 17, 18 (in the order they appear in report descriptor), it seems that
    // report 17 contains the necessary mouse information (3-buttons, X, Y, wheel) so we
    // specify it manually.
    if uaa.vendor == i32::from(USB_VENDOR_MICROSOFT)
        && uaa.product == i32::from(USB_PRODUCT_MICROSOFT_WLNOTEBOOK3)
    {
        ms.sc_flags.set(HIDMS_Z);
        ms.sc_num_buttons.set(3);
        // XXX change sc_hdev isize to 5?
        ms.update_loc_x(|l| l.pos = 8);
        ms.update_loc_y(|l| l.pos = 16);
        ms.update_loc_z(|l| l.pos = 24);
        ms.update_loc_btn(0, |l| l.pos = 0);
        ms.update_loc_btn(1, |l| l.pos = 1);
        ms.update_loc_btn(2, |l| l.pos = 2);
    }

    let cookie = ptr::from_ref(sc).cast_mut().cast::<c_void>();
    if sc.sc_quirks.get() & UQ_ALWAYS_OPEN != 0 {
        // open uhidev and keep it open
        ums_enable(cookie);
        // but mark the hidms not in use
        ums_disable(cookie);
    }

    hidms_attach(ms, &UMS_ACCESSOPS);
}

/// `ums_detach`.
pub fn ums_detach(self_: &Device, flags: i32) -> Result<(), Errno> {
    let sc = ums_softc(self_);

    hidms_detach(&sc.sc_ms, flags)
}

/// `ums_intr`: a report from the pointer's interrupt pipe (without its report ID).
pub fn ums_intr(addr: &Uhidev, ibuf: &mut [u8]) {
    // SAFETY: `ums_attach` set `sc_intr` to this function in the `sc_hdev` of a `UmsSoftc`,
    // which `uhidev_intr` hands back: the head of the softc, which has `UmsSoftc`'s layout.
    let sc = unsafe { &*ptr::from_ref(addr).cast::<UmsSoftc>() };
    let ms = &sc.sc_ms;

    if ms.sc_enabled.get() != 0 {
        hidms_input(ms, ibuf);
    }
}

/// `ums_enable`: the `wsmouse` access operation: open the interrupt pipe; returns the errno.
pub fn ums_enable(v: *mut c_void) -> i32 {
    let sc = ums_cookie(v);
    let ms = &sc.sc_ms;

    let Some(udev) = sc.sc_hdev.sc_udev.get() else {
        return Errno::EIO as i32;
    };
    if usbd_is_dying(udev) {
        return Errno::EIO as i32;
    }

    let rv = hidms_enable(ms);
    if rv != 0 {
        return rv;
    }

    if sc.sc_quirks.get() & UQ_ALWAYS_OPEN != 0 && sc.sc_hdev.sc_state.get() & UHIDEV_OPEN != 0 {
        0
    } else {
        match uhidev_open(&sc.sc_hdev) {
            Ok(()) => 0,
            Err(e) => e as i32,
        }
    }
}

/// `ums_disable`: the `wsmouse` access operation: close the interrupt pipe.
pub fn ums_disable(v: *mut c_void) {
    let sc = ums_cookie(v);
    let ms = &sc.sc_ms;

    hidms_disable(ms);

    if sc.sc_quirks.get() & UQ_ALWAYS_OPEN != 0 {
        return;
    }

    uhidev_close(&sc.sc_hdev);
}

/// `ums_ioctl`: the `wsmouse` access operation for the ioctls: `uhidev`'s, `hidms`'s, then
/// the mouse type. `Ok(false)` where the C returns -1.
pub fn ums_ioctl(
    v: *mut c_void,
    cmd: u64,
    data: &mut [u8],
    flag: i32,
    p: Option<&Proc>,
) -> Result<bool, Errno> {
    let sc = ums_cookie(v);
    let ms = &sc.sc_ms;

    if uhidev_ioctl(&sc.sc_hdev, cmd, data, flag, p)? {
        return Ok(true);
    }
    if hidms_ioctl(ms, cmd, data, flag, p)? {
        return Ok(true);
    }

    match cmd {
        WSMOUSEIO_GTYPE => {
            ioctl_ret(data, &WSMOUSE_TYPE_USB);
            Ok(true)
        }
        _ => Ok(false),
    }
}

/// The part of `ums_fix_elecom_descriptor` that looks at the descriptor `udesc` of the
/// Elecom product `product`: the number of buttons it patches in, or `None` when the
/// product or the descriptor is not one it fixes.
pub fn elecom_fix_descriptor(udesc: &mut [u8], product: i32) -> Option<u8> {
    // a descriptor fragment, offset: 12
    static MATCH: [u8; 10] = [
        0x95, 0x05, 0x75, 0x01, // Report Count (5), Report Size (1)
        0x05, 0x09, 0x19, 0x01, // Usage Page (Button), Usage Minimum (1)
        0x29, 0x05, //             Usage Maximum (5)
    ];
    let is = |p: u16| product == i32::from(p);

    let nbuttons: u8 = if is(USB_PRODUCT_ELECOM_MXT3URBK) // EX-G Trackballs
        || is(USB_PRODUCT_ELECOM_MXT3DRBK)
        || is(USB_PRODUCT_ELECOM_MXT4DRBK)
    {
        6
    } else if is(USB_PRODUCT_ELECOM_MDT1URBK) // DEFT Trackballs
        || is(USB_PRODUCT_ELECOM_MDT1DRBK)
        || is(USB_PRODUCT_ELECOM_MHT1URBK) // HUGE Trackballs
        || is(USB_PRODUCT_ELECOM_MHT1DRBK)
    {
        8
    } else {
        return None;
    };

    if udesc.len() < 32 || udesc[12..22] != MATCH || udesc[30] != 0x75 || udesc[31] != 3 {
        return None;
    }

    udesc[13] = nbuttons;
    udesc[21] = nbuttons;
    udesc[31] = 8 - nbuttons;
    Some(nbuttons)
}

/// `ums_fix_elecom_descriptor`: Elecom's trackballs describe five buttons in a report that
/// has more: patch the button count of the descriptor `uhidev` holds.
pub fn ums_fix_elecom_descriptor(sc: &UmsSoftc, uha: &UhidevAttachArg<'_>, product: i32) {
    let parent = uha.parent;
    let Some(base) = core::ptr::NonNull::new(parent.sc_repdesc.get()) else {
        return;
    };
    let size = parent.sc_repdesc_size.get().max(0) as usize;
    // SAFETY: `sc_repdesc` is an `M_USBDEV` allocation of `sc_repdesc_size` initialised bytes
    // that only `uhidev_detach` frees; during this attach nothing else refers to it (the
    // children attach one after the other under the kernel lock, and no slice of it is live).
    let udesc = unsafe { core::slice::from_raw_parts_mut(base.as_ptr(), size) };

    if let Some(nbuttons) = elecom_fix_descriptor(udesc, product) {
        kprintf!(
            "{}: fixing Elecom report descriptor (buttons: {})\n",
            sc.sc_hdev.sc_dev.xname(),
            nbuttons
        );
    }
}
/* </CODE> */

/* <TESTS> */
#[cfg(test)]
mod tests {
    // Host tests for `ums`: the match of the pointer collections over QEMU's report
    // descriptors and the Elecom patch.

    use std::vec::Vec;
    use std::{assert, assert_eq};

    use super::*;
    use crate::dev::hid::hid::hid_is_collection;

    /// QEMU's `usb-mouse` and `usb-tablet` are `HUG_MOUSE` collections, a keyboard is not.
    #[test]
    fn pointer_collections_match() {
        const MOUSE: &[u8] = &[
            0x05, 0x01, 0x09, 0x02, 0xa1, 0x01, 0x09, 0x01, 0xa1, 0x00, 0x05, 0x09, 0x19, 0x01,
            0x29, 0x03, 0x15, 0x00, 0x25, 0x01, 0x95, 0x03, 0x75, 0x01, 0x81, 0x02, 0xc0, 0xc0,
        ];
        assert!(hid_is_collection(
            MOUSE,
            0,
            hid_usage2(HUP_GENERIC_DESKTOP, HUG_MOUSE) as i32
        ));
        assert!(!hid_is_collection(
            MOUSE,
            0,
            hid_usage2(HUP_GENERIC_DESKTOP, HUG_POINTER) as i32
        ));
    }

    #[test]
    fn the_elecom_patch() {
        let mut d: Vec<u8> = std::vec![0u8; 40];
        d[12..22].copy_from_slice(&[0x95, 0x05, 0x75, 0x01, 0x05, 0x09, 0x19, 0x01, 0x29, 0x05]);
        d[30] = 0x75;
        d[31] = 3;
        let orig = d.clone();

        // not an Elecom product we fix
        assert_eq!(elecom_fix_descriptor(&mut d, 0x1234), None);
        assert_eq!(d, orig);

        // a DEFT trackball: eight buttons, no padding
        assert_eq!(
            elecom_fix_descriptor(&mut d, i32::from(USB_PRODUCT_ELECOM_MDT1URBK)),
            Some(8)
        );
        assert_eq!((d[13], d[21], d[31]), (8, 8, 0));

        // an EX-G trackball: six buttons, two bits of padding; the fragment is gone now
        let mut e = orig.clone();
        assert_eq!(
            elecom_fix_descriptor(&mut e, i32::from(USB_PRODUCT_ELECOM_MXT3DRBK)),
            Some(6)
        );
        assert_eq!((e[13], e[21], e[31]), (6, 6, 2));

        // a descriptor of another shape is left alone
        let mut f = orig.clone();
        f[30] = 0x95;
        assert_eq!(
            elecom_fix_descriptor(&mut f, i32::from(USB_PRODUCT_ELECOM_MXT3DRBK)),
            None
        );
        assert_eq!(f[13], 5);
        assert_eq!(
            elecom_fix_descriptor(&mut [0u8; 16], i32::from(USB_PRODUCT_ELECOM_MXT3DRBK)),
            None
        );
    }
}
/* </TESTS> */
