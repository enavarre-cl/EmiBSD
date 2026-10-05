//! Host tests for `hidkbd`: descriptor parsing over QEMU's `usb-kbd`, the decoding of reports
//! into key events (read back through the polling path, which is what the console keyboard
//! uses), the LED report, the Apple key code rewriting, the raw scancodes, the ioctls and the
//! debounce quirk.

use std::boxed::Box;
use std::sync::atomic::AtomicU32;
use std::vec::Vec;
use std::{assert, assert_eq};

use super::*;
use crate::kern::subr_pool::tests::setup_real_memory;
use crate::sys::ioccom::_io;

/// QEMU's `usb-kbd` report descriptor (`hw/usb/dev-hid.c`): 8 modifier bits, a constant byte,
/// five LEDs plus padding out, and an array of 6 key codes.
const QEMU_KBD: &[u8] = &[
    0x05, 0x01, 0x09, 0x06, 0xa1, 0x01, 0x75, 0x01, 0x95, 0x08, 0x05, 0x07, 0x19, 0xe0, 0x29, 0xe7,
    0x15, 0x00, 0x25, 0x01, 0x81, 0x02, 0x95, 0x01, 0x75, 0x08, 0x81, 0x01, 0x95, 0x05, 0x75, 0x01,
    0x05, 0x08, 0x19, 0x01, 0x29, 0x05, 0x91, 0x02, 0x95, 0x01, 0x75, 0x03, 0x91, 0x01, 0x95, 0x06,
    0x75, 0x08, 0x15, 0x00, 0x25, 0xff, 0x05, 0x07, 0x19, 0x00, 0x29, 0xff, 0x81, 0x00, 0xc0,
];

/// A boot protocol mouse: no keys.
const BOOT_MOUSE: &[u8] = &[
    0x05, 0x01, 0x09, 0x02, 0xa1, 0x01, 0x09, 0x01, 0xa1, 0x00, 0x05, 0x09, 0x19, 0x01, 0x29, 0x03,
    0x15, 0x00, 0x25, 0x01, 0x95, 0x03, 0x75, 0x01, 0x81, 0x02, 0x95, 0x01, 0x75, 0x05, 0x81, 0x01,
    0x05, 0x01, 0x09, 0x30, 0x09, 0x31, 0x15, 0x81, 0x25, 0x7f, 0x75, 0x08, 0x95, 0x02, 0x81, 0x06,
    0xc0, 0xc0,
];

fn kbd_for(desc: &[u8]) -> Box<Hidkbd> {
    let kbd = Box::new(Hidkbd::new());
    assert_eq!(hidkbd_parse_desc(&kbd, 0, desc), Ok(()));
    kbd
}

/// Decodes `report` while polling and returns the events as (type, key code).
fn poll(kbd: &Hidkbd, report: &[u8]) -> Vec<(u32, i32)> {
    kbd.sc_polling.set(1);
    kbd.sc_npollchar.set(0);
    hidkbd_input(kbd, &mut report.to_vec());
    let mut v = Vec::new();
    while kbd.sc_npollchar.get() > 0 {
        let (mut t, mut d) = (0, 0);
        hidkbd_cngetc(kbd, &mut t, &mut d);
        v.push((t, d));
    }
    v
}

const UP: u32 = WSCONS_EVENT_KEY_UP;
const DOWN: u32 = WSCONS_EVENT_KEY_DOWN;

fn fake_device() -> &'static Device {
    // SAFETY: a `Device` is all-zero valid (`config_make_softc` allocates it zeroed); the box
    // is leaked, so the reference lives forever.
    Box::leak(Box::new(unsafe { core::mem::zeroed::<Device>() }))
}

#[test]
fn parse_qemu_usb_kbd() {
    let _g = setup_real_memory();
    let kbd = kbd_for(QEMU_KBD);
    assert_eq!(kbd.sc_nvar.get(), 8);
    assert_eq!(kbd.sc_nkeycode.get(), 6);
    assert_eq!(
        kbd.sc_keycodeloc.get(),
        HidLocation {
            size: 8,
            count: 6,
            pos: 16
        }
    );
    for (i, v) in kbd.vars().iter().enumerate() {
        assert_eq!(v.key, 0xe0 + i as u8, "the modifiers are usages 0xe0..0xe7");
        assert_eq!(v.mask, 1 << i);
        assert_eq!(
            v.loc,
            HidLocation {
                size: 1,
                count: 1,
                pos: i as u32
            }
        );
    }
    // the LEDs are the bits of the output report, in the order of the usages 1 to 5
    assert_eq!(
        kbd.sc_numloc.get(),
        HidLocation {
            size: 1,
            count: 1,
            pos: 0
        }
    );
    assert_eq!(
        kbd.sc_capsloc.get(),
        HidLocation {
            size: 1,
            count: 1,
            pos: 1
        }
    );
    assert_eq!(
        kbd.sc_scroloc.get(),
        HidLocation {
            size: 1,
            count: 1,
            pos: 2
        }
    );
    assert_eq!(
        kbd.sc_compose.get(),
        HidLocation {
            size: 1,
            count: 1,
            pos: 3
        }
    );
    assert_eq!(hidkbd_detach(&kbd, 0), Ok(()));
    assert!(kbd.sc_var.get().is_null());
}

#[test]
fn a_mouse_is_not_a_keyboard() {
    let _g = setup_real_memory();
    let kbd = Hidkbd::new();
    assert_eq!(
        hidkbd_parse_desc(&kbd, 0, BOOT_MOUSE),
        Err("no usable key codes array")
    );
    assert_eq!(kbd.sc_nkeycode.get(), 0);
    // another report ID than the keyboard's finds nothing either
    assert_eq!(
        hidkbd_parse_desc(&kbd, 1, QEMU_KBD),
        Err("no usable key codes array")
    );
}

#[test]
fn attach_failure_is_enxio_and_success_records_the_device() {
    let _g = setup_real_memory();
    let dev = fake_device();
    let kbd = Hidkbd::new();
    assert_eq!(
        hidkbd_attach(dev, &kbd, 0, 0, 0, BOOT_MOUSE),
        Err(Errno::ENXIO)
    );
    assert_eq!(hidkbd_attach(dev, &kbd, 0, 0, 0, QEMU_KBD), Ok(()));
    assert_eq!(kbd.sc_device.get(), Some(NonNull::from(dev)));
    assert_eq!(kbd.sc_debounce.get(), 0);
    assert_eq!(kbd.sc_console_keyboard.get(), 0);
    assert_eq!(hidkbd_detach(&kbd, 0), Ok(()));
}

#[test]
fn the_first_console_keyboard_takes_the_console() {
    let _g = setup_real_memory();
    let dev = fake_device();
    hidkbd_is_console.store(1, Ordering::Relaxed);
    let first = Hidkbd::new();
    let second = Hidkbd::new();
    assert_eq!(hidkbd_attach(dev, &first, 1, 0, 0, QEMU_KBD), Ok(()));
    assert_eq!(hidkbd_attach(dev, &second, 1, 0, 0, QEMU_KBD), Ok(()));
    assert_eq!(first.sc_console_keyboard.get(), 1);
    assert_eq!(second.sc_console_keyboard.get(), 0);
    assert_eq!(hidkbd_is_console.load(Ordering::Relaxed), 0);
    // detaching the console keyboard hands the console back
    assert_eq!(hidkbd_detach(&first, 0), Ok(()));
    assert_eq!(hidkbd_is_console.load(Ordering::Relaxed), 1);
    assert_eq!(hidkbd_detach(&second, 0), Ok(()));
    hidkbd_is_console.store(0, Ordering::Relaxed);
}

#[test]
fn reports_decode_to_presses_and_releases() {
    let _g = setup_real_memory();
    let kbd = kbd_for(QEMU_KBD);
    // 'a' goes down
    assert_eq!(poll(&kbd, &[0, 0, 4, 0, 0, 0, 0, 0]), [(DOWN, 4)]);
    // the same report again: nothing
    assert_eq!(poll(&kbd, &[0, 0, 4, 0, 0, 0, 0, 0]), []);
    // left shift (modifier bit 1, key 0xe1) and 'b' go down: the modifiers come first
    assert_eq!(
        poll(&kbd, &[0x02, 0, 4, 5, 0, 0, 0, 0]),
        [(DOWN, 0xe1), (DOWN, 5)]
    );
    // 'a' goes up while 'b' stays, even though the array shifted
    assert_eq!(poll(&kbd, &[0x02, 0, 5, 0, 0, 0, 0, 0]), [(UP, 4)]);
    // two modifiers at once, then everything up: releases, modifiers first
    assert_eq!(poll(&kbd, &[0x03, 0, 5, 0, 0, 0, 0, 0]), [(DOWN, 0xe0)]);
    assert_eq!(
        poll(&kbd, &[0, 0, 0, 0, 0, 0, 0, 0]),
        [(UP, 0xe0), (UP, 0xe1), (UP, 5)]
    );
    assert_eq!(hidkbd_detach(&kbd, 0), Ok(()));
}

#[test]
fn the_error_report_is_ignored_and_does_not_replace_the_state() {
    let _g = setup_real_memory();
    let kbd = kbd_for(QEMU_KBD);
    assert_eq!(poll(&kbd, &[0, 0, 4, 0, 0, 0, 0, 0]), [(DOWN, 4)]);
    // rollover error: all six slots 0x01
    assert_eq!(poll(&kbd, &[0, 0, 1, 1, 1, 1, 1, 1]), []);
    // so the next real report compares against 'a' still down
    assert_eq!(poll(&kbd, &[0, 0, 0, 0, 0, 0, 0, 0]), [(UP, 4)]);
    assert_eq!(hidkbd_detach(&kbd, 0), Ok(()));
}

#[test]
fn a_short_report_reads_as_no_keys() {
    let _g = setup_real_memory();
    let kbd = kbd_for(QEMU_KBD);
    assert_eq!(poll(&kbd, &[0, 0, 4, 0, 0, 0, 0, 0]), [(DOWN, 4)]);
    // 5 bytes cannot hold the array: its codes read as zero, the key is released
    assert_eq!(poll(&kbd, &[0, 0, 4, 0, 0]), [(UP, 4)]);
    assert_eq!(hidkbd_detach(&kbd, 0), Ok(()));
}

#[test]
fn polling_events_are_consumed_in_order() {
    let _g = setup_real_memory();
    let kbd = kbd_for(QEMU_KBD);
    kbd.sc_polling.set(1);
    hidkbd_input(&kbd, &mut [0x02, 0, 4, 5, 0, 0, 0, 0]);
    assert_eq!(kbd.sc_npollchar.get(), 3);
    let (mut t, mut d) = (0, 0);
    hidkbd_cngetc(&kbd, &mut t, &mut d);
    assert_eq!((t, d), (DOWN, 0xe1));
    assert_eq!(kbd.sc_npollchar.get(), 2);
    hidkbd_cngetc(&kbd, &mut t, &mut d);
    assert_eq!((t, d), (DOWN, 4));
    hidkbd_cngetc(&kbd, &mut t, &mut d);
    assert_eq!((t, d), (DOWN, 5));
    assert_eq!(kbd.sc_npollchar.get(), 0);
    assert_eq!(hidkbd_detach(&kbd, 0), Ok(()));
}

#[test]
fn events_reach_wskbd_when_a_child_is_attached() {
    let _g = setup_real_memory();
    let kbd = kbd_for(QEMU_KBD);
    // no child: decoded, then dropped (the C's `sc_wskbddev == NULL`)
    hidkbd_input(&kbd, &mut [0, 0, 4, 0, 0, 0, 0, 0]);
    assert_eq!(kbd.sc_odata.get().keycode[0], 4);
    // with a child the events go to the visible stubs, cooked and raw, without a crash
    kbd.sc_wskbddev.set(Some(NonNull::from(fake_device())));
    hidkbd_input(&kbd, &mut [0, 0, 5, 0, 0, 0, 0, 0]);
    assert_eq!(kbd.sc_odata.get().keycode[0], 5);
    kbd.sc_rawkbd.set(1);
    hidkbd_input(&kbd, &mut [0, 0, 0, 0, 0, 0, 0, 0]);
    assert_eq!(kbd.sc_odata.get().keycode[0], 0);
    kbd.sc_wskbddev.set(None);
    assert_eq!(hidkbd_detach(&kbd, 0), Ok(()));
}

#[test]
fn raw_scancodes_are_xt() {
    let mut cbuf = [0u8; 8];
    // a (usb 0x04) is XT 0x1e; released, with the top bit
    let n = hidkbd_raw_translate(&[0x04 | PRESS, 0x04 | RELEASE], &mut cbuf);
    assert_eq!(&cbuf[..n], [0x1e, 0x9e]);
    // insert (usb 0x49) is the extended XT e0 52
    let n = hidkbd_raw_translate(&[0x49 | PRESS, 0x49 | RELEASE], &mut cbuf);
    assert_eq!(&cbuf[..n], [0xe0, 0x52, 0xe0, 0xd2]);
    // keys without a scancode are skipped
    let n = hidkbd_raw_translate(&[0x00 | PRESS, 0xa0 | PRESS, 0x05 | PRESS], &mut cbuf);
    assert_eq!(&cbuf[..n], [0x30]);
    // the modifiers: left control, left shift, right alt (extended)
    let n = hidkbd_raw_translate(&[0xe0 | PRESS, 0xe1 | PRESS, 0xe6 | PRESS], &mut cbuf);
    assert_eq!(&cbuf[..n], [0x1d, 0x2a, 0xe0, 0x38]);
    assert_eq!(HIDKBD_TRTAB.len(), 256);
    assert_eq!(HIDKBD_TRTAB[0x29], 0x01, "escape");
    assert_eq!(HIDKBD_TRTAB[0x2c], 0x39, "space");
}

#[test]
fn leds_build_the_output_report() {
    let _g = setup_real_memory();
    let kbd = kbd_for(QEMU_KBD);
    let mut report = 0xff;
    assert!(
        !hidkbd_set_leds(&kbd, 0, &mut report),
        "unchanged: nothing to send"
    );
    assert_eq!(report, 0xff);
    assert!(hidkbd_set_leds(
        &kbd,
        WSKBD_LED_CAPS | WSKBD_LED_SCROLL,
        &mut report
    ));
    assert_eq!(
        report, 0b0000_0110,
        "caps is the second LED, scroll the third"
    );
    assert_eq!(kbd.sc_leds.get(), WSKBD_LED_CAPS | WSKBD_LED_SCROLL);
    assert!(!hidkbd_set_leds(
        &kbd,
        WSKBD_LED_CAPS | WSKBD_LED_SCROLL,
        &mut report
    ));
    assert!(hidkbd_set_leds(
        &kbd,
        WSKBD_LED_NUM | WSKBD_LED_COMPOSE,
        &mut report
    ));
    assert_eq!(report, 0b0000_1001);
    assert!(hidkbd_set_leds(&kbd, 0, &mut report));
    assert_eq!(report, 0);
    // an LED the descriptor lacks stays dark
    kbd.sc_capsloc.set(HidLocation::default());
    assert!(hidkbd_set_leds(&kbd, WSKBD_LED_CAPS, &mut report));
    assert_eq!(report, 0);
    assert_eq!(hidkbd_detach(&kbd, 0), Ok(()));
}

#[test]
fn enable_toggles_and_refuses_a_repeat() {
    let kbd = Hidkbd::new();
    assert_eq!(hidkbd_enable(&kbd, 1), Ok(()));
    assert_eq!(hidkbd_enable(&kbd, 1), Err(Errno::EBUSY));
    assert_eq!(hidkbd_enable(&kbd, 0), Ok(()));
    assert_eq!(hidkbd_enable(&kbd, 0), Err(Errno::EBUSY));
}

static BELLS: AtomicU32 = AtomicU32::new(0);

fn test_bell(arg: *mut c_void, pitch: u32, period: u32, volume: u32, poll: i32) {
    assert_eq!(arg as usize, 0x1234);
    assert_eq!((pitch, period, volume, poll), (1500, 100, 50, 0));
    BELLS.fetch_add(1, Ordering::Relaxed);
}

fn other_bell(_: *mut c_void, _: u32, _: u32, _: u32, _: i32) {
    panic!("the first hook keeps the bell");
}

#[test]
fn ioctls_leds_mode_and_bell() {
    let _g = setup_real_memory();
    let kbd = Hidkbd::new();
    let p = &crate::kern::init_main::PROC0;
    let mut data = [0u8; 16];

    kbd.sc_leds.set(WSKBD_LED_NUM);
    assert_eq!(
        hidkbd_ioctl(&kbd, WSKBDIO_GETLEDS, &mut data, 0, p),
        Ok(true)
    );
    assert_eq!(ioctl_arg::<i32>(&data), WSKBD_LED_NUM);

    ioctl_ret(&mut data, &WSKBD_RAW);
    assert_eq!(
        hidkbd_ioctl(&kbd, WSKBDIO_SETMODE, &mut data, 0, p),
        Ok(true)
    );
    assert_eq!(kbd.sc_rawkbd.get(), 1);
    ioctl_ret(&mut data, &0i32);
    assert_eq!(
        hidkbd_ioctl(&kbd, WSKBDIO_SETMODE, &mut data, 0, p),
        Ok(true)
    );
    assert_eq!(kbd.sc_rawkbd.get(), 0);

    // the bell: nothing hooked up yet is not an error
    ioctl_ret(
        &mut data,
        &WskbdBellData {
            which: 7,
            pitch: 1500,
            period: 100,
            volume: 50,
        },
    );
    assert_eq!(
        hidkbd_ioctl(&kbd, WSKBDIO_COMPLEXBELL, &mut data, 0, p),
        Ok(true)
    );
    let before = BELLS.load(Ordering::Relaxed);
    hidkbd_hookup_bell(test_bell, 0x1234 as *mut c_void);
    hidkbd_hookup_bell(other_bell, ptr::null_mut());
    assert_eq!(
        hidkbd_ioctl(&kbd, WSKBDIO_COMPLEXBELL, &mut data, 0, p),
        Ok(true)
    );
    assert_eq!(BELLS.load(Ordering::Relaxed), before + 1);

    // not ours: wskbd goes on
    assert_eq!(
        hidkbd_ioctl(&kbd, _io(b'W', 99), &mut data, 0, p),
        Ok(false)
    );
}

#[test]
fn translation_tables() {
    assert_eq!(hidkbd_translate(&APPLE_FN_TRANS, 82), 75, "up -> page up");
    assert_eq!(hidkbd_translate(&APPLE_FN_TRANS, 4), 0, "not in the table");
    assert_eq!(hidkbd_translate(&APPLE_ISO_TRANS, 53), 100);
    assert_eq!(hidkbd_translate(&APPLE_ISO_TRANS, 100), 53);
    assert_eq!(hidkbd_translate(&[], 53), 0);
    assert_eq!(APPLE_FN_TRANS.len(), 12);
    assert_eq!(APPLE_TB_TRANS[0], tr(30, 58));
}

/// A keyboard that has the key code array at byte 2 of 8 and the Fn key as bit 1 of byte 1.
fn apple_kbd() -> Hidkbd {
    let kbd = Hidkbd::new();
    kbd.sc_keycodeloc.set(HidLocation {
        size: 8,
        count: 6,
        pos: 16,
    });
    kbd.sc_nkeycode.set(6);
    kbd.sc_fn.set(HidLocation {
        size: 1,
        count: 1,
        pos: 9,
    });
    kbd
}

#[test]
fn apple_munging_rewrites_the_key_codes_with_fn_down() {
    let kbd = apple_kbd();

    // Fn up: nothing changes
    let mut r = [0, 0x00, 82, 40, 4, 0, 0, 0];
    hidkbd_apple_munge(&kbd, &mut r);
    assert_eq!(r, [0, 0x00, 82, 40, 4, 0, 0, 0]);

    // Fn down: up -> page up, return -> insert, 'a' stays
    let mut r = [0, 0x02, 82, 40, 4, 0, 0, 0];
    hidkbd_apple_munge(&kbd, &mut r);
    assert_eq!(r, [0, 0x02, 75, 73, 4, 0, 0, 0]);

    // the number row is the function keys on a Tb keyboard, after the Fn layer
    let mut r = [0, 0x02, 30, 31, 58, 0, 0, 0];
    hidkbd_apple_tb_munge(&kbd, &mut r);
    assert_eq!(r, [0, 0x02, 58, 59, 233, 0, 0, 0]);
    let mut r = [0, 0x00, 30, 0, 0, 0, 0, 0];
    hidkbd_apple_tb_munge(&kbd, &mut r);
    assert_eq!(r[2], 30, "Fn up");

    // MacBook Air: F9 is mute with Fn
    let mut r = [0, 0x02, 66, 0, 0, 0, 0, 0];
    hidkbd_apple_mba_munge(&kbd, &mut r);
    assert_eq!(r[2], 127);

    // the ISO layout swaps less and grave whatever Fn says
    let mut r = [0, 0x00, 53, 100, 0, 0, 0, 0];
    hidkbd_apple_iso_munge(&kbd, &mut r);
    assert_eq!(r, [0, 0x00, 100, 53, 0, 0, 0, 0]);
    let mut r = [0, 0x02, 53, 82, 0, 0, 0, 0];
    hidkbd_apple_iso_mba_munge(&kbd, &mut r);
    assert_eq!(r, [0, 0x02, 100, 75, 0, 0, 0, 0]);
}

#[test]
fn apple_munging_stays_inside_a_short_report() {
    let kbd = apple_kbd();
    let mut r = [0u8, 0x02, 82, 82];
    hidkbd_apple_munge(&kbd, &mut r);
    assert_eq!(r, [0, 0x02, 75, 75]);
    let mut r = [0u8, 0x02];
    hidkbd_apple_munge(&kbd, &mut r);
    assert_eq!(r, [0, 0x02]);
    hidkbd_apple_translate(&kbd, &mut [], &APPLE_FN_TRANS);
}

fn rewrite_up_to_down(kbd: &Hidkbd, r: &mut [u8]) {
    hidkbd_apple_translate(kbd, r, &[tr(82, 81)]);
}

#[test]
fn munge_runs_before_decoding() {
    let _g = setup_real_memory();
    let kbd = kbd_for(QEMU_KBD);
    kbd.sc_munge.set(Some(rewrite_up_to_down));
    assert_eq!(poll(&kbd, &[0, 0, 82, 0, 0, 0, 0, 0]), [(DOWN, 81)]);
    assert_eq!(hidkbd_detach(&kbd, 0), Ok(()));
}

#[test]
fn the_debounce_quirk_holds_decoding_back() {
    let _g = setup_real_memory();
    // the debounce arms a timeout: the wheel is global, under its test lock
    let _t = crate::kern::kern_timeout::tests::LOCK
        .lock()
        .unwrap_or_else(|e| e.into_inner());
    crate::kern::kern_timeout::timeout_startup();
    let dev = fake_device();
    let kbd = Hidkbd::new();
    assert_eq!(
        hidkbd_attach(dev, &kbd, 0, HIDKBD_SPUR_BUT_UP, 0, QEMU_KBD),
        Ok(())
    );
    assert_eq!(kbd.sc_debounce.get(), 1);
    // not polling: the report waits in sc_data, nothing is decoded yet
    hidkbd_input(&kbd, &mut [0, 0, 4, 0, 0, 0, 0, 0]);
    assert_eq!(kbd.sc_data.get().keycode[0], 4);
    assert_eq!(kbd.sc_odata.get().keycode[0], 0);
    // the timeout decodes it (run by hand, the wheel is not ticking: take it off the wheel)
    assert!(crate::sys::timeout::timeout_pending(&kbd.sc_delay));
    assert!(crate::kern::kern_timeout::timeout_del(&kbd.sc_delay));
    hidkbd_delayed_decode(ptr::from_ref(&kbd).cast_mut().cast());
    assert_eq!(kbd.sc_odata.get().keycode[0], 4);
    // while polling there is no holding back
    kbd.sc_polling.set(1);
    hidkbd_input(&kbd, &mut [0, 0, 5, 0, 0, 0, 0, 0]);
    assert_eq!(kbd.sc_npollchar.get(), 2);
    assert_eq!(hidkbd_detach(&kbd, 0), Ok(()));
}

#[test]
fn odd_descriptors_are_parsed_like_the_c_does() {
    let _g = setup_real_memory();
    // 200 one-bit variable keys: capped at MAXVARS
    let d: &[u8] = &[
        0x05, 0x01, 0x09, 0x06, 0xa1, 0x01, 0x05, 0x07, 0x19, 0x00, 0x29, 0xc7, 0x75, 0x01, 0x95,
        0xc8, 0x81, 0x02, 0xc0,
    ];
    let kbd = Hidkbd::new();
    assert_eq!(hidkbd_parse_desc(&kbd, 0, d), Ok(()));
    assert_eq!(kbd.sc_nvar.get(), MAXVARS as u32);
    assert_eq!(kbd.sc_nkeycode.get(), 0);
    assert_eq!(kbd.vars()[127].mask, 1 << 7);
    assert_eq!(kbd.vars()[9].key, 9);
    assert_eq!(hidkbd_detach(&kbd, 0), Ok(()));

    // a variable key that is not one bit wide is skipped but still counted: its slot stays
    // zero (mask 0), which never produces an event
    let d: &[u8] = &[
        0x05, 0x01, 0x09, 0x06, 0xa1, 0x01, 0x05, 0x07, 0x19, 0xe0, 0x29, 0xe0, 0x75, 0x01, 0x95,
        0x01, 0x81, 0x02, 0x19, 0xe1, 0x29, 0xe1, 0x75, 0x02, 0x81, 0x02, 0xc0,
    ];
    let kbd = Hidkbd::new();
    assert_eq!(hidkbd_parse_desc(&kbd, 0, d), Ok(()));
    assert_eq!(kbd.sc_nvar.get(), 2);
    assert_eq!(kbd.vars()[0].key, 0xe0);
    assert_eq!(kbd.vars()[1].mask, 0);
    assert_eq!(hidkbd_detach(&kbd, 0), Ok(()));

    // an array that is not 8 bits wide, one off a byte boundary, a second array and more than
    // MAXKEYCODE codes: only the first byte-aligned 8-bit array counts, capped at 6
    let d: &[u8] = &[
        0x05, 0x01, 0x09, 0x06, 0xa1, 0x01, 0x05, 0x07, 0x19, 0x00, 0x29, 0xff, 0x75, 0x04, 0x95,
        0x02, 0x81, 0x00, // 4-bit array: skipped
        0x75, 0x01, 0x95, 0x01, 0x81,
        0x01, // constant bit: shifts the next array off the byte
        0x75, 0x08, 0x95, 0x02, 0x81, 0x00, // array at bit 9: skipped
        0x95, 0x08, 0x81, 0x00, // 8 key codes at bit 25: also off a byte
        0xc0,
    ];
    let kbd = Hidkbd::new();
    assert_eq!(
        hidkbd_parse_desc(&kbd, 0, d),
        Err("no usable key codes array")
    );
    let d: &[u8] = &[
        0x05, 0x01, 0x09, 0x06, 0xa1, 0x01, 0x05, 0x07, 0x19, 0x00, 0x29, 0xff, 0x75, 0x08, 0x95,
        0x08, 0x81, 0x00, 0x95, 0x02, 0x81, 0x00, 0xc0,
    ];
    let kbd = Hidkbd::new();
    assert_eq!(hidkbd_parse_desc(&kbd, 0, d), Ok(()));
    assert_eq!(kbd.sc_nkeycode.get(), 6);
    assert_eq!(
        kbd.sc_keycodeloc.get(),
        HidLocation {
            size: 8,
            count: 8,
            pos: 0
        }
    );
    assert_eq!(hidkbd_detach(&kbd, 0), Ok(()));
}

#[test]
fn the_keymap_data_starts_empty_and_takes_the_layout() {
    use crate::dev::wscons::wsksymdef::KB_US;
    assert!(UKBD_KEYMAPDATA.keydesc.is_empty());
    UKBD_KEYMAPDATA.set_layout(KB_US);
    assert_eq!(UKBD_KEYMAPDATA.layout(), KB_US);
}
