//! Host tests of `vio(4)`'s pure parts: the header layout, the checksum fold, the control
//! area's size and the feature bits; the reference-backed test reads the constants out of
//! `if_vio.c` itself, where the C keeps its private header.

use super::*;

#[test]
fn the_net_header_round_trips_through_its_bytes() {
    let h = VirtioNetHdr {
        flags: VIRTIO_NET_HDR_F_NEEDS_CSUM,
        gso_type: VIRTIO_NET_HDR_GSO_TCPV4,
        hdr_len: 54,
        gso_size: 1448,
        csum_start: 34,
        csum_offset: 16,
        num_buffers: 3,
    };
    let b = h.to_bytes();
    assert_eq!(b.len(), 12);
    assert_eq!(VirtioNetHdr::from_bytes(&b), h);
    // The 0.9 header without MRG_RXBUF stops before num_buffers.
    let short = VirtioNetHdr::from_bytes(&b[..offset_of!(VirtioNetHdr, num_buffers)]);
    assert_eq!(short.num_buffers, 0);
    assert_eq!(short.gso_size, 1448);
}

#[test]
fn the_checksum_update_folds_the_carry() {
    assert_eq!(vio_cksum_update(0x1234, 0x0100), 0x1334);
    // 0xffff + 1 = 0x10000, folded: 0x0000 + 0x0001.
    assert_eq!(vio_cksum_update(0xffff, 1), 0x0001);
}

#[test]
fn the_control_area_has_room_for_both_mac_tables() {
    assert_eq!(VIO_CTRL_MAC_INFO_SIZE, 2 * 4 + 65 * 6);
    // The whole control area after the transmit headers (vio_alloc_mem).
    let ctrl = size_of::<VirtioNetCtrlCmd>()
        + size_of::<VirtioNetCtrlStatus>()
        + size_of::<VirtioNetCtrlRx>()
        + size_of::<VirtioNetCtrlMqPairsSet>()
        + size_of::<VirtioNetCtrlGuestOffloads>()
        + VIO_CTRL_MAC_INFO_SIZE;
    assert_eq!(ctrl, 2 + 1 + 1 + 2 + 8 + 398);
}

#[test]
fn the_rx_buffer_offset_aligns_the_ip_header() {
    // vio_attach: (ETHER_ALIGN + 4 - hdr_size % 4) % 4, so that the IP header after the
    // virtio and Ethernet headers is 4-byte aligned.
    for hdr in [10i32, 12] {
        let off = (ETHER_ALIGN as i32 + 4 - hdr % 4) % 4;
        assert_eq!((off + hdr + ETHER_HDR_LEN as i32) % 4, 0, "hdr {hdr}");
    }
}

#[test]
fn feature_bits_are_the_spec_bit_numbers() {
    assert_eq!(VIRTIO_NET_F_MAC.trailing_zeros(), 5);
    assert_eq!(VIRTIO_NET_F_MRG_RXBUF.trailing_zeros(), 15);
    assert_eq!(VIRTIO_NET_F_CTRL_VQ.trailing_zeros(), 17);
    assert_eq!(VIRTIO_NET_F_MQ.trailing_zeros(), 22);
    assert_eq!(VIRTIO_NET_F_SPEED_DUPLEX.trailing_zeros(), 63);
    assert_eq!(VIRTIO_NET_FEATURE_NAMES_DEBUG.len(), 35);
    assert!(virtio_net_feature_names().is_empty());
}

#[test]
#[ignore = "needs OPENBSD_SRC (just test-ref)"]
fn values_match_the_c_file() {
    let defs = crate::reftest::defines("sys/dev/pv/if_vio.c");
    crate::reftest::assert_defines!(defs;
        VIRTIO_NET_CONFIG_MAC, VIRTIO_NET_CONFIG_STATUS, VIRTIO_NET_CONFIG_MAX_QUEUES,
        VIRTIO_NET_CONFIG_MTU, VIRTIO_NET_CONFIG_SPEED, VIRTIO_NET_CONFIG_DUPLEX,
        VIRTIO_NET_CONFIG_RSS_SIZE, VIRTIO_NET_CONFIG_RSS_LEN, VIRTIO_NET_CONFIG_HASH_TYPES,
        VIRTIO_NET_CONFIG_TUNNEL_TYPES,
        VIRTIO_NET_F_CSUM, VIRTIO_NET_F_GUEST_CSUM, VIRTIO_NET_F_CTRL_GUEST_OFFLOADS,
        VIRTIO_NET_F_MTU, VIRTIO_NET_F_MAC, VIRTIO_NET_F_GUEST_TSO4, VIRTIO_NET_F_GUEST_TSO6,
        VIRTIO_NET_F_GUEST_ECN, VIRTIO_NET_F_GUEST_UFO, VIRTIO_NET_F_HOST_TSO4,
        VIRTIO_NET_F_HOST_TSO6, VIRTIO_NET_F_HOST_ECN, VIRTIO_NET_F_HOST_UFO,
        VIRTIO_NET_F_MRG_RXBUF, VIRTIO_NET_F_STATUS, VIRTIO_NET_F_CTRL_VQ, VIRTIO_NET_F_CTRL_RX,
        VIRTIO_NET_F_CTRL_VLAN, VIRTIO_NET_F_CTRL_RX_EXTRA, VIRTIO_NET_F_GUEST_ANNOUNCE,
        VIRTIO_NET_F_MQ, VIRTIO_NET_F_CTRL_MAC_ADDR, VIRTIO_NET_F_DEVICE_STATS,
        VIRTIO_NET_F_HASH_TUNNEL, VIRTIO_NET_F_VQ_NOTF_COAL, VIRTIO_NET_F_NOTF_COAL,
        VIRTIO_NET_F_GUEST_USO4, VIRTIO_NET_F_GUEST_USO6, VIRTIO_NET_F_HOST_USO,
        VIRTIO_NET_F_HASH_REPORT, VIRTIO_NET_F_GUEST_HDRLEN, VIRTIO_NET_F_RSS,
        VIRTIO_NET_F_RSC_EXT, VIRTIO_NET_F_STANDBY,
        CONFFLAG_QEMU_VLAN_BUG, VIRTIO_NET_S_LINK_UP,
        VIRTIO_NET_HDR_F_NEEDS_CSUM, VIRTIO_NET_HDR_F_DATA_VALID, VIRTIO_NET_HDR_GSO_NONE,
        VIRTIO_NET_HDR_GSO_TCPV4, VIRTIO_NET_HDR_GSO_UDP, VIRTIO_NET_HDR_GSO_TCPV6,
        VIRTIO_NET_HDR_GSO_ECN, VIRTIO_NET_CTRL_RX, VIRTIO_NET_CTRL_RX_PROMISC,
        VIRTIO_NET_CTRL_RX_ALLMULTI, VIRTIO_NET_CTRL_MAC, VIRTIO_NET_CTRL_MAC_TABLE_SET,
        VIRTIO_NET_CTRL_VLAN, VIRTIO_NET_CTRL_VLAN_ADD, VIRTIO_NET_CTRL_VLAN_DEL,
        VIRTIO_NET_CTRL_MQ, VIRTIO_NET_CTRL_MQ_VQ_PAIRS_SET, VIRTIO_NET_CTRL_MQ_RSS_CONFIG,
        VIRTIO_NET_CTRL_MQ_HASH_CONFIG, VIRTIO_NET_CTRL_GUEST_OFFLOADS,
        VIRTIO_NET_CTRL_GUEST_OFFLOADS_SET, VIRTIO_NET_OK, VIRTIO_NET_ERR,
        VIRTIO_NET_CTRL_MQ_VQ_PAIRS_MIN, VIRTIO_NET_CTRL_MQ_VQ_PAIRS_MAX,
        VIRTIO_NET_CTRL_MAC_MC_ENTRIES, VIRTIO_NET_CTRL_MAC_UC_ENTRIES,
    );
    // (1ULL<<63) does not fit an i64 as the parser reads it.
    assert_eq!(VIRTIO_NET_F_SPEED_DUPLEX, 1u64 << 63);
}
