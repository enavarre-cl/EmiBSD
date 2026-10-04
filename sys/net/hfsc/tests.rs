//! Host tests for HFSC: the service-curve arithmetic, the statistics ABI and a two-class
//! link-sharing hierarchy on a send queue.

use std::boxed::Box;
use std::sync::MutexGuard;

use super::*;
use crate::kern::uipc_mbuf::m_freem;
use crate::net::if_::tests::{setup_net, test_ifnet, test_packet, zeroed_static};
use crate::net::ifq::{IFQ_PRIQ_OPS, ifq_attach, ifq_dequeue, ifq_enqueue, ifq_init, ifq_len};
use crate::net::pfvar::{PFQS_DEFAULT, PfiKif, pf_abi_zeroed};

#[test]
fn m2sm_and_sm2m_round_trip() {
    // 1 byte per nanosecond is exactly 1 << SM_SHIFT, and back.
    assert_eq!(m2sm(8_000_000_000), 1 << SM_SHIFT);
    assert_eq!(sm2m(1 << SM_SHIFT), 8_000_000_000);
    assert_eq!(m2ism(8_000_000_000), 1 << ISM_SHIFT);
    assert_eq!(m2ism(0), HFSC_HT_INFINITY);

    // 100Kbps to 1Gbps keep at least three significant digits (the table in the C: at a
    // 1GHz clock 100Kbps is a scaled slope of 209).
    for m in [
        100_000u64,
        1_000_000,
        10_000_000,
        100_000_000,
        1_000_000_000,
    ] {
        let back = sm2m(m2sm(m));
        assert!(back <= m, "{m}: {back}");
        assert!(m - back <= m / 200, "{m}: {back}");
    }
    assert!(1_000_000_000 - sm2m(m2sm(1_000_000_000)) <= 1_000_000_000 / 100_000);

    assert_eq!(d2dx(10), 10_000_000);
    assert_eq!(dx2d(d2dx(10)), 10);
    assert_eq!(dx2d(d2dx(u32::MAX)), u32::MAX);
}

#[test]
fn seg_x2y_and_seg_y2x_are_inverse() {
    // At 1 byte/ns the segment is the identity, even for coordinates whose product with
    // the slope would overflow 64 bits (the split into upper and lower bits).
    let sm = m2sm(8_000_000_000);
    let ism = m2ism(8_000_000_000);
    for x in [0u64, 1, 12345, 1 << 40, u64::MAX >> 8] {
        assert_eq!(seg_x2y(x, sm), x);
        assert_eq!(seg_y2x(x, ism), x);
    }

    // 100Mbps: 12.5 bytes per microsecond.
    let sm = m2sm(100_000_000);
    let ism = m2ism(100_000_000);
    let y = seg_x2y(1_000_000, sm); // 1ms
    assert!((12_499..=12_500).contains(&y), "{y}");
    let x = seg_y2x(12_500, ism);
    assert!((999_000..=1_001_000).contains(&x), "{x}");

    assert_eq!(seg_y2x(0, HFSC_HT_INFINITY), 0);
    assert_eq!(seg_y2x(1, HFSC_HT_INFINITY), HFSC_HT_INFINITY);
}

#[test]
fn runtime_curves_follow_both_segments() {
    // Concave: 800Mbps (100 bytes/us) for 1ms, then 80Mbps.
    let mut isc = HfscInternalSc::default();
    hfsc_sc2isc(&HfscSc::new(800_000_000, 1, 80_000_000), &mut isc);
    assert_eq!(isc.dx, 1_000_000);
    assert!((99_999..=100_000).contains(&isc.dy), "{}", isc.dy);

    let mut rt = HfscRuntimeSc::default();
    hfsc_rtsc_init(&mut rt, &isc, 1000, 500);
    assert_eq!(hfsc_rtsc_x2y(&rt, 0), 500);
    assert_eq!(hfsc_rtsc_y2x(&rt, 0), 1000);
    // In the first segment, then in the second; y2x inverts x2y to within a nanosecond per
    // byte of rounding.
    for dx in [10_000u64, 500_000, 2_000_000, 7_000_000] {
        let y = hfsc_rtsc_x2y(&rt, 1000 + dx);
        let x = hfsc_rtsc_y2x(&rt, y);
        assert!(x.abs_diff(1000 + dx) <= 200, "{dx}: {y} -> {x}");
    }
    // Past the first segment the slope is the second one (10 bytes/us).
    let y1 = hfsc_rtsc_x2y(&rt, 1000 + 2_000_000);
    let y2 = hfsc_rtsc_x2y(&rt, 1000 + 3_000_000);
    assert!((9_999..=10_000).contains(&(y2 - y1)), "{}", y2 - y1);

    // The minimum with the same curve started later and lower takes the new start.
    hfsc_rtsc_min(&mut rt, &isc, 5_000_000, 0);
    assert_eq!((rt.x, rt.y), (5_000_000, 0));
}

#[test]
fn class_stats_have_the_c_layout() {
    assert_eq!(size_of::<HfscClassStats>(), 272);
    assert_eq!(size_of::<HfscSc>(), 24);
    assert_eq!(core::mem::offset_of!(HfscClassStats, class_handle), 48);
    assert_eq!(core::mem::offset_of!(HfscClassStats, rsc), 56);
    assert_eq!(core::mem::offset_of!(HfscClassStats, cur_time), 240);
    assert_eq!(core::mem::offset_of!(HfscClassStats, nactive), 260);
}

/// The network test setup with the timeout wheel reset (HFSC arms `hif_defer`), under the
/// wheel's test lock too.
fn setup() -> (MutexGuard<'static, ()>, MutexGuard<'static, ()>) {
    let g = setup_net();
    let t = crate::kern::kern_timeout::tests::LOCK
        .lock()
        .unwrap_or_else(|e| e.into_inner());
    crate::kern::kern_timeout::timeout_startup();
    (g, t)
}

/// A kernel queue spec on `kif`: link-share `ls` bits/sec under `parent_qid`.
fn queue(
    kif: &'static PfiKif,
    qid: u32,
    parent_qid: u32,
    ls: u64,
    flags: u32,
    qlimit: u32,
) -> &'static PfQueuespec {
    let mut q = pf_abi_zeroed::<PfQueuespec>();
    q.qid = qid;
    q.parent_qid = parent_qid;
    q.linkshare.m2.absolute = ls;
    q.flags = flags;
    q.qlimit = qlimit;
    q.set_kif(Some(kif));
    Box::leak(q)
}

/// A 100-byte packet for pf queue `qid`.
fn packet(qid: u32) -> &'static Mbuf {
    let m = test_packet(&[0u8; 100]);
    m.m_pkthdr().pf.qid.set(qid);
    m
}

/// An interface with an HFSC tree: root queue 1 (100Mbps), its children 2 (75Mbps) and the
/// default 3 (25Mbps), both limited to 50 packets.
fn hfsc_interface() -> (&'static Ifnet, &'static HfscIf) {
    hfsc_initialize();
    let ifp = test_ifnet(b"thfsc0");
    ifq_init(&ifp.if_snd, ifp, 0);
    // SAFETY: the all-zero kif is valid (`PfAbi`).
    let kif: &'static PfiKif = unsafe { zeroed_static() };
    kif.set_pfik_ifp(Some(ifp));

    let disc = (PFQ_HFSC_OPS.pfq_alloc)(ifp);
    // Without link-share bandwidth a root queue is refused.
    assert_eq!(
        (PFQ_HFSC_OPS.pfq_addqueue)(disc, queue(kif, 1, 0, 0, 0, 0)),
        Err(Errno::EINVAL)
    );
    for q in [
        queue(kif, 1, 0, 100_000_000, 0, 0),
        queue(kif, 2, 1, 75_000_000, 0, 0),
        queue(kif, 3, 1, 25_000_000, PFQS_DEFAULT, 0),
    ] {
        assert_eq!((PFQ_HFSC_OPS.pfq_addqueue)(disc, q), Ok(()));
    }
    // A queue id is used once.
    assert_eq!(
        (PFQ_HFSC_OPS.pfq_addqueue)(disc, queue(kif, 2, 1, 1_000_000, 0, 0)),
        Err(Errno::EBUSY)
    );
    // An unknown parent.
    assert_eq!(
        (PFQ_HFSC_OPS.pfq_addqueue)(disc, queue(kif, 4, 9, 1_000_000, 0, 0)),
        Err(Errno::EINVAL)
    );

    ifq_attach(&ifp.if_snd, IFQ_HFSC_OPS, disc);
    assert!(hfsc_enabled(&ifp.if_snd));
    // SAFETY: the softc `hfsc_pf_alloc` made, attached to the queue.
    (ifp, unsafe { hif_of(disc) })
}

#[test]
fn link_share_divides_the_link_by_the_curves() {
    let _g = setup();
    let (ifp, hif) = hfsc_interface();
    let ifq = &ifp.if_snd;

    let root = hif.hif_rootclass.get().expect("root");
    assert_eq!(root.cl_handle.get(), HFSC_ROOT_CLASS | 1);
    let a = hfsc_clh2cph(hif, 2).expect("class 2");
    let b = hfsc_clh2cph(hif, 3).expect("class 3");
    assert!(hif.hif_defaultclass.get().is_some_and(|d| ptr::eq(d, b)));
    // Queue 1 is a class below the hidden root class pf's root queue creates.
    let q1 = hfsc_clh2cph(hif, 1).expect("class 1");
    assert!(ptr::eq(q1.cl_parent.get().expect("parent"), root));
    assert!(ptr::eq(a.cl_parent.get().expect("parent"), q1));
    assert_eq!(hif.hif_classes.get(), 4);

    for _ in 0..40 {
        assert_eq!(ifq_enqueue(ifq, packet(2)), Ok(()));
        assert_eq!(ifq_enqueue(ifq, packet(3)), Ok(()));
    }
    // An unknown queue id goes to the default class.
    assert_eq!(ifq_enqueue(ifq, packet(77)), Ok(()));
    assert_eq!(ifq_len(ifq), 81);
    assert_eq!(hfsc_class_qlength(b), 41);
    // Both leaves are active, on queue 1's active list; queue 1 is on the hidden root's.
    assert_eq!((a.cl_nactive.get(), b.cl_nactive.get()), (1, 1));
    assert_eq!((q1.cl_nactive.get(), q1.cl_actc.iter().count()), (2, 2));
    assert_eq!(root.cl_actc.iter().count(), 1);

    let (mut na, mut nb) = (0, 0);
    for _ in 0..40 {
        let m = ifq_dequeue(ifq).expect("a packet");
        match m.m_pkthdr().pf.qid.get() {
            2 => na += 1,
            _ => nb += 1,
        }
        m_freem(m);
    }
    // 75% and 25% of the link, to within a packet or two.
    assert!((29..=31).contains(&na), "{na} {nb}");
    assert_eq!(na + nb, 40);
    assert_eq!(a.cl_stats.xmit_cnt.get().packets, na);
    assert_eq!(a.cl_stats.xmit_cnt.get().bytes, 100 * na);

    let mut st = HfscClassStats::default();
    hfsc_getclstats(&mut st, a);
    assert_eq!(st.class_handle, 2);
    assert_eq!(st.qlength, 40 - na as u32);
    assert_eq!(st.qlimit, HFSC_DEFAULT_QLIMIT as u32);
    assert_eq!(st.fsc.m2, sm2m(m2sm(75_000_000)));
    assert_eq!(st.rsc, HfscSc::default());
    assert_eq!(st.total, 100 * na);
    assert_eq!(st.machclk_freq, 1_000_000_000);
    assert_eq!(st.period, 1);

    // Back to priq: the softc goes, the waiting packets move over.
    let left = ifq_len(ifq);
    ifq_attach(ifq, IFQ_PRIQ_OPS, ptr::null_mut());
    assert!(!hfsc_enabled(ifq));
    assert_eq!(ifq_len(ifq), left);
    while let Some(m) = ifq_dequeue(ifq) {
        m_freem(m);
    }
}

#[test]
fn a_full_class_drops_and_counts() {
    let _g = setup();
    let (ifp, hif) = hfsc_interface();
    let ifq = &ifp.if_snd;
    let a = hfsc_clh2cph(hif, 2).expect("class 2");

    let limit = HFSC_DEFAULT_QLIMIT as usize;
    for _ in 0..limit {
        assert_eq!(ifq_enqueue(ifq, packet(2)), Ok(()));
    }
    assert_eq!(ifq_enqueue(ifq, packet(2)), Err(Errno::ENOBUFS));
    assert_eq!(
        a.cl_stats.drop_cnt.get(),
        HfscPktcntr {
            packets: 1,
            bytes: 100
        }
    );
    assert_eq!(ifq.ifq_qdrops.get(), 1);

    // Purging (as ifq_purge does) makes the class passive again.
    assert_eq!(crate::net::ifq::ifq_purge(ifq), limit as u32);
    assert_eq!(hfsc_class_qlength(a), 0);
    assert!(hif.hif_rootclass.get().expect("root").cl_actc.is_empty());

    ifq_attach(ifq, IFQ_PRIQ_OPS, ptr::null_mut());
}
