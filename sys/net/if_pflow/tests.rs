//! Host tests for `pflow(4)`: the record layouts and templates, the constants against the C
//! header, the MTU arithmetic, and a removed state's flows in a version 5 and an IPFIX
//! datagram.

use std::sync::MutexGuard;

use super::*;
use crate::kern::uipc_mbuf::mq_dequeue;
use crate::net::if_::tests::setup_net;
use crate::net::if_::{if_put, if_unit};
use crate::net::if_pfsync::PFSYNC_SI_IOCTL;
use crate::net::pf::{pf_find_state_byid, pf_state_import};
use crate::net::pf_ioctl::pfattach;
use crate::net::pfvar::{PFSTATE_PFLOW, PFTM_UDP_FIRST_PACKET, PfStateCmp, PfsyncState};
use crate::net::pfvar_priv::{pf_lock, pf_unlock};
use crate::netinet::in_::IPPROTO_UDP;
use crate::reftest::{assert_complete, assert_defines};

/// The network test lock with fresh memory and pf attached.
fn setup() -> MutexGuard<'static, ()> {
    let guard = setup_net();
    crate::kern::kern_timeout::timeout_startup();
    pfattach(1);
    guard
}

#[test]
fn records_and_headers_have_the_c_layout() {
    // The sizes clang gives the __packed structures of <net/if_pflow.h>.
    assert_eq!(size_of::<PflowFlow>(), 48);
    assert_eq!(PFLOW_HDRLEN, 24);
    assert_eq!(PFLOW_IPFIX_HDRLEN, 16);
    assert_eq!(PFLOW_SET_HDRLEN, 4);
    assert_eq!(size_of::<PflowIpfixFlow4>(), 54);
    assert_eq!(size_of::<PflowIpfixNatFlow4>(), 66);
    assert_eq!(size_of::<PflowIpfixFlow6>(), 78);
    assert_eq!(size_of::<PflowIpfixTmpl>(), 176);
    assert_eq!(offset_of!(PflowIpfixTmpl, ipv4_nat_tmpl), 56);
    assert_eq!(size_of::<Pflowreq>(), 520);
    assert_eq!(PFLOW_MINMTU, 72);
}

#[test]
fn the_templates_name_their_fields() {
    let t = pflow_ipfix_tmpl();
    let b = wire_bytes(&t);
    // set id 2, the whole set's length
    assert_eq!(&b[..4], &[0, 2, 0, 176]);
    // the IPv4 template: id 256, 12 fields, the first sourceIPv4Address of 4 bytes
    assert_eq!(&b[4..12], &[1, 0, 0, 12, 0, 8, 0, 4]);
    let nat = t.ipv4_nat_tmpl;
    assert_eq!(
        nat.h,
        PflowTmplHdr {
            tmpl_id: htons(257),
            field_count: htons(16)
        }
    );
    let v6 = t.ipv6_tmpl;
    assert_eq!(v6.src_ip, fspec(PFIX_IE_sourceIPv6Address, 16));
}

#[test]
fn valid_socket_addresses() {
    let mut ss = SockaddrStorage::zeroed();
    assert!(!pflowvalidsockaddr(None, true));
    assert!(!pflowvalidsockaddr(Some(&ss), true), "no family");
    ss.ss_family = AF_INET;
    ss.ss_len = 16;
    let copy = pflow_sockaddr_copy(&ss).expect("an inet address");
    assert_eq!(copy.ss_len, 16);
    assert!(!pflowvalidsockaddr(Some(&ss), true), "INADDR_ANY");
    let mut sin = SockaddrIn {
        sin_len: 16,
        sin_family: AF_INET,
        sin_addr: crate::netinet::in_::InAddr {
            s_addr: htonl(0xc0a8_4d02),
        },
        ..SockaddrIn::default()
    };
    // SAFETY: a sockaddr_in fits at the start of a storage, any bytes are valid.
    unsafe { ptr::write_unaligned(ptr::from_mut(&mut ss).cast::<SockaddrIn>(), sin) };
    assert!(
        pflowvalidsockaddr(Some(&ss), true),
        "a source needs no port"
    );
    assert!(!pflowvalidsockaddr(Some(&ss), false), "a destination does");
    sin.sin_port = htons(9995);
    // SAFETY: as above.
    unsafe { ptr::write_unaligned(ptr::from_mut(&mut ss).cast::<SockaddrIn>(), sin) };
    assert!(pflowvalidsockaddr(Some(&ss), false));
    ss.ss_family = 99;
    assert!(pflow_sockaddr_copy(&ss).is_none());
}

/// A removed-state candidate: UDP 10.0.0.1:1000 -> 10.0.0.2:53 out of the stack, with
/// `PFSTATE_PFLOW`.
fn flow_state() -> &'static PfState {
    let mut sp = PfsyncState {
        id: 0x0102_0304_0506_0708,
        creatorid: htonl(0x00c0_ffee),
        af: AF_INET,
        proto: IPPROTO_UDP as u8,
        direction: PF_OUT,
        timeout: PFTM_UDP_FIRST_PACKET as u8,
        expire: htonl(30),
        rule: u32::MAX,
        anchor: u32::MAX,
        state_flags: PFSTATE_PFLOW.to_be(),
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

    net_lock();
    pf_lock();
    assert_eq!(pf_state_import(&sp, PFSYNC_SI_IOCTL), Ok(()));
    pf_unlock();
    net_unlock();
    let st = pf_find_state_byid(&PfStateCmp {
        id: sp.id,
        creatorid: sp.creatorid,
        ..PfStateCmp::default()
    })
    .expect("imported");
    st.packets[0].set(3);
    st.packets[1].set(2);
    st.bytes[0].set(300);
    st.bytes[1].set(200);
    st
}

#[test]
fn a_state_leaves_as_netflow_5_and_ipfix_records() {
    let _g = setup();
    crate::net::if_::softnet_init();
    static ONCE: std::sync::Once = std::sync::Once::new();
    ONCE.call_once(|| pflowattach(1));
    assert_eq!(pflow_clone_create(&PFLOW_CLONER, 0), Ok(()));
    let ifp = if_unit(b"pflow0").expect("pflow0");
    if_put(ifp);
    let sc = PflowSoftc::of_ifp(ifp);
    assert_eq!(ifp.if_type.get(), IFT_PFLOW);
    assert_eq!(ifp.if_flags.get() & (IFF_UP | IFF_RUNNING), IFF_UP);
    assert_eq!(sc.sc_version.get(), PFLOW_PROTO_5);
    // 24 + 28 + 30 records of 48 bytes.
    assert_eq!(ifp.if_mtu.get(), 1492);
    assert_eq!(sc.sc_maxcount.get(), PFLOW_MAXFLOWS);

    let st = flow_state();

    // Not running (no collector): nothing is buffered.
    assert_eq!(export_pflow(st), 0);
    assert!(sc.sc_mbuf.get().is_none());

    // Running: version 5, two records, one per direction.
    ifp.if_flags.set(ifp.if_flags.get() | IFF_RUNNING);
    assert_eq!(export_pflow(st), 0);
    assert_eq!(sc.sc_count.get(), 2);
    pflow_flush(sc);
    assert!(sc.sc_mbuf.get().is_none());
    let m = mq_dequeue(&sc.sc_outputqueue).expect("a datagram");
    let h: PflowHeader = m_get_wire(m, 0);
    assert_eq!((ntohs16(h.version), ntohs16(h.count)), (5, 2));
    assert_eq!(
        (h.engine_type, h.engine_id),
        (PFLOW_ENGINE_TYPE, PFLOW_ENGINE_ID)
    );
    let f1: PflowFlow = m_get_wire(m, PFLOW_HDRLEN as i32);
    // PF_OUT: the first record goes from addr[1]:port[1] to addr[0]:port[0].
    assert_eq!({ f1.src_ip }, htonl(0x0a00_0002));
    assert_eq!({ f1.dest_ip }, htonl(0x0a00_0001));
    assert_eq!((f1.src_port, f1.dest_port), (htons(53), htons(1000)));
    assert_eq!((f1.flow_packets, f1.flow_octets), (htonl(3), htonl(300)));
    assert_eq!(f1.protocol, IPPROTO_UDP as u8);
    let f2: PflowFlow = m_get_wire(m, (PFLOW_HDRLEN + size_of::<PflowFlow>()) as i32);
    assert_eq!({ f2.src_ip }, htonl(0x0a00_0001));
    assert_eq!({ f2.flow_octets }, htonl(200));
    m_freem(m);

    // IPFIX: the same state as two IPv4 records behind the template's set header.
    mtx_enter(&sc.sc_mtx);
    sc.sc_version.set(PFLOW_PROTO_10);
    pflow_setmtu(sc, ETHERMTU as i32);
    mtx_leave(&sc.sc_mtx);
    // 16 + 28 + min(22 * 66, 18 * 78)
    assert_eq!(ifp.if_mtu.get(), 1448);
    assert_eq!((sc.sc_maxcount4.get(), sc.sc_maxcount6.get()), (22, 18));
    assert_eq!(export_pflow(st), 0);
    assert_eq!(sc.sc_count4.get(), 2);
    pflow_flush(sc);
    let m = mq_dequeue(&sc.sc_outputqueue).expect("a datagram");
    let h10: PflowV10Header = m_get_wire(m, 0);
    let len = PFLOW_IPFIX_HDRLEN + PFLOW_SET_HDRLEN + 2 * size_of::<PflowIpfixFlow4>();
    assert_eq!(
        (ntohs16(h10.version), usize::from(ntohs16(h10.length))),
        (10, len)
    );
    assert_eq!(m.m_pkthdr().len.get() as usize, len);
    let set: PflowSetHeader = m_get_wire(m, PFLOW_IPFIX_HDRLEN as i32);
    assert_eq!(ntohs16(set.set_id), PFLOW_IPFIX_TMPL_IPV4_ID);
    let r1: PflowIpfixFlow4 = m_get_wire(m, (PFLOW_IPFIX_HDRLEN + PFLOW_SET_HDRLEN) as i32);
    assert_eq!(
        (r1.flow_packets, r1.flow_octets),
        (htobe64(3), htobe64(300))
    );
    m_freem(m);
    assert_eq!(sc.sc_sequence.get(), 2);

    // The template set goes out by itself.
    mtx_enter(&sc.sc_mtx);
    assert_eq!(pflow_sendout_ipfix_tmpl(sc), Ok(()));
    mtx_leave(&sc.sc_mtx);
    let m = mq_dequeue(&sc.sc_outputqueue).expect("the templates");
    assert_eq!(
        m.m_pkthdr().len.get() as usize,
        PFLOW_IPFIX_HDRLEN + size_of::<PflowIpfixTmpl>()
    );
    m_freem(m);

    // net.pflow.stats: 6 flows in 3 datagrams.
    let mut len = 0;
    assert_eq!(pflow_sysctl(&[NET_PFLOW_STATS], 0, &mut len, 0, 0), Ok(()));
    assert_eq!(len, 0, "sysctl_struct sets the length only with a buffer");
    assert!(PFLOW_COUNTERS[PflowstatCounters::PflowFlows as usize].load(Ordering::Relaxed) >= 4);
    assert_eq!(
        pflow_sysctl(&[NET_PFLOW_STATS], 0, &mut len, 1, 0),
        Err(Errno::EPERM)
    );
    assert_eq!(
        pflow_sysctl(&[7], 0, &mut len, 0, 0),
        Err(Errno::EOPNOTSUPP)
    );
    assert_eq!(
        pflow_sysctl(&[1, 1], 0, &mut len, 0, 0),
        Err(Errno::ENOTDIR)
    );

    // Off the list again, so that later tests see no interface.
    ifp.if_flags.set(ifp.if_flags.get() & !IFF_RUNNING);
    // SAFETY: created above, on the list.
    unsafe { PFLOWIF_LIST.remove(sc) };
}

/// `ntohs` of a field read out of a packed structure.
fn ntohs16(v: u16) -> u16 {
    u16::from_be(v)
}

#[test]
#[ignore = "needs OPENBSD_SRC (just test-ref)"]
fn values_match_the_c_header() {
    let defs = crate::reftest::defines("sys/net/if_pflow.h");
    let pflow = assert_defines!(defs;
        PFLOW_MAXFLOWS, PFLOW_ENGINE_TYPE, PFLOW_ENGINE_ID, PFLOW_MAXBYTES, PFLOW_TIMEOUT,
        PFLOW_TMPL_TIMEOUT, PFLOW_IPFIX_TMPL_SET_ID, PFLOW_IPFIX_TMPL_IPV4_FIELD_COUNT,
        PFLOW_IPFIX_TMPL_IPV4_ID, PFLOW_IPFIX_TMPL_NAT_IPV4_FIELD_COUNT,
        PFLOW_IPFIX_TMPL_NAT_IPV4_ID, PFLOW_IPFIX_TMPL_IPV6_FIELD_COUNT,
        PFLOW_IPFIX_TMPL_IPV6_ID, PFLOW_PROTO_5, PFLOW_PROTO_10, PFLOW_PROTO_MAX,
        PFLOW_PROTO_DEFAULT, PFLOW_MASK_SRCIP, PFLOW_MASK_DSTIP, PFLOW_MASK_VERSION);
    assert_complete(
        &defs,
        "PFLOW_",
        &[
            &pflow[..],
            &[
                "PFLOW_ID_LEN",
                "PFLOW_SET_HDRLEN",
                "PFLOW_HDRLEN",
                "PFLOW_IPFIX_HDRLEN",
            ],
            &["PFLOW_PROTOS"],
        ]
        .concat(),
    );
    let ie = assert_defines!(defs;
        PFIX_IE_octetDeltaCount, PFIX_IE_packetDeltaCount, PFIX_IE_protocolIdentifier,
        PFIX_IE_ipClassOfService, PFIX_IE_sourceTransportPort, PFIX_IE_sourceIPv4Address,
        PFIX_IE_ingressInterface, PFIX_IE_destinationTransportPort,
        PFIX_IE_destinationIPv4Address, PFIX_IE_egressInterface, PFIX_IE_flowEndSysUpTime,
        PFIX_IE_flowStartSysUpTime, PFIX_IE_sourceIPv6Address, PFIX_IE_destinationIPv6Address,
        PFIX_IE_flowStartMilliseconds, PFIX_IE_flowEndMilliseconds,
        PFIX_IE_postNATSourceIPv4Address, PFIX_IE_postNATDestinationIPv4Address,
        PFIX_IE_postNAPTSourceTransportPort, PFIX_IE_postNAPTDestinationTransportPort);
    assert_complete(&defs, "PFIX_IE_", &ie);
}
