//! Host tests for the neighbor cache: option parsing, the address printer, and the
//! reachability state machine of one entry driven by `nd6_resolve`, an advertisement
//! (`nd6_na_cache`) and `nd6_timer`. The entry is a host route built by hand: the inet6
//! routing table only exists once `inet6domain` is registered.

use std::boxed::Box;
use std::string::ToString;
use std::sync::MutexGuard;
use std::vec::Vec;

use super::*;
use crate::kern::kern_synch::refcnt_init;
use crate::kern::kern_tc::TIME_UPTIME;
use crate::kern::uipc_mbuf::m_freem;
use crate::net::if_::tests::{test_packet, zeroed_static};
use crate::net::if_::{IFF_RUNNING, IFF_UP};
use crate::net::route::RTF_HOST;
use crate::netinet::icmp6::Icmp6statCounters;
use crate::netinet::ip_input::tests::{PEER, bytes, setup, test_ether};
use crate::netinet6::icmp6::ICMP6COUNTERS;
use crate::netinet6::nd6_nbr::nd6_na_cache;
use crate::sys::mbuf::mq_len;

/// `fd00:77::1`, our address in the tests.
pub(crate) const OURS6: In6Addr =
    In6Addr::new([0xfd, 0, 0, 0x77, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 1]);
/// `fd00:77::2`, the neighbor's.
pub(crate) const PEER6: In6Addr =
    In6Addr::new([0xfd, 0, 0, 0x77, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 2]);

/// The value of an ICMPv6 counter.
pub(crate) fn icmp6stat(c: Icmp6statCounters) -> u64 {
    ICMP6COUNTERS[c as usize].load(Ordering::Relaxed)
}

/// The packets [`capture_output`] took: the destination's family and the bytes.
pub(crate) static SENT: std::sync::Mutex<Vec<(u8, Vec<u8>)>> = std::sync::Mutex::new(Vec::new());

/// An `if_output` that keeps what it is given in [`SENT`]: `ether_output` has no
/// `AF_INET6` case yet.
///
/// # Safety
///
/// `IfOutputFn`'s contract.
unsafe fn capture_output(
    _ifp: &'static Ifnet,
    m: &'static Mbuf,
    dst: *const Sockaddr,
    _rt: Option<&'static Rtentry>,
) -> Result<(), Errno> {
    // SAFETY: the caller's contract.
    let af = unsafe { (*dst).sa_family };
    SENT.lock()
        .unwrap_or_else(|e| e.into_inner())
        .push((af, bytes(m)));
    m_freem(m);
    Ok(())
}

/// Takes the packets sent so far.
pub(crate) fn take_sent() -> Vec<(u8, Vec<u8>)> {
    core::mem::take(&mut *SENT.lock().unwrap_or_else(|e| e.into_inner()))
}

/// The network setup of the IPv4 tests, the ND state, and an Ethernet interface that is
/// up, with its ND information.
pub(crate) fn nd6_setup() -> (
    (MutexGuard<'static, ()>, MutexGuard<'static, ()>),
    &'static Ifnet,
) {
    let g = setup();
    nd6_init();
    let ifp = test_ether();
    ifp.if_flags.set(ifp.if_flags.get() | IFF_UP | IFF_RUNNING);
    nd6_ifattach(ifp);
    ifp.if_output.set(Some(capture_output));
    // `ip6_output` drops (EMSGSIZE) a packet for an MTU below IPV6_MMTU: the solicitations
    // the tests trigger stop there, because `if_output_tso` has no AF_INET6 case yet
    // (phase 2 of the INET6 port) and would panic.
    ifp.if_mtu.set(0);
    let _ = take_sent();
    (g, ifp)
}

/// An inet6 address `addr` of `ifp` (not on its list), referenced once.
pub(crate) fn fake_ifa6(ifp: &'static Ifnet, addr: In6Addr) -> &'static In6Ifaddr {
    // SAFETY: the all-zero `In6Ifaddr` is valid (`malloc(M_ZERO)` in C).
    let ia: &'static In6Ifaddr = unsafe { zeroed_static() };
    ia.ia_addr.set(SockaddrIn6::with_addr(addr));
    ia.ia_ifa.ifa_addr.set(ia.ia_addr.as_ptr().cast());
    ia.ia_ifa.ifa_ifp.set(Some(ifp));
    refcnt_init(&ia.ia_ifa.ifa_refcnt);
    ia
}

/// A host route to `dst` on `ifp` with an empty link-layer gateway, as `rtrequest` clones
/// it from an on-link prefix, then given its neighbor cache entry by `nd6_rtrequest`.
fn fake_neighbor(ifp: &'static Ifnet, dst: In6Addr) -> &'static Rtentry {
    // SAFETY: the all-zero `Rtentry` is valid (cells, counters, empty lists).
    let rt: &'static Rtentry = unsafe { zeroed_static() };
    let key: &'static mut SockaddrIn6 = Box::leak(Box::new(SockaddrIn6::with_addr(dst)));
    let gate: &'static mut SockaddrDl = Box::leak(Box::new(SockaddrDl {
        sdl_len: size_of::<SockaddrDl>() as u8,
        sdl_family: AF_LINK,
        ..SockaddrDl::default()
    }));
    rt.rt_dest.set(ptr::from_mut(key).cast());
    rt.rt_gateway.set(ptr::from_mut(gate).cast());
    rt.rt_flags.set(RTF_HOST);
    rt.rt_ifidx.set(ifp.if_index.get());
    rt.rt_ifa.set(Some(&fake_ifa6(ifp, OURS6).ia_ifa));
    // One reference that is never dropped: the route is not the pool's.
    refcnt_init(&rt.rt_refcnt);

    net_lock();
    nd6_rtrequest(ifp, i32::from(RTM_RESOLVE), rt);
    net_unlock();
    assert_ne!(rt.rt_flags.get() & RTF_LLINFO, 0);
    rt
}

/// The link-layer address the entry holds, if resolved.
fn entry_lladdr(rt: &Rtentry) -> Option<[u8; 6]> {
    let sdl = satosdl(rt.rt_gateway.get());
    // SAFETY: the test's gateway is a `sockaddr_dl`.
    unsafe {
        ((*sdl).sdl_alen == 6).then(|| {
            let mut a = [0u8; 6];
            ptr::copy_nonoverlapping(lladdr(sdl), a.as_mut_ptr(), 6);
            a
        })
    }
}

/// An IPv6 packet from us to the neighbor (header only).
fn packet_to_peer() -> &'static Mbuf {
    let mut ip6 = Ip6Hdr::zeroed();
    ip6.set_ip6_vfc(0x60);
    ip6.ip6_hlim = 64;
    ip6.ip6_src = OURS6;
    ip6.ip6_dst = PEER6;
    // SAFETY: `Ip6Hdr` is 40 bytes of integers without padding.
    let b: [u8; 40] = unsafe { core::mem::transmute(ip6) };
    test_packet(&b)
}

#[test]
#[ignore = "needs OPENBSD_SRC (just test-ref)"]
fn values_match_the_c_header() {
    let defs = crate::reftest::defines("sys/netinet6/nd6.h");
    let ll = crate::reftest::assert_defines!(defs;
        ND6_LLINFO_PURGE, ND6_LLINFO_NOSTATE, ND6_LLINFO_INCOMPLETE, ND6_LLINFO_REACHABLE,
        ND6_LLINFO_STALE, ND6_LLINFO_DELAY, ND6_LLINFO_PROBE, ND6_INFINITE_LIFETIME);
    crate::reftest::assert_complete(&defs, "ND6_", &ll);
    crate::reftest::assert_defines!(defs;
        MAX_RTR_SOLICITATION_DELAY, RTR_SOLICITATION_INTERVAL, MAX_RTR_SOLICITATIONS,
        LN_HOLD_QUEUE, LN_HOLD_TOTAL, REACHABLE_TIME, RETRANS_TIMER, MIN_RANDOM_FACTOR,
        MAX_RANDOM_FACTOR);
}

#[test]
fn nd6_options_finds_the_link_layer_addresses() {
    // A source option, an MTU option, a second source option (ignored), a target option.
    let opt: [u8; 32] = [
        1, 1, 0x52, 0x55, 0x0a, 0, 2, 2, // source link-layer address
        5, 1, 0, 0, 0, 0, 0x05, 0xdc, // MTU
        1, 1, 1, 2, 3, 4, 5, 6, // duplicate source: the first one wins
        2, 1, 0x52, 0x54, 0, 0x12, 0x34, 0x56, // target link-layer address
    ];
    let mut ndopts = NdOpts::default();
    assert!(nd6_options(&opt, &mut ndopts));
    let src = ndopts.nd_opts_src_lladdr.expect("source option");
    let tgt = ndopts.nd_opts_tgt_lladdr.expect("target option");
    // SAFETY: the options are inside `opt`, alive.
    let ((sl, sn), (tl, tn)) = unsafe { (nd6_opt_lladdr(src), nd6_opt_lladdr(tgt)) };
    assert_eq!((&sl[..6], sn), (&PEER[..], 8));
    assert_eq!((&tl[..6], tn), (&[0x52, 0x54, 0, 0x12, 0x34, 0x56][..], 8));

    // No options at all is valid.
    assert!(nd6_options(&[], &mut ndopts));
    assert_eq!(ndopts, NdOpts::default());
}

#[test]
fn nd6_options_rejects_bad_lengths() {
    let bad = || icmp6stat(Icmp6statCounters::Icp6sNdBadopt);
    let mut ndopts = NdOpts::default();
    for opt in [
        &[1u8, 0, 0, 0, 0, 0, 0, 0][..],     // zero-length option
        &[1, 1, 0, 0, 0, 0][..],             // option overruns the buffer (truncated)
        &[1][..],                            // no room for nd_opt_len
        &[1, 1, 0, 0, 0, 0, 0, 0, 2][..],    // a trailing byte after a good option
        &[1, 2, 0, 0, 0, 0, 0, 0, 0, 0][..], // length says 16, 10 bytes there
    ] {
        let before = bad();
        assert!(!nd6_options(opt, &mut ndopts), "{opt:?}");
        assert_eq!(ndopts, NdOpts::default(), "cleared on error");
        assert!(bad() > before);
    }

    // More than nd6_maxndopt options: parsing stops, the message is still valid.
    let many = [[3u8, 1, 0, 0, 0, 0, 0, 0]; 12].concat();
    let before = icmp6stat(Icmp6statCounters::Icp6sNdToomanyopt);
    assert!(nd6_options(&many, &mut ndopts));
    assert!(icmp6stat(Icmp6statCounters::Icp6sNdToomanyopt) > before);
}

#[test]
fn in6_ntop_writes_what_inet_ntop_writes() {
    let a = |s: [u16; 8]| {
        let mut b = [0u8; 16];
        for (i, w) in s.iter().enumerate() {
            b[2 * i..2 * i + 2].copy_from_slice(&w.to_be_bytes());
        }
        In6Ntop(In6Addr::new(b)).to_string()
    };
    assert_eq!(a([0; 8]), "::");
    assert_eq!(a([0, 0, 0, 0, 0, 0, 0, 1]), "::1");
    assert_eq!(a([0xfe80, 0, 0, 0, 0, 0, 0, 1]), "fe80::1");
    assert_eq!(a([0xfd00, 0x77, 0, 0, 0, 0, 0, 2]), "fd00:77::2");
    assert_eq!(a([0xff02, 0, 0, 0, 0, 1, 0xff00, 2]), "ff02::1:ff00:2");
    assert_eq!(a([1, 0, 0, 2, 0, 0, 0, 3]), "1:0:0:2::3");
    assert_eq!(a([1, 0, 0, 2, 0, 0, 3, 4]), "1::2:0:0:3:4");
    assert_eq!(a([1, 0, 2, 3, 4, 5, 6, 7]), "1:0:2:3:4:5:6:7");
    assert_eq!(a([1, 2, 3, 4, 5, 6, 7, 0]), "1:2:3:4:5:6:7:0");
    assert_eq!(a([1, 2, 3, 4, 5, 0, 0, 0]), "1:2:3:4:5::");
    assert_eq!(
        a([0, 0, 0, 0, 0, 0xffff, 0x0a00, 0x0201]),
        "::ffff:10.0.2.1"
    );
    assert_eq!(a([0, 0, 0, 0, 0, 0, 0x0a00, 0x0201]), "::10.0.2.1");
}

#[test]
fn an_entry_walks_incomplete_reachable_stale_delay_probe() {
    let (_g, ifp) = nd6_setup();
    let rt = fake_neighbor(ifp, PEER6);
    let ln = rt_ln(rt).expect("llinfo");
    assert_eq!(ln.ln_state.get(), ND6_LLINFO_NOSTATE);
    let held = ln_hold_total.load(Ordering::Relaxed);
    let dst = SockaddrIn6::with_addr(PEER6);
    let mut desten = [0u8; ETHER_ADDR_LEN];

    // A packet to the neighbor: held, the entry INCOMPLETE, a solicitation sent.
    net_lock();
    // SAFETY: a local `sockaddr_in6`.
    let r = unsafe {
        nd6_resolve(
            ifp,
            Some(rt),
            packet_to_peer(),
            sin6tosa_const(&dst),
            &mut desten,
        )
    };
    net_unlock();
    assert_eq!(r, Err(Errno::EAGAIN));
    assert_eq!(ln.ln_state.get(), ND6_LLINFO_INCOMPLETE);
    assert_eq!(ln.ln_asked.get(), 1);
    assert_eq!(ln.ln_saddr6.get(), OURS6, "the prompting packet's source");
    assert_eq!(mq_len(&ln.ln_mq), 1);
    assert_eq!(ln_hold_total.load(Ordering::Relaxed), held + 1);
    let uptime = TIME_UPTIME.load(Ordering::Relaxed);
    assert_eq!(rt.rt_expire().get(), uptime + 1, "RETRANS_TIMER");

    // The solicited advertisement: REACHABLE for the interface's reachable time, the held
    // packet sent.
    net_lock();
    nd6_na_cache(
        ifp,
        rt,
        Some(&PEER),
        false,
        true,
        true,
        false,
        &PEER6,
        &PEER6,
    );
    net_unlock();
    assert_eq!(ln.ln_state.get(), ND6_LLINFO_REACHABLE);
    assert_eq!(entry_lladdr(rt), Some(PEER));
    assert_eq!(ln.ln_asked.get(), 0);
    assert_eq!(mq_len(&ln.ln_mq), 0, "the hold queue was flushed");
    let sent = take_sent();
    let held_packet = sent
        .iter()
        .find(|(af, _)| *af == AF_INET6)
        .expect("held packet sent");
    assert_eq!(&held_packet.1[24..40], &PEER6.s6_addr, "to the neighbor");
    assert_eq!(ln_hold_total.load(Ordering::Relaxed), held);
    let reachable = if_nd(ifp).expect("if_nd").get().reachable;
    assert!((14..=44).contains(&reachable), "ND_COMPUTE_RTIME(30000)");
    assert_eq!(rt.rt_expire().get(), uptime + i64::from(reachable));

    // Its timer runs out: STALE for a day.
    let t = rt.rt_expire().get();
    TIME_UPTIME.store(t, Ordering::Relaxed);
    nd6_timer(ptr::null_mut());
    assert_eq!(ln.ln_state.get(), ND6_LLINFO_STALE);
    assert_eq!(rt.rt_expire().get(), t + i64::from(ND6_GCTIMER));

    // A packet to a STALE neighbor goes out at once and starts the DELAY timer.
    let m = packet_to_peer();
    net_lock();
    // SAFETY: a local `sockaddr_in6`.
    let r = unsafe { nd6_resolve(ifp, Some(rt), m, sin6tosa_const(&dst), &mut desten) };
    net_unlock();
    assert_eq!(r, Ok(()));
    m_freem(m);
    assert_eq!(desten, PEER);
    assert_eq!(ln.ln_state.get(), ND6_LLINFO_DELAY);
    assert_eq!(
        rt.rt_expire().get(),
        t + i64::from(ND6_DELAY.load(Ordering::Relaxed))
    );

    // No confirmation within the delay: PROBE with unicast solicitations, one a second.
    let t = rt.rt_expire().get();
    TIME_UPTIME.store(t, Ordering::Relaxed);
    nd6_timer(ptr::null_mut());
    assert_eq!(ln.ln_state.get(), ND6_LLINFO_PROBE);
    assert_eq!(ln.ln_asked.get(), 1);
    TIME_UPTIME.store(t + 1, Ordering::Relaxed);
    nd6_timer(ptr::null_mut());
    assert_eq!(ln.ln_state.get(), ND6_LLINFO_PROBE);
    assert_eq!(ln.ln_asked.get(), 2);

    // An unsolicited advertisement with the same address and no override changes nothing;
    // a solicited one makes the entry REACHABLE again.
    net_lock();
    nd6_na_cache(
        ifp,
        rt,
        Some(&PEER),
        false,
        false,
        false,
        false,
        &PEER6,
        &PEER6,
    );
    assert_eq!(ln.ln_state.get(), ND6_LLINFO_PROBE);
    nd6_na_cache(
        ifp,
        rt,
        Some(&PEER),
        false,
        true,
        false,
        false,
        &PEER6,
        &PEER6,
    );
    assert_eq!(ln.ln_state.get(), ND6_LLINFO_REACHABLE);
    // A new address without override: REACHABLE becomes STALE, the address is kept.
    nd6_na_cache(
        ifp,
        rt,
        Some(&[1, 2, 3, 4, 5, 6]),
        false,
        false,
        false,
        false,
        &PEER6,
        &PEER6,
    );
    assert_eq!(ln.ln_state.get(), ND6_LLINFO_STALE);
    assert_eq!(entry_lladdr(rt), Some(PEER));
    // With override it is recorded.
    nd6_na_cache(
        ifp,
        rt,
        Some(&[1, 2, 3, 4, 5, 6]),
        false,
        false,
        true,
        false,
        &PEER6,
        &PEER6,
    );
    assert_eq!(entry_lladdr(rt), Some([1, 2, 3, 4, 5, 6]));

    // The route goes away: the entry leaves the list.
    nd6_rtrequest(ifp, i32::from(RTM_DELETE), rt);
    net_unlock();
    assert!(rt_ln(rt).is_none());
    assert_eq!(rt.rt_flags.get() & RTF_LLINFO, 0);
    assert!(ND6_LIST.0.is_empty());
}

#[test]
fn a_neighbor_that_never_answers_gets_its_packets_dropped() {
    let (_g, ifp) = nd6_setup();
    let rt = fake_neighbor(ifp, PEER6);
    let ln = rt_ln(rt).expect("llinfo");
    let held = ln_hold_total.load(Ordering::Relaxed);
    let dst = SockaddrIn6::with_addr(PEER6);
    let mut desten = [0u8; ETHER_ADDR_LEN];

    net_lock();
    for _ in 0..3 {
        // SAFETY: a local `sockaddr_in6`.
        let r = unsafe {
            nd6_resolve(
                ifp,
                Some(rt),
                packet_to_peer(),
                sin6tosa_const(&dst),
                &mut desten,
            )
        };
        assert_eq!(r, Err(Errno::EAGAIN));
    }
    net_unlock();
    assert_eq!(mq_len(&ln.ln_mq), 3);
    assert_eq!(
        ln.ln_asked.get(),
        1,
        "one solicitation for the three packets"
    );

    // nd6_mmaxtries solicitations in all, one per second.
    for asked in 2..=3 {
        TIME_UPTIME.store(rt.rt_expire().get(), Ordering::Relaxed);
        nd6_timer(ptr::null_mut());
        assert_eq!(ln.ln_asked.get(), asked);
        assert_eq!(ln.ln_state.get(), ND6_LLINFO_INCOMPLETE);
    }

    // Then the held packets are dropped (with an ICMPv6 unreachable each) and the entry is
    // invalidated.
    net_lock();
    assert!(
        nd6_llinfo_timer(rt, false),
        "the route should no longer be used"
    );
    net_unlock();
    assert_eq!(mq_len(&ln.ln_mq), 0);
    assert_eq!(ln_hold_total.load(Ordering::Relaxed), held);
    assert_eq!(ln.ln_asked.get(), 0);

    net_lock();
    nd6_rtrequest(ifp, i32::from(RTM_DELETE), rt);
    net_unlock();
    assert!(ND6_LIST.0.is_empty());
}
