use super::*;
use crate::reftest::{assert_complete, assert_defines};

#[test]
fn ifreq_union_accessors() {
    let mut ifr = Ifreq::zeroed();
    ifr.ifr_name[..4].copy_from_slice(b"vio0");
    ifr.set_ifr_flags((IFF_UP | IFF_BROADCAST) as i16);
    assert_eq!(ifr.ifr_flags(), 0x3);
    ifr.set_ifr_mtu(1500);
    assert_eq!(ifr.ifr_metric(), 1500);
    assert_eq!(ifr.ifr_hardmtu(), 1500);
    ifr.ifr_addr_mut().sa_len = 16;
    ifr.ifr_addr_mut().sa_family = 2;
    assert_eq!(ifr.ifr_dstaddr().sa_family, 2);
    assert_eq!(ifr.ifr_broadaddr().sa_len, 16);
    // The union starts right after the name, as in C.
    assert_eq!(core::mem::offset_of!(Ifreq, ifr_ifru), IFNAMSIZ);
    assert_eq!(core::mem::offset_of!(Ifaliasreq, ifra_dstaddr), 32);
    assert_eq!(core::mem::offset_of!(Ifgroupreq, ifgr_ifgru), 24);
}

#[test]
fn link_state_and_speed_helpers() {
    assert!(link_state_is_up(LINK_STATE_UNKNOWN));
    assert!(link_state_is_up(LINK_STATE_FULL_DUPLEX));
    assert!(!link_state_is_up(LINK_STATE_DOWN));
    let carp_backup = LINK_STATE_DESCRIPTIONS
        .iter()
        .find(|d| link_state_desc_match(d, IFT_CARP, LINK_STATE_DOWN));
    assert_eq!(carp_backup.map(|d| d.ifs_string), Some(&b"backup"[..]));
    let ether_up = LINK_STATE_DESCRIPTIONS
        .iter()
        .find(|d| link_state_desc_match(d, IFT_ETHER, LINK_STATE_UP));
    assert_eq!(ether_up.map(|d| d.ifs_string), Some(&b"active"[..]));
    assert_eq!(if_gbps(1), 1_000_000_000);
    assert_eq!(ifq_prio2tos(IFQ_DEFPRIO), 0x60);
    assert_eq!(ifq_tos2prio(0x60), IFQ_DEFPRIO);
}

#[test]
#[ignore = "needs OPENBSD_SRC (just test-ref)"]
fn values_match_the_c_header() {
    let defs = crate::reftest::defines("sys/net/if.h");
    let ifq = assert_defines!(defs; IFQ_NQUEUES, IFQ_MINPRIO, IFQ_MAXPRIO, IFQ_DEFPRIO);
    assert_complete(&defs, "IFQ_", &ifq);
    let link = assert_defines!(defs;
        LINK_STATE_UNKNOWN, LINK_STATE_INVALID, LINK_STATE_DOWN, LINK_STATE_KALIVE_DOWN,
        LINK_STATE_UP, LINK_STATE_HALF_DUPLEX, LINK_STATE_FULL_DUPLEX);
    // LINK_STATE_DESCRIPTIONS spans lines in the C.
    assert_complete(
        &defs,
        "LINK_STATE_",
        &[&link[..], &["LINK_STATE_DESCRIPTIONS"]].concat(),
    );
    let iff = assert_defines!(defs;
        IFF_UP, IFF_BROADCAST, IFF_DEBUG, IFF_LOOPBACK, IFF_POINTOPOINT, IFF_STATICARP,
        IFF_RUNNING, IFF_NOARP, IFF_PROMISC, IFF_ALLMULTI, IFF_OACTIVE, IFF_SIMPLEX, IFF_LINK0,
        IFF_LINK1, IFF_LINK2, IFF_MULTICAST);
    // IFF_CANTCHANGE spans lines in the C.
    assert_eq!(IFF_CANTCHANGE, 0x8e5a);
    assert_complete(&defs, "IFF_", &[&iff[..], &["IFF_CANTCHANGE"]].concat());
    let ifxf = assert_defines!(defs;
        IFXF_MPSAFE, IFXF_CLONED, IFXF_AUTOCONF6TEMP, IFXF_MPLS, IFXF_WOL, IFXF_AUTOCONF6,
        IFXF_INET6_NOSOII, IFXF_AUTOCONF4, IFXF_MONITOR, IFXF_LRO, IFXF_MBUF_64BIT);
    assert_eq!(IFXF_CANTCHANGE, 0x3);
    assert_complete(&defs, "IFXF_", &[&ifxf[..], &["IFXF_CANTCHANGE"]].concat());
    let ifcap = assert_defines!(defs;
        IFCAP_CSUM_IPv4, IFCAP_CSUM_TCPv4, IFCAP_CSUM_UDPv4, IFCAP_VLAN_MTU,
        IFCAP_VLAN_HWTAGGING, IFCAP_VLAN_HWOFFLOAD, IFCAP_CSUM_TCPv6, IFCAP_CSUM_UDPv6,
        IFCAP_TSOv4, IFCAP_TSOv6, IFCAP_LRO, IFCAP_WOL);
    assert_eq!(IFCAP_CSUM_MASK, 0x187);
    assert_complete(
        &defs,
        "IFCAP_",
        &[&ifcap[..], &["IFCAP_CSUM_MASK"]].concat(),
    );
    let ifqctl = assert_defines!(defs;
        IFQCTL_LEN, IFQCTL_MAXLEN, IFQCTL_DROPS, IFQCTL_CONGESTION, IFQCTL_MAXID);
    assert_complete(&defs, "IFQCTL_", &ifqctl);
    let ifan = assert_defines!(defs; IFAN_ARRIVAL, IFAN_DEPARTURE);
    assert_complete(&defs, "IFAN_", &ifan);
    let hdrprio = assert_defines!(defs;
        IF_HDRPRIO_MIN, IF_HDRPRIO_MAX, IF_HDRPRIO_PACKET, IF_HDRPRIO_PAYLOAD,
        IF_HDRPRIO_OUTER, IF_PWE3_ETHERNET, IF_PWE3_IP, IF_NAMESIZE, IF_MAX_VECTORS);
    assert_complete(&defs, "IF_", &hdrprio);
    let sff = assert_defines!(defs; IFSFF_ADDR_EEPROM, IFSFF_ADDR_DDM, IFSFF_DATA_LEN);
    assert_complete(&defs, "IFSFF_", &sff);
    assert_defines!(defs; MCLPOOLS, IFNAMSIZ, IFDESCRSIZE, IFLR_PREFIX);
    assert_eq!(defs["IFG_ALL"], "\"all\"");
    assert_eq!(defs["IFG_EGRESS"], "\"egress\"");
}

// net/if.c

use std::boxed::Box;
use std::sync::MutexGuard;

use crate::kern::uipc_mbuf::m_gethdr;
use crate::sys::mbuf::{M_DONTWAIT, MT_DATA};

/// Real memory, `mbinit`, a fresh index map (`ifinit`), and no interfaces, groups or pf kifs:
/// everything from earlier tests lives in their own (leaked) memory, so nothing may be grown
/// from it.
pub(crate) fn setup_net() -> MutexGuard<'static, ()> {
    let guard = crate::kern::uipc_mbuf::tests::setup();
    IF_IDXMAP.count.set(0);
    ifinit();
    // No interfaces, groups or pf kifs (pfi_attach_ifgroup) from earlier tests either.
    IFNETLIST.0.init();
    IFG_HEAD.0.init();
    crate::net::pf_if::pfi_test_reset();
    // Nor enc(4) interfaces or bpf(4) taps of interfaces that are gone.
    crate::net::if_enc::enc_reset();
    crate::net::bpf::bpf_test_reset();
    guard
}

/// An `ioctl` that accepts nothing: what `if_attach` needs to see.
///
/// # Safety
///
/// Never dereferences anything.
pub(crate) unsafe fn test_ioctl(
    _ifp: &'static Ifnet,
    _cmd: u64,
    _data: *mut u8,
) -> Result<(), Errno> {
    Err(Errno::ENOTTY)
}

/// A zero-filled, leaked `T`, as `malloc(M_ZERO)` gives a softc.
///
/// # Safety
///
/// The all-zero bit pattern is a valid `T`.
pub(crate) unsafe fn zeroed_static<T>() -> &'static T {
    // SAFETY: the caller's contract.
    Box::leak(Box::new(unsafe {
        MaybeUninit::<T>::zeroed().assume_init()
    }))
}

/// A zero-filled interface named `name`, with a test `if_ioctl`.
pub(crate) fn test_ifnet(name: &[u8]) -> &'static Ifnet {
    // SAFETY: the all-zero `Ifnet` is valid (`net/if_var.rs`).
    let ifp: &'static Ifnet = unsafe { zeroed_static() };
    let mut xname = [0u8; IFNAMSIZ];
    xname[..name.len()].copy_from_slice(name);
    ifp.if_xname.set(xname);
    ifp.if_ioctl.set(Some(test_ioctl));
    ifp
}

/// A packet header mbuf holding `bytes`.
pub(crate) fn test_packet(bytes: &[u8]) -> &'static Mbuf {
    let m = m_gethdr(M_DONTWAIT, MT_DATA).expect("mbuf");
    assert!(bytes.len() <= crate::sys::mbuf::MHLEN);
    // SAFETY: a fresh packet header mbuf has MHLEN bytes at m_data.
    unsafe { ptr::copy_nonoverlapping(bytes.as_ptr(), m.m_data().get(), bytes.len()) };
    m.m_len().set(bytes.len() as u32);
    m.m_pkthdr().len.set(bytes.len() as i32);
    m
}

#[test]
fn attached_interfaces_get_unique_indexes_and_the_map_grows() {
    let _g = setup_net();
    assert!(if_get(0).is_none(), "index 0 is no interface");

    let mut ifps = std::vec::Vec::new();
    for i in 0..12u8 {
        let name = [b't', b'm', b'a', b'p', b'0' + i / 10, b'0' + i % 10];
        let ifp = test_ifnet(&name);
        if_attach(ifp);
        ifps.push(ifp);
    }

    let mut seen = std::collections::BTreeSet::new();
    for ifp in &ifps {
        let index = ifp.if_index.get();
        assert_ne!(index, 0);
        assert!(seen.insert(index), "index {index} given twice");
        let got = if_get(index).expect("attached");
        assert!(ptr::eq(got, *ifp));
        if_put(got);
        assert!(ifp.if_nifqs.get() == 1 && ifp.if_niqs.get() == 1);
        assert!(ptr::eq(ifp.ifq(0), &ifp.if_snd));
        assert!(ptr::eq(ifp.ifiq(0), &ifp.if_rcv));
        // The C's defaults: if_txmit, if_llprio, if_qstart_compat for a non-MPSAFE driver.
        assert_eq!(ifp.if_txmit.get(), IF_TXMIT_DEFAULT);
        assert_eq!(u32::from(ifp.if_llprio.get()), IFQ_DEFPRIO);
        assert!(ifp.if_qstart.get().is_some() && ifp.if_enqueue.get().is_some());
        assert!(
            ifp.if_groups
                .iter()
                .any(|g| name_eq(&g.ifgl_group.ifg_group, IFG_ALL))
        );
    }
    // Twelve interfaces do not fit the initial map of 8 slots (slot 0 is the length).
    // SAFETY: the published map.
    assert!(unsafe { if_idxmap_limit(IF_IDXMAP.map.load(Ordering::Relaxed)) } >= 13);

    let found = if_unit(b"tmap07").expect("by name");
    assert!(ptr::eq(found, ifps[7]));
    if_put(found);
    assert!(if_unit(b"tmap99").is_none());

    // Taking an interface out of the map frees its index; the slot reads NULL.
    let gone = ifps[3];
    let index = gone.if_index.get();
    // (if_idxmap_remove's smr_barrier needs the SMR thread, which the host has not.)
    let _ = if_ref(gone);
    if_idxmap_unlink(gone);
    assert!(if_get(index).is_none());
    if_put(gone);
    if_put(gone);
}

#[test]
fn input_proto_queues_on_the_netstack_with_the_function_as_cookie() {
    let _g = setup_net();
    let ifp = test_ifnet(b"tproto0");
    if_attach(ifp);

    fn input(_ifp: &'static Ifnet, m: &'static Mbuf, _ns: Option<&Netstack>) {
        m.m_pkthdr().ph_flowid.set(0x5a5a);
    }

    let ns = Netstack::new();
    let m = test_packet(&[1, 2, 3]);
    if_input_proto(ifp, m, input, Some(&ns));
    assert_eq!(ml_len(&ns.ns_proto), 1);
    assert_eq!(m.m_pkthdr().ph_ifidx.get(), ifp.if_index.get());
    let m = ml_dequeue(&ns.ns_proto).expect("queued");
    if_input_process_proto(ifp, m, Some(&ns));
    assert_eq!(
        m.m_pkthdr().ph_flowid.get(),
        0x5a5a,
        "the cookie's function ran"
    );
    m_freem(m);
}

fn clone_create(_ifc: &'static IfClone, _unit: i32) -> Result<(), Errno> {
    Err(Errno::ENODEV)
}

static TEST_CLONER: IfClone = IfClone::new(b"tcl", clone_create, None);

#[test]
fn clone_lookup_splits_name_and_unit() {
    static ONCE: std::sync::Once = std::sync::Once::new();
    // SAFETY: registered once.
    ONCE.call_once(|| unsafe { if_clone_attach(&TEST_CLONER) });

    let (ifc, unit) = if_clone_lookup(b"tcl12").expect("cloner");
    assert!(ptr::eq(ifc, &TEST_CLONER));
    assert_eq!(unit, 12);
    assert_eq!(if_clone_lookup(b"tcl0\0junk").map(|(_, u)| u), Some(0));
    assert!(if_clone_lookup(b"tcl").is_none(), "no unit");
    assert!(if_clone_lookup(b"tcl01").is_none(), "unit number 0 padded");
    assert!(if_clone_lookup(b"tcl1x").is_none(), "bogus unit");
    assert!(if_clone_lookup(b"tc1").is_none(), "no such cloner");
    assert!(
        if_clone_lookup(b"tcl99999999999").is_none(),
        "unit overflows an int"
    );
    assert_eq!(if_clone_destroy(b"tcl3"), Err(Errno::EOPNOTSUPP));
}

#[test]
fn groups_check_names_and_count_members() {
    let _g = setup_net();
    let a = test_ifnet(b"tgrp0");
    let b = test_ifnet(b"tgrp1");
    if_attach(a);
    if_attach(b);

    assert_eq!(if_addgroup(a, b""), Err(Errno::EINVAL));
    assert_eq!(
        if_addgroup(a, b"bad1"),
        Err(Errno::EINVAL),
        "ends in a digit"
    );
    assert_eq!(if_addgroup(a, b"waytoolongagroupname"), Err(Errno::EINVAL));
    assert_eq!(if_addgroup(a, b"tgroup"), Ok(()));
    assert_eq!(if_addgroup(a, b"tgroup"), Err(Errno::EEXIST));
    assert_eq!(if_addgroup(b, b"tgroup"), Ok(()));

    let ifg = IFG_HEAD
        .0
        .iter()
        .find(|g| name_eq(&g.ifg_group, b"tgroup"))
        .expect("group");
    assert_eq!(ifg.ifg_refcnt.get(), 2);
    assert_eq!(ifg.ifg_members.iter().count(), 2);

    assert_eq!(if_delgroup(a, b"tgroup"), Ok(()));
    assert_eq!(if_delgroup(a, b"tgroup"), Err(Errno::ENOENT));
    assert_eq!(ifg.ifg_refcnt.get(), 1);
    assert_eq!(if_delgroup(b, b"tgroup"), Ok(()));
    assert!(!IFG_HEAD.0.iter().any(|g| name_eq(&g.ifg_group, b"tgroup")));
}

#[test]
fn header_priority_checks() {
    assert_eq!(if_txhprio_l2_check(IF_HDRPRIO_PACKET), Ok(()));
    assert_eq!(if_txhprio_l2_check(IF_HDRPRIO_PAYLOAD), Err(Errno::EINVAL));
    assert_eq!(if_txhprio_l3_check(IF_HDRPRIO_PAYLOAD), Ok(()));
    assert_eq!(if_rxhprio_l2_check(IF_HDRPRIO_OUTER), Ok(()));
    assert_eq!(if_rxhprio_l3_check(IF_HDRPRIO_OUTER), Ok(()));
    assert_eq!(if_txhprio_l3_check(IF_HDRPRIO_OUTER), Err(Errno::EINVAL));
    for prio in IF_HDRPRIO_MIN..=IF_HDRPRIO_MAX {
        assert_eq!(if_rxhprio_l3_check(prio), Ok(()));
    }
    assert_eq!(if_txhprio_l2_check(IF_HDRPRIO_MAX + 1), Err(Errno::EINVAL));
}

#[test]
fn rx_ring_accounting() {
    use crate::net::if_var::{if_rxr_cwm, if_rxr_inuse, if_rxr_needrefill, if_rxr_put};

    let mut rxr = IfRxring::default();
    if_rxr_init(&mut rxr, 2, 8);
    assert_eq!(if_rxr_cwm(&rxr), 2);
    // Within the same tick the current watermark is the limit.
    rxr.rxr_adjusted = TICKS.load(Ordering::Relaxed);
    assert_eq!(if_rxr_get(&mut rxr, 16), 2);
    assert_eq!(if_rxr_get(&mut rxr, 16), 0);
    if_rxr_put(&mut rxr, 2);
    assert!(if_rxr_needrefill(&rxr));
    // A tick later an empty ring may grow its watermark by one.
    rxr.rxr_adjusted = TICKS.load(Ordering::Relaxed).wrapping_sub(1);
    assert_eq!(if_rxr_get(&mut rxr, 16), 3);
    assert_eq!(if_rxr_inuse(&rxr), 3);
    // A livelock shrinks it again, never below the low watermark.
    rxr.rxr_adjusted = TICKS.load(Ordering::Relaxed).wrapping_sub(1);
    if_rxr_livelocked(&mut rxr);
    assert_eq!(rxr.rxr_cwm, 2);
}

#[test]
fn congestion_marker_lasts_a_hundredth_of_a_second() {
    if_congestion();
    assert!(if_congested());
    IFQ_CONGESTION.store(
        TICKS.load(Ordering::Relaxed).wrapping_sub(HZ),
        Ordering::Relaxed,
    );
    assert!(!if_congested());
}

/// The packets [`keep_output`] was given.
#[cfg(feature = "inet6")]
static KEPT: std::sync::Mutex<std::vec::Vec<usize>> = std::sync::Mutex::new(std::vec::Vec::new());

/// An `if_output` that keeps the packet (its address in [`KEPT`]) for the test to free.
///
/// # Safety
///
/// `IfOutputFn`'s contract.
#[cfg(feature = "inet6")]
unsafe fn keep_output(
    _ifp: &'static Ifnet,
    m: &'static Mbuf,
    _dst: *const Sockaddr,
    _rt: Option<&'static Rtentry>,
) -> Result<(), Errno> {
    KEPT.lock()
        .unwrap_or_else(|e| e.into_inner())
        .push(ptr::from_ref(m).addr());
    Ok(())
}

#[test]
#[cfg(feature = "inet6")]
fn output_tso_sends_an_ipv6_packet_with_its_checksum() {
    use crate::netinet::in_::IPPROTO_UDP;
    use crate::netinet6::in6::{In6Addr, SockaddrIn6, sin6tosa_const};
    use crate::netinet6::in6_cksum::in6_cksum;
    use crate::sys::mbuf::M_UDP_CSUM_OUT;

    let _g = setup_net();
    let ifp = test_ifnet(b"ttso6");
    ifp.if_output.set(Some(keep_output));
    // IPv6 from fe80::1 to fe80::2, UDP 1234 -> 53 with four bytes and no checksum yet.
    let mut p = std::vec![0x60, 0, 0, 0, 0, 12, IPPROTO_UDP as u8, 64];
    p.extend_from_slice(&[0xfe, 0x80, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 1]);
    p.extend_from_slice(&[0xfe, 0x80, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 2]);
    p.extend_from_slice(&[0x04, 0xd2, 0, 53, 0, 12, 0, 0, 1, 2, 3, 4]);
    let m = test_packet(&p);
    m.m_pkthdr().csum_flags.set(M_UDP_CSUM_OUT);
    let dst = SockaddrIn6::with_addr(In6Addr::new([
        0xfe, 0x80, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 2,
    ]));

    let mut mp = Some(m);
    // SAFETY: a local `sockaddr_in6`; the interface keeps the packet.
    let r = unsafe { if_output_tso(ifp, &mut mp, sin6tosa_const(&dst), None, 1500) };
    assert_eq!(r, Ok(()));
    assert!(mp.is_none(), "sent");
    assert_eq!(
        *KEPT.lock().unwrap_or_else(|e| e.into_inner()),
        [ptr::from_ref(m).addr()]
    );
    // No IFCAP_CSUM_UDPv6: in6_proto_cksum_out computed the checksum in software.
    assert_eq!(m.m_pkthdr().csum_flags.get() & M_UDP_CSUM_OUT, 0);
    assert_eq!(
        in6_cksum(m, IPPROTO_UDP as u8, 40, 12),
        0,
        "a valid checksum"
    );
    m_freem(m);
}

#[test]
#[cfg(feature = "inet6")]
fn if_up_gives_the_default_loopback_its_ipv6_addresses() {
    use crate::net::if_loop::{LOOP_CLONER, loop_clone_create};
    use crate::netinet6::in6::{IN6ADDR_LOOPBACK, in6ifa_ifpforlinklocal, in6ifa_ifpwithaddr};

    let _g = crate::netinet::ip_input::tests::setup();
    loop_clone_create(&LOOP_CLONER, 0).expect("lo0");
    loop_clone_create(&LOOP_CLONER, 1).expect("lo1");
    let lo0 = if_get(rtable_loindex(0)).expect("lo0");
    let lo1 = if_unit(b"lo1").expect("lo1");
    assert!(in6ifa_ifpwithaddr(lo0, &IN6ADDR_LOOPBACK).is_none());

    net_lock();
    if_up(lo0);
    if_up(lo1);
    net_unlock();
    assert!(
        in6ifa_ifpwithaddr(lo0, &IN6ADDR_LOOPBACK).is_some(),
        "::1 on lo0"
    );
    assert!(in6ifa_ifpforlinklocal(lo0, 0).is_some(), "fe80::1%lo0");
    // Only the default loopback of the rdomain gets ::1.
    assert!(in6ifa_ifpwithaddr(lo1, &IN6ADDR_LOOPBACK).is_none());
    if_put(lo0);
    if_put(lo1);
}
