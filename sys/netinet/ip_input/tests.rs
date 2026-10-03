//! Host tests for IPv4: header validation in `ipv4_input` on crafted packets, and the whole
//! path of a ping over a test Ethernet interface: an address through `in_ioctl`, the
//! routes it makes, a default route, ARP resolution with the held packet sent once the reply
//! arrives, and an echo reply counted by `icmp_input`.

use std::sync::MutexGuard;
use std::vec::Vec;

use super::*;
use crate::kern::uipc_mbuf::{m_freem, m_gethdr};
use crate::net::ethertypes::{ETHERTYPE_ARP, ETHERTYPE_IP};
use crate::net::if_::tests::{setup_net, test_packet, zeroed_static};
use crate::net::if_::{
    IFF_BROADCAST, IFF_MULTICAST, IFF_RUNNING, IFF_SIMPLEX, IFF_UP, IFNAMSIZ, if_attach,
};
use crate::net::if_arp::{ARPOP_REPLY, ARPOP_REQUEST};
use crate::net::if_ethersubr::{ether_ifattach, ether_input, ether_ioctl};
use crate::net::if_var::Ifnet;
use crate::net::ifq::ifq_dequeue;
use crate::net::route::{
    RTAX_DST, RTAX_GATEWAY, RTAX_NETMASK, RTF_BROADCAST, RTF_CLONED, RTF_CLONING, RTF_CONNECTED,
    RTF_GATEWAY, RTF_LLINFO, RTF_LOCAL, RTF_STATIC, RTM_ADD, RtAddrinfo, route_init, rtalloc,
    rtfree, rtrequest,
};
use crate::net::rtable::rtable_init;
use crate::netinet::icmp_var::IcmpstatCounters;
use crate::netinet::if_ether::{Arpcom, EtherArp, EtherHeader, arpcom_of, arpintr};
use crate::netinet::in_::{IPPROTO_ICMP, in_ioctl};
use crate::netinet::in_var::InAliasreq;
use crate::netinet::in4_cksum::in4_cksum;
use crate::netinet::ip_icmp::{ICMP_ECHO, ICMP_ECHOREPLY, ICMPCOUNTERS, IcmpPkt, icmp_init};
use crate::netinet::ip_output::ip_output;
use crate::sys::mbuf::{M_ICMP_CSUM_OUT, MHLEN};
use crate::sys::sockio::{SIOCAIFADDR, SIOCDIFADDR, SIOCSIFADDR};

pub(crate) const OURS: [u8; 6] = [0x52, 0x54, 0x00, 0x12, 0x34, 0x56];
pub(crate) const PEER: [u8; 6] = [0x52, 0x55, 0x0a, 0x00, 0x02, 0x02];
pub(crate) const ADDR: [u8; 4] = [10, 0, 2, 15];
pub(crate) const GATEWAY: [u8; 4] = [10, 0, 2, 2];

/// The network test setup plus what `rtable_init` and `domaininit` do for the inet and route
/// domains; with the timeout wheel reset (`arpinit` arms a timeout), under its lock too.
pub(crate) fn setup() -> (MutexGuard<'static, ()>, MutexGuard<'static, ()>) {
    let g = setup_net();
    let t = crate::kern::kern_timeout::tests::LOCK
        .lock()
        .unwrap_or_else(|e| e.into_inner());
    crate::kern::kern_timeout::timeout_startup();
    // ARP reads an expiry of 0 as "permanent": run as a system past its first second.
    crate::kern::kern_tc::TIME_UPTIME.store(100, Ordering::Relaxed);
    // The interface queues name their softnet task queue (no thread runs it here).
    crate::net::if_::softnet_init();
    rtable_init();
    route_init();
    ip_init();
    icmp_init();
    (g, t)
}

/// A `sockaddr_in` for `a`.
pub(crate) fn sin(a: [u8; 4]) -> SockaddrIn {
    SockaddrIn {
        sin_len: size_of::<SockaddrIn>() as u8,
        sin_family: AF_INET,
        sin_addr: InAddr {
            s_addr: u32::from_ne_bytes(a),
        },
        ..SockaddrIn::default()
    }
}

/// A driver `ioctl` that takes addresses (as drivers do with `ether_ioctl`).
///
/// # Safety
///
/// `IfIoctlFn`'s contract.
unsafe fn test_ether_ioctl(ifp: &'static Ifnet, cmd: u64, data: *mut u8) -> Result<(), Errno> {
    if cmd == SIOCSIFADDR {
        ifp.if_flags.set(ifp.if_flags.get() | IFF_UP | IFF_RUNNING);
        return Ok(());
    }
    // SAFETY: the caller's contract.
    unsafe { ether_ioctl(ifp, arpcom_of(ifp), cmd, data) }
}

/// An attached Ethernet interface `tvio0` with our hardware address.
pub(crate) fn test_ether() -> &'static Ifnet {
    // SAFETY: the all-zero `Arpcom` is valid (`netinet/if_ether.rs`).
    let ac: &'static Arpcom = unsafe { zeroed_static() };
    let mut xname = [0u8; IFNAMSIZ];
    xname[..5].copy_from_slice(b"tvio0");
    ac.ac_if.if_xname.set(xname);
    ac.ac_if.if_ioctl.set(Some(test_ether_ioctl));
    ac.ac_if
        .if_flags
        .set(IFF_BROADCAST | IFF_SIMPLEX | IFF_MULTICAST);
    ac.ac_enaddr.set(OURS);
    if_attach(&ac.ac_if);
    ether_ifattach(ac);
    &ac.ac_if
}

/// `ifconfig <ifp> inet <addr> netmask <mask>`, from the kernel.
pub(crate) fn configure(ifp: &'static Ifnet, addr: [u8; 4], mask: [u8; 4]) {
    let mut ifra = InAliasreq::zeroed();
    ifra.ifra_name = ifp.if_xname.get();
    *ifra.ifra_addr_mut() = sin(addr);
    ifra.ifra_mask = sin(mask);
    // SAFETY: an `in_aliasreq`, from the kernel itself.
    unsafe {
        in_ioctl(
            SIOCAIFADDR,
            ptr::from_mut(&mut ifra).cast(),
            Some(ifp),
            true,
        )
    }
    .expect("SIOCAIFADDR");
}

/// `ifconfig <ifp> inet <addr> delete`.
pub(crate) fn unconfigure(ifp: &'static Ifnet, addr: [u8; 4]) {
    let mut ifra = InAliasreq::zeroed();
    ifra.ifra_name = ifp.if_xname.get();
    *ifra.ifra_addr_mut() = sin(addr);
    // SAFETY: an `in_aliasreq`, from the kernel itself.
    unsafe {
        in_ioctl(
            SIOCDIFADDR,
            ptr::from_mut(&mut ifra).cast(),
            Some(ifp),
            true,
        )
    }
    .expect("SIOCDIFADDR");
}

/// The bytes of a packet.
pub(crate) fn bytes(m: &Mbuf) -> Vec<u8> {
    let mut v = std::vec![0u8; m.m_pkthdr().len.get() as usize];
    m_copydata(m, 0, &mut v);
    v
}

/// An IPv4 packet with header `hdr` (checksummed here unless `bad_sum`) and `payload`.
fn ip_packet(hdr: [u8; 20], payload: &[u8], bad_sum: bool) -> &'static Mbuf {
    let mut p = hdr.to_vec();
    p.extend_from_slice(payload);
    let m = test_packet(&p);
    if !bad_sum {
        let mut ip = mtod_ip(m);
        ip.ip_sum = 0;
        mtod_ip_store(m, &ip);
        ip.ip_sum = in_cksum(m, 20);
        mtod_ip_store(m, &ip);
    }
    m
}

/// The counter of `c`.
fn ipstat(c: IpstatCounters) -> u64 {
    IPCOUNTERS[c as usize].load(Ordering::Relaxed)
}

#[test]
fn ipv4_input_rejects_bad_headers() {
    let _g = setup();
    let ifp = test_ether();
    let hdr = |vhl: u8, len: u16| -> [u8; 20] {
        let l = len.to_be_bytes();
        [
            vhl, 0, l[0], l[1], 0, 0, 0, 0, 64, 1, 0, 0, 10, 0, 2, 2, 10, 0, 2, 15,
        ]
    };

    let badvers = ipstat(IpstatCounters::IpsBadvers);
    ipv4_input(ifp, ip_packet(hdr(0x65, 20), &[], false), None);
    assert_eq!(ipstat(IpstatCounters::IpsBadvers), badvers + 1);

    let badhlen = ipstat(IpstatCounters::IpsBadhlen);
    ipv4_input(ifp, ip_packet(hdr(0x44, 20), &[], false), None);
    assert_eq!(ipstat(IpstatCounters::IpsBadhlen), badhlen + 1);

    let badsum = ipstat(IpstatCounters::IpsBadsum);
    let m = ip_packet(hdr(0x45, 20), &[], true);
    let mut ip = mtod_ip(m);
    ip.ip_sum = 0x1234;
    mtod_ip_store(m, &ip);
    ipv4_input(ifp, m, None);
    assert_eq!(ipstat(IpstatCounters::IpsBadsum), badsum + 1);

    let badlen = ipstat(IpstatCounters::IpsBadlen);
    ipv4_input(ifp, ip_packet(hdr(0x45, 10), &[], false), None);
    assert_eq!(ipstat(IpstatCounters::IpsBadlen), badlen + 1);

    let tooshort = ipstat(IpstatCounters::IpsTooshort);
    ipv4_input(ifp, ip_packet(hdr(0x45, 40), &[0; 8], false), None);
    assert_eq!(ipstat(IpstatCounters::IpsTooshort), tooshort + 1);

    // 127/8 must not appear on the wire.
    let badaddr = ipstat(IpstatCounters::IpsBadaddr);
    let mut h = hdr(0x45, 20);
    h[12] = 127;
    ipv4_input(ifp, ip_packet(h, &[], false), None);
    assert_eq!(ipstat(IpstatCounters::IpsBadaddr), badaddr + 1);

    // A good header for an address we do not have: not forwarded (ip_forwarding is 0).
    let cantforward = ipstat(IpstatCounters::IpsCantforward);
    ipv4_input(ifp, ip_packet(hdr(0x45, 28), &[0; 8], false), None);
    assert_eq!(ipstat(IpstatCounters::IpsCantforward), cantforward + 1);
}

/// A frame from the peer to us of `etype` around `payload`, as the driver would hand it up.
pub(crate) fn frame(ifp: &Ifnet, dst: [u8; 6], etype: u16, payload: &[u8]) -> &'static Mbuf {
    let mut f = Vec::new();
    f.extend_from_slice(&dst);
    f.extend_from_slice(&PEER);
    f.extend_from_slice(&etype.to_be_bytes());
    f.extend_from_slice(payload);
    let m = test_packet(&f);
    m.m_pkthdr().ph_ifidx.set(ifp.if_index.get());
    m
}

#[test]
fn a_ping_to_the_gateway_resolves_it_and_counts_the_reply() {
    let _g = setup();
    let ifp = test_ether();

    // ifconfig tvio0 10.0.2.15/24
    configure(ifp, ADDR, [255, 255, 255, 0]);
    assert!(ifp.if_flags.get() & IFF_UP != 0, "the driver brought it up");

    // The local, cloning and broadcast routes.
    let route_flags = |a: [u8; 4]| {
        let mut s = sin(a);
        // SAFETY: a local `sockaddr_in`.
        let rt = unsafe { rtalloc(sintosa(&mut s), 0, 0) }.expect("route");
        let f = rt.rt_flags.get();
        rtfree(Some(rt));
        f
    };
    assert_ne!(route_flags(ADDR) & RTF_LOCAL, 0);
    assert_ne!(
        route_flags([10, 0, 2, 77]) & (RTF_CLONING | RTF_CONNECTED),
        0
    );
    assert_ne!(route_flags([10, 0, 2, 255]) & RTF_BROADCAST, 0);
    // The address was announced (a gratuitous ARP request for itself).
    let m = ifq_dequeue(&ifp.if_snd).expect("announcement");
    let b = bytes(m);
    assert_eq!(&b[12..14], &ETHERTYPE_ARP.to_be_bytes());
    assert_eq!(&b[28..32], &ADDR);
    assert_eq!(&b[38..42], &ADDR);
    m_freem(m);
    while let Some(m) = ifq_dequeue(&ifp.if_snd) {
        m_freem(m);
    }

    // route add default 10.0.2.2
    let mut dst = sin([0; 4]);
    let mut mask = sin([0; 4]);
    let mut gw = sin(GATEWAY);
    let mut info = RtAddrinfo::new();
    info.rti_info[RTAX_DST] = sintosa(&mut dst);
    info.rti_info[RTAX_NETMASK] = sintosa(&mut mask);
    info.rti_info[RTAX_GATEWAY] = sintosa(&mut gw);
    info.rti_flags = RTF_GATEWAY | RTF_STATIC;
    // SAFETY: a local `sockaddr_in`.
    info.rti_ifa = unsafe { crate::net::if_::ifaof_ifpforaddr(sintosa(&mut gw), ifp) };
    let mut rt = None;
    // SAFETY: the addresses are locals.
    unsafe { rtrequest(RTM_ADD, &mut info, 0, Some(&mut rt), 0) }.expect("default route");
    let def = rt.expect("route");
    let nh = def.rt_gwroute.get().expect("next hop cached");
    assert_ne!(nh.rt_flags.get() & RTF_LLINFO, 0);
    assert_ne!(nh.rt_flags.get() & RTF_CLONED, 0);
    rtfree(rt);
    assert_ne!(route_flags([8, 8, 8, 8]) & RTF_GATEWAY, 0);

    // ping: the request is held while ARP asks for the gateway.
    let m = m_gethdr(M_DONTWAIT, crate::sys::mbuf::MT_DATA).expect("mbuf");
    let len = 20 + 8 + 16;
    m.m_data()
        .set(m.m_data().get().wrapping_add((MHLEN - len) & !7));
    m.m_len().set(len as u32);
    m.m_pkthdr().len.set(len as i32);
    let ip = Ip {
        ip_len: htons(len as u16),
        ip_ttl: 64,
        ip_p: IPPROTO_ICMP as u8,
        ip_dst: sin(GATEWAY).sin_addr,
        ..Ip::default()
    };
    mtod_ip_store(m, &ip);
    let icp = IcmpPkt::of(m, 20);
    icp.set_icmp_type(ICMP_ECHO);
    icp.set_icmp_code(0);
    icp.set_icmp_cksum(0);
    icp.set_icmp_id(htons(7));
    icp.set_icmp_seq(htons(1));
    m.m_pkthdr().csum_flags.set(M_ICMP_CSUM_OUT);
    ip_output(m, None, None, 0, None, 0).expect("held by ARP");

    let req = ifq_dequeue(&ifp.if_snd).expect("ARP request");
    let b = bytes(req);
    m_freem(req);
    assert_eq!(&b[0..6], &[0xff; 6]);
    assert_eq!(&b[12..14], &ETHERTYPE_ARP.to_be_bytes());
    assert_eq!(&b[20..22], &ARPOP_REQUEST.to_be_bytes());
    assert_eq!(&b[28..32], &ADDR, "from our address");
    assert_eq!(&b[38..42], &GATEWAY, "for the gateway");
    assert!(
        ifq_dequeue(&ifp.if_snd).is_none(),
        "the echo request is held"
    );

    // The gateway answers: the held packet goes out to its hardware address.
    let reply = EtherArp {
        ea_hdr: crate::net::if_arp::Arphdr {
            ar_hrd: htons(crate::net::if_arp::ARPHRD_ETHER),
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
        core::slice::from_raw_parts(ptr::from_ref(&reply).cast::<u8>(), size_of::<EtherArp>())
    };
    ether_input(ifp, frame(ifp, OURS, ETHERTYPE_ARP, arp), None);
    arpintr();

    let echo = ifq_dequeue(&ifp.if_snd).expect("the echo request");
    let b = bytes(echo);
    assert_eq!(&b[0..6], &PEER, "to the gateway's hardware address");
    assert_eq!(&b[6..12], &OURS);
    assert_eq!(&b[12..14], &ETHERTYPE_IP.to_be_bytes());
    let ipb = &b[size_of::<EtherHeader>()..];
    assert_eq!(ipb[0], 0x45);
    assert_eq!(&ipb[12..16], &ADDR, "ip_output chose our address");
    assert_eq!(&ipb[16..20], &GATEWAY);
    assert_eq!(ipb[20], ICMP_ECHO);
    // Both checksums are right: the header's and the delayed ICMP one.
    let m2 = test_packet(ipb);
    assert_eq!(in_cksum(m2, 20), 0, "ip header checksum");
    assert_eq!(in4_cksum(m2, 0, 20, len as i32 - 20), 0, "icmp checksum");
    m_freem(m2);
    m_freem(echo);

    // The echo reply comes back and icmp_input counts it.
    let mut r = ipb.to_vec();
    r[12..16].copy_from_slice(&GATEWAY);
    r[16..20].copy_from_slice(&ADDR);
    r[10] = 0;
    r[11] = 0;
    r[20] = ICMP_ECHOREPLY;
    r[22] = 0;
    r[23] = 0;
    let m3 = test_packet(&r);
    let mut ip = mtod_ip(m3);
    ip.ip_sum = in_cksum(m3, 20);
    mtod_ip_store(m3, &ip);
    let sum = in4_cksum(m3, 0, 20, len as i32 - 20);
    IcmpPkt::of(m3, 20).set_icmp_cksum(sum);
    let r = bytes(m3);
    m_freem(m3);

    let counter =
        &ICMPCOUNTERS[IcmpstatCounters::IcpsInhist as usize + usize::from(ICMP_ECHOREPLY)];
    let before = counter.load(Ordering::Relaxed);
    ether_input(ifp, frame(ifp, OURS, ETHERTYPE_IP, &r), None);
    // icmp_input needs the exclusive net lock: the packet waits on ipintrq.
    assert_eq!(counter.load(Ordering::Relaxed), before);
    ipintr();
    assert_eq!(
        counter.load(Ordering::Relaxed),
        before + 1,
        "echo reply received"
    );

    // Deleting the address takes its routes, and the gateway's ARP entry, with it.
    unconfigure(ifp, ADDR);
    let mut s = sin(GATEWAY);
    // SAFETY: a local `sockaddr_in`.
    let rt = unsafe { rtalloc(sintosa(&mut s), 0, 0) };
    assert!(rt.is_none(), "no route to the gateway is left");
    while let Some(m) = ifq_dequeue(&ifp.if_snd) {
        m_freem(m);
    }
}

/// An ICMP message of `type_` from the gateway to us, with `opts` in its IP header.
fn icmp_to_us(type_: u8, opts: &[u8]) -> &'static Mbuf {
    let hlen = 20 + opts.len();
    let len = hlen + 8 + 8;
    let mut p = std::vec![0u8; len];
    p[0] = 0x40 | (hlen / 4) as u8;
    p[2..4].copy_from_slice(&(len as u16).to_be_bytes());
    p[8] = 64;
    p[9] = IPPROTO_ICMP as u8;
    p[12..16].copy_from_slice(&GATEWAY);
    p[16..20].copy_from_slice(&ADDR);
    p[20..hlen].copy_from_slice(opts);
    p[hlen] = type_;
    p[hlen + 4..hlen + 8].copy_from_slice(&[0, 9, 0, 1]);
    let m = test_packet(&p);
    let mut ip = mtod_ip(m);
    ip.ip_sum = in_cksum(m, hlen as i32);
    mtod_ip_store(m, &ip);
    let sum = in4_cksum(m, 0, hlen as i32, (len - hlen) as i32);
    IcmpPkt::of(m, hlen).set_icmp_cksum(sum);
    m
}

/// The packets `ip_send` queued, freed after `check` looked at each.
fn sent(mut check: impl FnMut(&[u8], u16)) -> usize {
    let ml = MbufList::new();
    mq_delist(&IPSEND_MQ, &ml);
    let mut n = 0;
    while let Some(m) = ml_dequeue(&ml) {
        check(&bytes(m), m.m_pkthdr().csum_flags.get());
        m_freem(m);
        n += 1;
    }
    n
}

#[test]
fn an_echo_request_is_answered() {
    let _g = setup();
    let ifp = test_ether();
    configure(ifp, ADDR, [255, 255, 255, 0]);
    let _ = sent(|_, _| {});

    let reflect = ICMPCOUNTERS[IcmpstatCounters::IcpsReflect as usize].load(Ordering::Relaxed);
    let m = icmp_to_us(ICMP_ECHO, &[]);
    m.m_pkthdr().ph_ifidx.set(ifp.if_index.get());
    ipv4_input(ifp, m, None);
    ipintr();
    assert_eq!(
        ICMPCOUNTERS[IcmpstatCounters::IcpsReflect as usize].load(Ordering::Relaxed),
        reflect + 1
    );

    let n = sent(|b, csum| {
        assert_eq!(&b[12..16], &ADDR, "from the address the request was for");
        assert_eq!(&b[16..20], &GATEWAY, "back to the sender");
        assert_eq!(b[8], crate::netinet::ip::MAXTTL);
        assert_eq!(b[20], ICMP_ECHOREPLY);
        assert_eq!(&b[24..28], &[0, 9, 0, 1], "the id and sequence stay");
        assert_ne!(csum & M_ICMP_CSUM_OUT, 0, "the checksum is left to output");
    });
    assert_eq!(n, 1);
    unconfigure(ifp, ADDR);
}

#[test]
fn a_bad_option_gets_a_parameter_problem() {
    let _g = setup();
    let ifp = test_ether();
    configure(ifp, ADDR, [255, 255, 255, 0]);
    let _ = sent(|_, _| {});

    // An option whose length runs past the header.
    let badoptions = ipstat(IpstatCounters::IpsBadoptions);
    let m = icmp_to_us(ICMP_ECHO, &[crate::netinet::ip::IPOPT_RR, 9, 4, 0]);
    m.m_pkthdr().ph_ifidx.set(ifp.if_index.get());
    ipv4_input(ifp, m, None);
    assert_eq!(ipstat(IpstatCounters::IpsBadoptions), badoptions + 1);

    let n = sent(|b, _| {
        assert_eq!(&b[16..20], &GATEWAY);
        assert_eq!(b[20], crate::netinet::ip_icmp::ICMP_PARAMPROB);
        assert_eq!(b[24], 21, "the pointer is the option's length byte");
        assert_eq!(
            &b[28..32],
            &[0x46, 0, 0, 40],
            "the offending header follows"
        );
    });
    assert_eq!(n, 1);
    unconfigure(ifp, ADDR);
}
