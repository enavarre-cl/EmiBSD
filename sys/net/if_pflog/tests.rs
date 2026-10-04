//! Host tests for `pflog(4)`: the `pfloghdr` layout and cloning `pflog` interfaces.

use super::*;
use crate::net::if_::tests::{setup_net, test_packet};
use crate::net::if_::{if_clone_create, if_put, if_unit};

#[test]
fn pfloghdr_has_the_c_layout() {
    // sizeof(struct pfloghdr) and offsetof(..., pad) as clang computes them for amd64 and
    // arm64.
    assert_eq!(PFLOG_HDRLEN, 100);
    assert_eq!(PFLOG_REAL_HDRLEN, PFLOG_HDRLEN);
    assert_eq!(PFLOG_OLD_HDRLEN, 63);
    assert_eq!(core::mem::offset_of!(Pfloghdr, ifname), 4);
    assert_eq!(core::mem::offset_of!(Pfloghdr, ruleset), 20);
    assert_eq!(core::mem::offset_of!(Pfloghdr, rulenr), 36);
    assert_eq!(core::mem::offset_of!(Pfloghdr, rule_pid), 56);
    assert_eq!(core::mem::offset_of!(Pfloghdr, dir), 60);
    assert_eq!(core::mem::offset_of!(Pfloghdr, saddr), 64);
    assert_eq!(core::mem::offset_of!(Pfloghdr, daddr), 80);
    assert_eq!(core::mem::offset_of!(Pfloghdr, dport), 98);
}

#[test]
fn pflog_interfaces_are_cloned() {
    let _g = setup_net();
    // The interface queues name their softnet task queue (no thread runs it here).
    crate::net::if_::softnet_init();
    static ONCE: std::sync::Once = std::sync::Once::new();
    ONCE.call_once(|| pflogattach(1));

    assert_eq!(pflog_clone_create(&PFLOG_CLONER, 0), Ok(()));
    let ifp = if_unit(b"pflog0").expect("pflog0");
    assert_eq!(ifp.if_type.get(), IFT_PFLOG);
    assert_eq!(ifp.if_mtu.get(), PFLOGMTU);
    assert_eq!(usize::from(ifp.if_hdrlen.get()), PFLOG_HDRLEN);
    assert_eq!(ifp.if_xflags.get() & IFXF_CLONED, IFXF_CLONED);
    if_put(ifp);

    net_lock();
    let sc = pflog_getif(0).expect("unit 0");
    assert!(ptr::eq(&sc.sc_if, ifp));
    assert!(pflog_getif(7).is_none());
    net_unlock();

    ifp.if_flags.set(ifp.if_flags.get() | IFF_UP);
    // SAFETY: SIOCSIFFLAGS reads no argument.
    let r = unsafe { pflogioctl(ifp, SIOCSIFFLAGS, ptr::null_mut()) };
    assert_eq!(r, Ok(()));
    assert_ne!(ifp.if_flags.get() & IFF_RUNNING, 0);
    ifp.if_flags.set(ifp.if_flags.get() & !IFF_UP);
    // SAFETY: as above.
    let r = unsafe { pflogioctl(ifp, SIOCSIFFLAGS, ptr::null_mut()) };
    assert_eq!(r, Ok(()));
    assert_eq!(ifp.if_flags.get() & IFF_RUNNING, 0);
    // SAFETY: as above.
    let r = unsafe { pflogioctl(ifp, 0, ptr::null_mut()) };
    assert_eq!(r, Err(Errno::ENOTTY));

    let m = test_packet(&[0x45, 0, 0, 20]);
    // SAFETY: `pflogoutput` reads no destination.
    let r = unsafe { pflogoutput(ifp, m, ptr::null(), None) };
    assert_eq!(r, Err(Errno::EAFNOSUPPORT));

    // The cloner makes more units, by name.
    assert_eq!(if_clone_create(b"pflog1", 0), Ok(()));
    assert_eq!(if_clone_create(b"pflog1", 0), Err(Errno::EEXIST));
    net_lock();
    assert_eq!(pflog_getif(1).map(|sc| sc.sc_unit.get()), Some(1));
    net_unlock();

    // pflog_clone_destroy runs if_detach, which sleeps on the interface's references and
    // task barriers: there is no process to sleep on the host, so destruction is left to
    // the kernel; here the softc is taken off the list as it does first.
    net_lock();
    // SAFETY: pflog_clone_create put the softc on pflog_ifs.
    unsafe { ListHead::<PflogIfs>::remove(sc) };
    assert!(pflog_getif(0).is_none());
    net_unlock();
}
