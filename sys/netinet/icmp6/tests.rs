use core::mem::offset_of;

use super::*;
use crate::reftest::{assert_complete, assert_defines};

#[test]
fn filter_macros() {
    let mut f = Icmp6Filter::default();
    icmp6_filter_setblockall(&mut f);
    assert!(icmp6_filter_willblock(ICMP6_ECHO_REQUEST, &f));
    icmp6_filter_setpass(ICMP6_ECHO_REQUEST, &mut f);
    assert!(icmp6_filter_willpass(ICMP6_ECHO_REQUEST, &f));
    assert!(icmp6_filter_willblock(ICMP6_ECHO_REPLY, &f));
    // 128 is bit 0 of word 4.
    assert_eq!(f.icmp6_filt[4], 1);
    icmp6_filter_setpassall(&mut f);
    assert!(icmp6_filter_willpass(ICMP6_MAXTYPE, &f) && icmp6_filter_willpass(255, &f));
    icmp6_filter_setblock(255, &mut f);
    assert!(icmp6_filter_willblock(255, &f) && icmp6_filter_willpass(254, &f));
    assert_eq!(f.icmp6_filt[7], 0x7fff_ffff);
}

#[test]
fn header_views() {
    let mut h = Icmp6Hdr::zeroed();
    h.set_icmp6_id(htons(0x1234));
    h.set_icmp6_seq(htons(7));
    assert_eq!(h.icmp6_data8(0), 0x12);
    assert_eq!(h.icmp6_data8(3), 7);
    assert_eq!(h.icmp6_maxdelay(), h.icmp6_id());
    h.set_icmp6_mtu(htonl(1280));
    assert_eq!(h.icmp6_pptr(), htonl(1280));
    let mut ra = NdRouterAdvert {
        nd_ra_hdr: Icmp6Hdr::zeroed(),
        nd_ra_reachable: 0,
        nd_ra_retransmit: 0,
    };
    ra.set_nd_ra_curhoplimit(64);
    ra.set_nd_ra_flags_reserved(ND_RA_FLAG_MANAGED);
    ra.set_nd_ra_router_lifetime(htons(1800));
    assert_eq!(ra.nd_ra_hdr.icmp6_data8(0), 64);
    assert_eq!(ra.nd_ra_hdr.icmp6_data16(1), htons(1800));
    assert_eq!(offset_of!(NdNeighborSolicit, nd_ns_target), 8);
    assert_eq!(offset_of!(NdRedirect, nd_rd_dst), 24);
    assert_eq!(offset_of!(NdOptPrefixInfo, nd_opt_pi_prefix), 16);
}

#[test]
fn counters_match_the_structure() {
    let w = |off: usize| off / size_of::<u64>();
    assert_eq!(
        w(offset_of!(Icmp6stat, icp6s_outhist)),
        Icmp6statCounters::Icp6sOuthist as usize
    );
    assert_eq!(
        w(offset_of!(Icmp6stat, icp6s_badcode)),
        Icmp6statCounters::Icp6sBadcode as usize
    );
    assert_eq!(
        w(offset_of!(Icmp6stat, icp6s_inhist)),
        Icmp6statCounters::Icp6sInhist as usize
    );
    assert_eq!(
        w(offset_of!(Icmp6stat, icp6s_nd_toomanyopt)),
        Icmp6statCounters::Icp6sNdToomanyopt as usize
    );
    assert_eq!(
        w(offset_of!(Icmp6stat, icp6s_badredirect)),
        Icmp6statCounters::Icp6sBadredirect as usize
    );
}

#[test]
#[ignore = "needs OPENBSD_SRC (just test-ref)"]
fn values_match_the_c_header() {
    let defs = crate::reftest::defines("sys/netinet/icmp6.h");
    assert_defines!(defs; ICMPV6_PLD_MAXLEN);
    let icmp6 = assert_defines!(defs;
        ICMP6_DST_UNREACH, ICMP6_PACKET_TOO_BIG, ICMP6_TIME_EXCEEDED, ICMP6_PARAM_PROB,
        ICMP6_ECHO_REQUEST, ICMP6_ECHO_REPLY, ICMP6_MEMBERSHIP_QUERY, ICMP6_MEMBERSHIP_REPORT,
        ICMP6_MEMBERSHIP_REDUCTION, ICMP6_ROUTER_RENUMBERING, ICMP6_WRUREQUEST, ICMP6_WRUREPLY,
        ICMP6_FQDN_QUERY, ICMP6_FQDN_REPLY, ICMP6_NI_QUERY, ICMP6_NI_REPLY, ICMP6_MAXTYPE,
        ICMP6_DST_UNREACH_NOROUTE, ICMP6_DST_UNREACH_ADMIN, ICMP6_DST_UNREACH_BEYONDSCOPE,
        ICMP6_DST_UNREACH_ADDR, ICMP6_DST_UNREACH_NOPORT, ICMP6_TIME_EXCEED_TRANSIT,
        ICMP6_TIME_EXCEED_REASSEMBLY, ICMP6_PARAMPROB_HEADER, ICMP6_PARAMPROB_NEXTHEADER,
        ICMP6_PARAMPROB_OPTION, ICMP6_INFOMSG_MASK, ICMP6_NI_SUBJ_IPV6, ICMP6_NI_SUBJ_FQDN,
        ICMP6_NI_SUBJ_IPV4, ICMP6_NI_SUCCESS, ICMP6_NI_REFUSED, ICMP6_NI_UNKNOWN,
        ICMP6_ROUTER_RENUMBERING_COMMAND, ICMP6_ROUTER_RENUMBERING_RESULT,
        ICMP6_ROUTER_RENUMBERING_SEQNUM_RESET, ICMP6_RR_FLAGS_TEST, ICMP6_RR_FLAGS_REQRESULT,
        ICMP6_RR_FLAGS_FORCEAPPLY, ICMP6_RR_FLAGS_SPECSITE, ICMP6_RR_FLAGS_PREVDONE,
        ICMP6_RR_PCOUSE_RAFLAGS_ONLINK, ICMP6_RR_PCOUSE_RAFLAGS_AUTO);
    // The htonl()/htons() values are checked by value below.
    let swapped = [
        "ICMP6_RR_PCOUSE_FLAGS_DECRVLTIME",
        "ICMP6_RR_PCOUSE_FLAGS_DECRPLTIME",
        "ICMP6_RR_RESULT_FLAGS_OOB",
        "ICMP6_RR_RESULT_FLAGS_FORBIDDEN",
    ];
    assert_complete(&defs, "ICMP6_", &[&icmp6[..], &swapped[..]].concat());
    let mld = assert_defines!(defs;
        MLD_LISTENER_QUERY, MLD_LISTENER_REPORT, MLD_LISTENER_DONE, MLD_MTRACE_RESP, MLD_MTRACE,
        MLDV2_LISTENER_REPORT);
    assert_complete(&defs, "MLD", &mld);
    let nd = assert_defines!(defs;
        ND_ROUTER_SOLICIT, ND_ROUTER_ADVERT, ND_NEIGHBOR_SOLICIT, ND_NEIGHBOR_ADVERT, ND_REDIRECT,
        ND_REDIRECT_ONLINK, ND_REDIRECT_ROUTER, ND_RA_FLAG_MANAGED, ND_RA_FLAG_OTHER,
        ND_RA_FLAG_RTPREF_MASK, ND_RA_FLAG_RTPREF_HIGH, ND_RA_FLAG_RTPREF_MEDIUM,
        ND_RA_FLAG_RTPREF_LOW, ND_RA_FLAG_RTPREF_RSV, ND_OPT_SOURCE_LINKADDR,
        ND_OPT_TARGET_LINKADDR, ND_OPT_PREFIX_INFORMATION, ND_OPT_REDIRECTED_HEADER, ND_OPT_MTU,
        ND_OPT_ROUTE_INFO, ND_OPT_RDNSS, ND_OPT_DNSSL, ND_OPT_PI_FLAG_ONLINK,
        ND_OPT_PI_FLAG_AUTO);
    let na = [
        "ND_NA_FLAG_ROUTER",
        "ND_NA_FLAG_SOLICITED",
        "ND_NA_FLAG_OVERRIDE",
    ];
    assert_complete(&defs, "ND_", &[&nd[..], &na[..]].concat());
    let ni = assert_defines!(defs;
        NI_QTYPE_NOOP, NI_QTYPE_SUPTYPES, NI_QTYPE_FQDN, NI_QTYPE_DNSNAME, NI_QTYPE_NODEADDR,
        NI_QTYPE_IPV4ADDR);
    assert_complete(&defs, "NI_QTYPE_", &ni);
    let rpm = assert_defines!(defs; RPM_PCO_ADD, RPM_PCO_CHANGE, RPM_PCO_SETGLOBAL, RPM_PCO_MAX);
    assert_complete(&defs, "RPM_", &rpm);
    let ctl = assert_defines!(defs;
        ICMPV6CTL_STATS, ICMPV6CTL_REDIRACCEPT, ICMPV6CTL_REDIRTIMEOUT, ICMPV6CTL_ND6_DELAY,
        ICMPV6CTL_ND6_UMAXTRIES, ICMPV6CTL_ND6_MMAXTRIES, ICMPV6CTL_ND6_QUEUED,
        ICMPV6CTL_NODEINFO, ICMPV6CTL_ERRPPSLIMIT, ICMPV6CTL_MTUDISC_HIWAT,
        ICMPV6CTL_MTUDISC_LOWAT, ICMPV6CTL_MAXID);
    assert_complete(
        &defs,
        "ICMPV6CTL_",
        &[&ctl[..], &["ICMPV6CTL_NAMES"]].concat(),
    );

    // The byte-swapped flags, by their host-order values.
    let swapped32: &[(&str, u32, u32)] = &[
        ("ND_NA_FLAG_ROUTER", ND_NA_FLAG_ROUTER, 0x8000_0000),
        ("ND_NA_FLAG_SOLICITED", ND_NA_FLAG_SOLICITED, 0x4000_0000),
        ("ND_NA_FLAG_OVERRIDE", ND_NA_FLAG_OVERRIDE, 0x2000_0000),
        (
            "ICMP6_RR_PCOUSE_FLAGS_DECRVLTIME",
            ICMP6_RR_PCOUSE_FLAGS_DECRVLTIME,
            0x8000_0000,
        ),
        (
            "ICMP6_RR_PCOUSE_FLAGS_DECRPLTIME",
            ICMP6_RR_PCOUSE_FLAGS_DECRPLTIME,
            0x4000_0000,
        ),
    ];
    for (name, ours, host) in swapped32 {
        assert_eq!(defs[*name], std::format!("htonl(0x{host:08x})"), "{name}");
        assert_eq!(*ours, htonl(*host), "{name}");
    }
}
