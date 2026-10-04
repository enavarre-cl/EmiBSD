//! Host tests for ICMPv6 on synthetic packets: the echo reply (`icmp6_echo_reply`, with the
//! copy and the fresh-mbuf paths), `icmp6_error`'s rules and the message it builds,
//! `icmp6_reflect`, the checks of `icmp6_input`, the redirect validation, path MTU discovery
//! and the sysctls.

use std::sync::MutexGuard;
use std::vec::Vec;

use super::*;
use crate::kern::uipc_mbuf::{m_copydata, m_freem, m_get};
use crate::net::if_::ifaof_ifpforaddr;
use crate::net::if_::tests::test_packet;
use crate::net::route::{
    RTAX_DST, RTAX_GATEWAY, RTAX_NETMASK, RTF_CLONING, RTF_CONNECTED, RTF_MPATH, RTF_STATIC,
    RtAddrinfo, rt_ifa_add,
};
use crate::netinet::icmp6::{ICMP6_PACKET_TOO_BIG, Icmp6stat};
use crate::netinet::ip_input::tests::{bytes, setup as setup_ip};
use crate::netinet::ip6::IPV6_VERSION;
use crate::netinet6::in6::tests::{a6, test_ia6, test_if};
use crate::netinet6::in6::{IN6ADDR_ANY, IN6ADDR_LINKLOCAL_ALLNODES};
use crate::netinet6::nd6::tests::{OURS6, PEER6, icmp6stat};
use crate::netinet6::nd6::{nd6_ifattach, nd6_init};
use crate::sys::endian::htonl;
use crate::sys::mbuf::MT_DATA;
use crate::sys::systm::{net_lock, net_unlock};
use std::ptr::NonNull;

const IPPROTO_UDP: u8 = 17;

/// The ICMPv6 and ND state, and an interface with `OURS6`/64, its connected route, and a
/// link-local address; no duplicate address detection runs: the addresses are usable.
///
/// The interface's MTU is 0: `ip6_output` drops (EMSGSIZE) what it is given, because
/// `if_output_tso` has no AF_INET6 case yet (phase 2 of the INET6 port) and would panic.
fn net6() -> (
    (MutexGuard<'static, ()>, MutexGuard<'static, ()>),
    &'static Ifnet,
) {
    let g = setup_ip();
    nd6_init();
    icmp6_init();
    reset_ratelimit();
    let ifp = test_if(b"tic0");
    nd6_ifattach(ifp);
    let ia = test_ia6(ifp, OURS6, 0);
    let mut ll = a6("fe80::5054:ff:fe12:3456");
    ll.set_s6_addr16(1, htons(ifp.if_index.get() as u16));
    test_ia6(ifp, ll, 0);
    // `in6_update_ifa` installs the connected route of the prefix.
    net_lock();
    // SAFETY: the address's own socket address; the address is leaked.
    unsafe {
        rt_ifa_add(
            &ia.ia_ifa,
            RTF_CLONING | RTF_CONNECTED | RTF_MPATH,
            ia.ia_ifa.ifa_addr.get(),
            0,
        )
    }
    .expect("connected route");
    net_unlock();
    ifp.if_mtu.set(0);
    (g, ifp)
}

/// Forgets the rate limit's history and restores its default.
fn reset_ratelimit() {
    ICMP6ERRPPSLIM.store(100, Ordering::Relaxed);
    set_ratelimit_window(0);
}

/// Starts the rate limit's window at second `sec`. The host clock of the tests stands at 0,
/// which `ppsratecheck` reads as "never checked" and resets the count on every call; a window
/// that started "in the future" is the way to count.
fn set_ratelimit_window(sec: i64) {
    // SAFETY: the tests run one at a time under the network test lock.
    let pps = unsafe { ICMP6ERRPPS.get_mut() };
    pps.last = Timeval {
        tv_sec: sec,
        tv_usec: 0,
    };
    pps.count = 0;
}

/// The value of the histogram entry `hist + t`.
fn hist(hist: Icmp6statCounters, t: u8) -> u64 {
    ICMP6COUNTERS[hist as usize + usize::from(t)].load(Ordering::Relaxed)
}

/// The bytes of an IPv6 header.
fn ip6_bytes(src: In6Addr, dst: In6Addr, nxt: u8, hlim: u8, plen: usize) -> Vec<u8> {
    let mut ip6 = Ip6Hdr::zeroed();
    ip6.set_ip6_vfc(IPV6_VERSION);
    ip6.ip6_plen = htons(plen as u16);
    ip6.ip6_nxt = nxt;
    ip6.ip6_hlim = hlim;
    ip6.ip6_src = src;
    ip6.ip6_dst = dst;
    let mut b = std::vec![0u8; 40];
    put(&mut b, 0, &ip6);
    b
}

/// A received packet of `b`, from `ifp`.
fn received(ifp: &Ifnet, b: &[u8]) -> &'static Mbuf {
    let m = test_packet(b);
    m.m_pkthdr().ph_ifidx.set(ifp.if_index.get());
    m
}

/// An ICMPv6 message from `src` to `dst`: `fixed` is type, code, checksum and the 4 bytes of
/// the header's last word, `data` what follows. The checksum is filled in unless `bad_sum`.
fn icmp6_packet(
    ifp: &Ifnet,
    src: In6Addr,
    dst: In6Addr,
    hlim: u8,
    fixed: [u8; 8],
    data: &[u8],
    bad_sum: bool,
) -> &'static Mbuf {
    let len = 8 + data.len();
    let mut b = ip6_bytes(src, dst, IPPROTO_ICMPV6 as u8, hlim, len);
    b.extend_from_slice(&fixed);
    b.extend_from_slice(data);
    let m = received(ifp, &b);
    if bad_sum {
        b[42..44].copy_from_slice(&0xdeadu16.to_ne_bytes());
    } else {
        let sum = in6_cksum(m, IPPROTO_ICMPV6 as u8, 40, len as u32);
        b[42..44].copy_from_slice(&sum.to_ne_bytes());
    }
    m_freem(m);
    received(ifp, &b)
}

/// The echo request the tests send: id 0x1234, sequence 7, 12 bytes of data.
const ECHO_DATA: &[u8] = b"ping6-data!!";

fn echo_request(ifp: &Ifnet, src: In6Addr, dst: In6Addr) -> &'static Mbuf {
    icmp6_packet(
        ifp,
        src,
        dst,
        64,
        [ICMP6_ECHO_REQUEST, 0, 0, 0, 0x12, 0x34, 0, 7],
        ECHO_DATA,
        false,
    )
}

/// The ICMPv6 header of the packet `b` at offset 40.
fn header_at_40(b: &[u8]) -> Icmp6Hdr {
    // SAFETY: `b` holds at least 48 bytes; `Icmp6Hdr` is plain data.
    unsafe { ptr::read_unaligned(b[40..].as_ptr().cast::<Icmp6Hdr>()) }
}

/// The IPv6 header at the start of `b`.
fn ip6_of(b: &[u8]) -> Ip6Hdr {
    // SAFETY: `b` holds at least 40 bytes; `Ip6Hdr` is plain data.
    unsafe { ptr::read_unaligned(b.as_ptr().cast::<Ip6Hdr>()) }
}

/// A packet of `parts` in a chain of mbufs, one part each.
fn chain(parts: &[&[u8]]) -> &'static Mbuf {
    let total: usize = parts.iter().map(|p| p.len()).sum();
    let head = test_packet(parts[0]);
    let mut last = head;
    for p in &parts[1..] {
        let m = m_get(M_DONTWAIT, MT_DATA).expect("mbuf");
        // SAFETY: a fresh mbuf has MLEN bytes at m_data; the parts are small.
        unsafe { ptr::copy_nonoverlapping(p.as_ptr(), m.m_data().get(), p.len()) };
        m.m_len().set(p.len() as u32);
        last.m_next().set(Some(m));
        last = m;
    }
    head.m_pkthdr().len.set(total as i32);
    head
}

#[test]
fn echo_request_is_answered_with_an_echo_reply() {
    let (_g, ifp) = net6();
    let m = echo_request(ifp, PEER6, OURS6);
    let req = bytes(m);
    let hdr = header_at_40(&req);

    let (reply, local) = icmp6_echo_reply(m, 40, hdr);
    assert!(local, "the request is kept for the raw sockets");
    let reply = reply.expect("a reply");
    let r = bytes(reply);

    // Addresses swapped, the reply comes from the address that was asked.
    let ip6 = ip6_of(&r);
    assert_eq!((ip6.ip6_src, ip6.ip6_dst), (OURS6, PEER6));
    assert_eq!(ip6.ip6_vfc(), IPV6_VERSION);
    assert_eq!(ip6.ip6_nxt, IPPROTO_ICMPV6 as u8);
    assert_eq!(i32::from(ip6.ip6_hlim), IP6_DEFHLIM.load(Ordering::Relaxed));
    assert_eq!(ip6.ip6_flow & !htonl(0xf000_0000), 0, "no flow label");
    // Echo reply, code 0, checksum left for ip6_output, id, sequence and data as they came.
    assert_eq!((r[40], r[41]), (ICMP6_ECHO_REPLY, 0));
    assert_eq!(r[42..44], [0, 0]);
    assert_eq!(r[44..], req[44..]);
    assert_ne!(
        reply.m_pkthdr().csum_flags.get() & M_ICMP_CSUM_OUT,
        0,
        "ip6_output has the checksum to make"
    );
    // The request itself is untouched.
    let after = bytes(m);
    assert_eq!(after, req);
    assert_eq!(after[40], ICMP6_ECHO_REQUEST);

    m_freem(reply);
    m_freem(m);
}

#[test]
fn echo_reply_of_a_split_packet_gets_its_own_header_mbuf() {
    let (_g, ifp) = net6();
    let full = echo_request(ifp, PEER6, OURS6);
    let req = bytes(full);
    m_freem(full);

    // The IPv6 header alone in the first mbuf: the ICMPv6 header is not in it, so the reply
    // is built in a fresh mbuf in front of the data.
    let m = chain(&[&req[..40], &req[40..]]);
    m.m_pkthdr().ph_ifidx.set(ifp.if_index.get());
    let (reply, local) = icmp6_echo_reply(m, 40, header_at_40(&req));
    assert!(local);
    let reply = reply.expect("a reply");
    let r = bytes(reply);
    assert_eq!(r.len(), req.len());
    let ip6 = ip6_of(&r);
    assert_eq!((ip6.ip6_src, ip6.ip6_dst), (OURS6, PEER6));
    assert_eq!((r[40], r[41], r[42], r[43]), (ICMP6_ECHO_REPLY, 0, 0, 0));
    assert_eq!(r[44..], req[44..]);
    // The request still has its data, and its type.
    let kept = bytes(m);
    assert_eq!(kept, req);

    m_freem(reply);
    m_freem(m);
}

#[test]
fn echo_request_to_a_stranger_is_not_answered_without_a_route() {
    let (_g, ifp) = net6();
    // The peer is on a prefix nobody routes: no reply, the packet is dropped.
    let m = echo_request(ifp, a6("2001:db8::5"), OURS6);
    let req = bytes(m);
    let (reply, local) = icmp6_echo_reply(m, 40, header_at_40(&req));
    assert!(reply.is_none());
    assert!(local);
    m_freem(m);
}

#[test]
fn icmp6_input_counts_and_answers_an_echo_request() {
    let (_g, ifp) = net6();
    let inreq = hist(Icmp6statCounters::Icp6sInhist, ICMP6_ECHO_REQUEST);
    let outrep = hist(Icmp6statCounters::Icp6sOuthist, ICMP6_ECHO_REPLY);
    let reflect = icmp6stat(Icmp6statCounters::Icp6sReflect);

    let mut mp = Some(echo_request(ifp, PEER6, OURS6));
    let mut off = 40;
    let r = icmp6_input(&mut mp, &mut off, IPPROTO_ICMPV6, 10, None);
    let _ = r;
    assert_eq!(
        hist(Icmp6statCounters::Icp6sInhist, ICMP6_ECHO_REQUEST),
        inreq + 1
    );
    assert_eq!(
        hist(Icmp6statCounters::Icp6sOuthist, ICMP6_ECHO_REPLY),
        outrep + 1
    );
    assert_eq!(icmp6stat(Icmp6statCounters::Icp6sReflect), reflect + 1);
}

#[test]
fn icmp6_input_drops_what_is_malformed() {
    let (_g, ifp) = net6();
    let case = |m: &'static Mbuf| {
        let mut mp = Some(m);
        let mut off = 40;
        let r = icmp6_input(&mut mp, &mut off, IPPROTO_ICMPV6, 10, None);
        (r, mp.is_none())
    };

    // Shorter than an ICMPv6 header.
    let before = icmp6stat(Icmp6statCounters::Icp6sTooshort);
    let mut b = ip6_bytes(PEER6, OURS6, IPPROTO_ICMPV6 as u8, 64, 4);
    b.extend_from_slice(&[ICMP6_ECHO_REQUEST, 0, 0, 0]);
    assert_eq!(case(received(ifp, &b)), (IPPROTO_DONE, true));
    assert_eq!(icmp6stat(Icmp6statCounters::Icp6sTooshort), before + 1);

    // A wrong checksum.
    let before = icmp6stat(Icmp6statCounters::Icp6sChecksum);
    let m = icmp6_packet(
        ifp,
        PEER6,
        OURS6,
        64,
        [ICMP6_ECHO_REQUEST, 0, 0, 0, 0, 1, 0, 1],
        &[],
        true,
    );
    assert_eq!(case(m), (IPPROTO_DONE, true));
    assert_eq!(icmp6stat(Icmp6statCounters::Icp6sChecksum), before + 1);

    // A code the type does not have, and messages shorter than their type needs.
    let msgs: [(u8, u8, usize, Icmp6statCounters); 6] = [
        (ICMP6_ECHO_REQUEST, 1, 0, Icmp6statCounters::Icp6sBadcode),
        (ICMP6_ECHO_REPLY, 3, 0, Icmp6statCounters::Icp6sBadcode),
        (MLD_LISTENER_QUERY, 0, 0, Icmp6statCounters::Icp6sBadlen),
        (ND_NEIGHBOR_SOLICIT, 0, 4, Icmp6statCounters::Icp6sBadlen),
        (ND_ROUTER_ADVERT, 0, 4, Icmp6statCounters::Icp6sBadlen),
        (ND_REDIRECT, 0, 8, Icmp6statCounters::Icp6sBadlen),
    ];
    for (ty, code, extra, counter) in msgs {
        let before = icmp6stat(counter);
        let m = icmp6_packet(
            ifp,
            PEER6,
            OURS6,
            255,
            [ty, code, 0, 0, 0, 0, 0, 0],
            &std::vec![0u8; extra],
            false,
        );
        let mut mp = Some(m);
        let mut off = 40;
        let _ = icmp6_input(&mut mp, &mut off, IPPROTO_ICMPV6, 10, None);
        assert_eq!(icmp6stat(counter), before + 1, "type {ty} code {code}");
    }
}

/// A UDP packet from `src` to `dst`, 20 bytes of payload, received on `ifp`.
fn udp_packet(ifp: &Ifnet, src: In6Addr, dst: In6Addr) -> (&'static Mbuf, Vec<u8>) {
    let mut b = ip6_bytes(src, dst, IPPROTO_UDP, 64, 20);
    b.extend_from_slice(&[0x5a; 20]);
    (received(ifp, &b), b)
}

#[test]
fn error_message_quotes_the_packet_and_counts_its_kind() {
    let (_g, ifp) = net6();
    let (m, orig) = udp_packet(ifp, PEER6, OURS6);
    let errors = icmp6stat(Icmp6statCounters::Icp6sError);
    let noport = icmp6stat(Icmp6statCounters::Icp6sOdstUnreachNoport);
    let out = hist(Icmp6statCounters::Icp6sOuthist, ICMP6_DST_UNREACH);

    let e = icmp6_do_error(m, ICMP6_DST_UNREACH, ICMP6_DST_UNREACH_NOPORT, 0).expect("an error");
    let b = bytes(e);
    assert_eq!(b.len(), 40 + 8 + orig.len());
    let ip6 = ip6_of(&b);
    assert_eq!(
        (ip6.ip6_src, ip6.ip6_dst),
        (PEER6, OURS6),
        "reflected later"
    );
    assert_eq!(
        (b[40], b[41], b[44..48].to_vec()),
        (ICMP6_DST_UNREACH, ICMP6_DST_UNREACH_NOPORT, std::vec![0; 4])
    );
    assert_eq!(b[48..], orig[..], "the offending packet follows");
    assert_eq!(e.m_pkthdr().ph_ifidx.get(), 0);
    assert_eq!(icmp6stat(Icmp6statCounters::Icp6sError), errors + 1);
    assert_eq!(
        icmp6stat(Icmp6statCounters::Icp6sOdstUnreachNoport),
        noport + 1
    );
    assert_eq!(
        hist(Icmp6statCounters::Icp6sOuthist, ICMP6_DST_UNREACH),
        out + 1
    );

    // The reflected error goes back to the sender, from us.
    let mut mp = Some(e);
    icmp6_reflect(&mut mp, 40, None).expect("reflect");
    let b = bytes(mp.expect("packet"));
    let ip6 = ip6_of(&b);
    assert_eq!((ip6.ip6_src, ip6.ip6_dst), (OURS6, PEER6));
    assert_eq!(b[48..], orig[..]);
}

#[test]
fn error_message_carries_the_pointer_or_mtu() {
    let (_g, ifp) = net6();
    let (m, _) = udp_packet(ifp, PEER6, OURS6);
    let e = icmp6_do_error(m, ICMP6_PACKET_TOO_BIG, 0, 1400).expect("an error");
    let b = bytes(e);
    assert_eq!(b[44..48], 1400u32.to_be_bytes());
    assert_eq!(header_at_40(&b).icmp6_mtu(), htonl(1400));
    m_freem(e);

    let (m, _) = udp_packet(ifp, PEER6, OURS6);
    let e = icmp6_do_error(m, ICMP6_PARAM_PROB, ICMP6_PARAMPROB_HEADER, 6).expect("an error");
    assert_eq!(bytes(e)[44..48], 6u32.to_be_bytes());
    m_freem(e);
}

#[test]
fn no_error_about_an_error() {
    let (_g, ifp) = net6();
    let canterror = icmp6stat(Icmp6statCounters::Icp6sCanterror);
    // ICMPv6 errors (any type below the echo request) and redirects: no error about them.
    for ty in [
        ICMP6_DST_UNREACH,
        ICMP6_PACKET_TOO_BIG,
        ICMP6_TIME_EXCEEDED,
        ND_REDIRECT,
    ] {
        let m = icmp6_packet(
            ifp,
            PEER6,
            OURS6,
            64,
            [ty, 0, 0, 0, 0, 0, 0, 0],
            &[0; 8],
            false,
        );
        assert!(
            icmp6_do_error(m, ICMP6_DST_UNREACH, 0, 0).is_none(),
            "type {ty}"
        );
    }
    assert_eq!(icmp6stat(Icmp6statCounters::Icp6sCanterror), canterror + 4);

    // An echo request is informational: it gets its error.
    let m = echo_request(ifp, PEER6, OURS6);
    let e = icmp6_do_error(m, ICMP6_DST_UNREACH, ICMP6_DST_UNREACH_ADDR, 0).expect("an error");
    m_freem(e);
}

#[test]
fn errors_about_multicast_and_unspecified_sources_are_suppressed() {
    let (_g, ifp) = net6();
    let mcast = a6("ff02::1:ff00:5");
    // To a multicast destination: no error, except Packet Too Big and a Parameter Problem
    // about an option.
    let (m, _) = udp_packet(ifp, PEER6, mcast);
    assert!(icmp6_do_error(m, ICMP6_DST_UNREACH, ICMP6_DST_UNREACH_NOPORT, 0).is_none());
    let (m, _) = udp_packet(ifp, PEER6, mcast);
    assert!(icmp6_do_error(m, ICMP6_TIME_EXCEEDED, 0, 0).is_none());
    let (m, _) = udp_packet(ifp, PEER6, mcast);
    let e = icmp6_do_error(m, ICMP6_PACKET_TOO_BIG, 0, 1280).expect("too big is sent");
    m_freem(e);
    let (m, _) = udp_packet(ifp, PEER6, mcast);
    let e = icmp6_do_error(m, ICMP6_PARAM_PROB, ICMP6_PARAMPROB_OPTION, 40).expect("option");
    m_freem(e);
    let (m, _) = udp_packet(ifp, PEER6, mcast);
    assert!(icmp6_do_error(m, ICMP6_PARAM_PROB, ICMP6_PARAMPROB_HEADER, 40).is_none());

    // Link-layer multicast or broadcast.
    let (m, _) = udp_packet(ifp, PEER6, OURS6);
    m.m_flags().set(m.m_flags().get() | M_MCAST);
    assert!(icmp6_do_error(m, ICMP6_DST_UNREACH, 0, 0).is_none());

    // The sender is the unspecified address or a multicast group: nobody to tell.
    let (m, _) = udp_packet(ifp, IN6ADDR_ANY, OURS6);
    assert!(icmp6_do_error(m, ICMP6_DST_UNREACH, 0, 0).is_none());
    let (m, _) = udp_packet(ifp, mcast, OURS6);
    assert!(icmp6_do_error(m, ICMP6_DST_UNREACH, 0, 0).is_none());
}

#[test]
fn errors_are_rate_limited() {
    let (_g, ifp) = net6();
    ICMP6ERRPPSLIM.store(2, Ordering::Relaxed);
    set_ratelimit_window(1);
    let toofreq = icmp6stat(Icmp6statCounters::Icp6sToofreq);
    let mut sent = 0;
    for _ in 0..5 {
        let (m, _) = udp_packet(ifp, PEER6, OURS6);
        if let Some(e) = icmp6_do_error(m, ICMP6_DST_UNREACH, 0, 0) {
            sent += 1;
            m_freem(e);
        }
    }
    assert_eq!(sent, 2, "two per second");
    assert_eq!(icmp6stat(Icmp6statCounters::Icp6sToofreq), toofreq + 3);

    // A negative limit: none.
    reset_ratelimit();
    ICMP6ERRPPSLIM.store(-1, Ordering::Relaxed);
    for _ in 0..5 {
        let (m, _) = udp_packet(ifp, PEER6, OURS6);
        m_freem(icmp6_do_error(m, ICMP6_DST_UNREACH, 0, 0).expect("unlimited"));
    }
    reset_ratelimit();
}

#[test]
fn a_long_packet_is_truncated_to_the_minimum_mtu() {
    let (_g, ifp) = net6();
    let m = m_gethdr(M_DONTWAIT, MT_DATA).expect("mbuf");
    mclget(m, M_DONTWAIT);
    let mut b = ip6_bytes(PEER6, OURS6, IPPROTO_UDP, 64, 1460);
    b.extend_from_slice(&[0x33; 1460]);
    // SAFETY: the cluster holds 2048 bytes.
    unsafe { ptr::copy_nonoverlapping(b.as_ptr(), m.m_data().get(), b.len()) };
    m.m_len().set(b.len() as u32);
    m.m_pkthdr().len.set(b.len() as i32);
    m.m_pkthdr().ph_ifidx.set(ifp.if_index.get());

    let e = icmp6_do_error(m, ICMP6_TIME_EXCEEDED, ICMP6_TIME_EXCEED_TRANSIT, 0).expect("error");
    assert_eq!(e.m_pkthdr().len.get() as usize, IPV6_MMTU as usize);
    assert_eq!(ICMPV6_PLD_MAXLEN + 48, IPV6_MMTU as usize);
    m_freem(e);
}

#[test]
fn error_message_does_not_quote_the_scope_zone() {
    let (_g, ifp) = net6();
    let mut src = a6("fe80::5054:ff:fe00:1");
    src.set_s6_addr16(1, htons(ifp.if_index.get() as u16));
    let (m, _) = udp_packet(ifp, src, OURS6);
    let e = icmp6_do_error(m, ICMP6_DST_UNREACH, ICMP6_DST_UNREACH_NOPORT, 0).expect("error");
    let b = bytes(e);
    // The new header keeps the zone (it routes the reply); the quoted one has it cleared.
    assert_eq!(ip6_of(&b).ip6_dst, OURS6);
    assert_eq!(
        ip6_of(&b).ip6_src.s6_addr16(1),
        htons(ifp.if_index.get() as u16)
    );
    assert_eq!(ip6_of(&b[48..]).ip6_src.s6_addr16(1), 0);
    m_freem(e);
}

#[test]
fn reflect_refuses_what_cannot_be_reflected() {
    let (_g, ifp) = net6();

    // Too short to hold an IPv6 header before the ICMPv6 one.
    let m = echo_request(ifp, PEER6, OURS6);
    let mut mp = Some(m);
    assert_eq!(icmp6_reflect(&mut mp, 20, None), Err(Errno::EHOSTUNREACH));
    assert!(mp.is_none());

    // A packet that has been looping.
    let m = echo_request(ifp, PEER6, OURS6);
    m.m_pkthdr().ph_loopcnt.set(M_MAXLOOP);
    let mut mp = Some(m);
    assert_eq!(icmp6_reflect(&mut mp, 40, None), Err(Errno::ELOOP));
    assert!(mp.is_none());

    // No route back to the sender.
    let m = echo_request(ifp, a6("2001:db8::5"), OURS6);
    let mut mp = Some(m);
    assert_eq!(icmp6_reflect(&mut mp, 40, None), Err(Errno::EHOSTUNREACH));
    assert!(mp.is_none());
}

#[test]
fn reflect_strips_extension_headers_and_picks_the_source() {
    let (_g, ifp) = net6();
    // An echo request that arrived with a destination options header (8 bytes of padding).
    let req = echo_request(ifp, PEER6, OURS6);
    let b = bytes(req);
    m_freem(req);
    let mut with_ext = b[..40].to_vec();
    with_ext[6] = 60; // next header: destination options
    with_ext.extend_from_slice(&[IPPROTO_ICMPV6 as u8, 0, 1, 4, 0, 0, 0, 0]);
    with_ext.extend_from_slice(&b[40..]);
    let m = received(ifp, &with_ext);

    let mut mp = Some(m);
    icmp6_reflect(&mut mp, 48, None).expect("reflect");
    let r = bytes(mp.expect("packet"));
    assert_eq!(r.len(), b.len(), "the extension header is gone");
    let ip6 = ip6_of(&r);
    assert_eq!(ip6.ip6_nxt, IPPROTO_ICMPV6 as u8);
    assert_eq!((ip6.ip6_src, ip6.ip6_dst), (OURS6, PEER6));
    assert_eq!(r[40..42], b[40..42]);
    assert_eq!(r[42..44], [0, 0], "the checksum is made by ip6_output");
    assert_eq!(r[44..], b[44..]);

    // Sent to a multicast group (a ping of ff02::1): the source is an address of ours, and the
    // answer goes to the sender.
    let m = echo_request(ifp, PEER6, IN6ADDR_LINKLOCAL_ALLNODES);
    let mut mp = Some(m);
    icmp6_reflect(&mut mp, 40, None).expect("reflect");
    let r = bytes(mp.expect("packet"));
    let ip6 = ip6_of(&r);
    assert_eq!(ip6.ip6_dst, PEER6);
    assert!(!in6_is_addr_unspecified(&ip6.ip6_src) && !in6_is_addr_multicast(&ip6.ip6_src));
    assert!(
        in6ifa_ifpwithaddr_any(ifp, &ip6.ip6_src),
        "the source is one of the interface's addresses"
    );

    // The source can be given: the route to it picks the interface and the address.
    let m = echo_request(ifp, PEER6, OURS6);
    let sa = SockaddrIn6::with_addr(PEER6);
    let mut mp = Some(m);
    icmp6_reflect(&mut mp, 40, Some(&sa)).expect("reflect with sa");
    m_freem(mp.expect("packet"));
}

/// Whether `ifp` has the IPv6 address `a`, whatever its flags.
fn in6ifa_ifpwithaddr_any(ifp: &Ifnet, a: &In6Addr) -> bool {
    crate::netinet6::in6::in6ifa_ifpwithaddr(ifp, a).is_some()
}

#[test]
fn redirects_are_validated() {
    let (_g, ifp) = net6();
    let bad = || icmp6stat(Icmp6statCounters::Icp6sBadredirect);
    let ll = |last: u16| {
        let mut a = a6("fe80::");
        a.set_s6_addr16(7, htons(last));
        a
    };
    let dst = a6("2001:db8::99");
    let redirect = |src: In6Addr, hlim: u8, target: In6Addr, redirected: In6Addr| {
        let mut data = target.s6_addr.to_vec();
        data.extend_from_slice(&redirected.s6_addr);
        // The redirect: the 8 bytes of the header, target and destination, no options.
        let m = icmp6_packet(
            ifp,
            src,
            OURS6,
            hlim,
            [ND_REDIRECT, 0, 0, 0, 0, 0, 0, 0],
            &data,
            false,
        );
        m.m_pkthdr().ph_ifidx.set(ifp.if_index.get());
        m
    };

    // The interface does not take redirects (no autoconf): ignored without a count.
    let before = bad();
    icmp6_redirect_input(redirect(ll(1), 255, ll(1), dst), 40);
    assert_eq!(bad(), before);

    // Taking them: each rule is checked.
    ifp.if_xflags.set(ifp.if_xflags.get() | IFXF_AUTOCONF6);
    let cases: [(&str, In6Addr, u8, In6Addr, In6Addr); 5] = [
        ("source not link-local", PEER6, 255, ll(1), dst),
        ("hop limit not 255", ll(1), 64, ll(1), dst),
        ("multicast destination", ll(1), 255, ll(1), a6("ff02::1")),
        (
            "no route to the destination's gateway",
            ll(1),
            255,
            ll(1),
            dst,
        ),
        (
            "target neither a router nor the destination",
            ll(1),
            255,
            PEER6,
            dst,
        ),
    ];
    for (what, src, hlim, target, redirected) in cases {
        let before = bad();
        icmp6_redirect_input(redirect(src, hlim, target, redirected), 40);
        assert_eq!(bad(), before + 1, "{what}");
    }

    // A router takes no redirects.
    IP6_FORWARDING.store(1, Ordering::Relaxed);
    let before = bad();
    icmp6_redirect_input(redirect(PEER6, 255, ll(1), dst), 40);
    IP6_FORWARDING.store(0, Ordering::Relaxed);
    assert_eq!(bad(), before);
    ifp.if_xflags.set(ifp.if_xflags.get() & !IFXF_AUTOCONF6);
}

#[test]
fn a_host_sends_no_redirects() {
    let (_g, ifp) = net6();
    let errors = hist(Icmp6statCounters::Icp6sOuthist, ND_REDIRECT);
    let count = icmp6stat(Icmp6statCounters::Icp6sOredirect);
    let (m, _) = udp_packet(ifp, PEER6, a6("fd00:77::99"));
    // SAFETY: a local `sockaddr_in6`.
    let rt = unsafe {
        let mut dst = SockaddrIn6::with_addr(a6("fd00:77::99"));
        rtalloc(sin6tosa(&mut dst), RT_RESOLVE, 0)
    };
    let rt = rt.expect("the connected route");
    icmp6_redirect_output(m, rt);
    // Counted as an attempt, not sent.
    assert_eq!(icmp6stat(Icmp6statCounters::Icp6sOredirect), count + 1);
    assert_eq!(hist(Icmp6statCounters::Icp6sOuthist, ND_REDIRECT), errors);
    rtfree(Some(rt));
}

/// `route add -inet6 default <gw>`: the route and the next hop it caches.
fn add_default_route(ifp: &'static Ifnet, gw: In6Addr) {
    let mut dst = SockaddrIn6::with_addr(IN6ADDR_ANY);
    let mut mask = SockaddrIn6::with_addr(IN6ADDR_ANY);
    let mut gate = SockaddrIn6::with_addr(gw);
    for s in [&mut dst, &mut mask, &mut gate] {
        s.sin6_len = size_of::<SockaddrIn6>() as u8;
    }
    let mut info = RtAddrinfo::new();
    info.rti_info[RTAX_DST] = sin6tosa(&mut dst);
    info.rti_info[RTAX_NETMASK] = sin6tosa(&mut mask);
    info.rti_info[RTAX_GATEWAY] = sin6tosa(&mut gate);
    info.rti_flags = RTF_GATEWAY | RTF_STATIC;
    // SAFETY: a local `sockaddr_in6`.
    info.rti_ifa = unsafe { ifaof_ifpforaddr(sin6tosa(&mut gate), ifp) };
    net_lock();
    let mut rt = None;
    // SAFETY: the addresses are locals.
    unsafe { rtrequest(RTM_ADD, &mut info, 0, Some(&mut rt), 0) }.expect("default route");
    rtfree(rt);
    net_unlock();
}

/// The ICMPv6 control parameters about packet `m`: Packet Too Big with `mtu`, quoting a UDP
/// packet to `dst`.
struct TooBig {
    hdr: Icmp6Hdr,
    dst: In6Addr,
}

impl TooBig {
    fn new(mtu: u32, dst: In6Addr) -> Self {
        let mut hdr = Icmp6Hdr::zeroed();
        hdr.icmp6_type = ICMP6_PACKET_TOO_BIG;
        hdr.set_icmp6_mtu(htonl(mtu));
        Self { hdr, dst }
    }

    fn param(&mut self, m: &'static Mbuf) -> Ip6ctlparam {
        let mut p = Ip6ctlparam::new();
        p.ip6c_m = Some(m);
        p.ip6c_icmp6 = ptr::from_mut(&mut self.hdr);
        p.ip6c_finaldst = ptr::from_mut(&mut self.dst);
        p
    }
}

static MTU_NOTIFIED: std::sync::Mutex<Vec<(In6Addr, u32)>> = std::sync::Mutex::new(Vec::new());

fn record_mtu_change(dst: &SockaddrIn6, rtableid: u32) {
    MTU_NOTIFIED
        .lock()
        .unwrap_or_else(|e| e.into_inner())
        .push((dst.sin6_addr, rtableid));
}

#[test]
fn path_mtu_discovery_clones_a_host_route_and_lowers_its_mtu() {
    let (_g, ifp) = net6();
    icmp6_mtudisc_callback_register(record_mtu_change);
    icmp6_mtudisc_callback_register(record_mtu_change); // once
    MTU_NOTIFIED
        .lock()
        .unwrap_or_else(|e| e.into_inner())
        .clear();
    // A destination beyond the router: the route to it is cloned from the default route.
    add_default_route(ifp, PEER6);
    // A path MTU below the link's: nothing is sent here, the MTU only bounds the learned one.
    ifp.if_mtu.set(1500);
    let dst = a6("2001:db8::99");
    let (m, _) = udp_packet(ifp, PEER6, dst);
    let pmtuchg = icmp6stat(Icmp6statCounters::Icp6sPmtuchg);
    net_lock();

    // Below the minimum MTU: ignored.
    let mut tb = TooBig::new(1200, dst);
    icmp6_mtudisc_update(&tb.param(m), true);
    assert_eq!(icmp6stat(Icmp6statCounters::Icp6sPmtuchg), pmtuchg);
    assert!(
        MTU_NOTIFIED
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .is_empty()
    );

    // A real one: the route holds the MTU; the callbacks hear of it.
    let mut tb = TooBig::new(1400, dst);
    icmp6_mtudisc_update(&tb.param(m), true);
    assert_eq!(icmp6stat(Icmp6statCounters::Icp6sPmtuchg), pmtuchg + 1);
    let sin6 = SockaddrIn6::with_addr(dst);
    let rt = icmp6_mtudisc_clone(&sin6, 0, false).expect("the host route");
    assert_ne!(rt.rt_flags.get() & RTF_HOST, 0);
    assert_eq!(rt.rt_mtu().load(Ordering::Relaxed), 1400);
    assert_eq!(rt_timer_queue_count(&ICMP6_MTUDISC_TIMEOUT_Q), 1);
    assert_eq!(
        *MTU_NOTIFIED.lock().unwrap_or_else(|e| e.into_inner()),
        std::vec![(dst, 0)]
    );

    // A larger one does not raise it, a smaller one lowers it.
    let mut tb = TooBig::new(1450, dst);
    icmp6_mtudisc_update(&tb.param(m), true);
    assert_eq!(rt.rt_mtu().load(Ordering::Relaxed), 1400);
    let mut tb = TooBig::new(1300, dst);
    icmp6_mtudisc_update(&tb.param(m), true);
    assert_eq!(rt.rt_mtu().load(Ordering::Relaxed), 1300);

    // The timer fires: the dynamic host route goes away.
    icmp6_mtudisc_timeout(rt, 0);
    rtfree(Some(rt));
    let again = icmp6_mtudisc_clone(&sin6, 0, false).expect("cloned afresh");
    assert_eq!(again.rt_mtu().load(Ordering::Relaxed), 0);
    rtfree(Some(again));
    net_unlock();
    m_freem(m);
}

#[test]
fn unvalidated_mtu_changes_need_room_in_the_table() {
    let (_g, ifp) = net6();
    let dst = a6("fd00:77::98");
    let (m, _) = udp_packet(ifp, PEER6, dst);
    net_lock();
    // No room (lowat 0 with one route cloned already): an unvalidated report is ignored.
    ICMP6_MTUDISC_LOWAT.store(0, Ordering::Relaxed);
    let mut tb = TooBig::new(1400, dst);
    let before = rt_timer_queue_count(&ICMP6_MTUDISC_TIMEOUT_Q);
    icmp6_mtudisc_update(&tb.param(m), false);
    let after = rt_timer_queue_count(&ICMP6_MTUDISC_TIMEOUT_Q);
    ICMP6_MTUDISC_LOWAT.store(256, Ordering::Relaxed);
    assert!(before == 0 || after == before);
    net_unlock();
    m_freem(m);
}

#[test]
fn the_icmp6_filter_is_a_socket_option() {
    use crate::kern::uipc_socket::{soclose, socreate};
    use crate::sys::socket::SOCK_RAW;
    let (_g, _t, _p) = crate::netinet::in_pcb::tests::setup();
    crate::netinet::raw_ip::rip_init();
    // A raw socket of the inet domain stands in for a raw ICMPv6 socket (no `AF_INET6`
    // socket is possible before `rip6_attach`): the option code only needs the pcb.
    let so = socreate(i32::from(crate::sys::socket::AF_INET), SOCK_RAW, 1).expect("socket");
    let inp = sotoinpcb(so).expect("pcb");
    let filt: &'static mut Icmp6Filter =
        std::boxed::Box::leak(std::boxed::Box::new(Icmp6Filter { icmp6_filt: [0; 8] }));
    inp.inp_icmp6filt.set(NonNull::new(filt));

    // Wrong level, wrong option.
    assert_eq!(
        icmp6_ctloutput(PRCO_SETOPT, so, 0, ICMP6_FILTER, None),
        Err(Errno::EINVAL)
    );
    assert_eq!(
        icmp6_ctloutput(PRCO_SETOPT, so, IPPROTO_ICMPV6, 99, None),
        Err(Errno::ENOPROTOOPT)
    );
    // The wrong size of filter.
    let m = m_get(M_DONTWAIT, MT_DATA).expect("mbuf");
    m.m_len().set(8);
    assert_eq!(
        icmp6_ctloutput(PRCO_SETOPT, so, IPPROTO_ICMPV6, ICMP6_FILTER, Some(m)),
        Err(Errno::EMSGSIZE)
    );

    // Set a filter that blocks everything but echo replies, and read it back.
    let mut want = Icmp6Filter { icmp6_filt: [0; 8] };
    want.setblockall();
    want.setpass(ICMP6_ECHO_REPLY);
    m.m_len().set(size_of::<Icmp6Filter>() as u32);
    // SAFETY: an mbuf holds MLEN bytes, more than a filter.
    unsafe {
        ptr::copy_nonoverlapping(
            ptr::from_ref(&want).cast::<u8>(),
            mtod::<u8>(m),
            size_of::<Icmp6Filter>(),
        )
    };
    icmp6_ctloutput(PRCO_SETOPT, so, IPPROTO_ICMPV6, ICMP6_FILTER, Some(m)).expect("set");
    assert!(filt.willpass(ICMP6_ECHO_REPLY) && filt.willblock(ICMP6_ECHO_REQUEST));

    let out = m_get(M_DONTWAIT, MT_DATA).expect("mbuf");
    icmp6_ctloutput(PRCO_GETOPT, so, IPPROTO_ICMPV6, ICMP6_FILTER, Some(out)).expect("get");
    assert_eq!(out.m_len().get() as usize, size_of::<Icmp6Filter>());
    assert_eq!(bytes_of(out), bytes_of_filter(&want));

    // No filter, no option.
    inp.inp_icmp6filt.set(None);
    assert_eq!(
        icmp6_ctloutput(PRCO_GETOPT, so, IPPROTO_ICMPV6, ICMP6_FILTER, Some(out)),
        Err(Errno::EINVAL)
    );
    m_freem(m);
    m_freem(out);
    soclose(so, 0).expect("close");
    crate::netinet::in_pcb::tests::teardown();
}

fn bytes_of(m: &Mbuf) -> Vec<u8> {
    let mut v = std::vec![0u8; m.m_len().get() as usize];
    m_copydata(m, 0, &mut v);
    v
}

fn bytes_of_filter(f: &Icmp6Filter) -> Vec<u8> {
    f.icmp6_filt.iter().flat_map(|w| w.to_ne_bytes()).collect()
}

/// A sysctl read of the integer `mib`.
fn sysctl_get(mib: i32) -> i32 {
    let mut v = 0i32;
    let mut len = size_of::<i32>();
    // SAFETY: a destination of the right size; `copyout` on the host copies there.
    let r = unsafe { sysctl_read_int(mib, ptr::from_mut(&mut v) as usize, &mut len) };
    r.expect("sysctl");
    v
}

/// Reads the integer sysctl `mib` into the user address `oldp`.
///
/// # Safety
///
/// `oldp` is the address of a writable `i32`.
unsafe fn sysctl_read_int(mib: i32, oldp: usize, len: &mut usize) -> Result<(), Errno> {
    icmp6_sysctl(&[mib], oldp, len, 0, 0)
}

#[test]
fn the_sysctls_read_and_write_the_variables() {
    let (_g, _ifp) = net6();
    assert_eq!(
        sysctl_get(ICMPV6CTL_ERRPPSLIMIT),
        ICMP6ERRPPSLIM.load(Ordering::Relaxed)
    );
    assert_eq!(
        sysctl_get(ICMPV6CTL_REDIRTIMEOUT),
        ICMP6_REDIRTIMEOUT.load(Ordering::Relaxed)
    );
    assert_eq!(sysctl_get(ICMPV6CTL_ND6_QUEUED), 0);
    assert_eq!(
        sysctl_get(ICMPV6CTL_MTUDISC_HIWAT),
        ICMP6_MTUDISC_HIWAT.load(Ordering::Relaxed)
    );

    // The error rate has bounds (-1..=1000).
    let mut len = size_of::<i32>();
    let mut new = 5000i32;
    assert_eq!(
        icmp6_sysctl(
            &[ICMPV6CTL_ERRPPSLIMIT],
            0,
            &mut len,
            ptr::from_mut(&mut new) as usize,
            size_of::<i32>()
        ),
        Err(Errno::EINVAL)
    );
    // Names with more than one level, and unknown ones.
    assert_eq!(
        icmp6_sysctl(&[ICMPV6CTL_STATS, 1], 0, &mut len, 0, 0),
        Err(Errno::ENOTDIR)
    );
    assert!(icmp6_sysctl(&[999], 0, &mut len, 0, 0).is_err());

    // The statistics are a `struct icmp6stat`.
    let mut size = 0usize;
    icmp6_sysctl(&[ICMPV6CTL_STATS], 0, &mut size, 0, 0).expect("size of the statistics");
    assert_eq!(size, size_of::<Icmp6stat>());
}
