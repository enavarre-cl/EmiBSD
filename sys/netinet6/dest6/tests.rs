//! Host tests for `dest6_input` on synthetic packets: the length validation and the
//! Pad1/PadN walk (options that need `ip6_unknown_opt` are that function's tests).

use super::*;
use crate::kern::uipc_mbuf::m_freem;
use crate::net::if_::tests::test_packet;
use crate::netinet::icmp6::Icmp6statCounters;
use crate::netinet::ip_input::tests::setup;
use crate::netinet::ip6::{IPV6_VERSION, Ip6Hdr};
use crate::netinet6::icmp6::ICMP6COUNTERS;
use crate::netinet6::in6::In6Addr;
use crate::netinet6::ip6_input::IP6COUNTERS;
use crate::netinet6::nd6::tests::{OURS6, PEER6};
use core::sync::atomic::Ordering;
use std::vec::Vec;

const IPPROTO_UDP: u8 = 17;

/// An IPv6 header followed by `ext`.
fn packet(ext: &[u8]) -> &'static Mbuf {
    packet_to(OURS6, ext)
}

/// An IPv6 header from `PEER6` to `dst` followed by `ext`.
fn packet_to(dst: In6Addr, ext: &[u8]) -> &'static Mbuf {
    let mut ip6 = Ip6Hdr::zeroed();
    ip6.set_ip6_vfc(IPV6_VERSION);
    ip6.ip6_nxt = 60;
    ip6.ip6_hlim = 64;
    ip6.ip6_src = PEER6;
    ip6.ip6_dst = dst;
    // SAFETY: `Ip6Hdr` is 40 bytes of integers without padding.
    let hdr: [u8; 40] = unsafe { core::mem::transmute(ip6) };
    let mut b: Vec<u8> = hdr.to_vec();
    b.extend_from_slice(ext);
    test_packet(&b)
}

fn toosmall() -> u64 {
    IP6COUNTERS[Ip6statCounters::Ip6sToosmall as usize].load(Ordering::Relaxed)
}

#[test]
fn pad_options_are_skipped_and_the_next_header_returned() {
    let _g = setup();
    // 16 bytes: the header, two Pad1, a PadN with 4 bytes of data, a PadN with 2.
    #[rustfmt::skip]
    let ext = [
        IPPROTO_UDP, 1,
        0, 0,
        1, 4, 0, 0, 0, 0,
        1, 2, 0, 0,
        0, 0,
    ];
    let mut mp = Some(packet(&ext));
    let mut off = 40;
    assert_eq!(
        dest6_input(&mut mp, &mut off, 60, 10, None),
        i32::from(IPPROTO_UDP)
    );
    assert_eq!(off, 40 + 16);
    m_freem(mp.take().expect("packet kept"));
}

#[test]
fn an_option_running_past_the_header_is_dropped() {
    let _g = setup();
    let before = toosmall();
    // The PadN claims 6 bytes of data, but only 4 are left in the 8-byte header.
    let ext = [IPPROTO_UDP, 0, 1, 6, 0, 0, 0, 0];
    let mut mp = Some(packet(&ext));
    let mut off = 40;
    assert_eq!(dest6_input(&mut mp, &mut off, 60, 10, None), IPPROTO_DONE);
    assert!(mp.is_none());
    assert_eq!(off, 40, "the offset is left alone");
    assert_eq!(toosmall(), before + 1);

    // A lone option type byte: no room for its length.
    let before = toosmall();
    let ext = [IPPROTO_UDP, 0, 0, 0, 0, 0, 0, 1];
    let mut mp = Some(packet(&ext));
    assert_eq!(dest6_input(&mut mp, &mut off, 60, 10, None), IPPROTO_DONE);
    assert!(mp.is_none());
    assert_eq!(toosmall(), before + 1);
}

#[test]
fn a_header_longer_than_the_packet_is_dropped() {
    let _g = setup();
    // The length field claims 24 bytes, the packet has 8.
    let ext = [IPPROTO_UDP, 2, 0, 0, 0, 0, 0, 0];
    let mut mp = Some(packet(&ext));
    let mut off = 40;
    assert_eq!(dest6_input(&mut mp, &mut off, 60, 10, None), IPPROTO_DONE);
    assert!(mp.is_none(), "ip6_exthdr_get freed the chain");
}

fn counter(c: Icmp6statCounters) -> u64 {
    ICMP6COUNTERS[c as usize].load(Ordering::Relaxed)
}

fn badoptions() -> u64 {
    IP6COUNTERS[Ip6statCounters::Ip6sBadoptions as usize].load(Ordering::Relaxed)
}

/// Runs `dest6_input` on a header with the one option `opt` (type, length 4, 4 bytes of data)
/// in a packet to `dst`: what it returns, and whether the packet is still there.
fn with_option(dst: In6Addr, opt: u8) -> (i32, bool) {
    let ext = [IPPROTO_UDP, 0, opt, 4, 0, 0, 0, 0];
    let mut mp = Some(packet_to(dst, &ext));
    let mut off = 40;
    let r = dest6_input(&mut mp, &mut off, 60, 10, None);
    let kept = mp.is_some();
    if let Some(m) = mp {
        m_freem(m);
    }
    (r, kept)
}

#[test]
fn unknown_options_follow_their_action_bits() {
    let _g = setup();
    let mcast = crate::netinet6::in6::IN6ADDR_LINKLOCAL_ALLNODES;
    let errors = || counter(Icmp6statCounters::Icp6sError);
    let option_errors = || counter(Icmp6statCounters::Icp6sOparamprobOption);

    // 00: skip over the option.
    assert_eq!(with_option(OURS6, 0x1e), (i32::from(IPPROTO_UDP), true));

    // 01: discard the packet, quietly.
    let (bad, err) = (badoptions(), errors());
    assert_eq!(with_option(OURS6, 0x5e), (IPPROTO_DONE, false));
    assert_eq!((badoptions(), errors()), (bad, err));

    // 10: discard and send a parameter problem, even for a multicast destination.
    for dst in [OURS6, mcast] {
        let (bad, err, opt) = (badoptions(), errors(), option_errors());
        assert_eq!(with_option(dst, 0x9e), (IPPROTO_DONE, false));
        assert_eq!(badoptions(), bad + 1);
        assert_eq!((errors(), option_errors()), (err + 1, opt + 1));
    }

    // 11: the parameter problem only for a unicast destination.
    let (bad, err) = (badoptions(), errors());
    assert_eq!(with_option(OURS6, 0xde), (IPPROTO_DONE, false));
    assert_eq!((badoptions(), errors()), (bad + 1, err + 1));
    let (bad, err) = (badoptions(), errors());
    assert_eq!(with_option(mcast, 0xde), (IPPROTO_DONE, false));
    assert_eq!((badoptions(), errors()), (bad + 1, err));
}
