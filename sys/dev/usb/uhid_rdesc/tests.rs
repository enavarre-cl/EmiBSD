//! Host tests for `uhid_rdesc`: the replacement report descriptors parse with the report IDs
//! their C comments name, and (reference-backed) are the header's bytes.

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
