use super::*;
use crate::reftest::{assert_complete, assert_defines};
use crate::sys::endian::ntohl;

#[test]
fn addresses_are_network_order() {
    assert_eq!(INADDR_LOOPBACK.to_ne_bytes(), [127, 0, 0, 1]);
    assert_eq!(INADDR_ALLHOSTS_GROUP.to_ne_bytes(), [224, 0, 0, 1]);
    assert_eq!(IN_CLASSC_NET.to_ne_bytes(), [255, 255, 255, 0]);
    let a = |b: [u8; 4]| u32::from_ne_bytes(b);
    assert!(in_classa(a([10, 0, 2, 15])));
    assert!(in_classb(a([172, 16, 0, 1])));
    assert!(in_classc(a([192, 168, 1, 1])));
    assert!(in_multicast(a([224, 0, 0, 251])));
    assert!(!in_multicast(a([10, 0, 2, 2])));
    assert!(in_local_group(a([224, 0, 0, 18])));
    assert!(in_badclass(a([255, 255, 255, 255])));
    assert!(in_classfulbroadcast(
        a([10, 255, 255, 255]),
        a([10, 0, 0, 0])
    ));
    assert!(in_classfulbroadcast(
        a([192, 168, 1, 255]),
        a([192, 168, 1, 0])
    ));
    assert!(!in_classfulbroadcast(a([10, 0, 2, 255]), a([10, 0, 2, 0])));
    assert!(in_rfc3021_subnet(IN_RFC3021_NET));
    assert!(in_nullhost(InAddr::default()));
    let lo = InAddr {
        s_addr: INADDR_LOOPBACK,
    };
    assert!(in_hosteq(lo, lo));
    let mut sin = SockaddrIn::default();
    assert_eq!(satosin(sintosa(&mut sin)), &mut sin as *mut SockaddrIn);
}

#[test]
#[ignore = "needs OPENBSD_SRC (just test-ref)"]
fn values_match_the_c_header() {
    let defs = crate::reftest::defines("sys/netinet/in.h");
    let proto = assert_defines!(defs;
        IPPROTO_IP, IPPROTO_HOPOPTS, IPPROTO_ICMP, IPPROTO_IGMP, IPPROTO_GGP, IPPROTO_IPIP,
        IPPROTO_IPV4, IPPROTO_TCP, IPPROTO_EGP, IPPROTO_PUP, IPPROTO_UDP, IPPROTO_IDP,
        IPPROTO_TP, IPPROTO_IPV6, IPPROTO_ROUTING, IPPROTO_FRAGMENT, IPPROTO_RSVP, IPPROTO_GRE,
        IPPROTO_ESP, IPPROTO_AH, IPPROTO_MOBILE, IPPROTO_ICMPV6, IPPROTO_NONE, IPPROTO_DSTOPTS,
        IPPROTO_EON, IPPROTO_ETHERIP, IPPROTO_ENCAP, IPPROTO_PIM, IPPROTO_IPCOMP, IPPROTO_CARP,
        IPPROTO_SCTP, IPPROTO_UDPLITE, IPPROTO_MPLS, IPPROTO_PFSYNC, IPPROTO_RAW, IPPROTO_MAX,
        IPPROTO_DIVERT, IPPROTO_DONE, IPPROTO_MAXID);
    assert_complete(&defs, "IPPROTO_", &proto);
    let port = assert_defines!(defs;
        IPPORT_RESERVED, IPPORT_USERRESERVED, IPPORT_HIFIRSTAUTO, IPPORT_HILASTAUTO);
    assert_complete(&defs, "IPPORT_", &port);
    let ip = assert_defines!(defs;
        IP_OPTIONS, IP_HDRINCL, IP_TOS, IP_TTL, IP_RECVOPTS, IP_RECVRETOPTS, IP_RECVDSTADDR,
        IP_RETOPTS, IP_MULTICAST_IF, IP_MULTICAST_TTL, IP_MULTICAST_LOOP, IP_ADD_MEMBERSHIP,
        IP_DROP_MEMBERSHIP, IP_PORTRANGE, IP_AUTH_LEVEL, IP_ESP_TRANS_LEVEL,
        IP_ESP_NETWORK_LEVEL, IP_IPSEC_LOCAL_ID, IP_IPSEC_REMOTE_ID, IP_IPSEC_LOCAL_CRED,
        IP_IPSEC_REMOTE_CRED, IP_IPSEC_LOCAL_AUTH, IP_IPSEC_REMOTE_AUTH, IP_IPCOMP_LEVEL,
        IP_RECVIF, IP_RECVTTL, IP_MINTTL, IP_RECVDSTPORT, IP_PIPEX, IP_RECVRTABLE,
        IP_IPSECFLOWINFO, IP_IPDEFTTL, IP_SENDSRCADDR, IP_RTABLE, IP_DEFAULT_MULTICAST_TTL,
        IP_DEFAULT_MULTICAST_LOOP, IP_MIN_MEMBERSHIPS, IP_MAX_MEMBERSHIPS,
        IP_PORTRANGE_DEFAULT, IP_PORTRANGE_HIGH, IP_PORTRANGE_LOW);
    assert_complete(&defs, "IP_", &ip);
    let ipsec = assert_defines!(defs;
        IPSEC_LEVEL_BYPASS, IPSEC_LEVEL_NONE, IPSEC_LEVEL_AVAIL, IPSEC_LEVEL_USE,
        IPSEC_LEVEL_REQUIRE, IPSEC_LEVEL_UNIQUE, IPSEC_LEVEL_DEFAULT, IPSEC_AUTH_LEVEL_DEFAULT,
        IPSEC_ESP_TRANS_LEVEL_DEFAULT, IPSEC_ESP_NETWORK_LEVEL_DEFAULT,
        IPSEC_IPCOMP_LEVEL_DEFAULT);
    assert_complete(&defs, "IPSEC_", &ipsec);
    let ipctl = assert_defines!(defs;
        IPCTL_FORWARDING, IPCTL_SENDREDIRECTS, IPCTL_DEFTTL, IPCTL_SOURCEROUTE,
        IPCTL_DIRECTEDBCAST, IPCTL_IPPORT_FIRSTAUTO, IPCTL_IPPORT_LASTAUTO,
        IPCTL_IPPORT_HIFIRSTAUTO, IPCTL_IPPORT_HILASTAUTO, IPCTL_IPPORT_MAXQUEUE,
        IPCTL_ENCDEBUG, IPCTL_IPSEC_STATS, IPCTL_IPSEC_EXPIRE_ACQUIRE,
        IPCTL_IPSEC_EMBRYONIC_SA_TIMEOUT, IPCTL_IPSEC_REQUIRE_PFS, IPCTL_IPSEC_SOFT_ALLOCATIONS,
        IPCTL_IPSEC_ALLOCATIONS, IPCTL_IPSEC_SOFT_BYTES, IPCTL_IPSEC_BYTES, IPCTL_IPSEC_TIMEOUT,
        IPCTL_IPSEC_SOFT_TIMEOUT, IPCTL_IPSEC_SOFT_FIRSTUSE, IPCTL_IPSEC_FIRSTUSE,
        IPCTL_IPSEC_ENC_ALGORITHM, IPCTL_IPSEC_AUTH_ALGORITHM, IPCTL_MTUDISC,
        IPCTL_MTUDISCTIMEOUT, IPCTL_IPSEC_IPCOMP_ALGORITHM, IPCTL_IFQUEUE, IPCTL_MFORWARDING,
        IPCTL_MULTIPATH, IPCTL_STATS, IPCTL_MRTPROTO, IPCTL_MRTSTATS, IPCTL_ARPQUEUED,
        IPCTL_MRTMFC, IPCTL_MRTVIF, IPCTL_ARPTIMEOUT, IPCTL_ARPDOWN, IPCTL_ARPQUEUE,
        IPCTL_MAXID);
    // IPCTL_NAMES is a sysctl name table (deferred).
    assert_complete(&defs, "IPCTL_", &[&ipctl[..], &["IPCTL_NAMES"]].concat());
    assert_defines!(defs;
        IN_CLASSA_NSHIFT, IN_CLASSA_MAX, IN_CLASSB_NSHIFT, IN_CLASSB_MAX, IN_CLASSC_NSHIFT,
        IN_CLASSD_NSHIFT, IN_RFC3021_NSHIFT, IN_LOOPBACKNET, INET_ADDRSTRLEN);

    // The kernel's __IPADDR(x) is htonl(x): compare the host-order literal.
    let ipaddr: &[(&str, u32)] = &[
        ("IN_CLASSA_NET", IN_CLASSA_NET),
        ("IN_CLASSA_HOST", IN_CLASSA_HOST),
        ("IN_CLASSB_NET", IN_CLASSB_NET),
        ("IN_CLASSB_HOST", IN_CLASSB_HOST),
        ("IN_CLASSC_NET", IN_CLASSC_NET),
        ("IN_CLASSC_HOST", IN_CLASSC_HOST),
        ("IN_CLASSD_NET", IN_CLASSD_NET),
        ("IN_CLASSD_HOST", IN_CLASSD_HOST),
        ("IN_RFC3021_NET", IN_RFC3021_NET),
        ("IN_RFC3021_HOST", IN_RFC3021_HOST),
        ("INADDR_ANY", INADDR_ANY),
        ("INADDR_LOOPBACK", INADDR_LOOPBACK),
        ("INADDR_BROADCAST", INADDR_BROADCAST),
        ("INADDR_UNSPEC_GROUP", INADDR_UNSPEC_GROUP),
        ("INADDR_ALLHOSTS_GROUP", INADDR_ALLHOSTS_GROUP),
        ("INADDR_ALLROUTERS_GROUP", INADDR_ALLROUTERS_GROUP),
        ("INADDR_CARP_GROUP", INADDR_CARP_GROUP),
        ("INADDR_PFSYNC_GROUP", INADDR_PFSYNC_GROUP),
        ("INADDR_MAX_LOCAL_GROUP", INADDR_MAX_LOCAL_GROUP),
    ];
    for (name, ours) in ipaddr {
        let text = &defs[*name];
        let inner = text
            .strip_prefix("__IPADDR(")
            .and_then(|t| t.strip_suffix(')'))
            .unwrap_or_else(|| panic!("{name}: {text}"));
        let host = crate::reftest::parse_int(inner).unwrap_or_else(|| panic!("{name}"));
        assert_eq!(i64::from(ntohl(*ours)), host, "{name}");
    }
    // INADDR_NONE is userland-only.
    let inaddr: std::vec::Vec<&str> = ipaddr.iter().map(|(n, _)| *n).collect();
    assert_complete(&defs, "INADDR_", &[&inaddr[..], &["INADDR_NONE"]].concat());
}
