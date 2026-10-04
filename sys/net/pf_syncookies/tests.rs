//! Host tests for the SYN cookies: a cookie made for a SYN validates on the matching ACK and
//! not on another; the mode and watermark ioctls check their bounds.

use std::sync::{Mutex as StdMutex, MutexGuard};
use std::{assert, assert_eq};

use super::*;
use crate::net::pfvar_priv::PfLoc;

/// Serialises the tests: they share `pf_status` and the secrets.
static LOCK: StdMutex<()> = StdMutex::new(());

fn lock() -> MutexGuard<'static, ()> {
    LOCK.lock().unwrap_or_else(|e| e.into_inner())
}

/// The client's and the server's addresses, side by side.
struct Addrs([u8; 8]);

/// A TCP descriptor from 192.168.1.5:40000 to 10.0.0.1:80 with `seq`, `ack` and `flags`;
/// its addresses are in `addrs`, which must outlive it.
fn tcp_pd(addrs: &mut Addrs, seq: u32, ack: u32, flags: u8) -> PfPdesc {
    addrs.0 = [192, 168, 1, 5, 10, 0, 0, 1];
    let mut pd = PfPdesc::new();
    pd.af = AF_INET;
    pd.proto = IPPROTO_TCP as u8;
    // SAFETY: `addrs` outlives the descriptor in every test and is not touched while it is
    // used.
    pd.src = unsafe { PfLoc::local(addrs.0.as_mut_ptr()) };
    pd.dst = pd.src.offset(4);
    pd.sport = PfLoc::Hdr(0);
    pd.dport = PfLoc::Hdr(2);
    let th = pd.tcp_mut();
    th.th_sport = 40000u16.to_be();
    th.th_dport = 80u16.to_be();
    th.th_seq = seq.to_be();
    th.th_ack = ack.to_be();
    th.th_flags = flags;
    th.set_th_off(5);
    pd
}

/// Fixed secrets, the even one current.
fn keys() {
    PF_SYNCOOKIE_STATUS.oddeven.store(0, Ordering::Relaxed);
    PF_SYNCOOKIE_STATUS.key[0].set(SiphashKey {
        k0: 0x0123_4567_89ab_cdef,
        k1: 0xfedc_ba98_7654_3210,
    });
    PF_SYNCOOKIE_STATUS.key[1].set(SiphashKey { k0: 1, k1: 2 });
}

#[test]
fn cookie_layout() {
    let mut c = PfSyncookie { cookie: 0 };
    c.set_oddeven(1);
    c.set_sack_ok(1);
    c.set_wscale_idx(5);
    c.set_mss_idx(7);
    assert_eq!(c.cookie, 0b1111_0111);
    assert_eq!(
        (c.oddeven(), c.sack_ok(), c.wscale_idx(), c.mss_idx()),
        (1, 1, 5, 7)
    );
}

#[test]
fn generated_cookie_validates() {
    let _g = lock();
    keys();
    let mut a1 = Addrs([0; 8]);
    let mut syn = tcp_pd(&mut a1, 1000, 0, TH_SYN);
    let iss = pf_syncookie_generate(&mut syn, 1460);
    // MSS 1460 is the last entry of the table; no window scale option (0) stops the C's
    // downward search at index 1, the first entry not above it.
    let cookie = PfSyncookie {
        cookie: ((iss & 0xff) ^ (iss >> 24)) as u8,
    };
    assert_eq!(cookie.mss_idx(), 7);
    assert_eq!(cookie.wscale_idx(), 1);
    assert_eq!(cookie.oddeven(), 0);

    PF_STATUS.syncookies_inflight[0].set(1);
    let mut a2 = Addrs([0; 8]);
    let mut ack = tcp_pd(&mut a2, 1001, iss.wrapping_add(1), TH_ACK);
    assert_eq!(pf_syncookie_validate(&mut ack), 1);
    assert_eq!(PF_STATUS.syncookies_inflight[0].get(), 0);
}

#[test]
fn wrong_ack_does_not_validate() {
    let _g = lock();
    keys();
    let mut a1 = Addrs([0; 8]);
    let mut syn = tcp_pd(&mut a1, 5000, 0, TH_SYN);
    let iss = pf_syncookie_generate(&mut syn, 536);

    PF_STATUS.syncookies_inflight[0].set(1);
    PF_STATUS.syncookies_inflight[1].set(1);
    // The MAC bits differ.
    let mut a2 = Addrs([0; 8]);
    let mut ack = tcp_pd(&mut a2, 5001, (iss ^ 0x0100_0000).wrapping_add(1), TH_ACK);
    assert_eq!(pf_syncookie_validate(&mut ack), 0);
    // The sequence number differs.
    let mut a3 = Addrs([0; 8]);
    let mut ack = tcp_pd(&mut a3, 7001, iss.wrapping_add(1), TH_ACK);
    assert_eq!(pf_syncookie_validate(&mut ack), 0);
    // Nothing in flight under the cookie's secret.
    PF_STATUS.syncookies_inflight[0].set(0);
    let mut a4 = Addrs([0; 8]);
    let mut ack = tcp_pd(&mut a4, 5001, iss.wrapping_add(1), TH_ACK);
    assert_eq!(pf_syncookie_validate(&mut ack), 0);
    PF_STATUS.syncookies_inflight[1].set(0);
}

#[test]
fn setmode_and_watermarks_check_bounds() {
    let _g = lock();
    assert_eq!(
        pf_syncookies_setmode(PF_SYNCOOKIES_MODE_MAX + 1),
        Err(Errno::EINVAL)
    );
    assert_eq!(pf_syncookies_setmode(PF_SYNCOOKIES_ADAPTIVE), Ok(()));
    assert_eq!(PF_STATUS.syncookies_mode.get(), PF_SYNCOOKIES_ADAPTIVE);
    assert_eq!(pf_syncookies_setmode(PF_SYNCOOKIES_NEVER), Ok(()));
    assert_eq!(PF_STATUS.syncookies_mode.get(), PF_SYNCOOKIES_NEVER);

    assert_eq!(pf_syncookies_setwats(10, 20), Err(Errno::EINVAL));
    assert_eq!(pf_syncookies_setwats(2500, 1250), Ok(()));
    let mut w = PfiocSynflwats::default();
    assert_eq!(pf_syncookies_getwats(&mut w), Ok(()));
    assert_eq!((w.hiwat, w.lowat), (2500, 1250));
    assert!(pf_syncookies_setwats(7, 7).is_ok());
}

#[test]
fn synflood_check_follows_the_mode() {
    let _g = lock();
    let mut a1 = Addrs([0; 8]);
    let mut syn = tcp_pd(&mut a1, 1, 0, TH_SYN);
    PF_STATUS.syncookies_mode.set(PF_SYNCOOKIES_NEVER);
    assert!(!pf_synflood_check(&mut syn));
    PF_STATUS.syncookies_mode.set(PF_SYNCOOKIES_ALWAYS);
    assert!(pf_synflood_check(&mut syn));
    // Adaptive below the high watermark: no flood.
    PF_STATUS.syncookies_mode.set(PF_SYNCOOKIES_ADAPTIVE);
    PF_STATUS.syncookies_active.set(0);
    PF_SYNCOOKIE_STATUS.hiwat.set(100);
    PF_STATUS.states_halfopen.set(10);
    assert!(!pf_synflood_check(&mut syn));
    PF_STATUS.syncookies_mode.set(PF_SYNCOOKIES_NEVER);
    PF_STATUS.states_halfopen.set(0);
}
