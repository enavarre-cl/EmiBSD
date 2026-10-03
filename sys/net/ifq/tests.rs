//! Host tests for the interface queues: the priq conditioner, the counters and the dequeue
//! protocol, over a zero-filled interface (no softnet task queue: nothing here starts the
//! queue).

use std::vec::Vec;

use super::*;
use crate::kern::uipc_mbuf::MBPOOL;
use crate::net::if_::tests::{setup_net, test_ifnet, test_packet};
use crate::sys::pool::Pool;

/// A packet of `len` bytes at priority `prio`, tagged with `id` in its flow id.
fn packet(len: usize, prio: u8, id: u16) -> &'static Mbuf {
    let bytes: Vec<u8> = (0..len).map(|i| i as u8).collect();
    let m = test_packet(&bytes);
    m.m_pkthdr().pf.prio.set(prio);
    m.m_pkthdr().ph_flowid.set(id);
    m
}

/// A send queue of `maxlen` packets on a fresh interface.
fn queue(maxlen: u32) -> &'static Ifqueue {
    let ifp = test_ifnet(b"tifq0");
    ifq_init_maxlen(&ifp.if_snd, maxlen);
    ifq_init(&ifp.if_snd, ifp, 0);
    &ifp.if_snd
}

fn outstanding(pp: &Pool) -> u32 {
    pp.pr_nout.get()
}

#[test]
fn priq_dequeues_the_highest_priority_first_and_counts() {
    let _g = setup_net();
    let ifq = queue(IFQ_MAXLEN);
    assert!(ifq_is_priq(ifq));
    assert_eq!(ifq.ifq_maxlen.get(), IFQ_MAXLEN);

    for (prio, id) in [(1, 10), (6, 60), (3, 30), (6, 61)] {
        assert_eq!(ifq_enqueue(ifq, packet(20, prio, id)), Ok(()));
    }
    let mcast = packet(20, 0, 0);
    mcast.m_flags().set(mcast.m_flags().get() | M_MCAST);
    assert_eq!(ifq_enqueue(ifq, mcast), Ok(()));

    assert_eq!(ifq_len(ifq), 5);
    assert_eq!(ifq.ifq_packets.get(), 5);
    assert_eq!(ifq.ifq_bytes.get(), 100);
    assert_eq!(ifq.ifq_mcasts.get(), 1);
    assert_eq!(ifq_hdatalen(ifq), 20);

    let order: Vec<u16> = core::iter::from_fn(|| {
        let m = ifq_dequeue(ifq)?;
        let id = m.m_pkthdr().ph_flowid.get();
        m_freem(m);
        Some(id)
    })
    .collect();
    // Highest priority first, FIFO within a priority.
    assert_eq!(order, [60, 61, 30, 10, 0]);
    assert!(ifq_empty(ifq));
    assert_eq!(ifq_hdatalen(ifq), 0);
}

#[test]
fn a_full_priq_drops_lower_priorities_or_refuses() {
    let _g = setup_net();
    let ifq = queue(2);
    let before = outstanding(&MBPOOL);

    assert_eq!(ifq_enqueue(ifq, packet(8, 0, 1)), Ok(()));
    assert_eq!(ifq_enqueue(ifq, packet(8, 0, 2)), Ok(()));
    // Full: a priority 3 packet evicts the oldest priority 0 one.
    assert_eq!(ifq_enqueue(ifq, packet(8, 3, 3)), Ok(()));
    assert_eq!(ifq_len(ifq), 2);
    assert_eq!(ifq.ifq_qdrops.get(), 1);
    // Full and nothing lower than priority 0: the new packet is refused (and freed).
    assert_eq!(ifq_enqueue(ifq, packet(8, 0, 4)), Err(Errno::ENOBUFS));
    assert_eq!(ifq.ifq_qdrops.get(), 2);
    assert_eq!(
        ifq.ifq_packets.get(),
        3,
        "a refused packet is not counted as sent"
    );

    let first = ifq_dequeue(ifq).expect("priority 3");
    assert_eq!(first.m_pkthdr().ph_flowid.get(), 3);
    m_freem(first);
    let second = ifq_dequeue(ifq).expect("the survivor");
    assert_eq!(second.m_pkthdr().ph_flowid.get(), 2);
    m_freem(second);
    assert_eq!(
        outstanding(&MBPOOL),
        before,
        "every dropped packet was freed"
    );
}

#[test]
fn deq_begin_and_rollback_leave_the_packet_queued() {
    let _g = setup_net();
    let ifq = queue(IFQ_MAXLEN);
    assert_eq!(ifq_enqueue(ifq, packet(12, 2, 7)), Ok(()));

    let m = ifq_deq_begin(ifq).expect("head");
    assert_eq!(m.m_pkthdr().ph_flowid.get(), 7);
    ifq_deq_set_oactive(ifq);
    ifq_deq_rollback(ifq, m);
    assert_eq!(ifq_len(ifq), 1);
    assert!(ifq_is_oactive(ifq));
    assert_eq!(ifq.ifq_oactives.get(), 1);
    ifq_clr_oactive(ifq);
    ifq_set_oactive(ifq);
    ifq_set_oactive(ifq);
    assert_eq!(ifq.ifq_oactives.get(), 2, "counted once per transition");

    let m = ifq_deq_begin(ifq).expect("still there");
    ifq_deq_commit(ifq, m);
    assert!(ifq_empty(ifq));
    m_freem(m);
}

#[test]
fn purge_and_attach_keep_the_books() {
    let _g = setup_net();
    let ifq = queue(IFQ_MAXLEN);
    let before = outstanding(&MBPOOL);
    for id in 0..4 {
        assert_eq!(ifq_enqueue(ifq, packet(4, (id % 8) as u8, id)), Ok(()));
    }

    // Reattaching the conditioner moves the packets to the new state.
    ifq_attach(ifq, IFQ_PRIQ_OPS, ptr::null_mut());
    assert_eq!(ifq_len(ifq), 4);
    assert_eq!(ifq.ifq_qdrops.get(), 0);

    // ifq_q_enter takes the mutex only for the queue's own conditioner.
    let q = ifq_q_enter(ifq, IFQ_PRIQ_OPS).expect("priq");
    ifq_q_leave(ifq, q);

    let mut data = IfData::default();
    ifq_add_data(ifq, &mut data);
    assert_eq!(data.ifi_opackets, 4);
    assert_eq!(data.ifi_obytes, 16);

    assert_eq!(ifq_purge(ifq), 4);
    assert!(ifq_empty(ifq));
    assert_eq!(ifq.ifq_qdrops.get(), 4);
    assert_eq!(outstanding(&MBPOOL), before);
}

#[test]
fn priq_idx_spreads_by_flow_id() {
    let _g = setup_net();
    let ifq = queue(IFQ_MAXLEN);
    let m = packet(4, 0, 13);
    assert_eq!(ifq_idx(ifq, 4, m), 0, "no M_FLOWID: queue 0");
    m.m_pkthdr()
        .csum_flags
        .set(m.m_pkthdr().csum_flags.get() | M_FLOWID);
    assert_eq!(ifq_idx(ifq, 4, m), 13 % 4);
    m_freem(m);
}

#[test]
#[ignore = "needs OPENBSD_SRC (just test-ref)"]
fn values_match_the_c_header() {
    let defs = crate::reftest::defines("sys/net/ifq.h");
    crate::reftest::assert_defines!(defs; IFQ_MAXLEN);
}
