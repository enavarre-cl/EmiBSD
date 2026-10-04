/*	$OpenBSD: if_vio.c,v 1.81 2026/06/23 14:40:40 bluhm Exp $	*/
/* <LICENSES> */
/*
 * Copyright (c) 2012 Stefan Fritsch, Alexander Fiveg.
 * Copyright (c) 2010 Minoura Makoto.
 * All rights reserved.
 *
 * Redistribution and use in source and binary forms, with or without
 * modification, are permitted provided that the following conditions
 * are met:
 * 1. Redistributions of source code must retain the above copyright
 *    notice, this list of conditions and the following disclaimer.
 * 2. Redistributions in binary form must reproduce the above copyright
 *    notice, this list of conditions and the following disclaimer in the
 *    documentation and/or other materials provided with the distribution.
 *
 * THIS SOFTWARE IS PROVIDED BY THE AUTHOR ``AS IS'' AND ANY EXPRESS OR
 * IMPLIED WARRANTIES, INCLUDING, BUT NOT LIMITED TO, THE IMPLIED WARRANTIES
 * OF MERCHANTABILITY AND FITNESS FOR A PARTICULAR PURPOSE ARE DISCLAIMED.
 * IN NO EVENT SHALL THE AUTHOR BE LIABLE FOR ANY DIRECT, INDIRECT,
 * INCIDENTAL, SPECIAL, EXEMPLARY, OR CONSEQUENTIAL DAMAGES (INCLUDING, BUT
 * NOT LIMITED TO, PROCUREMENT OF SUBSTITUTE GOODS OR SERVICES; LOSS OF USE,
 * DATA, OR PROFITS; OR BUSINESS INTERRUPTION) HOWEVER CAUSED AND ON ANY
 * THEORY OF LIABILITY, WHETHER IN CONTRACT, STRICT LIABILITY, OR TORT
 * (INCLUDING NEGLIGENCE OR OTHERWISE) ARISING IN ANY WAY OUT OF THE USE OF
 * THIS SOFTWARE, EVEN IF ADVISED OF THE POSSIBILITY OF SUCH DAMAGE.
 */
/* </LICENSES> */

//! `vio(4)`: the virtio network device driver (`vio* at virtio?`).
//!
//! Upstream: sys/dev/pv/if_vio.c @ 3ce1f3f79392
//!
//! One receive and one transmit virtqueue per queue pair, plus the control queue when the
//! device has one. Receive buffers are mbuf clusters loaded into the rx queue; received frames
//! (with `VIRTIO_NET_F_MRG_RXBUF`, spread over several buffers) go to `ifiq_input`. Frames to
//! send come off the interface's send queue in `vio_start`, each behind a `virtio_net_hdr`
//! in the driver's DMA area. The control queue carries the receive filter, the promiscuous
//! and all-multicast switches, the guest offloads and the queue count; its requests sleep
//! until the device answers (or poll while `cold`).
//!
//! ## Deviations
//! - The softc is `#[repr(C)]` with the device first and the `struct arpcom` inside, as the
//!   NIC rows of `docs/C_TO_RUST.md` say; it is reached as `&'static` (softcs are never freed
//!   while the interface exists). `if_softc` and `ifq_softc` hold the softc and the queue.
//! - The DMA area (`sc_dma_kva`: the transmit headers and the control structures) is reached
//!   through raw pointers: the control structures are `__packed` in C, so they are read and
//!   written unaligned, and the status the device writes is read volatile. `vio_tx_offload`
//!   fills a local `virtio_net_hdr` that is then copied into the slot's header; `vio_rxeof`
//!   copies the received header out of the buffer before trimming it, where the C keeps a
//!   pointer to the same bytes.
//! - The dmamap and mbuf arrays of a queue are one `mallocarray`, as in C; their slots are
//!   `Cell<Option<&'static ...>>` reached through bounds-checked accessors.
//! - Not ported, reported with `unported!` where called: `struct intrmap`
//!   (`kern/kern_intrmap.c`: the multi-queue interrupt map, used only when `VIRTIO_NET_F_MQ`
//!   is negotiated, which needs more than one CPU; `sc_intrmap` is therefore always NULL and
//!   left out, its tests commented), `struct ifmedia` and `ifmedia_*` (`net/if_media.c`:
//!   `sc_media` is left out; `SIOCGIFMEDIA`/`SIOCSIFMEDIA` fail with `ENOSYS`). Not configured
//!   (comments at the sites): `NVLAN`. `NBPFILTER` is configured: `vio_start` taps each
//!   packet it queues; so is `INET6` (feature `inet6`: TSO of IPv6 segments).
//! - `offsetof(struct tcphdr, th_sum)` and `offsetof(struct udphdr, uh_sum)` are the
//!   constants 16 and 6: `netinet/tcp.h` and `netinet/udp.h` are not ported.
//! - `KERNEL_LOCK()` is a comment (nothing without `MULTIPROCESSOR`).
//! - Functions returning 0 or an errno return `Result`; `vio_alloc_mem` and
//!   `vio_alloc_dmamem` (the C's -1 and 1) return `Err(ENOMEM)`.

use core::cell::Cell;
use core::ffi::c_void;
use core::mem::offset_of;
use core::ptr::{self, NonNull};

use crate::dev::pv::virtio::{
    virtio_alloc_vq, virtio_attach_finish, virtio_check_vq, virtio_dequeue, virtio_dequeue_commit,
    virtio_enqueue, virtio_enqueue_abort, virtio_enqueue_commit, virtio_enqueue_p,
    virtio_enqueue_prep, virtio_enqueue_reserve, virtio_free_vq, virtio_notify,
    virtio_postpone_intr_far, virtio_postpone_intr_smart, virtio_reinit_end, virtio_reinit_start,
    virtio_reset, virtio_start_vq_intr, virtio_stop_vq_intr, virtio_vq_dump,
};
use crate::dev::pv::virtioreg::{
    PCI_PRODUCT_VIRTIO_NETWORK, VIRTIO_CONFIG_DEVICE_STATUS_DEVICE_NEEDS_RESET,
    VIRTIO_F_ANY_LAYOUT, VIRTIO_F_RING_EVENT_IDX, VIRTIO_F_RING_INDIRECT_DESC, VIRTIO_F_VERSION_1,
};
use crate::dev::pv::virtiovar::{
    VIRTIO_CHILD_ERROR, VIRTIO_DEBUG, VirtioAttachArgs, VirtioFeatureName, VirtioSoftc, Virtqueue,
    virtio_get_status, virtio_has_feature, virtio_intr_barrier, virtio_intr_establish,
    virtio_negotiate_features, virtio_read_device_config_1, virtio_read_device_config_2,
};
use crate::kern::init_main::NCPUS;
use crate::kern::kern_lock::{mtx_enter, mtx_init, mtx_leave};
use crate::kern::kern_malloc::{free, mallocarray};
use crate::kern::kern_synch::{tsleep_nsec, wakeup};
use crate::kern::kern_timeout::{timeout_add_sec, timeout_del, timeout_set};
use crate::kern::subr_prf::{Str, panic, printf, snprintf};
use crate::kern::uipc_mbuf::{m_adj, m_clget, m_defrag, m_freem, ml_enqueue};
use crate::machine::bus::{
    BUS_DMA_64BIT, BUS_DMA_ALLOCNOW, BUS_DMA_NOWAIT, BUS_DMA_READ, BUS_DMA_WRITE, BUS_DMA_ZERO,
    BUS_DMASYNC_POSTREAD, BUS_DMASYNC_POSTWRITE, BUS_DMASYNC_PREREAD, BUS_DMASYNC_PREWRITE,
    BusDmaSegment, BusDmamap, BusSize, bus_dmamap_create, bus_dmamap_destroy, bus_dmamap_load,
    bus_dmamap_load_mbuf, bus_dmamap_sync, bus_dmamap_unload, bus_dmamem_alloc, bus_dmamem_free,
    bus_dmamem_map, bus_dmamem_unmap,
};
use crate::machine::intr::{IPL_MPSAFE, IPL_NET, splassert, splnet, splx};
use crate::net::bpf::{BPF_DIRECTION_OUT, bpf_mtap};
use crate::net::if_::{
    IF_MAX_VECTORS, IFCAP_CSUM_TCPv4, IFCAP_CSUM_TCPv6, IFCAP_CSUM_UDPv4, IFCAP_CSUM_UDPv6,
    IFCAP_LRO, IFCAP_TSOv4, IFCAP_TSOv6, IFF_ALLMULTI, IFF_BROADCAST, IFF_DEBUG, IFF_MULTICAST,
    IFF_PROMISC, IFF_RUNNING, IFF_SIMPLEX, IFF_UP, IFNAMSIZ, IFXF_LRO, IFXF_MBUF_64BIT,
    IFXF_MPSAFE, IfRxringInfo, Ifmediareq, Ifreq, LINK_STATE_DOWN, LINK_STATE_FULL_DUPLEX,
    if_attach, if_attach_iqueues, if_attach_queues, if_link_state_change, if_rxr_get,
    if_rxr_info_ioctl, if_rxr_init, if_rxr_livelocked, link_state_is_up,
};
use crate::net::if_ethersubr::{
    ether_extract_headers, ether_fakeaddr, ether_ifattach, ether_ioctl, ether_sprintf,
};
use crate::net::if_media::{IFM_ACTIVE, IFM_AUTO, IFM_AVALID, IFM_ETHER, IFM_FDX};
use crate::net::if_var::{Ifnet, if_rxr_put};
use crate::net::ifq::{
    Ifiqueue, Ifqueue, ifiq_input, ifq_barrier, ifq_clr_oactive, ifq_dequeue, ifq_init_maxlen,
    ifq_is_oactive, ifq_purge, ifq_restart, ifq_set_oactive,
};
use crate::netinet::if_ether::{
    Arpcom, ETHER_ADDR_LEN, ETHER_ALIGN, ETHER_HDR_LEN, ETHER_MAX_HARDMTU_LEN, EtherExtracted,
    EtherMultistep, ether_first_multi, ether_next_multi,
};
use crate::netinet::tcp_var::{TcpstatCounters, tcpstat_add, tcpstat_inc};
use crate::sys::device::{CD_COCOVM, CfMatch, Cfattach, Cfdriver, DV_IFNET, Device, Softc};
use crate::sys::errno::Errno;
use crate::sys::malloc::{M_DEVBUF, M_WAITOK, M_ZERO};
use crate::sys::mbuf::{
    M_DONTWAIT, M_FLOWID, M_PKTHDR, M_TCP_CSUM_IN_OK, M_TCP_CSUM_OUT, M_TCP_TSO, M_UDP_CSUM_IN_OK,
    M_UDP_CSUM_OUT, MAXMCLBYTES, MCLBYTES, Mbuf, MbufList, mtod,
};
use crate::sys::mutex::{Mutex, mutex_assert_locked};
use crate::sys::param::{PRIBIO, howmany};
use crate::sys::sockio::{
    SIOCGIFMEDIA, SIOCGIFRXR, SIOCSIFADDR, SIOCSIFFLAGS, SIOCSIFMEDIA, SIOCSIFXFLAGS,
};
use crate::sys::systm::{COLD, INFSLP};
use crate::sys::timeout::Timeout;
use crate::{kassert, unported};

// if_vioreg.h:

// Configuration registers

/// `VIRTIO_NET_CONFIG_MAC`: 8 bit x 6 byte.
pub const VIRTIO_NET_CONFIG_MAC: i32 = 0;
/// `VIRTIO_NET_CONFIG_STATUS`: 16 bit.
pub const VIRTIO_NET_CONFIG_STATUS: i32 = 6;
/// `VIRTIO_NET_CONFIG_MAX_QUEUES`: 16 bit.
pub const VIRTIO_NET_CONFIG_MAX_QUEUES: i32 = 8;
/// `VIRTIO_NET_CONFIG_MTU`: 16 bit.
pub const VIRTIO_NET_CONFIG_MTU: i32 = 10;
/// `VIRTIO_NET_CONFIG_SPEED`: 32 bit.
pub const VIRTIO_NET_CONFIG_SPEED: i32 = 12;
/// `VIRTIO_NET_CONFIG_DUPLEX`: 8 bit.
pub const VIRTIO_NET_CONFIG_DUPLEX: i32 = 16;
/// `VIRTIO_NET_CONFIG_RSS_SIZE`: 8 bit.
pub const VIRTIO_NET_CONFIG_RSS_SIZE: i32 = 17;
/// `VIRTIO_NET_CONFIG_RSS_LEN`: 16 bit.
pub const VIRTIO_NET_CONFIG_RSS_LEN: i32 = 18;
/// `VIRTIO_NET_CONFIG_HASH_TYPES`: 32 bit.
pub const VIRTIO_NET_CONFIG_HASH_TYPES: i32 = 20;
/// `VIRTIO_NET_CONFIG_TUNNEL_TYPES`: 32 bit.
pub const VIRTIO_NET_CONFIG_TUNNEL_TYPES: i32 = 24;

// Feature bits

/// `VIRTIO_NET_F_CSUM`.
pub const VIRTIO_NET_F_CSUM: u64 = 1 << 0;
/// `VIRTIO_NET_F_GUEST_CSUM`.
pub const VIRTIO_NET_F_GUEST_CSUM: u64 = 1 << 1;
/// `VIRTIO_NET_F_CTRL_GUEST_OFFLOADS`.
pub const VIRTIO_NET_F_CTRL_GUEST_OFFLOADS: u64 = 1 << 2;
/// `VIRTIO_NET_F_MTU`.
pub const VIRTIO_NET_F_MTU: u64 = 1 << 3;
/// `VIRTIO_NET_F_MAC`.
pub const VIRTIO_NET_F_MAC: u64 = 1 << 5;
/// `VIRTIO_NET_F_GUEST_TSO4`.
pub const VIRTIO_NET_F_GUEST_TSO4: u64 = 1 << 7;
/// `VIRTIO_NET_F_GUEST_TSO6`.
pub const VIRTIO_NET_F_GUEST_TSO6: u64 = 1 << 8;
/// `VIRTIO_NET_F_GUEST_ECN`.
pub const VIRTIO_NET_F_GUEST_ECN: u64 = 1 << 9;
/// `VIRTIO_NET_F_GUEST_UFO`.
pub const VIRTIO_NET_F_GUEST_UFO: u64 = 1 << 10;
/// `VIRTIO_NET_F_HOST_TSO4`.
pub const VIRTIO_NET_F_HOST_TSO4: u64 = 1 << 11;
/// `VIRTIO_NET_F_HOST_TSO6`.
pub const VIRTIO_NET_F_HOST_TSO6: u64 = 1 << 12;
/// `VIRTIO_NET_F_HOST_ECN`.
pub const VIRTIO_NET_F_HOST_ECN: u64 = 1 << 13;
/// `VIRTIO_NET_F_HOST_UFO`.
pub const VIRTIO_NET_F_HOST_UFO: u64 = 1 << 14;
/// `VIRTIO_NET_F_MRG_RXBUF`.
pub const VIRTIO_NET_F_MRG_RXBUF: u64 = 1 << 15;
/// `VIRTIO_NET_F_STATUS`.
pub const VIRTIO_NET_F_STATUS: u64 = 1 << 16;
/// `VIRTIO_NET_F_CTRL_VQ`.
pub const VIRTIO_NET_F_CTRL_VQ: u64 = 1 << 17;
/// `VIRTIO_NET_F_CTRL_RX`.
pub const VIRTIO_NET_F_CTRL_RX: u64 = 1 << 18;
/// `VIRTIO_NET_F_CTRL_VLAN`.
pub const VIRTIO_NET_F_CTRL_VLAN: u64 = 1 << 19;
/// `VIRTIO_NET_F_CTRL_RX_EXTRA`.
pub const VIRTIO_NET_F_CTRL_RX_EXTRA: u64 = 1 << 20;
/// `VIRTIO_NET_F_GUEST_ANNOUNCE`.
pub const VIRTIO_NET_F_GUEST_ANNOUNCE: u64 = 1 << 21;
/// `VIRTIO_NET_F_MQ`.
pub const VIRTIO_NET_F_MQ: u64 = 1 << 22;
/// `VIRTIO_NET_F_CTRL_MAC_ADDR`.
pub const VIRTIO_NET_F_CTRL_MAC_ADDR: u64 = 1 << 23;
/// `VIRTIO_NET_F_DEVICE_STATS`.
pub const VIRTIO_NET_F_DEVICE_STATS: u64 = 1 << 50;
/// `VIRTIO_NET_F_HASH_TUNNEL`.
pub const VIRTIO_NET_F_HASH_TUNNEL: u64 = 1 << 51;
/// `VIRTIO_NET_F_VQ_NOTF_COAL`.
pub const VIRTIO_NET_F_VQ_NOTF_COAL: u64 = 1 << 52;
/// `VIRTIO_NET_F_NOTF_COAL`.
pub const VIRTIO_NET_F_NOTF_COAL: u64 = 1 << 53;
/// `VIRTIO_NET_F_GUEST_USO4`.
pub const VIRTIO_NET_F_GUEST_USO4: u64 = 1 << 54;
/// `VIRTIO_NET_F_GUEST_USO6`.
pub const VIRTIO_NET_F_GUEST_USO6: u64 = 1 << 55;
/// `VIRTIO_NET_F_HOST_USO`.
pub const VIRTIO_NET_F_HOST_USO: u64 = 1 << 56;
/// `VIRTIO_NET_F_HASH_REPORT`.
pub const VIRTIO_NET_F_HASH_REPORT: u64 = 1 << 57;
/// `VIRTIO_NET_F_GUEST_HDRLEN`.
pub const VIRTIO_NET_F_GUEST_HDRLEN: u64 = 1 << 59;
/// `VIRTIO_NET_F_RSS`.
pub const VIRTIO_NET_F_RSS: u64 = 1 << 60;
/// `VIRTIO_NET_F_RSC_EXT`.
pub const VIRTIO_NET_F_RSC_EXT: u64 = 1 << 61;
/// `VIRTIO_NET_F_STANDBY`.
pub const VIRTIO_NET_F_STANDBY: u64 = 1 << 62;
/// `VIRTIO_NET_F_SPEED_DUPLEX`.
pub const VIRTIO_NET_F_SPEED_DUPLEX: u64 = 1 << 63;

// Config(8) flags. The lowest byte is reserved for generic virtio stuff.

/// `CONFFLAG_QEMU_VLAN_BUG`: workaround for vlan related bug in qemu < version 2.0.
pub const CONFFLAG_QEMU_VLAN_BUG: i32 = 1 << 8;

/// `virtio_net_feature_names[]` (the names only with `VIRTIO_DEBUG`).
static VIRTIO_NET_FEATURE_NAMES_DEBUG: [VirtioFeatureName; 35] = [
    VirtioFeatureName {
        bit: VIRTIO_NET_F_CSUM,
        name: "CSum",
    },
    VirtioFeatureName {
        bit: VIRTIO_NET_F_GUEST_CSUM,
        name: "GuestCSum",
    },
    VirtioFeatureName {
        bit: VIRTIO_NET_F_CTRL_GUEST_OFFLOADS,
        name: "CtrlGuestOffl",
    },
    VirtioFeatureName {
        bit: VIRTIO_NET_F_MTU,
        name: "MTU",
    },
    VirtioFeatureName {
        bit: VIRTIO_NET_F_MAC,
        name: "MAC",
    },
    VirtioFeatureName {
        bit: VIRTIO_NET_F_GUEST_TSO4,
        name: "GuestTSO4",
    },
    VirtioFeatureName {
        bit: VIRTIO_NET_F_GUEST_TSO6,
        name: "GuestTSO6",
    },
    VirtioFeatureName {
        bit: VIRTIO_NET_F_GUEST_ECN,
        name: "GuestECN",
    },
    VirtioFeatureName {
        bit: VIRTIO_NET_F_GUEST_UFO,
        name: "GuestUFO",
    },
    VirtioFeatureName {
        bit: VIRTIO_NET_F_HOST_TSO4,
        name: "HostTSO4",
    },
    VirtioFeatureName {
        bit: VIRTIO_NET_F_HOST_TSO6,
        name: "HostTSO6",
    },
    VirtioFeatureName {
        bit: VIRTIO_NET_F_HOST_ECN,
        name: "HostECN",
    },
    VirtioFeatureName {
        bit: VIRTIO_NET_F_HOST_UFO,
        name: "HostUFO",
    },
    VirtioFeatureName {
        bit: VIRTIO_NET_F_MRG_RXBUF,
        name: "MrgRXBuf",
    },
    VirtioFeatureName {
        bit: VIRTIO_NET_F_STATUS,
        name: "Status",
    },
    VirtioFeatureName {
        bit: VIRTIO_NET_F_CTRL_VQ,
        name: "CtrlVQ",
    },
    VirtioFeatureName {
        bit: VIRTIO_NET_F_CTRL_RX,
        name: "CtrlRX",
    },
    VirtioFeatureName {
        bit: VIRTIO_NET_F_CTRL_VLAN,
        name: "CtrlVLAN",
    },
    VirtioFeatureName {
        bit: VIRTIO_NET_F_CTRL_RX_EXTRA,
        name: "CtrlRXExtra",
    },
    VirtioFeatureName {
        bit: VIRTIO_NET_F_GUEST_ANNOUNCE,
        name: "GuestAnnounce",
    },
    VirtioFeatureName {
        bit: VIRTIO_NET_F_MQ,
        name: "MQ",
    },
    VirtioFeatureName {
        bit: VIRTIO_NET_F_CTRL_MAC_ADDR,
        name: "CtrlMAC",
    },
    VirtioFeatureName {
        bit: VIRTIO_NET_F_DEVICE_STATS,
        name: "DevStats",
    },
    VirtioFeatureName {
        bit: VIRTIO_NET_F_HASH_TUNNEL,
        name: "HashTun",
    },
    VirtioFeatureName {
        bit: VIRTIO_NET_F_VQ_NOTF_COAL,
        name: "VqNotfCoal",
    },
    VirtioFeatureName {
        bit: VIRTIO_NET_F_NOTF_COAL,
        name: "NotfCoal",
    },
    VirtioFeatureName {
        bit: VIRTIO_NET_F_GUEST_USO4,
        name: "GuestUso4",
    },
    VirtioFeatureName {
        bit: VIRTIO_NET_F_GUEST_USO6,
        name: "GuestUso6",
    },
    VirtioFeatureName {
        bit: VIRTIO_NET_F_HOST_USO,
        name: "HostUso",
    },
    VirtioFeatureName {
        bit: VIRTIO_NET_F_HASH_REPORT,
        name: "HashRpt",
    },
    VirtioFeatureName {
        bit: VIRTIO_NET_F_GUEST_HDRLEN,
        name: "GuestHdrlen",
    },
    VirtioFeatureName {
        bit: VIRTIO_NET_F_RSS,
        name: "RSS",
    },
    VirtioFeatureName {
        bit: VIRTIO_NET_F_RSC_EXT,
        name: "RSSExt",
    },
    VirtioFeatureName {
        bit: VIRTIO_NET_F_STANDBY,
        name: "Stdby",
    },
    VirtioFeatureName {
        bit: VIRTIO_NET_F_SPEED_DUPLEX,
        name: "SpdDplx",
    },
];

/// `virtio_net_feature_names`: the table `virtio_negotiate_features` is given, empty unless
/// `VIRTIO_DEBUG`.
fn virtio_net_feature_names() -> &'static [VirtioFeatureName] {
    if VIRTIO_DEBUG > 0 {
        &VIRTIO_NET_FEATURE_NAMES_DEBUG
    } else {
        &[]
    }
}

// Status

/// `VIRTIO_NET_S_LINK_UP`.
pub const VIRTIO_NET_S_LINK_UP: u16 = 1;

/// `VIRTIO_NET_HDR_F_NEEDS_CSUM`: flags.
pub const VIRTIO_NET_HDR_F_NEEDS_CSUM: u8 = 1;
/// `VIRTIO_NET_HDR_F_DATA_VALID`: flags.
pub const VIRTIO_NET_HDR_F_DATA_VALID: u8 = 2;
/// `VIRTIO_NET_HDR_GSO_NONE`: gso_type.
pub const VIRTIO_NET_HDR_GSO_NONE: u8 = 0;
/// `VIRTIO_NET_HDR_GSO_TCPV4`: gso_type.
pub const VIRTIO_NET_HDR_GSO_TCPV4: u8 = 1;
/// `VIRTIO_NET_HDR_GSO_UDP`: gso_type.
pub const VIRTIO_NET_HDR_GSO_UDP: u8 = 3;
/// `VIRTIO_NET_HDR_GSO_TCPV6`: gso_type.
pub const VIRTIO_NET_HDR_GSO_TCPV6: u8 = 4;
/// `VIRTIO_NET_HDR_GSO_ECN`: gso_type, |'ed.
pub const VIRTIO_NET_HDR_GSO_ECN: u8 = 0x80;

/// `VIRTIO_NET_MAX_GSO_LEN`.
pub const VIRTIO_NET_MAX_GSO_LEN: usize = 65536 + ETHER_HDR_LEN;

/// `struct virtio_net_hdr`: the packet header structure.
#[repr(C)]
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct VirtioNetHdr {
    /// `flags`.
    pub flags: u8,
    /// `gso_type`.
    pub gso_type: u8,
    /// `hdr_len`.
    pub hdr_len: u16,
    /// `gso_size`.
    pub gso_size: u16,
    /// `csum_start`.
    pub csum_start: u16,
    /// `csum_offset`.
    pub csum_offset: u16,
    /// `num_buffers`: only present if `VIRTIO_NET_F_MRG_RXBUF` is negotiated.
    pub num_buffers: u16,
}

impl VirtioNetHdr {
    /// The header's bytes.
    fn to_bytes(self) -> [u8; size_of::<VirtioNetHdr>()] {
        let mut b = [0u8; size_of::<VirtioNetHdr>()];
        b[0] = self.flags;
        b[1] = self.gso_type;
        b[2..4].copy_from_slice(&self.hdr_len.to_ne_bytes());
        b[4..6].copy_from_slice(&self.gso_size.to_ne_bytes());
        b[6..8].copy_from_slice(&self.csum_start.to_ne_bytes());
        b[8..10].copy_from_slice(&self.csum_offset.to_ne_bytes());
        b[10..12].copy_from_slice(&self.num_buffers.to_ne_bytes());
        b
    }

    /// A header from its first `bytes.len()` bytes, the rest 0.
    fn from_bytes(bytes: &[u8]) -> Self {
        let mut b = [0u8; size_of::<VirtioNetHdr>()];
        let n = bytes.len().min(b.len());
        b[..n].copy_from_slice(&bytes[..n]);
        Self {
            flags: b[0],
            gso_type: b[1],
            hdr_len: u16::from_ne_bytes([b[2], b[3]]),
            gso_size: u16::from_ne_bytes([b[4], b[5]]),
            csum_start: u16::from_ne_bytes([b[6], b[7]]),
            csum_offset: u16::from_ne_bytes([b[8], b[9]]),
            num_buffers: u16::from_ne_bytes([b[10], b[11]]),
        }
    }
}

// Control virtqueue

/// `struct virtio_net_ctrl_cmd`.
#[repr(C)]
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct VirtioNetCtrlCmd {
    /// `class`.
    pub class: u8,
    /// `command`.
    pub command: u8,
}

/// `VIRTIO_NET_CTRL_RX`.
pub const VIRTIO_NET_CTRL_RX: u8 = 0;
/// `VIRTIO_NET_CTRL_RX_PROMISC`.
pub const VIRTIO_NET_CTRL_RX_PROMISC: u8 = 0;
/// `VIRTIO_NET_CTRL_RX_ALLMULTI`.
pub const VIRTIO_NET_CTRL_RX_ALLMULTI: u8 = 1;

/// `VIRTIO_NET_CTRL_MAC`.
pub const VIRTIO_NET_CTRL_MAC: u8 = 1;
/// `VIRTIO_NET_CTRL_MAC_TABLE_SET`.
pub const VIRTIO_NET_CTRL_MAC_TABLE_SET: u8 = 0;

/// `VIRTIO_NET_CTRL_VLAN`.
pub const VIRTIO_NET_CTRL_VLAN: u8 = 2;
/// `VIRTIO_NET_CTRL_VLAN_ADD`.
pub const VIRTIO_NET_CTRL_VLAN_ADD: u8 = 0;
/// `VIRTIO_NET_CTRL_VLAN_DEL`.
pub const VIRTIO_NET_CTRL_VLAN_DEL: u8 = 1;

/// `VIRTIO_NET_CTRL_MQ`.
pub const VIRTIO_NET_CTRL_MQ: u8 = 4;
/// `VIRTIO_NET_CTRL_MQ_VQ_PAIRS_SET`.
pub const VIRTIO_NET_CTRL_MQ_VQ_PAIRS_SET: u8 = 0;
/// `VIRTIO_NET_CTRL_MQ_RSS_CONFIG`.
pub const VIRTIO_NET_CTRL_MQ_RSS_CONFIG: u8 = 1;
/// `VIRTIO_NET_CTRL_MQ_HASH_CONFIG`.
pub const VIRTIO_NET_CTRL_MQ_HASH_CONFIG: u8 = 2;

/// `VIRTIO_NET_CTRL_GUEST_OFFLOADS`.
pub const VIRTIO_NET_CTRL_GUEST_OFFLOADS: u8 = 5;
/// `VIRTIO_NET_CTRL_GUEST_OFFLOADS_SET`.
pub const VIRTIO_NET_CTRL_GUEST_OFFLOADS_SET: u8 = 0;

/// `struct virtio_net_ctrl_status`.
#[repr(C)]
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct VirtioNetCtrlStatus {
    /// `ack`.
    pub ack: u8,
}

/// `VIRTIO_NET_OK`.
pub const VIRTIO_NET_OK: u8 = 0;
/// `VIRTIO_NET_ERR`.
pub const VIRTIO_NET_ERR: u8 = 1;

/// `struct virtio_net_ctrl_rx`.
#[repr(C)]
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct VirtioNetCtrlRx {
    /// `onoff`.
    pub onoff: u8,
}

/// `struct virtio_net_ctrl_mq_pairs_set`.
#[repr(C)]
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct VirtioNetCtrlMqPairsSet {
    /// `virtqueue_pairs`.
    pub virtqueue_pairs: u16,
}

/// `VIRTIO_NET_CTRL_MQ_VQ_PAIRS_MIN`.
pub const VIRTIO_NET_CTRL_MQ_VQ_PAIRS_MIN: u16 = 1;
/// `VIRTIO_NET_CTRL_MQ_VQ_PAIRS_MAX`.
pub const VIRTIO_NET_CTRL_MQ_VQ_PAIRS_MAX: u16 = 0x8000;

/// `struct virtio_net_ctrl_guest_offloads` (`__packed`: byte aligned).
#[repr(C, packed)]
#[derive(Clone, Copy, Debug, Default)]
pub struct VirtioNetCtrlGuestOffloads {
    /// `offloads`.
    pub offloads: u64,
}

/// `struct virtio_net_ctrl_mac_tbl` (`__packed`): `nentries` and then `macs[][6]`.
#[repr(C, packed)]
#[derive(Clone, Copy, Debug, Default)]
pub struct VirtioNetCtrlMacTbl {
    /// `nentries`.
    pub nentries: u32,
}

/// `struct virtio_net_ctrl_vlan`.
#[repr(C)]
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct VirtioNetCtrlVlan {
    /// `id`.
    pub id: u16,
}

// if_viovar.h:

/// `enum vio_ctrl_state`.
#[repr(i32)]
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[allow(non_camel_case_types)] // OpenBSD names, verbatim
pub enum VioCtrlState {
    /// `FREE`.
    FREE = 0,
    /// `INUSE`.
    INUSE,
    /// `DONE`.
    DONE,
    /// `RESET`.
    RESET,
}

pub use VioCtrlState::*;

/// A slot of a queue's dmamap and mbuf arrays (`bus_dmamap_t`, `struct mbuf *`).
type MapSlot = Cell<Option<&'static BusDmamap>>;
/// A slot of a queue's mbuf arrays.
type MbufSlot = Cell<Option<&'static Mbuf>>;

/// `struct vio_queue`.
#[repr(C, align(64))]
pub struct VioQueue {
    /// `viq_sc`.
    pub viq_sc: Cell<*const VioSoftc>,
    /// `viq_txhdrs`: `vq_num` headers in the DMA area.
    pub viq_txhdrs: Cell<*mut VirtioNetHdr>,
    /// `viq_arrays`, also `viq_rxdmamaps`: the rx dmamaps, then the tx dmamaps, the rx mbufs
    /// and the tx mbufs, in one allocation.
    pub viq_arrays: Cell<*mut MapSlot>,
    /// `viq_txdmamaps`.
    pub viq_txdmamaps: Cell<*mut MapSlot>,
    /// `viq_rxmbufs`.
    pub viq_rxmbufs: Cell<*mut MbufSlot>,
    /// `viq_txmbufs`.
    pub viq_txmbufs: Cell<*mut MbufSlot>,
    /// `viq_rxring`.
    pub viq_rxring: Cell<crate::net::if_::IfRxring>,
    /// `viq_ifiq`.
    pub viq_ifiq: Cell<Option<&'static Ifiqueue>>,
    /// `viq_ifq`.
    pub viq_ifq: Cell<Option<&'static Ifqueue>>,
    /// `viq_rxvq`.
    pub viq_rxvq: Cell<*const Virtqueue>,
    /// `viq_txvq`.
    pub viq_txvq: Cell<*const Virtqueue>,
    /// `viq_txmtx`.
    pub viq_txmtx: Mutex,
    /// `viq_rxmtx`.
    pub viq_rxmtx: Mutex,
    /// `viq_txfree_slots`.
    pub viq_txfree_slots: Cell<i32>,
}

impl VioQueue {
    /// `vioq->viq_sc`.
    pub fn sc(&self) -> &'static VioSoftc {
        // SAFETY: vio_attach points it at the softc that holds this queue; softcs are never
        // freed while their interface exists.
        unsafe { nonnull(self.viq_sc.get(), "viq_sc") }
    }

    /// `vioq->viq_rxvq`.
    pub fn rxvq(&self) -> &'static Virtqueue {
        // SAFETY: a queue of the parent's `sc_vqs`, which lives as long as the softc.
        unsafe { nonnull(self.viq_rxvq.get(), "viq_rxvq") }
    }

    /// `vioq->viq_txvq`.
    pub fn txvq(&self) -> &'static Virtqueue {
        // SAFETY: as for `rxvq`.
        unsafe { nonnull(self.viq_txvq.get(), "viq_txvq") }
    }

    /// `vioq->viq_ifq`.
    pub fn ifq(&self) -> &'static Ifqueue {
        match self.viq_ifq.get() {
            Some(q) => q,
            None => panic(format_args!("vio: queue without an ifq")),
        }
    }

    /// `vioq->viq_ifiq`.
    pub fn ifiq(&self) -> &'static Ifiqueue {
        match self.viq_ifiq.get() {
            Some(q) => q,
            None => panic(format_args!("vio: queue without an ifiq")),
        }
    }

    /// Slot `i` of one of the arrays, `n` long.
    fn slot<T>(base: *mut T, i: usize, n: usize) -> &'static T {
        if base.is_null() || i >= n {
            panic(format_args!("vio: bad slot {i}"));
        }
        // SAFETY: vio_alloc_mem made the arrays (zeroed `Option`s, valid all-zero) with the
        // queue sizes as lengths; they stay for the softc's life.
        unsafe { &*base.add(i) }
    }

    /// `&vioq->viq_rxdmamaps[i]`.
    pub fn rxdmamap_slot(&self, i: usize) -> &'static MapSlot {
        Self::slot(self.viq_arrays.get(), i, self.rxvq().vq_num.get() as usize)
    }

    /// `vioq->viq_rxdmamaps[i]`.
    pub fn rxdmamap(&self, i: usize) -> &'static BusDmamap {
        match self.rxdmamap_slot(i).get() {
            Some(m) => m,
            None => panic(format_args!("vio: no rx dmamap {i}")),
        }
    }

    /// `&vioq->viq_txdmamaps[i]`.
    pub fn txdmamap_slot(&self, i: usize) -> &'static MapSlot {
        Self::slot(
            self.viq_txdmamaps.get(),
            i,
            self.txvq().vq_num.get() as usize,
        )
    }

    /// `vioq->viq_txdmamaps[i]`.
    pub fn txdmamap(&self, i: usize) -> &'static BusDmamap {
        match self.txdmamap_slot(i).get() {
            Some(m) => m,
            None => panic(format_args!("vio: no tx dmamap {i}")),
        }
    }

    /// `&vioq->viq_rxmbufs[i]`.
    pub fn rxmbuf(&self, i: usize) -> &'static MbufSlot {
        Self::slot(self.viq_rxmbufs.get(), i, self.rxvq().vq_num.get() as usize)
    }

    /// `&vioq->viq_txmbufs[i]`.
    pub fn txmbuf(&self, i: usize) -> &'static MbufSlot {
        Self::slot(self.viq_txmbufs.get(), i, self.txvq().vq_num.get() as usize)
    }

    /// `&vioq->viq_txhdrs[slot]`, in the DMA area.
    pub fn txhdr(&self, slot: usize) -> *mut VirtioNetHdr {
        let base = self.viq_txhdrs.get();
        if base.is_null() || slot >= self.txvq().vq_num.get() as usize {
            panic(format_args!("vio: bad tx header {slot}"));
        }
        // SAFETY: vio_alloc_mem carved `vq_num` headers for this queue out of the DMA area.
        unsafe { base.add(slot) }
    }
}

/// `&*p` for the softc's links, which attach sets before anything follows them.
///
/// # Safety
///
/// `p` is null or points at an object that lives for the rest of the kernel.
unsafe fn nonnull<T>(p: *const T, what: &str) -> &'static T {
    if p.is_null() {
        panic(format_args!("vio: no {what}"));
    }
    // SAFETY: the caller's guarantee.
    unsafe { &*p }
}

/// `struct vio_softc`.
#[repr(C)]
pub struct VioSoftc {
    /// `sc_dev`.
    pub sc_dev: Device,

    /// `sc_virtio`.
    pub sc_virtio: Cell<*const VirtioSoftc>,
    /// `sc_ctl_vq`.
    pub sc_ctl_vq: Cell<*const Virtqueue>,

    /// `sc_ac`.
    pub sc_ac: Arpcom,
    // sc_media: struct ifmedia, net/if_media.c is not ported (see the deviations).
    /// `sc_ifflags`.
    pub sc_ifflags: Cell<i16>,

    // bus_dmamem
    /// `sc_dma_seg`.
    pub sc_dma_seg: Cell<BusDmaSegment>,
    /// `sc_dma_map`.
    pub sc_dma_map: Cell<Option<&'static BusDmamap>>,
    /// `sc_dma_size`.
    pub sc_dma_size: Cell<usize>,
    /// `sc_dma_kva`.
    pub sc_dma_kva: Cell<*mut u8>,

    /// `sc_hdr_size`.
    pub sc_hdr_size: Cell<i32>,
    /// `sc_ctrl_cmd`.
    pub sc_ctrl_cmd: Cell<*mut VirtioNetCtrlCmd>,
    /// `sc_ctrl_status`.
    pub sc_ctrl_status: Cell<*mut VirtioNetCtrlStatus>,
    /// `sc_ctrl_rx`.
    pub sc_ctrl_rx: Cell<*mut VirtioNetCtrlRx>,
    /// `sc_ctrl_mq_pairs`.
    pub sc_ctrl_mq_pairs: Cell<*mut VirtioNetCtrlMqPairsSet>,
    /// `sc_ctrl_guest_offloads`.
    pub sc_ctrl_guest_offloads: Cell<*mut VirtioNetCtrlGuestOffloads>,
    /// `sc_ctrl_mac_tbl_uc`, also `sc_ctrl_mac_info`.
    pub sc_ctrl_mac_tbl_uc: Cell<*mut VirtioNetCtrlMacTbl>,
    /// `sc_ctrl_mac_tbl_mc`.
    pub sc_ctrl_mac_tbl_mc: Cell<*mut VirtioNetCtrlMacTbl>,

    // sc_intrmap: struct intrmap, always NULL here (see the deviations).
    /// `sc_q`: `sc_nqueues` queues.
    pub sc_q: Cell<*mut VioQueue>,
    /// `sc_nqueues`.
    pub sc_nqueues: Cell<u16>,
    /// `sc_rxr_lwm`.
    pub sc_rxr_lwm: Cell<u16>,
    /// `sc_tx_slots_per_req`.
    pub sc_tx_slots_per_req: Cell<i32>,
    /// `sc_rx_mbuf_size`.
    pub sc_rx_mbuf_size: Cell<i32>,
    /// `sc_rx_mbuf_offset`.
    pub sc_rx_mbuf_offset: Cell<i32>,

    /// `sc_ctrl_inuse`.
    pub sc_ctrl_inuse: Cell<VioCtrlState>,

    /// `sc_txtick`.
    pub sc_txtick: Timeout,
    /// `sc_rxtick`.
    pub sc_rxtick: Timeout,
}

impl VioSoftc {
    /// `sc->sc_virtio`.
    pub fn virtio(&self) -> &'static VirtioSoftc {
        // SAFETY: vio_attach sets it to the parent's softc, which outlives its child.
        unsafe { nonnull(self.sc_virtio.get(), "virtio softc") }
    }

    /// `sc->sc_ctl_vq`.
    pub fn ctl_vq(&self) -> &'static Virtqueue {
        // SAFETY: one of the parent's `sc_vqs`, set by vio_attach with the control queue.
        unsafe { nonnull(self.sc_ctl_vq.get(), "control queue") }
    }

    /// `&sc->sc_q[i]`.
    pub fn q(&self, i: usize) -> &'static VioQueue {
        let base = self.sc_q.get();
        if base.is_null() || i >= usize::from(self.sc_nqueues.get()) {
            panic(format_args!("vio: no queue {i}"));
        }
        // SAFETY: vio_attach allocated `sc_nqueues` zeroed queues (valid all-zero) that
        // live as long as the softc.
        unsafe { &*base.add(i) }
    }

    /// `sc->sc_dma_map`.
    pub fn dma_map(&self) -> &'static BusDmamap {
        match self.sc_dma_map.get() {
            Some(m) => m,
            None => panic(format_args!("vio: no dma map")),
        }
    }

    /// `&sc->sc_ac.ac_if`.
    pub fn ifp(&'static self) -> &'static Ifnet {
        &self.sc_ac.ac_if
    }
}

// SAFETY: `#[repr(C)]` with the device first; the arpcom and the timeouts are all-zero valid
// (`netinet/if_ether.rs`, `sys/timeout.rs`), and every other member is a `Cell` of an
// integer, a pointer, an `Option` or an enum whose zero is `FREE`.
unsafe impl Softc for VioSoftc {}

/// `VIO_DMAMEM_OFFSET(sc, p)`.
fn vio_dmamem_offset(sc: &VioSoftc, p: *const u8) -> usize {
    (p as usize) - (sc.sc_dma_kva.get() as usize)
}

/// `VIO_DMAMEM_SYNC(vsc, sc, p, size, flags)`.
fn vio_dmamem_sync(vsc: &VirtioSoftc, sc: &VioSoftc, p: *const u8, size: usize, flags: i32) {
    bus_dmamap_sync(
        vsc.dmat(),
        sc.dma_map(),
        vio_dmamem_offset(sc, p),
        size,
        flags,
    );
}

/// `VIO_VQ2Q(sc, vq)`: vioq N uses the rx/tx vq pair 2*N and 2*N + 1.
fn vio_vq2q(sc: &VioSoftc, vq: &Virtqueue) -> &'static VioQueue {
    sc.q(vq.vq_index.get() as usize / 2)
}

/// `VIRTIO_NET_CTRL_MAC_MC_ENTRIES`: for more entries, use ALLMULTI.
pub const VIRTIO_NET_CTRL_MAC_MC_ENTRIES: usize = 64;
/// `VIRTIO_NET_CTRL_MAC_UC_ENTRIES`: one entry for own unicast addr.
pub const VIRTIO_NET_CTRL_MAC_UC_ENTRIES: usize = 1;
/// `VIRTIO_NET_CTRL_TIMEOUT`: 5 seconds.
pub const VIRTIO_NET_CTRL_TIMEOUT: u64 = 5 * 1000 * 1000 * 1000;

/// `VIO_CTRL_MAC_INFO_SIZE`.
pub const VIO_CTRL_MAC_INFO_SIZE: usize = 2 * size_of::<VirtioNetCtrlMacTbl>()
    + (VIRTIO_NET_CTRL_MAC_MC_ENTRIES + VIRTIO_NET_CTRL_MAC_UC_ENTRIES) * ETHER_ADDR_LEN;

/// `offsetof(struct tcphdr, th_sum)` (`netinet/tcp.h`, not ported).
const TCPHDR_TH_SUM: u16 = 16;
/// `offsetof(struct udphdr, uh_sum)` (`netinet/udp.h`, not ported).
const UDPHDR_UH_SUM: u16 = 6;

/// `vio_ca`.
pub static VIO_CA: Cfattach = Cfattach {
    ca_devsize: size_of::<VioSoftc>(),
    ca_match: Some(vio_match),
    ca_attach: vio_attach,
    ca_detach: None,
    ca_activate: None,
};

/// `vio_cd`.
pub static VIO_CD: Cfdriver = Cfdriver::new(b"vio", DV_IFNET, CD_COCOVM);

/// `(struct vio_softc *)ifp->if_softc`.
pub fn vio_softc(ifp: &Ifnet) -> &'static VioSoftc {
    // SAFETY: vio_attach sets `if_softc` to its softc and installs vio's functions only on
    // its own interface; softcs outlive their interfaces.
    unsafe {
        nonnull(
            ifp.if_softc.get().cast::<VioSoftc>().cast_const(),
            "if_softc",
        )
    }
}

/// `(struct vio_softc *)vsc->sc_child`.
fn vio_child(vsc: &VirtioSoftc) -> &'static VioSoftc {
    let c = vsc.sc_child.get();
    if c.is_null() || c == VIRTIO_CHILD_ERROR {
        panic(format_args!("vio: virtio without its child"));
    }
    // SAFETY: vio_attach stored its own device, the head of a `VioSoftc` that lives as long
    // as the kernel.
    unsafe { &*c.cast::<VioSoftc>() }
}

/// `vio_match`: the virtio network device.
pub fn vio_match(_parent: Option<&Device>, _match: &CfMatch, aux: *mut c_void) -> i32 {
    // SAFETY: virtio transports attach their children with arguments that begin with a
    // `virtio_attach_args`.
    let va = unsafe { &*aux.cast::<VirtioAttachArgs>() };

    i32::from(va.va_devid == PCI_PRODUCT_VIRTIO_NETWORK)
}

/// `vio_alloc_dmamem`: the DMA area (one segment, mapped and loaded).
pub fn vio_alloc_dmamem(sc: &VioSoftc) -> Result<(), Errno> {
    let vsc = sc.virtio();
    let size = sc.sc_dma_size.get();

    let map = bus_dmamap_create(
        vsc.dmat(),
        size,
        1,
        size,
        0,
        BUS_DMA_NOWAIT | BUS_DMA_ALLOCNOW | BUS_DMA_64BIT,
    )
    .map_err(|_| Errno::ENOMEM)?;
    sc.sc_dma_map.set(Some(map));
    let destroy = || {
        // SAFETY: the map made above, which nothing else holds.
        unsafe { bus_dmamap_destroy(vsc.dmat(), NonNull::from(map)) };
        sc.sc_dma_map.set(None);
    };
    let mut seg = [BusDmaSegment::default(); 1];
    let nsegs = match bus_dmamem_alloc(
        vsc.dmat(),
        size,
        16,
        0,
        &mut seg,
        BUS_DMA_NOWAIT | BUS_DMA_ZERO | BUS_DMA_64BIT,
    ) {
        Ok(n) => n,
        Err(_) => {
            destroy();
            return Err(Errno::ENOMEM);
        }
    };
    sc.sc_dma_seg.set(seg[0]);
    let kva = match bus_dmamem_map(vsc.dmat(), &mut seg[..nsegs], size, BUS_DMA_NOWAIT) {
        Ok(kva) => kva,
        Err(_) => {
            // SAFETY: the segment allocated above, not mapped.
            unsafe { bus_dmamem_free(vsc.dmat(), &seg[..1]) };
            destroy();
            return Err(Errno::ENOMEM);
        }
    };
    sc.sc_dma_kva.set(kva.as_ptr());
    // SAFETY: `kva` maps `size` bytes that stay until vio_free_dmamem, which unloads first.
    if unsafe { bus_dmamap_load(vsc.dmat(), map, kva.as_ptr(), size, None, BUS_DMA_NOWAIT) }
        .is_err()
    {
        // unmap:
        // SAFETY: the mapping made above.
        unsafe { bus_dmamem_unmap(vsc.dmat(), kva, size) };
        // free:
        // SAFETY: the segment allocated above, now unmapped.
        unsafe { bus_dmamem_free(vsc.dmat(), &seg[..1]) };
        // destroy:
        destroy();
        return Err(Errno::ENOMEM);
    }
    Ok(())
}

/// `vio_free_dmamem`.
pub fn vio_free_dmamem(sc: &VioSoftc) {
    let vsc = sc.virtio();
    let map = sc.dma_map();

    bus_dmamap_unload(vsc.dmat(), map);
    if let Some(kva) = NonNull::new(sc.sc_dma_kva.get()) {
        // SAFETY: the area vio_alloc_dmamem mapped, unloaded above and no longer used.
        unsafe { bus_dmamem_unmap(vsc.dmat(), kva, sc.sc_dma_size.get()) };
    }
    // SAFETY: the area's segment, unmapped above.
    unsafe { bus_dmamem_free(vsc.dmat(), &[sc.sc_dma_seg.get()]) };
    // SAFETY: the area's map, unloaded above.
    unsafe { bus_dmamap_destroy(vsc.dmat(), NonNull::from(map)) };
    sc.sc_dma_map.set(None);
    sc.sc_dma_kva.set(ptr::null_mut());
}

/// `vio_alloc_mem`: allocate memory.
///
/// dma memory is used for:
/// - `viq_txhdrs[slot]`: metadata array for frames to be sent (WRITE)
/// - `sc_ctrl_cmd`: command to be sent via ctrl vq (WRITE)
/// - `sc_ctrl_status`: return value for a command via ctrl vq (READ)
/// - `sc_ctrl_rx`: parameter for a `VIRTIO_NET_CTRL_RX` class command (WRITE)
/// - `sc_ctrl_mq_pairs_set`: set number of rx/tx queue pais (WRITE)
/// - `sc_ctrl_guest_offloads`: configure offload features (WRITE)
/// - `sc_ctrl_mac_tbl_uc`: unicast MAC address filter for a `VIRTIO_NET_CTRL_MAC` class
///   command (WRITE)
/// - `sc_ctrl_mac_tbl_mc`: multicast MAC address filter for a `VIRTIO_NET_CTRL_MAC` class
///   command (WRITE)
///
/// `sc_ctrl_*` structures are allocated only one each; they are protected by
/// `sc_ctrl_inuse`, which must only be accessed at splnet.
///
/// metadata headers for received frames are stored at the start of the rx mbufs.
///
/// dynamically allocated memory is used for:
/// - `viq_rxdmamaps[slot]`: `bus_dmamap_t` array for received payload
/// - `viq_txdmamaps[slot]`: `bus_dmamap_t` array for sent payload
/// - `viq_rxmbufs[slot]`: mbuf pointer array for received frames
/// - `viq_txmbufs[slot]`: mbuf pointer array for sent frames
pub fn vio_alloc_mem(sc: &VioSoftc, tx_max_segments: i32, txsize: BusSize) -> Result<(), Errno> {
    let vsc = sc.virtio();
    let mut offset = 0;

    let rxqsize = sc.q(0).rxvq().vq_num.get() as usize;
    let txqsize = sc.q(0).txvq().vq_num.get() as usize;
    let nqueues = usize::from(sc.sc_nqueues.get());

    // For simplicity, we always allocate the full virtio_net_hdr size even if
    // VIRTIO_NET_F_MRG_RXBUF is not negotiated and only a part of the memory is ever used.
    let mut allocsize = size_of::<VirtioNetHdr>() * txqsize * nqueues;

    if virtio_has_feature(vsc, VIRTIO_NET_F_CTRL_VQ) {
        allocsize += size_of::<VirtioNetCtrlCmd>();
        allocsize += size_of::<VirtioNetCtrlStatus>();
        allocsize += size_of::<VirtioNetCtrlRx>();
        allocsize += size_of::<VirtioNetCtrlMqPairsSet>();
        allocsize += size_of::<VirtioNetCtrlGuestOffloads>();
        allocsize += VIO_CTRL_MAC_INFO_SIZE;
    }
    sc.sc_dma_size.set(allocsize);

    if vio_alloc_dmamem(sc).is_err() {
        printf(format_args!("unable to allocate dma region\n"));
        return Err(Errno::ENOMEM);
    }

    let kva = sc.sc_dma_kva.get();
    // SAFETY: every offset below stays inside the `allocsize` bytes just mapped (the
    // kassert checks the sum).
    let at = |off: usize| unsafe { kva.add(off) };

    for qidx in 0..nqueues {
        sc.q(qidx).viq_txhdrs.set(at(offset).cast());
        offset += size_of::<VirtioNetHdr>() * txqsize;
    }

    if virtio_has_feature(vsc, VIRTIO_NET_F_CTRL_VQ) {
        sc.sc_ctrl_cmd.set(at(offset).cast());
        offset += size_of::<VirtioNetCtrlCmd>();
        sc.sc_ctrl_status.set(at(offset).cast());
        offset += size_of::<VirtioNetCtrlStatus>();
        sc.sc_ctrl_rx.set(at(offset).cast());
        offset += size_of::<VirtioNetCtrlRx>();
        sc.sc_ctrl_mq_pairs.set(at(offset).cast());
        offset += size_of::<VirtioNetCtrlMqPairsSet>();
        sc.sc_ctrl_guest_offloads.set(at(offset).cast());
        offset += size_of::<VirtioNetCtrlGuestOffloads>();
        sc.sc_ctrl_mac_tbl_uc.set(at(offset).cast());
        offset +=
            size_of::<VirtioNetCtrlMacTbl>() + ETHER_ADDR_LEN * VIRTIO_NET_CTRL_MAC_UC_ENTRIES;
        sc.sc_ctrl_mac_tbl_mc.set(at(offset).cast());
        offset +=
            size_of::<VirtioNetCtrlMacTbl>() + ETHER_ADDR_LEN * VIRTIO_NET_CTRL_MAC_MC_ENTRIES;
    }
    kassert!(offset == allocsize);

    let arraysize = (rxqsize + txqsize) * (size_of::<MapSlot>() + size_of::<MbufSlot>());
    let mut r: Result<(), Errno> = Ok(());
    'destroy: for qidx in 0..nqueues {
        let vioq = sc.q(qidx);

        let Some(arrays) = mallocarray(
            rxqsize + txqsize,
            size_of::<MapSlot>() + size_of::<MbufSlot>(),
            M_DEVBUF,
            M_WAITOK | M_ZERO,
        ) else {
            printf(format_args!("unable to allocate mem for dmamaps\n"));
            vio_free_dmamem(sc);
            return Err(Errno::ENOMEM);
        };
        let maps = arrays.as_ptr().cast::<MapSlot>();
        vioq.viq_arrays.set(maps);
        // SAFETY: the four arrays follow one another inside the allocation.
        unsafe {
            vioq.viq_txdmamaps.set(maps.add(rxqsize));
            let mbufs = maps.add(rxqsize + txqsize).cast::<MbufSlot>();
            vioq.viq_rxmbufs.set(mbufs);
            vioq.viq_txmbufs.set(mbufs.add(rxqsize));
        }

        for i in 0..rxqsize {
            match bus_dmamap_create(
                vsc.dmat(),
                sc.sc_rx_mbuf_size.get() as BusSize,
                2,
                sc.sc_rx_mbuf_size.get() as BusSize,
                0,
                BUS_DMA_NOWAIT | BUS_DMA_ALLOCNOW | BUS_DMA_64BIT,
            ) {
                Ok(map) => vioq.rxdmamap_slot(i).set(Some(map)),
                Err(e) => {
                    r = Err(e);
                    break 'destroy;
                }
            }
        }

        for i in 0..txqsize {
            match bus_dmamap_create(
                vsc.dmat(),
                txsize,
                tx_max_segments,
                txsize,
                0,
                BUS_DMA_NOWAIT | BUS_DMA_ALLOCNOW | BUS_DMA_64BIT,
            ) {
                Ok(map) => vioq.txdmamap_slot(i).set(Some(map)),
                Err(e) => {
                    r = Err(e);
                    break 'destroy;
                }
            }
        }
    }

    let Err(e) = r else {
        return Ok(());
    };

    // destroy:
    printf(format_args!("dmamap creation failed, error {}\n", e as i32));
    for qidx in 0..nqueues {
        let vioq = sc.q(qidx);
        let Some(arrays) = NonNull::new(vioq.viq_arrays.get()) else {
            continue;
        };

        for i in 0..txqsize {
            let Some(map) = vioq.txdmamap_slot(i).get() else {
                break;
            };
            // SAFETY: a map made above, in no use yet.
            unsafe { bus_dmamap_destroy(vsc.dmat(), NonNull::from(map)) };
        }
        for i in 0..rxqsize {
            let Some(map) = vioq.rxdmamap_slot(i).get() else {
                break;
            };
            // SAFETY: as above.
            unsafe { bus_dmamap_destroy(vsc.dmat(), NonNull::from(map)) };
        }
        free(arrays.cast(), M_DEVBUF, arraysize);
        vioq.viq_arrays.set(ptr::null_mut());
    }
    // free:
    vio_free_dmamem(sc);
    Err(Errno::ENOMEM)
}

/// `vio_dmamem_enqueue`: `size` bytes at `p` of the DMA area as the next descriptor of
/// `slot`.
fn vio_dmamem_enqueue(
    vsc: &VirtioSoftc,
    sc: &VioSoftc,
    vq: &Virtqueue,
    slot: i32,
    p: *const u8,
    size: usize,
    write: bool,
) {
    vio_dmamem_sync(
        vsc,
        sc,
        p,
        size,
        if write {
            BUS_DMASYNC_PREWRITE
        } else {
            BUS_DMASYNC_PREREAD
        },
    );
    virtio_enqueue_p(
        vq,
        slot,
        sc.dma_map(),
        vio_dmamem_offset(sc, p),
        size,
        write,
    );
}

/// `vio_get_lladdr`: the MAC address from the device configuration.
pub fn vio_get_lladdr(ac: &Arpcom, vsc: &VirtioSoftc) {
    let mut enaddr = [0u8; ETHER_ADDR_LEN];
    for (i, b) in enaddr.iter_mut().enumerate() {
        *b = virtio_read_device_config_1(vsc, VIRTIO_NET_CONFIG_MAC + i as i32);
    }
    ac.ac_enaddr.set(enaddr);
}

/// `vio_needs_reset`.
fn vio_needs_reset(sc: &VioSoftc) -> bool {
    if virtio_get_status(sc.virtio()) & VIRTIO_CONFIG_DEVICE_STATUS_DEVICE_NEEDS_RESET != 0 {
        printf(format_args!(
            "{}: device needs reset\n",
            Str(&sc.sc_dev.dv_xname.get())
        ));
        vio_ctrl_wakeup(sc, RESET);
        return true;
    }
    false
}

/// `vio_attach`.
pub fn vio_attach(parent: Option<&Device>, self_: &Device, aux: *mut c_void) {
    // SAFETY: `self_` was made for `vio_ca`, whose softc is a `VioSoftc`; softcs are never
    // freed while their interface exists, so the softc may be borrowed for 'static.
    let sc: &'static VioSoftc = unsafe { &*ptr::from_ref(self_.softc::<VioSoftc>()) };
    let Some(parent) = parent else {
        return;
    };
    // SAFETY: `vio* at virtio?`: the parent is a virtio transport, whose softc begins with
    // the `struct virtio_softc`; it outlives its child.
    let vsc: &'static VirtioSoftc = unsafe { &*ptr::from_ref(parent.softc::<VirtioSoftc>()) };
    let va = aux.cast::<VirtioAttachArgs>();
    // SAFETY: the transport's attach arguments, alive for the duration of this attach.
    let va_nintr = unsafe { (*va).va_nintr };
    let ifp = sc.ifp();
    let mut want_tso = true;
    let mut want_mtu = true;
    let mut device_mtu: u32 = 0;

    if !vsc.sc_child.get().is_null() {
        printf(format_args!(
            ": child already attached for {}; something wrong...\n",
            Str(&parent.dv_xname.get())
        ));
        return;
    }

    sc.sc_virtio.set(vsc);

    vsc.sc_child.set(ptr::from_ref(self_));
    vsc.sc_ipl.set(IPL_NET | IPL_MPSAFE);

    let r: Result<(), Errno> = 'err: {
        // negotiate:
        loop {
            let mut f = VIRTIO_F_RING_EVENT_IDX;
            f |= VIRTIO_NET_F_MAC;
            f |= VIRTIO_NET_F_STATUS;
            f |= VIRTIO_NET_F_CTRL_VQ;
            f |= VIRTIO_NET_F_CTRL_RX;
            f |= VIRTIO_NET_F_MRG_RXBUF;
            f |= VIRTIO_NET_F_CSUM;
            f |= VIRTIO_NET_F_GUEST_CSUM;
            if want_tso {
                f |= VIRTIO_NET_F_CTRL_GUEST_OFFLOADS;
                f |= VIRTIO_NET_F_GUEST_TSO4;
                f |= VIRTIO_NET_F_GUEST_TSO6;
                f |= VIRTIO_NET_F_HOST_TSO4;
                f |= VIRTIO_NET_F_HOST_TSO6;
            }
            if want_mtu {
                f |= VIRTIO_NET_F_MTU;
            }
            if va_nintr > 3 && NCPUS.load(core::sync::atomic::Ordering::Relaxed) > 1 {
                f |= VIRTIO_NET_F_MQ;
            }
            vsc.sc_driver_features.set(f);

            if let Err(e) = virtio_negotiate_features(vsc, Some(virtio_net_feature_names())) {
                break 'err Err(e);
            }

            if (virtio_has_feature(vsc, VIRTIO_NET_F_GUEST_TSO4)
                || virtio_has_feature(vsc, VIRTIO_NET_F_GUEST_TSO6))
                && (!virtio_has_feature(vsc, VIRTIO_NET_F_CTRL_GUEST_OFFLOADS)
                    || !virtio_has_feature(vsc, VIRTIO_NET_F_MRG_RXBUF))
            {
                // We only support VIRTIO_NET_F_GUEST_TSO4/6 if we also have both
                // VIRTIO_NET_F_CTRL_GUEST_OFFLOADS and VIRTIO_NET_F_MRG_RXBUF. In order to
                // make this known to the hypervisor, we need to redo feature negotiation.
                //
                // Unfortunately, Apple Virtualisation does not work with only
                // VIRTIO_NET_F_HOST_TSO4/6. Therefore we disable all TSO in the fallback
                // case.
                want_tso = false;
                virtio_reset(vsc);
                continue;
            }

            if virtio_has_feature(vsc, VIRTIO_NET_F_MTU) {
                device_mtu = u32::from(virtio_read_device_config_2(vsc, VIRTIO_NET_CONFIG_MTU));
                if device_mtu as usize > ETHER_MAX_HARDMTU_LEN {
                    // According to the standard we MUST supply rx buffers for the MTU
                    // length, but we only support up to ETHER_MAX_HARDMTU_LEN.
                    printf(format_args!(
                        "{}: mtu {device_mtu} not supported\n",
                        Str(&sc.sc_dev.dv_xname.get())
                    ));
                    device_mtu = 0;
                    want_mtu = false;
                    virtio_reset(vsc);
                    continue;
                }
            }
            break;
        }

        if virtio_has_feature(vsc, VIRTIO_NET_F_MQ) {
            let i = virtio_read_device_config_2(vsc, VIRTIO_NET_CONFIG_MAX_QUEUES);
            vsc.sc_nvqs.set(2 * i32::from(i) + 1);
            let _i = i.min(VIRTIO_NET_CTRL_MQ_VQ_PAIRS_MAX);
            // sc->sc_intrmap = intrmap_create(&sc->sc_dev, i, MIN(va_nintr - 2,
            // IF_MAX_VECTORS), 0); sc->sc_nqueues = intrmap_count(sc->sc_intrmap):
            // kern_intrmap.c is not ported; one queue pair is used.
            let _ = (va_nintr.saturating_sub(2) as usize).min(IF_MAX_VECTORS);
            let _ = unported!("intrmap_create, intrmap_count (kern/kern_intrmap.c)");
            sc.sc_nqueues.set(1);
            let n = sc.sc_nqueues.get();
            printf(format_args!(": {n} queue{}", if n > 1 { "s" } else { "" }));
        } else {
            sc.sc_nqueues.set(1);
            printf(format_args!(": 1 queue"));
            vsc.sc_nvqs.set(2);
            if virtio_has_feature(vsc, VIRTIO_NET_F_CTRL_VQ) {
                vsc.sc_nvqs.set(vsc.sc_nvqs.get() + 1);
            }
        }

        let Some(vqs) = mallocarray(
            vsc.sc_nvqs.get() as usize,
            size_of::<Virtqueue>(),
            M_DEVBUF,
            M_WAITOK | M_ZERO,
        ) else {
            vsc.sc_nvqs.set(0);
            break 'err Err(Errno::ENOMEM);
        };
        vsc.sc_vqs.set(vqs.as_ptr().cast());

        let Some(q) = mallocarray(
            usize::from(sc.sc_nqueues.get()),
            size_of::<VioQueue>(),
            M_DEVBUF,
            M_WAITOK | M_ZERO,
        ) else {
            break 'err Err(Errno::ENOMEM);
        };
        if !(q.as_ptr() as usize).is_multiple_of(align_of::<VioQueue>()) {
            panic(format_args!("vio: misaligned queues"));
        }
        sc.sc_q.set(q.as_ptr().cast());

        if virtio_has_feature(vsc, VIRTIO_NET_F_MAC) {
            vio_get_lladdr(&sc.sc_ac, vsc);
        } else {
            // SAFETY: `ifp` is the `ac_if` of this softc's arpcom, which lives as long as
            // the interface; ether_fakeaddr reaches the arpcom through it before
            // ether_ifattach marks it.
            unsafe { ifp.set_arpcom() };
            ether_fakeaddr(ifp);
        }
        printf(format_args!(
            ", address {}",
            Str(&ether_sprintf(&sc.sc_ac.ac_enaddr.get()))
        ));

        if virtio_has_feature(vsc, VIRTIO_NET_F_MRG_RXBUF) || vsc.sc_version_1.get() != 0 {
            sc.sc_hdr_size.set(size_of::<VirtioNetHdr>() as i32);
        } else {
            sc.sc_hdr_size
                .set(offset_of!(VirtioNetHdr, num_buffers) as i32);
        }
        let u32sz = size_of::<u32>() as i32;
        sc.sc_rx_mbuf_offset
            .set((ETHER_ALIGN as i32 + u32sz - sc.sc_hdr_size.get() % u32sz) % u32sz);

        ifp.if_capabilities.set(0);
        ifp.if_flags
            .set(IFF_BROADCAST | IFF_SIMPLEX | IFF_MULTICAST);
        ifp.if_xflags.set(IFXF_MPSAFE | IFXF_MBUF_64BIT);
        // NVLAN > 0: IFCAP_VLAN_MTU, IFCAP_VLAN_HWOFFLOAD; vlan(4) is not configured.
        if virtio_has_feature(vsc, VIRTIO_NET_F_CSUM) {
            ifp.if_capabilities.set(
                ifp.if_capabilities.get()
                    | IFCAP_CSUM_TCPv4
                    | IFCAP_CSUM_UDPv4
                    | IFCAP_CSUM_TCPv6
                    | IFCAP_CSUM_UDPv6,
            );
        }
        if virtio_has_feature(vsc, VIRTIO_NET_F_HOST_TSO4) {
            ifp.if_capabilities
                .set(ifp.if_capabilities.get() | IFCAP_TSOv4);
        }
        if virtio_has_feature(vsc, VIRTIO_NET_F_HOST_TSO6) {
            ifp.if_capabilities
                .set(ifp.if_capabilities.get() | IFCAP_TSOv6);
        }

        sc.sc_rx_mbuf_size.set(MCLBYTES as i32);
        if virtio_has_feature(vsc, VIRTIO_NET_F_CTRL_GUEST_OFFLOADS)
            && (virtio_has_feature(vsc, VIRTIO_NET_F_GUEST_TSO4)
                || virtio_has_feature(vsc, VIRTIO_NET_F_GUEST_TSO6))
        {
            ifp.if_xflags.set(ifp.if_xflags.get() | IFXF_LRO);
            ifp.if_capabilities
                .set(ifp.if_capabilities.get() | IFCAP_LRO);
            sc.sc_rx_mbuf_size.set(4 * 1024);
        }

        let hdr_size = sc.sc_hdr_size.get() as u32;
        let rx_off = sc.sc_rx_mbuf_offset.get() as u32;
        ifp.if_hardmtu.set(ETHER_MAX_HARDMTU_LEN as u32);
        if device_mtu != 0 {
            printf(format_args!(", mtu {device_mtu}"));
            ifp.if_hardmtu.set(device_mtu);
        }
        if !virtio_has_feature(vsc, VIRTIO_NET_F_MRG_RXBUF) {
            if device_mtu != 0 {
                sc.sc_rx_mbuf_size.set(
                    sc.sc_rx_mbuf_size
                        .get()
                        .max((rx_off + device_mtu + hdr_size + ETHER_HDR_LEN as u32) as i32),
                );
            }
            ifp.if_hardmtu.set(
                ifp.if_hardmtu.get().min(
                    sc.sc_rx_mbuf_size.get() as u32 - rx_off - hdr_size - ETHER_HDR_LEN as u32,
                ),
            );
        }

        let mut txsize = (ifp.if_hardmtu.get() + hdr_size) as BusSize + ETHER_HDR_LEN as BusSize;
        if virtio_has_feature(vsc, VIRTIO_NET_F_HOST_TSO4)
            || virtio_has_feature(vsc, VIRTIO_NET_F_HOST_TSO6)
        {
            txsize = MAXMCLBYTES + hdr_size as BusSize;
        }

        let mut rxsize = (ifp.if_hardmtu.get() + rx_off + hdr_size) as BusSize + ETHER_HDR_LEN;
        if virtio_has_feature(vsc, VIRTIO_NET_F_GUEST_TSO4)
            || virtio_has_feature(vsc, VIRTIO_NET_F_GUEST_TSO6)
        {
            rxsize = MAXMCLBYTES + (rx_off + hdr_size) as BusSize;
        }

        printf(format_args!("\n"));
        sc.sc_rxr_lwm
            .set(2 * howmany(rxsize, (sc.sc_rx_mbuf_size.get() as u32 - rx_off) as usize) as u16);

        // defrag for longer mbuf chains
        let mut tx_max_segments = 16;
        if virtio_has_feature(vsc, VIRTIO_NET_F_HOST_TSO4)
            || virtio_has_feature(vsc, VIRTIO_NET_F_HOST_TSO6)
        {
            // With TSO, we may get 64K packets and want to be able to send longer chains
            // without defragmenting
            tx_max_segments = 32;
        }

        if virtio_has_feature(vsc, VIRTIO_F_RING_INDIRECT_DESC) {
            sc.sc_tx_slots_per_req.set(1);
        } else {
            sc.sc_tx_slots_per_req.set(tx_max_segments + 1);
        }

        for i in 0..usize::from(sc.sc_nqueues.get()) {
            let mut vqidx = 2 * i;
            let vioq = sc.q(i);

            vioq.viq_rxvq.set(vsc.vq(vqidx));
            mtx_init(&vioq.viq_txmtx, IPL_NET);
            mtx_init(&vioq.viq_rxmtx, IPL_NET);
            vioq.viq_sc.set(sc);
            if let Err(e) = virtio_alloc_vq(vsc, vioq.rxvq(), vqidx as i32, 2, "rx") {
                break 'err Err(e);
            }
            vioq.rxvq().vq_done.set(Some(vio_rx_intr));
            virtio_start_vq_intr(vsc, vioq.rxvq());

            vqidx += 1;
            vioq.viq_txvq.set(vsc.vq(vqidx));
            if let Err(e) =
                virtio_alloc_vq(vsc, vioq.txvq(), vqidx as i32, tx_max_segments + 1, "tx")
            {
                break 'err Err(e);
            }
            vioq.txvq().vq_done.set(Some(vio_tx_intr));
            if virtio_has_feature(vsc, VIRTIO_F_RING_EVENT_IDX) {
                virtio_postpone_intr_far(vioq.txvq());
            } else {
                virtio_stop_vq_intr(vsc, vioq.txvq());
            }
            vioq.viq_txfree_slots
                .set(vioq.txvq().vq_num.get() as i32 - 1);
            kassert!(vioq.viq_txfree_slots.get() > sc.sc_tx_slots_per_req.get());
            if vioq.txvq().vq_num.get() != sc.q(0).txvq().vq_num.get() {
                printf(format_args!(
                    "inequal tx queue size {i}: {} != {}\n",
                    vioq.txvq().vq_num.get(),
                    sc.q(0).txvq().vq_num.get()
                ));
                break 'err Err(Errno::EINVAL);
            }
            if VIRTIO_DEBUG > 0 {
                printf(format_args!(
                    "{i}: q {:p} rx {:p} tx {:p}\n",
                    ptr::from_ref(vioq),
                    vioq.viq_rxvq.get(),
                    vioq.viq_txvq.get()
                ));
            }

            // sc->sc_intrmap != NULL: vq_intr_vec = i + 2; sc_intrmap is always NULL.
        }

        // control queue
        if virtio_has_feature(vsc, VIRTIO_NET_F_CTRL_VQ) {
            let mut i = 2;
            if virtio_has_feature(vsc, VIRTIO_NET_F_MQ) {
                i = 2 * usize::from(virtio_read_device_config_2(
                    vsc,
                    VIRTIO_NET_CONFIG_MAX_QUEUES,
                ));
            }
            sc.sc_ctl_vq.set(vsc.vq(i));
            if let Err(e) = virtio_alloc_vq(vsc, sc.ctl_vq(), i as i32, 1, "control") {
                break 'err Err(e);
            }
            sc.ctl_vq().vq_done.set(Some(vio_ctrleof));
            // sc->sc_intrmap != NULL: vq_intr_vec = 1; sc_intrmap is always NULL.
            virtio_start_vq_intr(vsc, sc.ctl_vq());
        }

        // sc->sc_intrmap != NULL: virtio_intr_establish for the config interrupt (vector
        // 0, vio_config_intr), the control queue (1, vio_ctrl_intr) and each queue pair
        // (i + 2, vio_queue_intr on intrmap_cpu); sc_intrmap is always NULL.
        let _ = (
            vio_config_intr,
            vio_ctrl_intr,
            vio_queue_intr,
            virtio_intr_establish,
        );

        if let Err(e) = vio_alloc_mem(sc, tx_max_segments, txsize) {
            break 'err Err(e);
        }

        let mut xname = sc.sc_dev.dv_xname.get();
        xname[IFNAMSIZ - 1] = 0;
        ifp.if_xname.set(xname);
        ifp.if_softc.set(ptr::from_ref(sc).cast_mut().cast());
        ifp.if_qstart.set(Some(vio_start));
        ifp.if_ioctl.set(Some(vio_ioctl));

        ifq_init_maxlen(&ifp.if_snd, vsc.vq(1).vq_num.get() - 1);
        // ifmedia_init(&sc->sc_media, 0, vio_media_change, vio_media_status);
        // ifmedia_add(&sc->sc_media, IFM_ETHER | IFM_AUTO, 0, NULL);
        // ifmedia_set(&sc->sc_media, IFM_ETHER | IFM_AUTO);
        let _ = unported!("ifmedia_init, ifmedia_add, ifmedia_set (net/if_media.c)");
        vsc.sc_config_change.set(Some(vio_config_change));
        let arg: *mut c_void = ptr::from_ref(sc).cast_mut().cast();
        timeout_set(&sc.sc_txtick, vio_txtick, arg);
        timeout_set(&sc.sc_rxtick, vio_rxtick, arg);

        if let Err(e) = virtio_attach_finish(vsc, va) {
            break 'err Err(e);
        }

        if virtio_has_feature(vsc, VIRTIO_NET_F_MQ) {
            // ctrl queue works only after DRIVER_OK
            let _ = vio_ctrl_mq(sc);
        }

        if_attach(ifp);
        ether_ifattach(&sc.sc_ac);
        vio_link_state(ifp);
        if device_mtu != 0 {
            ifp.if_mtu.set(device_mtu);
        }

        let nq = u32::from(sc.sc_nqueues.get());
        if_attach_queues(ifp, nq);
        if_attach_iqueues(ifp, nq);

        for i in 0..nq {
            let vioq = sc.q(i as usize);
            ifp.ifq(i)
                .ifq_softc
                .set(ptr::from_ref(vioq).cast_mut().cast());
            vioq.viq_ifq.set(Some(ifp.ifq(i)));
            vioq.viq_ifiq.set(Some(ifp.ifiq(i)));
        }

        Ok(())
    };

    if r.is_ok() {
        return;
    }

    // err:
    for vq in vsc.vqs() {
        let _ = virtio_free_vq(vsc, vq);
    }
    if let Some(vqs) = NonNull::new(vsc.sc_vqs.get()) {
        free(
            vqs.cast(),
            M_DEVBUF,
            vsc.sc_nvqs.get() as usize * size_of::<Virtqueue>(),
        );
    }
    vsc.sc_vqs.set(ptr::null_mut());
    if let Some(q) = NonNull::new(sc.sc_q.get()) {
        free(
            q.cast(),
            M_DEVBUF,
            usize::from(sc.sc_nqueues.get()) * size_of::<VioQueue>(),
        );
    }
    sc.sc_q.set(ptr::null_mut());
    vsc.sc_nvqs.set(0);
    vsc.sc_child.set(VIRTIO_CHILD_ERROR);
}

/// `vio_link_state`: check link status.
pub fn vio_link_state(ifp: &'static Ifnet) {
    let sc = vio_softc(ifp);
    let vsc = sc.virtio();
    let mut link_state = LINK_STATE_FULL_DUPLEX;

    if virtio_has_feature(vsc, VIRTIO_NET_F_STATUS) {
        let status = virtio_read_device_config_2(vsc, VIRTIO_NET_CONFIG_STATUS);
        if status & VIRTIO_NET_S_LINK_UP == 0 {
            link_state = LINK_STATE_DOWN;
        }
    }
    if ifp.if_link_state.get() != link_state {
        ifp.if_link_state.set(link_state);
        if_link_state_change(ifp);
    }
}

/// `vio_queue_intr`: interrupt handler for multi-queue.
pub fn vio_queue_intr(arg: *mut c_void) -> i32 {
    // SAFETY: established with one of the softc's queues as its argument.
    let vioq = unsafe { &*arg.cast::<VioQueue>() };
    let vsc = vioq.sc().virtio();
    let mut r = virtio_check_vq(vsc, vioq.txvq());
    r |= virtio_check_vq(vsc, vioq.rxvq());
    r
}

/// `vio_config_intr`.
pub fn vio_config_intr(arg: *mut c_void) -> i32 {
    // SAFETY: established with the virtio softc as its argument.
    let vsc = unsafe { &*arg.cast::<VirtioSoftc>() };
    vio_config_change(vsc)
}

/// `vio_ctrl_intr`.
pub fn vio_ctrl_intr(arg: *mut c_void) -> i32 {
    // SAFETY: established with the control queue as its argument.
    let vq = unsafe { &*arg.cast::<Virtqueue>() };
    virtio_check_vq(vq.owner(), vq)
}

/// `vio_config_change`: the link state may have changed, or the device needs a reset.
pub fn vio_config_change(vsc: &VirtioSoftc) -> i32 {
    let sc = vio_child(vsc);
    // KERNEL_LOCK(): nothing without MULTIPROCESSOR.
    vio_link_state(sc.ifp());
    vio_needs_reset(sc);
    // KERNEL_UNLOCK()
    1
}

/// `vio_media_change`: ignore.
pub fn vio_media_change(_ifp: &Ifnet) -> Result<(), Errno> {
    Ok(())
}

/// `vio_media_status`.
pub fn vio_media_status(ifp: &'static Ifnet, imr: &mut Ifmediareq) {
    imr.ifm_active = IFM_ETHER | IFM_AUTO;
    imr.ifm_status = IFM_AVALID;

    vio_link_state(ifp);
    if link_state_is_up(ifp.if_link_state.get()) && ifp.if_flags.get() & IFF_UP != 0 {
        imr.ifm_status |= IFM_ACTIVE | IFM_FDX;
    }
}

/// `vio_set_offloads`: tell the device which receive offloads to use.
pub fn vio_set_offloads(ifp: &Ifnet) {
    let sc = vio_softc(ifp);
    let vsc = sc.virtio();
    let mut features = 0;

    if virtio_has_feature(vsc, VIRTIO_NET_F_CTRL_GUEST_OFFLOADS) {
        if virtio_has_feature(vsc, VIRTIO_NET_F_GUEST_CSUM) {
            features |= VIRTIO_NET_F_GUEST_CSUM;
        }

        if ifp.if_xflags.get() & IFXF_LRO != 0 {
            if virtio_has_feature(vsc, VIRTIO_NET_F_GUEST_TSO4) {
                features |= VIRTIO_NET_F_GUEST_TSO4;
            }
            if virtio_has_feature(vsc, VIRTIO_NET_F_GUEST_TSO6) {
                features |= VIRTIO_NET_F_GUEST_TSO6;
            }
        }

        let _ = vio_ctrl_guest_offloads(sc, features);
    }
}

/// `vio_init`: interface function for ifnet: fill the receive rings and start.
pub fn vio_init(ifp: &'static Ifnet) -> Result<(), Errno> {
    let sc = vio_softc(ifp);

    vio_stop(ifp, false);
    for qidx in 0..usize::from(sc.sc_nqueues.get()) {
        let vioq = sc.q(qidx);

        mtx_enter(&vioq.viq_rxmtx);
        let mut ring = vioq.viq_rxring.get();
        if_rxr_init(
            &mut ring,
            u32::from(sc.sc_rxr_lwm.get()),
            vioq.rxvq().vq_num.get(),
        );
        vioq.viq_rxring.set(ring);
        vio_populate_rx_mbufs(sc, vioq);
        ifq_clr_oactive(vioq.ifq());
        mtx_leave(&vioq.viq_rxmtx);
    }
    vio_iff(sc);
    vio_link_state(ifp);
    vio_set_offloads(ifp);

    ifp.if_flags.set(ifp.if_flags.get() | IFF_RUNNING);

    Ok(())
}

/// `vio_stop`: reset the device (the only way to stop I/O and DMA), reclaim what it
/// finished and set it up again; `disable` also frees the receive buffers.
pub fn vio_stop(ifp: &'static Ifnet, disable: bool) {
    let sc = vio_softc(ifp);
    let vsc = sc.virtio();

    ifp.if_flags.set(ifp.if_flags.get() & !IFF_RUNNING);
    timeout_del(&sc.sc_txtick);
    timeout_del(&sc.sc_rxtick);
    // only way to stop I/O and DMA is resetting...
    virtio_reset(vsc);
    virtio_intr_barrier(vsc);
    for i in 0..usize::from(sc.sc_nqueues.get()) {
        mtx_enter(&sc.q(i).viq_rxmtx);
        vio_rxeof(sc.q(i));
        mtx_leave(&sc.q(i).viq_rxmtx);
    }

    if virtio_has_feature(vsc, VIRTIO_NET_F_CTRL_VQ) {
        vio_ctrl_wakeup(sc, RESET);
    }
    vio_tx_drain(sc);
    if disable {
        vio_rx_drain(sc);
    }

    virtio_reinit_start(vsc);
    for i in 0..usize::from(sc.sc_nqueues.get()) {
        virtio_start_vq_intr(vsc, sc.q(i).rxvq());
        virtio_stop_vq_intr(vsc, sc.q(i).txvq());
    }
    if virtio_has_feature(vsc, VIRTIO_NET_F_CTRL_VQ) {
        virtio_start_vq_intr(vsc, sc.ctl_vq());
    }
    virtio_reinit_end(vsc);
    if virtio_has_feature(vsc, VIRTIO_NET_F_MQ) {
        let _ = vio_ctrl_mq(sc);
    }
    if virtio_has_feature(vsc, VIRTIO_NET_F_CTRL_VQ) {
        vio_ctrl_wakeup(sc, FREE);
    }
}

/// `vio_cksum_update`: add the payload length to a pseudo header checksum.
pub fn vio_cksum_update(mut cksum: u32, paylen: u16) -> u16 {
    // Add payload length
    cksum += u32::from(paylen);

    // Fold back to 16 bit
    cksum += cksum >> 16;

    cksum as u16
}

/// `vio_tx_offload`: the checksum and segmentation requests of `m` in `hdr`.
pub fn vio_tx_offload(hdr: &mut VirtioNetHdr, m: &Mbuf) {
    let mut ext = EtherExtracted::new();
    let csum_flags = m.m_pkthdr().csum_flags.get();

    // Checksum Offload

    if csum_flags & M_TCP_CSUM_OUT == 0 && csum_flags & M_UDP_CSUM_OUT == 0 {
        return;
    }

    ether_extract_headers(m, &mut ext);

    // Consistency Checks
    if (ext.ip4.is_null() && ext.ip6.is_null()) || (ext.tcp.is_null() && ext.udp.is_null()) {
        return;
    }

    if (!ext.tcp.is_null() && csum_flags & M_TCP_CSUM_OUT == 0)
        || (!ext.udp.is_null() && csum_flags & M_UDP_CSUM_OUT == 0)
    {
        return;
    }

    hdr.csum_start = ETHER_HDR_LEN as u16;
    // NVLAN > 0: sizeof(*ext.evh) when ext.evh; vlan(4) is not configured.
    hdr.csum_start += ext.iphlen as u16;

    if !ext.tcp.is_null() {
        hdr.csum_offset = TCPHDR_TH_SUM;
    } else if !ext.udp.is_null() {
        hdr.csum_offset = UDPHDR_UH_SUM;
    }

    hdr.flags = VIRTIO_NET_HDR_F_NEEDS_CSUM;

    // TCP Segmentation Offload

    if csum_flags & M_TCP_TSO == 0 {
        return;
    }

    if ext.tcp.is_null() || m.m_pkthdr().ph_mss.get() == 0 {
        tcpstat_inc(TcpstatCounters::TcpsOutbadtso);
        return;
    }

    hdr.hdr_len = hdr.csum_start + ext.tcphlen as u16;
    hdr.gso_size = m.m_pkthdr().ph_mss.get();

    if !ext.ip4.is_null() {
        hdr.gso_type = VIRTIO_NET_HDR_GSO_TCPV4;
    } else if !ext.ip6.is_null() {
        #[cfg(feature = "inet6")]
        {
            hdr.gso_type = VIRTIO_NET_HDR_GSO_TCPV6;
        }
    }

    // VirtIO-Net needs pseudo header cksum with IP-payload length for TSO
    // SAFETY: ether_extract_headers found a whole TCP header at `ext.tcp` inside the mbuf's
    // first buffer; `th_sum` is a 16-bit field at offset 16, possibly unaligned.
    unsafe {
        let th_sum = ext.tcp.add(usize::from(TCPHDR_TH_SUM)).cast::<u16>();
        let sum = vio_cksum_update(
            u32::from(th_sum.read_unaligned()),
            crate::sys::endian::htons((ext.iplen - ext.iphlen) as u16),
        );
        th_sum.write_unaligned(sum);
    }

    let mss = u32::from(m.m_pkthdr().ph_mss.get());
    tcpstat_add(
        TcpstatCounters::TcpsOutpkttso,
        u64::from(ext.paylen.div_ceil(mss)),
    );
}

/// `vio_start`: transmit what the send queue holds.
pub fn vio_start(viq_ifq: &'static Ifqueue) {
    let Some(ifp) = viq_ifq.ifq_if.get() else {
        return;
    };
    // SAFETY: vio_attach sets each send queue's `ifq_softc` to its `vio_queue`.
    let vioq = unsafe {
        nonnull(
            viq_ifq.ifq_softc.get().cast::<VioQueue>().cast_const(),
            "ifq_softc",
        )
    };
    let sc = vio_softc(ifp);
    let vsc = sc.virtio();
    let vq = vioq.txvq();
    let mut queued = 0;

    mtx_enter(&vioq.viq_txmtx);
    let r = vio_tx_dequeue(vq);
    if r != 0 && ifq_is_oactive(viq_ifq) {
        ifq_clr_oactive(viq_ifq);
    }

    // again:
    loop {
        let free_slots = vioq.viq_txfree_slots.get();
        kassert!(free_slots >= 0);
        let mut used_slots = 0;
        loop {
            if free_slots - used_slots < sc.sc_tx_slots_per_req.get() {
                ifq_set_oactive(viq_ifq);
                break;
            }

            let Some(m) = ifq_dequeue(viq_ifq) else {
                break;
            };

            let slot = match virtio_enqueue_prep(vq) {
                Ok(slot) => slot,
                Err(Errno::EAGAIN) => {
                    printf(format_args!("vio_start: virtio_enqueue_prep failed?\n"));
                    m_freem(m);
                    viq_ifq.ifq_errors.set(viq_ifq.ifq_errors.get() + 1);
                    break;
                }
                Err(r) => panic(format_args!(
                    "{}: enqueue_prep for tx buffer: {}",
                    Str(&sc.sc_dev.dv_xname.get()),
                    r as i32
                )),
            };

            let hdr = vioq.txhdr(slot as usize);
            let mut h = VirtioNetHdr::default();
            vio_tx_offload(&mut h, m);
            let n = sc.sc_hdr_size.get() as usize;
            // SAFETY: the slot's header in the DMA area, which the device does not read
            // before the request is committed below.
            unsafe { ptr::copy_nonoverlapping(h.to_bytes().as_ptr(), hdr.cast::<u8>(), n) };

            if vio_encap(vioq, slot, m).is_err() {
                virtio_enqueue_abort(vq, slot);
                m_freem(m);
                viq_ifq.ifq_errors.set(viq_ifq.ifq_errors.get() + 1);
                continue;
            }
            let txmap = vioq.txdmamap(slot as usize);
            if virtio_enqueue_reserve(vq, slot, txmap.dm_nsegs.get() + 1).is_err() {
                printf(format_args!("vio_start: virtio_enqueue_reserve failed?\n"));
                m_freem(m);
                viq_ifq.ifq_errors.set(viq_ifq.ifq_errors.get() + 1);
                bus_dmamap_unload(vsc.dmat(), txmap);
                vioq.txmbuf(slot as usize).set(None);
                break;
            }
            if sc.sc_tx_slots_per_req.get() == 1 {
                used_slots += 1;
            } else {
                used_slots += txmap.dm_nsegs.get() + 1;
            }

            bus_dmamap_sync(
                vsc.dmat(),
                txmap,
                0,
                txmap.dm_mapsize.get(),
                BUS_DMASYNC_PREWRITE,
            );
            vio_dmamem_enqueue(vsc, sc, vq, slot, hdr.cast(), n, true);
            virtio_enqueue(vq, slot, txmap, true);
            virtio_enqueue_commit(vsc, vq, slot, false);
            queued += 1;
            let if_bpf = ifp.if_bpf.get();
            if !if_bpf.is_null() {
                let _ = bpf_mtap(if_bpf, m, BPF_DIRECTION_OUT);
            }
        }
        if used_slots > 0 {
            if used_slots > vioq.viq_txfree_slots.get() {
                printf(format_args!(
                    "vio_start: used_slots {used_slots} viq_txfree_slots {} free_slots {free_slots}\n",
                    vioq.viq_txfree_slots.get()
                ));
            }
            vioq.viq_txfree_slots
                .set(vioq.viq_txfree_slots.get() - used_slots);
            kassert!(vioq.viq_txfree_slots.get() >= 0);
        }
        if ifq_is_oactive(viq_ifq) && ifp.if_flags.get() & IFF_RUNNING != 0 {
            let r = if virtio_has_feature(vsc, VIRTIO_F_RING_EVENT_IDX) {
                virtio_postpone_intr_smart(vq)
            } else {
                virtio_start_vq_intr(vsc, vq)
            };
            if r {
                if vio_tx_dequeue(vq) != 0 {
                    ifq_clr_oactive(viq_ifq);
                }
                continue;
            }
        }
        break;
    }

    if queued > 0 {
        virtio_notify(vsc, vq);
        timeout_add_sec(&sc.sc_txtick, 1);
    }
    mtx_leave(&vioq.viq_txmtx);
}

/// `vio_dump` (`VIRTIO_DEBUG`).
pub fn vio_dump(sc: &VioSoftc) {
    printf(format_args!(
        "{} status dump:\n",
        Str(&sc.sc_ac.ac_if.if_xname.get())
    ));
    printf(format_args!(
        "tx tick active: {}\n",
        i32::from(!crate::sys::timeout::timeout_triggered(&sc.sc_txtick))
    ));
    printf(format_args!(
        "max tx slots per req {}\n",
        sc.sc_tx_slots_per_req.get()
    ));
    printf(format_args!(
        "rx tick active: {}\n",
        i32::from(!crate::sys::timeout::timeout_triggered(&sc.sc_rxtick))
    ));
    for i in 0..usize::from(sc.sc_nqueues.get()) {
        printf(format_args!("{i}: TX virtqueue:\n"));
        printf(format_args!(
            "  tx free slots {}\n",
            sc.q(i).viq_txfree_slots.get()
        ));
        virtio_vq_dump(sc.q(i).txvq());
        printf(format_args!("{i}: RX virtqueue:\n"));
        virtio_vq_dump(sc.q(i).rxvq());
    }
    if !sc.sc_ctl_vq.get().is_null() {
        printf(format_args!("CTL virtqueue:\n"));
        virtio_vq_dump(sc.ctl_vq());
        printf(format_args!(
            "ctrl_inuse: {}\n",
            sc.sc_ctrl_inuse.get() as i32
        ));
    }
}

/// `vio_rxr_info`: `SIOCGIFRXR`; `ifri` is the user's `struct if_rxrinfo`.
fn vio_rxr_info(sc: &VioSoftc, ifri: usize) -> Result<(), Errno> {
    let n = usize::from(sc.sc_nqueues.get());
    let Some(p) = mallocarray(
        n,
        size_of::<IfRxringInfo>(),
        crate::sys::malloc::M_TEMP,
        M_WAITOK | M_ZERO | crate::sys::malloc::M_CANFAIL,
    ) else {
        return Err(Errno::ENOMEM);
    };
    // SAFETY: a fresh zeroed array of `n` entries (valid all-zero), freed below.
    let ifrs = unsafe { core::slice::from_raw_parts_mut(p.as_ptr().cast::<IfRxringInfo>(), n) };

    for (i, ifr) in ifrs.iter_mut().enumerate() {
        ifr.ifr_size = sc.sc_rx_mbuf_size.get() as u32;
        snprintf(&mut ifr.ifr_name, format_args!("{i}"));
        ifr.ifr_info = sc.q(i).viq_rxring.get();
    }

    let error = if_rxr_info_ioctl(ifri, n as u32, ifrs);

    free(p, crate::sys::malloc::M_TEMP, n * size_of::<IfRxringInfo>());

    error
}

/// `vio_ioctl`.
///
/// # Safety
///
/// As for `IfIoctlFn` (`net/if_var.rs`): `data` is the kernel copy of the request the
/// command encodes.
pub unsafe fn vio_ioctl(ifp: &'static Ifnet, cmd: u64, data: *mut u8) -> Result<(), Errno> {
    let sc = vio_softc(ifp);
    // SAFETY: every command handled here passes a `struct ifreq` (the caller's contract).
    let ifr = unsafe { &*data.cast::<Ifreq>() };
    let mut r: Result<(), Errno> = Ok(());

    let s = splnet();
    match cmd {
        SIOCSIFADDR => {
            ifp.if_flags.set(ifp.if_flags.get() | IFF_UP);
            if ifp.if_flags.get() & IFF_RUNNING == 0 {
                let _ = vio_init(ifp);
            }
        }
        SIOCSIFFLAGS => {
            if ifp.if_flags.get() & IFF_UP != 0 {
                if VIRTIO_DEBUG > 0 && ifp.if_flags.get() & IFF_DEBUG != 0 {
                    vio_dump(sc);
                }
                if ifp.if_flags.get() & IFF_RUNNING != 0 {
                    r = Err(Errno::ENETRESET);
                } else {
                    let _ = vio_init(ifp);
                }
            } else if ifp.if_flags.get() & IFF_RUNNING != 0 {
                vio_stop(ifp, true);
            }
        }
        SIOCSIFXFLAGS => {
            let flags = i32::from(ifr.ifr_flags());
            if flags & IFXF_LRO != ifp.if_xflags.get() & IFXF_LRO {
                if flags & IFXF_LRO != 0 {
                    ifp.if_xflags.set(ifp.if_xflags.get() | IFXF_LRO);
                } else {
                    ifp.if_xflags.set(ifp.if_xflags.get() & !IFXF_LRO);
                }

                vio_set_offloads(ifp);
            }
        }
        SIOCGIFMEDIA | SIOCSIFMEDIA => {
            // ifmedia_ioctl(ifp, ifr, &sc->sc_media, cmd)
            r = Err(unported!("ifmedia_ioctl (net/if_media.c)"));
        }
        SIOCGIFRXR => {
            r = vio_rxr_info(sc, ifr.ifr_data() as usize);
        }
        _ => {
            // SAFETY: the caller's contract, forwarded.
            r = unsafe { ether_ioctl(ifp, &sc.sc_ac, cmd, data) };
        }
    }

    if r == Err(Errno::ENETRESET) {
        if ifp.if_flags.get() & IFF_RUNNING != 0 {
            vio_iff(sc);
        }
        r = Ok(());
    }
    splx(s);
    r
}

// Receive implementation

/// `vio_add_rx_mbuf`: allocate and initialize a mbuf for receive.
pub fn vio_add_rx_mbuf(sc: &VioSoftc, vioq: &VioQueue, i: usize) -> Result<(), Errno> {
    let Some(m) = m_clget(None, M_DONTWAIT, sc.sc_rx_mbuf_size.get() as u32) else {
        return Err(Errno::ENOBUFS);
    };
    vioq.rxmbuf(i).set(Some(m));
    let size = m.m_ext().ext_size.get();
    m.m_len().set(size);
    m.m_pkthdr().len.set(size as i32);
    m_adj(m, sc.sc_rx_mbuf_offset.get());
    // SAFETY: the cluster stays the slot's (viq_rxmbufs) until vio_rxeof or
    // vio_free_rx_mbuf unload the map, before freeing or handing it on.
    let r = unsafe {
        bus_dmamap_load_mbuf(
            sc.virtio().dmat(),
            vioq.rxdmamap(i),
            m,
            BUS_DMA_READ | BUS_DMA_NOWAIT,
        )
    };
    if let Err(r) = r {
        m_freem(m);
        vioq.rxmbuf(i).set(None);
        return Err(r);
    }

    Ok(())
}

/// `vio_free_rx_mbuf`: free a mbuf for receive.
pub fn vio_free_rx_mbuf(sc: &VioSoftc, vioq: &VioQueue, i: usize) {
    bus_dmamap_unload(sc.virtio().dmat(), vioq.rxdmamap(i));
    m_freem(vioq.rxmbuf(i).get());
    vioq.rxmbuf(i).set(None);
}

/// `vio_populate_rx_mbufs`: add mbufs for all the empty receive slots.
pub fn vio_populate_rx_mbufs(sc: &VioSoftc, vioq: &VioQueue) {
    let vsc = sc.virtio();
    let mut done = false;
    let vq = vioq.rxvq();
    let no_split = virtio_has_feature(vsc, VIRTIO_NET_F_MRG_RXBUF)
        || virtio_has_feature(vsc, VIRTIO_F_VERSION_1)
        || virtio_has_feature(vsc, VIRTIO_F_ANY_LAYOUT);

    mutex_assert_locked(&vioq.viq_rxmtx, "vio_populate_rx_mbufs");
    let mut ring = vioq.viq_rxring.get();
    let mut slots = if_rxr_get(&mut ring, vq.vq_num.get());
    vioq.viq_rxring.set(ring);
    while slots > 0 {
        let slot = match virtio_enqueue_prep(vq) {
            Ok(slot) => slot,
            Err(Errno::EAGAIN) => break,
            Err(r) => panic(format_args!(
                "{}: enqueue_prep for rx buffer: {}",
                Str(&sc.sc_dev.dv_xname.get()),
                r as i32
            )),
        };
        let i = slot as usize;
        if vioq.rxmbuf(i).get().is_none() && vio_add_rx_mbuf(sc, vioq, i).is_err() {
            virtio_enqueue_abort(vq, slot);
            break;
        }
        let map = vioq.rxdmamap(i);
        if virtio_enqueue_reserve(vq, slot, map.dm_nsegs.get() + i32::from(!no_split)).is_err() {
            vio_free_rx_mbuf(sc, vioq, i);
            break;
        }
        bus_dmamap_sync(
            vsc.dmat(),
            map,
            0,
            map.dm_mapsize.get(),
            BUS_DMASYNC_PREREAD,
        );
        if no_split {
            virtio_enqueue(vq, slot, map, false);
        } else {
            // Legacy devices without VIRTIO_F_ANY_LAYOUT want a buffer of exactly the size
            // of the header in this case, so we have to split in two.
            let hdr = sc.sc_hdr_size.get() as BusSize;
            virtio_enqueue_p(vq, slot, map, 0, hdr, false);
            virtio_enqueue_p(
                vq,
                slot,
                map,
                hdr,
                (sc.sc_rx_mbuf_size.get() - sc.sc_rx_mbuf_offset.get()) as BusSize - hdr,
                false,
            );
        }
        virtio_enqueue_commit(vsc, vq, slot, false);
        done = true;
        slots -= 1;
    }
    let mut ring = vioq.viq_rxring.get();
    if_rxr_put(&mut ring, slots);
    vioq.viq_rxring.set(ring);

    if done {
        virtio_notify(vsc, vq);
    }
    timeout_add_sec(&sc.sc_rxtick, 1);
}

/// `vio_rx_offload`: the checksum and large-receive information the device gave.
pub fn vio_rx_offload(m: &Mbuf, hdr: &VirtioNetHdr) {
    let mut ext = EtherExtracted::new();
    let ph = m.m_pkthdr();

    if hdr.flags & VIRTIO_NET_HDR_F_DATA_VALID == 0 && hdr.flags & VIRTIO_NET_HDR_F_NEEDS_CSUM == 0
    {
        return;
    }

    ether_extract_headers(m, &mut ext);

    if !ext.tcp.is_null() {
        ph.csum_flags.set(ph.csum_flags.get() | M_TCP_CSUM_IN_OK);
        if hdr.flags & VIRTIO_NET_HDR_F_NEEDS_CSUM != 0 {
            ph.csum_flags.set(ph.csum_flags.get() | M_TCP_CSUM_OUT);
        }
    } else if !ext.udp.is_null() {
        ph.csum_flags.set(ph.csum_flags.get() | M_UDP_CSUM_IN_OK);
        if hdr.flags & VIRTIO_NET_HDR_F_NEEDS_CSUM != 0 {
            ph.csum_flags.set(ph.csum_flags.get() | M_UDP_CSUM_OUT);
        }
    }

    if hdr.gso_type == VIRTIO_NET_HDR_GSO_TCPV4 || hdr.gso_type == VIRTIO_NET_HDR_GSO_TCPV6 {
        let mss = u32::from(hdr.gso_size);

        if ext.tcp.is_null() || mss == 0 {
            tcpstat_inc(TcpstatCounters::TcpsInbadlro);
            return;
        }

        if ext.paylen.div_ceil(mss) <= 1 {
            return;
        }

        tcpstat_inc(TcpstatCounters::TcpsInhwlro);
        tcpstat_add(
            TcpstatCounters::TcpsInpktlro,
            u64::from(ext.paylen.div_ceil(mss)),
        );
        ph.csum_flags.set(ph.csum_flags.get() | M_TCP_TSO);
        ph.ph_mss.set(mss as u16);
    }
}

/// `vio_rxeof`: dequeue received packets.
pub fn vio_rxeof(vioq: &VioQueue) -> i32 {
    let sc = vioq.sc();
    let vsc = sc.virtio();
    let ifp = sc.ifp();
    let ml = MbufList::new();
    let mut m0: Option<&'static Mbuf> = None;
    let mut mlast: Option<&'static Mbuf> = None;
    let mut r = 0;
    let mut bufs_left = 0;
    let mut hdr = VirtioNetHdr::default();

    mutex_assert_locked(&vioq.viq_rxmtx, "vio_rxeof");
    while let Ok((slot, len)) = virtio_dequeue(vsc, vioq.rxvq()) {
        r = 1;
        let i = slot as usize;
        let map = vioq.rxdmamap(i);
        bus_dmamap_sync(
            vsc.dmat(),
            map,
            0,
            map.dm_mapsize.get(),
            BUS_DMASYNC_POSTREAD,
        );
        let m = vioq.rxmbuf(i).get();
        kassert!(m.is_some());
        let Some(m) = m else {
            continue;
        };
        bus_dmamap_unload(vsc.dmat(), map);
        vioq.rxmbuf(i).set(None);
        virtio_dequeue_commit(vioq.rxvq(), slot);
        let mut ring = vioq.viq_rxring.get();
        if_rxr_put(&mut ring, 1);
        vioq.viq_rxring.set(ring);
        m.m_len().set(len as u32);
        m.m_pkthdr().len.set(len);
        m.m_pkthdr().csum_flags.set(0);
        match (m0, mlast) {
            (Some(first), Some(last)) => {
                m.m_flags().set(m.m_flags().get() & !M_PKTHDR);
                first
                    .m_pkthdr()
                    .len
                    .set(first.m_pkthdr().len.get() + m.m_len().get() as i32);
                last.m_next().set(Some(m));
                mlast = Some(m);
                bufs_left -= 1;
            }
            _ => {
                let n = (sc.sc_hdr_size.get() as usize).min(m.m_len().get() as usize);
                // SAFETY: the device wrote `len` bytes at the buffer's start, the header
                // first; the bytes are read before m_adj trims them.
                hdr = VirtioNetHdr::from_bytes(unsafe {
                    core::slice::from_raw_parts(mtod::<u8>(m), n)
                });
                m_adj(m, sc.sc_hdr_size.get());
                m0 = Some(m);
                mlast = Some(m);
                if virtio_has_feature(vsc, VIRTIO_NET_F_MQ) {
                    m.m_pkthdr()
                        .ph_flowid
                        .set(vioq.ifiq().ifiq_idx.get() as u16);
                    m.m_pkthdr()
                        .csum_flags
                        .set(m.m_pkthdr().csum_flags.get() | M_FLOWID);
                }
                if virtio_has_feature(vsc, VIRTIO_NET_F_MRG_RXBUF) {
                    bufs_left = i32::from(hdr.num_buffers) - 1;
                } else {
                    bufs_left = 0;
                }
            }
        }

        if bufs_left == 0
            && let Some(first) = m0
        {
            if virtio_has_feature(vsc, VIRTIO_NET_F_GUEST_CSUM) {
                vio_rx_offload(first, &hdr);
            }
            ml_enqueue(&ml, first);
            m0 = None;
            mlast = None;
        }
    }
    if let Some(first) = m0 {
        if VIRTIO_DEBUG > 0 {
            printf(format_args!(
                "vio_rxeof: expected {} buffers, got {}\n",
                hdr.num_buffers,
                i32::from(hdr.num_buffers) - bufs_left
            ));
        }
        ifp.if_ierrors().set(ifp.if_ierrors().get() + 1);
        m_freem(first);
    }

    if ifiq_input(vioq.ifiq(), &ml) {
        let mut ring = vioq.viq_rxring.get();
        if_rxr_livelocked(&mut ring);
        vioq.viq_rxring.set(ring);
    }

    r
}

/// `vio_rx_intr`: the rx queue's `vq_done`.
pub fn vio_rx_intr(vq: &Virtqueue) -> i32 {
    let vsc = vq.owner();
    let sc = vio_child(vsc);
    let vioq = vio_vq2q(sc, vq);
    let mut sum = 0;

    mtx_enter(&vioq.viq_rxmtx);
    // again:
    loop {
        let r = vio_rxeof(vioq);
        sum += r;
        if r != 0 {
            vio_populate_rx_mbufs(sc, vioq);
            // set used event index to the next slot
            if virtio_has_feature(vsc, VIRTIO_F_RING_EVENT_IDX) && virtio_start_vq_intr(vsc, vq) {
                continue;
            }
        }
        break;
    }

    mtx_leave(&vioq.viq_rxmtx);
    sum
}

/// `vio_rxtick`: check the rx queues and refill them, once a second.
pub fn vio_rxtick(arg: *mut c_void) {
    // SAFETY: the timeout was set with the softc as its argument (vio_attach).
    let sc = unsafe { &*arg.cast::<VioSoftc>() };

    for i in 0..usize::from(sc.sc_nqueues.get()) {
        virtio_check_vq(sc.virtio(), sc.q(i).rxvq());
        mtx_enter(&sc.q(i).viq_rxmtx);
        vio_populate_rx_mbufs(sc, sc.q(i));
        mtx_leave(&sc.q(i).viq_rxmtx);
    }
}

/// `vio_rx_drain`: free all the mbufs; called from if_stop(disable).
pub fn vio_rx_drain(sc: &VioSoftc) {
    for qidx in 0..usize::from(sc.sc_nqueues.get()) {
        let vioq = sc.q(qidx);
        for i in 0..vioq.rxvq().vq_num.get() as usize {
            if vioq.rxmbuf(i).get().is_none() {
                continue;
            }
            vio_free_rx_mbuf(sc, vioq, i);
        }
    }
}

// Transmission implementation: actual transmission is done in if_start.

/// `vio_tx_intr`: tx interrupt; dequeue and free mbufs.
///
/// tx interrupt is actually disabled unless the tx queue is full, i.e. `IFF_OACTIVE` is set.
/// `vio_txtick` is used to make sure that mbufs are dequeued and freed even if no further
/// transfer happens.
pub fn vio_tx_intr(vq: &Virtqueue) -> i32 {
    let vsc = vq.owner();
    let sc = vio_child(vsc);
    let vioq = vio_vq2q(sc, vq);

    let r = vio_txeof(vq);
    vio_start(vioq.ifq());
    r
}

/// `vio_txtick`.
pub fn vio_txtick(arg: *mut c_void) {
    // SAFETY: the timeout was set with the softc as its argument (vio_attach).
    let sc = unsafe { &*arg.cast::<VioSoftc>() };

    for i in 0..usize::from(sc.sc_nqueues.get()) {
        virtio_check_vq(sc.virtio(), sc.q(i).txvq());
    }
}

/// `vio_tx_dequeue`: reclaim the sent frames; returns how many.
pub fn vio_tx_dequeue(vq: &Virtqueue) -> i32 {
    let vsc = vq.owner();
    let sc = vio_child(vsc);
    let vioq = vio_vq2q(sc, vq);
    let mut r = 0;
    let mut freed = 0;

    mutex_assert_locked(&vioq.viq_txmtx, "vio_tx_dequeue");

    while let Ok((slot, _len)) = virtio_dequeue(vsc, vq) {
        let i = slot as usize;
        let hdr = vioq.txhdr(i);
        r += 1;
        vio_dmamem_sync(
            vsc,
            sc,
            hdr.cast(),
            sc.sc_hdr_size.get() as usize,
            BUS_DMASYNC_POSTWRITE,
        );
        let map = vioq.txdmamap(i);
        bus_dmamap_sync(
            vsc.dmat(),
            map,
            0,
            map.dm_mapsize.get(),
            BUS_DMASYNC_POSTWRITE,
        );
        let m = vioq.txmbuf(i).get();
        bus_dmamap_unload(vsc.dmat(), map);
        vioq.txmbuf(i).set(None);
        freed += virtio_dequeue_commit(vq, slot);
        m_freem(m);
    }
    kassert!(vioq.viq_txfree_slots.get() >= 0);
    vioq.viq_txfree_slots
        .set(vioq.viq_txfree_slots.get() + freed);
    r
}

/// `vio_txeof`.
pub fn vio_txeof(vq: &Virtqueue) -> i32 {
    let vsc = vq.owner();
    let sc = vio_child(vsc);
    let vioq = vio_vq2q(sc, vq);

    mtx_enter(&vioq.viq_txmtx);
    let r = vio_tx_dequeue(vq);
    mtx_leave(&vioq.viq_txmtx);

    if r != 0 && ifq_is_oactive(vioq.ifq()) {
        mtx_enter(&vioq.viq_txmtx);
        virtio_stop_vq_intr(vsc, vq);
        mtx_leave(&vioq.viq_txmtx);
        ifq_restart(vioq.ifq());
    }
    if vq.vq_used_idx.get() == vq.vq_avail_idx.get() {
        timeout_del(&sc.sc_txtick);
    } else if r != 0 {
        timeout_add_sec(&sc.sc_txtick, 1);
    }
    r
}

/// `vio_encap`: load `m` into the slot's dmamap, defragmenting it once if it has too many
/// segments.
pub fn vio_encap(vioq: &VioQueue, slot: i32, m: &'static Mbuf) -> Result<(), Errno> {
    let vsc = vioq.sc().virtio();
    let dmap = vioq.txdmamap(slot as usize);

    // SAFETY: the mbuf stays in `viq_txmbufs[slot]` until vio_tx_dequeue or vio_tx_drain
    // unload the map, before freeing it.
    let load =
        || unsafe { bus_dmamap_load_mbuf(vsc.dmat(), dmap, m, BUS_DMA_WRITE | BUS_DMA_NOWAIT) };
    match load() {
        Ok(()) => {}
        Err(Errno::EFBIG) => {
            if m_defrag(m, M_DONTWAIT).is_err() || load().is_err() {
                return Err(Errno::ENOBUFS);
            }
        }
        Err(_) => return Err(Errno::ENOBUFS),
    }
    vioq.txmbuf(slot as usize).set(Some(m));
    Ok(())
}

/// `vio_tx_drain`: free all the mbufs already put on vq; called from if_stop(disable).
pub fn vio_tx_drain(sc: &VioSoftc) {
    let vsc = sc.virtio();

    for q in 0..usize::from(sc.sc_nqueues.get()) {
        let vioq = sc.q(q);
        ifq_barrier(vioq.ifq());
        mtx_enter(&vioq.viq_txmtx);
        for i in 0..vioq.txvq().vq_num.get() as usize {
            let Some(m) = vioq.txmbuf(i).get() else {
                continue;
            };
            bus_dmamap_unload(vsc.dmat(), vioq.txdmamap(i));
            m_freem(m);
            vioq.txmbuf(i).set(None);
        }
        ifq_purge(vioq.ifq());
        ifq_clr_oactive(vioq.ifq());
        vioq.viq_txfree_slots
            .set(vioq.txvq().vq_num.get() as i32 - 1);
        mtx_leave(&vioq.viq_txmtx);
    }
}

// Control vq

/// `vio_ctrl_start`: lock the control queue and the `sc_ctrl_*` structs and prepare a
/// request; returns its slot.
///
/// If this function succeeds, the caller must also call either `vio_ctrl_submit()` or
/// `virtio_enqueue_abort()`, in both cases followed by `vio_ctrl_finish()`.
pub fn vio_ctrl_start(sc: &VioSoftc, class: u8, cmd: u8, nslots: i32) -> Result<i32, Errno> {
    let vsc = sc.virtio();
    let vq = sc.ctl_vq();

    splassert(IPL_NET, "vio_ctrl_start");

    while sc.sc_ctrl_inuse.get() != FREE {
        if sc.sc_ctrl_inuse.get() == RESET || vio_needs_reset(sc) {
            return Err(Errno::ENXIO);
        }
        tsleep_nsec(ptr::from_ref(&sc.sc_ctrl_inuse), PRIBIO, "viowait", INFSLP)?;
    }
    sc.sc_ctrl_inuse.set(INUSE);

    let c = sc.sc_ctrl_cmd.get();
    // SAFETY: the command structure in the DMA area, which the device does not read before
    // the request is submitted.
    unsafe {
        c.write(VirtioNetCtrlCmd {
            class,
            command: cmd,
        })
    };

    let slot = match virtio_enqueue_prep(vq) {
        Ok(slot) => slot,
        Err(_) => panic(format_args!(
            "{}: vio_ctrl_start virtio_enqueue_prep: control vq busy",
            Str(&sc.sc_dev.dv_xname.get())
        )),
    };
    if virtio_enqueue_reserve(vq, slot, nslots + 2).is_err() {
        panic(format_args!(
            "{}: vio_ctrl_start virtio_enqueue_reserve: control vq busy",
            Str(&sc.sc_dev.dv_xname.get())
        ));
    }

    vio_dmamem_enqueue(
        vsc,
        sc,
        vq,
        slot,
        c.cast(),
        size_of::<VirtioNetCtrlCmd>(),
        true,
    );

    Ok(slot)
}

/// `vio_ctrl_submit`: submit a control queue request and wait for the result.
///
/// `vio_ctrl_start()` must have been called successfully. After `vio_ctrl_submit()`, the
/// caller may inspect the data returned from the hypervisor. Afterwards, the caller must
/// always call `vio_ctrl_finish()`.
pub fn vio_ctrl_submit(sc: &VioSoftc, slot: i32) -> Result<(), Errno> {
    let vsc = sc.virtio();
    let vq = sc.ctl_vq();
    let status = sc.sc_ctrl_status.get();

    vio_dmamem_enqueue(
        vsc,
        sc,
        vq,
        slot,
        status.cast(),
        size_of::<VirtioNetCtrlStatus>(),
        false,
    );

    virtio_enqueue_commit(vsc, vq, slot, true);

    while sc.sc_ctrl_inuse.get() != DONE {
        if sc.sc_ctrl_inuse.get() == RESET || vio_needs_reset(sc) {
            return Err(Errno::ENXIO);
        }
        if let Err(r) = tsleep_nsec(
            ptr::from_ref(&sc.sc_ctrl_inuse),
            PRIBIO,
            "viodone",
            VIRTIO_NET_CTRL_TIMEOUT,
        ) {
            if r == Errno::EWOULDBLOCK {
                printf(format_args!(
                    "{}: ctrl queue timeout\n",
                    Str(&sc.sc_dev.dv_xname.get())
                ));
            }
            vio_ctrl_wakeup(sc, RESET);
            return Err(Errno::ENXIO);
        }
        if COLD.load(core::sync::atomic::Ordering::Relaxed) {
            virtio_check_vq(vsc, sc.ctl_vq());
        }
    }

    vio_dmamem_sync(
        vsc,
        sc,
        sc.sc_ctrl_cmd.get().cast(),
        size_of::<VirtioNetCtrlCmd>(),
        BUS_DMASYNC_POSTWRITE,
    );
    vio_dmamem_sync(
        vsc,
        sc,
        status.cast(),
        size_of::<VirtioNetCtrlStatus>(),
        BUS_DMASYNC_POSTREAD,
    );

    // SAFETY: the status byte in the DMA area, which the device wrote; read volatile.
    let ack = unsafe { ptr::read_volatile(&raw const (*status).ack) };
    if ack != VIRTIO_NET_OK {
        return Err(Errno::EIO);
    }

    Ok(())
}

/// `vio_ctrl_finish`: unlock the control queue and the `sc_ctrl_*` structs.
///
/// It is ok to call this function if the control queue is marked dead due to a fatal error.
pub fn vio_ctrl_finish(sc: &VioSoftc) {
    if sc.sc_ctrl_inuse.get() == RESET {
        return;
    }

    vio_ctrl_wakeup(sc, FREE);
}

/// `vio_ctrl_rx`: issue a `VIRTIO_NET_CTRL_RX` class command and wait for completion.
pub fn vio_ctrl_rx(sc: &VioSoftc, cmd: u8, onoff: bool) -> Result<(), Errno> {
    let vsc = sc.virtio();
    let vq = sc.ctl_vq();

    let slot = vio_ctrl_start(sc, VIRTIO_NET_CTRL_RX, cmd, 1)?;

    let rx = sc.sc_ctrl_rx.get();
    // SAFETY: the rx structure in the DMA area, ours while the control queue is INUSE.
    unsafe {
        rx.write(VirtioNetCtrlRx {
            onoff: u8::from(onoff),
        })
    };

    vio_dmamem_enqueue(
        vsc,
        sc,
        vq,
        slot,
        rx.cast(),
        size_of::<VirtioNetCtrlRx>(),
        true,
    );

    let r = vio_ctrl_submit(sc, slot);
    vio_dmamem_sync(
        vsc,
        sc,
        rx.cast(),
        size_of::<VirtioNetCtrlRx>(),
        BUS_DMASYNC_POSTWRITE,
    );
    if r.is_err() {
        printf(format_args!(
            "{}: ctrl cmd {cmd} failed\n",
            Str(&sc.sc_dev.dv_xname.get())
        ));
    }

    if VIRTIO_DEBUG > 0 {
        printf(format_args!(
            "vio_ctrl_rx: cmd {cmd} {}: {}\n",
            i32::from(onoff),
            r.map_or_else(|e| e as i32, |()| 0)
        ));
    }

    vio_ctrl_finish(sc);
    r
}

/// `vio_ctrl_mq`: issue a `VIRTIO_NET_CTRL_MQ` class command and wait for completion.
pub fn vio_ctrl_mq(sc: &VioSoftc) -> Result<(), Errno> {
    let vsc = sc.virtio();
    let vq = sc.ctl_vq();

    let slot = vio_ctrl_start(sc, VIRTIO_NET_CTRL_MQ, VIRTIO_NET_CTRL_MQ_VQ_PAIRS_SET, 1)?;

    let mq = sc.sc_ctrl_mq_pairs.get();
    // SAFETY: the structure in the DMA area, ours while the control queue is INUSE; the
    // area is only byte aligned here.
    unsafe {
        mq.write_unaligned(VirtioNetCtrlMqPairsSet {
            virtqueue_pairs: sc.sc_nqueues.get(),
        })
    };

    vio_dmamem_enqueue(
        vsc,
        sc,
        vq,
        slot,
        mq.cast(),
        size_of::<VirtioNetCtrlMqPairsSet>(),
        true,
    );

    let r = vio_ctrl_submit(sc, slot);

    vio_dmamem_sync(
        vsc,
        sc,
        mq.cast(),
        size_of::<VirtioNetCtrlMqPairsSet>(),
        BUS_DMASYNC_POSTWRITE,
    );

    if r.is_err() {
        printf(format_args!(
            "{}: ctrl cmd {VIRTIO_NET_CTRL_MQ_VQ_PAIRS_SET} failed\n",
            Str(&sc.sc_dev.dv_xname.get())
        ));
    }

    if VIRTIO_DEBUG > 0 {
        printf(format_args!(
            "vio_ctrl_mq: cmd {VIRTIO_NET_CTRL_MQ_VQ_PAIRS_SET} {}: {}\n",
            sc.sc_nqueues.get(),
            r.map_or_else(|e| e as i32, |()| 0)
        ));
    }

    vio_ctrl_finish(sc);
    r
}

/// `vio_ctrl_guest_offloads`.
pub fn vio_ctrl_guest_offloads(sc: &VioSoftc, features: u64) -> Result<(), Errno> {
    let vsc = sc.virtio();
    let vq = sc.ctl_vq();

    let slot = vio_ctrl_start(
        sc,
        VIRTIO_NET_CTRL_GUEST_OFFLOADS,
        VIRTIO_NET_CTRL_GUEST_OFFLOADS_SET,
        1,
    )?;

    let go = sc.sc_ctrl_guest_offloads.get();
    // SAFETY: the `__packed` structure in the DMA area, ours while the control queue is
    // INUSE (byte aligned, as its type is).
    unsafe { go.write(VirtioNetCtrlGuestOffloads { offloads: features }) };

    vio_dmamem_enqueue(
        vsc,
        sc,
        vq,
        slot,
        go.cast(),
        size_of::<VirtioNetCtrlGuestOffloads>(),
        true,
    );

    let r = vio_ctrl_submit(sc, slot);

    vio_dmamem_sync(
        vsc,
        sc,
        go.cast(),
        size_of::<VirtioNetCtrlGuestOffloads>(),
        BUS_DMASYNC_POSTWRITE,
    );

    if r.is_err() && features != 0 {
        printf(format_args!(
            "{}: offload features 0x{features:x} failed\n",
            Str(&sc.sc_dev.dv_xname.get())
        ));
    }

    if VIRTIO_DEBUG > 0 {
        printf(format_args!(
            "vio_ctrl_guest_offloads: offload features 0x{features:x}: {}\n",
            r.map_or_else(|e| e as i32, |()| 0)
        ));
    }

    vio_ctrl_finish(sc);
    r
}

/// `vio_ctrl_wakeup`.
pub fn vio_ctrl_wakeup(sc: &VioSoftc, new: VioCtrlState) {
    sc.sc_ctrl_inuse.set(new);
    wakeup(ptr::from_ref(&sc.sc_ctrl_inuse));
}

/// `vio_ctrleof`: the control queue's `vq_done`.
pub fn vio_ctrleof(vq: &Virtqueue) -> i32 {
    let vsc = vq.owner();
    let sc = vio_child(vsc);
    let mut r = 0;

    // KERNEL_LOCK(): nothing without MULTIPROCESSOR.
    let s = splnet();
    // again:
    while let Ok((slot, _)) = virtio_dequeue(vsc, vq) {
        virtio_dequeue_commit(vq, slot);
        r += 1;
        vio_ctrl_wakeup(sc, DONE);
        if !virtio_start_vq_intr(vsc, vq) {
            break;
        }
    }

    // out:
    splx(s);
    // KERNEL_UNLOCK()
    r
}

/// `nentries` of a MAC table in the DMA area.
fn mac_tbl_nentries(t: *mut VirtioNetCtrlMacTbl) -> u32 {
    // SAFETY: one of the two tables in the DMA area (`__packed`: read unaligned).
    unsafe { (&raw const (*t).nentries).read_unaligned() }
}

/// `t->nentries = n`.
fn set_mac_tbl_nentries(t: *mut VirtioNetCtrlMacTbl, n: u32) {
    // SAFETY: as for `mac_tbl_nentries`.
    unsafe { (&raw mut (*t).nentries).write_unaligned(n) }
}

/// `memcpy(t->macs[i], addr, ETHER_ADDR_LEN)`, `i` below the table's room.
fn set_mac_tbl_entry(t: *mut VirtioNetCtrlMacTbl, room: usize, i: usize, addr: &[u8; 6]) {
    if i >= room {
        panic(format_args!("vio: MAC table entry {i} past {room}"));
    }
    // SAFETY: vio_alloc_mem left room for `room` addresses after the table's header.
    unsafe {
        ptr::copy_nonoverlapping(
            addr.as_ptr(),
            t.cast::<u8>()
                .add(size_of::<VirtioNetCtrlMacTbl>() + i * ETHER_ADDR_LEN),
            ETHER_ADDR_LEN,
        )
    };
}

/// `vio_set_rx_filter`: issue `VIRTIO_NET_CTRL_MAC_TABLE_SET` command and wait for
/// completion; the filter is already set in `sc_ctrl_mac_tbl`.
pub fn vio_set_rx_filter(sc: &VioSoftc) -> Result<(), Errno> {
    let vsc = sc.virtio();
    let vq = sc.ctl_vq();
    let uc = sc.sc_ctrl_mac_tbl_uc.get();
    let mc = sc.sc_ctrl_mac_tbl_mc.get();

    let slot = vio_ctrl_start(sc, VIRTIO_NET_CTRL_MAC, VIRTIO_NET_CTRL_MAC_TABLE_SET, 2)?;

    let len_uc = size_of::<VirtioNetCtrlMacTbl>() + mac_tbl_nentries(uc) as usize * ETHER_ADDR_LEN;
    let len_mc = size_of::<VirtioNetCtrlMacTbl>() + mac_tbl_nentries(mc) as usize * ETHER_ADDR_LEN;
    vio_dmamem_enqueue(vsc, sc, vq, slot, uc.cast(), len_uc, true);
    vio_dmamem_enqueue(vsc, sc, vq, slot, mc.cast(), len_mc, true);

    let r = vio_ctrl_submit(sc, slot);
    vio_dmamem_sync(vsc, sc, uc.cast(), len_uc, BUS_DMASYNC_POSTWRITE);
    vio_dmamem_sync(vsc, sc, mc.cast(), len_mc, BUS_DMASYNC_POSTWRITE);

    if r.is_err() {
        // The host's filter table is not large enough
        printf(format_args!(
            "{}: failed setting rx filter\n",
            Str(&sc.sc_dev.dv_xname.get())
        ));
    }

    vio_ctrl_finish(sc);
    r
}

/// `vio_iff`: program the receive filter: unicast address, multicast list, all-multicast
/// and promiscuous switches.
pub fn vio_iff(sc: &'static VioSoftc) {
    let vsc = sc.virtio();
    let ifp = sc.ifp();
    let ac = &sc.sc_ac;
    let mut nentries = 0;
    let mut promisc = false;
    let mut allmulti = false;
    let mut rxfilter = false;

    splassert(IPL_NET, "vio_iff");

    ifp.if_flags.set(ifp.if_flags.get() & !IFF_ALLMULTI);

    if !virtio_has_feature(vsc, VIRTIO_NET_F_CTRL_RX) {
        // no ctrl vq; always promisc
        ifp.if_flags
            .set(ifp.if_flags.get() | IFF_ALLMULTI | IFF_PROMISC);
        return;
    }

    if sc.sc_dev.cfdata().cf_flags & CONFFLAG_QEMU_VLAN_BUG != 0 {
        ifp.if_flags.set(ifp.if_flags.get() | IFF_PROMISC);
    }

    let mc = sc.sc_ctrl_mac_tbl_mc.get();
    let uc = sc.sc_ctrl_mac_tbl_uc.get();
    if ifp.if_flags.get() & IFF_PROMISC != 0
        || ac.ac_multirangecnt.get() > 0
        || ac.ac_multicnt.get() as usize >= VIRTIO_NET_CTRL_MAC_MC_ENTRIES
    {
        ifp.if_flags.set(ifp.if_flags.get() | IFF_ALLMULTI);
        if ifp.if_flags.get() & IFF_PROMISC != 0 {
            promisc = true;
        } else {
            allmulti = true;
        }
    } else {
        rxfilter = true;

        let mut step = EtherMultistep { e_enm: None };
        let mut enm = ether_first_multi(&mut step, ac);
        while let Some(e) = enm {
            set_mac_tbl_entry(mc, VIRTIO_NET_CTRL_MAC_MC_ENTRIES, nentries, &e.enm_addrlo);
            nentries += 1;

            enm = ether_next_multi(&mut step);
        }
    }

    // set unicast address, VirtualBox wants that
    set_mac_tbl_entry(uc, VIRTIO_NET_CTRL_MAC_UC_ENTRIES, 0, &ac.ac_enaddr.get());
    set_mac_tbl_nentries(uc, 1);

    set_mac_tbl_nentries(mc, if rxfilter { nentries as u32 } else { 0 });

    match vio_set_rx_filter(sc) {
        Err(Errno::EIO) => allmulti = true, // fallback
        Err(_) => return,
        Ok(()) => {}
    }

    match vio_ctrl_rx(sc, VIRTIO_NET_CTRL_RX_ALLMULTI, allmulti) {
        Err(Errno::EIO) => promisc = true, // fallback
        Err(_) => return,
        Ok(()) => {}
    }

    let _ = vio_ctrl_rx(sc, VIRTIO_NET_CTRL_RX_PROMISC, promisc);
}

const _: () = {
    assert!(size_of::<VirtioNetHdr>() == 12);
    assert!(offset_of!(VirtioNetHdr, num_buffers) == 10);
    assert!(size_of::<VirtioNetCtrlGuestOffloads>() == 8);
    assert!(align_of::<VirtioNetCtrlGuestOffloads>() == 1);
    assert!(size_of::<VirtioNetCtrlMacTbl>() == 4);
    assert!(VIO_CTRL_MAC_INFO_SIZE == 398);
};

#[cfg(test)]
mod tests;
