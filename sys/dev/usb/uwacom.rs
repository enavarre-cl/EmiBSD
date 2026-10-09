/* <LICENSES> */
/*
 * Copyright (c) 2016 Frank Groeneveld <frank@frankgroeneveld.nl>
 *
 * Permission to use, copy, modify, and distribute this software for any
 * purpose with or without fee is hereby granted, provided that the above
 * copyright notice and this permission notice appear in all copies.
 *
 * THE SOFTWARE IS PROVIDED "AS IS" AND THE AUTHOR DISCAIMS ALL WARRANTIES
 * WITH REGARD TO THIS SOFTWARE INCLUDING ALL IMPLIED WARRANTIES OF
 * MERCHANTABILITY AND FITNESS. IN NO EVENT SHALL THE AUTHOR BE LIABLE FOR
 * ANY SPECIAL, DIRECT, INDIRECT, OR CONSEQUENTIAL DAMAGES OR ANY DAMAGES
 * WHATSOEVER RESULTING FROM LOSS OF USE, DATA OR PROFITS, WHETHER IN AN
 * ACTION OF CONTRACT, NEGLIGENCE OR OTHER TORTIOUS ACTION, ARISING OUT OF
 * OR IN CONNECTION WITH THE USE OR PERFORMANCE OF THIS SOFTWARE.
 */
/* </LICENSES> */

/* <CODE> */
//! uwacom(4): the USB Wacom tablet driver, a child of `uhidev(4)`: it decodes the reports of
//! the Intuos Draw, One S/M and Intuos S tablets and feeds `wsmouse(4)` (as a touch panel).
//!
//! Upstream: sys/dev/usb/uwacom.c @ 3ce1f3f79392
//!
//! `uwacom_match` takes only the four Wacom products of `uwacom_devs` (and then a Wacom
//! digitizer collection or a pointer usage in the report). The Intuos Draw sends big-endian
//! absolute coordinates in a fixed layout and uses the tip pressure for the first button
//! (`uwacom_intr_legacy`); the One S and Intuos S take a feature report that switches them to
//! the descriptor-described mode, which `hidms_setup` (`HIDMS_WACOM_SETUP`) parses and
//! `uwacom_intr` reports as relative motion plus buttons.
//!
//! ## Deviations
//! - The `wsmouse` access operations are `fn` pointers over the device's address as the
//!   untyped cookie (`*mut c_void`), as `wsmousevar.rs` declares them; `uwacom_enable`
//!   returns the errno as an int, as the C; `uwacom_ioctl` returns `Ok(false)` where the C
//!   returns -1.
//! - `uwacom_intr_legacy` and `uwacom_intr` get the report as a slice (see `uhidev.rs`) and
//!   report to the `wsmouse` child only when one attached (the C would pass NULL).
//! - `be16toh` of the C's `int` coordinate is `u16::from_be` of its low 16 bits.

use core::cell::Cell;
use core::ffi::c_void;
use core::ptr::{self, NonNull};

use crate::dev::hid::hid::{
    HUD_DIGITIZER, HUG_POINTER, HUP_DIGITIZERS, HUP_WACOM, HidLocation, hid_feature, hid_get_data,
    hid_input, hid_is_collection, hid_locate, hid_output, hid_report_size, hid_usage2,
};
use crate::dev::hid::hidms::{
    hidms_attach, hidms_detach, hidms_disable, hidms_enable, hidms_ioctl, hidms_setup,
};
use crate::dev::hid::hidmsvar::{HIDMS_ABSX, HIDMS_ABSY, HIDMS_WACOM_SETUP, Hidms};
use crate::dev::usb::uhidev::{
    Uhidev, UhidevAttachArg, uhidev_close, uhidev_get_report_desc, uhidev_ioctl, uhidev_open,
    uhidev_set_report,
};
use crate::dev::usb::usbdevs::{
    USB_PRODUCT_WACOM_INTUOS_DRAW, USB_PRODUCT_WACOM_INTUOS_S, USB_PRODUCT_WACOM_ONE_M,
    USB_PRODUCT_WACOM_ONE_S, USB_VENDOR_WACOM,
};
use crate::dev::usb::usbdi::{UMATCH_IFACECLASS, UMATCH_NONE, UsbDevno, usb_lookup, usbd_is_dying};
use crate::dev::usb::usbdi_util::usbd_set_idle;
use crate::dev::usb::usbhid::UHID_FEATURE_REPORT;
use crate::dev::wscons::wsconsio::{WSMOUSE_TYPE_TPANEL, WSMOUSEIO_GTYPE};
use crate::dev::wscons::wsmouse::{
    wsmouse_buttons, wsmouse_input_sync, wsmouse_motion, wsmouse_position,
};
use crate::dev::wscons::wsmousevar::WsmouseAccessops;
use crate::sys::device::{CfMatch, Cfattach, Cfdriver, DV_DULL, Device, Softc};
use crate::sys::errno::Errno;
use crate::sys::ioctl::ioctl_ret;
use crate::sys::proc::Proc;

/// `UWACOM_USE_PRESSURE`: button 0 is flaky, use tip pressure.
pub const UWACOM_USE_PRESSURE: i32 = 0x0001;
/// `UWACOM_BIG_ENDIAN`: XY reporting byte order.
pub const UWACOM_BIG_ENDIAN: i32 = 0x0002;

/// `struct uwacom_softc`.
#[repr(C)]
pub struct UwacomSoftc {
    /// `sc_hdev`: the `uhidev` child head.
    pub sc_hdev: Uhidev,
    /// `sc_ms`: the bus-independent mouse.
    pub sc_ms: Hidms,
    /// `sc_loc_tip_press`.
    pub sc_loc_tip_press: Cell<HidLocation>,
    /// `sc_flags`: `UWACOM_*`.
    pub sc_flags: Cell<i32>,
    /// `sc_x`: the last X.
    pub sc_x: Cell<i32>,
    /// `sc_y`: the last Y.
    pub sc_y: Cell<i32>,
    /// `sc_z`: the last pressure.
    pub sc_z: Cell<i32>,
    /// `sc_w`: the last distance.
    pub sc_w: Cell<i32>,
    /// `sc_moved`: the first report has been seen.
    pub sc_moved: Cell<i32>,
}

// SAFETY: `#[repr(C)]`, the `Uhidev` (whose first member is the `Device`) first; `Hidms` is
// valid as all-zero (its `new()`), the rest are integer and `HidLocation` `Cell`s.
unsafe impl Softc for UwacomSoftc {}

/// `uwacom_cd`.
pub static UWACOM_CD: Cfdriver = Cfdriver::new(b"uwacom", DV_DULL, 0);

/// `uwacom_devs`.
pub static UWACOM_DEVS: [UsbDevno; 4] = [
    UsbDevno {
        ud_vendor: USB_VENDOR_WACOM,
        ud_product: USB_PRODUCT_WACOM_INTUOS_DRAW,
    },
    UsbDevno {
        ud_vendor: USB_VENDOR_WACOM,
        ud_product: USB_PRODUCT_WACOM_ONE_S,
    },
    UsbDevno {
        ud_vendor: USB_VENDOR_WACOM,
        ud_product: USB_PRODUCT_WACOM_ONE_M,
    },
    UsbDevno {
        ud_vendor: USB_VENDOR_WACOM,
        ud_product: USB_PRODUCT_WACOM_INTUOS_S,
    },
];

/// `uwacom_ca`.
pub static UWACOM_CA: Cfattach = Cfattach {
    ca_devsize: size_of::<UwacomSoftc>(),
    ca_match: Some(uwacom_match),
    ca_attach: uwacom_attach,
    ca_detach: Some(uwacom_detach),
    ca_activate: None,
};

/// `uwacom_accessops`: the tablet access functions.
pub static UWACOM_ACCESSOPS: WsmouseAccessops = WsmouseAccessops {
    enable: uwacom_enable,
    ioctl: uwacom_ioctl,
    disable: uwacom_disable,
};

/// `(struct uwacom_softc *)self`.
fn uwacom_softc(self_: &Device) -> &'static UwacomSoftc {
    // SAFETY: only called with devices `uwacom_ca` made (uwacom's own entry points); an
    // attached device lives until `config_detach` frees it after `uwacom_detach`.
    unsafe { &*ptr::from_ref(self_.softc::<UwacomSoftc>()) }
}

/// The softc behind a `wsmouse` access cookie: the address of the uwacom device.
fn uwacom_cookie(v: *mut c_void) -> &'static UwacomSoftc {
    // SAFETY: `hidms_attach` hands the address of the uwacom device (the head of its softc)
    // as the cookie; the device outlives the `wsmouse` child (`uwacom_detach` detaches the
    // child first).
    unsafe { &*v.cast::<UwacomSoftc>() }
}

/// The softc of a report handler's `Uhidev`.
fn uwacom_of_hdev(addr: &Uhidev) -> &UwacomSoftc {
    // SAFETY: `uwacom_attach` set `sc_intr` to one of this driver's handlers in the
    // `sc_hdev` of a `UwacomSoftc`, which `uhidev_intr` hands back: the head of the softc,
    // which has `UwacomSoftc`'s layout.
    unsafe { &*ptr::from_ref(addr).cast::<UwacomSoftc>() }
}

/// `uwacom_match`: one of the four Wacom tablets, with a digitizer collection or a pointer.
pub fn uwacom_match(_parent: Option<&Device>, _match: &CfMatch, aux: *mut c_void) -> i32 {
    // SAFETY: `uhidev_attach` passes a `UhidevAttachArg` that lives across the
    // `config_found_sm` calling this.
    let uha = unsafe { &*aux.cast::<UhidevAttachArg<'_>>() };

    if uha.claim_multiple_reportid() {
        return UMATCH_NONE;
    }

    if usb_lookup(&UWACOM_DEVS, uha.uaa.vendor as u16, uha.uaa.product as u16).is_none() {
        return UMATCH_NONE;
    }

    let desc = uhidev_get_report_desc(uha.parent);

    if hid_is_collection(
        desc,
        uha.reportid,
        hid_usage2(HUP_WACOM | HUP_DIGITIZERS, HUD_DIGITIZER) as i32,
    ) {
        return UMATCH_IFACECLASS;
    }
    if !hid_locate(
        desc,
        hid_usage2(HUP_WACOM, HUG_POINTER),
        uha.reportid,
        hid_input,
        None,
        None,
    ) {
        return UMATCH_NONE;
    }

    UMATCH_IFACECLASS
}

/// `uwacom_attach`.
pub fn uwacom_attach(_parent: Option<&Device>, self_: &Device, aux: *mut c_void) {
    let sc = uwacom_softc(self_);
    let ms = &sc.sc_ms;
    // SAFETY: as in `uwacom_match`, valid during the attach.
    let uha = unsafe { &*aux.cast::<UhidevAttachArg<'_>>() };
    let uaa = uha.uaa;
    let mut wacom_report_buf: [u8; 2] = [0x02, 0x02];

    sc.sc_hdev.sc_intr.set(Some(uwacom_intr_legacy));
    sc.sc_hdev.sc_parent.set(Some(uha.parent));
    sc.sc_hdev.sc_udev.set(Some(uaa.device()));
    sc.sc_hdev.sc_report_id.set(uha.reportid);

    let _ = usbd_set_idle(uha.parent.udev(), uha.parent.sc_ifaceno.get(), 0, 0);

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

    ms.sc_device.set(Some(NonNull::from(self_)));
    ms.sc_rawmode.set(1);
    ms.sc_flags.set(HIDMS_ABSX | HIDMS_ABSY);
    ms.sc_num_buttons.set(3);

    ms.update_loc_x(|l| {
        l.pos = 8;
        l.size = 16;
    });
    ms.update_loc_y(|l| {
        l.pos = 24;
        l.size = 16;
    });

    ms.update_tsscale(|t| {
        t.minx = 0;
        t.miny = 0;
    });

    for i in 0..3 {
        ms.update_loc_btn(i, |l| {
            l.pos = i as u32;
            l.size = 1;
        });
    }

    let product = uaa.product;
    if product == i32::from(USB_PRODUCT_WACOM_ONE_S)
        || product == i32::from(USB_PRODUCT_WACOM_INTUOS_S)
    {
        let _ = uhidev_set_report(
            uha.parent,
            i32::from(UHID_FEATURE_REPORT),
            i32::from(sc.sc_hdev.sc_report_id.get()),
            &mut wacom_report_buf,
        );
        sc.sc_hdev.sc_intr.set(Some(uwacom_intr));
        let _ = hidms_setup(self_, ms, HIDMS_WACOM_SETUP, i32::from(repid), desc);
    } else if product == i32::from(USB_PRODUCT_WACOM_INTUOS_DRAW) {
        sc.sc_flags.set(UWACOM_USE_PRESSURE | UWACOM_BIG_ENDIAN);
        sc.sc_loc_tip_press.set(HidLocation {
            size: 8,
            count: 0,
            pos: 43,
        });
        ms.update_tsscale(|t| {
            t.maxx = 7600;
            t.maxy = 4750;
        });
    }

    hidms_attach(ms, &UWACOM_ACCESSOPS);
}

/// `uwacom_detach`.
pub fn uwacom_detach(self_: &Device, flags: i32) -> Result<(), Errno> {
    let sc = uwacom_softc(self_);

    hidms_detach(&sc.sc_ms, flags)
}

/// `be16toh` of a coordinate the HID code read as a signed 16-bit field.
fn be16toh(v: i32) -> i32 {
    i32::from(u16::from_be(v as u16))
}

/// `uwacom_intr_legacy`: a report of the Intuos Draw (and any other tablet that is not
/// switched to the HID layout).
pub fn uwacom_intr_legacy(addr: &Uhidev, data: &mut [u8]) {
    let sc = uwacom_of_hdev(addr);
    let ms = &sc.sc_ms;
    let mut buttons: u32 = 0;

    if ms.sc_enabled.get() == 0 {
        return;
    }

    // ignore proximity, it will cause invalid button 2 events
    if data.first().is_some_and(|b| b & 0xf0 == 0xc0) {
        return;
    }

    let mut x = hid_get_data(data, &ms.sc_loc_x.get());
    let mut y = hid_get_data(data, &ms.sc_loc_y.get());

    if sc.sc_flags.get() & UWACOM_BIG_ENDIAN != 0 {
        x = be16toh(x);
        y = be16toh(y);
    }

    for i in 0..ms.sc_num_buttons.get().max(0) as usize {
        if hid_get_data(data, &ms.loc_btn(i)) != 0 {
            buttons |= 1 << i;
        }
    }

    if sc.sc_flags.get() & UWACOM_USE_PRESSURE != 0 {
        let pressure = hid_get_data(data, &sc.sc_loc_tip_press.get());
        if pressure > 10 {
            buttons |= 1;
        } else {
            buttons &= !1;
        }
    }

    if (x != 0 || y != 0 || buttons != ms.sc_buttons.get())
        && let Some(wsm) = ms.sc_wsmousedev.get()
    {
        // SAFETY: the `wsmouse` child `hidms_attach` stored; `hidms_detach` clears
        // `sc_wsmousedev` only after detaching it, under the kernel lock.
        let wsm = unsafe { wsm.as_ref() };
        wsmouse_position(wsm, x, y);
        wsmouse_buttons(wsm, buttons);
        wsmouse_input_sync(wsm);
    }
}

/// `uwacom_intr`: a report of a tablet in the HID layout: pen movement as relative deltas.
pub fn uwacom_intr(addr: &Uhidev, data: &mut [u8]) {
    let sc = uwacom_of_hdev(addr);
    let ms = &sc.sc_ms;
    let mut buttons: u32 = 0;

    if ms.sc_enabled.get() == 0 {
        return;
    }

    let mut x = hid_get_data(data, &ms.sc_loc_x.get());
    let y = hid_get_data(data, &ms.sc_loc_y.get());
    let pressure = hid_get_data(data, &ms.sc_loc_z.get());
    let distance = hid_get_data(data, &ms.sc_loc_w.get());
    let mut y = y;

    if sc.sc_moved.get() == 0 {
        sc.sc_x.set(x);
        sc.sc_y.set(y);
        sc.sc_z.set(pressure);
        sc.sc_w.set(distance);
        sc.sc_moved.set(1);
    }

    let dx = sc.sc_x.get().wrapping_sub(x);
    let dy = sc.sc_y.get().wrapping_sub(y);
    // Clamp sensitivity to +/-127
    let dz = (sc.sc_z.get() / 32).wrapping_sub(pressure / 32);
    let dw = sc.sc_w.get().wrapping_sub(distance);

    sc.sc_x.set(x);
    sc.sc_y.set(y);
    sc.sc_z.set(pressure);
    sc.sc_w.set(distance);

    if sc.sc_flags.get() & UWACOM_BIG_ENDIAN != 0 {
        x = be16toh(x);
        y = be16toh(y);
    }

    let nstylus = ms.sc_num_stylus_buttons.get().max(0) as usize;
    let nbuttons = ms.sc_num_buttons.get().max(0) as usize;
    let mut i = 0;
    while i < nstylus {
        if hid_get_data(data, &ms.loc_btn(i)) != 0 {
            buttons |= 1 << i;
        }
        i += 1;
    }

    let mut j = 0;
    while i < nbuttons {
        if hid_get_data(data, &ms.loc_btn(i)) != 0 {
            buttons |= 1 << j;
        }
        i += 1;
        j += 1;
    }

    if (x != 0 || y != 0 || pressure != 0 || distance != 0 || buttons != ms.sc_buttons.get())
        && let Some(wsm) = ms.sc_wsmousedev.get()
    {
        // SAFETY: as in `uwacom_intr_legacy`.
        let wsm = unsafe { wsm.as_ref() };
        wsmouse_motion(wsm, dx.wrapping_neg(), dy, dz, dw);
        wsmouse_buttons(wsm, buttons);
        wsmouse_input_sync(wsm);
    }
}

/// `uwacom_enable`: the `wsmouse` access operation: open the interrupt pipe; returns the
/// errno.
pub fn uwacom_enable(v: *mut c_void) -> i32 {
    let sc = uwacom_cookie(v);
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

    match uhidev_open(&sc.sc_hdev) {
        Ok(()) => 0,
        Err(e) => e as i32,
    }
}

/// `uwacom_disable`: the `wsmouse` access operation: close the interrupt pipe.
pub fn uwacom_disable(v: *mut c_void) {
    let sc = uwacom_cookie(v);
    let ms = &sc.sc_ms;

    hidms_disable(ms);
    uhidev_close(&sc.sc_hdev);
}

/// `uwacom_ioctl`: the `wsmouse` access operation for the ioctls: the type, then `uhidev`'s
/// and `hidms`'s. `Ok(false)` where the C returns -1.
pub fn uwacom_ioctl(
    v: *mut c_void,
    cmd: u64,
    data: &mut [u8],
    flag: i32,
    p: Option<&Proc>,
) -> Result<bool, Errno> {
    let sc = uwacom_cookie(v);
    let ms = &sc.sc_ms;

    if cmd == WSMOUSEIO_GTYPE {
        ioctl_ret(data, &WSMOUSE_TYPE_TPANEL);
        return Ok(true);
    }

    if uhidev_ioctl(&sc.sc_hdev, cmd, data, flag, p)? {
        return Ok(true);
    }

    hidms_ioctl(ms, cmd, data, flag, p)
}
/* </CODE> */

/* <TESTS> */
#[cfg(test)]
mod tests {
    // Host tests for `uwacom`: the device table, the byte order helper and the relative
    // reports (read back from the state they keep; there is no `wsmouse` child on the host).

    use std::boxed::Box;
    use std::{assert, assert_eq};

    use super::*;

    fn softc() -> Box<UwacomSoftc> {
        // SAFETY: a zeroed softc is a valid one (`unsafe impl Softc`): `config_make_softc`
        // allocates them zeroed.
        Box::new(unsafe { core::mem::zeroed::<UwacomSoftc>() })
    }

    #[test]
    fn only_four_wacom_products_match() {
        for p in [
            USB_PRODUCT_WACOM_INTUOS_DRAW,
            USB_PRODUCT_WACOM_ONE_S,
            USB_PRODUCT_WACOM_ONE_M,
            USB_PRODUCT_WACOM_INTUOS_S,
        ] {
            assert!(usb_lookup(&UWACOM_DEVS, USB_VENDOR_WACOM, p).is_some());
        }
        // QEMU's usb-wacom-tablet is a Wacom (0x056a) PenPartner, product 0x0000: not ours
        assert!(usb_lookup(&UWACOM_DEVS, USB_VENDOR_WACOM, 0x0000).is_none());
        assert!(usb_lookup(&UWACOM_DEVS, 0x1234, USB_PRODUCT_WACOM_ONE_S).is_none());
    }

    #[test]
    fn big_endian_coordinates() {
        assert_eq!(be16toh(0x3412), 0x1234);
        assert_eq!(be16toh(0x0100), 1);
        assert_eq!(be16toh(-1), 0xffff, "the low 16 bits, unsigned");
    }

    #[test]
    fn hid_layout_reports_move_relative_to_the_first() {
        let sc = softc();
        let ms = &sc.sc_ms;
        let loc = |pos| HidLocation {
            size: 16,
            count: 1,
            pos,
        };
        ms.sc_loc_x.set(loc(0));
        ms.sc_loc_y.set(loc(16));
        ms.sc_loc_z.set(loc(32));
        ms.sc_loc_w.set(loc(48));
        // not enabled: nothing is kept
        let mut r = [100, 0, 50, 0, 64, 0, 7, 0];
        uwacom_intr(&sc.sc_hdev, &mut r);
        assert_eq!(sc.sc_moved.get(), 0);

        ms.sc_enabled.set(1);
        uwacom_intr(&sc.sc_hdev, &mut r);
        assert_eq!(sc.sc_moved.get(), 1);
        assert_eq!((sc.sc_x.get(), sc.sc_y.get()), (100, 50));
        assert_eq!((sc.sc_z.get(), sc.sc_w.get()), (64, 7));

        let mut r2 = [90, 0, 60, 0, 0, 0, 0, 0];
        uwacom_intr(&sc.sc_hdev, &mut r2);
        assert_eq!((sc.sc_x.get(), sc.sc_y.get()), (90, 60));
        assert_eq!((sc.sc_z.get(), sc.sc_w.get()), (0, 0));
    }
}
/* </TESTS> */
