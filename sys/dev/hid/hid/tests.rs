//! Host tests for the HID report descriptor parser, over the descriptors of real devices:
//! QEMU's `usb-kbd`, the boot-protocol mouse of the HID 1.11 specification (appendix E.10),
//! and hand-made ones for report IDs, Push/Pop, long items, usage tables and truncation.

use std::vec::Vec;
use std::{assert, assert_eq};

use super::*;

/// `qemu_keyboard_hid_report_desc` (hw/usb/dev-hid.c): the 8 modifier bits, a constant byte,
/// 5 LEDs plus 3 bits of padding out, and an array of 6 key codes.
const QEMU_KBD: &[u8] = &[
    0x05, 0x01, 0x09, 0x06, 0xa1, 0x01, 0x75, 0x01, 0x95, 0x08, 0x05, 0x07, 0x19, 0xe0, 0x29, 0xe7,
    0x15, 0x00, 0x25, 0x01, 0x81, 0x02, 0x95, 0x01, 0x75, 0x08, 0x81, 0x01, 0x95, 0x05, 0x75, 0x01,
    0x05, 0x08, 0x19, 0x01, 0x29, 0x05, 0x91, 0x02, 0x95, 0x01, 0x75, 0x03, 0x91, 0x01, 0x95, 0x06,
    0x75, 0x08, 0x15, 0x00, 0x25, 0xff, 0x05, 0x07, 0x19, 0x00, 0x29, 0xff, 0x81, 0x00, 0xc0,
];

/// The boot protocol mouse (HID 1.11, E.10): three buttons, five bits of padding, X and Y.
const BOOT_MOUSE: &[u8] = &[
    0x05, 0x01, 0x09, 0x02, 0xa1, 0x01, 0x09, 0x01, 0xa1, 0x00, 0x05, 0x09, 0x19, 0x01, 0x29, 0x03,
    0x15, 0x00, 0x25, 0x01, 0x95, 0x03, 0x75, 0x01, 0x81, 0x02, 0x95, 0x01, 0x75, 0x05, 0x81, 0x01,
    0x05, 0x01, 0x09, 0x30, 0x09, 0x31, 0x15, 0x81, 0x25, 0x7f, 0x75, 0x08, 0x95, 0x02, 0x81, 0x06,
    0xc0, 0xc0,
];

fn items(desc: &[u8], kind: HidKind) -> Vec<HidItem> {
    let mut d = hid_start_parse(desc, kind);
    let mut h = HidItem::default();
    let mut v = Vec::new();
    while hid_get_item(&mut d, &mut h) {
        // collections come out whatever the kind asked for, as in the C
        if kind == hid_all || h.kind == kind {
            v.push(h);
        }
    }
    hid_end_parse(d);
    v
}

#[test]
fn qemu_kbd_inputs_are_eight_modifiers_a_constant_and_an_array() {
    let v = items(QEMU_KBD, hid_input);
    assert_eq!(v.len(), 10);
    for (i, h) in v[..8].iter().enumerate() {
        assert_eq!(h.usage, hid_usage2(HUP_KEYBOARD, 0xe0 + i as u32));
        assert_eq!(h.flags, HIO_VARIABLE);
        assert_eq!(
            h.loc,
            HidLocation {
                size: 1,
                count: 1,
                pos: i as u32
            }
        );
        assert_eq!(h.logical_minimum, 0);
        assert_eq!(h.logical_maximum, 1);
    }
    assert_eq!(v[8].flags, HIO_CONST);
    assert_eq!(
        v[8].loc,
        HidLocation {
            size: 8,
            count: 1,
            pos: 8
        }
    );
    // the array: not variable, 6 elements of 8 bits at bit 16; its usage is the start of the
    // range
    assert_eq!(v[9].flags, 0);
    assert_eq!(
        v[9].loc,
        HidLocation {
            size: 8,
            count: 6,
            pos: 16
        }
    );
    // `Logical Maximum (255)` is a one-byte item and reads signed, as in the C
    assert_eq!(v[9].logical_maximum, -1);
}

#[test]
fn qemu_kbd_outputs_are_five_leds_and_padding() {
    let v = items(QEMU_KBD, hid_output);
    assert_eq!(v.len(), 6);
    for (i, h) in v[..5].iter().enumerate() {
        assert_eq!(h.usage, hid_usage2(HUP_LED, 1 + i as u32));
        assert_eq!(
            h.loc,
            HidLocation {
                size: 1,
                count: 1,
                pos: i as u32
            }
        );
    }
    assert_eq!(v[5].flags, HIO_CONST);
    assert_eq!(
        v[5].loc,
        HidLocation {
            size: 3,
            count: 1,
            pos: 5
        }
    );
}

#[test]
fn qemu_kbd_collections_nest_and_balance() {
    let v = items(QEMU_KBD, hid_all);
    let colls: Vec<_> = v
        .iter()
        .filter(|h| h.kind == hid_collection || h.kind == hid_endcollection)
        .collect();
    assert_eq!(colls.len(), 2);
    assert_eq!(colls[0].kind, hid_collection);
    assert_eq!(colls[0].collection, HCOLL_APPLICATION);
    assert_eq!(
        colls[0].usage,
        hid_usage2(HUP_GENERIC_DESKTOP, HUG_KEYBOARD)
    );
    assert_eq!(colls[0].collevel, 1);
    assert_eq!(colls[1].kind, hid_endcollection);
    assert_eq!(colls[1].collevel, 0);
}

#[test]
fn qemu_kbd_report_sizes() {
    assert_eq!(hid_report_size(QEMU_KBD, hid_input, 0), 8);
    assert_eq!(hid_report_size(QEMU_KBD, hid_output, 0), 1);
    assert_eq!(hid_report_size(QEMU_KBD, hid_feature, 0), 0);
    // no such report ID
    assert_eq!(hid_report_size(QEMU_KBD, hid_input, 3), 0);
}

#[test]
fn qemu_kbd_locate_modifiers_and_keys() {
    let mut loc = HidLocation::default();
    let mut flags = 0;
    // the left shift modifier is bit 1 of the first byte
    assert!(hid_locate(
        QEMU_KBD,
        hid_usage2(HUP_KEYBOARD, 0xe1),
        0,
        hid_input,
        Some(&mut loc),
        Some(&mut flags)
    ));
    assert_eq!(
        loc,
        HidLocation {
            size: 1,
            count: 1,
            pos: 1
        }
    );
    assert_eq!(flags, HIO_VARIABLE);
    // the key array is found by the first usage of its range
    assert!(hid_locate(
        QEMU_KBD,
        hid_usage2(HUP_KEYBOARD, 0),
        0,
        hid_input,
        Some(&mut loc),
        None
    ));
    assert_eq!(
        loc,
        HidLocation {
            size: 8,
            count: 6,
            pos: 16
        }
    );
    // LEDs
    assert!(hid_locate(
        QEMU_KBD,
        hid_usage2(HUP_LED, HUL_CAPS_LOCK),
        0,
        hid_output,
        Some(&mut loc),
        None
    ));
    assert_eq!(
        loc,
        HidLocation {
            size: 1,
            count: 1,
            pos: 1
        }
    );
    // the constant padding is never located; a miss zeroes the size and the flags
    loc.size = 77;
    flags = 77;
    assert!(!hid_locate(
        QEMU_KBD,
        hid_usage2(HUP_LED, HUL_KANA + 1),
        0,
        hid_output,
        Some(&mut loc),
        Some(&mut flags)
    ));
    assert_eq!(loc.size, 0);
    assert_eq!(flags, 0);
    // wrong kind, wrong report ID
    assert!(!hid_locate(
        QEMU_KBD,
        hid_usage2(HUP_LED, HUL_NUM_LOCK),
        0,
        hid_input,
        None,
        None
    ));
    assert!(!hid_locate(
        QEMU_KBD,
        hid_usage2(HUP_LED, HUL_NUM_LOCK),
        1,
        hid_output,
        None,
        None
    ));
}

#[test]
fn qemu_kbd_is_a_keyboard_collection() {
    assert!(hid_is_collection(
        QEMU_KBD,
        0,
        hid_usage2(HUP_GENERIC_DESKTOP, HUG_KEYBOARD) as i32
    ));
    assert!(!hid_is_collection(
        QEMU_KBD,
        0,
        hid_usage2(HUP_GENERIC_DESKTOP, HUG_MOUSE) as i32
    ));
    assert!(!hid_is_collection(
        QEMU_KBD,
        1,
        hid_usage2(HUP_GENERIC_DESKTOP, HUG_KEYBOARD) as i32
    ));
}

#[test]
fn boot_mouse_locations() {
    let mut loc = HidLocation::default();
    assert!(hid_locate(
        BOOT_MOUSE,
        hid_usage2(HUP_GENERIC_DESKTOP, HUG_X),
        0,
        hid_input,
        Some(&mut loc),
        None
    ));
    assert_eq!(
        loc,
        HidLocation {
            size: 8,
            count: 1,
            pos: 8
        }
    );
    let mut yloc = HidLocation::default();
    let mut flags = 0;
    assert!(hid_locate(
        BOOT_MOUSE,
        hid_usage2(HUP_GENERIC_DESKTOP, HUG_Y),
        0,
        hid_input,
        Some(&mut yloc),
        Some(&mut flags)
    ));
    assert_eq!(
        yloc,
        HidLocation {
            size: 8,
            count: 1,
            pos: 16
        }
    );
    assert_eq!(flags, HIO_VARIABLE | HIO_RELATIVE);
    let mut b = [HidLocation::default(); 3];
    for (i, l) in b.iter_mut().enumerate() {
        assert!(hid_locate(
            BOOT_MOUSE,
            hid_usage2(HUP_BUTTON, 1 + i as u32),
            0,
            hid_input,
            Some(l),
            None
        ));
        assert_eq!(
            *l,
            HidLocation {
                size: 1,
                count: 1,
                pos: i as u32
            }
        );
    }
    assert_eq!(hid_report_size(BOOT_MOUSE, hid_input, 0), 3);
    assert!(hid_is_collection(
        BOOT_MOUSE,
        0,
        hid_usage2(HUP_GENERIC_DESKTOP, HUG_MOUSE) as i32
    ));
}

#[test]
fn boot_mouse_data_is_signed() {
    let xloc = HidLocation {
        size: 8,
        count: 1,
        pos: 8,
    };
    let yloc = HidLocation {
        size: 8,
        count: 1,
        pos: 16,
    };
    let button = HidLocation {
        size: 1,
        count: 1,
        pos: 2,
    };
    let report = [0x05u8, 0xfe, 0x7f];
    assert_eq!(hid_get_data(&report, &xloc), -2);
    assert_eq!(hid_get_data(&report, &yloc), 127);
    assert_eq!(hid_get_udata(&report, &xloc), 0xfe);
    assert_eq!(
        hid_get_data(&report, &button),
        -1,
        "a set signed bit extends to -1"
    );
    assert_eq!(hid_get_udata(&report, &button), 1);
}

#[test]
fn get_data_crosses_bytes_and_clamps() {
    // 12 bits at bit 4: low nibble of byte 0 is skipped, the data is 0x321 for 0x12 0x34 0x56?
    let report = [0x10u8, 0x32, 0x54, 0x76];
    let l = HidLocation {
        size: 12,
        count: 1,
        pos: 4,
    };
    assert_eq!(hid_get_udata(&report, &l), 0x321);
    // sign extension of a 12-bit field
    let report = [0x00u8, 0xf8, 0xff];
    let l = HidLocation {
        size: 12,
        count: 1,
        pos: 4,
    };
    assert_eq!(hid_get_data(&report, &l), -128);
    // a field wider than 32 bits is cut to 32
    let report = [0x78u8, 0x56, 0x34, 0x12, 0xff, 0xff];
    let l = HidLocation {
        size: 48,
        count: 1,
        pos: 0,
    };
    assert_eq!(hid_get_udata(&report, &l), 0x1234_5678);
    // a zero-size field is 0
    assert_eq!(hid_get_udata(&report, &HidLocation::default()), 0);
    // bytes past the end of the report read as 0
    let l = HidLocation {
        size: 16,
        count: 1,
        pos: 8,
    };
    assert_eq!(hid_get_udata(&[0xaa, 0xbb], &l), 0xbb);
    assert_eq!(hid_get_udata(&[], &l), 0);
    // a 32-bit field at an unaligned position loses its top bits, as in the C
    let l = HidLocation {
        size: 32,
        count: 1,
        pos: 4,
    };
    assert_eq!(hid_get_udata(&[0x10, 0, 0, 0x80, 0xff], &l), 0x0800_0001);
}

/// Two reports: ID 1 has a 16-bit variable input and an 8-bit output, ID 2 a 24-bit input;
/// positions are tracked per ID.
const REPORT_IDS: &[u8] = &[
    0x05, 0x01, 0x09, 0x04, 0xa1, 0x01, // Generic Desktop, Joystick, Application collection
    0x85, 0x01, // Report ID 1
    0x09, 0x30, 0x75, 0x10, 0x95, 0x01, 0x81, 0x02, // X: 16 bits
    0x05, 0x08, 0x09, 0x01, 0x75, 0x08, 0x95, 0x01, 0x91, 0x02, // an LED: 8 bits out
    0x85, 0x02, // Report ID 2
    0x05, 0x01, 0x09, 0x31, 0x75, 0x18, 0x95, 0x01, 0x81, 0x02, // Y: 24 bits
    0x85, 0x01, // back to ID 1
    0x05, 0x01, 0x09, 0x32, 0x75, 0x08, 0x95, 0x01, 0x81, 0x02, // Z: 8 bits, follows X
    0xc0,
];

#[test]
fn report_ids_keep_their_own_positions() {
    let mut loc = HidLocation::default();
    assert!(hid_locate(
        REPORT_IDS,
        hid_usage2(HUP_GENERIC_DESKTOP, HUG_X),
        1,
        hid_input,
        Some(&mut loc),
        None
    ));
    assert_eq!(
        loc,
        HidLocation {
            size: 16,
            count: 1,
            pos: 0
        }
    );
    assert!(hid_locate(
        REPORT_IDS,
        hid_usage2(HUP_GENERIC_DESKTOP, HUG_Y),
        2,
        hid_input,
        Some(&mut loc),
        None
    ));
    assert_eq!(
        loc,
        HidLocation {
            size: 24,
            count: 1,
            pos: 0
        }
    );
    assert!(hid_locate(
        REPORT_IDS,
        hid_usage2(HUP_GENERIC_DESKTOP, HUG_Z),
        1,
        hid_input,
        Some(&mut loc),
        None
    ));
    assert_eq!(
        loc,
        HidLocation {
            size: 8,
            count: 1,
            pos: 16
        },
        "resumes after X"
    );
    // an item is only found under its own report ID
    assert!(!hid_locate(
        REPORT_IDS,
        hid_usage2(HUP_GENERIC_DESKTOP, HUG_Y),
        1,
        hid_input,
        None,
        None
    ));
    assert_eq!(hid_report_size(REPORT_IDS, hid_input, 1), 3);
    assert_eq!(hid_report_size(REPORT_IDS, hid_input, 2), 3);
    assert_eq!(hid_report_size(REPORT_IDS, hid_output, 1), 1);
    assert_eq!(hid_report_size(REPORT_IDS, hid_output, 2), 0);
    assert_eq!(
        hid_get_id_of_collection(
            REPORT_IDS,
            hid_usage2(HUP_GENERIC_DESKTOP, HUG_JOYSTICK),
            HCOLL_APPLICATION
        ),
        Some(0),
        "the collection opens before any report ID"
    );
    assert_eq!(
        hid_get_id_of_collection(
            REPORT_IDS,
            hid_usage2(HUP_GENERIC_DESKTOP, HUG_MOUSE),
            HCOLL_APPLICATION
        ),
        None
    );
}

#[test]
fn more_report_ids_than_slots_restart_at_zero() {
    // 20 distinct IDs: the table holds MAXID - 1 non-zero ones, the rest restart at pos 0
    let mut d: Vec<u8> = std::vec![0x05, 0x01, 0x09, 0x04, 0xa1, 0x01, 0x75, 0x08, 0x95, 0x01];
    for id in 1..=20u8 {
        d.extend_from_slice(&[0x85, id, 0x09, 0x30, 0x81, 0x02]);
    }
    d.push(0xc0);
    let v = items(&d, hid_input);
    assert_eq!(v.len(), 20);
    assert!(v.iter().all(|h| h.loc.pos == 0 && h.loc.size == 8));
}

#[test]
fn find_report_by_usages_and_collections() {
    let x = hid_usage2(HUP_GENERIC_DESKTOP, HUG_X) as i32;
    let z = hid_usage2(HUP_GENERIC_DESKTOP, HUG_Z) as i32;
    let y = hid_usage2(HUP_GENERIC_DESKTOP, HUG_Y) as i32;
    let app = hid_usage2(HUP_GENERIC_DESKTOP, HUG_JOYSTICK) as i32;
    assert_eq!(
        hid_find_report(REPORT_IDS, hid_input, app, &[x], None),
        Some(1)
    );
    // Z comes after report 2 interleaved: the matches restart with every change of ID, as
    // in the C, which expects the fields of a report to be together
    assert_eq!(
        hid_find_report(REPORT_IDS, hid_input, app, &[x, z], None),
        None
    );
    let m = hid_usage2(HUP_GENERIC_DESKTOP, HUG_MOUSE) as i32;
    assert_eq!(
        hid_find_report(BOOT_MOUSE, hid_input, m, &[x, y], None),
        Some(0)
    );
    assert_eq!(
        hid_find_report(BOOT_MOUSE, hid_input, m, &[y, x], None),
        Some(0)
    );
    assert_eq!(
        hid_find_report(REPORT_IDS, hid_input, app, &[y], None),
        Some(2)
    );
    // X and Y are in different reports
    assert_eq!(
        hid_find_report(REPORT_IDS, hid_input, app, &[x, y], None),
        None
    );
    // wrong application collection
    let mouse = hid_usage2(HUP_GENERIC_DESKTOP, HUG_MOUSE) as i32;
    assert_eq!(
        hid_find_report(REPORT_IDS, hid_input, mouse, &[x], None),
        None
    );
    // wrong kind
    assert_eq!(
        hid_find_report(REPORT_IDS, hid_output, app, &[x], None),
        None
    );
    // a single report without an ID answers 0
    let btn1 = hid_usage2(HUP_BUTTON, 1) as i32;
    let m = hid_usage2(HUP_GENERIC_DESKTOP, HUG_MOUSE) as i32;
    assert_eq!(
        hid_find_report(BOOT_MOUSE, hid_input, m, &[btn1], None),
        Some(0)
    );
    // the mouse's pointer collection is physical; with a collection list that omits it the
    // buttons inside are skipped, with one that names it they are found
    let ptr = hid_usage2(HUP_GENERIC_DESKTOP, HUG_POINTER) as i32;
    assert_eq!(
        hid_find_report(BOOT_MOUSE, hid_input, m, &[btn1], Some(&[0])),
        None
    );
    assert_eq!(
        hid_find_report(BOOT_MOUSE, hid_input, m, &[btn1], Some(&[ptr, 0])),
        Some(0)
    );
}

#[test]
fn find_report_skips_vendor_collections() {
    // Application (Generic Desktop, Mouse) { Collection (vendor page 0xff00) { Input (X) } }
    let d: &[u8] = &[
        0x05, 0x01, 0x09, 0x02, 0xa1, 0x01, 0x06, 0x00, 0xff, 0x09, 0x01, 0xa1, 0x00, 0x05, 0x01,
        0x09, 0x30, 0x75, 0x08, 0x95, 0x01, 0x81, 0x02, 0xc0, 0xc0,
    ];
    let m = hid_usage2(HUP_GENERIC_DESKTOP, HUG_MOUSE) as i32;
    let x = hid_usage2(HUP_GENERIC_DESKTOP, HUG_X) as i32;
    assert_eq!(hid_find_report(d, hid_input, m, &[x], None), None);
    let vend = hid_usage2(0xff00, 1) as i32;
    assert_eq!(
        hid_find_report(d, hid_input, m, &[x], Some(&[vend, 0])),
        Some(0)
    );
}

#[test]
fn push_and_pop_restore_the_globals() {
    // Report size 8, count 1; Push; size 16; Input; Pop; Input again uses size 8.
    let d: &[u8] = &[
        0x05, 0x01, 0x09, 0x04, 0xa1, 0x01, 0x75, 0x08, 0x95, 0x01, 0xa4, // Push
        0x75, 0x10, 0x09, 0x30, 0x81, 0x02, 0xb4, // Pop
        0x09, 0x31, 0x81, 0x02, 0xc0,
    ];
    let v = items(d, hid_input);
    assert_eq!(v.len(), 2);
    assert_eq!(
        v[0].loc,
        HidLocation {
            size: 16,
            count: 1,
            pos: 0
        }
    );
    assert_eq!(
        v[1].loc,
        HidLocation {
            size: 8,
            count: 1,
            pos: 16
        }
    );
}

#[test]
fn more_than_maxpush_pushes_are_ignored() {
    let mut d: Vec<u8> = std::vec![0x75, 0x08, 0x95, 0x01];
    d.extend(std::iter::repeat_n(0xa4u8, 10));
    d.extend_from_slice(&[0x09, 0x30, 0x81, 0x02]);
    d.extend(std::iter::repeat_n(0xb4u8, 10));
    d.extend_from_slice(&[0x09, 0x31, 0x81, 0x02]);
    let v = items(&d, hid_input);
    assert_eq!(v.len(), 2);
}

#[test]
fn usage_ranges_and_usage_lists_distribute_over_a_count() {
    // Usage (X), Usage (Y), Report Count 4: the third and fourth items repeat the last usage
    let d: &[u8] = &[
        0x05, 0x01, 0x09, 0x30, 0x09, 0x31, 0x75, 0x08, 0x95, 0x04, 0x81, 0x02,
    ];
    let v = items(d, hid_input);
    let u: Vec<u32> = v.iter().map(|h| h.usage).collect();
    let (x, y) = (
        hid_usage2(HUP_GENERIC_DESKTOP, HUG_X),
        hid_usage2(HUP_GENERIC_DESKTOP, HUG_Y),
    );
    assert_eq!(u, std::vec![x, y, y, y]);
    let p: Vec<u32> = v.iter().map(|h| h.loc.pos).collect();
    assert_eq!(p, std::vec![0, 8, 16, 24]);
    // minimum without a maximum is dropped
    let d: &[u8] = &[0x05, 0x09, 0x19, 0x01, 0x75, 0x01, 0x95, 0x02, 0x81, 0x02];
    let v = items(d, hid_input);
    assert_eq!(v.len(), 2);
    assert_eq!(v[0].usage, 0);
    // an inverted range is dropped as well
    let d: &[u8] = &[
        0x05, 0x09, 0x19, 0x05, 0x29, 0x01, 0x75, 0x01, 0x95, 0x02, 0x81, 0x02,
    ];
    let v = items(d, hid_input);
    assert_eq!(v[0].usage, 0);
}

#[test]
fn full_width_usages_ignore_the_page() {
    // a 4-byte usage item carries its own page: 0x000c0001 (Consumer Control)
    let d: &[u8] = &[
        0x05, 0x01, 0x0b, 0x01, 0x00, 0x0c, 0x00, 0x75, 0x08, 0x95, 0x01, 0x81, 0x02,
    ];
    let v = items(d, hid_input);
    assert_eq!(v[0].usage, hid_usage2(HUP_CONSUMER, HUC_CONTROL));
}

#[test]
fn long_items_are_skipped() {
    // 0xfe: long item, 3 bytes of data, tag 0x10; then a normal Input
    let d: &[u8] = &[
        0x05, 0x01, 0x09, 0x30, 0x75, 0x08, 0x95, 0x01, 0xfe, 0x03, 0x00, 0x10, 0xaa, 0xbb, 0xcc,
        0x81, 0x02,
    ];
    let v = items(d, hid_input);
    assert_eq!(v.len(), 1);
    assert_eq!(v[0].usage, hid_usage2(HUP_GENERIC_DESKTOP, HUG_X));
}

#[test]
fn a_variable_count_is_capped() {
    // 5000 one-bit variable items are cut to MAXLOCCNT
    let d: &[u8] = &[0x75, 0x01, 0x96, 0x88, 0x13, 0x81, 0x02];
    assert_eq!(items(d, hid_input).len(), MAXLOCCNT as usize);
}

#[test]
fn an_unbalanced_end_collection_ends_the_parse() {
    let d: &[u8] = &[
        0x75, 0x08, 0x95, 0x01, 0x09, 0x30, 0x81, 0x02, 0xc0, 0x09, 0x31, 0x81, 0x02,
    ];
    assert_eq!(items(d, hid_input).len(), 1);
}

#[test]
fn a_truncated_descriptor_ends_cleanly() {
    // the last item claims 4 data bytes and the descriptor ends after 1
    let d: &[u8] = &[0x75, 0x08, 0x95, 0x01, 0x09, 0x30, 0x81, 0x02, 0x17, 0x01];
    assert_eq!(items(d, hid_input).len(), 1);
    assert_eq!(items(&[], hid_all).len(), 0);
    assert_eq!(hid_report_size(&[], hid_input, 0), 0);
}

#[test]
fn collection_data_resumes_after_the_collection() {
    let mut d = hid_get_collection_data(
        BOOT_MOUSE,
        hid_usage2(HUP_GENERIC_DESKTOP, HUG_POINTER) as i32,
        HCOLL_PHYSICAL,
    )
    .expect("the pointer collection is there");
    let mut h = HidItem::default();
    assert!(hid_get_item(&mut d, &mut h));
    assert_eq!(h.kind, hid_input);
    assert_eq!(h.usage, hid_usage2(HUP_BUTTON, 1));
    hid_end_parse(d);
    assert!(
        hid_get_collection_data(BOOT_MOUSE, hid_usage2(HUP_LED, 1) as i32, HCOLL_PHYSICAL)
            .is_none()
    );
}

#[test]
fn usage_helpers_split_and_join() {
    let u = hid_usage2(HUP_GENERIC_DESKTOP, HUG_WHEEL);
    assert_eq!(u, 0x0001_0038);
    assert_eq!(hid_get_usage_page(u), HUP_GENERIC_DESKTOP);
    assert_eq!(hid_get_usage(u), HUG_WHEEL);
}

/// Every constant of `<dev/hid/hid.h>` against the C header.
#[test]
#[ignore = "needs OPENBSD_SRC"]
fn constants_match_the_c_header() {
    let defs = crate::reftest::defines("sys/dev/hid/hid.h");
    let names = crate::reftest::assert_defines!(defs;
        HUP_UNDEFINED,
        HUP_GENERIC_DESKTOP,
        HUP_SIMULATION,
        HUP_VR_CONTROLS,
        HUP_SPORTS_CONTROLS,
        HUP_GAMING_CONTROLS,
        HUP_KEYBOARD,
        HUP_LED,
        HUP_BUTTON,
        HUP_ORDINALS,
        HUP_TELEPHONY,
        HUP_CONSUMER,
        HUP_DIGITIZERS,
        HUP_PHYSICAL_IFACE,
        HUP_UNICODE,
        HUP_ALPHANUM_DISPLAY,
        HUP_MONITOR,
        HUP_MONITOR_ENUM_VAL,
        HUP_VESA_VC,
        HUP_VESA_CMD,
        HUP_POWER,
        HUP_BATTERY,
        HUP_BARCODE_SCANNER,
        HUP_SCALE,
        HUP_CAMERA_CONTROL,
        HUP_ARCADE,
        HUP_VENDOR,
        HUP_FIDO,
        HUP_MICROSOFT,
        HUP_APPLE,
        HUP_WACOM,
        HUP_INAME,
        HUP_PRESENT_STATUS,
        HUP_CHANGED_STATUS,
        HUP_UPS,
        HUP_POWER_SUPPLY,
        HUP_BATTERY_SYSTEM,
        HUP_BATTERY_SYSTEM_ID,
        HUP_PD_BATTERY,
        HUP_BATTERY_ID,
        HUP_CHARGER,
        HUP_CHARGER_ID,
        HUP_POWER_CONVERTER,
        HUP_POWER_CONVERTER_ID,
        HUP_OUTLET_SYSTEM,
        HUP_OUTLET_SYSTEM_ID,
        HUP_INPUT,
        HUP_INPUT_ID,
        HUP_OUTPUT,
        HUP_OUTPUT_ID,
        HUP_FLOW,
        HUP_FLOW_ID,
        HUP_OUTLET,
        HUP_OUTLET_ID,
        HUP_GANG,
        HUP_GANG_ID,
        HUP_POWER_SUMMARY,
        HUP_POWER_SUMMARY_ID,
        HUP_VOLTAGE,
        HUP_CURRENT,
        HUP_FREQUENCY,
        HUP_APPARENT_POWER,
        HUP_ACTIVE_POWER,
        HUP_PERCENT_LOAD,
        HUP_TEMPERATURE,
        HUP_HUMIDITY,
        HUP_BADCOUNT,
        HUP_CONFIG_VOLTAGE,
        HUP_CONFIG_CURRENT,
        HUP_CONFIG_FREQUENCY,
        HUP_CONFIG_APP_POWER,
        HUP_CONFIG_ACT_POWER,
        HUP_CONFIG_PERCENT_LOAD,
        HUP_CONFIG_TEMPERATURE,
        HUP_CONFIG_HUMIDITY,
        HUP_SWITCHON_CONTROL,
        HUP_SWITCHOFF_CONTROL,
        HUP_TOGGLE_CONTROL,
        HUP_LOW_VOLT_TRANSF,
        HUP_HIGH_VOLT_TRANSF,
        HUP_DELAYBEFORE_REBOOT,
        HUP_DELAYBEFORE_STARTUP,
        HUP_DELAYBEFORE_SHUTDWN,
        HUP_TEST,
        HUP_MODULE_RESET,
        HUP_AUDIBLE_ALRM_CTL,
        HUP_PRESENT,
        HUP_GOOD,
        HUP_INTERNAL_FAILURE,
        HUP_PD_VOLT_OUTOF_RANGE,
        HUP_FREQ_OUTOFRANGE,
        HUP_OVERLOAD,
        HUP_OVERCHARGED,
        HUP_OVERTEMPERATURE,
        HUP_SHUTDOWN_REQUESTED,
        HUP_SHUTDOWN_IMMINENT,
        HUP_SWITCH_ON_OFF,
        HUP_SWITCHABLE,
        HUP_USED,
        HUP_BOOST,
        HUP_BUCK,
        HUP_INITIALIZED,
        HUP_TESTED,
        HUP_AWAITING_POWER,
        HUP_COMMUNICATION_LOST,
        HUP_IMANUFACTURER,
        HUP_IPRODUCT,
        HUP_ISERIALNUMBER,
        HUB_SMB_BATTERY_MODE,
        HUB_SMB_BATTERY_STATUS,
        HUB_SMB_ALARM_WARNING,
        HUB_SMB_CHARGER_MODE,
        HUB_SMB_CHARGER_STATUS,
        HUB_SMB_CHARGER_SPECINF,
        HUB_SMB_SELECTR_STATE,
        HUB_SMB_SELECTR_PRESETS,
        HUB_SMB_SELECTR_INFO,
        HUB_SMB_OPT_MFGFUNC1,
        HUB_SMB_OPT_MFGFUNC2,
        HUB_SMB_OPT_MFGFUNC3,
        HUB_SMB_OPT_MFGFUNC4,
        HUB_SMB_OPT_MFGFUNC5,
        HUB_CONNECTIONTOSMBUS,
        HUB_OUTPUT_CONNECTION,
        HUB_CHARGER_CONNECTION,
        HUB_BATTERY_INSERTION,
        HUB_USENEXT,
        HUB_OKTOUSE,
        HUB_BATTERY_SUPPORTED,
        HUB_SELECTOR_REVISION,
        HUB_CHARGING_INDICATOR,
        HUB_MANUFACTURER_ACCESS,
        HUB_REM_CAPACITY_LIM,
        HUB_REM_TIME_LIM,
        HUB_ATRATE,
        HUB_CAPACITY_MODE,
        HUB_BCAST_TO_CHARGER,
        HUB_PRIMARY_BATTERY,
        HUB_CHANGE_CONTROLLER,
        HUB_TERMINATE_CHARGE,
        HUB_TERMINATE_DISCHARGE,
        HUB_BELOW_REM_CAP_LIM,
        HUB_REM_TIME_LIM_EXP,
        HUB_CHARGING,
        HUB_DISCHARGING,
        HUB_FULLY_CHARGED,
        HUB_FULLY_DISCHARGED,
        HUB_CONDITIONING_FLAG,
        HUB_ATRATE_OK,
        HUB_SMB_ERROR_CODE,
        HUB_NEED_REPLACEMENT,
        HUB_ATRATE_TIMETOFULL,
        HUB_ATRATE_TIMETOEMPTY,
        HUB_AVERAGE_CURRENT,
        HUB_MAXERROR,
        HUB_REL_STATEOF_CHARGE,
        HUB_ABS_STATEOF_CHARGE,
        HUB_REM_CAPACITY,
        HUB_FULLCHARGE_CAPACITY,
        HUB_RUNTIMETO_EMPTY,
        HUB_AVERAGETIMETO_EMPTY,
        HUB_AVERAGETIMETO_FULL,
        HUB_CYCLECOUNT,
        HUB_BATTPACKMODEL_LEVEL,
        HUB_INTERNAL_CHARGE_CTL,
        HUB_PRIMARY_BATTERY_SUP,
        HUB_DESIGN_CAPACITY,
        HUB_SPECIFICATION_INFO,
        HUB_MANUFACTURER_DATE,
        HUB_SERIAL_NUMBER,
        HUB_IMANUFACTURERNAME,
        HUB_IDEVICENAME,
        HUB_IDEVICECHEMISTERY,
        HUB_MANUFACTURERDATA,
        HUB_RECHARGABLE,
        HUB_WARN_CAPACITY_LIM,
        HUB_CAPACITY_GRANUL1,
        HUB_CAPACITY_GRANUL2,
        HUB_IOEM_INFORMATION,
        HUB_INHIBIT_CHARGE,
        HUB_ENABLE_POLLING,
        HUB_RESTORE_TO_ZERO,
        HUB_AC_PRESENT,
        HUB_BATTERY_PRESENT,
        HUB_POWER_FAIL,
        HUB_ALARM_INHIBITED,
        HUB_THERMISTOR_UNDRANGE,
        HUB_THERMISTOR_HOT,
        HUB_THERMISTOR_COLD,
        HUB_THERMISTOR_OVERANGE,
        HUB_BS_VOLT_OUTOF_RANGE,
        HUB_BS_CURR_OUTOF_RANGE,
        HUB_BS_CURR_NOT_REGULTD,
        HUB_BS_VOLT_NOT_REGULTD,
        HUB_MASTER_MODE,
        HUB_CHARGER_SELECTR_SUP,
        HUB_CHARGER_SPEC,
        HUB_LEVEL2,
        HUB_LEVEL3,
        HUG_POINTER,
        HUG_MOUSE,
        HUG_FN_KEY,
        HUG_JOYSTICK,
        HUG_GAME_PAD,
        HUG_KEYBOARD,
        HUG_KEYPAD,
        HUG_X,
        HUG_Y,
        HUG_Z,
        HUG_RX,
        HUG_RY,
        HUG_RZ,
        HUG_SLIDER,
        HUG_DIAL,
        HUG_WHEEL,
        HUG_HAT_SWITCH,
        HUG_COUNTED_BUFFER,
        HUG_BYTE_COUNT,
        HUG_MOTION_WAKEUP,
        HUG_VX,
        HUG_VY,
        HUG_VZ,
        HUG_VBRX,
        HUG_VBRY,
        HUG_VBRZ,
        HUG_VNO,
        HUG_TWHEEL,
        HUG_SYSTEM_CONTROL,
        HUG_SYSTEM_POWER_DOWN,
        HUG_SYSTEM_SLEEP,
        HUG_SYSTEM_WAKEUP,
        HUG_SYSTEM_CONTEXT_MENU,
        HUG_SYSTEM_MAIN_MENU,
        HUG_SYSTEM_APP_MENU,
        HUG_SYSTEM_MENU_HELP,
        HUG_SYSTEM_MENU_EXIT,
        HUG_SYSTEM_MENU_SELECT,
        HUG_SYSTEM_MENU_RIGHT,
        HUG_SYSTEM_MENU_LEFT,
        HUG_SYSTEM_MENU_UP,
        HUG_SYSTEM_MENU_DOWN,
        HUD_UNDEFINED,
        HUD_DIGITIZER,
        HUD_PEN,
        HUD_TOUCHSCREEN,
        HUD_TOUCHPAD,
        HUD_CONFIG,
        HUD_STYLUS,
        HUD_FINGER,
        HUD_TIP_PRESSURE,
        HUD_BARREL_PRESSURE,
        HUD_IN_RANGE,
        HUD_TOUCH,
        HUD_UNTOUCH,
        HUD_TAP,
        HUD_QUALITY,
        HUD_DATA_VALID,
        HUD_TRANSDUCER_INDEX,
        HUD_TABLET_FKEYS,
        HUD_PROGRAM_CHANGE_KEYS,
        HUD_BATTERY_STRENGTH,
        HUD_INVERT,
        HUD_X_TILT,
        HUD_Y_TILT,
        HUD_AZIMUTH,
        HUD_ALTITUDE,
        HUD_TWIST,
        HUD_TIP_SWITCH,
        HUD_SEC_TIP_SWITCH,
        HUD_BARREL_SWITCH,
        HUD_ERASER,
        HUD_TABLET_PICK,
        HUD_CONFIDENCE,
        HUD_WIDTH,
        HUD_HEIGHT,
        HUD_CONTACTID,
        HUD_INPUT_MODE,
        HUD_DEVICE_INDEX,
        HUD_CONTACTCOUNT,
        HUD_CONTACT_MAX,
        HUD_SCAN_TIME,
        HUD_BUTTON_TYPE,
        HUD_SECONDARY_BARREL_SWITCH,
        HUD_WACOM_X,
        HUD_WACOM_Y,
        HUD_WACOM_DISTANCE,
        HUD_WACOM_PAD_BUTTONS00,
        HUD_WACOM_BATTERY,
        HUL_NUM_LOCK,
        HUL_CAPS_LOCK,
        HUL_SCROLL_LOCK,
        HUL_COMPOSE,
        HUL_KANA,
        HUC_CONTROL,
        HUC_TRACK_NEXT,
        HUC_TRACK_PREV,
        HUC_STOP,
        HUC_PLAY_PAUSE,
        HUC_VOLUME,
        HUC_MUTE,
        HUC_VOL_INC,
        HUC_VOL_DEC,
        HUC_AC_PAN,
        HUF_U2FHID,
        HUF_RAW_IN_DATA_REPORT,
        HUF_RAW_OUT_DATA_REPORT,
        HCOLL_PHYSICAL,
        HCOLL_APPLICATION,
        HCOLL_LOGICAL,
        HIO_CONST,
        HIO_VARIABLE,
        HIO_RELATIVE,
        HIO_WRAP,
        HIO_NONLINEAR,
        HIO_NOPREF,
        HIO_NULLSTATE,
        HIO_VOLATILE,
        HIO_BUFBYTES,
        HCC_UNDEFINED,
        HCC_MAX,
    );
    crate::reftest::assert_complete(&defs, "H", &names);
}
