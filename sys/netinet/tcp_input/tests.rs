//! Host tests for TCP input: the option parser (SYN options, options after the handshake,
//! malformed lists, a signature where none is expected), sequence subtraction, the receiver's
//! SACK report and the sender's SACK holes, the round-trip timer, the header size,
//! reassembly into a socket, the timestamp and sequence helpers, and the SYN cache (hash,
//! insert, lookup, reset, cleanup).

use std::boxed::Box;
use std::{assert, assert_eq};

use super::*;
use crate::kern::uipc_socket::{soclose, socreate};
use crate::kern::uipc_socket2::{solock, sounlock};
use crate::net::if_::tests::test_packet;
use crate::netinet::in_pcb::tests::{setup, teardown};
use crate::netinet::udp_usrreq::udp_init;
use crate::sys::socket::SOCK_DGRAM;
use crate::sys::systm::{net_lock_shared, net_unlock_shared};

/// A control block of a detached internet control block, as `tcp_newtcpcb` leaves it.
fn tcpcb() -> &'static Tcpcb {
    let inp: &'static Inpcb = Box::leak(Box::new(Inpcb::new(None, None)));
    Box::leak(Box::new(Tcpcb::new(inp)))
}

/// A header with `flags` and acknowledgement `ack` (host order, as `tcp_input` leaves it).
fn th(flags: u8, seq: TcpSeq, ack: TcpSeq) -> Tcphdr {
    Tcphdr {
        th_seq: seq,
        th_ack: ack,
        th_flags: flags,
        ..Tcphdr::default()
    }
}

/// The pools the SACK holes and the reassembly queue come from, over fresh memory.
fn pools() {
    pool_init(
        &SACKHL_POOL,
        size_of::<Sackhole>(),
        0,
        IPL_SOFTNET,
        0,
        "sackhlpl",
        None,
    );
    pool_init(
        &TCPQE_POOL,
        size_of::<Tcpqent>(),
        0,
        IPL_SOFTNET,
        0,
        "tcpqe",
        None,
    );
}

/// The options of a SYN: MSS 1460, NOP, window scale 7, SACK permitted, timestamp 1000/0.
const SYN_OPTIONS: [u8; 20] = [
    TCPOPT_MAXSEG,
    4,
    0x05,
    0xb4, //
    TCPOPT_NOP,
    TCPOPT_WINDOW,
    3,
    7, //
    TCPOPT_SACK_PERMITTED,
    2, //
    TCPOPT_TIMESTAMP,
    10,
    0,
    0,
    0x03,
    0xe8,
    0,
    0,
    0,
    0,
];

#[test]
fn dooptions_takes_the_syn_options() {
    let _g = crate::kern::uipc_mbuf::tests::setup();
    let tp = tcpcb();
    tp.t_state.set(TCPS_LISTEN);
    tp.sack_enable.set(1);
    let m = test_packet(&[0; 40]);
    let mut oi = TcpOptInfo::default();

    assert!(tcp_dooptions(
        tp,
        &SYN_OPTIONS,
        &th(TH_SYN, 1, 0),
        m,
        20,
        &mut oi,
        0,
        77
    ));
    assert_eq!(oi.maxseg, 1460);
    assert!(oi.ts_present);
    assert_eq!((oi.ts_val, oi.ts_ecr), (1000, 0));
    let want = TF_RCVD_SCALE | TF_SACK_PERMIT | TF_RCVD_TSTMP;
    assert_eq!(tp.t_flags.get() & want, want);
    assert_eq!(tp.requested_s_scale.get(), 7);
    assert_eq!((tp.ts_recent.get(), tp.ts_recent_age.get()), (1000, 77));
    m_freem(m);
}

#[test]
fn dooptions_ignores_syn_options_once_synchronized() {
    let _g = crate::kern::uipc_mbuf::tests::setup();
    let tp = tcpcb();
    tp.t_state.set(TCPS_ESTABLISHED);
    tp.sack_enable.set(1);
    let m = test_packet(&[0; 40]);
    let mut oi = TcpOptInfo::default();

    // A window scale of 20 is also clamped to TCP_MAX_WINSHIFT on a SYN; here it is ignored.
    let mut opts = SYN_OPTIONS;
    opts[7] = 20;
    assert!(tcp_dooptions(
        tp,
        &opts,
        &th(TH_SYN, 1, 0),
        m,
        20,
        &mut oi,
        0,
        77
    ));
    assert_eq!(oi.maxseg, 0);
    // The timestamp is read on any segment, but only a SYN's is remembered.
    assert!(oi.ts_present);
    assert_eq!(tp.t_flags.get(), 0);
    assert_eq!(tp.ts_recent.get(), 0);

    // On a SYN in LISTEN the scale is clamped.
    tp.t_state.set(TCPS_LISTEN);
    assert!(tcp_dooptions(
        tp,
        &opts,
        &th(TH_SYN, 1, 0),
        m,
        20,
        &mut oi,
        0,
        77
    ));
    assert_eq!(tp.requested_s_scale.get(), TCP_MAX_WINSHIFT);
    m_freem(m);
}

#[test]
fn dooptions_stops_at_a_malformed_option() {
    let _g = crate::kern::uipc_mbuf::tests::setup();
    let tp = tcpcb();
    tp.t_state.set(TCPS_LISTEN);
    let m = test_packet(&[0; 40]);

    // A length below 2 ends the walk before the MSS behind it.
    let mut oi = TcpOptInfo::default();
    let opts = [TCPOPT_WINDOW, 1, TCPOPT_MAXSEG, 4, 0x02, 0x00];
    assert!(tcp_dooptions(
        tp,
        &opts,
        &th(TH_SYN, 1, 0),
        m,
        20,
        &mut oi,
        0,
        0
    ));
    assert_eq!(oi.maxseg, 0);

    // A length past the end too.
    let opts = [TCPOPT_MAXSEG, 8, 0x02, 0x00];
    assert!(tcp_dooptions(
        tp,
        &opts,
        &th(TH_SYN, 1, 0),
        m,
        20,
        &mut oi,
        0,
        0
    ));
    assert_eq!(oi.maxseg, 0);

    // A wrong length for the kind is skipped, and EOL ends the list.
    let opts = [
        TCPOPT_MAXSEG,
        3,
        0,
        TCPOPT_MAXSEG,
        4,
        0x02,
        0x18,
        TCPOPT_EOL,
        TCPOPT_MAXSEG,
        4,
        0,
        1,
    ];
    assert!(tcp_dooptions(
        tp,
        &opts,
        &th(TH_SYN, 1, 0),
        m,
        20,
        &mut oi,
        0,
        0
    ));
    assert_eq!(oi.maxseg, 0x218);

    // A lone kind byte at the end.
    let opts = [TCPOPT_NOP, TCPOPT_MAXSEG];
    let mut oi = TcpOptInfo::default();
    assert!(tcp_dooptions(
        tp,
        &opts,
        &th(TH_SYN, 1, 0),
        m,
        20,
        &mut oi,
        0,
        0
    ));
    assert_eq!(oi.maxseg, 0);
    m_freem(m);
}

#[test]
fn dooptions_rejects_an_unexpected_signature() {
    let _g = crate::kern::uipc_mbuf::tests::setup();
    let tp = tcpcb();
    tp.t_state.set(TCPS_ESTABLISHED);
    let m = test_packet(&[0; 40]);
    let mut oi = TcpOptInfo::default();

    let mut opts = [0u8; 18];
    opts[0] = TCPOPT_SIGNATURE;
    opts[1] = TCPOLEN_SIGNATURE;
    assert!(!tcp_dooptions(
        tp,
        &opts,
        &th(TH_ACK, 1, 1),
        m,
        20,
        &mut oi,
        0,
        0
    ));
    // The wrong length is not a signature at all.
    opts[1] = 17;
    assert!(tcp_dooptions(
        tp,
        &opts[..17],
        &th(TH_ACK, 1, 1),
        m,
        20,
        &mut oi,
        0,
        0
    ));
    m_freem(m);
}

#[test]
fn seq_subtract_and_timestamp_helpers_wrap() {
    assert_eq!(tcp_seq_subtract(10, 3), 7);
    assert_eq!(tcp_seq_subtract(3, 10), u64::MAX - 6);
    assert!(tstmp_lt(0xffff_fff0, 0x10));
    assert!(tstmp_geq(0x10, 0xffff_fff0));
    assert!(tstmp_geq(5, 5));
    assert_eq!(seq_min(0xffff_fff0, 0x10), 0xffff_fff0);
    assert_eq!(seq_max(0xffff_fff0, 0x10), 0x10);
}

/// The receiver's SACK blocks, `rcv_numsacks` of them.
fn blocks(tp: &Tcpcb) -> std::vec::Vec<(TcpSeq, TcpSeq)> {
    (0..tp.rcv_numsacks.get() as usize)
        .map(|i| {
            let b = tp.sackblks[i].get();
            (b.start, b.end)
        })
        .collect()
}

#[test]
fn update_sack_list_keeps_the_newest_block_first() {
    let tp = tcpcb();
    tp.rcv_nxt.set(1000);

    // In-order data needs no block.
    tcp_update_sack_list(tp, 1000, 1100);
    assert_eq!(blocks(tp), []);

    tcp_update_sack_list(tp, 2000, 2100);
    assert_eq!(blocks(tp), [(2000, 2100)]);
    tcp_update_sack_list(tp, 3000, 3100);
    assert_eq!(blocks(tp), [(3000, 3100), (2000, 2100)]);
    // Adjacent to the older block: merged, and moved to the front.
    tcp_update_sack_list(tp, 2100, 2200);
    assert_eq!(blocks(tp), [(2000, 2200), (3000, 3100)]);
    // Once rcv_nxt passes a block it is dropped.
    tp.rcv_nxt.set(2200);
    tcp_update_sack_list(tp, 4000, 4100);
    assert_eq!(blocks(tp), [(4000, 4100), (3000, 3100)]);
    // A repeated block moves to the front without growing the list.
    tcp_update_sack_list(tp, 3000, 3100);
    assert_eq!(blocks(tp), [(3000, 3100), (4000, 4100)]);

    // At most MAX_SACK_BLKS are kept: the oldest falls off.
    for i in 0..10u32 {
        tcp_update_sack_list(tp, 10000 + 200 * i, 10100 + 200 * i);
    }
    assert_eq!(blocks(tp).len(), MAX_SACK_BLKS);
    assert_eq!(blocks(tp)[0], (11800, 11900));

    tcp_clean_sackreport(tp);
    assert_eq!(blocks(tp), []);
    assert!(tp.sackblks.iter().all(|b| b.get() == Sackblk::default()));
}

/// The sender's holes, in list order.
fn holes(tp: &Tcpcb) -> std::vec::Vec<(TcpSeq, TcpSeq, i32, TcpSeq)> {
    let mut v = std::vec::Vec::new();
    let mut cur = tp.snd_holes.get();
    while let Some(h) = cur {
        v.push((h.start.get(), h.end.get(), h.dups.get(), h.rxmit.get()));
        cur = h.next.get();
    }
    v
}

/// A SACK option holding `blocks`.
fn sack_option(blocks: &[(TcpSeq, TcpSeq)]) -> std::vec::Vec<u8> {
    let mut o = std::vec![TCPOPT_SACK, (2 + 8 * blocks.len()) as u8];
    for &(s, e) in blocks {
        o.extend_from_slice(&s.to_be_bytes());
        o.extend_from_slice(&e.to_be_bytes());
    }
    o
}

#[test]
fn sack_option_tracks_the_holes() {
    let _g = crate::kern::uipc_mbuf::tests::setup();
    pools();
    let tp = tcpcb();
    tp.sack_enable.set(1);
    tp.t_state.set(TCPS_ESTABLISHED);
    tp.snd_una.set(1000);
    tp.snd_max.set(10000);
    tp.t_maxseg.set(100);
    let ack = th(TH_ACK, 1, 1000);

    // A SACK without ACK, or with a malformed length, is ignored.
    tcp_sack_option(tp, &th(0, 1, 1000), &sack_option(&[(2000, 3000)]));
    tcp_sack_option(tp, &ack, &sack_option(&[(2000, 3000)])[..9]);
    assert_eq!(holes(tp), []);

    // The first block opens a hole from the ACK up to it.
    tcp_sack_option(tp, &ack, &sack_option(&[(2000, 3000)]));
    assert_eq!(holes(tp), [(1000, 2000, 3, 1000)]);
    assert_eq!(tp.rcv_lastsack.get(), 3000);
    // A block past the last one appends a hole.
    tcp_sack_option(tp, &ack, &sack_option(&[(4000, 4100)]));
    // The hole before it counts one more dup, capped at tcprexmtthresh.
    assert_eq!(holes(tp), [(1000, 2000, 3, 1000), (3000, 4000, 1, 3000)]);
    assert_eq!(tp.snd_numholes.get(), 2);
    // The beginning of the first hole arrives, and the middle of the second splits it.
    tcp_sack_option(tp, &ack, &sack_option(&[(1000, 1500), (3400, 3600)]));
    assert_eq!(
        holes(tp),
        [
            (1500, 2000, 3, 1500),
            (3000, 3400, 2, 3000),
            (3600, 4000, 1, 3600)
        ]
    );
    assert_eq!(tp.snd_numholes.get(), 3);
    // A block covering a whole hole deletes it.
    tcp_sack_option(tp, &ack, &sack_option(&[(3000, 3400)]));
    assert_eq!(holes(tp), [(1500, 2000, 3, 1500), (3600, 4000, 1, 3600)]);

    // A cumulative ACK deletes the holes below it and trims the one it falls into.
    tcp_del_sackholes(tp, &th(TH_ACK, 1, 3700));
    assert_eq!(holes(tp), [(3700, 4000, 1, 3700)]);
    assert_eq!(tp.snd_numholes.get(), 1);
    tcp_del_sackholes(tp, &th(TH_ACK, 1, 4000));
    assert_eq!(holes(tp), []);
    assert_eq!(tp.snd_numholes.get(), 0);
}

#[test]
fn xmit_timer_smooths_the_rtt() {
    let tp = tcpcb();
    tp.t_rxtshift.set(3);
    tp.t_softerror.set(Some(Errno::EHOSTUNREACH));

    // The first sample: srtt = (rtt + 1) << 5, rttvar = (rtt + 1) << 3.
    tcp_xmit_timer(tp, 100);
    assert_eq!((tp.t_srtt.get(), tp.t_rttvar.get()), (101 << 5, 101 << 3));
    assert_eq!(tp.t_rxtcur.get(), ((404 + 808) >> 2));
    assert_eq!(tp.t_rxtshift.get(), 0);
    assert_eq!(tp.t_softerror.get(), None);

    // The second: delta = 800 - 404 = 396, rttvar += 396 - 202.
    tcp_xmit_timer(tp, 200);
    assert_eq!(
        (tp.t_srtt.get(), tp.t_rttvar.get()),
        (3232 + 396, 808 + 194)
    );
    assert_eq!(tp.t_rxtcur.get(), (((3232 + 396) >> 3) + 1002) >> 2);

    // Negative samples count as 0; the timeout never drops below rtt + 2 ticks.
    let tp = tcpcb();
    tcp_xmit_timer(tp, -5);
    assert_eq!(tp.t_srtt.get(), 1 << 5);
    assert_eq!(tp.t_rxtcur.get(), 2 * (tcp_time(1) / HZ));
    // Huge samples are clamped to TCP_RTT_MAX, the timeout to TCPTV_REXMTMAX.
    let tp = tcpcb();
    tcp_xmit_timer(tp, i32::MAX);
    assert_eq!(tp.t_srtt.get(), (TCP_RTT_MAX + 1) << 5);
    assert_eq!(tp.t_rxtcur.get(), TCPTV_REXMTMAX);
}

#[test]
fn hdrsz_counts_the_options_sent_on_every_segment() {
    let tp = tcpcb();
    tp.pf.set(i32::from(AF_INET));
    assert_eq!(tcp_hdrsz(tp), 40);
    tp.set_flags(TF_REQ_TSTMP | TF_RCVD_TSTMP);
    assert_eq!(tcp_hdrsz(tp), 52);
    tp.set_flags(TF_NOOPT);
    assert_eq!(tcp_hdrsz(tp), 40);
    tp.set_flags(TF_SIGNATURE);
    assert_eq!(tcp_hdrsz(tp), 60);
    tp.pf.set(0);
    assert_eq!(tcp_hdrsz(tp), 40);
}

/// A segment of `len` data bytes valued `v`.
fn segment(len: usize, v: u8) -> &'static Mbuf {
    test_packet(&std::vec![v; len])
}

#[test]
fn reass_queues_out_of_order_data_until_the_hole_fills() {
    let (_g, _t, _p) = setup();
    udp_init();
    pools();
    let so = socreate(i32::from(AF_INET), SOCK_DGRAM, 0).expect("socket");
    let inp = sotoinpcb(so).expect("inpcb");
    let tp: &'static Tcpcb = Box::leak(Box::new(Tcpcb::new(inp)));
    tp.t_state.set(TCPS_ESTABLISHED);
    tp.rcv_nxt.set(1000);
    solock(so);

    // 1010..1020 and 1030..1040 wait for 1000..1010.
    let mut h = th(TH_ACK, 1010, 0);
    let mut len = 10;
    assert_eq!(tcp_reass(tp, &mut h, segment(10, 2), &mut len), Ok(0));
    let mut h = th(TH_ACK | TH_FIN, 1030, 0);
    let mut len = 10;
    assert_eq!(tcp_reass(tp, &mut h, segment(10, 4), &mut len), Ok(0));
    assert_eq!(tp.rcv_nxt.get(), 1000);
    assert_eq!(tp.t_rcvoopack.get(), 2);

    // 1015..1035 overlaps both: its head is trimmed by the earlier segment, and the later
    // one is trimmed in turn to start where it ends.
    let mut h = th(TH_ACK, 1015, 0);
    let mut len = 20;
    assert_eq!(tcp_reass(tp, &mut h, segment(20, 3), &mut len), Ok(0));
    assert_eq!((h.th_seq, len), (1020, 15));
    let seqs: std::vec::Vec<(TcpSeq, u16)> = tp
        .t_segq
        .iter()
        .map(|q| (q.tcpqe_tcp.get().th_seq, q.tcpqe_tcp.get().th_reseqlen()))
        .collect();
    assert_eq!(seqs, [(1010, 10), (1020, 15), (1035, 5)]);

    // A duplicate of queued data is dropped.
    let mut h = th(TH_ACK, 1012, 0);
    let mut len = 5;
    assert_eq!(tcp_reass(tp, &mut h, segment(5, 9), &mut len), Ok(0));
    assert_eq!(tp.t_segq.iter().count(), 3);

    // The missing head flushes everything, up to and with the FIN.
    let mut h = th(TH_ACK, 1000, 0);
    let mut len = 10;
    assert_eq!(tcp_reass(tp, &mut h, segment(10, 1), &mut len), Ok(TH_FIN));
    assert_eq!(tp.rcv_nxt.get(), 1040);
    assert!(tp.t_segq.is_empty());
    assert_eq!(so.so_rcv.sb_cc.get(), 40);
    let mut data = [0u8; 40];
    m_copydata(so.so_rcv.sb_mb.get().expect("data"), 0, &mut data);
    assert_eq!(data[..10], [1; 10]);
    assert_eq!(data[10..20], [2; 10]);
    assert_eq!(data[20..35], [3; 15]);
    assert_eq!(data[35..], [4; 5]);

    sounlock(so);
    soclose(so, 0).expect("close");
    teardown();
}

#[test]
fn reass_waits_in_syn_received_for_data() {
    let (_g, _t, _p) = setup();
    udp_init();
    pools();
    let so = socreate(i32::from(AF_INET), SOCK_DGRAM, 0).expect("socket");
    let inp = sotoinpcb(so).expect("inpcb");
    let tp: &'static Tcpcb = Box::leak(Box::new(Tcpcb::new(inp)));
    tp.rcv_nxt.set(500);
    solock(so);

    // Before ESTABLISHED nothing is presented, even in sequence.
    tp.t_state.set(TCPS_SYN_RECEIVED);
    let mut h = th(TH_ACK, 500, 0);
    let mut len = 8;
    assert_eq!(tcp_reass(tp, &mut h, segment(8, 7), &mut len), Ok(0));
    assert_eq!(tp.rcv_nxt.get(), 500);
    assert_eq!(tcp_flush_queue(tp), 0);

    // tcp_flush_queue presents it once the connection is established.
    tp.t_state.set(TCPS_ESTABLISHED);
    assert_eq!(tcp_flush_queue(tp), 0);
    assert_eq!(tp.rcv_nxt.get(), 508);
    assert_eq!(so.so_rcv.sb_cc.get(), 8);

    sounlock(so);
    soclose(so, 0).expect("close");
    teardown();
}

/// `addr:port` as the SYN cache keeps it.
fn sa(addr: [u8; 4], port: u16) -> SynCacheSa {
    SockaddrUnion::from_sin(&SockaddrIn {
        sin_len: size_of::<SockaddrIn>() as u8,
        sin_family: AF_INET,
        sin_port: port.to_be(),
        sin_addr: crate::netinet::in_::InAddr {
            s_addr: u32::from_ne_bytes(addr),
        },
        ..SockaddrIn::default()
    })
}

#[test]
fn syn_cache_hash_mixes_ports_and_source() {
    let src = sa([10, 0, 0, 1], 1234);
    let dst = sa([10, 0, 0, 2], 80);
    let rand = [0x1111_1111, 2, 3, 4, 0x5555_5555];
    let sport = u32::from(1234u16.to_be());
    let dport = u32::from(80u16.to_be());
    let addr = u32::from_ne_bytes([10, 0, 0, 1]);
    assert_eq!(
        syn_cache_hash(&src, &dst, &rand),
        (((dport << 16).wrapping_add(sport)) ^ rand[4]).wrapping_mul(addr ^ rand[0])
    );
    // Only the source address enters the product.
    let other = sa([10, 0, 0, 3], 80);
    assert_eq!(
        syn_cache_hash(&src, &dst, &rand),
        syn_cache_hash(&src, &other, &rand)
    );
}

/// A fresh entry for `src -> dst`, listening on `tp`, as `syn_cache_add` fills it.
fn entry(tp: &'static Tcpcb, src: &SynCacheSa, dst: &SynCacheSa, irs: TcpSeq) -> &'static SynCache {
    let sc = syn_cache_alloc().expect("syn cache entry");
    refcnt_init_trace(&sc.sc_refcnt, DT_REFCNT_IDX_SYNCACHE);
    timeout_set_flags(
        &sc.sc_timer,
        syn_cache_timer,
        ptr::from_ref(sc).cast_mut().cast(),
        KCLOCK_NONE,
        TIMEOUT_PROC | TIMEOUT_MPSAFE,
    );
    sc.sc_src.set(*src);
    sc.sc_dst.set(*dst);
    sc.sc_irs.set(irs);
    sc.sc_inplisten.set(in_pcbref(Some(tp.t_inpcb)));
    sc
}

#[test]
fn syn_cache_inserts_finds_resets_and_cleans_up() {
    let (_g, _t, _p) = setup();
    udp_init();
    net_lock_shared();
    TCP_SYN_CACHE_ACTIVE.store(0, Ordering::Relaxed);
    for set in &TCP_SYN_CACHE {
        set.scs_count.set(0);
        set.scs_use.set(0);
    }
    syn_cache_init();
    let so = socreate(i32::from(AF_INET), SOCK_DGRAM, 0).expect("socket");
    let inp = sotoinpcb(so).expect("inpcb");
    let tp: &'static Tcpcb = Box::leak(Box::new(Tcpcb::new(inp)));

    let src = sa([10, 0, 0, 1], 1234);
    let dst = sa([10, 0, 0, 2], 80);
    let src2 = sa([10, 0, 0, 1], 1235);

    mtx_enter(&SYN_CACHE_MTX);
    syn_cache_insert(entry(tp, &src, &dst, 5000), tp);
    syn_cache_insert(entry(tp, &src2, &dst, 6000), tp);
    let set = &TCP_SYN_CACHE[0];
    assert_eq!(set.scs_count.get(), 2);
    assert_eq!(
        set.scs_use.get(),
        i64::from(TCP_SYN_USE_LIMIT.load(Ordering::Relaxed)) - 2
    );
    let sc = syn_cache_lookup(&src, &dst, 0).expect("found");
    assert_eq!(sc.sc_irs.get(), 5000);
    assert!(sc.sc_set.get().is_some_and(|s| ptr::eq(s, set)));
    // The retransmit timer starts at the default RTT.
    assert_eq!(sc.sc_rxtcur.get(), TCPTV_SRTTDFLT as u32);
    assert!(syn_cache_lookup(&dst, &src, 0).is_none());
    mtx_leave(&SYN_CACHE_MTX);

    // A RST outside [irs, irs + 1] leaves the entry; one inside removes it.
    syn_cache_reset(&src, &dst, &th(TH_RST, 4000, 0), 0);
    assert_eq!(set.scs_count.get(), 2);
    syn_cache_reset(&src, &dst, &th(TH_RST, 5001, 0), 0);
    assert_eq!(set.scs_count.get(), 1);
    mtx_enter(&SYN_CACHE_MTX);
    assert!(syn_cache_lookup(&src, &dst, 0).is_none());
    assert!(syn_cache_lookup(&src2, &dst, 0).is_some());
    mtx_leave(&SYN_CACHE_MTX);

    // The listener going away takes its entries along.
    syn_cache_cleanup(tp);
    assert_eq!(set.scs_count.get(), 0);
    assert!(tp.t_sc.is_empty());

    net_unlock_shared();
    soclose(so, 0).expect("close");
    teardown();
}
