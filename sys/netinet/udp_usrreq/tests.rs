//! Host tests for UDP over the test Ethernet interface: a datagram in to a bound socket
//! (with the sender's address), a port unreachable for a closed port, a bad checksum, a
//! datagram out from an unbound socket (a port picked, the headers and the checksum right
//! once ARP has the gateway), connect/disconnect, an IP option and the IPsec levels through `ip_ctloutput`, and an
//! ICMP error passed on by `udp_ctlinput`.

use std::{assert, assert_eq, vec, vec::Vec};

use super::*;
use crate::kern::uipc_mbuf::{m_copydata, m_gethdr};
use crate::kern::uipc_socket::{soclose, socreate};
use crate::kern::uipc_socket2::{solock, sounlock};
use crate::net::ethertypes::{ETHERTYPE_ARP, ETHERTYPE_IP};
use crate::net::if_::tests::test_packet;
use crate::net::if_arp::{ARPHRD_ETHER, ARPOP_REPLY, Arphdr};
use crate::net::if_ethersubr::ether_input;
use crate::net::if_var::Ifnet;
use crate::net::ifq::ifq_dequeue;
use crate::netinet::if_ether::{EtherArp, EtherHeader, arpintr};
use crate::netinet::in_::IP_TTL;
use crate::netinet::in_cksum::in_cksum;
use crate::netinet::in_pcb::tests::{nam, setup, teardown};
use crate::netinet::ip_input::ipintr;
use crate::netinet::ip_input::tests::{
    ADDR, GATEWAY, OURS, PEER, bytes, configure, frame, sent, sin, test_ether,
};
use crate::netinet::ip_output::ip_ctloutput;
use crate::sys::mbuf::{MT_DATA, MT_SONAME, MT_SOOPTS};
use crate::sys::protosw::{PRC_UNREACH_PORT, PRCO_GETOPT, PRCO_SETOPT};
use crate::sys::socket::SOCK_DGRAM;

/// The interface with our address, its queue drained; UDP and raw IP set up.
fn net() -> &'static Ifnet {
    udp_init();
    crate::netinet::raw_ip::rip_init();
    let ifp = test_ether();
    configure(ifp, ADDR, [255, 255, 255, 0]);
    drain(ifp);
    ifp
}

/// Drops whatever the interface was asked to send.
fn drain(ifp: &Ifnet) {
    while let Some(m) = ifq_dequeue(&ifp.if_snd) {
        m_freem(m);
    }
}

/// A UDP socket.
fn udp_socket() -> &'static Socket {
    socreate(i32::from(AF_INET), SOCK_DGRAM, 0).expect("socket")
}

/// `addr:port`.
fn sinp(addr: [u8; 4], port: u16) -> SockaddrIn {
    let mut s = sin(addr);
    s.sin_port = port.to_be();
    s
}

/// The IPv4 datagram `src:sport -> dst:dport` carrying `payload`, both checksums right
/// unless `bad_sum`.
fn udp_datagram(sport: u16, dst: [u8; 4], dport: u16, payload: &[u8], bad_sum: bool) -> Vec<u8> {
    let len = 20 + 8 + payload.len();
    let mut p = vec![0x45, 0];
    p.extend_from_slice(&(len as u16).to_be_bytes());
    p.extend_from_slice(&[0, 0, 0, 0, 64, IPPROTO_UDP as u8, 0, 0]);
    p.extend_from_slice(&GATEWAY);
    p.extend_from_slice(&dst);
    p.extend_from_slice(&sport.to_be_bytes());
    p.extend_from_slice(&dport.to_be_bytes());
    p.extend_from_slice(&((8 + payload.len()) as u16).to_be_bytes());
    p.extend_from_slice(&[0, 0]);
    p.extend_from_slice(payload);
    let m = test_packet(&p);
    let mut ip = mtod_ip(m);
    ip.ip_sum = in_cksum(m, 20);
    mtod_ip_store(m, &ip);
    let sum = in4_cksum(m, IPPROTO_UDP as u8, 20, (8 + payload.len()) as i32) ^ u16::from(bad_sum);
    let b = bytes(m);
    m_freem(m);
    let mut b = b;
    b[26..28].copy_from_slice(&sum.to_ne_bytes());
    b
}

/// Hands `datagram` to the interface as the gateway's and runs the IP input queue.
fn receive(ifp: &'static Ifnet, datagram: &[u8]) {
    ether_input(ifp, frame(ifp, OURS, ETHERTYPE_IP, datagram), None);
    ipintr();
}

/// The gateway answers our ARP request.
fn answer_arp(ifp: &'static Ifnet) {
    let reply = EtherArp {
        ea_hdr: Arphdr {
            ar_hrd: htons_(ARPHRD_ETHER),
            ar_pro: htons_(ETHERTYPE_IP),
            ar_hln: 6,
            ar_pln: 4,
            ar_op: htons_(ARPOP_REPLY),
        },
        arp_sha: PEER,
        arp_spa: GATEWAY,
        arp_tha: OURS,
        arp_tpa: ADDR,
    };
    // SAFETY: an `ether_arp` is plain bytes.
    let arp = unsafe {
        core::slice::from_raw_parts(ptr::from_ref(&reply).cast::<u8>(), size_of::<EtherArp>())
    };
    ether_input(ifp, frame(ifp, OURS, ETHERTYPE_ARP, arp), None);
    arpintr();
}

/// `htons`.
fn htons_(v: u16) -> u16 {
    v.to_be()
}

/// The counter of `c`.
fn udpstat(c: UdpstatCounters) -> u64 {
    UDPCOUNTERS[c as usize].load(Ordering::Relaxed)
}

#[test]
fn datagrams_in_to_a_bound_socket_and_errors_for_the_others() {
    let (_g, _t, p) = setup();
    let ifp = net();

    let so = udp_socket();
    solock(so);
    udp_bind(so, nam(sinp(ADDR, 5353)), p).expect("bind");
    sounlock(so);

    receive(ifp, &udp_datagram(1234, ADDR, 5353, b"hello", false));
    let rec = so.so_rcv.sb_mb.get().expect("a record");
    assert_eq!(i32::from(rec.m_type().get()), MT_SONAME);
    // SAFETY: the record's address is a `sockaddr_in`.
    let from = unsafe { mtod::<SockaddrIn>(rec).read_unaligned() };
    assert_eq!(from, sinp(GATEWAY, 1234));
    let data = rec.m_next().get().expect("data");
    let mut got = vec![0u8; 5];
    m_copydata(data, 0, &mut got);
    assert_eq!(got, b"hello", "the payload alone");
    assert_eq!(so.so_rcv.sb_datacc.get(), 5);

    // A bad checksum is dropped.
    let badsum = udpstat(UdpstatCounters::UdpsBadsum);
    receive(ifp, &udp_datagram(1234, ADDR, 5353, b"hello", true));
    assert_eq!(udpstat(UdpstatCounters::UdpsBadsum), badsum + 1);
    assert_eq!(so.so_rcv.sb_datacc.get(), 5);

    // A closed port gets a port unreachable (queued for ip_send) quoting the datagram.
    let _ = sent(|_, _| {});
    let noport = udpstat(UdpstatCounters::UdpsNoport);
    receive(ifp, &udp_datagram(1234, ADDR, 9, b"x", false));
    assert_eq!(udpstat(UdpstatCounters::UdpsNoport), noport + 1);
    let n = sent(|b, _| {
        assert_eq!(b[9], 1, "ICMP");
        assert_eq!(&b[16..20], &GATEWAY);
        assert_eq!((b[20], b[21]), (ICMP_UNREACH, ICMP_UNREACH_PORT));
        assert_eq!(
            u16::from_be_bytes([b[28 + 22], b[28 + 23]]),
            9,
            "the quoted port"
        );
    });
    assert_eq!(n, 1);

    soclose(so, 0).expect("close");
    teardown();
}

#[test]
fn datagrams_out_options_and_errors() {
    let (_g, _t, _p) = setup();
    let ifp = net();

    let so = udp_socket();
    let inp = sotoinpcb(so).expect("attached");
    assert_eq!(
        inp.inp_ip.get().ip_ttl,
        IP_DEFTTL.load(Ordering::Relaxed) as u8
    );

    // setsockopt(IP_TTL, 7), then getsockopt.
    let opt = crate::kern::uipc_mbuf::m_get(M_DONTWAIT, MT_SOOPTS).expect("mbuf");
    opt.m_len().set(4);
    // SAFETY: a fresh mbuf of `MLEN` bytes.
    unsafe { mtod::<i32>(opt).write_unaligned(7) };
    ip_ctloutput(PRCO_SETOPT, so, IPPROTO_IP, IP_TTL, Some(opt)).expect("IP_TTL");
    // SAFETY: as above.
    unsafe { mtod::<i32>(opt).write_unaligned(0) };
    ip_ctloutput(PRCO_GETOPT, so, IPPROTO_IP, IP_TTL, Some(opt)).expect("get IP_TTL");
    // SAFETY: written as an `int`.
    assert_eq!(unsafe { mtod::<i32>(opt).read_unaligned() }, 7);
    m_freem(opt);

    // sendto(10.0.2.2:53) from an unbound socket.
    let payload = b"query";
    let m = m_gethdr(M_DONTWAIT, MT_DATA).expect("mbuf");
    m.m_data().set(m.m_data().get().wrapping_add(64));
    m.m_len().set(payload.len() as u32);
    m.m_pkthdr().len.set(payload.len() as i32);
    // SAFETY: the mbuf holds `MHLEN - 64` bytes from its data pointer.
    unsafe { ptr::copy_nonoverlapping(payload.as_ptr(), mtod::<u8>(m), payload.len()) };
    solock(so);
    udp_send(so, Some(m), Some(nam(sinp(GATEWAY, 53))), None).expect("held by ARP");
    sounlock(so);
    let lport = u16::from_be(inp.inp_lport.get());
    assert!(lport >= 1024, "a port was picked: {lport}");
    drain(ifp);
    answer_arp(ifp);
    let out = ifq_dequeue(&ifp.if_snd).expect("the datagram");
    let b = bytes(out);
    m_freem(out);
    let ipb = &b[size_of::<EtherHeader>()..];
    assert_eq!(ipb[8], 7, "the socket's TTL");
    assert_eq!(ipb[9], IPPROTO_UDP as u8);
    assert_eq!(&ipb[12..16], &ADDR);
    assert_eq!(&ipb[16..20], &GATEWAY);
    assert_eq!(u16::from_be_bytes([ipb[20], ipb[21]]), lport);
    assert_eq!(u16::from_be_bytes([ipb[22], ipb[23]]), 53);
    assert_eq!(
        usize::from(u16::from_be_bytes([ipb[24], ipb[25]])),
        8 + payload.len()
    );
    assert_eq!(&ipb[28..], payload);
    let m2 = test_packet(ipb);
    assert_eq!(in_cksum(m2, 20), 0, "ip header checksum");
    assert_eq!(
        in4_cksum(m2, IPPROTO_UDP as u8, 20, (8 + payload.len()) as i32),
        0,
        "udp checksum"
    );
    m_freem(m2);

    // connect(10.0.2.2:53): the address is ours now; then an ICMP port unreachable for a
    // datagram it sent comes back.
    solock(so);
    udp_connect(so, nam(sinp(GATEWAY, 53))).expect("connect");
    sounlock(so);
    assert!(so.has_state(SS_ISCONNECTED));
    assert_eq!(inp.inp_laddr.get(), sin(ADDR).sin_addr);
    let mut returned = Vec::new();
    returned.extend_from_slice(&ipb[..20]);
    returned.extend_from_slice(&ipb[20..28]);
    let dst = sinp(GATEWAY, 0);
    // SAFETY: a local `sockaddr_in` and the returned IP and UDP headers.
    unsafe {
        udp_ctlinput(
            PRC_UNREACH_PORT,
            ptr::from_ref(&dst).cast(),
            0,
            returned.as_mut_ptr().cast(),
        )
    };
    assert_eq!(so.error(), Some(Errno::ECONNREFUSED));
    so.set_error(None);

    solock(so);
    udp_disconnect(so).expect("disconnect");
    assert_eq!(udp_disconnect(so), Err(Errno::ENOTCONN));
    sounlock(so);
    assert!(!so.has_state(SS_ISCONNECTED));

    soclose(so, 0).expect("close");
    teardown();
}

/// `setsockopt`/`getsockopt` of an `int` option through `ip_ctloutput`.
fn int_opt(so: &'static Socket, op: i32, name: i32, v: i32) -> Result<i32, Errno> {
    let opt = crate::kern::uipc_mbuf::m_get(M_DONTWAIT, MT_SOOPTS).expect("mbuf");
    opt.m_len().set(4);
    // SAFETY: a fresh mbuf of `MLEN` bytes.
    unsafe { mtod::<i32>(opt).write_unaligned(v) };
    let r = ip_ctloutput(op, so, IPPROTO_IP, name, Some(opt));
    // SAFETY: as above.
    let out = unsafe { mtod::<i32>(opt).read_unaligned() };
    m_freem(opt);
    r.map(|()| out)
}

#[test]
fn ipsec_levels_and_the_udpencap_port() {
    use crate::netinet::in_::{
        IP_AUTH_LEVEL, IP_ESP_TRANS_LEVEL, IPSEC_LEVEL_BYPASS, IPSEC_LEVEL_DEFAULT,
        IPSEC_LEVEL_REQUIRE,
    };

    let (_g, _t, _p) = setup();
    let _ifp = net();
    let so = udp_socket();

    // in_pcballoc's defaults.
    assert_eq!(
        int_opt(so, PRCO_GETOPT, IP_AUTH_LEVEL, 0),
        Ok(IPSEC_LEVEL_DEFAULT)
    );
    // Root may set any level, the bypass included.
    for level in [IPSEC_LEVEL_REQUIRE, IPSEC_LEVEL_BYPASS] {
        int_opt(so, PRCO_SETOPT, IP_ESP_TRANS_LEVEL, level).expect("set");
        assert_eq!(int_opt(so, PRCO_GETOPT, IP_ESP_TRANS_LEVEL, 0), Ok(level));
    }
    let inp = sotoinpcb(so).expect("attached");
    assert_eq!(
        i32::from(inp.inp_seclevel.get().sl_esp_trans),
        IPSEC_LEVEL_BYPASS
    );
    assert_eq!(
        int_opt(so, PRCO_SETOPT, IP_ESP_TRANS_LEVEL, 5),
        Err(Errno::EINVAL)
    );

    // udpencap_port is never handed out as a dynamic port.
    assert!(crate::netinet::in_pcb::in_baddynamic(
        4500,
        IPPROTO_UDP as u16
    ));
    assert!(!crate::netinet::in_pcb::in_baddynamic(
        4501,
        IPPROTO_UDP as u16
    ));

    soclose(so, 0).expect("close");
    teardown();
}
