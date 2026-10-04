//! Host tests for TCP output: the SACK hole walks (`tcp_sack_output`, `tcp_sack_adjust`),
//! the persist timer's back-off and range (`tcp_setpersist`), `tcp_output`'s decisions that
//! send nothing (an idle connection, the retransmit timer, a window shrunk to zero, an
//! unsupported family), software TSO (`tcp_softtso_chop`: segment sizes, sequence numbers,
//! flags, both checksums; malformed packets) and `tcp_if_output_tso`'s four ways (no TSO,
//! segments over the MTU, software, hardware).

use std::boxed::Box;
use std::sync::{Mutex as StdMutex, MutexGuard};
use std::vec::Vec;
use std::{assert, assert_eq, vec};

use super::*;
use crate::kern::kern_clock::ticks;
use crate::kern::kern_timeout::timeout_set;
use crate::kern::uipc_socket::soalloc;
use crate::net::if_::tests::test_ifnet;
use crate::netinet::in_pcb::Inpcb;
use crate::netinet::in_proto::INETSW;
use crate::netinet::tcp_fsm::TCPS_ESTABLISHED;
use crate::netinet::tcp_timer::TCPT_NTIMERS;
use crate::netinet::tcp_var::{TF_TMR_PERSIST, TF_TMR_REXMT};
use crate::sys::mbuf::{M_WAIT, MCLBYTES, MT_DATA};

/// A timer callout that is never expected to run.
fn no_callout(_: *mut core::ffi::c_void) {}

/// A control block on a fresh `inp` (with socket `so`), its timers set up.
fn tcb(so: Option<&'static Socket>) -> &'static Tcpcb {
    let inp: &'static Inpcb = Box::leak(Box::new(Inpcb::new(None, so)));
    let tp: &'static Tcpcb = Box::leak(Box::new(Tcpcb::new(inp)));
    for t in 0..TCPT_NTIMERS {
        timeout_set(&tp.t_timer[t], no_callout, ptr::null_mut());
    }
    tp
}

/// Disarms every timer of `tp` (they hold references and sit on the timeout queue).
fn disarm_all(tp: &Tcpcb) {
    for t in 0..TCPT_NTIMERS {
        tcp_timer_disarm(tp, t);
    }
}

/// The timeout lock and a started timeout subsystem.
fn timeouts() -> MutexGuard<'static, ()> {
    let g = crate::kern::kern_timeout::tests::LOCK
        .lock()
        .unwrap_or_else(|e| e.into_inner());
    crate::kern::kern_timeout::timeout_startup();
    g
}

/// A hole `start..end` with `dups` duplicate ACKs and retransmission point `rxmit`, linked
/// before `next`.
fn hole(
    start: u32,
    end: u32,
    dups: i32,
    rxmit: u32,
    next: Option<&'static Sackhole>,
) -> &'static Sackhole {
    let h: &'static Sackhole = Box::leak(Box::new(Sackhole::new()));
    h.start.set(start);
    h.end.set(end);
    h.dups.set(dups);
    h.rxmit.set(rxmit);
    h.next.set(next);
    h
}

#[test]
fn sack_output_returns_the_first_hole_to_retransmit() {
    let tp = tcb(None);
    tp.snd_una.set(1000);
    // 1000..1100 retransmitted completely, 1200..1300 below the threshold, 1400..1500 to
    // resend from 1450, 1600..1700 also eligible.
    let h4 = hole(1600, 1700, TCPREXMTTHRESH, 1600, None);
    let h3 = hole(1400, 1500, TCPREXMTTHRESH, 1450, Some(h4));
    let h2 = hole(1200, 1300, TCPREXMTTHRESH - 1, 1200, Some(h3));
    let h1 = hole(1000, 1100, TCPREXMTTHRESH + 2, 1100, Some(h2));
    tp.snd_holes.set(Some(h1));

    assert!(tcp_sack_output(tp).is_none(), "SACK is off");
    tp.sack_enable.set(1);
    assert!(ptr::eq(tcp_sack_output(tp).expect("hole"), h3));

    // A hole whose retransmission point fell behind snd_una is an old one.
    tp.snd_una.set(1460);
    h3.rxmit.set(1455);
    assert!(ptr::eq(tcp_sack_output(tp).expect("hole"), h4));

    // Sequence numbers compare modulo 2^32.
    let w = hole(0xffff_ff00, 0x0000_0100, TCPREXMTTHRESH, 0xffff_fff0, None);
    tp.snd_una.set(0xffff_ff00);
    tp.snd_holes.set(Some(w));
    assert!(ptr::eq(tcp_sack_output(tp).expect("hole"), w));
}

#[test]
fn sack_adjust_skips_sacked_data() {
    let tp = tcb(None);
    tcp_sack_adjust(tp); // no holes: nothing to do
    let h2 = hole(3000, 4000, 0, 3000, None);
    let h1 = hole(1000, 2000, 0, 1000, Some(h2));
    tp.snd_holes.set(Some(h1));
    tp.rcv_lastsack.set(5000);

    // Inside a hole: stay.
    tp.snd_nxt.set(1500);
    tcp_sack_adjust(tp);
    assert_eq!(tp.snd_nxt.get(), 1500);
    // i) between the end of one hole and the start of the next: to the next hole.
    tp.snd_nxt.set(2500);
    tcp_sack_adjust(tp);
    assert_eq!(tp.snd_nxt.get(), 3000);
    // Inside the last hole: stay.
    tp.snd_nxt.set(3500);
    tcp_sack_adjust(tp);
    assert_eq!(tp.snd_nxt.get(), 3500);
    // ii) between the end of the last hole and rcv_lastsack: to rcv_lastsack.
    tp.snd_nxt.set(4500);
    tcp_sack_adjust(tp);
    assert_eq!(tp.snd_nxt.get(), 5000);
    // Beyond every SACKed block: stay.
    tp.snd_nxt.set(6000);
    tcp_sack_adjust(tp);
    assert_eq!(tp.snd_nxt.get(), 6000);
}

/// The ticks `tcp_timer_arm` asked for `msec` (`timeout_add_ticks` adds one).
fn persist_ticks(tp: &Tcpcb) -> i32 {
    tp.t_timer[TCPT_PERSIST].to_time.get().wrapping_sub(ticks())
}

fn ticks_for(msec: i32) -> i32 {
    let tick = crate::conf::param::TICK.load(Ordering::Relaxed) as u32;
    (msec as u32 * 1000).div_ceil(tick) as i32 + 1
}

#[test]
fn setpersist_backs_off_within_range() {
    let _g = timeouts();
    let tp = tcb(None);

    // ((800 >> 2) + 100) >> 3 = 37 ms: below TCPTV_PERSMIN.
    tp.t_srtt.set(800);
    tp.t_rttvar.set(100);
    tp.t_rttmin.set(10);
    tcp_setpersist(tp);
    assert!(tp.has_flags(TF_TMR_PERSIST));
    assert_eq!(tp.t_rxtshift.get(), 1);
    assert_eq!(persist_ticks(tp), ticks_for(TCPTV_PERSMIN));

    // t_rttmin raises a small estimate: 7000 ms, backed off twice (x2 at shift 1).
    tp.t_rttmin.set(7000);
    tcp_setpersist(tp);
    assert_eq!(tp.t_rxtshift.get(), 2);
    assert_eq!(persist_ticks(tp), ticks_for(14_000));

    // At the maximum shift the value is clamped and the shift stays.
    tp.t_rxtshift.set(TCP_MAXRXTSHIFT as i16);
    tcp_setpersist(tp);
    assert_eq!(i32::from(tp.t_rxtshift.get()), TCP_MAXRXTSHIFT);
    assert_eq!(persist_ticks(tp), ticks_for(TCPTV_PERSMAX));
    disarm_all(tp);
}

/// An established connection on a socket with empty buffers.
fn established() -> &'static Tcpcb {
    let so = soalloc(&INETSW[1], M_WAIT).expect("socket");
    let tp = tcb(Some(so));
    tp.t_state.set(TCPS_ESTABLISHED);
    tp.t_maxseg.set(512);
    tp.t_maxopd.set(512);
    tp.t_rxtcur.set(1000);
    for s in [&tp.snd_una, &tp.snd_nxt, &tp.snd_max, &tp.snd_up] {
        s.set(1000);
    }
    tp
}

#[test]
fn output_without_a_reason_sends_nothing() {
    let (_g, _t, _p) = crate::netinet::in_pcb::tests::setup();

    // Idle, nothing to send, nothing owed.
    let tp = established();
    tp.set_flags(TF_LASTIDLE);
    tp.t_rcvtime.set(tcp_now());
    tp.t_rxtcur.set(0); // idle for "a while": at least t_rxtcur since the last segment
    assert_eq!(tcp_output(tp), Ok(()));
    assert!(!tp.has_flags(TF_LASTIDLE | TF_TMR_REXMT | TF_TMR_PERSIST));
    assert_eq!(tp.snd_cwnd.get(), 2 * 512, "idle restarts slow start");

    // Data in flight but no timer: the retransmit timer is armed.
    let tp = established();
    tp.snd_wnd.set(65535);
    tp.snd_cwnd.set(65535);
    tp.snd_nxt.set(1100);
    tp.snd_max.set(1100);
    assert_eq!(tcp_output(tp), Ok(()));
    assert!(tp.has_flags(TF_TMR_REXMT));
    assert!(!tp.has_flags(TF_TMR_PERSIST));
    assert_eq!(tp.snd_nxt.get(), 1100);
    disarm_all(tp);

    // The window shrank to zero after we sent into it: snd_nxt is pulled back and the
    // persist timer runs instead of the retransmit timer.
    let tp = established();
    tp.snd_nxt.set(1100);
    tp.snd_max.set(1100);
    tp.t_rxtshift.set(3);
    assert_eq!(tcp_output(tp), Ok(()));
    assert_eq!(tp.snd_nxt.get(), 1000);
    assert!(tp.has_flags(TF_TMR_PERSIST));
    assert!(!tp.has_flags(TF_TMR_REXMT));
    assert_eq!(tp.t_rxtshift.get(), 1, "reset, then one back-off step");
    disarm_all(tp);

    // An ACK is owed but the family is not one we can build headers for.
    let tp = established();
    tp.set_flags(TF_ACKNOW);
    tp.pf.set(99);
    assert_eq!(tcp_output(tp), Err(Errno::EPFNOSUPPORT));

    crate::netinet::in_pcb::tests::teardown();
}

const SRC: [u8; 4] = [10, 0, 2, 15];
const DST: [u8; 4] = [10, 0, 2, 2];
const SEQ: u32 = 0xffff_fe00; // wraps inside the packet

/// A TSO packet: IPv4 (with header length `hl` words and `ip_off`), TCP with flags
/// PUSH|ACK|FIN and sequence [`SEQ`], `payload` bytes; `ph_mss` is `mss`.
fn tso_packet(payload: usize, mss: u16, hl: u8, ip_off: u16) -> &'static Mbuf {
    let iphlen = usize::from(hl) * 4;
    let len = iphlen + 20 + payload;
    let mut p = vec![0x40 | hl, 0];
    p.extend_from_slice(&(len as u16).to_be_bytes());
    p.extend_from_slice(&[0, 0]);
    p.extend_from_slice(&ip_off.to_be_bytes());
    p.extend_from_slice(&[64, IPPROTO_TCP as u8, 0, 0]);
    p.extend_from_slice(&SRC);
    p.extend_from_slice(&DST);
    p.resize(iphlen, 1); // option bytes (NOPs)
    p.extend_from_slice(&1234u16.to_be_bytes());
    p.extend_from_slice(&80u16.to_be_bytes());
    p.extend_from_slice(&SEQ.to_be_bytes());
    p.extend_from_slice(&7u32.to_be_bytes());
    p.extend_from_slice(&[5 << 4, TH_PUSH | TH_ACK | TH_FIN]);
    p.extend_from_slice(&[0xff, 0xff, 0, 0, 0, 0]);
    p.extend((0..payload).map(|i| i as u8));
    assert!(p.len() <= MCLBYTES);

    let m = m_gethdr(M_DONTWAIT, MT_DATA).expect("mbuf");
    mclget(m, M_DONTWAIT);
    assert!(m.m_flags().get() & M_EXT != 0);
    // SAFETY: a cluster of MCLBYTES bytes at m_data, which the packet fits.
    unsafe { ptr::copy_nonoverlapping(p.as_ptr(), mtod::<u8>(m), p.len()) };
    m.m_len().set(p.len() as u32);
    m.m_pkthdr().len.set(p.len() as i32);
    m.m_pkthdr().csum_flags.set(M_TCP_TSO | M_TCP_CSUM_OUT);
    m.m_pkthdr().ph_mss.set(mss);
    m
}

/// The bytes of packet `m`.
fn bytes(m: &Mbuf) -> Vec<u8> {
    let mut v = vec![0u8; m.m_pkthdr().len.get() as usize];
    m_copydata(m, 0, &mut v);
    v
}

/// The ones' complement sum of `words` (16-bit, big-endian; an odd byte padded), folded.
fn sum16(words: &[u8]) -> u16 {
    let mut sum: u32 = words
        .chunks(2)
        .map(|c| u32::from(u16::from_be_bytes([c[0], *c.get(1).unwrap_or(&0)])))
        .sum();
    while sum > 0xffff {
        sum = (sum & 0xffff) + (sum >> 16);
    }
    sum as u16
}

/// Checks that `segs` are `payload` bytes cut at `mss`, with consecutive sequence numbers,
/// PUSH and FIN only on the last, the IP lengths and both checksums right.
fn check_segments(segs: &[Vec<u8>], payload: usize, mss: usize) {
    assert_eq!(segs.len(), payload.div_ceil(mss));
    for (k, b) in segs.iter().enumerate() {
        let dlen = min(mss, payload - k * mss);
        assert_eq!(b.len(), 40 + dlen, "segment {k}");
        assert_eq!(u16::from_be_bytes([b[2], b[3]]) as usize, 40 + dlen);
        assert_eq!(&b[12..20], &[SRC, DST].concat()[..]);
        let seq = u32::from_be_bytes([b[24], b[25], b[26], b[27]]);
        assert_eq!(seq, SEQ.wrapping_add((k * mss) as u32), "segment {k}");
        let last = k + 1 == segs.len();
        let want = if last {
            TH_PUSH | TH_ACK | TH_FIN
        } else {
            TH_ACK
        };
        assert_eq!(b[33], want, "flags of segment {k}");
        for (i, &x) in b[40..].iter().enumerate() {
            assert_eq!(x, (k * mss + i) as u8);
        }
        assert_eq!(sum16(&b[..20]), 0xffff, "IP checksum of segment {k}");
        let mut pseudo = [SRC, DST].concat();
        pseudo.extend_from_slice(&[0, IPPROTO_TCP as u8]);
        pseudo.extend_from_slice(&((20 + dlen) as u16).to_be_bytes());
        pseudo.extend_from_slice(&b[20..]);
        assert_eq!(sum16(&pseudo), 0xffff, "TCP checksum of segment {k}");
    }
}

#[test]
fn softtso_chop_cuts_at_mss() {
    let (_g, _t) = crate::netinet::ip_input::tests::setup();
    let ifp = test_ifnet(b"ttso0");

    let m0 = tso_packet(1300, 500, 5, 0);
    let ml = MbufList::new();
    assert_eq!(tcp_softtso_chop(&ml, m0, ifp, 500), Ok(()));
    assert_eq!(ml_len(&ml), 3);
    let mut segs = Vec::new();
    while let Some(m) = ml_dequeue(&ml) {
        segs.push(m);
    }
    assert!(
        ptr::eq(segs[0], m0),
        "the first segment is the original packet"
    );
    for m in &segs {
        assert_eq!(m.m_pkthdr().csum_flags.get() & M_TCP_TSO, 0);
    }
    check_segments(
        &segs.iter().map(|m| bytes(m)).collect::<Vec<_>>(),
        1300,
        500,
    );
    for m in segs {
        m_freem(m);
    }

    // A packet that fits in one segment keeps its flags.
    let m0 = tso_packet(300, 500, 5, 0);
    assert_eq!(tcp_softtso_chop(&ml, m0, ifp, 500), Ok(()));
    let m = ml_dequeue(&ml).expect("segment");
    assert!(ml_dequeue(&ml).is_none());
    check_segments(&[bytes(m)], 300, 500);
    m_freem(m);
}

#[test]
fn softtso_chop_rejects_what_it_cannot_cut() {
    let (_g, _t) = crate::netinet::ip_input::tests::setup();
    let ifp = test_ifnet(b"ttso1");
    let ml = MbufList::new();

    let cases = [
        (tso_packet(1000, 500, 5, 0), 0, Errno::EINVAL),
        (tso_packet(1000, 500, 5, IP_MF), 500, Errno::EPROTOTYPE),
        (tso_packet(1000, 500, 5, 1), 500, Errno::EPROTOTYPE),
        (tso_packet(1000, 500, 6, 0), 500, Errno::EPROTOTYPE),
    ];
    for (m0, mss, error) in cases {
        assert_eq!(tcp_softtso_chop(&ml, m0, ifp, mss), Err(error));
        assert_eq!(ml_len(&ml), 0, "the list is purged");
    }

    // Shorter than the headers it claims.
    let m0 = tso_packet(0, 500, 5, 0);
    m0.m_pkthdr().len.set(30);
    assert_eq!(tcp_softtso_chop(&ml, m0, ifp, 500), Err(Errno::ENOPROTOOPT));
    assert_eq!(ml_len(&ml), 0);
}

/// What the test interfaces' `if_output` saw: (interface, packet bytes).
static SENT: StdMutex<Vec<(usize, Vec<u8>)>> = StdMutex::new(Vec::new());

/// An `if_output` that records the packet and frees it.
///
/// # Safety
///
/// None beyond the type's: `dst` is not read.
unsafe fn record_output(
    ifp: &'static Ifnet,
    m: &'static Mbuf,
    _dst: *const Sockaddr,
    _rt: Option<&'static Rtentry>,
) -> Result<(), Errno> {
    SENT.lock()
        .unwrap_or_else(|e| e.into_inner())
        .push((ptr::from_ref(ifp) as usize, bytes(m)));
    m_freem(m);
    Ok(())
}

/// The packets `ifp` sent, taken out of [`SENT`].
fn sent_by(ifp: &Ifnet) -> Vec<Vec<u8>> {
    let mut all = SENT.lock().unwrap_or_else(|e| e.into_inner());
    let me = ptr::from_ref(ifp) as usize;
    let mine = all
        .iter()
        .filter(|(i, _)| *i == me)
        .map(|(_, b)| b.clone())
        .collect();
    all.retain(|(i, _)| *i != me);
    mine
}

/// `tcp_if_output_tso` to `ifp` with the IPv4 TSO capability bit and `mtu`.
fn tso_out(ifp: &'static Ifnet, mp: &mut Option<&'static Mbuf>, mtu: u32) -> Result<(), Errno> {
    let dst = Sockaddr::default();
    // SAFETY: `dst` is a readable socket address; the test output routine does not read it.
    unsafe { tcp_if_output_tso(ifp, mp, &dst, None, IFCAP_TSOv4, mtu) }
}

#[test]
fn if_output_tso_chooses_the_way_out() {
    let (_g, _t) = crate::netinet::ip_input::tests::setup();
    let ifp = test_ifnet(b"ttso2");
    ifp.if_output.set(Some(record_output));

    // Not a TSO packet: left to the caller.
    let m = tso_packet(1000, 500, 5, 0);
    m.m_pkthdr().csum_flags.set(M_TCP_CSUM_OUT);
    let mut mp = Some(m);
    assert_eq!(tso_out(ifp, &mut mp, 1500), Ok(()));
    assert!(mp.is_some_and(|x| ptr::eq(x, m)));

    // Segments larger than the MTU: TSO is cleared, the caller fragments or drops.
    m.m_pkthdr().csum_flags.set(M_TCP_TSO | M_TCP_CSUM_OUT);
    assert_eq!(tso_out(ifp, &mut mp, 400), Ok(()));
    assert!(mp.is_some());
    assert_eq!(m.m_pkthdr().csum_flags.get() & M_TCP_TSO, 0);
    m_freem(m);
    assert!(sent_by(ifp).is_empty());

    // No hardware TSO: chopped in software.
    let mut mp = Some(tso_packet(1300, 500, 5, 0));
    assert_eq!(tso_out(ifp, &mut mp, 1500), Ok(()));
    assert!(mp.is_none());
    check_segments(&sent_by(ifp), 1300, 500);

    // Hardware TSO: the whole packet goes to the interface.
    ifp.if_capabilities.set(IFCAP_TSOv4);
    let mut mp = Some(tso_packet(1300, 500, 5, 0));
    assert_eq!(tso_out(ifp, &mut mp, 1500), Ok(()));
    assert!(mp.is_none());
    let out = sent_by(ifp);
    assert_eq!(out.len(), 1);
    assert_eq!(out[0].len(), 1340);
}
