//! Host tests for FQ-CoDel: the interval table and the control law, CoDel's dropping
//! state, the round robin across flows and the batch drop over the limit.

use std::boxed::Box;
use std::vec::Vec;

use super::*;
use crate::kern::uipc_mbuf::m_freem;
use crate::net::if_::tests::{setup_net, test_packet};
use crate::net::pfvar::{PFQS_FLOWQUEUE, pf_abi_zeroed};

/// A `len`-byte packet of flow `flowid`.
fn packet(len: usize, flowid: u16) -> &'static Mbuf {
    let m = test_packet(&std::vec![0u8; len]);
    m.m_pkthdr().ph_flowid.set(flowid);
    m.m_pkthdr()
        .csum_flags
        .set(m.m_pkthdr().csum_flags.get() | M_FLOWID);
    m
}

#[test]
fn the_intervals_are_100ms_over_the_square_root() {
    for (i, &v) in CODEL_INTERVALS.iter().enumerate() {
        let n = i as u128 + 1;
        // v * v * n == (100ms)^2, to the rounding of v.
        let sq = u128::from(v) * u128::from(v) * n;
        let want = 100_000_000u128 * 100_000_000;
        assert!(sq.abs_diff(want) * 1_000_000 < want, "{n}: {v}");
    }
    assert!(CODEL_INTERVALS.windows(2).all(|w| w[0] > w[1]));
}

#[test]
fn the_control_law_shrinks_the_interval_with_the_drops() {
    let cp = CodelParams::new();
    codel_initparams(&cp, 0, 0, 1500);
    assert_eq!(cp.target.get(), CODEL_TARGET);
    assert_eq!(cp.interval.get(), 100_000_000);
    assert_eq!(cp.grace.get(), 1_600_000_000);
    assert!(
        cp.intervals
            .get()
            .is_some_and(|t| ptr::eq(t, &CODEL_INTERVALS))
    );

    let cd = Codel::new();
    for (drops, next) in [(1u16, 100_000_000i64), (4, 50_000_000), (100, 10_000_000)] {
        cd.drops.set(drops);
        control_law(&cd, &cp, 1000);
        assert_eq!(cd.next.get(), 1000 + next);
    }
    // Past the table the last interval stays.
    cd.drops.set(60000);
    control_law(&cd, &cp, 0);
    assert_eq!(cd.next.get(), 5_006_262);
    codel_freeparams(&cp);
    assert!(cp.intervals.get().is_none());
}

#[test]
fn a_longer_interval_scales_the_table() {
    let _g = setup_net();
    let cp = CodelParams::new();
    codel_initparams(&cp, 1_000_000, 200_000_000, 1500);
    // 5% of the interval beats the smaller target.
    assert_eq!(cp.target.get(), 10_000_000);
    assert_eq!(cp.interval.get(), 200_000_000);
    let tbl = cp.intervals.get().expect("a table");
    assert!(!ptr::eq(tbl, &CODEL_INTERVALS));
    assert_eq!(tbl[0], 200_000_000);
    assert_eq!(tbl[3], 100_000_000);
    assert!(
        tbl.iter()
            .zip(CODEL_INTERVALS.iter())
            .all(|(&s, &o)| s == o * 2)
    );
    codel_freeparams(&cp);
    assert!(cp.intervals.get().is_none());
}

#[test]
fn codel_drops_after_an_interval_above_the_target() {
    let _g = setup_net();
    let cp = CodelParams::new();
    codel_initparams(&cp, 0, 0, 0);
    let cd = Codel::new();
    for _ in 0..5 {
        codel_enqueue(&cd, 0, packet(100, 0));
    }
    assert_eq!(codel_backlog(&cd), 500);

    let free_ml = MbufList::new();
    let (mut dpkts, mut dbytes) = (0u64, 0u64);

    // Delayed past the target: the observation interval starts, nothing is dropped yet.
    let now = 10_000_000;
    let m = codel_dequeue(&cd, &cp, now, &free_ml, &mut dpkts, &mut dbytes).expect("m");
    assert_eq!(cd.start.get(), now + 100_000_000);
    assert!(!cd.dropping.get());
    assert_eq!(cd.delay.get(), now);
    m_freem(codel_commit(&cd, Some(m)));

    // Still above the target a whole interval later: the head is dropped, the next packet
    // is sent, and the next drop is one interval away.
    let now = cd.start.get();
    let m = codel_dequeue(&cd, &cp, now, &free_ml, &mut dpkts, &mut dbytes).expect("m");
    assert_eq!((dpkts, dbytes), (1, 100));
    assert_eq!(ml_len(&free_ml), 1);
    assert!(cd.dropping.get());
    assert_eq!(cd.drops.get(), 1);
    assert_eq!(cd.next.get(), now + 100_000_000);
    m_freem(codel_commit(&cd, Some(m)));
    assert_eq!(codel_qlength(&cd), 2);
    assert_eq!(codel_backlog(&cd), 200);

    let ml = MbufList::new();
    codel_purge(&cd, &ml);
    assert_eq!(ml_len(&ml), 2);
    assert_eq!(codel_backlog(&cd), 0);
    let _ = ml_purge(&ml);
    let _ = ml_purge(&free_ml);
}

/// A flow queue of `flows` flows, a fixed `quantum` and `qlimit` packets.
fn fqcodel(flows: u32, quantum: u32, qlimit: u32) -> *mut c_void {
    let mut qs = pf_abi_zeroed::<PfQueuespec>();
    qs.flags = PFQS_FLOWQUEUE;
    qs.flowqueue.flows = flows;
    qs.flowqueue.quantum = quantum;
    qs.qlimit = qlimit;
    let qs: &'static PfQueuespec = Box::leak(qs);

    let ifp = crate::net::if_::tests::test_ifnet(b"tfqc0");
    let fqc = (PFQ_FQCODEL_OPS.pfq_alloc)(ifp);
    let mut bad = pf_abi_zeroed::<PfQueuespec>();
    bad.flowqueue.flows = 0x10000;
    assert_eq!(
        (PFQ_FQCODEL_OPS.pfq_addqueue)(fqc, Box::leak(bad)),
        Err(Errno::EINVAL)
    );
    assert_eq!((PFQ_FQCODEL_OPS.pfq_addqueue)(fqc, qs), Ok(()));
    fqc
}

/// Dequeues everything through the pf ops, returning the flow ids in order.
fn drain(fqc: *mut c_void) -> Vec<u16> {
    let mut order = Vec::new();
    loop {
        let free_ml = MbufList::new();
        let mut cookie = ptr::null_mut();
        let Some(m) = (PFQ_FQCODEL_OPS.pfq_deq_begin)(fqc, &mut cookie, &free_ml) else {
            break;
        };
        assert!(ml_empty(&free_ml));
        (PFQ_FQCODEL_OPS.pfq_deq_commit)(fqc, m, cookie);
        order.push(m.m_pkthdr().ph_flowid.get());
        m_freem(m);
    }
    order
}

#[test]
fn flows_are_served_round_robin_by_quantum() {
    let _g = setup_net();
    let fqc = fqcodel(4, 300, 0);
    // SAFETY: made by `fqcodel_pf_alloc`.
    let st = unsafe { fqc_of(fqc) };
    assert_eq!(st.qlimit.get(), FQCODEL_QLIMIT);
    assert_eq!(st.flags.get(), FQCF_FIXED_QUANTUM);

    for _ in 0..6 {
        for flow in [0, 1] {
            assert!((PFQ_FQCODEL_OPS.pfq_enqueue)(fqc, packet(100, flow)).is_none());
        }
    }
    assert_eq!((PFQ_FQCODEL_OPS.pfq_qlength)(fqc), 12);
    assert!(st.flows()[0].active.get() && st.flows()[1].active.get());
    assert!(!st.flows()[2].active.get());

    // A quantum of 300 bytes is three 100-byte packets per turn.
    assert_eq!(drain(fqc), [0, 0, 0, 1, 1, 1, 0, 0, 0, 1, 1, 1]);
    assert_eq!((PFQ_FQCODEL_OPS.pfq_qlength)(fqc), 0);
    assert_eq!(
        st.xmit_cnt.get(),
        FqcodelPktcntr {
            packets: 12,
            bytes: 1200
        }
    );
    assert_eq!(st.drop_cnt.get(), FqcodelPktcntr::default());
    // Emptied flows went idle.
    assert!(!st.flows()[0].active.get() && !st.flows()[1].active.get());

    (PFQ_FQCODEL_OPS.pfq_free)(fqc);
}

#[test]
fn over_the_limit_the_biggest_flow_loses_half() {
    let _g = setup_net();
    let fqc = fqcodel(2, 1500, 4);
    // SAFETY: made by `fqcodel_pf_alloc`.
    let st = unsafe { fqc_of(fqc) };

    let first: Vec<&'static Mbuf> = (0..4).map(|_| packet(100, 0)).collect();
    for &m in &first {
        assert!((PFQ_FQCODEL_OPS.pfq_enqueue)(fqc, m).is_none());
    }
    // The fifth packet puts the queue over its limit: half of flow 0 (two packets) is
    // dropped from the head, the first returned now, the other on the next enqueue.
    let d = (PFQ_FQCODEL_OPS.pfq_enqueue)(fqc, packet(100, 0)).expect("a drop");
    assert!(ptr::eq(d, first[0]));
    m_freem(d);
    assert_eq!(
        st.drop_cnt.get(),
        FqcodelPktcntr {
            packets: 2,
            bytes: 200
        }
    );
    assert_eq!((PFQ_FQCODEL_OPS.pfq_qlength)(fqc), 3);
    assert_eq!(ml_len(&st.pending_drops), 1);

    let d = (PFQ_FQCODEL_OPS.pfq_enqueue)(fqc, packet(100, 1)).expect("the pending drop");
    assert!(ptr::eq(d, first[1]));
    m_freem(d);
    assert_eq!((PFQ_FQCODEL_OPS.pfq_qlength)(fqc), 4);

    let ml = MbufList::new();
    (PFQ_FQCODEL_OPS.pfq_purge)(fqc, &ml);
    assert_eq!(ml_len(&ml), 4);
    assert_eq!((PFQ_FQCODEL_OPS.pfq_qlength)(fqc), 0);
    let _ = ml_purge(&ml);

    (PFQ_FQCODEL_OPS.pfq_free)(fqc);
}

#[test]
fn stats_have_the_c_layout() {
    assert_eq!(size_of::<FqcodelStats>(), 72);
    assert_eq!(core::mem::offset_of!(FqcodelStats, qlength), 32);
    assert_eq!(core::mem::offset_of!(FqcodelStats, target), 48);
    assert_eq!(core::mem::offset_of!(FqcodelStats, delaysumsq), 64);
}
