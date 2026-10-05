/*	$OpenBSD: usb_quirks.h,v 1.18 2021/03/24 02:49:57 jcs Exp $ */
/*	$NetBSD: usb_quirks.h,v 1.20 2001/04/15 09:38:01 augustss Exp $	*/
/*	$FreeBSD: src/sys/dev/usb/usb_quirks.h,v 1.9 1999/11/12 23:31:03 n_hibma Exp $	*/
/*	$OpenBSD: usb_quirks.c,v 1.81 2025/03/27 14:12:38 sthen Exp $ */
/*	$NetBSD: usb_quirks.c,v 1.45 2003/05/10 17:47:14 hamajima Exp $	*/
/*	$FreeBSD: src/sys/dev/usb/usb_quirks.c,v 1.30 2003/01/02 04:15:55 imp Exp $	*/
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

//! Per-device workarounds: `<dev/usb/usb_quirks.h>` and `dev/usb/usb_quirks.c`.
//!
//! Upstream: sys/dev/usb/usb_quirks.h @ 3ce1f3f79392, sys/dev/usb/usb_quirks.c @ 3ce1f3f79392
//!
//! `usbd_find_quirk` looks a device descriptor up in the vendor/product/revision table, then
//! (empty in OpenBSD today) the class table, and returns the device's quirks, or
//! `usbd_no_quirk`.
//!
//! ## Deviations
//! - The header and the file share this module.
//! - The tables are slices without the C's terminating `{ 0 }` entry; the search stops at the
//!   end of the slice, as the C stops at the terminator.
//! - `USB_DEBUG` is not in GENERIC: the debug `printf`s are absent.

use crate::dev::usb::usb::{UsbDeviceDescriptor, ugetw};
use crate::dev::usb::usbdevs::*;

/// `struct usbd_quirks`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct UsbdQuirks {
    /// `uq_flags`: device problems, `UQ_*`.
    pub uq_flags: u32,
}

/// `UQ_NO_SET_PROTO`: cannot handle SET PROTOCOL.
pub const UQ_NO_SET_PROTO: u32 = 0x00000001;
/// `UQ_SWAP_UNICODE`: some Unicode strings swapped.
pub const UQ_SWAP_UNICODE: u32 = 0x00000002;
/// `UQ_MS_REVZ`: mouse has Z-axis reversed.
pub const UQ_MS_REVZ: u32 = 0x00000004;
/// `UQ_NO_STRINGS`: string descriptors are broken.
pub const UQ_NO_STRINGS: u32 = 0x00000008;
/// `UQ_BUS_POWERED`: is bus-powered, despite claim.
pub const UQ_BUS_POWERED: u32 = 0x00000020;
/// `UQ_SPUR_BUT_UP`: spurious mouse button up events.
pub const UQ_SPUR_BUT_UP: u32 = 0x00000080;
/// `UQ_POWER_CLAIM`: hub lies about power status.
pub const UQ_POWER_CLAIM: u32 = 0x00000200;
/// `UQ_ASSUME_CM_OVER_DATA`: modem device breaks on cm over data.
pub const UQ_ASSUME_CM_OVER_DATA: u32 = 0x00001000;
/// `UQ_BROKEN_BIDIR`: printer has broken bidir mode.
pub const UQ_BROKEN_BIDIR: u32 = 0x00002000;
/// `UQ_BAD_HID`: device claims uhid, but isn't.
pub const UQ_BAD_HID: u32 = 0x00004000;
/// `UQ_MS_BAD_CLASS`: mouse doesn't identify properly.
pub const UQ_MS_BAD_CLASS: u32 = 0x00008000;
/// `UQ_MS_LEADING_BYTE`: mouse sends unknown leading byte.
pub const UQ_MS_LEADING_BYTE: u32 = 0x00010000;
/// `UQ_EHCI_NEEDTO_DISOWN`: must hand device over to USB 1.1 if attached to EHCI.
pub const UQ_EHCI_NEEDTO_DISOWN: u32 = 0x00020000;
/// `UQ_MS_VENDOR_BUTTONS`: mouse reports extra buttons in vendor page.
pub const UQ_MS_VENDOR_BUTTONS: u32 = 0x00040000;
/// `UQ_ALWAYS_OPEN`: always keep data pipe open.
pub const UQ_ALWAYS_OPEN: u32 = 0x00080000;

/// `ANY`: any revision.
const ANY: u16 = 0xffff;

/// `struct usbd_quirk_entry`.
#[allow(non_snake_case)] // the C member names
pub struct UsbdQuirkEntry {
    /// `idVendor`.
    pub idVendor: u16,
    /// `idProduct`.
    pub idProduct: u16,
    /// `bcdDevice`: `ANY` matches every revision.
    pub bcdDevice: u16,
    /// `quirks`.
    pub quirks: UsbdQuirks,
}

/// One `usb_quirks[]` row.
#[allow(non_snake_case)] // the C member names
const fn q(idVendor: u16, idProduct: u16, bcdDevice: u16, uq_flags: u32) -> UsbdQuirkEntry {
    UsbdQuirkEntry {
        idVendor,
        idProduct,
        bcdDevice,
        quirks: UsbdQuirks { uq_flags },
    }
}

/// `usb_quirks[]`.
pub static USB_QUIRKS: [UsbdQuirkEntry; 84] = [
    q(
        USB_VENDOR_KYE,
        USB_PRODUCT_KYE_NICHE,
        0x100,
        UQ_NO_SET_PROTO,
    ),
    q(
        USB_VENDOR_INSIDEOUT,
        USB_PRODUCT_INSIDEOUT_EDGEPORT4,
        0x094,
        UQ_SWAP_UNICODE,
    ),
    q(
        USB_VENDOR_QTRONIX,
        USB_PRODUCT_QTRONIX_980N,
        0x110,
        UQ_SPUR_BUT_UP,
    ),
    q(
        USB_VENDOR_ALCOR2,
        USB_PRODUCT_ALCOR2_KBD_HUB,
        0x001,
        UQ_SPUR_BUT_UP,
    ),
    q(
        USB_VENDOR_MCT,
        USB_PRODUCT_MCT_HUB0100,
        0x102,
        UQ_BUS_POWERED,
    ),
    q(
        USB_VENDOR_MCT,
        USB_PRODUCT_MCT_USB232,
        0x102,
        UQ_BUS_POWERED,
    ),
    q(
        USB_VENDOR_METRICOM,
        USB_PRODUCT_METRICOM_RICOCHET_GS,
        0x100,
        UQ_ASSUME_CM_OVER_DATA,
    ),
    q(
        USB_VENDOR_SANYO,
        USB_PRODUCT_SANYO_SCP4900,
        0x000,
        UQ_ASSUME_CM_OVER_DATA,
    ),
    q(
        USB_VENDOR_MOTOROLA2,
        USB_PRODUCT_MOTOROLA2_T720C,
        0x001,
        UQ_ASSUME_CM_OVER_DATA,
    ),
    q(
        USB_VENDOR_EICON,
        USB_PRODUCT_EICON_DIVA852,
        0x100,
        UQ_ASSUME_CM_OVER_DATA,
    ),
    // YAMAHA router's ucdDevice is the version of firmware and often changes.
    q(
        USB_VENDOR_YAMAHA,
        USB_PRODUCT_YAMAHA_RTA54I,
        ANY,
        UQ_ASSUME_CM_OVER_DATA,
    ),
    q(
        USB_VENDOR_YAMAHA,
        USB_PRODUCT_YAMAHA_RTA55I,
        ANY,
        UQ_ASSUME_CM_OVER_DATA,
    ),
    q(
        USB_VENDOR_YAMAHA,
        USB_PRODUCT_YAMAHA_RTW65B,
        ANY,
        UQ_ASSUME_CM_OVER_DATA,
    ),
    q(
        USB_VENDOR_YAMAHA,
        USB_PRODUCT_YAMAHA_RTW65I,
        ANY,
        UQ_ASSUME_CM_OVER_DATA,
    ),
    q(
        USB_VENDOR_QUALCOMM,
        USB_PRODUCT_QUALCOMM_MSM_MODEM,
        ANY,
        UQ_ASSUME_CM_OVER_DATA,
    ),
    q(
        USB_VENDOR_QUALCOMM2,
        USB_PRODUCT_QUALCOMM2_MSM_PHONE,
        ANY,
        UQ_ASSUME_CM_OVER_DATA,
    ),
    q(
        USB_VENDOR_SUNTAC,
        USB_PRODUCT_SUNTAC_AS64LX,
        0x100,
        UQ_ASSUME_CM_OVER_DATA,
    ),
    q(
        USB_VENDOR_CMOTECH,
        USB_PRODUCT_CMOTECH_CM5100P,
        ANY,
        UQ_ASSUME_CM_OVER_DATA,
    ),
    q(
        USB_VENDOR_CMOTECH,
        USB_PRODUCT_CMOTECH_CCU550,
        ANY,
        UQ_ASSUME_CM_OVER_DATA,
    ),
    q(
        USB_VENDOR_CMOTECH,
        USB_PRODUCT_CMOTECH_CNU550PRO,
        ANY,
        UQ_ASSUME_CM_OVER_DATA,
    ),
    q(
        USB_VENDOR_SIEMENS2,
        USB_PRODUCT_SIEMENS2_ES75,
        ANY,
        UQ_ASSUME_CM_OVER_DATA,
    ),
    q(USB_VENDOR_TI, USB_PRODUCT_TI_UTUSB41, 0x110, UQ_POWER_CLAIM),
    // XXX These should have a revision number, but I don't know what they are.
    q(USB_VENDOR_HP, USB_PRODUCT_HP_895C, ANY, UQ_BROKEN_BIDIR),
    q(USB_VENDOR_HP, USB_PRODUCT_HP_880C, ANY, UQ_BROKEN_BIDIR),
    q(USB_VENDOR_HP, USB_PRODUCT_HP_815C, ANY, UQ_BROKEN_BIDIR),
    q(USB_VENDOR_HP, USB_PRODUCT_HP_810C, ANY, UQ_BROKEN_BIDIR),
    q(USB_VENDOR_HP, USB_PRODUCT_HP_830C, ANY, UQ_BROKEN_BIDIR),
    q(USB_VENDOR_HP, USB_PRODUCT_HP_885C, ANY, UQ_BROKEN_BIDIR),
    q(USB_VENDOR_HP, USB_PRODUCT_HP_840C, ANY, UQ_BROKEN_BIDIR),
    q(USB_VENDOR_HP, USB_PRODUCT_HP_816C, ANY, UQ_BROKEN_BIDIR),
    q(USB_VENDOR_HP, USB_PRODUCT_HP_959C, ANY, UQ_BROKEN_BIDIR),
    q(USB_VENDOR_HP, USB_PRODUCT_HP_1220C, ANY, UQ_BROKEN_BIDIR),
    q(
        USB_VENDOR_NEC,
        USB_PRODUCT_NEC_PICTY900,
        ANY,
        UQ_BROKEN_BIDIR,
    ),
    q(
        USB_VENDOR_NEC,
        USB_PRODUCT_NEC_PICTY760,
        ANY,
        UQ_BROKEN_BIDIR,
    ),
    q(
        USB_VENDOR_NEC,
        USB_PRODUCT_NEC_PICTY920,
        ANY,
        UQ_BROKEN_BIDIR,
    ),
    q(
        USB_VENDOR_NEC,
        USB_PRODUCT_NEC_PICTY800,
        ANY,
        UQ_BROKEN_BIDIR,
    ),
    q(USB_VENDOR_APPLE, USB_PRODUCT_APPLE_IPHONE, ANY, UQ_BAD_HID),
    q(
        USB_VENDOR_APPLE,
        USB_PRODUCT_APPLE_IPHONE_3G,
        ANY,
        UQ_BAD_HID,
    ),
    q(
        USB_VENDOR_APPLE,
        USB_PRODUCT_APPLE_IPHONE_3GS,
        ANY,
        UQ_BAD_HID,
    ),
    q(
        USB_VENDOR_APPLE,
        USB_PRODUCT_APPLE_IPHONE_4_CDMA,
        ANY,
        UQ_BAD_HID,
    ),
    q(
        USB_VENDOR_APPLE,
        USB_PRODUCT_APPLE_IPHONE_4_GSM,
        ANY,
        UQ_BAD_HID,
    ),
    q(
        USB_VENDOR_APPLE,
        USB_PRODUCT_APPLE_IPHONE_4S,
        ANY,
        UQ_BAD_HID,
    ),
    q(
        USB_VENDOR_APPLE,
        USB_PRODUCT_APPLE_IPHONE_6,
        ANY,
        UQ_BAD_HID,
    ),
    q(
        USB_VENDOR_APPLE,
        USB_PRODUCT_APPLE_IPOD_TOUCH,
        ANY,
        UQ_BAD_HID,
    ),
    q(
        USB_VENDOR_APPLE,
        USB_PRODUCT_APPLE_IPOD_TOUCH_2G,
        ANY,
        UQ_BAD_HID,
    ),
    q(
        USB_VENDOR_APPLE,
        USB_PRODUCT_APPLE_IPOD_TOUCH_3G,
        ANY,
        UQ_BAD_HID,
    ),
    q(
        USB_VENDOR_APPLE,
        USB_PRODUCT_APPLE_IPOD_TOUCH_4G,
        ANY,
        UQ_BAD_HID,
    ),
    q(USB_VENDOR_APPLE, USB_PRODUCT_APPLE_IPAD, ANY, UQ_BAD_HID),
    q(USB_VENDOR_APPLE, USB_PRODUCT_APPLE_IPAD2, ANY, UQ_BAD_HID),
    q(
        USB_VENDOR_APPLE,
        USB_PRODUCT_APPLE_SPEAKERS,
        ANY,
        UQ_BAD_HID,
    ),
    q(
        USB_VENDOR_CYPRESS,
        USB_PRODUCT_CYPRESS_SISPM_OLD,
        ANY,
        UQ_BAD_HID,
    ),
    q(
        USB_VENDOR_CYPRESS,
        USB_PRODUCT_CYPRESS_SISPM,
        ANY,
        UQ_BAD_HID,
    ),
    q(
        USB_VENDOR_CYPRESS,
        USB_PRODUCT_CYPRESS_SISPM_FLASH,
        ANY,
        UQ_BAD_HID,
    ),
    q(
        USB_VENDOR_MICROCHIP,
        USB_PRODUCT_MICROCHIP_USBLCD20X2,
        ANY,
        UQ_BAD_HID,
    ),
    q(
        USB_VENDOR_MICROCHIP,
        USB_PRODUCT_MICROCHIP_USBLCD256X64,
        ANY,
        UQ_BAD_HID,
    ),
    q(
        USB_VENDOR_MECANIQUE,
        USB_PRODUCT_MECANIQUE_WISPY,
        ANY,
        UQ_BAD_HID,
    ),
    q(
        USB_VENDOR_METAGEEK,
        USB_PRODUCT_METAGEEK_WISPY24I,
        ANY,
        UQ_BAD_HID,
    ),
    q(
        USB_VENDOR_MUSTEK2,
        USB_PRODUCT_MUSTEK2_PM800,
        ANY,
        UQ_BAD_HID,
    ),
    q(USB_VENDOR_OMRON, USB_PRODUCT_OMRON_BX35F, ANY, UQ_BAD_HID),
    q(USB_VENDOR_OMRON, USB_PRODUCT_OMRON_BX50F, ANY, UQ_BAD_HID),
    q(USB_VENDOR_OMRON, USB_PRODUCT_OMRON_BY35S, ANY, UQ_BAD_HID),
    q(USB_VENDOR_TENX, USB_PRODUCT_TENX_MISSILE, ANY, UQ_BAD_HID),
    q(
        USB_VENDOR_TERRATEC,
        USB_PRODUCT_TERRATEC_AUREON,
        ANY,
        UQ_BAD_HID,
    ),
    q(USB_VENDOR_TI, USB_PRODUCT_TI_MSP430, ANY, UQ_BAD_HID),
    q(
        USB_VENDOR_VELLEMAN,
        USB_PRODUCT_VELLEMAN_K8055,
        ANY,
        UQ_BAD_HID,
    ),
    q(
        USB_VENDOR_DREAMLINK,
        USB_PRODUCT_DREAMLINK_ULMB1,
        ANY,
        UQ_BAD_HID,
    ),
    q(
        USB_VENDOR_HUAWEI,
        USB_PRODUCT_HUAWEI_E220,
        ANY,
        UQ_NO_STRINGS,
    ),
    q(
        USB_VENDOR_SHANTOU,
        USB_PRODUCT_SHANTOU_DM9601,
        ANY,
        UQ_NO_STRINGS,
    ),
    q(
        USB_VENDOR_RALINK,
        USB_PRODUCT_RALINK_RT2573,
        ANY,
        UQ_NO_STRINGS,
    ),
    // MS keyboards do weird things
    q(
        USB_VENDOR_MICROSOFT,
        USB_PRODUCT_MICROSOFT_WLNOTEBOOK,
        ANY,
        UQ_MS_BAD_CLASS | UQ_MS_LEADING_BYTE,
    ),
    q(
        USB_VENDOR_MICROSOFT,
        USB_PRODUCT_MICROSOFT_WLNOTEBOOK2,
        ANY,
        UQ_MS_BAD_CLASS | UQ_MS_LEADING_BYTE,
    ),
    q(
        USB_VENDOR_KENSINGTON,
        USB_PRODUCT_KENSINGTON_SLIMBLADE,
        ANY,
        UQ_MS_VENDOR_BUTTONS,
    ),
    // Devices that need their data pipe held open
    q(
        USB_VENDOR_CHERRY,
        USB_PRODUCT_CHERRY_MOUSE1,
        ANY,
        UQ_ALWAYS_OPEN,
    ),
    q(
        USB_VENDOR_CHICONY,
        USB_PRODUCT_CHICONY_OPTMOUSE,
        ANY,
        UQ_ALWAYS_OPEN,
    ),
    q(
        USB_VENDOR_DELL,
        USB_PRODUCT_DELL_PIXARTMOUSE,
        ANY,
        UQ_ALWAYS_OPEN,
    ),
    q(
        USB_VENDOR_HAILUCK,
        USB_PRODUCT_HAILUCK_KEYBOARD,
        ANY,
        UQ_ALWAYS_OPEN,
    ),
    q(
        USB_VENDOR_LENOVO,
        USB_PRODUCT_LENOVO_PIXARTMOUSE,
        ANY,
        UQ_ALWAYS_OPEN,
    ),
    q(
        USB_VENDOR_LENOVO,
        USB_PRODUCT_LENOVO_OPTUSBMOUSE,
        ANY,
        UQ_ALWAYS_OPEN,
    ),
    q(
        USB_VENDOR_LOGITECH,
        USB_PRODUCT_LOGITECH_B100_1,
        ANY,
        UQ_ALWAYS_OPEN,
    ),
    q(
        USB_VENDOR_LOGITECH,
        USB_PRODUCT_LOGITECH_B100_2,
        ANY,
        UQ_ALWAYS_OPEN,
    ),
    q(
        USB_VENDOR_MICROSOFT,
        USB_PRODUCT_MICROSOFT_PIXARTMOUSE,
        ANY,
        UQ_ALWAYS_OPEN,
    ),
    q(
        USB_VENDOR_MICROSOFT,
        USB_PRODUCT_MICROSOFT_TYPECOVER,
        ANY,
        UQ_ALWAYS_OPEN,
    ),
    q(
        USB_VENDOR_MICROSOFT,
        USB_PRODUCT_MICROSOFT_TYPECOVER2,
        ANY,
        UQ_ALWAYS_OPEN,
    ),
    q(
        USB_VENDOR_PIXART,
        USB_PRODUCT_PIXART_RPIMOUSE,
        ANY,
        UQ_ALWAYS_OPEN,
    ),
];

/// `bANY`: any subclass or protocol.
#[allow(non_upper_case_globals)] // the C's name
const bANY: u8 = 0xff;

/// `struct usbd_dev_quirk_entry`.
#[allow(non_snake_case)] // the C member names
pub struct UsbdDevQuirkEntry {
    /// `bDeviceClass`.
    pub bDeviceClass: u8,
    /// `bDeviceSubClass`: `bANY` matches all.
    pub bDeviceSubClass: u8,
    /// `bDeviceProtocol`: `bANY` matches all.
    pub bDeviceProtocol: u8,
    /// `quirks`.
    pub quirks: UsbdQuirks,
}

/// `usb_dev_quirks[]`: empty (the C's table holds only its terminator).
pub static USB_DEV_QUIRKS: [UsbdDevQuirkEntry; 0] = [];

/// `usbd_no_quirk`.
pub static USBD_NO_QUIRK: UsbdQuirks = UsbdQuirks { uq_flags: 0 };

/// `usbd_find_quirk`: the quirks of the device `d` describes.
pub fn usbd_find_quirk(d: &UsbDeviceDescriptor) -> &'static UsbdQuirks {
    usbd_find_quirk_in(d, &USB_QUIRKS, &USB_DEV_QUIRKS)
}

/// `usbd_find_quirk` over the given tables (the tests use their own class table).
fn usbd_find_quirk_in(
    d: &UsbDeviceDescriptor,
    quirks: &'static [UsbdQuirkEntry],
    dev_quirks: &'static [UsbdDevQuirkEntry],
) -> &'static UsbdQuirks {
    let vendor = ugetw(d.idVendor);
    let product = ugetw(d.idProduct);
    let revision = ugetw(d.bcdDevice);

    // search device specific quirks entry
    for t in quirks.iter().take_while(|t| t.idVendor != 0) {
        if t.idVendor == vendor
            && t.idProduct == product
            && (t.bcdDevice == ANY || t.bcdDevice == revision)
        {
            return &t.quirks;
        }
    }
    // no device specific quirks found, search class specific entry
    for td in dev_quirks.iter().take_while(|td| td.bDeviceClass != 0) {
        if td.bDeviceClass == d.bDeviceClass
            && (td.bDeviceSubClass == bANY || td.bDeviceSubClass == d.bDeviceSubClass)
            && (td.bDeviceProtocol == bANY || td.bDeviceProtocol == d.bDeviceProtocol)
        {
            return &td.quirks;
        }
    }

    &USBD_NO_QUIRK
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::dev::usb::usb::{UsbWire, usetw};

    fn dd(vendor: u16, product: u16, rev: u16, class: u8) -> UsbDeviceDescriptor {
        let mut d = UsbDeviceDescriptor::zeroed();
        usetw(&mut d.idVendor, vendor);
        usetw(&mut d.idProduct, product);
        usetw(&mut d.bcdDevice, rev);
        d.bDeviceClass = class;
        d
    }

    #[test]
    fn device_entries_match_vendor_product_and_revision() {
        let d = dd(USB_VENDOR_KYE, USB_PRODUCT_KYE_NICHE, 0x100, 0);
        assert_eq!(usbd_find_quirk(&d).uq_flags, UQ_NO_SET_PROTO);
        // Another revision has no quirk.
        let d = dd(USB_VENDOR_KYE, USB_PRODUCT_KYE_NICHE, 0x101, 0);
        assert!(core::ptr::eq(usbd_find_quirk(&d), &USBD_NO_QUIRK));
        // ANY matches every revision.
        let d = dd(USB_VENDOR_HUAWEI, USB_PRODUCT_HUAWEI_E220, 0x1234, 0);
        assert_eq!(usbd_find_quirk(&d).uq_flags, UQ_NO_STRINGS);
        let d = dd(USB_VENDOR_MICROSOFT, USB_PRODUCT_MICROSOFT_WLNOTEBOOK, 0, 0);
        assert_eq!(
            usbd_find_quirk(&d).uq_flags,
            UQ_MS_BAD_CLASS | UQ_MS_LEADING_BYTE
        );
    }

    #[test]
    fn class_entries_follow_device_entries() {
        static CLASS: [UsbdDevQuirkEntry; 2] = [
            UsbdDevQuirkEntry {
                bDeviceClass: 0x09,
                bDeviceSubClass: bANY,
                bDeviceProtocol: 2,
                quirks: UsbdQuirks {
                    uq_flags: UQ_POWER_CLAIM,
                },
            },
            UsbdDevQuirkEntry {
                bDeviceClass: 0,
                bDeviceSubClass: 0,
                bDeviceProtocol: 0,
                quirks: UsbdQuirks { uq_flags: 0 },
            },
        ];
        let mut d = dd(0x1d6b, 0x0002, 0, 0x09);
        d.bDeviceSubClass = 7;
        d.bDeviceProtocol = 2;
        assert_eq!(
            usbd_find_quirk_in(&d, &USB_QUIRKS, &CLASS).uq_flags,
            UQ_POWER_CLAIM
        );
        d.bDeviceProtocol = 1;
        assert_eq!(usbd_find_quirk_in(&d, &USB_QUIRKS, &CLASS).uq_flags, 0);
    }

    #[test]
    #[ignore = "needs OPENBSD_SRC (just test-ref)"]
    fn flags_match_the_c_header() {
        let defs = crate::reftest::defines("sys/dev/usb/usb_quirks.h");
        let ours = crate::reftest::assert_defines!(defs;
            UQ_NO_SET_PROTO, UQ_SWAP_UNICODE, UQ_MS_REVZ, UQ_NO_STRINGS, UQ_BUS_POWERED,
            UQ_SPUR_BUT_UP, UQ_POWER_CLAIM, UQ_ASSUME_CM_OVER_DATA, UQ_BROKEN_BIDIR,
            UQ_BAD_HID, UQ_MS_BAD_CLASS, UQ_MS_LEADING_BYTE, UQ_EHCI_NEEDTO_DISOWN,
            UQ_MS_VENDOR_BUTTONS, UQ_ALWAYS_OPEN,
        );
        crate::reftest::assert_complete(&defs, "UQ_", &ours);
    }
}
