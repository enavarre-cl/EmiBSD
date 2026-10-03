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

/// A `sockaddr_in` for `a` with length `len`.
fn sin(a: [u8; 4], len: u8) -> crate::netinet::in_::SockaddrIn {
    crate::netinet::in_::SockaddrIn {
        sin_len: len,
        sin_family: crate::sys::socket::AF_INET,
        sin_addr: crate::netinet::in_::InAddr {
            s_addr: u32::from_ne_bytes(a),
        },
        ..Default::default()
    }
}

#[test]
fn masked_copies_and_prefix_masks() {
    use crate::netinet::in_::sintosa;
    let mut src = sin([10, 0, 2, 77], 16);
    let mut mask = sin([255, 255, 255, 0], 7);
    let mut dst = sin([0xaa; 4], 0);
    dst.sin_zero = [0x55; 8];
    // SAFETY: local `sockaddr_in`s of 16 bytes.
    unsafe { rt_maskedcopy(sintosa(&mut src), sintosa(&mut dst), sintosa(&mut mask)) };
    assert_eq!(dst.sin_len, 16);
    assert_eq!(dst.sin_addr.s_addr.to_ne_bytes(), [10, 0, 2, 0]);
    assert_eq!(
        dst.sin_zero, [0; 8],
        "past the mask's length the copy is zero"
    );

    let mut buf = SockaddrStorage::zeroed();
    let m = rt_plentosa(crate::sys::socket::AF_INET, 20, &mut buf);
    // SAFETY: rt_plentosa wrote a `sockaddr_in` into the buffer it returned.
    let m = unsafe { *m.cast::<crate::netinet::in_::SockaddrIn>() };
    assert_eq!(m.sin_addr.s_addr.to_ne_bytes(), [255, 255, 240, 0]);
    assert!(rt_plentosa(crate::sys::socket::AF_INET, -1, &mut buf).is_null());
    assert!(rt_plentosa(crate::sys::socket::AF_UNIX, 8, &mut buf).is_null());
}

#[test]
fn labels_are_named_counted_and_reused() {
    let _g = crate::netinet::ip_input::tests::setup();
    let a = rtlabel_name2id(b"uplink\0");
    let b = rtlabel_name2id(b"backup\0");
    assert_ne!(a, 0);
    assert_ne!(b, 0);
    assert_ne!(a, b);
    assert_eq!(
        rtlabel_name2id(b"uplink\0"),
        a,
        "the same name, another reference"
    );
    assert_eq!(rtlabel_name2id(b"\0"), 0);

    let mut buf = [0u8; RTLABEL_LEN];
    assert_eq!(rtlabel_id2name(a, &mut buf), Some(&b"uplink"[..]));
    let mut sa = SockaddrRtlabel::default();
    let p = rtlabel_id2sa(b, &mut sa);
    assert!(!p.is_null());
    assert_eq!(&sa.sr_label[..7], b"backup\0");

    rtlabel_unref(a);
    assert!(rtlabel_id2name(a, &mut buf).is_some(), "one reference left");
    rtlabel_unref(a);
    assert!(rtlabel_id2name(a, &mut buf).is_none());
    rtlabel_unref(b);
    // The freed ids are free slots again.
    let c = rtlabel_name2id(b"again\0");
    assert_eq!(c, a.min(b));
    rtlabel_unref(c);
}
