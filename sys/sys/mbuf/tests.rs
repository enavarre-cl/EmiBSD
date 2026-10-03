//! Host tests for `<sys/mbuf.h>`: the layout and, against the C header, the constants.

use super::*;

#[test]
fn layout_matches_lp64_openbsd() {
    assert_eq!(size_of::<MHdr>(), 32);
    assert_eq!(MLEN, 224);
    assert_eq!(size_of::<Pkthdr>(), 80);
    assert_eq!(MHLEN, 144);
    assert_eq!(MINCLSIZE, 369);
    assert_eq!(size_of::<MTag>(), 16);
}

#[test]
#[ignore = "needs OPENBSD_SRC (just test-ref)"]
fn constants_match_the_c_header() {
    let defs = crate::reftest::defines("sys/sys/mbuf.h");
    let ours: &[(&str, i64)] = &[
        ("MSIZE", MSIZE as i64),
        ("MAXMCLBYTES", MAXMCLBYTES as i64),
        ("MCLSHIFT", MCLSHIFT as i64),
        ("M_EXT", i64::from(M_EXT)),
        ("M_PKTHDR", i64::from(M_PKTHDR)),
        ("M_EOR", i64::from(M_EOR)),
        ("M_EXTWR", i64::from(M_EXTWR)),
        ("M_PROTO1", i64::from(M_PROTO1)),
        ("M_VLANTAG", i64::from(M_VLANTAG)),
        ("M_LOOP", i64::from(M_LOOP)),
        ("M_BCAST", i64::from(M_BCAST)),
        ("M_MCAST", i64::from(M_MCAST)),
        ("M_CONF", i64::from(M_CONF)),
        ("M_AUTH", i64::from(M_AUTH)),
        ("M_TUNNEL", i64::from(M_TUNNEL)),
        ("M_ZEROIZE", i64::from(M_ZEROIZE)),
        ("M_COMP", i64::from(M_COMP)),
        ("M_LINK0", i64::from(M_LINK0)),
        ("M_IPV4_CSUM_OUT", i64::from(M_IPV4_CSUM_OUT)),
        ("M_TCP_CSUM_OUT", i64::from(M_TCP_CSUM_OUT)),
        ("M_UDP_CSUM_OUT", i64::from(M_UDP_CSUM_OUT)),
        ("M_IPV4_CSUM_IN_OK", i64::from(M_IPV4_CSUM_IN_OK)),
        ("M_IPV4_CSUM_IN_BAD", i64::from(M_IPV4_CSUM_IN_BAD)),
        ("M_TCP_CSUM_IN_OK", i64::from(M_TCP_CSUM_IN_OK)),
        ("M_TCP_CSUM_IN_BAD", i64::from(M_TCP_CSUM_IN_BAD)),
        ("M_UDP_CSUM_IN_OK", i64::from(M_UDP_CSUM_IN_OK)),
        ("M_UDP_CSUM_IN_BAD", i64::from(M_UDP_CSUM_IN_BAD)),
        ("M_ICMP_CSUM_OUT", i64::from(M_ICMP_CSUM_OUT)),
        ("M_ICMP_CSUM_IN_OK", i64::from(M_ICMP_CSUM_IN_OK)),
        ("M_ICMP_CSUM_IN_BAD", i64::from(M_ICMP_CSUM_IN_BAD)),
        ("M_IPV6_DF_OUT", i64::from(M_IPV6_DF_OUT)),
        ("M_TIMESTAMP", i64::from(M_TIMESTAMP)),
        ("M_FLOWID", i64::from(M_FLOWID)),
        ("M_TCP_TSO", i64::from(M_TCP_TSO)),
        ("PF_TAG_GENERATED", i64::from(PF_TAG_GENERATED)),
        (
            "PF_TAG_SYNCOOKIE_RECREATED",
            i64::from(PF_TAG_SYNCOOKIE_RECREATED),
        ),
        (
            "PF_TAG_TRANSLATE_LOCALHOST",
            i64::from(PF_TAG_TRANSLATE_LOCALHOST),
        ),
        ("PF_TAG_DIVERTED", i64::from(PF_TAG_DIVERTED)),
        ("PF_TAG_DIVERTED_PACKET", i64::from(PF_TAG_DIVERTED_PACKET)),
        ("PF_TAG_REROUTE", i64::from(PF_TAG_REROUTE)),
        ("PF_TAG_REFRAGMENTED", i64::from(PF_TAG_REFRAGMENTED)),
        ("PF_TAG_PROCESSED", i64::from(PF_TAG_PROCESSED)),
        ("MT_FREE", i64::from(MT_FREE)),
        ("MT_DATA", i64::from(MT_DATA)),
        ("MT_HEADER", i64::from(MT_HEADER)),
        ("MT_SONAME", i64::from(MT_SONAME)),
        ("MT_SOOPTS", i64::from(MT_SOOPTS)),
        ("MT_FTABLE", i64::from(MT_FTABLE)),
        ("MT_CONTROL", i64::from(MT_CONTROL)),
        ("MT_OOBDATA", i64::from(MT_OOBDATA)),
        ("MT_NTYPES", MT_NTYPES as i64),
        ("MEXTFREE_POOL", i64::from(MEXTFREE_POOL)),
        ("M_COPYALL", i64::from(M_COPYALL)),
        (
            "PACKET_TAG_IPSEC_IN_DONE",
            i64::from(PACKET_TAG_IPSEC_IN_DONE),
        ),
        (
            "PACKET_TAG_IPSEC_OUT_DONE",
            i64::from(PACKET_TAG_IPSEC_OUT_DONE),
        ),
        (
            "PACKET_TAG_IPSEC_FLOWINFO",
            i64::from(PACKET_TAG_IPSEC_FLOWINFO),
        ),
        ("PACKET_TAG_IP_OFFNXT", i64::from(PACKET_TAG_IP_OFFNXT)),
        ("PACKET_TAG_IP6_OFFNXT", i64::from(PACKET_TAG_IP6_OFFNXT)),
        ("PACKET_TAG_WIREGUARD", i64::from(PACKET_TAG_WIREGUARD)),
        ("PACKET_TAG_GRE", i64::from(PACKET_TAG_GRE)),
        ("PACKET_TAG_DLT", i64::from(PACKET_TAG_DLT)),
        ("PACKET_TAG_PF_DIVERT", i64::from(PACKET_TAG_PF_DIVERT)),
        (
            "PACKET_TAG_PF_REASSEMBLED",
            i64::from(PACKET_TAG_PF_REASSEMBLED),
        ),
        ("PACKET_TAG_SRCROUTE", i64::from(PACKET_TAG_SRCROUTE)),
        ("PACKET_TAG_TUNNEL", i64::from(PACKET_TAG_TUNNEL)),
        ("PACKET_TAG_CARP_BAL_IP", i64::from(PACKET_TAG_CARP_BAL_IP)),
        ("PACKET_TAG_MAXSIZE", PACKET_TAG_MAXSIZE as i64),
        ("M_MAXLOOP", i64::from(M_MAXLOOP)),
    ];
    for (name, value) in ours {
        assert_eq!(crate::reftest::int(&defs, name), Some(*value), "{name}");
    }
}
