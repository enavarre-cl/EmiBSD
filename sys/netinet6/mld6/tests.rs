//! Host tests for MLD: the message field macros, and the listener's timer arithmetic and state
//! transitions on a crafted query, a report and the fast timeout.

use std::sync::MutexGuard;
use std::vec::Vec;

use super::*;
use crate::net::if_::tests::test_packet;
use crate::netinet::icmp6::Icmp6statCounters;
use crate::netinet::ip_input::tests::setup as setup_ip;
use crate::netinet::ip6::Ip6Hdr;
use crate::netinet6::icmp6::{ICMP6COUNTERS, icmp6_init};
use crate::netinet6::in6::tests::{a6, test_ia6, test_if};
use crate::netinet6::in6::{IN6ADDR_LINKLOCAL_ALLNODES, in6_addmulti, in6_is_addr_mc_linklocal};
use crate::netinet6::nd6::{nd6_ifattach, nd6_init};
use crate::sys::errno::Errno;
use crate::sys::systm::{net_lock, net_unlock};

#[test]
fn query_fields() {
    // Maximum Response Code 0x7123: exponent 7, mantissa 0x123.
    assert_eq!(mld_mrc_exp(htons(0x7123)), 7);
    assert_eq!(mld_mrc_mant(htons(0x7123)), 0x123);
    // misc: resv 0xa, S 1, QRV 2.
    let misc = 0xa0 | 0x08 | 0x02;
    assert_eq!(
        (mld_qresv(misc), mld_sflag(misc), mld_qrv(misc)),
        (0xa, 1, 2)
    );
    assert_eq!((mld_qqic_exp(0x9c), mld_qqic_mant(0x9c)), (1, 0xc));
    assert_eq!(MLD_MINLEN, 8);
    assert_eq!(MLD_V2_QUERY_MINLEN, 28);
}

#[test]
#[ignore = "needs OPENBSD_SRC (just test-ref)"]
fn values_match_the_c_header() {
    let defs = crate::reftest::defines("sys/netinet6/mld6.h");
    crate::reftest::assert_defines!(defs;
        MLD_V2_REPORT_MAXRECS, MLD_DO_NOTHING, MLD_MODE_IS_INCLUDE, MLD_MODE_IS_EXCLUDE,
        MLD_CHANGE_TO_INCLUDE_MODE, MLD_CHANGE_TO_EXCLUDE_MODE, MLD_ALLOW_NEW_SOURCES,
        MLD_BLOCK_OLD_SOURCES, MLD_V2_GENERAL_QUERY, MLD_V2_GROUP_QUERY,
        MLD_V2_GROUP_SOURCE_QUERY, MLD_V1_MAX_RI, MLD_TIMER_SCALE);
}

/// An interface with a link-local address (which MLD reports come from) and the MLD state.
/// Its MTU is 0: `ip6_output` drops (EMSGSIZE) what it is given, because `if_output_tso` has no
/// AF_INET6 case yet (phase 2 of the INET6 port) and would panic.
fn mld_if() -> (
    (MutexGuard<'static, ()>, MutexGuard<'static, ()>),
    &'static Ifnet,
) {
    let g = setup_ip();
    nd6_init();
    icmp6_init();
    let ifp = test_if(b"tml0");
    ifp.if_ioctl.set(Some(accepting_ioctl));
    nd6_ifattach(ifp);
    test_ia6(ifp, ll(ifp), 0);
    ifp.if_mtu.set(0);
    MLD6_TIMERS_ARE_RUNNING.store(0, Ordering::Relaxed);
    (g, ifp)
}

/// `fe80::5054:ff:fe00:1` with the zone of `ifp` embedded.
fn ll(ifp: &Ifnet) -> In6Addr {
    let mut a = a6("fe80::5054:ff:fe00:1");
    a.set_s6_addr16(1, htons(ifp.if_index.get() as u16));
    a
}

/// A group address with `ifp`'s zone embedded when it has a link-local scope.
fn group(ifp: &Ifnet, s: &str) -> In6Addr {
    let mut g = a6(s);
    if in6_is_addr_mc_linklocal(&g) {
        g.set_s6_addr16(1, htons(ifp.if_index.get() as u16));
    }
    g
}

/// `ifp` joins `g`.
fn join(ifp: &'static Ifnet, g: In6Addr) -> &'static In6Multi {
    net_lock();
    let in6m = in6_addmulti(&g, ifp).expect("join");
    net_unlock();
    in6m
}

/// The count of MLD messages of `type_` sent.
fn sent(type_: u8) -> u64 {
    ICMP6COUNTERS[Icmp6statCounters::Icp6sOuthist as usize + usize::from(type_)]
        .load(Ordering::Relaxed)
}

fn tooshort() -> u64 {
    ICMP6COUNTERS[Icmp6statCounters::Icp6sTooshort as usize].load(Ordering::Relaxed)
}

/// A received MLD message `type_` from `src` for group `addr`, maximum response delay
/// `maxdelay` (in milliseconds), on `ifp`.
fn mld_message(
    ifp: &Ifnet,
    src: In6Addr,
    type_: u8,
    maxdelay: u16,
    addr: In6Addr,
) -> &'static Mbuf {
    let mut ip6 = Ip6Hdr::zeroed();
    ip6.set_ip6_vfc(IPV6_VERSION);
    ip6.ip6_plen = htons(size_of::<MldHdr>() as u16);
    ip6.ip6_nxt = IPPROTO_ICMPV6 as u8;
    ip6.ip6_hlim = 1;
    ip6.ip6_src = src;
    ip6.ip6_dst = IN6ADDR_LINKLOCAL_ALLNODES;
    let mut mldh = MldHdr {
        mld_icmp6_hdr: Icmp6Hdr::zeroed(),
        mld_addr: addr,
    };
    mldh.set_mld_type(type_);
    mldh.set_mld_maxdelay(htons(maxdelay));
    let mut b = std::vec![0u8; size_of::<Ip6Hdr>() + size_of::<MldHdr>()];
    // SAFETY: plain-data headers, no padding, copied into a buffer of their size.
    unsafe {
        ptr::copy_nonoverlapping(
            ptr::from_ref(&ip6).cast::<u8>(),
            b.as_mut_ptr(),
            size_of::<Ip6Hdr>(),
        );
        ptr::copy_nonoverlapping(
            ptr::from_ref(&mldh).cast::<u8>(),
            b.as_mut_ptr().add(size_of::<Ip6Hdr>()),
            size_of::<MldHdr>(),
        );
    }
    let m = test_packet(&b);
    m.m_pkthdr().ph_ifidx.set(ifp.if_index.get());
    m
}

#[test]
fn joining_a_group_starts_a_delayed_report() {
    let (_g, ifp) = mld_if();
    let reports = sent(MLD_LISTENER_REPORT);

    let g = group(ifp, "ff02::1234");
    let in6m = join(ifp, g);
    // The state says we reported last, the timer is a random delay of up to 10 seconds of
    // fast timeouts, and the first report went out with the join.
    assert_eq!(in6m.in6m_state.get(), MLD_IREPORTEDLAST);
    assert!((1..=MLD_V1_MAX_RI * PR_FASTHZ as u32).contains(&in6m.in6m_timer.get()));
    assert_eq!(MLD6_TIMERS_ARE_RUNNING.load(Ordering::Relaxed), 1);
    assert_eq!(sent(MLD_LISTENER_REPORT), reports + 1);

    // The all-nodes group, and the groups of interface-local and smaller scope, are never
    // reported.
    MLD6_TIMERS_ARE_RUNNING.store(0, Ordering::Relaxed);
    for s in ["ff02::1", "ff01::5", "ff01::1"] {
        let in6m = join(ifp, group(ifp, s));
        assert_eq!(in6m.in6m_state.get(), MLD_OTHERLISTENER, "{s}");
        assert_eq!(in6m.in6m_timer.get(), 0, "{s}");
    }
    assert_eq!(sent(MLD_LISTENER_REPORT), reports + 1);
    assert_eq!(MLD6_TIMERS_ARE_RUNNING.load(Ordering::Relaxed), 0);
}

#[test]
fn leaving_sends_done_only_if_we_reported_last() {
    let (_g, ifp) = mld_if();
    let g = group(ifp, "ff02::1234");
    let in6m = join(ifp, g);
    let all = join(ifp, group(ifp, "ff02::1"));
    let mut all_routers = IN6ADDR_LINKLOCAL_ALLROUTERS;
    all_routers.set_s6_addr16(1, htons(ifp.if_index.get() as u16));

    rw_enter_write(&ifp.if_maddrlock);
    // We reported last: a Done message to the all-routers group.
    let mut pkt = Mld6Pktinfo::default();
    mld6_stop_listening(in6m, ifp, &mut pkt);
    assert_eq!(
        pkt,
        Mld6Pktinfo {
            mpi_addr: all_routers,
            mpi_rdomain: ifp.if_rdomain.get(),
            mpi_ifidx: ifp.if_index.get(),
            mpi_type: i32::from(MLD_LISTENER_DONE),
        }
    );

    // Somebody else reported after us: nothing.
    in6m.in6m_state.set(MLD_OTHERLISTENER);
    let mut pkt = Mld6Pktinfo::default();
    mld6_stop_listening(in6m, ifp, &mut pkt);
    assert_eq!(pkt, Mld6Pktinfo::default());

    // The all-nodes group is never left loudly.
    all.in6m_state.set(MLD_IREPORTEDLAST);
    let mut pkt = Mld6Pktinfo::default();
    mld6_stop_listening(all, ifp, &mut pkt);
    assert_eq!(pkt, Mld6Pktinfo::default());
    rw_exit_write(&ifp.if_maddrlock);
}

#[test]
fn the_fast_timeout_counts_down_and_reports() {
    let (_g, ifp) = mld_if();
    let in6m = join(ifp, group(ifp, "ff02::1234"));
    let reports = sent(MLD_LISTENER_REPORT);

    // No timer running: nothing to do.
    in6m.in6m_timer.set(2);
    in6m.in6m_state.set(MLD_OTHERLISTENER);
    MLD6_TIMERS_ARE_RUNNING.store(0, Ordering::Relaxed);
    mld6_fasttimo();
    assert_eq!(
        in6m.in6m_timer.get(),
        2,
        "the shortcut says no timer is running"
    );

    // Two ticks: the first only counts, the second reports.
    MLD6_TIMERS_ARE_RUNNING.store(1, Ordering::Relaxed);
    mld6_fasttimo();
    assert_eq!(in6m.in6m_timer.get(), 1);
    assert_eq!(
        MLD6_TIMERS_ARE_RUNNING.load(Ordering::Relaxed),
        1,
        "still running"
    );
    assert_eq!(sent(MLD_LISTENER_REPORT), reports);
    mld6_fasttimo();
    assert_eq!(in6m.in6m_timer.get(), 0);
    assert_eq!(in6m.in6m_state.get(), MLD_IREPORTEDLAST);
    assert_eq!(sent(MLD_LISTENER_REPORT), reports + 1);
    assert_eq!(
        MLD6_TIMERS_ARE_RUNNING.load(Ordering::Relaxed),
        0,
        "idle again"
    );

    // checktimer on its own: the report is queued, not sent.
    in6m.in6m_timer.set(1);
    let mut pktlist = Vec::new();
    assert!(!mld6_checktimer(ifp, &mut pktlist));
    assert_eq!(pktlist.len(), 1);
    assert_eq!(pktlist[0].mpi_addr, in6m.in6m_addr());
    assert_eq!(pktlist[0].mpi_type, i32::from(MLD_LISTENER_REPORT));
}

#[test]
fn a_query_sets_the_timers_of_the_groups_it_asks_about() {
    let (_g, ifp) = mld_if();
    let a = join(ifp, group(ifp, "ff02::1234"));
    let b = join(ifp, group(ifp, "ff02::5678"));
    let all = join(ifp, group(ifp, "ff02::1"));
    let querier = ll(ifp);
    let reset = || {
        for x in [a, b] {
            x.in6m_timer.set(0);
            x.in6m_state.set(MLD_OTHERLISTENER);
        }
        MLD6_TIMERS_ARE_RUNNING.store(0, Ordering::Relaxed);
    };
    let query = |maxdelay: u16, g: In6Addr| {
        mld6_input(
            mld_message(ifp, querier, MLD_LISTENER_QUERY, maxdelay, g),
            40,
        );
    };

    // A general query (group ::) with a 10 s maximum response delay: 50 fast ticks at most.
    reset();
    query(10_000, IN6ADDR_ANY);
    for x in [a, b] {
        assert!((1..=50).contains(&x.in6m_timer.get()));
    }
    assert_eq!(all.in6m_timer.get(), 0, "never the all-nodes group");
    assert_eq!(MLD6_TIMERS_ARE_RUNNING.load(Ordering::Relaxed), 1);

    // A running timer longer than the response delay is restarted with a shorter one; a
    // shorter one is kept.
    a.in6m_timer.set(40);
    b.in6m_timer.set(1);
    query(1_000, IN6ADDR_ANY);
    assert!((1..=5).contains(&a.in6m_timer.get()));
    assert_eq!(b.in6m_timer.get(), 1);

    // A group-specific query only touches that group. 100 ms is less than a tick: 1.
    reset();
    query(100, group(ifp, "ff02::5678"));
    assert_eq!((a.in6m_timer.get(), b.in6m_timer.get()), (0, 1));

    // A maximum response delay of 0 asks for an answer at once.
    reset();
    let reports = sent(MLD_LISTENER_REPORT);
    query(0, IN6ADDR_ANY);
    assert_eq!((a.in6m_timer.get(), b.in6m_timer.get()), (0, 0));
    assert_eq!(a.in6m_state.get(), MLD_IREPORTEDLAST);
    assert_eq!(sent(MLD_LISTENER_REPORT), reports + 2, "one per group");
    assert_eq!(all.in6m_state.get(), MLD_OTHERLISTENER);
}

#[test]
fn queries_from_the_wrong_place_or_about_the_wrong_thing_are_ignored() {
    let (_g, ifp) = mld_if();
    let a = join(ifp, group(ifp, "ff02::1234"));
    a.in6m_timer.set(0);
    a.in6m_state.set(MLD_OTHERLISTENER);
    MLD6_TIMERS_ARE_RUNNING.store(0, Ordering::Relaxed);
    let untouched = |what: &str| {
        assert_eq!(a.in6m_timer.get(), 0, "{what}");
        assert_eq!(MLD6_TIMERS_ARE_RUNNING.load(Ordering::Relaxed), 0, "{what}");
    };

    // Not from a link-local address.
    mld6_input(
        mld_message(
            ifp,
            a6("fd00:77::2"),
            MLD_LISTENER_QUERY,
            10_000,
            IN6ADDR_ANY,
        ),
        40,
    );
    untouched("a global source");

    // About a unicast address.
    mld6_input(
        mld_message(ifp, ll(ifp), MLD_LISTENER_QUERY, 10_000, a6("fd00:77::2")),
        40,
    );
    untouched("a unicast group");

    // Shorter than an MLD header: counted, nothing else.
    let before = tooshort();
    let m = mld_message(ifp, ll(ifp), MLD_LISTENER_QUERY, 10_000, IN6ADDR_ANY);
    m.m_len().set(60);
    m.m_pkthdr().len.set(60);
    mld6_input(m, 40);
    assert_eq!(tooshort(), before + 1);
    untouched("a short message");
}

#[test]
fn a_report_from_another_listener_stops_our_timer() {
    let (_g, ifp) = mld_if();
    let a = join(ifp, group(ifp, "ff02::1234"));
    let other = group(ifp, "ff02::9999");
    let src = a6("fe80::5054:ff:fe00:7");
    let report = |g: In6Addr| mld_message(ifp, src, MLD_LISTENER_REPORT, 0, g);

    // A report for our group: we stop reporting it (somebody else did).
    a.in6m_timer.set(30);
    a.in6m_state.set(MLD_IREPORTEDLAST);
    mld6_input(report(group(ifp, "ff02::1234")), 40);
    assert_eq!(
        (a.in6m_timer.get(), a.in6m_state.get()),
        (0, MLD_OTHERLISTENER)
    );

    // For a group we are not in: nothing; for a non-multicast address: nothing.
    a.in6m_timer.set(30);
    mld6_input(report(other), 40);
    mld6_input(report(a6("fd00:77::2")), 40);
    assert_eq!(a.in6m_timer.get(), 30);

    // A report that we looped back ourselves does not count.
    let m = report(group(ifp, "ff02::1234"));
    m.m_flags().set(m.m_flags().get() | M_LOOP);
    mld6_input(m, 40);
    assert_eq!(a.in6m_timer.get(), 30);
}

#[test]
fn the_router_alert_option_is_ready_for_every_message() {
    let (_g, _ifp) = mld_if();
    let opts = mld6_ip6_opts();
    let hbh = opts.ip6po_hbh.expect("a hop-by-hop header");
    // SAFETY: the buffer is 8 bytes, written by `mld6_init` (`icmp6_init`) and read-only since.
    let b = unsafe { core::slice::from_raw_parts(hbh.as_ptr().cast::<u8>(), 8) };
    // next header (filled in by ip6_output), length 0 (8 bytes), PadN of 2, Router Alert
    // option (type 5, length 2) with value 0: MLD.
    assert_eq!(b[1..], [0, IP6OPT_PADN, 0, IP6OPT_ROUTER_ALERT, 2, 0, 0]);
    assert_eq!(opts.ip6po_hlim, -1);
    assert_eq!(opts.ip6po_tclass, -1);
}

/// A driver `ioctl` that accepts the multicast requests.
///
/// # Safety
///
/// `IfIoctlFn`'s contract.
unsafe fn accepting_ioctl(_ifp: &'static Ifnet, cmd: u64, _data: *mut u8) -> Result<(), Errno> {
    use crate::sys::sockio::{SIOCADDMULTI, SIOCDELMULTI};
    match cmd {
        SIOCADDMULTI | SIOCDELMULTI => Ok(()),
        _ => Err(Errno::ENOTTY),
    }
}
