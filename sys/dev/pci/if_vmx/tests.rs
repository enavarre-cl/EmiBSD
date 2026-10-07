//! Host tests of vmx(4)'s pure parts: the device table, the generation bits, the zeroed
//! softc and queues, and the offload words `vmxnet3_tx_offload` and `vmxnet3_rx_offload`
//! derive from real packets; the reference-backed test reads the constants out of
//! `if_vmx.c` itself, where the C keeps its private ones.

use std::boxed::Box;
use std::mem::MaybeUninit;

use super::*;
use crate::net::if_::tests::test_packet;

/// An Ethernet frame with an IPv4 header (protocol `proto`, checksum 0x1234) and a
/// transport header of `l4` bytes (TCP's with a data offset of 5 words), then 10 bytes.
fn ipv4_packet(proto: u8, l4: usize) -> std::vec::Vec<u8> {
    let iplen = 20 + l4 + 10;
    let mut p = std::vec![0u8; ETHER_HDR_LEN + iplen];
    p[12] = 0x08; // ETHERTYPE_IP
    let ip = &mut p[ETHER_HDR_LEN..];
    ip[0] = 0x45;
    ip[2..4].copy_from_slice(&(iplen as u16).to_be_bytes());
    ip[8] = 64;
    ip[9] = proto;
    ip[10..12].copy_from_slice(&[0x12, 0x34]);
    if proto == 6 {
        ip[20 + 12] = 0x50;
    }
    p
}

#[test]
fn the_device_table_has_the_vmxnet3() {
    assert_eq!(VMX_DEVICES.len(), 1);
    assert_eq!(u32::from(VMX_DEVICES[0].pm_vid), 0x15ad);
    assert_eq!(u32::from(VMX_DEVICES[0].pm_pid), 0x07b0);
}

#[test]
fn the_generation_bits_are_the_descriptors_top_bits() {
    assert_eq!(VMX_TX_GEN, 1 << 14);
    assert_eq!(VMX_TXC_GEN, 1 << 31);
    assert_eq!(VMX_RX_GEN, 1 << 31);
    assert_eq!(VMX_RXC_GEN, 1 << 31);
    assert_eq!(NRXCOMPDESC, 2 * NRXDESC);
    assert_eq!(JUMBO_LEN, VMXNET3_RX_LEN_M);
}

#[test]
fn an_all_zero_softc_and_queue_are_valid_values() {
    // config_make_softc and vmxnet3_attach's mallocarray hand out zeroed memory.
    let sc: Box<MaybeUninit<Vmxnet3Softc>> = Box::new_zeroed();
    // SAFETY: every member of `Vmxnet3Softc` is valid as zero bits (see its `Softc` impl).
    let sc = unsafe { sc.assume_init() };
    assert!(sc.sc_q.get().is_null());
    assert_eq!(sc.queues().count(), 0);
    assert!(sc.sc_intrmap.get().is_none());

    let q: Box<MaybeUninit<Vmxnet3Queue>> = Box::new_zeroed();
    // SAFETY: as above: Cells of integers, pointers and `Option`s, atomics, mutexes and
    // timeouts, all valid as zero bits.
    let q = unsafe { q.assume_init() };
    assert_eq!(q.tx.cmd_ring.prod.load(Ordering::Relaxed), 0);
    assert!(q.tx.cmd_ring.m.iter().all(|m| m.get().is_none()));
    assert!(q.rx.cmd_ring[1].dmap.iter().all(|m| m.get().is_none()));
    assert_eq!(q.intr.get(), 0);
    assert_eq!(align_of::<Vmxnet3Queue>(), 64);
}

#[test]
fn tcp_checksum_offload_names_the_header_and_the_checksum() {
    let _g = crate::kern::uipc_mbuf::tests::setup();
    let m = test_packet(&ipv4_packet(6, 20));
    let mut sop = Vmxnet3Txdesc::default();

    // Nothing asked: nothing set.
    vmxnet3_tx_offload(&mut sop, m);
    assert_eq!(sop, Vmxnet3Txdesc::default());

    m.m_pkthdr().csum_flags.set(M_TCP_CSUM_OUT);
    vmxnet3_tx_offload(&mut sop, m);
    assert_eq!(sop.tx_word3, (VMXNET3_OM_CSUM << VMXNET3_TX_OM_S) | 34);
    assert_eq!(sop.tx_word2, 50 << VMXNET3_TX_OP_S);
}

#[test]
fn udp_checksum_offload_points_at_uh_sum() {
    let _g = crate::kern::uipc_mbuf::tests::setup();
    let m = test_packet(&ipv4_packet(17, 8));
    let mut sop = Vmxnet3Txdesc::default();

    m.m_pkthdr().csum_flags.set(M_UDP_CSUM_OUT);
    vmxnet3_tx_offload(&mut sop, m);
    assert_eq!(sop.tx_word3, (VMXNET3_OM_CSUM << VMXNET3_TX_OM_S) | 34);
    assert_eq!(sop.tx_word2, 40 << VMXNET3_TX_OP_S);
}

#[test]
fn tso_gives_the_whole_header_and_the_mss() {
    let _g = crate::kern::uipc_mbuf::tests::setup();
    let m = test_packet(&ipv4_packet(6, 20));
    let mut sop = Vmxnet3Txdesc::default();

    m.m_pkthdr().csum_flags.set(M_TCP_CSUM_OUT | M_TCP_TSO);
    m.m_pkthdr().ph_mss.set(5);
    vmxnet3_tx_offload(&mut sop, m);
    assert_eq!(sop.tx_word3, (VMXNET3_OM_TSO << VMXNET3_TX_OM_S) | 54);
    assert_eq!(sop.tx_word2, 5 << VMXNET3_TX_OP_S);
    // The IPv4 checksum is the device's to fill.
    // SAFETY: test_packet copied the frame to the mbuf's data; the checksum is at 24.
    let sum = unsafe { std::slice::from_raw_parts(mtod::<u8>(m), 64) };
    assert_eq!(&sum[24..26], &[0, 0]);

    // TSO without an mss is refused.
    let m = test_packet(&ipv4_packet(6, 20));
    let mut sop = Vmxnet3Txdesc::default();
    m.m_pkthdr().csum_flags.set(M_TCP_CSUM_OUT | M_TCP_TSO);
    vmxnet3_tx_offload(&mut sop, m);
    assert_eq!(sop, Vmxnet3Txdesc::default());
}

#[test]
fn receive_offload_reports_what_the_device_checked() {
    let _g = crate::kern::uipc_mbuf::tests::setup();
    let m = test_packet(&ipv4_packet(6, 20));
    let ok = VMXNET3_RXC_IPV4 | VMXNET3_RXC_IPSUM_OK | VMXNET3_RXC_CSUM_OK | VMXNET3_RXC_TCP;

    let mut rxcd = Vmxnet3Rxcompdesc {
        rxc_word0: VMXNET3_RXC_NOCSUM,
        rxc_word3: ok,
        ..Vmxnet3Rxcompdesc::default()
    };
    vmxnet3_rx_offload(&rxcd, m);
    assert_eq!(m.m_pkthdr().csum_flags.get(), 0);

    rxcd.rxc_word0 = 0;
    vmxnet3_rx_offload(&rxcd, m);
    assert_eq!(
        m.m_pkthdr().csum_flags.get(),
        M_IPV4_CSUM_IN_OK | M_TCP_CSUM_IN_OK
    );

    // A fragment: the IP checksum only.
    m.m_pkthdr().csum_flags.set(0);
    rxcd.rxc_word3 = ok | VMXNET3_RXC_FRAGMENT;
    vmxnet3_rx_offload(&rxcd, m);
    assert_eq!(m.m_pkthdr().csum_flags.get(), M_IPV4_CSUM_IN_OK);
}

#[test]
fn an_lro_packet_becomes_tso_with_its_segment_size() {
    let _g = crate::kern::uipc_mbuf::tests::setup();
    let m = test_packet(&ipv4_packet(6, 20));
    let rxcd = Vmxnet3Rxcompdesc {
        rxc_word3: VMXNET3_RXC_IPV4 | VMXNET3_RXC_IPSUM_OK | VMXNET3_RXC_CSUM_OK | VMXNET3_RXC_TCP,
        ..Vmxnet3Rxcompdesc::default()
    };
    // Two segments of the 10-byte payload.
    m.m_pkthdr().ph_mss.set(2);
    vmxnet3_rx_offload(&rxcd, m);
    let flags = m.m_pkthdr().csum_flags.get();
    assert_ne!(flags & M_TCP_TSO, 0);
    assert_ne!(flags & M_TCP_CSUM_OUT, 0);
    assert_eq!(m.m_pkthdr().ph_mss.get(), 5);
}

#[test]
#[ignore = "needs OPENBSD_SRC (just test-ref)"]
fn values_match_the_c_file() {
    let defs = crate::reftest::defines("sys/dev/pci/if_vmx.c");
    crate::reftest::assert_defines!(defs; NTXDESC, NTXSEGS, NRXDESC, NTXCOMPDESC, NRXCOMPDESC,
        VMXNET3_DRIVER_VERSION, JUMBO_LEN);
}
