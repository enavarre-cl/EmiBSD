/*	$OpenBSD: usbhid.h,v 1.21 2016/01/09 04:10:36 jcs Exp $ */
/*	$NetBSD: usbhid.h,v 1.11 2001/12/28 00:20:24 augustss Exp $	*/
/*	$FreeBSD: src/sys/dev/usb/usbhid.h,v 1.7 1999/11/17 22:33:51 n_hibma Exp $ */
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
//! The USB HID class's requests and descriptor: `<dev/usb/usbhid.h>`.
//!
//! Upstream: sys/dev/usb/usbhid.h @ 3ce1f3f79392
//!
//! ## Deviations
//! - The header includes `<dev/hid/hid.h>` for its users; that header comes with the HID
//!   drivers (`uhidev`), so it is not re-exported here.
//! - `struct usb_hid_descriptor`'s variable tail (`descrs[1]`, `bNumDescriptors` entries) is
//!   one entry here, as in C; the others are read from the configuration descriptor's bytes
//!   past it. `USB_HID_DESCRIPTOR_SIZE(n)` is [`usb_hid_descriptor_size`].

use crate::dev::usb::usb::{UByte, UWord, UsbWire};

/// `UR_GET_HID_DESCRIPTOR`.
pub const UR_GET_HID_DESCRIPTOR: u8 = 0x06;
/// `UDESC_HID`.
pub const UDESC_HID: u8 = 0x21;
/// `UDESC_REPORT`.
pub const UDESC_REPORT: u8 = 0x22;
/// `UDESC_PHYSICAL`.
pub const UDESC_PHYSICAL: u8 = 0x23;
/// `UR_SET_HID_DESCRIPTOR`.
pub const UR_SET_HID_DESCRIPTOR: u8 = 0x07;
/// `UR_GET_REPORT`.
pub const UR_GET_REPORT: u8 = 0x01;
/// `UR_SET_REPORT`.
pub const UR_SET_REPORT: u8 = 0x09;
/// `UR_GET_IDLE`.
pub const UR_GET_IDLE: u8 = 0x02;
/// `UR_SET_IDLE`.
pub const UR_SET_IDLE: u8 = 0x0a;
/// `UR_GET_PROTOCOL`.
pub const UR_GET_PROTOCOL: u8 = 0x03;
/// `UR_SET_PROTOCOL`.
pub const UR_SET_PROTOCOL: u8 = 0x0b;

/// An entry of `struct usb_hid_descriptor`'s `descrs[]`.
#[repr(C)]
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
#[allow(non_snake_case)] // the USB specification's member names, as in C
pub struct UsbHidDescr {
    /// `bDescriptorType`.
    pub bDescriptorType: UByte,
    /// `wDescriptorLength`.
    pub wDescriptorLength: UWord,
}

/// `struct usb_hid_descriptor`.
#[repr(C)]
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
#[allow(non_snake_case)] // the USB specification's member names, as in C
pub struct UsbHidDescriptor {
    /// `bLength`.
    pub bLength: UByte,
    /// `bDescriptorType`.
    pub bDescriptorType: UByte,
    /// `bcdHID`.
    pub bcdHID: UWord,
    /// `bCountryCode`.
    pub bCountryCode: UByte,
    /// `bNumDescriptors`.
    pub bNumDescriptors: UByte,
    /// `descrs[1]`: the first of `bNumDescriptors` entries.
    pub descrs: [UsbHidDescr; 1],
}

/// `USB_HID_DESCRIPTOR_SIZE(n)`.
pub const fn usb_hid_descriptor_size(n: usize) -> usize {
    9 + n * 3
}

/// `UHID_INPUT_REPORT`.
pub const UHID_INPUT_REPORT: u8 = 0x01;
/// `UHID_OUTPUT_REPORT`.
pub const UHID_OUTPUT_REPORT: u8 = 0x02;
/// `UHID_FEATURE_REPORT`.
pub const UHID_FEATURE_REPORT: u8 = 0x03;

// SAFETY: `#[repr(C)]` of `u8`s and byte arrays only (sizes asserted below).
unsafe impl UsbWire for UsbHidDescr {}
// SAFETY: as above.
unsafe impl UsbWire for UsbHidDescriptor {}

const _: () = {
    assert!(size_of::<UsbHidDescr>() == 3);
    assert!(size_of::<UsbHidDescriptor>() == usb_hid_descriptor_size(0));
    assert!(align_of::<UsbHidDescriptor>() == 1);
};
/* </CODE> */

/* <TESTS> */
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    #[ignore = "needs OPENBSD_SRC (just test-ref)"]
    fn constants_match_the_c_header() {
        let defs = crate::reftest::defines("sys/dev/usb/usbhid.h");
        let ours = crate::reftest::assert_defines!(defs;
            UR_GET_HID_DESCRIPTOR, UDESC_HID, UDESC_REPORT, UDESC_PHYSICAL,
            UR_SET_HID_DESCRIPTOR, UR_GET_REPORT, UR_SET_REPORT, UR_GET_IDLE, UR_SET_IDLE,
            UR_GET_PROTOCOL, UR_SET_PROTOCOL, UHID_INPUT_REPORT, UHID_OUTPUT_REPORT,
            UHID_FEATURE_REPORT,
        );
        crate::reftest::assert_complete(&defs, "UR_", &ours);
        crate::reftest::assert_complete(&defs, "UHID_", &ours);
    }
}
/* </TESTS> */
