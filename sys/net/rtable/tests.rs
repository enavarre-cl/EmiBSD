//! Host tests for the routing tables: masks to prefix lengths, the table maps, and routes
//! inserted, matched, listed and deleted through an interface address.

use std::vec::Vec;

use super::*;
use crate::netinet::in_::{SockaddrIn, sintosa};
use crate::sys::socket::AF_INET;

/// A `sockaddr_in` for `a` with length `len`.
fn sin(a: [u8; 4], len: u8) -> SockaddrIn {
    SockaddrIn {
        sin_len: len,
        sin_family: AF_INET,
        sin_addr: crate::netinet::in_::InAddr {
            s_addr: u32::from_ne_bytes(a),
        },
        ..SockaddrIn::default()
    }
}

fn plen(mask: Option<SockaddrIn>) -> Result<u32, Errno> {
    match mask {
        // SAFETY: a local `sockaddr_in`.
        Some(mut m) => unsafe { rtable_satoplen(AF_INET, sintosa(&mut m)) },
        // SAFETY: a NULL mask is a host route.
        None => unsafe { rtable_satoplen(AF_INET, ptr::null()) },
    }
}

#[test]
fn masks_become_prefix_lengths() {
    let _g = crate::netinet::ip_input::tests::setup();
    assert_eq!(plen(None), Ok(32), "host route");
    assert_eq!(plen(Some(sin([0; 4], 0))), Ok(0), "sa_len 0: default route");
    assert_eq!(plen(Some(sin([0; 4], 16))), Ok(0));
    assert_eq!(plen(Some(sin([255, 255, 255, 0], 16))), Ok(24));
    assert_eq!(
        plen(Some(sin([255, 255, 255, 0], 7))),
        Ok(24),
        "trimmed by in_socktrim"
    );
    assert_eq!(plen(Some(sin([255, 255, 254, 0], 16))), Ok(23));
    assert_eq!(plen(Some(sin([255, 128, 0, 0], 16))), Ok(9));
    assert_eq!(plen(Some(sin([255, 255, 255, 255], 16))), Ok(32));
    assert_eq!(
        plen(Some(sin([255, 0, 255, 0], 16))),
        Err(Errno::EINVAL),
        "non contiguous"
    );
    assert_eq!(plen(Some(sin([255, 0x7f, 0, 0], 16))), Err(Errno::EINVAL));
    // SAFETY: a NULL mask.
    assert_eq!(
        unsafe { rtable_satoplen(crate::sys::socket::AF_UNIX, ptr::null()) },
        Err(Errno::EINVAL)
    );
}

#[test]
fn table_zero_exists_in_routing_domain_zero() {
    let _g = crate::netinet::ip_input::tests::setup();
    assert!(rtable_exists(0));
    assert!(!rtable_exists(1));
    assert!(rtable_get(0, AF_INET).is_some());
    assert!(rtable_get(0, crate::sys::socket::AF_UNIX).is_none());
    assert_eq!(rtable_l2(0), 0);

    // A new table, in rdomain 0 with lo0 until rtable_l2set.
    rtable_add(3).expect("rtable_add");
    assert!(rtable_exists(3) && !rtable_exists(2));
    assert!(rtable_empty(3));
    assert_eq!(rtable_l2(3), 0);
    rtable_l2set(3, 3, 9);
    assert_eq!(rtable_l2(3), 3);
    assert_eq!(rtable_loindex(3), 9);
    assert_eq!(rtable_add(RT_TABLEID_MAX + 1), Err(Errno::EINVAL));
}

#[test]
fn routes_of_an_address_are_matched_listed_and_deleted() {
    let _g = crate::netinet::ip_input::tests::setup();
    let ifp = crate::netinet::ip_input::tests::test_ether();
    crate::netinet::ip_input::tests::configure(ifp, [10, 0, 2, 15], [255, 255, 255, 0]);

    // The routes in_ifinit made, by walking the table.
    let mut seen: Vec<(Vec<u8>, i32, u32)> = Vec::new();
    rtable_walk(0, AF_INET, None, |rt, _| {
        // SAFETY: a route's key is a `sockaddr_in` here.
        let key = unsafe { (*crate::netinet::in_::satosin_const(rt_key(rt))).sin_addr };
        seen.push((
            key.s_addr.to_ne_bytes().to_vec(),
            rt_plen(rt),
            rt.rt_flags.get(),
        ));
        Ok(())
    })
    .expect("walk");
    let has = |a: [u8; 4], plen: i32| seen.iter().any(|(k, p, _)| *k == a && *p == plen);
    assert!(has([10, 0, 2, 15], 32), "local route: {seen:?}");
    assert!(has([10, 0, 2, 0], 24), "prefix route: {seen:?}");
    assert!(has([10, 0, 2, 255], 32), "broadcast route: {seen:?}");

    // rtable_read stops at the first error.
    let mut n = 0;
    assert_eq!(
        rtable_read(0, AF_INET, |_, _| {
            n += 1;
            Err(Errno::EEXIST)
        }),
        Err(Errno::EEXIST)
    );
    assert_eq!(n, 1);

    // Longest prefix: an address of the subnet matches the cloning route, a perfect lookup of
    // the /24 finds it too, a /25 does not exist.
    let mut a = sin([10, 0, 2, 77], 16);
    // SAFETY: local `sockaddr_in`s.
    let rt = unsafe { rtable_match(0, sintosa(&mut a), None) }.expect("match");
    assert_eq!(rt_plen(rt), 24);
    crate::net::route::rtfree(Some(rt));
    let mut net = sin([10, 0, 2, 0], 16);
    let mut m24 = sin([255, 255, 255, 0], 16);
    let mut m25 = sin([255, 255, 255, 128], 16);
    // SAFETY: local `sockaddr_in`s.
    unsafe {
        let rt = rtable_lookup(
            0,
            sintosa(&mut net),
            sintosa(&mut m24),
            ptr::null(),
            RTP_ANY,
        )
        .expect("perfect lookup");
        crate::net::route::rtfree(Some(rt));
        assert!(
            rtable_lookup(
                0,
                sintosa(&mut net),
                sintosa(&mut m25),
                ptr::null(),
                RTP_ANY
            )
            .is_none()
        );
    }

    // Removing the address empties the table.
    crate::netinet::ip_input::tests::unconfigure(ifp, [10, 0, 2, 15]);
    assert!(rtable_empty(0));
}
