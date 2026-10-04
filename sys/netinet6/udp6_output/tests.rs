//! Host tests for UDP output over IPv6: the headers `udp6_output` writes and the checksum
//! `in6_proto_cksum_out` then fills in for them (`M_UDP_CSUM_OUT`), and the destination
//! checks that fail before anything is sent.

use std::boxed::Box;
use std::{assert_eq, assert_ne};

use super::*;
use crate::kern::uipc_mbuf::m_get;
use crate::kern::uipc_socket::soalloc;
use crate::kern::uipc_socket2::{solock, sounlock};
use crate::net::if_::tests::test_packet;
use crate::netinet::in_pcb::tests::{setup, teardown};
use crate::netinet::in_pcb::{Inpcbtable, in_pcballoc, in_pcbdetach, in_pcbinit};
use crate::netinet::ip_input::tests::bytes;
use crate::netinet6::in6::IN6ADDR_ANY;
use crate::netinet6::in6::tests::a6;
use crate::netinet6::in6_cksum::in6_cksum;
use crate::netinet6::in6_pcb::tests::pcb_of;
use crate::netinet6::in6_proto::INET6SW;
use crate::netinet6::ip6_output::in6_proto_cksum_out;
use crate::sys::endian::htonl;
use crate::sys::errno::Errno;
use crate::sys::mbuf::{M_WAIT, MT_SONAME};
use crate::sys::socket::SOCK_DGRAM;
use crate::sys::socketvar::{SS_NOFDREF, soref};

/// An `INP_IPV6` control block of a datagram socket bound to `fd00:77::1` port 5353.
fn pcb6() -> &'static Inpcb {
    let t: &'static Inpcbtable = Box::leak(Box::new(Inpcbtable::new()));
    in_pcbinit(t, 1);
    let so = soalloc(&INET6SW[1], M_WAIT).expect("socket");
    so.so_type.set(SOCK_DGRAM);
    in_pcballoc(so, t, M_WAIT).expect("in_pcballoc");
    let inp = pcb_of(so);
    inp.set_flags(crate::netinet::in_pcb::INP_IPV6);
    inp.inp_laddr6.set(a6("fd00:77::1"));
    inp.inp_lport.set(htons(5353));
    inp
}

/// Detaches the control block and lets the socket go.
fn release(inp: &'static Inpcb) {
    let so = inp.socket();
    let _ = soref(Some(so));
    solock(so);
    so.set_state(SS_NOFDREF);
    in_pcbdetach(inp);
    sounlock(so);
    crate::kern::uipc_socket::sorele(so);
}

/// An address mbuf holding `sin6`.
fn nam6(sin6: SockaddrIn6) -> &'static Mbuf {
    let m = m_get(M_DONTWAIT, MT_SONAME).expect("mbuf");
    m.m_len().set(size_of::<SockaddrIn6>() as u32);
    // SAFETY: a fresh mbuf of `MLEN` bytes.
    unsafe { mtod::<SockaddrIn6>(m).write_unaligned(sin6) };
    m
}

#[test]
fn the_headers_and_the_checksum_of_a_datagram() {
    let (_g, _t, _p) = setup();
    let inp = pcb6();
    // A traffic class and a flow label; only the low 28 bits go to the header.
    inp.set_inp_flowinfo(htonl(0xfe_0a_bc_de));

    let payload = b"hello";
    let plen = (size_of::<Udphdr>() + payload.len()) as u32;
    let m = m_prepend(test_packet(payload), 48, M_DONTWAIT).expect("prepend");
    udp6_output_hdr(
        m,
        inp,
        &a6("fd00:77::1"),
        &a6("fd00:77::2"),
        htons(53),
        plen,
    );
    // ip6_output fills in the payload length.
    let mut ip6 = mtod_ip6(m);
    ip6.ip6_plen = htons(plen as u16);
    mtod_ip6_store(m, &ip6);

    let b = bytes(m);
    assert_eq!(b.len(), 40 + 8 + 5);
    assert_eq!(&b[..4], &[0x6e, 0x0a, 0xbc, 0xde], "version 6, flow info");
    assert_eq!(b[6], IPPROTO_UDP as u8);
    assert_eq!(b[7], 64, "the default hop limit");
    assert_eq!(&b[8..24], &a6("fd00:77::1").s6_addr);
    assert_eq!(&b[24..40], &a6("fd00:77::2").s6_addr);
    assert_eq!(&b[40..46], &[0x14, 0xe9, 0, 53, 0, 13], "ports and length");
    assert_eq!(&b[46..48], &[0, 0], "checksum left to in6_proto_cksum_out");
    assert_eq!(&b[48..], payload);

    // The checksum in software (no interface offloads it).
    let ph = m.m_pkthdr();
    ph.csum_flags.set(ph.csum_flags.get() | M_UDP_CSUM_OUT);
    in6_proto_cksum_out(m, None);
    let b = bytes(m);
    assert_ne!(&b[46..48], &[0, 0]);
    assert_eq!(in6_cksum(m, IPPROTO_UDP as u8, 40, plen), 0);
    m_freem(m);

    // A datagram longer than 64k has a zero UDP length (a jumbogram's).
    let m = m_prepend(test_packet(&[0]), 48, M_DONTWAIT).expect("prepend");
    udp6_output_hdr(
        m,
        inp,
        &a6("fd00:77::1"),
        &a6("fd00:77::2"),
        htons(53),
        70_000,
    );
    assert_eq!(&bytes(m)[44..46], &[0, 0]);
    m_freem(m);

    release(inp);
    teardown();
}

#[test]
fn destinations_that_cannot_be_sent_to() {
    let (_g, _t, _p) = setup();
    let inp = pcb6();
    let send = |addr: Option<SockaddrIn6>| {
        let nam = addr.map(nam6);
        let r = udp6_output(inp, test_packet(&[1, 2, 3]), nam, None);
        m_freem(nam);
        r
    };

    // Unconnected without an address; a zero port; an IPv4-mapped address.
    assert_eq!(send(None), Err(Errno::ENOTCONN));
    assert_eq!(
        send(Some(SockaddrIn6::with_addr(a6("fd00:77::2")))),
        Err(Errno::EADDRNOTAVAIL)
    );
    let mapped = SockaddrIn6 {
        sin6_port: htons(53),
        ..SockaddrIn6::with_addr(a6("::ffff:a00:202"))
    };
    assert_eq!(send(Some(mapped)), Err(Errno::EADDRNOTAVAIL));
    // Not a sockaddr_in6.
    let mut short = SockaddrIn6::with_addr(IN6ADDR_ANY);
    short.sin6_family = 2;
    assert_eq!(send(Some(short)), Err(Errno::EAFNOSUPPORT));

    // Connected: no other destination.
    inp.inp_faddr6.set(a6("fd00:77::2"));
    let other = SockaddrIn6 {
        sin6_port: htons(53),
        ..SockaddrIn6::with_addr(a6("fd00:77::3"))
    };
    assert_eq!(send(Some(other)), Err(Errno::EISCONN));

    inp.inp_faddr6.set(IN6ADDR_ANY);
    release(inp);
    teardown();
}
