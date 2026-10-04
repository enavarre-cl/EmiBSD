use super::*;
use crate::net::ethertypes::ETHERTYPE_ARP;
use crate::reftest::{assert_complete, assert_defines};
use crate::sys::endian::{htonl, htons};

#[test]
fn frame_layout_and_predicates() {
    let eh = EtherHeader {
        ether_dhost: [0xff; 6],
        ether_shost: [0x52, 0x54, 0x00, 0x12, 0x34, 0x56],
        ether_type: htons(ETHERTYPE_ARP),
    };
    // SAFETY: `EtherHeader` is 14 bytes of integers without padding.
    let bytes: [u8; 14] = unsafe { core::mem::transmute(eh) };
    assert_eq!(&bytes[12..], &[0x08, 0x06]);
    assert!(ether_is_broadcast(&eh.ether_dhost));
    assert!(ether_is_multicast(&eh.ether_dhost));
    assert!(!ether_is_multicast(&eh.ether_shost));
    assert!(ether_is_anyaddr(&[0; 6]));
    assert!(ether_is_eq(
        &eh.ether_shost,
        &[0x52, 0x54, 0x00, 0x12, 0x34, 0x56]
    ));
    assert!(eth64_is_multicast(0x0100_5e00_0001));
    assert!(eth64_is_broadcast(0xffff_ffff_ffff));
    assert!(eth64_is_8021_rsvd(0x0180_c200_000e));
    assert_eq!(ETHERMTU, 1500);
    assert_eq!(ETHERMIN, 46);
    assert_eq!(evl_vlanoftag(0x6064), 0x064);
    assert_eq!(evl_prioftag(0x6064), 3);

    let group = InAddr {
        s_addr: htonl(0xe0ff_0001),
    };
    assert_eq!(
        ether_map_ip_multicast(&group),
        [0x01, 0x00, 0x5e, 0x7f, 0x00, 0x01]
    );
    let mut ip6 = crate::netinet6::in6::In6Addr::default();
    ip6.s6_addr[12..].copy_from_slice(&[0xff, 0x00, 0x00, 0x01]);
    assert_eq!(
        ether_map_ipv6_multicast(&ip6),
        [0x33, 0x33, 0xff, 0x00, 0x00, 0x01]
    );
    assert_eq!(core::mem::offset_of!(EtherArp, arp_sha), 8);
}

#[test]
#[ignore = "needs OPENBSD_SRC (just test-ref)"]
fn values_match_the_c_header() {
    let defs = crate::reftest::defines("sys/netinet/if_ether.h");
    let ether = assert_defines!(defs;
        ETHER_ADDR_LEN, ETHER_TYPE_LEN, ETHER_CRC_LEN, ETHER_HDR_LEN, ETHER_MIN_LEN,
        ETHER_MAX_LEN, ETHER_MAX_DIX_LEN, ETHER_VLAN_ENCAP_LEN, ETHER_ALIGN,
        ETHER_MAX_HARDMTU_LEN, ETHER_CRC_POLY_LE, ETHER_CRC_POLY_BE);
    assert_complete(&defs, "ETHER_", &ether);
    let evl = assert_defines!(defs;
        EVL_VLID_MASK, EVL_VLID_NULL, EVL_VLID_MIN, EVL_VLID_MAX, EVL_PRIO_MAX, EVL_PRIO_BITS,
        EVL_ENCAPLEN);
    assert_complete(&defs, "EVL_", &evl);
    let eth64 = assert_defines!(defs; ETH64_8021_RSVD_PREFIX, ETH64_8021_RSVD_MASK);
    assert_complete(&defs, "ETH64_", &eth64);
    assert_defines!(defs; ETHERMTU, ETHERMIN, SIN_PROXY);
    assert_eq!(defs["RTF_USETRAILERS"], "RTF_PROTO1");
    assert_eq!(defs["RTF_PERMANENT_ARP"], "RTF_PROTO3");
}

/// An ARP packet of `op` from the gateway (`sha`/`spa`) to `tha`/`tpa`, as bytes.
fn arp_packet(op: u16, tha: [u8; 6], tpa: [u8; 4]) -> std::vec::Vec<u8> {
    use crate::netinet::ip_input::tests::{GATEWAY, PEER};
    let ea = EtherArp {
        ea_hdr: Arphdr {
            ar_hrd: htons(ARPHRD_ETHER),
            ar_pro: htons(ETHERTYPE_IP),
            ar_hln: 6,
            ar_pln: 4,
            ar_op: htons(op),
        },
        arp_sha: PEER,
        arp_spa: GATEWAY,
        arp_tha: tha,
        arp_tpa: tpa,
    };
    // SAFETY: an `ether_arp` is 28 bytes of integers without padding.
    let b: [u8; 28] = unsafe { core::mem::transmute(ea) };
    b.to_vec()
}

#[test]
fn an_arp_entry_is_made_resolved_answered_and_aged_out() {
    use crate::kern::kern_tc::TIME_UPTIME;
    use crate::kern::uipc_mbuf::m_freem;
    use crate::net::if_ethersubr::ether_input;
    use crate::net::ifq::ifq_dequeue;
    use crate::netinet::in_::sintosa;
    use crate::netinet::ip_input::tests::{
        ADDR, GATEWAY, OURS, PEER, bytes, configure, frame, setup, sin, test_ether, unconfigure,
    };

    let _g = setup();
    let ifp = test_ether();
    configure(ifp, ADDR, [255, 255, 255, 0]);
    while let Some(m) = ifq_dequeue(&ifp.if_snd) {
        m_freem(m);
    }

    // The gateway asks for our address: we learn its, and answer.
    ether_input(
        ifp,
        frame(
            ifp,
            [0xff; 6],
            ETHERTYPE_ARP,
            &arp_packet(ARPOP_REQUEST, [0; 6], ADDR),
        ),
        None,
    );
    arpintr();
    let reply = ifq_dequeue(&ifp.if_snd).expect("ARP reply");
    let b = bytes(reply);
    m_freem(reply);
    assert_eq!(&b[0..6], &PEER, "to the asker");
    assert_eq!(&b[20..22], &ARPOP_REPLY.to_be_bytes());
    assert_eq!(&b[22..28], &OURS);
    assert_eq!(&b[28..32], &ADDR);
    assert_eq!(&b[32..38], &PEER);
    assert_eq!(&b[38..42], &GATEWAY);

    let entry = || {
        let mut s = sin(GATEWAY);
        // SAFETY: a local `sockaddr_in`.
        let rt = unsafe { rtalloc(sintosa(&mut s), 0, 0) }.expect("route");
        let flags = rt.rt_flags.get();
        // SAFETY: an ARP route's gateway is a `sockaddr_dl`.
        let (alen, hw) = unsafe {
            let sdl = satosdl(rt.rt_gateway.get());
            let mut hw = [0u8; 6];
            if (*sdl).sdl_alen == 6 {
                ptr::copy_nonoverlapping(lladdr(sdl), hw.as_mut_ptr(), 6);
            }
            ((*sdl).sdl_alen, hw)
        };
        let expire = rt.rt_expire().get();
        rtfree(Some(rt));
        (flags, alen, hw, expire)
    };
    let (flags, alen, hw, expire) = entry();
    assert_ne!(flags & RTF_LLINFO, 0, "a cloned ARP entry");
    assert_eq!((alen, hw), (6, PEER), "resolved to the gateway's address");
    assert_eq!(
        expire,
        TIME_UPTIME.load(Ordering::Relaxed) + i64::from(ARPT_KEEP.load(Ordering::Relaxed))
    );

    // Past arpt_keep, arptimer removes the entry: the subnet's cloning route is what is left.
    TIME_UPTIME.store(expire + 1, Ordering::Relaxed);
    arptimer(ptr::from_ref(&ARPTIMER_TO).cast_mut().cast());
    let (flags, _, _, _) = entry();
    assert_eq!(flags & RTF_LLINFO, 0, "the entry is gone");
    assert_ne!(flags & RTF_CLONING, 0);

    unconfigure(ifp, ADDR);
    while let Some(m) = ifq_dequeue(&ifp.if_snd) {
        m_freem(m);
    }
}

/// The RARP client (`NFSCLIENT`): `revarprequest` broadcasts an RFC 903 request, and
/// `in_revarpinput` takes the reply only while a `revarpwhoarewe` is out on that interface and
/// only for our hardware address.
#[cfg(feature = "nfsclient")]
#[test]
fn a_rarp_request_goes_out_and_the_reply_is_taken_once() {
    use crate::kern::uipc_mbuf::m_freem;
    use crate::net::ethertypes::ETHERTYPE_REVARP;
    use crate::net::if_arp::ARPOP_REVREQUEST;
    use crate::net::if_ethersubr::ether_input;
    use crate::net::ifq::ifq_dequeue;
    use crate::netinet::ip_input::tests::{
        ADDR, GATEWAY, OURS, PEER, bytes, configure, frame, setup, test_ether, unconfigure,
    };

    let _g = setup();
    let ifp = test_ether();
    configure(ifp, ADDR, [255, 255, 255, 0]);
    while let Some(m) = ifq_dequeue(&ifp.if_snd) {
        m_freem(m);
    }
    REVARP_IFIDX.store(0, Ordering::Relaxed);
    REVARP_FINISHED.store(false, Ordering::Relaxed);
    REVARP_MYIP.store(0, Ordering::Relaxed);
    REVARP_SRVIP.store(0, Ordering::Relaxed);

    // The request: broadcast, from us, asking for the address of our own hardware address.
    revarprequest(ifp);
    let req = ifq_dequeue(&ifp.if_snd).expect("RARP request");
    let b = bytes(req);
    m_freem(req);
    assert_eq!(&b[0..6], &[0xff; 6], "to everyone");
    assert_eq!(&b[6..12], &OURS);
    assert_eq!(&b[12..14], &ETHERTYPE_REVARP.to_be_bytes());
    assert_eq!(
        &b[14..20],
        &[0, 1, 8, 0, 6, 4],
        "ethernet, ip, 6 and 4 bytes"
    );
    assert_eq!(&b[20..22], &ARPOP_REVREQUEST.to_be_bytes());
    assert_eq!(&b[22..28], &OURS, "sender hardware address");
    assert_eq!(&b[28..32], &[0; 4]);
    assert_eq!(&b[32..38], &OURS, "target hardware address");
    assert_eq!(&b[38..42], &[0; 4]);

    let deliver = |tha: [u8; 6]| {
        ether_input(
            ifp,
            frame(
                ifp,
                OURS,
                ETHERTYPE_REVARP,
                &arp_packet(ARPOP_REVREPLY, tha, ADDR),
            ),
            None,
        );
    };
    // Nobody is waiting: the reply is dropped.
    deliver(OURS);
    assert!(!REVARP_FINISHED.load(Ordering::Relaxed));

    // A request is out on this interface; a reply for somebody else is not ours.
    REVARP_IFIDX.store(ifp.if_index.get(), Ordering::Relaxed);
    deliver(PEER);
    assert!(!REVARP_FINISHED.load(Ordering::Relaxed));
    // One that came in on another interface is not either.
    REVARP_IFIDX.store(ifp.if_index.get() + 1, Ordering::Relaxed);
    deliver(OURS);
    assert!(!REVARP_FINISHED.load(Ordering::Relaxed));

    // Ours: the server (the sender's protocol address) and our address (the target's) are kept.
    REVARP_IFIDX.store(ifp.if_index.get(), Ordering::Relaxed);
    deliver(OURS);
    assert!(REVARP_FINISHED.load(Ordering::Relaxed));
    assert_eq!(REVARP_SRVIP.load(Ordering::Relaxed).to_ne_bytes(), GATEWAY);
    assert_eq!(REVARP_MYIP.load(Ordering::Relaxed).to_ne_bytes(), ADDR);

    // A later reply does not change what was taken.
    REVARP_MYIP.store(0, Ordering::Relaxed);
    deliver(OURS);
    assert_eq!(REVARP_MYIP.load(Ordering::Relaxed), 0);

    // Asking again after an answer is an error.
    REVARP_IFIDX.store(0, Ordering::Relaxed);
    assert_eq!(revarpwhoarewe(ifp).err(), Some(Errno::EIO));
    assert_eq!(revarpwhoami(ifp).err(), Some(Errno::EIO));

    REVARP_FINISHED.store(false, Ordering::Relaxed);
    REVARP_MYIP.store(0, Ordering::Relaxed);
    REVARP_SRVIP.store(0, Ordering::Relaxed);
    unconfigure(ifp, ADDR);
    while let Some(m) = ifq_dequeue(&ifp.if_snd) {
        m_freem(m);
    }
}
