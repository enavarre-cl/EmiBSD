use super::*;
use crate::reftest::{assert_complete, assert_defines};

#[test]
fn ifreq_union_accessors() {
    let mut ifr = Ifreq::zeroed();
    ifr.ifr_name[..4].copy_from_slice(b"vio0");
    ifr.set_ifr_flags((IFF_UP | IFF_BROADCAST) as i16);
    assert_eq!(ifr.ifr_flags(), 0x3);
    ifr.set_ifr_mtu(1500);
    assert_eq!(ifr.ifr_metric(), 1500);
    assert_eq!(ifr.ifr_hardmtu(), 1500);
    ifr.ifr_addr_mut().sa_len = 16;
    ifr.ifr_addr_mut().sa_family = 2;
    assert_eq!(ifr.ifr_dstaddr().sa_family, 2);
    assert_eq!(ifr.ifr_broadaddr().sa_len, 16);
    // The union starts right after the name, as in C.
    assert_eq!(core::mem::offset_of!(Ifreq, ifr_ifru), IFNAMSIZ);
    assert_eq!(core::mem::offset_of!(Ifaliasreq, ifra_dstaddr), 32);
    assert_eq!(core::mem::offset_of!(Ifgroupreq, ifgr_ifgru), 24);
}

#[test]
fn link_state_and_speed_helpers() {
    assert!(link_state_is_up(LINK_STATE_UNKNOWN));
    assert!(link_state_is_up(LINK_STATE_FULL_DUPLEX));
    assert!(!link_state_is_up(LINK_STATE_DOWN));
    let carp_backup = LINK_STATE_DESCRIPTIONS
        .iter()
        .find(|d| link_state_desc_match(d, IFT_CARP, LINK_STATE_DOWN));
    assert_eq!(carp_backup.map(|d| d.ifs_string), Some(&b"backup"[..]));
    let ether_up = LINK_STATE_DESCRIPTIONS
        .iter()
        .find(|d| link_state_desc_match(d, IFT_ETHER, LINK_STATE_UP));
    assert_eq!(ether_up.map(|d| d.ifs_string), Some(&b"active"[..]));
    assert_eq!(if_gbps(1), 1_000_000_000);
    assert_eq!(ifq_prio2tos(IFQ_DEFPRIO), 0x60);
    assert_eq!(ifq_tos2prio(0x60), IFQ_DEFPRIO);
}

#[test]
#[ignore = "needs OPENBSD_SRC (just test-ref)"]
fn values_match_the_c_header() {
    let defs = crate::reftest::defines("sys/net/if.h");
    let ifq = assert_defines!(defs; IFQ_NQUEUES, IFQ_MINPRIO, IFQ_MAXPRIO, IFQ_DEFPRIO);
    assert_complete(&defs, "IFQ_", &ifq);
    let link = assert_defines!(defs;
        LINK_STATE_UNKNOWN, LINK_STATE_INVALID, LINK_STATE_DOWN, LINK_STATE_KALIVE_DOWN,
        LINK_STATE_UP, LINK_STATE_HALF_DUPLEX, LINK_STATE_FULL_DUPLEX);
    // LINK_STATE_DESCRIPTIONS spans lines in the C.
    assert_complete(
        &defs,
        "LINK_STATE_",
        &[&link[..], &["LINK_STATE_DESCRIPTIONS"]].concat(),
    );
    let iff = assert_defines!(defs;
        IFF_UP, IFF_BROADCAST, IFF_DEBUG, IFF_LOOPBACK, IFF_POINTOPOINT, IFF_STATICARP,
        IFF_RUNNING, IFF_NOARP, IFF_PROMISC, IFF_ALLMULTI, IFF_OACTIVE, IFF_SIMPLEX, IFF_LINK0,
        IFF_LINK1, IFF_LINK2, IFF_MULTICAST);
    // IFF_CANTCHANGE spans lines in the C.
    assert_eq!(IFF_CANTCHANGE, 0x8e5a);
    assert_complete(&defs, "IFF_", &[&iff[..], &["IFF_CANTCHANGE"]].concat());
    let ifxf = assert_defines!(defs;
        IFXF_MPSAFE, IFXF_CLONED, IFXF_AUTOCONF6TEMP, IFXF_MPLS, IFXF_WOL, IFXF_AUTOCONF6,
        IFXF_INET6_NOSOII, IFXF_AUTOCONF4, IFXF_MONITOR, IFXF_LRO, IFXF_MBUF_64BIT);
    assert_eq!(IFXF_CANTCHANGE, 0x3);
    assert_complete(&defs, "IFXF_", &[&ifxf[..], &["IFXF_CANTCHANGE"]].concat());
    let ifcap = assert_defines!(defs;
        IFCAP_CSUM_IPv4, IFCAP_CSUM_TCPv4, IFCAP_CSUM_UDPv4, IFCAP_VLAN_MTU,
        IFCAP_VLAN_HWTAGGING, IFCAP_VLAN_HWOFFLOAD, IFCAP_CSUM_TCPv6, IFCAP_CSUM_UDPv6,
        IFCAP_TSOv4, IFCAP_TSOv6, IFCAP_LRO, IFCAP_WOL);
    assert_eq!(IFCAP_CSUM_MASK, 0x187);
    assert_complete(
        &defs,
        "IFCAP_",
        &[&ifcap[..], &["IFCAP_CSUM_MASK"]].concat(),
    );
    let ifqctl = assert_defines!(defs;
        IFQCTL_LEN, IFQCTL_MAXLEN, IFQCTL_DROPS, IFQCTL_CONGESTION, IFQCTL_MAXID);
    assert_complete(&defs, "IFQCTL_", &ifqctl);
    let ifan = assert_defines!(defs; IFAN_ARRIVAL, IFAN_DEPARTURE);
    assert_complete(&defs, "IFAN_", &ifan);
    let hdrprio = assert_defines!(defs;
        IF_HDRPRIO_MIN, IF_HDRPRIO_MAX, IF_HDRPRIO_PACKET, IF_HDRPRIO_PAYLOAD,
        IF_HDRPRIO_OUTER, IF_PWE3_ETHERNET, IF_PWE3_IP, IF_NAMESIZE, IF_MAX_VECTORS);
    assert_complete(&defs, "IF_", &hdrprio);
    let sff = assert_defines!(defs; IFSFF_ADDR_EEPROM, IFSFF_ADDR_DDM, IFSFF_DATA_LEN);
    assert_complete(&defs, "IFSFF_", &sff);
    assert_defines!(defs; MCLPOOLS, IFNAMSIZ, IFDESCRSIZE, IFLR_PREFIX);
    assert_eq!(defs["IFG_ALL"], "\"all\"");
    assert_eq!(defs["IFG_EGRESS"], "\"egress\"");
}
