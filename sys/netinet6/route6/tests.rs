//! Host tests for `route6_input`: a routing header with no segments left is skipped, one
//! with segments left (any type, type 0 included) gets an ICMPv6 parameter problem.

use super::*;
use crate::kern::uipc_mbuf::m_freem;
use crate::net::if_::tests::test_packet;
use crate::net::if_var::Ifnet;
use crate::netinet::icmp6::Icmp6statCounters;
use crate::netinet::ip6::{IPV6_VERSION, Ip6Hdr};
use crate::netinet6::ip6_input::IP6COUNTERS;
use crate::netinet6::nd6::tests::{OURS6, PEER6, icmp6stat, nd6_setup, take_sent};
use core::sync::atomic::Ordering;
use std::vec::Vec;

const IPPROTO_UDP: u8 = 17;
const IPPROTO_ROUTING: u8 = 43;

/// A packet from `PEER6` to `OURS6`, a routing header (`nxt`, `len`, `type`, `segleft`)
/// padded to its length, and 8 bytes of payload.
fn packet(ifp: &Ifnet, rh: [u8; 4]) -> &'static Mbuf {
    let mut ip6 = Ip6Hdr::zeroed();
    ip6.set_ip6_vfc(IPV6_VERSION);
    ip6.ip6_nxt = IPPROTO_ROUTING;
    ip6.ip6_hlim = 64;
    ip6.ip6_src = PEER6;
    ip6.ip6_dst = OURS6;
    // SAFETY: `Ip6Hdr` is 40 bytes of integers without padding.
    let hdr: [u8; 40] = unsafe { core::mem::transmute(ip6) };
    let mut b: Vec<u8> = hdr.to_vec();
    b.extend_from_slice(&rh);
    b.extend_from_slice(&[0; 4]);
    b.extend_from_slice(&[0xee; 8]);
    let m = test_packet(&b);
    m.m_pkthdr().ph_ifidx.set(ifp.if_index.get());
    m
}

fn ip6stat(c: Ip6statCounters) -> u64 {
    IP6COUNTERS[c as usize].load(Ordering::Relaxed)
}

#[test]
fn a_header_without_segments_left_is_skipped() {
    let (_g, ifp) = nd6_setup();
    // type 0 and an unknown type alike: nothing left to route, ignore the header.
    for ty in [0, 2, 77] {
        let mut mp = Some(packet(ifp, [IPPROTO_UDP, 0, ty, 0]));
        let mut off = 40;
        assert_eq!(
            route6_input(&mut mp, &mut off, 43, 10, None),
            i32::from(IPPROTO_UDP)
        );
        assert_eq!(off, 48);
        m_freem(mp.take().expect("packet kept"));
    }
}

#[test]
fn a_header_with_segments_left_is_refused() {
    let (_g, ifp) = nd6_setup();
    let _ = take_sent();
    let bad = ip6stat(Ip6statCounters::Ip6sBadoptions);
    let errors = icmp6stat(Icmp6statCounters::Icp6sError);
    let header = icmp6stat(Icmp6statCounters::Icp6sOparamprobHeader);
    let mut mp = Some(packet(ifp, [IPPROTO_UDP, 0, 0, 2]));
    let mut off = 40;
    assert_eq!(route6_input(&mut mp, &mut off, 43, 10, None), IPPROTO_DONE);
    assert!(mp.is_none(), "icmp6_error consumed the packet");
    assert_eq!(off, 40);
    assert_eq!(ip6stat(Ip6statCounters::Ip6sBadoptions), bad + 1);
    // A parameter problem about the routing header was made.
    assert_eq!(icmp6stat(Icmp6statCounters::Icp6sError), errors + 1);
    assert_eq!(
        icmp6stat(Icmp6statCounters::Icp6sOparamprobHeader),
        header + 1
    );
}
