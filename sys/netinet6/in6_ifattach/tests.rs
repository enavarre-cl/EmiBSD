//! Host tests for the interface identifier code of `in6_ifattach.c`: the EUI-64 of a
//! hardware address, the sanity checks, the borrowed and random identifiers.

use super::*;
use crate::net::if_::tests::test_ifnet;
use crate::net::if_dl::SockaddrDl;
use crate::net::if_types::IFT_PPP;
use crate::netinet6::in6::tests::{a6, setup, test_if};
use crate::sys::systm::{net_lock, net_unlock};

/// A leaked link-level address: `name`, then `lladdr`.
fn leak_sdl(name: &[u8], lladdr: &[u8]) -> *mut SockaddrDl {
    let mut sdl = SockaddrDl {
        sdl_nlen: name.len() as u8,
        sdl_alen: lladdr.len() as u8,
        ..SockaddrDl::default()
    };
    sdl.sdl_data[..name.len()].copy_from_slice(name);
    sdl.sdl_data[name.len()..name.len() + lladdr.len()].copy_from_slice(lladdr);
    std::boxed::Box::leak(std::boxed::Box::new(sdl))
}

/// An interface of type `ty` with the link-level address `lladdr` (after a 4 byte name).
fn if_with_lladdr(name: &[u8], ty: u8, lladdr: &[u8]) -> &'static Ifnet {
    let ifp = test_ifnet(name);
    ifp.if_type.set(ty);
    ifp.if_sadl.set(leak_sdl(b"tsdl", lladdr));
    ifp
}

fn ifid_of(ifp: &Ifnet) -> Option<In6Addr> {
    let mut a = a6("fe80::");
    in6_get_hw_ifid(ifp, &mut a).then_some(a)
}

#[test]
fn ethernet_address_gives_an_eui64_identifier() {
    // 52:54:00:bb:00:02 -> fe80::5054:ff:febb:2 (the "u" bit flipped, ff:fe in the middle).
    let ifp = if_with_lladdr(b"te0", IFT_ETHER, &[0x52, 0x54, 0x00, 0xbb, 0x00, 0x02]);
    let ifid = ifid_of(ifp).expect("an identifier");
    assert_eq!(
        &ifid.s6_addr[8..],
        [0x50, 0x54, 0x00, 0xff, 0xfe, 0xbb, 0x00, 0x02]
    );
    // The upper 64 bits are preserved.
    assert_eq!(&ifid.s6_addr[..8], &a6("fe80::").s6_addr[..8]);
    let mut ll = ifid;
    ll.set_s6_addr16(1, 0);
    assert_eq!(ll, a6("fe80::5054:ff:febb:2"));

    // A locally administered ("u" bit set) address becomes universal-looking and back.
    let ifp = if_with_lladdr(b"te1", IFT_ETHER, &[0x02, 0x00, 0x00, 0x00, 0x00, 0x01]);
    let ifid = ifid_of(ifp).expect("an identifier");
    assert_eq!(ifid.s6_addr[8], 0x00);
    assert_eq!(&ifid.s6_addr[11..13], [0xff, 0xfe]);
}

#[test]
fn eight_byte_addresses_are_used_as_they_are() {
    let eui = [0x02, 0x11, 0x22, 0xff, 0xfe, 0x33, 0x44, 0x55];
    for ty in [IFT_IEEE1394, IFT_IEEE80211] {
        let ifp = if_with_lladdr(b"tf0", ty, &eui);
        let ifid = ifid_of(ifp).expect("an identifier");
        assert_eq!(ifid.s6_addr[8], 0x00);
        assert_eq!(&ifid.s6_addr[9..], &eui[1..]);
    }
    // IEEE1394 and 802.11 longer than 8 bytes: the first 8.
    let long = [
        0x02, 0x11, 0x22, 0xff, 0xfe, 0x33, 0x44, 0x55, 9, 9, 9, 9, 9, 9, 9, 9,
    ];
    let ifp = if_with_lladdr(b"tf1", IFT_IEEE1394, &long);
    let ifid = ifid_of(ifp).expect("an identifier");
    assert_eq!(&ifid.s6_addr[9..], &eui[1..]);
    // But an Ethernet-type 16 byte address is no EUI.
    let ifp = if_with_lladdr(b"tf2", IFT_ETHER, &long);
    assert!(ifid_of(ifp).is_none());
}

#[test]
fn unusable_hardware_addresses_give_no_identifier() {
    // No link-level address.
    let ifp = test_ifnet(b"tn0");
    ifp.if_type.set(IFT_ETHER);
    assert!(ifid_of(ifp).is_none());
    let ifp = if_with_lladdr(b"tn1", IFT_ETHER, &[]);
    assert!(ifid_of(ifp).is_none());
    // All zero and all ones.
    assert!(ifid_of(if_with_lladdr(b"tn2", IFT_ETHER, &[0; 6])).is_none());
    assert!(ifid_of(if_with_lladdr(b"tn3", IFT_ETHER, &[0xff; 6])).is_none());
    assert!(ifid_of(if_with_lladdr(b"tn4", IFT_IEEE80211, &[0; 8])).is_none());
    assert!(ifid_of(if_with_lladdr(b"tn5", IFT_IEEE1394, &[0xff; 8])).is_none());
    // The group bit set: a multicast address.
    assert!(ifid_of(if_with_lladdr(b"tn6", IFT_ETHER, &[0x01, 0, 0x5e, 0, 0, 1])).is_none());
    // An identifier that would be all zero (subnet router anycast): u bit only.
    assert!(
        ifid_of(if_with_lladdr(
            b"tn7",
            IFT_IEEE1394,
            &[0x02, 0, 0, 0, 0, 0, 0, 0]
        ))
        .is_none()
    );
    // The wrong length for an Ethernet.
    assert!(ifid_of(if_with_lladdr(b"tn8", IFT_ETHER, &[1, 2, 3, 4, 5])).is_none());
    // Interface types that have none: gif (RFC 2893 says to borrow, the C does not) and PPP.
    assert!(
        ifid_of(if_with_lladdr(
            b"tn9",
            IFT_GIF,
            &[0x52, 0x54, 0, 0xbb, 0, 2]
        ))
        .is_none()
    );
    assert!(
        ifid_of(if_with_lladdr(
            b"tna",
            IFT_PPP,
            &[0x52, 0x54, 0, 0xbb, 0, 2]
        ))
        .is_none()
    );
    // CARP uses the Ethernet rules.
    assert!(
        ifid_of(if_with_lladdr(
            b"tnb",
            IFT_CARP,
            &[0x00, 0x00, 0x5e, 0x00, 0x01, 0x01]
        ))
        .is_some()
    );
}

#[test]
fn random_identifier_is_local_and_individual() {
    let ifp = test_ifnet(b"tr0");
    for _ in 0..32 {
        let mut a = a6("fe80:1::");
        in6_get_rand_ifid(ifp, &mut a);
        assert_eq!(&a.s6_addr[..8], &a6("fe80:1::").s6_addr[..8]);
        // "u" local and "g" individual in the EUI-64, then flipped by EUI64_TO_IFID:
        // both bits clear in the interface identifier.
        assert_eq!(a.s6_addr[8] & (EUI64_GBIT | EUI64_UBIT), 0);
    }
}

#[test]
fn identifier_is_borrowed_from_another_interface_or_random() {
    let _g = setup();
    // No hardware address on the first interface: it borrows the second's.
    let a = test_if(b"tb0");
    a.if_type.set(IFT_PPP);
    let b = test_if(b"tb1");
    b.if_type.set(IFT_ETHER);
    let mac = [0x52, 0x54, 0x00, 0xbb, 0x00, 0x02];
    b.if_sadl.set(leak_sdl(b"", &mac));

    net_lock();
    let mut id = a6("fe80::");
    in6_get_ifid(a, &mut id);
    assert_eq!(
        &id.s6_addr[8..],
        [0x50, 0x54, 0x00, 0xff, 0xfe, 0xbb, 0x00, 0x02]
    );
    // Its own hardware address first.
    let mut id = a6("fe80::");
    in6_get_ifid(b, &mut id);
    assert_eq!(
        &id.s6_addr[8..],
        [0x50, 0x54, 0x00, 0xff, 0xfe, 0xbb, 0x00, 0x02]
    );
    // With nobody to borrow from: random.
    b.if_type.set(IFT_PPP);
    let mut id = a6("fe80::");
    in6_get_ifid(a, &mut id);
    assert_eq!(id.s6_addr[8] & (EUI64_GBIT | EUI64_UBIT), 0);
    net_unlock();
}
