use std::boxed::Box;

use super::*;

#[test]
fn the_device_table_has_the_emulated_controllers() {
    // QEMU's e1000 (82540EM), e1000e (82574L) and igb (82576).
    for product in [0x100e, 0x10d3, 0x10c9] {
        assert!(
            EM_DEVICES
                .iter()
                .any(|m| m.pm_vid == 0x8086 && u32::from(m.pm_pid) == product),
            "{product:#x}"
        );
    }
    // Intel's only: virtio-net (1af4:1000) shares the 82542's product id, not its vendor.
    assert!(EM_DEVICES.iter().all(|m| m.pm_vid == 0x8086));
    assert!(EM_DEVICES.iter().any(|m| m.pm_pid == 0x1000));
}

#[test]
fn the_device_table_has_no_duplicates() {
    for (i, a) in EM_DEVICES.iter().enumerate() {
        assert!(
            !EM_DEVICES[i + 1..].iter().any(|b| b.pm_pid == a.pm_pid),
            "{:#x}",
            a.pm_pid
        );
    }
}

#[test]
fn tunables_follow_the_header() {
    assert_eq!(DEFAULT_ITR, 488);
    assert_eq!(EM_MCLBYTES, 2050);
    assert_eq!(AUTONEG_ADV_DEFAULT, 0x2f);
    // TDLEN/RDLEN must be multiples of 128 bytes.
    for n in [EM_MAX_TXD, EM_MAX_TXD_82543, EM_MAX_RXD, EM_MAX_RXD_82543] {
        assert_eq!(n as usize * 16 % EM_DBA_ALIGN as usize, 0);
    }
}

#[test]
fn em_roundup_rounds_to_the_unit() {
    assert_eq!(em_roundup(0, 16), 0);
    assert_eq!(em_roundup(1, 16), 16);
    assert_eq!(em_roundup(16, 16), 16);
    assert_eq!(em_roundup(1518 + EM_FIFO_HDR, EM_FIFO_HDR), 1536);
    assert_eq!(em_roundup(9234, 1024), 10240);
}

#[test]
fn fill_descriptors_keeps_safe_buffers_whole() {
    let mut d = DescArray::default();
    // Short buffers are never split.
    assert_eq!(em_fill_descriptors(0x1003, 4, &mut d), 1);
    assert_eq!(
        d.descriptor[0],
        AddressLengthPair {
            address: 0x1003,
            length: 4
        }
    );
    // (addr & 7) + (len & 0xf) = 0 + 0: safe.
    assert_eq!(em_fill_descriptors(0x2000, 64, &mut d), 1);
    assert_eq!(d.descriptor[0].length, 64);
    // 0 + 5: safe (5 to 8).
    assert_eq!(em_fill_descriptors(0x2000, 21, &mut d), 1);
    // 7 + 6 = 0xd: safe (0xd to 0xf).
    assert_eq!(em_fill_descriptors(0x2007, 6, &mut d), 1);
}

#[test]
fn fill_descriptors_splits_the_last_four_bytes() {
    let mut d = DescArray::default();
    // 0 + 0x3 = 3: the hang case.
    assert_eq!(em_fill_descriptors(0x3000, 0x13, &mut d), 2);
    assert_eq!(
        d.descriptor[0],
        AddressLengthPair {
            address: 0x3000,
            length: 0xf
        }
    );
    assert_eq!(
        d.descriptor[1],
        AddressLengthPair {
            address: 0x300f,
            length: 4
        }
    );
    assert_eq!(d.elements, 2);
    // 2 + 8 = 0xa: the DAC case.
    assert_eq!(em_fill_descriptors(0x4002, 0x28, &mut d), 2);
    assert_eq!(d.descriptor[0].length, 0x24);
    assert_eq!(d.descriptor[1].address, 0x4026);
}

#[test]
fn media_words_follow_the_phy() {
    let (w, n) = em_media_words(em_media_type_copper, em_82574, em_phy_bm);
    assert_eq!(n, 7);
    assert_eq!(w[0], IFM_ETHER | IFM_10_T);
    assert_eq!(w[5], IFM_ETHER | IFM_1000_T);
    assert_eq!(w[6], IFM_ETHER | IFM_AUTO);

    // An IFE PHY has no gigabit.
    let (w, n) = em_media_words(em_media_type_copper, em_ich8lan, em_phy_ife);
    assert_eq!(n, 5);
    assert_eq!(w[4], IFM_ETHER | IFM_AUTO);

    // Fiber: SX, or LX on the 82545.
    let (w, n) = em_media_words(em_media_type_fiber, em_82545, em_phy_m88);
    assert_eq!(n, 3);
    assert_eq!(w[0], IFM_ETHER | IFM_1000_LX | IFM_FDX);
    let (w, _) = em_media_words(em_media_type_internal_serdes, em_82571, em_phy_m88);
    assert_eq!(w[1], IFM_ETHER | IFM_1000_SX);
}

#[test]
fn the_context_descriptor_offsets_are_the_ipv4_ones() {
    assert_eq!(ETHER_HDR_LEN + offset_of!(Ip, ip_sum), 24);
    assert_eq!(ETHER_HDR_LEN + size_of::<Ip>() + TCPHDR_TH_SUM, 50);
    assert_eq!(ETHER_HDR_LEN + size_of::<Ip>() + UDPHDR_UH_SUM, 40);
}

#[test]
fn an_all_zero_softc_is_a_valid_value() {
    // config_make_softc hands drivers zeroed memory (the `Softc` contract).
    let sc: Box<MaybeUninit<EmSoftc>> = Box::new_zeroed();
    // SAFETY: every member of `EmSoftc` is valid as zero bits (see its `Softc` impl).
    let sc = unsafe { sc.assume_init() };
    assert_eq!(sc.link_active.load(Ordering::Relaxed), 0);
    assert!(sc.queues.get().is_null());
    assert_eq!(sc.queues().count(), 0);
    let q: Box<MaybeUninit<EmQueue>> = Box::new_zeroed();
    // SAFETY: as above, for the queue em_allocate_pci_resources allocates zeroed.
    let q = unsafe { q.assume_init() };
    assert_eq!(q.tx.active_checksum_context.get(), OFFLOAD_NONE);
    assert!(q.tx.sc_tx_pkts_ring.get().is_null());
}

#[test]
#[ignore = "needs OPENBSD_SRC (just test-ref)"]
fn values_match_the_c_header() {
    let defs = crate::reftest::defines("sys/dev/pci/if_em.h");
    crate::reftest::assert_defines!(defs;
        EM_MAX_TXD_82543, EM_MAX_TXD, EM_MAX_RXD_82543, EM_MAX_RXD, MAX_INTS_PER_SEC,
        EM_TIDV, EM_TADV, EM_RDTR, EM_RADV, EM_TX_TIMEOUT, DO_AUTO_NEG,
        WAIT_FOR_AUTO_NEG_DEFAULT, EM_MMBA, EM_FLASH, EM_SMARTSPEED_DOWNSHIFT,
        EM_SMARTSPEED_MAX, MAX_NUM_MULTICAST_ADDRESSES, PCICFG_DESC_RING_STATUS,
        FLUSH_DESC_REQUIRED, EM_DBA_ALIGN, DEBUG_INIT, DEBUG_IOCTL, DEBUG_HW,
        EM_RXBUFFER_2048, EM_RXBUFFER_4096, EM_RXBUFFER_8192, EM_RXBUFFER_16384,
        EM_MAX_SCATTER, EM_TSO_SIZE, EM_TSO_SEG_SIZE, EM_PBA_BYTES_SHIFT,
        EM_TX_HEAD_ADDR_SHIFT, EM_PBA_TX_MASK, EM_FIFO_HDR, EM_82547_PKT_THRESH);
}
