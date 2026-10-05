use std::vec::Vec;

use super::*;
use crate::dev::usb::usbdi::{
    USBD_STALLED, UsbdDescIter, usbd_desc_iter_next, usbd_get_no_alts, usbd_str,
};

/// A configuration: interface 0 (alternate 0 with a bulk-in and a bulk-out endpoint whose
/// packet sizes are wrong for high speed, alternate 1 with an interrupt endpoint), then
/// interface 1 with a control endpoint and a super speed companion.
fn sample_config() -> Vec<u8> {
    let mut v = Vec::new();
    v.extend_from_slice(&[9, UDESC_CONFIG, 0, 0, 2, 1, 0, 0x80, 50]);
    v.extend_from_slice(&[9, UDESC_INTERFACE, 0, 0, 2, 8, 6, 80, 0]);
    v.extend_from_slice(&[7, UDESC_ENDPOINT, 0x81, UE_BULK, 64, 0, 0]);
    v.extend_from_slice(&[7, UDESC_ENDPOINT, 0x02, UE_BULK, 0, 2, 0]);
    v.extend_from_slice(&[9, UDESC_INTERFACE, 0, 1, 1, 8, 6, 80, 0]);
    v.extend_from_slice(&[7, UDESC_ENDPOINT, 0x83, 0x03, 8, 0, 10]);
    v.extend_from_slice(&[9, UDESC_INTERFACE, 1, 0, 1, 0xff, 0, 0, 0]);
    v.extend_from_slice(&[7, UDESC_ENDPOINT, 0x04, UE_CONTROL, 8, 0, 0]);
    v.extend_from_slice(&[6, UDESC_ENDPOINT_SS_COMP, 3, 0, 0, 0]);
    let len = v.len() as u16;
    v[2..4].copy_from_slice(&len.to_le_bytes());
    v
}

#[test]
fn errstr_names_every_status() {
    assert_eq!(usbd_errstr(USBD_NORMAL_COMPLETION), "NORMAL_COMPLETION");
    assert_eq!(usbd_errstr(USBD_STALLED), "STALLED");
    assert_eq!(usbd_errstr(USBD_INVAL), "INVAL");
    // The C prints the number of a status past the table: only USBD_ERROR_MAX is.
    assert_eq!(usbd_errstr(USBD_ERROR_MAX), "19");
    assert_eq!(USBD_ERROR_MAX as i32, 19);
}

#[test]
fn find_interface_and_endpoint_descriptors() {
    let cd = sample_config();
    let i0 = usbd_find_idesc(&cd, 0, 0).unwrap();
    assert_eq!(
        (i0.bInterfaceNumber, i0.bAlternateSetting, i0.bNumEndpoints),
        (0, 0, 2)
    );
    let i0a1 = usbd_find_idesc(&cd, 0, 1).unwrap();
    assert_eq!(i0a1.bAlternateSetting, 1);
    let i1 = usbd_find_idesc(&cd, 1, 0).unwrap();
    assert_eq!(i1.bInterfaceClass, 0xff);
    assert!(usbd_find_idesc(&cd, 2, 0).is_none());
    assert!(usbd_find_idesc(&cd, 1, 1).is_none());

    assert_eq!(
        usbd_find_edesc(&cd, 0, 0, 0).unwrap().bEndpointAddress,
        0x81
    );
    assert_eq!(
        usbd_find_edesc(&cd, 0, 0, 1).unwrap().bEndpointAddress,
        0x02
    );
    // Endpoint indices stop at the next interface descriptor.
    assert!(usbd_find_edesc(&cd, 0, 0, 2).is_none());
    assert_eq!(
        usbd_find_edesc(&cd, 0, 1, 0).unwrap().bEndpointAddress,
        0x83
    );
    assert_eq!(
        usbd_find_edesc(&cd, 1, 0, 0).unwrap().bEndpointAddress,
        0x04
    );

    assert_eq!(usbd_get_no_alts(&cd, 0), 2);
    assert_eq!(usbd_get_no_alts(&cd, 1), 1);
    assert_eq!(usbd_get_no_alts(&cd, 7), 0);
}

#[test]
fn a_zero_length_descriptor_ends_the_walk() {
    let mut cd = sample_config();
    cd[9] = 0; // the first interface descriptor's bLength
    assert!(usbd_find_idesc(&cd, 0, 0).is_none());
    assert_eq!(usbd_get_no_alts(&cd, 0), 0);
}

#[test]
fn endpoints_and_companions_are_walked_as_parse_idesc_does() {
    let cd = sample_config();
    let mut seen = Vec::new();
    assert!(usbd_walk_endpoints(&cd, 9, |i, ed, essd| seen.push((
        i,
        cd[ed + 2],
        essd
    ))));
    assert_eq!(seen, [(0, 0x81, None), (1, 0x02, None)]);
    let i1 = cd.len() - 6 - 7 - 9;
    let mut seen = Vec::new();
    assert!(usbd_walk_endpoints(&cd, i1, |i, ed, essd| seen.push((
        i,
        cd[ed + 2],
        essd
    ))));
    assert_eq!(seen, [(0, 0x04, Some(cd.len() - 6))]);
    // An interface claiming more endpoints than follow it is refused.
    let mut bad = cd.clone();
    bad[9 + 4] = 3;
    assert!(!usbd_walk_endpoints(&bad, 9, |_, _, _| {}));
}

#[test]
fn high_speed_packet_sizes_are_fixed_before_publication() {
    let mut cd = sample_config();
    usbd_fix_max_packet(&mut cd);
    let e =
        |cd: &[u8], ifc, alt, n| ugetw(usbd_find_edesc(cd, ifc, alt, n).unwrap().wMaxPacketSize);
    assert_eq!(e(&cd, 0, 0, 0), USB_2_MAX_BULK_PACKET);
    assert_eq!(e(&cd, 0, 0, 1), USB_2_MAX_BULK_PACKET);
    // Interrupt endpoints keep theirs.
    assert_eq!(e(&cd, 0, 1, 0), 8);
    assert_eq!(e(&cd, 1, 0, 0), USB_2_MAX_CTRL_PACKET);
}

#[test]
fn descriptor_iterator() {
    let cd = sample_config();
    let mut it = UsbdDescIter::new(&cd);
    let mut types = Vec::new();
    while let Some(d) = usbd_desc_iter_next(&mut it) {
        types.push(d.bDescriptorType);
    }
    assert_eq!(
        types,
        [
            UDESC_CONFIG,
            UDESC_INTERFACE,
            UDESC_ENDPOINT,
            UDESC_ENDPOINT,
            UDESC_INTERFACE,
            UDESC_ENDPOINT,
            UDESC_INTERFACE,
            UDESC_ENDPOINT,
            UDESC_ENDPOINT_SS_COMP
        ]
    );
}

#[test]
fn strings_are_trimmed_and_bcd_printed() {
    let mut s = *b"  QEMU  USB Tablet \n \0xx";
    usbd_trim_spaces(&mut s);
    assert_eq!(&s[..cstrlen(&s)], b"QEMU  USB Tablet");

    let mut b = [0u8; 8];
    assert_eq!(usbd_printBCD(&mut b, 0x0200), 4);
    assert_eq!(&b[..5], b"2.00\0");
    let mut small = [0u8; 3];
    assert_eq!(usbd_printBCD(&mut small, 0x0110), 2);
    assert_eq!(&small, b"1.\0");
    assert_eq!(usbd_printBCD(&mut [], 0x0110), 0);

    let mut sd = UsbStringDescriptor::zeroed();
    assert_eq!(usbd_str(&mut sd, 254, b"EmiBSD\0"), 14);
    assert_eq!(sd.bLength, 14);
    assert_eq!(sd.bDescriptorType, UDESC_STRING);
    assert_eq!(ugetw(sd.bString[0]), u16::from(b'E'));
    assert_eq!(usbd_str(&mut sd, 1, b"x"), 1);
}
