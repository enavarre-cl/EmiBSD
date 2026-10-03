use super::*;
use crate::net::ethertypes::ETHERTYPE_ARP;
use crate::reftest::{assert_complete, assert_defines};
use crate::sys::endian::{htonl, htons};

#[test]
fn frame_layout_and_predicates() {
    let eh = EtherHeader {
        ether_dhost: [0xff; 6],
        ether_shost: [0x52, 0x54, 0x00, 0x12, 0x34, 0x56],
        ether_type: htons(ETHERTYPE_ARP),
    };
    // SAFETY: `EtherHeader` is 14 bytes of integers without padding.
    let bytes: [u8; 14] = unsafe { core::mem::transmute(eh) };
    assert_eq!(&bytes[12..], &[0x08, 0x06]);
    assert!(ether_is_broadcast(&eh.ether_dhost));
    assert!(ether_is_multicast(&eh.ether_dhost));
    assert!(!ether_is_multicast(&eh.ether_shost));
    assert!(ether_is_anyaddr(&[0; 6]));
    assert!(ether_is_eq(
        &eh.ether_shost,
        &[0x52, 0x54, 0x00, 0x12, 0x34, 0x56]
    ));
    assert!(eth64_is_multicast(0x0100_5e00_0001));
    assert!(eth64_is_broadcast(0xffff_ffff_ffff));
    assert!(eth64_is_8021_rsvd(0x0180_c200_000e));
    assert_eq!(ETHERMTU, 1500);
    assert_eq!(ETHERMIN, 46);
    assert_eq!(evl_vlanoftag(0x6064), 0x064);
    assert_eq!(evl_prioftag(0x6064), 3);

    let group = InAddr {
        s_addr: htonl(0xe0ff_0001),
    };
    assert_eq!(
        ether_map_ip_multicast(&group),
        [0x01, 0x00, 0x5e, 0x7f, 0x00, 0x01]
    );
    let mut ip6 = [0u8; 16];
    ip6[12..].copy_from_slice(&[0xff, 0x00, 0x00, 0x01]);
    assert_eq!(
        ether_map_ipv6_multicast(&ip6),
        [0x33, 0x33, 0xff, 0x00, 0x00, 0x01]
    );
    assert_eq!(core::mem::offset_of!(EtherArp, arp_sha), 8);
}

#[test]
#[ignore = "needs OPENBSD_SRC (just test-ref)"]
fn values_match_the_c_header() {
    let defs = crate::reftest::defines("sys/netinet/if_ether.h");
    let ether = assert_defines!(defs;
        ETHER_ADDR_LEN, ETHER_TYPE_LEN, ETHER_CRC_LEN, ETHER_HDR_LEN, ETHER_MIN_LEN,
        ETHER_MAX_LEN, ETHER_MAX_DIX_LEN, ETHER_VLAN_ENCAP_LEN, ETHER_ALIGN,
        ETHER_MAX_HARDMTU_LEN, ETHER_CRC_POLY_LE, ETHER_CRC_POLY_BE);
    assert_complete(&defs, "ETHER_", &ether);
    let evl = assert_defines!(defs;
        EVL_VLID_MASK, EVL_VLID_NULL, EVL_VLID_MIN, EVL_VLID_MAX, EVL_PRIO_MAX, EVL_PRIO_BITS,
        EVL_ENCAPLEN);
    assert_complete(&defs, "EVL_", &evl);
    let eth64 = assert_defines!(defs; ETH64_8021_RSVD_PREFIX, ETH64_8021_RSVD_MASK);
    assert_complete(&defs, "ETH64_", &eth64);
    assert_defines!(defs; ETHERMTU, ETHERMIN, SIN_PROXY);
    assert_eq!(defs["RTF_USETRAILERS"], "RTF_PROTO1");
    assert_eq!(defs["RTF_PERMANENT_ARP"], "RTF_PROTO3");
}
