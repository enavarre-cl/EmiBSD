use super::*;
use crate::reftest::{assert_complete, assert_defines};

#[test]
#[ignore = "needs OPENBSD_SRC (just test-ref)"]
fn values_match_the_c_header() {
    let defs = crate::reftest::defines("sys/net/route.h");
    let rtf = assert_defines!(defs;
        RTF_UP, RTF_GATEWAY, RTF_HOST, RTF_REJECT, RTF_DYNAMIC, RTF_MODIFIED, RTF_DONE,
        RTF_CLONING, RTF_MULTICAST, RTF_LLINFO, RTF_STATIC, RTF_BLACKHOLE, RTF_PROTO3,
        RTF_PROTO2, RTF_ANNOUNCE, RTF_PROTO1, RTF_CLONED, RTF_CACHED, RTF_MPATH, RTF_MPLS,
        RTF_LOCAL, RTF_BROADCAST, RTF_CONNECTED, RTF_BFD);
    // RTF_FMASK spans lines in the C.
    assert_eq!(RTF_FMASK, 0x0110_fc08);
    assert_complete(&defs, "RTF_", &[&rtf[..], &["RTF_FMASK"]].concat());
    let rtp = assert_defines!(defs;
        RTP_NONE, RTP_LOCAL, RTP_CONNECTED, RTP_STATIC, RTP_EIGRP, RTP_OSPF, RTP_ISIS,
        RTP_RIP, RTP_BGP, RTP_DEFAULT, RTP_PROPOSAL_STATIC, RTP_PROPOSAL_DHCLIENT,
        RTP_PROPOSAL_SLAAC, RTP_PROPOSAL_UMB, RTP_PROPOSAL_PPP, RTP_PROPOSAL_SOLICIT,
        RTP_MAX, RTP_ANY, RTP_MASK, RTP_DOWN);
    assert_complete(&defs, "RTP_", &rtp);
    let rtm = assert_defines!(defs;
        RTM_RTTUNIT, RTM_VERSION, RTM_MAXSIZE, RTM_ADD, RTM_DELETE, RTM_CHANGE, RTM_GET,
        RTM_LOSING, RTM_REDIRECT, RTM_MISS, RTM_RESOLVE, RTM_NEWADDR, RTM_DELADDR,
        RTM_IFINFO, RTM_IFANNOUNCE, RTM_DESYNC, RTM_INVALIDATE, RTM_BFD, RTM_PROPOSAL,
        RTM_CHGADDRATTR, RTM_80211INFO, RTM_SOURCE);
    assert_complete(&defs, "RTM_", &rtm);
    let rtv = assert_defines!(defs;
        RTV_MTU, RTV_HOPCOUNT, RTV_EXPIRE, RTV_RPIPE, RTV_SPIPE, RTV_SSTHRESH, RTV_RTT,
        RTV_RTTVAR);
    assert_complete(&defs, "RTV_", &rtv);
    let rta = assert_defines!(defs;
        RTA_DST, RTA_GATEWAY, RTA_NETMASK, RTA_GENMASK, RTA_IFP, RTA_IFA, RTA_AUTHOR,
        RTA_BRD, RTA_SRC, RTA_SRCMASK, RTA_LABEL, RTA_BFD, RTA_DNS, RTA_STATIC, RTA_SEARCH);
    assert_complete(&defs, "RTA_", &rta);
    let rtax = assert_defines!(defs;
        RTAX_DST, RTAX_GATEWAY, RTAX_NETMASK, RTAX_GENMASK, RTAX_IFP, RTAX_IFA, RTAX_AUTHOR,
        RTAX_BRD, RTAX_SRC, RTAX_SRCMASK, RTAX_LABEL, RTAX_BFD, RTAX_DNS, RTAX_STATIC,
        RTAX_SEARCH, RTAX_MAX);
    assert_complete(&defs, "RTAX_", &rtax);
    let route = assert_defines!(defs;
        ROUTE_MSGFILTER, ROUTE_TABLEFILTER, ROUTE_PRIOFILTER, ROUTE_FLAGFILTER);
    assert_complete(&defs, "ROUTE_", &route);
    let rest = assert_defines!(defs;
        RTABLE_ANY, RTLABEL_LEN, RTDNS_LEN, RTSTATIC_LEN, RTSEARCH_LEN, RT_RESOLVE);
    assert_complete(&defs, "RT_", &rest);
    assert_eq!(RtstatCounters::RtsNcounters as usize, 5);
}
