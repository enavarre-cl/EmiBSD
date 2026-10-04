//! Host tests for `pfsync(4)`: the wire layout, the constants against the C header, the
//! interface, the sync state hooks and a frame's round trip through `pfsync_input`.

use super::*;
use std::sync::MutexGuard;

use crate::net::if_::tests::setup_net;
use crate::net::pf::PF_STATE_PL;
use crate::net::pf_ioctl::pfattach;
use crate::reftest::{assert_complete, assert_defines};

/// The network test lock with fresh memory and pf attached.
fn setup() -> MutexGuard<'static, ()> {
    let guard = setup_net();
    crate::kern::kern_timeout::timeout_startup();
    pfattach(1);
    guard
}

/// A zeroed state from pf's pool.
fn state() -> &'static PfState {
    pf_pool_get::<PfState>(&PF_STATE_PL, PR_NOWAIT).expect("a state")
}

#[test]
fn wire_structures_have_the_c_layout() {
    // The sizes clang gives the __packed structures of <net/if_pfsync.h>.
    assert_eq!(PFSYNC_HDRLEN, 20);
    assert_eq!(size_of::<PfsyncSubheader>(), 4);
    assert_eq!(size_of::<PfsyncClr>(), 20);
    assert_eq!(size_of::<PfsyncInsAck>(), 12);
    assert_eq!(size_of::<PfsyncUpdC>(), 84);
    assert_eq!(offset_of!(PfsyncUpdC, creatorid), 72);
    assert_eq!(size_of::<PfsyncUpdReq>(), 12);
    assert_eq!(size_of::<PfsyncDelC>(), 12);
    assert_eq!(size_of::<PfsyncBus>(), 12);
    assert_eq!(offset_of!(PfsyncBus, status), 8);
    assert_eq!(size_of::<PfsyncTdb>(), 52);
    assert_eq!(offset_of!(PfsyncTdb, rpl), 32);
    assert_eq!(offset_of!(PfsyncTdb, rdomain), 50);
    assert_eq!(size_of::<Pfsyncreq>(), 28);
    assert_eq!(size_of::<Pfsyncstats>(), 16 * 8);
    assert_eq!(PFSYNC_MINPKT, 40);
}

#[test]
fn messages_round_trip_through_the_wire_helpers() {
    let mut buf = [0u8; 16];
    let ia = PfsyncInsAck {
        id: 0x0102_0304_0506_0708,
        creatorid: 0xaabb_ccdd,
    };
    wire_put(&mut buf[1..], &ia);
    assert_eq!(buf[0], 0);
    let back: PfsyncInsAck = wire_get(&buf[1..]);
    assert_eq!(back, ia);
    assert_eq!(wire_bytes(&ia).len(), 12);
}

#[test]
fn every_action_has_its_reader() {
    assert_eq!(PFSYNC_ACTS.len(), usize::from(PFSYNC_ACT_MAX));
    for a in [
        PFSYNC_ACT_OINS,
        PFSYNC_ACT_OUPD,
        PFSYNC_ACT_INS_F,
        PFSYNC_ACT_DEL_F,
        PFSYNC_ACT_OTDB,
        PFSYNC_ACT_EOF,
    ] {
        assert!(PFSYNC_ACTS[usize::from(a)].in_.is_none(), "{a}");
    }
    assert_eq!(PFSYNC_ACTS[usize::from(PFSYNC_ACT_UPD_C)].len, 84);
    assert_eq!(
        PFSYNC_ACTS[usize::from(PFSYNC_ACT_INS)].len,
        size_of::<PfsyncState>()
    );
    assert_eq!(
        PFSYNC_QS[usize::from(PFSYNC_S_DEL)].action,
        PFSYNC_ACT_DEL_C
    );
    assert_eq!(
        PFSYNC_QS[usize::from(PFSYNC_S_IACK)].action,
        PFSYNC_ACT_INS_ACK
    );
    assert_eq!(PFSYNC_ACTIONS[usize::from(PFSYNC_ACT_TDB)], "UPD TDB");
}

#[test]
fn init_state_follows_the_flags() {
    let _g = setup();
    let st = state();
    pfsync_init_state(&st, None, None, PFSYNC_SI_PFSYNC);
    assert_eq!(st.sync_state.get(), PFSYNC_S_PFSYNC);

    st.state_flags.set(PFSTATE_ACK);
    pfsync_init_state(&st, None, None, PFSYNC_SI_PFSYNC);
    assert_eq!(st.sync_state.get(), PFSYNC_S_SYNC);
    assert_eq!(st.state_flags.get() & PFSTATE_ACK, 0);

    st.sync_state.set(PFSYNC_S_NONE);
    pfsync_init_state(&st, None, None, PFSYNC_SI_IOCTL);
    assert_eq!(st.sync_state.get(), PFSYNC_S_NONE);

    st.state_flags.set(PFSTATE_NOSYNC);
    pfsync_init_state(&st, None, None, 0);
    assert_eq!(st.sync_state.get(), PFSYNC_S_DEAD);
}

#[test]
fn nothing_is_synced_without_an_interface() {
    let _g = setup();
    assert!(!pfsync_is_up());
    let st = state();
    assert!(!pfsync_state_in_use(&st));
}

#[test]
#[ignore = "needs OPENBSD_SRC (just test-ref)"]
fn values_match_the_c_header() {
    let defs = crate::reftest::defines("sys/net/if_pfsync.h");
    let act = assert_defines!(defs;
        PFSYNC_VERSION, PFSYNC_DFLTTL, PFSYNC_ACT_CLR, PFSYNC_ACT_OINS, PFSYNC_ACT_INS_ACK,
        PFSYNC_ACT_OUPD, PFSYNC_ACT_UPD_C, PFSYNC_ACT_UPD_REQ, PFSYNC_ACT_DEL, PFSYNC_ACT_DEL_C,
        PFSYNC_ACT_INS_F, PFSYNC_ACT_DEL_F, PFSYNC_ACT_BUS, PFSYNC_ACT_OTDB, PFSYNC_ACT_EOF,
        PFSYNC_ACT_INS, PFSYNC_ACT_UPD, PFSYNC_ACT_TDB, PFSYNC_ACT_MAX, PFSYNC_BUS_START,
        PFSYNC_BUS_END, PFSYNCCTL_STATS, PFSYNCCTL_MAXID, PFSYNC_S_IACK, PFSYNC_S_UPD_C,
        PFSYNC_S_DEL, PFSYNC_S_INS, PFSYNC_S_UPD, PFSYNC_S_COUNT, PFSYNC_S_NONE, PFSYNC_S_SYNC,
        PFSYNC_S_PFSYNC, PFSYNC_S_DEAD, PFSYNC_SI_IOCTL, PFSYNC_SI_CKSUM, PFSYNC_SI_ACK,
        PFSYNC_SI_PFSYNC);
    assert_complete(
        &defs,
        "PFSYNC",
        &[
            &act[..],
            &["PFSYNC_ACTIONS", "PFSYNC_HDRLEN", "PFSYNCCTL_NAMES"],
        ]
        .concat(),
    );
}

/// A state as a peer `creatorid` would send it: UDP 10.0.0.1:1000 -> 10.0.0.2:53, on `all`.
fn peer_state(id: u64, creatorid: u32) -> PfsyncState {
    let mut sp = PfsyncState {
        id,
        creatorid,
        af: AF_INET,
        proto: crate::netinet::in_::IPPROTO_UDP as u8,
        direction: PF_OUT,
        timeout: crate::net::pfvar::PFTM_UDP_FIRST_PACKET as u8,
        expire: htonl(30),
        rule: u32::MAX,
        anchor: u32::MAX,
        ..PfsyncState::default()
    };
    let mut ifname = [0u8; IFNAMSIZ];
    ifname[..3].copy_from_slice(b"all");
    sp.ifname = ifname;
    let mut key = crate::net::pfvar::PfsyncStateKey::default();
    key.addr[0].set_addr32(0, htonl(0x0a00_0001));
    key.addr[1].set_addr32(0, htonl(0x0a00_0002));
    key.port = [htons(1000), htons(53)];
    key.af = AF_INET;
    sp.key = [key, key];
    sp
}

/// A pfsync softc that is up on the interface `ifp0`, without `pfsync_up` (whose bulk request
/// would go through `ip_output`).
fn running_softc(ifp0: &'static Ifnet) -> &'static PfsyncSoftc {
    static ONCE: std::sync::Once = std::sync::Once::new();
    ONCE.call_once(|| pfsyncattach(1));
    assert_eq!(
        pfsync_clone_create(&PFSYNC_CLONER, 1),
        Err(Errno::ENXIO),
        "only unit 0"
    );
    assert_eq!(pfsync_clone_create(&PFSYNC_CLONER, 0), Ok(()));
    let ifp = if_unit(b"pfsync0").expect("pfsync0");
    if_put(ifp);
    assert_eq!(ifp.if_type.get(), IFT_PFSYNC);
    assert_eq!(ifp.if_mtu.get(), ETHERMTU as u32);
    let sc = PfsyncSoftc::of_ifp(ifp);
    assert_eq!(sc.sc_syncpeer.get().s_addr, INADDR_PFSYNC_GROUP);
    assert_eq!(sc.sc_maxupdates.get(), 128);

    sc.sc_sync_ifidx.set(ifp0.if_index.get());
    let mut ip = Ip::default();
    ip.set_ip_v(IPVERSION);
    ip.set_ip_hl(5);
    ip.ip_ttl = PFSYNC_DFLTTL;
    ip.ip_p = IPPROTO_PFSYNC as u8;
    sc.sc_template.set(ip);
    refcnt_init(&sc.sc_refs);
    ifp.if_flags.set(ifp.if_flags.get() | IFF_UP | IFF_RUNNING);
    PFSYNCIF.store(ptr::from_ref(sc).cast_mut(), Ordering::Release);
    sc
}

#[test]
fn a_state_goes_out_as_ins_and_a_peer_del_c_removes_it() {
    let _g = setup();
    crate::net::if_::softnet_init();
    // The IP ids pfsync's frames take (ip_init does this at boot).
    crate::netinet::ip_id::ip_randomid_init();
    let ifp0 = crate::net::if_::tests::test_ifnet(b"tsync0");
    crate::net::if_::if_attach(ifp0);
    let sc = running_softc(ifp0);
    assert!(pfsync_is_up());

    // A state from DIOCADDSTATE: pfsync queues its insert.
    let sp = peer_state(0x1122_3344_5566_7788, htonl(0xc0ff_ee00));
    net_lock();
    pf_lock();
    assert_eq!(pf_state_import(&sp, PFSYNC_SI_IOCTL), Ok(()));
    pf_unlock();
    net_unlock();
    let st = pfsync_find_state(sp.id, sp.creatorid).expect("imported");
    assert_eq!(st.sync_state.get(), PFSYNC_S_INS);
    let s = sc
        .sc_slices
        .iter()
        .find(|s| !s.s_qs[usize::from(PFSYNC_S_INS)].is_empty())
        .expect("a slice holds the insert");
    let flen = PFSYNC_MINPKT + size_of::<PfsyncSubheader>() + size_of::<PfsyncState>();
    assert_eq!(s.s_len.get(), flen);

    // The frame: IP (protocol 240, TTL 255), the pfsync header, one INS.
    mtx_enter(&s.s_mtx);
    let m = pfsync_slice_write(s).expect("a frame");
    mtx_leave(&s.s_mtx);
    assert_eq!(s.s_len.get(), PFSYNC_MINPKT);
    assert_eq!(st.sync_state.get(), PFSYNC_S_NONE);
    let mut f = std::vec![0u8; flen];
    crate::kern::uipc_mbuf::m_copydata(m, 0, &mut f);
    assert_eq!(m.m_pkthdr().len.get() as usize, flen);
    assert_eq!((f[9], f[8]), (IPPROTO_PFSYNC as u8, PFSYNC_DFLTTL));
    assert_eq!(u16::from_be_bytes([f[2], f[3]]) as usize, flen);
    let ph: PfsyncHeader = wire_get(&f[20..]);
    assert_eq!(ph.version, PFSYNC_VERSION);
    assert_eq!(usize::from(ntohs(ph.len)), flen - 20);
    let subh: PfsyncSubheader = wire_get(&f[40..]);
    assert_eq!((subh.action, ntohs(subh.count)), (PFSYNC_ACT_INS, 1));
    assert_eq!(usize::from(subh.len) << 2, size_of::<PfsyncState>());
    let out: PfsyncState = wire_get(&f[44..]);
    assert_eq!((out.id, out.creatorid), (sp.id, sp.creatorid));
    m_freem(m);

    // The peer deletes it: a DEL_C frame on the sync interface.
    let mut b = std::vec::Vec::new();
    let mut ip = Ip::default();
    ip.set_ip_v(IPVERSION);
    ip.set_ip_hl(5);
    ip.ip_ttl = PFSYNC_DFLTTL;
    ip.ip_p = IPPROTO_PFSYNC as u8;
    b.extend_from_slice(&ip_bytes(&ip));
    let ph = PfsyncHeader {
        version: PFSYNC_VERSION,
        len: htons((PFSYNC_HDRLEN + 4 + 12) as u16),
        ..PfsyncHeader::default()
    };
    b.extend_from_slice(wire_bytes(&ph));
    let subh = PfsyncSubheader {
        action: PFSYNC_ACT_DEL_C,
        len: 3,
        count: htons(1),
    };
    b.extend_from_slice(wire_bytes(&subh));
    let del = PfsyncDelC {
        id: sp.id,
        creatorid: sp.creatorid,
    };
    b.extend_from_slice(wire_bytes(&del));
    let m = crate::net::if_::tests::test_packet(&b);
    m.m_pkthdr().ph_ifidx.set(ifp0.if_index.get());

    PF_STATUS.running.set(1);
    let mut mp = Some(m);
    let mut off = 20;
    net_lock();
    assert_eq!(
        pfsync_input4(&mut mp, &mut off, IPPROTO_PFSYNC, 2, None),
        IPPROTO_DONE
    );
    net_unlock();
    assert!(mp.is_none());
    assert!(pfsync_find_state(sp.id, sp.creatorid).is_none(), "removed");
    assert_eq!(
        PFSYNCCOUNTERS[PfsyncCounters::PfsyncsBadstate as usize].load(Ordering::Relaxed),
        0
    );
    pf_state_unref(Some(st));

    // A frame from another interface is not applied.
    let m = crate::net::if_::tests::test_packet(&b);
    m.m_pkthdr().ph_ifidx.set(ifp0.if_index.get() + 7);
    let before = PFSYNCCOUNTERS[PfsyncCounters::PfsyncsBadif as usize].load(Ordering::Relaxed);
    let mut mp = Some(m);
    let _ = pfsync_input4(&mut mp, &mut off, IPPROTO_PFSYNC, 2, None);
    assert_eq!(
        PFSYNCCOUNTERS[PfsyncCounters::PfsyncsBadif as usize].load(Ordering::Relaxed),
        before + 1
    );

    // net.inet.pfsync.stats is the counters.
    let mut len = 0;
    assert_eq!(pfsync_sysctl(&[PFSYNCCTL_STATS], 0, &mut len, 0, 0), Ok(()));
    assert_eq!(len, size_of::<Pfsyncstats>());
    assert_eq!(
        pfsync_sysctl(&[9], 0, &mut len, 0, 0),
        Err(Errno::ENOPROTOOPT)
    );
    assert_eq!(
        pfsync_sysctl(&[1, 1], 0, &mut len, 0, 0),
        Err(Errno::ENOTDIR)
    );

    PF_STATUS.running.set(0);
    PFSYNCIF.store(ptr::null_mut(), Ordering::Release);
}
