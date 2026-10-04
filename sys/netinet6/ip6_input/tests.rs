//! Host tests for IPv6 input on synthetic packets: the header checks of `ipv6_check`, the
//! payload length checks and trimming of `ip6_hbhchcheck`, the hop-by-hop option walk of
//! `ip6_process_hopopts` and the actions of `ip6_unknown_opt`, the extension header chain
//! walkers (`ip6_nexthdr`, `ip6_lasthdr`, `ip6_get_prevhdr`) and the type 0 routing header
//! scan.

use std::vec::Vec;

use super::*;
use crate::net::if_::tests::{setup_net, test_ifnet, test_packet};
use crate::netinet::in_::IPPROTO_UDP;

/// An IPv6 header (`nxt`, payload length `plen`, `src` and `dst`) followed by `rest`.
fn packet(nxt: i32, plen: u16, src: [u8; 16], dst: [u8; 16], rest: &[u8]) -> &'static Mbuf {
    let mut ip6 = Ip6Hdr::zeroed();
    ip6.set_ip6_vfc(IPV6_VERSION);
    ip6.ip6_plen = htons(plen);
    ip6.ip6_nxt = nxt as u8;
    ip6.ip6_hlim = 64;
    ip6.ip6_src = crate::netinet6::in6::In6Addr::new(src);
    ip6.ip6_dst = crate::netinet6::in6::In6Addr::new(dst);
    let mut b: Vec<u8> = pod_bytes(&ip6).to_vec();
    b.extend_from_slice(rest);
    test_packet(&b)
}

/// fd00::`last`.
fn ula(last: u8) -> [u8; 16] {
    let mut a = [0u8; 16];
    a[0] = 0xfd;
    a[15] = last;
    a
}

fn stat(c: Ip6statCounters) -> u64 {
    IP6COUNTERS[c as usize].load(Ordering::Relaxed)
}

#[test]
fn ipv6_check_rejects_bad_headers() {
    let _g = setup_net();
    let ifp = test_ifnet(b"tv6c0");

    let good = packet(IPPROTO_UDP, 0, ula(1), ula(2), &[]);
    let m = ipv6_check(ifp, good).expect("a good header passes");
    m_freem(m);

    // Version 4 in the version field.
    let badvers = stat(Ip6statCounters::Ip6sBadvers);
    let m = packet(IPPROTO_UDP, 0, ula(1), ula(2), &[]);
    let mut ip6 = mtod_ip6(m);
    ip6.set_ip6_vfc(0x40);
    mtod_ip6_store(m, &ip6);
    assert!(ipv6_check(ifp, m).is_none());
    assert_eq!(stat(Ip6statCounters::Ip6sBadvers), badvers + 1);

    let mut mcast = [0u8; 16];
    mcast[0] = 0xff;
    mcast[1] = 0x02;
    mcast[15] = 1;
    let mut loopback = [0u8; 16];
    loopback[15] = 1;
    let mut v4mapped = [0u8; 16];
    v4mapped[10] = 0xff;
    v4mapped[11] = 0xff;
    v4mapped[12..].copy_from_slice(&[10, 0, 0, 1]);
    let mut v4compat = [0u8; 16];
    v4compat[12..].copy_from_slice(&[10, 0, 0, 1]);
    let mut embedded = [0u8; 16];
    embedded[0] = 0xfe;
    embedded[1] = 0x80;
    embedded[3] = 7;
    embedded[15] = 1;
    for (src, dst) in [
        (mcast, ula(2)),    // a multicast source
        (ula(1), [0; 16]),  // an unspecified destination
        (loopback, ula(2)), // ::1 on a non-loopback interface
        (ula(1), v4mapped), // an IPv4 mapped destination
        (v4compat, ula(2)), // an IPv4 compatible source
        (embedded, ula(2)), // a scope already embedded
    ] {
        let badscope = stat(Ip6statCounters::Ip6sBadscope);
        assert!(ipv6_check(ifp, packet(IPPROTO_UDP, 0, src, dst, &[])).is_none());
        assert_eq!(stat(Ip6statCounters::Ip6sBadscope), badscope + 1);
    }

    // A packet shorter than the header.
    let toosmall = stat(Ip6statCounters::Ip6sToosmall);
    assert!(ipv6_check(ifp, test_packet(&[0x60; 20])).is_none());
    assert_eq!(stat(Ip6statCounters::Ip6sToosmall), toosmall + 1);
}

#[test]
fn the_payload_length_is_checked_and_the_packet_trimmed() {
    let _g = setup_net();

    // The header announces 16 bytes, only 8 follow.
    let tooshort = stat(Ip6statCounters::Ip6sTooshort);
    let mut mp = Some(packet(IPPROTO_UDP, 16, ula(1), ula(2), &[0; 8]));
    let mut off = 0;
    assert_eq!(ip6_hbhchcheck(&mut mp, &mut off, None, 0), IPPROTO_DONE);
    assert!(mp.is_none());
    assert_eq!(stat(Ip6statCounters::Ip6sTooshort), tooshort + 1);

    // The header announces 8 bytes, 24 follow (link-layer padding): trimmed to 48.
    let mut mp = Some(packet(IPPROTO_UDP, 8, ula(1), ula(2), &[0; 24]));
    let mut off = 0;
    assert_eq!(ip6_hbhchcheck(&mut mp, &mut off, None, 0), IPPROTO_UDP);
    assert_eq!(off, 40);
    let m = mp.expect("kept");
    assert_eq!(m.m_len().get(), 48);
    assert_eq!(m.m_pkthdr().len.get(), 48);
    m_freem(m);
}

#[test]
fn hop_by_hop_options_are_walked() {
    let _g = setup_net();

    // Hop-by-hop header (next UDP, 8 bytes): Pad1, router alert (MLD 0x0001), PadN of 0.
    #[rustfmt::skip]
    let hbh = [
        IPPROTO_UDP as u8, 0,
        IP6OPT_PAD1,
        IP6OPT_ROUTER_ALERT, 2, 0x00, 0x01,
        IP6OPT_PAD1,
    ];
    let mut mp = Some(packet(IPPROTO_HOPOPTS, 8, ula(1), ula(2), &hbh));
    let mut off = 0;
    let mut ours = false;
    let nxt = ip6_hbhchcheck(&mut mp, &mut off, Some(&mut ours), IPV6_FORWARDING);
    assert_eq!(nxt, IPPROTO_UDP);
    assert_eq!(off, 48);
    assert!(ours, "a router alert makes a router accept the packet");
    m_freem(mp.take());

    // An unknown option whose action is "skip" (type 0x1e) between PadN options.
    let opts = [0x1e, 2, 0xaa, 0xbb, IP6OPT_PADN, 0];
    let m = test_packet(&opts);
    let mut mp = Some(m);
    let mut rtalert = !0;
    let mut plen = 0;
    // SAFETY: the six option bytes are in the mbuf.
    let ok = unsafe { ip6_process_hopopts(&mut mp, mtod::<u8>(m), 6, &mut rtalert, &mut plen) };
    assert!(ok);
    assert_eq!(rtalert, !0);
    assert_eq!(plen, 0);
    m_freem(mp.take());

    // The same option with the "discard" action (0x5e): the packet is dropped silently.
    let m = test_packet(&[0x5e, 2, 0xaa, 0xbb]);
    let mut mp = Some(m);
    // SAFETY: the four option bytes are in the mbuf.
    let ok = unsafe { ip6_process_hopopts(&mut mp, mtod::<u8>(m), 4, &mut rtalert, &mut plen) };
    assert!(!ok);
    assert!(mp.is_none());

    // A PadN with a single byte left is too small.
    let toosmall = stat(Ip6statCounters::Ip6sToosmall);
    let m = test_packet(&[IP6OPT_PAD1, IP6OPT_PADN]);
    let mut mp = Some(m);
    // SAFETY: the two option bytes are in the mbuf.
    let ok = unsafe { ip6_process_hopopts(&mut mp, mtod::<u8>(m), 2, &mut rtalert, &mut plen) };
    assert!(!ok);
    assert!(mp.is_none());
    assert_eq!(stat(Ip6statCounters::Ip6sToosmall), toosmall + 1);
}

#[test]
fn unknown_option_actions() {
    let _g = setup_net();
    let m = test_packet(&[0x1e, 0]);
    let mut mp = Some(m);
    // SAFETY: the option's two bytes are in the mbuf.
    assert!(unsafe { ip6_unknown_opt(&mut mp, mtod::<u8>(m), 42) });
    assert!(mp.is_some());
    m_freem(mp.take());

    let m = test_packet(&[0x7f, 0]);
    let mut mp = Some(m);
    // SAFETY: as above.
    assert!(!unsafe { ip6_unknown_opt(&mut mp, mtod::<u8>(m), 42) });
    assert!(mp.is_none());
}

/// An IPv6 header, a hop-by-hop header (8), destination options (16), a fragment header
/// (`offlg`) and 8 bytes of UDP.
fn chain(offlg: u16) -> &'static Mbuf {
    let mut rest = Vec::new();
    rest.extend_from_slice(&[IPPROTO_DSTOPTS as u8, 0, 1, 4, 0, 0, 0, 0]);
    rest.extend_from_slice(&[IPPROTO_FRAGMENT as u8, 1, 1, 12]);
    rest.extend_from_slice(&[0; 12]);
    rest.extend_from_slice(&[IPPROTO_UDP as u8, 0]);
    rest.extend_from_slice(&offlg.to_be_bytes());
    rest.extend_from_slice(&[0, 0, 0, 7]);
    rest.extend_from_slice(&[0; 8]);
    packet(IPPROTO_HOPOPTS, rest.len() as u16, ula(1), ula(2), &rest)
}

#[test]
fn the_extension_header_chain_is_walked() {
    let _g = setup_net();

    let m = chain(0x0001); // first fragment, more fragments
    let mut nxt = -1;
    assert_eq!(ip6_nexthdr(m, 0, IPPROTO_IPV6, &mut nxt), Some(40));
    assert_eq!(nxt, IPPROTO_HOPOPTS);
    assert_eq!(ip6_nexthdr(m, 40, IPPROTO_HOPOPTS, &mut nxt), Some(48));
    assert_eq!(nxt, IPPROTO_DSTOPTS);
    assert_eq!(ip6_lasthdr(m, 0, IPPROTO_IPV6, &mut nxt), Some(72));
    assert_eq!(nxt, IPPROTO_UDP);
    assert_eq!(ip6_get_prevhdr(m, 40), offset_of!(Ip6Hdr, ip6_nxt) as i32);
    assert_eq!(ip6_get_prevhdr(m, 64), 48);
    assert_eq!(ip6_get_prevhdr(m, 72), 64);
    m_freem(m);

    // A later fragment: the walk stops at the fragment header.
    let m = chain(0x0040); // byte offset 64
    let mut nxt = -1;
    assert_eq!(ip6_lasthdr(m, 0, IPPROTO_IPV6, &mut nxt), Some(64));
    assert_eq!(nxt, IPPROTO_FRAGMENT);
    // An ESP header is never walked into.
    assert_eq!(ip6_nexthdr(m, 64, IPPROTO_ESP, &mut nxt), None);
    // A header past the end of the packet.
    assert_eq!(ip6_nexthdr(m, 1000, IPPROTO_HOPOPTS, &mut nxt), None);
    m_freem(m);

    // AH counts its length in 4-byte words, plus 2.
    let ah = [IPPROTO_UDP as u8, 1, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0];
    let m = packet(IPPROTO_AH, ah.len() as u16, ula(1), ula(2), &ah);
    let mut nxt = -1;
    assert_eq!(ip6_lasthdr(m, 0, IPPROTO_IPV6, &mut nxt), Some(52));
    assert_eq!(nxt, IPPROTO_UDP);
    m_freem(m);
}

#[test]
fn type_0_routing_headers_are_rejected() {
    let _g = setup_net();
    let rthdr = |ty: u8, nxt: i32| [nxt as u8, 0, ty, 0, 0, 0, 0, 0];

    let mut off = 0;
    let m = packet(IPPROTO_ROUTING, 8, ula(1), ula(2), &rthdr(0, IPPROTO_UDP));
    assert!(ip6_check_rh0hdr(m, &mut off));
    assert_eq!(off, 40 + offset_of!(Ip6Rthdr, ip6r_type) as i32);
    m_freem(m);

    let mut off = 0;
    let m = packet(IPPROTO_ROUTING, 8, ula(1), ula(2), &rthdr(2, IPPROTO_UDP));
    assert!(!ip6_check_rh0hdr(m, &mut off));
    m_freem(m);

    // Two routing headers, behind a destination options header.
    let mut rest = vec![IPPROTO_ROUTING as u8, 0, 1, 4, 0, 0, 0, 0];
    rest.extend_from_slice(&rthdr(2, IPPROTO_ROUTING));
    rest.extend_from_slice(&rthdr(2, IPPROTO_UDP));
    let mut off = 0;
    let m = packet(IPPROTO_DSTOPTS, rest.len() as u16, ula(1), ula(2), &rest);
    assert!(ip6_check_rh0hdr(m, &mut off));
    assert_eq!(off, 56);
    m_freem(m);
}

#[test]
fn deep_names_are_not_directories() {
    let mut len = 0;
    assert_eq!(
        ip6_sysctl(&[IPV6CTL_DEFHLIM, 1], 0, &mut len, 0, 0),
        Err(Errno::ENOTDIR)
    );
    assert_eq!(ip6_sysctl(&[], 0, &mut len, 0, 0), Err(Errno::ENOTDIR));
}
