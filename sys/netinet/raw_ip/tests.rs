//! Host tests for raw IP sockets: what ping(8) does with a `SOCK_RAW`, `IPPROTO_ICMP` socket
//! over the test Ethernet interface (an echo request out through `rip_send` and `ip_output`,
//! held by ARP until the gateway answers, the echo reply in through `icmp_input` to
//! `rip_input` and the socket's receive buffer with the sender's address), `IP_HDRINCL`, the
//! filters of `rip_input`, and the privilege `rip_attach` requires.

use std::{assert, assert_eq, vec};

use super::*;
use crate::kern::kern_prot::crget;
use crate::kern::uipc_mbuf::{m_copydata, m_gethdr};
use crate::kern::uipc_socket::{soclose, socreate};
use crate::kern::uipc_socket2::{solock, sounlock};
use crate::net::ethertypes::{ETHERTYPE_ARP, ETHERTYPE_IP};
use crate::net::if_arp::{ARPHRD_ETHER, ARPOP_REPLY, Arphdr};
use crate::net::if_ethersubr::ether_input;
use crate::net::ifq::ifq_dequeue;
use crate::netinet::if_ether::{EtherArp, EtherHeader, arpintr};
use crate::netinet::in_cksum::in_cksum;
use crate::netinet::in_pcb::tests::{nam, setup, teardown};
use crate::netinet::in4_cksum::in4_cksum;
use crate::netinet::ip_icmp::{ICMP_ECHO, ICMP_ECHOREPLY, IcmpPkt};
use crate::netinet::ip_input::ipintr;
use crate::netinet::ip_input::tests::{
    ADDR, GATEWAY, OURS, PEER, bytes, configure, frame, sin, test_ether,
};
use crate::sys::mbuf::{MT_DATA, MT_SONAME, MT_SOOPTS};
use crate::sys::protosw::PRCO_GETOPT;
use crate::sys::socket::SOCK_RAW;

/// A raw ICMP socket, as ping(8) opens it.
fn icmp_socket() -> &'static Socket {
    socreate(i32::from(AF_INET), SOCK_RAW, IPPROTO_ICMP).expect("socket")
}

/// An option mbuf holding the `int` `v`.
fn intopt(v: i32) -> &'static Mbuf {
    let m = crate::kern::uipc_mbuf::m_get(M_DONTWAIT, MT_SOOPTS).expect("mbuf");
    m.m_len().set(4);
    // SAFETY: a fresh mbuf of `MLEN` bytes.
    unsafe { mtod::<i32>(m).write_unaligned(v) };
    m
}

#[test]
fn a_raw_icmp_socket_pings_the_gateway() {
    let (_g, _t, _p) = setup();
    rip_init();
    let ifp = test_ether();
    configure(ifp, ADDR, [255, 255, 255, 0]);
    while let Some(m) = ifq_dequeue(&ifp.if_snd) {
        m_freem(m);
    }

    let so = icmp_socket();
    let inp = sotoinpcb(so).expect("attached");
    assert_eq!(inp.inp_ip.get().ip_p, IPPROTO_ICMP as u8);
    assert_eq!(so.so_rcv.sb_hiwat.get(), RIP_RECVSPACE);

    // IP_HDRINCL is off, as ping leaves it.
    let m = intopt(7);
    rip_ctloutput(PRCO_GETOPT, so, IPPROTO_IP, IP_HDRINCL, Some(m)).expect("getsockopt");
    // SAFETY: the option was written as an `int`.
    assert_eq!(unsafe { mtod::<i32>(m).read_unaligned() }, 0);
    m_freem(m);

    // sendto(s, echo, 16, 0, 10.0.2.2): the payload is the ICMP message.
    let len = 8 + 8;
    let m = m_gethdr(M_DONTWAIT, MT_DATA).expect("mbuf");
    m.m_data().set(m.m_data().get().wrapping_add(64));
    m.m_len().set(len as u32);
    m.m_pkthdr().len.set(len as i32);
    let icp = IcmpPkt::of(m, 0);
    icp.set_icmp_type(ICMP_ECHO);
    icp.set_icmp_code(0);
    icp.set_icmp_cksum(0);
    icp.set_icmp_id(htons(7));
    icp.set_icmp_seq(htons(1));
    let sum = crate::netinet::in_cksum::in_cksum(m, len as i32);
    icp.set_icmp_cksum(sum);
    solock(so);
    rip_send(so, Some(m), Some(nam(sin(GATEWAY))), None).expect("held by ARP");
    sounlock(so);
    let req = ifq_dequeue(&ifp.if_snd).expect("ARP request");
    let b = bytes(req);
    m_freem(req);
    assert_eq!(&b[12..14], &ETHERTYPE_ARP.to_be_bytes());

    // The gateway answers ARP; the echo request leaves with the header rip_output made.
    let reply = EtherArp {
        ea_hdr: Arphdr {
            ar_hrd: htons(ARPHRD_ETHER),
            ar_pro: htons(ETHERTYPE_IP),
            ar_hln: 6,
            ar_pln: 4,
            ar_op: htons(ARPOP_REPLY),
        },
        arp_sha: PEER,
        arp_spa: GATEWAY,
        arp_tha: OURS,
        arp_tpa: ADDR,
    };
    // SAFETY: an `ether_arp` is plain bytes.
    let arp = unsafe {
        slice::from_raw_parts(
            core::ptr::from_ref(&reply).cast::<u8>(),
            size_of::<EtherArp>(),
        )
    };
    ether_input(ifp, frame(ifp, OURS, ETHERTYPE_ARP, arp), None);
    arpintr();
    let echo = ifq_dequeue(&ifp.if_snd).expect("the echo request");
    let b = bytes(echo);
    m_freem(echo);
    let ipb = &b[size_of::<EtherHeader>()..];
    assert_eq!(ipb[0], 0x45);
    assert_eq!(ipb[8], MAXTTL, "no TTL set: MAXTTL");
    assert_eq!(ipb[9], IPPROTO_ICMP as u8);
    assert_eq!(&ipb[12..16], &ADDR, "in_pcbselsrc chose our address");
    assert_eq!(&ipb[16..20], &GATEWAY);
    assert_eq!(ipb[20], ICMP_ECHO);
    assert_eq!(usize::from(u16::from_be_bytes([ipb[2], ipb[3]])), 20 + len);

    // The echo reply goes to the socket, IP header included, from 10.0.2.2.
    let mut r = ipb.to_vec();
    r[12..16].copy_from_slice(&GATEWAY);
    r[16..20].copy_from_slice(&ADDR);
    r[10] = 0;
    r[11] = 0;
    r[20] = ICMP_ECHOREPLY;
    r[22] = 0;
    r[23] = 0;
    let m3 = crate::net::if_::tests::test_packet(&r);
    let mut ip = mtod_ip(m3);
    ip.ip_sum = in_cksum(m3, 20);
    mtod_ip_store(m3, &ip);
    let sum = in4_cksum(m3, 0, 20, len as i32);
    IcmpPkt::of(m3, 20).set_icmp_cksum(sum);
    let r = bytes(m3);
    m_freem(m3);
    ether_input(ifp, frame(ifp, OURS, ETHERTYPE_IP, &r), None);
    ipintr();

    let rec = so.so_rcv.sb_mb.get().expect("a record");
    assert_eq!(i32::from(rec.m_type().get()), MT_SONAME);
    // SAFETY: an `MT_SONAME` mbuf of a raw inet socket holds a `sockaddr_in`.
    let from = unsafe { mtod::<SockaddrIn>(rec).read_unaligned() };
    assert_eq!(from.sin_addr, sin(GATEWAY).sin_addr);
    let data = rec.m_next().get().expect("the datagram");
    let mut got = vec![0u8; r.len()];
    m_copydata(data, 0, &mut got);
    assert_eq!(got, r, "the whole datagram, IP header included");

    soclose(so, 0).expect("close");
    teardown();
}

#[test]
fn rip_input_filters_and_rip_attach_needs_privilege() {
    let (_g, _t, p) = setup();
    rip_init();

    // A socket of protocol 17 bound to 10.0.2.15 does not see ICMP, nor UDP to another
    // address; a wildcard one sees both.
    let udp = socreate(i32::from(AF_INET), SOCK_RAW, 17).expect("raw udp socket");
    sotoinpcb(udp)
        .expect("attached")
        .inp_laddr
        .set(sin(ADDR).sin_addr);
    let any = socreate(i32::from(AF_INET), SOCK_RAW, 0).expect("wildcard socket");
    let packet = |proto: u8, dst: [u8; 4]| {
        let mut h = [
            0x45, 0, 0, 28, 0, 0, 0, 0, 64, proto, 0, 0, 10, 0, 2, 2, 0, 0, 0, 0,
        ];
        h[16..20].copy_from_slice(&dst);
        let mut v = h.to_vec();
        v.extend_from_slice(&[0; 8]);
        crate::net::if_::tests::test_packet(&v)
    };
    let deliver = |m: &'static Mbuf| {
        let mut mp = Some(m);
        let mut off = 20;
        rip_input(&mut mp, &mut off, 0, i32::from(AF_INET), None);
    };
    deliver(packet(1, ADDR));
    assert_eq!(udp.so_rcv.sb_cc.get(), 0);
    assert!(any.so_rcv.sb_cc.get() > 0);
    deliver(packet(17, [10, 0, 2, 99]));
    assert_eq!(udp.so_rcv.sb_cc.get(), 0);
    deliver(packet(17, ADDR));
    assert!(udp.so_rcv.sb_cc.get() > 0);
    soclose(udp, 0).expect("close");
    soclose(any, 0).expect("close");

    // An unprivileged process may not open a raw socket.
    let cr = crget();
    cr.cr_uid.set(1000);
    p.p_ucred.set(cr);
    assert_eq!(
        socreate(i32::from(AF_INET), SOCK_RAW, IPPROTO_ICMP).err(),
        Some(Errno::EACCES)
    );
    teardown();
}
