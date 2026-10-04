//! Host tests for IPv6 reassembly: fragments in order and out of order, overlapping
//! fragments, the atomic fragment of RFC 6946, the timeout and overflow of
//! `frag6_slowtimo`, and `frag6_deletefraghdr`.

use std::sync::MutexGuard;
use std::vec;
use std::vec::Vec;

use super::*;
use crate::kern::uipc_mbuf::m_copydata;
use crate::net::if_::tests::test_packet;
use crate::netinet::in_::{IPPROTO_FRAGMENT, IPPROTO_UDP};
use crate::netinet::ip_input::tests::setup;
use crate::netinet::ip6::IPV6_VERSION;
use crate::netinet6::in6::In6Addr;
use crate::netinet6::ip6_input::IP6COUNTERS;
use crate::sys::socket::AF_INET6;

/// The test setup plus an empty reassembly queue.
fn setup6() -> (MutexGuard<'static, ()>, MutexGuard<'static, ()>) {
    let g = setup();
    frag6_init();
    FRAG6_NFRAGS.store(0, Ordering::Relaxed);
    FRAG6_NFRAGPACKETS.store(0, Ordering::Relaxed);
    g
}

fn stat(c: Ip6statCounters) -> u64 {
    IP6COUNTERS[c as usize].load(Ordering::Relaxed)
}

/// A fragment of datagram `ident` from fd00::1 to fd00::2: `data` at byte offset `off`,
/// with the more-fragments bit `more`.
fn frag(ident: u32, off: u16, more: bool, data: &[u8]) -> &'static Mbuf {
    let mut ip6 = Ip6Hdr::zeroed();
    ip6.set_ip6_vfc(IPV6_VERSION);
    ip6.ip6_plen = htons((size_of::<Ip6Frag>() + data.len()) as u16);
    ip6.ip6_nxt = IPPROTO_FRAGMENT as u8;
    ip6.ip6_hlim = 64;
    let mut a = [0u8; 16];
    a[0] = 0xfd;
    a[15] = 1;
    ip6.ip6_src = In6Addr::new(a);
    a[15] = 2;
    ip6.ip6_dst = In6Addr::new(a);
    // SAFETY: `Ip6Hdr` is 40 bytes of integers without padding.
    let hdr: [u8; 40] = unsafe { core::mem::transmute(ip6) };
    let mut b: Vec<u8> = hdr.to_vec();
    b.push(IPPROTO_UDP as u8);
    b.push(0);
    b.extend_from_slice(&(off | u16::from(more)).to_be_bytes());
    b.extend_from_slice(&ident.to_be_bytes());
    b.extend_from_slice(data);
    test_packet(&b)
}

/// `frag6_input` on `m` at the fragment header.
fn input(m: &'static Mbuf) -> (i32, i32, Option<&'static Mbuf>) {
    let mut mp = Some(m);
    let mut off = 40;
    let nxt = frag6_input(
        &mut mp,
        &mut off,
        IPPROTO_FRAGMENT,
        i32::from(AF_INET6),
        None,
    );
    (nxt, off, mp)
}

fn data(n: u8, len: usize) -> Vec<u8> {
    (0..len)
        .map(|i| n.wrapping_mul(16).wrapping_add(i as u8))
        .collect()
}

/// Checks a reassembled datagram: UDP after the header, `payload` after it.
fn check_reassembled(r: (i32, i32, Option<&'static Mbuf>), payload: &[u8]) {
    let (nxt, off, mp) = r;
    assert_eq!(nxt, IPPROTO_UDP);
    assert_eq!(off, 40);
    let m = mp.expect("the reassembled datagram");
    assert_eq!(m.m_pkthdr().len.get() as usize, 40 + payload.len());
    let ip6 = mtod_ip6(m);
    assert_eq!(
        i32::from(ip6.ip6_nxt),
        IPPROTO_UDP,
        "the next header was restored"
    );
    assert_eq!(usize::from(ntohs(ip6.ip6_plen)), payload.len());
    let mut got = vec![0u8; payload.len()];
    m_copydata(m, 40, &mut got);
    assert_eq!(got, payload);
    m_freem(m);
}

#[test]
fn fragments_in_order_are_reassembled() {
    let _g = setup6();
    let (a, b, c) = (data(1, 16), data(2, 16), data(3, 8));
    let reassembled = stat(Ip6statCounters::Ip6sReassembled);

    let r = input(frag(7, 0, true, &a));
    assert_eq!((r.0, r.2.is_none()), (IPPROTO_DONE, true), "queued");
    let r = input(frag(7, 16, true, &b));
    assert_eq!((r.0, r.2.is_none()), (IPPROTO_DONE, true), "queued");
    assert_eq!(FRAG6_NFRAGS.load(Ordering::Relaxed), 2);
    let r = input(frag(7, 32, false, &c));
    check_reassembled(r, &[a, b, c].concat());
    assert_eq!(stat(Ip6statCounters::Ip6sReassembled), reassembled + 1);
    assert_eq!(FRAG6_NFRAGS.load(Ordering::Relaxed), 0);
    assert_eq!(FRAG6_NFRAGPACKETS.load(Ordering::Relaxed), 0);
}

#[test]
fn fragments_out_of_order_are_reassembled() {
    let _g = setup6();
    let (a, b, c) = (data(4, 24), data(5, 8), data(6, 3));

    assert_eq!(input(frag(9, 32, false, &c)).0, IPPROTO_DONE);
    assert_eq!(input(frag(9, 24, true, &b)).0, IPPROTO_DONE);
    // Another datagram in between does not mix in.
    assert_eq!(input(frag(10, 8, true, &b)).0, IPPROTO_DONE);
    assert_eq!(FRAG6_NFRAGPACKETS.load(Ordering::Relaxed), 2);
    let r = input(frag(9, 0, true, &a));
    check_reassembled(r, &[a, b, c].concat());
    assert_eq!(FRAG6_NFRAGPACKETS.load(Ordering::Relaxed), 1);
}

#[test]
fn an_overlap_discards_the_datagram() {
    let _g = setup6();
    let dropped = stat(Ip6statCounters::Ip6sFragdropped);

    assert_eq!(input(frag(11, 0, true, &data(1, 16))).0, IPPROTO_DONE);
    let (nxt, _, mp) = input(frag(11, 8, true, &data(2, 16)));
    assert_eq!(nxt, IPPROTO_DONE);
    assert!(mp.is_none());
    assert_eq!(stat(Ip6statCounters::Ip6sFragdropped), dropped + 2);
    assert_eq!(FRAG6_NFRAGPACKETS.load(Ordering::Relaxed), 0);
    assert_eq!(FRAG6_NFRAGS.load(Ordering::Relaxed), 0);
}

#[test]
fn an_atomic_fragment_is_processed_alone() {
    let _g = setup6();
    let reassembled = stat(Ip6statCounters::Ip6sReassembled);

    let (nxt, off, mp) = input(frag(12, 0, false, &data(1, 8)));
    assert_eq!(nxt, IPPROTO_UDP);
    assert_eq!(off, 48, "past the fragment header");
    assert_eq!(stat(Ip6statCounters::Ip6sReassembled), reassembled + 1);
    assert_eq!(FRAG6_NFRAGPACKETS.load(Ordering::Relaxed), 0);
    m_freem(mp.expect("kept"));
}

#[test]
fn the_timer_expires_and_trims_the_queues() {
    let _g = setup6();
    let timeouts = stat(Ip6statCounters::Ip6sFragtimeout);

    // A later fragment alone (no ICMPv6 error for it when it expires).
    assert_eq!(input(frag(13, 16, true, &data(1, 8))).0, IPPROTO_DONE);
    for _ in 1..IPV6_FRAGTTL {
        frag6_slowtimo();
    }
    assert_eq!(FRAG6_NFRAGPACKETS.load(Ordering::Relaxed), 1);
    frag6_slowtimo();
    assert_eq!(FRAG6_NFRAGPACKETS.load(Ordering::Relaxed), 0);
    assert_eq!(FRAG6_NFRAGS.load(Ordering::Relaxed), 0);
    assert_eq!(stat(Ip6statCounters::Ip6sFragtimeout), timeouts + 1);

    // Two datagrams with the limit lowered to one: the oldest goes.
    let overflow = stat(Ip6statCounters::Ip6sFragoverflow);
    assert_eq!(input(frag(14, 16, true, &data(1, 8))).0, IPPROTO_DONE);
    assert_eq!(input(frag(15, 16, true, &data(1, 8))).0, IPPROTO_DONE);
    IP6_MAXFRAGPACKETS.store(1, Ordering::Relaxed);
    frag6_slowtimo();
    IP6_MAXFRAGPACKETS.store(200, Ordering::Relaxed);
    assert_eq!(stat(Ip6statCounters::Ip6sFragoverflow), overflow + 1);
    assert_eq!(FRAG6_NFRAGPACKETS.load(Ordering::Relaxed), 1);
    let left = FRAG6_QUEUE.0.first().expect("one queue left");
    assert_eq!(left.ip6q_ident.get(), 15u32.to_be());
}

#[test]
fn the_fragment_header_is_deleted() {
    let _g = setup6();
    let payload = data(3, 8);
    let m = frag(16, 0, false, &payload);
    frag6_deletefraghdr(m, 40).expect("in the first mbuf");
    assert_eq!(m.m_len().get(), 48);
    let mut got = vec![0u8; 8];
    m_copydata(m, 40, &mut got);
    assert_eq!(got, payload);
    assert_eq!(
        i32::from(mtod_ip6(m).ip6_nxt),
        IPPROTO_FRAGMENT,
        "the header moved whole"
    );
    m_freem(m);
}
