/*	$OpenBSD: ukbd.c,v 1.91 2025/08/14 14:39:44 deraadt Exp $	*/
/*      $NetBSD: ukbd.c,v 1.85 2003/03/11 16:44:00 augustss Exp $        */
/*	$OpenBSD: ukbdvar.h,v 1.5 2010/07/31 16:04:50 miod Exp $ */
/*	$NetBSD: ukbdvar.h,v 1.2 2000/06/01 14:29:00 augustss Exp $	*/
/* <LICENSES> */
/*
 * Copyright (c) 2010 Miodrag Vallat.
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
/*-
 * Copyright (c) 1999 The NetBSD Foundation, Inc.
 * All rights reserved.
 *
 * This code is derived from software contributed to The NetBSD Foundation
 * by Jason R. Thorpe of the Numerical Aerospace Simulation Facility,
 * NASA Ames Research Center.
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

//! ukbd(4): the USB keyboard driver, a child of `uhidev(4)`: the glue between a keyboard's
//! interrupt pipe (`uhidev.rs`) and the bus-independent HID keyboard code (`hid/hidkbd.rs`)
//! that decodes the reports and feeds `wskbd(4)`.
//!
//! Upstream: sys/dev/usb/ukbd.c @ 3ce1f3f79392, sys/dev/usb/ukbdvar.h @ 3ce1f3f79392
//!
//! `ukbd_match` takes a report ID whose collection is a keyboard (but not Yubikeys, and not
//! when a driver claims several report IDs). `ukbd_attach` sizes the reports, lets `hidkbd`
//! parse the descriptor, picks the Apple key munging, the layout (from the keyboard's HID
//! country code) and the console role, blinks the LEDs and offers a `wskbd` child. The
//! interrupt pipe is opened by `ukbd_enable` (the `wskbd` child's `enable`, or the attach of
//! the console keyboard) through `uhidev_open`.
//!
//! ## Deviations
//! - The keyboard layouts (`ukbdmap.c`) are `ukbdmap.rs`'s, read through `hidkbd.rs`'s
//!   `UKBD_KEYMAPDATA`. With a serial console nothing calls `ukbd_cnattach`
//!   (`hidkbd_is_console` stays 0), so no USB keyboard is the console keyboard, as in
//!   OpenBSD; `wskbd* at ukbd? mux 1` joins it to the mux of `wsdisplay0`.
//! - `UKBD_DEBUG` is not in GENERIC: the `DPRINTF`s are absent. `UKBD_LAYOUT` is not set: the
//!   default layout is `KB_US | KB_DEFAULT`.
//! - `__loongson__`'s `ukbd_gdium_munge` and its Fn translation table are not carried (the
//!   port has no loongson).
//! - The `wskbd` access operations are `fn` pointers over the device's address as the
//!   untyped cookie (`*mut c_void`), as `wskbdvar.rs` declares them; `ukbd_enable` returns
//!   the errno as an int, as the C. `ukbd_cnattach` returns 0 like the C.
//! - The C dereferences the HID descriptor `usbd_get_hid_descriptor` returns; here a missing
//!   one leaves the layout at the default.
//! - `ukbd_intr` gets the report as a slice (see `uhidev.rs`); the keyboard's `sc_ddb`
//!   timeout is always there (`option DDB` is in both GENERICs, `db_enter` always exists).

use core::cell::Cell;
use core::ffi::c_void;
use core::ptr;
use core::sync::atomic::Ordering;

use crate::dev::hid::hid::{
    HCC_MAX, HIO_VARIABLE, HUG_FN_KEY, HUG_KEYBOARD, HUP_APPLE, HUP_GENERIC_DESKTOP, HidLocation,
    hid_feature, hid_input, hid_is_collection, hid_locate, hid_output, hid_report_size, hid_usage2,
};
use crate::dev::hid::hidkbd::{
    HIDKBD_SPUR_BUT_UP, Hidkbd, UKBD_KEYMAPDATA, hidkbd_apple_iso_mba_munge,
    hidkbd_apple_iso_munge, hidkbd_apple_mba_munge, hidkbd_apple_munge, hidkbd_attach,
    hidkbd_attach_wskbd, hidkbd_bell, hidkbd_cngetc, hidkbd_detach, hidkbd_enable, hidkbd_input,
    hidkbd_ioctl, hidkbd_is_console, hidkbd_set_leds,
};
use crate::dev::usb::uhidev::{
    UHIDEV_OPEN, Uhidev, UhidevAttachArg, uhidev_close, uhidev_get_report_desc, uhidev_ioctl,
    uhidev_open, uhidev_set_report_async,
};
use crate::dev::usb::usb::usb_needs_reattach;
use crate::dev::usb::usb_quirks::UQ_SPUR_BUT_UP;
use crate::dev::usb::usb_subr::usbd_delay_ms;
use crate::dev::usb::usbdevs::{
    USB_PRODUCT_APPLE_BLUETOOTH_HCI, USB_PRODUCT_APPLE_FOUNTAIN_ISO, USB_PRODUCT_APPLE_GEYSER_ISO,
    USB_PRODUCT_APPLE_GEYSER3_ISO, USB_PRODUCT_APPLE_WELLSPRING_ANSI,
    USB_PRODUCT_APPLE_WELLSPRING_ISO, USB_PRODUCT_APPLE_WELLSPRING_JIS,
    USB_PRODUCT_APPLE_WELLSPRING4_ANSI, USB_PRODUCT_APPLE_WELLSPRING4_ISO,
    USB_PRODUCT_APPLE_WELLSPRING4_JIS, USB_PRODUCT_APPLE_WELLSPRING4A_ANSI,
    USB_PRODUCT_APPLE_WELLSPRING4A_ISO, USB_PRODUCT_APPLE_WELLSPRING4A_JIS,
    USB_PRODUCT_APPLE_WELLSPRING6_ISO, USB_PRODUCT_APPLE_WELLSPRING8_ISO,
    USB_PRODUCT_MICRODIA_TEMPER, USB_PRODUCT_MICRODIA_TEMPERHUM, USB_PRODUCT_PCSENSORS_TEMPER,
    USB_PRODUCT_RDING_TEMPER, USB_PRODUCT_TOPRE_HHKB, USB_PRODUCT_WCH2_TEMPER, USB_VENDOR_APPLE,
    USB_VENDOR_MICRODIA, USB_VENDOR_PCSENSORS, USB_VENDOR_RDING, USB_VENDOR_TOPRE, USB_VENDOR_WCH2,
    USB_VENDOR_YUBICO,
};
use crate::dev::usb::usbdi::{
    UMATCH_IFACECLASS, UMATCH_NONE, UsbDevno, splusb, usb_lookup, usbd_dopoll,
    usbd_get_interface_descriptor, usbd_get_quirks, usbd_is_dying, usbd_set_polling,
};
use crate::dev::usb::usbdi_util::{usbd_get_hid_descriptor, usbd_set_idle};
use crate::dev::usb::usbhid::UHID_OUTPUT_REPORT;
use crate::dev::wscons::wsconsio::{
    WSKBD_LED_CAPS, WSKBD_LED_COMPOSE, WSKBD_LED_NUM, WSKBD_LED_SCROLL, WSKBD_TYPE_USB,
    WSKBDIO_GTYPE, WSKBDIO_SETLEDS,
};
use crate::dev::wscons::wskbd::wskbd_cnattach;
use crate::dev::wscons::wskbdvar::{WskbdAccessops, WskbdConsops};
use crate::dev::wscons::wsksymdef::{
    KB_BE, KB_CF, KB_DE, KB_DEFAULT, KB_DK, KB_ES, KB_FR, KB_HU, KB_IT, KB_JP, KB_LA, KB_NO, KB_PL,
    KB_PT, KB_RU, KB_SF, KB_SG, KB_SV, KB_TR, KB_UK, KB_US,
};
use crate::dev::wscons::wsksymvar::KbdT;
use crate::kern::kern_timeout::{timeout_add, timeout_set};
use crate::kprintf;
use crate::machine::db_machdep::db_enter;
use crate::machine::intr::splx;
use crate::sys::device::{CfMatch, Cfattach, Cfdriver, DV_DULL, Device, Softc};
use crate::sys::errno::Errno;
use crate::sys::ioctl::{ioctl_arg, ioctl_ret};
use crate::sys::proc::Proc;
use crate::sys::systm::COLD;
use crate::sys::timeout::Timeout;

/// `(kbd_t)-1`: no layout of this country code.
const KB_NONE: KbdT = KbdT::MAX;

/// `ukbd_countrylayout[1 + HCC_MAX]`: the layout of each HID country code.
pub static UKBD_COUNTRYLAYOUT: [KbdT; 1 + HCC_MAX as usize] = [
    KB_NONE, KB_NONE, // arabic
    KB_BE,   // belgian
    KB_NONE, // canadian bilingual
    KB_CF,   // canadian french
    KB_NONE, // czech
    KB_DK,   // danish
    KB_NONE, // finnish
    KB_FR,   // french
    KB_DE,   // german
    KB_NONE, // greek
    KB_NONE, // hebrew
    KB_HU,   // hungary
    KB_NONE, // international (iso)
    KB_IT,   // italian
    KB_JP,   // japanese (katakana)
    KB_NONE, // korean
    KB_LA,   // latin american
    KB_NONE, // netherlands/dutch
    KB_NO,   // norwegian
    KB_NONE, // persian (farsi)
    KB_PL,   // polish
    KB_PT,   // portuguese
    KB_RU,   // russian
    KB_NONE, // slovakia
    KB_ES,   // spanish
    KB_SV,   // swedish
    KB_SF,   // swiss french
    KB_SG,   // swiss german
    KB_NONE, // switzerland
    KB_NONE, // taiwan
    KB_TR,   // turkish Q
    KB_UK,   // uk
    KB_US,   // us
    KB_NONE, // yugoslavia
    KB_NONE, // turkish F
];

/// `struct ukbd_softc`.
#[repr(C)]
pub struct UkbdSoftc {
    /// `sc_hdev`: the `uhidev` child head; `sc_ledsize` is its `sc_osize`.
    pub sc_hdev: Uhidev,

    /// `sc_kbd`: the bus-independent keyboard.
    pub sc_kbd: Hidkbd,
    /// `sc_spl`: the level `ukbd_cnpollc` raised from.
    pub sc_spl: Cell<i32>,

    /// `sc_ddb`: for entering DDB.
    pub sc_ddb: Timeout,
}

// SAFETY: `#[repr(C)]`, the `Uhidev` (whose first member is the `Device`) first; `Hidkbd` is
// valid as all-zero (its `new()`), the `Cell` an integer and the `Timeout` zeroed.
unsafe impl Softc for UkbdSoftc {}

impl UkbdSoftc {
    /// `sc_ledsize`: the output report size.
    fn sc_ledsize(&self) -> i32 {
        self.sc_hdev.sc_osize.get()
    }
}

/// `ukbd_consops`: the console keyboard functions.
pub static UKBD_CONSOPS: WskbdConsops = WskbdConsops {
    getc: ukbd_cngetc,
    pollc: ukbd_cnpollc,
    bell: ukbd_cnbell,
    debugger: Some(ukbd_debugger),
};

/// `ukbd_accessops`: the keyboard access functions.
pub static UKBD_ACCESSOPS: WskbdAccessops = WskbdAccessops {
    enable: ukbd_enable,
    set_leds: ukbd_set_leds,
    ioctl: ukbd_ioctl,
};

/// `ukbd_cd`.
pub static UKBD_CD: Cfdriver = Cfdriver::new(b"ukbd", DV_DULL, 0);

/// `ukbd_ca`.
pub static UKBD_CA: Cfattach = Cfattach {
    ca_devsize: size_of::<UkbdSoftc>(),
    ca_match: Some(ukbd_match),
    ca_attach: ukbd_attach,
    ca_detach: Some(ukbd_detach),
    ca_activate: None,
};

/// `ukbd_never_console`: devices that must not claim the console.
static UKBD_NEVER_CONSOLE: [UsbDevno; 6] = [
    // Apple HID-proxy is always detected before any real USB keyboard
    UsbDevno {
        ud_vendor: USB_VENDOR_APPLE,
        ud_product: USB_PRODUCT_APPLE_BLUETOOTH_HCI,
    },
    // ugold(4) devices, which also present themselves as ukbd
    UsbDevno {
        ud_vendor: USB_VENDOR_MICRODIA,
        ud_product: USB_PRODUCT_MICRODIA_TEMPER,
    },
    UsbDevno {
        ud_vendor: USB_VENDOR_MICRODIA,
        ud_product: USB_PRODUCT_MICRODIA_TEMPERHUM,
    },
    UsbDevno {
        ud_vendor: USB_VENDOR_PCSENSORS,
        ud_product: USB_PRODUCT_PCSENSORS_TEMPER,
    },
    UsbDevno {
        ud_vendor: USB_VENDOR_RDING,
        ud_product: USB_PRODUCT_RDING_TEMPER,
    },
    UsbDevno {
        ud_vendor: USB_VENDOR_WCH2,
        ud_product: USB_PRODUCT_WCH2_TEMPER,
    },
];

/// `(struct ukbd_softc *)self`.
fn ukbd_softc(self_: &Device) -> &'static UkbdSoftc {
    // SAFETY: only called with devices `ukbd_ca` made (ukbd's own entry points); an attached
    // device lives until `config_detach` frees it after `ukbd_detach`.
    unsafe { &*ptr::from_ref(self_.softc::<UkbdSoftc>()) }
}

/// The softc behind a `wskbd` access or console cookie: the address of the ukbd device.
fn ukbd_cookie(v: *mut c_void) -> &'static UkbdSoftc {
    // SAFETY: `ukbd_attach` and `hidkbd_attach_wskbd` hand the address of the ukbd device
    // (the head of its softc) as the cookie; the device outlives the `wskbd` child and the
    // console registration (`ukbd_detach` detaches the child first).
    unsafe { &*v.cast::<UkbdSoftc>() }
}

/// `ukbd_match`: a report ID that is a keyboard collection.
pub fn ukbd_match(_parent: Option<&Device>, _match: &CfMatch, aux: *mut c_void) -> i32 {
    // SAFETY: `uhidev_attach` passes a `UhidevAttachArg` that lives across the
    // `config_found_sm` calling this.
    let uha = unsafe { &*aux.cast::<UhidevAttachArg<'_>>() };

    // Most Yubikey have OTP enabled by default, and the feature is difficult to disable.
    // Policy decision: Don't attach as a keyboard.
    if uha.uaa.vendor == i32::from(USB_VENDOR_YUBICO) {
        return UMATCH_NONE;
    }

    if uha.claim_multiple_reportid() {
        return UMATCH_NONE;
    }

    let desc = uhidev_get_report_desc(uha.parent);
    if !hid_is_collection(
        desc,
        uha.reportid,
        hid_usage2(HUP_GENERIC_DESKTOP, HUG_KEYBOARD) as i32,
    ) {
        return UMATCH_NONE;
    }

    UMATCH_IFACECLASS
}

/// `ukbd_attach`.
pub fn ukbd_attach(_parent: Option<&Device>, self_: &Device, aux: *mut c_void) {
    let sc = ukbd_softc(self_);
    let kbd = &sc.sc_kbd;
    // SAFETY: as in `ukbd_match`, valid during the attach.
    let uha = unsafe { &*aux.cast::<UhidevAttachArg<'_>>() };
    let uaa = uha.uaa;
    let mut qflags: u32 = 0;
    let mut console = 1;
    let mut layout: KbdT = KB_NONE;

    sc.sc_hdev.sc_intr.set(Some(ukbd_intr));
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

    // Do not allow unwanted devices to claim the console.
    if usb_lookup(&UKBD_NEVER_CONSOLE, uaa.vendor as u16, uaa.product as u16).is_some() {
        console = 0;
    }

    let quirks = usbd_get_quirks(uaa.device()).uq_flags;
    if quirks & UQ_SPUR_BUT_UP != 0 {
        qflags |= HIDKBD_SPUR_BUT_UP;
    }

    if hidkbd_attach(self_, kbd, console, qflags, i32::from(repid), desc).is_err() {
        return;
    }

    if uaa.vendor == i32::from(USB_VENDOR_APPLE) {
        let mut loc = HidLocation::default();
        let mut flags: u32 = 0;
        let found = hid_locate(
            desc,
            hid_usage2(HUP_APPLE, HUG_FN_KEY),
            uha.reportid,
            hid_input,
            Some(&mut loc),
            Some(&mut flags),
        );
        kbd.sc_fn.set(loc);
        if found && flags & HIO_VARIABLE != 0 {
            let product = uaa.product;
            let is = |ids: &[u16]| ids.iter().any(|&p| i32::from(p) == product);
            if is(&[
                USB_PRODUCT_APPLE_FOUNTAIN_ISO,
                USB_PRODUCT_APPLE_GEYSER_ISO,
                USB_PRODUCT_APPLE_GEYSER3_ISO,
                USB_PRODUCT_APPLE_WELLSPRING6_ISO,
                USB_PRODUCT_APPLE_WELLSPRING8_ISO,
            ]) {
                kbd.sc_munge.set(Some(hidkbd_apple_iso_munge));
            } else if is(&[
                USB_PRODUCT_APPLE_WELLSPRING_ISO,
                USB_PRODUCT_APPLE_WELLSPRING4_ISO,
                USB_PRODUCT_APPLE_WELLSPRING4A_ISO,
            ]) {
                kbd.sc_munge.set(Some(hidkbd_apple_iso_mba_munge));
            } else if is(&[
                USB_PRODUCT_APPLE_WELLSPRING_ANSI,
                USB_PRODUCT_APPLE_WELLSPRING_JIS,
                USB_PRODUCT_APPLE_WELLSPRING4_ANSI,
                USB_PRODUCT_APPLE_WELLSPRING4_JIS,
                USB_PRODUCT_APPLE_WELLSPRING4A_ANSI,
                USB_PRODUCT_APPLE_WELLSPRING4A_JIS,
            ]) {
                kbd.sc_munge.set(Some(hidkbd_apple_mba_munge));
            } else {
                kbd.sc_munge.set(Some(hidkbd_apple_munge));
            }
        }
    }

    if uaa.vendor == i32::from(USB_VENDOR_TOPRE) && uaa.product == i32::from(USB_PRODUCT_TOPRE_HHKB)
    {
        // ignore country code on purpose
    } else if let Some(iface) = uaa.iface
        && let Some(id) = usbd_get_interface_descriptor(iface)
        && let Some(hid) = usbd_get_hid_descriptor(uaa.device(), id)
    {
        if u32::from(hid.bCountryCode) <= HCC_MAX {
            layout = UKBD_COUNTRYLAYOUT[usize::from(hid.bCountryCode)];
        }
        #[cfg(feature = "diagnostic")]
        if hid.bCountryCode != 0 {
            kprintf!(", country code {}", hid.bCountryCode);
        }
    }
    if layout == KB_NONE {
        layout = KB_US | KB_DEFAULT;
    }

    kprintf!("\n");

    let cookie = ptr::from_ref(sc).cast_mut().cast::<c_void>();
    if kbd.sc_console_keyboard.get() != 0 {
        UKBD_KEYMAPDATA.set_layout(layout);
        wskbd_cnattach(&UKBD_CONSOPS, cookie, &UKBD_KEYMAPDATA);
        ukbd_enable(cookie, 1);
    }

    // Flash the leds; no real purpose, just shows we're alive.
    ukbd_set_leds(
        cookie,
        WSKBD_LED_SCROLL | WSKBD_LED_NUM | WSKBD_LED_CAPS | WSKBD_LED_COMPOSE,
    );
    usbd_delay_ms(sc.sc_hdev.sc_udev.get().unwrap_or(uaa.device()), 400);
    ukbd_set_leds(cookie, 0);

    hidkbd_attach_wskbd(kbd, layout, &UKBD_ACCESSOPS);

    timeout_set(&sc.sc_ddb, ukbd_db_enter, cookie);
}

/// `ukbd_detach`.
pub fn ukbd_detach(self_: &Device, flags: i32) -> Result<(), Errno> {
    let sc = ukbd_softc(self_);
    let kbd = &sc.sc_kbd;

    let rv = hidkbd_detach(kbd, flags);

    // The console keyboard does not get a disable call, so check pipe.
    if sc.sc_hdev.sc_state.get() & UHIDEV_OPEN != 0 {
        uhidev_close(&sc.sc_hdev);
    }

    rv
}

/// `ukbd_intr`: a report from the keyboard's interrupt pipe (without its report ID).
pub fn ukbd_intr(addr: &Uhidev, ibuf: &mut [u8]) {
    // SAFETY: `ukbd_attach` set `sc_intr` to this function in the `sc_hdev` of a `UkbdSoftc`,
    // which `uhidev_intr` hands back: the head of the softc, which has `UkbdSoftc`'s layout.
    let sc = unsafe { &*ptr::from_ref(addr).cast::<UkbdSoftc>() };
    let kbd = &sc.sc_kbd;

    if kbd.sc_enabled.get() != 0 {
        hidkbd_input(kbd, ibuf);
    }
}

/// `ukbd_enable`: the `wskbd` access operation: open the interrupt pipe (or close it);
/// returns the errno.
pub fn ukbd_enable(v: *mut c_void, on: i32) -> i32 {
    let sc = ukbd_cookie(v);
    let kbd = &sc.sc_kbd;

    let Some(udev) = sc.sc_hdev.sc_udev.get() else {
        return Errno::EIO as i32;
    };
    if on != 0 && usbd_is_dying(udev) {
        return Errno::EIO as i32;
    }

    if let Err(rv) = hidkbd_enable(kbd, on) {
        return rv as i32;
    }

    if on != 0 {
        match uhidev_open(&sc.sc_hdev) {
            Ok(()) => 0,
            Err(e) => e as i32,
        }
    } else {
        uhidev_close(&sc.sc_hdev);
        0
    }
}

/// `ukbd_set_leds`: the `wskbd` access operation: light the LEDs `leds`.
pub fn ukbd_set_leds(v: *mut c_void, leds: i32) {
    let sc = ukbd_cookie(v);
    let kbd = &sc.sc_kbd;
    let mut res: u8 = 0;

    let Some(udev) = sc.sc_hdev.sc_udev.get() else {
        return;
    };
    if usbd_is_dying(udev) {
        return;
    }

    if sc.sc_ledsize() != 0
        && hidkbd_set_leds(kbd, leds, &mut res)
        && let Some(parent) = sc.sc_hdev.sc_parent.get()
    {
        let _ = uhidev_set_report_async(
            parent,
            i32::from(UHID_OUTPUT_REPORT),
            i32::from(sc.sc_hdev.sc_report_id.get()),
            &[res],
        );
    }
}

/// `ukbd_ioctl`: the `wskbd` access operation for the ioctls: the keyboard type and LEDs,
/// then `uhidev`'s and `hidkbd`'s. `Ok(false)` where the C returns `-1`.
pub fn ukbd_ioctl(
    v: *mut c_void,
    cmd: u64,
    data: &mut [u8],
    flag: i32,
    p: Option<&Proc>,
) -> Result<bool, Errno> {
    let sc = ukbd_cookie(v);
    let kbd = &sc.sc_kbd;

    match cmd {
        WSKBDIO_GTYPE => {
            ioctl_ret(data, &(WSKBD_TYPE_USB as i32));
            Ok(true)
        }
        WSKBDIO_SETLEDS => {
            ukbd_set_leds(v, ioctl_arg::<i32>(data));
            Ok(true)
        }
        _ => {
            if uhidev_ioctl(&sc.sc_hdev, cmd, data, flag, p)? {
                Ok(true)
            } else {
                hidkbd_ioctl(kbd, cmd, data, flag, p)
            }
        }
    }
}

/// `ukbd_cngetc`: the console keyboard's next event, polling the bus until one is decoded.
pub fn ukbd_cngetc(v: *mut c_void, type_: &mut u32, data: &mut i32) {
    let sc = ukbd_cookie(v);
    let kbd = &sc.sc_kbd;

    kbd.sc_polling.set(1);
    while kbd.sc_npollchar.get() <= 0 {
        if let Some(udev) = sc.sc_hdev.sc_udev.get() {
            usbd_dopoll(udev);
        }
    }
    kbd.sc_polling.set(0);
    hidkbd_cngetc(kbd, type_, data);
}

/// `ukbd_cnpollc`: switch the console keyboard's polling on or off.
pub fn ukbd_cnpollc(v: *mut c_void, on: i32) {
    let sc = ukbd_cookie(v);

    if on != 0 {
        sc.sc_spl.set(splusb());
    } else {
        splx(sc.sc_spl.get());
    }
    if let Some(udev) = sc.sc_hdev.sc_udev.get() {
        usbd_set_polling(udev, on != 0);
    }
}

/// `ukbd_cnbell`: the console bell.
pub fn ukbd_cnbell(_v: *mut c_void, pitch: u32, period: u32, volume: u32) {
    hidkbd_bell(pitch, period, volume, 1);
}

/// `ukbd_debugger`: CTL-ALT-ESC on the console keyboard.
pub fn ukbd_debugger(v: *mut c_void) {
    let sc = ukbd_cookie(v);

    // For the console keyboard we can't deliver CTL-ALT-ESC from the interrupt routine.
    // Doing so would start polling from inside the interrupt routine and that loses
    // bigtime.
    timeout_add(&sc.sc_ddb, 1);
}

/// `ukbd_db_enter`: the `sc_ddb` timeout: enter the debugger.
pub fn ukbd_db_enter(_xsc: *mut c_void) {
    db_enter();
}

/// `ukbd_cnattach`: the console framework picked a USB keyboard: the next one to attach is
/// the console keyboard; after boot, make the attached ones attach again.
pub fn ukbd_cnattach() -> i32 {
    // XXX USB requires too many parts of the kernel to be running in order to work, so we
    // can't do much for the console keyboard until autoconfiguration has run its course.
    hidkbd_is_console.store(1, Ordering::Relaxed);

    if !COLD.load(Ordering::Relaxed) {
        // When switching console dynamically force all USB keyboards to re-attach and
        // possibly became the 'console' keyboard.
        for i in 0..UKBD_CD.cd_ndevs.get() {
            if let Some(dev) = UKBD_CD.cd_dev(i) {
                // SAFETY: an attached ukbd, in `cd_devs` until `config_detach`.
                let sc = ukbd_softc(unsafe { dev.as_ref() });
                if let Some(udev) = sc.sc_hdev.sc_udev.get() {
                    usb_needs_reattach(udev);
                }
                break;
            }
        }
    }

    0
}

#[cfg(test)]
mod tests;
