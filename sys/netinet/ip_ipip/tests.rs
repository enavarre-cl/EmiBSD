//! Host tests for IP-in-IP over IPv6: `ipip_output` puts an IPv6 header in front of an IPv4 or
//! IPv6 packet (or an IPv4 header in front of an IPv6 one), keeps the inner traffic class
//! and clears the scope of link-local inner addresses; `ipip_input_if` takes an IPv6 header
//! as the outer or as the inner one.

use std::sync::MutexGuard;
use std::vec;
use std::vec::Vec;

use super::*;
use crate::kern::uipc_mbuf::{m_copydata, m_freem};
use crate::net::if_::tests::{test_ifnet, test_packet};
use crate::netinet::ip_ipsp::SockaddrUnion;
use crate::netinet6::ip6_input::IP6COUNTERS;
use crate::netinet6::ip6_var::Ip6statCounters;
use crate::sys::mbuf::M_AUTH;

fn setup() -> (MutexGuard<'static, ()>, MutexGuard<'static, ()>) {
    crate::netinet::ip_input::tests::setup()
}

/// An IPv6 address from its eight 16-bit words.
fn a6(w: [u16; 8]) -> In6Addr {
    let mut a = [0u8; 16];
    for (i, w) in w.iter().enumerate() {
        a[2 * i..2 * i + 2].copy_from_slice(&w.to_be_bytes());
    }
    In6Addr::new(a)
}

fn su6(a: In6Addr) -> SockaddrUnion {
    SockaddrUnion::from_sin6(&SockaddrIn6 {
        sin6_len: size_of::<SockaddrIn6>() as u8,
        sin6_family: AF_INET6,
        sin6_addr: a,
        ..SockaddrIn6::default()
    })
}

fn su4(a: [u8; 4]) -> SockaddrUnion {
    SockaddrUnion::from_sin(&SockaddrIn {
        sin_len: size_of::<SockaddrIn>() as u8,
        sin_family: AF_INET,
        sin_addr: InAddr {
            s_addr: u32::from_ne_bytes(a),
        },
        ..SockaddrIn::default()
    })
}

/// A TDB from `src` to `dst`.
fn tdb(src: SockaddrUnion, dst: SockaddrUnion) -> Tdb {
    let t = Tdb::new();
    t.tdb_src.set(src);
    t.tdb_dst.set(dst);
    t
}

/// An IPv4 packet with type of service `tos` and 8 bytes of payload.
fn ip4_packet(tos: u8) -> Vec<u8> {
    let mut p = vec![0x45, tos];
    p.extend_from_slice(&28u16.to_be_bytes());
    p.extend_from_slice(&[0x12, 0x34, 0x40, 0x00, 64, 17, 0, 0]);
    p.extend_from_slice(&[10, 0, 0, 1, 10, 0, 0, 2]);
    p.extend_from_slice(&[1, 2, 3, 4, 5, 6, 7, 8]);
    p
}

/// An IPv6 packet with traffic class `tclass`, UDP, and 8 bytes of payload.
fn ip6_packet(tclass: u8, src: In6Addr, dst: In6Addr) -> Vec<u8> {
    let flow = 0x6000_0000u32 | (u32::from(tclass) << 20) | 0x12345;
    let mut p = flow.to_be_bytes().to_vec();
    p.extend_from_slice(&8u16.to_be_bytes());
    p.extend_from_slice(&[17, 64]);
    p.extend_from_slice(&src.s6_addr);
    p.extend_from_slice(&dst.s6_addr);
    p.extend_from_slice(&[1, 2, 3, 4, 5, 6, 7, 8]);
    p
}

fn bytes(m: &Mbuf) -> Vec<u8> {
    let mut v = vec![0u8; m.m_pkthdr().len.get() as usize];
    m_copydata(m, 0, &mut v);
    v
}

fn counter(c: IpipstatCounters) -> u64 {
    IPIPCOUNTERS[c as usize].load(Ordering::Relaxed)
}

#[test]
fn ipv4_travels_in_an_ipv6_tunnel() {
    let _g = setup();
    let (src, dst) = (
        a6([0x2001, 0xdb8, 0, 0, 0, 0, 0, 1]),
        a6([0x2001, 0xdb8, 0, 0, 0, 0, 0, 2]),
    );
    let t = tdb(su6(src), su6(dst));

    // The inner type of service carries over (CE becomes ECT(0)).
    for (itos, otos) in [(0xb8u8, 0xb8u8), (0xbb, 0xba), (0, 0)] {
        let inner = ip4_packet(itos);
        let mut mp = Some(test_packet(&inner));
        ipip_output(&mut mp, &t).expect("encapsulated");
        let out = bytes(mp.expect("the packet"));
        assert_eq!(out.len(), 40 + inner.len());
        assert_eq!(out[0] >> 4, 6, "version");
        let flow = u32::from_be_bytes([out[0], out[1], out[2], out[3]]);
        assert_eq!(flow & 0x000f_ffff, 0, "no flow label");
        assert_eq!((flow >> 20) & 0xff, u32::from(otos), "traffic class");
        assert_eq!(
            usize::from(u16::from_be_bytes([out[4], out[5]])),
            inner.len(),
            "ip6_plen"
        );
        assert_eq!(out[6], IPPROTO_IPIP as u8, "ip6_nxt");
        assert_eq!(out[7], 64, "ip6_hlim");
        assert_eq!(&out[8..24], &src.s6_addr);
        assert_eq!(&out[24..40], &dst.s6_addr);
        assert_eq!(&out[40..], &inner[..]);
        m_freem(mp);
    }
}

#[test]
fn ipv6_travels_in_an_ipv6_tunnel_without_its_link_local_scope() {
    let _g = setup();
    let (src, dst) = (
        a6([0x2001, 0xdb8, 0, 0, 0, 0, 0, 1]),
        a6([0x2001, 0xdb8, 0, 0, 0, 0, 0, 2]),
    );
    let t = tdb(su6(src), su6(dst));

    // An inner packet between link-local addresses has its embedded scope zeroed.
    let isrc = a6([0xfe80, 1, 0, 0, 0, 0, 0, 5]);
    let idst = a6([0xfe80, 1, 0, 0, 0, 0, 0, 6]);
    let inner = ip6_packet(0xb8, isrc, idst);
    let mut mp = Some(test_packet(&inner));
    ipip_output(&mut mp, &t).expect("encapsulated");
    let out = bytes(mp.expect("the packet"));
    assert_eq!(out.len(), 88);
    assert_eq!(out[6], IPPROTO_IPV6 as u8);
    assert_eq!(
        (u32::from_be_bytes([out[0], out[1], out[2], out[3]]) >> 20) & 0xff,
        0xb8
    );
    assert_eq!(&out[8..24], &src.s6_addr);
    assert_eq!(&out[24..40], &dst.s6_addr);
    // The inner header is the packet's, but for the scope words.
    assert_eq!(&out[40..48], &inner[..8]);
    assert_eq!(&out[48..50], &[0xfe, 0x80]);
    assert_eq!(&out[50..52], &[0, 0], "ip6_src scope");
    assert_eq!(&out[52..64], &inner[12..24]);
    assert_eq!(&out[64..66], &[0xfe, 0x80]);
    assert_eq!(&out[66..68], &[0, 0], "ip6_dst scope");
    assert_eq!(&out[68..], &inner[28..]);
    m_freem(mp);
}

#[test]
fn ipv6_travels_in_an_ipv4_tunnel() {
    let _g = setup();
    let t = tdb(su4([192, 168, 77, 1]), su4([192, 168, 77, 2]));
    let inner = ip6_packet(
        0xb8,
        a6([0x2001, 0xdb8, 0, 0, 0, 0, 0, 1]),
        a6([0x2001, 0xdb8, 0, 0, 0, 0, 0, 2]),
    );
    let mut mp = Some(test_packet(&inner));
    ipip_output(&mut mp, &t).expect("encapsulated");
    let out = bytes(mp.expect("the packet"));
    assert_eq!(out.len(), 20 + inner.len());
    assert_eq!(out[0], 0x45);
    assert_eq!(out[1], 0xb8, "the inner traffic class");
    assert_eq!(u16::from_be_bytes([out[2], out[3]]) as usize, out.len());
    assert_eq!(&out[6..8], &[0, 0], "ip_off: no DF or fragment bits");
    assert_eq!(out[9], IPPROTO_IPV6 as u8, "ip_p");
    assert_eq!(&out[12..16], &[192, 168, 77, 1]);
    assert_eq!(&out[16..20], &[192, 168, 77, 2]);
    assert_eq!(&out[20..], &inner[..]);
    m_freem(mp);
}

#[test]
fn bad_tunnel_endpoints_and_inner_packets_are_refused() {
    let _g = setup();
    let (src, dst) = (
        a6([0x2001, 0xdb8, 0, 0, 0, 0, 0, 1]),
        a6([0x2001, 0xdb8, 0, 0, 0, 0, 0, 2]),
    );
    let inner = ip4_packet(0);

    // An unspecified or mismatched endpoint (IPv6 outer).
    for t in [
        tdb(su6(src), su6(In6Addr::default())),
        tdb(su6(In6Addr::default()), su6(dst)),
        tdb(su4([10, 0, 0, 1]), su6(dst)),
    ] {
        let before = counter(IpipstatCounters::IpipsUnspec);
        let mut mp = Some(test_packet(&inner));
        assert_eq!(ipip_output(&mut mp, &t), Err(Errno::EINVAL));
        assert!(mp.is_none(), "the packet is freed");
        assert_eq!(counter(IpipstatCounters::IpipsUnspec), before + 1);
    }

    // Neither IPv4 nor IPv6 inside.
    let t = tdb(su6(src), su6(dst));
    let before = counter(IpipstatCounters::IpipsFamily);
    let mut mp = Some(test_packet(&[0x10; 40]));
    assert_eq!(ipip_output(&mut mp, &t), Err(Errno::EAFNOSUPPORT));
    assert!(mp.is_none());
    assert_eq!(counter(IpipstatCounters::IpipsFamily), before + 1);
}

fn ip6_total() -> u64 {
    IP6COUNTERS[Ip6statCounters::Ip6sTotal as usize].load(Ordering::Relaxed)
}

#[test]
fn ipv6_is_taken_out_of_an_ipv4_or_ipv6_tunnel() {
    let _g = setup();
    let ifp = test_ifnet(b"gif0");
    let (osrc, odst) = (
        a6([0x2001, 0xdb8, 0, 0, 0, 0, 0, 1]),
        a6([0x2001, 0xdb8, 0, 0, 0, 0, 0, 2]),
    );
    let inner = ip6_packet(
        0,
        a6([0x2001, 0xdb8, 5, 0, 0, 0, 0, 1]),
        a6([0x2001, 0xdb8, 5, 0, 0, 0, 0, 2]),
    );

    // IPv6 inside IPv4: the outer header goes, the inner packet goes to ip6_input_if.
    let mut outer = vec![0x45, 0];
    outer.extend_from_slice(&((20 + inner.len()) as u16).to_be_bytes());
    outer.extend_from_slice(&[
        0,
        0,
        0,
        0,
        64,
        IPPROTO_IPV6 as u8,
        0,
        0,
        192,
        168,
        77,
        2,
        192,
        168,
        77,
        1,
    ]);
    outer.extend_from_slice(&inner);
    let m = test_packet(&outer);
    m.m_flags().set(m.m_flags().get() | M_AUTH);
    let (bytes_before, total_before) = (counter(IpipstatCounters::IpipsIbytes), ip6_total());
    let mut mp = Some(m);
    let mut off = 20;
    // `allow` 2 skips the spoofing check, which needs a routing table.
    let r = ipip_input_if(
        &mut mp,
        &mut off,
        IPPROTO_IPV6,
        i32::from(AF_INET),
        2,
        ifp,
        None,
    );
    assert_eq!(r, IPPROTO_DONE);
    assert_eq!(off, 0);
    assert_eq!(
        ip6_total(),
        total_before + 1,
        "the inner packet reached ip6_input_if"
    );
    assert_eq!(
        counter(IpipstatCounters::IpipsIbytes),
        bytes_before + 8,
        "inner bytes after its header"
    );

    // IPv6 inside IPv6: the outer header is the IPv6 one (an IPv6 outer header's next
    // header is the inner protocol).
    let mut outer = ip6_packet(0, osrc, odst);
    outer[6] = IPPROTO_IPV6 as u8;
    outer[4..6].copy_from_slice(&(inner.len() as u16).to_be_bytes());
    outer.truncate(40);
    outer.extend_from_slice(&inner);
    let m = test_packet(&outer);
    m.m_flags().set(m.m_flags().get() | M_AUTH);
    let total_before = ip6_total();
    let mut mp = Some(m);
    let mut off = 40;
    let r = ipip_input_if(
        &mut mp,
        &mut off,
        IPPROTO_IPV6,
        i32::from(AF_INET6),
        2,
        ifp,
        None,
    );
    assert_eq!(r, IPPROTO_DONE);
    assert_eq!(ip6_total(), total_before + 1);

    // An inner header that is cut short is a header drop.
    let mut outer = ip6_packet(0, osrc, odst);
    outer[6] = IPPROTO_IPV6 as u8;
    outer.truncate(40);
    outer.extend_from_slice(&inner[..20]);
    let m = test_packet(&outer);
    let before = counter(IpipstatCounters::IpipsHdrops);
    let mut mp = Some(m);
    let mut off = 40;
    let r = ipip_input_if(
        &mut mp,
        &mut off,
        IPPROTO_IPV6,
        i32::from(AF_INET6),
        2,
        ifp,
        None,
    );
    assert_eq!(r, IPPROTO_DONE);
    assert!(mp.is_none());
    assert_eq!(counter(IpipstatCounters::IpipsHdrops), before + 1);

    // A protocol that is not IP-in-IP is counted as a family drop.
    let mut outer = ip6_packet(0, osrc, odst);
    outer.extend_from_slice(&[0; 40]);
    let m = test_packet(&outer);
    let before = counter(IpipstatCounters::IpipsFamily);
    let mut mp = Some(m);
    let mut off = 40;
    let r = ipip_input_if(&mut mp, &mut off, 99, i32::from(AF_INET6), 2, ifp, None);
    assert_eq!(r, IPPROTO_DONE);
    assert_eq!(counter(IpipstatCounters::IpipsFamily), before + 1);
}
