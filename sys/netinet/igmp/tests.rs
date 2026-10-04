//! Host tests for IGMP over the test Ethernet interface: the all-hosts group `in_ifinit`
//! joins is never reported; joining another group sends a v2 report with the Router Alert
//! option and TTL 1 at once and again when its timer runs out; a query restarts the timer
//! and another host's report stops it; leaving a group we may have reported last sends a
//! leave message; a v1 query makes the reports v1 until the router ages back to v2.

use std::vec::Vec;

use super::*;
use crate::kern::kern_rwlock::{rw_enter_write, rw_exit_write};
use crate::net::if_::tests::test_packet;
use crate::net::if_ethersubr::ether_ioctl;
use crate::net::ifq::ifq_dequeue;
use crate::netinet::if_ether::arpcom_of;
use crate::netinet::in_::{in_addmulti, in_delmulti};
use crate::netinet::ip_input::tests::{ADDR, bytes, configure, setup, test_ether};
use crate::sys::sockio::{SIOCADDMULTI, SIOCDELMULTI};

/// The test interface's `ioctl`, with the driver's answer to a changed multicast filter:
/// `ether_ioctl` says `ENETRESET` (reprogram the filter), which a driver turns into success.
///
/// # Safety
///
/// `IfIoctlFn`'s contract.
unsafe fn mcast_ioctl(ifp: &'static Ifnet, cmd: u64, data: *mut u8) -> Result<(), Errno> {
    if cmd == crate::sys::sockio::SIOCSIFADDR {
        ifp.if_flags
            .set(ifp.if_flags.get() | crate::net::if_::IFF_UP | crate::net::if_::IFF_RUNNING);
        return Ok(());
    }
    // SAFETY: the caller's contract.
    match unsafe { ether_ioctl(ifp, arpcom_of(ifp), cmd, data) } {
        Err(Errno::ENETRESET) if cmd == SIOCADDMULTI || cmd == SIOCDELMULTI => Ok(()),
        r => r,
    }
}

/// The value of counter `c`.
fn count(c: IgmpstatCounters) -> u64 {
    IGMPCOUNTERS[c as usize].load(Ordering::Relaxed)
}

/// `a.b.c.d` as an `InAddr`.
fn addr(a: [u8; 4]) -> InAddr {
    InAddr {
        s_addr: u32::from_ne_bytes(a),
    }
}

/// The internet checksum of `b`.
fn cksum(b: &[u8]) -> u16 {
    let mut sum: u32 = b
        .chunks(2)
        .map(|c| u32::from(u16::from_be_bytes([c[0], *c.get(1).unwrap_or(&0)])))
        .sum();
    while sum >> 16 != 0 {
        sum = (sum & 0xffff) + (sum >> 16);
    }
    !(sum as u16)
}

/// The IGMP messages the interface sent, as (IP destination, IGMP type, group); each one
/// checked for the Router Alert option, TTL 1 and its checksum.
fn sent(ifp: &Ifnet) -> Vec<([u8; 4], u8, [u8; 4])> {
    let mut v = Vec::new();
    while let Some(m) = ifq_dequeue(&ifp.if_snd) {
        let f = bytes(m);
        m_freem(m);
        assert_eq!(&f[12..14], &[0x08, 0x00], "IPv4");
        let ip = &f[14..];
        assert_eq!(ip[0], 0x46, "a 24-byte header: the Router Alert option");
        assert_eq!(ip[8], 1, "TTL 1");
        assert_eq!(ip[9], IPPROTO_IGMP as u8);
        assert_eq!(&ip[20..24], &[0x94, 0x04, 0, 0]);
        let igmp = &ip[24..32];
        assert_eq!(cksum(igmp), 0, "the IGMP checksum");
        // The destination MAC maps the group (01:00:5e and its low 23 bits).
        assert_eq!(&f[0..3], &[0x01, 0x00, 0x5e]);
        assert_eq!(&f[3..6], &[ip[17] & 0x7f, ip[18], ip[19]]);
        v.push((
            [ip[16], ip[17], ip[18], ip[19]],
            igmp[0],
            [igmp[4], igmp[5], igmp[6], igmp[7]],
        ));
    }
    v
}

/// An IGMP message of `type_`, `code` and `group` from 10.0.2.2 to `dst`, handed to
/// `igmp_input` as received on `ifp`.
fn receive(ifp: &Ifnet, dst: [u8; 4], type_: u8, code: u8, group: [u8; 4], bad_sum: bool) {
    let mut p = std::vec![0u8; 28];
    p[0] = 0x45;
    p[2..4].copy_from_slice(&28u16.to_be_bytes());
    p[8] = 1;
    p[9] = IPPROTO_IGMP as u8;
    p[12..16].copy_from_slice(&[10, 0, 2, 2]);
    p[16..20].copy_from_slice(&dst);
    p[20] = type_;
    p[21] = code;
    p[24..28].copy_from_slice(&group);
    let s = cksum(&p[20..]);
    p[22..24].copy_from_slice(&(if bad_sum { !s } else { s }).to_be_bytes());
    let m = test_packet(&p);
    m.m_pkthdr().ph_ifidx.set(ifp.if_index.get());
    let mut mp = Some(m);
    let mut off = 20;
    let _ = igmp_input(&mut mp, &mut off, IPPROTO_IGMP, i32::from(AF_INET), None);
    m_freemp(&mut mp);
}

/// The state and timer of our membership of `g` on `ifp`.
fn membership(ifp: &Ifnet, g: [u8; 4]) -> (u32, u32) {
    rw_enter_write(&ifp.if_maddrlock);
    let inm = in_lookupmulti(&addr(g), ifp).expect("a member");
    let r = (inm.inm_state.get(), inm.inm_timer.get());
    rw_exit_write(&ifp.if_maddrlock);
    r
}

/// Runs the fast timeout until no timer runs (at most `n` ticks).
fn tick(n: usize) {
    for _ in 0..n {
        igmp_fasttimo();
    }
}

#[test]
fn reports_queries_and_leaves() {
    let (_g, _t) = setup();
    igmp_init();
    let ifp = test_ether();
    ifp.if_ioctl.set(Some(mcast_ioctl));
    configure(ifp, ADDR, [255, 255, 255, 0]);
    while let Some(m) = ifq_dequeue(&ifp.if_snd) {
        m_freem(m);
    }

    // 224.0.0.1, joined by in_ifinit: a local group, idle and never reported.
    assert_eq!(membership(ifp, [224, 0, 0, 1]), (IGMP_IDLE_MEMBER, 0));

    // Joining 239.1.2.3: a v2 report at once, and the timer for the second one.
    let snd = count(IgmpstatCounters::IgpsSndReports);
    let inm = in_addmulti(&addr([239, 1, 2, 3]), ifp).expect("joined");
    assert_eq!(
        sent(ifp),
        [(
            [239, 1, 2, 3],
            IGMP_v2_HOST_MEMBERSHIP_REPORT,
            [239, 1, 2, 3]
        )]
    );
    let (state, timer) = membership(ifp, [239, 1, 2, 3]);
    assert_eq!(state, IGMP_DELAYING_MEMBER);
    assert!((1..=IGMP_MAX_HOST_REPORT_DELAY * PR_FASTHZ as u32).contains(&timer));
    tick(timer as usize);
    assert_eq!(
        sent(ifp),
        [(
            [239, 1, 2, 3],
            IGMP_v2_HOST_MEMBERSHIP_REPORT,
            [239, 1, 2, 3]
        )]
    );
    assert_eq!(membership(ifp, [239, 1, 2, 3]), (IGMP_IDLE_MEMBER, 0));
    assert_eq!(count(IgmpstatCounters::IgpsSndReports), snd + 2);

    // A v2 general query (max response 10 tenths of a second) restarts the timer.
    receive(
        ifp,
        [224, 0, 0, 1],
        IGMP_HOST_MEMBERSHIP_QUERY,
        10,
        [0; 4],
        false,
    );
    let (state, timer) = membership(ifp, [239, 1, 2, 3]);
    assert_eq!(state, IGMP_DELAYING_MEMBER);
    assert!(
        (1..=5).contains(&timer),
        "10 tenths of a second are 5 ticks"
    );
    assert_eq!(membership(ifp, [224, 0, 0, 1]), (IGMP_IDLE_MEMBER, 0));
    // Another host reports the group first: our timer stops, and we are lazy.
    let ours = count(IgmpstatCounters::IgpsRcvOurreports);
    receive(
        ifp,
        [239, 1, 2, 3],
        IGMP_v2_HOST_MEMBERSHIP_REPORT,
        0,
        [239, 1, 2, 3],
        false,
    );
    assert_eq!(membership(ifp, [239, 1, 2, 3]), (IGMP_LAZY_MEMBER, 0));
    assert_eq!(count(IgmpstatCounters::IgpsRcvOurreports), ours + 1);
    tick(1);
    assert!(sent(ifp).is_empty());

    // Leaving while lazy: someone else reported last, so no leave message.
    in_delmulti(inm);
    assert!(sent(ifp).is_empty());

    // Leaving a group we reported: a leave message to all routers.
    let inm = in_addmulti(&addr([239, 9, 9, 9]), ifp).expect("joined");
    let _ = sent(ifp);
    in_delmulti(inm);
    assert_eq!(
        sent(ifp),
        [([224, 0, 0, 2], IGMP_HOST_LEAVE_MESSAGE, [224, 0, 0, 2])]
    );

    // A bad checksum is counted and dropped.
    let bad = count(IgmpstatCounters::IgpsRcvBadsum);
    receive(
        ifp,
        [224, 0, 0, 1],
        IGMP_HOST_MEMBERSHIP_QUERY,
        10,
        [0; 4],
        true,
    );
    assert_eq!(count(IgmpstatCounters::IgpsRcvBadsum), bad + 1);
}

#[test]
fn a_v1_router_gets_v1_reports_until_it_ages_out() {
    let (_g, _t) = setup();
    igmp_init();
    let ifp = test_ether();
    ifp.if_ioctl.set(Some(mcast_ioctl));
    configure(ifp, ADDR, [255, 255, 255, 0]);
    let inm = in_addmulti(&addr([239, 4, 4, 4]), ifp).expect("joined");
    while let Some(m) = ifq_dequeue(&ifp.if_snd) {
        m_freem(m);
    }

    // A v1 query (code 0) to all hosts: the router is v1, and the reports follow.
    receive(
        ifp,
        [224, 0, 0, 1],
        IGMP_HOST_MEMBERSHIP_QUERY,
        0,
        [0; 4],
        false,
    );
    assert_eq!(rti_type(ifp.if_index.get()), IGMP_v1_ROUTER);
    tick((IGMP_MAX_HOST_REPORT_DELAY * PR_FASTHZ as u32) as usize);
    assert_eq!(
        sent(ifp),
        [(
            [239, 4, 4, 4],
            IGMP_v1_HOST_MEMBERSHIP_REPORT,
            [239, 4, 4, 4]
        )]
    );
    // A v1 query to another address is refused.
    let badq = count(IgmpstatCounters::IgpsRcvBadqueries);
    receive(
        ifp,
        [239, 4, 4, 4],
        IGMP_HOST_MEMBERSHIP_QUERY,
        0,
        [0; 4],
        false,
    );
    assert_eq!(count(IgmpstatCounters::IgpsRcvBadqueries), badq + 1);

    // Without v1 queries the router is v2 again after IGMP_AGE_THRESHOLD slow timeouts.
    for _ in 0..IGMP_AGE_THRESHOLD - 1 {
        igmp_slowtimo();
    }
    assert_eq!(rti_type(ifp.if_index.get()), IGMP_v1_ROUTER);
    igmp_slowtimo();
    assert_eq!(rti_type(ifp.if_index.get()), IGMP_v2_ROUTER);

    // rti_delete forgets the interface: unknown is v2.
    receive(
        ifp,
        [224, 0, 0, 1],
        IGMP_HOST_MEMBERSHIP_QUERY,
        0,
        [0; 4],
        false,
    );
    rti_delete(ifp);
    assert_eq!(rti_type(ifp.if_index.get()), IGMP_v2_ROUTER);

    in_delmulti(inm);
    while let Some(m) = ifq_dequeue(&ifp.if_snd) {
        m_freem(m);
    }

    // net.inet.igmp.stats: the counters as a struct igmpstat.
    let mut st = crate::netinet::igmp_var::Igmpstat::default();
    let mut len = size_of::<crate::netinet::igmp_var::Igmpstat>();
    igmp_sysctl(
        &[IGMPCTL_STATS],
        ptr::from_mut(&mut st) as usize,
        &mut len,
        0,
        0,
    )
    .expect("stats");
    assert_eq!(
        st.igps_rcv_badqueries,
        count(IgmpstatCounters::IgpsRcvBadqueries)
    );
}

#[test]
#[ignore = "needs OPENBSD_SRC (just test-ref)"]
fn values_match_the_c_headers() {
    use crate::reftest::{assert_complete, assert_defines};
    let defs = crate::reftest::defines("sys/netinet/igmp.h");
    let igmp = assert_defines!(defs;
        IGMP_MINLEN, IGMP_HOST_MEMBERSHIP_QUERY, IGMP_v1_HOST_MEMBERSHIP_REPORT, IGMP_DVMRP,
        IGMP_PIM, IGMP_v2_HOST_MEMBERSHIP_REPORT, IGMP_HOST_LEAVE_MESSAGE, IGMP_MTRACE_REPLY,
        IGMP_MTRACE_QUERY, IGMP_MAX_HOST_REPORT_DELAY, IGMP_TIMER_SCALE, IGMP_DELAYING_MEMBER,
        IGMP_IDLE_MEMBER, IGMP_LAZY_MEMBER, IGMP_SLEEPING_MEMBER, IGMP_AWAKENING_MEMBER,
        IGMP_v1_ROUTER, IGMP_v2_ROUTER, IGMP_AGE_THRESHOLD);
    assert_complete(&defs, "IGMP_", &igmp);
    let defs = crate::reftest::defines("sys/netinet/igmp_var.h");
    use crate::netinet::igmp_var::IGMPCTL_MAXID;
    assert_defines!(defs; IGMPCTL_STATS, IGMPCTL_MAXID);
}
