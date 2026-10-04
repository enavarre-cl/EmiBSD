use core::mem::offset_of;

use super::*;
use crate::reftest::{assert_complete, assert_defines};

#[test]
fn vfc_is_the_first_byte_of_the_header() {
    let mut ip6 = Ip6Hdr::zeroed();
    ip6.set_ip6_vfc(IPV6_VERSION);
    assert_eq!(ip6.ip6_vfc() & IPV6_VERSION_MASK, IPV6_VERSION);
    assert_eq!(ip6.ip6_flow.to_ne_bytes(), [0x60, 0, 0, 0]);
    // The flow label survives a version update, as with the C's union.
    ip6.ip6_flow = htonl(0x6123_4567);
    ip6.set_ip6_vfc(0x6f);
    assert_eq!(ip6.ip6_flow & IPV6_FLOWLABEL_MASK, htonl(0x0003_4567));
    assert_eq!(ip6.ip6_flow & IPV6_FLOWINFO_MASK, htonl(0x0f23_4567));
    ip6.ip6_hlim = 64;
    assert_eq!(ip6.ip6_hops(), 64);
}

#[test]
fn layout_matches_the_wire() {
    assert_eq!(offset_of!(Ip6Hdr, ip6_plen), 4);
    assert_eq!(offset_of!(Ip6Hdr, ip6_nxt), 6);
    assert_eq!(offset_of!(Ip6Hdr, ip6_hlim), 7);
    assert_eq!(offset_of!(Ip6Hdr, ip6_src), 8);
    assert_eq!(offset_of!(Ip6Hdr, ip6_dst), 24);
    assert_eq!(offset_of!(Ip6Frag, ip6f_offlg), 2);
    assert_eq!(offset_of!(Ip6Frag, ip6f_ident), 4);
    assert_eq!(offset_of!(Ip6HdrPseudo, ip6ph_len), 32);
    assert_eq!(offset_of!(Ip6HdrPseudo, ip6ph_nxt), 39);
}

#[test]
fn fragment_and_option_bits() {
    // offset 0x1238 bytes (in units of 8), more fragments
    let offlg = htons(0x1238 | 0x0001);
    assert_eq!(offlg & IP6F_OFF_MASK, htons(0x1238));
    assert_eq!(offlg & IP6F_MORE_FRAG, htons(1));
    assert_eq!(offlg & IP6F_RESERVED_MASK, 0);
    assert_eq!(ip6opt_type(IP6OPT_JUMBO), IP6OPT_TYPE_ICMP);
    assert_eq!(ip6opt_type(IP6OPT_ROUTER_ALERT), IP6OPT_TYPE_SKIP);
    assert_eq!(IP6_ALERT_MLD, 0);
    assert_eq!(IP6_ALERT_RSVP.to_ne_bytes(), [0, 1]);
}

#[test]
#[ignore = "needs OPENBSD_SRC (just test-ref)"]
fn values_match_the_c_header() {
    let defs = crate::reftest::defines("sys/netinet/ip6.h");
    let opt = assert_defines!(defs;
        IP6OPT_PAD1, IP6OPT_PADN, IP6OPT_JUMBO, IP6OPT_NSAP_ADDR, IP6OPT_TUNNEL_LIMIT,
        IP6OPT_ROUTER_ALERT, IP6OPT_RTALERT_LEN, IP6OPT_RTALERT_MLD, IP6OPT_RTALERT_RSVP,
        IP6OPT_RTALERT_ACTNET, IP6OPT_MINLEN, IP6OPT_TYPE_SKIP, IP6OPT_TYPE_DISCARD,
        IP6OPT_TYPE_FORCEICMP, IP6OPT_TYPE_ICMP, IP6OPT_MUTABLE, IP6OPT_JUMBO_LEN);
    assert_complete(&defs, "IP6OPT_", &opt);
    let ipv6 = assert_defines!(defs;
        IPV6_VERSION, IPV6_VERSION_MASK, IPV6_MAXHLIM, IPV6_DEFHLIM, IPV6_FRAGTTL, IPV6_HLIMDEC,
        IPV6_MMTU, IPV6_MAXPACKET);
    // The byte-order dependent masks: the last (little-endian) branch is what `defines`
    // keeps, the order of the host these tests run on.
    #[cfg(target_endian = "little")]
    let ipv6 = [
        &ipv6[..],
        &assert_defines!(defs; IPV6_FLOWINFO_MASK, IPV6_FLOWLABEL_MASK)[..],
    ]
    .concat();
    #[cfg(target_endian = "little")]
    {
        let frag = assert_defines!(defs; IP6F_OFF_MASK, IP6F_RESERVED_MASK, IP6F_MORE_FRAG);
        assert_complete(&defs, "IP6F_", &frag);
        let alert = assert_defines!(defs; IP6_ALERT_MLD, IP6_ALERT_RSVP, IP6_ALERT_AN);
        assert_complete(&defs, "IP6_ALERT_", &alert);
    }
    assert_complete(&defs, "IPV6_", &ipv6);
    assert_defines!(defs; IP6TOS_CE, IP6TOS_ECT);
}
