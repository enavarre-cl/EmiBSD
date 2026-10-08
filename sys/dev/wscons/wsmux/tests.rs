//! Host tests of the mux: creating muxes, stacking them, the device list ioctls, opening a
//! tree, injecting an event, and handing a display down to the children.
//!
//! Muxes are global and never freed, so each test uses mux numbers of its own (10 and up;
//! 0 and 1 are the ones `/dev/wsmouse`, `/dev/wskbd` and wsdisplay use).

use std::boxed::Box;
use std::vec;

use super::*;
use crate::dev::wscons::wsksymdef::{KB_DE, KB_US};
use crate::kern::subr_pool::tests::setup_real_memory;
use crate::sys::uio::{Iovec, UioRw, UioSeg};

/// The mux's own device.
fn dv(sc: &WsmuxSoftc) -> &Device {
    &sc.sc_base.me_dv
}

/// `WSMUXIO_LIST_DEVICES` on `sc`: the (type, unit) pairs.
fn list(sc: &WsmuxSoftc) -> std::vec::Vec<(i32, i32)> {
    let mut data = [0u8; size_of::<WsmuxDeviceList>()];
    wsmux_do_ioctl(dv(sc), WSMUXIO_LIST_DEVICES, &mut data, FREAD, None).unwrap();
    let l: WsmuxDeviceList = ioctl_arg(&data);
    l.devices[..l.ndevices as usize]
        .iter()
        .map(|d| (d.type_, d.idx))
        .collect()
}

/// A `WSMUXIO_ADD_DEVICE`/`WSMUXIO_REMOVE_DEVICE` on `sc`.
fn dev_ioctl(sc: &WsmuxSoftc, cmd: u64, type_: i32, idx: i32, flag: i32) -> Result<(), Errno> {
    let mut data = [0u8; size_of::<WsmuxDevice>()];
    ioctl_ret(&mut data, &WsmuxDevice { type_, idx });
    wsmux_do_ioctl(dv(sc), cmd, &mut data, flag, None)
}

#[test]
fn getmux_creates_each_mux_once() {
    let _g = setup_real_memory();
    assert!(wsmux_getmux(-1).is_none());
    assert!(wsmux_getmux(WSMUX_MAXDEV as i32).is_none());
    let a = wsmux_getmux(10).unwrap();
    assert!(ptr::eq(a, wsmux_getmux(10).unwrap()));
    assert_eq!(a.sc_base.me_dv.xname(), "wsmux10");
    assert_eq!(a.sc_base.me_dv.dv_unit.get(), 10);
    assert_eq!(a.sc_base.ops().type_, WSMUX_MUX);
    assert_eq!(wsmux_get_layout(a), KB_NONE);
    assert!(wsmux_lookup(10).is_some());

    // A driver's default layout or one without an encoding never becomes the mux's.
    wsmux_set_layout(a, KB_DE | KB_DEFAULT);
    assert_eq!(wsmux_get_layout(a), KB_NONE);
    wsmux_set_layout(a, KB_US | KB_NOENCODING);
    assert_eq!(wsmux_get_layout(a), KB_NONE);
    wsmux_set_layout(a, KB_DE);
    assert_eq!(wsmux_get_layout(a), KB_DE);
}

#[test]
fn muxes_stack_without_loops() {
    let _g = setup_real_memory();
    let (m11, m12, m13) = (
        wsmux_getmux(11).unwrap(),
        wsmux_getmux(12).unwrap(),
        wsmux_getmux(13).unwrap(),
    );

    assert_eq!(
        dev_ioctl(m11, WSMUXIO_ADD_DEVICE, WSMUX_MUX, 12, FWRITE),
        Ok(())
    );
    assert!(m12.sc_base.parent().is_some_and(|p| ptr::eq(p, m11)));
    assert_eq!(list(m11), [(WSMUX_MUX, 12)]);
    rw_enter_read(&WSMUX_TREE_LOCK);
    assert_eq!(wsmux_depth(m11), 2);
    rw_exit_read(&WSMUX_TREE_LOCK);

    // 11 is above 12: adding it below 12 would make a loop.
    assert_eq!(wsmux_add_mux(11, m12), Err(Errno::EINVAL));
    // 12 has a parent already.
    assert_eq!(wsmux_add_mux(12, m13), Err(Errno::EBUSY));

    assert_eq!(
        dev_ioctl(m11, WSMUXIO_REMOVE_DEVICE, WSMUX_MUX, 12, FREAD),
        Err(Errno::EACCES)
    );
    assert_eq!(
        dev_ioctl(m11, WSMUXIO_REMOVE_DEVICE, WSMUX_MUX, 12, FWRITE),
        Ok(())
    );
    assert!(list(m11).is_empty());
    assert!(m12.sc_base.parent().is_none());
    assert_eq!(
        dev_ioctl(m11, WSMUXIO_REMOVE_DEVICE, WSMUX_MUX, 12, FWRITE),
        Err(Errno::EINVAL)
    );
}

#[test]
fn add_device_by_type() {
    let _g = setup_real_memory();
    let m = wsmux_getmux(14).unwrap();
    let add = |type_, idx| dev_ioctl(m, WSMUXIO_ADD_DEVICE, type_, idx, FWRITE);
    assert_eq!(add(WSMUX_KBD, -1), Err(Errno::ENXIO));
    assert_eq!(add(WSMUX_KBD, 7), Err(Errno::ENXIO), "no such keyboard");
    assert_eq!(add(WSMUX_MOUSE, 31), Err(Errno::ENXIO), "no such mouse");
    assert_eq!(add(9, 0), Err(Errno::EINVAL));
    assert_eq!(
        dev_ioctl(m, WSMUXIO_ADD_DEVICE, WSMUX_MUX, 15, FREAD),
        Err(Errno::EACCES)
    );
    // Neither open nor a display's input: other ioctls are refused.
    let mut data = [0u8; 4];
    assert_eq!(
        wsmux_do_ioctl(dv(m), WSKBDIO_SETMODE + 1, &mut data, FWRITE, None),
        Err(Errno::EACCES)
    );
}

#[test]
fn an_open_tree_shares_the_root_queue() {
    let _g = setup_real_memory();
    let (root, child) = (wsmux_getmux(16).unwrap(), wsmux_getmux(17).unwrap());
    wsmux_add_mux(17, root).unwrap();

    let evar = &root.sc_base.me_evar;
    wsevent_init(evar).unwrap();
    wsmux_do_open(root, evar).unwrap();
    assert!(child.sc_base.evp().is_some_and(|e| ptr::eq(e, evar)));
    assert_eq!(wsmux_do_open(root, evar), Err(Errno::EBUSY));

    // FIOASYNC, then an injected event reaches the root's queue with a fresh time stamp.
    let mut on = 1i32.to_ne_bytes();
    wsmux_do_ioctl(dv(root), FIOASYNC, &mut on, FREAD, None).unwrap();
    assert_eq!(evar.ws_async.get(), 1);
    let mut data = [0u8; size_of::<WsconsEvent>()];
    let e = WsconsEvent {
        type_: 2,
        value: 42,
        time: crate::sys::time::Timespec::default(),
    };
    ioctl_ret(&mut data, &e);
    assert_eq!(
        wsmux_do_ioctl(dv(root), WSMUXIO_INJECTEVENT, &mut data, FREAD, None),
        Err(Errno::EACCES)
    );
    evar.ws_async.set(0); // no SIGIO owner on the host
    wsmux_do_ioctl(dv(root), WSMUXIO_INJECTEVENT, &mut data, FWRITE, None).unwrap();
    assert_eq!(evar.ws_put.get(), 1);

    let mut buf = vec![0u8; size_of::<WsconsEvent>()];
    let mut iov = [Iovec {
        iov_base: buf.as_mut_ptr().cast(),
        iov_len: buf.len(),
    }];
    let mut uio = Uio {
        uio_iov: &mut iov,
        uio_offset: 0,
        uio_resid: size_of::<WsconsEvent>(),
        uio_segflg: UioSeg::UIO_SYSSPACE,
        uio_rw: UioRw::UIO_READ,
        uio_procp: None,
    };
    wsevent_read(evar, &mut uio, crate::sys::vnode::IO_NDELAY).unwrap();
    let got: WsconsEvent = ioctl_arg(&buf);
    assert_eq!((got.type_, got.value), (2, 42));

    wsmux_do_close(root);
    root.sc_base.me_evp.set(None);
    assert!(child.sc_base.evp().is_none());
    wsevent_fini(evar);
}

#[test]
fn a_display_reaches_the_children() {
    let _g = setup_real_memory();
    let (root, child) = (wsmux_getmux(18).unwrap(), wsmux_getmux(19).unwrap());
    wsmux_add_mux(19, root).unwrap();
    // SAFETY: all-zero is a valid `Device` (its documented contract); leaked for good.
    let disp: &'static Device = Box::leak(Box::new(unsafe { core::mem::zeroed::<Device>() }));

    wsmux_set_display(root, Some(disp)).unwrap();
    assert!(root.displaydv().is_some_and(|d| ptr::eq(d, disp)));
    assert!(child.displaydv().is_some_and(|d| ptr::eq(d, disp)));
    assert_eq!(
        wsmux_evsrc_set_display(dv(root), Some(disp)),
        Err(Errno::EBUSY)
    );

    // A display's input answers ioctls (here none of its children takes this one).
    let mut data = [0u8; 4];
    assert_eq!(
        wsmux_do_displayioctl(dv(root), WSKBDIO_SETMODE + 1, &mut data, FWRITE, None),
        Ok(false)
    );

    wsmux_set_display(root, None).unwrap();
    assert!(root.displaydv().is_none());
    assert!(child.displaydv().is_none());
    assert_eq!(wsmux_evsrc_set_display(dv(root), None), Err(Errno::ENXIO));
}
