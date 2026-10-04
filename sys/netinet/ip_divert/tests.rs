//! Host tests for divert sockets: a packet pf diverts to port 700 lands in the receive buffer
//! of the divert socket bound to that port, with the receiving interface's address
//! (inbound) or the wildcard (outbound); without a socket on the port it is counted and
//! dropped; a write to an address that is not ours is refused.

use std::vec;

use super::*;
use crate::kern::uipc_socket::{soclose, socreate};
use crate::kern::uipc_socket2::{solock, sounlock};
use crate::net::if_::tests::test_packet;
use crate::netinet::in_::{IPPROTO_DIVERT, InAddr};
use crate::netinet::in_pcb::tests::{nam, setup, teardown};
use crate::netinet::ip_input::tests::{ADDR, configure, test_ether};
use crate::sys::endian::htons;
use crate::sys::mbuf::{MT_SONAME, mtod};
use crate::sys::socket::SOCK_RAW;

/// The value of counter `c`.
fn count(c: DivstatCounters) -> u64 {
    DIVCOUNTERS[c as usize].load(Ordering::Relaxed)
}

/// A UDP datagram from 10.0.2.2 to `dst`, as an IPv4 packet.
fn datagram(dst: [u8; 4]) -> &'static Mbuf {
    let mut p = vec![0u8; 20 + 8 + 4];
    p[0] = 0x45;
    p[2..4].copy_from_slice(&32u16.to_be_bytes());
    p[8] = 64;
    p[9] = IPPROTO_UDP as u8;
    p[12..16].copy_from_slice(&[10, 0, 2, 2]);
    p[16..20].copy_from_slice(&dst);
    p[22..24].copy_from_slice(&53u16.to_be_bytes());
    p[24..26].copy_from_slice(&12u16.to_be_bytes());
    p[28..].copy_from_slice(b"ping");
    test_packet(&p)
}

/// `addr:port` as a `sockaddr_in` (the port in host order).
fn sin(addr: [u8; 4], port: u16) -> SockaddrIn {
    SockaddrIn {
        sin_len: size_of::<SockaddrIn>() as u8,
        sin_family: AF_INET,
        sin_port: htons(port),
        sin_addr: InAddr {
            s_addr: u32::from_ne_bytes(addr),
        },
        ..SockaddrIn::default()
    }
}

#[test]
fn divert_packet_reaches_the_socket_on_its_port() {
    let (_g, _t, p) = setup();
    divert_init();
    let ifp = test_ether();
    configure(ifp, ADDR, [255, 255, 255, 0]);

    let so = socreate(i32::from(AF_INET), SOCK_RAW, IPPROTO_DIVERT).expect("divert socket");
    let inp = sotoinpcb(so).expect("attached");
    assert!(inp.has_flags(INP_HDRINCL));
    assert_eq!(so.so_rcv.sb_hiwat.get(), DIVERT_RECVSPACE as u64);
    solock(so);
    divert_bind(so, nam(sin([0; 4], 700)), p).expect("bind");
    sounlock(so);

    // Inbound on the test interface: the source is the interface's address.
    let ipackets = count(DivstatCounters::DivsIpackets);
    let m = datagram(ADDR);
    m.m_pkthdr().ph_ifidx.set(ifp.if_index.get());
    divert_packet(m, PF_IN, htons(700));
    assert_eq!(count(DivstatCounters::DivsIpackets), ipackets + 1);
    let rec = so.so_rcv.sb_mb.get().expect("a record");
    assert_eq!(i32::from(rec.m_type().get()), MT_SONAME);
    // SAFETY: the record's address is a `sockaddr_in`.
    let from = unsafe { mtod::<SockaddrIn>(rec).read_unaligned() };
    assert_eq!(from.sin_addr.s_addr, u32::from_ne_bytes(ADDR));
    let data = rec.m_next().get().expect("the packet");
    assert_eq!(data.m_pkthdr().len.get(), 32);
    let cc = so.so_rcv.sb_cc.get();

    // Outbound: the wildcard address, and the checksums computed for the reader.
    divert_packet(datagram([10, 0, 2, 2]), PF_OUT, htons(700));
    assert!(so.so_rcv.sb_cc.get() > cc);

    // No socket on port 701: counted and dropped.
    let noport = count(DivstatCounters::DivsNoport);
    divert_packet(datagram(ADDR), PF_IN, htons(701));
    assert_eq!(count(DivstatCounters::DivsNoport), noport + 1);

    // Reinjecting inbound toward an address that is not ours is refused.
    let errors = count(DivstatCounters::DivsErrors);
    solock(so);
    let r = divert_send(
        so,
        Some(datagram([10, 0, 2, 99])),
        Some(nam(sin([10, 0, 2, 99], 0))),
        None,
    );
    sounlock(so);
    assert_eq!(r, Err(Errno::EADDRNOTAVAIL));
    assert_eq!(count(DivstatCounters::DivsErrors), errors + 1);

    // net.inet.divert.stats: the counters as a struct divstat.
    let mut st = Divstat::default();
    let mut len = size_of::<Divstat>();
    divert_sysctl(
        &[DIVERTCTL_STATS],
        ptr::from_mut(&mut st) as usize,
        &mut len,
        0,
        0,
    )
    .expect("stats");
    assert_eq!(st.divs_noport, count(DivstatCounters::DivsNoport));

    soclose(so, 0).expect("close");
    teardown();
}
