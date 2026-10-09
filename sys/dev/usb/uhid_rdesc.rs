/* <LICENSES> */
/*
 * Copyright (c) 2000 Nick Hibma <n_hibma@freebsd.org>
 * Copyright (c) 2005 Ed Schouten <ed@FreeBSD.org>
 * All rights reserved.
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
 * THIS SOFTWARE IS PROVIDED BY THE AUTHOR AND CONTRIBUTORS ``AS IS'' AND
 * ANY EXPRESS OR IMPLIED WARRANTIES, INCLUDING, BUT NOT LIMITED TO, THE
 * IMPLIED WARRANTIES OF MERCHANTABILITY AND FITNESS FOR A PARTICULAR PURPOSE
 * ARE DISCLAIMED.  IN NO EVENT SHALL THE AUTHOR OR CONTRIBUTORS BE LIABLE
 * FOR ANY DIRECT, INDIRECT, INCIDENTAL, SPECIAL, EXEMPLARY, OR CONSEQUENTIAL
 * DAMAGES (INCLUDING, BUT NOT LIMITED TO, PROCUREMENT OF SUBSTITUTE GOODS
 * OR SERVICES; LOSS OF USE, DATA, OR PROFITS; OR BUSINESS INTERRUPTION)
 * HOWEVER CAUSED AND ON ANY THEORY OF LIABILITY, WHETHER IN CONTRACT, STRICT
 * LIABILITY, OR TORT (INCLUDING NEGLIGENCE OR OTHERWISE) ARISING IN ANY WAY
 * OUT OF THE USE OF THIS SOFTWARE, EVEN IF ADVISED OF THE POSSIBILITY OF
 * SUCH DAMAGE.
 */
/* </LICENSES> */

/* <CODE> */
//! `<dev/usb/uhid_rdesc.h>`: replacement HID report descriptors for devices that ship broken
//! (the Wacom Graphire) or no descriptor at all (the Xbox 360 and Xbox One gamepads), used by
//! `uhidev_attach` (`uhidev.rs`) and, later, `uhid(4)`.
//!
//! Upstream: sys/dev/usb/uhid_rdesc.h @ 3ce1f3f79392
//!
//! The tables are the header's bytes, one item per line, with its comments; the C's `static
//! uByte` arrays are `pub static [u8; N]`.
//!
//! ## Deviations
//! - `#[rustfmt::skip]` keeps the item-per-line layout of the C tables.
//! - The arrays are `pub` (the C's are `static` in a header included by `uhidev.c` and
//!   `uhid.c`), and `const` is dropped from the C's two `static const` ones: all four are
//!   immutable statics.

/// `uhid_graphire_report_descr`.
#[rustfmt::skip]
pub static UHID_GRAPHIRE_REPORT_DESCR: [u8; 125] = [
    0x05, 0x0d,  // USAGE_PAGE (Digitizers)
    0x09, 0x01,  // USAGE (Digitizer)
    0xa1, 0x01,  // COLLECTION (Application)
    0x85, 0x02,  // REPORT_ID (2)
    0x05, 0x0d,  // USAGE_PAGE (Digitizers)
    0x09, 0x01,  // USAGE (Digitizer)
    0xa1, 0x00,  // COLLECTION (Physical)
    0x15, 0x00,  // LOGICAL_MINIMUM (0)
    0x25, 0x01,  // LOGICAL_MAXIMUM (1)
    0x09, 0x33,  // USAGE (Touch)
    0x95, 0x01,  // REPORT_COUNT (1)
    0x75, 0x01,  // REPORT_SIZE (1)
    0x81, 0x02,  // INPUT (Data,Var,Abs)
    0x09, 0x44,  // USAGE (Barrel Switch)
    0x95, 0x02,  // REPORT_COUNT (2)
    0x75, 0x01,  // REPORT_SIZE (1)
    0x81, 0x02,  // INPUT (Data,Var,Abs)
    0x09, 0x00,  // USAGE (Undefined)
    0x95, 0x02,  // REPORT_COUNT (2)
    0x75, 0x01,  // REPORT_SIZE (1)
    0x81, 0x03,  // INPUT (Cnst,Var,Abs)
    0x09, 0x3c,  // USAGE (Invert)
    0x95, 0x01,  // REPORT_COUNT (1)
    0x75, 0x01,  // REPORT_SIZE (1)
    0x81, 0x02,  // INPUT (Data,Var,Abs)
    0x09, 0x38,  // USAGE (Transducer Index)
    0x95, 0x01,  // REPORT_COUNT (1)
    0x75, 0x01,  // REPORT_SIZE (1)
    0x81, 0x02,  // INPUT (Data,Var,Abs)
    0x09, 0x32,  // USAGE (In Range)
    0x95, 0x01,  // REPORT_COUNT (1)
    0x75, 0x01,  // REPORT_SIZE (1)
    0x81, 0x02,  // INPUT (Data,Var,Abs)
    0x05, 0x01,  // USAGE_PAGE (Generic Desktop)
    0x09, 0x30,  // USAGE (X)
    0x15, 0x00,  // LOGICAL_MINIMUM (0)
    0x26, 0xde, 0x27,  // LOGICAL_MAXIMUM (10206)
    0x95, 0x01,  // REPORT_COUNT (1)
    0x75, 0x10,  // REPORT_SIZE (16)
    0x81, 0x02,  // INPUT (Data,Var,Abs)
    0x09, 0x31,  // USAGE (Y)
    0x26, 0xfe, 0x1c,  // LOGICAL_MAXIMUM (7422)
    0x95, 0x01,  // REPORT_COUNT (1)
    0x75, 0x10,  // REPORT_SIZE (16)
    0x81, 0x02,  // INPUT (Data,Var,Abs)
    0x05, 0x0d,  // USAGE_PAGE (Digitizers)
    0x09, 0x30,  // USAGE (Tip Pressure)
    0x26, 0xff, 0x01,  // LOGICAL_MAXIMUM (511)
    0x95, 0x01,  // REPORT_COUNT (1)
    0x75, 0x10,  // REPORT_SIZE (16)
    0x81, 0x02,  // INPUT (Data,Var,Abs)
    0xc0,  // END_COLLECTION
    0x05, 0x0d,  // USAGE_PAGE (Digitizers)
    0x09, 0x00,  // USAGE (Undefined)
    0x85, 0x02,  // REPORT_ID (2)
    0x95, 0x01,  // REPORT_COUNT (1)
    0xb1, 0x02,  // FEATURE (Data,Var,Abs)
    0x09, 0x00,  // USAGE (Undefined)
    0x85, 0x03,  // REPORT_ID (3)
    0x95, 0x01,  // REPORT_COUNT (1)
    0xb1, 0x02,  // FEATURE (Data,Var,Abs)
    0xc0,  // END_COLLECTION
];

/// `uhid_graphire3_4x5_report_descr`.
#[rustfmt::skip]
pub static UHID_GRAPHIRE3_4X5_REPORT_DESCR: [u8; 161] = [
    0x05, 0x01,  // USAGE_PAGE (Generic Desktop)
    0x09, 0x02,  // USAGE (Mouse)
    0xa1, 0x01,  // COLLECTION (Application)
    0x85, 0x01,  // REPORT_ID (1)
    0x09, 0x01,  // USAGE (Pointer)
    0xa1, 0x00,  // COLLECTION (Physical)
    0x05, 0x09,  // USAGE_PAGE (Button)
    0x19, 0x01,  // USAGE_MINIMUM (Button 1)
    0x29, 0x03,  // USAGE_MAXIMUM (Button 3)
    0x15, 0x00,  // LOGICAL_MINIMUM (0)
    0x25, 0x01,  // LOGICAL_MAXIMUM (1)
    0x95, 0x03,  // REPORT_COUNT (3)
    0x75, 0x01,  // REPORT_SIZE (1)
    0x81, 0x02,  // INPUT (Data,Var,Abs)
    0x95, 0x01,  // REPORT_COUNT (1)
    0x75, 0x05,  // REPORT_SIZE (5)
    0x81, 0x01,  // INPUT (Cnst,Ary,Abs)
    0x05, 0x01,  // USAGE_PAGE (Generic Desktop)
    0x09, 0x30,  // USAGE (X)
    0x09, 0x31,  // USAGE (Y)
    0x09, 0x38,  // USAGE (Wheel)
    0x15, 0x81,  // LOGICAL_MINIMUM (-127)
    0x25, 0x7f,  // LOGICAL_MAXIMUM (127)
    0x75, 0x08,  // REPORT_SIZE (8)
    0x95, 0x03,  // REPORT_COUNT (3)
    0x81, 0x06,  // INPUT (Data,Var,Rel)
    0xc0,  // END_COLLECTION
    0xc0,  // END_COLLECTION
    0x05, 0x0d,  // USAGE_PAGE (Digitizers)
    0x09, 0x01,  // USAGE (Pointer)
    0xa1, 0x01,  // COLLECTION (Application)
    0x85, 0x02,  // REPORT_ID (2)
    0x05, 0x0d,  // USAGE_PAGE (Digitizers)
    0x09, 0x01,  // USAGE (Digitizer)
    0xa1, 0x00,  // COLLECTION (Physical)
    0x09, 0x33,  // USAGE (Touch)
    0x09, 0x44,  // USAGE (Barrel Switch)
    0x09, 0x44,  // USAGE (Barrel Switch)
    0x15, 0x00,  // LOGICAL_MINIMUM (0)
    0x25, 0x01,  // LOGICAL_MAXIMUM (1)
    0x75, 0x01,  // REPORT_SIZE (1)
    0x95, 0x03,  // REPORT_COUNT (3)
    0x81, 0x02,  // INPUT (Data,Var,Abs)
    0x75, 0x01,  // REPORT_SIZE (1)
    0x95, 0x02,  // REPORT_COUNT (2)
    0x81, 0x01,  // INPUT (Cnst,Ary,Abs)
    0x09, 0x3c,  // USAGE (Invert)
    0x09, 0x38,  // USAGE (Transducer Index)
    0x09, 0x32,  // USAGE (In Range)
    0x75, 0x01,  // REPORT_SIZE (1)
    0x95, 0x03,  // REPORT_COUNT (3)
    0x81, 0x02,  // INPUT (Data,Var,Abs)
    0x05, 0x01,  // USAGE_PAGE (Generic Desktop)
    0x09, 0x30,  // USAGE (X)
    0x15, 0x00,  // LOGICAL_MINIMUM (0)
    0x26, 0xde, 0x27,  // LOGICAL_MAXIMUM (10206)
    0x75, 0x10,  // REPORT_SIZE (16)
    0x95, 0x01,  // REPORT_COUNT (1)
    0x81, 0x02,  // INPUT (Data,Var,Abs)
    0x09, 0x31,  // USAGE (Y)
    0x26, 0xfe, 0x1c,  // LOGICAL_MAXIMUM (7422)
    0x75, 0x10,  // REPORT_SIZE (16)
    0x95, 0x01,  // REPORT_COUNT (1)
    0x81, 0x02,  // INPUT (Data,Var,Abs)
    0x05, 0x0d,  // USAGE_PAGE (Digitizers)
    0x09, 0x30,  // USAGE (Tip Pressure)
    0x26, 0xff, 0x01,  // LOGICAL_MAXIMUM (511)
    0x75, 0x10,  // REPORT_SIZE (16)
    0x95, 0x01,  // REPORT_COUNT (1)
    0x81, 0x02,  // INPUT (Data,Var,Abs)
    0xc0,  // END_COLLECTION
    0x05, 0x0d,  // USAGE_PAGE (Digitizers)
    0x09, 0x00,  // USAGE (Undefined)
    0x85, 0x02,  // REPORT_ID (2)
    0x95, 0x01,  // REPORT_COUNT (1)
    0xb1, 0x02,  // FEATURE (Data,Var,Abs)
    0x09, 0x00,  // USAGE (Undefined)
    0x85, 0x03,  // REPORT_ID (3)
    0x95, 0x01,  // REPORT_COUNT (1)
    0xb1, 0x02,  // FEATURE (Data,Var,Abs)
    0xc0,  // END_COLLECTION
];

/// The descriptor has no output report format, thus preventing you from
/// controlling the LEDs and the built-in rumblers.
/// `uhid_xb360gp_report_descr`.
#[rustfmt::skip]
pub static UHID_XB360GP_REPORT_DESCR: [u8; 166] = [
    0x05, 0x01,  // USAGE PAGE (Generic Desktop)
    0x09, 0x05,  // USAGE (Gamepad)
    0xa1, 0x01,  // COLLECTION (Application)
    // Unused
    0x75, 0x08,  // REPORT SIZE (8)
    0x95, 0x01,  // REPORT COUNT (1)
    0x81, 0x01,  // INPUT (Constant)
    // Byte count
    0x75, 0x08,  // REPORT SIZE (8)
    0x95, 0x01,  // REPORT COUNT (1)
    0x05, 0x01,  // USAGE PAGE (Generic Desktop)
    0x09, 0x3b,  // USAGE (Byte Count)
    0x81, 0x01,  // INPUT (Constant)
    // D-Pad
    0x05, 0x01,  // USAGE PAGE (Generic Desktop)
    0x09, 0x01,  // USAGE (Pointer)
    0xa1, 0x00,  // COLLECTION (Physical)
    0x75, 0x01,  // REPORT SIZE (1)
    0x15, 0x00,  // LOGICAL MINIMUM (0)
    0x25, 0x01,  // LOGICAL MAXIMUM (1)
    0x35, 0x00,  // PHYSICAL MINIMUM (0)
    0x45, 0x01,  // PHYSICAL MAXIMUM (1)
    0x95, 0x04,  // REPORT COUNT (4)
    0x05, 0x01,  // USAGE PAGE (Generic Desktop)
    0x09, 0x90,  // USAGE (D-Pad Up)
    0x09, 0x91,  // USAGE (D-Pad Down)
    0x09, 0x93,  // USAGE (D-Pad Left)
    0x09, 0x92,  // USAGE (D-Pad Right)
    0x81, 0x02,  // INPUT (Data, Variable, Absolute)
    0xc0,  // END COLLECTION
    // Buttons 5-11
    0x75, 0x01,  // REPORT SIZE (1)
    0x15, 0x00,  // LOGICAL MINIMUM (0)
    0x25, 0x01,  // LOGICAL MAXIMUM (1)
    0x35, 0x00,  // PHYSICAL MINIMUM (0)
    0x45, 0x01,  // PHYSICAL MAXIMUM (1)
    0x95, 0x07,  // REPORT COUNT (7)
    0x05, 0x09,  // USAGE PAGE (Button)
    0x09, 0x08,  // USAGE (Button 8)
    0x09, 0x07,  // USAGE (Button 7)
    0x09, 0x09,  // USAGE (Button 9)
    0x09, 0x0a,  // USAGE (Button 10)
    0x09, 0x05,  // USAGE (Button 5)
    0x09, 0x06,  // USAGE (Button 6)
    0x09, 0x0b,  // USAGE (Button 11)
    0x81, 0x02,  // INPUT (Data, Variable, Absolute)
    // Unused
    0x75, 0x01,  // REPORT SIZE (1)
    0x95, 0x01,  // REPORT COUNT (1)
    0x81, 0x01,  // INPUT (Constant)
    // Buttons 1-4
    0x75, 0x01,  // REPORT SIZE (1)
    0x15, 0x00,  // LOGICAL MINIMUM (0)
    0x25, 0x01,  // LOGICAL MAXIMUM (1)
    0x35, 0x00,  // PHYSICAL MINIMUM (0)
    0x45, 0x01,  // PHYSICAL MAXIMUM (1)
    0x95, 0x04,  // REPORT COUNT (4)
    0x05, 0x09,  // USAGE PAGE (Button)
    0x19, 0x01,  // USAGE MINIMUM (Button 1)
    0x29, 0x04,  // USAGE MAXIMUM (Button 4)
    0x81, 0x02,  // INPUT (Data, Variable, Absolute)
    // Triggers
    0x75, 0x08,  // REPORT SIZE (8)
    0x15, 0x00,  // LOGICAL MINIMUM (0)
    0x26, 0xff, 0x00,  // LOGICAL MAXIMUM (255)
    0x35, 0x00,  // PHYSICAL MINIMUM (0)
    0x46, 0xff, 0x00,  // PHYSICAL MAXIMUM (255)
    0x95, 0x02,  // REPORT SIZE (2)
    0x05, 0x01,  // USAGE PAGE (Generic Desktop)
    0x09, 0x32,  // USAGE (Z)
    0x09, 0x35,  // USAGE (Rz)
    0x81, 0x02,  // INPUT (Data, Variable, Absolute)
    // Sticks
    0x75, 0x10,  // REPORT SIZE (16)
    0x16, 0x00, 0x80,  // LOGICAL MINIMUM (-32768)
    0x26, 0xff, 0x7f,  // LOGICAL MAXIMUM (32767)
    0x36, 0x00, 0x80,  // PHYSICAL MINIMUM (-32768)
    0x46, 0xff, 0x7f,  // PHYSICAL MAXIMUM (32767)
    0x95, 0x04,  // REPORT COUNT (4)
    0x05, 0x01,  // USAGE PAGE (Generic Desktop)
    0x09, 0x30,  // USAGE (X)
    0x09, 0x31,  // USAGE (Y)
    0x09, 0x33,  // USAGE (Rx)
    0x09, 0x34,  // USAGE (Ry)
    0x81, 0x02,  // INPUT (Data, Variable, Absolute)
    // Unused
    0x75, 0x30,  // REPORT SIZE (48)
    0x95, 0x01,  // REPORT COUNT (1)
    0x81, 0x01,  // INPUT (Constant)
    0xc0,  // END COLLECTION
];

/// `uhid_xbonegp_report_descr`.
#[rustfmt::skip]
pub static UHID_XBONEGP_REPORT_DESCR: [u8; 149] = [
    0x05, 0x01,  // USAGE PAGE (Generic Desktop)
    0x09, 0x05,  // USAGE (Gamepad)
    0xa1, 0x01,  // COLLECTION (Application)
    // Button packet
    0xa1, 0x00,  // COLLECTION (Physical)
    0x85, 0x20,  // REPORT ID (0x20)
    // Skip unknown field and counter
    0x09, 0x00,  // USAGE (Undefined)
    0x75, 0x08,  // REPORT SIZE (8)
    0x95, 0x02,  // REPORT COUNT (2)
    0x81, 0x03,  // INPUT (Constant, Variable, Absolute)
    // Payload size
    0x09, 0x3b,  // USAGE (Byte Count)
    0x95, 0x01,  // REPORT COUNT (1)
    0x81, 0x02,  // INPUT (Data, Variable, Absolute)
    // 16 Buttons: 1-2=Unknown, 3=Start, 4=Back, 5-8=ABXY,
    // 9-12=D-Pad(Up,Dn,Lt,Rt), 13=LB, 14=RB, 15=LS, 16=RS
    // Skip the first 2 as they are not used
    0x75, 0x01,  // REPORT SIZE (1)
    0x95, 0x02,  // REPORT COUNT (2)
    0x81, 0x01,  // INPUT (Constant)
    // Assign buttons Start(7), Back(8), ABXY(1-4)
    0x15, 0x00,  // LOGICAL MINIMUM (0)
    0x25, 0x01,  // LOGICAL MAXIMUM (1)
    0x95, 0x06,  // REPORT COUNT (6)
    0x05, 0x09,  // USAGE PAGE (Button)
    0x09, 0x08,  // USAGE (Button 8)
    0x09, 0x07,  // USAGE (Button 7)
    0x09, 0x01,  // USAGE (Button 1)
    0x09, 0x02,  // USAGE (Button 2)
    0x09, 0x03,  // USAGE (Button 3)
    0x09, 0x04,  // USAGE (Button 4)
    0x81, 0x02,  // INPUT (Data, Variable, Absolute)
    // D-Pad
    0x05, 0x01,  // USAGE PAGE (Generic Desktop)
    0x09, 0x01,  // USAGE (Pointer)
    0xa1, 0x00,  // COLLECTION (Physical)
    0x75, 0x01,  // REPORT SIZE (1)
    0x15, 0x00,  // LOGICAL MINIMUM (0)
    0x25, 0x01,  // LOGICAL MAXIMUM (1)
    0x95, 0x04,  // REPORT COUNT (4)
    0x05, 0x01,  // USAGE PAGE (Generic Desktop)
    0x09, 0x90,  // USAGE (D-Pad Up)
    0x09, 0x91,  // USAGE (D-Pad Down)
    0x09, 0x93,  // USAGE (D-Pad Left)
    0x09, 0x92,  // USAGE (D-Pad Right)
    0x81, 0x02,  // INPUT (Data, Variable, Absolute)
    0xc0,  // END COLLECTION
    // Buttons 5-6 (Shoulder Buttons) and 9-10 (Stick Buttons)
    0x15, 0x00,  // LOGICAL MINIMUM (0)
    0x25, 0x01,  // LOGICAL MAXIMUM (1)
    0x95, 0x04,  // REPORT COUNT (4)
    0x05, 0x09,  // USAGE PAGE (Button)
    0x09, 0x05,  // USAGE (Button 5)
    0x09, 0x06,  // USAGE (Button 6)
    0x09, 0x09,  // USAGE (Button 9)
    0x09, 0x0a,  // USAGE (Button 10)
    0x81, 0x02,  // INPUT (Data, Variable, Absolute
    // Triggers
    0x15, 0x00,  // LOGICAL MINIMUM (0)
    0x26, 0xff, 0x03,  // LOGICAL MAXIMUM (1023)
    0x75, 0x10,  // REPORT SIZE (16)
    0x95, 0x02,  // REPORT COUNT (2)
    0x05, 0x01,  // USAGE PAGE (Generic Desktop)
    0x09, 0x32,  // USAGE (Z)
    0x09, 0x35,  // USAGE (Rz)
    0x81, 0x02,  // INPUT (Data, Variable, Absolute)
    // Sticks
    0x16, 0x00, 0x80,  // LOGICAL MINIMUM (-32768)
    0x26, 0xff, 0x7f,  // LOGICAL MAXIMUM (32767)
    0x09, 0x01,  // USAGE (Pointer)
    0xa1, 0x00,  // COLLECTION (Physical)
    0x95, 0x02,  // REPORT COUNT (2)
    0x05, 0x01,  // USAGE PAGE (Generic Desktop)
    0x09, 0x30,  // USAGE (X)
    0x09, 0x31,  // USAGE (Y)
    0x81, 0x02,  // INPUT (Data, Variable, Absolute)
    0xc0,  // END COLLECTION
    0x09, 0x01,  // USAGE (Pointer)
    0xa1, 0x00,  // COLLECTION (Physical)
    0x95, 0x02,  // REPORT COUNT (2)
    0x09, 0x33,  // USAGE (Rx)
    0x09, 0x34,  // USAGE (Ry)
    0x81, 0x02,  // INPUT (Data, Variable, Absolute)
    0xc0,  // END COLLECTION
    0xc0,  // END COLLECTION
];
/* </CODE> */

/* <TESTS> */
#[cfg(test)]
mod tests {
    // Host tests for `uhid_rdesc`: the replacement report descriptors parse with the report IDs
    // their C comments name, and (reference-backed) are the header's bytes.

    use std::string::String;
    use std::vec::Vec;
    use std::{assert, assert_eq};

    use super::*;
    use crate::dev::usb::uhidev::uhidev_maxrepid;

    #[test]
    fn the_descriptors_parse_with_their_report_ids() {
        // REPORT_ID 2 and 3 in the Graphire, 1, 2 and 3 in the Graphire3, none in the Xbox 360
        // gamepad (so the largest ID is 0), 0x20 in the Xbox One's.
        assert_eq!(uhidev_maxrepid(&UHID_GRAPHIRE_REPORT_DESCR), 3);
        assert_eq!(uhidev_maxrepid(&UHID_GRAPHIRE3_4X5_REPORT_DESCR), 3);
        assert_eq!(uhidev_maxrepid(&UHID_XB360GP_REPORT_DESCR), 0);
        assert_eq!(uhidev_maxrepid(&UHID_XBONEGP_REPORT_DESCR), 0x20);
    }

    #[test]
    fn each_descriptor_ends_in_its_application_collection() {
        // END_COLLECTION (0xc0) closes the application collection in all four.
        for d in [
            &UHID_GRAPHIRE_REPORT_DESCR[..],
            &UHID_GRAPHIRE3_4X5_REPORT_DESCR[..],
            &UHID_XB360GP_REPORT_DESCR[..],
            &UHID_XBONEGP_REPORT_DESCR[..],
        ] {
            assert_eq!(d.last(), Some(&0xc0));
        }
    }

    /// The bytes of each `uByte` array of `uhid_rdesc.h`, comments removed.
    fn c_arrays(src: &str) -> Vec<Vec<u8>> {
        let mut clean = String::new();
        let mut rest = src;
        while let Some(i) = rest.find("/*") {
            clean.push_str(&rest[..i]);
            let Some(j) = rest[i..].find("*/") else {
                break;
            };
            rest = &rest[i + j + 2..];
        }
        clean.push_str(rest);
        clean
            .split("uByte")
            .skip(1)
            .map(|a| {
                a.split(|c: char| !c.is_ascii_alphanumeric())
                    .filter_map(|tok| tok.strip_prefix("0x"))
                    .filter_map(|h| u8::from_str_radix(h, 16).ok())
                    .collect()
            })
            .collect()
    }

    #[test]
    #[ignore = "needs OPENBSD_SRC (just test-ref)"]
    fn the_descriptors_are_the_headers_bytes() {
        let path = crate::reftest::openbsd_src().join("sys/dev/usb/uhid_rdesc.h");
        let src = std::fs::read_to_string(path).unwrap();
        let c = c_arrays(&src);
        assert_eq!(c.len(), 4);
        assert_eq!(c[0], UHID_GRAPHIRE_REPORT_DESCR);
        assert_eq!(c[1], UHID_GRAPHIRE3_4X5_REPORT_DESCR);
        assert_eq!(c[2], UHID_XB360GP_REPORT_DESCR);
        assert_eq!(c[3], UHID_XBONEGP_REPORT_DESCR);
        assert!(c.iter().all(|a| !a.is_empty()));
    }
}
/* </TESTS> */
