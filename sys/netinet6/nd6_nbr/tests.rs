//! Host tests for neighbor solicitation/advertisement input on crafted packets (the checks
//! before any table lookup) and for Duplicate Address Detection's counting.

use std::vec::Vec;

use super::*;
use crate::kern::uipc_mbuf::m_freem;
use crate::net::if_::tests::test_packet;
use crate::netinet6::in6::IN6ADDR_ANY;
use crate::netinet6::in6_var::In6Ifaddr;
use crate::netinet6::nd6::tests::{OURS6, PEER6, fake_ifa6, icmp6stat, nd6_setup};

/// An ICMPv6 ND packet from `src` to `dst` with hop limit `hlim`: the fixed part `msg`
/// (type, code, checksum, reserved/flags, target) and the options `opts`.
fn nd_packet(
    ifp: &Ifnet,
    src: In6Addr,
    dst: In6Addr,
    hlim: u8,
    msg: &[u8],
    opts: &[u8],
) -> (&'static Mbuf, i32) {
    let mut ip6 = Ip6Hdr::zeroed();
    ip6.set_ip6_vfc(IPV6_VERSION);
    ip6.ip6_plen = htons((msg.len() + opts.len()) as u16);
    ip6.ip6_nxt = IPPROTO_ICMPV6 as u8;
    ip6.ip6_hlim = hlim;
    ip6.ip6_src = src;
    ip6.ip6_dst = dst;
    // SAFETY: `Ip6Hdr` is 40 bytes of integers without padding.
    let hdr: [u8; 40] = unsafe { core::mem::transmute(ip6) };
    let mut b: Vec<u8> = hdr.to_vec();
    b.extend_from_slice(msg);
    b.extend_from_slice(opts);
    let m = test_packet(&b);
    m.m_pkthdr().ph_ifidx.set(ifp.if_index.get());
    (m, (msg.len() + opts.len()) as i32)
}

/// The fixed part of a solicitation (`flags` 0) or advertisement for `target`.
fn nd_msg(type_: u8, flags: u32, target: In6Addr) -> Vec<u8> {
    let mut b = std::vec![type_, 0, 0, 0];
    b.extend_from_slice(&flags.to_ne_bytes());
    b.extend_from_slice(&target.s6_addr);
    b
}

/// `ff02::1:ff00:1`, the solicited-node group of `OURS6`.
fn solicited_node() -> In6Addr {
    let mut a = In6Addr::new([0xff, 2, 0, 0, 0, 0, 0, 0, 0, 0, 0, 1, 0xff, 0, 0, 1]);
    a.s6_addr[13..].copy_from_slice(&OURS6.s6_addr[13..]);
    a
}

/// A crafted solicitation: source, destination, hop limit, fixed part, options.
type NsCase<'a> = (In6Addr, In6Addr, u8, Vec<u8>, &'a [u8]);

#[test]
fn bad_solicitations_are_counted_and_dropped() {
    let (_g, ifp) = nd6_setup();
    let badns = || icmp6stat(Icmp6statCounters::Icp6sBadns);
    let badopt = || icmp6stat(Icmp6statCounters::Icp6sNdBadopt);
    let ns = nd_msg(ND_NEIGHBOR_SOLICIT, 0, OURS6);
    let lla = [ND_OPT_SOURCE_LINKADDR, 1, 0x52, 0x55, 0x0a, 0, 2, 2];

    let cases: [NsCase<'_>; 4] = [
        // the hop limit must be 255
        (IN6ADDR_ANY, solicited_node(), 254, ns.clone(), &[]),
        // from ::, to something other than a solicited-node group
        (IN6ADDR_ANY, OURS6, 255, ns.clone(), &[]),
        // a multicast target
        (
            IN6ADDR_ANY,
            solicited_node(),
            255,
            nd_msg(ND_NEIGHBOR_SOLICIT, 0, solicited_node()),
            &[],
        ),
        // a DAD probe (from ::) must not carry a source link-layer address
        (IN6ADDR_ANY, solicited_node(), 255, ns.clone(), &lla),
    ];
    for (i, (src, dst, hlim, msg, opts)) in cases.iter().enumerate() {
        let before = badns();
        let (m, len) = nd_packet(ifp, *src, *dst, *hlim, msg, opts);
        nd6_ns_input(m, 40, len);
        assert_eq!(badns(), before + 1, "case {i}");
    }

    // A zero-length option: dropped by nd6_options, counted there.
    let (before, before_opt) = (badns(), badopt());
    let (m, len) = nd_packet(
        ifp,
        IN6ADDR_ANY,
        solicited_node(),
        255,
        &ns,
        &[1, 0, 0, 0, 0, 0, 0, 0],
    );
    nd6_ns_input(m, 40, len);
    assert_eq!((badns(), badopt()), (before, before_opt + 1));

    // Shorter than a solicitation.
    let before = icmp6stat(Icmp6statCounters::Icp6sTooshort);
    let (m, _) = nd_packet(ifp, IN6ADDR_ANY, solicited_node(), 255, &ns[..20], &[]);
    nd6_ns_input(m, 40, 20);
    assert_eq!(icmp6stat(Icmp6statCounters::Icp6sTooshort), before + 1);
}

#[test]
fn bad_advertisements_are_counted_and_dropped() {
    let (_g, ifp) = nd6_setup();
    let badna = || icmp6stat(Icmp6statCounters::Icp6sBadna);
    let all_nodes = IN6ADDR_LINKLOCAL_ALLNODES;
    let tlla = [ND_OPT_TARGET_LINKADDR, 1, 0x52, 0x55, 0x0a, 0, 2, 2];

    let cases: [(In6Addr, u8, Vec<u8>, &[u8]); 4] = [
        // the hop limit must be 255
        (
            OURS6,
            64,
            nd_msg(ND_NEIGHBOR_ADVERT, ND_NA_FLAG_SOLICITED, PEER6),
            &tlla,
        ),
        // a multicast target
        (OURS6, 255, nd_msg(ND_NEIGHBOR_ADVERT, 0, all_nodes), &tlla),
        // solicited, to a multicast group
        (
            all_nodes,
            255,
            nd_msg(ND_NEIGHBOR_ADVERT, ND_NA_FLAG_SOLICITED, PEER6),
            &tlla,
        ),
        // to a multicast group without the target link-layer address
        (
            all_nodes,
            255,
            nd_msg(ND_NEIGHBOR_ADVERT, ND_NA_FLAG_OVERRIDE, PEER6),
            &[],
        ),
    ];
    net_lock();
    for (i, (dst, hlim, msg, opts)) in cases.iter().enumerate() {
        let before = badna();
        let (m, len) = nd_packet(ifp, PEER6, *dst, *hlim, msg, opts);
        nd6_na_input(m, 40, len);
        assert_eq!(badna(), before + 1, "case {i}");
    }

    // A good unsolicited advertisement for a neighbor we have no entry for (a host): ignored,
    // not counted as bad.
    let before = badna();
    let (m, len) = nd_packet(
        ifp,
        PEER6,
        all_nodes,
        255,
        &nd_msg(ND_NEIGHBOR_ADVERT, ND_NA_FLAG_OVERRIDE, PEER6),
        &tlla,
    );
    nd6_na_input(m, 40, len);
    assert_eq!(badna(), before);
    net_unlock();
}

/// A tentative address `OURS6` on `ifp`, and the DAD state `nd6_dad_start` made for it.
fn start_dad(ifp: &'static Ifnet) -> (&'static In6Ifaddr, &'static Dadq) {
    let ia = fake_ifa6(ifp, OURS6);
    ia.ia6_flags.set(IN6_IFF_TENTATIVE);
    net_lock();
    nd6_dad_start(&ia.ia_ifa);
    net_unlock();
    let dp = nd6_dad_find(&ia.ia_ifa).expect("dadq");
    (ia, dp)
}

/// Frees a `dadq` that `nd6_dad_destroy` left to its reaper (no softclock runs here).
fn reap(dp: &'static Dadq) {
    let _ = timeout_del(&dp.dad_timer_ch);
    nd6_dad_reaper(ptr::from_ref(dp).cast_mut().cast());
}

#[test]
fn dad_without_an_answer_makes_the_address_usable() {
    let (_g, ifp) = nd6_setup();
    let pending = IP6_DAD_PENDING.load(Ordering::Relaxed);
    let (ia, dp) = start_dad(ifp);
    assert_eq!(IP6_DAD_PENDING.load(Ordering::Relaxed), pending + 1);
    assert_eq!(dp.dad_count.get(), IP6_DAD_COUNT.load(Ordering::Relaxed));
    assert_eq!((dp.dad_ns_tcount.get(), dp.dad_ns_ocount.get()), (1, 1));
    assert_eq!(
        ia.ia_ifa.ifa_refcnt.r_refs.load(Ordering::Relaxed),
        2,
        "dadq's reference"
    );

    // A second start for the same address does nothing.
    net_lock();
    nd6_dad_start(&ia.ia_ifa);
    net_unlock();
    assert_eq!(IP6_DAD_PENDING.load(Ordering::Relaxed), pending + 1);

    nd6_dad_timer(ptr::from_ref(&ia.ia_ifa).cast_mut().cast());
    assert_eq!(
        ia.ia6_flags.get() & (IN6_IFF_TENTATIVE | IN6_IFF_DUPLICATED),
        0
    );
    assert!(nd6_dad_find(&ia.ia_ifa).is_none());
    assert_eq!(IP6_DAD_PENDING.load(Ordering::Relaxed), pending);
    reap(dp);
    assert_eq!(ia.ia_ifa.ifa_refcnt.r_refs.load(Ordering::Relaxed), 1);
}

#[test]
fn a_dad_probe_from_someone_else_marks_the_address_duplicated() {
    let (_g, ifp) = nd6_setup();
    let (ia, dp) = start_dad(ifp);

    // Another node probes for the same address after our probe: counted, decided at the
    // timer.
    net_lock();
    nd6_dad_ns_input(Some(&ia.ia_ifa));
    net_unlock();
    assert_eq!(dp.dad_ns_icount.get(), 1);
    assert_ne!(ia.ia6_flags.get() & IN6_IFF_TENTATIVE, 0);
    nd6_dad_timer(ptr::from_ref(&ia.ia_ifa).cast_mut().cast());
    assert_eq!(ia.ia6_flags.get() & IN6_IFF_TENTATIVE, 0);
    assert_ne!(ia.ia6_flags.get() & IN6_IFF_DUPLICATED, 0);
    assert!(nd6_dad_find(&ia.ia_ifa).is_none());
    reap(dp);

    // Down interface: no probe goes out (only the try counts); a probe from someone else
    // then wins at once.
    ifp.if_flags.set(ifp.if_flags.get() & !IFF_UP);
    let (ia, dp) = start_dad(ifp);
    assert_eq!((dp.dad_ns_tcount.get(), dp.dad_ns_ocount.get()), (1, 0));
    net_lock();
    nd6_dad_ns_input(Some(&ia.ia_ifa));
    net_unlock();
    assert_ne!(ia.ia6_flags.get() & IN6_IFF_DUPLICATED, 0);
    assert!(nd6_dad_find(&ia.ia_ifa).is_none());
    reap(dp);

    // An anycast address skips DAD.
    let ia = fake_ifa6(ifp, PEER6);
    ia.ia6_flags.set(IN6_IFF_TENTATIVE | IN6_IFF_ANYCAST);
    net_lock();
    nd6_dad_start(&ia.ia_ifa);
    net_unlock();
    assert_eq!(ia.ia6_flags.get(), IN6_IFF_ANYCAST);
    assert!(nd6_dad_find(&ia.ia_ifa).is_none());
}

#[test]
fn nd6_ifptomac_is_the_ethernet_address() {
    let (_g, ifp) = nd6_setup();
    assert_eq!(
        nd6_ifptomac(ifp),
        Some(crate::netinet::ip_input::tests::OURS)
    );
    let m = nd_packet(ifp, OURS6, PEER6, 255, &[], &[]).0;
    m_freem(m);
}
