//! Host tests for the IPv6 parts of `ipsec_input.c`: `ipsec_protoff` finds the next-header
//! field that precedes the IPsec header in an IPv6 extension header chain, and
//! `ipsec_local_check` leaves IPv6 option headers to the local delivery loop.

use std::sync::MutexGuard;
use std::vec;

use super::*;
use crate::kern::uipc_mbuf::m_freem;
use crate::net::if_::tests::test_packet;
use crate::netinet::in_::IPPROTO_HOPOPTS;

fn setup() -> MutexGuard<'static, ()> {
    crate::kern::uipc_mbuf::tests::setup()
}

/// An IPv6 header, then `exts` (extension headers as bytes), then 16 bytes of payload.
fn packet(nxt: u8, exts: &[u8]) -> &'static Mbuf {
    let mut p = vec![0x60, 0, 0, 0];
    p.extend_from_slice(&((exts.len() + 16) as u16).to_be_bytes());
    p.extend_from_slice(&[nxt, 64]);
    p.extend_from_slice(&[0x20, 0x01, 0x0d, 0xb8, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 1]);
    p.extend_from_slice(&[0x20, 0x01, 0x0d, 0xb8, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 2]);
    p.extend_from_slice(exts);
    p.extend_from_slice(&[0; 16]);
    test_packet(&p)
}

#[test]
fn the_next_header_field_of_an_ipv6_packet_is_found() {
    let _g = setup();
    let af = i32::from(AF_INET6);

    // IPv4: ip_p.
    assert_eq!(ipsec_protoff(packet(0, &[]), 20, i32::from(AF_INET)), 9);

    // The IPsec header follows the IPv6 header: ip6_nxt.
    let m = packet(IPPROTO_ESP as u8, &[]);
    assert_eq!(ipsec_protoff(m, 40, af), 6);

    // Less than an IPv6 header before it cannot be a packet.
    assert_eq!(ipsec_protoff(m, 39, af), -1);

    // After a hop-by-hop options header (8 bytes): its next header field, at 40.
    let hbh = [IPPROTO_ESP as u8, 0, 1, 4, 0, 0, 0, 0];
    let m = packet(IPPROTO_HOPOPTS as u8, &hbh);
    assert_eq!(ipsec_protoff(m, 48, af), 40);

    // After a hop-by-hop (8 bytes) and a destination options header (16 bytes).
    let mut chain = vec![IPPROTO_DSTOPTS as u8, 0, 1, 4, 0, 0, 0, 0];
    chain.extend_from_slice(&[
        IPPROTO_ESP as u8,
        1,
        1,
        12,
        0,
        0,
        0,
        0,
        0,
        0,
        0,
        0,
        0,
        0,
        0,
        0,
    ]);
    let m = packet(IPPROTO_HOPOPTS as u8, &chain);
    assert_eq!(
        ipsec_protoff(m, 64, af),
        48,
        "the destination options' next header"
    );

    // An offset that falls inside a header is a malformed chain.
    let m = packet(IPPROTO_HOPOPTS as u8, &hbh);
    assert_eq!(ipsec_protoff(m, 44, af), -1);
    assert_eq!(ipsec_protoff(m, 52, af), -1);
}

#[test]
fn ipv6_option_headers_are_not_policy_checked_on_their_own() {
    let _g = setup();
    let m = packet(
        IPPROTO_HOPOPTS as u8,
        &[IPPROTO_UDP as u8, 0, 1, 4, 0, 0, 0, 0],
    );
    let af = i32::from(AF_INET6);
    // Destination options, routing and fragment headers are left to the delivery loop, as
    // are tunnelled and IPsec packets and the transport protocols that check their own.
    for proto in [
        IPPROTO_DSTOPTS,
        IPPROTO_ROUTING,
        IPPROTO_FRAGMENT,
        IPPROTO_ESP,
        IPPROTO_AH,
        IPPROTO_IPCOMP,
        IPPROTO_IPV4,
        IPPROTO_IPV6,
        IPPROTO_TCP,
        IPPROTO_UDP,
    ] {
        assert_eq!(ipsec_local_check(m, 40, proto, af), Ok(()), "proto {proto}");
    }
    m_freem(Some(m));
}
