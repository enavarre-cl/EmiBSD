//! Host tests of TCP through its user requests, over the test Ethernet interface: an active
//! open (SYN out, SYN-ACK in, ACK out: ESTABLISHED), data both ways, the FIN exchange of a
//! close; a passive open through the SYN cache (SYN in, SYN-ACK out, ACK in: a new socket on
//! the listener's queue); a RST for a segment to a closed port; and the sysctl counters.

use std::{assert, assert_eq, boxed::Box, vec, vec::Vec};

use super::*;
use crate::kern::uipc_mbuf::{m_freem, m_gethdr};
use crate::kern::uipc_socket::{soclose, socreate};
use crate::kern::uipc_socket2::{solock, sounlock};
use crate::net::ethertypes::{ETHERTYPE_ARP, ETHERTYPE_IP};
use crate::net::if_::tests::test_packet;
use crate::net::if_arp::{ARPHRD_ETHER, ARPOP_REPLY, Arphdr};
use crate::net::if_ethersubr::ether_input;
use crate::net::if_var::Ifnet;
use crate::net::ifq::ifq_dequeue;
use crate::netinet::if_ether::{EtherArp, EtherHeader, arpintr};
use crate::netinet::in_cksum::in_cksum;
use crate::netinet::in_pcb::tests::{nam, setup, teardown};
use crate::netinet::in4_cksum::in4_cksum;
use crate::netinet::ip_input::ipintr;
use crate::netinet::ip_input::tests::{
    ADDR, GATEWAY, OURS, PEER, bytes, configure, frame, sin, test_ether,
};
use crate::netinet::ip_var::{mtod_ip, mtod_ip_store};
use crate::netinet::tcp::{TH_ACK, TH_FIN, TH_RST, TH_SYN};
use crate::netinet::tcp_fsm::{TCPS_FIN_WAIT_1, TCPS_FIN_WAIT_2, TCPS_TIME_WAIT};
use crate::netinet::tcp_subr::tcp_init;
use crate::netinet::tcp_var::sototcpcb;
use crate::sys::mbuf::M_DONTWAIT;
use crate::sys::mbuf::MT_DATA;
use crate::sys::socket::SOCK_STREAM;
use crate::sys::socketvar::SS_CANTRCVMORE;

/// The interface with our address, its queue drained; TCP set up.
fn net() -> &'static Ifnet {
    tcp_init();
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

/// The gateway answers our ARP request.
fn answer_arp(ifp: &'static Ifnet) {
    let reply = EtherArp {
        ea_hdr: Arphdr {
            ar_hrd: ARPHRD_ETHER.to_be(),
            ar_pro: ETHERTYPE_IP.to_be(),
            ar_hln: 6,
            ar_pln: 4,
            ar_op: ARPOP_REPLY.to_be(),
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

/// A segment the interface sent: its IP and TCP bytes.
struct Seg(Vec<u8>);

impl Seg {
    fn flags(&self) -> u8 {
        self.0[33]
    }
    fn seq(&self) -> u32 {
        u32::from_be_bytes([self.0[24], self.0[25], self.0[26], self.0[27]])
    }
    fn ack(&self) -> u32 {
        u32::from_be_bytes([self.0[28], self.0[29], self.0[30], self.0[31]])
    }
    fn sport(&self) -> u16 {
        u16::from_be_bytes([self.0[20], self.0[21]])
    }
    fn dport(&self) -> u16 {
        u16::from_be_bytes([self.0[22], self.0[23]])
    }
    fn payload(&self) -> &[u8] {
        let off = 20 + usize::from(self.0[32] >> 4) * 4;
        &self.0[off..]
    }
}

/// The next segment on the interface's send queue, its checksums verified.
fn next_seg(ifp: &Ifnet) -> Option<Seg> {
    let out = ifq_dequeue(&ifp.if_snd)?;
    let b = bytes(out);
    m_freem(out);
    let ipb = b[size_of::<EtherHeader>()..].to_vec();
    assert_eq!(ipb[9], IPPROTO_TCP as u8);
    let m = test_packet(&ipb);
    assert_eq!(in_cksum(m, 20), 0, "ip header checksum");
    // The checksum may be left to the interface (M_TCP_CSUM_OUT): in_proto_cksum_out filled
    // it when the test interface has no offload.
    assert_eq!(
        in4_cksum(m, IPPROTO_TCP as u8, 20, (ipb.len() - 20) as i32),
        0,
        "tcp checksum"
    );
    m_freem(m);
    Some(Seg(ipb))
}

/// The segment `GATEWAY:sport -> ADDR:dport` with `flags`, `seq`, `ack`, options and data.
fn segment(
    sport: u16,
    dport: u16,
    flags: u8,
    seq: u32,
    ack: u32,
    opts: &[u8],
    data: &[u8],
) -> Vec<u8> {
    let thlen = 20 + opts.len();
    let len = 20 + thlen + data.len();
    let mut p = vec![0x45, 0];
    p.extend_from_slice(&(len as u16).to_be_bytes());
    p.extend_from_slice(&[0, 0, 0, 0, 64, IPPROTO_TCP as u8, 0, 0]);
    p.extend_from_slice(&GATEWAY);
    p.extend_from_slice(&ADDR);
    p.extend_from_slice(&sport.to_be_bytes());
    p.extend_from_slice(&dport.to_be_bytes());
    p.extend_from_slice(&seq.to_be_bytes());
    p.extend_from_slice(&ack.to_be_bytes());
    p.push(((thlen / 4) as u8) << 4);
    p.push(flags);
    p.extend_from_slice(&16384u16.to_be_bytes());
    p.extend_from_slice(&[0, 0, 0, 0]);
    p.extend_from_slice(opts);
    p.extend_from_slice(data);
    let m = test_packet(&p);
    let mut ip = mtod_ip(m);
    ip.ip_sum = in_cksum(m, 20);
    mtod_ip_store(m, &ip);
    let sum = in4_cksum(m, IPPROTO_TCP as u8, 20, (len - 20) as i32);
    let mut b = bytes(m);
    m_freem(m);
    b[36..38].copy_from_slice(&sum.to_ne_bytes());
    b
}

/// Hands `datagram` to the interface as the gateway's and runs the IP input queue.
fn receive(ifp: &'static Ifnet, datagram: &[u8]) {
    ether_input(ifp, frame(ifp, OURS, ETHERTYPE_IP, datagram), None);
    ipintr();
}

/// `addr:port`.
fn sinp(addr: [u8; 4], port: u16) -> SockaddrIn {
    let mut s = sin(addr);
    s.sin_port = port.to_be();
    s
}

/// A data mbuf holding `data`.
fn data_mbuf(data: &[u8]) -> &'static Mbuf {
    let m = m_gethdr(M_DONTWAIT, MT_DATA).expect("mbuf");
    m.m_len().set(data.len() as u32);
    m.m_pkthdr().len.set(data.len() as i32);
    // SAFETY: a fresh packet header mbuf holds `MHLEN` bytes.
    unsafe { ptr::copy_nonoverlapping(data.as_ptr(), mtod::<u8>(m), data.len()) };
    m
}

/// The bytes queued in `so`'s receive buffer.
fn received(so: &Socket) -> Vec<u8> {
    let mut v = Vec::new();
    let mut m = so.so_rcv.sb_mb.get();
    while let Some(mm) = m {
        let mut d = Some(mm);
        while let Some(x) = d {
            // SAFETY: an mbuf holds `m_len` bytes at its data pointer.
            v.extend_from_slice(unsafe {
                core::slice::from_raw_parts(mtod::<u8>(x), x.m_len().get() as usize)
            });
            d = x.m_next().get();
        }
        m = mm.m_nextpkt().get();
    }
    v
}

/// The counter of `c`.
fn tcpstat(c: TcpstatCounters) -> u64 {
    TCPCOUNTERS[c as usize].load(Ordering::Relaxed)
}

#[test]
fn active_open_data_and_close() {
    let (_g, _t, _p) = setup();
    let ifp = net();

    let so = socreate(i32::from(AF_INET), SOCK_STREAM, 0).expect("socket");
    let attempts = tcpstat(TcpstatCounters::TcpsConnattempt);
    solock(so);
    tcp_connect(so, nam(sinp(GATEWAY, 80))).expect("connect");
    sounlock(so);
    assert_eq!(tcpstat(TcpstatCounters::TcpsConnattempt), attempts + 1);
    let tp = sototcpcb(so).expect("tcpcb");
    assert_eq!(tp.t_state.get(), TCPS_SYN_SENT);
    assert!(so.has_state(SS_ISCONNECTING));

    // The SYN waits for ARP.
    drain(ifp);
    answer_arp(ifp);
    let syn = next_seg(ifp).expect("the SYN");
    assert_eq!(syn.flags(), TH_SYN);
    assert_eq!(syn.dport(), 80);
    let lport = syn.sport();
    let iss = syn.seq();
    // MSS option first.
    assert_eq!(&syn.0[40..42], &[2, 4]);

    // SYN-ACK with an MSS of 1000.
    let peer_iss = 0x1000_0000u32;
    receive(
        ifp,
        &segment(
            80,
            lport,
            TH_SYN | TH_ACK,
            peer_iss,
            iss.wrapping_add(1),
            &[2, 4, 0x03, 0xe8],
            &[],
        ),
    );
    assert_eq!(tp.t_state.get(), TCPS_ESTABLISHED);
    assert!(so.has_state(SS_ISCONNECTED));
    assert_eq!(tp.t_maxseg.get(), 1000);
    let ack = next_seg(ifp).expect("the ACK");
    assert_eq!(ack.flags(), TH_ACK);
    assert_eq!(ack.ack(), peer_iss.wrapping_add(1));

    // Data out.
    solock(so);
    tcp_send(so, Some(data_mbuf(b"hello")), None, None).expect("send");
    sounlock(so);
    let d = next_seg(ifp).expect("data");
    assert_eq!(d.flags() & TH_ACK, TH_ACK);
    assert_eq!(d.seq(), iss.wrapping_add(1));
    assert_eq!(d.payload(), b"hello");

    // Data in, acknowledging ours.
    receive(
        ifp,
        &segment(
            80,
            lport,
            TH_ACK,
            peer_iss.wrapping_add(1),
            iss.wrapping_add(6),
            &[],
            b"world",
        ),
    );
    assert_eq!(received(so), b"world");
    assert_eq!(tp.snd_una.get(), iss.wrapping_add(6));
    assert_eq!(tp.rcv_nxt.get(), peer_iss.wrapping_add(6));

    // shutdown(2): our FIN; the peer acknowledges it and sends its own.
    solock(so);
    tcp_shutdown(so).expect("shutdown");
    sounlock(so);
    assert_eq!(tp.t_state.get(), TCPS_FIN_WAIT_1);
    let mut fin = next_seg(ifp).expect("our FIN");
    // A delayed ACK of the data may come first.
    if fin.flags() & TH_FIN == 0 {
        fin = next_seg(ifp).expect("our FIN");
    }
    assert_eq!(fin.flags() & TH_FIN, TH_FIN);
    assert_eq!(fin.seq(), iss.wrapping_add(6));
    receive(
        ifp,
        &segment(
            80,
            lport,
            TH_ACK,
            peer_iss.wrapping_add(6),
            iss.wrapping_add(7),
            &[],
            &[],
        ),
    );
    assert_eq!(tp.t_state.get(), TCPS_FIN_WAIT_2);
    receive(
        ifp,
        &segment(
            80,
            lport,
            TH_FIN | TH_ACK,
            peer_iss.wrapping_add(6),
            iss.wrapping_add(7),
            &[],
            &[],
        ),
    );
    assert_eq!(tp.t_state.get(), TCPS_TIME_WAIT);
    assert!(so.so_rcv.has_state(SS_CANTRCVMORE));
    let last = next_seg(ifp).expect("the ACK of the FIN");
    assert_eq!(last.ack(), peer_iss.wrapping_add(7));

    let _ = soclose(so, 0);
    drain(ifp);
    teardown();
}

#[test]
fn passive_open_through_the_syn_cache() {
    let (_g, _t, p) = setup();
    let ifp = net();
    // The peer's MAC is known (no ARP wait for the SYN-ACK).
    answer_arp(ifp);
    drain(ifp);

    let l = socreate(i32::from(AF_INET), SOCK_STREAM, 0).expect("socket");
    solock(l);
    tcp_bind(l, nam(sinp(ADDR, 8080)), p).expect("bind");
    crate::kern::uipc_socket::solisten(l, 5).expect("listen");
    sounlock(l);
    assert_eq!(sototcpcb(l).expect("tcpcb").t_state.get(), TCPS_LISTEN);

    let peer_iss = 0x2000_0000u32;
    receive(
        ifp,
        &segment(40000, 8080, TH_SYN, peer_iss, 0, &[2, 4, 0x05, 0xb4], &[]),
    );
    let synack = next_seg(ifp).expect("the SYN-ACK");
    assert_eq!(synack.flags(), TH_SYN | TH_ACK);
    assert_eq!(synack.ack(), peer_iss.wrapping_add(1));
    assert_eq!(synack.sport(), 8080);
    assert_eq!(l.so_qlen.get(), 0, "embryonic: in the syn cache only");

    receive(
        ifp,
        &segment(
            40000,
            8080,
            TH_ACK,
            peer_iss.wrapping_add(1),
            synack.seq().wrapping_add(1),
            &[],
            b"hi",
        ),
    );
    assert_eq!(l.so_qlen.get(), 1, "a connection to accept");
    let so = l.so_q.first().expect("the new socket");
    let tp = sototcpcb(so).expect("its tcpcb");
    assert_eq!(tp.t_state.get(), TCPS_ESTABLISHED);
    assert_eq!(received(so), b"hi");

    let _ = soclose(l, 0);
    drain(ifp);
    teardown();
}

#[test]
fn a_segment_to_a_closed_port_is_reset() {
    let (_g, _t, _p) = setup();
    let ifp = net();
    answer_arp(ifp);
    drain(ifp);

    let noport = tcpstat(TcpstatCounters::TcpsNoport);
    receive(ifp, &segment(40001, 9, TH_SYN, 77, 0, &[], &[]));
    assert_eq!(tcpstat(TcpstatCounters::TcpsNoport), noport + 1);
    let rst = next_seg(ifp).expect("a RST");
    assert_eq!(rst.flags(), TH_RST | TH_ACK);
    assert_eq!(rst.ack(), 78);
    assert_eq!(rst.dport(), 40001);
    teardown();
}

#[test]
fn sysctl_reads_the_statistics_and_the_variables() {
    let (_g, _t, _p) = setup();
    let _ifp = net();

    let mut buf = Box::new([0u8; size_of::<Tcpstat>()]);
    let mut len = buf.len();
    // A kernel buffer stands for the user one on the host (copyout is a memcpy there).
    tcp_sysctl(&[TCPCTL_STATS], buf.as_mut_ptr() as usize, &mut len, 0, 0).expect("stats");
    assert_eq!(len, size_of::<Tcpstat>());
    let mut v = 0i32;
    let mut len = size_of::<i32>();
    tcp_sysctl(
        &[TCPCTL_MSSDFLT],
        ptr::from_mut(&mut v) as usize,
        &mut len,
        0,
        0,
    )
    .expect("mssdflt");
    assert_eq!(v, TCP_MSS);
    assert_eq!(
        tcp_sysctl(&[TCPCTL_STATS, 1], 0, &mut len, 0, 0),
        Err(Errno::ENOTDIR)
    );
    teardown();
}
