//! Host tests for `ukbd`: the country code table (and, reference-backed, its agreement with
//! `ukbd.c`), the reports reaching `hidkbd` only while the keyboard is enabled, and the
//! ioctls the access operations answer.

use std::boxed::Box;
use std::mem::MaybeUninit;
use std::{assert, assert_eq};

use super::*;
use crate::dev::hid::hidkbd::hidkbd_parse_desc;
use crate::dev::usb::usb::USB_GET_REPORT_ID;
use crate::dev::wscons::wsconsio::{WSCONS_EVENT_KEY_DOWN, WSKBDIO_GETLEDS};
use crate::kern::subr_pool::tests::setup_real_memory;
use crate::sys::ioctl::{ioctl_arg, ioctl_ret};

/// QEMU's `usb-kbd` report descriptor.
const QEMU_KBD: &[u8] = &[
    0x05, 0x01, 0x09, 0x06, 0xa1, 0x01, 0x75, 0x01, 0x95, 0x08, 0x05, 0x07, 0x19, 0xe0, 0x29, 0xe7,
    0x15, 0x00, 0x25, 0x01, 0x81, 0x02, 0x95, 0x01, 0x75, 0x08, 0x81, 0x01, 0x95, 0x05, 0x75, 0x01,
    0x05, 0x08, 0x19, 0x01, 0x29, 0x05, 0x91, 0x02, 0x95, 0x01, 0x75, 0x03, 0x91, 0x01, 0x95, 0x06,
    0x75, 0x08, 0x15, 0x00, 0x25, 0xff, 0x05, 0x07, 0x19, 0x00, 0x29, 0xff, 0x81, 0x00, 0xc0,
];

/// A zeroed keyboard softc, as `config_make_softc` makes one.
fn softc() -> &'static UkbdSoftc {
    // SAFETY: the `Softc` contract: all-zero is a valid `UkbdSoftc`.
    Box::leak(Box::new(unsafe {
        MaybeUninit::<UkbdSoftc>::zeroed().assume_init()
    }))
}

fn cookie(sc: &UkbdSoftc) -> *mut c_void {
    ptr::from_ref(sc).cast_mut().cast()
}

#[test]
fn the_country_code_table() {
    assert_eq!(UKBD_COUNTRYLAYOUT.len(), 1 + HCC_MAX as usize);
    assert_eq!(UKBD_COUNTRYLAYOUT[0], KB_NONE);
    assert_eq!(UKBD_COUNTRYLAYOUT[8], KB_FR);
    assert_eq!(UKBD_COUNTRYLAYOUT[9], KB_DE);
    assert_eq!(UKBD_COUNTRYLAYOUT[32], KB_UK);
    assert_eq!(UKBD_COUNTRYLAYOUT[33], KB_US);
    assert_eq!(UKBD_COUNTRYLAYOUT[35], KB_NONE);
}

/// The layout a `ukbd_countrylayout[]` initialiser line of the C names.
fn layout_named(name: &str) -> KbdT {
    match name {
        "KB_BE" => KB_BE,
        "KB_CF" => KB_CF,
        "KB_DK" => KB_DK,
        "KB_FR" => KB_FR,
        "KB_DE" => KB_DE,
        "KB_HU" => KB_HU,
        "KB_IT" => KB_IT,
        "KB_JP" => KB_JP,
        "KB_LA" => KB_LA,
        "KB_NO" => KB_NO,
        "KB_PL" => KB_PL,
        "KB_PT" => KB_PT,
        "KB_RU" => KB_RU,
        "KB_ES" => KB_ES,
        "KB_SV" => KB_SV,
        "KB_SF" => KB_SF,
        "KB_SG" => KB_SG,
        "KB_TR" => KB_TR,
        "KB_UK" => KB_UK,
        "KB_US" => KB_US,
        "(kbd_t)-1" => KB_NONE,
        other => panic!("a layout the test does not know: {other}"),
    }
}

#[test]
#[ignore = "needs OPENBSD_SRC (just test-ref)"]
fn the_country_code_table_is_the_c_one() {
    let path = crate::reftest::openbsd_src().join("sys/dev/usb/ukbd.c");
    let src = std::fs::read_to_string(path).unwrap();
    let body = src
        .split("ukbd_countrylayout[1 + HCC_MAX] = {")
        .nth(1)
        .unwrap()
        .split("};")
        .next()
        .unwrap();
    let rows: std::vec::Vec<KbdT> = body
        .lines()
        .filter_map(|l| {
            let l = l.trim();
            let tok = l.split(['\t', ' ', ',']).find(|t| !t.is_empty())?;
            Some(layout_named(tok))
        })
        .collect();
    assert_eq!(rows, UKBD_COUNTRYLAYOUT);
}

#[test]
fn reports_reach_hidkbd_only_when_enabled() {
    let _g = setup_real_memory();
    let sc = softc();
    assert_eq!(hidkbd_parse_desc(&sc.sc_kbd, 0, QEMU_KBD), Ok(()));
    sc.sc_kbd.sc_polling.set(1);
    // 'a' pressed.
    let mut report = [0u8, 0, 0x04, 0, 0, 0, 0, 0];

    ukbd_intr(&sc.sc_hdev, &mut report);
    assert_eq!(sc.sc_kbd.sc_npollchar.get(), 0);

    sc.sc_kbd.sc_enabled.set(1);
    ukbd_intr(&sc.sc_hdev, &mut report);
    assert_eq!(sc.sc_kbd.sc_npollchar.get(), 1);
    let (mut t, mut d) = (0, 0);
    hidkbd_cngetc(&sc.sc_kbd, &mut t, &mut d);
    assert_eq!(t, WSCONS_EVENT_KEY_DOWN);
    assert!(d != 0);
}

#[test]
fn the_access_ioctls() {
    let sc = softc();
    let v = cookie(sc);
    let p = &crate::kern::init_main::PROC0;

    // The keyboard type is USB.
    let mut data = [0u8; 4];
    assert_eq!(
        ukbd_ioctl(v, WSKBDIO_GTYPE, &mut data, 0, Some(p)),
        Ok(true)
    );
    assert_eq!(ioctl_arg::<i32>(&data), WSKBD_TYPE_USB as i32);

    // Setting the LEDs of a keyboard that is not attached is a no-op that succeeds.
    ioctl_ret(&mut data, &WSKBD_LED_NUM);
    assert_eq!(
        ukbd_ioctl(v, WSKBDIO_SETLEDS, &mut data, 0, Some(p)),
        Ok(true)
    );
    assert_eq!(sc.sc_kbd.sc_leds.get(), 0);

    // uhidev's: the report ID of the child.
    sc.sc_hdev.sc_report_id.set(7);
    assert_eq!(
        ukbd_ioctl(v, USB_GET_REPORT_ID, &mut data, 0, Some(p)),
        Ok(true)
    );
    assert_eq!(ioctl_arg::<i32>(&data), 7);

    // hidkbd's: the LEDs it last set.
    sc.sc_kbd.sc_leds.set(WSKBD_LED_CAPS);
    assert_eq!(
        ukbd_ioctl(v, WSKBDIO_GETLEDS, &mut data, 0, Some(p)),
        Ok(true)
    );
    assert_eq!(ioctl_arg::<i32>(&data), WSKBD_LED_CAPS);

    // Nobody's.
    assert_eq!(ukbd_ioctl(v, 0x1234, &mut data, 0, Some(p)), Ok(false));
}

#[test]
fn the_never_console_list_matches_the_c_devices() {
    let hit = |v, p| usb_lookup(&UKBD_NEVER_CONSOLE, v, p).is_some();
    assert!(hit(USB_VENDOR_APPLE, USB_PRODUCT_APPLE_BLUETOOTH_HCI));
    assert!(hit(USB_VENDOR_MICRODIA, USB_PRODUCT_MICRODIA_TEMPER));
    assert!(hit(USB_VENDOR_WCH2, USB_PRODUCT_WCH2_TEMPER));
    // Another Apple product is a keyboard like any other.
    assert!(!hit(USB_VENDOR_APPLE, USB_PRODUCT_APPLE_WELLSPRING_ANSI));
}
