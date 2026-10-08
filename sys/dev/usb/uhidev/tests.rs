//! Host tests for `uhidev`: the report ID split of `uhidev_intr`, the descriptor's largest
//! report ID, the replacement descriptors `uhidev_use_rdesc` hands out, the generic HID
//! ioctls, and the child table (`uhidev_set_report_dev`).

use std::boxed::Box;
use std::mem::MaybeUninit;
use std::vec::Vec;
use std::{assert, assert_eq};

use super::*;
use crate::dev::hid::hid::hid_collection;
use crate::dev::usb::usb::UsbInterfaceDescriptor;
use crate::kern::subr_pool::tests::setup_real_memory;

/// QEMU's `usb-kbd` report descriptor: one report, no report ID.
const QEMU_KBD: &[u8] = &[
    0x05, 0x01, 0x09, 0x06, 0xa1, 0x01, 0x75, 0x01, 0x95, 0x08, 0x05, 0x07, 0x19, 0xe0, 0x29, 0xe7,
    0x15, 0x00, 0x25, 0x01, 0x81, 0x02, 0x95, 0x01, 0x75, 0x08, 0x81, 0x01, 0x95, 0x05, 0x75, 0x01,
    0x05, 0x08, 0x19, 0x01, 0x29, 0x05, 0x91, 0x02, 0x95, 0x01, 0x75, 0x03, 0x91, 0x01, 0x95, 0x06,
    0x75, 0x08, 0x15, 0x00, 0x25, 0xff, 0x05, 0x07, 0x19, 0x00, 0x29, 0xff, 0x81, 0x00, 0xc0,
];

/// A zeroed softc, as `config_make_softc` makes one, that lives as long as the test needs.
fn zeroed<T: Softc>() -> &'static T {
    // SAFETY: the `Softc` contract: all-zero is a valid `T`.
    Box::leak(Box::new(unsafe {
        MaybeUninit::<T>::zeroed().assume_init()
    }))
}

#[test]
fn the_report_id_is_the_first_byte_unless_there_is_only_one() {
    // One report ID: the device sends no ID byte.
    assert_eq!(uhidev_split_report(1, &[1, 2, 3]), Some((0, 0)));
    assert_eq!(uhidev_split_report(1, &[]), Some((0, 0)));
    // Several: the first byte is the ID, and the report starts after it.
    assert_eq!(uhidev_split_report(3, &[2, 0xaa, 0xbb]), Some((2, 1)));
    assert_eq!(uhidev_split_report(3, &[0, 0xaa]), Some((0, 1)));
    // A transfer without the ID byte (the C's `cc--` would wrap).
    assert_eq!(uhidev_split_report(3, &[]), None);
}

#[test]
fn maxrepid_of_a_descriptor() {
    assert_eq!(uhidev_maxrepid(QEMU_KBD), 0);
    assert_eq!(uhidev_maxrepid(&[]), -1);
    // REPORT_ID (5) in an otherwise minimal collection.
    assert_eq!(
        uhidev_maxrepid(&[
            0x05, 0x01, 0x09, 0x06, 0xa1, 0x01, 0x85, 0x05, 0x75, 0x08, 0x95, 0x01, 0x81, 0x02,
            0xc0
        ]),
        5
    );
}

#[test]
fn report_types_map_to_the_uhid_numbers() {
    assert_eq!(uhidev_report_type_conv(hid_input), Some(UHID_INPUT_REPORT));
    assert_eq!(
        uhidev_report_type_conv(hid_output),
        Some(UHID_OUTPUT_REPORT)
    );
    assert_eq!(
        uhidev_report_type_conv(hid_feature),
        Some(UHID_FEATURE_REPORT)
    );
    assert_eq!(uhidev_report_type_conv(hid_collection), None);
    assert_eq!(uhidev_report_type_conv(hid_all), None);
}

#[test]
fn a_child_offered_one_report_id_does_not_claim_several() {
    let sc: &'static UhidevSoftc = zeroed();
    let uaa: &'static UsbAttachArg = {
        // SAFETY: only the `Option`s, integers and raw pointers of the argument are read;
        // all-zero is `None`, 0 and null.
        Box::leak(Box::new(unsafe {
            MaybeUninit::<UsbAttachArg>::zeroed().assume_init()
        }))
    };
    let mut claimed = [0u8; 2];
    let mut uha = UhidevAttachArg {
        uaa,
        parent: sc,
        reportid: 0,
        nreports: 2,
        claimed: core::ptr::null_mut(),
    };
    assert!(!uha.claim_multiple_reportid());
    uha.claimed = claimed.as_mut_ptr();
    assert!(uha.claim_multiple_reportid());
}

fn iface(class: u8, sub: u8, proto: u8) -> UsbInterfaceDescriptor {
    UsbInterfaceDescriptor {
        bLength: 9,
        bInterfaceClass: class,
        bInterfaceSubClass: sub,
        bInterfaceProtocol: proto,
        ..UsbInterfaceDescriptor::default()
    }
}

/// The bytes `uhidev_use_rdesc` allocated, freed.
fn take(r: Result<Option<(NonNull<u8>, usize)>, Errno>) -> Option<Vec<u8>> {
    let (p, n) = r.unwrap()?;
    // SAFETY: a fresh `n`-byte `M_USBDEV` allocation, initialised by the copy.
    let v = unsafe { slice::from_raw_parts(p.as_ptr(), n) }.to_vec();
    free(p, M_USBDEV, n);
    Some(v)
}

#[test]
fn broken_or_missing_descriptors_are_replaced() {
    let _g = setup_real_memory();
    let sc: &UhidevSoftc = zeroed();
    let hid = iface(UICLASS_HID, 1, 1);

    // The Wacom Graphire has one, the Graphire3 sends a SET_REPORT first (needs a device:
    // not here).
    let d = take(uhidev_use_rdesc(
        sc,
        &hid,
        i32::from(USB_VENDOR_WACOM),
        i32::from(USB_PRODUCT_WACOM_GRAPHIRE),
    ))
    .unwrap();
    assert_eq!(d, UHID_GRAPHIRE_REPORT_DESCR);

    // A Wacom product with no replacement, and any other HID device: none.
    assert_eq!(
        take(uhidev_use_rdesc(
            sc,
            &hid,
            i32::from(USB_VENDOR_WACOM),
            0x1234
        )),
        None
    );
    assert_eq!(take(uhidev_use_rdesc(sc, &hid, 0x0627, 1)), None);

    // The Xbox 360 gamepad has no descriptor.
    let x360 = iface(
        UICLASS_VENDOR,
        UISUBCLASS_XBOX360_CONTROLLER,
        UIPROTO_XBOX360_GAMEPAD,
    );
    let d = take(uhidev_use_rdesc(sc, &x360, 0x045e, 0x028e)).unwrap();
    assert_eq!(d, UHID_XB360GP_REPORT_DESCR);
    assert_eq!(sc.sc_flags.get() & UHIDEV_F_XB1, 0);

    // The Xbox One's too, and the softc remembers which one it is.
    let xone = iface(
        UICLASS_VENDOR,
        UISUBCLASS_XBOXONE_CONTROLLER,
        UIPROTO_XBOXONE_GAMEPAD,
    );
    let d = take(uhidev_use_rdesc(sc, &xone, 0x045e, 0x02ea)).unwrap();
    assert_eq!(d, UHID_XBONEGP_REPORT_DESCR);
    assert_eq!(sc.sc_flags.get() & UHIDEV_F_XB1, UHIDEV_F_XB1);
}

/// A child of a parent whose report descriptor is `desc`, for the ioctls.
fn child(desc: &'static [u8], report_id: u8) -> &'static Uhidev {
    let sc: &'static UhidevSoftc = zeroed();
    sc.sc_repdesc.set(desc.as_ptr().cast_mut());
    sc.sc_repdesc_size.set(desc.len() as i32);
    let scd: &'static Uhidev = zeroed();
    scd.sc_parent.set(Some(sc));
    scd.sc_report_id.set(report_id);
    scd.sc_isize.set(8);
    scd
}

#[test]
fn the_generic_hid_ioctls() {
    let p = &crate::kern::init_main::PROC0;
    let scd = child(QEMU_KBD, 3);

    // USB_GET_REPORT_ID
    let mut data = [0u8; 4];
    assert_eq!(
        uhidev_ioctl(scd, USB_GET_REPORT_ID, &mut data, 0, Some(p)),
        Ok(true)
    );
    assert_eq!(ioctl_arg::<i32>(&data), 3);

    // USB_GET_REPORT_DESC: the size and the bytes.
    let mut data = std::vec![0u8; size_of::<UsbCtlReportDesc>()];
    assert_eq!(
        uhidev_ioctl(scd, USB_GET_REPORT_DESC, &mut data, 0, Some(p)),
        Ok(true)
    );
    let rd = ioctl_arg::<UsbCtlReportDesc>(&data);
    assert_eq!(rd.ucrd_size as usize, QEMU_KBD.len());
    assert_eq!(&rd.ucrd_data[..QEMU_KBD.len()], QEMU_KBD);

    // A report type that does not exist.
    let mut data = std::vec![0u8; size_of::<UsbCtlReport>()];
    ioctl_ret(&mut data, &{
        let mut re = ioctl_arg::<UsbCtlReport>(&[]);
        re.ucr_report = 9;
        re
    });
    assert_eq!(
        uhidev_ioctl(scd, USB_GET_REPORT, &mut data, 0, Some(p)),
        Err(Errno::EINVAL)
    );
    assert_eq!(
        uhidev_ioctl(scd, USB_SET_REPORT, &mut data, 0, Some(p)),
        Err(Errno::EINVAL)
    );

    // Not a HID ioctl: the caller goes on (the C's -1).
    assert_eq!(uhidev_ioctl(scd, 0x1234, &mut [], 0, Some(p)), Ok(false));
}

#[test]
fn a_child_gets_a_report_id_only_while_open() {
    let sc: &'static UhidevSoftc = zeroed();
    let subs: &'static mut [Cell<Option<NonNull<Uhidev>>>] = Box::leak(
        (0..3)
            .map(|_| Cell::new(None))
            .collect::<Vec<_>>()
            .into_boxed_slice(),
    );
    sc.sc_subdevs.set(subs.as_mut_ptr());
    sc.sc_nrepid.set(3);
    let dev: &'static Uhidev = zeroed();

    assert_eq!(uhidev_set_report_dev(sc, dev, 1), Err(Errno::ENODEV));
    dev.sc_state.set(UHIDEV_OPEN);
    assert_eq!(uhidev_set_report_dev(sc, dev, 3), Err(Errno::EINVAL));
    assert_eq!(uhidev_set_report_dev(sc, dev, 2), Ok(()));
    assert_eq!(sc.subdevs()[2].get(), Some(NonNull::from(dev)));
    assert_eq!(sc.subdevs()[1].get(), None);

    // `uhidev_attach_repid` leaves a slot assigned this way alone.
    let uaa: &'static UsbAttachArg =
        // SAFETY: as in the test above.
        Box::leak(Box::new(unsafe { MaybeUninit::<UsbAttachArg>::zeroed().assume_init() }));
    let mut uha = UhidevAttachArg {
        uaa,
        parent: sc,
        reportid: 0,
        nreports: 3,
        claimed: core::ptr::null_mut(),
    };
    assert!(!uhidev_attach_repid(sc, &mut uha, 2));
}
